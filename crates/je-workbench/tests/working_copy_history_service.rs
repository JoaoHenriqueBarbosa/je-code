//! Porte de `src/vs/workbench/services/workingCopy/test/electron-browser/workingCopyHistoryService.test.ts` (suíte `WorkingCopyHistoryService`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - **Infraestrutura.** O `TestWorkingCopyHistoryService` e o `TestWillShutdownEvent` ficam em
//!   `tests/support/working_copy_history_support.rs` (o teste do rastreador também os usa). `Promise.allSettled(event.value)`
//!   depois de `fireWillShutdown(event)` é `simulate_shutdown(...).await`.
//! - **Sistema de arquivos.** Como no original, o serviço roda sobre um `FileService` com dois `InMemoryFileSystemProvider`
//!   (esquemas `inmemory` e `vscode-userdata`); o diretório do caso é `URI.file(join(generateUuid(), 'vsctests',
//!   'workingcopyhistoryservice'))` com o esquema `inmemory`.
//! - **`addEntry` do teste.** É uma função `async` que já chama `service.addEntry` de forma síncrona (o caso do cancelamento
//!   depende disso): [`Fixture::add_entry_promise`] é um `async_fn`, que roda o corpo até o primeiro `await` como o JavaScript.
//!   Os dois overloads (`expectEntryAdded` verdadeiro, que devolve a entrada e confere o arquivo, e `false`, que pode devolver
//!   `undefined`) são [`Fixture::add_entry`] e [`Fixture::add_entry_unchecked`].
//! - **`assert.notStrictEqual(entries[n].location, entryN.location)`.** Compara a identidade dos objetos `URI`; aqui é a
//!   identidade das entradas (`Rc::ptr_eq`): a entrada movida é um objeto novo mesmo quando a pasta não muda.
//! - **`assert.ok(!entries[n].sourceDescription)` e `assert.ok(entries[n].sourceDescription)`.** São a ausência (ou texto
//!   vazio) e a presença de texto não vazio em `source_description()`.
//! - **`setUserConfiguration(key, value)`.** O valor é um [`Dynamic`] (`undefined` é `Dynamic::Undefined`).
//! - **`setup`, `teardown` e `ensureNoDisposablesAreLeakedInTestSuite()`.** O guarda de vazamentos é criado no começo do caso e o
//!   `disposables.clear()` do `teardown` roda no fim do corpo (o relógio é o real, como no mocha, e o agendador de 5 minutos do
//!   serviço precisa estar descartado para o laço de timers terminar). `await` é `.await.unwrap()`.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
#[path = "support/working_copy_history_support.rs"]
mod history_support;

use std::cell::{Cell, RefCell};
use std::future::Future;
use std::rc::Rc;

use common::{ensure_no_disposables_are_leaked_in_test_suite, run_with_real_timers};
use history_support::{TestWorkingCopyHistoryService, assert_moved_entry, simulate_shutdown, text, with_scheme};
use je_base::async_::async_fn;
use je_base::buffer::VSBuffer;
use je_base::cancellation::{CancellationToken, CancellationTokenSource};
use je_base::dynamic::Dynamic;
use je_base::lifecycle::{DisposableStore, IDisposable};
use je_base::network::schemas;
use je_base::path::PathModule;
use je_base::promise::Promise;
use je_base::uri::URI;
use je_base::uuid::generate_uuid;
use je_platform::files::file_service::{FileContentSource, FileService};
use je_platform::resources::{ext_uri, host_environment, join_path, uri_file};
use je_workbench::services::working_copy::working_copy::IWorkingCopy;
use je_workbench::services::working_copy::working_copy_history::{
    IWorkingCopyHistoryService, WorkingCopyHistoryEntryDescriptor, WorkingCopyHistoryEntryProperties, WorkingCopyHistoryEntryRef, WorkingCopyHistoryEvent,
};
use je_workbench::test::common::workbench_test_services::TestWorkingCopy;

const TEST_FILE1_PATH_CONTENTS: &str = "Hello Foo";
const TEST_FILE2_PATH_CONTENTS: &str = "Lorem ipsum dolor öäü sit amet adipiscing ßß elitconsectetur ";
const TEST_FILE3_PATH_CONTENTS: &str = "Hello Bar";

/// `(await fileService.readFile(resource)).value.toString()`.
async fn read_text(file_service: &Rc<FileService>, resource: &URI) -> String {
    let content = file_service.read_file(resource, None, None).await.unwrap();

    content.value.with_bytes(|bytes| String::from_utf8_lossy(bytes).into_owned())
}

/// `service._configurationService.setUserConfiguration(key, value)`.
fn set_user_configuration(service: &TestWorkingCopyHistoryService, key: &str, value: Dynamic) {
    let _ = service.configuration_service.set_user_configuration(key, value, None);
}

/// Os recursos de `resources` conferidos contra os permitidos (`assert.fail` para um inesperado).
fn assert_only_resources(resources: &[URI], allowed: &[&URI]) {
    for resource in resources {
        if !allowed.iter().any(|allowed| text(allowed) == text(resource)) {
            panic!("Unexpected history resource: {}", text(resource));
        }
    }
}

/// `assertEntryEqual(entryA, entryB, assertTimestamp = true)`.
fn assert_entry_equal(entry_a: &WorkingCopyHistoryEntryRef, entry_b: &WorkingCopyHistoryEntryRef, assert_timestamp: bool) {
    assert_eq!(entry_a.id, entry_b.id);
    assert_eq!(text(&entry_a.location), text(&entry_b.location));
    if assert_timestamp {
        assert_eq!(entry_a.timestamp(), entry_b.timestamp());
    }
    assert_eq!(entry_a.source(), entry_b.source());
    assert_eq!(entry_a.working_copy.name, entry_b.working_copy.name);
    assert_eq!(text(&entry_a.working_copy.resource), text(&entry_b.working_copy.resource));
}

