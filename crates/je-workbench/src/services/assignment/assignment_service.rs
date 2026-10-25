//! Porte de `src/vs/workbench/services/assignment/common/assignmentService.ts`: o serviço de atribuição do workbench
//! (experimentos do ExP/TAS), a telemetria do contexto de atribuição e os auxiliares de leitura de tratamentos.
//!
//! Divergências:
//! - **`tas-client` (pacote externo).** O `import type { IKeyValueStorage, IExperimentationTelemetry,
//!   IExperimentationFilterProvider, ExperimentationService as TASClient } from 'tas-client'` e o módulo carregado em
//!   execução (`resolveAmdNodeModulePath` e `await import(...)`) viram o contrato local: [`IKeyValueStorage`],
//!   [`IExperimentationTelemetry`], o [`ExperimentationFilterProvider`] da plataforma, o cliente [`TasClient`] (só o que o
//!   serviço usa: `initializePromise`, `initialFetch`, `getTreatmentVariableAsync`, `getTreatmentVariable` e `dispose`), as
//!   opções de construção [`TasClientOptions`] e o carregador [`TasClientModuleLoader`] (quem resolve o módulo e o
//!   importa). Os testes dublam esse contrato, como o original dubla o `TASClient` com `mock<TASClient>()`.
//! - **Injeção.** Os `@IService` do construtor são os campos de [`WorkbenchAssignmentServiceServices`]. O
//!   `IInstantiationService.createInstance(CopilotAssignmentFilterProvider)` e o do
//!   `GitHubCoreAssignmentsFilterProvider` (de `assignmentFilters.ts`, que depende do `IChatEntitlementService` e do
//!   `IExtensionService`) são o [`IAssignmentFilterProviderFactory`], e os provedores que ele devolve são os contratos
//!   [`ICopilotAssignmentFilterProvider`] e [`IGitHubCoreAssignmentsFilterProvider`].
//! - **`getTreatment<T extends string | number | boolean>`** devolve o [`TreatmentValue`] da plataforma (a união
//!   `string | number | boolean`), e `T | undefined` é `Option`. Um valor de configuração que não seja texto, número ou
//!   booleano não é um `T` válido e conta como ausente.
//! - **`Map<string, string>`** das propriedades de eventos do cliente é o [`OrderedMap`] (a ordem de inserção do `Map`);
//!   `mapsStrictEqualIgnoreOrder` é o de `je_base::map`.
//! - **`extends Disposable`** é o [`DisposableStore`] guardado em `store` (o `_store` do original, com o `isDisposed`).
//! - **Estado privado que o teste lê por colchetes** (`service['tasClient']`, `service['overrideInitDelay']`,
//!   `service['telemetry']` e o `Object.defineProperty(service, 'experimentsEnabled', ...)`) tem um acessor público:
//!   [`WorkbenchAssignmentService::set_tas_client`], [`WorkbenchAssignmentService::override_init_delay`],
//!   [`WorkbenchAssignmentService::telemetry`] e [`WorkbenchAssignmentService::override_experiments_enabled`].
//! - **`registerSingleton(IWorkbenchAssignmentService, ..., InstantiationType.Delayed)`** fica com o contêiner de
//!   serviços, que monta o [`WorkbenchAssignmentServiceServices`]; aqui há só o identificador
//!   [`I_WORKBENCH_ASSIGNMENT_SERVICE`]. O registro da configuração é [`register_assignment_configuration`].
//! - **`__GDPR__` e as classificações de `publicLog2`** são só anotações do compilador e não existem em tempo de execução.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::{Rc, Weak};

use indexmap::IndexMap;
use je_base::JsString;
use je_base::actions::ITelemetryData;
use je_base::async_::{async_fn, async_fn_maybe, timeout};
use je_base::cancellation::CancellationToken;
use je_base::dynamic::{Dynamic, ObjectRef, Truthy, json_stringify};
use je_base::errors::Error;
use je_base::event::{Emitter, Event};
use je_base::lifecycle::{DisposableStore, IDisposable, to_disposable};
use je_base::map::{OrderedMap, maps_strict_equal_ignore_order};
use je_base::nls::localize;
use je_base::parts::request::{HeaderValue, IRequestOptions};
use je_base::platform::Platform;
use je_base::promise::{CancelablePromise, MaybePromise, Promise};
use je_base::stopwatch::StopWatch;
use je_instantiation::service_id;
use je_platform::assignment::{
    ASSIGNMENT_REFETCH_INTERVAL, ASSIGNMENT_STORAGE_KEY, AssignmentFilterProvider, ExperimentationFilterProvider, IAssignmentService, TargetPopulation,
    TreatmentValue, VSCodeCoreAssignmentsFilterProvider, WindowKind,
};
use je_platform::configuration::registry::{ConfigurationNode, ConfigurationScope, PropertySchema, configuration_registry};
use je_platform::configuration::service::IConfigurationService;
use je_platform::product::IProductService;
use je_platform::request::{IRequestService, as_json};
use je_platform::storage::{StorageScope, StorageService, StorageTarget};
use je_platform::telemetry::ITelemetryService;
use je_platform::telemetry::telemetry_utils::TelemetryTrustedValue;

use super::assignment_context_filter::AssignmentContextFilter;
use crate::common::memento::Memento;
use crate::services::accounts::default_account::IDefaultAccountService;
use crate::services::editor::editor_resolver_service::workbench_configuration_node_base;
use crate::services::environment::environment_service::IWorkbenchEnvironmentService;
use crate::services::telemetry::common::workbench_telemetry_utils::experiments_enabled;

/// `IAssignmentFilter`.
pub trait IAssignmentFilter {
    /// Stable identifier for this filter. Used to persist and reconcile the set of assignment-context ids this filter
    /// has excluded, independently of other filters.
    fn id(&self) -> &str;

