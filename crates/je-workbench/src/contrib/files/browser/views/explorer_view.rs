//! Porte de `src/vs/workbench/contrib/files/browser/views/explorerView.ts`: `getContext`, `shouldPreserveWorkspaceNameCase`
//! e a lógica da vista que não depende de DOM nem da árvore desenhada (decisões do menu de contexto, chaves de contexto,
//! colapso, escolha da entrada da árvore, opções de `createTree`). O `ExplorerView` como `ViewPane` fica de fora: o porte
//! não tem `ViewPane`, `WorkbenchCompressibleAsyncDataTree` nem os `menuActionOptions`/`getActionsContext` do
//! `IContextMenuMenuDelegate`.

use std::rc::Rc;

use je_base::uri::URI;
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

/// `ExplorerView.TREE_VIEW_STATE_STORAGE_KEY`.
pub const TREE_VIEW_STATE_STORAGE_KEY: &str = "workbench.explorer.treeViewState";

/// O que a `WorkbenchCompressibleAsyncDataTree<ExplorerItem | ExplorerItem[], ExplorerItem, FuzzyScore>` oferece às
/// funções livres e à lógica de colapso da vista (`hasNode`, `isCollapsed`, `isCollapsible`, `collapse`).
pub trait ExplorerTreeNodes {
    fn has_node(&self, item: &Rc<ExplorerItem>) -> bool;
    fn is_collapsed(&self, item: &Rc<ExplorerItem>) -> bool;
    fn is_collapsible(&self, item: &Rc<ExplorerItem>) -> bool;
    fn collapse(&self, item: &Rc<ExplorerItem>, recursive: bool) -> bool;
}

/// `hasExpandedRootChild(tree, treeInput)`.
pub fn has_expanded_root_child(tree: &dyn ExplorerTreeNodes, tree_input: &[Rc<ExplorerItem>]) -> bool {
    for folder in tree_input {
        if tree.has_node(folder) && !tree.is_collapsed(folder) {
            for child in folder.children() {
                if tree.has_node(&child) && tree.is_collapsible(&child) && !tree.is_collapsed(&child) {
                    return true;
                }
            }
        }
    }
    false
}

/// `hasExpandedNode(tree, treeInput)`: whether or not any of the nodes in the tree are expanded.
pub fn has_expanded_node(tree: &dyn ExplorerTreeNodes, tree_input: &[Rc<ExplorerItem>]) -> bool {
    tree_input.iter().any(|folder| tree.has_node(folder) && !tree.is_collapsed(folder))
}

/// O `ExplorerItem | ExplorerItem[]` que a árvore recebe como entrada.
#[derive(Clone)]
pub enum ExplorerTreeInput {
    Item(Rc<ExplorerItem>),
    Roots(Vec<Rc<ExplorerItem>>),
}

impl ExplorerTreeInput {
    /// `Array.isArray(treeInput) ? treeInput : Array.from(treeInput.children.values())` (`updateAnyCollapsedContext`).
    pub fn top_level_items(&self) -> Vec<Rc<ExplorerItem>> {
        match self {
            ExplorerTreeInput::Roots(roots) => roots.clone(),
            ExplorerTreeInput::Item(item) => item.children(),
        }
    }
}

/// `updateAnyCollapsedContext`: o valor de `viewHasSomeCollapsibleRootItem`.
pub fn has_some_collapsible_root_item(tree: &dyn ExplorerTreeNodes, tree_input: &ExplorerTreeInput) -> bool {
    has_expanded_node(tree, &tree_input.top_level_items())
}

/// `collapseAll()` depois do foco: com várias raízes e algum filho de raiz expandido recolhe só os filhos de raiz e
/// devolve `true`; `false` manda a vista chamar `tree.collapseAll()`.
pub fn collapse_root_children(tree: &dyn ExplorerTreeNodes, tree_input: &ExplorerTreeInput) -> bool {
    if let ExplorerTreeInput::Roots(roots) = tree_input
        && has_expanded_root_child(tree, roots)
    {
        for folder in roots {
            for child in folder.children() {
                if tree.has_node(&child) {
                    tree.collapse(&child, true);
                }
            }
        }
        return true;
    }
    false
}

/// `doSetTreeInput`: `input = roots[0]`, e as raízes quando o espaço não é de uma pasta ou a primeira raiz tem erro.
pub fn choose_tree_input(workbench_state: WorkbenchState, roots: &[Rc<ExplorerItem>], first_root_has_error: bool) -> ExplorerTreeInput {
    if workbench_state != WorkbenchState::Folder || first_root_has_error {
        ExplorerTreeInput::Roots(roots.to_vec())
    } else {
        ExplorerTreeInput::Item(roots[0].clone())
    }
}

/// `doSetTreeInput`: quantas raízes expandir quando não há estado da vista (`Math.min(input.length, 5)`, #176226).
pub fn roots_to_expand_without_view_state(roots_len: usize) -> usize {
    roots_len.min(5)
}

