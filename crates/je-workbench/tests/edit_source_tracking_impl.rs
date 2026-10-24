//! Porte de `src/vs/workbench/contrib/editTelemetry/test/browser/editSourceTrackingImpl.test.ts` (suíte `Edit Source
//! Tracking Windows`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` é o `LeakGuard`, criado dentro do relógio virtual de cada caso
//!   ([`run`]); `runWithFakedTimers({}, async () => ...)` é o `run_with_faked_timers`, e `await timeout(ms)` é
//!   [`sleep`] (o `timeout` do relógio virtual).
//! - `new TestInstantiationService(new ServiceCollection(), false, undefined, true)` é um `InstantiationService` não
//!   estrito sobre um `ServiceCollection`, descartado junto do `DisposableStore` do caso. O `new EditSourceTrackingImpl(...,
//!   instantiationService)` e o `new AnnotatedDocuments(workspace, instantiationService)` do original são
//!   `create_instance` do mesmo serviço, que injeta a si mesmo.
//! - `instantiationService.stubInstance(DiffService, { computeDiff: ... computeStringDiff(original, modified, {
//!   maxComputationTimeMs: 500 }, 'advanced') })` é o `DiffService` de verdade sobre um `IEditorWorkerService` que usa o
//!   `compute_string_edit_from_diff` padrão (o mesmo `computeStringDiff` com 500 ms e o algoritmo `advanced`). O
//!   `stubInstance(ScmAdapter, ...)` e o `stubInstance(UriVisibilityProvider, ...)` são os serviços `I_SCM_ADAPTER` e
//!   `I_URI_VISIBILITY_PROVIDER` (ver `scm_adapter` e `annotated_documents`). Os `stub(ITelemetryService, ...)`,
//!   `stub(IRandomService, ...)`, `stub(IUserAttentionService, ...)`, `stub(ITextFileService, { isDirty })`,
//!   `stub(IAiEditTelemetryService, ...)` e `stub(ILogService, new NullLogService())` são serviços registrados na coleção;
//!   o `ITextFileService` leva as demais operações como `unreachable!` (o caso não as chama), como nos demais testes.
//! - O dado de cada evento de telemetria (`data as typeof allDetails[number]`) é lido como [`Dynamic`] e projetado em
//!   [`Detail`] e [`Stats`]; `Object.hasOwn(event, 'chatSessionId')` é a presença da chave no objeto. `localeCompare`
//!   dos casos 8, 9 e 6 é a ordem de bytes (os textos comparados são ASCII e a ordem é a mesma).
//! - `assert.deepStrictEqual` de objetos literais é `assert_eq!` sobre tuplas e structs locais com os mesmos campos, na
//!   mesma ordem. `undefined` é `None`.
//! - `TestExternalEditCorrelation` guarda os emissores e a promessa de drenagem; os objetos literais que implementam
//!   `IExternalEditCorrelation` são o [`StubCorrelation`], e os que implementam `IAgentHostEditMarkerService` são o
//!   [`StubMarkerService`] (`takeCoverageGap` ausente é `None`).
//! - As funções `async` dos stubs que lançam são promessas rejeitadas; as que só devolvem valor são promessas resolvidas.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
#[path = "support/edit_source_tracking_impl_support.rs"]
mod support;

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::rc::Rc;

use common::{ensure_no_disposables_are_leaked_in_test_suite, run_with_faked_timers, timeout};
use je_base::JsString;
use je_base::async_::async_fn;
use je_base::buffer::VSBufferReadableStream;
use je_base::dynamic::Dynamic;
use je_base::errors::Error;
use je_base::event::{Emitter, Event};
use je_base::lifecycle::{DisposableStore, IDisposable, none};
use je_base::observable::observables::observable_value::ObservableValue;
use je_base::observable::{IObservable, IReader, Observable, SettableExt, const_observable, observable_value};
use je_base::path::Platform;
use je_base::promise::{DeferredPromise, Promise};
use je_base::stream::ReadableStream;
use je_base::uri::URI;
use je_editor::editor_worker::{I_EDITOR_WORKER_SERVICE, IEditorWorkerService, WordRanges};
use je_editor::offset_range::OffsetRange;
use je_editor::range::Range;
use je_editor::text_model_edit_source::{
    AgentHostChatApplyEditsData, ChatApplyEditsData, EditSources, EditSuggestionId, TextModelEditSource, VersionedExtensionId,
};
use je_instantiation::{InstantiationService, ServiceCollection};
use je_platform::files::files::FileStatWithMetadata;
use je_platform::log::{I_LOG_SERVICE, NullLogService};
use je_platform::telemetry::edit_telemetry::EditTelemetryTrigger;
use je_platform::telemetry::telemetry::{CommonPropertyValue, I_TELEMETRY_SERVICE, ITelemetryData, ITelemetryService, TelemetryLevel};
use je_workbench::common::editor::RevertOptions;
use je_workbench::contrib::edit_telemetry::browser::helpers::annotated_documents::{AnnotatedDocuments, I_URI_VISIBILITY_PROVIDER, IAnnotatedDocuments, IUriVisibilityProvider};
use je_workbench::contrib::edit_telemetry::browser::helpers::observable_workspace::{IObservableDocument, ObservableWorkspace, StringEditWithReason};
use je_workbench::contrib::edit_telemetry::browser::random_service::{I_RANDOM_SERVICE, IRandomService};
use je_workbench::contrib::edit_telemetry::browser::telemetry::agent_host_edit_marker_service::{
    IAgentHostEditAttributionCoverageGap, IAgentHostEditMarkerService, IExternalEditCorrelation, IExternalEditCorrelationResolution,
    IPreparedAgentHostEditAttributionFlush, agent_host_edit_attribution_deferred_error, agent_host_edit_attribution_unknown_outcome_error,
};
use je_workbench::contrib::edit_telemetry::browser::telemetry::ai_edit_telemetry::ai_edit_telemetry_service::{
    EditTelemetryCodeAcceptedData, EditTelemetryCodeRejectedData, EditTelemetryCodeSuggestedData, I_AI_EDIT_TELEMETRY_SERVICE, IAiEditTelemetryService,
};
use je_workbench::contrib::edit_telemetry::browser::telemetry::edit_source_tracking_impl::EditSourceTrackingImpl;
use je_workbench::contrib::edit_telemetry::browser::telemetry::scm_adapter::{I_SCM_ADAPTER, IScmAdapter, IScmRepoAdapter};
use je_workbench::services::textfile::collaborators::FileOperationUndoRedoInfo;
use je_workbench::services::textfile::textfiles::{
    EncodedContent, I_TEXT_FILE_SERVICE, IResourceEncodings, ITextFileEditorModelManager, ITextFileService, ReadTextFileEncodingOptions, ReadTextFileOptions,
    ResolvedDecoding, ResolvedEncoding, TextFileContent, TextFileCreateOperation, TextFileSaveAsOptions, TextFileSaveOptions, TextFileStreamContent,
    TextOrSnapshot, WriteTextFileOptions,
};
use je_workbench::services::untitled::untitled_text_editor_service::IUntitledTextEditorModelManager;
use je_workbench::services::user_attention::user_attention_service::{I_USER_ATTENTION_SERVICE, IUserAttentionService};
use support::{CreateDocumentOptions, MutableObservableDocument, MutableObservableWorkspace};

// region: infraestrutura do caso

/// `ensureNoDisposablesAreLeakedInTestSuite()` e `runWithFakedTimers({}, async () => ...)`.
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

/// O dado de `editTelemetry.editSources.details`, como o `allDetails` do original o lê.
#[derive(Clone, Debug)]
struct Detail {
    mode: String,
    source_key: String,
    source_key_cleaned: String,
    origin: Option<String>,
    provider: Option<String>,
    model_id: Option<String>,
    auto_tier: Option<String>,
    fusion_model: Option<String>,
    conversation_id: Option<String>,
    chat_session_id: Option<String>,
    has_chat_session_id: bool,
    agent_session_id: Option<String>,
    request_id: Option<String>,
    stats_uuid: String,
    trigger: String,
    modified_count: usize,
    delta_modified_count: usize,
    total_modified_count: usize,
}

/// O dado de `editTelemetry.editSources.stats`.
#[derive(Clone, Debug)]
struct Stats {
    mode: String,
    stats_uuid: String,
    other_ai_modified_count: usize,
    agent_host_modified_count: usize,
    external_modified_count: usize,
    total_modified_characters: usize,
    agent_host_attribution_coverage: Option<String>,
    agent_host_untracked_edit_count: Option<usize>,
    agent_host_untracked_inserted_count: Option<usize>,
}

fn text(event: &Dynamic, key: &str) -> Option<String> {
    match event.get_str(key) {
        Dynamic::String(value) => Some(value.to_string()),
        _ => None,
    }
}

fn required_text(event: &Dynamic, key: &str) -> String {
    text(event, key).unwrap_or_else(|| panic!("o evento não tem o texto `{key}`"))
}

