//! Porte de `src/vs/workbench/contrib/files/browser/views/explorerView.ts`: `getContext`, `shouldPreserveWorkspaceNameCase`,
//! a lógica da vista que não depende de DOM nem da árvore desenhada (decisões do menu de contexto, chaves de contexto,
//! colapso, escolha da entrada da árvore, opções de `createTree`) e o [`ExplorerView`], a `ViewPane` do explorador, até o
//! fim de `createTree`.
//!
//! Divergências do [`ExplorerView`]:
//! - **Herança.** `ExplorerView extends ViewPane` é um [`ExplorerView`] que guarda a [`ViewPane`] (campo `pane`) e implementa o
//!   [`ViewPaneDelegate`] dela: `renderHeader`, `renderBody` e `layoutBody` entram pelo delegado. Os membros que o original
//!   sobrescreve como propriedade (`title`, `singleViewPaneContainerTitle`, `headerVisible`, `setVisible`) são métodos daqui que
//!   chamam a `ViewPane`: quem usa a vista passa por eles.
//! - **Injeção.** Os 25 serviços do construtor e os de `createInstance` (`FilesFilter`, `ExplorerFindProvider`, `FilesRenderer`,
//!   `FileDragAndDrop`, `ExplorerDataSource`, `ResourceLabels`, `ResourceContextKey`) são os campos de [`ExplorerViewServices`]. O que o
//!   porte ainda não tem como serviço (o tema de ícones de arquivo, o `ResourceContextKey`, o tema de cores) entra como fornecedor
//!   ou fábrica, como os de [`ViewPaneServices`].
//! - **Chaves de contexto.** As do `ExplorerView` ligam ao serviço de chaves com escopo da [`ViewPane`], que é o que o
//!   `contextKeyService` injetado é no original.
//! - **Entrada da árvore.** A `ExplorerDataSource` do porte tem `ExplorerInput` (a lista de itens do topo) como entrada: a árvore
//!   é uma `WorkbenchCompressibleAsyncDataTree<ExplorerInput, ExplorerItem, ()>`, e `updateAnyCollapsedContext` lê a entrada como os
//!   itens do topo (`Array.from(treeInput.children.values())` do original é a própria lista quando a entrada é uma pasta).
//! - **Opções da árvore que o widget não tem.** `accessibilityProvider`, `dnd` e `findProvider` não existem nas opções da
//!   `AsyncDataTree` do `je-widgets`: o `FilesRenderer`, o `FileDragAndDrop` e o `ExplorerFindProvider` são criados e guardados pela
//!   vista (e o provedor de busca acompanha a árvore por [`ExplorerTreeHandle`]), mas só passam a valer na árvore quando o widget os
//!   aceitar. `overrideStyles` chega resolvido por [`ExplorerViewServices::list_override_styles`].
//! - **Adaptador do renderer.** O `FilesRenderer` desenha por item; [`FilesRendererAdapter`] o liga aos nós da árvore de dados
//!   assíncrona e comprimível.
//! - **Eventos de DOM.** Não há `dragover` no cabeçalho (`DelayedDragHandler`) nem `paste` com `clipboardData` na janela nativa: o
//!   cabeçalho não expande no arrasto e a colagem de arquivos do sistema fica pelo comando `filesExplorer.paste`.
//! - **Abrir o item.** `SIDE_GROUP` e `ACTIVE_GROUP` são `PreferredGroup::Side` e `PreferredGroup::Active`.
//! - **`tryGetRelativeTop`.** Sem `try/catch`, o item que a árvore ainda não conhece (`hasNode` falso) vale `null`.
//! - **Estado da vista em JSON.** `JSON.stringify` e `JSON.parse` do `IAsyncDataTreeViewState` são
//!   [`tree_view_state_to_json`] e [`tree_view_state_from_json`] (a segunda devolve `None` para um texto inválido).
//!
//! Fica para a parte B, no mesmo módulo, o que o `createTree` e o `renderBody` chamam, com estas assinaturas (métodos em
//! `impl ExplorerView`, sobre `self: &Rc<Self>` quando o corpo precisa do `Rc`): `refresh(&self, recursive: bool, item:
//! Option<&Rc<ExplorerItem>>, cancel_editing: bool) -> Promise<()>`, `set_tree_input(self: &Rc<Self>) -> Promise<()>`,
//! `select_active_file(self: &Rc<Self>, reveal: Option<ExplorerReveal>) -> Promise<()>`, `on_context_menu(&self, event:
//! &ExplorerContextMenuEvent)`, o `impl IExplorerView for ExplorerView` (o construtor passa `this.clone()` ao `registerView`)
//! e o resto de `selectResource`, `setEditable`, `itemsCopied`, `expandAll`, `collapseAll`, as navegações comprimidas e
//! `hasPhantomElements`. Os campos que ela usa já existem: `set_tree_input_sequencer`, `horizontal_scrolling`, `tree_container`,
//! `file_copied_context_key`, `resource_cut_context_key`, `drag_and_drop`, `find_provider`, `filter`, `renderer`, `services`.

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use je_base::async_::{Sequencer, async_fn};
use je_base::dynamic::{Dynamic, Truthy};
use je_base::errors::{self, Error};
use je_base::event::Event;
use je_base::lifecycle::{Disposable, DisposableRef, IDisposable, to_disposable};
use je_base::nls::localize;
use je_base::promise::Promise;
use je_base::uri::URI;
use je_editor::services::clipboard_service::IClipboardService;
use je_platform::accessibility::IAccessibilityService;
use je_platform::commands::commands::ICommandService;
use je_platform::configuration::ConfigurationOverrides;
use je_platform::contextkey::contextkey::{IContextKey, IContextKeyService};
use je_platform::dialogs::dialogs::IDialogService;
use je_platform::environment::environment::EnvironmentService;
use je_platform::files::files::FileSystemProviderCapabilities;
use je_platform::files::i_file_service::IFileService;
use je_platform::label::label::{ILabelService, WorkspaceLabelTarget};
use je_platform::notification::notification::INotificationService;
use je_platform::opener::editor_types::{EditorOpenSource, EditorOptions};
use je_platform::progress::progress::IProgressService;
use je_platform::storage::storage::{StorageScope, StorageService, StorageTarget};
use je_platform::telemetry::telemetry::ITelemetryService;
use je_platform::uri_identity::uri_identity::IUriIdentityService;
use je_platform::workspace::{IWorkspace, IWorkspaceContextService, WorkbenchState, is_untitled_workspace};
use je_storage::StorageValue;
use je_ui::{NodeId, Role, Ui};
use je_widgets::dom::{create_el, is_ancestor_of_active_element};
use je_widgets::list::list_styles::IListStyles;
use je_widgets::list::list_types::{BrowserEvent, IIdentityProvider, IKeyboardNavigationLabelProvider, TemplateData};
use je_widgets::tree::abstract_tree::{AbstractTreePart, ExpandOnlyOnTwistieClick};
use je_widgets::tree::async_data_tree_node::{AsyncDataTreeNodeWrapper, IAsyncDataTreeRenderer, IAsyncDataTreeViewState};
use je_widgets::tree::compressible_async_data_tree::{
    CompressibleAsyncDataTreeNodeWrapper, CompressibleAsyncDataTreeOptions, ICompressibleAsyncDataTreeRenderer, ICompressibleAsyncKeyboardNavigationLabelProvider,
};
use je_widgets::tree::tree_types::{InputOrElement, TreeElementRenderDetails, TreeFilterResult, TreeMouseEventTarget, TreeNodeApi, TreeVisibility};

