//! Porte de `src/vs/editor/common/tokens/contiguousMultilineTokens.ts` e de
//! `contiguousMultilineTokensBuilder.ts`: os tokens de um bloco contíguo de linhas e o construtor.
//!
//! A serialização para o web worker (`serialize`/`deserialize`) não foi portada: o porte tokeniza no mesmo
//! thread ou por mensagens de Rust, sem `Uint8Array`.

use crate::contiguous_tokens_editing::{self as editing, RawLineTokens, TokenData};
use crate::eol_counter::count_eol;
use crate::line_range::LineRange;
use crate::range::Range;

/// `ContiguousMultilineTokens`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ContiguousMultilineTokens {
    start_line_number: i32,
    tokens: Vec<RawLineTokens>,
}

impl ContiguousMultilineTokens {
    /// O construtor: `tokens` são os tokens de cada linha a partir de `start_line_number`.
    pub fn new(start_line_number: i32, tokens: Vec<Vec<u32>>) -> Self {
        Self { start_line_number, tokens: tokens.into_iter().map(|t| Some(TokenData::Array(t))).collect() }
    }

    pub fn start_line_number(&self) -> i32 {
        self.start_line_number
    }

    pub fn end_line_number(&self) -> i32 {
        self.start_line_number + self.tokens.len() as i32 - 1
    }

    pub fn get_line_range(&self) -> LineRange {
        LineRange::new(self.start_line_number, self.start_line_number + self.tokens.len() as i32)
    }

    /// `getLineTokens(lineNumber)`: `None` fora do bloco (o `undefined` do original) ou em linha sem tokens.
    pub fn get_line_tokens(&self, line_number: i32) -> Option<&TokenData> {
        let index = usize::try_from(line_number - self.start_line_number).ok()?;
        self.tokens.get(index)?.as_ref()
    }

    /// Tira os tokens de uma linha do bloco (o armazenamento os recebe e os edita no lugar, como no original).
    pub fn take_line_tokens(&mut self, line_number: i32) -> RawLineTokens {
        let index = usize::try_from(line_number - self.start_line_number).ok()?;
        self.tokens.get_mut(index)?.take()
    }

    pub fn append_line_tokens(&mut self, line_tokens: Vec<u32>) {
        self.tokens.push(Some(TokenData::Array(line_tokens)));
    }

    /// `applyEdit(range, text)`.
    pub fn apply_edit(&mut self, range: &Range, text: &[u16]) {
        let counted = count_eol(text);
        self.accept_delete_range(range);
        self.accept_insert_text(range.start_line_number, range.start_column, counted.eol_count, counted.first_line_length);
    }

    fn accept_delete_range(&mut self, range: &Range) {
        if range.start_line_number == range.end_line_number && range.start_column == range.end_column {
            // Nothing to delete
            return;
        }

        let first_line_index = range.start_line_number - self.start_line_number;
        let last_line_index = range.end_line_number - self.start_line_number;

        if last_line_index < 0 {
            // this deletion occurs entirely before this block, so we only need to adjust line numbers
            let deleted_lines_count = last_line_index - first_line_index;
            self.start_line_number -= deleted_lines_count;
            return;
        }

        if first_line_index >= self.tokens.len() as i32 {
            // this deletion occurs entirely after this block, so there is nothing to do
            return;
        }

        if first_line_index < 0 && last_line_index >= self.tokens.len() as i32 {
            // this deletion completely encompasses this block
            self.start_line_number = 0;
            self.tokens.clear();
            return;
        }

        if first_line_index == last_line_index {
            // a delete on a single line
            let i = first_line_index as usize;
            let t = self.tokens[i].take();
            self.tokens[i] = editing::delete(t, range.start_column - 1, range.end_column - 1);
            return;
        }

        if first_line_index >= 0 {
            // The first line survives
            let first = first_line_index as usize;
            let t = self.tokens[first].take();
            self.tokens[first] = editing::delete_ending(t, range.start_column - 1);

            if last_line_index < self.tokens.len() as i32 {
                // The last line survives
                let last = last_line_index as usize;
                let t = self.tokens[last].take();
                let last_line_tokens = editing::delete_beginning(t, range.end_column - 1);

                // Take remaining text on last line and append it to remaining text on first line
                let t = self.tokens[first].take();
                self.tokens[first] = editing::append(t, last_line_tokens);

                // Delete middle lines
                self.tokens.drain(first + 1..first + 1 + (last - first));
            } else {
                // The last line does not survive

                // Take remaining text on last line and append it to remaining text on first line
                let t = self.tokens[first].take();
                self.tokens[first] = editing::append(t, None);

                // Delete lines
                self.tokens.truncate(first + 1);
            }
        } else {
            // The first line does not survive

            let deleted_before = -first_line_index;
            self.start_line_number -= deleted_before;

            // Remove beginning from last line
            let last = last_line_index as usize;
            let t = self.tokens[last].take();
            self.tokens[last] = editing::delete_beginning(t, range.end_column - 1);

            // Delete lines
            self.tokens.drain(..last);
        }
    }

    fn accept_insert_text(&mut self, line_number: i32, column: i32, eol_count: usize, first_line_length: usize) {
        // Here we depend on the fact that `insertText` is called only after `deleteRange`
        if eol_count == 0 && first_line_length == 0 {
            // Nothing to insert
            return;
        }

        let line_index = line_number - self.start_line_number;

        if line_index < 0 {
            // this insertion occurs before this block, so we only need to adjust line numbers
            self.start_line_number += eol_count as i32;
            return;
        }

        if line_index >= self.tokens.len() as i32 {
            // this insertion occurs after this block, so there is nothing to do
            return;
        }

        let i = line_index as usize;
        if eol_count == 0 {
            // Inserting text on one line
            let t = self.tokens[i].take();
            self.tokens[i] = editing::insert(t, column - 1, first_line_length as i32);
            return;
        }

        // Inserting text on more than one line
        let t = self.tokens[i].take();
        self.tokens[i] = editing::delete_ending(t, column - 1);
        let t = self.tokens[i].take();
        self.tokens[i] = editing::insert(t, column - 1, first_line_length as i32);

        // `arrayInsert(this._tokens, insertIndex = lineNumber, ...)`: o índice é o número da linha, não o
        // índice relativo ao bloco, como no original.
        let insert_index = (line_number as usize).min(self.tokens.len());
        self.tokens.splice(insert_index..insert_index, std::iter::repeat_n(None, eol_count));
    }
}

/// `ContiguousMultilineTokensBuilder`.
#[derive(Default)]
pub struct ContiguousMultilineTokensBuilder {
    tokens: Vec<ContiguousMultilineTokens>,
}

impl ContiguousMultilineTokensBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add(&mut self, line_number: i32, line_tokens: Vec<u32>) {
        if let Some(last) = self.tokens.last_mut()
            && last.end_line_number() + 1 == line_number
        {
            // pick up the last token
            last.append_line_tokens(line_tokens);
            return;
        }
        self.tokens.push(ContiguousMultilineTokens::new(line_number, vec![line_tokens]));
    }

    pub fn finalize(self) -> Vec<ContiguousMultilineTokens> {
        self.tokens
    }
}