    fn exclude(&self, assignment: &str) -> bool;

    fn on_did_change(&self) -> Event<()>;
}

service_id!(pub I_WORKBENCH_ASSIGNMENT_SERVICE: dyn IWorkbenchAssignmentService = "assignmentService");

/// Scope prefix that the new TAS assignments endpoint (`/api/v1/assignments`) prepends to the feature variable keys it
/// returns (e.g. `/vscode/config.chat...`). The legacy endpoint and VS Code both query treatments by the bare name, so
/// this prefix must be accounted for when a bare lookup misses. This is an interim workaround until tas-client strips
/// the scope itself.
const ASSIGNMENTS_SCOPE_PREFIX: &str = "/vscode/";

/// Resolves a treatment value preferring the `/vscode/`-scoped key emitted by the new TAS assignments endpoint over the
/// bare key used by the legacy endpoint, so the new endpoint wins when both assign a treatment (matching the behavior
/// once tas-client strips the scope itself). Falls back to the bare key for treatments served only by the legacy
/// endpoint.
///
/// Exported for testing.
pub fn resolve_scoped_treatment(read: impl Fn(&str) -> Option<TreatmentValue>, name: &str) -> Option<TreatmentValue> {
    let scoped = read(&format!("{ASSIGNMENTS_SCOPE_PREFIX}{name}"));
    if scoped.is_some() { scoped } else { read(name) }
}

/// Builds the telemetry payload for a tas-client feature query. The queried-feature name is marked trusted so the
/// telemetry cleaner does not redact a `/vscode/`-scoped name as a `user-file-path`.
///
/// Exported for testing.
pub fn to_experiment_telemetry_data(props: &OrderedMap<String, String>) -> ITelemetryData {
    let mut data = ITelemetryData::default();
    for (key, value) in props.iter() {
        let value = if key == "ABExp.queriedFeature" { TelemetryTrustedValue { value: Dynamic::string(value) }.to_dynamic() } else { Dynamic::string(value) };
        data.extra.push((key.clone(), value));
    }
    data
}

/// `IWorkbenchAssignmentService`.
pub trait IWorkbenchAssignmentService: IAssignmentService {
    fn get_current_experiments(&self) -> Promise<Option<Vec<String>>>;
    fn add_telemetry_assignment_filter(&self, filter: Rc<dyn IAssignmentFilter>);
    /// Resolves the effective value without delaying developer overrides; assignment metadata always comes from TAS.
    fn get_treatment_with_assignment(&self, name: &str) -> Promise<ITreatmentWithAssignment>;
}

/// `ITreatmentWithAssignment<T>`.
#[derive(Clone)]
pub struct ITreatmentWithAssignment {
    pub value: Option<TreatmentValue>,
    /// May remain pending after a developer override resolves; rejects on failure or cancellation, never conflating
    /// these with absence.
    pub has_assignment: Promise<bool>,
}

/// `resolveTreatmentWithAssignment`.
pub fn resolve_treatment_with_assignment(
    override_value: Option<TreatmentValue>,
    read_assignment: impl FnOnce() -> Promise<Option<TreatmentValue>> + 'static,
) -> Promise<ITreatmentWithAssignment> {
    async_fn(async move {
        if let Some(value) = override_value {
            return Ok(ITreatmentWithAssignment { value: Some(value), has_assignment: read_assignment().then(|value| value.is_some()) });
        }
        let value = read_assignment().await?;
        let has_assignment = Promise::resolved(value.is_some());
        Ok(ITreatmentWithAssignment { value, has_assignment })
    })
}

/// `IKeyValueStorage` (de `tas-client`): o armazenamento em que o cliente guarda os dados de atribuição.
pub trait IKeyValueStorage {
    fn get_value(&self, key: &str, default_value: Option<Dynamic>) -> Promise<Option<Dynamic>>;
    fn set_value(&self, key: &str, value: Dynamic);
}

/// `IExperimentationTelemetry` (de `tas-client`).
pub trait IExperimentationTelemetry {
    fn set_shared_property(&self, name: &str, value: &str);
    fn post_event(&self, event_name: &str, props: &OrderedMap<String, String>);
}

/// O `init` do `assignmentsFetch` do `tas-client`: `{ method: 'POST', headers, body }`.
#[derive(Clone, Debug)]
pub struct AssignmentsFetchInit {
    pub method: String,
    pub headers: IndexMap<String, String>,
    pub body: String,
}

/// O retorno do `assignmentsFetch`: `{ status, json() }`.
#[derive(Clone)]
pub struct AssignmentsFetchResponse {
    pub status: u32,
    pub json: Rc<dyn Fn() -> Promise<Dynamic>>,
}

/// O `assignmentsFetch` injetável do `tas-client`.
pub type AssignmentsFetch = Rc<dyn Fn(&str, AssignmentsFetchInit) -> Promise<AssignmentsFetchResponse>>;

/// As opções de `new tasClientModule.ExperimentationService({...})`.
pub struct TasClientOptions {
    pub filter_providers: Vec<Rc<dyn ExperimentationFilterProvider>>,
    pub telemetry: Rc<dyn IExperimentationTelemetry>,
    pub storage_key: String,
    pub key_value_storage: Rc<dyn IKeyValueStorage>,
    pub assignment_context_telemetry_property_name: String,
    pub telemetry_event_name: String,
    pub endpoint: String,
    pub extension_name: String,
    pub assignments_endpoint: Option<String>,
    pub assignments_filter_providers: Option<Vec<Rc<dyn ExperimentationFilterProvider>>>,
    pub assignments_fetch: Option<AssignmentsFetch>,
    pub refetch_interval: u64,
}