fn optional_count(event: &Dynamic, key: &str) -> Option<usize> {
    match event.get_str(key) {
        Dynamic::Number(value) => Some(value as usize),
        _ => None,
    }
}

fn count(event: &Dynamic, key: &str) -> usize {
    optional_count(event, key).unwrap_or_else(|| panic!("o evento não tem o número `{key}`"))
}

impl Detail {
    fn parse(event: &Dynamic) -> Self {
        Self {
            mode: required_text(event, "mode"),
            source_key: required_text(event, "sourceKey"),
            source_key_cleaned: required_text(event, "sourceKeyCleaned"),
            origin: text(event, "origin"),
            provider: text(event, "provider"),
            model_id: text(event, "modelId"),
            auto_tier: text(event, "autoTier"),
            fusion_model: text(event, "fusionModel"),
            conversation_id: text(event, "conversationId"),
            chat_session_id: text(event, "chatSessionId"),
            has_chat_session_id: event.own_keys().iter().any(|key| *key == "chatSessionId"),
            agent_session_id: text(event, "agentSessionId"),
            request_id: text(event, "requestId"),
            stats_uuid: required_text(event, "statsUuid"),
            trigger: required_text(event, "trigger"),
            modified_count: count(event, "modifiedCount"),
            delta_modified_count: count(event, "deltaModifiedCount"),
            total_modified_count: count(event, "totalModifiedCount"),
        }
    }
}

impl Stats {
    fn parse(event: &Dynamic) -> Self {
        Self {
            mode: required_text(event, "mode"),
            stats_uuid: required_text(event, "statsUuid"),
            other_ai_modified_count: count(event, "otherAIModifiedCount"),
            agent_host_modified_count: count(event, "agentHostModifiedCount"),
            external_modified_count: count(event, "externalModifiedCount"),
            total_modified_characters: count(event, "totalModifiedCharacters"),
            agent_host_attribution_coverage: text(event, "agentHostAttributionCoverage"),
            agent_host_untracked_edit_count: optional_count(event, "agentHostUntrackedEditCount"),
            agent_host_untracked_inserted_count: optional_count(event, "agentHostUntrackedInsertedCount"),
        }
    }
}

/// O `publicLog2` do `stub(ITelemetryService, ...)` do `setup`: guarda os eventos `details` e `stats`.
#[derive(Default)]
struct TestTelemetryService {
    details: RefCell<Vec<Dynamic>>,
    stats: RefCell<Vec<Dynamic>>,
}

impl ITelemetryService for TestTelemetryService {
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
        let Some(data) = data else { return };
        if event_name == "editTelemetry.editSources.details" {
            self.details.borrow_mut().push(data);
        } else if event_name == "editTelemetry.editSources.stats" {
            self.stats.borrow_mut().push(data);
        }
    }
    fn public_log_error(&self, _error_event_name: &str, _data: Option<ITelemetryData>) {}
    fn public_log_error2(&self, _event_name: &str, _data: Option<Dynamic>) {}
    fn set_experiment_property(&self, _name: &str, _value: &str) {}
    fn set_common_property(&self, _name: &str, _value: Option<CommonPropertyValue>) {}
}

/// O `IRandomService` do `setup`: `stats-N` e `<namespace>-N` com o mesmo contador.
struct TestRandomService {
    uuid: Cell<u32>,
}

impl TestRandomService {
    fn next(&self) -> u32 {
        self.uuid.set(self.uuid.get() + 1);
        self.uuid.get()
    }
}

impl IRandomService for TestRandomService {
    fn generate_uuid(&self) -> String {
        format!("stats-{}", self.next())
    }

    fn generate_prefixed_uuid(&self, namespace: &str) -> String {
        format!("{namespace}-{}", self.next())
    }
}

/// O `IUserAttentionService` do `setup`: tudo ligado, nenhum tempo de foco acumulado e nenhum disparo.
struct TestUserAttentionService;

impl IUserAttentionService for TestUserAttentionService {
    fn is_vs_code_focused(&self) -> IObservable<bool> {
        const_observable(true)
    }

    fn is_user_active(&self) -> IObservable<bool> {
        const_observable(true)
    }

    fn has_user_attention(&self) -> IObservable<bool> {
        const_observable(true)
    }

    fn total_focus_time_ms(&self) -> u64 {
        0
    }

    fn fire_after_given_focus_time_passed(&self, _focus_time_ms: u64, _callback: Box<dyn Fn()>) -> je_base::lifecycle::DisposableRef {
        none()
    }
}

/// O `IAiEditTelemetryService` do `setup`.
struct TestAiEditTelemetryService;

impl IAiEditTelemetryService for TestAiEditTelemetryService {
    fn create_suggestion_id(&self, _data: &EditTelemetryCodeSuggestedData) -> EditSuggestionId {
        EditSuggestionId::new_id(Some(&|_| "sgt-test".to_owned()))
    }

    fn handle_code_accepted(&self, _data: &EditTelemetryCodeAcceptedData) {}

    fn handle_code_rejected(&self, _data: &EditTelemetryCodeRejectedData) {}
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

/// O `repo` do `setup`.
struct TestRepo {
    head_hash: IObservable<Option<String>>,
    branch: IObservable<Option<String>>,
}

impl IScmRepoAdapter for TestRepo {
    fn head_branch_name_obs(&self) -> IObservable<Option<String>> {
        self.branch.clone()
    }

    fn head_commit_hash_obs(&self) -> IObservable<Option<String>> {
        self.head_hash.clone()
    }

    fn is_ignored(&self, _uri: &URI) -> Promise<bool> {
        Promise::resolved(false)
    }
}

/// `stubInstance(ScmAdapter, { getRepo: () => repo })`.
struct TestScmAdapter {
    repo: Rc<dyn IScmRepoAdapter>,
}

impl IScmAdapter for TestScmAdapter {
    fn get_repo(&self, _uri: &URI, _reader: Option<&dyn IReader>) -> Option<Rc<dyn IScmRepoAdapter>> {
        Some(self.repo.clone())
    }
}

/// `stubInstance(UriVisibilityProvider, { isVisible: (_uri, reader) => visible.read(reader) })`.
struct TestUriVisibilityProvider {
    visible: IObservable<bool>,
}

impl IUriVisibilityProvider for TestUriVisibilityProvider {
    fn is_visible(&self, _uri: &URI, reader: &dyn IReader) -> bool {
        self.visible.read(reader)
    }
}

/// `stub(ITextFileService, { isDirty: () => dirty })`: as demais operações o caso não chama.
struct TestTextFileService {
    dirty: bool,
}

impl ITextFileService for TestTextFileService {
    fn files(&self) -> Rc<dyn ITextFileEditorModelManager> {
        unreachable!("TestTextFileService.files não é exercitado por esta suíte")
    }

    fn untitled(&self) -> Rc<dyn IUntitledTextEditorModelManager> {
        unreachable!("TestTextFileService.untitled não é exercitado por esta suíte")
    }

    fn encoding(&self) -> Rc<dyn IResourceEncodings> {
        unreachable!("TestTextFileService.encoding não é exercitado por esta suíte")
    }

    fn is_dirty(&self, _resource: &URI) -> bool {
        self.dirty
    }

    fn save(&self, _resource: &URI, _options: Option<TextFileSaveOptions>) -> Promise<Option<URI>> {
        unreachable!("TestTextFileService.save não é exercitado por esta suíte")
    }

    fn save_as(&self, _resource: &URI, _target_resource: Option<&URI>, _options: Option<TextFileSaveAsOptions>) -> Promise<Option<URI>> {
        unreachable!("TestTextFileService.saveAs não é exercitado por esta suíte")
    }

    fn revert(&self, _resource: &URI, _options: Option<RevertOptions>) -> Promise<()> {
        unreachable!("TestTextFileService.revert não é exercitado por esta suíte")
    }

    fn read(&self, _resource: &URI, _options: Option<ReadTextFileOptions>) -> Promise<TextFileContent> {
        unreachable!("TestTextFileService.read não é exercitado por esta suíte")
    }

    fn read_stream(&self, _resource: &URI, _options: Option<ReadTextFileOptions>) -> Promise<TextFileStreamContent> {
        unreachable!("TestTextFileService.readStream não é exercitado por esta suíte")
    }

    fn write(&self, _resource: &URI, _value: TextOrSnapshot, _options: Option<WriteTextFileOptions>) -> Promise<FileStatWithMetadata> {
        unreachable!("TestTextFileService.write não é exercitado por esta suíte")
    }

    fn create(&self, _operations: Vec<TextFileCreateOperation>, _undo_info: Option<FileOperationUndoRedoInfo>) -> Promise<Vec<FileStatWithMetadata>> {
        unreachable!("TestTextFileService.create não é exercitado por esta suíte")
    }

    fn get_encoded_readable(&self, _resource: Option<&URI>, _value: Option<TextOrSnapshot>, _options: Option<WriteTextFileOptions>) -> Promise<Option<EncodedContent>> {
        unreachable!("TestTextFileService.getEncodedReadable não é exercitado por esta suíte")
    }

