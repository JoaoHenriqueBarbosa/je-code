//! Porte de `src/vs/editor/contrib/semanticTokens/test/browser/documentSemanticTokens.test.ts` (suíte
//! `ModelSemanticColoring`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()`, o `setup` e o `teardown` (`disposables.clear()`) são o `LeakGuard` e
//!   [`setup`] de [`run_test`], que roda o corpo com o relógio virtual (`runWithFakedTimers`, também nos casos que o
//!   original escreve síncronos, porque o `ModelSemanticColoring` agenda timers). O `disposables` da suíte é o próprio
//!   guarda; o `disposables.clear()` do último caso é `ds.store().clear()`.
//! - O `setup` do original monta `TestInstantiationService` com `ILanguageConfigurationService` e
//!   `ITreeSitterLibraryService` só para o `ModelService`; o `ModelService` do porte não usa injeção e recebe a
//!   configuração, o `TestTextResourcePropertiesService` e o `UndoRedoService` de teste (ver `marker_decorations_service`).
//!   `new NullLogService()` é `NullLogService::shared()`, `mock<IEnvironmentService>()` é [`MockEnvironmentService`] e o
//!   `IEnvironmentService` real do `LanguageFeatureDebounceService` é o que ele lê: `isBuilt` e `isExtensionDevelopment`.
//! - `TestColorTheme` e `TestThemeService` do original (com `semanticHighlighting`, `getTokenStyleMetadata` e o emissor
//!   `_onThemeChange` trocável) são [`SemanticTestTheme`] e [`TestThemeService`] daqui: o `IColorTheme` do porte não tem
//!   `type` (o `ColorScheme.DARK` e `ColorScheme.LIGHT` do original não são lidos pela contribuição e somem). O
//!   `TestThemeChangeEmitter` (`listenerCount` é `_size`) é o `Emitter` com `size_for_testing`. O subtipo anônimo de
//!   `TestColorTheme` que sobrescreve `getTokenStyleMetadata` é o `SemanticTestTheme` com `foreground`.
//! - `languageFeaturesService.documentSemanticTokensProvider.register('testMode', provider)` é o registro do
//!   `LanguageFeaturesService`; o provedor literal ou a classe com `provideDocumentSemanticTokens` e
//!   `releaseDocumentSemanticTokens` é o [`TestProvider`] (com os fechos de cada caso). `async provide...` é um
//!   `async_fn` quando espera algo e uma promessa já resolvida quando não.
//! - `assert.deepStrictEqual` é `assert_eq!` sobre tuplas; `string | null` e `string | undefined` são `Option<String>`.
//! - O `sinon.spy(secondModel, 'onDidChangeContent')` do terceiro caso não tem equivalente: o que ele prova (a contribuição
//!   não registra um `ModelSemanticColoring` para o modelo que um ouvinte de tokens descartou no meio da troca de tema) é
//!   conferido por `watcher_count_for_testing`, que só conta o modelo ainda vivo.
//! - `setUnexpectedErrorHandler(handler)` e a restauração com o `originalErrorHandler` do quarto caso são
//!   `set_unexpected_error_handler` e `record_unexpected_errors` (o tratador do guarda); `errorHandler.getUnexpectedErrorHandler()`
//!   não existe no porte.
//! - `Event.toPromise(event)` é `event.to_promise(None)`; `textModel.applyEdits([{ range, text }])` é `apply_edits` com
//!   `EditOperation::replace`; `new Uint32Array([...])` é `Vec<u32>`; `toArr` some, porque o `data` já é um `Vec`.
//! - `assert.fail('Unexpected call')` dentro do provedor é a rejeição da promessa com um `Error`, que a contribuição
//!   reporta como erro inesperado (o guarda reprova o caso).
//! - No último caso `result.tokens.resultId` ausente é `result_id: None`, e o `finally` (`disposables.clear()`, `timeout(0)`,
//!   `textModel.dispose()`) vem depois das conferências.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod support;

use std::cell::{Cell, RefCell};
use std::future::Future;
use std::rc::Rc;

