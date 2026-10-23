//! Porte de `src/vs/base/test/browser/ui/menu/menu.test.ts` (suíte `Menu`), caso a caso e na mesma ordem.
//!
//! Divergências (o `je-ui` não tem DOM, CSS computado nem relógio do navegador):
//! - `sinon.useFakeTimers()` e `clock.tickAsync(n)` são `runtime::with_faked_timers` e `advance_fake_time(n)`.
//! - `MouseEvent`/`KeyboardEvent` despachados no elemento são `Event` do `je-ui` despachados no nó (`PointerUp`,
//!   `PointerMove`, `KeyDown`, `KeyUp`); `movementX/Y` é a diferença da posição do ponteiro entre dois
//!   `PointerMove` (o primeiro evento nunca conta como movimento), e `e.key` é o `KeyCode` (`KeyA`, `KeyP`).
//! - `getAttribute` dos atributos ARIA que o `je-ui` não modela (`aria-keyshortcuts`, `aria-checked`,
//!   `aria-expanded`) é `MenuActionViewItem::attribute`; `aria-label` do rótulo é o rótulo do nó; `role` é
//!   `Tree::role`; `textContent` é o texto de todos os `Content::Text` da subárvore; `classList` do rótulo é o
//!   conjunto de classes conhecidas do teste, conferidas com `has_class` (o nó não lista as classes).
//! - `document.activeElement` é `Ui::focused`.
//! - Os oito casos `submenus open without animation ...` leem `animationName` computado (CSS de movimento e
//!   vidro fosco, que o `je-ui` não tem; `shadowDom` não existe, então os dois ramos montam a mesma árvore). O
//!   equivalente é a classe de movimento do menu de contexto (`context-view-menu-motion`) e a de fechamento: os
//!   submenus não podem ter nenhuma delas, nem o elemento rolável deles, e o contêiner raiz fica como estava.
//! - O caso `high contrast selection outline ...` (#327543) depende da cascata de CSS de `getMenuWidgetCSS` em
//!   shadow DOM (`outline-style` computado): `#[ignore]`. O comportamento equivalente (só o item em foco tem
//!   contorno de seleção, o aninhado não) está em `tests/menu_extra.rs`.

mod support;

use std::cell::RefCell;
use std::rc::Rc;

use je_base::actions::{Action, ActionRef, ActionRunner, IActionRunner, SubmenuAction, ToActionProps, to_action};
use je_base::dynamic::{Dynamic, ObjectRef};
use je_base::key_codes::KeyCode;
use je_base::keybindings::{ResolvedChord, ResolvedKeybinding, SingleModifierChord};
use je_base::platform::Platform;
use je_base::promise::MaybePromise;
use je_base::runtime;
use je_render::kurbo::Rect;
use je_ui::{Content, Event, EventKind, KeyData, Modifiers, NodeId, PointerButton, PointerData, Ui};
use je_widgets::context_view_api::{CONTEXT_VIEW_MENU_MOTION_CLASS, CONTEXT_VIEW_MENU_MOTION_CLOSING_CLASS};
use je_widgets::menu::{IMenuOptions, Menu, MenuActionViewItem, MenuItem, unthemed_menu_styles};
use je_widgets::modifier_key_emitter::ModifierKeyEmitter;
use support::{LeakGuard, create_container, ensure_no_disposables_are_leaked_in_test_suite};

fn new_ui() -> Ui {
    let ui = Ui::new();
    let root = ui.root();
    ui.tree_mut().set_bounds(root, Rect::new(0.0, 0.0, 1024.0, 768.0));
    ui
}

/// `append(document.body, $('div'))`.
fn append_host(ui: &Ui) -> NodeId {
    let host = create_container(ui, 1024.0, 0.0);
    let root = ui.root();
    ui.tree_mut().append_child(root, host);
    host
}

/// O `teardown` da suíte: `sinon.restore()` (o relógio volta sozinho) e `resetKeyStatus()`.
fn reset_modifiers(ui: &Ui) {
    ModifierKeyEmitter::get_instance(ui).reset_key_status();
}

fn new_menu(store: &LeakGuard, ui: &Ui, host: NodeId, actions: &[ActionRef], options: IMenuOptions) -> Rc<Menu> {
    store.add_rc(Menu::new(ui, host, actions, options, Rc::new(unthemed_menu_styles())))
}

fn string(value: &str) -> Dynamic {
    Dynamic::String(value.into())
}

