//! O mínimo do cursor: movimento, seleção por palavra e por linha. O `Cursor` do Monaco
//! (`src/vs/editor/common/cursor`, com vários cursores, auto fechamento, `cursorMoveCommands`) entra depois.
// TODO(integração): trocar pelo porte de `src/vs/editor/common/cursor` (CursorMoveCommands, WordOperations,
// DeleteOperations, TypeOperations) quando ele existir.

use je_editor::cursor_common::CursorSimpleModel;
use je_base::strings::{is_high_surrogate, is_low_surrogate};
use je_editor::cursor_columns::{column_from_visible_column, visible_column_from_column};
use je_editor::position::Position;
use je_editor::range::Range;
use je_editor::selection::Selection;
use je_editor::text_model::TextModel;
use je_editor::word_character_classifier::WordCharacterClass;

use crate::text::{self, Run};

/// O estado do cursor: a seleção e a coluna visual desejada (para subir e descer mantendo a coluna).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CursorState {
    pub selection: Selection,
    pub desired_visual_column: Option<i32>,
}

impl CursorState {
    pub fn at(position: Position) -> Self {
        Self { selection: Selection::new(position.line_number, position.column, position.line_number, position.column), desired_visual_column: None }
    }

    pub fn position(&self) -> Position {
        self.selection.get_position()
    }

    pub fn anchor(&self) -> Position {
        self.selection.get_selection_start()
    }

    pub fn is_empty(&self) -> bool {
        self.selection.is_empty()
    }
}

/// Uma seleção de `anchor` até `position`.
pub fn select(anchor: Position, position: Position) -> Selection {
    Selection::new(anchor.line_number, anchor.column, position.line_number, position.column)
}

fn max_column(model: &TextModel, line: i32) -> i32 {
    model.get_line_max_column(line)
}

/// Uma coluna para a esquerda (um caractere inteiro, par substituto incluso); no início da linha, o fim da anterior.
pub fn left_of(model: &TextModel, p: Position) -> Position {
    if p.column > 1 {
        let line = model.get_line_content(p.line_number);
        let units = line.units();
        let i = (p.column - 1) as usize;
        let step = if i >= 2 && is_low_surrogate(units[i - 1]) && is_high_surrogate(units[i - 2]) { 2 } else { 1 };
        Position::new(p.line_number, p.column - step)
    } else if p.line_number > 1 {
        Position::new(p.line_number - 1, max_column(model, p.line_number - 1))
    } else {
        p
    }
}

/// Uma coluna para a direita; no fim da linha, o início da seguinte.
pub fn right_of(model: &TextModel, p: Position) -> Position {
    let max = max_column(model, p.line_number);
    if p.column < max {
        let line = model.get_line_content(p.line_number);
        let units = line.units();
        let i = (p.column - 1) as usize;
        let step = if i + 1 < units.len() && is_high_surrogate(units[i]) && is_low_surrogate(units[i + 1]) { 2 } else { 1 };
        Position::new(p.line_number, p.column + step)
    } else if p.line_number < model.get_line_count() {
        Position::new(p.line_number + 1, 1)
    } else {
        p
    }
}

/// Sobe ou desce `delta` linhas mantendo a coluna visual desejada. Passando do começo vai para a coluna 1,
/// passando do fim vai para o fim da última linha, como o Monaco.
pub fn vertical(model: &TextModel, p: Position, delta: i32, desired: Option<i32>) -> (Position, i32) {
    let tab = model.get_options().tab_size;
    let line = model.get_line_content(p.line_number);
    let desired = desired.unwrap_or_else(|| visible_column_from_column(line.units(), p.column, tab));
    let target = p.line_number + delta;
    if target < 1 {
        return (Position::new(1, 1), desired);
    }
    if target > model.get_line_count() {
        let last = model.get_line_count();
        return (Position::new(last, max_column(model, last)), desired);
    }
    let units = model.get_line_content(target);
    (Position::new(target, column_from_visible_column(units.units(), desired, tab)), desired)
}

/// `Home` inteligente: a primeira coluna que não é espaço; estando nela, a coluna 1.
pub fn home(model: &TextModel, p: Position) -> Position {
    let first = model.get_line_first_non_whitespace_column(p.line_number);
    let target = if first == 0 || first == p.column { 1 } else { first };
    Position::new(p.line_number, target)
}

pub fn end(model: &TextModel, p: Position) -> Position {
    Position::new(p.line_number, max_column(model, p.line_number))
}

/// Ctrl+Seta para a direita: o fim da próxima palavra (`WordNavigationType.WordEnd`).
pub fn word_right(model: &TextModel, p: Position) -> Position {
    let max = max_column(model, p.line_number);
    if p.column >= max {
        return if p.line_number < model.get_line_count() { Position::new(p.line_number + 1, 1) } else { p };
    }
    let line = model.get_line_content(p.line_number);
    let classifier = text::word_classifier();
    let offset = (p.column - 1) as usize;
    let target = text::runs(&classifier, line.units())
        .into_iter()
        .filter(|r| r.class != WordCharacterClass::Whitespace)
        .find(|r| r.end > offset)
        .map_or(max, |r| r.end as i32 + 1);
    Position::new(p.line_number, target)
}

/// Ctrl+Seta para a esquerda: o início da palavra anterior (`WordNavigationType.WordStart`).
pub fn word_left(model: &TextModel, p: Position) -> Position {
    if p.column <= 1 {
        return if p.line_number > 1 { Position::new(p.line_number - 1, max_column(model, p.line_number - 1)) } else { p };
    }
    let line = model.get_line_content(p.line_number);
    let classifier = text::word_classifier();
    let offset = (p.column - 1) as usize;
    let target = text::runs(&classifier, line.units())
        .into_iter()
        .filter(|r| r.class != WordCharacterClass::Whitespace)
        .rfind(|r| r.start < offset)
        .map_or(1, |r| r.start as i32 + 1);
    Position::new(p.line_number, target)
}

