//! Porte de `src/vs/base/common/decorators.ts` e de `decorators/cancelPreviousCalls.ts`.
//!
//! O Rust não tem decorator de método. Cada decorator do original vira um tipo que o dono do método
//! guarda como campo e usa no lugar da chave `$memoize$nome` (e das chaves `$debounce$...` e
//! `$throttle$...`) que o original grava no próprio objeto:
//!
//! - `@memoize` é [`Memoize`];
//! - `@debounce(delay, reducer, initial)` é [`Debounce`];
//! - `@throttle(delay, reducer, initial)` é [`Throttle`];
//! - `@cancelPreviousCalls` é [`CancelPreviousCalls`].
//!
//! Diferença de forma: o valor acumulado do `reducer` é um `Option` (o `undefined` do original). O
//! original reinicia o acumulado com o teste `!valor`, o que também reinicia `0`, `""` e `false`;
//! aqui só o `None` reinicia, e a diferença só aparece se o acumulado for um valor falso que não é o
//! valor inicial, o que o `initialValueProvider` de um acumulador nunca produz.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use crate::cancellation::{CancellationToken, CancellationTokenSource};
use crate::lifecycle::IDisposable;
use crate::runtime::{self, TimerId};

/// `@memoize`: o resultado é calculado na primeira leitura e guardado, por instância. Quem lança não
/// deixa resultado guardado, então a próxima leitura tenta de novo.
pub struct Memoize<T>(std::cell::OnceCell<T>);

impl<T> Default for Memoize<T> {
    fn default() -> Self {
        Self(std::cell::OnceCell::new())
    }
}

impl<T> Memoize<T> {
    pub fn new() -> Self {
        Self::default()
    }

    /// Devolve o valor guardado; na primeira chamada, guarda o resultado de `compute`.
    pub fn get(&self, compute: impl FnOnce() -> T) -> &T {
        self.0.get_or_init(compute)
    }
}

/// `IDebounceReducer<T>`: `(previousValue, ...args) => T`, com os argumentos da chamada reunidos em `A`.
pub type Reducer<A, T> = Rc<dyn Fn(Option<T>, A) -> T>;
type Initial<T> = Option<Rc<dyn Fn() -> T>>;

struct DebounceState<A, T> {
    delay: u64,
    reducer: Reducer<A, T>,
    initial: Initial<T>,
    run: Rc<dyn Fn(T)>,
    timer: Cell<Option<TimerId>>,
    result: RefCell<Option<T>>,
}

/// `@debounce(delay, reducer, initialValueProvider)`: cada chamada reinicia o timer; quando ele vence,
/// o método roda uma vez com o valor acumulado. `A` são os argumentos da chamada e `T` o que o método
/// recebe.
pub struct Debounce<A: 'static, T: Clone + 'static>(Rc<DebounceState<A, T>>);

impl<A: 'static, T: Clone + 'static> Debounce<A, T> {
    /// Com `reducer`, o método recebe o valor acumulado (`args = [this[resultKey]]` do original).
    pub fn new(
        delay: u64,
        reducer: impl Fn(Option<T>, A) -> T + 'static,
        initial_value_provider: Option<Rc<dyn Fn() -> T>>,
        method: impl Fn(T) + 'static,
    ) -> Self {
        Self(Rc::new(DebounceState {
            delay,
            reducer: Rc::new(reducer),
            initial: initial_value_provider,
            run: Rc::new(method),
            timer: Cell::new(None),
            result: RefCell::new(None),
        }))
    }

    pub fn call(&self, args: A) {
        let state = &self.0;
        if state.result.borrow().is_none() {
            *state.result.borrow_mut() = state.initial.as_ref().map(|f| f());
        }
        if let Some(timer) = state.timer.take() {
            runtime::clear_timeout(timer);
        }
        let previous = state.result.borrow().clone();
        let value = (state.reducer)(previous, args);
        *state.result.borrow_mut() = Some(value.clone());
        let this = state.clone();
        state.timer.set(Some(runtime::set_timeout(
            move || {
                (this.run)(value);
                *this.result.borrow_mut() = this.initial.as_ref().map(|f| f());
            },
            state.delay,
        )));
    }
}