use common::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite, record_unexpected_errors, run_with_faked_timers, timeout};
use je_base::JsString;
use je_base::async_::{Barrier, async_fn};
use je_base::cancellation::CancellationToken;
use je_base::dynamic::Dynamic;
use je_base::errors::{self, Error};
use je_base::event::{Emitter, Event};
use je_base::promise::Promise;
use je_editor::edit_operation::EditOperation;
use je_editor::language_feature_debounce::{ILanguageFeatureDebounceService, LanguageFeatureDebounceService};
use je_editor::language_features_service::{
    DocumentSemanticTokensProvider, LanguageFeaturesService, SemanticTokens, SemanticTokensEdit, SemanticTokensEdits, SemanticTokensLegend,
    SemanticTokensResult,
};
use je_editor::language_selector::LanguageSelector;
use je_editor::language_service::{ILanguageService, LanguageService};
use je_editor::line_tokens::IViewLineTokens;
use je_editor::model_service::ModelService;
use je_editor::modes_registry::LanguageExtensionPoint;
use je_editor::range::Range;
use je_editor::semantic_tokens::document_semantic_tokens::DocumentSemanticTokensFeature;
use je_editor::semantic_tokens::get_semantic_tokens::{get_document_semantic_tokens, is_semantic_tokens};
use je_editor::semantic_tokens_styling::ISemanticTokensStylingService;
use je_editor::semantic_tokens_styling_service::SemanticTokensStylingService;
use je_editor::services::theme_service::{IColorTheme, ITokenStyle, IThemeService};
use je_editor::services_model::IModelService;
use je_editor::text_model::TextModel;
use je_editor::undo_redo_service::UndoRedoService;
use je_platform::configuration::service::IConfigurationService;
use je_platform::configuration::test_configuration_service::TestConfigurationService;
use je_platform::log::NullLogService;
use je_theme::color::Color;
use support::mock_environment_service::MockEnvironmentService;
use support::test_text_resource_properties_service::TestTextResourcePropertiesService;

/// `new TestColorTheme({}, scheme, semanticHighlighting)`, com o `getTokenStyleMetadata` que devolve só o `foreground`
/// quando o caso o dá (o subtipo anônimo do segundo caso) e `undefined` quando não.
struct SemanticTestTheme {
    semantic_highlighting: bool,
    foreground: Option<u32>,
}

impl SemanticTestTheme {
    fn new(semantic_highlighting: bool) -> Rc<dyn IColorTheme> {
        Rc::new(Self { semantic_highlighting, foreground: None })
    }

    fn with_foreground(foreground: u32) -> Rc<dyn IColorTheme> {
        Rc::new(Self { semantic_highlighting: true, foreground: Some(foreground) })
    }
}

impl IColorTheme for SemanticTestTheme {
    fn get_color(&self, _color: &str, _use_default: Option<bool>) -> Option<Color> {
        None
    }

    fn semantic_highlighting(&self) -> bool {
        self.semantic_highlighting
    }

    fn get_token_style_metadata(&self, _token_type: &str, _modifiers: &[String], _model_language: &str) -> Option<ITokenStyle> {
        self.foreground.map(|foreground| ITokenStyle { foreground: Some(foreground), bold: None, underline: None, strikethrough: None, italic: None })
    }
}

/// `TestThemeService` com o `_onThemeChange` trocado pelo `TestThemeChangeEmitter`.
struct TestThemeService {
    color_theme: RefCell<Rc<dyn IColorTheme>>,
    on_theme_change: Rc<Emitter<Rc<dyn IColorTheme>>>,
}

impl TestThemeService {
    fn new(on_theme_change: Rc<Emitter<Rc<dyn IColorTheme>>>) -> Rc<Self> {
        Rc::new(Self { color_theme: RefCell::new(SemanticTestTheme::new(false)), on_theme_change })
    }

    /// `setTheme(theme)`.
    fn set_theme(&self, theme: Rc<dyn IColorTheme>) {
        *self.color_theme.borrow_mut() = Rc::clone(&theme);
        self.on_theme_change.fire(theme);
    }

    /// `TestThemeChangeEmitter.listenerCount`.
    fn listener_count(&self) -> usize {
        self.on_theme_change.size_for_testing()
    }
}

impl IThemeService for TestThemeService {
    fn get_color_theme(&self) -> Rc<dyn IColorTheme> {
        Rc::clone(&self.color_theme.borrow())
    }

    fn on_did_color_theme_change(&self) -> Event<Rc<dyn IColorTheme>> {
        self.on_theme_change.event()
    }
}

