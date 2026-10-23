//! Porte de `src/vs/base/common/event.ts`: `Event<T>`, o namespace `Event`, o `Emitter<T>` e seus
//! derivados, multiplexadores, `EventBufferer`, `Relay` e o monitor de vazamento de listeners.
//!
//! Decisões de tradução:
//! - `Event<T>` é a função de inscrição do original: recebe um listener e devolve o disposable da
//!   inscrição. `thisArgs` não existe (o fecho captura o que precisar) e o parâmetro `disposables` virou
//!   [`Event::listen_into`]/[`Event::listen_in`], que guardam o disposable devolvido no store ou array.
//! - O listener recebe `&T`. Quem repassa o valor adiante (`filter`, `map`, `buffer`, `forward`, os
//!   multiplexadores) precisa de `T: Clone`, porque no JS o mesmo objeto circula e aqui o `fire` consome.
//!   Os tipos de evento do VS Code são valores pequenos ou referências, então é `Clone` ou `Rc`.
//! - O `Emitter<T>` é o próprio objeto (`Rc<Emitter<T>>`), e os ganchos de [`EmitterOptions`] recebem
//!   `&Emitter<T>` como o original passa `this`, o que deixa o gancho disparar o emitter sem referência
//!   circular. Uma exceção de listener é um pânico com [`Error`] (ver `docs/design.md`).
//! - A reentrância é a do original: um `fire` chamado enquanto outra entrega da mesma fila está no meio
//!   termina primeiro a entrega pendente; listener adicionado durante uma entrega não a recebe; listener
//!   removido durante uma entrega deixa de recebê-la.
//!
//! `Event.fromObservable` e `Event.fromObservableLight` ficam no fim do arquivo (usam o módulo
//! `observable`). Ficam de fora de propósito: o `EventProfiling` (`_profName`, só para
//! desenvolvimento), `_enableDisposeWithListenerWarning` e `_enableSnapshotPotentialLeakWarning` (constantes
//! `false` no original).

use std::any::Any;
use std::cell::{Cell, RefCell};
use std::hash::Hash;
use std::marker::PhantomData;
use std::rc::{Rc, Weak};

use crate::cancellation::CancellationToken;
use crate::collections::diff_sets;
use crate::errors::{self, Error};
use crate::lifecycle::{
    self, DisposableId, DisposableMap, DisposableRef, DisposableStore, IDisposable, combined_disposable, id_of, id_of_ref, plain_disposable,
    to_disposable,
};
use crate::linked_list::LinkedList;
use crate::promise::{CancelablePromise, Promise};
use crate::runtime::{self, TimerId};
use crate::stacktrace::Stacktrace;
use crate::symbols::Delay;

const BUFFER_LEAK_WARN_COUNT_THRESHOLD: usize = 100;
const BUFFER_LEAK_WARN_TIME_THRESHOLD_MS: u64 = 60_000; // 1 minuto

/// `_isBufferLeakWarningEnabled()`: `!!env['VSCODE_DEV']`.
fn is_buffer_leak_warning_enabled() -> bool {
    std::env::var("VSCODE_DEV").is_ok_and(|v| !v.is_empty())
}

/// Um listener. `Rc` porque o mesmo listener pode ser compartilhado por combinadores.
pub type Listener<T> = Rc<dyn Fn(&T)>;

/// O tratador de erro de um listener (`onListenerError`).
pub type ErrorHandler = Rc<dyn Fn(&Error)>;

/// As pilhas dos listeners com a contagem de cada uma, na ordem em que apareceram.
type StackCounts = Vec<(String, usize)>;

/// `Event<T>`: a função que inscreve um listener e devolve o disposable da inscrição.
pub struct Event<T: 'static> {
    subscribe: Rc<dyn Fn(Listener<T>) -> DisposableRef>,
}

impl<T: 'static> Clone for Event<T> {
    fn clone(&self) -> Self {
        Self { subscribe: self.subscribe.clone() }
    }
}

/// O `disposables?: IDisposable[] | DisposableStore` do original: onde guardar o disposable da inscrição.
#[derive(Clone)]
pub enum Disposables {
    Store(Rc<DisposableStore>),
    Array(Rc<RefCell<Vec<DisposableRef>>>),
}

impl Disposables {
    /// `addToDisposables(result, disposables)`.
    fn add(&self, result: &DisposableRef) {
        match self {
            Self::Store(store) => {
                store.add(result.clone());
            }
            Self::Array(array) => array.borrow_mut().push(result.clone()),
        }
    }

    /// `disposeAndRemove(result, disposables)`.
    fn dispose_and_remove(&self, result: &DisposableRef) {
        match self {
            Self::Store(store) => store.delete(result),
            Self::Array(array) => {
                let index = array.borrow().iter().position(|d| id_of(d) == id_of(result));
                if let Some(index) = index {
                    array.borrow_mut().remove(index);
                }
            }
        }
        result.dispose();
    }
}

impl<T: 'static> Event<T> {
    pub fn new(subscribe: impl Fn(Listener<T>) -> DisposableRef + 'static) -> Self {
        Self { subscribe: Rc::new(subscribe) }
    }

    /// `event(listener)`.
    pub fn listen(&self, listener: impl Fn(&T) + 'static) -> DisposableRef {
        (self.subscribe)(Rc::new(listener))
    }

    /// `event(listener)` com um listener já em `Rc`.
    pub fn listen_rc(&self, listener: Listener<T>) -> DisposableRef {
        (self.subscribe)(listener)
    }

    /// `event(listener, undefined, store)`: a inscrição vai para o store.
    pub fn listen_in(&self, listener: impl Fn(&T) + 'static, store: &DisposableStore) -> DisposableRef {
        store.add(self.listen(listener))
    }

    /// `event(listener, undefined, disposables)`: a inscrição vai para o store ou para o array.
    pub fn listen_into(&self, listener: impl Fn(&T) + 'static, disposables: &Disposables) -> DisposableRef {
        let result = self.listen(listener);
        disposables.add(&result);
        result
    }

    /// `Event.None`.
    pub fn none() -> Self {
        Self::new(|_| lifecycle::none())
    }

    /// `Event.once`: dispara uma vez só.
    pub fn once(&self) -> Self {
        let event = self.clone();
        Self::new(move |listener| {
            // Precisa disto no caso de o evento disparar durante a chamada do listener.
            let did_fire = Rc::new(Cell::new(false));
            let result: Rc<RefCell<Option<DisposableRef>>> = Rc::default();
            let subscription = event.listen({
                let (did_fire, result) = (did_fire.clone(), result.clone());
                move |e| {
                    if did_fire.get() {
                        return;
                    }
                    let current = result.borrow().clone();
                    match current {
                        Some(current) => current.dispose(),
                        None => did_fire.set(true),
                    }
                    listener(e);
                }
            });
            *result.borrow_mut() = Some(subscription.clone());
            if did_fire.get() {
                subscription.dispose();
            }
            subscription
        })
    }

    /// `Event.signal`: o mesmo evento visto como `Event<void>`.
    pub fn signal(&self) -> Event<()> {
        let event = self.clone();
        Event::new(move |listener: Listener<()>| event.listen(move |_| listener(&())))
    }

    /// `Event.forward(from, to)` na forma `from(e => to.fire(e))`.
    pub fn forward(&self, to: &Rc<Emitter<T>>) -> DisposableRef
    where
        T: Clone,
    {
        let to = to.clone();
        self.listen(move |e| to.fire(e.clone()))
    }

    /// `Event.runAndSubscribe`: chama o `handler` já com `initial` e depois a cada evento.
    pub fn run_and_subscribe(&self, handler: impl Fn(Option<&T>) + 'static, initial: Option<&T>) -> DisposableRef {
        handler(initial);
        self.listen(move |e| handler(Some(e)))
    }

    /// `Event.toPromise`: promessa do primeiro evento, com `cancel()` que solta a inscrição sem resolver.
    pub fn to_promise(&self, disposables: Option<Disposables>) -> CancelablePromise<T>
    where
        T: Clone,
    {
        let (promise, resolver) = Promise::new();
        let listener = self.once().listen(move |e| resolver.resolve(e.clone()));
        if let Some(disposables) = &disposables {
            disposables.add(&listener);
        }
        if let Some(disposables) = disposables.clone() {
            let listener = listener.clone();
            promise.finally(move || disposables.dispose_and_remove(&listener));
        }
        CancelablePromise::new(promise, move || match &disposables {
            Some(disposables) => disposables.dispose_and_remove(&listener),
            None => listener.dispose(),
        })
    }
}

/// `Event.any`: dispara quando qualquer um dos eventos dispara.
pub fn any<T: 'static>(events: Vec<Event<T>>) -> Event<T> {
    Event::new(move |listener: Listener<T>| combined_disposable(events.iter().map(|event| event.listen_rc(listener.clone())).collect()))
}

/// `Event.snapshot` (privado no original): reemite os eventos de `event`, ligando-se a ele só enquanto o
/// emitter tem listener.
fn snapshot<T: Clone + 'static>(event: Event<T>, disposable: Option<&DisposableStore>) -> Event<T> {
    let listener: Rc<RefCell<Option<DisposableRef>>> = Rc::default();
    let emitter = Emitter::with_options(EmitterOptions {
        on_will_add_first_listener: Some(Rc::new({
            let listener = listener.clone();
            move |emitter: &Emitter<T>| *listener.borrow_mut() = Some(event.listen(forwarding(emitter)))
        })),
        on_did_remove_last_listener: Some(Rc::new(move |_: &Emitter<T>| {
            let current = listener.borrow().clone();
            if let Some(current) = current {
                current.dispose();
            }
        })),
        ..EmitterOptions::default()
    });
    if let Some(store) = disposable {
        store.add(emitter.clone());
    }
    emitter.event()
}

/// `emitter.fire` como listener (`event(emitter.fire, emitter)`), sem segurar o emitter vivo.
fn forwarding<T: Clone + 'static>(emitter: &Emitter<T>) -> impl Fn(&T) + 'static {
    let weak = emitter.me.clone();
    move |value| {
        if let Some(emitter) = weak.upgrade() {
            emitter.fire(value.clone());
        }
    }
}

