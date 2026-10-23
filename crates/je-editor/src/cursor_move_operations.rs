//! Porte de `src/vs/editor/common/cursor/cursorMoveOperations.ts`: as operações de movimento do cursor
//! (esquerda, direita, cima, baixo, início e fim de linha, linhas em branco, início e fim do buffer).
//!
//! Decisões de tradução:
//!
//! - A classe estática `MoveOperations` vira funções livres neste módulo, com o nome em `snake_case`.
//!   Os métodos privados do original (`left`, `leftPositionAtomicSoftTabs`, `clipPositionColumn`,
//!   `clipRange`, `_isBlankLine`) continuam privados.
//! - `ICursorSimpleModel` é o trait [`CursorSimpleModel`], passado como `&dyn`, e `SingleCursorState.move`
//!   é [`SingleCursorState::move_to`].
//! - `normalizationAffinity?: PositionAffinity` de `vertical` é um `Option<PositionAffinity>`, e
//!   `position.delta(undefined, x)` é `delta(0, x)`.
//! - `leftoverVisibleColumns` e as colunas são `i32`; o `linesCount` de `moveDown`/`moveUp` e o `noOfColumns`
//!   também.

use je_base::strings;
use je_base::uint::constants::MAX_SAFE_SMALL_INTEGER;

use crate::cursor_atomic_move_operations::{atomic_position, Direction};
use crate::cursor_columns;
use crate::cursor_common::{CursorConfiguration, CursorSimpleModel, SelectionStartKind, SingleCursorState};
use crate::model::PositionAffinity;
use crate::position::Position;
use crate::range::Range;

/// `CursorPosition`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CursorPosition {
    pub line_number: i32,
    pub column: i32,
    pub leftover_visible_columns: i32,
}

impl CursorPosition {
    pub fn new(line_number: i32, column: i32, leftover_visible_columns: i32) -> Self {
        Self { line_number, column, leftover_visible_columns }
    }
}

/// `MoveOperations.leftPosition`.
pub fn left_position(model: &dyn CursorSimpleModel, position: Position) -> Position {
    if position.column > model.get_line_min_column(position.line_number) {
        let line_content = model.get_line_content(position.line_number);
        let length = strings::prev_char_length(line_content.units(), (position.column - 1) as usize);
        position.delta(0, -(length as i32))
    } else if position.line_number > 1 {
        let new_line_number = position.line_number - 1;
        Position::new(new_line_number, model.get_line_max_column(new_line_number))
    } else {
        position
    }
}

/// `MoveOperations.leftPositionAtomicSoftTabs`.
fn left_position_atomic_soft_tabs(model: &dyn CursorSimpleModel, position: Position, tab_size: i32) -> Position {
    if position.column <= model.get_line_indent_column(position.line_number) {
        let min_column = model.get_line_min_column(position.line_number);
        let line_content = model.get_line_content(position.line_number);
        let new_position = atomic_position(line_content.units(), position.column - 1, tab_size, Direction::Left);
        if new_position != -1 && new_position + 1 >= min_column {
            return Position::new(position.line_number, new_position + 1);
        }
    }
    left_position(model, position)
}

/// `MoveOperations.left`.
fn left(config: &CursorConfiguration, model: &dyn CursorSimpleModel, position: Position) -> CursorPosition {
    let pos = if config.sticky_tab_stops {
        left_position_atomic_soft_tabs(model, position, config.tab_size)
    } else {
        left_position(model, position)
    };
    CursorPosition::new(pos.line_number, pos.column, 0)
}