    fn get_decoded_stream(&self, _resource: Option<&URI>, _value: VSBufferReadableStream, _options: Option<ReadTextFileEncodingOptions>) -> Promise<Rc<dyn ReadableStream<String>>> {
        unreachable!("TestTextFileService.getDecodedStream não é exercitado por esta suíte")
    }

    fn get_encoding(&self, _resource: &URI) -> String {
        unreachable!("TestTextFileService.getEncoding não é exercitado por esta suíte")
    }

    fn resolve_decoding(&self, _resource: Option<&URI>, _options: Option<ReadTextFileEncodingOptions>) -> Promise<ResolvedDecoding> {
        unreachable!("TestTextFileService.resolveDecoding não é exercitado por esta suíte")
    }

    fn resolve_encoding(&self, _resource: Option<&URI>, _options: Option<WriteTextFileOptions>) -> Promise<ResolvedEncoding> {
        unreachable!("TestTextFileService.resolveEncoding não é exercitado por esta suíte")
    }

    fn validate_detected_encoding(&self, _resource: Option<&URI>, _detected_encoding: &str, _options: Option<ReadTextFileEncodingOptions>) -> Promise<String> {
        unreachable!("TestTextFileService.validateDetectedEncoding não é exercitado por esta suíte")
    }
}

type Marker = Rc<dyn IAgentHostEditMarkerService>;
type PrepareFlush = Box<dyn Fn(EditTelemetryTrigger, &str, bool) -> Promise<Option<IPreparedAgentHostEditAttributionFlush>>>;

/// Um objeto literal que implementa `IAgentHostEditMarkerService`.
struct StubMarkerService {
    correlation: Rc<dyn IExternalEditCorrelation>,
    take_coverage_gap: Option<Box<dyn Fn() -> IAgentHostEditAttributionCoverageGap>>,
    prepare_flush: PrepareFlush,
}

impl IAgentHostEditMarkerService for StubMarkerService {
    fn create_correlation(&self, _resource: &URI) -> Rc<dyn IExternalEditCorrelation> {
        self.correlation.clone()
    }

    fn take_coverage_gap(&self, _resource: &URI, _through_sequence: Option<u64>) -> Option<IAgentHostEditAttributionCoverageGap> {
        self.take_coverage_gap.as_ref().map(|take| take())
    }

    fn prepare_flush(
        &self,
        _resource: &URI,
        trigger: EditTelemetryTrigger,
        stats_uuid: &str,
        is_dirty: bool,
        _language_id: Option<&str>,
    ) -> Promise<Option<IPreparedAgentHostEditAttributionFlush>> {
        (self.prepare_flush)(trigger, stats_uuid, is_dirty)
    }
}

fn marker(correlation: Rc<dyn IExternalEditCorrelation>, prepare_flush: PrepareFlush) -> Marker {
    Rc::new(StubMarkerService { correlation, take_coverage_gap: None, prepare_flush })
}

type RegisterFn = Box<dyn Fn(&JsString, &JsString) -> String>;
type IsSuppressedFn = Box<dyn Fn(&str) -> bool>;
type GetResolutionFn = Box<dyn Fn(&str) -> Option<IExternalEditCorrelationResolution>>;

/// Um objeto literal que implementa `IExternalEditCorrelation` (sem `onDidResolve`, `onDidInvalidate: Event.None` e
/// `release: () => { }`).
struct StubCorrelation {
    on_did_suppress: Event<String>,
    register: RegisterFn,
    is_suppressed: IsSuppressedFn,
    get_resolution: Option<GetResolutionFn>,
}

impl StubCorrelation {
    /// `{ onDidSuppress: Event.None, onDidInvalidate: Event.None, register: () => 'observation', isSuppressed: () =>
    /// suppressed, release: () => { } }`.
    fn observation(suppressed: bool) -> Rc<dyn IExternalEditCorrelation> {
        Rc::new(Self {
            on_did_suppress: Event::none(),
            register: Box::new(|_, _| "observation".to_owned()),
            is_suppressed: Box::new(move |_| suppressed),
            get_resolution: None,
        })
    }
}

impl IExternalEditCorrelation for StubCorrelation {
    fn on_did_suppress(&self) -> Event<String> {
        self.on_did_suppress.clone()
    }

    fn on_did_invalidate(&self) -> Event<String> {
        Event::none()
    }

    fn register(&self, before: &JsString, after: &JsString) -> String {
        (self.register)(before, after)
    }

    fn is_suppressed(&self, id: &str) -> bool {
        (self.is_suppressed)(id)
    }

    fn get_resolution(&self, id: &str) -> Option<IExternalEditCorrelationResolution> {
        self.get_resolution.as_ref().and_then(|get| get(id))
    }

    fn release(&self, _id: &str) {}
}

/// `TestExternalEditCorrelation`.
struct TestExternalEditCorrelation {
    on_did_suppress: Rc<Emitter<String>>,
    on_did_resolve: Rc<Emitter<IExternalEditCorrelationResolution>>,
    on_did_invalidate: Rc<Emitter<String>>,
    drain: DeferredPromise<()>,
    resolution: RefCell<Option<IExternalEditCorrelationResolution>>,
    wait_for_drain: bool,
}

impl TestExternalEditCorrelation {
    fn new(wait_for_drain: bool) -> Rc<Self> {
        Rc::new(Self {
            on_did_suppress: Emitter::new(),
            on_did_resolve: Emitter::new(),
            on_did_invalidate: Emitter::new(),
            drain: DeferredPromise::new(),
            resolution: RefCell::new(None),
            wait_for_drain,
        })
    }

    fn resolve(&self, source: TextModelEditSource) {
        let resolution = IExternalEditCorrelationResolution { id: "observation".to_owned(), source: Some(Rc::new(source)) };
        *self.resolution.borrow_mut() = Some(resolution.clone());
        self.on_did_suppress.fire("observation".to_owned());
        self.on_did_resolve.fire(resolution);
    }

    fn complete_drain(&self) {
        let _ = self.drain.complete(());
    }
}

impl IExternalEditCorrelation for TestExternalEditCorrelation {
    fn on_did_suppress(&self) -> Event<String> {
        self.on_did_suppress.event()
    }

    fn on_did_resolve(&self) -> Option<Event<IExternalEditCorrelationResolution>> {
        Some(self.on_did_resolve.event())
    }

    fn on_did_invalidate(&self) -> Event<String> {
        self.on_did_invalidate.event()
    }

    fn register(&self, _before: &JsString, _after: &JsString) -> String {
        "observation".to_owned()
    }

    fn is_suppressed(&self, _id: &str) -> bool {
        self.resolution.borrow().is_some()
    }

    fn get_resolution(&self, _id: &str) -> Option<IExternalEditCorrelationResolution> {
        self.resolution.borrow().clone()
    }

    fn wait_for_resolution(&self, _ids: &[String], timeout_ms: u64) -> Promise<()> {
        let wait_for_drain = self.wait_for_drain;
        let drain = self.drain.promise.clone();
        async_fn(async move {
            if wait_for_drain {
                // `Promise.race([this._drain.p, timeout(timeoutMs)])`
                let timer: Promise<()> = (*je_base::async_::timeout(timeout_ms)).clone();
                Promise::race(vec![drain, timer]).await?;
            }
            Ok(())
        })
    }

    fn release(&self, _id: &str) {}
}

/// O que o `setup` devolve.
struct Context {
    disposables: Rc<DisposableStore>,
    document: Rc<MutableObservableDocument>,
    telemetry: Rc<TestTelemetryService>,
    head_hash: Rc<ObservableValue<Option<String>, ()>>,
    branch: Rc<ObservableValue<Option<String>, ()>>,
    imp: Rc<EditSourceTrackingImpl>,
}

impl Context {
    /// `allDetails`.
    fn all_details(&self) -> Vec<Detail> {
        self.telemetry.details.borrow().iter().map(Detail::parse).collect()
    }

    /// `details`: só o modo `longterm`.
    fn details(&self) -> Vec<Detail> {
        self.all_details().into_iter().filter(|event| event.mode == "longterm").collect()
    }

    /// `allStats`.
    fn all_stats(&self) -> Vec<Stats> {
        self.telemetry.stats.borrow().iter().map(Stats::parse).collect()
    }

