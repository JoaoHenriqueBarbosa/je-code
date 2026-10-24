//! A bancada dos testes de ponta a ponta da API `vscode` do host de extensões (`extension_host_api_*.rs`): o `AbstractExtensionService` do
//! workbench com o `LocalExtensionHost` real, uma extensão de exemplo em JavaScript ativada por `activateByEvent`, e o lado do
//! workbench dos namespaces que o teste liga por um `IMainThreadBinder` (os clientes `MainThread*` reais sobre dublês de serviço).
//!
//! É a mesma montagem de `extension_host_manager.rs`, com o que muda por teste (o código da extensão e o binder) entrando como
//! argumento de [`run_extension`]. Tudo roda na thread do motor (`with_runtime_thread`): o motor, o RPC e o workbench usam `Rc`.

#![allow(dead_code)]

#[path = "extension_service_doubles.rs"]
pub mod doubles;

use std::rc::Rc;

use doubles::{TestDialogService, TestRemoteAuthorityResolverService, TestWorkbenchExtensionEnablementService, TestWorkbenchExtensionManagementService};
use je_base::dynamic::Dynamic;
use je_base::lifecycle::{DisposableStore, IDisposable};
use je_base::promise::Promise;
use je_base::uri::URI;
use je_exthost::in_process::IMainThreadBinder;
use je_exthost::local_extension_host::{ILocalExtensionHostDataProvider, LocalExtensionHost};
use je_exthost::runtime::{run_host_program, with_runtime_thread, NodeRunner};
use je_extensions::manifest::{ExtensionContributions, ExtensionIdentifier, ExtensionKind, ExtensionManifest, TargetPlatform};
use je_extensions::scanner::ExtensionDescription;
use je_instantiation::{InstantiationService, ServiceCollection};
use je_platform::commands::commands::ICommandService;
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
    AbstractExtensionService, AbstractExtensionServiceHooks, AbstractExtensionServiceOptions, AbstractExtensionServiceServices, IExtensionHostFactory, LocalExtensions,
    ResolvedExtensions, ResolvedExtensionsQueue, ResolvedExtensionsStream,
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

/// O `IExtensionHostKindPicker` em que toda extensão roda no processo local.
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

/// A fábrica que cria o `LocalExtensionHost` (com o `binder` do teste) para o processo local e nenhum host para os demais lugares.
struct LocalHostFactory {
    runner: NodeRunner,
    startup: ExtensionHostStartup,
    data_provider: Rc<StaticDataProvider>,
    log_service: LogServiceRef,
    binder: std::cell::RefCell<Option<Rc<dyn IMainThreadBinder>>>,
}

impl IExtensionHostFactory for LocalHostFactory {
    fn create_extension_host(&self, _running_locations: &ExtensionRunningLocationTracker, running_location: ExtensionRunningLocation, _is_initial_start: bool) -> Option<Rc<dyn IExtensionHost>> {
        match running_location {
            ExtensionRunningLocation::LocalProcess { .. } => {
                let host = LocalExtensionHost::new(self.runner.clone(), running_location, self.startup, self.data_provider.clone(), self.log_service.clone());
                if let Some(binder) = self.binder.borrow().clone() {
                    host.set_main_thread_binder(binder);
                }
                Some(host)
            }
            _ => None,
        }
    }
}

/// O serviço de extensões do teste: entrega a extensão de exemplo como extensão local.
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

fn extension_description(location: &str, contributes: Option<ExtensionContributions>) -> ExtensionDescription {
    ExtensionDescription {
        id: "je-test.api".to_owned(),
        identifier: ExtensionIdentifier::new("je-test.api"),
        is_builtin: false,
        is_user_builtin: false,
        is_under_development: false,
        extension_location: location.to_owned(),
        uuid: None,
        target_platform: TargetPlatform::Undefined,
        publisher_display_name: None,
        pre_release: false,
        manifest: ExtensionManifest {
            name: "api".to_owned(),
            publisher: "je-test".to_owned(),
            version: "1.0.0".to_owned(),
            main: Some("./extension.js".to_owned()),
            activation_events: Some(vec!["onTest".to_owned()]),
            contributes,
            ..ExtensionManifest::default()
        },
    }
}

/// O valor de uma promessa que já deve ter assentado com sucesso.
pub fn settled_ok<T: Clone + 'static>(promise: &Promise<T>, what: &str) -> T {
    match promise.result() {
        Some(Ok(value)) => value,
        Some(Err(error)) => panic!("{what} rejeitou: {error:?}"),
        None => panic!("{what} não assentou"),
    }
}

/// O que o teste enxerga do workbench montado.
pub struct Workbench {
    pub runner: NodeRunner,
    pub extension_service: Rc<dyn IExtensionService>,
    pub command_service: Rc<dyn ICommandService>,
    pub log_service: LogServiceRef,
}

impl Workbench {
    /// Esvazia as microtarefas do motor, as das `Promise` do Rust e os timers vencidos (o laço de eventos do teste).
    pub fn pump(&self) {
        for _ in 0..64 {
            self.runner.drain_microtasks();
            je_base::runtime::turn();
        }
    }

    /// Avalia `source` no motor e devolve o texto resultante.
    pub fn probe(&self, source: &str) -> String {
        self.runner.eval(source, "probe.js").expect("a sonda avalia").as_text().expect("a sonda devolve texto")
    }

