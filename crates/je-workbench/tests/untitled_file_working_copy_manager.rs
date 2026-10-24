//! Porte de `src/vs/workbench/services/workingCopy/test/browser/untitledFileWorkingCopyManager.test.ts` (suíte `UntitledFileWorkingCopyManager`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - **`setup`, `teardown` e `ensureNoDisposablesAreLeakedInTestSuite()`.** O guarda de vazamentos é criado no começo de cada
//!   caso (`LeakGuard`, o `disposables` do original é o store dele) e o `setup` ([`setup`]) logo depois. O `teardown`
//!   (`for (const workingCopy of [...manager.untitled.workingCopies, ...manager.stored.workingCopies]) workingCopy.dispose()`)
//!   é o `Drop` do [`Fixture`], que roda antes de o guarda conferir os vazamentos. Os casos `async` rodam em
//!   `run_with_faked_timers`; `await` é `.await.unwrap()` (uma rejeição reprova o caso).
//! - **`workbenchInstantiationService`, `TestServiceAccessor` e os dublês.** Estão em
//!   `tests/support/untitled_file_working_copy_support.rs` (o acessor compõe o de `stored_file_working_copy_support`, troca o
//!   `IWorkingCopyFileService` pelo real e acrescenta os diálogos, o serviço de caminho, o ambiente e as decorações).
//! - **`manager.resolve(...)`.** As sobrecargas de `resolve` do `FileWorkingCopyManager` são o argumento
//!   `FileWorkingCopyResolveArg`, e o resultado é a união `FileWorkingCopy`: [`resolve_untitled`] escolhe o ramo sem título.
//!   `manager.untitled.resolve({ ... })` recebe as três opções convertidas por `into()` (ver o gerenciador).
//! - **Opções.** `{ contents: { value, markModified } }` é [`with_contents`]; `{ untitledResource }` e `{ associatedResource }`
//!   são as structs de mesmo nome do gerenciador.
//! - **Identidade.** `a === b` é `Rc::ptr_eq`; `get(...)` indefinido é `is_none()`.
//! - **`savedEvent`.** O ouvinte de `onDidSave` guarda o último evento em um `Rc<RefCell<...>>` ([`capture_saved_event`]).
//! - **`workingCopy.dispose()` das working copies e `manager.destroy()` sem `await`.** O `try/finally` dos dois últimos casos é
//!   a chamada de `destroy()` logo depois das verificações (a promessa não é esperada, como no original).

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod support;

use std::cell::RefCell;
use std::rc::Rc;

use common::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite, run_with_faked_timers};
use je_base::JsString;
use je_base::buffer::{VSBuffer, buffer_to_stream};
use je_base::lifecycle::IDisposable;
use je_base::network::schemas;
use je_base::uri::{Field, URI, UriChange, UriComponents};
use je_platform::dialogs::dialogs::ConfirmationResult;
use je_platform::resources::uri_file;
use je_workbench::services::untitled::untitled_text_editor_service::AssociatedResource;
use je_workbench::services::working_copy::abstract_file_working_copy_manager::IBaseFileWorkingCopyManager;
use je_workbench::services::working_copy::file_working_copy_manager::FileWorkingCopyResolveArg;
use je_workbench::services::working_copy::untitled_file_working_copy::{IUntitledFileWorkingCopy, UntitledFileWorkingCopyInitialContents};
use je_workbench::services::working_copy::untitled_file_working_copy_manager::{
    IUntitledFileWorkingCopyManager, NewOrExistingUntitledFileWorkingCopyOptions, NewUntitledFileWorkingCopyOptions, NewUntitledFileWorkingCopyWithAssociatedResourceOptions,
    UntitledFileWorkingCopyResolveOptions, UntitledFileWorkingCopySaveEvent,
};
use je_workbench::services::working_copy::working_copy::{IWorkingCopy, NO_TYPE_ID, WorkingCopyCapabilities};
use je_workbench::services::working_copy::working_copy_service::IWorkingCopyService;
use support::untitled_file_working_copy_support::{FileWorkingCopyManagerAccessor, TestFileWorkingCopyManager, TestUntitledFileWorkingCopy, count_events, initial};

//#region Utilitários dos casos

