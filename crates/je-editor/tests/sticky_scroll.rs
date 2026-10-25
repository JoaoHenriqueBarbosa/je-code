//! Porte de `src/vs/editor/contrib/stickyScroll/test/browser/stickyScroll.test.ts`.
//!
//! Suíte `Sticky Scroll Tests`, caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` e o `teardown` (`disposables.clear()`) são o `LeakGuard` de [`run_test`], que
//!   também roda o corpo com o relógio virtual (`runWithFakedTimers({ useFakeTimers: true }, ...)`).
//! - `withAsyncTestCodeEditor(model, { stickyScroll, envConfig: { outerHeight: 500 }, serviceCollection }, ...)` é
//!   [`with_sticky_scroll_editor`], sobre `with_async_test_code_editor`: o modelo é o de `createTextModel(text)` e o
//!   callback o descarta ao fim, como o original. A `serviceCollection` do original traz `ILanguageFeaturesService`,
//!   `ILogService`, `IContextMenuService`, `ILanguageConfigurationService`, `IEnvironmentService` e
//!   `ILanguageFeatureDebounceService`; o editor de teste do porte não tem coleção de serviços com esses identificadores, então
//!   os serviços que o sticky scroll lê chegam por [`StickyScrollControllerServices`]: o `LanguageFeaturesService` é um novo por
//!   caso (`new LanguageFeaturesService()`), o `ILanguageConfigurationService` é o do editor de teste
//!   (`TestLanguageConfigurationService`), o `IContextMenuService` é o `mock<IContextMenuService>()` ([`MockContextMenuService`],
//!   que o caso nunca aciona), o `IContextKeyService` é o `MockContextKeyService` de `je-platform` e a navegação de
//!   `goToDefinitionWithLocation` (que o caso também nunca aciona) é [`MockDefinitionNavigation`]. O `ILogService`, o
//!   `IEnvironmentService` e o `ILanguageFeatureDebounceService` só alimentam o serviço de debounce, que o sticky scroll
//!   não usa.
//! - `editor.registerAndInstantiateContribution(StickyScrollController.ID, StickyScrollController)` é
//!   `register_and_instantiate_contribution` com o construtor do controlador.
//! - O provedor literal `{ provideDocumentSymbols() { return [...] } }` é [`TestDocumentSymbolProvider`], e `register('*', ...)`
//!   usa o `LanguageSelector` `"*"`. `new StickyLineCandidate(a, b, c, d)` é [`candidate`].
//! - `editor.setHiddenAreas([...])` é `set_hidden_areas(..., None, None)` (sem fonte nem `forceUpdate`).

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod support;

use std::future::Future;
use std::rc::Rc;

use common::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite, run_with_faked_timers};
use je_base::async_::async_fn;
use je_base::cancellation::CancellationToken;
use je_base::dynamic::Dynamic;
use je_base::errors::Error;
use je_base::event::Event;
use je_base::lifecycle::IDisposable;
use je_base::promise::Promise;
use je_editor::click_link_gesture::ClickLinkMouseEvent;
use je_editor::code_editor_widget::{CodeEditorWidget, ICodeEditor};
use je_editor::config::editor_options::line_height::LINE_HEIGHT;
use je_editor::language_configuration_registry::ILanguageConfigurationService;
use je_editor::language_features_service::{LanguageFeaturesService, Location};
use je_editor::language_providers::{DocumentSymbol, DocumentSymbolProvider, ProviderResult, SymbolKind};
use je_editor::language_selector::LanguageSelector;
use je_editor::range::Range;
use je_editor::sticky_scroll::sticky_scroll_controller::{
    IStickyScrollController, IStickyScrollDefinitionNavigation, StickyScrollController, StickyScrollControllerServices,
};
use je_editor::sticky_scroll::sticky_scroll_element::StickyRange;
use je_editor::sticky_scroll::sticky_scroll_provider::{IStickyLineCandidateProvider, StickyLineCandidate, StickyLineCandidateProvider};
use je_editor::text_model::TextModel;
use je_platform::contextkey::IContextKeyService;
use je_platform::contextview::context_view::{ContextMenuRequest, IContextMenuService};
use je_platform::keybinding::mock_keybinding_service::MockContextKeyService;
use support::test_code_editor::{CodeEditorServices, TestCodeEditor, TestCodeEditorInstantiationOptions, with_async_test_code_editor};
use support::test_text_model::create_text_model;

