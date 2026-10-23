//! Porte de `src/vs/editor/common/cursor/cursorCollection.ts`: o conjunto de cursores (o primário e os
//! secundários) e a fusão dos que se sobrepõem.
//!
//! Decisões de tradução:
//!
//! - `states: PartialCursorState[] | null` de `setStates` é `Option<&[PartialCursorState]>`. Um `Some` vazio
//!   falha no `states[0]` como o original (que lança `TypeError`).
//! - `setSelections(ISelection[])` recebe `&[Selection]` (`ISelection` e `Selection` são o mesmo tipo).
//! - Em `normalize`, a cópia `cursors` do original é uma lista paralela que recebe os mesmos `splice` que
//!   `this.cursors` (o `_removeSecondaryCursor(looserIndex - 1)` remove o índice `looserIndex`), então os dois
//!   vetores têm sempre o mesmo conteúdo e os índices valem nos dois: a cópia não existe, usa-se `self.cursors`.
//! - O laço `for` com `sortedCursorIndex--` vira um `while` que só avança quando não houve fusão (o efeito
//!   líquido do decremento mais o incremento do `for`).
//! - `compareBy(c => c.viewState.position, Position.compare)` é o comparador escrito direto, e o `sort` do
//!   original (estável) é o `sort_by` do Rust (também estável).

use je_base::arrays::to_ordering;
use je_base::arrays_find::{find_first_min, find_last_max};

use crate::cursor_common::{CursorState, PartialCursorState};
use crate::cursor_context::CursorContext;
use crate::one_cursor::Cursor;
use crate::position::Position;
use crate::range::Range;
use crate::selection::Selection;

pub struct CursorCollection {
    context: CursorContext,

    /// `cursors[0]` é o cursor primário, portanto `cursors.len() >= 1` sempre vale.
    /// `cursors[1..]` são os cursores secundários.
    cursors: Vec<Cursor>,

    // Um índice que identifica o último cursor que foi adicionado / movido (pense em Ctrl+arrastar).
    // O índice se refere a `cursors[1..]`, isto é, depois de remover o cursor primário.
    last_added_cursor_index: usize,
}

/// `SortedCursor` de `normalize`.
struct SortedCursor {
    index: usize,
    selection: Selection,
}

fn compare_view_positions(a: &Cursor, b: &Cursor) -> f64 {
    f64::from(Position::compare(&a.view_state.position, &b.view_state.position))
}

impl CursorCollection {
    pub fn new(context: CursorContext) -> Self {
        let cursors = vec![Cursor::new(&context)];
        Self { context, cursors, last_added_cursor_index: 0 }
    }

    pub fn dispose(&mut self) {
        for cursor in &mut self.cursors {
            cursor.dispose(&self.context);
        }
    }

    pub fn start_tracking_selections(&mut self) {
        for cursor in &mut self.cursors {
            cursor.start_tracking_selection(&self.context);
        }
    }

    pub fn stop_tracking_selections(&mut self) {
        for cursor in &mut self.cursors {
            cursor.stop_tracking_selection(&self.context);
        }
    }

    pub fn update_context(&mut self, context: CursorContext) {
        self.context = context;
    }

    pub fn ensure_valid_state(&mut self) {
        for cursor in &mut self.cursors {
            cursor.ensure_valid_state(&self.context);
        }
    }

    pub fn read_selection_from_markers(&self) -> Vec<Selection> {
        self.cursors.iter().map(|c| c.read_selection_from_markers(&self.context)).collect()
    }

    pub fn get_all(&self) -> Vec<CursorState> {
        self.cursors.iter().map(Cursor::as_cursor_state).collect()
    }

    pub fn get_view_positions(&self) -> Vec<Position> {
        self.cursors.iter().map(|c| c.view_state.position).collect()
    }

    pub fn get_top_most_view_position(&self) -> Position {
        find_first_min(&self.cursors, compare_view_positions).expect("há sempre o cursor primário").view_state.position
    }

    pub fn get_bottom_most_view_position(&self) -> Position {
        find_last_max(&self.cursors, compare_view_positions).expect("há sempre o cursor primário").view_state.position
    }

