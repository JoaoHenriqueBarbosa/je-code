//! Porte de `src/vs/workbench/services/textMate/browser/tokenizationSupport/textMateTokenizationSupport.ts`
//! e de `tokenizationSupportWithLineLimit.ts`: o `ITokenizationSupport` que liga uma gramática TextMate
//! (`ferriki-textmate`) ao registro de tokenização do editor.
//!
//! Divergências:
//! - O tokenizador em segundo plano num worker (`ThreadedBackgroundTokenizerFactory`) não foi portado:
//!   `create_background_tokenizer` devolve `None` e o `TokenizerSyntaxTokenBackend` usa o
//!   `DefaultBackgroundTokenizer` no thread da UI, por fatias com orçamento de tempo.
//! - O relatório de tempo de tokenização (telemetria por amostragem) não existe.
//! - O `StateStack` do ferriki compartilha por `Arc` (a pilha é imutável); o estado do editor é um
//!   `Rc<dyn IState>` que o embrulha ([`TmState`]).

use std::any::Any;
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;

use ferriki_textmate::{Grammar, StateStack};
use je_base::JsString;
use je_base::errors::{self, Error};
use je_base::event::{Emitter, Event};
use je_base::lifecycle::{Disposable, IDisposable};
use je_editor::encoded_token_attributes::TokenMetadata;
use je_editor::languages::{
    EncodedTokenizationResult, FontToken, IState, ITokenizationSupport, State, TokenizationResult,
};
use je_editor::null_tokenize::null_tokenize_encoded;

/// O limite de tempo, em milissegundos, que o `tokenizeLine2` recebe por linha.
const TOKENIZE_TIME_LIMIT_MS: u64 = 500;

/// O `StateStack` do vscode-textmate como `IState` do editor.
pub struct TmState(pub Arc<StateStack>);

impl IState for TmState {
    fn clone_state(&self) -> State {
        Rc::new(TmState(self.0.clone_stack()))
    }