/// `!sourceDescription`.
fn has_no_source_description(entry: &WorkingCopyHistoryEntryRef) -> bool {
    entry.source_description().is_none_or(|description| description.is_empty())
}

/// O estado do `setup` de cada caso.
struct Fixture {
    disposables: Rc<DisposableStore>,
    service: Rc<TestWorkingCopyHistoryService>,
    file_service: Rc<FileService>,
    work_home: URI,
    test_file1_path: URI,
    test_file2_path: URI,
    test_file3_path: URI,
    increasing_timestamp_counter: Rc<Cell<i64>>,
}

impl Fixture {
    /// O `setup`.
    async fn setup(disposables: &Rc<DisposableStore>) -> Fixture {
        let environment = host_environment();
        let test_dir_path = PathModule::native(environment.as_ref()).join(&[&generate_uuid(), "vsctests", "workingcopyhistoryservice"]);
        let test_dir = with_scheme(&uri_file(&test_dir_path), schemas::IN_MEMORY);
        let history_home = join_path(&test_dir, &["User", "History"]);
        let work_home = join_path(&test_dir, &["work"]);

        let service = disposables.add_rc(TestWorkingCopyHistoryService::new(disposables, None));
        let file_service = service.file_service.clone();

        file_service.create_folder(&history_home).await.unwrap();
        file_service.create_folder(&work_home).await.unwrap();

        let test_file1_path = join_path(&work_home, &["foo.txt"]);
        let test_file2_path = join_path(&work_home, &["bar.txt"]);
        let test_file3_path = join_path(&work_home, &["foo-bar.txt"]);

        for (path, contents) in [(&test_file1_path, TEST_FILE1_PATH_CONTENTS), (&test_file2_path, TEST_FILE2_PATH_CONTENTS), (&test_file3_path, TEST_FILE3_PATH_CONTENTS)] {
            file_service.write_file(path, FileContentSource::Buffer(VSBuffer::from_string(contents)), None).await.unwrap();
        }

        Fixture {
            disposables: disposables.clone(),
            service,
            file_service,
            work_home,
            test_file1_path,
            test_file2_path,
            test_file3_path,
            increasing_timestamp_counter: Rc::new(Cell::new(1)),
        }
    }

    /// `addEntry(descriptor, token, expectEntryAdded)`: o `service.addEntry` roda já, de forma síncrona, e a promessa
    /// resolve com a entrada (conferida no sistema de arquivos quando `expect_entry_added`).
    fn add_entry_promise(&self, descriptor: WorkingCopyHistoryEntryDescriptor, token: CancellationToken, expect_entry_added: bool) -> Promise<Option<WorkingCopyHistoryEntryRef>> {
        let (service, file_service, counter) = (self.service.clone(), self.file_service.clone(), self.increasing_timestamp_counter.clone());

        async_fn(async move {
            // very important to get tests to not be flaky with stable sort order
            let timestamp = counter.get();
            counter.set(timestamp + 1);
            let entry = service.add_entry(WorkingCopyHistoryEntryDescriptor { timestamp: Some(timestamp), ..descriptor }, token).await?;

            if expect_entry_added {
                let entry = entry.as_ref().expect("Unexpected undefined local history entry");
                assert!(file_service.exists(&entry.location).await?, "Unexpected local history not stored");
            }

            Ok(entry)
        })
    }

    /// `addEntry(descriptor, token)` (com `expectEntryAdded` verdadeiro).
    async fn add_entry(&self, descriptor: WorkingCopyHistoryEntryDescriptor, token: CancellationToken) -> WorkingCopyHistoryEntryRef {
        self.add_entry_promise(descriptor, token, true).await.unwrap().expect("Unexpected undefined local history entry")
    }

    /// `addEntry(descriptor, token, false)`.
    async fn add_entry_unchecked(&self, descriptor: WorkingCopyHistoryEntryDescriptor, token: CancellationToken) -> Option<WorkingCopyHistoryEntryRef> {
        self.add_entry_promise(descriptor, token, false).await.unwrap()
    }

    /// `disposables.add(new TestWorkingCopy(resource))`.
    fn working_copy(&self, resource: &URI) -> Rc<TestWorkingCopy> {
        self.disposables.add_rc(TestWorkingCopy::new(resource.clone(), None, None))
    }

    /// `getEntries(resource, CancellationToken.None)`.
    async fn entries(&self, resource: &URI) -> Vec<WorkingCopyHistoryEntryRef> {
        self.service.get_entries(resource, CancellationToken::None).await.unwrap()
    }

    /// `Simulate shutdown`.
    async fn simulate_shutdown(&self) {
        simulate_shutdown(&self.service.lifecycle_service).await.unwrap();
    }

    /// `Resolve from file service fresh and verify again`: `service.dispose(); service = disposables.add(new
    /// TestWorkingCopyHistoryService(disposables, fileService))`.
    fn resolve_from_file_service_fresh(&mut self) {
        self.service.dispose();
        self.service = self.disposables.add_rc(TestWorkingCopyHistoryService::new(&self.disposables, Some(self.file_service.clone())));
    }
}

