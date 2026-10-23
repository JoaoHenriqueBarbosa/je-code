//! Testes extras do `HighlightedLabel`: medidas dos nós, cores do realce, ícones, memoização do `set`, corte com
//! reticências e o hover do título (o original só testa o `innerHTML`, em `highlighted_label.rs`).

mod hover_fixture;
mod support;

use je_render::Rgba;
use je_ui::{Content, NodeId, Tree, Ui};
use je_widgets::highlighted_label::{HighlightedLabel, HighlightedLabelStyles, IHighlight};
use je_widgets::text::{measure_text, ui_font};
use support::{create_container, ensure_no_disposables_are_leaked_in_test_suite};

const TEXT: Rgba = Rgba::opaque(0xcc, 0xcc, 0xcc);
const HIGHLIGHT: Rgba = Rgba::opaque(0x2a, 0xaa, 0xff);

fn styles() -> HighlightedLabelStyles {
    HighlightedLabelStyles { foreground: TEXT, highlight_foreground: Some(HIGHLIGHT), highlight_weight: 700, font_size: 13.0, font_weight: 400, line_height: Some(22.0), italic: false, strikethrough: false }
}

fn make(ui: &Ui, store: &support::LeakGuard) -> std::rc::Rc<HighlightedLabel> {
    let container = create_container(ui, 0.0, 0.0);
    store.add_rc(HighlightedLabel::new(ui, container, None, styles()))
}

/// Cada nó de texto sob `node`, na ordem do DOM: texto, cor, peso e retângulo.
fn runs(tree: &Tree, node: NodeId) -> Vec<(String, Rgba, u16, f64, f64)> {
    tree.subtree(node)
        .into_iter()
        .filter_map(|n| match &tree.style(n).content {
            Content::Text { text, color, style, .. } => Some((text.clone(), *color, style.weight, tree.bounds(n).x0, tree.bounds(n).width())),
            _ => None,
        })
        .collect()
}

#[test]
fn the_label_is_as_wide_as_its_text_and_one_line_tall() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = Ui::new();
    let label = make(&ui, &store);
    label.set(Some("hello world"), &[], "", false, false);
    let font = ui_font(13.0, 400);
    let r = ui.tree().bounds(label.element());
    assert_eq!((r.width(), r.height()), (measure_text("hello world", &font), 22.0));
    assert_eq!(label.width(), r.width());
    assert!(ui.tree().has_class(label.element(), "monaco-highlighted-label"));
}

#[test]
fn the_runs_are_laid_out_one_after_the_other() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = Ui::new();
    let label = make(&ui, &store);
    label.set(Some("foobarfoo"), &[IHighlight::new(3, 6)], "", false, false);
    let font = ui_font(13.0, 400);
    let bold = ui_font(13.0, 700);
    let tree = ui.tree();
    let found = runs(&tree, label.element());
    assert_eq!(found.iter().map(|r| r.0.as_str()).collect::<Vec<_>>(), ["foo", "bar", "foo"]);
    let (foo, bar) = (measure_text("foo", &font), measure_text("bar", &bold));
    assert_eq!(found[0].3, 0.0);
    // O realce é um `span` em `x = foo`, e o texto dentro dele começa em 0 do `span`.
    let span = tree.children_with_class(label.element(), "highlight")[0];
    assert_eq!(tree.bounds(span).x0, foo);
    assert_eq!(tree.bounds(span).width(), bar);
    assert_eq!(found[2].3, foo + bar);
}

#[test]
fn the_highlight_has_its_own_color_and_weight() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = Ui::new();
    let label = make(&ui, &store);
    label.set(Some("foobarfoo"), &[IHighlight::new(3, 6)], "", false, false);
    let found = runs(&ui.tree(), label.element());
    assert_eq!((found[0].1, found[0].2), (TEXT, 400));
    assert_eq!((found[1].1, found[1].2), (HIGHLIGHT, 700));
    assert_eq!((found[2].1, found[2].2), (TEXT, 400));
}

#[test]
fn extra_classes_go_on_the_highlight_span() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = Ui::new();
    let label = make(&ui, &store);
    label.set(Some("hello"), &[IHighlight { start: 0, end: 5, extra_classes: vec!["deprecated".to_owned()] }], "", false, false);
    let tree = ui.tree();
    let span = tree.children(label.element())[0];
    assert_eq!(tree.classes(span), ["highlight", "deprecated"]);
}

#[test]
fn empty_highlights_are_skipped_and_the_gap_before_the_next_is_kept() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = Ui::new();
    let label = make(&ui, &store);
    label.set(Some("abcdef"), &[IHighlight::new(2, 2), IHighlight::new(4, 5)], "", false, false);
    let tree = ui.tree();
    let found = runs(&tree, label.element());
    assert_eq!(found.iter().map(|r| r.0.as_str()).collect::<Vec<_>>(), ["abcd", "e", "f"]);
}

#[test]
fn highlights_count_utf16_code_units() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = Ui::new();
    let label = make(&ui, &store);
    // O emoji ocupa duas unidades UTF-16: o realce `[1, 3)` é ele inteiro.
    label.set(Some("a\u{1F600}b"), &[IHighlight::new(1, 3)], "", false, false);
    let found = runs(&ui.tree(), label.element());
    assert_eq!(found.iter().map(|r| r.0.as_str()).collect::<Vec<_>>(), ["a", "\u{1F600}", "b"]);
    assert_eq!(found[1].1, HIGHLIGHT);
}