use super::explorer_viewer::{
    ExplorerCompressionDelegate, ExplorerDelegate, FileSorter, ICompressedNavigationController, IExplorerFindHighlightTree,
};
use super::explorer_viewer_data::{ExplorerDataSource, ExplorerFindActiveKey, ExplorerFindProvider, ExplorerFindTree, ExplorerInput, FilesFilter};
use super::explorer_viewer_dnd::{ExternalFileDropHandler, FileDragAndDrop, FileDragAndDropServices};
use super::explorer_viewer_renderer::{FILES_RENDERER_TEMPLATE_ID, FileIconThemeInfo, FilesRenderer, FilesRendererServices, InlineEditServices};
use crate::browser::labels::{IResourceLabelsContainer, ResourceLabels, ResourceLabelsServices};
use crate::browser::parts::views::view_pane::{IViewPaneOptions, ListOverrideStyleColors, ViewPane, ViewPaneDelegate, ViewPaneServices};
use crate::browser::workbench_tree::{IWorkbenchTreeOptions, WorkbenchCompressibleAsyncDataTree, WorkbenchTreeServices};
use crate::common::editor::{TextEditorOptions, UntypedEditorInput};
use crate::contrib::files::browser_files::{ExplorerReveal, IExplorerService};
use crate::contrib::files::common_files::{
    explorer_compressed_first_focus_context, explorer_compressed_focus_context, explorer_compressed_last_focus_context, explorer_find_provider_active, explorer_focused_context,
    explorer_folder_context, explorer_resource_available_editor_ids_context, explorer_resource_cut, explorer_resource_moveable_to_trash, explorer_resource_parent_read_only_context,
    explorer_resource_readonly_context, explorer_root_context, files_explorer_focused_context, folders_view_visible_context, view_has_some_collapsible_root_item_context,
};
use crate::contrib::files::explorer_model::ExplorerItem;
use crate::services::editor::editor_groups_service::PreferredGroup;
use crate::services::editor::editor_resolver_service::IEditorResolverService;
use crate::services::editor::editor_service::IEditorService;
use crate::services::files_configuration::files_configuration_service::IFilesConfigurationService;
use crate::services::layout::layout_service::IWorkbenchLayoutService;
use crate::services::search::search::ISearchService;
use crate::services::workspaces::workspace_editing::IWorkspaceEditingService;

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

// ---------------------------------------------------------------------------------------------------------------------
// ExplorerView
// ---------------------------------------------------------------------------------------------------------------------

/// A árvore do explorador: `WorkbenchCompressibleAsyncDataTree<ExplorerItem | ExplorerItem[], ExplorerItem, FuzzyScore>`
/// (ver as divergências do módulo para a entrada e o dado de filtro).
pub type ExplorerTree = WorkbenchCompressibleAsyncDataTree<ExplorerInput, Rc<ExplorerItem>, ()>;

/// `FileCopiedContext` (`fileActions.ts`).
pub fn file_copied_context() -> je_platform::contextkey::contextkey::RawContextKey {
    use je_platform::contextkey::contextkey::{ContextKeyMeta, RawContextKey};
    thread_local! {
        static KEY: RawContextKey = RawContextKey::new("fileCopied", Dynamic::Bool(false), ContextKeyMeta::None);
    }
    KEY.with(Clone::clone)
}

/// `NEW_FILE_COMMAND_ID` (`fileActions.ts`).
pub const NEW_FILE_COMMAND_ID: &str = "explorer.newFile";

/// `IExplorerViewContainerDelegate`.
pub trait IExplorerViewContainerDelegate {
    fn will_open_element(&self, event: Option<&BrowserEvent>);
    fn did_open_element(&self, event: Option<&BrowserEvent>);
}

/// `IExplorerViewPaneOptions`.
pub struct IExplorerViewPaneOptions {
    pub pane: IViewPaneOptions,
    pub delegate: Option<Rc<dyn IExplorerViewContainerDelegate>>,
}

/// O `ResourceContextKey` no que o `ExplorerView` usa: `set(resource)`.
pub trait IExplorerResourceContextKey: IDisposable {
    fn set(&self, resource: Option<&URI>);
}

/// O que o renderer dos itens lê do tema de ícones e do tema de cores (ver [`FilesRendererServices`]).
#[derive(Clone)]
pub struct ExplorerRendererServices {
    pub inline_edit: InlineEditServices,
    /// `themeService.getFileIconTheme()`.
    pub file_icon_theme: Rc<dyn Fn() -> FileIconThemeInfo>,
    /// `{ ...defaultCountBadgeStyles, badgeBackground: listFilterMatchHighlight, badgeBorder: listFilterMatchHighlightBorder }`.
    pub badge_styles: je_widgets::count_badge::ICountBadgeStyles,
}

/// Os serviços do construtor do `ExplorerView` e dos objetos que o `createTree` cria (ver as divergências do módulo).
#[derive(Clone)]
pub struct ExplorerViewServices {
    /// Os dez serviços da `ViewPane` (contextMenuService, viewDescriptorService, keybindingService, configurationService...).
    pub pane: ViewPaneServices,
    pub context_service: Rc<dyn IWorkspaceContextService>,
    pub progress_service: Rc<dyn IProgressService>,
    pub notification_service: Rc<dyn INotificationService>,
    pub editor_service: Rc<dyn IEditorService>,
    pub editor_resolver_service: Rc<dyn IEditorResolverService>,
    pub layout_service: Rc<dyn IWorkbenchLayoutService>,
    pub label_service: Rc<dyn ILabelService>,
    pub telemetry_service: Rc<dyn ITelemetryService>,
    pub explorer_service: Rc<dyn IExplorerService>,
    pub storage_service: Rc<dyn StorageService>,
    pub clipboard_service: Rc<dyn IClipboardService>,
    pub file_service: Rc<dyn IFileService>,
    pub uri_identity_service: Rc<dyn IUriIdentityService>,
    pub command_service: Rc<dyn ICommandService>,
    pub accessibility_service: Rc<dyn IAccessibilityService>,
    pub environment_service: Rc<dyn EnvironmentService>,
    pub files_config_service: Rc<dyn IFilesConfigurationService>,
    pub search_service: Rc<dyn ISearchService>,
    pub dialog_service: Rc<dyn IDialogService>,
    pub workspace_editing_service: Rc<dyn IWorkspaceEditingService>,
    /// Os serviços da `WorkbenchCompressibleAsyncDataTree` (`IListService`, `defaultListStyles`...).
    pub tree: WorkbenchTreeServices,
    /// Os serviços do `ResourceLabels` do `createTree`.
    pub resource_labels: Rc<ResourceLabelsServices>,
    pub renderer: ExplorerRendererServices,
    /// `instantiationService.createInstance(ResourceContextKey)`.
    pub resource_context: Rc<dyn IExplorerResourceContextKey>,
    /// `themeService.onDidColorThemeChange`.
    pub on_did_color_theme_change: Event<()>,
    /// `createFileIconThemableTreeContainerScope(container, themeService)`.
    pub bind_file_icon_theme_scope: Rc<dyn Fn(NodeId) -> DisposableRef>,
    /// `getLocationBasedColors().listOverrideStyles` resolvido pelo tema.
    pub list_override_styles: Rc<dyn Fn(&ListOverrideStyleColors) -> IListStyles>,
    /// O drop de arquivos de fora do explorador (`ExternalFileImport`/`BrowserFileUpload`).
    pub external_file_drop: ExternalFileDropHandler,
}

