//! Testes extras (sem equivalente no VS Code) do comportamento portado de `button.ts`: `Button`,
//! `ButtonWithDescription`, `ButtonWithDropdown`, `ButtonBar` e os estilos padrão.

mod support;

use std::cell::RefCell;
use std::rc::Rc;

use je_base::actions::ActionRef;
use je_base::key_codes::KeyCode;
use je_base::lifecycle::IDisposable;
use je_base::themables::ThemeIcon;
use je_render::Rgba;
use je_ui::{Content, Event, EventKind, KeyData, Modifiers, NodeId, PointerData, PointerButton, Ui};
use je_widgets::button::{
    Button, ButtonBar, ButtonBarAlignment, ButtonDropdownActions, ButtonEvent, ButtonOptions, ButtonTitle, ButtonWithDescription, ButtonWithDropdown, ButtonWithDropdownOptions, ContextMenuDelegate, IButton,
    IButtonWithDescription, IContextMenuProvider, LabelPartKind, default_button_styles, unthemed_button_styles,
};
use support::{create_container, ensure_no_disposables_are_leaked_in_test_suite, load_theme};

fn click(ui: &Ui, node: NodeId) {
    let pointer = PointerData { x: 0.0, y: 0.0, button: PointerButton::Primary, modifiers: Modifiers::default(), click_count: 1 };
    ui.dispatch(&mut Event::new(EventKind::Click, node).with_pointer(pointer));
}

fn key(ui: &Ui, node: NodeId, code: KeyCode) {
    ui.dispatch(&mut Event::new(EventKind::KeyDown, node).with_key(KeyData { key_code: code, modifiers: Modifiers::default() }));
}

fn options() -> ButtonOptions {
    ButtonOptions { styles: unthemed_button_styles(), ..ButtonOptions::default() }
}

#[test]
fn default_styles_resolve_every_chave_in_both_themes() {
    for name in ["dark", "light"] {
        let theme = load_theme(name);
        let styles = default_button_styles(&theme);
        assert!(styles.button_background.is_some() && styles.button_foreground.is_some() && styles.button_hover_background.is_some(), "{name}");
        assert!(styles.button_separator.is_some() && styles.button_border.is_some() && styles.button_secondary_border.is_some() && styles.focus_border.is_some(), "{name}");
        assert!(styles.button_secondary_background.is_some() && styles.button_secondary_foreground.is_some() && styles.button_secondary_hover_background.is_some(), "{name}");
    }
}

#[test]
fn click_fires_when_enabled_and_is_swallowed_when_disabled() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = Ui::new();
    let container = create_container(&ui, 200.0, 100.0);
    let button = store.add_rc(Button::new(&ui, container, options()));
    let clicks = Rc::new(RefCell::new(0));
    let counter = clicks.clone();
    store.add(button.on_did_click().listen(move |e| {
        assert!(e.is_some_and(|e: ButtonEvent| e.pointer.is_some()));
        *counter.borrow_mut() += 1;
    }));
    click(&ui, button.element());
    button.set_enabled(false);
    let mut event = Event::new(EventKind::Click, button.element());
    ui.dispatch(&mut event);
    assert_eq!(*clicks.borrow(), 1);
    assert_eq!(button.attribute("aria-disabled").as_deref(), Some("true"));
    assert!(ui.tree().has_class(button.element(), "disabled"));
    assert_eq!(ui.tree().style(button.element()).opacity, 0.4);
}

#[test]
fn enter_and_space_click_and_escape_blurs() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = Ui::new();
    let container = create_container(&ui, 200.0, 100.0);
    let button = store.add_rc(Button::new(&ui, container, options()));
    let log = Rc::new(RefCell::new(Vec::new()));
    let sink = log.clone();
    store.add(button.on_did_click().listen(move |_| sink.borrow_mut().push("click")));
    let sink = log.clone();
    store.add(button.on_did_escape().listen(move |_| sink.borrow_mut().push("escape")));
    button.focus();
    assert!(button.has_focus());
    key(&ui, button.element(), KeyCode::Enter);
    key(&ui, button.element(), KeyCode::Space);
    key(&ui, button.element(), KeyCode::Escape);
    assert_eq!(*log.borrow(), ["click", "click", "escape"]);
    assert!(!button.has_focus());
}

