//! Porte de `src/vs/workbench/services/extensions/test/browser/extensionService.test.ts` (suítes `BrowserExtensionService` e
//! `ExtensionService`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()`: na suíte `BrowserExtensionService` o caso não cria nenhum descartável, então não há
//!   o que o guarda conferir. Na suíte `ExtensionService` cada caso monta o seu [`Fixture`] (o `setup`), cujo [`LeakGuard`] faz o
//!   papel do `disposables` e do guarda: o serviço e os serviços descartáveis entram nele, e o `teardown` (`disposables.dispose()`)
//!   é o `Drop` do guarda, depois das asserções.
//! - Cada `assert.deepStrictEqual(BrowserExtensionHostKindPicker.pickRunningLocation(kinds, local, remote, None), esperado)`
//!   é uma linha de [`check_row`], que confere as quatro combinações de `(isInstalledLocally, isInstalledRemotely)` na ordem do
//!   original (`false,false`, `false,true`, `true,false`, `true,true`), uma linha por lista de tipos, na ordem do original.
//! - `createServices(disposables, [[IExtensionService, MyTestExtensionService], ...])` é a montagem manual do [`Fixture::new`]. Os
//!   serviços de teste que o porte já tem (`TestLifecycleService`, `TestNotificationService`, `TestRemoteAgentService`,
//!   `TestConfigurationService`, `TestContextService`, `TestFileService`, `TestEnvironmentService`, `NullTelemetryService`,
//!   `NullLogService`, `ExtensionManifestPropertiesService`) entram como estão. `TestWorkbenchExtensionEnablementService`,
//!   `TestWorkbenchExtensionManagementService` (os dublês de `workbenchTestServices.ts`), `TestDialogService` e o dublê do
//!   `RemoteAuthorityResolverService` do navegador (que o porte não tem e que só o `_resolveAuthorityAgain` lê, que os casos não
//!   chamam) são definidos aqui. O `WorkspaceTrustEnablementService` real é o `TestWorkspaceTrustEnablementService` (o
//!   `ExtensionManifestPropertiesService` só o lê para as propriedades de manifesto, que nenhum caso consulta). Os serviços que o
//!   `AbstractExtensionService` do porte não lê (`IProductService`, `IRemoteExtensionsScannerService`, `IWebExtensionsScannerService`,
//!   `IUserDataProfilesService`, `IUserDataProfileService`, `IUriIdentityService`) não são montados.
//! - `null!` (o `IExtensionHostKindPicker` que o serviço de teste passa ao `super`) é o [`NullExtensionHostKindPicker`], que lança
//!   se alguém o chamar, como o acesso a uma propriedade de `null`.
//! - `mock<IExtensionHost>()` com `runningLocation` e `mock<IExtensionHostManager>()` com os membros do caso: o membro que o `mock` do
//!   original não define é `undefined`, e chamá-lo lança. Aqui o dublê o implementa com um pânico (`not_mocked`). A exceção é
//!   `startup` do gerenciador, que é `undefined` no `mock` (diferente de `EagerAutoStart`): o dublê devolve `EagerManualStart`, que
//!   leva o serviço ao mesmo caminho (chamar `start`).
//! - `_pickExtensionHostKind` e `_scanSingleExtension` da classe de teste não sobrescrevem nenhum membro do `AbstractExtensionService`
//!   (o original os deixa por herança antiga), então não existem aqui.
//! - Os casos `async` rodam em `spawn_local` com as microtarefas esvaziadas ([`run`]).

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use std::cell::{Cell, RefCell};
use std::future::Future;
use std::rc::{Rc, Weak};

