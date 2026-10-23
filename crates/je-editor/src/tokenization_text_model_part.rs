//! Porte de `src/vs/editor/common/model/tokens/tokenizationTextModelPart.ts`: a parte do modelo de texto
//! que cuida da linguagem e da tokenização (`getLineTokens`, `forceTokenization`, `getLanguageIdAtPosition`,
//! `setLanguageId`, `getWordAtPosition` e os eventos `onDidChangeTokens`/`onDidChangeLanguage`).
//!
//! Divergências:
//! - O `BracketPairsTextModelPart` é o do modelo (`model.bracket_pairs()`), e não um parâmetro do
//!   construtor: o modelo o cria antes desta parte e a partir dele as notificações seguem.
//! - A definição de palavra de cada linguagem vem de um provedor ([`WordDefinitionProvider`]), que o
//!   `TextModel` liga ao `ILanguageConfigurationService`.

use crate::cursor_common::CursorSimpleModel;
use std::cell::RefCell;
use std::rc::{Rc, Weak};

use je_base::JsString;
use je_base::errors::{self, Error};
use je_base::event::{Emitter, Event};
use je_base::lifecycle::{Disposable, IDisposable};

use crate::attached_views::AttachedViews;
use crate::eol_counter::count_eol;
use je_base::js_regexp::JsRegExp;
use crate::language_service::ILanguageService;
use crate::line_tokens::{IViewLineTokens, LineTokens};
use crate::position::Position;
use crate::range::Range;
use crate::sparse_multiline_tokens::SparseMultilineTokens;
use crate::sparse_tokens_store::SparseTokensStore;
use crate::text_model::TextModel;
use crate::text_model_events::{
    ModelContentChangedEvent, ModelFontTokensChangedEvent, ModelLanguageChangedEvent, ModelLanguageConfigurationChangedEvent,
    ModelTokensChangedEvent, ModelTokensChangedRange,
};
use crate::tokenizer_syntax_token_backend::{BackgroundTokenizationState, TokenizerSyntaxTokenBackend};
use crate::word_helper::{WordAtPosition, get_word_at_text};

/// Devolve a definição de palavra (`wordPattern`) de uma linguagem.
pub type WordDefinitionProvider = Rc<dyn Fn(&str) -> Rc<JsRegExp>>;

/// `TokenizationTextModelPart`.
pub struct TokenizationTextModelPart {
    base: Rc<Disposable>,
    text_model: Weak<TextModel>,
    language_id: Rc<RefCell<String>>,
    word_definition: WordDefinitionProvider,
    tokens: Rc<TokenizerSyntaxTokenBackend>,
    semantic_tokens: RefCell<SparseTokensStore>,
    on_did_change_language: Rc<Emitter<ModelLanguageChangedEvent>>,
    on_did_change_language_configuration: Rc<Emitter<ModelLanguageConfigurationChangedEvent>>,
    on_did_change_tokens: Rc<Emitter<ModelTokensChangedEvent>>,
    on_did_change_font_tokens: Rc<Emitter<ModelFontTokensChangedEvent>>,
}

