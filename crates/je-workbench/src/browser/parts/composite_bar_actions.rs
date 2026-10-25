//! Porte parcial de `src/vs/workbench/browser/parts/compositeBarActions.ts`: a `SwitchCompositeViewAction`, base das ações
//! `workbench.action.previousSideBarView` e `workbench.action.nextSideBarView` da barra de atividades, e o que a barra
//! global (`GlobalCompositeBar`) exige do arquivo: [`ICompositeBarActionItem`], [`CompositeBarAction`] e o
//! [`CompositeBarActionViewItem`].
//!
//! Divergências:
//! - O resto do arquivo (`CompositeOverflowActivityAction`, `CompositeActionViewItem`, `ToggleCompositePinnedAction`...)
//!   entra com a barra de painéis nativa.
//! - `Action2` não é herdada: [`SwitchCompositeViewAction`] guarda o [`Action2Options`] do `desc` e implementa o trait.
//! - `await paneCompositeService.openPaneComposite(...)` não espera: o [`Promise`] devolvido fica com o serviço, que o
//!   resolve e trata a rejeição.
//! - **`CompositeBarAction extends Action`** é composição: guarda uma [`Action`] e delega o [`IAction`] a ela
//!   (`as_base_action` devolve a interna). O setter `compositeBarActionItem` atribui `_label` sem avisar no original; aqui
//!   `set_label` avisa a mudança do rótulo, e o item se atualiza duas vezes com o mesmo resultado.
//! - **`CompositeBarActionViewItem extends BaseActionViewItem`** é o [`CompositeBarItemCore`] (o estado e os ouvintes, num
//!   `Rc` para os ouvintes não prenderem o item) mais o [`CompositeBarActionViewItem`], que o embute. As subclasses do
//!   original (os itens globais) embutem o mesmo núcleo e entram nos pontos que o original sobrescreve por
//!   [`CompositeBarItemHooks`] (`computeTitle`, e o fim de `updateActivity`).
//! - **DOM vira [`je_ui`].** `container > a (label) + .badge > .badge-content + .active-item-indicator`. O rótulo é
//!   o codicon da classe `codicon-*` (ou o texto da ação sem `icon`). Os atributos `aria-*`, `role` e `title` são o papel
//!   e o rótulo do nó. `iconUrl` não tem equivalente (não há imagem na árvore): o rótulo com URL pinta só pelo
//!   codicon da classe, quando existe. O `getColors(theme)` do selo não existe em [`IBadge`](crate::services::activity::activity::IBadge):
//!   o selo usa as cores da barra e as do tema (`badge.foreground`, `badge.background`, `contrastBorder`).
//! - O toque (`Gesture`) e o `title` removido logo após o `aria-label` do original não têm equivalente.

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use je_base::actions::{Action, ActionRef, IAction};
use je_base::event::{Emitter, Event};
use je_base::lifecycle::{Disposable, IDisposable};
use je_base::nls::localize;
use je_base::promise::Promise;
use je_base::runtime::{TimerId, clear_timeout, set_timeout};
use je_base::themables::ThemeIcon;
use je_base::uri::URI;
use je_editor::services::theme_service::IThemeService;
use je_platform::configuration::service::IConfigurationService;
use je_platform::keybinding::keybinding::IKeybindingService;
use je_render::Rgba;
use je_render::kurbo::Rect;
use je_theme::to_rgba8;
use je_ui::{Border, Content, EventKind, NodeId, PointerEvents, Role, Tree, Ui};
use je_widgets::action_view_items::{ACTION_ICON_SIZE, ActionViewItem, BaseActionViewItem, IActionViewItem, IActionViewItemOptions};
use je_widgets::hover_api::{HoverContent, HoverStyle, HoverTarget, IDelayedHoverOptions, IHoverAppearanceOptions, IHoverDelegate2, IHoverLifecycleOptions, IHoverPersistenceOptions, IHoverPositionOptions, ManagedHoverContent};
use je_widgets::dom::create_el;
use je_widgets::text::{WORKBENCH_LINE_HEIGHT, set_node_text, ui_font};

use crate::browser::parts::pane_composite_bar::{ActivityHoverOptions, CompositeBarColors};
use crate::services::activity::activity::{IActivity, IBadge};

