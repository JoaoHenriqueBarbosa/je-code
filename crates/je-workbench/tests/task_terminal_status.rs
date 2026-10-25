//! Porte de `src/vs/workbench/contrib/tasks/test/browser/taskTerminalStatus.test.ts` (suíte `Task Terminal Status`), caso a
//! caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` vira o guarda criado no começo de cada caso, e o `setup` é [`setup`], que
//!   monta os dublês sem o `TestInstantiationService` (os decoradores são argumentos dos construtores do porte). O que o
//!   original põe no `store` (`taskTerminalStatus`, `testTerminal`, `problemCollector`) entra no guarda, junto com os
//!   emissores e a lista de status que o rastreador do porte conta.
//! - `TestTaskService` implementa o recorte `TaskTerminalStatusTaskService` do módulo de produção, e `triggerStateChange(
//!   event: Partial<ITaskEvent>)` recebe um `TaskStatusEvent`: o `terminalId` ausente dos literais do original é `None`, o
//!   `runType` e o `exitCode` ausentes também (nenhum evento do original traz `terminalId`, então nenhum casa terminal; as
//!   asserções valem sobre o status inicial `{ id, severity: Info }` que `addTerminal` adiciona, como no original).
//! - `TestaccessibilitySignalService.playSignal` é a struct de mesmo nome (não faz nada).
//! - `TestTerminal` implementa o recorte `TaskTerminal`: `statusList` é uma `TerminalStatusList` com o
//!   `TestConfigurationService` vazio, `onDisposed` é um emissor, e `registerMarker` e `addBufferMarker`, que o
//!   `Partial<ITerminalInstance>` não define, falham com `panic!` (o `TypeError` do método ausente). O `instanceId`, que o
//!   original não define (a chave do mapa é `undefined`), é 1.
//! - `TestTask extends CommonTask` é uma `InMemoryTask` com id `test`, sem rótulo, sem tipo e com opções vazias (`{}`).
//! - `TestProblemCollector` implementa o recorte `TaskProblemCollector` com os três eventos do original; `maxMarkerSeverity`
//!   é `None` (`undefined`), `isStartStop` é falso e `numberOfMatches` e `problemMatchers`, que o original não define,
//!   falham com `panic!` (nenhum caso chega a lê-los, porque nenhum evento casa terminal).
//! - `poll` espera com `common::timeout` e roda sob o relógio falso (`run_with_faked_timers`), de modo que os `setTimeout`
//!   de 10 ms do original não gastam tempo real. A condição de `poll` é verdadeira na primeira tentativa em todos os casos.
//! - `ok(actual.statuses.length === 1)` e as demais são `assert!` com a mesma mensagem. `statuses?.includes(
//!   SUCCEEDED_TASK_STATUS)` compara por referência, que no porte é `Rc::ptr_eq`.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use std::future::IntoFuture;
use std::rc::Rc;

use common::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite, run_with_faked_timers, timeout};
use je_base::event::{Emitter, Event};
use je_base::lifecycle::IDisposable;
use je_base::promise::Promise;
use je_editor::code_action::platform::{AccessibilitySignal, IAccessibilitySignalService};
use je_instantiation::Injectable;
use je_platform::configuration::service::IConfigurationService;
use je_platform::configuration::test_configuration_service::TestConfigurationService;
use je_platform::markers::markers::{Marker, MarkerSeverity};
use je_platform::terminal::capabilities::capabilities::IMarker;
use je_workbench::contrib::tasks::task_terminal_status::{
    TaskBufferMarker, TaskProblemCollector, TaskStatusEvent, TaskTerminal, TaskTerminalStatus, TaskTerminalStatusTaskService,
    active_task_status, failed_task_status, succeeded_task_status,
};
use je_workbench::contrib::tasks::tasks::{ConfigurationProperties, InMemoryTask, InMemoryTaskSource, RunOptions, Task, TaskEventKind, TaskRunType};
use je_workbench::contrib::terminal::browser::terminal_status_list::{ITerminalStatusList, TerminalStatusList};
use je_workbench::contrib::terminal::common::terminal::ITerminalStatus;

/// `TestTaskService`.
struct TestTaskService {
    on_did_state_change: Rc<Emitter<TaskStatusEvent>>,
}

impl TestTaskService {
    fn new() -> Rc<Self> {
        Rc::new(Self { on_did_state_change: Emitter::new() })
    }

    fn trigger_state_change(&self, event: TaskStatusEvent) {
        self.on_did_state_change.fire(event);
    }
}

impl IDisposable for TestTaskService {
    fn dispose(&self) {
        self.on_did_state_change.dispose();
    }
}

impl TaskTerminalStatusTaskService for TestTaskService {
    fn on_did_state_change(&self) -> Event<TaskStatusEvent> {
        self.on_did_state_change.event()
    }
}

