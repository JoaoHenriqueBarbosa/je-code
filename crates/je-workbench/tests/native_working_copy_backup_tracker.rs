//! Porte de `src/vs/workbench/services/workingCopy/test/electron-browser/workingCopyBackupTracker.test.ts` (suíte `WorkingCopyBackupTracker (native)`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - **`EditorPart` e `EditorService`.** Os casos que abrem arquivos usam o `EditorService` real sobre uma `EditorPart` lógica,
//!   montados pelo [`EditorServiceAccessor`] (`workbenchInstantiationService` mais `createEditorPart`; as divergências estão no
//!   cabeçalho de `support/editor_service_support.rs`). O `registerTestFileEditor()` do `setup` é [`register_test_file_editor`]:
//!   o painel lógico de teste para a `FileEditorInput`, que resolve a entrada (e com isso cria o modelo do arquivo).
//! - **`createTracker`.** O original cria um segundo `workbenchInstantiationService` com um `TestFilesConfigurationService` sobre
//!   um `TestConfigurationService` com `files.autoSave` (`off` ou `afterDelay` com `autoSaveDelay: 1`); só o rastreador e o
//!   `accessor.filesConfigurationService` do teste enxergam esse serviço, e o gerenciador de modelos continua com o padrão.
//!   Aqui o [`TrackerFixture`] cria esse serviço ([`create_files_configuration_service`]) para o rastreador e para o
//!   `testOnFilesConfigurationChange`, e o acessor mantém o padrão para os modelos. O `files.autoSave` não chega ao
//!   `IConfigurationService` do acessor (nenhum caso o lê por lá).
//! - **Herança.** `TestWorkingCopyBackupTracker extends NativeWorkingCopyBackupTracker` é o [`WorkingCopyBackupTracker`] criado
//!   com os [`TestWorkingCopyBackupTrackerHooks`], que embrulham os hooks nativos: `getBackupScheduleDelay` devolve `10` e
//!   `suspendBackupOperations` dispara `onDidSuspend` e `onDidResume`. `waitForReady()` é `when_ready()`,
//!   `pendingBackupOperationCount` é `pending_backup_operation_count()` e o `dispose()` que cancela as operações pendentes é o
//!   `dispose()` seguido de `cancel_backup_operations()` no [`cleanup`].
//! - **`TestNativeWorkingCopyBackupService`.** O [`TestNativeWorkingCopyBackupService`] guarda o `NativeWorkingCopyBackupService`
//!   que ele estende, criado sem `backupPath` (como o ambiente de teste, que não tem), o que dá o serviço em memória. Os
//!   provedores de arquivos em memória e de perfis do construtor do original só servem à pasta de backup em disco e não
//!   entram; `testGetFileService` e `getBackupContents` não são lidos por esta suíte.
//! - **`TestNativeHostService`, `TestProgressService` e `INativeHostService`.** O serviço de host nativo do porte é o recorte
//!   que o rastreador lê (`getWindowCount`), e o serviço de progresso do teste roda a tarefa com `Progress.None`.
//! - **`TestBeforeShutdownEvent`.** O [`TestBeforeShutdownEvent`] dispara o `fireBeforeShutdown` do serviço de ciclo de vida e,
//!   como o `finalVeto` do original, chama a função de veto final na hora (`value`) e a guarda (`finalValue`).
//! - **`TestBackupWorkingCopy extends TestWorkingCopy`.** O Rust não tem herança: a [`TestBackupWorkingCopy`] guarda a
//!   `TestWorkingCopy` e delega a ela todo membro do contrato `IWorkingCopy`; a variante `scratchpad` sobrescreve
//!   `capabilities` (`Untitled | Scratchpad`), `isDirty` (`false`) e `isModified` (`true`), e o `backup` que lança é o
//!   `set_backup_override` com uma promessa rejeitada. O `registerWorkingCopy(this)` do construtor é feito por quem a cria.
//! - **`setup`.** A criação das pastas `Backups` e do `workspaces.json` no `fileService` do acessor não tem efeito sobre o
//!   serviço de backup em memória e não entra. O `disposables.add(files)` e o `registerTestFileEditor()` são feitos pelo
//!   [`create_tracker`], e `teardown` é o `disposables.clear()` ao fim do caso.
//! - **`toResource.call(this, path)`.** `URI.file(join('/', btoa(this.test.fullTitle()), path))` com o título completo do caso.
//! - **`isMacintosh`.** `Platform::detect().is_macintosh`. O caso é gerado por macro, um `#[test]` por linha das suítes
//!   `Hot Exit`; `this: any` e `.call(this, ...)` viram o título do caso.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod support;

use std::any::TypeId;
use std::cell::{Cell, RefCell};
use std::rc::Rc;

use common::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite, run_with_faked_timers};
use je_base::async_::async_fn;
use je_base::buffer::{VSBuffer, encode_base64};
use je_base::cancellation::CancellationToken;
use je_base::dynamic::Dynamic;
use je_base::errors::Error;
use je_base::event::{Emitter, Event};
use je_base::lifecycle::{DisposableRef, DisposableStore, IDisposable};
use je_base::path::PathModule;
use je_base::platform::Platform;
use je_base::promise::{MaybePromise, Promise, Resolver};
use je_editor::text_model::TextSource;
use je_instantiation::Injectable;
use je_platform::configuration::service::IConfigurationService;
use je_platform::configuration::test_configuration_service::TestConfigurationService;
use je_platform::dialogs::ConfirmResult;
use je_platform::environment::environment::EnvironmentService;
use je_platform::files::file_service::FileService;
use je_platform::files::i_file_service::IFileService;
use je_platform::keybinding::mock_keybinding_service::MockContextKeyService;
use je_platform::log::{LogServiceRef, NullLogService};
use je_platform::opener::editor_types::EditorOptions;
use je_platform::progress::progress::{IProgressService, Progress, ProgressOptions, ProgressStep, ProgressTask};
use je_platform::resources::{host_environment, uri_file};
use je_platform::uri_identity::uri_identity_service::UriIdentityService;
use je_platform::workspace::new_test_workspace;
use je_workbench::browser::parts::editor::editor_panes::{EditorPaneDescriptor, editor_pane_registry};
use je_workbench::common::editor::editor_model::IEditorModel;
use je_workbench::common::editor::{RevertOptions, SaveOptions, TextEditorOptions, UntypedEditorInput};
use je_workbench::contrib::files::browser::editors::file_editor_input::FileEditorInput;
use je_workbench::services::editor::editor_service::IEditorService;
use je_workbench::services::lifecycle::lifecycle::{FinalVetoFn, ILifecycleService, InternalBeforeShutdownEvent, ShutdownReason};
use je_workbench::services::textfile::textfiles::ITextFileEditorModelManager;
use je_workbench::services::working_copy::native_working_copy_backup_service::NativeWorkingCopyBackupService;
use je_workbench::services::working_copy::native_working_copy_backup_tracker::{INativeHostService, NativeWorkingCopyBackupTrackerHooks};
use je_workbench::services::working_copy::working_copy::{
    IWorkingCopy, WorkingCopyBackup, WorkingCopyBackupContent, WorkingCopyCapabilities, WorkingCopyIdentifier, WorkingCopySaveEvent,
};
use je_workbench::services::working_copy::working_copy_backup::{DiscardBackupsFilter, IWorkingCopyBackupService, ResolvedWorkingCopyBackup};
use je_workbench::services::working_copy::working_copy_backup_tracker::{BackupOperationsResume, WorkingCopyBackupTracker, WorkingCopyBackupTrackerHooks};
use je_workbench::services::working_copy::working_copy_service::IWorkingCopyService;
use je_workbench::test::browser::workbench_test_services::TestEnvironmentService;
use je_workbench::test::common::workbench_test_services::{TestContextService, TestFileService, TestFilesConfigurationService, TestLifecycleService, TestMarkerService, TestTextResourceConfigurationService, TestWorkingCopy};
use je_platform::files::files::HotExitConfiguration;
use je_workbench::services::working_copy::stored_file_working_copy::StoredFileWorkingCopySaveEvent;
use support::editor_service_support::EditorServiceAccessor;

