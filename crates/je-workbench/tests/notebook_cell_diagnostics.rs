//! Porte de `src/vs/workbench/contrib/notebook/test/browser/contrib/notebookCellDiagnostics.test.ts` (suíte `notebookCellDiagnostics`), caso a caso e na mesma ordem. Os 2 casos estão portados.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` é o `LeakGuard` do caso, e `await withTestNotebook(cells, async (editor, viewModel,
//!   store, accessor) => ..., instantiationService)` é o `with_test_notebook_services` com o corpo síncrono sob o relógio simulado; o
//!   `setup` do original (o `setupInstantiationService(disposables)` e os `instantiationService.stub(...)`) é o [`setup`] daqui, que
//!   troca no `NotebookTestServices` o serviço de execução, o de marcadores, o de agentes de chat e a configuração. O serviço de
//!   configuração não tem identificador no porte, então o `config.setUserConfiguration(NotebookSetting.cellFailureDiagnostics,
//!   true)` age direto no `TestConfigurationService` que o `NotebookTestServices` guarda.
//! - `TestExecutionService extends TestNotebookExecutionStateService` com `override onDidChangeExecution` é o
//!   `TestNotebookExecutionStateService` com `did_change_execution` apontando para o emissor do caso, e `fireExecutionChanged` é o
//!   [`fire_execution_changed`] com o evento de célula (`affectsNotebook` e `affectsCell` sempre verdadeiros).
//! - `{} as INotebookCellExecution` é o [`EmptyCellExecution`], que lança em qualquer método (um objeto vazio não tem nenhum).
//! - `new class extends mock<IChatAgentService>()` é o [`TestChatAgentService`], com o agente `testEditorAgent` no local `Notebook`;
//!   o `IChatAgentData` do porte guarda só `id`, `name`, `isDefault`, `locations` e `modes` (ver `chat_agents`).
//! - `markerService` é o [`TestMarkerService`]: `changeOne` guarda os marcadores por recurso (o `ResourceMap`, aqui por
//!   `URI.toString()`) e dispara `onMarkersUpdated`; os demais métodos, que o `mock` deixa indefinidos, devolvem o vazio.
//! - `cell.model.internalMetadata.lastRunSuccess = false` e `.error = {...}` mudam o objeto no lugar, sem evento; o modelo do porte é
//!   imutável, então é um único `set_internal_metadata` com os dois campos (o evento que ele dispara só refaz o layout da célula).
//!   O `uri` do erro é o `UriComponents` do `cell.uri` (o segundo caso usa o da primeira célula nas duas, como o original).
//! - `await new Promise(resolve => Event.once(markerService.onMarkersUpdated)(resolve))` é o [`MarkersUpdatedWatcher`]: um ouvinte
//!   que marca o disparo, e `await` é o avanço dos timers simulados um a um só até ele chegar (o `accumulate` de 200 ms do
//!   `CellDiagnostics` corre nesses timers).
//! - `cell?.executionErrorDiagnostic.get()?.message` é o `get()` do observável, e `markers.get(cell.uri)?.length` é o
//!   [`TestMarkerService::marker_count`].

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

mod support;

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::dynamic::Dynamic;
use je_base::errors::{self, Error};
use je_base::event::{Emitter, Event};
use je_base::lifecycle::{self, DisposableRef, DisposableStore, IDisposable};
use je_base::observable::Observable;
use je_base::runtime;
use je_base::uri::URI;
use je_editor::language_service::I_LANGUAGE_SERVICE;
use je_editor::range::Range;
use je_instantiation::{InstantiationService, ServiceCollection};
use je_platform::configuration::service::IConfigurationService;
use je_platform::configuration::test_configuration_service::TestConfigurationService;
use je_platform::markers::markers::{I_MARKER_SERVICE, IMarkerService, Marker, MarkerChangeOptions, MarkerData, MarkerReadOptions, MarkerStatistics, ResourceMarker};
use je_workbench::contrib::chat::common::constants::{ChatAgentLocation, ChatModeKind};
use je_workbench::contrib::chat::common::participants::chat_agents::{IChatAgentData, IChatAgentService, I_CHAT_AGENT_SERVICE};
use je_workbench::contrib::notebook::browser::contrib::cell_diagnostics::cell_diagnostic_editor_contrib::{CellDiagnostics, ICellDiagnosticsEditor};
use je_workbench::contrib::notebook::browser::view_model::notebook_view_model_impl::{CellViewModel, NotebookViewModel};
use je_workbench::contrib::notebook::common::notebook_common::{CellKind, ICell, NotebookSetting};
use je_workbench::contrib::notebook::common::notebook_execution_state_service::{
    CellExecutionErrorStack, ExecutionStateChange, ICellExecuteUpdate, ICellExecutionComplete, ICellExecutionError, ICellExecutionStateChangedEvent,
    INotebookCellExecution, INotebookExecutionStateService, I_NOTEBOOK_EXECUTION_STATE_SERVICE,
};
use je_workbench::contrib::notebook::common::notebook_logging_service::I_NOTEBOOK_LOGGING_SERVICE;
use je_workbench::contrib::notebook::common::notebook_common::NotebookCellExecutionState;
use je_workbench::services::language_detection::language_detection_worker_service::I_LANGUAGE_DETECTION_SERVICE;
use support::test_notebook_editor::{
    ActiveTestNotebookEditorDelegate, NotebookTestServices, TestNotebookExecutionStateService, empty_metadata, setup_notebook_services, with_test_notebook_services,
};

