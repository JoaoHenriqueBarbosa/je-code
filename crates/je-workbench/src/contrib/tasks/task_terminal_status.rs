//! Porte de `src/vs/workbench/contrib/tasks/browser/taskTerminalStatus.ts`: o `TaskTerminalStatus`, que mantém o status de
//! um terminal que roda uma tarefa (ativo, sucesso, erros, avisos, informações, e as variantes "esperando" das tarefas de
//! segundo plano) a partir dos eventos de estado do serviço de tarefas e do coletor de problemas, e toca os sinais de
//! acessibilidade de tarefa concluída ou com falha.
//!
//! Divergências:
//! - Os decoradores `@ITaskService` e `@IAccessibilitySignalService` são argumentos de [`TaskTerminalStatus::new`]. O
//!   `ITaskService` é o recorte [`TaskTerminalStatusTaskService`] (só `onDidStateChange`, que é o único membro lido).
//! - **Evento.** O `ITaskEvent` do original é lido por `kind`, `terminalId`, `runType` e `exitCode`, e o `Object.hasOwn(event,
//!   'terminalId')` distingue um evento sem terminal de um com `terminalId` indefinido. [`TaskStatusEvent`] guarda só esses
//!   quatro campos, com `terminalId` ausente como `None`, e `From<&TaskEvent>` converte o evento tipado do porte. Um
//!   `terminalId` zero é falso no original, então também não casa terminal algum.
//! - **`ITerminalInstance`.** É o recorte [`TaskTerminal`]: `instanceId`, `statusList`, `registerMarker`, `addBufferMarker` e
//!   `onDisposed` (que aqui não carrega a instância), os cinco membros que a classe lê.
//! - **`AbstractProblemCollector`.** É o recorte [`TaskProblemCollector`], implementado pelos três coletores do porte. O
//!   `instanceof StartStopProblemCollector` é [`TaskProblemCollector::is_start_stop`], e `problemMatchers.length` é
//!   [`TaskProblemCollector::problem_matchers_len`]. `maxMarkerSeverity` é `Option<MarkerSeverity>` (o `undefined` é `None`).
//! - **Identidade dos status.** O `ITerminalStatusList` compara os status por referência, então as constantes exportadas
//!   (`ACTIVE_TASK_STATUS`, `SUCCEEDED_TASK_STATUS`, `FAILED_TASK_STATUS`) e as internas são um único `Rc<ITerminalStatus>`
//!   por thread, devolvido pelas funções de mesmo nome em minúsculas.
//! - **Ícones.** `spinningLoading` é `ThemeIcon::from_id("loading~spin")` e os `Codicon` são `ThemeIcon::from_id` do id.
//! - **`IMarker` do xterm.** O marcador de `_marker` é o `IMarker` do porte; `this._marker` é compartilhado entre os terminais,
//!   como no original, e fica no estado interno ([`State`]).
//! - **`terminalMap`** é uma `DisposableMap` por `instanceId`, e o `ITerminalData` é [`TerminalData`], cujo `dispose` descarta o
//!   `store` do terminal.

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use je_base::event::Event;
use je_base::lifecycle::{Disposable, DisposableMap, DisposableRef, DisposableStore, IDisposable};
use je_base::nls::localize;
use je_base::severity::Severity;
use je_base::themables::ThemeIcon;
use je_editor::code_action::platform::{AccessibilitySignal, IAccessibilitySignalService};
use je_platform::markers::markers::{Marker as ProblemMarker, MarkerSeverity};
use je_platform::terminal::capabilities::capabilities::IMarker;

use super::problem_collectors::{AbstractProblemCollector, StartStopProblemCollector, WatchingProblemCollector};
use super::tasks::{Task, TaskEvent, TaskEventKind, TaskRunType};
use crate::contrib::terminal::browser::terminal_status_list::ITerminalStatusList;
use crate::contrib::terminal::common::terminal::ITerminalStatus;

const TASK_TERMINAL_STATUS_ID: &str = "task_terminal_status";

