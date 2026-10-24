//! Porte parcial de `src/vs/workbench/contrib/notebook/test/browser/testNotebookEditor.ts`: o que `notebookCommon.test.ts`
//! e `notebookTextModel.test.ts` usam dele, que é `setupInstantiationService`, `TestCell`, `valueBytesFromString`,
//! `withTestNotebook` (com `createTestNotebookEditor`), `TestNotebookExecutionStateService` e o
//! `NullNotebookLoggingService`. O resto do arquivo (o editor de teste completo, `NotebookEditorTestModel`) depende do editor
//! de notebook e fica para as fatias dele; `withTestNotebookDiffModel` e `createNotebookCellList` estão aqui.
//!
//! Divergências:
//! - `setupInstantiationService` registra o `ILanguageService` (o `LanguageService`), o `INotebookLoggingService`, o
//!   `INotebookExecutionStateService` e o `ILanguageDetectionService`; o `IUndoRedoService` e o `IModelService` não
//!   têm trait com identificador no porte, então ficam em [`NotebookTestServices`] (o `UndoRedoService` e o
//!   `ModelService` concretos, que o `NotebookTextModel` recebe). Os outros serviços que o original registra
//!   (configuração, tema, contexto, lista, layout, log, área de transferência, armazenamento, confiança, teclado, barra
//!   de status, outline...) não são lidos por nenhum caso portado. O `TestInstantiationService` é um
//!   `InstantiationService` sobre um `ServiceCollection`. O `spy(IUndoRedoService, 'pushElement')` do `suiteSetup` de
//!   `notebookTextModel.test.ts` não é lido por nenhum caso e não é portado.
//! - `TestCell extends NotebookCellTextModel` é composição com `Deref` para o [`NotebookCellTextModel`]; a igualdade
//!   é por identidade (o `===` do original, usado por `diff` e pelos `deepStrictEqual` de células). Como o original usa
//!   uma `TestCell` onde se espera um `ICellDto2` (a forma estrutural é a mesma), [`TestCell::cell_dto`] monta o DTO com o
//!   que a `NotebookCellTextModel` expõe (`source` é o texto do `TestCell`).
//! - `public source: string` do `TestCell` é o campo `source` (texto); o `source` do `ICellDto2` é o mesmo texto.
//! - `withTestNotebook` e `createTestNotebookEditor`: o `NotebookEditorTestModel` (só guarda o notebook e marca sujo) não entra
//!   (nenhum caso lê algo dele). O `NotebookOptions`, o `NotebookEventDispatcher`, o `ViewContext`, o `NotebookViewModel` real,
//!   a lista de células lógica (`NotebookCellList`, `createNotebookCellList`) e o `ListViewInfoAccessor` são os do porte, e o
//!   `mock<IActiveNotebookEditorDelegate>` é o [`ActiveTestNotebookEditorDelegate`], que implementa o trait
//!   `IActiveNotebookEditor` com os membros que os casos leem (`getViewIndexByModelIndex`, `setHiddenAreas`, `getActiveCell`,
//!   `focusNotebookCell`...). `setupInstantiationService` guarda a configuração no [`NotebookTestServices`]; os serviços
//!   que o original registra e nenhum caso lê (tema, contexto, lista, layout, log, área de transferência, armazenamento,
//!   confiança, teclado, barra de status, outline, `ICodeEditorService`) não existem no porte. O ramo
//!   `MultiModelEditStackElement` do desfazer não existe no porte do editor. O `ITextModelService` não existe aqui, então o
//!   `_withElement` do `NotebookViewModel` só chama o callback.
//! - `withTestNotebookDiffModel` é [`with_test_notebook_diff_model`]: os dois `mock<IResolvedNotebookEditorModel>` (só `notebook`
//!   e `resource` respondem) e o `mock<INotebookDiffEditorModel>` são o [`TestNotebookDiffModel`] com os dois
//!   `NotebookDiffEditorModelSide`; o callback é assíncrono (devolve um `Future`) e recebe o `DisposableStore` e os serviços
//!   (`setupInstantiationService`, que no original é o `accessor`: o `INotebookService` que os casos liam dele não existe no
//!   porte, e o `NotebookDiffViewModel` não o recebe). O `try`/`finally` do original é a limpeza depois do `await` (um
//!   pânico do callback reprova o caso de qualquer jeito).
//! - `runWithFakedTimers({ useFakeTimers: true }, ...)` é `runtime::with_faked_timers`; o `try`/`finally` do original é
//!   um `catch_unwind` com a limpeza e o novo lançamento do pânico. Os callbacks `async` do original são síncronos (o
//!   `UndoRedoService` do porte é síncrono).
//! - `TestNotebookExecutionStateService` guarda as execuções num `HashMap` por `URI.toString()` (o `ResourceMap`), e os
//!   eventos que ele cria com `new Emitter().event` são `Event::none()` (nunca disparam).

