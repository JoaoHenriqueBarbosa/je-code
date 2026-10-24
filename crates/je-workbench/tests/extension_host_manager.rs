//! Teste de integração (não é o teste de um arquivo do oráculo): o `AbstractExtensionService` com o host de extensões local REAL de
//! ponta a ponta. O serviço cria o `ExtensionHostManager` pelo corpo padrão de `_doCreateExtensionHostManager` (ou o
//! `LazyCreateExtensionHostManager`, para um host `LazyAutoStart`), o `LocalExtensionHost` sobe o host do `je-exthost` (zjsc com o
//! `vscode` montado em Rust) e o liga ao `RPCProtocol` do workbench por um `MessageChannel` em memória, e uma extensão CommonJS em
//! disco é ativada por `activateByEvent`: o `activate` dela roda no motor, registra um comando por `vscode.commands.registerCommand`
//! e o comando aparece no `CommandsRegistry` do workbench (atravessando a serialização do RPC nos dois sentidos).
//!
//! Notas:
//! - Tudo roda na thread do motor (`with_runtime_thread`): o motor, o RPC e o workbench usam `Rc` e microtarefas da mesma thread. O
//!   laço de eventos é o `pump`, que esvazia as microtarefas do motor e as das `Promise` do Rust até estabilizar.
//! - Os dublês de serviço são os de `support/extension_service_doubles.rs` (os mesmos de `extension_service.rs`).

#[path = "support/extension_service_doubles.rs"]
mod doubles;

use std::rc::Rc;

use doubles::{TestDialogService, TestRemoteAuthorityResolverService, TestWorkbenchExtensionEnablementService, TestWorkbenchExtensionManagementService};
use je_base::dynamic::Dynamic;
use je_base::lifecycle::DisposableStore;
use je_base::platform::Platform;
use je_base::promise::Promise;
use je_base::uri::URI;
use je_exthost::local_extension_host::{ILocalExtensionHostDataProvider, LocalExtensionHost};
use je_exthost::runtime::{NodeRunner, run_host_program, with_runtime_thread};
use je_extensions::manifest::{ExtensionIdentifier, ExtensionKind, ExtensionManifest, TargetPlatform};
use je_extensions::scanner::ExtensionDescription;
use je_instantiation::{InstantiationService, ServiceCollection};
use je_platform::commands::commands::{ICommandService, commands_registry};
use je_platform::configuration::test_configuration_service::TestConfigurationService;
use je_platform::log::{LogServiceRef, NullLogService};
use je_platform::product::ProductService;
use je_platform::remote::remote_authority_resolver::ResolverResult;
use je_platform::telemetry::telemetry_utils::NullTelemetryServiceShape;
use je_workbench::api::browser::main_thread_commands::promise_from_dynamic;
use je_workbench::api::browser::main_thread_customers::{register_main_thread_commands, register_main_thread_telemetry_bridge};
use je_workbench::api::browser::main_thread_extension_service::register_main_thread_extension_service;
use je_workbench::services::commands::command_service::CommandService;
use je_workbench::services::extensions::abstract_extension_service::{
    AbstractExtensionService, AbstractExtensionServiceHooks, AbstractExtensionServiceOptions, AbstractExtensionServiceServices, IExtensionHostFactory,
    LocalExtensions, ResolvedExtensions, ResolvedExtensionsQueue, ResolvedExtensionsStream,
};
use je_workbench::services::extensions::extension_host_kind::{ExtensionHostKind, ExtensionRunningPreference, IExtensionHostKindPicker};
use je_workbench::services::extensions::extension_manifest_properties_service::ExtensionManifestPropertiesService;
use je_workbench::services::extensions::extension_running_location::ExtensionRunningLocation;
use je_workbench::services::extensions::extension_running_location_tracker::ExtensionRunningLocationTracker;
use je_workbench::services::extensions::extensions::{ActivationKind, ExtensionHostExtensions, ExtensionHostStartup, IExtensionHost, IExtensionService};
use je_workbench::services::extensions::extensions_proposed_api::ExtensionsProposedApi;
use je_workbench::test::browser::workbench_test_services::{TestEnvironmentService, TestRemoteAgentService};
use je_workbench::test::common::workbench_test_services::{
    TestContextService, TestFileService, TestLifecycleService, TestNotificationService, TestWorkspaceTrustEnablementService,
};

