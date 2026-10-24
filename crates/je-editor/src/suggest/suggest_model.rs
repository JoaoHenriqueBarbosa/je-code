//! Porte de `src/vs/editor/contrib/suggest/browser/suggestModel.ts`: o modelo de sugestão do editor
//! (`SuggestModel`), que escuta o editor, decide quando disparar, redisparar, refiltrar ou cancelar o pedido de
//! sugestões e guarda o `CompletionModel` da sessão ativa, junto do `LineContext`, dos tipos de evento e das opções de
//! disparo.
//!
//! Representação:
//! - `SuggestModel implements IDisposable` é a struct [`SuggestModel`], criada por [`SuggestModel::new`], que devolve
//!   `Rc<Self>`: os ouvintes do editor guardam só um [`Weak`] do modelo e o editor é dono deles. O `_toDispose` é o
//!   `DisposableStore` do modelo, o `_triggerCharacterListener` e o `_completionDisposables` os seus homônimos.
//! - O editor é o `Rc<CodeEditorWidget>` (`ICodeEditor` não é objeto de trait, tem métodos genéricos).
//! - Os nove argumentos de injeção do construtor são o [`SuggestModelServices`]; `ILanguageFeaturesService` entra como
//!   os dois registros que o modelo lê (`completionProvider` e, por [`IInlineCompletionsHost`], o
//!   `inlineCompletionsProvider`), e `IConfigurationService`, que o original recebe e nunca lê, não existe aqui.
//! - `interface ICancelEvent`, `ITriggerEvent`, `ISuggestEvent` e `SuggestTriggerOptions` são structs com os mesmos
//!   campos. Os campos opcionais são `Option`; o `completionOptions?: Partial<CompletionOptions>` é o
//!   [`PartialCompletionOptions`] (cada campo ausente cai no padrão do construtor de `CompletionOptions`). O
//!   `SuggestTriggerOptions` tem `Default` (`auto: false` e o resto ausente) para o `{ auto: true }` do original ser
//!   `SuggestTriggerOptions { auto: true, ..Default::default() }`.
//! - `const enum State` é o enum [`State`] (`Idle`, `Manual`, `Auto`).
//! - `class LineContext` é a struct [`LineContext`] (a homônima de `completion_model.rs` é a interface do filtro; esta
//!   guarda o texto à esquerda do cursor como [`JsString`], porque as colunas contam unidades UTF-16, e entrega o texto
//!   em UTF-8 ao `CompletionModel`). `LineContext.shouldAutoTrigger` é [`LineContext::should_auto_trigger`].
//! - `Set<CompletionItemKind>` é um `HashSet`; `Map<string, Set<CompletionItemProvider>>` é um `HashMap<String,
//!   ProviderSet>` (só consulta por chave, a ordem não importa); `Set` e `Map` de provedores são o [`ProviderSet`] e o
//!   [`ProviderItems`] de `suggest.rs`.
//! - `static createSuggestFilter` é [`SuggestModel::create_suggest_filter`], que devolve o [`SuggestFilter`]. O
//!   `if (!suggestOptions.showX) result.add(Kind)` repetido vira uma tabela de pares `(mostrar, tipo)`: a ordem de
//!   inserção num conjunto não é observável. O `showColors` que adiciona `Color` e `Customcolor` é o do original.
//! - `Promise.all([completions, wordDistance])` é [`join2`]: o resultado só sai quando as duas assentam e a primeira
//!   rejeição rejeita o par. O `.then(async (...) => ...).catch(onUnexpectedError)` é um `async_fn` seguido de um
//!   `then_result` que repassa o erro a `on_unexpected_error`.
//! - `canShowQuickSuggest` e `canShowSuggestOnTriggerCharacters` são idênticas no original (a primeira lê as chaves por
//!   `InlineCompletionContextKeys`, a segunda pelo nome literal, que é o mesmo texto): uma função só,
//!   [`can_show_suggest_with_inline_suggestion`], com o `configurationService` que as duas recebem e não leem fora.
//! - `setTimeout(() => ..., undefined)` da telemetria é `runtime::set_timeout` com atraso 0. `JSON.stringify(durations)`
//!   é o `json_stringify` do [`Dynamic`] montado com os nomes de campo do original (`entries`, `providerName`,
//!   `elapsedProvider`, `elapsedOverall` e `elapsed`), que é o formato do evento.
//! - O `console`/`ILogService.warn(message, item.completion)` leva o item como texto de depuração (`{:?}`).
//! - `assertType` lança no original; aqui é o `assert_type` de `je-base` (um pânico que vira `Error`).
//!
//! Divergências:
//! - `ILanguageFeaturesService.inlineCompletionsProvider`, `getInlineCompletionsController` e o
//!   `InlineCompletionsModel` do original (`model`, `state`, `status`, `stop`) ainda não têm porte com observáveis. O
//!   modelo lê só o que o disparo por `offWhenInlineCompletions` usa, por três contratos reduzidos:
//!   [`IInlineCompletionsHost`] (se há provedor para o modelo e o controlador do editor), [`IInlineCompletionsController`]
//!   (o observável `model`) e [`IInlineCompletionsModel`] (os observáveis `status` e `state` e o `stop`). Sem o host
//!   (`None` nos serviços) não há provedor nem controlador, que é o caso de um editor sem inline completions, e o
//!   comportamento é o do original para `model == null`. O `state?.inlineSuggestion` do original é o
//!   [`InlineCompletionsModelState::has_inline_suggestion`].
//! - `IClipboardService`, `ITelemetryService`, `ILogService`, `IEnvironmentService` e `IEditorWorkerService` são os
//!   contratos já existentes do porte (o do clipboard e o do worker são recortes, com o que o modelo lê).
//! - O `CompletionItem` de `languages.rs` não tem `extensionId`: nada do modelo o lê.

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::rc::{Rc, Weak};

use je_base::JsString;
use je_base::async_::{TimeoutTimer, async_fn, disposable_timeout};
use je_base::cancellation::CancellationTokenSource;
use je_base::dynamic::{Dynamic, Truthy, json_stringify};
use je_base::errors::{Error, on_unexpected_error};
use je_base::event::{Emitter, Event};
use je_base::filters::FuzzyScoreOptions;
use je_base::lifecycle::{DisposableStore, IDisposable};
use je_base::observable::{IObservable, Observable, register_autorun_self_disposable};
use je_base::promise::Promise;
use je_base::random::random_f64;
use je_base::runtime;
use je_base::strings::{get_leading_whitespace, is_high_surrogate, is_low_surrogate};
use je_base::types::assert_type;
use je_platform::contextkey::IContextKeyService;
use je_platform::environment::environment::EnvironmentService;
use je_platform::js::string_to_number;
use je_platform::log::{LogArg, LogMessage, LogServiceRef, Logger};
use je_platform::telemetry::telemetry::ITelemetryService;