impl<T: Clone + 'static> Event<T> {
    /// `Event.defer`: debounce de 0 ms que vira sinal, ligado a um timer compartilhado.
    pub fn defer(&self, flush_on_listener_remove: Option<bool>, disposable: Option<&DisposableStore>) -> Event<()> {
        self.debounce(
            |_, _| (),
            DebounceOptions { delay: Delay::Millis(0), flush_on_listener_remove: flush_on_listener_remove.unwrap_or(true), ..DebounceOptions::default() },
            disposable,
        )
    }

    /// `Event.onceIf`: dispara uma vez, na primeira vez que a condição vale.
    pub fn once_if(&self, condition: impl Fn(&T) -> bool + 'static) -> Event<T> {
        self.filter(condition, None).once()
    }

    /// `Event.map`.
    pub fn map<O: Clone + 'static>(&self, map: impl Fn(&T) -> O + 'static, disposable: Option<&DisposableStore>) -> Event<O> {
        let (event, map) = (self.clone(), Rc::new(map));
        snapshot(
            Event::new(move |listener: Listener<O>| {
                let map = map.clone();
                event.listen(move |i| listener(&map(i)))
            }),
            disposable,
        )
    }

    /// `Event.forEach`.
    pub fn for_each(&self, each: impl Fn(&T) + 'static, disposable: Option<&DisposableStore>) -> Event<T> {
        let (event, each) = (self.clone(), Rc::new(each));
        snapshot(
            Event::new(move |listener: Listener<T>| {
                let each = each.clone();
                event.listen(move |i| {
                    each(i);
                    listener(i);
                })
            }),
            disposable,
        )
    }

    /// `Event.filter`.
    pub fn filter(&self, filter: impl Fn(&T) -> bool + 'static, disposable: Option<&DisposableStore>) -> Event<T> {
        let (event, filter) = (self.clone(), Rc::new(filter));
        snapshot(
            Event::new(move |listener: Listener<T>| {
                let filter = filter.clone();
                event.listen(move |e| {
                    if filter(e) {
                        listener(e);
                    }
                })
            }),
            disposable,
        )
    }

    /// `Event.reduce`.
    pub fn reduce<O: Clone + 'static>(&self, merge: impl Fn(Option<O>, &T) -> O + 'static, initial: Option<O>, disposable: Option<&DisposableStore>) -> Event<O> {
        let output = RefCell::new(initial);
        self.map(
            move |e| {
                let last = output.borrow_mut().take();
                let merged = merge(last, e);
                *output.borrow_mut() = Some(merged.clone());
                merged
            },
            disposable,
        )
    }

    /// `Event.debounce`: junta os eventos com `merge` e dispara depois de `delay`.
    pub fn debounce<O: 'static>(&self, merge: impl Fn(Option<O>, &T) -> O + 'static, options: DebounceOptions, disposable: Option<&DisposableStore>) -> Event<O> {
        /// `handle` do original: `undefined`, `null` (microtarefa agendada) ou o timer.
        #[derive(Clone, Copy)]
        enum Handle {
            Idle,
            Microtask,
            Timer(TimerId),
        }
        struct State<O> {
            subscription: RefCell<DisposableRef>,
            output: RefCell<Option<O>>,
            handle: Cell<Handle>,
            num_debounced_calls: Cell<usize>,
            do_fire: RefCell<Option<Rc<dyn Fn()>>>,
        }
        let DebounceOptions { delay, leading, flush_on_listener_remove, leak_warning_threshold } = options;
        let state = Rc::new(State {
            subscription: RefCell::new(lifecycle::none()),
            output: RefCell::new(None),
            handle: Cell::new(Handle::Idle),
            num_debounced_calls: Cell::new(0),
            do_fire: RefCell::new(None),
        });
        let (event, merge) = (self.clone(), Rc::new(merge));
        let emitter = Emitter::with_options(EmitterOptions {
            leak_warning_threshold,
            on_will_add_first_listener: Some(Rc::new({
                let state = state.clone();
                move |emitter: &Emitter<O>| {
                    let (state, merge, weak) = (state.clone(), merge.clone(), emitter.me.clone());
                    let subscription = event.listen({
                        let state = state.clone();
                        move |cur| {
                            let Some(emitter) = weak.upgrade() else { return };
                            state.num_debounced_calls.set(state.num_debounced_calls.get() + 1);
                            let last = state.output.borrow_mut().take();
                            *state.output.borrow_mut() = Some(merge(last, cur));

                            if leading && !matches!(state.handle.get(), Handle::Timer(_)) {
                                let output = state.output.borrow_mut().take();
                                if let Some(output) = output {
                                    emitter.fire(output);
                                }
                            }

                            let do_fire: Rc<dyn Fn()> = {
                                let state = state.clone();
                                Rc::new(move || {
                                    let output = state.output.borrow_mut().take();
                                    state.handle.set(Handle::Idle);
                                    if (!leading || state.num_debounced_calls.get() > 1)
                                        && let Some(output) = output
                                    {
                                        emitter.fire(output);
                                    }
                                    state.num_debounced_calls.set(0);
                                })
                            };
                            *state.do_fire.borrow_mut() = Some(do_fire.clone());

                            match delay {
                                Delay::Millis(ms) => {
                                    if let Handle::Timer(timer) = state.handle.get() {
                                        runtime::clear_timeout(timer);
                                    }
                                    state.handle.set(Handle::Timer(runtime::set_timeout(move || do_fire(), ms)));
                                }
                                Delay::Microtask => {
                                    if matches!(state.handle.get(), Handle::Idle) {
                                        state.handle.set(Handle::Microtask);
                                        runtime::queue_microtask(move || do_fire());
                                    }
                                }
                            }
                        }
                    });
                    *state.subscription.borrow_mut() = subscription;
                }
            })),
            on_will_remove_listener: Some(Rc::new({
                let state = state.clone();
                move |_: &Emitter<O>| {
                    if flush_on_listener_remove && state.num_debounced_calls.get() > 0 {
                        let do_fire = state.do_fire.borrow().clone();
                        if let Some(do_fire) = do_fire {
                            do_fire();
                        }
                    }
                }
            })),
            on_did_remove_last_listener: Some(Rc::new(move |_: &Emitter<O>| {
                *state.do_fire.borrow_mut() = None;
                let subscription = state.subscription.borrow().clone();
                subscription.dispose();
            })),
            ..EmitterOptions::default()
        });
        if let Some(store) = disposable {
            store.add(emitter.clone());
        }
        emitter.event()
    }

    /// `Event.accumulate`: debounce que entrega o array de todos os eventos do período.
    pub fn accumulate(&self, delay: Delay, flush_on_listener_remove: Option<bool>, disposable: Option<&DisposableStore>) -> Event<Vec<T>> {
        self.debounce(
            |last: Option<Vec<T>>, e: &T| {
                let mut all = last.unwrap_or_default();
                all.push(e.clone());
                all
            },
            DebounceOptions { delay, flush_on_listener_remove: flush_on_listener_remove.unwrap_or(true), ..DebounceOptions::default() },
            disposable,
        )
    }

    /// `Event.throttle`: no máximo um disparo por período, na borda de subida e/ou de descida.
    pub fn throttle<O: 'static>(&self, merge: impl Fn(Option<O>, &T) -> O + 'static, options: ThrottleOptions, disposable: Option<&DisposableStore>) -> Event<O> {
        struct State<O> {
            subscription: RefCell<DisposableRef>,
            output: RefCell<Option<O>>,
            throttling: Cell<bool>,
            num_throttled_calls: Cell<usize>,
        }
        let ThrottleOptions { delay, leading, trailing, leak_warning_threshold } = options;
        let state = Rc::new(State {
            subscription: RefCell::new(lifecycle::none()),
            output: RefCell::new(None),
            throttling: Cell::new(false),
            num_throttled_calls: Cell::new(0),
        });
        let (event, merge) = (self.clone(), Rc::new(merge));
        let emitter = Emitter::with_options(EmitterOptions {
            leak_warning_threshold,
            on_will_add_first_listener: Some(Rc::new({
                let state = state.clone();
                move |emitter: &Emitter<O>| {
                    let (state, merge, weak) = (state.clone(), merge.clone(), emitter.me.clone());
                    let subscription = event.listen({
                        let state = state.clone();
                        move |cur| {
                            let Some(emitter) = weak.upgrade() else { return };
                            state.num_throttled_calls.set(state.num_throttled_calls.get() + 1);
                            let last = state.output.borrow_mut().take();
                            *state.output.borrow_mut() = Some(merge(last, cur));

                            // Se não está em período de throttle, dispara já quando há borda de subida.
                            if !state.throttling.get() {
                                if leading {
                                    let output = state.output.borrow_mut().take();
                                    if let Some(output) = output {
                                        emitter.fire(output);
                                    }
                                    state.num_throttled_calls.set(0);
                                }

                                state.throttling.set(true);
                                let end_of_period = {
                                    let state = state.clone();
                                    move || {
                                        // Borda de descida, se houve chamadas durante o período.
                                        if trailing && state.num_throttled_calls.get() > 0 {
                                            let output = state.output.borrow_mut().take();
                                            if let Some(output) = output {
                                                emitter.fire(output);
                                            }
                                        }
                                        *state.output.borrow_mut() = None;
                                        state.throttling.set(false);
                                        state.num_throttled_calls.set(0);
                                    }
                                };
                                match delay {
                                    Delay::Millis(ms) => {
                                        runtime::set_timeout(end_of_period, ms);
                                    }
                                    Delay::Microtask => runtime::queue_microtask(end_of_period),
                                }
                            }
                            // Se já está em período de throttle, só acumula para a borda de descida.
                        }
                    });
                    *state.subscription.borrow_mut() = subscription;
                }
            })),
            on_did_remove_last_listener: Some(Rc::new(move |_: &Emitter<O>| {
                let subscription = state.subscription.borrow().clone();
                subscription.dispose();
            })),
            ..EmitterOptions::default()
        });
        if let Some(store) = disposable {
            store.add(emitter.clone());
        }
        emitter.event()
    }

    /// `Event.latch` com a igualdade padrão (`===`): só dispara quando o valor muda.
    pub fn latch(&self, disposable: Option<&DisposableStore>) -> Event<T>
    where
        T: PartialEq,
    {
        self.latch_by(|a, b| a == b, disposable)
    }

    /// `Event.latch(event, equals)`.
    pub fn latch_by(&self, equals: impl Fn(&T, &T) -> bool + 'static, disposable: Option<&DisposableStore>) -> Event<T> {
        let first_call = Cell::new(true);
        let cache: RefCell<Option<T>> = RefCell::new(None);
        self.filter(
            move |value| {
                let should_emit = first_call.get() || !cache.borrow().as_ref().is_some_and(|cached| equals(value, cached));
                first_call.set(false);
                *cache.borrow_mut() = Some(value.clone());
                should_emit
            },
            disposable,
        )
    }

    /// `Event.split`: dois eventos, o dos valores que satisfazem `is_t` e o dos demais.
    pub fn split(&self, is_t: impl Fn(&T) -> bool + 'static, disposable: Option<&DisposableStore>) -> (Event<T>, Event<T>) {
        let is_t = Rc::new(is_t);
        let other = is_t.clone();
        (self.filter(move |e| is_t(e), disposable), self.filter(move |e| !other(e), disposable))
    }

    /// `Event.buffer`: guarda os eventos até haver um listener.
    pub fn buffer(&self, debug_name: &str, flush_after_timeout: bool, initial: Vec<T>, disposable: Option<&Rc<DisposableStore>>) -> Event<T> {
        let disposable = disposable.cloned();
        struct LeakWarning {
            stack: Stacktrace,
            timer: Cell<Option<TimerId>>,
            warned: Cell<bool>,
        }
        struct State<T> {
            buffer: RefCell<Option<Vec<T>>>,
            listener: RefCell<Option<DisposableRef>>,
            leak_warning: Option<LeakWarning>,
        }
        let dev = is_buffer_leak_warning_enabled();
        let state = Rc::new(State {
            buffer: RefCell::new(Some(initial)),
            listener: RefCell::new(None),
            leak_warning: dev.then(|| LeakWarning { stack: Stacktrace::create(), timer: Cell::new(None), warned: Cell::new(false) }),
        });
        let debug_name: Rc<str> = Rc::from(debug_name);

        // Detecção de vazamento só em desenvolvimento: avisa se os eventos se acumulam sem consumo.
        if let Some(warning) = &state.leak_warning {
            let (timer_state, debug_name) = (state.clone(), debug_name.clone());
            warning.timer.set(Some(runtime::set_timeout(
                move || {
                    let warning = timer_state.leak_warning.as_ref().expect("só agendado com o aviso ligado");
                    let buffered = timer_state.buffer.borrow().as_ref().map_or(0, Vec::len);
                    if buffered > 0 && !warning.warned.get() {
                        warning.warned.set(true);
                        eprintln!(
                            "[Event.buffer][{debug_name}] potential LEAK detected: {buffered} events buffered for {}s without being consumed. Buffered here:",
                            BUFFER_LEAK_WARN_TIME_THRESHOLD_MS / 1000
                        );
                        warning.stack.print();
                    }
                },
                BUFFER_LEAK_WARN_TIME_THRESHOLD_MS,
            )));
            if let Some(store) = &disposable {
                let state = state.clone();
                store.add(to_disposable(move || state.clear_leak_warning_timer()));
            }
        }

        impl<T> State<T> {
            fn clear_leak_warning_timer(&self) {
                if let Some(timer) = self.leak_warning.as_ref().and_then(|w| w.timer.get()) {
                    runtime::clear_timeout(timer);
                }
            }
        }

        let event = self.clone();
        let flush = {
            let state = state.clone();
            move |emitter: &Emitter<T>| {
                // O buffer só é anulado depois de disparar, e `forEach` não visita o que entra no meio.
                let len = state.buffer.borrow().as_ref().map_or(0, Vec::len);
                for i in 0..len {
                    let e = state.buffer.borrow().as_ref().and_then(|b| b.get(i).cloned());
                    if let Some(e) = e {
                        emitter.fire(e);
                    }
                }
                *state.buffer.borrow_mut() = None;
                state.clear_leak_warning_timer();
            }
        };
        let emitter = Emitter::with_options(EmitterOptions {
            on_will_add_first_listener: Some(Rc::new({
                let (state, event, disposable) = (state.clone(), event.clone(), disposable.clone());
                move |emitter: &Emitter<T>| {
                    if state.listener.borrow().is_none() {
                        let listener = event.listen(forwarding(emitter));
                        if let Some(store) = &disposable {
                            store.add(listener.clone());
                        }
                        *state.listener.borrow_mut() = Some(listener);
                    }
                }
            })),
            on_did_add_first_listener: Some(Rc::new({
                let state = state.clone();
                move |emitter: &Emitter<T>| {
                    if state.buffer.borrow().is_some() {
                        if flush_after_timeout {
                            let weak = emitter.me.clone();
                            let flush = flush.clone();
                            runtime::set_timeout(
                                move || {
                                    if let Some(emitter) = weak.upgrade() {
                                        flush(&emitter);
                                    }
                                },
                                0,
                            );
                        } else {
                            flush(emitter);
                        }
                    }
                }
            })),
            on_did_remove_last_listener: Some(Rc::new({
                let state = state.clone();
                move |_: &Emitter<T>| {
                    let listener = state.listener.borrow_mut().take();
                    if let Some(listener) = listener {
                        listener.dispose();
                    }
                    state.clear_leak_warning_timer();
                }
            })),
            ..EmitterOptions::default()
        });

        // A inscrição inicial acontece já na criação, como no original (antes de existir listener).
        let listener = event.listen({
            let (state, weak, debug_name) = (state.clone(), emitter.me.clone(), debug_name.clone());
            move |e| {
                if state.buffer.borrow().is_some() {
                    let buffered = {
                        let mut buffer = state.buffer.borrow_mut();
                        let buffer = buffer.as_mut().expect("conferido acima");
                        buffer.push(e.clone());
                        buffer.len()
                    };
                    if let Some(warning) = &state.leak_warning
                        && !warning.warned.get()
                        && buffered >= BUFFER_LEAK_WARN_COUNT_THRESHOLD
                    {
                        warning.warned.set(true);
                        eprintln!("[Event.buffer][{debug_name}] potential LEAK detected: {buffered} events buffered without being consumed. Buffered here:");
                        warning.stack.print();
                    }
                } else if let Some(emitter) = weak.upgrade() {
                    emitter.fire(e.clone());
                }
            }
        });
        if let Some(store) = &disposable {
            store.add(listener.clone());
        }
        *state.listener.borrow_mut() = Some(listener);
        if let Some(store) = &disposable {
            store.add(emitter.clone());
        }
        emitter.event()
    }

    /// `Event.chain`: monta uma cadeia de `map`/`filter`/`reduce`/`latch` sobre o evento. A cadeia (e o
    /// estado dela) é criada por inscrição, como no original.
    pub fn chain<R: 'static>(&self, synthesize: impl Fn(Chain<T, T>) -> Chain<T, R> + 'static) -> Event<R> {
        let event = self.clone();
        Event::new(move |listener: Listener<R>| {
            let steps = synthesize(Chain::<T, T>::new()).steps;
            event.listen(move |value| {
                let mut current: Box<dyn Any> = Box::new(value.clone());
                for step in &steps {
                    match step(current) {
                        Some(next) => current = next,
                        None => return,
                    }
                }
                if let Some(result) = current.downcast_ref::<R>() {
                    listener(result);
                }
            })
        })
    }
}