    fn equals(&self, other: &dyn IState) -> bool {
        other.as_any().downcast_ref::<TmState>().is_some_and(|o| self.0.equals(&o.0))
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// `TextMateTokenizationSupport`.
pub struct TextMateTokenizationSupport {
    base: Rc<Disposable>,
    grammar: Rc<Grammar>,
    initial_state: State,
    contains_embedded_languages: bool,
    background_tokenizer_should_only_verify_tokens: bool,
    seen_languages: RefCell<Vec<bool>>,
    on_did_encounter_language: Rc<Emitter<u32>>,
}

impl TextMateTokenizationSupport {
    pub fn new(
        grammar: Rc<Grammar>,
        initial_state: Arc<StateStack>,
        contains_embedded_languages: bool,
        background_tokenizer_should_only_verify_tokens: bool,
    ) -> Rc<Self> {
        let base = Disposable::new();
        let on_did_encounter_language = Emitter::new();
        base.register(on_did_encounter_language.clone());
        Rc::new(Self {
            base,
            grammar,
            initial_state: Rc::new(TmState(initial_state)),
            contains_embedded_languages,
            background_tokenizer_should_only_verify_tokens,
            seen_languages: RefCell::new(Vec::new()),
            on_did_encounter_language,
        })
    }

    /// `onDidEncounterLanguage`: dispara na primeira vez que um token de uma linguagem (embutida) aparece.
    pub fn on_did_encounter_language(&self) -> Event<u32> {
        self.on_did_encounter_language.event()
    }
}

impl IDisposable for TextMateTokenizationSupport {
    fn dispose(&self) {
        IDisposable::dispose(&*self.base);
    }
}

impl ITokenizationSupport for TextMateTokenizationSupport {
    fn background_tokenizer_should_only_verify_tokens(&self) -> bool {
        self.background_tokenizer_should_only_verify_tokens
    }

    fn get_initial_state(&self) -> State {
        self.initial_state.clone()
    }

    fn tokenize(&self, _line: &JsString, _has_eol: bool, _state: &State) -> TokenizationResult {
        errors::throw(Error::new("Not supported!"))
    }

    fn tokenize_encoded(&self, line: &JsString, _has_eol: bool, state: &State) -> EncodedTokenizationResult {
        let Some(tm_state) = state.as_any().downcast_ref::<TmState>() else {
            errors::throw(Error::new("Estado de tokenização que não é do TextMate"))
        };
        let text = line.to_string_lossy();
        let text_mate_result = match self.grammar.tokenize_line2(&text, Some(tm_state.0.clone()), TOKENIZE_TIME_LIMIT_MS) {
            Ok(result) => result,
            Err(e) => errors::throw(Error::new(e.to_string())),
        };

        let fonts: Vec<FontToken> = text_mate_result
            .fonts
            .iter()
            .map(|f| FontToken {
                start_index: f.start_index,
                end_index: f.end_index,
                font_family: f.font_family.clone(),
                font_size_multiplier: f.font_size_multiplier,
                line_height_multiplier: f.line_height_multiplier,
            })
            .collect();

        if text_mate_result.stopped_early {
            eprintln!("Time limit reached when tokenizing line: {}", text.chars().take(100).collect::<String>());
            // Return the original state, so that the next line is tokenized from the same place.
            return EncodedTokenizationResult { tokens: text_mate_result.tokens, font_info: fonts, end_state: state.clone() };
        }

        if self.contains_embedded_languages {
            let mut seen = self.seen_languages.borrow_mut();
            let tokens = &text_mate_result.tokens;
            for i in 0..tokens.len() >> 1 {
                let language_id = TokenMetadata::get_language_id(tokens[(i << 1) + 1]) as usize;
                if seen.len() <= language_id {
                    seen.resize(language_id + 1, false);
                }
                if !seen[language_id] {
                    seen[language_id] = true;
                    // O listener pode pedir recursos ao serviço de linguagem: solta o empréstimo antes.
                    drop(seen);
                    self.on_did_encounter_language.fire(language_id as u32);
                    seen = self.seen_languages.borrow_mut();
                }
            }
        }

        let end_state: State = if tm_state.0.equals(&text_mate_result.rule_stack) {
            state.clone()
        } else {
            Rc::new(TmState(text_mate_result.rule_stack))
        };
        EncodedTokenizationResult { tokens: text_mate_result.tokens, font_info: fonts, end_state }
    }
}

/// `TokenizationSupportWithLineLimit`: linhas a partir de um comprimento não são tokenizadas.
pub struct TokenizationSupportWithLineLimit {
    base: Rc<Disposable>,
    encoded_language_id: u32,
    actual: Rc<dyn ITokenizationSupport>,
    max_tokenization_line_length: Cell<i64>,
}

impl TokenizationSupportWithLineLimit {
    pub fn new(
        encoded_language_id: u32,
        actual: Rc<dyn ITokenizationSupport>,
        disposable: Rc<dyn IDisposable>,
        max_tokenization_line_length: i64,
    ) -> Rc<Self> {
        let base = Disposable::new();
        base.register(je_base::lifecycle::to_disposable(move || disposable.dispose()));
        Rc::new(Self { base, encoded_language_id, actual, max_tokenization_line_length: Cell::new(max_tokenization_line_length) })
    }

    /// `editor.maxTokenizationLineLength` mudou.
    pub fn set_max_tokenization_line_length(&self, value: i64) {
        self.max_tokenization_line_length.set(value);
    }
}

impl IDisposable for TokenizationSupportWithLineLimit {
    fn dispose(&self) {
        IDisposable::dispose(&*self.base);
    }
}

impl ITokenizationSupport for TokenizationSupportWithLineLimit {
    fn background_tokenizer_should_only_verify_tokens(&self) -> bool {
        self.actual.background_tokenizer_should_only_verify_tokens()
    }

    fn get_initial_state(&self) -> State {
        self.actual.get_initial_state()
    }

    fn tokenize(&self, _line: &JsString, _has_eol: bool, _state: &State) -> TokenizationResult {
        errors::throw(Error::new("Not supported!"))
    }

    fn tokenize_encoded(&self, line: &JsString, has_eol: bool, state: &State) -> EncodedTokenizationResult {
        if line.len() as i64 >= self.max_tokenization_line_length.get() {
            return null_tokenize_encoded(self.encoded_language_id, Some(state.clone()));
        }
        self.actual.tokenize_encoded(line, has_eol, state)
    }

    fn create_background_tokenizer(
        &self,
        text_model: &Rc<je_editor::text_model::TextModel>,
        store: Rc<dyn je_editor::languages::IBackgroundTokenizationStore>,
    ) -> Option<Rc<dyn je_editor::languages::IBackgroundTokenizer>> {
        self.actual.create_background_tokenizer(text_model, store)
    }
}
