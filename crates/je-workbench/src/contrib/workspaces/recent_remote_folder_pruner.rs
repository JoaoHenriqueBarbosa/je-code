//! Porte de `src/vs/workbench/contrib/workspaces/browser/recentRemoteFolderPruner.ts`: tira dos recentes a pasta remota
//! aberta quando o sistema de arquivos remoto responde `FILE_NOT_FOUND`.
//!
//! Divergências:
//! - `async function` é uma [`Promise`] criada com `async_fn`; os serviços chegam como `Rc<dyn ...>` para viver dentro dela.
//! - `RecentRemoteFolderPrunerContribution` é a struct de mesmo nome com o `ID`; o construtor dispara a poda e reporta o erro
//!   que não for de cancelamento. O registro na fase `Eventually` do `WorkbenchContributionsRegistry` depende do módulo
//!   `common/contributions`, ainda não portado, e entra com ele.

use std::rc::Rc;

use je_base::async_::async_fn;
use je_base::errors::{is_cancellation_error, on_unexpected_error};
use je_base::network::schemas;
use je_base::promise::Promise;
use je_platform::files::files::{FileOperationResult, to_file_operation_result};
use je_platform::files::i_file_service::IFileService;
use je_platform::workspace::{IWorkspaceContextService, WorkbenchState};
use je_platform::workspaces::IWorkspacesService;

/// Remote counterpart to the local remove-on-failed-open in `WindowsMainService.resolvePath` (see #74465). Prunes the
/// current single-folder workspace's remote folder URI from Recently Opened when the remote file system answers
/// `FILE_NOT_FOUND` (#319230). Transient errors (`FILE_OTHER_ERROR` / `Unavailable`) must not drop entries.
pub fn prune_recent_remote_folder_if_missing(
    context_service: &dyn IWorkspaceContextService,
    file_service: &dyn IFileService,
    workspaces_service: Rc<dyn IWorkspacesService>,
) -> Promise<()> {
    if context_service.get_workbench_state() != WorkbenchState::Folder {
        return Promise::resolved(());
    }

    let Some(folder_uri) = context_service.get_workspace().folders().into_iter().next().map(|folder| folder.uri) else {
        return Promise::resolved(());
    };
    if folder_uri.scheme().to_string_lossy() != schemas::VSCODE_REMOTE {
        return Promise::resolved(()); // only mirror the local remove-on-failed-open for vscode-remote authorities
    }

    let stat = file_service.stat(&folder_uri);
    async_fn(async move {
        if let Err(error) = stat.await {
            // Only prune on an authoritative not-found answer. Any other error (provider not registered / activating, host
            // unreachable, WSL distro down, container not built, etc.) maps to `FILE_OTHER_ERROR` and leaves the entry intact.
            if to_file_operation_result(&error) == FileOperationResult::FileNotFound {
                workspaces_service.remove_recently_opened(vec![folder_uri]).await?;
            }
        }
        Ok(())
    })
}

/// `RecentRemoteFolderPrunerContribution`.
pub struct RecentRemoteFolderPrunerContribution;

impl RecentRemoteFolderPrunerContribution {
    pub const ID: &'static str = "workbench.contrib.recentRemoteFolderPruner";

    pub fn new(context_service: &dyn IWorkspaceContextService, file_service: &dyn IFileService, workspaces_service: Rc<dyn IWorkspacesService>) -> Self {
        let _ = prune_recent_remote_folder_if_missing(context_service, file_service, workspaces_service).catch(|error| {
            if !is_cancellation_error(&error) {
                on_unexpected_error(&error);
            }
        });
        Self
    }
}
