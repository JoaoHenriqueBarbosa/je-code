//! O `ExtHostExtensionService` do host de extensões em Rust: o lado do host do protocolo de ativação, que o `ExtensionHostManager` do
//! workbench chama pelo RPC (`$startExtensionHost`, `$activateByEvent`, `$activate`, `$deltaExtensions`...). Porte de
//! `src/vs/workbench/api/common/extHostExtensionService.ts` (`AbstractExtHostExtensionService`) e do carregador CommonJS de
//! `api/node/extHostExtensionService.ts`, sobre o zjsc: o código das extensões roda no motor JavaScript e tudo o mais é Rust.
//!
//! Decisão de arquitetura do `wip/notes/exthost-plan.md`: o `extensionHostProcess.js` não entra no produto; este serviço é o que o
//! substitui, com os registros, o `ExtensionsActivator` (de `je_workbench::api::common::ext_host_extension_activator`) e o
//! `SyncedActivationEventsReader` do original.
//!
//! Divergências:
//! - **Carregador.** `_loadCommonJSModule` é o `require(path)` do programa Node do `NodeRunner`: `require` só existe no escopo do
//!   módulo principal, então o programa principal publica a ponte `globalThis.__jeRequire = (p) => require(p)`
//!   ([`crate::runtime::HOST_MAIN_SOURCE`]). Extensões ESM (`.mjs`, `"type": "module"`) falham a ativação com um erro explícito.
//! - **`ExtensionContext`** reduzido ao que o host já sustenta: `subscriptions`, `extensionPath`, `asAbsolutePath` e `extensionMode`.
//!   `globalState`, `workspaceState`, `secrets`, `storageUri`, `globalStorageUri`, `logUri`, `extension` e
//!   `environmentVariableCollection` dependem do `ExtHostStorage`, do `ExtHostSecretState` e do `IExtensionStoragePaths`, ainda não
//!   portados no host.
//! - **`_startExtensionHost`** cobre a ativação `*` e `onStartupFinished`. `workspaceContains:` (precisa do `ExtHostWorkspace`) e
//!   `onResolveRemoteAuthority:` (host local não tem autoridade) ficam de fora; a espera de 10 s do original pelo término das
//!   ativações antes de `onStartupFinished` é a própria espera pelas ativações eager (sem relógio).
//! - **Resolvedores remotos.** O `vscode` do host não expõe `registerRemoteAuthorityResolver`, então o registro de resolvedores está
//!   sempre vazio: `$resolveAuthority` e `$getCanonicalURI` seguem o original até a procura do resolvedor (autoridade sem `+` é
//!   `InvalidAuthority`; com `+` ativa `onResolveRemoteAuthority:<prefixo>` e, sem resolvedor, é `NoResolverFound` /
//!   `null`).
//! - **`deactivate`** do módulo roda sem esperar o retorno (o `ExtensionModule.deactivate` do porte é `Fn()`); depois o descartável
//!   da ativação (as `subscriptions`) é descartado, como em `_deactivate`.
//! - **Telemetria** `activatePlugin` e `extensionActivationTimes` saem pelo proxy `MainThreadTelemetry`. `console.error` é `eprintln!`.
//! - `$extensionTestsExecute` espera as ativações eager e rejeita com `Cannot load test runner.` (o host não tem o ambiente de
//!   desenvolvimento de extensão: `extensionDevelopmentLocationURI` e `extensionTestsLocationURI`).
//! - `$setRemoteEnvironment` e `$updateRemoteConnectionData` não fazem nada num host local (o original só age quando `remote.isRemote`).

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::path::Path;
use std::rc::{Rc, Weak};

use je_base::async_::{Barrier, async_fn};
use je_base::buffer::VSBuffer;
use je_base::dynamic::Dynamic;
use je_base::errors::{Error, transform_error_for_serialization};
use je_base::lifecycle::{DisposableRef, to_disposable};
use je_base::promise::{MaybePromise, Promise};
use je_base::stopwatch::StopWatch;
use je_base::uri::URI;
use je_extensions::manifest::{ExtensionIdentifier, ExtensionIdentifierMap, ExtensionIdentifierSet};
use je_extensions::scanner::ExtensionDescription;
use je_platform::log::{LogServiceRef, Logger};
use je_platform::remote::remote_authority_resolver::{RemoteAuthorityResolverErrorCode, get_remote_authority_prefix};
use je_workbench::api::common::ext_host_extension_activator::{
    ActivatedExtension, ExtensionActivationTimes, ExtensionActivationTimesBuilder, ExtensionModule, ExtensionsActivator, IExtensionsActivatorHost,
};
use je_workbench::api::common::ext_host_protocol::{IMainContext, MainContext, MainThreadExtensionServiceShape, MainThreadTelemetryShape};
use je_workbench::api::common::ext_host_values::ApiValue;
use je_workbench::services::extensions::extension_activation::{ExtensionActivationReason, MissingExtensionDependency};
use je_workbench::services::extensions::extension_description_registry::{
    ExtensionDescriptionRegistry, IActivationEventsReader, IReadOnlyExtensionDescriptionRegistry, SharedExtensionDescription,
};
use je_workbench::services::extensions::extension_host_proxy::{IResolveAuthorityResult, ResolveAuthorityError, resolve_authority_result_to_dynamic};
use je_workbench::services::extensions::extension_host_protocol::{
    IExtensionDescriptionDelta, IExtensionDescriptionSnapshot, environment_from_dynamic, extension_activation_reason_from_dynamic,
    extension_identifier_from_dynamic,
};
use je_workbench::services::extensions::extensions::ActivationKind;
use je_workbench::services::extensions::rpc_protocol::IRpcActor;
use zjsc::api::embed_node::{JsData, JsHandle, NodeRunner};