/// `MoveOperations.moveLeft`.
///
/// `no_of_columns` deve ser `1` ou `Math.round(viewModel.getLineContent(viewLineNumber).length / 2)` (para
/// meias linhas).
pub fn move_left(
    config: &CursorConfiguration,
    model: &dyn CursorSimpleModel,
    cursor: &SingleCursorState,
    in_selection_mode: bool,
    no_of_columns: i32,
) -> SingleCursorState {
    let (line_number, column);

    if cursor.has_selection() && !in_selection_mode {
        // Se o usuário tem uma seleção e não quer estendê-la, o cursor vai para o começo dela.
        line_number = cursor.selection.start_line_number;
        column = cursor.selection.start_column;
    } else {
        // Sem efeito se `no_of_columns == 1`. Não tem problema fazer isso no cenário de meia linha.
        let pos = cursor.position.delta(0, -(no_of_columns - 1));
        // A posição é cortada antes da normalização, que não é definida para colunas possivelmente
        // negativas.
        let normalized_pos = model.normalize_position(clip_position_column(pos, model), PositionAffinity::Left);
        let p = left(config, model, normalized_pos);

        line_number = p.line_number;
        column = p.column;
    }

    cursor.move_to(in_selection_mode, line_number, column, 0)
}

/// `MoveOperations.clipPositionColumn`: ajusta a coluna para ficar entre o mínimo e o máximo da linha.
fn clip_position_column(position: Position, model: &dyn CursorSimpleModel) -> Position {
    Position::new(
        position.line_number,
        clip_range(
            position.column,
            model.get_line_min_column(position.line_number),
            model.get_line_max_column(position.line_number),
        ),
    )
}

/// `MoveOperations.clipRange`.
fn clip_range(value: i32, min: i32, max: i32) -> i32 {
    if value < min {
        return min;
    }
    if value > max {
        return max;
    }
    value
}

/// `MoveOperations.rightPosition`.
pub fn right_position(model: &dyn CursorSimpleModel, line_number: i32, column: i32) -> Position {
    let (mut line_number, mut column) = (line_number, column);
    if column < model.get_line_max_column(line_number) {
        let line_content = model.get_line_content(line_number);
        column += strings::next_char_length(line_content.units(), (column - 1) as usize) as i32;
    } else if line_number < model.get_line_count() {
        line_number += 1;
        column = model.get_line_min_column(line_number);
    }
    model.normalize_position(Position::new(line_number, column), PositionAffinity::Right)
}

/// `MoveOperations.rightPositionAtomicSoftTabs`. O `indentSize` do original não é lido, e fica na
/// assinatura para manter a paridade.
pub fn right_position_atomic_soft_tabs(
    model: &dyn CursorSimpleModel,
    line_number: i32,
    column: i32,
    tab_size: i32,
    _indent_size: i32,
) -> Position {
    if column < model.get_line_indent_column(line_number) {
        let line_content = model.get_line_content(line_number);
        let new_position = atomic_position(line_content.units(), column - 1, tab_size, Direction::Right);
        if new_position != -1 {
            return Position::new(line_number, new_position + 1);
        }
    }
    right_position(model, line_number, column)
}

/// `MoveOperations.right`.
pub fn right(config: &CursorConfiguration, model: &dyn CursorSimpleModel, position: Position) -> CursorPosition {
    let pos = if config.sticky_tab_stops {
        right_position_atomic_soft_tabs(model, position.line_number, position.column, config.tab_size, config.indent_size)
    } else {
        right_position(model, position.line_number, position.column)
    };
    CursorPosition::new(pos.line_number, pos.column, 0)
}

/// `MoveOperations.moveRight`.
pub fn move_right(
    config: &CursorConfiguration,
    model: &dyn CursorSimpleModel,
    cursor: &SingleCursorState,
    in_selection_mode: bool,
    no_of_columns: i32,
) -> SingleCursorState {
    let (line_number, column);

    if cursor.has_selection() && !in_selection_mode {
        // Mover para a direita sem selecionar cancela a seleção e põe o cursor no fim dela.
        line_number = cursor.selection.end_line_number;
        column = cursor.selection.end_column;
    } else {
        let pos = cursor.position.delta(0, no_of_columns - 1);
        let normalized_pos = model.normalize_position(clip_position_column(pos, model), PositionAffinity::Right);
        let r = right(config, model, normalized_pos);
        line_number = r.line_number;
        column = r.column;
    }

    cursor.move_to(in_selection_mode, line_number, column, 0)
}