/// O `main` da extensão de exemplo: registra `exthost.hello` na ativação e marca o `deactivate`.
const EXTENSION_SOURCE: &str = r#"
const vscode = require('vscode');
exports.activate = (context) => {
  globalThis.__activations = (globalThis.__activations || 0) + 1;
  context.subscriptions.push(vscode.commands.registerCommand('exthost.hello', (name) => vscode.l10n.t('Olá, {0}!', name)));
};
exports.deactivate = () => { globalThis.__deactivated = true; };
"#;

/// O dublê do `IExtensionHostKindPicker`: toda extensão roda no processo local.
struct AlwaysLocalProcessPicker;

impl IExtensionHostKindPicker for AlwaysLocalProcessPicker {
    fn pick_extension_host_kind(
        &self,
        _extension_id: &str,
        _extension_kinds: &[ExtensionKind],
        _is_installed_locally: bool,
        _is_installed_remotely: bool,
        _preference: ExtensionRunningPreference,
    ) -> Option<ExtensionHostKind> {
        Some(ExtensionHostKind::LocalProcess)
    }
}

/// O `initData.extensions` fixo que o host recebe ao subir.
struct StaticDataProvider {
    extensions: Rc<ExtensionHostExtensions>,
}

impl ILocalExtensionHostDataProvider for StaticDataProvider {
    fn get_init_data(&self) -> Promise<Rc<ExtensionHostExtensions>> {
        Promise::resolved(self.extensions.clone())
    }
}

/// A fábrica que cria o `LocalExtensionHost` para o processo local e nenhum host para os demais lugares.
struct LocalHostFactory {
    runner: NodeRunner,
    startup: ExtensionHostStartup,
    data_provider: Rc<StaticDataProvider>,
    log_service: LogServiceRef,
}

impl IExtensionHostFactory for LocalHostFactory {
    fn create_extension_host(&self, _running_locations: &ExtensionRunningLocationTracker, running_location: ExtensionRunningLocation, _is_initial_start: bool) -> Option<Rc<dyn IExtensionHost>> {
        match running_location {
            ExtensionRunningLocation::LocalProcess { .. } => {
                Some(LocalExtensionHost::new(self.runner.clone(), running_location, self.startup, self.data_provider.clone(), self.log_service.clone()))
            }
            _ => None,
        }
    }
}

/// O serviço de extensões do teste: entrega a extensão de exemplo como extensão local e NÃO sobrescreve
/// `do_create_extension_host_manager` (é o corpo padrão que está sob teste).
struct ExampleExtensionService {
    description: Rc<ExtensionDescription>,
}

impl AbstractExtensionServiceHooks for ExampleExtensionService {
    fn resolve_extensions(&self) -> Rc<dyn ResolvedExtensionsStream> {
        ResolvedExtensionsQueue::new(vec![ResolvedExtensions::Local(LocalExtensions { extensions: vec![self.description.clone()] })])
    }

    fn on_extension_host_exit(&self, _code: i32) -> Promise<()> {
        panic!("o host local não deve sair neste teste")
    }

    fn resolve_authority(&self, _remote_authority: &str) -> Promise<ResolverResult> {
        panic!("o teste não resolve autoridade remota")
    }
}

fn extension_description(location: &str) -> ExtensionDescription {
    ExtensionDescription {
        id: "je-test.hello".to_owned(),
        identifier: ExtensionIdentifier::new("je-test.hello"),
        is_builtin: false,
        is_user_builtin: false,
        is_under_development: false,
        extension_location: location.to_owned(),
        uuid: None,
        target_platform: TargetPlatform::Undefined,
        publisher_display_name: None,
        pre_release: false,
        manifest: ExtensionManifest {
            name: "hello".to_owned(),
            publisher: "je-test".to_owned(),
            version: "1.0.0".to_owned(),
            main: Some("./extension.js".to_owned()),
            activation_events: Some(vec!["onTest".to_owned()]),
            ..ExtensionManifest::default()
        },
    }
}

