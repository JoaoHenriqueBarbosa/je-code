//! Porte de `src/vs/editor/contrib/inlineCompletions/test/browser/utils.ts`: `MockInlineCompletionsProvider`,
//! `MockSearchReplaceCompletionsProvider`, `InlineEditContext`, `GhostTextContext`,
//! `withAsyncTestCodeEditorAndInlineCompletionsModel`, `AnnotatedString` e `AnnotatedText`.
//!
//! Representação:
//! - `runWithFakedTimers({ useFakeTimers: options.fakeClock })` é de quem chama: o caso roda dentro de
//!   `run_with_faked_timers` (`fakeClock: true`) ou de `run_with_real_timers` (sem `fakeClock`), ambos de `common`. O fixture
//!   é uma `async fn` que não escolhe o relógio.
//! - `ITraceLogger` é o `TraceLogger` de `je_base::virtual_scheduling`; `logger.logRun(() => model.trigger())` é
//!   `logger.log_run("() => model.trigger()", || ...)` (o texto-fonte da função vai junto, porque um fecho Rust não tem).
//! - Os objetos do histórico de chamadas do provedor (`{ position, triggerKind, text, changeHint? }`) são `Dynamic`, e a
//!   comparação do caso é `assert_deep_strict_equal` (a ordem das chaves não conta, e `changeHint` ausente é ausente).
//! - `setReturnValue(value, delayMs = 0)` e `setReturnValues(values, delayMs = 0)` viram dois métodos cada, sem e com o
//!   atraso, porque o Rust não tem argumento padrão. `provideInlineCompletions` é um [`async_fn`], como o `async` do original:
//!   o histórico e a lista são montados de forma síncrona e o `await timeout(delayMs)` só vem depois.
//! - As opções do fixture (`TestCodeEditorInstantiationOptions & { provider?, fakeClock?, logTimeTrace? }`) são
//!   [`InlineCompletionsTestOptions`]. `serviceCollection` não existe: os serviços que o original trocava pela coleção
//!   (`IConfigurationService` e `IDataChannelService`, que o primeiro caso usa) são os campos `configuration` e
//!   `data_channel_send`. `logTimeTrace` (o diagrama de raias de diagnóstico) fica de fora: nenhum caso o liga.
//! - `provider instanceof MockInlineCompletionsProvider` é o campo `mock_provider` (o construtor
//!   [`InlineCompletionsTestOptions::mock`] preenche os dois).
//! - Serviços do `withAsyncTestCodeEditor` que o original lia da coleção e o porte entrega por injeção: o
//!   `InlineCompletionsController` do porte recebe os serviços em [`InlineCompletionsControllerServices`], então o fixture os
//!   monta aqui com os dublês do próprio `testCodeEditor.ts` (acessibilidade sem leitor de tela, atalhos vazios, chaves de
//!   contexto sem valores, `TestConfigurationService`, `TestCommandService`...).
//! - `instantiationService.stubInstance(InlineSuggestionsView, { shouldShowHoverAtViewZone: () => false, dispose })` é
//!   `InlineSuggestionsViewSource::Custom` com a [`StubInlineSuggestionsView`].
//! - `MockTextModelService` guarda o modelo do editor (o `IModelService.getModel` do original achava só ele).
//! - O `DataChannelForwardingTelemetryService` (que o `InlineCompletionsSource` criava com o `IInstantiationService`) ainda
//!   não tem porte e entra pelo trait `InlineCompletionsRequestTelemetry`: [`DataChannelForwardingTelemetry`] reproduz o que
//!   ele faz (`isCopilotLikeExtension` e o envio ao canal `editTelemetry` quando o evento marca o encaminhamento).

use std::cell::{Cell, RefCell};
use std::future::Future;
use std::ops::Deref;
use std::rc::{Rc, Weak};