use common::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite};
use je_base::errors::Error;
use je_base::event::Event;
use je_base::lifecycle::IDisposable;
use je_base::parts::ipc::IMessagePassingProtocol;
use je_base::promise::Promise;
use je_base::runtime;
use je_base::uri::URI;
use je_extensions::management::ExtensionsControlManifest;
use je_extensions::manifest::ExtensionKind::{Ui, Web, Workspace};
use je_extensions::manifest::ExtensionKind;
use je_instantiation::{InstantiationService, ServiceCollection};
use je_platform::configuration::test_configuration_service::TestConfigurationService;
use je_platform::dialogs::{Confirmation, ConfirmationResult, IDialogService, Input, InputResult, Prompt, PromptResult};
use je_platform::log::NullLogService;
use je_platform::product::ProductService;
use je_platform::remote::remote_authority_resolver::{IRemoteAuthorityResolverService, ResolverResult};
use je_platform::telemetry::telemetry_utils::NullTelemetryServiceShape;
use je_workbench::services::extension_management::extension_management::{
    DidChangeProfileEvent, DidUninstallExtensionEvent, EnablementState, ExtensionRef, IExtension, IExtensionManagementService, InstallExtensionResult,
    IProfileAwareExtensionManagementService, IWorkbenchExtensionEnablementService, IWorkbenchExtensionManagementService, WorkspaceTypeOverrides,
};
use je_workbench::services::extensions::abstract_extension_service::{
    AbstractExtensionService, AbstractExtensionServiceHooks, AbstractExtensionServiceOptions, AbstractExtensionServiceServices, IExtensionHostFactory,
    ResolvedExtensionsQueue, ResolvedExtensionsStream,
};
use je_workbench::services::extensions::browser::extension_service::BrowserExtensionHostKindPicker;
use je_workbench::services::extensions::extension_activation::ExtensionActivationReason;
use je_workbench::services::extensions::extension_host_kind::{ExtensionHostKind, ExtensionRunningPreference, IExtensionHostKindPicker};
use je_workbench::services::extensions::extension_host_managers::IExtensionHostManager;
use je_workbench::services::extensions::extension_host_protocol::IExtensionDescriptionDelta;
use je_workbench::services::extensions::extension_host_proxy::IResolveAuthorityResult;
use je_workbench::services::extensions::extension_manifest_properties_service::ExtensionManifestPropertiesService;
use je_workbench::services::extensions::extension_running_location::ExtensionRunningLocation;
use je_workbench::services::extensions::extension_running_location_tracker::{ExtensionRunningLocationTracker, IRepresentsRunningLocation};
use je_workbench::services::extensions::extensions::{
    ActivationKind, ExtensionHostStartup, IExtensionHost, IExtensionInspectInfo, IExtensionService, IWillActivateEvent,
};
use je_workbench::services::extensions::extensions_proposed_api::ExtensionsProposedApi;
use je_workbench::services::extensions::rpc_protocol::ResponsiveState;
use je_workbench::test::browser::workbench_test_services::{TestEnvironmentService, TestRemoteAgentService};
use je_workbench::test::common::workbench_test_services::{
    TestContextService, TestFileService, TestLifecycleService, TestNotificationService, TestWorkspaceTrustEnablementService,
};

/// As quatro combinações de `(isInstalledLocally, isInstalledRemotely)` do original, na ordem delas.
const COMBINATIONS: [(bool, bool); 4] = [(false, false), (false, true), (true, false), (true, true)];

/// `pickRunningLocation(kinds, local, remote, ExtensionRunningPreference.None)` nas quatro combinações.
fn check_row(kinds: &[ExtensionKind], expected: [Option<ExtensionHostKind>; 4]) {
    for (index, (is_installed_locally, is_installed_remotely)) in COMBINATIONS.into_iter().enumerate() {
        assert_eq!(
            BrowserExtensionHostKindPicker::pick_running_location(kinds, is_installed_locally, is_installed_remotely, ExtensionRunningPreference::None),
            expected[index],
            "kinds: {kinds:?}, isInstalledLocally: {is_installed_locally}, isInstalledRemotely: {is_installed_remotely}"
        );
    }
}