/// `TestaccessibilitySignalService`.
struct TestaccessibilitySignalService;

impl IAccessibilitySignalService for TestaccessibilitySignalService {
    fn play_signal(&self, _cue: AccessibilitySignal) {}
}

/// `TestTerminal`.
struct TestTerminal {
    status_list: Rc<TerminalStatusList>,
    on_disposed: Rc<Emitter<()>>,
}

impl TaskTerminal for TestTerminal {
    fn instance_id(&self) -> u32 {
        1
    }

    fn status_list(&self) -> Rc<dyn ITerminalStatusList> {
        self.status_list.clone()
    }

    fn register_marker(&self) -> Option<Rc<dyn IMarker>> {
        panic!("registerMarker is not a function")
    }

    fn add_buffer_marker(&self, _options: TaskBufferMarker) {
        panic!("addBufferMarker is not a function")
    }

    fn on_disposed(&self) -> Event<()> {
        self.on_disposed.event()
    }
}

/// `TestProblemCollector`.
struct TestProblemCollector {
    on_did_find_first_match: Rc<Emitter<()>>,
    on_did_find_errors: Rc<Emitter<Vec<Marker>>>,
    on_did_request_invalidate_last_marker: Rc<Emitter<()>>,
}

impl IDisposable for TestProblemCollector {
    fn dispose(&self) {
        self.on_did_find_first_match.dispose();
        self.on_did_find_errors.dispose();
        self.on_did_request_invalidate_last_marker.dispose();
    }
}

impl TaskProblemCollector for TestProblemCollector {
    fn on_did_find_first_match(&self) -> Event<()> {
        self.on_did_find_first_match.event()
    }

    fn on_did_find_errors(&self) -> Event<Vec<Marker>> {
        self.on_did_find_errors.event()
    }

    fn on_did_request_invalidate_last_marker(&self) -> Event<()> {
        self.on_did_request_invalidate_last_marker.event()
    }

    fn number_of_matches(&self) -> usize {
        panic!("numberOfMatches is not defined")
    }

    fn max_marker_severity(&self) -> Option<MarkerSeverity> {
        None
    }

    fn problem_matchers_len(&self) -> usize {
        panic!("problemMatchers is not defined")
    }

    fn is_start_stop(&self) -> bool {
        false
    }
}

/// O que o `setup` da suíte guarda em variáveis.
struct Fixture {
    task_service: Rc<TestTaskService>,
    task_terminal_status: Rc<TaskTerminalStatus>,
    test_terminal: Rc<TestTerminal>,
    test_task: Task,
    problem_collector: Rc<TestProblemCollector>,
}

fn setup(store: &LeakGuard) -> Fixture {
    let task_service = store.add_rc(TestTaskService::new());
    let accessibility_signal_service: Rc<dyn IAccessibilitySignalService> = Rc::new(TestaccessibilitySignalService);
    let task_service_ref: Rc<dyn TaskTerminalStatusTaskService> = task_service.clone();
    let task_terminal_status = store.add_rc(TaskTerminalStatus::new(&task_service_ref, accessibility_signal_service));

    let configuration_service = TestConfigurationService::new(None);
    store.add_rc(configuration_service.on_did_change_configuration_emitter.clone());
    let configuration: Rc<dyn IConfigurationService> = configuration_service;
    let test_terminal = Rc::new(TestTerminal {
        status_list: store.add_rc(TerminalStatusList::create((), (configuration,))),
        on_disposed: {
            let on_disposed = Emitter::new();
            store.add(on_disposed.clone());
            on_disposed
        },
    });
    let test_task = Task::InMemory(InMemoryTask::new(
        "test",
        InMemoryTaskSource { label: String::new() },
        "",
        "",
        RunOptions::default(),
        ConfigurationProperties::default(),
    ));
    let problem_collector = store.add_rc(Rc::new(TestProblemCollector {
        on_did_find_first_match: Emitter::new(),
        on_did_find_errors: Emitter::new(),
        on_did_request_invalidate_last_marker: Emitter::new(),
    }));
    Fixture { task_service, task_terminal_status, test_terminal, test_task, problem_collector }
}

fn event(kind: TaskEventKind) -> TaskStatusEvent {
    TaskStatusEvent { kind, terminal_id: None, run_type: None, exit_code: None }
}

fn assert_status(actual: &dyn ITerminalStatusList, expected: &Rc<ITerminalStatus>) {
    assert!(actual.statuses().len() == 1, "# of statuses");
    assert!(actual.primary().is_some_and(|primary| primary.id == expected.id), "ID");
    assert!(actual.primary().is_some_and(|primary| primary.severity == expected.severity), "Severity");
}

