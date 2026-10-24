//! Porte de `src/vs/workbench/contrib/editTelemetry/test/browser/aiContributionFeature.test.ts` (suíte
//! `AiContributionFeature`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` é o `LeakGuard`, criado no início de cada caso dentro do relógio virtual de
//!   [`run`] (a conferência de vazamento roda antes dos timers que sobraram); `runWithFakedTimers({}, async () => ...)` é o
//!   `run_with_faked_timers`, e `await timeout(ms)` é [`sleep`].
//! - `new TestInstantiationService(new ServiceCollection(), false, undefined, true)` é um `InstantiationService` não
//!   estrito sobre um `ServiceCollection`. `stubInstance(DiffService, { computeDiff: ... computeStringDiff(original,
//!   modified, { maxComputationTimeMs: 500 }, 'advanced') })` é o `DiffService` de verdade sobre o [`TestEditorWorkerService`],
//!   que usa o `compute_string_edit_from_diff` padrão (o mesmo `computeStringDiff` com 500 ms e o algoritmo `advanced`).
//!   `stubInstance(UriVisibilityProvider, { isVisible: () => true })` é o serviço `I_URI_VISIBILITY_PROVIDER`, e
//!   `stub(ILogService, new NullLogService())` é o `NullLogService`. O `new AnnotatedDocuments(workspace, instantiationService)`
//!   e o `createInstance(AiContributionFeature, annotatedDocuments)` do original são `create_instance`.
//! - `CommandsRegistry.getCommand(id)!.handler(undefined!, uris, level)` é o `handler` do comando chamado dentro de um
//!   `invoke_function` (o `ServicesAccessor` do porte não pode ser nulo); `URI[]` é um `Dynamic::Array` de
//!   [`uri_argument`] (o `URI` como carga nativa do objeto, que o `URI.revive` do original devolve inalterado), e o
//!   `as unknown as boolean` é o `Dynamic::Bool` do retorno.
//! - `EditSources.chatApplyEdits({ ... undefined ... })`, `EditSources.cursor({ kind: 'type' })` e
//!   `EditSources.inlineCompletionAccept({ ... })` são as mesmas fábricas com os campos `undefined` como `None`.
//! - `MutableObservableWorkspace` e `MutableObservableDocument` estão em `support/edit_source_tracking_impl_support.rs`.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
#[path = "support/edit_source_tracking_impl_support.rs"]
mod support;

use std::future::Future;
use std::rc::Rc;

use common::{ensure_no_disposables_are_leaked_in_test_suite, run_with_faked_timers, timeout};
use je_base::dynamic::Dynamic;
use je_base::lifecycle::{DisposableStore, IDisposable};
use je_base::promise::Promise;
use je_base::uri::URI;
use je_editor::editor_worker::{I_EDITOR_WORKER_SERVICE, IEditorWorkerService, WordRanges};
use je_editor::range::Range;
use je_editor::text_model_edit_source::{ChatApplyEditsData, EditSources, InlineCompletionAcceptData, TextModelEditSource};
use je_instantiation::{InstantiationService, ServiceCollection};
use je_platform::commands::commands::commands_registry;
use je_platform::log::{I_LOG_SERVICE, NullLogService};
use je_workbench::contrib::edit_telemetry::browser::ai_contribution_feature::{AiContributionFeature, AiContributionLevel, uri_argument};
use je_workbench::contrib::edit_telemetry::browser::helpers::annotated_documents::{AnnotatedDocuments, I_URI_VISIBILITY_PROVIDER, IAnnotatedDocuments, IUriVisibilityProvider};
use je_workbench::contrib::edit_telemetry::browser::helpers::observable_workspace::{IObservableDocument, ObservableWorkspace, StringEditWithReason};
use support::{CreateDocumentOptions, MutableObservableWorkspace};

/// `runWithFakedTimers({}, async () => ...)` com o `ensureNoDisposablesAreLeakedInTestSuite()` dentro do relógio virtual.
fn run(body: impl Future<Output = ()> + 'static) {
    run_with_faked_timers(async move {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        body.await;
    });
}

