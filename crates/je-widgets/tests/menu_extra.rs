//! Testes extras do `Menu` (`src/vs/base/browser/ui/menu/menu.ts`): o comportamento portado que o
//! `menu.test.ts` não cobre (teclado, mnemônicos, separadores, marca de `checked`, medidas, submenus, rolagem,
//! contorno de seleção). Não fazem parte do porte caso a caso.

mod support;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use je_base::actions::{Action, ActionRef, IAction, Separator, SubmenuAction};
use je_base::dynamic::Dynamic;
use je_base::key_codes::KeyCode;
use je_base::runtime;
use je_render::Rgba;
use je_render::kurbo::Rect;
use je_ui::{KeyData, Modifiers, NodeId, Role, Ui};
use je_widgets::action_view_items::IActionViewItem;
use je_widgets::menu::mnemonics::{LabelPieces, clean_mnemonic, exec_menu_mnemonic, split_escaped_label};
use je_widgets::menu::{IMenuOptions, IMenuStyles, Menu, MenuActionViewItem, MenuItem, format_rule, unthemed_menu_styles};
use je_widgets::modifier_key_emitter::ModifierKeyEmitter;
use je_base::themables::ThemeIcon;
use je_base::lifecycle::IDisposable;
use support::{LeakGuard, create_container, ensure_no_disposables_are_leaked_in_test_suite};

fn new_ui(height: f64) -> Ui {
    let ui = Ui::new();
    let root = ui.root();
    ui.tree_mut().set_bounds(root, Rect::new(0.0, 0.0, 1024.0, height));
    ui
}

fn host_at(ui: &Ui, x: f64, y: f64) -> NodeId {
    let host = create_container(ui, 0.0, 0.0);
    let root = ui.root();
    let mut tree = ui.tree_mut();
    tree.append_child(root, host);
    tree.set_bounds(host, Rect::new(x, y, x, y));
    host
}

fn action(store: &LeakGuard, id: &str, label: &str) -> ActionRef {
    store.add_rc(Action::new(id, label, "", true, None)) as ActionRef
}

fn counting(store: &LeakGuard, id: &str, label: &str, count: &Rc<Cell<u32>>) -> ActionRef {
    let count = count.clone();
    store.add_rc(Action::new(id, label, "", true, Some(Rc::new(move |_| {
        count.set(count.get() + 1);
        Dynamic::Undefined.into()
    })))) as ActionRef
}

fn menu_of(store: &LeakGuard, ui: &Ui, host: NodeId, actions: &[ActionRef], options: IMenuOptions) -> Rc<Menu> {
    store.add_rc(Menu::new(ui, host, actions, options, Rc::new(unthemed_menu_styles())))
}

fn key_down(ui: &Ui, key_code: KeyCode) -> bool {
    ui.key_input(true, KeyData { key_code, modifiers: Modifiers::default() })
}

fn key_up(ui: &Ui, key_code: KeyCode) -> bool {
    ui.key_input(false, KeyData { key_code, modifiers: Modifiers::default() })
}

fn items(menu: &Menu) -> Vec<Rc<MenuActionViewItem>> {
    menu.menu_items().into_iter().filter_map(|i| if let MenuItem::Action(a) = i { Some(a) } else { None }).collect()
}

fn reset(ui: &Ui) {
    ModifierKeyEmitter::get_instance(ui).reset_key_status();
}

#[test]
fn clean_mnemonic_removes_the_marker_like_the_original() {
    assert_eq!(clean_mnemonic("&File"), "File");
    assert_eq!(clean_mnemonic("Open &Recent"), "Open Recent");
    assert_eq!(clean_mnemonic("文件(&F)"), "文件");
    assert_eq!(clean_mnemonic("Save && Close"), "Save && Close");
    assert_eq!(clean_mnemonic("No mnemonic"), "No mnemonic");
    assert_eq!(clean_mnemonic("  &Padded  "), "Padded");
}

