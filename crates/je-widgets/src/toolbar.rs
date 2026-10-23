//! Porte de `src/vs/base/browser/ui/toolbar/toolbar.ts` (e do `toolbar.css`): a [`ToolBar`], uma
//! [`ActionBar`] de ações primárias com um menu "mais" ([`ToggleMenuAction`]) para as secundárias, e, com o
//! comportamento responsivo ligado, o transbordo das primárias para esse menu quando a largura não basta.
//!
//! # API para quem consome (workbench, painéis, widgets)
//!
//! - [`ToolBar::new`]`(ui, container, context_menu_provider, options)`; [`ToolBar::set_actions`] troca as ações
//!   (as secundárias vão ao menu "mais"). `get_item_*`, `focus`, `has_overflow`, `set_context`,
//!   `set_action_runner`, `set_aria_label` e [`ToolBar::on_did_change_dropdown_visibility`] são os do original.
//! - **Sem `ResizeObserver`.** O original observa o elemento e chama `updateActions(getAvailableWidth())` a cada
//!   mudança de tamanho. O `je-ui` não notifica tamanho: quem redimensiona o contêiner chama
//!   [`ToolBar::relayout`] (o mesmo que o observador faria, e o que o original já oferece para mudanças de
//!   largura dos itens), e [`ActionBar::layout`] (em [`ToolBar::action_bar`]) para refazer a posição vertical.
//!   O `observedElement` das opções responsivas não existe por isso.
//! - **`protected actionBar`.** [`ToolBar::action_bar`] o expõe (o `TestToolBar` do original só faz isso).
//!
//! Como o DOM e o CSS viram [`je_ui`]: `div.monaco-toolbar` (`height: 100%`) é o [`ToolBar::get_element`], com o
//! `ActionBar` dentro. As classes `responsive`, `responsive-all`/`responsive-last`, `has-overflow` (na
//! `monaco-action-bar`) e `toolbar-shrinkable-item` (no `li`) são as do DOM. O `style` do elemento e do
//! contêiner que o original escreve vira estado lido por [`ToolBar::element_min_width`],
//! [`ToolBar::container_min_width`] e [`ToolBar::action_min_width_property`] (`--vscode-toolbar-action-min-width`);
//! `display: none` de um `li` é `NodeStyle::visible == false`.
//!
//! **Divergência (flex-shrink).** O `ActionBar` do `je-widgets` não tem motor de layout: cada `li` tem o tamanho
//! natural do item, e o `min-width`/`flex-shrink` que o CSS responsivo dá aos itens (para um rótulo encolher até a
//! largura mínima) não é modelado. A decisão de transbordar usa as mesmas larguras mínimas do original e não muda;
//! o que muda é que um item largo não encolhe visualmente antes de sair, e que `getUnshrunkItemWidth` é a própria
//! largura do `li`. A `min-width` reservada (`reserveMinWidth`) é só estado para quem posiciona o contêiner.
//!
//! Fica de fora: `allowContextMenu` já não muda nada no `ActionBar` (ver o módulo dele).

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use je_base::actions::{Action, ActionRef, IAction, IActionRunner, Separator, SubmenuAction};
use je_base::dynamic::Dynamic;
use je_base::event::{Event as BaseEvent, EventMultiplexer};
use je_base::keybindings::ResolvedKeybinding;
use je_base::layout::AnchorAlignment;
use je_base::lifecycle::{Disposable, DisposableMap, IDisposable};
use je_base::nls::localize;
use je_base::promise::Promise;
use je_base::themables::ThemeIcon;
use je_ui::{NodeId, Role, Ui};

use crate::action_bar::{ActionBar, ActionsOrientation, IActionBarOptions, IActionOptions, IActionViewItemProvider};
use crate::action_view_items::{ActionBarStyles, IActionViewItem, IActionViewItemOptions};
use crate::context_view_api::{IContextMenuProvider, IContextViewCloseAnimation};
use crate::dom::{create_el, is_ancestor};
use crate::dropdown::IActionProvider;
use crate::dropdown_action_view_item::{DropdownMenuActionViewItem, IAnchorAlignmentProvider, IDropdownMenuActionViewItemOptions, IKeybindingProvider, MenuActions};
use crate::hover_api::{IHoverDelegate, create_instant_hover_delegate};

const ACTION_MIN_WIDTH: f64 = 20.0; /* 20px codicon */
const ACTION_PADDING: f64 = 4.0; /* 4px padding */

/// `kind: 'last' | 'all'`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ResponsiveKind {
    /// Só o último item encolhe.
    Last,
    /// Todos encolhem por igual.
    #[default]
    All,
}

/// `overflowFrom: 'start' | 'end'`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum OverflowFrom {
    Start,
    #[default]
    End,
}

/// `allowOverflow?: boolean | (() => boolean)`.
#[derive(Clone)]
pub enum AllowOverflow {
    Always(bool),
    When(Rc<dyn Fn() -> bool>),
}

/// `getOverflowAction`: troca uma ação só enquanto ela está no menu de transbordo; recebe a ação e o `getAnchor`
/// do botão "mais".
pub type GetOverflowAction = Rc<dyn Fn(&ActionRef, Rc<dyn Fn() -> Option<NodeId>>) -> ActionRef>;

/// `getActionMinWidth`: a largura mínima de uma ação em px, ou nada para a padrão.
pub type GetActionMinWidth = Rc<dyn Fn(&ActionRef) -> Option<f64>>;

/// `getSecondaryActionsPosition`: o índice do botão das ações secundárias entre as primárias (um `number`).
pub type GetSecondaryActionsPosition = Rc<dyn Fn(&[ActionRef]) -> f64>;