/// `await timeout(ms)`.
async fn sleep(ms: u64) {
    timeout(ms).await.expect("timeout");
}

/// O `IEditorWorkerService` sobre o qual o `DiffService` calcula a diferença: o `computeStringEditFromDiff` padrão.
struct TestEditorWorkerService;

impl IEditorWorkerService for TestEditorWorkerService {
    fn can_compute_word_ranges(&self, _resource: &URI) -> bool {
        false
    }

    fn compute_word_ranges(&self, _resource: &URI, _range: Range) -> Promise<Option<WordRanges>> {
        Promise::resolved(None)
    }
}

/// `stubInstance(UriVisibilityProvider, { isVisible: (uri, reader) => true })`.
struct TestUriVisibilityProvider;

impl IUriVisibilityProvider for TestUriVisibilityProvider {
    fn is_visible(&self, _uri: &URI, _reader: &dyn je_base::observable::IReader) -> bool {
        true
    }
}

/// `chatEdit`.
fn chat_edit() -> Rc<TextModelEditSource> {
    Rc::new(EditSources::chat_apply_edits(&ChatApplyEditsData { language_id: "plaintext".to_owned(), ..Default::default() }))
}

/// `userEdit`.
fn user_edit() -> Rc<TextModelEditSource> {
    Rc::new(EditSources::cursor("type", None))
}

/// `inlineCompletionEdit`.
fn inline_completion_edit() -> Rc<TextModelEditSource> {
    Rc::new(EditSources::inline_completion_accept(&InlineCompletionAcceptData {
        nes: false,
        request_uuid: "test-uuid".to_owned(),
        language_id: "plaintext".to_owned(),
        provider_id: None,
        correlation_id: None,
    }))
}

/// O que `setup()` deixa nas variáveis da suíte (`disposables`, `workspace`) e o serviço de instanciação.
struct Env {
    disposables: Rc<DisposableStore>,
    workspace: Rc<MutableObservableWorkspace>,
    instantiation_service: Rc<InstantiationService>,
}

fn setup() -> Env {
    let disposables = DisposableStore::new();
    let editor_worker_service: Rc<dyn IEditorWorkerService> = Rc::new(TestEditorWorkerService);
    let uri_visibility_provider: Rc<dyn IUriVisibilityProvider> = Rc::new(TestUriVisibilityProvider);
    let instantiation_service = disposables.add_rc(InstantiationService::new(
        ServiceCollection::new()
            .with(I_EDITOR_WORKER_SERVICE, editor_worker_service)
            .with(I_URI_VISIBILITY_PROVIDER, uri_visibility_provider)
            .with(I_LOG_SERVICE, NullLogService::shared()),
    ));

    let workspace = MutableObservableWorkspace::new();
    let observable_workspace: Rc<dyn ObservableWorkspace> = workspace.clone();
    let annotated_documents = disposables.add_rc(instantiation_service.create_instance::<AnnotatedDocuments>(observable_workspace));
    let annotated: Rc<dyn IAnnotatedDocuments> = annotated_documents.clone();
    disposables.add_rc(instantiation_service.create_instance::<AiContributionFeature>(annotated));

    Env { disposables, workspace, instantiation_service }
}

/// `CommandsRegistry.getCommand(id)!.handler(undefined!, ...args)`.
fn call_command(env: &Env, id: &str, args: Vec<Dynamic>) -> Dynamic {
    let command = commands_registry().get_command(id).expect("comando registrado");
    env.instantiation_service.invoke_function(|accessor| (command.handler)(accessor, &args))
}

fn uris_argument(uris: &[&URI]) -> Dynamic {
    Dynamic::array(uris.iter().map(|uri| uri_argument(uri)))
}

