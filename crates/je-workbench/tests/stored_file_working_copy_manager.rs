//! Porte de `src/vs/workbench/services/workingCopy/test/browser/storedFileWorkingCopyManager.test.ts` (suíte `StoredFileWorkingCopyManager`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - **`setup`, `teardown` e `ensureNoDisposablesAreLeakedInTestSuite()`.** O guarda de vazamentos é criado no começo de
//!   cada caso (`LeakGuard`, o `disposables` do original é o store dele) e o `setup` ([`setup`]) logo depois. O `teardown`
//!   (`for (const workingCopy of manager.workingCopies) workingCopy.dispose()`) é o `Drop` do [`Fixture`], que roda antes
//!   de o guarda conferir os vazamentos. Os casos `async` rodam em `run_with_faked_timers`; `await` é `.await.unwrap()`
//!   (uma rejeição reprova o caso).
//! - **`workbenchInstantiationService`, `TestServiceAccessor` e os dublês.** Estão em
//!   `tests/support/stored_file_working_copy_support.rs` (o acessor ganhou o `ILifecycleService`, o `ILabelService`, o
//!   `IUriIdentityService`, o `TestWorkingCopyBackupService` e o `IWorkingCopyFileService` com as operações de arquivo e os
//!   eventos com `waitUntil`).
//! - **Construtor do gerenciador.** Os 16 parâmetros do `new StoredFileWorkingCopyManager(...)` são o `typeId`, a fábrica e os
//!   serviços do acessor ([`TestServiceAccessor::services`] mais o ciclo de vida, o rótulo e a identidade de URI).
//! - **`manager.resolve(resource, { reload })` e `{ contents }`** são [`reload`] e [`with_contents`], que montam as
//!   `StoredFileWorkingCopyManagerResolveOptions`.
//! - **`model?.resource`, `model?.contents`.** `workingCopy.model` é [`StoredFileWorkingCopy::model`] (o modelo tipado).
//! - **`onDidResolve` com `new Promise(...)`.** [`on_did_resolve_of`] cria o ouvinte e a promessa que o original monta com
//!   `new Promise<void>(resolve => disposables.add(manager.onDidResolve(...)))`; a bandeira `didResolve` é um `Rc<Cell<bool>>`.
//! - **`Promise.all`** é [`Promise::all`]; `Event.fire` do serviço de arquivos é `fire_file_changes` e
//!   `fire_file_system_provider_capabilities_change_event`.
//! - **`canDispose`** devolve `Option<Promise<()>>` (`None` é o `true` imediato): `assert.ok(x instanceof Promise)` é
//!   `is_some()` e `assert.strictEqual(x, true)` é `is_none()`.
//! - **`(isWeb ? test.skip : test)`.** O alvo é nativo, então o caso `pending saves join on shutdown` roda sempre (o ramo
//!   `test.skip` da web não existe). O `TestWillShutdownEvent` é o `fire_shutdown` do `TestLifecycleService`, que junta as
//!   promessas em `shutdown_joiners` (o `event.value` do original).
//! - **`isStoredFileWorkingCopySaveEvent`** e `assert.ok(lastSaveEvent!.stat)` não existem: o tipo do evento já garante o `stat`.
//! - **`workingCopyBackupService.resolved.has(workingCopy)`** é `has_resolved(WorkingCopyIdentifier::of(wc))` (ver o dublê).
//! - **`workingCopy1 === workingCopy2`** é a igualdade de endereço dos dois `Rc` (`Rc::ptr_eq`).
//! - **`try/finally` do `readShouldThrowError` e do `writeShouldThrowError`** é a limpeza logo depois do `await`, antes de o
//!   resultado ser verificado.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod support;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use common::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite, run_with_faked_timers, timeout};
use je_base::async_::async_fn;
use je_base::buffer::{VSBuffer, buffer_to_stream};
use je_base::errors::Error;
use je_base::event::Event;
use je_base::lifecycle::IDisposable;
use je_base::promise::Promise;
use je_base::uri::URI;
use je_platform::files::files::{FileChange, FileChangeType, FileChangesEvent, FileOperationError, FileOperationResult, FileSystemProvider, FileSystemProviderCapabilitiesChangeEvent};
use je_platform::files::in_memory::InMemoryFileSystemProvider;
use je_platform::resources::uri_file;
use je_workbench::common::editor::RevertOptions;
use je_workbench::services::textfile::collaborators::WorkingCopyFileEventFile;
use je_workbench::services::working_copy::abstract_file_working_copy_manager::IBaseFileWorkingCopyManager;
use je_workbench::services::working_copy::stored_file_working_copy::{
    IStoredFileWorkingCopy, IStoredFileWorkingCopyModelFactory, StoredFileWorkingCopyResolveOptions, StoredFileWorkingCopySaveAsOptions,
};
use je_workbench::services::working_copy::stored_file_working_copy_manager::{
    IStoredFileWorkingCopyManager, StoredFileWorkingCopyManager, StoredFileWorkingCopyManagerResolveOptions, StoredFileWorkingCopyManagerSaveEvent,
    StoredFileWorkingCopyReloadOptions,
};
use je_workbench::services::working_copy::working_copy::{IWorkingCopy, WorkingCopyIdentifier};
use je_workbench::services::working_copy::working_copy_service::IWorkingCopyService;
use je_workbench::test::common::workbench_test_services::TestFileService;
use support::stored_file_working_copy_support::{
    STORED_FILE_WORKING_COPY_TYPE_ID, TestServiceAccessor, TestStoredFileWorkingCopy, TestStoredFileWorkingCopyModel, TestStoredFileWorkingCopyModelFactory,
};

