//! Porte de `src/vs/workbench/browser/parts/statusbar/statusbarPart.ts`: a `StatusbarPart` (as duas listas de itens, as entradas
//! pendentes antes do `create`, as entradas compactas, a cor e a borda da barra, o menu de contexto, a sobreposição de entradas e
//! de estilo), a `MainStatusbarPart` e a `AuxiliaryStatusbarPart`.
//!
//! Divergências:
//! - **DOM vira estado nativo.** Os elementos são [`ViewElement`] (classes, estilo, pai). O que o original faz com o DOM e o
//!   `ViewElement` não modela (ordem dos filhos, `tabIndex`, `id`, ouvintes de `contextmenu`, de toque e de `mouseover`/`mouseout`,
//!   o foco, a folha de estilo dinâmica) entra pelo trait [`StatusbarPartView`], passado nas [`StatusbarPartDeps`]. As cores
//!   inline guardam o texto que o CSSOM devolveria (`rgb(...)`), e a folha de estilo dinâmica recebe as cores como `toString()`.
//! - **`Part` (`part.ts`) e `Themable`.** A base não foi portada como classe: o registro no layout, o `onDidChange`, o `layout`
//!   sem título (a área de conteúdo é o próprio elemento), a reação ao tema só depois de `create` e o `getColor` vivem aqui, sobre o
//!   [`PartTheme`](crate::browser::parts::part_support::PartTheme).
//! - **Herança de teste.** `hasWindowFocus` é o `has_focus` da [`TargetWindow`], e o `updateStyles` sobrescrito dos testes é o gancho
//!   [`StatusbarPartDeps::on_update_styles`], chamado a cada `update_styles`.
//! - **`IInstantiationService`.** O `createInstance(StatusbarEntryItem, ...)` é a fábrica [`StatusbarItemServicesFactory`] mais o
//!   `hover_delegate`; o `createInstance(WorkbenchHoverDelegate, ...)` é [`create_hover_delegate`]. `invokeFunction(ToggleStatusbarVisibilityAction)`
//!   é `toggle_statusbar_visibility`, e `ManageExtensionAction` usa o `ICommandService`.
//! - **`addEntry(entry, id, alignment, priorityOrLocation = 0)`** recebe a [`StatusbarEntryPriorityInput`]
//!   (`number | IStatusbarEntryLocation | IStatusbarEntryPriority`).
//! - **`IStatusbarViewModelEntry.name` e `hasCommand`** são getters vivos no original; o modelo guarda o valor de quando a entrada
//!   entrou, e a parte lê o valor vivo do item (`name` e `has_command`) onde o original os lê.
//! - **`overrideEntry(id, override)`** com `Partial<IStatusbarEntry>` é um [`StatusbarEntryOverride`] (uma função que escreve os campos).
//! - **`StatusbarService`, `ScopedStatusbarService` e `IStatusbarEntryContainer`** dependem do `MultiWindowParts` (não portado) e não
//!   entram aqui.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::{Rc, Weak};

use je_base::actions::{ActionRef, Separator, ToActionProps, to_action};
use je_base::arrays::equals;
use je_base::async_::TimeoutTimer;
use je_base::dynamic::Dynamic;
use je_base::event::{Emitter, Event};
use je_base::hash::hash;
use je_base::lifecycle::{Disposable, DisposableRef, DisposableStore, IDisposable, MutableDisposable, to_disposable};
use je_base::nls::localize;
use je_base::promise::MaybePromise;
use je_editor::services::theme_service::{IColorTheme, IThemeService};
use je_platform::commands::commands::ICommandService;
use je_platform::configuration::models::ConfigurationChangeEvent;
use je_platform::configuration::service::IConfigurationService;
use je_platform::contextkey::contextkey::IContextKeyService;
use je_platform::contextview::context_view::{AnchorTarget, ContextMenuRequest, IContextMenuDelegate, IContextMenuService};
use je_platform::contextview::view_element::ViewElement;
use je_platform::hover::hover_service::{
    HoverPlacement, HoverService, IHoverAppearanceOptions, IHoverDelegate, IHoverDom, IHoverPersistenceOptions, ManagedHoverContent, ManagedHoverContentOrFactory,
};
use je_platform::hover::workbench_hover_delegate::{HoverOverrideOptions, HoverOverrides, IHoverDelayOptions, WorkbenchHoverDelegate};
use je_platform::storage::StorageService;
use je_platform::workspace::{IWorkspaceContextService, WorkbenchState};
use je_widgets::gridview::ViewDimensions;

use crate::browser::parts::part_support::{PartTheme, TargetWindow, ViewElementTarget, is_part_inactive, set_style_or_remove};
use crate::browser::parts::statusbar::statusbar_item::{StatusbarEntryItem, StatusbarItemServices};
use crate::browser::parts::statusbar::statusbar_model::{StatusbarElementId, StatusbarEntryDom, StatusbarViewModel, StatusbarViewModelEntry, VisibleEdgeMark};
use crate::common::contextkeys::status_bar_focused;
use crate::contrib::modern_ui::frosted_glass_contribution::is_high_contrast;
use crate::contrib::modern_ui::resolved_style::css_rgb;
use crate::services::host::host::IHostService;
use crate::services::layout::layout_service::{IWorkbenchLayoutService, LayoutPart, LayoutSettings, Parts};
use crate::services::statusbar::statusbar::{
    StatusbarAlignment, StatusbarEntry, StatusbarEntryAccessor, StatusbarEntryLocation, StatusbarEntryOverride, StatusbarEntryPrimaryPriority, StatusbarEntryPriority,
    StatusbarStyleOverride,
};

// As cores de `workbench/common/theme.ts` e de `colorRegistry.ts` que a parte lê.
pub const STATUS_BAR_FOREGROUND: &str = "statusBar.foreground";
pub const STATUS_BAR_NO_FOLDER_FOREGROUND: &str = "statusBar.noFolderForeground";
pub const STATUS_BAR_BACKGROUND: &str = "statusBar.background";
pub const STATUS_BAR_INACTIVE_BACKGROUND: &str = "statusBar.inactiveBackground";
pub const STATUS_BAR_NO_FOLDER_BACKGROUND: &str = "statusBar.noFolderBackground";
pub const STATUS_BAR_BORDER: &str = "statusBar.border";
pub const STATUS_BAR_FOCUS_BORDER: &str = "statusBar.focusBorder";
pub const STATUS_BAR_NO_FOLDER_BORDER: &str = "statusBar.noFolderBorder";
pub const STATUS_BAR_ITEM_FOCUS_BORDER: &str = "statusBarItem.focusBorder";
pub const STATUS_BAR_ITEM_HOVER_BACKGROUND: &str = "statusBarItem.hoverBackground";
pub const STATUS_BAR_ITEM_COMPACT_HOVER_BACKGROUND: &str = "statusBarItem.compactHoverBackground";
pub const CONTRAST_BORDER: &str = "contrastBorder";
pub const ACTIVE_CONTRAST_BORDER: &str = "contrastActiveBorder";

/// `ToggleStatusbarVisibilityAction.ID`.
pub const TOGGLE_STATUSBAR_VISIBILITY_ACTION_ID: &str = "workbench.action.toggleStatusbarVisibility";

/// O `MouseEvent` (ou o `GestureEvent` de toque) do menu de contexto: o alvo e a posição do ponteiro (`StandardMouseEvent`).
pub struct StatusbarContextMenuEvent {
    pub target: Option<Rc<ViewElement>>,
    pub posx: f64,
    pub posy: f64,
}

