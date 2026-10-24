//! Porte de `src/vs/editor/common/languages/languageConfigurationRegistry.ts`:
//! `LanguageConfigurationRegistry` (as contribuições por linguagem, ordenadas por prioridade),
//! `ResolvedLanguageConfiguration` (a configuração combinada, com os suportes derivados dela) e
//! `LanguageConfigurationService`.
//!
//! Divergências:
//! - O `IConfigurationService` não existe no porte. O original usa dele as sobrescritas de usuário
//!   `editor.language.brackets` e `editor.language.colorizedBracketPairs` (por linguagem), que se
//!   combinam à configuração registrada, e o evento de mudança de configuração que invalida o cache.
//!   Aqui o serviço só combina o que foi registrado; sem sobrescritas, o resultado é o mesmo do original
//!   com a configuração do usuário vazia.
//! - O serviço é construído por [`LanguageConfigurationService::new`] com o `ILanguageService`, e não
//!   pelo contêiner de injeção (`registerSingleton`).
//! - `ICommentsConfiguration`, `LanguageConfigurationChangeEvent` e os demais tipos de dados têm o
//!   mesmo nome e os mesmos campos do original (em `snake_case`).
//! - `onEnter` recebe os textos como `&[u16]` (UTF-16, como o modelo).

use crate::cursor_common::CursorSimpleModel;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use je_base::JsString;
use je_base::event::{Emitter, EmitterOptions, Event};
use je_base::lifecycle::{
    Disposable, DisposableRef, IDisposable, mark_as_singleton, to_disposable,
};
use je_base::strings::get_leading_whitespace;

use crate::character_pair::CharacterPairSupport;
use crate::config::editor_options::EditorAutoIndentStrategy;
use crate::electric_character::BracketElectricCharacterSupport;
use crate::indent_rules::IndentRulesSupport;
use je_base::js_regexp::JsRegExp;
use crate::language_brackets_configuration::LanguageBracketsConfiguration;
use crate::language_configuration::{
    AutoClosingPair, AutoClosingPairs, EnterAction, FoldingRules, IndentationRule,
    LanguageConfiguration, LineComment,
};
use crate::language_service::ILanguageService;
use crate::modes_registry::PLAINTEXT_LANGUAGE_ID;
use crate::on_enter::{OnEnterSupport, OnEnterSupportOptions};
use crate::rich_edit_brackets::RichEditBrackets;
use crate::word_helper::{default_word_regexp, ensure_valid_word_definition};

/// `ICommentsConfiguration`: o que o editor usa para inserir comentários da linguagem.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CommentsConfiguration {
    pub line_comment_token: Option<String>,
    pub line_comment_no_indent: Option<bool>,
    pub block_comment_start_token: Option<String>,
    pub block_comment_end_token: Option<String>,
}

/// `ILanguageConfigurationService`.
pub trait ILanguageConfigurationService {
    fn on_did_change(&self) -> Event<LanguageConfigurationServiceChangeEvent>;

    /// `priority`: um número maior vale mais; o padrão é 0.
    fn register(
        &self,
        language_id: &str,
        configuration: LanguageConfiguration,
        priority: Option<i32>,
    ) -> DisposableRef;

    fn get_language_configuration(&self, language_id: &str) -> Rc<ResolvedLanguageConfiguration>;
}

/// `LanguageConfigurationServiceChangeEvent`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LanguageConfigurationServiceChangeEvent {
    pub language_id: Option<String>,
}

impl LanguageConfigurationServiceChangeEvent {
    pub fn new(language_id: Option<&str>) -> Self {
        Self {
            language_id: language_id.map(str::to_owned),
        }
    }

    pub fn affects(&self, language_id: &str) -> bool {
        match &self.language_id {
            None => true,
            Some(id) if id.is_empty() => true,
            Some(id) => id == language_id,
        }
    }
}

/// `LanguageConfigurationService`.
pub struct LanguageConfigurationService {
    base: Rc<Disposable>,
    registry: Rc<LanguageConfigurationRegistry>,
    on_did_change_emitter: Rc<Emitter<LanguageConfigurationServiceChangeEvent>>,
    configurations: RefCell<HashMap<String, Rc<ResolvedLanguageConfiguration>>>,
    language_service: Rc<dyn ILanguageService>,
}

