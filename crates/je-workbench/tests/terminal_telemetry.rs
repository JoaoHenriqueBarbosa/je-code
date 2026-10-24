//! Porte de `src/vs/workbench/contrib/terminalContrib/telemetry/test/browser/terminalTelemetry.test.ts` (suíte `TerminalTelemetryContribution`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - **`ensureNoDisposablesAreLeakedInTestSuite()`.** O guarda de vazamentos é criado no começo do caso e descarta os emissores
//!   e a contribuição ao fim.
//! - **`captureGlobalTimeApi()` e `pushGlobalTimeApi({ ...timeApi, setTimeout, clearTimeout })`.** É o [`RecordingTimeApi`]: delega
//!   tudo à `TimeApi` capturada (`runtime::time_api()`) e troca só o `set_timeout` (guarda o atraso e devolve o handle `1`) e o
//!   `clear_timeout` (guarda o handle), instalado com `push_global_time_api`. O `setTimeout` do original recebe `(handler,
//!   timeout)` e não o executa; o dublê também não.
//! - **`upcastPartial<ITerminalService>({ ... })`, `upcastPartial<ILifecycleService>({ onWillShutdown })`,
//!   `upcastPartial<ITerminalEditorService>({ getInputFromResource })` e `upcastPartial<ITerminalInstance>({ ... })`.** São o
//!   `PartialTerminalService` e o `PartialTerminalInstance` de `tests/support/terminal_contrib_support.rs`, o
//!   [`PartialLifecycleService`] e o [`NotAnEditorTerminalService`]: o que o teste não define lança `<membro> is not a function`.
//! - **`publicLog2: () => { telemetryEvents++; }`.** É o [`CountingTelemetryService`], que conta as chamadas de `public_log2`.
//! - **`await Promise.resolve()` (duas vezes).** O runtime de microtarefas do porte usa mais rodadas que o V8 para a mesma cadeia
//!   (`then`, `race` e a reação do cancelamento), então o teste espera [`MICROTASK_ROUNDS`] promessas resolvidas.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod support;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use common::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite, run_with_faked_timers};
use je_base::dynamic::Dynamic;
use je_base::errors::Error;
use je_base::event::{Emitter, Event};
use je_base::lifecycle::IDisposable;
use je_base::promise::Promise;
use je_base::runtime::{self, TimerId};
use je_base::uri::URI;
use je_base::virtual_scheduling::push_global_time_api;
use je_base::virtual_scheduling::time_api::{FrameCallback, Handler, TimeApi};
use je_platform::configuration::test_configuration_service::TestConfigurationService;
use je_platform::telemetry::telemetry::{CommonPropertyValue, ITelemetryData, ITelemetryService, TelemetryLevel};
use je_platform::telemetry::telemetry_utils::NullTelemetryServiceShape;
use je_platform::terminal::capabilities::terminal_capability_store::TerminalCapabilityStore;
use je_platform::terminal::terminal::{IShellLaunchConfig, TerminalLocation};
use je_workbench::contrib::terminal::browser::terminal::{ITerminalEditorService, ITerminalInstance, TerminalEditorInput};
use je_workbench::contrib::terminal_contrib::telemetry::terminal_telemetry::TerminalTelemetryContribution;
use je_workbench::services::lifecycle::lifecycle::{
    BeforeShutdownErrorEvent, BeforeShutdownEvent, ILifecycleService, LifecyclePhase, StartupKind, WillShutdownEvent,
};
use support::terminal_contrib_support::{PartialTerminalInstance, PartialTerminalService, unmocked};

/// As rodadas de microtarefa que o teste espera no lugar de `await Promise.resolve()`.
const MICROTASK_ROUNDS: usize = 20;

async fn flush_microtasks() {
    for _ in 0..MICROTASK_ROUNDS {
        Promise::resolved(()).await.unwrap();
    }
}

/// `{ ...timeApi, setTimeout, clearTimeout }`.
struct RecordingTimeApi {
    inner: Rc<dyn TimeApi>,
    /// `scheduledTimeout`.
    scheduled_timeout: Cell<Option<u64>>,
    /// `clearedTimeout`.
    cleared_timeout: Cell<Option<TimerId>>,
}