//#region Utilitários dos casos

type Manager = StoredFileWorkingCopyManager<TestStoredFileWorkingCopyModel>;

/// O que o `setup` da suíte monta: o acessor e o gerenciador.
struct Fixture {
    accessor: TestServiceAccessor,
    manager: Rc<Manager>,
}

impl Drop for Fixture {
    /// O `teardown`: `workingCopy.dispose()` das working copies ainda conhecidas do gerenciador.
    fn drop(&mut self) {
        for working_copy in self.manager.working_copies() {
            working_copy.dispose();
        }
    }
}

/// O `setup`: o acessor e o `disposables.add(new StoredFileWorkingCopyManager(...))`.
fn setup(disposables: &LeakGuard) -> Fixture {
    let accessor = TestServiceAccessor::new(&disposables.store());
    let manager = disposables.add_rc(StoredFileWorkingCopyManager::new(
        STORED_FILE_WORKING_COPY_TYPE_ID.to_string(),
        TestStoredFileWorkingCopyModelFactory::new() as Rc<dyn IStoredFileWorkingCopyModelFactory<TestStoredFileWorkingCopyModel>>,
        accessor.services(),
        accessor.lifecycle_service.clone(),
        accessor.label_service.clone(),
        accessor.uri_identity_service.clone(),
    ));

    Fixture { accessor, manager }
}

/// `{ reload: { async, force } }`.
fn reload(async_: bool, force: Option<bool>) -> Option<StoredFileWorkingCopyManagerResolveOptions> {
    Some(StoredFileWorkingCopyManagerResolveOptions { reload: Some(StoredFileWorkingCopyReloadOptions { async_, force }), ..StoredFileWorkingCopyManagerResolveOptions::default() })
}

/// `{ contents: bufferToStream(VSBuffer.fromString(text)) }`.
fn with_contents(text: &str) -> Option<StoredFileWorkingCopyManagerResolveOptions> {
    Some(StoredFileWorkingCopyManagerResolveOptions {
        options: StoredFileWorkingCopyResolveOptions { contents: Some(buffer_to_stream(VSBuffer::from_string(text))), ..StoredFileWorkingCopyResolveOptions::default() },
        ..StoredFileWorkingCopyManagerResolveOptions::default()
    })
}

/// `{ force: true }`.
fn forced() -> Option<StoredFileWorkingCopySaveAsOptions> {
    let mut options = StoredFileWorkingCopySaveAsOptions::default();
    options.options.base.force = Some(true);

    Some(options)
}

/// `{ soft: true }` das opções de reverter.
fn soft_revert() -> Option<RevertOptions> {
    Some(RevertOptions { soft: Some(true), ..RevertOptions::default() })
}

/// `new FileOperationError(message, result)`.
fn file_error(message: &str, result: FileOperationResult) -> Error {
    FileOperationError::new(message, result).into_error()
}

/// `resource.toString()`.
fn uri_string(resource: &URI) -> String {
    resource.to_string(false).to_string_lossy()
}

/// `workingCopy.model`.
fn model_of(working_copy: &TestStoredFileWorkingCopy) -> Rc<TestStoredFileWorkingCopyModel> {
    working_copy.model().expect("a working copy está resolvida")
}

