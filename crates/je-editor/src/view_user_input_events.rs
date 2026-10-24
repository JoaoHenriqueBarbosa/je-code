//! Porte de `src/vs/editor/browser/view/viewUserInputEvents.ts`: reemite os eventos de entrada do usuário (teclado,
//! mouse, roda) para quem os escuta, convertendo as posições dos alvos do mouse da visão para o modelo.
//!
//! Representação:
//! - `EventCallback<T>` é o [`EventCallback`] (`Rc<dyn Fn(&T)>`); os campos `onKeyDown`... são `RefCell<Option<...>>`
//!   públicos (o original os atribui depois da construção).
//! - `IKeyboardEvent` (`base/browser/keyboardEvent.ts`) e `IMouseWheelEvent` (`base/browser/mouseEvent.ts`) vivem em
//!   crates de interface que `je-editor` não conhece (ver `editor_browser.rs`, que faz o mesmo com o evento de
//!   mouse): são os parâmetros de tipo `K` e `W`, que o dono do `ViewUserInputEvents` instancia com os eventos reais.
//!   O evento só passa por aqui, sem ser lido.
//! - `{ ...target }` e a troca de `position`, `range` e `detail` é o `clone` do alvo seguido da troca dos campos.

use std::cell::RefCell;
use std::rc::Rc;

use crate::coordinates_converter::ICoordinatesConverter;
use crate::editor_browser::{EditorMouseEvent, MouseTarget, MouseTargetViewZoneData, PartialEditorMouseEvent};
use crate::position::Position;

/// `EventCallback<T>`.
pub type EventCallback<T> = Rc<dyn Fn(&T)>;

/// O que `EventCallback<void>` guarda.
pub type VoidCallback = Rc<dyn Fn()>;

/// `ViewUserInputEvents`.
pub struct ViewUserInputEvents<K, W> {
    pub on_key_down: RefCell<Option<EventCallback<K>>>,
    pub on_key_up: RefCell<Option<EventCallback<K>>>,
    pub on_context_menu: RefCell<Option<EventCallback<EditorMouseEvent>>>,
    pub on_mouse_move: RefCell<Option<EventCallback<EditorMouseEvent>>>,
    pub on_mouse_leave: RefCell<Option<EventCallback<PartialEditorMouseEvent>>>,
    pub on_mouse_down: RefCell<Option<EventCallback<EditorMouseEvent>>>,
    pub on_mouse_up: RefCell<Option<EventCallback<EditorMouseEvent>>>,
    pub on_mouse_drag: RefCell<Option<EventCallback<EditorMouseEvent>>>,
    pub on_mouse_drop: RefCell<Option<EventCallback<PartialEditorMouseEvent>>>,
    pub on_mouse_drop_canceled: RefCell<Option<VoidCallback>>,
    pub on_mouse_wheel: RefCell<Option<EventCallback<W>>>,

    coordinates_converter: Rc<dyn ICoordinatesConverter>,
}

/// O `this.onX?.(e)`: o callback é copiado antes da chamada, então ele pode trocar os callbacks sem conflito.
fn emit<T>(slot: &RefCell<Option<EventCallback<T>>>, event: &T) {
    let callback = slot.borrow().clone();
    if let Some(callback) = callback {
        callback(event);
    }
}

impl<K, W> ViewUserInputEvents<K, W> {
    pub fn new(coordinates_converter: Rc<dyn ICoordinatesConverter>) -> Self {
        Self {
            on_key_down: RefCell::new(None),
            on_key_up: RefCell::new(None),
            on_context_menu: RefCell::new(None),
            on_mouse_move: RefCell::new(None),
            on_mouse_leave: RefCell::new(None),
            on_mouse_down: RefCell::new(None),
            on_mouse_up: RefCell::new(None),
            on_mouse_drag: RefCell::new(None),
            on_mouse_drop: RefCell::new(None),
            on_mouse_drop_canceled: RefCell::new(None),
            on_mouse_wheel: RefCell::new(None),
            coordinates_converter,
        }
    }

    pub fn emit_key_down(&self, e: &K) {
        emit(&self.on_key_down, e);
    }

    pub fn emit_key_up(&self, e: &K) {
        emit(&self.on_key_up, e);
    }

    pub fn emit_context_menu(&self, e: &EditorMouseEvent) {
        emit(&self.on_context_menu, &self.convert_view_to_model_mouse_event(e));
    }

    pub fn emit_mouse_move(&self, e: &EditorMouseEvent) {
        emit(&self.on_mouse_move, &self.convert_view_to_model_mouse_event(e));
    }

    pub fn emit_mouse_leave(&self, e: &PartialEditorMouseEvent) {
        emit(&self.on_mouse_leave, &self.convert_view_to_model_partial_mouse_event(e));
    }

