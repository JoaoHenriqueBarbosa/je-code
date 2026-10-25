//! Porte de `src/vs/platform/windows/electron-main/windowImpl.ts`, a classe base `BaseWindow` e o `DockBadgeManager`:
//! os eventos de janela, o estado, o foco, o selo do aplicativo, os controles de janela (Window Controls Overlay) e a
//! tela cheia. O `CodeWindow` (carrega o workbench, o ciclo de vida do renderer e a configuração) fica fora: depende do
//! protocolo, do tema e do `WebContents`, que o processo único não tem.
//!
//! Decisões de tradução:
//! - **`electron.BrowserWindow`** é o contrato [`NativeBrowserWindow`], com os membros que a `BaseWindow` chama. Os
//!   eventos `win.on(...)` (`Event.fromNodeEventEmitter`) são um fluxo só, [`NativeBrowserWindow::events`], e a
//!   `BaseWindow` despacha cada [`NativeWindowEvent`] ao que o original faz no ouvinte do evento de mesmo nome.
//! - **`electron.app`, `electron.screen` e `electron.nativeImage`** são o [`BaseWindowHost`] (que estende o
//!   [`ElectronHost`] já portado). O `nativeImage.createFromDataURL` não existe: o host recebe o `data:` URL do ícone.
//! - **`DockBadgeManager.INSTANCE`** (um singleton do módulo) é o [`DockBadgeManager`] que o processo cria uma vez e
//!   entrega a cada janela, porque o selo único do aplicativo precisa do host.
//! - **`abstract id` e `abstract matches(webContents)`**: o `id` é um campo; `matches` pede o `WebContents`, que o processo
//!   único não tem, e fica com a janela concreta.
//! - **`IEnvironmentMainService`** é o [`EnvironmentMainService`], do qual a janela lê `args` e `enableRDPDisplayTracking`.
//! - **`isWindows`, `isLinux`, `isMacintosh`** vêm de [`Platform::detect`], guardado na construção.
//! - **`Color.Format.CSS.parse` lança** para `rgba(` e `rgb(` malformados; aqui a cor malformada fica como veio (o
//!   `dimColor` já devolve a cor original quando o parse dá `null`).
//! - **`WindowStateValidator.validateWindowStateOnDisplay`** (de `windows.ts`) é [`validate_window_state_on_display`],
//!   que lê a área útil do monitor pelo host ([`BaseWindowHost::work_area`]).
//! - **`getAllWindowsExcludingOffscreen().length`** é [`BaseWindowHost::windows_count_excluding_offscreen`].

use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::rc::{Rc, Weak};

use indexmap::IndexMap;
use je_base::async_::{async_fn, timeout};
use je_base::dynamic::Dynamic;
use je_base::event::{Emitter, Event};
use je_base::lifecycle::{Disposable, DisposableRef, DisposableStore, IDisposable, MutableDisposable, to_disposable};
use je_base::node::windows_version::os_release;
use je_base::platform::{Platform, is_tahoe_or_newer};
use je_base::promise::{DeferredPromise, Promise};
use je_base::runtime;
use je_theme::color::format::css;

use crate::configuration::service::IConfigurationService;
use crate::environment::environment_main_service::EnvironmentMainService;
use crate::environment::environment::NativeEnvironmentServiceApi;
use crate::log::{LogArg, LogServiceRef, Logger};
use crate::state::{StateReadService, StateService};
use crate::window::electron_main::{Display, ElectronHost, Point, Rectangle, WindowMode, WindowUiState};
use crate::window::window::{DEFAULT_CUSTOM_TITLEBAR_HEIGHT, IPoint, TitlebarStyle, get_macos_window_controls_position, has_native_titlebar, use_native_full_screen};

/// A chave do estado com a altura dos controles de janela.
const WINDOW_CONTROL_HEIGHT_STATE_STORAGE_KEY: &str = "windowControlHeight";

/// `FocusMode` (de `native.ts`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FocusMode {
    /// Transfer the focus to the window.
    Transfer,
    /// Only notify the user (badge and flash/bounce) without taking the focus.
    Notify,
    /// Force the focus even if another application is active.
    Force,
}

/// `IApplicationBadge` (de `native.ts`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ApplicationBadge {
    pub count: u32,
    /// O ícone desenhado pelo renderer (`data:` URL), usado no Windows.
    pub icon_data_url: Option<String>,
    pub description: String,
}

/// O argumento de `electron.app.setBadgeCount`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DockBadge {
    /// `setBadgeCount(0)`: sem selo.
    Cleared,
    /// `setBadgeCount(n)`.
    Count(u32),
    /// `setBadgeCount(undefined)`: o ponto genérico.
    Dot,
}

/// `BrowserWindowConstructorOptions['titleBarOverlay']`: `boolean | { color?, symbolColor?, height? }`.
#[derive(Clone, Debug, PartialEq)]
pub enum TitleBarOverlayOption {
    Flag(bool),
    Config { height: Option<f64> },
}