#[test]
fn pick_running_location() {
    const REMOTE: Option<ExtensionHostKind> = Some(ExtensionHostKind::Remote);
    const LOCAL: Option<ExtensionHostKind> = Some(ExtensionHostKind::LocalWebWorker);
    const NONE: Option<ExtensionHostKind> = None;

    check_row(&[], [NONE, NONE, NONE, NONE]);

    check_row(&[Ui], [NONE, REMOTE, NONE, REMOTE]);

    check_row(&[Workspace], [NONE, REMOTE, NONE, REMOTE]);

    check_row(&[Web], [NONE, LOCAL, LOCAL, LOCAL]);

    check_row(&[Ui, Workspace], [NONE, REMOTE, NONE, REMOTE]);
    check_row(&[Workspace, Ui], [NONE, REMOTE, NONE, REMOTE]);

    check_row(&[Web, Workspace], [NONE, LOCAL, LOCAL, LOCAL]);
    check_row(&[Workspace, Web], [NONE, REMOTE, LOCAL, REMOTE]);

    check_row(&[Ui, Web], [NONE, LOCAL, LOCAL, LOCAL]);
    check_row(&[Web, Ui], [NONE, LOCAL, LOCAL, LOCAL]);

    check_row(&[Ui, Web, Workspace], [NONE, LOCAL, LOCAL, LOCAL]);
    check_row(&[Ui, Workspace, Web], [NONE, REMOTE, LOCAL, REMOTE]);

    check_row(&[Web, Ui, Workspace], [NONE, LOCAL, LOCAL, LOCAL]);
    check_row(&[Web, Workspace, Ui], [NONE, LOCAL, LOCAL, LOCAL]);

    check_row(&[Workspace, Ui, Web], [NONE, REMOTE, LOCAL, REMOTE]);
    check_row(&[Workspace, Web, Ui], [NONE, REMOTE, LOCAL, REMOTE]);
}

/// O membro que o `mock<T>()` do original não define: `undefined`, e chamá-lo lança.
fn not_mocked(member: &str) -> ! {
    panic!("{member} não é definido pelo mock do original")
}

/// `new class implements IExtensionHostFactory { createExtensionHost(...) { return new class extends mock<IExtensionHost>() { override runningLocation = runningLocation }; } }`.
struct TestExtensionHostFactory;

struct TestExtensionHost {
    running_location: ExtensionRunningLocation,
}

impl IExtensionHostFactory for TestExtensionHostFactory {
    fn create_extension_host(&self, _running_locations: &ExtensionRunningLocationTracker, running_location: ExtensionRunningLocation, _is_initial_start: bool) -> Option<Rc<dyn IExtensionHost>> {
        Some(Rc::new(TestExtensionHost { running_location }))
    }
}

impl IExtensionHost for TestExtensionHost {
    fn pid(&self) -> Option<u32> {
        not_mocked("pid")
    }

    fn running_location(&self) -> ExtensionRunningLocation {
        self.running_location
    }

    fn remote_authority(&self) -> Option<String> {
        not_mocked("remoteAuthority")
    }

    fn startup(&self) -> ExtensionHostStartup {
        not_mocked("startup")
    }

    fn on_exit(&self) -> Event<(i32, Option<String>)> {
        not_mocked("onExit")
    }

    fn start(&self) -> Promise<Rc<dyn IMessagePassingProtocol>> {
        not_mocked("start")
    }

    fn get_inspect_port(&self) -> Option<IExtensionInspectInfo> {
        not_mocked("getInspectPort")
    }

    fn enable_inspect_port(&self) -> Promise<bool> {
        not_mocked("enableInspectPort")
    }

    fn dispose(&self) {
        not_mocked("dispose")
    }
}

/// `null!`.
struct NullExtensionHostKindPicker;

impl IExtensionHostKindPicker for NullExtensionHostKindPicker {
    fn pick_extension_host_kind(
        &self,
        _extension_id: &str,
        _extension_kinds: &[ExtensionKind],
        _is_installed_locally: bool,
        _is_installed_remotely: bool,
        _preference: ExtensionRunningPreference,
    ) -> Option<ExtensionHostKind> {
        panic!("TypeError: Cannot read properties of null (reading 'pickExtensionHostKind')")
    }
}

/// `{ event, activationKind, kind }` de `activationEvents`.
#[derive(Clone, Debug)]
struct ActivationRecord {
    event: String,
    activation_kind: ActivationKind,
    kind: ExtensionHostKind,
}

/// O `new class extends mock<IExtensionHostManager>() { ... }` de `_doCreateExtensionHostManager`.
struct TestExtensionHostManager {
    extension_host_id: u32,
    kind: ExtensionHostKind,
    running_location: ExtensionRunningLocation,
    order: Rc<RefCell<Vec<String>>>,
    activation_events: Rc<RefCell<Vec<ActivationRecord>>>,
    remote_ext_host_is_ready: Rc<Cell<bool>>,
    local_ext_host_is_ready: Rc<Cell<bool>>,
}

