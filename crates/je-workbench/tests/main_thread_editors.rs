//! Porte de `src/vs/workbench/api/test/browser/mainThreadEditors.test.ts` (suíte `MainThreadEditors`), caso a caso e na
//! mesma ordem. Entram os quatro casos de `$tryApplyWorkspaceEdit` (`MainThreadBulkEdits`) e os dois de `$tryApplyEdits`
//! (`applyWorkspaceEdit can control undo/redo stack 1` e `2`).
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` vira o guarda criado no começo de cada caso (ele faz também o papel do
//!   `disposables`); o `setup` é [`Fixture::new`].
//! - **Serviços.** O `InstantiationService` com o `ServiceCollection` do original é o `TestInstantiationService`, onde
//!   estão os serviços que o `BulkEditService` e o `MainThreadBulkEdits` leem (`IBulkEditService` é o `BulkEditService` real,
//!   criado por `createInstance` depois dos outros). O `MainThreadTextEditors` e o `MainThreadDocuments` recebem os serviços
//!   direto nos construtores, como nas outras suítes de `MainThreadXxx`. O `IModelService` é o `ModelService` do porte (sem
//!   o `IInstantiationService`) e divide o `UndoRedoService` com o `IUndoRedoService` do `BulkEditService` (no original os
//!   dois são o mesmo `undoRedoService`). O `ILabelService`, o `IWorkspaceContextService`, o `ITreeSitterLibraryService`, o
//!   `ILanguageService` e o `IPaneCompositePartService` que o original registra só serviam ao `ModelService` ou ao
//!   `MainThreadDocuments` do original; o `IEditorWorkerService` é o dublê vazio ([`EditorWorkerService`]).
//! - **`TestWorkingCopyService`, `TestLifecycleService`, `TestEditorService` e `TestDialogService`.** São os do porte
//!   (`WorkingCopyService`, `TestLifecycleService`, `TestEditorService`) e o [`TestDialogService`] local, que não confirma.
//! - **`ITextFileService`.** É o [`RecordingTextFileService`] (ver `support/main_thread_support.rs`), que registra os
//!   `createdResources` e devolve `undefined` em `getEncodedReadable`.
//! - **`IWorkingCopyFileService`.** É o [`RecordingWorkingCopyFileService`], com os quatro registros do original
//!   (`movedResources`, `copiedResources`, `createdResources` e `deletedResources`); os mapas por identidade de `URI` são
//!   listas comparadas por valor.
//! - **`ITextModelService`** do original devolve uma referência imortal com o `textEditorModel` do `ModelService`; é o
//!   [`ModelServiceTextModelService`], com o [`ResolvedTextEditorModel`] (que não é somente leitura).
//! - **`SingleProxyRPCProtocol(null)`** é o [`FixedProxyContext`] (ver `support/main_thread_support.rs`); `createTestCodeEditor(model)`
//!   usa o editor de teste padrão (foco no texto).
//! - **`editorLocator`** é o [`EditorLocator`]: `getEditor(id)` devolve o `testEditor` quando o id é `testEditorId`.
//! - `await model.undo()` é `model.undo()`: o `UndoRedoService` do porte é síncrono.
//! - Os `async` do original são [`run_with_faked_timers`] e `.await` das promessas do porte; o `return ...then(...)` dos
//!   casos é o `.await` dentro do corpo.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
#[allow(dead_code, unused_imports)]
#[path = "support/main_thread_support.rs"]
mod main_thread_support;

/// O suporte de editor do `je-editor/tests/support` que o `createTestCodeEditor` precisa, no mesmo módulo pai (os arquivos
/// se referem uns aos outros por `super::`).
#[allow(dead_code, unused_imports)]
#[path = "../../je-editor/tests/support"]
mod editor_support {
    pub mod editor_test_services;
    pub mod test_clipboard_service;
    pub mod test_code_editor;
    pub mod test_code_editor_service;
    pub mod test_color_theme_service;
    pub mod test_configuration;
    pub mod test_instantiation_service;
    pub mod test_language_configuration_service;
    pub mod test_notification_service;
    pub mod test_text_model;
    pub mod test_theme_service;
}

use std::any::Any;
use std::cell::RefCell;
use std::rc::{Rc, Weak};

use common::{ensure_no_disposables_are_leaked_in_test_suite, run_with_faked_timers, LeakGuard};
use editor_support::test_clipboard_service::TestClipboardService;
use editor_support::test_code_editor::{create_test_code_editor, TestCodeEditorInstantiationOptions};
use editor_support::test_code_editor_service::TestCodeEditorService;
use editor_support::test_color_theme_service::TestThemeService;
use editor_support::test_instantiation_service::TestInstantiationService;
use je_base::errors::{self, Error};
use je_base::event::Event;
use je_base::js_string::JsString;
use je_base::lifecycle::{IDisposable, Reference};
use je_base::cancellation::CancellationToken;
use je_base::promise::Promise;
use je_base::uri::URI;
use je_editor::code_action::languages::{IWorkspaceTextEdit, TextEdit};
use je_editor::code_action::platform::{I_BULK_EDIT_SERVICE, IBulkEditService};
use je_editor::edit_operation::{EditOperation, SingleEditOperation};
use je_editor::editor_worker::{I_EDITOR_WORKER_SERVICE, IEditorWorkerService, WordRanges};
use je_editor::model::{EndOfLineSequence, TextSnapshot};
use je_editor::model_service::ModelService;
use je_editor::position::Position;
use je_editor::range::Range;
use je_editor::services::abstract_code_editor_service::ICodeEditorRegistry;
use je_editor::services_model::{I_MODEL_SERVICE, IModelService};
use je_editor::text_model::TextModel;
use je_editor::undo_redo::{I_UNDO_REDO_SERVICE, IUndoRedoService};
use je_editor::undo_redo_service::UndoRedoService;
use je_instantiation::Injectable;
use je_platform::configuration::service::{I_CONFIGURATION_SERVICE, IConfigurationService};
use je_platform::configuration::test_configuration_service::TestConfigurationService;
use je_platform::dialogs::dialogs::{
    Confirmation, ConfirmationResult, ConfirmationResultCell, I_DIALOG_SERVICE, IDialogService, Input, InputResultCell, Prompt, PromptResultCell,
};
use je_platform::files::files::FileStatWithMetadata;
use je_platform::files::i_file_service::{I_FILE_SERVICE, IFileService};
use je_platform::log::{I_LOG_SERVICE, NullLogService};
use je_platform::uri_identity::uri_identity::{I_URI_IDENTITY_SERVICE, IUriIdentityService};
use je_platform::uri_identity::uri_identity_service::UriIdentityService;
use je_workbench::api::browser::main_thread_bulk_edits::MainThreadBulkEdits;
use je_workbench::api::browser::main_thread_documents::MainThreadDocuments;
use je_workbench::api::browser::main_thread_editor::{IFocusTracker, MainThreadTextEditor};
use je_workbench::api::browser::main_thread_editors::{IMainThreadEditorLocator, MainThreadTextEditors};
use je_workbench::api::common::ext_host_protocol::{
    ApplyEditsOptions, IWorkspaceEditDto, IWorkspaceFileEditDto, MainThreadBulkEditsShape, MainThreadTextEditorsShape, SerializableObjectWithBuffers,
    WorkspaceEditEntryDto,
};
use je_workbench::common::editor::editor_model::IEditorModel;
use je_workbench::common::editor::IEditorPane;
use je_workbench::contrib::bulk_edit::browser::bulk_edit_service::BulkEditService;
use je_workbench::services::editor::editor_service::{I_EDITOR_SERVICE, IActiveCodeEditor};
use je_workbench::services::files_configuration::files_configuration_service::Readonly;
use je_workbench::services::lifecycle::lifecycle::I_LIFECYCLE_SERVICE;
use je_workbench::services::textfile::collaborators::{
    CopyOperation, CreateFileOperation, CreateOperation, DeleteOperation, FileOperationUndoRedoInfo, I_WORKING_COPY_FILE_SERVICE, IWorkingCopyFileService,
    MoveOperation, WorkingCopyFileEvent,
};
use je_workbench::services::textfile::textfiles::I_TEXT_FILE_SERVICE;
use je_workbench::services::textmodel_resolver::resolver_service::{I_TEXT_MODEL_SERVICE, ITextEditorModel, ITextModelService, TextModelReference};
use je_workbench::services::working_copy::working_copy_service::{I_WORKING_COPY_SERVICE, IWorkingCopyService, WorkingCopyService};
use je_workbench::test::browser::workbench_test_services::{TestEditorGroupsService, TestEditorService, TestEnvironmentService, TestPathService};
use je_workbench::test::common::workbench_test_services::{TestFileService, TestLifecycleService, TestTextResourcePropertiesService};
use main_thread_support::{FixedProxyContext, RecordingProxy, RecordingTextFileService, StubQuickDiffModelService};

const EDITOR_ID: &str = "testEditorId";

/// `editorLocator`.
struct EditorLocator {
    test_editor: RefCell<Option<Rc<MainThreadTextEditor>>>,
}

impl IMainThreadEditorLocator for EditorLocator {
    fn get_editor(&self, id: &str) -> Option<Rc<MainThreadTextEditor>> {
        if id == EDITOR_ID { self.test_editor.borrow().clone() } else { None }
    }

    fn find_text_editor_id_for(&self, _editor_pane: &dyn IEditorPane) -> Option<String> {
        None
    }

    fn get_id_of_code_editor(&self, _code_editor: &dyn IActiveCodeEditor) -> Option<String> {
        None
    }
}

/// `{ onGainedFocus() { }, onLostFocus() { } }`.
struct NoopFocusTracker;

impl IFocusTracker for NoopFocusTracker {
    fn on_gained_focus(&self) {}

    fn on_lost_focus(&self) {}
}

/// O membro que o `mock<T>()` não define: chamá-lo é um `TypeError`.
fn not_defined(member: &str) -> ! {
    errors::throw(Error::type_error(format!("{member} is not a function")))
}

/// `new class extends mock<IEditorWorkerService>() { }`.
struct EditorWorkerService;

impl IEditorWorkerService for EditorWorkerService {
    fn can_compute_word_ranges(&self, _resource: &URI) -> bool {
        not_defined("canComputeWordRanges")
    }

    fn compute_word_ranges(&self, _resource: &URI, _range: Range) -> Promise<Option<WordRanges>> {
        not_defined("computeWordRanges")
    }
}

/// `new TestDialogService()`: `confirm` não confirma.
struct TestDialogService;

impl IDialogService for TestDialogService {
    fn on_will_show_dialog(&self) -> Event<()> {
        Event::none()
    }

    fn on_did_show_dialog(&self) -> Event<()> {
        Event::none()
    }

    fn confirm(&self, _confirmation: Confirmation) -> Promise<ConfirmationResultCell> {
        Promise::resolved(ConfirmationResult { checkbox_checked: None, confirmed: false })
    }

    fn prompt(&self, _prompt: Prompt) -> Promise<PromptResultCell> {
        not_defined("prompt")
    }

    fn input(&self, _input: Input) -> Promise<InputResultCell> {
        not_defined("input")
    }

    fn info(&self, _message: &str, _detail: Option<&str>) -> Promise<()> {
        Promise::resolved(())
    }

    fn warn(&self, _message: &str, _detail: Option<&str>) -> Promise<()> {
        Promise::resolved(())
    }

    fn error(&self, _message: &str, _detail: Option<&str>) -> Promise<()> {
        Promise::resolved(())
    }

    fn about(&self) -> Promise<()> {
        Promise::resolved(())
    }
}

/// `new class extends mock<IWorkingCopyFileService>() { ... }`: registra o que cada operação recebe.
struct RecordingWorkingCopyFileService {
    /// `movedResources`.
    moved_resources: Rc<RefCell<Vec<(URI, URI)>>>,
    /// `copiedResources`.
    copied_resources: Rc<RefCell<Vec<(URI, URI)>>>,
    /// `createdResources`.
    created_resources: Rc<RefCell<Vec<URI>>>,
    /// `deletedResources`.
    deleted_resources: Rc<RefCell<Vec<URI>>>,
}

impl IWorkingCopyFileService for RecordingWorkingCopyFileService {
    fn on_will_run_working_copy_file_operation(&self) -> Event<WorkingCopyFileEvent> {
        Event::none()
    }

    fn on_did_fail_working_copy_file_operation(&self) -> Event<WorkingCopyFileEvent> {
        Event::none()
    }

    /// `onDidRunWorkingCopyFileOperation = Event.None`.
    fn on_did_run_working_copy_file_operation(&self) -> Event<WorkingCopyFileEvent> {
        Event::none()
    }

    fn create(&self, operations: Vec<CreateFileOperation>, _token: CancellationToken, _undo_info: Option<FileOperationUndoRedoInfo>) -> Promise<Vec<FileStatWithMetadata>> {
        self.created_resources.borrow_mut().extend(operations.into_iter().map(|operation| operation.resource));
        Promise::resolved(Vec::new())
    }

    /// `createFolder(operations) { this.create(operations); }`.
    fn create_folder(&self, operations: Vec<CreateOperation>, _token: CancellationToken, _undo_info: Option<FileOperationUndoRedoInfo>) -> Promise<Vec<FileStatWithMetadata>> {
        self.created_resources.borrow_mut().extend(operations.into_iter().map(|operation| operation.resource));
        Promise::resolved(Vec::new())
    }

    fn move_(&self, operations: Vec<MoveOperation>, _token: CancellationToken, _undo_info: Option<FileOperationUndoRedoInfo>) -> Promise<Vec<FileStatWithMetadata>> {
        let file = &operations[0].file;
        self.moved_resources.borrow_mut().push((file.source.clone(), file.target.clone()));
        Promise::resolved(Vec::new())
    }

    fn copy(&self, operations: Vec<CopyOperation>, _token: CancellationToken, _undo_info: Option<FileOperationUndoRedoInfo>) -> Promise<Vec<FileStatWithMetadata>> {
        let file = &operations[0].file;
        self.copied_resources.borrow_mut().push((file.source.clone(), file.target.clone()));
        Promise::resolved(Vec::new())
    }

    fn delete(&self, operations: Vec<DeleteOperation>, _token: CancellationToken, _undo_info: Option<FileOperationUndoRedoInfo>) -> Promise<()> {
        self.deleted_resources.borrow_mut().extend(operations.into_iter().map(|operation| operation.resource));
        Promise::resolved(())
    }
}

/// `new class extends mock<IResolvedTextEditorModel>() { override textEditorModel = modelService.getModel(resource)!; }` com
/// `isReadonly = () => false`.
struct ResolvedTextEditorModel {
    model: Rc<TextModel>,
}

impl IDisposable for ResolvedTextEditorModel {
    fn dispose(&self) {}
}

impl IEditorModel for ResolvedTextEditorModel {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn on_will_dispose(&self) -> Event<()> {
        not_defined("onWillDispose")
    }

    fn resolve(&self) -> Promise<()> {
        not_defined("resolve")
    }

    fn is_resolved(&self) -> bool {
        not_defined("isResolved")
    }

    fn is_disposed(&self) -> bool {
        not_defined("isDisposed")
    }
}

impl ITextEditorModel for ResolvedTextEditorModel {
    fn text_editor_model(&self) -> Option<Rc<TextModel>> {
        Some(self.model.clone())
    }

    fn is_readonly(&self) -> Readonly {
        Readonly::Bool(false)
    }

    fn create_snapshot(&self) -> Option<Box<dyn TextSnapshot>> {
        not_defined("createSnapshot")
    }
}

/// `new class extends mock<ITextModelService>() { createModelReference(resource) { ... } }`: uma referência imortal com o
/// modelo do `ModelService`.
struct ModelServiceTextModelService {
    model_service: Rc<dyn IModelService>,
}

impl ITextModelService for ModelServiceTextModelService {
    fn create_model_reference(&self, resource: &URI) -> Promise<TextModelReference> {
        let model = self.model_service.get_model(resource).expect("modelService.getModel(resource)!");
        let text_editor_model: Rc<dyn ITextEditorModel> = Rc::new(ResolvedTextEditorModel { model });
        Promise::resolved(Reference::immortal(text_editor_model))
    }
}

/// O estado que o `setup` da suíte deixa nas variáveis do `suite`.
struct Fixture {
    model_service: Rc<ModelService>,
    bulk_edits: Rc<MainThreadBulkEdits>,
    editors: Rc<MainThreadTextEditors>,
    /// `editorLocator` (o `MainThreadTextEditors` só guarda um `Weak`).
    _editor_locator: Rc<EditorLocator>,
    moved_resources: Rc<RefCell<Vec<(URI, URI)>>>,
    created_resources: Rc<RefCell<Vec<URI>>>,
    deleted_resources: Rc<RefCell<Vec<URI>>>,
}

impl Fixture {
    /// `setup`.
    fn new(disposables: &LeakGuard) -> Self {
        let moved_resources: Rc<RefCell<Vec<(URI, URI)>>> = Rc::default();
        let copied_resources: Rc<RefCell<Vec<(URI, URI)>>> = Rc::default();
        let created_resources: Rc<RefCell<Vec<URI>>> = Rc::default();
        let deleted_resources: Rc<RefCell<Vec<URI>>> = Rc::default();

        let config_service = TestConfigurationService::new(None);
        let configuration_service: Rc<dyn IConfigurationService> = config_service.clone();
        let undo_redo_service = UndoRedoService::for_tests();

        let model_service = disposables.add_rc(ModelService::new(
            configuration_service.clone(),
            TestTextResourcePropertiesService::new(configuration_service.clone()),
            undo_redo_service.clone(),
        ));
        let model_service_contract: Rc<dyn IModelService> = model_service.clone();
        let code_editor_service = Rc::new(TestCodeEditorService::new(TestThemeService::new_default()));
        let code_editor_registry = disposables.add_rc(code_editor_service.registry().clone());
        let code_editor_registry: Rc<dyn ICodeEditorRegistry> = code_editor_registry;

        let file_service = disposables.add_rc(TestFileService::new());
        let file_service_contract: Rc<dyn IFileService> = file_service.clone();
        let uri_identity_service = disposables.add_rc(UriIdentityService::create((), (file_service_contract.clone(),)));
        let uri_identity_contract: Rc<dyn IUriIdentityService> = uri_identity_service.clone();

        let proxy = Rc::new(RecordingProxy::default());
        let ext_host_context = FixedProxyContext::new(proxy);

        let text_file_service = Rc::new(RecordingTextFileService::new(created_resources.clone()));
        let working_copy_file_service = Rc::new(RecordingWorkingCopyFileService {
            moved_resources: moved_resources.clone(),
            copied_resources,
            created_resources: created_resources.clone(),
            deleted_resources: deleted_resources.clone(),
        });
        let text_model_service = Rc::new(ModelServiceTextModelService { model_service: model_service_contract.clone() });

        let undo_redo_contract: Rc<dyn IUndoRedoService> = undo_redo_service;
        let instantiation = TestInstantiationService::new();
        instantiation.stub(I_LOG_SERVICE, NullLogService::shared());
        instantiation.stub(I_CONFIGURATION_SERVICE, configuration_service.clone());
        instantiation.stub(I_DIALOG_SERVICE, Rc::new(TestDialogService));
        instantiation.stub(I_UNDO_REDO_SERVICE, undo_redo_contract);
        instantiation.stub(I_MODEL_SERVICE, model_service_contract);
        instantiation.stub(I_FILE_SERVICE, file_service_contract);
        instantiation.stub(I_URI_IDENTITY_SERVICE, uri_identity_contract.clone());
        instantiation.stub(I_EDITOR_SERVICE, Rc::new(TestEditorService));
        instantiation.stub(I_LIFECYCLE_SERVICE, TestLifecycleService::new());
        let working_copy_service: Rc<dyn IWorkingCopyService> = WorkingCopyService::new();
        instantiation.stub(I_WORKING_COPY_SERVICE, working_copy_service);
        instantiation.stub(I_TEXT_FILE_SERVICE, text_file_service.clone());
        instantiation.stub(I_WORKING_COPY_FILE_SERVICE, working_copy_file_service.clone());
        instantiation.stub(I_TEXT_MODEL_SERVICE, text_model_service.clone());
        instantiation.stub(I_EDITOR_WORKER_SERVICE, Rc::new(EditorWorkerService));
        let bulk_edit_service: Rc<dyn IBulkEditService> = instantiation.create_instance::<BulkEditService>(());
        instantiation.stub(I_BULK_EDIT_SERVICE, bulk_edit_service.clone());

        let bulk_edits = MainThreadBulkEdits::new(&ext_host_context, bulk_edit_service, NullLogService::shared(), uri_identity_contract);
        let documents = disposables.add_rc(MainThreadDocuments::new(
            &ext_host_context,
            model_service.clone(),
            text_file_service,
            file_service.clone(),
            text_model_service,
            TestEnvironmentService::new(),
            uri_identity_service.clone(),
            working_copy_file_service,
            TestPathService::new(None, None),
        ));

        // Create editor locator
        let editor_locator = Rc::new(EditorLocator { test_editor: RefCell::new(None) });
        let locator: Weak<dyn IMainThreadEditorLocator> = Rc::downgrade(&editor_locator) as Weak<dyn IMainThreadEditorLocator>;

        let editors = MainThreadTextEditors::new(
            locator,
            &ext_host_context,
            code_editor_registry,
            Rc::new(TestEditorService),
            TestEditorGroupsService::new(None),
            configuration_service,
            Rc::new(StubQuickDiffModelService),
            uri_identity_service,
        );
        disposables.add_rc(editors.clone());

        // Create a test code editor using the helper
        let model = model_service.create_model("Hello world!", None, Some(&URI::parse("foo:existing", false)), false);
        // O `ModelService` do porte não é filho do `InstantiationService` do caso, então não descarta o modelo no teardown como o do original.
        disposables.add_rc(model.clone());
        let test_code_editor = disposables.add_rc(create_test_code_editor(Some(model.clone()), TestCodeEditorInstantiationOptions::default()));

        let clipboard_service = Rc::new(TestClipboardService::new());
        let test_editor = disposables.add_rc(MainThreadTextEditor::new(
            EDITOR_ID.to_string(),
            model,
            test_code_editor.widget().clone(),
            Rc::new(NoopFocusTracker),
            documents,
            model_service.clone(),
            clipboard_service,
        ));
        *editor_locator.test_editor.borrow_mut() = Some(test_editor);

        Self { model_service, bulk_edits, editors, _editor_locator: editor_locator, moved_resources, created_resources, deleted_resources }
    }
}

/// `{ range, text, forceMoveMarkers: false }`.
fn edit(range: Range, text: &str) -> SingleEditOperation {
    SingleEditOperation::new(range, Some(JsString::from(text)), false)
}

/// `{ undoStopBefore, undoStopAfter }` de `$tryApplyEdits`.
fn undo_stops(undo_stop_before: bool, undo_stop_after: bool) -> ApplyEditsOptions {
    ApplyEditsOptions { set_end_of_line: None, undo_stop_before, undo_stop_after }
}

/// `const resource = URI.parse('foo:bar')`.
fn resource() -> URI {
    URI::parse("foo:bar", false)
}

/// `new SerializableObjectWithBuffers({ edits })`.
fn workspace_edit(edits: Vec<WorkspaceEditEntryDto>) -> SerializableObjectWithBuffers<IWorkspaceEditDto> {
    SerializableObjectWithBuffers::new(IWorkspaceEditDto { edits })
}

/// `IWorkspaceTextEditDto`: `{ resource, versionId, textEdit: { range, text, eol? } }`.
fn text_edit_dto(resource: URI, version_id: i64, range: Range, text: &str, eol: Option<EndOfLineSequence>) -> WorkspaceEditEntryDto {
    WorkspaceEditEntryDto::Text(IWorkspaceTextEdit {
        resource,
        text_edit: TextEdit { range, text: text.to_string(), eol, insert_as_snippet: None, keep_whitespace: None },
        version_id: Some(version_id),
        metadata: None,
    })
}

/// `{ oldResource, newResource, options: undefined }`.
fn file_edit_dto(old_resource: Option<URI>, new_resource: Option<URI>) -> WorkspaceEditEntryDto {
    WorkspaceEditEntryDto::File(IWorkspaceFileEditDto { old_resource, new_resource, options: None, metadata: None })
}

/// `a === b` das `URI` que os mapas do original comparam por identidade.
fn same_uri(a: &URI, b: &URI) -> bool {
    a.to_string(false) == b.to_string(false)
}

#[test]
fn apply_workspace_edit_returns_false_if_model_is_changed_by_user() {
    let disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let fixture = Fixture::new(&disposables);
    let store = disposables.store();
    run_with_faked_timers(async move {
        let model = store.add_rc(fixture.model_service.create_model("something", None, Some(&resource()), false));

        let workspace_resource_edit = text_edit_dto(resource(), model.get_version_id(), Range::new(1, 1, 1, 1), "asdfg", None);

        // Act as if the user edited the model
        model.apply_edits(&[EditOperation::insert(Position::new(0, 0), "something")]);

        let result = fixture.bulk_edits.rpc_try_apply_workspace_edit(workspace_edit(vec![workspace_resource_edit]), None, None).await.expect("result");
        assert!(!result);
    });
}

#[test]
fn issue_54773_apply_workspace_edit_checks_model_version_in_race_situation() {
    let disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let fixture = Fixture::new(&disposables);
    let store = disposables.store();
    run_with_faked_timers(async move {
        let model = store.add_rc(fixture.model_service.create_model("something", None, Some(&resource()), false));

        let workspace_resource_edit1 = text_edit_dto(resource(), model.get_version_id(), Range::new(1, 1, 1, 1), "asdfg", None);
        let workspace_resource_edit2 = text_edit_dto(resource(), model.get_version_id(), Range::new(1, 1, 1, 1), "asdfg", None);

        let p1 = fixture.bulk_edits.rpc_try_apply_workspace_edit(workspace_edit(vec![workspace_resource_edit1]), None, None);
        let p2 = fixture.bulk_edits.rpc_try_apply_workspace_edit(workspace_edit(vec![workspace_resource_edit2]), None, None);

        // first edit request succeeds
        assert!(p1.await.expect("p1"));
        // second edit request fails
        assert!(!p2.await.expect("p2"));
    });
}

#[test]
fn apply_workspace_edit_noop_eol_edit_keeps_undo_stack_clean() {
    let disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let fixture = Fixture::new(&disposables);
    let store = disposables.store();
    run_with_faked_timers(async move {
        let initial_text = "hello\nworld";
        let model = store.add_rc(fixture.model_service.create_model(initial_text, None, Some(&resource()), false));
        let initial_alternative_version_id = model.get_alternative_version_id();

        let insert_edit = text_edit_dto(resource(), model.get_version_id(), Range::new(1, 6, 1, 6), "2", None);

        let insert_result = fixture.bulk_edits.rpc_try_apply_workspace_edit(workspace_edit(vec![insert_edit]), None, None).await.expect("insertResult");
        assert!(insert_result);
        assert_eq!(model.get_value().to_string_lossy(), "hello2\nworld");
        assert_ne!(model.get_alternative_version_id(), initial_alternative_version_id);

        let eol_edit = text_edit_dto(resource(), model.get_version_id(), Range::new(1, 1, 1, 1), "", Some(EndOfLineSequence::Lf));

        let eol_result = fixture.bulk_edits.rpc_try_apply_workspace_edit(workspace_edit(vec![eol_edit]), None, None).await.expect("eolResult");
        assert!(eol_result);
        assert_eq!(model.get_value().to_string_lossy(), "hello2\nworld");

        model.undo();
        assert_eq!(model.get_value().to_string_lossy(), initial_text);
        assert_eq!(model.get_alternative_version_id(), initial_alternative_version_id);
    });
}

#[test]
fn apply_workspace_edit_with_only_resource_edit() {
    let disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let fixture = Fixture::new(&disposables);
    run_with_faked_timers(async move {
        let result = fixture
            .bulk_edits
            .rpc_try_apply_workspace_edit(
                workspace_edit(vec![
                    file_edit_dto(Some(resource()), Some(resource())),
                    file_edit_dto(None, Some(resource())),
                    file_edit_dto(Some(resource()), None),
                ]),
                None,
                None,
            )
            .await
            .expect("result");
        assert!(result);
        assert!(fixture.moved_resources.borrow().iter().any(|(source, target)| same_uri(source, &resource()) && same_uri(target, &resource())));
        assert!(fixture.created_resources.borrow().iter().any(|created| same_uri(created, &resource())));
        assert!(fixture.deleted_resources.borrow().iter().any(|deleted| same_uri(deleted, &resource())));
    });
}

#[test]
fn apply_workspace_edit_can_control_undo_redo_stack_1() {
    let disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let fixture = Fixture::new(&disposables);
    run_with_faked_timers(async move {
        let model = fixture.model_service.get_model(&URI::parse("foo:existing", false)).expect("modelo existente");

        let edit1 = edit(Range::new(1, 1, 1, 2), "h");

        let applied1 = fixture.editors.rpc_try_apply_edits(EDITOR_ID, model.get_version_id() as f64, vec![edit1], undo_stops(false, false)).await.expect("applied1");
        assert!(applied1);
        assert_eq!(model.get_value().to_string_lossy(), "hello world!");

        let edit2 = edit(Range::new(1, 2, 1, 6), "ELLO");

        let applied2 = fixture.editors.rpc_try_apply_edits(EDITOR_ID, model.get_version_id() as f64, vec![edit2], undo_stops(false, false)).await.expect("applied2");
        assert!(applied2);
        assert_eq!(model.get_value().to_string_lossy(), "hELLO world!");

        model.undo();
        assert_eq!(model.get_value().to_string_lossy(), "Hello world!");
    });
}

#[test]
fn apply_workspace_edit_can_control_undo_redo_stack_2() {
    let disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let fixture = Fixture::new(&disposables);
    run_with_faked_timers(async move {
        let model = fixture.model_service.get_model(&URI::parse("foo:existing", false)).expect("modelo existente");

        let edit1 = edit(Range::new(1, 1, 1, 2), "h");

        let applied1 = fixture.editors.rpc_try_apply_edits(EDITOR_ID, model.get_version_id() as f64, vec![edit1], undo_stops(false, false)).await.expect("applied1");
        assert!(applied1);
        assert_eq!(model.get_value().to_string_lossy(), "hello world!");

        let edit2 = edit(Range::new(1, 2, 1, 6), "ELLO");

        let applied2 = fixture.editors.rpc_try_apply_edits(EDITOR_ID, model.get_version_id() as f64, vec![edit2], undo_stops(true, false)).await.expect("applied2");
        assert!(applied2);
        assert_eq!(model.get_value().to_string_lossy(), "hELLO world!");

        model.undo();
        assert_eq!(model.get_value().to_string_lossy(), "hello world!");
    });
}