/// Conta os disparos de `event` (o `counter++` dos ouvintes do original).
fn count_events<T: 'static>(disposables: &LeakGuard, event: Event<T>) -> Rc<Cell<usize>> {
    let counter = Rc::new(Cell::new(0));
    let listener_counter = counter.clone();
    disposables.add(event.listen(move |_| listener_counter.set(listener_counter.get() + 1)));

    counter
}

/// `new Promise<void>(resolve => disposables.add(manager.onDidResolve(({ model }) => { if (model?.resource.toString() ===
/// resource.toString()) { didResolve = true; resolve(); } })))`: devolve a bandeira `didResolve` e a promessa, que cumpre
/// quando o recurso foi resolvido `resolves_needed` vezes.
fn on_did_resolve_of(disposables: &LeakGuard, manager: &Manager, resource: &URI, resolves_needed: usize) -> (Rc<Cell<bool>>, Promise<()>) {
    let did_resolve = Rc::new(Cell::new(false));
    let resolved_counter = Rc::new(Cell::new(0));
    let (promise, resolver) = Promise::<()>::new();

    let (flag, resource) = (did_resolve.clone(), uri_string(resource));
    disposables.add(manager.on_did_resolve().listen(move |working_copy| {
        if working_copy.model().is_some_and(|model| uri_string(model.resource()) == resource) {
            resolved_counter.set(resolved_counter.get() + 1);
            if resolved_counter.get() >= resolves_needed {
                flag.set(true);
                resolver.resolve(());
            }
        }
    }));

    (did_resolve, promise)
}

/// `accessor.fileService.fireFileChanges(new FileChangesEvent([{ resource, type }], false))`.
fn fire_file_change(file_service: &TestFileService, resource: &URI, change_type: FileChangeType) {
    file_service.fire_file_changes(Rc::new(FileChangesEvent::new(&[FileChange::new(change_type, resource.clone())], false)));
}

/// `{ source, target }` das operações de mover e copiar.
fn source_target(source: &URI, target: &URI) -> WorkingCopyFileEventFile {
    WorkingCopyFileEventFile { source: Some(source.clone()), target: target.clone() }
}

//#endregion

#[test]
fn resolve() {
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let fixture = setup(&disposables);
        let manager = &fixture.manager;
        let resource = uri_file("/test.html");

        let events: Rc<RefCell<Vec<Rc<TestStoredFileWorkingCopy>>>> = Rc::default();
        let created = events.clone();
        let listener = manager.on_did_create().listen(move |working_copy| created.borrow_mut().push(working_copy.clone()));

        let resolve_promise = manager.resolve(&resource, None);
        assert!(manager.get(&resource).is_some()); // working copy known even before resolved()
        assert_eq!(manager.working_copies().len(), 1);

        let working_copy1 = resolve_promise.await.unwrap();
        assert!(working_copy1.model().is_some());
        assert_eq!(working_copy1.working_copy_type_id(), "testStoredFileWorkingCopyType");
        assert_eq!(uri_string(&working_copy1.resource()), uri_string(&resource));
        assert!(Rc::ptr_eq(&manager.get(&resource).unwrap(), &working_copy1));

        let working_copy2 = manager.resolve(&resource, None).await.unwrap();
        assert!(Rc::ptr_eq(&working_copy2, &working_copy1));
        assert_eq!(manager.working_copies().len(), 1);
        working_copy1.dispose();

        let working_copy3 = manager.resolve(&resource, None).await.unwrap();
        assert!(!Rc::ptr_eq(&working_copy3, &working_copy2));
        assert_eq!(manager.working_copies().len(), 1);
        assert!(Rc::ptr_eq(&manager.get(&resource).unwrap(), &working_copy3));
        working_copy3.dispose();

        assert_eq!(manager.working_copies().len(), 0);

        assert_eq!(events.borrow().len(), 2);
        assert_eq!(uri_string(&events.borrow()[0].resource()), uri_string(&working_copy1.resource()));
        assert_eq!(uri_string(&events.borrow()[1].resource()), uri_string(&working_copy2.resource()));

        listener.dispose();

        working_copy1.dispose();
        working_copy2.dispose();
        working_copy3.dispose();
    });
}