async fn poll<T: Clone + 'static>(fn_: impl Fn() -> Promise<T>, accept_fn: impl Fn(&T) -> bool, timeout_message: &str) -> T {
    let retry_count: u64 = 200;
    let retry_interval: u64 = 10; // millis
    let mut trial = 1;
    let mut last_error = String::new();

    loop {
        if trial > retry_count {
            panic!("Timeout: {timeout_message} after {} seconds.\r{last_error}", (retry_count * retry_interval) as f64 / 1000.0);
        }

        match fn_().into_future().await {
            Ok(result) => {
                if accept_fn(&result) {
                    return result;
                }
                last_error = "Did not pass accept function".to_string();
            }
            Err(error) => last_error = error.to_string(),
        }

        timeout(retry_interval).await.unwrap();
        trial += 1;
    }
}

#[test]
fn should_add_failed_status_when_there_is_an_exit_code_on_task_end() {
    run_with_faked_timers(async move {
        let store = ensure_no_disposables_are_leaked_in_test_suite();
        let f = setup(&store);
        f.task_terminal_status.add_terminal(&f.test_task, f.test_terminal.clone(), f.problem_collector.clone());
        f.task_service.trigger_state_change(event(TaskEventKind::ProcessStarted));
        assert_status(f.test_terminal.status_list.as_ref(), &active_task_status());
        f.task_service.trigger_state_change(event(TaskEventKind::Inactive));
        assert_status(f.test_terminal.status_list.as_ref(), &succeeded_task_status());
        f.task_service.trigger_state_change(event(TaskEventKind::End));
        let test_terminal = f.test_terminal.clone();
        poll(
            || Promise::resolved(()),
            move |_| test_terminal.status_list.primary().is_some_and(|primary| primary.id == failed_task_status().id),
            "terminal status should be updated",
        )
        .await;
    });
}

#[test]
fn should_add_active_status_when_a_non_background_task_is_run_for_a_second_time_in_the_same_terminal() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let f = setup(&store);
    f.task_terminal_status.add_terminal(&f.test_task, f.test_terminal.clone(), f.problem_collector.clone());
    f.task_service.trigger_state_change(event(TaskEventKind::ProcessStarted));
    assert_status(f.test_terminal.status_list.as_ref(), &active_task_status());
    f.task_service.trigger_state_change(event(TaskEventKind::Inactive));
    assert_status(f.test_terminal.status_list.as_ref(), &succeeded_task_status());
    f.task_service.trigger_state_change(TaskStatusEvent { run_type: Some(TaskRunType::SingleRun), ..event(TaskEventKind::ProcessStarted) });
    assert_status(f.test_terminal.status_list.as_ref(), &active_task_status());
    f.task_service.trigger_state_change(event(TaskEventKind::Inactive));
    assert_status(f.test_terminal.status_list.as_ref(), &succeeded_task_status());
}

#[test]
fn should_drop_status_when_a_background_task_exits() {
    run_with_faked_timers(async move {
        let store = ensure_no_disposables_are_leaked_in_test_suite();
        let f = setup(&store);
        f.task_terminal_status.add_terminal(&f.test_task, f.test_terminal.clone(), f.problem_collector.clone());
        f.task_service.trigger_state_change(TaskStatusEvent { run_type: Some(TaskRunType::Background), ..event(TaskEventKind::ProcessStarted) });
        assert_status(f.test_terminal.status_list.as_ref(), &active_task_status());
        f.task_service.trigger_state_change(event(TaskEventKind::Inactive));
        assert_status(f.test_terminal.status_list.as_ref(), &succeeded_task_status());
        f.task_service.trigger_state_change(TaskStatusEvent { exit_code: Some(0), ..event(TaskEventKind::ProcessEnded) });
        let test_terminal = f.test_terminal.clone();
        poll(
            || Promise::resolved(()),
            move |_| !test_terminal.status_list.statuses().iter().any(|status| Rc::ptr_eq(status, &succeeded_task_status())),
            "terminal should have dropped status",
        )
        .await;
    });
}

#[test]
fn should_add_succeeded_status_when_a_non_background_task_exits() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let f = setup(&store);
    f.task_terminal_status.add_terminal(&f.test_task, f.test_terminal.clone(), f.problem_collector.clone());
    f.task_service.trigger_state_change(TaskStatusEvent { run_type: Some(TaskRunType::SingleRun), ..event(TaskEventKind::ProcessStarted) });
    assert_status(f.test_terminal.status_list.as_ref(), &active_task_status());
    f.task_service.trigger_state_change(event(TaskEventKind::Inactive));
    assert_status(f.test_terminal.status_list.as_ref(), &succeeded_task_status());
    f.task_service.trigger_state_change(TaskStatusEvent { exit_code: Some(0), ..event(TaskEventKind::ProcessEnded) });
    assert_status(f.test_terminal.status_list.as_ref(), &succeeded_task_status());
}