/// O `ITreeContextMenuEvent<ExplorerItem>` que o `onContextMenu` (parte B) recebe: o item (se houver), o evento do
/// ponteiro ou do teclado e onde o clique caiu. O `anchor` do original sai do alvo do `browser_event`.
#[derive(Clone)]
pub struct ExplorerContextMenuEvent {
    pub element: Option<Rc<ExplorerItem>>,
    pub browser_event: BrowserEvent,
    pub target: TreeMouseEventTarget,
}

/// `identityProvider`.
struct ExplorerIdentityProvider;

impl IIdentityProvider<Rc<ExplorerItem>> for ExplorerIdentityProvider {
    fn get_id(&self, stat: &Rc<ExplorerItem>) -> String {
        if stat.is_new_explorer_item() {
            return format!("new:{}", stat.get_id());
        }

        stat.get_id()
    }
}

/// O `keyboardNavigationLabelProvider` do `createTree`.
struct ExplorerKeyboardNavigationLabelProvider {
    explorer_service: Rc<dyn IExplorerService>,
}

impl IKeyboardNavigationLabelProvider<Rc<ExplorerItem>> for ExplorerKeyboardNavigationLabelProvider {
    fn get_keyboard_navigation_label(&self, stat: &Rc<ExplorerItem>) -> Vec<Option<String>> {
        if self.explorer_service.is_editable(Some(stat)) {
            return vec![None];
        }

        vec![Some(stat.name())]
    }
}

impl ICompressibleAsyncKeyboardNavigationLabelProvider<Rc<ExplorerItem>> for ExplorerKeyboardNavigationLabelProvider {
    fn get_compressed_node_keyboard_navigation_label(&self, stats: &[Rc<ExplorerItem>]) -> Vec<Option<String>> {
        if stats.iter().any(|stat| self.explorer_service.is_editable(Some(stat))) {
            return vec![None];
        }

        vec![Some(stats.iter().map(|stat| stat.name()).collect::<Vec<_>>().join("/"))]
    }
}

/// O `FilesRenderer` como renderer dos nós da árvore de dados assíncrona e comprimível.
struct FilesRendererAdapter(Rc<FilesRenderer>);

impl IAsyncDataTreeRenderer<ExplorerInput, Rc<ExplorerItem>, ()> for FilesRendererAdapter {
    fn template_id(&self) -> String {
        FILES_RENDERER_TEMPLATE_ID.to_owned()
    }

    fn render_template(&self, container: NodeId) -> TemplateData {
        self.0.render_template_data(container)
    }

    fn render_element(&self, node: &Rc<AsyncDataTreeNodeWrapper<ExplorerInput, Rc<ExplorerItem>, ()>>, _index: usize, template_data: &TemplateData, _details: Option<&TreeElementRenderDetails>) {
        if let InputOrElement::Element(stat) = node.element() {
            self.0.render_stat_element(&stat, template_data);
        }
    }

    fn dispose_element(&self, _node: &Rc<AsyncDataTreeNodeWrapper<ExplorerInput, Rc<ExplorerItem>, ()>>, _index: i64, template_data: &TemplateData, _details: Option<&TreeElementRenderDetails>) {
        self.0.dispose_stat_element(template_data);
    }

    fn dispose_template(&self, template_data: &TemplateData) {
        self.0.dispose_template_data(template_data);
    }
}

impl ICompressibleAsyncDataTreeRenderer<ExplorerInput, Rc<ExplorerItem>, ()> for FilesRendererAdapter {
    fn render_compressed_elements(
        &self,
        node: &Rc<CompressibleAsyncDataTreeNodeWrapper<ExplorerInput, Rc<ExplorerItem>, ()>>,
        _index: usize,
        template_data: &TemplateData,
        _details: Option<&TreeElementRenderDetails>,
    ) {
        let elements = node.element().elements.into_iter().filter_map(|element| element.as_element().cloned()).collect();
        self.0.render_compressed_stats(elements, node.depth(), node.collapsed(), template_data);
    }

    fn dispose_compressed_elements(
        &self,
        _node: &Rc<CompressibleAsyncDataTreeNodeWrapper<ExplorerInput, Rc<ExplorerItem>, ()>>,
        _index: i64,
        template_data: &TemplateData,
        _details: Option<&TreeElementRenderDetails>,
    ) {
        self.0.dispose_compressed_stats(template_data);
    }
}

/// O `highlightTree` do `ExplorerFindProvider` como objeto próprio (o renderer o guarda).
struct FindHighlightTreeRef(Rc<ExplorerFindProvider<IAsyncDataTreeViewState>>);

impl IExplorerFindHighlightTree for FindHighlightTreeRef {
    fn get(&self, item: &Rc<ExplorerItem>) -> usize {
        self.0.highlight_tree().get(item)
    }

    fn is_match(&self, item: &Rc<ExplorerItem>) -> bool {
        self.0.highlight_tree().is_match(item)
    }
}

/// `ExplorerFindProviderActive.bindTo(contextKeyService)`.
struct ContextKeyFindActive(Rc<dyn IContextKey>);

impl ExplorerFindActiveKey for ContextKeyFindActive {
    fn set(&self, value: bool) {
        self.0.set(Dynamic::Bool(value));
    }
}

/// O `{ getCompressedNavigationController(stat) }` do `getContext` é o `FilesRenderer`.
impl CompressedNavigationControllerProvider for FilesRenderer {
    fn get_compressed_navigation_controller(&self, stat: &Rc<ExplorerItem>) -> Option<Vec<Rc<dyn ICompressedNavigationController>>> {
        FilesRenderer::get_compressed_navigation_controller(self, stat).map(|controllers| controllers.into_iter().map(|controller| controller as Rc<dyn ICompressedNavigationController>).collect())
    }
}

/// A árvore do `ExplorerView` vista pelo `ExplorerFindProvider` e pela lógica de colapso. A árvore é criada depois do
/// provedor (o `() => this.tree` do original), então o handle lê o campo da vista.
#[derive(Clone)]
pub struct ExplorerTreeHandle {
    slot: Rc<RefCell<Option<Rc<ExplorerTree>>>>,
}

impl ExplorerTreeHandle {
    fn tree(&self) -> Rc<ExplorerTree> {
        self.slot.borrow().clone().expect("a árvore do explorador ainda não foi criada")
    }

    fn element(item: &Rc<ExplorerItem>) -> InputOrElement<ExplorerInput, Rc<ExplorerItem>> {
        InputOrElement::Element(item.clone())
    }
}

impl ExplorerTreeNodes for ExplorerTreeHandle {
    fn has_node(&self, item: &Rc<ExplorerItem>) -> bool {
        self.tree().has_node(&Self::element(item))
    }

    fn is_collapsed(&self, item: &Rc<ExplorerItem>) -> bool {
        self.tree().is_collapsed(&Self::element(item))
    }

    fn is_collapsible(&self, item: &Rc<ExplorerItem>) -> bool {
        self.tree().is_collapsible(item)
    }

    fn collapse(&self, item: &Rc<ExplorerItem>, recursive: bool) -> bool {
        self.tree().collapse(item, recursive)
    }
}

impl ExplorerFindTree<IAsyncDataTreeViewState> for ExplorerTreeHandle {
    fn get_input(&self) -> Option<ExplorerInput> {
        self.tree().get_input()
    }

    fn get_view_state(&self) -> IAsyncDataTreeViewState {
        self.tree().get_view_state()
    }

