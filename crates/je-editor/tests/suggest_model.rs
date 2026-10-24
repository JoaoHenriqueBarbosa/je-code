//! Porte de `src/vs/editor/contrib/suggest/test/browser/suggestModel.test.ts` (suítes `SuggestModel - Context` e
//! `SuggestModel - TriggerAndCancelOracle`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()`, o `setup` e o `teardown` da suíte `TriggerAndCancelOracle` são o
//!   [`run_test`]: ele cria os dois registros (o `languageFeaturesService` do original é criado na carga da suíte, fora
//!   do rastreio de vazamentos, então aqui os registros nascem antes do `LeakGuard`), o `LeakGuard`, o [`Fixture`]
//!   (o modelo `abc def` com a URI `test:somefile.ttt` e o `DisposableStore` `disposables`) e roda o corpo com o relógio
//!   virtual (`runWithFakedTimers`), porque todo caso espera eventos do modelo, que chegam por promessas e temporizadores.
//!   Os casos que o original declara `async` ou devolvem `Promise` são o corpo `async` desse `run_test`.
//! - `createMockEditor(model, languageFeaturesService)` é [`create_mock_editor`]: o editor de teste, o
//!   `SnippetController2` registrado pelo `registerAndInstantiateContribution`, o foco de widget ligado
//!   (`editor.hasWidgetFocus = () => true`) e o `IContextKeyService` descartado junto do editor (o `InMemoryStorageService`
//!   do original não tem papel no porte, que não injeta armazenamento no editor). O `ISuggestMemoryService` falso
//!   (`memorize() {}` e `select() { return -1 }`) é o [`NoMemoryService`]: o `select` do porte devolve `usize`, então não
//!   há o `-1`, e o `SuggestController` do porte só lê o `select` no ouvinte de `onDidSuggest` que depende do widget
//!   (ver `suggest_controller.rs`), que não roda aqui.
//! - `withOracle(callback)` é o [`Fixture::with_oracle`], que devolve o [`Oracle`] (o `SuggestModel` e o editor), ambos
//!   descartados com o `disposables`, nessa ordem. `invokeWithinContext(... createInstance(SuggestModel, editor))` é o
//!   `SuggestModel::new` com os [`SuggestModelServices`] que o contêiner de serviços do original entregaria: o
//!   `IEditorWorkerService` falso ([`MockEditorWorkerService`], que a distância de palavras não chama porque
//!   `suggest.localityBonus` é falso), o `IClipboardService` de teste, o `NullTelemetryService`, o `NullLogService`, o
//!   `IEnvironmentService` com `isBuilt` e `isExtensionDevelopment` ([`MockEnvironmentService`]) e os dois registros.
//! - `assertEvent(event, action, assert)` é [`assert_event`]: a promessa é resolvida com o que o `assert` devolve (uma
//!   promessa, como o `assertEvent` aninhado), a asserção roda dentro do ouvinte e uma falha rejeita a promessa. Os
//!   `await assertEvent(...)` sequenciais são `.await.expect(...)`.
//! - Os provedores literais são [`TestProvider`], com o fecho que o `provideCompletionItems` do original seria;
//!   `triggerCharacters` e `resolveCompletionItem` são os métodos `with_trigger_characters` e `with_resolve`.
//!   `first.provider === alwaysSomethingSupport` é a comparação da identidade ([`provider_id`]).
//! - `languageFeaturesService.inlineCompletionsProvider` é um registro de [`InlineCompletionsProvider`], o recorte do
//!   `InlineCompletionsProvider` com o que os casos criam (`provideInlineCompletions: () => ({ items: [] })` e
//!   `disposeInlineCompletions`): o porte de inline completions ainda não existe, e o [`InlineCompletionsHost`] é o
//!   `IInlineCompletionsHost` que o `SuggestModel` lê (`inlineCompletionsProvider.has(model)`), sem controlador de
//!   inline completions no editor ("Without an InlineCompletionsController, the fallback triggers immediately").
//! - `OuterMode` e `InnerMode` são as funções `register_outer_mode` e `register_inner_mode`; o `tokenizeEncoded` do
//!   `OuterMode` monta os tokens com o `Uint32Array` do original como um `Vec<u32>`.
//! - O travessão que as mensagens de asserção do original têm (`'a|<x — should trigger at end of word'`) é um hífen aqui.
//! - `runCommand(CoreEditingCommands.DeleteLeft, null)` é o comando `delete_left` do `core_editing_commands`, chamado
//!   como no caso `tab` do `SnippetController2` (o `NoExtensionsRegistry` do editor de teste não resolve o id).
//! - `setSnippetSuggestSupport`/`getSnippetSuggestSupport` são os de `suggest.rs`; a comparação `=== snippetProvider` é a
//!   da identidade.
//!
//! Fora do porte: a suíte `SuggestModel - offWhenInlineCompletions with InlineCompletionsController` (5 casos) usa o
//! `InlineCompletionsController`, o `InlineCompletionsModel` observável, o `InlineSuggestionsView` e o
//! `withAsyncTestCodeEditor` com esses serviços, que o porte ainda não tem; ela entra com o porte de
//! `src/vs/editor/contrib/inlineCompletions`.

#![allow(dead_code)]

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod snippet_doubles;
mod support;

use std::cell::{Cell, RefCell};
use std::future::Future;
use std::rc::Rc;

use common::{ensure_no_disposables_are_leaked_in_test_suite, run_with_faked_timers};
use je_base::JsString;
use je_base::dynamic::Dynamic;
use je_base::errors::{self, Error};
use je_base::event::Event;
use je_base::lifecycle::{DisposableRef, DisposableStore, IDisposable, to_disposable};
use je_base::promise::Promise;
use je_base::runtime;
use je_base::uri::URI;
use je_editor::code_editor_widget::{CodeEditorWidget, ExecuteEditsSource, Handler, ICodeEditor, IEditorCommand, SelectionOrRange, TypePayload};
use je_editor::core_commands::core_editing_commands;
use je_editor::edit_operation::{EditOperation, SingleEditOperation};
use je_editor::encoded_token_attributes::metadata_consts;
use je_editor::language_configuration::LanguageConfiguration;
use je_editor::language_configuration_registry::ILanguageConfigurationService as _;
use je_editor::language_feature_registry::LanguageFeatureRegistry;
use je_editor::language_providers::ProviderResult;
use je_editor::language_selector::{LanguageFilter, LanguageSelector};
use je_editor::language_service::ILanguageService as _;
use je_editor::languages::{
    CompletionContext, CompletionItem as LanguageCompletionItem, CompletionItemKind, CompletionItemProvider,
    CompletionItemRange, CompletionLabel, CompletionList, CompletionTriggerKind, EncodedTokenizationResult, ILanguageIdCodec,
    ITokenizationSupport, State, TokenizationResult,
};
use je_editor::editor_worker::{IEditorWorkerService, WordRanges};
use je_editor::model::RelaxedTextModelCreationOptions;
use je_editor::modes_registry::LanguageExtensionPoint;
use je_editor::null_tokenize::null_state;
use je_editor::position::Position;
use je_editor::range::Range;
use je_editor::selection::Selection;
use je_editor::snippet::snippet_controller2::{SnippetController2, SnippetController2Services};
use je_editor::snippet::snippet_session::SnippetServices;
use je_editor::suggest::suggest::{CompletionItem, get_snippet_suggest_support, label_text, provider_id, set_snippet_suggest_support};
use je_editor::suggest::suggest_controller::{SuggestController, SuggestControllerServices, insert_flags};
use je_editor::suggest::suggest_memory::ISuggestMemoryService;
use je_editor::suggest::suggest_model::{
    IInlineCompletionsController, IInlineCompletionsHost, LineContext, SuggestModel, SuggestModelServices,
    SuggestTriggerOptions,
};
use je_editor::suggest::suggest_widget_contract::ISelectedSuggestion;
use je_editor::text_model::TextModel;
use je_editor::tokenization_registry::tokenization_registry;
use je_platform::configuration::test_configuration_service::TestConfigurationService;
use je_platform::contextkey::IContextKeyService;
use je_platform::contextkey::service::ContextKeyService;
use je_platform::environment::environment::{
    DisableExtensions, EnvironmentService, ExtensionHostDebugParams, ExtensionKind, SyncMode,
};
use je_platform::log::NullLogService;
use je_platform::telemetry::telemetry_utils::null_telemetry_service;
use je_platform::workspace::Workspace;
use snippet_doubles::{ClosureLabelService, WorkspaceOnlyService};
use support::test_clipboard_service::TestClipboardService;
use support::test_code_editor::{TestCodeEditor, TestCodeEditorInstantiationOptions, create_test_code_editor};
use support::test_text_model::{
    LanguageModelServices, create_language_model_services, create_text_model_with, instantiate_text_model_with_languages,
};