/// `ExperimentationService` de `tas-client` (o `TASClient` do original), no que o serviço usa.
pub trait TasClient {
    /// `initializePromise`.
    fn initialize_promise(&self) -> Promise<()>;
    /// `initialFetch`.
    fn initial_fetch(&self) -> Promise<()>;
    /// `getTreatmentVariableAsync(configId, name, checkCache)`.
    fn get_treatment_variable_async(&self, config_id: &str, name: &str, check_cache: bool) -> Promise<Option<TreatmentValue>>;
    /// `getTreatmentVariable(configId, name)`.
    fn get_treatment_variable(&self, config_id: &str, name: &str) -> Option<TreatmentValue>;
    /// `dispose()`: só para o polling do cliente.
    fn dispose(&self);
}

/// O módulo `tas-client` carregado: o construtor de `ExperimentationService`.
pub trait TasClientModule {
    fn create_experimentation_service(&self, options: TasClientOptions) -> Rc<dyn TasClient>;
}

/// `resolveAmdNodeModulePath('tas-client', 'dist/tas-client.min.js')` e o `await import(...)` do resultado.
pub trait TasClientModuleLoader {
    fn import_tas_client(&self) -> Promise<Rc<dyn TasClientModule>>;
}

/// `CopilotAssignmentFilterProvider` (de `assignmentFilters.ts`), no que o serviço usa.
pub trait ICopilotAssignmentFilterProvider: ExperimentationFilterProvider + IDisposable {
    fn on_did_change_filters(&self) -> Event<()>;
    /// Maps the core-available Copilot filters to new TAS userParams without forwarding legacy headers.
    fn get_assignments_filters(&self) -> IndexMap<String, Option<String>>;
}

/// `GitHubCoreAssignmentsFilterProvider` (de `assignmentFilters.ts`), no que o serviço usa.
pub trait IGitHubCoreAssignmentsFilterProvider: ExperimentationFilterProvider + IDisposable {
    fn on_did_change_filters(&self) -> Event<()>;
}

/// O `IInstantiationService.createInstance` dos dois provedores de filtros de `assignmentFilters.ts`.
pub trait IAssignmentFilterProviderFactory {
    fn create_copilot_assignment_filter_provider(&self) -> Rc<dyn ICopilotAssignmentFilterProvider>;
    fn create_git_hub_core_assignments_filter_provider(&self) -> Rc<dyn IGitHubCoreAssignmentsFilterProvider>;
}

fn empty_object() -> Dynamic {
    Dynamic::object(Vec::<(&str, Dynamic)>::new())
}

/// O `T` de uma configuração: texto, número ou booleano (qualquer outro valor conta como ausente).
fn treatment_from_dynamic(value: &Dynamic) -> Option<TreatmentValue> {
    match value {
        Dynamic::String(text) => Some(TreatmentValue::String(text.to_string_lossy())),
        Dynamic::Number(number) => Some(TreatmentValue::Number(*number)),
        Dynamic::Bool(flag) => Some(TreatmentValue::Boolean(*flag)),
        _ => None,
    }
}

/// `JSON.stringify(result)`: `undefined` (`None`) continua `undefined`.
fn stringify_treatment(result: Option<&TreatmentValue>) -> Option<String> {
    let value = match result? {
        TreatmentValue::String(text) => Dynamic::string(text),
        TreatmentValue::Number(number) => Dynamic::Number(*number),
        TreatmentValue::Boolean(flag) => Dynamic::Bool(*flag),
    };
    json_stringify(&value)
}

/// `MementoKeyValueStorage`.
struct MementoKeyValueStorage {
    memento: Memento,
    memento_obj: ObjectRef,
}

impl MementoKeyValueStorage {
    fn new(memento: Memento) -> Self {
        let memento_obj = memento.get_memento(StorageScope::Application, StorageTarget::Machine);
        Self { memento, memento_obj }
    }
}

impl IKeyValueStorage for MementoKeyValueStorage {
    fn get_value(&self, key: &str, default_value: Option<Dynamic>) -> Promise<Option<Dynamic>> {
        let value = self.memento_obj.get(&JsString::from(key));
        async_fn(async move {
            // `await this.mementoObj[key]`: o `await` de um valor que não é promessa ainda cede uma vez.
            Promise::resolved(()).await?;
            Ok(if value.is_truthy() { value } else { default_value })
        })
    }

    fn set_value(&self, key: &str, value: Dynamic) {
        self.memento_obj.set(&JsString::from(key), value);
        self.memento.save_memento();
    }
}

/// O estado mutável de [`WorkbenchAssignmentServiceTelemetry`].
#[derive(Default)]
struct TelemetryState {
    previous_assignment_context: Option<String>,
    last_assignment_context: Option<String>,
    last_logged_treatments: HashMap<String, LoggedTreatment>,
    last_logged_assignments_validation: Option<LoggedAssignmentsValidation>,
}

struct LoggedTreatment {
    value: Option<String>,
    assignment_context: Option<String>,
}

struct LoggedAssignmentsValidation {
    properties: OrderedMap<String, String>,
    assignment_context: Option<String>,
}

/// `WorkbenchAssignmentServiceTelemetry`.
pub struct WorkbenchAssignmentServiceTelemetry {
    store: Rc<DisposableStore>,
    on_did_update_assignment_context: Rc<Emitter<()>>,
    state: RefCell<TelemetryState>,
    telemetry_service: Rc<dyn ITelemetryService>,
    product_service: Rc<dyn IProductService>,
    context_filter: Rc<AssignmentContextFilter>,
}

