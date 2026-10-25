//! Porte de `src/vs/editor/contrib/hover/browser/hoverTypes.ts`, a parte que não depende de DOM: `IHoverPart`,
//! os ancoradores (`HoverAnchorType`, `HoverRangeAnchor`, `HoverForeignElementAnchor`, `HoverAnchor`) e o
//! contrato do participante do hover.
//!
//! Diferenças do original:
//! - `IEditorHoverParticipant` traz `hoverOrdinal`, `hideCopyButton`, `suggestHoverAnchor`, `computeSync`, `computeAsync`,
//!   `createLoadingMessage`, `getAccessibleContent`, `handleResize`, `handleHide`, `handleContentsChanged` e
//!   `handleScroll`. Fica de fora `renderHoverParts` (monta DOM, que a UI nativa troca pelos widgets, no porte do
//!   `ContentHoverWidget`). O `T extends IHoverPart` genérico dos métodos vira o `Rc<dyn IHoverPart>` (nos retornos) e o
//!   `&dyn IHoverPart` (nos argumentos).
//! - Os membros opcionais (`?`) viram métodos com o corpo padrão do "não definido": `computeAsync` devolve `None` (o
//!   método ausente, que o `ContentHoverComputer` troca pelo `AsyncIterableProducer.EMPTY`), `createLoadingMessage` devolve
//!   `None` (o `null`) e `handleScroll` não faz nada. `computeAsync` devolve o `AsyncIterable<T>` do original como
//!   `Rc<dyn AsyncIterable<Rc<dyn IHoverPart>>>`.
//! - `owner` é o `Rc<dyn IEditorHoverParticipant>`; a igualdade de dono (`this.owner === other.owner`) compara o
//!   endereço do objeto.
//! - `IEditorHoverStatusBar` e `IEditorHoverAction` são os traits deste módulo: o `HTMLElement` do `append` e do `run` é o
//!   [`NodeId`] da árvore do `je-ui`, e as opções de `addAction` são o [`IHoverAction`] do `je-widgets`, que tem os mesmos
//!   campos (`label`, `iconClass`, `run`, `commandId`).
//! - `IEditorHoverColorPickerWidget`, `IEditorHoverContext`, `IEditorHoverRenderContext`, `IRenderedHoverPart(s)`,
//!   `RenderedHoverParts`, `HoverParticipantRegistry` e `IHoverWidget` descrevem a montagem do widget em DOM e ficam para
//!   o porte dele.
//! - `initialMousePosX` e `initialMousePosY` (`number | undefined`) são `Option<f64>`.

use std::rc::Rc;

use je_base::async_::AsyncIterable;
use je_base::cancellation::CancellationToken;
use je_base::scrollable::ScrollEvent;
use je_ui::NodeId;
use je_widgets::hover_api::IHoverAction;
use je_widgets::hover_widget::HoverAction;

use crate::editor_browser::EditorMouseEvent;
use crate::hover::hover_operation::HoverStartSource;
use crate::model::ModelDecoration;
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

/// `IEditorHoverAction`.
pub trait IEditorHoverAction {
    /// `setEnabled(enabled)`.
    fn set_enabled(&self, enabled: bool);
}

impl IEditorHoverAction for HoverAction {
    fn set_enabled(&self, enabled: bool) {
        HoverAction::set_enabled(self, enabled);
    }
}

/// `IEditorHoverStatusBar`.
pub trait IEditorHoverStatusBar {
    /// `addAction(actionOptions)`.
    fn add_action(&self, action_options: IHoverAction) -> Rc<dyn IEditorHoverAction>;

    /// `append(element)`: devolve o próprio elemento.
    fn append(&self, element: NodeId) -> NodeId;
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

    /// `computeSync(anchor, lineDecorations, source)`.
    fn compute_sync(&self, anchor: &HoverAnchor, line_decorations: &[ModelDecoration], source: HoverStartSource) -> Vec<Rc<dyn IHoverPart>>;

    /// `computeAsync?(anchor, lineDecorations, source, token)`: sem implementação, o método ausente.
    fn compute_async(
        &self,
        _anchor: &HoverAnchor,
        _line_decorations: &[ModelDecoration],
        _source: HoverStartSource,
        _token: &CancellationToken,
    ) -> Option<Rc<dyn AsyncIterable<Rc<dyn IHoverPart>>>> {
        None
    }

    /// `createLoadingMessage?(anchor)`: sem implementação, o `null` do original.
    fn create_loading_message(&self, _anchor: &HoverAnchor) -> Option<Rc<dyn IHoverPart>> {
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

    /// `handleScroll(e)`.
    fn handle_scroll(&self, _e: &ScrollEvent) {}
}