/// Os status de terminal de uma tarefa, criados uma vez por thread (a identidade é a do `Rc`).
struct TaskStatuses {
    active: Rc<ITerminalStatus>,
    succeeded: Rc<ITerminalStatus>,
    succeeded_inactive: Rc<ITerminalStatus>,
    failed: Rc<ITerminalStatus>,
    failed_inactive: Rc<ITerminalStatus>,
    warning: Rc<ITerminalStatus>,
    warning_inactive: Rc<ITerminalStatus>,
    info: Rc<ITerminalStatus>,
    info_inactive: Rc<ITerminalStatus>,
}

fn task_status(icon: &str, severity: Severity, tooltip: String) -> Rc<ITerminalStatus> {
    Rc::new(ITerminalStatus {
        id: TASK_TERMINAL_STATUS_ID.to_string(),
        icon: Some(ThemeIcon::from_id(icon)),
        severity,
        tooltip: Some(tooltip),
        detailed_tooltip: None,
        hover_actions: None,
    })
}

impl TaskStatuses {
    fn new() -> Self {
        Self {
            active: task_status("loading~spin", Severity::Info, localize("taskTerminalStatus.active", "Task is running", &[])),
            succeeded: task_status("check", Severity::Info, localize("taskTerminalStatus.succeeded", "Task succeeded", &[])),
            succeeded_inactive: task_status("check", Severity::Info, localize("taskTerminalStatus.succeededInactive", "Task succeeded and waiting...", &[])),
            failed: task_status("error", Severity::Error, localize("taskTerminalStatus.errors", "Task has errors", &[])),
            failed_inactive: task_status("error", Severity::Error, localize("taskTerminalStatus.errorsInactive", "Task has errors and is waiting...", &[])),
            warning: task_status("warning", Severity::Warning, localize("taskTerminalStatus.warnings", "Task has warnings", &[])),
            warning_inactive: task_status("warning", Severity::Warning, localize("taskTerminalStatus.warningsInactive", "Task has warnings and is waiting...", &[])),
            info: task_status("info", Severity::Info, localize("taskTerminalStatus.infos", "Task has infos", &[])),
            info_inactive: task_status("info", Severity::Info, localize("taskTerminalStatus.infosInactive", "Task has infos and is waiting...", &[])),
        }
    }
}

thread_local! {
    static STATUSES: TaskStatuses = TaskStatuses::new();
}

/// `ACTIVE_TASK_STATUS`.
pub fn active_task_status() -> Rc<ITerminalStatus> {
    STATUSES.with(|statuses| statuses.active.clone())
}

/// `SUCCEEDED_TASK_STATUS`.
pub fn succeeded_task_status() -> Rc<ITerminalStatus> {
    STATUSES.with(|statuses| statuses.succeeded.clone())
}

/// `FAILED_TASK_STATUS`.
pub fn failed_task_status() -> Rc<ITerminalStatus> {
    STATUSES.with(|statuses| statuses.failed.clone())
}

/// O que `TaskTerminalStatus` lê de um `ITaskEvent`: `{ kind, terminalId?, runType?, exitCode? }`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TaskStatusEvent {
    pub kind: TaskEventKind,
    pub terminal_id: Option<i64>,
    pub run_type: Option<TaskRunType>,
    pub exit_code: Option<i64>,
}

impl From<&TaskEvent> for TaskStatusEvent {
    fn from(event: &TaskEvent) -> Self {
        let kind = event.kind();
        match event {
            TaskEvent::Changed => Self { kind, terminal_id: None, run_type: None, exit_code: None },
            TaskEvent::ProcessStarted { common, terminal_id, .. }
            | TaskEvent::Terminated { common, terminal_id, .. }
            | TaskEvent::Start { common, terminal_id, .. } => Self { kind, terminal_id: Some(*terminal_id), run_type: Some(common.run_type), exit_code: None },
            TaskEvent::ProcessEnded { common, terminal_id, exit_code, .. } => Self { kind, terminal_id: *terminal_id, run_type: Some(common.run_type), exit_code: *exit_code },
            TaskEvent::General { common, terminal_id, .. } => Self { kind, terminal_id: *terminal_id, run_type: Some(common.run_type), exit_code: None },
            TaskEvent::ProblemMatcherEnded { common, .. } => Self { kind, terminal_id: None, run_type: Some(common.run_type), exit_code: None },
        }
    }
}