impl TokenizationTextModelPart {
    pub fn new(
        text_model: Weak<TextModel>,
        language_id: &str,
        attached_views: &Rc<AttachedViews>,
        language_service: &Rc<dyn ILanguageService>,
        word_definition: WordDefinitionProvider,
    ) -> Rc<Self> {
        let base = Disposable::new();
        let language_id = Rc::new(RefCell::new(language_id.to_owned()));
        let on_did_change_language = Emitter::new();
        let on_did_change_language_configuration = Emitter::new();
        let on_did_change_tokens = Emitter::new();
        let on_did_change_font_tokens = Emitter::new();
        base.register(on_did_change_language.clone());
        base.register(on_did_change_language_configuration.clone());
        base.register(on_did_change_tokens.clone());
        base.register(on_did_change_font_tokens.clone());

        let id_for_backend = language_id.clone();
        let tokens = TokenizerSyntaxTokenBackend::new(
            language_service.language_id_codec(),
            text_model.clone(),
            Rc::new(move || id_for_backend.borrow().clone()),
            attached_views,
        );
        base.register(tokens.clone());

        let part = Rc::new(Self {
            base,
            text_model,
            language_id,
            word_definition,
            tokens,
            semantic_tokens: RefCell::new(SparseTokensStore::new(language_service.language_id_codec())),
            on_did_change_language,
            on_did_change_language_configuration,
            on_did_change_tokens,
            on_did_change_font_tokens,
        });

        let weak = Rc::downgrade(&part);
        part.base.register(part.tokens.on_did_change_tokens().listen(move |e| {
            if let Some(part) = weak.upgrade() {
                part.emit_model_tokens_changed_event(e);
            }
        }));
        let weak = Rc::downgrade(&part);
        part.base.register(part.tokens.on_did_change_background_tokenization_state().listen(move |_| {
            if let Some(part) = weak.upgrade() {
                part.model().bracket_pairs().handle_did_change_background_tokenization_state();
            }
        }));
        let weak = Rc::downgrade(&part);
        part.base.register(part.tokens.on_did_change_font_tokens().listen(move |e| {
            if let Some(part) = weak.upgrade()
                && !part.model().is_disposing()
            {
                part.on_did_change_font_tokens.fire(e.clone());
            }
        }));
        part
    }

    fn model(&self) -> Rc<TextModel> {
        self.text_model.upgrade().expect("o modelo de texto foi descartado")
    }

    pub fn on_did_change_language(&self) -> Event<ModelLanguageChangedEvent> {
        self.on_did_change_language.event()
    }

    pub fn on_did_change_language_configuration(&self) -> Event<ModelLanguageConfigurationChangedEvent> {
        self.on_did_change_language_configuration.event()
    }

    pub fn on_did_change_tokens(&self) -> Event<ModelTokensChangedEvent> {
        self.on_did_change_tokens.event()
    }

    pub fn on_did_change_font_tokens(&self) -> Event<ModelFontTokensChangedEvent> {
        self.on_did_change_font_tokens.event()
    }

    /// `_hasListeners()`.
    pub fn has_listeners(&self) -> bool {
        self.on_did_change_language.has_listeners()
            || self.on_did_change_language_configuration.has_listeners()
            || self.on_did_change_tokens.has_listeners()
    }

    /// `handleLanguageConfigurationServiceChange(e)`: `affects` diz se a mudança vale para a linguagem do modelo.
    pub fn handle_language_configuration_service_change(&self, affects_language: impl Fn(&str) -> bool) {
        if affects_language(&self.language_id.borrow()) {
            self.on_did_change_language_configuration.fire(ModelLanguageConfigurationChangedEvent);
        }
    }

    pub fn handle_did_change_content(&self, e: &ModelContentChangedEvent) {
        if e.is_flush {
            self.semantic_tokens.borrow_mut().flush();
        } else if !e.is_eol_change {
            // We don't have to do anything on an EOL change
            for c in &e.changes {
                let counted = count_eol(c.text.units());
                self.semantic_tokens.borrow_mut().accept_edit(
                    &c.range,
                    counted.eol_count as i32,
                    counted.first_line_length as i32,
                    counted.last_line_length as i32,
                    c.text.units().first().copied().unwrap_or(0),
                );
            }
        }
        self.tokens.handle_did_change_content(e);
    }

    pub fn handle_did_change_attached(&self) {
        self.tokens.handle_did_change_attached();
    }

    /// `getLineTokens(lineNumber)`.
    pub fn get_line_tokens(&self, line_number: i32) -> LineTokens {
        self.validate_line_number(line_number);
        let syntactic_tokens = self.tokens.get_line_tokens(line_number);
        self.semantic_tokens.borrow().add_sparse_tokens(line_number, syntactic_tokens)
    }

    fn emit_model_tokens_changed_event(&self, e: &ModelTokensChangedEvent) {
        let model = self.model();
        if !model.is_disposing() {
            model.bracket_pairs().handle_did_change_tokens(e);
            self.on_did_change_tokens.fire(e.clone());
        }
    }

    fn validate_line_number(&self, line_number: i32) {
        if line_number < 1 || line_number > self.model().get_line_count() {
            errors::throw(Error::bug_indicating(Some("Illegal value for lineNumber")));
        }
    }

