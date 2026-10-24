//! Porte de `src/vs/workbench/contrib/tags/common/workspaceTags.ts`: o contrato do `IWorkspaceTagsService` e
//! `getHashedRemotesFromConfig`.
//!
//! Notas de tradução:
//! - **`Tags`.** `{ [index: string]: boolean | number | string | undefined }` é o [`Dynamic`] de objeto.
//! - **`getTelemetryWorkspaceId(workspace: IWorkspace, state)`** recebe o workspace como `&dyn IWorkspace`.
//! - **`sha1Hex`.** O parâmetro `(str) => Promise<string>` é um `Fn(&str) -> Promise<String>`.

use je_base::dynamic::Dynamic;
use je_base::promise::Promise;
use je_base::uri::URI;
use je_extensions::config_remotes::get_remotes;
use je_instantiation::service_id;
use je_platform::workspace::{IWorkspace, WorkbenchState};

/// `Tags`.
pub type Tags = Dynamic;

/// `IWorkspaceTagsService`.
pub trait IWorkspaceTagsService {
    fn get_tags(&self) -> Promise<Tags>;

    /// Returns an id for the workspace, different from the id returned by the context service. A hash based
    /// on the folder uri or workspace configuration, not time-based, and undefined for empty workspaces.
    fn get_telemetry_workspace_id(&self, workspace: &dyn IWorkspace, state: WorkbenchState) -> Promise<Option<String>>;

    fn get_hashed_remotes_from_uri(&self, workspace_uri: &URI, strip_ending_dot_git: bool) -> Promise<Vec<String>>;
}

service_id!(pub I_WORKSPACE_TAGS_SERVICE: dyn IWorkspaceTagsService = "workspaceTagsService");

/// `getHashedRemotesFromConfig(text, stripEndingDotGit = false, sha1Hex)`.
pub fn get_hashed_remotes_from_config(text: &str, strip_ending_dot_git: bool, sha1_hex: impl Fn(&str) -> Promise<String>) -> Promise<Vec<String>> {
    Promise::all(get_remotes(text, strip_ending_dot_git).iter().map(|remote| sha1_hex(remote)).collect())
}
