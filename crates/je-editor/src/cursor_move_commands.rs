//! Porte de `src/vs/editor/common/cursor/cursorMoveCommands.ts`: os comandos de movimento do cursor
//! (adicionar cursor acima e abaixo, início e fim de linha e de buffer, selecionar tudo, seleção por linha e
//! por palavra, cancelar seleção, `moveTo`, movimentos simples e de viewport) e o namespace `CursorMove`, com
//! as direções, as unidades e o `parse` dos argumentos do comando `cursorMove`.
//!
//! Decisões de tradução:
//!
//! - A classe estática `CursorMoveCommands` vira funções livres neste módulo, em `snake_case`; os métodos
//!   privados do original continuam privados. `IViewModel` tem tipos associados, então as funções são
//!   genéricas (`V: IViewModel`) e recebem `&V`; o modelo de visão vira `&dyn CursorSimpleModel` onde as
//!   operações de `cursor_move_operations` esperam o modelo simples, e `viewModel.model` (o `ITextModel`) é o
//!   `Rc<TextModel>` lido por [`IViewModel::model`].
//! - `viewModel.cursorConfig` é lido uma vez por chamada ([`IViewModel::cursor_config`] devolve a configuração
//!   por valor).
//! - `PartialCursorState` é o enum de `cursor_common`: `new CursorState(...)` vira
//!   [`PartialCursorState::Full`], `CursorState.fromModelState` vira [`PartialCursorState::Model`] e
//!   `CursorState.fromViewState` vira [`PartialCursorState::View`]. `SingleCursorState.move` é
//!   [`SingleCursorState::move_to`].
//! - `IViewModel` não traz `getLineLength`: o comprimento de uma linha é o de `getLineContent` em unidades
//!   UTF-16, que é o que o original mede. `Math.round(x / 2)` sobre um inteiro `x` não negativo é
//!   `(x + 1) / 2`, e o mesmo vale para o centro da linha e do viewport (`div_euclid(x + 1, 2)`).
//! - `IPosition` é [`Position`]; `IPosition | undefined` é `Option<Position>`.
//! - `SimpleMoveDirection` e `ViewportDirection` são sinônimos de [`cursor_move::Direction`]: o TypeScript só
//!   estreita o tipo da união, e as duas funções devolvem `None` (o `null` do original) para a direção que
//!   não é delas, como o `default` do `switch`.
//! - `CursorMove.RawArguments.value` é `number` e `ParsedArguments.value` também; aqui o valor cru é `f64` e o
//!   valor já interpretado é `i32` (truncado por `as`, com saturação), que é o que as operações de movimento
//!   consomem como contagem de colunas e de linhas.
//! - `ICommandMetadata` (`platform/commands/common/commands.ts`) vira a struct de dados [`CommandMetadata`]
//!   deste arquivo, com o texto do original. O argumento do comando é `unknown` no TypeScript (a restrição
//!   `isCursorMoveArgs` e o `schema` JSON olham um objeto qualquer), então ele vira o valor dinâmico
//!   [`ArgumentValue`]. `types.isObject`, `types.isString`, `types.isBoolean`, `types.isNumber` e
//!   `types.isUndefined` são os métodos `is_*` dele.

use crate::cursor_common::{
    CursorConfiguration, CursorSimpleModel, CursorState, PartialCursorState, SelectionStartKind, SingleCursorState,
};
use crate::cursor_move_operations as move_operations;
use crate::cursor_word_operations::WordOperations;
use crate::model::TextDirection;
use crate::position::Position;
use crate::range::Range;
use crate::view_model_types::IViewModel;

use self::cursor_move::{Direction, Unit};

/// A forma das operações de movimento de `cursor_move_operations` que só dependem do modo de seleção.
type MoveFn = fn(&CursorConfiguration, &dyn CursorSimpleModel, &SingleCursorState, bool) -> SingleCursorState;

/// A forma de `moveUp` e `moveDown`: o modo de seleção e a contagem de linhas.
type MoveLinesFn = fn(&CursorConfiguration, &dyn CursorSimpleModel, &SingleCursorState, bool, i32) -> SingleCursorState;

/// A forma de `translateUp` e `translateDown`.
type TranslateFn = fn(&CursorConfiguration, &dyn CursorSimpleModel, &SingleCursorState) -> SingleCursorState;

/// `CursorState.fromModelState`, já como [`PartialCursorState`].
fn from_model(model_state: SingleCursorState) -> PartialCursorState {
    PartialCursorState::Model(CursorState::from_model_state(model_state))
}

/// `CursorState.fromViewState`, já como [`PartialCursorState`].
fn from_view(view_state: SingleCursorState) -> PartialCursorState {
    PartialCursorState::View(CursorState::from_view_state(view_state))
}

/// `new CursorState(cursor.modelState, cursor.viewState)`: o cursor como está.
fn unchanged(cursor: &CursorState) -> PartialCursorState {
    PartialCursorState::Full(CursorState::new(cursor.model_state, cursor.view_state))
}

/// Move cada cursor pelo estado da visão.
fn map_view(cursors: &[CursorState], f: impl Fn(&SingleCursorState) -> SingleCursorState) -> Vec<PartialCursorState> {
    cursors.iter().map(|cursor| from_view(f(&cursor.view_state))).collect()
}

/// Move cada cursor pelo estado do modelo.
fn map_model(cursors: &[CursorState], f: impl Fn(&SingleCursorState) -> SingleCursorState) -> Vec<PartialCursorState> {
    cursors.iter().map(|cursor| from_model(f(&cursor.model_state))).collect()
}

/// Aplica `op` a cada cursor, no estado da visão (`in_view`) ou no do modelo.
fn map_with<V: IViewModel>(
    view_model: &V,
    cursors: &[CursorState],
    in_selection_mode: bool,
    in_view: bool,
    op: MoveFn,
) -> Vec<PartialCursorState> {
    let config = view_model.cursor_config();
    if in_view {
        map_view(cursors, |state| op(&config, view_model, state, in_selection_mode))
    } else {
        let model = view_model.model();
        map_model(cursors, |state| op(&config, &*model, state, in_selection_mode))
    }
}

/// A posição na visão de `_position`/`_viewPosition`, como `line` e `moveTo` a calculam.
fn resolve_view_position<V: IViewModel>(view_model: &V, position: Position, view_position: Option<Position>) -> Position {
    let converter = view_model.coordinates_converter();
    match view_position {
        Some(view_position) => {
            converter.validate_view_position(Position::new(view_position.line_number, view_position.column), position)
        }
        None => converter.convert_model_position_to_view_position(position, None, None, None),
    }
}

/// `CursorMoveCommands.addCursorDown` e `addCursorUp`: cada cursor continua e ganha um vizinho transladado.
fn add_cursor_translated<V: IViewModel>(
    view_model: &V,
    cursors: &[CursorState],
    use_logical_line: bool,
    translate: TranslateFn,
) -> Vec<PartialCursorState> {
    let config = view_model.cursor_config();
    let model = view_model.model();
    let mut result = Vec::with_capacity(cursors.len() * 2);
    for cursor in cursors {
        result.push(unchanged(cursor));
        if use_logical_line {
            result.push(from_model(translate(&config, &*model, &cursor.model_state)));
        } else {
            result.push(from_view(translate(&config, view_model, &cursor.view_state)));
        }
    }
    result
}

