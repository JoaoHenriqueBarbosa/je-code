//! Porte de `src/vs/editor/common/tokens/sparseMultilineTokens.ts`: tokens esparsos (semânticos) sobre um
//! intervalo contíguo de linhas, que acompanham as edições.
//!
//! Decisões de tradução:
//! - O `Uint32Array` do original vira `Vec<u32>`. Toda aritmética sobre o que é lido do armazenamento usa
//!   `i64` e volta a `u32` com `as u32`, que trunca módulo 2^32 como a atribuição a um `Uint32Array`.
//! - `getLineTokens` devolve uma cópia da fatia (`SparseLineTokens` do original guarda um `subarray`).
//! - `IRange` e `Range` são o mesmo tipo em Rust; `acceptEdit` recebe [`Range`].
//! - `reportIfInvalid` recebe o [`TextModel`]; o `console.error` limitado a 10 vezes por minuto (`RateLimiter`
//!   de `tokens/common.ts`) vira `eprintln!` com o mesmo limite, contado pelo relógio de parede do loop
//!   (`runtime::date_now`).

use crate::cursor_common::CursorSimpleModel;
use std::cell::Cell;

use je_base::char_code;
use je_base::runtime;

use crate::eol_counter::count_eol;
use crate::position::Position;
use crate::range::Range;
use crate::text_model::TextModel;

/// `RateLimiter` de `tokens/common.ts`.
struct RateLimiter {
    last_run: Cell<u64>,
    minimum_time_between_runs: f64,
}

impl RateLimiter {
    fn new(times_per_second: f64) -> Self {
        RateLimiter { last_run: Cell::new(0), minimum_time_between_runs: 1000.0 / times_per_second }
    }

    fn run_if_not_limited(&self, callback: impl FnOnce()) {
        let now = runtime::date_now();
        if (now.saturating_sub(self.last_run.get())) as f64 >= self.minimum_time_between_runs {
            self.last_run.set(now);
            callback();
        }
    }
}

thread_local! {
    /// `SparseMultilineTokensStorage._rateLimiter`: no máximo 10 vezes por minuto.
    static RATE_LIMITER: RateLimiter = RateLimiter::new(10.0 / 60.0);
}

/// Tokens esparsos sobre um intervalo contíguo de linhas.
#[derive(Clone, Debug)]
pub struct SparseMultilineTokens {
    start_line_number: i32,
    end_line_number: i32,
    tokens: SparseMultilineTokensStorage,
}

impl SparseMultilineTokens {
    /// `SparseMultilineTokens.create(startLineNumber, tokens)`.
    pub fn create(start_line_number: i32, tokens: Vec<u32>) -> SparseMultilineTokens {
        SparseMultilineTokens::from_storage(start_line_number, SparseMultilineTokensStorage::new(tokens))
    }

    fn from_storage(start_line_number: i32, tokens: SparseMultilineTokensStorage) -> SparseMultilineTokens {
        let end_line_number = start_line_number + tokens.get_max_delta_line();
        SparseMultilineTokens { start_line_number, end_line_number, tokens }
    }

    /// O número da linha inicial (inclusivo).
    pub fn start_line_number(&self) -> i32 {
        self.start_line_number
    }

    /// O número da linha final (inclusivo).
    pub fn end_line_number(&self) -> i32 {
        self.end_line_number
    }

    fn update_end_line_number(&mut self) {
        self.end_line_number = self.start_line_number + self.tokens.get_max_delta_line();
    }

    pub fn is_empty(&self) -> bool {
        self.tokens.is_empty()
    }

    pub fn get_line_tokens(&self, line_number: i32) -> Option<SparseLineTokens> {
        if self.start_line_number <= line_number && line_number <= self.end_line_number {
            return self.tokens.get_line_tokens(line_number - self.start_line_number);
        }
        None
    }

    pub fn get_range(&self) -> Option<Range> {
        let delta_range = self.tokens.get_range()?;
        Some(Range::new(
            self.start_line_number + delta_range.start_line_number,
            delta_range.start_column,
            self.start_line_number + delta_range.end_line_number,
            delta_range.end_column,
        ))
    }