    fn set_input(&self, input: ExplorerInput, view_state: Option<IAsyncDataTreeViewState>) -> Promise<()> {
        self.tree().widget().set_input(input, view_state)
    }

    fn has_node(&self, item: &Rc<ExplorerItem>) -> bool {
        ExplorerTreeNodes::has_node(self, item)
    }

    fn rerender(&self, item: &Rc<ExplorerItem>) {
        self.tree().rerender(Some(item));
    }

    fn is_collapsed(&self, item: &Rc<ExplorerItem>) -> bool {
        ExplorerTreeNodes::is_collapsed(self, item)
    }
}

/// `JSON.stringify(tree.getViewState())`: as chaves ausentes (`undefined`) ficam de fora.
pub fn tree_view_state_to_json(state: &IAsyncDataTreeViewState) -> String {
    let mut object = serde_json::Map::new();
    if let Some(focus) = &state.focus {
        object.insert("focus".to_owned(), serde_json::json!(focus));
    }
    if let Some(selection) = &state.selection {
        object.insert("selection".to_owned(), serde_json::json!(selection));
    }
    if let Some(expanded) = &state.expanded {
        object.insert("expanded".to_owned(), serde_json::json!(expanded));
    }
    if let Some(scroll_top) = state.scroll_top {
        object.insert("scrollTop".to_owned(), serde_json::json!(scroll_top));
    }
    serde_json::Value::Object(object).to_string()
}

/// `JSON.parse(rawViewState)` como `IAsyncDataTreeViewState`; `None` para um texto que não é JSON.
pub fn tree_view_state_from_json(text: &str) -> Option<IAsyncDataTreeViewState> {
    let value: serde_json::Value = serde_json::from_str(text).ok()?;
    let strings = |key: &str| value.get(key).and_then(serde_json::Value::as_array).map(|items| items.iter().filter_map(|item| item.as_str().map(str::to_owned)).collect::<Vec<_>>());
    Some(IAsyncDataTreeViewState {
        focus: strings("focus"),
        selection: strings("selection"),
        expanded: strings("expanded"),
        scroll_top: value.get("scrollTop").and_then(serde_json::Value::as_f64),
    })
}

/// `explorer.autoReveal` (`boolean | 'force' | 'focusNoScroll'`).
fn reveal_from_setting(value: &Dynamic) -> ExplorerReveal {
    match value {
        Dynamic::String(text) => ExplorerReveal::Mode(text.to_string_lossy().to_string()),
        other => ExplorerReveal::Flag(other.is_truthy()),
    }
}

/// `if (this._autoReveal)`: `false` e a cadeia vazia são falsos.
fn reveal_is_enabled(reveal: &ExplorerReveal) -> bool {
    match reveal {
        ExplorerReveal::Flag(flag) => *flag,
        ExplorerReveal::Mode(mode) => !mode.is_empty(),
    }
}

/// `isItemVisible`: o valor que o `FilesFilter.filter` devolve, como `boolean`.
fn filter_result_is_visible(result: &TreeFilterResult<()>) -> bool {
    match result {
        TreeFilterResult::Bool(visible) => *visible,
        TreeFilterResult::Visibility(visibility) => *visibility != TreeVisibility::Hidden,
        _ => true,
    }
}

/// `ExplorerView`.
pub struct ExplorerView {
    weak_self: Weak<ExplorerView>,
    ui: Ui,
    /// A `ViewPane` de que o `ExplorerView` herda.
    pub pane: Rc<ViewPane>,
    services: ExplorerViewServices,
    delegate: Option<Rc<dyn IExplorerViewContainerDelegate>>,
    /// O `_register` do original.
    disposables: Rc<Disposable>,

    tree_slot: Rc<RefCell<Option<Rc<ExplorerTree>>>>,
    filter: RefCell<Option<Rc<FilesFilter>>>,
    find_provider: RefCell<Option<Rc<ExplorerFindProvider<IAsyncDataTreeViewState>>>>,
    renderer: RefCell<Option<Rc<FilesRenderer>>>,
    drag_and_drop: RefCell<Option<Rc<FileDragAndDrop>>>,

    resource_context: Rc<dyn IExplorerResourceContextKey>,
    folder_context: Rc<dyn IContextKey>,
    parent_readonly_context: Rc<dyn IContextKey>,
    readonly_context: Rc<dyn IContextKey>,
    available_editor_ids_context: Rc<dyn IContextKey>,
    root_context: Rc<dyn IContextKey>,
    resource_moveable_to_trash: Rc<dyn IContextKey>,
    compressed_focus_context: Rc<dyn IContextKey>,
    compressed_focus_first_context: Rc<dyn IContextKey>,
    compressed_focus_last_context: Rc<dyn IContextKey>,
    view_has_some_collapsible_root_item: Rc<dyn IContextKey>,
    view_visible_context_key: Rc<dyn IContextKey>,
    /// `@memoize fileCopiedContextKey`.
    file_copied_context_key: RefCell<Option<Rc<dyn IContextKey>>>,
    /// `@memoize resourceCutContextKey`.
    resource_cut_context_key: RefCell<Option<Rc<dyn IContextKey>>>,

    set_tree_input_sequencer: Sequencer,
    horizontal_scrolling: Cell<Option<bool>>,
    auto_reveal: RefCell<ExplorerReveal>,
    tree_container: Cell<Option<NodeId>>,
    container: Cell<Option<NodeId>>,
    workspace_title_container: Cell<Option<NodeId>>,
}

impl ExplorerView {
    /// `TREE_VIEW_STATE_STORAGE_KEY`.
    pub const TREE_VIEW_STATE_STORAGE_KEY: &'static str = TREE_VIEW_STATE_STORAGE_KEY;

    pub fn new(options: IExplorerViewPaneOptions, services: ExplorerViewServices) -> Rc<Self> {
        let ui = services.pane.ui.clone();
        let pane = ViewPane::new(options.pane, services.pane.clone());
        let scoped_keys = pane.scoped_context_key_service();
        let keys: &dyn IContextKeyService = &*scoped_keys;
        let disposables = Disposable::new();

        // this.resourceContext = instantiationService.createInstance(ResourceContextKey); this._register(this.resourceContext);
        let resource_context = services.resource_context.clone();
        disposables.register(resource_context.clone());

        let this = Rc::new_cyclic(|weak| Self {
            weak_self: weak.clone(),
            ui,
            pane: pane.clone(),
            delegate: options.delegate,
            disposables,
            tree_slot: Rc::new(RefCell::new(None)),
            filter: RefCell::new(None),
            find_provider: RefCell::new(None),
            renderer: RefCell::new(None),
            drag_and_drop: RefCell::new(None),
            resource_context,
            parent_readonly_context: explorer_resource_parent_read_only_context().bind_to(keys),
            folder_context: explorer_folder_context().bind_to(keys),
            readonly_context: explorer_resource_readonly_context().bind_to(keys),
            available_editor_ids_context: explorer_resource_available_editor_ids_context().bind_to(keys),
            root_context: explorer_root_context().bind_to(keys),
            resource_moveable_to_trash: explorer_resource_moveable_to_trash().bind_to(keys),
            compressed_focus_context: explorer_compressed_focus_context().bind_to(keys),
            compressed_focus_first_context: explorer_compressed_first_focus_context().bind_to(keys),
            compressed_focus_last_context: explorer_compressed_last_focus_context().bind_to(keys),
            view_has_some_collapsible_root_item: view_has_some_collapsible_root_item_context().bind_to(keys),
            view_visible_context_key: folders_view_visible_context().bind_to(keys),
            file_copied_context_key: RefCell::new(None),
            resource_cut_context_key: RefCell::new(None),
            set_tree_input_sequencer: Sequencer::new(),
            horizontal_scrolling: Cell::new(None),
            auto_reveal: RefCell::new(ExplorerReveal::Flag(false)),
            tree_container: Cell::new(None),
            container: Cell::new(None),
            workspace_title_container: Cell::new(None),
            services,
        });

        let delegate: Rc<dyn ViewPaneDelegate> = this.clone();
        pane.set_delegate(Rc::downgrade(&delegate));

        this.services.explorer_service.register_view(this.clone());
        let weak = this.weak_self.clone();
        this.disposables.register(to_disposable(move || {
            if let Some(this) = weak.upgrade() {
                this.clear_workspace_title_container();
            }
        }));

        this
    }