impl WorkbenchAssignmentServiceTelemetry {
    pub fn new(telemetry_service: Rc<dyn ITelemetryService>, product_service: Rc<dyn IProductService>, context_filter: Rc<AssignmentContextFilter>) -> Rc<Self> {
        let store = DisposableStore::new();
        let on_did_update_assignment_context = store.add_rc(Emitter::<()>::new());
        let this = Rc::new(Self { store, on_did_update_assignment_context, state: RefCell::new(TelemetryState::default()), telemetry_service, product_service, context_filter });

        // Re-apply the filters whenever a filter is added or changes its exclusion decisions.
        let weak = Rc::downgrade(&this);
        let subscription = this.context_filter.on_did_change().listen(move |()| {
            let Some(this) = weak.upgrade() else {
                return;
            };
            let previous = this.state.borrow().previous_assignment_context.clone();
            if let Some(previous) = previous.filter(|context| !context.is_empty()) {
                this.set_assignment_context(&previous);
            }
        });
        this.store.add(subscription);
        this
    }

    /// `onDidUpdateAssignmentContext`.
    pub fn on_did_update_assignment_context(&self) -> Event<()> {
        self.on_did_update_assignment_context.event()
    }

    /// `assignmentContext`.
    pub fn assignment_context(&self) -> Option<Vec<String>> {
        self.state.borrow().last_assignment_context.as_ref().map(|context| context.split(';').map(str::to_owned).collect())
    }

    /// `tasConfig?.assignmentContextTelemetryPropertyName`.
    fn assignment_context_property_name(&self) -> Option<String> {
        self.product_service.tas_config().map(|config| config.assignment_context_telemetry_property_name.clone())
    }

    fn set_assignment_context(&self, value: &str) {
        let filtered_value = self.context_filter.filter(value);
        self.state.borrow_mut().last_assignment_context = Some(filtered_value.clone());
        self.on_did_update_assignment_context.fire(());

        if let Some(name) = self.assignment_context_property_name().filter(|name| !name.is_empty()) {
            self.telemetry_service.set_experiment_property(&name, &filtered_value);
        }
    }

    pub fn log_treatment(&self, name: &str, result: Option<&TreatmentValue>) {
        let treatment_value = stringify_treatment(result);
        {
            let mut state = self.state.borrow_mut();
            let last_assignment_context = state.last_assignment_context.clone();
            if let Some(previous) = state.last_logged_treatments.get(name)
                && previous.value == treatment_value
                && previous.assignment_context == last_assignment_context
            {
                return;
            }
            state.last_logged_treatments.insert(name.to_owned(), LoggedTreatment { value: treatment_value.clone(), assignment_context: last_assignment_context });
        }

        self.telemetry_service.public_log2(
            "tasClientReadTreatmentComplete",
            Some(Dynamic::object([
                ("treatmentName", Dynamic::string(name)),
                ("treatmentValue", treatment_value.as_deref().map_or(Dynamic::Undefined, Dynamic::string)),
            ])),
        );
    }
}

impl IExperimentationTelemetry for WorkbenchAssignmentServiceTelemetry {
    // __GDPR__COMMON__ "abexp.assignmentcontext" : { "classification": "SystemMetaData", "purpose": "FeatureInsight" }
    fn set_shared_property(&self, name: &str, value: &str) {
        if self.assignment_context_property_name().as_deref() == Some(name) {
            self.state.borrow_mut().previous_assignment_context = Some(value.to_owned());
            return self.set_assignment_context(value);
        }

        self.telemetry_service.set_experiment_property(name, value);
    }

    fn post_event(&self, event_name: &str, props: &OrderedMap<String, String>) {
        if event_name == "assignments-validation" {
            let mut state = self.state.borrow_mut();
            let last_assignment_context = state.last_assignment_context.clone();
            if let Some(previous) = &state.last_logged_assignments_validation
                && previous.assignment_context == last_assignment_context
                && maps_strict_equal_ignore_order(&previous.properties, props)
            {
                return;
            }
            state.last_logged_assignments_validation = Some(LoggedAssignmentsValidation { properties: props.clone(), assignment_context: last_assignment_context });
        }

        let data = to_experiment_telemetry_data(props);

        self.telemetry_service.public_log(event_name, Some(data));
    }
}

impl IDisposable for WorkbenchAssignmentServiceTelemetry {
    fn dispose(&self) {
        self.store.dispose();
    }
}

/// O `IKeyValueStorage` de uma montagem do cliente: as escritas de um cliente substituído (revogado) não chegam ao memento.
struct SetupKeyValueStorage {
    inner: Rc<dyn IKeyValueStorage>,
    revoked: Rc<Cell<bool>>,
}

impl IKeyValueStorage for SetupKeyValueStorage {
    fn get_value(&self, key: &str, default_value: Option<Dynamic>) -> Promise<Option<Dynamic>> {
        self.inner.get_value(key, default_value)
    }

    fn set_value(&self, key: &str, value: Dynamic) {
        if !self.revoked.get() {
            self.inner.set_value(key, value);
        }
    }
}

/// O `IExperimentationTelemetry` de uma montagem do cliente: a telemetria de um cliente revogado é descartada.
struct SetupTelemetry {
    inner: Rc<WorkbenchAssignmentServiceTelemetry>,
    revoked: Rc<Cell<bool>>,
}

impl IExperimentationTelemetry for SetupTelemetry {
    fn set_shared_property(&self, name: &str, value: &str) {
        if !self.revoked.get() {
            self.inner.set_shared_property(name, value);
        }
    }

    fn post_event(&self, event_name: &str, props: &OrderedMap<String, String>) {
        if !self.revoked.get() {
            self.inner.post_event(event_name, props);
        }
    }
}

/// `{ getFilters: () => extensionsFilterProvider.getAssignmentsFilters() }`. O objeto do original só tem `getFilters`; o
/// `tas-client` lê só ele dos provedores do endpoint de atribuições, e o `getFilterValue` do contrato responde com a
/// consulta ao mesmo mapa.
struct ExtensionsAssignmentsFilterProvider {
    provider: Rc<dyn ICopilotAssignmentFilterProvider>,
}

