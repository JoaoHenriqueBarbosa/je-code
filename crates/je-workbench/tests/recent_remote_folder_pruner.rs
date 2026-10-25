//! Porte de `src/vs/workbench/contrib/workspaces/test/browser/recentRemoteFolderPruner.test.ts` (suíte
//! `pruneRecentRemoteFolderIfMissing`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` vira o guarda criado no começo do caso; `ds.add(new StubFileService())` é o
//!   `add_rc` do `TestFileService` embrulhado.
//! - `class StubFileService extends TestFileService` é [`StubFileService`]: o `TestFileService` do porte não é herdável, então a
//!   struct o embrulha e repassa cada método do `IFileService`, sobrescrevendo só o `stat` (que registra a chamada e lança o
//!   `statError`, se houver; sem erro, devolve o `super.stat`).
//! - `Pick<IWorkspacesService, 'removeRecentlyOpened'>` com `as IWorkspacesService` é [`RecordingWorkspacesService`]: só o
//!   `remove_recently_opened` funciona e os demais lançam, como um objeto sem esses métodos.
//! - `new FileOperationError(msg, result)` é `FileOperationError::new(..).into_error()` e `new Error(msg)` é `Error::new`.
//! - `URI.file` é `uri_file` e `URI.parse(x)` é `URI::parse(x, false)`; `u.toString()` é `to_string(false)`.
//! - `assert.deepStrictEqual` sobre os objetos é a comparação das tuplas `(stats, removed)` na mesma ordem das chaves.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use std::cell::RefCell;
use std::rc::Rc;

use common::{ensure_no_disposables_are_leaked_in_test_suite, run_with_faked_timers};
use je_base::cancellation::CancellationToken;
use je_base::errors::{self, Error};
use je_base::event::Event;
use je_base::lifecycle::DisposableRef;
use je_base::promise::Promise;
use je_base::uri::URI;
use je_platform::files::file_service::FileContentSource;
use je_platform::files::files::{
    CreateFileOptions, FileChangesEvent, FileContent, FileOperationError, FileOperationEvent, FileOperationResult, FileStat, FileStatWithMetadata, FileStreamContent,
    FileSystemProviderActivationEvent, FileSystemProviderCapabilities, FileSystemProviderCapabilitiesChangeEvent, FileSystemProviderRef, FileSystemProviderRegistrationEvent,
    PartialFileDeleteOptions, ReadFileOptions, ResolveFileOptions, WatchOptions, WriteFileOptions,
};
use je_platform::files::i_file_service::IFileService;
use je_platform::resources::uri_file;
use je_platform::workspace::{IWorkspaceContextService, test_workspace};
use je_platform::workspaces::{
    EnterWorkspaceResultCell, IWorkspacesService, Recent, RecentlyOpenedCell, WorkspaceFolderCreationData, WorkspaceIdentifier, WorkspaceIdentifierCell,
};
use je_platform::backup::BackupInfo;
use je_workbench::contrib::workspaces::recent_remote_folder_pruner::prune_recent_remote_folder_if_missing;
use je_workbench::test::common::workbench_test_services::{TestContextService, TestFileService};

/// `class StubFileService extends TestFileService`.
struct StubFileService {
    inner: Rc<TestFileService>,
    stat_error: RefCell<Option<Error>>,
    stat_calls: RefCell<Vec<URI>>,
}

impl IFileService for StubFileService {
    fn on_did_files_change(&self) -> Event<Rc<FileChangesEvent>> {
        IFileService::on_did_files_change(&*self.inner)
    }

    fn get_provider(&self, scheme: &str) -> Option<FileSystemProviderRef> {
        IFileService::get_provider(&*self.inner, scheme)
    }

    fn resolve_with_metadata(&self, resource: &URI, options: Option<ResolveFileOptions>) -> Promise<FileStatWithMetadata> {
        IFileService::resolve_with_metadata(&*self.inner, resource, options)
    }

    /// `override async stat(resource)`.
    fn stat(&self, resource: &URI) -> Promise<FileStatWithMetadata> {
        self.stat_calls.borrow_mut().push(resource.clone());
        if let Some(error) = self.stat_error.borrow().clone() {
            return Promise::rejected(error);
        }
        IFileService::stat(&*self.inner, resource)
    }

