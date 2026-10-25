//! Porte de `src/vs/platform/actions/browser/toolbar.ts`: o [`WorkbenchToolBar`], a `ToolBar` de `je-widgets` com o
//! esconder de itens do menu, a busca automática de atalhos, o `workbenchActionExecuted` por ação e o menu de contexto
//! da barra (configurar atalho, esconder, redefinir o menu).
//!
//! Vive no `je-workbench` (e não no `je-platform`) porque estende a `ToolBar` de `je-widgets`, e a plataforma não
//! depende de widgets.
//!
//! Divergências de tradução:
//! - **Herança.** `WorkbenchToolBar extends ToolBar` é composição com `Deref<Target = ToolBar>`: tudo que o original
//!   herda (`getElement`, `getItemAction`, `actionBar`...) é a `ToolBar` de dentro. O tipo de interseção
//!   `IToolBarOptions & { ... }` é [`IWorkbenchToolBarOptions`] com as opções da `ToolBar` no campo `tool_bar`.
//! - **`contextmenu`.** O evento `contextmenu` do elemento é o `EventKind::ContextMenu` da `Ui`; o `StandardMouseEvent`
//!   que serve de âncora é a posição do ponteiro do evento ([`AnchorTarget::Mouse`]).
//! - **`menuActionOptions`.** `{ renderShortTitle: true, ...menuOptions }` força `render_short_title` (um `false`
//!   explícito do chamador não se distingue do padrão de um `bool`).
//! - **Contexto das `menuIds`.** O `menuIds` do original é um parâmetro que o ouvinte relê e reatribui; aqui o ouvinte
//!   guarda uma cópia e o `resetMenu` entra como `[resetMenu]` quando não há `menuIds`, a cada abertura.
//! - **`MenuWorkbenchToolBar`.** Está neste módulo (ver a documentação do tipo para o que muda: sem o
//!   `IActionViewItemService` nem o `createActionViewItem`, que não existem numa UI de nós nativos).

use std::cell::RefCell;
use std::collections::HashSet;
use std::ops::Deref;
use std::rc::{Rc, Weak};

use je_base::actions::{ActionRef, Separator, ToActionProps, to_action};
use je_base::dynamic::Dynamic;
use je_base::errors::{self, Error};
use je_base::event::{Emitter, Event};
use je_base::lifecycle::{Disposable, DisposableStore, IDisposable};
use je_base::nls::localize;
use je_base::promise::MaybePromise;
use je_platform::actions::menu_entry_action_view_item::get_action_bar_actions;
use je_platform::actions::menu_service::create_configure_keybinding_action;
use je_platform::actions::{IMenu, IMenuService, MenuActionOptions, MenuCreateOptions, MenuId, MenuItemAction, MenuItemHide, SubmenuItemAction};
use je_platform::commands::ICommandService;
use je_platform::contextkey::IContextKeyService;
use je_platform::contextview::context_view::{AnchorTarget, ContextMenuRequest, IContextMenuDelegate, IContextMenuMenuDelegate, IContextMenuService};
use je_platform::keybinding::keybinding::IKeybindingService;
use je_platform::telemetry::ITelemetryService;
use je_ui::{EventKind, NodeId, Ui};
use je_widgets::toolbar::{IToolBarOptions, ToggleMenuAction, ToolBar};

use crate::platform::contextview::ContextMenuServiceProvider;

/// `primaryGroup?: string | ((actionGroup: string) => boolean)`.
#[derive(Clone)]
pub enum PrimaryGroup {
    Name(String),
    Predicate(Rc<dyn Fn(&str) -> bool>),
}

impl PrimaryGroup {
    /// O predicado que `getActionBarActions` monta: o grupo informado, a função, ou `navigation` por padrão.
    pub fn predicate(primary_group: Option<&PrimaryGroup>) -> Rc<dyn Fn(&str) -> bool> {
        match primary_group {
            Some(PrimaryGroup::Name(name)) => {
                let name = name.clone();
                Rc::new(move |group: &str| group == name)
            }
            Some(PrimaryGroup::Predicate(predicate)) => predicate.clone(),
            None => Rc::new(|group: &str| group == "navigation"),
        }
    }
}

