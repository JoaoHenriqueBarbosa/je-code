//! Porte de `src/vs/editor/contrib/snippet/browser/snippetController2.ts`: o controlador de snippets do editor
//! (`SnippetController2`), as quatro chaves de contexto (`inSnippetMode`, `hasNextTabstop`, `hasPrevTabstop`,
//! `inSnippetChoice`) e os comandos `jumpToNextSnippetPlaceholder`, `jumpToPrevSnippetPlaceholder`, `leaveSnippet` e
//! `acceptSnippet` com as chamadas de registro do fim do arquivo.
//!
//! Representação:
//! - `SnippetController2 implements IEditorContribution` é a struct [`SnippetController2`], criada por
//!   [`SnippetController2::new`] (que devolve `Rc<Self>`, porque os ouvintes e o provedor de opções de `choice` guardam
//!   um [`Weak`] do controlador). O `editor` do controlador é um [`Weak`] do widget: o editor é dono da contribuição e
//!   um `Rc` formaria um ciclo. `SnippetController2.get(editor)` é [`SnippetController2::get`].
//! - Os cinco argumentos de injeção do construtor (`ILogService`, `ILanguageFeaturesService`, `IContextKeyService`,
//!   `ILanguageConfigurationService` e, de dentro de `SnippetSession`, o `ILabelService` e o `IWorkspaceContextService`
//!   que `editor.invokeWithinContext` entrega) são o [`SnippetController2Services`].
//! - `static readonly InSnippetMode` e as outras três chaves são os `thread_local` lidos por
//!   [`SnippetController2::in_snippet_mode`], [`SnippetController2::has_next_tabstop`],
//!   [`SnippetController2::has_prev_tabstop`] e [`SnippetController2::in_snippet_choice`] (o `RawContextKey` guarda o
//!   nome, o valor padrão e a expressão `Defined`; `getValue(service)` e `bindTo(service)` são os do `RawContextKey`).
//! - `ISnippetInsertOptions` é o [`SnippetInsertOptions`] (o `Default` é o `_defaultOptions`) e o
//!   `Partial<ISnippetInsertOptions>` é o [`PartialSnippetInsertOptions`] (`{ ..._defaultOptions, ...opts }` é
//!   [`SnippetInsertOptions::with`]). `insert(template: string)` recebe um `impl Into<JsString>` e `apply(edits)` os
//!   [`SnippetEdit`]; `_doInsert(string | ISnippetEdit[])` recebe o [`SnippetTemplate`] da sessão.
//! - O `try { ... } catch (e) { this.cancel(); logService.error(...) }` de `insert` e `apply` é o
//!   [`je_base::errors::catch`] (um erro do original é um pânico no porte). Os argumentos `...args` do
//!   `logService.error('insert_edits=', edits)` são o [`LogArg`] do texto de depuração dos edits.
//! - `IObservable<boolean>` é o [`IObservable`] de um `observableValue`.
//! - `static get(editor)` com `EditorCommand.bindToContribution` é o `bind_to_contribution` do `editor_extensions`;
//!   `registerEditorContribution(ID, ctor, Lazy)` e `registerEditorCommand` estão em
//!   [`register_snippet_controller2`] (idempotente: o registro acontece uma vez por thread).
//!
//! Divergências:
//! - O widget não tem `IContextKeyService`, `ILanguageFeaturesService` nem `ILogService` próprios (o `@IContextKeyService`
//!   do construtor), então o [`SnippetController2Services`] os injeta: o serviço de chaves de contexto é o
//!   `IContextKeyService` de `je-platform` (o mesmo que o `RawContextKey` lê e escreve), o log é o `LogServiceRef`, e o
//!   `ILanguageFeaturesService` e o `showSimpleSuggestions` do módulo `suggest` (ainda sem porte) são o único ponto
//!   injetável [`ISnippetChoiceHost`], opcional: sem ele a lista de opções de um `choice` não é registrada nem exibida,
//!   e o resto do controlador (chaves, navegação, `merge`, `choice` ativo da sessão) funciona igual.
//! - O `CompletionItemProvider` do porte de `languages.rs` não tem `provideCompletionItems`, e o `CompletionItem` não
//!   tem `range` nem `command`: o provedor das opções de `choice` é o [`SnippetChoiceCompletionProvider`], que devolve
//!   [`SnippetChoiceSuggestion`] (o `CompletionItem` do original com o `range` e o `command`).
//! - O registro com `language`, `pattern`, `scheme` e `exclusive` é o [`CompletionProviderSelector`].
//! - `queueMicrotask(() => showSimpleSuggestions(...))` é o `queue_microtask` do `je-base`.
//! - Os `KeyCode`/`KeyMod` das regras de atalho são os de `je-base`, e o `EditorContextKeys.textInputFocus` é o
//!   `text_input_focus()` do `core_commands`.

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use je_base::JsString;
use je_base::dynamic::{Dynamic, Truthy};
use je_base::errors::{self, Error};
use je_base::key_codes::{KeyCode, KeyMod};
use je_base::lifecycle::{DisposableRef, DisposableStore, IDisposable};
use je_base::nls::localize;
use je_base::observable::{IObservable, ISettableObservable, Settable, SettableExt, observable_value};
use je_base::path::{Environment, ProcessEnvironment};
use je_base::runtime::queue_microtask;
use je_platform::contextkey::{ContextKeyExpr, ContextKeyExpression, IContextKey, IContextKeyService, RawContextKey};
use je_platform::keybinding::keybindings_registry::{KeybindingRule, KeybindingWeight, Keybindings, keybindings_registry};
use je_platform::log::{LogArg, LogMessage, LogServiceRef};

