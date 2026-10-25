//! Porte de `src/vs/workbench/services/extensions/common/abstractExtensionService.ts`: o serviço de extensões do workbench sem
//! a parte que depende da plataforma (navegador, Electron): o registro e os deltas de extensões, o ciclo de vida dos hosts
//! (iniciar, parar com veto, reiniciar após queda), a ativação por evento e por identificador, o estado de cada extensão
//! (`ExtensionStatus`) e a resolução da autoridade remota. Os hosts ficam atrás do [`IExtensionHost`] e de cada
//! [`IExtensionHostManager`], para a implementação concreta (`ExtensionService` do navegador, `NativeExtensionService`) e para
//! os dublês de teste.
//!
//! Divergências:
//! - **Herança.** Não há classe abstrata: [`AbstractExtensionService`] é a struct, e o que o original deixa para a subclasse
//!   (`_resolveExtensions`, `_onExtensionHostExit`, `_resolveAuthority` abstratos; `_doCreateExtensionHostManager`, `_initialize`,
//!   `_onExtensionHostCrashed` sobrescrevíveis) é o trait [`AbstractExtensionServiceHooks`], que a subclasse implementa e registra
//!   com [`AbstractExtensionService::set_hooks`] antes do primeiro uso. As versões "de base" dos métodos sobrescrevíveis são
//!   [`AbstractExtensionService::initialize_base`] e [`AbstractExtensionService::on_extension_host_crashed_base`], que os corpos
//!   padrão do trait chamam (o `super._initialize()` e o `super._onExtensionHostCrashed(...)` do original).
//! - **`_doCreateExtensionHostManager` tem corpo padrão**, o do original: cria o [`ExtensionHostManager`] (ou o
//!   `LazyCreateExtensionHostManager`, quando o host é `LazyAutoStart`) com os serviços que o serviço já guarda
//!   ([`AbstractExtensionService::do_create_extension_host_manager_base`]). O hook recebe o serviço (`service`), como `initialize`, e
//!   quem o sobrescreve (um dublê de teste) entrega o gerenciador que quiser.
//! - **`AsyncIterable<ResolvedExtensions>`** é o trait [`ResolvedExtensionsStream`] (`next()` devolve `None` no fim), e as classes
//!   `ResolverExtensions`, `LocalExtensions` e `RemoteExtensions` são as variantes do [`ResolvedExtensions`]. O serviço consome cada
//!   item assim que ele chega, como o `for await` do original.
//! - **Serviços lidos.** Só entram no construtor os serviços que a classe de base lê ([`AbstractExtensionServiceServices`]):
//!   `IProductService` e `IRemoteExtensionsScannerService` (campos `protected` que só as subclasses leem) ficam com a subclasse.
//!   O `IInstantiationService` serve ao `invokeFunction` do `checkGlobFileExists`; o `createInstance(ExtensionsProposedApi)` do
//!   construtor das subclasses é feito por quem constrói o serviço (o [`ExtensionsProposedApi`] chega pronto).
//! - **Sobre `this`.** Os métodos são `&self` e usam o `Weak` do próprio serviço ([`AbstractExtensionService::me`]) para dar um `Rc`
//!   às continuações assíncronas. As promessas que o original deixa soltas (`void`) são descartadas.
//! - **`_initializeIfNeeded()`** devolve `Promise<void> | null` no original, mas nunca devolve `null` depois de criar a promessa:
//!   aqui é sempre a promessa.
//! - **`_deltaExtensions`** aplica o `updateEnabledApiProposals` antes do `registry.deltaExtensions` (no original ele vem depois e
//!   muta as mesmas referências que o registro guarda; aqui as descrições são imutáveis e compartilhadas por `Rc`). O estado final
//!   é o mesmo. `toExtensionDescription` nunca devolve `undefined`, então o ramo `could not scan extension...` não existe.
//! - **`_resolveAuthorityInitial`**: o original devolve a promessa de `_resolveAuthorityWithLogging` sem `await` dentro do `try`,
//!   então o `catch` (as tentativas e o `isNoResolverFound`) nunca roda; o porte devolve a promessa direto.
//! - **`setRemoteEnvironment`**: o original faz `await` num array de promessas (e não em `Promise.all`), então não espera os hosts.
//!   O porte dispara as chamadas e resolve.
//! - **`_doHandleExtensionPoints`**: as contribuições de cada extensão são o JSON de `desc.contributes` (ver
//!   [`super::extensions::extension_contributions`]), e `ExtensionPoint.acceptUsers` é o do [`super::extensions_registry`].
//! - `console.error` é `eprintln!`. `isCI` é [`Platform::detect`]`().is_ci`, lido uma vez. `perf.mark` é
//!   [`je_base::performance::mark`]. Não portado: o `ActivationFeatureMarkdowneRenderer` e o `registerExtensionFeature` (a aba de
//!   funcionalidades do editor de extensões).

use std::cell::{Cell, RefCell};
use std::collections::{HashSet, VecDeque};
use std::rc::{Rc, Weak};
use std::time::{SystemTime, UNIX_EPOCH};

use indexmap::{IndexMap, IndexSet};
use je_base::async_::{Barrier, async_fn};
use je_base::dynamic::Dynamic;
use je_base::error_message::to_error_message_of;
use je_base::errors::{self, Error};
use je_base::event::{Emitter, Event};
use je_base::lifecycle::{Disposable, DisposableStore, IDisposable};
use je_base::network::schemas;
use je_base::nls::localize;
use je_base::performance::mark;
use je_base::platform::Platform;
use je_base::promise::{MaybePromise, Promise};
use je_base::runtime;
use je_base::stopwatch::StopWatch;
use je_base::uri::URI;
use je_extensions::management::InstallOperation;
use je_extensions::manifest::{ExtensionIdentifier, ExtensionIdentifierMap};
use je_extensions::scanner::ExtensionDescription;
use je_instantiation::InstantiationService;
use je_platform::configuration::service::IConfigurationService;
use je_platform::dialogs::dialogs::{BaseDialogOptions, Confirmation, DialogDetail, DialogTypeOrSeverity, IDialogService};
use je_platform::files::i_file_service::IFileService;
use je_platform::log::{LogMessage, LogServiceRef, Logger};
use je_platform::notification::notification::{
    BasePromptChoice, INotificationService, Notification, NotificationMessage, NotificationProperties, PromptChoice, PromptChoiceKind, PromptOptions, Severity, StatusMessageOptions,
};
use je_platform::notification::notification_telemetry::{NotificationActionTelemetryId, NotificationTelemetry, NotificationTelemetryId};
use je_platform::remote::remote_authority_resolver::{
    IRemoteAuthorityResolverService, RemoteAuthorityResolverError, RemoteAuthorityResolverErrorCode, ResolverResult, get_remote_authority_prefix,
};
use je_platform::resources::ext_uri;
use je_platform::telemetry::telemetry::ITelemetryService;
use je_platform::workspace::IWorkspaceContextService;
use serde_json::{Map, Value};

use super::extension_activation::ExtensionActivationReason;
use super::extension_dev_options::parse_extension_dev_options;
use super::extension_description_registry::{
    ExtensionDescriptionRegistryLock, ExtensionDescriptionRegistrySnapshot, IActivationEventsReader, IReadOnlyExtensionDescriptionRegistry, LockableExtensionDescriptionRegistry,
    SharedExtensionDescription,
};
use super::extension_host_kind::{ExtensionHostKind, ExtensionRunningPreference, IExtensionHostKindPicker};
use super::extension_host_manager::{ExtensionHostManager, ExtensionHostManagerServices};
use super::extension_host_managers::IExtensionHostManager;
use super::lazy_create_extension_host_manager::LazyCreateExtensionHostManager;
use super::extension_host_protocol::IExtensionDescriptionDelta;
use super::extension_host_proxy::IResolveAuthorityResult;
use super::extension_manifest_properties_service::IExtensionManifestPropertiesService;
use super::extension_running_location::ExtensionRunningLocation;
use super::extension_running_location_tracker::{ExtensionRunningLocationTracker, filter_extension_identifiers};
use super::extensions::{
    ActivationKind, ActivationTimes, ExtensionHostStartup, ExtensionHostUpdates, ExtensionPointContribution, IExtensionDescription, IExtensionHost, IExtensionInspectInfo,
    IExtensionService, IExtensionsChangeEvent, IExtensionsStatus, IInternalExtensionService, IMessage, IResponsiveStateChangeEvent, IWillActivateEvent, WillStopExtensionHostsEvent,
    extension_contributions, to_extension, to_extension_description,
};
use super::extensions_proposed_api::ExtensionsProposedApi;
use super::extensions_registry::{ExtensionMessageCollectorImpl, ExtensionPoint, ExtensionPointUser, extensions_registry};
use super::implicit_activation_events::implicit_activation_events;
use super::rpc_protocol::ResponsiveState;
use super::workspace_contains::{IExtensionActivationHost, check_activate_workspace_contains_extension, check_glob_file_exists};
use crate::services::environment::environment_service::IWorkbenchEnvironmentService;
use crate::services::extension_management::extension_management::{
    ExtensionRef, IExtension, IWorkbenchExtensionEnablementService, IWorkbenchExtensionManagementService, WorkspaceTypeOverrides, local_extension_uri,
};
use crate::services::lifecycle::lifecycle::{ILifecycleService, WillShutdownEventJoiner, WillShutdownJoinerOrder};
use crate::services::lifecycle::lifecycle_service::handle_vetos;
use crate::services::remote::remote_agent_service::{IExtensionHostExitInfo, IRemoteAgentService};

/// `ExtensionIdentifierMap<ExtensionRunningLocation | null>`: o que o rastreador devolve ao remover extensões.
type RunningLocations = ExtensionIdentifierMap<Option<ExtensionRunningLocation>>;

/// `ResolverExtensions`.
#[derive(Clone)]
pub struct ResolverExtensions {
    pub extensions: Vec<SharedExtensionDescription>,
}

/// `LocalExtensions`.
#[derive(Clone)]
pub struct LocalExtensions {
    pub extensions: Vec<SharedExtensionDescription>,
}

/// `RemoteExtensions`.
#[derive(Clone)]
pub struct RemoteExtensions {
    pub extensions: Vec<SharedExtensionDescription>,
}

/// `ResolvedExtensions = ResolverExtensions | LocalExtensions | RemoteExtensions`.
#[derive(Clone)]
pub enum ResolvedExtensions {
    Resolver(ResolverExtensions),
    Local(LocalExtensions),
    Remote(RemoteExtensions),
}

/// O `AsyncIterable<ResolvedExtensions>` de `_resolveExtensions()`: `next()` entrega o item seguinte, e `None` quando acabou.
pub trait ResolvedExtensionsStream {
    fn next(&self) -> Promise<Option<ResolvedExtensions>>;
}

/// Um [`ResolvedExtensionsStream`] de itens já conhecidos (um gerador que só tem `yield`s imediatos; vazio é o gerador sem
/// `yield`).
pub struct ResolvedExtensionsQueue {
    items: RefCell<VecDeque<ResolvedExtensions>>,
}

impl ResolvedExtensionsQueue {
    pub fn new(items: Vec<ResolvedExtensions>) -> Rc<Self> {
        Rc::new(Self { items: RefCell::new(items.into()) })
    }
}

impl ResolvedExtensionsStream for ResolvedExtensionsQueue {
    fn next(&self) -> Promise<Option<ResolvedExtensions>> {
        Promise::resolved(self.items.borrow_mut().pop_front())
    }
}

/// `IExtensionHostFactory`.
pub trait IExtensionHostFactory {
    fn create_extension_host(&self, running_locations: &ExtensionRunningLocationTracker, running_location: ExtensionRunningLocation, is_initial_start: bool) -> Option<Rc<dyn IExtensionHost>>;
}

/// O que a subclasse do `AbstractExtensionService` define (ver a nota do módulo).
pub trait AbstractExtensionServiceHooks {
    /// `_resolveExtensions()` (abstrato).
    fn resolve_extensions(&self) -> Rc<dyn ResolvedExtensionsStream>;

    /// `_onExtensionHostExit(code)` (abstrato).
    fn on_extension_host_exit(&self, code: i32) -> Promise<()>;

