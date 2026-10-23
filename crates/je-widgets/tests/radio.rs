//! Porte de `src/vs/base/test/browser/ui/radio/radio.test.ts` (suíte `Radio`), caso a caso e na mesma ordem.
//!
//! Divergências (o `je-ui` não modela o que o navegador mede): `getComputedStyle` vira o estilo do nó
//! (`NodeStyle`, `Content`) e a caixa calculada do botão (`Button::compute_box`); `getBoundingClientRect` e
//! `offset*` viram `Tree::absolute_bounds` e `Tree::bounds`; o `transform` do indicador é o retângulo do nó
//! do indicador; as variáveis CSS que o teste escreve no `cssText` são `RadioStyles` e `RadioMetrics`;
//! `box-shadow: none` não tem campo (a árvore não pinta sombra), então a asserção `selectionShadow` fica
//! implícita; o `requestAnimationFrame` duplo é a chamada síncrona a `Radio::set_width` (o
//! `ResizeObserver`); `.hc-light` e `.monaco-reduce-motion` no pai não existem (alto contraste entra pelas
//! cores `contrast_*` do estilo, e a árvore não anima). Os três casos de animação (`transition`, Web
//! Animations) ficam `#[ignore]`.

mod support;

use std::cell::RefCell;
use std::rc::Rc;

use je_base::errors;
use je_base::lifecycle::{IDisposable, to_disposable};
use je_base::key_codes::KeyCode;
use je_render::Rgba;
use je_render::kurbo::Rect;
use je_ui::{Content, Event, EventKind, KeyData, Modifiers, NodeId, Ui};
use je_widgets::button::IButton;
use je_widgets::radio::{ArrowKeyBehavior, Radio, RadioOptionItem, RadioOptions, RadioStyles};
use support::{LeakGuard, create_container, ensure_no_disposables_are_leaked_in_test_suite, query_all};

fn item(text: &str) -> RadioOptionItem {
    RadioOptionItem::new(text)
}

fn segmented(items: Vec<RadioOptionItem>) -> RadioOptions {
    RadioOptions { class_name: Some("segmented".to_owned()), items, ..RadioOptions::default() }
}

fn create_radio(ui: &Ui, store: &LeakGuard, options: RadioOptions) -> Rc<Radio> {
    let radio = store.add_rc(Radio::new(ui, options));
    let container = create_container(ui, 0.0, 0.0);
    {
        let mut tree = ui.tree_mut();
        let root = tree.root();
        tree.append_child(root, container);
        tree.append_child(container, radio.dom_node());
    }
    let ui = ui.clone();
    store.add(to_disposable(move || ui.remove(container)));
    // position: absolute; top: 0; left: 0
    radio.set_position(0.0, 0.0);
    radio
}

fn key(code: KeyCode) -> KeyData {
    KeyData { key_code: code, modifiers: Modifiers::default() }
}

fn click(ui: &Ui, node: NodeId) {
    ui.dispatch(&mut Event::new(EventKind::Click, node));
}

fn keydown(ui: &Ui, node: NodeId, code: KeyCode) {
    ui.dispatch(&mut Event::new(EventKind::KeyDown, node).with_key(key(code)));
}

fn focused_is(ui: &Ui, node: NodeId) -> bool {
    ui.focused() == Some(node)
}

fn aria_checked(radio: &Radio) -> Vec<String> {
    radio.option_buttons().iter().map(|b| b.attribute("aria-checked").unwrap_or_default()).collect()
}

fn tab_indexes(radio: &Radio) -> Vec<i32> {
    radio.option_buttons().iter().map(|b| b.tab_index()).collect()
}

fn selected_and_activated(radio: &Radio, store: &LeakGuard) -> Rc<RefCell<Vec<String>>> {
    let events = Rc::new(RefCell::new(Vec::new()));
    let log = events.clone();
    store.add(radio.on_did_select().listen(move |i| log.borrow_mut().push(format!("selected:{i}"))));
    let log = events.clone();
    store.add(radio.on_did_activate().listen(move |i| log.borrow_mut().push(format!("activated:{i}"))));
    events
}