use je_base::async_::async_fn;
use je_base::cancellation::CancellationToken;
use je_base::dynamic::{Dynamic, json_stringify};
use je_base::errors::{Error, throw};
use je_base::event::{Emitter, Event};
use je_base::keybindings::{KeyCodeChord, Keybinding, ResolvedKeybinding};
use je_base::lifecycle::{Disposable, DisposableRef, DisposableStore, IDisposable, plain_disposable};
use je_base::observable::{IObservable, Observable, autorun, const_observable, derived};
use je_base::platform::{OperatingSystem, Platform};
use je_base::promise::Promise;
use je_base::runtime::date_now;
use je_base::uri::URI;
use je_base::virtual_scheduling::{TraceLogBuffer, TraceLogger, create_trace_logger};
use je_editor::code_action::platform::{AccessibilitySignal, IAccessibilitySignalService};
use je_editor::code_editor_widget::{CodeEditorWidget, EditorViewModel, Handler, ICodeEditor};
use je_editor::core_commands::{
    BaseCommandOptions, CursorMoveCommandOptions, core_editing_commands, core_navigation_commands,
};
use je_editor::edits::text_edit::TextEdit;
use je_editor::editor_extensions::{ICodeEditorLookup, ITextModelService, ModelReference};
use je_editor::inline_completions::change_recorder::{ChangeReasonLogger, DocumentEventSetChangeReason};
use je_editor::inline_completions::controller::inline_completions_controller::{
    IInlineCompletionsCodeEditorService, IInlineSuggestionsView, InlineCompletionsController,
    InlineCompletionsControllerServices, InlineSuggestionsViewSource,
};
use je_editor::inline_completions::ghost_text::GhostTextOrReplacement;
use je_editor::inline_completions::inline_completions_model::{
    DefaultAccount, IDefaultAccountService, IInlineCompletionsService, InlineCompletionsModel,
};
use je_editor::inline_completions::inline_completions_source::{
    EmptyResponseTelemetryEvent, FetchLogEntry, InlineCompletionsFetchLogger, InlineCompletionsRenameProcessor,
    InlineCompletionsRequestTelemetry, InlineCompletionsSourceServices,
};
use je_editor::inline_completions::inline_suggestion_item::InlineSuggestionItem;
use je_editor::inline_completions::provide_inline_completions::InlineCompletionContextWithoutUuid;
use je_editor::inline_completions::suggest_widget_adapter::{SuggestItemSelector, SuggestWidgetHost};
use je_editor::language_configuration_registry::ILanguageConfigurationService;
use je_editor::language_feature_debounce::{ILanguageFeatureDebounceService, LanguageFeatureDebounceService};
use je_editor::language_features_service::LanguageFeaturesService;
use je_editor::language_providers::ProviderResult;
use je_editor::language_selector::{LanguageFilter, LanguageSelector};
use je_editor::languages::{
    IInlineCompletionChangeHint, InlineCompletion, InlineCompletionContext, InlineCompletionInsertText, InlineCompletions,
    InlineCompletionsDisposeReason, InlineCompletionsProvider,
};
use je_editor::position::Position;
use je_editor::position_to_offset::{PositionOffsetTransformer, PositionOffsetTransformerBase};
use je_editor::range::Range;
use je_editor::suggest::suggest_widget_contract::ISuggestWidget;
use je_editor::text::abstract_text::StringText;
use je_editor::text_model::TextModel;
use je_platform::accessibility::{AccessibilitySupport, IAccessibilityService};
use je_platform::configuration::service::IConfigurationService;
use je_platform::configuration::test_configuration_service::TestConfigurationService;
use je_platform::contextkey::{IContext, IContextKeyService, IContextKeyServiceTarget};
use je_platform::commands::I_COMMAND_SERVICE;
use je_platform::keybinding::keybinding_resolver::ResolutionResult;
use je_platform::keybinding::mock_keybinding_service::MockContextKeyService;
use je_platform::keybinding::resolved_keybinding_item::ResolvedKeybindingItem;
use je_platform::keybinding::us_layout_resolved_keybinding::USLayoutResolvedKeybinding;
use je_platform::keybinding::{IKeybindingService, KeyboardEvent, KeybindingsSchemaContribution};
use je_platform::log::{NullLogService, null_logger_service};

use super::mock_environment_service::MockEnvironmentService;
use super::test_code_editor::{
    CodeEditorServices, TestCodeEditor, TestCodeEditorInstantiationOptions, run_editor_command, with_async_test_code_editor,
};

/// `ensureNoDisposablesAreLeakedInTestSuite()` depois do registro global dos comandos do editor. No original esse
/// registro acontece quando o módulo é importado, antes de o guarda começar a rastrear; aqui ele é preguiçoso, então o
/// teste o força antes de armar o guarda para que os atalhos dos comandos não contem como vazamento do caso.
pub fn ensure_no_leaks_after_global_registrations() -> crate::common::LeakGuard {
    let _ = core_editing_commands::instance();
    let _ = core_navigation_commands::instance();
    crate::common::ensure_no_disposables_are_leaked_in_test_suite()
}

// #region MockInlineCompletionsProvider

/// `MockInlineCompletionsProvider`.
pub struct MockInlineCompletionsProvider {
    /// `enableForwardStability`.
    pub enable_forward_stability: bool,
    return_value: RefCell<Vec<InlineCompletion>>,
    delay_ms: Cell<u64>,
    call_history: RefCell<Vec<Dynamic>>,
    called_twice_in_50_ms: Cell<bool>,
    on_did_change_emitter: Rc<Emitter<Option<IInlineCompletionChangeHint>>>,
    last_time_ms: Cell<Option<u64>>,
}

impl MockInlineCompletionsProvider {
    /// `new MockInlineCompletionsProvider(enableForwardStability)`.
    pub fn new(enable_forward_stability: bool) -> Rc<Self> {
        Rc::new(Self {
            enable_forward_stability,
            return_value: RefCell::new(Vec::new()),
            delay_ms: Cell::new(0),
            call_history: RefCell::new(Vec::new()),
            called_twice_in_50_ms: Cell::new(false),
            on_did_change_emitter: Emitter::new(),
            last_time_ms: Cell::new(None),
        })
    }

    /// `setReturnValue(value, delayMs = 0)`.
    pub fn set_return_value(&self, value: Option<InlineCompletion>) {
        self.set_return_value_with_delay(value, 0);
    }

    /// `setReturnValue(value, delayMs)`.
    pub fn set_return_value_with_delay(&self, value: Option<InlineCompletion>, delay_ms: u64) {
        self.set_return_values_with_delay(value.into_iter().collect(), delay_ms);
    }

    /// `setReturnValues(values, delayMs = 0)`.
    pub fn set_return_values(&self, values: Vec<InlineCompletion>) {
        self.set_return_values_with_delay(values, 0);
    }

    /// `setReturnValues(values, delayMs)`.
    pub fn set_return_values_with_delay(&self, values: Vec<InlineCompletion>, delay_ms: u64) {
        *self.return_value.borrow_mut() = values;
        self.delay_ms.set(delay_ms);
    }

    /// `getAndClearCallHistory()`.
    pub fn get_and_clear_call_history(&self) -> Vec<Dynamic> {
        std::mem::take(&mut *self.call_history.borrow_mut())
    }

    /// `assertNotCalledTwiceWithin50ms()`.
    pub fn assert_not_called_twice_within_50ms(&self) {
        if self.called_twice_in_50_ms.get() {
            throw(Error::new("provideInlineCompletions has been called at least twice within 50ms. This should not happen."));
        }
    }

    /// `fireOnDidChange(changeHint?)`: dispara o `onDidChange` com a dica de mudança opcional.
    pub fn fire_on_did_change(&self, change_hint: Option<IInlineCompletionChangeHint>) {
        self.on_did_change_emitter.fire(change_hint);
    }
}

