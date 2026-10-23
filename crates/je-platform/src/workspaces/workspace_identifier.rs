//! Porte de `src/vs/platform/workspaces/common/workspaceIdentifier.ts`.
//!
//! NOTE: DO NOT CHANGE. IDENTIFIERS HAVE TO REMAIN STABLE.

use je_base::dynamic::Dynamic;
use je_base::hash::hash;
use je_base::uri::URI;

use crate::workspace::{SingleFolderWorkspaceIdentifier, WorkspaceIdentifier};

fn get_workspace_id(uri: &URI) -> String {
    // `hash(...).toString(16)`: o `number` com sinal em base 16, como o JavaScript o escreve.
    let value = hash(&Dynamic::String(uri.to_string(false)));
    if value < 0 { format!("-{:x}", i64::from(value).unsigned_abs()) } else { format!("{value:x}") }
}

/// `getWorkspaceIdentifier`.
pub fn get_workspace_identifier(workspace_uri: &URI) -> WorkspaceIdentifier {
    WorkspaceIdentifier { id: get_workspace_id(workspace_uri), config_path: workspace_uri.clone() }
}

/// `getSingleFolderWorkspaceIdentifier`.
pub fn get_single_folder_workspace_identifier(folder_uri: &URI) -> SingleFolderWorkspaceIdentifier {
    SingleFolderWorkspaceIdentifier { id: get_workspace_id(folder_uri), uri: folder_uri.clone() }
}
