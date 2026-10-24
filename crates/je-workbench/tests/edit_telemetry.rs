//! Porte de `src/vs/workbench/contrib/editTelemetry/test/browser/editTelemetry.test.ts` (suíte `Edit Telemetry`), caso a
//! caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` é o `LeakGuard`, criado no início de cada caso (o caso `1`, dentro do
//!   relógio virtual de [`run`], para a conferência de vazamento rodar antes dos timers que sobraram);
//!   `runWithFakedTimers({}, async () => ...)` é o `run_with_faked_timers`, e `await timeout(ms)` é [`sleep`].
//! - `new TestInstantiationService(new ServiceCollection([...SyncDescriptor...]), false, undefined, true)` é um
//!   `InstantiationService` não estrito sobre um `ServiceCollection`, com o `SyncDescriptor` de cada serviço, descartado
//!   junto do `DisposableStore` do caso. `instantiationService.stub(I, x)` é a instância registrada na coleção;
//!   `createInstance(X)` é `create_instance`, e o `new AnnotatedDocuments(w, insta)` e o `new
//!   EditSourceTrackingImpl(..., insta)` do original são `create_instance` do mesmo serviço, que injeta a si mesmo.
//! - `stubInstance(UserAttentionServiceEnv, { isUserActive, isVsCodeFocused, dispose })` é o [`StubUserAttentionEnv`]
//!   registrado como `IUserAttentionServiceEnv` (o `UserAttentionService` do porte recebe o adaptador por dependência,
//!   ver `user_attention_browser`). `stubInstance(DiffService, { computeDiff: ... computeStringDiff(original, modified,
//!   { maxComputationTimeMs: 500 }, 'advanced') })` é o `DiffService` de verdade sobre o [`TestEditorWorkerService`], que usa
//!   o `compute_string_edit_from_diff` padrão (o mesmo `computeStringDiff` com 500 ms e o algoritmo `advanced`).
//!   `stubInstance(ScmAdapter, { getRepo: () => undefined })` e `stubInstance(UriVisibilityProvider, { isVisible: () =>
//!   true })` são os serviços `I_SCM_ADAPTER` e `I_URI_VISIBILITY_PROVIDER`. `stub(ITextFileService, { isDirty: () =>
//!   false })` é o `StubTextFileService` do suporte, e `stub(ILogService, new NullLogService())` é o `NullLogService`.
//! - O `publicLog2(eventName, data)` dos dois `stub(ITelemetryService, ...)` é o [`RecordingTelemetryService`], que guarda
//!   nome, dado e a hora (`Date.now()`) de cada evento; o formato `${formatTime(Date.now())} ${eventName}:
//!   ${JSON.stringify(data)}` é [`format_event`], com o `json_stringify` do porte (as chaves `undefined` somem, como no
//!   `JSON.stringify`).
//! - `DeterministicRandomService extends RandomService` sobrescreve só o `generateUuid`, e o `generatePrefixedUuid` do pai
//!   o chama; por isso o [`DeterministicRandomService`] monta o prefixado sobre o próprio `generate_uuid`. O `Random.create(0)`
//!   é o `Random` de `je-editor/tests/support/random.rs` (com o `next_uuid` acrescentado).
//! - O `sentTelemetry` do caso 0 projeta cada evento num objeto literal, e `assert.deepStrictEqual` é `assert_eq!` sobre
//!   tuplas com os mesmos campos, na mesma ordem; `undefined` é `None`. Os campos `...baseData`, `...ids` e `{ ...baseData,
//!   x }` são a atualização de struct (`..base_data()`).
//! - `FakeAnnotatedDocuments` é exportado pelo original mas nenhum caso o usa, então não é portado;
//!   `MutableObservableWorkspace` e `MutableObservableDocument` estão em `support/edit_source_tracking_impl_support.rs`.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
#[path = "../../je-editor/tests/support/random.rs"]
mod random;
#[path = "support/main_thread_support.rs"]
mod main_thread_support;
#[path = "support/edit_source_tracking_impl_support.rs"]
mod support;

