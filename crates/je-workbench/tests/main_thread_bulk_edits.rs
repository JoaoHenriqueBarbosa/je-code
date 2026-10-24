//! Porte de `src/vs/workbench/api/test/browser/mainThreadBulkEdits.test.ts` (suíte `MainThreadBulkEdits`), caso a caso e na
//! mesma ordem.
//!
//! Notas de tradução:
//! - **`ensureNoDisposablesAreLeakedInTestSuite()`.** O `LeakGuard` no começo do caso.
//! - **`new class extends mock<IFileService>() { ... }`.** O dublê implementa o contrato inteiro; os dois eventos são
//!   `Event.None`, `hasProvider` é sempre verdadeiro, `hasCapability` sempre falso (sem capacidade nenhuma, e sobretudo
//!   sem diferenciar maiúsculas de minúsculas), e o membro que o original não sobrescreve lança o `TypeError` do `mock`
//!   ([`not_defined`]).
//! - **`new UriIdentityService(fileService)`.** É o `UriIdentityService::create` sobre o dublê.
//! - **`IWorkspaceTextEditDto`.** É o `IWorkspaceTextEdit` do editor (`versionId: undefined` é `None`), e o `edits` do DTO
//!   é a lista de [`WorkspaceEditEntryDto::Text`].
//! - **`<IWorkspaceTextEdit>out.edits[0]`.** É o `match` na variante [`WorkspaceEditItem::Text`].

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use std::rc::Rc;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::cancellation::CancellationToken;
use je_base::errors::{self, Error};
use je_base::event::Event;
use je_base::lifecycle::{DisposableRef, IDisposable};
use je_base::promise::Promise;
use je_base::uri::{URI, UriComponents};
use je_editor::code_action::languages::{IWorkspaceTextEdit, TextEdit, WorkspaceEditItem};
use je_editor::range::Range;
use je_instantiation::Injectable;
use je_platform::files::file_service::FileContentSource;
use je_platform::files::files::{
    CreateFileOptions, FileChangesEvent, FileContent, FileOperationEvent, FileStat, FileStatWithMetadata, FileStreamContent, FileSystemProviderCapabilities,
    FileSystemProviderCapabilitiesChangeEvent, FileSystemProviderRef, FileSystemProviderRegistrationEvent, PartialFileDeleteOptions, ReadFileOptions,
    ResolveFileOptions, WatchOptions, WriteFileOptions,
};
use je_platform::files::i_file_service::IFileService;
use je_platform::uri_identity::uri_identity::IUriIdentityService;
use je_platform::uri_identity::uri_identity_service::UriIdentityService;
use je_workbench::api::browser::main_thread_bulk_edits::revive_workspace_edit_dto;
use je_workbench::api::common::ext_host_protocol::{IWorkspaceEditDto, WorkspaceEditEntryDto};

/// O membro que o `mock<T>()` não define: chamá-lo é um `TypeError`.
fn not_defined(member: &str) -> ! {
    errors::throw(Error::type_error(format!("{member} is not a function")))
}

/// `new class extends mock<IFileService>() { onDidChangeFileSystemProviderCapabilities = Event.None; ... }`.
struct FakeFileService;

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

    fn resolve_with_metadata(&self, _resource: &URI, _options: Option<ResolveFileOptions>) -> Promise<FileStatWithMetadata> {
        not_defined("resolve")
    }

    fn exists(&self, _resource: &URI) -> Promise<bool> {
        not_defined("exists")
    }

    fn on_did_change_file_system_provider_registrations(&self) -> Event<FileSystemProviderRegistrationEvent> {
        Event::none()
    }

    fn on_did_change_file_system_provider_capabilities(&self) -> Event<FileSystemProviderCapabilitiesChangeEvent> {
        Event::none()
    }

    fn has_provider(&self, _uri: &URI) -> bool {
        true
    }

    fn has_capability(&self, _resource: &URI, _capability: FileSystemProviderCapabilities) -> bool {
        // NO capabilities, esp not being case-sensitive
        false
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

/// `URI.from({ scheme, path })`.
fn uri(scheme: &str, path: &str) -> URI {
    URI::from(&UriComponents { scheme: Some(scheme.into()), path: Some(path.into()), ..UriComponents::default() }, false)
}

/// `{ resource, textEdit: { range: { 1, 1, 1, 1 }, text: 'sss' }, versionId: undefined }`.
fn text_edit(resource: URI) -> WorkspaceEditEntryDto {
    WorkspaceEditEntryDto::Text(IWorkspaceTextEdit {
        resource,
        text_edit: TextEdit { range: Range::new(1, 1, 1, 1), text: "sss".to_string(), eol: None, insert_as_snippet: None, keep_whitespace: None },
        version_id: None,
        metadata: None,
    })
}

/// `(<IWorkspaceTextEdit>edit).resource.path`.
fn resource_path(edit: &WorkspaceEditItem) -> String {
    match edit {
        WorkspaceEditItem::Text(edit) => edit.resource.path().to_string_lossy(),
        _ => panic!("a edição não é de texto"),
    }
}

#[test]
fn rename_failed_to_apply_edits_in_monorepo_with_pnpm_158845() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();

    let file_service: Rc<dyn IFileService> = Rc::new(FakeFileService);

    let uri_identity_service = UriIdentityService::create((), (file_service,));
    let uri_identity_contract: Rc<dyn IUriIdentityService> = uri_identity_service.clone();

    let edits = vec![
        text_edit(uri("case", "/hello/WORLD/foo.txt")),
        text_edit(uri("case", "/heLLO/world/fOO.txt")),
        text_edit(uri("case", "/other/path.txt")),
        text_edit(uri("foo", "/other/path.txt")),
    ];

    let out = revive_workspace_edit_dto(&IWorkspaceEditDto { edits }, &uri_identity_contract, None).expect("reviveWorkspaceEditDto");

    assert_eq!(resource_path(&out.edits[0]), "/hello/WORLD/foo.txt");
    assert_eq!(resource_path(&out.edits[1]), "/hello/WORLD/foo.txt"); // the FIRST occurrence defined the shape!
    assert_eq!(resource_path(&out.edits[2]), "/other/path.txt");
    assert_eq!(resource_path(&out.edits[3]), "/other/path.txt");

    uri_identity_service.dispose();
    drop(guard);
}