/// `IToolBarRenderOptions`. O `shouldInlineSubmenu` não existe: o porte do `getActionBarActions` nunca desdobra
/// submenus (ver `menu_entry_action_view_item`).
#[derive(Clone, Default)]
pub struct IToolBarRenderOptions {
    /// Determines what groups are considered primary. Defaults to `navigation`. Items of the primary group are rendered
    /// with buttons and the rest is rendered in the secondary popup-menu.
    pub primary_group: Option<PrimaryGroup>,
    /// Should the primary group allow for separators.
    pub use_separators_in_primary_actions: bool,
}

/// `HiddenItemStrategy`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HiddenItemStrategy {
    /// This toolbar doesn't support hiding.
    NoHide = -1,
    /// Hidden items aren't shown anywhere.
    Ignore = 0,
    /// Hidden items move into the secondary group.
    RenderInSecondaryGroup = 1,
}

/// `overflowBehavior`: o máximo de itens e os ids que nunca transbordam.
#[derive(Clone, Debug, Default)]
pub struct OverflowBehavior {
    pub max_items: i64,
    pub exempted: Vec<String>,
}

/// `IWorkbenchToolBarOptions`.
#[derive(Clone, Default)]
pub struct IWorkbenchToolBarOptions {
    /// As opções da `ToolBar` (a parte `IToolBarOptions` da interseção). `allowContextMenu` é controlado pela
    /// `WorkbenchToolBar`.
    pub tool_bar: IToolBarOptions,
    /// Items of the primary group can be hidden. When this happens the item can move into the secondary popup-menu, or
    /// not be shown at all.
    pub hidden_item_strategy: Option<HiddenItemStrategy>,
    /// Optional menu id which is used for a "Reset Menu" command. This should be the menu id that defines the contents
    /// of this workbench menu.
    pub reset_menu: Option<MenuId>,
    /// Optional menu id which items are used for the context menu of the toolbar.
    pub context_menu: Option<MenuId>,
    /// Optional options how menu actions are created and invoked.
    pub menu_options: Option<MenuActionOptions>,
    /// When set the `workbenchActionExecuted` is automatically send for each invoked action. The `from` property of the
    /// event will the passed `telemetrySource`-value.
    pub telemetry_source: Option<String>,
    /// Controls the overflow behavior of the primary group of toolbar.
    pub overflow_behavior: Option<OverflowBehavior>,
}

/// `toAction({ id, label, ..., run() { } })`.
fn noop_action(id: &str, label: &str, checked: Option<bool>, enabled: bool) -> ActionRef {
    to_action(ToActionProps {
        id: id.to_owned(),
        label: label.to_owned(),
        tooltip: None,
        enabled: Some(enabled),
        checked,
        class: None,
        alt: None,
        run: Rc::new(|_: &[Dynamic]| MaybePromise::Value(Dynamic::Undefined)),
    })
}

/// O `hideActions` de um `MenuItemAction` ou de um `SubmenuItemAction`: `None` se a ação é de outro tipo (o
/// `instanceof` do original) e `Some(None)` se é de um desses mas não tem `hideActions`.
fn hide_actions_of(action: &ActionRef) -> Option<Option<MenuItemHide>> {
    if let Some(item) = action.as_any().downcast_ref::<MenuItemAction>() {
        return Some(item.hide_actions.clone());
    }
    SubmenuItemAction::from_action(action).map(|submenu| submenu.hide_actions.clone())
}

/// A âncora do menu de contexto da barra: a posição do ponteiro (`getAnchor: () => event`).
struct ToolBarContextMenuDelegate {
    posx: f64,
    posy: f64,
    skip_telemetry: bool,
}

impl IContextMenuDelegate for ToolBarContextMenuDelegate {
    fn get_anchor(&self) -> AnchorTarget {
        AnchorTarget::Mouse { posx: self.posx, posy: self.posy }
    }
    /// O `Omit<IContextMenuDelegate, 'getActions'>`: as ações vêm do `getActions` do delegado "de menu".
    fn get_actions(&self) -> Vec<ActionRef> {
        Vec::new()
    }
    fn skip_telemetry(&self) -> bool {
        self.skip_telemetry
    }
}

/// O `WorkbenchToolBar`.
pub struct WorkbenchToolBar {
    base: Rc<Disposable>,
    weak_self: Weak<WorkbenchToolBar>,
    ui: Ui,
    tool_bar: Rc<ToolBar>,
    options: Option<IWorkbenchToolBarOptions>,
    menu_service: Rc<dyn IMenuService>,
    context_key_service: Rc<dyn IContextKeyService>,
    context_menu_service: Rc<dyn IContextMenuService>,
    keybinding_service: Rc<dyn IKeybindingService>,
    command_service: Rc<dyn ICommandService>,
    session_disposables: Rc<DisposableStore>,
}