impl LanguageConfigurationService {
    pub fn new(language_service: Rc<dyn ILanguageService>) -> Rc<Self> {
        let base = Disposable::new();
        let registry = LanguageConfigurationRegistry::new();
        base.register(registry.clone());
        let on_did_change_emitter = Emitter::with_options(EmitterOptions {
            // Aumentado para os editores de diff múltiplos, com centenas de modelos de texto.
            leak_warning_threshold: Some(500),
            leak_warning_name: Some("LanguageConfigurationService.onDidChange".to_owned()),
            ..EmitterOptions::default()
        });
        base.register(on_did_change_emitter.clone());

        let this = Rc::new(Self {
            base,
            registry,
            on_did_change_emitter,
            configurations: RefCell::new(HashMap::new()),
            language_service,
        });

        let weak = Rc::downgrade(&this);
        let listener = this.registry.on_did_change().listen(move |e| {
            if let Some(this) = weak.upgrade() {
                this.configurations.borrow_mut().remove(&e.language_id);
                this.on_did_change_emitter
                    .fire(LanguageConfigurationServiceChangeEvent::new(Some(
                        &e.language_id,
                    )));
            }
        });
        this.base.register(listener);
        this
    }

    fn compute_config(&self, language_id: &str) -> Rc<ResolvedLanguageConfiguration> {
        let language_config = match self.registry.get_language_configuration(language_id) {
            Some(config) => config,
            None => {
                if !self
                    .language_service
                    .is_registered_language_id(Some(language_id))
                {
                    // Acontece com a linguagem nula, que o monarch pode devolver. Em vez de lançar, devolve
                    // uma configuração padrão.
                    return Rc::new(ResolvedLanguageConfiguration::new(
                        language_id,
                        LanguageConfiguration::default(),
                    ));
                }
                Rc::new(ResolvedLanguageConfiguration::new(
                    language_id,
                    LanguageConfiguration::default(),
                ))
            }
        };

        // Aqui o original combina com as sobrescritas de usuário (ver a divergência no topo do módulo).
        let data = combine_language_configurations(std::slice::from_ref(
            &language_config.underlying_config,
        ));
        Rc::new(ResolvedLanguageConfiguration::new(
            &language_config.language_id,
            data,
        ))
    }
}

impl IDisposable for LanguageConfigurationService {
    fn dispose(&self) {
        IDisposable::dispose(&*self.base);
    }
}

impl ILanguageConfigurationService for LanguageConfigurationService {
    fn on_did_change(&self) -> Event<LanguageConfigurationServiceChangeEvent> {
        self.on_did_change_emitter.event()
    }

    fn register(
        &self,
        language_id: &str,
        configuration: LanguageConfiguration,
        priority: Option<i32>,
    ) -> DisposableRef {
        self.registry.register(language_id, configuration, priority)
    }

    fn get_language_configuration(&self, language_id: &str) -> Rc<ResolvedLanguageConfiguration> {
        if let Some(result) = self.configurations.borrow().get(language_id) {
            return result.clone();
        }
        let result = self.compute_config(language_id);
        self.configurations
            .borrow_mut()
            .insert(language_id.to_owned(), result.clone());
        result
    }
}

/// `getIndentationAtPosition`, sobre o texto da linha: o espaço em branco do começo da linha, cortado
/// em `column - 1`.
pub fn get_indentation_at_column(line_text: &[u16], column: i32) -> JsString {
    let indentation = get_leading_whitespace(line_text, 0, line_text.len());
    let limit = (column - 1).max(0) as usize;
    JsString::from(&indentation[..indentation.len().min(limit)])
}

/// `getIndentationAtPosition`.
pub fn get_indentation_at_position(
    model: &crate::text_model::TextModel,
    line_number: i32,
    column: i32,
) -> JsString {
    get_indentation_at_column(model.get_line_content(line_number).units(), column)
}

struct LanguageConfigurationContribution {
    configuration: LanguageConfiguration,
    priority: i32,
    order: u64,
}

