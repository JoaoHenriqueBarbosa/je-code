//! Porte de `src/vs/editor/contrib/hover/browser/hoverOperation.ts`: a [`HoverOperation`], que decide quando calcular o
//! conteúdo de um hover (o `IHoverComputer`), e os tipos que a cercam ([`HoverStartMode`], [`HoverStartSource`],
//! [`HoverResult`]).
//!
//! Notas de tradução:
//! - `computeAsync?` e `computeSync?` são opcionais no `IHoverComputer`; aqui são métodos de [`IHoverComputer`] com o
//!   corpo padrão `None`, que é o membro ausente. `computeAsync` devolve o `AsyncIterableProducer` do original como
//!   `Rc<dyn AsyncIterable<TResult>>` (os produtores de `je-base`), e o `CancellationToken` chega por referência.
//! - `HoverOperation extends Disposable` é a struct com um [`Disposable`] dentro. `this._register(new Emitter())` e
//!   `this._register(new Debouncer(...))` registram no `Disposable` de dentro. O `Debouncer` é privado, como no
//!   original, e usa o [`RunOnceScheduler`] de `je-base`.
//! - Os fechamentos que o original passa aos `Debouncer` (`(options) => this._triggerAsyncComputation(options)`)
//!   seguram uma referência fraca para a operação, para a operação e os agendadores não formarem ciclo.
//! - `createCancelableAsyncIterableProducer(token => this._computer.computeAsync!(options, token))` só roda quando o
//!   computador tem `computeAsync`. O produtor de `je-base` chama o fechamento na hora, então a ausência do método
//!   (`None`) é anotada no próprio fechamento: nesse caso o produtor vazio é descartado, `_asyncIterableDone` vira
//!   `true` e `_asyncIterable` fica como estava, que é o que o ramo `else` do original faz.
//! - O `(async () => { try { for await ... } catch (e) { onUnexpectedError(e); } })()` é um [`async_fn`] (o corpo roda
//!   de forma síncrona até o primeiro `await`, como a função `async` do JavaScript). `if (item)` não existe: um
//!   `TResult` nunca é nulo.
//! - O atraso do hover (`EditorOption.hover.delay`, em milissegundos) é `f64`; ele vira o `u64` dos temporizadores
//!   truncando, como o `setTimeout` do JavaScript.

use std::cell::{Cell, RefCell};
use std::ops::ControlFlow;
use std::rc::{Rc, Weak};

use je_base::async_::{AsyncIterable, AsyncIterableProducer, CancelableAsyncIterableProducer, RunOnceScheduler, async_fn, create_cancelable_async_iterable_producer, for_await};
use je_base::cancellation::CancellationToken;
use je_base::errors::on_unexpected_error;
use je_base::event::{Emitter, Event};
use je_base::lifecycle::{Disposable, IDisposable};

use crate::code_editor_widget::{CodeEditorWidget, ICodeEditor};
use crate::config::editor_options::hover::HOVER;

/// `IHoverComputer<TArgs, TResult>`.
pub trait IHoverComputer<TArgs, TResult: Clone + 'static> {
    /// `computeAsync?`: chamado depois de metade do tempo do hover. `None` é o método ausente.
    fn compute_async(&self, _args: &TArgs, _token: &CancellationToken) -> Option<Rc<dyn AsyncIterable<TResult>>> {
        None
    }

    /// `computeSync?`: chamado depois de todo o tempo do hover. `None` é o método ausente.
    fn compute_sync(&self, _args: &TArgs) -> Option<Vec<TResult>> {
        None
    }
}

/// `HoverOperationState`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum HoverOperationState {
    Idle = 0,
    FirstWait = 1,
    SecondWait = 2,
    WaitingForAsync = 3,
    WaitingForAsyncShowingLoading = 4,
}

/// `HoverStartMode`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HoverStartMode {
    Delayed = 0,
    Immediate = 1,
}

/// `HoverStartSource`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HoverStartSource {
    Mouse = 0,
    Click = 1,
    Keyboard = 2,
}

/// `HoverResult<TArgs, TResult>`.
#[derive(Clone)]
pub struct HoverResult<TArgs, TResult> {
    pub value: Vec<TResult>,
    pub is_complete: bool,
    pub has_loading_message: bool,
    pub options: TArgs,
}

/// Os milissegundos de um atraso, como o `setTimeout` os lê.
fn to_millis(delay: f64) -> u64 {
    delay.max(0.0) as u64
}

