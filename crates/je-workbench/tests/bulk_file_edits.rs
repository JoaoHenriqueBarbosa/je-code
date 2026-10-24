//! Porte de `src/vs/workbench/contrib/bulkEdit/test/browser/bulkFileEdits.test.ts` (suíte `BulkFileEdits`), caso a caso e na
//! mesma ordem.
//!
//! Notas de tradução:
//! - **`ensureNoDisposablesAreLeakedInTestSuite()` e `store`.** O `LeakGuard` no começo de cada caso; o `store.add(x)` do
//!   original é o `guard.add_rc(x)`.
//! - **`mock<T>()`.** Cada dublê implementa o contrato inteiro, e o membro que o original não sobrescreve lança o
//!   `TypeError` do `mock` ([`not_defined`]). O `ITextFileService` é o [`StubTextFileService`] das suítes de `MainThread*`,
//!   cujo `getEncodedReadable` falha, como o `assert.fail` do original.
//! - **`fileService.resolve(resource)`.** É o `resolve_with_metadata` do contrato do porte (o `resolve` com
//!   `resolveMetadata: true` que o `DeleteOperation` chama).
//! - **`new InstantiationService(new ServiceCollection(...), true)`.** É o `TestInstantiationService` estrito, com os
//!   mesmos seis serviços, e o `createInstance(BulkFileEdits, ...)` é o `create_instance::<BulkFileEdits>`.
//! - **`FileType` e `IFileStatWithMetadata`.** As máscaras de [`FileType`] viram `is_file`, `is_directory` e
//!   `is_symbolic_link` do [`FileStat`]; o `value` do arquivo é o [`VSBuffer`] que o `readFile` devolve.
//! - **`assert.ok(contents instanceof VSBuffer)`.** É o `match` em [`EncodedContent::Buffer`].
//! - **`await undoElement.undo()`.** O `undo` do elemento de workspace devolve `MaybePromise`, esperado por
//!   [`await_maybe`].
//! - **`options` espalhado.** `{ recursive: true, folder: file.isDirectory, ...options }` é a opção do caso quando existe e o
//!   valor do literal quando não (`None` é a chave ausente).

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
#[path = "../../je-editor/tests/support/test_instantiation_service.rs"]
mod test_instantiation_service;
#[allow(dead_code, unused_imports)]
#[path = "support/main_thread_support.rs"]
mod main_thread_support;

use std::cell::RefCell;
use std::future::Future;
use std::rc::Rc;

use common::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite, run_with_faked_timers};
use je_base::async_::{async_fn, await_maybe};
use je_base::buffer::VSBuffer;
use je_base::cancellation::CancellationToken;
use je_base::errors::{self, Error};
use je_base::event::Event;
use je_base::lifecycle::DisposableRef;
use je_base::path::Platform;
use je_base::promise::Promise;
use je_base::uri::URI;
use je_editor::bulk_edit_service::ResourceFileEdit;
use je_editor::code_action::languages::WorkspaceFileEditOptions;
use je_editor::undo_redo::{
    I_UNDO_REDO_SERVICE, IUndoRedoService, PastFutureElements, ResourceEditStackSnapshot, UndoRedoElementRef, UndoRedoGroup, UndoRedoSource, UndoRedoTarget,
    UriComparisonKeyComputer,
};
use je_instantiation::ServiceCollection;
use je_platform::configuration::service::I_CONFIGURATION_SERVICE;
use je_platform::configuration::test_configuration_service::TestConfigurationService;
use je_platform::files::file_service::FileContentSource;
use je_platform::files::files::{
    CreateFileOptions, FileChangesEvent, FileContent, FileOperationEvent, FileStat, FileStatWithMetadata, FileStreamContent, FileSystemProviderCapabilities,
    FileSystemProviderCapabilitiesChangeEvent, FileSystemProviderRef, FileSystemProviderRegistrationEvent, FileType, PartialFileDeleteOptions, ReadFileOptions,
    ResolveFileOptions, WatchOptions, WriteFileOptions,
};
use je_platform::files::i_file_service::{I_FILE_SERVICE, IFileService};
use je_platform::log::{I_LOG_SERVICE, NullLogService};
use je_platform::progress::progress::Progress;
use je_platform::resources::ext_uri;
use je_workbench::contrib::bulk_edit::browser::bulk_file_edits::{BulkFileEdits, BulkFileEditsArgs};
use je_workbench::services::textfile::collaborators::{
    CreateFileOperation, CreateOperation, DeleteOperation, FileOperationUndoRedoInfo, I_WORKING_COPY_FILE_SERVICE, IWorkingCopyFileService, WorkingCopyFileEvent,
};
use je_workbench::services::textfile::textfiles::{EncodedContent, I_TEXT_FILE_SERVICE};
use main_thread_support::StubTextFileService;
use test_instantiation_service::TestInstantiationService;