fn label_color_and_weight(ui: &Ui, radio: &Radio, index: usize) -> (Rgba, u16) {
    let part = radio.option_buttons()[index].label_parts()[0].node;
    match &ui.tree().style(part).content {
        Content::Text { color, style, .. } => (*color, style.weight),
        other => panic!("trecho de texto esperado, veio {other:?}"),
    }
}

fn segmented_controls_share_rounded_rectangles(high_contrast: bool) {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = Ui::new();
    let styles = RadioStyles {
        foreground: Some(Rgba::opaque(0x33, 0x33, 0x33)),
        inactive_border: Some(Rgba::opaque(0xDD, 0xDD, 0xDD)),
        active_border: Some(Rgba::opaque(0x00, 0x00, 0xFF)),
        active_background: Some(Rgba::opaque(0x00, 0x00, 0xFF)),
        contrast_border: high_contrast.then_some(Rgba::opaque(0x44, 0x44, 0x44)),
        contrast_active_border: high_contrast.then_some(Rgba::opaque(0x00, 0x6B, 0xBD)),
        ..RadioStyles::default()
    };
    let radio = create_radio(&ui, &store, RadioOptions { styles, ..segmented(vec![item("Low"), RadioOptionItem { is_active: true, ..item("Medium") }, item("High")]) });
    radio.set_width(Some(240.0));
    let indicator = radio.selection_indicator().expect("indicador");
    let tree = ui.tree();
    let group = tree.style(radio.dom_node());
    let indicator_style = tree.style(indicator);
    // canvas: fundo branco, depois a cor do indicador por cima.
    let fill = indicator_style.background.expect("fundo do indicador");
    let blend = |c: u8| (f64::from(c) * f64::from(fill.a) / 255.0 + 255.0 * (1.0 - f64::from(fill.a) / 255.0)).round() as u8;
    let (red, green, blue) = (blend(fill.r), blend(fill.g), blend(fill.b));
    let selected = tree.absolute_bounds(radio.option_elements()[1]);
    let indicator_bounds = tree.absolute_bounds(indicator);
    let outline = group.border.map(|b| (b.width, b.color));
    let group_radius = group.radius;
    let segment_radii: Vec<(f64, Option<[f64; 4]>)> = radio.option_elements().iter().map(|e| (tree.style(*e).radius, tree.style(*e).corner_radii)).collect();
    let selection_border = indicator_style.border.map(|b| b.color);
    let indicator_radius = indicator_style.radius;
    drop(tree);
    let label_colors: Vec<Rgba> = (0..3).map(|i| label_color_and_weight(&ui, &radio, i).0).collect();
    let label_weights: Vec<u16> = (0..3).map(|i| label_color_and_weight(&ui, &radio, i).1).collect();

    assert_eq!(outline, Some((1.0, if high_contrast { Rgba::opaque(0x44, 0x44, 0x44) } else { Rgba::opaque(0xDD, 0xDD, 0xDD) })));
    assert_eq!(group_radius, 6.0);
    assert_eq!(segment_radii, [(4.0, None), (4.0, None), (4.0, None)]);
    assert_eq!(indicator_radius, 4.0);
    assert_eq!(selection_border, Some(if high_contrast { Rgba::opaque(0x00, 0x6B, 0xBD) } else { Rgba::TRANSPARENT }));
    assert!(red == green && green == blue && red > 51 && red < 255, "preenchimento neutro: {red} {green} {blue}");
    assert_eq!(label_colors, [Rgba::opaque(0x33, 0x33, 0x33); 3]);
    assert_eq!(label_weights, [400, 600, 400]);
    assert!((selected.x0 - indicator_bounds.x0).abs() < 1.0 && (selected.y0 - indicator_bounds.y0).abs() < 1.0);
}

#[test]
fn segmented_controls_share_rounded_rectangles_and_a_neutral_selected_fill() {
    segmented_controls_share_rounded_rectangles(false);
}

