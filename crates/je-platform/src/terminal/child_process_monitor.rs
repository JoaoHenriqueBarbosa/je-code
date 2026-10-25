//! Porte de `src/vs/platform/terminal/node/childProcessMonitor.ts`: o monitor dos processos filhos do shell, que
//! confere a árvore de processos depois de entrada e de saída e avisa quando o shell passa a ter (ou deixa de ter)
//! processos filhos.
//!
//! Representação:
//! - `extends Disposable` é um [`Disposable`] por composição. Os dois `RunOnceScheduler` guardam um `Weak` do monitor
//!   (o original captura `this`), então o monitor não fica preso pelos próprios timers.
//! - `listProcesses` de `base/node/ps.ts` precisa do [`FileAccess`] (resolve `ps.sh` e `cpuUsage.sh` no Linux) e da
//!   [`Platform`]: o monitor guarda os dois. [`ChildProcessMonitor::new`] usa um `FileAccess` sem raiz de arquivos
//!   (o mesmo que o `MacExternalTerminalService::new` usa); quem tem o `FileAccess` do aplicativo chama
//!   [`ChildProcessMonitor::with_file_access`].
//! - `ignoreProcessNames` (um `string[]` de módulo que o `PtyService` altera) é um `static` com `Mutex`, lido por
//!   [`ignore_process_names`] e escrito por [`set_ignore_process_names`].
//! - `_refreshActive` (`async`) devolve uma [`Promise`]; o agendador a ignora, como o original. O `-Number.MAX_VALUE`
//!   de `_lastInactiveRefreshTime` é `f64::MIN`.

use std::cell::Cell;
use std::rc::{Rc, Weak};
use std::sync::Mutex;

use je_base::async_::{RunOnceScheduler, async_fn};
use je_base::dynamic::Dynamic;
use je_base::event::{Emitter, Event};
use je_base::lifecycle::{Disposable, IDisposable};
use je_base::network::{FileAccess, FileAccessContext, RemoteAuthorities};
use je_base::node::ps::list_processes;
use je_base::path::{PathModule, ProcessEnvironment as HostEnvironment};
use je_base::platform::Platform;
use je_base::processes::ProcessItem;
use je_base::promise::Promise;
use je_base::runtime;

use crate::log::{LogArg, LogServiceRef};

/// The amount of time to throttle checks when the process receives output.
const INACTIVE_THROTTLE_DURATION: u64 = 5000;
/// The amount of time to debounce check when the process receives input.
const ACTIVE_DEBOUNCE_DURATION: u64 = 1000;

static IGNORE_PROCESS_NAMES: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// `ignoreProcessNames`: os nomes de processo que não contam como processo filho.
pub fn ignore_process_names() -> Vec<String> {
    IGNORE_PROCESS_NAMES.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clone()
}

/// Troca o conteúdo de `ignoreProcessNames` (`ignoreProcessNames.length = 0; ignoreProcessNames.push(...names)`).
pub fn set_ignore_process_names(names: Vec<String>) {
    *IGNORE_PROCESS_NAMES.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = names;
}

/// Monitors child processes at different intervals after input and output.
/// Pending checks are canceled when the monitor is disposed.
pub struct ChildProcessMonitor {
    base: Rc<Disposable>,
    pid: Cell<f64>,
    has_child_processes: Cell<bool>,
    on_did_change_has_child_processes: Rc<Emitter<bool>>,
    refresh_active_scheduler: Rc<RunOnceScheduler>,
    refresh_inactive_scheduler: Rc<RunOnceScheduler>,
    last_inactive_refresh_time: Cell<f64>,
    log_service: LogServiceRef,
    file_access: Rc<FileAccess>,
    platform: Platform,
}

impl ChildProcessMonitor {
    pub fn new(pid: impl Into<f64>, log_service: LogServiceRef) -> Rc<Self> {
        Self::with_file_access(pid, log_service, Rc::new(FileAccess::new(Rc::new(HostEnvironment), FileAccessContext::default(), RemoteAuthorities::new(false))))
    }

