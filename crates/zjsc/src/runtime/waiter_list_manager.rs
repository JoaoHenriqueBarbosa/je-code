//! Porte de `runtime/WaiterListManager.{h,cpp}` na parte que uma thread só alcança: as esperas assíncronas
//! de `Atomics.waitAsync` e o `Atomics.notify` que as acorda.
//!
//! O C++ guarda uma `WaiterList` por endereço do dado (`WaiterListManager::findList`), com os `Waiter`s em
//! ordem de chegada; `notifyWaiter` acorda os `count` primeiros e a promessa deles resolve com `"ok"`; o
//! prazo vence pelo `DeferredWorkTimer`, que resolve a promessa com `"timed-out"`.
//!
//! DIVERGÊNCIAS:
//! - O porte não tem laço de eventos nem `DeferredWorkTimer`. Os prazos correm num RELÓGIO VIRTUAL deste
//!   módulo (milissegundos, começa em 0, só avança quando alguém o avança): `run_next_timer` leva o relógio
//!   ao menor prazo pendente e resolve aquela promessa com `"timed-out"`. Quem dirige o motor (o modo de
//!   teste de `api/eval.rs`, `drain_virtual_timers`) chama isso quando só restam timers. O bun mede que o
//!   prazo de um `waitAsync` NÃO mantém o processo vivo (um script que só tem `waitAsync(..., 10)` sai sem
//!   imprimir o `then`), por isso `evaluate_script` não avança o relógio sozinho.
//! - As listas moram numa `thread_local!` (o motor é de uma thread; o endereço é a identidade do
//!   `ArrayBuffer` mais o deslocamento em bytes), não no `VM`: um `SharedArrayBuffer` compartilhado entre
//!   dois `VM` da mesma thread enxerga a mesma lista, como no C++ (a lista é do endereço, não do `VM`).
//! - A promessa é resolvida pelo `JSGlobalObject` de quem chama (`notify` ou o driver de timers), não pelo
//!   do `waitAsync` que a criou.

use std::cell::RefCell;

use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_promise::JSPromiseRef;
use crate::runtime::js_string::js_string;
use crate::runtime::js_value::JSValue;
use crate::wtf::text::wtf_string::String as WtfString;

/// Um `WaiterListManager::Waiter` assíncrono: a promessa e o prazo (no relógio virtual; `None` é sem prazo).
struct Waiter {
    /// A identidade do dado: o `ArrayBuffer` e o deslocamento em bytes do elemento.
    key: (usize, usize),
    promise: JSPromiseRef,
    deadline: Option<f64>,
}

#[derive(Default)]
struct WaiterLists {
    /// `m_waiterListsLock`/`m_waiterLists`: todos os waiters, na ordem em que esperaram.
    waiters: Vec<Waiter>,
    /// O relógio virtual, em milissegundos.
    now: f64,
}

thread_local! {
    static LISTS: RefCell<WaiterLists> = RefCell::new(WaiterLists::default());
}

/// Fim do programa (`cell_registry::reset_program_state`): os waiters guardam promessas do programa.
pub(crate) fn reset_for_program() {
    let taken = LISTS.try_with(|lists| std::mem::take(&mut *lists.borrow_mut()));
    drop(taken);
}

fn resolve_with(global_object: &JSGlobalObject, promise: &JSPromiseRef, text: &[u8]) {
    let value = JSValue::from_js_string(js_string(global_object.vm(), &WtfString::from_latin1(text)));
    promise.resolve(global_object, value);
}

/// `WaiterListManager::waitAsync`, depois das conferências de valor e de prazo zero (que ficam no chamador):
/// põe o waiter no fim da lista de `key`. O prazo é relativo ao relógio virtual atual.
pub fn add_async_waiter(key: (usize, usize), promise: JSPromiseRef, timeout_milliseconds: Option<f64>) {
    LISTS.with(|lists| {
        let mut lists = lists.borrow_mut();
        let deadline = timeout_milliseconds.filter(|milliseconds| milliseconds.is_finite()).map(|milliseconds| lists.now + milliseconds);
        lists.waiters.push(Waiter { key, promise, deadline });
    });
}

/// `WaiterListManager::notifyWaiter(vm, valueWait, count)`: acorda os `count` primeiros waiters de `key`
/// (resolvendo a promessa com `"ok"`) e devolve quantos acordou.
pub fn notify_waiters(global_object: &JSGlobalObject, key: (usize, usize), count: f64) -> usize {
    let woken: Vec<Waiter> = LISTS.with(|lists| {
        let mut lists = lists.borrow_mut();
        let mut taken = Vec::new();
        let mut index = 0;
        while index < lists.waiters.len() && (taken.len() as f64) < count {
            if lists.waiters[index].key == key {
                taken.push(lists.waiters.remove(index));
            } else {
                index += 1;
            }
        }
        taken
    });
    for waiter in &woken {
        resolve_with(global_object, &waiter.promise, b"ok");
    }
    woken.len()
}

/// Há algum waiter com prazo pendente?
pub fn has_pending_timers() -> bool {
    LISTS.with(|lists| lists.borrow().waiters.iter().any(|waiter| waiter.deadline.is_some()))
}

/// O relógio virtual em milissegundos. É o mesmo relógio dos timers do host (`timers.rs`): os dois tipos de
/// prazo vencem em ordem de tempo virtual. Com o tempo de CPU ligado, o trabalho feito desde a última leitura entra
/// antes (`cpu_clock`).
pub fn virtual_now() -> f64 {
    let work = crate::runtime::cpu_clock::take_pending_ms();
    LISTS.with(|lists| {
        let mut lists = lists.borrow_mut();
        lists.now += work;
        lists.now
    })
}

/// Leva o relógio virtual a `time` se ele ainda estiver antes (o relógio nunca anda para trás). O trabalho de CPU
/// ainda não cobrado aconteceu antes do salto e entra primeiro.
pub fn advance_virtual_clock_to(time: f64) {
    let work = crate::runtime::cpu_clock::take_pending_ms();
    LISTS.with(|lists| {
        let mut lists = lists.borrow_mut();
        lists.now = (lists.now + work).max(time);
    });
}

/// O menor prazo pendente de `Atomics.waitAsync` (no relógio virtual), se houver.
pub fn next_deadline() -> Option<f64> {
    LISTS.with(|lists| lists.borrow().waiters.iter().filter_map(|waiter| waiter.deadline).reduce(f64::min))
}

/// O que o `DeferredWorkTimer` faria ao vencer o prazo: leva o relógio virtual ao menor prazo pendente (o
/// mais antigo ganha o empate) e resolve aquela promessa com `"timed-out"`. `false` sem prazo pendente.
pub fn run_next_timer(global_object: &JSGlobalObject) -> bool {
    let expired = LISTS.with(|lists| {
        let mut lists = lists.borrow_mut();
        let mut best: Option<(usize, f64)> = None;
        for (index, waiter) in lists.waiters.iter().enumerate() {
            if let Some(deadline) = waiter.deadline {
                if best.map_or(true, |(_, best_deadline)| deadline < best_deadline) {
                    best = Some((index, deadline));
                }
            }
        }
        let (index, deadline) = best?;
        lists.now = lists.now.max(deadline);
        Some(lists.waiters.remove(index))
    });
    match expired {
        Some(waiter) => {
            resolve_with(global_object, &waiter.promise, b"timed-out");
            true
        }
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_timers_when_empty() {
        assert!(!has_pending_timers());
    }
}