    pub fn has_tokens(&self) -> bool {
        self.tokens.has_tokens()
    }

    pub fn reset_tokenization(&self) {
        self.tokens.todo_reset_tokenization(true);
    }

    pub fn background_tokenization_state(&self) -> BackgroundTokenizationState {
        self.tokens.background_tokenization_state()
    }

    /// `onDidChangeBackgroundTokenizationState`.
    pub fn on_did_change_background_tokenization_state(&self) -> Event<()> {
        self.tokens.on_did_change_background_tokenization_state()
    }

    pub fn force_tokenization(&self, line_number: i32) {
        self.validate_line_number(line_number);
        self.tokens.force_tokenization(line_number);
    }

    pub fn has_accurate_tokens_for_line(&self, line_number: i32) -> bool {
        self.validate_line_number(line_number);
        self.tokens.has_accurate_tokens_for_line(line_number)
    }

    pub fn is_cheap_to_tokenize(&self, line_number: i32) -> bool {
        self.validate_line_number(line_number);
        self.tokens.is_cheap_to_tokenize(line_number)
    }

    pub fn tokenize_if_cheap(&self, line_number: i32) {
        self.validate_line_number(line_number);
        self.tokens.tokenize_if_cheap(line_number);
    }

    pub fn get_token_type_if_inserting_character(&self, line_number: i32, column: i32, character: &JsString) -> u32 {
        self.tokens.get_token_type_if_inserting_character(line_number, column, character)
    }

    pub fn tokenize_lines_at(&self, line_number: i32, lines: &[JsString]) -> Option<Vec<LineTokens>> {
        self.tokens.tokenize_lines_at(line_number, lines)
    }

    /// `setSemanticTokens(tokens, isComplete)`.
    pub fn set_semantic_tokens(&self, tokens: Option<Vec<SparseMultilineTokens>>, is_complete: bool) {
        let model = self.model();
        let applied = tokens.is_some();
        self.semantic_tokens.borrow_mut().set(tokens, is_complete, Some(&model));
        self.emit_model_tokens_changed_event(&ModelTokensChangedEvent {
            semantic_tokens_applied: applied,
            ranges: vec![ModelTokensChangedRange { from_line_number: 1, to_line_number: model.get_line_count() }],
        });
    }

    /// `hasCompleteSemanticTokens()`.
    pub fn has_complete_semantic_tokens(&self) -> bool {
        self.semantic_tokens.borrow().is_complete()
    }

    /// `hasSomeSemanticTokens()`.
    pub fn has_some_semantic_tokens(&self) -> bool {
        !self.semantic_tokens.borrow().is_empty()
    }

    /// `setPartialSemanticTokens(range, tokens)`.
    pub fn set_partial_semantic_tokens(&self, range: &Range, tokens: Vec<SparseMultilineTokens>) {
        if self.has_complete_semantic_tokens() {
            return;
        }
        let model = self.model();
        let changed_range = model.validate_range(&self.semantic_tokens.borrow_mut().set_partial(range, tokens));
        self.emit_model_tokens_changed_event(&ModelTokensChangedEvent {
            semantic_tokens_applied: true,
            ranges: vec![ModelTokensChangedRange {
                from_line_number: changed_range.start_line_number,
                to_line_number: changed_range.end_line_number,
            }],
        });
    }

    /// `getWordAtPosition(position)`.
    pub fn get_word_at_position(&self, position: Position) -> Option<WordAtPosition> {
        let model = self.model();
        model.assert_not_disposed();

        let validated = model.validate_position(position);
        let line_content = model.get_line_content(validated.line_number);
        let line_tokens = self.get_line_tokens(validated.line_number);
        let token_index = line_tokens.find_token_index_at_offset(validated.column - 1);

        let (rb_start_offset, rb_end_offset) = Self::find_language_boundaries(&line_tokens, token_index);
        let right_biased_word = get_word_at_text(
            validated.column,
            &(self.word_definition)(&line_tokens.get_language_id(token_index)),
            &line_content.units()[rb_start_offset as usize..rb_end_offset as usize],
            rb_start_offset,
            None,
        );
        if let Some(word) = right_biased_word
            && word.start_column <= position.column
            && position.column <= word.end_column
        {
            return Some(word);
        }

        // Look at the left-biased word if there is one
        if token_index > 0 && rb_start_offset == validated.column - 1 {
            let (lb_start_offset, lb_end_offset) = Self::find_language_boundaries(&line_tokens, token_index - 1);
            let left_biased_word = get_word_at_text(
                validated.column,
                &(self.word_definition)(&line_tokens.get_language_id(token_index - 1)),
                &line_content.units()[lb_start_offset as usize..lb_end_offset as usize],
                lb_start_offset,
                None,
            );
            if let Some(word) = left_biased_word
                && word.start_column <= position.column
                && position.column <= word.end_column
            {
                return Some(word);
            }
        }

        None
    }