/// `CursorMoveCommands.addCursorDown`.
pub fn add_cursor_down<V: IViewModel>(
    view_model: &V,
    cursors: &[CursorState],
    use_logical_line: bool,
) -> Vec<PartialCursorState> {
    add_cursor_translated(view_model, cursors, use_logical_line, move_operations::translate_down)
}

/// `CursorMoveCommands.addCursorUp`.
pub fn add_cursor_up<V: IViewModel>(
    view_model: &V,
    cursors: &[CursorState],
    use_logical_line: bool,
) -> Vec<PartialCursorState> {
    add_cursor_translated(view_model, cursors, use_logical_line, move_operations::translate_up)
}

/// `CursorMoveCommands.moveToBeginningOfLine`.
pub fn move_to_beginning_of_line<V: IViewModel>(
    view_model: &V,
    cursors: &[CursorState],
    in_selection_mode: bool,
) -> Vec<PartialCursorState> {
    cursors.iter().map(|cursor| move_to_line_start(view_model, cursor, in_selection_mode)).collect()
}

/// `_moveToLineStart`: numa linha quebrada, o primeiro movimento vai ao começo da linha da visão, e o
/// seguinte ao da linha do modelo.
fn move_to_line_start<V: IViewModel>(view_model: &V, cursor: &CursorState, in_selection_mode: bool) -> PartialCursorState {
    let current_view_state_column = cursor.view_state.position.column;
    let current_model_state_column = cursor.model_state.position.column;
    let is_first_line_of_wrapped_line = current_view_state_column == current_model_state_column;

    let current_view_state_line_number = cursor.view_state.position.line_number;
    let first_non_blank_column = view_model.get_line_first_non_whitespace_column(current_view_state_line_number);
    let is_beginning_of_view_line = current_view_state_column == first_non_blank_column;

    let config = view_model.cursor_config();
    if !is_first_line_of_wrapped_line && !is_beginning_of_view_line {
        // `_moveToLineStartByView`.
        from_view(move_operations::move_to_beginning_of_line(&config, view_model, &cursor.view_state, in_selection_mode))
    } else {
        // `_moveToLineStartByModel`.
        let model = view_model.model();
        from_model(move_operations::move_to_beginning_of_line(&config, &*model, &cursor.model_state, in_selection_mode))
    }
}

/// `CursorMoveCommands.moveToEndOfLine`.
pub fn move_to_end_of_line<V: IViewModel>(
    view_model: &V,
    cursors: &[CursorState],
    in_selection_mode: bool,
    sticky: bool,
) -> Vec<PartialCursorState> {
    cursors.iter().map(|cursor| move_to_line_end(view_model, cursor, in_selection_mode, sticky)).collect()
}

/// `_moveToLineEnd`.
fn move_to_line_end<V: IViewModel>(
    view_model: &V,
    cursor: &CursorState,
    in_selection_mode: bool,
    sticky: bool,
) -> PartialCursorState {
    let model = view_model.model();
    let simple: &dyn CursorSimpleModel = &*model;

    let view_state_position = cursor.view_state.position;
    let view_model_max_column = view_model.get_line_max_column(view_state_position.line_number);
    let is_end_of_view_line = view_state_position.column == view_model_max_column;

    let model_state_position = cursor.model_state.position;
    let model_max_column = simple.get_line_max_column(model_state_position.line_number);
    let is_end_line_of_wrapped_line =
        view_model_max_column - view_state_position.column == model_max_column - model_state_position.column;

    let config = view_model.cursor_config();
    if is_end_of_view_line || is_end_line_of_wrapped_line {
        // `_moveToLineEndByModel`.
        from_model(move_operations::move_to_end_of_line(&config, simple, &cursor.model_state, in_selection_mode, sticky))
    } else {
        // `_moveToLineEndByView`.
        from_view(move_operations::move_to_end_of_line(&config, view_model, &cursor.view_state, in_selection_mode, sticky))
    }
}

/// `CursorMoveCommands.expandLineSelection`.
pub fn expand_line_selection<V: IViewModel>(view_model: &V, cursors: &[CursorState]) -> Vec<PartialCursorState> {
    let model = view_model.model();
    let simple: &dyn CursorSimpleModel = &*model;
    let mut result = Vec::with_capacity(cursors.len());
    for cursor in cursors {
        let start_line_number = cursor.model_state.selection.start_line_number;
        let line_count = simple.get_line_count();

        let mut end_line_number = cursor.model_state.selection.end_line_number;
        let end_column;
        if end_line_number == line_count {
            end_column = simple.get_line_max_column(line_count);
        } else {
            end_line_number += 1;
            end_column = 1;
        }

        result.push(from_model(SingleCursorState::new(
            Range::new(start_line_number, 1, start_line_number, 1),
            SelectionStartKind::Simple,
            0,
            Position::new(end_line_number, end_column),
            0,
        )));
    }
    result
}

/// `CursorMoveCommands.moveToBeginningOfBuffer`.
pub fn move_to_beginning_of_buffer<V: IViewModel>(
    view_model: &V,
    cursors: &[CursorState],
    in_selection_mode: bool,
) -> Vec<PartialCursorState> {
    map_with(view_model, cursors, in_selection_mode, false, move_operations::move_to_beginning_of_buffer)
}

/// `CursorMoveCommands.moveToEndOfBuffer`.
pub fn move_to_end_of_buffer<V: IViewModel>(
    view_model: &V,
    cursors: &[CursorState],
    in_selection_mode: bool,
) -> Vec<PartialCursorState> {
    map_with(view_model, cursors, in_selection_mode, false, move_operations::move_to_end_of_buffer)
}

/// `CursorMoveCommands.selectAll`. O cursor não é lido (como no original).
pub fn select_all<V: IViewModel>(view_model: &V, _cursor: &CursorState) -> PartialCursorState {
    let model = view_model.model();
    let simple: &dyn CursorSimpleModel = &*model;
    let line_count = simple.get_line_count();
    let max_column = simple.get_line_max_column(line_count);

    from_model(SingleCursorState::new(
        Range::new(1, 1, 1, 1),
        SelectionStartKind::Simple,
        0,
        Position::new(line_count, max_column),
        0,
    ))
}