    pub fn get_selections(&self) -> Vec<Selection> {
        self.cursors.iter().map(|c| c.model_state.selection).collect()
    }

    pub fn get_view_selections(&self) -> Vec<Selection> {
        self.cursors.iter().map(|c| c.view_state.selection).collect()
    }

    pub fn set_selections(&mut self, selections: &[Selection]) {
        let states: Vec<PartialCursorState> =
            CursorState::from_model_selections(selections).into_iter().map(PartialCursorState::Model).collect();
        self.set_states(Some(&states));
    }

    pub fn get_primary_cursor(&self) -> CursorState {
        self.cursors[0].as_cursor_state()
    }

    pub fn set_states(&mut self, states: Option<&[PartialCursorState]>) {
        let Some(states) = states else {
            return;
        };
        self.cursors[0].set_state(&self.context, states[0].model_state().copied(), states[0].view_state().copied());
        self.set_secondary_states(&states[1..]);
    }

    /// Cria ou descarta cursores secundários conforme o necessário para casar com o número de `secondary_states`.
    fn set_secondary_states(&mut self, secondary_states: &[PartialCursorState]) {
        let secondary_cursors_length = self.cursors.len() - 1;
        let secondary_states_length = secondary_states.len();

        if secondary_cursors_length < secondary_states_length {
            let create_cnt = secondary_states_length - secondary_cursors_length;
            for _ in 0..create_cnt {
                self.add_secondary_cursor();
            }
        } else if secondary_cursors_length > secondary_states_length {
            let remove_cnt = secondary_cursors_length - secondary_states_length;
            for _ in 0..remove_cnt {
                self.remove_secondary_cursor(self.cursors.len() - 2);
            }
        }

        for (i, secondary_state) in secondary_states.iter().enumerate() {
            self.cursors[i + 1].set_state(
                &self.context,
                secondary_state.model_state().copied(),
                secondary_state.view_state().copied(),
            );
        }
    }

    pub fn kill_secondary_cursors(&mut self) {
        self.set_secondary_states(&[]);
    }

    fn add_secondary_cursor(&mut self) {
        self.cursors.push(Cursor::new(&self.context));
        self.last_added_cursor_index = self.cursors.len() - 1;
    }

    pub fn get_last_added_cursor_index(&self) -> usize {
        if self.cursors.len() == 1 || self.last_added_cursor_index == 0 {
            return 0;
        }
        self.last_added_cursor_index
    }

    fn remove_secondary_cursor(&mut self, remove_index: usize) {
        if self.last_added_cursor_index >= remove_index + 1 {
            self.last_added_cursor_index -= 1;
        }
        self.cursors[remove_index + 1].dispose(&self.context);
        self.cursors.remove(remove_index + 1);
    }