/// `{} as INotebookCellExecution`: um objeto vazio, que não responde a nenhum método.
struct EmptyCellExecution;

impl INotebookCellExecution for EmptyCellExecution {
    fn notebook(&self) -> URI {
        errors::throw(Error::new("notebook is not a function"))
    }

    fn cell_handle(&self) -> i64 {
        errors::throw(Error::new("cellHandle is not a function"))
    }

    fn state(&self) -> NotebookCellExecutionState {
        errors::throw(Error::new("state is not a function"))
    }

    fn did_pause(&self) -> bool {
        errors::throw(Error::new("didPause is not a function"))
    }

    fn is_paused(&self) -> bool {
        errors::throw(Error::new("isPaused is not a function"))
    }

    fn confirm(&self) {
        errors::throw(Error::new("confirm is not a function"))
    }

    fn update(&self, _updates: Vec<ICellExecuteUpdate>) {
        errors::throw(Error::new("update is not a function"))
    }

    fn complete(&self, _complete: ICellExecutionComplete) {
        errors::throw(Error::new("complete is not a function"))
    }
}

/// O objeto literal que `fireExecutionChanged` dispara.
struct TestCellExecutionChange {
    notebook: URI,
    cell_handle: i64,
    changed: Option<Rc<dyn INotebookCellExecution>>,
}

impl ICellExecutionStateChangedEvent for TestCellExecutionChange {
    fn notebook(&self) -> URI {
        self.notebook.clone()
    }

    fn cell_handle(&self) -> i64 {
        self.cell_handle
    }

    fn changed(&self) -> Option<Rc<dyn INotebookCellExecution>> {
        self.changed.clone()
    }

    fn affects_cell(&self, _cell: &URI) -> bool {
        true
    }

    fn affects_notebook(&self, _notebook: &URI) -> bool {
        true
    }
}

/// `fireExecutionChanged(notebook, cellHandle, changed?)` do `TestExecutionService`.
fn fire_execution_changed(emitter: &Emitter<ExecutionStateChange>, notebook: &URI, cell_handle: i64, changed: Option<Rc<dyn INotebookCellExecution>>) {
    emitter.fire(ExecutionStateChange::Cell(Rc::new(TestCellExecutionChange { notebook: notebook.clone(), cell_handle, changed })));
}

/// O `new class extends mock<IChatAgentService>()` de `setup`.
struct TestChatAgentService;

impl IChatAgentService for TestChatAgentService {
    fn on_did_change_agents(&self) -> Event<()> {
        Event::none()
    }

    fn get_agents(&self) -> Vec<IChatAgentData> {
        vec![IChatAgentData {
            id: "testEditorAgent".to_string(),
            name: "testEditorAgent".to_string(),
            is_default: true,
            locations: vec![ChatAgentLocation::Notebook],
            modes: vec![ChatModeKind::Ask],
        }]
    }
}

/// `ITestMarkerService`: o `markerService` de `setup`.
struct TestMarkerService {
    markers: RefCell<HashMap<String, Vec<MarkerData>>>,
    on_markers_updated: Rc<Emitter<()>>,
}

impl TestMarkerService {
    fn key(resource: &URI) -> String {
        resource.to_string(false).to_string_lossy()
    }

    /// `markers.get(resource)?.length`.
    fn marker_count(&self, resource: &URI) -> Option<usize> {
        self.markers.borrow().get(&Self::key(resource)).map(Vec::len)
    }
}

impl IMarkerService for TestMarkerService {
    fn get_statistics(&self) -> MarkerStatistics {
        MarkerStatistics::default()
    }

    fn is_source_truncated(&self, _resource: &URI) -> bool {
        false
    }

    fn change_one(&self, _owner: &str, resource: &URI, markers: Vec<MarkerData>, _options: Option<MarkerChangeOptions>) {
        self.markers.borrow_mut().insert(Self::key(resource), markers);
        self.on_markers_updated.fire(());
    }

