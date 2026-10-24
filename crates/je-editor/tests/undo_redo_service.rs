//! Porte de `src/vs/platform/undoRedo/test/common/undoRedoService.test.ts` (suíte `UndoRedoService`), caso a caso e na mesma ordem.
//!
//! O `createUndoRedoService(dialogService)` do original é o `UndoRedoService::new(dialog, notificação)`; sem diálogo
//! (`new TestDialogService()`) é o `UndoRedoService::for_tests()`. O `await service.undo(resource)` é o
//! [`settled`]: roda o laço de microtarefas até assentar a promessa e confere que ela não rejeitou.
//! O `new class extends mock<IDialogService>()` do caso `multi resource elements` é o [`ConfirmingDialogService`]:
//! `prompt` roda o primeiro botão e `confirm` confirma.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
#[path = "support/test_notification_service.rs"]
mod test_notification_service;

use std::any::Any;
use std::cell::Cell;
use std::rc::Rc;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::event::Event;
use je_base::path::Platform;
use je_base::promise::{MaybePromise, Promise};
use je_base::runtime;
use je_base::uri::URI;
use je_editor::undo_redo::{IUndoRedoService, UndoRedoElement, UndoRedoElementRef, UndoRedoGroup, WorkspaceUndoRedoElement};
use je_editor::undo_redo_service::{UndoRedoService, UndoRedoTarget};
use je_platform::dialogs::{CheckboxResult, Confirmation, ConfirmationResult, IDialogService, Input, InputResult, Prompt, PromptResult};
use test_notification_service::TestNotificationService;

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

    fn calls(&self) -> (u32, u32) {
        (self.undo_calls.get(), self.redo_calls.get())
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

/// O `IUndoRedoElement` de workspace do caso `multi resource elements`, com o `split` que devolve os dois elementos de recurso.
struct TestWorkspaceElement {
    resources: Vec<URI>,
    undo_calls: Cell<u32>,
    redo_calls: Cell<u32>,
    split_elements: Vec<Rc<TestElement>>,
}

impl WorkspaceUndoRedoElement for TestWorkspaceElement {
    fn resources(&self) -> Vec<URI> {
        self.resources.clone()
    }

    fn label(&self) -> String {
        "typing 1".to_owned()
    }

    fn code(&self) -> String {
        "typing".to_owned()
    }

    fn undo(&self) -> MaybePromise<()> {
        self.undo_calls.set(self.undo_calls.get() + 1);
        MaybePromise::Value(())
    }

    fn redo(&self) -> MaybePromise<()> {
        self.redo_calls.set(self.redo_calls.get() + 1);
        MaybePromise::Value(())
    }

    fn can_split(&self) -> bool {
        true
    }

    fn split(&self) -> Option<Vec<Rc<dyn UndoRedoElement>>> {
        Some(self.split_elements.iter().map(|element| element.clone() as Rc<dyn UndoRedoElement>).collect())
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// `new class extends mock<IDialogService>()`: `prompt` roda o primeiro botão e `confirm` confirma.
struct ConfirmingDialogService;

impl IDialogService for ConfirmingDialogService {
    fn on_will_show_dialog(&self) -> Event<()> {
        unreachable!("mock<IDialogService>: onWillShowDialog não é definido")
    }

    fn on_did_show_dialog(&self) -> Event<()> {
        unreachable!("mock<IDialogService>: onDidShowDialog não é definido")
    }

    fn confirm(&self, _confirmation: Confirmation) -> Promise<ConfirmationResult> {
        // confirm!
        Promise::resolved(ConfirmationResult { checkbox_checked: None, confirmed: true })
    }

    fn prompt(&self, prompt: Prompt) -> Promise<PromptResult> {
        let first_button = prompt.buttons.as_ref().and_then(|buttons| buttons.first());
        match first_button.map(|button| (button.base.run)(CheckboxResult { checkbox_checked: Some(false) })) {
            Some(MaybePromise::Value(result)) => Promise::resolved(PromptResult { checkbox_checked: None, result: Some(result) }),
            Some(MaybePromise::Promise(result)) => result.then(|result| PromptResult { checkbox_checked: None, result: Some(result) }),
            None => Promise::resolved(PromptResult { checkbox_checked: None, result: None }),
        }
    }

    fn input(&self, _input: Input) -> Promise<InputResult> {
        unreachable!("mock<IDialogService>: input não é definido")
    }

    fn info(&self, _message: &str, _detail: Option<&str>) -> Promise<()> {
        unreachable!("mock<IDialogService>: info não é definido")
    }

    fn warn(&self, _message: &str, _detail: Option<&str>) -> Promise<()> {
        unreachable!("mock<IDialogService>: warn não é definido")
    }

    fn error(&self, _message: &str, _detail: Option<&str>) -> Promise<()> {
        unreachable!("mock<IDialogService>: error não é definido")
    }

    fn about(&self) -> Promise<()> {
        unreachable!("mock<IDialogService>: about não é definido")
    }
}

/// O `await` do resultado de `undo` ou `redo`: roda as microtarefas até a promessa assentar e confere que não rejeitou.
fn settled(result: MaybePromise<()>) {
    if let MaybePromise::Promise(promise) = result {
        runtime::run_until_idle();
        promise.result().expect("a promessa assentou").expect("a promessa não rejeitou");
    }
}

fn is_last_element(service: &UndoRedoService, resource: &URI, expected: Option<&Rc<TestElement>>) -> bool {
    match (service.get_last_element(resource), expected) {
        (None, None) => true,
        (Some(actual), Some(expected)) => actual.as_resource().is_some_and(|actual| std::ptr::addr_eq(Rc::as_ptr(actual), Rc::as_ptr(expected))),
        _ => false,
    }
}

fn is_last_workspace_element(service: &UndoRedoService, resource: &URI, expected: Option<&Rc<TestWorkspaceElement>>) -> bool {
    match (service.get_last_element(resource), expected) {
        (None, None) => true,
        (Some(actual), Some(expected)) => actual.as_workspace().is_some_and(|actual| std::ptr::addr_eq(Rc::as_ptr(actual), Rc::as_ptr(expected))),
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
    service.push_element(UndoRedoElementRef::Resource(element1.clone()), None, None);

    assert_eq!(element1.calls(), (0, 0));
    assert_eq!(states(&service, &resource), (true, false, true));
    assert!(is_last_element(&service, &resource, Some(&element1)));

    service.undo(UndoRedoTarget::Resource(&resource));
    assert_eq!(element1.calls(), (1, 0));
    assert_eq!(states(&service, &resource), (false, true, true));
    assert!(is_last_element(&service, &resource, None));

    service.redo(UndoRedoTarget::Resource(&resource));
    assert_eq!(element1.calls(), (1, 1));
    assert_eq!(states(&service, &resource), (true, false, true));
    assert!(is_last_element(&service, &resource, Some(&element1)));

    let element2 = TestElement::new(&resource, "typing 2");
    service.push_element(UndoRedoElementRef::Resource(element2.clone()), None, None);

    assert_eq!(element1.calls(), (1, 1));
    assert_eq!(element2.calls(), (0, 0));
    assert_eq!(states(&service, &resource), (true, false, true));
    assert!(is_last_element(&service, &resource, Some(&element2)));

    service.undo(UndoRedoTarget::Resource(&resource));

    assert_eq!(element1.calls(), (1, 1));
    assert_eq!(element2.calls(), (1, 0));
    assert_eq!(states(&service, &resource), (true, true, true));
    assert!(is_last_element(&service, &resource, None));

    let element3 = TestElement::new(&resource, "typing 2");
    service.push_element(UndoRedoElementRef::Resource(element3.clone()), None, None);

    assert_eq!(element1.calls(), (1, 1));
    assert_eq!(element2.calls(), (1, 0));
    assert_eq!(element3.calls(), (0, 0));
    assert_eq!(states(&service, &resource), (true, false, true));
    assert!(is_last_element(&service, &resource, Some(&element3)));

    service.undo(UndoRedoTarget::Resource(&resource));

    assert_eq!(element1.calls(), (1, 1));
    assert_eq!(element2.calls(), (1, 0));
    assert_eq!(element3.calls(), (1, 0));
    assert_eq!(states(&service, &resource), (true, true, true));
    assert!(is_last_element(&service, &resource, None));
}

#[test]
fn multi_resource_elements() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let resource1 = URI::file("test1.txt", Platform::Posix);
    let resource2 = URI::file("test2.txt", Platform::Posix);
    let service = UndoRedoService::new(Rc::new(ConfirmingDialogService), Rc::new(TestNotificationService));

    let split_element1 = TestElement::new(&resource1, "typing 1.1");
    let split_element2 = TestElement::new(&resource2, "typing 1.2");
    let element1 = Rc::new(TestWorkspaceElement {
        resources: vec![resource1.clone(), resource2.clone()],
        undo_calls: Cell::new(0),
        redo_calls: Cell::new(0),
        split_elements: vec![split_element1.clone(), split_element2.clone()],
    });
    service.push_element(UndoRedoElementRef::Workspace(element1.clone()), None, None);

    assert_eq!(states(&service, &resource1), (true, false, true));
    assert!(is_last_workspace_element(&service, &resource1, Some(&element1)));
    assert_eq!(states(&service, &resource2), (true, false, true));
    assert!(is_last_workspace_element(&service, &resource2, Some(&element1)));

    settled(service.undo(UndoRedoTarget::Resource(&resource1)));

    assert_eq!((element1.undo_calls.get(), element1.redo_calls.get()), (1, 0));
    assert_eq!(states(&service, &resource1), (false, true, true));
    assert!(is_last_workspace_element(&service, &resource1, None));
    assert_eq!(states(&service, &resource2), (false, true, true));
    assert!(is_last_workspace_element(&service, &resource2, None));

    settled(service.redo(UndoRedoTarget::Resource(&resource2)));
    assert_eq!((element1.undo_calls.get(), element1.redo_calls.get()), (1, 1));
    assert_eq!(split_element1.calls(), (0, 0));
    assert_eq!(split_element2.calls(), (0, 0));
    assert_eq!(states(&service, &resource1), (true, false, true));
    assert!(is_last_workspace_element(&service, &resource1, Some(&element1)));
    assert_eq!(states(&service, &resource2), (true, false, true));
    assert!(is_last_workspace_element(&service, &resource2, Some(&element1)));
}

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
        service.push_element(UndoRedoElementRef::Resource(element.clone()), None, None);
    }

    // Cria o snapshot depois de três elementos: [element1, element2, element3]
    let snapshot = service.create_snapshot(&resource);

    // Empilha mais elementos depois do snapshot
    for label in ["typing 4", "typing 5"] {
        service.push_element(UndoRedoElementRef::Resource(TestElement::new(&resource, label)), None, None);
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
        let past = current.past[index].as_resource().expect("o elemento é de um recurso");
        assert!(
            std::ptr::addr_eq(Rc::as_ptr(past), Rc::as_ptr(element)),
            "O elemento {} deveria ser element{}",
            index,
            index + 1
        );
    }
}
