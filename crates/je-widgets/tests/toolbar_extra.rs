//! Testes extras do `ToolBar` e do `ToggleMenuAction` (`src/vs/base/browser/ui/toolbar/`): o `toolbar.test.ts` cobre o
//! comportamento responsivo; estes cobrem o resto do que foi portado: o botão "mais" e as ações secundárias,
//! `SubmenuAction`, separador final, ícone e rótulo, contexto, runner, eventos de visibilidade do menu, classes
//! responsivas e descarte. Não fazem parte do porte caso a caso.

mod support;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use je_base::actions::{Action, ActionRef, ActionRunner, IActionRunner, Separator, SubmenuAction};
use je_base::dynamic::{Dynamic, ObjectRef};
use je_render::kurbo::Rect;
use je_ui::{Content, NodeId, PointerInput, PointerInputKind, Role, Ui};
use je_widgets::context_view_api::{IContextMenuDelegate, IContextMenuProvider};
use je_widgets::toolbar::{IToolBarOptions, IToolBarResponsiveBehaviorOptions, ResponsiveKind, ToggleMenuAction, ToolBar};
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

struct Env {
    ui: Ui,
    container: NodeId,
    provider: Rc<RecordingProvider>,
    store: LeakGuard,
}

fn env() -> Env {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = Ui::new();
    let container = {
        let mut tree = ui.tree_mut();
        let root = tree.root();
        tree.set_bounds(root, Rect::new(0.0, 0.0, 800.0, 600.0));
        let container = tree.create(root, Role::Generic);
        tree.set_bounds(container, Rect::new(0.0, 0.0, 273.0, 30.0));
        container
    };
    Env { ui, container, provider: Rc::default(), store }
}

impl Env {
    fn toolbar(&self, options: IToolBarOptions) -> Rc<ToolBar> {
        self.store.add_rc(ToolBar::new(&self.ui, self.container, self.provider.clone(), options))
    }

    fn action(&self, id: &str, label: &str, class: &str) -> ActionRef {
        self.store.add_rc(Action::new(id, label, class, true, None)) as ActionRef
    }

    fn ids(&self, toolbar: &ToolBar) -> Vec<String> {
        (0..toolbar.get_items_length()).map(|i| toolbar.get_item_action(i).unwrap().id()).collect()
    }

    fn click(&self, node: NodeId) {
        let r = self.ui.tree().absolute_bounds(node);
        let (x, y) = ((r.x0 + r.x1) / 2.0, (r.y0 + r.y1) / 2.0);
        self.ui.pointer_input(PointerInput::new(PointerInputKind::Down, x, y));
        self.ui.pointer_input(PointerInput::new(PointerInputKind::Up, x, y));
    }

    /// O rótulo (`a.action-label`) do item `index`.
    fn label(&self, toolbar: &ToolBar, index: usize) -> NodeId {
        toolbar.get_item_view_item(index).unwrap().base().element().unwrap()
    }
}

#[test]
fn secondary_actions_go_to_the_more_button_at_the_end_and_open_its_menu() {
    let env = env();
    let toolbar = env.toolbar(IToolBarOptions::default());
    toolbar.set_actions(&[env.action("a", "A", "codicon codicon-add")], &[env.action("s1", "S1", ""), env.action("s2", "S2", "")]);

    assert_eq!(env.ids(&toolbar), ["a", ToggleMenuAction::ID]);
    assert!(toolbar.has_overflow());
    assert!(env.ui.tree().has_class(toolbar.get_element(), "monaco-toolbar"));
    let more = env.label(&toolbar, 1);
    assert!(env.ui.tree().has_class(more, "codicon-toolbar-more"));
    assert!(matches!(&env.ui.tree().style(more).content, Content::Codicon { id, .. } if id == "toolbar-more"));
    assert_eq!(env.ui.tree().label(more), Some("More Actions..."));

    env.click(more);
    let delegates = env.provider.delegates.borrow();
    assert_eq!(delegates.len(), 1);
    assert_eq!(delegates[0].get_actions().iter().map(|a| a.id()).collect::<Vec<_>>(), ["s1", "s2"]);
}