/// O caso: o `ensureNoDisposablesAreLeakedInTestSuite()`, o `setup`, o corpo e o `teardown`.
fn run_case<Fut: Future<Output = ()> + 'static>(body: impl FnOnce(Fixture) -> Fut + 'static) {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let disposables = guard.store();

    run_with_real_timers(async move {
        let fixture = Fixture::setup(&disposables).await;
        body(fixture).await;

        // teardown
        disposables.clear();
    });
}

#[test]
fn add_entry() {
    run_case(|fx| async move {
        let add_events: Rc<RefCell<Vec<WorkingCopyHistoryEvent>>> = Rc::default();
        let events = add_events.clone();
        fx.disposables.add(fx.service.on_did_add_entry().listen(move |e| events.borrow_mut().push(e.clone())));

        let working_copy1 = fx.working_copy(&fx.test_file1_path);
        let working_copy2 = fx.working_copy(&fx.test_file2_path);

        // Add Entry works

        let entry1a = fx.add_entry(WorkingCopyHistoryEntryDescriptor::new(working_copy1.resource()), CancellationToken::None).await;
        let entry2a = fx.add_entry(WorkingCopyHistoryEntryDescriptor::with_source(working_copy2.resource(), "My Source"), CancellationToken::None).await;

        assert_eq!(read_text(&fx.file_service, &entry1a.location).await, TEST_FILE1_PATH_CONTENTS);
        assert_eq!(read_text(&fx.file_service, &entry2a.location).await, TEST_FILE2_PATH_CONTENTS);

        assert_eq!(add_events.borrow().len(), 2);
        assert_eq!(text(&add_events.borrow()[0].entry.working_copy.resource), text(&working_copy1.resource()));
        assert_eq!(text(&add_events.borrow()[1].entry.working_copy.resource), text(&working_copy2.resource()));
        assert_eq!(add_events.borrow()[1].entry.source(), "My Source");

        let entry1b = fx.add_entry(WorkingCopyHistoryEntryDescriptor::new(working_copy1.resource()), CancellationToken::None).await;
        let entry2b = fx.add_entry(WorkingCopyHistoryEntryDescriptor::new(working_copy2.resource()), CancellationToken::None).await;

        assert_eq!(read_text(&fx.file_service, &entry1b.location).await, TEST_FILE1_PATH_CONTENTS);
        assert_eq!(read_text(&fx.file_service, &entry2b.location).await, TEST_FILE2_PATH_CONTENTS);

        assert_eq!(add_events.borrow().len(), 4);
        assert_eq!(text(&add_events.borrow()[2].entry.working_copy.resource), text(&working_copy1.resource()));
        assert_eq!(text(&add_events.borrow()[3].entry.working_copy.resource), text(&working_copy2.resource()));

        // Cancellation works

        let cts = CancellationTokenSource::default();
        let entry1c_promise = fx.add_entry_promise(WorkingCopyHistoryEntryDescriptor::new(working_copy1.resource()), cts.token(), false);
        cts.dispose_with(true);

        let entry1c = entry1c_promise.await.unwrap();
        assert!(entry1c.is_none());

        assert_eq!(add_events.borrow().len(), 4);

        // Invalid working copies are ignored

        let working_copy3 = fx.working_copy(&with_scheme(&fx.test_file2_path, "unsupported"));
        let entry3a = fx.add_entry_unchecked(WorkingCopyHistoryEntryDescriptor::new(working_copy3.resource()), CancellationToken::None).await;
        assert!(entry3a.is_none());

        assert_eq!(add_events.borrow().len(), 4);
    });
}

#[test]
fn rename_entry() {
    run_case(|mut fx| async move {
        let change_events: Rc<RefCell<Vec<WorkingCopyHistoryEvent>>> = Rc::default();
        let events = change_events.clone();
        fx.disposables.add(fx.service.on_did_change_entry().listen(move |e| events.borrow_mut().push(e.clone())));

        let working_copy1 = fx.working_copy(&fx.test_file1_path);

        let entry = fx.add_entry(WorkingCopyHistoryEntryDescriptor::new(working_copy1.resource()), CancellationToken::None).await;
        fx.add_entry(WorkingCopyHistoryEntryDescriptor::new(working_copy1.resource()), CancellationToken::None).await;
        fx.add_entry(WorkingCopyHistoryEntryDescriptor::with_source(working_copy1.resource(), "My Source"), CancellationToken::None).await;

        let mut entries = fx.entries(&working_copy1.resource()).await;
        assert_eq!(entries.len(), 3);

        fx.service.update_entry(&entry, WorkingCopyHistoryEntryProperties { source: "Hello Rename".to_string() }, CancellationToken::None).await.unwrap();

        assert_eq!(change_events.borrow().len(), 1);
        assert!(Rc::ptr_eq(&change_events.borrow()[0].entry, &entry));

        entries = fx.entries(&working_copy1.resource()).await;
        assert_eq!(entries[0].source(), "Hello Rename");

        // Simulate shutdown
        fx.simulate_shutdown().await;

        // Resolve from file service fresh and verify again

        fx.resolve_from_file_service_fresh();

        entries = fx.entries(&working_copy1.resource()).await;
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].source(), "Hello Rename");
    });
}