use std::cell::RefCell;
use std::collections::HashMap;
use std::fmt;
use std::future::Future;
use std::ops::Deref;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::rc::Rc;

use je_base::JsString;
use je_base::buffer::VSBuffer;
use je_base::dynamic::{Dynamic, ObjectRef};
use je_base::errors;
use je_base::event::Event;
use je_base::lifecycle::{DisposableStore, IDisposable};
use je_base::mime::mimes;
use je_base::promise::Promise;
use je_base::runtime;
use je_base::uri::URI;
use je_base::uuid::generate_uuid;
use je_editor::language_service::{ILanguageService, I_LANGUAGE_SERVICE, LanguageService};
use je_editor::model::DefaultEndOfLine;
use je_editor::model_service::ModelService;
use je_editor::undo_redo_service::{IUndoRedoService, UndoRedoService};
use je_instantiation::{InstantiationService, ServiceCollection};
use je_platform::configuration::service::IConfigurationService;
use je_platform::configuration::test_configuration_service::TestConfigurationService;
use je_workbench::contrib::notebook::browser::controller::IActiveNotebookEditor;
use je_workbench::contrib::notebook::browser::notebook_options::NotebookOptions;
use je_workbench::contrib::notebook::browser::view::notebook_cell_list::{ListViewInfoAccessor, NotebookCellList};
use je_workbench::contrib::notebook::browser::view_model::cell_view_model_types::CellFocusMode;
use je_workbench::contrib::notebook::browser::view_model::event_dispatcher::NotebookEventDispatcher;
use je_workbench::contrib::notebook::browser::view_model::notebook_view_model_impl::{CellViewModel, NotebookViewModel, NotebookViewModelOptions};
use je_workbench::contrib::notebook::browser::view_model::view_context::{IBaseCellEditorOptions, ViewContext};
use je_workbench::contrib::notebook::common::model::notebook_cell_text_model::NotebookCellTextModel;
use je_workbench::contrib::notebook::common::model::notebook_text_model::NotebookTextModel;
use je_workbench::contrib::notebook::common::notebook_common::{
    CellKind, CellUri, ICell, ICellDto2, INotebookDiffEditorModel, IOutputDto, ISelectionIndexState, ISelectionState, NotebookCellExecutionState, NotebookCellMetadata,
    NotebookDiffEditorModelSide, TransientOptions,
};
use je_workbench::contrib::notebook::common::notebook_range::ICellRange;
use je_workbench::contrib::notebook::common::notebook_execution_state_service::{
    ExecutionStateChange, ICellExecuteUpdate, ICellExecutionComplete, INotebookCellExecution, INotebookExecution, INotebookExecutionStateService,
    INotebookFailStateChangedEvent, I_NOTEBOOK_EXECUTION_STATE_SERVICE,
};
use je_workbench::contrib::notebook::common::notebook_logging_service::{INotebookLoggingService, I_NOTEBOOK_LOGGING_SERVICE};
use je_workbench::services::language_detection::language_detection_worker_service::{ILanguageDetectionService, I_LANGUAGE_DETECTION_SERVICE};
use je_workbench::test::common::workbench_test_services::TestTextResourcePropertiesService;

pub struct NullNotebookLoggingService;

impl INotebookLoggingService for NullNotebookLoggingService {
    fn info(&self, _category: &str, _output: &str) {}
    fn warn(&self, _category: &str, _output: &str) {}
    fn error(&self, _category: &str, _output: &str) {}
    fn debug(&self, _category: &str, _output: &str) {}
    fn trace(&self, _category: &str, _output: &str) {}
}

pub struct TestCell {
    pub view_type: String,
    pub source: String,
    base: Rc<NotebookCellTextModel>,
}

