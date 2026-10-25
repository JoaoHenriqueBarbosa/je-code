//! Porte parcial de `src/vs/editor/browser/editorBrowser.ts`: o `MouseTargetType`, as interfaces de alvo do
//! mouse (`IBaseMouseTarget` e derivadas, o union `IMouseTarget`) e `IEditorMouseEvent` /
//! `IPartialEditorMouseEvent`, e os widgets de conteúdo e de overlay (`IContentWidget`, `IOverlayWidget` e as
//! posições). O resto do arquivo (`IEditor`, `ICodeEditor`, `IDiffEditor`, widgets de margem de glifos,
//! `IPasteEvent` e afins) não está aqui.
//!
//! Divergências:
//! - O union `IMouseTarget` vira o enum `MouseTarget`, com um struct de dados por variante. Os campos que a
//!   interface estreita para `null` (`position`/`range` do textarea, dos widgets) ou para não nulo (margem,
//!   view zone, texto, vazio, scrollbar) são tipados assim em cada struct; as variantes `IMouseTargetMargin`
//!   e `IMouseTargetViewZone`, que agrupam vários `MouseTargetType`, guardam o tipo no campo `target_type`
//!   (os métodos `new` conferem que ele é um dos tipos permitidos). Os campos comuns de `IBaseMouseTarget`
//!   são lidos por `MouseTarget::element`, `position`, `mouse_column` e `range`.
//! - `element: HTMLElement | null` vira `Option<ElementId>`, identificador opaco: não há DOM.
//! - `IMouseEvent` (`vs/base/browser/mouseEvent.ts`) vira `MouseEvent`, com os campos que o código e os testes
//!   leem (`leftButton`, `middleButton`, `rightButton`, `buttons`, `target`, `detail`, `posx`, `posy`, os
//!   quatro modificadores, `timestamp`, `defaultPrevented`). `browserEvent` (o `MouseEvent` do navegador)
//!   não existe; `preventDefault` e `stopPropagation` ficam para quando o despacho de eventos for portado.
//! - `je-editor` não depende de `je-ui`, então o evento é definido aqui; quando o `je-ui` trouxer o evento
//!   de mouse de base, este tipo passa a ser reexportado de lá.
//! - `InjectedText` é o de `model_line_projection_data` e `GlyphMarginLane` o de `model`.
//! - Os widgets de conteúdo e de overlay não têm `getDomNode()`: a UI é uma árvore nativa, e o dono do widget o
//!   desenha. As propriedades opcionais da interface são métodos com valor padrão.

use je_base::event::Event;

use crate::core_2d::dimension::Dimension;
use crate::model::{GlyphMarginLane, PositionAffinity};
use crate::model_line_projection_data::InjectedText;
use crate::position::Position;
use crate::range::Range;

/// Identificador opaco de um elemento (`HTMLElement`) da árvore de widgets.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct ElementId(pub u64);

/// Type of hit element with the mouse in the editor (`MouseTargetType`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum MouseTargetType {
    /// Mouse is on top of an unknown element.
    #[default]
    Unknown = 0,
    /// Mouse is on top of the textarea used for input.
    Textarea = 1,
    /// Mouse is on top of the glyph margin.
    GutterGlyphMargin = 2,
    /// Mouse is on top of the line numbers.
    GutterLineNumbers = 3,
    /// Mouse is on top of the line decorations.
    GutterLineDecorations = 4,
    /// Mouse is on top of the whitespace left in the gutter by a view zone.
    GutterViewZone = 5,
    /// Mouse is on top of text in the content.
    ContentText = 6,
    /// Mouse is on top of empty space in the content (e.g. after line text or below last line).
    ContentEmpty = 7,
    /// Mouse is on top of a view zone in the content.
    ContentViewZone = 8,
    /// Mouse is on top of a content widget.
    ContentWidget = 9,
    /// Mouse is on top of the decorations overview ruler.
    OverviewRuler = 10,
    /// Mouse is on top of a scrollbar.
    Scrollbar = 11,
    /// Mouse is on top of an overlay widget.
    OverlayWidget = 12,
    /// Mouse is outside of the editor.
    OutsideEditor = 13,
}

