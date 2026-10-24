//! O event loop do je-code, com a ordem do JavaScript (ver `docs/design.md`).
//!
//! Um loop por thread: a fila de microtarefas (`queueMicrotask`, continuação de promessa, retomada de
//! tarefa `async`) é esvaziada inteira depois de cada macrotarefa; os timers (`setTimeout`) disparam
//! em ordem de vencimento e, no empate, de criação, como no V8. O relógio é real por padrão e virtual
//! dentro de [`with_faked_timers`], o `runWithFakedTimers` dos testes do VS Code.
//!
//! Erro lançado por microtarefa ou timer vai para o `onUnexpectedError`, que é o que o renderer do
//! VS Code faz com exceção não capturada (o `unhandledrejection` e o `error` da janela).
//!
//! # Ligação com o agendamento virtual (`virtual_scheduling`)
//!
//! O `globalTimeApi.ts` do VS Code troca `globalThis.setTimeout`, `setInterval`, `Date` e
//! `performance.now` por uma `TimeApi` (a virtual, a de gravação, a de log). O equivalente aqui é a
//! `TimeApi` instalada no thread ([`time_api`] e [`set_time_api`]): [`set_timeout`], [`clear_timeout`] e
//! [`date_now`] (e os demais métodos, via [`time_api`]) delegam a ela. Sem nada instalado vale a
//! [`RealTimeApi`](crate::virtual_scheduling::time_api::RealTimeApi), que chama as funções `native_*`
//! deste arquivo (o agendador de verdade, o mesmo de sempre). Não existe segundo agendador: a API
//! virtual põe os eventos no `VirtualClock` e é o `VirtualTimeProcessor` que os dispara, usando a API
//! real (os timers nativos deste loop) só para ceder a vez entre eventos, como o original faz com o
//! `setImmediate` do host. O [`with_faked_timers`] (porte do `timeTravelScheduler.ts` antigo) continua
//! intacto e independente: ele troca o relógio nativo, e a `TimeApi` instalada fica por cima dele.

use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, HashMap, VecDeque};
use std::future::Future;
use std::pin::Pin;
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Wake, Waker};
use std::time::{Duration, Instant};

use crate::errors;
use crate::virtual_scheduling::time_api::{RealTimeApi, TimeApi};

type Task = Box<dyn FnOnce()>;
type LocalFuture = Pin<Box<dyn Future<Output = ()>>>;

/// Fila de tarefas acordadas: o `Waker` do Rust precisa ser `Send + Sync`, então ele só grava o
/// número da tarefa aqui, e o loop (no thread dono) faz o resto.
#[derive(Default)]
struct WakeQueue {
    woken: Mutex<Vec<u64>>,
}

struct TaskWaker {
    id: u64,
    queue: Arc<WakeQueue>,
    owner: std::thread::ThreadId,
}

impl Wake for TaskWaker {
    fn wake(self: Arc<Self>) {
        self.wake_by_ref();
    }

