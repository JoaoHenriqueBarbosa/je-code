//! Porte de `src/vs/editor/common/tokens/contiguousTokensStore.ts`: o armazenamento dos tokens de todas as
//! linhas do modelo, que acompanha as edições.

use std::rc::Rc;

use crate::contiguous_multiline_tokens::ContiguousMultilineTokens;
use crate::contiguous_tokens_editing::{self as editing, RawLineTokens, TokenData};
use crate::encoded_token_attributes::{
    TokenMetadata, color_id, font_style, metadata_consts as mc, standard_token_type,
};
use crate::languages::ILanguageIdCodec;
use crate::line_tokens::LineTokens;
use crate::range::Range;
use crate::text_model::TextModel;
use je_base::JsString;

/// Um intervalo de linhas que mudou (`{ fromLineNumber, toLineNumber }`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChangedLines {
    pub from_line_number: i32,
    pub to_line_number: i32,
}

/// `ContiguousTokensStore`.
pub struct ContiguousTokensStore {
    line_tokens: Vec<RawLineTokens>,
    language_id_codec: Rc<dyn ILanguageIdCodec>,
}

impl ContiguousTokensStore {
    pub fn new(language_id_codec: Rc<dyn ILanguageIdCodec>) -> Self {
        Self { line_tokens: Vec::new(), language_id_codec }
    }

    pub fn flush(&mut self) {
        self.line_tokens.clear();
    }

    pub fn has_tokens(&self) -> bool {
        !self.line_tokens.is_empty()
    }

    /// `getTokens(topLevelLanguageId, lineIndex, lineText)`.
    pub fn get_tokens(&self, top_level_language_id: &str, line_index: usize, line_text: JsString) -> LineTokens {
        if let Some(Some(TokenData::Array(raw))) = self.line_tokens.get(line_index) {
            return LineTokens::new(raw.clone(), line_text, self.language_id_codec.clone());
        }
        let line_tokens = vec![
            line_text.len() as u32,
            get_default_metadata(self.language_id_codec.encode_language_id(top_level_language_id)),
        ];
        LineTokens::new(line_tokens, line_text, self.language_id_codec.clone())
    }

    fn massage_tokens(top_level_language_id: u32, line_text_length: usize, tokens: RawLineTokens) -> TokenData {
        let tokens = tokens.map(TokenData::into_array);

        if line_text_length == 0 {
            let mut has_different_language_id = false;
            if let Some(tokens) = &tokens
                && tokens.len() > 1
            {
                has_different_language_id = TokenMetadata::get_language_id(tokens[1]) != top_level_language_id;
            }
            if !has_different_language_id {
                return TokenData::Empty;
            }
        }

        let Some(mut tokens) = tokens.filter(|t| !t.is_empty()) else {
            return TokenData::Array(vec![line_text_length as u32, get_default_metadata(top_level_language_id)]);
        };

        let last = tokens.len() - 2;
        tokens[last] = line_text_length as u32;
        TokenData::Array(tokens)
    }

    fn ensure_line(&mut self, line_index: usize) {
        if line_index >= self.line_tokens.len() {
            self.line_tokens.resize(line_index + 1, None);
        }
    }

    fn delete_lines(&mut self, start: usize, mut delete_count: usize) {
        if delete_count == 0 {
            return;
        }
        if start + delete_count > self.line_tokens.len() {
            delete_count = self.line_tokens.len().saturating_sub(start);
        }
        self.line_tokens.drain(start..start + delete_count);
    }

    fn insert_lines(&mut self, insert_index: usize, insert_count: usize) {
        if insert_count == 0 {
            return;
        }
        let insert_index = insert_index.min(self.line_tokens.len());
        self.line_tokens.splice(insert_index..insert_index, std::iter::repeat_n(None, insert_count));
    }

    /// `setTokens(topLevelLanguageId, lineIndex, lineTextLength, tokens, checkEquality)`.
    pub fn set_tokens(
        &mut self,
        top_level_language_id: &str,
        line_index: usize,
        line_text_length: usize,
        tokens: RawLineTokens,
        check_equality: bool,
    ) -> bool {
        let tokens = Self::massage_tokens(self.language_id_codec.encode_language_id(top_level_language_id), line_text_length, tokens);
        self.ensure_line(line_index);
        let old_tokens = self.line_tokens[line_index].replace(tokens.clone());

        if check_equality {
            return !Self::equals(old_tokens.as_ref(), Some(&tokens));
        }
        false
    }

    fn equals(a: Option<&TokenData>, b: Option<&TokenData>) -> bool {
        match (a, b) {
            (Some(a), Some(b)) => a.as_slice() == b.as_slice(),
            (None, None) => true,
            _ => false,
        }
    }

