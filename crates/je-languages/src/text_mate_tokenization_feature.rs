//! Porte de `src/vs/workbench/services/textMate/browser/textMateTokenizationFeatureImpl.ts` e de
//! `common/TMGrammarFactory.ts`: liga as gramáticas TextMate contribuídas pelas extensões ao
//! `TokenizationRegistry` do editor, com um suporte de tokenização criado sob demanda por linguagem e o tema
//! de cores aplicado à gramática.
//!
//! Divergências:
//! - O `SyncRegistry` do ferriki precisa de todas as gramáticas registradas de antemão para resolver as
//!   inclusões (`include`) sem E/S; o `vscode-textmate` as carrega uma a uma, assíncrono, ao precisar. A
//!   fábrica lê e interpreta os arquivos de todas as gramáticas na primeira vez que alguma linguagem pede
//!   tokenização.
//! - O `Promise` do `LazyTokenizationSupport` já nasce resolvido: ler e compilar a gramática é síncrono.
//! - Tokenização em worker, relatório de tempo e depuração do Oniguruma (`startDebugMode`) não foram
//!   portados; o limite de `editor.maxTokenizationLineLength` vem do valor padrão do editor (20 000) e dos
//!   `configurationDefaults` das extensões embutidas, sem `IConfigurationService`.

use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::path::PathBuf;
use std::rc::{Rc, Weak};

use ferriki_textmate::{
    EmbeddedLanguages, Grammar, GrammarConfiguration, RawTheme, StandardTokenType, StateStack, SyncRegistry, parse_raw_grammar,
};
use je_base::errors::{self, Error};
use je_base::lifecycle::{Disposable, DisposableStore, IDisposable};
use je_base::promise::Promise;
use je_editor::language_service::ILanguageService;
use je_editor::tokenization_registry::{LazyTokenizationSupport, SupportRef, tokenization_registry};
use serde_json::Value;

use crate::extensions::GrammarContribution;
use crate::jsonc;
use crate::text_mate_tokenization_support::{TextMateTokenizationSupport, TokenizationSupportWithLineLimit};
use crate::token_theme::TokenTheme;

/// `editor.maxTokenizationLineLength` (padrão do `editorConfigurationSchema.ts`).
pub const DEFAULT_MAX_TOKENIZATION_LINE_LENGTH: i64 = 20_000;

/// `IValidGrammarDefinition`.
#[derive(Clone, Debug)]
pub struct ValidGrammarDefinition {
    pub location: PathBuf,
    pub language: Option<String>,
    pub scope_name: String,
    /// Escopo para id de linguagem codificado.
    pub embedded_languages: BTreeMap<String, u32>,
    pub token_types: Vec<(String, StandardTokenType)>,
    pub inject_to: Option<Vec<String>>,
    pub balanced_bracket_selectors: Vec<String>,
    pub unbalanced_bracket_selectors: Vec<String>,
    pub source_extension_id: String,
}

/// `ICreateGrammarResult`.
pub struct CreateGrammarResult {
    pub grammar: Rc<Grammar>,
    pub initial_state: Arc<StateStack>,
    pub contains_embedded_languages: bool,
    pub source_extension_id: String,
}

use std::sync::Arc;

/// `TMGrammarFactory`.
struct GrammarFactory {
    registry: RefCell<SyncRegistry>,
    definitions: HashMap<String, ValidGrammarDefinition>,
    injected_embedded_languages: HashMap<String, Vec<BTreeMap<String, u32>>>,
    language_to_scope: HashMap<String, String>,
}

/// Remove comentários de um JSON de gramática para o ferriki (que só lê JSON estrito).
fn read_grammar_text(path: &std::path::Path) -> Result<String, String> {
    let text = fs::read_to_string(path).map_err(|e| e.to_string())?;
    if path.extension().is_some_and(|e| e == "json") {
        return Ok(jsonc::parse(&text)?.to_string());
    }
    Ok(text)
}

