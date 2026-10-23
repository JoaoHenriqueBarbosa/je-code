//! Porte de `src/vs/platform/environment/common/environmentService.ts`: `AbstractNativeEnvironmentService`,
//! `parseExtensionHostDebugPort` e `parseDebugParams`.
//!
//! Decisões de tradução:
//! - **`@memoize`**: os getters memoizados do original são funções puras do `args`, dos `paths` e do
//!   ambiente (`env[...]`), então aqui são calculados a cada chamada, sem cache. O que o `memoize`
//!   congelaria (uma variável de ambiente lida uma vez) muda só se o processo mudar o próprio ambiente.
//! - **`FileAccess.asFileUri('')`** (a raiz do código JavaScript) é [`NativeEnvironmentPaths::app_root`]: o
//!   chamador diz onde a aplicação está instalada. `appRoot` e `builtinExtensionsPath` partem dela.
//! - **`logsHome`** grava `args.logsPath` na primeira chamada, como o original (o objeto de argumentos é
//!   compartilhado).
//! - O ambiente (`process.env`, plataforma, diretório atual) é o [`Environment`] injetado, não o processo, para
//!   os testes de caminhos de outra plataforma.

use std::rc::Rc;

use je_base::date::{local_time_zone, to_local_iso_string};
use je_base::dynamic::{Dynamic, Truthy};
use je_base::json;
use je_base::network::schemas;
use je_base::path::{Environment, PathModule};
use je_base::runtime;
use je_base::uri::{URI, UriChange};

use super::environment::{
    DebugParams, DisableExtensions, EnvironmentService, ExtensionHostDebugParams, ExtensionKind, NativeEnvironmentServiceApi, SyncMode,
};
use super::native_parsed_args::NativeParsedArgs;
use crate::js::from_json;
use crate::product::ProductService;
use crate::resources::join_path;

/// `INativeEnvironmentPaths`.
#[derive(Clone, Debug)]
pub struct NativeEnvironmentPaths {
    /// The user data directory to use for anything that should be persisted except for the content that is
    /// meant for the `homeDir`. Only one instance of VSCode can use the same `userDataDir`.
    pub user_data_dir: String,
    /// The user home directory mainly used for persisting extensions and global configuration that should
    /// be shared across all versions.
    pub home_dir: String,
    /// OS tmp dir.
    pub tmp_dir: String,
    /// Onde a aplicação está instalada (o pai de `FileAccess.asFileUri('')` do original).
    pub app_root: String,
}

/// `EXTENSION_IDENTIFIER_WITH_LOG_REGEX`: `/^([^.]+\..+)[:=](.+)$/` sobre `entry`. Devolve o id da extensão e
/// o nível. O grupo 1 é guloso, então o separador é o último `:` ou `=` que deixa algo depois.
pub fn match_extension_identifier_with_log(entry: &str) -> Option<(String, String)> {
    let first_dot = entry.find('.')?;
    if first_dot == 0 {
        return None;
    }
    // `[^.]+\..+`: depois do primeiro ponto precisa haver ao menos um caractere antes do separador.
    let bytes = entry.as_bytes();
    (first_dot + 2..bytes.len()).rev().find(|&i| (bytes[i] == b':' || bytes[i] == b'=') && i + 1 < bytes.len()).map(|i| (entry[..i].to_owned(), entry[i + 1..].to_owned()))
}

/// `AbstractNativeEnvironmentService`.
pub struct AbstractNativeEnvironmentService {
    args: NativeParsedArgs,
    paths: NativeEnvironmentPaths,
    product_service: ProductService,
    env: Rc<dyn Environment>,
}

impl AbstractNativeEnvironmentService {
    pub fn new(args: NativeParsedArgs, paths: NativeEnvironmentPaths, product_service: ProductService, env: Rc<dyn Environment>) -> Self {
        Self { args, paths, product_service, env }
    }