use super::snippet_parser::MarkerRef;
use super::snippet_session::{SnippetEdit, SnippetServices, SnippetSession, SnippetSessionInsertOptions, SnippetTemplate};
use super::snippet_variables::IOvertypingCapturer;
use crate::code_editor_widget::{
    CodeEditorWidget, EditorContributionInstantiation, ICodeEditor, IEditorContribution,
};
use crate::core_commands::{key, text_input_focus};
use crate::editor_extensions::{
    CommandKeybindingsOptions, CommandOptions, ContributionCommandOptions, bind_to_contribution, register_editor_command,
    register_editor_contribution,
};
use crate::languages::CompletionItemKind;
use crate::position::Position;
use crate::range::Range;
use crate::text_model::TextModel;
use crate::text_model_edit_source::TextModelEditSource;

/// `ISnippetInsertOptions`: o `Default` é o `_defaultOptions`.
#[derive(Clone)]
pub struct SnippetInsertOptions {
    pub overwrite_before: i32,
    pub overwrite_after: i32,
    pub adjust_whitespace: bool,
    pub undo_stop_before: bool,
    pub undo_stop_after: bool,
    pub clipboard_text: Option<JsString>,
    pub overtyping_capturer: Option<Rc<dyn IOvertypingCapturer>>,
    pub reason: Option<TextModelEditSource>,
}

impl Default for SnippetInsertOptions {
    fn default() -> Self {
        Self {
            overwrite_before: 0,
            overwrite_after: 0,
            undo_stop_before: true,
            undo_stop_after: true,
            adjust_whitespace: true,
            clipboard_text: None,
            overtyping_capturer: None,
            reason: None,
        }
    }
}

/// `Partial<ISnippetInsertOptions>`: cada campo ausente (`None`) mantém o valor padrão.
#[derive(Clone, Default)]
pub struct PartialSnippetInsertOptions {
    pub overwrite_before: Option<i32>,
    pub overwrite_after: Option<i32>,
    pub adjust_whitespace: Option<bool>,
    pub undo_stop_before: Option<bool>,
    pub undo_stop_after: Option<bool>,
    pub clipboard_text: Option<JsString>,
    pub overtyping_capturer: Option<Rc<dyn IOvertypingCapturer>>,
    pub reason: Option<TextModelEditSource>,
}

