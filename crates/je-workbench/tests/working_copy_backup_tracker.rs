//! Porte de `src/vs/workbench/services/workingCopy/test/browser/workingCopyBackupTracker.test.ts` (suíte `WorkingCopyBackupTracker (browser)`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - **Casos fora.** O `EditorPart` e o `EditorService` não existem no porte (só o `IEditorService` reduzido, com a classe
//!   de teste `TestEditorService`, cujo `getEditors()` é `[]`, o `openEditors` lança e o `isVisible` é `false`). Os casos
//!   `Track backups (untitled)` e `Track backups (untitled with initial contents)` (que abrem o editor sem título por
//!   `accessor.editorService.openEditor`) e os cinco `Restore backups (...)` (que contam `accessor.editorService.count`,
//!   `editors` e `isVisible` num `EditorService` real e abrem editores por `openEditors`) não são portados: um dublê do
//!   editor que inventasse esse comportamento não seria o oráculo. Entram quando o `EditorService` for portado.
//! - **`createTracker`.** O `createEditorPart`, o `EditorService` e o `registerTestResourceEditor` não são lidos pelo caso
//!   `Track backups (custom)` (ele só usa o rastreador, o serviço de backup e o serviço de working copies): o rastreador
//!   recebe o `TestEditorService`. O contêiner de injeção não entra: o [`TestServiceAccessor`] cria os serviços que o
//!   rastreador lê, e o rastreador recebe cada um por argumento.
//! - **Herança.** `TestWorkingCopyBackupTracker extends BrowserWorkingCopyBackupTracker` (que sobrescreve
//!   `getBackupScheduleDelay` para `10`) é o [`WorkingCopyBackupTracker`] com os [`TestWorkingCopyBackupTrackerHooks`], que
//!   herdam o `onFinalBeforeShutdown` do navegador; `pendingBackupOperationCount` é
//!   [`WorkingCopyBackupTracker::pending_backup_operation_count`]. `TestBackupWorkingCopy extends TestWorkingCopy` é o
//!   [`TestWorkingCopy`] com `set_backup_delay` e `set_backup_override`.
//! - **`InMemoryTestWorkingCopyBackupService`.** O `BrowserWorkingCopyBackupService` não foi portado: o
//!   [`InMemoryTestWorkingCopyBackupService`] guarda o [`WorkingCopyBackupService`] que ele estende, criado com a pasta
//!   `joinPath(userRoamingDataHome, 'Backups', workspace.id)` do construtor do original, sobre um `FileService` com dois
//!   `InMemoryFileSystemProvider` (esquemas `file` e `vscode-userdata`). `override backup` e `override discardBackup` são os
//!   métodos do contrato com os joiners (`joinBackupResource` e `joinDiscardBackup`).
//! - **`toResource.call(this, path)`.** `URI.file(join('/', btoa(this.test.fullTitle()), path))` com o título completo do caso.
//! - **`setup`, `teardown` e `ensureNoDisposablesAreLeakedInTestSuite()`.** O guarda de vazamentos é criado no começo do caso
//!   (o `disposables` do original é o store dele) e o fim do caso é o `disposables.clear()`, antes do guarda conferir os
//!   vazamentos. O caso roda em `run_with_faked_timers`; `await` é `.await.unwrap()`.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod support;

use std::cell::RefCell;
use std::rc::Rc;

use common::{ensure_no_disposables_are_leaked_in_test_suite, run_with_faked_timers, timeout};
use je_base::async_::async_fn;
use je_base::buffer::{VSBuffer, encode_base64};
use je_base::cancellation::CancellationToken;
use je_base::dynamic::Dynamic;
use je_base::lifecycle::{DisposableStore, IDisposable};
use je_base::network::schemas;
use je_base::path::PathModule;
use je_base::promise::{MaybePromise, Promise, Resolver};
use je_platform::environment::environment::EnvironmentService;
use je_platform::files::file_service::FileService;
use je_platform::files::i_file_service::IFileService;
use je_platform::files::in_memory::InMemoryFileSystemProvider;
use je_platform::log::{LogServiceRef, NullLogService};
use je_platform::resources::{host_environment, join_path, uri_file};
use je_platform::workspace::IWorkspaceContextService;
use je_workbench::services::lifecycle::lifecycle::ShutdownReason;
use je_workbench::services::working_copy::working_copy::{IWorkingCopy, WorkingCopyBackup, WorkingCopyBackupContent, WorkingCopyIdentifier};
use je_workbench::services::working_copy::working_copy_backup::{DiscardBackupsFilter, IWorkingCopyBackupService, ResolvedWorkingCopyBackup};
use je_workbench::services::working_copy::working_copy_backup_service::WorkingCopyBackupService;
use je_workbench::services::working_copy::working_copy_backup_tracker::{BrowserWorkingCopyBackupTrackerHooks, WorkingCopyBackupTracker, WorkingCopyBackupTrackerHooks};
use je_workbench::services::working_copy::working_copy_service::IWorkingCopyService;
use je_workbench::test::browser::workbench_test_services::TestEnvironmentService;
use je_workbench::test::common::workbench_test_services::{TestContextService, TestWorkingCopy};
use support::stored_file_working_copy_support::TestServiceAccessor;

