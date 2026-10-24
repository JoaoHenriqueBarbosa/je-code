//! Porte parcial de `src/vs/workbench/test/browser/workbenchTestServices.ts`: os dublês de teste do navegador que os
//! testes de caminho e de rótulo usam: `TestPathService`, `TestEnvironmentService` e `TestRemoteAgentService`.
//!
//! Notas de tradução:
//! - **`TestEnvironmentService`** é uma constante do original (`new TestEnvironmentServiceWithArgs('', URI.file('tests')
//!   .with({ scheme: 'vscode-tests' }), Object.create(null), TestProductService)`, uma subclasse do
//!   `BrowserWorkbenchEnvironmentService` que só acrescenta `args = []`). O `BrowserWorkbenchEnvironmentService` não
//!   foi portado, então esta struct implementa os contratos [`EnvironmentService`] e [`IWorkbenchEnvironmentService`]
//!   com os valores que a classe do navegador calcula para essas opções: sem `payload`, sem `developmentOptions`, sem
//!   `remoteAuthority` e sem `enableWorkspaceTrust`. `@memoize` não existe aqui, os valores são puros.
//! - **`TestRemoteAgentService`** implementa o [`IRemoteAgentService`] reduzido do porte (`getEnvironment` e
//!   `getRawEnvironment`, que devolvem `null`). Os demais métodos da classe (`getConnection`,
//!   `getExtensionHostExitInfo`, `getDiagnosticInfo`, `updateTelemetryLevel`, `logTelemetry`, `flushTelemetry`,
//!   `getRoundTripTime` e `endConnection`) não fazem parte do contrato reduzido e entram junto com os tipos de que
//!   dependem.
//! - **`TestPathService.defaultUriScheme`** é uma propriedade pública atribuível, aqui o campo com
//!   [`TestPathService::set_default_uri_scheme`].

use std::cell::RefCell;
use std::rc::Rc;

use je_base::extpath::is_valid_basename;
use je_base::lifecycle::{self, DisposableRef};
use je_base::network::schemas;
use je_base::platform::OperatingSystem;
use je_base::promise::Promise;
use je_base::uri::{Field, URI, UriChange, UriComponents};
use je_base::JsString;
use je_platform::environment::environment::{DisableExtensions, EnvironmentService, ExtensionHostDebugParams, ExtensionKind, SyncMode};
use je_platform::product::ProductService;
use je_platform::remote::remote_agent_environment::RemoteAgentEnvironment;
use je_platform::resources::{ext_uri, host_platform, join_path, uri_file};

use crate::services::environment::environment_service::IWorkbenchEnvironmentService;
use crate::services::path::path_service::{IPathService, IResourcePathProvider, PathFlavor};
use crate::services::remote::remote_agent_service::IRemoteAgentService;
use crate::test::common::workbench_test_services::vscode_tests_resource;

/// `TestPathService`.
pub struct TestPathService {
    fallback_user_home: URI,
    default_uri_scheme: RefCell<String>,
}

impl TestPathService {
    /// `constructor(fallbackUserHome = URI.from({ scheme: Schemas.file, path: '/' }), defaultUriScheme = Schemas.file)`.
    pub fn new(fallback_user_home: Option<URI>, default_uri_scheme: Option<&str>) -> Rc<Self> {
        let fallback_user_home = fallback_user_home
            .unwrap_or_else(|| URI::from(&UriComponents { scheme: Some(JsString::from(schemas::FILE)), path: Some(JsString::from("/")), ..UriComponents::default() }, false));
        Rc::new(Self { fallback_user_home, default_uri_scheme: RefCell::new(default_uri_scheme.unwrap_or(schemas::FILE).to_string()) })
    }

    /// `testPathService.defaultUriScheme = scheme`.
    pub fn set_default_uri_scheme(&self, scheme: &str) {
        *self.default_uri_scheme.borrow_mut() = scheme.to_string();
    }

    /// `isValidBasename(name ?? basename(resource))`, com o `isWindowsOS` padrão (o do processo).
    fn valid_basename(resource: &URI, name: Option<&str>) -> bool {
        let basename = ext_uri().basename(resource, None);
        is_valid_basename(Some(name.unwrap_or(&basename)), host_platform() == PathFlavor::Win32)
    }
}