#[test]
fn resolve_async() {
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let fixture = setup(&disposables);
        let manager = &fixture.manager;
        let resource = uri_file("/path/index.txt");

        disposables.add_rc(manager.resolve(&resource, None).await.unwrap());

        let (did_resolve, on_did_resolve) = on_did_resolve_of(&disposables, manager, &resource, 1);

        let resolve = manager.resolve(&resource, reload(true, None));

        on_did_resolve.await.unwrap();

        assert!(did_resolve.get());

        let (did_resolve, on_did_resolve) = on_did_resolve_of(&disposables, manager, &resource, 1);

        drop(manager.resolve(&resource, reload(true, Some(true))));

        on_did_resolve.await.unwrap();

        assert!(did_resolve.get());

        disposables.add_rc(resolve.await.unwrap());
    });
}

#[test]
fn resolve_sync() {
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let fixture = setup(&disposables);
        let manager = &fixture.manager;
        let resource = uri_file("/path/index.txt");

        manager.resolve(&resource, None).await.unwrap();

        let (did_resolve, _) = on_did_resolve_of(&disposables, manager, &resource, 1);

        disposables.add_rc(manager.resolve(&resource, reload(false, None)).await.unwrap());
        assert!(did_resolve.get());

        did_resolve.set(false);

        disposables.add_rc(manager.resolve(&resource, reload(false, Some(true))).await.unwrap());
        assert!(did_resolve.get());
    });
}

#[test]
fn resolve_sync_model_disposed_when_error_and_first_call_to_resolve() {
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let fixture = setup(&disposables);
        let manager = &fixture.manager;
        let resource = uri_file("/path/index.txt");

        *fixture.accessor.file_service.read_should_throw_error.borrow_mut() = Some(file_error("fail", FileOperationResult::FileOtherError));

        let result = manager.resolve(&resource, None).await;
        *fixture.accessor.file_service.read_should_throw_error.borrow_mut() = None;

        assert!(result.is_err());
        assert_eq!(manager.working_copies().len(), 0);
    });
}

#[test]
fn resolve_sync_model_not_disposed_when_error_and_model_existed_before() {
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let fixture = setup(&disposables);
        let manager = &fixture.manager;
        let resource = uri_file("/path/index.txt");

        disposables.add_rc(manager.resolve(&resource, None).await.unwrap());

        *fixture.accessor.file_service.read_should_throw_error.borrow_mut() = Some(file_error("fail", FileOperationResult::FileOtherError));

        let result = manager.resolve(&resource, reload(false, None)).await;
        *fixture.accessor.file_service.read_should_throw_error.borrow_mut() = None;

        assert!(result.is_err());
        assert_eq!(manager.working_copies().len(), 1);
    });
}

#[test]
fn resolve_with_initial_contents() {
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let fixture = setup(&disposables);
        let manager = &fixture.manager;
        let resource = uri_file("/test.html");

        let working_copy = manager.resolve(&resource, with_contents("Hello World")).await.unwrap();
        assert_eq!(model_of(&working_copy).contents(), "Hello World");
        assert!(working_copy.is_dirty());

        manager.resolve(&resource, with_contents("More Changes")).await.unwrap();
        assert_eq!(model_of(&working_copy).contents(), "More Changes");
        assert!(working_copy.is_dirty());

        working_copy.dispose();
    });
}

#[test]
fn multiple_resolves_execute_in_sequence_same_resources() {
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let fixture = setup(&disposables);
        let manager = &fixture.manager;
        let resource = uri_file("/test.html");

        let first_promise = manager.resolve(&resource, None);
        let second_promise = manager.resolve(&resource, with_contents("Hello World"));
        let third_promise = manager.resolve(&resource, with_contents("More Changes"));

        first_promise.await.unwrap();
        second_promise.await.unwrap();
        let working_copy = third_promise.await.unwrap();

        assert_eq!(model_of(&working_copy).contents(), "More Changes");
        assert!(working_copy.is_dirty());

        working_copy.dispose();
    });
}