/// O título da suíte, que o `this.test.fullTitle()` do `toResource` usa.
const SUITE_TITLE: &str = "WorkingCopyBackupTracker (native)";

/// `toResource.call(this, path)`: `URI.file(join('/', btoa(this.test.fullTitle()), path))`.
fn to_resource(test_title: &str, path: &str) -> je_base::uri::URI {
    let full_title = format!("{SUITE_TITLE} {test_title}");
    let env = host_environment();

    uri_file(&PathModule::native(env.as_ref()).join(&["/", &encode_base64(&VSBuffer::from_string(&full_title), true, false), path]))
}

/// `isMacintosh`.
fn is_macintosh() -> bool {
    Platform::detect().is_macintosh
}

//#region TestBeforeShutdownEvent

/// `TestBeforeShutdownEvent`: `value` é o resultado da função de veto final, chamada na hora, e `finalValue` é a função.
struct TestBeforeShutdownEvent {
    value: Option<Promise<bool>>,
    final_value: Option<FinalVetoFn>,
}

impl TestBeforeShutdownEvent {
    /// `const event = new TestBeforeShutdownEvent(); event.reason = reason; lifecycleService.fireBeforeShutdown(event)`.
    fn fire(lifecycle_service: &TestLifecycleService, reason: ShutdownReason) -> Self {
        let event = InternalBeforeShutdownEvent::new(reason);
        lifecycle_service.fire_before_shutdown(event.clone());

        match event.get_final_veto() {
            Some((veto_fn, _id)) => Self { value: Some(Self::to_promise(veto_fn())), final_value: Some(veto_fn) },
            None => Self { value: None, final_value: None },
        }
    }

    fn to_promise(value: MaybePromise<bool>) -> Promise<bool> {
        match value {
            MaybePromise::Value(value) => Promise::resolved(value),
            MaybePromise::Promise(promise) => promise,
        }
    }

    /// `await event.value`: `None` é o `undefined` de quem nunca registrou o veto final.
    async fn value(&self) -> Option<bool> {
        match &self.value {
            Some(value) => Some(value.clone().await.unwrap()),
            None => None,
        }
    }

    /// `await event.finalValue?.()`.
    async fn final_value(&self) -> Option<bool> {
        match &self.final_value {
            Some(final_value) => Some(Self::to_promise(final_value()).await.unwrap()),
            None => None,
        }
    }
}

//#endregion

//#region Dublês de serviço

/// `TestNativeHostService`: só o `windowCount` que o rastreador lê.
struct TestNativeHostService {
    window_count: RefCell<Promise<usize>>,
}

impl TestNativeHostService {
    fn new() -> Rc<Self> {
        Rc::new(Self { window_count: RefCell::new(Promise::resolved(1)) })
    }

    /// `windowCount = Promise.resolve(2)`.
    fn set_window_count(&self, window_count: Promise<usize>) {
        *self.window_count.borrow_mut() = window_count;
    }
}

impl INativeHostService for TestNativeHostService {
    fn get_window_count(&self) -> Promise<usize> {
        self.window_count.borrow().clone()
    }
}

/// `TestProgressService`: roda a tarefa com `Progress.None`.
struct TestProgressService;

impl IProgressService for TestProgressService {
    fn with_progress(&self, _options: ProgressOptions, task: ProgressTask, _on_did_cancel: Option<Rc<dyn Fn(Option<i32>)>>) -> Promise<()> {
        task(Progress::<ProgressStep>::none())
    }
}

/// `while (joiners.length) { joiners.pop()!(); }`.
fn release_joiners(joiners: &Rc<RefCell<Vec<Resolver<()>>>>) {
    loop {
        let resolver = joiners.borrow_mut().pop();
        let Some(resolver) = resolver else { break };
        resolver.resolve(());
    }
}

/// `TestNativeWorkingCopyBackupService`.
struct TestNativeWorkingCopyBackupService {
    inner: Rc<NativeWorkingCopyBackupService>,
    service: Rc<dyn IWorkingCopyBackupService>,
    backup_resource_joiners: Rc<RefCell<Vec<Resolver<()>>>>,
    discard_backup_joiners: Rc<RefCell<Vec<Resolver<()>>>>,
    discarded_backups: Rc<RefCell<Vec<WorkingCopyIdentifier>>>,
    discarded_all_backups: Cell<bool>,
    pending_backups: Rc<RefCell<Vec<(u64, Promise<()>)>>>,
    next_pending_id: Cell<u64>,
}

impl TestNativeWorkingCopyBackupService {
    fn new(disposables: &Rc<DisposableStore>) -> Rc<Self> {
        let environment_service = TestEnvironmentService::new();
        let log_service: LogServiceRef = NullLogService::shared();
        let file_service = disposables.add_rc(FileService::new(log_service.clone()));
        let lifecycle_service = disposables.add_rc(TestLifecycleService::new());
        let file_service_contract: Rc<dyn IFileService> = file_service;
        let lifecycle_service_contract: Rc<dyn ILifecycleService> = lifecycle_service;

        let inner = NativeWorkingCopyBackupService::new(None, &environment_service.user_roaming_data_home(), file_service_contract, log_service, lifecycle_service_contract);
        let service = inner.service();

        Rc::new(Self {
            inner,
            service,
            backup_resource_joiners: Rc::new(RefCell::new(Vec::new())),
            discard_backup_joiners: Rc::new(RefCell::new(Vec::new())),
            discarded_backups: Rc::new(RefCell::new(Vec::new())),
            discarded_all_backups: Cell::new(false),
            pending_backups: Rc::new(RefCell::new(Vec::new())),
            next_pending_id: Cell::new(0),
        })
    }

    /// `waitForAllBackups()`.
    fn wait_for_all_backups(&self) -> Promise<()> {
        let pending: Vec<Promise<()>> = self.pending_backups.borrow().iter().map(|(_, promise)| promise.clone()).collect();

        async_fn(async move {
            for promise in pending {
                promise.await?;
            }

            Ok(())
        })
    }

