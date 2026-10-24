//! Porte de `src/vs/workbench/services/lifecycle/common/lifecycleService.ts` (o `AbstractLifecycleService`) e de `handleVetos`
//! (`src/vs/platform/lifecycle/common/lifecycle.ts`), que o desligamento usa para juntar as vetos.
//!
//! Divergências:
//! - O Rust não tem herança: o `AbstractLifecycleService` é uma struct que o serviço concreto (`NativeLifecycleService`) guarda. As
//!   propriedades `protected` do original (`_onBeforeShutdown`, `_willShutdown`, `shutdownReason`, `logService`...) são campos públicos.
//! - `phase` (leitura e escrita) são [`AbstractLifecycleService::phase`] e [`AbstractLifecycleService::set_phase`], que lança `Lifecycle
//!   cannot go backwards` (`errors::throw`) se a fase recua.
//! - `doResolveStartupKind` não é sobrescrito por nenhum serviço portado, então é um método comum.
//! - O `lastShutdownReason` que o original grava como `this.shutdownReason` (possivelmente `undefined`) é um [`StorageValue::Number`] ou
//!   [`StorageValue::Undefined`].
//! - `mark(...)` do `performance.ts` é `je_base::performance::mark`.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use je_base::async_::{promises, Barrier};
use je_base::errors::{self, Error};
use je_base::event::Emitter;
use je_base::lifecycle::Disposable;
use je_base::performance::mark;
use je_base::promise::{MaybePromise, Promise};
use je_platform::log::{LogServiceRef, Logger};
use je_platform::storage::storage::{StorageService, StorageScope, StorageTarget, WillSaveStateReason};
use je_storage::StorageValue;

use super::lifecycle::{
    lifecycle_phase_to_string, BeforeShutdownErrorEvent, BeforeShutdownEvent, LifecyclePhase, ShutdownReason, StartupKind, WillShutdownEvent,
};

const LAST_SHUTDOWN_REASON_KEY: &str = "lifecyle.lastShutdownReason";

/// `handleVetos(vetos, onError)`: `true` se alguma veto vetou (ou falhou, o que também conta como veto).
pub fn handle_vetos(vetos: Vec<MaybePromise<bool>>, on_error: Rc<dyn Fn(&Error)>) -> Promise<bool> {
    if vetos.is_empty() {
        return Promise::resolved(false);
    }

    let mut pending: Vec<Promise<()>> = Vec::new();
    let lazy_value = Rc::new(Cell::new(false));
    for value_or_promise in vetos {
        match value_or_promise {
            // veto, done
            MaybePromise::Value(true) => return Promise::resolved(true),
            MaybePromise::Value(false) => {}
            MaybePromise::Promise(promise) => {
                let (lazy_value, on_error) = (lazy_value.clone(), on_error.clone());
                pending.push(promise.then_result(move |result| {
                    match result {
                        Ok(value) => {
                            if value {
                                lazy_value.set(true);
                            }
                        }
                        Err(error) => {
                            lazy_value.set(true);
                            on_error(&error);
                        }
                    }
                    Ok(())
                }));
            }
        }
    }

    promises::settled(pending).then(move |_| lazy_value.get())
}

/// `AbstractLifecycleService`.
pub struct AbstractLifecycleService {
    pub base: Rc<Disposable>,
    pub log_service: LogServiceRef,
    pub storage_service: Rc<dyn StorageService>,

    pub on_before_shutdown: Rc<Emitter<BeforeShutdownEvent>>,
    pub on_will_shutdown: Rc<Emitter<WillShutdownEvent>>,
    pub on_did_shutdown: Rc<Emitter<()>>,
    pub on_before_shutdown_error: Rc<Emitter<BeforeShutdownErrorEvent>>,
    pub on_shutdown_veto: Rc<Emitter<()>>,

    startup_kind: StartupKind,
    phase: Cell<LifecyclePhase>,
    pub will_shutdown: Cell<bool>,
    phase_when: RefCell<Vec<(LifecyclePhase, Rc<Barrier>)>>,
    pub shutdown_reason: Cell<Option<ShutdownReason>>,
}

