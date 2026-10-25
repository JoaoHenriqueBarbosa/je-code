//! Porte do `WindowManager` de `src/vs/base/browser/browser.ts`: o nível de zoom, o fator de zoom e o estado de tela cheia
//! de cada janela, e os eventos de mudança.
//!
//! Divergências:
//! - **Singleton.** `WindowManager.INSTANCE` é um `thread_local!` (o `Rc` e o `Emitter` de `je_base::event` não cruzam
//!   threads), no mesmo molde do `EditorZoom`.
//! - **Janela.** `targetWindow: Window` é a [`CodeWindow`] do modelo de documento nativo; o identificador é o
//!   `vscodeWindowId`, que pode faltar (`None`, o `undefined` do original) e então vale como uma chave como qualquer outra.
//! - **Fora do porte.** O que o arquivo tira do `navigator` e do `matchMedia` (`isFirefox`, `isSafari`, `isStandalone`,
//!   `isIOS`...) não existe na renderização nativa; entra se um chamador portado precisar.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use crate::event::{Emitter, Event};

use super::dom::CodeWindow;

/// `WindowManager`.
struct WindowManager {
    map_window_id_to_zoom_level: RefCell<HashMap<Option<u32>, f64>>,
    on_did_change_zoom_level: Rc<Emitter<Option<u32>>>,
    map_window_id_to_zoom_factor: RefCell<HashMap<Option<u32>, f64>>,
    on_did_change_fullscreen: Rc<Emitter<Option<u32>>>,
    map_window_id_to_full_screen: RefCell<HashMap<Option<u32>, bool>>,
}

impl WindowManager {
    fn new() -> Self {
        Self {
            map_window_id_to_zoom_level: RefCell::new(HashMap::new()),
            on_did_change_zoom_level: Emitter::new(),
            map_window_id_to_zoom_factor: RefCell::new(HashMap::new()),
            on_did_change_fullscreen: Emitter::new(),
            map_window_id_to_full_screen: RefCell::new(HashMap::new()),
        }
    }

    fn get_zoom_level(&self, target_window: &CodeWindow) -> f64 {
        self.map_window_id_to_zoom_level.borrow().get(&target_window.vscode_window_id()).copied().unwrap_or(0.0)
    }

    fn set_zoom_level(&self, zoom_level: f64, target_window: &CodeWindow) {
        if self.get_zoom_level(target_window) == zoom_level {
            return;
        }

        let target_window_id = target_window.vscode_window_id();
        self.map_window_id_to_zoom_level.borrow_mut().insert(target_window_id, zoom_level);
        self.on_did_change_zoom_level.fire(target_window_id);
    }

    fn get_zoom_factor(&self, target_window: &CodeWindow) -> f64 {
        self.map_window_id_to_zoom_factor.borrow().get(&target_window.vscode_window_id()).copied().unwrap_or(1.0)
    }

    fn set_zoom_factor(&self, zoom_factor: f64, target_window: &CodeWindow) {
        self.map_window_id_to_zoom_factor.borrow_mut().insert(target_window.vscode_window_id(), zoom_factor);
    }

    fn set_fullscreen(&self, fullscreen: bool, target_window: &CodeWindow) {
        if self.is_fullscreen(target_window) == fullscreen {
            return;
        }

        let window_id = target_window.vscode_window_id();
        self.map_window_id_to_full_screen.borrow_mut().insert(window_id, fullscreen);
        self.on_did_change_fullscreen.fire(window_id);
    }

    fn is_fullscreen(&self, target_window: &CodeWindow) -> bool {
        self.map_window_id_to_full_screen.borrow().get(&target_window.vscode_window_id()).copied().unwrap_or(false)
    }
}

thread_local! {
    static WINDOW_MANAGER: WindowManager = WindowManager::new();
}

/// `setZoomLevel(zoomLevel, targetWindow)`: a zoom index, e.g. 1, 2, 3.
pub fn set_zoom_level(zoom_level: f64, target_window: &CodeWindow) {
    WINDOW_MANAGER.with(|manager| manager.set_zoom_level(zoom_level, target_window));
}

/// `getZoomLevel(targetWindow)`.
pub fn get_zoom_level(target_window: &CodeWindow) -> f64 {
    WINDOW_MANAGER.with(|manager| manager.get_zoom_level(target_window))
}

/// `onDidChangeZoomLevel`: dispara com o `vscodeWindowId` da janela cujo nível mudou.
pub fn on_did_change_zoom_level() -> Event<Option<u32>> {
    WINDOW_MANAGER.with(|manager| manager.on_did_change_zoom_level.event())
}

/// `getZoomFactor(targetWindow)`: the zoom scale for an index, e.g. 1, 1.2, 1.4.
pub fn get_zoom_factor(target_window: &CodeWindow) -> f64 {
    WINDOW_MANAGER.with(|manager| manager.get_zoom_factor(target_window))
}

/// `setZoomFactor(zoomFactor, targetWindow)`.
pub fn set_zoom_factor(zoom_factor: f64, target_window: &CodeWindow) {
    WINDOW_MANAGER.with(|manager| manager.set_zoom_factor(zoom_factor, target_window));
}

/// `setFullscreen(fullscreen, targetWindow)`.
pub fn set_fullscreen(fullscreen: bool, target_window: &CodeWindow) {
    WINDOW_MANAGER.with(|manager| manager.set_fullscreen(fullscreen, target_window));
}

/// `isFullscreen(targetWindow)`.
pub fn is_fullscreen(target_window: &CodeWindow) -> bool {
    WINDOW_MANAGER.with(|manager| manager.is_fullscreen(target_window))
}

/// `onDidChangeFullscreen`: dispara com o `vscodeWindowId` da janela cuja tela cheia mudou.
pub fn on_did_change_fullscreen() -> Event<Option<u32>> {
    WINDOW_MANAGER.with(|manager| manager.on_did_change_fullscreen.event())
}
