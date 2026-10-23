//! Testes extras do `mouseCursor` (o original só tem a constante e a regra de CSS `cursor: text`).

mod support;

use je_ui::{Cursor, Role, Ui};
use je_widgets::dom::create_el;
use je_widgets::mouse_cursor::{MOUSE_CURSOR_TEXT_CSS_CLASS_NAME, apply_mouse_cursor_text};
use support::ensure_no_disposables_are_leaked_in_test_suite;

#[test]
fn the_class_name_is_the_one_of_the_original() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(MOUSE_CURSOR_TEXT_CSS_CLASS_NAME, "monaco-mouse-cursor-text");
}

#[test]
fn toggles_the_class_and_the_text_cursor() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = Ui::new();
    let mut tree = ui.tree_mut();
    let root = tree.root();
    let node = create_el(&mut tree, root, Role::Generic, "");
    assert_eq!(tree.cursor(node), Cursor::Default);

    apply_mouse_cursor_text(&mut tree, node, true);
    assert!(tree.has_class(node, MOUSE_CURSOR_TEXT_CSS_CLASS_NAME));
    assert_eq!(tree.cursor(node), Cursor::Text);

    apply_mouse_cursor_text(&mut tree, node, false);
    assert!(!tree.has_class(node, MOUSE_CURSOR_TEXT_CSS_CLASS_NAME));
    assert_eq!(tree.cursor(node), Cursor::Default);
}

#[test]
fn children_inherit_the_text_cursor() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = Ui::new();
    let mut tree = ui.tree_mut();
    let root = tree.root();
    let parent = create_el(&mut tree, root, Role::Generic, "");
    let child = create_el(&mut tree, parent, Role::Generic, "");
    apply_mouse_cursor_text(&mut tree, parent, true);
    assert_eq!(tree.cursor(child), Cursor::Text);
}