type ProvideFn = Box<dyn Fn(&Rc<TextModel>, Option<&str>) -> Promise<Option<SemanticTokensResult>>>;

/// O provedor de tokens semânticos de cada caso (o literal ou a classe anônima do original).
struct TestProvider {
    legend: SemanticTokensLegend,
    on_did_change: Option<Event<()>>,
    provide: ProvideFn,
    release: Box<dyn Fn(Option<&str>)>,
}

impl TestProvider {
    /// `getLegend: () => ({ tokenTypes: ['class'], tokenModifiers: [] })`.
    fn class(provide: impl Fn(&Rc<TextModel>, Option<&str>) -> Promise<Option<SemanticTokensResult>> + 'static, release: impl Fn(Option<&str>) + 'static) -> Self {
        Self::with_legend(&["class"], provide, release)
    }

    fn with_legend(
        token_types: &[&str],
        provide: impl Fn(&Rc<TextModel>, Option<&str>) -> Promise<Option<SemanticTokensResult>> + 'static,
        release: impl Fn(Option<&str>) + 'static,
    ) -> Self {
        Self {
            legend: SemanticTokensLegend { token_types: token_types.iter().map(|token_type| token_type.to_string()).collect(), token_modifiers: Vec::new() },
            on_did_change: None,
            provide: Box::new(provide),
            release: Box::new(release),
        }
    }

    /// `onDidChange = emitter.event`.
    fn with_on_did_change(mut self, event: Event<()>) -> Self {
        self.on_did_change = Some(event);
        self
    }
}

impl DocumentSemanticTokensProvider for TestProvider {
    fn on_did_change(&self) -> Option<Event<()>> {
        self.on_did_change.clone()
    }

    fn get_legend(&self) -> SemanticTokensLegend {
        self.legend.clone()
    }

    fn provide_document_semantic_tokens(&self, model: &Rc<TextModel>, last_result_id: Option<&str>, _token: &CancellationToken) -> Promise<Option<SemanticTokensResult>> {
        (self.provide)(model, last_result_id)
    }

    fn release_document_semantic_tokens(&self, result_id: Option<&str>) {
        (self.release)(result_id);
    }
}

/// `{ resultId, data: new Uint32Array(data) }`.
fn full(result_id: Option<&str>, data: &[u32]) -> Option<SemanticTokensResult> {
    Some(SemanticTokensResult::Tokens(SemanticTokens { result_id: result_id.map(str::to_owned), data: data.to_vec() }))
}

/// `{ resultId, edits }`.
fn edits(result_id: &str, edits: Vec<SemanticTokensEdit>) -> Option<SemanticTokensResult> {
    Some(SemanticTokensResult::Edits(SemanticTokensEdits { result_id: Some(result_id.to_owned()), edits }))
}

struct Fixture {
    model_service: Rc<ModelService>,
    language_service: Rc<LanguageService>,
    language_features_service: LanguageFeaturesService,
    theme_service: Rc<TestThemeService>,
    feature: Rc<DocumentSemanticTokensFeature>,
}

/// O `setup` da suíte.
fn setup(ds: &LeakGuard) -> Fixture {
    let config_service = TestConfigurationService::new(Some(Dynamic::object([("editor", Dynamic::object([("semanticHighlighting", Dynamic::Bool(true))]))])));
    let configuration_service: Rc<dyn IConfigurationService> = config_service;
    let theme_change_emitter = ds.add_rc(Emitter::new());
    let theme_service = TestThemeService::new(theme_change_emitter);
    theme_service.set_theme(SemanticTestTheme::new(true));
    let log_service = NullLogService::shared();
    let language_features_service = LanguageFeaturesService::new();
    let language_service = ds.add_rc(LanguageService::new(false));
    let semantic_tokens_styling_service: Rc<dyn ISemanticTokensStylingService> = ds.add_rc(SemanticTokensStylingService::new(
        theme_service.clone() as Rc<dyn IThemeService>,
        Rc::clone(&log_service),
        language_service.clone() as Rc<dyn ILanguageService>,
    ));
    let model_service = ds.add_rc(ModelService::new(
        Rc::clone(&configuration_service),
        TestTextResourcePropertiesService::new(Rc::clone(&configuration_service)),
        UndoRedoService::for_tests(),
    ));
    let env_service = MockEnvironmentService::new(true, false);
    let feature = ds.add_rc(DocumentSemanticTokensFeature::new(
        semantic_tokens_styling_service,
        model_service.clone() as Rc<dyn IModelService>,
        theme_service.clone() as Rc<dyn IThemeService>,
        configuration_service,
        Rc::new(LanguageFeatureDebounceService::new(log_service, &*env_service)) as Rc<dyn ILanguageFeatureDebounceService>,
        &language_features_service,
    ));
    Fixture { model_service, language_service, language_features_service, theme_service, feature }
}

