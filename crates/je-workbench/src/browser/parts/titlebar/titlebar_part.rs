//! Porte de `src/vs/workbench/browser/parts/titlebar/titlebarPart.ts`: a `BrowserTitlebarPart` (as três áreas, o título e o
//! centro de comandos, a barra de menus customizada, as barras de ferramentas ao lado do centro, de atualização e de ações, os
//! controles da janela, as cores, o transbordo das barras opcionais, o zoom e a altura), a `MainBrowserTitlebarPart` e a
//! `AuxiliaryBrowserTitlebarPart`.
//!
//! Divergências:
//! - **DOM vira estado nativo.** Os elementos são [`ViewElement`] (classes, estilo, pai, `clientWidth` e `scrollWidth`). O que o
//!   original faz com o DOM e o `ViewElement` não modela (ordem dos filhos, atributos, texto do título, ouvintes do menu de
//!   contexto, zoom, janela de controles do navegador, agendamento no quadro de animação) entra pelo trait [`TitlebarPartView`].
//!   As cores inline guardam o texto que o CSSOM devolveria (`rgb(...)`); a propriedade personalizada `--modern-ui-shell-background`
//!   guarda o `toString()` da cor e é escrita no contêiner da janela pelo [`ContainerStyleHost`].
//! - **`getComputedStyle`.** O que os testes leem do estilo calculado das regras de `titlebarpart.css` (a opacidade do
//!   `.titlebar-container` em [`titlebar_container_opacity`] e a cor dos ícones do `.action-toolbar-container` em
//!   [`titlebar_action_toolbar_foreground`]) são resolvedores sobre as classes da parte e as cores do tema.
//! - **`Part` (`part.ts`) e `Themable`.** A base não foi portada como classe: o registro no layout, o `onDidChange`, o `layout` sem
//!   título (a área de conteúdo é a própria parte), a reação ao tema só depois de `create` e o `getColor` vivem aqui.
//! - **`createContentArea` sobrescrito dos testes.** É o [`TitlebarContentArea::ElementOnly`]: o elemento é o contêiner e nada
//!   mais é criado. `Reflect.set(this, 'leftContent', ...)` é [`TitlebarPart::set_left_content`], e o `uninstallMenubar` privado,
//!   alcançado pelo `Reflect.get` do teste, é [`TitlebarPart::uninstall_menubar`].
//! - **`updateTitleBarToolBarOverflow` privado.** O teste o chama sobre um objeto que só tem os três elementos; é a função
//!   [`update_title_bar_tool_bar_overflow`], que o método da parte chama.
//! - **Barras de ferramentas e centro de comandos** (`MenuWorkbenchToolBar`, `WorkbenchToolBar`, `CommandCenterControl`, os menus
//!   do título e o `actionViewItemProvider`) são o trait [`TitlebarToolbars`], chamado nos mesmos pontos do original. A
//!   `CustomMenubarControl` é o [`TitlebarMenubar`] criado pela [`TitlebarMenubarFactory`], e o `WindowTitle` é o
//!   [`TitlebarWindowTitle`]. O `IEditorGroupsContainer` é o [`TitlebarEditorGroups`] no que a parte lê (as opções e o evento).
//! - **Plataforma.** `isMacintosh`, `isWindows`, `isLinux`, `isWeb` e `isNative` são o [`Platform`]; `hasCustomTitlebar`,
//!   `hasNativeTitlebar`, `hasNativeMenu`, `isWCOEnabled` e `getMenuBarVisibility` são do [`TitleBarHost`].
//! - **`appIconBadge`** nunca é criado no `createContentArea` do original e fica de fora.
//! - **`BrowserTitleService`** depende do `MultiWindowParts` (não portado) e não entra aqui.

use std::cell::RefCell;
use std::rc::{Rc, Weak};

use je_base::dynamic::Dynamic;
use je_base::event::{Emitter, Event};
use je_base::lifecycle::{Disposable, DisposableRef, DisposableStore, IDisposable, MutableDisposable};
use je_base::platform::Platform;
use je_editor::core_2d::dimension::Dimension;
use je_editor::services::theme_service::{IColorTheme, IThemeService};
use je_platform::configuration::models::ConfigurationChangeEvent;
use je_platform::configuration::service::IConfigurationService;
use je_platform::contextkey::contextkey::{IContextKey, IContextKeyService};
use je_platform::contextview::view_element::ViewElement;
use je_platform::window::DEFAULT_CUSTOM_TITLEBAR_HEIGHT;
use je_theme::color::Color;
use je_widgets::gridview::ViewDimensions;

use crate::browser::parts::part_support::{ColorValue, ContainerStyleHost, PartTheme, TargetWindow, is_part_inactive, set_style_or_remove, workbench_background};
use crate::browser::parts::titlebar::menubar_control::{MENU_BAR_VISIBILITY_SETTING, MenuBarVisibility};
use crate::common::contextkeys::is_compact_title_bar_context;
use crate::services::editor::editor_groups_service::{EditorActionsLocation, EditorPartOptions, EditorPartOptionsChangeEvent, ShowTabs};
use crate::services::host::host::IHostService;
use crate::services::layout::layout_service::{ActivityBarPosition, ILayoutService, IWorkbenchLayoutService, LayoutPart, LayoutSettings, MAIN_WINDOW, Parts, TitleBarHost};

// As cores de `workbench/common/theme.ts` que a parte lê.
pub const TITLE_BAR_ACTIVE_FOREGROUND: &str = "titleBar.activeForeground";
pub const TITLE_BAR_INACTIVE_FOREGROUND: &str = "titleBar.inactiveForeground";
pub const TITLE_BAR_ACTIVE_BACKGROUND: &str = "titleBar.activeBackground";
pub const TITLE_BAR_INACTIVE_BACKGROUND: &str = "titleBar.inactiveBackground";
pub const MODERN_UI_SHELL_BACKGROUND: &str = "modernUI.shellBackground";
pub const MODERN_UI_INACTIVE_SHELL_BACKGROUND: &str = "modernUI.inactiveShellBackground";
pub const TITLE_BAR_BORDER: &str = "titleBar.border";

/// A propriedade personalizada do contêiner da janela que `updateStyles` escreve.
pub const MODERN_UI_SHELL_BACKGROUND_PROPERTY: &str = "--modern-ui-shell-background";

