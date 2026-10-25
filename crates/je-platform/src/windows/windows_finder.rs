//! Porte de `src/vs/platform/windows/electron-main/windowsFinder.ts`: acha a janela aberta que cobre um arquivo, uma
//! pasta ou workspace, ou um caminho de desenvolvimento de extensão.
//!
//! Decisões de tradução:
//! - **`ICodeWindow`** é o [`CodeWindow`] (ver [`crate::workspaces::workspaces_management_main_service`]).
//! - **`findWindowOnFile`** é `async` e o resolvedor local de workspace devolve `Promise`; um erro do resolvedor
//!   rejeita a busca, como no original, então a função devolve `Result`.
//! - **`extUriBiasedIgnorePathCase`**, a instância global do original, é [`ext_uri_biased_ignore_path_case`]. A
//!   `findWindowOnWorkspaceOrFolder` recebe a instância por parâmetro porque o `WorkspacesManagementMainService` usa a
//!   dele.
//! - **A ordenação das janelas de pasta única** pelo comprimento do caminho (do maior para o menor, estável) é a escolha do
//!   primeiro maior; o comprimento é em unidades UTF-16, como o `length` do JavaScript.

use std::rc::Rc;

use je_base::errors::Error;
use je_base::promise::Promise;
use je_base::resources::ExtUri;
use je_base::uri::URI;

use crate::resources::{ext_uri_biased_ignore_path_case, uri_file};
use crate::workspace::{AnyWorkspaceIdentifier, ResolvedWorkspace, WorkspaceIdentifier};
use crate::workspaces::workspaces_management_main_service::CodeWindow;

/// `findWindowOnFile`.
pub async fn find_window_on_file(
    windows: &[Rc<dyn CodeWindow>],
    file_uri: &URI,
    local_workspace_resolver: impl Fn(&WorkspaceIdentifier) -> Promise<Option<ResolvedWorkspace>>,
) -> Result<Option<Rc<dyn CodeWindow>>, Error> {
    let ext_uri = ext_uri_biased_ignore_path_case();

    // First check for windows with workspaces that have a parent folder of the provided path opened
    for window in windows {
        if let Some(AnyWorkspaceIdentifier::Workspace(workspace)) = window.opened_workspace() {
            let resolved_workspace = local_workspace_resolver(&workspace).await?;

            match resolved_workspace {
                // resolved workspace: folders are known and can be compared with
                Some(resolved_workspace) => {
                    if resolved_workspace.folders.iter().any(|folder| ext_uri.is_equal_or_parent(file_uri, &folder.uri, false, false)) {
                        return Ok(Some(window.clone()));
                    }
                }

                // unresolved: can only compare with workspace location
                None => {
                    if ext_uri.is_equal_or_parent(file_uri, &workspace.config_path, false, false) {
                        return Ok(Some(window.clone()));
                    }
                }
            }
        }
    }

    // Then go with single folder windows that are parent of the provided file path
    let mut most_specific: Option<(&Rc<dyn CodeWindow>, usize)> = None;
    for window in windows {
        if let Some(AnyWorkspaceIdentifier::SingleFolder(folder)) = window.opened_workspace()
            && ext_uri.is_equal_or_parent(file_uri, &folder.uri, false, false)
        {
            let path_length = folder.uri.path().units().len();
            if most_specific.is_none_or(|(_, longest)| path_length > longest) {
                most_specific = Some((window, path_length));
            }
        }
    }

    Ok(most_specific.map(|(window, _)| window.clone()))
}

/// `findWindowOnWorkspaceOrFolder`.
pub fn find_window_on_workspace_or_folder(windows: &[Rc<dyn CodeWindow>], folder_or_workspace_config_uri: &URI, ext_uri: &ExtUri) -> Option<Rc<dyn CodeWindow>> {
    windows
        .iter()
        .find(|window| match window.opened_workspace() {
            // check for workspace config path
            Some(AnyWorkspaceIdentifier::Workspace(workspace)) => ext_uri.is_equal(Some(&workspace.config_path), Some(folder_or_workspace_config_uri), false),
            // check for folder path
            Some(AnyWorkspaceIdentifier::SingleFolder(folder)) => ext_uri.is_equal(Some(&folder.uri), Some(folder_or_workspace_config_uri), false),
            _ => false,
        })
        .cloned()
}

/// `findWindowOnExtensionDevelopmentPath`.
pub fn find_window_on_extension_development_path(windows: &[Rc<dyn CodeWindow>], extension_development_paths: &[String]) -> Option<Rc<dyn CodeWindow>> {
    let ext_uri = ext_uri_biased_ignore_path_case();
    let matches = |uri_string: &str| extension_development_paths.iter().any(|path| ext_uri.is_equal(Some(&uri_file(path)), Some(&uri_file(uri_string)), false));

    windows
        .iter()
        .find(|window| {
            // match on extension development path. the path can be one or more paths
            // so we check if any of the paths match on any of the provided ones
            window.config().is_some_and(|config| config.borrow().extension_development_path.as_ref().is_some_and(|paths| paths.iter().any(|path| matches(path.as_str()))))
        })
        .cloned()
}