/// O que o mocha faz em volta de cada `test(...)` da suíte: o `ensureNoDisposablesAreLeakedInTestSuite()`, o corpo (com o
/// relógio virtual) e o `teardown`, nessa ordem.
fn run_test<F: Future<Output = ()> + 'static>(body: impl FnOnce(Rc<LeakGuard>) -> F) {
    let ds = Rc::new(ensure_no_disposables_are_leaked_in_test_suite());
    run_with_faked_timers(body(Rc::clone(&ds)));
}

/// `mock<IContextMenuService>()`: o que o caso não lê é `unimplemented!`, como um `mock` sem a propriedade.
struct MockContextMenuService;

impl IContextMenuService for MockContextMenuService {
    fn on_did_show_context_menu(&self) -> Event<()> {
        unimplemented!("mock")
    }

    fn on_did_hide_context_menu(&self) -> Event<()> {
        unimplemented!("mock")
    }

    fn show_context_menu(&self, _request: ContextMenuRequest) {
        unimplemented!("mock")
    }
}

/// O `goToDefinitionWithLocation` que o controlador chama no clique com o modificador: nenhum caso chega lá.
struct MockDefinitionNavigation;

impl IStickyScrollDefinitionNavigation for MockDefinitionNavigation {
    fn go_to_definition_with_location(&self, _event: &ClickLinkMouseEvent, _editor: &Rc<CodeEditorWidget>, _location: &Location) -> Promise<()> {
        unimplemented!("mock")
    }
}

/// O provedor `{ provideDocumentSymbols() { return [...]; } }`.
struct TestDocumentSymbolProvider {
    symbols: Vec<DocumentSymbol>,
}

impl DocumentSymbolProvider for TestDocumentSymbolProvider {
    fn provide_document_symbols(&self, _model: &Rc<TextModel>, _token: &CancellationToken) -> ProviderResult<Vec<DocumentSymbol>> {
        Promise::resolved(Some(self.symbols.clone()))
    }
}

/// Um símbolo `{ name, detail: name, kind, tags: [], range, selectionRange, children? }`.
fn symbol(name: &str, kind: SymbolKind, range: Range, selection_range: Range, children: Option<Vec<DocumentSymbol>>) -> DocumentSymbol {
    DocumentSymbol {
        name: name.to_owned(),
        detail: name.to_owned(),
        kind,
        tags: Vec::new(),
        container_name: None,
        range,
        selection_range,
        children,
    }
}

/// `{ startLineNumber, endLineNumber, startColumn: 1, endColumn: 1 }`.
fn lines(start_line_number: i32, end_line_number: i32) -> Range {
    Range::new(start_line_number, 1, end_line_number, 1)
}

/// `new StickyLineCandidate(startLineNumber, endLineNumber, top, height)`.
fn candidate(start_line_number: i32, end_line_number: i32, top: f64, height: f64) -> StickyLineCandidate {
    StickyLineCandidate { start_line_number, end_line_number, top, height }
}

fn text() -> String {
    [
        "function foo() {",
        "",
        "}",
        "/* comment related to TestClass",
        " end of the comment */",
        "@classDecorator",
        "class TestClass {",
        "// comment related to the function functionOfClass",
        "functionOfClass(){",
        "function function1(){",
        "}",
        "}}",
        "function bar() { function insideBar() {}",
        "}",
    ]
    .join("\n")
}

