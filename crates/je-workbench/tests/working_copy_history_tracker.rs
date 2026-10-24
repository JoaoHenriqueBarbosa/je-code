//! Porte de `src/vs/workbench/services/workingCopy/test/electron-browser/workingCopyHistoryTracker.test.ts` (suíte `WorkingCopyHistoryTracker`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - **Infraestrutura.** O `TestWorkingCopyHistoryService` fica em `tests/support/working_copy_history_support.rs` (é o mesmo
//!   que o teste do serviço usa, como o `import` do original).
//! - **`createTracker()`.** O `UriIdentityService` é o real sobre um `TestFileService` (como no original), o `UndoRedoService` é
//!   o dos testes (`UndoRedoService::for_tests()`, sem confirmação) e o `TestPathService` tem o esquema padrão `file`.
//! - **`assert.fail` dentro do ouvinte de `onDidAddEntry`.** Lançar dentro do ouvinte rejeitaria só a cadeia da working copy
//!   (que ninguém aguarda); o ouvinte grava um sinalizador e o caso o confere depois que a segunda working copy foi salva,
//!   quando a entrada da primeira já teria aparecido.
//! - **`workingCopy.save(undefined, stat)`.** É `TestWorkingCopy::save_with_stat(None, Some(stat))`, que dispara o `onDidSave` com o
//!   `stat` que o `WorkingCopyService` repassa ao rastreador (`WorkingCopySaveEvent::stat`).
//! - **`Event.toPromise(...)`.** É [`Event::to_promise`]; a promessa é criada antes do movimento e aguardada depois.
//! - **`setup`, `teardown` e `ensureNoDisposablesAreLeakedInTestSuite()`.** O guarda de vazamentos é criado no começo do caso; o
//!   `teardown` (`fileService.del(testDir, { recursive: true })` e `disposables.clear()`) roda no fim do corpo, com o relógio
//!   real (o agendador de 5 minutos do serviço precisa estar descartado para o laço de timers terminar). `await` é
//!   `.await.unwrap()`.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
#[path = "support/working_copy_history_support.rs"]
mod history_support;

use std::cell::Cell;
use std::future::Future;
use std::rc::Rc;

use common::{ensure_no_disposables_are_leaked_in_test_suite, run_with_real_timers};
use history_support::{TestWorkingCopyHistoryService, assert_moved_entry, text, with_scheme};
use je_base::buffer::VSBuffer;
use je_base::cancellation::CancellationToken;
use je_base::dynamic::Dynamic;
use je_base::extpath::random_path;
use je_base::lifecycle::{DisposableStore, IDisposable};
use je_base::network::schemas;
use je_base::path::PathModule;
use je_base::promise::DeferredPromise;
use je_base::uri::URI;
use je_instantiation::Injectable;
use je_platform::configuration::test_configuration_service::TestConfigurationService;
use je_platform::files::file_service::{FileContentSource, FileService};
use je_platform::files::files::PartialFileDeleteOptions;
use je_platform::files::i_file_service::IFileService;
use je_platform::resources::{ext_uri, host_environment, join_path, uri_file};
use je_platform::uri_identity::uri_identity::IUriIdentityService;
use je_platform::uri_identity::uri_identity_service::UriIdentityService;
use je_editor::undo_redo_service::UndoRedoService;
use je_workbench::services::working_copy::working_copy::IWorkingCopy;
use je_workbench::services::working_copy::working_copy_history::{IWorkingCopyHistoryService, WorkingCopyHistoryEntryDescriptor, WorkingCopyHistoryEntryRef};
use je_workbench::services::working_copy::working_copy_history_tracker::WorkingCopyHistoryTracker;
use je_workbench::services::working_copy::working_copy_service::{IWorkingCopyService, WorkingCopyService};
use je_workbench::test::browser::workbench_test_services::TestPathService;
use je_workbench::test::common::workbench_test_services::{TestContextService, TestFileService, TestWorkingCopy};

const TEST_FILE1_PATH_CONTENTS: &str = "Hello Foo";