impl GrammarFactory {
    fn new(definitions: &[ValidGrammarDefinition]) -> Self {
        let mut registry = SyncRegistry::new(None, None).expect("registro sem tema");
        let mut injections: HashMap<String, Vec<String>> = HashMap::new();
        let mut injected_embedded_languages: HashMap<String, Vec<BTreeMap<String, u32>>> = HashMap::new();
        let mut language_to_scope = HashMap::new();
        let mut by_scope = HashMap::new();

        for valid in definitions {
            // `loadGrammar(scopeName)`: lê e interpreta o arquivo; um erro vira log e a gramática some.
            let loaded = read_grammar_text(&valid.location).and_then(|text| {
                parse_raw_grammar(&text, Some(&valid.location.to_string_lossy())).map_err(|e| e.to_string())
            });
            match loaded {
                Ok(raw) => registry.add_grammar(raw, Vec::new()),
                Err(e) => eprintln!(
                    "Unable to load and parse grammar for scope {} from {}: {e}",
                    valid.scope_name,
                    valid.location.display()
                ),
            }
            by_scope.insert(valid.scope_name.clone(), valid.clone());

            if let Some(inject_to) = &valid.inject_to {
                for inject_scope in inject_to {
                    injections.entry(inject_scope.clone()).or_default().push(valid.scope_name.clone());
                }
                if !valid.embedded_languages.is_empty() {
                    for inject_scope in inject_to {
                        injected_embedded_languages.entry(inject_scope.clone()).or_default().push(valid.embedded_languages.clone());
                    }
                }
            }

            if let Some(language) = &valid.language {
                language_to_scope.insert(language.clone(), valid.scope_name.clone());
            }
        }

        // `getInjections(scopeName)` soma as injeções de cada prefixo do escopo.
        for valid in definitions {
            let parts: Vec<&str> = valid.scope_name.split('.').collect();
            let combined: Vec<String> =
                (1..=parts.len()).flat_map(|n| injections.get(&parts[..n].join(".")).cloned().unwrap_or_default()).collect();
            if !combined.is_empty() {
                registry.set_injections(valid.scope_name.clone(), combined);
            }
        }

        Self { registry: RefCell::new(registry), definitions: by_scope, injected_embedded_languages, language_to_scope }
    }

    fn has(&self, language_id: &str) -> bool {
        self.language_to_scope.contains_key(language_id)
    }

    fn set_theme(&self, theme: RawTheme) -> Result<(), String> {
        self.registry.borrow_mut().set_theme(Some(theme), None).map_err(|e| e.to_string())
    }

    fn get_color_map(&self) -> Vec<String> {
        self.registry.borrow().get_color_map()
    }

    /// `createGrammar(languageId, encodedLanguageId)`: `None` quando não há gramática registrada.
    fn create_grammar(&self, language_id: &str, encoded_language_id: u32) -> Result<Option<CreateGrammarResult>, String> {
        let Some(scope_name) = self.language_to_scope.get(language_id) else { return Ok(None) };
        let Some(definition) = self.definitions.get(scope_name) else { return Ok(None) };

        let mut embedded_languages: EmbeddedLanguages = definition.embedded_languages.clone();
        if let Some(injected) = self.injected_embedded_languages.get(scope_name) {
            for map in injected {
                for (scope, language) in map {
                    embedded_languages.insert(scope.clone(), *language);
                }
            }
        }
        let contains_embedded_languages = !embedded_languages.is_empty();

        let configuration = GrammarConfiguration::default()
            .with_initial_language_id(encoded_language_id)
            .with_embedded_languages(embedded_languages)
            .with_token_types(definition.token_types.clone())
            .with_balanced_bracket_selectors(Some(definition.balanced_bracket_selectors.clone()))
            .with_unbalanced_bracket_selectors(definition.unbalanced_bracket_selectors.clone());
        let grammar = self.registry.borrow_mut().grammar_for_scope_name(scope_name, configuration).map_err(|e| e.to_string())?;
        Ok(grammar.map(|grammar| CreateGrammarResult {
            grammar,
            initial_state: StateStack::null(),
            contains_embedded_languages,
            source_extension_id: definition.source_extension_id.clone(),
        }))
    }
}

/// `TextMateTokenizationFeature`.
pub struct TextMateTokenizationFeature {
    me: Weak<TextMateTokenizationFeature>,
    base: Rc<Disposable>,
    language_service: Rc<dyn ILanguageService>,
    created_modes: Rc<RefCell<Vec<String>>>,
    grammar_definitions: Vec<ValidGrammarDefinition>,
    grammar_factory: RefCell<Option<Rc<GrammarFactory>>>,
    tokenizers_registrations: Rc<DisposableStore>,
    current_theme: RefCell<Option<Value>>,
    max_tokenization_line_length: RefCell<HashMap<String, i64>>,
}

