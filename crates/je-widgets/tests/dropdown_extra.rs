//! Testes extras do `BaseDropdown`, do `DropdownMenu`, do `DropdownMenuActionViewItem` e do
//! `ActionWithDropdownActionViewItem` (`src/vs/base/browser/ui/dropdown/`): o `dropdown.test.ts` do VS Code 1.143.1
//! só cobre a animação e a classe do menu, então estes cobrem o comportamento portado: clique e teclas, o
//! delegado entregue ao provedor de menus, o contexto, o botão dividido e as medidas. Não fazem parte do porte
//! caso a caso.

mod support;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use je_base::actions::{Action, ActionRef};
use je_base::dynamic::{Dynamic, ObjectRef};
use je_base::key_codes::KeyCode;
use je_render::kurbo::Rect;
use je_ui::{Content, KeyData, Modifiers, NodeId, PointerButton, PointerInput, PointerInputKind, Role, Ui};
use je_widgets::action_view_items::{IActionViewItem, IActionViewItemOptions};
use je_widgets::context_view_api::{Anchor, IContextMenuDelegate, IContextMenuProvider};
use je_widgets::dropdown::{BaseDropdown, DropdownMenu, DropdownMenuOptions, IBaseDropdownOptions, IDropdown, IDropdownMenuOptions};
use je_widgets::dropdown_action_view_item::{ActionWithDropdownActionViewItem, DropdownMenuActionViewItem, IActionWithDropdownActionViewItemOptions, IDropdownMenuActionViewItemOptions, MenuActions};
use support::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite};

#[derive(Default)]
struct RecordingProvider {
    delegates: RefCell<Vec<Rc<dyn IContextMenuDelegate>>>,
}

impl IContextMenuProvider for RecordingProvider {
    fn show_context_menu(&self, delegate: Rc<dyn IContextMenuDelegate>) {
        self.delegates.borrow_mut().push(delegate);
    }
}

fn setup() -> (Ui, NodeId) {
    let ui = Ui::new();
    let container = {
        let mut tree = ui.tree_mut();
        let root = tree.root();
        tree.set_bounds(root, Rect::new(0.0, 0.0, 800.0, 600.0));
        let container = tree.create(root, Role::Generic);
        tree.set_bounds(container, Rect::new(0.0, 0.0, 200.0, 30.0));
        container
    };
    (ui, container)
}

fn action(store: &LeakGuard, id: &str, label: &str, enabled: bool) -> ActionRef {
    store.add_rc(Action::new(id, label, "", enabled, None)) as ActionRef
}

fn center(ui: &Ui, node: NodeId) -> (f64, f64) {
    let r = ui.tree().absolute_bounds(node);
    ((r.x0 + r.x1) / 2.0, (r.y0 + r.y1) / 2.0)
}

fn click(ui: &Ui, node: NodeId, button: PointerButton) {
    let (x, y) = center(ui, node);
    let mut down = PointerInput::new(PointerInputKind::Down, x, y);
    down.button = button;
    ui.pointer_input(down);
    let mut up = PointerInput::new(PointerInputKind::Up, x, y);
    up.button = button;
    ui.pointer_input(up);
}

fn key(ui: &Ui, key_code: KeyCode) {
    ui.key_input(true, KeyData { key_code, modifiers: Modifiers::default() });
}

fn visibility_log(dropdown: &BaseDropdown, store: &LeakGuard) -> Rc<RefCell<Vec<bool>>> {
    let log: Rc<RefCell<Vec<bool>>> = Rc::default();
    let sink = log.clone();
    store.add(dropdown.on_did_change_visibility().listen(move |visible| sink.borrow_mut().push(*visible)));
    log
}

fn sized_dropdown(store: &LeakGuard, ui: &Ui, container: NodeId, options: IBaseDropdownOptions) -> Rc<BaseDropdown> {
    let dropdown = store.add_rc(BaseDropdown::new(ui, container, options));
    ui.tree_mut().set_bounds(dropdown.element(), Rect::new(0.0, 0.0, 100.0, 30.0));
    dropdown
}

#[test]
fn base_dropdown_default_label_is_the_text_centered_in_the_label() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let (ui, container) = setup();
    let dropdown = sized_dropdown(&store, &ui, container, IBaseDropdownOptions { label: Some("Menu".to_owned()), ..IBaseDropdownOptions::default() });
    dropdown.layout();

    let tree = ui.tree();
    let label = dropdown.label().unwrap();
    assert!(tree.has_class(dropdown.element(), "monaco-dropdown"));
    assert!(tree.has_class(label, "dropdown-label"));
    assert_eq!(tree.bounds(label).width(), 100.0);
    assert_eq!(tree.bounds(label).height(), 30.0);
    let text = tree.children(label)[0];
    assert!(matches!(&tree.style(text).content, Content::Text { text, .. } if text == "Menu"));
    let bounds = tree.bounds(text);
    assert!(((bounds.x0 + bounds.x1) / 2.0 - 50.0).abs() < 1e-9);
    assert!(((bounds.y0 + bounds.y1) / 2.0 - 15.0).abs() < 1e-9);
}

