//! Porte de `src/vs/workbench/services/workingCopy/test/browser/workingCopyBackupTracker.test.ts` (suíte `WorkingCopyBackupTracker (browser)`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - **`EditorPart` e `EditorService`.** Os oito casos usam o `EditorService` real sobre uma [`EditorPart`] lógica, montados pelo
//!   [`EditorServiceAccessor`] (`workbenchInstantiationService` mais `createEditorPart`; as divergências estão no cabeçalho de
//!   `support/editor_service_support.rs`). `accessor.editorService.count`, `editors`, `isVisible`, `openEditor` e `openEditors`
//!   são os métodos do [`IEditorService`] real.
//! - **`createTracker`.** Cria o serviço de backup em memória, o acessor (que registra no `setup` o painel de
//!   `registerTestResourceEditor`) e o rastreador, que recebe cada serviço por argumento (o contêiner de injeção não entra).
//!   O `IWorkingCopyEditorService` é o dublê do [`TestServiceAccessor`] (o real não foi portado; `findEditor` não é exercitado).
//! - **Herança.** `TestWorkingCopyBackupTracker extends BrowserWorkingCopyBackupTracker` (que sobrescreve
//!   `getBackupScheduleDelay` para `10`) é o [`WorkingCopyBackupTracker`] com os [`TestWorkingCopyBackupTrackerHooks`], que
//!   herdam o `onFinalBeforeShutdown` do navegador; `pendingBackupOperationCount` é
//!   [`WorkingCopyBackupTracker::pending_backup_operation_count`], `getUnrestoredBackups()` é `unrestored_backups()` (o
//!   `.size` é `.len()`) e `testRestoreBackups` é `restore_backups`. `TestBackupWorkingCopy extends TestWorkingCopy` é o
//!   [`TestWorkingCopy`] com `set_backup_delay` e `set_backup_override`. `TestUntitledTextEditorInput extends
//!   UntitledTextEditorInput` é a [`TestUntitledTextEditorInput`] (ver o suporte), e `instanceof` é o `downcast_ref` de `as_any`.
//! - **Handlers.** O objeto literal `IWorkingCopyEditorHandler` é o [`TestHandler`], com uma função por membro. O `throw new
//!   Error('unexpected')` de `createEditor` é uma promessa rejeitada (o `await` do restaurador a relança, como o `throw`
//!   síncrono de dentro de uma função `async`); o de `isOpen`, que devolve `boolean`, é `throw`.
//! - **`Promise.all`.** `await Promise.all([first, second])` aguarda as duas promessas, que já estão em andamento.
//! - **`InMemoryTestWorkingCopyBackupService`.** O `BrowserWorkingCopyBackupService` não foi portado: o
//!   [`InMemoryTestWorkingCopyBackupService`] guarda o [`WorkingCopyBackupService`] que ele estende, criado com a pasta
//!   `joinPath(userRoamingDataHome, 'Backups', workspace.id)` do construtor do original, sobre um `FileService` com dois
//!   `InMemoryFileSystemProvider` (esquemas `file` e `vscode-userdata`). `override backup` e `override discardBackup` são os
//!   métodos do contrato com os joiners (`joinBackupResource` e `joinDiscardBackup`).
//! - **`toResource.call(this, path)`.** `URI.file(join('/', btoa(this.test.fullTitle()), path))` com o título completo do caso.
//! - **`teardown`, `setup` e `ensureNoDisposablesAreLeakedInTestSuite()`.** O `teardown` é o `workbenchTeardown` do acessor
//!   ([`EditorServiceAccessor::teardown`]) no fim de cada caso, seguido do `disposables.clear()` (o fim do caso, antes de o guarda
//!   conferir os vazamentos). O caso roda em `run_with_faked_timers`; `await` é `.await.unwrap()`.
//! - **`isWindows`.** O ramo do Windows de `fooFile` e `barFile` não existe: o caminho é o do POSIX (`/Foo` e `/Bar`).

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod support;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use common::{ensure_no_disposables_are_leaked_in_test_suite, run_with_faked_timers, timeout};
use je_base::JsString;
use je_base::async_::async_fn;
use je_base::buffer::{VSBuffer, buffer_to_readable, encode_base64};
use je_base::cancellation::CancellationToken;
use je_base::dynamic::Dynamic;
use je_base::errors::{Error, throw};
use je_base::lifecycle::{DisposableStore, IDisposable};
use je_base::network::schemas;
use je_base::path::PathModule;
use je_base::promise::{MaybePromise, Promise, Resolver};
use je_base::stream::Readable;
use je_base::uri::{URI, UriComponents};
use je_editor::text_model::TextSource;
use je_instantiation::InstantiationService;
use je_platform::environment::environment::EnvironmentService;
use je_platform::files::file_service::FileService;
use je_platform::files::i_file_service::IFileService;
use je_platform::files::in_memory::InMemoryFileSystemProvider;
use je_platform::log::{LogServiceRef, NullLogService};
use je_platform::resources::{host_environment, join_path, uri_file};
use je_platform::workspace::IWorkspaceContextService;
use je_workbench::common::editor::editor_input::EditorInput;
use je_workbench::common::editor::{EditorInputRef, EditorInputWithOptions, UntypedEditorInput};
use je_workbench::services::editor::editor_service::IEditorService;
use je_workbench::services::lifecycle::lifecycle::{ILifecycleService, LifecyclePhase, ShutdownReason};
use je_workbench::services::textmodel_resolver::resolver_service::ITextEditorModel;
use je_workbench::services::untitled::untitled_text_editor_input::UntitledTextEditorInput;
use je_workbench::services::untitled::untitled_text_editor_service::{IUntitledTextEditorService, UntitledTextEditorOptions, UntitledTextEditorService};
use je_workbench::services::working_copy::working_copy::{IWorkingCopy, WorkingCopyBackup, WorkingCopyBackupContent, WorkingCopyIdentifier};
use je_workbench::services::working_copy::working_copy_backup::{DiscardBackupsFilter, IWorkingCopyBackupService, ResolvedWorkingCopyBackup};
use je_workbench::services::working_copy::working_copy_backup_service::WorkingCopyBackupService;
use je_workbench::services::working_copy::working_copy_backup_tracker::{BrowserWorkingCopyBackupTrackerHooks, WorkingCopyBackupTracker, WorkingCopyBackupTrackerHooks};
use je_workbench::services::working_copy::working_copy_editor_service::IWorkingCopyEditorHandler;
use je_workbench::services::working_copy::working_copy_service::IWorkingCopyService;
use je_workbench::test::browser::workbench_test_services::TestEnvironmentService;
use je_workbench::test::common::workbench_test_services::{TestContextService, TestWorkingCopy};
use support::editor_service_support::{EditorServiceAccessor, TestUntitledTextEditorInput, register_test_resource_editor};

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
struct TrackerFixture {
    accessor: EditorServiceAccessor,
    tracker: Rc<WorkingCopyBackupTracker>,
    working_copy_backup_service: Rc<InMemoryTestWorkingCopyBackupService>,
}