/// Tudo que a parte faz com o DOM além do que o [`ViewElement`] guarda.
pub trait StatusbarPartView {
    /// `parent.appendChild(child)`.
    fn append_child(&self, parent: &Rc<ViewElement>, child: &Rc<ViewElement>);

    /// `parent.insertBefore(child, reference)`.
    fn insert_before(&self, parent: &Rc<ViewElement>, child: &Rc<ViewElement>, reference: &Rc<ViewElement>);

    /// `clearNode(parent)`.
    fn clear_node(&self, parent: &Rc<ViewElement>);

    /// `child.remove()`.
    fn remove(&self, child: &Rc<ViewElement>);

    /// `element.id = id` (o `$('.statusbar-item', { id })`).
    fn set_id(&self, element: &Rc<ViewElement>, id: &str);

    /// `element.tabIndex = index`.
    fn set_tab_index(&self, element: &Rc<ViewElement>, index: i32);

    /// `addDisposableListener(element, EventType.CONTEXT_MENU, ...)` e `addDisposableListener(element, TouchEventType.Contextmenu, ...)`:
    /// o ouvinte já para o evento (`EventHelper.stop(e, true)`) antes de chamar o tratador.
    fn add_context_menu_listener(&self, element: &Rc<ViewElement>, handler: Rc<dyn Fn(&StatusbarContextMenuEvent)>) -> DisposableRef;

    /// `Gesture.addTarget(element)`.
    fn add_touch_target(&self, element: &Rc<ViewElement>) -> DisposableRef;

    /// `addDisposableListener(element, EventType.MOUSE_OVER, ...)` e `addDisposableListener(element, EventType.MOUSE_OUT, ...)`.
    fn add_hover_listeners(&self, element: &Rc<ViewElement>, on_over: Rc<dyn Fn()>, on_out: Rc<dyn Fn()>) -> DisposableRef;

    /// `isAncestorOfActiveElement(element)`.
    fn is_ancestor_of_active_element(&self, element: &Rc<ViewElement>) -> bool;

    /// `element.focus()`.
    fn focus(&self, element: &Rc<ViewElement>);

    /// `createStyleSheet(container).textContent = css`.
    fn set_style_sheet(&self, container: &Rc<ViewElement>, css: &str);
}

/// `instantiationService.createInstance(StatusbarEntryItem, ...)`: os serviços de um item novo.
pub type StatusbarItemServicesFactory = Rc<dyn Fn() -> StatusbarItemServices>;

/// `IStatusbarEntryPriority | IStatusbarEntryLocation | number`, o `priorityOrLocation` de `addEntry`.
#[derive(Clone, Debug, PartialEq)]
pub enum StatusbarEntryPriorityInput {
    Number(f64),
    Location(StatusbarEntryLocation),
    Priority(StatusbarEntryPriority),
}

impl Default for StatusbarEntryPriorityInput {
    fn default() -> Self {
        Self::Number(0.0)
    }
}

/// O `WorkbenchHoverDelegate` do `hoverDelegate` da parte: `'element'`, `instantHover: true`, o atraso maior para conteúdo rico e a
/// persistência (`hideOnKeyDown`, `sticky: focus`) com `maxHeightRatio: 0.9`.
pub fn create_hover_delegate(configuration_service: Rc<dyn IConfigurationService>, hover_service: Rc<HoverService>, dom: Rc<dyn IHoverDom>) -> Rc<WorkbenchHoverDelegate> {
    let hover_options = IHoverDelayOptions {
        instant_hover: true,
        dynamic_delay: Some(Rc::new(|content: Option<&ManagedHoverContentOrFactory>| {
            // override the delay for content that is rich (e.g. html or long running) so that it appears more instantly. these
            // hovers carry more important information and should not be delayed by preference.
            let is_rich = match content {
                Some(ManagedHoverContentOrFactory::Factory(_)) => true,
                Some(ManagedHoverContentOrFactory::Content(ManagedHoverContent::Tooltip(tooltip))) => tooltip.element.is_some(),
                _ => false,
            };
            is_rich.then_some(500.0)
        })),
    };
    let override_options = HoverOverrideOptions::Dynamic(Rc::new(|_, focus| HoverOverrides {
        persistence: Some(IHoverPersistenceOptions { hide_on_key_down: Some(true), sticky: Some(focus), hide_on_hover: None }),
        appearance: Some(IHoverAppearanceOptions { max_height_ratio: Some(0.9), ..IHoverAppearanceOptions::default() }),
        ..HoverOverrides::default()
    }));
    WorkbenchHoverDelegate::new(HoverPlacement::Element, Some(hover_options), override_options, configuration_service, hover_service, dom)
}

/// As dependências injetadas no construtor de `StatusbarPart` (e de `MainStatusbarPart`).
pub struct StatusbarPartDeps {
    pub window: TargetWindow,
    pub theme_service: Rc<dyn IThemeService>,
    pub context_service: Rc<dyn IWorkspaceContextService>,
    pub storage_service: Rc<dyn StorageService>,
    pub layout_service: Rc<dyn IWorkbenchLayoutService>,
    pub context_menu_service: Rc<dyn IContextMenuService>,
    pub context_key_service: Rc<dyn IContextKeyService>,
    pub configuration_service: Rc<dyn IConfigurationService>,
    pub host_service: Rc<dyn IHostService>,
    pub command_service: Rc<dyn ICommandService>,
    pub view: Rc<dyn StatusbarPartView>,
    pub item_services: StatusbarItemServicesFactory,
    pub hover_delegate: Rc<dyn IHoverDelegate>,
    /// `invokeFunction(accessor => new ToggleStatusbarVisibilityAction().run(accessor))`.
    pub toggle_statusbar_visibility: Rc<dyn Fn()>,
    /// O `updateStyles` sobrescrito dos testes: chamado no começo de cada `update_styles`.
    pub on_update_styles: Option<Rc<dyn Fn()>>,
}

/// `IPendingStatusbarEntry`.
struct PendingStatusbarEntry {
    id: String,
    alignment: StatusbarAlignment,
    priority: StatusbarEntryPriority,
    entry: RefCell<StatusbarEntry>,
    accessor: RefCell<Option<Rc<dyn StatusbarEntryAccessor>>>,
}

/// O `IStatusbarEntryAccessor` das entradas pendentes e das criadas.
struct ClosureAccessor {
    update: Box<dyn Fn(StatusbarEntry)>,
    dispose: Box<dyn Fn()>,
}

impl StatusbarEntryAccessor for ClosureAccessor {
    fn update(&self, entry: StatusbarEntry) {
        (self.update)(entry);
    }
}

impl IDisposable for ClosureAccessor {
    fn dispose(&self) {
        (self.dispose)();
    }
}

/// Os `HTMLElement`s das entradas, que o modelo identifica por [`StatusbarElementId`].
struct EntryDom {
    view: Rc<dyn StatusbarPartView>,
    next_id: Cell<u64>,
    elements: RefCell<HashMap<u64, Rc<ViewElement>>>,
}

impl EntryDom {
    fn new(view: Rc<dyn StatusbarPartView>) -> Rc<Self> {
        Rc::new(Self { view, next_id: Cell::new(1), elements: RefCell::new(HashMap::new()) })
    }

