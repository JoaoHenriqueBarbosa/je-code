//! Porte de `src/vs/base/test/browser/iconLabels.test.ts` (suíte `renderLabelWithIcons`), caso a caso e na mesma ordem.
//!
//! Notas de tradução: `elementsToString` (o `outerHTML` de cada elemento) é [`elements_to_string`]: um `span` vira
//! `<span class="...">filhos</span>`, o texto fica como está.

mod support;

use je_widgets::icon_labels::{LabelPart, render_label_with_icons};
use support::ensure_no_disposables_are_leaked_in_test_suite;

fn elements_to_string(elements: &[LabelPart]) -> String {
    elements
        .iter()
        .map(|part| match part {
            LabelPart::Text(text) => text.clone(),
            LabelPart::Span(span) => format!("<span class=\"{}\">{}</span>", span.class_names.join(" "), elements_to_string(&span.children)),
        })
        .collect()
}

#[test]
fn no_icons() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    let result = render_label_with_icons(" hello World .", false);

    assert_eq!(elements_to_string(&result), " hello World .");
}

#[test]
fn icons_only() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    let result = render_label_with_icons("$(alert)", false);

    assert_eq!(elements_to_string(&result), "<span class=\"codicon codicon-alert\"></span>");
}

#[test]
fn icon_and_non_icon_strings() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    let result = render_label_with_icons(" $(alert) Unresponsive", false);

    assert_eq!(elements_to_string(&result), " <span class=\"codicon codicon-alert\"></span> Unresponsive");
}

#[test]
fn multiple_icons() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    let result = render_label_with_icons("$(check)$(error)", false);

    assert_eq!(elements_to_string(&result), "<span class=\"codicon codicon-check\"></span><span class=\"codicon codicon-error\"></span>");
}

#[test]
fn escaped_icons() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    let result = render_label_with_icons("\\$(escaped)", false);

    assert_eq!(elements_to_string(&result), "$(escaped)");
}

#[test]
fn icon_with_animation() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    let result = render_label_with_icons("$(zip~anim)", false);

    assert_eq!(elements_to_string(&result), "<span class=\"codicon codicon-zip codicon-modifier-anim\"></span>");
}