/// O `changeHint` do histórico: `{ data }`, ou `{}` sem dados.
fn change_hint_to_dynamic(hint: &IInlineCompletionChangeHint) -> Dynamic {
    match &hint.data {
        Some(data) => Dynamic::object([("data", data.clone())]),
        None => Dynamic::object(Vec::<(&str, Dynamic)>::new()),
    }
}

impl InlineCompletionsProvider for MockInlineCompletionsProvider {
    fn provide_inline_completions(
        &self,
        model: &Rc<TextModel>,
        position: Position,
        context: &InlineCompletionContext,
        _token: &CancellationToken,
    ) -> ProviderResult<Rc<InlineCompletions>> {
        let current_time_ms = date_now();
        // `this.lastTimeMs && currentTimeMs - this.lastTimeMs < 50`: zero é falso no JavaScript.
        if let Some(last_time_ms) = self.last_time_ms.get()
            && last_time_ms != 0
            && current_time_ms.saturating_sub(last_time_ms) < 50
        {
            self.called_twice_in_50_ms.set(true);
        }
        self.last_time_ms.set(Some(current_time_ms));

        let mut entry: Vec<(&str, Dynamic)> = vec![
            ("position", Dynamic::from(position.to_string())),
            ("triggerKind", Dynamic::Number(f64::from(context.trigger_kind as i32))),
            ("text", Dynamic::from(model.get_value().to_string_lossy())),
        ];
        if let Some(change_hint) = &context.change_hint {
            entry.push(("changeHint", change_hint_to_dynamic(change_hint)));
        }
        self.call_history.borrow_mut().push(Dynamic::object(entry));

        let items: Vec<Rc<InlineCompletion>> = self
            .return_value
            .borrow()
            .iter()
            .map(|v| {
                let mut x = v.clone();
                if x.range.is_none() {
                    x.range = Some(model.get_full_model_range());
                }
                Rc::new(x)
            })
            .collect();

        let delay_ms = self.delay_ms.get();
        let enable_forward_stability = self.enable_forward_stability;
        async_fn(async move {
            if delay_ms > 0 {
                crate::common::timeout(delay_ms).await?;
            }
            Ok(Some(Rc::new(InlineCompletions {
                items,
                enable_forward_stability: Some(enable_forward_stability),
                ..InlineCompletions::default()
            })))
        })
    }

    fn dispose_inline_completions(&self, _completions: &Rc<InlineCompletions>, _reason: InlineCompletionsDisposeReason) {}

    fn on_did_change_inline_completions(&self) -> Option<Event<Option<IInlineCompletionChangeHint>>> {
        Some(self.on_did_change_emitter.event())
    }
}

// #endregion

// #region MockSearchReplaceCompletionsProvider

/// `String.prototype.indexOf` sobre unidades UTF-16.
fn index_of(haystack: &[u16], needle: &[u16]) -> Option<usize> {
    if needle.is_empty() {
        return Some(0);
    }
    haystack.windows(needle.len()).position(|window| window == needle)
}

/// `MockSearchReplaceCompletionsProvider`.
pub struct MockSearchReplaceCompletionsProvider {
    /// O `Map<string, string>`: a ordem de inserção, e `set` de uma chave existente mantém a posição dela.
    map: RefCell<Vec<(String, String)>>,
}

impl MockSearchReplaceCompletionsProvider {
    pub fn new() -> Rc<Self> {
        Rc::new(Self { map: RefCell::new(Vec::new()) })
    }

    /// `add(search, replace)`.
    pub fn add(&self, search: &str, replace: &str) {
        let mut map = self.map.borrow_mut();
        match map.iter_mut().find(|(key, _)| key == search) {
            Some(entry) => entry.1 = replace.to_owned(),
            None => map.push((search.to_owned(), replace.to_owned())),
        }
    }
}

impl InlineCompletionsProvider for MockSearchReplaceCompletionsProvider {
    fn provide_inline_completions(
        &self,
        model: &Rc<TextModel>,
        _position: Position,
        _context: &InlineCompletionContext,
        _token: &CancellationToken,
    ) -> ProviderResult<Rc<InlineCompletions>> {
        let text = model.get_value();
        for (search, replace) in self.map.borrow().iter() {
            let search_units: Vec<u16> = search.encode_utf16().collect();
            let idx = index_of(text.units(), &search_units);
            // replace idx...idx+text.length with replace
            if let Some(idx) = idx {
                let range = Range::from_positions(model.get_position_at(idx as i64), model.get_position_at((idx + search_units.len()) as i64));
                let item = InlineCompletion {
                    range: Some(range),
                    insert_text: Some(InlineCompletionInsertText::Text(replace.clone())),
                    is_inline_edit: Some(true),
                    ..InlineCompletion::default()
                };
                let result = Rc::new(InlineCompletions { items: vec![Rc::new(item)], ..InlineCompletions::default() });
                return async_fn(async move { Ok(Some(result)) });
            }
        }
        async_fn(async move { Ok(Some(Rc::new(InlineCompletions::default()))) })
    }

    fn dispose_inline_completions(&self, _completions: &Rc<InlineCompletions>, _reason: InlineCompletionsDisposeReason) {}
}

// #endregion

// #region InlineEditContext

/// `InlineEditContext`.
pub struct InlineEditContext {
    base: Rc<Disposable>,
    /// `prettyViewStates`.
    pub pretty_view_states: Rc<RefCell<Vec<Option<String>>>>,
    logger: Option<Rc<TraceLogger>>,
}