/// `MoveOperations.vertical`.
#[allow(clippy::too_many_arguments)]
pub fn vertical(
    config: &CursorConfiguration,
    model: &dyn CursorSimpleModel,
    line_number: i32,
    column: i32,
    leftover_visible_columns: i32,
    new_line_number: i32,
    allow_move_on_edge_line: bool,
    normalization_affinity: Option<PositionAffinity>,
) -> CursorPosition {
    let (mut line_number, mut column, mut leftover_visible_columns) = (line_number, column, leftover_visible_columns);

    let current_visible_column = cursor_columns::visible_column_from_column(
        model.get_line_content(line_number).units(),
        column,
        config.tab_size,
    ) + leftover_visible_columns;
    let line_count = model.get_line_count();
    let was_on_first_position = line_number == 1 && column == 1;
    let was_on_last_position = line_number == line_count && column == model.get_line_max_column(line_number);
    let was_at_edge_position = if new_line_number < line_number { was_on_first_position } else { was_on_last_position };

    line_number = new_line_number;
    if line_number < 1 {
        line_number = 1;
        if allow_move_on_edge_line {
            column = model.get_line_min_column(line_number);
        } else {
            column = model.get_line_max_column(line_number).min(column);
        }
    } else if line_number > line_count {
        line_number = line_count;
        if allow_move_on_edge_line {
            column = model.get_line_max_column(line_number);
        } else {
            column = model.get_line_max_column(line_number).min(column);
        }
    } else {
        column = config.column_from_visible_column(model, line_number, current_visible_column);
    }

    if was_at_edge_position {
        leftover_visible_columns = 0;
    } else {
        leftover_visible_columns = current_visible_column
            - cursor_columns::visible_column_from_column(
                model.get_line_content(line_number).units(),
                column,
                config.tab_size,
            );
    }

    if let Some(affinity) = normalization_affinity {
        let position = Position::new(line_number, column);
        let new_position = model.normalize_position(position, affinity);
        leftover_visible_columns += column - new_position.column;
        line_number = new_position.line_number;
        column = new_position.column;
    }
    CursorPosition::new(line_number, column, leftover_visible_columns)
}

/// `MoveOperations.down`.
pub fn down(
    config: &CursorConfiguration,
    model: &dyn CursorSimpleModel,
    line_number: i32,
    column: i32,
    leftover_visible_columns: i32,
    count: i32,
    allow_move_on_last_line: bool,
) -> CursorPosition {
    vertical(
        config,
        model,
        line_number,
        column,
        leftover_visible_columns,
        line_number + count,
        allow_move_on_last_line,
        Some(PositionAffinity::RightOfInjectedText),
    )
}

/// `MoveOperations.moveDown`.
pub fn move_down(
    config: &CursorConfiguration,
    model: &dyn CursorSimpleModel,
    cursor: &SingleCursorState,
    in_selection_mode: bool,
    lines_count: i32,
) -> SingleCursorState {
    let (line_number, column);

    if cursor.has_selection() && !in_selection_mode {
        // Em modo de seleção, mover para baixo age em relação ao fim da seleção.
        line_number = cursor.selection.end_line_number;
        column = cursor.selection.end_column;
    } else {
        line_number = cursor.position.line_number;
        column = cursor.position.column;
    }

    // `do { ... } while (i++ < 10 && lineNumber + i < model.getLineCount())`.
    let mut i = 0;
    let r = loop {
        let r = down(config, model, line_number + i, column, cursor.leftover_visible_columns, lines_count, true);
        let np = model.normalize_position(Position::new(r.line_number, r.column), PositionAffinity::None);
        if np.line_number > line_number {
            break r;
        }
        let previous = i;
        i += 1;
        if !(previous < 10 && line_number + i < model.get_line_count()) {
            break r;
        }
    };

    cursor.move_to(in_selection_mode, r.line_number, r.column, r.leftover_visible_columns)
}

