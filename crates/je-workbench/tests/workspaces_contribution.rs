//! Porte de `src/vs/workbench/contrib/workspaces/test/browser/workspaces.contribution.test.ts` (suíte
//! `WorkspacesFinderContribution`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` vira o guarda criado no começo de cada caso. `TestContextService`,
//!   `NotificationService` e `InMemoryStorageService` (o `TestStorageService` não é usado aqui) são os do porte.
//! - O laço `for (const multiple of [false, true])` vira dois `#[test]` sobre [`run_case`], com o nome do original e o valor
//!   de `multiple` no sufixo.
//! - `new class extends mock<IFileService>() { override async resolve }` é [`ResolvingFileService`]: os demais métodos do
//!   `IFileService` lançam, como no `mock`. O `resolve` devolve o `FileStat` do `createFileStat` com os arquivos como filhos.
//! - `new class extends mock<IQuickInputService>() { }` é o `TestQuickInputService` do workbench (nenhum caso o usa).
//! - `new class extends mock<IHostService>() { override async openWindow }` é [`RecordingHostService`], que marca `opened`.
//! - `Event.toPromise(onDidChangeNotification)` é `.to_promise(None)` e `await` é `.await`.
//! - O `toDisposable` que fecha as notificações é registrado no guarda; `notification.message.linkedText.toString()` é
//!   `to_js_string()` e os nós que não são texto são os `LinkedTextNode::Link`.
//! - `assert.deepStrictEqual({ text, links, actions }, ...)` confere os três campos, na ordem do original.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use std::cell::Cell;
use std::rc::Rc;

use common::{ensure_no_disposables_are_leaked_in_test_suite, run_with_faked_timers};
use je_base::JsString;
use je_base::cancellation::CancellationToken;
use je_base::errors::{self, Error};
use je_base::event::Event;
use je_base::lifecycle::{DisposableRef, to_disposable};
use je_base::linked_text::{Link, LinkedTextNode};
use je_base::promise::Promise;
use je_base::uri::URI;
use je_platform::files::file_service::FileContentSource;
use je_platform::files::files::{
    CreateFileOptions, FileChangesEvent, FileContent, FileOperationEvent, FileStat, FileStatWithMetadata, FileStreamContent, FileSystemProviderActivationEvent,
    FileSystemProviderCapabilities, FileSystemProviderCapabilitiesChangeEvent, FileSystemProviderRef, FileSystemProviderRegistrationEvent, PartialFileDeleteOptions,
    ReadFileOptions, ResolveFileOptions, WatchOptions, WriteFileOptions,
};
use je_platform::files::i_file_service::IFileService;
use je_platform::notification::INotificationService;
use je_platform::quickinput::quick_input::IQuickInputService;
use je_platform::resources::{join_path, uri_file};
use je_platform::storage::{InMemoryStorageService, StorageService};
use je_platform::workspace::{IWorkspaceContextService, test_workspace};
use je_workbench::common::notifications::INotificationsModel;
use je_workbench::contrib::workspaces::WorkspacesFinderContribution;
use je_workbench::services::host::host::{IHostService, OpenWindowOptions, ReloadOptions, WindowOpenable};
use je_workbench::services::notification::common::notification_service::NotificationService;
use je_workbench::test::browser::workbench_test_services::TestQuickInputService;
use je_workbench::test::common::workbench_test_services::{FileStatChild, FileStatOptions, TestContextService, create_file_stat};

const FILENAME: &str = "project-[Open](command:unexpected).code-workspace";

/// `mock<IFileService>()` com `resolve` sobrescrito.
struct ResolvingFileService {
    folder: URI,
    files: Vec<String>,
}

impl ResolvingFileService {
    fn not_mocked(method: &str) -> ! {
        errors::throw(Error::type_error(format!("this.fileService.{method} is not a function")))
    }
}

impl IFileService for ResolvingFileService {
    fn on_did_files_change(&self) -> Event<Rc<FileChangesEvent>> {
        Self::not_mocked("onDidFilesChange")
    }

    fn get_provider(&self, _scheme: &str) -> Option<FileSystemProviderRef> {
        Self::not_mocked("getProvider")
    }

    fn resolve_with_metadata(&self, _resource: &URI, _options: Option<ResolveFileOptions>) -> Promise<FileStatWithMetadata> {
        Self::not_mocked("resolve")
    }

    fn exists(&self, _resource: &URI) -> Promise<bool> {
        Self::not_mocked("exists")
    }

    fn on_did_change_file_system_provider_registrations(&self) -> Event<FileSystemProviderRegistrationEvent> {
        Self::not_mocked("onDidChangeFileSystemProviderRegistrations")
    }

    fn on_did_change_file_system_provider_capabilities(&self) -> Event<FileSystemProviderCapabilitiesChangeEvent> {
        Self::not_mocked("onDidChangeFileSystemProviderCapabilities")
    }

    fn has_provider(&self, _resource: &URI) -> bool {
        Self::not_mocked("hasProvider")
    }

    fn has_capability(&self, _resource: &URI, _capability: FileSystemProviderCapabilities) -> bool {
        Self::not_mocked("hasCapability")
    }

    fn read_file(&self, _resource: &URI, _options: Option<ReadFileOptions>, _token: Option<CancellationToken>) -> Promise<FileContent> {
        Self::not_mocked("readFile")
    }

    /// `override async resolve(resource)`.
    fn resolve(&self, resource: &URI, _options: Option<ResolveFileOptions>) -> Promise<FileStat> {
        let children = self.files.iter().map(|name| FileStatChild { resource: join_path(&self.folder, &[name]), is_file: None, is_directory: None, is_symbolic_link: None, executable: None }).collect();
        let options = FileStatOptions { readonly: false, is_file: Some(false), is_directory: Some(true), is_symbolic_link: Some(false), children: Some(children), executable: None };
        Promise::resolved(create_file_stat(resource, &options).into_stat())
    }