impl WorkbenchToolBar {
    /// `new WorkbenchToolBar(container, options, menuService, contextKeyService, contextMenuService, keybindingService,
    /// commandService, telemetryService)`.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        ui: &Ui,
        container: NodeId,
        options: Option<IWorkbenchToolBarOptions>,
        menu_service: Rc<dyn IMenuService>,
        context_key_service: Rc<dyn IContextKeyService>,
        context_menu_service: Rc<dyn IContextMenuService>,
        keybinding_service: Rc<dyn IKeybindingService>,
        command_service: Rc<dyn ICommandService>,
        telemetry_service: Rc<dyn ITelemetryService>,
    ) -> Rc<Self> {
        let telemetry_source = options.as_ref().and_then(|o| o.telemetry_source.clone());

        // defaults, options (override defaults), mandatory (override options)
        let mut tool_bar_options = options.as_ref().map(|o| o.tool_bar.clone()).unwrap_or_default();
        if tool_bar_options.get_key_binding.is_none() {
            let keybinding_service = keybinding_service.clone();
            tool_bar_options.get_key_binding = Some(Rc::new(move |action: &ActionRef| keybinding_service.lookup_keybinding(&action.id(), None, false)));
        }
        tool_bar_options.allow_context_menu = true;
        tool_bar_options.skip_telemetry = telemetry_source.is_some();

        let base = Disposable::new();
        let tool_bar = ToolBar::new(ui, container, ContextMenuServiceProvider::new(context_menu_service.clone()), tool_bar_options);
        base.register(tool_bar.clone());
        let session_disposables = DisposableStore::new();
        base.register(session_disposables.clone());

        // telemetry logic
        if let Some(telemetry_source) = telemetry_source {
            base.register(tool_bar.action_bar().on_will_run().listen(move |e| {
                telemetry_service.public_log2("workbenchActionExecuted", Some(Dynamic::object([("id", Dynamic::from(e.action.id().as_str())), ("from", Dynamic::from(telemetry_source.as_str()))])));
            }));
        }

        Rc::new_cyclic(|weak| WorkbenchToolBar {
            base,
            weak_self: weak.clone(),
            ui: ui.clone(),
            tool_bar,
            options,
            menu_service,
            context_key_service,
            context_menu_service,
            keybinding_service,
            command_service,
            session_disposables,
        })
    }

    /// A `ToolBar` de dentro (a classe base).
    pub fn tool_bar(&self) -> &Rc<ToolBar> {
        &self.tool_bar
    }

    fn hidden_item_strategy(&self) -> Option<HiddenItemStrategy> {
        self.options.as_ref().and_then(|o| o.hidden_item_strategy)
    }

    /// `setActions(primary, secondary = [], menuIds?)`.
    pub fn set_actions(&self, primary_actions: &[ActionRef], secondary_actions: &[ActionRef], menu_ids: Option<&[MenuId]>) {
        self.session_disposables.clear();
        // for hiding and overflow we set some items to undefined
        let mut primary: Vec<Option<ActionRef>> = primary_actions.iter().cloned().map(Some).collect();
        let secondary: Vec<ActionRef> = secondary_actions.to_vec();
        let mut toggle_actions: Vec<ActionRef> = Vec::new();
        let mut toggle_actions_checked_count: usize = 0;

        // `extraSecondary` é indexado pela posição em `primary` (um array esparso no original)
        let mut extra_secondary: Vec<Option<ActionRef>> = vec![None; primary.len()];

        let mut some_are_hidden = false;
        // unless disabled, move all hidden items to secondary group or ignore them
        if self.hidden_item_strategy() != Some(HiddenItemStrategy::NoHide) {
            for i in 0..primary.len() {
                let Some(action) = primary[i].clone() else { continue };
                if action.as_any().is::<Separator>() {
                    // Track group boundaries from `primary` so hidden items keep
                    // their original groups in the overflow menu (relevant when
                    // all menu groups are treated as primary).
                    extra_secondary[i] = Some(action);
                    continue;
                }
                let Some(hide_actions) = hide_actions_of(&action) else {
                    // console.warn(`Action ${action.id}/${action.label} is not a MenuItemAction`);
                    continue;
                };
                let Some(hide_actions) = hide_actions else { continue };

                // collect all toggle actions
                toggle_actions.push(hide_actions.toggle.clone());
                if hide_actions.toggle.checked() == Some(true) {
                    toggle_actions_checked_count += 1;
                }

                // hidden items move into overflow or ignore
                if (hide_actions.is_hidden)() {
                    some_are_hidden = true;
                    primary[i] = None;
                    if self.hidden_item_strategy() != Some(HiddenItemStrategy::Ignore) {
                        extra_secondary[i] = Some(action);
                    }
                }
            }
        }

        // count for max
        if let Some(overflow_behavior) = self.options.as_ref().and_then(|o| o.overflow_behavior.as_ref()) {
            let primary_ids: HashSet<String> = primary.iter().flatten().map(|action| action.id()).collect();
            let exempted_ids: HashSet<&String> = overflow_behavior.exempted.iter().filter(|id| primary_ids.contains(*id)).collect();
            let max_items = overflow_behavior.max_items - exempted_ids.len() as i64;

            let mut count: i64 = 0;
            for i in 0..primary.len() {
                let Some(action) = primary[i].clone() else { continue };
                count += 1;
                if exempted_ids.contains(&action.id()) {
                    continue;
                }
                if count >= max_items {
                    primary[i] = None;
                    extra_secondary[i] = Some(action);
                }
            }
        }

        // coalesce turns Array<IAction|undefined> into IAction[]
        let mut primary: Vec<ActionRef> = primary.into_iter().flatten().collect();
        let mut extra_secondary: Vec<ActionRef> = extra_secondary.into_iter().flatten().collect();

        Separator::clean(&mut primary);
        Separator::clean(&mut extra_secondary);
        self.tool_bar.set_actions(&primary, &Separator::join(&[extra_secondary, secondary]));

        // add context menu for toggle and configure keybinding actions
        if !toggle_actions.is_empty() || !primary.is_empty() {
            let menu_ids: Option<Vec<MenuId>> = menu_ids.map(<[MenuId]>::to_vec);
            let toggle_actions = Rc::new(RefCell::new(toggle_actions));
            let weak = self.weak_self.clone();
            self.session_disposables.add(self.ui.listen(self.tool_bar.get_element(), EventKind::ContextMenu, false, move |e| {
                let Some(this) = weak.upgrade() else { return };
                let Some(action) = this.tool_bar.get_item_action_of_node(e.target) else { return };
                e.stop(true);
                let (posx, posy) = e.pointer.map_or((0.0, 0.0), |pointer| (pointer.x, pointer.y));

                let mut primary_actions: Vec<ActionRef> = Vec::new();

                // -- Configure Keybinding Action --
                if let Some(item) = action.as_any().downcast_ref::<MenuItemAction>()
                    && let Some(menu_keybinding) = &item.menu_keybinding
                {
                    primary_actions.push(menu_keybinding.clone());
                } else if SubmenuItemAction::from_action(&action).is_none() && !action.as_any().is::<ToggleMenuAction>() {
                    // only enable the configure keybinding action for actions that support keybindings
                    let supports_keybindings = this.keybinding_service.lookup_keybinding(&action.id(), None, false).is_some();
                    primary_actions.push(create_configure_keybinding_action(this.command_service.clone(), this.keybinding_service.clone(), &action.id(), None, supports_keybindings));
                }

                // -- Hide Actions --
                if !toggle_actions.borrow().is_empty() {
                    let mut no_hide = false;

                    // last item cannot be hidden when using ignore strategy
                    if toggle_actions_checked_count == 1 && this.hidden_item_strategy() == Some(HiddenItemStrategy::Ignore) {
                        no_hide = true;
                        let mut toggles = toggle_actions.borrow_mut();
                        if let Some(index) = toggles.iter().position(|toggle| toggle.checked() == Some(true)) {
                            toggles[index] = noop_action(&action.id(), &action.label(), Some(true), false);
                        }
                    }

                    // add "hide foo" actions
                    let hide_actions = hide_actions_of(&action);
                    if !no_hide && let Some(hide_actions) = hide_actions {
                        let Some(hide_actions) = hide_actions else {
                            // no context menu for MenuItemAction instances that support no hiding
                            // those are fake actions and need to be cleaned up
                            return;
                        };
                        primary_actions.push(hide_actions.hide.clone());
                    } else {
                        primary_actions.push(noop_action("label", &localize("hide", "Hide", &[]), None, false));
                    }
                }

                let mut actions = Separator::join(&[primary_actions, toggle_actions.borrow().clone()]);

                // add "Reset Menu" action
                let mut menu_ids = menu_ids.clone();
                if let Some(reset_menu) = this.options.as_ref().and_then(|o| o.reset_menu.clone())
                    && menu_ids.is_none()
                {
                    menu_ids = Some(vec![reset_menu]);
                }
                if some_are_hidden && let Some(menu_ids) = menu_ids {
                    actions.push(Separator::new());
                    let menu_service = this.menu_service.clone();
                    actions.push(to_action(ToActionProps {
                        id: "resetThisMenu".to_owned(),
                        label: localize("resetThisMenu", "Reset Menu", &[]),
                        tooltip: None,
                        enabled: None,
                        checked: None,
                        class: None,
                        alt: None,
                        run: Rc::new(move |_: &[Dynamic]| {
                            menu_service.reset_hidden_states(Some(menu_ids.as_slice()));
                            MaybePromise::Value(Dynamic::Undefined)
                        }),
                    }));
                }

                if actions.is_empty() {
                    return;
                }

                let options = this.options.as_ref();
                let skip_telemetry = options.is_some_and(|o| o.telemetry_source.is_some());
                let mut menu_action_options = options.and_then(|o| o.menu_options.clone()).unwrap_or_default();
                menu_action_options.render_short_title = true;
                this.context_menu_service.show_context_menu(ContextMenuRequest::Menu(IContextMenuMenuDelegate {
                    // add context menu actions (iff appicable)
                    menu_id: options.and_then(|o| o.context_menu.clone()),
                    menu_action_options: Some(menu_action_options),
                    get_actions_context: None,
                    context_key_service: Some(this.context_key_service.clone()),
                    get_actions: Some(Rc::new(move || actions.clone())),
                    base: Rc::new(ToolBarContextMenuDelegate { posx, posy, skip_telemetry }),
                }));
            }));
        }
    }
}