/// O estado de `translateDown` e `translateUp`: a âncora e a posição, cada uma já movida.
fn translated_state(selection_start: CursorPosition, position: CursorPosition) -> SingleCursorState {
    SingleCursorState::new(
        Range::new(selection_start.line_number, selection_start.column, selection_start.line_number, selection_start.column),
        SelectionStartKind::Simple,
        selection_start.leftover_visible_columns,
        Position::new(position.line_number, position.column),
        position.leftover_visible_columns,
    )
}

/// `MoveOperations.translateDown`.
pub fn translate_down(
    config: &CursorConfiguration,
    model: &dyn CursorSimpleModel,
    cursor: &SingleCursorState,
) -> SingleCursorState {
    let selection = cursor.selection;

    let selection_start = down(
        config,
        model,
        selection.selection_start_line_number,
        selection.selection_start_column,
        cursor.selection_start_leftover_visible_columns,
        1,
        false,
    );
    let position = down(
        config,
        model,
        selection.position_line_number,
        selection.position_column,
        cursor.leftover_visible_columns,
        1,
        false,
    );

    translated_state(selection_start, position)
}

/// `MoveOperations.up`.
pub fn up(
    config: &CursorConfiguration,
    model: &dyn CursorSimpleModel,
    line_number: i32,
    column: i32,
    leftover_visible_columns: i32,
    count: i32,
    allow_move_on_first_line: bool,
) -> CursorPosition {
    vertical(
        config,
        model,
        line_number,
        column,
        leftover_visible_columns,
        line_number - count,
        allow_move_on_first_line,
        Some(PositionAffinity::LeftOfInjectedText),
    )
}

/// `MoveOperations.moveUp`.
pub fn move_up(
    config: &CursorConfiguration,
    model: &dyn CursorSimpleModel,
    cursor: &SingleCursorState,
    in_selection_mode: bool,
    lines_count: i32,
) -> SingleCursorState {
    let (line_number, column);

    if cursor.has_selection() && !in_selection_mode {
        // Em modo de seleção, mover para cima age em relação ao começo da seleção.
        line_number = cursor.selection.start_line_number;
        column = cursor.selection.start_column;
    } else {
        line_number = cursor.position.line_number;
        column = cursor.position.column;
    }

    let r = up(config, model, line_number, column, cursor.leftover_visible_columns, lines_count, true);

    cursor.move_to(in_selection_mode, r.line_number, r.column, r.leftover_visible_columns)
}

/// `MoveOperations.translateUp`.
pub fn translate_up(
    config: &CursorConfiguration,
    model: &dyn CursorSimpleModel,
    cursor: &SingleCursorState,
) -> SingleCursorState {
    let selection = cursor.selection;

    let selection_start = up(
        config,
        model,
        selection.selection_start_line_number,
        selection.selection_start_column,
        cursor.selection_start_leftover_visible_columns,
        1,
        false,
    );
    let position = up(
        config,
        model,
        selection.position_line_number,
        selection.position_column,
        cursor.leftover_visible_columns,
        1,
        false,
    );

    translated_state(selection_start, position)
}

/// `MoveOperations._isBlankLine`: vazia ou só com espaço em branco.
fn is_blank_line(model: &dyn CursorSimpleModel, line_number: i32) -> bool {
    model.get_line_first_non_whitespace_column(line_number) == 0
}

/// `MoveOperations.moveToPrevBlankLine`.
pub fn move_to_prev_blank_line(
    _config: &CursorConfiguration,
    model: &dyn CursorSimpleModel,
    cursor: &SingleCursorState,
    in_selection_mode: bool,
) -> SingleCursorState {
    let mut line_number = cursor.position.line_number;

    // Se a linha atual está em branco, vai para a linha anterior que não está.
    while line_number > 1 && is_blank_line(model, line_number) {
        line_number -= 1;
    }

    // Acha a linha em branco anterior.
    while line_number > 1 && !is_blank_line(model, line_number) {
        line_number -= 1;
    }

    cursor.move_to(in_selection_mode, line_number, model.get_line_min_column(line_number), 0)
}