impl TitleBarOverlayOption {
    /// `!!titleBarOverlay`.
    pub fn is_enabled(&self) -> bool {
        match self {
            Self::Flag(enabled) => *enabled,
            Self::Config { .. } => true,
        }
    }
}

/// `BrowserWindowConstructorOptions`, só com o que a `BaseWindow` lê.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct BrowserWindowOptions {
    pub frame: Option<bool>,
    /// `titleBarStyle` (`'hidden'`, `'default'`...).
    pub title_bar_style: Option<String>,
    pub title_bar_overlay: Option<TitleBarOverlayOption>,
}

/// O argumento de `win.setTitleBarOverlay`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TitleBarOverlay {
    pub color: Option<String>,
    pub symbol_color: Option<String>,
    pub height: Option<f64>,
}

/// Os eventos da janela nativa que a `BaseWindow` ouve.
#[derive(Clone, Debug, PartialEq)]
pub enum NativeWindowEvent {
    Maximize,
    Unmaximize,
    Closed,
    Focus,
    EnterFullScreen,
    LeaveFullScreen,
    AlwaysOnTopChanged(bool),
    /// `system-context-menu`, com o ponto em coordenadas de tela.
    SystemContextMenu(Point),
}

/// `electron.BrowserWindow`, o que a `BaseWindow` chama.
pub trait NativeBrowserWindow {
    /// Os eventos `win.on(...)`.
    fn events(&self) -> Event<NativeWindowEvent>;

    fn get_position(&self) -> (f64, f64);
    fn get_size(&self) -> (f64, f64);
    fn set_bounds(&self, bounds: Rectangle);
    fn maximize(&self);
    fn show(&self);
    fn restore(&self);
    fn focus(&self);
    fn is_minimized(&self) -> bool;
    fn is_destroyed(&self) -> bool;
    fn is_full_screen(&self) -> bool;
    fn is_simple_full_screen(&self) -> bool;
    fn set_full_screen(&self, full_screen: bool);
    fn set_simple_full_screen(&self, full_screen: bool);
    fn set_sheet_offset(&self, offset: f64);
    fn set_title_bar_overlay(&self, overlay: TitleBarOverlay);
    /// `setWindowButtonPosition(position | null)`.
    fn set_window_button_position(&self, position: Option<IPoint>);
    /// `setOverlayIcon(image | null, description)`: o ícone é o `data:` URL da imagem.
    fn set_overlay_icon(&self, icon_data_url: Option<&str>, description: &str);
    fn flash_frame(&self, flash: bool);
    fn open_dev_tools(&self);
    /// `win.webContents.focus()`.
    fn focus_web_contents(&self);
    fn set_represented_filename(&self, filename: &str);
    fn get_represented_filename(&self) -> String;
    fn set_document_edited(&self, edited: bool);
    fn is_document_edited(&self) -> bool;
}

/// `electron.app`, `electron.screen` e o que `windows.ts` pergunta ao conjunto das janelas.
pub trait BaseWindowHost: ElectronHost {
    /// `electron.app.setBadgeCount(...)`.
    fn set_badge_count(&self, badge: DockBadge);
    /// `electron.app.isHidden()`.
    fn is_app_hidden(&self) -> bool;
    /// `electron.app.focus({ steal: true })`.
    fn focus_app_stealing(&self);
    /// `electron.app.dock?.bounce('informational')`.
    fn bounce_dock_informational(&self);
    /// `electron.screen.screenToDipPoint(point)`.
    fn screen_to_dip_point(&self, point: Point) -> Point;
    /// `electron.screen.on('display-added')`.
    fn on_display_added(&self) -> Event<Display>;
    /// `display.workArea`.
    fn work_area(&self, display: &Display) -> Rectangle;
    /// `getAllWindowsExcludingOffscreen().length`.
    fn windows_count_excluding_offscreen(&self) -> usize;
}

/// `WindowStateValidator.validateWindowStateOnDisplay`.
pub fn validate_window_state_on_display(host: &dyn BaseWindowHost, state: &WindowUiState, display: &Display) -> bool {
    let (Some(x), Some(y), Some(width), Some(height)) = (state.x, state.y, state.width, state.height) else {
        return false;
    };
    if width <= 0.0 || height <= 0.0 {
        return false;
    }

    // Prefer the working area of the display to account for taskbars on the desktop being positioned somewhere
    // (https://github.com/microsoft/vscode/issues/50830). Linux X11 sessions sometimes report wrong display bounds, so
    // we validate the reported sizes are positive.
    let work_area = host.work_area(display);
    let area = if work_area.width > 0.0 && work_area.height > 0.0 {
        work_area
    } else if display.bounds.width > 0.0 && display.bounds.height > 0.0 {
        display.bounds
    } else {
        return false;
    };

    x + width > area.x && // prevent window from falling out of the screen to the left
        y + height > area.y && // prevent window from falling out of the screen to the top
        x < area.x + area.width && // prevent window from falling out of the screen to the right
        y < area.y + area.height // prevent window from falling out of the screen to the bottom
}