/// `workbenchInstantiationService(undefined, disposables)` com o serviço de backup em memória, o `createEditorPart` e o
/// `EditorService` real (o `setup` registra o painel de `registerTestResourceEditor`).
fn create_accessor(disposables: &Rc<DisposableStore>, working_copy_backup_service: &Rc<InMemoryTestWorkingCopyBackupService>) -> EditorServiceAccessor {
    disposables.add(register_test_resource_editor());

    EditorServiceAccessor::new(disposables, working_copy_backup_service.clone())
}

/// `instantiationService.createInstance(TestWorkingCopyBackupTracker)`.
fn create_test_tracker(
    disposables: &Rc<DisposableStore>,
    accessor: &EditorServiceAccessor,
    working_copy_backup_service: &Rc<InMemoryTestWorkingCopyBackupService>,
) -> Rc<WorkingCopyBackupTracker> {
    disposables.add_rc(WorkingCopyBackupTracker::new(
        working_copy_backup_service.clone(),
        accessor.working_copy_service.clone(),
        accessor.log_service.clone(),
        accessor.lifecycle_service.clone(),
        accessor.files_configuration_service.clone(),
        accessor.working_copy_editor_service.clone(),
        accessor.editor_service.clone(),
        Rc::new(TestWorkingCopyBackupTrackerHooks),
    ))
}

fn create_tracker(disposables: &Rc<DisposableStore>) -> TrackerFixture {
    let working_copy_backup_service = disposables.add_rc(InMemoryTestWorkingCopyBackupService::new(disposables));
    let accessor = create_accessor(disposables, &working_copy_backup_service);
    let tracker = create_test_tracker(disposables, &accessor, &working_copy_backup_service);

    TrackerFixture { accessor, tracker, working_copy_backup_service }
}

