//! Porte de `src/vs/editor/contrib/suggest/test/browser/suggestController.test.ts` (suíte `SuggestController`), caso a caso
//! e na mesma ordem.
//!
//! Notas de tradução:
//! - O `setup` e o `teardown` da suíte são o [`run_test`]: o `languageFeaturesService` do original é criado na carga da
//!   suíte, então o registro de provedores nasce antes do [`Ctx`]; o [`Ctx`] guarda o `disposables`, o modelo (a URI
//!   `test-ctrl:/path.tst`, vazio), o editor de teste, o `SnippetController2` e o `SuggestController` registrados pelo
//!   `registerAndInstantiateContribution`, nessa ordem. Os casos que o original declara `async` são o corpo `async` do
//!   `run_test`, que roda com o relógio real (o original não usa relógio falso: os `timeout(10)` e `timeout(20)` são reais).
//!   O `ensureNoDisposablesAreLeakedInTestSuite()` está comentado no original e não tem equivalente aqui.
//! - Os serviços do `ServiceCollection` são os [`SuggestControllerServices`] e os [`SuggestModelServices`] que o contêiner
//!   do editor entregaria: o `ISuggestMemoryService` falso (`memorize() {}` e `select() { return 0 }`), o
//!   `IEditorWorkerService` falso (`computeWordRanges` devolve `{}`), o `IEnvironmentService` com `isBuilt` e
//!   `isExtensionDevelopment`, o `NullTelemetryService` e o `NullLogService`. O `IMenuService`, o `ILabelService`, o
//!   `IWorkspaceContextService` e o `IKeybindingService` do original são consumidos pelo widget e pelo `SnippetController2`
//!   com os dublês de `snippet_doubles`; o widget ainda não tem porte.
//! - `Event.toPromise(event)` é o `to_promise` do evento, `await p` o `.await` da promessa dele; `controller.model` é o
//!   campo público `model`.
//! - `acceptSelectedSuggestion` lê o item em foco que o controlador guarda no lugar do `widget.getFocusedItem()` (ver
//!   `suggest_controller.rs`).
//! - `resolve.forEach(fn => fn)` do caso `cancel` é um não-op no original (a função não é chamada), e aqui também: os
//!   resolvedores são guardados e descartados sem serem chamados.
//! - Os dois casos `test.skip` do original são `#[ignore]`, com o corpo portado.

#![allow(dead_code)]

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod snippet_doubles;
mod support;

use std::cell::{Cell, RefCell};
use std::future::Future;
use std::rc::Rc;

use common::{run_with_real_timers, timeout};
use je_base::JsString;
use je_base::lifecycle::{DisposableStore, IDisposable, to_disposable};
use je_base::promise::{Promise, Resolver};
use je_base::uri::{URI, UriComponents};
use je_editor::code_editor_widget::{Handler, ICodeEditor, SelectionOrRange, TypePayload};
use je_editor::edit_operation::SingleEditOperation;
use je_editor::editor_extensions::EditorAction;
use je_editor::editor_worker::{IEditorWorkerService, WordRanges};
use je_editor::language_feature_registry::LanguageFeatureRegistry;
use je_editor::language_providers::ProviderResult;
use je_editor::language_selector::{LanguageFilter, LanguageSelector};
use je_editor::languages::{
    CompletionContext, CompletionItem as LanguageCompletionItem, CompletionItemKind, CompletionItemProvider,
    CompletionItemRange, CompletionLabel, CompletionList, completion_item_insert_text_rule,
};
use je_editor::lines_operations::DeleteLinesAction;
use je_editor::model::RelaxedTextModelCreationOptions;
use je_editor::position::Position;
use je_editor::range::Range;
use je_editor::selection::Selection;
use je_editor::snippet::snippet_controller2::{SnippetController2, SnippetController2Services};
use je_editor::snippet::snippet_session::SnippetServices;
use je_editor::suggest::suggest::CompletionItem;
use je_editor::suggest::suggest_controller::{SuggestController, SuggestControllerServices};
use je_editor::suggest::suggest_memory::ISuggestMemoryService;
use je_editor::suggest::suggest_model::{ICancelEvent, ISuggestEvent, SuggestModelServices};
use je_editor::text_model::TextModel;
use je_platform::configuration::test_configuration_service::TestConfigurationService;
use je_platform::contextkey::IContextKeyService;
use je_platform::contextkey::service::ContextKeyService;
use je_platform::log::NullLogService;
use je_platform::telemetry::telemetry_utils::null_telemetry_service;
use je_platform::workspace::Workspace;
use snippet_doubles::{ClosureLabelService, WorkspaceOnlyService};
use support::mock_environment_service::MockEnvironmentService;
use support::null_command_service::NullCommandService;
use support::test_clipboard_service::TestClipboardService;
use support::test_code_editor::{TestCodeEditor, TestCodeEditorInstantiationOptions, create_test_code_editor};
use support::test_text_model::create_text_model_with;