#[test]
fn mnemonic_regex_finds_the_first_marker_with_its_groups() {
    let m = exec_menu_mnemonic("&File").expect("casa no começo");
    assert_eq!((m.index, m.len, m.group1, m.group2, m.group3), (0, 2, None, None, Some('F')));
    let m = exec_menu_mnemonic("Open &Recent").expect("casa no meio");
    assert_eq!((m.index, m.len, m.group2, m.group3), (4, 3, Some(' '), Some('R')));
    let m = exec_menu_mnemonic("Datei (&D)").expect("casa o formato asiático");
    assert_eq!((m.index, m.len, m.group1), (6, 4, Some('D')));
    assert!(exec_menu_mnemonic("Save && Close").is_none(), "&& é um & literal");
    assert!(exec_menu_mnemonic("trailing &").is_none());
    assert!(exec_menu_mnemonic("a & b").is_none(), "o & antes de espaço não é mnemônico");
}

#[test]
fn escaped_label_is_split_around_the_mnemonic_letter() {
    assert_eq!(split_escaped_label("&amp;File"), LabelPieces::Mnemonic { before: String::new(), letter: "F".into(), after: "ile".into() });
    assert_eq!(split_escaped_label("  Open &amp;Recent  "), LabelPieces::Mnemonic { before: "Open ".into(), letter: "R".into(), after: "ecent".into() });
    // `&amp;&amp;` é o `&&` escapado: vira `&amp;` e não é mnemônico.
    assert_eq!(split_escaped_label("A &amp;&amp; B"), LabelPieces::Plain("A &amp; B".into()));
    assert_eq!(split_escaped_label("Save &amp;&amp; &amp;Close"), LabelPieces::Mnemonic { before: "Save &amp; ".into(), letter: "C".into(), after: "lose".into() });
}

#[test]
fn format_rule_gives_the_codicon_its_font_character() {
    assert_eq!(format_rule(&ThemeIcon::from_id("menu-selection")).as_deref(), Some(".codicon-menu-selection:before { content: '\\eab2'; }"));
    assert_eq!(format_rule(&ThemeIcon::from_id("no-such-icon")), None);
}

#[test]
fn leading_trailing_and_repeated_separators_are_filtered() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = new_ui(768.0);
    reset(&ui);
    let actions = vec![Separator::new() as ActionRef, action(&store, "a", "A"), Separator::new(), Separator::new(), action(&store, "b", "B"), Separator::new()];
    let menu = menu_of(&store, &ui, host_at(&ui, 0.0, 0.0), &actions, IMenuOptions::default());
    let ids: Vec<String> = menu.view_items().iter().map(|i| i.action().id()).collect();
    assert_eq!(ids, ["a", Separator::ID, "b"]);
}

#[test]
fn the_menu_is_as_wide_as_its_widest_item_and_as_tall_as_its_items() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = new_ui(768.0);
    reset(&ui);
    let short = menu_of(&store, &ui, host_at(&ui, 0.0, 0.0), &[action(&store, "a", "A"), action(&store, "b", "B"), action(&store, "c", "C")], IMenuOptions::default());
    // `min-width: 160px`, borda de 1px; 3 itens de 24px e o `padding: 4px 0`.
    assert_eq!(short.menu_size(), (162.0, 3.0 * 24.0 + 8.0 + 2.0));
    let separated = menu_of(&store, &ui, host_at(&ui, 300.0, 0.0), &[action(&store, "a", "A"), Separator::new(), action(&store, "b", "B")], IMenuOptions::default());
    assert_eq!(separated.menu_size().1, 24.0 * 2.0 + 11.0 + 8.0 + 2.0, "o separador tem 11px");
    let wide = menu_of(&store, &ui, host_at(&ui, 600.0, 0.0), &[action(&store, "long", "A label that is long enough to make the menu wider than its minimum width, surely")], IMenuOptions::default());
    assert!(wide.menu_size().0 > 162.0);
    // O contêiner dá o tamanho do menu (shrink-to-fit do CSS).
    let size = wide.menu_size();
    let bounds = ui.tree().bounds(wide.container());
    assert_eq!((bounds.width(), bounds.height()), size);
}

