//! Porte de `src/vs/workbench/services/workingCopy/test/common/workingCopyService.test.ts` (suíte
//! `WorkingCopyService`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` vira o guarda criado no começo de cada caso; o `disposables` e o
//!   `teardown` do original são o próprio guarda (o `disposables.add(...)` é `add` e `add_rc`).
//! - `assert.strictEqual(a, b)` sobre working copies compara a identidade do objeto ([`id_of`]).
//! - `assert.throws(() => ...)` é [`errors::catch`] devolvendo `Err`.
//! - Uma working copy usada como `IWorkingCopyIdentifier` é o [`WorkingCopyIdentifier::of`].
//! - `copy1.save()` descarta a promessa de resultado, como o original.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use std::cell::RefCell;
use std::rc::Rc;

use common::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite};
use je_base::errors;
use je_base::lifecycle::id_of;
use je_base::uri::URI;
use je_platform::resources::uri_file;
use je_workbench::services::working_copy::working_copy::{IWorkingCopy, WorkingCopyIdentifier};
use je_workbench::services::working_copy::working_copy_service::{IWorkingCopyService, WorkingCopySaveEvent, WorkingCopyService};
use je_workbench::test::common::workbench_test_services::TestWorkingCopy;

/// `assert.strictEqual(actual, expected)` entre a working copy do serviço e a de teste.
fn assert_same(actual: &Rc<dyn IWorkingCopy>, expected: &Rc<TestWorkingCopy>) {
    assert_eq!(id_of(actual), id_of(expected), "não é a mesma working copy");
}

/// Um `IWorkingCopy[]` preenchido por um evento do serviço.
type Collected = Rc<RefCell<Vec<Rc<dyn IWorkingCopy>>>>;

fn collect(disposables: &LeakGuard, event: je_base::event::Event<Rc<dyn IWorkingCopy>>) -> Collected {
    let collected: Collected = Rc::new(RefCell::new(Vec::new()));
    let sink = collected.clone();
    disposables.add(event.listen(move |copy| sink.borrow_mut().push(copy.clone())));
    collected
}

fn new_copy(disposables: &LeakGuard, resource: &URI, is_dirty: Option<bool>, type_id: Option<&str>) -> Rc<TestWorkingCopy> {
    disposables.add_rc(TestWorkingCopy::new(resource.clone(), is_dirty, type_id))
}

#[test]
fn registry_basics() {
    let disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let service = disposables.add_rc(WorkingCopyService::new());

    let on_did_change_dirty = collect(&disposables, service.on_did_change_dirty());
    let on_did_change_content = collect(&disposables, service.on_did_change_content());

    let on_did_save: Rc<RefCell<Vec<WorkingCopySaveEvent>>> = Rc::new(RefCell::new(Vec::new()));
    let sink = on_did_save.clone();
    disposables.add(service.on_did_save().listen(move |copy| sink.borrow_mut().push(copy.clone())));

    let on_did_register = collect(&disposables, service.on_did_register());
    let on_did_unregister = collect(&disposables, service.on_did_unregister());

    assert!(!service.has_dirty());
    assert_eq!(service.dirty_count(), 0);
    assert_eq!(service.working_copies().len(), 0);
    assert!(!service.is_dirty(&uri_file("/"), None));

    // resource 1
    let resource1 = uri_file("/some/folder/file.txt");
    assert!(!service.has_resource(&resource1));
    let identifier1 = WorkingCopyIdentifier { resource: resource1.clone(), type_id: "testWorkingCopyType".to_string() };
    assert!(!service.has(&identifier1));
    assert!(service.get(&identifier1).is_none());
    assert!(service.get_all(&resource1).is_none());
    let copy1 = new_copy(&disposables, &resource1, None, None);
    let unregister1 = service.register_working_copy(copy1.clone());

    assert_eq!(service.working_copies().len(), 1);
    assert_same(&service.working_copies()[0], &copy1);
    assert_eq!(on_did_register.borrow().len(), 1);
    assert_same(&on_did_register.borrow()[0], &copy1);
    assert_eq!(service.dirty_count(), 0);
    assert_eq!(service.modified_count(), 0);
    assert!(!service.is_dirty(&resource1, None));
    assert!(service.has_resource(&resource1));
    assert!(service.has(&WorkingCopyIdentifier::of(&*copy1)));
    assert_same(&service.get(&WorkingCopyIdentifier::of(&*copy1)).unwrap(), &copy1);
    assert!(!service.has_dirty());

    let copies = service.get_all(&copy1.resource()).unwrap();
    assert_eq!(copies.len(), 1);
    assert_same(&copies[0], &copy1);

    copy1.set_dirty(true);
    let _ = copy1.save(None);

    assert!(copy1.is_dirty());
    assert_eq!(service.dirty_count(), 1);
    assert_eq!(service.dirty_working_copies().len(), 1);
    assert_same(&service.dirty_working_copies()[0], &copy1);
    assert_eq!(service.modified_count(), 1);
    assert_eq!(service.modified_working_copies().len(), 1);
    assert_same(&service.modified_working_copies()[0], &copy1);
    assert_eq!(service.working_copies().len(), 1);
    assert_same(&service.working_copies()[0], &copy1);
    assert!(service.is_dirty(&resource1, None));
    assert!(service.has_dirty());
    assert_eq!(on_did_change_dirty.borrow().len(), 1);
    assert_same(&on_did_change_dirty.borrow()[0], &copy1);
    assert_eq!(on_did_save.borrow().len(), 1);
    assert_same(&on_did_save.borrow()[0].working_copy, &copy1);

    copy1.set_content("foo");

    assert_eq!(on_did_change_content.borrow().len(), 1);
    assert_same(&on_did_change_content.borrow()[0], &copy1);

    copy1.set_dirty(false);

    assert_eq!(service.dirty_count(), 0);
    assert!(!service.is_dirty(&resource1, None));
    assert!(!service.has_dirty());
    assert_eq!(on_did_change_dirty.borrow().len(), 2);
    assert_same(&on_did_change_dirty.borrow()[1], &copy1);

    unregister1.dispose();

    assert_eq!(on_did_unregister.borrow().len(), 1);
    assert_same(&on_did_unregister.borrow()[0], &copy1);
    assert_eq!(service.working_copies().len(), 0);
    assert!(!service.has_resource(&resource1));

    // resource 2
    let resource2 = uri_file("/some/folder/file-dirty.txt");
    let copy2 = new_copy(&disposables, &resource2, Some(true), None);
    let unregister2 = service.register_working_copy(copy2.clone());

    assert_eq!(on_did_register.borrow().len(), 2);
    assert_same(&on_did_register.borrow()[1], &copy2);
    assert_eq!(service.dirty_count(), 1);
    assert!(service.is_dirty(&resource2, None));
    assert!(service.has_dirty());

    assert_eq!(on_did_change_dirty.borrow().len(), 3);
    assert_same(&on_did_change_dirty.borrow()[2], &copy2);

    copy2.set_content("foo");

    assert_eq!(on_did_change_content.borrow().len(), 2);
    assert_same(&on_did_change_content.borrow()[1], &copy2);

    unregister2.dispose();

    assert_eq!(on_did_unregister.borrow().len(), 2);
    assert_same(&on_did_unregister.borrow()[1], &copy2);
    assert_eq!(service.dirty_count(), 0);
    assert!(!service.has_dirty());
    assert_eq!(on_did_change_dirty.borrow().len(), 4);
    assert_same(&on_did_change_dirty.borrow()[3], &copy2);
}

