//! Testes extras (sem equivalente no VS Code) do comportamento portado de `findInput.ts` e `findInput.css`:
//! os botões comuns e adicionais, os eventos, a reserva de largura, o desabilitar, o realce e a navegação
//! por setas. O `findInput` não tem `.test.ts` na base do VS Code.

mod support;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use je_base::key_codes::KeyCode;
use je_base::lifecycle::IDisposable;
use je_render::Rgba;
use je_render::kurbo::Rect;
use je_ui::{Event, EventKind, KeyData, Modifiers, NodeId, Ui};
use je_widgets::find_input::{FindInput, FindInputLayoutStyle, FindInputOptions, FindInputStyles};
use je_widgets::input_box::{Message, MessageType, unthemed_input_box_styles};
use je_widgets::find_input_toggles::{FindInputToggleOpts, regex_toggle};
use je_widgets::toggle::unthemed_toggle_styles;
use support::{create_container, ensure_no_disposables_are_leaked_in_test_suite};

const DISABLED: Rgba = Rgba::opaque(0x33, 0x33, 0x33);

fn styles() -> FindInputStyles {
    FindInputStyles { input_box_styles: unthemed_input_box_styles(), toggle_styles: unthemed_toggle_styles(), disabled_background: Some(DISABLED) }
}

fn options(show_common_find_toggles: bool) -> FindInputOptions {
    FindInputOptions { show_common_find_toggles, caret_blink: false, ..FindInputOptions::new("Find", styles()) }
}

fn setup(opts: FindInputOptions) -> (Ui, NodeId, Rc<FindInput>) {
    let ui = Ui::new();
    let parent = create_container(&ui, 300.0, 100.0);
    let root = ui.tree().root();
    ui.tree_mut().append_child(root, parent);
    let find_input = FindInput::new(&ui, Some(parent), None, opts);
    (ui, parent, find_input)
}

fn click(ui: &Ui, node: NodeId) {
    ui.dispatch(&mut Event::new(EventKind::Click, node));
}

fn key(ui: &Ui, node: NodeId, key_code: KeyCode) -> bool {
    ui.dispatch(&mut Event::new(EventKind::KeyDown, node).with_key(KeyData { key_code, modifiers: Modifiers::default() }))
}

fn control_children(ui: &Ui, find_input: &FindInput) -> Vec<NodeId> {
    ui.tree().children(find_input.controls()).to_vec()
}

#[test]
fn without_common_toggles_the_controls_are_hidden_and_the_getters_are_false() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    let (ui, _parent, find_input) = setup(options(false));
    assert!(!ui.tree().style(find_input.controls()).visible);
    assert!(control_children(&ui, &find_input).is_empty());
    find_input.set_case_sensitive(true);
    find_input.set_regex(true);
    assert!(!find_input.get_case_sensitive() && !find_input.get_whole_words() && !find_input.get_regex());
    find_input.dispose();
}

#[test]
fn the_common_toggles_are_in_the_controls_in_the_order_case_words_regex() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    let (ui, _parent, find_input) = setup(FindInputOptions { append_regex_label: " (Alt+R)".to_owned(), ..options(true) });
    assert!(ui.tree().style(find_input.controls()).visible);
    let nodes = control_children(&ui, &find_input);
    assert_eq!(nodes, vec![find_input.case_sensitive().unwrap().dom_node(), find_input.whole_words().unwrap().dom_node(), find_input.regex().unwrap().dom_node()]);
    assert_eq!(find_input.toggle_dom_nodes(), nodes);
    let tree = ui.tree();
    assert!(tree.has_class(nodes[0], "codicon-case-sensitive"));
    assert!(tree.has_class(nodes[1], "codicon-whole-word"));
    assert!(tree.has_class(nodes[2], "codicon-regex"));
    assert_eq!(tree.label(nodes[0]), Some("Match Case"));
    assert_eq!(tree.label(nodes[1]), Some("Match Whole Word"));
    assert_eq!(tree.label(nodes[2]), Some("Use Regular Expression (Alt+R)"));
    assert_eq!(tree.label(find_input.input_box.input_element()), Some("Find"));
    drop(tree);
    find_input.dispose();
}