/// `documentSymbolProviderForTestModel()`.
fn document_symbol_provider_for_test_model() -> Rc<TestDocumentSymbolProvider> {
    let function1 = symbol("function1", SymbolKind::Function, lines(10, 11), lines(10, 10), None);
    let function_of_class = symbol("functionOfClass", SymbolKind::Function, lines(8, 12), lines(9, 9), Some(vec![function1]));
    let inside_bar = symbol("insideBar", SymbolKind::Function, lines(13, 13), lines(13, 13), None);
    Rc::new(TestDocumentSymbolProvider {
        symbols: vec![
            symbol("foo", SymbolKind::Function, lines(1, 3), lines(1, 1), None),
            symbol("TestClass", SymbolKind::Class, lines(4, 12), lines(7, 7), Some(vec![function_of_class])),
            symbol("bar", SymbolKind::Function, lines(13, 14), lines(13, 13), Some(vec![inside_bar])),
        ],
    })
}

fn text_with_scopes_with_same_starting_lines() -> String {
    ["class TestClass { foo() {", "function bar(){", "", "}}", "}", ""].join("\n")
}

/// `documentSymbolProviderForSecondTestModel()`.
fn document_symbol_provider_for_second_test_model() -> Rc<TestDocumentSymbolProvider> {
    let bar = symbol("bar", SymbolKind::Function, lines(2, 4), lines(2, 2), Some(Vec::new()));
    let foo = symbol("foo", SymbolKind::Function, lines(1, 4), lines(1, 1), Some(vec![bar]));
    Rc::new(TestDocumentSymbolProvider {
        symbols: vec![symbol("TestClass", SymbolKind::Class, lines(1, 5), lines(1, 1), Some(vec![foo]))],
    })
}

/// As opções `{ stickyScroll: { enabled: true, maxLineCount: 5, defaultModel: 'outlineModel' }, envConfig: { outerHeight: 500 } }`.
fn sticky_scroll_options() -> TestCodeEditorInstantiationOptions {
    TestCodeEditorInstantiationOptions::new(Dynamic::object([
        (
            "stickyScroll",
            Dynamic::object([
                ("enabled", Dynamic::Bool(true)),
                ("maxLineCount", Dynamic::Number(5.0)),
                ("defaultModel", Dynamic::from("outlineModel")),
            ]),
        ),
        ("envConfig", Dynamic::object([("outerHeight", Dynamic::Number(500.0))])),
    ]))
}

/// O que o callback de `withAsyncTestCodeEditor` recebe, mais o que a suíte guarda em variáveis de módulo.
struct StickyScrollTestContext {
    editor: Rc<TestCodeEditor>,
    services: Rc<CodeEditorServices>,
    /// `instantiationService.get(ILanguageFeaturesService)`.
    language_features_service: Rc<LanguageFeaturesService>,
    /// O `disposables` da suíte.
    ds: Rc<LeakGuard>,
}

impl StickyScrollTestContext {
    /// `instantiationService.get(ILanguageConfigurationService)`.
    fn language_configuration_service(&self) -> Rc<dyn ILanguageConfigurationService> {
        self.services.language_configuration_service.clone()
    }

    /// `disposables.add(languageService.documentSymbolProvider.register('*', provider))`.
    fn register_document_symbol_provider(&self, provider: Rc<TestDocumentSymbolProvider>) {
        let registration = self.language_features_service.document_symbol_provider.register(LanguageSelector::from("*"), provider);
        self.ds.add(registration);
    }

    /// `editor.registerAndInstantiateContribution(StickyScrollController.ID, StickyScrollController)`.
    fn create_sticky_scroll_controller(&self) -> Rc<StickyScrollController> {
        let language_features_service = Rc::clone(&self.language_features_service);
        let language_configuration_service = self.language_configuration_service();
        self.editor.register_and_instantiate_contribution(StickyScrollController::ID, move |widget, _| {
            let context_key_service: Rc<dyn IContextKeyService> = MockContextKeyService::new();
            StickyScrollController::new(
                widget,
                StickyScrollControllerServices {
                    context_menu_service: Rc::new(MockContextMenuService),
                    language_features_service,
                    language_configuration_service,
                    context_key_service,
                    definition_navigation: Rc::new(MockDefinitionNavigation),
                },
            )
        })
    }
}