    pub fn remove_tokens(&mut self, range: &Range) {
        let start_line_index = range.start_line_number - self.start_line_number;
        let end_line_index = range.end_line_number - self.start_line_number;

        self.start_line_number +=
            self.tokens.remove_tokens(start_line_index, range.start_column - 1, end_line_index, range.end_column - 1);
        self.update_end_line_number();
    }

    pub fn split(&self, range: &Range) -> (SparseMultilineTokens, SparseMultilineTokens) {
        // split tokens to two:
        // a) all the tokens before `range`
        // b) all the tokens after `range`
        let start_line_index = range.start_line_number - self.start_line_number;
        let end_line_index = range.end_line_number - self.start_line_number;

        let (a, b, b_delta_line) =
            self.tokens.split(start_line_index, range.start_column - 1, end_line_index, range.end_column - 1);
        (
            SparseMultilineTokens::from_storage(self.start_line_number, a),
            SparseMultilineTokens::from_storage(self.start_line_number + b_delta_line, b),
        )
    }

    /// `applyEdit(range, text)`.
    pub fn apply_edit(&mut self, range: &Range, text: &[u16]) {
        let eol = count_eol(text);
        self.accept_edit(
            range,
            eol.eol_count as i32,
            eol.first_line_length as i32,
            eol.last_line_length as i32,
            text.first().copied().unwrap_or(char_code::Null),
        );
    }

    pub fn accept_edit(
        &mut self,
        range: &Range,
        eol_count: i32,
        first_line_length: i32,
        last_line_length: i32,
        first_char_code: u16,
    ) {
        self.accept_delete_range(range);
        self.accept_insert_text(
            Position::new(range.start_line_number, range.start_column),
            eol_count,
            first_line_length,
            last_line_length,
            first_char_code,
        );
        self.update_end_line_number();
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

        let token_max_delta_line = self.tokens.get_max_delta_line();

        if first_line_index > token_max_delta_line {
            // this deletion occurs entirely after this block, so there is nothing to do
            return;
        }

        if first_line_index < 0 && last_line_index > token_max_delta_line {
            // this deletion completely encompasses this block
            self.start_line_number = 0;
            self.tokens.clear();
            return;
        }

        if first_line_index < 0 {
            let deleted_before = -first_line_index;
            self.start_line_number -= deleted_before;

            self.tokens.accept_delete_range(range.start_column - 1, 0, 0, last_line_index, range.end_column - 1);
        } else {
            self.tokens.accept_delete_range(
                0,
                first_line_index,
                range.start_column - 1,
                last_line_index,
                range.end_column - 1,
            );
        }
    }

    fn accept_insert_text(
        &mut self,
        position: Position,
        eol_count: i32,
        first_line_length: i32,
        last_line_length: i32,
        first_char_code: u16,
    ) {
        if eol_count == 0 && first_line_length == 0 {
            // Nothing to insert
            return;
        }

        let line_index = position.line_number - self.start_line_number;

        if line_index < 0 {
            // this insertion occurs before this block, so we only need to adjust line numbers
            self.start_line_number += eol_count;
            return;
        }

        let token_max_delta_line = self.tokens.get_max_delta_line();

        if line_index > token_max_delta_line {
            // this insertion occurs after this block, so there is nothing to do
            return;
        }

        self.tokens.accept_insert_text(
            line_index,
            position.column - 1,
            eol_count,
            first_line_length,
            last_line_length,
            first_char_code,
        );
    }

    pub fn report_if_invalid(&self, model: &TextModel) {
        self.tokens.report_if_invalid(model, self.start_line_number);
    }
}

impl std::fmt::Display for SparseMultilineTokens {
    /// `toString()`.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.tokens.to_display_string(self.start_line_number))
    }
}

#[derive(Clone, Debug)]
struct SparseMultilineTokensStorage {
    /// A codificação dos tokens é:
    ///  4*i    deltaLine (a partir de `startLineNumber`)
    ///  4*i+1  startCharacter (a partir do início da linha)
    ///  4*i+2  endCharacter (a partir do início da linha)
    ///  4*i+3  metadata
    tokens: Vec<u32>,
    token_count: usize,
}

impl SparseMultilineTokensStorage {
    fn new(tokens: Vec<u32>) -> Self {
        let token_count = tokens.len() / 4;
        SparseMultilineTokensStorage { tokens, token_count }
    }