// #region createMockEditor e os serviços falsos

/// O `ISuggestMemoryService` do `createMockEditor` (ver as notas no topo do arquivo).
struct NoMemoryService;

impl ISuggestMemoryService for NoMemoryService {
    fn memorize(&self, _model: &TextModel, _pos: Position, _item: &CompletionItem) {}

    fn select(&self, _model: &TextModel, _pos: Position, _items: &[CompletionItem]) -> usize {
        0
    }
}

/// O `IEditorWorkerService` falso: o `mock<IEditorWorkerService>()` sem métodos.
struct MockEditorWorkerService;

impl IEditorWorkerService for MockEditorWorkerService {
    fn can_compute_word_ranges(&self, _resource: &URI) -> bool {
        unimplemented!("mock")
    }

    fn compute_word_ranges(&self, _resource: &URI, _range: Range) -> Promise<Option<WordRanges>> {
        unimplemented!("mock")
    }
}

/// `mock<IEnvironmentService>()` com `isBuilt` e `isExtensionDevelopment`.
struct MockEnvironmentService {
    is_built: bool,
    is_extension_development: bool,
}

impl EnvironmentService for MockEnvironmentService {
    fn state_resource(&self) -> URI {
        unimplemented!("mock")
    }
    fn user_roaming_data_home(&self) -> URI {
        unimplemented!("mock")
    }
    fn keyboard_layout_resource(&self) -> URI {
        unimplemented!("mock")
    }
    fn argv_resource(&self) -> URI {
        unimplemented!("mock")
    }
    fn untitled_workspaces_home(&self) -> URI {
        unimplemented!("mock")
    }
    fn workspace_storage_home(&self) -> URI {
        unimplemented!("mock")
    }
    fn local_history_home(&self) -> URI {
        unimplemented!("mock")
    }
    fn cache_home(&self) -> URI {
        unimplemented!("mock")
    }
    fn app_shared_data_home(&self) -> URI {
        unimplemented!("mock")
    }
    fn user_data_sync_home(&self) -> URI {
        unimplemented!("mock")
    }
    fn sync(&self) -> Option<SyncMode> {
        unimplemented!("mock")
    }
    fn continue_on(&self) -> Option<String> {
        unimplemented!("mock")
    }
    fn edit_session_id(&self) -> Option<String> {
        unimplemented!("mock")
    }
    fn debug_extension_host(&self) -> ExtensionHostDebugParams {
        unimplemented!("mock")
    }
    fn is_extension_development(&self) -> bool {
        self.is_extension_development
    }
    fn disable_extensions(&self) -> DisableExtensions {
        unimplemented!("mock")
    }
    fn skip_builtin_extensions(&self) -> Vec<String> {
        unimplemented!("mock")
    }
    fn extension_development_location_uri(&self) -> Option<Vec<URI>> {
        unimplemented!("mock")
    }
    fn extension_development_kind(&self) -> Option<Vec<ExtensionKind>> {
        unimplemented!("mock")
    }
    fn extension_tests_location_uri(&self) -> Option<URI> {
        unimplemented!("mock")
    }
    fn logs_home(&self) -> URI {
        unimplemented!("mock")
    }
    fn log_level(&self) -> Option<String> {
        unimplemented!("mock")
    }
    fn extension_log_level(&self) -> Option<Vec<(String, String)>> {
        unimplemented!("mock")
    }
    fn verbose(&self) -> bool {
        unimplemented!("mock")
    }
    fn is_built(&self) -> bool {
        self.is_built
    }
    fn disable_telemetry(&self) -> bool {
        unimplemented!("mock")
    }
    fn disable_experiments(&self) -> bool {
        unimplemented!("mock")
    }
    fn service_machine_id_resource(&self) -> URI {
        unimplemented!("mock")
    }
    fn agent_sessions_workspace(&self) -> URI {
        unimplemented!("mock")
    }
    fn policy_file(&self) -> Option<URI> {
        unimplemented!("mock")
    }
}

/// `InlineCompletionsProvider`, recorte do que os casos criam (ver as notas no topo do arquivo).
trait InlineCompletionsProvider {
    /// `provideInlineCompletions`: os itens (`{ items: [] }`).
    fn provide_inline_completions(&self) -> Vec<String>;

    /// `disposeInlineCompletions`.
    fn dispose_inline_completions(&self);
}

/// `{ provideInlineCompletions: () => ({ items: [] }), disposeInlineCompletions: () => { } }`.
struct EmptyInlineCompletionsProvider;

impl InlineCompletionsProvider for EmptyInlineCompletionsProvider {
    fn provide_inline_completions(&self) -> Vec<String> {
        Vec::new()
    }

    fn dispose_inline_completions(&self) {}
}

type CompletionRegistry = Rc<LanguageFeatureRegistry<dyn CompletionItemProvider>>;
type InlineRegistry = Rc<LanguageFeatureRegistry<dyn InlineCompletionsProvider>>;

/// O `IInlineCompletionsHost` do editor sem `InlineCompletionsController`.
struct InlineCompletionsHost {
    registry: InlineRegistry,
}

impl IInlineCompletionsHost for InlineCompletionsHost {
    fn has_provider(&self, model: &TextModel) -> bool {
        self.registry.has(model)
    }

    fn get_controller(&self, _editor: &Rc<CodeEditorWidget>) -> Option<Rc<dyn IInlineCompletionsController>> {
        None
    }
}

/// O que `createMockEditor` devolve: o editor de teste e o serviço de chaves de contexto que o `SnippetController2` e o
/// `SuggestModel` compartilham (no original, o do contêiner de serviços do editor).
struct MockEditor {
    editor: Rc<TestCodeEditor>,
    context_keys: Rc<ContextKeyService>,
}

impl MockEditor {
    /// `editor.registerAndInstantiateContribution(SnippetController2.ID, SnippetController2)`.
    fn instantiate_snippet_controller(&self) -> Rc<SnippetController2> {
        let context_keys = Rc::clone(&self.context_keys);
        self.editor.register_and_instantiate_contribution(SnippetController2::ID, move |widget, _| {
            let workspace = Workspace::new("foo", Vec::new(), false, None, Rc::new(|_: &URI| false), None);
            let snippet_services = Rc::new(SnippetServices {
                language_configuration_service: widget.language_configuration_service(),
                label_service: ClosureLabelService::without_methods(),
                workspace_service: Some(WorkspaceOnlyService::new(workspace)),
            });
            let context_key_service: Rc<dyn IContextKeyService> = context_keys;
            SnippetController2::new(
                widget,
                SnippetController2Services {
                    log_service: NullLogService::shared(),
                    choice_host: None,
                    context_key_service,
                    snippet_services,
                },
            )
        })
    }
}

/// `createMockEditor(model, languageFeaturesService)`.
fn create_mock_editor(model: &Rc<TextModel>) -> MockEditor {
    let editor = create_test_code_editor(Some(Rc::clone(model)), TestCodeEditorInstantiationOptions::default());
    let context_keys = ContextKeyService::new(TestConfigurationService::new(None));
    let mock = MockEditor { editor, context_keys };
    let ctrl = mock.instantiate_snippet_controller();
    mock.editor.set_has_widget_focus(true);

    mock.editor.register_disposable(ctrl);
    let disposable_context_keys = Rc::clone(&mock.context_keys);
    mock.editor.register_disposable(to_disposable(move || disposable_context_keys.dispose()));
    mock
}

// #endregion

// #region Provedores

type ProvideFn = dyn Fn(&Rc<TextModel>, Position, &CompletionContext) -> Option<CompletionList>;

/// O provedor literal `{ _debugDisplayName: 'test', triggerCharacters?, provideCompletionItems(doc, pos, context) {...},
/// resolveCompletionItem? }`.
struct TestProvider {
    trigger_characters: Option<Vec<String>>,
    resolves: bool,
    provide: Box<ProvideFn>,
}

impl TestProvider {
    fn new(provide: impl Fn(&Rc<TextModel>, Position, &CompletionContext) -> Option<CompletionList> + 'static) -> Self {
        Self { trigger_characters: None, resolves: false, provide: Box::new(provide) }
    }

    fn with_trigger_characters(mut self, characters: &[&str]) -> Self {
        self.trigger_characters = Some(characters.iter().map(|character| (*character).to_string()).collect());
        self
    }

    /// `resolveCompletionItem(item) { return item; }`.
    fn with_resolve(mut self) -> Self {
        self.resolves = true;
        self
    }

    fn rc(self) -> Rc<dyn CompletionItemProvider> {
        Rc::new(self)
    }
}

impl CompletionItemProvider for TestProvider {
    fn debug_display_name(&self) -> &str {
        "test"
    }

