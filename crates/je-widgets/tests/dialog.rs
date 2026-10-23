//! Porte de `src/vs/base/test/browser/ui/dialog/dialog.test.ts` (suíte `Dialog`), caso a caso e na mesma ordem.
//!
//! Divergências de tradução (a árvore do `je-ui` não tem `<textarea>`, `contenteditable`, `textContent`
//! nem `style.cssText`):
//!
//! - O `<textarea>` e o `<div contenteditable>` do primeiro caso são nós com papel `TextInput` (o que o
//!   diálogo trata como alvo editável); o `KeyboardEvent` de `ArrowRight` é um `KeyDown` despachado ao nó
//!   focado, e `defaultPrevented` é o `!dispatch(..)`.
//! - O `textContent` do detalhe é o rótulo do nó (o diálogo guarda lá o texto inteiro) e o texto de um
//!   subárvore é a concatenação do `Content::Text` dos nós, na ordem do documento. `children.length === 0`
//!   (detalhe de texto simples) vira "todo filho é uma linha de texto (`dialog-line`)", já que a árvore
//!   representa cada linha quebrada como um nó.
//! - O `tabIndex` do link é [`Dialog::tab_index`]; o `style.color` e o `style.textDecoration` do link são
//!   [`Dialog::link_style`] (cor e sublinhado); os do botão (`Button.element.style.color`) são o `foreground`
//!   da caixa calculada do botão, e a ausência de decoração é a ausência de [`LinkStyle`].

mod support;

use je_base::key_codes::KeyCode;
use je_base::lifecycle::{IDisposable, to_disposable};
use je_render::Rgba;
use je_ui::{Content, Event, EventKind, KeyData, Modifiers, NodeId, Role, Tree, Ui};
use je_widgets::button::{Button, ButtonOptions, ButtonStyles, IButton, unthemed_button_styles};
use je_widgets::dialog::{Dialog, DialogOptions, DialogStyles, LinkStyle};
use je_widgets::dom::create_el;
use je_widgets::input_box::unthemed_input_box_styles;
use je_widgets::toggle::CheckboxStyles;
use support::{create_container, ensure_no_disposables_are_leaked_in_test_suite, query_all};

fn unthemed_dialog_styles() -> DialogStyles {
    DialogStyles::default()
}

fn unthemed_checkbox_styles() -> CheckboxStyles {
    CheckboxStyles::default()
}

fn options(dialog_styles: DialogStyles) -> DialogOptions {
    DialogOptions::new(unthemed_button_styles(), unthemed_checkbox_styles(), unthemed_input_box_styles(), dialog_styles)
}

/// `append(document.body, $('.test-dialog-container'))` e o `container.remove()` ao fim.
fn test_container(ui: &Ui, disposables: &support::LeakGuard) -> NodeId {
    let container = create_container(ui, 800.0, 600.0);
    let root = ui.root();
    ui.tree_mut().append_child(root, container);
    ui.tree_mut().set_class_name(container, "test-dialog-container");
    let ui = ui.clone();
    disposables.add(to_disposable(move || ui.remove(container)));
    container
}

/// O `textContent`: o texto dos nós da subárvore, na ordem do documento.
fn text_content(tree: &Tree, node: NodeId) -> String {
    tree.subtree(node)
        .into_iter()
        .filter_map(|n| match &tree.style(n).content {
            Content::Text { text, .. } => Some(text.clone()),
            _ => None,
        })
        .collect()
}

fn set_text(tree: &mut Tree, node: NodeId, text: &str) {
    tree.style_mut(node).content = Content::Text { text: text.to_owned(), style: je_widgets::text::ui_font(13.0, 400), color: Rgba::opaque(255, 255, 255), line_height: 18.0 };
}

#[test]
fn keeps_horizontal_arrow_keys_in_editable_custom_body_controls() {
    let disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = Ui::new();
    let container = test_container(&ui, &disposables);
    let controls = std::rc::Rc::new(std::cell::RefCell::new(Vec::<NodeId>::new()));
    let created = controls.clone();
    let dialog = disposables.add_rc(Dialog::new(
        &ui,
        container,
        "Message",
        Some(vec!["Save".to_owned(), "Cancel".to_owned()]),
        DialogOptions {
            render_body: Some(std::rc::Rc::new(move |ui, body| {
                let mut tree = ui.tree_mut();
                // textarea e contentEditable
                created.borrow_mut().push(create_el(&mut tree, body, Role::TextInput, ""));
                created.borrow_mut().push(create_el(&mut tree, body, Role::TextInput, ""));
            })),
            ..options(unthemed_dialog_styles())
        },
    ));
    let result = dialog.show();
    let controls = controls.borrow().clone();
    let (textarea, content_editable) = (controls[0], controls[1]);

    let dispatch_arrow_right = |target: NodeId| {
        ui.focus(target);
        let key = KeyData { key_code: KeyCode::RightArrow, modifiers: Modifiers::default() };
        let not_prevented = ui.dispatch(&mut Event::new(EventKind::KeyDown, target).with_key(key));
        (ui.focused(), !not_prevented)
    };

    assert_eq!(
        (dispatch_arrow_right(textarea), dispatch_arrow_right(content_editable)),
        ((Some(textarea), false), (Some(content_editable), false))
    );

    dialog.dispose();
    assert!(result.is_settled());
}