use je_base::dynamic::Dynamic;
use je_instantiation::ServicesAccessor;
use je_platform::actions::actions::{Action2, Action2Options};

use crate::common::views::ViewContainerLocation;
use crate::services::panecomposite::panecomposite::I_PANE_COMPOSITE_PART_SERVICE;

/// `SwitchCompositeViewAction`.
pub struct SwitchCompositeViewAction {
    desc: Action2Options,
    location: ViewContainerLocation,
    offset: i64,
}

impl SwitchCompositeViewAction {
    pub fn new(desc: Action2Options, location: ViewContainerLocation, offset: i64) -> Self {
        Self { desc, location, offset }
    }

    /// O identificador do painel que fica `offset` posições adiante do ativo, em volta da lista de visíveis; `None` se o
    /// ativo não está entre eles.
    pub fn target_composite_id(visible_composite_ids: &[String], active_composite_id: &str, offset: i64) -> Option<String> {
        let len = visible_composite_ids.len() as i64;
        let index = visible_composite_ids.iter().position(|id| id == active_composite_id)? as i64;
        Some(visible_composite_ids[((index + len + offset) % len) as usize].clone())
    }
}

impl Action2 for SwitchCompositeViewAction {
    fn desc(&self) -> Action2Options {
        self.desc.clone()
    }

    fn run(&self, accessor: &ServicesAccessor, _args: &[Dynamic]) {
        let pane_composite_service = accessor.get(I_PANE_COMPOSITE_PART_SERVICE);

        let Some(active_composite) = pane_composite_service.get_active_pane_composite(self.location) else {
            return;
        };

        let visible_composite_ids = pane_composite_service.get_visible_pane_composite_ids(self.location);
        if let Some(target_composite_id) = Self::target_composite_id(&visible_composite_ids, &active_composite.get_id(), self.offset) {
            let _ = pane_composite_service.open_pane_composite(Some(&target_composite_id), self.location, true);
        }
    }
}

/// `ICompositeBarActionItem`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ICompositeBarActionItem {
    pub id: String,
    pub name: String,
    pub keybinding_id: Option<String>,
    pub class_names: Option<Vec<String>>,
    pub icon_url: Option<URI>,
}

/// `CompositeBarAction`: a ação de um item da barra, com o que o item mostra (nome, classes) e as atividades (selos).
pub struct CompositeBarAction {
    base: Rc<Disposable>,
    action: Rc<Action>,
    item: RefCell<ICompositeBarActionItem>,
    activities: RefCell<Vec<IActivity>>,
    on_did_change_composite_bar_action_item: Rc<Emitter<()>>,
    on_did_change_activity: Rc<Emitter<Vec<IActivity>>>,
}

impl CompositeBarAction {
    /// `new CompositeBarAction(item)`: `super(item.id, item.name, item.classNames?.join(' '), true)`.
    pub fn new(item: ICompositeBarActionItem) -> Rc<Self> {
        let base = Disposable::new();
        let action = Action::new(&item.id, &item.name, &item.class_names.as_ref().map(|names| names.join(" ")).unwrap_or_default(), true, None);
        base.register(action.clone());
        let on_did_change_composite_bar_action_item = Emitter::new();
        base.register(on_did_change_composite_bar_action_item.clone());
        let on_did_change_activity = Emitter::new();
        base.register(on_did_change_activity.clone());
        Rc::new(Self { base, action, item: RefCell::new(item), activities: RefCell::new(Vec::new()), on_did_change_composite_bar_action_item, on_did_change_activity })
    }

    /// `onDidChangeCompositeBarActionItem`.
    pub fn on_did_change_composite_bar_action_item(&self) -> Event<()> {
        self.on_did_change_composite_bar_action_item.event()
    }

    /// `onDidChangeActivity`.
    pub fn on_did_change_activity(&self) -> Event<Vec<IActivity>> {
        self.on_did_change_activity.event()
    }

    /// `get compositeBarActionItem()`.
    pub fn composite_bar_action_item(&self) -> ICompositeBarActionItem {
        self.item.borrow().clone()
    }

    /// `set compositeBarActionItem(item)`.
    pub fn set_composite_bar_action_item(&self, item: ICompositeBarActionItem) {
        self.action.set_label(item.name.clone());
        *self.item.borrow_mut() = item;
        self.on_did_change_composite_bar_action_item.fire(());
    }