/// `MoveOperations.moveToNextBlankLine`.
pub fn move_to_next_blank_line(
    _config: &CursorConfiguration,
    model: &dyn CursorSimpleModel,
    cursor: &SingleCursorState,
    in_selection_mode: bool,
) -> SingleCursorState {
    let line_count = model.get_line_count();
    let mut line_number = cursor.position.line_number;

    // Se a linha atual está em branco, vai para a próxima linha que não está.
    while line_number < line_count && is_blank_line(model, line_number) {
        line_number += 1;
    }

    // Acha a próxima linha em branco.
    while line_number < line_count && !is_blank_line(model, line_number) {
        line_number += 1;
    }

    cursor.move_to(in_selection_mode, line_number, model.get_line_min_column(line_number), 0)
}

/// `MoveOperations.moveToBeginningOfLine`.
pub fn move_to_beginning_of_line(
    _config: &CursorConfiguration,
    model: &dyn CursorSimpleModel,
    cursor: &SingleCursorState,
    in_selection_mode: bool,
) -> SingleCursorState {
    let line_number = cursor.position.line_number;
    let min_column = model.get_line_min_column(line_number);
    // `getLineFirstNonWhitespaceColumn(lineNumber) || minColumn`: o zero cai no mínimo.
    let first_non_blank_column = match model.get_line_first_non_whitespace_column(line_number) {
        0 => min_column,
        column => column,
    };

    let relevant_column_number = cursor.position.column;
    let column = if relevant_column_number == first_non_blank_column { min_column } else { first_non_blank_column };

    cursor.move_to(in_selection_mode, line_number, column, 0)
}

/// `MoveOperations.moveToEndOfLine`.
pub fn move_to_end_of_line(
    _config: &CursorConfiguration,
    model: &dyn CursorSimpleModel,
    cursor: &SingleCursorState,
    in_selection_mode: bool,
    sticky: bool,
) -> SingleCursorState {
    let line_number = cursor.position.line_number;
    let max_column = model.get_line_max_column(line_number);
    cursor.move_to(in_selection_mode, line_number, max_column, if sticky { MAX_SAFE_SMALL_INTEGER - max_column } else { 0 })
}

/// `MoveOperations.moveToBeginningOfBuffer`.
pub fn move_to_beginning_of_buffer(
    _config: &CursorConfiguration,
    _model: &dyn CursorSimpleModel,
    cursor: &SingleCursorState,
    in_selection_mode: bool,
) -> SingleCursorState {
    cursor.move_to(in_selection_mode, 1, 1, 0)
}

/// `MoveOperations.moveToEndOfBuffer`.
pub fn move_to_end_of_buffer(
    _config: &CursorConfiguration,
    model: &dyn CursorSimpleModel,
    cursor: &SingleCursorState,
    in_selection_mode: bool,
) -> SingleCursorState {
    let last_line_number = model.get_line_count();
    let last_column = model.get_line_max_column(last_line_number);

    cursor.move_to(in_selection_mode, last_line_number, last_column, 0)
}

#[cfg(test)]
mod tests {
    use je_base::JsString;

    use super::*;

    /// Modelo simples de teste: as linhas, com a coluna mínima 1 e sem texto injetado.
    struct TestModel {
        lines: Vec<&'static str>,
    }

    impl CursorSimpleModel for TestModel {
        fn get_line_count(&self) -> i32 {
            self.lines.len() as i32
        }

        fn get_line_content(&self, line_number: i32) -> JsString {
            JsString::from_units(self.lines[line_number as usize - 1].encode_utf16().collect())
        }

        fn get_line_min_column(&self, _line_number: i32) -> i32 {
            1
        }

        fn get_line_max_column(&self, line_number: i32) -> i32 {
            self.get_line_content(line_number).len() as i32 + 1
        }

        fn get_line_first_non_whitespace_column(&self, line_number: i32) -> i32 {
            let content = self.get_line_content(line_number);
            content
                .units()
                .iter()
                .position(|&u| u != u16::from(b' ') && u != u16::from(b'\t'))
                .map_or(0, |index| index as i32 + 1)
        }

