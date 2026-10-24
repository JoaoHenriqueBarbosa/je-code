//! `extHostTypeConverters.ts`: `TextDocumentSaveReason`, entre o motivo de salvamento do workbench e o da API.

use crate::api::common::ext_host_types as types;
use crate::common::editor::SaveReason;

/// `typeConverters.TextDocumentSaveReason`.
pub struct TextDocumentSaveReason;

impl TextDocumentSaveReason {
    pub fn to(reason: SaveReason) -> types::TextDocumentSaveReason {
        match reason {
            SaveReason::Auto => types::TextDocumentSaveReason::AfterDelay,
            SaveReason::Explicit => types::TextDocumentSaveReason::Manual,
            SaveReason::FocusChange | SaveReason::WindowChange => types::TextDocumentSaveReason::FocusOut,
        }
    }
}