#[test]
fn the_controls_sit_at_the_top_right_and_reserve_three_toggle_widths() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    let (ui, _parent, find_input) = setup(options(true));
    let tree = ui.tree();
    // `top: 3px; right: 2px`, 3 botões de 22px (2px de margem e 20px de caixa), 20px de altura.
    assert_eq!(tree.bounds(find_input.controls()), Rect::new(300.0 - 2.0 - 66.0, 3.0, 300.0 - 2.0, 23.0));
    let nodes = control_children(&ui, &find_input);
    let xs: Vec<f64> = nodes.iter().map(|n| tree.bounds(*n).x0).collect();
    assert_eq!(xs, vec![2.0, 24.0, 46.0]);
    assert!(nodes.iter().all(|n| tree.bounds(*n).width() == 20.0 && tree.bounds(*n).y0 == 0.0));
    // O `domNode` preenche o pai e tem a altura do campo.
    assert_eq!(tree.bounds(find_input.dom_node()).width(), 300.0);
    assert_eq!(tree.bounds(find_input.dom_node()).height(), find_input.input_box.height());
    drop(tree);
    find_input.dispose();
}

#[test]
fn clicking_a_toggle_checks_it_fires_the_option_change_and_focuses_the_input() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let (ui, _parent, find_input) = setup(options(true));
    let changes = Rc::new(RefCell::new(Vec::new()));
    let sink = changes.clone();
    store.add(find_input.on_did_option_change().listen(move |via_keyboard| sink.borrow_mut().push(*via_keyboard)));

    click(&ui, find_input.case_sensitive().unwrap().dom_node());
    assert!(find_input.get_case_sensitive());
    assert_eq!(*changes.borrow(), vec![false]);
    assert_eq!(ui.focused(), Some(find_input.input_box.input_element()));

    find_input.set_focus_input_on_option_click(false);
    find_input.focus_on_regex();
    click(&ui, find_input.whole_words().unwrap().dom_node());
    assert!(find_input.get_whole_words());
    assert_eq!(ui.focused(), Some(find_input.regex().unwrap().dom_node()));
    find_input.dispose();
}

#[test]
fn toggling_by_keyboard_reports_via_keyboard_and_keeps_the_focus_on_the_toggle() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let (ui, _parent, find_input) = setup(options(true));
    let changes = Rc::new(RefCell::new(Vec::new()));
    let sink = changes.clone();
    store.add(find_input.on_did_option_change().listen(move |via_keyboard| sink.borrow_mut().push(*via_keyboard)));
    find_input.focus_on_regex();
    let regex = find_input.regex().unwrap().dom_node();
    key(&ui, regex, KeyCode::Space);
    assert!(find_input.get_regex());
    assert_eq!(*changes.borrow(), vec![true]);
    assert_eq!(ui.focused(), Some(regex));
    find_input.dispose();
}

#[test]
fn a_common_toggle_change_validates_the_input_but_setting_the_case_does_not() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    let validations = Rc::new(Cell::new(0));
    let counter = validations.clone();
    let validation: je_widgets::input_box::InputValidator = Rc::new(move |_| {
        counter.set(counter.get() + 1);
        None
    });
    let (ui, _parent, find_input) = setup(FindInputOptions { validation: Some(validation), ..options(true) });
    find_input.set_case_sensitive(true);
    assert_eq!(validations.get(), 0);
    find_input.set_regex(true);
    assert_eq!(validations.get(), 1);
    click(&ui, find_input.whole_words().unwrap().dom_node());
    assert_eq!(validations.get(), 2);
    find_input.dispose();
}

#[test]
fn additional_toggles_follow_the_common_ones_do_not_validate_and_are_replaced() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let validations = Rc::new(Cell::new(0));
    let counter = validations.clone();
    let validation: je_widgets::input_box::InputValidator = Rc::new(move |_| {
        counter.set(counter.get() + 1);
        None
    });
    let (ui, _parent, find_input) = setup(FindInputOptions { validation: Some(validation), ..options(false) });
    // Sem os botões comuns, os controles ficam escondidos até haver um adicional.
    assert!(!ui.tree().style(find_input.controls()).visible);
    let extra = regex_toggle(&ui, FindInputToggleOpts::new(unthemed_toggle_styles()));
    find_input.set_additional_toggles(Some(vec![extra.clone()]));
    assert!(ui.tree().style(find_input.controls()).visible);
    assert_eq!(control_children(&ui, &find_input), vec![extra.dom_node()]);
    assert_eq!(find_input.toggles_width(true), 22.0);

    let changes = Rc::new(Cell::new(0));
    let counter = changes.clone();
    store.add(find_input.on_did_option_change().listen(move |_| counter.set(counter.get() + 1)));
    click(&ui, extra.dom_node());
    assert!(extra.checked());
    assert_eq!((changes.get(), validations.get()), (1, 0));

    // Trocar descarta (e remove da árvore) o antigo.
    let other = regex_toggle(&ui, FindInputToggleOpts::new(unthemed_toggle_styles()));
    find_input.set_additional_toggles(Some(vec![other.clone()]));
    assert!(!ui.tree().is_alive(extra.dom_node()));
    assert_eq!(control_children(&ui, &find_input), vec![other.dom_node()]);
    find_input.dispose();
}