/// `ITaskService`, reduzido ao `onDidStateChange` (ver as divergências do módulo).
pub trait TaskTerminalStatusTaskService {
    fn on_did_state_change(&self) -> Event<TaskStatusEvent>;
}

/// As opções de `ITerminalInstance.addBufferMarker` que a classe usa: `{ marker, hoverMessage, disableCommandStorage }`.
pub struct TaskBufferMarker {
    pub marker: Rc<dyn IMarker>,
    pub hover_message: String,
    pub disable_command_storage: bool,
}

/// `ITerminalInstance`, reduzido aos membros que `TaskTerminalStatus` lê (ver as divergências do módulo).
pub trait TaskTerminal {
    fn instance_id(&self) -> u32;
    fn status_list(&self) -> Rc<dyn ITerminalStatusList>;
    /// `registerMarker()`.
    fn register_marker(&self) -> Option<Rc<dyn IMarker>>;
    /// `addBufferMarker(options)`.
    fn add_buffer_marker(&self, options: TaskBufferMarker);
    /// `onDisposed`.
    fn on_disposed(&self) -> Event<()>;
}

/// `AbstractProblemCollector`, reduzido aos membros que `TaskTerminalStatus` lê (ver as divergências do módulo).
pub trait TaskProblemCollector {
    fn on_did_find_first_match(&self) -> Event<()>;
    fn on_did_find_errors(&self) -> Event<Vec<ProblemMarker>>;
    fn on_did_request_invalidate_last_marker(&self) -> Event<()>;
    fn number_of_matches(&self) -> usize;
    fn max_marker_severity(&self) -> Option<MarkerSeverity>;
    /// `problemMatchers.length`.
    fn problem_matchers_len(&self) -> usize;
    /// `problemMatcher instanceof StartStopProblemCollector`.
    fn is_start_stop(&self) -> bool;
}

impl TaskProblemCollector for AbstractProblemCollector {
    fn on_did_find_first_match(&self) -> Event<()> {
        AbstractProblemCollector::on_did_find_first_match(self)
    }

    fn on_did_find_errors(&self) -> Event<Vec<ProblemMarker>> {
        AbstractProblemCollector::on_did_find_errors(self)
    }

    fn on_did_request_invalidate_last_marker(&self) -> Event<()> {
        AbstractProblemCollector::on_did_request_invalidate_last_marker(self)
    }

    fn number_of_matches(&self) -> usize {
        AbstractProblemCollector::number_of_matches(self)
    }

    fn max_marker_severity(&self) -> Option<MarkerSeverity> {
        AbstractProblemCollector::max_marker_severity(self)
    }

    fn problem_matchers_len(&self) -> usize {
        self.problem_matchers.len()
    }

    fn is_start_stop(&self) -> bool {
        false
    }
}

/// Os coletores concretos herdam tudo da base e só diferem no `instanceof`.
macro_rules! delegate_collector_to_base {
    ($collector:ty, $is_start_stop:expr) => {
        impl TaskProblemCollector for $collector {
            fn on_did_find_first_match(&self) -> Event<()> {
                TaskProblemCollector::on_did_find_first_match(&*self.base)
            }

            fn on_did_find_errors(&self) -> Event<Vec<ProblemMarker>> {
                TaskProblemCollector::on_did_find_errors(&*self.base)
            }

            fn on_did_request_invalidate_last_marker(&self) -> Event<()> {
                TaskProblemCollector::on_did_request_invalidate_last_marker(&*self.base)
            }

            fn number_of_matches(&self) -> usize {
                TaskProblemCollector::number_of_matches(&*self.base)
            }

            fn max_marker_severity(&self) -> Option<MarkerSeverity> {
                TaskProblemCollector::max_marker_severity(&*self.base)
            }

            fn problem_matchers_len(&self) -> usize {
                TaskProblemCollector::problem_matchers_len(&*self.base)
            }

            fn is_start_stop(&self) -> bool {
                $is_start_stop
            }
        }
    };
}

