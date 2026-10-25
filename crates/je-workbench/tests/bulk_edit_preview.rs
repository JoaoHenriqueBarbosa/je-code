//! Porte de `src/vs/workbench/contrib/bulkEdit/test/browser/bulkEditPreview.test.ts` (suíte `BulkEditPreview`), caso a caso e
//! na mesma ordem.
//!
//! Notas de tradução:
//! - **`ensureNoDisposablesAreLeakedInTestSuite()` e `store`.** O `LeakGuard` no começo de cada caso; o `store.add(ops)` é o
//!   `guard.add_rc(ops)`.
//! - **`mock<T>()`.** Cada dublê implementa o contrato inteiro; o membro que o original não sobrescreve lança o `TypeError`
//!   do `mock` ([`not_defined`]).
//! - **Identidade das edições.** `ops.checked.isChecked(edits[0])` é `ops.checked.is_checked(&0)` (a chave é o índice da
//!   edição), e `edits === ops.getWorkspaceEdit()` é `Rc::ptr_eq`.
//! - **`new InstantiationService(new ServiceCollection(...))`.** É o `TestInstantiationService` com os dois serviços do
//!   original, mais o serviço de log que o `ConflictDetector` lê.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
#[path = "../../je-editor/tests/support/test_instantiation_service.rs"]
mod test_instantiation_service;

use std::rc::Rc;

use common::{ensure_no_disposables_are_leaked_in_test_suite, run_with_faked_timers};
use je_base::cancellation::CancellationToken;
use je_base::errors::{self, Error};
use je_base::event::Event;
use je_base::lifecycle::DisposableRef;
use je_base::promise::Promise;
use je_base::uri::URI;
use je_editor::bulk_edit_service::{ResourceEdit, ResourceFileEdit, ResourceTextEdit};
use je_editor::code_action::languages::{TextEdit, WorkspaceEditMetadata};
use je_editor::language_service::ILanguageSelection;
use je_editor::model::TextModelCreationOptions;
use je_editor::range::Range;
use je_editor::services_model::{I_MODEL_SERVICE, IModelService};
use je_editor::text_model::{TextModel, TextSource};
use je_editor::text_model_edit_source::TextModelEditSource;
use je_instantiation::ServiceCollection;
use je_platform::files::file_service::FileContentSource;
use je_platform::files::files::{
    CreateFileOptions, FileChangesEvent, FileContent, FileOperationEvent, FileStat, FileStatWithMetadata, FileStreamContent, FileSystemProviderCapabilities,
    FileSystemProviderCapabilitiesChangeEvent, FileSystemProviderRef, FileSystemProviderRegistrationEvent, PartialFileDeleteOptions, ReadFileOptions,
    ResolveFileOptions, WatchOptions, WriteFileOptions,
};
use je_platform::files::i_file_service::{I_FILE_SERVICE, IFileService};
use je_platform::log::{I_LOG_SERVICE, NullLogService};
use je_workbench::contrib::bulk_edit::browser::preview::bulk_edit_preview::BulkFileOperations;
use test_instantiation_service::TestInstantiationService;

/// O membro que o `mock<T>()` não define: chamá-lo é um `TypeError`.
fn not_defined(member: &str) -> ! {
    errors::throw(Error::type_error(format!("{member} is not a function")))
}

/// `new class extends mock<IFileService>() { onDidFilesChange = Event.None; exists() { return true } }`.
struct FakeFileService;

impl IFileService for FakeFileService {
    fn on_did_files_change(&self) -> Event<Rc<FileChangesEvent>> {
        Event::none()
    }

    fn on_will_activate_file_system_provider(&self) -> Event<je_platform::files::files::FileSystemProviderActivationEvent> {
        not_defined("onWillActivateFileSystemProvider")
    }

    fn register_provider(&self, _scheme: &str, _provider: FileSystemProviderRef) -> Result<DisposableRef, Error> {
        not_defined("registerProvider")
    }

    fn get_provider(&self, _scheme: &str) -> Option<FileSystemProviderRef> {
        not_defined("getProvider")
    }

    fn resolve_with_metadata(&self, _resource: &URI, _options: Option<ResolveFileOptions>) -> Promise<FileStatWithMetadata> {
        not_defined("resolve")
    }

    fn exists(&self, _resource: &URI) -> Promise<bool> {
        Promise::resolved(true)
    }