    /// `get activities()`.
    pub fn activities(&self) -> Vec<IActivity> {
        self.activities.borrow().clone()
    }

    /// `set activities(activities)`.
    pub fn set_activities(&self, activities: Vec<IActivity>) {
        *self.activities.borrow_mut() = activities.clone();
        self.on_did_change_activity.fire(activities);
    }

    /// `activate()`.
    pub fn activate(&self) {
        if self.action.checked() != Some(true) {
            self.action.set_checked(Some(true));
        }
    }

    /// `deactivate()`.
    pub fn deactivate(&self) {
        if self.action.checked() == Some(true) {
            self.action.set_checked(Some(false));
        }
    }
}

impl IAction for CompositeBarAction {
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
    fn as_base_action(&self) -> Option<&Action> {
        Some(&*self.action)
    }
}

impl IDisposable for CompositeBarAction {
    fn dispose(&self) {
        self.base.dispose();
    }

    fn constructor_name(&self) -> &'static str {
        "CompositeBarAction"
    }
}

/// `ICompositeBarActionViewItemOptions`: as opções do item de ação mais as da barra de composites.
#[derive(Clone)]
pub struct ICompositeBarActionViewItemOptions {
    pub base: IActionViewItemOptions,
    pub icon: bool,
    pub colors: Rc<dyn Fn(&dyn je_editor::services::theme_service::IColorTheme) -> CompositeBarColors>,
    pub hover_options: ActivityHoverOptions,
    pub has_popup: bool,
    pub compact: bool,
}

/// Os pontos que as subclasses de `CompositeBarActionViewItem` sobrescrevem.
pub trait CompositeBarItemHooks {
    /// `computeTitle()` sobrescrito; `None` é o do [`CompositeBarItemCore`].
    fn compute_title(&self) -> Option<String> {
        None
    }

    /// O que a subclasse faz depois de `super.updateActivity()` (o `updateProfileBadge` do item global).
    fn did_update_activity(&self) {}
}

/// A medida do `.badge-content` da barra de atividades (`activityaction.css`): `top`, `right`, altura e `padding` de um lado.
struct BadgeMetrics {
    top: f64,
    right: f64,
    height: f64,
    padding: f64,
    min_width: f64,
}

const BADGE_METRICS: BadgeMetrics = BadgeMetrics { top: 24.0, right: 8.0, height: 16.0, padding: 4.0, min_width: 8.0 };
const COMPACT_BADGE_METRICS: BadgeMetrics = BadgeMetrics { top: 13.0, right: 6.0, height: 13.0, padding: 2.0, min_width: 9.0 };
const BADGE_FONT_SIZE: f32 = 9.0;
const BADGE_FONT_WEIGHT: u16 = 600;

/// O que `getVisibleBadges` devolve: os selos e o tipo.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BadgeType {
    Progress,
    Icon,
    Number,
}

/// O estado e os ouvintes do `CompositeBarActionViewItem`.
pub struct CompositeBarItemCore {
    base: BaseActionViewItem,
    action: Rc<CompositeBarAction>,
    options: ICompositeBarActionViewItemOptions,
    badges_enabled: Rc<dyn Fn(&str) -> bool>,
    theme_service: Rc<dyn IThemeService>,
    hover_service: Rc<dyn IHoverDelegate2>,
    keybinding_service: Rc<dyn IKeybindingService>,
    hooks: RefCell<Option<Weak<dyn CompositeBarItemHooks>>>,
    container: Cell<Option<NodeId>>,
    label: Cell<Option<NodeId>>,
    label_text: Cell<Option<NodeId>>,
    badge: Cell<Option<NodeId>>,
    badge_content: Cell<Option<NodeId>>,
    indicator: Cell<Option<NodeId>>,
    badge_state: RefCell<Option<BadgeShown>>,
    mouse_up_timeout: RefCell<Option<TimerId>>,
    keybinding_label: RefCell<Option<Option<String>>>,
    size: Cell<(f64, f64)>,
    badge_width: Cell<f64>,
}

/// O que o selo mostra agora: o texto do número (ou vazio) e as classes que `updateActivity` pôs.
#[derive(Clone, Default)]
struct BadgeShown {
    classes: Vec<String>,
    content_classes: Vec<String>,
}