    // Propriedades

    /// `autoReveal`.
    pub fn auto_reveal(&self) -> ExplorerReveal {
        self.auto_reveal.borrow().clone()
    }

    /// `set autoReveal(autoReveal)`.
    pub fn set_auto_reveal(&self, auto_reveal: ExplorerReveal) {
        *self.auto_reveal.borrow_mut() = auto_reveal;
    }

    /// `name`.
    pub fn name(&self) -> String {
        let workspace: Rc<dyn IWorkspace> = self.services.context_service.get_workspace();
        self.services.label_service.get_workspace_label(&WorkspaceLabelTarget::Full(workspace), None)
    }

    /// `get title()`; o `set title` do original é vazio.
    pub fn title(&self) -> String {
        self.name()
    }

    /// `get singleViewPaneContainerTitle()`.
    pub fn single_view_pane_container_title(&self) -> String {
        self.name()
    }

    /// `get headerVisible()`.
    pub fn header_visible(&self) -> bool {
        self.pane.header_visible()
    }

    /// `set headerVisible(visible)`.
    pub fn set_header_visible(&self, visible: bool) {
        self.pane.set_header_visible(visible);
        self.update_workspace_title_case();
    }

    /// `setVisible(visible)`.
    pub fn set_visible(&self, visible: bool) {
        if !visible {
            self.clear_workspace_title_container();
        }
        self.view_visible_context_key.set(Dynamic::Bool(visible));
        self.pane.set_visible(visible);
        if visible {
            self.update_workspace_title_container();
        }
    }

    /// `@memoize fileCopiedContextKey`.
    pub fn file_copied_context_key(&self) -> Rc<dyn IContextKey> {
        self.file_copied_context_key.borrow_mut().get_or_insert_with(|| self.bind_scoped(file_copied_context())).clone()
    }

    /// `@memoize resourceCutContextKey`.
    pub fn resource_cut_context_key(&self) -> Rc<dyn IContextKey> {
        self.resource_cut_context_key.borrow_mut().get_or_insert_with(|| self.bind_scoped(explorer_resource_cut())).clone()
    }

    /// `Key.bindTo(this.contextKeyService)`.
    fn bind_scoped(&self, key: je_platform::contextkey::contextkey::RawContextKey) -> Rc<dyn IContextKey> {
        let scoped = self.pane.scoped_context_key_service();
        let keys: &dyn IContextKeyService = &*scoped;
        key.bind_to(keys)
    }

    /// A árvore criada por `createTree` (o `tree!` do original).
    fn tree(&self) -> Rc<ExplorerTree> {
        self.tree_slot.borrow().clone().expect("o corpo do explorador ainda não foi desenhado")
    }

    // Título do espaço de trabalho

    fn update_workspace_title_container(&self) {
        // DOM.findParentWithClass(this.element, 'part') ?? undefined
        let workspace_title_container = {
            let tree = self.ui.tree_mut();
            let mut current = tree.parent(self.pane.element());
            while let Some(node) = current {
                if tree.has_class(node, "part") {
                    break;
                }
                current = tree.parent(node);
            }
            current
        };
        if self.workspace_title_container.get() != workspace_title_container {
            if let Some(previous) = self.workspace_title_container.get() {
                self.ui.tree_mut().remove_class(previous, PRESERVE_MERGED_WORKSPACE_NAME_CASE_CLASS);
            }
            self.workspace_title_container.set(workspace_title_container);
        }
        self.update_workspace_title_case();
    }

    fn clear_workspace_title_container(&self) {
        if let Some(container) = self.workspace_title_container.take() {
            self.ui.tree_mut().remove_class(container, PRESERVE_MERGED_WORKSPACE_NAME_CASE_CLASS);
        }
    }

    fn update_workspace_title_case(&self) {
        let workspace: Rc<dyn IWorkspace> = self.services.context_service.get_workspace();
        let preserve_workspace_name_case = should_preserve_workspace_name_case(self.services.context_service.get_workbench_state(), workspace.as_ref(), self.services.environment_service.as_ref());
        let mut tree = self.ui.tree_mut();
        tree.toggle_class(self.pane.element(), PRESERVE_WORKSPACE_NAME_CASE_CLASS, preserve_workspace_name_case);
        if let Some(container) = self.workspace_title_container.get() {
            tree.toggle_class(container, PRESERVE_MERGED_WORKSPACE_NAME_CASE_CLASS, preserve_workspace_name_case && self.pane.is_visible() && !self.pane.header_visible());
        }
    }

    /// O `setHeader` do `renderHeader`: o título, o rótulo de acessibilidade da seção e o do próprio título.
    fn set_header(&self) {
        let name = self.name();
        self.pane.update_title(&name);
        let aria_label = localize("explorerSection", "Explorer Section: {0}", &[&name]);
        self.pane.set_aria_header_label(&aria_label);
        if let Some(title_element) = self.pane.title_container() {
            self.ui.tree_mut().set_label(title_element, aria_label);
        }
    }

    // Foco e seleção

    /// `focus()`.
    pub fn focus(&self) {
        self.pane.focus();
        let tree = self.tree();
        tree.dom_focus();

        if tree.get_focused_part() == AbstractTreePart::Tree {
            let focused = tree.get_focus();
            if focused.len() == 1 && reveal_is_enabled(&self.auto_reveal.borrow()) {
                tree.reveal(&focused[0], Some(0.5));
            }
        }
    }

    /// `hasFocus()`.
    pub fn has_focus(&self) -> bool {
        self.container.get().is_some_and(|container| is_ancestor_of_active_element(&self.ui, container))
    }

    /// `getFocus()`.
    pub fn get_focus(&self) -> Vec<Rc<ExplorerItem>> {
        self.tree().get_focus()
    }

    /// `focusNext()`.
    pub fn focus_next(&self) {
        self.tree().focus_next(1, false, None, None);
    }

    /// `focusLast()`.
    pub fn focus_last(&self) {
        self.tree().focus_last(None, None);
    }

    /// `getContext(respectMultiSelection)`.
    pub fn get_context(&self, respect_multi_selection: bool) -> Vec<Rc<ExplorerItem>> {
        let tree = self.tree();
        let focused_items = if tree.get_focused_part() == AbstractTreePart::StickyScroll { tree.get_sticky_scroll_focus() } else { tree.get_focus() };
        let renderer = self.renderer.borrow().clone().expect("o corpo do explorador ainda não foi desenhado");
        get_context(&focused_items, &tree.get_selection(), respect_multi_selection, renderer.as_ref())
    }