    /// Executa o comando `id` pelo `CommandService` do workbench (que atravessa o RPC até o código da extensão) e devolve o
    /// resultado, depois de esvaziar o laço de eventos. Um comando que rejeita derruba o teste com a mensagem do erro.
    pub fn execute(&self, id: &str, args: Vec<Dynamic>) -> Dynamic {
        let executed = self.command_service.execute_command(id, args);
        self.pump();
        let wrapped = settled_ok(&executed, "executeCommand");
        let result = promise_from_dynamic(&wrapped).expect("o handler devolve o resultado como promessa embrulhada");
        self.pump();
        settled_ok(&result, id)
    }
}

/// Sobe o workbench com o host local, ativa a extensão de exemplo (`source` é o `extension.js` dela) por `activateByEvent('onTest')`,
/// roda `check` com o estado que `setup` criou e para os hosts. `setup` recebe o workbench já montado (sem o host iniciado) e devolve
/// o binder que liga os clientes `MainThread*` e o que o `check` precisa observar depois.
pub fn run_extension<State: 'static>(
    source: &'static str,
    setup: impl FnOnce(&Workbench) -> (Rc<dyn IMainThreadBinder>, State) + Send + 'static,
    check: impl FnOnce(&Workbench, State) + Send + 'static,
) {
    run_extension_contributing(source, None, setup, check);
}

/// Como [`run_extension`], com o `contributes` do manifesto da extensão de exemplo (por exemplo, o depurador que a extensão define).
pub fn run_extension_contributing<State: 'static>(
    source: &'static str,
    contributes: Option<ExtensionContributions>,
    setup: impl FnOnce(&Workbench) -> (Rc<dyn IMainThreadBinder>, State) + Send + 'static,
    check: impl FnOnce(&Workbench, State) + Send + 'static,
) {
    with_runtime_thread(move || {
        run_host_program(move |runner| {
            let dir = tempfile::tempdir().expect("diretório temporário");
            std::fs::write(dir.path().join("extension.js"), source).expect("grava a extensão");
            let location = URI::file(dir.path().to_string_lossy().into_owned(), je_platform::resources::host_platform()).to_string(false).to_string_lossy();
            let description = Rc::new(extension_description(&location, contributes));

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

            let factory = Rc::new(LocalHostFactory {
                runner: runner.clone(),
                startup: ExtensionHostStartup::EagerManualStart,
                data_provider: Rc::new(StaticDataProvider { extensions: ExtensionHostExtensions::new(0, &[], &[]) }),
                log_service: log_service.clone(),
                binder: std::cell::RefCell::new(None),
            });
            let base = disposables.add_rc(AbstractExtensionService::new(
                AbstractExtensionServiceOptions { allow_remote_extensions_in_local_web_worker: false, has_local_process: true },
                extensions_proposed_api,
                factory.clone(),
                Rc::new(AlwaysLocalProcessPicker),
                AbstractExtensionServiceServices {
                    instantiation_service: instantiation_service.clone(),
                    notification_service: Rc::new(TestNotificationService),
                    environment_service,
                    telemetry_service: Rc::new(NullTelemetryServiceShape),
                    extension_enablement_service: Rc::new(TestWorkbenchExtensionEnablementService),
                    file_service: { let concrete = disposables.add_rc(TestFileService::new()); concrete },
                    extension_management_service: Rc::new(TestWorkbenchExtensionManagementService),
                    context_service: TestContextService::new(None, None),
                    configuration_service,
                    extension_manifest_properties_service,
                    log_service: log_service.clone(),
                    remote_agent_service: Rc::new(TestRemoteAgentService),
                    lifecycle_service: { let concrete = disposables.add_rc(TestLifecycleService::new()); concrete },
                    remote_authority_resolver_service: Rc::new(TestRemoteAuthorityResolverService),
                    dialog_service: Rc::new(TestDialogService),
                },
            ));
            let hooks = Rc::new(ExampleExtensionService { description: description.clone() });
            let hooks_handle: Rc<dyn AbstractExtensionServiceHooks> = hooks.clone();
            base.set_hooks(Rc::downgrade(&hooks_handle));

            let extension_service: Rc<dyn IExtensionService> = base.clone();
            let command_service = CommandService::new(instantiation_service.clone(), extension_service.clone(), log_service.clone());
            register_main_thread_extension_service(extension_service.clone());
            register_main_thread_commands(command_service.clone(), extension_service.clone());
            register_main_thread_telemetry_bridge(Rc::new(NullTelemetryServiceShape));

            let workbench = Workbench { runner: runner.clone(), extension_service, command_service, log_service: log_service.clone() };
            let (binder, state) = setup(&workbench);
            *factory.binder.borrow_mut() = Some(binder);

            let initialized = base.initialize_if_needed();
            workbench.pump();
            settled_ok(&initialized, "initialize");

            let activation = base.activate_by_event("onTest", Some(ActivationKind::Normal));
            workbench.pump();
            settled_ok(&activation, "activateByEvent");
            assert_eq!(workbench.probe("String(globalThis.__activated)"), "true", "o activate da extensão deveria ter rodado");

            check(&workbench, state);

            let stopped = base.stop_extension_hosts("fim do teste", false);
            workbench.pump();
            assert!(settled_ok(&stopped, "stopExtensionHosts"), "o host deveria ter parado");
            drop(hooks_handle);
            disposables.dispose();
        })
        .expect("o programa do host roda");
    });
}