/// O que o `setup` da suíte monta: o acessor e o gerenciador.
struct Fixture {
    accessor: FileWorkingCopyManagerAccessor,
    manager: Rc<TestFileWorkingCopyManager>,
}

impl Drop for Fixture {
    /// O `teardown`: `workingCopy.dispose()` das working copies ainda conhecidas dos dois gerenciadores.
    fn drop(&mut self) {
        for working_copy in self.manager.untitled.working_copies() {
            working_copy.dispose();
        }
        for working_copy in self.manager.stored.working_copies() {
            working_copy.dispose();
        }
    }
}

/// O `setup`: o acessor, os provedores de arquivo e o `disposables.add(new FileWorkingCopyManager(...))`.
fn setup(disposables: &LeakGuard) -> Fixture {
    let accessor = FileWorkingCopyManagerAccessor::new(&disposables.store());
    accessor.register_providers(&disposables.store());
    let manager = disposables.add_rc(accessor.new_manager("testUntitledFileWorkingCopyType"));

    Fixture { accessor, manager }
}

/// `manager.resolve(options)` do `FileWorkingCopyManager` para uma working copy sem título.
async fn resolve_untitled(manager: &TestFileWorkingCopyManager, options: Option<impl Into<UntitledFileWorkingCopyResolveOptions>>) -> Rc<TestUntitledFileWorkingCopy> {
    manager.resolve(FileWorkingCopyResolveArg::untitled(options)).await.unwrap().into_untitled().expect("o resultado é uma working copy sem título")
}

/// `{ contents: { value: bufferToStream(VSBuffer.fromString(text)), markModified } }`.
fn with_contents(text: &str, mark_modified: Option<bool>) -> NewUntitledFileWorkingCopyOptions {
    NewUntitledFileWorkingCopyOptions { contents: Some(UntitledFileWorkingCopyInitialContents { value: buffer_to_stream(VSBuffer::from_string(text)), mark_modified }) }
}

/// `{ untitledResource }`.
fn existing(untitled_resource: URI) -> NewOrExistingUntitledFileWorkingCopyOptions {
    NewOrExistingUntitledFileWorkingCopyOptions { options: NewUntitledFileWorkingCopyOptions::default(), untitled_resource, is_scratchpad: None }
}

/// `{ associatedResource: { path } }`.
fn associated(path: &str) -> NewUntitledFileWorkingCopyWithAssociatedResourceOptions {
    NewUntitledFileWorkingCopyWithAssociatedResourceOptions {
        options: NewUntitledFileWorkingCopyOptions::default(),
        associated_resource: AssociatedResource { path: path.to_string(), ..AssociatedResource::default() },
    }
}

/// `resource.with({ scheme })`.
fn with_scheme(resource: &URI, scheme: &str) -> URI {
    resource.with(&UriChange { scheme: Field::from(scheme), ..UriChange::default() })
}

/// `resource.toString()`.
fn uri_string(resource: &URI) -> String {
    resource.to_string(false).to_string_lossy()
}

/// `let savedEvent; disposables.add(manager.untitled.onDidSave(e => { savedEvent = e; }))`.
fn capture_saved_event(disposables: &LeakGuard, manager: &TestFileWorkingCopyManager) -> Rc<RefCell<Option<UntitledFileWorkingCopySaveEvent>>> {
    let saved_event: Rc<RefCell<Option<UntitledFileWorkingCopySaveEvent>>> = Rc::default();
    let slot = saved_event.clone();
    disposables.add(manager.untitled.on_did_save().listen(move |e| *slot.borrow_mut() = Some(e.clone())));

    saved_event
}

//#endregion