#[test]
fn registry_multiple_copies_on_same_resource_throws_same_type_id() {
    let disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let service = disposables.add_rc(WorkingCopyService::new());

    let resource = URI::parse("custom://some/folder/custom.txt", false);

    let copy1 = new_copy(&disposables, &resource, None, None);
    disposables.add(service.register_working_copy(copy1.clone()));

    let copy2 = new_copy(&disposables, &resource, None, None);

    assert!(errors::catch(|| service.register_working_copy(copy2.clone())).is_err());
}

#[test]
fn registry_multiple_copies_on_same_resource_is_supported_different_type_id() {
    let disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let service = disposables.add_rc(WorkingCopyService::new());

    let resource = URI::parse("custom://some/folder/custom.txt", false);

    let type_id1 = "testWorkingCopyTypeId1";
    let mut copy1 = new_copy(&disposables, &resource, Some(false), Some(type_id1));
    let mut dispose1 = service.register_working_copy(copy1.clone());

    let type_id2 = "testWorkingCopyTypeId2";
    let copy2 = new_copy(&disposables, &resource, Some(false), Some(type_id2));
    let dispose2 = service.register_working_copy(copy2.clone());

    let type_id3 = "testWorkingCopyTypeId3";
    let copy3 = new_copy(&disposables, &resource, Some(false), Some(type_id3));
    let dispose3 = service.register_working_copy(copy3.clone());

    let copies = service.get_all(&resource).unwrap();
    assert_eq!(copies.len(), 3);
    assert_same(&copies[0], &copy1);
    assert_same(&copies[1], &copy2);
    assert_same(&copies[2], &copy3);

    assert_eq!(service.dirty_count(), 0);
    assert!(!service.is_dirty(&resource, None));
    assert!(!service.is_dirty(&resource, Some(type_id1)));

    copy1.set_dirty(true);
    assert_eq!(service.dirty_count(), 1);
    assert!(service.is_dirty(&resource, None));
    assert!(service.is_dirty(&resource, Some(type_id1)));
    assert!(!service.is_dirty(&resource, Some(type_id2)));

    copy2.set_dirty(true);
    assert_eq!(service.dirty_count(), 2);
    assert!(service.is_dirty(&resource, None));
    assert!(service.is_dirty(&resource, Some(type_id1)));
    assert!(service.is_dirty(&resource, Some(type_id2)));

    copy3.set_dirty(true);
    assert_eq!(service.dirty_count(), 3);
    assert!(service.is_dirty(&resource, None));
    assert!(service.is_dirty(&resource, Some(type_id1)));
    assert!(service.is_dirty(&resource, Some(type_id2)));
    assert!(service.is_dirty(&resource, Some(type_id3)));

    copy1.set_dirty(false);
    copy2.set_dirty(false);
    copy3.set_dirty(false);
    assert_eq!(service.dirty_count(), 0);
    assert!(!service.is_dirty(&resource, None));
    assert!(!service.is_dirty(&resource, Some(type_id1)));
    assert!(!service.is_dirty(&resource, Some(type_id2)));
    assert!(!service.is_dirty(&resource, Some(type_id3)));

    dispose1.dispose();
    copy1 = new_copy(&disposables, &resource, Some(false), Some(type_id1));
    dispose1 = service.register_working_copy(copy1.clone());

    dispose1.dispose();
    dispose2.dispose();
    dispose3.dispose();

    assert_eq!(service.working_copies().len(), 0);
}
