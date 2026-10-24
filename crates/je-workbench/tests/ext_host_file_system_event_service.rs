//! Porte de `src/vs/workbench/api/test/browser/extHostFileSystemEventService.test.ts` (suíte
//! `ExtHostFileSystemEventService`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` vira o guarda criado em [`run_test`]; os casos são síncronos.
//! - `protocol` (cujo `getProxy` devolve `undefined!`) é [`NoProxyContext`]: pedir um proxy falha com `panic!`, como o
//!   uso do `undefined` lançaria `TypeError`. `protocolWithProxy` é [`ProxyContext`], com o proxy
//!   `{ $watch() { }, $unwatch() { }, dispose() { } }` ([`NoopFileSystemEventProxy`]).
//! - O `undefined!` dos argumentos que o caso nunca lê (`workspace`, `configProvider`, a extensão e o
//!   `ExtHostDocumentsAndEditors`) é [`UndefinedWorkspace`] e [`UndefinedConfiguration`], que falham ao serem lidos, a
//!   extensão nula e um `ExtHostDocumentsAndEditors` sem proxies.
//! - `Pick<IExtHostWorkspace, 'getWorkspaceFolder'>` é [`WorkspaceStub`]; `{ getConfiguration: () => ({ get: () => ({}) }) }`
//!   é [`EmptyConfiguration`].
//! - `fileSystemInfo.$acceptProviderInfos(uri, capabilities)` é `rpc_accept_provider_infos`, e
//!   `service.$onFileEvent(...)` é `rpc_on_file_event`.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use std::any::Any;
use std::cell::RefCell;
use std::rc::Rc;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::dynamic::Dynamic;
use je_base::lifecycle::IDisposable;
use je_base::uri::URI;
use je_platform::files::files::{FileSystemProviderCapabilities, WatchOptions};
use je_platform::log::NullLogService;
use je_platform::resources::uri_file;
use je_workbench::api::common::ext_host_documents_and_editors::ExtHostDocumentsAndEditors;
use je_workbench::api::common::ext_host_file_system_event_service::{
    ExtHostFileSystemEventService, FileSystemWatcherCreateOptions, FileWatcherConfiguration, FileWatcherWorkspace,
};
use je_workbench::api::common::ext_host_file_system_info::ExtHostFileSystemInfo;
use je_workbench::api::common::ext_host_protocol::{
    ExtHostFileSystemEventServiceShape, ExtHostFileSystemInfoShape, FileSystemEvents, IMainContext, MainThreadFileSystemEventServiceShape,
};
use je_workbench::api::common::ext_host_workspace::RelativePattern;
use je_workbench::api::common::ext_host_workspace::{GlobPattern, WorkspaceFolder};
use je_workbench::services::extensions::extensions::null_extension_description;

// ---- os dublês --------------------------------------------------------------------------------------

/// `const protocol: IMainContext = { getProxy: () => undefined!, ... }`.
struct NoProxyContext;

impl IMainContext for NoProxyContext {
    fn get_proxy_erased(&self, sid: &'static str) -> Rc<dyn Any> {
        panic!("Cannot read properties of undefined (proxy '{sid}')")
    }

    fn set_erased(&self, _sid: &'static str, _value: Rc<dyn Any>) {}
}

/// `({ $watch() { }, $unwatch() { }, dispose() { } })`.
struct NoopFileSystemEventProxy;

impl IDisposable for NoopFileSystemEventProxy {
    fn dispose(&self) {}
}

impl MainThreadFileSystemEventServiceShape for NoopFileSystemEventProxy {
    fn rpc_watch(&self, _extension_id: &str, _session: f64, _resource: &URI, _opts: WatchOptions, _correlate: bool) {}

    fn rpc_unwatch(&self, _session: f64) {}
}

/// `const protocolWithProxy: IMainContext = { getProxy: () => ({ $watch() { }, $unwatch() { }, dispose() { } }) as never, ... }`.
struct ProxyContext {
    proxy: Rc<dyn MainThreadFileSystemEventServiceShape>,
}

impl ProxyContext {
    fn new() -> ProxyContext {
        ProxyContext { proxy: Rc::new(NoopFileSystemEventProxy) }
    }
}

impl IMainContext for ProxyContext {
    fn get_proxy_erased(&self, sid: &'static str) -> Rc<dyn Any> {
        match sid {
            "MainThreadFileSystemEventService" => Rc::new(self.proxy.clone()),
            other => panic!("proxy inesperado: {other}"),
        }
    }