    fn change_all(&self, _owner: &str, _data: Vec<ResourceMarker>) {}

    fn remove(&self, _owner: &str, _resources: &[URI]) {}

    fn read(&self, _filter: Option<&MarkerReadOptions>) -> Vec<Marker> {
        Vec::new()
    }

    fn install_resource_filter(&self, _resource: &URI, _reason: &str) -> DisposableRef {
        lifecycle::none()
    }

    fn on_marker_changed(&self) -> Event<Vec<URI>> {
        Event::none()
    }
}

/// `new Promise(resolve => Event.once(markerService.onMarkersUpdated)(resolve))`.
struct MarkersUpdatedWatcher {
    fired: Rc<Cell<bool>>,
    listener: DisposableRef,
}

impl MarkersUpdatedWatcher {
    fn new(marker_service: &TestMarkerService) -> Self {
        let fired = Rc::new(Cell::new(false));
        let slot = fired.clone();
        let listener = marker_service.on_markers_updated.event().once().listen(move |_| slot.set(true));
        Self { fired, listener }
    }

    /// `await promise`: avança os timers simulados um a um só até o evento chegar.
    fn wait(&self) {
        while !self.fired.get() && runtime::run_next_timer(true) {}
        self.listener.dispose();
        assert!(self.fired.get(), "onMarkersUpdated nunca disparou");
    }
}

/// O que o `setup` do original guarda em variáveis da suíte (`testExecutionService`, `markerService`).
struct Fixture {
    execution_emitter: Rc<Emitter<ExecutionStateChange>>,
    marker_service: Rc<TestMarkerService>,
}

/// `setup()`: o `setupInstantiationService(disposables)` com os `instantiationService.stub(...)` e a configuração ligada.
fn setup(disposables: &DisposableStore) -> (NotebookTestServices, Fixture) {
    let base = setup_notebook_services(disposables);

    let execution_emitter = disposables.add_rc(Emitter::<ExecutionStateChange>::new());
    let execution_service: Rc<dyn INotebookExecutionStateService> =
        Rc::new(TestNotebookExecutionStateService::with_did_change_execution(execution_emitter.event()));

    let chat_agent_service: Rc<dyn IChatAgentService> = Rc::new(TestChatAgentService);

    let marker_service = Rc::new(TestMarkerService { markers: RefCell::new(HashMap::new()), on_markers_updated: disposables.add_rc(Emitter::<()>::new()) });
    let marker_service_ref: Rc<dyn IMarkerService> = marker_service.clone();

    let test_configuration_service = TestConfigurationService::new(None);
    disposables.add_rc(test_configuration_service.on_did_change_configuration_emitter.clone());
    drop(test_configuration_service.set_user_configuration(NotebookSetting::CELL_FAILURE_DIAGNOSTICS, Dynamic::Bool(true), None));
    let configuration_service: Rc<dyn IConfigurationService> = test_configuration_service;

    let instantiation_service = disposables.add_rc(InstantiationService::new(
        ServiceCollection::new()
            .with(I_LANGUAGE_SERVICE, base.language_service())
            .with(I_NOTEBOOK_EXECUTION_STATE_SERVICE, execution_service)
            .with(I_NOTEBOOK_LOGGING_SERVICE, base.notebook_logging_service())
            .with(I_LANGUAGE_DETECTION_SERVICE, base.language_detection_service())
            .with(I_CHAT_AGENT_SERVICE, chat_agent_service)
            .with(I_MARKER_SERVICE, marker_service_ref),
    ));

    let services = NotebookTestServices {
        instantiation_service,
        undo_redo_service: base.undo_redo_service.clone(),
        model_service: base.model_service.clone(),
        configuration_service,
    };
    (services, Fixture { execution_emitter, marker_service })
}

/// O `editor` (`INotebookEditor`) do caso, sobre o editor de teste.
struct DiagnosticsEditor {
    editor: Rc<ActiveTestNotebookEditorDelegate>,
    view_model: Rc<NotebookViewModel>,
}

impl ICellDiagnosticsEditor for DiagnosticsEditor {
    fn text_model_uri(&self) -> Option<URI> {
        Some(self.editor.text_model.uri())
    }

    fn get_cell_by_handle(&self, handle: i64) -> Option<CellViewModel> {
        self.view_model.get_cell_by_handle(handle)
    }
}