use crate::code_editor_widget::{CodeEditorWidget, ICodeEditor};
use crate::config::editor_options::inline_suggest::INLINE_SUGGEST;
use crate::config::editor_options::quick_suggestions::{QUICK_SUGGESTIONS, QuickSuggestionsValue};
use crate::config::editor_options::registry::{
    QUICK_SUGGESTIONS_DELAY, READ_ONLY, SNIPPET_SUGGESTIONS, SUGGEST_ON_TRIGGER_CHARACTERS,
};
use crate::config::editor_options::suggest::SUGGEST;
use crate::cursor_common::CursorSimpleModel;
use crate::cursor_events::{CursorChangeReason, ICursorSelectionChangedEvent};
use crate::editor_worker::IEditorWorkerService;
use crate::language_feature_registry::LanguageFeatureRegistry;
use crate::languages::{CompletionContext, CompletionItemKind, CompletionItemProvider, CompletionTriggerKind};
use crate::line_tokens::IViewLineTokens;
use crate::position::Position;
use crate::selection::Selection;
use crate::services::clipboard_service::IClipboardService;
use crate::snippet::snippet_controller2::SnippetController2;
use crate::suggest::completion_model::{CompletionModel, LineContext as CompletionLineContext};
use crate::suggest::suggest::{
    CompletionDurations, CompletionItemModel, CompletionOptions, ProviderItems, ProviderSet, QuickSuggestionsOptions,
    SnippetSortOrder, get_snippet_suggest_support, provide_suggestion_items,
};
use crate::suggest::word_distance::WordDistance;
use crate::text_model::TextModel;
use crate::word_helper::WordAtPosition;

/// `ICancelEvent`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ICancelEvent {
    pub retrigger: bool,
}

/// `ITriggerEvent`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ITriggerEvent {
    pub auto: bool,
    pub shy: bool,
    pub position: Position,
}

/// `ISuggestEvent`.
#[derive(Clone)]
pub struct ISuggestEvent {
    pub completion_model: Rc<CompletionModel>,
    pub is_frozen: bool,
    pub trigger_options: SuggestTriggerOptions,
}

/// `Partial<CompletionOptions>` do `completionOptions` de [`SuggestTriggerOptions`]: o campo ausente cai no padrão do
/// construtor de `CompletionOptions`.
#[derive(Clone, Default)]
pub struct PartialCompletionOptions {
    pub kind_filter: Option<HashSet<CompletionItemKind>>,
    pub provider_filter: Option<ProviderSet>,
    pub provider_items_to_reuse: Option<ProviderItems>,
}

/// `SuggestTriggerOptions`.
#[derive(Clone, Default)]
pub struct SuggestTriggerOptions {
    pub auto: bool,
    pub shy: Option<bool>,
    pub refilter: Option<bool>,
    pub retrigger: Option<bool>,
    pub trigger_kind: Option<CompletionTriggerKind>,
    pub trigger_character: Option<String>,
    pub clipboard_text: Option<String>,
    pub completion_options: Option<PartialCompletionOptions>,
}

/// O texto à esquerda do cursor na linha (`model.getLineContent(line).substr(0, column - 1)`).
fn leading_line_content_of(model: &TextModel, position: Position) -> JsString {
    let line = model.get_line_content(position.line_number);
    let end = usize::try_from(position.column - 1).unwrap_or(0).min(line.len());
    line.slice(0, end)
}

/// `LineContext`.
pub struct LineContext {
    pub line_number: i32,
    pub column: i32,
    pub leading_line_content: JsString,
    pub leading_word: WordAtPosition,
    pub trigger_options: SuggestTriggerOptions,
}

impl LineContext {
    pub fn should_auto_trigger(editor: &CodeEditorWidget) -> bool {
        if !editor.has_model() {
            return false;
        }
        let model = editor.get_model().expect("o editor tem modelo");
        let pos = editor.get_position().expect("o editor com modelo tem posição");
        model.tokenization().tokenize_if_cheap(pos.line_number);

        let Some(word) = model.get_word_at_position(pos) else {
            return false;
        };
        if word.end_column != pos.column && word.start_column + 1 != pos.column
        /* after typing a single character before a word */
        {
            return false;
        }
        if !string_to_number(word.word.units()).is_nan() {
            return false;
        }
        true
    }

    pub fn new(model: &TextModel, position: Position, trigger_options: SuggestTriggerOptions) -> Self {
        Self {
            leading_line_content: leading_line_content_of(model, position),
            leading_word: model.get_word_until_position(position),
            line_number: position.line_number,
            column: position.column,
            trigger_options,
        }
    }
}

/// `State`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    Idle = 0,
    Manual = 1,
    Auto = 2,
}

/// `status` do `InlineCompletionsModel`: só o que o modelo de sugestão compara (`'loading'`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InlineCompletionsStatus {
    Loading,
    GhostText,
    InlineEdit,
    NoSuggestion,
}

/// `state` do `InlineCompletionsModel` (`{ ... } | undefined`): só o `state?.inlineSuggestion` é lido.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InlineCompletionsModelState {
    /// `state.inlineSuggestion` é verdadeiro (não é `undefined`).
    pub has_inline_suggestion: bool,
}

/// `stopReason` de `InlineCompletionsModel.stop`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InlineCompletionsStopReason {
    ExplicitCancel,
    Automatic,
}

/// `InlineCompletionsModel`, recorte do que o `SuggestModel` lê (ver "Divergências").
pub trait IInlineCompletionsModel {
    /// `status`.
    fn status(&self) -> IObservable<InlineCompletionsStatus>;

    /// `state`.
    fn state(&self) -> IObservable<Option<InlineCompletionsModelState>>;

    /// `stop(stopReason)`.
    fn stop(&self, reason: InlineCompletionsStopReason);
}

/// `InlineCompletionsController`, recorte do que o `SuggestModel` lê (ver "Divergências").
pub trait IInlineCompletionsController {
    /// `model`: o `derivedDisposable` do modelo, `undefined` enquanto não há.
    fn model(&self) -> IObservable<Option<Rc<dyn IInlineCompletionsModel>>>;
}