/// `Event.fromNodeEventEmitter` (`NodeEventEmitter`): quem aceita `on`/`removeListener` por nome.
pub trait NodeEventEmitter<A: 'static> {
    fn on(&self, event: &str, listener: Listener<A>);
    fn remove_listener(&self, event: &str, listener: &Listener<A>);
}

/// `Event.fromDOMEventEmitter` (`DOMEventEmitter`): quem aceita `addEventListener`/`removeEventListener`.
pub trait DomEventEmitter<A: 'static> {
    fn add_event_listener(&self, event: &str, listener: Listener<A>);
    fn remove_event_listener(&self, event: &str, listener: &Listener<A>);
}

/// O que `fromNodeEventEmitter` e `fromDOMEventEmitter` têm em comum: um emitter ligado ao primeiro
/// listener e solto no último.
fn from_listener_target<A: 'static, T: Clone + 'static>(
    event_name: &str,
    map: impl Fn(&A) -> T + 'static,
    attach: impl Fn(&str, Listener<A>) + 'static,
    detach: impl Fn(&str, &Listener<A>) + 'static,
) -> Event<T> {
    let name: Rc<str> = Rc::from(event_name);
    let handler: Rc<RefCell<Option<Listener<A>>>> = Rc::default();
    let map = Rc::new(map);
    let emitter = Emitter::with_options(EmitterOptions {
        on_will_add_first_listener: Some(Rc::new({
            let (name, handler) = (name.clone(), handler.clone());
            move |emitter: &Emitter<T>| {
                let weak = emitter.me.clone();
                let map = map.clone();
                let f: Listener<A> = Rc::new(move |args| {
                    if let Some(emitter) = weak.upgrade() {
                        emitter.fire(map(args));
                    }
                });
                *handler.borrow_mut() = Some(f.clone());
                attach(&name, f);
            }
        })),
        on_did_remove_last_listener: Some(Rc::new(move |_: &Emitter<T>| {
            let f = handler.borrow_mut().take();
            if let Some(f) = f {
                detach(&name, &f);
            }
        })),
        ..EmitterOptions::default()
    });
    emitter.event()
}

/// `Event.fromNodeEventEmitter`.
pub fn from_node_event_emitter<A: 'static, T: Clone + 'static>(emitter: Rc<dyn NodeEventEmitter<A>>, event_name: &str, map: impl Fn(&A) -> T + 'static) -> Event<T> {
    let (on, off) = (emitter.clone(), emitter);
    from_listener_target(event_name, map, move |name, f| on.on(name, f), move |name, f| off.remove_listener(name, f))
}

/// `Event.fromDOMEventEmitter`.
pub fn from_dom_event_emitter<A: 'static, T: Clone + 'static>(emitter: Rc<dyn DomEventEmitter<A>>, event_name: &str, map: impl Fn(&A) -> T + 'static) -> Event<T> {
    let (add, remove) = (emitter.clone(), emitter);
    from_listener_target(event_name, map, move |name, f| add.add_event_listener(name, f), move |name, f| remove.remove_event_listener(name, f))
}

/// Opções de [`Event::debounce`]: `delay = 100`, `leading = false`, `flushOnListenerRemove = false`.
#[derive(Clone, Copy, Debug)]
pub struct DebounceOptions {
    pub delay: Delay,
    pub leading: bool,
    pub flush_on_listener_remove: bool,
    pub leak_warning_threshold: Option<i64>,
}

impl Default for DebounceOptions {
    fn default() -> Self {
        Self { delay: Delay::Millis(100), leading: false, flush_on_listener_remove: false, leak_warning_threshold: None }
    }
}

/// Opções de [`Event::throttle`]: `delay = 100`, `leading = true`, `trailing = true`.
#[derive(Clone, Copy, Debug)]
pub struct ThrottleOptions {
    pub delay: Delay,
    pub leading: bool,
    pub trailing: bool,
    pub leak_warning_threshold: Option<i64>,
}

impl Default for ThrottleOptions {
    fn default() -> Self {
        Self { delay: Delay::Millis(100), leading: true, trailing: true, leak_warning_threshold: None }
    }
}

/// Um passo da cadeia: devolve `None` para interromper (o `HaltChainable` do original).
type ChainStep = Rc<dyn Fn(Box<dyn Any>) -> Option<Box<dyn Any>>>;

/// `IChainableSythensis<T>`: a cadeia de passos de [`Event::chain`], com o tipo de entrada `I` e o de saída `O`.
pub struct Chain<I, O> {
    steps: Vec<ChainStep>,
    _types: PhantomData<(I, O)>,
}

impl<I: 'static, O: 'static> Chain<I, O> {
    fn new() -> Chain<I, I> {
        Chain { steps: Vec::new(), _types: PhantomData }
    }

    fn then<P>(mut self, step: impl Fn(Box<dyn Any>) -> Option<Box<dyn Any>> + 'static) -> Chain<I, P> {
        self.steps.push(Rc::new(step));
        Chain { steps: self.steps, _types: PhantomData }
    }