impl InlineEditContext {
    pub fn new(model: &Rc<InlineCompletionsModel>, editor: &Rc<TestCodeEditor>, logger: Option<Rc<TraceLogger>>) -> Rc<Self> {
        let base = Disposable::new();
        let pretty_view_states: Rc<RefCell<Vec<Option<String>>>> = Rc::default();

        let edit: IObservable<Option<Rc<TextEdit>>> = {
            let model = Rc::clone(model);
            derived(move |reader| {
                let state = model.state.read(reader);
                state.map(|state| Rc::new(TextEdit::new(state.edits().to_vec())))
            })
        };

        let (editor, states) = (Rc::clone(editor), Rc::clone(&pretty_view_states));
        base.register(autorun(move |reader| {
            /* @description update */
            let e = edit.read(reader);
            let view = e.map(|e| e.describe_on(&StringText::new(editor.get_value(None))).to_string_lossy());
            states.borrow_mut().push(view);
        }));

        Rc::new(Self { base, pretty_view_states, logger })
    }

    /// `getAndClearViewStates()`.
    pub fn get_and_clear_view_states(&self) -> Vec<Option<String>> {
        let arr = std::mem::take(&mut *self.pretty_view_states.borrow_mut());
        if let Some(logger) = &self.logger {
            let json = Dynamic::array(arr.iter().map(|s| s.as_deref().map_or(Dynamic::Undefined, |s| Dynamic::from(s))));
            logger.log(&format!("getAndClearViewStates() => {}", json_stringify(&json).unwrap_or_default()));
        }
        arr
    }
}

impl IDisposable for InlineEditContext {
    fn dispose(&self) {
        self.base.dispose();
    }
}

// #endregion

// #region GhostTextContext

/// `GhostTextContext`.
pub struct GhostTextContext {
    base: Rc<Disposable>,
    /// `prettyViewStates`.
    pub pretty_view_states: Rc<RefCell<Vec<String>>>,
    current_pretty_view_state: Rc<RefCell<Option<String>>>,
    editor: Rc<TestCodeEditor>,
    view_model: Rc<EditorViewModel>,
    logger: Option<Rc<TraceLogger>>,
}

impl GhostTextContext {
    pub fn new(model: &Rc<InlineCompletionsModel>, editor: &Rc<TestCodeEditor>, logger: Option<Rc<TraceLogger>>) -> Rc<Self> {
        let base = Disposable::new();
        let pretty_view_states: Rc<RefCell<Vec<String>>> = Rc::default();
        let current_pretty_view_state: Rc<RefCell<Option<String>>> = Rc::default();

        let primary_ghost_text = Rc::clone(&model.primary_ghost_text);
        let (editor_for_run, states, current) = (Rc::clone(editor), Rc::clone(&pretty_view_states), Rc::clone(&current_pretty_view_state));
        base.register(autorun(move |reader| {
            /* @description update */
            let ghost_text = primary_ghost_text.read(reader);
            let value = editor_for_run.get_value(None).to_string_lossy();
            let view = match ghost_text.as_deref() {
                Some(GhostTextOrReplacement::GhostText(ghost_text)) => ghost_text.render(&value, true),
                Some(GhostTextOrReplacement::Replacement(replacement)) => replacement.render(&value, true),
                None => value,
            };

            let changed = current.borrow().as_deref() != Some(view.as_str());
            if changed {
                states.borrow_mut().push(view.clone());
            }
            *current.borrow_mut() = Some(view);
        }));

        let view_model = editor.get_view_model().expect("o editor de teste tem o modelo ligado");
        Rc::new(Self { base, pretty_view_states, current_pretty_view_state, editor: Rc::clone(editor), view_model, logger })
    }

    /// `currentPrettyViewState`.
    pub fn current_pretty_view_state(&self) -> Option<String> {
        self.current_pretty_view_state.borrow().clone()
    }

    /// `getAndClearViewStates()`.
    pub fn get_and_clear_view_states(&self) -> Vec<String> {
        let arr = std::mem::take(&mut *self.pretty_view_states.borrow_mut());
        if let Some(logger) = &self.logger {
            let json = Dynamic::array(arr.iter().map(|s| Dynamic::from(s.as_str())));
            logger.log(&format!("getAndClearViewStates() => {}", json_stringify(&json).unwrap_or_default()));
        }
        arr
    }

    /// `keyboardType(text)`.
    pub fn keyboard_type(&self, text: &str) {
        if let Some(logger) = &self.logger {
            logger.log(&format!("keyboardType({})", json_stringify(&Dynamic::from(text)).unwrap_or_default()));
        }
        self.editor.trigger(Some("keyboard"), Handler::TYPE, Some(Dynamic::object([("text", Dynamic::from(text))])));
    }

    /// `cursorUp()`.
    pub fn cursor_up(&self) {
        core_navigation_commands::instance().cursor_up.run_core_editor_command(&self.view_model, &CursorMoveCommandOptions::default());
    }

    /// `cursorRight()`.
    pub fn cursor_right(&self) {
        core_navigation_commands::instance().cursor_right.run_core_editor_command(&self.view_model, &CursorMoveCommandOptions::default());
    }

    /// `cursorLeft()`.
    pub fn cursor_left(&self) {
        core_navigation_commands::instance().cursor_left.run_core_editor_command(&self.view_model, &CursorMoveCommandOptions::default());
    }

    /// `cursorDown()`.
    pub fn cursor_down(&self) {
        core_navigation_commands::instance().cursor_down.run_core_editor_command(&self.view_model, &CursorMoveCommandOptions::default());
    }

    /// `cursorLineEnd()`.
    pub fn cursor_line_end(&self) {
        core_navigation_commands::instance().cursor_line_end.run_core_editor_command(&self.view_model, &BaseCommandOptions::default());
    }

    /// `leftDelete()`.
    pub fn left_delete(&self) {
        run_editor_command(&self.editor, &*core_editing_commands::instance().delete_left);
    }
}

impl IDisposable for GhostTextContext {
    fn dispose(&self) {
        self.base.dispose();
    }
}

// #endregion

// #region withAsyncTestCodeEditorAndInlineCompletionsModel