struct ComposedLanguageConfiguration {
    language_id: String,
    entries: RefCell<Vec<Rc<LanguageConfigurationContribution>>>,
    order: Cell<u64>,
    resolved: RefCell<Option<Rc<ResolvedLanguageConfiguration>>>,
}

impl ComposedLanguageConfiguration {
    fn new(language_id: &str) -> Rc<Self> {
        Rc::new(Self {
            language_id: language_id.to_owned(),
            entries: RefCell::new(Vec::new()),
            order: Cell::new(0),
            resolved: RefCell::new(None),
        })
    }

    fn register(
        self: &Rc<Self>,
        configuration: LanguageConfiguration,
        priority: i32,
    ) -> DisposableRef {
        self.order.set(self.order.get() + 1);
        let entry = Rc::new(LanguageConfigurationContribution {
            configuration,
            priority,
            order: self.order.get(),
        });
        self.entries.borrow_mut().push(entry.clone());
        *self.resolved.borrow_mut() = None;
        let this = self.clone();
        mark_as_singleton(to_disposable(move || {
            let mut entries = this.entries.borrow_mut();
            if let Some(index) = entries.iter().position(|e| Rc::ptr_eq(e, &entry)) {
                entries.remove(index);
                *this.resolved.borrow_mut() = None;
            }
        }))
    }

    fn get_resolved_configuration(&self) -> Option<Rc<ResolvedLanguageConfiguration>> {
        if self.resolved.borrow().is_none()
            && let Some(config) = self.resolve()
        {
            *self.resolved.borrow_mut() = Some(Rc::new(ResolvedLanguageConfiguration::new(
                &self.language_id,
                config,
            )));
        }
        self.resolved.borrow().clone()
    }

    fn resolve(&self) -> Option<LanguageConfiguration> {
        let mut entries = self.entries.borrow_mut();
        if entries.is_empty() {
            return None;
        }
        // Prioridade maior por último; na mesma prioridade, ordem de registro maior por último.
        entries.sort_by(|a, b| a.priority.cmp(&b.priority).then(a.order.cmp(&b.order)));
        let configurations: Vec<LanguageConfiguration> =
            entries.iter().map(|e| e.configuration.clone()).collect();
        Some(combine_language_configurations(&configurations))
    }
}

/// `combineLanguageConfigurations`: para cada campo vale o último que o define.
fn combine_language_configurations(configs: &[LanguageConfiguration]) -> LanguageConfiguration {
    let mut result = LanguageConfiguration::default();
    for entry in configs {
        result = LanguageConfiguration {
            comments: entry.comments.clone().or(result.comments),
            brackets: entry.brackets.clone().or(result.brackets),
            word_pattern: entry.word_pattern.clone().or(result.word_pattern),
            indentation_rules: entry.indentation_rules.clone().or(result.indentation_rules),
            on_enter_rules: entry.on_enter_rules.clone().or(result.on_enter_rules),
            auto_closing_pairs: entry
                .auto_closing_pairs
                .clone()
                .or(result.auto_closing_pairs),
            surrounding_pairs: entry.surrounding_pairs.clone().or(result.surrounding_pairs),
            auto_close_before: entry
                .auto_close_before
                .clone()
                .filter(|s| !s.is_empty())
                .or(result.auto_close_before),
            folding: entry.folding.clone().or(result.folding),
            colorized_bracket_pairs: entry
                .colorized_bracket_pairs
                .clone()
                .or(result.colorized_bracket_pairs),
            electric_character_support: entry
                .electric_character_support
                .clone()
                .or(result.electric_character_support),
        };
    }
    result
}

/// `LanguageConfigurationChangeEvent`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LanguageConfigurationChangeEvent {
    pub language_id: String,
}

/// `LanguageConfigurationRegistry`.
pub struct LanguageConfigurationRegistry {
    base: Rc<Disposable>,
    entries: RefCell<HashMap<String, Rc<ComposedLanguageConfiguration>>>,
    on_did_change: Rc<Emitter<LanguageConfigurationChangeEvent>>,
}