/// `isCompressionEnabled()` de `createTree`: o leitor de tela desliga as pastas compactas.
pub fn is_compression_enabled(config_value: bool, screen_reader_optimized: bool) -> bool {
    if screen_reader_optimized {
        return false;
    }
    config_value
}

/// `collapseByDefault` de `createTree` para um `ExplorerItem`.
pub fn collapse_by_default(has_nests: bool, expand_nests: bool, showing_filter_results: bool) -> bool {
    if has_nests && expand_nests {
        return false;
    }
    !showing_filter_results
}

/// `expandOnlyOnTwistieClick` de `createTree` para um `ExplorerItem`.
pub fn expand_only_on_twistie_click(has_nests: bool, expand_mode_is_double_click: bool) -> bool {
    has_nests || expand_mode_is_double_click
}

/// `updateCompressedNavigationContextKeys`: `(compressedFocusFirst, compressedFocusLast)`.
pub fn compressed_navigation_context_keys(controller: &dyn ICompressedNavigationController) -> (bool, bool) {
    (controller.index() == 0, controller.index() + 1 == controller.count())
}

/// O que `setContextKeys` grava para um item (ou para a raiz mais próxima quando não há item).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExplorerContextKeyValues {
    pub folder: bool,
    pub readonly: bool,
    pub parent_readonly: bool,
    pub root: bool,
}

impl ExplorerContextKeyValues {
    /// `!!stat && stat.isDirectory`, `!!stat && !!stat.isReadonly`, `Boolean(stat?.parent?.isReadonly)` e
    /// `!!stat && stat.isRoot`.
    pub fn of(stat: Option<&Rc<ExplorerItem>>) -> Self {
        Self {
            folder: stat.is_some_and(|stat| stat.is_directory()),
            readonly: stat.is_some_and(|stat| stat.is_readonly().is_readonly()),
            parent_readonly: stat.and_then(|stat| stat.parent()).is_some_and(|parent| parent.is_readonly().is_readonly()),
            root: stat.is_some_and(|stat| stat.is_root()),
        }
    }
}

/// `onContextMenu`: o item clicado fora da seleção a substitui (sem repassar o evento de teclado, para não abri-lo).
/// `None` deixa a seleção como está.
pub fn selection_for_context_menu(stat: Option<&Rc<ExplorerItem>>, selection: &[Rc<ExplorerItem>]) -> Option<Vec<Rc<ExplorerItem>>> {
    let stat = stat?;
    if selection.iter().any(|selected| Rc::ptr_eq(selected, stat)) { None } else { Some(vec![stat.clone()]) }
}

/// O argumento do menu de contexto (`arg`): o recurso do item (o corrente do nó comprimido, se houver), ou o da raiz
/// única, ou o objeto vazio.
#[derive(Clone, Debug, PartialEq)]
pub enum ContextMenuArg {
    Resource(URI),
    Empty,
}

/// `arg` de `onContextMenu`.
pub fn context_menu_arg(
    stat: Option<&Rc<ExplorerItem>>,
    compressed_navigation_controller_provider: &dyn CompressedNavigationControllerProvider,
    roots: &[Rc<ExplorerItem>],
) -> ContextMenuArg {
    match stat {
        Some(stat) => {
            let controllers = compressed_navigation_controller_provider.get_compressed_navigation_controller(stat);
            match controllers.as_ref().and_then(|controllers| controllers.first()) {
                Some(controller) => ContextMenuArg::Resource(controller.current().resource()),
                None => ContextMenuArg::Resource(stat.resource()),
            }
        }
        None if roots.len() == 1 => ContextMenuArg::Resource(roots[0].resource()),
        None => ContextMenuArg::Empty,
    }
}

/// `getActionsContext` de `onContextMenu`: a seleção quando o item pertence a ela, senão só o item, senão nada.
pub fn context_menu_actions_context(stat: Option<&Rc<ExplorerItem>>, selection: &[Rc<ExplorerItem>]) -> Vec<URI> {
    match stat {
        Some(stat) if selection.iter().any(|selected| Rc::ptr_eq(selected, stat)) => selection.iter().map(|item| item.resource()).collect(),
        Some(stat) => vec![stat.resource()],
        None => Vec::new(),
    }
}

/// Onde o menu de contexto ancora num item comprimido: o rótulo do segmento corrente (teclado ou clique no nome da pasta
/// comprimida) ou o último segmento (qualquer outro clique, que também chama `last()` nos controladores).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CompressedAnchor {
    /// `controllers[0].labels[controllers[0].index]`.
    CurrentLabel,
    /// `controllers.forEach(controller => controller.last())`.
    LastSegment,
}

/// A decisão de ancoragem de `onContextMenu` para um item com controladores de navegação comprimida.
pub fn compressed_anchor(is_keyboard_event: bool, target_is_compressed_folder_name: bool) -> CompressedAnchor {
    if is_keyboard_event || target_is_compressed_folder_name { CompressedAnchor::CurrentLabel } else { CompressedAnchor::LastSegment }
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