    /// `_findLanguageBoundaries(lineTokens, tokenIndex)`.
    fn find_language_boundaries(line_tokens: &LineTokens, token_index: usize) -> (i32, i32) {
        let language_id = line_tokens.get_language_id(token_index);

        // go left until a different language is hit
        let mut start_offset = 0;
        let mut i = token_index as i64;
        while i >= 0 && line_tokens.get_language_id(i as usize) == language_id {
            start_offset = line_tokens.get_start_offset(i as usize);
            i -= 1;
        }

        // go right until a different language is hit
        let mut end_offset = line_tokens.get_text_length() as i32;
        let token_count = line_tokens.get_count();
        let mut i = token_index;
        while i < token_count && line_tokens.get_language_id(i) == language_id {
            end_offset = line_tokens.get_end_offset(i);
            i += 1;
        }
        (start_offset, end_offset)
    }

    /// `getWordUntilPosition(position)`.
    pub fn get_word_until_position(&self, position: Position) -> WordAtPosition {
        match self.get_word_at_position(position) {
            None => WordAtPosition { word: JsString::new(), start_column: position.column, end_column: position.column },
            Some(word_at_position) => {
                let len = ((position.column - word_at_position.start_column).max(0) as usize).min(word_at_position.word.len());
                WordAtPosition { word: word_at_position.word.slice(0, len), start_column: word_at_position.start_column, end_column: position.column }
            }
        }
    }

    /// `getLanguageId()`.
    pub fn get_language_id(&self) -> String {
        self.language_id.borrow().clone()
    }

    /// `getLanguageIdAtPosition(lineNumber, column)`.
    pub fn get_language_id_at_position(&self, line_number: i32, column: i32) -> String {
        let position = self.model().validate_position(Position::new(line_number, column));
        let line_tokens = self.get_line_tokens(position.line_number);
        line_tokens.get_language_id(line_tokens.find_token_index_at_offset(position.column - 1))
    }

    /// `setLanguageId(languageId, source)`.
    pub fn set_language_id(&self, language_id: &str, source: &str) {
        if *self.language_id.borrow() == language_id {
            return;
        }

        let e = ModelLanguageChangedEvent {
            old_language: self.language_id.borrow().clone(),
            new_language: language_id.to_owned(),
            source: source.to_owned(),
        };

        *self.language_id.borrow_mut() = language_id.to_owned();
        // A troca de linguagem refaz a tokenização: no original o observável `_languageIdObs` recria os tokens.
        self.tokens.todo_reset_tokenization(true);
        self.model().bracket_pairs().handle_did_change_language(&e);

        self.on_did_change_language.fire(e);
        self.on_did_change_language_configuration.fire(ModelLanguageConfigurationChangedEvent);
    }

    /// `getStandardTokenTypeAtPosition`: o tipo de token padrão na posição, se for barato tokenizar a linha.
    pub fn get_standard_token_type_at_position(&self, position: Position) -> Option<u32> {
        let line_number = position.line_number;
        if !self.is_cheap_to_tokenize(line_number) {
            return None;
        }
        self.force_tokenization(line_number);
        let line_tokens = self.get_line_tokens(line_number);
        let token_index = line_tokens.find_token_index_at_offset(position.column - 1);
        Some(line_tokens.get_standard_token_type(token_index))
    }
}

impl IDisposable for TokenizationTextModelPart {
    fn dispose(&self) {
        IDisposable::dispose(&*self.base);
    }
}

