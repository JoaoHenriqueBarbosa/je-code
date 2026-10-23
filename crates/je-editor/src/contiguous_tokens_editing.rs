//! Porte de `src/vs/editor/common/tokens/contiguousTokensEditing.ts`: a edição dos tokens de uma linha.
//!
//! O `Uint32Array | ArrayBuffer | null` do original vira `Option<TokenData>`: `None` é o `null` (a linha
//! ainda não foi tokenizada), [`TokenData::Empty`] é o `EMPTY_LINE_TOKENS` (linha vazia, sem tokens) e
//! [`TokenData::Array`] são os pares `[fim, metadados]`. As funções recebem o valor por movimento e o
//! devolvem, o que reproduz a edição no lugar do original.

use crate::line_tokens::LineTokens;

/// Os tokens de uma linha tokenizada.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TokenData {
    /// `EMPTY_LINE_TOKENS`.
    Empty,
    Array(Vec<u32>),
}

impl TokenData {
    /// `toUint32Array(arr)`.
    pub fn into_array(self) -> Vec<u32> {
        match self {
            TokenData::Empty => Vec::new(),
            TokenData::Array(array) => array,
        }
    }

    pub fn as_slice(&self) -> &[u32] {
        match self {
            TokenData::Empty => &[],
            TokenData::Array(array) => array,
        }
    }
}

/// Os tokens crus de uma linha (`null` quando ainda não tokenizada).
pub type RawLineTokens = Option<TokenData>;

fn is_null_or_empty(line_tokens: &RawLineTokens) -> bool {
    matches!(line_tokens, None | Some(TokenData::Empty))
}

/// `ContiguousTokensEditing.deleteBeginning`.
pub fn delete_beginning(line_tokens: RawLineTokens, to_ch_index: i32) -> RawLineTokens {
    if is_null_or_empty(&line_tokens) {
        return line_tokens;
    }
    delete(line_tokens, 0, to_ch_index)
}

/// `ContiguousTokensEditing.deleteEnding`.
pub fn delete_ending(line_tokens: RawLineTokens, from_ch_index: i32) -> RawLineTokens {
    if is_null_or_empty(&line_tokens) {
        return line_tokens;
    }
    let tokens = line_tokens.as_ref().map(TokenData::as_slice).unwrap_or(&[]);
    let line_text_length = tokens[tokens.len() - 2] as i32;
    delete(line_tokens, from_ch_index, line_text_length)
}

/// `ContiguousTokensEditing.delete`.
pub fn delete(line_tokens: RawLineTokens, from_ch_index: i32, to_ch_index: i32) -> RawLineTokens {
    if is_null_or_empty(&line_tokens) || from_ch_index == to_ch_index {
        return line_tokens;
    }
    let mut tokens = line_tokens.map(TokenData::into_array).unwrap_or_default();
    let tokens_count = tokens.len() >> 1;

    // special case: deleting everything
    if from_ch_index == 0 && tokens[tokens.len() - 2] as i32 == to_ch_index {
        return Some(TokenData::Empty);
    }

    let from_token_index = LineTokens::find_index_in_tokens_array(&tokens, from_ch_index);
    let from_token_start_offset = if from_token_index > 0 { tokens[(from_token_index - 1) << 1] as i32 } else { 0 };
    let from_token_end_offset = tokens[from_token_index << 1] as i32;

    if to_ch_index < from_token_end_offset {
        // the delete range is inside a single token
        let delta = to_ch_index - from_ch_index;
        for i in from_token_index..tokens_count {
            tokens[i << 1] = (tokens[i << 1] as i32 - delta) as u32;
        }
        return Some(TokenData::Array(tokens));
    }

    let mut dest: usize;
    let mut last_end: i32;
    if from_token_start_offset != from_ch_index {
        tokens[from_token_index << 1] = from_ch_index as u32;
        dest = (from_token_index + 1) << 1;
        last_end = from_ch_index;
    } else {
        dest = from_token_index << 1;
        last_end = from_token_start_offset;
    }

    let delta = to_ch_index - from_ch_index;
    for token_index in from_token_index + 1..tokens_count {
        let token_end_offset = tokens[token_index << 1] as i32 - delta;
        if token_end_offset > last_end {
            tokens[dest] = token_end_offset as u32;
            tokens[dest + 1] = tokens[(token_index << 1) + 1];
            dest += 2;
            last_end = token_end_offset;
        }
    }

    tokens.truncate(dest);
    Some(TokenData::Array(tokens))
}

/// `ContiguousTokensEditing.append`.
pub fn append(line_tokens: RawLineTokens, other_tokens: RawLineTokens) -> RawLineTokens {
    if other_tokens == Some(TokenData::Empty) {
        return line_tokens;
    }
    if line_tokens == Some(TokenData::Empty) {
        return other_tokens;
    }
    if line_tokens.is_none() {
        return line_tokens;
    }
    let other_tokens = other_tokens?;
    let my_tokens = line_tokens.map(TokenData::into_array).unwrap_or_default();
    let other_tokens = other_tokens.into_array();
    let other_tokens_count = other_tokens.len() >> 1;

    let mut result = Vec::with_capacity(my_tokens.len() + other_tokens.len());
    result.extend_from_slice(&my_tokens);
    let delta = my_tokens[my_tokens.len() - 2];
    for i in 0..other_tokens_count {
        result.push(other_tokens[i << 1] + delta);
        result.push(other_tokens[(i << 1) + 1]);
    }
    Some(TokenData::Array(result))
}

/// `ContiguousTokensEditing.insert`.
pub fn insert(line_tokens: RawLineTokens, ch_index: i32, text_length: i32) -> RawLineTokens {
    if is_null_or_empty(&line_tokens) {
        // nothing to do
        return line_tokens;
    }
    let mut tokens = line_tokens.map(TokenData::into_array).unwrap_or_default();
    let tokens_count = tokens.len() >> 1;

    let mut from_token_index = LineTokens::find_index_in_tokens_array(&tokens, ch_index);
    if from_token_index > 0 {
        let from_token_start_offset = tokens[(from_token_index - 1) << 1] as i32;
        if from_token_start_offset == ch_index {
            from_token_index -= 1;
        }
    }
    for token_index in from_token_index..tokens_count {
        tokens[token_index << 1] = (tokens[token_index << 1] as i32 + text_length) as u32;
    }
    Some(TokenData::Array(tokens))
}