    /// `_resolveAuthority(remoteAuthority)` (abstrato).
    fn resolve_authority(&self, remote_authority: &str) -> Promise<ResolverResult>;

    /// `_doCreateExtensionHostManager(extensionHost, initialActivationEvents)`. O corpo padrão é o do original (ver
    /// [`AbstractExtensionService::do_create_extension_host_manager_base`]); `service` é o `this` da classe de base.
    fn do_create_extension_host_manager(
        &self,
        service: &AbstractExtensionService,
        extension_host: Rc<dyn IExtensionHost>,
        initial_activation_events: Vec<String>,
    ) -> Rc<dyn IExtensionHostManager> {
        service.do_create_extension_host_manager_base(extension_host, initial_activation_events)
    }

    /// `_initialize()`. O corpo padrão é o `super._initialize()`.
    fn initialize(&self, service: &AbstractExtensionService) -> Promise<()> {
        service.initialize_base()
    }

    /// `_onExtensionHostCrashed(extensionHost, code, signal)`. O corpo padrão é o `super._onExtensionHostCrashed(...)`.
    fn on_extension_host_crashed(&self, service: &AbstractExtensionService, extension_host: &Rc<dyn IExtensionHostManager>, code: i32, signal: Option<&str>) {
        service.on_extension_host_crashed_base(extension_host, code, signal);
    }
}

/// Os serviços que o construtor do `AbstractExtensionService` recebe por decoradores e que a classe de base lê.
pub struct AbstractExtensionServiceServices {
    pub instantiation_service: Rc<InstantiationService>,
    pub notification_service: Rc<dyn INotificationService>,
    pub environment_service: Rc<dyn IWorkbenchEnvironmentService>,
    pub telemetry_service: Rc<dyn ITelemetryService>,
    pub extension_enablement_service: Rc<dyn IWorkbenchExtensionEnablementService>,
    pub file_service: Rc<dyn IFileService>,
    pub extension_management_service: Rc<dyn IWorkbenchExtensionManagementService>,
    pub context_service: Rc<dyn IWorkspaceContextService>,
    pub configuration_service: Rc<dyn IConfigurationService>,
    pub extension_manifest_properties_service: Rc<dyn IExtensionManifestPropertiesService>,
    pub log_service: LogServiceRef,
    pub remote_agent_service: Rc<dyn IRemoteAgentService>,
    pub lifecycle_service: Rc<dyn ILifecycleService>,
    pub remote_authority_resolver_service: Rc<dyn IRemoteAuthorityResolverService>,
    pub dialog_service: Rc<dyn IDialogService>,
}

/// As opções do construtor: `{ hasLocalProcess, allowRemoteExtensionsInLocalWebWorker }`.
#[derive(Clone, Copy, Debug)]
pub struct AbstractExtensionServiceOptions {
    pub has_local_process: bool,
    pub allow_remote_extensions_in_local_web_worker: bool,
}

/// `DeltaExtensionsQueueItem.toRemove`: `string[] | IExtension[]`.
#[derive(Clone)]
pub enum DeltaToRemove {
    Ids(Vec<String>),
    Extensions(Vec<ExtensionRef>),
}

/// `DeltaExtensionsQueueItem`.
#[derive(Clone)]
pub struct DeltaExtensionsQueueItem {
    pub to_add: Vec<ExtensionRef>,
    pub to_remove: DeltaToRemove,
}

impl DeltaExtensionsQueueItem {
    pub fn new(to_add: Vec<ExtensionRef>, to_remove: DeltaToRemove) -> Self {
        Self { to_add, to_remove }
    }
}

/// O `ExtensionHostCollection` e o `ExtensionHostManagerData` do original.
struct ExtensionHostManagerData {
    extension_host: Rc<dyn IExtensionHostManager>,
    disposable_store: Rc<DisposableStore>,
}

impl ExtensionHostManagerData {
    fn dispose(&self) {
        self.disposable_store.dispose();
        self.extension_host.dispose();
    }
}

/// `ExtensionHostCollection`.
pub struct ExtensionHostCollection {
    extension_host_managers: RefCell<Vec<Rc<ExtensionHostManagerData>>>,
}

impl ExtensionHostCollection {
    fn new() -> Rc<Self> {
        Rc::new(Self { extension_host_managers: RefCell::new(Vec::new()) })
    }

    pub fn add(&self, extension_host_manager: Rc<dyn IExtensionHostManager>, disposable_store: Rc<DisposableStore>) {
        self.extension_host_managers.borrow_mut().push(Rc::new(ExtensionHostManagerData { extension_host: extension_host_manager, disposable_store }));
    }

    pub fn stop_all_in_reverse(self: &Rc<Self>) -> Promise<()> {
        let this = self.clone();
        async_fn(async move {
            // See https://github.com/microsoft/vscode/issues/152204
            // Dispose extension hosts in reverse creation order because the local extension host
            // might be critical in sustaining a connection to the remote extension host
            let mut index = this.extension_host_managers.borrow().len();
            while index > 0 {
                index -= 1;
                let manager = this.extension_host_managers.borrow().get(index).cloned();
                if let Some(manager) = manager {
                    manager.extension_host.disconnect().await?;
                    manager.dispose();
                }
            }
            this.extension_host_managers.borrow_mut().clear();
            Ok(())
        })
    }

    pub fn stop_one(&self, extension_host_manager: &Rc<dyn IExtensionHostManager>) -> Promise<()> {
        let index = self
            .extension_host_managers
            .borrow()
            .iter()
            .position(|el| std::ptr::addr_eq(Rc::as_ptr(&el.extension_host), Rc::as_ptr(extension_host_manager)));
        let Some(index) = index else {
            return Promise::resolved(());
        };
        self.extension_host_managers.borrow_mut().remove(index);
        let extension_host_manager = extension_host_manager.clone();
        async_fn(async move {
            extension_host_manager.disconnect().await?;
            extension_host_manager.dispose();
            Ok(())
        })
    }

    pub fn get_by_kind(&self, kind: ExtensionHostKind) -> Vec<Rc<dyn IExtensionHostManager>> {
        self.snapshot().into_iter().filter(|el| el.kind() == kind).collect()
    }

    pub fn get_by_running_location(&self, running_location: &ExtensionRunningLocation) -> Option<Rc<dyn IExtensionHostManager>> {
        self.extension_host_managers
            .borrow()
            .iter()
            .find(|el| el.extension_host.represents_running_location(running_location))
            .map(|el| el.extension_host.clone())
    }

    /// `[Symbol.iterator]()` (e, com ele, `map`, `every` e `filter`): os gerenciadores na ordem de criação.
    pub fn snapshot(&self) -> Vec<Rc<dyn IExtensionHostManager>> {
        self.extension_host_managers.borrow().iter().map(|el| el.extension_host.clone()).collect()
    }
}

impl IDisposable for ExtensionHostCollection {
    fn dispose(&self) {
        let managers = std::mem::take(&mut *self.extension_host_managers.borrow_mut());
        for manager in managers.iter().rev() {
            let _ = manager.extension_host.disconnect();
            manager.dispose();
        }
    }
}

/// `ExtensionStatus`.
pub struct ExtensionStatus {
    pub id: ExtensionIdentifier,
    messages: RefCell<Vec<IMessage>>,
    activation_times: RefCell<Option<ActivationTimes>>,
    runtime_errors: RefCell<Vec<Error>>,
    activation_started: Cell<bool>,
}

impl ExtensionStatus {
    pub fn new(id: ExtensionIdentifier) -> Self {
        Self {
            id,
            messages: RefCell::new(Vec::new()),
            activation_times: RefCell::new(None),
            runtime_errors: RefCell::new(Vec::new()),
            activation_started: Cell::new(false),
        }
    }

    pub fn messages(&self) -> Vec<IMessage> {
        self.messages.borrow().clone()
    }

    pub fn activation_times(&self) -> Option<ActivationTimes> {
        self.activation_times.borrow().clone()
    }

    pub fn runtime_errors(&self) -> Vec<Error> {
        self.runtime_errors.borrow().clone()
    }

    pub fn activation_started(&self) -> bool {
        self.activation_started.get()
    }

    pub fn clear_runtime_status(&self) {
        self.activation_started.set(false);
        *self.activation_times.borrow_mut() = None;
        *self.runtime_errors.borrow_mut() = Vec::new();
    }

    pub fn add_message(&self, msg: IMessage) {
        self.messages.borrow_mut().push(msg);
    }

    pub fn set_activation_times(&self, activation_times: ActivationTimes) {
        *self.activation_times.borrow_mut() = Some(activation_times);
    }

    pub fn add_runtime_error(&self, err: Error) {
        self.runtime_errors.borrow_mut().push(err);
    }

    pub fn on_will_activate(&self) {
        self.activation_started.set(true);
    }
}

/// `ExtensionHostCrashTracker`.
#[derive(Default)]
pub struct ExtensionHostCrashTracker {
    /// `IExtensionHostCrashInfo.timestamp` (`Date.now()`, em milissegundos).
    recent_crashes: RefCell<VecDeque<f64>>,
}

impl ExtensionHostCrashTracker {
    const TIME_LIMIT: f64 = 5.0 * 60.0 * 1000.0; // 5 minutes
    const CRASH_LIMIT: usize = 3;

    pub fn new() -> Self {
        Self::default()
    }

    fn now() -> f64 {
        SystemTime::now().duration_since(UNIX_EPOCH).map_or(0.0, |elapsed| elapsed.as_millis() as f64)
    }

    fn remove_old_crashes(&self) {
        let limit = Self::now() - Self::TIME_LIMIT;
        let mut recent_crashes = self.recent_crashes.borrow_mut();
        while recent_crashes.front().is_some_and(|timestamp| *timestamp < limit) {
            recent_crashes.pop_front();
        }
    }

    pub fn register_crash(&self) {
        self.remove_old_crashes();
        self.recent_crashes.borrow_mut().push_back(Self::now());
    }

    pub fn should_automatically_restart(&self) -> bool {
        self.remove_old_crashes();
        self.recent_crashes.borrow().len() < Self::CRASH_LIMIT
    }
}

/// `ImplicitActivationAwareReader`.
///
/// This can run correctly only on the renderer process because that is the only place
/// where all extension points and all implicit activation events generators are known.
pub struct ImplicitActivationAwareReader;

impl IActivationEventsReader for ImplicitActivationAwareReader {
    fn read_activation_events(&self, extension_description: &ExtensionDescription) -> Vec<String> {
        implicit_activation_events().read_activation_events(extension_description)
    }
}

/// `isResolverExtension(extension)`.
pub fn is_resolver_extension(extension: &ExtensionDescription) -> bool {
    extension.manifest.activation_events.as_ref().is_some_and(|activation_events| activation_events.iter().any(|activation_event| activation_event.starts_with("onResolveRemoteAuthority:")))
}

/// `checkEnabledAndProposedAPI(logService, extensionEnablementService, extensionsProposedApi, extensions, ignoreWorkspaceTrust)`.
///
/// `ignore_workspace_trust`: Do not take workspace trust into account.
pub fn check_enabled_and_proposed_api(
    log_service: &LogServiceRef,
    extension_enablement_service: &dyn IWorkbenchExtensionEnablementService,
    extensions_proposed_api: &ExtensionsProposedApi,
    mut extensions: Vec<SharedExtensionDescription>,
    ignore_workspace_trust: bool,
) -> Vec<SharedExtensionDescription> {
    // enable or disable proposed API per extension
    extensions_proposed_api.update_enabled_api_proposals(&mut extensions);

    // keep only enabled extensions
    filter_enabled_extensions(log_service, extension_enablement_service, &extensions, ignore_workspace_trust)
}