/// `IToolBarResponsiveBehaviorOptions`.
#[derive(Clone, Default)]
pub struct IToolBarResponsiveBehaviorOptions {
    pub enabled: bool,
    pub kind: ResponsiveKind,
    pub min_items: Option<usize>,
    pub overflow_from: Option<OverflowFrom>,
    pub action_min_width: Option<f64>,
    pub get_action_min_width: Option<GetActionMinWidth>,
    pub allow_overflow: Option<AllowOverflow>,
    pub get_overflow_action: Option<GetOverflowAction>,
    pub get_available_width: Option<Rc<dyn Fn() -> f64>>,
    pub reserve_min_width: Option<bool>,
}

/// `IToolBarOptions`.
#[derive(Clone, Default)]
pub struct IToolBarOptions {
    pub orientation: Option<ActionsOrientation>,
    pub action_view_item_provider: Option<IActionViewItemProvider>,
    pub aria_label: Option<String>,
    pub get_key_binding: Option<IKeybindingProvider>,
    /// Devolve o índice em que o botão das ações secundárias entra entre as primárias (um `number`, por isso
    /// `f64`: valores fora da faixa ou não finitos se ajustam). Padrão: depois de todas as primárias.
    pub get_secondary_actions_position: Option<GetSecondaryActionsPosition>,
    pub action_runner: Option<Rc<dyn IActionRunner>>,
    pub toggle_menu_title: Option<String>,
    pub anchor_alignment_provider: Option<Rc<dyn Fn() -> AnchorAlignment>>,
    pub dropdown_menu_class_name: Option<String>,
    pub dropdown_menu_close_animation: Option<IContextViewCloseAnimation>,
    pub render_dropdown_as_child_element: bool,
    pub more_icon: Option<ThemeIcon>,
    pub allow_context_menu: bool,
    pub skip_telemetry: bool,
    pub hover_delegate: Option<Rc<dyn IHoverDelegate>>,
    pub trailing_separator: bool,
    /// Itens primários alternados ganham o fundo de `actionBar.toggledBackground`.
    pub highlight_toggled_items: bool,
    /// Ícone nas ações (padrão: `true`).
    pub icon: Option<bool>,
    /// Rótulo nas ações (padrão: `false`).
    pub label: Option<bool>,
    /// O comportamento responsivo do grupo primário.
    pub responsive_behavior: Option<IToolBarResponsiveBehaviorOptions>,
    /// As cores do CSS (não está no original). Sem valor, [`ActionBarStyles::default`].
    pub styles: Option<Rc<ActionBarStyles>>,
}

/// A identidade de uma ação (o `===` do original).
fn same_action(a: &ActionRef, b: &ActionRef) -> bool {
    std::ptr::addr_eq(Rc::as_ptr(a), Rc::as_ptr(b))
}

fn contains_action(list: &[ActionRef], action: &ActionRef) -> bool {
    list.iter().any(|a| same_action(a, action))
}

fn is_separator(action: &ActionRef) -> bool {
    action.as_any().is::<Separator>()
}

/// A chave de um item no `DisposableMap` do toolbar (o objeto do original): o endereço do `Rc`.
fn item_key<T: ?Sized>(item: &Rc<T>) -> usize {
    Rc::as_ptr(item).cast::<()>() as usize
}

/// A largura de rótulo mínima de uma ação já com o `padding`.
fn with_padding(width: f64) -> f64 {
    width + ACTION_PADDING
}

/// A largura do botão "mais" já com o `padding`.
const OVERFLOW_WIDTH: f64 = ACTION_MIN_WIDTH + ACTION_PADDING;

/// `ToggleMenuAction`: a ação do botão "mais". Herda de `Action` no original; aqui guarda uma, repassa o
/// [`IAction`] e sobrescreve o `run` (que abre o menu). O `Action` da `je-base` só vira `Action` para o
/// `downcast` do item (que escuta `onDidChange`), e esta ação nunca muda.
pub struct ToggleMenuAction {
    action: Rc<Action>,
    menu_actions: RefCell<Vec<ActionRef>>,
}

impl ToggleMenuAction {
    pub const ID: &'static str = "toolbar.toggle.more";

    /// `new ToggleMenuAction(toggleDropdownMenu, title?)`.
    pub fn new(toggle_dropdown_menu: Rc<dyn Fn()>, title: Option<&str>) -> Rc<Self> {
        let title = title.filter(|t| !t.is_empty()).map_or_else(|| localize("moreActions", "More Actions...", &[]), str::to_owned);
        let action = Action::new(Self::ID, &title, "", true, Some(Rc::new(move |_| {
            toggle_dropdown_menu();
            Dynamic::Undefined.into()
        })));
        Rc::new(Self { action, menu_actions: RefCell::new(Vec::new()) })
    }

    /// `get menuActions`.
    pub fn menu_actions(&self) -> Vec<ActionRef> {
        self.menu_actions.borrow().clone()
    }

    /// `set menuActions`.
    pub fn set_menu_actions(&self, actions: Vec<ActionRef>) {
        *self.menu_actions.borrow_mut() = actions;
    }
}

