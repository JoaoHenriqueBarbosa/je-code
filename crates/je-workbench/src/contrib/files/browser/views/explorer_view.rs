//! Porte da parte lógica de `src/vs/workbench/contrib/files/browser/views/explorerView.ts`: `getContext` e
//! `shouldPreserveWorkspaceNameCase` (o `ExplorerView` em si é um `ViewPane` de DOM e não entra).

use std::rc::Rc;

use je_platform::environment::environment::EnvironmentService;
use je_platform::workspace::{IWorkspace, WorkbenchState, is_untitled_workspace};

use super::explorer_viewer::ICompressedNavigationController;
use crate::contrib::files::explorer_model::ExplorerItem;

/// `PRESERVE_WORKSPACE_NAME_CASE_CLASS`.
pub const PRESERVE_WORKSPACE_NAME_CASE_CLASS: &str = "preserve-workspace-name-case";

/// `PRESERVE_MERGED_WORKSPACE_NAME_CASE_CLASS`.
pub const PRESERVE_MERGED_WORKSPACE_NAME_CASE_CLASS: &str = "preserve-merged-workspace-name-case";

/// O `{ getCompressedNavigationController(stat) }` que o `getContext` recebe.
pub trait CompressedNavigationControllerProvider {
    fn get_compressed_navigation_controller(&self, stat: &Rc<ExplorerItem>) -> Option<Vec<Rc<dyn ICompressedNavigationController>>>;
}

/// `controller === compressedNavigationController`: a identidade do objeto (o ponteiro de dados, sem a tabela virtual).
fn same_controller(a: &Rc<dyn ICompressedNavigationController>, b: &Rc<dyn ICompressedNavigationController>) -> bool {
    std::ptr::addr_eq(Rc::as_ptr(a), Rc::as_ptr(b))
}

/// `getContext(focus, selection, respectMultiSelection, compressedNavigationControllerProvider)`.
pub fn get_context(
    focus: &[Rc<ExplorerItem>],
    selection: &[Rc<ExplorerItem>],
    respect_multi_selection: bool,
    compressed_navigation_controller_provider: &dyn CompressedNavigationControllerProvider,
) -> Vec<Rc<ExplorerItem>> {
    let mut focused_stat: Option<Rc<ExplorerItem>> = focus.first().cloned();

    // If we are respecting multi-select and we have a multi-selection we ignore focus as we want to act on the selection
    if respect_multi_selection && selection.len() > 1 {
        focused_stat = None;
    }

    let compressed_navigation_controllers = focused_stat.as_ref().and_then(|stat| compressed_navigation_controller_provider.get_compressed_navigation_controller(stat));
    let compressed_navigation_controller = compressed_navigation_controllers.and_then(|controllers| controllers.into_iter().next());
    if let Some(controller) = &compressed_navigation_controller {
        focused_stat = Some(controller.current());
    }

    let mut selected_stats: Vec<Rc<ExplorerItem>> = Vec::new();

    for stat in selection {
        let controller = compressed_navigation_controller_provider.get_compressed_navigation_controller(stat).and_then(|controllers| controllers.into_iter().next());
        if let (Some(controller), Some(focused)) = (&controller, &focused_stat)
            && compressed_navigation_controller.as_ref().is_some_and(|current| same_controller(controller, current))
        {
            if Rc::ptr_eq(stat, focused) {
                selected_stats.push(stat.clone());
            }
            // Ignore stats which are selected but are part of the same compact node as the focused stat
            continue;
        }

        match controller {
            Some(controller) => selected_stats.extend(controller.items()),
            None => selected_stats.push(stat.clone()),
        }
    }
    let Some(focused_stat) = focused_stat else {
        return if respect_multi_selection { selected_stats } else { Vec::new() };
    };

    if respect_multi_selection && selected_stats.iter().any(|stat| Rc::ptr_eq(stat, &focused_stat)) {
        return selected_stats;
    }

    vec![focused_stat]
}

/// `shouldPreserveWorkspaceNameCase(workbenchState, workspace, environmentService)`: whether the Explorer title shows a
/// name the user provided and therefore has to be rendered with its original casing. Untitled workspaces show a
/// generated label and empty workbenches show a static label, so both keep the default casing.
pub fn should_preserve_workspace_name_case(workbench_state: WorkbenchState, workspace: &dyn IWorkspace, environment_service: &dyn EnvironmentService) -> bool {
    if workbench_state == WorkbenchState::Empty {
        return false;
    }

    match workspace.configuration() {
        None => true,
        Some(configuration) => !is_untitled_workspace(&configuration, &environment_service.untitled_workspaces_home()),
    }
}