use crate::api::commands::settle_js_result;
use crate::api::disposable::DisposableTable;
use crate::convert::{dynamic_to_js, field, js_to_dynamic, js_to_error};

/// `ExtensionMode.Production`.
const EXTENSION_MODE_PRODUCTION: f64 = 1.0;

/// `SyncedActivationEventsReader`: os eventos de ativação que o workbench calculou (ele conhece todos os pontos de extensão), por
/// extensão.
#[derive(Default)]
pub struct SyncedActivationEventsReader {
    map: RefCell<ExtensionIdentifierMap<Vec<String>>>,
}

impl SyncedActivationEventsReader {
    pub fn new(activation_events: &HashMap<String, Vec<String>>) -> Rc<Self> {
        let reader = Rc::new(Self::default());
        reader.add_activation_events(activation_events);
        reader
    }

    pub fn add_activation_events(&self, activation_events: &HashMap<String, Vec<String>>) {
        for (extension_id, events) in activation_events {
            self.map.borrow_mut().set(extension_id, events.clone());
        }
    }
}

impl IActivationEventsReader for SyncedActivationEventsReader {
    fn read_activation_events(&self, extension_description: &ExtensionDescription) -> Vec<String> {
        self.map.borrow().get(&extension_description.identifier.value).cloned().unwrap_or_default()
    }
}

/// `filterExtensions(globalRegistry, desiredExtensions)`.
fn filter_extensions(global_registry: &ExtensionDescriptionRegistry, desired_extensions: &ExtensionIdentifierSet) -> Vec<SharedExtensionDescription> {
    global_registry.get_all_extension_descriptions().into_iter().filter(|extension| desired_extensions.has(&extension.identifier.value)).collect()
}

/// `printExtIds(registry)`.
fn print_ext_ids(registry: &ExtensionDescriptionRegistry) -> String {
    registry.get_all_extension_descriptions().iter().map(|extension| extension.identifier.value.as_str()).collect::<Vec<_>>().join(",")
}

/// `getTelemetryActivationEvent(extensionDescription, reason)`.
fn telemetry_activation_event(extension_description: &ExtensionDescription, reason: &ExtensionActivationReason) -> Vec<(&'static str, Dynamic)> {
    vec![
        ("id", Dynamic::string(&extension_description.identifier.value)),
        ("name", Dynamic::string(&extension_description.manifest.name)),
        ("extensionVersion", Dynamic::string(&extension_description.manifest.version)),
        ("publisherDisplayName", Dynamic::string(&extension_description.manifest.publisher)),
        (
            "activationEvents",
            extension_description.manifest.activation_events.as_ref().map_or(Dynamic::Null, |events| Dynamic::string(&events.join(","))),
        ),
        ("isBuiltin", Dynamic::Bool(extension_description.is_builtin)),
        ("reason", Dynamic::string(&reason.activation_event)),
        ("reasonId", Dynamic::string(&reason.extension_id.value)),
    ]
}

/// O que `_getEntryPoint` devolve: o `main` do manifesto.
fn entry_point(extension_description: &ExtensionDescription) -> Option<&str> {
    extension_description.manifest.main.as_deref().filter(|main| !main.is_empty())
}

/// O caminho do sistema de arquivos de uma URI de extensão (`module.scheme !== Schemas.file` lança).
fn file_system_path(location: &str) -> Result<String, Error> {
    let uri = URI::parse(location, false);
    if uri.scheme().to_string_lossy() != "file" {
        return Err(Error::new(format!("Cannot load URI: '{location}', must be of file-scheme")));
    }
    Ok(je_platform::resources::fs_path(&uri))
}

/// `_isESM(extensionDescription, modulePath)`.
fn is_esm(module_path: &str) -> bool {
    module_path.ends_with(".mjs")
}

/// `ExtHostExtensionService`.
pub struct ExtHostExtensionService {
    me: Weak<ExtHostExtensionService>,
    runner: NodeRunner,
    disposables: Rc<DisposableTable>,
    log_service: LogServiceRef,
    main_thread_extension_service: Rc<dyn MainThreadExtensionServiceShape>,
    main_thread_telemetry: Rc<dyn MainThreadTelemetryShape>,