#[test]
fn remove_entry() {
    run_case(|mut fx| async move {
        let remove_events: Rc<RefCell<Vec<WorkingCopyHistoryEvent>>> = Rc::default();
        let events = remove_events.clone();
        fx.disposables.add(fx.service.on_did_remove_entry().listen(move |e| events.borrow_mut().push(e.clone())));

        let working_copy1 = fx.working_copy(&fx.test_file1_path);

        fx.add_entry(WorkingCopyHistoryEntryDescriptor::new(working_copy1.resource()), CancellationToken::None).await;
        let entry2 = fx.add_entry(WorkingCopyHistoryEntryDescriptor::new(working_copy1.resource()), CancellationToken::None).await;
        fx.add_entry(WorkingCopyHistoryEntryDescriptor::new(working_copy1.resource()), CancellationToken::None).await;
        fx.add_entry(WorkingCopyHistoryEntryDescriptor::with_source(working_copy1.resource(), "My Source"), CancellationToken::None).await;

        let mut entries = fx.entries(&working_copy1.resource()).await;
        assert_eq!(entries.len(), 4);

        let mut removed = fx.service.remove_entry(&entry2, CancellationToken::None).await.unwrap();
        assert!(removed);

        assert_eq!(remove_events.borrow().len(), 1);
        assert!(Rc::ptr_eq(&remove_events.borrow()[0].entry, &entry2));

        // Cannot remove same entry again
        removed = fx.service.remove_entry(&entry2, CancellationToken::None).await.unwrap();
        assert!(!removed);

        entries = fx.entries(&working_copy1.resource()).await;
        assert_eq!(entries.len(), 3);

        // Simulate shutdown
        fx.simulate_shutdown().await;

        // Resolve from file service fresh and verify again

        fx.resolve_from_file_service_fresh();

        entries = fx.entries(&working_copy1.resource()).await;
        assert_eq!(entries.len(), 3);
    });
}

#[test]
fn remove_entry_deletes_history_entries_folder_when_last_entry_removed() {
    run_case(|mut fx| async move {
        let working_copy1 = fx.working_copy(&fx.test_file1_path);

        let mut entry = Some(fx.add_entry(WorkingCopyHistoryEntryDescriptor::new(working_copy1.resource()), CancellationToken::None).await);

        // Simulate shutdown
        fx.simulate_shutdown().await;

        // Resolve from file service fresh and verify again

        fx.resolve_from_file_service_fresh();

        assert!(fx.file_service.exists(&ext_uri().dirname(&entry.as_ref().unwrap().location)).await.unwrap());

        entry = fx.entries(&working_copy1.resource()).await.first().cloned();
        let entry = entry.expect("a entrada precisa existir");

        fx.service.remove_entry(&entry, CancellationToken::None).await.unwrap();

        // Simulate shutdown
        fx.simulate_shutdown().await;

        // Resolve from file service fresh and verify again

        fx.resolve_from_file_service_fresh();

        assert!(!fx.file_service.exists(&ext_uri().dirname(&entry.location)).await.unwrap());
    });
}

#[test]
fn remove_all() {
    run_case(|mut fx| async move {
        let removed = Rc::new(Cell::new(false));
        let flag = removed.clone();
        fx.disposables.add(fx.service.on_did_remove_entries().listen(move |_| flag.set(true)));

        let working_copy1 = fx.working_copy(&fx.test_file1_path);
        let working_copy2 = fx.working_copy(&fx.test_file2_path);

        fx.add_entry(WorkingCopyHistoryEntryDescriptor::new(working_copy1.resource()), CancellationToken::None).await;
        fx.add_entry(WorkingCopyHistoryEntryDescriptor::new(working_copy1.resource()), CancellationToken::None).await;
        fx.add_entry(WorkingCopyHistoryEntryDescriptor::new(working_copy2.resource()), CancellationToken::None).await;
        fx.add_entry(WorkingCopyHistoryEntryDescriptor::with_source(working_copy2.resource(), "My Source"), CancellationToken::None).await;

        let mut entries = fx.entries(&working_copy1.resource()).await;
        assert_eq!(entries.len(), 2);
        entries = fx.entries(&working_copy2.resource()).await;
        assert_eq!(entries.len(), 2);

        fx.service.remove_all(CancellationToken::None).await.unwrap();

        assert!(removed.get());

        entries = fx.entries(&working_copy1.resource()).await;
        assert_eq!(entries.len(), 0);
        entries = fx.entries(&working_copy2.resource()).await;
        assert_eq!(entries.len(), 0);

        // Simulate shutdown
        fx.simulate_shutdown().await;

        // Resolve from file service fresh and verify again

        fx.resolve_from_file_service_fresh();

        entries = fx.entries(&working_copy1.resource()).await;
        assert_eq!(entries.len(), 0);
        entries = fx.entries(&working_copy2.resource()).await;
        assert_eq!(entries.len(), 0);
    });
}

#[test]
fn get_entries_simple() {
    run_case(|fx| async move {
        let working_copy1 = fx.working_copy(&fx.test_file1_path);
        let working_copy2 = fx.working_copy(&fx.test_file2_path);

        let mut entries = fx.entries(&working_copy1.resource()).await;
        assert_eq!(entries.len(), 0);

        let entry1 = fx.add_entry(WorkingCopyHistoryEntryDescriptor::with_source(working_copy1.resource(), "test-source"), CancellationToken::None).await;

        entries = fx.entries(&working_copy1.resource()).await;
        assert_eq!(entries.len(), 1);
        assert_entry_equal(&entries[0], &entry1, true);

        let entry2 = fx.add_entry(WorkingCopyHistoryEntryDescriptor::with_source(working_copy1.resource(), "test-source"), CancellationToken::None).await;

        entries = fx.entries(&working_copy1.resource()).await;
        assert_eq!(entries.len(), 2);
        assert_entry_equal(&entries[1], &entry2, true);

        entries = fx.entries(&working_copy2.resource()).await;
        assert_eq!(entries.len(), 0);

        let entry3 = fx.add_entry(WorkingCopyHistoryEntryDescriptor::with_source(working_copy2.resource(), "other-test-source"), CancellationToken::None).await;

        entries = fx.entries(&working_copy2.resource()).await;
        assert_eq!(entries.len(), 1);
        assert_entry_equal(&entries[0], &entry3, true);
    });
}