/// `CursorMoveCommands.line`: a seleção por linha (clique no número da linha, ou triplo clique).
pub fn line<V: IViewModel>(
    view_model: &V,
    cursor: &CursorState,
    in_selection_mode: bool,
    position: Position,
    view_position: Option<Position>,
) -> PartialCursorState {
    let model = view_model.model();
    let simple: &dyn CursorSimpleModel = &*model;
    let position = model.validate_position(position);
    let view_position = resolve_view_position(view_model, position, view_position);

    if !in_selection_mode {
        // Entrando na seleção por linha pela primeira vez.
        let line_count = simple.get_line_count();

        let mut select_to_line_number = position.line_number + 1;
        let mut select_to_column = 1;
        if select_to_line_number > line_count {
            select_to_line_number = line_count;
            select_to_column = simple.get_line_max_column(select_to_line_number);
        }

        return from_model(SingleCursorState::new(
            Range::new(position.line_number, 1, select_to_line_number, select_to_column),
            SelectionStartKind::Line,
            0,
            Position::new(select_to_line_number, select_to_column),
            0,
        ));
    }

    // Continuando a seleção por linha.
    let entering_line_number = cursor.model_state.selection_start.get_start_position().line_number;

    if position.line_number < entering_line_number {
        from_view(cursor.view_state.move_to(true, view_position.line_number, 1, 0))
    } else if position.line_number > entering_line_number {
        let line_count = view_model.get_line_count();

        let mut select_to_view_line_number = view_position.line_number + 1;
        let mut select_to_view_column = 1;
        if select_to_view_line_number > line_count {
            select_to_view_line_number = line_count;
            select_to_view_column = view_model.get_line_max_column(select_to_view_line_number);
        }

        from_view(cursor.view_state.move_to(true, select_to_view_line_number, select_to_view_column, 0))
    } else {
        let end_position_of_selection_start = cursor.model_state.selection_start.get_end_position();
        from_model(cursor.model_state.move_to(
            true,
            end_position_of_selection_start.line_number,
            end_position_of_selection_start.column,
            0,
        ))
    }
}

/// `CursorMoveCommands.word`: a seleção por palavra (duplo clique).
pub fn word<V: IViewModel>(
    view_model: &V,
    cursor: &CursorState,
    in_selection_mode: bool,
    position: Position,
) -> PartialCursorState {
    let model = view_model.model();
    let position = model.validate_position(position);
    from_model(WordOperations::word(&view_model.cursor_config(), &*model, &cursor.model_state, in_selection_mode, position))
}

/// `CursorMoveCommands.cancelSelection`.
pub fn cancel_selection<V: IViewModel>(_view_model: &V, cursor: &CursorState) -> PartialCursorState {
    if !cursor.model_state.has_selection() {
        return unchanged(cursor);
    }

    let line_number = cursor.view_state.position.line_number;
    let column = cursor.view_state.position.column;

    from_view(SingleCursorState::new(
        Range::new(line_number, column, line_number, column),
        SelectionStartKind::Simple,
        0,
        Position::new(line_number, column),
        0,
    ))
}

/// `CursorMoveCommands.moveTo`.
pub fn move_to<V: IViewModel>(
    view_model: &V,
    cursor: &CursorState,
    in_selection_mode: bool,
    position: Position,
    view_position: Option<Position>,
) -> PartialCursorState {
    if in_selection_mode {
        if cursor.model_state.selection_start_kind == SelectionStartKind::Word {
            return word(view_model, cursor, in_selection_mode, position);
        }
        if cursor.model_state.selection_start_kind == SelectionStartKind::Line {
            return line(view_model, cursor, in_selection_mode, position, view_position);
        }
    }
    let position = view_model.model().validate_position(position);
    let view_position = resolve_view_position(view_model, position, view_position);
    from_view(cursor.view_state.move_to(in_selection_mode, view_position.line_number, view_position.column, 0))
}

/// `CursorMoveCommands.simpleMove`. Devolve `None` (o `null` do original) para as direções de viewport.
pub fn simple_move<V: IViewModel>(
    view_model: &V,
    cursors: &[CursorState],
    direction: cursor_move::SimpleMoveDirection,
    in_selection_mode: bool,
    value: i32,
    unit: Unit,
) -> Option<Vec<PartialCursorState>> {
    match direction {
        Direction::Left | Direction::Right => {
            let towards_left = direction == Direction::Left;
            if unit == Unit::HalfLine {
                // Mover pela metade do comprimento da linha atual.
                Some(move_half_line(view_model, cursors, in_selection_mode, towards_left))
            } else {
                // Mover `value` colunas.
                Some(move_horizontally(view_model, cursors, in_selection_mode, value, towards_left))
            }
        }
        Direction::Up => Some(move_by_lines(view_model, cursors, in_selection_mode, value, unit, true)),
        Direction::Down => Some(move_by_lines(view_model, cursors, in_selection_mode, value, unit, false)),
        Direction::PrevBlankLine => Some(map_with(
            view_model,
            cursors,
            in_selection_mode,
            unit == Unit::WrappedLine,
            move_operations::move_to_prev_blank_line,
        )),
        Direction::NextBlankLine => Some(map_with(
            view_model,
            cursors,
            in_selection_mode,
            unit == Unit::WrappedLine,
            move_operations::move_to_next_blank_line,
        )),
        // Início da linha da visão.
        Direction::WrappedLineStart => {
            Some(move_to_view_column(cursors, in_selection_mode, |line_number| {
                view_model.get_line_min_column(line_number)
            }))
        }
        // Primeira coluna sem espaço em branco da linha da visão.
        Direction::WrappedLineFirstNonWhitespaceCharacter => {
            Some(move_to_view_column(cursors, in_selection_mode, |line_number| {
                view_model.get_line_first_non_whitespace_column(line_number)
            }))
        }
        // O "centro" da linha da visão.
        Direction::WrappedLineColumnCenter => {
            Some(move_to_view_column(cursors, in_selection_mode, |line_number| {
                // `Math.round((max + min) / 2)`.
                (view_model.get_line_max_column(line_number) + view_model.get_line_min_column(line_number) + 1)
                    .div_euclid(2)
            }))
        }
        // Fim da linha da visão.
        Direction::WrappedLineEnd => {
            Some(move_to_view_column(cursors, in_selection_mode, |line_number| {
                view_model.get_line_max_column(line_number)
            }))
        }
        // Última coluna sem espaço em branco da linha da visão.
        Direction::WrappedLineLastNonWhitespaceCharacter => {
            Some(move_to_view_column(cursors, in_selection_mode, |line_number| {
                view_model.get_line_last_non_whitespace_column(line_number)
            }))
        }
        _ => None,
    }
}

/// `CursorMoveCommands.viewportMove`. Devolve `None` (o `null` do original) para as outras direções.
pub fn viewport_move<V: IViewModel>(
    view_model: &V,
    cursors: &[CursorState],
    direction: cursor_move::ViewportDirection,
    in_selection_mode: bool,
    value: i32,
) -> Option<Vec<PartialCursorState>> {
    let visible_view_range = view_model.get_completely_visible_view_range();
    let visible_model_range = view_model.coordinates_converter().convert_view_range_to_model_range(visible_view_range);
    let model = view_model.model();
    let simple: &dyn CursorSimpleModel = &*model;
    match direction {
        Direction::ViewPortTop | Direction::ViewPortBottom | Direction::ViewPortCenter => {
            let model_line_number = match direction {
                // Para a n-ésima linha do viewport (de cima).
                Direction::ViewPortTop => first_line_number_in_range(simple, &visible_model_range, value),
                // Para a n-ésima linha do viewport (de baixo).
                Direction::ViewPortBottom => last_line_number_in_range(simple, &visible_model_range, value),
                // Para a linha no centro do viewport (`Math.round`).
                _ => (visible_model_range.start_line_number + visible_model_range.end_line_number + 1).div_euclid(2),
            };
            let model_column = simple.get_line_first_non_whitespace_column(model_line_number);
            Some(vec![from_model(cursors[0].model_state.move_to(in_selection_mode, model_line_number, model_column, 0))])
        }
        Direction::ViewPortIfOutside => {
            // Para uma posição dentro do viewport.
            Some(
                cursors
                    .iter()
                    .map(|cursor| {
                        find_position_in_viewport_if_outside(view_model, cursor, &visible_view_range, in_selection_mode)
                    })
                    .collect(),
            )
        }
        _ => None,
    }
}