impl TestCell {
    pub fn new(
        view_type: &str,
        handle: i64,
        source: &str,
        language: &str,
        cell_kind: CellKind,
        outputs: Vec<IOutputDto>,
        language_service: Rc<dyn ILanguageService>,
    ) -> Self {
        let base = NotebookCellTextModel::new(
            CellUri::generate(&URI::parse("test:///fake/notebook", false), handle),
            handle,
            ICellDto2 {
                source: JsString::from(source),
                language: language.to_string(),
                mime: Some(mimes::TEXT.to_string()),
                cell_kind,
                outputs,
                metadata: None,
                internal_metadata: None,
                collapse_state: None,
            },
            TransientOptions::default(),
            language_service,
            DefaultEndOfLine::Lf,
            None, // defaultCollapseConfig
            None, // languageDetectionService
            Rc::new(NullNotebookLoggingService),
        );
        Self { view_type: view_type.to_string(), source: source.to_string(), base }
    }

    /// O `TestCell` usado como `ICellDto2` (a forma estrutural do original): o `source` do `TestCell` e o resto da célula.
    pub fn cell_dto(&self) -> ICellDto2 {
        ICellDto2 {
            source: JsString::from(self.source.as_str()),
            language: self.base.language(),
            mime: self.base.mime(),
            cell_kind: self.base.cell_kind(),
            outputs: self.base.outputs().iter().map(|output| output.as_dto()).collect(),
            metadata: Some(self.base.metadata()),
            internal_metadata: Some(self.base.internal_metadata()),
            collapse_state: self.base.collapse_state,
        }
    }
}

impl Deref for TestCell {
    type Target = NotebookCellTextModel;

    fn deref(&self) -> &NotebookCellTextModel {
        &self.base
    }
}

impl IDisposable for TestCell {
    fn dispose(&self) {
        self.base.dispose();
    }
}

impl PartialEq for TestCell {
    fn eq(&self, other: &Self) -> bool {
        std::ptr::eq(self, other)
    }
}

impl fmt::Debug for TestCell {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "TestCell({}, {:?})", self.view_type, self.source)
    }
}

/// `MockLanguageDetectionService` de `setupInstantiationService`.
struct MockLanguageDetectionService;

impl ILanguageDetectionService for MockLanguageDetectionService {
    fn is_enabled_for_language(&self, _language_id: &str) -> bool {
        false
    }

    fn detect_language(&self, _resource: &URI, _supported_langs: Option<&[String]>) -> Promise<Option<String>> {
        Promise::resolved(None)
    }
}

/// `TestCellExecution`.
struct TestCellExecution {
    notebook: URI,
    cell_handle: i64,
    on_complete: Box<dyn Fn()>,
}

impl INotebookCellExecution for TestCellExecution {
    fn notebook(&self) -> URI {
        self.notebook.clone()
    }

    fn cell_handle(&self) -> i64 {
        self.cell_handle
    }

    fn state(&self) -> NotebookCellExecutionState {
        NotebookCellExecutionState::Unconfirmed
    }

    fn did_pause(&self) -> bool {
        false
    }

    fn is_paused(&self) -> bool {
        false
    }

    fn confirm(&self) {}

    fn update(&self, _updates: Vec<ICellExecuteUpdate>) {}

    fn complete(&self, _complete: ICellExecutionComplete) {
        (self.on_complete)();
    }
}

/// `TestNotebookExecutionStateService`.
#[derive(Default)]
pub struct TestNotebookExecutionStateService {
    executions: Rc<RefCell<HashMap<String, Rc<dyn INotebookCellExecution>>>>,
}

impl INotebookExecutionStateService for TestNotebookExecutionStateService {
    fn on_did_change_execution(&self) -> Event<ExecutionStateChange> {
        Event::none()
    }

    fn on_did_change_last_run_fail_state(&self) -> Event<INotebookFailStateChangedEvent> {
        Event::none()
    }

    fn force_cancel_notebook_executions(&self, _notebook_uri: &URI) {}

    fn get_cell_executions_for_notebook(&self, _notebook: &URI) -> Vec<Rc<dyn INotebookCellExecution>> {
        Vec::new()
    }

    fn get_cell_execution(&self, cell_uri: &URI) -> Option<Rc<dyn INotebookCellExecution>> {
        self.executions.borrow().get(&cell_uri.to_string(false).to_string_lossy()).cloned()
    }

