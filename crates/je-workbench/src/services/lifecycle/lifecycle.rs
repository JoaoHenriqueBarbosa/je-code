//! Porte de `src/vs/workbench/services/lifecycle/common/lifecycle.ts`: o `ILifecycleService`, as fases do ciclo de vida
//! e os eventos de desligamento.
//!
//! Divergências:
//! - **Propriedades** viram métodos. `phase` (leitura e escrita no original) são [`ILifecycleService::phase`] e
//!   [`ILifecycleService::set_phase`]; `willShutdown` e `startupKind` são métodos; os eventos devolvem o [`Event`].
//! - **Interfaces de evento com métodos** (`BeforeShutdownEvent.veto`, `WillShutdownEvent.join`) são structs
//!   construídas por quem dispara o evento (o serviço de ciclo de vida real ou um dublê de teste). Elas guardam o
//!   estado que o original deixa no objeto literal: as vetos acumuladas em [`BeforeShutdownEvent`] e as funções de
//!   `join`, `joiners` e `force` em [`WillShutdownEvent`].
//! - `veto(boolean | Promise<boolean>, id)` recebe [`MaybePromise`]`<bool>`.
//! - `InternalBeforeShutdownEvent extends BeforeShutdownEvent` é a struct [`InternalBeforeShutdownEvent`], que guarda o
//!   [`BeforeShutdownEvent`] base e o `finalVeto`.
//! - A sobrecarga `join(promise, defaultJoiner)` / `join(promiseFn, lastJoiner)` são [`WillShutdownEvent::join`] e
//!   [`WillShutdownEvent::join_last`], e os joiners tipados por ordem (`IWillShutdownEventDefaultJoiner` e
//!   `IWillShutdownEventLastJoiner`) são o [`WillShutdownEventJoiner`] com a `order` correspondente.
//! - `LifecyclePhase` deriva a ordem numérica do original (`Starting < Ready < Restored < Eventually`).

use std::cell::RefCell;
use std::rc::Rc;

use je_base::cancellation::CancellationToken;
use je_base::errors::Error;
use je_base::event::Event;
use je_base::promise::{MaybePromise, Promise};
use je_instantiation::service_id;

/// `ShutdownReason`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShutdownReason {
    /// The window is closed.
    Close = 1,
    /// The window closes because the application quits.
    Quit = 2,
    /// The window is reloaded.
    Reload = 3,
    /// The window is loaded into a different workspace context.
    Load = 4,
}

/// `StartupKind`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StartupKind {
    NewWindow = 1,
    ReloadedWindow = 3,
    ReopenedWindow = 4,
}

/// `StartupKindToString`.
pub fn startup_kind_to_string(startup_kind: StartupKind) -> &'static str {
    match startup_kind {
        StartupKind::NewWindow => "NewWindow",
        StartupKind::ReloadedWindow => "ReloadedWindow",
        StartupKind::ReopenedWindow => "ReopenedWindow",
    }
}

/// `LifecyclePhase`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum LifecyclePhase {
    /// The first phase signals that we are about to startup getting ready.
    ///
    /// Note: doing work in this phase blocks an editor from showing to the user, so please rather consider to use
    /// `Restored` phase.
    Starting = 1,
    /// Services are ready and the window is about to restore its UI state.
    ///
    /// Note: doing work in this phase blocks an editor from showing to the user, so please rather consider to use
    /// `Restored` phase.
    Ready = 2,
    /// Views, panels and editors have restored. Editors are given a bit of time to restore their contents.
    Restored = 3,
    /// The last phase after views, panels and editors have restored and some time has passed (2-5 seconds).
    Eventually = 4,
}

/// `LifecyclePhaseToString`.
pub fn lifecycle_phase_to_string(phase: LifecyclePhase) -> &'static str {
    match phase {
        LifecyclePhase::Starting => "Starting",
        LifecyclePhase::Ready => "Ready",
        LifecyclePhase::Restored => "Restored",
        LifecyclePhase::Eventually => "Eventually",
    }
}

/// Uma veto acumulada por [`BeforeShutdownEvent::veto`]: o valor (direto ou promessa) e o identificador.
pub type ShutdownVeto = (MaybePromise<bool>, String);