impl IAction for ToggleMenuAction {
    fn id(&self) -> String {
        self.action.id()
    }
    fn label(&self) -> String {
        self.action.label()
    }
    fn set_label(&self, value: String) {
        self.action.set_label(value);
    }
    fn tooltip(&self) -> String {
        self.action.tooltip()
    }
    fn set_tooltip(&self, value: String) {
        self.action.set_tooltip(value);
    }
    fn class(&self) -> Option<String> {
        self.action.class()
    }
    fn set_class(&self, value: Option<String>) {
        self.action.set_class(value);
    }
    fn enabled(&self) -> bool {
        self.action.enabled()
    }
    fn set_enabled(&self, value: bool) {
        self.action.set_enabled(value);
    }
    fn checked(&self) -> Option<bool> {
        self.action.checked()
    }
    fn set_checked(&self, value: Option<bool>) {
        self.action.set_checked(value);
    }
    fn run(&self, args: &[Dynamic]) -> Promise<Dynamic> {
        self.action.run(args)
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl IDisposable for ToggleMenuAction {
    fn dispose(&self) {
        self.action.dispose();
    }
}

/// `ToolBar`.
pub struct ToolBar {
    base: Rc<Disposable>,
    weak_self: Weak<ToolBar>,
    ui: Ui,
    element: NodeId,
    options: Rc<IToolBarOptions>,
    hover_delegate: Rc<dyn IHoverDelegate>,
    context_menu_provider: Rc<dyn IContextMenuProvider>,
    action_bar: Rc<ActionBar>,
    toggle_menu_action: Rc<ToggleMenuAction>,
    toggle_menu_action_view_item: RefCell<Option<Rc<DropdownMenuActionViewItem>>>,
    on_did_change_dropdown_visibility: Rc<EventMultiplexer<bool>>,
    original_primary_actions: RefCell<Vec<ActionRef>>,
    original_secondary_actions: RefCell<Vec<ActionRef>>,
    hidden_actions: RefCell<Vec<ActionRef>>,
    min_items_width: Cell<f64>,
    dropdown_menu_disposables: Rc<DisposableMap<usize, dyn IDisposable>>,
    element_min_width: Cell<Option<f64>>,
    container_min_width: Cell<Option<f64>>,
    action_min_width_property: Cell<Option<f64>>,
}

impl ToolBar {
    /// `new ToolBar(container, contextMenuProvider, options)`.
    pub fn new(ui: &Ui, container: NodeId, context_menu_provider: Rc<dyn IContextMenuProvider>, mut options: IToolBarOptions) -> Rc<Self> {
        let base = Disposable::new();
        let hover_delegate: Rc<dyn IHoverDelegate> = match options.hover_delegate.clone() {
            Some(delegate) => delegate,
            None => {
                let scoped = create_instant_hover_delegate();
                base.register(scoped.clone());
                scoped
            }
        };
        options.hover_delegate = Some(hover_delegate.clone());
        let options = Rc::new(options);

        let on_did_change_dropdown_visibility = EventMultiplexer::<bool>::new();
        base.register(on_did_change_dropdown_visibility.clone());
        let dropdown_menu_disposables = DisposableMap::<usize, dyn IDisposable>::new();
        base.register(dropdown_menu_disposables.clone());

        let element = {
            let mut tree = ui.tree_mut();
            let element = create_el(&mut tree, container, Role::Generic, "monaco-toolbar");
            tree.set_fill(element, true, true);
            element
        };

        let this = Rc::new_cyclic(|weak: &Weak<ToolBar>| {
            let toggle_menu_action = ToggleMenuAction::new(
                {
                    let weak = weak.clone();
                    Rc::new(move || {
                        let item = weak.upgrade().and_then(|this| this.toggle_menu_action_view_item.borrow().clone());
                        if let Some(item) = item {
                            item.show();
                        }
                    })
                },
                options.toggle_menu_title.as_deref(),
            );
            base.register(toggle_menu_action.clone());

            let action_view_item_provider: IActionViewItemProvider = {
                let weak = weak.clone();
                Rc::new(move |action: &ActionRef, view_item_options: &IActionViewItemOptions| weak.upgrade()?.create_view_item(action, view_item_options))
            };
            let action_bar = ActionBar::new(
                ui,
                element,
                IActionBarOptions {
                    orientation: options.orientation,
                    aria_label: options.aria_label.clone(),
                    action_runner: options.action_runner.clone(),
                    allow_context_menu: options.allow_context_menu,
                    highlight_toggled_items: options.highlight_toggled_items,
                    hover_delegate: Some(hover_delegate.clone()),
                    action_view_item_provider: Some(action_view_item_provider),
                    styles: options.styles.clone(),
                    ..IActionBarOptions::default()
                },
            );
            base.register(action_bar.clone());

            ToolBar {
                base,
                weak_self: weak.clone(),
                ui: ui.clone(),
                element,
                options,
                hover_delegate,
                context_menu_provider,
                action_bar,
                toggle_menu_action,
                toggle_menu_action_view_item: RefCell::new(None),
                on_did_change_dropdown_visibility,
                original_primary_actions: RefCell::new(Vec::new()),
                original_secondary_actions: RefCell::new(Vec::new()),
                hidden_actions: RefCell::new(Vec::new()),
                min_items_width: Cell::new(0.0),
                dropdown_menu_disposables,
                element_min_width: Cell::new(None),
                container_min_width: Cell::new(None),
                action_min_width_property: Cell::new(None),
            }
        });

        // Responsive support. O `ResizeObserver` do original não existe: ver o módulo.
        if this.is_responsive() {
            let mut tree = this.ui.tree_mut();
            tree.add_class(element, "responsive");
            tree.toggle_class(element, "responsive-all", this.kind() == ResponsiveKind::All);
            tree.toggle_class(element, "responsive-last", this.kind() == ResponsiveKind::Last);
            this.action_min_width_property.set(Some(this.get_configured_action_min_width(None)));
        }
        this
    }

    /// O `actionViewItemProvider` do `ActionBar` do toolbar: o botão "mais", o provedor do usuário e o menu
    /// de uma `SubmenuAction`.
    fn create_view_item(&self, action: &ActionRef, view_item_options: &IActionViewItemOptions) -> Option<Rc<dyn IActionViewItem>> {
        let options = &self.options;
        let dropdown_options = |base_options: IActionViewItemOptions, class_names: Vec<String>| IDropdownMenuActionViewItemOptions {
            base: IActionViewItemOptions { hover_delegate: Some(self.hover_delegate.clone()), styles: view_item_options.styles.clone(), ..base_options },
            action_view_item_provider: options.action_view_item_provider.clone(),
            action_runner: Some(self.action_runner()),
            keybinding_provider: options.get_key_binding.clone(),
            class_names,
            menu_class_name: options.dropdown_menu_class_name.clone(),
            close_animation: options.dropdown_menu_close_animation.clone(),
            anchor_alignment_provider: options.anchor_alignment_provider.clone() as Option<IAnchorAlignmentProvider>,
            menu_as_child: options.render_dropdown_as_child_element,
            skip_telemetry: options.skip_telemetry,
            ..IDropdownMenuActionViewItemOptions::default()
        };

        if action.id() == ToggleMenuAction::ID {
            let toggle = self.toggle_menu_action.clone();
            let provider: Rc<dyn IActionProvider> = Rc::new(move || toggle.menu_actions());
            let more_icon = options.more_icon.clone().unwrap_or_else(|| ThemeIcon::from_id("toolbar-more"));
            let item = DropdownMenuActionViewItem::new(
                &self.ui,
                action.clone(),
                MenuActions::Provider(provider),
                self.context_menu_provider.clone(),
                dropdown_options(IActionViewItemOptions { is_menu: true, ..IActionViewItemOptions::default() }, more_icon.as_class_name_array()),
            );
            *self.toggle_menu_action_view_item.borrow_mut() = Some(item.clone());
            self.track_dropdown_visibility(&item);
            return Some(item);
        }

        if let Some(provider) = &options.action_view_item_provider
            && let Some(result) = provider(action, view_item_options)
        {
            return Some(result);
        }

        if let Some(submenu) = action.as_any().downcast_ref::<SubmenuAction>() {
            let class_names = action.class().map(|c| c.split_whitespace().map(str::to_owned).collect()).unwrap_or_default();
            let item = DropdownMenuActionViewItem::new(
                &self.ui,
                action.clone(),
                MenuActions::Actions(submenu.actions().to_vec()),
                self.context_menu_provider.clone(),
                dropdown_options(IActionViewItemOptions::default(), class_names),
            );
            self.track_dropdown_visibility(&item);
            return Some(item);
        }

        None
    }

    /// `dropdownMenuDisposables.set(item, onDidChangeDropdownVisibility.add(item.onDidChangeVisibility))`.
    fn track_dropdown_visibility(&self, item: &Rc<DropdownMenuActionViewItem>) {
        let registration = self.on_did_change_dropdown_visibility.add(item.on_did_change_visibility());
        self.dropdown_menu_disposables.set(item_key(item), registration, false);
    }

    /// `set actionRunner`.
    pub fn set_action_runner(&self, action_runner: Rc<dyn IActionRunner>) {
        self.action_bar.set_action_runner(action_runner);
    }

    /// `get actionRunner`.
    pub fn action_runner(&self) -> Rc<dyn IActionRunner> {
        self.action_bar.action_runner()
    }

    /// `set context`.
    pub fn set_context(&self, context: Option<Dynamic>) {
        self.action_bar.set_context(context);
    }

    /// `getElement()`: o `div.monaco-toolbar`.
    pub fn get_element(&self) -> NodeId {
        self.element
    }

    /// O `ActionBar` (o `protected actionBar` do original).
    pub fn action_bar(&self) -> &Rc<ActionBar> {
        &self.action_bar
    }

    /// `element.style.minWidth` (px), o que o original reserva com `reserveMinWidth`.
    pub fn element_min_width(&self) -> Option<f64> {
        self.element_min_width.get()
    }

    /// `container.style.minWidth` (px).
    pub fn container_min_width(&self) -> Option<f64> {
        self.container_min_width.get()
    }

    /// `element.style.getPropertyValue('--vscode-toolbar-action-min-width')` (px).
    pub fn action_min_width_property(&self) -> Option<f64> {
        self.action_min_width_property.get()
    }

    pub fn on_did_change_dropdown_visibility(&self) -> BaseEvent<bool> {
        self.on_did_change_dropdown_visibility.event()
    }

    /// `focus(index?)`: foca o item de `index`, ou o primeiro habilitado.
    pub fn focus(&self, index: Option<usize>) {
        match index {
            Some(index) => self.action_bar.focus_index(index),
            None => self.action_bar.focus(true),
        }
    }

    pub fn set_focusable(&self, focusable: bool) {
        self.action_bar.set_focusable(focusable);
    }

    pub fn get_items_width(&self) -> f64 {
        (0..self.action_bar.length()).map(|i| self.action_bar.get_width(i)).sum()
    }

    /// `getItemAction(index)`.
    pub fn get_item_action(&self, index: usize) -> Option<ActionRef> {
        self.action_bar.get_action(index)
    }

    /// `getItemAction(element)`.
    pub fn get_item_action_of_node(&self, node: NodeId) -> Option<ActionRef> {
        self.action_bar.get_action_of_node(node)
    }

    pub fn get_item_width(&self, index: usize) -> f64 {
        self.action_bar.get_width(index)
    }

    /// `getItemElement(index)`: o `li` do item.
    pub fn get_item_element(&self, index: usize) -> Option<NodeId> {
        let tree = self.ui.tree();
        tree.children(self.action_bar.actions_list()).get(index).copied()
    }

    pub fn get_item_view_item(&self, index: usize) -> Option<Rc<dyn IActionViewItem>> {
        self.action_bar.view_items().get(index).cloned()
    }

    pub fn get_items_length(&self) -> usize {
        self.action_bar.length()
    }

    pub fn has_overflow(&self) -> bool {
        self.has_toggle_menu_action()
    }

    pub fn set_aria_label(&self, label: &str) {
        self.action_bar.set_aria_label(label);
    }

    pub fn is_empty(&self) -> bool {
        self.action_bar.is_empty()
    }

    fn toggle_menu_action_ref(&self) -> ActionRef {
        self.toggle_menu_action.clone()
    }

    fn has_toggle_menu_action(&self) -> bool {
        self.action_bar.has_action(&self.toggle_menu_action_ref())
    }

    fn index_of_toggle_menu_action(&self) -> Option<usize> {
        let toggle = self.toggle_menu_action_ref();
        self.action_bar.view_items().iter().position(|item| same_action(&item.action(), &toggle))
    }

    fn responsive(&self) -> Option<&IToolBarResponsiveBehaviorOptions> {
        self.options.responsive_behavior.as_ref()
    }

    fn is_responsive(&self) -> bool {
        self.responsive().is_some_and(|r| r.enabled)
    }

    fn kind(&self) -> ResponsiveKind {
        self.responsive().map(|r| r.kind).unwrap_or_default()
    }

    fn reserves_min_width(&self) -> bool {
        self.responsive().and_then(|r| r.reserve_min_width) != Some(false)
    }

    /// `kind === 'all' && overflowFrom === 'start'`.
    fn overflow_from_start(&self) -> bool {
        self.kind() == ResponsiveKind::All && self.responsive().and_then(|r| r.overflow_from) == Some(OverflowFrom::Start)
    }

    /// `relayout()`: reavalia o transbordo depois de uma mudança de largura (do contêiner ou dos itens).
    pub fn relayout(&self) {
        if self.is_responsive() {
            self.update_actions(self.get_available_width());
        }
    }

    /// `setActions(primaryActions, secondaryActions?)`.
    pub fn set_actions(&self, primary_actions: &[ActionRef], secondary_actions: &[ActionRef]) {
        self.clear();

        // Store primary and secondary actions as rendered initially
        *self.original_primary_actions.borrow_mut() = primary_actions.to_vec();
        *self.original_secondary_actions.borrow_mut() = secondary_actions.to_vec();

        let mut primary_actions_to_set = primary_actions.to_vec();

        // Inject additional action to open secondary actions if present
        if !secondary_actions.is_empty() {
            self.toggle_menu_action.set_menu_actions(secondary_actions.to_vec());
            let position = self.get_secondary_actions_position(&primary_actions_to_set);
            primary_actions_to_set.insert(position, self.toggle_menu_action_ref());
        }

        if !primary_actions_to_set.is_empty() && self.options.trailing_separator {
            primary_actions_to_set.push(Separator::new());
        }

        for action in primary_actions_to_set {
            self.push_action(action, None);
        }

        self.update_overflow_class_name();
        self.apply_responsive_action_min_widths();

        if self.is_responsive() {
            // Reset hidden actions
            self.hidden_actions.borrow_mut().clear();

            let original_primary = self.original_primary_actions.borrow().clone();
            // Set the minimum width
            if let Some(item_count) = self.responsive().and_then(|r| r.min_items) {
                let required_actions = if self.overflow_from_start() {
                    &original_primary[original_primary.len().saturating_sub(item_count)..]
                } else {
                    &original_primary[..item_count.min(original_primary.len())]
                };
                let primary_actions_min_width: f64 = required_actions.iter().map(|action| self.get_action_min_width(Some(action))).sum();

                // Account for overflow menu
                let overflow_width = if !self.original_secondary_actions.borrow().is_empty() || item_count < original_primary.len() { OVERFLOW_WIDTH } else { 0.0 };

                let separator_width = if self.options.trailing_separator && !self.action_bar.is_empty() { with_padding(self.action_bar.get_width(self.action_bar.length() - 1)) } else { 0.0 };
                self.min_items_width.set(primary_actions_min_width + overflow_width + separator_width);
                self.reserve_min_width(self.min_items_width.get());
            } else {
                let minimum_action_width = original_primary.first().map_or(OVERFLOW_WIDTH, |action| self.get_action_min_width(Some(action)));
                self.min_items_width.set(0.0);
                self.reserve_min_width(minimum_action_width);
            }

            // Update toolbar actions to fit with container width
            self.update_actions(self.get_available_width());
        }
    }

    /// `actionBar.push(action, { icon, label, keybinding, index })`.
    fn push_action(&self, action: ActionRef, index: Option<usize>) {
        let item = IActionViewItemOptions { icon: Some(self.options.icon.unwrap_or(true)), label: Some(self.options.label.unwrap_or(false)), keybinding: self.get_keybinding_label(&action), ..IActionViewItemOptions::default() };
        self.action_bar.push_action(action, IActionOptions { item, index: index.map(|i| i as i64) });
    }

    fn get_secondary_actions_position(&self, primary_actions: &[ActionRef]) -> usize {
        let requested_position = self.options.get_secondary_actions_position.as_ref().map_or(primary_actions.len() as f64, |get| get(primary_actions));
        if requested_position.is_finite() { requested_position.trunc().clamp(0.0, primary_actions.len() as f64) as usize } else { primary_actions.len() }
    }

    fn get_rendered_primary_action_count_before(&self, position: usize) -> usize {
        let rendered: Vec<ActionRef> = self.action_bar.view_items().iter().map(|item| item.action()).collect();
        let original = self.original_primary_actions.borrow();
        original[..position.min(original.len())].iter().filter(|action| contains_action(&rendered, action)).count()
    }

    fn get_toggle_menu_action_insertion_index(&self) -> usize {
        let position = self.get_secondary_actions_position(&self.original_primary_actions.borrow());
        self.get_rendered_primary_action_count_before(position)
    }

    fn get_primary_action_insertion_index(&self, action: &ActionRef) -> usize {
        let Some(action_position) = self.original_primary_actions.borrow().iter().position(|a| same_action(a, action)) else { return self.action_bar.length() };
        let toggle_offset = usize::from(self.has_toggle_menu_action() && self.get_secondary_actions_position(&self.original_primary_actions.borrow()) <= action_position);
        self.get_rendered_primary_action_count_before(action_position) + toggle_offset
    }

    fn get_keybinding_label(&self, action: &ActionRef) -> Option<String> {
        self.options.get_key_binding.as_ref().and_then(|get| get(action)).and_then(|key: Rc<dyn ResolvedKeybinding>| key.get_label())
    }

    fn get_configured_action_min_width(&self, action: Option<&ActionRef>) -> f64 {
        if action.is_some_and(|a| a.id() == ToggleMenuAction::ID) {
            return ACTION_MIN_WIDTH;
        }
        let responsive = self.responsive();
        responsive
            .and_then(|r| r.get_action_min_width.as_ref())
            .and_then(|get| get(&action.cloned().unwrap_or_else(|| self.toggle_menu_action_ref())))
            .or_else(|| responsive.and_then(|r| r.action_min_width))
            .unwrap_or(ACTION_MIN_WIDTH)
    }

    fn get_action_min_width(&self, action: Option<&ActionRef>) -> f64 {
        with_padding(self.get_configured_action_min_width(action))
    }

    fn get_available_width(&self) -> f64 {
        match self.responsive().and_then(|r| r.get_available_width.as_ref()) {
            Some(get_available_width) => get_available_width(),
            None => self.ui.tree().bounds(self.element).width(),
        }
    }

    fn reserve_min_width(&self, width: f64) {
        if !self.reserves_min_width() {
            return;
        }
        self.container_min_width.set(Some(width));
        self.element_min_width.set(Some(width));
    }

    /// `applyResponsiveActionMinWidths()`: no modo `last`, marca o `li` do último item primário como
    /// `toolbar-shrinkable-item` e ajusta `--vscode-toolbar-action-min-width` à mínima dele. No modo `all`, o
    /// original só escreve `min-width` e `flex-shrink` nos `li` (ver "Divergência" no módulo).
    fn apply_responsive_action_min_widths(&self) {
        if !self.is_responsive() || self.kind() != ResponsiveKind::Last {
            return;
        }
        let original_primary = self.original_primary_actions.borrow().clone();
        let shrinkable_index = (0..self.action_bar.length()).rev().find(|index| self.action_bar.get_action(*index).is_some_and(|action| !is_separator(&action) && contains_action(&original_primary, &action)));
        {
            let mut tree = self.ui.tree_mut();
            for (index, li) in tree.children(self.action_bar.actions_list()).to_vec().into_iter().enumerate() {
                tree.toggle_class(li, "toolbar-shrinkable-item", Some(index) == shrinkable_index);
            }
        }
        let shrinkable_action = shrinkable_index.and_then(|index| self.action_bar.get_action(index));
        self.action_min_width_property.set(Some(self.get_configured_action_min_width(shrinkable_action.as_ref())));
    }

    /// `getComputedStyle(element).display !== 'none'`.
    fn is_action_item_visible(&self, index: usize) -> bool {
        self.get_item_element(index).is_none_or(|li| self.ui.tree().style(li).visible)
    }

    /// Os índices dos itens primários (os de `setActions`, não o botão "mais" nem o separador final) visíveis.
    fn get_visible_primary_action_indexes(&self) -> Vec<usize> {
        let original_primary = self.original_primary_actions.borrow().clone();
        (0..self.action_bar.length()).filter(|index| self.action_bar.get_action(*index).is_some_and(|action| contains_action(&original_primary, &action)) && self.is_action_item_visible(*index)).collect()
    }

    fn get_visible_primary_action_count(&self) -> usize {
        self.get_visible_primary_action_indexes().into_iter().filter(|index| !self.action_bar.get_action(*index).is_some_and(|a| is_separator(&a))).count()
    }

    /// As ações a esconder agora: a primeira (do fim, ou do começo com `overflowFrom: 'start'`) que não é
    /// separador, mais os separadores primários visíveis que a seguem no sentido do transbordo.
    fn get_actions_to_hide(&self) -> Vec<ActionRef> {
        let original_primary = self.original_primary_actions.borrow().clone();
        let from_start = self.overflow_from_start();
        let mut indexes = self.get_visible_primary_action_indexes();
        if !from_start {
            indexes.reverse();
        }
        let Some(action_index) = indexes.into_iter().find(|index| !self.action_bar.get_action(*index).is_some_and(|a| is_separator(&a))) else { return Vec::new() };

        let (mut start_index, mut end_index) = (action_index, action_index);
        let length = self.action_bar.length();
        let mut index = action_index;
        loop {
            // `index += step` com `step` +1 no começo e -1 no fim, parando fora de `0..length`
            index = if from_start { index + 1 } else { match index.checked_sub(1) { Some(i) => i, None => break } };
            if index >= length {
                break;
            }
            let action = self.action_bar.get_action(index);
            if !action.as_ref().is_some_and(is_separator) || !action.is_some_and(|a| contains_action(&original_primary, &a)) || !self.is_action_item_visible(index) {
                break;
            }
            start_index = start_index.min(index);
            end_index = end_index.max(index);
        }
        (start_index..=end_index).filter_map(|index| self.action_bar.get_action(index)).collect()
    }

    /// As ações a restaurar agora, na ordem inversa do transbordo.
    fn get_actions_to_restore(&self) -> Vec<ActionRef> {
        let hidden = self.hidden_actions.borrow();
        if hidden.is_empty() {
            return Vec::new();
        }
        if self.overflow_from_start() {
            let mut start_index = hidden.len() - 1;
            while start_index > 0 && is_separator(&hidden[start_index]) {
                start_index -= 1;
            }
            return hidden[start_index..].to_vec();
        }
        let mut end_index = 0;
        while end_index < hidden.len() - 1 && is_separator(&hidden[end_index]) {
            end_index += 1;
        }
        hidden[..=end_index].to_vec()
    }

    /// `actionBarMinimumWidth()`.
    fn action_bar_minimum_width(&self) -> f64 {
        if self.kind() == ResponsiveKind::Last {
            let has_toggle_menu_action = self.has_toggle_menu_action();
            let primary_action_indexes = self.get_visible_primary_action_indexes();
            let Some((last, others)) = primary_action_indexes.split_last() else { return if has_toggle_menu_action { OVERFLOW_WIDTH } else { 0.0 } };

            let mut items_width: f64 = others.iter().map(|index| with_padding(self.action_bar.get_width(*index))).sum();
            items_width += self.get_action_min_width(self.action_bar.get_action(*last).as_ref()); // item to shrink
            items_width += if has_toggle_menu_action { OVERFLOW_WIDTH } else { 0.0 }; // toggle menu action
            items_width
        } else {
            let mut items_width = 0.0;
            for i in 0..self.action_bar.length() {
                if self.is_action_item_visible(i) {
                    let action = self.action_bar.get_action(i);
                    items_width += if action.as_ref().is_some_and(is_separator) { with_padding(self.action_bar.get_width(i)) } else { self.get_action_min_width(action.as_ref()) };
                }
            }
            items_width
        }
    }

    /// `projectedActionBarMinimumWidth(actionsToAdd, keepToggleMenuAction)`.
    fn projected_action_bar_minimum_width(&self, actions_to_add: &[ActionRef], keep_toggle_menu_action: bool) -> f64 {
        let mut items_width: f64 = actions_to_add.iter().map(|action| self.get_action_min_width(Some(action))).sum();
        if self.kind() == ResponsiveKind::Last {
            // `getUnshrunkItemWidth(index)` é a largura do `li` (sem flex-shrink, ver o módulo).
            items_width += self.get_visible_primary_action_indexes().into_iter().map(|index| with_padding(self.action_bar.get_width(index))).sum::<f64>();
        } else {
            let toggle = self.toggle_menu_action_ref();
            for i in 0..self.action_bar.length() {
                let action = self.action_bar.get_action(i);
                if let Some(action) = action.filter(|action| !same_action(action, &toggle) && self.is_action_item_visible(i)) {
                    items_width += if is_separator(&action) { with_padding(self.action_bar.get_width(i)) } else { self.get_action_min_width(Some(&action)) };
                }
            }
        }
        if keep_toggle_menu_action {
            items_width += OVERFLOW_WIDTH;
        }
        items_width
    }

    /// `updateActions(containerWidth)`: esconde ações no menu "mais" até caber, ou as restaura se há espaço.
    fn update_actions(&self, container_width: f64) {
        // Actions bar is empty
        if self.action_bar.is_empty() {
            return;
        }

        self.apply_responsive_action_min_widths();

        // Ensure that the container width respects the minimum width of the
        // element which is set based on the `responsiveBehavior.minItems` option
        // (`parseInt` do `style.minWidth`: sem valor, `NaN`, que vale 0)
        let container_width = container_width.max(self.element_min_width.get().map_or(0.0, f64::trunc));

        // Without a reserved minimum width the toolbar can be narrower than the
        // width of its minimum items, in which case they overflow too.
        let min_items = if self.reserves_min_width() || container_width >= self.min_items_width.get() { self.responsive().and_then(|r| r.min_items) } else { None };

        let mut minimum_width = self.action_bar_minimum_width();

        // Action bar fits and there are no hidden actions to show
        if minimum_width <= container_width && self.hidden_actions.borrow().is_empty() {
            return;
        }

        let focused_element = self.ui.focused();
        let focused_action = focused_element.and_then(|node| self.action_bar.get_action_of_node(node));
        let previous_hidden_actions_count = self.hidden_actions.borrow().len();

        if minimum_width > container_width {
            match self.responsive().and_then(|r| r.allow_overflow.as_ref()) {
                Some(AllowOverflow::Always(false)) => return,
                Some(AllowOverflow::When(allow_overflow)) if !allow_overflow() => return,
                _ => {}
            }

            // Check for max items limit
            if let Some(min_items) = min_items
                && self.get_visible_primary_action_count() <= min_items
            {
                return;
            }

            // Hide actions from the configured end.
            while minimum_width > container_width && self.action_bar.length() > 0 {
                if min_items.is_some_and(|min_items| self.get_visible_primary_action_count() <= min_items) {
                    break;
                }

                let actions_to_hide = self.get_actions_to_hide();
                if actions_to_hide.is_empty() {
                    break;
                }
                for action in actions_to_hide {
                    self.hidden_actions.borrow_mut().push(action.clone());
                    if let Some(index) = self.action_bar.view_items().iter().position(|item| same_action(&item.action(), &action)) {
                        self.pull_action(index);
                    }
                }
                {
                    let original_primary = self.original_primary_actions.borrow();
                    let position = |action: &ActionRef| original_primary.iter().position(|a| same_action(a, action)).map_or(-1, |p| p as i64);
                    self.hidden_actions.borrow_mut().sort_by_key(position);
                }

                // There are no secondary actions, but we have actions that we need to hide so we
                // create the overflow menu. This will ensure that another primary action will be
                // removed making space for the overflow menu.
                if self.original_secondary_actions.borrow().is_empty() && !self.has_toggle_menu_action() {
                    let item = IActionViewItemOptions { icon: Some(self.options.icon.unwrap_or(true)), label: Some(self.options.label.unwrap_or(false)), keybinding: self.get_keybinding_label(&self.toggle_menu_action_ref()), ..IActionViewItemOptions::default() };
                    self.action_bar.push_action(self.toggle_menu_action_ref(), IActionOptions { item, index: Some(self.get_toggle_menu_action_insertion_index() as i64) });
                    self.update_overflow_class_name();
                }

                self.apply_responsive_action_min_widths();
                minimum_width = self.action_bar_minimum_width();
            }
        } else {
            // Restore actions in reverse overflow order.
            while !self.hidden_actions.borrow().is_empty() {
                let actions_to_restore = self.get_actions_to_restore();
                let keep_toggle_menu_action = !self.original_secondary_actions.borrow().is_empty() || self.hidden_actions.borrow().len() > actions_to_restore.len();
                let is_below_min_items = !self.reserves_min_width() && min_items.is_some_and(|min_items| self.get_visible_primary_action_count() < min_items);
                if !is_below_min_items && self.projected_action_bar_minimum_width(&actions_to_restore, keep_toggle_menu_action) > container_width {
                    // Not enough space to show the action
                    break;
                }

                for action in &actions_to_restore {
                    {
                        let mut hidden = self.hidden_actions.borrow_mut();
                        if let Some(position) = hidden.iter().position(|a| same_action(a, action)) {
                            hidden.remove(position);
                        }
                    }
                    self.push_action(action.clone(), Some(self.get_primary_action_insertion_index(action)));
                }

                // There are no secondary actions, and there is only one hidden item left so we
                // remove the overflow menu making space for the last hidden action to be shown.
                if self.original_secondary_actions.borrow().is_empty() && self.hidden_actions.borrow().is_empty() {
                    self.toggle_menu_action.set_menu_actions(Vec::new());
                    if let Some(index) = self.index_of_toggle_menu_action() {
                        self.pull_action(index);
                    }
                    self.update_overflow_class_name();
                }

                self.apply_responsive_action_min_widths();
            }
        }

        // Update overflow menu
        let hidden_actions: Vec<ActionRef> = self
            .hidden_actions
            .borrow()
            .iter()
            .map(|action| match self.responsive().and_then(|r| r.get_overflow_action.as_ref()) {
                Some(get_overflow_action) => get_overflow_action(action, self.toggle_anchor_getter()),
                None => action.clone(),
            })
            .collect();
        if !self.original_secondary_actions.borrow().is_empty() || !hidden_actions.is_empty() {
            let secondary_actions = self.original_secondary_actions.borrow().clone();
            let mut joined = Separator::join(&[hidden_actions, secondary_actions]);
            Separator::clean(&mut joined);
            self.toggle_menu_action.set_menu_actions(joined);
        }

        self.update_overflow_class_name();
        self.apply_responsive_action_min_widths();
        if self.hidden_actions.borrow().len() == previous_hidden_actions_count {
            return;
        }

        // Rebuild the roving tab stop after items have been removed or inserted.
        for item in self.action_bar.view_items() {
            item.set_focusable(false);
        }
        if let Some(focused_action) = focused_action {
            let index = (0..self.action_bar.length()).find(|index| self.action_bar.get_action(*index).is_some_and(|a| same_action(&a, &focused_action)));
            let overflow_index = self.index_of_toggle_menu_action();
            self.focus(index.or(overflow_index));
            if let (Some(index), Some(focused_node)) = (index, focused_element)
                && let Some(item_element) = self.get_item_element(index)
                && self.ui.tree().is_alive(focused_node)
                && is_ancestor(&self.ui, focused_node, item_element)
                && self.ui.focused() != Some(focused_node)
            {
                // Keep compound controls on their previously focused child.
                if let Some(item) = self.get_item_view_item(index) {
                    item.set_focusable(false);
                    item.focus_descendant(focused_node);
                }
            }
        } else {
            self.action_bar.set_focusable(true);
        }
    }

    /// O `() => this.toggleMenuActionViewItem?.element` do `getOverflowAction`, lido na hora de usar.
    fn toggle_anchor_getter(&self) -> Rc<dyn Fn() -> Option<NodeId>> {
        let weak = self.weak_self.clone();
        Rc::new(move || weak.upgrade()?.toggle_menu_action_view_item.borrow().as_ref()?.element())
    }

    fn update_overflow_class_name(&self) {
        let has_overflow = self.has_toggle_menu_action();
        self.ui.tree_mut().toggle_class(self.action_bar.dom_node(), "has-overflow", has_overflow);
    }

    /// `pullAction(index)`: tira o item da barra e solta a assinatura de visibilidade dele.
    fn pull_action(&self, index: usize) {
        if let Some(view_item) = self.action_bar.view_items().get(index) {
            self.dropdown_menu_disposables.delete_and_dispose(&item_key(view_item));
            let is_toggle = self.toggle_menu_action_view_item.borrow().as_ref().is_some_and(|toggle| item_key(toggle) == item_key(view_item));
            if is_toggle {
                *self.toggle_menu_action_view_item.borrow_mut() = None;
            }
        }
        self.action_bar.pull(index);
    }

    fn clear(&self) {
        *self.toggle_menu_action_view_item.borrow_mut() = None;
        self.toggle_menu_action.set_menu_actions(Vec::new());
        self.dropdown_menu_disposables.clear_and_dispose_all();
        self.action_bar.clear();
    }
}

impl IDisposable for ToolBar {
    fn dispose(&self) {
        self.clear();
        self.ui.remove(self.element);
        self.base.dispose();
    }
}
