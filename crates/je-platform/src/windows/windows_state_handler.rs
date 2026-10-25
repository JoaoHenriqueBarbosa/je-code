//! Porte de `src/vs/platform/windows/electron-main/windowsStateHandler.ts`: o estado das janelas (posição, tamanho, modo,
//! workspace, pasta e backup de cada uma) que o aplicativo salva ao perder o foco, fechar uma janela ou encerrar, e
//! restaura no próximo início, mais o cálculo do estado de uma janela nova.
//!
//! Não há processo principal do Electron no je-code, mas o papel dele existe no processo único; por isso o handler é o
//! mesmo, ligado direto aos eventos do ciclo de vida e das janelas.
//!
//! Decisões de tradução:
//! - **`IWindowsMainService`, `ILifecycleMainService` e `ICodeWindow`** são os contratos reais do porte
//!   ([`WindowsMainService`], [`LifecycleMainService`] e [`ProfilesWindow`], que estende o [`CodeWindow`]), com os membros
//!   que este arquivo leu a mais: `onBeforeShutdown`, `quitRequested` e `wasRestarted` no ciclo de vida;
//!   `getLastActiveWindow()`, `getWindowCount()`, `onDidChangeWindowsCount` e `onDidDestroyWindow` nas janelas;
//!   `id`, `isExtensionTestHost`, `getBounds()`, `serializeWindowState()` e `win?.isSimpleFullScreen()` na janela.
//! - **`electron.app` e `electron.screen`** são o [`ElectronHost`]. O ouvinte de `browser-window-blur`, que no original
//!   nunca é removido, é registrado no `Disposable` do handler e some com ele.
//! - **`IStateService.getItem`/`setItem`** guardam o estado como [`Dynamic`]: o dado serializado
//!   (`ISerializedWindowsState`) é o valor JSON, lido por [`restore_windows_state`] e gerado por
//!   [`get_windows_state_store_data`], que no original devolve o mesmo tipo `IWindowsState` com o conteúdo já serializado.
//! - **`IWindowState`** deste arquivo é [`WindowState`] e `IWindowsState` é [`WindowsState`]; o `IWindowState` de
//!   `window.ts` é o [`WindowUiState`].
//! - **`INativeWindowConfiguration`** é a [`NativeWindowConfiguration`], com `extensionTestsPath`, `extensionDevelopmentPath`,
//!   `workspace` e `backupPath`; `INewWindowState` é o [`NewWindowState`] (o `hasDefaultState` vira campo).
//! - **O estado devolvido por `getNewWindowState`** é uma cópia. No original o objeto devolvido é o mesmo guardado em
//!   `_state`, então trocar `mode` de tela cheia para normal ali também alterava o estado guardado; aqui o estado guardado
//!   não muda (o salvamento seguinte regrava o estado das janelas vivas de qualquer forma).
//! - **`Math.round`** é [`js_round`] (arredonda meio para cima, como o JavaScript).

use je_base::dynamic::Truthy;
use std::cell::{Cell, Ref, RefCell};
use std::collections::HashSet;
use std::rc::Rc;

use je_base::dynamic::Dynamic;
use je_base::lifecycle::{Disposable, IDisposable};
use je_base::platform::Platform;
use je_base::uri::URI;

use crate::configuration::service::IConfigurationService;
use crate::log::{LogArg, LogServiceRef};
use crate::resources::ext_uri_biased_ignore_path_case;
use crate::state::{StateReadService, StateService};
use crate::user_data_profile::user_data_profiles_handler::{LifecycleMainService, ProfilesWindow, WindowsCountChangedEvent, WindowsMainService};
use crate::window::electron_main::{Display, ElectronHost, WindowMode, WindowUiState, default_window_state};
use crate::window::window::{NewWindowDimensions, WindowSettings};
use crate::workspace::{AnyWorkspaceIdentifier, WorkspaceIdentifier};
use crate::workspaces::workspaces_management_main_service::{CodeWindow, NativeWindowConfiguration};

/// `IWindowState`: o estado de uma janela aberta.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct WindowState {
    pub window_id: Option<i32>,
    pub workspace: Option<WorkspaceIdentifier>,
    pub folder_uri: Option<URI>,
    pub backup_path: Option<String>,
    pub remote_authority: Option<String>,
    pub ui_state: WindowUiState,
}

