//! Testes extras (sem equivalente no VS Code) do comportamento portado de `toggle.ts` e `switch.ts`:
//! `Toggle`, `Checkbox`, `TriStateCheckbox`, `Switch` e os estilos padrão.

mod support;

use std::cell::RefCell;
use std::rc::Rc;

use je_base::key_codes::KeyCode;
use je_render::Rgba;
use je_ui::{Content, Event, EventKind, KeyData, Modifiers, NodeId, PointerEvents, Ui};
use je_widgets::switch::{Switch, SwitchOptions, SwitchStyles, default_switch_styles};
use je_widgets::toggle::{CheckState, Checkbox, CheckboxStyles, Toggle, ToggleOpts, ToggleStyles, TriStateCheckbox, default_checkbox_styles, default_toggle_styles, unthemed_toggle_styles};
use support::{ensure_no_disposables_are_leaked_in_test_suite, load_theme};

fn click(ui: &Ui, node: NodeId) {
    ui.dispatch(&mut Event::new(EventKind::Click, node));
}

fn key(ui: &Ui, node: NodeId, code: KeyCode) {
    ui.dispatch(&mut Event::new(EventKind::KeyDown, node).with_key(KeyData { key_code: code, modifiers: Modifiers::default() }));
}

fn changes<T: Clone + 'static>(event: je_base::event::Event<T>, store: &support::LeakGuard) -> Rc<RefCell<Vec<T>>> {
    let log = Rc::new(RefCell::new(Vec::new()));
    let sink = log.clone();
    store.add(event.listen(move |v| sink.borrow_mut().push(v.clone())));
    log
}

#[test]
fn default_styles_resolve_every_key_in_both_themes() {
    for name in ["dark", "light"] {
        let theme = load_theme(name);
        let toggle = default_toggle_styles(&theme);
        assert!(toggle.input_active_option_border.is_some() && toggle.input_active_option_foreground.is_some() && toggle.input_active_option_background.is_some(), "{name}");
        assert!(toggle.input_option_hover_background.is_some() && toggle.focus_border.is_some() && toggle.inherited_foreground.is_some(), "{name}");
        let checkbox = default_checkbox_styles(&theme);
        assert!(checkbox.checkbox_background.is_some() && checkbox.checkbox_border.is_some() && checkbox.checkbox_foreground.is_some(), "{name}");
        assert!(checkbox.checkbox_disabled_background.is_some() && checkbox.checkbox_disabled_foreground.is_some(), "{name}");
        let switch = default_switch_styles(&theme);
        assert!(switch.description_foreground.is_some() && switch.button_background.is_some() && switch.button_foreground.is_some() && switch.focus_border.is_some(), "{name}");
    }
}

#[test]
fn toggle_click_and_keys_flip_it_and_report_the_source() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = Ui::new();
    let toggle = store.add_rc(Toggle::new(&ui, ToggleOpts::new("Match Case", false, unthemed_toggle_styles())));
    let log = changes(toggle.on_change(), &store);
    click(&ui, toggle.dom_node());
    assert!(toggle.checked() && ui.tree().has_class(toggle.dom_node(), "checked"));
    key(&ui, toggle.dom_node(), KeyCode::Space);
    assert!(!toggle.checked());
    key(&ui, toggle.dom_node(), KeyCode::Enter);
    assert_eq!(*log.borrow(), [false, true, true]);
    assert_eq!(toggle.attribute("aria-checked").as_deref(), Some("true"));
    assert_eq!(toggle.attribute("aria-label").as_deref(), Some("Match Case"));
    assert_eq!(toggle.width(), 22.0);
}

#[test]
fn toggle_forwards_other_keys_and_ignores_input_when_disabled() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = Ui::new();
    let toggle = store.add_rc(Toggle::new(&ui, ToggleOpts::new("t", false, unthemed_toggle_styles())));
    let keys = changes(toggle.on_key_down(), &store);
    key(&ui, toggle.dom_node(), KeyCode::RightArrow);
    assert_eq!(keys.borrow().len(), 1);
    assert_eq!(keys.borrow()[0].key_code, KeyCode::RightArrow);
    toggle.disable();
    click(&ui, toggle.dom_node());
    key(&ui, toggle.dom_node(), KeyCode::Space);
    assert!(!toggle.checked());
    assert_eq!(ui.tree().style(toggle.dom_node()).pointer_events, PointerEvents::None);
    assert_eq!(ui.tree().style(toggle.dom_node()).opacity, 0.4);
    assert_eq!(keys.borrow().len(), 1);
}

