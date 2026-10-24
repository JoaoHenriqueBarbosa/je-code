//! A rede do `Bun.serve` e do `fetch` em loopback: os descritores TCP do [`Host`] (`net_*`) como funções nativas do
//! `bun_http.js`, e os observadores de descritor que o laço de eventos consulta. Cada observador guarda o callback
//! JavaScript, os eventos esperados (leitura, escrita) e se segura o processo (`ref`); o laço chama [`dispatch`] a cada
//! volta e, sem mais nada a fazer, espera aqui o próximo evento de rede.
//!
//! As funções nativas devolvem o resultado ou o errno negativo (`-11` é `EAGAIN`): o JavaScript decide o que fazer.

use std::cell::{Cell, RefCell};
use std::time::Duration;

use crate::host_function;
use crate::runtime::host::{host, NetReady, NetWatch};
use crate::runtime::host_call::{HostCall, HostResult};
use crate::runtime::js_array::construct_array;
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_value::{js_number, JSValue};
use crate::runtime::native_function::NativeFunction;
use crate::runtime::text_decoder::input_bytes;
use crate::runtime::uint8_array_base64::create_uint8_array;

struct Watch {
    fd: i32,
    read: bool,
    write: bool,
    callback: JSValue,
    refed: bool,
}

/// Um filho que o laço espera terminar (`procWatch`): o callback recebe `[código, sinal]` (um deles `-1`).
struct Child {
    pid: i32,
    callback: JSValue,
    refed: bool,
}

thread_local! {
    static CHILDREN: RefCell<Vec<Child>> = const { RefCell::new(Vec::new()) };
    /// O próximo descritor de mentira das conclusões imediatas (`netDefer`): negativos, que nenhum descritor de verdade é.
    static NEXT_DEFERRED: Cell<i32> = const { Cell::new(-2) };
    static WATCHES: RefCell<Vec<Watch>> = const { RefCell::new(Vec::new()) };
}

/// Fim do programa: os observadores guardam callbacks do programa.
pub(crate) fn reset_for_program() {
    let _ = WATCHES.try_with(|watches| watches.borrow_mut().clear());
    let _ = CHILDREN.try_with(|children| children.borrow_mut().clear());
}

/// Os callbacks dos observadores são raízes do coletor.
pub(crate) fn visit_gc_roots(marker: &mut crate::runtime::gc::Marker) {
    let _ = WATCHES.try_with(|watches| {
        let Ok(watches) = watches.try_borrow() else {
            marker.abort();
            return;
        };
        for watch in watches.iter() {
            marker.value(watch.callback);
        }
    });
    let _ = CHILDREN.try_with(|children| {
        let Ok(children) = children.try_borrow() else {
            marker.abort();
            return;
        };
        for child in children.iter() {
            marker.value(child.callback);
        }
    });
}

/// `true` quando algum observador com `ref` segura o processo.
pub(crate) fn holds_event_loop() -> bool {
    WATCHES.with(|watches| watches.borrow().iter().any(|watch| watch.refed)) || CHILDREN.with(|children| children.borrow().iter().any(|child| child.refed))
}

/// O prazo de espera da volta: o kernel não avisa o fim de um filho, então com filhos esperados o laço olha de
/// milissegundo em milissegundo (um filho que escreve ou fecha o pipe acorda o `poll(2)` antes disso).
const CHILD_POLL: Duration = Duration::from_millis(1);

/// Os filhos que terminaram nesta volta: o callback de cada um recebe `[código, sinal]`. `true` se algum rodou.
fn reap_children(global_object: &JSGlobalObject) -> bool {
    let pids: Vec<(i32, JSValue)> = CHILDREN.with(|children| children.borrow().iter().map(|child| (child.pid, child.callback)).collect());
    let mut ran = false;
    for (pid, callback) in pids {
        let exit = match host().proc_wait(pid, true) {
            Ok(Some(exit)) => exit,
            Ok(None) => continue,
            // O filho já foi recolhido por outro (ou nunca foi nosso): termina como sem estado.
            Err(_) => crate::runtime::host::ProcExit::Exited(0),
        };
        CHILDREN.with(|children| children.borrow_mut().retain(|child| child.pid != pid));
        ran = true;
        crate::runtime::timers::call_from_loop(global_object, callback, &[crate::runtime::proc_spawn::exit_value(global_object, exit)]);
    }
    ran
}