/// `CursorMoveCommands.findPositionInViewportIfOutside`.
pub fn find_position_in_viewport_if_outside<V: IViewModel>(
    view_model: &V,
    cursor: &CursorState,
    visible_view_range: &Range,
    in_selection_mode: bool,
) -> PartialCursorState {
    let view_line_number = cursor.view_state.position.line_number;

    if visible_view_range.start_line_number <= view_line_number && view_line_number <= visible_view_range.end_line_number - 1 {
        // Nada a fazer, o cursor está no viewport.
        return unchanged(cursor);
    }

    let new_view_line_number = if view_line_number > visible_view_range.end_line_number - 1 {
        visible_view_range.end_line_number - 1
    } else if view_line_number < visible_view_range.start_line_number {
        visible_view_range.start_line_number
    } else {
        view_line_number
    };
    let position = move_operations::vertical(
        &view_model.cursor_config(),
        view_model,
        view_line_number,
        cursor.view_state.position.column,
        cursor.view_state.leftover_visible_columns,
        new_view_line_number,
        false,
        None,
    );
    from_view(cursor.view_state.move_to(
        in_selection_mode,
        position.line_number,
        position.column,
        position.leftover_visible_columns,
    ))
}

/// `_firstLineNumberInRange`: acha a n-ésima linha, a partir do começo, cujo início está no intervalo.
fn first_line_number_in_range(model: &dyn CursorSimpleModel, range: &Range, count: i32) -> i32 {
    let mut start_line_number = range.start_line_number;
    if range.start_column != model.get_line_min_column(start_line_number) {
        // Passa para a segunda linha se o início da primeira não está no intervalo.
        start_line_number += 1;
    }

    range.end_line_number.min(start_line_number + count - 1)
}

/// `_lastLineNumberInRange`: acha a n-ésima linha, a partir do fim, cujo início está no intervalo.
fn last_line_number_in_range(model: &dyn CursorSimpleModel, range: &Range, count: i32) -> i32 {
    let mut start_line_number = range.start_line_number;
    if range.start_column != model.get_line_min_column(start_line_number) {
        // Passa para a segunda linha se o início da primeira não está no intervalo.
        start_line_number += 1;
    }

    start_line_number.max(range.end_line_number - count + 1)
}

/// `_moveLeft` e `_moveRight`: no texto da direita para a esquerda, a esquerda e a direita se invertem.
fn move_horizontally<V: IViewModel>(
    view_model: &V,
    cursors: &[CursorState],
    in_selection_mode: bool,
    no_of_columns: i32,
    towards_left: bool,
) -> Vec<PartialCursorState> {
    let config = view_model.cursor_config();
    cursors
        .iter()
        .map(|cursor| {
            let direction = view_model.get_text_direction(cursor.view_state.position.line_number);
            let is_rtl = direction == TextDirection::Rtl;

            from_view(if towards_left != is_rtl {
                move_operations::move_left(&config, view_model, &cursor.view_state, in_selection_mode, no_of_columns)
            } else {
                move_operations::move_right(&config, view_model, &cursor.view_state, in_selection_mode, no_of_columns)
            })
        })
        .collect()
}

/// `_moveHalfLineLeft` e `_moveHalfLineRight`.
fn move_half_line<V: IViewModel>(
    view_model: &V,
    cursors: &[CursorState],
    in_selection_mode: bool,
    towards_left: bool,
) -> Vec<PartialCursorState> {
    let config = view_model.cursor_config();
    let op: MoveLinesFn = if towards_left { move_operations::move_left } else { move_operations::move_right };
    map_view(cursors, |state| {
        let view_line_number = state.position.line_number;
        // `Math.round(getLineLength(viewLineNumber) / 2)`.
        let half_line = (view_model.get_line_content(view_line_number).len() as i32 + 1) / 2;
        op(&config, view_model, state, in_selection_mode, half_line)
    })
}

/// `_moveUpByViewLines`, `_moveUpByModelLines`, `_moveUpByFoldedLines` e os de baixo: a unidade escolhe se o
/// movimento é pelas linhas da visão, pelas do modelo ou pelas do modelo pulando regiões dobradas.
fn move_by_lines<V: IViewModel>(
    view_model: &V,
    cursors: &[CursorState],
    in_selection_mode: bool,
    lines_count: i32,
    unit: Unit,
    up: bool,
) -> Vec<PartialCursorState> {
    let op: MoveLinesFn = if up { move_operations::move_up } else { move_operations::move_down };
    let config = view_model.cursor_config();
    match unit {
        // Mover pelas linhas da visão.
        Unit::WrappedLine => {
            map_view(cursors, |state| op(&config, view_model, state, in_selection_mode, lines_count))
        }
        // Mover pelas linhas do modelo, pulando as regiões dobradas.
        Unit::FoldedLine => move_by_folded_lines(view_model, cursors, in_selection_mode, lines_count, up),
        // Mover pelas linhas do modelo.
        _ => {
            let model = view_model.model();
            map_model(cursors, |state| op(&config, &*model, state, in_selection_mode, lines_count))
        }
    }
}

/// `_moveDownByFoldedLines` e `_moveUpByFoldedLines`.
fn move_by_folded_lines<V: IViewModel>(
    view_model: &V,
    cursors: &[CursorState],
    in_selection_mode: bool,
    count: i32,
    up: bool,
) -> Vec<PartialCursorState> {
    let config = view_model.cursor_config();
    let model = view_model.model();
    let simple: &dyn CursorSimpleModel = &*model;
    let line_count = simple.get_line_count();
    let hidden_areas = view_model.get_hidden_areas();

    cursors
        .iter()
        .map(|cursor| {
            let model_state = &cursor.model_state;
            let from_selection_edge = model_state.has_selection() && !in_selection_mode;

            let delta = if up {
                let start_line = if from_selection_edge {
                    model_state.selection.start_line_number
                } else {
                    model_state.position.line_number
                };
                start_line - target_folded_up(start_line, count, &hidden_areas)
            } else {
                let start_line = if from_selection_edge {
                    model_state.selection.end_line_number
                } else {
                    model_state.position.line_number
                };
                target_folded_down(start_line, count, &hidden_areas, line_count) - start_line
            };
            if delta == 0 {
                return from_model(*model_state);
            }
            let op: MoveLinesFn = if up { move_operations::move_up } else { move_operations::move_down };
            from_model(op(&config, simple, model_state, in_selection_mode, delta))
        })
        .collect()
}