impl Deref for WorkbenchToolBar {
    type Target = ToolBar;

    fn deref(&self) -> &ToolBar {
        &self.tool_bar
    }
}

impl IDisposable for WorkbenchToolBar {
    fn dispose(&self) {
        self.base.dispose();
    }

    fn constructor_name(&self) -> &'static str {
        "WorkbenchToolBar"
    }
}

// ---- MenuWorkbenchToolBar -------------------------------------------------

/// `IMenuWorkbenchToolBarOptions`.
#[derive(Clone, Default)]
pub struct IMenuWorkbenchToolBarOptions {
    /// A parte `IWorkbenchToolBarOptions` da interseção. O `resetMenu` do original só aceita `undefined` (desligar o comando
    /// de redefinir); aqui `None` é o padrão do menu da barra.
    pub base: IWorkbenchToolBarOptions,
    /// Optional options to configure how the toolbar renderes items.
    pub toolbar_options: Option<IToolBarRenderOptions>,
    /// Customize the debounce delay for menu updates.
    pub event_debounce_delay: Option<u64>,
}

/// A [`WorkbenchToolBar`] movida só por um [`MenuId`].
///
/// *Note* that Manual updates via `set_actions` are NOT supported.
///
/// Divergências de tradução:
/// - **Herança.** `MenuWorkbenchToolBar extends WorkbenchToolBar` é composição com `Deref<Target = WorkbenchToolBar>`, e o
///   `setActions` que lança é o [`MenuWorkbenchToolBar::set_actions`] próprio (um método do tipo de fora tem precedência sobre
///   o do `Deref`).
/// - **`IActionViewItemService` e `createActionViewItem`.** O `actionViewItemProvider` que consulta o serviço de view items e cai
///   no `createActionViewItem` não existe: a UI é uma árvore de nós nativa e o `ToolBar` de `je-widgets` desenha cada ação, então
///   não há provedor de view item por menu para consultar (nem o `onDidChange` do serviço, que só reconstruía a barra).
/// - **`IInstantiationService`.** Quem cria a barra passa os serviços que o construtor injetava.
pub struct MenuWorkbenchToolBar {
    inner: Rc<WorkbenchToolBar>,
    ui: Ui,
    container: NodeId,
    menu: Rc<dyn IMenu>,
    menu_options: Option<MenuActionOptions>,
    toolbar_options: Option<IToolBarRenderOptions>,
    on_did_change_menu_items: Rc<Emitter<()>>,
}