    fn trigger_characters(&self) -> Option<&[String]> {
        self.trigger_characters.as_deref()
    }

    fn provide_completion_items(
        &self,
        model: &Rc<TextModel>,
        position: Position,
        context: &CompletionContext,
        _token: &je_base::cancellation::CancellationToken,
    ) -> ProviderResult<Rc<CompletionList>> {
        Promise::resolved((self.provide)(model, position, context).map(Rc::new))
    }

    fn has_resolve_completion_item(&self) -> bool {
        self.resolves
    }

    fn resolve_completion_item(
        &self,
        item: &LanguageCompletionItem,
        _token: &je_base::cancellation::CancellationToken,
    ) -> Promise<Option<LanguageCompletionItem>> {
        Promise::resolved(Some(item.clone()))
    }
}

/// `{ label, kind, insertText, range }`.
fn suggestion(label: CompletionLabel, kind: CompletionItemKind, insert_text: &str, range: Range) -> LanguageCompletionItem {
    LanguageCompletionItem {
        label,
        kind,
        range: CompletionItemRange::Single(range),
        tags: None,
        detail: None,
        documentation: None,
        sort_text: None,
        filter_text: None,
        preselect: None,
        insert_text: insert_text.to_string(),
        insert_text_rules: None,
        commit_characters: None,
        additional_text_edits: None,
    }
}

/// `{ label, kind, insertText, range }` com o rótulo em texto.
fn text_suggestion(label: &str, kind: CompletionItemKind, insert_text: &str, range: Range) -> LanguageCompletionItem {
    suggestion(CompletionLabel::Text(label.to_string()), kind, insert_text, range)
}

/// `{ incomplete, suggestions }`.
fn list(suggestions: Vec<LanguageCompletionItem>, incomplete: Option<bool>) -> Option<CompletionList> {
    Some(CompletionList { suggestions, incomplete, dispose: None, duration: None })
}

/// `getDefaultSuggestRange(model, position)`.
fn get_default_suggest_range(model: &TextModel, position: Position) -> Range {
    let word_until = model.get_word_until_position(position);
    Range::new(position.line_number, word_until.start_column, position.line_number, word_until.end_column)
}

/// `Range.fromPositions(pos.with(undefined, 1), pos)`.
fn range_from_line_start(position: Position) -> Range {
    Range::from_positions(Position::new(position.line_number, 1), position)
}

/// `alwaysEmptySupport`.
fn always_empty_support() -> Rc<dyn CompletionItemProvider> {
    TestProvider::new(|_doc, _pos, _context| list(Vec::new(), Some(false))).rc()
}

/// `alwaysSomethingSupport`.
fn always_something_support() -> Rc<dyn CompletionItemProvider> {
    TestProvider::new(|doc, pos, _context| {
        list(
            vec![suggestion(
                CompletionLabel::Text(doc.get_word_until_position(pos).word.to_string_lossy()),
                CompletionItemKind::Property,
                "foofoo",
                get_default_suggest_range(doc, pos),
            )],
            Some(false),
        )
    })
    .rc()
}

fn label_of(item: &CompletionItem) -> String {
    label_text(&item.completion.borrow().label).to_string()
}

// #endregion

// #region Fixture

fn selector_test() -> LanguageSelector {
    LanguageSelector::Filter(LanguageFilter { scheme: Some("test".to_string()), ..LanguageFilter::default() })
}

/// O que o `setup` e as variáveis da suíte `TriggerAndCancelOracle` deixam: `disposables`, `model`, e os registros do
/// `languageFeaturesService`.
struct Fixture {
    disposables: Rc<DisposableStore>,
    model: Rc<TextModel>,
    completion_provider: CompletionRegistry,
    inline_completions_provider: InlineRegistry,
}

impl Fixture {
    /// `disposables.add(registry.register({ scheme: 'test' }, provider))`.
    fn register(&self, provider: &Rc<dyn CompletionItemProvider>) {
        self.disposables.add(self.completion_provider.register(selector_test(), Rc::clone(provider)));
    }

    /// `disposables.add(languageFeaturesService.inlineCompletionsProvider.register({ scheme: 'test' }, inlineProvider))`.
    fn register_inline(&self) {
        let provider: Rc<dyn InlineCompletionsProvider> = Rc::new(EmptyInlineCompletionsProvider);
        self.disposables.add(self.inline_completions_provider.register(selector_test(), provider));
    }

    /// Os serviços que o contêiner de serviços do editor entregaria ao `SuggestModel`.
    fn model_services(&self, context_keys: &Rc<ContextKeyService>) -> SuggestModelServices {
        let context_key_service: Rc<dyn IContextKeyService> = Rc::clone(context_keys);
        SuggestModelServices {
            editor_worker_service: Rc::new(MockEditorWorkerService),
            clipboard_service: Rc::new(TestClipboardService::new()),
            telemetry_service: null_telemetry_service(),
            log_service: NullLogService::shared(),
            context_key_service,
            completion_provider: Rc::clone(&self.completion_provider),
            inline_completions: Some(Rc::new(InlineCompletionsHost { registry: Rc::clone(&self.inline_completions_provider) })),
            environment_service: Rc::new(MockEnvironmentService { is_built: true, is_extension_development: false }),
        }
    }

    /// `withOracle(callback)`: o editor, o `SuggestModel`, e o descarte de ambos.
    fn with_oracle(&self) -> Oracle {
        let mock = create_mock_editor(&self.model);
        let model = SuggestModel::new(mock.editor.widget(), self.model_services(&mock.context_keys));
        self.disposables.add_rc(Rc::clone(&model));
        self.disposables.add_rc(Rc::clone(&mock.editor));
        Oracle { model, editor: Rc::clone(&mock.editor), mock: Rc::new(mock) }
    }

    /// `teardown()`.
    fn teardown(&self) {
        self.disposables.dispose();
    }
}

/// O `(model, editor)` que `withOracle` entrega ao callback.
#[derive(Clone)]
struct Oracle {
    model: Rc<SuggestModel>,
    editor: Rc<TestCodeEditor>,
    mock: Rc<MockEditor>,
}

impl Oracle {
    /// `model.trigger({ auto })`.
    fn trigger(&self, auto: bool) {
        self.model.trigger(SuggestTriggerOptions { auto, ..SuggestTriggerOptions::default() });
    }

    /// `editor.trigger('keyboard', Handler.Type, { text })`.
    fn type_text(&self, text: &str) {
        self.editor.trigger(Some("keyboard"), Handler::TYPE, Some(TypePayload { text: JsString::from(text) }.to_dynamic()));
    }

    /// `editor.setPosition({ lineNumber, column })`.
    fn set_position(&self, line_number: i32, column: i32) {
        self.editor.set_position(Position::new(line_number, column), None);
    }

    /// `editor.setSelection(new Selection(...))`.
    fn set_selection(&self, start_line_number: i32, start_column: i32, end_line_number: i32, end_column: i32) {
        self.editor.set_selection(
            SelectionOrRange::Selection(Selection::new(start_line_number, start_column, end_line_number, end_column)),
            None,
        );
    }

    /// `editor.setValue(value)`.
    fn set_value(&self, value: &str) {
        self.editor.set_value(JsString::from(value));
    }

    /// `editor.getModel()!.setValue(value)`.
    fn set_model_value(&self, value: &str) {
        self.editor.get_model().expect("o editor tem modelo").set_value(value.into());
    }

    /// `editor.updateOptions({ quickSuggestions })` com o objeto `{ comments, strings, other }`.
    fn update_quick_suggestions_object(&self, comments: &str, strings: &str, other: &str) {
        self.editor.update_options(Some(&Dynamic::object([(
            "quickSuggestions",
            quick_suggestions_object(comments, strings, other),
        )])));
    }
}

/// `{ comments, strings, other }` de `quickSuggestions`.
fn quick_suggestions_object(comments: &str, strings: &str, other: &str) -> Dynamic {
    Dynamic::object([
        ("comments", Dynamic::string(comments)),
        ("strings", Dynamic::string(strings)),
        ("other", Dynamic::string(other)),
    ])
}

/// O que o mocha faz em volta de cada `test(...)` da suíte `TriggerAndCancelOracle` (ver as notas no topo do arquivo).
fn run_test<F: Future<Output = ()> + 'static>(body: impl FnOnce(Rc<Fixture>) -> F) {
    je_editor::core_commands::register_core_commands();

    // o `languageFeaturesService` do original é criado na carga da suíte, antes do rastreio de vazamentos
    let completion_provider: CompletionRegistry = Rc::new(LanguageFeatureRegistry::new(None));
    let inline_completions_provider: InlineRegistry = Rc::new(LanguageFeatureRegistry::new(None));

    let _ds = ensure_no_disposables_are_leaked_in_test_suite();

    // setup
    let disposables = DisposableStore::new();
    let model = create_text_model_with(
        "abc def",
        None,
        RelaxedTextModelCreationOptions::default(),
        Some(URI::parse("test:somefile.ttt", false)),
    );
    let owned_model = Rc::clone(&model);
    disposables.add(to_disposable(move || owned_model.dispose()));
    let fixture = Rc::new(Fixture { disposables, model, completion_provider, inline_completions_provider });

    run_with_faked_timers(async move {
        body(Rc::clone(&fixture)).await;
        fixture.teardown();
    });
}