/// `IWindowsState`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct WindowsState {
    pub last_active_window: Option<WindowState>,
    pub last_plugin_development_host_window: Option<WindowState>,
    pub opened_windows: Vec<WindowState>,
}

/// `INewWindowState`: o estado de UI de uma janela nova e se ele é o padrão (e não o de uma sessão anterior).
#[derive(Clone, Debug, PartialEq)]
pub struct NewWindowState {
    pub ui_state: WindowUiState,
    pub has_default_state: bool,
}

/// `Math.round`: arredonda o meio para cima.
fn js_round(value: f64) -> f64 {
    (value + 0.5).floor()
}

const WINDOWS_STATE_STORAGE_KEY: &str = "windowsState";

/// `WindowsStateHandler`.
pub struct WindowsStateHandler {
    base: Rc<Disposable>,

    state: RefCell<WindowsState>,
    last_closed_state: RefCell<Option<WindowState>>,
    shutting_down: Cell<bool>,

    windows_main_service: Rc<dyn WindowsMainService>,
    state_service: Rc<dyn StateService>,
    lifecycle_main_service: Rc<dyn LifecycleMainService>,
    log_service: LogServiceRef,
    configuration_service: Rc<dyn IConfigurationService>,
    electron_host: Rc<dyn ElectronHost>,
}

impl WindowsStateHandler {
    pub fn new(
        windows_main_service: Rc<dyn WindowsMainService>,
        state_service: Rc<dyn StateService>,
        lifecycle_main_service: Rc<dyn LifecycleMainService>,
        log_service: LogServiceRef,
        configuration_service: Rc<dyn IConfigurationService>,
        electron_host: Rc<dyn ElectronHost>,
    ) -> Rc<Self> {
        let state = restore_windows_state(&state_service.get_item(WINDOWS_STATE_STORAGE_KEY, Dynamic::Undefined));

        let this = Rc::new(Self {
            base: Disposable::new(),
            state: RefCell::new(state),
            last_closed_state: RefCell::new(None),
            shutting_down: Cell::new(false),
            windows_main_service,
            state_service,
            lifecycle_main_service,
            log_service,
            configuration_service,
            electron_host,
        });

        this.register_listeners();

        this
    }

