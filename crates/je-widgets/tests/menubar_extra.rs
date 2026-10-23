//! Testes extras da `MenuBar` (`src/vs/base/browser/ui/menu/menubar.ts`): abrir, navegar e fechar os menus
//! (mouse, teclado, `Alt` e mnemônicos), os estados de foco e de visibilidade, o estouro (`...`) e o modo
//! compacto. Não fazem parte do porte caso a caso (que está em `tests/menubar.rs`).

mod support;

use std::cell::RefCell;
use std::rc::Rc;

use je_base::actions::{Action, ActionRef};
use je_base::key_codes::KeyCode;
use je_base::lifecycle::IDisposable;
use je_render::kurbo::Rect;
use je_ui::{Event, EventKind, KeyData, Modifiers, NodeId, PointerInput, PointerInputKind, Role, Ui};
use je_widgets::menu::{HorizontalDirection, IMenuDirection, VerticalDirection, unthemed_menu_styles};
use je_widgets::menu_bar::{IMenuBarOptions, MenuBar, MenuBarMenu};
use je_widgets::modifier_key_emitter::ModifierKeyEmitter;
use support::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite};

fn setup(width: f64) -> (Ui, NodeId) {
    let ui = Ui::new();
    let root = ui.root();
    let mut tree = ui.tree_mut();
    tree.set_bounds(root, Rect::new(0.0, 0.0, 1024.0, 768.0));
    let container = tree.create(root, Role::Generic);
    tree.add_class(container, "menubar");
    tree.set_bounds(container, Rect::new(0.0, 0.0, width, 35.0));
    drop(tree);
    (ui, container)
}

fn menus(store: &LeakGuard, labels: &[&str]) -> Vec<MenuBarMenu> {
    labels
        .iter()
        .map(|label| MenuBarMenu {
            label: (*label).to_owned(),
            actions: vec![store.add_rc(Action::new(&format!("{label}.one"), "One", "", true, None)) as ActionRef, store.add_rc(Action::new(&format!("{label}.two"), "Two", "", true, None)) as ActionRef],
        })
        .collect()
}

fn bar(store: &LeakGuard, ui: &Ui, container: NodeId, options: IMenuBarOptions, labels: &[&str]) -> Rc<MenuBar> {
    ModifierKeyEmitter::get_instance(ui).reset_key_status();
    let bar = MenuBar::new(ui, container, options, unthemed_menu_styles());
    bar.push(&menus(store, labels));
    bar.update(None);
    ui.run_animation_frame();
    bar
}

fn mnemonic_options() -> IMenuBarOptions {
    IMenuBarOptions { enable_mnemonics: true, visibility: Some("classic".to_owned()), ..Default::default() }
}

fn buttons(ui: &Ui, container: NodeId) -> Vec<NodeId> {
    ui.tree().children(container).to_vec()
}

fn center(ui: &Ui, node: NodeId) -> (f64, f64) {
    let r = ui.tree().absolute_bounds(node);
    ((r.x0 + r.x1) / 2.0, (r.y0 + r.y1) / 2.0)
}

fn click(ui: &Ui, node: NodeId) {
    let (x, y) = center(ui, node);
    ui.pointer_input(PointerInput::new(PointerInputKind::Move, x, y));
    ui.pointer_input(PointerInput::new(PointerInputKind::Down, x, y));
    ui.pointer_input(PointerInput::new(PointerInputKind::Up, x, y));
}

fn key(ui: &Ui, kind: EventKind, target: NodeId, key_code: KeyCode, modifiers: Modifiers) {
    ui.dispatch(&mut Event::new(kind, target).with_key(KeyData { key_code, modifiers }));
}

fn press(ui: &Ui, key_code: KeyCode) {
    let target = ui.focused().unwrap_or_else(|| ui.root());
    key(ui, EventKind::KeyDown, target, key_code, Modifiers::default());
    key(ui, EventKind::KeyUp, target, key_code, Modifiers::default());
}

fn holders(ui: &Ui, container: NodeId) -> Vec<NodeId> {
    let tree = ui.tree();
    tree.subtree(container).into_iter().filter(|n| tree.has_class(*n, "menubar-menu-items-holder")).collect()
}