impl SnippetInsertOptions {
    /// `typeof opts === 'undefined' ? _defaultOptions : { ..._defaultOptions, ...opts }`.
    pub fn with(opts: Option<PartialSnippetInsertOptions>) -> Self {
        let defaults = Self::default();
        let Some(opts) = opts else {
            return defaults;
        };
        Self {
            overwrite_before: opts.overwrite_before.unwrap_or(defaults.overwrite_before),
            overwrite_after: opts.overwrite_after.unwrap_or(defaults.overwrite_after),
            adjust_whitespace: opts.adjust_whitespace.unwrap_or(defaults.adjust_whitespace),
            undo_stop_before: opts.undo_stop_before.unwrap_or(defaults.undo_stop_before),
            undo_stop_after: opts.undo_stop_after.unwrap_or(defaults.undo_stop_after),
            clipboard_text: opts.clipboard_text.or(defaults.clipboard_text),
            overtyping_capturer: opts.overtyping_capturer.or(defaults.overtyping_capturer),
            reason: opts.reason.or(defaults.reason),
        }
    }

    /// O que a `SnippetSession` lê das opções (as cinco que a sessão conhece).
    fn session_options(&self) -> SnippetSessionInsertOptions {
        SnippetSessionInsertOptions {
            overwrite_before: self.overwrite_before,
            overwrite_after: self.overwrite_after,
            adjust_whitespace: self.adjust_whitespace,
            clipboard_text: self.clipboard_text.clone(),
            overtyping_capturer: self.overtyping_capturer.clone(),
        }
    }
}

/// `command: { id, title }` de um item de opção de `choice`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SnippetChoiceCommand {
    pub id: String,
    pub title: String,
}

/// O `CompletionItem` que o provedor das opções de `choice` devolve.
#[derive(Clone, Debug)]
pub struct SnippetChoiceSuggestion {
    pub kind: CompletionItemKind,
    pub label: String,
    pub insert_text: String,
    pub sort_text: String,
    pub range: Range,
    pub filter_text: Option<String>,
    pub command: SnippetChoiceCommand,
}

/// O seletor do `completionProvider.register({ language, pattern, scheme, exclusive }, provider)`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompletionProviderSelector {
    pub language: String,
    pub pattern: String,
    pub scheme: String,
    pub exclusive: bool,
}

/// O que o controlador usa do `ILanguageFeaturesService` (`completionProvider.register`) e do módulo `suggest`
/// (`showSimpleSuggestions`). Ver "Divergências".
pub trait ISnippetChoiceHost {
    /// `languageFeaturesService.completionProvider.register(selector, provider)`.
    fn register_completion_provider(
        &self,
        selector: CompletionProviderSelector,
        provider: Rc<SnippetChoiceCompletionProvider>,
    ) -> DisposableRef;

    /// `showSimpleSuggestions(editor, provider)`.
    fn show_simple_suggestions(&self, editor: &Rc<CodeEditorWidget>, provider: &Rc<SnippetChoiceCompletionProvider>);
}

/// Os serviços que o construtor e a `SnippetSession` recebem por injeção (ver "Divergências").
pub struct SnippetController2Services {
    /// `@ILogService`.
    pub log_service: LogServiceRef,
    /// `@ILanguageFeaturesService` (e o `showSimpleSuggestions`), se houver.
    pub choice_host: Option<Rc<dyn ISnippetChoiceHost>>,
    /// `@IContextKeyService`.
    pub context_key_service: Rc<dyn IContextKeyService>,
    /// `@ILanguageConfigurationService`, `ILabelService` e `IWorkspaceContextService`.
    pub snippet_services: Rc<SnippetServices>,
}

/// As quatro chaves de contexto (os `static readonly` do original).
struct SnippetContextKeys {
    in_snippet_mode: RawContextKey,
    has_next_tabstop: RawContextKey,
    has_prev_tabstop: RawContextKey,
    in_snippet_choice: RawContextKey,
}

thread_local! {
    static CONTEXT_KEYS: SnippetContextKeys = SnippetContextKeys {
        in_snippet_mode: RawContextKey::new(
            "inSnippetMode",
            false,
            localize("inSnippetMode", "Whether the editor in current in snippet mode", &[]),
        ),
        has_next_tabstop: RawContextKey::new(
            "hasNextTabstop",
            false,
            localize("hasNextTabstop", "Whether there is a next tab stop when in snippet mode", &[]),
        ),
        has_prev_tabstop: RawContextKey::new(
            "hasPrevTabstop",
            false,
            localize("hasPrevTabstop", "Whether there is a previous tab stop when in snippet mode", &[]),
        ),
        in_snippet_choice: RawContextKey::new(
            "inSnippetChoice",
            false,
            localize("inSnippetChoice", "Whether the current snippet tab stop is a choice", &[]),
        ),
    };

    /// O registro de contribuição, comandos e atalhos acontece uma vez por thread.
    static REGISTERED: Cell<bool> = const { Cell::new(false) };
}