    /// `joinBackupResource()`.
    fn join_backup_resource(&self) -> Promise<()> {
        let joiners = self.backup_resource_joiners.clone();

        Promise::with_executor(move |resolver| joiners.borrow_mut().push(resolver))
    }

    /// `joinDiscardBackup()`.
    fn join_discard_backup(&self) -> Promise<()> {
        let joiners = self.discard_backup_joiners.clone();

        Promise::with_executor(move |resolver| joiners.borrow_mut().push(resolver))
    }

    /// `discardedBackups.length`.
    fn discarded_backups_count(&self) -> usize {
        self.discarded_backups.borrow().len()
    }

    /// `discardedAllBackups`.
    fn discarded_all_backups(&self) -> bool {
        self.discarded_all_backups.get()
    }
}

impl IDisposable for TestNativeWorkingCopyBackupService {
    fn dispose(&self) {
        self.inner.dispose();
    }
}

impl IWorkingCopyBackupService for TestNativeWorkingCopyBackupService {
    fn has_backup_sync(&self, identifier: &WorkingCopyIdentifier, version_id: Option<u64>, meta: Option<&Dynamic>) -> bool {
        self.service.has_backup_sync(identifier, version_id, meta)
    }

    fn get_backups(&self) -> Promise<Vec<WorkingCopyIdentifier>> {
        self.service.get_backups()
    }

    fn resolve(&self, identifier: &WorkingCopyIdentifier) -> Promise<Option<ResolvedWorkingCopyBackup>> {
        self.service.resolve(identifier)
    }

    fn backup(
        &self,
        identifier: &WorkingCopyIdentifier,
        content: Option<WorkingCopyBackupContent>,
        version_id: Option<u64>,
        meta: Option<Dynamic>,
        token: Option<CancellationToken>,
    ) -> Promise<()> {
        let backup = self.service.backup(identifier, content, version_id, meta, token);

        let id = self.next_pending_id.get();
        self.next_pending_id.set(id + 1);
        self.pending_backups.borrow_mut().push((id, backup.clone()));

        let pending_backups = self.pending_backups.clone();
        let joiners = self.backup_resource_joiners.clone();

        async_fn(async move {
            let result = backup.await;
            pending_backups.borrow_mut().retain(|(candidate, _)| *candidate != id); // `removeFromPendingBackups()`
            result?;

            release_joiners(&joiners);

            Ok(())
        })
    }

    fn discard_backup(&self, identifier: &WorkingCopyIdentifier, _token: Option<CancellationToken>) -> Promise<()> {
        let discard = self.service.discard_backup(identifier, None);
        let identifier = identifier.clone();
        let discarded_backups = self.discarded_backups.clone();
        let joiners = self.discard_backup_joiners.clone();

        async_fn(async move {
            discard.await?;
            discarded_backups.borrow_mut().push(identifier);

            release_joiners(&joiners);

            Ok(())
        })
    }

    fn discard_backups(&self, filter: Option<DiscardBackupsFilter>) -> Promise<()> {
        self.discarded_all_backups.set(true);

        self.service.discard_backups(filter)
    }
}

/// `TestWorkingCopyBackupTracker`: sobrescreve `getBackupScheduleDelay` e `suspendBackupOperations`.
struct TestWorkingCopyBackupTrackerHooks {
    native: NativeWorkingCopyBackupTrackerHooks,
    on_did_resume: Rc<Emitter<()>>,
    on_did_suspend: Rc<Emitter<()>>,
}

impl TestWorkingCopyBackupTrackerHooks {
    /// `onDidResume`.
    fn on_did_resume(&self) -> Event<()> {
        self.on_did_resume.event()
    }

    /// `onDidSuspend`.
    fn on_did_suspend(&self) -> Event<()> {
        self.on_did_suspend.event()
    }
}

impl WorkingCopyBackupTrackerHooks for TestWorkingCopyBackupTrackerHooks {
    fn on_final_before_shutdown(&self, tracker: &WorkingCopyBackupTracker, reason: ShutdownReason) -> MaybePromise<bool> {
        self.native.on_final_before_shutdown(tracker, reason)
    }

    /// `return 10; // Reduce timeout for tests`.
    fn get_backup_schedule_delay(&self, _tracker: &WorkingCopyBackupTracker, _working_copy: &dyn IWorkingCopy) -> u64 {
        10
    }

    fn suspend_backup_operations(&self, tracker: &WorkingCopyBackupTracker) -> BackupOperationsResume {
        let resume = tracker.default_suspend_backup_operations();

        self.on_did_suspend.fire(());

        let on_did_resume = self.on_did_resume.clone();
        BackupOperationsResume::new(move || {
            resume.resume();

            on_did_resume.fire(());
        })
    }
}

/// `TestBackupWorkingCopy extends TestWorkingCopy` (ver as notas do módulo).
struct TestBackupWorkingCopy {
    inner: Rc<TestWorkingCopy>,
    scratchpad: bool,
}

impl TestBackupWorkingCopy {
    /// `new TestBackupWorkingCopy(resource)`: registra no serviço de working copies, como o construtor do original.
    fn new(disposables: &Rc<DisposableStore>, accessor: &EditorServiceAccessor, resource: je_base::uri::URI, scratchpad: bool, backup_fails: bool) -> Rc<Self> {
        let inner = TestWorkingCopy::new(resource, None, None);
        if backup_fails {
            inner.set_backup_override(Rc::new(|_token: &CancellationToken| Promise::rejected(Error::new("unable to backup"))));
        }

        let working_copy = Rc::new(Self { inner, scratchpad });
        disposables.add(accessor.working_copy_service.register_working_copy(working_copy.clone()));
        disposables.add_rc(working_copy.clone());

        working_copy
    }

    fn set_dirty(&self, dirty: bool) {
        self.inner.set_dirty(dirty);
    }
}

impl IDisposable for TestBackupWorkingCopy {
    fn dispose(&self) {
        self.inner.dispose();
    }
}

impl IWorkingCopy for TestBackupWorkingCopy {
    fn working_copy_type_id(&self) -> String {
        IWorkingCopy::working_copy_type_id(&*self.inner)
    }

    fn resource(&self) -> je_base::uri::URI {
        IWorkingCopy::resource(&*self.inner)
    }

    fn name(&self) -> String {
        IWorkingCopy::name(&*self.inner)
    }

    /// `override capabilities = WorkingCopyCapabilities.Untitled | WorkingCopyCapabilities.Scratchpad`.
    fn capabilities(&self) -> WorkingCopyCapabilities {
        if self.scratchpad {
            WorkingCopyCapabilities(WorkingCopyCapabilities::UNTITLED.0 | WorkingCopyCapabilities::SCRATCHPAD.0)
        } else {
            IWorkingCopy::capabilities(&*self.inner)
        }
    }

    fn on_did_change_dirty(&self) -> Event<()> {
        IWorkingCopy::on_did_change_dirty(&*self.inner)
    }

    fn on_did_change_content(&self) -> Event<()> {
        IWorkingCopy::on_did_change_content(&*self.inner)
    }