/// `workingCopyBackupService.hasBackupSync(workingCopy)`.
fn has_backup_sync(service: &InMemoryTestWorkingCopyBackupService, working_copy: &dyn IWorkingCopy) -> bool {
    service.has_backup_sync(&WorkingCopyIdentifier::of(working_copy), None, None)
}

#[test]
fn track_backups_custom() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let disposables = guard.store();

    run_with_faked_timers(async move {
        let TrackerFixture { accessor, tracker, working_copy_backup_service } = create_tracker(&disposables);

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
        assert!(has_backup_sync(&working_copy_backup_service, custom_working_copy.as_ref()));

        custom_working_copy.set_dirty(false);
        custom_working_copy.set_dirty(true);
        assert_eq!(tracker.pending_backup_operation_count(), 1);
        working_copy_backup_service.join_backup_resource().await.unwrap();
        assert!(has_backup_sync(&working_copy_backup_service, custom_working_copy.as_ref()));

        custom_working_copy.set_dirty(false);
        assert_eq!(tracker.pending_backup_operation_count(), 1);
        working_copy_backup_service.join_discard_backup().await.unwrap();
        assert!(!has_backup_sync(&working_copy_backup_service, custom_working_copy.as_ref()));

        // Cancellation
        custom_working_copy.set_dirty(true);
        timeout(0).await.unwrap();
        custom_working_copy.set_dirty(false);
        assert_eq!(tracker.pending_backup_operation_count(), 1);
        working_copy_backup_service.join_discard_backup().await.unwrap();
        assert!(!has_backup_sync(&working_copy_backup_service, custom_working_copy.as_ref()));

        accessor.teardown().await.unwrap();
    });
}

/// `URI.from({ scheme: Schemas.untitled, path })`.
fn untitled_resource_of(path: &str) -> URI {
    URI::from(&UriComponents { scheme: Some(JsString::from(schemas::UNTITLED)), path: Some(JsString::from(path)), ..UriComponents::default() }, false)
}

/// `untitledBackupTest(untitled)`.
async fn untitled_backup_test(disposables: Rc<DisposableStore>, untitled: UntypedEditorInput) {
    let TrackerFixture { accessor, working_copy_backup_service, tracker: _tracker } = create_tracker(&disposables);

    let pane = IEditorService::open_editor(accessor.editor_service.as_ref(), untitled.clone()).await.unwrap();
    let untitled_text_editor_ref = pane.expect("o editor abriu").input().expect("o painel tem a entrada");
    disposables.add(untitled_text_editor_ref.clone());
    let untitled_text_editor = untitled_text_editor_ref.as_any().downcast_ref::<UntitledTextEditorInput>().expect("a entrada é sem título");
    let untitled_text_model = untitled_text_editor.resolve_untitled().await.unwrap();
    disposables.add(untitled_text_model.clone());

    if untitled.contents.as_deref().is_none_or(str::is_empty) {
        if let Some(text_model) = untitled_text_model.text_editor_model() {
            text_model.set_value(TextSource::from("Super Good"));
        }
    }

    working_copy_backup_service.join_backup_resource().await.unwrap();

    assert!(has_backup_sync(&working_copy_backup_service, &*untitled_text_model));

    untitled_text_model.dispose();

    working_copy_backup_service.join_discard_backup().await.unwrap();

    assert!(!has_backup_sync(&working_copy_backup_service, &*untitled_text_model));

    accessor.teardown().await.unwrap();
}

#[test]
fn track_backups_untitled() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let disposables = guard.store();

    run_with_faked_timers(async move {
        untitled_backup_test(disposables, UntypedEditorInput::default()).await;
    });
}

#[test]
fn track_backups_untitled_with_initial_contents() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let disposables = guard.store();

    run_with_faked_timers(async move {
        untitled_backup_test(disposables, UntypedEditorInput { contents: Some("Foo Bar".to_string()), ..UntypedEditorInput::default() }).await;
    });
}

/// O `typeId` padrão de `toTypedWorkingCopyId`.
const TEST_BACKUP_TYPE_ID: &str = "testBackupTypeId";

/// `toTypedWorkingCopyId(resource, typeId)`.
fn to_typed_working_copy_id(resource: &URI, type_id: &str) -> WorkingCopyIdentifier {
    WorkingCopyIdentifier { type_id: type_id.to_string(), resource: resource.clone() }
}

/// `toUntypedWorkingCopyId(resource)`: `toTypedWorkingCopyId(resource, '')`.
fn to_untyped_working_copy_id(resource: &URI) -> WorkingCopyIdentifier {
    to_typed_working_copy_id(resource, "")
}