    almost_ready_to_run_extensions: Barrier,
    ready_to_start_extension_host: Barrier,
    ready_to_run_extensions: Barrier,
    eager_extensions_activated: Barrier,
    activation_events_reader: Rc<SyncedActivationEventsReader>,
    global_registry: Rc<ExtensionDescriptionRegistry>,
    my_registry: Rc<ExtensionDescriptionRegistry>,
    activator: RefCell<Option<Rc<ExtensionsActivator>>>,
    started: Cell<bool>,
    auto_start: bool,
}

/// O `IExtensionsActivatorHost` do serviço (as duas funções que o original passa ao construtor do ativador).
struct ActivatorHost {
    service: Weak<ExtHostExtensionService>,
}

impl IExtensionsActivatorHost for ActivatorHost {
    fn on_extension_activation_error(&self, extension_id: &ExtensionIdentifier, error: Option<&Error>, missing_extension_dependency: Option<&MissingExtensionDependency>) {
        if let (Some(service), Some(error)) = (self.service.upgrade(), error) {
            let _ = service.main_thread_extension_service.rpc_on_extension_activation_error(
                extension_id,
                transform_error_for_serialization(error),
                missing_extension_dependency.cloned(),
            );
        }
    }

    fn actual_activate_extension(&self, extension_id: &ExtensionIdentifier, reason: &ExtensionActivationReason) -> Promise<Rc<ActivatedExtension>> {
        let Some(service) = self.service.upgrade() else { return Promise::rejected(Error::cancellation()) };
        let extension_id = extension_id.clone();
        let reason = reason.clone();
        async_fn(async move {
            if ExtensionDescriptionRegistry::is_host_extension(&extension_id.value, &service.my_registry, &service.global_registry) {
                service.main_thread_extension_service.rpc_activate_extension(&extension_id, &reason).await?;
                return Ok(Rc::new(ActivatedExtension::host()));
            }
            let Some(extension_description) = service.my_registry.get_extension_description(&extension_id.value) else {
                return Err(Error::new(format!("Cannot activate unknown extension '{}'", extension_id.value)));
            };
            service.activate_extension(extension_description, reason).await
        })
    }
}

impl ExtHostExtensionService {
    /// Cria o serviço sobre `context` (o `RPCProtocol` do lado do host, de onde saem os proxies `MainThread*`), com os registros do
    /// `initData.extensions` (`snapshot`: todas as extensões, as do host e os eventos de ativação que o workbench calculou).
    /// `auto_start` é o `initData.autoStart`.
    pub fn new(
        runner: NodeRunner,
        disposables: Rc<DisposableTable>,
        context: &dyn IMainContext,
        log_service: LogServiceRef,
        auto_start: bool,
        snapshot: IExtensionDescriptionSnapshot,
    ) -> Rc<Self> {
        let activation_events_reader = SyncedActivationEventsReader::new(&snapshot.activation_events);
        let reader: Rc<dyn IActivationEventsReader> = activation_events_reader.clone();
        let global_registry = ExtensionDescriptionRegistry::new(reader.clone(), snapshot.all_extensions.clone());
        let my_extensions_set = ExtensionIdentifierSet::new(snapshot.my_extensions.iter().map(|id| id.value.clone()));
        let my_registry = ExtensionDescriptionRegistry::new(reader, filter_extensions(&global_registry, &my_extensions_set));
        log_service.info(&format!("Creating extension host with the following global extensions: {}", print_ext_ids(&global_registry)));
        log_service.info(&format!("Creating extension host with the following local extensions: {}", print_ext_ids(&my_registry)));
        let service = Rc::new_cyclic(|me| Self {
            me: me.clone(),
            runner,
            disposables,
            log_service: log_service.clone(),
            main_thread_extension_service: context.get_proxy(&MainContext::MAIN_THREAD_EXTENSION_SERVICE),
            main_thread_telemetry: context.get_proxy(&MainContext::MAIN_THREAD_TELEMETRY),
            almost_ready_to_run_extensions: Barrier::new(),
            ready_to_start_extension_host: Barrier::new(),
            ready_to_run_extensions: Barrier::new(),
            eager_extensions_activated: Barrier::new(),
            activation_events_reader,
            global_registry: global_registry.clone(),
            my_registry: my_registry.clone(),
            activator: RefCell::new(None),
            started: Cell::new(false),
            auto_start,
        });
        let host: Rc<dyn IExtensionsActivatorHost> = Rc::new(ActivatorHost { service: Rc::downgrade(&service) });
        *service.activator.borrow_mut() = Some(ExtensionsActivator::new(my_registry, global_registry, host, log_service));
        service
    }

    fn this(&self) -> Rc<Self> {
        self.me.upgrade().expect("o serviço está vivo enquanto um método dele roda")
    }

    fn activator(&self) -> Rc<ExtensionsActivator> {
        self.activator.borrow().clone().expect("o ativador é criado junto com o serviço")
    }

    /// `initialize()`: sem `_beforeAlmostReadyToRunExtensions` nem espera de `$initializeWorkspace` (o host em processo não tem
    /// workspace próprio), abre as duas primeiras barreiras e, com `autoStart`, inicia o host.
    pub fn initialize(&self) {
        self.almost_ready_to_run_extensions.open();
        self.ready_to_start_extension_host.open();
        if self.auto_start {
            let _ = self.start_extension_host_impl();
        }
    }