/// `BeforeShutdownEvent`: enviado quando a janela está para fechar. Os clientes podem vetar o fechamento chamando
/// [`BeforeShutdownEvent::veto`] com `true` direto ou com uma promessa que resolve para um booleano.
///
/// Note: It is absolutely important to avoid long running promises if possible. Please try hard to return a boolean
/// directly. Returning a promise has quite an impact on the shutdown sequence!
#[derive(Clone)]
pub struct BeforeShutdownEvent {
    /// The reason why the application will be shutting down.
    pub reason: ShutdownReason,
    vetos: Rc<RefCell<Vec<ShutdownVeto>>>,
}

impl BeforeShutdownEvent {
    pub fn new(reason: ShutdownReason) -> Self {
        Self { reason, vetos: Rc::new(RefCell::new(Vec::new())) }
    }

    /// Allows to veto the shutdown. The veto can be a long running operation but it will block the application from
    /// closing. `id` identifies the veto operation in case it takes very long or never completes.
    pub fn veto(&self, value: MaybePromise<bool>, id: &str) {
        self.vetos.borrow_mut().push((value, id.to_string()));
    }

    /// As vetos registradas até agora (para quem dispara o evento decidir o desligamento).
    pub fn vetos(&self) -> Vec<ShutdownVeto> {
        self.vetos.borrow().clone()
    }
}

/// A função de `InternalBeforeShutdownEvent.finalVeto`.
pub type FinalVetoFn = Rc<dyn Fn() -> MaybePromise<bool>>;

/// `InternalBeforeShutdownEvent`.
#[derive(Clone)]
pub struct InternalBeforeShutdownEvent {
    /// O `BeforeShutdownEvent` que esta versão interna estende.
    pub event: BeforeShutdownEvent,
    final_veto: Rc<RefCell<Option<(FinalVetoFn, String)>>>,
}

impl InternalBeforeShutdownEvent {
    pub fn new(reason: ShutdownReason) -> Self {
        Self { event: BeforeShutdownEvent::new(reason), final_veto: Rc::new(RefCell::new(None)) }
    }

    /// Allows to set a veto operation to run after all other vetos have been handled from the `BeforeShutdownEvent`.
    ///
    /// This method is hidden from the API because it is intended to be only used once internally.
    pub fn final_veto(&self, veto_fn: FinalVetoFn, id: &str) {
        *self.final_veto.borrow_mut() = Some((veto_fn, id.to_string()));
    }

    /// A veto final registrada, se houver.
    pub fn get_final_veto(&self) -> Option<(FinalVetoFn, String)> {
        self.final_veto.borrow().clone()
    }
}

/// `BeforeShutdownErrorEvent`: an event that signals an error happened during `onBeforeShutdown` veto handling. In
/// this case the shutdown operation will not proceed because this is an unexpected condition that is treated like a
/// veto.
#[derive(Clone)]
pub struct BeforeShutdownErrorEvent {
    /// The reason why the application is shutting down.
    pub reason: ShutdownReason,
    /// The error that happened during shutdown handling.
    pub error: Error,
}

/// `WillShutdownJoinerOrder`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WillShutdownJoinerOrder {
    /// Joiners to run before the `Last` joiners. This is the default order and best for most cases. You can be sure
    /// that services are still functional at this point.
    Default = 1,
    /// The joiners to run last. This should ONLY be used in rare cases when you have no dependencies to workbench
    /// services or state. The workbench may be in a state where resources can no longer be accessed or changed.
    Last = 2,
}

/// `IWillShutdownEventJoiner` (e as variantes `IWillShutdownEventDefaultJoiner` e `IWillShutdownEventLastJoiner`, que
/// só fixam a `order`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WillShutdownEventJoiner {
    pub id: String,
    pub label: String,
    pub order: Option<WillShutdownJoinerOrder>,
}

/// O que `WillShutdownEvent.join` recebe: uma promessa (joiner padrão) ou uma função que a produz (joiner `Last`).
#[derive(Clone)]
pub enum WillShutdownJoin {
    Promise(Promise<()>),
    Function(Rc<dyn Fn() -> Promise<()>>),
}