    fn on_did_save(&self) -> Event<WorkingCopySaveEvent> {
        IWorkingCopy::on_did_save(&*self.inner)
    }

    fn stored_file_save_event(&self) -> Option<Event<StoredFileWorkingCopySaveEvent>> {
        IWorkingCopy::stored_file_save_event(&*self.inner)
    }

    /// `override isDirty() { return false; }` do scratchpad.
    fn is_dirty(&self) -> bool {
        !self.scratchpad && IWorkingCopy::is_dirty(&*self.inner)
    }

    /// `override isModified() { return true; }` do scratchpad.
    fn is_modified(&self) -> bool {
        self.scratchpad || IWorkingCopy::is_dirty(&*self.inner)
    }

    fn backup_delay(&self) -> Option<u64> {
        IWorkingCopy::backup_delay(&*self.inner)
    }

    fn backup(&self, token: &CancellationToken) -> Promise<WorkingCopyBackup> {
        IWorkingCopy::backup(&*self.inner, token)
    }

    fn save(&self, options: Option<SaveOptions>) -> Promise<bool> {
        IWorkingCopy::save(&*self.inner, options)
    }

    fn revert(&self, options: Option<RevertOptions>) -> Promise<()> {
        IWorkingCopy::revert(&*self.inner, options)
    }
}

/// `registerTestFileEditor()`: o painel lógico de teste para a `FileEditorInput`.
fn register_test_file_editor() -> DisposableRef {
    editor_pane_registry().register_editor_pane(EditorPaneDescriptor::new("testTextFileEditor", "Text File Editor"), vec![TypeId::of::<FileEditorInput>()])
}

//#endregion

//#region createTracker

/// `createTracker()`: o acessor, o rastreador e os serviços que os casos leem.
struct TrackerFixture {
    accessor: EditorServiceAccessor,
    tracker: Rc<WorkingCopyBackupTracker>,
    hooks: Rc<TestWorkingCopyBackupTrackerHooks>,
    working_copy_backup_service: Rc<TestNativeWorkingCopyBackupService>,
    files_configuration_service: Rc<TestFilesConfigurationService>,
    native_host_service: Rc<TestNativeHostService>,
}

/// O `TestFilesConfigurationService` do `createTracker`, sobre um `TestConfigurationService` com `files.autoSave` e
/// `files.autoSaveDelay: 1`.
fn create_files_configuration_service(disposables: &Rc<DisposableStore>, auto_save_enabled: bool) -> Rc<TestFilesConfigurationService> {
    let test_configuration_service = TestConfigurationService::new(Some(Dynamic::object([(
        "files",
        Dynamic::object([("autoSave", Dynamic::from(if auto_save_enabled { "afterDelay" } else { "off" })), ("autoSaveDelay", Dynamic::from(1))]),
    )])));
    disposables.add_rc(test_configuration_service.on_did_change_configuration_emitter.clone());
    let configuration_service: Rc<dyn IConfigurationService> = test_configuration_service;

    let context_key_service = disposables.add_rc(MockContextKeyService::new());
    let uri_identity_file_service = disposables.add_rc(TestFileService::new());
    let uri_identity_file_service_contract: Rc<dyn IFileService> = uri_identity_file_service;
    let uri_identity_service = disposables.add_rc(UriIdentityService::create((), (uri_identity_file_service_contract,)));
    let file_service = disposables.add_rc(TestFileService::new());
    let file_service_contract: Rc<dyn IFileService> = file_service;

    disposables.add_rc(TestFilesConfigurationService::create(
        (),
        (
            context_key_service,
            configuration_service.clone(),
            TestContextService::new(None, None),
            TestEnvironmentService::new(),
            uri_identity_service,
            file_service_contract,
            Rc::new(TestMarkerService),
            TestTextResourceConfigurationService::new(configuration_service),
        ),
    ))
}

/// O `setup` e o `createTracker(autoSaveEnabled)`.
fn create_tracker(disposables: &Rc<DisposableStore>, auto_save_enabled: bool) -> TrackerFixture {
    let working_copy_backup_service = disposables.add_rc(TestNativeWorkingCopyBackupService::new(disposables));
    let accessor = EditorServiceAccessor::new(disposables, working_copy_backup_service.clone());
    disposables.add_rc(accessor.files());
    disposables.add(register_test_file_editor());

    let files_configuration_service = create_files_configuration_service(disposables, auto_save_enabled);
    let native_host_service = TestNativeHostService::new();

    let on_did_resume = Emitter::new();
    let on_did_suspend = Emitter::new();
    disposables.add_rc(on_did_resume.clone());
    disposables.add_rc(on_did_suspend.clone());
    let hooks = Rc::new(TestWorkingCopyBackupTrackerHooks {
        native: NativeWorkingCopyBackupTrackerHooks::new(
            accessor.file_dialog_service.clone(),
            accessor.dialog_service.clone(),
            accessor.context_service.clone(),
            native_host_service.clone(),
            TestEnvironmentService::new(),
            Rc::new(TestProgressService),
        ),
        on_did_resume,
        on_did_suspend,
    });

    let tracker = WorkingCopyBackupTracker::new(
        working_copy_backup_service.clone(),
        accessor.working_copy_service.clone(),
        accessor.log_service.clone(),
        accessor.lifecycle_service.clone(),
        files_configuration_service.clone(),
        accessor.working_copy_editor_service.clone(),
        accessor.editor_service.clone(),
        hooks.clone(),
    );

    TrackerFixture { accessor, tracker, hooks, working_copy_backup_service, files_configuration_service, native_host_service }
}

/// O `cleanup` do `createTracker`.
async fn cleanup(fixture: &TrackerFixture) {
    fixture.working_copy_backup_service.wait_for_all_backups().await.unwrap(); // File changes could also schedule some backup operations so we need to wait for them before finishing the test

    fixture.accessor.teardown().await.unwrap();

    // `tracker.dispose()` do teste, que cancela e descarta as operações pendentes
    fixture.tracker.dispose();
    fixture.tracker.cancel_backup_operations();
}

/// `workingCopyBackupService.hasBackupSync(workingCopy)`.
fn has_backup_sync(service: &TestNativeWorkingCopyBackupService, working_copy: &dyn IWorkingCopy) -> bool {
    service.has_backup_sync(&WorkingCopyIdentifier::of(working_copy), None, None)
}

/// `accessor.editorService.openEditor({ resource, options: { pinned: true } })`.
async fn open_pinned_editor(accessor: &EditorServiceAccessor, resource: &je_base::uri::URI) {
    IEditorService::open_editor(
        accessor.editor_service.as_ref(),
        UntypedEditorInput {
            resource: Some(resource.clone()),
            options: Some(TextEditorOptions::from(EditorOptions { pinned: Some(true), ..EditorOptions::default() })),
            ..UntypedEditorInput::default()
        },
    )
    .await
    .unwrap();
}

/// `{ files: { hotExit: setting } }`.
fn hot_exit_configuration(setting: &str) -> Dynamic {
    Dynamic::object([("files", Dynamic::object([("hotExit", Dynamic::from(setting))]))])
}