use std::cell::RefCell;
use std::future::Future;
use std::rc::Rc;

use common::{ensure_no_disposables_are_leaked_in_test_suite, run_with_faked_timers, timeout};
use je_base::dynamic::{Dynamic, json_stringify};
use je_base::lifecycle::{DisposableStore, IDisposable};
use je_base::observable::observables::observable_value::ObservableValue;
use je_base::observable::{IObservable, Observable, SettableExt, const_observable, observable_value};
use je_base::promise::Promise;
use je_base::runtime;
use je_base::uri::URI;
use je_editor::edits::string_edit::StringReplacement;
use je_editor::editor_worker::{I_EDITOR_WORKER_SERVICE, IEditorWorkerService, WordRanges};
use je_editor::range::Range;
use je_editor::text_model_edit_source::{ChatApplyEditsData, EditSources};
use je_instantiation::{InstantiationService, ServiceCollection, SyncDescriptor};
use je_platform::log::{I_LOG_SERVICE, NullLogService};
use je_platform::telemetry::telemetry::{CommonPropertyValue, I_TELEMETRY_SERVICE, ITelemetryData, ITelemetryService, TelemetryLevel};
use je_workbench::contrib::edit_telemetry::browser::helpers::annotated_documents::{AnnotatedDocuments, I_URI_VISIBILITY_PROVIDER, IAnnotatedDocuments, IUriVisibilityProvider};
use je_workbench::contrib::edit_telemetry::browser::helpers::observable_workspace::{ObservableWorkspace, StringEditWithReason};
use je_workbench::contrib::edit_telemetry::browser::random_service::{I_RANDOM_SERVICE, IRandomService};
use je_workbench::contrib::edit_telemetry::browser::telemetry::ai_edit_telemetry::ai_edit_telemetry_service::{
    AcceptanceMethod, EditTelemetryBaseData, EditTelemetryCodeAcceptedData, EditTelemetryCodeRejectedData, EditTelemetryCodeSuggestedData, EditTelemetryFeature,
    EditTelemetryPresentation, I_AI_EDIT_TELEMETRY_SERVICE, IAiEditTelemetryService, RejectionMethod,
};
use je_workbench::contrib::edit_telemetry::browser::telemetry::ai_edit_telemetry::ai_edit_telemetry_service_impl::AiEditTelemetryServiceImpl;
use je_workbench::contrib::edit_telemetry::browser::telemetry::edit_source_tracking_impl::EditSourceTrackingImpl;
use je_workbench::contrib::edit_telemetry::browser::telemetry::scm_adapter::{I_SCM_ADAPTER, IScmAdapter, IScmRepoAdapter};
use je_workbench::services::textfile::textfiles::I_TEXT_FILE_SERVICE;
use je_workbench::services::user_attention::user_attention_browser::{I_USER_ATTENTION_SERVICE_ENV, IUserAttentionServiceEnv, UserAttentionService};
use je_workbench::services::user_attention::user_attention_service::{I_USER_ATTENTION_SERVICE, IUserAttentionService};
use main_thread_support::StubTextFileService;
use random::Random;
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

/// O `publicLog2` dos `stub(ITelemetryService, ...)`: guarda o nome, o dado e a hora de cada evento.
#[derive(Default)]
struct RecordingTelemetryService {
    events: RefCell<Vec<RecordedEvent>>,
}

#[derive(Clone)]
struct RecordedEvent {
    event_name: String,
    data: Option<Dynamic>,
    time_ms: u64,
}

impl RecordedEvent {
    /// `event.data?.<key>` como texto.
    fn text(&self, key: &str) -> Option<String> {
        match self.data.as_ref()?.get_str(key) {
            Dynamic::String(value) => Some(value.to_string()),
            _ => None,
        }
    }