#[test]
fn toggle_checked_style_uses_the_active_option_colors() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = Ui::new();
    let styles = ToggleStyles {
        input_active_option_border: Some(Rgba::opaque(1, 2, 3)),
        input_active_option_background: Some(Rgba::opaque(4, 5, 6)),
        input_active_option_foreground: Some(Rgba::opaque(7, 8, 9)),
        input_option_hover_background: Some(Rgba::opaque(10, 11, 12)),
        ..ToggleStyles::default()
    };
    let toggle = store.add_rc(Toggle::new(&ui, ToggleOpts { icon: Some(je_base::themables::ThemeIcon::from_id("regex")), ..ToggleOpts::new("Regex", false, styles) }));
    let style = |ui: &Ui| ui.tree().style(toggle.dom_node()).clone();
    assert_eq!((style(&ui).background, style(&ui).border), (None, None));
    assert!(matches!(style(&ui).content, Content::Codicon { ref id, .. } if id == "regex"));
    ui.dispatch(&mut Event::new(EventKind::PointerEnter, toggle.dom_node()));
    assert_eq!(style(&ui).background, Some(Rgba::opaque(10, 11, 12)), ":hover");
    toggle.set_checked(true);
    let checked = style(&ui);
    assert_eq!(checked.background, Some(Rgba::opaque(4, 5, 6)));
    assert_eq!(checked.border.map(|b| (b.width, b.color)), Some((1.0, Rgba::opaque(1, 2, 3))));
    assert!(matches!(checked.content, Content::Codicon { color, .. } if color == Rgba::opaque(7, 8, 9)));
    assert_eq!(ui.tree().bounds(toggle.dom_node()).width(), 20.0);
    toggle.set_visible(false);
    assert!(!toggle.visible());
}

fn checkbox_styles() -> CheckboxStyles {
    CheckboxStyles {
        checkbox_background: Some(Rgba::opaque(1, 1, 1)),
        checkbox_border: Some(Rgba::opaque(2, 2, 2)),
        checkbox_foreground: Some(Rgba::opaque(3, 3, 3)),
        checkbox_disabled_background: Some(Rgba::opaque(4, 4, 4)),
        checkbox_disabled_foreground: Some(Rgba::opaque(5, 5, 5)),
        ..CheckboxStyles::default()
    }
}

#[test]
fn checkbox_shows_the_check_only_when_checked_and_follows_the_enabled_colors() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = Ui::new();
    let checkbox = store.add_rc(Checkbox::new(&ui, "Option", false, checkbox_styles()));
    let log = changes(checkbox.on_change(), &store);
    let node = checkbox.dom_node();
    assert!(ui.tree().has_class(node, "monaco-checkbox") && ui.tree().has_class(node, "monaco-custom-toggle"));
    assert_eq!(ui.tree().style(node).content, Content::None);
    assert_eq!((ui.tree().bounds(node).width(), ui.tree().bounds(node).height()), (18.0, 18.0));
    assert_eq!(ui.tree().style(node).background, Some(Rgba::opaque(1, 1, 1)));
    assert_eq!(ui.tree().style(node).border.map(|b| b.color), Some(Rgba::opaque(2, 2, 2)));
    click(&ui, node);
    assert!(checkbox.checked());
    assert!(matches!(ui.tree().style(node).content, Content::Codicon { ref id, size, color } if id == "check" && size == 16.0 && color == Rgba::opaque(3, 3, 3)));
    checkbox.disable();
    assert!(!checkbox.enabled());
    // A borda desabilitada usa o fundo desabilitado (como o `applyStyles` do original).
    assert_eq!(ui.tree().style(node).border.map(|b| b.color), Some(Rgba::opaque(4, 4, 4)));
    assert!(matches!(ui.tree().style(node).content, Content::Codicon { color, .. } if color == Rgba::opaque(5, 5, 5)));
    checkbox.enable();
    checkbox.set_checked(false);
    assert_eq!(ui.tree().style(node).content, Content::None);
    assert_eq!(*log.borrow(), [false], "set checked não dispara onChange");
    key(&ui, node, KeyCode::Space);
    assert_eq!(*log.borrow(), [false, true]);
}

