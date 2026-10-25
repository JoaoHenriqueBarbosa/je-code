//! Porte de `src/vs/editor/standalone/common/monarch/monarchLexer.ts`: o `MonarchTokenizer`, um realçador de
//! sintaxe com uma descrição declarativa em JSON, compilada por [`compile`](super::compile::compile).
//!
//! Decisões de tradução:
//! - Os estados da pilha e do tokenizador são [`JsString`] (podem carregar trechos casados da linha), e as
//!   fábricas de pilha e de estado de linha são `thread_local` (o original usa singletons estáticos).
//! - O `throw` do tokenizador é o pânico de [`je_base::errors::throw`] (o contrato de `ITokenizationSupport`
//!   não devolve `Result`); por dentro, as funções devolvem `Result<_, MonarchError>`.
//! - `IStandaloneThemeService` é o recorte que o lexer lê (`getColorTheme().tokenTheme`): [`IStandaloneThemeService`].
//!   O original aceita `null!` quando só se usa `tokenize` (o caminho clássico); aqui é um `Option` que o
//!   `tokenizeEncoded` exige.
//! - `MonarchLineState.clone()` devolve `this` quando nada muda; o estado guarda um `Weak` de si mesmo para isso.
//! - `tokenizationSupport instanceof MonarchTokenizer` é [`ITokenizationSupport::as_any`] com o downcast.

use std::any::Any;
use std::cell::{Cell, RefCell};
use std::collections::{HashMap, VecDeque};
use std::rc::{Rc, Weak};

use je_base::JsString;
use je_base::dynamic::Dynamic;
use je_base::errors::{self, Error};
use je_base::lifecycle::{Disposable, IDisposable};
use je_base::promise::Promise;
use je_platform::configuration::ConfigurationOverrides;
use je_platform::configuration::service::IConfigurationService;

use super::common::{FuzzyAction, IRule, Lexer, MonarchBracket, MonarchError, MonarchRegExp, create_error, find_rules, fix_case, log, sanitize, substitute_matches};
use crate::encoded_token_attributes::{language_id as language_id_consts, metadata_consts};
use crate::language_service::ILanguageService;
use crate::languages::{EncodedTokenizationResult, ITokenizationSupport, State, Token, TokenizationResult, IState};
use crate::null_tokenize::{null_state, null_tokenize, null_tokenize_encoded};
use crate::token_theme::TokenTheme;
use crate::tokenization_registry::tokenization_registry;

const CACHE_STACK_DEPTH: usize = 5;

/// O recorte de `IStandaloneThemeService` que o lexer lê: `getColorTheme().tokenTheme`.
pub trait IStandaloneThemeService {
    fn token_theme(&self) -> Rc<TokenTheme>;
}

fn throw_monarch(error: MonarchError) -> ! {
    errors::throw(Error::new(error.0))
}

fn unwrap_or_throw<T>(result: Result<T, MonarchError>) -> T {
    match result {
        Ok(value) => value,
        Err(error) => throw_monarch(error),
    }
}

/// Reuse the same stack elements up to a certain depth.
struct MonarchStackElementFactory;

thread_local! {
    static STACK_ENTRIES: RefCell<HashMap<JsString, Rc<MonarchStackElement>>> = RefCell::new(HashMap::new());
    static LINE_STATE_ENTRIES: RefCell<HashMap<JsString, Rc<MonarchLineState>>> = RefCell::new(HashMap::new());
}

impl MonarchStackElementFactory {
    fn create(parent: Option<&Rc<MonarchStackElement>>, state: &JsString) -> Rc<MonarchStackElement> {
        if let Some(parent) = parent
            && parent.depth >= CACHE_STACK_DEPTH
        {
            // no caching above a certain depth
            return Rc::new(MonarchStackElement::new(Some(parent.clone()), state.clone()));
        }
        let mut stack_element_id = MonarchStackElement::get_stack_element_id(parent);
        if !stack_element_id.is_empty() {
            stack_element_id.push_unit(u16::from(b'|'));
        }
        stack_element_id.push_units(state.units());

        if let Some(result) = STACK_ENTRIES.with(|entries| entries.borrow().get(&stack_element_id).cloned()) {
            return result;
        }
        let result = Rc::new(MonarchStackElement::new(parent.cloned(), state.clone()));
        STACK_ENTRIES.with(|entries| entries.borrow_mut().insert(stack_element_id, result.clone()));
        result
    }
}

struct MonarchStackElement {
    parent: Option<Rc<MonarchStackElement>>,
    state: JsString,
    depth: usize,
}

impl MonarchStackElement {
    fn new(parent: Option<Rc<MonarchStackElement>>, state: JsString) -> Self {
        let depth = parent.as_ref().map_or(0, |p| p.depth) + 1;
        Self { parent, state, depth }
    }

    fn get_stack_element_id(element: Option<&Rc<MonarchStackElement>>) -> JsString {
        let mut result = JsString::new();
        let mut element = element;
        while let Some(current) = element {
            if !result.is_empty() {
                result.push_unit(u16::from(b'|'));
            }
            result.push_units(current.state.units());
            element = current.parent.as_ref();
        }
        result
    }

