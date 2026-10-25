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
//! - **`createInstance` do `createTree`.** As oito peças que o `createTree` instancia (`FilesFilter`, `ResourceLabels`,
//!   `ExplorerFindProvider`, `FilesRenderer`, `FileDragAndDrop`, `ExplorerDataSource`, `FileSorter` e a árvore) vêm de um
//!   [`ExplorerTreeFactory`]. Sem fábrica em [`ExplorerViewServices::tree_factory`], a vista usa a [`FilesExplorerTreeFactory`], que
//!   monta os componentes reais; com fábrica, ela faz o papel do `stubInstance` do original (os testes entregam a árvore e os
//!   dublês). A vista guarda cada peça pelo contrato que usa: do filtro, o [`ITreeFilter`], o `onDidChange` e o `dispose`
//!   ([`ExplorerFilter`]); do provedor de busca, o `isShowingFilterResults` ([`IsShowingFilterResults`]); do renderer, o
//!   [`IExplorerRenderer`]; da fonte de dados, o [`IAsyncDataSource`]; do sorter, o [`ITreeSorter`]; do resto, o `dispose`.
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
//! A segunda metade do módulo (de `refresh` até o fim) é o [`IExplorerView`] e o resto da vista:
//! - **Membros do `IExplorerView`.** `autoReveal`, `getContext`, `refresh`, `selectResource`, `setTreeInput`, `itemsCopied`,
//!   `setEditable`, `isItemVisible`, `isItemCollapsed`, `hasFocus`, `getFocus`, `focusNext`, `focusLast` e `hasPhantomElements` têm o
//!   corpo no próprio `impl IExplorerView for ExplorerView`, sem método avulso que só o repasse. Os parâmetros com valor padrão do
//!   original (`cancelEditing = true`, `reveal = this._autoReveal`, `retry = 0`) chegam como `Option`.
//! - **Entrada da árvore.** `ExplorerInput` é a lista dos itens do topo: com um espaço de uma pasta, a entrada do original é a
//!   própria pasta e a lista são os filhos dela, resolvidos pela `ExplorerDataSource` (com o mesmo progresso e o mesmo item de
//!   erro). O que o original guarda em `tree.getInput()` (a pasta ou o vetor de raízes) é o campo `tree_input`, que responde a
//!   `Array.isArray(treeInput)`, a `treeInput === item` e a `hasNode(raiz)`. Como a lista dos filhos da pasta fica fixa na
//!   entrada, `refresh` da pasta (e `refresh` sem item) refaz a entrada com o estado da vista, em vez do `updateChildren`.
//! - **`horizontalScrolling`.** A árvore do `je-widgets` não tem rolagem horizontal, então o par `updateOptions({ horizontalScrolling
//!   })` de `setEditable` (desligar na edição e restaurar depois) não tem efeito e não existe aqui.
//! - **Menu de contexto.** O evento da árvore não carrega a posição do ponteiro nem as linhas da pasta comprimida: a âncora é o nó
//!   alvo do evento (ou a própria árvore), e o ajuste `labels[index]` do teclado em pasta comprimida, que só vale quando a âncora é
//!   um elemento (teclado), não tem rótulo para apontar. `isEditableElement` é o papel `TextInput` do nó alvo.
//! - **`getOptimalWidth`.** `getLargestChildWidth` mede a largura dos nós `.label-name` dentro de `.explorer-item` pelos limites
//!   absolutos do `Ui` (sem `scrollWidth` nem margens).
//! - **`JSON.parse` do estado.** Um texto inválido rejeita a promessa de `setTreeInput`, como o `SyntaxError` do original.
//! - **`perf.mark`.** É [`je_base::performance::mark`].
//! - **`DelayedDragHandler` e `dragHandler`.** O arrasto sobre o cabeçalho não existe (ver acima), então `dispose` não o libera.
//! - **Ações do arquivo.** As quatro `registerAction2` do fim (`createFileFromExplorer`, `createFolderFromExplorer`,
//!   `refreshFilesExplorer` e `collapseExplorerFolders`) estão em [`explorer_view_actions`](super::explorer_view_actions). O
//!   `view as ExplorerView` da última é o `downcast_ref` de um `&dyn Any`: o [`IViewPane`](crate::services::views::views_service::IViewPane)
//!   tem `Any` como supertrait e o [`ExplorerView`] o implementa.

use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::rc::{Rc, Weak};

use je_base::actions::ActionRef;
use je_base::async_::{Sequencer, async_fn, await_maybe};
use je_base::dynamic::{Dynamic, Truthy};
use je_base::errors::{self, Error};
use je_base::event::Event;
use je_base::lifecycle::{Disposable, DisposableRef, IDisposable, to_disposable};
use je_base::nls::localize;
use je_base::parts::contextmenu::IContextMenuEvent;
use je_base::performance::mark;
use je_base::promise::Promise;
use je_base::uri::URI;
use je_editor::services::clipboard_service::IClipboardService;
use je_platform::accessibility::IAccessibilityService;
use je_platform::actions::{MenuActionOptions, menu_ids};
use je_platform::commands::commands::ICommandService;
use je_platform::configuration::ConfigurationOverrides;
use je_platform::contextkey::contextkey::{IContextKey, IContextKeyService};
use je_platform::contextview::context_view::{AnchorTarget, ContextMenuRequest, IContextMenuDelegate, IContextMenuMenuDelegate};
use je_platform::contextview::view_element::ViewElement;
use je_platform::dialogs::dialogs::IDialogService;
use je_platform::environment::environment::EnvironmentService;
use je_platform::files::files::FileSystemProviderCapabilities;
use je_platform::files::i_file_service::IFileService;
use je_platform::label::label::{ILabelService, WorkspaceLabelTarget};
use je_platform::notification::notification::INotificationService;
use je_platform::opener::editor_types::{EditorOpenSource, EditorOptions};
use je_platform::progress::progress::{IProgressService, ProgressLocation, ProgressOptions};
use je_platform::storage::storage::{StorageScope, StorageService, StorageTarget};
use je_platform::telemetry::telemetry::ITelemetryService;
use je_platform::uri_identity::uri_identity::IUriIdentityService;
use je_platform::workspace::{IWorkspace, IWorkspaceContextService, WorkbenchState, is_untitled_workspace};
use je_storage::StorageValue;
use je_ui::{NodeId, Role, Tree, Ui};
use je_widgets::dom::{create_el, is_ancestor_of_active_element};
use je_widgets::list::list_styles::IListStyles;
use je_widgets::list::list_types::{BrowserEvent, IIdentityProvider, IKeyboardNavigationLabelProvider, IListVirtualDelegate, TemplateData};
use je_widgets::tree::abstract_tree::{AbstractTreePart, ExpandOnlyOnTwistieClick};
use je_widgets::tree::async_data_tree_node::{AsyncDataTreeNodeWrapper, IAsyncDataTreeRenderer, IAsyncDataTreeViewState};
use je_widgets::tree::compressible_async_data_tree::{
    CompressibleAsyncDataTreeNodeWrapper, CompressibleAsyncDataTreeOptions, ICompressibleAsyncDataTreeRenderer, ICompressibleAsyncKeyboardNavigationLabelProvider, ITreeCompressionDelegate,
};
use je_widgets::tree::tree_types::{
    IAsyncDataSource, ITreeFilter, ITreeSorter, InputOrElement, TreeElementRenderDetails, TreeFilterResult, TreeMouseEventTarget, TreeNodeApi, TreeVisibility,
};

