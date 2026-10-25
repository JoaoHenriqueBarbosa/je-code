//! Porte de `src/vs/workbench/services/assignment/test/common/assignmentService.test.ts` (suítes `resolveScopedTreatment`,
//! `resolveTreatmentWithAssignment`, `WorkbenchAssignmentService treatment lifetime`, `WorkbenchAssignmentService telemetry`,
//! `assignments-validation` e `toExperimentTelemetryData`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - **`tas-client` (pacote externo).** O `import type { ExperimentationService as TASClient } from 'tas-client'` e o
//!   `mock<TASClient>()` viram o contrato [`TasClient`] declarado em `assignment_service.rs`, que os dublês [`TestTasClient`] e
//!   [`StatefulTasClient`] implementam: `getTreatmentVariableAsync` e `getTreatmentVariable` fazem o que o original
//!   sobrescreve, e os demais membros falham como o `TypeError` do `mock` (nenhum caso os chama).
//! - **`ensureNoDisposablesAreLeakedInTestSuite()`.** É o [`LeakGuard`] criado no começo de cada caso (dentro do corpo
//!   assíncrono, que roda em `run_with_real_timers`); `store.add` vira `guard.add_rc`.
//! - **`TestInstantiationService`, `stub(...)` e `createInstance`.** É [`create_service`], que monta o
//!   [`WorkbenchAssignmentServiceServices`] com os mesmos dublês (`TestConfigurationService`, `InMemoryStorageService`, o
//!   serviço de telemetria do caso, um `IProductService` e o `IWorkbenchEnvironmentService` de teste). O original não
//!   registra o `IDefaultAccountService`, o `IRequestService` nem o `IInstantiationService` que o construtor injeta e que
//!   nenhum caso usa (os experimentos ficam desligados no construtor): o porte passa o `DefaultAccountService` real e dublês
//!   que falham ao uso ([`UnusedRequestService`], [`UnusedFilterProviderFactory`] e [`UnusedTasClientLoader`]).
//! - **Estado privado por colchetes.** `service['tasClient'] = ...` é `set_tas_client`, `service['overrideInitDelay']` é
//!   `override_init_delay()`, `service['telemetry']` é `telemetry()` e o `Object.defineProperty(service, 'experimentsEnabled',
//!   { value: true })` é `override_experiments_enabled(true)`.
//! - **`NullTelemetryServiceShape` com `publicLog`, `publicLog2` e `setExperimentProperty` sobrescritos.** É o
//!   [`RecordingTelemetryService`], por composição: `public_log` recebe o `ITelemetryData` e guarda o objeto dele
//!   (`telemetry_data_to_dynamic`), `public_log2` recebe o dado já como objeto, e os dois guardam `{ eventName, data,
//!   assignmentContext }` quando o nome não é vazio.
//! - **`Record<string, string | number | boolean>` do `readFrom`** é uma lista de pares com o [`TreatmentValue`].
//! - **`Promise.all([...])` de promessas de tipos diferentes** é aguardar as três promessas, já iniciadas, na ordem.
//! - **`for (const [property, value] of Object.entries({...})) test(...)`** vira um teste por propriedade, na ordem do original.
//! - **`cleanData(data, [])`** é `clean_data(Some(&data), &[], None, &[])` (o porte recebe também os padrões de caminho
//!   relativo e de conteúdo do usuário, ausentes no original).
//! - **`deepStrictEqual` de objetos literais** é `assert_eq!` de tuplas com os mesmos campos, na mesma ordem; os eventos são
//!   comparados por [`assert_events`] (o objeto de dados pela igualdade profunda do `Dynamic`).

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use common::{LeakGuard, assert_deep_strict_equal, ensure_no_disposables_are_leaked_in_test_suite, run_with_real_timers};
use je_base::actions::ITelemetryData;
use je_base::async_::async_fn;
use je_base::cancellation::CancellationToken;
use je_base::dynamic::{Dynamic, json_stringify};
use je_base::errors::{Error, is_cancellation_error};
use je_base::event::Event;
use je_base::lifecycle::IDisposable;
use je_base::map::OrderedMap;
use je_base::parts::request::{IRequestContext, IRequestOptions};
use je_base::product::{IChatProvider, ITasConfig};
use je_base::promise::{DeferredPromise, Promise};
use je_platform::assignment::{IAssignmentService, TreatmentValue};
use je_platform::configuration::service::IConfigurationService;
use je_platform::configuration::test_configuration_service::TestConfigurationService;
use je_platform::product::ProductService;
use je_platform::request::{AuthInfo, Credentials, IRequestService, RequestCompleteEvent};
use je_platform::storage::{InMemoryStorageService, StorageService};
use je_platform::telemetry::telemetry_service::telemetry_data_to_dynamic;
use je_platform::telemetry::telemetry_utils::{NullTelemetryServiceShape, clean_data, null_telemetry_service};
use je_platform::telemetry::{CommonPropertyValue, ITelemetryService, TelemetryLevel};
use je_workbench::services::accounts::default_account::{DefaultAccountService, IDefaultAccountService};
use je_workbench::services::assignment::assignment_service::{
    IAssignmentFilter, IAssignmentFilterProviderFactory, ICopilotAssignmentFilterProvider, IExperimentationTelemetry, IGitHubCoreAssignmentsFilterProvider,
    IWorkbenchAssignmentService, TasClient, TasClientModule, TasClientModuleLoader, WorkbenchAssignmentService, WorkbenchAssignmentServiceServices,
    resolve_scoped_treatment, resolve_treatment_with_assignment, to_experiment_telemetry_data,
};
use je_workbench::services::environment::environment_service::IWorkbenchEnvironmentService;
use je_workbench::test::browser::workbench_test_services::TestEnvironmentService;