/// `bufferToReadable(VSBuffer.fromString(text))`.
fn backup_content(text: &str) -> WorkingCopyBackupContent {
    let readable: Box<dyn Readable<VSBuffer>> = Box::new(buffer_to_readable(VSBuffer::from_string(text)));

    WorkingCopyBackupContent::Readable(Rc::new(RefCell::new(readable)))
}

/// `disposables.add(accessor.instantiationService.createInstance(TestUntitledTextEditorInput,
/// accessor.untitledTextEditorService.create({ initialValue: 'foo' })))`.
#[derive(Clone)]
struct EditorInputFactory {
    disposables: Rc<DisposableStore>,
    untitled_text_editor_service: Rc<UntitledTextEditorService>,
    instantiation_service: Rc<InstantiationService>,
}

impl EditorInputFactory {
    fn new(disposables: &Rc<DisposableStore>, accessor: &EditorServiceAccessor) -> Self {
        Self {
            disposables: disposables.clone(),
            untitled_text_editor_service: accessor.untitled_text_editor_service.clone(),
            instantiation_service: accessor.instantiation_service.clone(),
        }
    }

    fn create(&self) -> Rc<TestUntitledTextEditorInput> {
        let model = IUntitledTextEditorService::create(
            self.untitled_text_editor_service.as_ref(),
            Some(UntitledTextEditorOptions { initial_value: Some("foo".to_string()), ..UntitledTextEditorOptions::default() }),
        );

        self.disposables.add_rc(TestUntitledTextEditorInput::new(&self.instantiation_service, model))
    }

    /// `createEditor: workingCopy => <editor de teste>`.
    fn create_editor(&self) -> MaybePromise<EditorInputRef> {
        let editor: EditorInputRef = self.create();

        MaybePromise::Value(editor)
    }
}

type HandlesFn = Box<dyn Fn(&WorkingCopyIdentifier) -> bool>;
type IsOpenFn = Box<dyn Fn(&WorkingCopyIdentifier, &EditorInputRef) -> bool>;
type CreateEditorFn = Box<dyn Fn(&WorkingCopyIdentifier) -> MaybePromise<EditorInputRef>>;

/// O objeto literal `IWorkingCopyEditorHandler` dos casos (`handles`, `isOpen` e `createEditor`).
struct TestHandler {
    handles: HandlesFn,
    is_open: IsOpenFn,
    create_editor: CreateEditorFn,
}

impl IWorkingCopyEditorHandler for TestHandler {
    fn handles(&self, working_copy: &WorkingCopyIdentifier) -> MaybePromise<bool> {
        MaybePromise::Value((self.handles)(working_copy))
    }

    fn is_open(&self, working_copy: &WorkingCopyIdentifier, editor: &EditorInputRef) -> bool {
        (self.is_open)(working_copy, editor)
    }

    fn create_editor(&self, working_copy: &WorkingCopyIdentifier) -> MaybePromise<EditorInputRef> {
        (self.create_editor)(working_copy)
    }
}

/// `handles: workingCopy => <contador> e typeId === 'testBackupTypeId'`: conta cada chamada, e `matches` decide.
fn counting_handles(counter: &Rc<Cell<usize>>, matches: impl Fn(&WorkingCopyIdentifier) -> bool + 'static) -> HandlesFn {
    let counter = counter.clone();

    Box::new(move |working_copy: &WorkingCopyIdentifier| {
        counter.set(counter.get() + 1);

        matches(working_copy)
    })
}

/// `isOpen: (workingCopy, editor) => { <contador>++; return result; }`.
fn counting_is_open(counter: &Rc<Cell<usize>>, result: bool) -> IsOpenFn {
    let counter = counter.clone();

    Box::new(move |_working_copy: &WorkingCopyIdentifier, _editor: &EditorInputRef| {
        counter.set(counter.get() + 1);

        result
    })
}

/// `isOpen: (workingCopy, editor) => { throw new Error('unexpected'); }`.
fn unexpected_is_open() -> IsOpenFn {
    Box::new(|_working_copy: &WorkingCopyIdentifier, _editor: &EditorInputRef| -> bool { throw(Error::new("unexpected")) })
}

/// `createEditor: workingCopy => { throw new Error('unexpected'); }`.
fn unexpected_create_editor() -> CreateEditorFn {
    Box::new(|_working_copy: &WorkingCopyIdentifier| MaybePromise::Promise(Promise::rejected(Error::new("unexpected"))))
}

