//! Porte de `src/vs/base/test/browser/ui/toolbar/toolbar.test.ts` (suíte `ToolBar`), caso a caso e na mesma ordem.
//!
//! Divergências de tradução (nenhuma enfraquece uma asserção; cada uma troca algo do navegador pelo equivalente na
//! árvore do `je-ui`):
//!
//! - O contêiner de 273px é um nó sob a raiz com `set_bounds`; o `ResizeObserver` não existe, então o que o
//!   original dispara sozinho é o `relayout()` explícito do teste (o original também o chama explicitamente).
//! - `sinon.spy(item, 'setActionContext')` não tem par: um item que recebe `setActionContext` guarda o contexto,
//!   e um item descartado guarda `None`; o teste lê `base().context()` (e o contexto do menu) em vez de contar
//!   chamadas. "Nenhuma chamada nos itens removidos" é "contexto continua `None`"; "uma chamada nos atuais" é
//!   "contexto é o objeto novo". `assert.ok(x instanceof DropdownMenuActionViewItem)` é [`is_dropdown_item`]: o
//!   `element` do item é o rótulo dentro de um `.monaco-dropdown` (o trait `IActionViewItem` não tem `as_any`).
//! - O `getBoundingClientRect()` e o `actionBar.getWidth` que o original sobrescreve só mudam a MEDIDA, não o
//!   DOM: `pin_element_width` mede o elemento de fato (o toolbar lê a largura do nó) e as larguras mapeadas vão
//!   para o próprio item (`FixedWidthActionViewItem` lê a largura do mapa a cada layout; o botão "mais" mede
//!   22px, o `16px + 2 * 3px` do `.action-label`, igual ao mapeado). `style.width = '48px'` num `li` é
//!   `set_bounds` do `li`. `style.display` é `NodeStyle::visible`, `style.minWidth` e a propriedade
//!   `--vscode-toolbar-action-min-width` são os leitores `element_min_width`, `container_min_width` e
//!   `action_min_width_property`.
//! - `[tabindex="0"]` é a soma de `IActionViewItem::tab_stops`; `[aria-haspopup="true"]` é o nó de rótulo de um
//!   `DropdownMenuActionViewItem` ([`popup_nodes`]); `document.activeElement` é `ui.focused()`; `focusin` é um
//!   ouvinte de `Focus` em captura no elemento do toolbar.
//! - `Separator.clean`/`join` do menu de transbordo e o `await ...run()` do proxy são síncronos aqui (o `run` do
//!   proxy é um `to_action`, que roda até o primeiro `await`).

mod support;

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::rc::{Rc, Weak};

use je_base::actions::{Action, ActionRef, Separator, SubmenuAction, ToActionProps, to_action};
use je_base::dynamic::{Dynamic, ObjectRef};
use je_base::key_codes::KeyCode;
use je_base::lifecycle::to_disposable;
use je_render::kurbo::Rect;
use je_ui::{EventKind, KeyData, Modifiers, NodeId, Role, Ui};
use je_widgets::action_bar::IActionViewItemProvider;
use je_widgets::action_view_items::{BaseActionViewItem, IActionViewItem, IActionViewItemOptions, ItemSize};
use je_widgets::context_view_api::{IContextMenuDelegate, IContextMenuProvider};
use je_widgets::dropdown_action_view_item::{ActionWithDropdownActionViewItem, IActionWithDropdownActionViewItemOptions, MenuActions};
use je_widgets::text::{natural_line_height, set_node_text, ui_font};
use je_widgets::toolbar::{AllowOverflow, IToolBarOptions, IToolBarResponsiveBehaviorOptions, OverflowFrom, ResponsiveKind, ToggleMenuAction, ToolBar};
use support::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite, query_all};

/// `class FixedWidthActionViewItem extends BaseActionViewItem`: o `li` ganha largura fixa, `overflow: hidden` e o
/// rótulo como texto; `visible = false` é `display: none`. A largura é lida a cada layout (um mapa que o teste
/// muda faz o papel do `actionBar.getWidth` sobrescrito).
struct FixedWidthActionViewItem {
    base: BaseActionViewItem,
    width: Rc<dyn Fn() -> f64>,
    visible: bool,
}

#[allow(clippy::new_ret_no_self)] // devolve o item já como `Rc<dyn IActionViewItem>`, como o `new` do original o usa
impl FixedWidthActionViewItem {
    fn new(ui: &Ui, action: &ActionRef, width: f64, visible: bool) -> Rc<dyn IActionViewItem> {
        Self::with_width(ui, action, Rc::new(move || width), visible)
    }

    fn with_width(ui: &Ui, action: &ActionRef, width: Rc<dyn Fn() -> f64>, visible: bool) -> Rc<dyn IActionViewItem> {
        Rc::new_cyclic(|weak: &Weak<Self>| Self { base: BaseActionViewItem::new_derived(ui, None, action.clone(), IActionViewItemOptions::default(), weak.clone() as Weak<dyn IActionViewItem>), width, visible })
    }
}

impl IActionViewItem for FixedWidthActionViewItem {
    fn base(&self) -> &BaseActionViewItem {
        &self.base
    }

    fn render(&self, container: NodeId) {
        self.base.base_render(container);
        let font = ui_font(13.0, 400);
        let mut tree = self.base.ui().tree_mut();
        let style = tree.style_mut(container);
        style.visible = self.visible; // `container.style.display = visible ? '' : 'none'`
        style.clip = true; // `overflow: hidden`
        set_node_text(&mut tree, container, &self.base.action().label(), &font, je_render::Rgba::new(204, 204, 204, 255), natural_line_height(&font));
    }

    fn size(&self, _vertical: bool) -> ItemSize {
        // `display: none` não ocupa lugar; `width: ${width}px`.
        if self.visible { ((self.width)(), natural_line_height(&ui_font(13.0, 400))) } else { (0.0, 0.0) }
    }
}

je_widgets::impl_action_view_item_dispose!(FixedWidthActionViewItem, base);

/// `const contextMenuProvider: IContextMenuProvider = { showContextMenu: () => { } }`.
struct NoopContextMenuProvider;

impl IContextMenuProvider for NoopContextMenuProvider {
    fn show_context_menu(&self, _delegate: Rc<dyn IContextMenuDelegate>) {}
}

/// O `setup`/`teardown` da suíte: um contêiner de 273px na janela, tirado ao fim.
struct Env {
    ui: Ui,
    container: NodeId,
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
    store.add(to_disposable({
        let ui = ui.clone();
        move || ui.remove(container)
    }));
    Env { ui, container, store }
}

impl Env {
    fn toolbar(&self, options: IToolBarOptions) -> Rc<ToolBar> {
        self.store.add_rc(ToolBar::new(&self.ui, self.container, Rc::new(NoopContextMenuProvider), options))
    }

    /// `store.add(new Action(id, label))`.
    fn action(&self, id: &str, label: &str) -> ActionRef {
        self.action_with(id, label, None)
    }

