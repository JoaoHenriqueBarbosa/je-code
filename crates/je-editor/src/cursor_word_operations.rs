//! Porte de `src/vs/editor/common/cursor/cursorWordOperations.ts`: as operações de palavra do cursor
//! (mover, apagar, selecionar por palavra e por parte de palavra).
//!
//! Decisões de tradução:
//!
//! - `WordOperations` e `WordPartOperations` são estruturas sem campos com funções associadas (as `static` do
//!   original). `WordPartOperations extends WordOperations` só acrescenta estáticas, então as de
//!   `WordOperations` não são repetidas nela: quem chamaria `WordPartOperations.moveWordLeft` chama
//!   `WordOperations::move_word_left`.
//! - `ICursorSimpleModel` é `&dyn CursorSimpleModel`. `ITextModel` é o [`TextModel`] concreto, que precisa
//!   implementar [`CursorSimpleModel`] (o original passa o modelo de texto onde se espera o modelo simples).
//! - Linhas são UTF-16 (`&[u16]`). Um índice fora da linha lê como o `NaN` do `charCodeAt`, que cai na classe
//!   regular do classificador e não casa com nenhum caractere; por isso os índices são `i32` (podem ser -1).
//! - `DeleteWordContext` empresta os dados (`'a`). As estratégias de fechamento automático são `&str`, como em
//!   [`CursorConfiguration`].
//! - `deleteWordPartLeft` devolve `Option<Range>`: o `candidates[2]` do original é `undefined` quando
//!   `deleteWordLeft` devolve `null` nas duas formas (início do arquivo), e o `Option` guarda esse caso.
//! - `DeleteOperations.isAutoClosingPairDelete` vem de `crate::cursor_delete_operations`, com os mesmos
//!   parâmetros do original.

use je_base::char_code;
use je_base::strings;

use crate::cursor_common::{CursorConfiguration, CursorSimpleModel, SelectionStartKind, SingleCursorState};
use crate::cursor_delete_operations::DeleteOperations;
use crate::language_configuration::AutoClosingPairs;
use crate::position::Position;
use crate::range::Range;
use crate::selection::Selection;
use crate::text_model::TextModel;
use crate::word_character_classifier::{
    get_map_for_word_separators, IntlWordSegmentData, WordCharacterClass, WordCharacterClassifier,
};
use crate::word_helper::WordAtPosition;

/// `IFindWordResult`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct FindWordResult {
    /// O índice onde a palavra começa.
    start: i32,
    /// O índice onde a palavra termina.
    end: i32,
    /// O tipo da palavra.
    word_type: WordType,
    /// O motivo pelo qual a palavra terminou.
    next_char_class: WordCharacterClass,
}

/// `WordType`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum WordType {
    None = 0,
    Regular = 1,
    Separator = 2,
}

/// `WordNavigationType`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WordNavigationType {
    WordStart = 0,
    WordStartFast = 1,
    WordEnd = 2,
    /// Respeita a definição de palavra do Chrome.
    WordAccessibility = 3,
}

/// `DeleteWordContext`.
pub struct DeleteWordContext<'a> {
    pub word_separators: &'a WordCharacterClassifier,
    pub model: &'a TextModel,
    pub selection: Selection,
    pub whitespace_heuristics: bool,
    /// `EditorAutoClosingEditStrategy`.
    pub auto_closing_delete: &'a str,
    /// `EditorAutoClosingStrategy`.
    pub auto_closing_brackets: &'a str,
    /// `EditorAutoClosingStrategy`.
    pub auto_closing_quotes: &'a str,
    pub auto_closing_pairs: &'a AutoClosingPairs,
    pub auto_closed_characters: &'a [Range],
}

/// `charCodeAt`: `None` fora da linha (o `NaN` do JavaScript).
fn code_at(line: &[u16], index: i32) -> Option<u16> {
    usize::try_from(index).ok().and_then(|index| line.get(index).copied())
}

/// `wordSeparators.get(lineContent.charCodeAt(index))`: fora da linha, o `NaN` cai na classe padrão.
fn class_at(word_separators: &WordCharacterClassifier, line: &[u16], index: i32) -> WordCharacterClass {
    match code_at(line, index) {
        Some(code) => word_separators.get(i32::from(code)),
        None => WordCharacterClass::Regular,
    }
}

/// `findPrevIntlWordBeforeOrAtOffset`: um deslocamento negativo não acha nada.
fn find_prev_intl_word(
    word_separators: &WordCharacterClassifier,
    line: &[u16],
    offset: i32,
) -> Option<IntlWordSegmentData> {
    usize::try_from(offset).ok().and_then(|offset| word_separators.find_prev_intl_word_before_or_at_offset(line, offset))
}

/// `findNextIntlWordAtOrAfterOffset`: um deslocamento negativo não acha nada.
fn find_next_intl_word(
    word_separators: &WordCharacterClassifier,
    line: &[u16],
    offset: i32,
) -> Option<IntlWordSegmentData> {
    usize::try_from(offset).ok().and_then(|offset| word_separators.find_next_intl_word_at_or_after_offset(line, offset))
}

fn is_lower_or_digit(code: Option<u16>) -> bool {
    code.is_some_and(|code| strings::is_lower_ascii_letter(code) || strings::is_ascii_digit(code))
}

fn is_upper(code: Option<u16>) -> bool {
    code.is_some_and(strings::is_upper_ascii_letter)
}

/// O conteúdo da linha em unidades UTF-16.
fn units_of(model: &dyn CursorSimpleModel, line_number: i32) -> Vec<u16> {
    model.get_line_content(line_number).units().to_vec()
}

/// `WordOperations`.
pub struct WordOperations;

impl WordOperations {
    fn create_word(word_type: WordType, next_char_class: WordCharacterClass, start: i32, end: i32) -> FindWordResult {
        FindWordResult { start, end, word_type, next_char_class }
    }

