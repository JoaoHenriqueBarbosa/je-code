//! Porte de `src/vs/editor/common/tokens/lineTokens.ts`: os tokens de uma linha (`LineTokens`, a fatia
//! `SliceLineTokens`, `TokenArray`) e `getStandardTokenTypeAtPosition`.
//!
//! O `Uint32Array` do original vira `Vec<u32>` (pares `[fim, metadados]`); o texto, [`JsString`].
//! `LineTokens::new` copia os tokens que recebe do armazenamento, onde o original os compartilha.

use std::any::Any;
use std::rc::Rc;

use je_base::JsString;
use je_base::errors::{self, Error};

use crate::encoded_token_attributes::{
    TokenMetadata, TokenPresentation, color_id, font_style, metadata_consts as mc,
};
use crate::languages::ILanguageIdCodec;
use crate::offset_range::OffsetRange;

/// `IViewLineTokens`.
pub trait IViewLineTokens {
    fn language_id_codec(&self) -> &Rc<dyn ILanguageIdCodec>;
    fn equals(&self, other: &dyn IViewLineTokens) -> bool;
    fn get_count(&self) -> usize;
    fn get_standard_token_type(&self, token_index: usize) -> u32;
    fn get_foreground(&self, token_index: usize) -> u32;
    fn get_end_offset(&self, token_index: usize) -> i32;
    fn get_class_name(&self, token_index: usize) -> String;
    fn get_inline_style(&self, token_index: usize, color_map: &[String]) -> String;
    fn get_presentation(&self, token_index: usize) -> TokenPresentation;
    fn find_token_index_at_offset(&self, offset: i32) -> usize;
    fn get_line_content(&self) -> JsString;
    fn get_metadata(&self, token_index: usize) -> u32;
    fn get_language_id(&self, token_index: usize) -> String;
    fn get_token_text(&self, token_index: usize) -> JsString;
    fn as_any(&self) -> &dyn Any;

    /// `forEach(callback)`.
    fn for_each(&self, callback: &mut dyn FnMut(usize)) {
        for token_index in 0..self.get_count() {
            callback(token_index);
        }
    }
}

/// Um token a inserir em [`LineTokens::with_inserted`].
#[derive(Clone, Debug)]
pub struct InsertToken {
    pub offset: i32,
    pub text: JsString,
    pub token_metadata: u32,
}

/// `LineTokens`.
#[derive(Clone)]
pub struct LineTokens {
    tokens: Vec<u32>,
    tokens_count: usize,
    text: JsString,
    pub language_id_codec: Rc<dyn ILanguageIdCodec>,
}

/// `LineTokens.defaultTokenMetadata`.
pub const DEFAULT_TOKEN_METADATA: u32 = (font_style::NONE << mc::FONT_STYLE_OFFSET)
    | (color_id::DEFAULT_FOREGROUND << mc::FOREGROUND_OFFSET)
    | (color_id::DEFAULT_BACKGROUND << mc::BACKGROUND_OFFSET);

impl LineTokens {
    /// `LineTokens.createEmpty(lineContent, decoder)`.
    pub fn create_empty(line_content: JsString, decoder: Rc<dyn ILanguageIdCodec>) -> LineTokens {
        let tokens = vec![line_content.len() as u32, DEFAULT_TOKEN_METADATA];
        LineTokens::new(tokens, line_content, decoder)
    }

    /// `LineTokens.createFromTextAndMetadata(data, decoder)`.
    pub fn create_from_text_and_metadata(data: &[(JsString, u32)], decoder: Rc<dyn ILanguageIdCodec>) -> LineTokens {
        let mut offset = 0u32;
        let mut full_text = JsString::new();
        let mut tokens = Vec::new();
        for (text, metadata) in data {
            tokens.push(offset + text.len() as u32);
            tokens.push(*metadata);
            offset += text.len() as u32;
            full_text.push_units(text.units());
        }
        LineTokens::new(tokens, full_text, decoder)
    }