/// `[ 'Lorem ipsum ', 'dolor öäü sit amet ', 'adipiscing ßß elit', 'consectetur ' ].join('').repeat(1000)`.
fn test_file2_path_contents() -> String {
    "Lorem ipsum dolor öäü sit amet adipiscing ßß elitconsectetur ".repeat(1000)
}


/// `isEqual(a, b)`.
fn is_equal(a: &URI, b: &URI) -> bool {
    ext_uri().is_equal(Some(a), Some(b), false)
}

/// O estado do `setup` de cada caso.
struct Fixture {
    disposables: Rc<DisposableStore>,
    test_dir: URI,
    work_home: URI,
    working_copy_history_service: Rc<TestWorkingCopyHistoryService>,
    working_copy_service: Rc<WorkingCopyService>,
    file_service: Rc<FileService>,
    configuration_service: Rc<TestConfigurationService>,
    tracker: Rc<WorkingCopyHistoryTracker>,
    test_file1_path: URI,
    test_file2_path: URI,
    increasing_timestamp_counter: Cell<i64>,
}

impl Fixture {
    /// O `setup`.
    async fn setup(disposables: &Rc<DisposableStore>) -> Fixture {
        let environment = host_environment();
        let parent = PathModule::native(environment.as_ref()).join(&["vsctests", "workingcopyhistorytracker"]);
        let test_dir_path = random_path(Some(parent.as_str()), None, 8, environment.as_ref());
        let test_dir = with_scheme(&uri_file(&test_dir_path), schemas::IN_MEMORY);
        let history_home = join_path(&test_dir, &["User", "History"]);
        let work_home = join_path(&test_dir, &["work"]);

        let working_copy_history_service = disposables.add_rc(TestWorkingCopyHistoryService::new(disposables, None));
        let working_copy_service = disposables.add_rc(WorkingCopyService::new());
        let file_service = working_copy_history_service.file_service.clone();
        let configuration_service = working_copy_history_service.configuration_service.clone();

        let tracker = disposables.add_rc(create_tracker(disposables, &working_copy_service, &working_copy_history_service));

        file_service.create_folder(&history_home).await.unwrap();
        file_service.create_folder(&work_home).await.unwrap();

        let test_file1_path = join_path(&work_home, &["foo.txt"]);
        let test_file2_path = join_path(&work_home, &["bar.txt"]);

        file_service.write_file(&test_file1_path, FileContentSource::Buffer(VSBuffer::from_string(TEST_FILE1_PATH_CONTENTS)), None).await.unwrap();
        file_service.write_file(&test_file2_path, FileContentSource::Buffer(VSBuffer::from_string(&test_file2_path_contents())), None).await.unwrap();

        Fixture {
            disposables: disposables.clone(),
            test_dir,
            work_home,
            working_copy_history_service,
            working_copy_service,
            file_service,
            configuration_service,
            tracker,
            test_file1_path,
            test_file2_path,
            increasing_timestamp_counter: Cell::new(1),
        }
    }

    /// `tracker.dispose(); tracker = disposables.add(createTracker())`.
    fn recreate_tracker(&mut self) {
        self.tracker.dispose();
        self.tracker = self.disposables.add_rc(create_tracker(&self.disposables, &self.working_copy_service, &self.working_copy_history_service));
    }

    /// `addEntry(descriptor, token)`.
    async fn add_entry(&self, descriptor: WorkingCopyHistoryEntryDescriptor, token: CancellationToken) -> WorkingCopyHistoryEntryRef {
        // very important to get tests to not be flaky with stable sort order
        let timestamp = self.increasing_timestamp_counter.get();
        self.increasing_timestamp_counter.set(timestamp + 1);
        let entry = self.working_copy_history_service.add_entry(WorkingCopyHistoryEntryDescriptor { timestamp: Some(timestamp), ..descriptor }, token).await.unwrap();

        entry.expect("a entrada precisa estar definida")
    }

