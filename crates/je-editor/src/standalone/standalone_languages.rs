//! Porte de `src/vs/editor/standalone/browser/standaloneLanguages.ts`, no que os tokenizadores escritos por fora usam:
//! [`score`], os tipos de token ([`IToken`], [`ILineTokens`], [`IEncodedLineTokens`]), os provedores ([`TokensProvider`] e
//! [`EncodedTokensProvider`]) e os adaptadores que os transformam num [`ITokenizationSupport`]
//! ([`TokenizationSupportAdapter`] e [`EncodedTokenizationSupportAdapter`]).
//!
//! As funções que dependem do singleton `StandaloneServices` (`register`, `getLanguages`, `onLanguage`,
//! `setLanguageConfiguration`, `setTokensProvider`, `setMonarchTokensProvider`, `setColorMap` e os `register*Provider`) ficam
//! com o contêiner de serviços do editor standalone, que o porte ainda não monta; elas só encadeiam os serviços que já existem
//! (`ModesRegistry`, `LanguageFeaturesService`, `TokenizationRegistry`).
//!
//! Divergências de forma, nenhuma de comportamento:
//! - **`IToken.startIndex: number`** é `usize`. O original sanitiza qualquer valor (negativo inclusive) de um tokenizador com
//!   defeito: o primeiro token vira 0 e um índice menor que o anterior vira o anterior. Com `usize` o índice negativo não
//!   existe, e os dois ajustes acontecem igual.
//! - **O texto da linha** é [`JsString`] (unidades UTF-16), o mesmo do [`ITokenizationSupport`].
//! - **`EncodedTokensProvider.tokenize?`** (método opcional) é [`EncodedTokensProvider::tokenize`] devolvendo `Option`: `None`
//!   é a ausência do método, e o `tokenize` do adaptador lança `Not supported!`, como o original.
//! - **`TokenizationSupportAdapter.adaptTokenize(language, actual, line, state)`** recebe o resultado já calculado
//!   (`actual.tokenize(line, state)`), que o chamador obtém de [`TokensProvider`] ou de [`EncodedTokensProvider`].
//! - **`score`** devolve a pontuação de [`crate::language_selector::score`] com o documento sincronizado e sem caderno.

use std::rc::Rc;

use je_base::JsString;
use je_base::errors::{self, Error};
use je_base::lifecycle::IDisposable;
use je_base::uri::URI;

use super::standalone_theme::IStandaloneThemeService;
use crate::encoded_token_attributes::metadata_consts;
use crate::language_selector::{LanguageSelector, score as score_language_selector};
use crate::language_service::ILanguageService;
use crate::languages::{EncodedTokenizationResult, ILanguageIdCodec, ITokenizationSupport, State, Token, TokenizationResult};

/// `score(selector, candidateUri, candidateLanguage)`: a pontuação de um seletor de linguagem contra um documento.
pub fn score(selector: Option<&LanguageSelector>, candidate_uri: &URI, candidate_language: &str) -> i32 {
    score_language_selector(selector, candidate_uri, candidate_language, true, None, None)
}

/// `IToken`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IToken {
    pub start_index: usize,
    pub scopes: String,
}

/// `ILineTokens`: o resultado da tokenização de uma linha. O tokenizador não deve alterar o `end_state` depois de devolvê-lo.
#[derive(Clone)]
pub struct ILineTokens {
    /// A lista de tokens da linha.
    pub tokens: Vec<IToken>,
    /// O estado ao fim da tokenização.
    pub end_state: State,
}

/// `IEncodedLineTokens`: o resultado da tokenização de uma linha em forma binária.
///
/// Cada token ocupa dois índices de `tokens`: no deslocamento `2*i` o `startIndex`, em `2*i + 1` o metadado, que é
/// binário:
///
/// ```text
///     3322 2222 2222 1111 1111 1100 0000 0000
///     1098 7654 3210 9876 5432 1098 7654 3210
///     bbbb bbbb bfff ffff ffFF FFTT LLLL LLLL
/// ```
///
/// - L = EncodedLanguageId (8 bits).
/// - T = StandardTokenType (2 bits): Other = 0, Comment = 1, String = 2, RegEx = 3.
/// - F = FontStyle (4 bits): None = 0, Italic = 1, Bold = 2, Underline = 4, Strikethrough = 8.
/// - f = foreground ColorId (9 bits).
/// - b = background ColorId (9 bits).
#[derive(Clone)]
pub struct IEncodedLineTokens {
    pub tokens: Vec<u32>,
    pub end_state: State,
}

/// `TokensProvider`: um provedor "manual" de tokens.
pub trait TokensProvider {
    /// O estado inicial da linguagem, passado para tokenizar a primeira linha.
    fn get_initial_state(&self) -> State;

    /// Tokeniza uma linha dado o estado no começo dela.
    fn tokenize(&self, line: &JsString, state: &State) -> ILineTokens;
}

/// `EncodedTokensProvider`: um provedor "manual" de tokens que devolve os tokens em forma binária.
pub trait EncodedTokensProvider {
    /// O estado inicial da linguagem, passado para tokenizar a primeira linha.
    fn get_initial_state(&self) -> State;

    /// Tokeniza uma linha dado o estado no começo dela.
    fn tokenize_encoded(&self, line: &JsString, state: &State) -> IEncodedLineTokens;

    /// `tokenize?`: a tokenização clássica, que o provedor pode não ter (`None`).
    fn tokenize(&self, _line: &JsString, _state: &State) -> Option<ILineTokens> {
        None
    }
}