/// As opções de `withAsyncTestCodeEditorAndInlineCompletionsModel` (ver o topo do módulo).
pub struct InlineCompletionsTestOptions {
    /// As opções do editor (`IEditorOptions`, por exemplo `inlineSuggest`).
    pub editor_options: Dynamic,
    /// `provider`.
    pub provider: Option<Rc<dyn InlineCompletionsProvider>>,
    /// O `provider` quando é um `MockInlineCompletionsProvider`: o fixture confere `assertNotCalledTwiceWithin50ms` no fim.
    pub mock_provider: Option<Rc<MockInlineCompletionsProvider>>,
    /// O conteúdo do `TestConfigurationService` (o `IConfigurationService` da coleção).
    pub configuration: Option<Dynamic>,
    /// `getDataChannel(channelId).sendData(...)` do `IDataChannelService` da coleção: recebe o id do canal.
    pub data_channel_send: Option<Rc<dyn Fn(&str)>>,
}

impl Default for InlineCompletionsTestOptions {
    /// O `{}` do original.
    fn default() -> Self {
        Self {
            editor_options: Dynamic::object(Vec::<(&str, Dynamic)>::new()),
            provider: None,
            mock_provider: None,
            configuration: None,
            data_channel_send: None,
        }
    }
}

impl InlineCompletionsTestOptions {
    /// `{ provider }` com um `MockInlineCompletionsProvider`.
    pub fn mock(provider: &Rc<MockInlineCompletionsProvider>) -> Self {
        let as_provider: Rc<dyn InlineCompletionsProvider> = provider.clone();
        Self { provider: Some(as_provider), mock_provider: Some(Rc::clone(provider)), ..Self::default() }
    }

    /// `{ provider }` com um provedor qualquer.
    pub fn with_provider(provider: Rc<dyn InlineCompletionsProvider>) -> Self {
        Self { provider: Some(provider), ..Self::default() }
    }

    /// `{ inlineSuggest: { enabled } }`.
    pub fn with_inline_suggest_enabled(mut self, enabled: bool) -> Self {
        self.editor_options = Dynamic::object([("inlineSuggest", Dynamic::object([("enabled", Dynamic::Bool(enabled))]))]);
        self
    }
}

/// `IWithAsyncTestCodeEditorAndInlineCompletionsModel`.
pub struct InlineCompletionsTestContext {
    pub editor: Rc<TestCodeEditor>,
    pub editor_view_model: Rc<EditorViewModel>,
    pub model: Rc<InlineCompletionsModel>,
    pub context: Rc<GhostTextContext>,
    pub store: Rc<DisposableStore>,
    pub logger: Rc<TraceLogger>,
    pub instantiation_service: Rc<CodeEditorServices>,
    /// Os serviços da fonte de sugestões que o modelo recebeu (o primeiro caso cria uma `InlineCompletionsSource` com eles,
    /// como o `instantiationService.createInstance(InlineCompletionsSource, ...)` do original).
    pub source_services: InlineCompletionsSourceServices,
}

/// `withAsyncTestCodeEditorAndInlineCompletionsModel(text, options, callback)`.
pub async fn with_async_test_code_editor_and_inline_completions_model<F, Fut>(
    text: &str,
    options: InlineCompletionsTestOptions,
    callback: F,
) where
    F: FnOnce(InlineCompletionsTestContext) -> Fut + 'static,
    Fut: Future<Output = ()> + 'static,
{
    let InlineCompletionsTestOptions { editor_options, provider, mock_provider, configuration, data_channel_send } = options;

    let logs: TraceLogBuffer = Rc::default();
    let logger = Rc::new(create_trace_logger(logs));
    let disposable_store = DisposableStore::new();

    let language_features_service = Rc::new(LanguageFeaturesService::new());
    if let Some(provider) = provider {
        let selector = LanguageSelector::from(LanguageFilter { pattern: Some("**".into()), ..LanguageFilter::default() });
        let registration = language_features_service.inline_completions_provider.register(selector, provider);
        disposable_store.add(registration);
    }

    let store_for_context = Rc::clone(&disposable_store);
    let logger_for_context = Rc::clone(&logger);
    let finished = with_async_test_code_editor(
        text,
        TestCodeEditorInstantiationOptions::new(editor_options),
        move |editor, editor_view_model, instantiation_service| {
            async_fn(async move {
                let language_configuration_service: Rc<dyn ILanguageConfigurationService> =
                    instantiation_service.language_configuration_service.clone();
                let configuration_service: Rc<dyn IConfigurationService> = TestConfigurationService::new(configuration);
                let context_key_service: Rc<dyn IContextKeyService> = MockContextKeyService::with_context(Rc::new(EmptyContext));
                let noop_send: Rc<dyn Fn(&str)> = Rc::new(|_: &str| {});
                let text_model = editor.get_model().expect("o editor de teste tem modelo");

                let source_services = InlineCompletionsSourceServices {
                    language_configuration_service,
                    log_service: NullLogService::shared(),
                    configuration_service,
                    context_key_service: Rc::clone(&context_key_service),
                    text_model_service: Rc::new(MockTextModelService { models: vec![text_model] }),
                    rename_processor: Rc::new(NoRenameProcessor),
                    fetch_logger: Rc::new(DisabledFetchLogger),
                    telemetry: Rc::new(DataChannelForwardingTelemetry { send_data: data_channel_send.unwrap_or(noop_send) }),
                };

                let logger_service = null_logger_service();
                let environment_service = MockEnvironmentService::new(true, false);
                let debounce_service: Rc<dyn ILanguageFeatureDebounceService> =
                    Rc::new(LanguageFeatureDebounceService::new(NullLogService::shared(), &*environment_service));

                let controller = InlineCompletionsController::new(
                    editor.widget(),
                    InlineCompletionsControllerServices {
                        command_service: instantiation_service.get(I_COMMAND_SERVICE),
                        accessibility_service: Rc::new(TestAccessibilityService),
                        keybinding_service: Rc::new(MockKeybindingService),
                        context_key_service,
                        view: InlineSuggestionsViewSource::Custom(Rc::new(|| -> Rc<dyn IInlineSuggestionsView> {
                            Rc::new(StubInlineSuggestionsView)
                        })),
                        source_services: source_services.clone(),
                        debounce_service,
                        language_features_service,
                        accessibility_signal_service: Rc::new(SilentAccessibilitySignalService),
                        inline_completions_service: Rc::new(NeverSnoozingInlineCompletionsService),
                        default_account_service: Rc::new(NoDefaultAccountService),
                        completions_enablement_setting: None,
                        suggest_widget_host: Rc::new(NoSuggestWidgetHost),
                        code_editor_service: Rc::new(TestInlineCompletionsCodeEditorService),
                        code_editor_lookup: Rc::new(TestCodeEditorLookup { editor: Rc::downgrade(&editor) }),
                        logger_service: logger_service.clone(),
                        create_change_reason_logger: Rc::new(|| -> Rc<dyn ChangeReasonLogger> { Rc::new(DisabledChangeReasonLogger) }),
                    },
                );
                let model = controller.model.get().expect("o controller cria o modelo");
                let context = GhostTextContext::new(&model, &editor, Some(Rc::clone(&logger_for_context)));

                callback(InlineCompletionsTestContext {
                    editor: Rc::clone(&editor),
                    editor_view_model,
                    model: Rc::clone(&model),
                    context: Rc::clone(&context),
                    store: store_for_context,
                    logger: logger_for_context,
                    instantiation_service,
                    source_services,
                })
                .await;

                context.dispose();
                model.dispose();
                controller.dispose();
                logger_service.dispose();
                Ok(())
            })
        },
    );
    finished.await.expect("o editor de teste terminou sem erro");

    if let Some(mock_provider) = mock_provider {
        mock_provider.assert_not_called_twice_within_50ms();
    }
    disposable_store.dispose();
}