fn ok() -> Promise<()> {
    Promise::resolved(())
}

/// `assertEvent(event, action, assert)`.
fn assert_event<E: 'static>(
    event: Event<E>,
    action: impl FnOnce(),
    assert_fn: impl FnOnce(&E) -> Promise<()> + 'static,
) -> Promise<()> {
    let (promise, resolver) = Promise::<()>::new();
    let sub: Rc<RefCell<Option<DisposableRef>>> = Rc::new(RefCell::new(None));
    let assert_fn = RefCell::new(Some(assert_fn));
    let listener = {
        let (sub, resolver) = (Rc::clone(&sub), resolver.clone());
        event.listen(move |e| {
            let listener = sub.borrow_mut().take();
            if let Some(listener) = listener {
                listener.dispose();
            }
            let Some(assert_fn) = assert_fn.borrow_mut().take() else {
                return;
            };
            match errors::catch(|| assert_fn(e)) {
                Ok(result) => resolver.resolve_with(&result),
                Err(error) => resolver.reject(error),
            }
        })
    };
    *sub.borrow_mut() = Some(listener);
    if let Err(error) = errors::catch(action) {
        let listener = sub.borrow_mut().take();
        if let Some(listener) = listener {
            listener.dispose();
        }
        resolver.reject(error);
    }
    promise
}

/// `await promise` de um `assertEvent`: uma rejeição reprova o caso.
async fn settled(promise: Promise<()>) {
    promise.await.expect("assertEvent");
}

// #endregion

// suite('SuggestModel - Context')

/// `assertAutoTrigger(model, offset, expected, message)`.
fn assert_auto_trigger(model: &Rc<TextModel>, offset: i64, expected: bool, message: &str) {
    let pos = model.get_position_at(offset);
    let mock = create_mock_editor(model);
    mock.editor.set_position(pos, None);
    assert_eq!(LineContext::should_auto_trigger(&mock.editor), expected, "{message}");
    mock.editor.dispose();
}

#[test]
fn context_should_auto_trigger() {
    je_editor::core_commands::register_core_commands();
    let ds = ensure_no_disposables_are_leaked_in_test_suite();

    let model = create_text_model_with(
        "Das Pferd frisst keinen Gurkensalat - Philipp Reis 1861.\nWer hat's erfunden?",
        None,
        RelaxedTextModelCreationOptions::default(),
        None,
    );
    let owned_model = Rc::clone(&model);
    ds.add(to_disposable(move || {
        if !owned_model.is_disposed() {
            owned_model.dispose();
        }
    }));

    assert_auto_trigger(&model, 3, true, "end of word, Das|");
    assert_auto_trigger(&model, 4, false, "no word Das |");
    assert_auto_trigger(&model, 1, true, "typing a single character before a word: D|as");
    assert_auto_trigger(&model, 55, false, "number, 1861|");
    model.dispose();
}

const OUTER_LANGUAGE_ID: &str = "outerMode";
const INNER_LANGUAGE_ID: &str = "innerMode";

/// O tokenizador do `OuterMode`: cada `x` é da linguagem interna, o resto da externa.
struct OuterModeTokenizationSupport {
    language_id_codec: Rc<dyn ILanguageIdCodec>,
}

impl ITokenizationSupport for OuterModeTokenizationSupport {
    fn get_initial_state(&self) -> State {
        null_state()
    }

    fn tokenize(&self, _line: &JsString, _has_eol: bool, _state: &State) -> TokenizationResult {
        unreachable!("`tokenize` é `undefined!` no original")
    }

    fn tokenize_encoded(&self, line: &JsString, _has_eol: bool, state: &State) -> EncodedTokenizationResult {
        let mut tokens_arr: Vec<u32> = Vec::new();
        let mut prev_language_id: Option<&str> = None;
        for (i, unit) in line.units().iter().enumerate() {
            let language_id = if *unit == u16::from(b'x') { INNER_LANGUAGE_ID } else { OUTER_LANGUAGE_ID };
            let encoded_language_id = self.language_id_codec.encode_language_id(language_id);
            if prev_language_id != Some(language_id) {
                tokens_arr.push(i as u32);
                tokens_arr.push(encoded_language_id << metadata_consts::LANGUAGEID_OFFSET);
            }
            prev_language_id = Some(language_id);
        }

        EncodedTokenizationResult { tokens: tokens_arr, font_info: Vec::new(), end_state: state.clone() }
    }
}

/// `class OuterMode`: a linguagem, a configuração e o tokenizador, descartados com o `LeakGuard`.
fn register_outer_mode(ds: &common::LeakGuard, services: &LanguageModelServices) {
    ds.add(services.language_service.register_language(LanguageExtensionPoint::new(OUTER_LANGUAGE_ID)));
    ds.add(services.language_configuration_service.register(OUTER_LANGUAGE_ID, LanguageConfiguration::default(), None));

    let support = OuterModeTokenizationSupport { language_id_codec: services.language_service.language_id_codec() };
    ds.add(tokenization_registry().register(OUTER_LANGUAGE_ID, Rc::new(support)));
}

/// `class InnerMode`: a linguagem e a configuração, descartadas com o `LeakGuard`.
fn register_inner_mode(ds: &common::LeakGuard, services: &LanguageModelServices) {
    ds.add(services.language_service.register_language(LanguageExtensionPoint::new(INNER_LANGUAGE_ID)));
    ds.add(services.language_configuration_service.register(INNER_LANGUAGE_ID, LanguageConfiguration::default(), None));
}

#[test]
fn should_auto_trigger_at_embedded_language_boundaries() {
    je_editor::core_commands::register_core_commands();
    let ds = ensure_no_disposables_are_leaked_in_test_suite();

    let services = create_language_model_services(&ds);
    register_outer_mode(&ds, &services);
    register_inner_mode(&ds, &services);

    let model = instantiate_text_model_with_languages(&services, "a<xx>a<x>", OUTER_LANGUAGE_ID);
    let owned_model = Rc::clone(&model);
    ds.add(to_disposable(move || owned_model.dispose()));

    assert_auto_trigger(&model, 1, true, "a|<x - should trigger at end of word");
    assert_auto_trigger(&model, 2, false, "a<|x - should NOT trigger at start of word");
    assert_auto_trigger(&model, 3, true, "a<x|x -  should trigger after typing a single character before a word");
    assert_auto_trigger(&model, 4, true, "a<xx|> - should trigger at boundary between languages");
    assert_auto_trigger(&model, 5, false, "a<xx>|a - should NOT trigger at start of word");
    assert_auto_trigger(&model, 6, true, "a<xx>a|< - should trigger at end of word");
    assert_auto_trigger(&model, 8, true, "a<xx>a<x|> - should trigger at end of word at boundary");
}

// suite('SuggestModel - TriggerAndCancelOracle')

#[test]
fn events_cancel_trigger() {
    run_test(|fx| async move {
        let o = fx.with_oracle();

        let first = {
            let o = o.clone();
            assert_event(
                o.model.on_did_trigger(),
                || o.trigger(true),
                {
                    let o = o.clone();
                    move |event| {
                        assert!(event.auto);

                        assert_event(
                            o.model.on_did_cancel(),
                            || o.model.cancel(false),
                            |event| {
                                assert!(!event.retrigger);
                                ok()
                            },
                        )
                    }
                },
            )
        };

        let second = assert_event(
            o.model.on_did_trigger(),
            || o.trigger(true),
            |event| {
                assert!(event.auto);
                ok()
            },
        );

        let third = assert_event(
            o.model.on_did_trigger(),
            || o.trigger(false),
            |event| {
                assert!(!event.auto);
                ok()
            },
        );

        Promise::<()>::all(vec![first, second, third]).await.expect("Promise.all");
    });
}

#[test]
fn events_suggest_empty() {
    run_test(|fx| async move {
        fx.register(&always_empty_support());

        let o = fx.with_oracle();

        let cancelled = assert_event(
            o.model.on_did_cancel(),
            || o.trigger(true),
            |event| {
                assert!(!event.retrigger);
                ok()
            },
        );
        let suggested = assert_event(
            o.model.on_did_suggest(),
            || o.trigger(false),
            |event| {
                assert!(!event.trigger_options.auto);
                assert!(!event.is_frozen);
                assert_eq!(event.completion_model.items().len(), 0);
                ok()
            },
        );
        Promise::<()>::all(vec![cancelled, suggested]).await.expect("Promise.all");
    });
}