fn has_ai_contributions(env: &Env, uris: &[&URI], level: AiContributionLevel) -> bool {
    match call_command(env, "_aiEdits.hasAiContributions", vec![uris_argument(uris), Dynamic::string(level.as_str())]) {
        Dynamic::Bool(value) => value,
        other => panic!("o comando devolveu algo que não é booleano: {}", other.type_of()),
    }
}

fn clear_ai_contributions(env: &Env, uris: &[&URI]) {
    call_command(env, "_aiEdits.clearAiContributions", vec![uris_argument(uris)]);
}

fn clear_all_ai_contributions(env: &Env) {
    call_command(env, "_aiEdits.clearAllAiContributions", Vec::new());
}

fn file_a() -> URI {
    URI::parse("file:///a.ts", false)
}

fn file_b() -> URI {
    URI::parse("file:///b.ts", false)
}

fn options(uri: URI, initial_value: &str) -> CreateDocumentOptions {
    CreateDocumentOptions { uri: Some(uri), initial_value: Some(initial_value.to_owned()), ..Default::default() }
}

#[test]
fn no_contributions_initially() {
    run(async {
        let env = setup();
        let d = env.disposables.add_rc(env.workspace.create_document(options(file_a(), "hello"), None));
        sleep(1500).await;
        assert!(!has_ai_contributions(&env, &[d.uri()], AiContributionLevel::All));
        assert!(!has_ai_contributions(&env, &[d.uri()], AiContributionLevel::ChatAndAgent));
        env.disposables.dispose();
    });
}

#[test]
fn detects_chat_ai_edits() {
    run(async {
        let env = setup();
        let d = env.disposables.add_rc(env.workspace.create_document(options(file_a(), "hello"), None));
        sleep(1500).await;

        d.apply_edit(StringEditWithReason::replace(d.find_range("hello"), "world", Some(chat_edit())), None, None);
        sleep(1500).await;

        assert!(has_ai_contributions(&env, &[d.uri()], AiContributionLevel::All));
        assert!(has_ai_contributions(&env, &[d.uri()], AiContributionLevel::ChatAndAgent));
        env.disposables.dispose();
    });
}

#[test]
fn detects_inline_completion_ai_edits_at_all_level_only() {
    run(async {
        let env = setup();
        let d = env.disposables.add_rc(env.workspace.create_document(options(file_a(), "hello"), None));
        sleep(1500).await;

        d.apply_edit(StringEditWithReason::replace(d.find_range("hello"), "world", Some(inline_completion_edit())), None, None);
        sleep(1500).await;

        assert!(has_ai_contributions(&env, &[d.uri()], AiContributionLevel::All));
        assert!(!has_ai_contributions(&env, &[d.uri()], AiContributionLevel::ChatAndAgent));
        env.disposables.dispose();
    });
}

#[test]
fn does_not_detect_user_edits_as_ai() {
    run(async {
        let env = setup();
        let d = env.disposables.add_rc(env.workspace.create_document(options(file_a(), "hello"), None));
        sleep(1500).await;

        d.apply_edit(StringEditWithReason::replace(d.find_range("hello"), "world", Some(user_edit())), None, None);
        sleep(1500).await;

        assert!(!has_ai_contributions(&env, &[d.uri()], AiContributionLevel::All));
        assert!(!has_ai_contributions(&env, &[d.uri()], AiContributionLevel::ChatAndAgent));
        env.disposables.dispose();
    });
}

#[test]
fn clear_resets_contributions_for_specific_resources() {
    run(async {
        let env = setup();
        let d_a = env.disposables.add_rc(env.workspace.create_document(options(file_a(), "hello"), None));
        let d_b = env.disposables.add_rc(env.workspace.create_document(options(file_b(), "world"), None));
        sleep(1500).await;

        d_a.apply_edit(StringEditWithReason::replace(d_a.find_range("hello"), "foo", Some(chat_edit())), None, None);
        d_b.apply_edit(StringEditWithReason::replace(d_b.find_range("world"), "bar", Some(chat_edit())), None, None);
        sleep(1500).await;

        assert!(has_ai_contributions(&env, &[d_a.uri()], AiContributionLevel::All));
        assert!(has_ai_contributions(&env, &[d_b.uri()], AiContributionLevel::All));

        clear_ai_contributions(&env, &[d_a.uri()]);

        assert!(!has_ai_contributions(&env, &[d_a.uri()], AiContributionLevel::All));
        assert!(has_ai_contributions(&env, &[d_b.uri()], AiContributionLevel::All));
        env.disposables.dispose();
    });
}