    /// `LineTokens.convertToEndOffset(tokens, lineTextLength)`: de "início" para "fim" de cada token.
    pub fn convert_to_end_offset(tokens: &mut [u32], line_text_length: usize) {
        let token_count = tokens.len() >> 1;
        if token_count == 0 {
            return;
        }
        let last_token_index = token_count - 1;
        for token_index in 0..last_token_index {
            tokens[token_index << 1] = tokens[(token_index + 1) << 1];
        }
        tokens[last_token_index << 1] = line_text_length as u32;
    }

    /// `LineTokens.findIndexInTokensArray(tokens, desiredIndex)`.
    pub fn find_index_in_tokens_array(tokens: &[u32], desired_index: i32) -> usize {
        if tokens.len() <= 2 {
            return 0;
        }
        let mut low = 0usize;
        let mut high = (tokens.len() >> 1) - 1;
        while low < high {
            let mid = low + (high - low) / 2;
            let end_offset = tokens[mid << 1] as i64;
            let desired = desired_index as i64;
            if end_offset == desired {
                return mid + 1;
            } else if end_offset < desired {
                low = mid + 1;
            } else {
                high = mid;
            }
        }
        low
    }

    pub fn new(tokens: Vec<u32>, text: JsString, decoder: Rc<dyn ILanguageIdCodec>) -> LineTokens {
        let tokens_length = if tokens.len() > 1 { tokens[tokens.len() - 2] as usize } else { 0 };
        if tokens_length != text.len() {
            errors::on_unexpected_error(&Error::new("Token length and text length do not match!"));
        }
        let tokens_count = tokens.len() >> 1;
        LineTokens { tokens, tokens_count, text, language_id_codec: decoder }
    }

    pub fn get_text_length(&self) -> usize {
        self.text.len()
    }

    /// `equals(other)` para outro `LineTokens`.
    pub fn equals_line_tokens(&self, other: &LineTokens) -> bool {
        self.sliced_equals(other, 0, self.tokens_count)
    }

    pub fn sliced_equals(&self, other: &LineTokens, slice_from_token_index: usize, slice_token_count: usize) -> bool {
        if self.text != other.text {
            return false;
        }
        if self.tokens_count != other.tokens_count {
            return false;
        }
        let from = slice_from_token_index << 1;
        let to = from + (slice_token_count << 1);
        (from..to).all(|i| self.tokens.get(i) == other.tokens.get(i))
    }

    pub fn get_line_content_ref(&self) -> &JsString {
        &self.text
    }

    pub fn get_start_offset(&self, token_index: usize) -> i32 {
        if token_index > 0 { self.tokens[(token_index - 1) << 1] as i32 } else { 0 }
    }

    /// O `Uint32Array` cru (pares `[fim, metadados]`).
    pub fn raw_tokens(&self) -> &[u32] {
        &self.tokens
    }

    /// `inflate()`.
    pub fn inflate(&self) -> LineTokens {
        self.clone()
    }

    /// `sliceAndInflate(startOffset, endOffset, deltaOffset)`.
    pub fn slice_and_inflate(&self, start_offset: i32, end_offset: i32, delta_offset: i32) -> SliceLineTokens {
        SliceLineTokens::new(self.clone(), start_offset, end_offset, delta_offset)
    }

    /// `sliceZeroCopy(range)`.
    pub fn slice_zero_copy(&self, range: OffsetRange) -> SliceLineTokens {
        self.slice_and_inflate(range.start, range.end_exclusive, 0)
    }