    pub fn normalize(&mut self) {
        if self.cursors.len() == 1 {
            return;
        }

        let mut sorted_cursors: Vec<SortedCursor> = self
            .cursors
            .iter()
            .enumerate()
            .map(|(index, cursor)| SortedCursor { index, selection: cursor.model_state.selection })
            .collect();

        sorted_cursors.sort_by(|a, b| {
            to_ordering(f64::from(Range::compare_ranges_using_starts(Some(&*a.selection), Some(&*b.selection))))
        });

        let mut sorted_cursor_index = 0;
        while sorted_cursor_index + 1 < sorted_cursors.len() {
            let current = &sorted_cursors[sorted_cursor_index];
            let next = &sorted_cursors[sorted_cursor_index + 1];

            let current_selection = current.selection;
            let next_selection = next.selection;

            if !self.context.cursor_config.multi_cursor_merge_overlapping {
                sorted_cursor_index += 1;
                continue;
            }

            let should_merge_cursors = if next_selection.is_empty() || current_selection.is_empty() {
                // Funde cursores encostados se um deles está colapsado
                next_selection.get_start_position().is_before_or_equal(&current_selection.get_end_position())
            } else {
                // Funde só os que se sobrepõem (isto é, permite intervalos encostados)
                next_selection.get_start_position().is_before(&current_selection.get_end_position())
            };

            if !should_merge_cursors {
                sorted_cursor_index += 1;
                continue;
            }

            let (winner_sorted_cursor_index, looser_sorted_cursor_index) = if current.index < next.index {
                (sorted_cursor_index, sorted_cursor_index + 1)
            } else {
                (sorted_cursor_index + 1, sorted_cursor_index)
            };

            let looser_index = sorted_cursors[looser_sorted_cursor_index].index;
            let winner_index = sorted_cursors[winner_sorted_cursor_index].index;

            let looser_selection = sorted_cursors[looser_sorted_cursor_index].selection;
            let winner_selection = sorted_cursors[winner_sorted_cursor_index].selection;

            if looser_selection != winner_selection {
                let resulting_range = looser_selection.plus_range(&winner_selection);
                let looser_selection_is_ltr = looser_selection.selection_start_line_number == looser_selection.start_line_number
                    && looser_selection.selection_start_column == looser_selection.start_column;
                let winner_selection_is_ltr = winner_selection.selection_start_line_number == winner_selection.start_line_number
                    && winner_selection.selection_start_column == winner_selection.start_column;

                // Dá mais importância ao último cursor adicionado (pense em Ctrl+arrastar e bater em outro cursor)
                let resulting_selection_is_ltr = if looser_index == self.last_added_cursor_index {
                    self.last_added_cursor_index = winner_index;
                    looser_selection_is_ltr
                } else {
                    // O vencedor leva tudo
                    winner_selection_is_ltr
                };

                let resulting_selection = if resulting_selection_is_ltr {
                    Selection::new(
                        resulting_range.start_line_number,
                        resulting_range.start_column,
                        resulting_range.end_line_number,
                        resulting_range.end_column,
                    )
                } else {
                    Selection::new(
                        resulting_range.end_line_number,
                        resulting_range.end_column,
                        resulting_range.start_line_number,
                        resulting_range.start_column,
                    )
                };

                sorted_cursors[winner_sorted_cursor_index].selection = resulting_selection;
                let resulting_state = CursorState::from_model_selection(resulting_selection);
                self.cursors[winner_index].set_state(&self.context, Some(resulting_state.model_state), None);
            }

            for sorted_cursor in &mut sorted_cursors {
                if sorted_cursor.index > looser_index {
                    sorted_cursor.index -= 1;
                }
            }

            sorted_cursors.remove(looser_sorted_cursor_index);
            self.remove_secondary_cursor(looser_index - 1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cursor_context::test_support::context_for;

    fn collection(text: &str, merge_overlapping: bool) -> CursorCollection {
        CursorCollection::new(context_for(text, merge_overlapping))
    }

    #[test]
    fn starts_with_a_single_primary_cursor_at_the_start() {
        let cursors = collection("one two\nthree\nfour", true);
        assert_eq!(cursors.get_selections(), vec![Selection::new(1, 1, 1, 1)]);
        assert_eq!(cursors.get_last_added_cursor_index(), 0);
        assert_eq!(cursors.get_primary_cursor().model_state.position, Position::new(1, 1));
    }

    #[test]
    fn set_selections_creates_and_kills_secondary_cursors() {
        let mut cursors = collection("one two\nthree\nfour", true);
        cursors.set_selections(&[Selection::new(1, 1, 1, 2), Selection::new(2, 1, 2, 3), Selection::new(3, 1, 3, 1)]);
        assert_eq!(cursors.get_all().len(), 3);
        assert_eq!(cursors.get_view_selections(), cursors.get_selections());
        assert_eq!(cursors.get_last_added_cursor_index(), 2);

        cursors.kill_secondary_cursors();
        assert_eq!(cursors.get_selections(), vec![Selection::new(1, 1, 1, 2)]);
        assert_eq!(cursors.get_last_added_cursor_index(), 0);
    }

    #[test]
    fn set_selections_with_fewer_states_removes_the_last_secondary_cursors() {
        let mut cursors = collection("one two\nthree\nfour", true);
        cursors.set_selections(&[Selection::new(1, 1, 1, 1), Selection::new(2, 1, 2, 1), Selection::new(3, 1, 3, 1)]);
        cursors.set_selections(&[Selection::new(3, 1, 3, 2), Selection::new(2, 1, 2, 2)]);
        assert_eq!(cursors.get_selections(), vec![Selection::new(3, 1, 3, 2), Selection::new(2, 1, 2, 2)]);
        assert_eq!(cursors.get_last_added_cursor_index(), 1);
    }

    #[test]
    fn top_and_bottom_most_view_positions() {
        let mut cursors = collection("one two\nthree\nfour", true);
        cursors.set_selections(&[Selection::new(3, 1, 3, 1), Selection::new(1, 2, 1, 2), Selection::new(2, 1, 2, 1)]);
        assert_eq!(cursors.get_top_most_view_position(), Position::new(1, 2));
        assert_eq!(cursors.get_bottom_most_view_position(), Position::new(3, 1));
        assert_eq!(
            cursors.get_view_positions(),
            vec![Position::new(3, 1), Position::new(1, 2), Position::new(2, 1)]
        );
    }

    #[test]
    fn normalize_merges_overlapping_selections_and_the_winner_keeps_its_direction() {
        let mut cursors = collection("one two\nthree\nfour", true);
        cursors.set_selections(&[Selection::new(1, 1, 1, 3), Selection::new(1, 2, 1, 5), Selection::new(3, 1, 3, 1)]);
        cursors.normalize();
        assert_eq!(cursors.get_selections(), vec![Selection::new(1, 1, 1, 5), Selection::new(3, 1, 3, 1)]);
        // O último adicionado era o índice 2; remover o secundário 0 o desloca para 1.
        assert_eq!(cursors.get_last_added_cursor_index(), 1);
    }

    #[test]
    fn normalize_gives_the_last_added_cursor_the_direction() {
        let mut cursors = collection("one two\nthree\nfour", true);
        cursors.set_selections(&[Selection::new(1, 1, 1, 1), Selection::new(1, 5, 1, 2), Selection::new(1, 3, 1, 7)]);
        cursors.normalize();
        assert_eq!(cursors.get_selections(), vec![Selection::new(1, 1, 1, 1), Selection::new(1, 2, 1, 7)]);
        assert_eq!(cursors.get_last_added_cursor_index(), 1);
    }

    #[test]
    fn normalize_merges_touching_cursors_only_when_one_is_collapsed() {
        let mut cursors = collection("one two\nthree\nfour", true);
        cursors.set_selections(&[Selection::new(1, 1, 1, 3), Selection::new(1, 3, 1, 3)]);
        cursors.normalize();
        assert_eq!(cursors.get_selections(), vec![Selection::new(1, 1, 1, 3)]);

        let mut cursors = collection("one two\nthree\nfour", true);
        cursors.set_selections(&[Selection::new(1, 1, 1, 3), Selection::new(1, 3, 1, 5)]);
        cursors.normalize();
        assert_eq!(cursors.get_selections(), vec![Selection::new(1, 1, 1, 3), Selection::new(1, 3, 1, 5)]);
    }

    #[test]
    fn normalize_merges_equal_selections_without_changing_the_survivor() {
        let mut cursors = collection("one two", true);
        cursors.set_selections(&[Selection::new(1, 2, 1, 2), Selection::new(1, 2, 1, 2)]);
        cursors.normalize();
        assert_eq!(cursors.get_selections(), vec![Selection::new(1, 2, 1, 2)]);
    }

    #[test]
    fn normalize_does_nothing_without_merge_overlapping() {
        let mut cursors = collection("one two\nthree\nfour", false);
        cursors.set_selections(&[Selection::new(1, 1, 1, 3), Selection::new(1, 2, 1, 5), Selection::new(1, 2, 1, 5)]);
        cursors.normalize();
        assert_eq!(cursors.get_selections().len(), 3);
    }

    #[test]
    fn read_selection_from_markers_returns_every_cursor_selection() {
        let mut cursors = collection("one two\nthree\nfour", true);
        cursors.set_selections(&[Selection::new(1, 3, 1, 1), Selection::new(2, 1, 2, 4)]);
        assert_eq!(cursors.read_selection_from_markers(), vec![Selection::new(1, 3, 1, 1), Selection::new(2, 1, 2, 4)]);
        cursors.stop_tracking_selections();
        cursors.start_tracking_selections();
        cursors.ensure_valid_state();
        assert_eq!(cursors.read_selection_from_markers(), cursors.get_selections());
        cursors.dispose();
    }
}