    /// `store.add(new Action(id, label, undefined, true, run))`.
    fn action_with(&self, id: &str, label: &str, run: Option<je_base::actions::ActionCallback>) -> ActionRef {
        self.store.add_rc(Action::new(id, label, "", true, run)) as ActionRef
    }

    /// Um provedor de itens de largura fixa, com a largura de cada id (ids fora do mapa: item padrão).
    fn fixed_widths(&self, widths: &HashMap<&'static str, f64>) -> IActionViewItemProvider {
        let (ui, widths) = (self.ui.clone(), widths.clone());
        Rc::new(move |action: &ActionRef, _| widths.get(action.id().as_str()).map(|width| FixedWidthActionViewItem::new(&ui, action, *width, true)))
    }

    /// `new FixedWidthActionViewItem(action, width)` para todo item.
    fn all_fixed(&self, width: f64) -> IActionViewItemProvider {
        let ui = self.ui.clone();
        Rc::new(move |action: &ActionRef, _| Some(FixedWidthActionViewItem::new(&ui, action, width, true)))
    }

    /// O contrato do `getBoundingClientRect` sobrescrito: o elemento do toolbar mede `width`.
    fn pin_element_width(&self, toolbar: &ToolBar, width: f64) {
        let element = toolbar.get_element();
        let mut tree = self.ui.tree_mut();
        tree.set_fill(element, false, true);
        let height = tree.bounds(element).height();
        tree.set_bounds(element, Rect::new(0.0, 0.0, width, height));
    }

    fn has_class(&self, node: NodeId, class: &str) -> bool {
        self.ui.tree().has_class(node, class)
    }

    fn has_overflow_class(&self, toolbar: &ToolBar) -> bool {
        let bar = query_all(&self.ui.tree(), toolbar.get_element(), &["monaco-action-bar"])[0];
        self.has_class(bar, "has-overflow")
    }

    /// `Array.from({ length: toolbar.getItemsLength() }, (_, index) => toolbar.getItemAction(index)?.id)`.
    fn ids(&self, toolbar: &ToolBar) -> Vec<String> {
        (0..toolbar.get_items_length()).map(|index| toolbar.get_item_action(index).map(|action| action.id()).unwrap_or_default()).collect()
    }

    /// `toolbar.getElement().querySelectorAll('[tabindex="0"]').length`.
    fn tab_stops(&self, toolbar: &ToolBar) -> usize {
        toolbar.action_bar().view_items().iter().map(|item| item.tab_stops()).sum()
    }

    /// `getItemElement(index)?.style.display === 'none'`.
    fn display_none(&self, toolbar: &ToolBar, index: usize) -> bool {
        toolbar.get_item_element(index).is_some_and(|li| !self.ui.tree().style(li).visible)
    }

    /// Os nós `[aria-haspopup="true"]` sob `root`: o rótulo de um `DropdownMenuActionViewItem`.
    fn popup_nodes(&self, root: NodeId) -> Vec<NodeId> {
        let tree = self.ui.tree();
        tree.subtree(root)
            .into_iter()
            .skip(1)
            .filter(|node| tree.has_class(*node, "action-label") && tree.parent(*node).and_then(|p| tree.parent(p)).is_some_and(|dropdown| tree.has_class(dropdown, "monaco-dropdown")))
            .collect()
    }

    /// `item instanceof DropdownMenuActionViewItem`.
    fn is_dropdown_item(&self, item: &Option<Rc<dyn IActionViewItem>>) -> bool {
        let Some(element) = item.as_ref().and_then(|item| item.base().element()) else { return false };
        let tree = self.ui.tree();
        tree.parent(element).and_then(|p| tree.parent(p)).is_some_and(|dropdown| tree.has_class(dropdown, "monaco-dropdown"))
    }
}

fn menu_action_ids(action: &ActionRef) -> Option<Vec<String>> {
    action.as_any().downcast_ref::<ToggleMenuAction>().map(|toggle| toggle.menu_actions().iter().map(|a| a.id()).collect())
}

fn width_source(cell: &Rc<Cell<f64>>) -> Option<Rc<dyn Fn() -> f64>> {
    let cell = cell.clone();
    Some(Rc::new(move || cell.get()))
}

fn responsive(kind: ResponsiveKind) -> IToolBarResponsiveBehaviorOptions {
    IToolBarResponsiveBehaviorOptions { enabled: true, kind, ..IToolBarResponsiveBehaviorOptions::default() }
}

fn strings(items: &[&str]) -> Vec<String> {
    items.iter().map(|s| (*s).to_owned()).collect()
}

#[test]
fn only_updates_current_dropdown_contexts_after_repeated_responsive_overflow() {
    let env = env();
    let available_width = Rc::new(Cell::new(400.0));
    let child = env.action("child", "Child");
    let actions: Vec<ActionRef> = vec![env.action("first", "First"), SubmenuAction::new("submenu", "Submenu", vec![child], None)];
    let toolbar = env.toolbar(IToolBarOptions {
        responsive_behavior: Some(IToolBarResponsiveBehaviorOptions { action_min_width: Some(24.0), reserve_min_width: Some(false), get_available_width: width_source(&available_width), ..responsive(ResponsiveKind::All) }),
        ..IToolBarOptions::default()
    });
    toolbar.set_actions(&actions, &[]);
    // os itens que saem da barra: o contexto deles nunca deve ser tocado
    let mut removed_view_items: Vec<Rc<dyn IActionViewItem>> = Vec::new();
    for _ in 0..5 {
        let submenu = toolbar.get_item_view_item(1);
        assert!(env.is_dropdown_item(&submenu));
        available_width.set(24.0);
        toolbar.relayout();
        let overflow = toolbar.get_item_view_item(0);
        assert!(env.is_dropdown_item(&overflow));
        available_width.set(400.0);
        toolbar.relayout();
        removed_view_items.extend([submenu.unwrap(), overflow.unwrap()]);
    }
    let current = toolbar.get_item_view_item(1);
    assert!(env.is_dropdown_item(&current));
    let context = Dynamic::Object(ObjectRef::from_entries([("response", Dynamic::string("current"))]));
    toolbar.set_context(Some(context.clone()));
    let removed_context_updates: Vec<bool> = removed_view_items.iter().map(|item| item.base().context().is_some()).collect();
    let current_context = current.as_ref().and_then(|item| item.base().context());
    let rendered_actions = vec![toolbar.get_item_action(0).map(|a| a.id()), toolbar.get_item_action(1).map(|a| a.id())];
    assert_eq!(removed_context_updates, vec![false; 10]);
    assert!(current_context == Some(context));
    assert_eq!(rendered_actions, vec![Some("first".to_owned()), Some("submenu".to_owned())]);
}