    /// `map`.
    pub fn map<P: 'static>(self, f: impl Fn(O) -> P + 'static) -> Chain<I, P> {
        self.then(move |v| v.downcast::<O>().ok().map(|v| Box::new(f(*v)) as Box<dyn Any>))
    }

    /// `forEach`.
    pub fn for_each(self, f: impl Fn(&O) + 'static) -> Chain<I, O> {
        self.then(move |v| {
            f(v.downcast_ref::<O>()?);
            Some(v)
        })
    }

    /// `filter`.
    pub fn filter(self, f: impl Fn(&O) -> bool + 'static) -> Chain<I, O> {
        self.then(move |v| f(v.downcast_ref::<O>()?).then_some(v))
    }

    /// `reduce`.
    pub fn reduce<R: Clone + 'static>(self, merge: impl Fn(Option<R>, O) -> R + 'static, initial: Option<R>) -> Chain<I, R> {
        let last = RefCell::new(initial);
        self.then(move |v| {
            let value = *v.downcast::<O>().ok()?;
            let previous = last.borrow_mut().take();
            let merged = merge(previous, value);
            *last.borrow_mut() = Some(merged.clone());
            Some(Box::new(merged) as Box<dyn Any>)
        })
    }

    /// `latch(equals)`.
    pub fn latch_by(self, equals: impl Fn(&O, &O) -> bool + 'static) -> Chain<I, O>
    where
        O: Clone,
    {
        let first_call = Cell::new(true);
        let cache: RefCell<Option<O>> = RefCell::new(None);
        self.then(move |v| {
            let value = v.downcast_ref::<O>()?;
            let should_emit = first_call.get() || !cache.borrow().as_ref().is_some_and(|cached| equals(value, cached));
            first_call.set(false);
            *cache.borrow_mut() = Some(value.clone());
            should_emit.then_some(v)
        })
    }

    /// `latch()` com a igualdade padrão.
    pub fn latch(self) -> Chain<I, O>
    where
        O: Clone + PartialEq,
    {
        self.latch_by(|a, b| a == b)
    }
}

// #region Emitter

/// Gancho de [`EmitterOptions`]: recebe o emitter, como o original passa `this`.
pub type Hook<T> = Rc<dyn Fn(&Emitter<T>)>;

/// `EmitterOptions`.
pub struct EmitterOptions<T: 'static> {
    /// Chamado *antes* de o primeiro listener entrar.
    pub on_will_add_first_listener: Option<Hook<T>>,
    /// Chamado *depois* de o primeiro listener entrar.
    pub on_did_add_first_listener: Option<Hook<T>>,
    /// Chamado depois de um listener entrar.
    pub on_did_add_listener: Option<Hook<T>>,
    /// Chamado *depois* de o último listener sair.
    pub on_did_remove_last_listener: Option<Hook<T>>,
    /// Chamado *antes* de um listener sair.
    pub on_will_remove_listener: Option<Hook<T>>,
    /// Chamado quando um listener lança. O padrão é `onUnexpectedError`.
    pub on_listener_error: Option<ErrorHandler>,
    /// Quantos listeners valem antes de supor vazamento. O padrão é o valor global.
    pub leak_warning_threshold: Option<i64>,
    /// Nome legível do emitter, que entra na mensagem de vazamento.
    pub leak_warning_name: Option<String>,
    /// Fila de entrega compartilhada, para garantir a ordem entre emitters.
    pub delivery_queue: Option<EventDeliveryQueue>,
}

impl<T: 'static> Default for EmitterOptions<T> {
    fn default() -> Self {
        Self {
            on_will_add_first_listener: None,
            on_did_add_first_listener: None,
            on_did_add_listener: None,
            on_did_remove_last_listener: None,
            on_will_remove_listener: None,
            on_listener_error: None,
            leak_warning_threshold: None,
            leak_warning_name: None,
            delivery_queue: None,
        }
    }
}

thread_local! {
    static GLOBAL_LEAK_WARNING_THRESHOLD: Cell<i64> = const { Cell::new(-1) };
    static LEAKAGE_MONITOR_ID: Cell<u32> = const { Cell::new(1) };
}

/// `setGlobalLeakWarningThreshold`: devolve o disposable que restaura o valor anterior.
pub fn set_global_leak_warning_threshold(n: i64) -> DisposableRef {
    let old = GLOBAL_LEAK_WARNING_THRESHOLD.with(|t| t.replace(n));
    plain_disposable(move || GLOBAL_LEAK_WARNING_THRESHOLD.with(|t| t.set(old)))
}

fn next_leakage_monitor_name() -> String {
    format!("{:03x}", LEAKAGE_MONITOR_ID.with(|id| id.replace(id.get() + 1)))
}

/// `ListenerLeakError` e `ListenerRefusalError`: o erro logado ao passar do limite de listeners, ou
/// quando o emitter passa a recusar listeners. Viaja como [`Error`] com `kind`, `listenerCount` e
/// `details` nas propriedades.
pub struct ListenerLeakError {
    pub refusal: bool,
    pub kind: String,
    pub details: String,
    pub stack: String,
    pub listener_count: usize,
    pub emitter_name: Option<String>,
}

impl ListenerLeakError {
    pub fn into_error(self) -> Error {
        let message = match &self.emitter_name {
            Some(name) => format!("[{name}] potential listener LEAK detected, {}", self.kind),
            None => format!("potential listener LEAK detected, {}", self.kind),
        };
        Error::named(if self.refusal { "ListenerRefusalError" } else { "ListenerLeakError" }, message)
            .with_stack(self.stack)
            .with_prop("kind", self.kind)
            .with_prop("listenerCount", self.listener_count)
            .with_prop("details", self.details)
    }

    /// `ListenerLeakError.is(err)`: reconhece pelo formato (`kind` e `listenerCount`), como o original.
    pub fn from_error(error: &Error) -> Option<Self> {
        Some(Self {
            refusal: error.name == "ListenerRefusalError",
            kind: error.prop("kind")?.to_owned(),
            details: error.prop("details").unwrap_or_default().to_owned(),
            stack: error.stack.clone().unwrap_or_default(),
            listener_count: error.prop("listenerCount")?.parse().ok()?,
            emitter_name: None,
        })
    }
}

struct LeakageMonitor {
    error_handler: ErrorHandler,
    threshold: f64,
    name: String,
    /// `None` até o primeiro `check`; depois, as pilhas com a contagem, na ordem em que apareceram.
    stacks: Rc<RefCell<Option<StackCounts>>>,
    warn_countdown: Cell<f64>,
}

impl LeakageMonitor {
    fn new(error_handler: ErrorHandler, threshold: f64, name: String) -> Self {
        Self { error_handler, threshold, name, stacks: Rc::default(), warn_countdown: Cell::new(0.0) }
    }

    fn dispose(&self) {
        if let Some(stacks) = self.stacks.borrow_mut().as_mut() {
            stacks.clear();
        }
    }

    fn check(&self, stack: &Stacktrace, listener_count: usize) -> Option<Box<dyn FnOnce()>> {
        let threshold = self.threshold;
        if threshold <= 0.0 || (listener_count as f64) < threshold {
            return None;
        }

        {
            let mut stacks = self.stacks.borrow_mut();
            let stacks = stacks.get_or_insert_with(Vec::new);
            match stacks.iter_mut().find(|(key, _)| *key == stack.value) {
                Some(entry) => entry.1 += 1,
                None => stacks.push((stack.value.clone(), 1)),
            }
        }
        self.warn_countdown.set(self.warn_countdown.get() - 1.0);

        if self.warn_countdown.get() <= 0.0 {
            // Só avisa na primeira vez que passa e depois a cada vez que passa 50% de novo.
            self.warn_countdown.set(threshold * 0.5);

            let (top_stack, top_count) = self.most_frequent_stack().expect("acabou de contar uma pilha");
            let emitter_name = (!is_hex_name(&self.name)).then(|| self.name.clone());
            let message = format!("[{}] potential listener LEAK detected, having {listener_count} listeners already. MOST frequent listener ({top_count}):", self.name);
            eprintln!("{message}");
            eprintln!("{top_stack}");

            let kind = if top_count as f64 / listener_count as f64 > 0.3 { "dominated" } else { "popular" };
            let error = ListenerLeakError { refusal: false, kind: kind.into(), details: message, stack: top_stack, listener_count, emitter_name }.into_error();
            (self.error_handler)(&error);
        }

        let (stacks, key) = (self.stacks.clone(), stack.value.clone());
        Some(Box::new(move || {
            let mut stacks = stacks.borrow_mut();
            let stacks = stacks.as_mut().expect("existe desde o check");
            if let Some(index) = stacks.iter().position(|(k, _)| *k == key) {
                if stacks[index].1 <= 1 {
                    stacks.remove(index);
                } else {
                    stacks[index].1 -= 1;
                }
            }
        }))
    }

    fn most_frequent_stack(&self) -> Option<(String, usize)> {
        let stacks = self.stacks.borrow();
        let mut top: Option<(String, usize)> = None;
        for (stack, count) in stacks.as_ref()? {
            if top.as_ref().is_none_or(|(_, top_count)| top_count < count) {
                top = Some((stack.clone(), *count));
            }
        }
        top
    }
}

/// `/^[0-9a-f]+$/i.test(name)`.
fn is_hex_name(name: &str) -> bool {
    !name.is_empty() && name.bytes().all(|b| b.is_ascii_hexdigit())
}

/// Quem a fila de entrega precisa de um emitter, de qualquer tipo, para terminar a entrega pendente.
trait QueueOwner {
    /// Entrega o `value` ao listener da posição `index`, com o tratador de erro de quem está entregando.
    fn deliver_at(&self, index: usize, value: &Rc<dyn Any>, on_error: &dyn Fn(&Error));
}

#[derive(Default)]
struct QueueState {
    /// Posição na lista de listeners de `current`.
    i: usize,
    /// A última posição a entregar.
    end: usize,
    /// O emitter em entrega. Fraco: a fila é compartilhada e não pode prender o emitter.
    current: Option<Weak<dyn QueueOwner>>,
    current_id: DisposableId,
    /// O valor em entrega, definido sempre que `current` está.
    value: Option<Rc<dyn Any>>,
}

/// `EventDeliveryQueue` (`createEventDeliveryQueue`): pode ser compartilhada entre emitters para que
/// um `fire` de um termine primeiro a entrega pendente do outro.
#[derive(Clone, Default)]
pub struct EventDeliveryQueue(Rc<RefCell<QueueState>>);

impl EventDeliveryQueue {
    pub fn new() -> Self {
        Self::default()
    }

    fn has_current(&self) -> bool {
        self.0.borrow().current.is_some()
    }

    fn enqueue(&self, emitter: Weak<dyn QueueOwner>, emitter_id: DisposableId, value: Rc<dyn Any>, end: usize) {
        let mut q = self.0.borrow_mut();
        q.i = 0;
        q.end = end;
        q.current = Some(emitter);
        q.current_id = emitter_id;
        q.value = Some(value);
    }

    /// Força qualquer laço de entrega em curso a parar (principalmente no `dispose`).
    fn reset(&self) {
        let mut q = self.0.borrow_mut();
        q.i = q.end;
        q.current = None;
        q.current_id = 0;
        q.value = None;
    }
}