/// O membro que o `mock<T>()` não define: chamá-lo é um `TypeError`.
fn not_defined(member: &str) -> ! {
    errors::throw(Error::type_error(format!("{member} is not a function")))
}

/// `{ kind: 'read' | 'delete' | 'create' | 'createFolder'; resource: string; contents?: number[] }`.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Operation {
    kind: &'static str,
    resource: String,
    contents: Option<Vec<u8>>,
}

fn operation(kind: &'static str, resource: &str) -> Operation {
    Operation { kind, resource: resource.to_string(), contents: None }
}

fn create_operation(resource: &str, contents: &[u8]) -> Operation {
    Operation { kind: "create", resource: resource.to_string(), contents: Some(contents.to_vec()) }
}

type Operations = Rc<RefCell<Vec<Operation>>>;

/// Um dos `files` do caso: o `IFileStatWithMetadata` com o `value` do `IFileContent`.
struct File {
    stat: FileStatWithMetadata,
    value: VSBuffer,
}

/// `new class extends mock<IFileService>() { resolve, hasCapability, readFile }`.
struct FakeFileService {
    files: Vec<File>,
    operations: Operations,
}

impl FakeFileService {
    /// `getFile(resource)`.
    fn get_file(&self, resource: &URI) -> &File {
        self.files.iter().find(|file| ext_uri().is_equal(Some(&file.stat.resource), Some(resource), false)).expect("assert.ok(file)")
    }
}

impl IFileService for FakeFileService {
    fn on_did_files_change(&self) -> Event<Rc<FileChangesEvent>> {
        not_defined("onDidFilesChange")
    }

    fn on_will_activate_file_system_provider(&self) -> Event<je_platform::files::files::FileSystemProviderActivationEvent> {
        not_defined("onWillActivateFileSystemProvider")
    }

    fn register_provider(&self, _scheme: &str, _provider: je_platform::files::files::FileSystemProviderRef) -> Result<je_base::lifecycle::DisposableRef, je_base::errors::Error> {
        not_defined("registerProvider")
    }

    fn get_provider(&self, _scheme: &str) -> Option<FileSystemProviderRef> {
        not_defined("getProvider")
    }

    fn resolve_with_metadata(&self, resource: &URI, _options: Option<ResolveFileOptions>) -> Promise<FileStatWithMetadata> {
        Promise::resolved(self.get_file(resource).stat.clone())
    }

    fn exists(&self, _resource: &URI) -> Promise<bool> {
        not_defined("exists")
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
        false
    }