impl LanguageConfigurationRegistry {
    pub fn new() -> Rc<Self> {
        let base = Disposable::new();
        let on_did_change = Emitter::new();
        base.register(on_did_change.clone());
        let this = Rc::new(Self {
            base,
            entries: RefCell::new(HashMap::new()),
            on_did_change,
        });
        let pair = |open: &str, close: &str| (open.to_owned(), close.to_owned());
        let surrounding = |open: &str, close: &str| AutoClosingPair::new(open, close);
        let plain_text = this.register(
            PLAINTEXT_LANGUAGE_ID,
            LanguageConfiguration {
                brackets: Some(vec![pair("(", ")"), pair("[", "]"), pair("{", "}")]),
                surrounding_pairs: Some(vec![
                    surrounding("{", "}"),
                    surrounding("[", "]"),
                    surrounding("(", ")"),
                    surrounding("<", ">"),
                    surrounding("\"", "\""),
                    surrounding("'", "'"),
                    surrounding("`", "`"),
                ]),
                colorized_bracket_pairs: Some(Vec::new()),
                folding: Some(FoldingRules {
                    off_side: Some(true),
                    markers: None,
                }),
                ..LanguageConfiguration::default()
            },
            Some(0),
        );
        this.base.register(plain_text);
        this
    }

    pub fn on_did_change(&self) -> Event<LanguageConfigurationChangeEvent> {
        self.on_did_change.event()
    }

    /// `priority`: um número maior vale mais; o padrão é 0.
    pub fn register(
        self: &Rc<Self>,
        language_id: &str,
        configuration: LanguageConfiguration,
        priority: Option<i32>,
    ) -> DisposableRef {
        let entries = {
            let mut map = self.entries.borrow_mut();
            map.entry(language_id.to_owned())
                .or_insert_with(|| ComposedLanguageConfiguration::new(language_id))
                .clone()
        };

        let disposable = entries.register(configuration, priority.unwrap_or(0));
        self.on_did_change.fire(LanguageConfigurationChangeEvent {
            language_id: language_id.to_owned(),
        });

        let emitter = self.on_did_change.clone();
        let language_id = language_id.to_owned();
        mark_as_singleton(to_disposable(move || {
            disposable.dispose();
            emitter.fire(LanguageConfigurationChangeEvent { language_id });
        }))
    }

    pub fn get_language_configuration(
        &self,
        language_id: &str,
    ) -> Option<Rc<ResolvedLanguageConfiguration>> {
        let entries = self.entries.borrow().get(language_id).cloned();
        entries.and_then(|e| e.get_resolved_configuration())
    }
}

impl IDisposable for LanguageConfigurationRegistry {
    fn dispose(&self) {
        IDisposable::dispose(&*self.base);
    }
}

/// `ResolvedLanguageConfiguration`. Imutável.
#[derive(Debug)]
pub struct ResolvedLanguageConfiguration {
    pub language_id: String,
    pub underlying_config: LanguageConfiguration,
    brackets: RefCell<Option<Rc<RichEditBrackets>>>,
    electric_character: RefCell<Option<Rc<BracketElectricCharacterSupport>>>,
    on_enter_support: Option<OnEnterSupport>,
    pub comments: Option<CommentsConfiguration>,
    pub character_pair: CharacterPairSupport,
    pub word_definition: Rc<JsRegExp>,
    pub indent_rules_support: Option<IndentRulesSupport>,
    pub indentation_rules: Option<IndentationRule>,
    pub folding_rules: FoldingRules,
    pub brackets_new: LanguageBracketsConfiguration,
}