impl IRepresentsRunningLocation for TestExtensionHostManager {
    fn represents_running_location(&self, running_location: &ExtensionRunningLocation) -> bool {
        self.running_location.equals(running_location)
    }
}

impl IDisposable for TestExtensionHostManager {
    fn dispose(&self) {
        self.order.borrow_mut().push(format!("dispose {}", self.extension_host_id));
    }
}

impl IExtensionHostManager for TestExtensionHostManager {
    fn pid(&self) -> Option<u32> {
        not_mocked("pid")
    }

    fn kind(&self) -> ExtensionHostKind {
        self.kind
    }

    fn startup(&self) -> ExtensionHostStartup {
        ExtensionHostStartup::EagerManualStart
    }

    fn friendly_name(&self) -> String {
        not_mocked("friendyName")
    }

    fn on_did_exit(&self) -> Event<(i32, Option<String>)> {
        Event::none()
    }

    fn on_did_change_responsive_state(&self) -> Event<ResponsiveState> {
        Event::none()
    }

    fn disconnect(&self) -> Promise<()> {
        Promise::resolved(())
    }

    fn is_ready(&self) -> bool {
        if self.kind == ExtensionHostKind::Remote { self.remote_ext_host_is_ready.get() } else { self.local_ext_host_is_ready.get() }
    }

    fn ready(&self) -> Promise<()> {
        Promise::resolved(())
    }

    fn delta_extensions(&self, _extensions_delta: IExtensionDescriptionDelta) -> Promise<()> {
        not_mocked("deltaExtensions")
    }

    fn contains_extension(&self, _extension_id: &je_extensions::manifest::ExtensionIdentifier) -> bool {
        not_mocked("containsExtension")
    }

    fn activate(&self, _extension: &je_extensions::manifest::ExtensionIdentifier, _reason: ExtensionActivationReason) -> Promise<bool> {
        not_mocked("activate")
    }

    fn activate_by_event(&self, activation_event: &str, activation_kind: ActivationKind) -> Promise<()> {
        self.activation_events.borrow_mut().push(ActivationRecord { event: activation_event.to_owned(), activation_kind, kind: self.kind });
        Promise::resolved(())
    }

    fn activation_event_is_done(&self, _activation_event: &str) -> bool {
        not_mocked("activationEventIsDone")
    }

    fn get_inspect_port(&self, _try_enable_inspector: bool) -> Promise<Option<IExtensionInspectInfo>> {
        not_mocked("getInspectPort")
    }

    fn resolve_authority(&self, _remote_authority: &str, _resolve_attempt: u32) -> Promise<IResolveAuthorityResult> {
        not_mocked("resolveAuthority")
    }

    fn get_canonical_uri(&self, _remote_authority: &str, _uri: &URI) -> Promise<Option<URI>> {
        not_mocked("getCanonicalURI")
    }

    fn start(&self, _extension_registry_version_id: i64, _all_extensions: Vec<je_workbench::services::extensions::extension_description_registry::SharedExtensionDescription>, _my_extensions: Vec<je_extensions::manifest::ExtensionIdentifier>) -> Promise<()> {
        Promise::resolved(())
    }

    fn extension_tests_execute(&self) -> Promise<i32> {
        not_mocked("extensionTestsExecute")
    }

    fn set_remote_environment(&self, _env: Vec<(String, Option<String>)>) -> Promise<()> {
        not_mocked("setRemoteEnvironment")
    }
}

/// `class MyTestExtensionService extends AbstractExtensionService`.
struct MyTestExtensionService {
    base: Rc<AbstractExtensionService>,
    ext_host_id: Cell<u32>,
    order: Rc<RefCell<Vec<String>>>,
    activation_events: Rc<RefCell<Vec<ActivationRecord>>>,
    remote_ext_host_is_ready: Rc<Cell<bool>>,
    local_ext_host_is_ready: Rc<Cell<bool>>,
}

