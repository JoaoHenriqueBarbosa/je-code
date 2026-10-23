//! Porte de `src/vs/editor/common/languages/supports.ts`: os tokens de uma linha restritos a uma
//! linguagem (`ScopedLineTokens`, `createScopedLineTokens`) e `ignoreBracketsInToken`.

use std::rc::Rc;

use je_base::JsString;

use crate::encoded_token_attributes::standard_token_type;
use crate::languages::ILanguageIdCodec;
use crate::line_tokens::{IViewLineTokens, LineTokens, SliceLineTokens};

/// `createScopedLineTokens`: a faixa de tokens, em volta de `offset`, que tem a mesma linguagem do token
/// que contém `offset`.
pub fn create_scoped_line_tokens(context: Rc<LineTokens>, offset: i32) -> ScopedLineTokens {
    let token_count = context.get_count();
    if token_count == 0 {
        // Linha sem tokens: no original os acessos fora do `Uint32Array` dão `undefined` (linguagem 0 e
        // fim `undefined`, que o `substring` trata como o fim do texto, aqui vazio).
        let language_id = context.language_id_codec.decode_language_id(0);
        return ScopedLineTokens::new(context, language_id, 0, 0, 0, 0);
    }
    let token_index = context.find_token_index_at_offset(offset);
    let desired_language_id = context.get_language_id(token_index);

    let mut last_token_index = token_index;
    while last_token_index + 1 < token_count
        && context.get_language_id(last_token_index + 1) == desired_language_id
    {
        last_token_index += 1;
    }

    let mut first_token_index = token_index;
    while first_token_index > 0
        && context.get_language_id(first_token_index - 1) == desired_language_id
    {
        first_token_index -= 1;
    }

    let first_char_offset = context.get_start_offset(first_token_index);
    let last_char_offset = context.get_end_offset(last_token_index);
    ScopedLineTokens::new(
        context,
        desired_language_id,
        first_token_index,
        last_token_index + 1,
        first_char_offset,
        last_char_offset,
    )
}

/// `ScopedLineTokens`.
pub struct ScopedLineTokens {
    pub language_id_codec: Rc<dyn ILanguageIdCodec>,
    pub language_id: String,
    actual: Rc<LineTokens>,
    first_token_index: usize,
    last_token_index: usize,
    pub first_char_offset: i32,
    last_char_offset: i32,
}

impl ScopedLineTokens {
    pub fn new(
        actual: Rc<LineTokens>,
        language_id: String,
        first_token_index: usize,
        last_token_index: usize,
        first_char_offset: i32,
        last_char_offset: i32,
    ) -> Self {
        Self {
            language_id_codec: actual.language_id_codec.clone(),
            language_id,
            actual,
            first_token_index,
            last_token_index,
            first_char_offset,
            last_char_offset,
        }
    }

    pub fn get_line_content(&self) -> JsString {
        self.actual.get_line_content().slice(
            self.first_char_offset as usize,
            self.last_char_offset as usize,
        )
    }

    pub fn get_line_length(&self) -> i32 {
        self.last_char_offset - self.first_char_offset
    }

    pub fn get_actual_line_content_before(&self, offset: i32) -> JsString {
        let line = self.actual.get_line_content();
        let end = ((self.first_char_offset + offset).max(0) as usize).min(line.len());
        line.slice(0, end)
    }

    pub fn get_token_count(&self) -> usize {
        self.last_token_index - self.first_token_index
    }

    /// O índice relativo ao primeiro token da faixa: fica negativo quando `offset` cai antes dela.
    pub fn find_token_index_at_offset(&self, offset: i32) -> i32 {
        self.actual
            .find_token_index_at_offset(offset + self.first_char_offset) as i32
            - self.first_token_index as i32
    }

    pub fn get_standard_token_type(&self, token_index: i32) -> u32 {
        let index = usize::try_from(token_index + self.first_token_index as i32)
            .expect("o índice de token está antes do começo da linha");
        self.actual.get_standard_token_type(index)
    }

    /// `toIViewLineTokens`.
    pub fn to_i_view_line_tokens(&self) -> SliceLineTokens {
        self.actual
            .slice_and_inflate(self.first_char_offset, self.last_char_offset, 0)
    }
}

/// `IgnoreBracketsInTokens.value`.
const IGNORE_BRACKETS_IN_TOKENS: u32 =
    standard_token_type::COMMENT | standard_token_type::STRING | standard_token_type::REG_EX;

/// `ignoreBracketsInToken`.
pub fn ignore_brackets_in_token(standard_token_type: u32) -> bool {
    standard_token_type & IGNORE_BRACKETS_IN_TOKENS != 0
}