    fn exists(&self, resource: &URI) -> Promise<bool> {
        IFileService::exists(&*self.inner, resource)
    }

    fn on_did_change_file_system_provider_registrations(&self) -> Event<FileSystemProviderRegistrationEvent> {
        IFileService::on_did_change_file_system_provider_registrations(&*self.inner)
    }

    fn on_did_change_file_system_provider_capabilities(&self) -> Event<FileSystemProviderCapabilitiesChangeEvent> {
        IFileService::on_did_change_file_system_provider_capabilities(&*self.inner)
    }

    fn has_provider(&self, resource: &URI) -> bool {
        IFileService::has_provider(&*self.inner, resource)
    }

    fn has_capability(&self, resource: &URI, capability: FileSystemProviderCapabilities) -> bool {
        IFileService::has_capability(&*self.inner, resource, capability)
    }

    fn read_file(&self, resource: &URI, options: Option<ReadFileOptions>, token: Option<CancellationToken>) -> Promise<FileContent> {
        IFileService::read_file(&*self.inner, resource, options, token)
    }

    fn resolve(&self, resource: &URI, options: Option<ResolveFileOptions>) -> Promise<FileStat> {
        IFileService::resolve(&*self.inner, resource, options)
    }

    fn create_folder(&self, resource: &URI) -> Promise<FileStatWithMetadata> {
        IFileService::create_folder(&*self.inner, resource)
    }

    fn watch(&self, resource: &URI, options: Option<WatchOptions>) -> DisposableRef {
        IFileService::watch(&*self.inner, resource, options)
    }

    fn read_file_stream(&self, resource: &URI, options: Option<ReadFileOptions>, token: Option<CancellationToken>) -> Promise<FileStreamContent> {
        IFileService::read_file_stream(&*self.inner, resource, options, token)
    }

    fn write_file(&self, resource: &URI, content: FileContentSource, options: Option<WriteFileOptions>) -> Promise<FileStatWithMetadata> {
        IFileService::write_file(&*self.inner, resource, content, options)
    }

    fn del(&self, resource: &URI, options: Option<PartialFileDeleteOptions>) -> Promise<()> {
        IFileService::del(&*self.inner, resource, options)
    }

    fn on_did_run_operation(&self) -> Event<FileOperationEvent> {
        IFileService::on_did_run_operation(&*self.inner)
    }

    fn create_file(&self, resource: &URI, content: Option<FileContentSource>, options: Option<CreateFileOptions>) -> Promise<FileStatWithMetadata> {
        IFileService::create_file(&*self.inner, resource, content, options)
    }

    fn on_will_activate_file_system_provider(&self) -> Event<FileSystemProviderActivationEvent> {
        IFileService::on_will_activate_file_system_provider(&*self.inner)
    }

    fn register_provider(&self, scheme: &str, provider: FileSystemProviderRef) -> Result<DisposableRef, Error> {
        IFileService::register_provider(&*self.inner, scheme, provider)
    }
}

/// `Pick<IWorkspacesService, 'removeRecentlyOpened'>`: o que o caso grava em `removed`.
struct RecordingWorkspacesService {
    removed: Rc<RefCell<Vec<Vec<URI>>>>,
}

impl RecordingWorkspacesService {
    fn not_mocked(method: &str) -> ! {
        errors::throw(Error::type_error(format!("this.workspacesService.{method} is not a function")))
    }
}

impl IWorkspacesService for RecordingWorkspacesService {
    fn enter_workspace(&self, _workspace_uri: &URI) -> Promise<Option<EnterWorkspaceResultCell>> {
        Self::not_mocked("enterWorkspace")
    }

    fn create_untitled_workspace(&self, _folders: Vec<WorkspaceFolderCreationData>, _remote_authority: Option<String>) -> Promise<WorkspaceIdentifierCell> {
        Self::not_mocked("createUntitledWorkspace")
    }

    fn delete_untitled_workspace(&self, _workspace: &WorkspaceIdentifier) -> Promise<()> {
        Self::not_mocked("deleteUntitledWorkspace")
    }

    fn get_workspace_identifier(&self, _workspace_uri: &URI) -> Promise<WorkspaceIdentifierCell> {
        Self::not_mocked("getWorkspaceIdentifier")
    }