impl MyTestExtensionService {
    /// O construtor: `super(...)` e `this._initializeIfNeeded()`.
    fn new(base: Rc<AbstractExtensionService>) -> Rc<Self> {
        let this = Rc::new(Self {
            base,
            ext_host_id: Cell::new(0),
            order: Rc::new(RefCell::new(Vec::new())),
            activation_events: Rc::new(RefCell::new(Vec::new())),
            remote_ext_host_is_ready: Rc::new(Cell::new(true)),
            local_ext_host_is_ready: Rc::new(Cell::new(true)),
        });
        this.base.set_hooks(Rc::downgrade(&this) as Weak<dyn AbstractExtensionServiceHooks>);
        let _ = this.base.initialize_if_needed();
        this
    }

    fn order(&self) -> Vec<String> {
        self.order.borrow().clone()
    }

    fn activated_kinds(&self) -> Vec<ExtensionHostKind> {
        self.activation_events.borrow().iter().map(|e| e.kind).collect()
    }
}

impl AbstractExtensionServiceHooks for MyTestExtensionService {
    fn resolve_extensions(&self) -> Rc<dyn ResolvedExtensionsStream> {
        // Return empty iterable - no extensions to resolve in tests
        ResolvedExtensionsQueue::new(Vec::new())
    }

    fn on_extension_host_exit(&self, _code: i32) -> Promise<()> {
        panic!("Method not implemented.")
    }

    fn resolve_authority(&self, _remote_authority: &str) -> Promise<ResolverResult> {
        panic!("Method not implemented.")
    }

    fn do_create_extension_host_manager(&self, extension_host: Rc<dyn IExtensionHost>, _initial_activation_events: Vec<String>) -> Rc<dyn IExtensionHostManager> {
        self.ext_host_id.set(self.ext_host_id.get() + 1);
        let extension_host_id = self.ext_host_id.get();
        let running_location = extension_host.running_location();
        let kind = running_location.kind();
        self.order.borrow_mut().push(format!("create {extension_host_id}"));
        Rc::new(TestExtensionHostManager {
            extension_host_id,
            kind,
            running_location,
            order: self.order.clone(),
            activation_events: self.activation_events.clone(),
            remote_ext_host_is_ready: self.remote_ext_host_is_ready.clone(),
            local_ext_host_is_ready: self.local_ext_host_is_ready.clone(),
        })
    }
}

/// `TestWorkbenchExtensionEnablementService` (`workbenchTestServices.ts`).
struct TestWorkbenchExtensionEnablementService;

impl IWorkbenchExtensionEnablementService for TestWorkbenchExtensionEnablementService {
    fn on_enablement_changed(&self) -> Event<Vec<ExtensionRef>> {
        Event::none()
    }

    fn get_enablement_state(&self, _extension: &IExtension) -> EnablementState {
        EnablementState::EnabledGlobally
    }

    fn get_enablement_states(&self, _extensions: &[ExtensionRef], _workspace_type_overrides: WorkspaceTypeOverrides) -> Vec<EnablementState> {
        Vec::new()
    }

    fn get_dependencies_enablement_states(&self, _extension: &IExtension) -> Vec<(ExtensionRef, EnablementState)> {
        Vec::new()
    }

    fn can_change_enablement(&self, _extension: &IExtension) -> bool {
        true
    }

    fn can_change_workspace_enablement(&self, _extension: &IExtension) -> bool {
        true
    }

    fn is_enabled(&self, _extension: &IExtension) -> bool {
        true
    }

    fn is_enabled_enablement_state(&self, _enablement_state: EnablementState) -> bool {
        true
    }

    fn is_disabled_globally(&self, _extension: &IExtension) -> bool {
        false
    }

    fn set_enablement(&self, _extensions: Vec<ExtensionRef>, _state: EnablementState) -> Promise<Vec<bool>> {
        Promise::resolved(Vec::new())
    }

    fn update_extensions_enablements_when_workspace_trust_changes(&self) -> Promise<()> {
        Promise::resolved(())
    }
}

/// `TestWorkbenchExtensionManagementService` (`workbenchTestServices.ts`), com os métodos que o contrato do porte tem.
struct TestWorkbenchExtensionManagementService;

impl IExtensionManagementService for TestWorkbenchExtensionManagementService {
    fn on_did_install_extensions(&self) -> Event<Vec<InstallExtensionResult>> {
        Event::none()
    }

    fn on_did_uninstall_extension(&self) -> Event<DidUninstallExtensionEvent> {
        Event::none()
    }

    fn get_installed(&self) -> Promise<Vec<ExtensionRef>> {
        Promise::resolved(Vec::new())
    }