fn pointer(x: f64, y: f64, modifiers: Modifiers) -> PointerData {
    PointerData { x, y, button: PointerButton::Primary, modifiers, click_count: 1 }
}

fn dispatch_pointer(ui: &Ui, kind: EventKind, target: NodeId, x: f64, y: f64, modifiers: Modifiers) {
    ui.dispatch(&mut Event::new(kind, target).with_pointer(pointer(x, y, modifiers)));
}

fn dispatch_key(ui: &Ui, kind: EventKind, target: NodeId, key_code: KeyCode, modifiers: Modifiers) {
    ui.dispatch(&mut Event::new(kind, target).with_key(KeyData { key_code, modifiers }));
}

/// Os itens que não são separadores, na ordem (o `.action-menu-item`).
fn menu_items_of(menu: &Menu) -> Vec<Rc<MenuActionViewItem>> {
    menu.menu_items().into_iter().filter_map(|i| if let MenuItem::Action(a) = i { Some(a) } else { None }).collect()
}

/// O `textContent` de um nó: o texto de todos os `Content::Text` da subárvore, na ordem.
fn text_content(ui: &Ui, node: NodeId) -> String {
    let tree = ui.tree();
    tree.subtree(node).into_iter().filter_map(|n| if let Content::Text { text, .. } = &tree.style(n).content { Some(text.clone()) } else { None }).collect()
}

fn find_with_class(ui: &Ui, root: NodeId, class: &str) -> Vec<NodeId> {
    let tree = ui.tree();
    tree.subtree(root).into_iter().skip(1).filter(|n| tree.has_class(*n, class)).collect()
}

/// `toAction({ id, label, run })`: o `run` empilha o primeiro argumento em `runs`.
fn run_pushing(runs: &Rc<RefCell<Vec<Dynamic>>>) -> je_base::actions::ToActionRun {
    let runs = runs.clone();
    Rc::new(move |args: &[Dynamic]| {
        runs.borrow_mut().push(args.first().cloned().unwrap_or(Dynamic::Undefined));
        MaybePromise::Value(Dynamic::Undefined)
    })
}

fn props(id: &str, label: &str, run: je_base::actions::ToActionRun) -> ToActionProps {
    ToActionProps { id: id.to_owned(), label: label.to_owned(), tooltip: None, enabled: None, checked: None, class: None, alt: None, run }
}

fn assert_same(actual: &[Dynamic], expected: &[Dynamic], message: &str) {
    let same = actual.len() == expected.len() && actual.iter().zip(expected).all(|(a, b)| a == b);
    assert!(same, "{message}: esperado {} itens, obtido {}", expected.len(), actual.len());
}

fn is_windows_or_linux() -> bool {
    let platform = Platform::detect();
    platform.is_windows || platform.is_linux
}

fn selects_alternatives(submenu: bool, keyboard: bool) {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = new_ui();
    reset_modifiers(&ui);
    runtime::with_faked_timers(|| {
        let host = append_host(&ui);
        let context = Dynamic::Object(ObjectRef::from_entries([("source", string("test"))]));
        let runs: Rc<RefCell<Vec<Dynamic>>> = Rc::new(RefCell::new(Vec::new()));
        let runner = store.add_rc(ActionRunner::new());
        let on_will_run = runs.clone();
        store.add(runner.on_will_run().listen(move |e| on_will_run.borrow_mut().push(string(&e.action.id()))));
        let alternative = to_action(props("alternative", "Alternative", run_pushing(&runs)));
        let action = to_action(ToActionProps { alt: Some(alternative), ..props("primary", "Primary", run_pushing(&runs)) });
        let actions: Vec<ActionRef> = if submenu { vec![SubmenuAction::new("submenu", "Submenu", vec![action], None)] } else { vec![action] };
        let menu = new_menu(&store, &ui, host, &actions, IMenuOptions { context: Some(context.clone()), action_runner: Some(runner.clone()), ..Default::default() });
        menu.focus(true);
        if submenu {
            menu.trigger(0);
        }
        runtime::advance_fake_time(100);
        let item = if submenu {
            let sub = menu_items_of(&menu)[0].submenu().expect("o submenu abriu");
            menu_items_of(&sub)[0].container().expect("li do item")
        } else {
            menu_items_of(&menu)[0].container().expect("li do item")
        };

        let none = Modifiers::default();
        for modifiers in [none, Modifiers { alt: true, ..none }, Modifiers { shift: true, ..none }, Modifiers { alt: true, shift: true, ..none }, none] {
            if keyboard {
                dispatch_key(&ui, EventKind::KeyDown, item, KeyCode::Enter, modifiers);
                dispatch_key(&ui, EventKind::KeyUp, item, KeyCode::Enter, modifiers);
            } else {
                dispatch_pointer(&ui, EventKind::PointerUp, item, 5.0, 5.0, modifiers);
            }
            runtime::advance_fake_time(1);
        }
        let shift_runs: Vec<Dynamic> = if is_windows_or_linux() {
            vec![string("alternative"), context.clone()]
        } else if keyboard {
            vec![]
        } else {
            vec![string("primary"), context.clone()]
        };
        let mut expected = vec![string("primary"), context.clone(), string("alternative"), context.clone()];
        expected.extend(shift_runs);
        expected.extend([string("alternative"), context.clone(), string("primary"), context.clone()]);
        assert_same(&runs.borrow(), &expected, "ações rodadas");
    });
    reset_modifiers(&ui);
}

