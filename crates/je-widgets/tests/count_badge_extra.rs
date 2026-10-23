//! Testes extras do `CountBadge` (o original não tem teste dedicado): formatação do número, estilo, tamanho
//! pela regra de `countBadge.css` e o hover do título.

mod hover_fixture;
mod support;

use je_base::lifecycle::IDisposable;
use je_render::Rgba;
use je_render::kurbo::Rect;
use je_ui::{Content, Role, Ui};
use je_widgets::count_badge::{CountBadge, ICountBadgeOptions, ICountBadgeStyles, UNTHEMED_COUNT_STYLES};
use je_widgets::dom::create_el;
use je_widgets::text::{measure_text, natural_line_height, ui_font};
use support::{create_container, ensure_no_disposables_are_leaked_in_test_suite};

fn text_of(ui: &Ui, badge: &CountBadge) -> String {
    let tree = ui.tree();
    let child = tree.children(badge.element())[0];
    match &tree.style(child).content {
        Content::Text { text, .. } => text.clone(),
        other => panic!("conteúdo inesperado: {other:?}"),
    }
}

#[test]
fn renders_the_count_through_the_count_format() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = Ui::new();
    let container = create_container(&ui, 100.0, 100.0);
    let badge = store.add_rc(CountBadge::new(&ui, container, &ICountBadgeOptions { count: Some(5), count_format: Some("{0}+".to_owned()), ..Default::default() }, UNTHEMED_COUNT_STYLES));
    assert!(ui.tree().has_class(badge.element(), "monaco-count-badge"));
    assert_eq!(text_of(&ui, &badge), "5+");
    badge.set_count(42);
    assert_eq!(text_of(&ui, &badge), "42+");
    badge.set_count_format("({0})");
    assert_eq!(text_of(&ui, &badge), "(42)");
}

#[test]
fn defaults_to_zero_and_the_plain_format() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = Ui::new();
    let container = create_container(&ui, 100.0, 100.0);
    let badge = store.add_rc(CountBadge::new(&ui, container, &ICountBadgeOptions::default(), UNTHEMED_COUNT_STYLES));
    assert_eq!(text_of(&ui, &badge), "0");
}

#[test]
fn paints_the_themed_colors_and_the_border() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = Ui::new();
    let container = create_container(&ui, 100.0, 100.0);
    let styles = ICountBadgeStyles { badge_background: Some(Rgba::opaque(1, 2, 3)), badge_foreground: Some(Rgba::opaque(4, 5, 6)), badge_border: Some(Rgba::opaque(7, 8, 9)) };
    let badge = store.add_rc(CountBadge::new(&ui, container, &ICountBadgeOptions { count: Some(3), ..Default::default() }, styles));
    let tree = ui.tree();
    let style = tree.style(badge.element());
    assert_eq!(style.background, Some(Rgba::opaque(1, 2, 3)));
    assert_eq!(style.border.map(|b| (b.width, b.color)), Some((1.0, Rgba::opaque(7, 8, 9))));
    assert_eq!(style.radius, 11.0);
    let Content::Text { color, .. } = &tree.style(tree.children(badge.element())[0]).content else { panic!("sem texto") };
    assert_eq!(*color, Rgba::opaque(4, 5, 6));
}

#[test]
fn a_short_count_has_the_minimum_size_of_18_px() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = Ui::new();
    let container = create_container(&ui, 100.0, 100.0);
    let badge = store.add_rc(CountBadge::new(&ui, container, &ICountBadgeOptions { count: Some(1), ..Default::default() }, UNTHEMED_COUNT_STYLES));
    let r = ui.tree().bounds(badge.element());
    assert_eq!((r.width(), r.height()), (18.0, 18.0), "min-width e min-height de 18 px");
}

#[test]
fn a_long_count_grows_by_the_text_width_plus_the_padding() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = Ui::new();
    let container = create_container(&ui, 100.0, 100.0);
    let badge = store.add_rc(CountBadge::new(&ui, container, &ICountBadgeOptions { count: Some(123_456), ..Default::default() }, UNTHEMED_COUNT_STYLES));
    let text_width = measure_text("123456", &ui_font(11.0, 400));
    let r = ui.tree().bounds(badge.element());
    assert_eq!(r.width(), text_width + 10.0, "padding de 5 px dos dois lados");
    assert_eq!(r.height(), 18.0);
}