    /// `stats`: só o modo `longterm`.
    fn stats(&self) -> Vec<Stats> {
        self.all_stats().into_iter().filter(|event| event.mode == "longterm").collect()
    }
}

fn visible_observable(initial: bool) -> Rc<ObservableValue<bool, ()>> {
    observable_value::<bool, ()>("visible", initial)
}

fn setup(visible: Rc<ObservableValue<bool, ()>>, marker_service: Option<Marker>, dirty: bool) -> Context {
    let disposables = DisposableStore::new();
    let head_hash = observable_value::<Option<String>, ()>("headHash", Some("hash-1".to_owned()));
    let branch = observable_value::<Option<String>, ()>("branch", Some("main".to_owned()));
    let repo: Rc<dyn IScmRepoAdapter> = Rc::new(TestRepo { head_hash: head_hash.to_observable(), branch: branch.to_observable() });
    let telemetry = Rc::new(TestTelemetryService::default());
    let telemetry_service: Rc<dyn ITelemetryService> = telemetry.clone();
    let editor_worker_service: Rc<dyn IEditorWorkerService> = Rc::new(TestEditorWorkerService);
    let scm_adapter: Rc<dyn IScmAdapter> = Rc::new(TestScmAdapter { repo });
    let uri_visibility_provider: Rc<dyn IUriVisibilityProvider> = Rc::new(TestUriVisibilityProvider { visible: visible.to_observable() });
    let random_service: Rc<dyn IRandomService> = Rc::new(TestRandomService { uuid: Cell::new(0) });
    let user_attention_service: Rc<dyn IUserAttentionService> = Rc::new(TestUserAttentionService);
    let text_file_service: Rc<dyn ITextFileService> = Rc::new(TestTextFileService { dirty });
    let ai_edit_telemetry_service: Rc<dyn IAiEditTelemetryService> = Rc::new(TestAiEditTelemetryService);

    let instantiation_service = disposables.add_rc(InstantiationService::new(
        ServiceCollection::new()
            .with(I_TELEMETRY_SERVICE, telemetry_service)
            .with(I_EDITOR_WORKER_SERVICE, editor_worker_service)
            .with(I_SCM_ADAPTER, scm_adapter)
            .with(I_URI_VISIBILITY_PROVIDER, uri_visibility_provider)
            .with(I_RANDOM_SERVICE, random_service)
            .with(I_USER_ATTENTION_SERVICE, user_attention_service)
            .with(I_TEXT_FILE_SERVICE, text_file_service)
            .with(I_AI_EDIT_TELEMETRY_SERVICE, ai_edit_telemetry_service)
            .with(I_LOG_SERVICE, NullLogService::shared()),
    ));

    let workspace = MutableObservableWorkspace::new();
    let observable_workspace: Rc<dyn ObservableWorkspace> = workspace.clone();
    let annotated_documents = disposables.add_rc(instantiation_service.create_instance::<AnnotatedDocuments>(observable_workspace));
    let annotated: Rc<dyn IAnnotatedDocuments> = annotated_documents;
    let imp = disposables.add_rc(instantiation_service.create_instance::<EditSourceTrackingImpl>((const_observable(true), annotated, marker_service)));
    let document = disposables.add_rc(workspace.create_document(
        CreateDocumentOptions {
            uri: Some(URI::file("C:\\repo\\file.ts", Platform::Windows)),
            initial_value: Some("hello".to_owned()),
            language_id: Some("typescript".to_owned()),
            ..Default::default()
        },
        None,
    ));

    Context { disposables, document, telemetry, head_hash, branch, imp }
}

fn chat_edit(request_id: &str) -> Rc<TextModelEditSource> {
    Rc::new(EditSources::chat_apply_edits(&ChatApplyEditsData {
        session_id: Some("session-1".to_owned()),
        request_id: Some(request_id.to_owned()),
        language_id: "typescript".to_owned(),
        mode: Some("agent".to_owned()),
        ..Default::default()
    }))
}

/// `StringEditWithReason.replace(context.document.findRange(search), newText, source)`.
fn replace(context: &Context, search: &str, new_text: &str, source: Rc<TextModelEditSource>) -> StringEditWithReason {
    StringEditWithReason::replace(context.document.find_range(search), new_text, Some(source))
}

/// `StringEditWithReason.replace(OffsetRange.emptyAt(context.document.value.get().value.length), newText, source)`.
fn append(context: &Context, new_text: &str, source: Rc<TextModelEditSource>) -> StringEditWithReason {
    let length = context.document.value().get().value.len() as i32;
    StringEditWithReason::replace(OffsetRange::empty_at(length), new_text, Some(source))
}

fn apply(context: &Context, edit: StringEditWithReason) {
    context.document.apply_edit(edit, None, None);
}

fn s(value: &str) -> Option<String> {
    Some(value.to_owned())
}

/// `{ commit: async totalModifiedCount => ... }` de um flush preparado que registra o total.
fn flush_committing_into(flush_token: &str, agent_modified_count: usize, commits: Rc<RefCell<Vec<usize>>>) -> IPreparedAgentHostEditAttributionFlush {
    IPreparedAgentHostEditAttributionFlush::new(flush_token, agent_modified_count, move |total_modified_count| {
        commits.borrow_mut().push(total_modified_count);
        Promise::resolved(())
    })
}

// endregion

#[test]
fn flushes_and_recreates_the_long_term_tracker_on_hash_and_branch_changes() {
    run(async {
        let context = setup(visible_observable(true), None, false);
        sleep(10).await;

        apply(&context, replace(&context, "hello", "alpha", chat_edit("request-1")));
        sleep(1500).await;
        context.head_hash.set(Some("hash-2".to_owned()), None);

        apply(&context, replace(&context, "alpha", "beta", chat_edit("request-2")));
        sleep(1500).await;
        context.branch.set(Some("feature".to_owned()), None);

        let actual: Vec<_> = context.details().iter().map(|event| (event.trigger.clone(), event.request_id.clone(), event.modified_count, event.delta_modified_count)).collect();
        assert_eq!(
            actual,
            vec![
                ("hashChange".to_owned(), s("request-1"), 5, 5),
                ("branchChange".to_owned(), s("request-2"), 3, 3),
            ]
        );

        context.disposables.dispose();
    });
}

#[test]
fn flushes_the_long_term_tracker_when_the_document_closes() {
    run(async {
        let context = setup(visible_observable(true), None, false);
        sleep(10).await;

        apply(&context, replace(&context, "hello", "alpha", chat_edit("request-1")));
        sleep(1500).await;
        context.document.dispose();
        sleep(0).await;

        let actual: Vec<_> = context.details().iter().map(|event| (event.trigger.clone(), event.request_id.clone())).collect();
        assert_eq!(actual, vec![("closed".to_owned(), s("request-1"))]);

        context.disposables.dispose();
    });
}

#[test]
fn flushes_and_recreates_the_long_term_tracker_after_ten_hours() {
    run(async {
        let context = setup(visible_observable(true), None, false);
        sleep(10).await;

        apply(&context, replace(&context, "hello", "alpha", chat_edit("request-1")));
        sleep(1500).await;
        sleep(10 * 60 * 60 * 1000).await;

        apply(&context, replace(&context, "alpha", "beta", chat_edit("request-2")));
        sleep(1500).await;
        context.head_hash.set(Some("hash-2".to_owned()), None);

        let actual: Vec<_> = context.details().iter().map(|event| (event.trigger.clone(), event.request_id.clone())).collect();
        assert_eq!(actual, vec![("10hours".to_owned(), s("request-1")), ("hashChange".to_owned(), s("request-2"))]);

        context.disposables.dispose();
    });
}

#[test]
fn emits_only_the_top_thirty_long_term_sources_by_retained_count() {
    run(async {
        let context = setup(visible_observable(true), None, false);
        sleep(10).await;

        for i in 1..=31 {
            apply(&context, append(&context, &"x".repeat(i), Rc::new(EditSources::unknown(Some(&format!("source-{i}"))))));
        }
        sleep(10).await;
        context.head_hash.set(Some("hash-2".to_owned()), None);

        let details = context.details();
        #[derive(Debug, PartialEq)]
        struct Summary {
            count: usize,
            first: String,
            last: Option<String>,
            contains_smallest: bool,
        }
        assert_eq!(
            Summary {
                count: details.len(),
                first: details[0].source_key.clone(),
                last: details.last().map(|event| event.source_key.clone()),
                contains_smallest: details.iter().any(|event| event.source_key == "source:unknown-name:source-1"),
            },
            Summary {
                count: 30,
                first: "source:unknown-name:source-31".to_owned(),
                last: s("source:unknown-name:source-2"),
                contains_smallest: false,
            }
        );

        context.disposables.dispose();
    });
}

#[test]
fn chat_apply_edits_reports_generated_and_retained_characters_by_auto_tier() {
    run(async {
        let context = setup(visible_observable(true), None, false);
        sleep(10).await;
        // `{ text, autoTier, modelId }`
        let edits: [(&str, Option<&str>, Option<&str>); 7] = [
            ("aaaaaa", Some("efficiency"), None),
            ("bbb", Some("efficiency"), None),
            ("ccccc", Some("balance"), None),
            ("dddd", Some("intelligence"), None),
            ("ee", Some("fast"), None),
            ("fff", None, None),
            ("hh", None, Some("copilot/gpt-5")),
        ];
        for (index, (new_text, auto_tier, model_id)) in edits.iter().enumerate() {
            let source = EditSources::chat_apply_edits(&ChatApplyEditsData {
                model_id: Some(model_id.unwrap_or("copilot/auto").to_owned()),
                auto_tier: auto_tier.map(str::to_owned),
                session_id: s("session"),
                request_id: Some(format!("request-{index}")),
                language_id: "typescript".to_owned(),
                mode: s("agent"),
                extension_id: Some(VersionedExtensionId { extension_id: "github.copilot-chat".to_owned(), version: "1.0.0".to_owned() }),
                code_block_suggestion_id: None,
                ..Default::default()
            });
            apply(&context, append(&context, new_text, Rc::new(source)));
            sleep(1500).await;
        }
        for text in ["aaa", "ccccc"] {
            apply(&context, replace(&context, text, "", Rc::new(EditSources::cursor("type", None))));
            sleep(10).await;
        }
        context.document.dispose();
        sleep(10).await;

        #[derive(Debug, PartialEq)]
        struct Row {
            source_key: String,
            source_key_cleaned: String,
            model_id: Option<String>,
            auto_tier: Option<String>,
            modified_count: usize,
            delta_modified_count: usize,
            total_modified_count: usize,
        }
        let actual: Vec<Row> = context
            .all_details()
            .into_iter()
            .filter(|event| event.mode == "longterm" && event.source_key.starts_with("source:Chat.applyEdits"))
            .map(|event| Row {
                source_key: event.source_key,
                source_key_cleaned: event.source_key_cleaned,
                model_id: event.model_id,
                auto_tier: event.auto_tier,
                modified_count: event.modified_count,
                delta_modified_count: event.delta_modified_count,
                total_modified_count: event.total_modified_count,
            })
            .collect();
        // `{ autoTier, modifiedCount, deltaModifiedCount, modelId? }` e o `.map` que completa cada entrada.
        let expected: Vec<Row> = [
            (s("efficiency"), 6, 9, None),
            (s("balance"), 0, 5, None),
            (s("intelligence"), 4, 4, None),
            (s("fast"), 2, 2, None),
            (None, 3, 3, None),
            (None, 2, 2, s("copilot|gpt-5")),
        ]
        .into_iter()
        .map(|(auto_tier, modified_count, delta_modified_count, model_id): (Option<String>, usize, usize, Option<String>)| {
            let model_id = model_id.unwrap_or_else(|| "copilot|auto".to_owned());
            Row {
                source_key: format!(
                    "source:Chat.applyEdits-$modelId:{model_id}{}-$extensionId:github.copilot-chat-$extensionVersion:1.0.0",
                    auto_tier.as_ref().map(|tier| format!("-$autoTier:{tier}")).unwrap_or_default()
                ),
                source_key_cleaned: "source:Chat.applyEdits".to_owned(),
                model_id: Some(model_id),
                auto_tier,
                modified_count,
                delta_modified_count,
                total_modified_count: 17,
            }
        })
        .collect();
        assert_eq!(actual, expected);

        context.disposables.dispose();
    });
}

#[test]
fn chat_apply_edits_reports_hydrafusion_models_as_separate_rows_after_applying_the_source_cap() {
    run(async {
        let context = setup(visible_observable(true), None, false);
        sleep(10).await;
        let models: Vec<String> = (0..16).map(|index| format!("model-{index}")).collect();
        let fusion_models = ["hydrafusion", "hydrafusion-max"];
        let mut index = 0;
        for model_id in &models {
            for fusion_model in fusion_models {
                let source = EditSources::chat_apply_edits(&ChatApplyEditsData {
                    model_id: Some(model_id.clone()),
                    fusion_model: s(fusion_model),
                    session_id: s("session"),
                    request_id: Some(format!("request-{index}")),
                    language_id: "typescript".to_owned(),
                    mode: s("agent"),
                    extension_id: None,
                    code_block_suggestion_id: None,
                    origin: s("agentHost"),
                    harness: s("copilotcli"),
                    ..Default::default()
                });
                index += 1;
                apply(&context, append(&context, "x", Rc::new(source)));
                sleep(1500).await;
            }
        }
        context.document.dispose();
        sleep(10).await;

        let rows: Vec<Detail> = context.all_details().into_iter().filter(|event| event.mode == "longterm" && event.source_key.starts_with("source:Chat.applyEdits")).collect();
        let mut sample: Vec<(String, String, Option<String>)> = rows
            .iter()
            .filter(|event| event.model_id.as_deref() == Some("model-0"))
            .map(|event| (event.source_key.clone(), event.source_key_cleaned.clone(), event.fusion_model.clone()))
            .collect();
        sample.sort_by(|a, b| a.0.cmp(&b.0));
        assert_eq!(rows.len(), models.len() * fusion_models.len());
        assert_eq!(
            sample,
            fusion_models
                .iter()
                .map(|fusion_model| (
                    format!("source:Chat.applyEdits-$modelId:model-0-$fusionModel:{fusion_model}-$harness:copilotcli-$origin:agentHost"),
                    "source:Chat.applyEdits-$harness:copilotcli-$origin:agentHost".to_owned(),
                    s(fusion_model),
                ))
                .collect::<Vec<_>>()
        );

        context.disposables.dispose();
    });
}

#[test]
fn starts_after_first_visibility_and_keeps_only_the_long_term_tracker_while_hidden() {
    run(async {
        let visible = visible_observable(false);
        let context = setup(visible.clone(), None, false);
        sleep(10).await;

        assert_eq!(context.imp.docs_state.get().size(), 0);

        visible.set(true, None);
        let document: Rc<dyn IObservableDocument> = context.document.clone();
        let visible_state = context.imp.docs_state.get().get(&document).expect("Expected visible document state");
        assert!(visible_state.longterm_tracker.get().is_some());
        let first_windowed_tracker = visible_state.windowed_tracker.get();
        assert!(first_windowed_tracker.is_some());
        assert!(visible_state.windowed_focus_tracker.get().is_some());

        visible.set(false, None);
        let hidden_state = context.imp.docs_state.get().get(&document).expect("Expected hidden document state");
        assert!(hidden_state.longterm_tracker.get().is_some());
        assert!(hidden_state.windowed_tracker.get().is_none());
        assert!(hidden_state.windowed_focus_tracker.get().is_none());

        visible.set(true, None);
        let visible_again_state = context.imp.docs_state.get().get(&document).expect("Expected visible document state after reopening");
        let windowed_again = visible_again_state.windowed_tracker.get();
        assert!(windowed_again.is_some());
        assert!(!Rc::ptr_eq(windowed_again.as_ref().unwrap(), first_windowed_tracker.as_ref().unwrap()));

        context.disposables.dispose();
    });
}

#[test]
fn fans_out_agent_host_reload_attribution_to_both_focus_windows() {
    run(async {
        let visible = visible_observable(true);
        let correlation = TestExternalEditCorrelation::new(false);
        let prepare_count = Rc::new(Cell::new(0));
        let count = prepare_count.clone();
        let marker_service = marker(
            correlation.clone(),
            Box::new(move |_, _, _| {
                count.set(count.get() + 1);
                Promise::resolved(None)
            }),
        );
        let context = setup(visible.clone(), Some(marker_service), false);
        sleep(10).await;

        apply(&context, replace(&context, "hello", "external", Rc::new(EditSources::reload_from_disk())));
        sleep(1500).await;
        correlation.resolve(EditSources::agent_host_chat_apply_edits(&AgentHostChatApplyEditsData {
            model_id: s("gpt-5"),
            auto_tier: None,
            fusion_model: None,
            session_id: "session-1".to_owned(),
            agent_session_id: None,
            chat_session_id: None,
            request_id: "turn-1".to_owned(),
            harness: "copilotcli".to_owned(),
        }));
        visible.set(false, None);
        sleep(10).await;

        let focus_details: Vec<Detail> = context.all_details().into_iter().filter(|event| event.mode != "longterm").collect();
        let focus_stats: Vec<Stats> = context.all_stats().into_iter().filter(|event| event.mode != "longterm").collect();
        #[derive(Debug, PartialEq)]
        struct DetailRow {
            mode: String,
            source_key: String,
            source_key_cleaned: String,
            origin: Option<String>,
            provider: Option<String>,
            model_id: Option<String>,
            conversation_id: Option<String>,
            request_id: Option<String>,
            modified_count: usize,
            delta_modified_count: usize,
            total_modified_count: usize,
        }
        #[derive(Debug, PartialEq)]
        struct StatsRow {
            mode: String,
            other_ai_modified_count: usize,
            agent_host_modified_count: usize,
            external_modified_count: usize,
            total_modified_characters: usize,
        }
        let mut details: Vec<DetailRow> = focus_details
            .iter()
            .map(|event| DetailRow {
                mode: event.mode.clone(),
                source_key: event.source_key.clone(),
                source_key_cleaned: event.source_key_cleaned.clone(),
                origin: event.origin.clone(),
                provider: event.provider.clone(),
                model_id: event.model_id.clone(),
                conversation_id: event.conversation_id.clone(),
                request_id: event.request_id.clone(),
                modified_count: event.modified_count,
                delta_modified_count: event.delta_modified_count,
                total_modified_count: event.total_modified_count,
            })
            .collect();
        details.sort_by(|a, b| a.mode.cmp(&b.mode));
        let mut stats: Vec<StatsRow> = focus_stats
            .iter()
            .map(|event| StatsRow {
                mode: event.mode.clone(),
                other_ai_modified_count: event.other_ai_modified_count,
                agent_host_modified_count: event.agent_host_modified_count,
                external_modified_count: event.external_modified_count,
                total_modified_characters: event.total_modified_characters,
            })
            .collect();
        stats.sort_by(|a, b| a.mode.cmp(&b.mode));
        let stats_uuids: HashSet<&String> = focus_details.iter().map(|event| &event.stats_uuid).collect();

        let expected_detail = |mode: &str| DetailRow {
            mode: mode.to_owned(),
            source_key: "source:Chat.applyEdits-$modelId:gpt-5-$harness:copilotcli-$origin:agentHost".to_owned(),
            source_key_cleaned: "source:Chat.applyEdits-$harness:copilotcli-$origin:agentHost".to_owned(),
            origin: s("agentHost"),
            provider: s("copilotcli"),
            model_id: s("gpt-5"),
            conversation_id: s("session-1"),
            request_id: s("turn-1"),
            modified_count: 8,
            delta_modified_count: 8,
            total_modified_count: 8,
        };
        let expected_stats = |mode: &str| StatsRow {
            mode: mode.to_owned(),
            other_ai_modified_count: 0,
            agent_host_modified_count: 8,
            external_modified_count: 0,
            total_modified_characters: 8,
        };
        assert_eq!(details, vec![expected_detail("10minFocusWindow"), expected_detail("20minFocusWindow")]);
        assert_eq!(stats, vec![expected_stats("10minFocusWindow"), expected_stats("20minFocusWindow")]);
        assert_eq!(stats_uuids.len(), 2);
        assert_eq!(prepare_count.get(), 0);

        context.disposables.dispose();
    });
}

#[test]
fn drains_a_late_agent_host_marker_before_focus_window_emission() {
    run(async {
        let visible = visible_observable(true);
        let correlation = TestExternalEditCorrelation::new(true);
        let marker_service = marker(correlation.clone(), Box::new(|_, _, _| Promise::resolved(None)));
        let context = setup(visible.clone(), Some(marker_service), false);
        sleep(10).await;

        apply(&context, replace(&context, "hello", "external", Rc::new(EditSources::reload_from_disk())));
        sleep(1500).await;
        visible.set(false, None);
        sleep(10).await;
        assert_eq!(context.all_details().len(), 0);

        correlation.resolve(EditSources::agent_host_chat_apply_edits(&AgentHostChatApplyEditsData {
            model_id: None,
            auto_tier: None,
            fusion_model: None,
            session_id: "session-1".to_owned(),
            agent_session_id: None,
            chat_session_id: None,
            request_id: "turn-late".to_owned(),
            harness: "claude".to_owned(),
        }));
        correlation.complete_drain();
        sleep(10).await;

        let mut actual: Vec<(String, Option<String>, Option<String>)> =
            context.all_details().iter().map(|event| (event.mode.clone(), event.provider.clone(), event.request_id.clone())).collect();
        actual.sort_by(|a, b| a.0.cmp(&b.0));
        assert_eq!(
            actual,
            vec![
                ("10minFocusWindow".to_owned(), s("claude"), s("turn-late")),
                ("20minFocusWindow".to_owned(), s("claude"), s("turn-late")),
            ]
        );

        context.disposables.dispose();
    });
}

/// O `correlation` dos casos que resolvem a atribuição pelo texto novo do documento (`register: (_before, after) =>
/// after`, `isSuppressed: id => sources.has(id)` e `getResolution`).
fn correlation_by_content(sources: Rc<RefCell<HashMap<String, Rc<TextModelEditSource>>>>) -> Rc<dyn IExternalEditCorrelation> {
    let suppressed = sources.clone();
    let resolutions = sources;
    Rc::new(StubCorrelation {
        on_did_suppress: Event::none(),
        register: Box::new(|_before, after| after.to_string()),
        is_suppressed: Box::new(move |id| suppressed.borrow().contains_key(id)),
        get_resolution: Some(Box::new(move |id| {
            resolutions.borrow().get(id).map(|source| IExternalEditCorrelationResolution { id: id.to_owned(), source: Some(source.clone()) })
        })),
    })
}

#[test]
fn caps_known_chat_scopes_and_preserves_legacy_unknown_chat_grouping_in_both_focus_windows() {
    run(async {
        let visible = visible_observable(true);
        let sources: Rc<RefCell<HashMap<String, Rc<TextModelEditSource>>>> = Rc::default();
        let marker_service = marker(correlation_by_content(sources.clone()), Box::new(|_, _, _| Promise::resolved(None)));
        let context = setup(visible.clone(), Some(marker_service), false);
        sleep(10).await;
        let mut chat_session_ids: Vec<Option<String>> = (1..=12).map(|i| Some(format!("hashed-chat-{i}"))).collect();
        chat_session_ids.extend([None, None]);
        let mut content = "hello".to_owned();
        for (index, chat_session_id) in chat_session_ids.iter().enumerate() {
            let new_text = "x".repeat(index + 1);
            content.push_str(&new_text);
            sources.borrow_mut().insert(
                content.clone(),
                Rc::new(EditSources::agent_host_chat_apply_edits(&AgentHostChatApplyEditsData {
                    model_id: s("model"),
                    auto_tier: None,
                    fusion_model: None,
                    session_id: if chat_session_id.is_some() { "session-1".to_owned() } else { format!("session-{index}") },
                    agent_session_id: chat_session_id.as_ref().map(|_| "owner-session".to_owned()),
                    chat_session_id: chat_session_id.clone(),
                    request_id: format!("turn-{index}"),
                    harness: "copilotcli".to_owned(),
                })),
            );
            apply(&context, append(&context, &new_text, Rc::new(EditSources::reload_from_disk())));
            sleep(1500).await;
        }
        visible.set(false, None);
        sleep(10).await;

        #[derive(Debug, PartialEq)]
        struct Projected {
            source_key: String,
            conversation_id: Option<String>,
            chat_session_id: Option<String>,
            agent_session_id: Option<String>,
            has_chat_session_id: bool,
            modified_count: usize,
            delta_modified_count: usize,
        }
        let project = |mode: &str| -> Vec<Projected> {
            let mut events: Vec<Detail> = context.all_details().into_iter().filter(|event| event.mode == mode).collect();
            events.sort_by_key(|event| event.modified_count);
            events
                .into_iter()
                .map(|event| Projected {
                    source_key: event.source_key,
                    conversation_id: event.conversation_id,
                    chat_session_id: event.chat_session_id,
                    agent_session_id: event.agent_session_id,
                    has_chat_session_id: event.has_chat_session_id,
                    modified_count: event.modified_count,
                    delta_modified_count: event.delta_modified_count,
                })
                .collect()
        };
        // The legacy group retains 13 + 14 characters, so the top ten exclude the three smallest known chats.
        let expected = || -> Vec<Projected> {
            chat_session_ids[3..chat_session_ids.len() - 1]
                .iter()
                .enumerate()
                .map(|(index, chat_session_id)| Projected {
                    source_key: "source:Chat.applyEdits-$modelId:model-$harness:copilotcli-$origin:agentHost".to_owned(),
                    conversation_id: if chat_session_id.is_some() { s("session-1") } else { s("session-12") },
                    chat_session_id: chat_session_id.clone(),
                    agent_session_id: chat_session_id.as_ref().map(|_| "owner-session".to_owned()),
                    has_chat_session_id: chat_session_id.is_some(),
                    modified_count: if chat_session_id.is_some() { index + 4 } else { 27 },
                    delta_modified_count: if chat_session_id.is_some() { index + 4 } else { 27 },
                })
                .collect()
        };
        assert_eq!(project("10minFocusWindow"), expected());
        assert_eq!(project("20minFocusWindow"), expected());
        context.disposables.dispose();
    });
}

#[test]
fn falls_back_to_external_attribution_after_the_focus_correlation_drain_times_out() {
    run(async {
        let visible = visible_observable(true);
        let correlation = TestExternalEditCorrelation::new(true);
        let marker_service = marker(correlation, Box::new(|_, _, _| Promise::resolved(None)));
        let context = setup(visible.clone(), Some(marker_service), false);
        sleep(10).await;

        apply(&context, replace(&context, "hello", "external", Rc::new(EditSources::reload_from_disk())));
        sleep(1500).await;
        visible.set(false, None);
        sleep(1_001).await;

        let mut details: Vec<(String, String, usize, usize)> =
            context.all_details().iter().map(|event| (event.mode.clone(), event.source_key.clone(), event.modified_count, event.total_modified_count)).collect();
        details.sort_by(|a, b| a.0.cmp(&b.0));
        let mut stats: Vec<(String, usize, usize, usize)> = context
            .all_stats()
            .iter()
            .map(|event| (event.mode.clone(), event.agent_host_modified_count, event.external_modified_count, event.total_modified_characters))
            .collect();
        stats.sort_by(|a, b| a.0.cmp(&b.0));
        assert_eq!(
            details,
            vec![
                ("10minFocusWindow".to_owned(), "source:reloadFromDisk".to_owned(), 8, 8),
                ("20minFocusWindow".to_owned(), "source:reloadFromDisk".to_owned(), 8, 8),
            ]
        );
        assert_eq!(stats, vec![("10minFocusWindow".to_owned(), 0, 8, 8), ("20minFocusWindow".to_owned(), 0, 8, 8)]);

        context.disposables.dispose();
    });
}

#[test]
fn separates_known_owners_with_no_chat_provenance_in_both_focus_windows() {
    run(async {
        let visible = visible_observable(true);
        let sources: Rc<RefCell<HashMap<String, Rc<TextModelEditSource>>>> = Rc::default();
        let marker_service = marker(correlation_by_content(sources.clone()), Box::new(|_, _, _| Promise::resolved(None)));
        let context = setup(visible.clone(), Some(marker_service), false);
        sleep(10).await;
        let mut content = "hello".to_owned();
        for (index, agent_session_id) in ["owner-one", "owner-two"].iter().enumerate() {
            let new_text = "x".repeat(index + 1);
            content.push_str(&new_text);
            sources.borrow_mut().insert(
                content.clone(),
                Rc::new(EditSources::agent_host_chat_apply_edits(&AgentHostChatApplyEditsData {
                    model_id: s("model"),
                    auto_tier: None,
                    fusion_model: None,
                    session_id: "same-legacy-value".to_owned(),
                    agent_session_id: s(agent_session_id),
                    chat_session_id: None,
                    request_id: "same-request".to_owned(),
                    harness: "copilotcli".to_owned(),
                })),
            );
            apply(&context, append(&context, &new_text, Rc::new(EditSources::reload_from_disk())));
            sleep(1500).await;
        }
        visible.set(false, None);
        sleep(10).await;
        let project = |mode: &str| -> Vec<(Option<String>, Option<String>, usize, usize)> {
            let mut events: Vec<Detail> = context.all_details().into_iter().filter(|event| event.mode == mode).collect();
            events.sort_by_key(|event| event.modified_count);
            events.into_iter().map(|event| (event.agent_session_id, event.chat_session_id, event.modified_count, event.delta_modified_count)).collect()
        };
        let expected = vec![(s("owner-one"), None, 1, 1), (s("owner-two"), None, 2, 2)];
        assert_eq!(project("10minFocusWindow"), expected);
        assert_eq!(project("20minFocusWindow"), expected);
        context.disposables.dispose();
    });
}

#[test]
fn coordinates_long_term_totals_with_agent_host_attribution() {
    run(async {
        let commits: Rc<RefCell<Vec<usize>>> = Rc::default();
        let commit_target = commits.clone();
        let marker_service = marker(
            StubCorrelation::observation(false),
            Box::new(move |trigger, stats_uuid, is_dirty| {
                if is_dirty || trigger != EditTelemetryTrigger::HashChange {
                    return Promise::resolved(None);
                }
                let stats_uuid = stats_uuid.to_owned();
                let commits = commit_target.clone();
                Promise::resolved(Some(IPreparedAgentHostEditAttributionFlush::new("flush-1", 3, move |total_modified_count| {
                    assert_eq!(stats_uuid, "stats-2");
                    commits.borrow_mut().push(total_modified_count);
                    Promise::resolved(())
                })))
            }),
        );
        let context = setup(visible_observable(true), Some(marker_service), false);
        sleep(10).await;

        apply(&context, replace(&context, "hello", "alpha", chat_edit("request-1")));
        sleep(1500).await;
        context.head_hash.set(Some("hash-2".to_owned()), None);
        sleep(10).await;

        let details: Vec<(String, usize, usize)> = context.details().iter().map(|event| (event.stats_uuid.clone(), event.modified_count, event.total_modified_count)).collect();
        let stats: Vec<(String, usize, usize, usize)> = context
            .stats()
            .iter()
            .map(|event| (event.stats_uuid.clone(), event.other_ai_modified_count, event.agent_host_modified_count, event.total_modified_characters))
            .collect();
        assert_eq!(details, vec![("stats-2".to_owned(), 5, 8)]);
        assert_eq!(stats, vec![("stats-2".to_owned(), 5, 3, 8)]);
        assert_eq!(*commits.borrow(), vec![8]);

        context.disposables.dispose();
    });
}

#[test]
fn recomputes_workbench_totals_after_a_late_agent_marker() {
    run(async {
        let on_did_suppress = Emitter::<String>::new();
        let prepare_started = Rc::new(DeferredPromise::<()>::new());
        let continue_prepare = DeferredPromise::<()>::new();
        let suppressed = Rc::new(Cell::new(false));
        let committed_totals: Rc<RefCell<Vec<usize>>> = Rc::default();

        let correlation: Rc<dyn IExternalEditCorrelation> = Rc::new(StubCorrelation {
            on_did_suppress: on_did_suppress.event(),
            register: Box::new(|_, _| "observation".to_owned()),
            is_suppressed: {
                let suppressed = suppressed.clone();
                Box::new(move |_| suppressed.get())
            },
            get_resolution: None,
        });
        let (continue_signal, totals) = (continue_prepare.promise.clone(), committed_totals.clone());
        let completer = prepare_started.clone();
        let marker_service = marker(
            correlation,
            Box::new(move |trigger, _, _| {
                if trigger != EditTelemetryTrigger::HashChange {
                    return Promise::resolved(None);
                }
                let _ = completer.complete(());
                let continue_signal = continue_signal.clone();
                let totals = totals.clone();
                async_fn(async move {
                    continue_signal.await?;
                    Ok(Some(IPreparedAgentHostEditAttributionFlush::new("flush-1", 3, move |total_modified_count| {
                        totals.borrow_mut().push(total_modified_count);
                        Promise::resolved(())
                    })))
                })
            }),
        );
        let context = setup(visible_observable(true), Some(marker_service), false);
        context.disposables.add_rc(on_did_suppress.clone());
        sleep(10).await;

        apply(&context, replace(&context, "hello", "external", Rc::new(EditSources::reload_from_disk())));
        sleep(1500).await;
        context.head_hash.set(Some("hash-2".to_owned()), None);
        prepare_started.promise.clone().await.expect("prepareStarted");
        suppressed.set(true);
        on_did_suppress.fire("observation".to_owned());
        let _ = continue_prepare.complete(());
        sleep(10).await;

        let stats: Vec<(usize, usize, usize, usize)> = context
            .stats()
            .iter()
            .map(|event| (event.other_ai_modified_count, event.agent_host_modified_count, event.external_modified_count, event.total_modified_characters))
            .collect();
        assert_eq!(*committed_totals.borrow(), vec![3]);
        assert_eq!(stats, vec![(0, 3, 0, 3)]);

        context.disposables.dispose();
    });
}

#[test]
fn defers_agent_host_attribution_while_the_model_is_dirty() {
    run(async {
        let dirty_states: Rc<RefCell<Vec<bool>>> = Rc::default();
        let coverage_gap_take_count = Rc::new(Cell::new(0));
        let states = dirty_states.clone();
        let take_count = coverage_gap_take_count.clone();
        let marker_service: Marker = Rc::new(StubMarkerService {
            correlation: StubCorrelation::observation(false),
            take_coverage_gap: Some(Box::new(move || {
                take_count.set(take_count.get() + 1);
                IAgentHostEditAttributionCoverageGap { edit_count: 1, inserted_count: 42 }
            })),
            prepare_flush: Box::new(move |trigger, _, is_dirty| {
                if trigger == EditTelemetryTrigger::HashChange {
                    states.borrow_mut().push(is_dirty);
                }
                Promise::resolved(None)
            }),
        });
        let context = setup(visible_observable(true), Some(marker_service), true);
        sleep(10).await;

        apply(&context, replace(&context, "hello", "alpha", chat_edit("request-1")));
        sleep(1500).await;
        context.head_hash.set(Some("hash-2".to_owned()), None);
        sleep(10).await;

        let details: Vec<(usize, usize)> = context.details().iter().map(|event| (event.modified_count, event.total_modified_count)).collect();
        assert_eq!(*dirty_states.borrow(), vec![true]);
        assert_eq!(coverage_gap_take_count.get(), 0);
        assert_eq!(details, vec![(5, 5)]);

        context.disposables.dispose();
    });
}

#[test]
fn does_not_fall_back_matched_agent_edits_while_the_model_is_dirty() {
    run(async {
        let marker_service = marker(StubCorrelation::observation(true), Box::new(|_, _, _| Promise::resolved(None)));
        let context = setup(visible_observable(true), Some(marker_service), true);
        sleep(10).await;

        apply(&context, replace(&context, "hello", "external", Rc::new(EditSources::reload_from_disk())));
        sleep(1500).await;
        context.head_hash.set(Some("hash-2".to_owned()), None);
        sleep(10).await;

        assert_eq!((context.details().len(), context.stats().len()), (0, 0));

        context.disposables.dispose();
    });
}

#[test]
fn keeps_unmatched_reloads_as_standard_external_telemetry() {
    run(async {
        let observation = Rc::new(Cell::new(0));
        let counter = observation.clone();
        let correlation: Rc<dyn IExternalEditCorrelation> = Rc::new(StubCorrelation {
            on_did_suppress: Event::none(),
            register: Box::new(move |_, _| {
                counter.set(counter.get() + 1);
                format!("observation-{}", counter.get())
            }),
            is_suppressed: Box::new(|_| false),
            get_resolution: None,
        });
        let marker_service = marker(correlation, Box::new(|_, _, _| Promise::resolved(None)));
        let context = setup(visible_observable(true), Some(marker_service), false);
        sleep(10).await;

        apply(&context, replace(&context, "hello", "alpha", chat_edit("request-1")));
        apply(&context, replace(&context, "alpha", "external", Rc::new(EditSources::reload_from_disk())));
        sleep(1500).await;
        context.head_hash.set(Some("hash-2".to_owned()), None);
        sleep(10).await;

        let mut source_keys: Vec<String> = context.details().iter().map(|event| event.source_key.clone()).collect();
        source_keys.sort();
        let has_internal_observation_key = context.details().iter().any(|event| event.source_key.starts_with("external-observation:"));
        assert_eq!(source_keys, vec!["source:Chat.applyEdits".to_owned(), "source:reloadFromDisk".to_owned()]);
        assert!(!has_internal_observation_key);

        context.disposables.dispose();
    });
}

/// Um `IAgentHostEditMarkerService` com `takeCoverageGap: () => ({ editCount: 1, insertedCount: 42 })` e o contador de
/// chamadas, quando o caso o confere.
fn marker_with_coverage_gap(correlation: Rc<dyn IExternalEditCorrelation>, take_count: Option<Rc<Cell<usize>>>, prepare_flush: PrepareFlush) -> Marker {
    Rc::new(StubMarkerService {
        correlation,
        take_coverage_gap: Some(Box::new(move || {
            if let Some(take_count) = &take_count {
                take_count.set(take_count.get() + 1);
            }
            IAgentHostEditAttributionCoverageGap { edit_count: 1, inserted_count: 42 }
        })),
        prepare_flush,
    })
}

#[test]
fn reports_partial_agent_host_coverage_without_dropping_workbench_attribution() {
    run(async {
        let marker_service = marker_with_coverage_gap(StubCorrelation::observation(false), None, Box::new(|_, _, _| Promise::resolved(None)));
        let context = setup(visible_observable(true), Some(marker_service), false);
        sleep(10).await;

        apply(&context, replace(&context, "hello", "alpha", chat_edit("request-1")));
        apply(&context, replace(&context, "alpha", "external", Rc::new(EditSources::reload_from_disk())));
        sleep(1500).await;
        context.head_hash.set(Some("hash-2".to_owned()), None);
        sleep(10).await;

        let actual: Vec<(usize, usize, Option<String>, Option<usize>, Option<usize>)> = context
            .stats()
            .iter()
            .map(|event| {
                (
                    event.external_modified_count,
                    event.total_modified_characters,
                    event.agent_host_attribution_coverage.clone(),
                    event.agent_host_untracked_edit_count,
                    event.agent_host_untracked_inserted_count,
                )
            })
            .collect();
        assert_eq!(actual, vec![(8, 8, s("partial"), Some(1), Some(42))]);

        context.disposables.dispose();
    });
}

#[test]
fn emits_workbench_telemetry_when_agent_host_coordination_fails() {
    run(async {
        let marker_service = marker(StubCorrelation::observation(false), Box::new(|_, _, _| Promise::rejected(Error::new("Agent Host unavailable"))));
        let context = setup(visible_observable(true), Some(marker_service), false);
        sleep(10).await;

        apply(&context, replace(&context, "hello", "alpha", chat_edit("request-1")));
        sleep(1500).await;
        context.head_hash.set(Some("hash-2".to_owned()), None);
        sleep(10).await;

        let actual: Vec<(usize, usize)> = context.details().iter().map(|event| (event.modified_count, event.total_modified_count)).collect();
        assert_eq!(actual, vec![(5, 5)]);

        context.disposables.dispose();
    });
}

#[test]
fn falls_back_to_external_telemetry_when_a_matched_agent_flush_cannot_prepare() {
    run(async {
        let marker_service = marker(StubCorrelation::observation(true), Box::new(|_, _, _| Promise::rejected(Error::new("Agent Host unavailable"))));
        let context = setup(visible_observable(true), Some(marker_service), false);
        sleep(10).await;

        apply(&context, replace(&context, "hello", "alpha", chat_edit("request-1")));
        apply(&context, replace(&context, "alpha", "external", Rc::new(EditSources::reload_from_disk())));
        sleep(1500).await;
        context.head_hash.set(Some("hash-2".to_owned()), None);
        sleep(10).await;

        let mut source_keys: Vec<String> = context.details().iter().map(|event| event.source_key.clone()).collect();
        source_keys.sort();
        assert_eq!(source_keys, vec!["source:Chat.applyEdits".to_owned(), "source:reloadFromDisk".to_owned()]);

        context.disposables.dispose();
    });
}

#[test]
fn falls_back_to_a_matched_initial_external_edit_when_agent_host_is_unavailable() {
    run(async {
        let marker_service = marker(StubCorrelation::observation(true), Box::new(|_, _, _| Promise::rejected(Error::new("Agent Host unavailable"))));
        let context = setup(visible_observable(true), Some(marker_service), false);
        sleep(10).await;

        apply(&context, replace(&context, "hello", "external", Rc::new(EditSources::reload_from_disk())));
        sleep(1500).await;
        context.head_hash.set(Some("hash-2".to_owned()), None);
        sleep(10).await;

        let actual: Vec<(String, usize, usize)> = context.details().iter().map(|event| (event.source_key.clone(), event.modified_count, event.total_modified_count)).collect();
        assert_eq!(actual, vec![("source:reloadFromDisk".to_owned(), 8, 8)]);

        context.disposables.dispose();
    });
}

#[test]
fn does_not_fall_back_when_agent_host_attribution_is_deferred() {
    run(async {
        let coverage_gap_take_count = Rc::new(Cell::new(0));
        let marker_service = marker_with_coverage_gap(
            StubCorrelation::observation(true),
            Some(coverage_gap_take_count.clone()),
            Box::new(|_, _, _| Promise::rejected(agent_host_edit_attribution_deferred_error(Error::new("Prepare cancelled")))),
        );
        let context = setup(visible_observable(true), Some(marker_service), false);
        sleep(10).await;

        apply(&context, replace(&context, "hello", "external", Rc::new(EditSources::reload_from_disk())));
        sleep(1500).await;
        context.head_hash.set(Some("hash-2".to_owned()), None);
        sleep(10).await;

        assert_eq!((context.details().len(), context.stats().len(), coverage_gap_take_count.get()), (0, 0, 0));

        context.disposables.dispose();
    });
}

#[test]
fn does_not_emit_external_fallback_when_the_agent_host_commit_outcome_is_unknown() {
    run(async {
        let coverage_gap_take_count = Rc::new(Cell::new(0));
        let marker_service = marker_with_coverage_gap(
            StubCorrelation::observation(true),
            Some(coverage_gap_take_count.clone()),
            Box::new(|_, _, _| {
                Promise::resolved(Some(IPreparedAgentHostEditAttributionFlush::new("flush-1", 3, |_| {
                    Promise::rejected(agent_host_edit_attribution_unknown_outcome_error(Error::new("Transport unavailable")))
                })))
            }),
        );
        let context = setup(visible_observable(true), Some(marker_service), false);
        sleep(10).await;

        apply(&context, replace(&context, "hello", "external", Rc::new(EditSources::reload_from_disk())));
        sleep(1500).await;
        context.head_hash.set(Some("hash-2".to_owned()), None);
        sleep(10).await;

        let stats: Vec<(usize, usize, usize, usize)> = context
            .stats()
            .iter()
            .map(|event| (event.other_ai_modified_count, event.agent_host_modified_count, event.external_modified_count, event.total_modified_characters))
            .collect();
        assert_eq!(context.details().len(), 0);
        assert_eq!(coverage_gap_take_count.get(), 0);
        assert_eq!(stats, vec![(0, 3, 0, 3)]);

        context.disposables.dispose();
    });
}

#[test]
fn commits_zero_retention_agent_host_windows() {
    run(async {
        let commits: Rc<RefCell<Vec<usize>>> = Rc::default();
        let commit_target = commits.clone();
        let marker_service = marker(
            StubCorrelation::observation(false),
            Box::new(move |trigger, _, _| {
                if trigger == EditTelemetryTrigger::HashChange {
                    Promise::resolved(Some(flush_committing_into("flush-1", 0, commit_target.clone())))
                } else {
                    Promise::resolved(None)
                }
            }),
        );
        let context = setup(visible_observable(true), Some(marker_service), false);
        sleep(10).await;

        context.head_hash.set(Some("hash-2".to_owned()), None);
        sleep(10).await;

        assert_eq!((commits.borrow().clone(), context.details().len(), context.stats().len()), (vec![0], 0, 0));

        context.disposables.dispose();
    });
}