impl IPathService for TestPathService {
    fn path(&self) -> Promise<PathFlavor> {
        Promise::resolved(host_platform())
    }

    fn default_uri_scheme(&self) -> String {
        self.default_uri_scheme.borrow().clone()
    }

    fn file_uri(&self, path: &str) -> Promise<URI> {
        Promise::resolved(uri_file(path))
    }

    fn user_home(&self) -> Promise<URI> {
        Promise::resolved(self.fallback_user_home.clone())
    }

    fn local_user_home(&self) -> URI {
        self.fallback_user_home.clone()
    }

    fn has_valid_basename(&self, resource: &URI, basename: Option<&str>) -> Promise<bool> {
        Promise::resolved(Self::valid_basename(resource, basename))
    }

    fn has_valid_basename_for_os(&self, resource: &URI, _os: OperatingSystem, basename: Option<&str>) -> bool {
        Self::valid_basename(resource, basename)
    }

    fn get_operating_system(&self, _resource: &URI) -> Promise<Option<OperatingSystem>> {
        Promise::resolved(Some(if host_platform() == PathFlavor::Win32 { OperatingSystem::Windows } else { OperatingSystem::Linux }))
    }

    fn get_path(&self, _resource: &URI) -> Promise<Option<PathFlavor>> {
        Promise::resolved(Some(host_platform()))
    }

    fn register_path_provider(&self, _scheme: &str, _provider: Rc<dyn IResourcePathProvider>) -> DisposableRef {
        lifecycle::none()
    }

    fn resolved_user_home(&self) -> Option<URI> {
        Some(self.fallback_user_home.clone())
    }
}

/// `TestRemoteAgentService`.
pub struct TestRemoteAgentService;

impl IRemoteAgentService for TestRemoteAgentService {
    fn get_environment(&self) -> Promise<Option<RemoteAgentEnvironment>> {
        Promise::resolved(None)
    }

    fn get_raw_environment(&self) -> Promise<Option<RemoteAgentEnvironment>> {
        Promise::resolved(None)
    }
}

/// `TestEnvironmentService`.
pub struct TestEnvironmentService {
    workspace_id: String,
    logs_home: URI,
    is_built: bool,
}

impl TestEnvironmentService {
    pub fn new() -> Rc<Self> {
        Rc::new(Self { workspace_id: String::new(), logs_home: vscode_tests_resource(), is_built: ProductService::default().commit.is_some_and(|commit| !commit.is_empty()) })
    }

    /// `joinPath(this.userRoamingDataHome, ...fragments)`.
    fn under_user_roaming_data_home(&self, fragments: &[&str]) -> URI {
        join_path(&self.user_roaming_data_home(), fragments)
    }
}

impl EnvironmentService for TestEnvironmentService {
    fn state_resource(&self) -> URI {
        self.under_user_roaming_data_home(&["State", "storage.json"])
    }

    fn user_roaming_data_home(&self) -> URI {
        uri_file("/User").with(&UriChange { scheme: Field::from(schemas::VSCODE_USER_DATA), ..UriChange::default() })
    }

    fn keyboard_layout_resource(&self) -> URI {
        self.under_user_roaming_data_home(&["keyboardLayout.json"])
    }

    fn argv_resource(&self) -> URI {
        self.under_user_roaming_data_home(&["argv.json"])
    }

    fn untitled_workspaces_home(&self) -> URI {
        self.under_user_roaming_data_home(&["Workspaces"])
    }

    fn workspace_storage_home(&self) -> URI {
        self.under_user_roaming_data_home(&["workspaceStorage"])
    }

    fn local_history_home(&self) -> URI {
        self.under_user_roaming_data_home(&["History"])
    }

    fn cache_home(&self) -> URI {
        self.under_user_roaming_data_home(&["caches"])
    }

    fn app_shared_data_home(&self) -> URI {
        self.under_user_roaming_data_home(&["sharedData"])
    }