/// `filterEnabledExtensions(logService, extensionEnablementService, extensions, ignoreWorkspaceTrust)`: o subconjunto das
/// extensões que estão habilitadas.
pub fn filter_enabled_extensions(
    log_service: &LogServiceRef,
    extension_enablement_service: &dyn IWorkbenchExtensionEnablementService,
    extensions: &[SharedExtensionDescription],
    ignore_workspace_trust: bool,
) -> Vec<SharedExtensionDescription> {
    let is_ci = Platform::detect().is_ci;
    let mut enabled_extensions: Vec<SharedExtensionDescription> = Vec::new();
    let mut extensions_to_check: Vec<SharedExtensionDescription> = Vec::new();
    let mut mapped_extensions: Vec<ExtensionRef> = Vec::new();
    for extension in extensions {
        if extension.is_under_development {
            // Never disable extensions under development
            enabled_extensions.push(extension.clone());
        } else {
            extensions_to_check.push(extension.clone());
            mapped_extensions.push(Rc::new(to_extension(extension)));
        }
    }

    let enablement_states = extension_enablement_service
        .get_enablement_states(&mapped_extensions, if ignore_workspace_trust { WorkspaceTypeOverrides { trusted: Some(true) } } else { WorkspaceTypeOverrides::default() });
    for (index, enablement_state) in enablement_states.iter().enumerate() {
        if extension_enablement_service.is_enabled_enablement_state(*enablement_state) {
            enabled_extensions.push(extensions_to_check[index].clone());
        } else if is_ci {
            log_service.info(&format!("filterEnabledExtensions: extension '{}' is disabled", extensions_to_check[index].identifier.value));
        }
    }

    enabled_extensions
}

/// `extensionIsEnabled(logService, extensionEnablementService, extension, ignoreWorkspaceTrust)`.
///
/// `ignore_workspace_trust`: Do not take workspace trust into account.
pub fn extension_is_enabled(
    log_service: &LogServiceRef,
    extension_enablement_service: &dyn IWorkbenchExtensionEnablementService,
    extension: &SharedExtensionDescription,
    ignore_workspace_trust: bool,
) -> bool {
    filter_enabled_extensions(log_service, extension_enablement_service, std::slice::from_ref(extension), ignore_workspace_trust).iter().any(|enabled| Rc::ptr_eq(enabled, extension))
}

fn includes(extensions: &[SharedExtensionDescription], identifier: &ExtensionIdentifier) -> bool {
    extensions.iter().any(|extension| ExtensionIdentifier::equals(Some(&extension.identifier.value), Some(&identifier.value)))
}

/// O `IExtensionActivationHost` que `_activateAddedExtensionIfNeeded` monta.
struct ActivationHost {
    log_service: LogServiceRef,
    folders: Vec<URI>,
    force_using_search: bool,
    file_service: Rc<dyn IFileService>,
    instantiation_service: Rc<InstantiationService>,
}

impl IExtensionActivationHost for ActivationHost {
    fn log_service(&self) -> LogServiceRef {
        self.log_service.clone()
    }

    fn folders(&self) -> Vec<URI> {
        self.folders.clone()
    }

    fn force_using_search(&self) -> bool {
        self.force_using_search
    }

    fn exists(&self, uri: &URI) -> Promise<bool> {
        self.file_service.exists(uri)
    }

    fn check_exists(&self, folders: &[URI], includes: Vec<String>, token: je_base::cancellation::CancellationToken) -> Promise<bool> {
        self.instantiation_service.invoke_function(|accessor| check_glob_file_exists(accessor, folders, includes, token))
    }
}

/// O objeto que `_acquireInternalAPI(extensionHost)` entrega ao host.
struct InternalExtensionServiceApi {
    service: Weak<AbstractExtensionService>,
    extension_host: Rc<dyn IExtensionHost>,
}

impl InternalExtensionServiceApi {
    fn service(&self) -> Rc<AbstractExtensionService> {
        self.service.upgrade().expect("o AbstractExtensionService foi descartado")
    }
}

impl IInternalExtensionService for InternalExtensionServiceApi {
    fn activate_by_id(&self, extension_id: &ExtensionIdentifier, reason: ExtensionActivationReason) -> Promise<()> {
        self.service().activate_by_id(extension_id, reason)
    }

    fn on_will_activate_extension(&self, extension_id: &ExtensionIdentifier) {
        self.service().on_will_activate_extension(extension_id, self.extension_host.running_location());
    }

    fn on_did_activate_extension(
        &self,
        extension_id: &ExtensionIdentifier,
        code_loading_time: f64,
        activate_call_time: f64,
        activate_resolved_time: f64,
        activation_reason: ExtensionActivationReason,
    ) {
        self.service().on_did_activate_extension(extension_id, code_loading_time, activate_call_time, activate_resolved_time, activation_reason);
    }

    fn on_did_activate_extension_error(&self, extension_id: &ExtensionIdentifier, error: &Error) {
        self.service().on_did_activate_extension_error(extension_id, error);
    }

    fn on_extension_runtime_error(&self, extension_id: &ExtensionIdentifier, err: &Error) {
        self.service().on_extension_runtime_error(extension_id, err);
    }
}

/// `AbstractExtensionService`.
pub struct AbstractExtensionService {
    this: Weak<AbstractExtensionService>,
    base: Rc<Disposable>,

    has_local_process: bool,
    allow_remote_extensions_in_local_web_worker: bool,
    is_ci: bool,

    on_did_register_extensions: Rc<Emitter<()>>,
    on_did_change_extensions_status: Rc<Emitter<Vec<ExtensionIdentifier>>>,
    on_did_change_extensions: Rc<Emitter<IExtensionsChangeEvent>>,
    on_will_activate_by_event: Rc<Emitter<IWillActivateEvent>>,
    on_did_change_responsive_change: Rc<Emitter<IResponsiveStateChangeEvent>>,
    on_will_stop: Rc<Emitter<WillStopExtensionHostsEvent>>,

    activation_event_reader: Rc<ImplicitActivationAwareReader>,
    registry: Rc<LockableExtensionDescriptionRegistry>,
    installed_extensions_ready: Barrier,
    extension_status: RefCell<ExtensionIdentifierMap<Rc<ExtensionStatus>>>,
    all_requested_activate_events: RefCell<IndexSet<String>>,
    pending_remote_activation_events: RefCell<IndexSet<String>>,
    running_locations: ExtensionRunningLocationTracker,
    remote_crash_tracker: ExtensionHostCrashTracker,

    delta_extensions_queue: RefCell<VecDeque<DeltaExtensionsQueueItem>>,
    in_handle_delta_extensions: Cell<bool>,

    extension_host_managers: Rc<ExtensionHostCollection>,

    resolve_authority_attempt: Cell<u32>,
    initialize_promise: RefCell<Option<Promise<()>>>,
    hooks: RefCell<Option<Weak<dyn AbstractExtensionServiceHooks>>>,

    extensions_proposed_api: Rc<ExtensionsProposedApi>,
    extension_host_factory: Rc<dyn IExtensionHostFactory>,
    extension_host_kind_picker: Rc<dyn IExtensionHostKindPicker>,

    instantiation_service: Rc<InstantiationService>,
    notification_service: Rc<dyn INotificationService>,
    environment_service: Rc<dyn IWorkbenchEnvironmentService>,
    telemetry_service: Rc<dyn ITelemetryService>,
    extension_enablement_service: Rc<dyn IWorkbenchExtensionEnablementService>,
    file_service: Rc<dyn IFileService>,
    extension_management_service: Rc<dyn IWorkbenchExtensionManagementService>,
    context_service: Rc<dyn IWorkspaceContextService>,
    log_service: LogServiceRef,
    remote_agent_service: Rc<dyn IRemoteAgentService>,
    lifecycle_service: Rc<dyn ILifecycleService>,
    remote_authority_resolver_service: Rc<dyn IRemoteAuthorityResolverService>,
    dialog_service: Rc<dyn IDialogService>,
}

impl AbstractExtensionService {
    pub fn new(
        options: AbstractExtensionServiceOptions,
        extensions_proposed_api: Rc<ExtensionsProposedApi>,
        extension_host_factory: Rc<dyn IExtensionHostFactory>,
        extension_host_kind_picker: Rc<dyn IExtensionHostKindPicker>,
        services: AbstractExtensionServiceServices,
    ) -> Rc<Self> {
        let base = Disposable::new();
        let on_did_register_extensions = Emitter::new();
        let on_did_change_extensions_status = Emitter::new();
        let on_did_change_extensions = Emitter::new();
        let on_will_activate_by_event = Emitter::new();
        let on_did_change_responsive_change = Emitter::new();
        let on_will_stop = Emitter::new();
        base.register(on_did_register_extensions.clone());
        base.register(on_did_change_extensions_status.clone());
        base.register(on_did_change_extensions.clone());
        base.register(on_will_activate_by_event.clone());
        base.register(on_did_change_responsive_change.clone());
        base.register(on_will_stop.clone());

        let extension_host_managers = ExtensionHostCollection::new();
        base.register(extension_host_managers.clone());

        let activation_event_reader = Rc::new(ImplicitActivationAwareReader);
        let registry = LockableExtensionDescriptionRegistry::new(activation_event_reader.clone());

        let running_locations = ExtensionRunningLocationTracker::new(
            registry.clone(),
            extension_host_kind_picker.clone(),
            services.environment_service.clone(),
            services.configuration_service.clone(),
            services.log_service.clone(),
            services.extension_manifest_properties_service.clone(),
        );

        let this = Rc::new_cyclic(|weak| Self {
            this: weak.clone(),
            base,
            has_local_process: options.has_local_process,
            allow_remote_extensions_in_local_web_worker: options.allow_remote_extensions_in_local_web_worker,
            is_ci: Platform::detect().is_ci,
            on_did_register_extensions,
            on_did_change_extensions_status,
            on_did_change_extensions,
            on_will_activate_by_event,
            on_did_change_responsive_change,
            on_will_stop,
            activation_event_reader,
            registry,
            installed_extensions_ready: Barrier::new(),
            extension_status: RefCell::new(ExtensionIdentifierMap::new()),
            all_requested_activate_events: RefCell::new(IndexSet::new()),
            pending_remote_activation_events: RefCell::new(IndexSet::new()),
            running_locations,
            remote_crash_tracker: ExtensionHostCrashTracker::new(),
            delta_extensions_queue: RefCell::new(VecDeque::new()),
            in_handle_delta_extensions: Cell::new(false),
            extension_host_managers,
            resolve_authority_attempt: Cell::new(0),
            initialize_promise: RefCell::new(None),
            hooks: RefCell::new(None),
            extensions_proposed_api,
            extension_host_factory,
            extension_host_kind_picker,
            instantiation_service: services.instantiation_service,
            notification_service: services.notification_service,
            environment_service: services.environment_service,
            telemetry_service: services.telemetry_service,
            extension_enablement_service: services.extension_enablement_service,
            file_service: services.file_service,
            extension_management_service: services.extension_management_service,
            context_service: services.context_service,
            log_service: services.log_service,
            remote_agent_service: services.remote_agent_service,
            lifecycle_service: services.lifecycle_service,
            remote_authority_resolver_service: services.remote_authority_resolver_service,
            dialog_service: services.dialog_service,
        });
        this.register_listeners();
        this
    }

    /// Registra a subclasse (os métodos abstratos e sobrescrevíveis). Deve ser chamado antes de qualquer outro uso.
    pub fn set_hooks(&self, hooks: Weak<dyn AbstractExtensionServiceHooks>) {
        *self.hooks.borrow_mut() = Some(hooks);
    }

    fn hooks(&self) -> Rc<dyn AbstractExtensionServiceHooks> {
        self.hooks.borrow().as_ref().and_then(Weak::upgrade).expect("AbstractExtensionService.set_hooks não foi chamado (ou a subclasse foi descartada)")
    }

    /// O `Rc` deste serviço, para as continuações assíncronas.
    pub fn me(&self) -> Rc<Self> {
        self.this.upgrade().expect("o AbstractExtensionService foi descartado")
    }