#[test]
fn segmented_controls_share_rounded_rectangles_and_a_neutral_selected_fill_in_high_contrast() {
    segmented_controls_share_rounded_rectangles(true);
}

#[test]
fn the_decorative_selection_indicator_follows_unequal_options_and_resizes_without_moving_buttons() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = Ui::new();
    let radio = create_radio(&ui, &store, segmented(["None", "Low", "Medium", "High", "Extra High", "Max"].map(item).to_vec()));
    radio.set_width(Some(276.0));
    let indicator = radio.selection_indicator().expect("indicador");
    let aligned = |index: usize| -> bool {
        let tree = ui.tree();
        let option = tree.bounds(radio.option_elements()[index]);
        let rect: Rect = tree.bounds(indicator);
        rect == option && radio.selection_indicator_bounds().is_some_and(|b| (b.left, b.top, b.width, b.height) == (option.x0, option.y0, option.width(), option.height()))
    };
    let selections: Vec<bool> = (0..radio.option_elements().len())
        .map(|index| {
            radio.set_active_item(index as isize);
            aligned(index)
        })
        .collect();
    radio.set_width(Some(360.0));
    assert_eq!(selections, [true; 6]);
    assert!(aligned(5), "resized");
    assert_eq!(radio.selection_indicator_attribute("aria-hidden").as_deref(), Some("true"));
    assert_eq!(radio.option_elements().len(), 6);
}

#[test]
fn items_that_start_a_row_wrap_the_segments_slide_only_within_a_row_and_are_replaced_with_the_items() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = Ui::new();
    let radio = create_radio(
        &ui,
        &store,
        segmented(vec![RadioOptionItem { is_active: true, ..item("Low") }, item("High"), RadioOptionItem { starts_row: true, ..item("Alternative") }, item("Other")]),
    );
    radio.set_width(Some(400.0));
    let rows = || -> Vec<Vec<String>> {
        let buttons = radio.option_buttons();
        let tops: Vec<f64> = buttons.iter().map(|b| ui.tree().bounds(b.element()).y0).collect();
        let mut unique: Vec<f64> = Vec::new();
        for top in &tops {
            if !unique.contains(top) {
                unique.push(*top);
            }
        }
        unique.iter().map(|top| buttons.iter().zip(&tops).filter(|(_, t)| *t == top).map(|(b, _)| b.text_content()).collect()).collect()
    };
    let animates = |index: isize| {
        radio.set_active_item(index);
        ui.tree().has_class(radio.dom_node(), "animate-selection")
    };
    let initial_rows = rows();
    let within_row = animates(1);
    let across_rows = animates(2);
    let same_row = animates(3);
    radio.set_items(vec![item("One"), item("Two")]);
    radio.layout();
    let breaks = query_all(&ui.tree(), radio.dom_node(), &["monaco-radio-row-break"]).len();
    let multi_row = ui.tree().has_class(radio.dom_node(), "multi-row");

    assert_eq!(initial_rows, [vec!["Low", "High"], vec!["Alternative", "Other"]]);
    assert!(within_row);
    assert!(!across_rows);
    assert!(same_row);
    assert_eq!(rows(), [vec!["One", "Two"]]);
    assert_eq!(breaks, 0);
    assert!(!multi_row);
}

#[test]
#[ignore = "lê `transitionDuration` computado (CSS transition) e `getAnimations()`: a árvore do je-ui não anima, o indicador pula"]
fn segmented_motion_uses_a_short_slide_and_respects_reduced_motion() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = Ui::new();
    let radio = create_radio(&ui, &store, segmented(vec![item("One"), item("Two"), item("Six")]));
    radio.set_width(Some(200.0));
    radio.set_active_item(1);
    radio.set_active_item(2);
    // Só a espera é portável: sem animação ela termina na hora.
    assert!(radio.when_selection_animation_settles().is_settled());
}