use super::explorer_viewer::{
    ExplorerCompressionDelegate, ExplorerDelegate, FileSorter, ICompressedNavigationController, IExplorerFindHighlightTree,
};
use super::explorer_viewer_data::{ExplorerDataSource, ExplorerFindActiveKey, ExplorerFindProvider, ExplorerFindTree, ExplorerInput, FilesFilter};
use super::explorer_viewer_dnd::{ExternalFileDropHandler, FileDragAndDrop, FileDragAndDropServices};
use super::explorer_viewer_renderer::{FILES_RENDERER_TEMPLATE_ID, FileIconThemeInfo, FilesRenderer, FilesRendererServices, InlineEditServices};
use crate::api::common::ext_host_values::ApiValue;
use crate::browser::labels::{IResourceLabelsContainer, ResourceLabels, ResourceLabelsServices};
use crate::browser::parts::views::view_pane::{IViewPaneOptions, ListOverrideStyleColors, ViewPane, ViewPaneDelegate, ViewPaneServices};
use crate::browser::workbench_tree::{IWorkbenchTreeOptions, WorkbenchCompressibleAsyncDataTree, WorkbenchTreeServices};
use crate::common::editor::{EditorResourceAccessor, EditorResourceAccessorOptions, SideBySideEditor, TextEditorOptions, UntypedEditorInput};
use crate::contrib::files::browser_files::{ExplorerReveal, IExplorerService, IExplorerView};
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

impl ContextMenuArg {
    /// O `arg: URI | {}` como valor da API (o `URI` na forma de [`ApiValue`], o objeto vazio sem propriedades).
    pub fn to_dynamic(&self) -> Dynamic {
        match self {
            ContextMenuArg::Resource(resource) => <URI as ApiValue>::to_dynamic(resource),
            ContextMenuArg::Empty => Dynamic::object(std::iter::empty::<(&str, Dynamic)>()),
        }
    }
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

/// Onde a vista guarda a árvore depois de criá-la (o `this.tree` que o `() => this.tree` do original lê).
pub type ExplorerTreeSlot = Rc<RefCell<Option<Rc<ExplorerTree>>>>;

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

/// `NEW_FOLDER_COMMAND_ID` (`fileActions.ts`).
pub const NEW_FOLDER_COMMAND_ID: &str = "explorer.newFolder";

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
    /// O `instantiationService` do `createTree`: `None` instancia os componentes reais ([`FilesExplorerTreeFactory`]).
    pub tree_factory: Option<Rc<dyn ExplorerTreeFactory>>,
}

/// `isShowingFilterResults()` do `ExplorerFindProvider`: a parte do provedor de busca que a vista usa.
pub type IsShowingFilterResults = Rc<dyn Fn() -> bool>;

/// O `FilesFilter` no que a vista usa dele: o filtro da árvore (`filter` e `isItemVisible`), o `onDidChange` e o `dispose`.
pub struct ExplorerFilter {
    pub tree_filter: Rc<dyn ITreeFilter<Rc<ExplorerItem>, ()>>,
    pub on_did_change: Event<()>,
    pub disposable: DisposableRef,
}

/// O `FilesRenderer` no que a vista usa dele: o renderer dos nós da árvore, o `getCompressedNavigationController` (que o
/// `getContext` e o menu de contexto consultam) e o `dispose`.
pub trait IExplorerRenderer: ICompressibleAsyncDataTreeRenderer<ExplorerInput, Rc<ExplorerItem>, ()> + CompressedNavigationControllerProvider + IDisposable {}

/// O que o `createTree` entrega aos `createInstance` dos componentes, e que depende da vista.
pub struct ExplorerComponentsArgs {
    /// `{ onDidChangeVisibility: this.onDidChangeBodyVisibility }` do `ResourceLabels`.
    pub labels_container: IResourceLabelsContainer,
    /// `() => this.tree` do `ExplorerFindProvider` e o `updateWidth` do `FilesRenderer`.
    pub tree_slot: ExplorerTreeSlot,
    /// `ExplorerFindProviderActive.bindTo(contextKeyService)`.
    pub find_active_key: Rc<dyn ExplorerFindActiveKey>,
    /// `(item) => this.isItemCollapsed(item)` do `FileDragAndDrop`.
    pub is_item_collapsed: Rc<dyn Fn(&Rc<ExplorerItem>) -> bool>,
}