/// Um ou vários listeners: o original guarda o caso comum (um só) sem array.
enum Listeners<T> {
    Single(Listener<T>),
    /// Esparso: a remoção deixa `None`, e a compactação só acontece quando mais da metade está vazia.
    Many(Vec<Option<Listener<T>>>),
}

const COMPACTION_THRESHOLD: usize = 2;

/// `Emitter<T>`.
pub struct Emitter<T: 'static> {
    me: Weak<Emitter<T>>,
    options: EmitterOptions<T>,
    leak_warning_threshold: Option<f64>,
    leak_warning_name: Option<String>,
    leak_warning_error_handler: Option<ErrorHandler>,
    leakage_mon: RefCell<Option<Rc<LeakageMonitor>>>,
    disposed: Cell<bool>,
    listeners: RefCell<Option<Listeners<T>>>,
    /// Sempre definida quando `listeners` é `Many`. Guarda o estado da entrega: se `fire` é chamado, o
    /// que sobrou na fila termina primeiro.
    delivery_queue: RefCell<Option<EventDeliveryQueue>>,
    size: Cell<usize>,
}

impl<T: 'static> Emitter<T> {
    pub fn new() -> Rc<Self> {
        Self::with_options(EmitterOptions::default())
    }

    pub fn with_options(options: EmitterOptions<T>) -> Rc<Self> {
        let global = GLOBAL_LEAK_WARNING_THRESHOLD.with(Cell::get);
        let monitored = global > 0 || options.leak_warning_threshold.is_some_and(|t| t != 0);
        let (threshold, name, handler) = if monitored {
            (
                Some(options.leak_warning_threshold.unwrap_or(global) as f64),
                Some(options.leak_warning_name.clone().unwrap_or_else(next_leakage_monitor_name)),
                Some(options.on_listener_error.clone().unwrap_or_else(|| -> ErrorHandler { Rc::new(errors::on_unexpected_error) })),
            )
        } else {
            (None, None, None)
        };
        let queue = options.delivery_queue.clone();
        Rc::new_cyclic(|me| Self {
            me: me.clone(),
            options,
            leak_warning_threshold: threshold,
            leak_warning_name: name,
            leak_warning_error_handler: handler,
            leakage_mon: RefCell::new(None),
            disposed: Cell::new(false),
            listeners: RefCell::new(None),
            delivery_queue: RefCell::new(queue),
            size: Cell::new(0),
        })
    }

    fn hook(&self, hook: &Option<Hook<T>>) {
        if let Some(hook) = hook {
            hook(self);
        }
    }

    fn this(&self) -> Rc<Self> {
        self.me.upgrade().expect("o emitter está vivo enquanto um método dele roda")
    }

    fn leakage_monitor(&self) -> Option<Rc<LeakageMonitor>> {
        let (threshold, name, handler) = (self.leak_warning_threshold?, self.leak_warning_name.clone()?, self.leak_warning_error_handler.clone()?);
        Some(self.leakage_mon.borrow_mut().get_or_insert_with(|| Rc::new(LeakageMonitor::new(handler, threshold, name))).clone())
    }

    /// `_options.onListenerError || onUnexpectedError`.
    fn report_listener_error(&self, error: &Error) {
        match &self.options.on_listener_error {
            Some(handler) => handler(error),
            None => errors::on_unexpected_error(error),
        }
    }

    /// `get event()`: para o público poder se inscrever nos eventos deste emitter.
    pub fn event(&self) -> Event<T> {
        let this = self.this();
        Event::new(move |callback| this.subscribe(callback))
    }

    fn subscribe(&self, callback: Listener<T>) -> DisposableRef {
        let size = self.size.get();
        if let Some(threshold) = self.leak_warning_threshold
            && size as f64 > threshold * threshold
            && let Some(monitor) = self.leakage_monitor()
        {
            let message = format!("[{}] REFUSES to accept new listeners because it exceeded its threshold by far ({size} vs {})", monitor.name, monitor.threshold);
            eprintln!("{message}");

            let (top_stack, top_count) = monitor.most_frequent_stack().map_or(("UNKNOWN stack".to_owned(), -1), |(stack, count)| (stack, count as i64));
            let kind = if top_count as f64 / size as f64 > 0.3 { "dominated" } else { "popular" };
            let error = ListenerLeakError {
                refusal: true,
                kind: kind.into(),
                details: format!("{message}. HINT: Stack shows most frequent listener ({top_count}-times)"),
                stack: top_stack,
                listener_count: size,
                emitter_name: self.options.leak_warning_name.clone(),
            }
            .into_error();
            self.report_listener_error(&error);
            return lifecycle::none();
        }

        if self.disposed.get() {
            // Adicionar listener a um emitter descartado acontece com frequência, então não avisa.
            return lifecycle::none();
        }

        // Cada inscrição é um contêiner único, mesmo que o mesmo listener entre duas vezes.
        let contained: Listener<T> = Rc::new(move |value: &T| callback(value));

        let mut remove_monitor: Option<Box<dyn FnOnce()>> = None;
        if let Some(threshold) = self.leak_warning_threshold
            && size as f64 >= (threshold * 0.2).ceil()
            && let Some(monitor) = self.leakage_monitor()
            && (size + 1) as f64 >= monitor.threshold
        {
            // Confere e registra este emitter como possível vazamento.
            remove_monitor = monitor.check(&Stacktrace::create(), size + 1);
        }

        enum Shape {
            Empty,
            One,
            Many,
        }
        let shape = match &*self.listeners.borrow() {
            None => Shape::Empty,
            Some(Listeners::Single(_)) => Shape::One,
            Some(Listeners::Many(_)) => Shape::Many,
        };
        match shape {
            Shape::Empty => {
                self.hook(&self.options.on_will_add_first_listener);
                *self.listeners.borrow_mut() = Some(Listeners::Single(contained.clone()));
                self.hook(&self.options.on_did_add_first_listener);
            }
            Shape::One => {
                self.delivery_queue.borrow_mut().get_or_insert_with(EventDeliveryQueue::new);
                let mut listeners = self.listeners.borrow_mut();
                if let Some(Listeners::Single(first)) = listeners.take() {
                    *listeners = Some(Listeners::Many(vec![Some(first), Some(contained.clone())]));
                }
            }
            Shape::Many => {
                if let Some(Listeners::Many(list)) = &mut *self.listeners.borrow_mut() {
                    list.push(Some(contained.clone()));
                }
            }
        }
        self.hook(&self.options.on_did_add_listener);

        self.size.set(self.size.get() + 1);

        let this = self.this();
        to_disposable(move || {
            if let Some(remove_monitor) = remove_monitor {
                remove_monitor();
            }
            this.remove_listener(&contained);
        })
    }

    fn remove_listener(&self, listener: &Listener<T>) {
        self.hook(&self.options.on_will_remove_listener);

        if self.listeners.borrow().is_none() {
            return; // esperado quando um listener é descartado
        }

        if self.size.get() == 1 {
            *self.listeners.borrow_mut() = None;
            self.hook(&self.options.on_did_remove_last_listener);
            self.size.set(0);
            return;
        }

        // size > 1, o que exige que `listeners` seja uma lista.
        let mut guard = self.listeners.borrow_mut();
        let Some(Listeners::Many(listeners)) = &mut *guard else {
            drop(guard);
            errors::throw(Error::named("TypeError", "listeners.indexOf is not a function"));
        };
        let Some(index) = listeners.iter().position(|l| l.as_ref().is_some_and(|l| Rc::ptr_eq(l, listener))) else {
            drop(guard);
            eprintln!("disposed? {}", self.disposed.get());
            eprintln!("size? {}", self.size.get());
            errors::throw(Error::new("Attempted to dispose unknown listener"));
        };

        self.size.set(self.size.get() - 1);
        listeners[index] = None;

        let queue = self.delivery_queue.borrow().clone();
        let adjust_delivery_queue = queue.as_ref().is_some_and(|q| q.0.borrow().current_id == id_of_ref(self));
        if self.size.get() * COMPACTION_THRESHOLD <= listeners.len() {
            let mut n = 0;
            for i in 0..listeners.len() {
                if listeners[i].is_some() {
                    listeners.swap(n, i);
                    n += 1;
                } else if adjust_delivery_queue
                    && let Some(queue) = &queue
                {
                    let mut queue = queue.0.borrow_mut();
                    if n < queue.end {
                        queue.end -= 1;
                        if n < queue.i {
                            queue.i -= 1;
                        }
                    }
                }
            }
            listeners.truncate(n);
        }
    }

    fn deliver(&self, listener: &Listener<T>, value: &T) {
        if let Err(e) = errors::catch(|| listener(value)) {
            self.report_listener_error(&e);
        }
    }

    /// Entrega os itens da fila. Supõe que a fila está pronta.
    fn deliver_queue(&self, dq: &EventDeliveryQueue) {
        let current = dq.0.borrow().current.as_ref().and_then(Weak::upgrade);
        let Some(current) = current else {
            dq.reset();
            return;
        };
        loop {
            // `i` avança antes da entrega porque ela pode reentrar aqui.
            let next = {
                let mut q = dq.0.borrow_mut();
                if q.i >= q.end {
                    break;
                }
                let i = q.i;
                q.i += 1;
                q.value.clone().map(|v| (i, v))
            };
            let Some((index, value)) = next else { break };
            current.deliver_at(index, &value, &|e| self.report_listener_error(e));
        }
        dq.reset();
    }

    /// `fire(event)`.
    pub fn fire(&self, event: T) {
        let queue = self.delivery_queue.borrow().clone();
        if let Some(queue) = &queue
            && queue.has_current()
        {
            self.deliver_queue(queue);
        }

        enum Pending<T> {
            Nothing,
            One(Listener<T>),
            Many(usize),
        }
        let pending = match &*self.listeners.borrow() {
            None => Pending::Nothing,
            Some(Listeners::Single(listener)) => Pending::One(listener.clone()),
            Some(Listeners::Many(list)) => Pending::Many(list.len()),
        };
        match pending {
            Pending::Nothing => {}
            Pending::One(listener) => self.deliver(&listener, &event),
            Pending::Many(count) => {
                let dq = self.delivery_queue.borrow_mut().get_or_insert_with(EventDeliveryQueue::new).clone();
                let me: Weak<dyn QueueOwner> = self.me.clone();
                dq.enqueue(me, id_of_ref(self), Rc::new(event), count);
                self.deliver_queue(&dq);
            }
        }
    }

    pub fn has_listeners(&self) -> bool {
        self.size.get() > 0
    }

    /// Os listeners atuais, na ordem (o `forEachListener` do `AsyncEmitter`).
    fn current_listeners(&self) -> Vec<Listener<T>> {
        match &*self.listeners.borrow() {
            None => Vec::new(),
            Some(Listeners::Single(listener)) => vec![listener.clone()],
            Some(Listeners::Many(list)) => list.iter().flatten().cloned().collect(),
        }
    }

    /// Só para teste: o `_size = n` que a suíte do original faz numa subclasse para simular muitos
    /// listeners sem criá-los.
    #[doc(hidden)]
    pub fn set_size_for_testing(&self, size: usize) {
        self.size.set(size);
    }

    /// Só para teste: o `emitter['_size']` do original (o número de listeners).
    #[doc(hidden)]
    pub fn size_for_testing(&self) -> usize {
        self.size.get()
    }

    /// Só para teste: o `Object.hasOwn(emitter, '_leakageMon')` do original.
    #[doc(hidden)]
    pub fn has_leakage_monitor_for_testing(&self) -> bool {
        self.leakage_mon.borrow().is_some()
    }
}