impl ResolvedLanguageConfiguration {
    pub fn new(language_id: &str, mut underlying_config: LanguageConfiguration) -> Self {
        let on_enter_support = (underlying_config.brackets.is_some()
            || underlying_config.indentation_rules.is_some()
            || underlying_config.on_enter_rules.is_some())
        .then(|| OnEnterSupport::new(OnEnterSupportOptions::from(&underlying_config)));
        // O construtor do `OnEnterSupport` do original faz `opts.brackets = opts.brackets || [...]` sobre o próprio
        // objeto da configuração, e o `CharacterPairSupport` logo abaixo lê esse `brackets` já preenchido: uma
        // linguagem só com `indentationRules` ou `onEnterRules` ganha os pares `()`, `{}` e `[]` (medido no JS).
        if on_enter_support.is_some() && underlying_config.brackets.is_none() {
            let pair = |open: &str, close: &str| (open.to_owned(), close.to_owned());
            underlying_config.brackets = Some(vec![pair("(", ")"), pair("{", "}"), pair("[", "]")]);
        }
        Self {
            language_id: language_id.to_owned(),
            brackets: RefCell::new(None),
            electric_character: RefCell::new(None),
            on_enter_support,
            comments: Self::handle_comments(&underlying_config),
            character_pair: CharacterPairSupport::new(&underlying_config),
            word_definition: underlying_config
                .word_pattern
                .clone()
                .unwrap_or_else(default_word_regexp),
            indent_rules_support: underlying_config
                .indentation_rules
                .clone()
                .map(IndentRulesSupport::new),
            indentation_rules: underlying_config.indentation_rules.clone(),
            folding_rules: underlying_config.folding.clone().unwrap_or_default(),
            brackets_new: LanguageBracketsConfiguration::new(language_id, &underlying_config),
            underlying_config,
        }
    }

    /// `getWordDefinition`: a definição de palavra com a flag `g` garantida (e o `lastIndex` em 0).
    pub fn get_word_definition(&self) -> Rc<JsRegExp> {
        ensure_valid_word_definition(Some(&self.word_definition))
    }

    /// `brackets`: os colchetes agrupados, criados na primeira leitura; `None` sem `brackets` na
    /// configuração.
    pub fn brackets(&self) -> Option<Rc<RichEditBrackets>> {
        if self.brackets.borrow().is_none()
            && let Some(brackets) = &self.underlying_config.brackets
        {
            *self.brackets.borrow_mut() =
                Some(Rc::new(RichEditBrackets::new(&self.language_id, brackets)));
        }
        self.brackets.borrow().clone()
    }

    /// `electricCharacter`: criado na primeira leitura (sobre `brackets`, que pode ser `None`).
    pub fn electric_character(&self) -> Rc<BracketElectricCharacterSupport> {
        if self.electric_character.borrow().is_none() {
            let support = Rc::new(BracketElectricCharacterSupport::new(self.brackets()));
            *self.electric_character.borrow_mut() = Some(support);
        }
        self.electric_character
            .borrow()
            .clone()
            .expect("acabou de ser criado")
    }

    pub fn on_enter(
        &self,
        auto_indent: EditorAutoIndentStrategy,
        previous_line_text: &[u16],
        before_enter_text: &[u16],
        after_enter_text: &[u16],
    ) -> Option<EnterAction> {
        self.on_enter_support.as_ref()?.on_enter(
            auto_indent,
            previous_line_text,
            before_enter_text,
            after_enter_text,
        )
    }

    pub fn get_auto_closing_pairs(&self) -> AutoClosingPairs {
        AutoClosingPairs::new(self.character_pair.get_auto_closing_pairs())
    }

    pub fn get_auto_close_before_set(&self, for_quotes: bool) -> &str {
        self.character_pair.get_auto_close_before_set(for_quotes)
    }

    pub fn get_surrounding_pairs(&self) -> &[AutoClosingPair] {
        self.character_pair.get_surrounding_pairs()
    }

    fn handle_comments(conf: &LanguageConfiguration) -> Option<CommentsConfiguration> {
        let comment_rule = conf.comments.as_ref()?;

        let mut comments = CommentsConfiguration::default();
        match &comment_rule.line_comment {
            Some(LineComment::Token(token)) if !token.is_empty() => {
                comments.line_comment_token = Some(token.clone())
            }
            Some(LineComment::Config(config)) => {
                comments.line_comment_token = Some(config.comment.clone());
                comments.line_comment_no_indent = config.no_indent;
            }
            _ => {}
        }
        if let Some((block_start, block_end)) = &comment_rule.block_comment {
            comments.block_comment_start_token = Some(block_start.clone());
            comments.block_comment_end_token = Some(block_end.clone());
        }
        Some(comments)
    }
}