    fn get_extensions_control_manifest(&self) -> Promise<ExtensionsControlManifest> {
        Promise::rejected(Error::new("Method not implemented."))
    }
}

impl IProfileAwareExtensionManagementService for TestWorkbenchExtensionManagementService {
    fn on_profile_aware_did_install_extensions(&self) -> Event<Vec<InstallExtensionResult>> {
        Event::none()
    }

    fn on_profile_aware_did_uninstall_extension(&self) -> Event<DidUninstallExtensionEvent> {
        Event::none()
    }

    fn on_did_change_profile(&self) -> Event<DidChangeProfileEvent> {
        Event::none()
    }
}

impl IWorkbenchExtensionManagementService for TestWorkbenchExtensionManagementService {
    fn get_installed_workspace_extensions(&self, _include_invalid: bool) -> Promise<Vec<ExtensionRef>> {
        Promise::resolved(Vec::new())
    }
}

/// O `RemoteAuthorityResolverService` do navegador (ver as notas do módulo): rejeita qualquer resolução.
struct TestRemoteAuthorityResolverService;

impl IRemoteAuthorityResolverService for TestRemoteAuthorityResolverService {
    fn resolve_authority(&self, _authority: &str) -> Promise<ResolverResult> {
        Promise::rejected(Error::new("Method not implemented."))
    }

    fn get_canonical_uri(&self, _uri: &URI) -> Promise<URI> {
        Promise::rejected(Error::new("Method not implemented."))
    }
}

/// `TestDialogService` (`platform/dialogs/test/common/testDialogService.ts`): `confirm` resolve `{ confirmed: false }`.
struct TestDialogService;

impl IDialogService for TestDialogService {
    fn on_will_show_dialog(&self) -> Event<()> {
        Event::none()
    }

    fn on_did_show_dialog(&self) -> Event<()> {
        Event::none()
    }

    fn confirm(&self, _confirmation: Confirmation) -> Promise<ConfirmationResult> {
        Promise::resolved(ConfirmationResult { checkbox_checked: None, confirmed: false })
    }

    fn prompt(&self, _prompt: Prompt) -> Promise<PromptResult> {
        Promise::resolved(PromptResult { checkbox_checked: None, result: None })
    }

    fn input(&self, _input: Input) -> Promise<InputResult> {
        Promise::resolved(InputResult { confirmation: ConfirmationResult { checkbox_checked: None, confirmed: true }, values: Some(Vec::new()) })
    }

    fn info(&self, _message: &str, _detail: Option<&str>) -> Promise<()> {
        Promise::resolved(())
    }

    fn warn(&self, _message: &str, _detail: Option<&str>) -> Promise<()> {
        Promise::resolved(())
    }

    fn error(&self, _message: &str, _detail: Option<&str>) -> Promise<()> {
        Promise::resolved(())
    }

    fn about(&self) -> Promise<()> {
        Promise::resolved(())
    }
}

/// O `setup` da suíte `ExtensionService`: o `disposables`/guarda e o `extService`.
struct Fixture {
    guard: LeakGuard,
    ext_service: Rc<MyTestExtensionService>,
}

impl Fixture {
    fn new() -> Self {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let log_service = NullLogService::shared();
        let product_service = ProductService::default();
        let environment_service = TestEnvironmentService::new();
        let configuration_service = TestConfigurationService::new(None);

        let extensions_proposed_api = Rc::new(ExtensionsProposedApi::new(log_service.clone(), environment_service.clone(), &product_service));
        let extension_manifest_properties_service = guard.add_rc(ExtensionManifestPropertiesService::new(
            Rc::new(product_service),
            configuration_service.clone(),
            TestWorkspaceTrustEnablementService::new(true),
            log_service.clone(),
        ));

        let base = guard.add_rc(AbstractExtensionService::new(
            AbstractExtensionServiceOptions { allow_remote_extensions_in_local_web_worker: false, has_local_process: true },
            extensions_proposed_api,
            Rc::new(TestExtensionHostFactory),
            Rc::new(NullExtensionHostKindPicker),
            AbstractExtensionServiceServices {
                instantiation_service: guard.add_rc(InstantiationService::new(ServiceCollection::new())),
                notification_service: Rc::new(TestNotificationService),
                environment_service,
                telemetry_service: Rc::new(NullTelemetryServiceShape),
                extension_enablement_service: Rc::new(TestWorkbenchExtensionEnablementService),
                file_service: guard.add_rc(TestFileService::new()),
                extension_management_service: Rc::new(TestWorkbenchExtensionManagementService),
                context_service: TestContextService::new(None, None),
                configuration_service,
                extension_manifest_properties_service,
                log_service,
                remote_agent_service: Rc::new(TestRemoteAgentService),
                lifecycle_service: guard.add_rc(TestLifecycleService::new()),
                remote_authority_resolver_service: Rc::new(TestRemoteAuthorityResolverService),
                dialog_service: Rc::new(TestDialogService),
            },
        ));
        let ext_service = MyTestExtensionService::new(base);
        Self { guard, ext_service }
    }
}