#[test]
fn selects_alternatives_at_mouse_activation_time_in_a_menu() {
    selects_alternatives(false, false);
}

#[test]
fn selects_alternatives_at_keyboard_activation_time_in_a_menu() {
    selects_alternatives(false, true);
}

#[test]
fn selects_alternatives_at_mouse_activation_time_in_a_submenu() {
    selects_alternatives(true, false);
}

#[test]
fn selects_alternatives_at_keyboard_activation_time_in_a_submenu() {
    selects_alternatives(true, true);
}

/// O `ResolvedKeybinding` do teste (`upcastPartial<ResolvedKeybinding>({ getLabel })`): só o rótulo importa.
struct LabelOnlyKeybinding(String);

impl ResolvedKeybinding for LabelOnlyKeybinding {
    fn get_label(&self) -> Option<String> {
        Some(self.0.clone())
    }
    fn get_aria_label(&self) -> Option<String> {
        None
    }
    fn get_electron_accelerator(&self) -> Option<String> {
        None
    }
    fn get_user_settings_label(&self) -> Option<String> {
        None
    }
    fn is_wysiwyg(&self) -> bool {
        false
    }
    fn has_multiple_chords(&self) -> bool {
        false
    }
    fn get_chords(&self) -> Vec<ResolvedChord> {
        Vec::new()
    }
    fn get_dispatch_chords(&self) -> Vec<Option<String>> {
        Vec::new()
    }
    fn get_single_modifier_dispatch_chords(&self) -> Vec<Option<SingleModifierChord>> {
        Vec::new()
    }
}

#[derive(Debug, PartialEq)]
struct ItemState {
    label: String,
    aria_label: Option<String>,
    mnemonic: Option<String>,
    keybinding: String,
    role: je_ui::Role,
    checked: Option<String>,
    /// As classes conhecidas do teste presentes no rótulo.
    icons: Vec<&'static str>,
}

fn read_items(ui: &Ui, items: &[Rc<MenuActionViewItem>]) -> Vec<ItemState> {
    items
        .iter()
        .map(|item| {
            let (item_node, label) = (item.item_node().expect("a"), item.label_node().expect("rótulo"));
            let tree = ui.tree();
            let icons = ["action-label", "icon", "primary-icon", "alternative-icon"].into_iter().filter(|c| tree.has_class(label, c)).collect();
            ItemState {
                label: text_content(ui, label),
                aria_label: tree.label(label).map(str::to_owned),
                mnemonic: item.attribute("aria-keyshortcuts"),
                keybinding: item.keybinding_node().map(|k| text_content(ui, k)).unwrap_or_default(),
                role: tree.role(item_node),
                checked: item.attribute("aria-checked"),
                icons,
            }
        })
        .collect()
}