fn text(value: &str) -> TreatmentValue {
    TreatmentValue::String(value.to_owned())
}

/// `JSON.stringify(value)`: `undefined` continua `undefined`.
fn stringify(value: &Option<TreatmentValue>) -> Option<String> {
    let dynamic = match value.as_ref()? {
        TreatmentValue::String(value) => Dynamic::string(value),
        TreatmentValue::Number(value) => Dynamic::Number(*value),
        TreatmentValue::Boolean(value) => Dynamic::Bool(*value),
    };
    json_stringify(&dynamic)
}

// --- Dublês dos serviços que o construtor injeta e nenhum caso usa

/// O `IRequestService` que o original não registra: nenhum caso faz requisição.
struct UnusedRequestService;

impl IRequestService for UnusedRequestService {
    fn on_did_complete_request(&self) -> Event<RequestCompleteEvent> {
        Event::none()
    }

    fn request(&self, _options: IRequestOptions, _token: CancellationToken) -> Promise<IRequestContext> {
        unreachable!("IRequestService.request não é usado por este teste")
    }

    fn resolve_proxy(&self, _url: &str) -> Promise<Option<String>> {
        unreachable!("IRequestService.resolveProxy não é usado por este teste")
    }

    fn lookup_authorization(&self, _auth_info: AuthInfo) -> Promise<Option<Credentials>> {
        unreachable!("IRequestService.lookupAuthorization não é usado por este teste")
    }

    fn lookup_kerberos_authorization(&self, _url: &str) -> Promise<Option<String>> {
        unreachable!("IRequestService.lookupKerberosAuthorization não é usado por este teste")
    }

    fn load_certificates(&self) -> Promise<Vec<String>> {
        unreachable!("IRequestService.loadCertificates não é usado por este teste")
    }
}

/// O `IInstantiationService.createInstance` dos provedores de filtros: os experimentos ficam desligados, e nenhum caso monta o cliente.
struct UnusedFilterProviderFactory;

impl IAssignmentFilterProviderFactory for UnusedFilterProviderFactory {
    fn create_copilot_assignment_filter_provider(&self) -> Rc<dyn ICopilotAssignmentFilterProvider> {
        unreachable!("CopilotAssignmentFilterProvider não é criado por este teste")
    }

    fn create_git_hub_core_assignments_filter_provider(&self) -> Rc<dyn IGitHubCoreAssignmentsFilterProvider> {
        unreachable!("GitHubCoreAssignmentsFilterProvider não é criado por este teste")
    }
}

/// O `import('tas-client')`: nenhum caso carrega o módulo, os clientes são injetados.
struct UnusedTasClientLoader;

impl TasClientModuleLoader for UnusedTasClientLoader {
    fn import_tas_client(&self) -> Promise<Rc<dyn TasClientModule>> {
        unreachable!("o módulo tas-client não é carregado por este teste")
    }
}

// --- Dublês do cliente TAS

/// `new class extends mock<TASClient>() { getTreatmentVariableAsync() { await fetch(); return undefined; } getTreatmentVariable() { reads++; return value; } }`.
struct TestTasClient {
    value: String,
    fetch: Box<dyn Fn() -> Promise<()>>,
    reads: Cell<u32>,
}

impl TasClient for TestTasClient {
    fn initialize_promise(&self) -> Promise<()> {
        unreachable!("mock<TASClient>: initializePromise não é definido")
    }

    fn initial_fetch(&self) -> Promise<()> {
        unreachable!("mock<TASClient>: initialFetch não é definido")
    }

    fn get_treatment_variable_async(&self, _config_id: &str, _name: &str, _check_cache: bool) -> Promise<Option<TreatmentValue>> {
        let fetched = (self.fetch)();
        async_fn(async move {
            fetched.await?;
            Ok(None)
        })
    }

    fn get_treatment_variable(&self, _config_id: &str, _name: &str) -> Option<TreatmentValue> {
        self.reads.set(self.reads.get() + 1);
        Some(text(&self.value))
    }

    fn dispose(&self) {
        unreachable!("mock<TASClient>: dispose não é definido")
    }
}

/// O `createClient(value, fetch)` do original: o cliente e a leitura do contador.
struct CreatedClient {
    client: Rc<dyn TasClient>,
    concrete: Rc<TestTasClient>,
}

impl CreatedClient {
    fn reads(&self) -> u32 {
        self.concrete.reads.get()
    }
}