/// `DockBadgeManager`: o dono do selo único do aplicativo (`app.setBadgeCount`), para o ponto de atenção do
/// [`FocusMode::Notify`] e as contagens que cada janela empurra por [`BaseWindow::set_application_badge`] não se
/// sobrescreverem. Uma contagem sempre vence o ponto, porque carrega mais informação, e as contagens de várias janelas se
/// somam.
pub struct DockBadgeManager {
    host: Rc<dyn BaseWindowHost>,
    is_linux: bool,
    attention: RefCell<HashSet<u32>>,
    counts: RefCell<IndexMap<u32, u32>>,
}

impl DockBadgeManager {
    pub fn new(host: Rc<dyn BaseWindowHost>) -> Rc<Self> {
        Rc::new(Self { host, is_linux: Platform::detect().is_linux, attention: RefCell::default(), counts: RefCell::default() })
    }

    /// `acquireBadge(window)`: o ponto de atenção da janela, até o `dispose` devolvido.
    pub fn acquire_badge(self: &Rc<Self>, window_id: u32) -> DisposableRef {
        self.attention.borrow_mut().insert(window_id);

        self.update();

        let this = self.clone();
        to_disposable(move || {
            this.attention.borrow_mut().remove(&window_id);

            this.update();
        })
    }

    pub fn set_count(&self, window_id: u32, count: u32) {
        if count > 0 {
            self.counts.borrow_mut().insert(window_id, count);
        } else if self.counts.borrow_mut().shift_remove(&window_id).is_none() {
            return; // window had no count to begin with
        }

        self.update();
    }

    fn update(&self) {
        let total: u32 = self.counts.borrow().values().sum();

        if total > 0 {
            self.host.set_badge_count(DockBadge::Count(total));
        } else if !self.attention.borrow().is_empty() {
            self.host.set_badge_count(if self.is_linux { DockBadge::Count(1) /* only numbers supported */ } else { DockBadge::Dot /* generic dot */ });
        } else {
            self.host.set_badge_count(DockBadge::Cleared);
        }
    }
}

/// Os argumentos de `updateWindowControls`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct WindowControlsUpdate {
    pub height: Option<f64>,
    pub horizontal_inset: Option<f64>,
    pub background_color: Option<String>,
    pub foreground_color: Option<String>,
    pub dimmed: Option<bool>,
}

/// `{ backgroundColor?, foregroundColor? }`.
#[derive(Clone, Debug, Default)]
struct WindowControlColors {
    background_color: Option<String>,
    foreground_color: Option<String>,
}

/// `Math.round`: arredonda o meio para cima.
fn js_round(value: f64) -> f64 {
    (value + 0.5).floor()
}

/// `dimColor`: mistura a cor CSS com preto a 50% de opacidade, como o painel de fundo `rgba(0, 0, 0, 0.5)` dos modais.
fn dim_color(color: &str) -> String {
    let Some(parsed) = css::parse(color).ok().flatten() else {
        return color.to_owned();
    };

    let dim_factor = 0.5; // 1 - 0.5 opacity of black overlay
    let rgba = parsed.rgba();
    let r = js_round(f64::from(rgba.r) * dim_factor);
    let g = js_round(f64::from(rgba.g) * dim_factor);
    let b = js_round(f64::from(rgba.b) * dim_factor);

    format!("rgb({r}, {g}, {b})")
}

/// `color?.trim() === '' ? undefined : color`.
fn blank_to_none(color: Option<String>) -> Option<String> {
    color.filter(|color| !color.trim().is_empty())
}

/// `BaseWindow`.
pub struct BaseWindow {
    me: Weak<Self>,
    base: Rc<Disposable>,
    platform: Platform,

    /// `abstract readonly id`.
    pub id: u32,

    application_badge_window_id: Cell<Option<u32>>,

    on_did_close: Rc<Emitter<()>>,
    on_did_maximize: Rc<Emitter<()>>,
    on_did_unmaximize: Rc<Emitter<()>>,
    on_did_trigger_system_context_menu: Rc<Emitter<(f64, f64)>>,
    on_did_enter_full_screen: Rc<Emitter<()>>,
    on_did_leave_full_screen: Rc<Emitter<()>>,
    on_did_change_always_on_top: Rc<Emitter<bool>>,

    last_focus_time: Cell<f64>,
    maximized_window_state: RefCell<Option<WindowUiState>>,
    has_window_control_overlay: Cell<bool>,
    win: RefCell<Option<Rc<dyn NativeBrowserWindow>>>,