/// `IMouseTargetUnknown`: os campos de `IBaseMouseTarget` sem estreitamento.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MouseTargetUnknown {
    /// The target element.
    pub element: Option<ElementId>,
    /// The 'approximate' editor position.
    pub position: Option<Position>,
    /// Desired mouse column (e.g. when position.column gets clamped to text length, clicking after text on a line).
    pub mouse_column: i32,
    /// The 'approximate' editor range.
    pub range: Option<Range>,
}

/// `IMouseTargetTextarea`: `position` e `range` são sempre `null`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MouseTargetTextarea {
    pub element: Option<ElementId>,
    pub mouse_column: i32,
}

/// `IMouseTargetMarginData`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MouseTargetMarginData {
    pub is_after_lines: bool,
    pub glyph_margin_left: f64,
    pub glyph_margin_width: f64,
    pub glyph_margin_lane: Option<GlyphMarginLane>,
    pub line_numbers_width: f64,
    pub offset_x: f64,
}

/// `IMouseTargetMargin`: `target_type` é `GutterGlyphMargin`, `GutterLineNumbers` ou `GutterLineDecorations`.
#[derive(Clone, Debug, PartialEq)]
pub struct MouseTargetMargin {
    pub target_type: MouseTargetType,
    pub element: Option<ElementId>,
    pub position: Position,
    pub mouse_column: i32,
    pub range: Range,
    pub detail: MouseTargetMarginData,
}

impl MouseTargetMargin {
    /// Falha se `target_type` não for um dos três tipos de margem do `IMouseTargetMargin`.
    pub fn new(
        target_type: MouseTargetType,
        element: Option<ElementId>,
        position: Position,
        mouse_column: i32,
        range: Range,
        detail: MouseTargetMarginData,
    ) -> Self {
        assert!(
            matches!(
                target_type,
                MouseTargetType::GutterGlyphMargin
                    | MouseTargetType::GutterLineNumbers
                    | MouseTargetType::GutterLineDecorations
            ),
            "tipo inválido para IMouseTargetMargin: {target_type:?}"
        );
        Self { target_type, element, position, mouse_column, range, detail }
    }
}

/// `IMouseTargetViewZoneData`.
#[derive(Clone, Debug, PartialEq)]
pub struct MouseTargetViewZoneData {
    pub view_zone_id: String,
    pub position_before: Option<Position>,
    pub position_after: Option<Position>,
    pub position: Position,
    pub after_line_number: i32,
}

/// `IMouseTargetViewZone`: `target_type` é `GutterViewZone` ou `ContentViewZone`.
#[derive(Clone, Debug, PartialEq)]
pub struct MouseTargetViewZone {
    pub target_type: MouseTargetType,
    pub element: Option<ElementId>,
    pub position: Position,
    pub mouse_column: i32,
    pub range: Range,
    pub detail: MouseTargetViewZoneData,
}

impl MouseTargetViewZone {
    /// Falha se `target_type` não for `GutterViewZone` nem `ContentViewZone`.
    pub fn new(
        target_type: MouseTargetType,
        element: Option<ElementId>,
        position: Position,
        mouse_column: i32,
        range: Range,
        detail: MouseTargetViewZoneData,
    ) -> Self {
        assert!(
            matches!(target_type, MouseTargetType::GutterViewZone | MouseTargetType::ContentViewZone),
            "tipo inválido para IMouseTargetViewZone: {target_type:?}"
        );
        Self { target_type, element, position, mouse_column, range, detail }
    }
}

/// `IMouseTargetContentTextData`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MouseTargetContentTextData {
    pub might_be_foreign_element: bool,
    /// `@internal` no original.
    pub injected_text: Option<InjectedText>,
}

/// `IMouseTargetContentText`.
#[derive(Clone, Debug, PartialEq)]
pub struct MouseTargetContentText {
    pub element: Option<ElementId>,
    pub position: Position,
    pub mouse_column: i32,
    pub range: Range,
    pub detail: MouseTargetContentTextData,
}

