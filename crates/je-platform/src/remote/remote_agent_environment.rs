//! Porte de `src/vs/platform/remote/common/remoteAgentEnvironment.ts`: o ambiente do agente remoto.

use je_base::performance::PerformanceMark;
use je_base::platform::OperatingSystem;
use je_base::uri::URI;

use crate::user_data_profile::UserDataProfile;

/// O `profiles` de `IRemoteAgentEnvironment`: `{ all: IUserDataProfile[]; home: URI }`.
#[derive(Clone, Debug, PartialEq)]
pub struct RemoteAgentProfiles {
    pub all: Vec<UserDataProfile>,
    pub home: URI,
}

/// `IRemoteAgentEnvironment`.
#[derive(Clone, Debug, PartialEq)]
pub struct RemoteAgentEnvironment {
    pub pid: u32,
    pub connection_token: String,
    pub app_root: URI,
    pub exec_path: String,
    pub tmp_dir: URI,
    pub settings_path: URI,
    pub mcp_resource: URI,
    pub logs_path: URI,
    pub extension_host_logs_path: URI,
    pub global_storage_home: URI,
    pub workspace_storage_home: URI,
    pub local_history_home: URI,
    pub user_home: URI,
    pub os: OperatingSystem,
    pub arch: String,
    pub marks: Vec<PerformanceMark>,
    pub use_host_proxy: bool,
    pub profiles: RemoteAgentProfiles,
    pub is_unsupported_glibc: bool,
    pub reconnection_grace_time: Option<f64>,
}

/// `RemoteAgentConnectionContext`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RemoteAgentConnectionContext {
    pub remote_authority: String,
    pub client_id: String,
}