    pub fn is_activated(&self, extension_id: &ExtensionIdentifier) -> bool {
        self.activator().is_activated(&extension_id.value)
    }

    /// `getExtensionExports(extensionId)`: `None` é o `null` do original (a extensão não está ativada ou é desconhecida); `Some(None)`
    /// é o `undefined` de uma extensão ativada cujo `activate` não devolveu nada.
    pub fn get_extension_exports(&self, extension_id: &ExtensionIdentifier) -> Option<Option<Dynamic>> {
        self.activator().get_activated_extension(&extension_id.value).ok().map(|activated| activated.exports.clone())
    }

    /// `activateByIdWithErrors(extensionId, reason)`: espera o host ficar pronto para rodar extensões, ativa a extensão e rejeita com o
    /// erro dela se a ativação falhou.
    pub fn activate_by_id_with_errors(&self, extension_id: ExtensionIdentifier, reason: ExtensionActivationReason) -> Promise<()> {
        let this = self.this();
        async_fn(async move {
            this.ready_to_run_extensions.wait().await?;
            this.activator().activate_by_id(&extension_id, reason)?.await?;
            let extension = this.activator().get_activated_extension(&extension_id.value)?;
            if extension.activation_failed {
                return Err(extension.activation_failed_error.clone().unwrap_or_else(|| Error::new(format!("Activation of '{}' failed", extension_id.value))));
            }
            Ok(())
        })
    }

    /// `getExtensionRegistry()`.
    pub fn my_registry(&self) -> &Rc<ExtensionDescriptionRegistry> {
        &self.my_registry
    }

    pub fn global_registry(&self) -> &Rc<ExtensionDescriptionRegistry> {
        &self.global_registry
    }

    // #region Ativação

    fn activate_by_event_impl(&self, activation_event: &str, startup: bool) -> Promise<()> {
        self.activator().activate_by_event(activation_event, startup)
    }

    /// `_activateExtension(extensionDescription, reason)`.
    fn activate_extension(&self, extension_description: SharedExtensionDescription, reason: ExtensionActivationReason) -> Promise<Rc<ActivatedExtension>> {
        let this = self.this();
        async_fn(async move {
            // local extension host process
            this.main_thread_extension_service.rpc_on_will_activate_extension(&extension_description.identifier).await?;
            match this.do_activate_extension(&extension_description, &reason).await {
                Ok(activated_extension) => {
                    let times = activated_extension.activation_times;
                    this.main_thread_extension_service.rpc_on_did_activate_extension(
                        &extension_description.identifier,
                        times.code_loading_time,
                        times.activate_call_time,
                        times.activate_resolved_time,
                        &reason,
                    );
                    this.log_extension_activation_times(&extension_description, &reason, "success", Some(&times));
                    Ok(activated_extension)
                }
                Err(err) => {
                    this.log_extension_activation_times(&extension_description, &reason, "failure", None);
                    Err(err)
                }
            }
        })
    }

    /// `_logExtensionActivationTimes(extensionDescription, reason, outcome, activationTimes?)`.
    fn log_extension_activation_times(&self, extension_description: &ExtensionDescription, reason: &ExtensionActivationReason, outcome: &str, activation_times: Option<&ExtensionActivationTimes>) {
        let mut event = telemetry_activation_event(extension_description, reason);
        if let Some(times) = activation_times {
            event.push(("startup", Dynamic::Bool(times.startup)));
            event.push(("codeLoadingTime", Dynamic::Number(times.code_loading_time)));
            event.push(("activateCallTime", Dynamic::Number(times.activate_call_time)));
            event.push(("activateResolvedTime", Dynamic::Number(times.activate_resolved_time)));
        }
        event.push(("outcome", Dynamic::string(outcome)));
        self.main_thread_telemetry.rpc_public_log2("extensionActivationTimes", Some(Dynamic::object(event)));
    }

