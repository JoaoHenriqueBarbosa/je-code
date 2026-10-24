//! Porte de `src/vs/editor/contrib/hover/test/browser/hoverCopyButton.test.ts` (suíte `Hover Copy Button`), caso a
//! caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` é o `LeakGuard` criado no começo de cada caso; o `disposables`
//!   da suíte é o próprio guarda (`ds.add_rc`).
//! - O `container` (`div` no `document.body`) é o `HoverContainer`, e `container.querySelector('.hover-copy-button')`
//!   é `query_selector("hover-copy-button")`. O ícone `.codicon-copy` é a classe `codicon-copy` do botão.
//! - `NullHoverService` não existe como parâmetro (ver `hover_copy_button.rs`); o `status` de `aria` é uma
//!   função vazia.
//! - `buttonElement.click()` é `SimpleButton::click`, e o `KeyboardEvent` com `keyCode` 13 ou 32 que o
//!   `StandardKeyboardEvent` traduz é `SimpleButton::key_down(KeyCode::Enter | KeyCode::Space)`.
//! - `await clipboardService.readText()` lê a promessa já resolvida do `TestClipboardService`.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod support;

use std::rc::Rc;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::key_codes::KeyCode;
use je_editor::hover::hover_copy_button::{HoverContainer, HoverCopyButton};
use je_editor::services::clipboard_service::IClipboardService;
use support::test_clipboard_service::TestClipboardService;

fn setup() -> (Rc<TestClipboardService>, HoverContainer) {
    (Rc::new(TestClipboardService::new()), HoverContainer::new())
}

fn create_button(
    container: &HoverContainer,
    clipboard_service: &Rc<TestClipboardService>,
    get_content: impl Fn() -> String + 'static,
) -> Rc<HoverCopyButton> {
    let clipboard: Rc<dyn IClipboardService> = clipboard_service.clone();
    Rc::new(HoverCopyButton::new(container, get_content, clipboard, |_| {}))
}

fn read_clipboard(clipboard_service: &TestClipboardService) -> String {
    clipboard_service.read_text().result().expect("promessa resolvida").expect("leitura sem erro")
}

#[test]
fn should_create_button_element_in_container() {
    let ds = ensure_no_disposables_are_leaked_in_test_suite();
    let (clipboard_service, container) = setup();

    ds.add_rc(create_button(&container, &clipboard_service, || "test content".to_string()));

    let button_element = container.query_selector("hover-copy-button").expect("Button element should be created");
    assert_eq!(button_element.role(), "button");
    assert_eq!(button_element.tab_index(), 0);
    assert_eq!(button_element.aria_label(), "Copy");
}

#[test]
fn should_add_hover_row_with_copy_class_to_container() {
    let ds = ensure_no_disposables_are_leaked_in_test_suite();
    let (clipboard_service, container) = setup();

    assert!(!container.has_class("hover-row-with-copy"), "Container should not have class before button creation");

    ds.add_rc(create_button(&container, &clipboard_service, || "test content".to_string()));

    assert!(container.has_class("hover-row-with-copy"), "Container should have hover-row-with-copy class after button creation");
}

#[test]
fn should_have_copy_icon() {
    let ds = ensure_no_disposables_are_leaked_in_test_suite();
    let (clipboard_service, container) = setup();

    ds.add_rc(create_button(&container, &clipboard_service, || "test content".to_string()));

    let button_element = container.query_selector("hover-copy-button").expect("Button element should be created");
    assert!(button_element.has_class("codicon-copy"), "Copy icon should be present");
}

#[test]
fn should_copy_content_on_click() {
    let ds = ensure_no_disposables_are_leaked_in_test_suite();
    let (clipboard_service, container) = setup();

    let test_content = "test content to copy";
    ds.add_rc(create_button(&container, &clipboard_service, move || test_content.to_string()));

    let button_element = container.query_selector("hover-copy-button").expect("botão criado");

    button_element.click();

    let copied_text = read_clipboard(&clipboard_service);
    assert_eq!(copied_text, test_content, "Content should be copied to clipboard");
}

#[test]
fn should_copy_content_on_enter_key() {
    let ds = ensure_no_disposables_are_leaked_in_test_suite();
    let (clipboard_service, container) = setup();

    let test_content = "test content for enter key";
    ds.add_rc(create_button(&container, &clipboard_service, move || test_content.to_string()));

    let button_element = container.query_selector("hover-copy-button").expect("botão criado");

    // Simulate Enter key press
    button_element.key_down(KeyCode::Enter);

    let copied_text = read_clipboard(&clipboard_service);
    assert_eq!(copied_text, test_content, "Content should be copied on Enter key");
}

#[test]
fn should_copy_content_on_space_key() {
    let ds = ensure_no_disposables_are_leaked_in_test_suite();
    let (clipboard_service, container) = setup();

    let test_content = "test content for space key";
    ds.add_rc(create_button(&container, &clipboard_service, move || test_content.to_string()));

    let button_element = container.query_selector("hover-copy-button").expect("botão criado");

    // Simulate Space key press
    button_element.key_down(KeyCode::Space);

    let copied_text = read_clipboard(&clipboard_service);
    assert_eq!(copied_text, test_content, "Content should be copied on Space key");
}