fn create_client(value: &str, fetch: impl Fn() -> Promise<()> + 'static) -> CreatedClient {
    let concrete = Rc::new(TestTasClient { value: value.to_owned(), fetch: Box::new(fetch), reads: Cell::new(0) });
    let client: Rc<dyn TasClient> = concrete.clone();
    CreatedClient { client, concrete }
}

/// `async () => { }`.
fn no_fetch() -> Promise<()> {
    async_fn(async { Ok(()) })
}

/// O cliente da suíte de telemetria: `getTreatmentVariableAsync` conta as leituras e devolve `undefined`, e
/// `getTreatmentVariable` devolve o `value` que o caso escolhe.
struct StatefulTasClient {
    value: Rc<RefCell<Option<TreatmentValue>>>,
    reads: Rc<Cell<u32>>,
}

impl TasClient for StatefulTasClient {
    fn initialize_promise(&self) -> Promise<()> {
        unreachable!("mock<TASClient>: initializePromise não é definido")
    }

    fn initial_fetch(&self) -> Promise<()> {
        unreachable!("mock<TASClient>: initialFetch não é definido")
    }

    fn get_treatment_variable_async(&self, _config_id: &str, _name: &str, _check_cache: bool) -> Promise<Option<TreatmentValue>> {
        self.reads.set(self.reads.get() + 1);
        async_fn(async { Ok(None) })
    }

    fn get_treatment_variable(&self, _config_id: &str, _name: &str) -> Option<TreatmentValue> {
        self.value.borrow().clone()
    }

    fn dispose(&self) {
        unreachable!("mock<TASClient>: dispose não é definido")
    }
}

// --- A montagem do serviço

struct Created {
    service: Rc<WorkbenchAssignmentService>,
    configuration: Rc<TestConfigurationService>,
}

/// `createService()`: o `TestInstantiationService` com os serviços empilhados e o serviço com `experimentsEnabled` ligado.
fn create_service(guard: &LeakGuard, telemetry_service: Rc<dyn ITelemetryService>, product_service: ProductService) -> Created {
    let configuration = TestConfigurationService::new(None);
    guard.add_rc(configuration.on_did_change_configuration_emitter.clone());
    let configuration_service: Rc<dyn IConfigurationService> = configuration.clone();
    let storage_service: Rc<dyn StorageService> = Rc::new(guard.add_rc(InMemoryStorageService::new()));
    let environment_service: Rc<dyn IWorkbenchEnvironmentService> = TestEnvironmentService::new();
    let default_account_service: Rc<dyn IDefaultAccountService> =
        guard.add_rc(DefaultAccountService::new(IChatProvider { id: "github".to_owned(), name: "GitHub".to_owned(), extra: Default::default() }));
    let service = guard.add_rc(WorkbenchAssignmentService::new(WorkbenchAssignmentServiceServices {
        telemetry_service,
        storage_service,
        configuration_service,
        product_service: Rc::new(product_service),
        environment_service,
        filter_provider_factory: Rc::new(UnusedFilterProviderFactory),
        default_account_service,
        request_service: Rc::new(UnusedRequestService),
        tas_client_loader: Rc::new(UnusedTasClientLoader),
    }));
    service.override_experiments_enabled(true);
    Created { service, configuration }
}

// --- Suíte `resolveScopedTreatment`

const BARE: &str = "config.chat.agentHost.copilot.multiTurnContextRouting.enabled";

fn scoped() -> String {
    format!("/vscode/{BARE}")
}

/// `readFrom(values)`: `name => values[name]`.
fn read_from(values: Vec<(String, TreatmentValue)>) -> impl Fn(&str) -> Option<TreatmentValue> {
    move |name| values.iter().find(|(key, _)| key == name).map(|(_, value)| value.clone())
}

#[test]
fn prefers_the_vscode_scoped_value_new_endpoint_over_the_bare_value_on_collision() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let read = read_from(vec![(BARE.to_owned(), text("legacy")), (scoped(), text("new"))]);
    assert_eq!(resolve_scoped_treatment(read, BARE), Some(text("new")));
}

#[test]
fn falls_back_to_the_bare_value_when_only_the_legacy_endpoint_assigns_it() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let read = read_from(vec![(BARE.to_owned(), text("legacy"))]);
    assert_eq!(resolve_scoped_treatment(read, BARE), Some(text("legacy")));
}

#[test]
fn uses_the_scoped_value_when_only_the_new_endpoint_assigns_it() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let read = read_from(vec![(scoped(), text("new"))]);
    assert_eq!(resolve_scoped_treatment(read, BARE), Some(text("new")));
}

#[test]
fn returns_undefined_when_neither_endpoint_assigns_it() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let read = read_from(Vec::new());
    assert_eq!(resolve_scoped_treatment(read, BARE), None);
}

#[test]
fn preserves_a_defined_falsy_scoped_value_instead_of_falling_back_to_bare() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let read = read_from(vec![(BARE.to_owned(), TreatmentValue::Boolean(true)), (scoped(), TreatmentValue::Boolean(false))]);
    assert_eq!(resolve_scoped_treatment(read, BARE), Some(TreatmentValue::Boolean(false)));
}

// --- Suíte `resolveTreatmentWithAssignment`