/// Uma volta de `poll(2)` sobre os observadores, esperando até `timeout` (`None` sem prazo), e o callback de cada um
/// que ficou pronto, seguido das microtasks. `true` se algum callback rodou.
pub(crate) fn dispatch(global_object: &JSGlobalObject, timeout: Option<Duration>) -> bool {
    let snapshot: Vec<(NetWatch, JSValue)> = WATCHES.with(|watches| {
        watches.borrow().iter().map(|watch| (NetWatch { fd: watch.fd, read: watch.read, write: watch.write }, watch.callback)).collect()
    });
    let waiting_children = CHILDREN.with(|children| !children.borrow().is_empty());
    if snapshot.is_empty() && !waiting_children {
        return false;
    }
    let timeout = if waiting_children { Some(timeout.map_or(CHILD_POLL, |timeout| timeout.min(CHILD_POLL))) } else { timeout };
    // As conclusões imediatas (`fd < 0`) não passam pelo `poll(2)`: contam como prontas na posição em que foram pedidas,
    // na mesma volta que os descritores de verdade, como uma conexão recusada que o bun só entrega no turno do laço.
    let deferred = snapshot.iter().any(|(watch, _)| watch.fd < 0);
    let real: Vec<NetWatch> = snapshot.iter().map(|(watch, _)| *watch).filter(|watch| watch.fd >= 0).collect();
    let mut polled = if real.is_empty() {
        // Sem descritores a espera é só o prazo (o `poll(2)` de zero descritores dorme).
        if waiting_children && !deferred {
            let _ = host().net_poll(&[], timeout);
        }
        Vec::new()
    } else {
        let Ok(ready) = host().net_poll(&real, if deferred { Some(Duration::ZERO) } else { timeout }) else { return false };
        ready
    }
    .into_iter();
    let mut ran = false;
    for (watch, callback) in snapshot {
        let outcome = if watch.fd < 0 { NetReady { writable: true, ..NetReady::default() } } else { polled.next().unwrap_or_default() };
        if !outcome.any() || !still_watching(watch.fd, callback) {
            continue;
        }
        if watch.fd < 0 {
            WATCHES.with(|watches| watches.borrow_mut().retain(|existing| existing.fd != watch.fd));
        }
        ran = true;
        let flags = [outcome.readable, outcome.writable, outcome.hangup, outcome.error].map(JSValue::Bool);
        crate::runtime::timers::call_from_loop(global_object, callback, &flags);
    }
    reap_children(global_object) || ran
}

/// O observador de `fd` ainda é o mesmo (um callback anterior da volta pode tê-lo trocado ou tirado).
fn still_watching(fd: i32, callback: JSValue) -> bool {
    WATCHES.with(|watches| watches.borrow().iter().any(|watch| watch.fd == fd && watch.callback == callback))
}

fn errno_value(errno: i32) -> JSValue {
    js_number(-f64::from(errno))
}

fn pair(global_object: &JSGlobalObject, fd: i32, port: u16) -> JSValue {
    construct_array(global_object.vm(), &global_object.array_structure(), &[js_number(f64::from(fd)), js_number(f64::from(port))]).as_value()
}

fn fd_of(call: &HostCall) -> i32 {
    call.argument(0).to_number() as i32
}

fn listen_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let (port, backlog) = (call.argument(0).to_number() as u16, call.argument(1).to_number() as u32);
    Ok(host().net_listen(port, backlog).map_or_else(errno_value, |(fd, port)| pair(global_object, fd, port)))
}

fn accept_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    Ok(host().net_accept(fd_of(call)).map_or_else(errno_value, |(fd, port)| pair(global_object, fd, port)))
}

fn connect_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    Ok(host().net_connect(call.argument(0).to_number() as u16).map_or_else(errno_value, |(fd, port)| pair(global_object, fd, port)))
}

fn recv_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    match host().net_recv(fd_of(call), call.argument(1).to_number() as usize) {
        Ok(bytes) => {
            let array = create_uint8_array(global_object, bytes.len())?;
            array.with_vector_mut(|destination| destination[..bytes.len()].copy_from_slice(&bytes));
            Ok(array.as_value())
        }
        Err(errno) => Ok(errno_value(errno)),
    }
}

fn send_body(_global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let bytes = input_bytes(call.argument(1)).unwrap_or_default();
    Ok(host().net_send(fd_of(call), &bytes).map_or_else(errno_value, |sent| js_number(sent as f64)))
}

fn shutdown_body(_global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let result = host().net_shutdown(fd_of(call), call.argument(1).to_boolean(), call.argument(2).to_boolean());
    Ok(result.map_or_else(errno_value, |()| js_number(0.0)))
}