/// `accessor.filesConfigurationService.testOnFilesConfigurationChange({ files: { hotExit: setting } })`.
fn set_hot_exit(fixture: &TrackerFixture, setting: &str) {
    fixture.files_configuration_service.on_files_configuration_change(&hot_exit_configuration(setting), false);
}

//#endregion

#[test]
fn track_backups_file_auto_save_off() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let disposables = guard.store();

    run_with_faked_timers(async move {
        let resource = to_resource("Track backups (file, auto save off)", "/path/index.txt");
        track_backups_test(disposables, resource, false).await;
    });
}

#[test]
fn track_backups_file_auto_save_on() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let disposables = guard.store();

    run_with_faked_timers(async move {
        let resource = to_resource("Track backups (file, auto save on)", "/path/index.txt");
        track_backups_test(disposables, resource, true).await;
    });
}

/// `trackBackupsTest(resource, autoSave)`.
async fn track_backups_test(disposables: Rc<DisposableStore>, resource: je_base::uri::URI, auto_save: bool) {
    let fixture = create_tracker(&disposables, auto_save);

    open_pinned_editor(&fixture.accessor, &resource).await;

    let file_model = fixture.accessor.files().get(&resource).expect("o modelo do arquivo existe");
    if let Some(text_editor_model) = file_model.text_editor_model() {
        text_editor_model.set_value(TextSource::from("Super Good"));
    }

    fixture.working_copy_backup_service.join_backup_resource().await.unwrap();

    assert!(has_backup_sync(&fixture.working_copy_backup_service, &*file_model));

    file_model.dispose();

    fixture.working_copy_backup_service.join_discard_backup().await.unwrap();

    assert!(!has_backup_sync(&fixture.working_copy_backup_service, &*file_model));

    cleanup(&fixture).await;
}

#[test]
fn on_will_shutdown_no_veto_if_no_dirty_files() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let disposables = guard.store();

    run_with_faked_timers(async move {
        let fixture = create_tracker(&disposables, false);

        let resource = to_resource("onWillShutdown - no veto if no dirty files", "/path/index.txt");
        open_pinned_editor(&fixture.accessor, &resource).await;

        let event = TestBeforeShutdownEvent::fire(&fixture.accessor.lifecycle_service, ShutdownReason::Close);

        let veto = event.value().await;
        assert!(!veto.unwrap_or(false));

        cleanup(&fixture).await;
    });
}

#[test]
fn on_will_shutdown_veto_if_user_cancels_hot_exit_off() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let disposables = guard.store();

    run_with_faked_timers(async move {
        let fixture = create_tracker(&disposables, false);

        let resource = to_resource("onWillShutdown - veto if user cancels (hot.exit: off)", "/path/index.txt");
        open_pinned_editor(&fixture.accessor, &resource).await;

        let model = fixture.accessor.files().get(&resource).expect("o modelo do arquivo existe");

        fixture.accessor.file_dialog_service.set_confirm_result(ConfirmResult::Cancel);
        set_hot_exit(&fixture, HotExitConfiguration::OFF);

        model.resolve().await.unwrap();
        if let Some(text_editor_model) = model.text_editor_model() {
            text_editor_model.set_value(TextSource::from("foo"));
        }
        assert_eq!(fixture.accessor.working_copy_service.dirty_count(), 1);

        let event = TestBeforeShutdownEvent::fire(&fixture.accessor.lifecycle_service, ShutdownReason::Close);

        let veto = event.value().await;
        assert!(veto.unwrap_or(false));

        cleanup(&fixture).await;
    });
}

#[test]
fn on_will_shutdown_no_veto_if_auto_save_is_on() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let disposables = guard.store();

    run_with_faked_timers(async move {
        let fixture = create_tracker(&disposables, true /* auto save enabled */);

        let resource = to_resource("onWillShutdown - no veto if auto save is on", "/path/index.txt");
        open_pinned_editor(&fixture.accessor, &resource).await;

        let model = fixture.accessor.files().get(&resource).expect("o modelo do arquivo existe");

        model.resolve().await.unwrap();
        if let Some(text_editor_model) = model.text_editor_model() {
            text_editor_model.set_value(TextSource::from("foo"));
        }
        assert_eq!(fixture.accessor.working_copy_service.dirty_count(), 1);

        let event = TestBeforeShutdownEvent::fire(&fixture.accessor.lifecycle_service, ShutdownReason::Close);

        let veto = event.value().await;
        assert!(!veto.unwrap_or(false));

        assert_eq!(fixture.accessor.working_copy_service.dirty_count(), 0);

        cleanup(&fixture).await;
    });
}

#[test]
fn on_will_shutdown_no_veto_and_backups_cleaned_up_if_user_does_not_want_to_save_hot_exit_off() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let disposables = guard.store();

    run_with_faked_timers(async move {
        let fixture = create_tracker(&disposables, false);

        let resource = to_resource("onWillShutdown - no veto and backups cleaned up if user does not want to save (hot.exit: off)", "/path/index.txt");
        open_pinned_editor(&fixture.accessor, &resource).await;

        let model = fixture.accessor.files().get(&resource).expect("o modelo do arquivo existe");

        fixture.accessor.file_dialog_service.set_confirm_result(ConfirmResult::DontSave);
        set_hot_exit(&fixture, HotExitConfiguration::OFF);

        model.resolve().await.unwrap();
        if let Some(text_editor_model) = model.text_editor_model() {
            text_editor_model.set_value(TextSource::from("foo"));
        }
        assert_eq!(fixture.accessor.working_copy_service.dirty_count(), 1);
        let event = TestBeforeShutdownEvent::fire(&fixture.accessor.lifecycle_service, ShutdownReason::Close);

        let veto = event.value().await;
        assert!(!veto.unwrap_or(false));
        assert!(fixture.working_copy_backup_service.discarded_backups_count() > 0);

        cleanup(&fixture).await;
    });
}

#[test]
fn on_will_shutdown_no_backups_discarded_when_shutdown_without_dirty_but_tracker_not_ready() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let disposables = guard.store();

    run_with_faked_timers(async move {
        let fixture = create_tracker(&disposables, false);

        let event = TestBeforeShutdownEvent::fire(&fixture.accessor.lifecycle_service, ShutdownReason::Close);

        let veto = event.value().await;
        assert!(!veto.unwrap_or(false));
        assert!(!fixture.working_copy_backup_service.discarded_all_backups());

        cleanup(&fixture).await;
    });
}

#[test]
fn on_will_shutdown_backups_discarded_when_shutdown_without_dirty() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let disposables = guard.store();

    run_with_faked_timers(async move {
        let fixture = create_tracker(&disposables, false);

        fixture.tracker.when_ready().await.unwrap();

        let event = TestBeforeShutdownEvent::fire(&fixture.accessor.lifecycle_service, ShutdownReason::Close);

        let veto = event.value().await;
        assert!(!veto.unwrap_or(false));
        assert!(fixture.working_copy_backup_service.discarded_all_backups());

        cleanup(&fixture).await;
    });
}