/// `restoreBackupsInit()`: dois arquivos e dois sem título com backup, o rastreador e o acessor com o ciclo de vida restaurado.
async fn restore_backups_init(disposables: &Rc<DisposableStore>) -> (Rc<WorkingCopyBackupTracker>, EditorServiceAccessor) {
    let foo_file = uri_file("/Foo");
    let bar_file = uri_file("/Bar");
    let untitled_file_1 = untitled_resource_of("Untitled-1");
    let untitled_file_2 = untitled_resource_of("Untitled-2");

    let working_copy_backup_service = disposables.add_rc(InMemoryTestWorkingCopyBackupService::new(disposables));
    let accessor = create_accessor(disposables, &working_copy_backup_service);

    // Backup 2 normal files and 2 untitled files
    let untitled_file_1_working_copy_id = to_untyped_working_copy_id(&untitled_file_1);
    let untitled_file_2_working_copy_id = to_typed_working_copy_id(&untitled_file_2, TEST_BACKUP_TYPE_ID);
    working_copy_backup_service.backup(&untitled_file_1_working_copy_id, Some(backup_content("untitled-1")), None, None, None).await.unwrap();
    working_copy_backup_service.backup(&untitled_file_2_working_copy_id, Some(backup_content("untitled-2")), None, None, None).await.unwrap();

    let foo_file_working_copy_id = to_untyped_working_copy_id(&foo_file);
    let bar_file_working_copy_id = to_typed_working_copy_id(&bar_file, TEST_BACKUP_TYPE_ID);
    working_copy_backup_service.backup(&foo_file_working_copy_id, Some(backup_content("fooFile")), None, None, None).await.unwrap();
    working_copy_backup_service.backup(&bar_file_working_copy_id, Some(backup_content("barFile")), None, None, None).await.unwrap();

    let tracker = create_test_tracker(disposables, &accessor, &working_copy_backup_service);

    accessor.lifecycle_service.set_phase(LifecyclePhase::Restored);

    (tracker, accessor)
}

#[test]
fn restore_backups_basics_some_handled() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let disposables = guard.store();

    run_with_faked_timers(async move {
        let (tracker, accessor) = restore_backups_init(&disposables).await;

        assert_eq!(tracker.unrestored_backups().len(), 0);

        let handles_counter = Rc::new(Cell::new(0));
        let is_open_counter = Rc::new(Cell::new(0));
        let create_editor_counter = Rc::new(Cell::new(0));
        let factory = EditorInputFactory::new(&disposables, &accessor);

        tracker
            .restore_backups(Rc::new(TestHandler {
                handles: counting_handles(&handles_counter, |working_copy| working_copy.type_id == TEST_BACKUP_TYPE_ID),
                is_open: counting_is_open(&is_open_counter, false),
                create_editor: Box::new({
                    let create_editor_counter = create_editor_counter.clone();
                    move |_working_copy: &WorkingCopyIdentifier| {
                        create_editor_counter.set(create_editor_counter.get() + 1);

                        factory.create_editor()
                    }
                }),
            }))
            .await
            .unwrap();

        assert_eq!(handles_counter.get(), 4);
        assert_eq!(is_open_counter.get(), 0);
        assert_eq!(create_editor_counter.get(), 2);

        assert_eq!(accessor.editor_service.count(), 2);
        assert!(accessor.editor_service.editors().iter().all(|editor| editor.is_dirty()));
        assert_eq!(tracker.unrestored_backups().len(), 2);

        for editor in accessor.editor_service.editors() {
            let editor = editor.as_any().downcast_ref::<TestUntitledTextEditorInput>().expect("o editor é um TestUntitledTextEditorInput");
            assert!(editor.resolved());
        }

        accessor.teardown().await.unwrap();
    });
}

#[test]
fn restore_backups_basics_none_handled() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let disposables = guard.store();

    run_with_faked_timers(async move {
        let (tracker, accessor) = restore_backups_init(&disposables).await;

        tracker
            .restore_backups(Rc::new(TestHandler {
                handles: Box::new(|_working_copy: &WorkingCopyIdentifier| false),
                is_open: unexpected_is_open(),
                create_editor: unexpected_create_editor(),
            }))
            .await
            .unwrap();

        assert_eq!(accessor.editor_service.count(), 0);
        assert_eq!(tracker.unrestored_backups().len(), 4);

        accessor.teardown().await.unwrap();
    });
}