#[test]
fn clears_removed_overflow_context_and_actions_when_replacing_the_toolbar_actions() {
    let env = env();
    let secondary = env.action("secondary", "Secondary");
    let toolbar = env.toolbar(IToolBarOptions::default());
    toolbar.set_actions(&[], &[secondary]);
    let overflow = toolbar.get_item_view_item(0);
    assert!(env.is_dropdown_item(&overflow));
    let overflow = overflow.unwrap();
    let action = overflow.action();
    assert!(action.as_any().is::<ToggleMenuAction>());
    let replacement = env.action("replacement", "Replacement");
    toolbar.set_actions(&[replacement], &[]);
    toolbar.set_context(Some(Dynamic::Object(ObjectRef::from_entries([("response", Dynamic::string("next"))]))));
    // `setContext.callCount === 0`: o item removido continua sem contexto
    assert!(overflow.base().context().is_none());
    assert_eq!(menu_action_ids(&action), Some(Vec::<String>::new()));
    assert_eq!(toolbar.get_items_length(), 1);
    assert!(!toolbar.has_overflow());
}

#[test]
fn preserves_the_secondary_actions_trigger_position_through_responsive_relayouts() {
    let env = env();
    let available_width = Rc::new(Cell::new(300.0));
    let toolbar = env.toolbar(IToolBarOptions {
        get_secondary_actions_position: Some(Rc::new(|primary_actions: &[ActionRef]| primary_actions.iter().position(|action| action.id() == "close").map_or(-1.0, |index| index as f64))),
        responsive_behavior: Some(IToolBarResponsiveBehaviorOptions { min_items: Some(0), get_available_width: width_source(&available_width), ..responsive(ResponsiveKind::Last) }),
        ..IToolBarOptions::default()
    });
    toolbar.set_actions(&[env.action("pin", "Pin"), env.action("close", "Close")], &[env.action("rename", "Rename")]);

    let snapshot = || {
        let shrinkable = (0..toolbar.get_items_length()).find(|index| toolbar.get_item_element(*index).is_some_and(|li| env.has_class(li, "toolbar-shrinkable-item")));
        (env.ids(&toolbar), shrinkable.and_then(|index| toolbar.get_item_action(index)).map(|action| action.id()))
    };
    let initial = snapshot();
    available_width.set(1.0);
    toolbar.relayout();
    let collapsed = snapshot();
    available_width.set(300.0);
    toolbar.relayout();

    let expanded = (strings(&["pin", ToggleMenuAction::ID, "close"]), Some("close".to_owned()));
    assert_eq!((initial, collapsed, snapshot()), (expanded.clone(), (strings(&[ToggleMenuAction::ID]), None), expanded));
}

#[test]
fn keeps_the_last_primary_action_shrinkable_when_overflow_is_inserted() {
    let env = env();
    let widths: HashMap<&'static str, f64> = HashMap::from([
        ("workbench.action.chat.attachContext", 22.0),
        ("workbench.action.chat.openModePicker", 75.0),
        ("workbench.action.chat.openModelPicker", 271.0),
        ("workbench.action.chat.configureTools", 22.0),
        (ToggleMenuAction::ID, 22.0),
    ]);
    let toolbar = env.toolbar(IToolBarOptions {
        responsive_behavior: Some(IToolBarResponsiveBehaviorOptions { min_items: Some(1), action_min_width: Some(22.0), ..responsive(ResponsiveKind::Last) }),
        action_view_item_provider: Some(env.fixed_widths(&widths)),
        ..IToolBarOptions::default()
    });
    env.pin_element_width(&toolbar, 273.0);

    let actions = vec![
        env.action("workbench.action.chat.attachContext", "Add Context..."),
        env.action("workbench.action.chat.openModePicker", "Open Agent Picker"),
        env.action("workbench.action.chat.openModelPicker", "Open Model Picker"),
        env.action("workbench.action.chat.configureTools", "Configure Tools..."),
    ];

    toolbar.set_actions(&actions, &[]);

    // o `actionBar.getWidth` sobrescrito devolve o mapa; aqui as larguras do mapa são as reais
    for index in 0..toolbar.get_items_length() {
        let id = toolbar.get_item_action(index).unwrap().id();
        assert_eq!(toolbar.get_item_width(index), widths[id.as_str()], "{id}");
    }
    assert_eq!(toolbar.get_items_length(), 4);
    assert_eq!(toolbar.get_item_action(0).map(|a| a.id()).as_deref(), Some("workbench.action.chat.attachContext"));
    assert_eq!(toolbar.get_item_action(1).map(|a| a.id()).as_deref(), Some("workbench.action.chat.openModePicker"));
    assert_eq!(toolbar.get_item_action(2).map(|a| a.id()).as_deref(), Some("workbench.action.chat.openModelPicker"));
    assert_eq!(toolbar.get_item_action(3).map(|a| a.id()).as_deref(), Some(ToggleMenuAction::ID));
    assert!(toolbar.has_overflow());
}

#[test]
fn applies_per_action_responsive_min_widths() {
    let env = env();
    let toolbar = env.toolbar(IToolBarOptions {
        responsive_behavior: Some(IToolBarResponsiveBehaviorOptions {
            min_items: Some(1),
            action_min_width: Some(22.0),
            get_action_min_width: Some(Rc::new(|action: &ActionRef| (action.id() == "workbench.action.chat.openModelPicker").then_some(28.0))),
            ..responsive(ResponsiveKind::Last)
        }),
        action_view_item_provider: Some(env.all_fixed(22.0)),
        ..IToolBarOptions::default()
    });

    let actions = vec![
        env.action("workbench.action.chat.attachContext", "Add Context..."),
        env.action("workbench.action.chat.openModePicker", "Open Agent Picker"),
        env.action("workbench.action.chat.openModelPicker", "Open Model Picker"),
    ];

    toolbar.set_actions(&actions, &[]);

    assert_eq!(toolbar.action_min_width_property(), Some(28.0)); // '28px'
}