#[test]
fn arrows_home_end_and_tab_navigate_skipping_separators_and_disabled_items() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = new_ui(768.0);
    reset(&ui);
    let disabled = action(&store, "disabled", "Disabled");
    disabled.set_enabled(false);
    let actions = vec![action(&store, "a", "A"), Separator::new(), disabled, action(&store, "b", "B"), action(&store, "c", "C")];
    let menu = menu_of(&store, &ui, host_at(&ui, 0.0, 0.0), &actions, IMenuOptions::default());
    menu.focus(true);
    // Índices da barra: a=0, separador=1, desabilitado=2, b=3, c=4.
    assert_eq!(menu.focused_item(), Some(0));
    key_down(&ui, KeyCode::DownArrow);
    assert_eq!(menu.focused_item(), Some(3), "pula o separador e o desabilitado");
    key_down(&ui, KeyCode::DownArrow);
    assert_eq!(menu.focused_item(), Some(4));
    key_down(&ui, KeyCode::DownArrow);
    assert_eq!(menu.focused_item(), Some(0), "dá a volta");
    key_down(&ui, KeyCode::UpArrow);
    assert_eq!(menu.focused_item(), Some(4));
    key_down(&ui, KeyCode::Home);
    assert_eq!(menu.focused_item(), Some(0));
    key_down(&ui, KeyCode::End);
    assert_eq!(menu.focused_item(), Some(4));
    assert!(!key_down(&ui, KeyCode::Tab), "o Tab é impedido nos menus");
    assert!(ui.focused().is_some_and(|f| menu.is_item_focused(4) && ui.tree().is_ancestor(menu.dom_node(), f)));
}

#[test]
fn escape_cancels_and_enter_runs_the_focused_action() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = new_ui(768.0);
    reset(&ui);
    runtime::with_faked_timers(|| {
        let (first, second) = (Rc::new(Cell::new(0)), Rc::new(Cell::new(0)));
        let menu = menu_of(&store, &ui, host_at(&ui, 0.0, 0.0), &[counting(&store, "a", "A", &first), counting(&store, "b", "B", &second)], IMenuOptions::default());
        let cancelled = Rc::new(Cell::new(0));
        let c = cancelled.clone();
        store.add(menu.on_did_cancel().listen(move |_| c.set(c.get() + 1)));
        menu.focus(true);
        key_down(&ui, KeyCode::DownArrow);
        key_down(&ui, KeyCode::Enter);
        runtime::advance_fake_time(1);
        assert_eq!((first.get(), second.get()), (0, 1));
        key_down(&ui, KeyCode::Escape);
        assert_eq!(cancelled.get(), 1);
    });
}

#[test]
fn a_checked_item_shows_the_check_and_is_a_menuitemcheckbox() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = new_ui(768.0);
    reset(&ui);
    let (on, off, plain) = (action(&store, "on", "On"), action(&store, "off", "Off"), action(&store, "plain", "Plain"));
    on.set_checked(Some(true));
    off.set_checked(Some(false));
    let menu = menu_of(&store, &ui, host_at(&ui, 0.0, 0.0), &[on, off, plain], IMenuOptions::default());
    let menu_items = items(&menu);
    let items = menu_items;
    let state = |item: &MenuActionViewItem| {
        let tree = ui.tree();
        (tree.style(item.check_node().expect("marca")).visible, tree.has_class(item.item_node().expect("a"), "checked"), tree.role(item.item_node().expect("a")), item.attribute("aria-checked"))
    };
    assert_eq!(state(&items[0]), (true, true, Role::MenuItemCheckbox, Some("true".to_owned())));
    assert_eq!(state(&items[1]), (false, false, Role::MenuItemCheckbox, Some("false".to_owned())));
    assert_eq!(state(&items[2]), (false, false, Role::MenuItem, Some(String::new())));
    // O `aria-posinset`/`aria-setsize` contam os itens que não são separadores.
    assert_eq!((items[1].attribute("aria-posinset"), items[1].attribute("aria-setsize")), (Some("2".to_owned()), Some("3".to_owned())));
}