    /// No thread dono, retoma a tarefa já, dentro da microtarefa que acordou (a reação da promessa),
    /// como o motor JS retoma a função `async`. Se a tarefa está no meio de um `poll` (acordou a si
    /// mesma) ou o aviso vem de outro thread, a retomada entra na fila e vira microtarefa.
    fn wake_by_ref(self: &Arc<Self>) {
        if std::thread::current().id() == self.owner && LOOP.with(|l| l.futures.borrow().contains_key(&self.id)) {
            poll_task(self.id);
        } else if let Ok(mut woken) = self.queue.woken.lock() {
            woken.push(self.id);
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
struct TimerKey {
    due: u64,
    seq: u64,
}

enum Clock {
    Real(Instant),
    Fake(Cell<u64>),
}

struct EventLoop {
    microtasks: RefCell<VecDeque<Task>>,
    timers: RefCell<BTreeMap<TimerKey, (u64, Task)>>,
    timer_keys: RefCell<HashMap<u64, TimerKey>>,
    next_seq: Cell<u64>,
    clock: RefCell<Clock>,
    futures: RefCell<HashMap<u64, LocalFuture>>,
    wake_queue: Arc<WakeQueue>,
    next_task: Cell<u64>,
}

impl EventLoop {
    fn new() -> Self {
        Self {
            microtasks: RefCell::new(VecDeque::new()),
            timers: RefCell::new(BTreeMap::new()),
            timer_keys: RefCell::new(HashMap::new()),
            next_seq: Cell::new(0),
            clock: RefCell::new(Clock::Real(Instant::now())),
            futures: RefCell::new(HashMap::new()),
            wake_queue: Arc::new(WakeQueue::default()),
            next_task: Cell::new(0),
        }
    }

    fn now(&self) -> u64 {
        match &*self.clock.borrow() {
            Clock::Real(start) => u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX),
            Clock::Fake(now) => now.get(),
        }
    }

    fn seq(&self) -> u64 {
        let s = self.next_seq.get() + 1;
        self.next_seq.set(s);
        s
    }
}

thread_local! {
    static LOOP: EventLoop = EventLoop::new();
}

/// `queueMicrotask(f)`.
pub fn queue_microtask(f: impl FnOnce() + 'static) {
    LOOP.with(|l| l.microtasks.borrow_mut().push_back(Box::new(f)));
}

/// O handle de um timer (também o de intervalo, imediato e quadro de animação, que dividem a numeração);
/// `cancel` é o `clearTimeout`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TimerId(u64);

impl TimerId {
    /// O handle de numeração própria (a `TimeApi` virtual numera os seus, fora da faixa do loop).
    pub const fn from_raw(raw: u64) -> Self {
        Self(raw)
    }

    pub const fn raw(self) -> u64 {
        self.0
    }
}

thread_local! {
    /// A `TimeApi` que o `setTimeout`, o `Date.now` e companhia deste thread usam (o `globalThis` do
    /// original). Começa na real.
    static TIME_API: RefCell<Rc<dyn TimeApi>> = RefCell::new(Rc::new(RealTimeApi) as Rc<dyn TimeApi>);
}

/// A `TimeApi` instalada agora: os globais de tempo do thread (`captureGlobalTimeApi`).
pub fn time_api() -> Rc<dyn TimeApi> {
    TIME_API.with(|api| api.borrow().clone())
}

/// Instala `api` como a `TimeApi` do thread. Quem instala guarda a anterior ([`time_api`]) e a devolve
/// depois (`pushGlobalTimeApi`).
pub fn set_time_api(api: Rc<dyn TimeApi>) {
    TIME_API.with(|current| *current.borrow_mut() = api);
}

/// `setTimeout(f, ms)`, o global: vai para a `TimeApi` instalada. Como no navegador, atraso negativo ou
/// zero vale zero.
pub fn set_timeout(f: impl FnOnce() + 'static, ms: u64) -> TimerId {
    time_api().set_timeout(Box::new(f), ms)
}

/// `clearTimeout(id)`, o global.
pub fn clear_timeout(id: TimerId) {
    time_api().clear_timeout(id);
}

/// O `setTimeout` do próprio loop, sem passar pela `TimeApi` instalada (o real do `RealTimeApi`).
pub(crate) fn native_set_timeout(f: Task, ms: u64) -> TimerId {
    LOOP.with(|l| {
        let id = l.seq();
        let key = TimerKey { due: l.now().saturating_add(ms), seq: id };
        l.timers.borrow_mut().insert(key, (id, f));
        l.timer_keys.borrow_mut().insert(id, key);
        TimerId(id)
    })
}

/// O `clearTimeout` do próprio loop.
pub(crate) fn native_clear_timeout(id: TimerId) {
    LOOP.with(|l| {
        if let Some(key) = l.timer_keys.borrow_mut().remove(&id.0) {
            l.timers.borrow_mut().remove(&key);
        }
    });
}

thread_local! {
    /// O timer pendente de cada intervalo nativo.
    static INTERVALS: RefCell<HashMap<u64, TimerId>> = RefCell::new(HashMap::new());
}

/// Arma o próximo disparo do intervalo `id`: rearma antes de chamar o `handler`, para que uma exceção
/// dele não mate o intervalo (a mesma ordem do intervalo virtual).
fn arm_interval(id: u64, handler: Rc<dyn Fn()>, ms: u64) {
    let timer = native_set_timeout(
        Box::new(move || {
            arm_interval(id, handler.clone(), ms);
            handler();
        }),
        ms,
    );
    INTERVALS.with(|i| i.borrow_mut().insert(id, timer));
}

/// O `setInterval` do próprio loop.
pub(crate) fn native_set_interval(handler: Rc<dyn Fn()>, ms: u64) -> TimerId {
    let id = LOOP.with(EventLoop::seq);
    arm_interval(id, handler, ms);
    TimerId(id)
}

/// O `clearInterval` do próprio loop.
pub(crate) fn native_clear_interval(id: TimerId) {
    let timer = INTERVALS.with(|i| i.borrow_mut().remove(&id.0));
    if let Some(timer) = timer {
        native_clear_timeout(timer);
    }
}

/// Milissegundos do relógio do loop (`Date.now()` relativo ao início, ou o relógio virtual do
/// [`with_faked_timers`]). Nativo: não passa pela `TimeApi` instalada.
pub fn now() -> u64 {
    LOOP.with(EventLoop::now)
}

thread_local! {
    /// O deslocamento do relógio de parede em relação ao do loop (`clock.setSystemTime` do sinon).
    static SYSTEM_TIME_SHIFT: Cell<i64> = const { Cell::new(0) };
}

/// `Date.now()`, o global: vai para a `TimeApi` instalada.
pub fn date_now() -> u64 {
    time_api().date_now()
}

/// O `Date.now()` do próprio loop: o relógio de parede, que é o do loop mais o deslocamento de
/// [`set_system_time`]. Os timers contam pelo relógio do loop e não notam o deslocamento (um
/// `setTimeout` não se importa com o relógio de parede mudar).
pub(crate) fn native_date_now() -> u64 {
    let now = i64::try_from(now()).unwrap_or(i64::MAX);
    u64::try_from(now.saturating_add(SYSTEM_TIME_SHIFT.with(Cell::get))).unwrap_or(0)
}

/// `clock.setSystemTime(ms)` do sinon: muda só o `Date.now()`, sem disparar timers nem mexer no relógio
/// que os conta.
pub fn set_system_time(ms: u64) {
    let delta = i64::try_from(ms).unwrap_or(i64::MAX).saturating_sub(i64::try_from(now()).unwrap_or(i64::MAX));
    SYSTEM_TIME_SHIFT.with(|s| s.set(delta));
}

fn run_guarded(task: Task) {
    if let Err(e) = errors::catch(task) {
        errors::on_unexpected_error(&e);
    }
}

fn poll_task(id: u64) {
    let future = LOOP.with(|l| l.futures.borrow_mut().remove(&id));
    let Some(mut future) = future else { return };
    let queue = LOOP.with(|l| l.wake_queue.clone());
    let waker = Waker::from(Arc::new(TaskWaker { id, queue, owner: std::thread::current().id() }));
    let mut cx = Context::from_waker(&waker);
    match errors::catch(move || {
        let poll = future.as_mut().poll(&mut cx);
        (poll, future)
    }) {
        Ok((Poll::Pending, future)) => {
            LOOP.with(|l| l.futures.borrow_mut().insert(id, future));
        }
        Ok((Poll::Ready(()), _)) => {}
        Err(e) => errors::on_unexpected_error(&e),
    }
}

/// Tarefas acordadas viram microtarefas de retomada, na ordem em que acordaram.
fn schedule_woken() -> bool {
    let woken: Vec<u64> = LOOP.with(|l| l.wake_queue.woken.lock().map(|mut w| std::mem::take(&mut *w)).unwrap_or_default());
    let any = !woken.is_empty();
    for id in woken {
        queue_microtask(move || poll_task(id));
    }
    any
}

/// Esvazia a fila de microtarefas, inclusive as que forem entrando durante o esvaziamento.
pub fn run_microtasks() {
    loop {
        schedule_woken();
        let next = LOOP.with(|l| l.microtasks.borrow_mut().pop_front());
        match next {
            Some(task) => run_guarded(task),
            None => {
                if !schedule_woken() {
                    break;
                }
            }
        }
    }
    // Como o Node, rejeições sem tratador são conferidas depois que as microtarefas acabam.
    crate::promise::flush_unhandled_rejections();
}

/// Roda uma tarefa `async` local até o fim, retomando-a por microtarefa a cada `await` que acorda,
/// como o motor JS retoma uma função `async`. O primeiro trecho roda já, de forma síncrona.
pub fn spawn_local(future: impl Future<Output = ()> + 'static) {
    let id = LOOP.with(|l| {
        let id = l.next_task.get() + 1;
        l.next_task.set(id);
        l.futures.borrow_mut().insert(id, Box::pin(future));
        id
    });
    poll_task(id);
}

/// Dispara o próximo timer vencido (ou, no relógio virtual, avança até ele). Devolve `false` se não
/// há timer pendente.
fn run_next_timer(advance_fake_clock: bool) -> bool {
    let next = LOOP.with(|l| {
        let now = l.now();
        let mut timers = l.timers.borrow_mut();
        let key = *timers.keys().next()?;
        if key.due > now {
            let fake = matches!(&*l.clock.borrow(), Clock::Fake(_));
            if !(advance_fake_clock && fake) {
                return None;
            }
            if let Clock::Fake(c) = &*l.clock.borrow() {
                c.set(key.due);
            }
        }
        let (id, task) = timers.remove(&key)?;
        l.timer_keys.borrow_mut().remove(&id);
        Some(task)
    });
    match next {
        Some(task) => {
            run_guarded(task);
            run_microtasks();
            true
        }
        None => false,
    }
}

/// Uma volta do loop: microtarefas, depois os timers vencidos, cada um seguido das microtarefas que
/// ele gerou.
pub fn turn() {
    run_microtasks();
    while run_next_timer(false) {}
}

/// Quanto falta para o próximo timer, para o loop da janela dormir o tempo certo.
pub fn time_to_next_timer() -> Option<Duration> {
    LOOP.with(|l| {
        let now = l.now();
        l.timers.borrow().keys().next().map(|k| Duration::from_millis(k.due.saturating_sub(now)))
    })
}

/// `runWithFakedTimers`: roda `f` com o relógio virtual a partir de 0 e, ao fim, dispara todos os
/// timers pendentes avançando o relógio até cada um (o `useFakeTimers` com `maxTaskCount` do
/// original, sem limite). Restaura o relógio real ao sair.
pub fn with_faked_timers<T>(f: impl FnOnce() -> T) -> T {
    LOOP.with(|l| *l.clock.borrow_mut() = Clock::Fake(Cell::new(0)));
    SYSTEM_TIME_SHIFT.with(|s| s.set(0));
    let result = errors::catch(f);
    run_microtasks();
    while run_next_timer(true) {}
    LOOP.with(|l| *l.clock.borrow_mut() = Clock::Real(Instant::now()));
    SYSTEM_TIME_SHIFT.with(|s| s.set(0));
    match result {
        Ok(v) => v,
        Err(e) => errors::throw(e),
    }
}

/// Avança o relógio virtual em `ms`, disparando os timers que vencem no caminho.
pub fn advance_fake_time(ms: u64) {
    let target = now().saturating_add(ms);
    loop {
        let due = LOOP.with(|l| l.timers.borrow().keys().next().map(|k| k.due));
        match due {
            Some(due) if due <= target => {
                run_next_timer(true);
            }
            _ => break,
        }
    }
    LOOP.with(|l| {
        if let Clock::Fake(c) = &*l.clock.borrow() {
            c.set(target);
        }
    });
    run_microtasks();
}

/// Roda o loop até não sobrar nada (testes): microtarefas e timers, esperando os timers reais.
pub fn run_until_idle() {
    loop {
        run_microtasks();
        match time_to_next_timer() {
            Some(wait) => {
                if !wait.is_zero() && LOOP.with(|l| matches!(&*l.clock.borrow(), Clock::Real(_))) {
                    std::thread::sleep(wait);
                }
                run_next_timer(true);
            }
            None => {
                let pending = LOOP.with(|l| !l.futures.borrow().is_empty());
                if !schedule_woken() || !pending {
                    break;
                }
            }
        }
    }
}

/// Um flag compartilhado que vira `true` quando a tarefa termina (uso em testes).
pub fn completion_flag() -> (Rc<Cell<bool>>, impl FnOnce()) {
    let flag = Rc::new(Cell::new(false));
    let f = flag.clone();
    (flag, move || f.set(true))
}
