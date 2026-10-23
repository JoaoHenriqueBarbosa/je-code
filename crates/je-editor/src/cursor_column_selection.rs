//! Porte de `src/vs/editor/common/cursor/cursorColumnSelection.ts`: a seleção em coluna (retangular).
//!
//! Decisões de tradução:
//!
//! - A classe estática `ColumnSelection` vira funções livres neste módulo.
//! - `IColumnSelectData` é o [`ColumnSelectData`] de `cursor_common.rs` (os campos `toViewVisualColumn` etc.
//!   viram `to_view_visual_column`), e `IColumnSelectResult` é o [`ColumnSelectResult`] daqui.
//! - `config.pageSize` é `f64` no porte, então a aritmética de `columnSelectUp` e `columnSelectDown` é feita em
//!   `f64`, como no `Math.max`/`Math.min` do original, e o resultado volta a `i32` (o valor é sempre um
//!   número de linha).

use crate::cursor_common::{
    ColumnSelectData, CursorConfiguration, CursorSimpleModel, SelectionStartKind, SingleCursorState,
};
use crate::position::Position;
use crate::range::Range;

/// `IColumnSelectResult`.
#[derive(Clone, Debug)]
pub struct ColumnSelectResult {
    pub view_states: Vec<SingleCursorState>,
    pub reversed: bool,
    pub from_line_number: i32,
    pub from_visual_column: i32,
    pub to_line_number: i32,
    pub to_visual_column: i32,
}

/// Um cursor sem seleção estendida: a âncora e a posição na mesma linha, nas colunas dadas.
fn column_cursor(line_number: i32, start_column: i32, end_column: i32) -> SingleCursorState {
    SingleCursorState::new(
        Range::new(line_number, start_column, line_number, start_column),
        SelectionStartKind::Simple,
        0,
        Position::new(line_number, end_column),
        0,
    )
}

/// `ColumnSelection.columnSelect`.
pub fn column_select(
    config: &CursorConfiguration,
    model: &dyn CursorSimpleModel,
    from_line_number: i32,
    from_visible_column: i32,
    to_line_number: i32,
    to_visible_column: i32,
) -> ColumnSelectResult {
    let line_count = (to_line_number - from_line_number).abs() + 1;
    let reversed = from_line_number > to_line_number;
    let is_rtl = from_visible_column > to_visible_column;
    let is_ltr = from_visible_column < to_visible_column;

    let mut result: Vec<SingleCursorState> = Vec::new();

    for i in 0..line_count {
        let line_number = from_line_number + if reversed { -i } else { i };

        let start_column = config.column_from_visible_column(model, line_number, from_visible_column);
        let end_column = config.column_from_visible_column(model, line_number, to_visible_column);
        let visible_start_column = config.visible_column_from_column(model, Position::new(line_number, start_column));
        let visible_end_column = config.visible_column_from_column(model, Position::new(line_number, end_column));

        if is_ltr {
            if visible_start_column > to_visible_column {
                continue;
            }
            if visible_end_column < from_visible_column {
                continue;
            }
        }

        if is_rtl {
            if visible_end_column > from_visible_column {
                continue;
            }
            if visible_start_column < to_visible_column {
                continue;
            }
        }

        result.push(column_cursor(line_number, start_column, end_column));
    }

    if result.is_empty() {
        // Estamos depois de todas as linhas: põe um cursor no fim de cada uma.
        for i in 0..line_count {
            let line_number = from_line_number + if reversed { -i } else { i };
            let max_column = model.get_line_max_column(line_number);

            result.push(column_cursor(line_number, max_column, max_column));
        }
    }

    ColumnSelectResult {
        view_states: result,
        reversed,
        from_line_number,
        from_visual_column: from_visible_column,
        to_line_number,
        to_visual_column: to_visible_column,
    }
}