#[test]
fn relayout_re_evaluates_responsive_overflow_after_action_width_changes() {
    let env = env();
    let widths = Rc::new(RefCell::new(HashMap::from([
        ("workbench.action.chat.attachContext".to_owned(), 22.0),
        ("workbench.action.chat.openModePicker".to_owned(), 22.0),
        ("workbench.action.chat.openModelPicker".to_owned(), 50.0),
        (ToggleMenuAction::ID.to_owned(), 22.0),
    ])));
    let provider: IActionViewItemProvider = {
        let (ui, widths) = (env.ui.clone(), widths.clone());
        Rc::new(move |action: &ActionRef, _| {
            let id = action.id();
            widths.borrow().contains_key(&id).then(|| {
                let widths = widths.clone();
                FixedWidthActionViewItem::with_width(&ui, action, Rc::new(move || widths.borrow()[&id]), true)
            })
        })
    };
    let toolbar = env.toolbar(IToolBarOptions {
        responsive_behavior: Some(IToolBarResponsiveBehaviorOptions { min_items: Some(1), action_min_width: Some(22.0), ..responsive(ResponsiveKind::Last) }),
        action_view_item_provider: Some(provider),
        ..IToolBarOptions::default()
    });
    env.pin_element_width(&toolbar, 110.0);

    let actions = vec![
        env.action("workbench.action.chat.attachContext", "Add Context..."),
        env.action("workbench.action.chat.openModePicker", "Open Mode Picker"),
        env.action("workbench.action.chat.openModelPicker", "Open Model Picker"),
    ];

    toolbar.set_actions(&actions, &[]);

    assert_eq!(toolbar.get_items_length(), 3);
    assert_eq!(toolbar.get_item_action(2).map(|a| a.id()).as_deref(), Some("workbench.action.chat.openModelPicker"));
    assert!(!env.has_overflow_class(&toolbar));

    widths.borrow_mut().insert("workbench.action.chat.openModePicker".to_owned(), 80.0);
    toolbar.action_bar().layout(); // o `getWidth` sobrescrito lia o valor novo na hora
    toolbar.relayout();

    assert_eq!(toolbar.get_items_length(), 3);
    assert_eq!(toolbar.get_item_action(0).map(|a| a.id()).as_deref(), Some("workbench.action.chat.attachContext"));
    assert_eq!(toolbar.get_item_action(1).map(|a| a.id()).as_deref(), Some("workbench.action.chat.openModePicker"));
    assert_eq!(toolbar.get_item_action(2).map(|a| a.id()).as_deref(), Some(ToggleMenuAction::ID));
    assert!(env.has_overflow_class(&toolbar));
}

#[test]
fn does_not_repeatedly_restore_an_action_below_its_required_width() {
    let env = env();
    let widths: HashMap<&'static str, f64> = HashMap::from([("primary.a", 56.0), ("primary.b", 48.0)]);
    let render_counts: Rc<RefCell<HashMap<String, usize>>> = Rc::default();
    let available_width = Rc::new(Cell::new(128.0));

    let provider: IActionViewItemProvider = {
        let (inner, render_counts) = (env.fixed_widths(&widths), render_counts.clone());
        Rc::new(move |action: &ActionRef, options: &IActionViewItemOptions| {
            *render_counts.borrow_mut().entry(action.id()).or_default() += 1;
            inner(action, options)
        })
    };
    let toolbar = env.toolbar(IToolBarOptions {
        responsive_behavior: Some(IToolBarResponsiveBehaviorOptions { min_items: Some(1), action_min_width: Some(48.0), get_available_width: width_source(&available_width), ..responsive(ResponsiveKind::Last) }),
        action_view_item_provider: Some(provider),
        ..IToolBarOptions::default()
    });
    let primary_actions = vec![env.action("primary.a", "Primary A"), env.action("primary.b", "Primary B")];
    let secondary_actions = vec![env.action("secondary", "Secondary")];

    toolbar.set_actions(&primary_actions, &secondary_actions);
    let after_initial_layout = env.ids(&toolbar);
    toolbar.relayout();
    let after_repeated_layout = env.ids(&toolbar);

    available_width.set(136.0);
    toolbar.relayout();
    let after_growing = env.ids(&toolbar);

    available_width.set(128.0);
    toolbar.relayout();
    toolbar.relayout();
    let after_shrinking_again = env.ids(&toolbar);

    assert_eq!(after_initial_layout, strings(&["primary.a", ToggleMenuAction::ID]));
    assert_eq!(after_repeated_layout, strings(&["primary.a", ToggleMenuAction::ID]));
    assert_eq!(after_growing, strings(&["primary.a", "primary.b", ToggleMenuAction::ID]));
    assert_eq!(after_shrinking_again, strings(&["primary.a", ToggleMenuAction::ID]));
    assert_eq!(render_counts.borrow().get("primary.b"), Some(&2));
}

#[test]
fn ignores_the_responsive_minimum_when_measuring_an_action_that_will_stop_shrinking() {
    let env = env();
    let widths: HashMap<&'static str, f64> = HashMap::from([("primary.a", 22.0), ("primary.b", 48.0)]);
    let available_width = Rc::new(Cell::new(90.0));

    let toolbar = env.toolbar(IToolBarOptions {
        responsive_behavior: Some(IToolBarResponsiveBehaviorOptions { min_items: Some(1), action_min_width: Some(48.0), get_available_width: width_source(&available_width), ..responsive(ResponsiveKind::Last) }),
        action_view_item_provider: Some(env.fixed_widths(&widths)),
        ..IToolBarOptions::default()
    });
    let primary_actions = vec![env.action("primary.a", "Primary A"), env.action("primary.b", "Primary B")];
    let secondary_actions = vec![env.action("secondary", "Secondary")];

    toolbar.set_actions(&primary_actions, &secondary_actions);
    let before_growing = env.ids(&toolbar);

    available_width.set(110.0);
    toolbar.relayout();
    let after_growing = env.ids(&toolbar);

    assert_eq!(before_growing, strings(&["primary.a", ToggleMenuAction::ID]));
    assert_eq!(after_growing, strings(&["primary.a", "primary.b", ToggleMenuAction::ID]));
}

#[test]
fn restores_a_hidden_action_after_a_visible_action_shrinks() {
    let env = env();
    let available_width = 128.0;

    let provider: IActionViewItemProvider = {
        let ui = env.ui.clone();
        Rc::new(move |action: &ActionRef, _| match action.id().as_str() {
            "primary.a" => Some(FixedWidthActionViewItem::new(&ui, action, 100.0, true)),
            "primary.b" => Some(FixedWidthActionViewItem::new(&ui, action, 48.0, true)),
            _ => None,
        })
    };
    let toolbar = env.toolbar(IToolBarOptions {
        responsive_behavior: Some(IToolBarResponsiveBehaviorOptions { min_items: Some(1), action_min_width: Some(48.0), get_available_width: Some(Rc::new(move || available_width)), ..responsive(ResponsiveKind::Last) }),
        action_view_item_provider: Some(provider),
        ..IToolBarOptions::default()
    });
    let primary_actions = vec![env.action("primary.a", "Primary A"), env.action("primary.b", "Primary B")];
    let secondary_actions = vec![env.action("secondary", "Secondary")];

    toolbar.set_actions(&primary_actions, &secondary_actions);
    let before_shrinking = env.ids(&toolbar);

    let primary_a_item = query_all(&env.ui.tree(), toolbar.get_element(), &["action-item"])[0];
    {
        // `primaryAItem.style.width = '48px'`
        let mut tree = env.ui.tree_mut();
        let bounds = tree.bounds(primary_a_item);
        tree.set_bounds(primary_a_item, Rect::new(bounds.x0, bounds.y0, bounds.x0 + 48.0, bounds.y1));
    }
    toolbar.relayout();
    let after_shrinking = env.ids(&toolbar);

    assert_eq!(before_shrinking, strings(&["primary.a", ToggleMenuAction::ID]));
    assert_eq!(after_shrinking, strings(&["primary.a", "primary.b", ToggleMenuAction::ID]));
}