    /// `get state()`.
    pub fn state(&self) -> Ref<'_, WindowsState> {
        self.state.borrow()
    }

    /// Um ouvinte que só roda enquanto o handler vive, sem mantê-lo vivo.
    fn while_alive<T: 'static>(this: &Rc<Self>, handler: impl Fn(&Self, &T) + 'static) -> Box<dyn Fn(&T)> {
        let this = Rc::downgrade(this);
        Box::new(move |event: &T| {
            if let Some(this) = this.upgrade() {
                handler(this.as_ref(), event);
            }
        })
    }

    fn register_listeners(self: &Rc<Self>) {
        // When a window looses focus, save all windows state. This allows to
        // prevent loss of window-state data when OS is restarted without properly
        // shutting down the application (https://github.com/microsoft/vscode/issues/87171)
        self.base.register(self.electron_host.on_browser_window_blur().listen(Self::while_alive(self, |this, _: &()| {
            if !this.shutting_down.get() {
                this.save_windows_state();
            }
        })));

        // Handle various lifecycle events around windows
        self.base.register(
            self.lifecycle_main_service
                .on_before_close_window()
                .listen(Self::while_alive(self, |this, window: &Rc<dyn ProfilesWindow>| this.on_before_close_window(window.as_ref()))),
        );
        self.base.register(self.lifecycle_main_service.on_before_shutdown().listen(Self::while_alive(self, |this, _: &()| this.on_before_shutdown())));
        self.base.register(self.windows_main_service.on_did_change_windows_count().listen(Self::while_alive(self, |this, e: &WindowsCountChangedEvent| {
            if e.new_count > e.old_count {
                // clear last closed window state when a new window opens. this helps on macOS where
                // otherwise closing the last window, opening a new window and then quitting would
                // use the state of the previously closed window when restarting.
                *this.last_closed_state.borrow_mut() = None;
            }
        })));

        // try to save state before destroy because close will not fire
        self.base.register(
            self.windows_main_service.on_did_destroy_window().listen(Self::while_alive(self, |this, window: &Rc<dyn ProfilesWindow>| this.on_before_close_window(window.as_ref()))),
        );
    }

    // Note that onBeforeShutdown() and onBeforeCloseWindow() are fired in different order depending on the OS:
    // - macOS: since the app will not quit when closing the last window, you will always first get
    //          the onBeforeShutdown() event followed by N onBeforeCloseWindow() events for each window
    // - other: on other OS, closing the last window will quit the app so the order depends on the
    //          user interaction: closing the last window will first trigger onBeforeCloseWindow()
    //          and then onBeforeShutdown(). Using the quit action however will first issue onBeforeShutdown()
    //          and then onBeforeCloseWindow().
    //
    // Here is the behavior on different OS depending on action taken (Electron 1.7.x):
    //
    // Legend
    // -  quit(N): quit application with N windows opened
    // - close(1): close one window via the window close button
    // - closeAll: close all windows via the taskbar command
    // - onBeforeShutdown(N): number of windows reported in this event handler
    // - onBeforeCloseWindow(N, M): number of windows reported and quitRequested boolean in this event handler
    //
    // macOS
    // 	-     quit(1): onBeforeShutdown(1), onBeforeCloseWindow(1, true)
    // 	-     quit(2): onBeforeShutdown(2), onBeforeCloseWindow(2, true), onBeforeCloseWindow(2, true)
    // 	-     quit(0): onBeforeShutdown(0)
    // 	-    close(1): onBeforeCloseWindow(1, false)
    //
    // Windows
    // 	-     quit(1): onBeforeShutdown(1), onBeforeCloseWindow(1, true)
    // 	-     quit(2): onBeforeShutdown(2), onBeforeCloseWindow(2, true), onBeforeCloseWindow(2, true)
    // 	-    close(1): onBeforeCloseWindow(2, false)[not last window]
    // 	-    close(1): onBeforeCloseWindow(1, false), onBeforeShutdown(0)[last window]
    // 	- closeAll(2): onBeforeCloseWindow(2, false), onBeforeCloseWindow(2, false), onBeforeShutdown(0)
    //
    // Linux
    // 	-     quit(1): onBeforeShutdown(1), onBeforeCloseWindow(1, true)
    // 	-     quit(2): onBeforeShutdown(2), onBeforeCloseWindow(2, true), onBeforeCloseWindow(2, true)
    // 	-    close(1): onBeforeCloseWindow(2, false)[not last window]
    // 	-    close(1): onBeforeCloseWindow(1, false), onBeforeShutdown(0)[last window]
    // 	- closeAll(2): onBeforeCloseWindow(2, false), onBeforeCloseWindow(2, false), onBeforeShutdown(0)
    //
    fn on_before_shutdown(&self) {
        self.shutting_down.set(true);

        self.save_windows_state();
    }

    fn save_windows_state(&self) {
        // TODO@electron workaround for Electron not being able to restore
        // multiple (native) fullscreen windows on the same display at once
        // on macOS.
        // https://github.com/electron/electron/issues/34367
        let mut displays_with_full_screen_window: HashSet<Option<i64>> = HashSet::new();
        let is_macintosh = Platform::detect().is_macintosh;

        let mut current_windows_state = WindowsState {
            opened_windows: Vec::new(),
            last_plugin_development_host_window: self.state.borrow().last_plugin_development_host_window.clone(),
            last_active_window: self.last_closed_state.borrow().clone(),
        };

        // 1.) Find a last active window (pick any other first window otherwise)
        if current_windows_state.last_active_window.is_none() {
            let mut active_window = self.windows_main_service.get_last_active_window();
            if active_window.as_ref().is_none_or(|window| window.is_extension_development_host()) {
                active_window = self.windows_main_service.get_windows().into_iter().find(|window| !window.is_extension_development_host());
            }

            if let Some(active_window) = active_window {
                let last_active_window = self.to_window_state(active_window.as_ref());

                if last_active_window.ui_state.mode == Some(WindowMode::Fullscreen) {
                    displays_with_full_screen_window.insert(last_active_window.ui_state.display); // always allow fullscreen for active window
                }
                current_windows_state.last_active_window = Some(last_active_window);
            }
        }

        // 2.) Find extension host window
        let extension_host_window = self.windows_main_service.get_windows().into_iter().find(|window| window.is_extension_development_host() && !window.is_extension_test_host());
        if let Some(extension_host_window) = extension_host_window {
            let mut last_plugin_development_host_window = self.to_window_state(extension_host_window.as_ref());

            if last_plugin_development_host_window.ui_state.mode == Some(WindowMode::Fullscreen) {
                if displays_with_full_screen_window.contains(&last_plugin_development_host_window.ui_state.display) {
                    if is_macintosh && !extension_host_window.is_simple_full_screen() {
                        last_plugin_development_host_window.ui_state.mode = Some(WindowMode::Normal);
                    }
                } else {
                    displays_with_full_screen_window.insert(last_plugin_development_host_window.ui_state.display);
                }
            }
            current_windows_state.last_plugin_development_host_window = Some(last_plugin_development_host_window);
        }

        // 3.) All windows (except extension host) for N >= 2 to support `restoreWindows: all` or for auto update
        //
        // Careful here: asking a window for its window state after it has been closed returns bogus values (width: 0, height: 0)
        // so if we ever want to persist the UI state of the last closed window (window count === 1), it has
        // to come from the stored lastClosedWindowState on Win/Linux at least
        if self.windows_main_service.get_window_count() > 1 {
            let last_active_window_id = current_windows_state.last_active_window.as_ref().and_then(|window| window.window_id);

            for window in self.windows_main_service.get_windows().into_iter().filter(|window| !window.is_extension_development_host()) {
                let mut window_state = self.to_window_state(window.as_ref());

                if window_state.ui_state.mode == Some(WindowMode::Fullscreen) {
                    if displays_with_full_screen_window.contains(&window_state.ui_state.display) {
                        if is_macintosh && window_state.window_id != last_active_window_id && !window.is_simple_full_screen() {
                            window_state.ui_state.mode = Some(WindowMode::Normal);
                        }
                    } else {
                        displays_with_full_screen_window.insert(window_state.ui_state.display);
                    }
                }

                current_windows_state.opened_windows.push(window_state);
            }
        }

        // Persist
        let state = get_windows_state_store_data(&current_windows_state);
        self.state_service.set_item(WINDOWS_STATE_STORAGE_KEY, state.clone());

        if self.shutting_down.get() {
            self.log_service.trace_args("[WindowsStateHandler] onBeforeShutdown", &[LogArg::from(state)]);
        }
    }

    // See note on #onBeforeShutdown() for details how these events are flowing
    fn on_before_close_window(&self, window: &dyn ProfilesWindow) {
        if self.lifecycle_main_service.quit_requested() {
            return; // during quit, many windows close in parallel so let it be handled in the before-quit handler
        }

        // On Window close, update our stored UI state of this window
        let state = self.to_window_state(window);
        if window.is_extension_development_host() && !window.is_extension_test_host() {
            self.state.borrow_mut().last_plugin_development_host_window = Some(state.clone()); // do not let test run window state overwrite our extension development state
        }
        // Any non extension host window with same workspace or folder
        else if !window.is_extension_development_host()
            && let Some(opened_workspace) = window.opened_workspace()
        {
            let ext_uri = ext_uri_biased_ignore_path_case();
            for opened_window in self.state.borrow_mut().opened_windows.iter_mut() {
                let same_workspace = match &opened_workspace {
                    AnyWorkspaceIdentifier::Workspace(workspace) => opened_window.workspace.as_ref().is_some_and(|opened| opened.id == workspace.id),
                    _ => false,
                };
                let same_folder = match &opened_workspace {
                    AnyWorkspaceIdentifier::SingleFolder(folder) => opened_window.folder_uri.as_ref().is_some_and(|opened| ext_uri.is_equal(Some(opened), Some(&folder.uri), false)),
                    _ => false,
                };

                if same_workspace || same_folder {
                    opened_window.ui_state = state.ui_state.clone();
                }
            }
        }

        // On Windows and Linux closing the last window will trigger quit. Since we are storing all UI state
        // before quitting, we need to remember the UI state of this window to be able to persist it.
        // On macOS we keep the last closed window state ready in case the user wants to quit right after or
        // wants to open another window, in which case we use this state over the persisted one.
        if self.windows_main_service.get_window_count() == 1 {
            *self.last_closed_state.borrow_mut() = Some(state);
        }
    }

    fn to_window_state(&self, window: &dyn ProfilesWindow) -> WindowState {
        let opened_workspace = window.opened_workspace();
        WindowState {
            window_id: Some(window.id()),
            workspace: match &opened_workspace {
                Some(AnyWorkspaceIdentifier::Workspace(workspace)) => Some(workspace.clone()),
                _ => None,
            },
            folder_uri: match &opened_workspace {
                Some(AnyWorkspaceIdentifier::SingleFolder(folder)) => Some(folder.uri.clone()),
                _ => None,
            },
            backup_path: window.backup_path(),
            remote_authority: window.remote_authority(),
            ui_state: window.serialize_window_state(),
        }
    }

    /// `configurationService.getValue<IWindowSettings | undefined>('window')`.
    fn window_settings(&self) -> Option<WindowSettings> {
        WindowSettings::from_dynamic(&self.configuration_service.get_value(Some("window"), None))
    }

    pub fn get_new_window_state(&self, configuration: &NativeWindowConfiguration) -> NewWindowState {
        let mut state = self.do_get_new_window_state(configuration);
        let window_config = self.window_settings();

        // Fullscreen state gets special treatment
        if state.ui_state.mode == Some(WindowMode::Fullscreen) {
            // Window state is not from a previous session: only allow fullscreen if we inherit it or user wants fullscreen
            let allow_fullscreen = if state.has_default_state {
                matches!(
                    window_config.and_then(|config| config.new_window_dimensions),
                    Some(NewWindowDimensions::Fullscreen | NewWindowDimensions::Inherit | NewWindowDimensions::Offset)
                )
            }
            // Window state is from a previous session: only allow fullscreen when we got updated or user wants to restore
            else {
                self.lifecycle_main_service.was_restarted() || window_config.is_some_and(|config| config.restore_fullscreen)
            };

            if !allow_fullscreen {
                state.ui_state.mode = Some(WindowMode::Normal);
            }
        }

        state
    }

    fn do_get_new_window_state(&self, configuration: &NativeWindowConfiguration) -> NewWindowState {
        let last_active = self.windows_main_service.get_last_active_window();
        let ext_uri = ext_uri_biased_ignore_path_case();
        let stored = |ui_state: &WindowUiState| NewWindowState { ui_state: ui_state.clone(), has_default_state: false };

        // Restore state unless we are running extension tests
        if configuration.extension_tests_path.as_deref().is_none_or(str::is_empty) {
            let state = self.state.borrow();

            // extension development host Window - load from stored settings if any
            if configuration.extension_development_path.is_some()
                && let Some(last_plugin_development_host_window) = &state.last_plugin_development_host_window
            {
                return stored(&last_plugin_development_host_window.ui_state);
            }

            // Known Workspace - load from stored settings
            let workspace = configuration.workspace.as_ref();
            if let Some(AnyWorkspaceIdentifier::Workspace(workspace)) = workspace
                && let Some(opened_window) = state.opened_windows.iter().find(|opened_window| opened_window.workspace.as_ref().is_some_and(|opened| opened.id == workspace.id))
            {
                return stored(&opened_window.ui_state);
            }

            // Known Folder - load from stored settings
            if let Some(AnyWorkspaceIdentifier::SingleFolder(folder)) = workspace {
                if let Some(opened_window) = state
                    .opened_windows
                    .iter()
                    .find(|opened_window| opened_window.folder_uri.as_ref().is_some_and(|opened| ext_uri.is_equal(Some(opened), Some(&folder.uri), false)))
                {
                    return stored(&opened_window.ui_state);
                }
            }
            // Empty windows with backups
            else if let Some(backup_path) = configuration.backup_path.as_deref().filter(|backup_path| !backup_path.is_empty())
                && let Some(opened_window) = state.opened_windows.iter().find(|opened_window| opened_window.backup_path.as_deref() == Some(backup_path))
            {
                return stored(&opened_window.ui_state);
            }

            // First Window
            let last_active_state = self.last_closed_state.borrow().clone().or_else(|| state.last_active_window.clone());
            if last_active.is_none()
                && let Some(last_active_state) = last_active_state
            {
                return stored(&last_active_state.ui_state);
            }
        }

        //
        // In any other case, we do not have any stored settings for the window state, so we come up with something smart
        //

        // We want the new window to open on the same display that the last active one is in
        let display_to_use = self.display_for_new_window(last_active.as_deref());

        // Compute x/y based on display bounds
        // Note: important to use Math.round() because Electron does not seem to be too happy about
        // display coordinates that are not absolute numbers.
        let has_workspace = configuration.workspace.as_ref().is_some_and(|workspace| workspace.is_workspace_identifier() || workspace.is_single_folder_workspace_identifier());
        let mut state = default_window_state(WindowMode::Normal, has_workspace);
        state.x = Some(js_round(display_to_use.bounds.x + (display_to_use.bounds.width / 2.0) - (state.width.unwrap_or_default() / 2.0)));
        state.y = Some(js_round(display_to_use.bounds.y + (display_to_use.bounds.height / 2.0) - (state.height.unwrap_or_default() / 2.0)));

        // Check for newWindowDimensions setting and adjust accordingly
        let mut ensure_no_overlap = true;
        match self.window_settings().and_then(|config| config.new_window_dimensions) {
            Some(NewWindowDimensions::Maximized) => {
                state.mode = Some(WindowMode::Maximized);
                ensure_no_overlap = false;
            }
            Some(NewWindowDimensions::Fullscreen) => {
                state.mode = Some(WindowMode::Fullscreen);
                ensure_no_overlap = false;
            }
            Some(dimensions @ (NewWindowDimensions::Inherit | NewWindowDimensions::Offset)) if last_active.is_some() => {
                let last_active_state = last_active.as_ref().map(|window| window.serialize_window_state()).unwrap_or_default();
                if last_active_state.mode == Some(WindowMode::Fullscreen) {
                    state.mode = Some(WindowMode::Fullscreen); // only take mode (fixes https://github.com/microsoft/vscode/issues/19331)
                } else {
                    state = WindowUiState {
                        zoom_level: None, // do not inherit zoom level
                        ..last_active_state
                    };
                }

                ensure_no_overlap = state.mode != Some(WindowMode::Fullscreen) && dimensions == NewWindowDimensions::Offset;
            }
            _ => {}
        }

        if ensure_no_overlap {
            state = self.ensure_no_overlap(state);
        }

        NewWindowState { ui_state: state, has_default_state: true } // flag as default state
    }

    /// O monitor onde a janela nova abre.
    fn display_for_new_window(&self, last_active: Option<&dyn ProfilesWindow>) -> Display {
        let displays = self.electron_host.get_all_displays();

        // Single Display
        if displays.len() == 1 {
            return displays[0];
        }

        // Multi Display

        // on mac there is 1 menu per window so we need to use the monitor where the cursor currently is
        if Platform::detect().is_macintosh {
            let cursor_point = self.electron_host.get_cursor_screen_point();
            return self.electron_host.get_display_nearest_point(cursor_point);
        }

        // if we have a last active window, use that display for the new window
        if let Some(last_active) = last_active {
            return self.electron_host.get_display_matching(last_active.get_bounds());
        }

        // fallback to primary display or first display
        self.electron_host.get_primary_display()
    }

    fn ensure_no_overlap(&self, mut state: WindowUiState) -> WindowUiState {
        let windows = self.windows_main_service.get_windows();
        if windows.is_empty() {
            return state;
        }

        let mut x = state.x.unwrap_or(0.0);
        let mut y = state.y.unwrap_or(0.0);

        let existing_window_bounds: Vec<_> = windows.iter().map(|window| window.get_bounds()).collect();
        while existing_window_bounds.iter().any(|bounds| bounds.x == x || bounds.y == y) {
            x += 30.0;
            y += 30.0;
        }

        state.x = Some(x);
        state.y = Some(y);

        state
    }
}