    fn register(&self, element: &Rc<ViewElement>) -> StatusbarElementId {
        let id = self.next_id.get();
        self.next_id.set(id + 1);
        self.elements.borrow_mut().insert(id, element.clone());
        StatusbarElementId(id)
    }

    fn unregister(&self, id: StatusbarElementId) {
        self.elements.borrow_mut().remove(&id.0);
    }

    fn element(&self, id: StatusbarElementId) -> Rc<ViewElement> {
        self.elements.borrow().get(&id.0).cloned().expect("o elemento da entrada foi registrado")
    }

    /// O identificador do elemento, ou `None` se ele não é de uma entrada.
    fn id_of(&self, element: &Rc<ViewElement>) -> Option<StatusbarElementId> {
        self.elements.borrow().iter().find(|(_, candidate)| Rc::ptr_eq(candidate, element)).map(|(id, _)| StatusbarElementId(*id))
    }
}

impl StatusbarEntryDom for EntryDom {
    fn set_visible(&self, container: StatusbarElementId, visible: bool) {
        let element = self.element(container);
        if visible {
            element.show();
        } else {
            element.hide();
        }
    }

    fn is_ancestor_of_active_element(&self, container: StatusbarElementId) -> bool {
        self.view.is_ancestor_of_active_element(&self.element(container))
    }

    fn focus(&self, label_container: StatusbarElementId) {
        self.view.focus(&self.element(label_container));
    }

    fn remove_visible_edge_marks(&self, container: StatusbarElementId) {
        let element = self.element(container);
        element.class_list_remove(VisibleEdgeMark::First.class_name());
        element.class_list_remove(VisibleEdgeMark::Last.class_name());
    }

    fn add_visible_edge_mark(&self, container: StatusbarElementId, mark: VisibleEdgeMark) {
        self.element(container).class_list_add(mark.class_name());
    }
}

/// O contador de `AuxiliaryStatusbarPart.COUNTER`.
fn next_auxiliary_id() -> u32 {
    thread_local! {
        static COUNTER: Cell<u32> = const { Cell::new(1) };
    }
    COUNTER.with(|counter| {
        let id = counter.get();
        counter.set(id + 1);
        id
    })
}

type EntryRef = Rc<StatusbarViewModelEntry>;

/// `Map<string, T>` do JavaScript: `set` mantém a posição da chave que já existe.
fn map_set<T>(map: &mut Vec<(String, T)>, key: &str, value: T) {
    match map.iter_mut().find(|(candidate, _)| candidate == key) {
        Some(existing) => existing.1 = value,
        None => map.push((key.to_owned(), value)),
    }
}

fn map_get<'a, T>(map: &'a [(String, T)], key: &str) -> Option<&'a T> {
    map.iter().find(|(candidate, _)| candidate == key).map(|(_, value)| value)
}

#[derive(Default)]
struct PartState {
    element: Option<Rc<ViewElement>>,
    left_items_container: Option<Rc<ViewElement>>,
    right_items_container: Option<Rc<ViewElement>>,
    dimension: Option<(f64, f64)>,
    content_position: Option<(f64, f64)>,
    active_window_id: Option<u32>,
}

/// `StatusbarPart`.
pub struct StatusbarPart {
    this: Weak<StatusbarPart>,
    id: String,
    window: TargetWindow,
    theme: PartTheme,
    context_service: Rc<dyn IWorkspaceContextService>,
    layout_service: Rc<dyn IWorkbenchLayoutService>,
    context_menu_service: Rc<dyn IContextMenuService>,
    context_key_service: Rc<dyn IContextKeyService>,
    configuration_service: Rc<dyn IConfigurationService>,
    host_service: Rc<dyn IHostService>,
    command_service: Rc<dyn ICommandService>,
    view: Rc<dyn StatusbarPartView>,
    item_services: StatusbarItemServicesFactory,
    hover_delegate: Rc<dyn IHoverDelegate>,
    toggle_statusbar_visibility: Rc<dyn Fn()>,
    on_update_styles: Option<Rc<dyn Fn()>>,

    base: Rc<Disposable>,
    entry_dom: Rc<EntryDom>,
    view_model: Rc<StatusbarViewModel>,
    items: RefCell<Vec<(EntryRef, Rc<StatusbarEntryItem>)>>,
    pending_entries: RefCell<Vec<Rc<PendingStatusbarEntry>>>,
    on_did_change: Rc<Emitter<Option<ViewDimensions>>>,
    on_will_dispose: Rc<Emitter<()>>,
    on_did_override_entry: Rc<Emitter<String>>,
    entry_overrides: RefCell<Vec<(String, StatusbarEntryOverride)>>,
    compact_entries_disposable: Rc<MutableDisposable<DisposableStore>>,
    style_overrides: RefCell<Vec<Rc<StatusbarStyleOverride>>>,
    state: RefCell<PartState>,
}

impl StatusbarPart {
    pub const HEIGHT: f64 = 22.0;

    /// Vertical padding reserved around the main status bar under the floating panels experiment so its items remain centered. The
    /// part grows by this amount and the matching padding is applied in `floatingPanels.css`.
    pub const FLOATING_BOTTOM_PADDING: f64 = 6.0;
    pub const COMPACT_DENSITY_FLOATING_BOTTOM_PADDING: f64 = 4.0;

    pub const MINIMUM_WIDTH: f64 = 0.0;
    pub const MAXIMUM_WIDTH: f64 = f64::INFINITY;

    /// `new StatusbarPart(id, targetWindow, ...)` com `hasTitle: false`.
    pub fn new(id: String, deps: StatusbarPartDeps) -> Rc<Self> {
        let base = Disposable::new();
        let entry_dom = EntryDom::new(deps.view.clone());
        let view_model = StatusbarViewModel::new(deps.storage_service.clone(), entry_dom.clone());
        base.register(view_model.clone());

        let on_did_change: Rc<Emitter<Option<ViewDimensions>>> = Emitter::new();
        let on_will_dispose: Rc<Emitter<()>> = Emitter::new();
        let on_did_override_entry: Rc<Emitter<String>> = Emitter::new();
        base.register(on_did_change.clone());
        base.register(on_will_dispose.clone());
        base.register(on_did_override_entry.clone());
        let compact_entries_disposable = MutableDisposable::<DisposableStore>::new();
        base.register(compact_entries_disposable.clone());

        let this = Rc::new_cyclic(|this| Self {
            this: this.clone(),
            id,
            window: deps.window,
            theme: PartTheme::new(deps.theme_service.get_color_theme()),
            context_service: deps.context_service,
            layout_service: deps.layout_service,
            context_menu_service: deps.context_menu_service,
            context_key_service: deps.context_key_service,
            configuration_service: deps.configuration_service,
            host_service: deps.host_service,
            command_service: deps.command_service,
            view: deps.view,
            item_services: deps.item_services,
            hover_delegate: deps.hover_delegate,
            toggle_statusbar_visibility: deps.toggle_statusbar_visibility,
            on_update_styles: deps.on_update_styles,
            base,
            entry_dom,
            view_model,
            items: RefCell::new(Vec::new()),
            pending_entries: RefCell::new(Vec::new()),
            on_did_change,
            on_will_dispose,
            on_did_override_entry,
            entry_overrides: RefCell::new(Vec::new()),
            compact_entries_disposable,
            style_overrides: RefCell::new(Vec::new()),
            state: RefCell::new(PartState::default()),
        });

        // Part: `this._register(layoutService.registerPart(this))`.
        let registration = this.layout_service.register_part(this.clone());
        this.base.register(registration);

        // Themable: `this._register(themeService.onDidColorThemeChange(theme => this.onThemeChange(theme)))`.
        let weak = Rc::downgrade(&this);
        this.base.register(deps.theme_service.on_did_color_theme_change().listen(move |theme| {
            if let Some(this) = weak.upgrade() {
                this.on_theme_change(theme.clone());
            }
        }));

        this.register_listeners();
        this
    }