    fn path(&self) -> PathModule<'_> {
        PathModule::native(self.env.as_ref())
    }

    fn env_var(&self, name: &str) -> Option<String> {
        self.env.env_var(name)
    }

    fn vscode_portable(&self) -> Option<String> {
        self.env_var("VSCODE_PORTABLE").filter(|value| !value.is_empty())
    }

    /// `URI.file(path)`: o esquema `file` com o caminho na plataforma do ambiente.
    fn file_uri(&self, path: &str) -> URI {
        URI::file(path, self.env.platform())
    }


    fn resource_from_argument(&self, value: &str) -> URI {
        // /^[^:/?#]+?:\/\//
        if has_scheme_prefix(value) {
            return URI::parse(value, false);
        }
        self.file_uri(&self.path().normalize(value))
    }

    /// `crashReporterId`.
    pub fn crash_reporter_id(&self) -> Option<String> {
        self.args.get_string("crash-reporter-id")
    }

    /// `crashReporterDirectory`.
    pub fn crash_reporter_directory(&self) -> Option<String> {
        self.args.get_string("crash-reporter-directory")
    }

    /// `disableWorkspaceTrust`.
    pub fn disable_workspace_trust(&self) -> bool {
        self.args.get_bool("disable-workspace-trust")
    }

    /// `debugRenderer`.
    pub fn debug_renderer(&self) -> bool {
        self.args.get_bool("debugRenderer")
    }

    /// `set continueOn(value)`.
    pub fn set_continue_on(&self, value: Option<&str>) {
        self.args.set("continueOn", value.map_or(Dynamic::Undefined, Dynamic::string));
    }
}

/// `/^[^:/?#]+?:\/\//.test(value)`.
fn has_scheme_prefix(value: &str) -> bool {
    matches!(value.find(['/', '?', '#', ':']), Some(index) if index > 0 && value[index..].starts_with("://"))
}

impl EnvironmentService for AbstractNativeEnvironmentService {
    fn state_resource(&self) -> URI {
        join_path(&self.app_settings_home(), &["globalStorage", "storage.json"])
    }

    fn user_roaming_data_home(&self) -> URI {
        self.app_settings_home().with(&UriChange { scheme: schemas::VSCODE_USER_DATA.into(), ..Default::default() })
    }

    fn keyboard_layout_resource(&self) -> URI {
        join_path(&self.user_roaming_data_home(), &["keyboardLayout.json"])
    }

    fn argv_resource(&self) -> URI {
        if let Some(portable) = self.vscode_portable() {
            return self.file_uri(&self.path().join(&[&portable, "argv.json"]));
        }
        join_path(&self.user_home(), &[&self.product_service.data_folder_name, "argv.json"])
    }

    fn untitled_workspaces_home(&self) -> URI {
        self.file_uri(&self.path().join(&[&self.user_data_path(), "Workspaces"]))
    }

    fn workspace_storage_home(&self) -> URI {
        join_path(&self.app_settings_home(), &["workspaceStorage"])
    }

    fn local_history_home(&self) -> URI {
        join_path(&self.app_settings_home(), &["History"])
    }

    fn cache_home(&self) -> URI {
        self.file_uri(&self.user_data_path())
    }

    fn app_shared_data_home(&self) -> URI {
        if let Some(shared) = self.args.get_string("shared-data-dir").filter(|v| !v.is_empty()) {
            return self.file_uri(&self.path().resolve(&[&shared]));
        }
        if let Some(portable) = self.vscode_portable() {
            return self.file_uri(&self.path().join(&[&portable, "shared-data"]));
        }
        join_path(&self.user_home(), &[&self.product_service.shared_data_folder_name])
    }

    fn user_data_sync_home(&self) -> URI {
        join_path(&self.app_settings_home(), &["sync"])
    }

    fn sync(&self) -> Option<SyncMode> {
        match self.args.get_string("sync").as_deref() {
            Some("on") => Some(SyncMode::On),
            Some("off") => Some(SyncMode::Off),
            _ => None,
        }
    }

    fn continue_on(&self) -> Option<String> {
        self.args.get_string("continueOn")
    }

    fn edit_session_id(&self) -> Option<String> {
        self.args.get_string("editSessionId")
    }

    fn debug_extension_host(&self) -> ExtensionHostDebugParams {
        parse_extension_host_debug_port(&self.args, self.is_built())
    }

    fn is_extension_development(&self) -> bool {
        self.args.get("extensionDevelopmentPath").is_truthy()
    }

    fn disable_extensions(&self) -> DisableExtensions {
        if self.args.get_bool("disable-extensions") {
            return DisableExtensions::All(true);
        }

        match self.args.get("disable-extension") {
            Dynamic::String(single) if !single.is_empty() => return DisableExtensions::Some(vec![single.to_string_lossy()]),
            Dynamic::Array(_) => {
                let list = self.args.get_list("disable-extension").unwrap_or_default();
                if !list.is_empty() {
                    return DisableExtensions::Some(list);
                }
            }
            _ => {}
        }

        DisableExtensions::All(false)
    }

