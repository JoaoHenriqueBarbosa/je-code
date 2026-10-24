//! Porte parcial de `src/vs/workbench/common/views.ts`: os contratos da árvore de visões que o `MainThreadTreeViews`
//! (`api/browser/mainThreadTreeViews.ts`) e o `CustomTreeView` (`browser/parts/views/treeView.ts`) usam entre si:
//! [`ITreeView`], [`ITreeViewDataProvider`] e [`ITreeViewDragAndDropController`].
//!
//! Os contratos têm a implementação do lado da interface em `crate::browser::parts::views::tree_view` (o `CustomTreeView`,
//! sem o `TreeViewPane` e sem a árvore desenhada) e a do lado das extensões no `MainThreadTreeViews`
//! (`api/browser/main_thread_tree_views.rs`).
//!
//! Divergências:
//! - `ITreeViewDataProvider.didUpdateItems` não existe no original: o `CustomTreeView` do original muda por identidade os
//!   objetos `ITreeItem` que o provedor devolveu (`node.children = ...`, `child.parent = ...`, `checkbox.isChecked = ...`) e
//!   o provedor (`TreeViewDataProvider`) enxerga a mudança porque guarda os mesmos objetos. Aqui os itens que cruzam o
//!   contrato são valores, então a árvore avisa o provedor das mudanças com [`ITreeViewDataProvider::did_update_items`].
//! - `Promise<readonly ITreeItem[] | undefined>` é `Promise<Option<Vec<ITreeItem>>>`; os membros opcionais da interface
//!   (`isTreeEmpty`, `onDidChangeEmpty`, `getChildrenBatch`, `dragAndDropController`, `message`, `handleDragEnd`) são
//!   métodos com implementação padrão (ausente, sem eventos), como nos outros contratos parciais do porte.
//! - `message?: string | IMarkdownString` é `Option<StringOrMarkdown>`; os `Event<...>` de seleção e foco entregam a
//!   seleção e o item com foco num [`TreeSelectionAndFocus`].
//! - `container: unknown /* HTMLElement */ | undefined` e `show(container)` não existem (sem DOM): o contêiner da árvore
//!   é da camada de interface.
//! - Os setters de propriedade do original (`dataProvider = ...`, `title = ...`, `message = ...`, `badge = ...`,
//!   `description = ...`, `showCollapseAllAction = ...`, `canSelectMany = ...`, `manuallyManageCheckboxes = ...`,
//!   `dragAndDropController = ...`) são os métodos `set_*` (`&self`, estado interno mutável).

use std::rc::Rc;

use je_base::cancellation::CancellationToken;
use je_base::data_transfer::VSDataTransfer;
use je_base::event::Event;
use je_base::lifecycle::IDisposable;
use je_base::promise::Promise;
use je_editor::languages::StringOrMarkdown;

use crate::common::views::{ITreeItem, IViewBadge};

/// `ITreeViewDataProvider`.
pub trait ITreeViewDataProvider {
    /// `isTreeEmpty?`.
    fn is_tree_empty(&self) -> Option<bool> {
        None
    }

    /// `onDidChangeEmpty?`.
    fn on_did_change_empty(&self) -> Event<()> {
        Event::none()
    }

    /// `getChildren(element?)`.
    fn get_children(&self, element: Option<&ITreeItem>) -> Promise<Option<Vec<ITreeItem>>>;

    /// `getChildrenBatch?(element?)`: `None` quando o provedor não implementa a leitura em lote.
    fn get_children_batch(&self, _elements: Option<&[ITreeItem]>) -> Option<Promise<Option<Vec<Vec<ITreeItem>>>>> {
        None
    }

    /// Os itens que a árvore mudou (filhos carregados, pai, estado do checkbox), como valores completos: o provedor que
    /// guarda a identidade dos itens as aplica (ver as divergências do módulo).
    fn did_update_items(&self, _items: &[ITreeItem]) {}
}

/// `ITreeViewDragAndDropController`.
pub trait ITreeViewDragAndDropController {
    /// `dropMimeTypes`.
    fn drop_mime_types(&self) -> Vec<String>;

    /// `dragMimeTypes`.
    fn drag_mime_types(&self) -> Vec<String>;

    /// `handleDrag(sourceTreeItemHandles, operationUuid, token)`.
    fn handle_drag(&self, source_tree_item_handles: Vec<String>, operation_uuid: &str, token: CancellationToken) -> Promise<Option<Rc<VSDataTransfer>>>;