    /// `new MainStatusbarPart(...)`: o `id` é `Parts.STATUSBAR_PART` e a janela alvo é a principal (a que `deps.window` traz).
    pub fn new_main(deps: StatusbarPartDeps) -> Rc<Self> {
        Self::new(Parts::StatusbarPart.as_str().to_owned(), deps)
    }

    /// `new AuxiliaryStatusbarPart(container, ...)`: o `id` é `workbench.parts.auxiliaryStatus.<n>`, e a altura é [`Self::HEIGHT`].
    pub fn new_auxiliary(deps: StatusbarPartDeps) -> Rc<Self> {
        Self::new(format!("workbench.parts.auxiliaryStatus.{}", next_auxiliary_id()), deps)
    }

    /// `Part.onThemeChange`: só depois de `create` (`Themable.onThemeChange` troca o tema e atualiza os estilos).
    fn on_theme_change(&self, theme: Rc<dyn IColorTheme>) {
        if self.state.borrow().element.is_some() {
            self.theme.set(theme);
            self.update_styles();
        }
    }

    fn register_listeners(self: &Rc<Self>) {
        // Entry visibility changes
        let weak = Rc::downgrade(self);
        self.base.register(self.view_model.on_did_change_entry_visibility().listen(move |_| {
            if let Some(this) = weak.upgrade() {
                this.update_compact_entries();
            }
        }));

        // Workbench state changes
        let weak = Rc::downgrade(self);
        self.base.register(self.context_service.on_did_change_workbench_state().listen(move |_| {
            if let Some(this) = weak.upgrade() {
                this.update_styles();
            }
        }));
        let weak = Rc::downgrade(self);
        self.base.register(self.host_service.on_did_change_focus().listen(move |_| {
            if let Some(this) = weak.upgrade() {
                this.update_styles_if_created();
            }
        }));
        let weak = Rc::downgrade(self);
        self.base.register(self.host_service.on_did_change_active_window().listen(move |window_id| {
            if let Some(this) = weak.upgrade() {
                this.state.borrow_mut().active_window_id = Some(*window_id);
                this.update_styles_if_created();
            }
        }));

        // Floating panels changes the reserved bottom padding (and therefore the part height) for the main status bar only: signal
        // the grid that the size constraint changed.
        let weak = Rc::downgrade(self);
        self.base.register(self.configuration_service.on_did_change_configuration().listen(move |event: &Rc<ConfigurationChangeEvent>| {
            let Some(this) = weak.upgrade() else {
                return;
            };
            if this.get_id() == Parts::StatusbarPart.as_str()
                && (event.affects_configuration(LayoutSettings::ModernUi.as_str(), None) || event.affects_configuration(LayoutSettings::ModernUiDensity.as_str(), None))
            {
                this.on_did_change.fire(None);
                this.update_styles_if_created();
            }
        }));
    }

    fn update_styles_if_created(&self) {
        if self.state.borrow().element.is_some() {
            self.update_styles();
        }
    }

    // region IView

    fn floating_bottom_padding(&self) -> f64 {
        if self.get_id() != Parts::StatusbarPart.as_str() || !self.layout_service.is_floating_panels_enabled() {
            return 0.0;
        }
        if self.layout_service.is_modern_ui_compact() { Self::COMPACT_DENSITY_FLOATING_BOTTOM_PADDING } else { Self::FLOATING_BOTTOM_PADDING }
    }

    pub fn minimum_height(&self) -> f64 {
        Self::HEIGHT + self.floating_bottom_padding()
    }

    pub fn maximum_height(&self) -> f64 {
        Self::HEIGHT + self.floating_bottom_padding()
    }

    /// `onDidChange`: dispara `None` (o `undefined` do original) quando as restrições de tamanho mudam.
    pub fn on_did_change(&self) -> Event<Option<ViewDimensions>> {
        self.on_did_change.event()
    }

    // endregion

    /// `onWillDispose`.
    pub fn on_will_dispose(&self) -> Event<()> {
        self.on_will_dispose.event()
    }

    /// `onDidChangeEntryVisibility`.
    pub fn on_did_change_entry_visibility(&self) -> Event<crate::browser::parts::statusbar::statusbar_model::StatusbarEntryVisibilityChange> {
        self.view_model.on_did_change_entry_visibility()
    }

    /// `getId()`.
    pub fn get_id(&self) -> &str {
        &self.id
    }

    /// `getContainer()`.
    pub fn get_container(&self) -> Option<Rc<ViewElement>> {
        self.state.borrow().element.clone()
    }

    /// `getColor(id)` do `Themable`.
    fn get_color(&self, id: &str) -> Option<crate::browser::parts::part_support::ColorValue> {
        self.theme.get_color(id, None).map(crate::browser::parts::part_support::ColorValue::new)
    }

    pub fn overrides_entry(&self, id: &str) -> bool {
        self.entry_overrides.borrow().iter().any(|(candidate, _)| candidate == id)
    }

    pub fn override_entry(&self, id: &str, override_entry: StatusbarEntryOverride) -> DisposableRef {
        {
            let mut overrides = self.entry_overrides.borrow_mut();
            match overrides.iter_mut().find(|(candidate, _)| candidate == id) {
                Some(existing) => existing.1 = override_entry.clone(),
                None => overrides.push((id.to_owned(), override_entry.clone())),
            }
        }
        self.on_did_override_entry.fire(id.to_owned());

        let weak = self.this.clone();
        let id = id.to_owned();
        to_disposable(move || {
            let Some(this) = weak.upgrade() else {
                return;
            };
            let current_is_ours = this.entry_overrides.borrow().iter().find(|(candidate, _)| *candidate == id).is_some_and(|(_, current)| Rc::ptr_eq(current, &override_entry));
            if current_is_ours {
                this.entry_overrides.borrow_mut().retain(|(candidate, _)| *candidate != id);
                this.on_did_override_entry.fire(id.clone());
            }
        })
    }

    fn with_entry_override(&self, mut entry: StatusbarEntry, id: &str) -> StatusbarEntry {
        let override_entry = self.entry_overrides.borrow().iter().find(|(candidate, _)| candidate == id).map(|(_, value)| value.clone());
        if let Some(override_entry) = override_entry {
            override_entry(&mut entry);
        }
        entry
    }

    pub fn add_entry(&self, entry: StatusbarEntry, id: &str, alignment: StatusbarAlignment, priority_or_location: StatusbarEntryPriorityInput) -> Rc<dyn StatusbarEntryAccessor> {
        let priority = match priority_or_location {
            StatusbarEntryPriorityInput::Priority(priority) => priority,
            StatusbarEntryPriorityInput::Number(primary) => {
                StatusbarEntryPriority { primary: StatusbarEntryPrimaryPriority::Number(primary), secondary: Self::secondary_of(id) }
            }
            StatusbarEntryPriorityInput::Location(location) => {
                StatusbarEntryPriority { primary: StatusbarEntryPrimaryPriority::Location(location), secondary: Self::secondary_of(id) }
            }
        };

        // As long as we have not been created into a container yet, record all entries that are pending so that they can get created
        // at a later point
        if self.state.borrow().element.is_none() {
            return self.do_add_pending_entry(entry, id, alignment, priority);
        }

        // Otherwise add to view
        self.do_add_entry(entry, id, alignment, priority)
    }