    fn equals_elements(a: &Rc<MonarchStackElement>, b: &Rc<MonarchStackElement>) -> bool {
        let (mut a, mut b) = (Some(a), Some(b));
        while let (Some(left), Some(right)) = (a, b) {
            if Rc::ptr_eq(left, right) {
                return true;
            }
            if left.state != right.state {
                return false;
            }
            a = left.parent.as_ref();
            b = right.parent.as_ref();
        }
        a.is_none() && b.is_none()
    }

    fn push(self: &Rc<Self>, state: &JsString) -> Rc<MonarchStackElement> {
        MonarchStackElementFactory::create(Some(self), state)
    }

    fn pop(&self) -> Option<Rc<MonarchStackElement>> {
        self.parent.clone()
    }

    fn popall(self: &Rc<Self>) -> Rc<MonarchStackElement> {
        let mut result = self.clone();
        while let Some(parent) = result.parent.clone() {
            result = parent;
        }
        result
    }

    fn switch_to(&self, state: &JsString) -> Rc<MonarchStackElement> {
        MonarchStackElementFactory::create(self.parent.as_ref(), state)
    }
}

struct EmbeddedLanguageData {
    language_id: String,
    state: State,
}

impl EmbeddedLanguageData {
    fn equals(&self, other: &EmbeddedLanguageData) -> bool {
        self.language_id == other.language_id && self.state.equals(&*other.state)
    }

    fn clone_data(self: &Rc<Self>) -> Rc<EmbeddedLanguageData> {
        let state_clone = self.state.clone_state();
        // save an object
        if Rc::ptr_eq(&state_clone, &self.state) {
            return self.clone();
        }
        Rc::new(EmbeddedLanguageData { language_id: self.language_id.clone(), state: self.state.clone() })
    }
}

/// Reuse the same line states up to a certain depth.
struct MonarchLineStateFactory;

impl MonarchLineStateFactory {
    fn create(stack: &Rc<MonarchStackElement>, embedded_language_data: Option<Rc<EmbeddedLanguageData>>) -> Rc<MonarchLineState> {
        if embedded_language_data.is_some() {
            // no caching when embedding
            return MonarchLineState::new(stack.clone(), embedded_language_data);
        }
        if stack.depth >= CACHE_STACK_DEPTH {
            // no caching above a certain depth
            return MonarchLineState::new(stack.clone(), embedded_language_data);
        }
        let stack_element_id = MonarchStackElement::get_stack_element_id(Some(stack));

        if let Some(result) = LINE_STATE_ENTRIES.with(|entries| entries.borrow().get(&stack_element_id).cloned()) {
            return result;
        }
        let result = MonarchLineState::new(stack.clone(), None);
        LINE_STATE_ENTRIES.with(|entries| entries.borrow_mut().insert(stack_element_id, result.clone()));
        result
    }
}

struct MonarchLineState {
    me: Weak<MonarchLineState>,
    stack: Rc<MonarchStackElement>,
    embedded_language_data: Option<Rc<EmbeddedLanguageData>>,
}

impl MonarchLineState {
    fn new(stack: Rc<MonarchStackElement>, embedded_language_data: Option<Rc<EmbeddedLanguageData>>) -> Rc<Self> {
        Rc::new_cyclic(|me| Self { me: me.clone(), stack, embedded_language_data })
    }
}

impl IState for MonarchLineState {
    fn clone_state(&self) -> State {
        let embedded_language_data_clone = self.embedded_language_data.as_ref().map(|data| data.clone_data());
        // save an object
        let unchanged = match (&embedded_language_data_clone, &self.embedded_language_data) {
            (Some(a), Some(b)) => Rc::ptr_eq(a, b),
            (None, None) => true,
            _ => false,
        };
        if unchanged {
            return self.me.upgrade().expect("o estado está vivo enquanto é clonado");
        }
        MonarchLineStateFactory::create(&self.stack, self.embedded_language_data.clone())
    }