#[test]
fn uses_get_available_width_override_instead_of_the_element_width() {
    let env = env();
    let widths: HashMap<&'static str, f64> = HashMap::from([("a", 50.0), ("b", 50.0), ("c", 50.0), (ToggleMenuAction::ID, 22.0)]);

    let available_width = Rc::new(Cell::new(200.0));

    let toolbar = env.toolbar(IToolBarOptions {
        responsive_behavior: Some(IToolBarResponsiveBehaviorOptions { min_items: Some(1), action_min_width: Some(22.0), get_available_width: width_source(&available_width), ..responsive(ResponsiveKind::Last) }),
        action_view_item_provider: Some(env.fixed_widths(&widths)),
        ..IToolBarOptions::default()
    });

    // Force the element's bounding rect to a value that would otherwise hide everything
    // to prove the toolbar uses the override callback instead.
    env.pin_element_width(&toolbar, 0.0);

    let actions = vec![env.action("a", "A"), env.action("b", "B"), env.action("c", "C")];

    toolbar.set_actions(&actions, &[]);

    // availableWidth = 200 is plenty for all 3 actions; the element's 0 width is ignored
    assert_eq!(toolbar.get_items_length(), 3);
    assert!(!toolbar.has_overflow());

    available_width.set(60.0);
    toolbar.relayout();

    // availableWidth shrank: actions overflow into the toggle menu
    assert_eq!(toolbar.get_item_action(toolbar.get_items_length() - 1).map(|a| a.id()).as_deref(), Some(ToggleMenuAction::ID));
    assert!(toolbar.has_overflow());

    available_width.set(200.0);
    toolbar.relayout();

    assert_eq!(toolbar.get_items_length(), 3);
    assert!(!toolbar.has_overflow());
}

#[test]
fn ignores_non_rendered_actions_when_deciding_to_overflow() {
    let env = env();
    let hidden_action_ids: HashSet<&'static str> = HashSet::from(["hidden.a", "hidden.b", "hidden.c"]);
    let provider: IActionViewItemProvider = {
        let ui = env.ui.clone();
        Rc::new(move |action: &ActionRef, _| Some(FixedWidthActionViewItem::new(&ui, action, 22.0, !hidden_action_ids.contains(action.id().as_str()))))
    };
    let toolbar = env.toolbar(IToolBarOptions {
        responsive_behavior: Some(IToolBarResponsiveBehaviorOptions {
            min_items: Some(1),
            action_min_width: Some(48.0),
            get_action_min_width: Some(Rc::new(|_: &ActionRef| Some(22.0))),
            get_available_width: Some(Rc::new(|| 60.0)),
            ..responsive(ResponsiveKind::All)
        }),
        action_view_item_provider: Some(provider),
        ..IToolBarOptions::default()
    });
    toolbar.set_actions(
        &[env.action("hidden.a", "Hidden A"), env.action("hidden.b", "Hidden B"), env.action("visible.a", "Visible A"), env.action("hidden.c", "Hidden C"), env.action("visible.b", "Visible B")],
        &[],
    );

    let visible_actions: Vec<String> = (0..toolbar.get_items_length()).filter(|index| !env.display_none(&toolbar, *index)).map(|index| toolbar.get_item_action(index).unwrap().id()).collect();
    assert_eq!((visible_actions, toolbar.has_overflow()), (strings(&["visible.a", "visible.b"]), false));
}

#[test]
fn can_keep_compact_actions_visible_instead_of_overflowing() {
    let env = env();
    let toolbar = env.toolbar(IToolBarOptions {
        responsive_behavior: Some(IToolBarResponsiveBehaviorOptions {
            min_items: Some(1),
            action_min_width: Some(22.0),
            get_available_width: Some(Rc::new(|| 50.0)),
            allow_overflow: Some(AllowOverflow::Always(false)),
            ..responsive(ResponsiveKind::All)
        }),
        action_view_item_provider: Some(env.all_fixed(22.0)),
        ..IToolBarOptions::default()
    });

    toolbar.set_actions(&[env.action("a", "A"), env.action("b", "B"), env.action("c", "C")], &[]);

    assert_eq!((env.ids(&toolbar), toolbar.has_overflow()), (strings(&["a", "b", "c"]), false));
}

#[test]
fn allows_overflow_only_after_compact_actions_still_exceed_the_width() {
    let env = env();
    let available_width = Rc::new(Cell::new(100.0));
    let all_compact = Rc::new(Cell::new(false));
    let toolbar = env.toolbar(IToolBarOptions {
        responsive_behavior: Some(IToolBarResponsiveBehaviorOptions {
            min_items: Some(1),
            action_min_width: Some(22.0),
            get_available_width: width_source(&available_width),
            allow_overflow: Some(AllowOverflow::When({
                let all_compact = all_compact.clone();
                Rc::new(move || all_compact.get())
            })),
            ..responsive(ResponsiveKind::All)
        }),
        action_view_item_provider: Some(env.all_fixed(22.0)),
        ..IToolBarOptions::default()
    });
    toolbar.set_actions(&[env.action("a", "A"), env.action("b", "B"), env.action("c", "C")], &[]);

    available_width.set(50.0);
    toolbar.relayout();
    let before_compact = toolbar.has_overflow();

    all_compact.set(true);
    toolbar.relayout();
    let after_compact = toolbar.has_overflow();

    assert_eq!((before_compact, after_compact), (false, true));
}

#[test]
fn keeps_the_configured_minimum_actions_visible_across_repeated_relayouts() {
    let env = env();
    let available_width = Rc::new(Cell::new(300.0));
    let toolbar = env.toolbar(IToolBarOptions {
        responsive_behavior: Some(IToolBarResponsiveBehaviorOptions { min_items: Some(2), action_min_width: Some(22.0), get_available_width: width_source(&available_width), ..responsive(ResponsiveKind::Last) }),
        action_view_item_provider: Some(env.all_fixed(60.0)),
        ..IToolBarOptions::default()
    });
    toolbar.set_actions(&[env.action("attach", "Attach"), env.action("agent", "Agent"), env.action("model", "Model"), env.action("settings", "Settings")], &[]);

    let mut states: Vec<Vec<String>> = Vec::new();
    for width in [100.0, 300.0, 100.0, 300.0, 100.0, 300.0] {
        available_width.set(width);
        toolbar.relayout();
        states.push(env.ids(&toolbar));
    }

    assert_eq!(
        states,
        vec![
            strings(&["attach", "agent", ToggleMenuAction::ID]),
            strings(&["attach", "agent", "model", "settings"]),
            strings(&["attach", "agent", ToggleMenuAction::ID]),
            strings(&["attach", "agent", "model", "settings"]),
            strings(&["attach", "agent", ToggleMenuAction::ID]),
            strings(&["attach", "agent", "model", "settings"]),
        ]
    );
}

