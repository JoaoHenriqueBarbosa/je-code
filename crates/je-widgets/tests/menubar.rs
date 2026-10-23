//! Porte de `src/vs/base/test/browser/ui/menu/menubar.test.ts` (suíte `Menubar`), caso a caso e na mesma ordem.
//!
//! Divergências: `getAttribute('aria-label')` é o rótulo do nó (`Tree::label`); o elemento `<mnemonic>` do
//! original é um nó com a classe `mnemonic` (o `je-ui` não tem tags), então `tagName === 'mnemonic'` é
//! `has_class("mnemonic")`; `textContent` é o texto de todos os `Content::Text` da subárvore. O contêiner é um nó
//! solto (`$('.container')`), um novo por teste (o original o reaproveita entre os casos).

mod support;

use std::rc::Rc;

use je_ui::{Content, NodeId, Ui};
use je_widgets::menu::unthemed_menu_styles;
use je_widgets::menu_bar::{IMenuBarOptions, MenuBar, MenuBarMenu};
use je_widgets::modifier_key_emitter::ModifierKeyEmitter;
use support::{create_container, ensure_no_disposables_are_leaked_in_test_suite};

fn text_content(ui: &Ui, node: NodeId) -> String {
    let tree = ui.tree();
    tree.subtree(node).into_iter().filter_map(|n| if let Content::Text { text, .. } = &tree.style(n).content { Some(text.clone()) } else { None }).collect()
}

fn get_button_element_by_aria_label(ui: &Ui, menubar_element: NodeId, aria_label: &str) -> Option<NodeId> {
    let tree = ui.tree();
    tree.children(menubar_element).iter().copied().find(|child| tree.label(*child) == Some(aria_label))
}

fn get_title_div_from_button_div(ui: &Ui, menu_button_element: NodeId) -> Option<NodeId> {
    let tree = ui.tree();
    tree.children(menu_button_element).iter().copied().find(|child| tree.has_class(*child, "menubar-menu-title"))
}

fn get_mnemonic_from_title_div(ui: &Ui, menu_title_div: NodeId) -> Option<String> {
    let tree = ui.tree();
    let mnemonic = tree.children(menu_title_div).iter().copied().find(|child| tree.has_class(*child, "mnemonic"));
    drop(tree);
    mnemonic.map(|node| text_content(ui, node))
}

fn validate_menu_bar_item(ui: &Ui, menubar: &Rc<MenuBar>, menubar_container: NodeId, label: &str, readable_label: &str, mnemonic: &str) {
    menubar.push(&[MenuBarMenu { actions: Vec::new(), label: label.to_owned() }]);

    let button_element = get_button_element_by_aria_label(ui, menubar_container, readable_label);
    assert!(button_element.is_some(), "Button element not found for {readable_label} button.");

    let title_div = get_title_div_from_button_div(ui, button_element.expect("botão"));
    assert!(title_div.is_some(), "Title div not found for {readable_label} button.");

    let mnem = get_mnemonic_from_title_div(ui, title_div.expect("título"));
    assert_eq!(mnem.as_deref(), Some(mnemonic), "Mnemonic not correct");
}

fn with_menu_menubar(callback: impl FnOnce(&Ui, &Rc<MenuBar>, NodeId)) {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = Ui::new();
    let container = create_container(&ui, 0.0, 0.0);
    let menubar = MenuBar::new(&ui, container, IMenuBarOptions { enable_mnemonics: true, visibility: Some("visible".to_owned()), ..Default::default() }, unthemed_menu_styles());

    callback(&ui, &menubar, container);

    je_base::lifecycle::IDisposable::dispose(&*menubar);
    ModifierKeyEmitter::dispose_instance(&ui);
}

#[test]
fn english_file_menu_renders_mnemonics() {
    with_menu_menubar(|ui, menubar, container| {
        validate_menu_bar_item(ui, menubar, container, "&File", "File", "F");
    });
}

#[test]
fn russian_file_menu_renders_mnemonics() {
    with_menu_menubar(|ui, menubar, container| {
        validate_menu_bar_item(ui, menubar, container, "&Файл", "Файл", "Ф");
    });
}

#[test]
fn chinese_file_menu_renders_mnemonics() {
    with_menu_menubar(|ui, menubar, container| {
        validate_menu_bar_item(ui, menubar, container, "文件(&F)", "文件", "F");
    });
}