    /// `hash(id)`: derive from identifier to accomplish uniqueness.
    fn secondary_of(id: &str) -> f64 {
        f64::from(hash(&Dynamic::string(id)))
    }

    fn do_add_pending_entry(&self, entry: StatusbarEntry, id: &str, alignment: StatusbarAlignment, priority: StatusbarEntryPriority) -> Rc<dyn StatusbarEntryAccessor> {
        let pending_entry = Rc::new(PendingStatusbarEntry { id: id.to_owned(), alignment, priority, entry: RefCell::new(entry), accessor: RefCell::new(None) });
        self.pending_entries.borrow_mut().push(pending_entry.clone());

        let update_pending = pending_entry.clone();
        let dispose_pending = pending_entry;
        let weak = self.this.clone();
        Rc::new(ClosureAccessor {
            update: Box::new(move |entry| {
                let accessor = update_pending.accessor.borrow().clone();
                match accessor {
                    Some(accessor) => accessor.update(entry),
                    None => *update_pending.entry.borrow_mut() = entry,
                }
            }),
            dispose: Box::new(move || {
                let accessor = dispose_pending.accessor.borrow().clone();
                match accessor {
                    Some(accessor) => accessor.dispose(),
                    None => {
                        if let Some(this) = weak.upgrade() {
                            this.pending_entries.borrow_mut().retain(|candidate| !Rc::ptr_eq(candidate, &dispose_pending));
                        }
                    }
                }
            }),
        })
    }

    fn do_add_entry(&self, entry: StatusbarEntry, id: &str, alignment: StatusbarAlignment, priority: StatusbarEntryPriority) -> Rc<dyn StatusbarEntryAccessor> {
        let disposables = DisposableStore::new();

        // View model item
        let item_container = self.do_create_status_item(id, alignment);
        let item = StatusbarEntryItem::new(
            (self.item_services)(),
            item_container.clone(),
            self.with_entry_override(entry.clone(), id),
            self.hover_delegate.clone(),
        );
        disposables.add_rc(item.clone());

        // View model entry
        let container_id = self.entry_dom.register(&item_container);
        let label_container_id = self.entry_dom.register(&item.label_container);
        let view_model_entry: EntryRef = Rc::new(StatusbarViewModelEntry {
            id: id.to_owned(),
            extension_id: entry.extension_id.clone(),
            name: item.name(),
            has_command: item.has_command(),
            alignment,
            priority,
            container: container_id,
            label_container: label_container_id,
        });
        self.items.borrow_mut().push((view_model_entry.clone(), item.clone()));

        // Add to view model
        let needs_full_refresh = self.do_add_or_remove_model_entry(&view_model_entry, true);
        if needs_full_refresh {
            self.append_statusbar_entries();
        } else {
            self.append_statusbar_entry(&view_model_entry);
        }

        let last_entry = Rc::new(RefCell::new(entry));
        let weak = self.this.clone();
        let update_entry: Rc<dyn Fn(StatusbarEntry)> = {
            let (weak, item, item_container, last_entry, id) = (weak.clone(), item, item_container.clone(), last_entry.clone(), id.to_owned());
            Rc::new(move |entry| {
                let Some(this) = weak.upgrade() else {
                    return;
                };
                *last_entry.borrow_mut() = entry.clone();
                let had_background_color = item_container.class_list_contains("has-background-color");
                item.update(this.with_entry_override(entry, &id));
                if had_background_color != item_container.class_list_contains("has-background-color") {
                    this.update_visible_background_color_neighbors();
                }
            })
        };

        // React to overrides
        {
            let (update_entry, last_entry, id) = (update_entry.clone(), last_entry, id.to_owned());
            disposables.add(self.on_did_override_entry.event().listen(move |override_entry_id| {
                if *override_entry_id == id {
                    let entry = last_entry.borrow().clone();
                    update_entry(entry);
                }
            }));
        }

        let dispose_entry = view_model_entry;
        let dispose_disposables = disposables;
        Rc::new(ClosureAccessor {
            update: Box::new(move |entry| update_entry(entry)),
            dispose: Box::new(move || {
                let Some(this) = weak.upgrade() else {
                    return;
                };
                let needs_full_refresh = this.do_add_or_remove_model_entry(&dispose_entry, false);
                if needs_full_refresh {
                    this.append_statusbar_entries();
                } else {
                    this.view.remove(&item_container);
                    this.update_compact_entries();
                }
                this.items.borrow_mut().retain(|(candidate, _)| !Rc::ptr_eq(candidate, &dispose_entry));
                this.entry_dom.unregister(dispose_entry.container);
                this.entry_dom.unregister(dispose_entry.label_container);
                dispose_disposables.dispose();
            }),
        })
    }

    fn do_create_status_item(&self, id: &str, alignment: StatusbarAlignment) -> Rc<ViewElement> {
        let item_container = ViewElement::with_classes(&["statusbar-item"]);
        self.view.set_id(&item_container, id);

        if alignment == StatusbarAlignment::Right {
            item_container.class_list_add("right");
        } else {
            item_container.class_list_add("left");
        }

        item_container
    }

    /// `doAddOrRemoveModelEntry(entry, add)`: devolve `needsFullRefresh`.
    fn do_add_or_remove_model_entry(&self, entry: &EntryRef, add: bool) -> bool {
        // Update model but remember previous entries
        let mut entries_before = self.view_model.entries();
        if add {
            self.view_model.add(entry.clone());
        } else {
            self.view_model.remove(entry);
        }
        let entries_after = self.view_model.entries();

        // Apply operation onto the entries from before
        if add {
            let index = entries_after.iter().position(|candidate| Rc::ptr_eq(candidate, entry)).unwrap_or(0).min(entries_before.len());
            entries_before.insert(index, entry.clone());
        } else if let Some(index) = entries_before.iter().position(|candidate| Rc::ptr_eq(candidate, entry)) {
            entries_before.remove(index);
        }

        // Figure out if a full refresh is needed by comparing arrays
        !equals(Some(entries_before.as_slice()), Some(entries_after.as_slice()), |a, b| Rc::ptr_eq(a, b))
    }

    pub fn is_entry_visible(&self, id: &str) -> bool {
        !self.view_model.is_hidden(id)
    }

    pub fn update_entry_visibility(&self, id: &str, visible: bool) {
        if visible {
            self.view_model.show(id);
        } else {
            self.view_model.hide(id);
        }
    }

    pub fn focus_next_entry(&self) {
        self.view_model.focus_next_entry();
    }

    pub fn focus_previous_entry(&self) {
        self.view_model.focus_previous_entry();
    }

    pub fn is_entry_focused(&self) -> bool {
        self.view_model.is_entry_focused()
    }

    pub fn focus(&self, preserve_entry_focus: bool) {
        if let Some(container) = self.get_container() {
            self.view.focus(&container);
        }
        let last_focused_entry = self.view_model.last_focused_entry();
        if let (true, Some(last_focused_entry)) = (preserve_entry_focus, last_focused_entry) {
            // Need a timeout, for some reason without it the inner label container will not get focused
            let view = self.view.clone();
            let label_container = self.entry_dom.element(last_focused_entry.label_container);
            let _ = TimeoutTimer::with_runner(move || view.focus(&label_container), 0);
        }
    }