    fn register_listeners(&self) {
        let weak = self.this.clone();

        // help the file service to activate providers by activating extensions by file system event
        let service = weak.clone();
        self.base.register(self.file_service.on_will_activate_file_system_provider().listen(move |e| {
            if e.scheme != schemas::VSCODE_REMOTE
                && let Some(this) = service.upgrade()
            {
                e.join(this.activate_by_event(&format!("onFileSystem:{}", e.scheme), None));
            }
        }));

        let service = weak.clone();
        self.base.register(self.extension_enablement_service.on_enablement_changed().listen(move |extensions: &Vec<ExtensionRef>| {
            let Some(this) = service.upgrade() else { return };
            let mut to_add: Vec<ExtensionRef> = Vec::new();
            let mut to_remove: Vec<ExtensionRef> = Vec::new();
            for extension in extensions {
                if this.safe_invoke_is_enabled(extension) {
                    // an extension has been enabled
                    to_add.push(extension.clone());
                } else {
                    // an extension has been disabled
                    to_remove.push(extension.clone());
                }
            }
            if this.is_ci {
                this.log_service.info(&format!(
                    "AbstractExtensionService.onEnablementChanged fired for {}",
                    extensions.iter().map(|e| e.identifier.id.as_str()).collect::<Vec<_>>().join(", ")
                ));
            }
            let _ = this.handle_delta_extensions(DeltaExtensionsQueueItem::new(to_add, DeltaToRemove::Extensions(to_remove)));
        }));

        let service = weak.clone();
        self.base.register(self.extension_management_service.on_did_change_profile().listen(move |event| {
            let Some(this) = service.upgrade() else { return };
            if !event.added.is_empty() || !event.removed.is_empty() {
                if this.is_ci {
                    this.log_service.info("AbstractExtensionService.onDidChangeProfile fired");
                }
                let _ = this.handle_delta_extensions(DeltaExtensionsQueueItem::new(event.added.clone(), DeltaToRemove::Extensions(event.removed.clone())));
            }
        }));

        let service = weak.clone();
        self.base.register(self.extension_management_service.on_did_enable_extensions().listen(move |extensions: &Vec<ExtensionRef>| {
            let Some(this) = service.upgrade() else { return };
            if !extensions.is_empty() {
                if this.is_ci {
                    this.log_service.info("AbstractExtensionService.onDidEnableExtensions fired");
                }
                let _ = this.handle_delta_extensions(DeltaExtensionsQueueItem::new(extensions.clone(), DeltaToRemove::Extensions(Vec::new())));
            }
        }));

        let service = weak.clone();
        self.base.register(self.extension_management_service.on_did_install_extensions().listen(move |result| {
            let Some(this) = service.upgrade() else { return };
            let mut extensions: Vec<ExtensionRef> = Vec::new();
            let mut to_remove: Vec<String> = Vec::new();
            for item in result {
                if let Some(local) = &item.local
                    && local.is_valid
                    && item.operation != InstallOperation::Migrate
                    && this.safe_invoke_is_enabled(local)
                {
                    extensions.push(local.clone());
                    if item.operation == InstallOperation::Update {
                        to_remove.push(local.identifier.id.clone());
                    }
                }
            }
            if !extensions.is_empty() {
                if this.is_ci {
                    this.log_service.info(&format!(
                        "AbstractExtensionService.onDidInstallExtensions fired for {}",
                        extensions.iter().map(|e| e.identifier.id.as_str()).collect::<Vec<_>>().join(", ")
                    ));
                }
                let _ = this.handle_delta_extensions(DeltaExtensionsQueueItem::new(extensions, DeltaToRemove::Ids(to_remove)));
            }
        }));

        let service = weak.clone();
        self.base.register(self.extension_management_service.on_did_uninstall_extension().listen(move |event| {
            let Some(this) = service.upgrade() else { return };
            if event.error.is_none() {
                // an extension has been uninstalled
                if this.is_ci {
                    this.log_service.info(&format!("AbstractExtensionService.onDidUninstallExtension fired for {}", event.identifier.id));
                }
                let _ = this.handle_delta_extensions(DeltaExtensionsQueueItem::new(Vec::new(), DeltaToRemove::Ids(vec![event.identifier.id.clone()])));
            }
        }));

        let service = weak;
        self.base.register(self.lifecycle_service.on_will_shutdown().listen(move |event| {
            let Some(this) = service.upgrade() else { return };
            if this.remote_agent_service.get_connection().is_some() {
                let service = this.clone();
                event.join_last(
                    Rc::new(move || {
                        let this = service.clone();
                        async_fn(async move {
                            // We need to disconnect the management connection before killing the local extension host.
                            // Otherwise, the local extension host might terminate the underlying tunnel before the
                            // management connection has a chance to send its disconnection message.
                            let result: Result<(), Error> = async {
                                this.remote_agent_service.end_connection().await?;
                                this.do_stop_extension_hosts().await?;
                                if let Some(connection) = this.remote_agent_service.get_connection() {
                                    connection.dispose();
                                }
                                Ok(())
                            }
                            .await;
                            if result.is_err() {
                                this.log_service.warn("Error while disconnecting remote agent");
                            }
                            Ok(())
                        })
                    }),
                    WillShutdownEventJoiner {
                        id: "join.disconnectRemote".to_owned(),
                        label: localize("disconnectRemote", "Disconnect Remote Agent", &[]),
                        order: Some(WillShutdownJoinerOrder::Last), // after others have joined that might depend on a remote connection
                    },
                );
            } else {
                event.join(
                    this.do_stop_extension_hosts(),
                    WillShutdownEventJoiner { id: "join.stopExtensionHosts".to_owned(), label: localize("stopExtensionHosts", "Stopping Extension Hosts", &[]), order: None },
                );
            }
        }));
    }

    /// `_getExtensionHostManagers(kind)`.
    pub fn get_extension_host_managers(&self, kind: ExtensionHostKind) -> Vec<Rc<dyn IExtensionHostManager>> {
        self.extension_host_managers.get_by_kind(kind)
    }

    /// `onWillActivateByEvent`: an event that is fired when activation happens.
    pub fn on_will_activate_by_event(&self) -> Event<IWillActivateEvent> {
        self.on_will_activate_by_event.event()
    }

    /// `onDidChangeResponsiveChange`: an event that is fired when an extension host changes its responsive-state.
    pub fn on_did_change_responsive_change(&self) -> Event<IResponsiveStateChangeEvent> {
        self.on_did_change_responsive_change.event()
    }

    //#region deltaExtensions

    /// `_handleDeltaExtensions(item)`.
    fn handle_delta_extensions(&self, item: DeltaExtensionsQueueItem) -> Promise<()> {
        let this = self.me();
        async_fn(async move {
            this.delta_extensions_queue.borrow_mut().push_back(item);
            if this.in_handle_delta_extensions.get() {
                // Let the current item finish, the new one will be picked up
                return Ok(());
            }

            let lock: RefCell<Option<Rc<ExtensionDescriptionRegistryLock>>> = RefCell::new(None);
            this.in_handle_delta_extensions.set(true);
            let result = Self::drain_delta_extensions_queue(&this, &lock).await;
            this.in_handle_delta_extensions.set(false);
            if let Some(lock) = lock.borrow_mut().take() {
                lock.dispose();
            }
            result
        })
    }

    async fn drain_delta_extensions_queue(this: &Rc<Self>, lock_slot: &RefCell<Option<Rc<ExtensionDescriptionRegistryLock>>>) -> Result<(), Error> {
        // wait for _initialize to finish before hanlding any delta extension events
        this.installed_extensions_ready.wait().await?;

        let lock = this.registry.acquire_lock("handleDeltaExtensions").await?;
        *lock_slot.borrow_mut() = Some(lock.clone());
        loop {
            let item = this.delta_extensions_queue.borrow_mut().pop_front();
            let Some(item) = item else { break };
            Self::delta_extensions(this, &lock, item.to_add, item.to_remove).await?;
        }
        Ok(())
    }

    async fn delta_extensions(this: &Rc<Self>, lock: &ExtensionDescriptionRegistryLock, to_add_input: Vec<ExtensionRef>, to_remove_input: DeltaToRemove) -> Result<(), Error> {
        if this.is_ci {
            let removed: Vec<String> = match &to_remove_input {
                DeltaToRemove::Ids(ids) => ids.clone(),
                DeltaToRemove::Extensions(extensions) => extensions.iter().map(|e| e.identifier.id.clone()).collect(),
            };
            this.log_service.info(&format!(
                "AbstractExtensionService._deltaExtensions: toAdd: [{}] toRemove: [{}]",
                to_add_input.iter().map(|e| e.identifier.id.as_str()).collect::<Vec<_>>().join(","),
                removed.join(",")
            ));
        }

        // `string[] | IExtension[]`: cada item é o identificador e, se houver, a extensão.
        let to_remove_items: Vec<(String, Option<ExtensionRef>)> = match to_remove_input {
            DeltaToRemove::Ids(ids) => ids.into_iter().map(|id| (id, None)).collect(),
            DeltaToRemove::Extensions(extensions) => extensions.into_iter().map(|extension| (extension.identifier.id.clone(), Some(extension))).collect(),
        };

        let mut to_remove: Vec<SharedExtensionDescription> = Vec::new();
        for (extension_id, extension) in &to_remove_items {
            let Some(extension_description) = this.registry.get_extension_description(extension_id) else {
                // ignore disabling/uninstalling an extension which is not running
                continue;
            };

            if let Some(extension) = extension
                && URI::parse(extension_description.extension_location.as_str(), false).scheme().to_string_lossy() != local_extension_uri(extension).scheme().to_string_lossy()
            {
                // this event is for a different extension than mine (maybe for the local extension, while I have the remote extension)
                continue;
            }

            if !this.can_remove_extension(&extension_description) {
                // uses non-dynamic extension point or is activated
                continue;
            }

            to_remove.push(extension_description);
        }

        let mut to_add: Vec<SharedExtensionDescription> = Vec::new();
        for extension in &to_add_input {
            let extension_description: SharedExtensionDescription = Rc::new(to_extension_description(extension, false));

            if !this.can_add_extension_description(&extension_description, &to_remove) {
                continue;
            }

            to_add.push(extension_description);
        }

        if to_add.is_empty() && to_remove.is_empty() {
            return Ok(());
        }

        // enable or disable proposed API per extension
        this.extensions_proposed_api.update_enabled_api_proposals(&mut to_add);

        // Update the local registry
        let to_remove_identifiers: Vec<ExtensionIdentifier> = to_remove.iter().map(|e| e.identifier.clone()).collect();
        let result = this.registry.delta_extensions(lock, to_add.clone(), &to_remove_identifiers)?;
        this.on_did_change_extensions.fire(IExtensionsChangeEvent {
            added: to_add.iter().map(|e| (**e).clone()).collect(),
            removed: to_remove.iter().map(|e| (**e).clone()).collect(),
        });

        to_remove.extend(result.removed_due_to_looping.iter().cloned());
        if !result.removed_due_to_looping.is_empty() {
            this.notify_dependency_loop(&result.removed_due_to_looping);
        }

        // Update extension points
        let mut affected: Vec<SharedExtensionDescription> = to_add.clone();
        affected.extend(to_remove.iter().cloned());
        this.do_handle_extension_points(&affected, false);

        // Update the extension host
        let to_remove_identifiers: Vec<ExtensionIdentifier> = to_remove.iter().map(|e| e.identifier.clone()).collect();
        this.update_extensions_on_ext_hosts(result.version_id, &to_add, &to_remove_identifiers).await?;

        for extension in &to_add {
            let _ = this.activate_added_extension_if_needed(extension);
        }
        Ok(())
    }

    fn update_extensions_on_ext_hosts(&self, version_id: i64, to_add: &[SharedExtensionDescription], to_remove: &[ExtensionIdentifier]) -> Promise<()> {
        let removed_running_location = self.running_locations.delta_extensions(to_add, to_remove);
        let promises: Vec<Promise<()>> = self
            .extension_host_managers
            .snapshot()
            .iter()
            .map(|ext_host_manager| self.update_extensions_on_ext_host(ext_host_manager, version_id, to_add, to_remove, &removed_running_location))
            .collect();
        Promise::all(promises).then(|_| ())
    }