#[test]
fn label_with_icons_builds_trimmed_text_and_icon_parts() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = Ui::new();
    let container = create_container(&ui, 300.0, 100.0);
    let button = store.add_rc(Button::new(&ui, container, ButtonOptions { support_icons: true, ..options() }));
    button.set_label("  $(add) Add \\$(not-icon) done$(sync~spin) ".into());
    let parts: Vec<_> = button.label_parts().into_iter().map(|p| (p.kind, p.text)).collect();
    assert_eq!(
        parts,
        [
            (LabelPartKind::Icon, "add".to_owned()),
            (LabelPartKind::Text, "Add".to_owned()),
            (LabelPartKind::Text, "$(not-icon)".to_owned()),
            (LabelPartKind::Text, "done".to_owned()),
            (LabelPartKind::Icon, "sync~spin".to_owned()),
        ]
    );
    let icon_node = button.label_parts()[4].node;
    assert!(ui.tree().has_class(icon_node, "codicon") && ui.tree().has_class(icon_node, "codicon-sync") && ui.tree().has_class(icon_node, "codicon-modifier-spin"));
    assert!(ui.tree().has_class(button.element(), "monaco-text-button"));
}

#[test]
fn text_button_metrics_follow_the_css() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = Ui::new();
    let container = create_container(&ui, 200.0, 100.0);
    let primary = store.add_rc(Button::new(&ui, container, options()));
    primary.set_label("Primary".into());
    let small = store.add_rc(Button::new(&ui, container, ButtonOptions { small: true, ..options() }));
    small.set_label("Small".into());
    // 4px + 4px de padding, 1px + 1px de borda, 16px de linha; o pequeno usa 3px, 3px e 14px.
    assert_eq!(primary.size(), (200.0, 26.0));
    assert_eq!(small.size(), (200.0, 22.0));
    assert!(ui.tree().has_class(small.element(), "small"));
    // A caixa de texto fica centrada na largura.
    let part = ui.tree().bounds(primary.label_parts()[0].node);
    assert!((part.x0 + part.x1 - 200.0).abs() < 0.01, "{part:?}");
    assert_eq!((part.y0, part.y1), (5.0, 21.0));
    assert_eq!(primary.min_content_width(), part.width() + 18.0);
}

#[test]
fn secondary_uses_the_secondary_colors_and_can_change() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = Ui::new();
    let container = create_container(&ui, 200.0, 100.0);
    let styles = je_widgets::button::ButtonStyles { button_secondary_background: Some(Rgba::opaque(1, 2, 3)), button_secondary_border: Some(Rgba::opaque(9, 9, 9)), ..unthemed_button_styles() };
    let button = store.add_rc(Button::new(&ui, container, ButtonOptions { styles, secondary: true, ..ButtonOptions::default() }));
    button.set_label("Cancel".into());
    let style = ui.tree().style(button.element()).clone();
    assert_eq!(style.background, Some(Rgba::opaque(1, 2, 3)));
    assert_eq!(style.border.map(|b| (b.width, b.color)), Some((1.0, Rgba::opaque(9, 9, 9))));
    button.set_secondary(false);
    assert_eq!(ui.tree().style(button.element()).background, unthemed_button_styles().button_background);
    assert!(!ui.tree().has_class(button.element(), "secondary"));
}

#[test]
fn checked_icon_title_and_aria_label() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = Ui::new();
    let container = create_container(&ui, 200.0, 100.0);
    let button = store.add_rc(Button::new(&ui, container, ButtonOptions { title: ButtonTitle::Text("Add a file".to_owned()), ..options() }));
    // O construtor só escreve o `aria-label` das opções; o título vira rótulo no `set label` e no `set icon`.
    assert_eq!(button.attribute("aria-label"), None);
    button.set_checked(true);
    assert!(button.checked() && ui.tree().has_class(button.element(), "checked"));
    assert_eq!(button.attribute("aria-pressed").as_deref(), Some("true"));
    button.set_icon(&ThemeIcon::from_id("add"));
    button.set_icon(&ThemeIcon::from_id("sync~spin"));
    assert_eq!(button.attribute("aria-label").as_deref(), Some("Add a file"));
    assert_eq!(ui.tree().label(button.element()), Some("Add a file"));
    let tree = ui.tree();
    assert!(!tree.has_class(button.element(), "codicon-add") && tree.has_class(button.element(), "codicon-sync") && tree.has_class(button.element(), "codicon-modifier-spin"));
    assert!(matches!(&tree.style(button.element()).content, Content::Codicon { id, .. } if id == "sync"));
}