    /// `event.data?.<key>` como booleano.
    fn flag(&self, key: &str) -> Option<bool> {
        match self.data.as_ref()?.get_str(key) {
            Dynamic::Bool(value) => Some(value),
            _ => None,
        }
    }
}

impl ITelemetryService for RecordingTelemetryService {
    fn telemetry_level(&self) -> TelemetryLevel {
        TelemetryLevel::Usage
    }
    fn session_id(&self) -> String {
        String::new()
    }
    fn machine_id(&self) -> String {
        String::new()
    }
    fn sqm_id(&self) -> String {
        String::new()
    }
    fn dev_device_id(&self) -> String {
        String::new()
    }
    fn first_session_date(&self) -> String {
        String::new()
    }
    fn send_error_telemetry(&self) -> bool {
        false
    }
    fn public_log(&self, _event_name: &str, _data: Option<ITelemetryData>) {}
    fn public_log2(&self, event_name: &str, data: Option<Dynamic>) {
        self.events.borrow_mut().push(RecordedEvent { event_name: event_name.to_owned(), data, time_ms: runtime::date_now() });
    }
    fn public_log_error(&self, _error_event_name: &str, _data: Option<ITelemetryData>) {}
    fn public_log_error2(&self, _event_name: &str, _data: Option<Dynamic>) {}
    fn set_experiment_property(&self, _name: &str, _value: &str) {}
    fn set_common_property(&self, _name: &str, _value: Option<CommonPropertyValue>) {}
}

/// `formatTime(timeMs)`.
fn format_time(time_ms: u64) -> String {
    let minutes = time_ms / 60000;
    let seconds = (time_ms % 60000) / 1000;
    let ms = time_ms % 1000;
    format!("{minutes:02}:{seconds:02}:{ms:03}")
}

/// `${formatTime(Date.now())} ${eventName}: ${JSON.stringify(data)}`.
fn format_event(event: &RecordedEvent) -> String {
    let data = event.data.as_ref().and_then(json_stringify).unwrap_or_else(|| "undefined".to_owned());
    format!("{} {}: {}", format_time(event.time_ms), event.event_name, data)
}

/// `DeterministicRandomService`.
struct DeterministicRandomService {
    rand: RefCell<Random>,
}

impl DeterministicRandomService {
    fn new() -> Rc<dyn IRandomService> {
        Rc::new(Self { rand: RefCell::new(Random::create(0)) })
    }
}

impl IRandomService for DeterministicRandomService {
    fn generate_uuid(&self) -> String {
        self.rand.borrow_mut().next_uuid()
    }

    /// O `generatePrefixedUuid` do `RandomService` chama o `generateUuid` sobrescrito.
    fn generate_prefixed_uuid(&self, namespace: &str) -> String {
        format!("{namespace}-{}", self.generate_uuid())
    }
}

/// `stubInstance(UserAttentionServiceEnv, { isUserActive: userActive, isVsCodeFocused: constObservable(true), dispose })`.
struct StubUserAttentionEnv {
    user_active: Rc<ObservableValue<bool, ()>>,
}

impl IUserAttentionServiceEnv for StubUserAttentionEnv {
    fn is_vs_code_focused(&self) -> IObservable<bool> {
        const_observable(true)
    }