impl ExperimentationFilterProvider for ExtensionsAssignmentsFilterProvider {
    fn get_filter_value(&self, filter: &str) -> Option<String> {
        self.provider.get_assignments_filters().get(filter).cloned().flatten()
    }

    fn get_filters(&self) -> IndexMap<String, Option<String>> {
        self.provider.get_assignments_filters()
    }
}

/// O transporte do endpoint de atribuições, apoiado no serviço de requisições do processo principal (evita o CORS do
/// renderizador). A forma é a do `assignmentsFetch` injetável do `tas-client`.
fn assignments_fetch(request_service: Rc<dyn IRequestService>, url: &str, init: AssignmentsFetchInit) -> Promise<AssignmentsFetchResponse> {
    let options = IRequestOptions {
        r#type: Some(init.method),
        url: Some(url.to_owned()),
        data: Some(init.body),
        headers: Some(init.headers.into_iter().map(|(name, value)| (name, HeaderValue::One(value))).collect()),
        disable_cache: Some(true),
        call_site: "assignmentService.assignments".to_owned(),
        ..IRequestOptions::default()
    };
    async_fn(async move {
        let context = request_service.request(options, CancellationToken::None).await?;
        let status = u32::from(context.res.status_code.unwrap_or(0));
        let json: Rc<dyn Fn() -> Promise<Dynamic>> = Rc::new(move || {
            let context = context.clone();
            async_fn(async move { Ok(as_json(&context).await?.unwrap_or_else(empty_object)) })
        });
        Ok(AssignmentsFetchResponse { status, json })
    })
}

/// Os serviços que o construtor do `WorkbenchAssignmentService` recebe por injeção.
pub struct WorkbenchAssignmentServiceServices {
    pub telemetry_service: Rc<dyn ITelemetryService>,
    pub storage_service: Rc<dyn StorageService>,
    pub configuration_service: Rc<dyn IConfigurationService>,
    pub product_service: Rc<dyn IProductService>,
    pub environment_service: Rc<dyn IWorkbenchEnvironmentService>,
    pub filter_provider_factory: Rc<dyn IAssignmentFilterProviderFactory>,
    pub default_account_service: Rc<dyn IDefaultAccountService>,
    pub request_service: Rc<dyn IRequestService>,
    pub tas_client_loader: Rc<dyn TasClientModuleLoader>,
}

/// `WorkbenchAssignmentService`.
pub struct WorkbenchAssignmentService {
    me: Weak<Self>,
    store: Rc<DisposableStore>,

    tas_client: RefCell<Option<Promise<Rc<dyn TasClient>>>>,
    tas_setup_disposables: Rc<DisposableStore>,

    assignments_endpoint: RefCell<Option<String>>,

    network_initialized: Cell<bool>,
    setup_generation: Cell<u32>,
    /// Revokes the current setup's storage/telemetry/fetch wrappers, neutralizing a superseded in-flight client.
    revoke_current_setup: RefCell<Option<Rc<dyn Fn()>>>,
    override_init_delay: CancelablePromise<()>,

    context_filter: Rc<AssignmentContextFilter>,
    telemetry: Rc<WorkbenchAssignmentServiceTelemetry>,
    key_value_storage: Rc<dyn IKeyValueStorage>,

    experiments_enabled: Cell<bool>,

    on_did_refetch_assignments: Rc<Emitter<()>>,

    telemetry_service: Rc<dyn ITelemetryService>,
    configuration_service: Rc<dyn IConfigurationService>,
    product_service: Rc<dyn IProductService>,
    environment_service: Rc<dyn IWorkbenchEnvironmentService>,
    filter_provider_factory: Rc<dyn IAssignmentFilterProviderFactory>,
    default_account_service: Rc<dyn IDefaultAccountService>,
    request_service: Rc<dyn IRequestService>,
    tas_client_loader: Rc<dyn TasClientModuleLoader>,
}

impl WorkbenchAssignmentService {
    pub fn new(services: WorkbenchAssignmentServiceServices) -> Rc<Self> {
        let experiments_enabled = experiments_enabled(services.configuration_service.as_ref(), services.product_service.as_ref(), services.environment_service.as_ref());

        // Os campos que o original inicializa antes do corpo do construtor, na ordem em que se registram.
        let store = DisposableStore::new();
        let tas_setup_disposables = store.add_rc(DisposableStore::new());
        let on_did_refetch_assignments = store.add_rc(Emitter::<()>::new());

        let context_filter = AssignmentContextFilter::new(services.storage_service.clone());
        let telemetry = WorkbenchAssignmentServiceTelemetry::new(services.telemetry_service.clone(), services.product_service.clone(), context_filter.clone());
        let key_value_storage: Rc<dyn IKeyValueStorage> =
            Rc::new(MementoKeyValueStorage::new(Memento::new("experiment.service.memento", services.storage_service.clone())));

        // For development purposes, configure the delay until tas local tas treatment ovverrides are available
        let override_delay = match services.configuration_service.get_value(Some("experiments.overrideDelay"), None) {
            Dynamic::Number(delay) => delay,
            _ => 0.0,
        };
        let override_init_delay = timeout(if override_delay > 0.0 { override_delay as u64 } else { 0 });

        let this = Rc::new_cyclic(|me| Self {
            me: me.clone(),
            store,
            tas_client: RefCell::new(None),
            tas_setup_disposables,
            assignments_endpoint: RefCell::new(None),
            network_initialized: Cell::new(false),
            setup_generation: Cell::new(0),
            revoke_current_setup: RefCell::new(None),
            override_init_delay,
            context_filter,
            telemetry,
            key_value_storage,
            experiments_enabled: Cell::new(experiments_enabled),
            on_did_refetch_assignments,
            telemetry_service: services.telemetry_service,
            configuration_service: services.configuration_service,
            product_service: services.product_service,
            environment_service: services.environment_service,
            filter_provider_factory: services.filter_provider_factory,
            default_account_service: services.default_account_service,
            request_service: services.request_service,
            tas_client_loader: services.tas_client_loader,
        });

        if experiments_enabled {
            let tas_client = this.setup_tas_client();
            *this.tas_client.borrow_mut() = Some(tas_client);

            // The assignments endpoint is sourced from account entitlements, which load asynchronously. The initial
            // account load resolves the readiness barrier without firing onDidChangeDefaultAccount, so proactively
            // re-check once it is ready, and again whenever the account changes later.
            let weak = this.me.clone();
            drop(this.default_account_service.get_default_account().then(move |_| {
                if let Some(this) = weak.upgrade() {
                    this.recreate_tas_client_if_endpoint_changed();
                }
            }));
            let weak = this.me.clone();
            this.store.add(this.default_account_service.on_did_change_default_account().listen(move |_| {
                if let Some(this) = weak.upgrade() {
                    this.recreate_tas_client_if_endpoint_changed();
                }
            }));

            // Stop the final client's auto-polling and revoke its wrappers when the service is disposed.
            let weak = this.me.clone();
            this.store.add(to_disposable(move || {
                if let Some(this) = weak.upgrade() {
                    this.revoke_setup();
                    Self::dispose_tas_client(this.tas_client.borrow().clone());
                }
            }));
        }

        this.store.add_rc(this.context_filter.clone());
        this.store.add_rc(this.telemetry.clone());
        let on_did_refetch_assignments = this.on_did_refetch_assignments.clone();
        this.store.add(this.telemetry.on_did_update_assignment_context().listen(move |()| on_did_refetch_assignments.fire(())));
        let on_did_refetch_assignments = this.on_did_refetch_assignments.clone();
        this.store.add(this.configuration_service.on_did_change_configuration().listen(move |event| {
            if event.affects_configuration("experiments.override", None) {
                on_did_refetch_assignments.fire(());
            }
        }));

        this
    }

