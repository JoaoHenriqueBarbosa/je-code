//! Porte parcial de `src/vs/workbench/contrib/notebook/test/browser/testNotebookEditor.ts`: o que `notebookCommon.test.ts`
//! e `notebookTextModel.test.ts` usam dele, que é `setupInstantiationService`, `TestCell`, `valueBytesFromString`,
//! `withTestNotebook` (com `createTestNotebookEditor`), `TestNotebookExecutionStateService` e o
//! `NullNotebookLoggingService`. O resto do arquivo (o editor de teste completo, `NotebookEditorTestModel`,
//! `withTestNotebookDiffModel`, `createNotebookCellList`) depende do editor de notebook e fica para as fatias dele.
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
//! - `withTestNotebook` e `createTestNotebookEditor`: o `NotebookEditorTestModel` (só guarda o notebook e marca sujo), o
//!   `NotebookOptions`, o `NotebookEventDispatcher`, a lista de células (`NotebookCellList`) e o editor com
//!   `mock<IActiveNotebookEditorDelegate>` não entram: nenhum caso lê algo deles. O que os casos leem é
//!   `editor.textModel` ([`ActiveTestNotebookEditorDelegate::text_model`]) e o `NotebookViewModel`, de que só
//!   `notebookDocument`, `undo` e `redo` são usados; [`NotebookViewModel`] tem esses três. Sem células de view model e sem
//!   `ITextModelService`, o `_withElement` do original (que abre referências aos modelos das células de view) só chama o
//!   callback; o ramo `MultiModelEditStackElement` não existe no porte do editor.
//! - `runWithFakedTimers({ useFakeTimers: true }, ...)` é `runtime::with_faked_timers`; o `try`/`finally` do original é
//!   um `catch_unwind` com a limpeza e o novo lançamento do pânico. Os callbacks `async` do original são síncronos (o
//!   `UndoRedoService` do porte é síncrono).
//! - `TestNotebookExecutionStateService` guarda as execuções num `HashMap` por `URI.toString()` (o `ResourceMap`), e os
//!   eventos que ele cria com `new Emitter().event` são `Event::none()` (nunca disparam).

use std::cell::RefCell;
use std::collections::HashMap;
use std::fmt;
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
use je_editor::edit_stack::SingleModelEditStackElement;
use je_editor::language_service::{ILanguageService, I_LANGUAGE_SERVICE, LanguageService};
use je_editor::model::DefaultEndOfLine;
use je_editor::model_service::ModelService;
use je_editor::undo_redo::UndoRedoElement;
use je_editor::undo_redo_service::{IUndoRedoService, UndoRedoService, UndoRedoTarget};
use je_instantiation::{InstantiationService, ServiceCollection};
use je_platform::configuration::service::IConfigurationService;
use je_platform::configuration::test_configuration_service::TestConfigurationService;
use je_workbench::contrib::notebook::common::model::notebook_cell_text_model::NotebookCellTextModel;
use je_workbench::contrib::notebook::common::model::notebook_text_model::NotebookTextModel;
use je_workbench::contrib::notebook::common::notebook_common::{
    CellKind, CellUri, ICell, ICellDto2, IOutputDto, NotebookCellExecutionState, NotebookCellMetadata, TransientOptions,
};
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
        TestTextResourcePropertiesService::new(configuration_service),
        undo_redo_service.clone(),
    ));
    NotebookTestServices { instantiation_service, undo_redo_service, model_service }
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

/// O `NotebookViewModel` do original, só com o que os casos usam: `notebookDocument`, `undo` e `redo` (ver as
/// divergências).
pub struct NotebookViewModel {
    notebook: Rc<NotebookTextModel>,
    undo_service: Rc<UndoRedoService>,
}

impl NotebookViewModel {
    pub fn notebook_document(&self) -> Rc<NotebookTextModel> {
        self.notebook.clone()
    }

    pub fn uri(&self) -> URI {
        self.notebook.uri()
    }

    pub fn undo(&self) -> Vec<URI> {
        let edit_stack = self.undo_service.get_elements(&self.uri());
        let element = edit_stack.past.last().cloned();

        if let Some(element) = element
            && let Some(element) = element.as_resource()
            && element.as_any().is::<SingleModelEditStackElement>()
        {
            self.undo_service.undo(UndoRedoTarget::Resource(&self.uri()));

            return vec![element.resource()];
        }

        self.undo_service.undo(UndoRedoTarget::Resource(&self.uri()));
        Vec::new()
    }

    pub fn redo(&self) -> Vec<URI> {
        let edit_stack = self.undo_service.get_elements(&self.uri());
        let element = edit_stack.future.first().cloned();

        if let Some(element) = element
            && let Some(element) = element.as_resource()
            && element.as_any().is::<SingleModelEditStackElement>()
        {
            self.undo_service.redo(UndoRedoTarget::Resource(&self.uri()));

            return vec![element.resource()];
        }

        self.undo_service.redo(UndoRedoTarget::Resource(&self.uri()));

        Vec::new()
    }

    pub fn dispose(&self) {}
}

/// `IActiveTestNotebookEditorDelegate`, só com o `textModel`.
pub struct ActiveTestNotebookEditorDelegate {
    pub text_model: Rc<NotebookTextModel>,
    view_model: Rc<NotebookViewModel>,
}

impl ActiveTestNotebookEditorDelegate {
    pub fn dispose(&self) {
        self.view_model.dispose();
    }
}

pub struct TestNotebookEditor {
    pub editor: ActiveTestNotebookEditorDelegate,
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

    let view_model = Rc::new(NotebookViewModel { notebook: notebook.clone(), undo_service: services.undo_redo_service.clone() });
    let editor = ActiveTestNotebookEditorDelegate { text_model: notebook, view_model: view_model.clone() };

    TestNotebookEditor { editor, view_model }
}

/// `withTestNotebook(cells, callback)`.
pub fn with_test_notebook<R>(
    cells: Vec<MockNotebookCell>,
    callback: impl FnOnce(&ActiveTestNotebookEditorDelegate, &NotebookViewModel, &Rc<DisposableStore>, &NotebookTestServices) -> R,
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
