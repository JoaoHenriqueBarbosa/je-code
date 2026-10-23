//! Porte de `src/vs/editor/common/tokens/sparseTokensStore.ts`: os tokens esparsos (semânticos) de um
//! modelo de texto e a mesclagem deles com os tokens de uma linha.
//!
//! Decisões de tradução:
//! - `IRange` e `Range` são o mesmo tipo em Rust; `acceptEdit` recebe [`Range`].
//! - O `textModel?: ITextModel` de `set` vira `Option<&TextModel>`.
//! - O `Uint32Array` do resultado de `addSparseTokens` vira `Vec<u32>`; o `LineTokens` recebe o
//!   `ILanguageIdCodec` guardado no armazenamento.

use std::rc::Rc;

use je_base::arrays;

use crate::encoded_token_attributes::metadata_consts as mc;
use crate::languages::ILanguageIdCodec;
use crate::line_tokens::{IViewLineTokens, LineTokens};
use crate::range::Range;
use crate::sparse_multiline_tokens::SparseMultilineTokens;
use crate::text_model::TextModel;

/// `SparseTokensStore`: os tokens esparsos de um modelo de texto.
pub struct SparseTokensStore {
    pieces: Vec<SparseMultilineTokens>,
    is_complete: bool,
    language_id_codec: Rc<dyn ILanguageIdCodec>,
}

impl SparseTokensStore {
    pub fn new(language_id_codec: Rc<dyn ILanguageIdCodec>) -> Self {
        SparseTokensStore { pieces: Vec::new(), is_complete: false, language_id_codec }
    }

    pub fn flush(&mut self) {
        self.pieces = Vec::new();
        self.is_complete = false;
    }

    pub fn is_empty(&self) -> bool {
        self.pieces.is_empty()
    }

    pub fn set(&mut self, pieces: Option<Vec<SparseMultilineTokens>>, is_complete: bool, text_model: Option<&TextModel>) {
        self.pieces = pieces.unwrap_or_default();
        self.is_complete = is_complete;

        if let Some(text_model) = text_model {
            for p in &self.pieces {
                p.report_if_invalid(text_model);
            }
        }
    }

    pub fn set_partial(&mut self, range: &Range, pieces: Vec<SparseMultilineTokens>) -> Range {
        let mut range = *range;
        let original_range = range;
        if !pieces.is_empty() {
            let first_range = pieces[0].get_range();
            let last_range = pieces[pieces.len() - 1].get_range();
            let (Some(first_range), Some(last_range)) = (first_range, last_range) else {
                return original_range;
            };
            range = original_range.plus_range(&first_range).plus_range(&last_range);
        }

        let mut insert_position: Option<usize> = None;
        let mut i = 0;
        while i < self.pieces.len() {
            if self.pieces[i].end_line_number() < range.start_line_number {
                // this piece is before the range
                i += 1;
                continue;
            }

            if self.pieces[i].start_line_number() > range.end_line_number {
                // this piece is after the range, so mark the spot before this piece
                // as a good insertion position and stop looping
                insert_position = insert_position.or(Some(i));
                break;
            }

            // this piece might intersect with the range
            self.pieces[i].remove_tokens(&range);

            if self.pieces[i].is_empty() {
                // remove the piece if it became empty
                self.pieces.remove(i);
                continue;
            }

            if self.pieces[i].end_line_number() < range.start_line_number {
                // after removal, this piece is before the range
                i += 1;
                continue;
            }

            if self.pieces[i].start_line_number() > range.end_line_number {
                // after removal, this piece is after the range
                insert_position = insert_position.or(Some(i));
                i += 1;
                continue;
            }

            // after removal, this piece contains the range
            let (a, b) = self.pieces[i].split(&range);
            if a.is_empty() {
                // this piece is actually after the range
                insert_position = insert_position.or(Some(i));
                i += 1;
                continue;
            }
            if b.is_empty() {
                // this piece is actually before the range
                i += 1;
                continue;
            }
            self.pieces.splice(i..i + 1, [a, b]);
            i += 1;

            insert_position = insert_position.or(Some(i));
            i += 1;
        }

        let insert_position = insert_position.unwrap_or(self.pieces.len());

        if !pieces.is_empty() {
            self.pieces = arrays::array_insert(&self.pieces, insert_position as i64, &pieces);
        }

        range
    }

    pub fn is_complete(&self) -> bool {
        self.is_complete
    }