    fn on_did_change_file_system_provider_registrations(&self) -> Event<FileSystemProviderRegistrationEvent> {
        not_defined("onDidChangeFileSystemProviderRegistrations")
    }

    fn on_did_change_file_system_provider_capabilities(&self) -> Event<FileSystemProviderCapabilitiesChangeEvent> {
        not_defined("onDidChangeFileSystemProviderCapabilities")
    }

    fn has_provider(&self, _resource: &URI) -> bool {
        not_defined("hasProvider")
    }

    fn has_capability(&self, _resource: &URI, _capability: FileSystemProviderCapabilities) -> bool {
        not_defined("hasCapability")
    }

    fn read_file(&self, _resource: &URI, _options: Option<ReadFileOptions>, _token: Option<CancellationToken>) -> Promise<FileContent> {
        not_defined("readFile")
    }

    fn resolve(&self, _resource: &URI, _options: Option<ResolveFileOptions>) -> Promise<FileStat> {
        not_defined("resolve")
    }

    fn create_folder(&self, _resource: &URI) -> Promise<FileStatWithMetadata> {
        not_defined("createFolder")
    }

    fn watch(&self, _resource: &URI, _options: Option<WatchOptions>) -> DisposableRef {
        not_defined("watch")
    }

    fn read_file_stream(&self, _resource: &URI, _options: Option<ReadFileOptions>, _token: Option<CancellationToken>) -> Promise<FileStreamContent> {
        not_defined("readFileStream")
    }

    fn write_file(&self, _resource: &URI, _content: FileContentSource, _options: Option<WriteFileOptions>) -> Promise<FileStatWithMetadata> {
        not_defined("writeFile")
    }

    fn del(&self, _resource: &URI, _options: Option<PartialFileDeleteOptions>) -> Promise<()> {
        not_defined("del")
    }

    fn on_did_run_operation(&self) -> Event<FileOperationEvent> {
        not_defined("onDidRunOperation")
    }

    fn create_file(&self, _resource: &URI, _content: Option<FileContentSource>, _options: Option<CreateFileOptions>) -> Promise<FileStatWithMetadata> {
        not_defined("createFile")
    }
}

/// `new class extends mock<IModelService>() { getModel() { return null } getModels() { return [] } }`.
struct FakeModelService;

impl IModelService for FakeModelService {
    fn get_model(&self, _resource: &URI) -> Option<Rc<TextModel>> {
        None
    }

    fn get_models(&self) -> Vec<Rc<TextModel>> {
        Vec::new()
    }

    fn on_model_added(&self) -> Event<Rc<TextModel>> {
        not_defined("onModelAdded")
    }

    fn on_model_removed(&self) -> Event<Rc<TextModel>> {
        not_defined("onModelRemoved")
    }

    fn create_model(&self, _value: TextSource, _language_selection: Option<&dyn ILanguageSelection>, _resource: Option<&URI>, _is_for_simple_widget: bool) -> Rc<TextModel> {
        not_defined("createModel")
    }

    fn update_model(&self, _model: &TextModel, _value: TextSource, _reason: Option<TextModelEditSource>) {
        not_defined("updateModel")
    }

    fn destroy_model(&self, _resource: &URI) {
        not_defined("destroyModel")
    }

    fn get_creation_options(&self, _language: &str, _resource: Option<&URI>, _is_for_simple_widget: bool) -> TextModelCreationOptions {
        not_defined("getCreationOptions")
    }
}

/// O `setup`: o serviço de instanciação com o `IFileService` e o `IModelService` do original.
fn setup() -> Rc<TestInstantiationService> {
    let insta_service = TestInstantiationService::with_options(ServiceCollection::new(), false, None, false);
    insta_service.stub(I_FILE_SERVICE, Rc::new(FakeFileService));
    insta_service.stub(I_MODEL_SERVICE, Rc::new(FakeModelService));
    insta_service.stub(I_LOG_SERVICE, NullLogService::shared());
    insta_service
}

fn metadata(label: &str, needs_confirmation: bool) -> Option<WorkspaceEditMetadata> {
    Some(WorkspaceEditMetadata { label: label.to_string(), needs_confirmation, description: None, icon_path: None })
}

fn uri(value: &str) -> URI {
    URI::parse(value, false)
}