delegate_collector_to_base!(StartStopProblemCollector, true);
delegate_collector_to_base!(WatchingProblemCollector, false);

/// `ITerminalData`.
struct TerminalData {
    terminal: Rc<dyn TaskTerminal>,
    task: Task,
    status: Rc<ITerminalStatus>,
    problem_matcher: Rc<dyn TaskProblemCollector>,
    task_run_ended: Cell<bool>,
    store: Rc<DisposableStore>,
}

impl IDisposable for TerminalData {
    fn dispose(&self) {
        self.store.dispose();
    }
}

/// O estado que os ouvintes compartilham.
struct State {
    terminal_map: Rc<DisposableMap<i64, TerminalData>>,
    marker: RefCell<Option<Rc<dyn IMarker>>>,
    accessibility_signal_service: Rc<dyn IAccessibilitySignalService>,
}

impl State {
    fn terminal_from_event(&self, event: &TaskStatusEvent) -> Option<Rc<TerminalData>> {
        let terminal_id = event.terminal_id.filter(|terminal_id| *terminal_id != 0)?;
        self.terminal_map.get(&terminal_id)
    }

    fn event_end(&self, event: &TaskStatusEvent) {
        let Some(terminal_data) = self.terminal_from_event(event) else { return };
        terminal_data.task_run_ended.set(true);
        let status_list = terminal_data.terminal.status_list();
        status_list.remove(&terminal_data.status);
        let max_marker_severity = terminal_data.problem_matcher.max_marker_severity().filter(|severity| severity.0 != 0);
        let statuses = STATUSES.with(|statuses| (statuses.succeeded.clone(), statuses.failed.clone(), statuses.warning.clone(), statuses.info.clone()));
        if event.exit_code == Some(0) && max_marker_severity.is_none_or(|severity| severity.0 < MarkerSeverity::WARNING.0) {
            self.accessibility_signal_service.play_signal(AccessibilitySignal::TaskCompleted);
            if terminal_data.task.common().configuration_properties.is_background == Some(true) {
                for status in status_list.statuses() {
                    status_list.remove(&status);
                }
            } else {
                status_list.add(statuses.0, None);
            }
        } else if event.exit_code.is_some_and(|exit_code| exit_code != 0) || max_marker_severity == Some(MarkerSeverity::ERROR) {
            self.accessibility_signal_service.play_signal(AccessibilitySignal::TaskFailed);
            status_list.add(statuses.1, None);
        } else if max_marker_severity == Some(MarkerSeverity::WARNING) {
            self.accessibility_signal_service.play_signal(AccessibilitySignal::TaskFailed);
            status_list.add(statuses.2, None);
        } else if max_marker_severity == Some(MarkerSeverity::INFO) {
            status_list.add(statuses.3, None);
        }
    }

    fn event_inactive(&self, event: &TaskStatusEvent) {
        let Some(terminal_data) = self.terminal_from_event(event) else { return };
        if terminal_data.task_run_ended.get() {
            return;
        }
        let status_list = terminal_data.terminal.status_list();
        status_list.remove(&terminal_data.status);
        let max_marker_severity = terminal_data.problem_matcher.max_marker_severity();
        if terminal_data.problem_matcher.number_of_matches() == 0 {
            self.accessibility_signal_service.play_signal(AccessibilitySignal::TaskCompleted);
            status_list.add(STATUSES.with(|statuses| statuses.succeeded_inactive.clone()), None);
        } else if max_marker_severity == Some(MarkerSeverity::ERROR) {
            self.accessibility_signal_service.play_signal(AccessibilitySignal::TaskFailed);
            status_list.add(STATUSES.with(|statuses| statuses.failed_inactive.clone()), None);
        } else if max_marker_severity == Some(MarkerSeverity::WARNING) {
            status_list.add(STATUSES.with(|statuses| statuses.warning_inactive.clone()), None);
        } else if max_marker_severity == Some(MarkerSeverity::INFO) {
            status_list.add(STATUSES.with(|statuses| statuses.info_inactive.clone()), None);
        }
    }