    fn update_extensions_on_ext_host(
        &self,
        extension_host_manager: &Rc<dyn IExtensionHostManager>,
        version_id: i64,
        to_add: &[SharedExtensionDescription],
        to_remove: &[ExtensionIdentifier],
        removed_running_location: &RunningLocations,
    ) -> Promise<()> {
        let my_to_add = self.running_locations.filter_by_extension_host_manager(to_add, &**extension_host_manager);
        let my_to_remove = filter_extension_identifiers(to_remove, removed_running_location, |ext_running_location| extension_host_manager.represents_running_location(ext_running_location));
        let add_activation_events = implicit_activation_events().create_activation_events_map(to_add);
        if self.is_ci {
            let print_ext_ids = |extensions: &[SharedExtensionDescription]| extensions.iter().map(|e| e.identifier.value.as_str()).collect::<Vec<_>>().join(",");
            let print_ids = |extensions: &[ExtensionIdentifier]| extensions.iter().map(|e| e.value.as_str()).collect::<Vec<_>>().join(",");
            self.log_service.info(&format!(
                "AbstractExtensionService: Calling deltaExtensions: toRemove: [{}], toAdd: [{}], myToRemove: [{}], myToAdd: [{}],",
                print_ids(to_remove),
                print_ext_ids(to_add),
                print_ids(&my_to_remove),
                print_ext_ids(&my_to_add)
            ));
        }
        extension_host_manager.delta_extensions(IExtensionDescriptionDelta {
            version_id,
            to_remove: to_remove.to_vec(),
            to_add: to_add.to_vec(),
            add_activation_events,
            my_to_remove,
            my_to_add: my_to_add.iter().map(|extension| extension.identifier.clone()).collect(),
        })
    }

    fn can_add_extension_description(&self, extension: &SharedExtensionDescription, extensions_being_removed: &[SharedExtensionDescription]) -> bool {
        // (Also check for renamed extensions)
        let existing = self.registry.get_extension_description_by_id_or_uuid(&extension.identifier.value, Some(extension.id.as_str()));
        if existing.is_some() {
            // This extension is already known (most likely at a different version)
            // so it cannot be added again unless it is removed first
            let is_being_removed = extensions_being_removed.iter().any(|extension_description| ExtensionIdentifier::equals(Some(&extension.identifier.value), Some(&extension_description.identifier.value)));
            if !is_being_removed {
                return false;
            }
        }

        let extension_kinds = self.running_locations.read_extension_kinds(extension);
        let is_remote = URI::parse(extension.extension_location.as_str(), false).scheme().to_string_lossy() == schemas::VSCODE_REMOTE;
        let extension_host_kind =
            self.extension_host_kind_picker.pick_extension_host_kind(&extension.identifier.value, &extension_kinds, !is_remote, is_remote, ExtensionRunningPreference::None);
        if extension_host_kind.is_none() {
            return false;
        }

        true
    }

    fn activate_added_extension_if_needed(&self, extension_description: &SharedExtensionDescription) -> Promise<()> {
        let this = self.me();
        let extension_description = extension_description.clone();
        async_fn(async move {
            let mut should_activate_reason: Option<String> = None;
            let mut has_workspace_contains = false;
            let activation_events = this.activation_event_reader.read_activation_events(&extension_description);
            for activation_event in &activation_events {
                if this.all_requested_activate_events.borrow().contains(activation_event) {
                    // This activation event was fired before the extension was added
                    should_activate_reason = Some(activation_event.clone());
                    break;
                }

                if activation_event == "*" {
                    should_activate_reason = Some(activation_event.clone());
                    break;
                }

                if activation_event.starts_with("workspaceContains") {
                    has_workspace_contains = true;
                }

                if activation_event == "onStartupFinished" {
                    should_activate_reason = Some(activation_event.clone());
                    break;
                }
            }

            if should_activate_reason.is_none() && has_workspace_contains {
                let workspace = this.context_service.get_complete_workspace().await?;
                let force_using_search = this.environment_service.remote_authority().is_some_and(|authority| !authority.is_empty());
                let host = Rc::new(ActivationHost {
                    log_service: this.log_service.clone(),
                    folders: workspace.folders().iter().map(|folder| folder.uri.clone()).collect(),
                    force_using_search,
                    file_service: this.file_service.clone(),
                    instantiation_service: this.instantiation_service.clone(),
                });

                let result = check_activate_workspace_contains_extension(host, &extension_description).await?;
                if let Some(result) = result {
                    should_activate_reason = Some(result.activation_event);
                }
            }

            if let Some(should_activate_reason) = should_activate_reason {
                let promises: Vec<Promise<bool>> = this
                    .extension_host_managers
                    .snapshot()
                    .iter()
                    .map(|ext_host_manager| {
                        ext_host_manager.activate(
                            &extension_description.identifier,
                            ExtensionActivationReason {
                                startup: false,
                                extension_id: extension_description.identifier.clone(),
                                activation_event: should_activate_reason.clone(),
                            },
                        )
                    })
                    .collect();
                Promise::all(promises).await?;
            }
            Ok(())
        })
    }

    //#endregion

    /// `_initializeIfNeeded()`.
    pub fn initialize_if_needed(&self) -> Promise<()> {
        if let Some(promise) = self.initialize_promise.borrow().as_ref() {
            return promise.clone();
        }
        let promise = self.hooks().initialize(self);
        *self.initialize_promise.borrow_mut() = Some(promise.clone());
        promise
    }

    /// `_initialize()`.
    pub fn initialize_base(&self) -> Promise<()> {
        let this = self.me();
        // No original `this` é a própria subclasse, que a continuação mantém viva; aqui a continuação segura a subclasse (os hooks).
        let hooks = self.hooks();
        async_fn(async move {
            mark("code/willLoadExtensions", None);
            this.start_extension_hosts_if_necessary(true, Vec::new());

            let lock = this.registry.acquire_lock("_initialize").await?;
            let result: Result<(), Error> = async {
                Self::resolve_and_process_extensions(&this, &hooks, &lock).await?;
                // Start extension hosts which are not automatically started
                this.start_on_demand_extension_hosts();
                Ok(())
            }
            .await;
            lock.dispose();
            result?;

            this.release_barrier();
            mark("code/didLoadExtensions", None);

            // Activate deferred remote events now that remote hosts are starting
            // This is done after the barrier is released to avoid blocking initialization
            let _ = this.activate_deferred_remote_events();

            this.handle_extension_tests(hooks).await
        })
    }

    fn activate_deferred_remote_events(&self) -> Promise<()> {
        let this = self.me();
        async_fn(async move {
            if this.pending_remote_activation_events.borrow().is_empty() {
                return Ok(());
            }

            let remote_extension_hosts = this.get_extension_host_managers(ExtensionHostKind::Remote);
            if remote_extension_hosts.is_empty() {
                this.pending_remote_activation_events.borrow_mut().clear();
                return Ok(());
            }

            // Wait for remote extension hosts to be ready
            Promise::all(remote_extension_hosts.iter().map(|ext_host| ext_host.ready()).collect()).await?;

            // Replay deferred activation events on remote hosts
            let pending: Vec<String> = this.pending_remote_activation_events.borrow().iter().cloned().collect();
            for activation_event in pending {
                let result = Promise::all(remote_extension_hosts.iter().map(|ext_host_manager| ext_host_manager.activate_by_event(&activation_event, ActivationKind::Normal)).collect())
                    .then(|_| ());
                this.on_will_activate_by_event.fire(IWillActivateEvent { event: activation_event, activation: result, activation_kind: ActivationKind::Normal });
            }

            this.pending_remote_activation_events.borrow_mut().clear();
            Ok(())
        })
    }

    async fn resolve_and_process_extensions(this: &Rc<Self>, hooks: &Rc<dyn AbstractExtensionServiceHooks>, lock: &ExtensionDescriptionRegistryLock) -> Result<(), Error> {
        let mut resolver_extensions: Vec<SharedExtensionDescription> = Vec::new();
        let mut local_extensions: Vec<SharedExtensionDescription> = Vec::new();
        let mut remote_extensions: Vec<SharedExtensionDescription> = Vec::new();

        let stream = hooks.resolve_extensions();
        while let Some(extensions) = stream.next().await? {
            match extensions {
                ResolvedExtensions::Resolver(extensions) => {
                    resolver_extensions = this.check_enabled_and_proposed_api(extensions.extensions);
                    this.registry.delta_extensions(lock, resolver_extensions.clone(), &[])?;
                    this.do_handle_extension_points(&resolver_extensions, true);
                }
                ResolvedExtensions::Local(extensions) => {
                    local_extensions = this.check_enabled_and_proposed_api(extensions.extensions);
                }
                ResolvedExtensions::Remote(extensions) => {
                    remote_extensions = this.check_enabled_and_proposed_api(extensions.extensions);
                }
            }
        }

        // `initializeRunningLocation` will look at the complete picture (e.g. an extension installed on both sides),
        // takes care of duplicates and picks a running location for each extension
        this.running_locations.initialize_running_location(&local_extensions, &remote_extensions);

        this.start_extension_hosts_if_necessary(true, Vec::new());

        // Some remote extensions could run locally in the web worker, so store them
        let remote_extensions_that_need_to_run_locally = if this.allow_remote_extensions_in_local_web_worker {
            this.running_locations.filter_by_extension_host_kind(&remote_extensions, ExtensionHostKind::LocalWebWorker)
        } else {
            Vec::new()
        };
        let local_process_extensions = if this.has_local_process {
            this.running_locations.filter_by_extension_host_kind(&local_extensions, ExtensionHostKind::LocalProcess)
        } else {
            Vec::new()
        };
        let mut local_web_worker_extensions = this.running_locations.filter_by_extension_host_kind(&local_extensions, ExtensionHostKind::LocalWebWorker);
        let remote_extensions = this.running_locations.filter_by_extension_host_kind(&remote_extensions, ExtensionHostKind::Remote);

        // Add locally the remote extensions that need to run locally in the web worker
        for ext in &remote_extensions_that_need_to_run_locally {
            if !includes(&local_web_worker_extensions, &ext.identifier) {
                local_web_worker_extensions.push(ext.clone());
            }
        }

        let mut all_extensions = remote_extensions;
        all_extensions.extend(local_process_extensions);
        all_extensions.extend(local_web_worker_extensions);
        let mut to_add = all_extensions.clone();

        let same_extension = |a: &SharedExtensionDescription, b: &SharedExtensionDescription| {
            ExtensionIdentifier::equals(Some(&a.identifier.value), Some(&b.identifier.value)) && a.extension_location == b.extension_location
        };
        if !resolver_extensions.is_empty() {
            // Add extensions that are not registered as resolvers but are in the final resolved set
            to_add = all_extensions.iter().filter(|extension| !resolver_extensions.iter().any(|e| same_extension(e, *extension))).cloned().collect();
            // Remove extensions that are registered as resolvers but are not in the final resolved set
            if all_extensions.len() < to_add.len() + resolver_extensions.len() {
                let to_remove: Vec<SharedExtensionDescription> =
                    resolver_extensions.iter().filter(|registered| !all_extensions.iter().any(|e| same_extension(e, *registered))).cloned().collect();
                if !to_remove.is_empty() {
                    let identifiers: Vec<ExtensionIdentifier> = to_remove.iter().map(|e| e.identifier.clone()).collect();
                    this.registry.delta_extensions(lock, Vec::new(), &identifiers)?;
                    this.do_handle_extension_points(&to_remove, true);
                }
            }
        }

        let result = this.registry.delta_extensions(lock, to_add, &[])?;
        if !result.removed_due_to_looping.is_empty() {
            this.notify_dependency_loop(&result.removed_due_to_looping);
        }

        this.do_handle_extension_points(&this.registry.get_all_extension_descriptions(), false);
        Ok(())
    }

    /// `checkEnabledAndProposedAPI(this._logService, this._extensionEnablementService, this._extensionsProposedApi, extensions, false)`.
    fn check_enabled_and_proposed_api(&self, extensions: Vec<SharedExtensionDescription>) -> Vec<SharedExtensionDescription> {
        check_enabled_and_proposed_api(&self.log_service, &*self.extension_enablement_service, &self.extensions_proposed_api, extensions, false)
    }

    /// A notificação de laço de dependência (`looping`), que o original escreve em dois lugares.
    fn notify_dependency_loop(&self, removed_due_to_looping: &[SharedExtensionDescription]) {
        let names = removed_due_to_looping.iter().map(|e| format!("'{}'", e.identifier.value)).collect::<Vec<_>>().join(", ");
        let mut notification = Notification::new(
            Severity::Error,
            localize("looping", "The following extensions contain dependency loops and have been disabled: {0}", &[names.as_str()]),
        );
        notification.properties.telemetry = Some(NotificationTelemetry::Id(NotificationTelemetryId::ExtensionDependencyLoop));
        self.notification_service.notify(notification);
    }