fn modifier_updates_labels(modifier: KeyCode, submenu: bool) {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = new_ui();
    runtime::with_faked_timers(|| {
        let modifiers = ModifierKeyEmitter::get_instance(&ui);
        modifiers.reset_key_status();
        let is_alt = modifier == KeyCode::Alt;
        let down = Modifiers { alt: is_alt, shift: !is_alt, ..Modifiers::default() };
        let root = ui.root();
        let press = || dispatch_key(&ui, EventKind::KeyDown, root, modifier, down);
        let release = || dispatch_key(&ui, EventKind::KeyUp, root, modifier, Modifiers::default());
        let runs: Rc<RefCell<Vec<Dynamic>>> = Rc::new(RefCell::new(Vec::new()));
        let record = |name: &'static str| -> je_base::actions::ToActionRun {
            let runs = runs.clone();
            Rc::new(move |_: &[Dynamic]| {
                runs.borrow_mut().push(string(name));
                MaybePromise::Value(Dynamic::Undefined)
            })
        };
        let alternative = to_action(ToActionProps { class: Some("alternative-icon".to_owned()), ..props("alternative", "&Alternative", record("alternative")) });
        let action = to_action(ToActionProps { class: Some("primary-icon".to_owned()), checked: Some(true), alt: Some(alternative), ..props("primary", "&Primary", record("primary")) });
        press();
        let host = append_host(&ui);
        let actions: Vec<ActionRef> = if submenu { vec![SubmenuAction::new("submenu", "Submenu", vec![action.clone(), action], None)] } else { vec![action.clone(), action] };
        let menu = new_menu(
            &store,
            &ui,
            host,
            &actions,
            IMenuOptions {
                enable_mnemonics: true,
                get_key_binding: Some(Rc::new(|action: &ActionRef| Some(Rc::new(LabelOnlyKeybinding(format!("{}-key", action.id()))) as Rc<dyn ResolvedKeybinding>))),
                ..Default::default()
            },
        );
        menu.focus(true);
        if submenu {
            menu.trigger(0);
        }
        let items = || if submenu { menu_items_of(&menu_items_of(&menu)[0].submenu().expect("o submenu abriu")) } else { menu_items_of(&menu) };
        let mut states = vec![read_items(&ui, &items())];
        release();
        states.push(read_items(&ui, &items()));
        press();
        states.push(read_items(&ui, &items()));
        let use_alternative = is_alt || is_windows_or_linux();
        let primary = || ItemState { label: "Primary".into(), aria_label: Some("Primary".into()), mnemonic: Some("p".into()), keybinding: "primary-key".into(), role: je_ui::Role::MenuItemCheckbox, checked: Some("true".into()), icons: vec!["action-label"] };
        let alternative = || ItemState { label: "Alternative".into(), aria_label: Some("Alternative".into()), mnemonic: Some("a".into()), keybinding: "alternative-key".into(), role: je_ui::Role::MenuItem, checked: Some(String::new()), icons: vec!["action-label"] };
        let selected = || if use_alternative { vec![alternative(), alternative()] } else { vec![primary(), primary()] };
        assert_eq!(states, vec![selected(), vec![primary(), primary()], selected()]);

        let item = items()[0].item_node().expect("a");
        let key = if use_alternative { KeyCode::KeyA } else { KeyCode::KeyP };
        dispatch_key(&ui, EventKind::KeyDown, item, key, Modifiers::default());
        dispatch_key(&ui, EventKind::KeyDown, item, key, Modifiers::default());
        let mnemonic_focus = ui.focused() == items()[1].item_node();
        dispatch_key(&ui, EventKind::KeyDown, item, KeyCode::Enter, Modifiers { alt: is_alt, shift: use_alternative && !is_alt, ..Modifiers::default() });
        runtime::advance_fake_time(1);
        let ran: Vec<Dynamic> = runs.borrow().clone();
        assert_same(&ran, &[string(if use_alternative { "alternative" } else { "primary" })], "ações rodadas");
        assert!(mnemonic_focus, "o mnemônico repetido anda entre os itens");
        modifiers.reset_key_status();
    });
}

#[test]
fn alt_updates_menu_labels_mnemonics_and_state_without_hover_in_a_menu() {
    modifier_updates_labels(KeyCode::Alt, false);
}

#[test]
fn alt_updates_menu_labels_mnemonics_and_state_without_hover_in_a_submenu() {
    modifier_updates_labels(KeyCode::Alt, true);
}

#[test]
fn shift_updates_menu_labels_mnemonics_and_state_without_hover_in_a_menu() {
    modifier_updates_labels(KeyCode::Shift, false);
}

#[test]
fn shift_updates_menu_labels_mnemonics_and_state_without_hover_in_a_submenu() {
    modifier_updates_labels(KeyCode::Shift, true);
}

