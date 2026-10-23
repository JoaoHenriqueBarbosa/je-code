//! Porte de `src/vs/base/test/browser/highlightedLabel.test.ts` (suíte `HighlightedLabel`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `label.element.innerHTML` é [`inner_html`]: percorre os filhos do nó. Um nó sem classes com [`Content::Text`] é
//!   um nó de texto do DOM (com `&`, `<` e `>` escapados como o `innerHTML`); um nó com classes é um `span`.
//! - `new HighlightedLabel(document.createElement('div'))` é um rótulo num contêiner solto; o `teardown` que
//!   descarta o rótulo é o `store` do guarda de vazamento.

mod support;

use je_render::Rgba;
use je_ui::{Content, NodeId, Tree, Ui};
use je_widgets::highlighted_label::{HighlightedLabel, HighlightedLabelStyles, IHighlight};
use support::{create_container, ensure_no_disposables_are_leaked_in_test_suite};

fn styles() -> HighlightedLabelStyles {
    HighlightedLabelStyles {
        foreground: Rgba::opaque(0xcc, 0xcc, 0xcc),
        highlight_foreground: Some(Rgba::opaque(0x2a, 0xaa, 0xff)),
        highlight_weight: 400,
        font_size: 13.0,
        font_weight: 400,
        line_height: Some(22.0),
        italic: false,
        strikethrough: false,
    }
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

/// O `innerHTML` do nó.
fn inner_html(tree: &Tree, node: NodeId) -> String {
    tree.children(node)
        .iter()
        .map(|child| {
            let style = tree.style(*child);
            match &style.content {
                Content::Text { text, .. } if tree.children(*child).is_empty() && tree.classes(*child).is_empty() => escape(text),
                _ => format!("<span class=\"{}\">{}</span>", tree.classes(*child).join(" "), inner_html(tree, *child)),
            }
        })
        .collect()
}

fn make_label(ui: &Ui, store: &support::LeakGuard) -> std::rc::Rc<HighlightedLabel> {
    let container = create_container(ui, 0.0, 0.0);
    store.add_rc(HighlightedLabel::new(ui, container, None, styles()))
}

#[test]
fn empty_label() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = Ui::new();
    let label = make_label(&ui, &store);
    assert_eq!(inner_html(&ui.tree(), label.element()), "");
}

#[test]
fn no_decorations() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = Ui::new();
    let label = make_label(&ui, &store);
    label.set(Some("hello"), &[], "", false, false);
    assert_eq!(inner_html(&ui.tree(), label.element()), "hello");
}

#[test]
fn colored_symbol_icon() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = Ui::new();
    let label = make_label(&ui, &store);
    label.set(Some("$(symbol-class) hello"), &[], "", false, true);
    assert_eq!(inner_html(&ui.tree(), label.element()), "<span class=\"codicon codicon-symbol-class codicon-colored\"></span> hello");
}

#[test]
fn escape_html() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = Ui::new();
    let label = make_label(&ui, &store);
    label.set(Some("hel<lo"), &[], "", false, false);
    assert_eq!(inner_html(&ui.tree(), label.element()), "hel&lt;lo");
}

#[test]
fn everything_highlighted() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = Ui::new();
    let label = make_label(&ui, &store);
    label.set(Some("hello"), &[IHighlight::new(0, 5)], "", false, false);
    assert_eq!(inner_html(&ui.tree(), label.element()), "<span class=\"highlight\">hello</span>");
}

#[test]
fn beginning_highlighted() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = Ui::new();
    let label = make_label(&ui, &store);
    label.set(Some("hellothere"), &[IHighlight::new(0, 5)], "", false, false);
    assert_eq!(inner_html(&ui.tree(), label.element()), "<span class=\"highlight\">hello</span>there");
}

#[test]
fn ending_highlighted() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = Ui::new();
    let label = make_label(&ui, &store);
    label.set(Some("goodbye"), &[IHighlight::new(4, 7)], "", false, false);
    assert_eq!(inner_html(&ui.tree(), label.element()), "good<span class=\"highlight\">bye</span>");
}

#[test]
fn middle_highlighted() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = Ui::new();
    let label = make_label(&ui, &store);
    label.set(Some("foobarfoo"), &[IHighlight::new(3, 6)], "", false, false);
    assert_eq!(inner_html(&ui.tree(), label.element()), "foo<span class=\"highlight\">bar</span>foo");
}

#[test]
fn escape_new_lines() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();

    let mut highlights = vec![IHighlight::new(0, 5), IHighlight::new(7, 9), IHighlight::new(11, 12)]; // before,after,after
    let mut escaped = HighlightedLabel::escape_new_lines("ACTION\r\n_TYPE2", &mut highlights);
    assert_eq!(escaped, "ACTION\u{23CE}_TYPE2");
    assert_eq!(highlights, [IHighlight::new(0, 5), IHighlight::new(6, 8), IHighlight::new(10, 11)]);

    highlights = vec![IHighlight::new(5, 9), IHighlight::new(11, 12)]; // overlap,after
    escaped = HighlightedLabel::escape_new_lines("ACTION\r\n_TYPE2", &mut highlights);
    assert_eq!(escaped, "ACTION\u{23CE}_TYPE2");
    assert_eq!(highlights, [IHighlight::new(5, 8), IHighlight::new(10, 11)]);
}