/// `ITitleVariable`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TitleVariable {
    pub name: String,
    pub context_key: String,
}

/// `ITitleProperties`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TitleProperties {
    pub is_pure: Option<bool>,
    pub is_admin: Option<bool>,
    pub prefix: Option<String>,
}

/// O `WindowTitle` no que a parte lê: o título, o evento de mudança e as variáveis.
pub trait TitlebarWindowTitle: IDisposable {
    /// `value`.
    fn value(&self) -> String;

    /// `onDidChange`.
    fn on_did_change(&self) -> Event<()>;

    /// `updateProperties(properties)`.
    fn update_properties(&self, properties: &TitleProperties);

    /// `registerVariables(variables)`.
    fn register_variables(&self, variables: &[TitleVariable]);
}

/// A `CustomMenubarControl` no que a parte lê.
pub trait TitlebarMenubar: IDisposable {
    /// `onVisibilityChange`.
    fn on_visibility_change(&self) -> Event<bool>;

    /// `onFocusStateChange`.
    fn on_focus_state_change(&self) -> Event<bool>;

    /// `create(parent)`.
    fn create(&self, parent: &Rc<ViewElement>);

    /// `layout(dimension)`.
    fn layout(&self, dimension: Dimension);

    /// `toggleFocus()`.
    fn toggle_focus(&self);
}

/// `instantiationService.createInstance(CustomMenubarControl)`.
pub type TitlebarMenubarFactory = Rc<dyn Fn() -> Rc<dyn TitlebarMenubar>>;

/// O `IEditorGroupsContainer` no que a parte lê: o evento das opções da parte e as opções.
pub trait TitlebarEditorGroups {
    /// `onDidChangeEditorPartOptions`.
    fn on_did_change_editor_part_options(&self) -> Event<EditorPartOptionsChangeEvent>;

    /// `partOptions`.
    fn part_options(&self) -> EditorPartOptions;
}

/// O que `createActionToolBarMenus(update)` atualiza (`true` é tudo).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ActionToolbarMenusUpdate {
    pub editor_actions: bool,
    pub layout_actions: bool,
    pub global_actions: bool,
    pub activity_actions: bool,
}

impl ActionToolbarMenusUpdate {
    /// `update === true`.
    pub fn all() -> Self {
        Self { editor_actions: true, layout_actions: true, global_actions: true, activity_actions: true }
    }
}

/// As barras de ferramentas e o centro de comandos da barra de título.
pub trait TitlebarToolbars {
    /// A barra do `MenuId.TitleBarAdjacentCenter` em `element`; `on_did_change_menu_items` reavalia o transbordo.
    fn create_center_adjacent_toolbar(&self, element: &Rc<ViewElement>, on_did_change_menu_items: Rc<dyn Fn()>) -> DisposableRef;

    /// A barra do `MenuId.TitleBarUpdate` em `element`; `on_did_change_menu_items` reavalia o transbordo.
    fn create_update_toolbar(&self, element: &Rc<ViewElement>, on_did_change_menu_items: Rc<dyn Fn()>) -> DisposableRef;

    /// `createActionToolBar()` sobre `element` (recria a barra de ações).
    fn create_action_toolbar(&self, element: &Rc<ViewElement>);

    /// `createActionToolBarMenus(update)`.
    fn create_action_toolbar_menus(&self, update: ActionToolbarMenusUpdate);

    /// `createInstance(CommandCenterControl, windowTitle, hoverDelegate)` com `reset(this.title, commandCenter.element)`.
    fn create_command_center(&self, title: &Rc<ViewElement>) -> DisposableRef;
}

/// Qual menu de contexto o clique abre (`MenuId.TitleBarContext` e `MenuId.TitleBarTitleContext`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TitlebarContextMenu {
    TitleBarContext,
    TitleBarTitleContext,
}

/// O `MouseEvent` do menu de contexto: o alvo, para decidir se foi sobre o título, e a posição do ponteiro.
pub struct TitlebarContextMenuEvent {
    pub target: Option<Rc<ViewElement>>,
    pub posx: f64,
    pub posy: f64,
}

/// Tudo que a parte faz com o DOM além do que o [`ViewElement`] guarda.
pub trait TitlebarPartView {
    /// `parent.appendChild(child)`.
    fn append_child(&self, parent: &Rc<ViewElement>, child: &Rc<ViewElement>);

    /// `parent.prepend(child)`.
    fn prepend(&self, parent: &Rc<ViewElement>, child: &Rc<ViewElement>);

    /// `child.remove()`.
    fn remove(&self, child: &Rc<ViewElement>);

    /// `element.setAttribute(name, value)`.
    fn set_attribute(&self, element: &Rc<ViewElement>, name: &str, value: &str);

    /// `element.textContent = text`.
    fn set_text_content(&self, element: &Rc<ViewElement>, text: &str);

    /// `reset(element)`: tira os filhos do elemento.
    fn reset(&self, element: &Rc<ViewElement>);

    /// `getZoomFactor(window)`.
    fn zoom_factor(&self) -> f64;

    /// `getWCOTitlebarAreaRect(window)?.height`.
    fn wco_titlebar_area_height(&self) -> Option<f64>;

    /// Se o idioma da plataforma é da direita para a esquerda (`new Intl.Locale(platformLocale).textInfo.direction === 'rtl'`).
    fn is_rtl_locale(&self) -> bool;

    /// `addDisposableListener(rootContainer, CONTEXT_MENU, ...)` (e, no macOS, o `mousedown` com `metaKey` no título): chama o
    /// tratador com o menu a abrir e o evento. O ouvinte já parou o evento (`EventHelper.stop`).
    fn add_context_menu_listener(&self, root_container: &Rc<ViewElement>, title: &Rc<ViewElement>, handler: Rc<dyn Fn(TitlebarContextMenu, &TitlebarContextMenuEvent)>) -> DisposableRef;

    /// `this.element.querySelector('[tabindex]:not([tabindex="-1"])')?.focus()`.
    fn focus_first_tabbable(&self, element: &Rc<ViewElement>);