    fn create_cell_execution(&self, notebook: &URI, cell_handle: i64) -> Rc<dyn INotebookCellExecution> {
        let executions = self.executions.clone();
        let key = CellUri::generate(notebook, cell_handle).to_string(false).to_string_lossy();
        let on_complete_key = key.clone();
        let exe: Rc<dyn INotebookCellExecution> = Rc::new(TestCellExecution {
            notebook: notebook.clone(),
            cell_handle,
            on_complete: Box::new(move || {
                executions.borrow_mut().remove(&on_complete_key);
            }),
        });
        self.executions.borrow_mut().insert(key, exe.clone());
        exe
    }

    fn get_cell_executions_by_handle_for_notebook(&self, _notebook: &URI) -> Option<HashMap<i64, Rc<dyn INotebookCellExecution>>> {
        None
    }

    fn get_last_failed_cell_for_notebook(&self, _notebook: &URI) -> Option<i64> {
        None
    }

    fn get_last_completed_cell_for_notebook(&self, _notebook: &URI) -> Option<i64> {
        None
    }

    fn get_execution(&self, _notebook: &URI) -> Option<Rc<dyn INotebookExecution>> {
        None
    }

    fn create_execution(&self, _notebook: &URI) -> Rc<dyn INotebookExecution> {
        errors::throw(errors::Error::new("Method not implemented."))
    }
}

/// `setupInstantiationService(disposables)`.
pub fn setup_instantiation_service(disposables: &DisposableStore) -> Rc<InstantiationService> {
    let language_service = disposables.add_rc(LanguageService::new(false));
    let language_service: Rc<dyn ILanguageService> = language_service;
    let notebook_execution_state_service: Rc<dyn INotebookExecutionStateService> = Rc::new(TestNotebookExecutionStateService::default());
    let notebook_logging_service: Rc<dyn INotebookLoggingService> = Rc::new(NullNotebookLoggingService);
    let language_detection_service: Rc<dyn ILanguageDetectionService> = Rc::new(MockLanguageDetectionService);
    disposables.add_rc(InstantiationService::new(
        ServiceCollection::new()
            .with(I_LANGUAGE_SERVICE, language_service)
            .with(I_NOTEBOOK_EXECUTION_STATE_SERVICE, notebook_execution_state_service)
            .with(I_NOTEBOOK_LOGGING_SERVICE, notebook_logging_service)
            .with(I_LANGUAGE_DETECTION_SERVICE, language_detection_service),
    ))
}

/// `instantiationService.get(ILanguageService)`.
pub fn get_language_service(instantiation_service: &InstantiationService) -> Rc<dyn ILanguageService> {
    instantiation_service.invoke_function(|accessor| accessor.get(I_LANGUAGE_SERVICE))
}

/// Os serviços que o `NotebookTextModel` recebe: os do `setupInstantiationService` e os dois que não têm trait com
/// identificador no porte (ver as divergências).
pub struct NotebookTestServices {
    pub instantiation_service: Rc<InstantiationService>,
    pub undo_redo_service: Rc<UndoRedoService>,
    pub model_service: Rc<ModelService>,
    pub configuration_service: Rc<dyn IConfigurationService>,
}

impl NotebookTestServices {
    pub fn language_service(&self) -> Rc<dyn ILanguageService> {
        get_language_service(&self.instantiation_service)
    }

    pub fn notebook_execution_state_service(&self) -> Rc<dyn INotebookExecutionStateService> {
        self.instantiation_service.invoke_function(|accessor| accessor.get(I_NOTEBOOK_EXECUTION_STATE_SERVICE))
    }

    pub fn notebook_logging_service(&self) -> Rc<dyn INotebookLoggingService> {
        self.instantiation_service.invoke_function(|accessor| accessor.get(I_NOTEBOOK_LOGGING_SERVICE))
    }

    pub fn language_detection_service(&self) -> Rc<dyn ILanguageDetectionService> {
        self.instantiation_service.invoke_function(|accessor| accessor.get(I_LANGUAGE_DETECTION_SERVICE))
    }
}