    fn handle_extension_tests(&self, hooks: Rc<dyn AbstractExtensionServiceHooks>) -> Promise<()> {
        let this = self.me();
        async_fn(async move {
            if !this.environment_service.is_extension_development() {
                return Ok(());
            }
            let Some(extension_tests_location_uri) = this.environment_service.extension_tests_location_uri() else {
                return Ok(());
            };

            let Some(extension_host_manager) = this.find_test_extension_host(&extension_tests_location_uri) else {
                let msg = localize(
                    "extensionTestError",
                    "No extension host found that can launch the test runner at {0}.",
                    &[extension_tests_location_uri.to_string(false).to_string_lossy().as_str()],
                );
                eprintln!("{msg}");
                this.notification_service.error(vec![NotificationMessage::Text(msg)]);
                return Ok(());
            };

            let exit_code: i32 = match extension_host_manager.extension_tests_execute().await {
                Ok(exit_code) => {
                    if this.is_ci {
                        this.log_service.info(&format!("Extension host test runner exit code: {exit_code}"));
                    }
                    exit_code
                }
                Err(err) => {
                    if this.is_ci {
                        this.log_service.error_args(&LogMessage::from("Extension host test runner error"), &[err.clone().into()]);
                    }
                    eprintln!("{}", to_error_message_of(&err, false));
                    1 /* ERROR */
                }
            };

            let _ = hooks.on_extension_host_exit(exit_code);
            Ok(())
        })
    }

    fn find_test_extension_host(&self, test_location: &URI) -> Option<Rc<dyn IExtensionHostManager>> {
        let mut running_location: Option<ExtensionRunningLocation> = None;

        for extension in self.registry.get_all_extension_descriptions() {
            if ext_uri().is_equal_or_parent(test_location, &URI::parse(extension.extension_location.as_str(), false), false, false) {
                running_location = self.running_locations.get_running_location(&extension.identifier.value);
                break;
            }
        }
        let running_location = match running_location {
            Some(running_location) => running_location,
            None => {
                // not sure if we should support that, but it was possible to have an test outside an extension

                if test_location.scheme().to_string_lossy() == schemas::VSCODE_REMOTE {
                    ExtensionRunningLocation::Remote
                } else {
                    // When a debugger attaches to the extension host, it will surface all console.log messages from the extension host,
                    // but not necessarily from the window. So it would be best if any errors get printed to the console of the extension host.
                    // That is why here we use the local process extension host even for non-file URIs
                    ExtensionRunningLocation::LocalProcess { affinity: 0 }
                }
            }
        };
        self.extension_host_managers.get_by_running_location(&running_location)
    }

    fn release_barrier(&self) {
        self.installed_extensions_ready.open();
        self.on_did_register_extensions.fire(());
        self.on_did_change_extensions_status.fire(self.registry.get_all_extension_descriptions().iter().map(|e| e.identifier.clone()).collect());
    }

    //#region remote authority resolving

    /// `_resolveAuthorityInitial(remoteAuthority)` (ver a nota do módulo sobre o `catch` do original).
    pub fn resolve_authority_initial(&self, remote_authority: &str) -> Promise<ResolverResult> {
        self.resolve_authority_with_logging(remote_authority)
    }

    /// `_resolveAuthorityAgain()`.
    pub fn resolve_authority_again(&self) -> Promise<()> {
        let this = self.me();
        async_fn(async move {
            let Some(remote_authority) = this.environment_service.remote_authority().filter(|authority| !authority.is_empty()) else {
                return Ok(());
            };

            this.remote_authority_resolver_service.clear_resolved_authority(&remote_authority);
            match this.resolve_authority_with_logging(&remote_authority).await {
                Ok(result) => this.remote_authority_resolver_service.set_resolved_authority(result.authority, result.options),
                Err(err) => this.remote_authority_resolver_service.set_resolved_authority_error(&remote_authority, err),
            }
            Ok(())
        })
    }

    fn resolve_authority_with_logging(&self, remote_authority: &str) -> Promise<ResolverResult> {
        let this = self.me();
        let remote_authority = remote_authority.to_owned();
        async_fn(async move {
            let authority_prefix = get_remote_authority_prefix(&remote_authority).to_owned();
            let sw = StopWatch::create(Some(false));
            this.log_service.info(&format!("Invoking resolveAuthority({authority_prefix})..."));
            mark(&format!("code/willResolveAuthority/{authority_prefix}"), None);
            match this.hooks().resolve_authority(&remote_authority).await {
                Ok(result) => {
                    mark(&format!("code/didResolveAuthorityOK/{authority_prefix}"), None);
                    this.log_service.info(&format!("resolveAuthority({authority_prefix}) returned '{}' after {} ms", result.authority.connect_to, sw.elapsed()));
                    Ok(result)
                }
                Err(err) => {
                    mark(&format!("code/didResolveAuthorityError/{authority_prefix}"), None);
                    this.log_service.error_args(&LogMessage::from(format!("resolveAuthority({authority_prefix}) returned an error after {} ms", sw.elapsed())), &[err.clone().into()]);
                    Err(err)
                }
            }
        })
    }

    /// `_resolveAuthorityOnExtensionHosts(kind, remoteAuthority)`.
    pub fn resolve_authority_on_extension_hosts(&self, kind: ExtensionHostKind, remote_authority: &str) -> Promise<ResolverResult> {
        let extension_hosts = self.get_extension_host_managers(kind);
        if extension_hosts.is_empty() {
            // no local process extension hosts
            return Promise::rejected(Error::new("Cannot resolve authority"));
        }

        self.resolve_authority_attempt.set(self.resolve_authority_attempt.get() + 1);
        let attempt = self.resolve_authority_attempt.get();
        let promises: Vec<Promise<IResolveAuthorityResult>> = extension_hosts.iter().map(|ext_host| ext_host.resolve_authority(remote_authority, attempt)).collect();
        async_fn(async move {
            let results = Promise::all(promises).await?;

            let mut best_error_result = None;
            for result in results {
                match result {
                    IResolveAuthorityResult::Ok(value) => return Ok(value),
                    IResolveAuthorityResult::Error(error) => {
                        let Some(best) = &best_error_result else {
                            best_error_result = Some(error);
                            continue;
                        };
                        let best_error_is_unknown = best.code == RemoteAuthorityResolverErrorCode::Unknown;
                        let error_is_unknown = error.code == RemoteAuthorityResolverErrorCode::Unknown;
                        if best_error_is_unknown && !error_is_unknown {
                            best_error_result = Some(error);
                        }
                    }
                }
            }

            // we can only reach this if there is an error
            let best_error_result = best_error_result.expect("sem resultado ok, há ao menos um erro");
            Err(RemoteAuthorityResolverError::new(
                best_error_result.message.as_deref(),
                best_error_result.code,
                matches!(best_error_result.detail, Some(Dynamic::Bool(true))),
            ))
        })
    }

    //#endregion

    //#region Stopping / Starting / Restarting

    /// `_doStopExtensionHosts()`.
    pub fn do_stop_extension_hosts(&self) -> Promise<()> {
        let this = self.me();
        async_fn(async move {
            let previously_activated_extension_ids: Vec<ExtensionIdentifier> =
                this.extension_status.borrow().values().filter(|status| status.activation_started()).map(|status| status.id.clone()).collect();

            this.extension_host_managers.stop_all_in_reverse().await?;
            for extension_status in this.extension_status.borrow().values() {
                extension_status.clear_runtime_status();
            }

            if !previously_activated_extension_ids.is_empty() {
                this.on_did_change_extensions_status.fire(previously_activated_extension_ids);
            }
            Ok(())
        })
    }

    fn do_stop_extension_hosts_with_veto(&self, reason: &str, auto: bool) -> Promise<bool> {
        let this = self.me();
        let reason = reason.to_owned();
        async_fn(async move {
            if auto && this.environment_service.is_extension_development() {
                return Ok(false);
            }

            let veto_reasons: Rc<RefCell<IndexSet<String>>> = Rc::new(RefCell::new(IndexSet::new()));

            let event = WillStopExtensionHostsEvent::new(&reason, auto);
            this.on_will_stop.fire(event.clone());

            let mut vetos: Vec<MaybePromise<bool>> = Vec::new();
            for (value, veto_reason) in event.vetoes() {
                vetos.push(MaybePromise::Promise(value.clone()));

                let veto_reasons = veto_reasons.clone();
                let _ = value.then_result(move |result| {
                    match result {
                        Ok(true) => {
                            veto_reasons.borrow_mut().insert(veto_reason);
                        }
                        Ok(false) => {}
                        Err(error) => {
                            veto_reasons.borrow_mut().insert(localize(
                                "extensionStopVetoError",
                                "{0} (Error: {1})",
                                &[veto_reason.as_str(), to_error_message_of(&error, false).as_str()],
                            ));
                        }
                    }
                    Ok(())
                });
            }

            let log_service = this.log_service.clone();
            let veto = handle_vetos(vetos, Rc::new(move |error: &Error| log_service.error_args(&LogMessage::from(error), &[]))).await?;
            if !veto {
                this.do_stop_extension_hosts().await?;
            } else if !auto {
                let veto_reasons_array: Vec<String> = veto_reasons.borrow().iter().cloned().collect();

                this.log_service.warn(&format!(
                    "Extension host was not stopped because of veto (stop reason: {reason}, veto reason: {})",
                    veto_reasons_array.join(", ")
                ));

                let mut base = BaseDialogOptions::new(localize("extensionStopVetoMessage", "Please confirm restart of extensions.", &[]));
                base.dialog_type = Some(DialogTypeOrSeverity::Severity(Severity::Warning));
                base.detail = Some(DialogDetail::Text(if veto_reasons_array.len() == 1 { veto_reasons_array[0].clone() } else { veto_reasons_array.join("\n -") }));
                let confirmation = this
                    .dialog_service
                    .confirm(Confirmation { base, primary_button: Some(localize("proceedAnyways", "Restart Anyway", &[])), cancel_button: None })
                    .await?;

                if confirmation.confirmed {
                    return Ok(true);
                }
            }

            Ok(!veto)
        })
    }

    /// `Array.from(this._allRequestedActivateEvents.keys())`.
    fn requested_activate_events(&self) -> Vec<String> {
        self.all_requested_activate_events.borrow().iter().cloned().collect()
    }

    fn start_extension_hosts_if_necessary(&self, is_initial_start: bool, initial_activation_events: Vec<String>) {
        let mut locations: Vec<ExtensionRunningLocation> = Vec::new();
        for affinity in 0..=self.running_locations.max_local_process_affinity() {
            locations.push(ExtensionRunningLocation::LocalProcess { affinity });
        }
        for affinity in 0..=self.running_locations.max_local_web_worker_affinity() {
            locations.push(ExtensionRunningLocation::LocalWebWorker { affinity });
        }
        locations.push(ExtensionRunningLocation::Remote);
        for location in locations {
            if self.extension_host_managers.get_by_running_location(&location).is_some() {
                // already running
                continue;
            }
            if let Some((ext_host_manager, disposable_store)) = self.create_extension_host_manager(location, is_initial_start, &initial_activation_events) {
                self.extension_host_managers.add(ext_host_manager, disposable_store);
            }
        }
    }

