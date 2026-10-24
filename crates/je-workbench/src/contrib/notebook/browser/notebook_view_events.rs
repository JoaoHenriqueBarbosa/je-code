//! Porte de `src/vs/workbench/contrib/notebook/browser/notebookViewEvents.ts`: os eventos que o editor de notebook troca com
//! os view models (layout, metadados e estado da célula).
//!
//! Divergências:
//! - `NotebookViewEvent` (a união discriminada por `type`) é o enum [`NotebookViewEvent`]; o `type` de cada classe é a
//!   variante, e [`NotebookViewEventType`] existe para quem lê o discriminante.
//! - `fontInfo: FontInfo` é `Rc<FontInfo>` (o `FontInfo` do original é imutável e compartilhado por referência).
//! - `NotebookCellStateChangedEvent.cell` é a `Rc<NotebookCellTextModel>`.

use std::rc::Rc;

use je_editor::config::font_info::FontInfo;

use crate::contrib::notebook::common::model::notebook_cell_text_model::NotebookCellTextModel;
use crate::contrib::notebook::common::notebook_common::NotebookDocumentMetadata;

/// `NotebookLayoutInfo`.
#[derive(Clone, Debug)]
pub struct NotebookLayoutInfo {
    pub width: f64,
    pub height: f64,
    pub scroll_height: f64,
    pub font_info: Rc<FontInfo>,
    pub sticky_height: f64,
    pub list_view_offset_top: f64,
}

/// `CellViewModelStateChangeEvent`: todos os campos são opcionais no original (ausente é falso).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CellViewModelStateChangeEvent {
    pub metadata_changed: bool,
    pub internal_metadata_changed: bool,
    pub selection_changed: bool,
    pub focus_mode_changed: bool,
    pub edit_state_changed: bool,
    pub language_changed: bool,
    pub folding_state_changed: bool,
    pub content_changed: bool,
    pub output_is_hovered_changed: bool,
    pub output_is_focused_changed: bool,
    pub cell_is_hovered_changed: bool,
    pub cell_line_number_changed: bool,
    pub input_collapsed_changed: bool,
    pub output_collapsed_changed: bool,
    pub drag_state_changed: bool,
}

/// `NotebookLayoutChangeEvent`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct NotebookLayoutChangeEvent {
    pub width: bool,
    pub height: bool,
    pub font_info: bool,
}

/// `NotebookViewEventType`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NotebookViewEventType {
    LayoutChanged = 1,
    MetadataChanged = 2,
    CellStateChanged = 3,
}

/// `NotebookLayoutChangedEvent`.
#[derive(Clone, Debug)]
pub struct NotebookLayoutChangedEvent {
    pub source: NotebookLayoutChangeEvent,
    pub value: NotebookLayoutInfo,
}

/// `NotebookMetadataChangedEvent`.
#[derive(Clone, Debug)]
pub struct NotebookMetadataChangedEvent {
    pub source: NotebookDocumentMetadata,
}

/// `NotebookCellStateChangedEvent`.
#[derive(Clone)]
pub struct NotebookCellStateChangedEvent {
    pub source: CellViewModelStateChangeEvent,
    pub cell: Rc<NotebookCellTextModel>,
}

/// `NotebookViewEvent`.
#[derive(Clone)]
pub enum NotebookViewEvent {
    LayoutChanged(NotebookLayoutChangedEvent),
    MetadataChanged(NotebookMetadataChangedEvent),
    CellStateChanged(NotebookCellStateChangedEvent),
}

impl NotebookViewEvent {
    /// O campo `type` do original.
    pub fn type_(&self) -> NotebookViewEventType {
        match self {
            NotebookViewEvent::LayoutChanged(_) => NotebookViewEventType::LayoutChanged,
            NotebookViewEvent::MetadataChanged(_) => NotebookViewEventType::MetadataChanged,
            NotebookViewEvent::CellStateChanged(_) => NotebookViewEventType::CellStateChanged,
        }
    }
}