    fn equals(&self, other: &dyn IState) -> bool {
        let Some(other) = other.as_any().downcast_ref::<MonarchLineState>() else {
            return false;
        };
        if !MonarchStackElement::equals_elements(&self.stack, &other.stack) {
            return false;
        }
        match (&self.embedded_language_data, &other.embedded_language_data) {
            (None, None) => true,
            (Some(a), Some(b)) => a.equals(b),
            _ => false,
        }
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

trait IMonarchTokensCollector {
    fn enter_language(&mut self, language_id: &str);
    fn emit(&mut self, start_offset: usize, type_: &str);
    fn nested_language_tokenize(&mut self, embedded_language_line: &JsString, has_eol: bool, embedded_language_data: &EmbeddedLanguageData, offset_delta: usize) -> State;
}

struct MonarchClassicTokensCollector {
    tokens: Vec<Token>,
    language_id: Option<String>,
    last_token_type: Option<String>,
    last_token_language: Option<String>,
}

impl MonarchClassicTokensCollector {
    fn new() -> Self {
        Self { tokens: Vec::new(), language_id: None, last_token_type: None, last_token_language: None }
    }

    fn finalize(self, end_state: Rc<MonarchLineState>) -> TokenizationResult {
        TokenizationResult { tokens: self.tokens, end_state }
    }
}

impl IMonarchTokensCollector for MonarchClassicTokensCollector {
    fn enter_language(&mut self, language_id: &str) {
        self.language_id = Some(language_id.to_owned());
    }

    fn emit(&mut self, start_offset: usize, type_: &str) {
        if self.last_token_type.as_deref() == Some(type_) && self.last_token_language == self.language_id {
            return;
        }
        self.last_token_type = Some(type_.to_owned());
        self.last_token_language = self.language_id.clone();
        self.tokens.push(Token::new(start_offset, type_, self.language_id.as_deref().unwrap_or_default()));
    }

    fn nested_language_tokenize(&mut self, embedded_language_line: &JsString, has_eol: bool, embedded_language_data: &EmbeddedLanguageData, offset_delta: usize) -> State {
        let nested_language_id = &embedded_language_data.language_id;
        let embedded_mode_state = &embedded_language_data.state;

        let Some(nested_language_tokenization_support) = tokenization_registry().get(nested_language_id) else {
            self.enter_language(nested_language_id);
            self.emit(offset_delta, "");
            return embedded_mode_state.clone();
        };

        let nested_result = nested_language_tokenization_support.tokenize(embedded_language_line, has_eol, embedded_mode_state);
        for token in &nested_result.tokens {
            self.tokens.push(Token { offset: token.offset + offset_delta, type_: token.type_.clone(), language: token.language.clone() });
        }
        self.last_token_type = None;
        self.last_token_language = None;
        self.language_id = None;
        nested_result.end_state
    }
}

struct MonarchModernTokensCollector {
    language_service: Rc<dyn ILanguageService>,
    theme: Rc<TokenTheme>,
    prepend_tokens: Vec<u32>,
    tokens: Vec<u32>,
    current_language_id: u32,
    last_token_metadata: u32,
}

impl MonarchModernTokensCollector {
    fn new(language_service: Rc<dyn ILanguageService>, theme: Rc<TokenTheme>) -> Self {
        Self { language_service, theme, prepend_tokens: Vec::new(), tokens: Vec::new(), current_language_id: language_id_consts::NULL, last_token_metadata: 0 }
    }

    fn finalize(mut self, end_state: Rc<MonarchLineState>) -> EncodedTokenizationResult {
        self.prepend_tokens.append(&mut self.tokens);
        EncodedTokenizationResult { tokens: self.prepend_tokens, font_info: Vec::new(), end_state }
    }
}

impl IMonarchTokensCollector for MonarchModernTokensCollector {
    fn enter_language(&mut self, language_id: &str) {
        self.current_language_id = self.language_service.language_id_codec().encode_language_id(language_id);
    }

    fn emit(&mut self, start_offset: usize, type_: &str) {
        let metadata = self.theme.match_token(self.current_language_id, type_) | metadata_consts::BALANCED_BRACKETS_MASK;
        if self.last_token_metadata == metadata {
            return;
        }
        self.last_token_metadata = metadata;
        self.tokens.push(start_offset as u32);
        self.tokens.push(metadata);
    }

    fn nested_language_tokenize(&mut self, embedded_language_line: &JsString, has_eol: bool, embedded_language_data: &EmbeddedLanguageData, offset_delta: usize) -> State {
        let nested_language_id = &embedded_language_data.language_id;
        let embedded_mode_state = &embedded_language_data.state;

        let Some(nested_language_tokenization_support) = tokenization_registry().get(nested_language_id) else {
            self.enter_language(nested_language_id);
            self.emit(offset_delta, "");
            return embedded_mode_state.clone();
        };

        let mut nested_result = nested_language_tokenization_support.tokenize_encoded(embedded_language_line, has_eol, embedded_mode_state);
        if offset_delta != 0 {
            for offset in nested_result.tokens.iter_mut().step_by(2) {
                *offset += offset_delta as u32;
            }
        }

        // `_merge(prependTokens, tokens, nestedResult.tokens)`
        self.prepend_tokens.append(&mut self.tokens);
        self.prepend_tokens.append(&mut nested_result.tokens);
        self.current_language_id = 0;
        self.last_token_metadata = 0;
        nested_result.end_state
    }
}

/// `ILoadStatus`.
#[derive(Clone)]
pub enum LoadStatus {
    Loaded,
    Pending(Promise<()>),
}

/// `editor.maxTokenizationLineLength` como o número que o `line.length >= value` do JavaScript compara: o que
/// não é número vira o `ToNumber` da linguagem (`NaN` para o que não converte, e então a comparação é falsa).
fn to_number(value: &Dynamic) -> f64 {
    match value {
        Dynamic::Number(number) => *number,
        Dynamic::Null => 0.0,
        Dynamic::Bool(flag) => f64::from(u8::from(*flag)),
        Dynamic::String(text) => {
            let trimmed = je_base::js_string::trim(text.units());
            if trimmed.is_empty() { 0.0 } else { String::from_utf16_lossy(trimmed).parse::<f64>().unwrap_or(f64::NAN) }
        }
        _ => f64::NAN,
    }
}

const MAX_TOKENIZATION_LINE_LENGTH_KEY: &str = "editor.maxTokenizationLineLength";

fn read_max_tokenization_line_length(configuration_service: &dyn IConfigurationService, language_id: &str) -> f64 {
    to_number(&configuration_service.get_value(Some(MAX_TOKENIZATION_LINE_LENGTH_KEY), Some(&ConfigurationOverrides::with_identifier(language_id))))
}

/// `MonarchTokenizer`.
pub struct MonarchTokenizer {
    base: Rc<Disposable>,
    language_service: Rc<dyn ILanguageService>,
    standalone_theme_service: Option<Rc<dyn IStandaloneThemeService>>,
    language_id: String,
    lexer: Rc<Lexer>,
    embedded_languages: Rc<RefCell<Vec<String>>>,
    pub embedded_loaded: RefCell<Promise<()>>,
    max_tokenization_line_length: Rc<Cell<f64>>,
}

/// O que `_myTokenize` decide fazer com o resultado de uma regra: um token, ou um grupo de ações.
enum RuleResult {
    Token(JsString),
    Group(Vec<FuzzyAction>),
}

/// `GroupMatching`: os casamentos de grupos de captura que ainda faltam processar.
struct GroupMatching {
    matches: Vec<Option<JsString>>,
    rule: Option<Rc<dyn IRule>>,
    groups: VecDeque<(FuzzyAction, JsString)>,
}

impl MonarchTokenizer {
    pub fn new(
        language_service: Rc<dyn ILanguageService>,
        standalone_theme_service: Option<Rc<dyn IStandaloneThemeService>>,
        language_id: &str,
        lexer: Lexer,
        configuration_service: Rc<dyn IConfigurationService>,
    ) -> Rc<Self> {
        let base = Disposable::new();
        let embedded_languages: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(Vec::new()));

        // Set up listening for embedded modes
        let emitting = Rc::new(Cell::new(false));
        {
            let (embedded_languages, language_id) = (embedded_languages.clone(), language_id.to_owned());
            base.register(tokenization_registry().on_did_change().listen(move |e| {
                if emitting.get() {
                    return;
                }
                let is_one_of_my_embedded_modes = e.changed_languages.iter().any(|language| embedded_languages.borrow().contains(language));
                if is_one_of_my_embedded_modes {
                    emitting.set(true);
                    tokenization_registry().handle_change(vec![language_id.clone()]);
                    emitting.set(false);
                }
            }));
        }
        let max_tokenization_line_length = Rc::new(Cell::new(read_max_tokenization_line_length(&*configuration_service, language_id)));
        {
            let (max_length, service, language_id) = (max_tokenization_line_length.clone(), configuration_service.clone(), language_id.to_owned());
            base.register(configuration_service.on_did_change_configuration().listen(move |e| {
                if e.affects_configuration(MAX_TOKENIZATION_LINE_LENGTH_KEY, None) {
                    max_length.set(read_max_tokenization_line_length(&*service, &language_id));
                }
            }));
        }

        Rc::new(Self {
            base,
            language_service,
            standalone_theme_service,
            language_id: language_id.to_owned(),
            lexer: Rc::new(lexer),
            embedded_languages,
            embedded_loaded: RefCell::new(Promise::resolved(())),
            max_tokenization_line_length,
        })
    }

    /// `getLoadStatus()`.
    pub fn get_load_status(&self) -> LoadStatus {
        let mut promises: Vec<Promise<()>> = Vec::new();
        let registry = tokenization_registry();
        for nested_language_id in self.embedded_languages.borrow().iter() {
            if let Some(tokenization_support) = registry.get(nested_language_id) {
                // The nested language is already loaded
                if let Some(monarch) = tokenization_support.as_any().and_then(|any| any.downcast_ref::<MonarchTokenizer>())
                    && let LoadStatus::Pending(promise) = monarch.get_load_status()
                {
                    promises.push(promise);
                }
                continue;
            }

            if !registry.is_resolved(nested_language_id) {
                // The nested language is in the process of being loaded
                promises.push(registry.get_or_create(nested_language_id).then(|_| ()));
            }
        }

        if promises.is_empty() {
            return LoadStatus::Loaded;
        }
        LoadStatus::Pending(Promise::all(promises).then(|_| ()))
    }

    fn line_state_of(line_state: &State) -> Rc<MonarchLineState> {
        // `<MonarchLineState>lineState`: o estado de outro tokenizador não é um estado de linha do Monarch.
        let state = line_state.as_any().downcast_ref::<MonarchLineState>().expect("o estado não é do Monarch");
        state.me.upgrade().expect("o estado está vivo")
    }

    fn tokenize_line(&self, line: &JsString, has_eol: bool, line_state: &State, collector: &mut dyn IMonarchTokensCollector) -> Rc<MonarchLineState> {
        let line_state = Self::line_state_of(line_state);
        unwrap_or_throw(self.tokenize_inner(line, has_eol, &line_state, collector))
    }

    fn tokenize_inner(&self, line: &JsString, has_eol: bool, line_state: &Rc<MonarchLineState>, collector: &mut dyn IMonarchTokensCollector) -> Result<Rc<MonarchLineState>, MonarchError> {
        if line_state.embedded_language_data.is_some() {
            self.nested_tokenize(line, has_eol, line_state, 0, collector)
        } else {
            self.my_tokenize(line, has_eol, line_state, 0, collector)
        }
    }

    fn find_leaving_nested_language_offset(&self, line: &JsString, state: &Rc<MonarchLineState>) -> Result<Option<usize>, MonarchError> {
        let rules = match self.lexer.tokenizer.get(&state.stack.state).cloned() {
            Some(rules) => rules,
            None => match find_rules(&self.lexer, &state.stack.state) {
                // do parent matching
                Some(rules) => rules,
                None => return Err(create_error(&*self.lexer, &format!("tokenizer state is not defined: {}", state.stack.state))),
            },
        };

        let mut pop_offset: Option<usize> = None;
        let mut has_embedded_pop_rule = false;

        for rule in rules.iter() {
            let embedded_end = match rule.action() {
                FuzzyAction::Str(_) => false,
                FuzzyAction::Action(action) => action.next_embedded.as_deref() == Some("@pop") || action.has_embedded_end_in_cases,
            };
            if !embedded_end {
                continue;
            }
            has_embedded_pop_rule = true;

            let mut regex = rule.resolve_regex(&state.stack.state)?;
            let regex_source = regex.source().clone();
            let units = regex_source.units();
            let prefix: Vec<u16> = "^(?:".encode_utf16().collect();
            if units.len() >= 5 && units.starts_with(&prefix) && units[units.len() - 1] == u16::from(b')') {
                let inner = JsString::from(&units[4..units.len() - 1]);
                regex = Rc::new(MonarchRegExp::new(&inner, regex.ignore_case(), regex.unicode())?);
            }

            let Some(result) = regex.search(line.units()) else {
                continue;
            };
            if result != 0 && rule.match_only_at_line_start() {
                continue;
            }

            if pop_offset.is_none_or(|current| result < current) {
                pop_offset = Some(result);
            }
        }

        if !has_embedded_pop_rule {
            return Err(create_error(&*self.lexer, &format!("no rule containing nextEmbedded: \"@pop\" in tokenizer embedded state: {}", state.stack.state)));
        }

        Ok(pop_offset)
    }

    fn nested_tokenize(
        &self,
        line: &JsString,
        has_eol: bool,
        line_state: &Rc<MonarchLineState>,
        offset_delta: usize,
        tokens_collector: &mut dyn IMonarchTokensCollector,
    ) -> Result<Rc<MonarchLineState>, MonarchError> {
        let embedded = line_state.embedded_language_data.clone().expect("estado com linguagem embutida");
        let Some(pop_offset) = self.find_leaving_nested_language_offset(line, line_state)? else {
            // tokenization will not leave nested language
            let nested_end_state = tokens_collector.nested_language_tokenize(line, has_eol, &embedded, offset_delta);
            return Ok(MonarchLineStateFactory::create(
                &line_state.stack,
                Some(Rc::new(EmbeddedLanguageData { language_id: embedded.language_id.clone(), state: nested_end_state })),
            ));
        };

        let nested_language_line = line.slice(0, pop_offset);
        if !nested_language_line.is_empty() {
            // tokenize with the nested language
            tokens_collector.nested_language_tokenize(&nested_language_line, false, &embedded, offset_delta);
        }

        let rest_of_the_line = line.slice(pop_offset, line.len());
        self.my_tokenize(&rest_of_the_line, has_eol, line_state, offset_delta + pop_offset, tokens_collector)
    }

    fn safe_rule_name(rule: &Option<Rc<dyn IRule>>) -> String {
        match rule {
            Some(rule) => rule.name().to_owned(),
            None => "(unknown)".to_owned(),
        }
    }

    /// `computeNewStateForEmbeddedLanguage`.
    #[allow(clippy::too_many_arguments)]
    fn compute_new_state_for_embedded_language(
        &self,
        entering_embedded_language: &JsString,
        stack: &Rc<MonarchStackElement>,
        pos: usize,
        line_length: usize,
        line_without_lf: &JsString,
        has_eol: bool,
        offset_delta: usize,
        tokens_collector: &mut dyn IMonarchTokensCollector,
    ) -> Result<Rc<MonarchLineState>, MonarchError> {
        // support language names, mime types, and language ids
        let entering = entering_embedded_language.to_string_lossy();
        let language_id = self
            .language_service
            .get_language_id_by_language_name(&entering)
            .filter(|id| !id.is_empty())
            .or_else(|| self.language_service.get_language_id_by_mime_type(Some(&entering)).filter(|id| !id.is_empty()))
            .unwrap_or(entering);

        let embedded_language_data = self.get_nested_embedded_language_data(&language_id);

        if pos < line_length {
            // there is content from the embedded language on this line
            let rest_of_line = line_without_lf.slice(pos.min(line_without_lf.len()), line_without_lf.len());
            let state = MonarchLineStateFactory::create(stack, Some(embedded_language_data));
            self.nested_tokenize(&rest_of_line, has_eol, &state, offset_delta + pos, tokens_collector)
        } else {
            Ok(MonarchLineStateFactory::create(stack, Some(embedded_language_data)))
        }
    }

    fn my_tokenize(
        &self,
        line_without_lf: &JsString,
        has_eol: bool,
        line_state: &Rc<MonarchLineState>,
        offset_delta: usize,
        tokens_collector: &mut dyn IMonarchTokensCollector,
    ) -> Result<Rc<MonarchLineState>, MonarchError> {
        tokens_collector.enter_language(&self.language_id);

        let lexer = &self.lexer;
        let line_without_lf_length = line_without_lf.len();
        let line: JsString = if has_eol && lexer.include_lf {
            let mut with_lf = line_without_lf.clone();
            with_lf.push_unit(u16::from(b'\n'));
            with_lf
        } else {
            line_without_lf.clone()
        };
        let line_length = line.len();

        let mut embedded_language_data = line_state.embedded_language_data.clone();
        let mut stack = line_state.stack.clone();
        let mut pos: usize = 0;

        // regular expression group matching
        // these never need cloning or equality since they are only used within a line match
        let mut group_matching: Option<GroupMatching> = None;

        // See https://github.com/microsoft/monaco-editor/issues/1235
        // Evaluate rules at least once for an empty line
        let mut force_evaluation = true;

        while force_evaluation || pos < line_length {
            let pos0 = pos;
            let stack_len0 = stack.depth;
            let group_len0 = group_matching.as_ref().map_or(0, |g| g.groups.len());
            let state = stack.state.clone();

            let mut matches: Option<Vec<Option<JsString>>> = None;
            let mut matched: Option<JsString> = None;
            let mut action: Option<FuzzyAction> = None;
            let mut rule: Option<Rc<dyn IRule>> = None;

            let mut entering_embedded_language: Option<JsString> = None;

            // check if we need to process group matches first
            if let Some(group) = group_matching.as_mut() {
                matches = Some(group.matches.clone());
                let (group_action, group_matched) = group.groups.pop_front().expect("o grupo tem entradas");
                matched = Some(group_matched);
                action = Some(group_action);
                rule = group.rule.clone();

                // cleanup if necessary
                if group.groups.is_empty() {
                    group_matching = None;
                }
            } else {
                // otherwise we match on the token stream

                if !force_evaluation && pos >= line_length {
                    // nothing to do
                    break;
                }

                force_evaluation = false;

                // get the rules for this state
                let rules = match lexer.tokenizer.get(&state).cloned() {
                    Some(rules) => rules,
                    None => match find_rules(lexer, &state) {
                        // do parent matching
                        Some(rules) => rules,
                        None => return Err(create_error(&**lexer, &format!("tokenizer state is not defined: {state}"))),
                    },
                };

                // try each rule until we match
                let rest_of_line = &line.units()[pos..];
                for candidate in rules.iter() {
                    if pos == 0 || !candidate.match_only_at_line_start() {
                        let regex = candidate.resolve_regex(&state)?;
                        if let Some(found) = regex.exec(rest_of_line) {
                            let captured: Vec<Option<JsString>> = found.captures.iter().map(|range| range.as_ref().map(|r| JsString::from(&rest_of_line[r.clone()]))).collect();
                            matched = captured[0].clone();
                            matches = Some(captured);
                            action = Some(candidate.action().clone());
                            break;
                        }
                    }
                }
            }

            // We matched 'rule' with 'matches' and 'action'
            let mut matches = match matches {
                Some(matches) => matches,
                None => {
                    matched = Some(JsString::new());
                    vec![Some(JsString::new())]
                }
            };

            let action = match action {
                Some(action) => action,
                None => {
                    // bad: we didn't match anything, and there is no action to take
                    // we need to advance the stream or we get progress trouble
                    if pos < line_length {
                        let character = JsString::from_units(vec![line.units()[pos]]);
                        matches = vec![Some(character.clone())];
                        matched = Some(character);
                    }
                    FuzzyAction::Str(lexer.default_token.clone())
                }
            };

            let Some(mut matched) = matched else {
                // should never happen, needed for strict null checking
                break;
            };

            // advance stream
            pos += matched.len();

            // maybe call action function (used for 'cases')
            let mut action = action;
            loop {
                let next_action = match &action {
                    FuzzyAction::Action(inner) => match &inner.test {
                        Some(test) => Some(test(&matched, &matches, &state, pos == line_length)?),
                        None => None,
                    },
                    FuzzyAction::Str(_) => None,
                };
                match next_action {
                    Some(next_action) => action = next_action,
                    None => break,
                }
            }

            let mut result: Option<RuleResult> = None;
            // set the result: either a string or an array of actions
            match &action {
                FuzzyAction::Str(text) => result = Some(RuleResult::Token(JsString::from(text.as_str()))),
                FuzzyAction::Action(inner) => {
                    if let Some(group) = &inner.group {
                        result = Some(RuleResult::Group(group.clone()));
                    } else if let Some(token) = &inner.token {
                        // do $n replacements?
                        let token_text = if inner.token_subst { substitute_matches(&**lexer, &JsString::from(token.as_str()), &matched, &matches, &state) } else { JsString::from(token.as_str()) };
                        result = Some(RuleResult::Token(token_text));

                        // enter embedded language?
                        if let Some(next_embedded) = inner.next_embedded.as_deref().filter(|next| !next.is_empty()) {
                            if next_embedded == "@pop" {
                                if embedded_language_data.is_none() {
                                    return Err(create_error(&**lexer, "cannot pop embedded language if not inside one"));
                                }
                                embedded_language_data = None;
                            } else if embedded_language_data.is_some() {
                                return Err(create_error(&**lexer, "cannot enter embedded language from within an embedded language"));
                            } else {
                                entering_embedded_language = Some(substitute_matches(&**lexer, &JsString::from(next_embedded), &matched, &matches, &state));
                            }
                        }

                        // state transformations
                        if let Some(go_back) = inner.go_back.filter(|go_back| *go_back != 0) {
                            // back up the stream..
                            pos = (pos as i64 - i64::from(go_back)).max(0) as usize;
                        }

                        if let Some(switch_to) = inner.switch_to.as_deref().filter(|switch_to| !switch_to.is_empty()) {
                            // switch state without a push...
                            let mut next_state = substitute_matches(&**lexer, &JsString::from(switch_to), &matched, &matches, &state);
                            if next_state.units().first() == Some(&u16::from(b'@')) {
                                next_state = next_state.slice(1, next_state.len()); // peel off starting '@'
                            }
                            if find_rules(lexer, &next_state).is_none() {
                                return Err(create_error(&**lexer, &format!("trying to switch to a state '{next_state}' that is undefined in rule: {}", Self::safe_rule_name(&rule))));
                            }
                            stack = stack.switch_to(&next_state);
                        } else if let Some(next) = inner.next.as_deref().filter(|next| !next.is_empty()) {
                            if next == "@push" {
                                if stack.depth >= lexer.max_stack {
                                    let parent_state = stack.parent.as_ref().map(|p| p.state.to_string_lossy()).unwrap_or_default();
                                    return Err(create_error(&**lexer, &format!("maximum tokenizer stack size reached: [{},{},...]", stack.state, parent_state)));
                                }
                                stack = stack.push(&state);
                            } else if next == "@pop" {
                                if stack.depth <= 1 {
                                    return Err(create_error(&**lexer, &format!("trying to pop an empty stack in rule: {}", Self::safe_rule_name(&rule))));
                                }
                                stack = stack.pop().expect("a pilha tem mais de um elemento");
                            } else if next == "@popall" {
                                stack = stack.popall();
                            } else {
                                let mut next_state = substitute_matches(&**lexer, &JsString::from(next), &matched, &matches, &state);
                                if next_state.units().first() == Some(&u16::from(b'@')) {
                                    next_state = next_state.slice(1, next_state.len()); // peel off starting '@'
                                }

                                if find_rules(lexer, &next_state).is_none() {
                                    return Err(create_error(&**lexer, &format!("trying to set a next state '{next_state}' that is undefined in rule: {}", Self::safe_rule_name(&rule))));
                                }
                                stack = stack.push(&next_state);
                            }
                        }

                        if let Some(log_text) = inner.log.as_deref().filter(|log_text| !log_text.is_empty()) {
                            let message = format!("{}: {}", lexer.language_id, substitute_matches(&**lexer, &JsString::from(log_text), &matched, &matches, &state));
                            log(&**lexer, &message);
                        }
                    }
                }
            }

            // check result
            let Some(result) = result else {
                return Err(create_error(&**lexer, &format!("lexer rule has no well-defined action in rule: {}", Self::safe_rule_name(&rule))));
            };

            // is the result a group match?
            let result_text = match result {
                RuleResult::Group(result) => {
                    if group_matching.as_ref().is_some_and(|g| !g.groups.is_empty()) {
                        return Err(create_error(&**lexer, &format!("groups cannot be nested: {}", Self::safe_rule_name(&rule))));
                    }
                    if matches.len() != result.len() + 1 {
                        return Err(create_error(&**lexer, &format!("matched number of groups does not match the number of actions in rule: {}", Self::safe_rule_name(&rule))));
                    }
                    let mut total_len = 0;
                    for group in matches.iter().skip(1) {
                        match group {
                            Some(text) => total_len += text.len(),
                            None => return Err(MonarchError("Cannot read properties of undefined (reading 'length')".to_owned())),
                        }
                    }
                    if total_len != matched.len() {
                        return Err(create_error(&**lexer, &format!("with groups, all characters should be matched in consecutive groups in rule: {}", Self::safe_rule_name(&rule))));
                    }

                    let groups: VecDeque<(FuzzyAction, JsString)> = result.into_iter().enumerate().map(|(i, group_action)| (group_action, matches[i + 1].clone().unwrap_or_default())).collect();
                    group_matching = Some(GroupMatching { rule: rule.clone(), matches: matches.clone(), groups });

                    pos -= matched.len();
                    // call recursively to initiate first result match
                    continue;
                }
                RuleResult::Token(text) => text,
            };

            // regular result
            let mut result_text = result_text;

            // check for '@rematch'
            if result_text == *"@rematch" {
                pos = pos.saturating_sub(matched.len());
                matched = JsString::new(); // better set the next state too..
                matches.clear();
                result_text = JsString::new();

                // Even though `@rematch` was specified, if `nextEmbedded` also specified,
                // a state transition should occur.
                if let Some(entering) = &entering_embedded_language {
                    return self.compute_new_state_for_embedded_language(entering, &stack, pos, line_length, line_without_lf, has_eol, offset_delta, tokens_collector);
                }
            }

            // check progress
            if matched.is_empty() {
                let groups_now = group_matching.as_ref().map_or(0, |g| g.groups.len());
                if line_length == 0 || stack_len0 != stack.depth || state != stack.state || groups_now != group_len0 {
                    continue;
                }
                return Err(create_error(&**lexer, &format!("no progress in tokenizer in rule: {}", Self::safe_rule_name(&rule))));
            }

            // return the result (and check for brace matching)
            // todo: for efficiency we could pre-sanitize tokenPostfix and substitutions
            let token_type: JsString;
            if result_text.units().starts_with(&"@brackets".encode_utf16().collect::<Vec<u16>>()) {
                let rest = result_text.slice("@brackets".len(), result_text.len());
                let Some((bracket_token, _bracket_type)) = find_bracket(lexer, &matched) else {
                    return Err(create_error(&**lexer, &format!("@brackets token returned but no bracket defined as: {matched}")));
                };
                let mut joined = JsString::from(bracket_token.as_str());
                joined.push_units(rest.units());
                token_type = sanitize(&joined);
            } else {
                let token = if result_text.is_empty() {
                    JsString::new()
                } else {
                    let mut with_postfix = result_text.clone();
                    with_postfix.push_units(JsString::from(lexer.token_postfix.as_str()).units());
                    with_postfix
                };
                token_type = sanitize(&token);
            }

            if pos0 < line_without_lf_length {
                tokens_collector.emit(pos0 + offset_delta, &token_type.to_string_lossy());
            }

            if let Some(entering) = &entering_embedded_language {
                return self.compute_new_state_for_embedded_language(entering, &stack, pos, line_length, line_without_lf, has_eol, offset_delta, tokens_collector);
            }
        }

        Ok(MonarchLineStateFactory::create(&stack, embedded_language_data))
    }

    fn get_nested_embedded_language_data(&self, language_id: &str) -> Rc<EmbeddedLanguageData> {
        if !self.language_service.is_registered_language_id(Some(language_id)) {
            return Rc::new(EmbeddedLanguageData { language_id: language_id.to_owned(), state: null_state() });
        }

        if language_id != self.language_id {
            // Fire language loading event
            self.language_service.request_basic_language_features(language_id);
            let _ = tokenization_registry().get_or_create(language_id);
            let mut embedded = self.embedded_languages.borrow_mut();
            if !embedded.iter().any(|existing| existing == language_id) {
                embedded.push(language_id.to_owned());
            }
        }

        if let Some(tokenization_support) = tokenization_registry().get(language_id) {
            return Rc::new(EmbeddedLanguageData { language_id: language_id.to_owned(), state: tokenization_support.get_initial_state() });
        }

        Rc::new(EmbeddedLanguageData { language_id: language_id.to_owned(), state: null_state() })
    }
}

impl IDisposable for MonarchTokenizer {
    fn dispose(&self) {
        IDisposable::dispose(&*self.base);
    }
}

impl ITokenizationSupport for MonarchTokenizer {
    fn get_initial_state(&self) -> State {
        let start = self.lexer.start.clone().expect("o lexer compilado tem estado inicial");
        let root_state = MonarchStackElementFactory::create(None, &start);
        MonarchLineStateFactory::create(&root_state, None)
    }

