//! Testes extras (sem equivalente no VS Code) do comportamento portado de `radio.ts`: a geometria do
//! `display: flex` do grupo, as classes de estado, os raios por canto e os estilos padrão.

mod support;

use je_base::errors;
use je_render::Rgba;
use je_ui::{Event, EventKind, Ui};
use je_widgets::radio::{Radio, RadioOptionItem, RadioOptions, RadioStyles, default_radio_styles};
use support::{ensure_no_disposables_are_leaked_in_test_suite, load_theme};

fn items(texts: &[&str]) -> Vec<RadioOptionItem> {
    texts.iter().map(|t| RadioOptionItem::new(*t)).collect()
}

fn attach(ui: &Ui, radio: &Radio) {
    let mut tree = ui.tree_mut();
    let root = tree.root();
    tree.append_child(root, radio.dom_node());
}

#[test]
fn default_styles_resolve_every_key_in_both_themes() {
    for name in ["dark", "light"] {
        let styles = default_radio_styles(&load_theme(name));
        assert!(styles.foreground.is_some() && styles.toolbar_hover_background.is_some() && styles.focus_border.is_some(), "{name}");
        assert!(styles.active_foreground.is_some() && styles.active_background.is_some(), "{name}");
    }
}

#[test]
fn segmented_group_geometry_follows_the_css() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = Ui::new();
    let radio = store.add_rc(Radio::new(&ui, RadioOptions { class_name: Some("segmented".to_owned()), items: items(&["Low", "Medium", "High"]), ..RadioOptions::default() }));
    attach(&ui, &radio);
    radio.set_position(10.0, 20.0);
    radio.set_width(Some(240.0));
    let tree = ui.tree();
    let group = tree.bounds(radio.dom_node());
    // borda de 1px, padding de 2px e uma opção de 24px: 30px de altura.
    assert_eq!((group.x0, group.y0, group.width(), group.height()), (10.0, 20.0, 240.0, 30.0));
    let boxes: Vec<_> = radio.option_elements().iter().map(|e| tree.bounds(*e)).collect();
    let share = (240.0 - 2.0 * 3.0 - 2.0 * 2.0) / 3.0;
    for (index, b) in boxes.iter().enumerate() {
        assert!((b.width() - share).abs() < 0.01, "{index}: {b:?}");
        assert_eq!((b.y0, b.height()), (3.0, 24.0));
    }
    assert_eq!(boxes[0].x0, 3.0);
    assert!((boxes[1].x0 - (boxes[0].x1 + 2.0)).abs() < 0.01, "gap de 2px");
}

#[test]
fn auto_width_group_is_the_sum_of_the_options() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = Ui::new();
    let radio = store.add_rc(Radio::new(&ui, RadioOptions { class_name: Some("segmented".to_owned()), items: items(&["One", "Two"]), ..RadioOptions::default() }));
    attach(&ui, &radio);
    radio.layout();
    let tree = ui.tree();
    let boxes: Vec<_> = radio.option_elements().iter().map(|e| tree.bounds(*e)).collect();
    let group = tree.bounds(radio.dom_node());
    assert!((group.width() - (boxes[1].x1 + 3.0)).abs() < 0.01, "{group:?} {boxes:?}");
    assert!(boxes.iter().all(|b| b.width() >= 14.0 + 10.0), "{boxes:?}");
}

#[test]
fn state_classes_and_aria_checked_follow_the_active_item() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = Ui::new();
    let radio = store.add_rc(Radio::new(&ui, RadioOptions { items: items(&["One", "Two", "Three"]), ..RadioOptions::default() }));
    attach(&ui, &radio);
    radio.set_active_item(1);
    let tree = ui.tree();
    let state: Vec<(bool, bool)> = radio.option_elements().iter().map(|e| (tree.has_class(*e, "active"), tree.has_class(*e, "previous-active"))).collect();
    assert_eq!(state, [(false, false), (true, false), (false, true)]);
    assert_eq!(radio.option_buttons()[1].attribute("aria-checked").as_deref(), Some("true"));
    assert_eq!(radio.option_buttons()[1].tab_index(), 0);
    assert_eq!(radio.option_buttons()[0].tab_index(), -1);
    assert_eq!(tree.role(radio.dom_node()), je_ui::Role::RadioGroup);
    assert_eq!(tree.role(radio.option_elements()[0]), je_ui::Role::Radio);
}