#[test]
#[ignore = "depende da Web Animations API (`indicator.animate`, `animation.finish`): a árvore do je-ui não tem animações"]
fn selection_waiting_follows_interruptions_and_settles_on_completion() {}

#[test]
#[ignore = "depende da Web Animations API (`indicator.animate`, `animation.cancel`): a árvore do je-ui não tem animações"]
fn selection_waiting_follows_interruptions_and_settles_on_disposal() {}

fn segmented_option_bounds_stay_fixed(width: f64, labels: &[&str]) {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = Ui::new();
    let radio = create_radio(&ui, &store, segmented(labels.iter().map(|l| item(l)).collect()));
    radio.set_width(Some(width));
    let measure = || -> Vec<Rect> { radio.option_elements().iter().map(|e| ui.tree().absolute_bounds(*e)).collect() };
    let initial = measure();
    let selections: Vec<Vec<Rect>> = (0..labels.len())
        .map(|index| {
            click(&ui, radio.option_elements()[index]);
            measure()
        })
        .collect();

    assert!(initial.iter().all(|b| b.width() > 0.0 && b.height() > 0.0), "laid out");
    assert_eq!(selections, vec![initial; labels.len()]);
}

macro_rules! bounds_test {
    ($name:ident, $width:expr, [$($label:expr),+]) => {
        #[test]
        fn $name() {
            segmented_option_bounds_stay_fixed($width, &[$($label),+]);
        }
    };
}

bounds_test!(segmented_option_bounds_stay_fixed_at_200px_low_medium_high, 200.0, ["Low", "Medium", "High"]);
bounds_test!(segmented_option_bounds_stay_fixed_at_276px_low_medium_high, 276.0, ["Low", "Medium", "High"]);
bounds_test!(segmented_option_bounds_stay_fixed_at_200px_low_medium_high_extra_high, 200.0, ["Low", "Medium", "High", "Extra High"]);
bounds_test!(segmented_option_bounds_stay_fixed_at_276px_low_medium_high_extra_high, 276.0, ["Low", "Medium", "High", "Extra High"]);
bounds_test!(segmented_option_bounds_stay_fixed_at_200px_minimal_low_medium_high_max, 200.0, ["Minimal", "Low", "Medium", "High", "Max"]);
bounds_test!(segmented_option_bounds_stay_fixed_at_276px_minimal_low_medium_high_max, 276.0, ["Minimal", "Low", "Medium", "High", "Max"]);
bounds_test!(segmented_option_bounds_stay_fixed_at_200px_none_low_medium_high_extra_high_max, 200.0, ["None", "Low", "Medium", "High", "Extra High", "Max"]);
bounds_test!(segmented_option_bounds_stay_fixed_at_276px_none_low_medium_high_extra_high_max, 276.0, ["None", "Low", "Medium", "High", "Extra High", "Max"]);
bounds_test!(segmented_option_bounds_stay_fixed_at_200px_standard_zap_fast, 200.0, ["Standard", "$(zap) Fast"]);
bounds_test!(segmented_option_bounds_stay_fixed_at_276px_standard_zap_fast, 276.0, ["Standard", "$(zap) Fast"]);

#[test]
fn segmented_label_sizing_follows_temporary_labels_and_restoration_without_duplicating_icons() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = Ui::new();
    let radio = create_radio(&ui, &store, segmented(vec![RadioOptionItem { aria_label: Some("Fast".to_owned()), ..item("$(zap) Fast") }]));
    let button = radio.option_buttons()[0].clone();
    let read_label = || {
        let parts = button.label_parts();
        (
            button.text_content(),
            parts.iter().filter_map(|p| p.data_label.clone()).collect::<Vec<_>>(),
            parts.iter().filter(|p| p.kind == je_widgets::button::LabelPartKind::Icon).count(),
            button.attribute("aria-label"),
        )
    };
    let initial = read_label();
    let override_label = store.add(radio.override_option_label(0, "$(check) Standard"));
    let temporary = read_label();
    override_label.dispose();

    assert_eq!(initial, ("Fast".to_owned(), vec!["Fast".to_owned()], 1, Some("Fast".to_owned())));
    assert_eq!(temporary, ("Standard".to_owned(), vec!["Standard".to_owned()], 1, Some("Fast".to_owned())));
    assert_eq!(read_label(), initial);
}