/// `WillShutdownEvent`: enviado quando a janela fecha. Os clientes podem participar do fechamento entregando uma
/// promessa a [`WillShutdownEvent::join`].
///
/// Note: It is absolutely important to avoid long running promises if possible. Please try hard to return a boolean
/// directly. Returning a promise has quite an impact on the shutdown sequence!
#[derive(Clone)]
pub struct WillShutdownEvent {
    /// The reason why the application is shutting down.
    pub reason: ShutdownReason,
    /// A token that will signal cancellation when the shutdown was forced by the user.
    pub token: CancellationToken,
    join: Rc<dyn Fn(WillShutdownJoin, WillShutdownEventJoiner)>,
    joiners: Rc<dyn Fn() -> Vec<WillShutdownEventJoiner>>,
    force: Rc<dyn Fn()>,
}

impl WillShutdownEvent {
    /// Quem dispara o evento entrega o que o original deixa no objeto literal: o que fazer com cada `join`, quais
    /// joiners ainda não terminaram e como forçar o desligamento.
    pub fn new(
        reason: ShutdownReason,
        token: CancellationToken,
        join: Rc<dyn Fn(WillShutdownJoin, WillShutdownEventJoiner)>,
        joiners: Rc<dyn Fn() -> Vec<WillShutdownEventJoiner>>,
        force: Rc<dyn Fn()>,
    ) -> Self {
        Self { reason, token, join, joiners, force }
    }

    /// Allows to join the shutdown. The promise can be a long running operation but it will block the application from
    /// closing. `joiner` identifies the join operation in case it takes very long or never completes.
    pub fn join(&self, promise: Promise<()>, joiner: WillShutdownEventJoiner) {
        (self.join)(WillShutdownJoin::Promise(promise), joiner);
    }

    /// Allows to join the shutdown at the end. The promise can be a long running operation but it will block the
    /// application from closing. `joiner` identifies the join operation in case it takes very long or never completes.
    pub fn join_last(&self, promise_fn: Rc<dyn Fn() -> Promise<()>>, joiner: WillShutdownEventJoiner) {
        (self.join)(WillShutdownJoin::Function(promise_fn), joiner);
    }

    /// Allows to access the joiners that have not finished joining this event.
    pub fn joiners(&self) -> Vec<WillShutdownEventJoiner> {
        (self.joiners)()
    }

    /// Allows to enforce the shutdown, even when there are pending `join` operations to complete.
    pub fn force(&self) {
        (self.force)();
    }
}

/// `ILifecycleService`: informa sobre os eventos do ciclo de vida da aplicação, como o desligamento.
pub trait ILifecycleService {
    /// Value indicates how this window got loaded.
    fn startup_kind(&self) -> StartupKind;

    /// A flag indicating in what phase of the lifecycle we currently are.
    fn phase(&self) -> LifecyclePhase;

    /// `phase = value`.
    fn set_phase(&self, phase: LifecyclePhase);

    /// Fired before shutdown happens. Allows listeners to veto against the shutdown to prevent it from happening. The
    /// event carries a shutdown reason that indicates how the shutdown was triggered.
    fn on_before_shutdown(&self) -> Event<BeforeShutdownEvent>;

    /// Fired when the shutdown was prevented by a component giving veto.
    fn on_shutdown_veto(&self) -> Event<()>;

    /// Fired when an error happened during `onBeforeShutdown` veto handling. In this case the shutdown operation will
    /// not proceed because this is an unexpected condition that is treated like a veto.
    fn on_before_shutdown_error(&self) -> Event<BeforeShutdownErrorEvent>;

    /// Fired when no client is preventing the shutdown from happening (from `onBeforeShutdown`). This event can be
    /// joined with a long running operation via `WillShutdownEvent#join()` to handle long running shutdown operations.
    fn on_will_shutdown(&self) -> Event<WillShutdownEvent>;

    /// A flag indicating that we are about to shutdown without further veto.
    fn will_shutdown(&self) -> bool;

    /// Fired when the shutdown is about to happen after long running shutdown operations have finished (from
    /// `onWillShutdown`). This event should be used to dispose resources.
    fn on_did_shutdown(&self) -> Event<()>;

    /// Returns a promise that resolves when a certain lifecycle phase has started.
    fn when(&self, phase: LifecyclePhase) -> Promise<()>;

    /// Triggers a shutdown of the workbench. Depending on native or web, this can have different implementations and
    /// behaviour.
    ///
    /// **Note:** this should normally not be called. See related methods in `IHostService` and `INativeHostService` to
    /// close a window or quit the application.
    fn shutdown(&self) -> Promise<()>;
}

service_id!(pub I_LIFECYCLE_SERVICE: dyn ILifecycleService = "lifecycleService");