/// Roda o caso: o `setup`, o corpo e o `teardown` (o `Drop` do [`Fixture`]).
fn run<F: Future<Output = ()> + 'static>(body: impl FnOnce(Rc<MyTestExtensionService>, Rc<LeakGuard>) -> F) {
    let Fixture { guard, ext_service } = Fixture::new();
    let guard = Rc::new(guard);
    let done = Rc::new(Cell::new(false));
    let finished = done.clone();
    let future = body(ext_service, guard.clone());
    runtime::spawn_local(async move {
        future.await;
        finished.set(true);
    });
    runtime::run_microtasks();
    assert!(done.get(), "o teste assíncrono não terminou");
}

mod extension_service {
    use super::*;

    #[test]
    fn issue_152204_remote_extension_host_not_disposed_after_closing_vscode_client() {
        run(|ext_service, _guard| async move {
            ext_service.base.stop_extension_hosts("foo", false).await.expect("stopExtensionHosts resolve");
            assert_eq!(ext_service.order(), ["create 1", "create 2", "create 3", "dispose 3", "dispose 2", "dispose 1"]);
        });
    }

    #[test]
    fn extension_host_disposed_when_awaited() {
        run(|ext_service, _guard| async move {
            ext_service.base.stop_extension_hosts("foo", false).await.expect("stopExtensionHosts resolve");
            assert_eq!(ext_service.order(), ["create 1", "create 2", "create 3", "dispose 3", "dispose 2", "dispose 1"]);
        });
    }

    #[test]
    fn extension_host_not_disposed_when_vetoed_sync() {
        run(|ext_service, guard| async move {
            guard.add(ext_service.base.on_will_stop().listen(|e| e.veto(Promise::resolved(true), "test 1")));
            guard.add(ext_service.base.on_will_stop().listen(|e| e.veto(Promise::resolved(false), "test 2")));

            ext_service.base.stop_extension_hosts("foo", false).await.expect("stopExtensionHosts resolve");
            assert_eq!(ext_service.order(), ["create 1", "create 2", "create 3"]);
        });
    }

    #[test]
    fn extension_host_not_disposed_when_vetoed_async() {
        run(|ext_service, guard| async move {
            guard.add(ext_service.base.on_will_stop().listen(|e| e.veto(Promise::resolved(false), "test 1")));
            guard.add(ext_service.base.on_will_stop().listen(|e| e.veto(Promise::resolved(true), "test 2")));
            guard.add(ext_service.base.on_will_stop().listen(|e| e.veto(Promise::resolved(false), "test 3")));

            ext_service.base.stop_extension_hosts("foo", false).await.expect("stopExtensionHosts resolve");
            assert_eq!(ext_service.order(), ["create 1", "create 2", "create 3"]);
        });
    }

    #[test]
    fn on_will_activate_by_event_includes_activation_kind_for_normal_activation() {
        run(|ext_service, guard| async move {
            let events: Rc<RefCell<Vec<IWillActivateEvent>>> = Rc::new(RefCell::new(Vec::new()));
            let listener_events = events.clone();
            guard.add(ext_service.base.on_will_activate_by_event().listen(move |e| listener_events.borrow_mut().push(e.clone())));

            ext_service.base.activate_by_event("onTest", Some(ActivationKind::Normal)).await.expect("activateByEvent resolve");

            let events = events.borrow();
            assert_eq!(events.len(), 1);
            assert_eq!(events[0].event, "onTest");
            assert_eq!(events[0].activation_kind, ActivationKind::Normal);
        });
    }

