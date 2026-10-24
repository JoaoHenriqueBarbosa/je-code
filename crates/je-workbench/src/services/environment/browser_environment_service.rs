//! Porte de `src/vs/workbench/services/environment/browser/environmentService.ts`: o `IBrowserWorkbenchEnvironmentService`
//! e o `BrowserWorkbenchEnvironmentService`, o serviço de ambiente do workbench para os ambientes onde a API da web existe
//! (navegador, Electron), mais o subconjunto de `IWorkbenchConstructionOptions` (`browser/web.api.ts`) que ele lê.
//!
//! Divergências:
//! - **`@memoize`.** O que o decorador guarda é uma função pura das opções, do produto e do `payload` (todos imutáveis
//!   depois do construtor), então os getters recalculam sem mudar o resultado. A exceção é a resolução do ambiente de
//!   depuração do extension host (`resolveExtensionHostDebugEnvironment`), que tem efeito (`onUnexpectedError`) e é
//!   compartilhada por sete getters: ela é resolvida uma única vez, na primeira leitura, como o campo
//!   `extensionHostDebugEnvironment` do original.
//! - **`IWorkbenchConstructionOptions`.** É a struct [`WorkbenchConstructionOptions`], só com o que este serviço lê
//!   (`remoteAuthority`, `webSocketFactory`, `webviewEndpoint`, `editSessionId`, `workspaceProvider.payload`,
//!   `enabledExtensions`, `enabledExtensionProposedApi`, `enableWorkspaceTrust` e `developmentOptions`). O
//!   `IWebSocketFactory` não foi portado: `webSocketFactory` é [`WorkbenchConstructionOptions::has_web_socket_factory`], que
//!   é o único uso do original (`!this.options.webSocketFactory`). `UriComponents` de `extensions` e `extensionTestsPath` já
//!   chegam como `URI` (o `URI.revive` do original é a identidade para elas).
//! - **`payload`.** O `new Map(options.workspaceProvider.payload)` é a lista de pares sem repetir chave (a última
//!   atribuição vence e a posição é a da primeira, como no `Map`); a proteção `Array.isArray` e o `try/catch` de um
//!   `payload` com entradas inválidas não têm correspondente, porque o tipo `Vec<(String, String)>` já garante pares.
//! - **`parseInt(value)`.** É [`parse_int`] (o prefixo numérico decimal, `NaN` quando não há dígito).
//! - **`JSON.parse(value)`** de `extensionEnvironment` é o `parse_str` do `je-base`, o mesmo que o serviço nativo usa para o
//!   `--extensionEnvironment`; um texto inválido vai para `onUnexpectedError`.
//! - **`extensionDevelopmentKind`.** O original só faz o `cast` do texto (`<ExtensionKind>value`); aqui o texto vira o
//!   [`ExtensionKind`] (`ui`, `workspace` ou `web`), e um valor fora desses três é ignorado (não há `ExtensionKind` para ele).
//! - **`filesToOpenOrCreate`.** O `IPath` do porte só tem o `fileUri` (ver `je_platform::window`): o `options.selection` que o
//!   original monta com `gotoLineMode` (linha e coluna do `--goto`) não é carregado, mas o `fileUri` já sai sem o sufixo
//!   `:linha:coluna`, como no original.
//! - **`webviewExternalEndpoint`.** O `IProductService` do porte não traz `webviewContentExternalBaseUrlTemplate` (e o trait
//!   [`IWorkbenchEnvironmentService`] não declara a propriedade): ela é o método próprio
//!   [`BrowserWorkbenchEnvironmentService::webview_external_endpoint`], que parte de `options.webviewEndpoint` ou do modelo
//!   padrão do original (o mesmo valor do `product.json` estável). O `String.replace` com texto troca só a primeira
//!   ocorrência, como o `replacen(.., 1)`.
//! - **`logExtensionHostCommunication`.** O trait devolve `Option<bool>`: este serviço devolve sempre `Some`.
//! - **`IBrowserWorkbenchEnvironmentService.options?`.** É `Option<&WorkbenchConstructionOptions>`.