impl<T: 'static> QueueOwner for Emitter<T> {
    fn deliver_at(&self, index: usize, value: &Rc<dyn Any>, on_error: &dyn Fn(&Error)) {
        let listener = match &*self.listeners.borrow() {
            Some(Listeners::Many(list)) => list.get(index).cloned().flatten(),
            _ => None,
        };
        let (Some(listener), Some(value)) = (listener, value.downcast_ref::<T>()) else { return };
        if let Err(e) = errors::catch(|| listener(value)) {
            on_error(&e);
        }
    }
}

impl<T: 'static> IDisposable for Emitter<T> {
    fn dispose(&self) {
        if self.disposed.replace(true) {
            return;
        }
        // É ruim ter listeners ao descartar um emitter, e pior ainda eles segurarem o emitter vivo pelo
        // disposable da inscrição. O original só avisa, em modo de desenvolvimento, no próximo tick;
        // aqui os listeners simplesmente são soltos.
        let queue = self.delivery_queue.borrow().clone();
        if let Some(queue) = queue
            && queue.0.borrow().current_id == id_of_ref(self)
        {
            queue.reset();
        }
        if self.listeners.borrow().is_some() {
            *self.listeners.borrow_mut() = None;
            self.size.set(0);
        }
        self.hook(&self.options.on_did_remove_last_listener);
        let monitor = self.leakage_mon.borrow().clone();
        if let Some(monitor) = monitor {
            monitor.dispose();
        }
    }
}

// #endregion

/// `IWaitUntil` mais os dados do evento (`IWaitUntilData<T>`): o que o listener de um [`AsyncEmitter`]
/// recebe. `data` é o evento sem `token` e `waitUntil`.
pub struct WaitUntilEvent<D> {
    pub data: D,
    pub token: CancellationToken,
    thenables: Rc<RefCell<Vec<Promise<()>>>>,
    /// `Object.isFrozen(thenables)`: depois da chamada do listener, `waitUntil` não vale mais.
    frozen: Rc<Cell<bool>>,
    promise_join: Option<PromiseJoin<D>>,
    listener: Listener<WaitUntilEvent<D>>,
}

impl<D> WaitUntilEvent<D> {
    /// `waitUntil(thenable)`: só pode ser chamado de forma síncrona, durante o listener.
    pub fn wait_until(&self, promise: Promise<()>) {
        if self.frozen.get() {
            errors::throw(Error::new("waitUntil can NOT be called asynchronous"));
        }
        let promise = match &self.promise_join {
            Some(join) => join(promise, &self.listener),
            None => promise,
        };
        self.thenables.borrow_mut().push(promise);
    }
}

/// O `promiseJoin` de `fireAsync`: embrulha cada promessa de `waitUntil` junto com o listener que a deu.
pub type PromiseJoin<D> = Rc<dyn Fn(Promise<()>, &Listener<WaitUntilEvent<D>>) -> Promise<()>>;

/// Delega `Deref` para o emitter base e `IDisposable` para o `dispose` dele, o "herda de `Emitter`" do
/// original para os emitters derivados.
macro_rules! inherit_emitter {
    ($name:ident, $base:ty, $field:ident) => {
        impl<T: 'static> std::ops::Deref for $name<T> {
            type Target = $base;

            fn deref(&self) -> &$base {
                &self.$field
            }
        }

        impl<T: 'static> IDisposable for $name<T> {
            fn dispose(&self) {
                (**self).dispose();
            }
        }
    };
}

/// `AsyncEmitter<T>`: o `fireAsync` espera, em sequência, as promessas que cada listener pendurou com
/// `waitUntil`.
pub struct AsyncEmitter<D: Clone + 'static> {
    emitter: Rc<Emitter<WaitUntilEvent<D>>>,
    async_delivery_queue: Rc<LinkedList<(Listener<WaitUntilEvent<D>>, D)>>,
}

impl<D: Clone + 'static> std::ops::Deref for AsyncEmitter<D> {
    type Target = Emitter<WaitUntilEvent<D>>;

    fn deref(&self) -> &Self::Target {
        &self.emitter
    }
}

impl<D: Clone + 'static> IDisposable for AsyncEmitter<D> {
    fn dispose(&self) {
        (**self).dispose();
    }
}

impl<D: Clone + 'static> AsyncEmitter<D> {
    pub fn new() -> Rc<Self> {
        Self::with_options(EmitterOptions::default())
    }

    pub fn with_options(options: EmitterOptions<WaitUntilEvent<D>>) -> Rc<Self> {
        Rc::new(Self { emitter: Emitter::with_options(options), async_delivery_queue: Rc::new(LinkedList::new()) })
    }

    /// `fireAsync(data, token, promiseJoin)`. Roda de forma síncrona até o primeiro `await`, como uma
    /// função `async`.
    pub fn fire_async(&self, data: D, token: CancellationToken, promise_join: Option<PromiseJoin<D>>) -> Promise<()> {
        let (done, resolver) = Promise::new();
        let listeners = self.emitter.current_listeners();
        if listeners.is_empty() {
            resolver.resolve(());
            return done;
        }
        let queue = self.async_delivery_queue.clone();
        for listener in listeners {
            drop(queue.push((listener, data.clone())));
        }
        runtime::spawn_local(async move {
            while queue.size() > 0 && !token.is_cancellation_requested() {
                let Some((listener, data)) = queue.shift() else { break };
                let thenables = Rc::new(RefCell::new(Vec::new()));
                let frozen = Rc::new(Cell::new(false));
                let event = WaitUntilEvent {
                    data,
                    token: token.clone(),
                    thenables: thenables.clone(),
                    frozen: frozen.clone(),
                    promise_join: promise_join.clone(),
                    listener: listener.clone(),
                };
                if let Err(e) = errors::catch(|| listener(&event)) {
                    errors::on_unexpected_error(&e);
                    continue;
                }

                // Congela a coleção para obrigar `waitUntil` a ser síncrono, e espera todas.
                frozen.set(true);
                let waiting = std::mem::take(&mut *thenables.borrow_mut());
                let settled = Promise::all_settled(waiting).then(|values| {
                    for value in values {
                        if let Err(e) = value {
                            errors::on_unexpected_error(&e);
                        }
                    }
                });
                let _ = settled.await;
            }
            resolver.resolve(());
        });
        done
    }
}

/// `PauseableEmitter<T>`: enquanto pausado, guarda os eventos e entrega ao retomar (um a um, ou fundidos
/// pela função `merge`).
pub struct PauseableEmitter<T: 'static> {
    emitter: Rc<Emitter<T>>,
    is_paused: Cell<u32>,
    event_queue: LinkedList<T>,
    merge: Option<Rc<dyn Fn(Vec<T>) -> T>>,
}

inherit_emitter!(PauseableEmitter, Emitter<T>, emitter);

impl<T: 'static> PauseableEmitter<T> {
    pub fn new() -> Rc<Self> {
        Self::with_options(EmitterOptions::default(), None)
    }

    pub fn with_options(options: EmitterOptions<T>, merge: Option<Rc<dyn Fn(Vec<T>) -> T>>) -> Rc<Self> {
        Rc::new(Self { emitter: Emitter::with_options(options), is_paused: Cell::new(0), event_queue: LinkedList::new(), merge })
    }

    pub fn is_paused(&self) -> bool {
        self.is_paused.get() != 0
    }

    pub fn pause(&self) {
        self.is_paused.set(self.is_paused.get() + 1);
    }

    pub fn resume(&self) {
        if self.is_paused.get() == 0 {
            return;
        }
        self.is_paused.set(self.is_paused.get() - 1);
        if self.is_paused.get() != 0 {
            return;
        }
        match &self.merge {
            Some(merge) => {
                // Usa a função de fusão para criar um evento composto. A fila é esvaziada antes, caso o
                // disparo pause este emitter de novo.
                if self.event_queue.size() > 0 {
                    let mut events = Vec::new();
                    while let Some(event) = self.event_queue.shift() {
                        events.push(event);
                    }
                    self.emitter.fire(merge(events));
                }
            }
            None => {
                // Sem fusão, dispara cada evento e confere que este emitter não foi pausado no meio.
                while !self.is_paused()
                    && let Some(event) = self.event_queue.shift()
                {
                    self.emitter.fire(event);
                }
            }
        }
    }

    pub fn fire(&self, event: T) {
        if self.emitter.has_listeners() {
            if self.is_paused() {
                drop(self.event_queue.push(event));
            } else {
                self.emitter.fire(event);
            }
        }
    }
}

/// `DebounceEmitter<T>`: um `PauseableEmitter` que pausa no primeiro `fire` e retoma depois de `delay`,
/// entregando o que juntou fundido por `merge`.
pub struct DebounceEmitter<T: 'static> {
    me: Weak<DebounceEmitter<T>>,
    pauseable: Rc<PauseableEmitter<T>>,
    delay: u64,
    handle: Cell<Option<TimerId>>,
}

inherit_emitter!(DebounceEmitter, PauseableEmitter<T>, pauseable);

impl<T: 'static> DebounceEmitter<T> {
    /// `delay` padrão: 100 ms.
    pub fn new(merge: impl Fn(Vec<T>) -> T + 'static, delay: Option<u64>) -> Rc<Self> {
        Self::with_options(EmitterOptions::default(), merge, delay)
    }

    pub fn with_options(options: EmitterOptions<T>, merge: impl Fn(Vec<T>) -> T + 'static, delay: Option<u64>) -> Rc<Self> {
        Rc::new_cyclic(|me| Self {
            me: me.clone(),
            pauseable: PauseableEmitter::with_options(options, Some(Rc::new(merge))),
            delay: delay.unwrap_or(100),
            handle: Cell::new(None),
        })
    }

    pub fn fire(&self, event: T) {
        if self.handle.get().is_none() {
            self.pauseable.pause();
            let this = self.me.upgrade().expect("o emitter está vivo enquanto um método dele roda");
            self.handle.set(Some(runtime::set_timeout(
                move || {
                    this.handle.set(None);
                    this.pauseable.resume();
                },
                self.delay,
            )));
        }
        self.pauseable.fire(event);
    }
}