    represented_filename: RefCell<Option<String>>,
    document_edited: Cell<Option<bool>>,
    notify_focus_disposable: Rc<MutableDisposable<DisposableStore>>,

    window_controls_dimmed: Cell<bool>,
    last_window_control_colors: RefCell<Option<WindowControlColors>>,

    transient_is_native_full_screen: Cell<Option<bool>>,
    join_native_full_screen_transition: RefCell<Option<Rc<DeferredPromise<bool>>>>,

    configuration_service: Rc<dyn IConfigurationService>,
    state_service: Rc<dyn StateService>,
    environment_main_service: Rc<EnvironmentMainService>,
    log_service: LogServiceRef,
    host: Rc<dyn BaseWindowHost>,
    dock_badge_manager: Rc<DockBadgeManager>,
}

impl BaseWindow {
    pub fn new(
        id: u32,
        configuration_service: Rc<dyn IConfigurationService>,
        state_service: Rc<dyn StateService>,
        environment_main_service: Rc<EnvironmentMainService>,
        log_service: LogServiceRef,
        host: Rc<dyn BaseWindowHost>,
        dock_badge_manager: Rc<DockBadgeManager>,
    ) -> Rc<Self> {
        let base = Disposable::new();
        let notify_focus_disposable = MutableDisposable::new();
        base.register(notify_focus_disposable.clone());

        let on_did_close = base_emitter(&base);
        let on_did_maximize = base_emitter(&base);
        let on_did_unmaximize = base_emitter(&base);
        let on_did_trigger_system_context_menu = base_emitter(&base);
        let on_did_enter_full_screen = base_emitter(&base);
        let on_did_leave_full_screen = base_emitter(&base);
        let on_did_change_always_on_top = base_emitter(&base);

        let this = Rc::new_cyclic(|me| Self {
            me: me.clone(),
            base,
            platform: Platform::detect(),
            id,
            application_badge_window_id: Cell::new(None),
            on_did_close,
            on_did_maximize,
            on_did_unmaximize,
            on_did_trigger_system_context_menu,
            on_did_enter_full_screen,
            on_did_leave_full_screen,
            on_did_change_always_on_top,
            last_focus_time: Cell::new(runtime::date_now() as f64), // window is shown on creation so take current time
            maximized_window_state: RefCell::new(None),
            has_window_control_overlay: Cell::new(false),
            win: RefCell::new(None),
            represented_filename: RefCell::new(None),
            document_edited: Cell::new(None),
            notify_focus_disposable,
            window_controls_dimmed: Cell::new(false),
            last_window_control_colors: RefCell::new(None),
            transient_is_native_full_screen: Cell::new(None),
            join_native_full_screen_transition: RefCell::new(None),
            configuration_service,
            state_service,
            environment_main_service,
            log_service,
            host,
            dock_badge_manager,
        });

        // Release this window's share of the application wide badge, so that a closed or crashed window cannot leave a
        // phantom count behind.
        let weak = Rc::downgrade(&this);
        this.base.register(to_disposable(move || {
            if let Some(this) = weak.upgrade() {
                if let Some(window_id) = this.application_badge_window_id.get() {
                    this.dock_badge_manager.set_count(window_id, 0);
                }
            }
        }));

        this
    }

    // region: eventos

    pub fn on_did_close(&self) -> Event<()> {
        self.on_did_close.event()
    }

    pub fn on_did_maximize(&self) -> Event<()> {
        self.on_did_maximize.event()
    }

    pub fn on_did_unmaximize(&self) -> Event<()> {
        self.on_did_unmaximize.event()
    }

    /// `onDidTriggerSystemContextMenu`: `{ x, y }`.
    pub fn on_did_trigger_system_context_menu(&self) -> Event<(f64, f64)> {
        self.on_did_trigger_system_context_menu.event()
    }

    pub fn on_did_enter_full_screen(&self) -> Event<()> {
        self.on_did_enter_full_screen.event()
    }

    pub fn on_did_leave_full_screen(&self) -> Event<()> {
        self.on_did_leave_full_screen.event()
    }

    pub fn on_did_change_always_on_top(&self) -> Event<bool> {
        self.on_did_change_always_on_top.event()
    }

    // endregion

    /// `get lastFocusTime()`.
    pub fn last_focus_time(&self) -> f64 {
        self.last_focus_time.get()
    }

    /// `get win()`.
    pub fn win(&self) -> Option<Rc<dyn NativeBrowserWindow>> {
        self.win.borrow().clone()
    }

