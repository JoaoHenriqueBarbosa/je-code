//! Porte de `src/vs/editor/common/languages/nullTokenize.ts`: o tokenizador nulo.

use std::any::Any;
use std::rc::Rc;

use crate::encoded_token_attributes::{color_id, font_style, metadata_consts as mc, standard_token_type};
use crate::languages::{EncodedTokenizationResult, IState, State, Token, TokenizationResult};

/// O tipo de `NullState`.
struct NullStateImpl;

impl IState for NullStateImpl {
    fn clone_state(&self) -> State {
        null_state()
    }

    fn equals(&self, other: &dyn IState) -> bool {
        // `this === other`: só o próprio singleton é igual a ele.
        other.as_any().is::<NullStateImpl>()
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

thread_local! {
    static NULL_STATE: State = Rc::new(NullStateImpl);
}

/// `NullState`: o estado único do tokenizador nulo.
pub fn null_state() -> State {
    NULL_STATE.with(Rc::clone)
}

/// `nullTokenize(languageId, state)`.
pub fn null_tokenize(language_id: &str, state: State) -> TokenizationResult {
    TokenizationResult { tokens: vec![Token::new(0, "", language_id)], end_state: state }
}

/// `nullTokenizeEncoded(languageId, state)`.
pub fn null_tokenize_encoded(language_id: u32, state: Option<State>) -> EncodedTokenizationResult {
    let metadata = (language_id << mc::LANGUAGEID_OFFSET)
        | (standard_token_type::OTHER << mc::TOKEN_TYPE_OFFSET)
        | (font_style::NONE << mc::FONT_STYLE_OFFSET)
        | (color_id::DEFAULT_FOREGROUND << mc::FOREGROUND_OFFSET)
        | (color_id::DEFAULT_BACKGROUND << mc::BACKGROUND_OFFSET);
    EncodedTokenizationResult { tokens: vec![0, metadata], font_info: Vec::new(), end_state: state.unwrap_or_else(null_state) }
}