#[test]
fn multiple_resolves_execute_in_parallel_different_resources() {
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let fixture = setup(&disposables);
        let manager = &fixture.manager;
        let resource1 = uri_file("/test1.html");
        let resource2 = uri_file("/test2.html");
        let resource3 = uri_file("/test3.html");

        let first_promise = manager.resolve(&resource1, None);
        let second_promise = manager.resolve(&resource2, None);
        let third_promise = manager.resolve(&resource3, None);

        let working_copies = Promise::all(vec![first_promise, second_promise, third_promise]).await.unwrap();
        let (working_copy1, working_copy2, working_copy3) = (&working_copies[0], &working_copies[1], &working_copies[2]);

        assert_eq!(manager.working_copies().len(), 3);
        assert_eq!(uri_string(&working_copy1.resource()), uri_string(&resource1));
        assert_eq!(uri_string(&working_copy2.resource()), uri_string(&resource2));
        assert_eq!(uri_string(&working_copy3.resource()), uri_string(&resource3));

        working_copy1.dispose();
        working_copy2.dispose();
        working_copy3.dispose();
    });
}

#[test]
fn removed_from_cache_when_working_copy_or_model_gets_disposed() {
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let fixture = setup(&disposables);
        let manager = &fixture.manager;
        let resource = uri_file("/test.html");

        let mut working_copy = manager.resolve(&resource, with_contents("Hello World")).await.unwrap();

        assert!(Rc::ptr_eq(&manager.get(&uri_file("/test.html")).unwrap(), &working_copy));

        working_copy.dispose();
        assert!(manager.get(&uri_file("/test.html")).is_none());

        working_copy = manager.resolve(&resource, with_contents("Hello World")).await.unwrap();

        assert!(Rc::ptr_eq(&manager.get(&uri_file("/test.html")).unwrap(), &working_copy));

        model_of(&working_copy).dispose();
        assert!(manager.get(&uri_file("/test.html")).is_none());
    });
}

#[test]
fn events() {
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let fixture = setup(&disposables);
        let manager = &fixture.manager;
        let resource1 = uri_file("/path/index.txt");
        let resource2 = uri_file("/path/other.txt");

        let created_counter = count_events(&disposables, manager.on_did_create());

        let removed_counter = Rc::new(Cell::new(0));
        let (counter, resource1_string, resource2_string) = (removed_counter.clone(), uri_string(&resource1), uri_string(&resource2));
        disposables.add(manager.on_did_remove().listen(move |resource| {
            if uri_string(resource) == resource1_string || uri_string(resource) == resource2_string {
                counter.set(counter.get() + 1);
            }
        }));

        let resolved_counter = Rc::new(Cell::new(0));
        let (counter, resource1_string) = (resolved_counter.clone(), uri_string(&resource1));
        disposables.add(manager.on_did_resolve().listen(move |working_copy| {
            if uri_string(&working_copy.resource()) == resource1_string {
                counter.set(counter.get() + 1);
            }
        }));

        let got_dirty_counter = Rc::new(Cell::new(0));
        let got_non_dirty_counter = Rc::new(Cell::new(0));
        let (dirty_counter, non_dirty_counter, resource1_string) = (got_dirty_counter.clone(), got_non_dirty_counter.clone(), uri_string(&resource1));
        disposables.add(manager.on_did_change_dirty().listen(move |working_copy| {
            if uri_string(&working_copy.resource()) == resource1_string {
                if working_copy.is_dirty() {
                    dirty_counter.set(dirty_counter.get() + 1);
                } else {
                    non_dirty_counter.set(non_dirty_counter.get() + 1);
                }
            }
        }));

        let reverted_counter = Rc::new(Cell::new(0));
        let (counter, resource1_string) = (reverted_counter.clone(), uri_string(&resource1));
        disposables.add(manager.on_did_revert().listen(move |working_copy| {
            if uri_string(&working_copy.resource()) == resource1_string {
                counter.set(counter.get() + 1);
            }
        }));

        let saved_counter = Rc::new(Cell::new(0));
        let last_save_event: Rc<RefCell<Option<StoredFileWorkingCopyManagerSaveEvent<TestStoredFileWorkingCopyModel>>>> = Rc::default();
        let (counter, last, resource1_string) = (saved_counter.clone(), last_save_event.clone(), uri_string(&resource1));
        disposables.add(manager.on_did_save().listen(move |e| {
            if uri_string(&e.working_copy.resource()) == resource1_string {
                *last.borrow_mut() = Some(e.clone());
                counter.set(counter.get() + 1);
            }
        }));

        let save_error_counter = Rc::new(Cell::new(0));
        let (counter, resource1_string) = (save_error_counter.clone(), uri_string(&resource1));
        disposables.add(manager.on_did_save_error().listen(move |working_copy| {
            if uri_string(&working_copy.resource()) == resource1_string {
                counter.set(counter.get() + 1);
            }
        }));

        let working_copy1 = disposables.add_rc(manager.resolve(&resource1, None).await.unwrap());
        assert_eq!(resolved_counter.get(), 1);
        assert_eq!(created_counter.get(), 1);

        fire_file_change(&fixture.accessor.file_service, &resource1, FileChangeType::Deleted);
        fire_file_change(&fixture.accessor.file_service, &resource1, FileChangeType::Added);

        let working_copy2 = disposables.add_rc(manager.resolve(&resource2, None).await.unwrap());
        assert_eq!(resolved_counter.get(), 2);
        assert_eq!(created_counter.get(), 2);

        model_of(&working_copy1).update_contents("changed");

        working_copy1.revert(None).await.unwrap();
        model_of(&working_copy1).update_contents("changed again");

        working_copy1.save_stored(None).await.unwrap();

        *fixture.accessor.file_service.write_should_throw_error.borrow_mut() = Some(file_error("write error", FileOperationResult::FilePermissionDenied));
        let save_with_error = working_copy1.save_stored(forced()).await;
        *fixture.accessor.file_service.write_should_throw_error.borrow_mut() = None;
        save_with_error.unwrap();

        working_copy1.dispose();
        working_copy2.dispose();

        working_copy1.revert(None).await.unwrap();
        assert_eq!(removed_counter.get(), 2);
        assert_eq!(got_dirty_counter.get(), 3);
        assert_eq!(got_non_dirty_counter.get(), 2);
        assert_eq!(reverted_counter.get(), 1);
        assert_eq!(saved_counter.get(), 1);
        assert!(Rc::ptr_eq(&last_save_event.borrow().as_ref().expect("houve um salvar").working_copy, &working_copy1));
        assert_eq!(save_error_counter.get(), 1);
        assert_eq!(created_counter.get(), 2);

        working_copy1.dispose();
        working_copy2.dispose();
    });
}