/// O provedor de opções de `choice` (o `CompletionItemProvider` com `_debugDisplayName: 'snippetChoiceCompletions'`).
pub struct SnippetChoiceCompletionProvider {
    controller: Weak<SnippetController2>,
}

impl SnippetChoiceCompletionProvider {
    /// `_debugDisplayName`.
    pub fn debug_display_name(&self) -> &'static str {
        "snippetChoiceCompletions"
    }

    /// `provideCompletionItems(model, position)`: as opções do `choice` ativo, ou `None` (o `undefined`).
    pub fn provide_completion_items(&self, model: &Rc<TextModel>, position: Position) -> Option<Vec<SnippetChoiceSuggestion>> {
        let controller = self.controller.upgrade()?;
        let session = controller.session.borrow().clone()?;
        let editor = controller.editor.upgrade()?;
        if !editor.get_model().is_some_and(|editor_model| Rc::ptr_eq(&editor_model, model))
            || editor.get_position() != Some(position)
        {
            return None;
        }
        let active_choice = session.active_choice()?;
        let options = active_choice.choice.as_choice().expect("o choice ativo é um Choice").options.borrow().clone();
        if options.is_empty() {
            return None;
        }

        let word = model.get_value_in_range(&active_choice.range);
        let option_values: Vec<JsString> =
            options.iter().map(|option| option.as_text().expect("a opção é um Text").value.borrow().clone()).collect();
        let is_any_of_options = option_values.contains(&word);
        let suggestions = option_values
            .iter()
            .enumerate()
            .map(|(i, value)| SnippetChoiceSuggestion {
                kind: CompletionItemKind::Value,
                label: value.to_string_lossy(),
                insert_text: value.to_string_lossy(),
                sort_text: "a".repeat(i + 1),
                range: active_choice.range,
                filter_text: is_any_of_options.then(|| format!("{}_{}", word.to_string_lossy(), value.to_string_lossy())),
                command: SnippetChoiceCommand {
                    id: "jumpToNextSnippetPlaceholder".to_owned(),
                    title: localize("next", "Go to next placeholder...", &[]),
                },
            })
            .collect();
        Some(suggestions)
    }
}

/// `_choiceCompletions`: `{ provider, enable(), disable() }` com o `registration` e o `isRegistered` das closures.
struct ChoiceCompletions {
    provider: Rc<SnippetChoiceCompletionProvider>,
    model: Rc<TextModel>,
    host: Option<Rc<dyn ISnippetChoiceHost>>,
    /// `_snippetListener`, onde o registro é guardado.
    snippet_listener: Rc<DisposableStore>,
    registration: RefCell<Option<DisposableRef>>,
    is_registered: Cell<bool>,
}

impl ChoiceCompletions {
    /// `disable()`.
    fn disable(&self) {
        if let Some(registration) = self.registration.borrow().as_ref() {
            registration.dispose();
        }
        self.is_registered.set(false);
    }

    /// `enable()`.
    fn enable(&self) {
        if self.is_registered.get() {
            return;
        }
        if let Some(host) = &self.host {
            let uri = self.model.uri();
            let registration = host.register_completion_provider(
                CompletionProviderSelector {
                    language: self.model.get_language_id(),
                    pattern: uri.fs_path(ProcessEnvironment.platform()).to_string_lossy(),
                    scheme: uri.scheme().to_string_lossy(),
                    exclusive: true,
                },
                self.provider.clone(),
            );
            self.snippet_listener.add(registration.clone());
            *self.registration.borrow_mut() = Some(registration);
        }
        self.is_registered.set(true);
    }
}