/// `setupInstantiationService(disposables)` com o `IUndoRedoService` e o `IModelService`.
pub fn setup_notebook_services(disposables: &DisposableStore) -> NotebookTestServices {
    let instantiation_service = setup_instantiation_service(disposables);
    let undo_redo_service = UndoRedoService::for_tests();
    let test_configuration_service = TestConfigurationService::new(None);
    disposables.add_rc(test_configuration_service.on_did_change_configuration_emitter.clone());
    let configuration_service: Rc<dyn IConfigurationService> = test_configuration_service;
    let model_service = disposables.add_rc(ModelService::new(
        configuration_service.clone(),
        TestTextResourcePropertiesService::new(configuration_service.clone()),
        undo_redo_service.clone(),
    ));
    NotebookTestServices { instantiation_service, undo_redo_service, model_service, configuration_service }
}

/// `valueBytesFromString(value)`.
pub fn value_bytes_from_string(value: &str) -> VSBuffer {
    VSBuffer::from_string(value)
}

/// `[source, lang, kind, output?, metadata?]`.
pub type MockNotebookCell = (&'static str, &'static str, CellKind, Vec<IOutputDto>, Option<NotebookCellMetadata>);

/// `{}`.
pub fn empty_metadata() -> NotebookCellMetadata {
    Dynamic::Object(ObjectRef::new())
}

/// `const baseCellEditorOptions = new class extends mock<IBaseCellEditorOptions>() { }`: `value` é `undefined`, então ler o
/// `wordWrap` lança (nenhum caso chega lá: o `fontInfo` das células de teste é nulo).
pub struct MockBaseCellEditorOptions;

impl IBaseCellEditorOptions for MockBaseCellEditorOptions {
    fn word_wrap(&self) -> String {
        errors::throw(errors::Error::new("Cannot read properties of undefined (reading 'wordWrap')"))
    }
}

/// `new NotebookOptions(mainWindow, false, undefined, configurationService, executionStateService, codeEditorService)`.
pub fn create_notebook_options(services: &NotebookTestServices, disposables: &DisposableStore) -> Rc<NotebookOptions> {
    disposables.add_rc(NotebookOptions::new(false, None, services.configuration_service.clone(), services.notebook_execution_state_service()))
}

/// `createNotebookCellList(instantiationService, disposables, viewContext?)`.
pub fn create_notebook_cell_list(services: &NotebookTestServices, disposables: &DisposableStore, view_context: Option<&Rc<ViewContext>>) -> Rc<NotebookCellList> {
    create_notebook_cell_list_with_configuration(services, disposables, view_context, services.configuration_service.clone())
}

/// `createNotebookCellList` com o `IConfigurationService` que o `instantiationService.stub(IConfigurationService, config)` de
/// `notebookCellList.test.ts` registra: é o que o `NotebookOptions` e o `NotebookCellAnchor` da lista leem.
pub fn create_notebook_cell_list_with_configuration(
    services: &NotebookTestServices,
    disposables: &DisposableStore,
    view_context: Option<&Rc<ViewContext>>,
    configuration_service: Rc<dyn IConfigurationService>,
) -> Rc<NotebookCellList> {
    let notebook_options = match view_context {
        Some(view_context) => view_context.notebook_options.clone(),
        None => disposables.add_rc(NotebookOptions::new(false, None, configuration_service.clone(), services.notebook_execution_state_service())),
    };
    disposables.add_rc(NotebookCellList::new(
        "NotebookCellList",
        notebook_options,
        |element| element.get_height(17.0),
        configuration_service,
        services.notebook_execution_state_service(),
    ))
}

/// `IActiveTestNotebookEditorDelegate`: o `mock<IActiveNotebookEditorDelegate>` de `_createTestNotebookEditor` com o que os
/// casos leem.
pub struct ActiveTestNotebookEditorDelegate {
    pub text_model: Rc<NotebookTextModel>,
    pub notebook_options: Rc<NotebookOptions>,
    view_model: Rc<NotebookViewModel>,
    pub cell_list: Rc<NotebookCellList>,
    pub list_view_info_accessor: ListViewInfoAccessor,
    visible_ranges: RefCell<Vec<ICellRange>>,
    id: String,
}

impl ActiveTestNotebookEditorDelegate {
    pub fn dispose(&self) {
        self.view_model.dispose();
    }

    pub fn get_id(&self) -> String {
        self.id.clone()
    }

    pub fn visible_ranges(&self) -> Vec<ICellRange> {
        self.visible_ranges.borrow().clone()
    }

    pub fn set_visible_ranges(&self, ranges: Vec<ICellRange>) {
        *self.visible_ranges.borrow_mut() = ranges;
    }

