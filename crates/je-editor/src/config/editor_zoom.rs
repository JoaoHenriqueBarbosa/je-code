//! Porte de `src/vs/editor/common/config/editorZoom.ts`.
//!
//! O `EditorZoom` do original é um singleton do módulo. Aqui é um `thread_local!`, no mesmo molde do
//! restante do porte (`Rc` e `Emitter` de `je_base::event` não cruzam threads). O nível de zoom nasce em
//! 0, é limitado a `-5..=20` e o evento só dispara quando o valor muda de fato.

use std::cell::Cell;
use std::rc::Rc;

use je_base::event::{Emitter, Event};

/// Menor nível de zoom aceito (`Math.max(-5, zoomLevel)`).
pub const MIN_ZOOM_LEVEL: f64 = -5.0;

/// Maior nível de zoom aceito (`Math.min(.., 20)`).
pub const MAX_ZOOM_LEVEL: f64 = 20.0;

/// O estado do singleton: o nível atual e o emitter de mudança.
struct EditorZoomState {
    zoom_level: Cell<f64>,
    on_did_change_zoom_level: Rc<Emitter<f64>>,
}

thread_local! {
    static EDITOR_ZOOM: EditorZoomState = EditorZoomState {
        zoom_level: Cell::new(0.0),
        on_did_change_zoom_level: Emitter::new(),
    };
}

/// `EditorZoom.onDidChangeZoomLevel`: dispara com o novo nível (já limitado).
pub fn on_did_change_zoom_level() -> Event<f64> {
    EDITOR_ZOOM.with(|zoom| zoom.on_did_change_zoom_level.event())
}

/// `EditorZoom.getZoomLevel()`.
pub fn get_zoom_level() -> f64 {
    EDITOR_ZOOM.with(|zoom| zoom.zoom_level.get())
}

/// `EditorZoom.setZoomLevel(zoomLevel)`: limita a `-5..=20` e só dispara o evento se o valor mudou.
pub fn set_zoom_level(zoom_level: f64) {
    // `Math.min(Math.max(-5, zoomLevel), 20)`: com NaN o `Math.max` devolve NaN, e NaN nunca é igual
    // a nada, então o nível passa a NaN e o evento dispara, como no original.
    let zoom_level = if zoom_level.is_nan() { f64::NAN } else { zoom_level.clamp(MIN_ZOOM_LEVEL, MAX_ZOOM_LEVEL) };
    let emitter = EDITOR_ZOOM.with(|zoom| {
        if zoom.zoom_level.get() == zoom_level {
            return None;
        }
        zoom.zoom_level.set(zoom_level);
        Some(zoom.on_did_change_zoom_level.clone())
    });
    // O `fire` roda fora do `with`, porque um listener pode chamar `get_zoom_level` ou `set_zoom_level`.
    if let Some(emitter) = emitter {
        emitter.fire(zoom_level);
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use super::*;

    #[test]
    fn starts_at_zero() {
        set_zoom_level(0.0);
        assert_eq!(get_zoom_level(), 0.0);
    }

    #[test]
    fn clamps_to_range() {
        set_zoom_level(100.0);
        assert_eq!(get_zoom_level(), 20.0);
        set_zoom_level(-100.0);
        assert_eq!(get_zoom_level(), -5.0);
        set_zoom_level(2.5);
        assert_eq!(get_zoom_level(), 2.5);
        set_zoom_level(0.0);
    }

    #[test]
    fn fires_only_on_change() {
        set_zoom_level(0.0);
        let seen: Rc<RefCell<Vec<f64>>> = Rc::default();
        let subscription = on_did_change_zoom_level().listen({
            let seen = seen.clone();
            move |level| seen.borrow_mut().push(*level)
        });

        set_zoom_level(0.0);
        set_zoom_level(3.0);
        set_zoom_level(3.0);
        set_zoom_level(50.0);
        set_zoom_level(20.0);
        set_zoom_level(0.0);

        assert_eq!(*seen.borrow(), vec![3.0, 20.0, 0.0]);
        subscription.dispose();
    }
}