#[test]
fn resolve_registers_as_working_copy_and_dispose_clears() {
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let fixture = setup(&disposables);
        let manager = &fixture.manager;
        let resource1 = uri_file("/test1.html");
        let resource2 = uri_file("/test2.html");
        let resource3 = uri_file("/test3.html");

        assert_eq!(fixture.accessor.working_copy_service.working_copies().len(), 0);

        let first_promise = manager.resolve(&resource1, None);
        let second_promise = manager.resolve(&resource2, None);
        let third_promise = manager.resolve(&resource3, None);

        Promise::all(vec![first_promise.clone(), second_promise.clone(), third_promise.clone()]).await.unwrap();

        assert_eq!(fixture.accessor.working_copy_service.working_copies().len(), 3);
        assert_eq!(manager.working_copies().len(), 3);

        manager.dispose();

        assert_eq!(manager.working_copies().len(), 0);

        // dispose does not remove from working copy service, only `destroy` should
        assert_eq!(fixture.accessor.working_copy_service.working_copies().len(), 3);

        disposables.add_rc(first_promise.await.unwrap());
        disposables.add_rc(second_promise.await.unwrap());
        disposables.add_rc(third_promise.await.unwrap());
    });
}

#[test]
fn destroy() {
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let fixture = setup(&disposables);
        let manager = &fixture.manager;
        let resource1 = uri_file("/test1.html");
        let resource2 = uri_file("/test2.html");
        let resource3 = uri_file("/test3.html");

        assert_eq!(fixture.accessor.working_copy_service.working_copies().len(), 0);

        let first_promise = manager.resolve(&resource1, None);
        let second_promise = manager.resolve(&resource2, None);
        let third_promise = manager.resolve(&resource3, None);

        Promise::all(vec![first_promise, second_promise, third_promise]).await.unwrap();

        assert_eq!(fixture.accessor.working_copy_service.working_copies().len(), 3);
        assert_eq!(manager.working_copies().len(), 3);

        manager.destroy().await.unwrap();

        assert_eq!(fixture.accessor.working_copy_service.working_copies().len(), 0);
        assert_eq!(manager.working_copies().len(), 0);
    });
}