    pub fn set_focus(&self, focus: ICellRange) {
        self.view_model.update_selections_state(&ISelectionState::Index(ISelectionIndexState { focus, selections: self.view_model.get_selections() }), "model");
    }

    pub fn set_selections(&self, selections: Vec<ICellRange>) {
        self.view_model.update_selections_state(&ISelectionState::Index(ISelectionIndexState { focus: self.view_model.get_focus(), selections }), "model");
    }

    pub fn set_hidden_areas(&self, ranges: &[ICellRange]) -> bool {
        self.cell_list.set_hidden_areas(ranges, true)
    }

    pub fn get_active_cell(&self) -> Option<CellViewModel> {
        self.cell_list.get_focused_elements().first().cloned()
    }
}

impl IActiveNotebookEditor for ActiveTestNotebookEditorDelegate {
    fn text_model(&self) -> Rc<NotebookTextModel> {
        self.text_model.clone()
    }

    fn is_read_only(&self) -> bool {
        false
    }

    fn get_selections(&self) -> Vec<ICellRange> {
        self.view_model.get_selections()
    }

    fn get_focus(&self) -> ICellRange {
        self.view_model.get_focus()
    }

    fn get_length(&self) -> usize {
        self.view_model.length()
    }

    fn cell_at(&self, index: i64) -> Option<CellViewModel> {
        self.view_model.cell_at(index)
    }

    fn get_cell_index(&self, cell: &CellViewModel) -> i64 {
        self.view_model.get_cell_index(cell)
    }

    fn get_cells_in_range(&self, range: Option<ICellRange>) -> Vec<CellViewModel> {
        self.view_model.get_cells_in_range(range)
    }

    fn get_view_model(&self) -> Rc<NotebookViewModel> {
        self.view_model.clone()
    }

    fn get_view_index_by_model_index(&self, index: i64) -> i64 {
        match self.view_model.cell_at(index) {
            Some(cell) => self.list_view_info_accessor.get_view_index(&cell),
            None => -1,
        }
    }

    fn get_cell_range_from_view_range(&self, start_index: i64, end_index: i64) -> Option<ICellRange> {
        self.list_view_info_accessor.get_cell_range_from_view_range(start_index, end_index)
    }

    fn reveal_cell_range_in_view(&self, _range: ICellRange) {}

    fn focus_notebook_cell(&self, cell: &CellViewModel, focus_item: &str) {
        cell.base().set_focus_mode(match focus_item {
            "editor" => CellFocusMode::Editor,
            "output" => CellFocusMode::Output,
            _ => CellFocusMode::Container,
        });
    }

    fn active_kernel_supported_languages(&self) -> Option<Vec<String>> {
        None
    }
}

pub struct TestNotebookEditor {
    pub editor: Rc<ActiveTestNotebookEditorDelegate>,
    pub view_model: Rc<NotebookViewModel>,
}

/// `createTestNotebookEditor(instantiationService, disposables, cells)`.
pub fn create_test_notebook_editor(services: &NotebookTestServices, disposables: &DisposableStore, cells: Vec<MockNotebookCell>) -> TestNotebookEditor {
    let view_type = "notebook";
    let cell_dtos: Vec<ICellDto2> = cells
        .into_iter()
        .map(|cell| ICellDto2 {
            source: JsString::from(cell.0),
            mime: None,
            language: cell.1.to_string(),
            cell_kind: cell.2,
            outputs: cell.3,
            metadata: cell.4,
            internal_metadata: None,
            collapse_state: None,
        })
        .collect();
    let notebook = disposables.add_rc(NotebookTextModel::new(
        view_type,
        URI::parse("test://test", false),
        cell_dtos,
        empty_metadata(),
        TransientOptions::default(),
        services.undo_redo_service.clone(),
        services.model_service.clone(),
        services.language_service(),
        services.language_detection_service(),
        services.notebook_execution_state_service(),
        services.notebook_logging_service(),
    ));

    let notebook_options = create_notebook_options(services, disposables);
    let base_cell_editor_options: Rc<dyn IBaseCellEditorOptions> = Rc::new(MockBaseCellEditorOptions);
    let view_context = ViewContext::new(notebook_options.clone(), disposables.add_rc(NotebookEventDispatcher::new()), move |_language| base_cell_editor_options.clone());
    let undo_redo_service: Rc<dyn IUndoRedoService> = services.undo_redo_service.clone();
    let view_model = disposables.add_rc(NotebookViewModel::new(
        view_type,
        notebook.clone(),
        view_context.clone(),
        None,
        NotebookViewModelOptions { is_read_only: false },
        services.configuration_service.clone(),
        undo_redo_service,
        services.notebook_execution_state_service(),
    ));

    let cell_list = create_notebook_cell_list(services, disposables, Some(&view_context));
    cell_list.attach_view_model(view_model.clone());
    let list_view_info_accessor = ListViewInfoAccessor::new(cell_list.clone());

    let editor = Rc::new(ActiveTestNotebookEditorDelegate {
        text_model: notebook,
        notebook_options,
        view_model: view_model.clone(),
        cell_list,
        list_view_info_accessor,
        visible_ranges: RefCell::new(vec![ICellRange { start: 0, end: 100 }]),
        id: generate_uuid(),
    });

    TestNotebookEditor { editor, view_model }
}