fn file_edit(old: Option<&str>, new: Option<&str>, metadata: Option<WorkspaceEditMetadata>) -> ResourceEdit {
    ResourceEdit::File(ResourceFileEdit::new(old.map(uri), new.map(uri), None, metadata))
}

fn text_edit(resource: &str, metadata: Option<WorkspaceEditMetadata>) -> ResourceEdit {
    ResourceEdit::Text(ResourceTextEdit::new(
        uri(resource),
        TextEdit { text: "foo".to_string(), range: Range::new(1, 1, 1, 1), eol: None, insert_as_snippet: None, keep_whitespace: None },
        None,
        metadata,
    ))
}

fn create_ops(insta_service: &Rc<TestInstantiationService>, edits: &Rc<Vec<ResourceEdit>>) -> Promise<Rc<BulkFileOperations>> {
    let edits = edits.clone();
    insta_service.invoke_function(|accessor| BulkFileOperations::create(accessor, edits))
}

#[test]
fn one_needs_confirmation_unchecks_all_of_file() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let insta_service = setup();
    run_with_faked_timers(async move {
        let edits = Rc::new(vec![
            file_edit(None, Some("some:///uri1"), metadata("cat1", true)),
            file_edit(Some("some:///uri1"), Some("some:///uri2"), metadata("cat2", false)),
        ]);

        let ops = create_ops(&insta_service, &edits).await.expect("create");
        guard.add_rc(ops.clone());
        assert_eq!(ops.file_operations().len(), 1);
        assert!(!ops.checked.is_checked(&0));
    });
}

#[test]
fn has_categories() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let insta_service = setup();
    run_with_faked_timers(async move {
        let edits = Rc::new(vec![file_edit(None, Some("some:///uri1"), metadata("uri1", true)), file_edit(None, Some("some:///uri2"), metadata("uri2", false))]);

        let ops = create_ops(&insta_service, &edits).await.expect("create");
        guard.add_rc(ops.clone());
        let categories = ops.categories();
        assert_eq!(categories.len(), 2);
        assert_eq!(categories[0].metadata.label, "uri1"); // unconfirmed!
        assert_eq!(categories[1].metadata.label, "uri2");
    });
}

#[test]
fn has_not_categories() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let insta_service = setup();
    run_with_faked_timers(async move {
        let edits = Rc::new(vec![file_edit(None, Some("some:///uri1"), metadata("uri1", true)), file_edit(None, Some("some:///uri2"), metadata("uri1", false))]);

        let ops = create_ops(&insta_service, &edits).await.expect("create");
        guard.add_rc(ops.clone());
        let categories = ops.categories();
        assert_eq!(categories.len(), 1);
        assert_eq!(categories[0].metadata.label, "uri1"); // unconfirmed!
        assert_eq!(categories[0].metadata.label, "uri1");
    });
}

#[test]
fn category_selection() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let insta_service = setup();
    run_with_faked_timers(async move {
        let edits = Rc::new(vec![file_edit(None, Some("some:///uri1"), metadata("C1", false)), text_edit("some:///uri2", metadata("C2", false))]);

        let ops = create_ops(&insta_service, &edits).await.expect("create");
        guard.add_rc(ops.clone());

        assert!(ops.checked.is_checked(&0));
        assert!(ops.checked.is_checked(&1));

        assert!(Rc::ptr_eq(&edits, &ops.get_workspace_edit()));

        // NOT taking to create, but the invalid text edit will
        // go through
        ops.checked.update_checked(&0, false);
        let new_edits = ops.get_workspace_edit();
        assert!(!Rc::ptr_eq(&edits, &new_edits));

        assert_eq!(edits.len(), 2);
        assert_eq!(new_edits.len(), 1);
    });
}

#[test]
fn fix_bad_metadata() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let insta_service = setup();
    run_with_faked_timers(async move {
        // bogous edit that wants creation to be confirmed, but not it's textedit-child...

        let edits = Rc::new(vec![file_edit(None, Some("some:///uri1"), metadata("C1", true)), text_edit("some:///uri1", metadata("C2", false))]);

        let ops = create_ops(&insta_service, &edits).await.expect("create");
        guard.add_rc(ops.clone());

        assert!(!ops.checked.is_checked(&0));
        assert!(!ops.checked.is_checked(&1));
    });
}