/// `createTextModel(text)` seguido de `withAsyncTestCodeEditor(model, { stickyScroll, envConfig, serviceCollection }, ...)`. O
/// `model.dispose()` do fim do callback é o último passo.
async fn with_sticky_scroll_editor<F, Fut>(ds: Rc<LeakGuard>, text: &str, body: F)
where
    F: FnOnce(StickyScrollTestContext) -> Fut + 'static,
    Fut: Future<Output = Result<(), Error>> + 'static,
{
    let model = create_text_model(text);
    let model_to_dispose = Rc::clone(&model);
    let language_features_service = Rc::new(LanguageFeaturesService::new());
    with_async_test_code_editor(model, sticky_scroll_options(), move |editor, _view_model, services| {
        async_fn(async move {
            body(StickyScrollTestContext { editor, services, language_features_service, ds }).await?;
            model_to_dispose.dispose();
            Ok(())
        })
    })
    .await
    .expect("o caso não rejeita");
}

#[test]
fn testing_the_function_get_candidate_sticky_lines_intersecting() {
    run_test(|ds| async move {
        with_sticky_scroll_editor(ds, &text(), |ctx| async move {
            ctx.register_document_symbol_provider(document_symbol_provider_for_test_model());
            let provider = StickyLineCandidateProvider::new(ctx.editor.widget(), Rc::clone(&ctx.language_features_service), ctx.language_configuration_service());
            provider.update().await?;
            assert_eq!(provider.get_candidate_sticky_lines_intersecting(StickyRange::new(1, 4)), vec![candidate(1, 2, 0.0, 19.0)]);
            assert_eq!(
                provider.get_candidate_sticky_lines_intersecting(StickyRange::new(8, 10)),
                vec![candidate(7, 11, 0.0, 19.0), candidate(9, 11, 19.0, 19.0)]
            );
            assert_eq!(
                provider.get_candidate_sticky_lines_intersecting(StickyRange::new(10, 13)),
                vec![candidate(7, 11, 0.0, 19.0), candidate(9, 11, 19.0, 19.0)]
            );

            provider.dispose();
            Ok(())
        })
        .await;
    });
}

#[test]
fn issue_157180_render_the_correct_line_corresponding_to_the_scope_definition() {
    run_test(|ds| async move {
        with_sticky_scroll_editor(ds, &text(), |ctx| async move {
            let sticky_scroll_controller = ctx.create_sticky_scroll_controller();
            let line_height = *ctx.editor.get_option(LINE_HEIGHT);
            ctx.register_document_symbol_provider(document_symbol_provider_for_test_model());
            sticky_scroll_controller.sticky_scroll_candidate_provider().update().await?;

            ctx.editor.set_scroll_top(1.0, None);
            let state = sticky_scroll_controller.find_scroll_widget_state();
            assert_eq!(state.start_line_numbers, vec![1]);

            ctx.editor.set_scroll_top(line_height + 1.0, None);
            let state = sticky_scroll_controller.find_scroll_widget_state();
            assert_eq!(state.start_line_numbers, vec![1]);

            ctx.editor.set_scroll_top(4.0 * line_height + 1.0, None);
            let state = sticky_scroll_controller.find_scroll_widget_state();
            assert_eq!(state.start_line_numbers, Vec::<i32>::new());

            ctx.editor.set_scroll_top(8.0 * line_height + 1.0, None);
            let state = sticky_scroll_controller.find_scroll_widget_state();
            assert_eq!(state.start_line_numbers, vec![7, 9]);

            ctx.editor.set_scroll_top(9.0 * line_height + 1.0, None);
            let state = sticky_scroll_controller.find_scroll_widget_state();
            assert_eq!(state.start_line_numbers, vec![7, 9]);

            ctx.editor.set_scroll_top(10.0 * line_height + 1.0, None);
            let state = sticky_scroll_controller.find_scroll_widget_state();
            assert_eq!(state.start_line_numbers, vec![7]);

            sticky_scroll_controller.dispose();
            sticky_scroll_controller.sticky_scroll_candidate_provider().dispose();
            Ok(())
        })
        .await;
    });
}