/// O título da suíte, que o `this.test.fullTitle()` do `toResource` usa.
const SUITE_TITLE: &str = "WorkingCopyBackupTracker (browser)";

/// `toResource.call(this, path)`: `URI.file(join('/', btoa(this.test.fullTitle()), path))`.
fn to_resource(test_title: &str, path: &str) -> je_base::uri::URI {
    let full_title = format!("{SUITE_TITLE} {test_title}");
    let env = host_environment();

    uri_file(&PathModule::native(env.as_ref()).join(&["/", &encode_base64(&VSBuffer::from_string(&full_title), true, false), path]))
}

/// `InMemoryTestWorkingCopyBackupService`.
struct InMemoryTestWorkingCopyBackupService {
    inner: Rc<WorkingCopyBackupService>,
    backup_resource_joiners: Rc<RefCell<Vec<Resolver<()>>>>,
    discard_backup_joiners: Rc<RefCell<Vec<Resolver<()>>>>,
    /// `discardedBackups`.
    #[allow(dead_code)]
    discarded_backups: Rc<RefCell<Vec<WorkingCopyIdentifier>>>,
}

impl InMemoryTestWorkingCopyBackupService {
    fn new(disposables: &Rc<DisposableStore>) -> Rc<Self> {
        let environment_service = TestEnvironmentService::new();
        let log_service: LogServiceRef = NullLogService::shared();
        let file_service = disposables.add_rc(FileService::new(log_service.clone()));
        let file_provider = disposables.add_rc(InMemoryFileSystemProvider::new());
        disposables.add(file_service.register_provider(schemas::FILE, file_provider).unwrap());
        let vscode_user_data_provider = disposables.add_rc(InMemoryFileSystemProvider::new());
        disposables.add(file_service.register_provider(schemas::VSCODE_USER_DATA, vscode_user_data_provider).unwrap());

        let context_service = TestContextService::new(None, None);
        let backup_workspace_home = join_path(&environment_service.user_roaming_data_home(), &["Backups", &context_service.get_workspace().id()]);
        let file_service_contract: Rc<dyn IFileService> = file_service;

        Rc::new(Self {
            inner: WorkingCopyBackupService::new(Some(backup_workspace_home), file_service_contract, log_service),
            backup_resource_joiners: Rc::new(RefCell::new(Vec::new())),
            discard_backup_joiners: Rc::new(RefCell::new(Vec::new())),
            discarded_backups: Rc::new(RefCell::new(Vec::new())),
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
}

/// `while (joiners.length) { joiners.pop()!(); }`.
fn release_joiners(joiners: &Rc<RefCell<Vec<Resolver<()>>>>) {
    loop {
        let resolver = joiners.borrow_mut().pop();
        let Some(resolver) = resolver else { break };
        resolver.resolve(());
    }
}

impl IDisposable for InMemoryTestWorkingCopyBackupService {
    fn dispose(&self) {
        self.inner.dispose();
    }
}

impl IWorkingCopyBackupService for InMemoryTestWorkingCopyBackupService {
    fn has_backup_sync(&self, identifier: &WorkingCopyIdentifier, version_id: Option<u64>, meta: Option<&Dynamic>) -> bool {
        self.inner.has_backup_sync(identifier, version_id, meta)
    }

    fn get_backups(&self) -> Promise<Vec<WorkingCopyIdentifier>> {
        self.inner.get_backups()
    }

    fn resolve(&self, identifier: &WorkingCopyIdentifier) -> Promise<Option<ResolvedWorkingCopyBackup>> {
        self.inner.resolve(identifier)
    }

    fn backup(
        &self,
        identifier: &WorkingCopyIdentifier,
        content: Option<WorkingCopyBackupContent>,
        version_id: Option<u64>,
        meta: Option<Dynamic>,
        token: Option<CancellationToken>,
    ) -> Promise<()> {
        let backup = self.inner.backup(identifier, content, version_id, meta, token);
        let joiners = self.backup_resource_joiners.clone();

        async_fn(async move {
            backup.await?;

            release_joiners(&joiners);

            Ok(())
        })
    }

    fn discard_backup(&self, identifier: &WorkingCopyIdentifier, _token: Option<CancellationToken>) -> Promise<()> {
        let discard = self.inner.discard_backup(identifier, None);
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
        self.inner.discard_backups(filter)
    }
}

/// `TestWorkingCopyBackupTracker`: sobrescreve só o `getBackupScheduleDelay`.
struct TestWorkingCopyBackupTrackerHooks;

impl WorkingCopyBackupTrackerHooks for TestWorkingCopyBackupTrackerHooks {
    fn on_final_before_shutdown(&self, tracker: &WorkingCopyBackupTracker, reason: ShutdownReason) -> MaybePromise<bool> {
        BrowserWorkingCopyBackupTrackerHooks.on_final_before_shutdown(tracker, reason)
    }

    /// `return 10; // Reduce timeout for tests`.
    fn get_backup_schedule_delay(&self, _tracker: &WorkingCopyBackupTracker, _working_copy: &dyn IWorkingCopy) -> u64 {
        10
    }
}

/// `createTracker()`: o acessor, o rastreador e o serviço de backup.
fn create_tracker(disposables: &Rc<DisposableStore>) -> (TestServiceAccessor, Rc<WorkingCopyBackupTracker>, Rc<InMemoryTestWorkingCopyBackupService>) {
    let working_copy_backup_service = disposables.add_rc(InMemoryTestWorkingCopyBackupService::new(disposables));
    let accessor = TestServiceAccessor::new(disposables);

    let tracker = disposables.add_rc(WorkingCopyBackupTracker::new(
        working_copy_backup_service.clone(),
        accessor.working_copy_service.clone(),
        accessor.log_service.clone(),
        accessor.lifecycle_service.clone(),
        accessor.files_configuration_service.clone(),
        accessor.working_copy_editor_service.clone(),
        accessor.editor_service.clone(),
        Rc::new(TestWorkingCopyBackupTrackerHooks),
    ));

    (accessor, tracker, working_copy_backup_service)
}

/// `workingCopyBackupService.hasBackupSync(workingCopy)`.
fn has_backup_sync(service: &InMemoryTestWorkingCopyBackupService, working_copy: &TestWorkingCopy) -> bool {
    service.has_backup_sync(&WorkingCopyIdentifier::of(working_copy), None, None)
}

#[test]
fn track_backups_custom() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let disposables = guard.store();

    run_with_faked_timers(async move {
        let (accessor, tracker, working_copy_backup_service) = create_tracker(&disposables);

        // `class TestBackupWorkingCopy extends TestWorkingCopy`: registra a si mesma no construtor, com `backupDelay = 10`
        // e um `backup` que espera `timeout(0)` e devolve `{}`.
        let resource = to_resource("Track backups (custom)", "/path/custom.txt");
        let custom_working_copy = TestWorkingCopy::new(resource, None, None);
        custom_working_copy.set_backup_delay(Some(10));
        custom_working_copy.set_backup_override(Rc::new(|_token: &CancellationToken| {
            async_fn(async move {
                timeout(0).await?;

                Ok(WorkingCopyBackup::default())
            })
        }));
        disposables.add(accessor.working_copy_service.register_working_copy(custom_working_copy.clone()));
        disposables.add_rc(custom_working_copy.clone());

        // Normal
        custom_working_copy.set_dirty(true);
        assert_eq!(tracker.pending_backup_operation_count(), 1);
        working_copy_backup_service.join_backup_resource().await.unwrap();
        assert!(has_backup_sync(&working_copy_backup_service, &custom_working_copy));

        custom_working_copy.set_dirty(false);
        custom_working_copy.set_dirty(true);
        assert_eq!(tracker.pending_backup_operation_count(), 1);
        working_copy_backup_service.join_backup_resource().await.unwrap();
        assert!(has_backup_sync(&working_copy_backup_service, &custom_working_copy));

        custom_working_copy.set_dirty(false);
        assert_eq!(tracker.pending_backup_operation_count(), 1);
        working_copy_backup_service.join_discard_backup().await.unwrap();
        assert!(!has_backup_sync(&working_copy_backup_service, &custom_working_copy));

        // Cancellation
        custom_working_copy.set_dirty(true);
        timeout(0).await.unwrap();
        custom_working_copy.set_dirty(false);
        assert_eq!(tracker.pending_backup_operation_count(), 1);
        working_copy_backup_service.join_discard_backup().await.unwrap();
        assert!(!has_backup_sync(&working_copy_backup_service, &custom_working_copy));
    });
}