/// `[undefined, false, 0, '', 'treatment']` e `[undefined, false, 0, '', 'override']`: o primeiro valor é `undefined`.
fn falsy_values(last: &str) -> Vec<Option<TreatmentValue>> {
    vec![None, Some(TreatmentValue::Boolean(false)), Some(TreatmentValue::Number(0.0)), Some(text("")), Some(text(last))]
}

#[test]
fn distinguishes_real_assignments_from_developer_overrides_including_falsy_values() {
    run_with_real_timers(async move {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let mut results = Vec::new();
        for assignment in falsy_values("treatment") {
            for override_value in falsy_values("override") {
                let reads = Rc::new(Cell::new(0));
                let result = resolve_treatment_with_assignment(override_value, {
                    let reads = reads.clone();
                    let assignment = assignment.clone();
                    move || {
                        reads.set(reads.get() + 1);
                        async_fn(async move { Ok(assignment) })
                    }
                })
                .await
                .unwrap();
                results.push((result.value.clone(), result.has_assignment.clone().await.unwrap(), reads.get()));
            }
        }
        let mut expected = Vec::new();
        for assignment in falsy_values("treatment") {
            for override_value in falsy_values("override") {
                expected.push((if override_value.is_some() { override_value } else { assignment.clone() }, assignment.is_some(), 1));
            }
        }
        assert_eq!(results, expected);
    });
}

// Suíte `WorkbenchAssignmentService treatment lifetime`

fn reads_preserve_their_disposal_contract(with_assignment: bool) {
    run_with_real_timers(async move {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let Created { service, .. } = create_service(&guard, null_telemetry_service(), ProductService::default());
        let started = Rc::new(DeferredPromise::<()>::new());
        let fetched = Rc::new(DeferredPromise::<()>::new());
        let client = create_client("treatment", {
            let started = started.clone();
            let fetched = fetched.clone();
            move || {
                let started = started.clone();
                let fetched = fetched.clone();
                async_fn(async move {
                    started.complete(()).await?;
                    fetched.promise.clone().await?;
                    Ok(())
                })
            }
        });
        service.set_tas_client(Promise::resolved(client.client.clone()));
        let pending_assignment = with_assignment.then(|| service.get_treatment_with_assignment("test"));
        let pending_legacy = (!with_assignment).then(|| service.get_treatment("test"));
        started.promise.clone().await.unwrap();
        service.dispose();
        fetched.complete(()).await.unwrap();
        if let Some(pending) = pending_assignment {
            let error = pending.await.err().expect("a leitura com atribuição devia ser rejeitada");
            assert!(is_cancellation_error(&error), "esperava CancellationError, veio {error}");
            assert_eq!(client.reads(), 0);
        } else {
            let pending = pending_legacy.expect("a leitura legada foi iniciada");
            assert_eq!(pending.await.unwrap(), Some(text("treatment")));
        }
    });
}

#[test]
fn legacy_reads_preserve_their_disposal_contract() {
    reads_preserve_their_disposal_contract(false);
}

#[test]
fn assignment_aware_reads_preserve_their_disposal_contract() {
    reads_preserve_their_disposal_contract(true);
}

#[test]
fn retries_a_replaced_client_once_without_reading_its_stale_treatment() {
    run_with_real_timers(async move {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let Created { service, .. } = create_service(&guard, null_telemetry_service(), ProductService::default());
        let current = create_client("current", no_fetch);
        let stale = create_client("stale", {
            let service = Rc::downgrade(&service);
            let current_client = current.client.clone();
            move || {
                if let Some(service) = Weak::upgrade(&service) {
                    service.set_tas_client(Promise::resolved(current_client.clone()));
                }
                no_fetch()
            }
        });
        service.set_tas_client(Promise::resolved(stale.client.clone()));
        let result = service.get_treatment_with_assignment("test").await.unwrap();
        assert_eq!(
            (result.value.clone(), result.has_assignment.clone().await.unwrap(), stale.reads(), current.reads()),
            (Some(text("current")), true, 0, 1)
        );
    });
}

#[test]
fn cancels_after_a_second_replacement_instead_of_retrying_indefinitely() {
    run_with_real_timers(async move {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let Created { service, .. } = create_service(&guard, null_telemetry_service(), ProductService::default());
        let third = create_client("third", no_fetch);
        let replace_with = |next: &CreatedClient| {
            let service = Rc::downgrade(&service);
            let next_client = next.client.clone();
            move || {
                if let Some(service) = Weak::upgrade(&service) {
                    service.set_tas_client(Promise::resolved(next_client.clone()));
                }
                no_fetch()
            }
        };
        let second = create_client("second", replace_with(&third));
        let first = create_client("first", replace_with(&second));
        service.set_tas_client(Promise::resolved(first.client.clone()));
        let error = service.get_treatment_with_assignment("test").await.err().expect("a leitura devia ser cancelada");
        assert!(is_cancellation_error(&error), "esperava CancellationError, veio {error}");
        assert_eq!([first.reads(), second.reads(), third.reads()], [0, 0, 0]);
    });
}