    fn create_intl_word(intl_word: &IntlWordSegmentData, next_char_class: WordCharacterClass) -> FindWordResult {
        FindWordResult {
            start: intl_word.index as i32,
            end: (intl_word.index + intl_word.segment.len()) as i32,
            word_type: WordType::Regular,
            next_char_class,
        }
    }

    fn find_previous_word_on_line(
        word_separators: &WordCharacterClassifier,
        model: &dyn CursorSimpleModel,
        position: Position,
    ) -> Option<FindWordResult> {
        let line_content = units_of(model, position.line_number);
        WordOperations::do_find_previous_word_on_line(&line_content, word_separators, position)
    }

    fn do_find_previous_word_on_line(
        line_content: &[u16],
        word_separators: &WordCharacterClassifier,
        position: Position,
    ) -> Option<FindWordResult> {
        let mut word_type = WordType::None;

        let previous_intl_word = find_prev_intl_word(word_separators, line_content, position.column - 2);

        let mut ch_index = position.column - 2;
        while ch_index >= 0 {
            let ch_class = class_at(word_separators, line_content, ch_index);

            if let Some(previous_intl_word) = &previous_intl_word
                && ch_index as i64 == previous_intl_word.index as i64
            {
                return Some(WordOperations::create_intl_word(previous_intl_word, ch_class));
            }

            match ch_class {
                WordCharacterClass::Regular => {
                    if word_type == WordType::Separator {
                        return Some(WordOperations::create_word(
                            word_type,
                            ch_class,
                            ch_index + 1,
                            WordOperations::find_end_of_word(line_content, word_separators, word_type, ch_index + 1),
                        ));
                    }
                    word_type = WordType::Regular;
                }
                WordCharacterClass::WordSeparator => {
                    if word_type == WordType::Regular {
                        return Some(WordOperations::create_word(
                            word_type,
                            ch_class,
                            ch_index + 1,
                            WordOperations::find_end_of_word(line_content, word_separators, word_type, ch_index + 1),
                        ));
                    }
                    word_type = WordType::Separator;
                }
                WordCharacterClass::Whitespace => {
                    if word_type != WordType::None {
                        return Some(WordOperations::create_word(
                            word_type,
                            ch_class,
                            ch_index + 1,
                            WordOperations::find_end_of_word(line_content, word_separators, word_type, ch_index + 1),
                        ));
                    }
                }
            }
            ch_index -= 1;
        }

        if word_type != WordType::None {
            return Some(WordOperations::create_word(
                word_type,
                WordCharacterClass::Whitespace,
                0,
                WordOperations::find_end_of_word(line_content, word_separators, word_type, 0),
            ));
        }

        None
    }

    fn find_end_of_word(
        line_content: &[u16],
        word_separators: &WordCharacterClassifier,
        word_type: WordType,
        start_index: i32,
    ) -> i32 {
        let next_intl_word = find_next_intl_word(word_separators, line_content, start_index);

        let len = line_content.len() as i32;
        let mut ch_index = start_index;
        while ch_index < len {
            let ch_class = class_at(word_separators, line_content, ch_index);

            if let Some(next_intl_word) = &next_intl_word
                && ch_index as i64 == next_intl_word.index as i64 + next_intl_word.segment.len() as i64
            {
                return ch_index;
            }

            if ch_class == WordCharacterClass::Whitespace {
                return ch_index;
            }
            if word_type == WordType::Regular && ch_class == WordCharacterClass::WordSeparator {
                return ch_index;
            }
            if word_type == WordType::Separator && ch_class == WordCharacterClass::Regular {
                return ch_index;
            }
            ch_index += 1;
        }
        len
    }

    fn find_next_word_on_line(
        word_separators: &WordCharacterClassifier,
        model: &dyn CursorSimpleModel,
        position: Position,
    ) -> Option<FindWordResult> {
        let line_content = units_of(model, position.line_number);
        WordOperations::do_find_next_word_on_line(&line_content, word_separators, position)
    }

    fn do_find_next_word_on_line(
        line_content: &[u16],
        word_separators: &WordCharacterClassifier,
        position: Position,
    ) -> Option<FindWordResult> {
        let mut word_type = WordType::None;
        let len = line_content.len() as i32;

        let next_intl_word = find_next_intl_word(word_separators, line_content, position.column - 1);

        let mut ch_index = position.column - 1;
        while ch_index < len {
            let ch_class = class_at(word_separators, line_content, ch_index);

            if let Some(next_intl_word) = &next_intl_word
                && ch_index as i64 == next_intl_word.index as i64
            {
                return Some(WordOperations::create_intl_word(next_intl_word, ch_class));
            }

            match ch_class {
                WordCharacterClass::Regular => {
                    if word_type == WordType::Separator {
                        return Some(WordOperations::create_word(
                            word_type,
                            ch_class,
                            WordOperations::find_start_of_word(line_content, word_separators, word_type, ch_index - 1),
                            ch_index,
                        ));
                    }
                    word_type = WordType::Regular;
                }
                WordCharacterClass::WordSeparator => {
                    if word_type == WordType::Regular {
                        return Some(WordOperations::create_word(
                            word_type,
                            ch_class,
                            WordOperations::find_start_of_word(line_content, word_separators, word_type, ch_index - 1),
                            ch_index,
                        ));
                    }
                    word_type = WordType::Separator;
                }
                WordCharacterClass::Whitespace => {
                    if word_type != WordType::None {
                        return Some(WordOperations::create_word(
                            word_type,
                            ch_class,
                            WordOperations::find_start_of_word(line_content, word_separators, word_type, ch_index - 1),
                            ch_index,
                        ));
                    }
                }
            }
            ch_index += 1;
        }

        if word_type != WordType::None {
            return Some(WordOperations::create_word(
                word_type,
                WordCharacterClass::Whitespace,
                WordOperations::find_start_of_word(line_content, word_separators, word_type, len - 1),
                len,
            ));
        }

        None
    }