/// `SnippetController2`.
pub struct SnippetController2 {
    me: Weak<SnippetController2>,
    editor: Weak<CodeEditorWidget>,
    log_service: LogServiceRef,
    choice_host: Option<Rc<dyn ISnippetChoiceHost>>,
    snippet_services: Rc<SnippetServices>,

    in_snippet: Rc<dyn IContextKey>,
    in_snippet_observable: ISettableObservable<bool, ()>,
    has_next_tabstop: Rc<dyn IContextKey>,
    has_prev_tabstop: Rc<dyn IContextKey>,
    in_snippet_choice: Rc<dyn IContextKey>,

    session: RefCell<Option<Rc<SnippetSession>>>,
    snippet_listener: Rc<DisposableStore>,
    model_version_id: Cell<i64>,
    current_choice: RefCell<Option<MarkerRef>>,
    choice_completions: RefCell<Option<Rc<ChoiceCompletions>>>,
}

impl SnippetController2 {
    pub const ID: &'static str = "snippetController2";

    /// `SnippetController2.get(editor)`.
    pub fn get(editor: &Rc<CodeEditorWidget>) -> Option<Rc<SnippetController2>> {
        editor.get_contribution_as::<SnippetController2>(Self::ID)
    }

    /// `SnippetController2.InSnippetMode`.
    pub fn in_snippet_mode() -> RawContextKey {
        CONTEXT_KEYS.with(|keys| keys.in_snippet_mode.clone())
    }

    /// `SnippetController2.HasNextTabstop`.
    pub fn has_next_tabstop() -> RawContextKey {
        CONTEXT_KEYS.with(|keys| keys.has_next_tabstop.clone())
    }

    /// `SnippetController2.HasPrevTabstop`.
    pub fn has_prev_tabstop() -> RawContextKey {
        CONTEXT_KEYS.with(|keys| keys.has_prev_tabstop.clone())
    }

    /// `SnippetController2.InSnippetChoice`.
    pub fn in_snippet_choice() -> RawContextKey {
        CONTEXT_KEYS.with(|keys| keys.in_snippet_choice.clone())
    }

    pub fn new(editor: &Rc<CodeEditorWidget>, services: SnippetController2Services) -> Rc<Self> {
        let SnippetController2Services { log_service, choice_host, context_key_service, snippet_services } = services;
        let service = &*context_key_service;
        Rc::new_cyclic(|me| Self {
            me: me.clone(),
            editor: Rc::downgrade(editor),
            log_service,
            choice_host,
            snippet_services,
            in_snippet: Self::in_snippet_mode().bind_to(service),
            in_snippet_observable: observable_value::<bool, ()>("snippetController2.inSnippet", false).to_settable(),
            has_next_tabstop: Self::has_next_tabstop().bind_to(service),
            has_prev_tabstop: Self::has_prev_tabstop().bind_to(service),
            in_snippet_choice: Self::in_snippet_choice().bind_to(service),
            session: RefCell::new(None),
            snippet_listener: DisposableStore::new(),
            model_version_id: Cell::new(-1),
            current_choice: RefCell::new(None),
            choice_completions: RefCell::new(None),
        })
    }

    /// As quatro chaves de contexto e o observável voltam ao padrão (o começo comum de `dispose` e `cancel`).
    fn reset_context_keys(&self) {
        self.in_snippet.reset();
        self.in_snippet_observable.set(false, None);
        self.has_prev_tabstop.reset();
        self.has_next_tabstop.reset();
        self.in_snippet_choice.reset();
    }

    /// `apply(edits, opts)`.
    pub fn apply(&self, edits: Vec<SnippetEdit>, opts: Option<PartialSnippetInsertOptions>) {
        let options = SnippetInsertOptions::with(opts);
        if let Err(error) = errors::catch(|| self.do_insert(SnippetTemplate::Edits(edits.clone()), options)) {
            self.cancel(false);
            self.log_failure(&error, "insert_edits=", &format!("{edits:?}"));
        }
    }

    /// `insert(template, opts)`.
    pub fn insert(&self, template: impl Into<JsString>, opts: Option<PartialSnippetInsertOptions>) {
        // this is here to find out more about the yet-not-understood
        // error that sometimes happens when we fail to inserted a nested
        // snippet
        let template = template.into();
        let options = SnippetInsertOptions::with(opts);
        if let Err(error) = errors::catch(|| self.do_insert(SnippetTemplate::Text(template.clone()), options)) {
            self.cancel(false);
            self.log_failure(&error, "insert_template=", &template.to_string_lossy());
        }
    }