#[test]
fn joined_radios_retain_shared_borders_and_do_not_add_label_sizing() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = Ui::new();
    let radio = create_radio(&ui, &store, RadioOptions { items: vec![item("One"), item("Two"), item("Three")], ..RadioOptions::default() });
    let borders = || -> Vec<[f64; 2]> {
        radio
            .option_buttons()
            .iter()
            .map(|b| {
                let widths = b.compute_box().border_widths;
                [widths[3], widths[1]]
            })
            .collect()
    };
    let first_selected = borders();
    radio.set_active_item(1);
    let second_selected = borders();
    let sizing_labels: usize = radio.option_buttons().iter().map(|b| b.label_parts().iter().filter(|p| p.data_label.is_some()).count()).sum();

    assert_eq!(first_selected, [[1.0, 1.0], [0.0, 0.0], [1.0, 1.0]]);
    assert_eq!(second_selected, [[1.0, 0.0], [1.0, 1.0], [0.0, 1.0]]);
    assert_eq!(sizing_labels, 0);
}

#[test]
fn segmented_arrows_can_move_focus_without_selecting_until_enter() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = Ui::new();
    let radio = create_radio(&ui, &store, RadioOptions { arrow_key_behavior: ArrowKeyBehavior::Focus, ..segmented(vec![item("Low"), item("Medium"), item("High")]) });
    let selected = Rc::new(RefCell::new(Vec::new()));
    let log = selected.clone();
    store.add(radio.on_did_select().listen(move |i| log.borrow_mut().push(*i)));
    let options = radio.option_elements();
    let (low, medium) = (options[0], options[1]);
    ui.focus(low);
    keydown(&ui, low, KeyCode::RightArrow);
    let before_enter = (focused_is(&ui, medium), aria_checked(&radio), tab_indexes(&radio));
    keydown(&ui, medium, KeyCode::Enter);

    assert_eq!(before_enter, (true, vec!["true".to_owned(), "false".to_owned(), "false".to_owned()], vec![-1, 0, -1]));
    assert_eq!(*selected.borrow(), [1]);
    assert_eq!(aria_checked(&radio), ["false", "true", "false"]);
}

#[test]
fn activation_fires_for_clicks_on_both_selected_and_unselected_items_without_changing_selection_events() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = Ui::new();
    let radio = create_radio(&ui, &store, RadioOptions { items: vec![item("One"), item("Two")], ..RadioOptions::default() });
    let events = selected_and_activated(&radio, &store);
    let options = radio.option_elements();
    click(&ui, options[0]);
    click(&ui, options[1]);
    click(&ui, options[1]);
    radio.set_active_item(0);

    assert_eq!(*events.borrow(), ["activated:0", "selected:1", "activated:1", "activated:1"]);
}

fn activates_both_selected_and_unselected_items_exactly_once(code: KeyCode) {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = Ui::new();
    let radio = create_radio(&ui, &store, RadioOptions { arrow_key_behavior: ArrowKeyBehavior::Focus, items: vec![item("One"), item("Two")], ..RadioOptions::default() });
    let events = selected_and_activated(&radio, &store);
    for index in [0, 1, 1] {
        keydown(&ui, radio.option_elements()[index], code);
    }

    assert_eq!(*events.borrow(), ["activated:0", "selected:1", "activated:1", "activated:1"]);
}

#[test]
fn enter_activates_both_selected_and_unselected_items_exactly_once() {
    activates_both_selected_and_unselected_items_exactly_once(KeyCode::Enter);
}

#[test]
fn space_activates_both_selected_and_unselected_items_exactly_once() {
    activates_both_selected_and_unselected_items_exactly_once(KeyCode::Space);
}