impl<A: Clone + 'static> Debounce<A, A> {
    /// Sem `reducer` o método recebe os argumentos da última chamada.
    pub fn without_reducer(delay: u64, method: impl Fn(A) + 'static) -> Self {
        Self::new(delay, |_, args| args, None, method)
    }
}

struct ThrottleState<A, T> {
    delay: u64,
    reducer: Reducer<A, T>,
    initial: Initial<T>,
    run: Rc<dyn Fn(T)>,
    result: RefCell<Option<T>>,
    last_run: Cell<f64>,
    pending: Cell<bool>,
}

/// `@throttle(delay, reducer, initialValueProvider)`: a primeira chamada roda já; as que chegam
/// dentro do intervalo se acumulam no `reducer` e rodam juntas, uma vez, quando o intervalo acaba.
pub struct Throttle<A: 'static, T: Clone + 'static>(Rc<ThrottleState<A, T>>);

impl<A: 'static, T: Clone + 'static> Throttle<A, T> {
    pub fn new(
        delay: u64,
        reducer: impl Fn(Option<T>, A) -> T + 'static,
        initial_value_provider: Option<Rc<dyn Fn() -> T>>,
        method: impl Fn(T) + 'static,
    ) -> Self {
        Self(Rc::new(ThrottleState {
            delay,
            reducer: Rc::new(reducer),
            initial: initial_value_provider,
            run: Rc::new(method),
            result: RefCell::new(None),
            // `-Number.MAX_VALUE`
            last_run: Cell::new(-f64::MAX),
            pending: Cell::new(false),
        }))
    }

    pub fn call(&self, args: A) {
        let state = &self.0;
        if state.result.borrow().is_none() {
            *state.result.borrow_mut() = state.initial.as_ref().map(|f| f());
        }
        let previous = state.result.borrow().clone();
        *state.result.borrow_mut() = Some((state.reducer)(previous, args));

        if state.pending.get() {
            return;
        }

        let now = runtime::date_now() as f64;
        let next_time = state.last_run.get() + state.delay as f64;
        if next_time <= now {
            state.last_run.set(now);
            Self::flush(state);
        } else {
            state.pending.set(true);
            let this = state.clone();
            runtime::set_timeout(
                move || {
                    this.pending.set(false);
                    this.last_run.set(runtime::date_now() as f64);
                    Self::flush(&this);
                },
                (next_time - now) as u64,
            );
        }
    }

    fn flush(state: &ThrottleState<A, T>) {
        let value = state.result.borrow().clone().expect("acumulado definido antes de rodar o método");
        (state.run)(value);
        *state.result.borrow_mut() = state.initial.as_ref().map(|f| f());
    }
}

/// `@cancelPreviousCalls`: cada chamada do método cancela o token entregue à chamada anterior e
/// entrega um token novo. Se a chamada já traz um token, o novo é filho dele (o `lastArgument` do
/// original), então cancelar o token de fora também cancela o novo.
///
/// O dono do método descarta o `CancelPreviousCalls` com ele (o `this._register` do original), o que
/// descarta o token em andamento sem cancelá-lo, como o `DisposableMap.dispose` do original.
#[derive(Default)]
pub struct CancelPreviousCalls {
    source: RefCell<Option<CancellationTokenSource>>,
}

impl CancelPreviousCalls {
    pub fn new() -> Self {
        Self::default()
    }

    /// Cancela a chamada anterior e devolve o token da chamada que começa.
    pub fn next_call(&self, token: Option<&CancellationToken>) -> CancellationToken {
        if let Some(previous) = self.source.borrow_mut().take() {
            previous.dispose_with(true);
        }
        let source = CancellationTokenSource::new(token);
        let new_token = source.token();
        *self.source.borrow_mut() = Some(source);
        new_token
    }
}

impl IDisposable for CancelPreviousCalls {
    fn dispose(&self) {
        if let Some(source) = self.source.borrow_mut().take() {
            source.dispose_with(false);
        }
    }
}