#[test]
fn disabling_disables_the_input_and_every_toggle_and_paints_the_disabled_background() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    let (ui, _parent, find_input) = setup(options(true));
    find_input.set_enabled(false);
    assert!(ui.tree().has_class(find_input.dom_node(), "disabled"));
    assert_eq!(ui.tree().style(find_input.dom_node()).background, Some(DISABLED));
    assert!(!find_input.input_box.is_enabled());
    assert!(control_children(&ui, &find_input).iter().all(|n| ui.tree().has_class(*n, "disabled")));
    find_input.set_enabled(true);
    assert!(!ui.tree().has_class(find_input.dom_node(), "disabled"));
    assert_eq!(ui.tree().style(find_input.dom_node()).background, None);
    assert!(find_input.input_box.is_enabled());
    assert!(control_children(&ui, &find_input).iter().all(|n| !ui.tree().has_class(*n, "disabled")));
    find_input.dispose();
}

#[test]
fn value_input_and_history() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let (ui, _parent, find_input) = setup(options(true));
    let inputs = Rc::new(RefCell::new(Vec::new()));
    let sink = inputs.clone();
    store.add(find_input.on_input().listen(move |e| sink.borrow_mut().push(e.from_composition_end)));
    let changes = Rc::new(RefCell::new(Vec::new()));
    let sink = changes.clone();
    store.add(find_input.on_did_change().listen(move |v| sink.borrow_mut().push(v.clone())));

    find_input.focus();
    ui.text_input("abc");
    assert_eq!(find_input.get_value(), "abc");
    assert_eq!(*inputs.borrow(), vec![false]);
    assert_eq!(*changes.borrow(), vec!["abc".to_owned()]);

    find_input.set_value("abc");
    assert_eq!(changes.borrow().len(), 1, "o mesmo valor não muda nada");
    find_input.set_value("xyz");
    assert_eq!(find_input.get_value(), "xyz");
    assert_eq!(inputs.borrow().len(), 1, "setValue não é digitação");

    find_input.on_search_submit();
    assert_eq!(find_input.input_box.history(), vec!["xyz".to_owned()]);
    find_input.clear();
    assert_eq!(find_input.get_value(), "");
    assert_eq!(ui.focused(), Some(find_input.input_box.input_element()));
    find_input.dispose();
}

#[test]
fn key_and_mouse_events_of_the_input_are_forwarded() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let (ui, _parent, find_input) = setup(options(true));
    let downs = Rc::new(Cell::new(0));
    let ups = Rc::new(Cell::new(0));
    let mouse = Rc::new(Cell::new(0));
    let counter = downs.clone();
    store.add(find_input.on_key_down().listen(move |_| counter.set(counter.get() + 1)));
    let counter = ups.clone();
    store.add(find_input.on_key_up().listen(move |_| counter.set(counter.get() + 1)));
    let counter = mouse.clone();
    store.add(find_input.on_mouse_down().listen(move |_| counter.set(counter.get() + 1)));
    find_input.focus();
    ui.key_input(true, KeyData { key_code: KeyCode::KeyA, modifiers: Modifiers::default() });
    ui.key_input(false, KeyData { key_code: KeyCode::KeyA, modifiers: Modifiers::default() });
    let input = find_input.input_box.input_element();
    ui.dispatch(&mut Event::new(EventKind::PointerDown, input).with_pointer(je_ui::PointerData { x: 10.0, y: 10.0, button: je_ui::PointerButton::Primary, modifiers: Modifiers::default(), click_count: 1 }));
    assert_eq!((downs.get(), ups.get(), mouse.get()), (1, 1, 1));
    find_input.dispose();
}