fn close_body(_global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let fd = fd_of(call);
    WATCHES.with(|watches| watches.borrow_mut().retain(|watch| watch.fd != fd));
    Ok(host().fs_close(fd).map_or_else(errno_value, |()| js_number(0.0)))
}

/// `netWatch(fd, leitura, escrita, callback)`: troca o observador de `fd`.
fn watch_body(_global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let fd = fd_of(call);
    let watch = Watch { fd, read: call.argument(1).to_boolean(), write: call.argument(2).to_boolean(), callback: call.argument(3), refed: true };
    WATCHES.with(|watches| {
        let mut watches = watches.borrow_mut();
        let refed = watches.iter().find(|existing| existing.fd == fd).is_none_or(|existing| existing.refed);
        watches.retain(|existing| existing.fd != fd);
        watches.push(Watch { refed, ..watch });
    });
    Ok(JSValue::undefined())
}

/// `netDefer(callback)`: o callback roda na próxima volta do laço, na ordem dos outros pedidos de rede.
fn defer_body(_global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let fd = NEXT_DEFERRED.with(|next| next.replace(next.get() - 1));
    WATCHES.with(|watches| watches.borrow_mut().push(Watch { fd, read: false, write: true, callback: call.argument(0), refed: true }));
    Ok(JSValue::undefined())
}

fn unwatch_body(_global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let fd = fd_of(call);
    WATCHES.with(|watches| watches.borrow_mut().retain(|watch| watch.fd != fd));
    Ok(JSValue::undefined())
}

/// `netRef(fd, ligado)`: o observador de `fd` segura (ou deixa de segurar) o processo.
fn ref_body(_global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let (fd, refed) = (fd_of(call), call.argument(1).to_boolean());
    WATCHES.with(|watches| watches.borrow_mut().iter_mut().filter(|watch| watch.fd == fd).for_each(|watch| watch.refed = refed));
    Ok(JSValue::undefined())
}

/// `procWatch(pid, callback)`: o laço espera o filho terminar e chama `callback([código, sinal])`.
fn proc_watch_body(_global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let pid = call.argument(0).to_number() as i32;
    CHILDREN.with(|children| {
        let mut children = children.borrow_mut();
        children.retain(|child| child.pid != pid);
        children.push(Child { pid, callback: call.argument(1), refed: true });
    });
    Ok(JSValue::undefined())
}

fn proc_unwatch_body(_global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let pid = call.argument(0).to_number() as i32;
    CHILDREN.with(|children| children.borrow_mut().retain(|child| child.pid != pid));
    Ok(JSValue::undefined())
}

/// `procRef(pid, ligado)`: o filho segura (ou deixa de segurar) o processo.
fn proc_ref_body(_global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let (pid, refed) = (call.argument(0).to_number() as i32, call.argument(1).to_boolean());
    CHILDREN.with(|children| children.borrow_mut().iter_mut().filter(|child| child.pid == pid).for_each(|child| child.refed = refed));
    Ok(JSValue::undefined())
}

host_function!(proc_watch_function, proc_watch_body);
host_function!(proc_unwatch_function, proc_unwatch_body);
host_function!(proc_ref_function, proc_ref_body);
host_function!(listen_function, listen_body);
host_function!(accept_function, accept_body);
host_function!(connect_function, connect_body);
host_function!(recv_function, recv_body);
host_function!(send_function, send_body);
host_function!(shutdown_function, shutdown_body);
host_function!(close_function, close_body);
host_function!(watch_function, watch_body);
host_function!(unwatch_function, unwatch_body);
host_function!(defer_function, defer_body);
host_function!(ref_function, ref_body);

/// As funções que o `bun_http.js` recebe no `host`.
pub(crate) const NATIVES: [(&str, u32, NativeFunction); 14] = [
    ("netListen", 2, listen_function),
    ("netAccept", 1, accept_function),
    ("netConnect", 1, connect_function),
    ("netRecv", 2, recv_function),
    ("netSend", 2, send_function),
    ("netShutdown", 3, shutdown_function),
    ("netClose", 1, close_function),
    ("netWatch", 4, watch_function),
    ("netUnwatch", 1, unwatch_function),
    ("netRef", 2, ref_function),
    ("netDefer", 1, defer_function),
    ("procWatch", 2, proc_watch_function),
    ("procUnwatch", 1, proc_unwatch_function),
    ("procRef", 2, proc_ref_function),
];