use std::cell::OnceCell;
use std::rc::Rc;

use je_base::errors::{Error, on_unexpected_error};
use je_base::extpath::parse_line_and_column_aware;
use je_base::json;
use je_base::network::schemas;
use je_base::uri::{Field, URI, UriChange};
use je_instantiation::service_id;
use je_platform::environment::environment::{DisableExtensions, EnvironmentService, ExtensionHostDebugParams, ExtensionKind, SyncMode};
use je_platform::environment::environment_service::match_extension_identifier_with_log;
use je_platform::js::from_json;
use je_platform::log::{LogLevel, log_level_to_string};
use je_platform::product::IProductService;
use je_platform::resources::{join_path, uri_file};
use je_platform::window::IPath;

use crate::services::environment::environment_service::IWorkbenchEnvironmentService;

/// `IDevelopmentOptions` (`browser/web.api.ts`).
#[derive(Clone, Debug, Default)]
pub struct DevelopmentOptions {
    /// `logLevel`: the default log level to use. Default is `LogLevel.Info`.
    pub log_level: Option<LogLevel>,
    /// `extensionLogLevel`: the log level to use for the given extensions.
    pub extension_log_level: Option<Vec<(String, LogLevel)>>,
    /// `extensionTestsPath`: endpoint to a test bundle that contains extension tests.
    pub extension_tests_path: Option<URI>,
    /// `extensions`: the locations of extensions to load for development.
    pub extensions: Option<Vec<URI>>,
    /// `enableSmokeTestDriver`: current logging level.
    pub enable_smoke_test_driver: Option<bool>,
}

/// `IWorkspaceProvider`, só com o `payload` que o serviço lê.
#[derive(Clone, Debug, Default)]
pub struct WorkspaceProviderOptions {
    /// `payload`: arbitrary payload from the workspace provider (as `[string, string][]`).
    pub payload: Option<Vec<(String, String)>>,
}

/// `IWorkbenchConstructionOptions`, só com o que o [`BrowserWorkbenchEnvironmentService`] lê.
#[derive(Clone, Debug, Default)]
pub struct WorkbenchConstructionOptions {
    /// `remoteAuthority`.
    pub remote_authority: Option<String>,
    /// `webSocketFactory !== undefined`.
    pub has_web_socket_factory: bool,
    /// `webviewEndpoint`.
    pub webview_endpoint: Option<String>,
    /// `editSessionId`.
    pub edit_session_id: Option<String>,
    /// `workspaceProvider`.
    pub workspace_provider: Option<WorkspaceProviderOptions>,
    /// `enabledExtensions`.
    pub enabled_extensions: Option<Vec<String>>,
    /// `enabledExtensionProposedApi`: an empty array enables proposed APIs for all extensions.
    pub enabled_extension_proposed_api: Option<Vec<String>>,
    /// `enableWorkspaceTrust`.
    pub enable_workspace_trust: Option<bool>,
    /// `developmentOptions`.
    pub development_options: Option<DevelopmentOptions>,
}

/// `IBrowserWorkbenchEnvironmentService`: a subclass of the `IWorkbenchEnvironmentService` to be used only environments where
/// the web API is available (browsers, Electron).
pub trait IBrowserWorkbenchEnvironmentService: IWorkbenchEnvironmentService {
    /// Options used to configure the workbench.
    fn options(&self) -> Option<&WorkbenchConstructionOptions>;

    /// Title of the agent session that launched this workbench.
    fn session_title(&self) -> Option<String>;

    /// Gets whether a resolver extension is expected for the environment.
    fn expects_resolver_extension(&self) -> bool;
}

service_id!(pub I_BROWSER_WORKBENCH_ENVIRONMENT_SERVICE: dyn IBrowserWorkbenchEnvironmentService = "environmentService");