#[test]
fn running_the_toggle_action_opens_the_menu_and_the_title_is_configurable() {
    let env = env();
    let toolbar = env.toolbar(IToolBarOptions { toggle_menu_title: Some("Mais".to_owned()), ..IToolBarOptions::default() });
    toolbar.set_actions(&[], &[env.action("s", "S", "")]);
    let toggle = toolbar.get_item_action(0).unwrap();
    assert_eq!(toggle.label(), "Mais");

    let _ = toggle.run(&[]);
    je_base::runtime::run_microtasks();
    assert_eq!(env.provider.delegates.borrow().len(), 1);
}

#[test]
fn the_secondary_actions_position_callback_is_clamped_to_the_primary_range() {
    let env = env();
    let position = Rc::new(Cell::new(1.0));
    let toolbar = env.toolbar(IToolBarOptions { get_secondary_actions_position: Some({ let position = position.clone(); Rc::new(move |_: &[ActionRef]| position.get()) }), ..IToolBarOptions::default() });
    let primary = [env.action("a", "A", ""), env.action("b", "B", "")];
    let secondary = [env.action("s", "S", "")];
    for (requested, expected) in [(1.0, 1), (-5.0, 0), (99.0, 2), (f64::INFINITY, 2), (f64::NAN, 2), (0.9, 0)] {
        position.set(requested);
        toolbar.set_actions(&primary, &secondary);
        let toggle_index = env.ids(&toolbar).iter().position(|id| id == ToggleMenuAction::ID).unwrap();
        assert_eq!(toggle_index, expected, "posição pedida {requested}");
    }
}

#[test]
fn trailing_separator_is_appended_only_when_there_are_actions() {
    let env = env();
    let toolbar = env.toolbar(IToolBarOptions { trailing_separator: true, ..IToolBarOptions::default() });
    toolbar.set_actions(&[], &[]);
    assert!(toolbar.is_empty());
    toolbar.set_actions(&[env.action("a", "A", "")], &[]);
    assert_eq!(env.ids(&toolbar), ["a", Separator::ID]);
}

#[test]
fn submenu_actions_render_as_dropdowns_with_the_class_names_of_the_action() {
    let env = env();
    let toolbar = env.toolbar(IToolBarOptions::default());
    let child = env.action("child", "Child", "");
    let submenu: ActionRef = SubmenuAction::new("submenu", "Submenu", vec![child], Some("codicon codicon-gear"));
    toolbar.set_actions(&[submenu], &[]);

    let label = env.label(&toolbar, 0);
    assert!(env.ui.tree().has_class(label, "codicon-gear"));
    env.click(label);
    let delegates = env.provider.delegates.borrow();
    assert_eq!(delegates[0].get_actions().iter().map(|a| a.id()).collect::<Vec<_>>(), ["child"]);
}

#[test]
fn dropdown_visibility_events_follow_the_current_menus_only() {
    let env = env();
    let toolbar = env.toolbar(IToolBarOptions::default());
    let log: Rc<RefCell<Vec<bool>>> = Rc::default();
    env.store.add(toolbar.on_did_change_dropdown_visibility().listen({
        let log = log.clone();
        move |visible| log.borrow_mut().push(*visible)
    }));
    toolbar.set_actions(&[], &[env.action("s", "S", "")]);
    let old_item = toolbar.get_item_view_item(0).unwrap();
    env.click(env.label(&toolbar, 0));
    env.provider.delegates.borrow()[0].on_hide(false);
    assert_eq!(*log.borrow(), [true, false]);

    // um item que saiu da barra não repassa mais nada
    toolbar.set_actions(&[], &[env.action("s", "S", "")]);
    let _ = old_item;
    env.click(env.label(&toolbar, 0));
    assert_eq!(*log.borrow(), [true, false, true]);
}

#[test]
fn set_context_and_action_runner_reach_the_items() {
    let env = env();
    let toolbar = env.toolbar(IToolBarOptions::default());
    toolbar.set_actions(&[env.action("a", "A", "")], &[]);
    let context = Dynamic::Object(ObjectRef::from_entries([("k", Dynamic::string("v"))]));
    toolbar.set_context(Some(context.clone()));
    assert!(toolbar.get_item_view_item(0).unwrap().base().context() == Some(context));

    let runner = env.store.add_rc(ActionRunner::new());
    let runner: Rc<dyn IActionRunner> = runner;
    toolbar.set_action_runner(runner.clone());
    assert!(Rc::ptr_eq(&toolbar.action_runner(), &runner));
    assert!(Rc::ptr_eq(&toolbar.get_item_view_item(0).unwrap().action_runner(), &runner));
}