// #endregion

// #region AnnotatedString e AnnotatedText

/// `{ mark: string; idx: number }`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AnnotatedMarker {
    pub mark: String,
    pub idx: usize,
}

/// `AnnotatedString`.
pub struct AnnotatedString {
    pub value: String,
    pub markers: Vec<AnnotatedMarker>,
}

impl AnnotatedString {
    /// `new AnnotatedString(src)`: a anotação padrão é `↓`.
    pub fn new(src: &str) -> Self {
        Self::with_annotations(src, &["↓"])
    }

    /// `new AnnotatedString(src, annotations)`.
    pub fn with_annotations(src: &str, annotations: &[&str]) -> Self {
        let (markers, text_without_markers) = find_markers(src, annotations);
        Self { value: text_without_markers, markers }
    }

    /// `getMarkerOffset(markerIdx = 0)`.
    pub fn get_marker_offset(&self, marker_idx: usize) -> usize {
        if marker_idx >= self.markers.len() {
            throw(Error::bug_indicating(Some(&format!("Marker index {marker_idx} out of bounds"))));
        }
        self.markers[marker_idx].idx
    }
}

/// `findMarkers(text, markers)`: os deslocamentos são em unidades UTF-16, como os índices de `string` do JavaScript.
fn find_markers(text: &str, markers: &[&str]) -> (Vec<AnnotatedMarker>, String) {
    let text_units: Vec<u16> = text.encode_utf16().collect();
    let mut sorted: Vec<Vec<u16>> = markers.iter().map(|marker| marker.encode_utf16().collect()).collect();
    // `markers.sort((a, b) => b.length - a.length)`: a ordenação dos dois é estável.
    sorted.sort_by(|a, b| b.len().cmp(&a.len()));

    let mut results: Vec<AnnotatedMarker> = Vec::new();
    let mut text_without_markers: Vec<u16> = Vec::new();

    let mut pos = 0usize;
    let mut i = 0usize;
    while i < text_units.len() {
        match sorted.iter().find(|marker| text_units[i..].starts_with(marker)) {
            Some(marker) => {
                results.push(AnnotatedMarker { mark: String::from_utf16_lossy(marker), idx: pos });
                i += marker.len();
            }
            None => {
                text_without_markers.push(text_units[i]);
                pos += 1;
                i += 1;
            }
        }
    }

    (results, String::from_utf16_lossy(&text_without_markers))
}

/// `AnnotatedText extends AnnotatedString`: a herança é o `Deref` para o [`AnnotatedString`].
pub struct AnnotatedText {
    base: AnnotatedString,
    transformer: PositionOffsetTransformer,
}

impl Deref for AnnotatedText {
    type Target = AnnotatedString;

    fn deref(&self) -> &AnnotatedString {
        &self.base
    }
}

impl AnnotatedText {
    /// `new AnnotatedText(src)`.
    pub fn new(src: &str) -> Self {
        Self::from_string(AnnotatedString::new(src))
    }

    /// `new AnnotatedText(src, annotations)`.
    pub fn with_annotations(src: &str, annotations: &[&str]) -> Self {
        Self::from_string(AnnotatedString::with_annotations(src, annotations))
    }

    fn from_string(base: AnnotatedString) -> Self {
        let transformer = PositionOffsetTransformer::new(&base.value);
        Self { base, transformer }
    }

    /// `getMarkerPosition(markerIdx = 0)`.
    pub fn get_marker_position(&self, marker_idx: usize) -> Position {
        self.transformer.get_position(self.get_marker_offset(marker_idx) as i32)
    }
}

// #endregion

// #region Dublês dos serviços (o que o testCodeEditor.ts e o utils.ts davam pela coleção de serviços)

/// `MockTextModelService`: resolve só os modelos que conhece.
struct MockTextModelService {
    models: Vec<Rc<TextModel>>,
}