    fn read_file(&self, resource: &URI, _options: Option<ReadFileOptions>, _token: Option<CancellationToken>) -> Promise<FileContent> {
        self.operations.borrow_mut().push(operation("read", &resource.path().to_string_lossy()));
        let file = self.get_file(resource);
        Promise::resolved(FileContent { stat: file.stat.clone(), value: file.value.clone() })
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

/// `new class extends mock<IWorkingCopyFileService>() { delete, create, createFolder }`.
struct FakeWorkingCopyFileService {
    operations: Operations,
}

impl IWorkingCopyFileService for FakeWorkingCopyFileService {
    fn on_will_run_working_copy_file_operation(&self) -> Event<WorkingCopyFileEvent> {
        not_defined("onWillRunWorkingCopyFileOperation")
    }

    fn on_did_fail_working_copy_file_operation(&self) -> Event<WorkingCopyFileEvent> {
        not_defined("onDidFailWorkingCopyFileOperation")
    }

    fn on_did_run_working_copy_file_operation(&self) -> Event<WorkingCopyFileEvent> {
        not_defined("onDidRunWorkingCopyFileOperation")
    }

    fn delete(&self, deletes: Vec<DeleteOperation>, _token: CancellationToken, _undo_info: Option<FileOperationUndoRedoInfo>) -> Promise<()> {
        let operations = self.operations.clone();
        async_fn(async move {
            for DeleteOperation { resource, .. } in deletes {
                operations.borrow_mut().push(operation("delete", &resource.path().to_string_lossy()));
            }
            Ok(())
        })
    }

    fn create(&self, creates: Vec<CreateFileOperation>, _token: CancellationToken, _undo_info: Option<FileOperationUndoRedoInfo>) -> Promise<Vec<FileStatWithMetadata>> {
        let operations = self.operations.clone();
        async_fn(async move {
            for CreateFileOperation { resource, contents, .. } in creates {
                let Some(EncodedContent::Buffer(contents)) = contents else {
                    panic!("assert.ok(contents instanceof VSBuffer)");
                };
                operations.borrow_mut().push(create_operation(&resource.path().to_string_lossy(), &contents.to_vec()));
            }
            Ok(Vec::new())
        })
    }

    fn create_folder(&self, creates: Vec<CreateOperation>, _token: CancellationToken, _undo_info: Option<FileOperationUndoRedoInfo>) -> Promise<Vec<FileStatWithMetadata>> {
        let operations = self.operations.clone();
        async_fn(async move {
            for CreateOperation { resource, .. } in creates {
                operations.borrow_mut().push(operation("createFolder", &resource.path().to_string_lossy()));
            }
            Ok(Vec::new())
        })
    }
}

/// `new class extends mock<IUndoRedoService>() { pushElement }`.
struct FakeUndoRedoService {
    undo_element: Rc<RefCell<Option<UndoRedoElementRef>>>,
}

impl IUndoRedoService for FakeUndoRedoService {
    fn register_uri_comparison_key_computer(&self, _scheme: &str, _uri_comparison_key_computer: Rc<dyn UriComparisonKeyComputer>) -> DisposableRef {
        not_defined("registerUriComparisonKeyComputer")
    }

    fn get_uri_comparison_key(&self, _resource: &URI) -> String {
        not_defined("getUriComparisonKey")
    }

    fn push_element(&self, element: UndoRedoElementRef, _group: Option<&UndoRedoGroup>, _source: Option<&UndoRedoSource>) {
        *self.undo_element.borrow_mut() = Some(element);
    }

    fn get_last_element(&self, _resource: &URI) -> Option<UndoRedoElementRef> {
        not_defined("getLastElement")
    }

    fn get_elements(&self, _resource: &URI) -> PastFutureElements {
        not_defined("getElements")
    }

    fn set_elements_valid_flag(&self, _resource: &URI, _is_valid: bool, _filter: &dyn Fn(&UndoRedoElementRef) -> bool) {
        not_defined("setElementsValidFlag")
    }

    fn remove_elements(&self, _resource: &URI) {
        not_defined("removeElements")
    }

    fn create_snapshot(&self, _resource: &URI) -> ResourceEditStackSnapshot {
        not_defined("createSnapshot")
    }

    fn restore_snapshot(&self, _snapshot: &ResourceEditStackSnapshot) {
        not_defined("restoreSnapshot")
    }

    fn can_undo(&self, _target: UndoRedoTarget<'_>) -> bool {
        not_defined("canUndo")
    }

    fn undo(&self, _target: UndoRedoTarget<'_>) -> je_base::promise::MaybePromise<()> {
        not_defined("undo")
    }

    fn can_redo(&self, _target: UndoRedoTarget<'_>) -> bool {
        not_defined("canRedo")
    }

    fn redo(&self, _target: UndoRedoTarget<'_>) -> je_base::promise::MaybePromise<()> {
        not_defined("redo")
    }
}

/// `async function applyDelete(types, options?)`: o preparo roda na hora (como o JavaScript até o primeiro `await`) e o
/// futuro devolvido faz o `apply`, o `undo` e devolve as operações.
fn apply_delete(guard: &LeakGuard, types: &[FileType], options: WorkspaceFileEditOptions) -> impl Future<Output = Vec<Operation>> + 'static {
    let files: Vec<File> = types
        .iter()
        .enumerate()
        .map(|(index, file_type)| {
            let stat = FileStat {
                resource: URI::file(format!("/workspace/entry-{index}"), Platform::Posix),
                name: format!("entry-{index}"),
                size: Some(3),
                mtime: Some(0),
                ctime: Some(0),
                etag: Some(String::new()),
                readonly: Some(false),
                locked: Some(false),
                executable: Some(false),
                is_file: file_type.intersects(FileType::FILE),
                is_directory: file_type.intersects(FileType::DIRECTORY),
                is_symbolic_link: file_type.intersects(FileType::SYMBOLIC_LINK),
                children: None,
            };
            File { stat: FileStatWithMetadata::new(stat), value: VSBuffer::from_byte_array(&[index as u8, 128, 255]) }
        })
        .collect();

    let edits: Vec<ResourceFileEdit> = files
        .iter()
        .map(|file| {
            let edit_options = WorkspaceFileEditOptions {
                recursive: options.recursive.or(Some(true)),
                folder: options.folder.or(Some(file.stat.is_directory)),
                ..options.clone()
            };
            ResourceFileEdit::new(Some(file.stat.resource.clone()), None, Some(edit_options), None)
        })
        .collect();

    let operations: Operations = Rc::default();
    let file_service: Rc<dyn IFileService> = Rc::new(FakeFileService { files, operations: operations.clone() });
    let working_copy_file_service: Rc<dyn IWorkingCopyFileService> = Rc::new(FakeWorkingCopyFileService { operations: operations.clone() });
    let undo_element: Rc<RefCell<Option<UndoRedoElementRef>>> = Rc::default();
    let undo_redo_service: Rc<dyn IUndoRedoService> = Rc::new(FakeUndoRedoService { undo_element: undo_element.clone() });

    let instantiation_service = TestInstantiationService::with_options(ServiceCollection::new(), true, None, true);
    guard.add_rc(instantiation_service.clone());
    let configuration_service: Rc<TestConfigurationService> = TestConfigurationService::new(None);
    instantiation_service.stub(I_CONFIGURATION_SERVICE, configuration_service);
    instantiation_service.stub(I_FILE_SERVICE, file_service);
    instantiation_service.stub(I_LOG_SERVICE, NullLogService::shared());
    instantiation_service.stub(I_TEXT_FILE_SERVICE, Rc::new(StubTextFileService));
    instantiation_service.stub(I_UNDO_REDO_SERVICE, undo_redo_service);
    instantiation_service.stub(I_WORKING_COPY_FILE_SERVICE, working_copy_file_service);

    let bulk_file_edits = instantiation_service.create_instance::<BulkFileEdits>(BulkFileEditsArgs {
        label: "Delete".to_string(),
        code: "delete".to_string(),
        undo_redo_group: UndoRedoGroup::new(),
        undo_redo_source: None,
        confirm_before_undo: false,
        progress: Progress::<()>::none(),
        token: CancellationToken::None,
        edits,
    });

    async move {
        bulk_file_edits.apply().await.expect("apply");
        let undo_element = undo_element.borrow().clone().expect("assert.ok(undoElement)");
        await_maybe(undo_element.undo()).await.expect("undo");

        let operations = operations.borrow().clone();
        operations
    }
}

#[test]
fn delete_skips_undo_contents_for_non_regular_files() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let applied = apply_delete(&guard, &[FileType::UNKNOWN], WorkspaceFileEditOptions::default());
    run_with_faked_timers(async move {
        assert_eq!(applied.await, vec![operation("delete", "/workspace/entry-0")]);
    });
}