#[test]
fn base_dropdown_toggles_on_primary_pointer_down_and_ignores_the_secondary_button() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let (ui, container) = setup();
    let dropdown = sized_dropdown(&store, &ui, container, IBaseDropdownOptions { label: Some("Menu".to_owned()), ..IBaseDropdownOptions::default() });
    let log = visibility_log(&dropdown, &store);
    let label = dropdown.label().unwrap();

    click(&ui, label, PointerButton::Secondary);
    assert!(log.borrow().is_empty());
    assert!(!dropdown.is_visible());

    click(&ui, label, PointerButton::Primary);
    assert!(dropdown.is_visible());
    click(&ui, label, PointerButton::Primary);
    assert!(!dropdown.is_visible());
    assert_eq!(*log.borrow(), [true, false]);
}

#[test]
fn base_dropdown_toggles_with_enter_and_space_and_stops_the_key() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let (ui, container) = setup();
    let dropdown = sized_dropdown(&store, &ui, container, IBaseDropdownOptions::default());
    let reached_window = Rc::new(Cell::new(false));
    store.add(ui.listen(ui.root(), je_ui::EventKind::KeyDown, false, {
        let reached_window = reached_window.clone();
        move |_| reached_window.set(true)
    }));
    ui.focus(dropdown.label().unwrap());

    key(&ui, KeyCode::Enter);
    assert!(dropdown.is_visible());
    key(&ui, KeyCode::Space);
    assert!(!dropdown.is_visible());
    key(&ui, KeyCode::KeyA);
    assert!(reached_window.get(), "uma tecla comum borbulha");
    reached_window.set(false);
    key(&ui, KeyCode::Enter);
    assert!(!reached_window.get(), "Enter é consumido (#57997)");
}

#[test]
fn base_dropdown_does_not_open_when_disabled() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let (ui, container) = setup();
    let enabled = Rc::new(Cell::new(false));
    let dropdown = sized_dropdown(&store, &ui, container, IBaseDropdownOptions { is_enabled: Some({ let enabled = enabled.clone(); Rc::new(move || enabled.get()) }), ..IBaseDropdownOptions::default() });
    click(&ui, dropdown.label().unwrap(), PointerButton::Primary);
    ui.focus(dropdown.label().unwrap());
    key(&ui, KeyCode::Enter);
    dropdown.base_show();
    assert!(!dropdown.is_visible());

    enabled.set(true);
    click(&ui, dropdown.label().unwrap(), PointerButton::Primary);
    assert!(dropdown.is_visible());
}

#[test]
fn base_dropdown_dispose_hides_and_removes_the_label() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let (ui, container) = setup();
    let dropdown = sized_dropdown(&store, &ui, container, IBaseDropdownOptions::default());
    let label = dropdown.label().unwrap();
    dropdown.base_show();
    assert!(dropdown.is_visible());

    je_base::lifecycle::IDisposable::dispose(&*dropdown);
    assert!(!dropdown.is_visible());
    assert!(dropdown.label().is_none());
    assert!(!ui.tree().is_alive(label));
    assert!(ui.tree().is_alive(dropdown.element()), "o `.monaco-dropdown` fica, como no original");
}

#[test]
fn dropdown_menu_hands_the_provider_a_delegate_with_actions_context_and_anchor() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let (ui, container) = setup();
    let provider = Rc::new(RecordingProvider::default());
    let mut options = IDropdownMenuOptions::new(provider.clone());
    options.actions = Some(vec![action(&store, "one", "One", true), action(&store, "two", "Two", true)]);
    let menu = store.add_rc(DropdownMenu::new(&ui, container, options));
    let context = Dynamic::Object(ObjectRef::from_entries([("target", Dynamic::string("x"))]));
    menu.set_menu_options(Some(DropdownMenuOptions { context: Some(context.clone()), ..DropdownMenuOptions::default() }));
    let log = visibility_log(&menu, &store);

    menu.show();

    let delegate = provider.delegates.borrow()[0].clone();
    assert_eq!(delegate.get_actions().iter().map(|a| a.id()).collect::<Vec<_>>(), ["one", "two"]);
    assert!(delegate.get_actions_context(None) == Some(context));
    assert!(matches!(delegate.get_anchor(), Anchor::Node(node) if node == menu.element()));
    assert_eq!(delegate.anchor_alignment(), Some(je_base::layout::AnchorAlignment::Left));
    assert!(delegate.dom_for_shadow_root().is_none());
    assert!(ui.tree().has_class(menu.element(), "active"));
    assert!(menu.is_visible());

    // o menu fecha: o dropdown esconde e perde `active`
    delegate.on_hide(false);
    assert!(!menu.is_visible());
    assert!(!ui.tree().has_class(menu.element(), "active"));
    assert_eq!(*log.borrow(), [true, false]);
}