/// `_targetFoldedDown`: a linha de destino depois de andar `count` passos para baixo a partir de
/// `start_line`, tratando cada região dobrada como um passo só.
fn target_folded_down(start_line: i32, count: i32, hidden_areas: &[Range], line_count: i32) -> i32 {
    let mut line = start_line;
    let mut i = 0;

    while i < hidden_areas.len() && hidden_areas[i].end_line_number < line + 1 {
        i += 1;
    }

    for _ in 0..count {
        if line >= line_count {
            return line_count;
        }

        let mut candidate = line + 1;
        while i < hidden_areas.len() && hidden_areas[i].end_line_number < candidate {
            i += 1;
        }

        if i < hidden_areas.len() && hidden_areas[i].start_line_number <= candidate {
            candidate = hidden_areas[i].end_line_number + 1;
        }

        if candidate > line_count {
            // A próxima linha visível não existe (por exemplo, uma dobra que chega ao fim do arquivo).
            return line;
        }

        line = candidate;
    }

    line
}

/// `_targetFoldedUp`: a linha de destino depois de andar `count` passos para cima a partir de `start_line`,
/// tratando cada região dobrada como um passo só.
fn target_folded_up(start_line: i32, count: i32, hidden_areas: &[Range]) -> i32 {
    let mut line = start_line;
    // O índice vai a -1, por isso é com sinal.
    let mut i = hidden_areas.len() as isize - 1;

    while i >= 0 && hidden_areas[i as usize].start_line_number > line - 1 {
        i -= 1;
    }

    for _ in 0..count {
        if line <= 1 {
            return 1;
        }

        let mut candidate = line - 1;
        while i >= 0 && hidden_areas[i as usize].start_line_number > candidate {
            i -= 1;
        }

        if i >= 0 && hidden_areas[i as usize].end_line_number >= candidate {
            candidate = hidden_areas[i as usize].start_line_number - 1;
        }

        if candidate < 1 {
            // A linha visível anterior não existe (por exemplo, uma dobra que chega ao começo do arquivo).
            return line;
        }

        line = candidate;
    }

    line
}

/// `_moveToViewMinColumn`, `_moveToViewFirstNonWhitespaceColumn`, `_moveToViewCenterColumn`,
/// `_moveToViewMaxColumn` e `_moveToViewLastNonWhitespaceColumn`: movem para uma coluna da mesma linha da
/// visão, e `column_of` escolhe a coluna a partir do número da linha. `_moveToViewPosition` é o `move_to`.
fn move_to_view_column(
    cursors: &[CursorState],
    in_selection_mode: bool,
    column_of: impl Fn(i32) -> i32,
) -> Vec<PartialCursorState> {
    map_view(cursors, |state| {
        let view_line_number = state.position.line_number;
        state.move_to(in_selection_mode, view_line_number, column_of(view_line_number), 0)
    })
}

/// `CursorMove`: o namespace com os argumentos, as direções e as unidades do comando `cursorMove`.
pub mod cursor_move {
    use super::{ArgumentValue, CommandArgument, CommandMetadata, TypeConstraint};

    /// A restrição de tipo do argumento (`isCursorMoveArgs`).
    fn is_cursor_move_args(arg: &ArgumentValue) -> bool {
        if !arg.is_object() {
            return false;
        }

        if !arg.property("to").is_string() {
            return false;
        }

        let select = arg.property("select");
        if !select.is_undefined() && !select.is_boolean() {
            return false;
        }

        let by = arg.property("by");
        if !by.is_undefined() && !by.is_string() {
            return false;
        }

        let value = arg.property("value");
        if !value.is_undefined() && !value.is_number() {
            return false;
        }

        let no_history = arg.property("noHistory");
        if !no_history.is_undefined() && !no_history.is_boolean() {
            return false;
        }

        true
    }

    /// `metadata`: a descrição e os argumentos do comando, para o registro de comandos.
    pub fn metadata() -> CommandMetadata {
        const DESCRIPTION: &str = concat!(
            "Property-value pairs that can be passed through this argument:\n",
            "\t\t\t\t\t\t* 'to': A mandatory logical position value providing where to move the cursor.\n",
            "\t\t\t\t\t\t\t```\n",
            "\t\t\t\t\t\t\t'left', 'right', 'up', 'down', 'prevBlankLine', 'nextBlankLine',\n",
            "\t\t\t\t\t\t\t'wrappedLineStart', 'wrappedLineEnd', 'wrappedLineColumnCenter'\n",
            "\t\t\t\t\t\t\t'wrappedLineFirstNonWhitespaceCharacter', 'wrappedLineLastNonWhitespaceCharacter'\n",
            "\t\t\t\t\t\t\t'viewPortTop', 'viewPortCenter', 'viewPortBottom', 'viewPortIfOutside'\n",
            "\t\t\t\t\t\t\t```\n",
            "\t\t\t\t\t\t* 'by': Unit to move. Default is computed based on 'to' value.\n",
            "\t\t\t\t\t\t\t```\n",
            "\t\t\t\t\t\t\t'line', 'wrappedLine', 'character', 'halfLine', 'foldedLine'\n",
            "\t\t\t\t\t\t\t```\n",
            "\t\t\t\t\t\tUse 'foldedLine' with 'up'/'down' to move by logical lines while treating each\n",
            "\t\t\t\t\t\tfolded region as a single step.\n",
            "\t\t\t\t\t\t* 'value': Number of units to move. Default is '1'.\n",
            "\t\t\t\t\t\t* 'select': If 'true' makes the selection. Default is 'false'.\n",
            "\t\t\t\t\t\t* 'noHistory': If 'true' does not add the movement to navigation history. Default is 'false'.\n",
            "\t\t\t\t\t",
        );

        let schema = ArgumentValue::object(vec![
            ("type", ArgumentValue::string("object")),
            ("required", ArgumentValue::strings(&["to"])),
            (
                "properties",
                ArgumentValue::object(vec![
                    (
                        "to",
                        ArgumentValue::object(vec![
                            ("type", ArgumentValue::string("string")),
                            (
                                "enum",
                                ArgumentValue::strings(&[
                                    "left",
                                    "right",
                                    "up",
                                    "down",
                                    "prevBlankLine",
                                    "nextBlankLine",
                                    "wrappedLineStart",
                                    "wrappedLineEnd",
                                    "wrappedLineColumnCenter",
                                    "wrappedLineFirstNonWhitespaceCharacter",
                                    "wrappedLineLastNonWhitespaceCharacter",
                                    "viewPortTop",
                                    "viewPortCenter",
                                    "viewPortBottom",
                                    "viewPortIfOutside",
                                ]),
                            ),
                        ]),
                    ),
                    (
                        "by",
                        ArgumentValue::object(vec![
                            ("type", ArgumentValue::string("string")),
                            ("enum", ArgumentValue::strings(&["line", "wrappedLine", "character", "halfLine", "foldedLine"])),
                        ]),
                    ),
                    (
                        "value",
                        ArgumentValue::object(vec![
                            ("type", ArgumentValue::string("number")),
                            ("default", ArgumentValue::Number(1.0)),
                        ]),
                    ),
                    (
                        "select",
                        ArgumentValue::object(vec![
                            ("type", ArgumentValue::string("boolean")),
                            ("default", ArgumentValue::Boolean(false)),
                        ]),
                    ),
                    (
                        "noHistory",
                        ArgumentValue::object(vec![
                            ("type", ArgumentValue::string("boolean")),
                            ("default", ArgumentValue::Boolean(false)),
                        ]),
                    ),
                ]),
            ),
        ]);

        CommandMetadata {
            description: "Move cursor to a logical position in the view".to_owned(),
            args: Some(vec![CommandArgument {
                name: "Cursor move argument object".to_owned(),
                is_optional: None,
                description: Some(DESCRIPTION.to_owned()),
                constraint: Some(TypeConstraint::Predicate(is_cursor_move_args)),
                schema: Some(schema),
            }]),
            returns: None,
        }
    }