#[test]
fn destroy_saves_dirty_working_copies() {
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let fixture = setup(&disposables);
        let manager = &fixture.manager;
        let resource = uri_file("/path/source.txt");

        let working_copy = manager.resolve(&resource, None).await.unwrap();

        let saved = Rc::new(Cell::new(false));
        let did_save = saved.clone();
        disposables.add(working_copy.on_did_save().listen(move |_| did_save.set(true)));

        model_of(&working_copy).update_contents("hello create");
        assert!(working_copy.is_dirty());

        assert_eq!(fixture.accessor.working_copy_service.working_copies().len(), 1);
        assert_eq!(manager.working_copies().len(), 1);

        manager.destroy().await.unwrap();

        assert_eq!(fixture.accessor.working_copy_service.working_copies().len(), 0);
        assert_eq!(manager.working_copies().len(), 0);

        assert!(saved.get());
    });
}

#[test]
fn destroy_falls_back_to_using_backup_when_save_fails() {
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let fixture = setup(&disposables);
        let manager = &fixture.manager;
        let resource = uri_file("/path/source.txt");

        let working_copy = manager.resolve(&resource, None).await.unwrap();
        model_of(&working_copy).set_throw_on_snapshot();

        let unexpected_save = Rc::new(Cell::new(false));
        let did_save = unexpected_save.clone();
        disposables.add(working_copy.on_did_save().listen(move |_| did_save.set(true)));

        model_of(&working_copy).update_contents("hello create");
        assert!(working_copy.is_dirty());

        assert_eq!(fixture.accessor.working_copy_service.working_copies().len(), 1);
        assert_eq!(manager.working_copies().len(), 1);

        assert!(fixture.accessor.working_copy_backup_service.has_resolved(&WorkingCopyIdentifier::of(&*working_copy)));

        manager.destroy().await.unwrap();

        assert_eq!(fixture.accessor.working_copy_service.working_copies().len(), 0);
        assert_eq!(manager.working_copies().len(), 0);

        assert!(!unexpected_save.get());
    });
}

#[test]
fn file_change_event_triggers_working_copy_resolve() {
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let fixture = setup(&disposables);
        let manager = &fixture.manager;
        let resource = uri_file("/path/index.txt");

        manager.resolve(&resource, None).await.unwrap();

        let (did_resolve, on_did_resolve) = on_did_resolve_of(&disposables, manager, &resource, 1);

        fire_file_change(&fixture.accessor.file_service, &resource, FileChangeType::Updated);

        on_did_resolve.await.unwrap();

        assert!(did_resolve.get());
    });
}

#[test]
fn file_change_event_triggers_working_copy_resolve_when_working_copy_is_pending_to_resolve() {
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let fixture = setup(&disposables);
        let manager = &fixture.manager;
        let resource = uri_file("/path/index.txt");

        drop(manager.resolve(&resource, None));

        let (did_resolve, on_did_resolve) = on_did_resolve_of(&disposables, manager, &resource, 2);

        fire_file_change(&fixture.accessor.file_service, &resource, FileChangeType::Updated);

        on_did_resolve.await.unwrap();

        assert!(did_resolve.get());
    });
}

#[test]
fn file_system_provider_change_triggers_working_copy_resolve() {
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let fixture = setup(&disposables);
        let manager = &fixture.manager;
        let resource = uri_file("/path/index.txt");

        disposables.add_rc(manager.resolve(&resource, None).await.unwrap());

        let (did_resolve, on_did_resolve) = on_did_resolve_of(&disposables, manager, &resource, 1);

        let concrete_provider = disposables.add_rc(InMemoryFileSystemProvider::new());
        let provider: Rc<dyn FileSystemProvider> = concrete_provider;
        fixture
            .accessor
            .file_service
            .fire_file_system_provider_capabilities_change_event(FileSystemProviderCapabilitiesChangeEvent { provider, scheme: resource.scheme().to_string_lossy() });

        on_did_resolve.await.unwrap();

        assert!(did_resolve.get());
    });
}

#[test]
fn working_copy_file_event_handling_create() {
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let fixture = setup(&disposables);
        let manager = &fixture.manager;
        let resource = uri_file("/path/source.txt");

        let working_copy = manager.resolve(&resource, None).await.unwrap();
        model_of(&working_copy).update_contents("hello create");
        assert!(working_copy.is_dirty());

        fixture.accessor.working_copy_file_service.create(&[resource]).await.unwrap();
        assert!(!working_copy.is_dirty());
    });
}

#[test]
fn working_copy_file_event_handling_move() {
    run_with_faked_timers(test_move_copy_file_working_copy(true));
}

#[test]
fn working_copy_file_event_handling_copy() {
    run_with_faked_timers(test_move_copy_file_working_copy(false));
}