    /// O `logService.error(...)` do `catch` de `apply` e `insert`.
    fn log_failure(&self, error: &Error, inserted_label: &str, inserted: &str) {
        self.log_service.error_args(&LogMessage::from(error), &[]);
        self.log_service.error("snippet_error");
        self.log_service.error_args(&LogMessage::from(inserted_label), &[LogArg::from(inserted)]);
        let existing_template = self.session.borrow().as_ref().map_or_else(|| "<no_session>".to_owned(), |session| session.log_info());
        self.log_service.error_args(&LogMessage::from("existing_template="), &[LogArg::from(existing_template.as_str())]);
    }

    /// `_doInsert(template, opts)`.
    fn do_insert(&self, template: SnippetTemplate, opts: SnippetInsertOptions) {
        let Some(editor) = self.editor.upgrade() else {
            return;
        };
        let Some(model) = editor.get_model() else {
            return;
        };

        // don't listen while inserting the snippet
        // as that is the inflight state causing cancelation
        self.snippet_listener.clear();

        if opts.undo_stop_before {
            model.push_stack_element();
        }

        // don't merge
        if matches!(template, SnippetTemplate::Edits(_)) && self.session.borrow().is_some() {
            self.cancel(false);
        }

        let existing = self.session.borrow().clone();
        match existing {
            None => {
                self.model_version_id.set(model.get_alternative_version_id());
                let session = Rc::new(SnippetSession::new(
                    editor.clone(),
                    template,
                    Some(opts.session_options()),
                    self.snippet_services.clone(),
                ));
                *self.session.borrow_mut() = Some(session.clone());
                session.insert(opts.reason.clone());
            }
            Some(session) => {
                let SnippetTemplate::Text(text) = template else {
                    errors::throw(Error::named("TypeError", "Unexpected type"));
                };
                session.merge(text, Some(&opts.session_options()));
            }
        }

        if opts.undo_stop_after {
            model.push_stack_element();
        }

        // regster completion item provider when there is any choice element
        let has_choice = self.session.borrow().as_ref().is_some_and(|session| session.has_choice());
        if has_choice {
            *self.choice_completions.borrow_mut() = Some(Rc::new(ChoiceCompletions {
                provider: Rc::new(SnippetChoiceCompletionProvider { controller: self.me.clone() }),
                model: model.clone(),
                host: self.choice_host.clone(),
                snippet_listener: self.snippet_listener.clone(),
                registration: RefCell::new(None),
                is_registered: Cell::new(false),
            }));
        }

        self.update_state();

        let me = self.me.clone();
        self.snippet_listener.add(editor.on_did_change_model_content().listen({
            let me = me.clone();
            move |e| {
                if e.is_flush
                    && let Some(this) = me.upgrade()
                {
                    this.cancel(false);
                }
            }
        }));
        self.snippet_listener.add(editor.on_did_change_model().listen({
            let me = me.clone();
            move |_| {
                if let Some(this) = me.upgrade() {
                    this.cancel(false);
                }
            }
        }));
        self.snippet_listener.add(editor.on_did_change_cursor_selection().listen(move |_| {
            if let Some(this) = me.upgrade() {
                this.update_state();
            }
        }));
    }

    /// `_updateState()`.
    fn update_state(&self) {
        let Some(session) = self.session.borrow().clone() else {
            // canceled in the meanwhile
            return;
        };
        let Some(model) = self.editor.upgrade().and_then(|editor| editor.get_model()) else {
            return;
        };

        if self.model_version_id.get() == model.get_alternative_version_id() {
            // undo until the 'before' state happened
            // and makes use cancel snippet mode
            return self.cancel(false);
        }

        if !session.has_placeholder() {
            // don't listen for selection changes and don't
            // update context keys when the snippet is plain text
            return self.cancel(false);
        }

        if session.is_at_last_placeholder() || !session.is_selection_within_placeholders() {
            model.push_stack_element();
            return self.cancel(false);
        }

        self.in_snippet.set(Dynamic::Bool(true));
        self.in_snippet_observable.set(true, None);
        self.has_prev_tabstop.set(Dynamic::Bool(!session.is_at_first_placeholder()));
        self.has_next_tabstop.set(Dynamic::Bool(!session.is_at_last_placeholder()));

        self.handle_choice();
    }

