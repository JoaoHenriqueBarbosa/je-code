//! Porte de `src/vs/editor/common/cursor/oneCursor.ts`: um cursor (uma seleção), com o estado no modelo e
//! na visão e o intervalo rastreado que acompanha a seleção durante as edições.
//!
//! Decisões de tradução:
//!
//! - `modelState!` e `viewState!` do original são atribuídos pelo `_setState` do construtor; aqui o
//!   construtor parte do estado inicial e chama o mesmo `_setState` (validação e intervalo rastreado
//!   incluídos), então o resultado é o mesmo.
//! - `_selTrackedRange: string | null` é `Option<String>`, o id do intervalo rastreado no modelo
//!   (`set_tracked_range` / `get_tracked_range` do `TextModel`).
//! - `modelState: SingleCursorState | null` dos parâmetros é `Option<SingleCursorState>` (o estado é `Copy`).

use crate::cursor_common::{CursorSimpleModel, CursorState, SelectionStartKind, SingleCursorState};
use crate::cursor_context::CursorContext;
use crate::model::{PositionAffinity, TrackedRangeStickiness};
use crate::position::Position;
use crate::range::Range;
use crate::selection::Selection;

/// Representa um único cursor.
pub struct Cursor {
    pub model_state: SingleCursorState,
    pub view_state: SingleCursorState,

    sel_tracked_range: Option<String>,
    track_selection: bool,
}

impl Cursor {
    pub fn new(context: &CursorContext) -> Self {
        let initial = SingleCursorState::new(Range::new(1, 1, 1, 1), SelectionStartKind::Simple, 0, Position::new(1, 1), 0);
        let mut cursor = Self { model_state: initial, view_state: initial, sel_tracked_range: None, track_selection: true };
        cursor.set_state_impl(context, Some(initial), Some(initial));
        cursor
    }

    pub fn dispose(&mut self, context: &CursorContext) {
        self.remove_tracked_range(context);
    }

    pub fn start_tracking_selection(&mut self, context: &CursorContext) {
        self.track_selection = true;
        self.update_tracked_range(context);
    }

    pub fn stop_tracking_selection(&mut self, context: &CursorContext) {
        self.track_selection = false;
        self.remove_tracked_range(context);
    }

    fn update_tracked_range(&mut self, context: &CursorContext) {
        if !self.track_selection {
            // não rastrear a seleção
            return;
        }
        self.sel_tracked_range = context.model.set_tracked_range(
            self.sel_tracked_range.as_deref(),
            Some(&*self.model_state.selection),
            TrackedRangeStickiness::AlwaysGrowsWhenTypingAtEdges,
        );
    }

    fn remove_tracked_range(&mut self, context: &CursorContext) {
        self.sel_tracked_range = context.model.set_tracked_range(
            self.sel_tracked_range.as_deref(),
            None,
            TrackedRangeStickiness::AlwaysGrowsWhenTypingAtEdges,
        );
    }

    pub fn as_cursor_state(&self) -> CursorState {
        CursorState::new(self.model_state, self.view_state)
    }

    pub fn read_selection_from_markers(&self, context: &CursorContext) -> Selection {
        let id = self.sel_tracked_range.as_deref().expect("o intervalo rastreado da seleção existe");
        let range = context.model.get_tracked_range(id).expect("o intervalo rastreado da seleção existe");

        if self.model_state.selection.is_empty() && !range.is_empty() {
            // Evita selecionar texto ao recuperar a partir dos marcadores
            return Selection::from_range(&range.collapse_to_end(), self.model_state.selection.get_direction());
        }

        Selection::from_range(&range, self.model_state.selection.get_direction())
    }

    pub fn ensure_valid_state(&mut self, context: &CursorContext) {
        self.set_state_impl(context, Some(self.model_state), Some(self.view_state));
    }

    pub fn set_state(
        &mut self,
        context: &CursorContext,
        model_state: Option<SingleCursorState>,
        view_state: Option<SingleCursorState>,
    ) {
        self.set_state_impl(context, model_state, view_state);
    }

    fn validate_position_with_cache(
        view_model: &dyn CursorSimpleModel,
        position: Position,
        cache_input: Position,
        cache_output: Position,
    ) -> Position {
        if position == cache_input {
            return cache_output;
        }
        view_model.normalize_position(position, PositionAffinity::None)
    }