    fn skip_builtin_extensions(&self) -> Vec<String> {
        match self.env_var("VSCODE_SKIP_BUILTIN_EXTENSIONS").filter(|value| !value.is_empty()) {
            None => Vec::new(),
            Some(value) => value.split(',').map(|id| id.trim().to_owned()).filter(|id| !id.is_empty()).collect(),
        }
    }

    fn extension_development_location_uri(&self) -> Option<Vec<URI>> {
        let paths = match self.args.get("extensionDevelopmentPath") {
            Dynamic::Array(_) => self.args.get_list("extensionDevelopmentPath")?,
            _ => return None,
        };
        Some(paths.iter().map(|path| self.resource_from_argument(path)).collect())
    }

    fn extension_development_kind(&self) -> Option<Vec<ExtensionKind>> {
        self.args.get_list("extensionDevelopmentKind").map(|kinds| {
            kinds
                .iter()
                .map(|kind| match kind.as_str() {
                    "ui" => ExtensionKind::Ui,
                    "web" => ExtensionKind::Web,
                    _ => ExtensionKind::Workspace,
                })
                .collect()
        })
    }

    fn extension_tests_location_uri(&self) -> Option<URI> {
        self.args.get_string("extensionTestsPath").filter(|path| !path.is_empty()).map(|path| self.resource_from_argument(&path))
    }

    fn logs_home(&self) -> URI {
        if !self.args.get("logsPath").is_truthy() {
            let key = to_local_iso_string(runtime::date_now() as f64, &local_time_zone());
            let key = strip_log_key_separators(&key);
            self.args.set("logsPath", Dynamic::string(&self.path().join(&[&self.user_data_path(), "logs", &key])));
        }

        self.file_uri(&self.args.get_string("logsPath").unwrap_or_default())
    }

    fn log_level(&self) -> Option<String> {
        self.args.get_list("log")?.into_iter().find(|entry| match_extension_identifier_with_log(entry).is_none())
    }

    fn extension_log_level(&self) -> Option<Vec<(String, String)>> {
        let result: Vec<(String, String)> = self
            .args
            .get_list("log")
            .unwrap_or_default()
            .iter()
            .filter_map(|entry| match_extension_identifier_with_log(entry))
            .filter(|(id, level)| !id.is_empty() && !level.is_empty())
            .collect();
        (!result.is_empty()).then_some(result)
    }

    fn verbose(&self) -> bool {
        self.args.get_bool("verbose")
    }

    fn is_built(&self) -> bool {
        self.env_var("VSCODE_DEV").filter(|value| !value.is_empty()).is_none()
    }

    fn disable_telemetry(&self) -> bool {
        self.args.get_bool("disable-telemetry")
    }

    fn disable_experiments(&self) -> bool {
        self.args.get_bool("disable-experiments")
    }

    fn service_machine_id_resource(&self) -> URI {
        join_path(&self.file_uri(&self.user_data_path()), &["machineid"])
    }

    fn agent_sessions_workspace(&self) -> URI {
        join_path(&self.app_settings_home(), &["agent-sessions.code-workspace"])
    }

    fn policy_file(&self) -> Option<URI> {
        if self.args.get_bool("__enable-file-policy") {
            if let Some(portable) = self.vscode_portable() {
                return Some(self.file_uri(&self.path().join(&[&portable, "policy.json"])));
            }
            return Some(join_path(&self.user_home(), &[&self.product_service.data_folder_name, "policy.json"]));
        }
        None
    }
}

/// `.replace(/-|:|\.\d+Z$/g, '')` do carimbo do nome da pasta de logs.
fn strip_log_key_separators(key: &str) -> String {
    let without_suffix = match key.rfind('.') {
        Some(dot) if key.ends_with('Z') && key[dot + 1..key.len() - 1].chars().all(|c| c.is_ascii_digit()) && dot + 2 < key.len() => &key[..dot],
        _ => key,
    };
    without_suffix.chars().filter(|c| *c != '-' && *c != ':').collect()
}

impl NativeEnvironmentServiceApi for AbstractNativeEnvironmentService {
    fn args(&self) -> NativeParsedArgs {
        self.args.clone()
    }

    fn app_root(&self) -> String {
        self.paths.app_root.clone()
    }

    fn user_home(&self) -> URI {
        self.file_uri(&self.paths.home_dir)
    }

    fn app_settings_home(&self) -> URI {
        self.file_uri(&self.path().join(&[&self.user_data_path(), "User"]))
    }