/// O `startIndex` de um token depois de contida a falha de um tokenizador externo (`i` é a posição do token na linha).
fn sanitized_start_index(i: usize, start_index: usize, previous_start_index: usize) -> usize {
    // Prevent issues stemming from a buggy external tokenizer.
    if i == 0 {
        // Force first token to start at first index!
        0
    } else if start_index < previous_start_index {
        // Force tokens to be after one another!
        previous_start_index
    } else {
        start_index
    }
}

/// O estado final da tokenização: o próprio `state` quando o tokenizador devolve um igual, para economizar um objeto.
fn reuse_state(actual_end_state: State, state: &State) -> State {
    // try to save an object if possible
    if actual_end_state.equals(&**state) { state.clone() } else { actual_end_state }
}

/// `TokenizationSupportAdapter.adaptTokenize` (ver a nota no topo do módulo): os tokens clássicos sanitizados e o estado final.
fn adapt_tokenize(language: &str, actual_result: ILineTokens, state: &State) -> TokenizationResult {
    let tokens = to_classic_tokens(&actual_result.tokens, language);
    TokenizationResult { tokens, end_state: reuse_state(actual_result.end_state, state) }
}

/// `_toClassicTokens`.
fn to_classic_tokens(tokens: &[IToken], language: &str) -> Vec<Token> {
    let mut result = Vec::with_capacity(tokens.len());
    let mut previous_start_index = 0;
    for (i, t) in tokens.iter().enumerate() {
        let start_index = sanitized_start_index(i, t.start_index, previous_start_index);
        result.push(Token::new(start_index, &t.scopes, language));
        previous_start_index = start_index;
    }
    result
}

/// `EncodedTokenizationSupportAdapter`.
pub struct EncodedTokenizationSupportAdapter {
    language_id: String,
    actual: Rc<dyn EncodedTokensProvider>,
}

impl EncodedTokenizationSupportAdapter {
    pub fn new(language_id: &str, actual: Rc<dyn EncodedTokensProvider>) -> Self {
        Self { language_id: language_id.to_owned(), actual }
    }
}

impl IDisposable for EncodedTokenizationSupportAdapter {
    fn dispose(&self) {
        // NOOP
    }
}

impl ITokenizationSupport for EncodedTokenizationSupportAdapter {
    fn get_initial_state(&self) -> State {
        self.actual.get_initial_state()
    }

    fn tokenize(&self, line: &JsString, _has_eol: bool, state: &State) -> TokenizationResult {
        match self.actual.tokenize(line, state) {
            Some(actual_result) => adapt_tokenize(&self.language_id, actual_result, state),
            None => errors::throw(Error::new("Not supported!")),
        }
    }

    fn tokenize_encoded(&self, line: &JsString, _has_eol: bool, state: &State) -> EncodedTokenizationResult {
        let result = self.actual.tokenize_encoded(line, state);
        EncodedTokenizationResult { tokens: result.tokens, font_info: Vec::new(), end_state: result.end_state }
    }
}

/// `TokenizationSupportAdapter`.
pub struct TokenizationSupportAdapter {
    language_id: String,
    actual: Rc<dyn TokensProvider>,
    language_service: Rc<dyn ILanguageService>,
    standalone_theme_service: Rc<dyn IStandaloneThemeService>,
}

impl TokenizationSupportAdapter {
    pub fn new(
        language_id: &str,
        actual: Rc<dyn TokensProvider>,
        language_service: Rc<dyn ILanguageService>,
        standalone_theme_service: Rc<dyn IStandaloneThemeService>,
    ) -> Self {
        Self { language_id: language_id.to_owned(), actual, language_service, standalone_theme_service }
    }

    /// `_toBinaryTokens`: pares `(startIndex, metadado)` com os tokens de mesmo metadado seguidos fundidos num só e a mesma
    /// sanitização dos índices da forma clássica.
    fn to_binary_tokens(&self, language_id_codec: &dyn ILanguageIdCodec, tokens: &[IToken]) -> Vec<u32> {
        let language_id = language_id_codec.encode_language_id(&self.language_id);
        let token_theme = self.standalone_theme_service.get_standalone_color_theme().token_theme();

        let mut result: Vec<u32> = Vec::new();
        let mut previous_start_index = 0;
        for (i, t) in tokens.iter().enumerate() {
            let metadata = token_theme.match_token(language_id, &t.scopes) | metadata_consts::BALANCED_BRACKETS_MASK;
            if result.last() == Some(&metadata) {
                // same metadata
                continue;
            }

            let start_index = sanitized_start_index(i, t.start_index, previous_start_index);

            result.push(u32::try_from(start_index).unwrap_or(u32::MAX));
            result.push(metadata);

            previous_start_index = start_index;
        }

        result
    }
}

impl IDisposable for TokenizationSupportAdapter {
    fn dispose(&self) {
        // NOOP
    }
}

impl ITokenizationSupport for TokenizationSupportAdapter {
    fn get_initial_state(&self) -> State {
        self.actual.get_initial_state()
    }

    fn tokenize(&self, line: &JsString, _has_eol: bool, state: &State) -> TokenizationResult {
        adapt_tokenize(&self.language_id, self.actual.tokenize(line, state), state)
    }

    fn tokenize_encoded(&self, line: &JsString, _has_eol: bool, state: &State) -> EncodedTokenizationResult {
        let actual_result = self.actual.tokenize(line, state);
        let tokens = self.to_binary_tokens(&*self.language_service.language_id_codec(), &actual_result.tokens);
        EncodedTokenizationResult { tokens, font_info: Vec::new(), end_state: reuse_state(actual_result.end_state, state) }
    }
}
