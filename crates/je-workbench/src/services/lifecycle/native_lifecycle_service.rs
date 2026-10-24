//! Porte de `src/vs/workbench/services/lifecycle/electron-browser/lifecycleService.ts` (o `NativeLifecycleService`): o ciclo de vida
//! de uma janela nativa, que a thread principal avisa por IPC antes (`vscode:onBeforeUnload`) e durante (`vscode:onWillUnload`) o
//! fechamento.
//!
//! Divergências:
//! - O `ipcRenderer` (`ipcRenderer.on(canal, ...)`/`send`) e o `INativeHostService` (`windowId`, `closeWindow`) são o contrato
//!   [`INativeLifecycleHost`], que o processo nativo implementa. O original registra dois tratadores de canal; aqui o contrato oferece
//!   um registro por mensagem ([`INativeLifecycleHost::on_before_unload`] e [`INativeLifecycleHost::on_will_unload`]) com a resposta já
//!   decodificada.
//! - `handleBeforeShutdown` e `handleWillShutdown` são `protected` no original (o teste os expõe por uma subclasse); aqui são `pub`.
//! - `this._onBeforeShutdown.fire({ reason, veto(value, id) {...}, finalVeto(value, id) {...} })` dispara um [`BeforeShutdownEvent`]
//!   que acumula as vetos; o registro de log de cada veto (`Shutdown was prevented`) e o rastreio das pendentes rodam logo depois do
//!   disparo, sobre as vetos acumuladas, e não dentro de cada chamada de `veto`. O resultado é o mesmo.
//! - `registerSingleton(ILifecycleService, NativeLifecycleService, InstantiationType.Eager)` não é portado (o contrato do serviço de
//!   arquivos nativos e o IPC do processo nativo ainda não existem para compor o serviço).

use std::cell::RefCell;
use std::rc::{Rc, Weak};

use je_base::async_::{async_fn, disposable_timeout, promises};
use je_base::cancellation::CancellationTokenSource;
use je_base::error_message::to_error_message_of;
use je_base::errors::{self, Error};
use je_base::event::Event;
use je_base::lifecycle::{DisposableRef, IDisposable};
use je_base::promise::{MaybePromise, Promise};
use je_platform::log::{LogServiceRef, Logger};
use je_platform::storage::storage::StorageService;

use super::lifecycle::{
    BeforeShutdownErrorEvent, BeforeShutdownEvent, ILifecycleService, InternalBeforeShutdownEvent, LifecyclePhase, ShutdownReason, StartupKind, WillShutdownEvent,
    WillShutdownEventJoiner, WillShutdownJoin, WillShutdownJoinerOrder,
};
use super::lifecycle_service::{handle_vetos, AbstractLifecycleService};

const BEFORE_SHUTDOWN_WARNING_DELAY: u64 = 5000;
const WILL_SHUTDOWN_WARNING_DELAY: u64 = 800;

/// A mensagem `vscode:onBeforeUnload`: `{ okChannel, cancelChannel, reason }`.
#[derive(Clone, Debug)]
pub struct BeforeUnloadRequest {
    pub ok_channel: String,
    pub cancel_channel: String,
    pub reason: ShutdownReason,
}

/// A mensagem `vscode:onWillUnload`: `{ replyChannel, reason }`.
#[derive(Clone, Debug)]
pub struct WillUnloadRequest {
    pub reply_channel: String,
    pub reason: ShutdownReason,
}

/// O que o `NativeLifecycleService` usa do `ipcRenderer` e do `INativeHostService`.
pub trait INativeLifecycleHost {
    /// `nativeHostService.windowId`.
    fn window_id(&self) -> u32;

    /// `nativeHostService.closeWindow()`.
    fn close_window(&self) -> Promise<()>;

    /// `ipcRenderer.on('vscode:onBeforeUnload', ...)`.
    fn on_before_unload(&self, listener: Rc<dyn Fn(BeforeUnloadRequest)>);