#[test]
fn get_entries_metadata_preserved_when_stored() {
    run_case(|mut fx| async move {
        let working_copy1 = fx.working_copy(&fx.test_file1_path);
        let working_copy2 = fx.working_copy(&fx.test_file2_path);

        let entry1 = fx.add_entry(WorkingCopyHistoryEntryDescriptor::with_source(working_copy1.resource(), "test-source"), CancellationToken::None).await;
        let entry2 = fx.add_entry(WorkingCopyHistoryEntryDescriptor::new(working_copy2.resource()), CancellationToken::None).await;
        let entry3 = fx.add_entry(WorkingCopyHistoryEntryDescriptor::with_source(working_copy2.resource(), "other-source"), CancellationToken::None).await;

        // Simulate shutdown
        fx.simulate_shutdown().await;

        // Resolve from file service fresh and verify again

        fx.resolve_from_file_service_fresh();

        let mut entries = fx.entries(&working_copy1.resource()).await;
        assert_eq!(entries.len(), 1);
        assert_entry_equal(&entries[0], &entry1, true);

        entries = fx.entries(&working_copy2.resource()).await;
        assert_eq!(entries.len(), 2);
        assert_entry_equal(&entries[0], &entry2, true);
        assert_entry_equal(&entries[1], &entry3, true);
    });
}

#[test]
fn get_entries_corrupt_meta_json_is_no_problem() {
    run_case(|mut fx| async move {
        let working_copy1 = fx.working_copy(&fx.test_file1_path);

        let entry1 = fx.add_entry(WorkingCopyHistoryEntryDescriptor::new(working_copy1.resource()), CancellationToken::None).await;

        // Simulate shutdown
        fx.simulate_shutdown().await;

        // Resolve from file service fresh and verify again

        fx.resolve_from_file_service_fresh();

        let meta_file = join_path(&ext_uri().dirname(&entry1.location), &["entries.json"]);
        assert!(fx.file_service.exists(&meta_file).await.unwrap());
        fx.file_service.del(&meta_file, None).await.unwrap();

        let entries = fx.entries(&working_copy1.resource()).await;
        assert_eq!(entries.len(), 1);
        assert_entry_equal(&entries[0], &entry1, false /* skip timestamp that is unreliable when entries.json is gone */);
    });
}

#[test]
fn get_entries_missing_entries_from_meta_json_is_no_problem() {
    run_case(|mut fx| async move {
        let working_copy1 = fx.working_copy(&fx.test_file1_path);

        let entry1 = fx.add_entry(WorkingCopyHistoryEntryDescriptor::new(working_copy1.resource()), CancellationToken::None).await;
        let entry2 = fx.add_entry(WorkingCopyHistoryEntryDescriptor::new(working_copy1.resource()), CancellationToken::None).await;

        // Simulate shutdown
        fx.simulate_shutdown().await;

        // Resolve from file service fresh and verify again

        fx.resolve_from_file_service_fresh();

        fx.file_service.del(&entry1.location, None).await.unwrap();

        let entries = fx.entries(&working_copy1.resource()).await;
        assert_eq!(entries.len(), 1);
        assert_entry_equal(&entries[0], &entry2, true);
    });
}

#[test]
fn get_entries_in_memory_and_on_disk_entries_are_merged() {
    run_case(|mut fx| async move {
        let working_copy1 = fx.working_copy(&fx.test_file1_path);

        let entry1 = fx.add_entry(WorkingCopyHistoryEntryDescriptor::with_source(working_copy1.resource(), "test-source"), CancellationToken::None).await;
        let entry2 = fx.add_entry(WorkingCopyHistoryEntryDescriptor::with_source(working_copy1.resource(), "other-source"), CancellationToken::None).await;

        // Simulate shutdown
        fx.simulate_shutdown().await;

        // Resolve from file service fresh and verify again

        fx.resolve_from_file_service_fresh();

        let entry3 = fx.add_entry(WorkingCopyHistoryEntryDescriptor::with_source(working_copy1.resource(), "test-source"), CancellationToken::None).await;
        let entry4 = fx.add_entry(WorkingCopyHistoryEntryDescriptor::with_source(working_copy1.resource(), "other-source"), CancellationToken::None).await;

        let entries = fx.entries(&working_copy1.resource()).await;
        assert_eq!(entries.len(), 4);
        assert_entry_equal(&entries[0], &entry1, true);
        assert_entry_equal(&entries[1], &entry2, true);
        assert_entry_equal(&entries[2], &entry3, true);
        assert_entry_equal(&entries[3], &entry4, true);
    });
}