#[test]
fn trigger_on_type() {
    run_test(|fx| async move {
        let always_something_support = always_something_support();
        fx.register(&always_something_support);

        let o = fx.with_oracle();

        settled(assert_event(
            o.model.on_did_suggest(),
            || {
                o.set_position(1, 4);
                o.type_text("d");
            },
            move |event| {
                assert!(event.trigger_options.auto);
                assert_eq!(event.completion_model.items().len(), 1);
                let items = event.completion_model.items();
                let first = &items[0];

                assert!(provider_id(&first.provider) == provider_id(&always_something_support));
                ok()
            },
        ))
        .await;
    });
}

#[test]
fn issue_17400_keep_filtering_suggest_model_ts_after_space() {
    run_test(|fx| async move {
        let provider = TestProvider::new(|doc, pos, _context| {
            list(
                vec![text_suggestion("My Table", CompletionItemKind::Property, "My Table", get_default_suggest_range(doc, pos))],
                Some(false),
            )
        })
        .rc();
        fx.register(&provider);

        fx.model.set_value("".into());

        let o = fx.with_oracle();

        settled(assert_event(
            o.model.on_did_suggest(),
            || {
                // make sure completionModel starts here!
                o.trigger(true);
            },
            {
                let o = o.clone();
                move |_event| {
                    let o2 = o.clone();
                    assert_event(
                        o.model.on_did_suggest(),
                        || {
                            o.set_position(1, 1);
                            o.type_text("My");
                        },
                        move |event| {
                            assert!(event.trigger_options.auto);
                            assert_eq!(event.completion_model.items().len(), 1);
                            let items = event.completion_model.items();
                            assert_eq!(label_of(&items[0]), "My Table");

                            let o = o2.clone();
                            assert_event(
                                o2.model.on_did_suggest(),
                                || {
                                    o.set_position(1, 3);
                                    o.type_text(" ");
                                },
                                |event| {
                                    assert!(event.trigger_options.auto);
                                    assert_eq!(event.completion_model.items().len(), 1);
                                    let items = event.completion_model.items();
                                    assert_eq!(label_of(&items[0]), "My Table");
                                    ok()
                                },
                            )
                        },
                    )
                }
            },
        ))
        .await;
    });
}

#[test]
fn issue_21484_trigger_character_always_force_a_new_completion_session() {
    run_test(|fx| async move {
        fx.register(
            &TestProvider::new(|_doc, pos, _context| {
                list(
                    vec![text_suggestion("foo.bar", CompletionItemKind::Property, "foo.bar", range_from_line_start(pos))],
                    Some(false),
                )
            })
            .rc(),
        );

        fx.register(
            &TestProvider::new(|doc, pos, _context| {
                let content = doc.get_line_content(pos.line_number);
                let index = pos.column - 2;
                let preceded_by_dot = index >= 0 && content.units().get(index as usize) == Some(&u16::from(b'.'));
                list(
                    vec![text_suggestion(
                        "boom",
                        CompletionItemKind::Property,
                        "boom",
                        Range::from_positions(Position::new(pos.line_number, pos.column + if preceded_by_dot { 0 } else { -1 }), pos),
                    )],
                    Some(false),
                )
            })
            .with_trigger_characters(&["."])
            .rc(),
        );

        fx.model.set_value("".into());

        let o = fx.with_oracle();

        settled(assert_event(
            o.model.on_did_suggest(),
            || {
                o.set_position(1, 1);
                o.type_text("foo");
            },
            |event| {
                assert!(event.trigger_options.auto);
                assert_eq!(event.completion_model.items().len(), 1);
                let items = event.completion_model.items();
                assert_eq!(label_of(&items[0]), "foo.bar");
                ok()
            },
        ))
        .await;

        settled(assert_event(
            o.model.on_did_suggest(),
            || {
                o.type_text(".");
            },
            |event| {
                // SYNC
                assert!(event.trigger_options.auto);
                assert_eq!(event.completion_model.items().len(), 1);
                let items = event.completion_model.items();
                assert_eq!(label_of(&items[0]), "foo.bar");
                ok()
            },
        ))
        .await;

        settled(assert_event(
            o.model.on_did_suggest(),
            || {
                // nothing -> triggered by the trigger character typing (see above)
            },
            |event| {
                // ASYNC
                assert!(event.trigger_options.auto);
                assert_eq!(event.completion_model.items().len(), 2);
                let items = event.completion_model.items();
                assert_eq!(label_of(&items[0]), "foo.bar");
                assert_eq!(label_of(&items[1]), "boom");
                ok()
            },
        ))
        .await;
    });
}

/// O corpo do caso `Intellisense Completion doesn't respect space after equal sign (.html file), #29353`, com o texto
/// que dispara o cancelamento (`+` em 1/2 e ` ` em 2/2).
async fn space_after_equal_sign_case(fx: Rc<Fixture>, typed: &'static str) {
    fx.register(&always_something_support());

    let o = fx.with_oracle();

    o.set_model_value("fo");
    o.set_position(1, 3);

    settled(assert_event(
        o.model.on_did_suggest(),
        || o.trigger(false),
        {
            let o = o.clone();
            move |event| {
                assert!(!event.trigger_options.auto);
                assert!(!event.is_frozen);
                assert_eq!(event.completion_model.items().len(), 1);

                assert_event(
                    o.model.on_did_cancel(),
                    || o.type_text(typed),
                    |event| {
                        assert!(!event.retrigger);
                        ok()
                    },
                )
            }
        },
    ))
    .await;
}

#[test]
fn intellisense_completion_does_not_respect_space_after_equal_sign_html_file_29353_1_2() {
    run_test(|fx| space_after_equal_sign_case(fx, "+"));
}

#[test]
fn intellisense_completion_does_not_respect_space_after_equal_sign_html_file_29353_2_2() {
    run_test(|fx| space_after_equal_sign_case(fx, " "));
}

#[test]
fn incomplete_suggestion_results_cause_re_triggering_when_typing_without_further_context_28400_1_2() {
    run_test(|fx| async move {
        fx.register(
            &TestProvider::new(|_doc, pos, _context| {
                list(vec![text_suggestion("foo", CompletionItemKind::Property, "foo", range_from_line_start(pos))], Some(true))
            })
            .rc(),
        );

        let o = fx.with_oracle();

        o.set_model_value("foo");
        o.set_position(1, 4);

        settled(assert_event(
            o.model.on_did_suggest(),
            || o.trigger(false),
            {
                let o = o.clone();
                move |event| {
                    assert!(!event.trigger_options.auto);
                    assert_eq!(event.completion_model.get_incomplete_provider().size(), 1);
                    assert_eq!(event.completion_model.items().len(), 1);

                    assert_event(
                        o.model.on_did_cancel(),
                        || o.type_text(";"),
                        |event| {
                            assert!(!event.retrigger);
                            ok()
                        },
                    )
                }
            },
        ))
        .await;
    });
}

#[test]
fn incomplete_suggestion_results_cause_re_triggering_when_typing_without_further_context_28400_2_2() {
    run_test(|fx| async move {
        fx.register(
            &TestProvider::new(|_doc, pos, _context| {
                list(vec![text_suggestion("foo;", CompletionItemKind::Property, "foo", range_from_line_start(pos))], Some(true))
            })
            .rc(),
        );

        let o = fx.with_oracle();

        o.set_model_value("foo");
        o.set_position(1, 4);

        settled(assert_event(
            o.model.on_did_suggest(),
            || o.trigger(false),
            {
                let o = o.clone();
                move |event| {
                    assert!(!event.trigger_options.auto);
                    assert_eq!(event.completion_model.get_incomplete_provider().size(), 1);
                    assert_eq!(event.completion_model.items().len(), 1);

                    assert_event(
                        o.model.on_did_suggest(),
                        || {
                            // while we cancel incrementally enriching the set of
                            // completions we still filter against those that we have
                            // until now
                            o.type_text(";");
                        },
                        |event| {
                            assert!(!event.trigger_options.auto);
                            assert_eq!(event.completion_model.get_incomplete_provider().size(), 1);
                            assert_eq!(event.completion_model.items().len(), 1);
                            ok()
                        },
                    )
                }
            },
        ))
        .await;
    });
}