#[test]
fn basics() {
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let fixture = setup(&disposables);
        let (accessor, manager) = (&fixture.accessor, &fixture.manager);
        let store = disposables.store();

        let create_counter = count_events(&store, manager.untitled.on_did_create());
        let dispose_counter = count_events(&store, manager.untitled.on_will_dispose());
        let dirty_counter = count_events(&store, manager.untitled.on_did_change_dirty());

        assert_eq!(accessor.working_copy_service.working_copies().len(), 0);
        assert_eq!(manager.untitled.working_copies().len(), 0);

        assert!(manager.untitled.get(&uri_file("/some/invalidPath")).is_none());
        assert!(manager.untitled.get(&with_scheme(&uri_file("/some/invalidPath"), schemas::UNTITLED)).is_none());

        let working_copy1 = manager.untitled.resolve(None).await.unwrap();
        let working_copy2 = manager.untitled.resolve(None).await.unwrap();

        assert_eq!(working_copy1.working_copy_type_id(), "testUntitledFileWorkingCopyType");
        assert!(*working_copy1.resource().scheme() == schemas::UNTITLED);

        assert_eq!(create_counter.get(), 2);

        assert!(Rc::ptr_eq(&manager.untitled.get(&working_copy1.resource()).unwrap(), &working_copy1));
        assert!(Rc::ptr_eq(&manager.untitled.get(&working_copy2.resource()).unwrap(), &working_copy2));

        assert_eq!(accessor.working_copy_service.working_copies().len(), 2);
        assert_eq!(manager.untitled.working_copies().len(), 2);

        assert_ne!(uri_string(&working_copy1.resource()), uri_string(&working_copy2.resource()));

        for working_copy in [&working_copy1, &working_copy2] {
            assert_eq!(working_copy.capabilities(), WorkingCopyCapabilities::UNTITLED);
            assert!(!working_copy.is_dirty());
            assert!(!working_copy.is_modified());
            assert!(working_copy.model().is_some());
        }

        working_copy1.model().unwrap().update_contents("Hello World");

        assert!(working_copy1.is_dirty());
        assert!(working_copy1.is_modified());
        assert_eq!(dirty_counter.get(), 1);

        working_copy1.model().unwrap().update_contents(""); // change to empty clears dirty/modified flags
        assert!(!working_copy1.is_dirty());
        assert!(!working_copy1.is_modified());
        assert_eq!(dirty_counter.get(), 2);

        working_copy2.model().unwrap().fire_content_change_event(initial(false));
        assert!(working_copy2.is_dirty());
        assert!(working_copy2.is_modified());
        assert_eq!(dirty_counter.get(), 3);

        working_copy1.dispose();

        assert_eq!(manager.untitled.working_copies().len(), 1);
        assert!(manager.untitled.get(&working_copy1.resource()).is_none());

        working_copy2.dispose();

        assert_eq!(manager.untitled.working_copies().len(), 0);
        assert!(manager.untitled.get(&working_copy2.resource()).is_none());

        assert_eq!(dispose_counter.get(), 2);
    });
}

#[test]
fn dirty_scratchpads_are_never_dirty() {
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let fixture = setup(&disposables);
        let manager = &fixture.manager;

        let dirty_counter = count_events(&disposables.store(), manager.untitled.on_did_change_dirty());

        let untitled_resource = URI::from(&UriComponents { scheme: Some(JsString::from(schemas::UNTITLED)), path: Some(JsString::from("/myscratchpad")), ..UriComponents::default() }, false);
        let working_copy1 = resolve_untitled(
            manager,
            Some(NewOrExistingUntitledFileWorkingCopyOptions { options: NewUntitledFileWorkingCopyOptions::default(), untitled_resource, is_scratchpad: Some(true) }),
        )
        .await;

        assert!(*working_copy1.resource().scheme() == schemas::UNTITLED);
        assert_eq!(manager.untitled.working_copies().len(), 1);

        working_copy1.model().unwrap().update_contents("contents");
        assert!(!working_copy1.is_dirty());
        assert!(working_copy1.is_modified());

        working_copy1.model().unwrap().fire_content_change_event(initial(true));
        assert!(!working_copy1.is_dirty());
        assert!(!working_copy1.is_modified());

        assert_eq!(dirty_counter.get(), 0);

        working_copy1.dispose();
    });
}

#[test]
fn resolve_with_initial_value() {
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let fixture = setup(&disposables);
        let manager = &fixture.manager;

        let dirty_counter = count_events(&disposables.store(), manager.untitled.on_did_change_dirty());

        let working_copy1 = manager.untitled.resolve(Some(with_contents("Hello World", None).into())).await.unwrap();

        assert!(working_copy1.is_modified());
        assert!(working_copy1.is_dirty());
        assert_eq!(dirty_counter.get(), 1);
        assert_eq!(working_copy1.model().unwrap().contents(), "Hello World");

        working_copy1.dispose();

        let working_copy2 = manager.untitled.resolve(Some(with_contents("Hello World", Some(true)).into())).await.unwrap();

        assert!(working_copy2.is_modified());
        assert!(working_copy2.is_dirty());
        assert_eq!(dirty_counter.get(), 2);
        assert_eq!(working_copy2.model().unwrap().contents(), "Hello World");

        working_copy2.dispose();
    });
}

