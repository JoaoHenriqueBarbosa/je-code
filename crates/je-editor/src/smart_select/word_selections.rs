//! Porte de `src/vs/editor/contrib/smartSelect/browser/wordSelections.ts`: o provedor de intervalos de seleção por
//! palavra (`WordSelectionRangeProvider`), que sempre participa da expansão: a subpalavra (`foo` em `fooBar`, `foo_bar`
//! ou `foo-bar`), a palavra, a linha só de espaços e o documento inteiro.
//!
//! Divergência: o `word.charCodeAt(i)` do original devolve `NaN` fora da palavra (o cursor no fim dela), e `NaN` falha
//! em todas as comparações; aqui a unidade ausente é `0`, que também falha em todas (não é `_`, `-` nem letra ASCII).

use std::rc::Rc;

use je_base::cancellation::CancellationToken;
use je_base::char_code as cc;
use je_base::promise::Promise;
use je_base::strings::{is_lower_ascii_letter, is_upper_ascii_letter};

use crate::cursor_common::CursorSimpleModel;
use crate::language_providers::{ProviderResult, SelectionRange, SelectionRangeProvider};
use crate::position::Position;
use crate::range::Range;
use crate::text_model::TextModel;

/// `WordSelectionRangeProvider`.
pub struct WordSelectionRangeProvider {
    select_subwords: bool,
}

impl WordSelectionRangeProvider {
    /// `new WordSelectionRangeProvider(selectSubwords)`.
    pub fn new(select_subwords: bool) -> Self {
        Self { select_subwords }
    }

    fn add_in_word_ranges(bucket: &mut Vec<SelectionRange>, model: &TextModel, pos: Position) {
        let Some(obj) = model.get_word_at_position(pos) else {
            return;
        };

        let word = obj.word.units();
        let start_column = obj.start_column;
        let offset = i64::from(pos.column - start_column);
        let mut start = offset;
        let mut end = offset;
        let mut last_ch: u16 = 0;
        let char_code_at = |index: i64| -> u16 {
            usize::try_from(index).ok().and_then(|index| word.get(index).copied()).unwrap_or(0)
        };

        // LEFT anchor (start)
        while start >= 0 {
            let ch = char_code_at(start);
            if start != offset && (ch == cc::Underline || ch == cc::Dash) {
                // foo-bar OR foo_bar
                break;
            } else if is_lower_ascii_letter(ch) && is_upper_ascii_letter(last_ch) {
                // fooBar
                break;
            }
            last_ch = ch;
            start -= 1;
        }
        start += 1;

        // RIGHT anchor (end)
        while end < word.len() as i64 {
            let ch = char_code_at(end);
            if is_upper_ascii_letter(ch) && is_lower_ascii_letter(last_ch) {
                // fooBar
                break;
            } else if ch == cc::Underline || ch == cc::Dash {
                // foo-bar OR foo_bar
                break;
            }
            last_ch = ch;
            end += 1;
        }

        if start < end {
            bucket.push(SelectionRange {
                range: Range::new(pos.line_number, start_column + start as i32, pos.line_number, start_column + end as i32),
            });
        }
    }

    fn add_word_ranges(bucket: &mut Vec<SelectionRange>, model: &TextModel, pos: Position) {
        if let Some(word) = model.get_word_at_position(pos) {
            bucket.push(SelectionRange { range: Range::new(pos.line_number, word.start_column, pos.line_number, word.end_column) });
        }
    }

    fn add_whitespace_line(bucket: &mut Vec<SelectionRange>, model: &TextModel, pos: Position) {
        if model.get_line_length(pos.line_number) > 0
            && model.get_line_first_non_whitespace_column(pos.line_number) == 0
            && model.get_line_last_non_whitespace_column(pos.line_number) == 0
        {
            bucket.push(SelectionRange {
                range: Range::new(pos.line_number, 1, pos.line_number, model.get_line_max_column(pos.line_number)),
            });
        }
    }
}

impl Default for WordSelectionRangeProvider {
    /// `selectSubwords = true`.
    fn default() -> Self {
        Self::new(true)
    }
}

impl SelectionRangeProvider for WordSelectionRangeProvider {
    fn provide_selection_ranges(
        &self,
        model: &Rc<TextModel>,
        positions: &[Position],
        _token: &CancellationToken,
    ) -> ProviderResult<Vec<Vec<SelectionRange>>> {
        let mut result: Vec<Vec<SelectionRange>> = Vec::new();
        for &position in positions {
            let mut bucket: Vec<SelectionRange> = Vec::new();
            if self.select_subwords {
                Self::add_in_word_ranges(&mut bucket, model, position);
            }
            Self::add_word_ranges(&mut bucket, model, position);
            Self::add_whitespace_line(&mut bucket, model, position);
            bucket.push(SelectionRange { range: model.get_full_model_range() });
            result.push(bucket);
        }
        Promise::resolved(Some(result))
    }
}