/// O que o `SuggestModel` lê das inline completions: `languageFeaturesService.inlineCompletionsProvider.has(model)` e
/// `getInlineCompletionsController(editor)` (ver "Divergências").
pub trait IInlineCompletionsHost {
    /// `inlineCompletionsProvider.has(model)`.
    fn has_provider(&self, model: &TextModel) -> bool;

    /// `getInlineCompletionsController(editor)`.
    fn get_controller(&self, editor: &Rc<CodeEditorWidget>) -> Option<Rc<dyn IInlineCompletionsController>>;
}

/// Os argumentos de injeção do construtor de `SuggestModel` (ver "Representação").
pub struct SuggestModelServices {
    /// `@IEditorWorkerService`.
    pub editor_worker_service: Rc<dyn IEditorWorkerService>,
    /// `@IClipboardService`.
    pub clipboard_service: Rc<dyn IClipboardService>,
    /// `@ITelemetryService`.
    pub telemetry_service: Rc<dyn ITelemetryService>,
    /// `@ILogService`.
    pub log_service: LogServiceRef,
    /// `@IContextKeyService`.
    pub context_key_service: Rc<dyn IContextKeyService>,
    /// `languageFeaturesService.completionProvider`.
    pub completion_provider: Rc<LanguageFeatureRegistry<dyn CompletionItemProvider>>,
    /// `languageFeaturesService.inlineCompletionsProvider` e o controlador de inline completions, se houver.
    pub inline_completions: Option<Rc<dyn IInlineCompletionsHost>>,
    /// `@IEnvironmentService`.
    pub environment_service: Rc<dyn EnvironmentService>,
}

/// O nome da chave de contexto `InlineCompletionContextKeys.inlineSuggestionVisible`.
const INLINE_SUGGESTION_VISIBLE: &str = "inlineSuggestionVisible";

/// O nome da chave de contexto `InlineCompletionContextKeys.suppressSuggestions`.
const INLINE_SUGGESTION_SUPPRESS_SUGGESTIONS: &str = "inlineSuggestionSuppressSuggestions";

/// `canShowQuickSuggest` e `canShowSuggestOnTriggerCharacters` (ver "Representação").
fn can_show_suggest_with_inline_suggestion(editor: &CodeEditorWidget, context_key_service: &dyn IContextKeyService) -> bool {
    if !context_key_service.get_context_key_value(INLINE_SUGGESTION_VISIBLE).is_truthy() {
        // Allow if there is no inline suggestion.
        return true;
    }
    let suppress_suggestions = context_key_service.get_context_key_value(INLINE_SUGGESTION_SUPPRESS_SUGGESTIONS);
    if !suppress_suggestions.is_undefined() {
        return !suppress_suggestions.is_truthy();
    }
    !editor.get_option(INLINE_SUGGEST).suppress_suggestions
}

/// `Promise.all([a, b])` para dois tipos: resolve com o par quando as duas assentam, e a primeira rejeição rejeita.
fn join2<A: Clone + 'static, B: Clone + 'static>(a: Promise<A>, b: Promise<B>) -> Promise<(A, B)> {
    let (promise, resolver) = Promise::new();
    let slots: Rc<RefCell<(Option<A>, Option<B>)>> = Rc::new(RefCell::new((None, None)));

    let finish: Rc<dyn Fn()> = {
        let slots = Rc::clone(&slots);
        let resolver = resolver.clone();
        Rc::new(move || {
            let pair = {
                let slots = slots.borrow();
                match (&slots.0, &slots.1) {
                    (Some(a), Some(b)) => Some((a.clone(), b.clone())),
                    _ => None,
                }
            };
            if let Some(pair) = pair {
                resolver.resolve(pair);
            }
        })
    };

    {
        let (slots, resolver, finish) = (Rc::clone(&slots), resolver.clone(), Rc::clone(&finish));
        a.then_result(move |result| {
            match result {
                Ok(value) => {
                    slots.borrow_mut().0 = Some(value);
                    finish();
                }
                Err(error) => resolver.reject(error),
            }
            Ok::<(), Error>(())
        });
    }
    {
        let (slots, resolver) = (Rc::clone(&slots), resolver.clone());
        b.then_result(move |result| {
            match result {
                Ok(value) => {
                    slots.borrow_mut().1 = Some(value);
                    finish();
                }
                Err(error) => resolver.reject(error),
            }
            Ok::<(), Error>(())
        });
    }
    promise
}

/// O resultado de [`SuggestModel::create_suggest_filter`].
pub struct SuggestFilter {
    pub item_kind: HashSet<CompletionItemKind>,
    pub show_deprecated: bool,
}

/// O estado mutável do modelo.
struct ModelState {
    trigger_state: Option<SuggestTriggerOptions>,
    request_token: Option<CancellationTokenSource>,
    context: Option<Rc<LineContext>>,
    current_selection: Selection,
    completion_model: Option<Rc<CompletionModel>>,
}

/// `SuggestModel`.
pub struct SuggestModel {
    editor: Rc<CodeEditorWidget>,
    services: SuggestModelServices,
    to_dispose: Rc<DisposableStore>,
    trigger_character_listener: Rc<DisposableStore>,
    trigger_quick_suggest: Rc<TimeoutTimer>,
    wait_for_inline_completions: RefCell<Option<Rc<DisposableStore>>>,
    inner: RefCell<ModelState>,
    completion_disposables: Rc<DisposableStore>,
    on_did_cancel: Rc<Emitter<ICancelEvent>>,
    on_did_trigger: Rc<Emitter<ITriggerEvent>>,
    on_did_suggest: Rc<Emitter<ISuggestEvent>>,
    me: Weak<SuggestModel>,
}