#[test]
fn overflows_the_minimum_actions_when_their_width_is_not_reserved_and_does_not_fit() {
    let env = env();
    let available_width = Rc::new(Cell::new(300.0));
    let toolbar = env.toolbar(IToolBarOptions {
        responsive_behavior: Some(IToolBarResponsiveBehaviorOptions {
            min_items: Some(2),
            action_min_width: Some(22.0),
            reserve_min_width: Some(false),
            get_available_width: width_source(&available_width),
            ..responsive(ResponsiveKind::Last)
        }),
        action_view_item_provider: Some(env.all_fixed(60.0)),
        ..IToolBarOptions::default()
    });
    toolbar.set_actions(&[env.action("attach", "Attach"), env.action("agent", "Agent"), env.action("model", "Model"), env.action("settings", "Settings")], &[]);

    let mut states: Vec<Vec<String>> = Vec::new();
    for width in [100.0, 40.0, 100.0, 300.0] {
        available_width.set(width);
        toolbar.relayout();
        states.push(env.ids(&toolbar));
    }

    assert_eq!(
        (states, [toolbar.container_min_width(), toolbar.element_min_width()]),
        (
            vec![
                strings(&["attach", "agent", ToggleMenuAction::ID]),
                strings(&[ToggleMenuAction::ID]),
                strings(&["attach", "agent", ToggleMenuAction::ID]),
                strings(&["attach", "agent", "model", "settings"]),
            ],
            [None, None], // ['', '']
        )
    );
}

#[test]
fn uses_overflow_specific_proxy_actions() {
    let env = env();
    let runs: Rc<RefCell<Vec<String>>> = Rc::default();
    let overflow_anchor: Rc<Cell<Option<NodeId>>> = Rc::default();
    let toolbar = env.toolbar(IToolBarOptions {
        responsive_behavior: Some(IToolBarResponsiveBehaviorOptions {
            min_items: Some(1),
            action_min_width: Some(22.0),
            get_available_width: Some(Rc::new(|| 50.0)),
            get_overflow_action: Some({
                let (runs, overflow_anchor) = (runs.clone(), overflow_anchor.clone());
                Rc::new(move |action: &ActionRef, get_anchor: Rc<dyn Fn() -> Option<NodeId>>| {
                    let (runs, overflow_anchor, id) = (runs.clone(), overflow_anchor.clone(), action.id());
                    // `{ ...action, run: () => { ... } }`
                    to_action(ToActionProps {
                        id: id.clone(),
                        label: action.label(),
                        tooltip: None,
                        enabled: Some(action.enabled()),
                        checked: None,
                        class: action.class(),
                        alt: None,
                        run: Rc::new(move |_| {
                            overflow_anchor.set(get_anchor());
                            runs.borrow_mut().push(format!("overflow:{id}"));
                            Dynamic::Undefined.into()
                        }),
                    })
                })
            }),
            ..responsive(ResponsiveKind::All)
        }),
        action_view_item_provider: Some(env.all_fixed(22.0)),
        ..IToolBarOptions::default()
    });
    let original = |id: &'static str| -> ActionRef {
        let runs = runs.clone();
        env.action_with(id, &id.to_uppercase(), Some(Rc::new(move |_| {
            runs.borrow_mut().push(format!("original:{id}"));
            Dynamic::Undefined.into()
        })))
    };
    toolbar.set_actions(&[original("a"), original("b"), original("c")], &[]);

    let overflow_action = toolbar.get_item_action(toolbar.get_items_length() - 1).unwrap();
    assert_eq!(overflow_action.id(), ToggleMenuAction::ID);
    let menu_actions = overflow_action.as_any().downcast_ref::<ToggleMenuAction>().unwrap().menu_actions();
    let _ = menu_actions[0].run(&[]);
    je_base::runtime::run_microtasks();
    let overflow_button = toolbar.get_item_view_item(toolbar.get_items_length() - 1).and_then(|item| item.base().element());

    assert_eq!((runs.borrow().clone(), overflow_anchor.get().is_some() && overflow_anchor.get() == overflow_button), (strings(&["overflow:b"]), true));
}

#[test]
fn keeps_the_trailing_separator_after_overflow_while_resizing() {
    let env = env();
    let baseline = env.toolbar(IToolBarOptions { trailing_separator: true, ..IToolBarOptions::default() });
    baseline.set_actions(&[env.action("baseline", "Baseline")], &[]);
    let separator_width = baseline.get_item_width(1);
    let available_width = Rc::new(Cell::new(300.0));
    let toolbar = env.toolbar(IToolBarOptions {
        trailing_separator: true,
        responsive_behavior: Some(IToolBarResponsiveBehaviorOptions { min_items: Some(0), get_available_width: width_source(&available_width), ..responsive(ResponsiveKind::All) }),
        ..IToolBarOptions::default()
    });
    toolbar.set_actions(&[env.action("new", "New Chat"), env.action("settings", "Open Customizations")], &[env.action("history", "History")]);

    let mut states = Vec::new();
    for width in [24.0, 300.0, 24.0] {
        available_width.set(width);
        toolbar.relayout();
        let overflow = (0..toolbar.get_items_length()).filter_map(|index| toolbar.get_item_action(index)).find_map(|action| menu_action_ids(&action));
        states.push((env.ids(&toolbar), overflow, toolbar.get_item_width(toolbar.get_items_length() - 1)));
    }
    assert_eq!(
        states,
        vec![
            (strings(&[ToggleMenuAction::ID, Separator::ID]), Some(strings(&["new", "settings", Separator::ID, "history"])), separator_width),
            (strings(&["new", "settings", ToggleMenuAction::ID, Separator::ID]), Some(strings(&["history"])), separator_width),
            (strings(&[ToggleMenuAction::ID, Separator::ID]), Some(strings(&["new", "settings", Separator::ID, "history"])), separator_width),
        ]
    );
}

#[test]
fn keeps_overflow_keyboard_reachable_when_every_primary_action_moves_into_it() {
    let env = env();
    let available_width = Rc::new(Cell::new(300.0));
    let toolbar = env.toolbar(IToolBarOptions {
        responsive_behavior: Some(IToolBarResponsiveBehaviorOptions { min_items: Some(0), get_available_width: width_source(&available_width), ..responsive(ResponsiveKind::All) }),
        ..IToolBarOptions::default()
    });
    toolbar.set_actions(&[env.action("new", "New Chat")], &[env.action("history", "History")]);
    available_width.set(24.0);
    toolbar.relayout();
    // `querySelector('[aria-haspopup="true"]')?.tabIndex === 0`
    let popup = env.popup_nodes(toolbar.get_element());
    assert_eq!(popup.len(), 1);
    assert_eq!(toolbar.get_item_view_item(0).map(|item| item.tab_stops()), Some(1));
    assert_eq!(toolbar.get_item_view_item(0).and_then(|item| item.base().element()), popup.first().copied());
}