#[test]
fn the_border_counts_inside_the_box() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = Ui::new();
    let container = create_container(&ui, 100.0, 100.0);
    let styles = ICountBadgeStyles { badge_border: Some(Rgba::opaque(7, 8, 9)), ..UNTHEMED_COUNT_STYLES };
    let badge = store.add_rc(CountBadge::new(&ui, container, &ICountBadgeOptions { count: Some(123_456), ..Default::default() }, styles));
    let text_width = measure_text("123456", &ui_font(11.0, 400));
    let r = ui.tree().bounds(badge.element());
    assert_eq!(r.width(), text_width + 12.0, "box-sizing: border-box: 10 de padding e 2 de borda");
    assert_eq!(r.height(), 19.0, "11 de linha, 6 de padding e 2 de borda");
}

#[test]
fn the_long_variant_uses_the_small_radius_and_the_natural_line_height() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = Ui::new();
    let container = create_container(&ui, 100.0, 100.0);
    let badge = store.add_rc(CountBadge::new(&ui, container, &ICountBadgeOptions { count: Some(123_456), ..Default::default() }, UNTHEMED_COUNT_STYLES));
    ui.tree_mut().add_class(badge.element(), "long");
    badge.refresh();
    let text_width = measure_text("123456", &ui_font(11.0, 400));
    let tree = ui.tree();
    let r = tree.bounds(badge.element());
    assert_eq!(tree.style(badge.element()).radius, 2.0);
    assert_eq!(r.width(), (text_width + 6.0).max(18.0), "padding de 3 px");
    assert_eq!(r.height(), natural_line_height(&ui_font(11.0, 400)) + 4.0, "`min-height: auto`, padding de 2 px");
}

#[test]
fn keeps_the_position_the_owner_gave() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = Ui::new();
    let container = create_container(&ui, 100.0, 100.0);
    let badge = store.add_rc(CountBadge::new(&ui, container, &ICountBadgeOptions::default(), UNTHEMED_COUNT_STYLES));
    ui.tree_mut().set_bounds(badge.element(), Rect::new(30.0, 40.0, 30.0, 40.0));
    badge.set_count(9);
    let r = ui.tree().bounds(badge.element());
    assert_eq!((r.x0, r.y0), (30.0, 40.0));
}

#[test]
fn dispose_removes_the_element_from_the_container() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = Ui::new();
    let container = {
        let mut tree = ui.tree_mut();
        let root = tree.root();
        create_el(&mut tree, root, Role::Generic, "")
    };
    let badge = CountBadge::new(&ui, container, &ICountBadgeOptions::default(), UNTHEMED_COUNT_STYLES);
    assert_eq!(ui.tree().children(container).len(), 1);
    badge.dispose();
    assert!(ui.tree().children(container).is_empty());
    ui.tree_mut().remove(container);
}

#[test]
fn the_title_format_sets_up_and_tears_down_a_hover_at_the_mouse() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let log = hover_fixture::install();
    let ui = Ui::new();
    let container = create_container(&ui, 100.0, 100.0);
    let badge = store.add_rc(CountBadge::new(&ui, container, &ICountBadgeOptions { count: Some(3), ..Default::default() }, UNTHEMED_COUNT_STYLES));
    assert!(log.delayed.borrow().is_none(), "sem `titleFormat` não há hover");

    badge.set_title_format("{0} pendentes");
    let (target, options) = log.delayed.borrow().clone().expect("hover criado");
    assert_eq!(target, badge.element());
    let first = options();
    assert_eq!(hover_fixture::hover_text(&first.content), "3 pendentes");
    assert_eq!(first.appearance.and_then(|a| a.compact), Some(true));

    badge.set_count(8);
    assert_eq!(hover_fixture::hover_text(&options().content), "8 pendentes", "o conteúdo é lido na hora de mostrar");

    badge.set_title_format("");
    assert_eq!(*log.delayed_disposed.borrow(), 1);
}