#[test]
fn trigger_character_is_provided_in_suggest_context() {
    run_test(|fx| async move {
        let trigger_character = Rc::new(RefCell::new(String::new()));
        {
            let trigger_character = Rc::clone(&trigger_character);
            fx.register(
                &TestProvider::new(move |_doc, pos, context| {
                    assert_eq!(context.trigger_kind, CompletionTriggerKind::TriggerCharacter);
                    *trigger_character.borrow_mut() = context.trigger_character.clone().expect("triggerCharacter");
                    list(
                        vec![text_suggestion("foo.bar", CompletionItemKind::Property, "foo.bar", range_from_line_start(pos))],
                        Some(false),
                    )
                })
                .with_trigger_characters(&["."])
                .rc(),
            );
        }

        fx.model.set_value("".into());

        let o = fx.with_oracle();

        settled(assert_event(
            o.model.on_did_suggest(),
            || {
                o.set_position(1, 1);
                o.type_text("foo.");
            },
            move |_event| {
                assert_eq!(*trigger_character.borrow(), ".");
                ok()
            },
        ))
        .await;
    });
}

#[test]
fn mac_press_and_hold_accent_character_insertion_does_not_update_suggestions_35269() {
    run_test(|fx| async move {
        fx.register(
            &TestProvider::new(|_doc, pos, _context| {
                list(
                    vec![
                        text_suggestion("abc", CompletionItemKind::Property, "abc", range_from_line_start(pos)),
                        text_suggestion("äbc", CompletionItemKind::Property, "äbc", range_from_line_start(pos)),
                    ],
                    Some(true),
                )
            })
            .rc(),
        );

        fx.model.set_value("".into());
        let o = fx.with_oracle();

        settled(assert_event(
            o.model.on_did_suggest(),
            || {
                o.set_position(1, 1);
                o.type_text("a");
            },
            {
                let o = o.clone();
                move |event| {
                    assert_eq!(event.completion_model.items().len(), 1);
                    assert_eq!(label_of(&event.completion_model.items()[0]), "abc");

                    assert_event(
                        o.model.on_did_suggest(),
                        || {
                            o.editor.execute_edits(
                                ExecuteEditsSource::Name(Some("test")),
                                vec![EditOperation::replace(Range::new(1, 1, 1, 2), Some(JsString::from("ä")))],
                                None,
                            );
                        },
                        |event| {
                            // suggest model changed to äbc
                            assert_eq!(event.completion_model.items().len(), 1);
                            assert_eq!(label_of(&event.completion_model.items()[0]), "äbc");
                            ok()
                        },
                    )
                }
            },
        ))
        .await;
    });
}

#[test]
fn backspace_should_not_always_cancel_code_completion_36491() {
    run_test(|fx| async move {
        let always_something_support = always_something_support();
        fx.register(&always_something_support);

        let o = fx.with_oracle();

        {
            let always_something_support = Rc::clone(&always_something_support);
            settled(assert_event(
                o.model.on_did_suggest(),
                || {
                    o.set_position(1, 4);
                    o.type_text("d");
                },
                move |event| {
                    assert!(event.trigger_options.auto);
                    assert_eq!(event.completion_model.items().len(), 1);
                    let items = event.completion_model.items();
                    let first = &items[0];

                    assert!(provider_id(&first.provider) == provider_id(&always_something_support));
                    ok()
                },
            ))
            .await;
        }

        settled(assert_event(
            o.model.on_did_suggest(),
            || {
                // `editor.runCommand(CoreEditingCommands.DeleteLeft, null)`
                let delete_left = core_editing_commands::instance().delete_left.clone();
                let _ = IEditorCommand::run_editor_command(&*delete_left, None, o.editor.widget(), &Dynamic::Null);
            },
            move |event| {
                assert!(event.trigger_options.auto);
                assert_eq!(event.completion_model.items().len(), 1);
                let items = event.completion_model.items();
                let first = &items[0];

                assert!(provider_id(&first.provider) == provider_id(&always_something_support));
                ok()
            },
        ))
        .await;
    });
}

#[test]
fn text_changes_for_completion_code_action_are_affected_by_the_completion_39893() {
    run_test(|fx| async move {
        fx.register(
            &TestProvider::new(|_doc, pos, _context| {
                let mut item = text_suggestion(
                    "bar",
                    CompletionItemKind::Property,
                    "bar",
                    Range::from_positions(Position::new(pos.line_number, pos.column - 2), pos),
                );
                item.additional_text_edits = Some(vec![SingleEditOperation::new(
                    Range::new(1, 17, 1, 17),
                    Some(JsString::from(", bar")),
                    false,
                )]);
                list(vec![item], Some(true))
            })
            .rc(),
        );

        fx.model.set_value("ba; import { foo } from \"./b\"".into());

        let o = fx.with_oracle();

        // `class TestCtrl extends SuggestController`: o `_insertSuggestion` é público no porte.
        let ctrl = {
            let services = fx.model_services(&o.mock.context_keys);
            let context_key_service: Rc<dyn IContextKeyService> = Rc::clone(&o.mock.context_keys);
            o.editor.register_and_instantiate_contribution(SuggestController::ID, move |widget, _| {
                SuggestController::new(
                    widget,
                    SuggestControllerServices {
                        memory_service: Rc::new(NoMemoryService),
                        context_key_service,
                        log_service: NullLogService::shared(),
                        model_services: services,
                        aria_alert: Rc::new(|_: &str| {}),
                    },
                )
            })
        };
        o.mock.instantiate_snippet_controller();

        settled(assert_event(
            o.model.on_did_suggest(),
            || {
                o.set_position(1, 3);
                o.trigger(false);
            },
            move |event| {
                assert_eq!(event.completion_model.items().len(), 1);
                let items = event.completion_model.items();
                let first = &items[0];
                assert_eq!(label_of(first), "bar");

                ctrl.insert_suggestion(
                    Some(&ISelectedSuggestion {
                        item: first.clone(),
                        index: 0,
                        model: Rc::clone(&event.completion_model),
                    }),
                    insert_flags::NONE,
                );
                ok()
            },
        ))
        .await;

        assert_eq!(fx.model.get_value(), "bar; import { foo, bar } from \"./b\"");
    });
}

#[test]
fn completion_unexpectedly_triggers_on_second_keypress_of_an_edit_group_in_a_snippet_43523() {
    run_test(|fx| async move {
        let always_something_support = always_something_support();
        fx.register(&always_something_support);

        let o = fx.with_oracle();

        settled(assert_event(
            o.model.on_did_suggest(),
            || {
                o.set_value("d");
                o.set_selection(1, 1, 1, 2);
                o.type_text("e");
            },
            move |event| {
                assert!(event.trigger_options.auto);
                assert_eq!(event.completion_model.items().len(), 1);
                let items = event.completion_model.items();
                let first = &items[0];

                assert!(provider_id(&first.provider) == provider_id(&always_something_support));
                ok()
            },
        ))
        .await;
    });
}

#[test]
fn fails_to_render_completion_details_47988() {
    run_test(|fx| async move {
        let dispose_a = Rc::new(Cell::new(0));
        let dispose_b = Rc::new(Cell::new(0));

        {
            let dispose_a = Rc::clone(&dispose_a);
            fx.register(
                &TestProvider::new(move |doc, pos, _context| {
                    let counter = Rc::clone(&dispose_a);
                    let mut item = text_suggestion(
                        "CompleteNot",
                        CompletionItemKind::Folder,
                        "Incomplete",
                        get_default_suggest_range(doc, pos),
                    );
                    item.sort_text = Some("a".to_string());
                    Some(CompletionList {
                        suggestions: vec![item],
                        incomplete: Some(true),
                        dispose: Some(Rc::new(move || counter.set(counter.get() + 1))),
                        duration: None,
                    })
                })
                .rc(),
            );
        }
        {
            let dispose_b = Rc::clone(&dispose_b);
            fx.register(
                &TestProvider::new(move |doc, pos, _context| {
                    let counter = Rc::clone(&dispose_b);
                    let mut item =
                        text_suggestion("Complete", CompletionItemKind::Folder, "Complete", get_default_suggest_range(doc, pos));
                    item.sort_text = Some("z".to_string());
                    Some(CompletionList {
                        suggestions: vec![item],
                        incomplete: Some(false),
                        dispose: Some(Rc::new(move || counter.set(counter.get() + 1))),
                        duration: None,
                    })
                })
                .with_resolve()
                .rc(),
            );
        }

        let o = fx.with_oracle();

        {
            let (dispose_a, dispose_b) = (Rc::clone(&dispose_a), Rc::clone(&dispose_b));
            settled(assert_event(
                o.model.on_did_suggest(),
                || {
                    o.set_value("");
                    o.set_selection(1, 1, 1, 1);
                    o.type_text("c");
                },
                move |event| {
                    assert!(event.trigger_options.auto);
                    assert_eq!(event.completion_model.items().len(), 2);
                    assert_eq!(dispose_a.get(), 0);
                    assert_eq!(dispose_b.get(), 0);
                    ok()
                },
            ))
            .await;
        }

        settled(assert_event(
            o.model.on_did_suggest(),
            || {
                o.type_text("o");
            },
            {
                let o = o.clone();
                move |event| {
                    assert!(event.trigger_options.auto);
                    assert_eq!(event.completion_model.items().len(), 2);

                    // clean up
                    o.model.clear();
                    assert_eq!(dispose_a.get(), 2); // provide got called two times!
                    assert_eq!(dispose_b.get(), 1);
                    ok()
                }
            },
        ))
        .await;
    });
}