/// `IMouseTargetContentEmptyData`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MouseTargetContentEmptyData {
    pub is_after_lines: bool,
    pub horizontal_distance_to_text: Option<f64>,
}

/// `IMouseTargetContentEmpty`.
#[derive(Clone, Debug, PartialEq)]
pub struct MouseTargetContentEmpty {
    pub element: Option<ElementId>,
    pub position: Position,
    pub mouse_column: i32,
    pub range: Range,
    pub detail: MouseTargetContentEmptyData,
}

/// `IMouseTargetContentWidget`: `position` e `range` são sempre `null`; `detail` é o id do widget.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MouseTargetContentWidget {
    pub element: Option<ElementId>,
    pub mouse_column: i32,
    pub detail: String,
}

/// `IMouseTargetOverlayWidget`: `position` e `range` são sempre `null`; `detail` é o id do widget.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MouseTargetOverlayWidget {
    pub element: Option<ElementId>,
    pub mouse_column: i32,
    pub detail: String,
}

/// `IMouseTargetScrollbar`.
#[derive(Clone, Debug, PartialEq)]
pub struct MouseTargetScrollbar {
    pub element: Option<ElementId>,
    pub position: Position,
    pub mouse_column: i32,
    pub range: Range,
}

/// `IMouseTargetOverviewRuler`: só os campos de `IBaseMouseTarget`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MouseTargetOverviewRuler {
    pub element: Option<ElementId>,
    pub position: Option<Position>,
    pub mouse_column: i32,
    pub range: Option<Range>,
}

/// `'above' | 'below' | 'left' | 'right'`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum OutsidePosition {
    Above,
    Below,
    Left,
    Right,
}

/// `IMouseTargetOutsideEditor`.
#[derive(Clone, Debug, PartialEq)]
pub struct MouseTargetOutsideEditor {
    pub element: Option<ElementId>,
    pub position: Option<Position>,
    pub mouse_column: i32,
    pub range: Option<Range>,
    pub outside_position: OutsidePosition,
    pub outside_distance: f64,
}

/// Target hit with the mouse in the editor (`IMouseTarget`).
#[derive(Clone, Debug, PartialEq)]
pub enum MouseTarget {
    Unknown(MouseTargetUnknown),
    Textarea(MouseTargetTextarea),
    Margin(MouseTargetMargin),
    ViewZone(MouseTargetViewZone),
    ContentText(MouseTargetContentText),
    ContentEmpty(MouseTargetContentEmpty),
    ContentWidget(MouseTargetContentWidget),
    OverlayWidget(MouseTargetOverlayWidget),
    Scrollbar(MouseTargetScrollbar),
    OverviewRuler(MouseTargetOverviewRuler),
    OutsideEditor(MouseTargetOutsideEditor),
}

impl Default for MouseTarget {
    fn default() -> Self {
        MouseTarget::Unknown(MouseTargetUnknown::default())
    }
}

impl MouseTarget {
    /// O campo `type` do alvo.
    pub fn target_type(&self) -> MouseTargetType {
        match self {
            MouseTarget::Unknown(_) => MouseTargetType::Unknown,
            MouseTarget::Textarea(_) => MouseTargetType::Textarea,
            MouseTarget::Margin(target) => target.target_type,
            MouseTarget::ViewZone(target) => target.target_type,
            MouseTarget::ContentText(_) => MouseTargetType::ContentText,
            MouseTarget::ContentEmpty(_) => MouseTargetType::ContentEmpty,
            MouseTarget::ContentWidget(_) => MouseTargetType::ContentWidget,
            MouseTarget::OverlayWidget(_) => MouseTargetType::OverlayWidget,
            MouseTarget::Scrollbar(_) => MouseTargetType::Scrollbar,
            MouseTarget::OverviewRuler(_) => MouseTargetType::OverviewRuler,
            MouseTarget::OutsideEditor(_) => MouseTargetType::OutsideEditor,
        }
    }