    /// `withInserted(insertTokens)`.
    pub fn with_inserted(&self, insert_tokens: &[InsertToken]) -> LineTokens {
        if insert_tokens.is_empty() {
            return self.clone();
        }
        let mut next_original_token_idx = 0usize;
        let mut next_insert_token_idx = 0usize;
        let mut text = JsString::new();
        let mut new_tokens: Vec<u32> = Vec::new();
        let mut original_end_offset = 0usize;
        loop {
            let next_original_token_end_offset: i64 =
                if next_original_token_idx < self.tokens_count { self.tokens[next_original_token_idx << 1] as i64 } else { -1 };
            let next_insert_token = insert_tokens.get(next_insert_token_idx);
            if next_original_token_end_offset != -1
                && next_insert_token.is_none_or(|t| next_original_token_end_offset <= t.offset as i64)
            {
                let end = next_original_token_end_offset as usize;
                text.push_units(&self.text.units()[original_end_offset..end]);
                let metadata = self.tokens[(next_original_token_idx << 1) + 1];
                new_tokens.push(text.len() as u32);
                new_tokens.push(metadata);
                next_original_token_idx += 1;
                original_end_offset = end;
            } else if let Some(insert) = next_insert_token {
                if insert.offset as usize > original_end_offset {
                    text.push_units(&self.text.units()[original_end_offset..insert.offset as usize]);
                    let metadata = self.tokens[(next_original_token_idx << 1) + 1];
                    new_tokens.push(text.len() as u32);
                    new_tokens.push(metadata);
                    original_end_offset = insert.offset as usize;
                }
                text.push_units(insert.text.units());
                new_tokens.push(text.len() as u32);
                new_tokens.push(insert.token_metadata);
                next_insert_token_idx += 1;
            } else {
                break;
            }
        }
        LineTokens::new(new_tokens, text, self.language_id_codec.clone())
    }

    /// `getTokensInRange(range)`.
    pub fn get_tokens_in_range(&self, range: OffsetRange) -> TokenArray {
        let mut builder = TokenArrayBuilder::new();
        let start_token_index = self.find_token_index_at_offset(range.start);
        let end_token_index = self.find_token_index_at_offset(range.end_exclusive);
        for token_index in start_token_index..=end_token_index {
            let token_range = OffsetRange::new(self.get_start_offset(token_index), self.get_end_offset(token_index));
            let length = token_range.intersection_length(&range);
            if length > 0 {
                builder.add(length, self.get_metadata(token_index));
            }
        }
        builder.build()
    }

    /// `toString()`.
    pub fn to_display_string(&self) -> String {
        let mut result = String::new();
        for i in 0..self.get_count() {
            result.push_str(&format!("[{}]{{{}}}", self.get_token_text(i), self.get_class_name(i)));
        }
        result
    }
}

impl IViewLineTokens for LineTokens {
    fn language_id_codec(&self) -> &Rc<dyn ILanguageIdCodec> {
        &self.language_id_codec
    }

    fn equals(&self, other: &dyn IViewLineTokens) -> bool {
        other.as_any().downcast_ref::<LineTokens>().is_some_and(|o| self.equals_line_tokens(o))
    }

    fn get_count(&self) -> usize {
        self.tokens_count
    }

    fn get_standard_token_type(&self, token_index: usize) -> u32 {
        TokenMetadata::get_token_type(self.get_metadata(token_index))
    }

    fn get_foreground(&self, token_index: usize) -> u32 {
        TokenMetadata::get_foreground(self.get_metadata(token_index))
    }

    fn get_end_offset(&self, token_index: usize) -> i32 {
        self.tokens[token_index << 1] as i32
    }

    fn get_class_name(&self, token_index: usize) -> String {
        TokenMetadata::get_class_name_from_metadata(self.get_metadata(token_index))
    }

    fn get_inline_style(&self, token_index: usize, color_map: &[String]) -> String {
        TokenMetadata::get_inline_style_from_metadata(self.get_metadata(token_index), color_map)
    }

    fn get_presentation(&self, token_index: usize) -> TokenPresentation {
        TokenMetadata::get_presentation_from_metadata(self.get_metadata(token_index))
    }

    fn find_token_index_at_offset(&self, offset: i32) -> usize {
        LineTokens::find_index_in_tokens_array(&self.tokens, offset)
    }