#[test]
fn get_entries_configured_max_entries_respected() {
    run_case(|fx| async move {
        let working_copy1 = fx.working_copy(&fx.test_file1_path);

        fx.add_entry(WorkingCopyHistoryEntryDescriptor::new(working_copy1.resource()), CancellationToken::None).await;
        fx.add_entry(WorkingCopyHistoryEntryDescriptor::new(working_copy1.resource()), CancellationToken::None).await;
        let entry3 = fx.add_entry(WorkingCopyHistoryEntryDescriptor::with_source(working_copy1.resource(), "Test source"), CancellationToken::None).await;
        let entry4 = fx.add_entry(WorkingCopyHistoryEntryDescriptor::new(working_copy1.resource()), CancellationToken::None).await;

        set_user_configuration(&fx.service, "workbench.localHistory.maxFileEntries", Dynamic::Number(2.0));

        let mut entries = fx.entries(&working_copy1.resource()).await;
        assert_eq!(entries.len(), 2);
        assert_entry_equal(&entries[0], &entry3, true);
        assert_entry_equal(&entries[1], &entry4, true);

        set_user_configuration(&fx.service, "workbench.localHistory.maxFileEntries", Dynamic::Number(4.0));

        entries = fx.entries(&working_copy1.resource()).await;
        assert_eq!(entries.len(), 4);

        set_user_configuration(&fx.service, "workbench.localHistory.maxFileEntries", Dynamic::Number(5.0));

        entries = fx.entries(&working_copy1.resource()).await;
        assert_eq!(entries.len(), 4);
    });
}

#[test]
fn get_all() {
    run_case(|mut fx| async move {
        let working_copy1 = fx.working_copy(&fx.test_file1_path);
        let working_copy2 = fx.working_copy(&fx.test_file2_path);

        let mut resources = fx.service.get_all(CancellationToken::None).await.unwrap();
        assert_eq!(resources.len(), 0);

        fx.add_entry(WorkingCopyHistoryEntryDescriptor::with_source(working_copy1.resource(), "test-source"), CancellationToken::None).await;
        fx.add_entry(WorkingCopyHistoryEntryDescriptor::with_source(working_copy1.resource(), "test-source"), CancellationToken::None).await;
        fx.add_entry(WorkingCopyHistoryEntryDescriptor::with_source(working_copy2.resource(), "test-source"), CancellationToken::None).await;
        fx.add_entry(WorkingCopyHistoryEntryDescriptor::with_source(working_copy2.resource(), "test-source"), CancellationToken::None).await;

        resources = fx.service.get_all(CancellationToken::None).await.unwrap();
        assert_eq!(resources.len(), 2);
        assert_only_resources(&resources, &[&working_copy1.resource(), &working_copy2.resource()]);

        // Simulate shutdown
        fx.simulate_shutdown().await;

        // Resolve from file service fresh and verify again

        fx.resolve_from_file_service_fresh();

        let working_copy3 = fx.working_copy(&fx.test_file3_path);
        fx.add_entry(WorkingCopyHistoryEntryDescriptor::with_source(working_copy3.resource(), "test-source"), CancellationToken::None).await;

        resources = fx.service.get_all(CancellationToken::None).await.unwrap();
        assert_eq!(resources.len(), 3);
        assert_only_resources(&resources, &[&working_copy1.resource(), &working_copy2.resource(), &working_copy3.resource()]);
    });
}

#[test]
fn get_all_ignores_resource_when_no_entries_exist() {
    run_case(|mut fx| async move {
        let working_copy1 = fx.working_copy(&fx.test_file1_path);

        let entry = fx.add_entry(WorkingCopyHistoryEntryDescriptor::with_source(working_copy1.resource(), "test-source"), CancellationToken::None).await;

        let mut resources = fx.service.get_all(CancellationToken::None).await.unwrap();
        assert_eq!(resources.len(), 1);

        fx.service.remove_entry(&entry, CancellationToken::None).await.unwrap();

        resources = fx.service.get_all(CancellationToken::None).await.unwrap();
        assert_eq!(resources.len(), 0);

        // Simulate shutdown
        fx.simulate_shutdown().await;

        // Resolve from file service fresh and verify again

        fx.resolve_from_file_service_fresh();

        resources = fx.service.get_all(CancellationToken::None).await.unwrap();
        assert_eq!(resources.len(), 0);
    });
}

#[test]
fn entries_cleaned_up_on_shutdown() {
    run_case(|mut fx| async move {
        let working_copy1 = fx.working_copy(&fx.test_file1_path);

        let entry1 = fx.add_entry(WorkingCopyHistoryEntryDescriptor::with_source(working_copy1.resource(), "test-source"), CancellationToken::None).await;
        let entry2 = fx.add_entry(WorkingCopyHistoryEntryDescriptor::with_source(working_copy1.resource(), "other-source"), CancellationToken::None).await;
        let entry3 = fx.add_entry(WorkingCopyHistoryEntryDescriptor::with_source(working_copy1.resource(), "other-source"), CancellationToken::None).await;
        let entry4 = fx.add_entry(WorkingCopyHistoryEntryDescriptor::with_source(working_copy1.resource(), "other-source"), CancellationToken::None).await;

        set_user_configuration(&fx.service, "workbench.localHistory.maxFileEntries", Dynamic::Number(2.0));

        // Simulate shutdown
        fx.simulate_shutdown().await;

        assert!(!fx.file_service.exists(&entry1.location).await.unwrap());
        assert!(!fx.file_service.exists(&entry2.location).await.unwrap());
        assert!(fx.file_service.exists(&entry3.location).await.unwrap());
        assert!(fx.file_service.exists(&entry4.location).await.unwrap());

        // Resolve from file service fresh and verify again

        fx.resolve_from_file_service_fresh();

        let mut entries = fx.entries(&working_copy1.resource()).await;
        assert_eq!(entries.len(), 2);
        assert_entry_equal(&entries[0], &entry3, true);
        assert_entry_equal(&entries[1], &entry4, true);

        set_user_configuration(&fx.service, "workbench.localHistory.maxFileEntries", Dynamic::Number(3.0));

        let entry5 = fx.add_entry(WorkingCopyHistoryEntryDescriptor::with_source(working_copy1.resource(), "other-source"), CancellationToken::None).await;

        // Simulate shutdown
        fx.simulate_shutdown().await;

        assert!(fx.file_service.exists(&entry3.location).await.unwrap());
        assert!(fx.file_service.exists(&entry4.location).await.unwrap());
        assert!(fx.file_service.exists(&entry5.location).await.unwrap());

        // Resolve from file service fresh and verify again

        fx.resolve_from_file_service_fresh();

        entries = fx.entries(&working_copy1.resource()).await;
        assert_eq!(entries.len(), 3);
        assert_entry_equal(&entries[0], &entry3, true);
        assert_entry_equal(&entries[1], &entry4, true);
        assert_entry_equal(&entries[2], &entry5, true);
    });
}