fn open_button(ui: &Ui, container: NodeId) -> Option<usize> {
    let tree = ui.tree();
    tree.children(container).iter().position(|b| tree.has_class(*b, "open"))
}

#[test]
fn buttons_are_laid_out_in_a_row_with_the_title_padding_and_the_overflow_button_last() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let (ui, container) = setup(600.0);
    let bar = store.add_rc(bar(&store, &ui, container, mnemonic_options(), &["&File", "&Edit"]));
    let buttons = buttons(&ui, container);
    assert_eq!(buttons.len(), 3, "dois menus e o botão de estouro");
    let tree = ui.tree();
    let (file, edit) = (tree.bounds(buttons[0]), tree.bounds(buttons[1]));
    assert_eq!(file.x0, 0.0);
    assert_eq!(edit.x0, file.x1, "um depois do outro");
    assert!(file.width() > 16.0, "o texto mais o padding de 8px de cada lado");
    assert_eq!(file.height(), 35.0);
    assert_eq!(tree.role(buttons[0]), Role::MenuItem);
    assert_eq!(tree.role(container), Role::MenuBar);
    assert_eq!(tree.label(buttons[2]), Some("More"));
    assert!(!tree.style(buttons[2]).visible, "tudo cabe: o botão de estouro fica escondido");
    drop(tree);
    assert!(bar.get_width() > 0.0 && bar.get_height() == 35.0);
}

#[test]
fn clicking_a_button_opens_its_menu_and_clicking_again_closes_it() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let (ui, container) = setup(600.0);
    let bar = store.add_rc(bar(&store, &ui, container, mnemonic_options(), &["&File", "&Edit"]));
    let states = Rc::new(RefCell::new(Vec::new()));
    let s = states.clone();
    store.add(bar.on_focus_state_change().listen(move |focused| s.borrow_mut().push(*focused)));
    let buttons = buttons(&ui, container);

    click(&ui, buttons[0]);
    assert_eq!(open_button(&ui, container), Some(0));
    assert_eq!(holders(&ui, container).len(), 1);
    assert_eq!(*states.borrow(), [true]);

    // O menu abre embaixo do título.
    let holder = holders(&ui, container)[0];
    let (title, holder_box) = {
        let tree = ui.tree();
        (tree.absolute_bounds(tree.children(buttons[0])[0]), tree.absolute_bounds(holder))
    };
    assert_eq!((holder_box.x0, holder_box.y0), (title.x0, title.y1));

    click(&ui, buttons[0]);
    assert_eq!(open_button(&ui, container), None);
    assert!(holders(&ui, container).is_empty());
    assert_eq!(*states.borrow(), [true, false]);
}

#[test]
fn hovering_another_button_with_a_menu_open_switches_the_menu() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let (ui, container) = setup(600.0);
    let bar = store.add_rc(bar(&store, &ui, container, mnemonic_options(), &["&File", "&Edit"]));
    let buttons = buttons(&ui, container);
    click(&ui, buttons[0]);
    let (x, y) = center(&ui, buttons[1]);
    ui.pointer_input(PointerInput::new(PointerInputKind::Move, x, y));
    assert_eq!(open_button(&ui, container), Some(1));
    assert_eq!(holders(&ui, container).len(), 1);
    drop(bar);
}

#[test]
fn arrows_move_between_the_open_menus_and_wrap_around() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let (ui, container) = setup(600.0);
    let bar = store.add_rc(bar(&store, &ui, container, mnemonic_options(), &["&File", "&Edit", "&View"]));
    let buttons = buttons(&ui, container);
    click(&ui, buttons[0]);
    press(&ui, KeyCode::RightArrow);
    assert_eq!(open_button(&ui, container), Some(1));
    press(&ui, KeyCode::RightArrow);
    assert_eq!(open_button(&ui, container), Some(2));
    press(&ui, KeyCode::LeftArrow);
    assert_eq!(open_button(&ui, container), Some(1));
    press(&ui, KeyCode::LeftArrow);
    press(&ui, KeyCode::LeftArrow);
    // Antes do primeiro: com estouro seria o botão de estouro; aqui tudo cabe, então volta ao último menu.
    assert_eq!(open_button(&ui, container), Some(2));
    drop(bar);
}