/// Esvazia as microtarefas do motor e as das `Promise` do Rust até estabilizar (o laço de eventos do teste).
fn pump(runner: &NodeRunner) {
    for _ in 0..64 {
        runner.drain_microtasks();
        je_base::runtime::run_microtasks();
    }
}

/// O valor de uma promessa que já deve ter assentado com sucesso.
fn settled_ok<T: Clone + 'static>(promise: &Promise<T>, what: &str) -> T {
    match promise.result() {
        Some(Ok(value)) => value,
        Some(Err(error)) => panic!("{what} rejeitou: {error:?}"),
        None => panic!("{what} não assentou"),
    }
}

fn probe(runner: &NodeRunner, source: &str) -> String {
    runner.eval(source, "probe.js").expect("a sonda avalia").as_text().expect("a sonda devolve texto")
}

/// O cenário completo, para o gerenciador e o host que `startup` escolhe. `init_with_extension` põe a extensão no `initData` do host
/// (o caso `LazyAutoStart`, em que o `ExtensionHostManager.start` não é chamado); senão ela chega pelo `$startExtensionHost`.
fn scenario(runner: &NodeRunner, startup: ExtensionHostStartup, init_with_extension: bool) {
    let dir = tempfile::tempdir().expect("diretório temporário");
    std::fs::write(dir.path().join("extension.js"), EXTENSION_SOURCE).expect("grava a extensão");
    let location = URI::file(dir.path().to_string_lossy().into_owned(), Platform::detect()).to_string(false).to_string_lossy();
    let description = Rc::new(extension_description(&location));

    let initial_extensions = if init_with_extension {
        ExtensionHostExtensions::new(0, std::slice::from_ref(&description), &[description.identifier.clone()])
    } else {
        ExtensionHostExtensions::new(0, &[], &[])
    };

    // Os serviços, como o `Fixture::new` de `extension_service.rs`.
    let disposables = DisposableStore::new();
    let log_service = NullLogService::shared();
    let product_service = ProductService::default();
    let environment_service = TestEnvironmentService::new();
    let configuration_service = TestConfigurationService::new(None);
    let extensions_proposed_api = Rc::new(ExtensionsProposedApi::new(log_service.clone(), environment_service.clone(), &product_service));
    let extension_manifest_properties_service = disposables.add_rc(ExtensionManifestPropertiesService::new(
        Rc::new(product_service),
        configuration_service.clone(),
        TestWorkspaceTrustEnablementService::new(true),
        log_service.clone(),
    ));
    let instantiation_service = disposables.add_rc(InstantiationService::new(ServiceCollection::new()));

    let factory = LocalHostFactory {
        runner: runner.clone(),
        startup,
        data_provider: Rc::new(StaticDataProvider { extensions: initial_extensions }),
        log_service: log_service.clone(),
    };
    let base = disposables.add_rc(AbstractExtensionService::new(
        AbstractExtensionServiceOptions { allow_remote_extensions_in_local_web_worker: false, has_local_process: true },
        extensions_proposed_api,
        Rc::new(factory),
        Rc::new(AlwaysLocalProcessPicker),
        AbstractExtensionServiceServices {
            instantiation_service: instantiation_service.clone(),
            notification_service: Rc::new(TestNotificationService),
            environment_service,
            telemetry_service: Rc::new(NullTelemetryServiceShape),
            extension_enablement_service: Rc::new(TestWorkbenchExtensionEnablementService),
            file_service: {
                let concrete = disposables.add_rc(TestFileService::new());
                concrete
            },
            extension_management_service: Rc::new(TestWorkbenchExtensionManagementService),
            context_service: TestContextService::new(None, None),
            configuration_service,
            extension_manifest_properties_service,
            log_service: log_service.clone(),
            remote_agent_service: Rc::new(TestRemoteAgentService),
            lifecycle_service: {
                let concrete = disposables.add_rc(TestLifecycleService::new());
                concrete
            },
            remote_authority_resolver_service: Rc::new(TestRemoteAuthorityResolverService),
            dialog_service: Rc::new(TestDialogService),
        },
    ));
    let hooks = Rc::new(ExampleExtensionService { description: description.clone() });
    let hooks_handle: Rc<dyn AbstractExtensionServiceHooks> = hooks.clone();
    base.set_hooks(Rc::downgrade(&hooks_handle));

    // Os clientes `MainThread*` que o gerenciador instancia para cada host (`@extHostNamedCustomer`).
    let extension_service: Rc<dyn IExtensionService> = base.clone();
    let command_service = CommandService::new(instantiation_service.clone(), extension_service.clone(), log_service.clone());
    register_main_thread_extension_service(extension_service.clone());
    register_main_thread_commands(command_service.clone(), extension_service);
    register_main_thread_telemetry_bridge(Rc::new(NullTelemetryServiceShape));

    // Inicializa: resolve as extensões, cria o gerenciador e o host e entrega as extensões ao host.
    let initialized = base.initialize_if_needed();
    pump(runner);
    settled_ok(&initialized, "initialize");

    // Nenhuma ativação ainda: o comando só existe depois do `activate` da extensão.
    assert!(commands_registry().get_command("exthost.hello").is_none(), "a extensão não deveria estar ativa antes do evento");
    assert_eq!(probe(runner, "String(globalThis.__activations)"), "undefined");

    // `activateByEvent` atravessa o gerenciador, o RPC, o `ExtHostExtensionService` e o motor.
    let activation = base.activate_by_event("onTest", Some(ActivationKind::Normal));
    pump(runner);
    settled_ok(&activation, "activateByEvent");
    assert_eq!(probe(runner, "String(globalThis.__activations)"), "1", "o activate da extensão deveria ter rodado uma vez");
    assert!(base.activation_event_is_done("onTest"), "o evento deveria constar como concluído");

    // O comando que a extensão registrou no host aparece no `CommandsRegistry` do workbench.
    assert!(commands_registry().get_command("exthost.hello").is_some(), "exthost.hello deveria estar registrado no workbench");

    // E executá-lo no workbench roda o código da extensão no motor e devolve o resultado pelo RPC.
    let executed = command_service.execute_command("exthost.hello", vec![Dynamic::string("mundo")]);
    pump(runner);
    let wrapped = settled_ok(&executed, "executeCommand");
    let result = promise_from_dynamic(&wrapped).expect("o handler devolve o resultado como promessa embrulhada");
    pump(runner);
    assert_eq!(settled_ok(&result, "o comando da extensão"), Dynamic::string("Olá, mundo!"));

    // Um segundo evento igual não reativa (o ativador e o gerenciador guardam o resultado).
    let again = base.activate_by_event("onTest", Some(ActivationKind::Normal));
    pump(runner);
    settled_ok(&again, "activateByEvent (repetido)");
    assert_eq!(probe(runner, "String(globalThis.__activations)"), "1");

    // Parar os hosts desativa a extensão (`deactivate`) e descarta os clientes, que desregistram o comando no workbench.
    let stopped = base.stop_extension_hosts("fim do teste", false);
    pump(runner);
    assert!(settled_ok(&stopped, "stopExtensionHosts"), "o host deveria ter parado");
    assert_eq!(probe(runner, "String(globalThis.__deactivated)"), "true", "o deactivate da extensão deveria ter rodado");
    assert!(commands_registry().get_command("exthost.hello").is_none(), "o comando deveria ter sido desregistrado com o host");

    // `hooks_handle` (e o `hooks` dentro dele) precisa viver até aqui: o serviço só guarda um `Weak`.
    drop(hooks_handle);
    disposables.dispose();
}

fn run(startup: ExtensionHostStartup, init_with_extension: bool) {
    with_runtime_thread(move || {
        run_host_program(move |runner| scenario(runner, startup, init_with_extension)).expect("o programa do host roda");
    });
}

#[test]
fn eager_manual_start_host_activates_extension_and_registers_command_in_workbench() {
    run(ExtensionHostStartup::EagerManualStart, false);
}

#[test]
fn lazy_auto_start_host_activates_extension_and_registers_command_in_workbench() {
    run(ExtensionHostStartup::LazyAutoStart, true);
}

#[test]
fn eager_auto_start_host_activates_extension_and_registers_command_in_workbench() {
    run(ExtensionHostStartup::EagerAutoStart, true);
}