/// `IExtensionHostDebugEnvironment`.
#[derive(Clone, Debug)]
struct ExtensionHostDebugEnvironment {
    params: ExtensionHostDebugParams,
    debug_renderer: bool,
    is_extension_development: bool,
    extension_development_location_uri: Option<Vec<URI>>,
    extension_development_kind: Option<Vec<ExtensionKind>>,
    extension_tests_location_uri: Option<URI>,
    extension_enabled_proposed_api: Option<Vec<String>>,
}

/// O modelo padrão de `webviewExternalEndpoint`.
const DEFAULT_WEBVIEW_EXTERNAL_ENDPOINT: &str = "https://{{uuid}}.vscode-cdn.net/{{quality}}/{{commit}}/out/vs/workbench/contrib/webview/browser/pre/";

/// O `commit` quando nem o `payload` nem o produto trazem um.
const DEFAULT_WEBVIEW_COMMIT: &str = "ef65ac1ba57f57f2a3961bfe94aa20481caca4c6";

/// `parseInt(value)` (base 10): o prefixo numérico, `NaN` quando o texto não começa com dígito.
pub fn parse_int(value: &str) -> f64 {
    let trimmed = value.trim_start();
    let (sign, digits) = match trimmed.as_bytes().first() {
        Some(b'-') => (-1.0, &trimmed[1..]),
        Some(b'+') => (1.0, &trimmed[1..]),
        _ => (1.0, trimmed),
    };
    let length = digits.bytes().take_while(u8::is_ascii_digit).count();
    if length == 0 {
        return f64::NAN;
    }
    sign * digits[..length].parse::<f64>().unwrap_or(f64::NAN)
}

/// `ExtensionKind` a partir do texto do `payload`.
fn parse_extension_kind(value: &str) -> Option<ExtensionKind> {
    match value {
        "ui" => Some(ExtensionKind::Ui),
        "workspace" => Some(ExtensionKind::Workspace),
        "web" => Some(ExtensionKind::Web),
        _ => None,
    }
}

/// `BrowserWorkbenchEnvironmentService`.
pub struct BrowserWorkbenchEnvironmentService {
    workspace_id: String,
    logs_home: URI,
    options: WorkbenchConstructionOptions,
    product_service: Rc<dyn IProductService>,
    /// `payload`: o `Map` do `workspaceProvider.payload`, na ordem de inserção.
    payload: Option<Vec<(String, String)>>,
    extension_host_debug_environment: OnceCell<ExtensionHostDebugEnvironment>,
}

impl BrowserWorkbenchEnvironmentService {
    /// `constructor(workspaceId, logsHome, options, productService)`.
    pub fn new(workspace_id: &str, logs_home: URI, options: WorkbenchConstructionOptions, product_service: Rc<dyn IProductService>) -> Self {
        let payload = options.workspace_provider.as_ref().and_then(|provider| provider.payload.as_ref()).map(|entries| {
            let mut map: Vec<(String, String)> = Vec::new();
            for (key, value) in entries {
                match map.iter_mut().find(|(existing, _)| existing == key) {
                    Some(entry) => entry.1 = value.clone(),
                    None => map.push((key.clone(), value.clone())),
                }
            }
            map
        });

        Self { workspace_id: workspace_id.to_owned(), logs_home, options, product_service, payload, extension_host_debug_environment: OnceCell::new() }
    }

    /// `this.payload?.get(key)`.
    fn payload_get(&self, key: &str) -> Option<&str> {
        self.payload.as_ref()?.iter().find(|(existing, _)| existing == key).map(|(_, value)| value.as_str())
    }

    /// `this.payload?.has(key)`.
    fn payload_has(&self, key: &str) -> bool {
        self.payload.as_ref().is_some_and(|entries| entries.iter().any(|(existing, _)| existing == key))
    }