    /// `RawDirection`: as posições na visão do comando `cursorMove`.
    pub mod raw_direction {
        pub const LEFT: &str = "left";
        pub const RIGHT: &str = "right";
        pub const UP: &str = "up";
        pub const DOWN: &str = "down";

        pub const PREV_BLANK_LINE: &str = "prevBlankLine";
        pub const NEXT_BLANK_LINE: &str = "nextBlankLine";

        pub const WRAPPED_LINE_START: &str = "wrappedLineStart";
        pub const WRAPPED_LINE_FIRST_NON_WHITESPACE_CHARACTER: &str = "wrappedLineFirstNonWhitespaceCharacter";
        pub const WRAPPED_LINE_COLUMN_CENTER: &str = "wrappedLineColumnCenter";
        pub const WRAPPED_LINE_END: &str = "wrappedLineEnd";
        pub const WRAPPED_LINE_LAST_NON_WHITESPACE_CHARACTER: &str = "wrappedLineLastNonWhitespaceCharacter";

        pub const VIEW_PORT_TOP: &str = "viewPortTop";
        pub const VIEW_PORT_CENTER: &str = "viewPortCenter";
        pub const VIEW_PORT_BOTTOM: &str = "viewPortBottom";

        pub const VIEW_PORT_IF_OUTSIDE: &str = "viewPortIfOutside";
    }

    /// `RawUnit`: as unidades do argumento `by` do comando `cursorMove`.
    pub mod raw_unit {
        pub const LINE: &str = "line";
        pub const WRAPPED_LINE: &str = "wrappedLine";
        pub const CHARACTER: &str = "character";
        pub const HALF_LINE: &str = "halfLine";
        pub const FOLDED_LINE: &str = "foldedLine";
    }

    /// `RawArguments`: os argumentos do comando `cursorMove`.
    #[derive(Clone, Debug, PartialEq)]
    pub struct RawArguments {
        pub to: String,
        pub select: Option<bool>,
        pub by: Option<String>,
        pub value: Option<f64>,
        pub no_history: Option<bool>,
    }

    /// `Partial<RawArguments>`: a entrada de [`parse`].
    #[derive(Clone, Debug, Default, PartialEq)]
    pub struct PartialRawArguments {
        pub to: Option<String>,
        pub select: Option<bool>,
        pub by: Option<String>,
        pub value: Option<f64>,
        pub no_history: Option<bool>,
    }

    impl From<RawArguments> for PartialRawArguments {
        fn from(args: RawArguments) -> Self {
            Self {
                to: Some(args.to),
                select: args.select,
                by: args.by,
                value: args.value,
                no_history: args.no_history,
            }
        }
    }

    /// `parse`: `None` (o `null` do original) para argumentos ilegais.
    pub fn parse(args: &PartialRawArguments) -> Option<ParsedArguments> {
        let to = args.to.as_deref().filter(|to| !to.is_empty())?;

        let direction = match to {
            raw_direction::LEFT => Direction::Left,
            raw_direction::RIGHT => Direction::Right,
            raw_direction::UP => Direction::Up,
            raw_direction::DOWN => Direction::Down,
            raw_direction::PREV_BLANK_LINE => Direction::PrevBlankLine,
            raw_direction::NEXT_BLANK_LINE => Direction::NextBlankLine,
            raw_direction::WRAPPED_LINE_START => Direction::WrappedLineStart,
            raw_direction::WRAPPED_LINE_FIRST_NON_WHITESPACE_CHARACTER => {
                Direction::WrappedLineFirstNonWhitespaceCharacter
            }
            raw_direction::WRAPPED_LINE_COLUMN_CENTER => Direction::WrappedLineColumnCenter,
            raw_direction::WRAPPED_LINE_END => Direction::WrappedLineEnd,
            raw_direction::WRAPPED_LINE_LAST_NON_WHITESPACE_CHARACTER => Direction::WrappedLineLastNonWhitespaceCharacter,
            raw_direction::VIEW_PORT_TOP => Direction::ViewPortTop,
            raw_direction::VIEW_PORT_BOTTOM => Direction::ViewPortBottom,
            raw_direction::VIEW_PORT_CENTER => Direction::ViewPortCenter,
            raw_direction::VIEW_PORT_IF_OUTSIDE => Direction::ViewPortIfOutside,
            // Argumentos ilegais.
            _ => return None,
        };

        let unit = match args.by.as_deref() {
            Some(raw_unit::LINE) => Unit::Line,
            Some(raw_unit::WRAPPED_LINE) => Unit::WrappedLine,
            Some(raw_unit::CHARACTER) => Unit::Character,
            Some(raw_unit::HALF_LINE) => Unit::HalfLine,
            Some(raw_unit::FOLDED_LINE) => Unit::FoldedLine,
            _ => Unit::None,
        };

        // `args.value || 1`: zero e `NaN` caem em 1.
        let value = match args.value {
            Some(value) if value != 0.0 && !value.is_nan() => value as i32,
            _ => 1,
        };

        Some(ParsedArguments {
            direction,
            unit,
            select: args.select.unwrap_or(false),
            value,
            no_history: args.no_history.unwrap_or(false),
        })
    }

    /// `ParsedArguments`.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct ParsedArguments {
        pub direction: Direction,
        pub unit: Unit,
        pub select: bool,
        pub value: i32,
        pub no_history: bool,
    }

    /// `SimpleMoveArguments`.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct SimpleMoveArguments {
        pub direction: SimpleMoveDirection,
        pub unit: Unit,
        pub select: bool,
        pub value: i32,
    }

    /// `Direction` (um `const enum` no original).
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum Direction {
        Left = 0,
        Right = 1,
        Up = 2,
        Down = 3,
        PrevBlankLine = 4,
        NextBlankLine = 5,

        WrappedLineStart = 6,
        WrappedLineFirstNonWhitespaceCharacter = 7,
        WrappedLineColumnCenter = 8,
        WrappedLineEnd = 9,
        WrappedLineLastNonWhitespaceCharacter = 10,

        ViewPortTop = 11,
        ViewPortCenter = 12,
        ViewPortBottom = 13,

        ViewPortIfOutside = 14,
    }

    /// `SimpleMoveDirection`: `Left`, `Right`, `Up`, `Down`, `PrevBlankLine`, `NextBlankLine` e as cinco
    /// `WrappedLine*`. O tipo é o próprio [`Direction`] (ver a nota no topo do módulo).
    pub type SimpleMoveDirection = Direction;

    /// `ViewportDirection`: `ViewPortTop`, `ViewPortCenter`, `ViewPortBottom` e `ViewPortIfOutside`. O tipo é
    /// o próprio [`Direction`] (ver a nota no topo do módulo).
    pub type ViewportDirection = Direction;

    /// `Unit` (um `const enum` no original).
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum Unit {
        None = 0,
        Line = 1,
        WrappedLine = 2,
        Character = 3,
        HalfLine = 4,
        FoldedLine = 5,
    }
}