#[test]
fn dropdown_menu_asks_the_action_provider_on_each_show_and_honors_menu_as_child() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let (ui, container) = setup();
    let provider = Rc::new(RecordingProvider::default());
    let calls = Rc::new(Cell::new(0));
    let mut options = IDropdownMenuOptions::new(provider.clone());
    options.menu_as_child = true;
    options.action_provider = Some({
        let (calls, first) = (calls.clone(), action(&store, "a", "A", true));
        Rc::new(move || {
            calls.set(calls.get() + 1);
            vec![first.clone()]
        })
    });
    let menu = store.add_rc(DropdownMenu::new(&ui, container, options));
    menu.show();
    menu.hide();
    menu.show();

    assert_eq!(calls.get(), 0, "o provedor só é consultado quando o menu pede as ações");
    let delegates = provider.delegates.borrow();
    assert_eq!(delegates.len(), 2);
    assert_eq!(delegates[0].get_actions().len(), 1);
    assert_eq!(delegates[1].get_actions().len(), 1);
    assert_eq!(calls.get(), 2);
    assert_eq!(delegates[0].dom_for_shadow_root(), Some(menu.element()));
}

fn render_dropdown_item(store: &LeakGuard, ui: &Ui, container: NodeId, provider: Rc<RecordingProvider>, action: ActionRef, options: IDropdownMenuActionViewItemOptions) -> Rc<DropdownMenuActionViewItem> {
    let item = store.add_rc(DropdownMenuActionViewItem::new(ui, action, MenuActions::Actions(Vec::new()), provider, options));
    let li = {
        let mut tree = ui.tree_mut();
        let li = tree.create(container, Role::Presentation);
        tree.set_bounds(li, Rect::new(0.0, 0.0, 22.0, 22.0));
        li
    };
    item.render(li);
    item.layout(false, 22.0, 22.0);
    item
}

#[test]
fn dropdown_menu_action_view_item_renders_the_icon_label_and_opens_on_click() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let (ui, container) = setup();
    let provider = Rc::new(RecordingProvider::default());
    let item = render_dropdown_item(&store, &ui, container, provider.clone(), action(&store, "more", "More", true), IDropdownMenuActionViewItemOptions { class_names: vec!["codicon".to_owned(), "codicon-toolbar-more".to_owned()], ..Default::default() });
    let visibility = Rc::new(RefCell::new(Vec::new()));
    store.add(item.on_did_change_visibility().listen({
        let visibility = visibility.clone();
        move |visible| visibility.borrow_mut().push(*visible)
    }));
    let element = item.element().unwrap();

    {
        let tree = ui.tree();
        assert!(tree.has_class(element, "action-label"));
        assert!(tree.has_class(element, "codicon"));
        assert_eq!(tree.role(element), Role::Button);
        assert_eq!(tree.label(element), Some("More"));
        assert!(matches!(&tree.style(element).content, Content::Codicon { id, size, .. } if id == "toolbar-more" && *size == 16.0));
        assert_eq!((tree.bounds(element).width(), tree.bounds(element).height()), (22.0, 22.0));
    }
    click(&ui, element, PointerButton::Primary);
    assert_eq!(provider.delegates.borrow().len(), 1);
    assert_eq!(*visibility.borrow(), [true]);
    item.dropdown_menu().unwrap().hide();
    assert_eq!(*visibility.borrow(), [true, false]);
}

#[test]
fn dropdown_menu_action_view_item_adds_codicon_only_without_an_icon_class_and_follows_enabled() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let (ui, container) = setup();
    let provider = Rc::new(RecordingProvider::default());
    let disabled = action(&store, "d", "D", false);
    let with_icon = render_dropdown_item(&store, &ui, container, provider.clone(), disabled.clone(), IDropdownMenuActionViewItemOptions { class_names: vec!["icon".to_owned()], ..Default::default() });
    let element = with_icon.element().unwrap();
    assert!(!ui.tree().has_class(element, "codicon"));
    assert!(ui.tree().has_class(element, "disabled"));
    assert_eq!(ui.tree().style(element).cursor, Some(je_ui::Cursor::Default));

    click(&ui, element, PointerButton::Primary);
    assert!(provider.delegates.borrow().is_empty(), "desabilitado não abre");

    disabled.set_enabled(true);
    assert!(!ui.tree().has_class(element, "disabled"));
    click(&ui, element, PointerButton::Primary);
    assert_eq!(provider.delegates.borrow().len(), 1);
}