#[test]
fn resolve_with_initial_value_but_mark_dirty_false() {
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let fixture = setup(&disposables);
        let manager = &fixture.manager;

        let dirty_counter = count_events(&disposables.store(), manager.untitled.on_did_change_dirty());

        let working_copy = manager.untitled.resolve(Some(with_contents("Hello World", Some(false)).into())).await.unwrap();

        assert!(!working_copy.is_modified());
        assert!(!working_copy.is_dirty());
        assert_eq!(dirty_counter.get(), 0);
        assert_eq!(working_copy.model().unwrap().contents(), "Hello World");

        working_copy.dispose();
    });
}

#[test]
fn resolve_begins_counter_from_1_for_disposed_untitled() {
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let fixture = setup(&disposables);
        let manager = &fixture.manager;

        let untitled1 = manager.untitled.resolve(None).await.unwrap();
        untitled1.dispose();

        let untitled1_again = disposables.add_rc(manager.untitled.resolve(None).await.unwrap());
        assert_eq!(uri_string(&untitled1.resource()), uri_string(&untitled1_again.resource()));
    });
}

#[test]
fn resolve_existing() {
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let fixture = setup(&disposables);
        let manager = &fixture.manager;

        let create_counter = count_events(&disposables.store(), manager.untitled.on_did_create());

        let working_copy1 = manager.untitled.resolve(None).await.unwrap();
        assert_eq!(create_counter.get(), 1);

        let working_copy2 = manager.untitled.resolve(Some(existing(working_copy1.resource()).into())).await.unwrap();
        assert!(Rc::ptr_eq(&working_copy1, &working_copy2));
        assert_eq!(create_counter.get(), 1);

        let working_copy3 = manager.untitled.resolve(Some(existing(uri_file("/invalid/untitled")).into())).await.unwrap();
        assert!(*working_copy3.resource().scheme() == schemas::UNTITLED);

        working_copy1.dispose();
        working_copy2.dispose();
        working_copy3.dispose();
    });
}

#[test]
fn resolve_untitled_resource_used_for_new_working_copy() {
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let fixture = setup(&disposables);
        let manager = &fixture.manager;

        let invalid_untitled_resource = uri_file("my/untitled.txt");
        let valid_untitled_resource = with_scheme(&invalid_untitled_resource, schemas::UNTITLED);

        let working_copy1 = manager.untitled.resolve(Some(existing(invalid_untitled_resource.clone()).into())).await.unwrap();
        assert_ne!(uri_string(&working_copy1.resource()), uri_string(&invalid_untitled_resource));

        let working_copy2 = manager.untitled.resolve(Some(existing(valid_untitled_resource.clone()).into())).await.unwrap();
        assert_eq!(uri_string(&working_copy2.resource()), uri_string(&valid_untitled_resource));

        working_copy1.dispose();
        working_copy2.dispose();
    });
}

#[test]
fn resolve_with_associated_resource() {
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let fixture = setup(&disposables);
        let manager = &fixture.manager;

        let working_copy = manager.untitled.resolve(Some(associated("/some/associated.txt").into())).await.unwrap();

        assert!(working_copy.has_associated_file_path());
        assert_eq!(working_copy.resource().path().to_string_lossy(), "/some/associated.txt");

        working_copy.dispose();
    });
}

#[test]
fn save_without_associated_resource() {
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let fixture = setup(&disposables);
        let (accessor, manager) = (&fixture.accessor, &fixture.manager);

        let saved_event = capture_saved_event(&disposables, manager);

        let working_copy = manager.untitled.resolve(None).await.unwrap();
        working_copy.model().unwrap().update_contents("Simple Save");

        accessor.file_dialog_service.set_pick_file_to_save(uri_file("simple/file.txt"));

        let result = working_copy.save(None).await.unwrap();
        assert!(result);

        assert!(manager.untitled.get(&working_copy.resource()).is_none());
        let saved_event = saved_event.borrow().clone().expect("o evento de salvar foi disparado");
        assert_eq!(uri_string(&saved_event.source), uri_string(&working_copy.resource()));
        assert_eq!(uri_string(&saved_event.target), uri_string(&uri_file("simple/file.txt")));

        working_copy.dispose();
    });
}