#[test]
fn checkbox_size_comes_from_the_styles() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = Ui::new();
    let checkbox = store.add_rc(Checkbox::new(&ui, "Option", true, CheckboxStyles { size: Some(24.0), ..checkbox_styles() }));
    let r = ui.tree().bounds(checkbox.dom_node());
    assert_eq!((r.width(), r.height()), (24.0, 24.0));
    assert!(matches!(ui.tree().style(checkbox.dom_node()).content, Content::Codicon { size, .. } if size == 22.0));
}

#[test]
fn tri_state_checkbox_shows_dash_for_mixed_and_toggles_to_checked() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = Ui::new();
    let checkbox = store.add_rc(TriStateCheckbox::new(&ui, "All", CheckState::Mixed, checkbox_styles()));
    let node = checkbox.dom_node();
    assert!(matches!(ui.tree().style(node).content, Content::Codicon { ref id, .. } if id == "dash"));
    assert_eq!(checkbox.checked(), CheckState::Mixed);
    click(&ui, node);
    // O toggle de baixo estava desmarcado (`mixed` não é `true`), então o clique marca.
    assert_eq!(checkbox.checked(), CheckState::Checked);
    assert!(matches!(ui.tree().style(node).content, Content::Codicon { ref id, .. } if id == "check"));
    checkbox.set_checked(CheckState::Unchecked);
    assert_eq!(ui.tree().style(node).content, Content::None);
    checkbox.set_checked(CheckState::Mixed);
    assert!(matches!(ui.tree().style(node).content, Content::Codicon { ref id, .. } if id == "dash"));
}

fn switch_options(checked: bool) -> SwitchOptions {
    SwitchOptions { checked, aria_label: "Setting".to_owned(), title: None, disabled: false }
}

#[test]
fn switch_flips_on_click_and_keys_and_only_the_user_fires_on_change() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = Ui::new();
    let switch = store.add_rc(Switch::new(&ui, switch_options(false), SwitchStyles::default()));
    let log = changes(switch.on_change(), &store);
    click(&ui, switch.dom_node());
    key(&ui, switch.dom_node(), KeyCode::Space);
    key(&ui, switch.dom_node(), KeyCode::Enter);
    switch.set_checked(false);
    assert_eq!(*log.borrow(), [true, false, true]);
    assert!(!switch.checked() && !ui.tree().has_class(switch.dom_node(), "checked"));
    assert_eq!(switch.attribute("aria-checked").as_deref(), Some("false"));
    assert_eq!(switch.attribute("aria-label").as_deref(), Some("Setting"));
    switch.set_checked(true);
    assert!(ui.tree().has_class(switch.dom_node(), "checked"));
}

#[test]
fn switch_geometry_and_disabled_state_follow_the_css() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = Ui::new();
    let styles = SwitchStyles {
        description_foreground: Some(Rgba::opaque(100, 100, 100)),
        button_background: Some(Rgba::opaque(0, 0, 200)),
        button_foreground: Some(Rgba::opaque(255, 255, 255)),
        focus_border: None,
    };
    let switch = store.add_rc(Switch::new(&ui, switch_options(false), styles));
    switch.set_position(10.0, 20.0);
    let tree = ui.tree();
    let r = tree.bounds(switch.dom_node());
    assert_eq!((r.x0, r.y0, r.width(), r.height()), (10.0, 20.0, 28.0, 16.0));
    // `color-mix(descriptionForeground 36%, transparent)`.
    assert_eq!(tree.style(switch.dom_node()).background, Some(Rgba { a: 92, ..Rgba::opaque(100, 100, 100) }));
    let thumb = tree.children(switch.dom_node())[0];
    // Desmarcado: o disco de 12px a 2px do canto; marcado: o mesmo, 12px mais à direita.
    assert_eq!((tree.bounds(thumb).x0, tree.bounds(thumb).width()), (2.0, 12.0));
    drop(tree);
    switch.set_checked(true);
    let tree = ui.tree();
    assert_eq!(tree.style(switch.dom_node()).background, Some(Rgba::opaque(0, 0, 200)));
    assert_eq!(tree.bounds(thumb).x0, 14.0);
    assert_eq!(tree.style(switch.dom_node()).border.map(|b| b.color), Some(Rgba::opaque(0, 0, 200)));
    drop(tree);
    switch.set_disabled(true);
    assert_eq!((switch.tab_index(), ui.tree().style(switch.dom_node()).opacity), (-1, 0.5));
    click(&ui, switch.dom_node());
    assert!(switch.checked(), "desabilitado não vira");
}