#[test]
fn alt_plus_the_mnemonic_opens_the_menu() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let (ui, container) = setup(600.0);
    let bar = store.add_rc(bar(&store, &ui, container, mnemonic_options(), &["&File", "&Edit"]));
    key(&ui, EventKind::KeyDown, ui.root(), KeyCode::KeyE, Modifiers { alt: true, ..Modifiers::default() });
    assert_eq!(open_button(&ui, container), Some(1), "Alt+E abre o menu Edit");
    drop(bar);
}

#[test]
fn a_clean_alt_press_focuses_the_first_button_and_alt_shows_the_mnemonics() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let (ui, container) = setup(600.0);
    let bar = store.add_rc(bar(&store, &ui, container, mnemonic_options(), &["&File", "&Edit"]));
    let underlines = |ui: &Ui| {
        let tree = ui.tree();
        tree.subtree(container).into_iter().filter(|n| tree.has_class(*n, "underline")).count()
    };
    let states = Rc::new(RefCell::new(Vec::new()));
    let s = states.clone();
    store.add(bar.on_focus_state_change().listen(move |focused| s.borrow_mut().push(*focused)));
    let buttons = buttons(&ui, container);
    let alt = Modifiers { alt: true, ..Modifiers::default() };

    key(&ui, EventKind::KeyDown, ui.root(), KeyCode::Alt, alt);
    assert_eq!(underlines(&ui), 2, "Alt apertado mostra os mnemônicos");
    key(&ui, EventKind::KeyUp, ui.root(), KeyCode::Alt, Modifiers::default());
    assert_eq!(ui.focused(), Some(buttons[0]), "Alt apertado e solto foca o primeiro menu");
    assert_eq!(*states.borrow(), [true]);

    press(&ui, KeyCode::RightArrow);
    assert_eq!(ui.focused(), Some(buttons[1]));
    press(&ui, KeyCode::Escape);
    assert_eq!(*states.borrow(), [true, false]);
    assert_eq!(underlines(&ui), 0);
}

#[test]
fn escape_closes_the_open_menu_to_the_focused_bar_and_then_unfocuses_it() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let (ui, container) = setup(600.0);
    let bar = store.add_rc(bar(&store, &ui, container, mnemonic_options(), &["&File", "&Edit"]));
    let states = Rc::new(RefCell::new(Vec::new()));
    let s = states.clone();
    store.add(bar.on_focus_state_change().listen(move |focused| s.borrow_mut().push(*focused)));
    let buttons = buttons(&ui, container);
    click(&ui, buttons[0]);
    press(&ui, KeyCode::DownArrow);
    press(&ui, KeyCode::Escape);
    assert_eq!(open_button(&ui, container), None, "o Escape do menu fecha o menu");
    assert_eq!(ui.focused(), Some(buttons[0]), "e foca o botão");
    press(&ui, KeyCode::Escape);
    assert_eq!(*states.borrow(), [true, true, false]);
}

#[test]
fn menus_that_do_not_fit_move_to_the_overflow_menu() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let (ui, container) = setup(150.0);
    let bar = store.add_rc(bar(&store, &ui, container, mnemonic_options(), &["&File", "&Edit", "&Selection", "&View", "&Go", "&Run"]));
    let buttons = buttons(&ui, container);
    let visible: Vec<bool> = buttons.iter().map(|b| ui.tree().style(*b).visible).collect();
    // Os que cabem aparecem, os outros somem e o botão de estouro (que vem logo depois do último que cabe) aparece.
    let overflow = *buttons.iter().find(|b| ui.tree().label(**b) == Some("More")).expect("botão de estouro");
    assert!(ui.tree().style(overflow).visible, "{visible:?}");
    assert!(visible.iter().filter(|v| **v).count() < buttons.len(), "alguém ficou de fora");
    click(&ui, overflow);
    let holder = holders(&ui, container);
    assert_eq!(holder.len(), 1, "o estouro abre um menu com os que sobraram como submenus");
    drop(bar);
}