    pub fn add_sparse_tokens(&self, line_number: i32, a_tokens: LineTokens) -> LineTokens {
        if a_tokens.get_text_length() == 0 {
            // Don't do anything for empty lines
            return a_tokens;
        }

        let pieces = &self.pieces;

        if pieces.is_empty() {
            return a_tokens;
        }

        let piece_index = Self::find_first_piece_with_line(pieces, line_number);
        let Some(b_tokens) = pieces[piece_index].get_line_tokens(line_number) else {
            return a_tokens;
        };

        let a_len = a_tokens.get_count();
        let b_len = b_tokens.get_count();

        let mut a_index = 0;
        let mut result: Vec<u32> = Vec::new();
        let mut last_end_offset: i64 = 0;

        let mut emit_token = |end_offset: i64, metadata: u32| {
            if end_offset <= last_end_offset {
                return;
            }
            last_end_offset = end_offset;
            result.push(end_offset as u32);
            result.push(metadata);
        };

        let text_length = a_tokens.get_text_length() as i64;

        for b_index in 0..b_len {
            // bTokens is not validated yet, but aTokens is. We want to make sure that the LineTokens we return
            // are valid, so we clamp the ranges to ensure that.
            let b_start_character = (b_tokens.get_start_character(b_index) as i64).min(text_length);
            let b_end_character = (b_tokens.get_end_character(b_index) as i64).min(text_length);
            let b_metadata = b_tokens.get_metadata(b_index);

            let b_mask = (if b_metadata & mc::SEMANTIC_USE_ITALIC != 0 { mc::ITALIC_MASK } else { 0 })
                | (if b_metadata & mc::SEMANTIC_USE_BOLD != 0 { mc::BOLD_MASK } else { 0 })
                | (if b_metadata & mc::SEMANTIC_USE_UNDERLINE != 0 { mc::UNDERLINE_MASK } else { 0 })
                | (if b_metadata & mc::SEMANTIC_USE_STRIKETHROUGH != 0 { mc::STRIKETHROUGH_MASK } else { 0 })
                | (if b_metadata & mc::SEMANTIC_USE_FOREGROUND != 0 { mc::FOREGROUND_MASK } else { 0 })
                | (if b_metadata & mc::SEMANTIC_USE_BACKGROUND != 0 { mc::BACKGROUND_MASK } else { 0 });
            let a_mask = !b_mask;

            // push any token from `a` that is before `b`
            while a_index < a_len && a_tokens.get_end_offset(a_index) as i64 <= b_start_character {
                emit_token(a_tokens.get_end_offset(a_index) as i64, a_tokens.get_metadata(a_index));
                a_index += 1;
            }

            // push the token from `a` if it intersects the token from `b`
            if a_index < a_len && (a_tokens.get_start_offset(a_index) as i64) < b_start_character {
                emit_token(b_start_character, a_tokens.get_metadata(a_index));
            }

            // skip any tokens from `a` that are contained inside `b`
            while a_index < a_len && (a_tokens.get_end_offset(a_index) as i64) < b_end_character {
                emit_token(
                    a_tokens.get_end_offset(a_index) as i64,
                    (a_tokens.get_metadata(a_index) & a_mask) | (b_metadata & b_mask),
                );
                a_index += 1;
            }

            if a_index < a_len {
                emit_token(b_end_character, (a_tokens.get_metadata(a_index) & a_mask) | (b_metadata & b_mask));
                if a_tokens.get_end_offset(a_index) as i64 == b_end_character {
                    // `a` ends exactly at the same spot as `b`!
                    a_index += 1;
                }
            } else {
                let a_merge_index = a_index.saturating_sub(1).min(a_len - 1);

                // push the token from `b`
                emit_token(b_end_character, (a_tokens.get_metadata(a_merge_index) & a_mask) | (b_metadata & b_mask));
            }
        }

        // push the remaining tokens from `a`
        while a_index < a_len {
            emit_token(a_tokens.get_end_offset(a_index) as i64, a_tokens.get_metadata(a_index));
            a_index += 1;
        }

        LineTokens::new(result, a_tokens.get_line_content(), self.language_id_codec.clone())
    }

    fn find_first_piece_with_line(pieces: &[SparseMultilineTokens], line_number: i32) -> usize {
        let mut low: i64 = 0;
        let mut high: i64 = pieces.len() as i64 - 1;

        while low < high {
            let mut mid = low + (high - low) / 2;

            if pieces[mid as usize].end_line_number() < line_number {
                low = mid + 1;
            } else if pieces[mid as usize].start_line_number() > line_number {
                high = mid - 1;
            } else {
                while mid > low
                    && pieces[mid as usize - 1].start_line_number() <= line_number
                    && line_number <= pieces[mid as usize - 1].end_line_number()
                {
                    mid -= 1;
                }
                return mid as usize;
            }
        }

        low as usize
    }

    pub fn accept_edit(
        &mut self,
        range: &Range,
        eol_count: i32,
        first_line_length: i32,
        last_line_length: i32,
        first_char_code: u16,
    ) {
        let mut i = 0;
        while i < self.pieces.len() {
            self.pieces[i].accept_edit(range, eol_count, first_line_length, last_line_length, first_char_code);

            if self.pieces[i].is_empty() {
                // Remove empty pieces
                self.pieces.remove(i);
            } else {
                i += 1;
            }
        }
    }
}