#[test]
fn focus_draws_the_outline_and_blur_hides_it() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = Ui::new();
    let container = create_container(&ui, 200.0, 100.0);
    let focus = Rgba::opaque(0, 0x7A, 0xCC);
    let button = store.add_rc(Button::new(&ui, container, ButtonOptions { styles: je_widgets::button::ButtonStyles { focus_border: Some(focus), ..unthemed_button_styles() }, ..ButtonOptions::default() }));
    button.set_label("Focus".into());
    let outlines = |ui: &Ui| ui.tree().children_with_class(button.element(), "outline");
    assert!(outlines(&ui).is_empty());
    button.focus();
    let nodes = outlines(&ui);
    assert_eq!(nodes.len(), 1);
    {
        let tree = ui.tree();
        // `outline-offset: 2px` e `1px` de largura: três pixels para fora.
        assert_eq!((tree.bounds(nodes[0]).x0, tree.bounds(nodes[0]).y0), (-3.0, -3.0));
        assert_eq!(tree.style(nodes[0]).border.map(|b| b.color), Some(focus));
        assert!(tree.style(nodes[0]).visible);
    }
    // O `FocusTracker` adia a perda do foco para o próximo ciclo (`setTimeout(0)`), como o do original.
    je_base::runtime::with_faked_timers(|| {
        ui.blur();
        je_base::runtime::advance_fake_time(0);
    });
    assert!(!ui.tree().style(nodes[0]).visible);
}

#[test]
fn button_bar_moves_focus_with_arrows_and_wraps() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = Ui::new();
    let container = create_container(&ui, 300.0, 100.0);
    for (alignment, previous, next) in [(ButtonBarAlignment::Horizontal, KeyCode::LeftArrow, KeyCode::RightArrow), (ButtonBarAlignment::Vertical, KeyCode::UpArrow, KeyCode::DownArrow)] {
        let bar = ButtonBar::new(&ui, container, Some(alignment));
        let buttons: Vec<_> = (0..3).map(|_| bar.add_button(options())).collect();
        buttons[0].focus();
        key(&ui, buttons[0].element(), previous);
        assert!(buttons[2].has_focus(), "{alignment:?}: volta ao último");
        key(&ui, buttons[2].element(), next);
        assert!(buttons[0].has_focus(), "{alignment:?}: volta ao primeiro");
        key(&ui, buttons[0].element(), next);
        assert!(buttons[1].has_focus());
        assert_eq!(bar.buttons().len(), 3);
        bar.clear();
        assert!(bar.buttons().is_empty());
        bar.dispose();
    }
}

struct RecordingMenu(RefCell<Vec<ContextMenuDelegate>>);

impl IContextMenuProvider for RecordingMenu {
    fn show_context_menu(&self, delegate: ContextMenuDelegate) {
        self.0.borrow_mut().push(delegate);
    }
}

fn dropdown(ui: &Ui, container: NodeId, menu: &Rc<RecordingMenu>, add_primary: Option<bool>) -> Rc<ButtonWithDropdown> {
    ButtonWithDropdown::new(
        ui,
        container,
        ButtonWithDropdownOptions {
            button: options(),
            context_menu_provider: menu.clone(),
            actions: ButtonDropdownActions::Actions(Vec::new()),
            action_runner: None,
            add_primary_action_to_dropdown: add_primary,
            dropdown_layer: Some(3),
        },
    )
}