    /// `Part.create(parent)` com o `createContentArea` da barra de status, seguido de `updateStyles`.
    pub fn create(&self, parent: &Rc<ViewElement>) {
        self.state.borrow_mut().element = Some(parent.clone());

        // Track focus within container
        let scoped_context_key_service = self.context_key_service.create_scoped(ViewElementTarget::new(parent.clone()));
        self.base.register(scoped_context_key_service.clone());
        status_bar_focused().bind_to(&*scoped_context_key_service).set(Dynamic::Bool(true));

        // Left items container
        let left_items_container = ViewElement::with_classes(&["left-items", "items-container"]);
        self.view.append_child(parent, &left_items_container);
        self.view.set_tab_index(parent, 0);

        // Right items container
        let right_items_container = ViewElement::with_classes(&["right-items", "items-container"]);
        self.view.append_child(parent, &right_items_container);

        {
            let mut state = self.state.borrow_mut();
            state.left_items_container = Some(left_items_container);
            state.right_items_container = Some(right_items_container);
        }

        // Context menu support
        let weak = self.this.clone();
        self.base.register(self.view.add_context_menu_listener(
            parent,
            Rc::new(move |event| {
                if let Some(this) = weak.upgrade() {
                    this.show_context_menu(event);
                }
            }),
        ));
        self.base.register(self.view.add_touch_target(parent));

        // Initial status bar entries
        self.create_initial_statusbar_entries();

        self.update_styles();
    }

    fn create_initial_statusbar_entries(&self) {
        // Add items in order according to alignment
        self.append_statusbar_entries();

        // Fill in pending entries if any
        loop {
            let pending = {
                let mut pending_entries = self.pending_entries.borrow_mut();
                if pending_entries.is_empty() {
                    break;
                }
                pending_entries.remove(0)
            };
            let entry = pending.entry.borrow().clone();
            let primary = match &pending.priority.primary {
                StatusbarEntryPrimaryPriority::Number(primary) => StatusbarEntryPriorityInput::Number(*primary),
                StatusbarEntryPrimaryPriority::Location(location) => StatusbarEntryPriorityInput::Location(location.clone()),
            };
            let accessor = self.add_entry(entry, &pending.id, pending.alignment, primary);
            *pending.accessor.borrow_mut() = Some(accessor);
        }
    }

    fn items_containers(&self) -> (Rc<ViewElement>, Rc<ViewElement>) {
        let state = self.state.borrow();
        (
            state.left_items_container.clone().expect("a parte foi criada"),
            state.right_items_container.clone().expect("a parte foi criada"),
        )
    }

    fn container_of(&self, entry: &StatusbarViewModelEntry) -> Rc<ViewElement> {
        self.entry_dom.element(entry.container)
    }

    fn append_statusbar_entries(&self) {
        let (left_items_container, right_items_container) = self.items_containers();

        // Clear containers
        self.view.clear_node(&left_items_container);
        self.view.clear_node(&right_items_container);

        // Append all
        let mut right_entries = self.view_model.get_entries(StatusbarAlignment::Right);
        right_entries.reverse(); // reversing due to flex: row-reverse
        for entry in self.view_model.get_entries(StatusbarAlignment::Left).into_iter().chain(right_entries) {
            let target = if entry.alignment == StatusbarAlignment::Left { &left_items_container } else { &right_items_container };

            self.view.append_child(target, &self.container_of(&entry));
        }

        // Update compact entries
        self.update_compact_entries();
    }

    fn append_statusbar_entry(&self, entry: &EntryRef) {
        let mut entries = self.view_model.get_entries(entry.alignment);

        if entry.alignment == StatusbarAlignment::Right {
            entries.reverse(); // reversing due to flex: row-reverse
        }

        let (left_items_container, right_items_container) = self.items_containers();
        let target = if entry.alignment == StatusbarAlignment::Left { left_items_container } else { right_items_container };

        let index = entries.iter().position(|candidate| Rc::ptr_eq(candidate, entry)).unwrap_or(0);
        if index + 1 == entries.len() {
            self.view.append_child(&target, &self.container_of(entry)); // append at the end if last
        } else {
            self.view.insert_before(&target, &self.container_of(entry), &self.container_of(&entries[index + 1])); // insert before next element otherwise
        }

        // Update compact entries
        self.update_compact_entries();
    }

    /// O `hasCommand` vivo do item da entrada (o getter do original).
    fn has_command(&self, entry: &EntryRef) -> bool {
        self.items.borrow().iter().find(|(candidate, _)| Rc::ptr_eq(candidate, entry)).map_or(entry.has_command, |(_, item)| item.has_command())
    }

    /// O `name` vivo do item da entrada (o getter do original).
    fn name_of(&self, entry: &EntryRef) -> String {
        self.items.borrow().iter().find(|(candidate, _)| Rc::ptr_eq(candidate, entry)).map_or_else(|| entry.name.clone(), |(_, item)| item.name())
    }

    fn update_compact_entries(&self) {
        let entries = self.view_model.entries();

        // Find visible entries and clear compact related CSS classes if any
        let mut map_id_to_visible_entry: Vec<(String, EntryRef)> = Vec::new();
        for entry in &entries {
            if !self.view_model.is_hidden(&entry.id) {
                map_set(&mut map_id_to_visible_entry, &entry.id, entry.clone());
            }

            let container = self.container_of(entry);
            container.class_list_remove("compact-left");
            container.class_list_remove("compact-right");
        }

        // Figure out groups of entries with `compact` alignment
        let mut compact_entry_groups: Vec<(String, Vec<(String, EntryRef)>)> = Vec::new();
        for (_, entry) in &map_id_to_visible_entry {
            // entry references another entry as location, and wants to be compact
            let Some(location) = entry.priority.primary.location().filter(|location| location.compact == Some(true)) else {
                continue;
            };
            let location_id = &location.location.id;
            let Some(location_entry) = map_get(&map_id_to_visible_entry, location_id) else {
                continue; // skip if location does not exist
            };

            // Build a map of entries that are compact among each other
            let mut group_index = compact_entry_groups.iter().position(|(key, _)| key == location_id);
            if group_index.is_none() {
                // It is possible that this entry references another entry that itself references an entry. In that case, we want to
                // add it to the entries of the referenced entry.
                group_index = compact_entry_groups.iter().position(|(_, group)| map_get(group, location_id).is_some());

                if group_index.is_none() {
                    compact_entry_groups.push((location_id.clone(), Vec::new()));
                    group_index = Some(compact_entry_groups.len() - 1);
                }
            }
            if let Some(index) = group_index {
                let group = &mut compact_entry_groups[index].1;
                map_set(group, &entry.id, entry.clone());
                map_set(group, &location_entry.id, location_entry.clone());
            }

            // Adjust CSS classes to move compact items closer together
            if location.alignment == StatusbarAlignment::Left {
                self.container_of(location_entry).class_list_add("compact-left");
                self.container_of(entry).class_list_add("compact-right");
            } else {
                self.container_of(location_entry).class_list_add("compact-right");
                self.container_of(entry).class_list_add("compact-left");
            }
        }

        // Install mouse listeners to update hover feedback for all compact entries that belong to each other
        let status_bar_item_hover_background = self.get_color(STATUS_BAR_ITEM_HOVER_BACKGROUND);
        let status_bar_item_compact_hover_background = self.get_color(STATUS_BAR_ITEM_COMPACT_HOVER_BACKGROUND);
        let compact_entries_store = DisposableStore::new();
        self.compact_entries_disposable.set(Some(compact_entries_store.clone()));
        if let (Some(hover_background), Some(compact_hover_background)) = (status_bar_item_hover_background, status_bar_item_compact_hover_background)
            && !is_high_contrast(self.theme.current().theme_type())
        {
            for (_, compact_entry_group) in &compact_entry_groups {
                let group_labels: Rc<Vec<Rc<ViewElement>>> =
                    Rc::new(compact_entry_group.iter().map(|(_, entry)| self.entry_dom.element(entry.label_container)).collect());
                for (_, compact_entry) in compact_entry_group {
                    if !self.has_command(compact_entry) {
                        continue; // only show hover feedback when we have a command
                    }

                    let label_container = self.entry_dom.element(compact_entry.label_container);
                    let (over_group, over_label, out_group) = (group_labels.clone(), label_container.clone(), group_labels.clone());
                    let (hover_css, compact_hover_css) = (hover_background.css(), compact_hover_background.css());
                    compact_entries_store.add(self.view.add_hover_listeners(
                        &label_container,
                        Rc::new(move || {
                            for label in over_group.iter() {
                                set_style_or_remove(label, "background-color", Some(&hover_css));
                            }
                            set_style_or_remove(&over_label, "background-color", Some(&compact_hover_css));
                        }),
                        Rc::new(move || {
                            for label in out_group.iter() {
                                set_style_or_remove(label, "background-color", None);
                            }
                        }),
                    ));
                }
            }
        }

        self.update_visible_background_color_neighbors();
    }