    fn tmp_dir(&self) -> URI {
        self.file_uri(&self.paths.tmp_dir)
    }

    fn user_data_path(&self) -> String {
        self.paths.user_data_dir.clone()
    }

    fn extensions_path(&self) -> String {
        if let Some(cli) = self.args.get_string("extensions-dir").filter(|v| !v.is_empty()) {
            return self.path().resolve(&[&cli]);
        }
        if let Some(extensions) = self.env_var("VSCODE_EXTENSIONS").filter(|v| !v.is_empty()) {
            return extensions;
        }
        if let Some(portable) = self.vscode_portable() {
            return self.path().join(&[&portable, "extensions"]);
        }
        fs_path_in(&join_path(&self.user_home(), &[&self.product_service.data_folder_name, "extensions"]), self.env.as_ref())
    }

    fn extensions_download_location(&self) -> URI {
        if let Some(cli) = self.args.get_string("extensions-download-dir").filter(|v| !v.is_empty()) {
            return self.file_uri(&self.path().resolve(&[&cli]));
        }
        self.file_uri(&self.path().join(&[&self.user_data_path(), "CachedExtensionVSIXs"]))
    }

    fn builtin_extensions_path(&self) -> String {
        if let Some(cli) = self.args.get_string("builtin-extensions-dir").filter(|v| !v.is_empty()) {
            return self.path().resolve(&[&cli]);
        }
        self.path().normalize(&self.path().join(&[&self.paths.app_root, "extensions"]))
    }

    fn use_in_memory_secret_storage(&self) -> bool {
        self.args.get_bool("use-inmemory-secretstorage")
    }

    fn cross_origin_isolated(&self) -> bool {
        false
    }

    fn export_policy_data(&self) -> Option<String> {
        self.args.get_string("export-policy-data")
    }

    fn export_default_keybindings(&self) -> Option<String> {
        self.args.get_string("export-default-keybindings")
    }
}

fn fs_path_in(uri: &URI, env: &dyn Environment) -> String {
    uri.fs_path(env.platform()).to_string_lossy()
}

/// `parseExtensionHostDebugPort(args, isBuilt)`.
pub fn parse_extension_host_debug_port(args: &NativeParsedArgs, is_built: bool) -> ExtensionHostDebugParams {
    parse_debug_params(
        args.get_string("inspect-extensions").as_deref(),
        args.get_string("inspect-brk-extensions").as_deref(),
        5870,
        is_built,
        args.get_string("debugId"),
        args.get_string("extensionEnvironment").as_deref(),
    )
}

/// `parseDebugParams(...)`.
pub fn parse_debug_params(
    debug_arg: Option<&str>,
    debug_brk_arg: Option<&str>,
    default_build_port: u16,
    is_built: bool,
    debug_id: Option<String>,
    environment_string: Option<&str>,
) -> ExtensionHostDebugParams {
    let port_str = debug_brk_arg.filter(|v| !v.is_empty()).or(debug_arg);
    // `Number(portStr) || (!isBuilt ? defaultBuildPort : null)`
    let number = port_str.map_or(f64::NAN, js_number);
    let port = if number != 0.0 && !number.is_nan() { Some(number) } else if !is_built { Some(f64::from(default_build_port)) } else { None };
    let break_ = port.is_some() && debug_brk_arg.is_some_and(|v| !v.is_empty());
    let env = environment_string.filter(|v| !v.is_empty()).and_then(|text| {
        let mut errors = Vec::new();
        let value = json::parse_str(text, &mut errors)?;
        errors.is_empty().then(|| from_json(&value))
    });

    ExtensionHostDebugParams { port, break_, debug_id, env }
}

/// `Number(text)` para o texto de uma porta.
fn js_number(text: &str) -> f64 {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return 0.0;
    }
    if let Some(hex) = trimmed.strip_prefix("0x").or_else(|| trimmed.strip_prefix("0X")) {
        return u64::from_str_radix(hex, 16).map_or(f64::NAN, |n| n as f64);
    }
    if trimmed.chars().all(|c| c.is_ascii_digit() || matches!(c, '.' | 'e' | 'E' | '+' | '-')) { trimmed.parse().unwrap_or(f64::NAN) } else { f64::NAN }
}

/// `IDebugParams` a partir de [`ExtensionHostDebugParams`], sem `debugId` e `env`.
pub fn to_debug_params(params: &ExtensionHostDebugParams) -> DebugParams {
    DebugParams { port: params.port, break_: params.break_ }
}

