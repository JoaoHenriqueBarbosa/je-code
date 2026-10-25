//! Porte de `src/vs/platform/window/electron-main/window.ts`, só o que o lado "main" do processo único lê: o estado de
//! UI da janela (`IWindowState`, `WindowMode`, `defaultWindowState`) e o que o original pede ao módulo `electron`
//! (`electron.app` e `electron.screen`), que aqui é o [`ElectronHost`].
//!
//! Decisões de tradução:
//! - **`IWindowState`** é [`WindowUiState`] (o `windowsStateHandler.ts` o importa como `IWindowUIState`, porque lá
//!   `IWindowState` é outra coisa). Os campos opcionais são `Option`; `display` é `Option<i64>` (o `id` de um
//!   `electron.Display` é um inteiro seguro).
//! - **`WindowMode`** guarda os valores numéricos do original (`Maximized = 0`...), porque o estado salvo em disco os usa.
//!   Um número fora de 0 a 3 no estado salvo é lido como ausente.
//! - **`electron.Rectangle`, `electron.Point` e `electron.Display`** são [`Rectangle`], [`Point`] e [`Display`] (só `bounds`,
//!   que é o que o `WindowsStateHandler` lê).
//! - **`electron.app.on('browser-window-blur')` e `electron.screen`** são o [`ElectronHost`]: o hospedeiro do processo único
//!   responde pelos monitores e avisa quando uma janela perde o foco.
//! - **`defaultAuxWindowState`** fica de fora: depende de `electron.screen.getPrimaryDisplay().workArea` e só a janela
//!   auxiliar a chama.

use je_base::dynamic::Dynamic;
use je_base::event::Event;

use super::window::{DEFAULT_EMPTY_WINDOW_SIZE, DEFAULT_WORKSPACE_WINDOW_SIZE};

/// `WindowMode`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WindowMode {
    Maximized = 0,
    Normal = 1,
    /// Not used anymore, but also cannot remove due to existing stored UI state (needs migration).
    Minimized = 2,
    Fullscreen = 3,
}

impl WindowMode {
    /// O modo guardado como número no estado salvo; `None` para um número desconhecido.
    pub fn from_number(value: f64) -> Option<Self> {
        match value {
            v if v == 0.0 => Some(Self::Maximized),
            v if v == 1.0 => Some(Self::Normal),
            v if v == 2.0 => Some(Self::Minimized),
            v if v == 3.0 => Some(Self::Fullscreen),
            _ => None,
        }
    }
}

/// `IWindowState` de `electron-main/window.ts`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct WindowUiState {
    pub width: Option<f64>,
    pub height: Option<f64>,
    pub x: Option<f64>,
    pub y: Option<f64>,
    pub mode: Option<WindowMode>,
    pub zoom_level: Option<f64>,
    pub display: Option<i64>,
}

fn number_of(value: &Dynamic, key: &str) -> Option<f64> {
    match value.get_str(key) {
        Dynamic::Number(number) => Some(number),
        _ => None,
    }
}

impl WindowUiState {
    /// O estado de UI como o `JSON.parse` do estado salvo o entrega; um valor que não é objeto dá o estado vazio.
    pub fn from_dynamic(value: &Dynamic) -> Self {
        if !matches!(value, Dynamic::Object(_)) {
            return Self::default();
        }
        Self {
            width: number_of(value, "width"),
            height: number_of(value, "height"),
            x: number_of(value, "x"),
            y: number_of(value, "y"),
            mode: number_of(value, "mode").and_then(WindowMode::from_number),
            zoom_level: number_of(value, "zoomLevel"),
            display: number_of(value, "display").map(|display| display as i64),
        }
    }

    /// O estado de UI como o `JSON.stringify` o grava: só as chaves definidas.
    pub fn to_dynamic(&self) -> Dynamic {
        let mut entries: Vec<(&str, Dynamic)> = Vec::new();
        let mut push = |key: &'static str, value: Option<f64>| {
            if let Some(value) = value {
                entries.push((key, Dynamic::Number(value)));
            }
        };
        push("width", self.width);
        push("height", self.height);
        push("x", self.x);
        push("y", self.y);
        push("mode", self.mode.map(|mode| f64::from(mode as i32)));
        push("zoomLevel", self.zoom_level);
        push("display", self.display.map(|display| display as f64));
        Dynamic::object(entries)
    }
}

/// `defaultWindowState(mode, hasWorkspace)`.
pub fn default_window_state(mode: WindowMode, has_workspace: bool) -> WindowUiState {
    let size = if has_workspace { DEFAULT_WORKSPACE_WINDOW_SIZE } else { DEFAULT_EMPTY_WINDOW_SIZE };
    WindowUiState { width: Some(size.width), height: Some(size.height), mode: Some(mode), ..Default::default() }
}

/// `electron.Rectangle`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rectangle {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

/// `electron.Point`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

/// `electron.Display`, só com os limites.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Display {
    pub bounds: Rectangle,
}

/// `electron.app` e `electron.screen`, o que o lado "main" pede ao hospedeiro.
pub trait ElectronHost {
    /// `electron.app.on('browser-window-blur')`: uma janela perdeu o foco.
    fn on_browser_window_blur(&self) -> Event<()>;

    /// `electron.screen.getAllDisplays()`.
    fn get_all_displays(&self) -> Vec<Display>;

    /// `electron.screen.getPrimaryDisplay()`.
    fn get_primary_display(&self) -> Display;

    /// `electron.screen.getCursorScreenPoint()`.
    fn get_cursor_screen_point(&self) -> Point;

    /// `electron.screen.getDisplayNearestPoint(point)`.
    fn get_display_nearest_point(&self, point: Point) -> Display;

    /// `electron.screen.getDisplayMatching(rect)`.
    fn get_display_matching(&self, rect: Rectangle) -> Display;
}