    fn to_display_string(&self, start_line_number: i32) -> String {
        let pieces: Vec<String> = (0..self.token_count)
            .map(|i| {
                format!(
                    "({},{}-{})",
                    self.get_delta_line(i) as i64 + start_line_number as i64,
                    self.get_start_character(i),
                    self.get_end_character(i)
                )
            })
            .collect();
        format!("[{}]", pieces.join(","))
    }

    fn get_max_delta_line(&self) -> i32 {
        if self.token_count == 0 {
            return -1;
        }
        self.get_delta_line(self.token_count - 1) as i32
    }

    fn get_range(&self) -> Option<Range> {
        if self.token_count == 0 {
            return None;
        }
        let start_char = self.get_start_character(0) as i32;
        let max_delta_line = self.get_delta_line(self.token_count - 1) as i32;
        let end_char = self.get_end_character(self.token_count - 1) as i32;
        Some(Range::new(0, start_char + 1, max_delta_line, end_char + 1))
    }

    fn get_delta_line(&self, token_index: usize) -> u32 {
        self.tokens[4 * token_index]
    }

    fn get_start_character(&self, token_index: usize) -> u32 {
        self.tokens[4 * token_index + 1]
    }

    fn get_end_character(&self, token_index: usize) -> u32 {
        self.tokens[4 * token_index + 2]
    }

    fn is_empty(&self) -> bool {
        self.token_count == 0
    }

    fn get_line_tokens(&self, delta_line: i32) -> Option<SparseLineTokens> {
        if self.token_count == 0 {
            return None;
        }
        let delta_line = delta_line as i64;
        let mut low: i64 = 0;
        let mut high: i64 = self.token_count as i64 - 1;

        while low < high {
            let mid = low + (high - low) / 2;
            let mid_delta_line = self.get_delta_line(mid as usize) as i64;

            if mid_delta_line < delta_line {
                low = mid + 1;
            } else if mid_delta_line > delta_line {
                high = mid - 1;
            } else {
                let mut min = mid;
                while min > low && self.get_delta_line(min as usize - 1) as i64 == delta_line {
                    min -= 1;
                }
                let mut max = mid;
                while max < high && self.get_delta_line(max as usize + 1) as i64 == delta_line {
                    max += 1;
                }
                return Some(SparseLineTokens::new(self.tokens[4 * min as usize..4 * max as usize + 4].to_vec()));
            }
        }

        if self.get_delta_line(low as usize) as i64 == delta_line {
            return Some(SparseLineTokens::new(self.tokens[4 * low as usize..4 * low as usize + 4].to_vec()));
        }

        None
    }

    fn clear(&mut self) {
        self.token_count = 0;
    }

    fn remove_tokens(&mut self, start_delta_line: i32, start_char: i32, end_delta_line: i32, end_char: i32) -> i32 {
        let (start_delta_line, start_char) = (start_delta_line as i64, start_char as i64);
        let (end_delta_line, end_char) = (end_delta_line as i64, end_char as i64);
        let token_count = self.token_count;
        let mut new_token_count = 0;
        let mut has_deleted_tokens = false;
        let mut first_delta_line: i64 = 0;
        for i in 0..token_count {
            let src_offset = 4 * i;
            let token_delta_line = self.tokens[src_offset] as i64;
            let token_start_character = self.tokens[src_offset + 1] as i64;
            let token_end_character = self.tokens[src_offset + 2] as i64;
            let token_metadata = self.tokens[src_offset + 3];

            if (token_delta_line > start_delta_line
                || (token_delta_line == start_delta_line && token_end_character >= start_char))
                && (token_delta_line < end_delta_line
                    || (token_delta_line == end_delta_line && token_start_character <= end_char))
            {
                has_deleted_tokens = true;
            } else {
                if new_token_count == 0 {
                    first_delta_line = token_delta_line;
                }
                if has_deleted_tokens {
                    // must move the token to the left
                    let dest_offset = 4 * new_token_count;
                    self.tokens[dest_offset] = (token_delta_line - first_delta_line) as u32;
                    self.tokens[dest_offset + 1] = token_start_character as u32;
                    self.tokens[dest_offset + 2] = token_end_character as u32;
                    self.tokens[dest_offset + 3] = token_metadata;
                } else if first_delta_line != 0 {
                    // must adjust the delta line in place
                    self.tokens[src_offset] = (token_delta_line - first_delta_line) as u32;
                }
                new_token_count += 1;
            }
        }

        self.token_count = new_token_count;

        first_delta_line as i32
    }

