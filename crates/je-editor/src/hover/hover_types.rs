//! Porte de `src/vs/editor/contrib/hover/browser/hoverTypes.ts`, a parte que não depende de DOM: `IHoverPart`,
//! os ancoradores (`HoverAnchorType`, `HoverRangeAnchor`, `HoverForeignElementAnchor`, `HoverAnchor`) e o
//! contrato do participante do hover.
//!
//! Diferenças do original:
//! - `IEditorHoverParticipant` traz só os membros que não dependem de tipos sem porte: `hoverOrdinal`,
//!   `hideCopyButton`, `suggestHoverAnchor`, `getAccessibleContent`, `handleResize`, `handleHide` e
//!   `handleContentsChanged`. Ficam de fora `computeSync`, `computeAsync` e `createLoadingMessage` (precisam do
//!   `HoverStartSource`, de `hoverOperation.ts`, e do `IModelDecoration`), `renderHoverParts` (monta DOM, que a
//!   UI nativa troca pelos widgets) e `handleScroll`. O `T extends IHoverPart` genérico dos métodos vira o
//!   `&dyn IHoverPart`.
//! - Os membros opcionais (`?`) viram métodos com o corpo padrão do "não definido".
//! - `owner` é o `Rc<dyn IEditorHoverParticipant>`; a igualdade de dono (`this.owner === other.owner`) compara o
//!   endereço do objeto.
//! - `IEditorHoverStatusBar`, `IEditorHoverAction`, `IEditorHoverColorPickerWidget`, `IEditorHoverContext`,
//!   `IEditorHoverRenderContext`, `IRenderedHoverPart(s)`, `RenderedHoverParts`, `HoverParticipantRegistry` e
//!   `IHoverWidget` descrevem a montagem do widget em DOM e ficam para o porte dele.
//! - `initialMousePosX` e `initialMousePosY` (`number | undefined`) são `Option<f64>`.

use std::rc::Rc;

use crate::editor_browser::EditorMouseEvent;
use crate::position::Position;
use crate::range::Range;

/// `IHoverPart`.
pub trait IHoverPart {
    /// `owner`: o criador desta parte do hover.
    fn owner(&self) -> Rc<dyn IEditorHoverParticipant>;

    /// `range`: o intervalo ao qual esta parte se aplica.
    fn range(&self) -> Range;

    /// `forceShowAtRange`: força o hover a aparecer sempre neste intervalo, mesmo com várias partes.
    fn force_show_at_range(&self) -> bool {
        false
    }

    /// `isBeforeContent`: o item do hover aparece antes do conteúdo.
    fn is_before_content(&self) -> bool {
        false
    }

    /// `isValidForHoverAnchor(anchor)`: esta parte continua valendo para o novo ancorador?
    fn is_valid_for_hover_anchor(&self, anchor: &HoverAnchor) -> bool;
}

/// `HoverAnchorType`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HoverAnchorType {
    Range = 1,
    ForeignElement = 2,
}

/// `HoverRangeAnchor`.
#[derive(Clone, Debug)]
pub struct HoverRangeAnchor {
    pub priority: i32,
    pub range: Range,
    pub initial_mouse_pos_x: Option<f64>,
    pub initial_mouse_pos_y: Option<f64>,
}

/// `HoverForeignElementAnchor`.
#[derive(Clone)]
pub struct HoverForeignElementAnchor {
    pub priority: i32,
    pub owner: Rc<dyn IEditorHoverParticipant>,
    pub range: Range,
    pub initial_mouse_pos_x: Option<f64>,
    pub initial_mouse_pos_y: Option<f64>,
    pub supports_marker_hover: Option<bool>,
}

/// `HoverAnchor`: `HoverRangeAnchor | HoverForeignElementAnchor`.
#[derive(Clone)]
pub enum HoverAnchor {
    Range(HoverRangeAnchor),
    ForeignElement(HoverForeignElementAnchor),
}

/// A igualdade de identidade de dois participantes (`a === b`): o mesmo objeto.
fn same_participant(a: &Rc<dyn IEditorHoverParticipant>, b: &Rc<dyn IEditorHoverParticipant>) -> bool {
    std::ptr::addr_eq(Rc::as_ptr(a), Rc::as_ptr(b))
}

impl HoverAnchor {
    /// `type`.
    pub fn anchor_type(&self) -> HoverAnchorType {
        match self {
            HoverAnchor::Range(_) => HoverAnchorType::Range,
            HoverAnchor::ForeignElement(_) => HoverAnchorType::ForeignElement,
        }
    }

    pub fn priority(&self) -> i32 {
        match self {
            HoverAnchor::Range(anchor) => anchor.priority,
            HoverAnchor::ForeignElement(anchor) => anchor.priority,
        }
    }

    pub fn range(&self) -> Range {
        match self {
            HoverAnchor::Range(anchor) => anchor.range,
            HoverAnchor::ForeignElement(anchor) => anchor.range,
        }
    }

    pub fn initial_mouse_pos_x(&self) -> Option<f64> {
        match self {
            HoverAnchor::Range(anchor) => anchor.initial_mouse_pos_x,
            HoverAnchor::ForeignElement(anchor) => anchor.initial_mouse_pos_x,
        }
    }

    pub fn initial_mouse_pos_y(&self) -> Option<f64> {
        match self {
            HoverAnchor::Range(anchor) => anchor.initial_mouse_pos_y,
            HoverAnchor::ForeignElement(anchor) => anchor.initial_mouse_pos_y,
        }
    }

    /// `equals(other)`: dois ancoradores de intervalo são iguais quando os intervalos são; dois de elemento
    /// estrangeiro, quando o dono é o mesmo.
    pub fn equals(&self, other: &HoverAnchor) -> bool {
        match (self, other) {
            (HoverAnchor::Range(anchor), HoverAnchor::Range(other)) => anchor.range == other.range,
            (HoverAnchor::ForeignElement(anchor), HoverAnchor::ForeignElement(other)) => same_participant(&anchor.owner, &other.owner),
            _ => false,
        }
    }

    /// `canAdoptVisibleHover(lastAnchor, showAtPosition)`.
    pub fn can_adopt_visible_hover(&self, last_anchor: &HoverAnchor, show_at_position: Position) -> bool {
        match (self, last_anchor) {
            (HoverAnchor::Range(anchor), HoverAnchor::Range(_)) => show_at_position.line_number == anchor.range.start_line_number,
            (HoverAnchor::ForeignElement(anchor), HoverAnchor::ForeignElement(last)) => same_participant(&anchor.owner, &last.owner),
            _ => false,
        }
    }
}

/// `IEditorHoverParticipant` (ver as diferenças no topo do módulo).
pub trait IEditorHoverParticipant {
    /// `hoverOrdinal`.
    fn hover_ordinal(&self) -> i32;

    /// `hideCopyButton`.
    fn hide_copy_button(&self) -> bool {
        false
    }

    /// `suggestHoverAnchor(mouseEvent)`: sem implementação, o `undefined` do original.
    fn suggest_hover_anchor(&self, _mouse_event: &EditorMouseEvent) -> Option<HoverAnchor> {
        None
    }

    /// `getAccessibleContent(hoverPart)`.
    fn get_accessible_content(&self, hover_part: &dyn IHoverPart) -> String;

    /// `handleResize()`.
    fn handle_resize(&self) {}

    /// `handleHide()`.
    fn handle_hide(&self) {}

    /// `handleContentsChanged()`.
    fn handle_contents_changed(&self) {}
}