#[test]
fn a_mnemonic_key_focuses_and_runs_the_item_and_skips_disabled_ones() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = new_ui(768.0);
    reset(&ui);
    let (alpha, beta) = (Rc::new(Cell::new(0)), Rc::new(Cell::new(0)));
    let delta = action(&store, "delta", "&Delta");
    delta.set_enabled(false);
    let menu = menu_of(&store, &ui, host_at(&ui, 0.0, 0.0), &[counting(&store, "alpha", "&Alpha", &alpha), counting(&store, "beta", "&Beta", &beta), delta], IMenuOptions { enable_mnemonics: true, ..Default::default() });
    let observed = runtime::with_faked_timers(|| {
        menu.focus(true);
        key_down(&ui, KeyCode::KeyD);
        let after_disabled = menu.focused_item();
        key_down(&ui, KeyCode::KeyB);
        runtime::advance_fake_time(1);
        (after_disabled, menu.focused_item())
    });
    assert_eq!(observed.0, Some(0), "o mnemônico de um item desabilitado não vale");
    assert_eq!((alpha.get(), beta.get(), observed.1), (0, 1, Some(0)), "só o item de submenu ganha o foco; o de ação roda");
    assert_eq!(items(&menu)[0].get_mnemonic().as_deref(), Some("a"));
}

#[test]
fn the_mnemonic_letter_is_underlined_only_with_mnemonics_enabled() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = new_ui(768.0);
    reset(&ui);
    let with = menu_of(&store, &ui, host_at(&ui, 0.0, 0.0), &[action(&store, "a", "&Alpha")], IMenuOptions { enable_mnemonics: true, ..Default::default() });
    let without = menu_of(&store, &ui, host_at(&ui, 300.0, 0.0), &[action(&store, "a", "&Alpha")], IMenuOptions::default());
    let text = |menu: &Menu| {
        let item = items(menu)[0].clone();
        let tree = ui.tree();
        let label = item.label_node().expect("rótulo");
        (tree.children(label).len(), tree.children_with_class(label, "u").len(), item.get_mnemonic())
    };
    // `&Alpha`: a letra (`<u>`) e o resto; sem mnemônicos o rótulo é `Alpha` num trecho só.
    assert_eq!(text(&with), (2, 1, Some("a".to_owned())));
    assert_eq!(text(&without), (1, 0, None));
}

#[test]
fn a_submenu_opens_with_the_right_arrow_beside_the_item_and_closes_with_the_left_arrow() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = new_ui(768.0);
    reset(&ui);
    let submenu = SubmenuAction::new("submenu", "Submenu", vec![action(&store, "child", "Child"), action(&store, "other", "Other")], None);
    let menu = menu_of(&store, &ui, host_at(&ui, 50.0, 50.0), &[submenu], IMenuOptions::default());
    menu.focus(true);
    let item = items(&menu)[0].clone();
    assert_eq!(item.attribute("aria-expanded").as_deref(), Some("false"));

    key_down(&ui, KeyCode::RightArrow);
    key_up(&ui, KeyCode::RightArrow);
    assert_eq!(item.attribute("aria-expanded").as_deref(), Some("true"));
    let sub = item.submenu().expect("o submenu abriu");
    assert!(sub.is_item_focused(0), "abre com o primeiro item em foco");
    // À direita do item, alinhado ao topo dele menos o `padding` da barra.
    let (li, abs) = (ui.tree().absolute_bounds(item.container().expect("li")), ui.tree().absolute_bounds(sub.container()));
    assert_eq!((abs.x0, abs.y0), (li.x1, li.y0 - 4.0));

    key_down(&ui, KeyCode::LeftArrow);
    key_up(&ui, KeyCode::LeftArrow);
    assert_eq!(item.attribute("aria-expanded").as_deref(), Some("false"));
    assert!(menu.is_item_focused(0), "o foco volta ao item do pai");
    assert!(ui.tree().children_with_class(item.container().expect("li"), "monaco-submenu").is_empty(), "o contêiner do submenu saiu da árvore");
}