    fn event_active(&self, event: &TaskStatusEvent) {
        let Some(terminal_data) = self.terminal_from_event(event) else { return };
        terminal_data.task_run_ended.set(false);
        let status_list = terminal_data.terminal.status_list();
        status_list.remove(&terminal_data.status);
        // We don't want to show an infinite status for a background task that doesn't have a problem matcher.
        if terminal_data.problem_matcher.is_start_stop()
            || terminal_data.problem_matcher.problem_matchers_len() > 0
            || event.run_type == Some(TaskRunType::SingleRun)
        {
            status_list.add(active_task_status(), None);
        }
    }
}

/// `TaskTerminalStatus`.
pub struct TaskTerminalStatus {
    base: Rc<Disposable>,
    state: Rc<State>,
}

impl TaskTerminalStatus {
    pub fn new(task_service: &Rc<dyn TaskTerminalStatusTaskService>, accessibility_signal_service: Rc<dyn IAccessibilitySignalService>) -> Rc<Self> {
        let base = Disposable::new();
        let terminal_map = DisposableMap::new();
        base.register(terminal_map.clone());
        let state = Rc::new(State { terminal_map, marker: RefCell::new(None), accessibility_signal_service });
        let listener_state = Rc::clone(&state);
        base.register(task_service.on_did_state_change().listen(move |event| match event.kind {
            TaskEventKind::ProcessStarted | TaskEventKind::Active => listener_state.event_active(event),
            TaskEventKind::Inactive => listener_state.event_inactive(event),
            TaskEventKind::ProcessEnded => listener_state.event_end(event),
            _ => {}
        }));
        Rc::new(Self { base, state })
    }

    pub fn add_terminal(&self, task: &Task, terminal: Rc<dyn TaskTerminal>, problem_matcher: Rc<dyn TaskProblemCollector>) {
        let status = Rc::new(ITerminalStatus {
            id: TASK_TERMINAL_STATUS_ID.to_string(),
            severity: Severity::Info,
            icon: None,
            tooltip: None,
            detailed_tooltip: None,
            hover_actions: None,
        });
        terminal.status_list().add(Rc::clone(&status), None);
        let store = DisposableStore::new();
        let weak_state: Weak<State> = Rc::downgrade(&self.state);

        let (state, store_for_marker, terminal_for_marker) = (weak_state.clone(), Rc::clone(&store), Rc::clone(&terminal));
        store.add(problem_matcher.on_did_find_first_match().listen(move |_| {
            let Some(state) = state.upgrade() else { return };
            let marker = terminal_for_marker.register_marker();
            *state.marker.borrow_mut() = marker.clone();
            if let Some(marker) = marker {
                let marker: DisposableRef = marker;
                store_for_marker.add(marker);
            }
        }));

        let (state, terminal_for_errors) = (weak_state.clone(), Rc::clone(&terminal));
        store.add(problem_matcher.on_did_find_errors().listen(move |_| {
            let Some(state) = state.upgrade() else { return };
            let marker = state.marker.borrow().clone();
            if let Some(marker) = marker {
                terminal_for_errors.add_buffer_marker(TaskBufferMarker {
                    marker,
                    hover_message: localize("task.watchFirstError", "Beginning of detected errors for this run", &[]),
                    disable_command_storage: true,
                });
            }
        }));

        let state = weak_state.clone();
        store.add(problem_matcher.on_did_request_invalidate_last_marker().listen(move |_| {
            let Some(state) = state.upgrade() else { return };
            let marker = state.marker.borrow_mut().take();
            if let Some(marker) = marker {
                marker.dispose();
            }
        }));

        let instance_id = i64::from(terminal.instance_id());
        let state = weak_state;
        store.add(terminal.on_disposed().listen(move |_| {
            if let Some(state) = state.upgrade() {
                state.terminal_map.delete_and_dispose(&instance_id);
            }
        }));

        self.state.terminal_map.set(
            instance_id,
            Rc::new(TerminalData { terminal, task: task.clone(), status, problem_matcher, task_run_ended: Cell::new(false), store }),
            false,
        );
    }
}

impl IDisposable for TaskTerminalStatus {
    fn dispose(&self) {
        self.base.dispose();
    }
}