    /// O monitor com o `FileAccess` do aplicativo, de onde `listProcesses` tira os scripts `ps.sh` e `cpuUsage.sh`.
    pub fn with_file_access(pid: impl Into<f64>, log_service: LogServiceRef, file_access: Rc<FileAccess>) -> Rc<Self> {
        let base = Disposable::new();
        let on_did_change_has_child_processes: Rc<Emitter<bool>> = Emitter::new();
        base.register(on_did_change_has_child_processes.clone());
        let pid = pid.into();
        Rc::new_cyclic(|weak: &Weak<Self>| {
            let refresh_active_scheduler = {
                let weak = weak.clone();
                RunOnceScheduler::new(
                    move || {
                        if let Some(this) = weak.upgrade() {
                            let _ = this.refresh_active();
                        }
                    },
                    ACTIVE_DEBOUNCE_DURATION,
                )
            };
            let refresh_inactive_scheduler = {
                let weak = weak.clone();
                RunOnceScheduler::new(
                    move || {
                        if let Some(this) = weak.upgrade() {
                            this.refresh_inactive();
                        }
                    },
                    INACTIVE_THROTTLE_DURATION,
                )
            };
            base.register(refresh_active_scheduler.clone());
            base.register(refresh_inactive_scheduler.clone());
            Self {
                base,
                pid: Cell::new(pid),
                has_child_processes: Cell::new(false),
                on_did_change_has_child_processes,
                refresh_active_scheduler,
                refresh_inactive_scheduler,
                last_inactive_refresh_time: Cell::new(f64::MIN),
                log_service,
                file_access,
                platform: Platform::detect(),
            }
        })
    }

    /// Whether the process has child processes.
    pub fn has_child_processes(&self) -> bool {
        self.has_child_processes.get()
    }

    fn set_has_child_processes(&self, value: bool) {
        if self.has_child_processes.get() != value {
            self.has_child_processes.set(value);
            self.log_service.debug_args("ChildProcessMonitor: Has child processes changed", &[LogArg::Value(Dynamic::Bool(value))]);
            self.on_did_change_has_child_processes.fire(value);
        }
    }

    /// An event that fires when whether the process has child processes changes.
    pub fn on_did_change_has_child_processes(&self) -> Event<bool> {
        self.on_did_change_has_child_processes.event()
    }

    /// Updates the pid to monitor. This is needed when the pid is not available
    /// immediately after spawn (e.g. node-pty deferred conpty connection).
    pub fn set_pid(&self, pid: impl Into<f64>) {
        self.pid.set(pid.into());
    }

    /// Input was triggered on the process.
    pub fn handle_input(&self) {
        if !self.base.store().is_disposed() {
            self.refresh_active_scheduler.schedule(None);
        }
    }

    /// Output was triggered on the process.
    pub fn handle_output(&self) {
        if self.base.store().is_disposed() || self.refresh_inactive_scheduler.is_scheduled() {
            return;
        }
        let delay = self.last_inactive_refresh_time.get() + INACTIVE_THROTTLE_DURATION as f64 - runtime::date_now() as f64;
        if delay <= 0.0 {
            self.refresh_inactive();
        } else {
            self.refresh_inactive_scheduler.schedule(Some(delay as u64));
        }
    }

    fn refresh_active(self: &Rc<Self>) -> Promise<()> {
        let this = self.clone();
        async_fn(async move {
            if this.base.store().is_disposed() {
                return Ok(());
            }
            match list_processes(this.pid.get(), &this.file_access, &this.platform).await {
                Ok(process_item) => this.set_has_child_processes(Self::process_contains_children(&process_item)),
                Err(error) => this.log_service.debug_args("ChildProcessMonitor: Fetching process tree failed", &[LogArg::from(&error)]),
            }
            Ok(())
        })
    }

    fn refresh_inactive(&self) {
        self.last_inactive_refresh_time.set(runtime::date_now() as f64);
        self.refresh_active_scheduler.schedule(None);
    }

    fn process_contains_children(process_item: &ProcessItem) -> bool {
        // No child processes
        let Some(children) = &process_item.children else {
            return false;
        };

        // A single child process, handle special cases
        if children.len() == 1 {
            let item = &children[0];
            let cmd: &str = if item.cmd.starts_with('"') {
                // `substring(1, indexOf('"', 1))`: sem a segunda aspa o `substring` troca os limites e devolve a primeira.
                match item.cmd[1..].find('"') {
                    Some(end) => &item.cmd[1..1 + end],
                    None => &item.cmd[..1],
                }
            } else {
                match item.cmd.find(' ') {
                    None => &item.cmd,
                    Some(space_index) => &item.cmd[..space_index],
                }
            };
            let host = HostEnvironment;
            let name = PathModule::native(&host).parse(cmd).name;
            return !ignore_process_names().contains(&name);
        }

        // Fallback, count child processes
        !children.is_empty()
    }
}

impl IDisposable for ChildProcessMonitor {
    fn dispose(&self) {
        self.base.dispose();
    }
}