#[test]
fn dropdown_menu_action_view_item_set_action_context_reaches_the_menu() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let (ui, container) = setup();
    let provider = Rc::new(RecordingProvider::default());
    let item = render_dropdown_item(&store, &ui, container, provider.clone(), action(&store, "more", "More", true), IDropdownMenuActionViewItemOptions::default());
    let context = Dynamic::Object(ObjectRef::from_entries([("k", Dynamic::string("v"))]));
    item.set_action_context(Some(context.clone()));
    item.show();
    assert!(provider.delegates.borrow()[0].get_actions_context(None) == Some(context.clone()));
    assert!(item.base().context() == Some(context));
}

#[test]
fn action_with_dropdown_lays_out_label_separator_and_menu_button_and_navigates_with_arrows() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let (ui, container) = setup();
    let provider = Rc::new(RecordingProvider::default());
    let main = action(&store, "run", "Run", true);
    let more = action(&store, "more", "More", true);
    let item = store.add_rc(ActionWithDropdownActionViewItem::new(
        &ui,
        None,
        main,
        IActionWithDropdownActionViewItemOptions { base: IActionViewItemOptions { icon: Some(true), label: Some(false), ..Default::default() }, menu_actions_or_provider: MenuActions::Actions(vec![more]), menu_action_class_names: vec!["prominent".to_owned()], keybinding_provider: None },
        provider.clone(),
    ));
    let li = {
        let mut tree = ui.tree_mut();
        let li = tree.create(container, Role::Presentation);
        tree.set_bounds(li, Rect::new(0.0, 0.0, 0.0, 0.0));
        li
    };
    item.render(li);
    let (width, height) = item.size(false);
    assert_eq!((width, height), (22.0 + 1.0 + 22.0, 22.0));
    ui.tree_mut().set_bounds(li, Rect::new(0.0, 0.0, width, height));
    item.layout(false, width, height);

    let dropdown = item.dropdown_menu_action_view_item().unwrap();
    let button = dropdown.element().unwrap();
    {
        let tree = ui.tree();
        assert!(tree.has_class(li, "action-dropdown-item"));
        let separator = tree.children_with_class(li, "action-dropdown-item-separator")[0];
        assert!(tree.has_class(separator, "prominent"));
        assert!(tree.has_class(button, "dropdown") && tree.has_class(button, "prominent"));
        assert!(matches!(&tree.style(button).content, Content::Codicon { id, .. } if id == "drop-down-button"));
        assert_eq!(tree.absolute_bounds(button).x0, 23.0);
    }

    // seta para a direita: foco vai ao botão do menu; seta para a esquerda: volta
    item.focus(false);
    assert!(item.is_focused());
    key(&ui, KeyCode::RightArrow);
    assert_eq!(ui.focused(), Some(button));
    assert!(dropdown.is_focused() && !item.is_focused());
    key(&ui, KeyCode::LeftArrow);
    assert!(item.is_focused() && !dropdown.is_focused());
    assert_ne!(ui.focused(), Some(button));
    assert_eq!(item.tab_stops(), 1);
}

#[test]
fn action_with_dropdown_keeps_the_menu_button_enabled_state_in_sync() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let (ui, container) = setup();
    let provider = Rc::new(RecordingProvider::default());
    let main = action(&store, "run", "Run", true);
    let item = store.add_rc(ActionWithDropdownActionViewItem::new(
        &ui,
        None,
        main.clone(),
        IActionWithDropdownActionViewItemOptions { base: IActionViewItemOptions { icon: Some(true), label: Some(false), ..Default::default() }, menu_actions_or_provider: MenuActions::Actions(Vec::new()), menu_action_class_names: Vec::new(), keybinding_provider: None },
        provider,
    ));
    let li = ui.tree_mut().create(container, Role::Presentation);
    item.render(li);
    let dropdown = item.dropdown_menu_action_view_item().unwrap();
    assert!(dropdown.action().enabled());
    main.set_enabled(false);
    assert!(!dropdown.action().enabled());
    main.set_enabled(true);
    assert!(dropdown.action().enabled());
    // sem ações no menu, a seta para a direita não leva o foco ao botão (#164050)
    item.focus(false);
    key(&ui, KeyCode::RightArrow);
    assert!(item.is_focused());
}