    /// `_doActivateExtension(extensionDescription, reason)`.
    fn do_activate_extension(&self, extension_description: &SharedExtensionDescription, reason: &ExtensionActivationReason) -> Promise<Rc<ActivatedExtension>> {
        let this = self.this();
        let extension_description = extension_description.clone();
        let reason = reason.clone();
        async_fn(async move {
            this.main_thread_telemetry.rpc_public_log2("activatePlugin", Some(Dynamic::object(telemetry_activation_event(&extension_description, &reason))));
            let Some(entry_point) = entry_point(&extension_description) else {
                // Treat the extension as being empty => NOT AN ERROR CASE
                return Ok(Rc::new(ActivatedExtension::empty(ExtensionActivationTimes::NONE)));
            };

            this.log_service.info(&format!(
                "ExtensionService#_doActivateExtension {}, startup: {}, activationEvent: '{}'{}",
                extension_description.identifier.value,
                reason.startup,
                reason.activation_event,
                if extension_description.identifier.value != reason.extension_id.value {
                    format!(", root cause: {}", reason.extension_id.value)
                } else {
                    String::new()
                }
            ));
            this.log_service.flush();

            if is_esm(entry_point) {
                return Err(Error::new(format!("Cannot load ES module '{entry_point}': the zjsc extension host loads CommonJS extensions only")));
            }
            let module_path = Path::new(&file_system_path(&extension_description.extension_location)?).join(entry_point.trim_start_matches("./")).to_string_lossy().into_owned();

            let activation_times_builder = Rc::new(RefCell::new(ExtensionActivationTimesBuilder::new(reason.startup)));
            let extension_module = {
                activation_times_builder.borrow_mut().code_loading_start();
                let loaded = this.load_common_js_module(&module_path);
                activation_times_builder.borrow_mut().code_loading_stop();
                loaded?
            };
            let (context_data, subscriptions) = this.load_extension_context(&extension_description);
            this.call_activate(&extension_description.identifier, extension_module, context_data, subscriptions, activation_times_builder).await
        })
    }

    /// `_loadCommonJSModule(extension, module, activationTimesBuilder)`: o `require` da ponte do programa principal.
    fn load_common_js_module(&self, module_path: &str) -> Result<ExtensionModule, Error> {
        let require = self.runner.eval("globalThis.__jeRequire", "je-exthost/require-bridge.js").map_err(Error::new)?;
        if require.is_undefined() {
            return Err(Error::new("a ponte globalThis.__jeRequire não existe: o programa principal do host precisa publicá-la"));
        }
        let exports = self.runner.call_data_throwing(&require, &[JsData::Str(module_path.to_owned())]).map_err(|thrown| js_to_error(&thrown))?;
        let function_of = |name: &str| match field(&exports, name) {
            Some(JsData::Handle(function)) => Some(function.clone()),
            _ => None,
        };
        let runner = self.runner.clone();
        let activate = function_of("activate").map(|function| -> Rc<dyn Fn(Dynamic) -> Promise<Dynamic>> {
            let runner = runner.clone();
            Rc::new(move |context: Dynamic| match runner.call_data_throwing(&function, &[dynamic_to_js(&context)]) {
                Ok(result) => match settle_js_result(&runner, result) {
                    MaybePromise::Value(value) => Promise::resolved(value),
                    MaybePromise::Promise(promise) => promise,
                },
                Err(thrown) => Promise::rejected(js_to_error(&thrown)),
            })
        });
        let deactivate = function_of("deactivate").map(|function| -> Rc<dyn Fn()> {
            let runner = runner.clone();
            Rc::new(move || {
                let _ = runner.call_data_throwing(&function, &[]);
            })
        });
        Ok(ExtensionModule { activate, deactivate })
    }

    /// `_loadExtensionContext(extensionDescription, extensionInternalStore)`, reduzido (ver a nota do módulo). Devolve o contexto e o
    /// array `subscriptions` que a extensão preenche.
    fn load_extension_context(&self, extension_description: &ExtensionDescription) -> (Dynamic, Rc<JsHandle>) {
        self.log_service.trace(&format!("ExtensionService#loadExtensionContext {}", extension_description.identifier.value));
        let subscriptions = Rc::new(self.runner.eval("[]", "je-exthost/subscriptions.js").expect("um array literal avalia"));
        let extension_path = file_system_path(&extension_description.extension_location).unwrap_or_default();
        let as_absolute_path = {
            let base = extension_path.clone();
            Rc::new(self.runner.make_function(
                "asAbsolutePath",
                1,
                Rc::new(move |args| match args.first() {
                    Some(JsData::Str(relative)) => Ok(JsData::Str(Path::new(&base).join(relative).to_string_lossy().into_owned())),
                    _ => Err("asAbsolutePath: o caminho relativo precisa ser texto".to_owned()),
                }),
            ))
        };
        let context = JsData::Object(vec![
            ("subscriptions".to_owned(), JsData::Handle(subscriptions.clone())),
            ("extensionPath".to_owned(), JsData::Str(extension_path)),
            ("asAbsolutePath".to_owned(), JsData::Handle(as_absolute_path)),
            ("extensionMode".to_owned(), JsData::Num(EXTENSION_MODE_PRODUCTION)),
        ]);
        (js_to_dynamic(&context), subscriptions)
    }