#[test]
fn does_not_delay_a_developer_override_while_assignment_metadata_loads() {
    run_with_real_timers(async move {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let assignment = DeferredPromise::<Option<TreatmentValue>>::new();
        let result = resolve_treatment_with_assignment(Some(text("override")), {
            let assigned = assignment.promise.clone();
            move || assigned
        })
        .await
        .unwrap();
        assert_eq!(result.value, Some(text("override")));
        assignment.complete(Some(text("treatment"))).await.unwrap();
        assert!(result.has_assignment.clone().await.unwrap());
    });
}

#[test]
fn does_not_disguise_assignment_errors_as_absence() {
    run_with_real_timers(async move {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let error = Error::new("assignment unavailable");
        let failing = |error: Error| move || async_fn::<Option<TreatmentValue>>(async move { Err(error) });
        let result = resolve_treatment_with_assignment(Some(text("override")), failing(error.clone())).await.unwrap();
        assert_eq!(result.has_assignment.clone().await.err(), Some(error.clone()));
        assert_eq!(resolve_treatment_with_assignment(None, failing(error.clone())).await.err(), Some(error));
    });
}

// --- Suíte `WorkbenchAssignmentService telemetry`

const CONTEXT_PROPERTY: &str = "abexp.assignmentcontext";

/// `{ eventName, data, assignmentContext }`.
#[derive(Clone, Debug)]
struct RecordedEvent {
    event_name: String,
    data: Option<Dynamic>,
    assignment_context: Option<String>,
}

/// `assert.deepStrictEqual(events, expected)`.
#[track_caller]
fn assert_events(actual: &[RecordedEvent], expected: &[RecordedEvent]) {
    assert_eq!(actual.len(), expected.len(), "quantidade de eventos\n  atual:    {actual:?}\n  esperado: {expected:?}");
    for (actual, expected) in actual.iter().zip(expected) {
        assert_eq!(actual.event_name, expected.event_name);
        match (&actual.data, &expected.data) {
            (Some(actual), Some(expected)) => assert_deep_strict_equal(actual, expected),
            (None, None) => {}
            (actual, expected) => panic!("dados do evento\n  atual:    {actual:?}\n  esperado: {expected:?}"),
        }
        assert_eq!(actual.assignment_context, expected.assignment_context);
    }
}

/// `new class extends NullTelemetryServiceShape { setExperimentProperty, publicLog, publicLog2 }`.
struct RecordingTelemetryService {
    inner: NullTelemetryServiceShape,
    events: Rc<RefCell<Vec<RecordedEvent>>>,
    assignment_context: RefCell<Option<String>>,
}

impl RecordingTelemetryService {
    fn record(&self, event_name: &str, data: Option<Dynamic>) {
        if !event_name.is_empty() {
            self.events.borrow_mut().push(RecordedEvent { event_name: event_name.to_owned(), data, assignment_context: self.assignment_context.borrow().clone() });
        }
    }
}

impl ITelemetryService for RecordingTelemetryService {
    fn telemetry_level(&self) -> TelemetryLevel {
        self.inner.telemetry_level()
    }

    fn session_id(&self) -> String {
        self.inner.session_id()
    }

    fn machine_id(&self) -> String {
        self.inner.machine_id()
    }

    fn sqm_id(&self) -> String {
        self.inner.sqm_id()
    }

    fn dev_device_id(&self) -> String {
        self.inner.dev_device_id()
    }

    fn first_session_date(&self) -> String {
        self.inner.first_session_date()
    }

    fn send_error_telemetry(&self) -> bool {
        self.inner.send_error_telemetry()
    }

    fn public_log(&self, event_name: &str, data: Option<ITelemetryData>) {
        self.record(event_name, data.as_ref().map(telemetry_data_to_dynamic));
    }

    fn public_log2(&self, event_name: &str, data: Option<Dynamic>) {
        self.record(event_name, data);
    }

    fn public_log_error(&self, error_event_name: &str, data: Option<ITelemetryData>) {
        self.inner.public_log_error(error_event_name, data);
    }

    fn public_log_error2(&self, event_name: &str, data: Option<Dynamic>) {
        self.inner.public_log_error2(event_name, data);
    }

    fn set_experiment_property(&self, name: &str, value: &str) {
        if name == CONTEXT_PROPERTY {
            *self.assignment_context.borrow_mut() = Some(value.to_owned());
        }
    }

    fn set_common_property(&self, name: &str, value: Option<CommonPropertyValue>) {
        self.inner.set_common_property(name, value);
    }
}

/// O que o `createService()` da suíte devolve.
struct TelemetryFixture {
    service: Rc<WorkbenchAssignmentService>,
    events: Rc<RefCell<Vec<RecordedEvent>>>,
    configuration: Rc<TestConfigurationService>,
    value: Rc<RefCell<Option<TreatmentValue>>>,
    reads: Rc<Cell<u32>>,
}