    /// `contextMenuService.showContextMenu({ getAnchor: () => event, menuId, contextKeyService, domForShadowRoot })`.
    fn show_context_menu(&self, menu: TitlebarContextMenu, event: &TitlebarContextMenuEvent);

    /// O `AnimationFrameScheduler(rootContainer, run).schedule()`: roda `run` no próximo quadro, uma vez por vários pedidos.
    fn schedule_tool_bar_overflow_update(&self, root_container: &Rc<ViewElement>, run: Rc<dyn Fn()>);
}

/// O `createContentArea` da parte.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TitlebarContentArea {
    /// O `createContentArea` da `BrowserTitlebarPart`: contêineres, barra de menus, título, barras de ferramentas e controles.
    Full,
    /// O `createContentArea` sobrescrito dos testes: `this.element = parent; return parent;`.
    ElementOnly,
}

/// As dependências injetadas no construtor de `BrowserTitlebarPart` (e de `MainBrowserTitlebarPart`).
pub struct TitlebarPartDeps {
    pub window: TargetWindow,
    pub editor_groups: Rc<dyn TitlebarEditorGroups>,
    pub configuration_service: Rc<dyn IConfigurationService>,
    pub theme_service: Rc<dyn IThemeService>,
    pub layout_service: Rc<dyn IWorkbenchLayoutService>,
    pub context_key_service: Rc<dyn IContextKeyService>,
    pub host_service: Rc<dyn IHostService>,
    pub title_bar_host: Rc<dyn TitleBarHost>,
    pub platform: Platform,
    pub view: Rc<dyn TitlebarPartView>,
    pub container_styles: Rc<dyn ContainerStyleHost>,
    pub menubar_factory: TitlebarMenubarFactory,
    pub toolbars: Rc<dyn TitlebarToolbars>,
    pub window_title: Rc<dyn TitlebarWindowTitle>,
    pub content_area: TitlebarContentArea,
    /// A barra de título principal que a auxiliar consulta em `preventZoom` (`mainTitlebar`).
    pub main_titlebar: Option<Weak<TitlebarPart>>,
}

#[derive(Default)]
struct TitlebarState {
    element: Option<Rc<ViewElement>>,
    root_container: Option<Rc<ViewElement>>,
    window_controls_container: Option<Rc<ViewElement>>,
    drag_region: Option<Rc<ViewElement>>,
    title: Option<Rc<ViewElement>>,
    title_text: String,
    left_content: Option<Rc<ViewElement>>,
    center_content: Option<Rc<ViewElement>>,
    right_content: Option<Rc<ViewElement>>,
    app_icon: Option<Rc<ViewElement>>,
    menubar: Option<Rc<ViewElement>>,
    action_toolbar_element: Option<Rc<ViewElement>>,
    center_adjacent_toolbar_element: Option<Rc<ViewElement>>,
    update_toolbar_element: Option<Rc<ViewElement>>,
    last_layout_dimensions: Option<Dimension>,
    active_window_id: Option<u32>,
    is_compact: bool,
    dimension: Option<(f64, f64)>,
}

/// `updateTitleBarToolBarOverflow()`: esconde as barras opcionais quando mostrá-las empurraria os controles da janela para fora
/// da tela (#303222). A barra ao lado do centro some primeiro, a de atualização só se ainda transbordar.
pub fn update_title_bar_tool_bar_overflow(root_container: &ViewElement, center_adjacent_tool_bar_element: Option<&Rc<ViewElement>>, update_tool_bar_element: Option<&Rc<ViewElement>>) {
    let center_adjacent_tool_bar = center_adjacent_tool_bar_element.filter(|element| !element.class_list_contains("has-no-actions"));
    let update_tool_bar = update_tool_bar_element.filter(|element| !element.class_list_contains("has-no-actions"));

    if let Some(element) = center_adjacent_tool_bar_element {
        element.class_list_remove("overflowing");
    }
    if let Some(element) = update_tool_bar_element {
        element.class_list_remove("overflowing");
    }

    if root_container.scroll_width() <= root_container.client_width() {
        return;
    }

    if let Some(element) = center_adjacent_tool_bar {
        element.class_list_add("overflowing");
    }
    if root_container.scroll_width() > root_container.client_width() {
        if let Some(element) = update_tool_bar {
            element.class_list_add("overflowing");
        }
    }
}

/// O `opacity` calculado do `.titlebar-container`: `.part.titlebar.inactive > * { opacity: 0.6 }` e, com a barra de menus em
/// foco, `.part.titlebar.inactive.menubar-focused > .titlebar-container { opacity: 1 }`.
pub fn titlebar_container_opacity(titlebar: &ViewElement) -> f64 {
    if titlebar.class_list_contains("inactive") && !titlebar.class_list_contains("menubar-focused") { 0.6 } else { 1.0 }
}

/// A cor calculada dos ícones (`.codicon`) do `.action-toolbar-container`: o `.action-toolbar-container` pinta
/// `var(--vscode-titleBar-activeForeground)` (ou a `inactiveForeground` na barra `.inactive`) e o `.codicon` herda (`color: inherit`).
/// Sem a variável do tema a declaração é inválida em tempo de cálculo e a propriedade herdada vale a `inherited` (a cor do
/// `.monaco-workbench`).
pub fn titlebar_action_toolbar_foreground(theme: &dyn IColorTheme, titlebar: &ViewElement, inherited: Option<Color>) -> Option<Color> {
    let id = if titlebar.class_list_contains("inactive") { TITLE_BAR_INACTIVE_FOREGROUND } else { TITLE_BAR_ACTIVE_FOREGROUND };
    theme.get_color(id, None).or(inherited)
}

/// `makeOpaque(color, theme)` do `updateStyles`: as cores do título são opacas porque a parte define a própria camada da GPU
/// (renderização LCD), então a cor translúcida é composta sobre o fundo do workbench.
fn make_opaque(color: Color, theme: &dyn IColorTheme) -> Color {
    if color.is_opaque() { color } else { color.make_opaque(&workbench_background(theme)) }
}