impl Fixture {
    /// `disposables.add(languageService.registerLanguage({ id }))`.
    fn register_language(&self, ds: &LeakGuard, id: &str) {
        ds.add(self.language_service.register_language(LanguageExtensionPoint::new(id)));
    }

    /// `disposables.add(languageFeaturesService.documentSemanticTokensProvider.register(language, provider))`.
    fn register_provider(&self, ds: &LeakGuard, language: &str, provider: TestProvider) {
        let provider: Rc<dyn DocumentSemanticTokensProvider> = Rc::new(provider);
        ds.add(self.language_features_service.document_semantic_tokens_provider.register(LanguageSelector::from(language), provider));
    }

    /// `modelService.createModel(text, language ? languageService.createById(language) : null)`, sem registrar o descarte.
    fn create_model_raw(&self, text: &str, language: Option<&str>) -> Rc<TextModel> {
        let selection = language.map(|language| self.language_service.create_by_id(Some(language)));
        self.model_service.create_model(text, selection.as_deref(), None, false)
    }

    /// `disposables.add(modelService.createModel(text, language))`.
    fn create_model(&self, ds: &LeakGuard, text: &str, language: Option<&str>) -> Rc<TextModel> {
        ds.add_rc(self.create_model_raw(text, language))
    }
}

/// O que o mocha faz em volta de cada `test(...)` da suíte: o `ensureNoDisposablesAreLeakedInTestSuite()`, o `setup`, o corpo
/// (com o relógio virtual) e o `teardown`, nessa ordem.
fn run_test<F: Future<Output = ()> + 'static>(body: impl FnOnce(Rc<LeakGuard>, Rc<Fixture>) -> F) {
    let ds = Rc::new(ensure_no_disposables_are_leaked_in_test_suite());
    let fixture = Rc::new(setup(&ds));
    run_with_faked_timers(body(Rc::clone(&ds), Rc::clone(&fixture)));
}

fn insert_at_start(model: &TextModel, text: &str) {
    model.apply_edits(&[EditOperation::replace(Range::new(1, 1, 1, 1), Some(JsString::from(text)))]);
}

#[test]
fn theme_listeners_do_not_grow_with_semantic_coloring_models() {
    run_test(|ds, f| async move {
        let initial_count = f.theme_service.listener_count();
        let models: Vec<Rc<TextModel>> = (0..20).map(|_| f.create_model(&ds, "Hello world", None)).collect();
        let after_adding_models = f.theme_service.listener_count();

        f.theme_service.set_theme(SemanticTestTheme::new(false));
        let after_disabling = f.theme_service.listener_count();
        f.theme_service.set_theme(SemanticTestTheme::new(true));
        let after_enabling = f.theme_service.listener_count();

        for model in &models {
            model.dispose();
        }

        assert_eq!(
            (after_adding_models, after_disabling, after_enabling, f.theme_service.listener_count()),
            (initial_count, initial_count, initial_count, initial_count)
        );
    });
}