// #region Serviços falsos

/// O `ISuggestMemoryService` do `ServiceCollection`: `memorize(): void { }` e `select(): number { return 0; }`.
struct NoMemoryService;

impl ISuggestMemoryService for NoMemoryService {
    fn memorize(&self, _model: &TextModel, _pos: Position, _item: &CompletionItem) {}

    fn select(&self, _model: &TextModel, _pos: Position, _items: &[CompletionItem]) -> usize {
        0
    }
}

/// O `IEditorWorkerService` do `ServiceCollection`: `computeWordRanges() { return Promise.resolve({}); }`.
struct EmptyWordRangesService;

impl IEditorWorkerService for EmptyWordRangesService {
    fn can_compute_word_ranges(&self, _resource: &URI) -> bool {
        true
    }

    fn compute_word_ranges(&self, _resource: &URI, _range: Range) -> Promise<Option<WordRanges>> {
        Promise::resolved(Some(WordRanges::new()))
    }
}

// #endregion

// #region Provedores

type ProvideFn = dyn Fn(&Rc<TextModel>, Position) -> Option<CompletionList>;
type ResolveFn = dyn Fn(&LanguageCompletionItem) -> Promise<Option<LanguageCompletionItem>>;

/// O provedor literal `{ _debugDisplayName: 'test', provideCompletionItems(doc, pos) {...}, resolveCompletionItem? }`.
struct TestProvider {
    provide: Box<ProvideFn>,
    resolve: Option<Box<ResolveFn>>,
}

impl TestProvider {
    fn new(provide: impl Fn(&Rc<TextModel>, Position) -> Option<CompletionList> + 'static) -> Self {
        Self { provide: Box::new(provide), resolve: None }
    }

    /// `resolveCompletionItem(item) {...}`.
    fn with_resolve(
        mut self,
        resolve: impl Fn(&LanguageCompletionItem) -> Promise<Option<LanguageCompletionItem>> + 'static,
    ) -> Self {
        self.resolve = Some(Box::new(resolve));
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
        None
    }

    fn provide_completion_items(
        &self,
        model: &Rc<TextModel>,
        position: Position,
        _context: &CompletionContext,
        _token: &je_base::cancellation::CancellationToken,
    ) -> ProviderResult<Rc<CompletionList>> {
        Promise::resolved((self.provide)(model, position).map(Rc::new))
    }

    fn has_resolve_completion_item(&self) -> bool {
        self.resolve.is_some()
    }

    fn resolve_completion_item(
        &self,
        item: &LanguageCompletionItem,
        _token: &je_base::cancellation::CancellationToken,
    ) -> Promise<Option<LanguageCompletionItem>> {
        match &self.resolve {
            Some(resolve) => resolve(item),
            None => Promise::resolved(Some(item.clone())),
        }
    }
}

/// `{ kind, label, insertText, range }` (os demais campos ausentes).
fn suggestion(label: &str, kind: CompletionItemKind, insert_text: &str, range: Range) -> LanguageCompletionItem {
    LanguageCompletionItem {
        label: CompletionLabel::Text(label.to_string()),
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
        command: None,
        action: None,
        extension_id: None,
        internal_id: None,
    }
}

/// `{ suggestions }`.
fn list(suggestions: Vec<LanguageCompletionItem>) -> Option<CompletionList> {
    Some(CompletionList { suggestions, incomplete: None, dispose: None, duration: None })
}