/// `BrowserTitlebarPart`.
pub struct TitlebarPart {
    this: Weak<TitlebarPart>,
    id: String,
    window: TargetWindow,
    is_auxiliary: bool,
    theme: PartTheme,
    editor_groups: Rc<dyn TitlebarEditorGroups>,
    configuration_service: Rc<dyn IConfigurationService>,
    layout_service: Rc<dyn IWorkbenchLayoutService>,
    host_service: Rc<dyn IHostService>,
    title_bar_host: Rc<dyn TitleBarHost>,
    platform: Platform,
    view: Rc<dyn TitlebarPartView>,
    container_styles: Rc<dyn ContainerStyleHost>,
    menubar_factory: TitlebarMenubarFactory,
    toolbars: Rc<dyn TitlebarToolbars>,
    content_area: TitlebarContentArea,
    main_titlebar: Option<Weak<TitlebarPart>>,
    is_compact_context_key: Rc<dyn IContextKey>,
    pub window_title: Rc<dyn TitlebarWindowTitle>,

    base: Rc<Disposable>,
    on_menubar_visibility_change: Rc<Emitter<bool>>,
    on_will_dispose: Rc<Emitter<()>>,
    on_did_change: Rc<Emitter<Option<ViewDimensions>>>,
    custom_menubar: Rc<MutableDisposable<dyn TitlebarMenubar>>,
    custom_menubar_disposables: Rc<DisposableStore>,
    center_adjacent_tool_bar_disposable: Rc<DisposableStore>,
    update_tool_bar_disposable: Rc<DisposableStore>,
    title_disposables: Rc<DisposableStore>,
    state: RefCell<TitlebarState>,
}

impl TitlebarPart {
    pub const MINIMUM_WIDTH: f64 = 0.0;
    pub const MAXIMUM_WIDTH: f64 = f64::INFINITY;

