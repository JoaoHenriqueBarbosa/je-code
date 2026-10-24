//! Porte de `src/vs/workbench/services/history/common/history.ts`: o contrato do serviço de histórico de editores
//! (`IHistoryService`), com os filtros e os escopos da navegação (`GoFilter`, `GoScope`).
//!
//! A implementação (`HistoryService`, `browser/historyService.ts`) está em [`history_service`](super::history_service).
//!
//! Divergências:
//! - **`const enum`** (`GoFilter`, `GoScope`) são enums com o mesmo valor numérico dos membros.
//! - **`readonly (EditorInput | IResourceEditorInput)[]`** é `Vec<EditorInputOrUntyped>` (a união com as entradas sem tipo
//!   em geral, que contém a forma `IResourceEditorInput`).
//! - **`_serviceBrand`** não existe: o identificador do serviço é o [`I_HISTORY_SERVICE`].

use je_base::promise::Promise;
use je_base::uri::URI;
use je_instantiation::service_id;

use crate::common::editor::{EditorInputOrUntyped, GroupIdentifier};

service_id!(pub I_HISTORY_SERVICE: dyn IHistoryService = "historyService");

pub const MOUSE_BACK_FORWARD_NAVIGATION_SETTING: &str = "workbench.editor.mouseBackForwardToNavigate";

/// `GoFilter`: limit editor navigation to certain kinds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GoFilter {
    /// Navigate between editor navigation history entries from any kind of navigation source.
    None = 0,
    /// Only navigate between editor navigation history entries that were resulting from edits.
    Edits = 1,
    /// Only navigate between editor navigation history entries that were resulting from navigations, such as "Go to
    /// definition".
    Navigation = 2,
}

/// `GoScope`: limit editor navigation to certain scopes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GoScope {
    /// Navigate across all editors and editor groups.
    Default = 0,
    /// Navigate only in editors of the active editor group.
    EditorGroup = 1,
    /// Navigate only in the active editor.
    Editor = 2,
}

/// `IHistoryService`.
pub trait IHistoryService {
    /// Navigate forwards in editor navigation history.
    fn go_forward(&self, filter: Option<GoFilter>) -> Promise<()>;

    /// Navigate backwards in editor navigation history.
    fn go_back(&self, filter: Option<GoFilter>) -> Promise<()>;

    /// Navigate between the current editor navigation history entry and the previous one that was navigated to. This
    /// command is like a toggle for `forward` and `back` to jump between 2 points in editor navigation history.
    fn go_previous(&self, filter: Option<GoFilter>) -> Promise<()>;

    /// Navigate to the last entry in editor navigation history.
    fn go_last(&self, filter: Option<GoFilter>) -> Promise<()>;

    /// Re-opens the last closed editor if any.
    fn reopen_last_closed_editor(&self) -> Promise<()>;

    /// Get the entire history of editors that were opened.
    fn get_history(&self) -> Vec<EditorInputOrUntyped>;

    /// Removes an entry from history.
    fn remove_from_history(&self, input: &EditorInputOrUntyped);

    /// Looking at the editor history, returns the workspace root of the last file that was inside the workspace and part
    /// of the editor history.
    ///
    /// `scheme_filter`: filter to restrict roots by scheme.
    fn get_last_active_workspace_root(&self, scheme_filter: Option<&str>, authority_filter: Option<&str>) -> Option<URI>;

    /// Looking at the editor history, returns the resource of the last file that was opened.
    ///
    /// `scheme_filter`: filter to restrict roots by scheme.
    fn get_last_active_file(&self, scheme_filter: &str, authority_filter: Option<&str>) -> Option<URI>;

    /// Opens the next used editor if any.
    ///
    /// `group`: optional indicator to scope to a specific group.
    fn open_next_recently_used_editor(&self, group: Option<GroupIdentifier>) -> Promise<()>;

    /// Opens the previously used editor if any.
    ///
    /// `group`: optional indicator to scope to a specific group.
    fn open_previously_used_editor(&self, group: Option<GroupIdentifier>) -> Promise<()>;

    /// Clears all history.
    fn clear(&self);

    /// Clear list of recently opened editors.
    fn clear_recently_opened(&self);
}