/// Os componentes que o `createTree` instancia antes da árvore, cada um no contrato que a vista usa.
pub struct ExplorerComponents {
    pub filter: ExplorerFilter,
    /// O `ResourceLabels`: a vista só o descarta (quem o usa é o renderer).
    pub labels: DisposableRef,
    pub find_provider: IsShowingFilterResults,
    pub renderer: Rc<dyn IExplorerRenderer>,
    /// O `FileDragAndDrop`: a vista só o descarta (a opção `dnd` ainda não existe na árvore do `je-widgets`).
    pub drag_and_drop: DisposableRef,
    pub data_source: Rc<dyn IAsyncDataSource<ExplorerInput, Rc<ExplorerItem>>>,
    pub sorter: Rc<dyn ITreeSorter<Rc<ExplorerItem>>>,
}

/// Os argumentos do `createInstance(WorkbenchCompressibleAsyncDataTree, 'FileExplorer', container, ...)`.
pub struct ExplorerTreeArgs {
    pub container: NodeId,
    pub delegate: Rc<dyn IListVirtualDelegate<Rc<ExplorerItem>>>,
    pub compression_delegate: Rc<dyn ITreeCompressionDelegate<Rc<ExplorerItem>>>,
    pub renderers: Vec<Rc<dyn ICompressibleAsyncDataTreeRenderer<ExplorerInput, Rc<ExplorerItem>, ()>>>,
    pub data_source: Rc<dyn IAsyncDataSource<ExplorerInput, Rc<ExplorerItem>>>,
    pub options: CompressibleAsyncDataTreeOptions<Rc<ExplorerItem>, ()>,
    pub workbench_options: IWorkbenchTreeOptions,
}

/// O `instantiationService.createInstance` que o `createTree` usa para os componentes e para a árvore.
pub trait ExplorerTreeFactory {
    /// `createInstance(FilesFilter | ResourceLabels | ExplorerFindProvider | FilesRenderer | FileDragAndDrop | ExplorerDataSource |
    /// FileSorter)`.
    fn create_components(&self, services: &ExplorerViewServices, args: ExplorerComponentsArgs) -> ExplorerComponents;

    /// `createInstance(WorkbenchCompressibleAsyncDataTree, 'FileExplorer', container, delegate, compressionDelegate, renderers,
    /// dataSource, options, workbenchOptions)`.
    fn create_tree(&self, services: &ExplorerViewServices, args: ExplorerTreeArgs) -> Rc<ExplorerTree>;
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

/// O `{ getCompressedNavigationController(stat) }` do `getContext` é o `FilesRenderer`, que o adaptador entrega.
impl CompressedNavigationControllerProvider for FilesRendererAdapter {
    fn get_compressed_navigation_controller(&self, stat: &Rc<ExplorerItem>) -> Option<Vec<Rc<dyn ICompressedNavigationController>>> {
        self.0.get_compressed_navigation_controller(stat).map(|controllers| controllers.into_iter().map(|controller| controller as Rc<dyn ICompressedNavigationController>).collect())
    }
}

impl IDisposable for FilesRendererAdapter {
    fn dispose(&self) {
        self.0.dispose();
    }
}

impl IExplorerRenderer for FilesRendererAdapter {}

/// A árvore do `ExplorerView` vista pelo `ExplorerFindProvider` e pela lógica de colapso. A árvore é criada depois do
/// provedor (o `() => this.tree` do original), então o handle lê o campo da vista.
#[derive(Clone)]
pub struct ExplorerTreeHandle {
    slot: ExplorerTreeSlot,
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

/// A fábrica de produção: o `createInstance` real, com os serviços da vista.
pub struct FilesExplorerTreeFactory;

impl ExplorerTreeFactory for FilesExplorerTreeFactory {
    fn create_components(&self, services: &ExplorerViewServices, args: ExplorerComponentsArgs) -> ExplorerComponents {
        let configuration_service = services.pane.configuration_service.clone();

        let filter = FilesFilter::new(
            services.context_service.clone(),
            configuration_service.clone(),
            services.explorer_service.clone(),
            services.editor_service.clone(),
            services.uri_identity_service.clone(),
            services.file_service.clone(),
        );
        let explorer_labels = ResourceLabels::new(args.labels_container, services.resource_labels.clone());

        let tree_handle = ExplorerTreeHandle { slot: args.tree_slot.clone() };
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
            args.find_active_key,
            Rc::new(services.uri_identity_service.ext_uri()),
        );

