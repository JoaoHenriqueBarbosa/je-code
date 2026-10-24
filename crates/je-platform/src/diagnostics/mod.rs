//! Porte dos tipos de dados de `src/vs/platform/diagnostics/common/diagnostics.ts`: o que o relator de
//! problemas lê para montar o relatório (`SystemInfo`, `IMachineInfo`, `IRemoteDiagnosticInfo`, ...).
//!
//! Divergências:
//! - O serviço (`IDiagnosticsService`) e a injeção dele não entram aqui: pedem o `IWorkspace` e os processos do
//!   processo principal, e quem consome estes tipos (o modelo do relator) só lê os dados.
//! - `gpuStatus: any` é a lista de pares chave e valor de texto, na ordem em que o original a escreve
//!   (`Object.keys`): o Electron entrega o `getGPUFeatureStatus()` como cadeias (`'enabled'`, `'disabled_off'`...).
//! - `remoteData: (IRemoteDiagnosticInfo | IRemoteDiagnosticError)[]` é o enum [`RemoteDiagnostic`]; o
//!   `isRemoteDiagnosticError` do original decide pelo formato do objeto (`hostName` e `errorMessage`
//!   preenchidos), e aqui a variante já diz qual é, com [`RemoteDiagnostic::as_error`] como type guard.

use indexmap::IndexMap;
use je_base::processes::ProcessItem;

/// `ILinuxEnv`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LinuxEnv {
    pub desktop_session: Option<String>,
    pub xdg_session_desktop: Option<String>,
    pub xdg_current_desktop: Option<String>,
    pub xdg_session_type: Option<String>,
}

/// `IMachineInfo`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MachineInfo {
    pub os: String,
    pub cpus: Option<String>,
    pub memory: String,
    pub vm_hint: String,
    pub linux_env: Option<LinuxEnv>,
}

/// `WorkspaceStatItem`.
#[derive(Debug, Clone, PartialEq)]
pub struct WorkspaceStatItem {
    pub name: String,
    pub count: f64,
}

/// `WorkspaceStats`.
#[derive(Debug, Clone, PartialEq)]
pub struct WorkspaceStats {
    pub file_types: Vec<WorkspaceStatItem>,
    pub config_files: Vec<WorkspaceStatItem>,
    pub file_count: f64,
    pub max_files_reached: bool,
    pub launch_config_files: Vec<WorkspaceStatItem>,
    pub total_scan_time: f64,
    pub total_readdir_count: f64,
}

/// `IDiagnosticInfo`.
#[derive(Debug, Clone, PartialEq)]
pub struct DiagnosticInfo {
    pub machine_info: MachineInfo,
    pub workspace_metadata: Option<IndexMap<String, WorkspaceStats>>,
    pub processes: Option<ProcessItem>,
}

/// O `latency` de `IRemoteDiagnosticInfo`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RemoteLatency {
    pub current: f64,
    pub average: f64,
}

/// `IRemoteDiagnosticInfo`.
#[derive(Debug, Clone, PartialEq)]
pub struct RemoteDiagnosticInfo {
    pub info: DiagnosticInfo,
    pub host_name: String,
    pub latency: Option<RemoteLatency>,
}

/// `IRemoteDiagnosticError`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteDiagnosticError {
    pub host_name: String,
    pub error_message: String,
}

/// `IRemoteDiagnosticInfo | IRemoteDiagnosticError`.
#[derive(Debug, Clone, PartialEq)]
pub enum RemoteDiagnostic {
    Info(RemoteDiagnosticInfo),
    Error(RemoteDiagnosticError),
}

impl RemoteDiagnostic {
    /// `isRemoteDiagnosticError`, como type guard: o erro, quando for um.
    pub fn as_error(&self) -> Option<&RemoteDiagnosticError> {
        match self {
            Self::Error(error) => Some(error),
            Self::Info(_) => None,
        }
    }
}

/// `SystemInfo`.
#[derive(Debug, Clone, PartialEq)]
pub struct SystemInfo {
    pub machine_info: MachineInfo,
    pub process_args: String,
    pub gpu_status: Vec<(String, String)>,
    pub screen_reader: String,
    pub remote_data: Vec<RemoteDiagnostic>,
    pub load: Option<String>,
}