    /// `isItemVisible(item)`.
    pub fn is_item_visible(&self, item: &Rc<ExplorerItem>) -> bool {
        // If filter is undefined it means the tree hasn't been rendered yet, so nothing is visible
        let Some(filter) = self.filter.borrow().clone() else {
            return false;
        };
        filter_result_is_visible(&filter.filter_stat(item, TreeVisibility::Visible))
    }

    /// `isItemCollapsed(item)`.
    pub fn is_item_collapsed(&self, item: &Rc<ExplorerItem>) -> bool {
        self.tree().is_collapsed(&InputOrElement::Element(item.clone()))
    }

    /// `tryGetRelativeTop(element)`: o item que a árvore ainda não conhece vale `None`.
    fn try_get_relative_top(&self, element: &Rc<ExplorerItem>) -> Option<f64> {
        let tree = self.tree_slot.borrow().clone()?;
        if !tree.has_node(&InputOrElement::Element(element.clone())) {
            return None;
        }

        tree.get_relative_top(element)
    }

    // createTree

    fn create_tree(self: &Rc<Self>, container: NodeId) {
        let services = &self.services;
        let configuration_service = services.pane.configuration_service.clone();
        let scoped_keys = self.pane.scoped_context_key_service();
        let keys: &dyn IContextKeyService = &*scoped_keys;

        let filter = FilesFilter::new(
            services.context_service.clone(),
            configuration_service.clone(),
            services.explorer_service.clone(),
            services.editor_service.clone(),
            services.uri_identity_service.clone(),
            services.file_service.clone(),
        );
        self.disposables.register(to_disposable({
            let filter = filter.clone();
            move || filter.dispose()
        }));
        *self.filter.borrow_mut() = Some(filter.clone());
        let weak = self.weak_self.clone();
        self.disposables.register(filter.on_did_change().listen(move |_| {
            if let Some(this) = weak.upgrade() {
                let _ = this.refresh(true, None, true);
            }
        }));
        let explorer_labels = ResourceLabels::new(IResourceLabelsContainer { on_did_change_visibility: self.pane.on_did_change_body_visibility() }, services.resource_labels.clone());
        self.disposables.register(explorer_labels.clone());

        let tree_handle = ExplorerTreeHandle { slot: self.tree_slot.clone() };
        let find_provider = ExplorerFindProvider::new(
            filter.clone(),
            Box::new({
                let tree_handle = tree_handle.clone();
                move || Rc::new(tree_handle.clone()) as Rc<dyn ExplorerFindTree<IAsyncDataTreeViewState>>
            }),
            services.search_service.clone(),
            services.file_service.clone(),
            configuration_service.clone(),
            services.files_config_service.clone(),
            services.progress_service.clone(),
            services.explorer_service.clone(),
            Rc::new(ContextKeyFindActive(explorer_find_provider_active().bind_to(keys))),
            Rc::new(services.uri_identity_service.ext_uri()),
        );
        *self.find_provider.borrow_mut() = Some(find_provider.clone());

        let update_width: Rc<dyn Fn(&Rc<ExplorerItem>)> = {
            let slot = self.tree_slot.clone();
            Rc::new(move |stat| {
                let tree = slot.borrow().clone();
                if let Some(tree) = tree {
                    tree.update_width(stat);
                }
            })
        };
        let renderer = FilesRenderer::new(FilesRendererServices {
            ui: self.ui.clone(),
            labels: explorer_labels,
            highlight_tree: Rc::new(FindHighlightTreeRef(find_provider.clone())),
            update_width,
            configuration_service: configuration_service.clone(),
            explorer_service: services.explorer_service.clone(),
            label_service: services.label_service.clone(),
            context_service: services.context_service.clone(),
            file_icon_theme: services.renderer.file_icon_theme.clone(),
            inline_edit: services.renderer.inline_edit.clone(),
            badge_styles: services.renderer.badge_styles,
        });
        self.disposables.register(renderer.clone());
        *self.renderer.borrow_mut() = Some(renderer.clone());

        self.disposables.register((services.bind_file_icon_theme_scope)(container));

        let compression_enabled: Rc<dyn Fn() -> bool> = {
            let (configuration_service, accessibility_service) = (configuration_service.clone(), services.accessibility_service.clone());
            Rc::new(move || {
                is_compression_enabled(
                    configuration_service.get_value(Some("explorer.compactFolders"), None).is_truthy(),
                    // Disable compact folders when screen reader is optimized for better accessibility
                    accessibility_service.is_screen_reader_optimized(),
                )
            })
        };

        let get_file_nesting_expand: Rc<dyn Fn(&Rc<ExplorerItem>) -> bool> = {
            let configuration_service = configuration_service.clone();
            Rc::new(move |item| {
                let overrides = ConfigurationOverrides::with_resource(item.root().resource());
                configuration_service.get_value(None, Some(&overrides)).get_str("explorer").get_str("fileNesting").get_str("expand").is_truthy()
            })
        };

        let drag_and_drop = Rc::new(FileDragAndDrop::new(
            {
                let weak = self.weak_self.clone();
                Rc::new(move |item| weak.upgrade().is_some_and(|this| this.is_item_collapsed(item)))
            },
            FileDragAndDropServices {
                explorer_service: services.explorer_service.clone(),
                editor_service: services.editor_service.clone(),
                dialog_service: services.dialog_service.clone(),
                context_service: services.context_service.clone(),
                file_service: services.file_service.clone(),
                configuration_service: configuration_service.clone(),
                workspace_editing_service: services.workspace_editing_service.clone(),
                uri_identity_service: services.uri_identity_service.clone(),
                external_file_drop: services.external_file_drop.clone(),
            },
        ));
        self.disposables.register(drag_and_drop.clone());
        *self.drag_and_drop.borrow_mut() = Some(drag_and_drop);

        let mut options = CompressibleAsyncDataTreeOptions::<Rc<ExplorerItem>, ()>::default();
        options.compression_enabled = Some(compression_enabled());
        options.auto_expand_compressed_children = true;
        options.keyboard_navigation_label_provider = Some(Rc::new(ExplorerKeyboardNavigationLabelProvider { explorer_service: services.explorer_service.clone() }));
        options.base.identity_provider = Some(Rc::new(ExplorerIdentityProvider));
        options.base.update.multiple_selection_support = Some(true);
        options.base.filter = Some(filter.clone());
        options.base.sorter = Some(Rc::new(FileSorter::new(services.explorer_service.clone(), services.context_service.clone())));
        options.base.collapse_by_default = Some({
            let (find_provider, get_file_nesting_expand) = (find_provider.clone(), get_file_nesting_expand.clone());
            Rc::new(move |item: &Rc<ExplorerItem>| {
                let expand_nests = item.has_nests() && get_file_nesting_expand(item);
                collapse_by_default(item.has_nests(), expand_nests, find_provider.is_showing_filter_results())
            })
        });
        options.base.update.expand_only_on_twistie_click = Some(ExpandOnlyOnTwistieClick::When({
            let configuration_service = configuration_service.clone();
            Rc::new(move |item: &Rc<ExplorerItem>| {
                let double_click = matches!(configuration_service.get_value(Some("workbench.tree.expandMode"), None), Dynamic::String(mode) if mode.to_string_lossy() == "doubleClick");
                expand_only_on_twistie_click(item.has_nests(), double_click)
            })
        }));
        options.base.update.padding_bottom = Some(ExplorerDelegate::ITEM_HEIGHT);

        let renderers: Vec<Rc<dyn ICompressibleAsyncDataTreeRenderer<ExplorerInput, Rc<ExplorerItem>, ()>>> = vec![Rc::new(FilesRendererAdapter(renderer.clone()))];
        let workbench_options = IWorkbenchTreeOptions {
            override_styles: Some((services.list_override_styles)(&self.pane.get_location_based_colors().list_override_styles)),
            ..IWorkbenchTreeOptions::default()
        };
        let tree = ExplorerTree::new(
            "FileExplorer",
            &self.ui,
            container,
            Rc::new(ExplorerDelegate),
            Rc::new(ExplorerCompressionDelegate),
            renderers,
            Rc::new(ExplorerDataSource::new(
                filter.clone(),
                find_provider.clone(),
                services.progress_service.clone(),
                configuration_service.clone(),
                services.notification_service.clone(),
                services.layout_service.clone(),
                services.file_service.clone(),
                services.explorer_service.clone(),
                services.context_service.clone(),
                services.files_config_service.clone(),
            )),
            options,
            workbench_options,
            &services.tree,
        );
        self.disposables.register(tree.clone());
        *self.tree_slot.borrow_mut() = Some(tree.clone());

        let weak_tree = Rc::downgrade(&tree);
        self.disposables.register(services.on_did_color_theme_change.listen({
            let weak_tree = weak_tree.clone();
            move |_| {
                if let Some(tree) = weak_tree.upgrade() {
                    tree.rerender(None);
                }
            }
        }));

        // Bind configuration
        self.disposables.register(configuration_service.on_did_change_configuration().listen({
            let (weak_tree, compression_enabled) = (weak_tree.clone(), compression_enabled.clone());
            move |e| {
                if e.affects_configuration("explorer.compactFolders", None)
                    && let Some(tree) = weak_tree.upgrade()
                {
                    tree.set_compression_enabled(compression_enabled());
                }
            }
        }));

        // Update compression when screen reader mode changes
        self.disposables.register(services.accessibility_service.on_did_change_screen_reader_optimized().listen({
            let weak_tree = weak_tree.clone();
            move |_| {
                if let Some(tree) = weak_tree.upgrade() {
                    tree.set_compression_enabled(compression_enabled());
                }
            }
        }));

        // Bind context keys
        let tree_keys_owner = tree.context_key_service();
        let tree_keys: &dyn IContextKeyService = &*tree_keys_owner;
        files_explorer_focused_context().bind_to(tree_keys);
        explorer_focused_context().bind_to(tree_keys);

        // Update resource context based on focused element
        let weak = self.weak_self.clone();
        self.disposables.register(tree.on_did_change_focus().listen(move |e| {
            if let Some(this) = weak.upgrade() {
                this.on_focus_changed(&e.elements);
            }
        }));
        self.on_focus_changed(&[]);

        // Open when selecting via keyboard
        let weak = self.weak_self.clone();
        self.disposables.register(tree.on_did_open().listen(move |e| {
            let Some(this) = weak.upgrade() else { return };
            let element = e.element.clone();
            // Do not react if the user is expanding selection via keyboard.
            // Check if the item was previously also selected, if yes the user is simply expanding / collapsing current selection #66589.
            let shift_down = e.browser_event.is_some_and(|event| event.is_keyboard && event.modifiers.shift);
            if shift_down {
                return;
            }
            if element.is_directory() || this.services.explorer_service.is_editable(None) {
                // Do not react if user is clicking on explorer items while some are being edited #70276
                // Do not react if clicking on directories
                return;
            }
            this.services.telemetry_service.public_log2(
                "workbenchActionExecuted",
                Some(Dynamic::object([("id", Dynamic::string("workbench.files.openFile")), ("from", Dynamic::string("explorer"))])),
            );
            let group = if e.side_by_side { PreferredGroup::Side } else { PreferredGroup::Active };
            let mut editor_options = TextEditorOptions::from(EditorOptions {
                preserve_focus: e.editor_options.preserve_focus,
                pinned: e.editor_options.pinned,
                source: Some(EditorOpenSource::User),
                ..EditorOptions::default()
            });
            editor_options.selection = None;
            let input = UntypedEditorInput { resource: Some(element.resource()), options: Some(editor_options), ..UntypedEditorInput::default() };
            if let Some(delegate) = &this.delegate {
                delegate.will_open_element(e.browser_event.as_ref());
            }
            let _ = async_fn(async move {
                let opened = this.services.editor_service.open_editor_in(input, Some(group)).await;
                if let Some(delegate) = &this.delegate {
                    delegate.did_open_element(None);
                }
                opened.map(|_| ())
            });
        }));

        let weak = self.weak_self.clone();
        self.disposables.register(tree.on_context_menu().listen(move |e| {
            if let Some(this) = weak.upgrade() {
                this.on_context_menu(&ExplorerContextMenuEvent {
                    element: e.element.clone().flatten().map(|node| node.element_t()),
                    browser_event: e.browser_event,
                    target: e.target,
                });
            }
        }));

        let weak = self.weak_self.clone();
        self.disposables.register(tree.on_did_scroll().listen(move |e| {
            let Some(this) = weak.upgrade() else { return };
            if !e.scroll_top_changed {
                return;
            }
            if let Some(editable) = this.services.explorer_service.get_editable()
                && this.try_get_relative_top(&editable.stat).is_none()
            {
                let _ = (editable.data.on_finish)(String::new(), false);
            }
        }));

        let weak = self.weak_self.clone();
        self.disposables.register(tree.on_did_change_collapse_state().listen(move |e| {
            let Some(this) = weak.upgrade() else { return };
            let element = e.node.element().map(|node| node.element_value());
            let element = match element {
                Some(InputOrElement::Element(item)) => Some(item),
                Some(InputOrElement::Input(input)) => input.first().cloned(),
                None => None,
            };
            if let Some(element) = element {
                let renderer = this.renderer.borrow().clone();
                if let Some(controllers) = renderer.and_then(|renderer| renderer.get_compressed_navigation_controller(&element)) {
                    for controller in controllers {
                        controller.update_collapsed(e.node.collapsed());
                    }
                }
            }
            // Update showing expand / collapse button
            this.update_any_collapsed_context();
        }));

        self.update_any_collapsed_context();

        let weak = self.weak_self.clone();
        self.disposables.register(tree.on_mouse_dbl_click().listen(move |e| {
            let Some(this) = weak.upgrade() else { return };
            // If empty space is clicked, and not scrolling by page enabled #173261
            let scrolling_by_page = this.services.pane.configuration_service.get_value(Some("workbench.list.scrollByPage"), None).is_truthy();
            if e.element.is_none() && !scrolling_by_page {
                // click in empty area -> create a new file #116676
                let _ = this.services.command_service.execute_command(NEW_FILE_COMMAND_ID, Vec::new());
            }
        }));

        // save view state
        let weak = self.weak_self.clone();
        self.disposables.register(services.storage_service.on_will_save_state().listen(move |_| {
            if let Some(this) = weak.upgrade() {
                this.store_tree_view_state();
            }
        }));
    }