#[test]
fn restore_backups_basics_error_case() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let disposables = guard.store();

    run_with_faked_timers(async move {
        let (tracker, accessor) = restore_backups_init(&disposables).await;

        // `try { await ... } catch (error) { // ignore }`
        let _ignored = tracker
            .restore_backups(Rc::new(TestHandler {
                handles: Box::new(|_working_copy: &WorkingCopyIdentifier| true),
                is_open: unexpected_is_open(),
                create_editor: unexpected_create_editor(),
            }))
            .await;

        assert_eq!(tracker.unrestored_backups().len(), 4);

        accessor.teardown().await.unwrap();
    });
}

#[test]
fn restore_backups_multiple_handlers() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let disposables = guard.store();

    run_with_faked_timers(async move {
        let (tracker, accessor) = restore_backups_init(&disposables).await;
        let factory = EditorInputFactory::new(&disposables, &accessor);

        let first_handler = tracker.restore_backups(Rc::new(TestHandler {
            handles: Box::new(|working_copy: &WorkingCopyIdentifier| working_copy.type_id == TEST_BACKUP_TYPE_ID),
            is_open: Box::new(|_working_copy: &WorkingCopyIdentifier, _editor: &EditorInputRef| false),
            create_editor: Box::new({
                let factory = factory.clone();
                move |_working_copy: &WorkingCopyIdentifier| factory.create_editor()
            }),
        }));

        let second_handler = tracker.restore_backups(Rc::new(TestHandler {
            handles: Box::new(|working_copy: &WorkingCopyIdentifier| working_copy.type_id.is_empty()),
            is_open: Box::new(|_working_copy: &WorkingCopyIdentifier, _editor: &EditorInputRef| false),
            create_editor: Box::new(move |_working_copy: &WorkingCopyIdentifier| factory.create_editor()),
        }));

        // `await Promise.all([firstHandler, secondHandler])`
        first_handler.await.unwrap();
        second_handler.await.unwrap();

        assert_eq!(accessor.editor_service.count(), 4);
        assert!(accessor.editor_service.editors().iter().all(|editor| editor.is_dirty()));
        assert_eq!(tracker.unrestored_backups().len(), 0);

        for editor in accessor.editor_service.editors() {
            let editor = editor.as_any().downcast_ref::<TestUntitledTextEditorInput>().expect("o editor é um TestUntitledTextEditorInput");
            assert!(editor.resolved());
        }

        accessor.teardown().await.unwrap();
    });
}

#[test]
fn restore_backups_editors_already_opened() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let disposables = guard.store();

    run_with_faked_timers(async move {
        let (tracker, accessor) = restore_backups_init(&disposables).await;

        assert_eq!(tracker.unrestored_backups().len(), 0);

        let handles_counter = Rc::new(Cell::new(0));
        let is_open_counter = Rc::new(Cell::new(0));
        let factory = EditorInputFactory::new(&disposables, &accessor);

        let editor1 = factory.create();
        let editor2 = factory.create();

        let editor1_ref: EditorInputRef = editor1.clone();
        let editor2_ref: EditorInputRef = editor2.clone();
        IEditorService::open_editors(
            accessor.editor_service.as_ref(),
            vec![EditorInputWithOptions { editor: editor1_ref, options: None }, EditorInputWithOptions { editor: editor2_ref, options: None }],
        )
        .await
        .unwrap();

        editor1.set_resolved(false);
        editor2.set_resolved(false);

        tracker
            .restore_backups(Rc::new(TestHandler {
                handles: counting_handles(&handles_counter, |working_copy| working_copy.type_id == TEST_BACKUP_TYPE_ID),
                is_open: counting_is_open(&is_open_counter, true),
                create_editor: unexpected_create_editor(),
            }))
            .await
            .unwrap();

        assert_eq!(handles_counter.get(), 4);
        assert_eq!(is_open_counter.get(), 4);

        assert_eq!(accessor.editor_service.count(), 2);
        assert_eq!(tracker.unrestored_backups().len(), 2);

        for editor in accessor.editor_service.editors() {
            let is_visible = accessor.editor_service.is_visible(&editor);
            let editor = editor.as_any().downcast_ref::<TestUntitledTextEditorInput>().expect("o editor é um TestUntitledTextEditorInput");

            // assert that we only call `resolve` on inactive editors
            if is_visible {
                assert!(!editor.resolved());
            } else {
                assert!(editor.resolved());
            }
        }

        accessor.teardown().await.unwrap();
    });
}