/// O valor dinâmico de um argumento de comando (`unknown` e `IJSONSchema` no TypeScript): o que o JSON
/// carrega.
#[derive(Clone, Debug, PartialEq)]
pub enum ArgumentValue {
    Undefined,
    Null,
    Boolean(bool),
    Number(f64),
    String(String),
    Array(Vec<ArgumentValue>),
    /// Um objeto, com as propriedades na ordem de inserção.
    Object(Vec<(String, ArgumentValue)>),
}

/// O valor de uma propriedade ausente.
static UNDEFINED: ArgumentValue = ArgumentValue::Undefined;

impl ArgumentValue {
    /// Um objeto a partir de pares nome e valor.
    pub fn object(entries: Vec<(&str, ArgumentValue)>) -> Self {
        ArgumentValue::Object(entries.into_iter().map(|(name, value)| (name.to_owned(), value)).collect())
    }

    /// Um texto.
    pub fn string(value: &str) -> Self {
        ArgumentValue::String(value.to_owned())
    }

    /// Uma lista de textos.
    pub fn strings(values: &[&str]) -> Self {
        ArgumentValue::Array(values.iter().map(|value| ArgumentValue::string(value)).collect())
    }

    /// `obj[name]`: `undefined` se não é um objeto ou se a propriedade não existe. A última propriedade de
    /// mesmo nome vence, como numa atribuição repetida.
    pub fn property(&self, name: &str) -> &ArgumentValue {
        match self {
            ArgumentValue::Object(entries) => {
                entries.iter().rev().find(|(key, _)| key == name).map_or(&UNDEFINED, |(_, value)| value)
            }
            _ => &UNDEFINED,
        }
    }

    /// `types.isObject`: um objeto que não é nulo, lista, `RegExp` nem `Date`.
    pub fn is_object(&self) -> bool {
        matches!(self, ArgumentValue::Object(_))
    }

    /// `types.isString`.
    pub fn is_string(&self) -> bool {
        matches!(self, ArgumentValue::String(_))
    }

    /// `types.isBoolean`.
    pub fn is_boolean(&self) -> bool {
        matches!(self, ArgumentValue::Boolean(_))
    }

    /// `types.isNumber`: um número que não é `NaN`.
    pub fn is_number(&self) -> bool {
        matches!(self, ArgumentValue::Number(value) if !value.is_nan())
    }

    /// `types.isUndefined`.
    pub fn is_undefined(&self) -> bool {
        matches!(self, ArgumentValue::Undefined)
    }
}

/// `TypeConstraint` (`base/common/types.ts`): o nome de um tipo ou um predicado sobre o valor.
#[derive(Clone, Copy, Debug)]
pub enum TypeConstraint {
    TypeName(&'static str),
    Predicate(fn(&ArgumentValue) -> bool),
}

/// Um item de `ICommandMetadata.args`.
#[derive(Clone, Debug)]
pub struct CommandArgument {
    pub name: String,
    pub is_optional: Option<bool>,
    pub description: Option<String>,
    pub constraint: Option<TypeConstraint>,
    /// `IJSONSchema`.
    pub schema: Option<ArgumentValue>,
}

/// `ICommandMetadata` (`platform/commands/common/commands.ts`): a descrição de um comando e dos seus
/// argumentos, que o registro de comandos usa.
#[derive(Clone, Debug)]
pub struct CommandMetadata {
    pub description: String,
    pub args: Option<Vec<CommandArgument>>,
    pub returns: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::cursor_move::{
        metadata, parse, raw_direction, raw_unit, Direction, ParsedArguments, PartialRawArguments, RawArguments, Unit,
    };
    use super::*;

    fn raw(to: &str) -> PartialRawArguments {
        PartialRawArguments { to: Some(to.to_owned()), ..PartialRawArguments::default() }
    }

    #[test]
    fn parse_without_a_valid_direction_is_none() {
        assert_eq!(parse(&PartialRawArguments::default()), None);
        assert_eq!(parse(&raw("")), None);
        assert_eq!(parse(&raw("sideways")), None);
        // Os nomes diferenciam maiúsculas de minúsculas.
        assert_eq!(parse(&raw("Left")), None);
    }

    #[test]
    fn parse_applies_the_defaults() {
        assert_eq!(
            parse(&raw("left")),
            Some(ParsedArguments { direction: Direction::Left, unit: Unit::None, select: false, value: 1, no_history: false })
        );
    }

    #[test]
    fn parse_maps_every_raw_direction() {
        let cases = [
            (raw_direction::LEFT, Direction::Left),
            (raw_direction::RIGHT, Direction::Right),
            (raw_direction::UP, Direction::Up),
            (raw_direction::DOWN, Direction::Down),
            (raw_direction::PREV_BLANK_LINE, Direction::PrevBlankLine),
            (raw_direction::NEXT_BLANK_LINE, Direction::NextBlankLine),
            (raw_direction::WRAPPED_LINE_START, Direction::WrappedLineStart),
            (raw_direction::WRAPPED_LINE_FIRST_NON_WHITESPACE_CHARACTER, Direction::WrappedLineFirstNonWhitespaceCharacter),
            (raw_direction::WRAPPED_LINE_COLUMN_CENTER, Direction::WrappedLineColumnCenter),
            (raw_direction::WRAPPED_LINE_END, Direction::WrappedLineEnd),
            (raw_direction::WRAPPED_LINE_LAST_NON_WHITESPACE_CHARACTER, Direction::WrappedLineLastNonWhitespaceCharacter),
            (raw_direction::VIEW_PORT_TOP, Direction::ViewPortTop),
            (raw_direction::VIEW_PORT_CENTER, Direction::ViewPortCenter),
            (raw_direction::VIEW_PORT_BOTTOM, Direction::ViewPortBottom),
            (raw_direction::VIEW_PORT_IF_OUTSIDE, Direction::ViewPortIfOutside),
        ];
        for (name, direction) in cases {
            assert_eq!(parse(&raw(name)).map(|parsed| parsed.direction), Some(direction), "{name}");
        }
        // Os textos são os do original.
        assert_eq!(raw_direction::WRAPPED_LINE_FIRST_NON_WHITESPACE_CHARACTER, "wrappedLineFirstNonWhitespaceCharacter");
        assert_eq!(raw_direction::VIEW_PORT_IF_OUTSIDE, "viewPortIfOutside");
    }

    #[test]
    fn parse_maps_every_raw_unit() {
        let cases = [
            (raw_unit::LINE, Unit::Line),
            (raw_unit::WRAPPED_LINE, Unit::WrappedLine),
            (raw_unit::CHARACTER, Unit::Character),
            (raw_unit::HALF_LINE, Unit::HalfLine),
            (raw_unit::FOLDED_LINE, Unit::FoldedLine),
        ];
        for (name, unit) in cases {
            let args = PartialRawArguments { by: Some(name.to_owned()), ..raw("down") };
            assert_eq!(parse(&args).map(|parsed| parsed.unit), Some(unit), "{name}");
        }
        // Uma unidade desconhecida é `None`, sem invalidar o comando.
        let unknown = PartialRawArguments { by: Some("paragraph".to_owned()), ..raw("down") };
        assert_eq!(parse(&unknown).map(|parsed| parsed.unit), Some(Unit::None));
    }