impl ITextModelService for MockTextModelService {
    fn create_model_reference(&self, resource: &URI) -> Promise<ModelReference> {
        match self.models.iter().find(|model| model.uri() == *resource) {
            Some(model) => Promise::resolved(ModelReference::new(Rc::clone(model), plain_disposable(|| {}))),
            None => Promise::rejected(Error::new(format!(
                "MockTextModelService: Model not found for {}",
                resource.to_string(false).to_string_lossy()
            ))),
        }
    }
}

/// O `IContext` do `getContext(domNode)`: sem nenhum valor (o `MockContextKeyService` do original devolve `null`, e quem o
/// lê usa `?.`).
struct EmptyContext;

impl IContext for EmptyContext {
    fn get_value(&self, _key: &str) -> Dynamic {
        Dynamic::Undefined
    }
}

/// `TestAccessibilityService`: sem leitor de tela, e nenhum evento dispara.
struct TestAccessibilityService;

impl IAccessibilityService for TestAccessibilityService {
    fn on_did_change_screen_reader_optimized(&self) -> Event<()> {
        Event::none()
    }
    fn on_did_change_reduced_motion(&self) -> Event<()> {
        Event::none()
    }
    fn on_did_change_reduced_transparency(&self) -> Event<()> {
        Event::none()
    }
    fn always_underline_access_keys(&self) -> Promise<bool> {
        Promise::resolved(false)
    }
    fn is_screen_reader_optimized(&self) -> bool {
        false
    }
    fn is_motion_reduced(&self) -> bool {
        false
    }
    fn is_transparency_reduced(&self) -> bool {
        false
    }
    fn get_accessibility_support(&self) -> AccessibilitySupport {
        AccessibilitySupport::Unknown
    }
    fn set_accessibility_support(&self, _accessibility_support: AccessibilitySupport) {}
    fn alert(&self, _message: &str) {}
    fn status(&self, _message: &str) {}
}

/// O `IAccessibilitySignalService` do `utils.ts`: `{ playSignal: async () => { }, isSoundEnabled: () => false }`.
struct SilentAccessibilitySignalService;

impl IAccessibilitySignalService for SilentAccessibilitySignalService {
    fn play_signal(&self, _signal: AccessibilitySignal) {}
}

/// `MockKeybindingService` de `mockKeybindingService.ts`: não tem atalho nenhum e resolve pelo layout US.
struct MockKeybindingService;

fn current_os() -> OperatingSystem {
    Platform::detect().os
}

impl IKeybindingService for MockKeybindingService {
    fn in_chord_mode(&self) -> bool {
        false
    }
    fn on_did_update_keybindings(&self) -> Event<()> {
        Event::none()
    }
    fn resolve_keybinding(&self, keybinding: &Keybinding) -> Vec<Rc<dyn ResolvedKeybinding>> {
        USLayoutResolvedKeybinding::resolve_keybinding(keybinding, current_os())
    }
    fn resolve_keyboard_event(&self, e: &KeyboardEvent) -> Rc<dyn ResolvedKeybinding> {
        let chord = KeyCodeChord::new(e.ctrl_key, e.shift_key, e.alt_key, e.meta_key, e.key_code);
        self.resolve_keybinding(&chord.to_keybinding()).remove(0)
    }
    fn resolve_user_binding(&self, _user_binding: &str) -> Vec<Rc<dyn ResolvedKeybinding>> {
        Vec::new()
    }
    fn dispatch_event(&self, _e: &KeyboardEvent, _target: Option<Rc<dyn IContextKeyServiceTarget>>) -> bool {
        false
    }
    fn soft_dispatch(&self, _keyboard_event: &KeyboardEvent, _target: Option<Rc<dyn IContextKeyServiceTarget>>) -> ResolutionResult {
        ResolutionResult::NoMatchingKb
    }
    fn enable_keybinding_hold_mode(&self, _command_id: &str) -> Option<Promise<()>> {
        None
    }
    fn dispatch_by_user_settings_label(&self, _user_settings_label: &str, _target: Option<Rc<dyn IContextKeyServiceTarget>>) {}
    fn lookup_keybindings(&self, _command_id: &str) -> Vec<Rc<dyn ResolvedKeybinding>> {
        Vec::new()
    }
    fn lookup_keybinding(
        &self,
        _command_id: &str,
        _context: Option<&dyn IContextKeyService>,
        _enforce_context_check: bool,
    ) -> Option<Rc<dyn ResolvedKeybinding>> {
        None
    }
    fn get_default_keybindings_content(&self) -> String {
        String::new()
    }
    fn get_default_keybindings(&self) -> Vec<Rc<ResolvedKeybindingItem>> {
        Vec::new()
    }
    fn get_keybindings(&self) -> Vec<Rc<ResolvedKeybindingItem>> {
        Vec::new()
    }
    fn custom_keybindings_count(&self) -> usize {
        0
    }
    fn might_produce_printable_character(&self, _event: &KeyboardEvent) -> bool {
        false
    }
    fn register_schema_contribution(&self, _contribution: Rc<dyn KeybindingsSchemaContribution>) -> DisposableRef {
        plain_disposable(|| {})
    }
    fn toggle_logging(&self) -> bool {
        false
    }
    fn append_keybinding(
        &self,
        label: &str,
        _command_id: Option<&str>,
        _context: Option<&dyn IContextKeyService>,
        _enforce_context_check: bool,
    ) -> String {
        label.to_owned()
    }
    fn dump_debug_info(&self) -> String {
        String::new()
    }
    fn dump_debug_info_json(&self) -> String {
        String::new()
    }
}

/// O `IInlineCompletionsService` dos testes: nunca em modo soneca.
struct NeverSnoozingInlineCompletionsService;

impl IInlineCompletionsService for NeverSnoozingInlineCompletionsService {
    fn on_did_change_is_snoozing(&self) -> Event<bool> {
        Event::none()
    }
    fn is_snoozing(&self) -> bool {
        false
    }
    fn report_new_completion(&self, _request_uuid: &str) {}
}