#[test]
fn save_with_associated_resource() {
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let fixture = setup(&disposables);
        let (accessor, manager) = (&fixture.accessor, &fixture.manager);

        let saved_event = capture_saved_event(&disposables, manager);

        let working_copy = manager.untitled.resolve(Some(associated("/some/associated.txt").into())).await.unwrap();
        working_copy.model().unwrap().update_contents("Simple Save with associated resource");

        let target = URI::from(&UriComponents { scheme: Some(JsString::from(schemas::FILE)), path: Some(JsString::from("/some/associated.txt")), ..UriComponents::default() }, false);
        accessor.file_service.not_exists_set.borrow_mut().set(target, true);

        let result = working_copy.save(None).await.unwrap();
        assert!(result);

        assert!(manager.untitled.get(&working_copy.resource()).is_none());
        let saved_event = saved_event.borrow().clone().expect("o evento de salvar foi disparado");
        assert_eq!(uri_string(&saved_event.source), uri_string(&working_copy.resource()));
        assert_eq!(uri_string(&saved_event.target), uri_string(&uri_file("/some/associated.txt")));

        working_copy.dispose();
    });
}

#[test]
fn save_with_associated_resource_asks_to_overwrite() {
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let fixture = setup(&disposables);
        let (accessor, manager) = (&fixture.accessor, &fixture.manager);

        let working_copy = manager.untitled.resolve(Some(associated("/some/associated.txt").into())).await.unwrap();
        working_copy.model().unwrap().update_contents("Simple Save with associated resource");

        let mut result = working_copy.save(None).await.unwrap();
        assert!(!result); // not confirmed

        assert!(Rc::ptr_eq(&manager.untitled.get(&working_copy.resource()).unwrap(), &working_copy));

        accessor.dialog_service.set_confirm_result(ConfirmationResult { checkbox_checked: None, confirmed: true });

        result = working_copy.save(None).await.unwrap();
        assert!(result); // confirmed

        assert!(manager.untitled.get(&working_copy.resource()).is_none());

        working_copy.dispose();
    });
}

#[test]
fn destroy() {
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let fixture = setup(&disposables);
        let (accessor, manager) = (&fixture.accessor, &fixture.manager);

        assert_eq!(accessor.working_copy_service.working_copies().len(), 0);

        manager.untitled.resolve(None).await.unwrap();
        manager.untitled.resolve(None).await.unwrap();
        manager.untitled.resolve(None).await.unwrap();

        assert_eq!(accessor.working_copy_service.working_copies().len(), 3);
        assert_eq!(manager.untitled.working_copies().len(), 3);

        manager.untitled.destroy().await.unwrap();

        assert_eq!(accessor.working_copy_service.working_copies().len(), 0);
        assert_eq!(manager.untitled.working_copies().len(), 0);
    });
}

#[test]
fn manager_with_different_types_produce_different_uris() {
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let fixture = setup(&disposables);

        let manager = disposables.add_rc(fixture.accessor.new_manager("someOtherUntitledTypeId"));

        let untitled1_original_type = disposables.add_rc(manager.untitled.resolve(None).await.unwrap());
        let untitled1_other_type = disposables.add_rc(manager.untitled.resolve(None).await.unwrap());

        assert_ne!(uri_string(&untitled1_original_type.resource()), uri_string(&untitled1_other_type.resource()));

        drop(manager.destroy());
    });
}

#[test]
fn manager_without_type_id_produces_backwards_compatible_uris() {
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let fixture = setup(&disposables);

        let manager = disposables.add_rc(fixture.accessor.new_manager(NO_TYPE_ID));

        let result = disposables.add_rc(manager.untitled.resolve(None).await.unwrap());
        assert!(*result.resource().scheme() == schemas::UNTITLED);
        assert!(!result.resource().path().is_empty());
        assert_eq!(result.resource().query().to_string_lossy(), "");
        assert_eq!(result.resource().authority().to_string_lossy(), "");
        assert_eq!(result.resource().fragment().to_string_lossy(), "");

        drop(manager.destroy());
    });
}