    fn create_folder(&self, _resource: &URI) -> Promise<FileStatWithMetadata> {
        Self::not_mocked("createFolder")
    }

    fn watch(&self, _resource: &URI, _options: Option<WatchOptions>) -> DisposableRef {
        Self::not_mocked("watch")
    }

    fn read_file_stream(&self, _resource: &URI, _options: Option<ReadFileOptions>, _token: Option<CancellationToken>) -> Promise<FileStreamContent> {
        Self::not_mocked("readFileStream")
    }

    fn write_file(&self, _resource: &URI, _content: FileContentSource, _options: Option<WriteFileOptions>) -> Promise<FileStatWithMetadata> {
        Self::not_mocked("writeFile")
    }

    fn del(&self, _resource: &URI, _options: Option<PartialFileDeleteOptions>) -> Promise<()> {
        Self::not_mocked("del")
    }

    fn on_did_run_operation(&self) -> Event<FileOperationEvent> {
        Self::not_mocked("onDidRunOperation")
    }

    fn create_file(&self, _resource: &URI, _content: Option<FileContentSource>, _options: Option<CreateFileOptions>) -> Promise<FileStatWithMetadata> {
        Self::not_mocked("createFile")
    }

    fn on_will_activate_file_system_provider(&self) -> Event<FileSystemProviderActivationEvent> {
        Self::not_mocked("onWillActivateFileSystemProvider")
    }

    fn register_provider(&self, _scheme: &str, _provider: FileSystemProviderRef) -> Result<DisposableRef, Error> {
        Self::not_mocked("registerProvider")
    }
}

/// `mock<IHostService>()` com `openWindow` sobrescrito.
struct RecordingHostService {
    opened: Rc<Cell<bool>>,
}

impl IHostService for RecordingHostService {
    /// `override async openWindow(): Promise<void> { opened = true; }`.
    fn open_window(&self, _to_open: Vec<WindowOpenable>, _options: Option<OpenWindowOptions>) -> Promise<()> {
        self.opened.set(true);
        Promise::resolved(())
    }

    fn reload(&self, _options: Option<ReloadOptions>) -> Promise<()> {
        errors::throw(Error::type_error("this.hostService.reload is not a function"))
    }

    fn restart(&self) -> Promise<()> {
        errors::throw(Error::type_error("this.hostService.restart is not a function"))
    }
}

fn run_case(multiple: bool) {
    let store = ensure_no_disposables_are_leaked_in_test_suite();

    let folder = uri_file("/workspace");
    let files: Vec<String> = if multiple { vec![FILENAME.to_string(), "second.code-workspace".to_string()] } else { vec![FILENAME.to_string()] };
    let storage_service = store.add_rc(InMemoryStorageService::new());
    let storage: Rc<dyn StorageService> = Rc::new(storage_service.clone());
    let notification_service = store.add_rc(NotificationService::new(storage.clone()));
    {
        let notification_service = notification_service.clone();
        store.add(to_disposable(move || {
            for notification in notification_service.model.notifications() {
                notification.close();
            }
        }));
    }
    let opened = Rc::new(Cell::new(false));
    let added = notification_service.model.on_did_change_notification().to_promise(None);

    let context_service: Rc<dyn IWorkspaceContextService> = TestContextService::new(Some(test_workspace(std::slice::from_ref(&folder))), None);
    let notification_dyn: Rc<dyn INotificationService> = notification_service.clone();
    let file_service: Rc<dyn IFileService> = Rc::new(ResolvingFileService { folder: folder.clone(), files });
    let quick_input_service: Rc<dyn IQuickInputService> = TestQuickInputService::new();
    let host_service: Rc<dyn IHostService> = Rc::new(RecordingHostService { opened: opened.clone() });
    store.add_rc(WorkspacesFinderContribution::new(context_service, notification_dyn, file_service, quick_input_service, host_service, storage));

    run_with_faked_timers(async move {
        added.await.unwrap();
        let notification = notification_service.model.notifications()[0].clone();

        let message = notification.message();
        let links: Vec<LinkedTextNode> = message.linked_text.nodes.iter().filter(|node| !matches!(node, LinkedTextNode::Text(_))).cloned().collect();
        let actions = notification.actions();
        let labels: Vec<String> = actions.primary.iter().map(|action| action.label()).collect();

        assert_eq!(
            message.linked_text.to_js_string().to_string_lossy(),
            if multiple {
                "This folder contains multiple workspace files. Do you want to open one? Learn more about workspace files.".to_string()
            } else {
                format!("This folder contains a workspace file '{FILENAME}'. Do you want to open it? Learn more about workspace files.")
            }
        );
        assert_eq!(
            links,
            vec![LinkedTextNode::Link(Link {
                label: JsString::from("Learn more"),
                href: JsString::from("https://go.microsoft.com/fwlink/?linkid=2025315"),
                title: None
            })]
        );
        assert_eq!(labels, vec![if multiple { "Select Workspace" } else { "Open Workspace" }.to_string()]);

        if !multiple {
            actions.primary[0].run(&[]).await.unwrap();
            assert!(opened.get());
        }
    });
}

#[test]
fn preserves_the_documentation_link_and_literal_workspace_names_multiple_false() {
    run_case(false);
}

#[test]
fn preserves_the_documentation_link_and_literal_workspace_names_multiple_true() {
    run_case(true);
}