    fn validate_view_state(view_model: &dyn CursorSimpleModel, view_state: SingleCursorState) -> SingleCursorState {
        let position = view_state.position;
        let s_start_position = view_state.selection_start.get_start_position();
        let s_end_position = view_state.selection_start.get_end_position();

        let valid_position = view_model.normalize_position(position, PositionAffinity::None);
        let valid_s_start_position =
            Self::validate_position_with_cache(view_model, s_start_position, position, valid_position);
        let valid_s_end_position =
            Self::validate_position_with_cache(view_model, s_end_position, s_start_position, valid_s_start_position);

        if position == valid_position && s_start_position == valid_s_start_position && s_end_position == valid_s_end_position
        {
            // caminho rápido: o estado é válido
            return view_state;
        }

        SingleCursorState::new(
            Range::from_positions(valid_s_start_position, valid_s_end_position),
            view_state.selection_start_kind,
            view_state.selection_start_leftover_visible_columns + s_start_position.column - valid_s_start_position.column,
            valid_position,
            view_state.leftover_visible_columns + position.column - valid_position.column,
        )
    }

    fn set_state_impl(
        &mut self,
        context: &CursorContext,
        model_state: Option<SingleCursorState>,
        view_state: Option<SingleCursorState>,
    ) {
        let view_state = view_state.map(|view_state| Self::validate_view_state(&*context.view_model, view_state));

        let model_state = match model_state {
            None => {
                let Some(view_state) = view_state else {
                    return;
                };
                // Só temos o estado da visão => calcula o estado do modelo
                let selection_start = context
                    .model
                    .validate_range(&context.coordinates_converter.convert_view_range_to_model_range(view_state.selection_start));

                let position = context
                    .model
                    .validate_position(context.coordinates_converter.convert_view_position_to_model_position(view_state.position));

                SingleCursorState::new(
                    selection_start,
                    view_state.selection_start_kind,
                    view_state.selection_start_leftover_visible_columns,
                    position,
                    view_state.leftover_visible_columns,
                )
            }
            Some(model_state) => {
                // Valida o novo estado do modelo
                let selection_start = context.model.validate_range(&model_state.selection_start);
                let selection_start_leftover_visible_columns = if model_state.selection_start == selection_start {
                    model_state.selection_start_leftover_visible_columns
                } else {
                    0
                };

                let position = context.model.validate_position(model_state.position);
                let leftover_visible_columns =
                    if model_state.position == position { model_state.leftover_visible_columns } else { 0 };

                SingleCursorState::new(
                    selection_start,
                    model_state.selection_start_kind,
                    selection_start_leftover_visible_columns,
                    position,
                    leftover_visible_columns,
                )
            }
        };

        let view_state = match view_state {
            None => {
                // Só temos o estado do modelo => calcula o estado da visão
                let converter = &context.coordinates_converter;
                let view_selection_start1 = converter.convert_model_position_to_view_position(
                    Position::new(model_state.selection_start.start_line_number, model_state.selection_start.start_column),
                    None,
                    None,
                    None,
                );
                let view_selection_start2 = converter.convert_model_position_to_view_position(
                    Position::new(model_state.selection_start.end_line_number, model_state.selection_start.end_column),
                    None,
                    None,
                    None,
                );
                let view_selection_start = Range::new(
                    view_selection_start1.line_number,
                    view_selection_start1.column,
                    view_selection_start2.line_number,
                    view_selection_start2.column,
                );
                let view_position = converter.convert_model_position_to_view_position(model_state.position, None, None, None);
                SingleCursorState::new(
                    view_selection_start,
                    model_state.selection_start_kind,
                    model_state.selection_start_leftover_visible_columns,
                    view_position,
                    model_state.leftover_visible_columns,
                )
            }
            Some(view_state) => {
                // Valida o novo estado da visão
                let view_selection_start = context
                    .coordinates_converter
                    .validate_view_range(view_state.selection_start, model_state.selection_start);
                let view_position =
                    context.coordinates_converter.validate_view_position(view_state.position, model_state.position);
                SingleCursorState::new(
                    view_selection_start,
                    model_state.selection_start_kind,
                    model_state.selection_start_leftover_visible_columns,
                    view_position,
                    model_state.leftover_visible_columns,
                )
            }
        };

        self.model_state = model_state;
        self.view_state = view_state;

        self.update_tracked_range(context);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cursor_context::test_support::context_for;
    use crate::selection::SelectionDirection;

    fn state(start: Range, position: Position, start_leftover: i32, leftover: i32) -> SingleCursorState {
        SingleCursorState::new(start, SelectionStartKind::Simple, start_leftover, position, leftover)
    }

    #[test]
    fn new_cursor_starts_at_the_document_start() {
        let context = context_for("hello\nworld", true);
        let cursor = Cursor::new(&context);
        assert_eq!(cursor.model_state.selection, Selection::new(1, 1, 1, 1));
        assert_eq!(cursor.view_state.selection, Selection::new(1, 1, 1, 1));
        assert_eq!(cursor.read_selection_from_markers(&context), Selection::new(1, 1, 1, 1));
    }

    #[test]
    fn set_state_validates_the_model_state_and_drops_the_leftover() {
        let context = context_for("abc\ndef", true);
        let mut cursor = Cursor::new(&context);
        // Posição fora do documento: vai para o fim (2,4) e o excedente de colunas visíveis é zerado.
        cursor.set_state(&context, Some(state(Range::new(5, 100, 5, 100), Position::new(5, 100), 3, 7)), None);
        assert_eq!(cursor.model_state.position, Position::new(2, 4));
        assert_eq!(cursor.model_state.selection_start, Range::new(2, 4, 2, 4));
        assert_eq!(cursor.model_state.leftover_visible_columns, 0);
        assert_eq!(cursor.model_state.selection_start_leftover_visible_columns, 0);
        assert_eq!(cursor.view_state.position, Position::new(2, 4));
    }

    #[test]
    fn set_state_keeps_the_leftover_of_a_valid_model_state() {
        let context = context_for("abc\ndef", true);
        let mut cursor = Cursor::new(&context);
        cursor.set_state(&context, Some(state(Range::new(1, 2, 1, 2), Position::new(2, 3), 5, 7)), None);
        assert_eq!(cursor.model_state.position, Position::new(2, 3));
        assert_eq!(cursor.model_state.selection, Selection::new(1, 2, 2, 3));
        assert_eq!(cursor.model_state.leftover_visible_columns, 7);
        assert_eq!(cursor.model_state.selection_start_leftover_visible_columns, 5);
        assert_eq!(cursor.view_state.selection, Selection::new(1, 2, 2, 3));
        assert_eq!(cursor.view_state.leftover_visible_columns, 7);
    }

    #[test]
    fn set_state_computes_the_model_state_from_the_view_state() {
        let context = context_for("abc\ndef", true);
        let mut cursor = Cursor::new(&context);
        cursor.set_state(&context, None, Some(state(Range::new(1, 1, 1, 1), Position::new(2, 2), 0, 4)));
        assert_eq!(cursor.model_state.position, Position::new(2, 2));
        assert_eq!(cursor.model_state.leftover_visible_columns, 4);
        assert_eq!(cursor.model_state.selection, Selection::new(1, 1, 2, 2));
    }

    #[test]
    fn set_state_without_any_state_changes_nothing() {
        let context = context_for("abc", true);
        let mut cursor = Cursor::new(&context);
        cursor.set_state(&context, Some(state(Range::new(1, 2, 1, 2), Position::new(1, 3), 0, 0)), None);
        cursor.set_state(&context, None, None);
        assert_eq!(cursor.model_state.selection, Selection::new(1, 2, 1, 3));
    }

    #[test]
    fn tracked_range_follows_the_selection_and_its_direction() {
        let context = context_for("hello world", true);
        let mut cursor = Cursor::new(&context);
        // Seleção da direita para a esquerda: âncora em (1,6), posição em (1,2).
        cursor.set_state(&context, Some(state(Range::new(1, 6, 1, 6), Position::new(1, 2), 0, 0)), None);
        let selection = cursor.read_selection_from_markers(&context);
        assert_eq!(selection, Selection::new(1, 6, 1, 2));
        assert_eq!(selection.get_direction(), SelectionDirection::Rtl);
    }

    #[test]
    fn stop_and_start_tracking_selection_recreate_the_tracked_range() {
        let context = context_for("hello world", true);
        let mut cursor = Cursor::new(&context);
        cursor.set_state(&context, Some(state(Range::new(1, 3, 1, 3), Position::new(1, 5), 0, 0)), None);
        cursor.stop_tracking_selection(&context);
        // Sem rastreio, o `_setState` não cria o intervalo.
        cursor.ensure_valid_state(&context);
        assert!(cursor.sel_tracked_range.is_none());
        cursor.start_tracking_selection(&context);
        assert_eq!(cursor.read_selection_from_markers(&context), Selection::new(1, 3, 1, 5));
        cursor.dispose(&context);
        assert!(cursor.sel_tracked_range.is_none());
    }

    #[test]
    fn as_cursor_state_pairs_the_model_and_view_states() {
        let context = context_for("abc", true);
        let mut cursor = Cursor::new(&context);
        cursor.set_state(&context, Some(state(Range::new(1, 2, 1, 2), Position::new(1, 3), 0, 0)), None);
        let both = cursor.as_cursor_state();
        assert!(both.model_state.equals(&cursor.model_state));
        assert!(both.view_state.equals(&cursor.view_state));
    }
}