impl SuggestModel {
    pub fn new(editor: &Rc<CodeEditorWidget>, services: SuggestModelServices) -> Rc<Self> {
        let current_selection = editor.get_selection().unwrap_or(Selection::new(1, 1, 1, 1));
        let model = Rc::new_cyclic(|me| Self {
            editor: Rc::clone(editor),
            services,
            to_dispose: DisposableStore::new(),
            trigger_character_listener: DisposableStore::new(),
            trigger_quick_suggest: TimeoutTimer::new(),
            wait_for_inline_completions: RefCell::new(None),
            inner: RefCell::new(ModelState {
                trigger_state: None,
                request_token: None,
                context: None,
                current_selection,
                completion_model: None,
            }),
            completion_disposables: DisposableStore::new(),
            on_did_cancel: Emitter::new(),
            on_did_trigger: Emitter::new(),
            on_did_suggest: Emitter::new(),
            me: me.clone(),
        });

        // wire up various listeners
        Self::listen(&model, editor.on_did_change_model(), |model, _| {
            model.update_trigger_characters();
            model.cancel(false);
        });
        Self::listen(&model, editor.on_did_change_model_language(), |model, _| {
            model.update_trigger_characters();
            model.cancel(false);
        });
        Self::listen(&model, editor.on_did_change_configuration(), |model, _| {
            model.update_trigger_characters();
        });
        Self::listen(&model, model.services.completion_provider.on_did_change(), |model, _| {
            model.update_trigger_characters();
            model.update_active_suggest_session();
        });

        let editor_is_composing = Rc::new(Cell::new(false));
        {
            let editor_is_composing = Rc::clone(&editor_is_composing);
            Self::listen(&model, editor.on_did_composition_start(), move |_, _| editor_is_composing.set(true));
        }
        {
            let editor_is_composing = Rc::clone(&editor_is_composing);
            Self::listen(&model, editor.on_did_composition_end(), move |model, _| {
                editor_is_composing.set(false);
                model.on_composition_end();
            });
        }
        {
            let editor_is_composing = Rc::clone(&editor_is_composing);
            Self::listen(&model, editor.on_did_change_cursor_selection(), move |model, e| {
                // only trigger suggest when the editor isn't composing a character
                if !editor_is_composing.get() {
                    model.on_cursor_change(e);
                }
            });
        }
        Self::listen(&model, editor.on_did_change_model_content(), move |model, _| {
            // only filter completions when the editor isn't composing a character
            // e.g. ¨ + u makes ü but just ¨ cannot be used for filtering
            if !editor_is_composing.get() && model.inner.borrow().trigger_state.is_some() {
                model.refilter_completion_items();
            }
        });

        model.update_trigger_characters();
        model
    }