/// O trecho de mesma classe sob a posição (a palavra, a pontuação ou o espaço), para o duplo clique.
pub fn word_range_at(model: &TextModel, p: Position) -> Range {
    let line = model.get_line_content(p.line_number);
    let units = line.units();
    if units.is_empty() {
        return Range::new(p.line_number, 1, p.line_number, 1);
    }
    let classifier = text::word_classifier();
    let runs = text::runs(&classifier, units);
    let offset = (p.column - 1).max(0) as usize;
    let pick = |r: &&Run| r.start <= offset && offset < r.end;
    // Entre duas classes, o Monaco prefere o trecho à direita do cursor, e no fim da linha o último.
    let run = runs.iter().find(pick).or_else(|| runs.last()).copied().unwrap_or(Run { start: 0, end: 0, class: WordCharacterClass::Regular });
    Range::new(p.line_number, run.start as i32 + 1, p.line_number, run.end as i32 + 1)
}

/// A linha inteira com a quebra (o triplo clique): do início da linha ao início da seguinte, ou ao fim da última.
pub fn line_range_at(model: &TextModel, line: i32) -> Range {
    if line < model.get_line_count() {
        Range::new(line, 1, line + 1, 1)
    } else {
        Range::new(line, 1, line, max_column(model, line))
    }
}

#[cfg(test)]
mod tests {
    use std::rc::Rc;

    use je_editor::text_model::{TextModel, TextSource};
    use je_editor::undo_redo_service::UndoRedoService;

    use super::*;

    fn model(text: &str) -> Rc<TextModel> {
        TextModel::new(TextSource::from(text), "plaintext", TextModel::default_creation_options(), None, UndoRedoService::for_tests())
    }

    #[test]
    fn left_and_right_cross_lines() {
        let m = model("ab\ncd");
        assert_eq!(right_of(&m, Position::new(1, 3)), Position::new(2, 1));
        assert_eq!(left_of(&m, Position::new(2, 1)), Position::new(1, 3));
        assert_eq!(left_of(&m, Position::new(1, 1)), Position::new(1, 1));
        assert_eq!(right_of(&m, Position::new(2, 3)), Position::new(2, 3));
    }

    #[test]
    fn surrogate_pair_moves_whole() {
        let m = model("a😀b");
        assert_eq!(right_of(&m, Position::new(1, 2)), Position::new(1, 4));
        assert_eq!(left_of(&m, Position::new(1, 4)), Position::new(1, 2));
    }

    #[test]
    fn up_and_down_keep_visual_column() {
        let m = model("abcdefgh\nab\nabcdefgh");
        let (p, d) = vertical(&m, Position::new(1, 6), 1, None);
        assert_eq!(p, Position::new(2, 3));
        let (p, _) = vertical(&m, p, 1, Some(d));
        assert_eq!(p, Position::new(3, 6));
        assert_eq!(vertical(&m, Position::new(1, 4), -1, None).0, Position::new(1, 1));
        assert_eq!(vertical(&m, Position::new(3, 4), 1, None).0, Position::new(3, 9));
    }

    #[test]
    fn home_toggles_between_text_and_column_one() {
        let m = model("    foo");
        assert_eq!(home(&m, Position::new(1, 8)), Position::new(1, 5));
        assert_eq!(home(&m, Position::new(1, 5)), Position::new(1, 1));
        assert_eq!(home(&m, Position::new(1, 1)), Position::new(1, 5));
        assert_eq!(end(&m, Position::new(1, 1)), Position::new(1, 8));
    }

    #[test]
    fn word_movement_by_ctrl_arrow() {
        let m = model("foo.bar  baz\nqux");
        assert_eq!(word_right(&m, Position::new(1, 1)), Position::new(1, 4));
        assert_eq!(word_right(&m, Position::new(1, 4)), Position::new(1, 5));
        assert_eq!(word_right(&m, Position::new(1, 5)), Position::new(1, 8));
        assert_eq!(word_right(&m, Position::new(1, 8)), Position::new(1, 13));
        assert_eq!(word_right(&m, Position::new(1, 13)), Position::new(2, 1));
        assert_eq!(word_left(&m, Position::new(1, 13)), Position::new(1, 10));
        assert_eq!(word_left(&m, Position::new(1, 10)), Position::new(1, 5));
        assert_eq!(word_left(&m, Position::new(1, 5)), Position::new(1, 4));
        assert_eq!(word_left(&m, Position::new(1, 4)), Position::new(1, 1));
        assert_eq!(word_left(&m, Position::new(2, 1)), Position::new(1, 13));
    }

    #[test]
    fn word_and_line_selection() {
        let m = model("foo.bar  baz\nqux");
        assert_eq!(word_range_at(&m, Position::new(1, 2)), Range::new(1, 1, 1, 4));
        assert_eq!(word_range_at(&m, Position::new(1, 5)), Range::new(1, 5, 1, 8));
        assert_eq!(word_range_at(&m, Position::new(1, 8)), Range::new(1, 8, 1, 10));
        assert_eq!(word_range_at(&m, Position::new(1, 13)), Range::new(1, 10, 1, 13));
        assert_eq!(line_range_at(&m, 1), Range::new(1, 1, 2, 1));
        assert_eq!(line_range_at(&m, 2), Range::new(2, 1, 2, 4));
    }
}