    fn find_start_of_word(
        line_content: &[u16],
        word_separators: &WordCharacterClassifier,
        word_type: WordType,
        start_index: i32,
    ) -> i32 {
        let previous_intl_word = find_prev_intl_word(word_separators, line_content, start_index);

        let mut ch_index = start_index;
        while ch_index >= 0 {
            let ch_class = class_at(word_separators, line_content, ch_index);

            if let Some(previous_intl_word) = &previous_intl_word
                && ch_index as i64 == previous_intl_word.index as i64
            {
                return ch_index;
            }

            if ch_class == WordCharacterClass::Whitespace {
                return ch_index + 1;
            }
            if word_type == WordType::Regular && ch_class == WordCharacterClass::WordSeparator {
                return ch_index + 1;
            }
            if word_type == WordType::Separator && ch_class == WordCharacterClass::Regular {
                return ch_index + 1;
            }
            ch_index -= 1;
        }
        0
    }

    pub fn move_word_left(
        word_separators: &WordCharacterClassifier,
        model: &dyn CursorSimpleModel,
        position: Position,
        word_navigation_type: WordNavigationType,
        has_multicursor: bool,
    ) -> Position {
        let mut line_number = position.line_number;
        let mut column = position.column;

        if column == 1 && line_number > 1 {
            line_number -= 1;
            column = model.get_line_max_column(line_number);
        }

        let mut prev_word_on_line =
            WordOperations::find_previous_word_on_line(word_separators, model, Position::new(line_number, column));

        if word_navigation_type == WordNavigationType::WordStart {
            return Position::new(line_number, prev_word_on_line.map_or(1, |word| word.start + 1));
        }

        if word_navigation_type == WordNavigationType::WordStartFast {
            if !has_multicursor // evita que vários cursores parem em lugares diferentes ao mover para o início da palavra
                && let Some(prev) = prev_word_on_line
                && prev.word_type == WordType::Separator
                && prev.end - prev.start == 1
                && prev.next_char_class == WordCharacterClass::Regular
            {
                // Pula uma palavra feita de um único separador e seguida de um caractere regular.
                prev_word_on_line = WordOperations::find_previous_word_on_line(
                    word_separators,
                    model,
                    Position::new(line_number, prev.start + 1),
                );
            }

            return Position::new(line_number, prev_word_on_line.map_or(1, |word| word.start + 1));
        }

        if word_navigation_type == WordNavigationType::WordAccessibility {
            while let Some(prev) = prev_word_on_line
                && prev.word_type == WordType::Separator
            {
                // Pula as palavras feitas só de separadores.
                prev_word_on_line = WordOperations::find_previous_word_on_line(
                    word_separators,
                    model,
                    Position::new(line_number, prev.start + 1),
                );
            }

            return Position::new(line_number, prev_word_on_line.map_or(1, |word| word.start + 1));
        }

        // Para no fim das palavras.

        if let Some(prev) = prev_word_on_line
            && column <= prev.end + 1
        {
            prev_word_on_line = WordOperations::find_previous_word_on_line(
                word_separators,
                model,
                Position::new(line_number, prev.start + 1),
            );
        }

        Position::new(line_number, prev_word_on_line.map_or(1, |word| word.end + 1))
    }

    /// `_moveWordPartLeft`.
    pub fn move_word_part_left(model: &dyn CursorSimpleModel, position: Position) -> Position {
        let line_number = position.line_number;
        let max_column = model.get_line_max_column(line_number);

        if position.column == 1 {
            return if line_number > 1 {
                Position::new(line_number - 1, model.get_line_max_column(line_number - 1))
            } else {
                position
            };
        }

        let line_content = units_of(model, line_number);
        let mut column = position.column - 1;
        while column > 1 {
            let left = code_at(&line_content, column - 2);
            let right = code_at(&line_content, column - 1);

            if left == Some(char_code::Underline) && right != Some(char_code::Underline) {
                // snake_case_variables
                return Position::new(line_number, column);
            }

            if left == Some(char_code::Dash) && right != Some(char_code::Dash) {
                // kebab-case-variables
                return Position::new(line_number, column);
            }

            if is_lower_or_digit(left) && is_upper(right) {
                // camelCaseVariables
                return Position::new(line_number, column);
            }

            if is_upper(left) && is_upper(right) {
                // thisIsACamelCaseWithOneLetterWords
                if column + 1 < max_column {
                    let right_right = code_at(&line_content, column);
                    if is_lower_or_digit(right_right) {
                        return Position::new(line_number, column);
                    }
                }
            }
            column -= 1;
        }

        Position::new(line_number, 1)
    }