    // React on events

    /// `onConfigurationUpdated(event)`: `affects` é o `event.affectsConfiguration`; `None` é o `undefined` da primeira chamada.
    fn on_configuration_updated(self: &Rc<Self>, affects: Option<&dyn Fn(&str) -> bool>) {
        if affects.is_none_or(|affects| affects("explorer.autoReveal")) {
            let auto_reveal = self.services.pane.configuration_service.get_value(None, None).get_str("explorer").get_str("autoReveal");
            *self.auto_reveal.borrow_mut() = reveal_from_setting(&auto_reveal);
        }

        // Push down config updates to components of viewer
        if affects.is_some_and(|affects| affects("explorer.decorations.colors") || affects("explorer.decorations.badges")) {
            let _ = self.refresh(true, None, true);
        }
    }

    /// `storeTreeViewState()`.
    fn store_tree_view_state(&self) {
        let Some(tree) = self.tree_slot.borrow().clone() else { return };
        let state = StorageValue::String(tree_view_state_to_json(&tree.get_view_state()));
        self.services.storage_service.store(TREE_VIEW_STATE_STORAGE_KEY, state, StorageScope::Workspace, StorageTarget::Machine);
    }

    /// `setContextKeys(stat)`.
    fn set_context_keys(&self, stat: Option<&Rc<ExplorerItem>>) {
        let folders = self.services.context_service.get_workspace().folders();
        let resource = match stat {
            Some(stat) => stat.resource(),
            None => match folders.last() {
                Some(folder) => folder.uri.clone(),
                None => errors::throw(Error::type_error("Cannot read properties of undefined (reading 'uri')")),
            },
        };
        let stat = stat.cloned().or_else(|| self.services.explorer_service.find_closest(&resource));
        self.resource_context.set(Some(&resource));
        let values = ExplorerContextKeyValues::of(stat.as_ref());
        self.folder_context.set(Dynamic::Bool(values.folder));
        self.readonly_context.set(Dynamic::Bool(values.readonly));
        self.parent_readonly_context.set(Dynamic::Bool(values.parent_readonly));
        self.root_context.set(Dynamic::Bool(values.root));

        let overrides = self.services.editor_resolver_service.get_editors(&resource, None).into_iter().map(|editor| editor.id).collect::<Vec<_>>();
        self.available_editor_ids_context.set(Dynamic::string(&overrides.join(",")));
    }