    /// `_handleChoice()`.
    fn handle_choice(&self) {
        let session = self.session.borrow().clone();
        let has_model = self.editor.upgrade().is_some_and(|editor| editor.has_model());
        let Some(session) = session.filter(|_| has_model) else {
            *self.current_choice.borrow_mut() = None;
            self.in_snippet_choice.reset();
            return;
        };

        let choice_completions = self.choice_completions.borrow().clone();
        let (Some(active_choice), Some(choice_completions)) = (session.active_choice(), choice_completions.clone()) else {
            if let Some(choice_completions) = choice_completions {
                choice_completions.disable();
            }
            *self.current_choice.borrow_mut() = None;
            self.in_snippet_choice.reset();
            return;
        };

        self.in_snippet_choice.set(Dynamic::Bool(true));

        let is_current = self.current_choice.borrow().as_ref().is_some_and(|current| Rc::ptr_eq(current, &active_choice.choice));
        if !is_current {
            *self.current_choice.borrow_mut() = Some(active_choice.choice);

            choice_completions.enable();

            // trigger suggest with the special choice completion provider
            let me = self.me.clone();
            queue_microtask(move || {
                let Some(this) = me.upgrade() else {
                    return;
                };
                let choice_completions = this.choice_completions.borrow().clone();
                if let (Some(choice_completions), Some(editor)) = (choice_completions, this.editor.upgrade())
                    && let Some(host) = &choice_completions.host
                {
                    host.show_simple_suggestions(&editor, &choice_completions.provider);
                }
            });
        }
    }

    pub fn finish(&self) {
        while self.in_snippet.get().is_truthy() {
            self.next();
        }
    }

    pub fn cancel(&self, reset_selection: bool) {
        self.reset_context_keys();
        self.snippet_listener.clear();

        *self.current_choice.borrow_mut() = None;

        let session = self.session.borrow_mut().take();
        if let Some(session) = session {
            session.dispose();
        }
        self.model_version_id.set(-1);
        if reset_selection && let Some(editor) = self.editor.upgrade() {
            // reset selection to the primary cursor when being asked
            // for. this happens when explicitly cancelling snippet mode,
            // e.g. when pressing ESC
            let selection = editor.get_selection().expect("o editor tem o modelo");
            editor.set_selections(&[selection], None, None);
        }
    }

    pub fn prev(&self) {
        let session = self.session.borrow().clone();
        if let Some(session) = session {
            session.prev();
        }
        self.update_state();
    }

    #[allow(clippy::should_implement_trait)]
    pub fn next(&self) {
        let session = self.session.borrow().clone();
        if let Some(session) = session {
            session.next();
        }
        self.update_state();
    }

    pub fn is_in_snippet(&self) -> bool {
        self.in_snippet.get().is_truthy()
    }

    /// `isInSnippetObservable`.
    pub fn is_in_snippet_observable(&self) -> IObservable<bool> {
        self.in_snippet_observable.to_observable()
    }

    /// `getSessionEnclosingRange()`.
    pub fn get_session_enclosing_range(&self) -> Option<Range> {
        self.session.borrow().as_ref()?.get_enclosing_range()
    }
}

impl IDisposable for SnippetController2 {
    fn dispose(&self) {
        self.reset_context_keys();
        let session = self.session.borrow().clone();
        if let Some(session) = session {
            session.dispose();
        }
        self.snippet_listener.dispose();
    }
}

impl IEditorContribution for SnippetController2 {}

/// A condição `inSnippetMode` mais outra chave: o `precondition` dos comandos de navegação.
fn in_snippet_mode_and(other: RawContextKey) -> Option<ContextKeyExpression> {
    ContextKeyExpr::and([Some(SnippetController2::in_snippet_mode().expr()), Some(other.expr())])
}