#[test]
fn theme_changes_clear_semantic_tokens_and_refetch_with_the_new_styling() {
    run_test(|ds, f| async move {
        f.theme_service.set_theme(SemanticTestTheme::with_foreground(1));
        f.register_language(&ds, "testMode");

        let last_result_ids: Rc<RefCell<Vec<Option<String>>>> = Rc::default();
        let released_result_ids: Rc<RefCell<Vec<Option<String>>>> = Rc::default();
        let provided_ids = Rc::clone(&last_result_ids);
        let released_ids = Rc::clone(&released_result_ids);
        f.register_provider(
            &ds,
            "testMode",
            TestProvider::class(
                move |_model, last_result_id| {
                    provided_ids.borrow_mut().push(last_result_id.map(str::to_owned));
                    let result_id = provided_ids.borrow().len().to_string();
                    Promise::resolved(full(Some(&result_id), &[0, 0, 5, 0, 0]))
                },
                move |result_id| released_ids.borrow_mut().push(result_id.map(str::to_owned)),
            ),
        );
        let model = f.create_model(&ds, "Hello world", Some("testMode"));
        let _attached = model.on_before_attached();
        timeout(1000).await.unwrap();
        let initial_foreground = model.tokenization().get_line_tokens(1).get_foreground(0);

        f.theme_service.set_theme(SemanticTestTheme::with_foreground(2));
        let cleared_immediately = !model.tokenization().has_complete_semantic_tokens();
        timeout(1000).await.unwrap();

        assert_eq!(
            (
                initial_foreground,
                cleared_immediately,
                model.tokenization().get_line_tokens(1).get_foreground(0),
                last_result_ids.borrow().clone(),
                released_result_ids.borrow().clone(),
            ),
            (1, true, 2, vec![None, None], vec![Some("1".to_owned())])
        );
    });
}

#[test]
fn theme_changes_do_not_register_models_disposed_by_token_listeners() {
    run_test(|ds, f| async move {
        let first_model = f.create_model(&ds, "First model", None);
        let second_model = f.create_model(&ds, "Second model", None);
        let disposed_model = Rc::clone(&second_model);
        ds.add(first_model.on_did_change_tokens().listen(move |_| disposed_model.dispose()));

        f.theme_service.set_theme(SemanticTestTheme::new(true));

        assert!(second_model.is_disposed());
        assert_eq!(f.feature.watcher_count_for_testing(), 1);
    });
}

#[test]
fn theme_changes_continue_refreshing_models_after_a_provider_release_error() {
    run_test(|ds, f| async move {
        f.register_language(&ds, "testMode");
        let requests: Rc<RefCell<Vec<String>>> = Rc::default();
        let reported_errors: Rc<RefCell<Vec<Error>>> = Rc::default();
        let release_error = Error::new("Failed to release semantic tokens");
        let throw_for_result_id: Rc<RefCell<Option<String>>> = Rc::default();
        let requested = Rc::clone(&requests);
        let throw_for = Rc::clone(&throw_for_result_id);
        let thrown = release_error.clone();
        f.register_provider(
            &ds,
            "testMode",
            TestProvider::class(
                move |model, _last_result_id| {
                    requested.borrow_mut().push(model.id().to_owned());
                    Promise::resolved(full(Some(model.id()), &[0, 0, 5, 0, 0]))
                },
                move |result_id| {
                    if throw_for.borrow().as_deref() == result_id {
                        *throw_for.borrow_mut() = None;
                        errors::throw(thrown.clone());
                    }
                },
            ),
        );
        let first_model = f.create_model(&ds, "First model", Some("testMode"));
        let second_model = f.create_model(&ds, "Second model", Some("testMode"));
        let _first_attached = first_model.on_before_attached();
        let _second_attached = second_model.on_before_attached();
        timeout(1000).await.unwrap();

        let reported = Rc::clone(&reported_errors);
        errors::set_unexpected_error_handler(move |error| reported.borrow_mut().push(error.clone()));
        *throw_for_result_id.borrow_mut() = Some(first_model.id().to_owned());
        f.theme_service.set_theme(SemanticTestTheme::new(true));
        timeout(1000).await.unwrap();

        let result = (requests.borrow().clone(), reported_errors.borrow().clone());
        record_unexpected_errors();
        assert_eq!(result, (vec![first_model.id().to_owned(), second_model.id().to_owned(), second_model.id().to_owned()], vec![release_error]));
    });
}