    #[test]
    fn on_will_activate_by_event_includes_activation_kind_for_immediate_activation() {
        run(|ext_service, guard| async move {
            let events: Rc<RefCell<Vec<IWillActivateEvent>>> = Rc::new(RefCell::new(Vec::new()));
            let listener_events = events.clone();
            guard.add(ext_service.base.on_will_activate_by_event().listen(move |e| listener_events.borrow_mut().push(e.clone())));

            ext_service.base.activate_by_event("onTest", Some(ActivationKind::Immediate)).await.expect("activateByEvent resolve");

            let events = events.borrow();
            assert_eq!(events.len(), 1);
            assert_eq!(events[0].event, "onTest");
            assert_eq!(events[0].activation_kind, ActivationKind::Immediate);
        });
    }

    #[test]
    fn immediate_activation_includes_ready_remote_extension_hosts_issue_297019() {
        run(|ext_service, _guard| async move {
            ext_service.activation_events.borrow_mut().clear(); // Clear any initial activations

            ext_service.base.activate_by_event("onTest", Some(ActivationKind::Immediate)).await.expect("activateByEvent resolve");

            // When remote host is ready, Immediate activation should include it
            let activated_kinds = ext_service.activated_kinds();
            assert!(activated_kinds.contains(&ExtensionHostKind::LocalProcess), "Should activate on LocalProcess");
            assert!(activated_kinds.contains(&ExtensionHostKind::LocalWebWorker), "Should activate on LocalWebWorker");
            assert!(activated_kinds.contains(&ExtensionHostKind::Remote), "Should activate on ready Remote host");
        });
    }

    #[test]
    fn immediate_activation_excludes_not_ready_remote_extension_hosts() {
        run(|ext_service, _guard| async move {
            ext_service.remote_ext_host_is_ready.set(false);
            ext_service.activation_events.borrow_mut().clear(); // Clear any initial activations

            ext_service.base.activate_by_event("onTest", Some(ActivationKind::Immediate)).await.expect("activateByEvent resolve");

            // When remote host is not ready, Immediate activation should skip it
            let activated_kinds = ext_service.activated_kinds();
            assert!(activated_kinds.contains(&ExtensionHostKind::LocalProcess), "Should activate on LocalProcess");
            assert!(activated_kinds.contains(&ExtensionHostKind::LocalWebWorker), "Should activate on LocalWebWorker");
            assert!(!activated_kinds.contains(&ExtensionHostKind::Remote), "Should NOT activate on not-ready Remote host");
        });
    }

    #[test]
    fn immediate_activation_still_activates_local_extension_hosts_even_when_not_ready() {
        run(|ext_service, _guard| async move {
            ext_service.local_ext_host_is_ready.set(false);
            ext_service.activation_events.borrow_mut().clear(); // Clear any initial activations

            ext_service.base.activate_by_event("onTest", Some(ActivationKind::Immediate)).await.expect("activateByEvent resolve");

            // Local hosts should always be activated for Immediate, regardless of isReady
            let activated_kinds = ext_service.activated_kinds();
            assert!(activated_kinds.contains(&ExtensionHostKind::LocalProcess), "Should activate on LocalProcess even when not ready");
            assert!(activated_kinds.contains(&ExtensionHostKind::LocalWebWorker), "Should activate on LocalWebWorker even when not ready");
        });
    }

    #[test]
    fn normal_activation_activates_all_extension_hosts() {
        run(|ext_service, _guard| async move {
            ext_service.activation_events.borrow_mut().clear(); // Clear any initial activations

            ext_service.base.activate_by_event("onTest", Some(ActivationKind::Normal)).await.expect("activateByEvent resolve");

            // Should activate on all hosts
            let activated_kinds = ext_service.activated_kinds();
            assert!(activated_kinds.contains(&ExtensionHostKind::LocalProcess), "Should activate on LocalProcess");
            assert!(activated_kinds.contains(&ExtensionHostKind::LocalWebWorker), "Should activate on LocalWebWorker");
            assert!(activated_kinds.contains(&ExtensionHostKind::Remote), "Should activate on Remote");
        });
    }
}