    /// `onFocusChanged(elements)`.
    fn on_focus_changed(&self, elements: &[Rc<ExplorerItem>]) {
        let stat = elements.first();
        self.set_context_keys(stat);

        match stat {
            Some(stat) => {
                let enable_trash = self.services.pane.configuration_service.get_value(None, None).get_str("files").get_str("enableTrash").is_truthy();
                let has_capability = self.services.file_service.has_capability(&stat.resource(), FileSystemProviderCapabilities::TRASH);
                self.resource_moveable_to_trash.set(Dynamic::Bool(enable_trash && has_capability));
            }
            None => self.resource_moveable_to_trash.reset(),
        }

        let renderer = self.renderer.borrow().clone();
        let compressed_navigation_controllers = stat.and_then(|stat| renderer.and_then(|renderer| renderer.get_compressed_navigation_controller(stat)));

        let Some(compressed_navigation_controllers) = compressed_navigation_controllers else {
            self.compressed_focus_context.set(Dynamic::Bool(false));
            return;
        };

        self.compressed_focus_context.set(Dynamic::Bool(true));
        for controller in &compressed_navigation_controllers {
            self.update_compressed_navigation_context_keys(controller.as_ref());
        }
    }

    /// `updateCompressedNavigationContextKeys(controller)`.
    fn update_compressed_navigation_context_keys(&self, controller: &dyn ICompressedNavigationController) {
        let (first, last) = compressed_navigation_context_keys(controller);
        self.compressed_focus_first_context.set(Dynamic::Bool(first));
        self.compressed_focus_last_context.set(Dynamic::Bool(last));
    }

    /// `updateAnyCollapsedContext()`.
    fn update_any_collapsed_context(&self) {
        let Some(tree) = self.tree_slot.borrow().clone() else { return };
        let Some(tree_input) = tree.get_input() else { return };
        // Has collapsible root when anything is expanded
        let handle = ExplorerTreeHandle { slot: self.tree_slot.clone() };
        self.view_has_some_collapsible_root_item.set(Dynamic::Bool(has_expanded_node(&handle, &tree_input)));
        // synchronize state to cache
        self.store_tree_view_state();
    }
}

impl ViewPaneDelegate for ExplorerView {
    /// `renderHeader(container)` depois do `super.renderHeader`.
    fn render_header(&self, _container: NodeId) {
        let Some(this) = self.weak_self.upgrade() else { return };

        let set_header = {
            let weak = this.weak_self.clone();
            move || {
                if let Some(this) = weak.upgrade() {
                    this.set_header();
                }
            }
        };

        let (header, weak) = (set_header.clone(), this.weak_self.clone());
        this.disposables.register(this.services.context_service.on_did_change_workspace_name().listen(move |_| {
            header();
            if let Some(this) = weak.upgrade() {
                this.update_workspace_title_case();
            }
        }));
        let weak = this.weak_self.clone();
        this.disposables.register(this.services.context_service.on_did_change_workbench_state().listen(move |_| {
            if let Some(this) = weak.upgrade() {
                this.update_workspace_title_case();
            }
        }));
        let header = set_header.clone();
        this.disposables.register(this.services.label_service.on_did_change_formatters().listen(move |_| header()));
        set_header();
    }

    /// `renderBody(container)` depois do `super.renderBody`.
    fn render_body(&self, container: NodeId) {
        let Some(this) = self.weak_self.upgrade() else { return };

        this.container.set(Some(container));
        let tree_container = create_el(&mut this.ui.tree_mut(), container, Role::Generic, "explorer-folders-view");
        this.tree_container.set(Some(tree_container));

        this.create_tree(tree_container);

        let weak = this.weak_self.clone();
        this.disposables.register(this.services.label_service.on_did_change_formatters().listen(move |_| {
            if let Some(this) = weak.upgrade() {
                this.pane.fire_did_change_title_area();
            }
        }));

        // Update configuration
        this.on_configuration_updated(None);

        // When the explorer viewer is loaded, listen to changes to the editor input
        let weak = this.weak_self.clone();
        this.disposables.register(this.services.editor_service.on_did_active_editor_change().listen(move |_| {
            if let Some(this) = weak.upgrade() {
                let _ = this.select_active_file(None);
            }
        }));

        // Also handle configuration updates
        let weak = this.weak_self.clone();
        this.disposables.register(this.services.pane.configuration_service.on_did_change_configuration().listen(move |e| {
            if let Some(this) = weak.upgrade() {
                let event = e.clone();
                this.on_configuration_updated(Some(&move |key: &str| event.affects_configuration(key, None)));
            }
        }));

        let weak = this.weak_self.clone();
        this.disposables.register(this.pane.on_did_change_body_visibility().listen(move |visible| {
            if !*visible {
                return;
            }
            let Some(this) = weak.upgrade() else { return };
            let _ = async_fn(async move {
                // Always refresh explorer when it becomes visible to compensate for missing file events #126817
                this.set_tree_input().await?;
                // Update the collapse / expand  button state
                this.update_any_collapsed_context();
                // Find resource to focus from active editor input if set
                this.select_active_file(Some(ExplorerReveal::Flag(true))).await
            });
        }));
    }

    /// `layoutBody(height, width)` depois do `super.layoutBody`.
    fn layout_body(&self, height: f64, width: f64) {
        self.tree().layout(Some(height), Some(width));
    }
}

impl IDisposable for ExplorerView {
    fn dispose(&self) {
        self.disposables.dispose();
        self.pane.dispose();
    }

    fn constructor_name(&self) -> &'static str {
        "ExplorerView"
    }
}