    fn create_extension_host_manager(
        &self,
        running_location: ExtensionRunningLocation,
        is_initial_start: bool,
        initial_activation_events: &[String],
    ) -> Option<(Rc<dyn IExtensionHostManager>, Rc<DisposableStore>)> {
        let extension_host = self.extension_host_factory.create_extension_host(&self.running_locations, running_location, is_initial_start)?;

        let process_manager: Rc<dyn IExtensionHostManager> = self.hooks().do_create_extension_host_manager(self, extension_host, initial_activation_events.to_vec());
        let disposable_store = DisposableStore::new();

        let service = self.this.clone();
        let manager = Rc::downgrade(&process_manager);
        disposable_store.add(process_manager.on_did_exit().listen(move |(code, signal): &(i32, Option<String>)| {
            if let (Some(this), Some(manager)) = (service.upgrade(), manager.upgrade()) {
                this.on_extension_host_crash_or_exit(&manager, *code, signal.as_deref());
            }
        }));

        let service = self.this.clone();
        let manager = Rc::downgrade(&process_manager);
        disposable_store.add(process_manager.on_did_change_responsive_state().listen(move |responsive_state: &ResponsiveState| {
            let (Some(this), Some(manager)) = (service.upgrade(), manager.upgrade()) else { return };
            let is_responsive = *responsive_state == ResponsiveState::Responsive;
            this.log_service.info(&format!("Extension host ({}) is {}.", manager.friendly_name(), if is_responsive { "responsive" } else { "unresponsive" }));
            let inspect_manager = manager.clone();
            this.on_did_change_responsive_change.fire(IResponsiveStateChangeEvent {
                extension_host_kind: manager.kind(),
                is_responsive,
                get_inspect_listener: Rc::new(move |try_enable_inspector: bool| inspect_manager.get_inspect_port(try_enable_inspector)),
            });
        }));
        Some((process_manager, disposable_store))
    }

    fn on_extension_host_crash_or_exit(&self, extension_host: &Rc<dyn IExtensionHostManager>, code: i32, signal: Option<&str>) {
        // Unexpected termination
        let is_extension_dev_host = parse_extension_dev_options(&*self.environment_service).is_extension_dev_host;
        if !is_extension_dev_host {
            self.hooks().on_extension_host_crashed(self, extension_host, code, signal);
            return;
        }

        let _ = self.hooks().on_extension_host_exit(code);
    }

    /// O corpo de `_onExtensionHostCrashed(extensionHost, code, signal)` (o `super._onExtensionHostCrashed`).
    pub fn on_extension_host_crashed_base(&self, extension_host: &Rc<dyn IExtensionHostManager>, code: i32, signal: Option<&str>) {
        eprintln!("Extension host ({}) terminated unexpectedly. Code: {code}, Signal: {}", extension_host.friendly_name(), signal.unwrap_or("null"));
        match extension_host.kind() {
            ExtensionHostKind::LocalProcess => {
                let _ = self.do_stop_extension_hosts();
            }
            ExtensionHostKind::Remote => {
                if let Some(signal) = signal.filter(|signal| !signal.is_empty()) {
                    let _ = self.on_remote_extension_host_crashed(extension_host, signal);
                }
                let _ = self.extension_host_managers.stop_one(extension_host);
            }
            ExtensionHostKind::LocalWebWorker => {}
        }
    }

    fn get_extension_host_exit_info_with_timeout(&self, reconnection_token: &str) -> Promise<Option<IExtensionHostExitInfo>> {
        let (promise, resolver) = Promise::new();
        let timeout_resolver = resolver.clone();
        let timeout_handle = runtime::set_timeout(move || timeout_resolver.reject(Error::new("getExtensionHostExitInfo timed out")), 2000);
        let _ = self.remote_agent_service.get_extension_host_exit_info(reconnection_token).then_result(move |result| {
            match result {
                Ok(info) => {
                    runtime::clear_timeout(timeout_handle);
                    resolver.resolve(info);
                }
                Err(error) => resolver.reject(error),
            }
            Ok(())
        });
        promise
    }

    fn on_remote_extension_host_crashed(&self, extension_host: &Rc<dyn IExtensionHostManager>, reconnection_token: &str) -> Promise<()> {
        let this = self.me();
        let extension_host = extension_host.clone();
        let reconnection_token = reconnection_token.to_owned();
        async_fn(async move {
            let result: Result<(), Error> = async {
                let info = this.get_extension_host_exit_info_with_timeout(&reconnection_token).await?;
                if let Some(info) = info {
                    this.log_service.error(&format!("Extension host ({}) terminated unexpectedly with code {}.", extension_host.friendly_name(), info.code));
                }

                this.log_extension_host_crash(&extension_host);
                this.remote_crash_tracker.register_crash();

                if this.remote_crash_tracker.should_automatically_restart() {
                    this.log_service.info("Automatically restarting the remote extension host.");
                    this.notification_service.status(
                        NotificationMessage::Text(localize("extensionService.autoRestart", "The remote extension host terminated unexpectedly. Restarting...", &[])),
                        Some(StatusMessageOptions { show_after: None, hide_after: Some(5000) }),
                    );
                    this.start_extension_hosts_if_necessary(false, this.requested_activate_events());
                } else {
                    let service = this.clone();
                    this.notification_service.prompt(
                        Severity::Error,
                        NotificationMessage::Text(localize("extensionService.crash", "Remote Extension host terminated unexpectedly 3 times within the last 5 minutes.", &[])),
                        vec![PromptChoiceKind::Plain(PromptChoice {
                            base: BasePromptChoice {
                                telemetry_id: Some(NotificationActionTelemetryId::RestartExtensionHost),
                                label: localize("restart", "Restart Remote Extension Host", &[]),
                                keep_open: false,
                                run: Rc::new(move || {
                                    service.start_extension_hosts_if_necessary(false, service.requested_activate_events());
                                }),
                            },
                            is_secondary: false,
                        })],
                        Some(PromptOptions {
                            properties: NotificationProperties { telemetry: Some(NotificationTelemetry::Id(NotificationTelemetryId::RemoteExtensionHostCrashRepeated)), ..Default::default() },
                            on_cancel: None,
                        }),
                    );
                }
                Ok(())
            }
            .await;
            // maybe this wasn't an extension host crash and it was a permanent disconnection
            let _ = result;
            Ok(())
        })
    }

    /// `_logExtensionHostCrash(extensionHost)`.
    pub fn log_extension_host_crash(&self, extension_host: &Rc<dyn IExtensionHostManager>) {
        let activated_extensions: Vec<ExtensionIdentifier> = self
            .extension_status
            .borrow()
            .values()
            .filter(|status| status.activation_started() && extension_host.contains_extension(&status.id))
            .map(|status| status.id.clone())
            .collect();

        if !activated_extensions.is_empty() {
            self.log_service.error(&format!(
                "Extension host ({}) terminated unexpectedly. The following extensions were running: {}",
                extension_host.friendly_name(),
                activated_extensions.iter().map(|id| id.value.as_str()).collect::<Vec<_>>().join(", ")
            ));
        } else {
            self.log_service.error(&format!("Extension host ({}) terminated unexpectedly. No extensions were activated.", extension_host.friendly_name()));
        }
    }

    fn start_on_demand_extension_hosts(&self) {
        let snapshot = self.registry.get_snapshot();
        for ext_host_manager in self.extension_host_managers.snapshot() {
            if ext_host_manager.startup() != ExtensionHostStartup::EagerAutoStart {
                let extensions = self.running_locations.filter_by_extension_host_manager(&snapshot.extensions, &*ext_host_manager);
                let _ = ext_host_manager.start(snapshot.version_id, snapshot.extensions.clone(), extensions.iter().map(|extension| extension.identifier.clone()).collect());
            }
        }
    }

    //#endregion

    //#region IExtensionService

    fn do_activate_by_event(&self, activation_event: &str, activation_kind: ActivationKind) -> Promise<()> {
        let managers: Vec<Rc<dyn IExtensionHostManager>> = if activation_kind == ActivationKind::Immediate {
            // For immediate activation, only activate on local extension hosts
            // and on remote extension hosts that are already ready.
            // Defer activation for remote hosts that are not yet ready to avoid
            // blocking (e.g. during remote authority resolution).
            let managers = self
                .extension_host_managers
                .snapshot()
                .into_iter()
                .filter(|ext_host_manager| {
                    ext_host_manager.kind() == ExtensionHostKind::LocalProcess || ext_host_manager.kind() == ExtensionHostKind::LocalWebWorker || ext_host_manager.is_ready()
                })
                .collect();
            self.pending_remote_activation_events.borrow_mut().insert(activation_event.to_owned());
            managers
        } else {
            self.extension_host_managers.snapshot()
        };

        let result = Promise::all(managers.iter().map(|ext_host_manager| ext_host_manager.activate_by_event(activation_event, activation_kind)).collect()).then(|_| ());
        self.on_will_activate_by_event.fire(IWillActivateEvent { event: activation_event.to_owned(), activation: result.clone(), activation_kind });
        result
    }

    /// `activateById(extensionId, reason)` / `_activateById(extensionId, reason)`.
    pub fn activate_by_id(&self, extension_id: &ExtensionIdentifier, reason: ExtensionActivationReason) -> Promise<()> {
        let results: Vec<Promise<bool>> = self.extension_host_managers.snapshot().iter().map(|manager| manager.activate(extension_id, reason.clone())).collect();
        let extension_id = extension_id.value.clone();
        async_fn(async move {
            let results = Promise::all(results).await?;
            let activated = results.iter().any(|activated| *activated);
            if !activated {
                return Err(Error::new(format!("Unknown extension {extension_id}")));
            }
            Ok(())
        })
    }

    /// `_getExtensionRegistrySnapshotWhenReady()`.
    pub fn get_extension_registry_snapshot_when_ready(&self) -> Promise<ExtensionDescriptionRegistrySnapshot> {
        let this = self.me();
        self.installed_extensions_ready.wait().then(move |_| this.registry.get_snapshot())
    }

    /// `readExtensionPointContributions(extPoint)`.
    pub fn read_extension_point_contributions(&self, ext_point: &ExtensionPoint) -> Promise<Vec<ExtensionPointContribution>> {
        let this = self.me();
        let name = ext_point.name.clone();
        self.installed_extensions_ready.wait().then(move |_| {
            let available_extensions = this.registry.get_all_extension_descriptions();

            let mut result: Vec<ExtensionPointContribution> = Vec::new();
            for desc in available_extensions {
                if let Some(value) = extension_contributions(&desc).get(&name) {
                    result.push(ExtensionPointContribution { description: desc.clone(), value: value.clone() });
                }
            }

            result
        })
    }

    /// `getExtensionsStatus()`.
    pub fn get_extensions_status(&self) -> IndexMap<String, IExtensionsStatus> {
        let mut result: IndexMap<String, IExtensionsStatus> = IndexMap::new();
        let extensions = self.registry.get_all_extension_descriptions();
        for extension in extensions {
            let extension_status = self.extension_status.borrow().get(&extension.identifier.value).cloned();
            result.insert(
                extension.identifier.value.clone(),
                IExtensionsStatus {
                    id: extension.identifier.clone(),
                    messages: extension_status.as_ref().map(|status| status.messages()).unwrap_or_default(),
                    activation_started: extension_status.as_ref().is_some_and(|status| status.activation_started()),
                    activation_times: extension_status.as_ref().and_then(|status| status.activation_times()),
                    runtime_errors: extension_status.as_ref().map(|status| status.runtime_errors()).unwrap_or_default(),
                    running_location: self.running_locations.get_running_location(&extension.identifier.value),
                },
            );
        }
        result
    }

    /// `getInspectPorts(extensionHostKind, tryEnableInspector)`.
    pub fn get_inspect_ports(&self, extension_host_kind: ExtensionHostKind, try_enable_inspector: bool) -> Promise<Vec<IExtensionInspectInfo>> {
        let promises: Vec<Promise<Option<IExtensionInspectInfo>>> = self
            .get_extension_host_managers(extension_host_kind)
            .into_iter()
            .map(|ext_host| {
                let friendly_name = ext_host.friendly_name();
                ext_host.get_inspect_port(try_enable_inspector).then(move |port_info| {
                    port_info.map(|port_info| IExtensionInspectInfo { devtools_label: Some(friendly_name), ..port_info })
                })
            })
            .collect();
        // remove 0s:
        Promise::all(promises).then(|result| result.into_iter().flatten().collect())
    }

    /// `setRemoteEnvironment(env)` (ver a nota do módulo: o original não espera os hosts).
    pub fn set_remote_environment(&self, env: Vec<(String, Option<String>)>) -> Promise<()> {
        for manager in self.extension_host_managers.snapshot() {
            let _ = manager.set_remote_environment(env.clone());
        }
        Promise::resolved(())
    }

    //#endregion

    // --- impl

    fn safe_invoke_is_enabled(&self, extension: &IExtension) -> bool {
        errors::catch(|| self.extension_enablement_service.is_enabled(extension)).unwrap_or(false)
    }