    fn split(
        &self,
        start_delta_line: i32,
        start_char: i32,
        end_delta_line: i32,
        end_char: i32,
    ) -> (SparseMultilineTokensStorage, SparseMultilineTokensStorage, i32) {
        let (start_delta_line, start_char) = (start_delta_line as i64, start_char as i64);
        let (end_delta_line, end_char) = (end_delta_line as i64, end_char as i64);
        let mut a_tokens: Vec<u32> = Vec::new();
        let mut b_tokens: Vec<u32> = Vec::new();
        let mut dest_is_b = false;
        let mut dest_first_delta_line: i64 = 0;
        for i in 0..self.token_count {
            let src_offset = 4 * i;
            let token_delta_line = self.tokens[src_offset] as i64;
            let token_start_character = self.tokens[src_offset + 1] as i64;
            let token_end_character = self.tokens[src_offset + 2] as i64;
            let token_metadata = self.tokens[src_offset + 3];

            if token_delta_line > start_delta_line
                || (token_delta_line == start_delta_line && token_end_character >= start_char)
            {
                if token_delta_line < end_delta_line
                    || (token_delta_line == end_delta_line && token_start_character <= end_char)
                {
                    // this token is touching the range
                    continue;
                } else if !dest_is_b {
                    // this token is after the range, and it is the first token after the range
                    dest_is_b = true;
                    dest_first_delta_line = token_delta_line;
                }
            }

            let dest = if dest_is_b { &mut b_tokens } else { &mut a_tokens };
            dest.push((token_delta_line - dest_first_delta_line) as u32);
            dest.push(token_start_character as u32);
            dest.push(token_end_character as u32);
            dest.push(token_metadata);
        }

        (
            SparseMultilineTokensStorage::new(a_tokens),
            SparseMultilineTokensStorage::new(b_tokens),
            dest_first_delta_line as i32,
        )
    }