impl CompositeBarItemCore {
    /// O construtor de `CompositeBarActionViewItem` para quem embute o núcleo. `owner` é o item completo (o `Weak` do
    /// `Rc::new_cyclic` da subclasse).
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        ui: &Ui,
        owner: Weak<dyn IActionViewItem>,
        action: Rc<CompositeBarAction>,
        options: ICompositeBarActionViewItemOptions,
        badges_enabled: Rc<dyn Fn(&str) -> bool>,
        theme_service: Rc<dyn IThemeService>,
        hover_service: Rc<dyn IHoverDelegate2>,
        keybinding_service: Rc<dyn IKeybindingService>,
    ) -> Rc<Self> {
        let core = Rc::new_cyclic(|weak: &Weak<Self>| {
            let base = BaseActionViewItem::new_derived(ui, None, action.clone() as ActionRef, options.base.clone(), owner);
            let this = Self {
                base,
                action: action.clone(),
                options,
                badges_enabled,
                theme_service: theme_service.clone(),
                hover_service,
                keybinding_service: keybinding_service.clone(),
                hooks: RefCell::new(None),
                container: Cell::new(None),
                label: Cell::new(None),
                label_text: Cell::new(None),
                badge: Cell::new(None),
                badge_content: Cell::new(None),
                indicator: Cell::new(None),
                badge_state: RefCell::new(None),
                mouse_up_timeout: RefCell::new(None),
                keybinding_label: RefCell::new(None),
                size: Cell::new((0.0, 0.0)),
                badge_width: Cell::new(0.0),
            };
            let on = |f: Box<dyn Fn(&Rc<Self>)>| {
                let weak = weak.clone();
                move || {
                    if let Some(core) = weak.upgrade() {
                        f(&core);
                    }
                }
            };
            let on_theme = on(Box::new(|core| core.update_styles()));
            this.base.register(theme_service.on_did_color_theme_change().listen(move |_| on_theme()));
            let on_item = on(Box::new(|core| core.update()));
            this.base.register(action.on_did_change_composite_bar_action_item().listen(move |_| on_item()));
            let on_keybindings = on(Box::new(|core| {
                // `Event.filter(onDidUpdateKeybindings, () => this.keybindingLabel !== this.computeKeybindingLabel())`.
                let changed = core.keybinding_label.borrow().clone() != Some(core.compute_keybinding_label());
                if changed {
                    core.update_title();
                }
            }));
            this.base.register(keybinding_service.on_did_update_keybindings().listen(move |_| on_keybindings()));
            let on_activity = on(Box::new(|core| core.update_activity()));
            this.base.register(action.on_did_change_activity().listen(move |_| on_activity()));
            this
        });
        core
    }

    /// Registra a subclasse que sobrescreve `computeTitle` e o fim de `updateActivity`.
    pub fn set_hooks(&self, hooks: Weak<dyn CompositeBarItemHooks>) {
        *self.hooks.borrow_mut() = Some(hooks);
    }

    fn hooks(&self) -> Option<Rc<dyn CompositeBarItemHooks>> {
        self.hooks.borrow().as_ref().and_then(Weak::upgrade)
    }

    /// O `BaseActionViewItem` do núcleo.
    pub fn base(&self) -> &BaseActionViewItem {
        &self.base
    }

    /// O `_action` do original, já como [`CompositeBarAction`].
    pub fn composite_action(&self) -> &Rc<CompositeBarAction> {
        &self.action
    }

    /// O `container` (o `li` da barra), depois do `render`.
    pub fn container(&self) -> Option<NodeId> {
        self.container.get()
    }

    /// O `label` (o `a`), depois do `render`.
    pub fn label(&self) -> Option<NodeId> {
        self.label.get()
    }

    /// O `badge`, depois do `render`.
    pub fn badge(&self) -> Option<NodeId> {
        self.badge.get()
    }

    /// O `badgeContent`, depois do `render`.
    pub fn badge_content(&self) -> Option<NodeId> {
        self.badge_content.get()
    }

    /// As opções da barra.
    pub fn options(&self) -> &ICompositeBarActionViewItemOptions {
        &self.options
    }

    /// `get compositeBarActionItem()`.
    pub fn composite_bar_action_item(&self) -> ICompositeBarActionItem {
        self.action.composite_bar_action_item()
    }

    /// `updateStyles()`.
    pub fn update_styles(&self) {
        let theme = self.theme_service.get_color_theme();
        let colors = (self.options.colors)(&*theme);
        let checked = self.action.checked() == Some(true);
        let mut tree = self.base.ui.tree_mut();

        if let Some(label) = self.label.get() {
            let foreground = if checked { colors.active_foreground_color } else { colors.inactive_foreground_color };
            Self::paint_label(&mut tree, label, foreground.as_ref().map(to_rgba8));
            if let Some(text) = self.label_text.get() {
                if let Content::Text { color, .. } = &mut tree.style_mut(text).content {
                    *color = foreground.as_ref().map(to_rgba8).unwrap_or(*color);
                }
            }
            if !self.options.icon {
                let border_bottom = if checked { colors.active_border_bottom_color.as_ref().map(to_rgba8) } else { None };
                tree.style_mut(label).border = border_bottom.map(|color| Border { width: 1.0, color });
            }
        }

        if let Some(content) = self.badge_content.get() {
            let badge_foreground = colors.badge_foreground.or_else(|| theme.get_color("badge.foreground", None));
            let badge_background = colors.badge_background.or_else(|| theme.get_color("badge.background", None));
            let contrast_border = theme.get_color("contrastBorder", None);
            let style = tree.style_mut(content);
            style.background = badge_background.as_ref().map(to_rgba8);
            if let Content::Text { color, .. } = &mut style.content {
                if let Some(foreground) = badge_foreground.as_ref() {
                    *color = to_rgba8(foreground);
                }
            }
            style.border = contrast_border.as_ref().filter(|_| !self.options.compact).map(|color| Border { width: 1.0, color: to_rgba8(color) });
        }
        if let Some(indicator) = self.indicator.get() {
            tree.style_mut(indicator).background = if checked { colors.active_border_color.as_ref().map(to_rgba8) } else { None };
        }
        if let Some(container) = self.container.get() {
            tree.style_mut(container).background = if checked { colors.active_background_color.as_ref().map(to_rgba8) } else { colors.inactive_background_color.as_ref().map(to_rgba8) };
        }
    }

    /// A cor do codicon do rótulo (`label.style.color`).
    fn paint_label(tree: &mut Tree, label: NodeId, color: Option<Rgba>) {
        if let Content::Codicon { color: current, .. } = &mut tree.style_mut(label).content {
            if let Some(color) = color {
                *current = color;
            }
        }
    }

    /// `render(container)`.
    pub fn render(self: &Rc<Self>, container: NodeId) {
        self.base.base_render(container);
        self.container.set(Some(container));
        let ui = self.base.ui.clone();
        let (label, badge, badge_content, indicator) = {
            let mut tree = ui.tree_mut();
            if self.options.icon {
                tree.add_class(container, "icon");
            }
            // Use 'tab' inside tablist, 'button' for popup items outside tablist.
            tree.set_role(container, if self.options.base.is_tab_list || !self.options.has_popup { Role::Tab } else { Role::Button });
            let label = create_el(&mut tree, container, Role::Generic, "action-label");
            let badge = create_el(&mut tree, container, Role::Generic, "badge");
            let badge_content = create_el(&mut tree, badge, Role::Generic, "badge-content");
            let indicator = create_el(&mut tree, container, Role::Generic, "active-item-indicator");
            tree.style_mut(badge).visible = false;
            tree.style_mut(indicator).pointer_events = PointerEvents::None;
            (label, badge, badge_content, indicator)
        };
        self.label.set(Some(label));
        self.badge.set(Some(badge));
        self.badge_content.set(Some(badge_content));
        self.indicator.set(Some(indicator));

        // Try hard to prevent keyboard only focus feedback when using mouse.
        let weak = Rc::downgrade(self);
        self.base.register(ui.listen(container, EventKind::PointerDown, false, {
            let ui = ui.clone();
            move |_| ui.tree_mut().add_class(container, "clicked")
        }));
        self.base.register(ui.listen(container, EventKind::PointerUp, false, move |_| {
            let Some(core) = weak.upgrade() else { return };
            if let Some(timer) = core.mouse_up_timeout.borrow_mut().take() {
                clear_timeout(timer);
            }
            let weak = Rc::downgrade(&core);
            // delayed to prevent focus feedback from showing on mouse up
            let timer = set_timeout(
                move || {
                    if let Some(core) = weak.upgrade() {
                        core.mouse_up_timeout.borrow_mut().take();
                        core.base.ui.tree_mut().remove_class(container, "clicked");
                    }
                },
                800,
            );
            *core.mouse_up_timeout.borrow_mut() = Some(timer);
        }));

        let weak = Rc::downgrade(self);
        self.base.register(self.hover_service.setup_delayed_hover(
            container,
            Rc::new(move || {
                let title = weak.upgrade().map(|core| core.compute_title()).unwrap_or_default();
                let mut options = IDelayedHoverOptions::new(HoverContent::Text(title), HoverTarget::Node(container));
                options.style = Some(HoverStyle::Pointer);
                if let Some(core) = weak.upgrade() {
                    options.position = Some(IHoverPositionOptions { hover_position: Some((core.options.hover_options.position)()), ..IHoverPositionOptions::default() });
                }
                options.persistence = Some(IHoverPersistenceOptions { hide_on_key_down: Some(true), ..IHoverPersistenceOptions::default() });
                options.appearance = Some(IHoverAppearanceOptions::default());
                options
            }),
            Some(IHoverLifecycleOptions { group_id: Some("composite-bar-actions".to_owned()), ..IHoverLifecycleOptions::default() }),
        ));

        self.update();
        self.update_styles();
        self.update_title();
    }

    /// `update()`.
    pub fn update(&self) {
        self.update_label();
        self.update_activity();
        self.update_title();
        self.update_styles();
    }

    /// `getActivities()`.
    fn get_activities(&self) -> Vec<IActivity> {
        self.action.activities()
    }

    /// `updateActivity()`.
    pub fn update_activity(&self) {
        let (Some(badge), Some(badge_content)) = (self.badge.get(), self.badge_content.get()) else { return };
        let (badges, kind) = Self::get_visible_badges(&self.get_activities());

        // `this.badgeDisposable.value = new DisposableStore()`: desfaz as classes que a atualização anterior pôs.
        let previous = self.badge_state.borrow_mut().take();
        let mut shown = BadgeShown::default();
        {
            let mut tree = self.base.ui.tree_mut();
            if let Some(previous) = previous {
                for class in &previous.classes {
                    tree.remove_class(badge, class);
                }
                for class in &previous.content_classes {
                    tree.remove_class(badge_content, class);
                }
            }
            tree.style_mut(badge_content).content = Content::None;
            tree.style_mut(badge).visible = false;
        }

        let should_render_badges = (self.badges_enabled)(&self.composite_bar_action_item().id);
        if !badges.is_empty() && should_render_badges {
            let mut classes: Vec<String> = Vec::new();
            if self.options.compact {
                classes.push("compact".to_owned());
            }
            let mut tree = self.base.ui.tree_mut();
            match kind {
                // Progress
                Some(BadgeType::Progress) => {
                    tree.style_mut(badge).visible = true;
                    classes.push("progress-badge".to_owned());
                }
                // Number
                Some(BadgeType::Number) => {
                    let total: f64 = badges.iter().map(|b| b.number().unwrap_or(0.0)).sum();
                    if total > 0.0 {
                        let mut badge_number = format_number(total);
                        if total > 999.0 {
                            let no_of_thousands = total / 1000.0;
                            let floor = no_of_thousands.floor();
                            badge_number = if no_of_thousands > floor { format!("{}K+", format_number(floor)) } else { format!("{}K", format_number(no_of_thousands)) };
                        }
                        if self.options.compact && badge_number.chars().count() >= 3 {
                            classes.push("compact-content".to_owned());
                        }
                        let metrics = if self.options.compact { &COMPACT_BADGE_METRICS } else { &BADGE_METRICS };
                        let font = ui_font(BADGE_FONT_SIZE, BADGE_FONT_WEIGHT);
                        // A cor definitiva vem de `update_styles`, logo depois.
                        let width = set_node_text(&mut tree, badge_content, &badge_number, &font, Rgba::new(255, 255, 255, 255), metrics.height);
                        self.badge_width.set(width + 2.0 * metrics.padding);
                        tree.style_mut(badge).visible = true;
                    }
                }
                // Icon
                Some(BadgeType::Icon) => {
                    classes.push("icon-badge".to_owned());
                    let icon = badges[0].icon().unwrap_or_else(|| ThemeIcon::from_id("error"));
                    let mut content_classes = vec!["icon-overlay".to_owned()];
                    content_classes.extend(icon.as_class_name_array());
                    for class in &content_classes {
                        tree.add_class(badge_content, class);
                    }
                    if let Some(id) = ActionViewItem::codicon_id(&content_classes.join(" ")) {
                        tree.style_mut(badge_content).content = Content::Codicon { id, size: 13.0, color: Rgba::new(255, 255, 255, 255) };
                    }
                    shown.content_classes = content_classes;
                    tree.style_mut(badge).visible = true;
                }
                None => {}
            }
            for class in &classes {
                tree.add_class(badge, class);
            }
            shown.classes = classes;
        }
        *self.badge_state.borrow_mut() = Some(shown);

        self.update_title();
        self.update_styles();
        self.relayout();
        if let Some(hooks) = self.hooks() {
            hooks.did_update_activity();
        }
    }

    /// `getVisibleBadges(activities)`.
    fn get_visible_badges(activities: &[IActivity]) -> (Vec<Rc<dyn IBadge>>, Option<BadgeType>) {
        let pick = |filter: &dyn Fn(&Rc<dyn IBadge>) -> bool| -> Vec<Rc<dyn IBadge>> {
            activities.iter().map(|activity| activity.badge.clone()).filter(|badge| filter(badge)).collect()
        };
        let progress = pick(&|badge| badge.is_progress_badge());
        if !progress.is_empty() {
            return (progress, Some(BadgeType::Progress));
        }
        let icon = pick(&|badge| badge.icon().is_some());
        if !icon.is_empty() {
            return (icon, Some(BadgeType::Icon));
        }
        let number = pick(&|badge| badge.number().is_some());
        if !number.is_empty() {
            return (number, Some(BadgeType::Number));
        }
        (Vec::new(), None)
    }

    /// `updateLabel()`.
    pub fn update_label(&self) {
        let Some(label) = self.label.get() else { return };
        let item = self.composite_bar_action_item();
        let mut class_names = vec!["action-label".to_owned()];
        class_names.extend(item.class_names.clone().unwrap_or_default());
        let mut tree = self.base.ui.tree_mut();
        tree.set_class_name(label, &class_names.join(" "));
        if self.options.icon {
            let color = match tree.style(label).content {
                Content::Codicon { color, .. } => color,
                _ => Rgba::new(204, 204, 204, 255),
            };
            tree.style_mut(label).content = match ActionViewItem::codicon_id(&class_names.join(" ")) {
                Some(id) => Content::Codicon { id, size: ACTION_ICON_SIZE as f32, color },
                None => Content::None,
            };
        } else {
            let text_node = match self.label_text.get() {
                Some(text) => text,
                None => {
                    let text = create_el(&mut tree, label, Role::Generic, "action-label-text");
                    self.label_text.set(Some(text));
                    text
                }
            };
            let font = ui_font(11.0, 400);
            let text = self.base.action().label();
            set_node_text(&mut tree, text_node, &text, &font, Rgba::new(204, 204, 204, 255), WORKBENCH_LINE_HEIGHT);
        }
    }

    /// `updateTitle()`.
    pub fn update_title(&self) {
        let title = self.compute_title();
        let mut tree = self.base.ui.tree_mut();
        for node in [self.label.get(), self.badge.get(), self.container.get()].into_iter().flatten() {
            tree.set_label(node, title.clone());
        }
    }

    /// `computeTitle()`: o da subclasse, quando ela sobrescreve.
    pub fn compute_title(&self) -> String {
        if let Some(title) = self.hooks().and_then(|hooks| hooks.compute_title()) {
            return title;
        }
        self.compute_base_title()
    }

    /// `computeTitle()` de `CompositeBarActionViewItem`.
    pub fn compute_base_title(&self) -> String {
        let keybinding_label = self.compute_keybinding_label();
        *self.keybinding_label.borrow_mut() = Some(keybinding_label.clone());
        let name = self.composite_bar_action_item().name;
        let mut title = match keybinding_label.filter(|label| !label.is_empty()) {
            Some(label) => localize("titleKeybinding", "{0} ({1})", &[&name, &label]),
            None => name,
        };
        for badge in Self::get_visible_badges(&self.action.activities()).0 {
            let description = badge.get_description();
            if description.is_empty() {
                continue;
            }
            title = format!("{title} - {description}");
        }
        title
    }

    /// `computeKeybindingLabel()`.
    fn compute_keybinding_label(&self) -> Option<String> {
        let keybinding_id = self.composite_bar_action_item().keybinding_id?;
        self.keybinding_service.lookup_keybinding(&keybinding_id, None, false)?.get_label()
    }

    /// O tamanho natural do item: o `container` é o quadrado do `li` que a barra posiciona.
    pub fn layout(&self, width: f64, height: f64) {
        self.size.set((width, height));
        self.relayout();
    }

    /// Posiciona o rótulo, o selo e o indicador no `container` (`activityaction.css`).
    fn relayout(&self) {
        let (width, height) = self.size.get();
        let (Some(label), Some(badge), Some(badge_content), Some(indicator)) = (self.label.get(), self.badge.get(), self.badge_content.get(), self.indicator.get()) else {
            return;
        };
        let metrics = if self.options.compact { &COMPACT_BADGE_METRICS } else { &BADGE_METRICS };
        let mut tree = self.base.ui.tree_mut();
        for node in [label, badge, indicator] {
            tree.set_bounds(node, Rect::new(0.0, 0.0, width, height));
        }
        let content_width = self.badge_width.get().max(metrics.min_width + 2.0 * metrics.padding);
        let right = width - metrics.right;
        tree.set_bounds(badge_content, Rect::new(right - content_width, metrics.top, right, metrics.top + metrics.height));
    }

    /// `dispose()` do `CompositeBarActionViewItem`.
    pub fn dispose(&self) {
        if let Some(timer) = self.mouse_up_timeout.borrow_mut().take() {
            clear_timeout(timer);
        }
        let badge = self.badge.take();
        self.base.base_dispose();
        if let Some(badge) = badge {
            self.base.ui.remove(badge);
        }
    }
}