    /// Um ouvinte que só roda enquanto a janela vive, sem mantê-la viva.
    fn while_alive<T: 'static>(&self, handler: impl Fn(&Self, &T) + 'static) -> impl Fn(&T) + 'static {
        let me = self.me.clone();
        move |event: &T| {
            if let Some(this) = me.upgrade() {
                handler(this.as_ref(), event);
            }
        }
    }

    /// `setWin(win, options?)`.
    pub fn set_win(&self, win: Rc<dyn NativeBrowserWindow>, options: Option<&BrowserWindowOptions>) {
        *self.win.borrow_mut() = Some(win.clone());
        // Electron requires the overlay to have been enabled when the window was created.
        self.has_window_control_overlay.set(options.and_then(|options| options.title_bar_overlay.as_ref()).is_some_and(TitleBarOverlayOption::is_enabled));

        // Sheet Offsets
        let creation_style = options.and_then(|options| options.title_bar_style.as_deref()).filter(|style| *style == "hidden").map(|_| TitlebarStyle::Custom);
        let use_custom_title_style = !has_native_titlebar(self.configuration_service.as_ref(), creation_style /* None: unknown */);
        let is_windows_or_linux = self.platform.is_windows || self.platform.is_linux;

        // Window Events
        let handle_system_context_menu = is_windows_or_linux && use_custom_title_style;
        self.base.register(win.events().listen(self.while_alive(move |this, event: &NativeWindowEvent| this.on_native_event(event, handle_system_context_menu))));

        if self.platform.is_macintosh && use_custom_title_style {
            win.set_sheet_offset(if is_tahoe_or_newer(&os_release()) { 32.0 } else { 28.0 }); // offset dialogs by the height of the custom title bar if we have any
        }

        // Update the window controls immediately based on cached or default values
        if self.has_window_control_overlay.get() {
            match self.state_service.get_item(WINDOW_CONTROL_HEIGHT_STATE_STORAGE_KEY, Dynamic::Undefined) {
                Dynamic::Number(cached) if cached != 0.0 && !cached.is_nan() => self.update_window_controls(&WindowControlsUpdate { height: Some(cached), ..Default::default() }),
                _ => self.update_window_controls(&WindowControlsUpdate { height: Some(f64::from(DEFAULT_CUSTOM_TITLEBAR_HEIGHT)), ..Default::default() }),
            }
        }

        // Open devtools if instructed from command line args
        if matches!(self.environment_main_service.args().get("open-devtools"), Dynamic::Bool(true)) {
            win.open_dev_tools();
        }

        // macOS: Window Fullscreen Transitions
        if self.platform.is_macintosh {
            self.base.register(self.on_did_enter_full_screen().listen(self.while_alive(|this, _: &()| this.complete_native_full_screen_transition())));
            self.base.register(self.on_did_leave_full_screen().listen(self.while_alive(|this, _: &()| this.complete_native_full_screen_transition())));
        }

        if self.platform.is_windows && self.environment_main_service.enable_rdp_display_tracking() {
            // Handles the display-added event on Windows RDP multi-monitor scenarios.
            // This helps restore maximized windows to their correct monitor after RDP reconnection.
            // Refs https://github.com/electron/electron/issues/47016
            self.base.register(self.host.on_display_added().listen(self.while_alive(|this, display: &Display| this.on_display_added(display))));
        }
    }

    fn complete_native_full_screen_transition(&self) {
        let transition = self.join_native_full_screen_transition.borrow().clone();
        if let Some(transition) = transition {
            transition.complete(true);
        }
    }

    /// O que o original faz no ouvinte de cada evento da janela nativa.
    fn on_native_event(&self, event: &NativeWindowEvent, handle_system_context_menu: bool) {
        match event {
            NativeWindowEvent::Maximize => {
                let win = self.win();
                if let Some(win) = win.filter(|_| self.platform.is_windows && self.environment_main_service.enable_rdp_display_tracking()) {
                    let (x, y) = win.get_position();
                    let (width, height) = win.get_size();

                    let state = WindowUiState { mode: Some(WindowMode::Maximized), width: Some(width), height: Some(height), x: Some(x), y: Some(y), ..Default::default() };
                    self.log_service.debug_args(&format!("Saved maximized window {} display state:", self.id), &[LogArg::from(state.to_dynamic())]);
                    *self.maximized_window_state.borrow_mut() = Some(state);
                }

                self.on_did_maximize.fire(());
            }
            NativeWindowEvent::Unmaximize => {
                if self.platform.is_windows && self.environment_main_service.enable_rdp_display_tracking() && self.maximized_window_state.borrow().is_some() {
                    *self.maximized_window_state.borrow_mut() = None;

                    self.log_service.debug(&format!("Cleared maximized window {} state", self.id));
                }

                self.on_did_unmaximize.fire(());
            }
            NativeWindowEvent::Closed => {
                self.on_did_close.fire(());

                self.dispose();
            }
            NativeWindowEvent::Focus => {
                self.clear_notify_focus();

                self.last_focus_time.set(runtime::date_now() as f64);
            }
            NativeWindowEvent::EnterFullScreen => self.on_did_enter_full_screen.fire(()),
            NativeWindowEvent::LeaveFullScreen => self.on_did_leave_full_screen.fire(()),
            NativeWindowEvent::AlwaysOnTopChanged(always_on_top) => self.on_did_change_always_on_top.fire(*always_on_top),
            NativeWindowEvent::SystemContextMenu(point) => {
                // Setup windows/linux system context menu so it only is allowed over the app icon
                let Some(win) = self.win().filter(|_| handle_system_context_menu) else {
                    return;
                };
                let (x, y) = win.get_position();
                let cursor_pos = self.host.screen_to_dip_point(*point);
                let cx = cursor_pos.x.floor() - x;
                let cy = cursor_pos.y.floor() - y;

                // TODO@deepak1556 workaround for https://github.com/microsoft/vscode/issues/250632
                // where showing the custom menu seems broken on Windows
                if self.platform.is_linux && cx > 35.0 /* Cursor is beyond app icon in title bar */ {
                    // `e.event.preventDefault()` é do host, que não repassa o evento nativo.
                    self.on_did_trigger_system_context_menu.fire((cx, cy));
                }
            }
        }
    }

    fn on_display_added(&self, display: &Display) {
        let state = self.maximized_window_state.borrow().clone();
        let win = self.win();
        if let (Some(state), Some(win)) = (state, win) {
            if validate_window_state_on_display(self.host.as_ref(), &state, display) {
                self.log_service.debug_args(&format!("Setting maximized window {} bounds to match newly added display", self.id), &[LogArg::from(state.to_dynamic())]);

                win.set_bounds(Rectangle { x: state.x.unwrap_or_default(), y: state.y.unwrap_or_default(), width: state.width.unwrap_or_default(), height: state.height.unwrap_or_default() });
            }
        }
    }

    /// `applyState(state, hasMultipleDisplays = electron.screen.getAllDisplays().length > 0)`.
    pub fn apply_state(&self, state: &WindowUiState, has_multiple_displays: Option<bool>) {
        let has_multiple_displays = has_multiple_displays.unwrap_or_else(|| !self.host.get_all_displays().is_empty());

        // TODO@electron (Electron 4 regression): when running on multiple displays where the target display
        // to open the window has a larger resolution than the primary display, the window will not size
        // correctly unless we set the bounds again (https://github.com/microsoft/vscode/issues/74872)
        //
        // Extended to cover Windows as well as Mac (https://github.com/microsoft/vscode/issues/146499)
        //
        // However, when running with native tabs with multiple windows we cannot use this workaround
        // because there is a potential that the new window will be added as native tab instead of being
        // a window on its own. In that case calling setBounds() would cause https://github.com/microsoft/vscode/issues/75830

        let window_settings = self.configuration_service.get_value(Some("window"), None);
        let use_native_tabs = self.platform.is_macintosh && matches!(window_settings.get_str("nativeTabs"), Dynamic::Bool(true));
        if (self.platform.is_macintosh || self.platform.is_windows) && has_multiple_displays && (!use_native_tabs || self.host.windows_count_excluding_offscreen() == 1) {
            if let (Some(width), Some(height), Some(x), Some(y)) = (state.width, state.height, state.x, state.y) {
                if let Some(win) = self.win() {
                    win.set_bounds(Rectangle { x, y, width, height });
                }
            }
        }

        if state.mode == Some(WindowMode::Maximized) || state.mode == Some(WindowMode::Fullscreen) {
            // this call may or may not show the window, depends
            // on the platform: currently on Windows and Linux will
            // show the window as active. To be on the safe side,
            // we show the window at the end of this block.
            if let Some(win) = self.win() {
                win.maximize();
            }

            if state.mode == Some(WindowMode::Fullscreen) {
                self.set_full_screen(true, true);
            }

            // to reduce flicker from the default window size
            // to maximize or fullscreen, we only show after
            if let Some(win) = self.win() {
                win.show();
            }
        }
    }

    pub fn set_represented_filename(&self, filename: &str) {
        if self.platform.is_macintosh {
            if let Some(win) = self.win() {
                win.set_represented_filename(filename);
            }
        } else {
            *self.represented_filename.borrow_mut() = Some(filename.to_owned());
        }
    }

    pub fn get_represented_filename(&self) -> Option<String> {
        if self.platform.is_macintosh {
            return self.win().map(|win| win.get_represented_filename());
        }

        self.represented_filename.borrow().clone()
    }

    pub fn set_document_edited(&self, edited: bool) {
        if self.platform.is_macintosh {
            if let Some(win) = self.win() {
                win.set_document_edited(edited);
            }
        }

        self.document_edited.set(Some(edited));
    }

    pub fn is_document_edited(&self) -> bool {
        if self.platform.is_macintosh {
            return self.win().is_some_and(|win| win.is_document_edited());
        }

        self.document_edited.get().unwrap_or(false)
    }

    pub fn set_application_badge(&self, badge: Option<&ApplicationBadge>) {
        let count = badge.map_or(0, |badge| badge.count);

        // Windows has no application wide badge, instead an overlay is
        // rendered over the taskbar icon of this window. The image is drawn
        // by the renderer because the main process cannot draw.
        if self.platform.is_windows {
            let win = self.win();
            match badge.filter(|badge| count > 0 && badge.icon_data_url.is_some()) {
                None => {
                    if let Some(win) = win {
                        win.set_overlay_icon(None, "");
                    }
                }
                Some(badge) => {
                    if let Some(win) = win {
                        win.set_overlay_icon(badge.icon_data_url.as_deref(), &badge.description);
                    }
                }
            }
        }
        // macOS (dock) and Linux (Unity launcher) render the count themselves,
        // on a badge shared by the whole application.
        else {
            let window_id = self.application_badge_window_id.get().unwrap_or(self.id);
            self.application_badge_window_id.set(Some(window_id));
            self.dock_badge_manager.set_count(window_id, count);
        }
    }

    pub fn focus(&self, mode: Option<FocusMode>) {
        match mode.unwrap_or(FocusMode::Transfer) {
            FocusMode::Transfer => self.do_focus_window(),

            FocusMode::Notify => self.show_notify_focus(),

            FocusMode::Force => {
                if self.platform.is_macintosh {
                    if self.host.is_app_hidden() {
                        // Show first: a hidden app ignores window focus and re-focuses its last active window when un-hidden
                        if let Some(win) = self.win() {
                            win.show();
                        }
                    }
                    self.host.focus_app_stealing();
                }
                self.do_focus_window();
            }
        }
    }

    fn show_notify_focus(&self) {
        let disposables = DisposableStore::new();
        self.notify_focus_disposable.set(Some(disposables.clone()));

        // Badge
        disposables.add(self.dock_badge_manager.acquire_badge(self.id));

        // Flash/Bounce
        if self.platform.is_windows || self.platform.is_linux {
            if let Some(win) = self.win() {
                win.flash_frame(true);
            }
            let weak = self.me.clone();
            disposables.add(to_disposable(move || {
                let win = weak.upgrade().and_then(|this| this.win());
                if let Some(win) = win.filter(|win| !win.is_destroyed()) {
                    win.flash_frame(false);
                }
            }));
        } else if self.platform.is_macintosh {
            self.host.bounce_dock_informational();
        }
    }

    fn clear_notify_focus(&self) {
        self.notify_focus_disposable.clear();
    }

    fn do_focus_window(&self) {
        let Some(win) = self.win() else {
            return;
        };

        if win.is_minimized() {
            win.restore();
        }

        win.focus();

        // When focusing the window, the workbench should always be the view that receives focus.
        // However, in scenarios where the window has multiple child views (e.g. browser WebContentsViews),
        // the last focused view in the window may not be the workbench.
        // So we explicitly focus the workbench web contents here to ensure it gets focus.
        win.focus_web_contents();
    }

    // region: Window Control Overlays

    /// `updateWindowControls(options)`.
    pub fn update_window_controls(&self, options: &WindowControlsUpdate) {
        let Some(win) = self.win() else {
            return;
        };

        // Cache the height for speeds lookups on startup
        if let Some(height) = options.height.filter(|height| *height != 0.0) {
            self.state_service.set_item(WINDOW_CONTROL_HEIGHT_STATE_STORAGE_KEY, Dynamic::Number(height));
        }

        // Windows/Linux: update window controls via setTitleBarOverlay()
        if !self.platform.is_macintosh && self.has_window_control_overlay.get() {
            // Update dimmed state if explicitly provided
            if let Some(dimmed) = options.dimmed {
                self.window_controls_dimmed.set(dimmed);
            }

            let last_colors = self.last_window_control_colors.borrow().clone();
            let background_color = options.background_color.clone().or_else(|| last_colors.as_ref().and_then(|colors| colors.background_color.clone()));
            let foreground_color = options.foreground_color.clone().or_else(|| last_colors.as_ref().and_then(|colors| colors.foreground_color.clone()));

            if options.background_color.is_some() || options.foreground_color.is_some() {
                *self.last_window_control_colors.borrow_mut() = Some(WindowControlColors { background_color: background_color.clone(), foreground_color: foreground_color.clone() });
            }

            let dimmed = self.window_controls_dimmed.get();
            let effective_background_color = if dimmed { background_color.as_deref().filter(|color| !color.is_empty()).map(dim_color).or(background_color) } else { background_color };
            let effective_foreground_color = if dimmed { foreground_color.as_deref().filter(|color| !color.is_empty()).map(dim_color).or(foreground_color) } else { foreground_color };

            win.set_title_bar_overlay(TitleBarOverlay {
                color: blank_to_none(effective_background_color),
                symbol_color: blank_to_none(effective_foreground_color),
                height: options.height.filter(|height| *height != 0.0).map(|height| height - 1.0), // account for window border
            });
        }
        // macOS: update window controls via setWindowButtonPosition()
        else if self.platform.is_macintosh {
            if let Some(height) = options.height {
                win.set_window_button_position(get_macos_window_controls_position(height, &os_release(), options.horizontal_inset));
            }
        }
    }

    // endregion

    // region: Fullscreen

    pub fn toggle_full_screen(&self) {
        self.set_full_screen(!self.is_full_screen(), false);
    }

    pub fn set_full_screen(&self, full_screen: bool, from_restore: bool) {
        // Set fullscreen state
        if use_native_full_screen(self.configuration_service.as_ref()) {
            self.set_native_full_screen(full_screen, from_restore);
        } else {
            self.set_simple_full_screen(full_screen);
        }
    }

    /// `get isFullScreen()`.
    pub fn is_full_screen(&self) -> bool {
        if let (true, Some(transient)) = (self.platform.is_macintosh, self.transient_is_native_full_screen.get()) {
            return transient;
        }

        let win = self.win();
        let is_full_screen = win.as_ref().is_some_and(|win| win.is_full_screen());
        let is_simple_full_screen = win.as_ref().is_some_and(|win| win.is_simple_full_screen());

        is_full_screen || is_simple_full_screen
    }

    fn set_native_full_screen(&self, full_screen: bool, from_restore: bool) {
        if let Some(win) = self.win().filter(|win| win.is_simple_full_screen()) {
            win.set_simple_full_screen(false);
        }

        self.do_set_native_full_screen(full_screen, from_restore);
    }

    fn do_set_native_full_screen(&self, full_screen: bool, from_restore: bool) {
        if self.platform.is_macintosh {
            // macOS: Electron windows report `false` for `isFullScreen()` for as long
            // as the fullscreen transition animation takes place. As such, we need to
            // listen to the transition events and carry around an intermediate state
            // for knowing if we are in fullscreen or not
            // Refs: https://github.com/electron/electron/issues/35360

            self.transient_is_native_full_screen.set(Some(full_screen));

            let join_native_full_screen_transition = Rc::new(DeferredPromise::<bool>::new());
            *self.join_native_full_screen_transition.borrow_mut() = Some(join_native_full_screen_transition.clone());

            let me = self.me.clone();
            let transition = join_native_full_screen_transition.clone();
            let _ = async_fn(async move {
                let transitioned = Promise::race(vec![transition.promise.clone(), timeout(10000).then(|_| false)]).await?;

                let Some(this) = me.upgrade() else {
                    return Ok(());
                };
                let is_latest = this.join_native_full_screen_transition.borrow().as_ref().is_some_and(|current| Rc::ptr_eq(current, &transition));
                if !is_latest {
                    return Ok(()); // another transition was requested later
                }

                this.transient_is_native_full_screen.set(None);
                *this.join_native_full_screen_transition.borrow_mut() = None;

                // There is one interesting gotcha on macOS: when you are opening a new
                // window from a fullscreen window, that new window will immediately
                // open fullscreen and emit the `enter-full-screen` event even before we
                // reach this method. In that case, we actually will timeout after 10s
                // for detecting the transition and as such it is important that we only
                // signal to leave fullscreen if the window reports as not being in fullscreen.

                if !transitioned && full_screen && from_restore && this.win().is_some_and(|win| !win.is_full_screen()) {
                    // We have seen requests for fullscreen failing eventually after some
                    // time, for example when an OS update was performed and windows restore.
                    // In those cases a user would find a window that is not in fullscreen
                    // but also does not show any custom titlebar (and thus window controls)
                    // because we think the window is in fullscreen.
                    //
                    // As a workaround in that case we emit a warning and leave fullscreen
                    // so that at least the window controls are back.

                    this.log_service.warn("window: native macOS fullscreen transition did not happen within 10s from restoring");

                    this.on_did_leave_full_screen.fire(());
                }
                Ok(())
            });
        }

        if let Some(win) = self.win() {
            win.set_full_screen(full_screen);
        }
    }

    fn set_simple_full_screen(&self, full_screen: bool) {
        let win = self.win();
        if win.as_ref().is_some_and(|win| win.is_full_screen()) {
            self.do_set_native_full_screen(false, false);
        }

        if let Some(win) = win {
            win.set_simple_full_screen(full_screen);
            win.focus_web_contents(); // workaround issue where focus is not going into window
        }
    }

    // endregion
}

fn base_emitter<T: 'static>(base: &Rc<Disposable>) -> Rc<Emitter<T>> {
    let emitter = Emitter::new();
    base.register(emitter.clone());
    emitter
}

impl IDisposable for BaseWindow {
    fn dispose(&self) {
        self.base.dispose();

        *self.win.borrow_mut() = None; // Important to dereference the window object to allow for GC
    }
}