impl TimeApi for RecordingTimeApi {
    fn set_timeout(&self, _handler: Handler, timeout: u64) -> TimerId {
        self.scheduled_timeout.set(Some(timeout));
        TimerId::from_raw(1)
    }

    fn clear_timeout(&self, id: TimerId) {
        self.cleared_timeout.set(Some(id));
    }

    fn set_interval(&self, handler: Rc<dyn Fn()>, interval: u64) -> TimerId {
        self.inner.set_interval(handler, interval)
    }

    fn clear_interval(&self, id: TimerId) {
        self.inner.clear_interval(id);
    }

    fn set_immediate(&self, handler: Handler) -> Result<TimerId, Handler> {
        self.inner.set_immediate(handler)
    }

    fn clear_immediate(&self, id: TimerId) {
        self.inner.clear_immediate(id);
    }

    fn request_animation_frame(&self, callback: FrameCallback) -> Result<TimerId, FrameCallback> {
        self.inner.request_animation_frame(callback)
    }

    fn cancel_animation_frame(&self, id: TimerId) {
        self.inner.cancel_animation_frame(id);
    }

    fn date_now(&self) -> u64 {
        self.inner.date_now()
    }

    fn performance_now(&self) -> f64 {
        self.inner.performance_now()
    }

    fn performance_time_origin(&self) -> f64 {
        self.inner.performance_time_origin()
    }
}

/// `upcastPartial<ILifecycleService>({ onWillShutdown: onWillShutdown.event })`.
struct PartialLifecycleService {
    on_will_shutdown: Rc<Emitter<WillShutdownEvent>>,
}

impl ILifecycleService for PartialLifecycleService {
    fn startup_kind(&self) -> StartupKind {
        unmocked("startupKind")
    }
    fn phase(&self) -> LifecyclePhase {
        unmocked("phase")
    }
    fn set_phase(&self, _phase: LifecyclePhase) {
        unmocked("phase")
    }
    fn on_before_shutdown(&self) -> Event<BeforeShutdownEvent> {
        unmocked("onBeforeShutdown")
    }
    fn on_shutdown_veto(&self) -> Event<()> {
        unmocked("onShutdownVeto")
    }
    fn on_before_shutdown_error(&self) -> Event<BeforeShutdownErrorEvent> {
        unmocked("onBeforeShutdownError")
    }
    fn on_will_shutdown(&self) -> Event<WillShutdownEvent> {
        self.on_will_shutdown.event()
    }
    fn will_shutdown(&self) -> bool {
        unmocked("willShutdown")
    }
    fn on_did_shutdown(&self) -> Event<()> {
        unmocked("onDidShutdown")
    }
    fn when(&self, _phase: LifecyclePhase) -> Promise<()> {
        unmocked("when")
    }
    fn shutdown(&self) -> Promise<()> {
        unmocked("shutdown")
    }
}

/// `upcastPartial<ITerminalEditorService>({ getInputFromResource: () => { throw new Error('Not an editor terminal'); } })`.
struct NotAnEditorTerminalService;

impl ITerminalEditorService for NotAnEditorTerminalService {
    fn reveal_active_editor(&self, _preserve_focus: bool) -> Promise<()> {
        unmocked("revealActiveEditor")
    }

    fn get_input_from_resource(&self, _resource: &URI) -> Result<TerminalEditorInput, Error> {
        Err(Error::new("Not an editor terminal"))
    }
}

/// `upcastPartial<ITelemetryService>({ publicLog2: () => { telemetryEvents++; } })`.
#[derive(Default)]
struct CountingTelemetryService {
    null: NullTelemetryServiceShape,
    events: Cell<usize>,
}