    /// `handleDragEnd?(operationUuid)`.
    fn handle_drag_end(&self, _operation_uuid: &str) {}

    /// `handleDrop(elements, target, token, operationUuid?, sourceTreeId?, sourceTreeItemHandles?)`.
    fn handle_drop(
        &self,
        elements: Rc<VSDataTransfer>,
        target: Option<ITreeItem>,
        token: CancellationToken,
        operation_uuid: Option<&str>,
        source_tree_id: Option<&str>,
        source_tree_item_handles: Option<Vec<String>>,
    ) -> Promise<()>;
}

/// O valor de `onDidChangeSelectionAndFocus`: `{ selection: readonly ITreeItem[]; focus: ITreeItem }`.
#[derive(Clone, Debug)]
pub struct TreeSelectionAndFocus {
    pub selection: Vec<ITreeItem>,
    pub focus: ITreeItem,
}

/// `ITreeView`.
pub trait ITreeView: IDisposable {
    /// `dataProvider`.
    fn data_provider(&self) -> Option<Rc<dyn ITreeViewDataProvider>>;
    fn set_data_provider(&self, data_provider: Option<Rc<dyn ITreeViewDataProvider>>);

    /// `dragAndDropController?`.
    fn drag_and_drop_controller(&self) -> Option<Rc<dyn ITreeViewDragAndDropController>>;
    fn set_drag_and_drop_controller(&self, controller: Option<Rc<dyn ITreeViewDragAndDropController>>);

    /// `showCollapseAllAction`.
    fn show_collapse_all_action(&self) -> bool;
    fn set_show_collapse_all_action(&self, show: bool);

    /// `canSelectMany`.
    fn can_select_many(&self) -> bool;
    fn set_can_select_many(&self, can_select_many: bool);

    /// `manuallyManageCheckboxes`.
    fn manually_manage_checkboxes(&self) -> bool;
    fn set_manually_manage_checkboxes(&self, manually_manage_checkboxes: bool);

    /// `message?`.
    fn message(&self) -> Option<StringOrMarkdown>;
    fn set_message(&self, message: Option<StringOrMarkdown>);

    /// `title`.
    fn title(&self) -> String;
    fn set_title(&self, title: &str);

    /// `description`.
    fn description(&self) -> Option<String>;
    fn set_description(&self, description: Option<&str>);

    /// `badge`.
    fn badge(&self) -> Option<IViewBadge>;
    fn set_badge(&self, badge: Option<IViewBadge>);

    /// `visible`.
    fn visible(&self) -> bool;

    fn on_did_expand_item(&self) -> Event<ITreeItem>;
    fn on_did_collapse_item(&self) -> Event<ITreeItem>;
    fn on_did_change_selection_and_focus(&self) -> Event<TreeSelectionAndFocus>;
    fn on_did_change_visibility(&self) -> Event<bool>;
    fn on_did_change_actions(&self) -> Event<()>;
    fn on_did_change_title(&self) -> Event<String>;
    fn on_did_change_description(&self) -> Event<Option<String>>;
    fn on_did_change_welcome_state(&self) -> Event<()>;
    fn on_did_change_checkbox_state(&self) -> Event<Vec<ITreeItem>>;

    /// `refresh(treeItems?, checkboxesChanged?)`: `checkboxesChanged` é um subconjunto de `treeItems`.
    fn refresh(&self, tree_items: Option<&[ITreeItem]>, checkboxes_changed: Option<&[ITreeItem]>) -> Promise<()>;

    /// `setVisibility(visible)`.
    fn set_visibility(&self, visible: bool);

    /// `focus()`.
    fn focus(&self);

    /// `layout(height, width)`.
    fn layout(&self, height: f64, width: f64);

    /// `getOptimalWidth()`.
    fn get_optimal_width(&self) -> f64;

    /// `reveal(item)`.
    fn reveal(&self, item: &ITreeItem) -> Promise<()>;

    /// `expand(itemOrItems)`.
    fn expand(&self, items: &[ITreeItem]) -> Promise<()>;

    /// `isCollapsed(item)`.
    fn is_collapsed(&self, item: &ITreeItem) -> bool;

    /// `setSelection(items)`.
    fn set_selection(&self, items: &[ITreeItem]);

    /// `getSelection()`.
    fn get_selection(&self) -> Vec<ITreeItem>;

    /// `setFocus(item?)`.
    fn set_focus(&self, item: Option<&ITreeItem>);
}