#[test]
fn compact_mode_has_a_single_application_menu_button() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let (ui, container) = setup(35.0);
    let options = IMenuBarOptions { compact_mode: Some(IMenuDirection { horizontal: HorizontalDirection::Right, vertical: VerticalDirection::Below }), enable_mnemonics: true, ..Default::default() };
    let bar = store.add_rc(bar(&store, &ui, container, options, &["&File", "&Edit"]));
    let buttons = buttons(&ui, container);
    assert_eq!(buttons.len(), 1);
    assert_eq!(ui.tree().label(buttons[0]), Some("Application Menu"));
    assert!(ui.tree().has_class(container, "compact"));
    assert!(ui.tree().style(buttons[0]).visible);
    click(&ui, buttons[0]);
    assert_eq!(holders(&ui, container).len(), 1);
    drop(bar);
}

#[test]
fn the_toggle_visibility_hides_the_bar_until_alt_and_reports_it() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let (ui, container) = setup(600.0);
    let options = IMenuBarOptions { enable_mnemonics: true, visibility: Some("toggle".to_owned()), ..Default::default() };
    let bar = store.add_rc(bar(&store, &ui, container, options, &["&File"]));
    let changes = Rc::new(RefCell::new(Vec::new()));
    let c = changes.clone();
    store.add(bar.on_visibility_change().listen(move |visible| c.borrow_mut().push(*visible)));
    assert!(!ui.tree().style(container).visible, "toggle começa escondida");
    let alt = Modifiers { alt: true, ..Modifiers::default() };
    key(&ui, EventKind::KeyDown, ui.root(), KeyCode::Alt, alt);
    key(&ui, EventKind::KeyUp, ui.root(), KeyCode::Alt, Modifiers::default());
    assert!(ui.tree().style(container).visible);
    assert!(bar.is_visible());
    bar.blur();
    assert!(!ui.tree().style(container).visible);
    assert_eq!(*changes.borrow(), [true, false]);
}

#[test]
fn toggle_focus_focuses_the_first_menu_and_a_hidden_bar_ignores_it() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let (ui, container) = setup(600.0);
    let bar = store.add_rc(bar(&store, &ui, container, mnemonic_options(), &["&File", "&Edit"]));
    let buttons = buttons(&ui, container);
    bar.toggle_focus();
    assert_eq!(ui.focused(), Some(buttons[0]));
    bar.toggle_focus();
    assert_ne!(ui.focused(), Some(buttons[0]), "o segundo toggle desfoca");

    let (ui2, container2) = setup(600.0);
    let hidden = store.add_rc(bar_hidden(&store, &ui2, container2));
    hidden.toggle_focus();
    assert_eq!(ui2.focused(), None);
}

fn bar_hidden(store: &LeakGuard, ui: &Ui, container: NodeId) -> Rc<MenuBar> {
    bar(store, ui, container, IMenuBarOptions { visibility: Some("hidden".to_owned()), ..Default::default() }, &["&File"])
}

#[test]
fn update_menu_replaces_the_actions_of_the_menu_with_that_label() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let (ui, container) = setup(600.0);
    let bar = store.add_rc(bar(&store, &ui, container, mnemonic_options(), &["&File"]));
    let action = store.add_rc(Action::new("only", "Only", "", true, None)) as ActionRef;
    bar.update_menu(&MenuBarMenu { label: "&File".to_owned(), actions: vec![action.clone()] });
    let buttons = buttons(&ui, container);
    click(&ui, buttons[0]);
    let holder = holders(&ui, container)[0];
    let tree = ui.tree();
    let items = tree.subtree(holder).into_iter().filter(|n| tree.has_class(*n, "action-item")).count();
    assert_eq!(items, 1, "o menu aberto tem só o item novo");
}

#[test]
fn dispose_removes_the_buttons() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let (ui, container) = setup(600.0);
    let bar = bar(&store, &ui, container, mnemonic_options(), &["&File", "&Edit"]);
    bar.dispose();
    assert!(ui.tree().children(container).is_empty());
}