    #[test]
    fn parse_reads_select_value_and_no_history() {
        let args = PartialRawArguments {
            select: Some(true),
            by: Some("wrappedLine".to_owned()),
            value: Some(5.0),
            no_history: Some(true),
            ..raw("up")
        };
        assert_eq!(
            parse(&args),
            Some(ParsedArguments { direction: Direction::Up, unit: Unit::WrappedLine, select: true, value: 5, no_history: true })
        );
    }

    #[test]
    fn parse_value_falls_back_to_one_for_zero_and_nan() {
        for value in [0.0, -0.0, f64::NAN] {
            let args = PartialRawArguments { value: Some(value), ..raw("right") };
            assert_eq!(parse(&args).map(|parsed| parsed.value), Some(1), "{value}");
        }
        let negative = PartialRawArguments { value: Some(-3.0), ..raw("right") };
        assert_eq!(parse(&negative).map(|parsed| parsed.value), Some(-3));
    }

    #[test]
    fn raw_arguments_convert_to_partial() {
        let args = RawArguments {
            to: "down".to_owned(),
            select: Some(false),
            by: Some("line".to_owned()),
            value: Some(2.0),
            no_history: None,
        };
        let partial = PartialRawArguments::from(args);
        assert_eq!(partial.to.as_deref(), Some("down"));
        assert_eq!(parse(&partial).map(|parsed| (parsed.direction, parsed.unit, parsed.value)), Some((Direction::Down, Unit::Line, 2)));
    }

    #[test]
    fn direction_and_unit_values_follow_the_original() {
        assert_eq!(Direction::Left as i32, 0);
        assert_eq!(Direction::WrappedLineStart as i32, 6);
        assert_eq!(Direction::ViewPortTop as i32, 11);
        assert_eq!(Direction::ViewPortIfOutside as i32, 14);
        assert_eq!(Unit::None as i32, 0);
        assert_eq!(Unit::FoldedLine as i32, 5);
    }

    fn constraint() -> fn(&ArgumentValue) -> bool {
        let metadata = metadata();
        let argument = metadata.args.expect("args").remove(0);
        match argument.constraint {
            Some(TypeConstraint::Predicate(predicate)) => predicate,
            other => panic!("restrição inesperada: {other:?}"),
        }
    }

    #[test]
    fn metadata_describes_the_command() {
        let metadata = metadata();
        assert_eq!(metadata.description, "Move cursor to a logical position in the view");
        assert_eq!(metadata.returns, None);
        let args = metadata.args.expect("args");
        assert_eq!(args.len(), 1);
        assert_eq!(args[0].name, "Cursor move argument object");
        let description = args[0].description.as_deref().expect("description");
        assert!(description.starts_with("Property-value pairs that can be passed through this argument:\n"));
        assert!(description.contains("\t\t\t\t\t\t* 'to': A mandatory logical position value"));
        assert!(description.ends_with("Default is 'false'.\n\t\t\t\t\t"));
        let schema = args[0].schema.as_ref().expect("schema");
        assert_eq!(schema.property("required"), &ArgumentValue::strings(&["to"]));
        let to = schema.property("properties").property("to");
        let ArgumentValue::Array(names) = to.property("enum") else { panic!("enum de to") };
        assert_eq!(names.len(), 15);
        assert_eq!(schema.property("properties").property("value").property("default"), &ArgumentValue::Number(1.0));
        assert_eq!(schema.property("properties").property("noHistory").property("default"), &ArgumentValue::Boolean(false));
    }

    #[test]
    fn cursor_move_args_constraint_checks_every_property_type() {
        let accepts = constraint();
        let ok = ArgumentValue::object(vec![("to", ArgumentValue::string("left"))]);
        assert!(accepts(&ok));

        let full = ArgumentValue::object(vec![
            ("to", ArgumentValue::string("down")),
            ("select", ArgumentValue::Boolean(true)),
            ("by", ArgumentValue::string("line")),
            ("value", ArgumentValue::Number(3.0)),
            ("noHistory", ArgumentValue::Boolean(false)),
        ]);
        assert!(accepts(&full));

        // Sem objeto, ou sem `to` textual.
        assert!(!accepts(&ArgumentValue::Undefined));
        assert!(!accepts(&ArgumentValue::Null));
        assert!(!accepts(&ArgumentValue::string("left")));
        assert!(!accepts(&ArgumentValue::Array(vec![])));
        assert!(!accepts(&ArgumentValue::object(vec![])));
        assert!(!accepts(&ArgumentValue::object(vec![("to", ArgumentValue::Number(1.0))])));

        // Cada opcional presente precisa ter o tipo certo (um `undefined` explícito vale como ausente).
        let with = |name: &'static str, value: ArgumentValue| {
            ArgumentValue::object(vec![("to", ArgumentValue::string("up")), (name, value)])
        };
        assert!(!accepts(&with("select", ArgumentValue::string("true"))));
        assert!(!accepts(&with("by", ArgumentValue::Number(1.0))));
        assert!(!accepts(&with("value", ArgumentValue::string("1"))));
        assert!(!accepts(&with("value", ArgumentValue::Number(f64::NAN))));
        assert!(!accepts(&with("noHistory", ArgumentValue::Number(0.0))));
        assert!(accepts(&with("select", ArgumentValue::Undefined)));
    }

    #[test]
    fn folded_targets_treat_each_fold_as_one_step() {
        // Dobra escondendo as linhas 3 a 5 de um arquivo de 10 linhas.
        let hidden = [Range::new(3, 1, 5, 1)];
        // Da linha 2, um passo para baixo pula a dobra e cai na 6.
        assert_eq!(target_folded_down(2, 1, &hidden, 10), 6);
        assert_eq!(target_folded_down(2, 2, &hidden, 10), 7);
        // Sem dobra no caminho, o passo é de uma linha.
        assert_eq!(target_folded_down(6, 1, &hidden, 10), 7);
        // No fim do arquivo, fica na última linha.
        assert_eq!(target_folded_down(10, 3, &hidden, 10), 10);
        // Da linha 6, um passo para cima pula a dobra e cai na 2.
        assert_eq!(target_folded_up(6, 1, &hidden), 2);
        assert_eq!(target_folded_up(6, 2, &hidden), 1);
        assert_eq!(target_folded_up(1, 4, &hidden), 1);
        // Dobra que chega ao fim do arquivo: não há próxima linha visível.
        let to_end = [Range::new(8, 1, 10, 1)];
        assert_eq!(target_folded_down(7, 1, &to_end, 10), 7);
        // Dobra que chega ao começo do arquivo: não há linha visível anterior.
        let from_start = [Range::new(1, 1, 3, 1)];
        assert_eq!(target_folded_up(4, 1, &from_start), 4);
    }
}
