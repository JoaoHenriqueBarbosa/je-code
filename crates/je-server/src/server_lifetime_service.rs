//! Porte de `src/vs/server/node/serverLifetimeService.ts`: o serviço que conta os consumidores ativos (hosts de extensões e
//! afins) que mantêm o servidor vivo e, com o desligamento automático ligado, decide quando o processo sai.
//!
//! Divergências:
//! - `_exit: (code?: number) => never` é um `Rc<dyn Fn(i32)>` (o padrão chama `std::process::exit`).
//! - `IServerWillShutdownEvent.join(promise)` é [`ServerWillShutdownEvent::join`], e o `Promise.all(joins.map(p =>
//!   p.catch(...)))` espera cada promessa em sequência, registrando a falha de cada uma: assenta quando todas assentaram,
//!   como o original. A continuação roda numa microtarefa (um `.then` nunca é síncrono), mesmo sem nenhuma junção.
//! - `_shutdownPromise` guardado só importa como "há um desligamento em andamento": é um booleano.
//! - `setTimeout`/`clearTimeout` são os do runtime (`set_timeout` e `clear_timeout`), que respeitam o relógio virtual.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::{Rc, Weak};

use je_base::event::{Emitter, Event};
use je_base::lifecycle::{Disposable, DisposableRef, IDisposable, to_disposable};
use je_base::promise::Promise;
use je_base::runtime::{self, TimerId};
use je_instantiation::service_id;
use je_platform::log::LogServiceRef;

pub const SHUTDOWN_TIMEOUT: u64 = 5 * 60 * 1000;

/// Options controlling the auto-shutdown behaviour.
#[derive(Clone, Copy, Debug, Default)]
pub struct ServerLifetimeOptions {
    /// When `false` (default), the server never auto-shuts down.
    pub enable_auto_shutdown: bool,
    /// When `true`, skip the 5-minute grace period on non-initial shutdowns.
    pub shutdown_without_delay: bool,
}

/// `IServerWillShutdownEvent`.
#[derive(Clone)]
pub struct ServerWillShutdownEvent {
    joins: Rc<RefCell<Vec<Promise<()>>>>,
}

impl ServerWillShutdownEvent {
    /// `join(promise)`.
    pub fn join(&self, promise: Promise<()>) {
        self.joins.borrow_mut().push(promise);
    }
}

/// Tracks active consumers (extension hosts, agent sessions, etc.) that keep
/// the server alive. When auto-shutdown is enabled, the service manages a
/// shutdown timer and fires `onWillShutdown` when it is time for the process to exit.
pub trait IServerLifetimeService {
    fn on_will_shutdown(&self) -> Event<ServerWillShutdownEvent>;
    fn on_did_abort_shutdown(&self) -> Event<()>;

    /// Marks a consumer as active. The server will not auto-shutdown until the returned disposable is disposed.
    fn active(&self, consumer: &str) -> DisposableRef;

    /// Delays the auto-shutdown timer. If the server is currently in a shutdown timeout (all consumers inactive), the
    /// timer is reset.
    fn delay(&self);

    /// Whether any consumer is currently active.
    fn has_active_consumers(&self) -> bool;
}

service_id!(pub I_SERVER_LIFETIME_SERVICE: dyn IServerLifetimeService = "serverLifetimeService");

#[derive(Default)]
struct State {
    consumers: HashMap<String, usize>,
    total_count: usize,
    shutdown_timer: Option<TimerId>,
    shutdown_in_progress: bool,
    shutdown_generation: u64,
}

/// `ServerLifetimeService`.
pub struct ServerLifetimeService {
    this: Weak<ServerLifetimeService>,
    base: Rc<Disposable>,
    on_will_shutdown: Rc<Emitter<ServerWillShutdownEvent>>,
    on_did_abort_shutdown: Rc<Emitter<()>>,
    options: ServerLifetimeOptions,
    exit: Rc<dyn Fn(i32)>,
    log_service: LogServiceRef,
    state: RefCell<State>,
}

impl ServerLifetimeService {
    /// `new ServerLifetimeService(options, exit, logService)`.
    pub fn new(options: ServerLifetimeOptions, exit: Option<Rc<dyn Fn(i32)>>, log_service: LogServiceRef) -> Rc<Self> {
        let base = Disposable::new();
        let on_will_shutdown = Emitter::new();
        base.register(on_will_shutdown.clone());
        let on_did_abort_shutdown = Emitter::new();
        base.register(on_did_abort_shutdown.clone());

        let this = Rc::new_cyclic(|this| Self {
            this: this.clone(),
            base,
            on_will_shutdown,
            on_did_abort_shutdown,
            options,
            exit: exit.unwrap_or_else(|| Rc::new(|code| std::process::exit(code))),
            log_service,
            state: RefCell::default(),
        });

        if this.options.enable_auto_shutdown {
            // Start initial shutdown timer (no clients connected yet)
            this.schedule_shutdown(true);
        }

        this
    }