    /// `ipcRenderer.on('vscode:onWillUnload', ...)`.
    fn on_will_unload(&self, listener: Rc<dyn Fn(WillUnloadRequest)>);

    /// `ipcRenderer.send(channel, windowId)`.
    fn send(&self, channel: &str, window_id: u32);
}

/// `NativeLifecycleService`.
pub struct NativeLifecycleService {
    me: Weak<NativeLifecycleService>,
    base: Rc<AbstractLifecycleService>,
    native_host_service: Rc<dyn INativeLifecycleHost>,
}

impl NativeLifecycleService {
    pub fn new(native_host_service: Rc<dyn INativeLifecycleHost>, storage_service: Rc<dyn StorageService>, log_service: LogServiceRef) -> Rc<Self> {
        let base = AbstractLifecycleService::new(log_service, storage_service);
        let this = Rc::new_cyclic(|me| Self { me: me.clone(), base, native_host_service });
        this.register_listeners();
        this
    }

    fn this(&self) -> Rc<Self> {
        self.me.upgrade().expect("o serviço está vivo enquanto um método dele roda")
    }

    fn register_listeners(&self) {
        let window_id = self.native_host_service.window_id();

        // Main side indicates that window is about to unload, check for vetos
        let weak = self.me.clone();
        self.native_host_service.on_before_unload(Rc::new(move |reply: BeforeUnloadRequest| {
            let Some(this) = weak.upgrade() else {
                return;
            };
            this.base.log_service.trace(&format!("[lifecycle] onBeforeUnload (reason: {})", reply.reason as i32));

            // trigger onBeforeShutdown events and veto collecting
            let service = this.clone();
            let _ = this.handle_before_shutdown(reply.reason).then(move |veto| {
                // veto: cancel unload
                if veto {
                    service.base.log_service.trace("[lifecycle] onBeforeUnload prevented via veto");

                    // Indicate as event
                    service.base.on_shutdown_veto.fire(());

                    service.native_host_service.send(&reply.cancel_channel, window_id);
                }
                // no veto: allow unload
                else {
                    service.base.log_service.trace("[lifecycle] onBeforeUnload continues without veto");

                    service.base.shutdown_reason.set(Some(reply.reason));
                    service.native_host_service.send(&reply.ok_channel, window_id);
                }
            });
        }));

        // Main side indicates that we will indeed shutdown
        let weak = self.me.clone();
        self.native_host_service.on_will_unload(Rc::new(move |reply: WillUnloadRequest| {
            let Some(this) = weak.upgrade() else {
                return;
            };
            this.base.log_service.trace(&format!("[lifecycle] onWillUnload (reason: {})", reply.reason as i32));

            // trigger onWillShutdown events and joining
            let service = this.clone();
            let _ = this.handle_will_shutdown(reply.reason).then(move |()| {
                // trigger onDidShutdown event now that we know we will quit
                service.base.on_did_shutdown.fire(());

                // acknowledge to main side
                service.native_host_service.send(&reply.reply_channel, window_id);
            });
        }));
    }