    /// `this.payload?.get(key)` quando o valor é verdadeiro no JavaScript (não vazio).
    fn payload_truthy(&self, key: &str) -> Option<&str> {
        self.payload_get(key).filter(|value| !value.is_empty())
    }

    /// `this.payload?.get(key) === 'true'`.
    fn payload_is_true(&self, key: &str) -> bool {
        self.payload_get(key) == Some("true")
    }

    /// `joinPath(this.userRoamingDataHome, ...fragments)`.
    fn under_user_roaming_data_home(&self, fragments: &[&str]) -> URI {
        join_path(&self.user_roaming_data_home(), fragments)
    }

    /// `profile`.
    pub fn profile(&self) -> Option<String> {
        self.payload_get("profile").map(str::to_owned)
    }

    /// `webviewExternalEndpoint`.
    pub fn webview_external_endpoint(&self) -> String {
        let endpoint = self.options.webview_endpoint.as_deref().filter(|endpoint| !endpoint.is_empty()).unwrap_or(DEFAULT_WEBVIEW_EXTERNAL_ENDPOINT);

        let webview_external_endpoint_commit = self.payload_get("webviewExternalEndpointCommit");
        let commit = webview_external_endpoint_commit.or_else(|| self.product_service.commit()).unwrap_or(DEFAULT_WEBVIEW_COMMIT);
        let quality = if webview_external_endpoint_commit.is_some_and(|commit| !commit.is_empty()) { "insider" } else { self.product_service.quality().unwrap_or("insider") };

        endpoint.replacen("{{commit}}", commit, 1).replacen("{{quality}}", quality, 1)
    }

    /// `this.extensionHostDebugEnvironment`, resolvido na primeira leitura.
    fn debug_environment(&self) -> &ExtensionHostDebugEnvironment {
        self.extension_host_debug_environment.get_or_init(|| self.resolve_extension_host_debug_environment())
    }

    fn resolve_extension_host_debug_environment(&self) -> ExtensionHostDebugEnvironment {
        let mut environment = ExtensionHostDebugEnvironment {
            params: ExtensionHostDebugParams { port: None, break_: false, debug_id: None, env: None },
            debug_renderer: false,
            is_extension_development: false,
            extension_development_location_uri: None,
            extension_development_kind: None,
            extension_tests_location_uri: None,
            extension_enabled_proposed_api: None,
        };

        // Extension host development options from the payload are only valid in development or smoke test builds.
        if let Some(payload) = &self.payload {
            if !self.is_built() || self.enable_smoke_test_driver().unwrap_or(false) {
                for (key, value) in payload {
                    match key.as_str() {
                        "extensionDevelopmentPath" => {
                            environment.extension_development_location_uri.get_or_insert_with(Vec::new).push(URI::parse(value.as_str(), false));
                            environment.is_extension_development = true;
                        }
                        "extensionDevelopmentKind" => {
                            environment.extension_development_kind = parse_extension_kind(value).map(|kind| vec![kind]);
                        }
                        "extensionTestsPath" => {
                            environment.extension_tests_location_uri = Some(URI::parse(value.as_str(), false));
                        }
                        "debugRenderer" => {
                            environment.debug_renderer = value == "true";
                        }
                        "debugId" => {
                            environment.params.debug_id = Some(value.clone());
                        }
                        "inspect-brk-extensions" => {
                            environment.params.port = Some(parse_int(value));
                            environment.params.break_ = true;
                        }
                        "inspect-extensions" => {
                            environment.params.port = Some(parse_int(value));
                        }
                        "extensionEnvironment" => {
                            let mut errors = Vec::new();
                            match json::parse_str(value, &mut errors) {
                                Some(parsed) if errors.is_empty() => environment.params.env = Some(from_json(&parsed)),
                                _ => on_unexpected_error(&Error::new(format!("Unexpected token in JSON: {value}"))),
                            }
                        }
                        "enableProposedApi" => {
                            environment.extension_enabled_proposed_api = Some(Vec::new());
                        }
                        _ => {}
                    }
                }
            }
        }

        if let Some(development_options) = &self.options.development_options {
            if !environment.is_extension_development {
                if let Some(extensions) = development_options.extensions.as_ref().filter(|extensions| !extensions.is_empty()) {
                    environment.extension_development_location_uri = Some(extensions.clone());
                    environment.is_extension_development = true;
                }

                if let Some(extension_tests_path) = &development_options.extension_tests_path {
                    environment.extension_tests_location_uri = Some(extension_tests_path.clone());
                }
            }
        }

        environment
    }
}