#[test]
fn enabling_semantic_coloring_through_a_theme_immediately_fetches_only_attached_models() {
    run_test(|ds, f| async move {
        f.theme_service.set_theme(SemanticTestTheme::new(false));
        f.register_language(&ds, "testMode");

        let requested_model_ids: Rc<RefCell<Vec<String>>> = Rc::default();
        let requested = Rc::clone(&requested_model_ids);
        f.register_provider(
            &ds,
            "testMode",
            TestProvider::class(
                move |model, _last_result_id| {
                    requested.borrow_mut().push(model.id().to_owned());
                    Promise::resolved(full(None, &[0, 0, 5, 0, 0]))
                },
                |_result_id| {},
            ),
        );
        let visible_model = f.create_model(&ds, "Hello world", Some("testMode"));
        f.create_model(&ds, "Hidden model", Some("testMode"));
        let _attached = visible_model.on_before_attached();

        f.theme_service.set_theme(SemanticTestTheme::new(true));
        timeout(1).await.unwrap();
        let immediately_requested = requested_model_ids.borrow().len();
        timeout(1000).await.unwrap();

        assert_eq!((immediately_requested, requested_model_ids.borrow().clone()), (1, vec![visible_model.id().to_owned()]));
    });
}

#[test]
fn document_semantic_tokens_should_be_fetched_when_the_result_is_empty_if_there_are_pending_changes() {
    run_test(|ds, f| async move {
        f.register_language(&ds, "testMode");

        let in_first_call = Barrier::new();
        let delay_first_result = Barrier::new();
        let second_result_provided = Barrier::new();
        let call_count = Rc::new(Cell::new(0));

        let (in_first_call_provider, delay_first_result_provider, second_result_provider, calls) =
            (in_first_call.clone(), delay_first_result.clone(), second_result_provided.clone(), Rc::clone(&call_count));
        f.register_provider(
            &ds,
            "testMode",
            TestProvider::class(
                move |_model, _last_result_id| {
                    calls.set(calls.get() + 1);
                    let call = calls.get();
                    let (in_first_call, delay_first_result, second_result_provided) =
                        (in_first_call_provider.clone(), delay_first_result_provider.clone(), second_result_provider.clone());
                    async_fn(async move {
                        if call == 1 {
                            in_first_call.open();
                            delay_first_result.wait().await?;
                            timeout(0).await?; // wait for the simple scheduler to fire to check that we do actually get rescheduled
                            return Ok(None);
                        }
                        if call == 2 {
                            second_result_provided.open();
                            return Ok(None);
                        }
                        Err(Error::new("Unexpected call"))
                    })
                },
                |_result_id| {},
            ),
        );

        let text_model = f.create_model(&ds, "Hello world", Some("testMode"));
        // pretend the text model is attached to an editor (so that semantic tokens are computed)
        let _attached = text_model.on_before_attached();

        // wait for the provider to be called
        in_first_call.wait().await.unwrap();

        // the provider is now in the provide call
        // change the text buffer while the provider is running
        insert_at_start(&text_model, "x");

        // let the provider finish its first result
        delay_first_result.open();

        // we need to check that the provider is called again, even if it returns null
        second_result_provided.wait().await.unwrap();

        // assert that it got called twice
        assert_eq!(call_count.get(), 2);
    });
}

#[test]
fn issue_149412_vs_code_hangs_when_bad_semantic_token_data_is_received() {
    run_test(|ds, f| async move {
        f.register_language(&ds, "testMode");

        let last_result: Rc<RefCell<Option<SemanticTokensResult>>> = Rc::default();
        let provided = Rc::clone(&last_result);
        f.register_provider(
            &ds,
            "testMode",
            TestProvider::class(
                move |_model, last_result_id| {
                    let result = if last_result_id.is_none() {
                        // this is the first call
                        full(Some("1"), &[4294967293, 0, 7, 16, 0, 1, 4, 3, 11, 1])
                    } else {
                        // this is the second call
                        edits("2", vec![SemanticTokensEdit { start: 4294967276, delete_count: 0, data: Some(vec![2, 0, 3, 11, 0]) }])
                    };
                    *provided.borrow_mut() = result.clone();
                    Promise::resolved(result)
                },
                |_result_id| {},
            ),
        );

        let text_model = f.create_model(&ds, "", Some("testMode"));
        // pretend the text model is attached to an editor (so that semantic tokens are computed)
        let _attached = text_model.on_before_attached();

        // wait for the semantic tokens to be fetched
        text_model.on_did_change_tokens().to_promise(None).await.unwrap();
        assert_eq!(result_id_of(&last_result), Some("1".to_owned()));

        // edit the text
        insert_at_start(&text_model, "foo");

        // wait for the semantic tokens to be fetched again
        text_model.on_did_change_tokens().to_promise(None).await.unwrap();
        assert_eq!(result_id_of(&last_result), Some("2".to_owned()));
    });
}