    fn on_did_change_recently_opened(&self) -> Event<()> {
        Self::not_mocked("onDidChangeRecentlyOpened")
    }

    fn add_recently_opened(&self, _recents: Vec<Recent>) -> Promise<()> {
        Self::not_mocked("addRecentlyOpened")
    }

    /// `async removeRecentlyOpened(paths) { removed.push(paths); }`.
    fn remove_recently_opened(&self, workspaces: Vec<URI>) -> Promise<()> {
        self.removed.borrow_mut().push(workspaces);
        Promise::resolved(())
    }

    fn clear_recently_opened(&self) -> Promise<()> {
        Self::not_mocked("clearRecentlyOpened")
    }

    fn get_recently_opened(&self) -> Promise<RecentlyOpenedCell> {
        Self::not_mocked("getRecentlyOpened")
    }

    fn get_dirty_workspaces(&self) -> Promise<Vec<BackupInfo>> {
        Self::not_mocked("getDirtyWorkspaces")
    }
}

/// O que o caso confere de cada execução: as chamadas de `stat` (textos) e as listas removidas (textos).
struct Outcome {
    stats: Vec<String>,
    removed: Vec<Vec<String>>,
}

fn texts(uri: &URI) -> String {
    uri.to_string(false).to_string_lossy()
}

fn summary(outcome: &Outcome) -> (usize, usize) {
    (outcome.stats.len(), outcome.removed.len())
}

/// `async function run(options: { folder: URI; statError?: Error })`.
async fn run(store: &Rc<je_base::lifecycle::DisposableStore>, folder: &URI, stat_error: Option<Error>) -> Outcome {
    let removed: Rc<RefCell<Vec<Vec<URI>>>> = Rc::default();

    let context_service: Rc<dyn IWorkspaceContextService> = TestContextService::new(Some(test_workspace(std::slice::from_ref(folder))), None);
    let inner = store.add_rc(TestFileService::new());
    let file_service = Rc::new(StubFileService { inner, stat_error: RefCell::new(stat_error), stat_calls: RefCell::default() });

    let workspaces_service: Rc<dyn IWorkspacesService> = Rc::new(RecordingWorkspacesService { removed: removed.clone() });

    prune_recent_remote_folder_if_missing(context_service.as_ref(), file_service.as_ref(), workspaces_service).await.unwrap();

    let stats = file_service.stat_calls.borrow().iter().map(texts).collect();
    let removed = removed.borrow().iter().map(|paths| paths.iter().map(texts).collect()).collect();
    Outcome { stats, removed }
}

#[test]
fn prunes_vscode_remote_on_file_not_found_preserves_entry_on_transient_errors_generic_errors_for_local_uris_and_for_non_remote_authorities() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let store = guard.store();

    run_with_faked_timers(async move {
        let remote_folder = URI::parse("vscode-remote://wsl%2BUbuntu/home/user/ghost-test", false);
        let virtual_folder = URI::parse("vscode-vfs://github/microsoft/vscode", false);
        let local_folder = uri_file("/tmp/local-folder");

        let not_found = run(&store, &remote_folder, Some(FileOperationError::new("gone", FileOperationResult::FileNotFound).into_error())).await;
        let transient = run(&store, &remote_folder, Some(FileOperationError::new("host unreachable", FileOperationResult::FileOtherError).into_error())).await;
        let generic_error = run(&store, &remote_folder, Some(Error::new("No file system provider found for resource"))).await;
        let local_uri = run(&store, &local_folder, Some(FileOperationError::new("should never run", FileOperationResult::FileNotFound).into_error())).await;
        let virtual_uri = run(&store, &virtual_folder, Some(FileOperationError::new("should never run", FileOperationResult::FileNotFound).into_error())).await;

        assert_eq!((not_found.stats, not_found.removed), (vec![texts(&remote_folder)], vec![vec![texts(&remote_folder)]]));
        assert_eq!(summary(&transient), (1, 0));
        assert_eq!(summary(&generic_error), (1, 0));
        assert_eq!(summary(&local_uri), (0, 0));
        assert_eq!(summary(&virtual_uri), (0, 0));
    });
}