    fn rc(&self) -> Rc<Self> {
        self.this.upgrade().expect("o serviço está vivo enquanto é usado")
    }

    fn schedule_shutdown(&self, initial: bool) {
        if self.options.shutdown_without_delay && !initial {
            self.try_shutdown();
        } else {
            self.log_service.debug("ServerLifetime: scheduling shutdown timer");
            let this = self.this.clone();
            let timer = runtime::set_timeout(
                move || {
                    if let Some(this) = this.upgrade() {
                        this.state.borrow_mut().shutdown_timer = None;
                        this.try_shutdown();
                    }
                },
                SHUTDOWN_TIMEOUT,
            );
            self.state.borrow_mut().shutdown_timer = Some(timer);
        }
    }

    fn try_shutdown(&self) {
        if self.state.borrow().total_count > 0 {
            self.log_service.debug("ServerLifetime: consumer became active, aborting shutdown");
            return;
        }
        if self.state.borrow().shutdown_in_progress {
            return;
        }
        self.log_service.info("ServerLifetime: all consumers inactive, shutting down");
        let generation = self.state.borrow().shutdown_generation;
        let joins: Rc<RefCell<Vec<Promise<()>>>> = Rc::default();
        self.on_will_shutdown.fire(ServerWillShutdownEvent { joins: joins.clone() });
        self.state.borrow_mut().shutdown_in_progress = true;

        let this = self.rc();
        runtime::queue_microtask(move || {
            runtime::spawn_local(async move {
                let joins: Vec<Promise<()>> = joins.borrow().clone();
                for join in joins {
                    if let Err(error) = join.await {
                        this.log_service.error(&format!("ServerLifetime: shutdown participant failed {error}"));
                    }
                }

                this.state.borrow_mut().shutdown_in_progress = false;
                let aborted = {
                    let state = this.state.borrow();
                    state.total_count > 0 || generation != state.shutdown_generation
                };
                if aborted {
                    this.log_service.info("ServerLifetime: consumer became active during shutdown, aborting exit");
                    this.on_did_abort_shutdown.fire(());
                    if this.state.borrow().total_count == 0 && this.options.enable_auto_shutdown {
                        this.schedule_shutdown(false);
                    }
                    return;
                }
                this.dispose();
                (this.exit)(0);
            });
        });
    }

    fn cancel_shutdown(&self) {
        let timer = self.state.borrow_mut().shutdown_timer.take();
        if let Some(timer) = timer {
            self.log_service.debug("ServerLifetime: cancelling shutdown timer");
            runtime::clear_timeout(timer);
        }
    }
}

impl IDisposable for ServerLifetimeService {
    fn dispose(&self) {
        self.base.dispose();
    }
}

impl IServerLifetimeService for ServerLifetimeService {
    fn on_will_shutdown(&self) -> Event<ServerWillShutdownEvent> {
        self.on_will_shutdown.event()
    }

    fn on_did_abort_shutdown(&self) -> Event<()> {
        self.on_did_abort_shutdown.event()
    }

    fn has_active_consumers(&self) -> bool {
        self.state.borrow().total_count > 0
    }

    fn active(&self, consumer: &str) -> DisposableRef {
        let (was_empty, total) = {
            let mut state = self.state.borrow_mut();
            state.shutdown_generation += 1;
            let was_empty = state.total_count == 0;
            *state.consumers.entry(consumer.to_owned()).or_insert(0) += 1;
            state.total_count += 1;
            (was_empty, state.total_count)
        };

        self.log_service.debug(&format!("ServerLifetime: consumer '{consumer}' active (total: {total})"));

        if was_empty {
            self.cancel_shutdown();
        }

        let (this, consumer, disposed) = (self.rc(), consumer.to_owned(), Cell::new(false));
        to_disposable(move || {
            if disposed.get() {
                return;
            }
            disposed.set(true);

            let total = {
                let mut state = this.state.borrow_mut();
                if let Some(count) = state.consumers.get(&consumer).copied() {
                    if count <= 1 {
                        state.consumers.remove(&consumer);
                    } else {
                        state.consumers.insert(consumer.clone(), count - 1);
                    }
                }
                state.total_count -= 1;
                state.total_count
            };

            this.log_service.debug(&format!("ServerLifetime: consumer '{consumer}' inactive (total: {total})"));

            if total == 0 && this.options.enable_auto_shutdown {
                this.schedule_shutdown(false);
            }
        })
    }

    fn delay(&self) {
        let (has_timer, shutting_down) = {
            let mut state = self.state.borrow_mut();
            state.shutdown_generation += 1;
            (state.shutdown_timer.is_some(), state.shutdown_in_progress)
        };
        if has_timer {
            self.log_service.debug("ServerLifetime: delay requested, resetting shutdown timer");
            self.cancel_shutdown();
            self.schedule_shutdown(false);
        } else if shutting_down {
            self.log_service.debug("ServerLifetime: delay requested during shutdown, aborting pending exit");
        }
    }
}