    pub fn move_word_right(
        word_separators: &WordCharacterClassifier,
        model: &dyn CursorSimpleModel,
        position: Position,
        word_navigation_type: WordNavigationType,
    ) -> Position {
        let mut line_number = position.line_number;
        let mut column = position.column;

        let mut moved_down = false;
        if column == model.get_line_max_column(line_number) && line_number < model.get_line_count() {
            moved_down = true;
            line_number += 1;
            column = 1;
        }

        let mut next_word_on_line =
            WordOperations::find_next_word_on_line(word_separators, model, Position::new(line_number, column));

        if word_navigation_type == WordNavigationType::WordEnd {
            if let Some(next) = next_word_on_line
                && next.word_type == WordType::Separator
                && next.end - next.start == 1
                && next.next_char_class == WordCharacterClass::Regular
            {
                // Pula uma palavra feita de um único separador e seguida de um caractere regular.
                next_word_on_line = WordOperations::find_next_word_on_line(
                    word_separators,
                    model,
                    Position::new(line_number, next.end + 1),
                );
            }
            column = match next_word_on_line {
                Some(next) => next.end + 1,
                None => model.get_line_max_column(line_number),
            };
        } else if word_navigation_type == WordNavigationType::WordAccessibility {
            if moved_down {
                // Ao ir para a próxima linha, finge que o cursor está antes do primeiro caractere. É preciso
                // quando a primeira palavra começa na primeira coluna: para não perdê-la, começa-se antes.
                column = 0;
            }

            while let Some(next) = next_word_on_line
                && (next.word_type == WordType::Separator || next.start + 1 <= column)
            {
                // Pula uma palavra feita de um único separador, e também a palavra que começa antes da posição
                // do cursor, para garantir que o cursor avance pelo menos 1 caractere.
                next_word_on_line = WordOperations::find_next_word_on_line(
                    word_separators,
                    model,
                    Position::new(line_number, next.end + 1),
                );
            }

            column = match next_word_on_line {
                Some(next) => next.start + 1,
                None => model.get_line_max_column(line_number),
            };
        } else {
            if let Some(next) = next_word_on_line
                && !moved_down
                && column > next.start
            {
                next_word_on_line = WordOperations::find_next_word_on_line(
                    word_separators,
                    model,
                    Position::new(line_number, next.end + 1),
                );
            }
            column = match next_word_on_line {
                Some(next) => next.start + 1,
                None => model.get_line_max_column(line_number),
            };
        }

        Position::new(line_number, column)
    }

    /// `_moveWordPartRight`.
    pub fn move_word_part_right(model: &dyn CursorSimpleModel, position: Position) -> Position {
        let line_number = position.line_number;
        let max_column = model.get_line_max_column(line_number);

        if position.column == max_column {
            return if line_number < model.get_line_count() {
                Position::new(line_number + 1, 1)
            } else {
                position
            };
        }

        let line_content = units_of(model, line_number);
        let mut column = position.column + 1;
        while column < max_column {
            let left = code_at(&line_content, column - 2);
            let right = code_at(&line_content, column - 1);

            if left != Some(char_code::Underline) && right == Some(char_code::Underline) {
                // snake_case_variables
                return Position::new(line_number, column);
            }

            if left != Some(char_code::Dash) && right == Some(char_code::Dash) {
                // kebab-case-variables
                return Position::new(line_number, column);
            }

            if is_lower_or_digit(left) && is_upper(right) {
                // camelCaseVariables
                return Position::new(line_number, column);
            }

            if is_upper(left) && is_upper(right) {
                // thisIsACamelCaseWithOneLetterWords
                if column + 1 < max_column {
                    let right_right = code_at(&line_content, column);
                    if is_lower_or_digit(right_right) {
                        return Position::new(line_number, column);
                    }
                }
            }
            column += 1;
        }

        Position::new(line_number, max_column)
    }

    /// `_deleteWordLeftWhitespace`.
    pub fn delete_word_left_whitespace(model: &dyn CursorSimpleModel, position: Position) -> Option<Range> {
        let line_content = units_of(model, position.line_number);
        let start_index = position.column - 2;
        let last_non_whitespace = strings::last_non_whitespace_index(&line_content, i64::from(start_index))
            .map_or(-1, |index| index as i32);
        if last_non_whitespace + 1 < start_index {
            return Some(Range::new(position.line_number, last_non_whitespace + 2, position.line_number, position.column));
        }
        None
    }

    pub fn delete_word_left(ctx: &DeleteWordContext, word_navigation_type: WordNavigationType) -> Option<Range> {
        let word_separators = ctx.word_separators;
        let model: &dyn CursorSimpleModel = ctx.model;
        let selection = ctx.selection;
        let whitespace_heuristics = ctx.whitespace_heuristics;

        if !selection.is_empty() {
            return Some(*selection);
        }

        if DeleteOperations::is_auto_closing_pair_delete(
            ctx.auto_closing_delete,
            ctx.auto_closing_brackets,
            ctx.auto_closing_quotes,
            &ctx.auto_closing_pairs.auto_closing_pairs_open_by_end,
            model,
            &[ctx.selection],
            ctx.auto_closed_characters,
        ) {
            let position = ctx.selection.get_position();
            return Some(Range::new(position.line_number, position.column - 1, position.line_number, position.column + 1));
        }

        let position = Position::new(selection.position_line_number, selection.position_column);

        let mut line_number = position.line_number;
        let mut column = position.column;

        if line_number == 1 && column == 1 {
            // Ignora o apagar no começo do arquivo.
            return None;
        }

        if whitespace_heuristics && let Some(range) = WordOperations::delete_word_left_whitespace(model, position) {
            return Some(range);
        }

        let mut prev_word_on_line = WordOperations::find_previous_word_on_line(word_separators, model, position);

        if word_navigation_type == WordNavigationType::WordStart {
            match prev_word_on_line {
                Some(prev) => column = prev.start + 1,
                None => {
                    if column > 1 {
                        column = 1;
                    } else {
                        line_number -= 1;
                        column = model.get_line_max_column(line_number);
                    }
                }
            }
        } else {
            if let Some(prev) = prev_word_on_line
                && column <= prev.end + 1
            {
                prev_word_on_line = WordOperations::find_previous_word_on_line(
                    word_separators,
                    model,
                    Position::new(line_number, prev.start + 1),
                );
            }
            match prev_word_on_line {
                Some(prev) => column = prev.end + 1,
                None => {
                    if column > 1 {
                        column = 1;
                    } else {
                        line_number -= 1;
                        column = model.get_line_max_column(line_number);
                    }
                }
            }
        }

        Some(Range::new(line_number, column, position.line_number, position.column))
    }

    pub fn delete_inside_word(
        word_separators: &WordCharacterClassifier,
        model: &TextModel,
        selection: Selection,
        only_word: bool,
    ) -> Range {
        if !selection.is_empty() {
            return *selection;
        }

        let position = Position::new(selection.position_line_number, selection.position_column);
        let model: &dyn CursorSimpleModel = model;

        if let Some(range) = WordOperations::delete_inside_word_whitespace(model, position) {
            return range;
        }

        WordOperations::delete_inside_word_determine_delete_range(word_separators, model, position, only_word)
    }