/// `withTestNotebook(cells, callback, instantiationService)` com o `setup` do caso; o corpo recebe o `editor`, o `viewModel`, o
/// `CellDiagnostics` criado (`disposables.add(instantiationService.createInstance(CellDiagnostics, editor))`) e o [`Fixture`].
fn with_diagnostics_notebook(cell_sources: &[&'static str], body: impl FnOnce(&Rc<ActiveTestNotebookEditorDelegate>, &Rc<NotebookViewModel>, Fixture)) {
    let fixture_slot: Rc<RefCell<Option<Fixture>>> = Rc::new(RefCell::new(None));
    let setup_slot = fixture_slot.clone();
    with_test_notebook_services(
        move |disposables| {
            let (services, fixture) = setup(disposables);
            *setup_slot.borrow_mut() = Some(fixture);
            services
        },
        cell_sources.iter().map(|source| (*source, "python", CellKind::Code, Vec::new(), Some(empty_metadata()))).collect(),
        |editor, view_model, disposables, services| {
            let fixture = fixture_slot.borrow_mut().take().expect("o setup rodou");
            let (execution_service, marker_service, chat_agent_service) = services.instantiation_service.invoke_function(|accessor| {
                (accessor.get(I_NOTEBOOK_EXECUTION_STATE_SERVICE), accessor.get(I_MARKER_SERVICE), accessor.get(I_CHAT_AGENT_SERVICE))
            });
            let diagnostics = CellDiagnostics::new(
                Rc::new(DiagnosticsEditor { editor: editor.clone(), view_model: view_model.clone() }),
                execution_service,
                marker_service,
                chat_agent_service,
                services.configuration_service.clone(),
            );
            disposables.add_rc(diagnostics);
            body(editor, view_model, fixture);
        },
    );
}

/// `cell.model.internalMetadata.lastRunSuccess = false; cell.model.internalMetadata.error = {...}`.
fn fail_cell(cell: &CellViewModel, message: &str, stack: &str, error_uri: &URI) {
    let model = cell.model();
    let mut metadata = model.internal_metadata();
    metadata.last_run_success = Some(false);
    metadata.error = Some(ICellExecutionError {
        name: "error".to_string(),
        message: message.to_string(),
        stack: Some(CellExecutionErrorStack::Text(stack.to_string())),
        uri: error_uri.to_json().components,
        location: Some(Range::new(1, 1, 1, 5)),
    });
    model.set_internal_metadata(metadata);
}

/// `cell?.executionErrorDiagnostic.get()?.message`.
fn diagnostic_message(cell: &CellViewModel) -> Option<String> {
    cell.as_code().expect("célula de código").execution_error_diagnostic().get().map(|error| error.message.clone())
}

#[test]
fn diagnostic_is_added_for_cell_execution_failure() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    with_diagnostics_notebook(&["print(x)"], |editor, view_model, fixture| {
        let cell = view_model.cell_at(0).expect("célula 0");

        fail_cell(&cell, "something bad happened", "line 1 : print(x)", &cell.uri());
        fire_execution_changed(&fixture.execution_emitter, &editor.text_model.uri(), cell.handle(), None);
        MarkersUpdatedWatcher::new(&fixture.marker_service).wait();

        assert_eq!(diagnostic_message(&cell), Some("something bad happened".to_string()));
        assert_eq!(fixture.marker_service.marker_count(&cell.uri()), Some(1));
    });
}

#[test]
fn diagnostics_are_cleared_only_for_cell_with_new_execution() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    with_diagnostics_notebook(&["print(x)", "print(y)"], |editor, view_model, fixture| {
        let cell = view_model.cell_at(0).expect("célula 0");
        let cell2 = view_model.cell_at(1).expect("célula 1");

        fail_cell(&cell, "something bad happened", "line 1 : print(x)", &cell.uri());
        fail_cell(&cell2, "another bad thing happened", "line 1 : print(y)", &cell.uri());
        fire_execution_changed(&fixture.execution_emitter, &editor.text_model.uri(), cell.handle(), None);
        fire_execution_changed(&fixture.execution_emitter, &editor.text_model.uri(), cell2.handle(), None);

        MarkersUpdatedWatcher::new(&fixture.marker_service).wait();

        let clear_markers = MarkersUpdatedWatcher::new(&fixture.marker_service);
        // on NotebookCellExecution value will make it look like its currently running
        fire_execution_changed(&fixture.execution_emitter, &editor.text_model.uri(), cell.handle(), Some(Rc::new(EmptyCellExecution)));

        clear_markers.wait();

        assert_eq!(diagnostic_message(&cell), None);
        assert_eq!(diagnostic_message(&cell2), Some("another bad thing happened".to_string()), "cell that was not executed should still have an error");
        assert_eq!(fixture.marker_service.marker_count(&cell.uri()), Some(0));
        assert_eq!(fixture.marker_service.marker_count(&cell2.uri()), Some(1));
    });
}