impl MenuWorkbenchToolBar {
    /// `new MenuWorkbenchToolBar(container, menuId, options, menuService, contextKeyService, contextMenuService,
    /// keybindingService, commandService, telemetryService)`.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        ui: &Ui,
        container: NodeId,
        menu_id: &MenuId,
        options: Option<IMenuWorkbenchToolBarOptions>,
        menu_service: Rc<dyn IMenuService>,
        context_key_service: Rc<dyn IContextKeyService>,
        context_menu_service: Rc<dyn IContextMenuService>,
        keybinding_service: Rc<dyn IKeybindingService>,
        command_service: Rc<dyn ICommandService>,
        telemetry_service: Rc<dyn ITelemetryService>,
    ) -> Rc<Self> {
        // `{ resetMenu: menuId, ...options }`
        let mut base = options.as_ref().map(|o| o.base.clone()).unwrap_or_default();
        if base.reset_menu.is_none() {
            base.reset_menu = Some(menu_id.clone());
        }
        let inner = WorkbenchToolBar::new(ui, container, Some(base), menu_service.clone(), context_key_service.clone(), context_menu_service, keybinding_service, command_service, telemetry_service);

        // `this._store`
        let store = DisposableStore::new();
        inner.base.register(store.clone());

        // update logic
        let menu: Rc<dyn IMenu> = menu_service.create_menu(
            menu_id,
            context_key_service,
            Some(MenuCreateOptions { emit_events_for_submenu_changes: true, event_debounce_delay: options.as_ref().and_then(|o| o.event_debounce_delay) }),
        );
        store.add(menu.clone());
        let on_did_change_menu_items = Emitter::new();
        store.add(on_did_change_menu_items.clone());

        let menu_options = options.as_ref().and_then(|o| o.base.menu_options.clone());
        let toolbar_options = options.and_then(|o| o.toolbar_options);
        let this = Rc::new(Self { inner, ui: ui.clone(), container, menu: menu.clone(), menu_options, toolbar_options, on_did_change_menu_items });

        let weak = Rc::downgrade(&this);
        store.add(menu.on_did_change().listen(move |_| {
            let Some(this) = weak.upgrade() else { return };
            this.update_toolbar();
            this.on_did_change_menu_items.fire(());
        }));
        this.update_toolbar();
        this
    }

    /// `onDidChangeMenuItems`.
    pub fn on_did_change_menu_items(&self) -> Event<()> {
        self.on_did_change_menu_items.event()
    }

    fn update_toolbar(&self) {
        let is_primary_action = PrimaryGroup::predicate(self.toolbar_options.as_ref().and_then(|o| o.primary_group.as_ref()));
        let use_separators_in_primary_actions = self.toolbar_options.as_ref().is_some_and(|o| o.use_separators_in_primary_actions);
        let actions = get_action_bar_actions(&self.menu.get_actions(self.menu_options.as_ref()), &*is_primary_action, use_separators_in_primary_actions);
        self.ui.tree_mut().toggle_class(self.container, "has-no-actions", actions.primary.is_empty() && actions.secondary.is_empty());
        self.inner.set_actions(&actions.primary, &actions.secondary, None);
    }

    /// `refresh()`: force the toolbar to immediately re-evaluate its menu actions. Use this after synchronously updating context
    /// keys to avoid layout shifts caused by the debounced menu change event.
    pub fn refresh(&self) {
        self.update_toolbar();
    }

    /// `override setActions()`: lança, a barra é alimentada pelo menu.
    pub fn set_actions(&self) -> ! {
        errors::throw(Error::bug_indicating(Some("This toolbar is populated from a menu.")))
    }
}

impl Deref for MenuWorkbenchToolBar {
    type Target = WorkbenchToolBar;

    fn deref(&self) -> &WorkbenchToolBar {
        &self.inner
    }
}

impl IDisposable for MenuWorkbenchToolBar {
    fn dispose(&self) {
        self.inner.dispose();
    }

    fn constructor_name(&self) -> &'static str {
        "MenuWorkbenchToolBar"
    }
}