/// O `IDefaultAccountService` do `utils.ts`: sem conta (`getDefaultAccount: async () => null`).
struct NoDefaultAccountService;

impl IDefaultAccountService for NoDefaultAccountService {
    fn get_default_account(&self) -> Promise<Option<DefaultAccount>> {
        Promise::resolved(None)
    }
    fn on_did_change_default_account(&self) -> Event<Option<DefaultAccount>> {
        Event::none()
    }
}

/// O editor de teste não tem o `SuggestController`: o `SuggestWidgetAdaptor` só pede o seletor e o widget quando o
/// controller existe, então aqui só o descarte e os dois `...ForceRenderingAbove` (que o controller chama sem checar) rodam.
struct NoSuggestWidgetHost;

impl SuggestWidgetHost for NoSuggestWidgetHost {
    fn register_selector(&self, _selector: SuggestItemSelector) -> DisposableRef {
        plain_disposable(|| {})
    }
    fn widget(&self) -> Rc<dyn ISuggestWidget> {
        panic!("o editor de teste não tem o SuggestController, então o adaptador nunca pede o widget")
    }
    fn force_rendering_above(&self) {}
    fn stop_force_rendering_above(&self) {}
}

/// O `ICodeEditorService` do controller: nenhum editor de diff, nenhum editor aberto, nenhuma animação.
struct TestInlineCompletionsCodeEditorService;

impl IInlineCompletionsCodeEditorService for TestInlineCompletionsCodeEditorService {
    fn is_in_diff_editor(&self, _editor_id: &str) -> bool {
        false
    }
    fn open_code_editor(&self, _resource: URI, _source_editor: &Rc<CodeEditorWidget>) -> Promise<Option<Rc<CodeEditorWidget>>> {
        Promise::resolved(None)
    }
    fn start_edits_fadeout(&self, _editor: &Rc<CodeEditorWidget>, _ranges: Vec<Range>, _on_finished: Rc<dyn Fn()>) -> DisposableRef {
        plain_disposable(|| {})
    }
}

/// `getFocusedCodeEditor()` e `getActiveCodeEditor()` do `TestCodeEditorService`: o editor focado é o que tem o foco de
/// texto (o `TestCodeEditor.hasTextFocus()` vale `true` por padrão), e o editor ativo é `null`.
struct TestCodeEditorLookup {
    editor: Weak<TestCodeEditor>,
}

impl ICodeEditorLookup for TestCodeEditorLookup {
    fn get_focused_code_editor(&self) -> Option<Rc<CodeEditorWidget>> {
        self.editor.upgrade().filter(|editor| editor.has_text_focus()).map(|editor| Rc::clone(editor.widget()))
    }

    fn get_active_code_editor(&self) -> Option<Rc<CodeEditorWidget>> {
        None
    }
}

/// O `InlineSuggestionsView` trocado pelo `stubInstance`: `{ shouldShowHoverAtViewZone: () => false, dispose: () => { } }`.
struct StubInlineSuggestionsView;

impl IDisposable for StubInlineSuggestionsView {
    fn dispose(&self) {}
}

impl IInlineSuggestionsView for StubInlineSuggestionsView {
    fn should_show_hover_at_view_zone(&self, _view_zone_id: &str) -> bool {
        false
    }
}

/// O `StructuredLogger` do `TextModelChangeRecorder` sem o comando de log configurado: desligado.
struct DisabledChangeReasonLogger;

impl IDisposable for DisabledChangeReasonLogger {
    fn dispose(&self) {}
}

impl ChangeReasonLogger for DisabledChangeReasonLogger {
    fn is_enabled(&self) -> IObservable<bool> {
        const_observable(false)
    }
    fn log(&self, _data: &DocumentEventSetChangeReason) -> bool {
        false
    }
}

/// O `StructuredLogger` do `logFetch` sem o comando configurado: desligado.
struct DisabledFetchLogger;

impl IDisposable for DisabledFetchLogger {
    fn dispose(&self) {}
}

impl InlineCompletionsFetchLogger for DisabledFetchLogger {
    fn is_enabled(&self) -> IObservable<bool> {
        const_observable(false)
    }
    fn log(&self, _entry: &FetchLogEntry) {}
}

/// O `RenameSymbolProcessor` para sugestões que não pedem a refatoração de renomear (nenhum caso pede): devolve a sugestão.
struct NoRenameProcessor;

impl IDisposable for NoRenameProcessor {
    fn dispose(&self) {}
}

impl InlineCompletionsRenameProcessor for NoRenameProcessor {
    fn propose_rename_refactoring(
        &self,
        _text_model: &Rc<TextModel>,
        suggestion: InlineSuggestionItem,
        _context: &InlineCompletionContextWithoutUuid,
    ) -> Promise<InlineSuggestionItem> {
        Promise::resolved(suggestion)
    }
}

/// O `DataChannelForwardingTelemetryService`: `isCopilotLikeExtension` e, quando o evento marca o encaminhamento
/// (`forwardToChannelIf`), o envio ao canal `editTelemetry` do `IDataChannelService`.
struct DataChannelForwardingTelemetry {
    send_data: Rc<dyn Fn(&str)>,
}

impl InlineCompletionsRequestTelemetry for DataChannelForwardingTelemetry {
    fn is_copilot_like_extension(&self, extension_id: Option<&str>) -> bool {
        let Some(extension_id) = extension_id.filter(|id| !id.is_empty()) else {
            return false;
        };
        let lower_case = extension_id.to_lowercase();
        lower_case == "github.copilot" || lower_case == "github.copilot-chat"
    }

    fn send_empty_response(&self, event: EmptyResponseTelemetryEvent) {
        if event.forward_to_channel {
            (self.send_data)("editTelemetry");
        }
    }
}

// #endregion