#[test]
fn delete_preserves_undo_contents_for_regular_files() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let applied = apply_delete(&guard, &[FileType::FILE], WorkspaceFileEditOptions::default());
    run_with_faked_timers(async move {
        assert_eq!(
            applied.await,
            vec![
                operation("read", "/workspace/entry-0"),
                operation("delete", "/workspace/entry-0"),
                create_operation("/workspace/entry-0", &[0, 128, 255]),
            ]
        );
    });
}

#[test]
fn undo_restores_only_regular_files_in_a_mixed_deletion() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let applied = apply_delete(&guard, &[FileType::FILE, FileType::UNKNOWN, FileType::FILE], WorkspaceFileEditOptions::default());
    run_with_faked_timers(async move {
        assert_eq!(
            applied.await,
            vec![
                operation("read", "/workspace/entry-0"),
                operation("read", "/workspace/entry-2"),
                operation("delete", "/workspace/entry-0"),
                operation("delete", "/workspace/entry-1"),
                operation("delete", "/workspace/entry-2"),
                create_operation("/workspace/entry-0", &[0, 128, 255]),
                create_operation("/workspace/entry-2", &[2, 128, 255]),
            ]
        );
    });
}

#[test]
fn delete_does_not_restore_non_regular_files_when_the_folder_option_is_set() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let applied = apply_delete(&guard, &[FileType::UNKNOWN], WorkspaceFileEditOptions { folder: Some(true), ..WorkspaceFileEditOptions::default() });
    run_with_faked_timers(async move {
        assert_eq!(applied.await, vec![operation("delete", "/workspace/entry-0")]);
    });
}

#[test]
fn delete_preserves_undo_for_empty_folders() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let applied = apply_delete(&guard, &[FileType::DIRECTORY], WorkspaceFileEditOptions::default());
    run_with_faked_timers(async move {
        assert_eq!(applied.await, vec![operation("delete", "/workspace/entry-0"), operation("createFolder", "/workspace/entry-0")]);
    });
}

#[test]
fn delete_skips_undo_contents_exceeding_the_size_limit() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let applied = apply_delete(&guard, &[FileType::FILE], WorkspaceFileEditOptions { max_size: Some(2.0), ..WorkspaceFileEditOptions::default() });
    run_with_faked_timers(async move {
        assert_eq!(applied.await, vec![operation("delete", "/workspace/entry-0")]);
    });
}