    fn update_visible_background_color_neighbors(&self) {
        self.do_update_visible_background_color_neighbors(self.view_model.get_entries(StatusbarAlignment::Left), StatusbarAlignment::Left);
        let mut right_entries = self.view_model.get_entries(StatusbarAlignment::Right);
        right_entries.reverse();
        self.do_update_visible_background_color_neighbors(right_entries, StatusbarAlignment::Right);
    }

    fn do_update_visible_background_color_neighbors(&self, entries: Vec<EntryRef>, alignment: StatusbarAlignment) {
        let mut previous_visible_entry: Option<EntryRef> = None;

        for entry in entries {
            let container = self.container_of(&entry);
            container.class_list_remove("visible-background-color-neighbor");

            if self.view_model.is_hidden(&entry.id) {
                continue;
            }

            let previous_container = previous_visible_entry.as_ref().map(|previous| self.container_of(previous));
            let previous_has = |class_name: &str| previous_container.as_ref().is_some_and(|previous| previous.class_list_contains(class_name));
            let is_compact_neighbor = if alignment == StatusbarAlignment::Left {
                previous_has("compact-right") && container.class_list_contains("compact-left")
            } else {
                previous_has("compact-left") && container.class_list_contains("compact-right")
            };
            if previous_has("has-background-color") && container.class_list_contains("has-background-color") && !is_compact_neighbor {
                container.class_list_add("visible-background-color-neighbor");
            }

            previous_visible_entry = Some(entry);
        }
    }

    fn show_context_menu(&self, event: &StatusbarContextMenuEvent) {
        let actions: Rc<RefCell<Vec<ActionRef>>> = Rc::new(RefCell::new(Vec::new()));
        let delegate = StatusbarContextMenuDelegate {
            anchor: AnchorTarget::Mouse { posx: event.posx, posy: event.posy },
            part: self.this.clone(),
            target: event.target.clone(),
            actions: actions.clone(),
        };
        self.context_menu_service.show_context_menu(ContextMenuRequest::Delegate(Rc::new(delegate)));
    }

    fn get_context_menu_actions(&self, target: Option<&Rc<ViewElement>>) -> Vec<ActionRef> {
        let mut actions: Vec<ActionRef> = Vec::new();

        // Provide an action to hide the status bar at last
        let toggle_statusbar_visibility = self.toggle_statusbar_visibility.clone();
        actions.push(to_action(ToActionProps {
            id: TOGGLE_STATUSBAR_VISIBILITY_ACTION_ID.to_owned(),
            label: localize("hideStatusBar", "Hide Status Bar", &[]),
            tooltip: None,
            enabled: None,
            checked: None,
            class: None,
            alt: None,
            run: Rc::new(move |_| {
                toggle_statusbar_visibility();
                MaybePromise::Value(Dynamic::Undefined)
            }),
        }));
        actions.push(Separator::new());

        // Show an entry per known status entry
        // Note: even though entries have an identifier, there can be multiple entries having the same identifier (e.g. from
        // extensions). So we make sure to only show a single entry per identifier we handled.
        let mut handled_entries: Vec<String> = Vec::new();
        for entry in self.view_model.entries() {
            if !handled_entries.contains(&entry.id) {
                actions.push(self.toggle_entry_visibility_action(&entry.id, &self.name_of(&entry)));
                handled_entries.push(entry.id.clone());
            }
        }

        // Figure out if mouse is over an entry
        let mut status_entry_under_mouse: Option<EntryRef> = None;
        let mut element = target.cloned();
        while let Some(current) = element {
            if let Some(entry) = self.entry_dom.id_of(&current).and_then(|id| self.view_model.find_entry(id)) {
                status_entry_under_mouse = Some(entry);
                break;
            }
            element = current.parent_element();
        }

        if let Some(entry) = status_entry_under_mouse {
            actions.push(Separator::new());
            if let Some(extension_id) = &entry.extension_id {
                actions.push(self.manage_extension_action(extension_id));
            }
            actions.push(self.hide_entry_action(&entry.id, &self.name_of(&entry)));
        }

        actions
    }

    /// `ToggleStatusbarEntryVisibilityAction`.
    fn toggle_entry_visibility_action(&self, id: &str, label: &str) -> ActionRef {
        let model = self.view_model.clone();
        let action_id = id.to_owned();
        to_action(ToActionProps {
            id: id.to_owned(),
            label: label.to_owned(),
            tooltip: None,
            enabled: Some(true),
            checked: Some(!self.view_model.is_hidden(id)),
            class: None,
            alt: None,
            run: Rc::new(move |_| {
                if model.is_hidden(&action_id) {
                    model.show(&action_id);
                } else {
                    model.hide(&action_id);
                }
                MaybePromise::Value(Dynamic::Undefined)
            }),
        })
    }

    /// `HideStatusbarEntryAction`.
    fn hide_entry_action(&self, id: &str, name: &str) -> ActionRef {
        let model = self.view_model.clone();
        let action_id = id.to_owned();
        to_action(ToActionProps {
            id: id.to_owned(),
            label: localize("hide", "Hide '{0}'", &[name]),
            tooltip: None,
            enabled: Some(true),
            checked: None,
            class: None,
            alt: None,
            run: Rc::new(move |_| {
                model.hide(&action_id);
                MaybePromise::Value(Dynamic::Undefined)
            }),
        })
    }

    /// `ManageExtensionAction`.
    fn manage_extension_action(&self, extension_id: &str) -> ActionRef {
        let command_service = self.command_service.clone();
        let extension_id = extension_id.to_owned();
        to_action(ToActionProps {
            id: "statusbar.manage.extension".to_owned(),
            label: localize("manageExtension", "Manage Extension", &[]),
            tooltip: None,
            enabled: None,
            checked: None,
            class: None,
            alt: None,
            run: Rc::new(move |_| MaybePromise::Promise(command_service.execute_command("_extensions.manage", vec![Dynamic::string(&extension_id)]))),
        })
    }