impl IDisposable for WindowsStateHandler {
    fn dispose(&self) {
        self.base.dispose();
    }
}

/// `restoreWindowsState`: o estado das janelas a partir do valor guardado (`ISerializedWindowsState | undefined`).
pub fn restore_windows_state(data: &Dynamic) -> WindowsState {
    let mut result = WindowsState::default();
    if data.is_nullish() {
        return result;
    }

    let last_active_window = data.get_str("lastActiveWindow");
    if last_active_window.is_truthy() {
        result.last_active_window = Some(restore_window_state(&last_active_window));
    }

    let last_plugin_development_host_window = data.get_str("lastPluginDevelopmentHostWindow");
    if last_plugin_development_host_window.is_truthy() {
        result.last_plugin_development_host_window = Some(restore_window_state(&last_plugin_development_host_window));
    }

    if let Dynamic::Array(opened_windows) = data.get_str("openedWindows") {
        result.opened_windows = opened_windows.to_vec().iter().map(restore_window_state).collect();
    }

    result
}

/// O texto de `key`, quando é um texto não vazio (o `if (windowState.backupPath)` do original).
fn non_empty_text(value: &Dynamic, key: &str) -> Option<String> {
    match value.get_str(key) {
        Dynamic::String(text) if !text.units().is_empty() => Some(text.to_string_lossy()),
        _ => None,
    }
}

