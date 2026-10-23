//! Porte de `src/vs/editor/test/common/modesTestUtils.ts`: `TokenText` e `createFakeScopedLineTokens`.

use std::rc::Rc;

use je_base::JsString;
use je_editor::encoded_token_attributes::metadata_consts;
use je_editor::languages::ILanguageIdCodec;
use je_editor::languages_registry::LanguageIdCodec;
use je_editor::line_tokens::LineTokens;
use je_editor::supports::{ScopedLineTokens, create_scoped_line_tokens};

pub struct TokenText {
    pub text: &'static str,
    pub type_: u32,
}

pub fn create_fake_scoped_line_tokens(raw_tokens: &[TokenText]) -> ScopedLineTokens {
    let mut tokens = vec![0u32; raw_tokens.len() << 1];
    let mut line = JsString::new();

    for (i, raw_token) in raw_tokens.iter().enumerate() {
        let start_offset = line.len() as u32;
        let metadata = raw_token.type_ << metadata_consts::TOKEN_TYPE_OFFSET;

        tokens[i << 1] = start_offset;
        tokens[(i << 1) + 1] = metadata;
        line.push_units(JsString::from(raw_token.text).units());
    }

    LineTokens::convert_to_end_offset(&mut tokens, line.len());
    let codec: Rc<dyn ILanguageIdCodec> = Rc::new(LanguageIdCodec::new());
    create_scoped_line_tokens(Rc::new(LineTokens::new(tokens, line, codec)), 0)
}