impl ITelemetryService for CountingTelemetryService {
    fn telemetry_level(&self) -> TelemetryLevel {
        self.null.telemetry_level()
    }
    fn session_id(&self) -> String {
        self.null.session_id()
    }
    fn machine_id(&self) -> String {
        self.null.machine_id()
    }
    fn sqm_id(&self) -> String {
        self.null.sqm_id()
    }
    fn dev_device_id(&self) -> String {
        self.null.dev_device_id()
    }
    fn first_session_date(&self) -> String {
        self.null.first_session_date()
    }
    fn send_error_telemetry(&self) -> bool {
        self.null.send_error_telemetry()
    }
    fn public_log(&self, event_name: &str, data: Option<ITelemetryData>) {
        self.null.public_log(event_name, data);
    }
    fn public_log2(&self, _event_name: &str, _data: Option<Dynamic>) {
        self.events.set(self.events.get() + 1);
    }
    fn public_log_error(&self, error_event_name: &str, data: Option<ITelemetryData>) {
        self.null.public_log_error(error_event_name, data);
    }
    fn public_log_error2(&self, event_name: &str, data: Option<Dynamic>) {
        self.null.public_log_error2(event_name, data);
    }
    fn set_experiment_property(&self, name: &str, value: &str) {
        self.null.set_experiment_property(name, value);
    }
    fn set_common_property(&self, name: &str, value: Option<CommonPropertyValue>) {
        self.null.set_common_property(name, value);
    }
}

/// `{ scheduledTimeout, clearedTimeout, telemetryEvents }`.
fn state(time_api: &RecordingTimeApi, telemetry_service: &CountingTelemetryService) -> (Option<u64>, Option<TimerId>, usize) {
    (time_api.scheduled_timeout.get(), time_api.cleared_timeout.get(), telemetry_service.events.get())
}

#[test]
fn cancels_the_shell_integration_timeout_when_the_terminal_or_contribution_is_disposed() {
    run_with_faked_timers(async {
        let guard: LeakGuard = ensure_no_disposables_are_leaked_in_test_suite();
        let on_did_create_instance = guard.add_rc(Emitter::<Rc<dyn ITerminalInstance>>::new());
        let on_any_instance_shell_type_changed = guard.add_rc(Emitter::<Rc<dyn ITerminalInstance>>::new());
        let on_disposed = guard.add_rc(Emitter::<Rc<dyn ITerminalInstance>>::new());
        let on_will_shutdown = guard.add_rc(Emitter::<WillShutdownEvent>::new());
        let timeout_handle = TimerId::from_raw(1);

        let time_api = Rc::new(RecordingTimeApi { inner: runtime::time_api(), scheduled_timeout: Cell::new(None), cleared_timeout: Cell::new(None) });
        guard.add(push_global_time_api(time_api.clone()));

        let terminal_service = Rc::new(PartialTerminalService {
            on_did_create_instance: on_did_create_instance.clone(),
            on_any_instance_shell_type_changed: on_any_instance_shell_type_changed.clone(),
        });
        let lifecycle_service = Rc::new(PartialLifecycleService { on_will_shutdown });
        let telemetry_service = Rc::new(CountingTelemetryService::default());
        let configuration_service = TestConfigurationService::new(None);
        guard.add_rc(configuration_service.on_did_change_configuration_emitter.clone());
        let contribution = guard.add_rc(TerminalTelemetryContribution::new(
            lifecycle_service,
            terminal_service,
            Rc::new(NotAnEditorTerminalService),
            configuration_service,
            telemetry_service.clone(),
        ));

        let instance: Rc<dyn ITerminalInstance> = Rc::new(PartialTerminalInstance {
            resource: Some(URI::parse("terminal:test", false)),
            target: Some(TerminalLocation::Panel),
            process_ready: Some(Promise::resolved(())),
            on_disposed: Some(on_disposed.clone()),
            shell_launch_config: Some(Rc::new(RefCell::new(IShellLaunchConfig::default()))),
            capabilities: Some(guard.add_rc(TerminalCapabilityStore::new())),
            has_remote_authority: Some(false),
            used_shell_integration_injection: Some(false),
            session_id: Some("test".to_owned()),
            ..PartialTerminalInstance::default()
        });

        on_did_create_instance.fire(instance.clone());
        on_disposed.fire(instance.clone());
        flush_microtasks().await;

        assert_eq!(state(&time_api, &telemetry_service), (Some(10_000), Some(timeout_handle), 1));

        time_api.scheduled_timeout.set(None);
        time_api.cleared_timeout.set(None);
        on_did_create_instance.fire(instance);
        flush_microtasks().await;
        contribution.dispose();
        flush_microtasks().await;

        assert_eq!(state(&time_api, &telemetry_service), (Some(10_000), Some(timeout_handle), 1));
    });
}