/// `{ startLineNumber, startColumn, endLineNumber, endColumn }` de um ponto só.
fn point(line_number: i32, column: i32) -> Range {
    Range::new(line_number, column, line_number, column)
}

/// `{ text, range }` de `additionalTextEdits`.
fn edit(range: Range, text: &str) -> SingleEditOperation {
    SingleEditOperation::new(range, Some(JsString::from(text)), false)
}

/// `const word = doc.getWordUntilPosition(pos); new Range(pos.lineNumber, word.startColumn, pos.lineNumber, word.endColumn)`.
fn word_range(doc: &Rc<TextModel>, pos: Position) -> Range {
    let word = doc.get_word_until_position(pos);
    Range::new(pos.line_number, word.start_column, pos.line_number, word.end_column)
}

// #endregion

// #region Fixture

type CompletionRegistry = Rc<LanguageFeatureRegistry<dyn CompletionItemProvider>>;

fn selector_test_ctrl() -> LanguageSelector {
    LanguageSelector::Filter(LanguageFilter { scheme: Some("test-ctrl".to_string()), ..LanguageFilter::default() })
}

/// O que o `setup` e as variáveis da suíte deixam: `disposables`, `editor`, `model`, `controller` e o registro do
/// `languageFeaturesService`.
struct Ctx {
    disposables: Rc<DisposableStore>,
    editor: Rc<TestCodeEditor>,
    model: Rc<TextModel>,
    controller: Rc<SuggestController>,
    completion_provider: CompletionRegistry,
}