#[test]
fn moves_focus_to_overflow_when_the_focused_primary_action_no_longer_fits() {
    let env = env();
    let available_width = Rc::new(Cell::new(300.0));
    let toolbar = env.toolbar(IToolBarOptions {
        responsive_behavior: Some(IToolBarResponsiveBehaviorOptions { min_items: Some(0), overflow_from: Some(OverflowFrom::Start), get_available_width: width_source(&available_width), ..responsive(ResponsiveKind::All) }),
        ..IToolBarOptions::default()
    });
    toolbar.set_actions(&[env.action("new", "New Chat"), env.action("settings", "Settings")], &[env.action("history", "History")]);
    toolbar.focus(None);
    available_width.set(24.0);
    toolbar.relayout();
    let popup = env.popup_nodes(toolbar.get_element()).first().copied();
    assert!(popup.is_some());
    assert_eq!(env.ui.focused(), popup);
    available_width.set(300.0);
    toolbar.relayout();
    let popup = env.popup_nodes(toolbar.get_element()).first().copied();
    assert_eq!((env.ui.focused() == popup, env.tab_stops(&toolbar)), (true, 1));
}

#[test]
fn reserves_minimum_items_at_the_retained_end_when_overflowing_from_the_start() {
    let env = env();
    let toolbar = env.toolbar(IToolBarOptions {
        responsive_behavior: Some(IToolBarResponsiveBehaviorOptions {
            overflow_from: Some(OverflowFrom::Start),
            min_items: Some(1),
            get_action_min_width: Some(Rc::new(|action: &ActionRef| Some(if action.id() == "new" { 36.0 } else { 10.0 }))),
            get_available_width: Some(Rc::new(|| 1.0)),
            ..responsive(ResponsiveKind::All)
        }),
        ..IToolBarOptions::default()
    });
    toolbar.set_actions(&[env.action("new", "New Chat"), env.action("settings", "Settings")], &[]);
    assert_eq!((env.ids(&toolbar), toolbar.element_min_width()), (strings(&["settings", ToggleMenuAction::ID]), Some(38.0))); // '38px'
}

/// O corpo de `restores grouped actions without confusing the trailing separator (overflowFrom, secondary)`.
fn restores_grouped_actions_without_confusing_the_trailing_separator(overflow_from: OverflowFrom, secondary: bool) {
    let env = env();
    let available_width = Rc::new(Cell::new(300.0));
    let toolbar = env.toolbar(IToolBarOptions {
        trailing_separator: true,
        responsive_behavior: Some(IToolBarResponsiveBehaviorOptions { min_items: Some(0), overflow_from: Some(overflow_from), get_available_width: width_source(&available_width), ..responsive(ResponsiveKind::All) }),
        ..IToolBarOptions::default()
    });
    let separator: ActionRef = Separator::new();
    let history = env.action("history", "History");
    toolbar.set_actions(&[env.action("a", "A"), separator, env.action("b", "B")], if secondary { std::slice::from_ref(&history) } else { &[] });

    let mut states = Vec::new();
    for width in [300.0, 48.0, 61.0, 72.0, 300.0, 48.0, 300.0] {
        available_width.set(width);
        toolbar.relayout();
        let items: Vec<ActionRef> = (0..toolbar.get_items_length()).map(|index| toolbar.get_item_action(index).unwrap()).collect();
        let overflow = items.iter().find_map(menu_action_ids).unwrap_or_default();
        states.push((items.iter().map(|action| action.id()).collect::<Vec<_>>(), overflow));
    }
    let divider = [Separator::ID];
    let overflow_tail: Vec<&str> = if secondary { vec![Separator::ID, "history"] } else { vec![] };
    let (visible_action, hidden_action) = if overflow_from == OverflowFrom::Start { ("b", "a") } else { ("a", "b") };
    let state = |items: Vec<&str>, overflow: Vec<&str>| (strings(&items), strings(&overflow));
    let expanded = state(["a", Separator::ID, "b"].into_iter().chain(secondary.then_some(ToggleMenuAction::ID)).chain(divider).collect(), if secondary { vec!["history"] } else { vec![] });
    let partial = state([visible_action, ToggleMenuAction::ID].into_iter().chain(divider).collect(), [hidden_action].into_iter().chain(overflow_tail.iter().copied()).collect());
    let collapsed = state([ToggleMenuAction::ID].into_iter().chain(divider).collect(), ["a", Separator::ID, "b"].into_iter().chain(overflow_tail.iter().copied()).collect());
    assert_eq!(states, vec![expanded.clone(), collapsed.clone(), partial.clone(), partial, expanded.clone(), collapsed, expanded]);
}

#[test]
fn restores_grouped_actions_without_confusing_the_trailing_separator_overflow_from_start_secondary_false() {
    restores_grouped_actions_without_confusing_the_trailing_separator(OverflowFrom::Start, false);
}

#[test]
fn restores_grouped_actions_without_confusing_the_trailing_separator_overflow_from_start_secondary_true() {
    restores_grouped_actions_without_confusing_the_trailing_separator(OverflowFrom::Start, true);
}

#[test]
fn restores_grouped_actions_without_confusing_the_trailing_separator_overflow_from_end_secondary_false() {
    restores_grouped_actions_without_confusing_the_trailing_separator(OverflowFrom::End, false);
}

#[test]
fn restores_grouped_actions_without_confusing_the_trailing_separator_overflow_from_end_secondary_true() {
    restores_grouped_actions_without_confusing_the_trailing_separator(OverflowFrom::End, true);
}