    fn accept_delete_range(
        &mut self,
        horizontal_shift_for_first_line_tokens: i32,
        start_delta_line: i32,
        start_character: i32,
        end_delta_line: i32,
        end_character: i32,
    ) {
        // This is a bit complex, here are the cases I used to think about this:
        //
        // 1. The token starts before the deletion range
        // 1a. The token is completely before the deletion range
        //               -----------
        //                          xxxxxxxxxxx
        // 1b. The token starts before, the deletion range ends after the token
        //               -----------
        //                      xxxxxxxxxxx
        // 1c. The token starts before, the deletion range ends precisely with the token
        //               ---------------
        //                      xxxxxxxx
        // 1d. The token starts before, the deletion range is inside the token
        //               ---------------
        //                    xxxxx
        //
        // 2. The token starts at the same position with the deletion range
        // 2a. The token starts at the same position, and ends inside the deletion range
        //               -------
        //               xxxxxxxxxxx
        // 2b. The token starts at the same position, and ends at the same position as the deletion range
        //               ----------
        //               xxxxxxxxxx
        // 2c. The token starts at the same position, and ends after the deletion range
        //               -------------
        //               xxxxxxx
        //
        // 3. The token starts inside the deletion range
        // 3a. The token is inside the deletion range
        //                -------
        //             xxxxxxxxxxxxx
        // 3b. The token starts inside the deletion range, and ends at the same position as the deletion range
        //                ----------
        //             xxxxxxxxxxxxx
        // 3c. The token starts inside the deletion range, and ends after the deletion range
        //                ------------
        //             xxxxxxxxxxx
        //
        // 4. The token starts after the deletion range
        //                  -----------
        //          xxxxxxxx
        //
        let horizontal_shift = horizontal_shift_for_first_line_tokens as i64;
        let (start_delta_line, start_character) = (start_delta_line as i64, start_character as i64);
        let (end_delta_line, end_character) = (end_delta_line as i64, end_character as i64);
        let token_count = self.token_count;
        let deleted_line_count = end_delta_line - start_delta_line;
        let mut new_token_count = 0;
        let mut has_deleted_tokens = false;
        for i in 0..token_count {
            let src_offset = 4 * i;
            let mut token_delta_line = self.tokens[src_offset] as i64;
            let mut token_start_character = self.tokens[src_offset + 1] as i64;
            let mut token_end_character = self.tokens[src_offset + 2] as i64;
            let token_metadata = self.tokens[src_offset + 3];

            if token_delta_line < start_delta_line
                || (token_delta_line == start_delta_line && token_end_character <= start_character)
            {
                // 1a. The token is completely before the deletion range
                // => nothing to do
                new_token_count += 1;
                continue;
            } else if token_delta_line == start_delta_line && token_start_character < start_character {
                // 1b, 1c, 1d
                // => the token survives, but it needs to shrink
                if token_delta_line == end_delta_line && token_end_character > end_character {
                    // 1d. The token starts before, the deletion range is inside the token
                    // => the token shrinks by the deletion character count
                    token_end_character -= end_character - start_character;
                } else {
                    // 1b. The token starts before, the deletion range ends after the token
                    // 1c. The token starts before, the deletion range ends precisely with the token
                    // => the token shrinks its ending to the deletion start
                    token_end_character = start_character;
                }
            } else if token_delta_line == start_delta_line && token_start_character == start_character {
                // 2a, 2b, 2c
                if token_delta_line == end_delta_line && token_end_character > end_character {
                    // 2c. The token starts at the same position, and ends after the deletion range
                    // => the token shrinks by the deletion character count
                    token_end_character -= end_character - start_character;
                } else {
                    // 2a. The token starts at the same position, and ends inside the deletion range
                    // 2b. The token starts at the same position, and ends at the same position as the deletion range
                    // => the token is deleted
                    has_deleted_tokens = true;
                    continue;
                }
            } else if token_delta_line < end_delta_line
                || (token_delta_line == end_delta_line && token_start_character < end_character)
            {
                // 3a, 3b, 3c
                if token_delta_line == end_delta_line && token_end_character > end_character {
                    // 3c. The token starts inside the deletion range, and ends after the deletion range
                    // => the token moves to continue right after the deletion
                    token_delta_line = start_delta_line;
                    token_start_character = start_character;
                    token_end_character = token_start_character + (token_end_character - end_character);
                } else {
                    // 3a. The token is inside the deletion range
                    // 3b. The token starts inside the deletion range, and ends at the same position as the deletion range
                    // => the token is deleted
                    has_deleted_tokens = true;
                    continue;
                }
            } else if token_delta_line > end_delta_line {
                // 4. (partial) The token starts after the deletion range, on a line below...
                if deleted_line_count == 0 && !has_deleted_tokens {
                    // early stop, there is no need to walk all the tokens and do nothing...
                    new_token_count = token_count;
                    break;
                }
                token_delta_line -= deleted_line_count;
            } else if token_delta_line == end_delta_line && token_start_character >= end_character {
                // 4. (continued) The token starts after the deletion range, on the last line where a deletion occurs
                if horizontal_shift != 0 && token_delta_line == 0 {
                    token_start_character += horizontal_shift;
                    token_end_character += horizontal_shift;
                }
                token_delta_line -= deleted_line_count;
                token_start_character -= end_character - start_character;
                token_end_character -= end_character - start_character;
            } else {
                panic!("Not possible!");
            }

            let dest_offset = 4 * new_token_count;
            self.tokens[dest_offset] = token_delta_line as u32;
            self.tokens[dest_offset + 1] = token_start_character as u32;
            self.tokens[dest_offset + 2] = token_end_character as u32;
            self.tokens[dest_offset + 3] = token_metadata;
            new_token_count += 1;
        }

        self.token_count = new_token_count;
    }