#[test]
fn trigger_full_completions_when_incomplete_completions_are_already_active_99504() {
    run_test(|fx| async move {
        let count_a = Rc::new(Cell::new(0));
        let count_b = Rc::new(Cell::new(0));

        {
            let count_a = Rc::clone(&count_a);
            fx.register(
                &TestProvider::new(move |_doc, pos, _context| {
                    count_a.set(count_a.get() + 1);
                    list(
                        vec![text_suggestion(
                            "Z aaa",
                            CompletionItemKind::Class,
                            "Z aaa",
                            Range::new(1, 1, pos.line_number, pos.column),
                        )],
                        Some(false), // doesn't matter if incomplete or not
                    )
                })
                .rc(),
            );
        }
        {
            let count_b = Rc::clone(&count_b);
            fx.register(
                &TestProvider::new(move |doc, pos, _context| {
                    count_b.set(count_b.get() + 1);
                    if !doc.get_word_until_position(pos).word.units().starts_with(&[u16::from(b'a')]) {
                        return None;
                    }
                    list(
                        vec![text_suggestion("aaa", CompletionItemKind::Folder, "aaa", get_default_suggest_range(doc, pos))],
                        Some(false),
                    )
                })
                .rc(),
            );
        }

        let o = fx.with_oracle();

        settled(assert_event(
            o.model.on_did_suggest(),
            || {
                o.set_value("");
                o.set_selection(1, 1, 1, 1);
                o.type_text("Z");
            },
            |event| {
                assert!(event.trigger_options.auto);
                assert_eq!(event.completion_model.items().len(), 1);
                assert_eq!(event.completion_model.items()[0].text_label, "Z aaa");
                ok()
            },
        ))
        .await;

        settled(assert_event(
            o.model.on_did_suggest(),
            || {
                // started another word: Z a|
                // item should be: Z aaa, aaa
                o.type_text(" a");
            },
            move |event| {
                assert!(event.trigger_options.auto);
                assert_eq!(event.completion_model.items().len(), 2);
                assert_eq!(event.completion_model.items()[0].text_label, "Z aaa");
                assert_eq!(event.completion_model.items()[1].text_label, "aaa");

                assert_eq!(count_a.get(), 1); // should we keep the suggestions from the "active" provider?, Yes! See: #106573
                assert_eq!(count_b.get(), 2);
                ok()
            },
        ))
        .await;
    });
}

#[test]
fn register_completion_item_provider_with_letters_as_trigger_characters_block_other_completion_items_to_show_up_127815() {
    run_test(|fx| async move {
        fx.register(
            &TestProvider::new(|_doc, pos, _context| {
                list(
                    vec![text_suggestion(
                        "AAAA",
                        CompletionItemKind::Class,
                        "WordTriggerA",
                        Range::new(pos.line_number, pos.column, pos.line_number, pos.column),
                    )],
                    None,
                )
            })
            .rc(),
        );
        fx.register(
            &TestProvider::new(|_doc, pos, _context| {
                list(
                    vec![text_suggestion(
                        "AAAA",
                        CompletionItemKind::Class,
                        "AutoTriggerA",
                        Range::new(pos.line_number, pos.column, pos.line_number, pos.column),
                    )],
                    None,
                )
            })
            .with_trigger_characters(&["a", "."])
            .rc(),
        );

        let o = fx.with_oracle();

        settled(assert_event(
            o.model.on_did_suggest(),
            || {
                o.set_value("");
                o.set_selection(1, 1, 1, 1);
                o.type_text(".");
            },
            |event| {
                assert!(event.trigger_options.auto);
                assert_eq!(event.completion_model.items().len(), 1);
                ok()
            },
        ))
        .await;

        o.editor.get_model().expect("o editor tem modelo").set_value("".into());

        settled(assert_event(
            o.model.on_did_suggest(),
            || {
                o.set_value("");
                o.set_selection(1, 1, 1, 1);
                o.type_text("a");
            },
            |event| {
                assert!(event.trigger_options.auto);
                assert_eq!(event.completion_model.items().len(), 2);
                ok()
            },
        ))
        .await;
    });
}

#[test]
fn unexpected_suggest_scoring_167242() {
    run_test(|fx| async move {
        fx.disposables.add(fx.completion_provider.register(
            LanguageSelector::Id("*".to_string()),
            // word-based
            TestProvider::new(|doc, pos, _context| {
                let word = doc.get_word_until_position(pos);
                list(
                    vec![text_suggestion(
                        "pull",
                        CompletionItemKind::Text,
                        "pull",
                        Range::new(pos.line_number, word.start_column, pos.line_number, word.end_column),
                    )],
                    None,
                )
            })
            .rc(),
        ));
        fx.register(
            // JSON-based
            &TestProvider::new(|_doc, pos, _context| {
                list(
                    vec![text_suggestion(
                        "git.pull",
                        CompletionItemKind::Class,
                        "git.pull",
                        Range::new(pos.line_number, 1, pos.line_number, pos.column),
                    )],
                    None,
                )
            })
            .rc(),
        );

        let o = fx.with_oracle();

        settled(assert_event(
            o.model.on_did_suggest(),
            || {
                o.set_value("gi");
                o.set_selection(1, 3, 1, 3);
                o.type_text("t");
            },
            |event| {
                assert!(event.trigger_options.auto);
                assert_eq!(event.completion_model.items().len(), 1);
                assert_eq!(event.completion_model.items()[0].text_label, "git.pull");
                ok()
            },
        ))
        .await;

        o.type_text(".");

        settled(assert_event(
            o.model.on_did_suggest(),
            || {
                o.type_text("p");
            },
            |event| {
                assert!(event.trigger_options.auto);
                assert_eq!(event.completion_model.items().len(), 1);
                assert_eq!(event.completion_model.items()[0].text_label, "git.pull");
                ok()
            },
        ))
        .await;
    });
}

#[test]
fn completion_list_closes_unexpectedly_when_typing_a_digit_after_a_word_separator_169390() {
    run_test(|fx| async move {
        let request_counts = [Rc::new(Cell::new(0)), Rc::new(Cell::new(0))];

        {
            let count = Rc::clone(&request_counts[0]);
            fx.register(
                &TestProvider::new(move |_doc, pos, _context| {
                    count.set(count.get() + 1);
                    list(
                        vec![
                            text_suggestion(
                                "foo-20",
                                CompletionItemKind::Text,
                                "foo-20",
                                Range::new(pos.line_number, 1, pos.line_number, pos.column),
                            ),
                            text_suggestion(
                                "foo-hello",
                                CompletionItemKind::Text,
                                "foo-hello",
                                Range::new(pos.line_number, 1, pos.line_number, pos.column),
                            ),
                        ],
                        None,
                    )
                })
                .rc(),
            );
        }
        {
            let count = Rc::clone(&request_counts[1]);
            fx.register(
                &TestProvider::new(move |_doc, pos, ctx| {
                    count.set(count.get() + 1);
                    if ctx.trigger_kind != CompletionTriggerKind::TriggerCharacter {
                        return None;
                    }
                    list(
                        vec![text_suggestion(
                            "foo-210",
                            CompletionItemKind::Class,
                            "foo-210",
                            Range::new(pos.line_number, 1, pos.line_number, pos.column),
                        )],
                        None,
                    )
                })
                .with_trigger_characters(&["2"])
                .rc(),
            );
        }

        let o = fx.with_oracle();

        settled(assert_event(
            o.model.on_did_suggest(),
            || {
                o.set_value("foo");
                o.set_selection(1, 4, 1, 4);
                o.trigger(false);
            },
            |event| {
                assert!(!event.trigger_options.auto);
                assert_eq!(event.completion_model.items().len(), 2);
                assert_eq!(event.completion_model.items()[0].text_label, "foo-20");
                assert_eq!(event.completion_model.items()[1].text_label, "foo-hello");
                ok()
            },
        ))
        .await;

        o.type_text("-");

        settled(assert_event(
            o.model.on_did_suggest(),
            || {
                o.type_text("2");
            },
            move |event| {
                assert!(event.trigger_options.auto);
                assert_eq!(event.completion_model.items().len(), 2);
                assert_eq!(event.completion_model.items()[0].text_label, "foo-20");
                assert_eq!(event.completion_model.items()[1].text_label, "foo-210");
                assert_eq!([request_counts[0].get(), request_counts[1].get()], [1, 2]);
                ok()
            },
        ))
        .await;
    });
}