#[test]
fn clear_all_resets_all_contributions() {
    run(async {
        let env = setup();
        let d_a = env.disposables.add_rc(env.workspace.create_document(options(file_a(), "hello"), None));
        let d_b = env.disposables.add_rc(env.workspace.create_document(options(file_b(), "world"), None));
        sleep(1500).await;

        d_a.apply_edit(StringEditWithReason::replace(d_a.find_range("hello"), "foo", Some(chat_edit())), None, None);
        d_b.apply_edit(StringEditWithReason::replace(d_b.find_range("world"), "bar", Some(chat_edit())), None, None);
        sleep(1500).await;

        clear_all_ai_contributions(&env);

        assert!(!has_ai_contributions(&env, &[d_a.uri()], AiContributionLevel::All));
        assert!(!has_ai_contributions(&env, &[d_b.uri()], AiContributionLevel::All));
        env.disposables.dispose();
    });
}

#[test]
fn tracks_new_edits_after_clear() {
    run(async {
        let env = setup();
        let d = env.disposables.add_rc(env.workspace.create_document(options(file_a(), "hello"), None));
        sleep(1500).await;

        d.apply_edit(StringEditWithReason::replace(d.find_range("hello"), "world", Some(chat_edit())), None, None);
        sleep(1500).await;

        clear_ai_contributions(&env, &[d.uri()]);
        assert!(!has_ai_contributions(&env, &[d.uri()], AiContributionLevel::All));

        d.apply_edit(StringEditWithReason::replace(d.find_range("world"), "again", Some(chat_edit())), None, None);
        sleep(1500).await;

        assert!(has_ai_contributions(&env, &[d.uri()], AiContributionLevel::All));
        env.disposables.dispose();
    });
}

#[test]
fn cleans_up_tracker_when_document_is_closed() {
    run(async {
        let env = setup();
        let d = env.disposables.add_rc(env.workspace.create_document(options(file_a(), "hello"), None));
        sleep(1500).await;

        d.apply_edit(StringEditWithReason::replace(d.find_range("hello"), "world", Some(chat_edit())), None, None);
        sleep(1500).await;

        assert!(has_ai_contributions(&env, &[d.uri()], AiContributionLevel::All));

        d.dispose();
        sleep(1500).await;

        assert!(!has_ai_contributions(&env, &[&file_a()], AiContributionLevel::All));
        env.disposables.dispose();
    });
}

#[test]
fn returns_false_for_unknown_uris() {
    run(async {
        let env = setup();
        assert!(!has_ai_contributions(&env, &[&URI::parse("file:///unknown.ts", false)], AiContributionLevel::All));
        env.disposables.dispose();
    });
}

#[test]
fn checks_multiple_resources() {
    run(async {
        let env = setup();
        let d_a = env.disposables.add_rc(env.workspace.create_document(options(file_a(), "hello"), None));
        env.disposables.add_rc(env.workspace.create_document(options(file_b(), "world"), None));
        sleep(1500).await;

        d_a.apply_edit(StringEditWithReason::replace(d_a.find_range("hello"), "foo", Some(chat_edit())), None, None);
        sleep(1500).await;

        // Returns true if any of the resources has AI contributions
        assert!(has_ai_contributions(&env, &[&file_a(), &file_b()], AiContributionLevel::All));
        assert!(has_ai_contributions(&env, &[&file_b(), &file_a()], AiContributionLevel::All));
        assert!(!has_ai_contributions(&env, &[&file_b()], AiContributionLevel::All));
        env.disposables.dispose();
    });
}