    // --- Editing

    /// `acceptEdit(range, eolCount, firstLineLength)`.
    pub fn accept_edit(&mut self, range: &Range, eol_count: usize, first_line_length: usize) {
        self.accept_delete_range(range);
        self.accept_insert_text(range.start_line_number, range.start_column, eol_count, first_line_length);
    }

    fn take(&mut self, index: usize) -> RawLineTokens {
        self.line_tokens[index].take()
    }

    fn accept_delete_range(&mut self, range: &Range) {
        let first_line_index = (range.start_line_number - 1) as usize;
        if first_line_index >= self.line_tokens.len() {
            return;
        }

        if range.start_line_number == range.end_line_number {
            if range.start_column == range.end_column {
                // Nothing to delete
                return;
            }
            let t = self.take(first_line_index);
            self.line_tokens[first_line_index] = editing::delete(t, range.start_column - 1, range.end_column - 1);
            return;
        }

        // We need to delete all lines in between the first and last line, and then join them
        let t = self.take(first_line_index);
        self.line_tokens[first_line_index] = editing::delete_ending(t, range.start_column - 1);

        let last_line_index = (range.end_line_number - 1) as usize;
        let mut last_line_tokens: RawLineTokens = None;
        if last_line_index < self.line_tokens.len() {
            let t = self.take(last_line_index);
            last_line_tokens = editing::delete_beginning(t, range.end_column - 1);
        }

        // Take remaining text on last line and append it to remaining text on first line
        let t = self.take(first_line_index);
        self.line_tokens[first_line_index] = editing::append(t, last_line_tokens);

        // Delete middle lines
        self.delete_lines(range.start_line_number as usize, (range.end_line_number - range.start_line_number) as usize);
    }

    fn accept_insert_text(&mut self, line_number: i32, column: i32, eol_count: usize, first_line_length: usize) {
        // Here we depend on the fact that `insertText` is called only after `deleteRange`
        if eol_count == 0 && first_line_length == 0 {
            // Nothing to insert
            return;
        }

        let line_index = (line_number - 1) as usize;
        if line_index >= self.line_tokens.len() {
            return;
        }

        if eol_count == 0 {
            // Inserting text on one line
            let t = self.take(line_index);
            self.line_tokens[line_index] = editing::insert(t, column - 1, first_line_length as i32);
            return;
        }

        // Inserting text on more than one line
        let t = self.take(line_index);
        self.line_tokens[line_index] = editing::delete_ending(t, column - 1);
        let t = self.take(line_index);
        self.line_tokens[line_index] = editing::insert(t, column - 1, first_line_length as i32);

        self.insert_lines(line_number as usize, eol_count);
    }

    /// `setMultilineTokens(tokens, textModel)`: devolve os intervalos de linhas que mudaram.
    pub fn set_multiline_tokens(&mut self, tokens: Vec<ContiguousMultilineTokens>, text_model: &TextModel) -> Vec<ChangedLines> {
        if tokens.is_empty() {
            return Vec::new();
        }
        let language_id = text_model.get_language_id();
        let mut ranges = Vec::new();

        for mut element in tokens {
            let mut min_changed_line_number = 0;
            let mut max_changed_line_number = 0;
            let mut has_change = false;
            for line_number in element.start_line_number()..=element.end_line_number() {
                let line_tokens = element.take_line_tokens(line_number);
                let line_length = text_model.get_line_length(line_number);
                if has_change {
                    self.set_tokens(&language_id, (line_number - 1) as usize, line_length, line_tokens, false);
                    max_changed_line_number = line_number;
                } else {
                    let line_has_change = self.set_tokens(&language_id, (line_number - 1) as usize, line_length, line_tokens, true);
                    if line_has_change {
                        has_change = true;
                        min_changed_line_number = line_number;
                        max_changed_line_number = line_number;
                    }
                }
            }
            if has_change {
                ranges.push(ChangedLines { from_line_number: min_changed_line_number, to_line_number: max_changed_line_number });
            }
        }
        ranges
    }
}

fn get_default_metadata(top_level_language_id: u32) -> u32 {
    (top_level_language_id << mc::LANGUAGEID_OFFSET)
        | (standard_token_type::OTHER << mc::TOKEN_TYPE_OFFSET)
        | (font_style::NONE << mc::FONT_STYLE_OFFSET)
        | (color_id::DEFAULT_FOREGROUND << mc::FOREGROUND_OFFSET)
        | (color_id::DEFAULT_BACKGROUND << mc::BACKGROUND_OFFSET)
        | mc::BALANCED_BRACKETS_MASK
}