#[test]
fn on_will_shutdown_save_hot_exit_off() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let disposables = guard.store();

    run_with_faked_timers(async move {
        let fixture = create_tracker(&disposables, false);

        let resource = to_resource("onWillShutdown - save (hot.exit: off)", "/path/index.txt");
        open_pinned_editor(&fixture.accessor, &resource).await;

        let model = fixture.accessor.files().get(&resource).expect("o modelo do arquivo existe");

        fixture.accessor.file_dialog_service.set_confirm_result(ConfirmResult::Save);
        set_hot_exit(&fixture, HotExitConfiguration::OFF);

        model.resolve().await.unwrap();
        if let Some(text_editor_model) = model.text_editor_model() {
            text_editor_model.set_value(TextSource::from("foo"));
        }
        assert_eq!(fixture.accessor.working_copy_service.dirty_count(), 1);
        let event = TestBeforeShutdownEvent::fire(&fixture.accessor.lifecycle_service, ShutdownReason::Close);

        let veto = event.value().await;
        assert!(!veto.unwrap_or(false));
        assert!(!model.is_dirty());

        cleanup(&fixture).await;
    });
}

#[test]
fn on_will_shutdown_veto_if_backup_fails() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let disposables = guard.store();

    run_with_faked_timers(async move {
        let fixture = create_tracker(&disposables, false);

        let resource = to_resource("onWillShutdown - veto if backup fails", "/path/custom.txt");
        let custom_working_copy = TestBackupWorkingCopy::new(&disposables, &fixture.accessor, resource, false, true);
        custom_working_copy.set_dirty(true);

        let event = TestBeforeShutdownEvent::fire(&fixture.accessor.lifecycle_service, ShutdownReason::Quit);

        let veto = event.value().await;
        assert!(veto.unwrap_or(false));

        let final_veto = event.final_value().await;
        assert!(final_veto.unwrap_or(false)); // assert the tracker uses the internal finalVeto API

        cleanup(&fixture).await;
    });
}

#[test]
fn on_will_shutdown_scratchpads_veto_if_backup_fails() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let disposables = guard.store();

    run_with_faked_timers(async move {
        let fixture = create_tracker(&disposables, false);

        let resource = to_resource("onWillShutdown - scratchpads - veto if backup fails", "/path/custom.txt");
        TestBackupWorkingCopy::new(&disposables, &fixture.accessor, resource, true, true);

        let event = TestBeforeShutdownEvent::fire(&fixture.accessor.lifecycle_service, ShutdownReason::Quit);

        let veto = event.value().await;
        assert!(veto.unwrap_or(false));

        let final_veto = event.final_value().await;
        assert!(final_veto.unwrap_or(false)); // assert the tracker uses the internal finalVeto API

        cleanup(&fixture).await;
    });
}

#[test]
fn on_will_shutdown_pending_backup_operations_canceled_and_tracker_suspended_resumsed() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let disposables = guard.store();

    run_with_faked_timers(async move {
        let fixture = create_tracker(&disposables, false);

        let resource = to_resource("onWillShutdown - pending backup operations canceled and tracker suspended/resumsed", "/path/index.txt");
        open_pinned_editor(&fixture.accessor, &resource).await;

        let model = fixture.accessor.files().get(&resource).expect("o modelo do arquivo existe");

        model.resolve().await.unwrap();
        if let Some(text_editor_model) = model.text_editor_model() {
            text_editor_model.set_value(TextSource::from("foo"));
        }
        assert_eq!(fixture.accessor.working_copy_service.dirty_count(), 1);
        assert_eq!(fixture.tracker.pending_backup_operation_count(), 1);

        let on_suspend = fixture.hooks.on_did_suspend().to_promise(None);

        let event = TestBeforeShutdownEvent::fire(&fixture.accessor.lifecycle_service, ShutdownReason::Quit);

        on_suspend.promise.clone().await.unwrap();

        assert_eq!(fixture.tracker.pending_backup_operation_count(), 0);

        // Ops are suspended during shutdown!
        if let Some(text_editor_model) = model.text_editor_model() {
            text_editor_model.set_value(TextSource::from("bar"));
        }
        assert_eq!(fixture.accessor.working_copy_service.dirty_count(), 1);
        assert_eq!(fixture.tracker.pending_backup_operation_count(), 0);

        let on_resume = fixture.hooks.on_did_resume().to_promise(None);
        event.value().await;

        // Ops are resumed after shutdown!
        if let Some(text_editor_model) = model.text_editor_model() {
            text_editor_model.set_value(TextSource::from("foo"));
        }
        on_resume.promise.clone().await.unwrap();
        assert_eq!(fixture.tracker.pending_backup_operation_count(), 1);

        cleanup(&fixture).await;
    });
}

//#region Hot Exit

/// Os parâmetros de `hotExitTest` e `scratchpadHotExitTest` (`this: any` é o título do caso).
struct HotExitCase {
    suite: &'static str,
    title: &'static str,
    setting: &'static str,
    reason: ShutdownReason,
    multiple_windows: bool,
    workspace: bool,
    should_veto: bool,
}

impl HotExitCase {
    /// Os ajustes comuns dos dois helpers: o hot exit, o workspace vazio, as janelas e o `CANCEL` que força o veto.
    fn configure(&self, fixture: &TrackerFixture) {
        // Set hot exit config
        set_hot_exit(fixture, self.setting);

        // Set empty workspace if required
        if !self.workspace {
            fixture.accessor.context_service.set_workspace(new_test_workspace("empty:1508317022751", Vec::new(), None, None));
        }

        // Set multiple windows if required
        if self.multiple_windows {
            fixture.native_host_service.set_window_count(Promise::resolved(2));
        }

        // Set cancel to force a veto if hot exit does not trigger
        fixture.accessor.file_dialog_service.set_confirm_result(ConfirmResult::Cancel);
    }

    /// A parte final dos dois helpers: dispara o desligamento e confere o veto.
    async fn assert_shutdown(&self, fixture: &TrackerFixture) {
        let event = TestBeforeShutdownEvent::fire(&fixture.accessor.lifecycle_service, self.reason);

        let veto = event.value().await;
        assert!(event.final_value.is_some()); // assert the tracker uses the internal finalVeto API
        assert_eq!(fixture.working_copy_backup_service.discarded_backups_count(), 0); // When hot exit is set, backups should never be cleaned since the confirm result is cancel
        assert_eq!(veto, Some(self.should_veto));
    }

    fn test_title(&self) -> String {
        format!("Hot Exit {} {}", self.suite, self.title)
    }
}

/// `hotExitTest(setting, shutdownReason, multipleWindows, workspace, shouldVeto)`.
async fn hot_exit_test(disposables: Rc<DisposableStore>, case: HotExitCase) {
    let fixture = create_tracker(&disposables, false);

    let resource = to_resource(&case.test_title(), "/path/index.txt");
    open_pinned_editor(&fixture.accessor, &resource).await;

    let model = fixture.accessor.files().get(&resource).expect("o modelo do arquivo existe");

    case.configure(&fixture);

    model.resolve().await.unwrap();
    if let Some(text_editor_model) = model.text_editor_model() {
        text_editor_model.set_value(TextSource::from("foo"));
    }
    assert_eq!(fixture.accessor.working_copy_service.dirty_count(), 1);

    case.assert_shutdown(&fixture).await;

    cleanup(&fixture).await;
}

