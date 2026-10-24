//! Porte de `src/vs/workbench/test/browser/parts/editor/modalEditorCompactHeader.test.ts` (suíte `Modal Editor Compact
//! Header`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - **`unknown` vira o tipo.** `isModalEditorOptionsProvider(obj: unknown)` e `canMoveModalEditorToEditorArea(obj:
//!   unknown)` recebem `undefined`, `null`, `{}` e `{ getModalEditorOptions: 'nope' }` no original, que a checagem
//!   estrutural rejeita. Em Rust a checagem é o tipo: um valor que não implementa `ModalEditorOptionsProvider` chega como
//!   `None`, e esses quatro casos são o `None`.
//! - **`modalElement.classList`.** `document.createElement('div')` e o `classList.toggle('compact-header', force)` são
//!   a [`ModalElement`], uma lista de classes que só sabe o que o caso observa (`toggle` com `force` e `contains`).
//!   `applyCompactHeaderClass` usa `is_compact_header`, a decisão de produção que o `ModalEditorPart.create` toma.
//! - **`headerElement.offsetHeight`.** `resolveHeaderHeight({ offsetHeight })` é `resolve_header_height(offset_height)`:
//!   o objeto com `offsetHeight` é o próprio número.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use std::cell::RefCell;
use std::collections::BTreeSet;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_platform::opener::editor_types::{ModalEditorOptions, ModalEditorOptionsProvider, can_move_modal_editor_to_editor_area, is_modal_editor_options_provider};
use je_workbench::browser::parts::editor::modal_editor_part::{MODAL_HEADER_HEIGHT, is_compact_header, resolve_header_height};

/// `{ classList: DOMTokenList }`.
#[derive(Default)]
struct ModalElement {
    class_list: RefCell<BTreeSet<String>>,
}

impl ModalElement {
    /// `classList.toggle(name, force)`.
    fn toggle(&self, name: &str, force: bool) {
        if force {
            self.class_list.borrow_mut().insert(name.to_owned());
        } else {
            self.class_list.borrow_mut().remove(name);
        }
    }

    /// `classList.contains(name)`.
    fn contains(&self, name: &str) -> bool {
        self.class_list.borrow().contains(name)
    }
}

/// `applyCompactHeaderClass(modalElement, activeEditor)`.
fn apply_compact_header_class(modal_element: &ModalElement, active_editor: Option<&dyn ModalEditorOptionsProvider>) {
    modal_element.toggle("compact-header", is_compact_header(active_editor));
}

/// `class TestEditor implements IModalEditorOptionsProvider`.
struct TestEditor {
    options: Option<ModalEditorOptions>,
}

impl ModalEditorOptionsProvider for TestEditor {
    fn get_modal_editor_options(&self) -> Option<ModalEditorOptions> {
        self.options
    }
}

/// `new TestEditor(options)`.
fn editor(options: Option<ModalEditorOptions>) -> TestEditor {
    TestEditor { options }
}

fn compact_header(compact_header: Option<bool>) -> ModalEditorOptions {
    ModalEditorOptions { compact_header, ..ModalEditorOptions::default() }
}

fn can_move_to_editor_area(can_move_to_editor_area: Option<bool>) -> ModalEditorOptions {
    ModalEditorOptions { can_move_to_editor_area, ..ModalEditorOptions::default() }
}

#[test]
fn is_modal_editor_options_provider_type_guard() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    assert!(!is_modal_editor_options_provider(None)); // undefined
    assert!(!is_modal_editor_options_provider(None)); // null
    assert!(!is_modal_editor_options_provider(None)); // {}
    assert!(!is_modal_editor_options_provider(None)); // { getModalEditorOptions: 'nope' }
    assert!(is_modal_editor_options_provider(Some(&editor(Some(compact_header(Some(true)))))));
}

#[test]
fn compact_header_class_toggles_based_on_active_editor_options() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let modal_element = ModalElement::default();

    // No active editor → class not set
    apply_compact_header_class(&modal_element, None);
    assert!(!modal_element.contains("compact-header"));

    // Editor that doesn't implement the provider → class not set
    apply_compact_header_class(&modal_element, None);
    assert!(!modal_element.contains("compact-header"));

    // Provider returning undefined → class not set
    apply_compact_header_class(&modal_element, Some(&editor(None)));
    assert!(!modal_element.contains("compact-header"));

    // Provider opting out → class not set
    apply_compact_header_class(&modal_element, Some(&editor(Some(compact_header(Some(false))))));
    assert!(!modal_element.contains("compact-header"));

    // Provider opting in → class set
    apply_compact_header_class(&modal_element, Some(&editor(Some(compact_header(Some(true))))));
    assert!(modal_element.contains("compact-header"));

    // Switching back to a non-compact editor clears the class
    apply_compact_header_class(&modal_element, Some(&editor(Some(compact_header(Some(false))))));
    assert!(!modal_element.contains("compact-header"));
}

#[test]
fn moving_to_an_editor_area_defaults_to_enabled_and_supports_opt_out() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    assert_eq!(
        vec![
            can_move_modal_editor_to_editor_area(None),
            can_move_modal_editor_to_editor_area(None), // {}
            can_move_modal_editor_to_editor_area(Some(&editor(None))),
            can_move_modal_editor_to_editor_area(Some(&editor(Some(ModalEditorOptions::default())))),
            can_move_modal_editor_to_editor_area(Some(&editor(Some(can_move_to_editor_area(Some(true)))))),
            can_move_modal_editor_to_editor_area(Some(&editor(Some(can_move_to_editor_area(Some(false)))))),
        ],
        vec![true, true, true, true, true, false]
    );
}

#[test]
fn header_height_prefers_measured_offset_height_over_fallback_constant() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    // Before layout: offsetHeight === 0 → fallback constant is used
    assert_eq!(resolve_header_height(0.0), MODAL_HEADER_HEIGHT);

    // Default header measured at 33px
    assert_eq!(resolve_header_height(33.0), 33.0);

    // Compact-header variant measured at 40px → measured value wins
    assert_eq!(resolve_header_height(40.0), 40.0);
}