    pub fn handle_before_shutdown(&self, reason: ShutdownReason) -> Promise<bool> {
        let log_service = self.base.log_service.clone();
        let this = self.this();

        let event = InternalBeforeShutdownEvent::new(reason);
        self.base.on_before_shutdown.fire(event.event.clone());

        let pending_vetos: Rc<RefCell<Vec<String>>> = Rc::default();
        let mut vetos: Vec<MaybePromise<bool>> = Vec::new();
        for (value, id) in event.event.vetos() {
            // Log any veto instantly
            match &value {
                MaybePromise::Value(true) => log_service.info(&format!("[lifecycle]: Shutdown was prevented (id: {id})")),
                // Track promise completion
                MaybePromise::Promise(promise) => {
                    {
                        let mut pending = pending_vetos.borrow_mut();
                        if !pending.contains(&id) {
                            pending.push(id.clone());
                        }
                    }
                    let (log_service, pending_vetos, id) = (log_service.clone(), pending_vetos.clone(), id.clone());
                    let _ = promise
                        .then({
                            let (log_service, id) = (log_service, id.clone());
                            move |veto| {
                                if veto {
                                    log_service.info(&format!("[lifecycle]: Shutdown was prevented (id: {id})"));
                                }
                            }
                        })
                        .finally(move || pending_vetos.borrow_mut().retain(|pending| *pending != id));
                }
                MaybePromise::Value(false) => {}
            }
            vetos.push(value);
        }
        let final_veto = event.get_final_veto();

        let warning_pending = pending_vetos.clone();
        let warning_log = log_service.clone();
        let long_running_before_shutdown_warning: DisposableRef = disposable_timeout(
            move || {
                warning_log.warn(&format!("[lifecycle] onBeforeShutdown is taking a long time, pending operations: {}", warning_pending.borrow().join(", ")));
            },
            BEFORE_SHUTDOWN_WARNING_DELAY,
            None,
        );

        async_fn(async move {
            let result = async {
                // First: run list of vetos in parallel
                let error_service = this.clone();
                let mut veto = handle_vetos(vetos, Rc::new(move |error: &Error| error_service.handle_before_shutdown_error(error, reason))).await?;
                if veto {
                    return Ok(veto);
                }

                // Second: run the final veto if defined
                if let Some((final_veto_fn, final_veto_id)) = final_veto {
                    {
                        let mut pending = pending_vetos.borrow_mut();
                        if !pending.contains(&final_veto_id) {
                            pending.push(final_veto_id.clone());
                        }
                    }
                    let outcome = match errors::catch(|| final_veto_fn()) {
                        Ok(MaybePromise::Value(value)) => Ok(value),
                        Ok(MaybePromise::Promise(promise)) => promise.await,
                        Err(error) => Err(error),
                    };
                    match outcome {
                        Ok(value) => {
                            veto = value;
                            if veto {
                                log_service.info(&format!("[lifecycle]: Shutdown was prevented by final veto (id: {final_veto_id})"));
                            }
                        }
                        Err(error) => {
                            veto = true; // treat error as veto

                            this.handle_before_shutdown_error(&error, reason);
                        }
                    }
                }

                Ok::<bool, Error>(veto)
            }
            .await;
            long_running_before_shutdown_warning.dispose();
            result
        })
    }

    fn handle_before_shutdown_error(&self, error: &Error, reason: ShutdownReason) {
        self.base.log_service.error(&format!("[lifecycle]: Error during before-shutdown phase (error: {})", to_error_message_of(error, false)));

        self.base.on_before_shutdown_error.fire(BeforeShutdownErrorEvent { reason, error: error.clone() });
    }