/// `MicrotaskEmitter<T>`: enfileira os eventos e processa todos no fim da volta do loop (uma
/// microtarefa). Divergência do original: evento disparado durante a entrega vai para a próxima
/// microtarefa, em vez de ser descartado quando o original zera a fila.
pub struct MicrotaskEmitter<T: 'static> {
    me: Weak<MicrotaskEmitter<T>>,
    emitter: Rc<Emitter<T>>,
    queued_events: RefCell<Vec<T>>,
    merge: Option<Rc<dyn Fn(Vec<T>) -> T>>,
}

inherit_emitter!(MicrotaskEmitter, Emitter<T>, emitter);

impl<T: 'static> MicrotaskEmitter<T> {
    pub fn new() -> Rc<Self> {
        Self::with_options(EmitterOptions::default(), None)
    }

    pub fn with_options(options: EmitterOptions<T>, merge: Option<Rc<dyn Fn(Vec<T>) -> T>>) -> Rc<Self> {
        Rc::new_cyclic(|me| Self { me: me.clone(), emitter: Emitter::with_options(options), queued_events: RefCell::new(Vec::new()), merge })
    }

    pub fn fire(&self, event: T) {
        if !self.emitter.has_listeners() {
            return;
        }
        let first = {
            let mut queued = self.queued_events.borrow_mut();
            queued.push(event);
            queued.len() == 1
        };
        if first {
            let this = self.me.upgrade().expect("o emitter está vivo enquanto um método dele roda");
            runtime::queue_microtask(move || {
                let events = std::mem::take(&mut *this.queued_events.borrow_mut());
                match &this.merge {
                    Some(merge) => this.emitter.fire(merge(events)),
                    None => events.into_iter().for_each(|e| this.emitter.fire(e)),
                }
            });
        }
    }
}

/// Um evento multiplexado: a origem e a inscrição nela enquanto o multiplexador tem listeners.
struct Multiplexed<T: 'static> {
    event: Event<T>,
    listener: RefCell<Option<DisposableRef>>,
}

struct MultiplexerState<T: 'static> {
    has_listeners: Cell<bool>,
    events: RefCell<Vec<Rc<Multiplexed<T>>>>,
}

impl<T: Clone + 'static> MultiplexerState<T> {
    fn hook(e: &Multiplexed<T>, emitter: &Emitter<T>) {
        *e.listener.borrow_mut() = Some(e.event.listen(forwarding(emitter)));
    }

    fn unhook(e: &Multiplexed<T>) {
        let listener = e.listener.borrow_mut().take();
        if let Some(listener) = listener {
            listener.dispose();
        }
    }
}

/// `EventMultiplexer<T>`: junta muitos eventos num só, com origens que entram e saem a qualquer hora.
pub struct EventMultiplexer<T: Clone + 'static> {
    emitter: Rc<Emitter<T>>,
    state: Rc<MultiplexerState<T>>,
}

impl<T: Clone + 'static> EventMultiplexer<T> {
    pub fn new() -> Rc<Self> {
        let state = Rc::new(MultiplexerState { has_listeners: Cell::new(false), events: RefCell::new(Vec::new()) });
        let emitter = Emitter::with_options(EmitterOptions {
            on_will_add_first_listener: Some(Rc::new({
                let state = state.clone();
                move |emitter: &Emitter<T>| {
                    state.has_listeners.set(true);
                    let events = state.events.borrow().clone();
                    events.iter().for_each(|e| MultiplexerState::hook(e, emitter));
                }
            })),
            on_did_remove_last_listener: Some(Rc::new({
                let state = state.clone();
                move |_: &Emitter<T>| {
                    state.has_listeners.set(false);
                    let events = state.events.borrow().clone();
                    events.iter().for_each(|e| MultiplexerState::unhook(e));
                }
            })),
            ..EmitterOptions::default()
        });
        Rc::new(Self { emitter, state })
    }

    pub fn event(&self) -> Event<T> {
        self.emitter.event()
    }

    pub fn add(&self, event: Event<T>) -> DisposableRef {
        let e = Rc::new(Multiplexed { event, listener: RefCell::new(None) });
        self.state.events.borrow_mut().push(e.clone());

        if self.state.has_listeners.get() {
            MultiplexerState::hook(&e, &self.emitter);
        }

        let state = self.state.clone();
        to_disposable(move || {
            if state.has_listeners.get() {
                MultiplexerState::unhook(&e);
            }
            let mut events = state.events.borrow_mut();
            // `splice(indexOf(e), 1)`: com `indexOf` = -1 o JS remove o último.
            match events.iter().position(|x| Rc::ptr_eq(x, &e)) {
                Some(index) => {
                    events.remove(index);
                }
                None => {
                    events.pop();
                }
            }
        })
    }
}

impl<T: Clone + 'static> IDisposable for EventMultiplexer<T> {
    fn dispose(&self) {
        self.emitter.dispose();
        let events = std::mem::take(&mut *self.state.events.borrow_mut());
        for e in &events {
            let listener = e.listener.borrow().clone();
            if let Some(listener) = listener {
                listener.dispose();
            }
        }
    }
}

/// `DynamicListEventMultiplexer<TItem, TEventType>`: o evento de cada item de uma lista que cresce e
/// encolhe, num evento só.
pub struct DynamicListEventMultiplexer<TItem: Eq + Hash + Clone + 'static, TEventType: Clone + 'static> {
    store: Rc<DisposableStore>,
    pub event: Event<TEventType>,
    _items: PhantomData<TItem>,
}

impl<TItem: Eq + Hash + Clone + 'static, TEventType: Clone + 'static> DynamicListEventMultiplexer<TItem, TEventType> {
    pub fn new(items: Vec<TItem>, on_add_item: &Event<TItem>, on_remove_item: &Event<TItem>, get_event: impl Fn(&TItem) -> Event<TEventType> + 'static) -> Self {
        let store = DisposableStore::new();
        let multiplexer = store.add_rc(EventMultiplexer::<TEventType>::new());
        let item_listeners = store.add_rc(DisposableMap::<TItem, dyn IDisposable>::new());

        let add_item = {
            let (multiplexer, item_listeners) = (multiplexer.clone(), item_listeners.clone());
            move |instance: &TItem| item_listeners.set(instance.clone(), multiplexer.add(get_event(instance)), false)
        };

        // Itens existentes
        for instance in &items {
            add_item(instance);
        }

        // Itens adicionados
        store.add(on_add_item.listen(add_item));

        // Itens removidos
        store.add(on_remove_item.listen(move |instance| item_listeners.delete_and_dispose(instance)));

        Self { store, event: multiplexer.event(), _items: PhantomData }
    }
}

impl<TItem: Eq + Hash + Clone + 'static, TEventType: Clone + 'static> IDisposable for DynamicListEventMultiplexer<TItem, TEventType> {
    fn dispose(&self) {
        self.store.dispose();
    }
}

/// O dado do buffer de um `bufferEvents`: as funções de entrega adiadas e o estado das reduções.
struct BufferData {
    buffers: RefCell<Vec<Box<dyn FnOnce()>>>,
    /// O que `reduceData.items`/`reducedResult` guardam no original, por chamada de `wrapEvent`.
    reductions: RefCell<Vec<(usize, Rc<dyn Any>)>>,
}

/// O estado de uma redução em buffer: os itens acumulados e o resultado, calculado uma vez e dividido
/// entre os listeners.
struct Reduction<T, O> {
    items: RefCell<Vec<T>>,
    reduced_result: RefCell<Option<O>>,
}

/// `EventBufferer`: segura os eventos enquanto um trecho de código roda e entrega no fim.
#[derive(Default)]
pub struct EventBufferer {
    data: RefCell<Vec<Rc<BufferData>>>,
    next_wrap: Cell<usize>,
}

impl EventBufferer {
    pub fn new() -> Rc<Self> {
        Rc::default()
    }

    /// `wrapEvent(event)`.
    pub fn wrap_event<T: Clone + 'static>(self: &Rc<Self>, event: &Event<T>) -> Event<T> {
        let (this, event) = (self.clone(), event.clone());
        Event::new(move |listener: Listener<T>| {
            let this = this.clone();
            event.listen(move |i| {
                let data = this.data.borrow().last().cloned();
                match data {
                    // Buffering
                    Some(data) => {
                        let (listener, i) = (listener.clone(), i.clone());
                        data.buffers.borrow_mut().push(Box::new(move || listener(&i)));
                    }
                    // Not buffering
                    None => listener(i),
                }
            })
        })
    }

    /// `wrapEvent(event, reduce, initial)`: no buffer, os eventos do período viram um só.
    pub fn wrap_event_reduce<T: Clone + 'static, O: Clone + 'static>(
        self: &Rc<Self>,
        event: &Event<T>,
        reduce: impl Fn(Option<O>, &T) -> O + 'static,
        initial: Option<O>,
    ) -> Event<O> {
        self.wrap_reduced(event, reduce, initial, None::<fn(&T) -> O>)
    }

    /// `wrapEvent(event, reduce)` com `O = T`: no buffer, o primeiro evento do período é o ponto de
    /// partida da redução (o `items.reduce(reduce)` sem valor inicial).
    pub fn wrap_event_merge<T: Clone + 'static>(self: &Rc<Self>, event: &Event<T>, reduce: impl Fn(Option<T>, &T) -> T + 'static) -> Event<T> {
        self.wrap_reduced(event, reduce, None, Some(T::clone))
    }

    fn wrap_reduced<T: Clone + 'static, O: Clone + 'static>(
        self: &Rc<Self>,
        event: &Event<T>,
        reduce: impl Fn(Option<O>, &T) -> O + 'static,
        initial: Option<O>,
        seed: Option<fn(&T) -> O>,
    ) -> Event<O> {
        let (this, event, reduce) = (self.clone(), event.clone(), Rc::new(reduce));
        let wrap_id = this.next_wrap.replace(this.next_wrap.get() + 1);
        Event::new(move |listener: Listener<O>| {
            let (this, reduce, initial) = (this.clone(), reduce.clone(), initial.clone());
            event.listen(move |i| {
                let data = this.data.borrow().last().cloned();
                // Not buffering
                let Some(data) = data else {
                    listener(&reduce(initial.clone(), i));
                    return;
                };

                // Buffering: acumula os itens (compartilhados entre os listeners desta chamada).
                let reduction = {
                    let mut reductions = data.reductions.borrow_mut();
                    match reductions.iter().find(|(id, _)| *id == wrap_id) {
                        Some((_, reduction)) => reduction.clone(),
                        None => {
                            let reduction: Rc<dyn Any> = Rc::new(Reduction::<T, O> { items: RefCell::new(Vec::new()), reduced_result: RefCell::new(None) });
                            reductions.push((wrap_id, reduction.clone()));
                            reduction
                        }
                    }
                };
                let Ok(reduction) = reduction.downcast::<Reduction<T, O>>() else { return };
                reduction.items.borrow_mut().push(i.clone());
                if data.buffers.borrow().is_empty() {
                    // Uma só função em buffer, que reduz todos os eventos quando o buffer fecha.
                    let (listener, reduce, initial) = (listener.clone(), reduce.clone(), initial.clone());
                    data.buffers.borrow_mut().push(Box::new(move || {
                        // Guarda o resultado para dividir o valor entre todos os listeners.
                        if reduction.reduced_result.borrow().is_none() {
                            let items = reduction.items.borrow();
                            let mut items = items.iter();
                            let mut acc = initial.clone().or_else(|| items.next().and_then(|first| seed.map(|seed| seed(first))));
                            for item in items {
                                acc = Some(reduce(acc, item));
                            }
                            *reduction.reduced_result.borrow_mut() = acc;
                        }
                        if let Some(result) = reduction.reduced_result.borrow().as_ref() {
                            listener(result);
                        }
                    }));
                }
            })
        })
    }

    /// `bufferEvents(fn)`.
    pub fn buffer_events<R>(&self, f: impl FnOnce() -> R) -> R {
        let data = Rc::new(BufferData { buffers: RefCell::new(Vec::new()), reductions: RefCell::new(Vec::new()) });
        self.data.borrow_mut().push(data.clone());
        let r = f();
        self.data.borrow_mut().pop();
        let buffers = std::mem::take(&mut *data.buffers.borrow_mut());
        for flush in buffers {
            flush();
        }
        r
    }
}