impl TelemetryFixture {
    async fn new(guard: &LeakGuard) -> Self {
        let events = Rc::new(RefCell::new(Vec::new()));
        let telemetry_service = Rc::new(RecordingTelemetryService { inner: NullTelemetryServiceShape, events: events.clone(), assignment_context: RefCell::new(None) });
        let product_service = ProductService {
            tas_config: Some(ITasConfig {
                endpoint: String::new(),
                telemetry_event_name: "query-expfeature".to_owned(),
                assignment_context_telemetry_property_name: CONTEXT_PROPERTY.to_owned(),
                extra: Default::default(),
            }),
            ..ProductService::default()
        };
        let Created { service, configuration } = create_service(guard, telemetry_service, product_service);

        let value = Rc::new(RefCell::new(None));
        let reads = Rc::new(Cell::new(0));
        let client: Rc<dyn TasClient> = Rc::new(StatefulTasClient { value: value.clone(), reads: reads.clone() });
        service.set_tas_client(Promise::resolved(client));
        // Let the initialization timer dispose its cancellation listeners before test teardown.
        service.override_init_delay().await.unwrap();
        Self { service, events, configuration, value, reads }
    }

    fn set_context(&self, context: &str) {
        self.service.telemetry().set_shared_property(CONTEXT_PROPERTY, context);
    }

    fn post_event(&self, event_name: &str, props: &OrderedMap<String, String>) {
        self.service.telemetry().post_event(event_name, props);
    }

    fn set_value(&self, next: Option<TreatmentValue>) {
        *self.value.borrow_mut() = next;
    }

    fn events(&self) -> Vec<RecordedEvent> {
        self.events.borrow().clone()
    }
}

/// `{ id: 'test', exclude: id => id === 'hidden', onDidChange: Event.None }`.
struct HiddenFilter;

impl IAssignmentFilter for HiddenFilter {
    fn id(&self) -> &str {
        "test"
    }

    fn exclude(&self, assignment: &str) -> bool {
        assignment == "hidden"
    }

    fn on_did_change(&self) -> Event<()> {
        Event::none()
    }
}

fn expected_event(value: Option<TreatmentValue>, assignment_context: Option<&str>, treatment_name: &str) -> RecordedEvent {
    RecordedEvent {
        event_name: "tasClientReadTreatmentComplete".to_owned(),
        data: Some(Dynamic::object([
            ("treatmentName", Dynamic::string(treatment_name)),
            ("treatmentValue", stringify(&value).as_deref().map_or(Dynamic::Undefined, Dynamic::string)),
        ])),
        assignment_context: assignment_context.map(str::to_owned),
    }
}

#[test]
fn deduplicates_concurrent_reads_across_both_apis_without_skipping_assignment_reads() {
    run_with_real_timers(async move {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let fixture = TelemetryFixture::new(&guard).await;
        let first = fixture.service.get_treatment("test");
        let second = fixture.service.get_treatment_with_assignment("test");
        let third = fixture.service.get_treatment("test");
        let results = (first.await.unwrap(), second.await.unwrap(), third.await.unwrap());
        assert_eq!(
            (results.0, results.1.value.clone(), results.1.has_assignment.clone().await.unwrap(), results.2, fixture.reads.get()),
            (None, None, false, None, 3)
        );
        assert_events(&fixture.events(), &[expected_event(None, None, "test")]);
    });
}

/// `[undefined, false, 0, '', 'A', 'B', 'A', undefined]`.
fn transition_values() -> Vec<Option<TreatmentValue>> {
    vec![None, Some(TreatmentValue::Boolean(false)), Some(TreatmentValue::Number(0.0)), Some(text("")), Some(text("A")), Some(text("B")), Some(text("A")), None]
}

#[test]
fn preserves_value_transitions_falsy_values_and_returning_to_earlier_values() {
    run_with_real_timers(async move {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let fixture = TelemetryFixture::new(&guard).await;
        let values = transition_values();
        let mut results = Vec::new();
        for value in &values {
            fixture.set_value(value.clone());
            let legacy = fixture.service.get_treatment("test").await.unwrap();
            let result = fixture.service.get_treatment_with_assignment("test").await.unwrap();
            results.push((legacy, (result.value.clone(), result.has_assignment.clone().await.unwrap())));
        }
        let expected_results: Vec<_> = values.iter().map(|value| (value.clone(), (value.clone(), value.is_some()))).collect();
        assert_eq!(results, expected_results);
        let expected_events: Vec<_> = values.iter().map(|value| expected_event(value.clone(), None, "test")).collect();
        assert_events(&fixture.events(), &expected_events);
    });
}

#[test]
fn preserves_context_transitions_without_changing_refresh_notifications() {
    run_with_real_timers(async move {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let fixture = TelemetryFixture::new(&guard).await;
        fixture.set_value(Some(text("value")));
        let refreshes = Rc::new(Cell::new(0));
        guard.add(fixture.service.on_did_refetch_assignments().listen({
            let refreshes = refreshes.clone();
            move |()| refreshes.set(refreshes.get() + 1)
        }));
        fixture.service.get_treatment("test").await.unwrap();
        for context in ["A", "A", "B", "B", "A", "", ""] {
            fixture.set_context(context);
            fixture.service.get_treatment("test").await.unwrap();
        }
        let expected_events: Vec<_> = [None, Some("A"), Some("B"), Some("A"), Some("")].into_iter().map(|context| expected_event(Some(text("value")), context, "test")).collect();
        assert_events(&fixture.events(), &expected_events);
        assert_eq!(refreshes.get(), 7);
    });
}