    /// `IBaseMouseTarget.element`.
    pub fn element(&self) -> Option<ElementId> {
        match self {
            MouseTarget::Unknown(t) => t.element,
            MouseTarget::Textarea(t) => t.element,
            MouseTarget::Margin(t) => t.element,
            MouseTarget::ViewZone(t) => t.element,
            MouseTarget::ContentText(t) => t.element,
            MouseTarget::ContentEmpty(t) => t.element,
            MouseTarget::ContentWidget(t) => t.element,
            MouseTarget::OverlayWidget(t) => t.element,
            MouseTarget::Scrollbar(t) => t.element,
            MouseTarget::OverviewRuler(t) => t.element,
            MouseTarget::OutsideEditor(t) => t.element,
        }
    }

    /// `IBaseMouseTarget.position`.
    pub fn position(&self) -> Option<Position> {
        match self {
            MouseTarget::Unknown(t) => t.position,
            MouseTarget::Textarea(_) | MouseTarget::ContentWidget(_) | MouseTarget::OverlayWidget(_) => None,
            MouseTarget::Margin(t) => Some(t.position),
            MouseTarget::ViewZone(t) => Some(t.position),
            MouseTarget::ContentText(t) => Some(t.position),
            MouseTarget::ContentEmpty(t) => Some(t.position),
            MouseTarget::Scrollbar(t) => Some(t.position),
            MouseTarget::OverviewRuler(t) => t.position,
            MouseTarget::OutsideEditor(t) => t.position,
        }
    }

    /// `IBaseMouseTarget.mouseColumn`.
    pub fn mouse_column(&self) -> i32 {
        match self {
            MouseTarget::Unknown(t) => t.mouse_column,
            MouseTarget::Textarea(t) => t.mouse_column,
            MouseTarget::Margin(t) => t.mouse_column,
            MouseTarget::ViewZone(t) => t.mouse_column,
            MouseTarget::ContentText(t) => t.mouse_column,
            MouseTarget::ContentEmpty(t) => t.mouse_column,
            MouseTarget::ContentWidget(t) => t.mouse_column,
            MouseTarget::OverlayWidget(t) => t.mouse_column,
            MouseTarget::Scrollbar(t) => t.mouse_column,
            MouseTarget::OverviewRuler(t) => t.mouse_column,
            MouseTarget::OutsideEditor(t) => t.mouse_column,
        }
    }

    /// `IBaseMouseTarget.range`.
    pub fn range(&self) -> Option<Range> {
        match self {
            MouseTarget::Unknown(t) => t.range,
            MouseTarget::Textarea(_) | MouseTarget::ContentWidget(_) | MouseTarget::OverlayWidget(_) => None,
            MouseTarget::Margin(t) => Some(t.range),
            MouseTarget::ViewZone(t) => Some(t.range),
            MouseTarget::ContentText(t) => Some(t.range),
            MouseTarget::ContentEmpty(t) => Some(t.range),
            MouseTarget::Scrollbar(t) => Some(t.range),
            MouseTarget::OverviewRuler(t) => t.range,
            MouseTarget::OutsideEditor(t) => t.range,
        }
    }
}

/// `IMouseEvent` de `vs/base/browser/mouseEvent.ts`, sem o `browserEvent`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MouseEvent {
    pub left_button: bool,
    pub middle_button: bool,
    pub right_button: bool,
    pub buttons: u32,
    pub target: ElementId,
    pub detail: i32,
    pub posx: f64,
    pub posy: f64,
    pub ctrl_key: bool,
    pub shift_key: bool,
    pub alt_key: bool,
    pub meta_key: bool,
    pub timestamp: f64,
    pub default_prevented: bool,
}

/// A mouse event originating from the editor (`IEditorMouseEvent`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct EditorMouseEvent {
    pub event: MouseEvent,
    pub target: MouseTarget,
}

/// `IPartialEditorMouseEvent`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PartialEditorMouseEvent {
    pub event: MouseEvent,
    pub target: Option<MouseTarget>,
}

// #region Widgets de conteúdo e de overlay

/// `ContentWidgetPositionPreference`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ContentWidgetPositionPreference {
    /// Place the content widget exactly at a position.
    Exact,
    /// Place the content widget above a position.
    Above,
    /// Place the content widget below a position.
    Below,
}