struct LazySupport {
    feature: Weak<TextMateTokenizationFeature>,
    language_id: String,
}

impl LazyTokenizationSupport<SupportRef> for LazySupport {
    fn tokenization_support(&self) -> Promise<Option<SupportRef>> {
        Promise::resolved(self.feature.upgrade().and_then(|f| f.create_tokenization_support(&self.language_id)))
    }
}

fn standard_token_type(name: &str) -> Option<StandardTokenType> {
    match name {
        "string" => Some(StandardTokenType::String),
        "other" => Some(StandardTokenType::Other),
        "comment" => Some(StandardTokenType::Comment),
        "regex" => Some(StandardTokenType::RegEx),
        _ => None,
    }
}

impl TextMateTokenizationFeature {
    /// `_handleGrammarsExtPoint(extensions)`: valida as contribuições e registra uma fábrica de tokenização
    /// para cada linguagem com gramática. `max_line_length_by_language` são os `configurationDefaults` de
    /// `editor.maxTokenizationLineLength` das extensões.
    pub fn new(
        language_service: Rc<dyn ILanguageService>,
        contributions: &[GrammarContribution],
        max_line_length_by_language: HashMap<String, i64>,
    ) -> Rc<Self> {
        let base = Disposable::new();
        let tokenizers_registrations = DisposableStore::new();
        base.register(tokenizers_registrations.clone());

        let grammar_definitions: Vec<ValidGrammarDefinition> =
            contributions.iter().filter_map(|c| Self::validate_grammar_definition(&language_service, c)).collect();

        let feature = Rc::new_cyclic(|me| Self {
            me: me.clone(),
            base,
            language_service,
            created_modes: Rc::new(RefCell::new(Vec::new())),
            grammar_definitions,
            grammar_factory: RefCell::new(None),
            tokenizers_registrations,
            current_theme: RefCell::new(None),
            max_tokenization_line_length: RefCell::new(max_line_length_by_language),
        });

        let created_modes = feature.created_modes.clone();
        feature.base.register(
            feature.language_service.on_did_request_rich_language_features().listen(move |language_id| {
                created_modes.borrow_mut().push(language_id.clone());
            }),
        );

        for definition in &feature.grammar_definitions {
            if let Some(language) = &definition.language {
                let lazy = Rc::new(LazySupport { feature: feature.me.clone(), language_id: language.clone() });
                feature.tokenizers_registrations.add(tokenization_registry().register_factory(language, lazy));
            }
        }
        let created: Vec<String> = feature.created_modes.borrow().clone();
        for created_mode in created {
            let _ = tokenization_registry().get_or_create(&created_mode);
        }
        feature
    }

    /// `_validateGrammarDefinition(extension, grammar)`.
    fn validate_grammar_definition(language_service: &Rc<dyn ILanguageService>, grammar: &GrammarContribution) -> Option<ValidGrammarDefinition> {
        if grammar.invalid_language_type {
            return None;
        }
        if let Some(language) = &grammar.language
            && !language_service.is_registered_language_id(Some(language))
        {
            return None;
        }
        let codec = language_service.language_id_codec();
        let embedded_languages: BTreeMap<String, u32> = grammar
            .embedded_languages
            .iter()
            .filter(|(_, language)| language_service.is_registered_language_id(Some(language)))
            .map(|(scope, language)| (scope.clone(), codec.encode_language_id(language)))
            .collect();
        let token_types = grammar.token_types.iter().filter_map(|(scope, t)| Some((scope.clone(), standard_token_type(t)?))).collect();
        Some(ValidGrammarDefinition {
            location: grammar.extension_dir.join(&grammar.path),
            language: grammar.language.clone(),
            scope_name: grammar.scope_name.clone(),
            embedded_languages,
            token_types,
            inject_to: grammar.inject_to.clone(),
            balanced_bracket_selectors: grammar.balanced_bracket_scopes.clone().unwrap_or_else(|| vec!["*".to_owned()]),
            unbalanced_bracket_selectors: grammar.unbalanced_bracket_scopes.clone().unwrap_or_default(),
            source_extension_id: grammar.extension_id.clone(),
        })
    }