/// Computing the hover is very fine tuned.
///
/// Suppose the hover delay is 300ms (the default). Then, when resting the mouse at an anchor:
/// - at 150ms, the async computation is triggered (i.e. semantic hover)
///   - if async results already come in, they are not rendered yet.
/// - at 300ms, the sync computation is triggered (i.e. decorations, markers)
///   - if there are sync or async results, they are rendered.
/// - at 900ms, if the async computation hasn't finished, a "Loading..." result is added.
pub struct HoverOperation<TArgs: Clone + 'static, TResult: Clone + 'static> {
    base: Rc<Disposable>,
    me: Weak<Self>,
    editor: Rc<CodeEditorWidget>,
    computer: Rc<dyn IHoverComputer<TArgs, TResult>>,
    on_result: Rc<Emitter<HoverResult<TArgs, TResult>>>,
    async_computation_scheduler: Rc<Debouncer<TArgs>>,
    sync_computation_scheduler: Rc<Debouncer<TArgs>>,
    loading_message_scheduler: Rc<Debouncer<TArgs>>,
    state: Cell<HoverOperationState>,
    async_iterable: RefCell<Option<Rc<CancelableAsyncIterableProducer<TResult>>>>,
    async_iterable_done: Cell<bool>,
    result: RefCell<Vec<TResult>>,
    options: RefCell<Option<TArgs>>,
}

impl<TArgs: Clone + 'static, TResult: Clone + 'static> HoverOperation<TArgs, TResult> {
    pub fn new(editor: Rc<CodeEditorWidget>, computer: Rc<dyn IHoverComputer<TArgs, TResult>>) -> Rc<Self> {
        let base = Disposable::new();
        let on_result: Rc<Emitter<HoverResult<TArgs, TResult>>> = Emitter::new();
        base.register(on_result.clone());

        Rc::new_cyclic(|me: &Weak<Self>| {
            let new_debouncer = |trigger: fn(&Self, TArgs)| {
                let me = me.clone();
                let debouncer = Debouncer::new(
                    move |options: TArgs| {
                        if let Some(this) = me.upgrade() {
                            trigger(&this, options);
                        }
                    },
                    0,
                );
                base.register(debouncer.clone());
                debouncer
            };
            let async_computation_scheduler = new_debouncer(Self::trigger_async_computation);
            let sync_computation_scheduler = new_debouncer(Self::trigger_sync_computation);
            let loading_message_scheduler = new_debouncer(Self::trigger_loading_message);

            Self {
                base,
                me: me.clone(),
                editor,
                computer,
                on_result,
                async_computation_scheduler,
                sync_computation_scheduler,
                loading_message_scheduler,
                state: Cell::new(HoverOperationState::Idle),
                async_iterable: RefCell::new(None),
                async_iterable_done: Cell::new(false),
                result: RefCell::new(Vec::new()),
                options: RefCell::new(None),
            }
        })
    }

    /// `onResult`.
    pub fn on_result(&self) -> Event<HoverResult<TArgs, TResult>> {
        self.on_result.event()
    }

    /// `_hoverTime`.
    fn hover_time(&self) -> f64 {
        self.editor.get_option(HOVER).delay
    }

    /// `_firstWaitTime`.
    fn first_wait_time(&self) -> f64 {
        self.hover_time() / 2.0
    }

    /// `_secondWaitTime`.
    fn second_wait_time(&self) -> f64 {
        self.hover_time() - self.first_wait_time()
    }

    /// `_loadingMessageTime`.
    fn loading_message_time(&self) -> f64 {
        3.0 * self.hover_time()
    }

    /// `_setState(state, options)`.
    fn set_state(&self, state: HoverOperationState, options: TArgs) {
        *self.options.borrow_mut() = Some(options.clone());
        self.state.set(state);
        self.fire_result(options);
    }

    /// `_triggerAsyncComputation(options)`.
    fn trigger_async_computation(&self, options: TArgs) {
        self.set_state(HoverOperationState::SecondWait, options.clone());
        self.sync_computation_scheduler.schedule(options.clone(), to_millis(self.second_wait_time()));

        let computer = Rc::clone(&self.computer);
        let compute_async_absent = Rc::new(Cell::new(false));
        let producer = create_cancelable_async_iterable_producer({
            let (compute_options, compute_async_absent) = (options.clone(), Rc::clone(&compute_async_absent));
            move |token: CancellationToken| -> Rc<dyn AsyncIterable<TResult>> {
                match computer.compute_async(&compute_options, &token) {
                    Some(iterable) => iterable,
                    None => {
                        compute_async_absent.set(true);
                        AsyncIterableProducer::<TResult>::empty()
                    }
                }
            }
        });

        if compute_async_absent.get() {
            self.async_iterable_done.set(true);
            return;
        }

        self.async_iterable_done.set(false);
        let producer = Rc::new(producer);
        *self.async_iterable.borrow_mut() = Some(Rc::clone(&producer));

        let weak = self.me.clone();
        let _ = async_fn(async move {
            let outcome = for_await(&*producer, |item: TResult| {
                if let Some(this) = weak.upgrade() {
                    this.result.borrow_mut().push(item);
                    this.fire_result(options.clone());
                }
                Ok(ControlFlow::Continue(()))
            })
            .await;
            match outcome {
                Ok(()) => {
                    if let Some(this) = weak.upgrade() {
                        this.async_iterable_done.set(true);

                        if matches!(this.state.get(), HoverOperationState::WaitingForAsync | HoverOperationState::WaitingForAsyncShowingLoading) {
                            this.set_state(HoverOperationState::Idle, options.clone());
                        }
                    }
                }
                Err(error) => on_unexpected_error(&error),
            }
            Ok(())
        });
    }