/// `scratchpadHotExitTest(setting, shutdownReason, multipleWindows, workspace, shouldVeto)`.
async fn scratchpad_hot_exit_test(disposables: Rc<DisposableStore>, case: HotExitCase) {
    let fixture = create_tracker(&disposables, false);

    case.configure(&fixture);

    let resource = to_resource(&case.test_title(), "/path/custom.txt");
    TestBackupWorkingCopy::new(&disposables, &fixture.accessor, resource, true, false);

    case.assert_shutdown(&fixture).await;

    cleanup(&fixture).await;
}

const CLOSE: ShutdownReason = ShutdownReason::Close;
const QUIT: ShutdownReason = ShutdownReason::Quit;
const RELOAD: ShutdownReason = ShutdownReason::Reload;
const LOAD: ShutdownReason = ShutdownReason::Load;

/// Um `#[test]` por caso das suítes `Hot Exit`.
macro_rules! hot_exit_cases {
    ($runner:ident, $suite:expr, $setting:expr; $($name:ident: $title:expr, $reason:expr, $multiple_windows:expr, $workspace:expr, $should_veto:expr;)+) => {
        $(
            #[test]
            fn $name() {
                let guard: LeakGuard = ensure_no_disposables_are_leaked_in_test_suite();
                let disposables = guard.store();

                run_with_faked_timers(async move {
                    $runner(
                        disposables,
                        HotExitCase {
                            suite: $suite,
                            title: $title,
                            setting: $setting,
                            reason: $reason,
                            multiple_windows: $multiple_windows,
                            workspace: $workspace,
                            should_veto: $should_veto,
                        },
                    )
                    .await;
                });
            }
        )+
    };
}

hot_exit_cases! { hot_exit_test, "\"onExit\" setting", HotExitConfiguration::ON_EXIT;
    on_exit_should_hot_exit_on_non_mac_close_single_workspace: "should hot exit on non-Mac (reason: CLOSE, windows: single, workspace)", CLOSE, false, true, is_macintosh();
    on_exit_should_hot_exit_on_non_mac_close_single_empty_workspace: "should hot exit on non-Mac (reason: CLOSE, windows: single, empty workspace)", CLOSE, false, false, is_macintosh();
    on_exit_should_not_hot_exit_close_multiple_workspace: "should NOT hot exit (reason: CLOSE, windows: multiple, workspace)", CLOSE, true, true, true;
    on_exit_should_not_hot_exit_close_multiple_empty_workspace: "should NOT hot exit (reason: CLOSE, windows: multiple, empty workspace)", CLOSE, true, false, true;
    on_exit_should_hot_exit_quit_single_workspace: "should hot exit (reason: QUIT, windows: single, workspace)", QUIT, false, true, false;
    on_exit_should_hot_exit_quit_single_empty_workspace: "should hot exit (reason: QUIT, windows: single, empty workspace)", QUIT, false, false, false;
    on_exit_should_hot_exit_quit_multiple_workspace: "should hot exit (reason: QUIT, windows: multiple, workspace)", QUIT, true, true, false;
    on_exit_should_hot_exit_quit_multiple_empty_workspace: "should hot exit (reason: QUIT, windows: multiple, empty workspace)", QUIT, true, false, false;
    on_exit_should_hot_exit_reload_single_workspace: "should hot exit (reason: RELOAD, windows: single, workspace)", RELOAD, false, true, false;
    on_exit_should_hot_exit_reload_single_empty_workspace: "should hot exit (reason: RELOAD, windows: single, empty workspace)", RELOAD, false, false, false;
    on_exit_should_hot_exit_reload_multiple_workspace: "should hot exit (reason: RELOAD, windows: multiple, workspace)", RELOAD, true, true, false;
    on_exit_should_hot_exit_reload_multiple_empty_workspace: "should hot exit (reason: RELOAD, windows: multiple, empty workspace)", RELOAD, true, false, false;
    on_exit_should_not_hot_exit_load_single_workspace: "should NOT hot exit (reason: LOAD, windows: single, workspace)", LOAD, false, true, true;
    on_exit_should_not_hot_exit_load_single_empty_workspace: "should NOT hot exit (reason: LOAD, windows: single, empty workspace)", LOAD, false, false, true;
    on_exit_should_not_hot_exit_load_multiple_workspace: "should NOT hot exit (reason: LOAD, windows: multiple, workspace)", LOAD, true, true, true;
    on_exit_should_not_hot_exit_load_multiple_empty_workspace: "should NOT hot exit (reason: LOAD, windows: multiple, empty workspace)", LOAD, true, false, true;
}

hot_exit_cases! { hot_exit_test, "\"onExitAndWindowClose\" setting", HotExitConfiguration::ON_EXIT_AND_WINDOW_CLOSE;
    on_exit_and_window_close_should_hot_exit_close_single_workspace: "should hot exit (reason: CLOSE, windows: single, workspace)", CLOSE, false, true, false;
    on_exit_and_window_close_should_hot_exit_close_single_empty_workspace: "should hot exit (reason: CLOSE, windows: single, empty workspace)", CLOSE, false, false, is_macintosh();
    on_exit_and_window_close_should_hot_exit_close_multiple_workspace: "should hot exit (reason: CLOSE, windows: multiple, workspace)", CLOSE, true, true, false;
    on_exit_and_window_close_should_not_hot_exit_close_multiple_empty_workspace: "should NOT hot exit (reason: CLOSE, windows: multiple, empty workspace)", CLOSE, true, false, true;
    on_exit_and_window_close_should_hot_exit_quit_single_workspace: "should hot exit (reason: QUIT, windows: single, workspace)", QUIT, false, true, false;
    on_exit_and_window_close_should_hot_exit_quit_single_empty_workspace: "should hot exit (reason: QUIT, windows: single, empty workspace)", QUIT, false, false, false;
    on_exit_and_window_close_should_hot_exit_quit_multiple_workspace: "should hot exit (reason: QUIT, windows: multiple, workspace)", QUIT, true, true, false;
    on_exit_and_window_close_should_hot_exit_quit_multiple_empty_workspace: "should hot exit (reason: QUIT, windows: multiple, empty workspace)", QUIT, true, false, false;
    on_exit_and_window_close_should_hot_exit_reload_single_workspace: "should hot exit (reason: RELOAD, windows: single, workspace)", RELOAD, false, true, false;
    on_exit_and_window_close_should_hot_exit_reload_single_empty_workspace: "should hot exit (reason: RELOAD, windows: single, empty workspace)", RELOAD, false, false, false;
    on_exit_and_window_close_should_hot_exit_reload_multiple_workspace: "should hot exit (reason: RELOAD, windows: multiple, workspace)", RELOAD, true, true, false;
    on_exit_and_window_close_should_hot_exit_reload_multiple_empty_workspace: "should hot exit (reason: RELOAD, windows: multiple, empty workspace)", RELOAD, true, false, false;
    on_exit_and_window_close_should_hot_exit_load_single_workspace: "should hot exit (reason: LOAD, windows: single, workspace)", LOAD, false, true, false;
    on_exit_and_window_close_should_not_hot_exit_load_single_empty_workspace: "should NOT hot exit (reason: LOAD, windows: single, empty workspace)", LOAD, false, false, true;
    on_exit_and_window_close_should_hot_exit_load_multiple_workspace: "should hot exit (reason: LOAD, windows: multiple, workspace)", LOAD, true, true, false;
    on_exit_and_window_close_should_not_hot_exit_load_multiple_empty_workspace: "should NOT hot exit (reason: LOAD, windows: multiple, empty workspace)", LOAD, true, false, true;
}