    fn get_line_content(&self) -> JsString {
        self.text.clone()
    }

    fn get_metadata(&self, token_index: usize) -> u32 {
        // `this._tokens[(tokenIndex << 1) + 1]`: fora do vetor o JS lê `undefined`, que as operações de bits de
        // `TokenMetadata` tratam como 0 (linha ainda não tokenizada depois de uma edição).
        self.tokens.get((token_index << 1) + 1).copied().unwrap_or(0)
    }

    fn get_language_id(&self, token_index: usize) -> String {
        let language_id = TokenMetadata::get_language_id(self.get_metadata(token_index));
        self.language_id_codec.decode_language_id(language_id)
    }

    fn get_token_text(&self, token_index: usize) -> JsString {
        let start = self.get_start_offset(token_index) as usize;
        let end = self.get_end_offset(token_index) as usize;
        self.text.slice(start.min(self.text.len()), end.min(self.text.len()))
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// `SliceLineTokens`.
pub struct SliceLineTokens {
    source: LineTokens,
    start_offset: i32,
    end_offset: i32,
    delta_offset: i32,
    first_token_index: usize,
    tokens_count: usize,
}

impl SliceLineTokens {
    fn new(source: LineTokens, start_offset: i32, end_offset: i32, delta_offset: i32) -> Self {
        let first_token_index = source.find_token_index_at_offset(start_offset);
        let mut tokens_count = 0;
        for i in first_token_index..source.get_count() {
            let token_start_offset = source.get_start_offset(i);
            if token_start_offset >= end_offset {
                break;
            }
            tokens_count += 1;
        }
        SliceLineTokens { source, start_offset, end_offset, delta_offset, first_token_index, tokens_count }
    }
}

impl IViewLineTokens for SliceLineTokens {
    fn language_id_codec(&self) -> &Rc<dyn ILanguageIdCodec> {
        &self.source.language_id_codec
    }

    fn equals(&self, other: &dyn IViewLineTokens) -> bool {
        other.as_any().downcast_ref::<SliceLineTokens>().is_some_and(|o| {
            self.start_offset == o.start_offset
                && self.end_offset == o.end_offset
                && self.delta_offset == o.delta_offset
                && self.source.sliced_equals(&o.source, self.first_token_index, self.tokens_count)
        })
    }

    fn get_count(&self) -> usize {
        self.tokens_count
    }

    fn get_standard_token_type(&self, token_index: usize) -> u32 {
        self.source.get_standard_token_type(self.first_token_index + token_index)
    }

    fn get_foreground(&self, token_index: usize) -> u32 {
        self.source.get_foreground(self.first_token_index + token_index)
    }

    fn get_end_offset(&self, token_index: usize) -> i32 {
        let token_end_offset = self.source.get_end_offset(self.first_token_index + token_index);
        self.end_offset.min(token_end_offset) - self.start_offset + self.delta_offset
    }

    fn get_class_name(&self, token_index: usize) -> String {
        self.source.get_class_name(self.first_token_index + token_index)
    }

    fn get_inline_style(&self, token_index: usize, color_map: &[String]) -> String {
        self.source.get_inline_style(self.first_token_index + token_index, color_map)
    }

    fn get_presentation(&self, token_index: usize) -> TokenPresentation {
        self.source.get_presentation(self.first_token_index + token_index)
    }

    fn find_token_index_at_offset(&self, offset: i32) -> usize {
        let index = self.source.find_token_index_at_offset(offset + self.start_offset - self.delta_offset);
        index.wrapping_sub(self.first_token_index)
    }

    fn get_line_content(&self) -> JsString {
        let len = self.source.get_text_length();
        let end = (self.end_offset.max(0) as usize).min(len);
        let start = (self.start_offset.max(0) as usize).min(end);
        self.source.get_line_content_ref().slice(start, end)
    }

    fn get_metadata(&self, token_index: usize) -> u32 {
        self.source.get_metadata(self.first_token_index + token_index)
    }

    fn get_language_id(&self, token_index: usize) -> String {
        self.source.get_language_id(self.first_token_index + token_index)
    }

    fn get_token_text(&self, token_index: usize) -> JsString {
        let adjusted = self.first_token_index + token_index;
        let token_start_offset = self.source.get_start_offset(adjusted);
        let token_end_offset = self.source.get_end_offset(adjusted);
        let mut text = self.source.get_token_text(adjusted);
        if token_start_offset < self.start_offset {
            let cut = ((self.start_offset - token_start_offset) as usize).min(text.len());
            text = text.slice(cut, text.len());
        }
        if token_end_offset > self.end_offset {
            let cut = ((token_end_offset - self.end_offset) as usize).min(text.len());
            text = text.slice(0, text.len() - cut);
        }
        text
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// `TokenInfo`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TokenInfo {
    pub length: i32,
    pub metadata: u32,
}

/// `TokenArray`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TokenArray {
    token_info: Vec<TokenInfo>,
}

impl TokenArray {
    /// `TokenArray.fromLineTokens(lineTokens)`.
    pub fn from_line_tokens(line_tokens: &LineTokens) -> TokenArray {
        let info = (0..line_tokens.get_count())
            .map(|i| TokenInfo {
                length: line_tokens.get_end_offset(i) - line_tokens.get_start_offset(i),
                metadata: line_tokens.get_metadata(i),
            })
            .collect();
        TokenArray::create(info)
    }

    pub fn create(token_info: Vec<TokenInfo>) -> TokenArray {
        TokenArray { token_info }
    }

    /// `toLineTokens(lineContent, decoder)`.
    pub fn to_line_tokens(&self, line_content: &JsString, decoder: Rc<dyn ILanguageIdCodec>) -> LineTokens {
        let data: Vec<(JsString, u32)> = self
            .map(|r, t| {
                let end = (r.end_exclusive.max(0) as usize).min(line_content.len());
                let start = (r.start.max(0) as usize).min(end);
                (line_content.slice(start, end), t.metadata)
            });
        LineTokens::create_from_text_and_metadata(&data, decoder)
    }

    pub fn for_each(&self, mut cb: impl FnMut(OffsetRange, &TokenInfo)) {
        let mut length_sum = 0;
        for token_info in &self.token_info {
            cb(OffsetRange::new(length_sum, length_sum + token_info.length), token_info);
            length_sum += token_info.length;
        }
    }

    pub fn map<T>(&self, mut cb: impl FnMut(OffsetRange, &TokenInfo) -> T) -> Vec<T> {
        let mut result = Vec::new();
        self.for_each(|r, t| result.push(cb(r, t)));
        result
    }

    pub fn slice(&self, range: OffsetRange) -> TokenArray {
        let mut result = Vec::new();
        let mut length_sum = 0;
        for token_info in &self.token_info {
            let token_start = length_sum;
            let token_end_ex = token_start + token_info.length;
            if token_end_ex > range.start {
                if token_start >= range.end_exclusive {
                    break;
                }
                let delta_before = (range.start - token_start).max(0);
                let delta_after = (token_end_ex - range.end_exclusive).max(0);
                result.push(TokenInfo { length: token_info.length - delta_before - delta_after, metadata: token_info.metadata });
            }
            length_sum += token_info.length;
        }
        TokenArray::create(result)
    }

    pub fn append(&self, other: &TokenArray) -> TokenArray {
        let mut result = self.token_info.clone();
        result.extend_from_slice(&other.token_info);
        TokenArray::create(result)
    }
}

/// `TokenArrayBuilder`.
#[derive(Default)]
pub struct TokenArrayBuilder {
    tokens: Vec<TokenInfo>,
}

impl TokenArrayBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add(&mut self, length: i32, metadata: u32) {
        self.tokens.push(TokenInfo { length, metadata });
    }

    pub fn build(self) -> TokenArray {
        TokenArray::create(self.tokens)
    }
}