#[test]
fn renders_a_plain_string_detail_as_text() {
    let disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = Ui::new();
    let container = test_container(&ui, &disposables);
    let dialog = disposables.add_rc(Dialog::new(&ui, container, "Message", Some(vec!["OK".to_owned()]), DialogOptions { detail: Some("Some detail text".to_owned()), ..options(unthemed_dialog_styles()) }));
    let result = dialog.show();

    let tree = ui.tree();
    let detail_element = query_all(&tree, container, &["dialog-message-detail"])[0];
    assert_eq!(tree.label(detail_element), Some("Some detail text"));
    assert_eq!(text_content(&tree, detail_element), "Some detail text");
    assert!(tree.children(detail_element).iter().all(|c| tree.has_class(*c, "dialog-line")));
    drop(tree);

    dialog.dispose();
    assert!(result.is_settled());
}

#[test]
fn applies_modal_blocker_classes() {
    let disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = Ui::new();
    let container = test_container(&ui, &disposables);
    let dialog = disposables.add_rc(Dialog::new(&ui, container, "Message", Some(vec!["OK".to_owned()]), DialogOptions { modal_block_extra_classes: vec!["test-modal-block".to_owned()], ..options(unthemed_dialog_styles()) }));
    let result = dialog.show();

    let tree = ui.tree();
    let modal = query_all(&tree, container, &["monaco-dialog-modal-block"]).first().copied();
    assert_eq!(modal.map(|m| tree.has_class(m, "test-modal-block")), Some(true));
    drop(tree);

    dialog.dispose();
    assert!(result.is_settled());
}

#[test]
fn prefers_a_pre_rendered_detail_element_over_plain_detail_text_and_makes_its_links_keyboard_focusable() {
    let disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = Ui::new();
    let container = test_container(&ui, &disposables);

    let (rendered, link) = {
        let mut tree = ui.tree_mut();
        let rendered = tree.create_detached(Role::Generic);
        tree.set_class_name(rendered, "rendered-markdown");
        let link = create_el(&mut tree, rendered, Role::Link, "");
        set_text(&mut tree, link, "Command Link");
        (rendered, link)
    };

    let dialog = disposables.add_rc(Dialog::new(
        &ui,
        container,
        "Message",
        Some(vec!["OK".to_owned()]),
        DialogOptions { detail: Some("ignored plain-text detail".to_owned()), detail_element: Some(rendered), ..options(unthemed_dialog_styles()) },
    ));
    let result = dialog.show();

    let tree = ui.tree();
    let detail_element = query_all(&tree, container, &["dialog-message-detail"])[0];
    assert!(tree.is_ancestor(detail_element, rendered));
    assert_eq!(text_content(&tree, detail_element), "Command Link");
    drop(tree);
    assert_eq!(dialog.tab_index(link), Some(0));

    dialog.dispose();
    assert!(result.is_settled());
}

#[test]
fn focuses_a_footer_only_action_without_applying_hyperlink_styles_to_it() {
    let disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = Ui::new();
    let container = test_container(&ui, &disposables);
    let created = std::rc::Rc::new(std::cell::RefCell::new(None::<(std::rc::Rc<Button>, NodeId)>));
    let (created_in_footer, store) = (created.clone(), disposables.store());
    let dialog = disposables.add_rc(Dialog::new(
        &ui,
        container,
        "Message",
        Some(Vec::new()),
        DialogOptions {
            disable_default_action: true,
            render_footer: Some(std::rc::Rc::new(move |ui, footer| {
                let action = store.add_rc(Button::new(ui, footer, ButtonOptions { styles: ButtonStyles { button_foreground: Some(Rgba::opaque(1, 2, 3)), ..ButtonStyles::default() }, ..ButtonOptions::default() }));
                action.set_label("Cancel".into());
                let mut tree = ui.tree_mut();
                let link = create_el(&mut tree, footer, Role::Link, "");
                set_text(&mut tree, link, "Terms");
                *created_in_footer.borrow_mut() = Some((action, link));
            })),
            ..options(DialogStyles { text_link_foreground: Some(Rgba::opaque(4, 5, 6)), ..unthemed_dialog_styles() })
        },
    ));
    let result = dialog.show();
    let (action, link) = created.borrow().clone().expect("o rodapé foi renderizado");

    let link_color = match &ui.tree().style(link).content {
        Content::Text { color, .. } => *color,
        other => panic!("o link perdeu o texto: {other:?}"),
    };
    assert_eq!(
        (ui.focused(), action.compute_box().foreground, dialog.link_style(action.element()), link_color, dialog.link_style(link)),
        (Some(action.element()), Some(Rgba::opaque(1, 2, 3)), None, Rgba::opaque(4, 5, 6), Some(LinkStyle { color: Rgba::opaque(4, 5, 6), underline: true }))
    );

    dialog.dispose();
    assert!(result.is_settled());
}