/// O corpo de `preserves split-button dropdown focus across responsive relayouts (kind, overflowFrom)`.
fn preserves_split_button_dropdown_focus_across_responsive_relayouts(kind: ResponsiveKind, overflow_from: OverflowFrom) {
    let env = env();
    let available_width = Rc::new(Cell::new(300.0));
    let split_action = env.action("split", "Split Button");
    let dropdown_action = env.action("dropdown", "Dropdown Action");
    let provider: IActionViewItemProvider = {
        let (ui, split_action) = (env.ui.clone(), split_action.clone());
        Rc::new(move |action: &ActionRef, _| {
            if Rc::ptr_eq(action, &split_action) {
                let item: Rc<dyn IActionViewItem> = ActionWithDropdownActionViewItem::new(
                    &ui,
                    None,
                    action.clone(),
                    IActionWithDropdownActionViewItemOptions {
                        base: IActionViewItemOptions { icon: Some(true), label: Some(false), ..IActionViewItemOptions::default() },
                        menu_actions_or_provider: MenuActions::Actions(vec![dropdown_action.clone()]),
                        menu_action_class_names: Vec::new(),
                        keybinding_provider: None,
                    },
                    Rc::new(NoopContextMenuProvider),
                );
                Some(item)
            } else {
                Some(FixedWidthActionViewItem::new(&ui, action, 22.0, true))
            }
        })
    };
    let toolbar = env.toolbar(IToolBarOptions {
        responsive_behavior: Some(IToolBarResponsiveBehaviorOptions {
            enabled: true,
            kind,
            overflow_from: Some(overflow_from),
            min_items: Some(2),
            get_action_min_width: Some({
                let split_action = split_action.clone();
                Rc::new(move |action: &ActionRef| Some(if Rc::ptr_eq(action, &split_action) { 36.0 } else { 22.0 }))
            }),
            get_available_width: width_source(&available_width),
            ..IToolBarResponsiveBehaviorOptions::default()
        }),
        action_view_item_provider: Some(provider),
        ..IToolBarOptions::default()
    });
    let mut actions: Vec<ActionRef> = ["a", "b", "c"].into_iter().map(|id| env.action(id, id)).collect();
    actions.insert(if overflow_from == OverflowFrom::Start { 2 } else { 1 }, split_action.clone());
    toolbar.set_actions(&actions, &[]);
    let split_index = actions.iter().position(|action| Rc::ptr_eq(action, &split_action)).unwrap();
    toolbar.focus(Some(0));
    toolbar.focus(Some(split_index));
    env.ui.key_input(true, KeyData { key_code: KeyCode::RightArrow, modifiers: Modifiers::default() });
    let dropdown = env.popup_nodes(toolbar.get_item_element(split_index).unwrap())[0];
    assert_eq!(env.ui.focused(), Some(dropdown));

    let focus_events = Rc::new(Cell::new(0usize));
    env.store.add(env.ui.listen(toolbar.get_element(), EventKind::Focus, true, {
        let focus_events = focus_events.clone();
        move |_| focus_events.set(focus_events.get() + 1)
    }));
    let mut states = Vec::new();
    for width in [1.0, 1.0, 300.0, 1.0, 1.0, 300.0] {
        let unchanged_width = available_width.get() == width;
        let previous_focus_events = focus_events.get();
        available_width.set(width);
        toolbar.relayout();
        let root = env.ui.root();
        states.push((
            env.ui.tree().is_alive(dropdown) && env.ui.tree().is_ancestor(root, dropdown),
            env.ui.focused() == Some(dropdown),
            env.tab_stops(&toolbar),
            toolbar.get_items_length(),
            if unchanged_width { focus_events.get() - previous_focus_events } else { 0 },
        ));
    }
    let collapsed = (true, true, 1, 3, 0);
    let expanded = (true, true, 1, 4, 0);
    assert_eq!(states, vec![collapsed, collapsed, expanded, collapsed, collapsed, expanded]);
}

#[test]
fn preserves_split_button_dropdown_focus_across_responsive_relayouts_all_start() {
    preserves_split_button_dropdown_focus_across_responsive_relayouts(ResponsiveKind::All, OverflowFrom::Start);
}

#[test]
fn preserves_split_button_dropdown_focus_across_responsive_relayouts_all_end() {
    preserves_split_button_dropdown_focus_across_responsive_relayouts(ResponsiveKind::All, OverflowFrom::End);
}

#[test]
fn preserves_split_button_dropdown_focus_across_responsive_relayouts_last_end() {
    preserves_split_button_dropdown_focus_across_responsive_relayouts(ResponsiveKind::Last, OverflowFrom::End);
}

/// O corpo de `progressively overflows from the start and restores in reverse order (secondary, divider)`.
fn progressively_overflows_from_the_start_and_restores_in_reverse_order(secondary: bool, trailing_separator: bool) {
    let env = env();
    let available_width = Rc::new(Cell::new(300.0));
    let toolbar = env.toolbar(IToolBarOptions {
        trailing_separator,
        responsive_behavior: Some(IToolBarResponsiveBehaviorOptions {
            min_items: Some(0),
            overflow_from: Some(OverflowFrom::Start),
            get_action_min_width: Some(Rc::new(|action: &ActionRef| Some(if action.id() == "new" { 36.0 } else { 22.0 }))),
            get_available_width: width_source(&available_width),
            ..responsive(ResponsiveKind::All)
        }),
        ..IToolBarOptions::default()
    });
    let actions: Vec<ActionRef> = ["new", "settings", "extra"].into_iter().map(|id| env.action(id, id)).collect();
    let secondary_actions: Vec<ActionRef> = if secondary { vec![env.action("history", "History")] } else { Vec::new() };
    toolbar.set_actions(&actions, &secondary_actions);
    let snapshot = || {
        let overflow = (0..toolbar.get_items_length()).filter_map(|index| toolbar.get_item_action(index)).find_map(|action| menu_action_ids(&action)).unwrap_or_default();
        (env.ids(&toolbar), overflow, env.tab_stops(&toolbar))
    };
    let mut states = Vec::new();
    for width in [90.0, 24.0, 70.0, 90.0, 300.0, 24.0, 300.0] {
        available_width.set(width);
        toolbar.relayout();
        states.push(snapshot());
    }
    available_width.set(24.0);
    toolbar.set_actions(&actions[..2], &secondary_actions);
    states.push(snapshot());
    let divider: Vec<&str> = if trailing_separator { vec![Separator::ID] } else { vec![] };
    let overflow_tail: Vec<&str> = if secondary { vec![Separator::ID, "history"] } else { vec![] };
    let state = |items: Vec<&str>, overflow: Vec<&str>| (strings(&items), strings(&overflow), 1usize);
    let collapsed = state([ToggleMenuAction::ID].into_iter().chain(divider.iter().copied()).collect(), ["new", "settings", "extra"].into_iter().chain(overflow_tail.iter().copied()).collect());
    let partial = state(["settings", "extra", ToggleMenuAction::ID].into_iter().chain(divider.iter().copied()).collect(), ["new"].into_iter().chain(overflow_tail.iter().copied()).collect());
    let expanded = state(["new", "settings", "extra"].into_iter().chain(secondary.then_some(ToggleMenuAction::ID)).chain(divider.iter().copied()).collect(), if secondary { vec!["history"] } else { vec![] });
    assert_eq!(
        states,
        vec![
            partial.clone(),
            collapsed.clone(),
            state(["extra", ToggleMenuAction::ID].into_iter().chain(divider.iter().copied()).collect(), ["new", "settings"].into_iter().chain(overflow_tail.iter().copied()).collect()),
            partial,
            expanded.clone(),
            collapsed,
            expanded,
            state([ToggleMenuAction::ID].into_iter().chain(divider.iter().copied()).collect(), ["new", "settings"].into_iter().chain(overflow_tail.iter().copied()).collect()),
        ]
    );
}

#[test]
fn progressively_overflows_from_the_start_and_restores_in_reverse_order_secondary_false_divider_false() {
    progressively_overflows_from_the_start_and_restores_in_reverse_order(false, false);
}

#[test]
fn progressively_overflows_from_the_start_and_restores_in_reverse_order_secondary_false_divider_true() {
    progressively_overflows_from_the_start_and_restores_in_reverse_order(false, true);
}

#[test]
fn progressively_overflows_from_the_start_and_restores_in_reverse_order_secondary_true_divider_false() {
    progressively_overflows_from_the_start_and_restores_in_reverse_order(true, false);
}

#[test]
fn progressively_overflows_from_the_start_and_restores_in_reverse_order_secondary_true_divider_true() {
    progressively_overflows_from_the_start_and_restores_in_reverse_order(true, true);
}