    /// `_callActivate(...)` e `_callActivateOptional(...)`.
    fn call_activate(
        &self,
        extension_id: &ExtensionIdentifier,
        extension_module: ExtensionModule,
        context: Dynamic,
        subscriptions: Rc<JsHandle>,
        activation_times_builder: Rc<RefCell<ExtensionActivationTimesBuilder>>,
    ) -> Promise<Rc<ActivatedExtension>> {
        let this = self.this();
        let extension_id = extension_id.clone();
        async_fn(async move {
            let exports = match &extension_module.activate {
                Some(activate) => {
                    activation_times_builder.borrow_mut().activate_call_start();
                    this.log_service.trace(&format!("ExtensionService#_callActivateOptional {}", extension_id.value));
                    let activate_result = activate(context);
                    activation_times_builder.borrow_mut().activate_call_stop();

                    activation_times_builder.borrow_mut().activate_resolve_start();
                    let value = activate_result.await?;
                    activation_times_builder.borrow_mut().activate_resolve_stop();
                    value
                }
                // No activate found => the module is the extension's exports
                None => Dynamic::Undefined,
            };
            let runner = this.runner.clone();
            let table = this.disposables.clone();
            let disposable: DisposableRef = to_disposable(move || dispose_subscriptions(&runner, &table, &subscriptions));
            let times = activation_times_builder.borrow().build();
            Ok(Rc::new(ActivatedExtension::new(false, None, times, extension_module, Some(exports), disposable)))
        })
    }

    /// `_deactivateAll()`: o `deactivate` do módulo e o descartável de cada extensão ativada.
    pub fn deactivate_all(&self) {
        for extension in self.my_registry.get_all_extension_descriptions() {
            if let Ok(activated) = self.activator().get_activated_extension(&extension.identifier.value) {
                if let Some(deactivate) = &activated.module.deactivate {
                    deactivate();
                }
                activated.disposable.dispose();
            }
        }
    }

    // #endregion

    // #region Chamado pelo workbench ($métodos)

    /// `applyExtensionsDelta(...)` seguido do `set` nos dois registros.
    fn apply_extensions_delta(&self, extensions_delta: &IExtensionDescriptionDelta) {
        self.activation_events_reader.add_activation_events(&extensions_delta.add_activation_events);
        let reader: Rc<dyn IActivationEventsReader> = self.activation_events_reader.clone();
        let global_registry = ExtensionDescriptionRegistry::new(reader, self.global_registry.get_all_extension_descriptions());
        global_registry.delta_extensions(extensions_delta.to_add.clone(), &extensions_delta.to_remove);

        let mut my_extensions_set = ExtensionIdentifierSet::new(self.my_registry.get_all_extension_descriptions().iter().map(|extension| extension.identifier.value.clone()));
        for extension_id in &extensions_delta.my_to_remove {
            my_extensions_set.delete(&extension_id.value);
        }
        for extension_id in &extensions_delta.my_to_add {
            my_extensions_set.add(&extension_id.value);
        }
        let my_extensions = filter_extensions(&global_registry, &my_extensions_set);

        self.global_registry.set(global_registry.get_all_extension_descriptions());
        self.my_registry.set(my_extensions);
    }

    /// `$startExtensionHost(extensionsDelta)`.
    pub fn rpc_start_extension_host(&self, extensions_delta: IExtensionDescriptionDelta) -> Promise<()> {
        self.apply_extensions_delta(&extensions_delta);
        self.log_service.info(&format!("$startExtensionHost: global extensions: {}", print_ext_ids(&self.global_registry)));
        self.log_service.info(&format!("$startExtensionHost: local extensions: {}", print_ext_ids(&self.my_registry)));
        self.start_extension_host_impl()
    }

    /// `_startExtensionHost()`: lança (`Extension host is already started!`) se já iniciou.
    fn start_extension_host_impl(&self) -> Promise<()> {
        if self.started.get() {
            return Promise::rejected(Error::new("Extension host is already started!"));
        }
        self.started.set(true);

        let this = self.this();
        async_fn(async move {
            this.ready_to_start_extension_host.wait().await?;
            this.ready_to_run_extensions.open();
            // wait for all activation events that came in during workbench startup
            this.activator().wait_for_activating_extensions().await?;
            this.handle_eager_extensions().await?;
            this.eager_extensions_activated.open();
            this.log_service.info("Eager extensions activated");
            Ok(())
        })
    }

    /// `_handleEagerExtensions()`: `*` e, depois, `onStartupFinished`.
    fn handle_eager_extensions(&self) -> Promise<()> {
        let this = self.this();
        async_fn(async move {
            if let Err(err) = this.activate_by_event_impl("*", true).await {
                this.log_service.error_args(&je_platform::log::LogMessage::Error(err), &[]);
            }
            this.activate_all_startup_finished();
            Ok(())
        })
    }

    /// `_activateAllStartupFinished()`.
    fn activate_all_startup_finished(&self) {
        for desc in self.my_registry.get_all_extension_descriptions() {
            if desc.manifest.main.is_none() && desc.manifest.browser.is_none() {
                continue;
            }
            for activation_event in self.activation_events_reader.read_activation_events(&desc) {
                if activation_event == "onStartupFinished" {
                    let reason = ExtensionActivationReason { startup: false, extension_id: desc.identifier.clone(), activation_event };
                    match self.activator().activate_by_id(&desc.identifier, reason) {
                        Ok(promise) => {
                            let log_service = self.log_service.clone();
                            let _ = promise.then_result(move |outcome| {
                                if let Err(err) = outcome {
                                    log_service.error_args(&je_platform::log::LogMessage::Error(err), &[]);
                                }
                                Ok(())
                            });
                        }
                        Err(err) => self.log_service.error_args(&je_platform::log::LogMessage::Error(err), &[]),
                    }
                    break;
                }
            }
        }
    }