    fn char_at_is_whitespace(line: &[u16], index: i32) -> bool {
        matches!(code_at(line, index), Some(char_code::Space | char_code::Tab))
    }

    fn delete_inside_word_whitespace(model: &dyn CursorSimpleModel, position: Position) -> Option<Range> {
        let line_content = units_of(model, position.line_number);
        let line_content_length = line_content.len() as i32;

        if line_content_length == 0 {
            // Linha vazia.
            return None;
        }

        let mut left_index = (position.column - 2).max(0);
        if !WordOperations::char_at_is_whitespace(&line_content, left_index) {
            // Encosta num caractere que não é espaço à esquerda.
            return None;
        }

        let mut right_index = (position.column - 1).min(line_content_length - 1);
        if !WordOperations::char_at_is_whitespace(&line_content, right_index) {
            // Encosta num caractere que não é espaço à direita.
            return None;
        }

        // Anda sobre o espaço à esquerda.
        while left_index > 0 && WordOperations::char_at_is_whitespace(&line_content, left_index - 1) {
            left_index -= 1;
        }

        // Anda sobre o espaço à direita.
        while right_index + 1 < line_content_length
            && WordOperations::char_at_is_whitespace(&line_content, right_index + 1)
        {
            right_index += 1;
        }

        Some(Range::new(position.line_number, left_index + 1, position.line_number, right_index + 2))
    }

    fn delete_inside_word_determine_delete_range(
        word_separators: &WordCharacterClassifier,
        model: &dyn CursorSimpleModel,
        position: Position,
        only_word: bool,
    ) -> Range {
        let line_content = units_of(model, position.line_number);
        let line_length = line_content.len() as i32;
        if line_length == 0 {
            // Linha vazia.
            if position.line_number > 1 {
                return Range::new(
                    position.line_number - 1,
                    model.get_line_max_column(position.line_number - 1),
                    position.line_number,
                    1,
                );
            } else if position.line_number < model.get_line_count() {
                return Range::new(position.line_number, 1, position.line_number + 1, 1);
            } else {
                // Modelo vazio.
                return Range::new(position.line_number, 1, position.line_number, 1);
            }
        }

        let touches_word = |word: &FindWordResult| word.start + 1 <= position.column && position.column <= word.end + 1;
        let create_range_with_position = |start_column: i32, end_column: i32| {
            let start_column = start_column.min(position.column);
            let end_column = end_column.max(position.column);
            Range::new(position.line_number, start_column, position.line_number, end_column)
        };
        let delete_word_and_adjacent_whitespace = |word: &FindWordResult| {
            let mut start_column = word.start + 1;
            let mut end_column = word.end + 1;
            if only_word {
                return create_range_with_position(start_column, end_column);
            }
            let mut expanded_to_the_right = false;
            while end_column - 1 < line_length && WordOperations::char_at_is_whitespace(&line_content, end_column - 1) {
                expanded_to_the_right = true;
                end_column += 1;
            }
            if !expanded_to_the_right {
                while start_column > 1 && WordOperations::char_at_is_whitespace(&line_content, start_column - 2) {
                    start_column -= 1;
                }
            }
            create_range_with_position(start_column, end_column)
        };

        let prev_word_on_line = WordOperations::find_previous_word_on_line(word_separators, model, position);
        if let Some(prev) = &prev_word_on_line
            && touches_word(prev)
        {
            return delete_word_and_adjacent_whitespace(prev);
        }
        let next_word_on_line = WordOperations::find_next_word_on_line(word_separators, model, position);
        if let Some(next) = &next_word_on_line
            && touches_word(next)
        {
            return delete_word_and_adjacent_whitespace(next);
        }
        if let (Some(prev), Some(next)) = (&prev_word_on_line, &next_word_on_line) {
            return create_range_with_position(prev.end + 1, next.start + 1);
        }
        if let Some(prev) = &prev_word_on_line {
            return create_range_with_position(prev.start + 1, prev.end + 1);
        }
        if let Some(next) = &next_word_on_line {
            return create_range_with_position(next.start + 1, next.end + 1);
        }

        create_range_with_position(1, line_length + 1)
    }

    /// `_deleteWordPartLeft`.
    pub fn delete_word_part_left(model: &dyn CursorSimpleModel, selection: Selection) -> Range {
        if !selection.is_empty() {
            return *selection;
        }

        let pos = selection.get_position();
        let to_position = WordOperations::move_word_part_left(model, pos);
        Range::new(pos.line_number, pos.column, to_position.line_number, to_position.column)
    }

    fn find_first_non_whitespace_char(line: &[u16], start_index: i32) -> i32 {
        let len = line.len() as i32;
        let mut ch_index = start_index;
        while ch_index < len {
            if !matches!(code_at(line, ch_index), Some(char_code::Space | char_code::Tab)) {
                return ch_index;
            }
            ch_index += 1;
        }
        len
    }

    /// `_deleteWordRightWhitespace`.
    pub fn delete_word_right_whitespace(model: &dyn CursorSimpleModel, position: Position) -> Option<Range> {
        let line_content = units_of(model, position.line_number);
        let start_index = position.column - 1;
        let first_non_whitespace = WordOperations::find_first_non_whitespace_char(&line_content, start_index);
        if start_index < first_non_whitespace {
            // Achou.
            return Some(Range::new(position.line_number, position.column, position.line_number, first_non_whitespace + 1));
        }
        None
    }