#[test]
fn dropdown_opens_the_menu_with_the_primary_action_first() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = Ui::new();
    let container = create_container(&ui, 200.0, 100.0);
    let menu = Rc::new(RecordingMenu(RefCell::new(Vec::new())));
    let button = store.add_rc(dropdown(&ui, container, &menu, None));
    button.set_label("Commit".into());
    let clicks = Rc::new(RefCell::new(Vec::new()));
    let sink = clicks.clone();
    store.add(button.on_did_click().listen(move |e| sink.borrow_mut().push(e.is_some())));
    click(&ui, button.primary_button.element());
    click(&ui, button.dropdown_button_element());
    assert_eq!(*clicks.borrow(), [true]);
    let delegate = menu.0.borrow()[0].clone();
    assert_eq!(delegate.layer, Some(3));
    let actions: Vec<ActionRef> = (delegate.get_actions)();
    assert_eq!(actions.len(), 1);
    assert_eq!((actions[0].id(), actions[0].label()), ("primaryAction".to_owned(), "Commit".to_owned()));
    assert_eq!(button.dropdown_button.attribute("aria-expanded").as_deref(), Some("true"));
    (delegate.on_hide)();
    assert_eq!(button.dropdown_button.attribute("aria-expanded").as_deref(), Some("false"));
    // O item principal do menu dispara o clique sem evento.
    let _ = actions[0].run(&[]);
    assert_eq!(*clicks.borrow(), [true, false]);
}

#[test]
fn dropdown_can_leave_the_primary_action_out_of_the_menu() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = Ui::new();
    let container = create_container(&ui, 200.0, 100.0);
    let menu = Rc::new(RecordingMenu(RefCell::new(Vec::new())));
    let button = store.add_rc(dropdown(&ui, container, &menu, Some(false)));
    click(&ui, button.dropdown_button_element());
    assert!((menu.0.borrow()[0].get_actions)().is_empty());
}

#[test]
fn dropdown_layout_shrinks_the_primary_button_around_the_separator_and_the_arrow() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = Ui::new();
    let container = create_container(&ui, 200.0, 100.0);
    let menu = Rc::new(RecordingMenu(RefCell::new(Vec::new())));
    let button = store.add_rc(dropdown(&ui, container, &menu, None));
    button.set_label("Commit".into());
    button.set_width(Some(160.0));
    let tree = ui.tree();
    let (primary, arrow) = (tree.bounds(button.primary_button.element()), tree.bounds(button.dropdown_button_element()));
    // 16px do ícone, 4px + 4px de padding e 1px de borda à direita (a da esquerda é zero): 25px.
    assert_eq!(arrow.width(), 25.0);
    assert_eq!(primary.width() + 1.0 + arrow.width(), 160.0);
    assert_eq!(primary.height(), 26.0);
    assert_eq!(arrow.height(), 26.0);
    assert_eq!(tree.bounds(button.element()).width(), 160.0);
    // O botão principal não tem borda à direita e o do menu não tem à esquerda.
    drop(tree);
    assert_eq!(button.primary_button.compute_box().border_widths, [1.0, 0.0, 1.0, 1.0]);
    assert_eq!(button.dropdown_button.compute_box().border_widths, [1.0, 1.0, 1.0, 0.0]);
    assert!(ui.tree().has_class(button.dropdown_button_element(), "monaco-dropdown-button"));
    button.set_enabled(false);
    assert!(ui.tree().has_class(button.element(), "disabled") && !button.enabled());
}

#[test]
fn description_button_stacks_the_description_under_the_button() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = Ui::new();
    let container = create_container(&ui, 300.0, 200.0);
    let button = store.add_rc(ButtonWithDescription::new(&ui, container, options()));
    button.set_label("Sign in".into());
    button.set_description("Uses your account");
    button.layout(Some((0.0, 0.0, 240.0)));
    assert_eq!(button.description_text(), "Uses your account");
    let tree = ui.tree();
    let (element, inner) = (tree.bounds(button.element()), tree.bounds(button.button().element()));
    // `margin: 4px 5px`: a caixa ocupa a largura menos as margens.
    assert_eq!((element.x0, element.y0, element.width()), (5.0, 4.0, 230.0));
    assert_eq!((inner.width(), inner.height()), (230.0, 26.0));
    let description = tree.bounds(button.description_element());
    assert_eq!(description.y0, 26.0);
    assert!((description.x0 + description.x1 - 230.0).abs() < 0.01, "descrição centrada");
    assert_eq!(element.height(), 26.0 + description.height());
    assert!(ui.tree().has_class(button.element(), "monaco-description-button"));
}

#[test]
fn dispose_removes_the_element() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = Ui::new();
    let container = create_container(&ui, 200.0, 100.0);
    let button = Button::new(&ui, container, options());
    let element = button.element();
    button.dispose();
    assert!(!ui.tree().is_alive(element));
}