#[test]
fn set_refilter_flag_keep_trigger_kind() {
    run_test(|fx| async move {
        fx.register(
            &TestProvider::new(|doc, pos, _context| {
                let word = doc.get_word_until_position(pos).word.to_string_lossy();
                list(
                    vec![text_suggestion(
                        if word.is_empty() { "hello" } else { word.as_str() },
                        CompletionItemKind::Property,
                        "foofoo",
                        get_default_suggest_range(doc, pos),
                    )],
                    None,
                )
            })
            .with_trigger_characters(&["."])
            .rc(),
        );

        let o = fx.with_oracle();

        settled(assert_event(
            o.model.on_did_suggest(),
            || {
                o.set_value("foo");
                o.set_selection(1, 4, 1, 4);
                o.type_text("o");
            },
            |event| {
                assert!(event.trigger_options.auto);
                assert_eq!(event.trigger_options.trigger_character, None);
                assert_eq!(event.trigger_options.trigger_kind, None);
                assert_eq!(event.completion_model.items().len(), 1);
                ok()
            },
        ))
        .await;

        settled(assert_event(
            o.model.on_did_suggest(),
            || {
                o.type_text(".");
            },
            |event| {
                assert!(event.trigger_options.auto);
                assert_eq!(event.trigger_options.refilter, None);
                assert_eq!(event.trigger_options.trigger_character, Some(".".to_string()));
                assert_eq!(event.trigger_options.trigger_kind, Some(CompletionTriggerKind::TriggerCharacter));
                assert_eq!(event.completion_model.items().len(), 1);
                ok()
            },
        ))
        .await;

        settled(assert_event(
            o.model.on_did_suggest(),
            || {
                o.type_text("h");
            },
            |event| {
                assert!(event.trigger_options.auto);
                assert_eq!(event.trigger_options.refilter, Some(true));
                assert_eq!(event.trigger_options.trigger_character, Some(".".to_string()));
                assert_eq!(event.trigger_options.trigger_kind, Some(CompletionTriggerKind::TriggerCharacter));
                assert_eq!(event.completion_model.items().len(), 1);
                ok()
            },
        ))
        .await;
    });
}

#[test]
fn snippets_gone_from_intellisense_173244() {
    run_test(|fx| async move {
        let snippet_provider = TestProvider::new(|doc, pos, _context| {
            list(
                vec![text_suggestion("log", CompletionItemKind::Snippet, "log", get_default_suggest_range(doc, pos))],
                None,
            )
        })
        .rc();
        let old = set_snippet_suggest_support(Some(Rc::clone(&snippet_provider)));

        {
            let snippet_provider = Rc::clone(&snippet_provider);
            fx.disposables.add(to_disposable(move || {
                if get_snippet_suggest_support().is_some_and(|current| provider_id(&current) == provider_id(&snippet_provider)) {
                    set_snippet_suggest_support(old);
                }
            }));
        }

        fx.register(
            &TestProvider::new(|doc, pos, _context| {
                list(
                    vec![text_suggestion("locals", CompletionItemKind::Property, "locals", get_default_suggest_range(doc, pos))],
                    Some(true),
                )
            })
            .with_trigger_characters(&["."])
            .rc(),
        );

        let o = fx.with_oracle();

        settled(assert_event(
            o.model.on_did_suggest(),
            || {
                o.set_value("");
                o.set_selection(1, 1, 1, 1);
                o.type_text("l");
            },
            |event| {
                assert!(event.trigger_options.auto);
                assert_eq!(event.trigger_options.trigger_character, None);
                assert_eq!(event.trigger_options.trigger_kind, None);
                assert_eq!(event.completion_model.items().len(), 2);
                assert_eq!(event.completion_model.items()[0].text_label, "locals");
                assert_eq!(event.completion_model.items()[1].text_label, "log");
                ok()
            },
        ))
        .await;

        settled(assert_event(
            o.model.on_did_suggest(),
            || {
                o.type_text("o");
            },
            |event| {
                assert_eq!(event.trigger_options.trigger_kind, Some(CompletionTriggerKind::TriggerForIncompleteCompletions));
                assert!(event.trigger_options.auto);
                assert_eq!(event.completion_model.items().len(), 2);
                assert_eq!(event.completion_model.items()[0].text_label, "locals");
                assert_eq!(event.completion_model.items()[1].text_label, "log");
                ok()
            },
        ))
        .await;
    });
}

#[test]
fn off_when_inline_completions_allows_quick_suggest_when_inline_provider_returns_empty_results() {
    run_test(|fx| async move {
        fx.register(&always_something_support());

        // Register a dummy inline completions provider that returns no items
        fx.register_inline();

        let o = fx.with_oracle();
        o.update_quick_suggestions_object("off", "off", "offWhenInlineCompletions");

        // Without an InlineCompletionsController, the fallback triggers immediately
        settled(assert_event(
            o.model.on_did_suggest(),
            || {
                o.set_position(1, 4);
                o.type_text("d");
            },
            |suggest_event| {
                assert!(suggest_event.trigger_options.auto);
                ok()
            },
        ))
        .await;
    });
}

#[test]
fn off_when_inline_completions_allows_quick_suggest_when_no_inline_provider_exists() {
    run_test(|fx| async move {
        fx.register(&always_something_support());

        // No inline completions provider registered for 'test' scheme

        let o = fx.with_oracle();
        o.update_quick_suggestions_object("off", "off", "offWhenInlineCompletions");

        settled(assert_event(
            o.model.on_did_suggest(),
            || {
                o.set_position(1, 4);
                o.type_text("d");
            },
            |suggest_event| {
                assert!(suggest_event.trigger_options.auto);
                assert_eq!(suggest_event.completion_model.items().len(), 1);
                ok()
            },
        ))
        .await;
    });
}

#[test]
fn off_when_inline_completions_allows_quick_suggest_when_inline_suggest_is_disabled() {
    run_test(|fx| async move {
        fx.register(&always_something_support());

        // Register a dummy inline completions provider
        fx.register_inline();

        let o = fx.with_oracle();
        o.editor.update_options(Some(&Dynamic::object([
            ("quickSuggestions", quick_suggestions_object("off", "off", "offWhenInlineCompletions")),
            ("inlineSuggest", Dynamic::object([("enabled", Dynamic::Bool(false))])),
        ])));

        settled(assert_event(
            o.model.on_did_suggest(),
            || {
                o.set_position(1, 4);
                o.type_text("d");
            },
            |suggest_event| {
                assert!(suggest_event.trigger_options.auto);
                assert_eq!(suggest_event.completion_model.items().len(), 1);
                ok()
            },
        ))
        .await;
    });
}

#[test]
fn string_shorthand_off_disables_quick_suggestions_for_all_token_types() {
    run_test(|fx| async move {
        fx.register(&always_something_support());

        let o = fx.with_oracle();
        // Use string shorthand instead of object form
        o.editor.update_options(Some(&Dynamic::object([("quickSuggestions", Dynamic::string("off"))])));

        let (promise, resolver) = Promise::<()>::new();
        let sub: Rc<RefCell<Option<DisposableRef>>> = Rc::new(RefCell::new(None));
        {
            let (sub_in_listener, resolver) = (Rc::clone(&sub), resolver.clone());
            let listener = o.model.on_did_suggest().listen(move |_| {
                let listener = sub_in_listener.borrow_mut().take();
                if let Some(listener) = listener {
                    listener.dispose();
                }
                resolver.reject(Error::new("Quick suggestions should have been suppressed by string shorthand \"off\""));
            });
            *sub.borrow_mut() = Some(listener);
        }

        o.set_position(1, 4);
        o.type_text("d");

        runtime::set_timeout(
            move || {
                let listener = sub.borrow_mut().take();
                if let Some(listener) = listener {
                    listener.dispose();
                }
                resolver.resolve(());
            },
            200,
        );

        promise.await.expect("Quick suggestions should have been suppressed");
    });
}

#[test]
fn string_shorthand_off_when_inline_completions_allows_quick_suggest_when_inline_provider_returns_empty() {
    run_test(|fx| async move {
        fx.register(&always_something_support());

        fx.register_inline();

        let o = fx.with_oracle();
        // Use string shorthand - applies to all token types
        o.editor.update_options(Some(&Dynamic::object([("quickSuggestions", Dynamic::string("offWhenInlineCompletions"))])));

        // Without InlineCompletionsController, the fallback triggers immediately
        settled(assert_event(
            o.model.on_did_suggest(),
            || {
                o.set_position(1, 4);
                o.type_text("d");
            },
            |suggest_event| {
                assert!(suggest_event.trigger_options.auto);
                ok()
            },
        ))
        .await;
    });
}