impl AbstractLifecycleService {
    pub fn new(log_service: LogServiceRef, storage_service: Rc<dyn StorageService>) -> Rc<Self> {
        let base = Disposable::new();
        let on_before_shutdown = Emitter::new();
        let on_will_shutdown = Emitter::new();
        let on_did_shutdown = Emitter::new();
        let on_before_shutdown_error = Emitter::new();
        let on_shutdown_veto = Emitter::new();
        base.register(on_before_shutdown.clone());
        base.register(on_will_shutdown.clone());
        base.register(on_did_shutdown.clone());
        base.register(on_before_shutdown_error.clone());
        base.register(on_shutdown_veto.clone());

        let this = Rc::new({
            // Resolve startup kind
            let startup_kind = Self::resolve_startup_kind(&log_service, storage_service.as_ref());
            AbstractLifecycleService {
                base,
                log_service,
                storage_service,
                on_before_shutdown,
                on_will_shutdown,
                on_did_shutdown,
                on_before_shutdown_error,
                on_shutdown_veto,
                startup_kind,
                phase: Cell::new(LifecyclePhase::Starting),
                will_shutdown: Cell::new(false),
                phase_when: RefCell::default(),
                shutdown_reason: Cell::new(None),
            }
        });

        // Save shutdown reason to retrieve on next startup
        let weak = Rc::downgrade(&this);
        this.base.register(this.storage_service.on_will_save_state().listen(move |event| {
            if event.reason == WillSaveStateReason::Shutdown
                && let Some(this) = weak.upgrade()
            {
                let value = this.shutdown_reason.get().map_or(StorageValue::Undefined, |reason| StorageValue::Number(f64::from(reason as i32)));
                this.storage_service.store(LAST_SHUTDOWN_REASON_KEY, value, StorageScope::Workspace, StorageTarget::Machine);
            }
        }));
        this
    }

    fn resolve_startup_kind(log_service: &LogServiceRef, storage_service: &dyn StorageService) -> StartupKind {
        let startup_kind = Self::do_resolve_startup_kind(storage_service).unwrap_or(StartupKind::NewWindow);
        log_service.trace(&format!("[lifecycle] starting up (startup kind: {})", startup_kind as i32));

        startup_kind
    }

    fn do_resolve_startup_kind(storage_service: &dyn StorageService) -> Option<StartupKind> {
        // Retrieve and reset last shutdown reason
        let last_shutdown_reason = storage_service.get_number(LAST_SHUTDOWN_REASON_KEY, StorageScope::Workspace, None);
        storage_service.remove(LAST_SHUTDOWN_REASON_KEY, StorageScope::Workspace);

        // Convert into startup kind
        match last_shutdown_reason {
            Some(reason) if reason == f64::from(ShutdownReason::Reload as i32) => Some(StartupKind::ReloadedWindow),
            Some(reason) if reason == f64::from(ShutdownReason::Load as i32) => Some(StartupKind::ReopenedWindow),
            _ => None,
        }
    }

    pub fn startup_kind(&self) -> StartupKind {
        self.startup_kind
    }

    pub fn phase(&self) -> LifecyclePhase {
        self.phase.get()
    }

    /// `phase = value`.
    pub fn set_phase(&self, value: LifecyclePhase) {
        if value < self.phase.get() {
            errors::throw(Error::new("Lifecycle cannot go backwards"));
        }

        if self.phase.get() == value {
            return;
        }

        self.log_service.trace(&format!("lifecycle: phase changed (value: {})", value as i32));

        self.phase.set(value);
        mark(&format!("code/LifecyclePhase/{}", lifecycle_phase_to_string(value)), None);

        let barrier = {
            let mut phase_when = self.phase_when.borrow_mut();
            phase_when.iter().position(|(phase, _)| *phase == value).map(|index| phase_when.remove(index).1)
        };
        if let Some(barrier) = barrier {
            barrier.open();
        }
    }

    pub fn when(&self, phase: LifecyclePhase) -> Promise<()> {
        if phase <= self.phase.get() {
            return Promise::resolved(());
        }

        let barrier = {
            let mut phase_when = self.phase_when.borrow_mut();
            match phase_when.iter().find(|(known, _)| *known == phase) {
                Some((_, barrier)) => barrier.clone(),
                None => {
                    let barrier = Rc::new(Barrier::new());
                    phase_when.push((phase, barrier.clone()));
                    barrier
                }
            }
        };

        barrier.wait().then(|_| ())
    }
}