    /// `_triggerSyncComputation(options)`.
    fn trigger_sync_computation(&self, options: TArgs) {
        if let Some(computed) = self.computer.compute_sync(&options) {
            self.result.borrow_mut().extend(computed);
        }
        let state = if self.async_iterable_done.get() { HoverOperationState::Idle } else { HoverOperationState::WaitingForAsync };
        self.set_state(state, options);
    }

    /// `_triggerLoadingMessage(options)`.
    fn trigger_loading_message(&self, options: TArgs) {
        if self.state.get() == HoverOperationState::WaitingForAsync {
            self.set_state(HoverOperationState::WaitingForAsyncShowingLoading, options);
        }
    }

    /// `_fireResult(options)`.
    fn fire_result(&self, options: TArgs) {
        let state = self.state.get();
        if state == HoverOperationState::FirstWait || state == HoverOperationState::SecondWait {
            // Do not send out results before the hover time
            return;
        }
        let is_complete = state == HoverOperationState::Idle;
        let has_loading_message = state == HoverOperationState::WaitingForAsyncShowingLoading;
        let value = self.result.borrow().clone();
        self.on_result.fire(HoverResult { value, is_complete, has_loading_message, options });
    }

    /// `start(mode, options)`.
    pub fn start(&self, mode: HoverStartMode, options: TArgs) {
        match mode {
            HoverStartMode::Delayed => {
                if self.state.get() == HoverOperationState::Idle {
                    self.set_state(HoverOperationState::FirstWait, options.clone());
                    self.async_computation_scheduler.schedule(options.clone(), to_millis(self.first_wait_time()));
                    self.loading_message_scheduler.schedule(options, to_millis(self.loading_message_time()));
                }
            }
            HoverStartMode::Immediate => match self.state.get() {
                HoverOperationState::Idle => {
                    self.trigger_async_computation(options.clone());
                    self.sync_computation_scheduler.cancel();
                    self.trigger_sync_computation(options);
                }
                HoverOperationState::SecondWait => {
                    self.sync_computation_scheduler.cancel();
                    self.trigger_sync_computation(options);
                }
                _ => {}
            },
        }
    }

    /// `cancel()`.
    pub fn cancel(&self) {
        self.async_computation_scheduler.cancel();
        self.sync_computation_scheduler.cancel();
        self.loading_message_scheduler.cancel();
        self.cancel_async_iterable();
        self.result.borrow_mut().clear();
        *self.options.borrow_mut() = None;
        self.state.set(HoverOperationState::Idle);
    }

    /// `if (this._asyncIterable) { this._asyncIterable.cancel(); this._asyncIterable = null; }`.
    fn cancel_async_iterable(&self) {
        let async_iterable = self.async_iterable.borrow_mut().take();
        if let Some(async_iterable) = async_iterable {
            async_iterable.cancel();
        }
    }

    /// `get options()`.
    pub fn options(&self) -> Option<TArgs> {
        self.options.borrow().clone()
    }
}

impl<TArgs: Clone + 'static, TResult: Clone + 'static> IDisposable for HoverOperation<TArgs, TResult> {
    fn dispose(&self) {
        self.cancel_async_iterable();
        *self.options.borrow_mut() = None;
        self.base.dispose();
    }

    fn constructor_name(&self) -> &'static str {
        "HoverOperation"
    }
}

/// `Debouncer<TArgs>`: um [`RunOnceScheduler`] que guarda os argumentos da última chamada de `schedule` e os entrega ao
/// `runner`.
struct Debouncer<TArgs: Clone + 'static> {
    base: Rc<Disposable>,
    scheduler: Rc<RunOnceScheduler>,
    options: Rc<RefCell<Option<TArgs>>>,
}

impl<TArgs: Clone + 'static> Debouncer<TArgs> {
    fn new(runner: impl Fn(TArgs) + 'static, debounce_time_ms: u64) -> Rc<Self> {
        let base = Disposable::new();
        let options: Rc<RefCell<Option<TArgs>>> = Rc::new(RefCell::new(None));
        let scheduler = RunOnceScheduler::new(
            {
                let options = Rc::clone(&options);
                move || {
                    let current = options.borrow().clone();
                    runner(current.expect("o Debouncer só dispara depois de schedule"));
                }
            },
            debounce_time_ms,
        );
        base.register(scheduler.clone());
        Rc::new(Self { base, scheduler, options })
    }

    fn schedule(&self, options: TArgs, debounce_time_ms: u64) {
        *self.options.borrow_mut() = Some(options);
        self.scheduler.schedule(Some(debounce_time_ms));
    }

    fn cancel(&self) {
        self.scheduler.cancel();
    }
}

impl<TArgs: Clone + 'static> IDisposable for Debouncer<TArgs> {
    fn dispose(&self) {
        self.base.dispose();
    }
}