    pub fn grammar_definitions(&self) -> &[ValidGrammarDefinition] {
        &self.grammar_definitions
    }

    fn get_or_create_grammar_factory(&self) -> Rc<GrammarFactory> {
        if let Some(factory) = self.grammar_factory.borrow().clone() {
            return factory;
        }
        let factory = Rc::new(GrammarFactory::new(&self.grammar_definitions));
        *self.grammar_factory.borrow_mut() = Some(factory.clone());
        self.apply_current_theme();
        factory
    }

    /// `_createTokenizationSupport(languageId)`.
    fn create_tokenization_support(&self, language_id: &str) -> Option<SupportRef> {
        if !self.language_service.is_registered_language_id(Some(language_id)) {
            return None;
        }
        let factory = self.get_or_create_grammar_factory();
        if !factory.has(language_id) {
            return None;
        }
        let encoded_language_id = self.language_service.language_id_codec().encode_language_id(language_id);
        let result = match factory.create_grammar(language_id, encoded_language_id) {
            Ok(Some(result)) => result,
            Ok(None) => return None,
            Err(e) => {
                errors::on_unexpected_error(&Error::new(e));
                return None;
            }
        };
        let max_length = self
            .max_tokenization_line_length
            .borrow()
            .get(language_id)
            .copied()
            .unwrap_or(DEFAULT_MAX_TOKENIZATION_LINE_LENGTH);
        let tokenization =
            TextMateTokenizationSupport::new(result.grammar, result.initial_state, result.contains_embedded_languages, false);
        let language_service = self.language_service.clone();
        let encountered = RefCell::new(Vec::<bool>::new());
        let subscription = tokenization.on_did_encounter_language().listen(move |&encoded| {
            let mut seen = encountered.borrow_mut();
            if seen.len() <= encoded as usize {
                seen.resize(encoded as usize + 1, false);
            }
            if !seen[encoded as usize] {
                seen[encoded as usize] = true;
                drop(seen);
                let language_id = language_service.language_id_codec().decode_language_id(encoded);
                language_service.request_basic_language_features(&language_id);
            }
        });
        let store = DisposableStore::new();
        store.add(subscription);
        store.add(tokenization.clone());
        let support: SupportRef = TokenizationSupportWithLineLimit::new(encoded_language_id, tokenization, store, max_length);
        Some(support)
    }

    /// `_updateTheme(colorTheme, forceUpdate)`: aplica as regras de cor de token às gramáticas e publica o
    /// mapa de cores no `TokenizationRegistry`.
    pub fn set_theme(&self, theme: &TokenTheme) {
        *self.current_theme.borrow_mut() = Some(theme.raw_theme());
        self.apply_current_theme();
    }

    fn apply_current_theme(&self) {
        let Some(raw) = self.current_theme.borrow().clone() else { return };
        let Some(factory) = self.grammar_factory.borrow().clone() else { return };
        let raw_theme: RawTheme = match serde_json::from_value(raw) {
            Ok(theme) => theme,
            Err(e) => {
                errors::on_unexpected_error(&Error::new(format!("tema inválido: {e}")));
                return;
            }
        };
        if let Err(e) = factory.set_theme(raw_theme) {
            errors::on_unexpected_error(&Error::new(e));
            return;
        }
        tokenization_registry().set_color_map(factory.get_color_map());
    }

    /// `createTokenizer(languageId)`: a gramática de uma linguagem, para quem tokeniza por conta própria.
    pub fn create_tokenizer(&self, language_id: &str) -> Option<Rc<Grammar>> {
        if !self.language_service.is_registered_language_id(Some(language_id)) {
            return None;
        }
        let factory = self.get_or_create_grammar_factory();
        if !factory.has(language_id) {
            return None;
        }
        let encoded = self.language_service.language_id_codec().encode_language_id(language_id);
        factory.create_grammar(language_id, encoded).ok().flatten().map(|r| r.grammar)
    }

    /// O mapa de cores atual do ferriki (`getColorMap`), vazio antes de haver fábrica e tema.
    pub fn color_map(&self) -> Vec<String> {
        self.grammar_factory.borrow().as_ref().map(|f| f.get_color_map()).unwrap_or_default()
    }
}

impl IDisposable for TextMateTokenizationFeature {
    fn dispose(&self) {
        IDisposable::dispose(&*self.base);
    }
}