    fn do_handle_extension_points(&self, affected_extensions: &[SharedExtensionDescription], only_resolver_extension_points: bool) {
        let mut affected_extension_points: HashSet<String> = HashSet::new();
        for extension_description in affected_extensions {
            for ext_point_name in extension_contributions(extension_description).keys() {
                affected_extension_points.insert(ext_point_name.clone());
            }
        }

        let service = self.this.clone();
        let message_handler: Rc<dyn Fn(IMessage)> = Rc::new(move |msg: IMessage| {
            if let Some(this) = service.upgrade() {
                this.handle_extension_point_message(msg);
            }
        });
        let available_extensions: Vec<(SharedExtensionDescription, Map<String, Value>)> =
            self.registry.get_all_extension_descriptions().into_iter().map(|desc| { let contributions = extension_contributions(&desc); (desc, contributions) }).collect();
        let extension_points = extensions_registry().get_extension_points();
        mark(if only_resolver_extension_points { "code/willHandleResolverExtensionPoints" } else { "code/willHandleExtensionPoints" }, None);
        for extension_point in extension_points {
            if affected_extension_points.contains(&extension_point.name) && (!only_resolver_extension_points || extension_point.can_handle_resolver == Some(true)) {
                mark(&format!("code/willHandleExtensionPoint/{}", extension_point.name), None);
                Self::handle_extension_point(&extension_point, &available_extensions, &message_handler);
                mark(&format!("code/didHandleExtensionPoint/{}", extension_point.name), None);
            }
        }
        mark(if only_resolver_extension_points { "code/didHandleResolverExtensionPoints" } else { "code/didHandleExtensionPoints" }, None);
    }

    fn get_or_create_extension_status(&self, extension_id: &ExtensionIdentifier) -> Rc<ExtensionStatus> {
        if let Some(status) = self.extension_status.borrow().get(&extension_id.value) {
            return status.clone();
        }
        let status = Rc::new(ExtensionStatus::new(extension_id.clone()));
        self.extension_status.borrow_mut().set(&extension_id.value, status.clone());
        status
    }

    fn handle_extension_point_message(&self, msg: IMessage) {
        let extension_status = self.get_or_create_extension_status(&msg.extension_id);
        extension_status.add_message(msg.clone());

        let extension = self.registry.get_extension_description(&msg.extension_id.value);
        let str_msg = format!("[{}]: {}", msg.extension_id.value, msg.message);
        let is_under_development = extension.is_some_and(|extension| extension.is_under_development);

        if msg.severity == Severity::Error {
            if is_under_development {
                // This message is about the extension currently being developed
                self.notification_service.notify(Notification::new(Severity::Error, str_msg.clone()));
            }
            self.log_service.error(&str_msg);
        } else if msg.severity == Severity::Warning {
            if is_under_development {
                // This message is about the extension currently being developed
                self.notification_service.notify(Notification::new(Severity::Warning, str_msg.clone()));
            }
            self.log_service.warn(&str_msg);
        } else {
            self.log_service.info(&str_msg);
        }

        if self.environment_service.is_built() && !self.environment_service.is_extension_development() {
            self.telemetry_service.public_log2(
                "extensionsMessage",
                Some(Dynamic::object([
                    ("type", Dynamic::Number(f64::from(msg.severity as i32))),
                    ("extensionId", Dynamic::string(&msg.extension_id.value)),
                    ("extensionPointId", Dynamic::string(&msg.extension_point_id)),
                    ("message", Dynamic::string(&msg.message)),
                ])),
            );
        }
    }

    fn handle_extension_point(extension_point: &ExtensionPoint, available_extensions: &[(SharedExtensionDescription, Map<String, Value>)], message_handler: &Rc<dyn Fn(IMessage)>) {
        let mut users: Vec<ExtensionPointUser<Value>> = Vec::new();
        for (desc, contributions) in available_extensions {
            if let Some(value) = contributions.get(&extension_point.name) {
                users.push(ExtensionPointUser {
                    description: desc.clone(),
                    value: value.clone(),
                    collector: Rc::new(ExtensionMessageCollectorImpl::new(message_handler.clone(), desc.clone(), &extension_point.name)),
                });
            }
        }
        extension_point.accept_users(users);
    }

    //#region Called by extension host

    /// `_doCreateExtensionHostManager(extensionHost, initialActivationEvents)` do `AbstractExtensionService`: o gerenciador de verdade
    /// ([`ExtensionHostManager`]) ou, quando o host é de partida preguiçosa, o [`LazyCreateExtensionHostManager`].
    pub fn do_create_extension_host_manager_base(&self, extension_host: Rc<dyn IExtensionHost>, initial_activation_events: Vec<String>) -> Rc<dyn IExtensionHostManager> {
        let internal_extension_service = self.acquire_internal_api(extension_host.clone());
        let services = ExtensionHostManagerServices {
            environment_service: self.environment_service.clone(),
            telemetry_service: self.telemetry_service.clone(),
            log_service: self.log_service.clone(),
        };
        if extension_host.startup() == ExtensionHostStartup::LazyAutoStart {
            return LazyCreateExtensionHostManager::new(extension_host, initial_activation_events, internal_extension_service, services);
        }
        ExtensionHostManager::new(extension_host, initial_activation_events, internal_extension_service, services)
    }

    /// `_acquireInternalAPI(extensionHost)`: o que o host recebe do serviço.
    pub fn acquire_internal_api(&self, extension_host: Rc<dyn IExtensionHost>) -> Rc<dyn IInternalExtensionService> {
        Rc::new(InternalExtensionServiceApi { service: self.this.clone(), extension_host })
    }

    fn on_will_activate_extension(&self, extension_id: &ExtensionIdentifier, running_location: ExtensionRunningLocation) {
        self.running_locations.set(&extension_id.value, running_location);
        let extension_status = self.get_or_create_extension_status(extension_id);
        extension_status.on_will_activate();
    }

    fn on_did_activate_extension(
        &self,
        extension_id: &ExtensionIdentifier,
        code_loading_time: f64,
        activate_call_time: f64,
        activate_resolved_time: f64,
        activation_reason: ExtensionActivationReason,
    ) {
        let extension_status = self.get_or_create_extension_status(extension_id);
        extension_status.set_activation_times(ActivationTimes { code_loading_time, activate_call_time, activate_resolved_time, activation_reason });
        self.on_did_change_extensions_status.fire(vec![extension_id.clone()]);
    }

    fn on_did_activate_extension_error(&self, extension_id: &ExtensionIdentifier, error: &Error) {
        self.telemetry_service.public_log2(
            "extensionActivationError",
            Some(Dynamic::object([("extensionId", Dynamic::string(&extension_id.value)), ("error", Dynamic::string(&error.message))])),
        );
    }

    fn on_extension_runtime_error(&self, extension_id: &ExtensionIdentifier, err: &Error) {
        let extension_status = self.get_or_create_extension_status(extension_id);
        extension_status.add_runtime_error(err.clone());
        self.on_did_change_extensions_status.fire(vec![extension_id.clone()]);
    }

    //#endregion
}

impl IDisposable for AbstractExtensionService {
    fn dispose(&self) {
        self.base.dispose();
    }
}

impl IExtensionService for AbstractExtensionService {
    fn on_will_stop(&self) -> Event<WillStopExtensionHostsEvent> {
        self.on_will_stop.event()
    }

    fn activate_by_event(&self, activation_event: &str, activation_kind: Option<ActivationKind>) -> Promise<()> {
        let activation_kind = activation_kind.unwrap_or(ActivationKind::Normal);
        if self.installed_extensions_ready.is_open() {
            // Extensions have been scanned and interpreted

            // Record the fact that this activationEvent was requested (in case of a restart)
            self.all_requested_activate_events.borrow_mut().insert(activation_event.to_owned());

            if !self.registry.contains_activation_event(activation_event) {
                // There is no extension that is interested in this activation event
                return Promise::resolved(());
            }

            self.do_activate_by_event(activation_event, activation_kind)
        } else {
            // Extensions have not been scanned yet.

            // Record the fact that this activationEvent was requested (in case of a restart)
            self.all_requested_activate_events.borrow_mut().insert(activation_event.to_owned());

            if activation_kind == ActivationKind::Immediate {
                // Do not wait for the normal start-up of the extension host(s)

                // Note: some callers come in so early that the extension hosts have not even been created yet.
                // Therefore we kick off the extension host creation, but without awaiting it.
                // See https://github.com/microsoft/vscode/issues/260061
                let _ = self.initialize_if_needed();

                return self.do_activate_by_event(activation_event, activation_kind);
            }

            let this = self.me();
            let activation_event = activation_event.to_owned();
            self.installed_extensions_ready.wait().then_promise(move |_| this.do_activate_by_event(&activation_event, activation_kind))
        }
    }

    fn activation_event_is_done(&self, activation_event: &str) -> bool {
        if !self.installed_extensions_ready.is_open() {
            return false;
        }
        if !self.registry.contains_activation_event(activation_event) {
            // There is no extension that is interested in this activation event
            return true;
        }
        self.extension_host_managers.snapshot().iter().all(|manager| manager.activation_event_is_done(activation_event))
    }

    fn get_extension(&self, id: &str) -> Promise<Option<IExtensionDescription>> {
        let this = self.me();
        let id = id.to_owned();
        self.installed_extensions_ready.wait().then(move |_| {
            this.registry.get_extension_description(&id).map(|description| IExtensionDescription {
                extension_location: URI::parse(description.extension_location.as_str(), false),
                display_name: description.manifest.display_name.clone(),
                name: Some(description.manifest.name.clone()),
            })
        })
    }

    fn on_did_register_extensions(&self) -> Event<()> {
        self.on_did_register_extensions.event()
    }

    fn when_installed_extensions_registered(&self) -> Promise<bool> {
        self.installed_extensions_ready.wait()
    }

    fn extensions(&self) -> Vec<ExtensionDescription> {
        self.registry.get_all_extension_descriptions().iter().map(|extension| (**extension).clone()).collect()
    }

    fn on_did_change_extensions(&self) -> Event<IExtensionsChangeEvent> {
        self.on_did_change_extensions.event()
    }

    fn on_did_change_extensions_status(&self) -> Event<Vec<ExtensionIdentifier>> {
        self.on_did_change_extensions_status.event()
    }

    fn can_remove_extension(&self, extension: &ExtensionDescription) -> bool {
        let Some(extension_description) = self.registry.get_extension_description(&extension.identifier.value) else {
            // Can't remove an extension that is unknown!
            return false;
        };

        if self.extension_status.borrow().get(&extension_description.identifier.value).is_some_and(|status| status.activation_started()) {
            // Extension is running, cannot remove it safely
            return false;
        }

        true
    }

    fn can_add_extension(&self, extension: &ExtensionDescription) -> bool {
        self.can_add_extension_description(&Rc::new(extension.clone()), &[])
    }

    fn stop_extension_hosts(&self, reason: &str, auto: bool) -> Promise<bool> {
        let this = self.me();
        let reason = reason.to_owned();
        async_fn(async move {
            this.initialize_if_needed().await?;
            this.do_stop_extension_hosts_with_veto(&reason, auto).await
        })
    }

    fn start_extension_hosts(&self, updates: Option<ExtensionHostUpdates>) -> Promise<()> {
        let this = self.me();
        async_fn(async move {
            this.do_stop_extension_hosts().await?;

            if let Some(updates) = updates {
                this.handle_delta_extensions(DeltaExtensionsQueueItem::new(updates.to_add.into_iter().map(Rc::new).collect(), DeltaToRemove::Ids(updates.to_remove))).await?;
            }

            let lock = this.registry.acquire_lock("startExtensionHosts").await?;
            let result: Result<(), Error> = async {
                this.start_extension_hosts_if_necessary(false, this.requested_activate_events());
                this.start_on_demand_extension_hosts();

                let local_process_extension_hosts = this.get_extension_host_managers(ExtensionHostKind::LocalProcess);
                Promise::all(local_process_extension_hosts.iter().map(|ext_host| ext_host.ready()).collect()).await?;
                Ok(())
            }
            .await;
            lock.dispose();
            result
        })
    }
}