    fn set_erased(&self, _sid: &'static str, _value: Rc<dyn Any>) {}
}

/// O `undefined!` de `workspace`: nenhuma leitura é esperada.
struct UndefinedWorkspace;

impl FileWatcherWorkspace for UndefinedWorkspace {
    fn get_workspace_folder(&self, _uri: &URI) -> Option<Rc<WorkspaceFolder>> {
        panic!("Cannot read properties of undefined (reading 'getWorkspaceFolder')")
    }
}

/// O `undefined!` de `configProvider`: nenhuma leitura é esperada.
struct UndefinedConfiguration;

impl FileWatcherConfiguration for UndefinedConfiguration {
    fn get_files_watcher_exclude(&self, _workspace_folder: Option<&Rc<WorkspaceFolder>>) -> Dynamic {
        panic!("Cannot read properties of undefined (reading 'getConfiguration')")
    }
}

/// `{ getWorkspaceFolder: () => ({ uri: URI.file('/workspace'), name: 'test', index: 0 }) }`.
struct WorkspaceStub;

impl FileWatcherWorkspace for WorkspaceStub {
    fn get_workspace_folder(&self, _uri: &URI) -> Option<Rc<WorkspaceFolder>> {
        Some(WorkspaceFolder::new(uri_file("/workspace"), "test", 0))
    }
}

/// `{ getConfiguration: () => ({ get: () => ({}) }) }`.
struct EmptyConfiguration;

impl FileWatcherConfiguration for EmptyConfiguration {
    fn get_files_watcher_exclude(&self, _workspace_folder: Option<&Rc<WorkspaceFolder>>) -> Dynamic {
        Dynamic::object(Vec::<(&str, Dynamic)>::new())
    }
}

/// `new ExtHostFileSystemEventService(protocol, new NullLogService(), undefined!)`: o `ExtHostDocumentsAndEditors` do
/// original é `undefined!`, e nenhum caso chega a lê-lo.
fn new_service(context: Rc<dyn IMainContext>) -> Rc<ExtHostFileSystemEventService> {
    let documents = Rc::new(ExtHostDocumentsAndEditors::new(Rc::new(NoProxyContext), NullLogService::shared()));
    ExtHostFileSystemEventService::new(context, NullLogService::shared(), documents)
}

/// `{ session: undefined, created: [...], changed: [], deleted: [] }`.
fn created_events(paths: &[&str]) -> FileSystemEvents {
    FileSystemEvents { session: None, created: paths.iter().map(|path| uri_file(path)).collect(), changed: Vec::new(), deleted: Vec::new() }
}

/// O corpo de um caso: o `setup` do `ensureNoDisposablesAreLeakedInTestSuite` e o `teardown`.
fn run_test(body: impl FnOnce()) {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    body();
}

fn collect_created(watcher: &je_workbench::api::common::ext_host_file_system_event_service::FileSystemWatcher) -> (Rc<RefCell<Vec<URI>>>, je_base::lifecycle::DisposableRef) {
    let created: Rc<RefCell<Vec<URI>>> = Rc::new(RefCell::new(Vec::new()));
    let sink = created.clone();
    let subscription = watcher.on_did_create().listen(move |uri: &URI| sink.borrow_mut().push(uri.clone()));
    (created, subscription)
}

// ---- os casos --------------------------------------------------------------------------------------

#[test]
fn file_system_watcher_ignore_events_properties_are_reversed_26851() {
    run_test(|| {
        let file_system_info = ExtHostFileSystemInfo::new();
        let extension = null_extension_description();
        let workspace: Rc<dyn FileWatcherWorkspace> = Rc::new(UndefinedWorkspace);

        let watcher1 = new_service(Rc::new(NoProxyContext)).create_file_system_watcher(
            workspace.clone(),
            &UndefinedConfiguration,
            &file_system_info,
            &extension,
            &GlobPattern::from("**/somethingInteresting"),
            FileSystemWatcherCreateOptions::default(),
        );
        assert!(!watcher1.ignore_change_events());
        assert!(!watcher1.ignore_create_events());
        assert!(!watcher1.ignore_delete_events());
        watcher1.dispose();

        let watcher2 = new_service(Rc::new(NoProxyContext)).create_file_system_watcher(
            workspace,
            &UndefinedConfiguration,
            &file_system_info,
            &extension,
            &GlobPattern::from("**/somethingBoring"),
            FileSystemWatcherCreateOptions { ignore_create_events: Some(true), ignore_change_events: Some(true), ignore_delete_events: Some(true) },
        );
        assert!(watcher2.ignore_change_events());
        assert!(watcher2.ignore_create_events());
        assert!(watcher2.ignore_delete_events());
        watcher2.dispose();
    });
}

#[test]
fn file_system_watcher_matches_case_insensitively_via_pre_lowercasing() {
    run_test(|| {
        let file_system_info = ExtHostFileSystemInfo::new();
        // Default: no PathCaseSensitive capability → ignoreCase=true for string patterns

        let workspace: Rc<dyn FileWatcherWorkspace> = Rc::new(WorkspaceStub);

        let service = new_service(Rc::new(NoProxyContext));
        let watcher = service.create_file_system_watcher(
            workspace,
            &UndefinedConfiguration,
            &file_system_info,
            &null_extension_description(),
            &GlobPattern::from("**/*.TXT"),
            FileSystemWatcherCreateOptions::default(),
        );

        let (created, subscription) = collect_created(&watcher);

        // lowercase path should match uppercase pattern on case-insensitive fs
        service.rpc_on_file_event(created_events(&["/workspace/file.txt"]));

        assert_eq!(created.borrow().len(), 1);

        subscription.dispose();
        watcher.dispose();
    });
}

#[test]
fn file_system_watcher_matches_case_sensitively_when_path_case_sensitive() {
    run_test(|| {
        let file_system_info = ExtHostFileSystemInfo::new();
        file_system_info.rpc_accept_provider_infos(&uri_file("/"), Some(FileSystemProviderCapabilities::PATH_CASE_SENSITIVE));

        let workspace: Rc<dyn FileWatcherWorkspace> = Rc::new(WorkspaceStub);

        let service = new_service(Rc::new(NoProxyContext));
        let watcher = service.create_file_system_watcher(
            workspace,
            &UndefinedConfiguration,
            &file_system_info,
            &null_extension_description(),
            &GlobPattern::from("**/*.TXT"),
            FileSystemWatcherCreateOptions::default(),
        );

        let (created, subscription) = collect_created(&watcher);

        // lowercase path should NOT match uppercase pattern on case-sensitive fs
        service.rpc_on_file_event(created_events(&["/workspace/file.txt"]));

        assert_eq!(created.borrow().len(), 0);

        // uppercase path SHOULD match
        service.rpc_on_file_event(created_events(&["/workspace/file.TXT"]));

        assert_eq!(created.borrow().len(), 1);

        subscription.dispose();
        watcher.dispose();
    });
}

#[test]
fn file_system_watcher_matches_relative_pattern_case_insensitively_via_pre_lowercasing() {
    run_test(|| {
        let file_system_info = ExtHostFileSystemInfo::new();
        file_system_info.rpc_accept_provider_infos(&uri_file("/"), Some(FileSystemProviderCapabilities::FILE_READ_WRITE)); // no PathCaseSensitive → ignoreCase=true

        let workspace: Rc<dyn FileWatcherWorkspace> = Rc::new(WorkspaceStub);

        let service = new_service(Rc::new(ProxyContext::new()));
        let watcher = service.create_file_system_watcher(
            workspace,
            &EmptyConfiguration,
            &file_system_info,
            &null_extension_description(),
            &GlobPattern::from(RelativePattern::new("/Workspace", "**/*.TXT")),
            FileSystemWatcherCreateOptions::default(),
        );

        let (created, subscription) = collect_created(&watcher);

        // lowercase path should match mixed-case base + uppercase extension on case-insensitive fs
        service.rpc_on_file_event(created_events(&["/workspace/file.txt"]));

        assert_eq!(created.borrow().len(), 1);

        subscription.dispose();
        watcher.dispose();
    });
}