    /// `this._toDispose.add(event(handler))`: o ouvinte guarda só um [`Weak`] do modelo.
    fn listen<E: 'static>(model: &Rc<Self>, event: Event<E>, handler: impl Fn(&Self, &E) + 'static) {
        let weak = Rc::downgrade(model);
        event.listen_in(
            move |e| {
                if let Some(model) = weak.upgrade() {
                    handler(&model, e);
                }
            },
            &model.to_dispose,
        );
    }

    /// O `Rc` deste modelo (o modelo só vive dentro de um `Rc`, criado por [`SuggestModel::new`]).
    fn rc(&self) -> Rc<Self> {
        self.me.upgrade().expect("o SuggestModel vive dentro de um Rc")
    }

    /// `onDidCancel`.
    pub fn on_did_cancel(&self) -> Event<ICancelEvent> {
        self.on_did_cancel.event()
    }

    /// `onDidTrigger`.
    pub fn on_did_trigger(&self) -> Event<ITriggerEvent> {
        self.on_did_trigger.event()
    }

    /// `onDidSuggest`.
    pub fn on_did_suggest(&self) -> Event<ISuggestEvent> {
        self.on_did_suggest.event()
    }

    fn update_trigger_characters(&self) {
        self.trigger_character_listener.clear();

        if *self.editor.get_option(READ_ONLY)
            || !self.editor.has_model()
            || !*self.editor.get_option(SUGGEST_ON_TRIGGER_CHARACTERS)
        {
            return;
        }

        let model = self.editor.get_model().expect("o editor tem modelo");
        let mut supports_by_trigger_character: HashMap<String, ProviderSet> = HashMap::new();
        for support in self.services.completion_provider.all(&model) {
            for ch in support.trigger_characters().unwrap_or(&[]) {
                let set = supports_by_trigger_character.entry(ch.clone()).or_insert_with(|| {
                    let mut set = ProviderSet::new();
                    if let Some(suggest_support) = get_snippet_suggest_support() {
                        set.add(suggest_support);
                    }
                    set
                });
                set.add(Rc::clone(&support));
            }
        }

        let supports = Rc::new(supports_by_trigger_character);
        {
            let (weak, supports) = (self.me.clone(), Rc::clone(&supports));
            self.editor.on_did_type().listen_in(
                move |text| {
                    if let Some(model) = weak.upgrade() {
                        model.check_trigger_character(&supports, Some(text));
                    }
                },
                &self.trigger_character_listener,
            );
        }
        {
            let weak = self.me.clone();
            self.editor.on_did_composition_end().listen_in(
                move |_| {
                    if let Some(model) = weak.upgrade() {
                        model.check_trigger_character(&supports, None);
                    }
                },
                &self.trigger_character_listener,
            );
        }
    }

    /// `checkTriggerCharacter(text?)`: `None` é a chamada do `compositionEnd`.
    fn check_trigger_character(&self, supports_by_trigger_character: &HashMap<String, ProviderSet>, text: Option<&JsString>) {
        if !can_show_suggest_with_inline_suggestion(&self.editor, &*self.services.context_key_service) {
            return;
        }

        if LineContext::should_auto_trigger(&self.editor) {
            // don't trigger by trigger characters when this is a case for quick suggest
            return;
        }

        let text = match text {
            Some(text) => text.clone(),
            None => {
                // came here from the compositionEnd-event
                let position = self.editor.get_position().expect("o editor com modelo tem posição");
                let model = self.editor.get_model().expect("o editor tem modelo");
                leading_line_content_of(&model, position)
            }
        };

        let units = text.units();
        let length = units.len();
        let mut last_char = String::new();
        let last_unit = length.checked_sub(1).map(|index| units[index]);
        if last_unit.is_some_and(is_low_surrogate) {
            if length >= 2 && is_high_surrogate(units[length - 2]) {
                last_char = String::from_utf16_lossy(&units[length - 2..]);
            }
        } else if let Some(unit) = last_unit {
            last_char = String::from_utf16_lossy(&[unit]);
        }

        let Some(supports) = supports_by_trigger_character.get(&last_char) else {
            return;
        };

        // keep existing items that where not computed by the
        // supports/providers that want to trigger now
        let completion_model = self.inner.borrow().completion_model.clone();
        let mut provider_items_to_reuse = ProviderItems::new();
        if let Some(completion_model) = &completion_model {
            for (provider, items) in completion_model.get_items_by_provider().iter() {
                if !supports.has(provider) {
                    provider_items_to_reuse.set(Rc::clone(provider), items.clone());
                }
            }
        }

        self.trigger(SuggestTriggerOptions {
            auto: true,
            trigger_kind: Some(CompletionTriggerKind::TriggerCharacter),
            trigger_character: Some(last_char),
            retrigger: Some(completion_model.is_some()),
            clipboard_text: completion_model.as_ref().and_then(|model| model.clipboard_text.clone()),
            completion_options: Some(PartialCompletionOptions {
                provider_filter: Some(supports.clone()),
                provider_items_to_reuse: Some(provider_items_to_reuse),
                ..PartialCompletionOptions::default()
            }),
            ..SuggestTriggerOptions::default()
        });
    }

    // --- trigger/retrigger/cancel suggest

    pub fn state(&self) -> State {
        match &self.inner.borrow().trigger_state {
            None => State::Idle,
            Some(trigger_state) if !trigger_state.auto => State::Manual,
            Some(_) => State::Auto,
        }
    }

    pub fn cancel(&self, retrigger: bool) {
        self.trigger_quick_suggest.cancel();
        self.dispose_wait_for_inline_completions();

        let token = {
            let mut inner = self.inner.borrow_mut();
            if inner.trigger_state.is_none() {
                return;
            }
            inner.request_token.take()
        };
        if let Some(token) = token {
            token.cancel();
        }
        {
            let mut inner = self.inner.borrow_mut();
            inner.trigger_state = None;
            inner.completion_model = None;
            inner.context = None;
        }
        self.on_did_cancel.fire(ICancelEvent { retrigger });
    }

    /// `this._waitForInlineCompletions?.dispose(); this._waitForInlineCompletions = undefined;`
    fn dispose_wait_for_inline_completions(&self) {
        let wait = self.wait_for_inline_completions.borrow_mut().take();
        if let Some(wait) = wait {
            wait.dispose();
        }
    }

    pub fn clear(&self) {
        self.completion_disposables.clear();
    }

    fn update_active_suggest_session(&self) {
        let auto = self.inner.borrow().trigger_state.as_ref().map(|trigger_state| trigger_state.auto);
        if let Some(auto) = auto {
            let has_providers = self.editor.get_model().is_some_and(|model| self.services.completion_provider.has(&model));
            if !has_providers {
                self.cancel(false);
            } else {
                self.trigger(SuggestTriggerOptions { auto, retrigger: Some(true), ..SuggestTriggerOptions::default() });
            }
        }
    }

    fn on_cursor_change(&self, e: &ICursorSelectionChangedEvent) {
        if !self.editor.has_model() {
            return;
        }

        let current_selection = self.editor.get_selection().expect("o editor com modelo tem seleção");
        let previous_selection = std::mem::replace(&mut self.inner.borrow_mut().current_selection, current_selection);

        if !e.selection.is_empty()
            || (e.reason != CursorChangeReason::NotSet && e.reason != CursorChangeReason::Explicit)
            || (e.source != "keyboard" && e.source != "deleteLeft")
        {
            // Early exit if nothing needs to be done!
            // Leave some form of early exit check here if you wish to continue being a cursor position change listener ;)
            self.cancel(false);
            return;
        }

        let is_triggered = self.inner.borrow().trigger_state.is_some();
        if !is_triggered && e.reason == CursorChangeReason::NotSet {
            if previous_selection.contains_range(&current_selection)
                || previous_selection.get_end_position().is_before_or_equal(&current_selection.get_position())
            {
                // cursor did move RIGHT due to typing -> trigger quick suggest
                self.do_trigger_quick_suggest();
            }
        } else if is_triggered && e.reason == CursorChangeReason::Explicit {
            // suggest is active and something like cursor keys are used to move
            // the cursor. this means we can refilter at the new position
            self.refilter_completion_items();
        }
    }

    fn on_composition_end(&self) {
        // trigger or refilter when composition ends
        if self.inner.borrow().trigger_state.is_none() {
            self.do_trigger_quick_suggest();
        } else {
            self.refilter_completion_items();
        }
    }

    fn do_trigger_quick_suggest(&self) {
        if QuickSuggestionsOptions::is_all_off(self.editor.get_option(QUICK_SUGGESTIONS)) {
            // not enabled
            return;
        }

        if self.editor.get_option(SUGGEST).snippets_prevent_quick_suggestions
            && SnippetController2::get(&self.editor).is_some_and(|controller| controller.is_in_snippet())
        {
            // no quick suggestion when in snippet mode
            return;
        }

        self.cancel(false);

        // Cancel any in-flight wait for inline completions from a previous cycle
        self.dispose_wait_for_inline_completions();

        let weak = self.me.clone();
        let delay = *self.editor.get_option(QUICK_SUGGESTIONS_DELAY) as u64;
        self.trigger_quick_suggest.cancel_and_set(
            move || {
                if let Some(model) = weak.upgrade() {
                    model.run_quick_suggest();
                }
            },
            delay,
        );
    }

    /// O corpo do temporizador de `_doTriggerQuickSuggest`.
    fn run_quick_suggest(&self) {
        if self.inner.borrow().trigger_state.is_some() {
            return;
        }
        if !LineContext::should_auto_trigger(&self.editor) {
            return;
        }
        if !self.editor.has_model() || !self.editor.has_widget_focus() {
            return;
        }
        let model = self.editor.get_model().expect("o editor tem modelo");
        let pos = self.editor.get_position().expect("o editor com modelo tem posição");
        // validate enabled now
        let config = *self.editor.get_option(QUICK_SUGGESTIONS);
        if QuickSuggestionsOptions::is_all_off(&config) {
            return;
        }

        let mut wait_for_inline_completions = false;
        if !QuickSuggestionsOptions::is_all_on(&config) {
            // Check the type of the token that triggered this
            model.tokenization().tokenize_if_cheap(pos.line_number);
            let line_tokens = model.tokenization().get_line_tokens(pos.line_number);
            let token_index = line_tokens.find_token_index_at_offset((pos.column - 1 - 1).max(0));
            let token_type = line_tokens.get_standard_token_type(token_index);
            let value = QuickSuggestionsOptions::value_for(&config, token_type);
            if value == QuickSuggestionsValue::Off || value == QuickSuggestionsValue::Inline {
                return;
            }
            if value == QuickSuggestionsValue::OffWhenInlineCompletions {
                wait_for_inline_completions = self
                    .services
                    .inline_completions
                    .as_ref()
                    .is_some_and(|host| host.has_provider(&model))
                    && self.editor.get_option(INLINE_SUGGEST).enabled;
            }
        }

        if !can_show_suggest_with_inline_suggestion(&self.editor, &*self.services.context_key_service) {
            // do not trigger quick suggestions if inline suggestions are shown
            return;
        }

        if !self.services.completion_provider.has(&model) {
            return;
        }

        if wait_for_inline_completions {
            // Wait for inline completions to resolve before deciding
            self.wait_for_inline_completions_and_trigger(&model, pos);
        } else {
            self.trigger(SuggestTriggerOptions { auto: true, ..SuggestTriggerOptions::default() });
        }
    }

    fn wait_for_inline_completions_and_trigger(&self, initial_model: &Rc<TextModel>, initial_position: Position) {
        let initial_model_version = initial_model.get_version_id();
        let inline_controller =
            self.services.inline_completions.as_ref().and_then(|host| host.get_controller(&self.editor));
        let inline_model = inline_controller.as_ref().and_then(|controller| controller.model().get());
        let (Some(inline_controller), Some(inline_model)) = (inline_controller, inline_model) else {
            self.trigger(SuggestTriggerOptions { auto: true, ..SuggestTriggerOptions::default() });
            return;
        };

        let state = inline_model.state().get();
        if state.is_some_and(|state| state.has_inline_suggestion) {
            // Inline completions are already showing - suppress
            return;
        }

        let store = DisposableStore::new();
        *self.wait_for_inline_completions.borrow_mut() = Some(Rc::clone(&store));

        let trigger_and_clean_up: Rc<dyn Fn(bool)> = {
            let weak = self.me.clone();
            let store = Rc::clone(&store);
            let initial_model = Rc::clone(initial_model);
            Rc::new(move |do_trigger: bool| {
                store.dispose();
                let Some(model) = weak.upgrade() else {
                    return;
                };
                {
                    let mut wait = model.wait_for_inline_completions.borrow_mut();
                    if wait.as_ref().is_some_and(|current| Rc::ptr_eq(current, &store)) {
                        *wait = None;
                    }
                }
                if model.inner.borrow().trigger_state.is_some() {
                    return;
                }
                if !do_trigger {
                    return;
                }
                let current_model = model.editor.get_model();
                let current_position = model.editor.get_position();
                if current_model.is_some_and(|current| {
                    Rc::ptr_eq(&current, &initial_model) && current.get_version_id() == initial_model_version
                }) && current_position == Some(initial_position)
                    && model.editor.has_widget_focus()
                {
                    model.trigger(SuggestTriggerOptions { auto: true, ..SuggestTriggerOptions::default() });
                }
            })
        };

        // Reading `inlineController.model` first in a single autorun binds the
        // wait to the model's lifetime: nested autoruns would have no defined
        // run order, so an inner state-watcher could fire on a disposed model
        // before the outer model-watcher cleaned it up.
        {
            let trigger_and_clean_up = Rc::clone(&trigger_and_clean_up);
            let inline_model = Rc::clone(&inline_model);
            disposable_timeout(
                move || {
                    trigger_and_clean_up(true);
                    inline_model.stop(InlineCompletionsStopReason::Automatic);
                },
                750,
                Some(Rc::clone(&store)),
            );
        }

        register_autorun_self_disposable(Rc::clone(&store), move |reader| {
            let current_inline_model = inline_controller.model().read(reader);
            let is_same_model = current_inline_model
                .as_ref()
                .is_some_and(|current| std::ptr::addr_eq(Rc::as_ptr(current), Rc::as_ptr(&inline_model)));
            if !is_same_model {
                reader.dispose();
                trigger_and_clean_up(false);
                return;
            }
            let status = inline_model.status().read(reader);
            let current_state = inline_model.state().read(reader);
            if current_state.is_none() && status == InlineCompletionsStatus::Loading {
                // Still loading
                return;
            }
            reader.dispose();
            trigger_and_clean_up(current_state.is_none());
        });
    }

    fn refilter_completion_items(&self) {
        assert_type(&self.editor.has_model(), None);
        let trigger_state = self.inner.borrow().trigger_state.clone();
        assert_type(&trigger_state.is_some(), None);

        let model = self.editor.get_model().expect("o editor tem modelo");
        let position = self.editor.get_position().expect("o editor com modelo tem posição");
        let trigger_state = trigger_state.expect("conferido acima");
        let ctx = LineContext::new(&model, position, SuggestTriggerOptions { refilter: Some(true), ..trigger_state });
        self.on_new_context(&Rc::new(ctx));
    }

    pub fn trigger(&self, options: SuggestTriggerOptions) {
        if !self.editor.has_model() {
            return;
        }

        let model = self.editor.get_model().expect("o editor tem modelo");
        let position = self.editor.get_position().expect("o editor com modelo tem posição");
        let ctx = Rc::new(LineContext::new(&model, position, options.clone()));

        // Cancel previous requests, change state & update UI
        self.cancel(options.retrigger.unwrap_or(false));
        self.inner.borrow_mut().trigger_state = Some(options.clone());
        self.on_did_trigger.fire(ITriggerEvent {
            auto: options.auto,
            shy: options.shy.unwrap_or(false),
            position: self.editor.get_position().expect("o editor com modelo tem posição"),
        });

        // Capture context when request was sent
        self.inner.borrow_mut().context = Some(ctx);

        // Build context for request
        let mut suggest_ctx = CompletionContext {
            trigger_kind: options.trigger_kind.unwrap_or(CompletionTriggerKind::Invoke),
            trigger_character: None,
        };
        if let Some(trigger_character) = options.trigger_character.clone().filter(|character| !character.is_empty()) {
            suggest_ctx = CompletionContext {
                trigger_kind: CompletionTriggerKind::TriggerCharacter,
                trigger_character: Some(trigger_character),
            };
        }

        let request_token = CancellationTokenSource::new(None);
        let token = request_token.token();
        self.inner.borrow_mut().request_token = Some(request_token);

        // kind filter and snippet sort rules
        let snippet_sort_order = match self.editor.get_option(SNIPPET_SUGGESTIONS).as_str() {
            "top" => SnippetSortOrder::Top,
            // ↓ that's the default anyways...
            // "inline" => SnippetSortOrder::Inline,
            "bottom" => SnippetSortOrder::Bottom,
            _ => SnippetSortOrder::Inline,
        };

        let SuggestFilter { item_kind: item_kind_filter, show_deprecated } = Self::create_suggest_filter(&self.editor);
        let partial = options.completion_options.clone().unwrap_or_default();
        let completion_options = CompletionOptions {
            snippet_sort_order,
            kind_filter: partial.kind_filter.unwrap_or(item_kind_filter),
            provider_filter: partial.provider_filter.unwrap_or_default(),
            provider_items_to_reuse: partial.provider_items_to_reuse.unwrap_or_default(),
            show_deprecated,
        };
        let word_distance = <dyn WordDistance>::create(&self.services.editor_worker_service, &self.editor);

        let completions = provide_suggestion_items(
            &self.services.completion_provider,
            &model,
            position,
            completion_options,
            suggest_ctx,
            &token,
        );

        let this = self.rc();
        async_fn(async move {
            let (completions, word_distance) = join2(completions, word_distance).await?;
            this.on_completions(&options, completions, word_distance).await
        })
        .then_result(|result| {
            if let Err(error) = result {
                on_unexpected_error(&error);
            }
            Ok(())
        });
    }

    /// O `async ([completions, wordDistance]) => { ... }` de `trigger`: roda já, de forma síncrona, até o primeiro
    /// `await`, dentro do `async_fn` de quem chama.
    async fn on_completions(
        &self,
        options: &SuggestTriggerOptions,
        completions: CompletionItemModel,
        word_distance: Rc<dyn WordDistance>,
    ) -> Result<(), Error> {
        // `this._requestToken?.dispose()`
        if let Some(request_token) = self.inner.borrow().request_token.as_ref() {
            request_token.dispose();
        }

        if !self.editor.has_model() {
            completions.disposable.dispose();
            return Ok(());
        }

        let mut clipboard_text = options.clipboard_text.clone();
        if clipboard_text.as_deref().is_none_or(str::is_empty) && completions.needs_clipboard {
            clipboard_text = Some(self.services.clipboard_service.read_text().await?);
        }

        if self.inner.borrow().trigger_state.is_none() {
            completions.disposable.dispose();
            return Ok(());
        }

        let model = self.editor.get_model().expect("o editor tem modelo");
        let position = self.editor.get_position().expect("o editor com modelo tem posição");

        let ctx = Rc::new(LineContext::new(&model, position, options.clone()));
        let suggest_options = self.editor.get_option(SUGGEST);
        let fuzzy_search_options = FuzzyScoreOptions {
            first_match_can_be_weak: !suggest_options.match_on_word_start_only,
            ..FuzzyScoreOptions::DEFAULT
        };
        let context_column = self
            .inner
            .borrow()
            .context
            .as_ref()
            .map(|context| context.column)
            .ok_or_else(|| Error::type_error("Cannot read properties of undefined (reading 'column')"))?;
        let completion_model = Rc::new(CompletionModel::new(
            completions.items.clone(),
            context_column,
            CompletionLineContext {
                leading_line_content: ctx.leading_line_content.to_string_lossy(),
                character_count_delta: ctx.column - context_column,
            },
            word_distance,
            suggest_options.clone(),
            self.editor.get_option(SNIPPET_SUGGESTIONS),
            Some(fuzzy_search_options),
            clipboard_text,
        ));
        self.inner.borrow_mut().completion_model = Some(Rc::clone(&completion_model));

        // store containers so that they can be disposed later
        self.completion_disposables.add(completions.disposable.clone());

        self.on_new_context(&ctx);

        // finally report telemetry about durations
        self.report_durations_telemetry(&completions.durations);

        // report invalid completions by source
        if !self.services.environment_service.is_built() || self.services.environment_service.is_extension_development() {
            for item in &completions.items {
                if item.is_invalid {
                    self.services.log_service.warn_args(
                        &LogMessage::from(format!(
                            "[suggest] did IGNORE invalid completion item from {}",
                            item.provider.debug_display_name()
                        )),
                        &[LogArg::from(format!("{:?}", item.completion.borrow()).as_str())],
                    );
                }
            }
        }
        Ok(())
    }

    /// Report durations telemetry with a 1% sampling rate.
    /// The telemetry is reported only if a random number between 0 and 100 is less than or equal to 1.
    fn report_durations_telemetry(&self, durations: &CompletionDurations) {
        if random_f64() > 0.0001 {
            // 0.01%
            return;
        }

        let data = Dynamic::object([
            (
                "entries",
                Dynamic::array(durations.entries.iter().map(|entry| {
                    Dynamic::object([
                        ("providerName", Dynamic::string(&entry.provider_name)),
                        ("elapsedProvider", Dynamic::Number(entry.elapsed_provider)),
                        ("elapsedOverall", Dynamic::Number(entry.elapsed_overall)),
                    ])
                })),
            ),
            ("elapsed", Dynamic::Number(durations.elapsed)),
        ]);
        let telemetry_service = Rc::clone(&self.services.telemetry_service);
        let log_service = Rc::clone(&self.services.log_service);
        runtime::set_timeout(
            move || {
                let json = json_stringify(&data).unwrap_or_default();
                telemetry_service.public_log2("suggest.durations.json", Some(Dynamic::object([("data", Dynamic::string(&json))])));
                log_service.debug_args("suggest.durations.json", &[LogArg::from(data)]);
            },
            0,
        );
    }

    pub fn create_suggest_filter(editor: &CodeEditorWidget) -> SuggestFilter {
        // kind filter and snippet sort rules
        let mut result = HashSet::new();

        // snippet setting
        if editor.get_option(SNIPPET_SUGGESTIONS) == "none" {
            result.insert(CompletionItemKind::Snippet);
        }

        // type setting
        let suggest_options = editor.get_option(SUGGEST);
        let shown_kinds = [
            (suggest_options.show_methods, CompletionItemKind::Method),
            (suggest_options.show_functions, CompletionItemKind::Function),
            (suggest_options.show_constructors, CompletionItemKind::Constructor),
            (suggest_options.show_fields, CompletionItemKind::Field),
            (suggest_options.show_variables, CompletionItemKind::Variable),
            (suggest_options.show_classes, CompletionItemKind::Class),
            (suggest_options.show_structs, CompletionItemKind::Struct),
            (suggest_options.show_interfaces, CompletionItemKind::Interface),
            (suggest_options.show_modules, CompletionItemKind::Module),
            (suggest_options.show_properties, CompletionItemKind::Property),
            (suggest_options.show_events, CompletionItemKind::Event),
            (suggest_options.show_operators, CompletionItemKind::Operator),
            (suggest_options.show_units, CompletionItemKind::Unit),
            (suggest_options.show_values, CompletionItemKind::Value),
            (suggest_options.show_constants, CompletionItemKind::Constant),
            (suggest_options.show_enums, CompletionItemKind::Enum),
            (suggest_options.show_enum_members, CompletionItemKind::EnumMember),
            (suggest_options.show_keywords, CompletionItemKind::Keyword),
            (suggest_options.show_words, CompletionItemKind::Text),
            (suggest_options.show_colors, CompletionItemKind::Color),
            (suggest_options.show_files, CompletionItemKind::File),
            (suggest_options.show_references, CompletionItemKind::Reference),
            (suggest_options.show_colors, CompletionItemKind::Customcolor),
            (suggest_options.show_folders, CompletionItemKind::Folder),
            (suggest_options.show_type_parameters, CompletionItemKind::TypeParameter),
            (suggest_options.show_snippets, CompletionItemKind::Snippet),
            (suggest_options.show_users, CompletionItemKind::User),
            (suggest_options.show_issues, CompletionItemKind::Issue),
        ];
        for (shown, kind) in shown_kinds {
            if !shown {
                result.insert(kind);
            }
        }

        SuggestFilter { item_kind: result, show_deprecated: suggest_options.show_deprecated }
    }

    fn on_new_context(&self, ctx: &Rc<LineContext>) {
        let Some(context) = self.inner.borrow().context.clone() else {
            // happens when 24x7 IntelliSense is enabled and still in its delay
            return;
        };

        if ctx.line_number != context.line_number {
            // e.g. happens when pressing Enter while IntelliSense is computed
            self.cancel(false);
            return;
        }

        let leading_whitespace = |content: &JsString| get_leading_whitespace(content.units(), 0, content.len()).to_vec();
        if leading_whitespace(&ctx.leading_line_content) != leading_whitespace(&context.leading_line_content) {
            // cancel IntelliSense when line start changes
            // happens when the current word gets outdented
            self.cancel(false);
            return;
        }

        if ctx.column < context.column {
            // typed -> moved cursor LEFT -> retrigger if still on a word
            if !ctx.leading_word.word.is_empty() {
                self.trigger(SuggestTriggerOptions {
                    auto: context.trigger_options.auto,
                    retrigger: Some(true),
                    ..SuggestTriggerOptions::default()
                });
            } else {
                self.cancel(false);
            }
            return;
        }

        let Some(completion_model) = self.inner.borrow().completion_model.clone() else {
            // happens when IntelliSense is not yet computed
            return;
        };

        if !ctx.leading_word.word.is_empty() && ctx.leading_word.start_column > context.leading_word.start_column {
            // started a new word while IntelliSense shows -> retrigger but reuse all items that we currently have
            let should_auto_trigger = LineContext::should_auto_trigger(&self.editor);
            // shouldAutoTrigger forces tokenization, which can cause pending cursor change events to be emitted, which can cause
            // suggestions to be cancelled, which causes `this._context` to be undefined
            let current_context = self.inner.borrow().context.clone();
            if should_auto_trigger && let Some(current_context) = current_context {
                let map = completion_model.get_items_by_provider();
                self.trigger(SuggestTriggerOptions {
                    auto: current_context.trigger_options.auto,
                    retrigger: Some(true),
                    clipboard_text: completion_model.clipboard_text.clone(),
                    completion_options: Some(PartialCompletionOptions {
                        provider_items_to_reuse: Some((*map).clone()),
                        ..PartialCompletionOptions::default()
                    }),
                    ..SuggestTriggerOptions::default()
                });
            }
            return;
        }

        if ctx.column > context.column
            && completion_model.get_incomplete_provider().size() > 0
            && !ctx.leading_word.word.is_empty()
        {
            // typed -> moved cursor RIGHT & incomple model & still on a word -> retrigger

            let mut provider_items_to_reuse = ProviderItems::new();
            let mut provider_filter = ProviderSet::new();
            for (provider, items) in completion_model.get_items_by_provider().iter() {
                if items.first().is_some_and(|item| item.container.incomplete == Some(true)) {
                    provider_filter.add(Rc::clone(provider));
                } else {
                    provider_items_to_reuse.set(Rc::clone(provider), items.clone());
                }
            }

            self.trigger(SuggestTriggerOptions {
                auto: context.trigger_options.auto,
                trigger_kind: Some(CompletionTriggerKind::TriggerForIncompleteCompletions),
                retrigger: Some(true),
                clipboard_text: completion_model.clipboard_text.clone(),
                completion_options: Some(PartialCompletionOptions {
                    provider_filter: Some(provider_filter),
                    provider_items_to_reuse: Some(provider_items_to_reuse),
                    ..PartialCompletionOptions::default()
                }),
                ..SuggestTriggerOptions::default()
            });
        } else {
            // typed -> moved cursor RIGHT -> update UI
            let old_line_context = completion_model.line_context();
            let mut is_frozen = false;

            completion_model.set_line_context(CompletionLineContext {
                leading_line_content: ctx.leading_line_content.to_string_lossy(),
                character_count_delta: ctx.column - context.column,
            });

            if completion_model.items().is_empty() {
                let should_auto_trigger = LineContext::should_auto_trigger(&self.editor);
                let Some(current_context) = self.inner.borrow().context.clone() else {
                    // shouldAutoTrigger forces tokenization, which can cause pending cursor change events to be emitted, which can cause
                    // suggestions to be cancelled, which causes `this._context` to be undefined
                    self.cancel(false);
                    return;
                };

                if should_auto_trigger && current_context.leading_word.end_column < ctx.leading_word.start_column {
                    // retrigger when heading into a new word
                    self.trigger(SuggestTriggerOptions {
                        auto: current_context.trigger_options.auto,
                        retrigger: Some(true),
                        ..SuggestTriggerOptions::default()
                    });
                    return;
                }

                if !current_context.trigger_options.auto {
                    // freeze when IntelliSense was manually requested
                    completion_model.set_line_context(old_line_context);
                    is_frozen = !completion_model.items().is_empty();

                    if is_frozen && ctx.leading_word.word.is_empty() {
                        // there were results before but now there aren't
                        // and also we are not on a word anymore -> cancel
                        self.cancel(false);
                        return;
                    }
                } else {
                    // nothing left
                    self.cancel(false);
                    return;
                }
            }

            self.on_did_suggest.fire(ISuggestEvent {
                completion_model,
                trigger_options: ctx.trigger_options.clone(),
                is_frozen,
            });
        }
    }
}

impl IDisposable for SuggestModel {
    fn dispose(&self) {
        self.trigger_character_listener.dispose();
        self.on_did_cancel.dispose();
        self.on_did_suggest.dispose();
        self.on_did_trigger.dispose();
        self.trigger_quick_suggest.dispose();
        self.dispose_wait_for_inline_completions();
        self.to_dispose.dispose();
        self.completion_disposables.dispose();
        self.cancel(false);
    }
}