    /// In Web every workspace can potentially have scoped user-data and/or extensions and if Sync state is shared
    /// then it can make Sync error prone - say removing extensions from another workspace. Hence scope Sync state per
    /// workspace. Sync scoped to a workspace is capable of handling opening same workspace in multiple windows.
    fn user_data_sync_home(&self) -> URI {
        self.under_user_roaming_data_home(&["sync", &self.workspace_id])
    }

    fn sync(&self) -> Option<SyncMode> {
        None
    }

    fn continue_on(&self) -> Option<String> {
        None
    }

    fn edit_session_id(&self) -> Option<String> {
        None
    }

    fn debug_extension_host(&self) -> ExtensionHostDebugParams {
        ExtensionHostDebugParams { port: None, break_: false, debug_id: None, env: None }
    }

    fn is_extension_development(&self) -> bool {
        false
    }

    fn disable_extensions(&self) -> DisableExtensions {
        DisableExtensions::All(false)
    }

    fn skip_builtin_extensions(&self) -> Vec<String> {
        Vec::new()
    }

    fn extension_development_location_uri(&self) -> Option<Vec<URI>> {
        None
    }

    fn extension_development_kind(&self) -> Option<Vec<ExtensionKind>> {
        None
    }

    fn extension_tests_location_uri(&self) -> Option<URI> {
        None
    }

    fn logs_home(&self) -> URI {
        self.logs_home.clone()
    }

    fn log_level(&self) -> Option<String> {
        None
    }

    fn extension_log_level(&self) -> Option<Vec<(String, String)>> {
        None
    }

    fn verbose(&self) -> bool {
        false
    }

    fn is_built(&self) -> bool {
        self.is_built
    }

    fn disable_telemetry(&self) -> bool {
        false
    }

    fn disable_experiments(&self) -> bool {
        false
    }

    fn service_machine_id_resource(&self) -> URI {
        self.under_user_roaming_data_home(&["machineid"])
    }

    fn agent_sessions_workspace(&self) -> URI {
        self.under_user_roaming_data_home(&["agent-sessions.code-workspace"])
    }

    fn policy_file(&self) -> Option<URI> {
        None
    }
}

impl IWorkbenchEnvironmentService for TestEnvironmentService {
    fn log_file(&self) -> URI {
        join_path(&self.window_logs_path(), &["window.log"])
    }

    fn window_logs_path(&self) -> URI {
        self.logs_home()
    }

    fn ext_host_logs_path(&self) -> URI {
        join_path(&self.logs_home(), &["exthost"])
    }

    fn extension_enabled_proposed_api(&self) -> Option<Vec<String>> {
        None
    }

    fn remote_authority(&self) -> Option<String> {
        None
    }

    fn skip_release_notes(&self) -> bool {
        false
    }

    fn skip_welcome(&self) -> bool {
        false
    }

    /// `!this.options.enableWorkspaceTrust`.
    fn disable_workspace_trust(&self) -> bool {
        true
    }

    fn is_sessions_window(&self) -> bool {
        false
    }

    fn debug_renderer(&self) -> bool {
        false
    }

    fn log_extension_host_communication(&self) -> Option<bool> {
        Some(false)
    }

    fn enable_smoke_test_driver(&self) -> Option<bool> {
        None
    }

    fn prof_duration_markers(&self) -> Option<Vec<String>> {
        None
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// `TestEditorGroupsService`, reduzido ao contrato `IEditorGroupsService` do porte (só os `groups`). A classe do original
// recebe os `IEditorGroupView[]` e acrescenta dezenas de membros de layout, que entram junto com o contrato.
// ---------------------------------------------------------------------------------------------------------------------

use crate::services::editor::editor_groups_service::{IEditorGroup, IEditorGroupsService};

/// `TestEditorGroupsService`.
pub struct TestEditorGroupsService {
    groups: Vec<Rc<dyn IEditorGroup>>,
}

impl TestEditorGroupsService {
    /// `constructor(public groups: IEditorGroupView[] = [])`.
    pub fn new(groups: Option<Vec<Rc<dyn IEditorGroup>>>) -> Rc<Self> {
        Rc::new(Self { groups: groups.unwrap_or_default() })
    }
}

impl IEditorGroupsService for TestEditorGroupsService {
    fn groups(&self) -> Vec<Rc<dyn IEditorGroup>> {
        self.groups.clone()
    }
}