    fn rc(&self) -> Rc<Self> {
        self.me.upgrade().expect("o serviço de atribuição está vivo enquanto é chamado")
    }

    /// `service['tasClient'] = value`.
    pub fn set_tas_client(&self, tas_client: Promise<Rc<dyn TasClient>>) {
        *self.tas_client.borrow_mut() = Some(tas_client);
    }

    /// `Object.defineProperty(service, 'experimentsEnabled', { value })`.
    pub fn override_experiments_enabled(&self, value: bool) {
        self.experiments_enabled.set(value);
    }

    /// `service['overrideInitDelay']`.
    pub fn override_init_delay(&self) -> Promise<()> {
        self.override_init_delay.promise.clone()
    }

    /// `service['telemetry']`.
    pub fn telemetry(&self) -> Rc<WorkbenchAssignmentServiceTelemetry> {
        self.telemetry.clone()
    }

    /// `experiments.override.<name>`, o valor de desenvolvimento que vale no lugar da atribuição.
    fn read_override(&self, name: &str) -> Option<TreatmentValue> {
        let key = format!("experiments.override.{name}");
        treatment_from_dynamic(&self.configuration_service.get_value(Some(key.as_str()), None))
    }

    fn do_get_treatment(self: &Rc<Self>, name: &str) -> Promise<Option<TreatmentValue>> {
        let this = self.clone();
        let name = name.to_owned();
        async_fn_maybe(async move {
            this.override_init_delay.promise.clone().await?; // For development purposes, allow overriding tas assignments to test variants locally.

            if let Some(override_value) = this.read_override(&name) {
                return Ok(MaybePromise::Value(Some(override_value)));
            }

            Ok(MaybePromise::Promise(this.get_assigned_treatment(&name, false)))
        })
    }

    fn get_assigned_treatment(self: &Rc<Self>, name: &str, require_current_client: bool) -> Promise<Option<TreatmentValue>> {
        let this = self.clone();
        let name = name.to_owned();
        async_fn(async move {
            if this.tas_client.borrow().is_none() {
                return Ok(None);
            }

            if !this.experiments_enabled.get() {
                return Ok(None);
            }

            for _attempt in 0..2 {
                if require_current_client && this.store.is_disposed() {
                    return Err(Error::cancellation());
                }
                let client_promise = this.tas_client.borrow().clone();
                let Some(client_promise) = client_promise else {
                    return Ok(None);
                };
                let client = client_promise.clone().await?;

                // Prefer cached treatments while the initial fetch is still pending.
                if !this.network_initialized.get() {
                    client.get_treatment_variable_async("vscode", &format!("{ASSIGNMENTS_SCOPE_PREFIX}{name}"), true).await?;
                }
                // Assignment metadata must not outlive its client; legacy value-only reads retain their original contract.
                if require_current_client {
                    if this.store.is_disposed() {
                        return Err(Error::cancellation());
                    }
                    let is_current = this.tas_client.borrow().as_ref().is_some_and(|current| current.ptr_eq(&client_promise));
                    if !is_current {
                        continue;
                    }
                }
                return Ok(resolve_scoped_treatment(|read_name| client.get_treatment_variable("vscode", read_name), &name));
            }
            Err(Error::cancellation())
        })
    }

    /// Resolves the new TAS assignments API URL from the account entitlements `exp` endpoint, or `None` when no
    /// account/endpoint is available.
    fn get_assignments_endpoint(&self) -> Option<String> {
        let account = self.default_account_service.current_default_account();
        let exp = account?.entitlements_data?.endpoints?.get("exp").cloned().filter(|exp| !exp.is_empty())?;
        Some(format!("{}/api/v1/assignments", exp.trim_end_matches('/')))
    }