    pub fn handle_will_shutdown(&self, reason: ShutdownReason) -> Promise<()> {
        self.base.will_shutdown.set(true);

        let joiners: Rc<RefCell<Vec<Promise<()>>>> = Rc::default();
        let last_joiners: Rc<RefCell<Vec<Rc<dyn Fn() -> Promise<()>>>>> = Rc::default();
        let pending_joiners: Rc<RefCell<Vec<WillShutdownEventJoiner>>> = Rc::default();
        let cts = CancellationTokenSource::new(None);

        let join: Rc<dyn Fn(WillShutdownJoin, WillShutdownEventJoiner)> = {
            let (joiners, last_joiners, pending_joiners) = (joiners.clone(), last_joiners.clone(), pending_joiners.clone());
            Rc::new(move |promise_or_promise_fn, joiner| {
                pending_joiners.borrow_mut().push(joiner.clone());

                if joiner.order == Some(WillShutdownJoinerOrder::Last) {
                    let promise_fn: Rc<dyn Fn() -> Promise<()>> = match promise_or_promise_fn {
                        WillShutdownJoin::Function(promise_fn) => promise_fn,
                        WillShutdownJoin::Promise(promise) => Rc::new(move || promise.clone()),
                    };
                    let pending_joiners = pending_joiners.clone();
                    last_joiners.borrow_mut().push(Rc::new(move || {
                        let (pending_joiners, joiner) = (pending_joiners.clone(), joiner.clone());
                        promise_fn().finally(move || {
                            let mut pending = pending_joiners.borrow_mut();
                            if let Some(index) = pending.iter().position(|pending| *pending == joiner) {
                                pending.remove(index);
                            }
                        })
                    }));
                } else {
                    let promise = match promise_or_promise_fn {
                        WillShutdownJoin::Function(promise_fn) => promise_fn(),
                        WillShutdownJoin::Promise(promise) => promise,
                    };
                    let pending_joiners = pending_joiners.clone();
                    let finished = joiner.clone();
                    let _ = promise.finally(move || {
                        let mut pending = pending_joiners.borrow_mut();
                        if let Some(index) = pending.iter().position(|pending| *pending == finished) {
                            pending.remove(index);
                        }
                    });
                    joiners.borrow_mut().push(promise);
                }
            })
        };

        let listed_joiners = pending_joiners.clone();
        let source = cts.clone();
        self.base.on_will_shutdown.fire(WillShutdownEvent::new(
            reason,
            cts.token(),
            join,
            Rc::new(move || listed_joiners.borrow().clone()),
            Rc::new(move || source.dispose_with(true)),
        ));

        let warning_joiners = pending_joiners.clone();
        let warning_log = self.base.log_service.clone();
        let long_running_will_shutdown_warning: DisposableRef = disposable_timeout(
            move || {
                warning_log.warn(&format!(
                    "[lifecycle] onWillShutdown is taking a long time, pending operations: {}",
                    warning_joiners.borrow().iter().map(|joiner| joiner.id.as_str()).collect::<Vec<_>>().join(", ")
                ));
            },
            WILL_SHUTDOWN_WARNING_DELAY,
            None,
        );

        let log_service = self.base.log_service.clone();
        let token = cts.token();
        async_fn(async move {
            if let Err(error) = je_base::async_::race_cancellation(&promises::settled(joiners.borrow().clone()), &token).await {
                log_service.error(&format!("[lifecycle]: Error during will-shutdown phase in default joiners (error: {})", to_error_message_of(&error, false)));
            }

            let last: Vec<Promise<()>> = last_joiners.borrow().iter().map(|last_joiner| last_joiner()).collect();
            if let Err(error) = je_base::async_::race_cancellation(&promises::settled(last), &token).await {
                log_service.error(&format!("[lifecycle]: Error during will-shutdown phase in last joiners (error: {})", to_error_message_of(&error, false)));
            }

            long_running_will_shutdown_warning.dispose();
            Ok(())
        })
    }
}

impl ILifecycleService for NativeLifecycleService {
    fn startup_kind(&self) -> StartupKind {
        self.base.startup_kind()
    }

    fn phase(&self) -> LifecyclePhase {
        self.base.phase()
    }

    fn set_phase(&self, phase: LifecyclePhase) {
        self.base.set_phase(phase);
    }

    fn on_before_shutdown(&self) -> Event<BeforeShutdownEvent> {
        self.base.on_before_shutdown.event()
    }

    fn on_shutdown_veto(&self) -> Event<()> {
        self.base.on_shutdown_veto.event()
    }

    fn on_before_shutdown_error(&self) -> Event<BeforeShutdownErrorEvent> {
        self.base.on_before_shutdown_error.event()
    }

    fn on_will_shutdown(&self) -> Event<WillShutdownEvent> {
        self.base.on_will_shutdown.event()
    }

    fn will_shutdown(&self) -> bool {
        self.base.will_shutdown.get()
    }

    fn on_did_shutdown(&self) -> Event<()> {
        self.base.on_did_shutdown.event()
    }

    fn when(&self, phase: LifecyclePhase) -> Promise<()> {
        self.base.when(phase)
    }

    fn shutdown(&self) -> Promise<()> {
        self.native_host_service.close_window()
    }
}

impl IDisposable for NativeLifecycleService {
    fn dispose(&self) {
        self.base.base.dispose();
    }
}