#[test]
fn entries_are_merged_when_source_is_same() {
    run_case(|fx| async move {
        let replaced: Rc<RefCell<Option<WorkingCopyHistoryEntryRef>>> = Rc::default();
        let replaced_entry = replaced.clone();
        fx.disposables.add(fx.service.on_did_replace_entry().listen(move |e| *replaced_entry.borrow_mut() = Some(e.entry.clone())));

        let working_copy1 = fx.working_copy(&fx.test_file1_path);

        set_user_configuration(&fx.service, "workbench.localHistory.mergeWindow", Dynamic::Number(1.0));

        let entry1 = fx.add_entry(WorkingCopyHistoryEntryDescriptor::with_source(working_copy1.resource(), "test-source"), CancellationToken::None).await;
        assert!(replaced.borrow().is_none());

        let entry2 = fx.add_entry(WorkingCopyHistoryEntryDescriptor::with_source(working_copy1.resource(), "test-source"), CancellationToken::None).await;
        assert!(Rc::ptr_eq(replaced.borrow().as_ref().unwrap(), &entry1));

        let entry3 = fx.add_entry(WorkingCopyHistoryEntryDescriptor::with_source(working_copy1.resource(), "test-source"), CancellationToken::None).await;
        assert!(Rc::ptr_eq(replaced.borrow().as_ref().unwrap(), &entry2));

        let mut entries = fx.entries(&working_copy1.resource()).await;
        assert_eq!(entries.len(), 1);
        assert_entry_equal(&entries[0], &entry3, true);

        set_user_configuration(&fx.service, "workbench.localHistory.mergeWindow", Dynamic::Undefined);

        fx.add_entry(WorkingCopyHistoryEntryDescriptor::with_source(working_copy1.resource(), "test-source"), CancellationToken::None).await;
        fx.add_entry(WorkingCopyHistoryEntryDescriptor::with_source(working_copy1.resource(), "test-source"), CancellationToken::None).await;

        entries = fx.entries(&working_copy1.resource()).await;
        assert_eq!(entries.len(), 3);
    });
}

#[test]
fn move_entries_file_rename() {
    run_case(|fx| async move {
        let working_copy = fx.working_copy(&fx.test_file1_path);

        let entry1 = fx.add_entry(WorkingCopyHistoryEntryDescriptor::with_source(working_copy.resource(), "test-source"), CancellationToken::None).await;
        let entry2 = fx.add_entry(WorkingCopyHistoryEntryDescriptor::with_source(working_copy.resource(), "test-source"), CancellationToken::None).await;
        let entry3 = fx.add_entry(WorkingCopyHistoryEntryDescriptor::with_source(working_copy.resource(), "test-source"), CancellationToken::None).await;

        let mut entries = fx.entries(&working_copy.resource()).await;
        assert_eq!(entries.len(), 3);

        let renamed_working_copy_resource = join_path(&ext_uri().dirname(&working_copy.resource()), &["renamed.txt"]);
        fx.file_service.move_(&working_copy.resource(), &renamed_working_copy_resource, None).await.unwrap();

        let result = fx.service.move_entries(&working_copy.resource(), &renamed_working_copy_resource).await.unwrap();

        assert_eq!(result.len(), 1);
        assert_eq!(text(&result[0]), text(&renamed_working_copy_resource));

        entries = fx.entries(&working_copy.resource()).await;
        assert_eq!(entries.len(), 0);

        entries = fx.entries(&renamed_working_copy_resource).await;
        assert_eq!(entries.len(), 4);

        assert_moved_entry(&entries[0], &entry1, &renamed_working_copy_resource);
        assert!(has_no_source_description(&entries[0]));

        assert_moved_entry(&entries[1], &entry2, &renamed_working_copy_resource);
        assert!(has_no_source_description(&entries[1]));

        assert_moved_entry(&entries[2], &entry3, &renamed_working_copy_resource);
        assert!(has_no_source_description(&entries[2]));

        assert_eq!(entries[3].source(), "renamed.source" /* for the move */);
        assert!(!has_no_source_description(&entries[3])); // contains the source working copy path

        let all = fx.service.get_all(CancellationToken::None).await.unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(text(&all[0]), text(&renamed_working_copy_resource));
    });
}

#[test]
fn entries_moved_folder_rename() {
    run_case(|fx| async move {
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

        let renamed_work_home = join_path(&ext_uri().dirname(&fx.work_home), &["renamed"]);
        fx.file_service.move_(&fx.work_home, &renamed_work_home, None).await.unwrap();

        let resources = fx.service.move_entries(&fx.work_home, &renamed_work_home).await.unwrap();

        let renamed_working_copy1_resource = join_path(&renamed_work_home, &[&ext_uri().basename(&working_copy1.resource(), None)]);
        let renamed_working_copy2_resource = join_path(&renamed_work_home, &[&ext_uri().basename(&working_copy2.resource(), None)]);

        assert_eq!(resources.len(), 2);
        assert_only_resources(&resources, &[&renamed_working_copy1_resource, &renamed_working_copy2_resource]);

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

        assert_eq!(entries[3].source(), "moved.source" /* for the move */);
        assert!(!has_no_source_description(&entries[3])); // contains the source working copy path

        let all = fx.service.get_all(CancellationToken::None).await.unwrap();
        assert_eq!(all.len(), 2);
        assert_only_resources(&all, &[&renamed_working_copy1_resource, &renamed_working_copy2_resource]);
    });
}