#[test]
fn a_submenu_without_room_on_the_right_opens_on_the_left() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = new_ui(768.0);
    reset(&ui);
    let submenu = SubmenuAction::new("submenu", "Submenu", vec![action(&store, "child", "Child")], None);
    let menu = menu_of(&store, &ui, host_at(&ui, 900.0, 50.0), &[submenu], IMenuOptions::default());
    menu.focus(true);
    let item = items(&menu)[0].clone();
    item.open(true);
    let sub = item.submenu().expect("o submenu abriu");
    let abs = ui.tree().absolute_bounds(sub.container());
    // A borda esquerda do item menos a largura do submenu.
    let li = ui.tree().absolute_bounds(item.container().expect("li"));
    assert_eq!(abs.x0, li.x0 - sub.menu_size().0);
}

#[test]
fn an_empty_submenu_shows_the_disabled_empty_item_and_a_submenu_cycle_is_dropped() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = new_ui(768.0);
    reset(&ui);
    let empty = SubmenuAction::new("empty", "Empty", Vec::new(), None);
    let menu = menu_of(&store, &ui, host_at(&ui, 0.0, 0.0), &[empty], IMenuOptions::default());
    let item = items(&menu)[0].clone();
    item.open(true);
    let sub = item.submenu().expect("o submenu abriu");
    let children = items(&sub);
    assert_eq!(children.len(), 1);
    assert_eq!((children[0].action().id(), children[0].action().enabled()), ("vs.actions.empty".to_owned(), false));

    // Um submenu com o mesmo id de um antepassado é um ciclo: sai.
    let inner = SubmenuAction::new("loop", "Inner", vec![action(&store, "leaf", "Leaf")], None);
    let outer = SubmenuAction::new("loop", "Outer", vec![inner as ActionRef], None);
    let menu = menu_of(&store, &ui, host_at(&ui, 400.0, 0.0), &[outer], IMenuOptions::default());
    let item = items(&menu)[0].clone();
    item.open(true);
    assert_eq!(item.submenu().expect("o submenu abriu").length(), 0);
}

#[test]
fn a_tall_menu_is_limited_to_the_window_and_scrolls_to_the_focused_item() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = new_ui(400.0);
    reset(&ui);
    let many: Vec<ActionRef> = (0..40).map(|i| action(&store, &format!("item{i}"), &format!("Item {i}"))).collect();
    let menu = menu_of(&store, &ui, host_at(&ui, 0.0, 0.0), &many, IMenuOptions::default());
    // `max(10, innerHeight - top - 35)` mais a borda.
    assert_eq!(menu.menu_size().1, 400.0 - 35.0 + 2.0);
    assert_eq!(menu.scroll_offset(), 0.0);
    menu.focus(true);
    for _ in 0..30 {
        key_down(&ui, KeyCode::DownArrow);
    }
    assert!(menu.scroll_offset() > 0.0, "o foco leva o item à vista");
    let li = items(&menu)[30].container().expect("li");
    let abs = ui.tree().absolute_bounds(li);
    let window = ui.tree().absolute_bounds(menu.get_container());
    assert!(abs.y0 >= window.y0 && abs.y1 <= window.y1, "o item em foco está dentro da janela do menu: {abs:?} em {window:?}");
}