    fn tokenize(&self, line: &JsString, has_eol: bool, line_state: &State) -> TokenizationResult {
        if (line.len() as f64) >= self.max_tokenization_line_length.get() {
            return null_tokenize(&self.language_id, line_state.clone());
        }
        let mut tokens_collector = MonarchClassicTokensCollector::new();
        let end_line_state = self.tokenize_line(line, has_eol, line_state, &mut tokens_collector);
        tokens_collector.finalize(end_line_state)
    }

    fn tokenize_encoded(&self, line: &JsString, has_eol: bool, line_state: &State) -> EncodedTokenizationResult {
        if (line.len() as f64) >= self.max_tokenization_line_length.get() {
            return null_tokenize_encoded(self.language_service.language_id_codec().encode_language_id(&self.language_id), Some(line_state.clone()));
        }
        let theme = self.standalone_theme_service.as_ref().expect("o tokenizador não recebeu o serviço de tema").token_theme();
        let mut tokens_collector = MonarchModernTokensCollector::new(self.language_service.clone(), theme);
        let end_line_state = self.tokenize_line(line, has_eol, line_state, &mut tokens_collector);
        tokens_collector.finalize(end_line_state)
    }

    fn as_any(&self) -> Option<&dyn Any> {
        Some(self)
    }
}

/// Searches for a bracket in the 'brackets' attribute that matches the input.
fn find_bracket(lexer: &Lexer, matched: &JsString) -> Option<(String, MonarchBracket)> {
    if matched.is_empty() {
        return None;
    }
    let matched = fix_case(lexer, matched).to_string_lossy();

    for bracket in &lexer.brackets {
        if bracket.open == matched {
            return Some((bracket.token.clone(), MonarchBracket::Open));
        } else if bracket.close == matched {
            return Some((bracket.token.clone(), MonarchBracket::Close));
        }
    }
    None
}
