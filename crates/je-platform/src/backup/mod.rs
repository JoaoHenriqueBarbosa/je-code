//! Porte de `src/vs/platform/backup/common/backup.ts` e `node/backup.ts`: os tipos de informação de backup por
//! workspace, pasta e janela vazia, e a leitura do formato serializado.
//!
//! Fora do porte: `electron-main/backupMainService.ts` (e o teste `backupMainService.test.ts`), que gerencia os
//! backups no processo principal do Electron (janelas, `IWorkspacesManagementMainService`, `IEnvironmentMainService`).

use je_base::uri::URI;

use crate::workspace::WorkspaceIdentifier;

/// `IWorkspaceBackupInfo`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorkspaceBackupInfo {
    pub workspace: WorkspaceIdentifier,
    pub remote_authority: Option<String>,
}

/// `IFolderBackupInfo`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FolderBackupInfo {
    pub folder_uri: URI,
    pub remote_authority: Option<String>,
}

/// `IWorkspaceBackupInfo | IFolderBackupInfo`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BackupInfo {
    Workspace(WorkspaceBackupInfo),
    Folder(FolderBackupInfo),
}

/// `isFolderBackupInfo`.
pub fn is_folder_backup_info(curr: &BackupInfo) -> bool {
    matches!(curr, BackupInfo::Folder(_))
}

/// `isWorkspaceBackupInfo`.
pub fn is_workspace_backup_info(curr: &BackupInfo) -> bool {
    matches!(curr, BackupInfo::Workspace(_))
}

/// `IEmptyWindowBackupInfo`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EmptyWindowBackupInfo {
    pub backup_folder: String,
    pub remote_authority: Option<String>,
}

/// `ISerializedWorkspaceBackupInfo`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SerializedWorkspaceBackupInfo {
    pub id: String,
    pub config_uri_path: String,
    pub remote_authority: Option<String>,
}

/// `ISerializedFolderBackupInfo`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SerializedFolderBackupInfo {
    pub folder_uri: String,
    pub remote_authority: Option<String>,
}

/// `ISerializedBackupWorkspaces`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SerializedBackupWorkspaces {
    pub workspaces: Vec<SerializedWorkspaceBackupInfo>,
    pub folders: Vec<SerializedFolderBackupInfo>,
    pub empty_windows: Vec<EmptyWindowBackupInfo>,
}

/// `deserializeWorkspaceInfos`.
pub fn deserialize_workspace_infos(serialized: &SerializedBackupWorkspaces) -> Vec<WorkspaceBackupInfo> {
    serialized
        .workspaces
        .iter()
        .map(|workspace| WorkspaceBackupInfo {
            workspace: WorkspaceIdentifier { id: workspace.id.clone(), config_path: URI::parse(workspace.config_uri_path.as_str(), false) },
            remote_authority: workspace.remote_authority.clone(),
        })
        .collect()
}

/// `deserializeFolderInfos`.
pub fn deserialize_folder_infos(serialized: &SerializedBackupWorkspaces) -> Vec<FolderBackupInfo> {
    serialized
        .folders
        .iter()
        .map(|folder| FolderBackupInfo { folder_uri: URI::parse(folder.folder_uri.as_str(), false), remote_authority: folder.remote_authority.clone() })
        .collect()
}
