//! Porte de `src/vs/platform/undoRedo/test/common/undoRedoService.test.ts` (suíte `UndoRedoService`), caso a caso e na mesma ordem.
//!
//! Pendente: `multi resource elements` depende do elemento de workspace (`IWorkspaceUndoRedoElement`, várias
//! URIs numa operação), do diálogo assíncrono e do `split`, que o `UndoRedoService` portado ainda não tem.
//! O `UndoRedoService` do original recebe `IDialogService` e `INotificationService`; o porte recebe funções
//! (`ConfirmFn` e `NotifyFn`), então `createUndoRedoService` vira `UndoRedoService::for_tests()`.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use std::any::Any;
use std::cell::Cell;
use std::rc::Rc;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::path::Platform;
use je_base::uri::URI;
use je_editor::undo_redo::{UndoRedoElement, UndoRedoGroup};
use je_editor::undo_redo_service::{UndoRedoService, UndoRedoTarget};

/// O `IUndoRedoElement` literal do original: rótulo, código e contadores de `undo` e `redo`.
struct TestElement {
    resource: URI,
    label: &'static str,
    undo_calls: Cell<u32>,
    redo_calls: Cell<u32>,
}

impl TestElement {
    fn new(resource: &URI, label: &'static str) -> Rc<TestElement> {
        Rc::new(TestElement {
            resource: resource.clone(),
            label,
            undo_calls: Cell::new(0),
            redo_calls: Cell::new(0),
        })
    }
}

impl UndoRedoElement for TestElement {
    fn resource(&self) -> URI {
        self.resource.clone()
    }

    fn label(&self) -> String {
        self.label.to_owned()
    }

    fn code(&self) -> String {
        "typing".to_owned()
    }

    fn undo(&self) {
        self.undo_calls.set(self.undo_calls.get() + 1);
    }

    fn redo(&self) {
        self.redo_calls.set(self.redo_calls.get() + 1);
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

fn is_last_element(service: &UndoRedoService, resource: &URI, expected: Option<&Rc<TestElement>>) -> bool {
    match (service.get_last_element(resource), expected) {
        (None, None) => true,
        (Some(actual), Some(expected)) => std::ptr::addr_eq(Rc::as_ptr(&actual), Rc::as_ptr(expected)),
        _ => false,
    }
}

fn states(service: &UndoRedoService, resource: &URI) -> (bool, bool, bool) {
    (
        service.can_undo(UndoRedoTarget::Resource(resource)),
        service.can_redo(UndoRedoTarget::Resource(resource)),
        service.has_elements(resource),
    )
}

#[test]
fn simple_single_resource_elements() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let resource = URI::file("test.txt", Platform::Posix);
    let service = UndoRedoService::for_tests();

    assert_eq!(states(&service, &resource), (false, false, false));
    assert!(service.get_last_element(&resource).is_none());

    let element1 = TestElement::new(&resource, "typing 1");
    service.push_element(element1.clone(), None, None);

    assert_eq!((element1.undo_calls.get(), element1.redo_calls.get()), (0, 0));
    assert_eq!(states(&service, &resource), (true, false, true));
    assert!(is_last_element(&service, &resource, Some(&element1)));

    service.undo(UndoRedoTarget::Resource(&resource));
    assert_eq!((element1.undo_calls.get(), element1.redo_calls.get()), (1, 0));
    assert_eq!(states(&service, &resource), (false, true, true));
    assert!(is_last_element(&service, &resource, None));

    service.redo(UndoRedoTarget::Resource(&resource));
    assert_eq!((element1.undo_calls.get(), element1.redo_calls.get()), (1, 1));
    assert_eq!(states(&service, &resource), (true, false, true));
    assert!(is_last_element(&service, &resource, Some(&element1)));

    let element2 = TestElement::new(&resource, "typing 2");
    service.push_element(element2.clone(), None, None);

    assert_eq!((element1.undo_calls.get(), element1.redo_calls.get()), (1, 1));
    assert_eq!((element2.undo_calls.get(), element2.redo_calls.get()), (0, 0));
    assert_eq!(states(&service, &resource), (true, false, true));
    assert!(is_last_element(&service, &resource, Some(&element2)));

    service.undo(UndoRedoTarget::Resource(&resource));

    assert_eq!((element1.undo_calls.get(), element1.redo_calls.get()), (1, 1));
    assert_eq!((element2.undo_calls.get(), element2.redo_calls.get()), (1, 0));
    assert_eq!(states(&service, &resource), (true, true, true));
    assert!(is_last_element(&service, &resource, None));

    let element3 = TestElement::new(&resource, "typing 2");
    service.push_element(element3.clone(), None, None);

    assert_eq!((element1.undo_calls.get(), element1.redo_calls.get()), (1, 1));
    assert_eq!((element2.undo_calls.get(), element2.redo_calls.get()), (1, 0));
    assert_eq!((element3.undo_calls.get(), element3.redo_calls.get()), (0, 0));
    assert_eq!(states(&service, &resource), (true, false, true));
    assert!(is_last_element(&service, &resource, Some(&element3)));

    service.undo(UndoRedoTarget::Resource(&resource));

    assert_eq!((element1.undo_calls.get(), element1.redo_calls.get()), (1, 1));
    assert_eq!((element2.undo_calls.get(), element2.redo_calls.get()), (1, 0));
    assert_eq!((element3.undo_calls.get(), element3.redo_calls.get()), (1, 0));
    assert_eq!(states(&service, &resource), (true, true, true));
    assert!(is_last_element(&service, &resource, None));
}

#[test]
#[ignore = "Pendente: depende do elemento de workspace (várias URIs), do diálogo assíncrono e do split, ainda não portados"]
fn multi_resource_elements() {}

#[test]
fn undo_redo_group_none_uses_id_0() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let none = UndoRedoGroup::none();
    assert_eq!(none.id, 0);
    assert_eq!(none.next_order(), 0);
    assert_eq!(none.next_order(), 0);
}

#[test]
fn restore_snapshot_preserves_elements_that_match_the_snapshot() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let resource = URI::file("test.txt", Platform::Posix);
    let service = UndoRedoService::for_tests();

    // Empilha três elementos
    let elements: Vec<Rc<TestElement>> = ["typing 1", "typing 2", "typing 3"]
        .into_iter()
        .map(|label| TestElement::new(&resource, label))
        .collect();
    for element in &elements {
        service.push_element(element.clone(), None, None);
    }

    // Cria o snapshot depois de três elementos: [element1, element2, element3]
    let snapshot = service.create_snapshot(&resource);

    // Empilha mais elementos depois do snapshot
    for label in ["typing 4", "typing 5"] {
        service.push_element(TestElement::new(&resource, label), None, None);
    }

    // Confere que agora são 5 elementos
    let current = service.get_elements(&resource);
    assert_eq!(current.past.len(), 5);
    assert_eq!(current.future.len(), 0);

    // Restaura o snapshot: tira element4 e element5, mas mantém element1, element2 e element3
    service.restore_snapshot(&snapshot);

    // Confere que os elementos que casam com o snapshot foram preservados
    let current = service.get_elements(&resource);
    assert_eq!(current.past.len(), 3, "Should have 3 past elements after restore");
    assert_eq!(current.future.len(), 0, "Should have 0 future elements after restore");
    for (index, element) in elements.iter().enumerate() {
        assert!(
            std::ptr::addr_eq(Rc::as_ptr(&current.past[index]), Rc::as_ptr(element)),
            "O elemento {} deveria ser element{}",
            index,
            index + 1
        );
    }
}