/// As chamadas `registerEditorContribution(...)`, `registerEditorCommand(...)` e
/// `KeybindingsRegistry.registerKeybindingRule(...)` do fim do arquivo (o que o carregamento do módulo faz no
/// original). `services` entrega os [`SnippetController2Services`] de cada editor (o `createInstance` do original).
/// É idempotente: o registro acontece uma vez por thread.
pub fn register_snippet_controller2(services: Rc<dyn Fn(&Rc<CodeEditorWidget>) -> SnippetController2Services>) {
    if REGISTERED.with(|registered| registered.replace(true)) {
        return;
    }
    register_editor_contribution(
        SnippetController2::ID,
        Rc::new(move |editor: &Rc<CodeEditorWidget>| -> Rc<dyn IEditorContribution> {
            SnippetController2::new(editor, services(editor))
        }),
        EditorContributionInstantiation::Lazy,
    );

    let command = bind_to_contribution::<SnippetController2>(Rc::new(SnippetController2::get));
    let editor_contrib_weight = KeybindingWeight::EDITOR_CONTRIB + 30;
    let primary = |key_code: u32| Keybindings { primary: Some(key_code), ..Keybindings::default() };

    register_editor_command(Rc::new(command(ContributionCommandOptions {
        command: CommandOptions {
            precondition: in_snippet_mode_and(SnippetController2::has_next_tabstop()),
            kb_opts: vec![CommandKeybindingsOptions::new(
                editor_contrib_weight,
                Some(text_input_focus()),
                primary(key(0, KeyCode::Tab)),
            )],
            ..CommandOptions::new("jumpToNextSnippetPlaceholder")
        },
        handler: Rc::new(|controller: &SnippetController2, _| controller.next()),
    })));

    register_editor_command(Rc::new(command(ContributionCommandOptions {
        command: CommandOptions {
            precondition: in_snippet_mode_and(SnippetController2::has_prev_tabstop()),
            kb_opts: vec![CommandKeybindingsOptions::new(
                editor_contrib_weight,
                Some(text_input_focus()),
                primary(key(KeyMod::SHIFT, KeyCode::Tab)),
            )],
            ..CommandOptions::new("jumpToPrevSnippetPlaceholder")
        },
        handler: Rc::new(|controller: &SnippetController2, _| controller.prev()),
    })));

    // When a snippet choice is active, the suggest widget is shown and would normally
    // claim Shift+Tab for `acceptAlternativeSelectedSuggestion`. Register a higher-weight
    // rule so Shift+Tab navigates to the previous snippet placeholder instead. See #236489.
    keybindings_registry().register_keybinding_rule(KeybindingRule {
        keybindings: primary(key(KeyMod::SHIFT, KeyCode::Tab)),
        id: "jumpToPrevSnippetPlaceholder".to_owned(),
        weight: KeybindingWeight::EDITOR_CONTRIB + 91,
        args: Dynamic::Undefined,
        when: ContextKeyExpr::and([
            Some(SnippetController2::in_snippet_mode().expr()),
            Some(SnippetController2::has_prev_tabstop().expr()),
            Some(SnippetController2::in_snippet_choice().expr()),
            Some(text_input_focus()),
        ]),
    });

    register_editor_command(Rc::new(command(ContributionCommandOptions {
        command: CommandOptions {
            precondition: Some(SnippetController2::in_snippet_mode().expr()),
            kb_opts: vec![CommandKeybindingsOptions::new(
                editor_contrib_weight,
                Some(text_input_focus()),
                Keybindings {
                    primary: Some(KeyCode::Escape.bits()),
                    secondary: Some(vec![key(KeyMod::SHIFT, KeyCode::Escape)]),
                    ..Keybindings::default()
                },
            )],
            ..CommandOptions::new("leaveSnippet")
        },
        handler: Rc::new(|controller: &SnippetController2, _| controller.cancel(true)),
    })));

    register_editor_command(Rc::new(command(ContributionCommandOptions {
        command: CommandOptions {
            precondition: Some(SnippetController2::in_snippet_mode().expr()),
            ..CommandOptions::new("acceptSnippet")
        },
        handler: Rc::new(|controller: &SnippetController2, _| controller.finish()),
    })));
}