    pub fn delete_word_right(ctx: &DeleteWordContext, word_navigation_type: WordNavigationType) -> Option<Range> {
        let word_separators = ctx.word_separators;
        let model: &dyn CursorSimpleModel = ctx.model;
        let selection = ctx.selection;
        let whitespace_heuristics = ctx.whitespace_heuristics;

        if !selection.is_empty() {
            return Some(*selection);
        }

        let position = Position::new(selection.position_line_number, selection.position_column);

        let mut line_number = position.line_number;
        let mut column = position.column;

        let line_count = model.get_line_count();
        let max_column = model.get_line_max_column(line_number);
        if line_number == line_count && column == max_column {
            // Ignora o apagar no fim do arquivo.
            return None;
        }

        if whitespace_heuristics && let Some(range) = WordOperations::delete_word_right_whitespace(model, position) {
            return Some(range);
        }

        let mut next_word_on_line = WordOperations::find_next_word_on_line(word_separators, model, position);

        if word_navigation_type == WordNavigationType::WordEnd {
            if let Some(next) = next_word_on_line {
                column = next.end + 1;
            } else if column < max_column || line_number == line_count {
                column = max_column;
            } else {
                line_number += 1;
                next_word_on_line =
                    WordOperations::find_next_word_on_line(word_separators, model, Position::new(line_number, 1));
                column = match next_word_on_line {
                    Some(next) => next.start + 1,
                    None => model.get_line_max_column(line_number),
                };
            }
        } else {
            if let Some(next) = next_word_on_line
                && column > next.start
            {
                next_word_on_line = WordOperations::find_next_word_on_line(
                    word_separators,
                    model,
                    Position::new(line_number, next.end + 1),
                );
            }
            if let Some(next) = next_word_on_line {
                column = next.start + 1;
            } else if column < max_column || line_number == line_count {
                column = max_column;
            } else {
                line_number += 1;
                next_word_on_line =
                    WordOperations::find_next_word_on_line(word_separators, model, Position::new(line_number, 1));
                column = match next_word_on_line {
                    Some(next) => next.start + 1,
                    None => model.get_line_max_column(line_number),
                };
            }
        }

        Some(Range::new(line_number, column, position.line_number, position.column))
    }

    /// `_deleteWordPartRight`.
    pub fn delete_word_part_right(model: &dyn CursorSimpleModel, selection: Selection) -> Range {
        if !selection.is_empty() {
            return *selection;
        }

        let pos = selection.get_position();
        let to_position = WordOperations::move_word_part_right(model, pos);
        Range::new(pos.line_number, pos.column, to_position.line_number, to_position.column)
    }

    fn create_word_at_position(model: &TextModel, line_number: i32, word: &FindWordResult) -> WordAtPosition {
        let range = Range::new(line_number, word.start + 1, line_number, word.end + 1);
        WordAtPosition {
            word: model.get_value_in_range(&range),
            start_column: range.start_column,
            end_column: range.end_column,
        }
    }

    pub fn get_word_at_position(
        model: &TextModel,
        word_separators: &[u16],
        intl_segmenter_locales: &[String],
        position: Position,
    ) -> Option<WordAtPosition> {
        let word_separators = get_map_for_word_separators(word_separators, intl_segmenter_locales);
        let simple: &dyn CursorSimpleModel = model;
        let prev_word = WordOperations::find_previous_word_on_line(&word_separators, simple, position);
        if let Some(prev) = &prev_word
            && prev.word_type == WordType::Regular
            && prev.start <= position.column - 1
            && position.column - 1 <= prev.end
        {
            return Some(WordOperations::create_word_at_position(model, position.line_number, prev));
        }
        let next_word = WordOperations::find_next_word_on_line(&word_separators, simple, position);
        if let Some(next) = &next_word
            && next.word_type == WordType::Regular
            && next.start <= position.column - 1
            && position.column - 1 <= next.end
        {
            return Some(WordOperations::create_word_at_position(model, position.line_number, next));
        }
        None
    }

    pub fn word(
        config: &CursorConfiguration,
        model: &dyn CursorSimpleModel,
        cursor: &SingleCursorState,
        in_selection_mode: bool,
        position: Position,
    ) -> SingleCursorState {
        let separators: Vec<u16> = config.word_separators.encode_utf16().collect();
        let word_separators = get_map_for_word_separators(&separators, &config.word_segmenter_locales);
        let prev_word = WordOperations::find_previous_word_on_line(&word_separators, model, position);
        let next_word = WordOperations::find_next_word_on_line(&word_separators, model, position);

        if !in_selection_mode {
            // Entrando na seleção por palavra pela primeira vez.
            let start_column;
            let end_column;

            if let Some(prev) = prev_word
                && prev.word_type == WordType::Regular
                && prev.start <= position.column - 1
                && position.column - 1 <= prev.end
            {
                // isTouchingPrevWord (palavra regular)
                start_column = prev.start + 1;
                end_column = prev.end + 1;
            } else if let Some(prev) = prev_word
                && prev.word_type == WordType::Separator
                && prev.start <= position.column - 1
                && position.column - 1 < prev.end
            {
                // isTouchingPrevWord (palavra de separadores): checagem mais estrita, sem a borda do fim.
                start_column = prev.start + 1;
                end_column = prev.end + 1;
            } else if let Some(next) = next_word
                && next.word_type == WordType::Regular
                && next.start <= position.column - 1
                && position.column - 1 <= next.end
            {
                // isTouchingNextWord (palavra regular)
                start_column = next.start + 1;
                end_column = next.end + 1;
            } else if let Some(next) = next_word
                && next.word_type == WordType::Separator
                && next.start <= position.column - 1
                && position.column - 1 < next.end
            {
                // isTouchingNextWord (palavra de separadores): checagem mais estrita, sem a borda do fim.
                start_column = next.start + 1;
                end_column = next.end + 1;
            } else {
                start_column = prev_word.map_or(1, |prev| prev.end + 1);
                end_column = match next_word {
                    Some(next) => next.start + 1,
                    None => model.get_line_max_column(position.line_number),
                };
            }

            return SingleCursorState::new(
                Range::new(position.line_number, start_column, position.line_number, end_column),
                SelectionStartKind::Word,
                0,
                Position::new(position.line_number, end_column),
                0,
            );
        }

        let start_column;
        let end_column;

        if let Some(prev) = prev_word
            && prev.word_type == WordType::Regular
            && prev.start < position.column - 1
            && position.column - 1 < prev.end
        {
            // isInsidePrevWord (palavra regular)
            start_column = prev.start + 1;
            end_column = prev.end + 1;
        } else if let Some(next) = next_word
            && next.word_type == WordType::Regular
            && next.start < position.column - 1
            && position.column - 1 < next.end
        {
            // isInsideNextWord (palavra regular)
            start_column = next.start + 1;
            end_column = next.end + 1;
        } else {
            start_column = position.column;
            end_column = position.column;
        }

        let line_number = position.line_number;
        let mut column;
        if cursor.selection_start.contains_position(&position) {
            column = cursor.selection_start.end_column;
        } else if position.is_before_or_equal(&cursor.selection_start.get_start_position()) {
            column = start_column;
            let possible_position = Position::new(line_number, column);
            if cursor.selection_start.contains_position(&possible_position) {
                column = cursor.selection_start.end_column;
            }
        } else {
            column = end_column;
            let possible_position = Position::new(line_number, column);
            if cursor.selection_start.contains_position(&possible_position) {
                column = cursor.selection_start.start_column;
            }
        }

        cursor.move_to(true, line_number, column, 0)
    }
}