fn arrows_skip_disabled_items_and_never_activate(behavior: ArrowKeyBehavior) {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = Ui::new();
    let radio = create_radio(&ui, &store, RadioOptions { arrow_key_behavior: behavior, items: vec![item("One"), RadioOptionItem { disabled: true, ..item("Two") }, item("Three")], ..RadioOptions::default() });
    let events = selected_and_activated(&radio, &store);
    radio.focus_active_item();
    let options = radio.option_elements();
    keydown(&ui, options[0], KeyCode::RightArrow);
    let focused_after_arrow = focused_is(&ui, options[2]);
    keydown(&ui, options[2], KeyCode::RightArrow);
    let focused_after_wrap = focused_is(&ui, options[0]);

    let expected: &[&str] = if behavior == ArrowKeyBehavior::Select { &["selected:2", "selected:0"] } else { &[] };
    assert_eq!(*events.borrow(), expected, "selected e activated (nenhum activated)");
    assert!(focused_after_arrow);
    assert!(focused_after_wrap);
}

#[test]
fn arrows_in_select_mode_skip_disabled_items_and_never_activate() {
    arrows_skip_disabled_items_and_never_activate(ArrowKeyBehavior::Select);
}

#[test]
fn arrows_in_focus_mode_skip_disabled_items_and_never_activate() {
    arrows_skip_disabled_items_and_never_activate(ArrowKeyBehavior::Focus);
}

#[test]
fn disabled_items_and_disabled_controls_cannot_be_activated() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = Ui::new();
    let radio = create_radio(&ui, &store, RadioOptions { items: vec![item("One"), RadioOptionItem { disabled: true, ..item("Two") }], ..RadioOptions::default() });
    let events = selected_and_activated(&radio, &store);
    let activate = |element: NodeId| {
        click(&ui, element);
        keydown(&ui, element, KeyCode::Enter);
        keydown(&ui, element, KeyCode::Space);
    };
    activate(radio.option_elements()[1]);
    radio.set_enabled(false);
    activate(radio.option_elements()[0]);

    assert!(events.borrow().is_empty());
}

#[test]
fn restoring_focus_keeps_one_tab_stop_without_selecting_or_activating() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = Ui::new();
    let radio = create_radio(&ui, &store, RadioOptions { arrow_key_behavior: ArrowKeyBehavior::Focus, items: vec![item("One"), item("Two")], ..RadioOptions::default() });
    let events = selected_and_activated(&radio, &store);
    let options = radio.option_elements();
    radio.focus_item(1);
    let focused_item = (focused_is(&ui, options[1]), tab_indexes(&radio), aria_checked(&radio));
    radio.focus_active_item();

    assert_eq!(focused_item, (true, vec![-1, 0], vec!["true".to_owned(), "false".to_owned()]));
    assert!(events.borrow().is_empty());
    assert!(focused_is(&ui, options[0]), "activeFocused");
    assert_eq!(tab_indexes(&radio), [0, -1]);
}

#[test]
fn replaced_and_disposed_buttons_no_longer_select_or_activate() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = Ui::new();
    let radio = create_radio(&ui, &store, RadioOptions { items: vec![item("One"), item("Two")], ..RadioOptions::default() });
    let events = selected_and_activated(&radio, &store);
    let previous_button = radio.option_elements()[1];
    radio.set_items(vec![item("Three")]);
    click(&ui, previous_button);
    let current_button = radio.option_elements()[0];
    radio.dispose();
    click(&ui, current_button);

    assert!(events.borrow().is_empty());
}

#[test]
fn focusing_an_invalid_item_reports_an_error_without_changing_the_tab_order() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = Ui::new();
    let radio = create_radio(&ui, &store, RadioOptions { items: vec![item("One")], ..RadioOptions::default() });
    for index in [-1, 1] {
        let error = errors::catch(|| radio.focus_item(index)).expect_err("deveria lançar");
        assert!(error.to_string().contains("Invalid Index"), "{error}");
    }
    assert_eq!(tab_indexes(&radio), [0]);
}