#[test]
fn icons_are_rendered_by_default_and_labels_only_on_request() {
    let env = env();
    let toolbar = env.toolbar(IToolBarOptions::default());
    toolbar.set_actions(&[env.action("a", "A", "codicon codicon-add")], &[]);
    // ActionViewItem: o rótulo é filho do `li`
    let li = toolbar.get_item_element(0).unwrap();
    let label = env.ui.tree().children_with_class(li, "action-label")[0];
    assert!(matches!(&env.ui.tree().style(label).content, Content::Codicon { id, .. } if id == "add"));
    let default_width = toolbar.get_item_width(0);

    let labeled = env.toolbar(IToolBarOptions { icon: Some(false), label: Some(true), ..IToolBarOptions::default() });
    labeled.set_actions(&[env.action("a", "Add things", "")], &[]);
    assert!(labeled.get_item_width(0) > default_width, "o rótulo de texto é mais largo que o ícone de 22px");
}

#[test]
fn responsive_classes_and_the_reserved_minimum_width() {
    let env = env();
    let plain = env.toolbar(IToolBarOptions::default());
    assert!(!env.ui.tree().has_class(plain.get_element(), "responsive"));
    assert_eq!(plain.action_min_width_property(), None);

    for (kind, all, last) in [(ResponsiveKind::All, true, false), (ResponsiveKind::Last, false, true)] {
        let toolbar = env.toolbar(IToolBarOptions { responsive_behavior: Some(IToolBarResponsiveBehaviorOptions { enabled: true, kind, min_items: Some(1), action_min_width: Some(30.0), ..Default::default() }), ..IToolBarOptions::default() });
        let tree = env.ui.tree();
        assert!(tree.has_class(toolbar.get_element(), "responsive"));
        assert_eq!((tree.has_class(toolbar.get_element(), "responsive-all"), tree.has_class(toolbar.get_element(), "responsive-last")), (all, last));
        drop(tree);
        assert_eq!(toolbar.action_min_width_property(), Some(30.0));
        toolbar.set_actions(&[env.action("a", "A", ""), env.action("b", "B", "")], &[]);
        // minItems 1: (30 + 4) do primeiro, mais o botão "mais" (24), porque há uma ação além do mínimo
        assert_eq!((toolbar.element_min_width(), toolbar.container_min_width()), (Some(58.0), Some(58.0)));
    }
}

#[test]
fn the_element_fills_the_container_width_and_relayout_without_responsive_does_nothing() {
    let env = env();
    let toolbar = env.toolbar(IToolBarOptions::default());
    toolbar.set_actions(&[env.action("a", "A", ""), env.action("b", "B", "")], &[]);
    assert_eq!(env.ui.tree().bounds(toolbar.get_element()).width(), 273.0);
    toolbar.relayout();
    assert_eq!(toolbar.get_items_length(), 2);
    assert!(!toolbar.has_overflow());
}

#[test]
fn focus_without_index_goes_to_the_first_enabled_item_and_with_index_to_that_item() {
    let env = env();
    let toolbar = env.toolbar(IToolBarOptions::default());
    toolbar.set_actions(&[env.action("a", "A", ""), env.action("b", "B", "")], &[]);
    toolbar.focus(None);
    assert!(toolbar.action_bar().is_item_focused(0));
    toolbar.focus(Some(1));
    assert!(toolbar.action_bar().is_item_focused(1));
}

#[test]
fn a_toolbar_can_be_disposed_and_leaves_no_nodes_behind() {
    let env = env();
    let toolbar = env.toolbar(IToolBarOptions::default());
    toolbar.set_actions(&[env.action("a", "A", "")], &[env.action("s", "S", "")]);
    let element = toolbar.get_element();
    je_base::lifecycle::IDisposable::dispose(&*toolbar);
    assert!(!env.ui.tree().is_alive(element));
    assert!(env.ui.tree().children(env.container).is_empty());
}
