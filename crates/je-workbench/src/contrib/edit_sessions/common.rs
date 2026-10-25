//! Porte de `src/vs/workbench/contrib/editSessions/common/editSessions.ts`, sem os serviços (`IEditSessionsStorageService` e
//! `IEditSessionsLogService`), que dependem do cliente de armazenamento ainda não portado.
//!
//! Divergências:
//! - `Change` (união de `Addition` e `Deletion`) é um enum com o `contents` presente só na adição.
//! - `EditSession.workspaceStateId` e `machine` são `Option<String>`.

use je_base::buffer::{VSBuffer, decode_base64};
use je_base::errors::Error;
use je_base::hash::StringSha1;

pub const EDIT_SESSION_SYNC_CATEGORY: &str = "Cloud Changes";

/// `ChangeType`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChangeType {
    Addition = 1,
    Deletion = 2,
}

/// `FileType`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileType {
    File = 1,
}

/// `Change`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Change {
    Addition { relative_file_path: String, file_type: FileType, contents: String },
    Deletion { relative_file_path: String, file_type: FileType },
}

impl Change {
    pub fn relative_file_path(&self) -> &str {
        match self {
            Self::Addition { relative_file_path, .. } | Self::Deletion { relative_file_path, .. } => relative_file_path,
        }
    }

    pub fn change_type(&self) -> ChangeType {
        match self {
            Self::Addition { .. } => ChangeType::Addition,
            Self::Deletion { .. } => ChangeType::Deletion,
        }
    }
}

/// `Folder`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Folder {
    pub name: String,
    pub canonical_identity: Option<String>,
    pub working_changes: Vec<Change>,
    pub absolute_uri: Option<String>,
}

pub const EDIT_SESSION_SCHEMA_VERSION: u32 = 3;

/// `EditSession`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EditSession {
    pub version: u32,
    pub workspace_state_id: Option<String>,
    pub machine: Option<String>,
    pub folders: Vec<Folder>,
}

pub const EDIT_SESSIONS_SIGNED_IN_KEY: &str = "editSessionsSignedIn";
pub const EDIT_SESSIONS_PENDING_KEY: &str = "editSessionsPending";
pub const EDIT_SESSIONS_CONTAINER_ID: &str = "workbench.view.editSessions";
pub const EDIT_SESSIONS_DATA_VIEW_ID: &str = "workbench.views.editSessions.data";
pub const EDIT_SESSIONS_TITLE: &str = "Cloud Changes";
pub const EDIT_SESSIONS_SCHEME: &str = "vscode-edit-sessions";
pub const EDIT_SESSIONS_LOG_ID: &str = "editSessions";

/// `decodeEditSessionFileContent(version, content)`.
pub fn decode_edit_session_file_content(version: u32, content: &str) -> Result<VSBuffer, Error> {
    match version {
        1 => Ok(VSBuffer::from_string(content)),
        2 => Ok(decode_base64(content)),
        _ => Err(Error::new("Upgrade to a newer version to decode this content.")),
    }
}

/// `hashedEditSessionId(editSessionId)`.
pub fn hashed_edit_session_id(edit_session_id: &str) -> String {
    let mut sha1 = StringSha1::new();
    let units: Vec<u16> = edit_session_id.encode_utf16().collect();
    sha1.update(&units);
    sha1.digest()
}
