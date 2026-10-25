//! Apoio dos testes do túnel remoto: o processo filho falso (o `createProcess`/`createTestChildProcess` dos testes de
//! `src/vs/platform/remoteTunnel/test/node`) e o serviço de telemetria que registra as chamadas (o `sinon.spy` sobre
//! `publicLog2`).
//!
//! O filho falso guarda os ouvintes que a CLI registrou ([`TunnelChildHandlers`]); `write_stdout`, `write_stderr`,
//! `emit_exit` e `emit_error` são o `stdout.write(...)`, `stderr.write(...)` e `child.emit('exit' | 'error', ...)` do
//! original. O `kill` conta as chamadas, roda os ganchos de `kill` e, se o filho morre ao ser morto, emite o `exit`
//! por microtarefa (o `queueMicrotask(() => child.emit('exit', null))`).
#![allow(dead_code)]

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use je_base::dynamic::Dynamic;
use je_base::errors::Error;
use je_base::runtime;
use je_platform::remote_tunnel::code_tunnel_cli_process::{KillSignal, TunnelChildHandlers, TunnelChildProcess};
use je_platform::telemetry::telemetry::{CommonPropertyValue, ITelemetryData, ITelemetryService, TelemetryLevel};
use je_base::path::{FixedEnvironment, Platform};
use je_platform::environment::NativeParsedArgs;
use je_platform::environment::environment_service::{AbstractNativeEnvironmentService, NativeEnvironmentPaths};
use je_platform::product::ProductService;

/// `{ appRoot: 'installation', isBuilt: true, logsHome: URI.file('logs'), userDataPath: 'custom-user-data' } as
/// INativeEnvironmentService`: o ambiente nativo de verdade com esses quatro valores.
pub fn test_native_environment() -> Rc<AbstractNativeEnvironmentService> {
    let args = NativeParsedArgs::default();
    args.set("logsPath", Dynamic::string("logs"));
    let paths = NativeEnvironmentPaths {
        user_data_dir: "custom-user-data".to_owned(),
        home_dir: "/home".to_owned(),
        tmp_dir: "/tmp".to_owned(),
        app_root: "installation".to_owned(),
    };
    Rc::new(AbstractNativeEnvironmentService::new(args, paths, ProductService::default(), Rc::new(FixedEnvironment::new(Platform::Posix, "/"))))
}

/// `{ tunnelApplicationName: 'code-tunnel' } as IProductService`.
pub fn test_tunnel_product() -> ProductService {
    ProductService { tunnel_application_name: Some("code-tunnel".to_owned()), ..ProductService::default() }
}

/// O `ChildProcess` falso.
pub struct TestChild {
    me: Weak<TestChild>,
    pid: u32,
    handlers: TunnelChildHandlers,
    kill_calls: Cell<usize>,
    exit_on_kill: bool,
    exit_hooks: RefCell<Vec<Box<dyn Fn()>>>,
    kill_hooks: RefCell<Vec<Box<dyn Fn()>>>,
}

impl TestChild {
    pub fn new(handlers: TunnelChildHandlers, pid: u32, exit_on_kill: bool) -> Rc<Self> {
        Rc::new_cyclic(|me| Self {
            me: me.clone(),
            pid,
            handlers,
            kill_calls: Cell::new(0),
            exit_on_kill,
            exit_hooks: RefCell::new(Vec::new()),
            kill_hooks: RefCell::new(Vec::new()),
        })
    }

    /// `stdout.write(text)`.
    pub fn write_stdout(&self, text: &str) {
        (self.handlers.on_stdout)(text.as_bytes());
    }

    /// `stderr.write(text)`.
    pub fn write_stderr(&self, text: &str) {
        (self.handlers.on_stderr)(text.as_bytes());
    }

    /// `child.emit('exit', code)`; os ouvintes extras (`child.on('exit', ...)` do teste) vêm antes, porque o teste os
    /// registra dentro do `spawn`, antes de a CLI registrar os dela.
    pub fn emit_exit(&self, code: Option<i32>) {
        for hook in self.exit_hooks.borrow().iter() {
            hook();
        }
        (self.handlers.on_exit)(code);
    }

    /// `child.emit('error', error)`.
    pub fn emit_error(&self, error: Error) {
        (self.handlers.on_error)(error);
    }

    /// `child.on('exit', hook)`.
    pub fn on_exit(&self, hook: impl Fn() + 'static) {
        self.exit_hooks.borrow_mut().push(Box::new(hook));
    }

    /// O embrulho do `kill` (`process.child.kill = () => { hook(); return kill.call(child) }`).
    pub fn on_kill(&self, hook: impl Fn() + 'static) {
        self.kill_hooks.borrow_mut().push(Box::new(hook));
    }

    /// `kill.callCount`.
    pub fn kill_calls(&self) -> usize {
        self.kill_calls.get()
    }

    /// `wasKilled()`.
    pub fn was_killed(&self) -> bool {
        self.kill_calls.get() > 0
    }
}

impl TunnelChildProcess for TestChild {
    fn pid(&self) -> Option<u32> {
        Some(self.pid)
    }

    fn kill(&self, _signal: KillSignal) -> bool {
        for hook in self.kill_hooks.borrow().iter() {
            hook();
        }
        self.kill_calls.set(self.kill_calls.get() + 1);
        if self.exit_on_kill
            && let Some(child) = self.me.upgrade()
        {
            runtime::queue_microtask(move || child.emit_exit(None));
        }
        true
    }
}

/// O `ITelemetryService` que registra os `publicLog2` (com o `sessionId` do `NullTelemetryService`).
#[derive(Default)]
pub struct RecordingTelemetryService {
    calls: RefCell<Vec<(String, Option<Dynamic>)>>,
}

impl RecordingTelemetryService {
    /// O `sessionId` do `NullTelemetryService`.
    pub const SESSION_ID: &'static str = "someValue.sessionId";

    /// `publicLog2.getCalls().map(call => call.args)`.
    pub fn calls(&self) -> Vec<(String, Option<Dynamic>)> {
        self.calls.borrow().clone()
    }
}

impl ITelemetryService for RecordingTelemetryService {
    fn telemetry_level(&self) -> TelemetryLevel {
        TelemetryLevel::None
    }

    fn session_id(&self) -> String {
        Self::SESSION_ID.to_owned()
    }

    fn machine_id(&self) -> String {
        "someValue.machineId".to_owned()
    }

    fn sqm_id(&self) -> String {
        "someValue.sqmId".to_owned()
    }

    fn dev_device_id(&self) -> String {
        "someValue.devDeviceId".to_owned()
    }

    fn first_session_date(&self) -> String {
        "someValue.firstSessionDate".to_owned()
    }

    fn send_error_telemetry(&self) -> bool {
        false
    }

    fn public_log(&self, _event_name: &str, _data: Option<ITelemetryData>) {}

    fn public_log2(&self, event_name: &str, data: Option<Dynamic>) {
        self.calls.borrow_mut().push((event_name.to_owned(), data));
    }

    fn public_log_error(&self, _error_event_name: &str, _data: Option<ITelemetryData>) {}

    fn public_log_error2(&self, _event_name: &str, _data: Option<Dynamic>) {}

    fn set_experiment_property(&self, _name: &str, _value: &str) {}

    fn set_common_property(&self, _name: &str, _value: Option<CommonPropertyValue>) {}
}