struct RelayState<T: 'static> {
    listening: Cell<bool>,
    input_event: RefCell<Event<T>>,
    input_event_listener: RefCell<DisposableRef>,
}

/// `Relay<T>`: um repassador de eventos com a origem trocável. Conecta-se uma `input` e ele repassa os
/// eventos dela pelo próprio `event`; a `input` pode mudar a qualquer hora.
pub struct Relay<T: Clone + 'static> {
    state: Rc<RelayState<T>>,
    emitter: Rc<Emitter<T>>,
}

impl<T: Clone + 'static> Relay<T> {
    pub fn new() -> Rc<Self> {
        let state = Rc::new(RelayState { listening: Cell::new(false), input_event: RefCell::new(Event::none()), input_event_listener: RefCell::new(lifecycle::none()) });
        let emitter = Emitter::with_options(EmitterOptions {
            on_did_add_first_listener: Some(Rc::new({
                let state = state.clone();
                move |emitter: &Emitter<T>| {
                    state.listening.set(true);
                    let input = state.input_event.borrow().clone();
                    *state.input_event_listener.borrow_mut() = input.listen(forwarding(emitter));
                }
            })),
            on_did_remove_last_listener: Some(Rc::new({
                let state = state.clone();
                move |_: &Emitter<T>| {
                    state.listening.set(false);
                    let listener = state.input_event_listener.borrow().clone();
                    listener.dispose();
                }
            })),
            ..EmitterOptions::default()
        });
        Rc::new(Self { state, emitter })
    }

    pub fn event(&self) -> Event<T> {
        self.emitter.event()
    }

    /// `set input(event)`.
    pub fn set_input(&self, event: Event<T>) {
        *self.state.input_event.borrow_mut() = event.clone();
        if self.state.listening.get() {
            let listener = self.state.input_event_listener.borrow().clone();
            listener.dispose();
            *self.state.input_event_listener.borrow_mut() = event.listen(forwarding(&self.emitter));
        }
    }
}

impl<T: Clone + 'static> IDisposable for Relay<T> {
    fn dispose(&self) {
        let listener = self.state.input_event_listener.borrow().clone();
        listener.dispose();
        self.emitter.dispose();
    }
}

/// `IValueWithChangeEvent<T>`.
pub trait ValueWithChangeEventLike<T> {
    fn on_did_change(&self) -> Event<()>;
    fn value(&self) -> T;
}

/// `ValueWithChangeEvent<T>`: um valor que dispara `onDidChange` ao mudar de verdade.
pub struct ValueWithChangeEvent<T: Clone + PartialEq + 'static> {
    on_did_change: Rc<Emitter<()>>,
    value: RefCell<T>,
}

impl<T: Clone + PartialEq + 'static> ValueWithChangeEvent<T> {
    pub fn new(value: T) -> Self {
        Self { on_did_change: Emitter::new(), value: RefCell::new(value) }
    }

    /// `ValueWithChangeEvent.const(value)`.
    pub fn constant(value: T) -> ConstValueWithChangeEvent<T> {
        ConstValueWithChangeEvent { value }
    }

    /// `set value(value)`.
    pub fn set(&self, value: T) {
        if value != *self.value.borrow() {
            *self.value.borrow_mut() = value;
            self.on_did_change.fire(());
        }
    }
}

impl<T: Clone + PartialEq + 'static> ValueWithChangeEventLike<T> for ValueWithChangeEvent<T> {
    fn on_did_change(&self) -> Event<()> {
        self.on_did_change.event()
    }

    fn value(&self) -> T {
        self.value.borrow().clone()
    }
}

/// `ConstValueWithChangeEvent<T>`: nunca muda, então o evento é `Event.None`.
pub struct ConstValueWithChangeEvent<T> {
    value: T,
}

impl<T: Clone> ValueWithChangeEventLike<T> for ConstValueWithChangeEvent<T> {
    fn on_did_change(&self) -> Event<()> {
        Event::none()
    }

    fn value(&self) -> T {
        self.value.clone()
    }
}

/// `trackSetChanges`: chama `handle_item` uma vez para cada item novo do conjunto, e descarta o que ele
/// devolveu quando o item sai do conjunto. O conjunto chega como a lista dos itens distintos, na ordem.
pub fn track_set_changes<T: Eq + Hash + Clone + 'static, D: 'static>(
    get_data: impl Fn() -> Vec<T> + 'static,
    on_did_change_data: &Event<D>,
    handle_item: impl Fn(&T) -> DisposableRef + 'static,
) -> DisposableRef {
    let map = DisposableMap::<T, dyn IDisposable>::new();
    let old_data: Rc<RefCell<Vec<T>>> = Rc::new(RefCell::new(get_data()));
    let handle_item = Rc::new(handle_item);
    for d in old_data.borrow().iter() {
        map.set(d.clone(), handle_item(d), false);
    }

    let store = DisposableStore::new();
    store.add(on_did_change_data.listen({
        let map = map.clone();
        move |_| {
            let new_data = get_data();
            let diff = diff_sets(&old_data.borrow(), &new_data);
            for r in &diff.removed {
                map.delete_and_dispose(r);
            }
            for a in &diff.added {
                map.set(a.clone(), handle_item(a), false);
            }
            *old_data.borrow_mut() = new_data;
        }
    }));
    store.add(map);
    store
}

// #region Event.fromObservable / Event.fromObservableLight

/// `EmitterObserver<T>` do `event.ts`: o observador que dispara o emitter quando o observável muda.
struct EmitterObserver<T: Clone + 'static> {
    observable: crate::observable::base::IObservable<T>,
    emitter: RefCell<Weak<Emitter<T>>>,
    counter: Cell<i64>,
    has_changed: Cell<bool>,
}

impl<T: Clone + 'static> crate::observable::base::IObserver for EmitterObserver<T> {
    fn begin_update(&self, _observable: &dyn crate::observable::base::AnyObservable) {
        self.counter.set(self.counter.get() + 1);
    }

    fn handle_possible_change(&self, _observable: &dyn crate::observable::base::AnyObservable) {}

    fn handle_change(&self, _observable: &dyn crate::observable::base::AnyObservable, _change: &dyn Any) {
        self.has_changed.set(true);
    }

    fn end_update(&self, _observable: &dyn crate::observable::base::AnyObservable) {
        self.counter.set(self.counter.get() - 1);
        if self.counter.get() == 0 {
            crate::observable::base::AnyObservable::report_changes(&*self.observable);
            if self.has_changed.get() {
                self.has_changed.set(false);
                let emitter = self.emitter.borrow().upgrade();
                if let Some(emitter) = emitter {
                    emitter.fire(self.observable.get());
                }
            }
        }
    }
}

impl<T: Clone + 'static> Event<T> {
    /// `Event.fromObservable(obs, store)`: um emitter que dispara quando o observável muda, e cada
    /// listener se inscreve no emitter. O `_addLeakageTraceLogic` do original (`false` na constante
    /// `_enableSnapshotPotentialLeakWarning`) não existe.
    pub fn from_observable(obs: crate::observable::base::IObservable<T>, store: Option<&DisposableStore>) -> Event<T> {
        use crate::observable::base::ObserverRef;
        let observer = Rc::new(EmitterObserver {
            observable: obs.clone(),
            emitter: RefCell::new(Weak::new()),
            counter: Cell::new(0),
            has_changed: Cell::new(false),
        });
        let (on_add, on_remove) = (observer.clone(), observer.clone());
        let options = EmitterOptions {
            on_will_add_first_listener: Some(Rc::new(move |_: &Emitter<T>| {
                on_add.observable.add_observer(on_add.clone() as ObserverRef);
                // Avisa o observável de que recebemos o valor atual e queremos as mudanças futuras.
                on_add.observable.report_changes();
            })),
            on_did_remove_last_listener: Some(Rc::new(move |_: &Emitter<T>| {
                on_remove.observable.remove_observer(&(on_remove.clone() as ObserverRef));
            })),
            ..EmitterOptions::default()
        };
        let emitter = Emitter::with_options(options);
        *observer.emitter.borrow_mut() = Rc::downgrade(&emitter);
        if let Some(store) = store {
            store.add(emitter.clone());
        }
        emitter.event()
    }
}

/// O observador de `Event.fromObservableLight`.
struct LightObserver {
    observable: crate::observable::base::ObservableRef,
    count: Cell<i64>,
    did_change: Cell<bool>,
    listener: Listener<()>,
}

impl crate::observable::base::IObserver for LightObserver {
    fn begin_update(&self, _observable: &dyn crate::observable::base::AnyObservable) {
        self.count.set(self.count.get() + 1);
    }

    fn end_update(&self, _observable: &dyn crate::observable::base::AnyObservable) {
        self.count.set(self.count.get() - 1);
        if self.count.get() == 0 {
            self.observable.report_changes();
            if self.did_change.get() {
                self.did_change.set(false);
                (self.listener)(&());
            }
        }
    }

    fn handle_possible_change(&self, _observable: &dyn crate::observable::base::AnyObservable) {}

    fn handle_change(&self, _observable: &dyn crate::observable::base::AnyObservable, _change: &dyn Any) {
        self.did_change.set(true);
    }
}

impl Event<()> {
    /// `Event.fromObservableLight(observable)`: cada listener se inscreve direto no observável.
    pub fn from_observable_light(observable: crate::observable::base::ObservableRef) -> Event<()> {
        Event::new(move |listener: Listener<()>| {
            use crate::observable::base::ObserverRef;
            let observer: ObserverRef = Rc::new(LightObserver { observable: observable.clone(), count: Cell::new(0), did_change: Cell::new(false), listener });
            observable.add_observer(observer.clone());
            observable.report_changes();
            let observable = observable.clone();
            lifecycle::plain_disposable(move || observable.remove_observer(&observer))
        })
    }
}

// #endregion