impl Ctx {
    /// `setup()`.
    fn new(completion_provider: CompletionRegistry) -> Self {
        let disposables = DisposableStore::new();

        let model = create_text_model_with(
            "",
            None,
            RelaxedTextModelCreationOptions::default(),
            Some(URI::from(
                &UriComponents {
                    scheme: Some(JsString::from("test-ctrl")),
                    path: Some(JsString::from("/path.tst")),
                    ..UriComponents::default()
                },
                false,
            )),
        );
        let owned_model = Rc::clone(&model);
        disposables.add(to_disposable(move || owned_model.dispose()));

        let editor = create_test_code_editor(Some(Rc::clone(&model)), TestCodeEditorInstantiationOptions::default());
        disposables.add_rc(Rc::clone(&editor));

        let context_keys = ContextKeyService::new(TestConfigurationService::new(None));
        let disposable_context_keys = Rc::clone(&context_keys);
        editor.register_disposable(to_disposable(move || disposable_context_keys.dispose()));

        let snippet_controller = {
            let context_keys = Rc::clone(&context_keys);
            editor.register_and_instantiate_contribution(SnippetController2::ID, move |widget, _| {
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
        };
        editor.register_disposable(snippet_controller);

        let controller = {
            let context_keys = Rc::clone(&context_keys);
            let registry = Rc::clone(&completion_provider);
            editor.register_and_instantiate_contribution(SuggestController::ID, move |widget, _| {
                let context_key_service: Rc<dyn IContextKeyService> = context_keys;
                SuggestController::new(
                    widget,
                    SuggestControllerServices {
                        memory_service: Rc::new(NoMemoryService),
                        command_service: Rc::new(NullCommandService),
                        context_key_service: Rc::clone(&context_key_service),
                        log_service: NullLogService::shared(),
                        model_services: SuggestModelServices {
                            editor_worker_service: Rc::new(EmptyWordRangesService),
                            clipboard_service: Rc::new(TestClipboardService::new()),
                            telemetry_service: null_telemetry_service(),
                            log_service: NullLogService::shared(),
                            context_key_service,
                            completion_provider: registry,
                            inline_completions: None,
                            environment_service: MockEnvironmentService::new(true, false),
                        },
                        aria_alert: Rc::new(|_: &str| {}),
                    },
                )
            })
        };
        editor.register_disposable(Rc::clone(&controller));

        Self { disposables, editor, model, controller, completion_provider }
    }

    /// `disposables.add(languageFeaturesService.completionProvider.register({ scheme: 'test-ctrl' }, provider))`.
    fn register(&self, provider: Rc<dyn CompletionItemProvider>) {
        self.disposables.add(self.completion_provider.register(selector_test_ctrl(), provider));
    }

    /// `editor.setValue(value)`.
    fn set_value(&self, value: &str) {
        self.editor.set_value(JsString::from(value));
    }

    /// `editor.getValue()`.
    fn value(&self) -> String {
        self.model.get_value().to_string_lossy()
    }

    /// `editor.setSelection(new Selection(...))`.
    fn set_selection(&self, start_line_number: i32, start_column: i32, end_line_number: i32, end_column: i32) {
        self.editor.set_selection(
            SelectionOrRange::Selection(Selection::new(start_line_number, start_column, end_line_number, end_column)),
            None,
        );
    }

    /// `editor.trigger('test', 'type', { text })`.
    fn type_text(&self, source: &str, text: &str) {
        self.editor.trigger(Some(source), Handler::TYPE, Some(TypePayload { text: JsString::from(text) }.to_dynamic()));
    }

    /// `assert.ok(editor.getSelection()?.equalsSelection(new Selection(...)))`.
    fn assert_selection(&self, expected: Selection) {
        assert_eq!(self.editor.get_selection(), Some(expected));
    }

    /// `const p1 = Event.toPromise(controller.model.onDidSuggest); controller.triggerSuggest(); await p1;`.
    async fn trigger_suggest(&self) -> ISuggestEvent {
        let p1 = self.controller.model.on_did_suggest().to_promise(None);
        self.controller.trigger_suggest(None, None, None);
        p1.promise.clone().await.expect("onDidSuggest")
    }

    /// `const p2 = Event.toPromise(controller.model.onDidCancel); controller.acceptSelectedSuggestion(...); await p2;`.
    async fn accept_selected_suggestion_and_wait_cancel(&self, keep_alternative_suggestions: bool) -> ICancelEvent {
        let p2 = self.controller.model.on_did_cancel().to_promise(None);
        self.controller.accept_selected_suggestion(keep_alternative_suggestions, false);
        p2.promise.clone().await.expect("onDidCancel")
    }

    /// `teardown()`.
    fn teardown(&self) {
        self.disposables.dispose();
    }
}

/// O que o mocha faz em volta de cada `test(...)` da suíte (ver as notas no topo do arquivo).
fn run_test<F: Future<Output = ()> + 'static>(body: impl FnOnce(Rc<Ctx>) -> F + 'static) {
    je_editor::core_commands::register_core_commands();

    // o `languageFeaturesService` do original é criado na carga da suíte
    let completion_provider: CompletionRegistry = Rc::new(LanguageFeatureRegistry::new(None));
    let ctx = Rc::new(Ctx::new(completion_provider));

    run_with_real_timers(async move {
        body(Rc::clone(&ctx)).await;
        ctx.teardown();
    });
}

/// Um resolvedor que o provedor guarda para o teste chamar depois (`let resolve: Function = () => { }`).
type ResolverSlot = Rc<RefCell<Option<Resolver<()>>>>;

/// `await new Promise(_resolve => resolve = _resolve)`: a promessa pendente, com o resolvedor guardado em `slot`.
fn pending_in(slot: &ResolverSlot) -> Promise<()> {
    let slot = Rc::clone(slot);
    Promise::with_executor(move |resolver| {
        *slot.borrow_mut() = Some(resolver);
    })
}

/// `resolve()`: chama o resolvedor guardado.
fn call_resolver(slot: &ResolverSlot) {
    let resolver = slot.borrow().clone().expect("o resolvedor foi guardado");
    resolver.resolve(());
}

/// `item.additionalTextEdits = [{ text, range }]; return item;` depois da espera `wait`.
fn resolve_with_edit_after(
    wait: Promise<()>,
    item: &LanguageCompletionItem,
    range: Range,
    text: &'static str,
) -> Promise<Option<LanguageCompletionItem>> {
    let mut item = item.clone();
    wait.then(move |()| {
        item.additional_text_edits = Some(vec![edit(range, text)]);
        Some(item)
    })
}

// #endregion

#[test]
fn postfix_completion_reports_incorrect_position_86984() {
    run_test(|ctx| async move {
        ctx.register(
            TestProvider::new(|_doc, _pos| {
                let mut item = suggestion("let", CompletionItemKind::Snippet, "let ${1:name} = foo$0", Range::new(1, 9, 1, 11));
                item.insert_text_rules = Some(completion_item_insert_text_rule::INSERT_AS_SNIPPET);
                item.additional_text_edits = Some(vec![edit(Range::new(1, 5, 1, 9), "")]);
                list(vec![item])
            })
            .rc(),
        );

        ctx.set_value("    foo.le");
        ctx.set_selection(1, 11, 1, 11);

        // trigger
        ctx.trigger_suggest().await;

        //
        ctx.accept_selected_suggestion_and_wait_cancel(false).await;

        assert_eq!(ctx.value(), "    let name = foo");
    });
}

#[test]
fn use_additional_text_edits_sync_when_possible() {
    run_test(|ctx| async move {
        ctx.register(
            TestProvider::new(|_doc, pos| {
                let mut item = suggestion("let", CompletionItemKind::Snippet, "hello", Range::from_positions(pos, pos));
                item.additional_text_edits = Some(vec![edit(point(1, 1), "I came sync")]);
                list(vec![item])
            })
            .with_resolve(|item| Promise::resolved(Some(item.clone())))
            .rc(),
        );

        ctx.set_value("hello\nhallo");
        ctx.set_selection(2, 6, 2, 6);

        // trigger
        ctx.trigger_suggest().await;

        //
        ctx.accept_selected_suggestion_and_wait_cancel(false).await;

        // insertText happens sync!
        assert_eq!(ctx.value(), "I came synchello\nhallohello");
    });
}

#[test]
fn resolve_additional_text_edits_async_when_needed() {
    run_test(|ctx| async move {
        let resolve_call_count = Rc::new(Cell::new(0));

        ctx.register(
            TestProvider::new(|_doc, pos| list(vec![suggestion("let", CompletionItemKind::Snippet, "hello", Range::from_positions(pos, pos))]))
                .with_resolve({
                    let resolve_call_count = Rc::clone(&resolve_call_count);
                    move |item| {
                        resolve_call_count.set(resolve_call_count.get() + 1);
                        resolve_with_edit_after(timeout(10), item, point(1, 1), "I came late")
                    }
                })
                .rc(),
        );

        ctx.set_value("hello\nhallo");
        ctx.set_selection(2, 6, 2, 6);

        // trigger
        ctx.trigger_suggest().await;

        //
        ctx.accept_selected_suggestion_and_wait_cancel(false).await;

        // insertText happens sync!
        assert_eq!(ctx.value(), "hello\nhallohello");
        assert_eq!(resolve_call_count.get(), 1);

        // additional edits happened after a litte wait
        timeout(20).await.expect("timeout");
        assert_eq!(ctx.value(), "I came latehello\nhallohello");

        // single undo stop
        ctx.editor.get_model().expect("o editor tem modelo").undo();
        assert_eq!(ctx.value(), "hello\nhallo");
    });
}

#[test]
fn resolve_additional_text_edits_async_when_needed_typing() {
    run_test(|ctx| async move {
        let resolve_call_count = Rc::new(Cell::new(0));
        let resolve: ResolverSlot = Rc::new(RefCell::new(None));

        ctx.register(
            TestProvider::new(|_doc, pos| list(vec![suggestion("let", CompletionItemKind::Snippet, "hello", Range::from_positions(pos, pos))]))
                .with_resolve({
                    let (resolve_call_count, resolve) = (Rc::clone(&resolve_call_count), Rc::clone(&resolve));
                    move |item| {
                        resolve_call_count.set(resolve_call_count.get() + 1);
                        resolve_with_edit_after(pending_in(&resolve), item, point(1, 1), "I came late")
                    }
                })
                .rc(),
        );

        ctx.set_value("hello\nhallo");
        ctx.set_selection(2, 6, 2, 6);

        // trigger
        ctx.trigger_suggest().await;

        //
        ctx.accept_selected_suggestion_and_wait_cancel(false).await;

        // insertText happens sync!
        assert_eq!(ctx.value(), "hello\nhallohello");
        assert_eq!(resolve_call_count.get(), 1);

        // additional edits happened after a litte wait
        ctx.assert_selection(Selection::new(2, 11, 2, 11));
        ctx.type_text("test", "TYPING");

        assert_eq!(ctx.value(), "hello\nhallohelloTYPING");

        call_resolver(&resolve);
        timeout(10).await.expect("timeout");
        assert_eq!(ctx.value(), "I came latehello\nhallohelloTYPING");
        ctx.assert_selection(Selection::new(2, 17, 2, 17));
    });
}

// additional edit come late and are AFTER the selection -> cancel
#[test]
fn resolve_additional_text_edits_async_when_needed_simple_conflict() {
    run_test(|ctx| async move {
        let resolve_call_count = Rc::new(Cell::new(0));
        let resolve: ResolverSlot = Rc::new(RefCell::new(None));

        ctx.register(
            TestProvider::new(|_doc, pos| list(vec![suggestion("let", CompletionItemKind::Snippet, "hello", Range::from_positions(pos, pos))]))
                .with_resolve({
                    let (resolve_call_count, resolve) = (Rc::clone(&resolve_call_count), Rc::clone(&resolve));
                    move |item| {
                        resolve_call_count.set(resolve_call_count.get() + 1);
                        resolve_with_edit_after(pending_in(&resolve), item, point(1, 6), "I came late")
                    }
                })
                .rc(),
        );

        ctx.set_value("");
        ctx.set_selection(1, 1, 1, 1);

        // trigger
        ctx.trigger_suggest().await;

        //
        ctx.accept_selected_suggestion_and_wait_cancel(false).await;

        // insertText happens sync!
        assert_eq!(ctx.value(), "hello");
        assert_eq!(resolve_call_count.get(), 1);

        call_resolver(&resolve);
        timeout(10).await.expect("timeout");
        assert_eq!(ctx.value(), "hello");
    });
}

// additional edit come late and are AFTER the position at which the user typed -> cancelled
#[test]
fn resolve_additional_text_edits_async_when_needed_conflict() {
    run_test(|ctx| async move {
        let resolve_call_count = Rc::new(Cell::new(0));
        let resolve: ResolverSlot = Rc::new(RefCell::new(None));

        ctx.register(
            TestProvider::new(|_doc, pos| list(vec![suggestion("let", CompletionItemKind::Snippet, "hello", Range::from_positions(pos, pos))]))
                .with_resolve({
                    let (resolve_call_count, resolve) = (Rc::clone(&resolve_call_count), Rc::clone(&resolve));
                    move |item| {
                        resolve_call_count.set(resolve_call_count.get() + 1);
                        resolve_with_edit_after(pending_in(&resolve), item, point(1, 2), "I came late")
                    }
                })
                .rc(),
        );

        ctx.set_value("hello\nhallo");
        ctx.set_selection(2, 6, 2, 6);

        // trigger
        ctx.trigger_suggest().await;

        //
        ctx.accept_selected_suggestion_and_wait_cancel(false).await;

        // insertText happens sync!
        assert_eq!(ctx.value(), "hello\nhallohello");
        assert_eq!(resolve_call_count.get(), 1);

        // additional edits happened after a litte wait
        ctx.set_selection(1, 1, 1, 1);
        ctx.type_text("test", "TYPING");

        assert_eq!(ctx.value(), "TYPINGhello\nhallohello");

        call_resolver(&resolve);
        timeout(10).await.expect("timeout");
        assert_eq!(ctx.value(), "TYPINGhello\nhallohello");
        ctx.assert_selection(Selection::new(1, 7, 1, 7));
    });
}

#[test]
fn resolve_additional_text_edits_async_when_needed_cancel() {
    run_test(|ctx| async move {
        // `const resolve: Function[] = []`: os resolvedores guardados, que o caso nunca chama
        let resolve: Rc<RefCell<Vec<ResolverSlot>>> = Rc::new(RefCell::new(Vec::new()));

        ctx.register(
            TestProvider::new(|_doc, pos| {
                list(vec![
                    suggestion("let", CompletionItemKind::Snippet, "hello", Range::from_positions(pos, pos)),
                    suggestion("let", CompletionItemKind::Snippet, "hallo", Range::from_positions(pos, pos)),
                ])
            })
            .with_resolve({
                let resolve = Rc::clone(&resolve);
                move |item| {
                    let slot: ResolverSlot = Rc::new(RefCell::new(None));
                    let wait = pending_in(&slot);
                    resolve.borrow_mut().push(slot);
                    resolve_with_edit_after(wait, item, point(1, 2), "additionalTextEdits")
                }
            })
            .rc(),
        );

        ctx.set_value("abc");
        ctx.set_selection(1, 1, 1, 1);

        // trigger
        ctx.trigger_suggest().await;

        //
        ctx.accept_selected_suggestion_and_wait_cancel(true).await;

        // insertText happens sync!
        assert_eq!(ctx.value(), "helloabc");

        // next
        ctx.controller.accept_next_suggestion();

        // resolve additional edits (MUST be cancelled)
        // `resolve.forEach(fn => fn)` não chama nada
        resolve.borrow_mut().clear();
        timeout(10).await.expect("timeout");

        // next suggestion used
        assert_eq!(ctx.value(), "halloabc");
    });
}

#[test]
fn completion_edits_are_applied_inconsistently_when_additional_text_edits_and_text_edit_start_at_the_same_offset_143888() {
    run_test(|ctx| async move {
        ctx.register(
            TestProvider::new(|_doc, pos| {
                let mut item = suggestion("MyClassName", CompletionItemKind::Text, "MyClassName", Range::from_positions(pos, pos));
                item.additional_text_edits = Some(vec![edit(Range::from_positions(pos, pos), "import \"my_class.txt\";\n")]);
                list(vec![item])
            })
            .rc(),
        );

        ctx.set_value("");
        ctx.set_selection(1, 1, 1, 1);

        // trigger
        ctx.trigger_suggest().await;

        //
        ctx.accept_selected_suggestion_and_wait_cancel(true).await;

        // insertText happens sync!
        assert_eq!(ctx.value(), "import \"my_class.txt\";\nMyClassName");
    });
}

#[test]
fn pressing_enter_on_autocomplete_should_always_apply_the_selected_dropdown_completion_not_a_different_hidden_one_161883() {
    run_test(|ctx| async move {
        ctx.register(
            TestProvider::new(|doc, pos| {
                let range = word_range(doc, pos);

                let mut first = suggestion("filterBankSize", CompletionItemKind::Text, "filterBankSize", range);
                first.sort_text = Some("a".to_string());
                let mut second = suggestion("filter", CompletionItemKind::Text, "filter", range);
                second.sort_text = Some("b".to_string());
                list(vec![first, second])
            })
            .rc(),
        );

        ctx.set_value("filte");
        ctx.set_selection(1, 6, 1, 6);

        let completion_model = ctx.trigger_suggest().await.completion_model;
        let items = completion_model.items();
        assert_eq!(items.len(), 2);

        let (first, second) = (&items[0], &items[1]);
        assert_eq!(first.text_label, "filterBankSize");
        assert_eq!(second.text_label, "filter");

        assert_eq!(ctx.editor.get_selection(), Some(Selection::new(1, 6, 1, 6)));
        ctx.type_text("keyboard", "r"); // now filter "overtakes" filterBankSize because it is fully matched
        assert_eq!(ctx.editor.get_selection(), Some(Selection::new(1, 7, 1, 7)));

        ctx.controller.accept_selected_suggestion(false, false);
        assert_eq!(ctx.value(), "filter");
    });
}

#[test]
fn fast_autocomple_typing_selects_the_previous_autocomplete_suggestion_71795() {
    run_test(|ctx| async move {
        ctx.register(
            TestProvider::new(|doc, pos| {
                let range = word_range(doc, pos);

                list(
                    ["false", "float", "for", "foreach"]
                        .into_iter()
                        .map(|label| suggestion(label, CompletionItemKind::Text, label, range))
                        .collect(),
                )
            })
            .rc(),
        );

        ctx.set_value("f");
        ctx.set_selection(1, 2, 1, 2);

        let completion_model = ctx.trigger_suggest().await.completion_model;
        let items = completion_model.items();
        assert_eq!(items.len(), 4);

        let (first, second, third, fourth) = (&items[0], &items[1], &items[2], &items[3]);
        assert_eq!(first.text_label, "false");
        assert_eq!(second.text_label, "float");
        assert_eq!(third.text_label, "for");
        assert_eq!(fourth.text_label, "foreach");

        assert_eq!(ctx.editor.get_selection(), Some(Selection::new(1, 2, 1, 2)));
        ctx.type_text("keyboard", "o"); // filters`false` and `float`
        assert_eq!(ctx.editor.get_selection(), Some(Selection::new(1, 3, 1, 3)));

        ctx.controller.accept_selected_suggestion(false, false);
        assert_eq!(ctx.value(), "for");
    });
}

#[test]
#[ignore = "test.skip no original"]
fn suggest_widget_gets_orphaned_in_editor_187779() {
    run_test(|ctx| async move {
        ctx.register(
            TestProvider::new(|doc, pos| {
                let word = doc.get_line_content(pos.line_number).to_string_lossy();
                let range = Range::new(pos.line_number, 1, pos.line_number, pos.column);

                list(vec![suggestion(&word, CompletionItemKind::Text, &word, range)])
            })
            .rc(),
        );

        ctx.set_value("console.log(example.)\nconsole.log(EXAMPLE.not)");
        ctx.set_selection(1, 21, 1, 21);

        ctx.trigger_suggest().await;

        let p2 = ctx.controller.model.on_did_cancel().to_promise(None);
        let _ = DeleteLinesAction::new().run(None, ctx.editor.widget(), &je_base::dynamic::Dynamic::Undefined);

        p2.promise.clone().await.expect("onDidCancel");
    });
}

#[test]
fn ranges_where_additional_text_edits_are_applied_are_not_appropriate_when_characters_are_typed_177591() {
    run_test(|ctx| async move {
        ctx.register(
            TestProvider::new(|_doc, pos| {
                let mut item = suggestion("aaa", CompletionItemKind::Snippet, "aaa", Range::from_positions(pos, pos));
                let shifted = pos.delta(0, 10);
                item.additional_text_edits = Some(vec![edit(Range::from_positions(shifted, shifted), "aaa")]);
                list(vec![item])
            })
            .rc(),
        );

        {
            // PART1 - no typing
            ctx.set_value("123456789123456789");
            ctx.set_selection(1, 1, 1, 1);

            let e = ctx.trigger_suggest().await;
            assert_eq!(e.completion_model.items().len(), 1);
            assert_eq!(e.completion_model.items()[0].text_label, "aaa");

            ctx.controller.accept_selected_suggestion(false, false);

            assert_eq!(ctx.value(), "aaa1234567891aaa23456789");
        }

        {
            // PART2 - typing
            ctx.set_value("123456789123456789");
            ctx.set_selection(1, 1, 1, 1);

            let e = ctx.trigger_suggest().await;
            assert_eq!(e.completion_model.items().len(), 1);
            assert_eq!(e.completion_model.items()[0].text_label, "aaa");

            ctx.type_text("keyboard", "aa");

            ctx.controller.accept_selected_suggestion(false, false);

            assert_eq!(ctx.value(), "aaa1234567891aaa23456789");
        }
    });
}

#[test]
#[ignore = "test.skip no original"]
fn bug_no_suggestions_persists_while_typing_if_the_completion_helper_is_set_to_return_an_empty_list_for_empty_content_3557() {
    run_test(|ctx| async move {
        let request_count = Rc::new(Cell::new(0));

        ctx.register(
            TestProvider::new({
                let request_count = Rc::clone(&request_count);
                move |_doc, pos| {
                    request_count.set(request_count.get() + 1);

                    if request_count.get() == 1 {
                        return None;
                    }

                    list(vec![suggestion("foo", CompletionItemKind::Text, "foo", Range::new(pos.line_number, 1, pos.line_number, pos.column))])
                }
            })
            .rc(),
        );

        let e1 = ctx.trigger_suggest().await;
        assert_eq!(e1.completion_model.items().len(), 0);
        assert_eq!(request_count.get(), 1);

        let p2 = ctx.controller.model.on_did_suggest().to_promise(None);
        ctx.type_text("keyboard", "f");

        let e2 = p2.promise.clone().await.expect("onDidSuggest");
        assert_eq!(e2.completion_model.items().len(), 1);
        assert_eq!(request_count.get(), 2);
    });
}