        let update_width: Rc<dyn Fn(&Rc<ExplorerItem>)> = {
            let slot = args.tree_slot;
            Rc::new(move |stat| {
                let tree = slot.borrow().clone();
                if let Some(tree) = tree {
                    tree.update_width(stat);
                }
            })
        };
        let renderer = FilesRenderer::new(FilesRendererServices {
            ui: services.pane.ui.clone(),
            labels: explorer_labels.clone(),
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

        let drag_and_drop = Rc::new(FileDragAndDrop::new(
            args.is_item_collapsed,
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

        let data_source = Rc::new(ExplorerDataSource::new(
            filter.clone(),
            find_provider.clone(),
            services.progress_service.clone(),
            configuration_service,
            services.notification_service.clone(),
            services.layout_service.clone(),
            services.file_service.clone(),
            services.explorer_service.clone(),
            services.context_service.clone(),
            services.files_config_service.clone(),
        ));

        ExplorerComponents {
            filter: ExplorerFilter {
                tree_filter: filter.clone(),
                on_did_change: filter.on_did_change(),
                disposable: to_disposable({
                    let filter = filter.clone();
                    move || filter.dispose()
                }),
            },
            labels: explorer_labels,
            find_provider: Rc::new(move || find_provider.is_showing_filter_results()),
            renderer: Rc::new(FilesRendererAdapter(renderer)),
            drag_and_drop,
            data_source,
            sorter: Rc::new(FileSorter::new(services.explorer_service.clone(), services.context_service.clone())),
        }
    }

    fn create_tree(&self, services: &ExplorerViewServices, args: ExplorerTreeArgs) -> Rc<ExplorerTree> {
        ExplorerTree::new(
            "FileExplorer",
            &services.pane.ui,
            args.container,
            args.delegate,
            args.compression_delegate,
            args.renderers,
            args.data_source,
            args.options,
            args.workbench_options,
            &services.tree,
        )
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

    tree_slot: ExplorerTreeSlot,
    filter: RefCell<Option<Rc<dyn ITreeFilter<Rc<ExplorerItem>, ()>>>>,
    find_provider: RefCell<Option<IsShowingFilterResults>>,
    renderer: RefCell<Option<Rc<dyn IExplorerRenderer>>>,

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
    /// O que o original guarda em `tree.getInput()` (ver as divergências do módulo): a pasta ou as raízes.
    tree_input: RefCell<Option<ExplorerTreeInput>>,
    /// A fonte de dados da árvore, que resolve os itens do topo de um espaço de uma pasta.
    data_source: RefCell<Option<Rc<dyn IAsyncDataSource<ExplorerInput, Rc<ExplorerItem>>>>>,
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
            tree_input: RefCell::new(None),
            data_source: RefCell::new(None),
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
    pub fn tree(&self) -> Rc<ExplorerTree> {
        self.tree_slot.borrow().clone().expect("o corpo do explorador ainda não foi desenhado")
    }

    /// O `FilesRenderer` criado por `createTree` (o `renderer!` do original), no contrato que a vista usa.
    pub fn renderer(&self) -> Rc<dyn IExplorerRenderer> {
        self.renderer.borrow().clone().expect("o corpo do explorador ainda não foi desenhado")
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

    /// `isItemCollapsed(item)`: a pasta da entrada não é um nó da árvore e nunca está recolhida.
    pub fn is_item_collapsed(&self, item: &Rc<ExplorerItem>) -> bool {
        if self.is_tree_input_item(item) {
            return false;
        }
        self.tree().is_collapsed(&InputOrElement::Element(item.clone()))
    }

    /// O `this` como `Rc`, para as funções `async` (o `ExplorerView` só existe dentro de um `Rc`).
    fn rc(&self) -> Rc<Self> {
        self.weak_self.upgrade().expect("o ExplorerView foi descartado")
    }

    /// `treeInput === item` quando a entrada é a pasta do espaço de uma pasta.
    fn is_tree_input_item(&self, item: &Rc<ExplorerItem>) -> bool {
        matches!(&*self.tree_input.borrow(), Some(ExplorerTreeInput::Item(root)) if Rc::ptr_eq(root, item))
    }

    /// `tree.hasNode(item)`: a pasta da entrada é a raiz da árvore e sempre existe.
    fn has_node(&self, item: &Rc<ExplorerItem>) -> bool {
        self.is_tree_input_item(item) || self.tree().has_node(&InputOrElement::Element(item.clone()))
    }

    /// `tree.expand(item)`: a pasta da entrada já está sempre expandida.
    fn expand_item(&self, item: &Rc<ExplorerItem>) -> Promise<bool> {
        if self.is_tree_input_item(item) {
            return Promise::resolved(false);
        }
        self.tree().widget().expand(item, false)
    }

    /// Os itens do topo da árvore para a entrada: as raízes, ou os filhos da pasta (ver as divergências do módulo).
    fn resolve_top_level(&self, input: &ExplorerTreeInput) -> Promise<ExplorerInput> {
        match input {
            ExplorerTreeInput::Roots(roots) => Promise::resolved(roots.clone()),
            ExplorerTreeInput::Item(root) => {
                let data_source = self.data_source.borrow().clone().expect("o corpo do explorador ainda não foi desenhado");
                let children = data_source.get_children(&InputOrElement::Element(root.clone()));
                async_fn(async move { await_maybe(children).await })
            }
        }
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

        // `this.instantiationService.createInstance(...)`: a fábrica dos serviços, ou os componentes reais.
        let production_factory = FilesExplorerTreeFactory;
        let factory: &dyn ExplorerTreeFactory = services.tree_factory.as_deref().unwrap_or(&production_factory);

        let components = factory.create_components(
            services,
            ExplorerComponentsArgs {
                labels_container: IResourceLabelsContainer { on_did_change_visibility: self.pane.on_did_change_body_visibility() },
                tree_slot: self.tree_slot.clone(),
                find_active_key: Rc::new(ContextKeyFindActive(explorer_find_provider_active().bind_to(keys))),
                is_item_collapsed: {
                    let weak = self.weak_self.clone();
                    Rc::new(move |item: &Rc<ExplorerItem>| weak.upgrade().is_some_and(|this| this.is_item_collapsed(item)))
                },
            },
        );

        self.disposables.register(components.filter.disposable.clone());
        *self.filter.borrow_mut() = Some(components.filter.tree_filter.clone());
        let weak = self.weak_self.clone();
        self.disposables.register(components.filter.on_did_change.listen(move |_| {
            if let Some(this) = weak.upgrade() {
                let _ = this.refresh(true, None, None);
            }
        }));
        self.disposables.register(components.labels.clone());
        *self.find_provider.borrow_mut() = Some(components.find_provider.clone());
        self.disposables.register(components.renderer.clone());
        *self.renderer.borrow_mut() = Some(components.renderer.clone());
        self.disposables.register(components.drag_and_drop.clone());

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

        let mut options = CompressibleAsyncDataTreeOptions::<Rc<ExplorerItem>, ()>::default();
        options.compression_enabled = Some(compression_enabled());
        options.auto_expand_compressed_children = true;
        options.keyboard_navigation_label_provider = Some(Rc::new(ExplorerKeyboardNavigationLabelProvider { explorer_service: services.explorer_service.clone() }));
        options.base.identity_provider = Some(Rc::new(ExplorerIdentityProvider));
        options.base.update.multiple_selection_support = Some(true);
        options.base.filter = Some(components.filter.tree_filter.clone());
        options.base.sorter = Some(components.sorter.clone());
        options.base.collapse_by_default = Some({
            let (is_showing_filter_results, get_file_nesting_expand) = (components.find_provider.clone(), get_file_nesting_expand.clone());
            Rc::new(move |item: &Rc<ExplorerItem>| {
                let expand_nests = item.has_nests() && get_file_nesting_expand(item);
                collapse_by_default(item.has_nests(), expand_nests, is_showing_filter_results())
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

        let renderer: Rc<dyn ICompressibleAsyncDataTreeRenderer<ExplorerInput, Rc<ExplorerItem>, ()>> = components.renderer.clone();
        let workbench_options = IWorkbenchTreeOptions {
            override_styles: Some((services.list_override_styles)(&self.pane.get_location_based_colors().list_override_styles)),
            ..IWorkbenchTreeOptions::default()
        };
        *self.data_source.borrow_mut() = Some(components.data_source.clone());
        let tree = factory.create_tree(
            services,
            ExplorerTreeArgs {
                container,
                delegate: Rc::new(ExplorerDelegate),
                compression_delegate: Rc::new(ExplorerCompressionDelegate),
                renderers: vec![renderer],
                data_source: components.data_source,
                options,
                workbench_options,
            },
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
                let _ = this.on_context_menu(&ExplorerContextMenuEvent {
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
            let _ = self.refresh(true, None, None);
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
                let enable_trash = self.services.pane.configuration_service.get_value(None, None).get_str("files").optional_get_str("enableTrash").is_truthy();
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

/// `uri.toString()`.
fn uri_text(uri: &URI) -> String {
    uri.to_string(false).to_string_lossy()
}

/// O `IContextMenuDelegate` base do `showContextMenu` de `onContextMenu`: a âncora e o foco de volta na árvore ao cancelar.
struct ExplorerContextMenuDelegate {
    anchor: AnchorTarget,
    tree: Weak<ExplorerTree>,
}

impl IContextMenuDelegate for ExplorerContextMenuDelegate {
    fn get_anchor(&self) -> AnchorTarget {
        self.anchor.clone()
    }

    /// O `IContextMenuMenuDelegate` ignora o `getActions` do delegado base: as ações vêm do `menuId`.
    fn get_actions(&self) -> Vec<ActionRef> {
        Vec::new()
    }

    fn on_hide(&self, did_cancel: bool) {
        if did_cancel && let Some(tree) = self.tree.upgrade() {
            tree.dom_focus();
        }
    }
}

/// Os nós `.label-name` dentro de `.explorer-item` abaixo de `node` (`querySelectorAll('.explorer-item .label-name')`).
fn collect_explorer_label_names(tree: &Tree, node: NodeId, inside_item: bool, out: &mut Vec<NodeId>) {
    for &child in tree.children(node) {
        if inside_item && tree.has_class(child, "label-name") {
            out.push(child);
        }
        collect_explorer_label_names(tree, child, inside_item || tree.has_class(child, "explorer-item"), out);
    }
}

/// `createFileIconThemableTreeContainerScope(container, themeService)`: `theme` é o `themeService.getFileIconTheme()` e
/// `on_did_file_icon_theme_change` o `themeService.onDidFileIconThemeChange`. Devolve o que desfaz a escuta.
pub fn create_file_icon_themable_tree_container_scope(ui: &Ui, container: NodeId, theme: FileIconThemeInfo, on_did_file_icon_theme_change: Event<FileIconThemeInfo>) -> DisposableRef {
    {
        let mut tree = ui.tree_mut();
        tree.add_class(container, "file-icon-themable-tree");
        tree.add_class(container, "show-file-icons");
    }

    let ui = ui.clone();
    let on_did_change_file_icon_theme = move |theme: &FileIconThemeInfo| {
        let mut tree = ui.tree_mut();
        tree.toggle_class(container, "align-icons-and-twisties", theme.has_file_icons && !theme.has_folder_icons);
        tree.toggle_class(container, "hide-arrows", theme.hides_explorer_arrows);
    };

    on_did_change_file_icon_theme(&theme);
    on_did_file_icon_theme_change.listen(on_did_change_file_icon_theme)
}

impl ExplorerView {
    /// `this.findProvider?.isShowingFilterResults()`.
    fn is_showing_filter_results(&self) -> bool {
        self.find_provider.borrow().as_ref().is_some_and(|is_showing_filter_results| is_showing_filter_results())
    }

    /// `selectActiveFile(reveal = this._autoReveal)`.
    fn select_active_file(self: &Rc<Self>, reveal: Option<ExplorerReveal>) -> Promise<()> {
        let this = self.clone();
        let reveal = reveal.unwrap_or_else(|| self.auto_reveal());
        async_fn(async move {
            if reveal_is_enabled(&this.auto_reveal.borrow()) {
                let options = EditorResourceAccessorOptions { support_side_by_side: Some(SideBySideEditor::Primary), filter_by_scheme: None };
                let active_editor = this.services.editor_service.active_editor();
                if let Some(active_file) = EditorResourceAccessor::get_canonical_uri(active_editor.as_ref(), Some(&options)) {
                    let tree = this.tree();
                    let (focus, selection) = (tree.get_focus(), tree.get_selection());
                    let ext_uri = this.services.uri_identity_service.ext_uri();
                    if focus.len() == 1
                        && ext_uri.is_equal(Some(&focus[0].resource()), Some(&active_file), false)
                        && selection.len() == 1
                        && ext_uri.is_equal(Some(&selection[0].resource()), Some(&active_file), false)
                    {
                        // No action needed, active file is already focused and selected
                        return Ok(());
                    }
                    return this.services.explorer_service.select(&active_file, Some(reveal)).await;
                }
            }
            Ok(())
        })
    }

    /// `selectResource(resource, reveal, retry)`, com os padrões já resolvidos.
    fn select_resource_with_retry(self: &Rc<Self>, resource: Option<URI>, reveal: ExplorerReveal, retry: usize) -> Promise<()> {
        let this = self.clone();
        async_fn(async move {
            // do no retry more than once to prevent infinite loops in cases of inconsistent model
            if retry == 2 {
                return Ok(());
            }

            let Some(resource) = resource.filter(|_| this.pane.is_body_visible()) else {
                return Ok(());
            };

            // If something is refreshing the explorer, we must await it or else a selection race condition can occur
            this.set_tree_input_sequencer.queue(|| Promise::resolved(())).await?;

            // Expand all stats in the parent chain.
            let ext_uri = this.services.uri_identity_service.ext_uri();
            let mut item = this.services.explorer_service.find_closest_root(&resource);
            while let Some(current) = item.clone() {
                if uri_text(&current.resource()) == uri_text(&resource) {
                    break;
                }
                if this.expand_item(&current).await.is_err() {
                    return this.select_resource_with_retry(Some(resource), reveal, retry + 1).await;
                }
                item = current.children().into_iter().find(|child| ext_uri.is_equal_or_parent(&resource, &child.resource(), false, false));
            }

            let Some(item) = item else {
                return Ok(());
            };

            let tree = this.tree();
            if this.is_tree_input_item(&item) {
                tree.set_focus(&[], None);
                tree.set_selection(&[], None);
                return Ok(());
            }

            // We must expand the nest to have it be populated in the tree
            if let Some(nested_parent) = item.nested_parent()
                && this.expand_item(&nested_parent).await.is_err()
            {
                return this.select_resource_with_retry(Some(resource), reveal, retry + 1).await;
            }

            let selected = errors::catch(|| {
                let force = matches!(&reveal, ExplorerReveal::Flag(true)) || matches!(&reveal, ExplorerReveal::Mode(mode) if mode == "force");
                if force && tree.get_relative_top(&item).is_none() {
                    // Don't scroll to the item if it's already visible, or if set not to.
                    tree.reveal(&item, Some(0.5));
                }

                tree.set_focus(&[item.clone()], None);
                tree.set_selection(&[item.clone()], None);
            });
            if selected.is_err() {
                // Element might not be in the tree, try again and silently fail
                return this.select_resource_with_retry(Some(resource), reveal, retry + 1).await;
            }
            Ok(())
        })
    }

    /// `doSetTreeInput()`.
    fn do_set_tree_input(self: &Rc<Self>) -> Promise<()> {
        let this = self.clone();
        async_fn(async move {
            if !this.pane.is_body_visible() {
                return Ok(());
            }

            let tree = this.tree();
            let initial_input_setup = tree.get_input().is_none();
            if initial_input_setup {
                mark("code/willResolveExplorer", None);
            }
            let roots = this.services.explorer_service.roots();
            let workbench_state = this.services.context_service.get_workbench_state();
            // `roots[0].error` só é lido num espaço de uma pasta (o `||` do original curto-circuita).
            let first_root_has_error = workbench_state == WorkbenchState::Folder
                && roots.first().ok_or_else(|| Error::type_error("Cannot read properties of undefined (reading 'error')"))?.error().is_some();
            // Display roots only when multi folder workspace
            let input = choose_tree_input(workbench_state, &roots, first_root_has_error);

            let view_state = if tree.get_input().is_some() {
                Some(tree.get_view_state())
            } else {
                let raw_view_state = this.services.storage_service.get(TREE_VIEW_STATE_STORAGE_KEY, StorageScope::Workspace, None).filter(|raw| !raw.is_empty());
                match raw_view_state {
                    Some(raw) => Some(tree_view_state_from_json(&raw).ok_or_else(|| Error::new("Unexpected token in the stored explorer tree view state"))?),
                    None => None,
                }
            };

            let previous_input = this.tree_input.borrow().clone();
            let top_level = this.resolve_top_level(&input).await?;
            *this.tree_input.borrow_mut() = Some(input.clone());

            let promise = {
                let (this, tree, view_state) = (this.clone(), tree.clone(), view_state.clone());
                async_fn(async move {
                    tree.widget().set_input(top_level, view_state.clone()).await?;
                    if let ExplorerTreeInput::Roots(input) = &input {
                        if view_state.is_none() || matches!(previous_input, Some(ExplorerTreeInput::Item(_))) {
                            // There is no view state for this workspace (we transitioned from a folder workspace?), expand up to five roots.
                            // If there are many roots in a workspace, expanding them all would can cause performance issues #176226
                            for root in input.iter().take(roots_to_expand_without_view_state(input.len())) {
                                let _ = tree.widget().expand(root, false).await;
                            }
                        }
                        // Reloaded or transitioned from an empty workspace, but only have a single folder in the workspace.
                        if previous_input.is_none()
                            && input.len() == 1
                            && this.services.pane.configuration_service.get_value(None, None).get_str("explorer").get_str("expandSingleFolderWorkspaces").is_truthy()
                        {
                            let _ = tree.widget().expand(&input[0], false).await;
                        }
                        if let Some(ExplorerTreeInput::Roots(previous_input)) = &previous_input {
                            let previous_roots: HashSet<String> = previous_input.iter().map(|previous_root| uri_text(&previous_root.resource())).collect();

                            // Roots added to the explorer -> expand them.
                            let expansions: Vec<Promise<bool>> =
                                input.iter().filter(|item| !previous_roots.contains(&uri_text(&item.resource()))).map(|item| tree.widget().expand(item, false)).collect();
                            for expansion in expansions {
                                let _ = expansion.await;
                            }
                        }
                    }
                    if initial_input_setup {
                        mark("code/didResolveExplorer", None);
                    }
                    Ok(())
                })
            };

            let mut options = ProgressOptions::new(ProgressLocation::Explorer);
            // reduce progress visibility when still restoring
            options.delay = Some(if this.services.layout_service.is_restored() { 800.0 } else { 1500.0 });
            let tracked = promise.clone();
            let _ = this.services.progress_service.with_progress(options, Box::new(move |_progress| tracked.then(|_| ())), None);

            promise.await
        })
    }

    /// O `refresh` de uma entrada que é a pasta do espaço: a lista dos filhos fica fixa na entrada da árvore, então a entrada é
    /// refeita com o estado da vista (ver as divergências do módulo).
    fn refresh_tree_input(&self, root: Rc<ExplorerItem>) -> Promise<()> {
        let (this, tree) = (self.rc(), self.tree());
        async_fn(async move {
            let top_level = this.resolve_top_level(&ExplorerTreeInput::Item(root)).await?;
            let view_state = tree.get_view_state();
            tree.widget().set_input(top_level, Some(view_state)).await
        })
    }

    /// `onContextMenu(e)`.
    pub fn on_context_menu(&self, event: &ExplorerContextMenuEvent) -> Promise<()> {
        let browser_event = event.browser_event;
        if browser_event.target.is_some_and(|target| self.ui.tree().role(target) == Role::TextInput) {
            return Promise::resolved(());
        }

        let tree = self.tree();
        let stat = event.element.clone();
        if let Some(selection) = selection_for_context_menu(stat.as_ref(), &tree.get_selection()) {
            // Do not forward the keyboard event: selecting the context-menu target must not open it.
            tree.set_selection(&selection, None);
        }

        let anchor = AnchorTarget::Element(ViewElement::from_native(Rc::new(browser_event.target.unwrap_or_else(|| tree.get_html_element()))));

        let this = self.rc();
        async_fn(async move {
            // update dynamic contexts
            let has_resources = this.services.clipboard_service.has_resources().await?;
            this.file_copied_context_key().set(Dynamic::Bool(has_resources));
            this.set_context_keys(stat.as_ref());

            let selection = tree.get_selection();

            // If the click is outside of the elements pass the root resource if there is only one root. If there are multiple roots pass empty object.
            let roots = this.services.explorer_service.roots();
            let renderer = this.renderer();
            let arg = context_menu_arg(stat.as_ref(), renderer.as_ref(), &roots);
            let actions_context = context_menu_actions_context(stat.as_ref(), &selection);

            let context_key_service: Rc<dyn IContextKeyService> = tree.context_key_service();
            this.services.pane.context_menu_service.show_context_menu(ContextMenuRequest::Menu(IContextMenuMenuDelegate {
                menu_id: Some(menu_ids::explorer_context()),
                menu_action_options: Some(MenuActionOptions { arg: Some(arg.to_dynamic()), should_forward_args: true, ..MenuActionOptions::default() }),
                get_actions_context: Some(Rc::new(move |_: Option<&IContextMenuEvent>| Some(Dynamic::array(actions_context.iter().map(<URI as ApiValue>::to_dynamic))))),
                context_key_service: Some(context_key_service),
                get_actions: None,
                base: Rc::new(ExplorerContextMenuDelegate { anchor, tree: Rc::downgrade(&tree) }),
            }));
            Ok(())
        })
    }

    /// `expandAll()`.
    pub fn expand_all(&self) {
        if self.services.explorer_service.is_editable(None) {
            self.tree().dom_focus();
        }

        self.tree().expand_all();
    }

    /// `collapseAll()`.
    pub fn collapse_all(&self) {
        let tree = self.tree();
        if self.services.explorer_service.is_editable(None) {
            tree.dom_focus();
        }

        let tree_input = self.tree_input.borrow().clone();
        if let Some(tree_input) = &tree_input
            && collapse_root_children(&ExplorerTreeHandle { slot: self.tree_slot.clone() }, tree_input)
        {
            return;
        }

        tree.collapse_all();
    }

    /// O corpo comum de `previousCompressedStat`, `nextCompressedStat`, `firstCompressedStat` e `lastCompressedStat`.
    fn navigate_compressed_stat(&self, navigate: impl Fn(&dyn ICompressedNavigationController)) {
        let focused = self.tree().get_focus();
        let Some(focused) = focused.first() else {
            return;
        };

        let Some(compressed_navigation_controllers) = self.renderer().get_compressed_navigation_controller(focused) else {
            errors::throw(Error::type_error("Cannot read properties of undefined (reading 'forEach')"))
        };
        for controller in compressed_navigation_controllers {
            navigate(controller.as_ref());
            self.update_compressed_navigation_context_keys(controller.as_ref());
        }
    }

    /// `previousCompressedStat()`.
    pub fn previous_compressed_stat(&self) {
        self.navigate_compressed_stat(|controller| controller.previous());
    }

    /// `nextCompressedStat()`.
    pub fn next_compressed_stat(&self) {
        self.navigate_compressed_stat(|controller| controller.next());
    }

    /// `firstCompressedStat()`.
    pub fn first_compressed_stat(&self) {
        self.navigate_compressed_stat(|controller| controller.first());
    }

    /// `lastCompressedStat()`.
    pub fn last_compressed_stat(&self) {
        self.navigate_compressed_stat(|controller| controller.last());
    }
}

impl IExplorerView for ExplorerView {
    fn id(&self) -> String {
        self.pane.id.clone()
    }

    /// `autoReveal`.
    fn auto_reveal(&self) -> ExplorerReveal {
        self.auto_reveal.borrow().clone()
    }

    /// `set autoReveal(autoReveal)`.
    fn set_auto_reveal(&self, value: ExplorerReveal) {
        *self.auto_reveal.borrow_mut() = value;
    }

    /// `getContext(respectMultiSelection)`.
    fn get_context(&self, respect_multi_selection: bool) -> Vec<Rc<ExplorerItem>> {
        let tree = self.tree();
        let focused_items = if tree.get_focused_part() == AbstractTreePart::StickyScroll { tree.get_sticky_scroll_focus() } else { tree.get_focus() };
        let renderer = self.renderer();
        get_context(&focused_items, &tree.get_selection(), respect_multi_selection, renderer.as_ref())
    }

    /// Refresh the contents of the explorer to get up to date data from the disk about the file structure. If the item is
    /// passed we refresh only that level of the tree, otherwise we do a full refresh.
    fn refresh(&self, recursive: bool, item: Option<&Rc<ExplorerItem>>, cancel_editing: Option<bool>) -> Promise<()> {
        let cancel_editing = cancel_editing.unwrap_or(true);
        let Some(tree) = self.tree_slot.borrow().clone() else {
            return Promise::resolved(());
        };
        if !self.pane.is_body_visible() || item.is_some_and(|item| !self.has_node(item)) || (self.is_showing_filter_results() && recursive) {
            // Tree node doesn't exist yet, when it becomes visible we will refresh
            return Promise::resolved(());
        }

        if cancel_editing && self.services.explorer_service.is_editable(None) {
            tree.dom_focus();
        }

        // `item || this.tree.getInput()`: a pasta da entrada refaz a entrada.
        let input_root = match item {
            Some(item) => Some(item).filter(|item| self.is_tree_input_item(item)).cloned(),
            None => match &*self.tree_input.borrow() {
                Some(ExplorerTreeInput::Item(root)) => Some(root.clone()),
                _ => None,
            },
        };
        if let Some(root) = input_root {
            return self.refresh_tree_input(root);
        }
        match item {
            Some(item) => tree.widget().update_children(Some(&InputOrElement::Element(item.clone())), recursive, true, None),
            None => tree.widget().update_children(None, recursive, false, None),
        }
    }

    /// `selectResource(resource, reveal = this._autoReveal, retry = 0)`.
    fn select_resource(&self, resource: Option<&URI>, reveal: Option<ExplorerReveal>, retry: Option<usize>) -> Promise<()> {
        self.rc().select_resource_with_retry(resource.cloned(), reveal.unwrap_or_else(|| self.auto_reveal()), retry.unwrap_or(0))
    }

    /// `setTreeInput()`.
    fn set_tree_input(&self) -> Promise<()> {
        let this = self.rc();
        self.set_tree_input_sequencer.queue(move || this.do_set_tree_input())
    }

    /// `itemsCopied(stats, cut, previousCut)`.
    fn items_copied(&self, stats: &[Rc<ExplorerItem>], cut: bool, previous_cut: Option<&[Rc<ExplorerItem>]>) {
        self.file_copied_context_key().set(Dynamic::Bool(!stats.is_empty()));
        self.resource_cut_context_key().set(Dynamic::Bool(cut && !stats.is_empty()));
        let tree = self.tree();
        for item in previous_cut.into_iter().flatten() {
            tree.rerender(Some(item));
        }
        if cut {
            for stat in stats {
                tree.rerender(Some(stat));
            }
        }
    }

    /// `setEditable(stat, isEditing)`.
    fn set_editable(&self, stat: &Rc<ExplorerItem>, is_editing: bool) -> Promise<()> {
        let (this, stat) = (self.rc(), stat.clone());
        async_fn(async move {
            let tree = this.tree();
            if is_editing {
                let Some(parent) = stat.parent() else {
                    return Err(Error::new("Data tree node not found"));
                };
                this.expand_item(&parent).await?;
            } else if let Some(tree_container) = this.tree_container.get() {
                this.ui.tree_mut().remove_class(tree_container, "highlight");
            }

            IExplorerView::refresh(&*this, false, stat.parent().as_ref(), Some(false)).await?;

            if is_editing {
                if let Some(tree_container) = this.tree_container.get() {
                    this.ui.tree_mut().add_class(tree_container, "highlight");
                }
                tree.reveal(&stat, None);
            } else {
                tree.dom_focus();
            }
            Ok(())
        })
    }

    /// `isItemVisible(item)`.
    fn is_item_visible(&self, item: &Rc<ExplorerItem>) -> bool {
        // If filter is undefined it means the tree hasn't been rendered yet, so nothing is visible
        let Some(filter) = self.filter.borrow().clone() else {
            return false;
        };
        filter_result_is_visible(&filter.filter(item, TreeVisibility::Visible))
    }

    /// `isItemCollapsed(item)`.
    fn is_item_collapsed(&self, item: &Rc<ExplorerItem>) -> Result<bool, Error> {
        errors::catch(|| ExplorerView::is_item_collapsed(self, item))
    }

    /// `hasFocus()`.
    fn has_focus(&self) -> bool {
        self.container.get().is_some_and(|container| is_ancestor_of_active_element(&self.ui, container))
    }

    /// `getFocus()`.
    fn get_focus(&self) -> Vec<Rc<ExplorerItem>> {
        self.tree().get_focus()
    }

    /// `focusNext()`.
    fn focus_next(&self) {
        self.tree().focus_next(1, false, None, None);
    }

    /// `focusLast()`.
    fn focus_last(&self) {
        self.tree().focus_last(None, None);
    }

    /// `hasPhantomElements()`.
    fn has_phantom_elements(&self) -> bool {
        self.is_showing_filter_results()
    }
}

/// O `ExplorerView` é a vista que `IViewsService.getViewWithId` devolve; `setExpanded` é o da `ViewPane` de que ele herda.
impl crate::services::views::views_service::IViewPane for ExplorerView {
    fn set_expanded(&self, expanded: bool) -> bool {
        self.pane.set_expanded(expanded)
    }

    fn focus(&self) {
        ExplorerView::focus(self);
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

    /// `getOptimalWidth()`: `DOM.getLargestChildWidth(parentNode, childNodes)` dos `.explorer-item .label-name` da árvore.
    fn get_optimal_width(&self) -> f64 {
        let parent_node = self.tree().get_html_element();
        let ui_tree = self.ui.tree();
        let mut child_nodes = Vec::new();
        collect_explorer_label_names(&ui_tree, parent_node, false, &mut child_nodes);

        // `Math.max(getTotalScrollWidth(child), getTotalWidth(child)) + getRelativeLeft(child, parent)`; `Math.max()` sem nós é `-Infinity`.
        let parent_left = ui_tree.absolute_bounds(parent_node).x0;
        child_nodes
            .into_iter()
            .map(|child| {
                let bounds = ui_tree.absolute_bounds(child);
                (bounds.x1 - bounds.x0) + (bounds.x0 - parent_left)
            })
            .fold(f64::NEG_INFINITY, f64::max)
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