/// `ColumnSelection.columnSelectLeft`.
pub fn column_select_left(
    config: &CursorConfiguration,
    model: &dyn CursorSimpleModel,
    prev_column_select_data: &ColumnSelectData,
) -> ColumnSelectResult {
    let mut to_view_visual_column = prev_column_select_data.to_view_visual_column;
    if to_view_visual_column > 0 {
        to_view_visual_column -= 1;
    }

    column_select(
        config,
        model,
        prev_column_select_data.from_view_line_number,
        prev_column_select_data.from_view_visual_column,
        prev_column_select_data.to_view_line_number,
        to_view_visual_column,
    )
}

/// `ColumnSelection.columnSelectRight`.
pub fn column_select_right(
    config: &CursorConfiguration,
    model: &dyn CursorSimpleModel,
    prev_column_select_data: &ColumnSelectData,
) -> ColumnSelectResult {
    let mut max_visual_view_column = 0;
    let min_view_line_number = prev_column_select_data.from_view_line_number.min(prev_column_select_data.to_view_line_number);
    let max_view_line_number = prev_column_select_data.from_view_line_number.max(prev_column_select_data.to_view_line_number);
    for line_number in min_view_line_number..=max_view_line_number {
        let line_max_view_column = model.get_line_max_column(line_number);
        let line_max_visual_view_column =
            config.visible_column_from_column(model, Position::new(line_number, line_max_view_column));
        max_visual_view_column = max_visual_view_column.max(line_max_visual_view_column);
    }

    let mut to_view_visual_column = prev_column_select_data.to_view_visual_column;
    if to_view_visual_column < max_visual_view_column {
        to_view_visual_column += 1;
    }

    column_select(
        config,
        model,
        prev_column_select_data.from_view_line_number,
        prev_column_select_data.from_view_visual_column,
        prev_column_select_data.to_view_line_number,
        to_view_visual_column,
    )
}

/// `isPaged ? config.pageSize : 1`.
fn lines_count(config: &CursorConfiguration, is_paged: bool) -> f64 {
    if is_paged { config.page_size } else { 1.0 }
}

/// `ColumnSelection.columnSelectUp`.
pub fn column_select_up(
    config: &CursorConfiguration,
    model: &dyn CursorSimpleModel,
    prev_column_select_data: &ColumnSelectData,
    is_paged: bool,
) -> ColumnSelectResult {
    let lines_count = lines_count(config, is_paged);
    let to_view_line_number = (f64::from(prev_column_select_data.to_view_line_number) - lines_count).max(1.0) as i32;
    column_select(
        config,
        model,
        prev_column_select_data.from_view_line_number,
        prev_column_select_data.from_view_visual_column,
        to_view_line_number,
        prev_column_select_data.to_view_visual_column,
    )
}

/// `ColumnSelection.columnSelectDown`.
pub fn column_select_down(
    config: &CursorConfiguration,
    model: &dyn CursorSimpleModel,
    prev_column_select_data: &ColumnSelectData,
    is_paged: bool,
) -> ColumnSelectResult {
    let lines_count = lines_count(config, is_paged);
    let to_view_line_number = f64::from(model.get_line_count())
        .min(f64::from(prev_column_select_data.to_view_line_number) + lines_count) as i32;
    column_select(
        config,
        model,
        prev_column_select_data.from_view_line_number,
        prev_column_select_data.from_view_visual_column,
        to_view_line_number,
        prev_column_select_data.to_view_visual_column,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn column_cursor_anchors_and_positions_on_the_same_line() {
        let state = column_cursor(3, 2, 5);
        assert_eq!(state.selection_start, Range::new(3, 2, 3, 2));
        assert_eq!(state.position, Position::new(3, 5));
        assert_eq!(state.selection_start_kind, SelectionStartKind::Simple);
        assert_eq!(state.selection_start_leftover_visible_columns, 0);
        assert_eq!(state.leftover_visible_columns, 0);
        assert!(state.has_selection());
    }

    #[test]
    fn column_cursor_with_equal_columns_has_no_selection() {
        assert!(!column_cursor(1, 4, 4).has_selection());
    }
}