/// `WordPartOperations`.
pub struct WordPartOperations;

impl WordPartOperations {
    pub fn delete_word_part_left(ctx: &DeleteWordContext) -> Option<Range> {
        let model: &dyn CursorSimpleModel = ctx.model;
        // `enforceDefined`: descarta os `null`.
        let mut candidates: Vec<Range> = [
            WordOperations::delete_word_left(ctx, WordNavigationType::WordStart),
            WordOperations::delete_word_left(ctx, WordNavigationType::WordEnd),
            Some(WordOperations::delete_word_part_left(model, ctx.selection)),
        ]
        .into_iter()
        .flatten()
        .collect();
        candidates.sort_by(|a, b| Range::compare_ranges_using_ends(a, b).cmp(&0));
        candidates.get(2).copied()
    }

    pub fn delete_word_part_right(ctx: &DeleteWordContext) -> Range {
        let model: &dyn CursorSimpleModel = ctx.model;
        let mut candidates: Vec<Range> = [
            WordOperations::delete_word_right(ctx, WordNavigationType::WordStart),
            WordOperations::delete_word_right(ctx, WordNavigationType::WordEnd),
            Some(WordOperations::delete_word_part_right(model, ctx.selection)),
        ]
        .into_iter()
        .flatten()
        .collect();
        candidates.sort_by(|a, b| Range::compare_ranges_using_starts(Some(a), Some(b)).cmp(&0));
        candidates[0]
    }

    pub fn move_word_part_left(
        word_separators: &WordCharacterClassifier,
        model: &dyn CursorSimpleModel,
        position: Position,
        has_multicursor: bool,
    ) -> Position {
        let mut candidates = [
            WordOperations::move_word_left(word_separators, model, position, WordNavigationType::WordStart, has_multicursor),
            WordOperations::move_word_left(word_separators, model, position, WordNavigationType::WordEnd, has_multicursor),
            WordOperations::move_word_part_left(model, position),
        ];
        candidates.sort_by(|a, b| Position::compare(a, b).cmp(&0));
        candidates[2]
    }