#[test]
fn disabled_alternatives_fall_back_to_the_primary_action() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = new_ui();
    reset_modifiers(&ui);
    runtime::with_faked_timers(|| {
        let host = append_host(&ui);
        let runs: Rc<RefCell<Vec<Dynamic>>> = Rc::new(RefCell::new(Vec::new()));
        let record = |name: &'static str| -> je_base::actions::ToActionRun {
            let runs = runs.clone();
            Rc::new(move |_: &[Dynamic]| {
                runs.borrow_mut().push(string(name));
                MaybePromise::Value(Dynamic::Undefined)
            })
        };
        let alternative = to_action(ToActionProps { enabled: Some(false), ..props("alternative", "Alternative", record("alternative")) });
        let action = to_action(ToActionProps { alt: Some(alternative), ..props("primary", "Primary", record("primary")) });
        let menu = new_menu(&store, &ui, host, &[action], IMenuOptions::default());
        runtime::advance_fake_time(100);
        let item = menu_items_of(&menu)[0].container().expect("li do item");
        dispatch_pointer(&ui, EventKind::PointerUp, item, 5.0, 5.0, Modifiers { alt: true, ..Modifiers::default() });
        runtime::advance_fake_time(1);
        assert_same(&runs.borrow(), &[string("primary")], "ações rodadas");
    });
    reset_modifiers(&ui);
}

// A menu positioned under a resting pointer can receive synthetic pointer events,
// so hover must react only when the pointer coordinates actually change.
#[test]
fn stationary_mouse_does_not_change_focus_110594_148158() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = new_ui();
    reset_modifiers(&ui);
    let host = append_host(&ui);
    let menu = new_menu(&store, &ui, host, &[store.add_rc(Action::new("first", "First", "", true, None)) as ActionRef, store.add_rc(Action::new("second", "Second", "", true, None)) as ActionRef], IMenuOptions::default());
    let action_items: Vec<NodeId> = menu_items_of(&menu).iter().map(|i| i.container().expect("li")).collect();
    let focused_actions = || (0..action_items.len()).map(|index| menu.is_item_focused(index)).collect::<Vec<_>>();
    let none = Modifiers::default();

    menu.focus(true);
    let mut focus_states = vec![focused_actions()];

    dispatch_pointer(&ui, EventKind::PointerEnter, action_items[1], 10.0, 10.0, none);
    focus_states.push(focused_actions());

    dispatch_pointer(&ui, EventKind::PointerMove, action_items[1], 10.0, 10.0, none);
    focus_states.push(focused_actions());

    dispatch_pointer(&ui, EventKind::PointerMove, action_items[1], 11.0, 10.0, none);
    focus_states.push(focused_actions());

    dispatch_pointer(&ui, EventKind::PointerMove, action_items[1], 12.0, 10.0, none);
    focus_states.push(focused_actions());

    dispatch_pointer(&ui, EventKind::PointerMove, action_items[0], 11.0, 10.0, none);
    focus_states.push(focused_actions());

    assert_eq!(focus_states, vec![vec![true, false], vec![true, false], vec![true, false], vec![false, true], vec![false, true], vec![true, false]]);
    reset_modifiers(&ui);
}

#[test]
fn stationary_mouse_does_not_open_submenu_110594_148158() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = new_ui();
    reset_modifiers(&ui);
    runtime::with_faked_timers(|| {
        let host = append_host(&ui);
        let submenu = SubmenuAction::new("submenu", "Submenu", vec![store.add_rc(Action::new("child", "Child", "", true, None)) as ActionRef], None);
        let menu = new_menu(&store, &ui, host, &[submenu], IMenuOptions::default());
        let submenu_item = menu_items_of(&menu)[0].clone();
        let submenu_action = submenu_item.container().expect("li do item");
        let none = Modifiers::default();

        dispatch_pointer(&ui, EventKind::PointerEnter, submenu_action, 10.0, 10.0, none);
        runtime::advance_fake_time(250);
        let expanded_after_mouse_over = submenu_item.attribute("aria-expanded");

        dispatch_pointer(&ui, EventKind::PointerMove, submenu_action, 10.0, 10.0, none);
        runtime::advance_fake_time(250);
        let expanded_after_stationary_mouse_move = submenu_item.attribute("aria-expanded");

        dispatch_pointer(&ui, EventKind::PointerMove, submenu_action, 10.0, 11.0, none);
        runtime::advance_fake_time(250);

        assert_eq!(
            (expanded_after_mouse_over, expanded_after_stationary_mouse_move, submenu_item.attribute("aria-expanded")),
            (Some("false".to_owned()), Some("false".to_owned()), Some("true".to_owned()))
        );
    });
    reset_modifiers(&ui);
}