    pub fn emit_mouse_down(&self, e: &EditorMouseEvent) {
        emit(&self.on_mouse_down, &self.convert_view_to_model_mouse_event(e));
    }

    pub fn emit_mouse_up(&self, e: &EditorMouseEvent) {
        emit(&self.on_mouse_up, &self.convert_view_to_model_mouse_event(e));
    }

    pub fn emit_mouse_drag(&self, e: &EditorMouseEvent) {
        emit(&self.on_mouse_drag, &self.convert_view_to_model_mouse_event(e));
    }

    pub fn emit_mouse_drop(&self, e: &PartialEditorMouseEvent) {
        emit(&self.on_mouse_drop, &self.convert_view_to_model_partial_mouse_event(e));
    }

    pub fn emit_mouse_drop_canceled(&self) {
        let callback = self.on_mouse_drop_canceled.borrow().clone();
        if let Some(callback) = callback {
            callback();
        }
    }

    pub fn emit_mouse_wheel(&self, e: &W) {
        emit(&self.on_mouse_wheel, e);
    }

    /// `_convertViewToModelMouseEvent(e: IEditorMouseEvent)`.
    fn convert_view_to_model_mouse_event(&self, e: &EditorMouseEvent) -> EditorMouseEvent {
        EditorMouseEvent {
            event: e.event.clone(),
            target: Self::convert_view_to_model_mouse_target(&e.target, &*self.coordinates_converter),
        }
    }

    /// `_convertViewToModelMouseEvent(e: IPartialEditorMouseEvent)`: sem alvo, o evento passa como veio.
    fn convert_view_to_model_partial_mouse_event(&self, e: &PartialEditorMouseEvent) -> PartialEditorMouseEvent {
        match &e.target {
            Some(target) => PartialEditorMouseEvent {
                event: e.event.clone(),
                target: Some(Self::convert_view_to_model_mouse_target(target, &*self.coordinates_converter)),
            },
            None => e.clone(),
        }
    }

    /// `convertViewToModelMouseTarget(target, coordinatesConverter)`.
    pub fn convert_view_to_model_mouse_target(target: &MouseTarget, coordinates_converter: &dyn ICoordinatesConverter) -> MouseTarget {
        let to_model_position = |position: Position| coordinates_converter.convert_view_position_to_model_position(position);
        let mut result = target.clone();
        // O `if (result.position)` e o `if (result.range)`: só os alvos que têm o campo o convertem.
        match &mut result {
            MouseTarget::Unknown(t) => {
                t.position = t.position.map(to_model_position);
                t.range = t.range.map(|range| coordinates_converter.convert_view_range_to_model_range(range));
            }
            MouseTarget::Textarea(_) | MouseTarget::ContentWidget(_) | MouseTarget::OverlayWidget(_) => {}
            MouseTarget::Margin(t) => {
                t.position = to_model_position(t.position);
                t.range = coordinates_converter.convert_view_range_to_model_range(t.range);
            }
            MouseTarget::ViewZone(t) => {
                t.position = to_model_position(t.position);
                t.range = coordinates_converter.convert_view_range_to_model_range(t.range);
                t.detail = Self::convert_view_to_model_view_zone_data(&t.detail, coordinates_converter);
            }
            MouseTarget::ContentText(t) => {
                t.position = to_model_position(t.position);
                t.range = coordinates_converter.convert_view_range_to_model_range(t.range);
            }
            MouseTarget::ContentEmpty(t) => {
                t.position = to_model_position(t.position);
                t.range = coordinates_converter.convert_view_range_to_model_range(t.range);
            }
            MouseTarget::Scrollbar(t) => {
                t.position = to_model_position(t.position);
                t.range = coordinates_converter.convert_view_range_to_model_range(t.range);
            }
            MouseTarget::OverviewRuler(t) => {
                t.position = t.position.map(to_model_position);
                t.range = t.range.map(|range| coordinates_converter.convert_view_range_to_model_range(range));
            }
            MouseTarget::OutsideEditor(t) => {
                t.position = t.position.map(to_model_position);
                t.range = t.range.map(|range| coordinates_converter.convert_view_range_to_model_range(range));
            }
        }
        result
    }

    /// `convertViewToModelViewZoneData(data, coordinatesConverter)`.
    fn convert_view_to_model_view_zone_data(
        data: &MouseTargetViewZoneData,
        coordinates_converter: &dyn ICoordinatesConverter,
    ) -> MouseTargetViewZoneData {
        let to_model_position = |position: Position| coordinates_converter.convert_view_position_to_model_position(position);
        MouseTargetViewZoneData {
            view_zone_id: data.view_zone_id.clone(),
            position_before: data.position_before.map(to_model_position),
            position_after: data.position_after.map(to_model_position),
            position: to_model_position(data.position),
            after_line_number: to_model_position(Position::new(data.after_line_number, 1)).line_number,
        }
    }
}