    /// `workingCopyHistoryService.getEntries(resource, CancellationToken.None)`.
    async fn entries(&self, resource: &URI) -> Vec<WorkingCopyHistoryEntryRef> {
        self.working_copy_history_service.get_entries(resource, CancellationToken::None).await.unwrap()
    }

    /// `disposables.add(new TestWorkingCopy(resource))`.
    fn working_copy(&self, resource: &URI) -> Rc<TestWorkingCopy> {
        self.disposables.add_rc(TestWorkingCopy::new(resource.clone(), None, None))
    }

    /// `workingCopyService.registerWorkingCopy(workingCopy)`, descartado no `teardown`.
    fn register_working_copy(&self, working_copy: &Rc<TestWorkingCopy>) {
        self.disposables.add(self.working_copy_service.register_working_copy(working_copy.clone()));
    }
}

/// `createTracker()`.
fn create_tracker(
    disposables: &Rc<DisposableStore>,
    working_copy_service: &Rc<WorkingCopyService>,
    working_copy_history_service: &Rc<TestWorkingCopyHistoryService>,
) -> Rc<WorkingCopyHistoryTracker> {
    let test_file_service = disposables.add_rc(TestFileService::new());
    let test_file_service_contract: Rc<dyn IFileService> = test_file_service;
    let uri_identity_service = disposables.add_rc(UriIdentityService::create((), (test_file_service_contract,)));
    let uri_identity_contract: Rc<dyn IUriIdentityService> = uri_identity_service;
    let file_service_contract: Rc<dyn IFileService> = working_copy_history_service.file_service.clone();

    WorkingCopyHistoryTracker::new(
        working_copy_service.clone(),
        working_copy_history_service.service(),
        uri_identity_contract,
        TestPathService::new(None, Some(schemas::FILE)),
        working_copy_history_service.configuration_service.clone(),
        UndoRedoService::for_tests(),
        TestContextService::new(None, None),
        file_service_contract,
    )
}

/// O caso: o `ensureNoDisposablesAreLeakedInTestSuite()`, o `setup`, o corpo e o `teardown`.
fn run_case<Fut: Future<Output = ()> + 'static>(body: impl FnOnce(Fixture) -> Fut + 'static) {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let disposables = guard.store();

    run_with_real_timers(async move {
        let fixture = Fixture::setup(&disposables).await;
        let (file_service, test_dir) = (fixture.file_service.clone(), fixture.test_dir.clone());
        body(fixture).await;

        // teardown
        file_service.del(&test_dir, Some(PartialFileDeleteOptions { recursive: Some(true), ..PartialFileDeleteOptions::default() })).await.unwrap();
        disposables.clear();
    });
}

#[test]
fn history_entry_added_on_save() {
    run_case(|fx| async move {
        let working_copy1 = fx.working_copy(&fx.test_file1_path);
        let working_copy2 = fx.working_copy(&fx.test_file2_path);

        let stat1 = fx.file_service.resolve_with_metadata(&working_copy1.resource(), None).await.unwrap();
        let stat2 = fx.file_service.resolve_with_metadata(&working_copy2.resource(), None).await.unwrap();

        fx.register_working_copy(&working_copy1);
        fx.register_working_copy(&working_copy2);

        let save_result = Rc::new(DeferredPromise::<()>::new());
        let added_counter = Rc::new(Cell::new(0));
        let (result, counter) = (save_result.clone(), added_counter.clone());
        let (resource1, resource2) = (working_copy1.resource(), working_copy2.resource());
        fx.disposables.add(fx.working_copy_history_service.on_did_add_entry().listen(move |e| {
            if is_equal(&e.entry.working_copy.resource, &resource1) || is_equal(&e.entry.working_copy.resource, &resource2) {
                counter.set(counter.get() + 1);

                if counter.get() == 2 {
                    let _ = result.complete(());
                }
            }
        }));

        working_copy1.save_with_stat(None, Some(stat1)).await.unwrap();
        working_copy2.save_with_stat(None, Some(stat2)).await.unwrap();

        save_result.promise.clone().await.unwrap();
    });
}