/// `testMoveCopyFileWorkingCopy(move)`.
async fn test_move_copy_file_working_copy(is_move: bool) {
    let disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let fixture = setup(&disposables);
    let manager = &fixture.manager;
    let source = uri_file("/path/source.txt");
    let target = uri_file("/path/other.txt");

    let source_working_copy = manager.resolve(&source, None).await.unwrap();
    model_of(&source_working_copy).update_contents("hello move or copy");
    assert!(source_working_copy.is_dirty());

    if is_move {
        fixture.accessor.working_copy_file_service.move_files(&[source_target(&source, &target)]).await.unwrap();
    } else {
        fixture.accessor.working_copy_file_service.copy(&[source_target(&source, &target)]).await.unwrap();
    }

    let target_working_copy = manager.resolve(&target, None).await.unwrap();
    assert!(target_working_copy.is_dirty());
    assert_eq!(model_of(&target_working_copy).contents(), "hello move or copy");
}

#[test]
fn working_copy_file_event_handling_delete() {
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let fixture = setup(&disposables);
        let manager = &fixture.manager;
        let resource = uri_file("/path/source.txt");

        let working_copy = manager.resolve(&resource, None).await.unwrap();
        model_of(&working_copy).update_contents("hello delete");
        assert!(working_copy.is_dirty());

        fixture.accessor.working_copy_file_service.delete(&[resource]).await.unwrap();
        assert!(!working_copy.is_dirty());
    });
}

#[test]
fn working_copy_file_event_handling_move_to_same_resource() {
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let fixture = setup(&disposables);
        let manager = &fixture.manager;
        let source = uri_file("/path/source.txt");

        let source_working_copy = manager.resolve(&source, None).await.unwrap();
        model_of(&source_working_copy).update_contents("hello move");
        assert!(source_working_copy.is_dirty());

        fixture.accessor.working_copy_file_service.move_files(&[source_target(&source, &source)]).await.unwrap();

        assert!(source_working_copy.is_dirty());
        assert_eq!(model_of(&source_working_copy).contents(), "hello move");
    });
}

#[test]
fn can_dispose_with_dirty_working_copy() {
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let fixture = setup(&disposables);
        let manager = &fixture.manager;
        let resource = uri_file("/path/index_something.txt");

        let working_copy = manager.resolve(&resource, None).await.unwrap();
        model_of(&working_copy).update_contents("make dirty");

        let can_dispose_promise = manager.can_dispose(working_copy.clone());
        assert!(can_dispose_promise.is_some());

        let can_dispose = Rc::new(Cell::new(false));
        let can_dispose_result = can_dispose.clone();
        drop(async_fn(async move {
            can_dispose_promise.expect("é uma promessa").await?;
            can_dispose_result.set(true);
            Ok(())
        }));

        assert!(!can_dispose.get());
        drop(working_copy.revert(soft_revert()));

        timeout(0).await.unwrap();

        assert!(can_dispose.get());

        let can_dispose2 = manager.can_dispose(working_copy.clone());
        assert!(can_dispose2.is_none());
    });
}

#[test]
fn pending_saves_join_on_shutdown() {
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let fixture = setup(&disposables);
        let manager = &fixture.manager;
        let resource1 = uri_file("/path/index_something1.txt");
        let resource2 = uri_file("/path/index_something2.txt");

        let working_copy1 = disposables.add_rc(manager.resolve(&resource1, None).await.unwrap());
        model_of(&working_copy1).update_contents("make dirty");

        let working_copy2 = disposables.add_rc(manager.resolve(&resource2, None).await.unwrap());
        model_of(&working_copy2).update_contents("make dirty");

        let saved1 = Rc::new(Cell::new(false));
        let did_save1 = saved1.clone();
        drop(working_copy1.save_stored(None).then(move |_| did_save1.set(true)));

        let saved2 = Rc::new(Cell::new(false));
        let did_save2 = saved2.clone();
        drop(working_copy2.save_stored(None).then(move |_| did_save2.set(true)));

        fixture.accessor.lifecycle_service.fire_shutdown(None);

        let joiners = fixture.accessor.lifecycle_service.shutdown_joiners.borrow().clone();
        assert!(!joiners.is_empty());
        Promise::all(joiners).await.unwrap();

        assert!(saved1.get());
        assert!(saved2.get());
    });
}
