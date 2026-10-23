//! Porte de `src/vs/platform/environment/common/environment.ts`: `IDebugParams`,
//! `IExtensionHostDebugParams`, `ExtensionKind` e as interfaces `IEnvironmentService` e
//! `INativeEnvironmentService`.
//!
//! `IEnvironmentService` é o trait [`EnvironmentService`] (o que cabe em todos os processos, inclusive
//! na web) e `INativeEnvironmentService` o [`NativeEnvironmentServiceApi`] (só o que existe em Windows,
//! Linux e macOS). Os caminhos que o original dá como `string` ou `URI` seguem assim.

use je_base::dynamic::Dynamic;
use je_base::uri::URI;

use super::native_parsed_args::NativeParsedArgs;

/// `IDebugParams`.
#[derive(Clone, Debug, PartialEq)]
pub struct DebugParams {
    pub port: Option<f64>,
    pub break_: bool,
}

/// `IExtensionHostDebugParams`: `debugId` e `env` (o objeto do `--extensionEnvironment`).
#[derive(Clone, Debug, PartialEq)]
pub struct ExtensionHostDebugParams {
    pub port: Option<f64>,
    pub break_: bool,
    pub debug_id: Option<String>,
    pub env: Option<Dynamic>,
}

/// `ExtensionKind`: `'ui' | 'workspace' | 'web'`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExtensionKind {
    Ui,
    Workspace,
    Web,
}

/// `disableExtensions: boolean | string[]`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DisableExtensions {
    All(bool),
    Some(Vec<String>),
}

/// `sync: 'on' | 'off' | undefined`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SyncMode {
    On,
    Off,
}

/// `IEnvironmentService`.
pub trait EnvironmentService {
    // --- user roaming data
    fn state_resource(&self) -> URI;
    fn user_roaming_data_home(&self) -> URI;
    fn keyboard_layout_resource(&self) -> URI;
    fn argv_resource(&self) -> URI;

    // --- data paths
    fn untitled_workspaces_home(&self) -> URI;
    fn workspace_storage_home(&self) -> URI;
    fn local_history_home(&self) -> URI;
    fn cache_home(&self) -> URI;
    fn app_shared_data_home(&self) -> URI;

    // --- settings sync
    fn user_data_sync_home(&self) -> URI;
    fn sync(&self) -> Option<SyncMode>;

    // --- continue edit session
    fn continue_on(&self) -> Option<String>;
    fn edit_session_id(&self) -> Option<String>;

    // --- extension development
    fn debug_extension_host(&self) -> ExtensionHostDebugParams;
    fn is_extension_development(&self) -> bool;
    fn disable_extensions(&self) -> DisableExtensions;
    fn skip_builtin_extensions(&self) -> Vec<String>;
    fn extension_development_location_uri(&self) -> Option<Vec<URI>>;
    fn extension_development_kind(&self) -> Option<Vec<ExtensionKind>>;
    fn extension_tests_location_uri(&self) -> Option<URI>;

    // --- logging
    fn logs_home(&self) -> URI;
    fn log_level(&self) -> Option<String>;
    fn extension_log_level(&self) -> Option<Vec<(String, String)>>;
    fn verbose(&self) -> bool;
    fn is_built(&self) -> bool;

    // --- telemetry/exp
    fn disable_telemetry(&self) -> bool;
    fn disable_experiments(&self) -> bool;
    fn service_machine_id_resource(&self) -> URI;

    // --- agent sessions workspace
    fn agent_sessions_workspace(&self) -> URI;

    // --- Policy
    fn policy_file(&self) -> Option<URI>;
}

/// `INativeEnvironmentService`.
pub trait NativeEnvironmentServiceApi: EnvironmentService {
    // --- CLI Arguments
    fn args(&self) -> NativeParsedArgs;

    // --- data paths
    /// Root path of the JavaScript sources. Note: This is NOT the installation root directory itself but
    /// contained in it at a level that is platform dependent.
    fn app_root(&self) -> String;
    fn user_home(&self) -> URI;
    fn app_settings_home(&self) -> URI;
    fn tmp_dir(&self) -> URI;
    fn user_data_path(&self) -> String;

    // --- extensions
    fn extensions_path(&self) -> String;
    fn extensions_download_location(&self) -> URI;
    fn builtin_extensions_path(&self) -> String;

    // --- use in-memory Secret Storage
    fn use_in_memory_secret_storage(&self) -> bool;

    fn cross_origin_isolated(&self) -> bool;
    fn export_policy_data(&self) -> Option<String>;
    fn export_default_keybindings(&self) -> Option<String>;
}