#[test]
fn move_entries_file_rename_preserves_previous_entries_no_new_entries() {
    run_case(|fx| async move {
        let working_copy_target = fx.working_copy(&fx.test_file1_path);
        let working_copy_source = fx.working_copy(&fx.test_file2_path);

        let entry1 = fx.add_entry(WorkingCopyHistoryEntryDescriptor::with_source(working_copy_target.resource(), "test-source1"), CancellationToken::None).await;
        let entry2 = fx.add_entry(WorkingCopyHistoryEntryDescriptor::with_source(working_copy_target.resource(), "test-source2"), CancellationToken::None).await;
        let entry3 = fx.add_entry(WorkingCopyHistoryEntryDescriptor::with_source(working_copy_target.resource(), "test-source3"), CancellationToken::None).await;

        let mut entries = fx.entries(&working_copy_target.resource()).await;
        assert_eq!(entries.len(), 3);

        entries = fx.entries(&working_copy_source.resource()).await;
        assert_eq!(entries.len(), 0);

        fx.file_service.move_(&working_copy_source.resource(), &working_copy_target.resource(), Some(true)).await.unwrap();

        let result = fx.service.move_entries(&working_copy_source.resource(), &working_copy_target.resource()).await.unwrap();

        assert_eq!(result.len(), 1);
        assert_eq!(text(&result[0]), text(&working_copy_target.resource()));

        entries = fx.entries(&working_copy_source.resource()).await;
        assert_eq!(entries.len(), 0);

        entries = fx.entries(&working_copy_target.resource()).await;
        assert_eq!(entries.len(), 4);

        assert_moved_entry(&entries[0], &entry1, &working_copy_target.resource());
        assert_moved_entry(&entries[1], &entry2, &working_copy_target.resource());
        assert_moved_entry(&entries[2], &entry3, &working_copy_target.resource());

        assert_eq!(entries[3].source(), "renamed.source" /* for the move */);
        assert!(!has_no_source_description(&entries[3])); // contains the source working copy path

        let all = fx.service.get_all(CancellationToken::None).await.unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(text(&all[0]), text(&working_copy_target.resource()));
    });
}

#[test]
fn move_entries_file_rename_preserves_previous_entries_new_entries() {
    run_case(|fx| async move {
        let working_copy_target = fx.working_copy(&fx.test_file1_path);
        let working_copy_source = fx.working_copy(&fx.test_file2_path);

        let target_entry1 = fx.add_entry(WorkingCopyHistoryEntryDescriptor::with_source(working_copy_target.resource(), "test-target1"), CancellationToken::None).await;
        let target_entry2 = fx.add_entry(WorkingCopyHistoryEntryDescriptor::with_source(working_copy_target.resource(), "test-target2"), CancellationToken::None).await;
        let target_entry3 = fx.add_entry(WorkingCopyHistoryEntryDescriptor::with_source(working_copy_target.resource(), "test-target3"), CancellationToken::None).await;

        let source_entry1 = fx.add_entry(WorkingCopyHistoryEntryDescriptor::with_source(working_copy_source.resource(), "test-source1"), CancellationToken::None).await;
        let source_entry2 = fx.add_entry(WorkingCopyHistoryEntryDescriptor::with_source(working_copy_source.resource(), "test-source2"), CancellationToken::None).await;
        let source_entry3 = fx.add_entry(WorkingCopyHistoryEntryDescriptor::with_source(working_copy_source.resource(), "test-source3"), CancellationToken::None).await;

        let mut entries = fx.entries(&working_copy_target.resource()).await;
        assert_eq!(entries.len(), 3);

        entries = fx.entries(&working_copy_source.resource()).await;
        assert_eq!(entries.len(), 3);

        fx.file_service.move_(&working_copy_source.resource(), &working_copy_target.resource(), Some(true)).await.unwrap();

        let result = fx.service.move_entries(&working_copy_source.resource(), &working_copy_target.resource()).await.unwrap();

        assert_eq!(result.len(), 1);
        assert_eq!(text(&result[0]), text(&working_copy_target.resource()));

        entries = fx.entries(&working_copy_source.resource()).await;
        assert_eq!(entries.len(), 0);

        entries = fx.entries(&working_copy_target.resource()).await;
        assert_eq!(entries.len(), 7);

        assert_moved_entry(&entries[0], &target_entry1, &working_copy_target.resource());
        assert_moved_entry(&entries[1], &target_entry2, &working_copy_target.resource());
        assert_moved_entry(&entries[2], &target_entry3, &working_copy_target.resource());
        assert_moved_entry(&entries[3], &source_entry1, &working_copy_target.resource());
        assert_moved_entry(&entries[4], &source_entry2, &working_copy_target.resource());
        assert_moved_entry(&entries[5], &source_entry3, &working_copy_target.resource());

        assert_eq!(entries[6].source(), "renamed.source" /* for the move */);
        assert!(!has_no_source_description(&entries[6])); // contains the source working copy path

        let all = fx.service.get_all(CancellationToken::None).await.unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(text(&all[0]), text(&working_copy_target.resource()));
    });
}