#[test]
fn deduplicates_using_the_filtered_telemetry_context() {
    run_with_real_timers(async move {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let fixture = TelemetryFixture::new(&guard).await;
        fixture.service.add_telemetry_assignment_filter(Rc::new(HiddenFilter));
        fixture.set_context("A;hidden");
        fixture.service.get_treatment("test").await.unwrap();
        fixture.set_context("A");
        fixture.service.get_treatment_with_assignment("test").await.unwrap();
        assert_events(&fixture.events(), &[expected_event(None, Some("A"), "test")]);
    });
}

#[test]
fn tracks_treatments_and_service_instances_independently() {
    run_with_real_timers(async move {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let first = TelemetryFixture::new(&guard).await;
        let second = TelemetryFixture::new(&guard).await;
        first.service.get_treatment("test").await.unwrap();
        first.service.get_treatment("another").await.unwrap();
        first.service.get_treatment("test").await.unwrap();
        second.service.get_treatment("test").await.unwrap();
        assert_events(&first.events(), &[expected_event(None, None, "test"), expected_event(None, None, "another")]);
        assert_events(&second.events(), &[expected_event(None, None, "test")]);
    });
}

#[test]
fn preserves_developer_override_transitions_without_changing_assignment_metadata() {
    run_with_real_timers(async move {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let fixture = TelemetryFixture::new(&guard).await;
        fixture.set_value(Some(text("assigned")));
        for override_value in ["override", "override", "changed"] {
            fixture.configuration.set_user_configuration("experiments.override.test", Dynamic::string(override_value), None).await.unwrap();
            let result = fixture.service.get_treatment_with_assignment("test").await.unwrap();
            assert_eq!((result.value.clone(), result.has_assignment.clone().await.unwrap()), (Some(text(override_value)), true));
        }
        assert_events(&fixture.events(), &[expected_event(Some(text("override")), None, "test"), expected_event(Some(text("changed")), None, "test")]);
    });
}

// Suíte `assignments-validation`

/// `validationProperties(overrides)`.
fn validation_properties(overrides: &[(&str, &str)]) -> OrderedMap<String, String> {
    let mut properties = OrderedMap::new();
    for (key, value) in [("FeatureVariableCount", "0"), ("AssignedVariantCount", "0"), ("DataVersion", "1"), ("AssignmentContext", "")] {
        properties.insert(key.to_owned(), value.to_owned());
    }
    for (key, value) in overrides {
        properties.insert((*key).to_owned(), (*value).to_owned());
    }
    properties
}

/// `expectedValidation(props, assignmentContext)`: `data: Object.fromEntries(props)`.
fn expected_validation(props: &OrderedMap<String, String>, assignment_context: Option<&str>) -> RecordedEvent {
    RecordedEvent {
        event_name: "assignments-validation".to_owned(),
        data: Some(Dynamic::object(props.iter().map(|(key, value)| (key.as_str(), Dynamic::string(value))))),
        assignment_context: assignment_context.map(str::to_owned),
    }
}

/// `new Map(props)`.
fn copy_of(props: &OrderedMap<String, String>) -> OrderedMap<String, String> {
    props.clone()
}

/// `new Map([...props].reverse())`.
fn reversed(props: &OrderedMap<String, String>) -> OrderedMap<String, String> {
    let mut result = OrderedMap::new();
    for (key, value) in props.iter().collect::<Vec<_>>().into_iter().rev() {
        result.insert(key.clone(), value.clone());
    }
    result
}

#[test]
fn logs_the_first_payload_and_suppresses_duplicates_regardless_of_property_order() {
    run_with_real_timers(async move {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let fixture = TelemetryFixture::new(&guard).await;
        let props = validation_properties(&[]);
        fixture.post_event("assignments-validation", &props);
        fixture.post_event("assignments-validation", &copy_of(&props));
        fixture.post_event("assignments-validation", &reversed(&props));
        assert_events(&fixture.events(), &[expected_validation(&props, None)]);
    });
}

/// `test(`preserves ${property} transitions including returning to an earlier value`)`.
fn preserves_property_transitions(property: &'static str, value: &'static str) {
    run_with_real_timers(async move {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let fixture = TelemetryFixture::new(&guard).await;
        let original = validation_properties(&[]);
        let changed = validation_properties(&[(property, value)]);
        for props in [&original, &original, &changed, &changed, &original] {
            fixture.post_event("assignments-validation", props);
        }
        assert_events(&fixture.events(), &[expected_validation(&original, None), expected_validation(&changed, None), expected_validation(&original, None)]);
    });
}

#[test]
fn preserves_feature_variable_count_transitions_including_returning_to_an_earlier_value() {
    preserves_property_transitions("FeatureVariableCount", "1");
}

#[test]
fn preserves_assigned_variant_count_transitions_including_returning_to_an_earlier_value() {
    preserves_property_transitions("AssignedVariantCount", "1");
}

#[test]
fn preserves_data_version_transitions_including_returning_to_an_earlier_value() {
    preserves_property_transitions("DataVersion", "2");
}

#[test]
fn preserves_assignment_context_transitions_including_returning_to_an_earlier_value() {
    preserves_property_transitions("AssignmentContext", "B");
}