impl EnvironmentService for BrowserWorkbenchEnvironmentService {
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

    /// In Web every workspace can potentially have scoped user-data and/or extensions and if Sync state is shared then it can
    /// make Sync error prone - say removing extensions from another workspace. Hence scope Sync state per workspace. Sync
    /// scoped to a workspace is capable of handling opening same workspace in multiple windows.
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
        self.options.edit_session_id.clone()
    }

    fn debug_extension_host(&self) -> ExtensionHostDebugParams {
        self.debug_environment().params.clone()
    }

    fn is_extension_development(&self) -> bool {
        self.debug_environment().is_extension_development
    }

    fn disable_extensions(&self) -> DisableExtensions {
        DisableExtensions::All(self.payload_is_true("disableExtensions"))
    }

    fn skip_builtin_extensions(&self) -> Vec<String> {
        Vec::new()
    }

    fn enable_extensions(&self) -> Option<Vec<String>> {
        self.options.enabled_extensions.clone()
    }

    fn extension_development_location_uri(&self) -> Option<Vec<URI>> {
        self.debug_environment().extension_development_location_uri.clone()
    }

    fn extension_development_kind(&self) -> Option<Vec<ExtensionKind>> {
        self.debug_environment().extension_development_kind.clone()
    }

    fn extension_tests_location_uri(&self) -> Option<URI> {
        self.debug_environment().extension_tests_location_uri.clone()
    }

    fn logs_home(&self) -> URI {
        self.logs_home.clone()
    }

    fn log_level(&self) -> Option<String> {
        if let Some(log_level_from_payload) = self.payload_truthy("logLevel") {
            return log_level_from_payload.split(',').find(|entry| match_extension_identifier_with_log(entry).is_none()).map(str::to_owned);
        }

        self.options.development_options.as_ref().and_then(|options| options.log_level).map(|log_level| log_level_to_string(log_level).to_owned())
    }

    fn extension_log_level(&self) -> Option<Vec<(String, String)>> {
        if let Some(log_level_from_payload) = self.payload_truthy("logLevel") {
            let result: Vec<(String, String)> = log_level_from_payload
                .split(',')
                .filter_map(match_extension_identifier_with_log)
                .filter(|(extension, level)| !extension.is_empty() && !level.is_empty())
                .collect();

            return if result.is_empty() { None } else { Some(result) };
        }

        self.options
            .development_options
            .as_ref()
            .and_then(|options| options.extension_log_level.as_ref())
            .map(|levels| levels.iter().map(|(extension, log_level)| (extension.clone(), log_level_to_string(*log_level).to_owned())).collect())
    }

    fn verbose(&self) -> bool {
        self.payload_is_true("verbose")
    }

    fn is_built(&self) -> bool {
        self.product_service.commit().is_some_and(|commit| !commit.is_empty())
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

impl IWorkbenchEnvironmentService for BrowserWorkbenchEnvironmentService {
    fn log_file(&self) -> URI {
        join_path(&self.window_logs_path(), &["window.log"])
    }

    fn window_logs_path(&self) -> URI {
        self.logs_home()
    }

    fn ext_host_logs_path(&self) -> URI {
        join_path(&self.logs_home, &["exthost"])
    }

    fn extension_enabled_proposed_api(&self) -> Option<Vec<String>> {
        let environment = self.debug_environment();
        if environment.extension_enabled_proposed_api.is_some() {
            return environment.extension_enabled_proposed_api.clone();
        }

        self.options.enabled_extension_proposed_api.clone()
    }

    fn remote_authority(&self) -> Option<String> {
        self.options.remote_authority.clone()
    }

    fn skip_release_notes(&self) -> bool {
        self.payload_is_true("skipReleaseNotes")
    }

    fn skip_welcome(&self) -> bool {
        self.payload_is_true("skipWelcome")
    }

    fn disable_workspace_trust(&self) -> bool {
        !self.options.enable_workspace_trust.unwrap_or(false)
    }

    fn is_sessions_window(&self) -> bool {
        self.payload_is_true("isSessionsWindow")
    }

    fn files_to_open_or_create(&self) -> Option<Vec<IPath>> {
        let file_to_open = self.payload_truthy("openFile")?;
        let file_uri = URI::parse(file_to_open, false);

        // Support: --goto parameter to open on line/col
        if self.payload_has("gotoLineMode") {
            let path_column_aware = parse_line_and_column_aware(&file_uri.path().to_string_lossy());

            return Some(vec![IPath { file_uri: Some(file_uri.with(&UriChange { path: Field::from(path_column_aware.path.as_str()), ..UriChange::default() })) }]);
        }

        Some(vec![IPath { file_uri: Some(file_uri) }])
    }

    fn files_to_diff(&self) -> Option<Vec<IPath>> {
        let file_to_diff_primary = self.payload_truthy("diffFilePrimary")?;
        let file_to_diff_secondary = self.payload_truthy("diffFileSecondary")?;

        Some(vec![IPath { file_uri: Some(URI::parse(file_to_diff_secondary, false)) }, IPath { file_uri: Some(URI::parse(file_to_diff_primary, false)) }])
    }

    fn files_to_merge(&self) -> Option<Vec<IPath>> {
        let file_to_merge1 = self.payload_truthy("mergeFile1")?;
        let file_to_merge2 = self.payload_truthy("mergeFile2")?;
        let file_to_merge_base = self.payload_truthy("mergeFileBase")?;
        let file_to_merge_result = self.payload_truthy("mergeFileResult")?;

        Some(
            [file_to_merge1, file_to_merge2, file_to_merge_base, file_to_merge_result]
                .into_iter()
                .map(|file| IPath { file_uri: Some(URI::parse(file, false)) })
                .collect(),
        )
    }

    fn debug_renderer(&self) -> bool {
        self.debug_environment().debug_renderer
    }

    fn log_extension_host_communication(&self) -> Option<bool> {
        Some(self.payload_is_true("logExtensionHostCommunication"))
    }

    fn enable_smoke_test_driver(&self) -> Option<bool> {
        self.options.development_options.as_ref().and_then(|options| options.enable_smoke_test_driver)
    }

    fn prof_duration_markers(&self) -> Option<Vec<String>> {
        let prof_duration_markers_from_payload = self.payload_truthy("profDurationMarkers")?;
        let result: Vec<String> = prof_duration_markers_from_payload.split(',').map(str::to_owned).collect();

        if result.len() == 2 { Some(result) } else { None }
    }
}

impl IBrowserWorkbenchEnvironmentService for BrowserWorkbenchEnvironmentService {
    fn options(&self) -> Option<&WorkbenchConstructionOptions> {
        Some(&self.options)
    }

    fn session_title(&self) -> Option<String> {
        self.payload_get("sessionTitle").map(str::to_owned)
    }

    /// `!!this.options.remoteAuthority?.includes('+') && !this.options.webSocketFactory`.
    fn expects_resolver_extension(&self) -> bool {
        self.options.remote_authority.as_deref().is_some_and(|authority| authority.contains('+')) && !self.options.has_web_socket_factory
    }
}