#[test]
fn icons_are_sixteen_pixels_wide_and_sit_in_the_text_flow() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = Ui::new();
    let label = make(&ui, &store);
    label.set(Some("$(symbol-class) hello"), &[], "", false, true);
    let tree = ui.tree();
    let icon = tree.children(label.element())[0];
    assert!(matches!(&tree.style(icon).content, Content::Codicon { id, size, .. } if id == "symbol-class" && *size == 16.0));
    let r = tree.bounds(icon);
    assert_eq!((r.x0, r.width(), r.height()), (0.0, 16.0, 16.0));
    assert_eq!(r.y0, 3.0, "centrado na linha de 22 px");
    let text = tree.children(label.element())[1];
    assert_eq!(tree.bounds(text).x0, 16.0);
    assert_eq!(label.width(), 16.0 + measure_text(" hello", &ui_font(13.0, 400)));
}

#[test]
fn without_support_for_icons_the_syntax_stays_as_text() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = Ui::new();
    let label = make(&ui, &store);
    label.set(Some("$(alert) x"), &[], "", false, false);
    let found = runs(&ui.tree(), label.element());
    assert_eq!(found.iter().map(|r| r.0.as_str()).collect::<Vec<_>>(), ["$(alert) x"]);
}

#[test]
fn setting_the_same_values_keeps_the_nodes() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = Ui::new();
    let label = make(&ui, &store);
    label.set(Some("hello"), &[IHighlight::new(0, 2)], "t", false, false);
    let before = ui.tree().children(label.element()).to_vec();
    label.set(Some("hello"), &[IHighlight::new(0, 2)], "t", false, false);
    assert_eq!(ui.tree().children(label.element()), before);
    label.set(Some("hello"), &[IHighlight::new(0, 3)], "t", false, false);
    assert_ne!(ui.tree().children(label.element()), before);
}

#[test]
fn escape_new_lines_option_replaces_line_breaks_and_shifts_the_highlights() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = Ui::new();
    let label = make(&ui, &store);
    label.set(Some("ACTION\r\n_TYPE2"), &[IHighlight::new(7, 9)], "", true, false);
    assert_eq!(label.text(), "ACTION\u{23CE}_TYPE2");
    let found = runs(&ui.tree(), label.element());
    assert_eq!(found.iter().map(|r| r.0.as_str()).collect::<Vec<_>>(), ["ACTION", "\u{23CE}_", "TYPE2"]);
}

#[test]
fn text_beyond_the_max_width_is_cut_with_an_ellipsis() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = Ui::new();
    let label = make(&ui, &store);
    label.set(Some("an extremely long label"), &[], "", false, false);
    let natural = label.width();
    label.set_max_width(Some(60.0));
    assert!(label.width() <= 60.0 && label.width() > 40.0, "{}", label.width());
    let found = runs(&ui.tree(), label.element());
    assert!(found[0].0.ends_with('\u{2026}'));
    assert_eq!(ui.tree().bounds(label.element()).width(), label.width());
    label.set_max_width(None);
    assert_eq!(label.width(), natural);
    assert_eq!(runs(&ui.tree(), label.element())[0].0, "an extremely long label");
}

#[test]
fn what_comes_after_the_cut_is_hidden() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = Ui::new();
    let label = make(&ui, &store);
    label.set(Some("aaaaaaaaaaaa"), &[IHighlight::new(0, 4), IHighlight::new(4, 8), IHighlight::new(8, 12)], "", false, false);
    label.set_max_width(Some(50.0));
    let tree = ui.tree();
    let children = tree.children(label.element()).to_vec();
    assert_eq!(children.iter().map(|c| tree.style(*c).visible).collect::<Vec<_>>(), [true, true, false], "o segundo é cortado com reticências, o terceiro some");
}

#[test]
fn set_styles_repaints_with_the_new_colors() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = Ui::new();
    let label = make(&ui, &store);
    label.set(Some("foobarfoo"), &[IHighlight::new(3, 6)], "", false, false);
    let other = Rgba::opaque(1, 2, 3);
    label.set_styles(HighlightedLabelStyles { foreground: other, highlight_foreground: None, ..styles() });
    let found = runs(&ui.tree(), label.element());
    assert_eq!((found[0].1, found[1].1, found[2].1), (other, other, other), "sem cor de realce, o realce herda a do texto");
}

#[test]
fn the_title_creates_the_hover_once_and_then_updates_it() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let log = hover_fixture::install();
    let ui = Ui::new();
    let label = make(&ui, &store);
    label.set(Some("a"), &[], "", false, false);
    assert!(log.events.borrow().is_empty(), "sem título não há hover");
    label.set(Some("a"), &[], "dica", false, false);
    label.set(Some("a"), &[], "outra", false, false);
    let events = log.events.borrow().clone();
    assert_eq!(events.len(), 2);
    assert!(events[0].ends_with(&format!(":{}", "dica")) && events[0].starts_with("managed:"));
    assert_eq!(events[1], "update:outra");
}

#[test]
fn dispose_disposes_the_hover() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let log = hover_fixture::install();
    let ui = Ui::new();
    let container = create_container(&ui, 0.0, 0.0);
    let label = HighlightedLabel::new(&ui, container, None, styles());
    label.set(Some("a"), &[], "dica", false, false);
    je_base::lifecycle::IDisposable::dispose(&*label);
    assert_eq!(log.events.borrow().last().map(String::as_str), Some("dispose"));
}