    fn accept_insert_text(
        &mut self,
        delta_line: i32,
        character: i32,
        eol_count: i32,
        first_line_length: i32,
        last_line_length: i32,
        first_char_code: u16,
    ) {
        // Here are the cases I used to think about this:
        //
        // 1. The token is completely before the insertion point
        //            -----------   |
        // 2. The token ends precisely at the insertion point
        //            -----------|
        // 3. The token contains the insertion point
        //            -----|------
        // 4. The token starts precisely at the insertion point
        //            |-----------
        // 5. The token is completely after the insertion point
        //            |   -----------
        //
        let is_inserting_precisely_one_word_character = eol_count == 0
            && first_line_length == 1
            && ((char_code::Digit0..=char_code::Digit9).contains(&first_char_code)
                || (char_code::A..=char_code::Z).contains(&first_char_code)
                || (char_code::a..=char_code::z).contains(&first_char_code));
        let (delta_line, character) = (delta_line as i64, character as i64);
        let (eol_count, first_line_length, last_line_length) =
            (eol_count as i64, first_line_length as i64, last_line_length as i64);
        for i in 0..self.token_count {
            let offset = 4 * i;
            let mut token_delta_line = self.tokens[offset] as i64;
            let mut token_start_character = self.tokens[offset + 1] as i64;
            let mut token_end_character = self.tokens[offset + 2] as i64;

            if token_delta_line < delta_line || (token_delta_line == delta_line && token_end_character < character) {
                // 1. The token is completely before the insertion point
                // => nothing to do
                continue;
            } else if token_delta_line == delta_line && token_end_character == character {
                // 2. The token ends precisely at the insertion point
                // => expand the end character only if inserting precisely one character that is a word character
                if is_inserting_precisely_one_word_character {
                    token_end_character += 1;
                } else {
                    continue;
                }
            } else if token_delta_line == delta_line
                && token_start_character < character
                && character < token_end_character
            {
                // 3. The token contains the insertion point
                if eol_count == 0 {
                    // => just expand the end character
                    token_end_character += first_line_length;
                } else {
                    // => cut off the token
                    token_end_character = character;
                }
            } else {
                // 4. or 5.
                if token_delta_line == delta_line
                    && token_start_character == character
                    && is_inserting_precisely_one_word_character
                {
                    // 4. The token starts precisely at the insertion point
                    // => grow the token (by keeping its start constant) only if inserting precisely one character that is a word character
                    // => otherwise behave as in case 5.
                    continue;
                }
                // => the token must move and keep its size constant
                if token_delta_line == delta_line {
                    token_delta_line += eol_count;
                    // this token is on the line where the insertion is taking place
                    if eol_count == 0 {
                        token_start_character += first_line_length;
                        token_end_character += first_line_length;
                    } else {
                        let token_length = token_end_character - token_start_character;
                        token_start_character = last_line_length + (token_start_character - character);
                        token_end_character = token_start_character + token_length;
                    }
                } else {
                    token_delta_line += eol_count;
                }
            }

            self.tokens[offset] = token_delta_line as u32;
            self.tokens[offset + 1] = token_start_character as u32;
            self.tokens[offset + 2] = token_end_character as u32;
        }
    }

    fn report_if_invalid(&self, model: &TextModel, start_line_number: i32) {
        for i in 0..self.token_count {
            let line_number = self.get_delta_line(i) as i64 + start_line_number as i64;

            let message = if line_number < 1 {
                "Invalid Semantic Tokens Data From Extension: lineNumber < 1"
            } else if line_number > model.get_line_count() as i64 {
                "Invalid Semantic Tokens Data From Extension: lineNumber > model.getLineCount()"
            } else if self.get_end_character(i) as usize > model.get_line_length(line_number as i32) {
                "Invalid Semantic Tokens Data From Extension: end character > model.getLineLength(lineNumber)"
            } else {
                continue;
            };
            RATE_LIMITER.with(|limiter| limiter.run_if_not_limited(|| eprintln!("{message}")));
        }
    }
}

/// Os tokens esparsos de uma linha.
#[derive(Clone, Debug)]
pub struct SparseLineTokens {
    tokens: Vec<u32>,
}

impl SparseLineTokens {
    fn new(tokens: Vec<u32>) -> Self {
        SparseLineTokens { tokens }
    }

    pub fn get_count(&self) -> usize {
        self.tokens.len() / 4
    }

    pub fn get_start_character(&self, token_index: usize) -> u32 {
        self.tokens[4 * token_index + 1]
    }

    pub fn get_end_character(&self, token_index: usize) -> u32 {
        self.tokens[4 * token_index + 2]
    }

    pub fn get_metadata(&self, token_index: usize) -> u32 {
        self.tokens[4 * token_index + 3]
    }
}