#[test]
fn snapshots_mutable_payloads_and_detects_added_and_removed_properties() {
    run_with_real_timers(async move {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let fixture = TelemetryFixture::new(&guard).await;
        let mut props = validation_properties(&[]);
        let original = expected_validation(&props, None);
        fixture.post_event("assignments-validation", &props);
        props.insert("FeatureVariableCount".to_owned(), "1".to_owned());
        let changed = expected_validation(&props, None);
        fixture.post_event("assignments-validation", &props);
        props.remove(&"FeatureVariableCount".to_owned());
        let removed = expected_validation(&props, None);
        fixture.post_event("assignments-validation", &props);
        props.insert("FeatureVariableCount".to_owned(), "1".to_owned());
        fixture.post_event("assignments-validation", &props);
        assert_events(&fixture.events(), &[original, changed.clone(), removed, changed]);
    });
}

#[test]
fn preserves_changes_to_the_filtered_common_context_independently_of_the_payload() {
    run_with_real_timers(async move {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let fixture = TelemetryFixture::new(&guard).await;
        let props = validation_properties(&[]);
        fixture.service.add_telemetry_assignment_filter(Rc::new(HiddenFilter));
        fixture.post_event("assignments-validation", &props);
        for context in ["A;hidden", "A", "B", "B", "A", "", ""] {
            fixture.set_context(context);
            fixture.post_event("assignments-validation", &props);
        }
        let expected: Vec<_> = [None, Some("A"), Some("B"), Some("A"), Some("")].into_iter().map(|context| expected_validation(&props, context)).collect();
        assert_events(&fixture.events(), &expected);
    });
}

#[test]
fn tracks_service_instances_independently_for_validation_events() {
    run_with_real_timers(async move {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let first = TelemetryFixture::new(&guard).await;
        let second = TelemetryFixture::new(&guard).await;
        let props = validation_properties(&[]);
        first.post_event("assignments-validation", &props);
        first.post_event("assignments-validation", &props);
        second.post_event("assignments-validation", &props);
        assert_events(&first.events(), &[expected_validation(&props, None)]);
        assert_events(&second.events(), &[expected_validation(&props, None)]);
    });
}

/// `new Map([[key, value], ...])`.
fn map_of(entries: &[(&str, &str)]) -> OrderedMap<String, String> {
    let mut map = OrderedMap::new();
    for (key, value) in entries {
        map.insert((*key).to_owned(), (*value).to_owned());
    }
    map
}

#[test]
fn does_not_suppress_other_sdk_events_or_reset_validation_deduplication() {
    run_with_real_timers(async move {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let fixture = TelemetryFixture::new(&guard).await;
        let props = validation_properties(&[]);
        fixture.post_event("assignments-validation", &props);
        let other_events = [
            ("tas-call", map_of(&[("callType", "assignments"), ("outcome", "Success"), ("extensionName", "vscode-core"), ("assignmentContext", "A")])),
            ("tas-call", map_of(&[("callType", "assignments"), ("outcome", "ServerError"), ("extensionName", "vscode-core"), ("assignmentContext", "")])),
            ("call-assignments-error", map_of(&[("ErrorType", "ServerError")])),
            ("query-expfeature", map_of(&[("ABExp.queriedFeature", "vscode.test")])),
        ];
        for (name, properties) in &other_events {
            fixture.post_event(name, properties);
            fixture.post_event(name, properties);
        }
        fixture.post_event("assignments-validation", &props);
        let mut expected = vec![expected_validation(&props, None)];
        for (event_name, properties) in &other_events {
            for _ in 0..2 {
                expected.push(RecordedEvent {
                    event_name: (*event_name).to_owned(),
                    data: Some(telemetry_data_to_dynamic(&to_experiment_telemetry_data(properties))),
                    assignment_context: None,
                });
            }
        }
        assert_events(&fixture.events(), &expected);
    });
}

// --- Suíte `toExperimentTelemetryData`

#[test]
fn marks_the_queried_feature_name_trusted_so_a_vscode_scoped_key_survives_telemetry_cleaning() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let scoped = "/vscode/config.chat.agentHost.copilot.multiTurnContextRouting.enabled";
    let data = to_experiment_telemetry_data(&map_of(&[("ABExp.queriedFeature", scoped)]));

    // The trusted feature name survives cleaning, whereas the same value left unmarked would be
    // redacted by the file-path heuristic - guarding against a regression back to that behavior.
    let cleaned = clean_data(Some(&telemetry_data_to_dynamic(&data)), &[], None, &[]);
    assert!(cleaned.get_str("ABExp.queriedFeature").strict_equals(&Dynamic::string(scoped)), "o nome confiável devia sobreviver à limpeza: {cleaned:?}");
    let unmarked = Dynamic::object([("ABExp.queriedFeature", Dynamic::string(scoped))]);
    let cleaned_unmarked = clean_data(Some(&unmarked), &[], None, &[]);
    assert!(
        cleaned_unmarked.get_str("ABExp.queriedFeature").strict_equals(&Dynamic::string("<REDACTED: user-file-path>")),
        "o mesmo valor sem a marca devia ser redigido: {cleaned_unmarked:?}"
    );
}