/// `lastResult!.resultId`.
fn result_id_of(result: &Rc<RefCell<Option<SemanticTokensResult>>>) -> Option<String> {
    match result.borrow().as_ref().expect("o provedor já foi chamado") {
        SemanticTokensResult::Tokens(tokens) => tokens.result_id.clone(),
        SemanticTokensResult::Edits(edits) => edits.result_id.clone(),
    }
}

#[test]
fn issue_322571_semantic_token_edits_cannot_delete_more_data_than_the_previous_result_contains() {
    run_test(|ds, f| async move {
        f.register_language(&ds, "testMode");

        let last_result_ids: Rc<RefCell<Vec<Option<String>>>> = Rc::default();
        let released_result_ids: Rc<RefCell<Vec<Option<String>>>> = Rc::default();
        let recovered_result_provided = Barrier::new();

        let provided_ids = Rc::clone(&last_result_ids);
        let released_ids = Rc::clone(&released_result_ids);
        let recovered = recovered_result_provided.clone();
        f.register_provider(
            &ds,
            "testMode",
            TestProvider::class(
                move |_model, last_result_id| {
                    provided_ids.borrow_mut().push(last_result_id.map(str::to_owned));
                    let call = provided_ids.borrow().len();
                    match call {
                        1 => Promise::resolved(full(Some("full"), &[0, 0, 1, 0, 0])),
                        2 => Promise::resolved(edits("invalid-edit", vec![SemanticTokensEdit { start: 0, delete_count: 6, data: None }])),
                        3 => {
                            recovered.open();
                            Promise::resolved(full(Some("recovered-full"), &[0, 0, 1, 0, 0]))
                        }
                        _ => Promise::rejected(Error::new("Unexpected call")),
                    }
                },
                move |result_id| released_ids.borrow_mut().push(result_id.map(str::to_owned)),
            ),
        );

        let text_model = f.create_model(&ds, "a", Some("testMode"));
        let _attached = text_model.on_before_attached();

        text_model.on_did_change_tokens().to_promise(None).await.unwrap();
        insert_at_start(&text_model, "b");
        recovered_result_provided.wait().await.unwrap();

        assert_eq!(
            (last_result_ids.borrow().clone(), released_result_ids.borrow().clone()),
            (
                vec![None, Some("full".to_owned()), None],
                vec![Some("full".to_owned()), Some("invalid-edit".to_owned())]
            )
        );
    });
}

#[test]
fn invalid_semantic_token_edits_without_a_previous_result_are_not_retried() {
    run_test(|ds, f| async move {
        f.register_language(&ds, "testMode");

        let request_count = Rc::new(Cell::new(0));
        let released_result_ids: Rc<RefCell<Vec<Option<String>>>> = Rc::default();
        let result_provided = Barrier::new();

        let requests = Rc::clone(&request_count);
        let released_ids = Rc::clone(&released_result_ids);
        let provided = result_provided.clone();
        f.register_provider(
            &ds,
            "testMode",
            TestProvider::class(
                move |_model, _last_result_id| {
                    requests.set(requests.get() + 1);
                    provided.open();
                    Promise::resolved(edits("invalid-edit", vec![SemanticTokensEdit { start: 0, delete_count: 1, data: None }]))
                },
                move |result_id| released_ids.borrow_mut().push(result_id.map(str::to_owned)),
            ),
        );

        let text_model = f.create_model(&ds, "a", Some("testMode"));
        let _attached = text_model.on_before_attached();

        result_provided.wait().await.unwrap();
        timeout(1000).await.unwrap();

        assert_eq!((request_count.get(), released_result_ids.borrow().clone()), (1, vec![Some("invalid-edit".to_owned())]));
    });
}