    fn is_user_active(&self) -> IObservable<bool> {
        self.user_active.to_observable()
    }
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

/// `stubInstance(ScmAdapter, { getRepo: (uri, reader) => undefined })`.
struct TestScmAdapter;

impl IScmAdapter for TestScmAdapter {
    fn get_repo(&self, _uri: &URI, _reader: Option<&dyn je_base::observable::IReader>) -> Option<Rc<dyn IScmRepoAdapter>> {
        None
    }
}

/// `stubInstance(UriVisibilityProvider, { isVisible: (uri, reader) => true })`.
struct TestUriVisibilityProvider;

impl IUriVisibilityProvider for TestUriVisibilityProvider {
    fn is_visible(&self, _uri: &URI, _reader: &dyn je_base::observable::IReader) -> bool {
        true
    }
}

/// O `baseData` do primeiro caso.
fn base_data() -> EditTelemetryBaseData {
    EditTelemetryBaseData {
        suggestion_id: None,
        feature: Some(EditTelemetryFeature::InlineChat),
        presentation: EditTelemetryPresentation::HighlightedEdit,
        source: None,
        language_id: None,
        edit_delta_info: None,
        mode_id: None,
        apply_code_block_suggestion_id: None,
        model_id: None,
        source_request_id: None,
        is_agent_host_session: None,
    }
}

fn some(value: &str) -> Option<String> {
    Some(value.to_owned())
}

#[test]
fn reports_agent_host_session_mode_for_accepted_and_rejected_edits() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let telemetry = Rc::new(RecordingTelemetryService::default());
    let telemetry_service: Rc<dyn ITelemetryService> = telemetry.clone();
    let instantiation_service = guard.add_rc(InstantiationService::new(
        ServiceCollection::new().with(I_TELEMETRY_SERVICE, telemetry_service).with(I_RANDOM_SERVICE, DeterministicRandomService::new()),
    ));
    let ai_edit_telemetry_service = instantiation_service.create_instance::<AiEditTelemetryServiceImpl>(());

    ai_edit_telemetry_service.create_suggestion_id(&EditTelemetryCodeSuggestedData {
        base: EditTelemetryBaseData { is_agent_host_session: Some(true), ..base_data() },
        provider: None,
    });
    ai_edit_telemetry_service.handle_code_accepted(&EditTelemetryCodeAcceptedData {
        base: EditTelemetryBaseData {
            presentation: EditTelemetryPresentation::CodeBlock,
            feature: Some(EditTelemetryFeature::SideBarChat),
            is_agent_host_session: Some(true),
            source_request_id: some("request-origin"),
            ..base_data()
        },
        agent_session_id: None,
        provider: some("copilotcli"),
        chat_session_id: some("agent-host-copilotcli:/session#chat"),
        acceptance_method: AcceptanceMethod::CopyButton,
    });
    ai_edit_telemetry_service.handle_code_rejected(&EditTelemetryCodeRejectedData {
        base: EditTelemetryBaseData { is_agent_host_session: Some(false), ..base_data() },
        agent_session_id: None,
        chat_session_id: None,
        rejection_method: RejectionMethod::Reject,
    });

    let events = telemetry.events.borrow().clone();
    let actual: Vec<_> = events
        .iter()
        .map(|event| (event.event_name.clone(), event.flag("isAgentHostSession"), event.text("sourceRequestId"), event.text("chatSessionId"), event.text("provider")))
        .collect();
    assert_eq!(
        actual,
        vec![
            ("editTelemetry.codeSuggested".to_owned(), Some(true), None, None, some("unknown")),
            ("editTelemetry.codeAccepted".to_owned(), Some(true), some("request-origin"), some("agent-host-copilotcli:/session#chat"), some("copilotcli")),
            ("editTelemetry.codeRejected".to_owned(), Some(false), None, None, None),
        ]
    );

    let source_request_id = || some("source-turn");
    let chat_session_id = || some("agent-host-copilotcli:/owner#peer");
    let agent_session_id = || some("owner");
    ai_edit_telemetry_service.handle_code_accepted(&EditTelemetryCodeAcceptedData {
        base: EditTelemetryBaseData { source_request_id: source_request_id(), is_agent_host_session: Some(true), ..base_data() },
        agent_session_id: agent_session_id(),
        provider: None,
        chat_session_id: chat_session_id(),
        acceptance_method: AcceptanceMethod::Accept,
    });
    ai_edit_telemetry_service.handle_code_rejected(&EditTelemetryCodeRejectedData {
        base: EditTelemetryBaseData { source_request_id: source_request_id(), is_agent_host_session: Some(true), ..base_data() },
        agent_session_id: agent_session_id(),
        chat_session_id: chat_session_id(),
        rejection_method: RejectionMethod::Reject,
    });
    let events = telemetry.events.borrow().clone();
    let last_two: Vec<_> = events[events.len() - 2..]
        .iter()
        .map(|event| (event.event_name.clone(), event.text("agentSessionId"), event.text("chatSessionId"), event.text("sourceRequestId")))
        .collect();
    let expected: Vec<_> = ["editTelemetry.codeAccepted", "editTelemetry.codeRejected"]
        .iter()
        .map(|event_name| ((*event_name).to_owned(), agent_session_id(), chat_session_id(), source_request_id()))
        .collect();
    assert_eq!(last_two, expected);
}