    /// `$activateByEvent(activationEvent, activationKind)`.
    pub fn rpc_activate_by_event(&self, activation_event: &str, activation_kind: ActivationKind) -> Promise<()> {
        let this = self.this();
        let activation_event = activation_event.to_owned();
        let barrier = if activation_kind == ActivationKind::Immediate { &self.almost_ready_to_run_extensions } else { &self.ready_to_run_extensions };
        barrier.wait().then_promise(move |_| this.activate_by_event_impl(&activation_event, false))
    }

    /// `$activate(extensionId, reason)`.
    pub fn rpc_activate(&self, extension_id: ExtensionIdentifier, reason: ExtensionActivationReason) -> Promise<bool> {
        let this = self.this();
        async_fn(async move {
            this.ready_to_run_extensions.wait().await?;
            if this.my_registry.get_extension_description(&extension_id.value).is_none() {
                // unknown extension => ignore
                return Ok(false);
            }
            this.activator().activate_by_id(&extension_id, reason)?.await?;
            Ok(true)
        })
    }

    /// `$deltaExtensions(extensionsDelta)`.
    pub fn rpc_delta_extensions(&self, extensions_delta: IExtensionDescriptionDelta) -> Promise<()> {
        self.apply_extensions_delta(&extensions_delta);
        self.log_service.info(&format!("$deltaExtensions: global extensions: {}", print_ext_ids(&self.global_registry)));
        self.log_service.info(&format!("$deltaExtensions: local extensions: {}", print_ext_ids(&self.my_registry)));
        Promise::resolved(())
    }

    /// `$extensionTestsExecute()`.
    pub fn rpc_extension_tests_execute(&self) -> Promise<i32> {
        let this = self.this();
        async_fn(async move {
            this.eager_extensions_activated.wait().await?;
            let error = Error::new("Cannot load test runner.");
            eprintln!("{}", error.message);
            Err(error)
        })
    }

    /// `_activateAndGetResolver(remoteAuthority)`: `Ok(prefixo)` depois de ativar o evento (o registro de resolvedores está sempre
    /// vazio, ver a nota do módulo); `Err` com o código quando a autoridade não pode ser resolvida.
    fn activate_and_get_resolver(&self, remote_authority: &str) -> Promise<String> {
        let this = self.this();
        let remote_authority = remote_authority.to_owned();
        async_fn(async move {
            let Some(authority_plus_index) = remote_authority.find('+') else {
                return Err(je_platform::remote::remote_authority_resolver::RemoteAuthorityResolverError::new(
                    Some("Not an authority that can be resolved!"),
                    RemoteAuthorityResolverErrorCode::InvalidAuthority,
                    false,
                ));
            };
            let authority_prefix = remote_authority[..authority_plus_index].to_owned();

            this.almost_ready_to_run_extensions.wait().await?;
            this.activate_by_event_impl(&format!("onResolveRemoteAuthority:{authority_prefix}"), false).await?;
            Ok(authority_prefix)
        })
    }

    /// `$resolveAuthority(remoteAuthorityChain, resolveAttempt)`.
    pub fn rpc_resolve_authority(&self, remote_authority: &str, resolve_attempt: u32) -> Promise<IResolveAuthorityResult> {
        let this = self.this();
        let remote_authority = remote_authority.to_owned();
        async_fn(async move {
            let sw = StopWatch::create(Some(false));
            let prefix = format!("[resolveAuthority({},{resolve_attempt})][{}ms] ", get_remote_authority_prefix(&remote_authority), sw.elapsed());
            this.log_service.info(&format!("{prefix}activating resolver for {remote_authority}..."));
            match this.activate_and_get_resolver(&remote_authority).await {
                Ok(authority_prefix) => {
                    this.log_service.error(&format!("{prefix}no resolver for {authority_prefix}"));
                    Ok(IResolveAuthorityResult::Error(ResolveAuthorityError {
                        message: Some(format!("No remote extension installed to resolve {authority_prefix}.")),
                        code: RemoteAuthorityResolverErrorCode::NoResolverFound,
                        detail: None,
                    }))
                }
                // `normalizeError`: só o `RemoteAuthorityResolverError` vira resultado; qualquer outro erro é lançado.
                Err(err) => match je_platform::remote::remote_authority_resolver::RemoteAuthorityResolverError::code(&err) {
                    Some(code) => Ok(IResolveAuthorityResult::Error(ResolveAuthorityError { message: Some(err.message.clone()), code, detail: None })),
                    None => Err(err),
                },
            }
        })
    }

    /// `$getCanonicalURI(remoteAuthority, uriComponents)`: `null` quando não há resolvedor.
    pub fn rpc_get_canonical_uri(&self, remote_authority: &str) -> Promise<Option<URI>> {
        self.log_service.info(&format!("$getCanonicalURI invoked for authority ({})", get_remote_authority_prefix(remote_authority)));
        self.activate_and_get_resolver(remote_authority).then(|_| None)
    }