hot_exit_cases! { scratchpad_hot_exit_test, "\"onExit\" setting - scratchpad", HotExitConfiguration::ON_EXIT;
    on_exit_scratchpad_should_hot_exit_close_single_workspace: "should hot exit (reason: CLOSE, windows: single, workspace)", CLOSE, false, true, false;
    on_exit_scratchpad_should_hot_exit_close_single_empty_workspace: "should hot exit (reason: CLOSE, windows: single, empty workspace)", CLOSE, false, false, is_macintosh();
    on_exit_scratchpad_should_hot_exit_close_multiple_workspace: "should hot exit (reason: CLOSE, windows: multiple, workspace)", CLOSE, true, true, false;
    on_exit_scratchpad_should_not_hot_exit_close_multiple_empty_workspace: "should NOT hot exit (reason: CLOSE, windows: multiple, empty workspace)", CLOSE, true, false, true;
    on_exit_scratchpad_should_hot_exit_quit_single_workspace: "should hot exit (reason: QUIT, windows: single, workspace)", QUIT, false, true, false;
    on_exit_scratchpad_should_hot_exit_quit_single_empty_workspace: "should hot exit (reason: QUIT, windows: single, empty workspace)", QUIT, false, false, false;
    on_exit_scratchpad_should_hot_exit_quit_multiple_workspace: "should hot exit (reason: QUIT, windows: multiple, workspace)", QUIT, true, true, false;
    on_exit_scratchpad_should_hot_exit_quit_multiple_empty_workspace: "should hot exit (reason: QUIT, windows: multiple, empty workspace)", QUIT, true, false, false;
    on_exit_scratchpad_should_hot_exit_reload_single_workspace: "should hot exit (reason: RELOAD, windows: single, workspace)", RELOAD, false, true, false;
    on_exit_scratchpad_should_hot_exit_reload_single_empty_workspace: "should hot exit (reason: RELOAD, windows: single, empty workspace)", RELOAD, false, false, false;
    on_exit_scratchpad_should_hot_exit_reload_multiple_workspace: "should hot exit (reason: RELOAD, windows: multiple, workspace)", RELOAD, true, true, false;
    on_exit_scratchpad_should_hot_exit_reload_multiple_empty_workspace: "should hot exit (reason: RELOAD, windows: multiple, empty workspace)", RELOAD, true, false, false;
    on_exit_scratchpad_should_hot_exit_load_single_workspace: "should hot exit (reason: LOAD, windows: single, workspace)", LOAD, false, true, false;
    on_exit_scratchpad_should_not_hot_exit_load_single_empty_workspace: "should NOT hot exit (reason: LOAD, windows: single, empty workspace)", LOAD, false, false, true;
    on_exit_scratchpad_should_hot_exit_load_multiple_workspace: "should hot exit (reason: LOAD, windows: multiple, workspace)", LOAD, true, true, false;
    on_exit_scratchpad_should_not_hot_exit_load_multiple_empty_workspace: "should NOT hot exit (reason: LOAD, windows: multiple, empty workspace)", LOAD, true, false, true;
}

hot_exit_cases! { scratchpad_hot_exit_test, "\"onExitAndWindowClose\" setting - scratchpad", HotExitConfiguration::ON_EXIT_AND_WINDOW_CLOSE;
    on_exit_and_window_close_scratchpad_should_hot_exit_close_single_workspace: "should hot exit (reason: CLOSE, windows: single, workspace)", CLOSE, false, true, false;
    on_exit_and_window_close_scratchpad_should_hot_exit_close_single_empty_workspace: "should hot exit (reason: CLOSE, windows: single, empty workspace)", CLOSE, false, false, is_macintosh();
    on_exit_and_window_close_scratchpad_should_hot_exit_close_multiple_workspace: "should hot exit (reason: CLOSE, windows: multiple, workspace)", CLOSE, true, true, false;
    on_exit_and_window_close_scratchpad_should_not_hot_exit_close_multiple_empty_workspace: "should NOT hot exit (reason: CLOSE, windows: multiple, empty workspace)", CLOSE, true, false, true;
    on_exit_and_window_close_scratchpad_should_hot_exit_quit_single_workspace: "should hot exit (reason: QUIT, windows: single, workspace)", QUIT, false, true, false;
    on_exit_and_window_close_scratchpad_should_hot_exit_quit_single_empty_workspace: "should hot exit (reason: QUIT, windows: single, empty workspace)", QUIT, false, false, false;
    on_exit_and_window_close_scratchpad_should_hot_exit_quit_multiple_workspace: "should hot exit (reason: QUIT, windows: multiple, workspace)", QUIT, true, true, false;
    on_exit_and_window_close_scratchpad_should_hot_exit_quit_multiple_empty_workspace: "should hot exit (reason: QUIT, windows: multiple, empty workspace)", QUIT, true, false, false;
    on_exit_and_window_close_scratchpad_should_hot_exit_reload_single_workspace: "should hot exit (reason: RELOAD, windows: single, workspace)", RELOAD, false, true, false;
    on_exit_and_window_close_scratchpad_should_hot_exit_reload_single_empty_workspace: "should hot exit (reason: RELOAD, windows: single, empty workspace)", RELOAD, false, false, false;
    on_exit_and_window_close_scratchpad_should_hot_exit_reload_multiple_workspace: "should hot exit (reason: RELOAD, windows: multiple, workspace)", RELOAD, true, true, false;
    on_exit_and_window_close_scratchpad_should_hot_exit_reload_multiple_empty_workspace: "should hot exit (reason: RELOAD, windows: multiple, empty workspace)", RELOAD, true, false, false;
    on_exit_and_window_close_scratchpad_should_hot_exit_load_single_workspace: "should hot exit (reason: LOAD, windows: single, workspace)", LOAD, false, true, false;
    on_exit_and_window_close_scratchpad_should_not_hot_exit_load_single_empty_workspace: "should NOT hot exit (reason: LOAD, windows: single, empty workspace)", LOAD, false, false, true;
    on_exit_and_window_close_scratchpad_should_hot_exit_load_multiple_workspace: "should hot exit (reason: LOAD, windows: multiple, workspace)", LOAD, true, true, false;
    on_exit_and_window_close_scratchpad_should_not_hot_exit_load_multiple_empty_workspace: "should NOT hot exit (reason: LOAD, windows: multiple, empty workspace)", LOAD, true, false, true;
}

//#endregion