#[test]
fn history_entry_skipped_when_setting_disabled_globally() {
    run_case(|fx| async move {
        let _ = fx.configuration_service.set_user_configuration("workbench.localHistory.enabled", Dynamic::Bool(false), Some(&fx.test_file1_path));

        assert_no_local_history_entry_added_with_settings_configured(&fx).await;
    });
}

#[test]
fn history_entry_skipped_when_setting_disabled_exclude() {
    run_case(|mut fx| async move {
        let _ = fx.configuration_service.set_user_configuration("workbench.localHistory.exclude", Dynamic::object([("**/foo.txt", Dynamic::Bool(true))]), None);

        // Recreate to apply settings
        fx.recreate_tracker();

        assert_no_local_history_entry_added_with_settings_configured(&fx).await;
    });
}

#[test]
fn history_entry_skipped_when_too_large() {
    run_case(|fx| async move {
        let _ = fx.configuration_service.set_user_configuration("workbench.localHistory.maxFileSize", Dynamic::Number(0.0), Some(&fx.test_file1_path));

        assert_no_local_history_entry_added_with_settings_configured(&fx).await;
    });
}

async fn assert_no_local_history_entry_added_with_settings_configured(fx: &Fixture) {
    let working_copy1 = fx.working_copy(&fx.test_file1_path);
    let working_copy2 = fx.working_copy(&fx.test_file2_path);

    let stat1 = fx.file_service.resolve_with_metadata(&working_copy1.resource(), None).await.unwrap();
    let stat2 = fx.file_service.resolve_with_metadata(&working_copy2.resource(), None).await.unwrap();

    fx.register_working_copy(&working_copy1);
    fx.register_working_copy(&working_copy2);

    let save_result = Rc::new(DeferredPromise::<()>::new());
    let unexpected_entry = Rc::new(Cell::new(false));
    let (result, unexpected) = (save_result.clone(), unexpected_entry.clone());
    let (resource1, resource2) = (working_copy1.resource(), working_copy2.resource());
    fx.disposables.add(fx.working_copy_history_service.on_did_add_entry().listen(move |e| {
        if is_equal(&e.entry.working_copy.resource, &resource1) {
            unexpected.set(true); // assert.fail('Unexpected working copy history entry: ' + e.entry.workingCopy.resource.toString())
        }

        if is_equal(&e.entry.working_copy.resource, &resource2) {
            let _ = result.complete(());
        }
    }));

    working_copy1.save_with_stat(None, Some(stat1)).await.unwrap();
    working_copy2.save_with_stat(None, Some(stat2)).await.unwrap();

    save_result.promise.clone().await.unwrap();

    assert!(!unexpected_entry.get(), "Unexpected working copy history entry: {}", text(&working_copy1.resource()));
}

#[test]
fn entries_moved_file_rename() {
    run_case(|fx| async move {
        let entries_moved = fx.working_copy_history_service.on_did_move_entries().to_promise(None);

        let working_copy = fx.working_copy(&fx.test_file1_path);

        let entry1 = fx.add_entry(WorkingCopyHistoryEntryDescriptor::with_source(working_copy.resource(), "test-source"), CancellationToken::None).await;
        let entry2 = fx.add_entry(WorkingCopyHistoryEntryDescriptor::with_source(working_copy.resource(), "test-source"), CancellationToken::None).await;
        let entry3 = fx.add_entry(WorkingCopyHistoryEntryDescriptor::with_source(working_copy.resource(), "test-source"), CancellationToken::None).await;

        let mut entries = fx.entries(&working_copy.resource()).await;
        assert_eq!(entries.len(), 3);

        let renamed_working_copy_resource = join_path(&ext_uri().dirname(&working_copy.resource()), &["renamed.txt"]);
        fx.file_service.move_(&working_copy.resource(), &renamed_working_copy_resource, None).await.unwrap();

        entries_moved.await.unwrap();

        entries = fx.entries(&working_copy.resource()).await;
        assert_eq!(entries.len(), 0);

        entries = fx.entries(&renamed_working_copy_resource).await;
        assert_eq!(entries.len(), 4);

        assert_moved_entry(&entries[0], &entry1, &renamed_working_copy_resource);
        assert_moved_entry(&entries[1], &entry2, &renamed_working_copy_resource);
        assert_moved_entry(&entries[2], &entry3, &renamed_working_copy_resource);

        let all = fx.working_copy_history_service.get_all(CancellationToken::None).await.unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(text(&all[0]), text(&renamed_working_copy_resource));
    });
}