    /// `updateStyles()`: o fundo, o primeiro plano, a sombra, a borda e a folha de estilo dinâmica.
    pub fn update_styles(&self) {
        if let Some(hook) = &self.on_update_styles {
            hook();
        }

        let container = self.get_container().expect("a parte foi criada");
        let style_override: Option<Rc<StatusbarStyleOverride>> =
            self.style_overrides.borrow().iter().min_by(|a, b| a.priority.total_cmp(&b.priority)).cloned();
        let override_background = style_override.as_ref().and_then(|style| style.background.clone()).filter(|background| !background.is_empty());

        // Background / foreground colors
        let has_folder = self.context_service.get_workbench_state() != WorkbenchState::Empty;
        let background = override_background.clone().unwrap_or_else(|| (if has_folder { STATUS_BAR_BACKGROUND } else { STATUS_BAR_NO_FOLDER_BACKGROUND }).to_owned());

        // The inactive background is a resting state only: a style override (e.g. while debugging) and the no folder background both
        // communicate state and must stay visible when inactive.
        let active_window_id = self.state.borrow().active_window_id;
        let is_inactive = is_part_inactive(active_window_id, &self.window, self.host_service.as_ref());
        let inactive_background = if is_inactive && override_background.is_none() && has_folder { self.get_color(STATUS_BAR_INACTIVE_BACKGROUND) } else { None };
        let background_color = inactive_background.or_else(|| self.get_color(&background));
        let background_css = background_color.as_ref().map(|color| color.css());
        set_style_or_remove(&container, "background-color", background_css.as_deref());
        let box_shadow = (self.get_id() == Parts::StatusbarPart.as_str()
            && self.layout_service.is_floating_panels_enabled()
            && !is_high_contrast(self.theme.current().theme_type()))
        .then(|| background_css.as_ref().map(|css| format!("0px 1px 0px {css}")))
        .flatten();
        set_style_or_remove(&container, "box-shadow", box_shadow.as_deref());
        let foreground_id = style_override
            .as_ref()
            .and_then(|style| style.foreground.clone())
            .unwrap_or_else(|| (if has_folder { STATUS_BAR_FOREGROUND } else { STATUS_BAR_NO_FOLDER_FOREGROUND }).to_owned());
        let foreground_color = self.get_color(&foreground_id).map(|color| color.css());
        set_style_or_remove(&container, "color", foreground_color.as_deref());
        let item_border_color = self.get_color(STATUS_BAR_ITEM_FOCUS_BORDER);

        // Update compact entries to refresh hover colors based on current theme
        self.update_compact_entries();

        // Border color
        let border_id = style_override
            .as_ref()
            .and_then(|style| style.border.clone())
            .unwrap_or_else(|| (if has_folder { STATUS_BAR_BORDER } else { STATUS_BAR_NO_FOLDER_BORDER }).to_owned());
        let border_color = self.get_color(&border_id).or_else(|| self.get_color(CONTRAST_BORDER));
        match &border_color {
            Some(border_color) => {
                container.class_list_add("status-border-top");
                container.style_set_property("--status-border-top-color", &border_color.raw());
            }
            None => {
                container.class_list_remove("status-border-top");
                container.style_remove_property("--status-border-top-color");
            }
        }

        // Colors and focus outlines via dynamic stylesheet
        let status_bar_focus_color = self.get_color(STATUS_BAR_FOCUS_BORDER);
        // `${null}` e `${undefined}` do template literal.
        let raw_or_null = |color: &Option<crate::browser::parts::part_support::ColorValue>| color.as_ref().map_or_else(|| "null".to_owned(), |color| color.raw());
        let item_outline_color = self.get_color(ACTIVE_CONTRAST_BORDER).or(item_border_color);
        let beak_color = border_color.as_ref().map(|color| color.raw()).unwrap_or_else(|| background_color.as_ref().map(|color| color.raw()).unwrap_or_default());
        self.view.set_style_sheet(
            &container,
            &format!(
                "\n\n\t\t\t\t/* Status bar focus outline */\n\t\t\t\t.monaco-workbench .part.statusbar:focus {{\n\t\t\t\t\toutline-color: {};\n\t\t\t\t}}\n\n\t\t\t\t/* Status bar item focus outline */\n\t\t\t\t.monaco-workbench .part.statusbar > .items-container > .statusbar-item a:focus-visible {{\n\t\t\t\t\toutline: 1px solid {};\n\t\t\t\t\toutline-offset: {};\n\t\t\t\t}}\n\n\t\t\t\t/* Notification Beak */\n\t\t\t\t.monaco-workbench .part.statusbar > .items-container > .statusbar-item.has-beak > .status-bar-item-beak-container:before {{\n\t\t\t\t\tborder-bottom-color: {};\n\t\t\t\t}}\n\t\t\t",
                raw_or_null(&status_bar_focus_color),
                raw_or_null(&item_outline_color),
                if border_color.is_some() { "-2px" } else { "-1px" },
                beak_color,
            ),
        );
    }

    /// `layout(width, height, top, left)`: guarda a dimensão e dimensiona a área de conteúdo (a parte não tem título).
    pub fn layout(&self, width: f64, height: f64, top: f64, left: f64) {
        let element = {
            let mut state = self.state.borrow_mut();
            state.dimension = Some((width, height));
            state.content_position = Some((top, left));
            state.element.clone()
        };
        if let Some(element) = element {
            element.set_size(width, height);
        }
    }

    pub fn override_style(&self, style: Rc<StatusbarStyleOverride>) -> DisposableRef {
        self.style_overrides.borrow_mut().push(style.clone());
        self.update_styles();

        let weak = self.this.clone();
        to_disposable(move || {
            if let Some(this) = weak.upgrade() {
                this.style_overrides.borrow_mut().retain(|candidate| !Rc::ptr_eq(candidate, &style));
                this.update_styles();
            }
        })
    }

    /// `toJSON()`.
    pub fn to_json(&self) -> Parts {
        Parts::StatusbarPart
    }
}

impl LayoutPart for StatusbarPart {
    fn get_id(&self) -> String {
        self.id.clone()
    }
}

impl IDisposable for StatusbarPart {
    fn dispose(&self) {
        self.on_will_dispose.fire(());

        self.base.dispose();
    }
}

/// O delegado do `showContextMenu`: as ações saem de `getActions` e a âncora é o evento do ponteiro.
struct StatusbarContextMenuDelegate {
    anchor: AnchorTarget,
    part: Weak<StatusbarPart>,
    target: Option<Rc<ViewElement>>,
    actions: Rc<RefCell<Vec<ActionRef>>>,
}

impl IContextMenuDelegate for StatusbarContextMenuDelegate {
    fn get_anchor(&self) -> AnchorTarget {
        self.anchor.clone()
    }

    fn get_actions(&self) -> Vec<ActionRef> {
        let actions = self.part.upgrade().map(|part| part.get_context_menu_actions(self.target.as_ref())).unwrap_or_default();
        *self.actions.borrow_mut() = actions.clone();
        actions
    }

    fn on_hide(&self, _did_cancel: bool) {
        // `disposeIfDisposable(actions)`: as ações são `to_action`, sem nada a descartar.
        self.actions.borrow_mut().clear();
    }
}