    pub fn move_word_part_right(
        word_separators: &WordCharacterClassifier,
        model: &dyn CursorSimpleModel,
        position: Position,
    ) -> Position {
        let mut candidates = [
            WordOperations::move_word_right(word_separators, model, position, WordNavigationType::WordStart),
            WordOperations::move_word_right(word_separators, model, position, WordNavigationType::WordEnd),
            WordOperations::move_word_part_right(model, position),
        ];
        candidates.sort_by(|a, b| Position::compare(a, b).cmp(&0));
        candidates[0]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::PositionAffinity;
    use crate::word_helper::USUAL_WORD_SEPARATORS;
    use je_base::JsString;

    /// Modelo simples de teste: só as linhas.
    struct TestModel {
        lines: Vec<&'static str>,
    }

    impl CursorSimpleModel for TestModel {
        fn get_line_count(&self) -> i32 {
            self.lines.len() as i32
        }

        fn get_line_content(&self, line_number: i32) -> JsString {
            JsString::from(self.lines[line_number as usize - 1])
        }

        fn get_line_min_column(&self, _line_number: i32) -> i32 {
            1
        }

        fn get_line_max_column(&self, line_number: i32) -> i32 {
            self.get_line_content(line_number).len() as i32 + 1
        }

        fn get_line_first_non_whitespace_column(&self, _line_number: i32) -> i32 {
            0
        }

        fn get_line_last_non_whitespace_column(&self, _line_number: i32) -> i32 {
            0
        }

        fn normalize_position(&self, position: Position, _affinity: PositionAffinity) -> Position {
            position
        }

        fn get_line_indent_column(&self, line_number: i32) -> i32 {
            self.get_line_max_column(line_number)
        }
    }

    fn separators() -> std::rc::Rc<WordCharacterClassifier> {
        let units: Vec<u16> = USUAL_WORD_SEPARATORS.encode_utf16().collect();
        get_map_for_word_separators(&units, &[])
    }

    #[test]
    fn word_part_right_stops_at_camel_case_humps() {
        let model = TestModel { lines: vec!["camelCaseVar"] };
        assert_eq!(WordOperations::move_word_part_right(&model, Position::new(1, 1)), Position::new(1, 6));
        assert_eq!(WordOperations::move_word_part_right(&model, Position::new(1, 6)), Position::new(1, 10));
        assert_eq!(WordOperations::move_word_part_right(&model, Position::new(1, 10)), Position::new(1, 13));
    }

    #[test]
    fn word_part_left_stops_at_camel_case_humps() {
        let model = TestModel { lines: vec!["camelCaseVar"] };
        assert_eq!(WordOperations::move_word_part_left(&model, Position::new(1, 13)), Position::new(1, 10));
    }

    #[test]
    fn word_part_moves_over_snake_case() {
        let model = TestModel { lines: vec!["snake_case"] };
        assert_eq!(WordOperations::move_word_part_right(&model, Position::new(1, 1)), Position::new(1, 6));
        assert_eq!(WordOperations::move_word_part_right(&model, Position::new(1, 6)), Position::new(1, 11));
        assert_eq!(WordOperations::move_word_part_left(&model, Position::new(1, 11)), Position::new(1, 7));
    }

    #[test]
    fn word_part_moves_across_lines_at_the_edges() {
        let model = TestModel { lines: vec!["ab", "cd"] };
        assert_eq!(WordOperations::move_word_part_left(&model, Position::new(2, 1)), Position::new(1, 3));
        assert_eq!(WordOperations::move_word_part_right(&model, Position::new(1, 3)), Position::new(2, 1));
        assert_eq!(WordOperations::move_word_part_left(&model, Position::new(1, 1)), Position::new(1, 1));
        assert_eq!(WordOperations::move_word_part_right(&model, Position::new(2, 3)), Position::new(2, 3));
    }

    #[test]
    fn move_word_left_word_start() {
        let model = TestModel { lines: vec!["foo bar"] };
        let position =
            WordOperations::move_word_left(&separators(), &model, Position::new(1, 8), WordNavigationType::WordStart, false);
        assert_eq!(position, Position::new(1, 5));
    }

    #[test]
    fn move_word_left_goes_to_the_previous_line_at_the_start() {
        let model = TestModel { lines: vec!["foo", "bar"] };
        let position =
            WordOperations::move_word_left(&separators(), &model, Position::new(2, 1), WordNavigationType::WordStart, false);
        assert_eq!(position, Position::new(1, 1));
    }

    #[test]
    fn move_word_right_word_start_and_word_end() {
        let model = TestModel { lines: vec!["foo bar"] };
        let start = WordOperations::move_word_right(&separators(), &model, Position::new(1, 1), WordNavigationType::WordStart);
        assert_eq!(start, Position::new(1, 5));
        let end = WordOperations::move_word_right(&separators(), &model, Position::new(1, 1), WordNavigationType::WordEnd);
        assert_eq!(end, Position::new(1, 4));
    }

    #[test]
    fn move_word_right_word_end_stops_before_a_separator() {
        let model = TestModel { lines: vec!["a.b"] };
        let end = WordOperations::move_word_right(&separators(), &model, Position::new(1, 1), WordNavigationType::WordEnd);
        assert_eq!(end, Position::new(1, 2));
    }

    #[test]
    fn delete_word_whitespace_heuristics() {
        let model = TestModel { lines: vec!["ab   cd"] };
        // À esquerda de (1,6): só espaços entre o `b` e o cursor.
        assert_eq!(WordOperations::delete_word_left_whitespace(&model, Position::new(1, 6)), Some(Range::new(1, 3, 1, 6)));
        // Encostado num caractere que não é espaço: nada.
        assert_eq!(WordOperations::delete_word_left_whitespace(&model, Position::new(1, 3)), None);
        // À direita de (1,3): os três espaços até o `c`.
        assert_eq!(WordOperations::delete_word_right_whitespace(&model, Position::new(1, 3)), Some(Range::new(1, 3, 1, 6)));
        assert_eq!(WordOperations::delete_word_right_whitespace(&model, Position::new(1, 1)), None);
    }

    #[test]
    fn delete_inside_word_whitespace_takes_the_whole_run() {
        let model = TestModel { lines: vec!["ab   cd"] };
        assert_eq!(
            WordOperations::delete_inside_word_whitespace(&model, Position::new(1, 5)),
            Some(Range::new(1, 3, 1, 6))
        );
        assert_eq!(WordOperations::delete_inside_word_whitespace(&model, Position::new(1, 2)), None);
    }

    #[test]
    fn delete_inside_word_range_on_empty_lines() {
        let model = TestModel { lines: vec!["", "x", ""] };
        let ws = separators();
        assert_eq!(
            WordOperations::delete_inside_word_determine_delete_range(&ws, &model, Position::new(1, 1), false),
            Range::new(1, 1, 2, 1)
        );
        assert_eq!(
            WordOperations::delete_inside_word_determine_delete_range(&ws, &model, Position::new(3, 1), false),
            Range::new(2, 2, 3, 1)
        );
    }

    #[test]
    fn delete_inside_word_range_takes_the_word_and_the_trailing_whitespace() {
        let model = TestModel { lines: vec!["foo bar"] };
        let ws = separators();
        // Dentro de `foo`: a palavra e o espaço à direita.
        assert_eq!(
            WordOperations::delete_inside_word_determine_delete_range(&ws, &model, Position::new(1, 2), false),
            Range::new(1, 1, 1, 5)
        );
        // Só a palavra.
        assert_eq!(
            WordOperations::delete_inside_word_determine_delete_range(&ws, &model, Position::new(1, 2), true),
            Range::new(1, 1, 1, 4)
        );
    }

    #[test]
    fn word_part_left_and_right_pick_the_farthest_candidate() {
        let model = TestModel { lines: vec!["camelCase"] };
        let ws = separators();
        assert_eq!(WordPartOperations::move_word_part_right(&ws, &model, Position::new(1, 1)), Position::new(1, 6));
        assert_eq!(WordPartOperations::move_word_part_left(&ws, &model, Position::new(1, 10), false), Position::new(1, 6));
    }
}