/// `IContentWidgetPosition`: a posição de um widget de conteúdo.
#[derive(Clone, Debug, PartialEq)]
pub struct ContentWidgetPosition {
    /// Desired position which serves as an anchor for placing the content widget.
    pub position: Option<Position>,
    /// A secondary position with the same line number as the primary one.
    pub secondary_position: Option<Position>,
    /// Placement preference for position, in order of preference.
    pub preference: Vec<ContentWidgetPositionPreference>,
    /// Placement preference when multiple view positions refer to the same (model) position.
    pub position_affinity: Option<PositionAffinity>,
}

/// `IContentWidgetRenderedCoordinate`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ContentWidgetRenderedCoordinate {
    /// Top position relative to the editor content.
    pub top: f64,
    /// Left position relative to the editor content.
    pub left: f64,
}

/// `IContentWidget`: renders inline with the text and can be placed near an editor position. Sem DOM, o
/// `getDomNode()` do original não existe: quem desenha o widget é o dono dele, a partir da posição que a
/// `View` lê por [`IContentWidget::get_position`].
pub trait IContentWidget {
    /// `getId()`.
    fn get_id(&self) -> String;

    /// `getPosition()`: `None` põe o widget fora da tela.
    fn get_position(&self) -> Option<ContentWidgetPosition>;

    /// `allowEditorOverflow`.
    fn allow_editor_overflow(&self) -> bool {
        false
    }

    /// `useDisplayNone`: o widget não tem representação visual.
    fn use_display_none(&self) -> bool {
        false
    }

    /// `suppressMouseDown`.
    fn suppress_mouse_down(&self) -> bool {
        false
    }

    /// `beforeRender?()`.
    fn before_render(&self) -> Option<Dimension> {
        None
    }

    /// `afterRender?(position, coordinate)`.
    fn after_render(&self, _position: Option<ContentWidgetPositionPreference>, _coordinate: Option<ContentWidgetRenderedCoordinate>) {}
}

/// `OverlayWidgetPositionPreference`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OverlayWidgetPositionPreference {
    /// Position the overlay widget in the top right corner.
    TopRightCorner,
    /// Position the overlay widget in the bottom right corner.
    BottomRightCorner,
    /// Position the overlay widget in the top center.
    TopCenter,
}

/// `IOverlayWidgetPositionCoordinates`: coordenadas relativas ao editor.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OverlayWidgetPositionCoordinates {
    /// The top position for the overlay widget, relative to the editor.
    pub top: f64,
    /// The left position for the overlay widget, relative to the editor.
    pub left: f64,
}

/// `OverlayWidgetPositionPreference | IOverlayWidgetPositionCoordinates` do campo `preference`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum OverlayWidgetPlacement {
    Preference(OverlayWidgetPositionPreference),
    Coordinates(OverlayWidgetPositionCoordinates),
}

/// `IOverlayWidgetPosition`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OverlayWidgetPosition {
    /// The position preference for the overlay widget (`null` é `None`).
    pub preference: Option<OverlayWidgetPlacement>,
    /// When set, stacks with other overlay widgets with the same preference.
    pub stack_ordinal: Option<f64>,
}

/// `IOverlayWidget`: renders on top of the text. Sem DOM, o `getDomNode()` do original não existe (ver
/// [`IContentWidget`]).
pub trait IOverlayWidget {
    /// `getId()`.
    fn get_id(&self) -> String;

    /// `getPosition()`: `None` deixa o widget se posicionar sozinho.
    fn get_position(&self) -> Option<OverlayWidgetPosition>;

    /// `onDidLayout?`: evento disparado quando o layout do widget muda.
    fn on_did_layout(&self) -> Option<Event<()>> {
        None
    }

    /// `allowEditorOverflow`.
    fn allow_editor_overflow(&self) -> bool {
        false
    }

    /// `getMinContentWidthInPx?()`: o editor garante que a largura de rolagem seja pelo menos esse valor.
    fn get_min_content_width_in_px(&self) -> Option<f64> {
        None
    }
}

// #endregion