fn restore_window_state(window_state: &Dynamic) -> WindowState {
    let mut result = WindowState { ui_state: WindowUiState::from_dynamic(&window_state.get_str("uiState")), ..Default::default() };
    result.backup_path = non_empty_text(window_state, "backupPath");
    result.remote_authority = non_empty_text(window_state, "remoteAuthority");

    if let Some(folder) = non_empty_text(window_state, "folder") {
        result.folder_uri = Some(URI::parse(folder.as_str(), false));
    }

    let workspace_identifier = window_state.get_str("workspaceIdentifier");
    if workspace_identifier.is_truthy() {
        result.workspace = Some(WorkspaceIdentifier {
            id: non_empty_text(&workspace_identifier, "id").unwrap_or_default(),
            config_path: URI::parse(non_empty_text(&workspace_identifier, "configURIPath").unwrap_or_default().as_str(), false),
        });
    }

    result
}

/// `getWindowsStateStoreData`: o estado das janelas como o valor guardado (`ISerializedWindowsState`).
pub fn get_windows_state_store_data(windows_state: &WindowsState) -> Dynamic {
    let mut entries: Vec<(&str, Dynamic)> = Vec::new();
    if let Some(last_active_window) = &windows_state.last_active_window {
        entries.push(("lastActiveWindow", serialize_window_state(last_active_window)));
    }
    if let Some(last_plugin_development_host_window) = &windows_state.last_plugin_development_host_window {
        entries.push(("lastPluginDevelopmentHostWindow", serialize_window_state(last_plugin_development_host_window)));
    }
    entries.push(("openedWindows", Dynamic::array(windows_state.opened_windows.iter().map(serialize_window_state))));
    Dynamic::object(entries)
}

fn serialize_window_state(window_state: &WindowState) -> Dynamic {
    let mut entries: Vec<(&str, Dynamic)> = Vec::new();
    if let Some(workspace) = &window_state.workspace {
        entries.push(("workspaceIdentifier", Dynamic::object([("id", Dynamic::string(&workspace.id)), ("configURIPath", Dynamic::String(workspace.config_path.to_string(false)))])));
    }
    if let Some(folder_uri) = &window_state.folder_uri {
        entries.push(("folder", Dynamic::String(folder_uri.to_string(false))));
    }
    if let Some(backup_path) = &window_state.backup_path {
        entries.push(("backupPath", Dynamic::string(backup_path)));
    }
    if let Some(remote_authority) = &window_state.remote_authority {
        entries.push(("remoteAuthority", Dynamic::string(remote_authority)));
    }
    entries.push(("uiState", window_state.ui_state.to_dynamic()));
    Dynamic::object(entries)
}