    /// `new BrowserTitlebarPart(id, targetWindow, editorGroupsContainer, ...)` com `hasTitle: false`.
    pub fn new(id: String, deps: TitlebarPartDeps) -> Rc<Self> {
        let base = Disposable::new();
        let on_menubar_visibility_change: Rc<Emitter<bool>> = Emitter::new();
        let on_will_dispose: Rc<Emitter<()>> = Emitter::new();
        let on_did_change: Rc<Emitter<Option<ViewDimensions>>> = Emitter::new();
        base.register(on_menubar_visibility_change.clone());
        base.register(on_will_dispose.clone());
        base.register(on_did_change.clone());
        let custom_menubar = MutableDisposable::<dyn TitlebarMenubar>::new();
        base.register(custom_menubar.clone());
        let custom_menubar_disposables = DisposableStore::new();
        let center_adjacent_tool_bar_disposable = DisposableStore::new();
        let update_tool_bar_disposable = DisposableStore::new();
        let title_disposables = DisposableStore::new();
        for store in [&custom_menubar_disposables, &center_adjacent_tool_bar_disposable, &update_tool_bar_disposable, &title_disposables] {
            base.register(store.clone());
        }
        base.register(deps.window_title.clone());

        let is_compact_context_key = is_compact_title_bar_context().bind_to(&*deps.context_key_service);

        let this = Rc::new_cyclic(|this| Self {
            this: this.clone(),
            id,
            is_auxiliary: deps.window.id != MAIN_WINDOW,
            window: deps.window,
            theme: PartTheme::new(deps.theme_service.get_color_theme()),
            editor_groups: deps.editor_groups,
            configuration_service: deps.configuration_service,
            layout_service: deps.layout_service,
            host_service: deps.host_service,
            title_bar_host: deps.title_bar_host,
            platform: deps.platform,
            view: deps.view,
            container_styles: deps.container_styles,
            menubar_factory: deps.menubar_factory,
            toolbars: deps.toolbars,
            content_area: deps.content_area,
            main_titlebar: deps.main_titlebar,
            is_compact_context_key,
            window_title: deps.window_title,
            base,
            on_menubar_visibility_change,
            on_will_dispose,
            on_did_change,
            custom_menubar,
            custom_menubar_disposables,
            center_adjacent_tool_bar_disposable,
            update_tool_bar_disposable,
            title_disposables,
            state: RefCell::new(TitlebarState::default()),
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

    /// `new MainBrowserTitlebarPart(...)`: o `id` é `Parts.TITLEBAR_PART`.
    pub fn new_main(deps: TitlebarPartDeps) -> Rc<Self> {
        Self::new(Parts::TitlebarPart.as_str().to_owned(), deps)
    }

    /// `new AuxiliaryBrowserTitlebarPart(container, ...)`: o `id` é `workbench.parts.auxiliaryTitle.<n>`.
    pub fn new_auxiliary(deps: TitlebarPartDeps) -> Rc<Self> {
        thread_local! {
            static COUNTER: std::cell::Cell<u32> = const { std::cell::Cell::new(1) };
        }
        let counter = COUNTER.with(|counter| {
            let id = counter.get();
            counter.set(id + 1);
            id
        });
        Self::new(format!("workbench.parts.auxiliaryTitle.{counter}"), deps)
    }

    /// `Part.onThemeChange`: só depois de `create` (`Themable.onThemeChange` troca o tema e atualiza os estilos).
    fn on_theme_change(&self, theme: Rc<dyn IColorTheme>) {
        if self.state.borrow().element.is_some() {
            self.theme.set(theme);
            self.update_styles();
        }
    }

    fn register_listeners(self: &Rc<Self>) {
        let weak = Rc::downgrade(self);
        self.base.register(self.host_service.on_did_change_focus().listen(move |_| {
            if let Some(this) = weak.upgrade() {
                this.update_styles();
            }
        }));
        let weak = Rc::downgrade(self);
        self.base.register(self.host_service.on_did_change_active_window().listen(move |window_id| {
            if let Some(this) = weak.upgrade() {
                this.state.borrow_mut().active_window_id = Some(*window_id);
                this.update_styles();
            }
        }));
        let weak = Rc::downgrade(self);
        self.base.register(self.configuration_service.on_did_change_configuration().listen(move |event: &Rc<ConfigurationChangeEvent>| {
            if let Some(this) = weak.upgrade() {
                this.on_configuration_changed(event);
            }
        }));
        let weak = Rc::downgrade(self);
        self.base.register(self.editor_groups.on_did_change_editor_part_options().listen(move |event| {
            if let Some(this) = weak.upgrade() {
                this.on_editor_part_configuration_change(event);
            }
        }));
    }

    // region IView

    /// `minimumHeight`.
    pub fn minimum_height(&self) -> f64 {
        let wco_enabled = self.platform.is_web && self.title_bar_host.is_wco_enabled();
        let mut value = if self.is_command_center_visible() || wco_enabled { f64::from(DEFAULT_CUSTOM_TITLEBAR_HEIGHT) } else { 30.0 };
        if wco_enabled {
            value = value.max(self.view.wco_titlebar_area_height().unwrap_or(0.0));
        }

        value / if self.prevent_zoom() { self.view.zoom_factor() } else { 1.0 }
    }

    pub fn maximum_height(&self) -> f64 {
        self.minimum_height()
    }

    /// `AuxiliaryBrowserTitlebarPart.height`.
    pub fn height(&self) -> f64 {
        self.minimum_height()
    }

    /// `onDidChange`: dispara `None` (o `undefined` do original) quando as restrições de tamanho mudam.
    pub fn on_did_change(&self) -> Event<Option<ViewDimensions>> {
        self.on_did_change.event()
    }

    // endregion

    /// `onMenubarVisibilityChange`.
    pub fn on_menubar_visibility_change(&self) -> Event<bool> {
        self.on_menubar_visibility_change.event()
    }

    /// `onWillDispose`.
    pub fn on_will_dispose(&self) -> Event<()> {
        self.on_will_dispose.event()
    }

    /// `getContainer()`.
    pub fn get_container(&self) -> Option<Rc<ViewElement>> {
        self.state.borrow().element.clone()
    }

    fn on_editor_part_configuration_change(&self, event: &EditorPartOptionsChangeEvent) {
        if event.old_part_options.editor_actions_location != event.new_part_options.editor_actions_location || event.old_part_options.show_tabs != event.new_part_options.show_tabs {
            let action_toolbar_element = self.state.borrow().action_toolbar_element.clone();
            if let (true, Some(element)) = (self.title_bar_host.has_custom_titlebar(), action_toolbar_element) {
                self.toolbars.create_action_toolbar(&element);
                self.toolbars.create_action_toolbar_menus(ActionToolbarMenusUpdate { editor_actions: true, ..ActionToolbarMenusUpdate::default() });
                self.on_did_change.fire(None);
            }
        }
    }

    fn on_configuration_changed(&self, event: &Rc<ConfigurationChangeEvent>) {
        if event.affects_configuration(LayoutSettings::ModernUi.as_str(), None) {
            self.update_styles_if_created();
        }

        // Custom menu bar (disabled if auxiliary)
        if !self.is_auxiliary && !self.title_bar_host.has_native_menu() && (!self.platform.is_macintosh || self.platform.is_web) && event.affects_configuration(MENU_BAR_VISIBILITY_SETTING, None) {
            if self.current_menubar_visibility() == MenuBarVisibility::Compact {
                self.uninstall_menubar();
            } else {
                self.install_menubar();
            }
        }

        // Actions
        let action_toolbar_exists = self.state.borrow().action_toolbar_element.is_some();
        if self.title_bar_host.has_custom_titlebar() && action_toolbar_exists {
            let affects_layout_control = event.affects_configuration(LayoutSettings::LayoutActions.as_str(), None);
            let affects_activity_control = event.affects_configuration(LayoutSettings::ActivityBarLocation.as_str(), None);

            if affects_layout_control || affects_activity_control {
                self.toolbars.create_action_toolbar_menus(ActionToolbarMenusUpdate {
                    layout_actions: affects_layout_control,
                    activity_actions: affects_activity_control,
                    ..ActionToolbarMenusUpdate::default()
                });

                self.on_did_change.fire(None);
            }
        }

        // Command Center
        if event.affects_configuration(LayoutSettings::CommandCenter.as_str(), None) {
            self.recreate_title();
        }
    }

    fn recreate_title(&self) {
        self.create_title();

        self.on_did_change.fire(None);
    }

    /// `updateOptions({ compact })`.
    pub fn update_options(&self, compact: bool) {
        let old_is_compact = self.state.borrow().is_compact;
        self.state.borrow_mut().is_compact = compact;

        self.is_compact_context_key.set(Dynamic::Bool(compact));

        if old_is_compact != compact {
            self.recreate_title();
            self.toolbars.create_action_toolbar_menus(ActionToolbarMenusUpdate::all());
        }
    }

    /// `Reflect.set(this, 'leftContent', leftContent)` dos testes.
    pub fn set_left_content(&self, left_content: &Rc<ViewElement>) {
        self.state.borrow_mut().left_content = Some(left_content.clone());
    }

    /// `installMenubar()`.
    pub fn install_menubar(&self) {
        if self.state.borrow().menubar.is_some() {
            return; // If the menubar is already installed, skip
        }

        let custom_menubar = (self.menubar_factory)();
        self.custom_menubar.set(Some(custom_menubar.clone()));

        let menubar = ViewElement::with_classes(&["menubar"]);
        let left_content = self.state.borrow().left_content.clone().expect("a área da esquerda foi criada");
        self.view.append_child(&left_content, &menubar);
        self.view.set_attribute(&menubar, "role", "menubar");
        self.state.borrow_mut().menubar = Some(menubar.clone());

        let weak = self.this.clone();
        self.custom_menubar_disposables.add(custom_menubar.on_visibility_change().listen(move |visible| {
            if let Some(this) = weak.upgrade() {
                this.on_menubar_visibility_changed(*visible);
            }
        }));
        let weak = self.this.clone();
        self.custom_menubar_disposables.add(custom_menubar.on_focus_state_change().listen(move |focused| {
            let Some(this) = weak.upgrade() else {
                return;
            };
            let element = this.state.borrow().element.clone().expect("a parte foi criada");
            if *focused {
                element.class_list_add("menubar-focused");
            } else {
                element.class_list_remove("menubar-focused");
            }
        }));

        custom_menubar.create(&menubar);
    }

    /// `uninstallMenubar()`.
    pub fn uninstall_menubar(&self) {
        self.custom_menubar_disposables.clear();
        self.custom_menubar.clear();
        if let Some(element) = self.state.borrow().element.clone() {
            element.class_list_remove("menubar-focused");
        }

        let menubar = self.state.borrow_mut().menubar.take();
        if let Some(menubar) = menubar {
            self.view.remove(&menubar);
        }

        self.on_menubar_visibility_changed(false);
    }

    fn on_menubar_visibility_changed(&self, visible: bool) {
        if self.platform.is_web || self.platform.is_windows || self.platform.is_linux {
            let last_layout_dimensions = self.state.borrow().last_layout_dimensions;
            if let Some(dimension) = last_layout_dimensions {
                self.layout(dimension.width, dimension.height);
            }

            self.on_menubar_visibility_change.fire(visible);
        }
    }

    /// `updateProperties(properties)`.
    pub fn update_properties(&self, properties: &TitleProperties) {
        self.window_title.update_properties(properties);
    }

    /// `registerVariables(variables)`.
    pub fn register_variables(&self, variables: &[TitleVariable]) {
        self.window_title.register_variables(variables);
    }

    /// `Part.create(parent)` com o `createContentArea` escolhido, seguido de `updateStyles`.
    pub fn create(&self, parent: &Rc<ViewElement>) {
        match self.content_area {
            TitlebarContentArea::ElementOnly => self.state.borrow_mut().element = Some(parent.clone()),
            TitlebarContentArea::Full => self.create_content_area(parent),
        }

        self.update_styles();
    }

    fn create_content_area(&self, parent: &Rc<ViewElement>) {
        let root_container = ViewElement::with_classes(&["titlebar-container"]);
        self.view.append_child(parent, &root_container);
        let left_content = ViewElement::with_classes(&["titlebar-left"]);
        self.view.append_child(&root_container, &left_content);
        let center_content = ViewElement::with_classes(&["titlebar-center"]);
        self.view.append_child(&root_container, &center_content);
        let right_content = ViewElement::with_classes(&["titlebar-right"]);
        self.view.append_child(&root_container, &right_content);
        {
            let mut state = self.state.borrow_mut();
            state.element = Some(parent.clone());
            state.root_container = Some(root_container.clone());
            state.left_content = Some(left_content.clone());
            state.center_content = Some(center_content.clone());
            state.right_content = Some(right_content.clone());
        }

        let has_custom_titlebar = self.title_bar_host.has_custom_titlebar();
        let has_native_titlebar = self.title_bar_host.has_native_titlebar();

        // App Icon (Windows, Linux)
        if (self.platform.is_windows || self.platform.is_linux) && !has_native_titlebar {
            let app_icon = ViewElement::with_classes(&["window-appicon"]);
            self.view.prepend(&left_content, &app_icon);
            self.state.borrow_mut().app_icon = Some(app_icon);
        }

        // Draggable region that we can manipulate for #52522
        let drag_region = ViewElement::with_classes(&["titlebar-drag-region"]);
        self.view.prepend(&root_container, &drag_region);
        self.state.borrow_mut().drag_region = Some(drag_region);

        // Menubar: install a custom menu bar depending on configuration
        if !self.is_auxiliary && !self.title_bar_host.has_native_menu() && (!self.platform.is_macintosh || self.platform.is_web) && self.current_menubar_visibility() != MenuBarVisibility::Compact {
            self.install_menubar();
        }

        // Title
        let title = ViewElement::with_classes(&["window-title"]);
        self.view.append_child(&center_content, &title);
        self.state.borrow_mut().title = Some(title.clone());
        self.create_title();

        // Center-Adjacent Toolbar
        if has_custom_titlebar {
            let element = ViewElement::with_classes(&["center-adjacent-toolbar-container"]);
            self.view.append_child(&right_content, &element);
            self.state.borrow_mut().center_adjacent_toolbar_element = Some(element.clone());
            // Re-evaluate fit when items change, see #303222.
            let toolbar = self.toolbars.create_center_adjacent_toolbar(&element, self.overflow_rescheduler());
            self.center_adjacent_tool_bar_disposable.add(toolbar);
        }

        // Update Toolbar (before the right-aligned toolbar actions)
        if has_custom_titlebar {
            let element = ViewElement::with_classes(&["update-toolbar-container"]);
            self.view.append_child(&right_content, &element);
            self.state.borrow_mut().update_toolbar_element = Some(element.clone());
            let toolbar = self.toolbars.create_update_toolbar(&element, self.overflow_rescheduler());
            self.update_tool_bar_disposable.add(toolbar);
        }

        // Create Toolbar Actions
        if has_custom_titlebar {
            let element = ViewElement::with_classes(&["action-toolbar-container"]);
            self.view.append_child(&right_content, &element);
            self.state.borrow_mut().action_toolbar_element = Some(element.clone());
            self.toolbars.create_action_toolbar(&element);
            self.toolbars.create_action_toolbar_menus(ActionToolbarMenusUpdate::all());
        }

        // Window Controls Container
        if !has_native_titlebar {
            let mut primary_window_controls_on_left = self.platform.is_macintosh;
            if self.platform.is_macintosh && self.platform.is_native {
                // Check if the locale is RTL, macOS will move traffic lights in RTL locales
                if self.view.is_rtl_locale() {
                    primary_window_controls_on_left = false;
                }
            }

            let (primary, secondary) = if primary_window_controls_on_left { (&left_content, &right_content) } else { (&right_content, &left_content) };
            if self.platform.is_macintosh && self.platform.is_native && primary_window_controls_on_left {
                // macOS native: controls are on the left and the container is not needed to make room for something, except for web
                // where a custom menu being supported). not putting the container helps with allowing to move the window when
                // clicking very close to the window control buttons.
            } else if self.window_controls_hidden() {
                // Linux/Windows: controls are explicitly disabled
            } else {
                let container = ViewElement::with_classes(&["window-controls-container"]);
                self.view.append_child(primary, &container);
                self.state.borrow_mut().window_controls_container = Some(container.clone());
                if self.title_bar_host.is_wco_enabled() {
                    container.class_list_add("wco-enabled");
                    if self.platform.is_web || self.platform.is_linux {
                        let other = ViewElement::with_classes(&["window-controls-container", "wco-enabled"]);
                        self.view.append_child(secondary, &other);
                    }
                } else if self.platform.is_web {
                    // Web: its possible to have control overlays on both sides, for example on macOS with window controls on the left
                    // and PWA controls on the right.
                    let other = ViewElement::with_classes(&["window-controls-container"]);
                    self.view.append_child(secondary, &other);
                }
            }
        }

        // Context menu over title bar: depending on the OS and the location of the click this will either be the overall context menu
        // for the entire title bar or a specific title context menu. Windows / Linux: we only support the overall context menu on the
        // title bar. macOS: we support both the overall context menu and the title context menu. in addition, we allow Cmd+click to
        // bring up the title context menu.
        let weak = self.this.clone();
        let registration = self.view.add_context_menu_listener(
            &root_container,
            &title,
            Rc::new(move |menu, event| {
                if let Some(this) = weak.upgrade() {
                    this.on_context_menu(event, menu);
                }
            }),
        );
        self.base.register(registration);
    }

    /// `getWindowControlsStyle(configurationService) === WindowControlsStyle.HIDDEN`.
    fn window_controls_hidden(&self) -> bool {
        matches!(self.configuration_service.get_value(Some("window.controlsStyle"), None), Dynamic::String(style) if style == "hidden")
    }

    /// O `() => this.titleBarToolBarOverflowScheduler.schedule()` dos `onDidChangeMenuItems`.
    fn overflow_rescheduler(&self) -> Rc<dyn Fn()> {
        let weak = self.this.clone();
        Rc::new(move || {
            if let Some(this) = weak.upgrade() {
                this.schedule_tool_bar_overflow_update();
            }
        })
    }

    fn schedule_tool_bar_overflow_update(&self) {
        let root_container = self.state.borrow().root_container.clone();
        let Some(root_container) = root_container else {
            return;
        };
        let weak = self.this.clone();
        self.view.schedule_tool_bar_overflow_update(
            &root_container,
            Rc::new(move || {
                if let Some(this) = weak.upgrade() {
                    this.update_title_bar_tool_bar_overflow();
                }
            }),
        );
    }

    /// `updateTitleBarToolBarOverflow()`.
    fn update_title_bar_tool_bar_overflow(&self) {
        let (root_container, center_adjacent, update) = {
            let state = self.state.borrow();
            (state.root_container.clone(), state.center_adjacent_toolbar_element.clone(), state.update_toolbar_element.clone())
        };
        if let Some(root_container) = root_container {
            update_title_bar_tool_bar_overflow(&root_container, center_adjacent.as_ref(), update.as_ref());
        }
    }

    fn create_title(&self) {
        self.title_disposables.clear();

        let is_showing_title_in_native_titlebar = self.title_bar_host.has_native_titlebar();
        let title = self.state.borrow().title.clone();
        let Some(title) = title else {
            return;
        };

        // Text Title
        if !self.is_command_center_visible() {
            if !is_showing_title_in_native_titlebar {
                self.set_title_text(&title, self.window_title.value());
                let weak = self.this.clone();
                self.title_disposables.add(self.window_title.on_did_change().listen(move |_| {
                    let Some(this) = weak.upgrade() else {
                        return;
                    };
                    let title = this.state.borrow().title.clone();
                    if let Some(title) = title {
                        this.set_title_text(&title, this.window_title.value());
                    }
                    let last_layout_dimensions = this.state.borrow().last_layout_dimensions;
                    if let Some(dimension) = last_layout_dimensions {
                        this.update_layout(dimension); // layout menubar and other renderings in the titlebar
                    }
                }));
            } else {
                self.view.reset(&title);
                self.state.borrow_mut().title_text.clear();
            }
        }
        // Menu Title
        else {
            let command_center = self.toolbars.create_command_center(&title);
            self.title_disposables.add(command_center);
        }
    }

    fn set_title_text(&self, title: &Rc<ViewElement>, text: String) {
        self.view.set_text_content(title, &text);
        self.state.borrow_mut().title_text = text;
    }

    fn on_context_menu(&self, event: &TitlebarContextMenuEvent, menu: TitlebarContextMenu) {
        // Show it: o menu (`MenuId`), o `getAnchor` e o `contextKeyService` são de quem desenha a barra.
        self.view.show_context_menu(menu, event);
    }

    /// `getColor(id, modify)` do `Themable`.
    fn get_color(&self, id: &str, modify: bool) -> Option<ColorValue> {
        let modifier: Option<&dyn Fn(Color, &dyn IColorTheme) -> Color> = if modify { Some(&make_opaque) } else { None };
        self.theme.get_color(id, modifier).map(ColorValue::new)
    }

    fn update_styles_if_created(&self) {
        if self.state.borrow().element.is_some() {
            self.update_styles();
        }
    }

    /// `updateStyles()`: o fundo, o fundo do shell, a classe `inactive`, a classe `light`, o primeiro plano e a borda.
    pub fn update_styles(&self) {
        // Part container
        let Some(element) = self.state.borrow().element.clone() else {
            return;
        };

        let active_window_id = self.state.borrow().active_window_id;
        let is_inactive = is_part_inactive(active_window_id, &self.window, self.host_service.as_ref());
        if is_inactive {
            element.class_list_add("inactive");
        } else {
            element.class_list_remove("inactive");
        }

        let title_background = self
            .get_color(if is_inactive { TITLE_BAR_INACTIVE_BACKGROUND } else { TITLE_BAR_ACTIVE_BACKGROUND }, true)
            .or_else(|| self.get_color(TITLE_BAR_ACTIVE_BACKGROUND, true));
        let shell_background = self
            .get_color(if is_inactive { MODERN_UI_INACTIVE_SHELL_BACKGROUND } else { MODERN_UI_SHELL_BACKGROUND }, true)
            .or_else(|| self.get_color(MODERN_UI_SHELL_BACKGROUND, true))
            .or_else(|| title_background.clone());
        set_style_or_remove(&element, "background-color", title_background.as_ref().map(ColorValue::css).as_deref());
        let container = self.layout_service.get_container(self.window.id);
        self.container_styles.set_property(container, MODERN_UI_SHELL_BACKGROUND_PROPERTY, &shell_background.as_ref().map(ColorValue::raw).unwrap_or_default());

        if title_background.as_ref().is_some_and(|background| Color::from_hex(&background.raw()).is_lighter()) {
            element.class_list_add("light");
        } else {
            element.class_list_remove("light");
        }

        let title_foreground = self.get_color(if is_inactive { TITLE_BAR_INACTIVE_FOREGROUND } else { TITLE_BAR_ACTIVE_FOREGROUND }, false);
        set_style_or_remove(&element, "color", title_foreground.as_ref().map(ColorValue::css).as_deref());

        let modern_ui = matches!(self.configuration_service.get_value(Some(LayoutSettings::ModernUi.as_str()), None), Dynamic::Bool(true));
        let title_border = if !self.is_auxiliary && modern_ui { None } else { self.get_color(TITLE_BAR_BORDER, false) };
        let border_bottom = title_border.as_ref().map(|border| format!("1px solid {}", border.css()));
        set_style_or_remove(&element, "border-bottom", border_bottom.as_deref());
    }

    /// `currentMenubarVisibility`.
    fn current_menubar_visibility(&self) -> MenuBarVisibility {
        if self.is_auxiliary {
            return MenuBarVisibility::Hidden;
        }

        self.title_bar_host.get_menu_bar_visibility().and_then(|visibility| MenuBarVisibility::parse(&visibility)).unwrap_or(MenuBarVisibility::Classic)
    }

    /// `layoutControlEnabled`.
    fn layout_control_enabled(&self) -> bool {
        !matches!(self.configuration_service.get_value(Some(LayoutSettings::LayoutActions.as_str()), None), Dynamic::Bool(false))
    }

    /// `isCommandCenterVisible`.
    pub fn is_command_center_visible(&self) -> bool {
        !self.state.borrow().is_compact && !matches!(self.configuration_service.get_value(Some(LayoutSettings::CommandCenter.as_str()), None), Dynamic::Bool(false))
    }

    /// `editorActionsEnabled`.
    fn editor_actions_enabled(&self) -> bool {
        let options = self.editor_groups.part_options();
        options.editor_actions_location == EditorActionsLocation::TitleBar || (options.editor_actions_location == EditorActionsLocation::Default && options.show_tabs == ShowTabs::None)
    }

    /// `activityActionsEnabled`.
    fn activity_actions_enabled(&self) -> bool {
        let activity_bar_position = self.configuration_service.get_value(Some(LayoutSettings::ActivityBarLocation.as_str()), None);
        let at_top_or_bottom =
            matches!(&activity_bar_position, Dynamic::String(position) if position == ActivityBarPosition::Top.as_str() || position == ActivityBarPosition::Bottom.as_str());
        !self.state.borrow().is_compact && !self.is_auxiliary && at_top_or_bottom
    }

    /// `globalActionsEnabled`.
    fn global_actions_enabled(&self) -> bool {
        !self.state.borrow().is_compact
    }

    /// `hasZoomableElements`.
    pub fn has_zoomable_elements(&self) -> bool {
        let visibility = self.current_menubar_visibility();
        let has_menubar = !(matches!(visibility, MenuBarVisibility::Hidden | MenuBarVisibility::Compact) || (!self.platform.is_web && self.platform.is_macintosh));
        let has_command_center = self.is_command_center_visible();
        let has_tool_bar_actions = self.global_actions_enabled() || self.layout_control_enabled() || self.editor_actions_enabled() || self.activity_actions_enabled();
        has_menubar || has_command_center || has_tool_bar_actions
    }

    /// `preventZoom`: não aplica o zoom quando encolher abaixo do tamanho dos controles da janela (zoom menor que 1) ou quando a
    /// barra não tem itens customizados. A auxiliar consulta os da barra principal.
    pub fn prevent_zoom(&self) -> bool {
        let has_zoomable_elements = match self.main_titlebar.as_ref().filter(|_| self.is_auxiliary).and_then(Weak::upgrade) {
            Some(main) => main.has_zoomable_elements(),
            None => self.has_zoomable_elements(),
        };
        self.view.zoom_factor() < 1.0 || !has_zoomable_elements
    }

    /// `layout(width, height)`.
    pub fn layout(&self, width: f64, height: f64) {
        self.update_layout(Dimension { width, height });

        // `layoutContents`: sem título, a área de conteúdo é o próprio elemento.
        let element = {
            let mut state = self.state.borrow_mut();
            state.dimension = Some((width, height));
            state.element.clone()
        };
        if let Some(element) = element {
            element.set_size(width, height);
        }

        self.schedule_tool_bar_overflow_update();
    }

    fn update_layout(&self, dimension: Dimension) {
        self.state.borrow_mut().last_layout_dimensions = Some(dimension);

        if !self.title_bar_host.has_custom_titlebar() {
            return;
        }

        let zoom_factor = self.view.zoom_factor();
        let (element, root_container, title_text) = {
            let state = self.state.borrow();
            (state.element.clone(), state.root_container.clone(), state.title_text.clone())
        };

        if let Some(element) = &element {
            element.style_set_property("--zoom-factor", &zoom_factor.to_string());
        }
        if let Some(root_container) = &root_container {
            if self.prevent_zoom() {
                root_container.class_list_add("counter-zoom");
            } else {
                root_container.class_list_remove("counter-zoom");
            }
        }

        if let Some(menubar) = self.custom_menubar.value() {
            menubar.layout(Dimension { width: 0.0, height: dimension.height });
        }

        let has_center = self.is_command_center_visible() || !title_text.is_empty();
        if let Some(root_container) = &root_container {
            if has_center {
                root_container.class_list_add("has-center");
            } else {
                root_container.class_list_remove("has-center");
            }
        }
    }

    /// `focus()`.
    pub fn focus(&self) {
        if let Some(menubar) = self.custom_menubar.value() {
            menubar.toggle_focus();
        } else if let Some(element) = self.get_container() {
            self.view.focus_first_tabbable(&element);
        }
    }

    /// `toJSON()`.
    pub fn to_json(&self) -> Parts {
        Parts::TitlebarPart
    }
}

/// `contextMenuService.showContextMenu({ getAnchor: () => event, menuId, contextKeyService, ... })` fica com quem desenha a barra.
impl LayoutPart for TitlebarPart {
    fn get_id(&self) -> String {
        self.id.clone()
    }
}

impl IDisposable for TitlebarPart {
    fn dispose(&self) {
        self.on_will_dispose.fire(());

        self.base.dispose();
    }
}