    /// Recreates the TAS client when the resolved assignments endpoint has changed.
    fn recreate_tas_client_if_endpoint_changed(self: &Rc<Self>) {
        if self.store.is_disposed() {
            return; // the service was disposed before the (async) account load resolved
        }
        let next = self.get_assignments_endpoint();
        let changed = next != *self.assignments_endpoint.borrow();
        if changed {
            let tas_client = self.setup_tas_client();
            *self.tas_client.borrow_mut() = Some(tas_client);
        }
    }

    /// `this.revokeCurrentSetup?.()`.
    fn revoke_setup(&self) {
        let revoke = self.revoke_current_setup.borrow().clone();
        if let Some(revoke) = revoke {
            revoke();
        }
    }

    fn setup_tas_client(self: &Rc<Self>) -> Promise<Rc<dyn TasClient>> {
        let this = self.clone();
        async_fn(async move {
            this.tas_setup_disposables.clear();

            // Each setup supersedes the previous client; track a generation so a stale client's initialFetch cannot flip
            // networkInitialized for a newer client.
            let generation = this.setup_generation.get() + 1;
            this.setup_generation.set(generation);
            this.network_initialized.set(false);

            // Revoke the previous setup's wrappers, then dispose its client. Revoking neutralizes a superseded,
            // still-in-flight client: after replacement it can no longer write the shared memento, emit telemetry, or
            // hit the assignments endpoint. This is needed because the tas-client's dispose() only stops its polling
            // timer, not an already-running fetch.
            this.revoke_setup();
            Self::dispose_tas_client(this.tas_client.borrow().clone());

            let revoked = Rc::new(Cell::new(false));
            let revoke_flag = revoked.clone();
            *this.revoke_current_setup.borrow_mut() = Some(Rc::new(move || revoke_flag.set(true)));

            let key_value_storage: Rc<dyn IKeyValueStorage> = Rc::new(SetupKeyValueStorage { inner: this.key_value_storage.clone(), revoked: revoked.clone() });
            let telemetry: Rc<dyn IExperimentationTelemetry> = Rc::new(SetupTelemetry { inner: this.telemetry.clone(), revoked: revoked.clone() });

            let product_service = this.product_service.clone();
            let target_population = match product_service.quality() {
                Some("stable") => TargetPopulation::Public,
                Some("exploration") => TargetPopulation::Exploration,
                _ => TargetPopulation::Insiders,
            };
            let window_kind = if this.environment_service.is_sessions_window() { WindowKind::Agents } else { WindowKind::Editor };
            let release_date = product_service.date().unwrap_or("").to_owned();

            let filter_provider: Rc<dyn ExperimentationFilterProvider> = Rc::new(AssignmentFilterProvider::new(
                product_service.version(),
                product_service.name_long(),
                &this.telemetry_service.machine_id(),
                &this.telemetry_service.dev_device_id(),
                target_population,
                &release_date,
                window_kind,
                Platform::detect(),
            ));

            let extensions_filter_provider = this.filter_provider_factory.create_copilot_assignment_filter_provider();
            this.tas_setup_disposables.add(extensions_filter_provider.clone());
            let weak = this.me.clone();
            this.tas_setup_disposables.add(extensions_filter_provider.on_did_change_filters().listen(move |()| {
                if let Some(this) = weak.upgrade() {
                    drop(this.refetch_assignments());
                }
            }));

            // New TAS assignments API. Its endpoint is sourced from account entitlements and it uses dedicated providers
            // that emit the new userParam key names, so the legacy filter keys never reach it. Its assignments are merged
            // with the legacy provider's results.
            let assignments_endpoint = this.get_assignments_endpoint();
            *this.assignments_endpoint.borrow_mut() = assignments_endpoint.clone();
            let mut assignments_filter_providers: Option<Vec<Rc<dyn ExperimentationFilterProvider>>> = None;
            if assignments_endpoint.is_some() {
                let core_assignments_filter_provider: Rc<dyn ExperimentationFilterProvider> = Rc::new(VSCodeCoreAssignmentsFilterProvider::new(
                    product_service.version(),
                    product_service.name_long(),
                    &this.telemetry_service.dev_device_id(),
                    target_population,
                    &release_date,
                    window_kind,
                    Platform::detect(),
                ));
                let github_assignments_filter_provider = this.filter_provider_factory.create_git_hub_core_assignments_filter_provider();
                this.tas_setup_disposables.add(github_assignments_filter_provider.clone());
                let weak = this.me.clone();
                this.tas_setup_disposables.add(github_assignments_filter_provider.on_did_change_filters().listen(move |()| {
                    if let Some(this) = weak.upgrade() {
                        drop(this.refetch_assignments());
                    }
                }));
                let github_filters: Rc<dyn ExperimentationFilterProvider> = github_assignments_filter_provider;
                let extensions_filters: Rc<dyn ExperimentationFilterProvider> =
                    Rc::new(ExtensionsAssignmentsFilterProvider { provider: extensions_filter_provider.clone() });
                assignments_filter_providers = Some(vec![core_assignments_filter_provider, github_filters, extensions_filters]);
            }

            let tas_config = product_service.tas_config().cloned();

            // tas-client ships as pure ESM; load it via a runtime-resolved URL so bundlers do not rewrite the import
            // (mirrors how the editor loads the `@vscode/diff` module).
            let tas_client_module = this.tas_client_loader.import_tas_client().await?;
            let Some(tas_config) = tas_config else {
                return Err(Error::type_error("Cannot read properties of undefined (reading 'assignmentContextTelemetryPropertyName')"));
            };

            // Route the assignments request through the main-process request service so it is not subject to renderer CORS
            // (parity with how core reaches api.github.com).
            let assignments_fetch_transport: Option<AssignmentsFetch> = assignments_endpoint.as_ref().map(|_| {
                let revoked = revoked.clone();
                let request_service = this.request_service.clone();
                let transport: AssignmentsFetch = Rc::new(move |url: &str, init: AssignmentsFetchInit| {
                    if revoked.get() {
                        Promise::resolved(AssignmentsFetchResponse { status: 0, json: Rc::new(|| async_fn(async { Ok(empty_object()) })) })
                    } else {
                        assignments_fetch(request_service.clone(), url, init)
                    }
                });
                transport
            });

            // Measure the client-side latency of the first network call to the Treatment Assignment Service. The fetch is
            // triggered by constructing the client, so start timing right before construction to exclude module loading
            // time from the measurement.
            let fetch_stop_watch = StopWatch::create(None);
            let extensions_experimentation_filter_provider: Rc<dyn ExperimentationFilterProvider> = extensions_filter_provider;
            let tas_client = tas_client_module.create_experimentation_service(TasClientOptions {
                filter_providers: vec![filter_provider, extensions_experimentation_filter_provider],
                telemetry,
                storage_key: ASSIGNMENT_STORAGE_KEY.to_owned(),
                key_value_storage,
                assignment_context_telemetry_property_name: tas_config.assignment_context_telemetry_property_name,
                telemetry_event_name: tas_config.telemetry_event_name,
                endpoint: tas_config.endpoint,
                extension_name: "vscode-core".to_owned(),
                assignments_endpoint,
                assignments_filter_providers,
                assignments_fetch: assignments_fetch_transport,
                refetch_interval: ASSIGNMENT_REFETCH_INTERVAL,
            });

            tas_client.initialize_promise().await?;
            let initial_fetch_service = this.clone();
            tas_client
                .initial_fetch()
                .then(move |()| {
                    if generation != initial_fetch_service.setup_generation.get() {
                        return; // superseded by a newer setup
                    }
                    initial_fetch_service.network_initialized.set(true);
                    initial_fetch_service.log_fetch_latency("initial", fetch_stop_watch.elapsed());
                })
                .catch(|_| ());

            Ok(tas_client)
        })
    }