/// `number.toString()` do JavaScript para um inteiro (sem a parte decimal) ou um decimal (`1.5`).
fn format_number(value: f64) -> String {
    if value.fract() == 0.0 { format!("{}", value as i64) } else { format!("{value}") }
}

/// `CompositeBarActionViewItem`.
pub struct CompositeBarActionViewItem {
    core: Rc<CompositeBarItemCore>,
}

impl CompositeBarActionViewItem {
    /// `new CompositeBarActionViewItem(action, options, badgesEnabled, themeService, hoverService, configurationService,
    /// keybindingService)`: o `IConfigurationService` não é lido pelo item base, só pelas subclasses.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        ui: &Ui,
        action: Rc<CompositeBarAction>,
        options: ICompositeBarActionViewItemOptions,
        badges_enabled: Rc<dyn Fn(&str) -> bool>,
        theme_service: Rc<dyn IThemeService>,
        hover_service: Rc<dyn IHoverDelegate2>,
        _configuration_service: Rc<dyn IConfigurationService>,
        keybinding_service: Rc<dyn IKeybindingService>,
    ) -> Rc<Self> {
        Rc::new_cyclic(|weak: &Weak<Self>| Self {
            core: CompositeBarItemCore::new(ui, weak.clone() as Weak<dyn IActionViewItem>, action, options, badges_enabled, theme_service, hover_service, keybinding_service),
        })
    }

    /// O núcleo.
    pub fn core(&self) -> &Rc<CompositeBarItemCore> {
        &self.core
    }
}

impl IActionViewItem for CompositeBarActionViewItem {
    fn base(&self) -> &BaseActionViewItem {
        self.core.base()
    }

    fn render(&self, container: NodeId) {
        self.core.render(container);
    }

    fn update_label(&self) {
        self.core.update_label();
    }

    fn update_aria_label(&self) {
        self.core.update_title();
    }

    fn get_hover_contents(&self) -> Option<ManagedHoverContent> {
        // A dica é a do `setupDelayedHover` do `render`, não a gerenciada do base.
        None
    }

    fn layout(&self, _vertical: bool, width: f64, height: f64) {
        self.core.layout(width, height);
    }
}

impl IDisposable for CompositeBarActionViewItem {
    fn dispose(&self) {
        self.core.dispose();
    }
}