#[test]
fn joined_radio_rounds_only_the_outer_corners_and_shares_the_width() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = Ui::new();
    let radio = store.add_rc(Radio::new(&ui, RadioOptions { items: items(&["One", "Two", "Three"]), ..RadioOptions::default() }));
    attach(&ui, &radio);
    radio.set_width(Some(300.0));
    let tree = ui.tree();
    let options = radio.option_elements();
    let radii: Vec<[f64; 4]> = options.iter().map(|e| tree.style(*e).corner_radii.unwrap_or([tree.style(*e).radius; 4])).collect();
    assert_eq!(radii, [[3.0, 0.0, 0.0, 3.0], [0.0; 4], [0.0, 3.0, 3.0, 0.0]]);
    let widths: Vec<f64> = options.iter().map(|e| tree.bounds(*e).width()).collect();
    assert!((widths.iter().sum::<f64>() - 300.0).abs() < 0.01, "{widths:?}");
    // Cada opção é `width: 100%` e encolhe na proporção do tamanho interno (sem padding e borda), que difere
    // por poucos pixels entre as opções porque as bordas de cada uma são outras.
    assert!(widths.iter().all(|w| (w - 100.0).abs() < 2.0), "{widths:?}");
    let heights: Vec<f64> = options.iter().map(|e| tree.bounds(*e).height()).collect();
    assert!(heights.iter().all(|h| *h == heights[0]), "align-items: stretch: {heights:?}");
}

#[test]
fn hover_changes_the_inactive_background_but_not_the_active_one() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = Ui::new();
    let styles = RadioStyles {
        inactive_background: Some(Rgba::opaque(1, 1, 1)),
        inactive_hover_background: Some(Rgba::opaque(2, 2, 2)),
        active_background: Some(Rgba::opaque(3, 3, 3)),
        ..RadioStyles::default()
    };
    let radio = store.add_rc(Radio::new(&ui, RadioOptions { items: items(&["One", "Two"]), styles, ..RadioOptions::default() }));
    attach(&ui, &radio);
    let [one, two] = radio.option_elements()[..] else { panic!("duas opções") };
    let background = |node| ui.tree().style(node).background;
    assert_eq!((background(one), background(two)), (Some(Rgba::opaque(3, 3, 3)), Some(Rgba::opaque(1, 1, 1))));
    ui.dispatch(&mut Event::new(EventKind::PointerEnter, two));
    assert_eq!(background(two), Some(Rgba::opaque(2, 2, 2)));
    ui.dispatch(&mut Event::new(EventKind::PointerEnter, one));
    assert_eq!(background(one), Some(Rgba::opaque(3, 3, 3)), "o ativo ignora o hover");
    ui.dispatch(&mut Event::new(EventKind::PointerLeave, two));
    assert_eq!(background(two), Some(Rgba::opaque(1, 1, 1)));
}

#[test]
fn invalid_active_index_throws_and_set_items_resets_the_indicator() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = Ui::new();
    let radio = store.add_rc(Radio::new(&ui, RadioOptions { class_name: Some("segmented".to_owned()), items: items(&["One", "Two"]), ..RadioOptions::default() }));
    attach(&ui, &radio);
    radio.set_width(Some(200.0));
    for index in [-1, 2] {
        let error = errors::catch(|| radio.set_active_item(index)).expect_err("deveria lançar");
        assert!(error.to_string().contains("Invalid Index"));
    }
    let indicator = radio.selection_indicator().expect("indicador");
    assert!(ui.tree().style(indicator).visible && ui.tree().has_class(radio.dom_node(), "selection-indicator-ready"));
    radio.set_items(Vec::new());
    assert!(!ui.tree().has_class(radio.dom_node(), "selection-indicator-ready") && radio.selection_indicator_bounds().is_none());
    assert!(!ui.tree().style(indicator).visible);
}

#[test]
fn overriding_a_label_restores_it_on_dispose_and_dispose_removes_the_group() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = Ui::new();
    let radio = Radio::new(&ui, RadioOptions { items: items(&["Original"]), ..RadioOptions::default() });
    attach(&ui, &radio);
    let button = radio.option_buttons()[0].clone();
    let restore = radio.override_option_label(0, "Temporary");
    assert_eq!(button.text_content(), "Temporary");
    restore.dispose();
    assert_eq!(button.text_content(), "Original");
    let group = radio.dom_node();
    je_base::lifecycle::IDisposable::dispose(&*radio);
    assert!(!ui.tree().is_alive(group));
}