/// `withTestNotebook(cells, callback)`.
pub fn with_test_notebook<R>(
    cells: Vec<MockNotebookCell>,
    callback: impl FnOnce(&Rc<ActiveTestNotebookEditorDelegate>, &Rc<NotebookViewModel>, &Rc<DisposableStore>, &NotebookTestServices) -> R,
) -> R {
    let disposables = DisposableStore::new();
    let services = setup_notebook_services(&disposables);
    let notebook_editor = create_test_notebook_editor(&services, &disposables, cells);

    let mut outcome = None;
    runtime::with_faked_timers(|| {
        let result = catch_unwind(AssertUnwindSafe(|| callback(&notebook_editor.editor, &notebook_editor.view_model, &disposables, &services)));
        notebook_editor.editor.dispose();
        notebook_editor.view_model.dispose();
        notebook_editor.editor.text_model.dispose();
        disposables.dispose();
        outcome = Some(result);
    });

    match outcome.expect("o corpo rodou") {
        Ok(result) => result,
        // O `with_faked_timers` captura o pânico em silêncio; o lançamento de novo mostra a mensagem do caso que falhou.
        Err(payload) => panic!("{}", errors::from_panic(payload)),
    }
}

/// O `mock<INotebookDiffEditorModel>` de `withTestNotebookDiffModel`, com os dois `mock<IResolvedNotebookEditorModel>`.
pub struct TestNotebookDiffModel {
    original: NotebookDiffEditorModelSide,
    modified: NotebookDiffEditorModelSide,
}

impl INotebookDiffEditorModel for TestNotebookDiffModel {
    fn original(&self) -> NotebookDiffEditorModelSide {
        self.original.clone()
    }

    fn modified(&self) -> NotebookDiffEditorModelSide {
        self.modified.clone()
    }
}

impl IDisposable for TestNotebookDiffModel {
    fn dispose(&self) {}
}

fn diff_model_side(notebook: &TestNotebookEditor) -> NotebookDiffEditorModelSide {
    let text_model = notebook.editor.text_model.clone();
    NotebookDiffEditorModelSide { resource: text_model.uri(), view_type: text_model.view_type(), notebook: text_model }
}

/// `withTestNotebookDiffModel(originalCells, modifiedCells, callback)`.
pub async fn with_test_notebook_diff_model<R, Fut>(
    original_cells: Vec<MockNotebookCell>,
    modified_cells: Vec<MockNotebookCell>,
    callback: impl FnOnce(Rc<TestNotebookDiffModel>, Rc<DisposableStore>, Rc<NotebookTestServices>) -> Fut,
) -> R
where
    Fut: Future<Output = R>,
{
    let disposables = DisposableStore::new();
    let services = Rc::new(setup_notebook_services(&disposables));
    let original_notebook = create_test_notebook_editor(&services, &disposables, original_cells);
    let modified_notebook = create_test_notebook_editor(&services, &disposables, modified_cells);
    let model = Rc::new(TestNotebookDiffModel { original: diff_model_side(&original_notebook), modified: diff_model_side(&modified_notebook) });

    let result = callback(model, disposables.clone(), services).await;

    for notebook in [&original_notebook, &modified_notebook] {
        notebook.editor.dispose();
        notebook.editor.text_model.dispose();
        notebook.view_model.dispose();
    }
    disposables.dispose();
    result
}