    fn log_fetch_latency(&self, fetch_type: &str, duration_ms: f64) {
        self.telemetry_service.public_log2(
            "tasClientFetchLatency",
            Some(Dynamic::object([("fetchType", Dynamic::string(fetch_type)), ("durationMs", Dynamic::Number(duration_ms))])),
        );
    }

    fn refetch_assignments(self: &Rc<Self>) -> Promise<()> {
        let tas_client = self.tas_client.borrow().clone();
        let this = self.clone();
        async_fn(async move {
            let Some(tas_client) = tas_client else {
                return Ok(()); // Setup has not started, assignments will use latest filters
            };

            // Await the client to be setup and the initial fetch to complete
            let tas_client = tas_client.await?;
            tas_client.initial_fetch().await?;

            // Refresh the assignments and measure the network latency of the refetch.
            let refetch_stop_watch = StopWatch::create(None);
            tas_client.get_treatment_variable_async("vscode", "refresh", false).await?;
            this.log_fetch_latency("refetch", refetch_stop_watch.elapsed());
            Ok(())
        })
    }

    /// Stops a TAS client's auto-polling once it resolves. Safe to call with `None`.
    fn dispose_tas_client(client: Option<Promise<Rc<dyn TasClient>>>) {
        if let Some(client) = client {
            client.then(|client| client.dispose()).catch(|_| ());
        }
    }
}

impl IAssignmentService for WorkbenchAssignmentService {
    fn on_did_refetch_assignments(&self) -> Event<()> {
        self.on_did_refetch_assignments.event()
    }

    fn get_treatment(&self, name: &str) -> Promise<Option<TreatmentValue>> {
        let this = self.rc();
        let name = name.to_owned();
        async_fn(async move {
            let result = this.do_get_treatment(&name).await?;
            this.telemetry.log_treatment(&name, result.as_ref());
            Ok(result)
        })
    }
}

impl IWorkbenchAssignmentService for WorkbenchAssignmentService {
    fn get_current_experiments(&self) -> Promise<Option<Vec<String>>> {
        let this = self.rc();
        async_fn(async move {
            let tas_client = this.tas_client.borrow().clone();
            let Some(tas_client) = tas_client else {
                return Ok(None);
            };

            if !this.experiments_enabled.get() {
                return Ok(None);
            }

            tas_client.await?;

            Ok(this.telemetry.assignment_context())
        })
    }

    fn add_telemetry_assignment_filter(&self, filter: Rc<dyn IAssignmentFilter>) {
        self.context_filter.add_filter(filter);
    }

    fn get_treatment_with_assignment(&self, name: &str) -> Promise<ITreatmentWithAssignment> {
        let this = self.rc();
        let name = name.to_owned();
        async_fn(async move {
            this.override_init_delay.promise.clone().await?;
            let override_value = this.read_override(&name);
            let assigned = this.clone();
            let assigned_name = name.clone();
            let result = resolve_treatment_with_assignment(override_value, move || assigned.get_assigned_treatment(&assigned_name, true)).await?;
            this.telemetry.log_treatment(&name, result.value.as_ref());
            Ok(result)
        })
    }
}

impl IDisposable for WorkbenchAssignmentService {
    fn dispose(&self) {
        self.store.dispose();
    }
}

/// O registro da configuração `workbench.enableExperiments` (o corpo do módulo do original roda no `import`).
pub fn register_assignment_configuration() {
    let property = PropertySchema::from_entries([
        ("type", Dynamic::string("boolean")),
        ("description", Dynamic::string(&localize("workbench.enableExperiments", "Fetches experiments to run from a Microsoft online service.", &[]))),
        ("default", Dynamic::Bool(true)),
        ("scope", ConfigurationScope::Application.to_dynamic()),
        ("restricted", Dynamic::Bool(true)),
        ("tags", Dynamic::array([Dynamic::string("usesOnlineServices")])),
    ]);
    configuration_registry().register_configuration(ConfigurationNode::new(workbench_configuration_node_base().with_properties([("workbench.enableExperiments", property)])));
}