#[test]
fn issue_156268_do_not_reveal_sticky_lines_when_they_are_in_a_folded_region() {
    run_test(|ds| async move {
        with_sticky_scroll_editor(ds, &text(), |ctx| async move {
            let sticky_scroll_controller = ctx.create_sticky_scroll_controller();
            let line_height = *ctx.editor.get_option(LINE_HEIGHT);

            ctx.register_document_symbol_provider(document_symbol_provider_for_test_model());
            sticky_scroll_controller.sticky_scroll_candidate_provider().update().await?;
            ctx.editor.set_hidden_areas(&[lines(2, 2), lines(10, 11)], None, None);

            ctx.editor.set_scroll_top(1.0, None);
            let state = sticky_scroll_controller.find_scroll_widget_state();
            assert_eq!(state.start_line_numbers, vec![1]);

            ctx.editor.set_scroll_top(line_height + 1.0, None);
            let state = sticky_scroll_controller.find_scroll_widget_state();
            assert_eq!(state.start_line_numbers, Vec::<i32>::new());

            ctx.editor.set_scroll_top(6.0 * line_height + 1.0, None);
            let state = sticky_scroll_controller.find_scroll_widget_state();
            assert_eq!(state.start_line_numbers, vec![7, 9]);

            ctx.editor.set_scroll_top(7.0 * line_height + 1.0, None);
            let state = sticky_scroll_controller.find_scroll_widget_state();
            assert_eq!(state.start_line_numbers, vec![7]);

            ctx.editor.set_scroll_top(10.0 * line_height + 1.0, None);
            let state = sticky_scroll_controller.find_scroll_widget_state();
            assert_eq!(state.start_line_numbers, Vec::<i32>::new());

            sticky_scroll_controller.dispose();
            sticky_scroll_controller.sticky_scroll_candidate_provider().dispose();
            Ok(())
        })
        .await;
    });
}

#[test]
fn issue_159271_render_the_correct_widget_state_when_the_child_scope_starts_on_the_same_line_as_the_parent_scope() {
    run_test(|ds| async move {
        with_sticky_scroll_editor(ds, &text_with_scopes_with_same_starting_lines(), |ctx| async move {
            let sticky_scroll_controller = ctx.create_sticky_scroll_controller();
            sticky_scroll_controller.sticky_scroll_candidate_provider().update().await?;
            let line_height = *ctx.editor.get_option(LINE_HEIGHT);

            ctx.register_document_symbol_provider(document_symbol_provider_for_second_test_model());
            sticky_scroll_controller.sticky_scroll_candidate_provider().update().await?;

            ctx.editor.set_scroll_top(1.0, None);
            let state = sticky_scroll_controller.find_scroll_widget_state();
            assert_eq!(state.start_line_numbers, vec![1, 2]);

            ctx.editor.set_scroll_top(line_height + 1.0, None);
            let state = sticky_scroll_controller.find_scroll_widget_state();
            assert_eq!(state.start_line_numbers, vec![1, 2]);

            ctx.editor.set_scroll_top(2.0 * line_height + 1.0, None);
            let state = sticky_scroll_controller.find_scroll_widget_state();
            assert_eq!(state.start_line_numbers, vec![1]);

            ctx.editor.set_scroll_top(3.0 * line_height + 1.0, None);
            let state = sticky_scroll_controller.find_scroll_widget_state();
            assert_eq!(state.start_line_numbers, vec![1]);

            ctx.editor.set_scroll_top(4.0 * line_height + 1.0, None);
            let state = sticky_scroll_controller.find_scroll_widget_state();
            assert_eq!(state.start_line_numbers, Vec::<i32>::new());

            sticky_scroll_controller.dispose();
            sticky_scroll_controller.sticky_scroll_candidate_provider().dispose();
            Ok(())
        })
        .await;
    });
}
