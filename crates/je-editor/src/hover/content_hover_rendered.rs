//! Porte parcial de `src/vs/editor/contrib/hover/browser/contentHoverRendered.ts`: o
//! `RenderedContentHover.computeHoverPositions`, a lógica que decide onde o hover aparece.
//!
//! O restante da classe (o `RenderedContentHover` com as partes renderizadas, o fragmento de DOM, os
//! ouvintes de foco, o botão de cópia por parte e a atualização de verbosidade) monta o widget em DOM, que a UI
//! nativa troca pelos widgets, e fica para o porte deles (fatia H2); por isso o tipo não tem campos. Os tipos de que o
//! construtor depende ([`ContentHoverResult`](crate::hover::content_hover_types::ContentHoverResult),
//! [`EditorHoverStatusBar`](crate::hover::content_hover_status_bar::EditorHoverStatusBar) e
//! [`HoverStartSource`](crate::hover::hover_operation::HoverStartSource)) já existem; falta o
//! `IEditorHoverRenderContext` e o `renderHoverParts` dos participantes.

use crate::code_editor_widget::{CodeEditorWidget, ICodeEditor};
use crate::cursor_common::CursorSimpleModel;
use crate::hover::hover_types::IHoverPart;
use crate::position::Position;
use crate::range::Range;

/// O retorno de `computeHoverPositions`: `{ showAtPosition, showAtSecondaryPosition }`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HoverPositions {
    pub show_at_position: Position,
    pub show_at_secondary_position: Position,
}

/// `RenderedContentHover` (só a parte estática, ver o topo do módulo).
pub struct RenderedContentHover;

impl RenderedContentHover {
    /// `computeHoverPositions(editor, anchorRange, hoverParts)`.
    pub fn compute_hover_positions(editor: &CodeEditorWidget, anchor_range: Range, hover_parts: &[&dyn IHoverPart]) -> HoverPositions {
        let mut start_column_boundary = 1;
        if editor.has_model() {
            // Ensure the range is on the current view line
            let view_model = editor.get_view_model().expect("o editor tem modelo");
            let coordinates_converter = &view_model.coordinates_converter;
            let anchor_view_range = coordinates_converter.convert_model_range_to_view_range(anchor_range, None);
            let anchor_view_min_column = CursorSimpleModel::get_line_min_column(&*view_model, anchor_view_range.start_line_number);
            let anchor_view_range_start = Position::new(anchor_view_range.start_line_number, anchor_view_min_column);
            start_column_boundary = coordinates_converter.convert_view_position_to_model_position(anchor_view_range_start).column;
        }

        // The anchor range is always on a single line
        let anchor_start_line_number = anchor_range.start_line_number;
        let mut secondary_position_column = anchor_range.start_column;
        let mut force_show_at_range: Option<Range> = None;

        for hover_part in hover_parts {
            let hover_part_range = hover_part.range();
            let hover_part_range_on_anchor_start_line = hover_part_range.start_line_number == anchor_start_line_number;
            let hover_part_range_on_anchor_end_line = hover_part_range.end_line_number == anchor_start_line_number;
            let hover_part_range_is_on_anchor_line = hover_part_range_on_anchor_start_line && hover_part_range_on_anchor_end_line;
            if hover_part_range_is_on_anchor_line {
                // this message has a range that is completely sitting on the line of the anchor
                let hover_part_start_column = hover_part_range.start_column;
                let min_secondary_position_column = secondary_position_column.min(hover_part_start_column);
                secondary_position_column = min_secondary_position_column.max(start_column_boundary);
            }
            if hover_part.force_show_at_range() {
                force_show_at_range = Some(hover_part_range);
            }
        }

        let (show_at_position, show_at_secondary_position) = match force_show_at_range {
            Some(range) => {
                let force_show_at_position = range.get_start_position();
                (force_show_at_position, force_show_at_position)
            }
            None => (anchor_range.get_start_position(), Position::new(anchor_start_line_number, secondary_position_column)),
        };
        HoverPositions { show_at_position, show_at_secondary_position }
    }
}