/// O que o teste lê de `getComputedStyle(...).animationName`: as classes de movimento do contêiner e do
/// elemento rolável dele (e o `::before` do vidro fosco, que não existe no `je-ui`).
#[derive(Debug, PartialEq)]
struct Motion {
    container: (bool, bool),
    content: (bool, bool),
}

fn motion(ui: &Ui, container: NodeId) -> Motion {
    let tree = ui.tree();
    let classes = |node: NodeId| (tree.has_class(node, CONTEXT_VIEW_MENU_MOTION_CLASS), tree.has_class(node, CONTEXT_VIEW_MENU_MOTION_CLOSING_CLASS));
    let content = tree.children_with_class(container, "monaco-scrollable-element")[0];
    Motion { container: classes(container), content: classes(content) }
}

fn submenus_open_without_animation(classes: &[&str], _shadow_dom: bool) {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = new_ui();
    reset_modifiers(&ui);
    let host = append_host(&ui);
    {
        let mut tree = ui.tree_mut();
        tree.add_class(host, "monaco-workbench");
        for class in classes {
            tree.add_class(host, class);
        }
    }
    // `host.attachShadow`: o `je-ui` não tem raiz de sombra, o contêiner é filho do host nos dois casos.
    let root = {
        let mut tree = ui.tree_mut();
        let root = tree.create(host, je_ui::Role::Generic);
        tree.set_class_name(root, "context-view");
        root
    };
    let child = store.add_rc(Action::new("child", "Child", "", true, None)) as ActionRef;
    let nested = SubmenuAction::new("nested", "Nested", vec![child], None);
    let submenu = SubmenuAction::new("submenu", "Submenu", vec![nested], None);
    new_menu(&store, &ui, root, &[submenu], IMenuOptions::default());
    let root_motion = motion(&ui, root);

    let right = |node: NodeId| dispatch_key(&ui, EventKind::KeyUp, node, KeyCode::RightArrow, Modifiers::default());
    right(find_with_class(&ui, root, "monaco-submenu-item")[0]);
    let first = find_with_class(&ui, root, "monaco-submenu")[0];
    right(find_with_class(&ui, first, "monaco-submenu-item")[0]);
    let second = find_with_class(&ui, first, "monaco-submenu")[0];
    let no_motion = || Motion { container: (false, false), content: (false, false) };

    assert_eq!(motion(&ui, root), root_motion);
    assert_eq!((motion(&ui, first), motion(&ui, second)), (no_motion(), no_motion()));
    reset_modifiers(&ui);
}

#[test]
fn submenus_open_without_animation_with_standard_menus_in_regular_dom() {
    submenus_open_without_animation(&[], false);
}

#[test]
fn submenus_open_without_animation_with_standard_menus_in_shadow_dom() {
    submenus_open_without_animation(&[], true);
}

#[test]
fn submenus_open_without_animation_with_frosted_glass_motion_in_regular_dom() {
    submenus_open_without_animation(&["modern-ui-frosted-glass", "monaco-enable-motion"], false);
}

#[test]
fn submenus_open_without_animation_with_frosted_glass_motion_in_shadow_dom() {
    submenus_open_without_animation(&["modern-ui-frosted-glass", "monaco-enable-motion"], true);
}

#[test]
fn submenus_open_without_animation_with_reduced_motion_in_regular_dom() {
    submenus_open_without_animation(&["modern-ui-frosted-glass", "monaco-reduce-motion"], false);
}

#[test]
fn submenus_open_without_animation_with_reduced_motion_in_shadow_dom() {
    submenus_open_without_animation(&["modern-ui-frosted-glass", "monaco-reduce-motion"], true);
}

#[test]
fn submenus_open_without_animation_with_high_contrast_in_regular_dom() {
    submenus_open_without_animation(&["modern-ui-frosted-glass", "monaco-enable-motion", "hc-black"], false);
}

#[test]
fn submenus_open_without_animation_with_high_contrast_in_shadow_dom() {
    submenus_open_without_animation(&["modern-ui-frosted-glass", "monaco-enable-motion", "hc-black"], true);
}

#[test]
#[ignore = "lê o outline-style computado da cascata de CSS de getMenuWidgetCSS em shadow DOM (regras .hc-black); o je-ui não tem folha de estilo nem alto contraste. O equivalente (contorno só no item em foco) está em tests/menu_extra.rs"]
fn high_contrast_selection_outline_does_not_apply_to_nested_submenu_items_327543() {}