#[test]
fn highlight_find_options_alternates_the_highlight_classes() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    let (ui, _parent, find_input) = setup(options(true));
    let dom = find_input.dom_node();
    find_input.highlight_find_options();
    assert!(ui.tree().has_class(dom, "highlight-1") && !ui.tree().has_class(dom, "highlight-0"));
    find_input.highlight_find_options();
    assert!(ui.tree().has_class(dom, "highlight-0") && !ui.tree().has_class(dom, "highlight-1"));
    find_input.dispose();
}

#[test]
fn arrow_keys_navigate_between_the_toggles_and_escape_goes_back_to_the_input() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    let (ui, _parent, find_input) = setup(options(true));
    let nodes = find_input.toggle_dom_nodes();
    find_input.focus_on_case_sensitive();
    assert_eq!(ui.focused(), Some(nodes[0]));
    key(&ui, nodes[0], KeyCode::RightArrow);
    assert_eq!(ui.focused(), Some(nodes[1]));
    key(&ui, nodes[1], KeyCode::RightArrow);
    key(&ui, nodes[2], KeyCode::RightArrow);
    assert_eq!(ui.focused(), Some(nodes[0]), "dá a volta");
    key(&ui, nodes[0], KeyCode::LeftArrow);
    assert_eq!(ui.focused(), Some(nodes[2]));
    key(&ui, nodes[2], KeyCode::Escape);
    assert_eq!(ui.focused(), Some(find_input.input_box.input_element()));
    find_input.dispose();
}

#[test]
fn the_case_sensitive_and_regex_key_down_events_carry_the_keys_the_toggle_does_not_handle() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let (ui, _parent, find_input) = setup(options(true));
    let case_keys = Rc::new(RefCell::new(Vec::new()));
    let regex_keys = Rc::new(RefCell::new(Vec::new()));
    let sink = case_keys.clone();
    store.add(find_input.on_case_sensitive_key_down().listen(move |e| sink.borrow_mut().push(e.key_code)));
    let sink = regex_keys.clone();
    store.add(find_input.on_regex_key_down().listen(move |e| sink.borrow_mut().push(e.key_code)));
    let nodes = find_input.toggle_dom_nodes();
    key(&ui, nodes[0], KeyCode::Tab);
    key(&ui, nodes[0], KeyCode::Space);
    key(&ui, nodes[2], KeyCode::Tab);
    assert_eq!(*case_keys.borrow(), vec![KeyCode::Tab]);
    assert_eq!(*regex_keys.borrow(), vec![KeyCode::Tab]);
    find_input.dispose();
}

#[test]
fn messages_set_the_state_class_and_clear_message_restores_idle() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    let (ui, _parent, find_input) = setup(options(true));
    let element = find_input.input_box.element();
    find_input.show_message(Message { content: Some("bad".to_owned()), format_content: false, r#type: Some(MessageType::Error) });
    assert!(ui.tree().has_class(element, "error"));
    find_input.clear_message();
    assert!(ui.tree().has_class(element, "idle") && !ui.tree().has_class(element, "error"));
    find_input.dispose();
}

#[test]
fn layout_with_a_collapsed_find_widget_reserves_no_width_for_the_toggles() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    let (ui, parent, find_input) = setup(options(true));
    find_input.input_box.set_value("some text that fills the field and goes under the toggles of the box");
    let text_clip = |find_input: &FindInput, ui: &Ui| ui.tree().bounds(ui.tree().children(find_input.input_box.input_element())[0]).width();
    let reserved = text_clip(&find_input, &ui);
    find_input.layout(FindInputLayoutStyle { collapsed_find_widget: true, ..Default::default() });
    assert_eq!(text_clip(&find_input, &ui), reserved + 66.0);
    find_input.layout(FindInputLayoutStyle::default());
    assert_eq!(text_clip(&find_input, &ui), reserved);
    // Quem redimensiona o contêiner chama `layout`.
    ui.tree_mut().set_bounds(parent, Rect::new(0.0, 0.0, 200.0, 100.0));
    find_input.layout(FindInputLayoutStyle::default());
    assert_eq!(text_clip(&find_input, &ui), reserved - 100.0);
    assert_eq!(ui.tree().bounds(find_input.controls()).x1, 198.0);
    find_input.dispose();
}