#[test]
fn case_1() {
    run(async {
        let disposables = DisposableStore::new();
        let user_active = observable_value::<bool, ()>("userActive", true);
        let telemetry = Rc::new(RecordingTelemetryService::default());
        let telemetry_service: Rc<dyn ITelemetryService> = telemetry.clone();
        let user_attention_env: Rc<dyn IUserAttentionServiceEnv> = Rc::new(StubUserAttentionEnv { user_active: user_active.clone() });
        let editor_worker_service: Rc<dyn IEditorWorkerService> = Rc::new(TestEditorWorkerService);
        let scm_adapter: Rc<dyn IScmAdapter> = Rc::new(TestScmAdapter);
        let uri_visibility_provider: Rc<dyn IUriVisibilityProvider> = Rc::new(TestUriVisibilityProvider);
        let instantiation_service = disposables.add_rc(InstantiationService::new(
            ServiceCollection::new()
                .with(I_AI_EDIT_TELEMETRY_SERVICE, SyncDescriptor::<dyn IAiEditTelemetryService>::new::<AiEditTelemetryServiceImpl>(()))
                .with(I_USER_ATTENTION_SERVICE, SyncDescriptor::<dyn IUserAttentionService>::new::<UserAttentionService>(()))
                .with(I_USER_ATTENTION_SERVICE_ENV, user_attention_env)
                .with(I_TELEMETRY_SERVICE, telemetry_service)
                .with(I_EDITOR_WORKER_SERVICE, editor_worker_service)
                .with(I_SCM_ADAPTER, scm_adapter)
                .with(I_URI_VISIBILITY_PROVIDER, uri_visibility_provider)
                .with(I_RANDOM_SERVICE, DeterministicRandomService::new())
                .with(I_LOG_SERVICE, NullLogService::shared())
                .with(I_TEXT_FILE_SERVICE, Rc::new(StubTextFileService) as Rc<dyn je_workbench::services::textfile::textfiles::ITextFileService>),
        ));

        let w = MutableObservableWorkspace::new();
        let workspace: Rc<dyn ObservableWorkspace> = w.clone();
        let docs = disposables.add_rc(instantiation_service.create_instance::<AnnotatedDocuments>(workspace));
        let annotated: Rc<dyn IAnnotatedDocuments> = docs.clone();
        disposables.add_rc(instantiation_service.create_instance::<EditSourceTrackingImpl>((const_observable(true), annotated, None)));

        let d1 = disposables.add_rc(w.create_document(
            CreateDocumentOptions {
                uri: Some(URI::parse("file:///a", false)),
                initial_value: Some("\nfunction fib(n) {\n\tif (n <= 1) return n;\n\treturn fib(n - 1) + fib(n - 2);\n}\n".to_owned()),
                ..Default::default()
            },
            None,
        ));

        sleep(10).await;

        let chat_edit = Rc::new(EditSources::chat_apply_edits(&ChatApplyEditsData { language_id: "plaintext".to_owned(), ..Default::default() }));

        d1.apply_edit(StringEditWithReason::replace(d1.find_range("≪≫function fib(n) {"), "// Computes the nth fibonacci number\n", Some(chat_edit.clone())), None, None);

        sleep(5000).await;

        d1.apply_edit(
            StringEditWithReason::new(
                vec![StringReplacement::replace(d1.find_range("≪//≫ Computes"), "/*"), StringReplacement::replace(d1.find_range("fibonacci number≪≫"), " */")],
                Rc::new(EditSources::cursor("type", None)),
            ),
            None,
            None,
        );

        sleep(5000).await;

        d1.apply_edit(StringEditWithReason::replace(d1.find_range("Computes the nth fibonacci number"), "Berechnet die nte Fibonacci Zahl", Some(chat_edit)), None, None);

        sleep(3 * 60 * 1000).await;
        user_active.set(false, None);
        sleep(3 * 60 * 1000).await;
        user_active.set(true, None);
        sleep(18 * 60 * 1000).await;

        let sent_telemetry: Vec<String> = telemetry.events.borrow().iter().map(format_event).collect();
        let expected: Vec<String> = [
            r#"00:01:010 editTelemetry.reportEditArc: {"sourceKeyCleaned":"source:Chat.applyEdits","languageId":"plaintext","uniqueEditId":"8c97b7d8-9adb-4bd8-ac9f-a562704ce40e","didBranchChange":0,"timeDelayMs":0,"originalCharCount":37,"originalLineCount":1,"originalDeletedLineCount":0,"arc":37,"currentLineCount":1,"currentDeletedLineCount":0}"#,
            r#"00:01:010 editTelemetry.codeSuggested: {"eventId":"evt-055ed5f5-c723-4ede-ba79-cccd7685c7ad","suggestionId":"sgt-f645627a-cacf-477a-9164-ecd6125616a5","presentation":"highlightedEdit","feature":"sideBarChat","languageId":"plaintext","editCharsInserted":37,"editCharsDeleted":0,"editLinesInserted":1,"editLinesDeleted":0}"#,
            r#"00:11:010 editTelemetry.reportEditArc: {"sourceKeyCleaned":"source:Chat.applyEdits","languageId":"plaintext","uniqueEditId":"1eb8a394-2489-41c2-851b-6a79432fc6bc","didBranchChange":0,"timeDelayMs":0,"originalCharCount":19,"originalLineCount":1,"originalDeletedLineCount":1,"arc":19,"currentLineCount":1,"currentDeletedLineCount":1}"#,
            r#"00:11:010 editTelemetry.codeSuggested: {"eventId":"evt-5c9c6fe7-b219-4ff8-aaa7-ab2b355b21c0","suggestionId":"sgt-74379122-0452-4e26-9c38-9d62f1e7ae73","presentation":"highlightedEdit","feature":"sideBarChat","languageId":"plaintext","editCharsInserted":19,"editCharsDeleted":20,"editLinesInserted":1,"editLinesDeleted":1}"#,
            r#"01:01:010 editTelemetry.reportEditArc: {"sourceKeyCleaned":"source:Chat.applyEdits","languageId":"plaintext","uniqueEditId":"8c97b7d8-9adb-4bd8-ac9f-a562704ce40e","didBranchChange":0,"timeDelayMs":60000,"originalCharCount":37,"originalLineCount":1,"originalDeletedLineCount":0,"arc":16,"currentLineCount":1,"currentDeletedLineCount":0}"#,
            r#"01:11:010 editTelemetry.reportEditArc: {"sourceKeyCleaned":"source:Chat.applyEdits","languageId":"plaintext","uniqueEditId":"1eb8a394-2489-41c2-851b-6a79432fc6bc","didBranchChange":0,"timeDelayMs":60000,"originalCharCount":19,"originalLineCount":1,"originalDeletedLineCount":1,"arc":19,"currentLineCount":1,"currentDeletedLineCount":1}"#,
            r#"05:01:010 editTelemetry.reportEditArc: {"sourceKeyCleaned":"source:Chat.applyEdits","languageId":"plaintext","uniqueEditId":"8c97b7d8-9adb-4bd8-ac9f-a562704ce40e","didBranchChange":0,"timeDelayMs":300000,"originalCharCount":37,"originalLineCount":1,"originalDeletedLineCount":0,"arc":16,"currentLineCount":1,"currentDeletedLineCount":0}"#,
            r#"05:11:010 editTelemetry.reportEditArc: {"sourceKeyCleaned":"source:Chat.applyEdits","languageId":"plaintext","uniqueEditId":"1eb8a394-2489-41c2-851b-6a79432fc6bc","didBranchChange":0,"timeDelayMs":300000,"originalCharCount":19,"originalLineCount":1,"originalDeletedLineCount":1,"arc":19,"currentLineCount":1,"currentDeletedLineCount":1}"#,
            r#"12:00:000 editTelemetry.editSources.details: {"mode":"10minFocusWindow","sourceKey":"source:Chat.applyEdits","sourceKeyCleaned":"source:Chat.applyEdits","trigger":"time","languageId":"plaintext","statsUuid":"509b5d53-9109-40a2-bdf5-1aa735a229fe","modifiedCount":35,"deltaModifiedCount":56,"totalModifiedCount":39}"#,
            r#"12:00:000 editTelemetry.editSources.details: {"mode":"10minFocusWindow","sourceKey":"source:cursor-kind:type","sourceKeyCleaned":"source:cursor-kind:type","trigger":"time","languageId":"plaintext","statsUuid":"509b5d53-9109-40a2-bdf5-1aa735a229fe","modifiedCount":4,"deltaModifiedCount":4,"totalModifiedCount":39}"#,
            r#"12:00:000 editTelemetry.editSources.stats: {"attributionSchemaVersion":2,"mode":"10minFocusWindow","languageId":"plaintext","statsUuid":"509b5d53-9109-40a2-bdf5-1aa735a229fe","nesModifiedCount":0,"inlineCompletionsCopilotModifiedCount":0,"inlineCompletionsNESModifiedCount":0,"otherAIModifiedCount":35,"agentHostModifiedCount":0,"unknownModifiedCount":0,"userModifiedCount":4,"ideModifiedCount":0,"totalModifiedCharacters":39,"externalModifiedCount":0,"isTrackedByGit":0,"focusTime":600000,"actualTime":720000,"trigger":"time"}"#,
            r#"22:00:000 editTelemetry.editSources.details: {"mode":"20minFocusWindow","sourceKey":"source:Chat.applyEdits","sourceKeyCleaned":"source:Chat.applyEdits","trigger":"time","languageId":"plaintext","statsUuid":"a794406a-7779-4e9f-a856-1caca85123c7","modifiedCount":35,"deltaModifiedCount":56,"totalModifiedCount":39}"#,
            r#"22:00:000 editTelemetry.editSources.details: {"mode":"20minFocusWindow","sourceKey":"source:cursor-kind:type","sourceKeyCleaned":"source:cursor-kind:type","trigger":"time","languageId":"plaintext","statsUuid":"a794406a-7779-4e9f-a856-1caca85123c7","modifiedCount":4,"deltaModifiedCount":4,"totalModifiedCount":39}"#,
            r#"22:00:000 editTelemetry.editSources.stats: {"attributionSchemaVersion":2,"mode":"20minFocusWindow","languageId":"plaintext","statsUuid":"a794406a-7779-4e9f-a856-1caca85123c7","nesModifiedCount":0,"inlineCompletionsCopilotModifiedCount":0,"inlineCompletionsNESModifiedCount":0,"otherAIModifiedCount":35,"agentHostModifiedCount":0,"unknownModifiedCount":0,"userModifiedCount":4,"ideModifiedCount":0,"totalModifiedCharacters":39,"externalModifiedCount":0,"isTrackedByGit":0,"focusTime":1200000,"actualTime":1320000,"trigger":"time"}"#,
        ]
        .iter()
        .map(|line| (*line).to_owned())
        .collect();
        assert_eq!(sent_telemetry, expected);

        disposables.dispose();
    });
}