        fn get_line_last_non_whitespace_column(&self, line_number: i32) -> i32 {
            let content = self.get_line_content(line_number);
            content
                .units()
                .iter()
                .rposition(|&u| u != u16::from(b' ') && u != u16::from(b'\t'))
                .map_or(0, |index| index as i32 + 2)
        }

        fn normalize_position(&self, position: Position, _affinity: PositionAffinity) -> Position {
            position
        }

        fn get_line_indent_column(&self, line_number: i32) -> i32 {
            match self.get_line_first_non_whitespace_column(line_number) {
                0 => self.get_line_max_column(line_number),
                column => column,
            }
        }
    }

    #[test]
    fn clip_range_clamps_to_the_limits() {
        assert_eq!(clip_range(0, 1, 5), 1);
        assert_eq!(clip_range(9, 1, 5), 5);
        assert_eq!(clip_range(3, 1, 5), 3);
    }

    #[test]
    fn left_position_moves_one_character_or_wraps_to_the_previous_line() {
        let model = TestModel { lines: vec!["abc", "", "x"] };
        assert_eq!(left_position(&model, Position::new(1, 3)), Position::new(1, 2));
        // No começo da linha 2 vai para o fim da linha 1 (coluna máxima 4).
        assert_eq!(left_position(&model, Position::new(2, 1)), Position::new(1, 4));
        // No começo do documento fica onde está.
        assert_eq!(left_position(&model, Position::new(1, 1)), Position::new(1, 1));
    }

    #[test]
    fn left_position_skips_a_whole_surrogate_pair() {
        // `a` e U+1F600 (dois code units): coluna máxima 4.
        let model = TestModel { lines: vec!["a\u{1F600}"] };
        assert_eq!(left_position(&model, Position::new(1, 4)), Position::new(1, 2));
    }

    #[test]
    fn right_position_moves_one_character_or_wraps_to_the_next_line() {
        let model = TestModel { lines: vec!["abc", "", "x"] };
        assert_eq!(right_position(&model, 1, 1), Position::new(1, 2));
        // No fim da linha 1 (coluna 4) vai para o começo da linha 2.
        assert_eq!(right_position(&model, 1, 4), Position::new(2, 1));
        // No fim do documento fica onde está (a linha 3 tem coluna máxima 2).
        assert_eq!(right_position(&model, 3, 2), Position::new(3, 2));
    }

    #[test]
    fn right_position_skips_a_whole_surrogate_pair() {
        let model = TestModel { lines: vec!["a\u{1F600}"] };
        assert_eq!(right_position(&model, 1, 2), Position::new(1, 4));
    }

    #[test]
    fn atomic_soft_tabs_jump_over_a_whole_indent_level() {
        // Quatro espaços de indentação, tabulação 4: a coluna de indentação é 5.
        let model = TestModel { lines: vec!["    x"] };
        assert_eq!(right_position_atomic_soft_tabs(&model, 1, 1, 4, 4), Position::new(1, 5));
        assert_eq!(left_position_atomic_soft_tabs(&model, Position::new(1, 5), 4), Position::new(1, 1));
        // Fora da indentação o movimento é o comum.
        assert_eq!(right_position_atomic_soft_tabs(&model, 1, 5, 4, 4), Position::new(1, 6));
        assert_eq!(left_position_atomic_soft_tabs(&model, Position::new(1, 6), 4), Position::new(1, 5));
    }

    #[test]
    fn blank_line_detection_follows_the_first_non_whitespace_column() {
        let model = TestModel { lines: vec!["a", "", "  \t", " b"] };
        assert!(!is_blank_line(&model, 1));
        assert!(is_blank_line(&model, 2));
        assert!(is_blank_line(&model, 3));
        assert!(!is_blank_line(&model, 4));
    }

    #[test]
    fn cursor_position_keeps_its_fields() {
        let position = CursorPosition::new(3, 4, 5);
        assert_eq!((position.line_number, position.column, position.leftover_visible_columns), (3, 4, 5));
    }
}