#[test]
fn entries_moved_folder_rename() {
    run_case(|fx| async move {
        let entries_moved = fx.working_copy_history_service.on_did_move_entries().to_promise(None);

        let working_copy1 = fx.working_copy(&fx.test_file1_path);
        let working_copy2 = fx.working_copy(&fx.test_file2_path);

        let entry1a = fx.add_entry(WorkingCopyHistoryEntryDescriptor::with_source(working_copy1.resource(), "test-source"), CancellationToken::None).await;
        let entry2a = fx.add_entry(WorkingCopyHistoryEntryDescriptor::with_source(working_copy1.resource(), "test-source"), CancellationToken::None).await;
        let entry3a = fx.add_entry(WorkingCopyHistoryEntryDescriptor::with_source(working_copy1.resource(), "test-source"), CancellationToken::None).await;

        let entry1b = fx.add_entry(WorkingCopyHistoryEntryDescriptor::with_source(working_copy2.resource(), "test-source"), CancellationToken::None).await;
        let entry2b = fx.add_entry(WorkingCopyHistoryEntryDescriptor::with_source(working_copy2.resource(), "test-source"), CancellationToken::None).await;
        let entry3b = fx.add_entry(WorkingCopyHistoryEntryDescriptor::with_source(working_copy2.resource(), "test-source"), CancellationToken::None).await;

        let mut entries = fx.entries(&working_copy1.resource()).await;
        assert_eq!(entries.len(), 3);

        entries = fx.entries(&working_copy2.resource()).await;
        assert_eq!(entries.len(), 3);

        let renamed_work_home = join_path(&ext_uri().dirname(&fx.test_dir), &["renamed"]);
        fx.file_service.move_(&fx.work_home, &renamed_work_home, None).await.unwrap();

        let renamed_working_copy1_resource = join_path(&renamed_work_home, &[&ext_uri().basename(&working_copy1.resource(), None)]);
        let renamed_working_copy2_resource = join_path(&renamed_work_home, &[&ext_uri().basename(&working_copy2.resource(), None)]);

        entries_moved.await.unwrap();

        entries = fx.entries(&working_copy1.resource()).await;
        assert_eq!(entries.len(), 0);
        entries = fx.entries(&working_copy2.resource()).await;
        assert_eq!(entries.len(), 0);

        entries = fx.entries(&renamed_working_copy1_resource).await;
        assert_eq!(entries.len(), 4);

        assert_moved_entry(&entries[0], &entry1a, &renamed_working_copy1_resource);
        assert_moved_entry(&entries[1], &entry2a, &renamed_working_copy1_resource);
        assert_moved_entry(&entries[2], &entry3a, &renamed_working_copy1_resource);

        entries = fx.entries(&renamed_working_copy2_resource).await;
        assert_eq!(entries.len(), 4);

        assert_moved_entry(&entries[0], &entry1b, &renamed_working_copy2_resource);
        assert_moved_entry(&entries[1], &entry2b, &renamed_working_copy2_resource);
        assert_moved_entry(&entries[2], &entry3b, &renamed_working_copy2_resource);

        let all = fx.working_copy_history_service.get_all(CancellationToken::None).await.unwrap();
        assert_eq!(all.len(), 2);
        for resource in &all {
    if text(resource) != text(&renamed_working_copy1_resource) && text(resource) != text(&renamed_working_copy2_resource) {
                panic!("Unexpected history resource: {}", text(resource));
            }
        }
    });
}