fn styles_with_selection_border() -> IMenuStyles {
    let yellow = Rgba::new(255, 255, 0, 255);
    IMenuStyles { foreground_color: Some(Rgba::new(255, 255, 255, 255)), selection_background_color: Some(Rgba::new(0, 0, 255, 255)), selection_border_color: Some(yellow), ..unthemed_menu_styles() }
}

#[test]
fn the_selection_outline_shows_only_for_keyboard_focus_and_not_for_nested_items() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = new_ui(768.0);
    reset(&ui);
    let submenu = SubmenuAction::new("submenu", "Submenu", vec![action(&store, "c1", "Child 1"), action(&store, "c2", "Child 2")], None);
    let menu = store.add_rc(Menu::new(&ui, host_at(&ui, 0.0, 0.0), &[action(&store, "a", "A"), action(&store, "b", "B"), submenu], IMenuOptions::default(), Rc::new(styles_with_selection_border())));
    let border_of = |item: &MenuActionViewItem| ui.tree().style(item.item_node().expect("a")).border.is_some();
    let background_of = |item: &MenuActionViewItem| ui.tree().style(item.item_node().expect("a")).background;
    let items = self::items(&menu);

    menu.focus(true);
    assert_eq!((border_of(&items[0]), background_of(&items[0]).is_some()), (false, true), "foco sem teclado: fundo, sem contorno");
    key_down(&ui, KeyCode::DownArrow);
    assert_eq!((border_of(&items[0]), border_of(&items[1])), (false, true), "foco por teclado: contorno só no item em foco");

    // Abre o submenu pelo teclado: o item do pai fica selecionado, os filhos só têm contorno no que está em foco.
    key_down(&ui, KeyCode::DownArrow);
    key_down(&ui, KeyCode::RightArrow);
    key_up(&ui, KeyCode::RightArrow);
    let children = self::items(&items[2].submenu().expect("o submenu abriu"));
    assert_eq!((border_of(&children[0]), border_of(&children[1])), (true, false));
}

#[test]
fn the_menu_exposes_the_container_class_and_the_dispose_removes_everything() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = new_ui(768.0);
    reset(&ui);
    let host = host_at(&ui, 0.0, 0.0);
    let menu = menu_of(&store, &ui, host, &[action(&store, "a", "A")], IMenuOptions::default());
    assert!(ui.tree().has_class(host, "monaco-menu-container"));
    assert_eq!(ui.tree().role(host), Role::Presentation);
    let before = ui.tree().children(host).len();
    assert_eq!(before, 1);
    menu.dispose();
    assert!(ui.tree().children(host).is_empty());
}

#[test]
fn a_submenu_item_does_not_observe_the_enablement_of_its_action() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = new_ui(768.0);
    reset(&ui);
    let submenu = SubmenuAction::new("submenu", "Submenu", vec![action(&store, "c", "C")], None);
    submenu.set_enabled(false);
    let menu = menu_of(&store, &ui, host_at(&ui, 0.0, 0.0), &[submenu], IMenuOptions::default());
    let li = items(&menu)[0].container().expect("li");
    assert!(!ui.tree().has_class(li, "disabled"), "menus nativos não desabilitam submenus");
}

#[test]
fn trigger_runs_an_action_and_ignores_an_out_of_range_index() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = new_ui(768.0);
    reset(&ui);
    runtime::with_faked_timers(|| {
        let count = Rc::new(Cell::new(0));
        let ran = RefCell::new(0);
        let menu = menu_of(&store, &ui, host_at(&ui, 0.0, 0.0), &[counting(&store, "a", "A", &count)], IMenuOptions::default());
        menu.trigger(0);
        menu.trigger(5);
        runtime::advance_fake_time(1);
        *ran.borrow_mut() += count.get();
        assert_eq!(*ran.borrow(), 1);
    });
}
