//! Porte de `src/vs/platform/workspace/common/virtualWorkspace.ts`: workspaces virtuais, os que não estão no disco
//! local nem num servidor remoto do VS Code (o esquema das pastas não é `file` nem `vscode-remote`).
//!
//! `{ scheme: string; authority: string } | undefined` de `getVirtualWorkspaceLocation` é o `URI` que o original
//! devolve (ele tem os dois campos).

use je_base::network::schemas;
use je_base::uri::URI;

use crate::workspace::IWorkspace;

/// `isVirtualResource`.
pub fn is_virtual_resource(resource: &URI) -> bool {
    resource.scheme() != schemas::FILE && resource.scheme() != schemas::VSCODE_REMOTE
}

/// `getVirtualWorkspaceLocation`.
pub fn get_virtual_workspace_location(workspace: &dyn IWorkspace) -> Option<URI> {
    let folders = workspace.folders();
    if !folders.is_empty() {
        return if folders.iter().all(|folder| is_virtual_resource(&folder.uri)) { Some(folders[0].uri.clone()) } else { None };
    }
    workspace.configuration().filter(is_virtual_resource)
}

/// `getVirtualWorkspaceScheme`.
pub fn get_virtual_workspace_scheme(workspace: &dyn IWorkspace) -> Option<String> {
    get_virtual_workspace_location(workspace).map(|location| location.scheme().to_string_lossy())
}

/// `getVirtualWorkspaceAuthority`.
pub fn get_virtual_workspace_authority(workspace: &dyn IWorkspace) -> Option<String> {
    get_virtual_workspace_location(workspace).map(|location| location.authority().to_string_lossy())
}

/// `isVirtualWorkspace`.
pub fn is_virtual_workspace(workspace: &dyn IWorkspace) -> bool {
    get_virtual_workspace_location(workspace).is_some()
}