    // #endregion
}

/// `dispose(context.subscriptions)`: descarta cada item, e um item que lança não impede os demais.
fn dispose_subscriptions(runner: &NodeRunner, table: &DisposableTable, subscriptions: &JsHandle) {
    let JsData::Array(items) = runner.export(subscriptions) else { return };
    for item in items {
        match &item {
            JsData::Handle(handle) => {
                let _ = runner.call_method(handle, "dispose", &[]);
            }
            other => {
                if let Some(JsData::Num(id)) = field(other, "_id") {
                    table.dispose(*id as u64);
                }
            }
        }
    }
}

/// O ator do `ExtHostExtensionService` no RPC: decodifica os argumentos de cada `$método`.
pub struct ExtHostExtensionServiceActor {
    service: Rc<ExtHostExtensionService>,
}

impl ExtHostExtensionServiceActor {
    pub fn new(service: Rc<ExtHostExtensionService>) -> Rc<Self> {
        Rc::new(Self { service })
    }
}

fn argument(args: &[Dynamic], index: usize) -> Dynamic {
    args.get(index).cloned().unwrap_or(Dynamic::Undefined)
}

fn text_argument(args: &[Dynamic], index: usize, method: &str) -> Result<String, Error> {
    match argument(args, index) {
        Dynamic::String(text) => Ok(text.to_string_lossy()),
        _ => Err(Error::type_error(format!("{method}: o argumento {index} precisa ser texto"))),
    }
}

fn number_argument(args: &[Dynamic], index: usize, method: &str) -> Result<f64, Error> {
    match argument(args, index) {
        Dynamic::Number(number) => Ok(number),
        _ => Err(Error::type_error(format!("{method}: o argumento {index} precisa ser número"))),
    }
}

fn void(promise: Promise<()>) -> MaybePromise<Dynamic> {
    MaybePromise::Promise(promise.then(|()| Dynamic::Undefined))
}

impl IRpcActor for ExtHostExtensionServiceActor {
    fn invoke(&self, method: &str, args: Vec<Dynamic>) -> Option<MaybePromise<Dynamic>> {
        let service = &self.service;
        let result: Result<MaybePromise<Dynamic>, Error> = match method {
            "$startExtensionHost" => IExtensionDescriptionDelta::from_dynamic(&argument(&args, 0)).map(|delta| void(service.rpc_start_extension_host(delta))),
            "$deltaExtensions" => IExtensionDescriptionDelta::from_dynamic(&argument(&args, 0)).map(|delta| void(service.rpc_delta_extensions(delta))),
            "$activateByEvent" => text_argument(&args, 0, method).and_then(|event| {
                let kind = if number_argument(&args, 1, method)? == 1.0 { ActivationKind::Immediate } else { ActivationKind::Normal };
                Ok(void(service.rpc_activate_by_event(&event, kind)))
            }),
            "$activate" => extension_identifier_from_dynamic(&argument(&args, 0)).and_then(|id| {
                let reason = extension_activation_reason_from_dynamic(&argument(&args, 1))?;
                Ok(MaybePromise::Promise(service.rpc_activate(id, reason).then(Dynamic::Bool)))
            }),
            "$extensionTestsExecute" => Ok(MaybePromise::Promise(service.rpc_extension_tests_execute().then(|code| Dynamic::Number(f64::from(code))))),
            "$setRemoteEnvironment" => environment_from_dynamic(&argument(&args, 0)).map(|_| MaybePromise::Value(Dynamic::Undefined)),
            "$updateRemoteConnectionData" => Ok(MaybePromise::Value(Dynamic::Undefined)),
            "$resolveAuthority" => text_argument(&args, 0, method).and_then(|authority| {
                let attempt = number_argument(&args, 1, method)? as u32;
                Ok(MaybePromise::Promise(service.rpc_resolve_authority(&authority, attempt).then(|result| resolve_authority_result_to_dynamic(&result))))
            }),
            "$getCanonicalURI" => text_argument(&args, 0, method).map(|authority| {
                MaybePromise::Promise(service.rpc_get_canonical_uri(&authority).then(|uri| uri.map_or(Dynamic::Null, |uri| uri.to_dynamic())))
            }),
            "$test_latency" => number_argument(&args, 0, method).map(|n| MaybePromise::Value(Dynamic::Number(n))),
            "$test_up" => Ok(MaybePromise::Value(Dynamic::Number(
                je_workbench::api::common::ext_host_values::bytes_of(&argument(&args, 0)).map_or(0.0, |bytes| bytes.len() as f64),
            ))),
            "$test_down" => number_argument(&args, 0, method).map(|size| {
                let buffer = VSBuffer::alloc(size as usize);
                MaybePromise::Value(je_workbench::api::common::ext_host_values::vs_buffer_to_dynamic(&buffer))
            }),
            _ => return None,
        };
        Some(match result {
            Ok(value) => value,
            Err(error) => MaybePromise::Promise(Promise::rejected(error)),
        })
    }
}
