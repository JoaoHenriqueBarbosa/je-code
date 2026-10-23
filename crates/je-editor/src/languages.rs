//! Porte da parte de tokenização de `src/vs/editor/common/languages.ts`: `ILanguageIdCodec`, `IState`,
//! `Token`, `TokenizationResult`, `EncodedTokenizationResult`, `ITokenizationSupport`,
//! `IBackgroundTokenizer`, `IBackgroundTokenizationStore` e `ILazyTokenizationSupport`.
//!
//! Decisões de tradução:
//! - `IState` vira o trait [`IState`] com `as_any` (o `equals` de cada implementação recorre ao
//!   downcast, como o `instanceof` do original); o estado compartilhado é `Rc<dyn IState>` ([`State`]).
//! - Os tokens codificados são `Vec<u32>` (o `Uint32Array`); o texto da linha é [`JsString`].
//! - `FontTokensUpdate` (`AnnotationsUpdate`, de `annotations.ts`) não foi portado: [`FontTokensUpdate`]
//!   é um marcador sem conteúdo, e nenhum tokenizador do porte produz informação de fonte.

use std::any::Any;
use std::rc::Rc;

use je_base::JsString;
use je_base::lifecycle::IDisposable;

use crate::contiguous_multiline_tokens::ContiguousMultilineTokens;
use crate::text_model::TextModel;

/// `ILanguageIdCodec`.
pub trait ILanguageIdCodec {
    fn encode_language_id(&self, language_id: &str) -> u32;
    fn decode_language_id(&self, language_id: u32) -> String;
}

/// `IState`.
pub trait IState: Any {
    fn clone_state(&self) -> State;
    fn equals(&self, other: &dyn IState) -> bool;
    fn as_any(&self) -> &dyn Any;
}

/// O estado do tokenizador no fim de uma linha.
pub type State = Rc<dyn IState>;

/// `Token`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Token {
    pub offset: usize,
    pub type_: String,
    pub language: String,
}

impl Token {
    pub fn new(offset: usize, type_: &str, language: &str) -> Self {
        Self { offset, type_: type_.to_owned(), language: language.to_owned() }
    }
}

impl std::fmt::Display for Token {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "({}, {})", self.offset, self.type_)
    }
}

/// `TokenizationResult`.
#[derive(Clone)]
pub struct TokenizationResult {
    pub tokens: Vec<Token>,
    pub end_state: State,
}

/// `IFontToken`.
#[derive(Clone, Debug, PartialEq)]
pub struct FontToken {
    pub start_index: usize,
    pub end_index: usize,
    pub font_family: Option<String>,
    pub font_size_multiplier: Option<f64>,
    pub line_height_multiplier: Option<f64>,
}

/// `EncodedTokenizationResult`.
#[derive(Clone)]
pub struct EncodedTokenizationResult {
    pub tokens: Vec<u32>,
    pub font_info: Vec<FontToken>,
    pub end_state: State,
}

/// `FontTokensUpdate` (ver a nota do módulo).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FontTokensUpdate;

/// `ITokenizationSupport`.
pub trait ITokenizationSupport {
    /// `backgroundTokenizerShouldOnlyVerifyTokens`.
    fn background_tokenizer_should_only_verify_tokens(&self) -> bool {
        false
    }

    fn get_initial_state(&self) -> State;

    fn tokenize(&self, line: &JsString, has_eol: bool, state: &State) -> TokenizationResult;

    fn tokenize_encoded(&self, line: &JsString, has_eol: bool, state: &State) -> EncodedTokenizationResult;

    /// `createBackgroundTokenizer?`: `None` quando o suporte não tem tokenizador em segundo plano próprio.
    fn create_background_tokenizer(
        &self,
        _text_model: &Rc<TextModel>,
        _store: Rc<dyn IBackgroundTokenizationStore>,
    ) -> Option<Rc<dyn IBackgroundTokenizer>> {
        None
    }
}

/// `IBackgroundTokenizer`.
pub trait IBackgroundTokenizer: IDisposable {
    fn request_tokens(&self, start_line_number: i32, end_line_number_exclusive: i32);

    /// `reportMismatchingTokens?`: devolve `false` quando o tokenizador não tem o método.
    fn report_mismatching_tokens(&self, _line_number: i32) -> bool {
        false
    }

    /// Se o tokenizador implementa `reportMismatchingTokens`.
    fn has_report_mismatching_tokens(&self) -> bool {
        false
    }
}

/// `IBackgroundTokenizationStore`.
pub trait IBackgroundTokenizationStore {
    fn set_tokens(&self, tokens: Vec<ContiguousMultilineTokens>);
    fn set_font_info(&self, changes: FontTokensUpdate);
    fn set_end_state(&self, line_number: i32, state: State);
    fn background_tokenization_finished(&self);
}

/// `ITokenizationSupportChangedEvent`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TokenizationSupportChangedEvent {
    pub changed_languages: Vec<String>,
    pub changed_color_map: bool,
}