#[test]
fn issue_161573_on_did_change_semantic_tokens_doesnt_consistently_trigger_provide_document_semantic_tokens() {
    run_test(|ds, f| async move {
        f.register_language(&ds, "testMode");

        let emitter: Rc<Emitter<()>> = ds.add_rc(Emitter::new());
        let request_count = Rc::new(Cell::new(0));
        let requests = Rc::clone(&request_count);
        let change_emitter = Rc::clone(&emitter);
        f.register_provider(
            &ds,
            "testMode",
            TestProvider::class(
                move |_model, _last_result_id| {
                    requests.set(requests.get() + 1);
                    if requests.get() == 1 {
                        let change_emitter = Rc::clone(&change_emitter);
                        return async_fn(async move {
                            timeout(1000).await?;
                            // send a change event
                            change_emitter.fire(());
                            timeout(1000).await?;
                            Ok(None)
                        });
                    }
                    Promise::resolved(None)
                },
                |_result_id| {},
            )
            .with_on_did_change(emitter.event()),
        );

        let text_model = f.create_model(&ds, "", Some("testMode"));
        // pretend the text model is attached to an editor (so that semantic tokens are computed)
        let _attached = text_model.on_before_attached();

        timeout(5000).await.unwrap();
        assert_eq!(request_count.get(), 2);
    });
}

#[test]
fn document_semantic_tokens_should_be_pick_the_token_provider_with_actual_items() {
    run_test(|ds, f| async move {
        let call_count = Rc::new(Cell::new(0));
        f.register_language(&ds, "testMode2");

        let calls = Rc::clone(&call_count);
        f.register_provider(
            &ds,
            "testMode2",
            TestProvider::with_legend(
                &["class1"],
                move |_model, last_result_id| {
                    calls.set(calls.get() + 1);
                    // For a secondary request return a different value
                    if last_result_id.is_some() {
                        return Promise::resolved(full(None, &[2, 1, 1, 1, 1, 0, 2, 1, 1, 1]));
                    }
                    Promise::resolved(full(Some("1"), &[0, 1, 1, 1, 1, 0, 2, 1, 1, 1]))
                },
                |_result_id| {},
            ),
        );
        let calls = Rc::clone(&call_count);
        f.register_provider(
            &ds,
            "testMode2",
            TestProvider::with_legend(
                &["class2"],
                move |_model, _last_result_id| {
                    calls.set(calls.get() + 1);
                    Promise::resolved(None)
                },
                |_result_id| {},
            ),
        );

        let text_model = f.create_model_raw("Hello world 2", Some("testMode2"));
        let registry = Rc::clone(&f.language_features_service.document_semantic_tokens_provider);
        let class1_legend = SemanticTokensLegend { token_types: vec!["class1".to_owned()], token_modifiers: Vec::new() };

        let result = get_document_semantic_tokens(&registry, &text_model, None, None, &CancellationToken::None).await.unwrap();
        let result = result.expect("We should have tokens (1)");
        let tokens = result.tokens.clone().expect("Tokens are found from multiple providers (1)");
        assert!(is_semantic_tokens(&tokens), "Tokens are full (1)");
        let SemanticTokensResult::Tokens(tokens) = tokens else { unreachable!("Tokens are full (1)") };
        assert!(tokens.result_id.is_some(), "Token result id found from multiple providers (1)");
        assert_eq!(tokens.data, vec![0, 1, 1, 1, 1, 0, 2, 1, 1, 1], "Token data returned for multiple providers (1)");
        assert_eq!(call_count.get(), 2, "Called both token providers (1)");
        assert_eq!(result.provider.get_legend(), class1_legend, "Legend matches the tokens (1)");

        // Make a second request. Make sure we get the secondary value
        let result = get_document_semantic_tokens(&registry, &text_model, Some(&result.provider), tokens.result_id.as_deref(), &CancellationToken::None)
            .await
            .unwrap();
        let result = result.expect("We should have tokens (2)");
        let tokens = result.tokens.clone().expect("Tokens are found from multiple providers (2)");
        assert!(is_semantic_tokens(&tokens), "Tokens are full (2)");
        let SemanticTokensResult::Tokens(tokens) = tokens else { unreachable!("Tokens are full (2)") };
        assert!(tokens.result_id.is_none(), "Token result id found from multiple providers (2)");
        assert_eq!(tokens.data, vec![2, 1, 1, 1, 1, 0, 2, 1, 1, 1], "Token data returned for multiple providers (2)");
        assert_eq!(call_count.get(), 4, "Called both token providers (2)");
        assert_eq!(result.provider.get_legend(), class1_legend, "Legend matches the tokens (2)");

        ds.store().clear();

        // Wait for scheduler to finish
        timeout(0).await.unwrap();

        // Now dispose the text model
        text_model.dispose();
    });
}
