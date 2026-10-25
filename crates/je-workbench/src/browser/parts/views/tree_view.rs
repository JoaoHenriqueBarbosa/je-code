//! Porte de `src/vs/workbench/browser/parts/views/treeView.ts`: a parte lógica do `CustomTreeView` (`AbstractTreeView`), do
//! `CustomTreeViewDragAndDrop` e do cascateamento de checkboxes, que o `MainThreadTreeViews` e a suíte
//! `mainThreadTreeViews.test.ts` exercitam.
//!
//! Fora do porte (camada de interface, sem equivalente nativo ainda): `TreeViewPane` (o `ViewPane`), `TreeRenderer`, `Aligner`,
//! `TreeMenus`, `MultipleSelectionActionRunner`, `CheckboxStateHandler`, o menu de
//! contexto, o `onDidOpen` (execução do comando do item), o `resolveCommand`, os `RawContextKey` e as ações
//! `refresh`/`collapseAll` registradas por `registerActions`, a renderização da mensagem (`showMessage`, `processMessage`),
//! `show(container)`, `container`, `viewContainer`/`viewLocation` (cores por local) e `showRefreshAction`.
//!
//! Divergências:
//! - **`TreeView`** (a variante sem extensão) é o [`CustomTreeView::new_without_extension`]: a mesma struct com `extension_id`
//!   ausente, em que `activate` só cria a árvore.
//! - **A árvore (`WorkbenchAsyncDataTree`)** é o modelo lógico de nós: o [`TreeNodes`] guarda, por `handle`, o item, os
//!   filhos carregados, o pai, o conjunto de expandidos, a seleção e o foco. `expand`, `reveal`, `setSelection`, `setFocus`,
//!   `isCollapsed` e o recarregamento (`updateChildren`, que só recarrega a raiz e os nós expandidos) seguem o que a árvore
//!   faz, sem rolagem nem DOM. Um nó que o modelo não conhece é ignorado por `expand`, `setSelection` e `setFocus` (o
//!   original lança "Tree element not found", que `expand` já engolia).
//! - **Identidade dos itens.** `ITreeItem` cruza o contrato por valor: o modelo é a fonte da verdade e entrega cópias; o
//!   provedor é avisado das mudanças com [`ITreeViewDataProvider::did_update_items`]. O `Root` é o handle `"0"`.
//!   `child.parent = node` é o mapa de pais do modelo (o campo `parent` dos itens entregues é `None`).
//! - **Contexto e serviços.** As chaves de contexto `treeView.<id>.enableCollapseAll` e `toggleCollapseAll` são booleanos
//!   ([`CustomTreeView::collapse_all_toggled`]); a mensagem exibida é [`CustomTreeView::displayed_message`].
//!   `getOptimalWidth` devolve 0 (mede o DOM no original) e `layout` guarda as dimensões.
//! - **Arrastar e soltar.** `DragEvent`, `DataTransfer`, `toExternalVSDataTransfer` e `fillEditorsDragData` são o trait
//!   [`IDragDataTransfer`]; `ElementsDragAndDropData` é a struct de mesmo nome. `LocalSelectionTransfer` é um estado da thread.
//!   `onDragOver` devolve a reação sem os argumentos de alvo (o original não os lê).
//! - `IProgressService.withProgress` não devolve o resultado da tarefa: [`CustomTreeView::with_progress`] repassa a
//!   promessa da tarefa e o serviço só mostra o progresso.

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::rc::{Rc, Weak};

use je_base::async_::{async_fn, timeout};
use je_base::cancellation::{CancellationToken, CancellationTokenSource};
use je_base::data_transfer::{DataTransferItem, VSDataTransfer};
use je_base::dynamic::Dynamic;
use je_base::errors::Error;
use je_base::event::{Emitter, Event};
use je_base::lifecycle::{Disposable, DisposableRef, DisposableStore, IDisposable, MutableDisposable};
use je_base::mime::mimes;
use je_base::nls::localize;
use je_base::promise::Promise;
use je_base::runtime::set_timeout;
use je_base::uri::URI;
use je_base::uuid::generate_uuid;
use je_editor::languages::StringOrMarkdown;
use je_editor::tree_views_dnd::{DraggedTreeItemsIdentifier, ITreeViewsDnDService};
use je_platform::label::label::{ILabelService, UriLabelOptions};
use je_platform::log::{LogServiceRef, Logger};
use je_platform::progress::progress::{IProgressService, ProgressLocationId, ProgressOptions};
use je_platform::telemetry::telemetry::ITelemetryService;
use je_platform::telemetry::telemetry_utils::TelemetryTrustedValue;

use crate::common::tree_view_contracts::{ITreeView, ITreeViewDataProvider, ITreeViewDragAndDropController, TreeSelectionAndFocus};
use crate::common::views::{ITreeItem, ITreeItemLabel, IViewBadge, TreeItemCollapsibleState};
use crate::services::activity::activity::{IActivity, IActivityService, NumberBadge};
use crate::services::extensions::extensions::IExtensionService;

/// O handle do `Root`.
pub const ROOT_HANDLE: &str = "0";

/// `DataTransfers.RESOURCES`.
pub const DATA_TRANSFERS_RESOURCES: &str = "ResourceURLs";
/// `DataTransfers.FILES`.
pub const DATA_TRANSFERS_FILES: &str = "Files";
/// `CodeDataTransfers.FILES`.
pub const CODE_DATA_TRANSFERS_FILES: &str = "CodeFiles";

fn no_data_provider_message() -> String {
    localize("no-dataprovider", "There is no data provider registered that can provide view data.", &[])
}

/// `new Root()`.
fn root_item() -> ITreeItem {
    ITreeItem {
        handle: ROOT_HANDLE.to_owned(),
        label: Some(ITreeItemLabel { label: StringOrMarkdown::String("root".to_owned()), highlights: None }),
        collapsible_state: TreeItemCollapsibleState::Expanded,
        ..ITreeItem::default()
    }
}

fn handles_of(items: &[ITreeItem]) -> Vec<String> {
    items.iter().map(|item| item.handle.clone()).collect()
}

fn tree_element_not_found() -> Error {
    Error::new("Tree element not found")
}

// ---------------------------------------------------------------------------------------------------------------------
// O modelo de nós
// ---------------------------------------------------------------------------------------------------------------------

/// O modelo lógico da árvore (ver as divergências do módulo).
#[derive(Default)]
struct TreeNodes {
    /// O item de cada handle, com `children` e `parent` sempre `None`.
    items: HashMap<String, ITreeItem>,
    /// `node.children`: `None` é `undefined` (não carregado).
    children: HashMap<String, Option<Vec<String>>>,
    parents: HashMap<String, String>,
    expanded: HashSet<String>,
    selection: Vec<String>,
    focus: Vec<String>,
}

impl TreeNodes {
    fn new() -> Self {
        let mut nodes = Self::default();
        nodes.insert(&root_item(), None);
        nodes
    }

    fn contains(&self, handle: &str) -> bool {
        self.items.contains_key(handle)
    }

    fn item(&self, handle: &str) -> Option<ITreeItem> {
        self.items.get(handle).cloned()
    }

    fn items_of(&self, handles: &[String]) -> Vec<ITreeItem> {
        handles.iter().filter_map(|handle| self.item(handle)).collect()
    }

    /// O item com os filhos carregados (um nível), que é o que o provedor guarda.
    fn item_with_children(&self, handle: &str) -> Option<ITreeItem> {
        let mut item = self.item(handle)?;
        item.children = self.children.get(handle).cloned().flatten().map(|children| self.items_of(&children));
        Some(item)
    }

    fn children_loaded(&self, handle: &str) -> bool {
        matches!(self.children.get(handle), Some(Some(_)))
    }

    fn children_handles(&self, handle: &str) -> Vec<String> {
        self.children.get(handle).cloned().flatten().unwrap_or_default()
    }

    /// Guarda o item; um handle novo que nasce `Expanded` começa expandido (`collapseByDefault`).
    fn insert(&mut self, item: &ITreeItem, parent: Option<&str>) {
        let is_new = !self.items.contains_key(&item.handle);
        let mut stored = item.clone();
        stored.children = None;
        stored.parent = None;
        self.items.insert(item.handle.clone(), stored);
        if let Some(parent) = parent {
            self.parents.insert(item.handle.clone(), parent.to_owned());
        }
        if is_new && item.collapsible_state == TreeItemCollapsibleState::Expanded {
            self.expanded.insert(item.handle.clone());
        }
    }

    /// Um item que o modelo não conhece entra com os filhos que ele trouxer.
    fn adopt(&mut self, item: &ITreeItem) {
        if self.contains(&item.handle) {
            return;
        }
        self.insert(item, None);
        if let Some(children) = &item.children {
            self.set_children(&item.handle, Some(children.clone()));
        }
    }

    /// Troca os dados de um item que já existe (o objeto que o `$refresh` atualizou), ou o adota.
    fn replace(&mut self, item: &ITreeItem) {
        if self.contains(&item.handle) {
            let mut stored = item.clone();
            stored.children = None;
            stored.parent = None;
            self.items.insert(item.handle.clone(), stored);
        } else {
            self.adopt(item);
        }
    }

    /// `node.children = children`.
    fn set_children(&mut self, handle: &str, children: Option<Vec<ITreeItem>>) {
        match children {
            None => {
                self.children.insert(handle.to_owned(), None);
            }
            Some(list) => {
                for child in &list {
                    self.insert(child, Some(handle));
                }
                self.children.insert(handle.to_owned(), Some(handles_of(&list)));
            }
        }
    }

    fn reset_children(&mut self, handle: &str) {
        if self.contains(handle) {
            self.children.insert(handle.to_owned(), None);
        }
    }

    /// Descarta o que a raiz não alcança pelos filhos carregados (o que o `setChildren` da árvore remove).
    fn retain_reachable(&mut self) {
        let mut reachable: HashSet<String> = HashSet::new();
        let mut pending = vec![ROOT_HANDLE.to_owned()];
        while let Some(handle) = pending.pop() {
            if !reachable.insert(handle.clone()) {
                continue;
            }
            pending.extend(self.children_handles(&handle));
        }
        self.items.retain(|handle, _| reachable.contains(handle));
        self.children.retain(|handle, _| reachable.contains(handle));
        self.parents.retain(|handle, _| reachable.contains(handle));
        self.expanded.retain(|handle| reachable.contains(handle));
        self.selection.retain(|handle| reachable.contains(handle));
        self.focus.retain(|handle| reachable.contains(handle));
    }

    fn is_checked(&self, handle: &str) -> Option<bool> {
        self.items.get(handle).and_then(|item| item.checkbox.as_ref()).map(|checkbox| checkbox.is_checked)
    }

    fn set_checked(&mut self, handle: &str, checked: bool) {
        if let Some(checkbox) = self.items.get_mut(handle).and_then(|item| item.checkbox.as_mut()) {
            checkbox.is_checked = checked;
        }
    }

    /// `setCascadingCheckboxUpdates(items)`: `items.concat(additionalItems)`.
    fn cascade_checkboxes(&mut self, handles: &[String]) -> Vec<String> {
        let mut additional: Vec<String> = Vec::new();
        for handle in handles {
            if self.is_checked(handle).is_none() {
                continue;
            }
            self.check_children(handle, &mut additional);
            let mut visited_parents: HashSet<String> = HashSet::new();
            self.check_parents(handle, &mut visited_parents, &mut additional);
        }
        let mut all = handles.to_vec();
        all.extend(additional);
        all
    }

    fn check_children(&mut self, current: &str, additional: &mut Vec<String>) {
        for child in self.children_handles(current) {
            if let (Some(child_checked), Some(current_checked)) = (self.is_checked(&child), self.is_checked(current))
                && child_checked != current_checked
            {
                self.set_checked(&child, current_checked);
                additional.push(child.clone());
                self.check_children(&child, additional);
            }
        }
    }

    fn check_parents(&mut self, current: &str, visited_parents: &mut HashSet<String>, additional: &mut Vec<String>) {
        let Some(parent) = self.parents.get(current).cloned() else { return };
        if self.is_checked(&parent).is_none() || !self.children_loaded(&parent) {
            return;
        }
        if !visited_parents.insert(parent.clone()) {
            return;
        }
        let (mut some_unchecked, mut some_checked) = (false, false);
        for child in self.children_handles(&parent) {
            if some_unchecked && some_checked {
                break;
            }
            if let Some(checked) = self.is_checked(&child) {
                if checked {
                    some_checked = true;
                } else {
                    some_unchecked = true;
                }
            }
        }
        if some_checked && !some_unchecked && self.is_checked(&parent) != Some(true) {
            self.set_checked(&parent, true);
            additional.push(parent.clone());
            self.check_parents(&parent, visited_parents, additional);
        } else if some_unchecked && self.is_checked(&parent) != Some(false) {
            self.set_checked(&parent, false);
            additional.push(parent.clone());
            self.check_parents(&parent, visited_parents, additional);
        }
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// Arrastar e soltar
// ---------------------------------------------------------------------------------------------------------------------

/// `ElementsDragAndDropData` (`base/browser/ui/list/listView.ts`): os elementos arrastados.
pub struct ElementsDragAndDropData<T: Clone> {
    elements: Vec<T>,
}

impl<T: Clone> ElementsDragAndDropData<T> {
    pub fn new(elements: Vec<T>) -> Self {
        Self { elements }
    }

    /// `getData()`.
    pub fn get_data(&self) -> Vec<T> {
        self.elements.clone()
    }
}

/// Um item de `dataTransfer.items`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DragDataTransferItem {
    /// `kind`: `"file"` ou `"string"`.
    pub kind: String,
    /// `type`.
    pub type_: String,
}

/// O `DataTransfer` de um `DragEvent` (ver as divergências do módulo).
pub trait IDragDataTransfer {
    /// `clearData(format)`.
    fn clear_data(&self, format: &str);
    /// `setData(format, data)`.
    fn set_data(&self, format: &str, data: &str);
    /// `dropEffect`.
    fn drop_effect(&self) -> String;
    /// `items` (o `kind` e o `type` de cada um).
    fn items(&self) -> Vec<DragDataTransferItem>;
    /// `toExternalVSDataTransfer(dataTransfer, disableFileEntries)`.
    fn to_external_vs_data_transfer(&self, disable_file_entries: bool) -> VSDataTransfer;
    /// `fillEditorsDragData(accessor, resources, originalEvent)`.
    fn fill_editors_drag_data(&self, resources: &[URI]);
}

/// `DragEvent`: só o `dataTransfer`.
#[derive(Clone, Default)]
pub struct DragEventData {
    pub data_transfer: Option<Rc<dyn IDragDataTransfer>>,
}

/// `TreeDragOverBubble`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TreeDragOverBubble {
    Down,
    Up,
}

/// `boolean | ITreeDragOverReaction`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TreeDragOverReaction {
    Reject,
    Accept { bubble: TreeDragOverBubble, auto_expand: bool },
}

thread_local! {
    /// `LocalSelectionTransfer.getInstance<DraggedTreeItemsIdentifier>()`.
    static TREE_ITEMS_TRANSFER: RefCell<Option<Vec<DraggedTreeItemsIdentifier>>> = const { RefCell::new(None) };
}

fn tree_items_transfer_first() -> Option<DraggedTreeItemsIdentifier> {
    TREE_ITEMS_TRANSFER.with(|transfer| transfer.borrow().as_ref().and_then(|data| data.first().cloned()))
}

/// Os serviços do [`CustomTreeViewDragAndDrop`].
#[derive(Clone)]
pub struct TreeViewDragAndDropServices {
    pub label_service: Rc<dyn ILabelService>,
    pub tree_views_dnd_service: Rc<dyn ITreeViewsDnDService<Rc<VSDataTransfer>>>,
    pub log_service: LogServiceRef,
}

struct DragOperation {
    uuid: String,
    controller: Rc<dyn ITreeViewDragAndDropController>,
    cancellation_token: CancellationTokenSource,
}

/// `CustomTreeViewDragAndDrop`.
pub struct CustomTreeViewDragAndDrop {
    tree_id: String,
    tree_mime_type: String,
    services: TreeViewDragAndDropServices,
    dnd_controller: RefCell<Option<Rc<dyn ITreeViewDragAndDropController>>>,
    drag_operation: RefCell<Option<DragOperation>>,
}

impl CustomTreeViewDragAndDrop {
    pub fn new(tree_id: &str, services: TreeViewDragAndDropServices) -> Rc<Self> {
        Rc::new(Self {
            tree_id: tree_id.to_owned(),
            tree_mime_type: format!("application/vnd.code.tree.{}", tree_id.to_lowercase()),
            services,
            dnd_controller: RefCell::new(None),
            drag_operation: RefCell::new(None),
        })
    }

    /// `set controller`.
    pub fn set_controller(&self, controller: Option<Rc<dyn ITreeViewDragAndDropController>>) {
        self.cancel_drag_operation();
        *self.dnd_controller.borrow_mut() = controller;
    }

    fn cancel_drag_operation(&self) {
        let operation = self.drag_operation.borrow_mut().take();
        let Some(operation) = operation else { return };
        operation.cancellation_token.dispose_with(true);
        self.services.tree_views_dnd_service.remove_drag_operation_transfer(Some(&operation.uuid));
        operation.controller.handle_drag_end(&operation.uuid);
        if tree_items_transfer_first().is_some_and(|first| first.identifier == operation.uuid) {
            TREE_ITEMS_TRANSFER.with(|transfer| *transfer.borrow_mut() = None);
        }
    }

    fn handle_drag_and_log(
        &self,
        dnd_controller: &Rc<dyn ITreeViewDragAndDropController>,
        item_handles: Vec<String>,
        uuid: &str,
        drag_cancellation_token: CancellationToken,
    ) -> Promise<Option<Rc<VSDataTransfer>>> {
        let (controller, tree_mime_type, tree_id, log_service) =
            (dnd_controller.clone(), self.tree_mime_type.clone(), self.tree_id.clone(), self.services.log_service.clone());
        dnd_controller.handle_drag(item_handles, uuid, drag_cancellation_token).then(move |additional_data_transfer| {
            if let Some(additional) = &additional_data_transfer {
                let drag_mime_types = controller.drag_mime_types();
                let unlisted_types: Vec<String> =
                    additional.iter().filter(|(type_, _)| *type_ != tree_mime_type && !drag_mime_types.iter().any(|value| value == type_)).map(|(type_, _)| type_.to_owned()).collect();
                if !unlisted_types.is_empty() {
                    log_service.warn(&format!(
                        "Drag and drop controller for tree {tree_id} adds the following data transfer types but does not declare them in dragMimeTypes: {}",
                        unlisted_types.join(", ")
                    ));
                }
            }
            additional_data_transfer
        })
    }

    fn add_extension_provided_transfer_types(&self, data_transfer: &Rc<dyn IDragDataTransfer>, item_handles: Vec<String>) {
        let Some(controller) = self.dnd_controller.borrow().clone() else { return };
        let uuid = generate_uuid();

        self.cancel_drag_operation();
        let cancellation_token = CancellationTokenSource::new(None);
        *self.drag_operation.borrow_mut() = Some(DragOperation { uuid: uuid.clone(), controller: controller.clone(), cancellation_token: cancellation_token.clone() });
        let transfer = self.handle_drag_and_log(&controller, item_handles, &uuid, cancellation_token.token());
        self.services.tree_views_dnd_service.add_drag_operation_transfer(&uuid, transfer);
        TREE_ITEMS_TRANSFER.with(|transfer| *transfer.borrow_mut() = Some(vec![DraggedTreeItemsIdentifier::new(uuid.clone())]));
        data_transfer.clear_data(mimes::TEXT);
        let drag_mime_types = controller.drag_mime_types();
        if drag_mime_types.iter().any(|element| element == mimes::URI_LIST) {
            // Add the type that the editor knows
            data_transfer.set_data(DATA_TRANSFERS_RESOURCES, "");
        }
        for supported_type in &drag_mime_types {
            data_transfer.set_data(supported_type, "");
        }
    }

    fn add_resource_info_to_transfer(&self, data_transfer: &Rc<dyn IDragDataTransfer>, resources: &[URI]) {
        if resources.is_empty() {
            return;
        }
        // Apply some datatransfer types to allow for dragging the element outside of the application
        data_transfer.fill_editors_drag_data(resources);

        // The only custom data transfer we set from the explorer is a file transfer
        // to be able to DND between multiple code file explorers across windows
        let file_resources: Vec<String> = resources.iter().filter(|resource| resource.scheme().to_string_lossy() == "file").map(|resource| je_platform::resources::fs_path(resource)).collect();
        if !file_resources.is_empty() {
            data_transfer.set_data(CODE_DATA_TRANSFERS_FILES, &serde_json::json!(file_resources).to_string());
        }
    }

    /// `onDragStart(data, originalEvent)`.
    pub fn on_drag_start(&self, data: &ElementsDragAndDropData<ITreeItem>, original_event: &DragEventData) {
        let Some(data_transfer) = &original_event.data_transfer else { return };
        let mut resources: Vec<URI> = Vec::new();
        let mut item_handles: Vec<String> = Vec::new();
        for item in data.get_data() {
            item_handles.push(item.handle.clone());
            if let Some(resource_uri) = &item.resource_uri {
                resources.push(resource_uri.clone());
            }
        }
        self.add_resource_info_to_transfer(data_transfer, &resources);
        self.add_extension_provided_transfer_types(data_transfer, item_handles.clone());
        data_transfer.set_data(&self.tree_mime_type, &serde_json::json!({ "id": self.tree_id, "itemHandles": item_handles }).to_string());
    }

    /// `onDragOver(data, targetElement, targetIndex, targetSector, originalEvent)`.
    pub fn on_drag_over(&self, original_event: &DragEventData) -> TreeDragOverReaction {
        let mut types: Vec<String> = Vec::new();
        let mut add_type = |type_: &str| {
            if !types.iter().any(|existing| existing == type_) {
                types.push(type_.to_owned());
            }
        };
        if let Some(data_transfer) = &original_event.data_transfer {
            for (type_, _) in data_transfer.to_external_vs_data_transfer(false).iter() {
                add_type(type_);
            }
            // Also add uri-list if we have any files. At this stage we can't actually access the file itself though.
            if data_transfer.items().iter().any(|item| item.kind == "file" || item.type_ == DATA_TRANSFERS_RESOURCES.to_lowercase()) {
                add_type(mimes::URI_LIST);
            }
        }

        if types.is_empty() {
            self.services.log_service.debug("TreeView dragged with no supported mime types.");
        } else {
            self.services.log_service.debug(&format!("TreeView dragged mime types: {}", types.join(", ")));
        }

        let dnd_controller = self.dnd_controller.borrow().clone();
        let Some(dnd_controller) = dnd_controller else { return TreeDragOverReaction::Reject };
        let drop_mime_types = dnd_controller.drop_mime_types();
        if original_event.data_transfer.is_none() || drop_mime_types.is_empty() {
            return TreeDragOverReaction::Reject;
        }
        let drag_containers_supported_type = types.iter().any(|value| *value == self.tree_mime_type || drop_mime_types.contains(value));
        if drag_containers_supported_type {
            return TreeDragOverReaction::Accept { bubble: TreeDragOverBubble::Down, auto_expand: true };
        }
        TreeDragOverReaction::Reject
    }

    /// `getDragURI(element)`.
    pub fn get_drag_uri(&self, element: &ITreeItem) -> Option<String> {
        self.dnd_controller.borrow().as_ref()?;
        Some(element.resource_uri.as_ref().map_or_else(|| element.handle.clone(), |uri| uri.to_string(false).to_string_lossy()))
    }

    /// `getDragLabel(elements)`.
    pub fn get_drag_label(&self, elements: &[ITreeItem]) -> Option<String> {
        self.dnd_controller.borrow().as_ref()?;
        if elements.len() > 1 {
            return Some(elements.len().to_string());
        }
        let element = elements.first()?;
        if let Some(label) = &element.label {
            return Some(match &label.label {
                StringOrMarkdown::String(text) => text.clone(),
                StringOrMarkdown::Markdown(markdown) => markdown.value.to_string_lossy(),
            });
        }
        element.resource_uri.as_ref().map(|uri| self.services.label_service.get_uri_label(uri, &UriLabelOptions::default()))
    }

    /// `drop(data, targetNode, targetIndex, targetSector, originalEvent)`.
    pub fn drop(self: &Rc<Self>, target_node: Option<ITreeItem>, original_event: &DragEventData) -> Promise<()> {
        let this = Rc::clone(self);
        let data_transfer = original_event.data_transfer.clone();
        async_fn(async move {
            let dnd_controller = this.dnd_controller.borrow().clone();
            let (Some(data_transfer), Some(dnd_controller)) = (data_transfer, dnd_controller) else { return Ok(()) };

            let will_drop_uuid = tree_items_transfer_first().map(|first| first.identifier);

            let original_data_transfer = data_transfer.to_external_vs_data_transfer(true);
            let drop_mime_types = dnd_controller.drop_mime_types();
            let mut out_data_transfer = VSDataTransfer::new();
            let mut tree_source_info: Option<(String, Vec<String>)> = None;
            let entries: Vec<(String, DataTransferItem)> = original_data_transfer.iter().map(|(type_, item)| (type_.to_owned(), item.clone())).collect();
            for (type_, item) in entries {
                if type_ == this.tree_mime_type || drop_mime_types.contains(&type_) || (item.as_file().is_some() && drop_mime_types.iter().any(|mime| mime == &DATA_TRANSFERS_FILES.to_lowercase())) {
                    out_data_transfer.append(&type_, item.clone());
                    if type_ == this.tree_mime_type
                        && let Ok(text) = item.as_string().await
                        && let Ok(info) = serde_json::from_str::<serde_json::Value>(&text)
                    {
                        let id = info.get("id").and_then(|id| id.as_str()).map(str::to_owned);
                        let handles = info.get("itemHandles").and_then(|handles| handles.as_array()).map(|handles| handles.iter().filter_map(|handle| handle.as_str().map(str::to_owned)).collect::<Vec<_>>());
                        if let (Some(id), Some(handles)) = (id, handles) {
                            tree_source_info = Some((id, handles));
                        }
                    }
                }
            }

            let additional = this.services.tree_views_dnd_service.remove_drag_operation_transfer(will_drop_uuid.as_deref());
            let additional_data_transfer = match additional {
                Some(promise) => promise.await?,
                None => None,
            };
            if let Some(additional) = additional_data_transfer {
                for (type_, item) in additional.iter() {
                    out_data_transfer.append(type_, item.clone());
                }
            }
            let (source_id, source_handles) = match tree_source_info {
                Some((id, handles)) => (Some(id), Some(handles)),
                None => (None, None),
            };
            dnd_controller.handle_drop(Rc::new(out_data_transfer), target_node, CancellationToken::None, will_drop_uuid.as_deref(), source_id.as_deref(), source_handles).await
        })
    }

    /// `onDragEnd(originalEvent)`.
    pub fn on_drag_end(&self, original_event: &DragEventData) {
        // Check if the drag was cancelled.
        if original_event.data_transfer.as_ref().is_some_and(|data_transfer| data_transfer.drop_effect() == "none") {
            self.cancel_drag_operation();
        } else {
            // A successful drop may still be awaiting its additional transfer data.
            let operation = self.drag_operation.borrow_mut().take();
            if let Some(operation) = operation {
                operation.cancellation_token.dispose();
            }
        }
    }
}

impl IDisposable for CustomTreeViewDragAndDrop {
    fn dispose(&self) {
        self.cancel_drag_operation();
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// O provedor de dados da árvore (a classe anônima do setter `dataProvider`)
// ---------------------------------------------------------------------------------------------------------------------

/// `doGetChildrenOrBatch(dataProvider, nodes)`.
fn do_get_children_or_batch(data_provider: &Rc<dyn ITreeViewDataProvider>, nodes: Option<Vec<ITreeItem>>) -> Promise<Option<Vec<Vec<ITreeItem>>>> {
    if let Some(batch) = data_provider.get_children_batch(nodes.as_deref()) {
        return batch;
    }
    match nodes {
        Some(nodes) => {
            let promises: Vec<Promise<Vec<ITreeItem>>> = nodes.iter().map(|node| data_provider.get_children(Some(node)).then(|children| children.unwrap_or_default())).collect();
            Promise::all(promises).then(Some)
        }
        None => data_provider.get_children(None).then(|children| Some(children.into_iter().collect())),
    }
}

/// O provedor que a árvore guarda: embrulha o provedor da extensão, mantém o modelo de nós e o estado de vazio.
#[derive(Clone)]
struct ViewDataProvider {
    view: Weak<CustomTreeView>,
    inner: Rc<dyn ITreeViewDataProvider>,
    is_empty: Rc<Cell<bool>>,
    on_did_change_empty: Rc<Emitter<()>>,
}

impl ViewDataProvider {
    fn update_empty_state(&self, node_handles: &[String], group_handles: &[Vec<String>]) {
        if node_handles.len() == 1 && node_handles[0] == ROOT_HANDLE {
            let old_empty = self.is_empty.get();
            self.is_empty.set(group_handles.is_empty() || group_handles[0].is_empty());
            if old_empty != self.is_empty.get() {
                self.on_did_change_empty.fire(());
            }
        }
    }

    /// `findCheckboxesUpdated(nodes, childrenGroups)`.
    fn find_checkboxes_updated(view: &CustomTreeView, node_handles: &[String], group_handles: &[Vec<String>]) -> Result<Vec<String>, Error> {
        if group_handles.is_empty() {
            return Ok(Vec::new());
        }
        let mut checkboxes_updated = Vec::new();
        let mut nodes = view.nodes.borrow_mut();
        for (index, node) in node_handles.iter().enumerate() {
            let Some(children) = group_handles.get(index) else { return Err(Error::new("children is not iterable")) };
            for child in children {
                nodes.parents.insert(child.clone(), node.clone());
                if !view.manually_manage_checkboxes.get() && nodes.is_checked(node) == Some(true) && nodes.is_checked(child) == Some(false) {
                    nodes.set_checked(child, true);
                    checkboxes_updated.push(child.clone());
                }
            }
        }
        Ok(checkboxes_updated)
    }

    /// `getChildrenBatch(nodes?)`.
    fn get_children_batch_impl(&self, nodes: Option<Vec<ITreeItem>>) -> Promise<Vec<Vec<ITreeItem>>> {
        let this = self.clone();
        async_fn(async move {
            let Some(view) = this.view.upgrade() else { return Ok(Vec::new()) };
            if let Some(list) = &nodes {
                for node in list {
                    view.nodes.borrow_mut().adopt(node);
                }
            }
            let all_loaded = nodes.as_ref().is_some_and(|list| list.iter().all(|node| view.nodes.borrow().children_loaded(&node.handle)));
            let node_handles: Vec<String>;
            let group_handles: Vec<Vec<String>>;
            let mut checkboxes_updated: Vec<String> = Vec::new();
            if all_loaded {
                node_handles = handles_of(nodes.as_deref().unwrap_or_default());
                group_handles = node_handles.iter().map(|handle| view.nodes.borrow().children_handles(handle)).collect();
            } else {
                node_handles = nodes.as_deref().map(handles_of).unwrap_or_else(|| vec![ROOT_HANDLE.to_owned()]);
                let node_items = view.nodes.borrow().items_of(&node_handles);
                let is_root = node_handles.len() == 1 && node_handles[0] == ROOT_HANDLE;
                let batched_children = do_get_children_or_batch(&this.inner, if is_root { None } else { Some(node_items) }).await?;
                {
                    let mut store = view.nodes.borrow_mut();
                    for (index, handle) in node_handles.iter().enumerate() {
                        store.set_children(handle, batched_children.as_ref().and_then(|batches| batches.get(index).cloned()));
                    }
                }
                group_handles = batched_children.map(|batches| batches.iter().map(|group| handles_of(group)).collect()).unwrap_or_default();
                checkboxes_updated = Self::find_checkboxes_updated(&view, &node_handles, &group_handles)?;

                let mut changed = node_handles.clone();
                changed.extend(checkboxes_updated.iter().filter(|handle| !node_handles.contains(handle)).cloned());
                let snapshots: Vec<ITreeItem> = changed.iter().filter_map(|handle| view.nodes.borrow().item_with_children(handle)).collect();
                this.inner.did_update_items(&snapshots);
            }

            this.update_empty_state(&node_handles, &group_handles);

            if !checkboxes_updated.is_empty() {
                let snapshots = view.nodes.borrow().items_of(&checkboxes_updated);
                view.on_did_change_checkbox_state.fire(snapshots);
            }
            let store = view.nodes.borrow();
            Ok(group_handles.iter().map(|group| store.items_of(group)).collect())
        })
    }
}

impl ITreeViewDataProvider for ViewDataProvider {
    fn is_tree_empty(&self) -> Option<bool> {
        Some(self.is_empty.get())
    }

    fn on_did_change_empty(&self) -> Event<()> {
        self.on_did_change_empty.event()
    }

    fn get_children(&self, element: Option<&ITreeItem>) -> Promise<Option<Vec<ITreeItem>>> {
        self.get_children_batch_impl(element.map(|element| vec![element.clone()])).then(|batches| batches.into_iter().next())
    }

    fn get_children_batch(&self, elements: Option<&[ITreeItem]>) -> Option<Promise<Option<Vec<Vec<ITreeItem>>>>> {
        Some(self.get_children_batch_impl(elements.map(<[ITreeItem]>::to_vec)).then(Some))
    }

    fn did_update_items(&self, items: &[ITreeItem]) {
        self.inner.did_update_items(items);
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// CustomTreeView
// ---------------------------------------------------------------------------------------------------------------------

/// Os serviços do [`CustomTreeView`] que a parte lógica usa.
#[derive(Clone)]
pub struct CustomTreeViewServices {
    pub log_service: LogServiceRef,
    pub telemetry_service: Rc<dyn ITelemetryService>,
    pub extension_service: Rc<dyn IExtensionService>,
    pub progress_service: Rc<dyn IProgressService>,
    pub activity_service: Rc<dyn IActivityService>,
    pub label_service: Rc<dyn ILabelService>,
    pub tree_views_dnd_service: Rc<dyn ITreeViewsDnDService<Rc<VSDataTransfer>>>,
}

/// `CustomTreeView extends AbstractTreeView`.
pub struct CustomTreeView {
    base: Rc<Disposable>,
    this: Weak<CustomTreeView>,
    id: String,
    /// `None` na variante `TreeView` (sem extensão).
    extension_id: Option<String>,
    services: CustomTreeViewServices,

    is_visible: Cell<bool>,
    is_initialized: Cell<bool>,
    activated: Cell<bool>,
    refreshing: Cell<bool>,
    can_select_many: Cell<bool>,
    manually_manage_checkboxes: Cell<bool>,
    show_collapse_all: Cell<bool>,
    collapse_all_toggle: Cell<bool>,
    height: Cell<f64>,
    width: Cell<f64>,

    title: RefCell<String>,
    description: RefCell<Option<String>>,
    message: RefCell<Option<StringOrMarkdown>>,
    displayed_message: RefCell<Option<StringOrMarkdown>>,
    badge: RefCell<Option<IViewBadge>>,
    activity: Rc<MutableDisposable<dyn IDisposable>>,

    data_provider: RefCell<Option<Rc<dyn ITreeViewDataProvider>>>,
    drag_and_drop_controller: RefCell<Option<Rc<dyn ITreeViewDragAndDropController>>>,
    tree_view_dnd: RefCell<Option<Rc<CustomTreeViewDragAndDrop>>>,
    tree_disposables: Rc<DisposableStore>,

    /// `this.tree !== undefined`: a árvore (o modelo [`TreeNodes`]) foi criada.
    tree_created: Cell<bool>,
    nodes: RefCell<TreeNodes>,
    elements_to_refresh: RefCell<Vec<String>>,
    last_selection: RefCell<Vec<ITreeItem>>,
    last_active: RefCell<ITreeItem>,

    /// O estado do `TreeDataSource`: o lote de elementos pendentes e a promessa dele.
    batch: RefCell<Option<Vec<ITreeItem>>>,
    batch_promise: RefCell<Option<Promise<Option<Vec<Vec<ITreeItem>>>>>>,

    /// `subTreeRefreshPromises` do `AsyncDataTree`: as recargas em andamento (identificador, nó, promessa).
    sub_tree_refreshes: RefCell<Vec<(u64, String, Promise<()>)>>,
    next_refresh_id: Cell<u64>,

    on_did_expand_item: Rc<Emitter<ITreeItem>>,
    on_did_collapse_item: Rc<Emitter<ITreeItem>>,
    on_did_change_selection_and_focus: Rc<Emitter<TreeSelectionAndFocus>>,
    on_did_change_visibility: Rc<Emitter<bool>>,
    on_did_change_actions: Rc<Emitter<()>>,
    on_did_change_welcome_state: Rc<Emitter<()>>,
    on_did_change_title: Rc<Emitter<String>>,
    on_did_change_description: Rc<Emitter<Option<String>>>,
    on_did_change_checkbox_state: Rc<Emitter<Vec<ITreeItem>>>,
    on_did_complete_refresh: Rc<Emitter<()>>,
}

impl CustomTreeView {
    /// `new CustomTreeView(id, title, extensionId, ...services)`.
    pub fn new(id: &str, title: &str, extension_id: &str, services: CustomTreeViewServices) -> Rc<Self> {
        Self::build(id, title, Some(extension_id.to_owned()), services)
    }

    /// `new TreeView(id, title, ...services)`: a variante sem extensão, que ao ativar só cria a árvore (sem telemetria, sem
    /// progresso e sem `activateByEvent`). Os serviços de extensão e telemetria do `services` não são usados.
    pub fn new_without_extension(id: &str, title: &str, services: CustomTreeViewServices) -> Rc<Self> {
        Self::build(id, title, None, services)
    }

    fn build(id: &str, title: &str, extension_id: Option<String>, services: CustomTreeViewServices) -> Rc<Self> {
        let base = Disposable::new();
        fn emitter<T: Clone + 'static>(base: &Rc<Disposable>) -> Rc<Emitter<T>> {
            let emitter = Emitter::new();
            base.register(emitter.clone());
            emitter
        }
        let tree_disposables = DisposableStore::new();
        base.register(tree_disposables.clone());
        let activity: Rc<MutableDisposable<dyn IDisposable>> = MutableDisposable::new();
        base.register(activity.clone());
        Rc::new_cyclic(|this| Self {
            this: this.clone(),
            id: id.to_owned(),
            extension_id,
            services,
            is_visible: Cell::new(false),
            is_initialized: Cell::new(false),
            activated: Cell::new(false),
            refreshing: Cell::new(false),
            can_select_many: Cell::new(false),
            manually_manage_checkboxes: Cell::new(false),
            show_collapse_all: Cell::new(false),
            collapse_all_toggle: Cell::new(false),
            height: Cell::new(0.0),
            width: Cell::new(0.0),
            title: RefCell::new(title.to_owned()),
            description: RefCell::new(None),
            message: RefCell::new(None),
            displayed_message: RefCell::new(None),
            badge: RefCell::new(None),
            activity,
            data_provider: RefCell::new(None),
            drag_and_drop_controller: RefCell::new(None),
            tree_view_dnd: RefCell::new(None),
            tree_disposables,
            tree_created: Cell::new(false),
            nodes: RefCell::new(TreeNodes::new()),
            elements_to_refresh: RefCell::new(Vec::new()),
            last_selection: RefCell::new(Vec::new()),
            last_active: RefCell::new(root_item()),
            batch: RefCell::new(None),
            batch_promise: RefCell::new(None),
            sub_tree_refreshes: RefCell::new(Vec::new()),
            next_refresh_id: Cell::new(0),
            on_did_expand_item: emitter(&base),
            on_did_collapse_item: emitter(&base),
            on_did_change_selection_and_focus: emitter(&base),
            on_did_change_visibility: emitter(&base),
            on_did_change_actions: emitter(&base),
            on_did_change_welcome_state: emitter(&base),
            on_did_change_title: emitter(&base),
            on_did_change_description: emitter(&base),
            on_did_change_checkbox_state: emitter(&base),
            on_did_complete_refresh: emitter(&base),
            base,
        })
    }

    fn rc(&self) -> Rc<Self> {
        self.this.upgrade().expect("a árvore está viva enquanto um método dela roda")
    }

    /// `id`.
    pub fn id(&self) -> &str {
        &self.id
    }

    /// `treeViewDnd` (privado no original; a suíte o lê por `Reflect.get`): existe depois da primeira `setVisibility`.
    pub fn tree_view_dnd(&self) -> Option<Rc<CustomTreeViewDragAndDrop>> {
        self.tree_view_dnd.borrow().clone()
    }

    /// A mensagem que a parte mostraria (`updateMessage`): a da extensão, ou o aviso de que não há provedor.
    pub fn displayed_message(&self) -> Option<StringOrMarkdown> {
        self.displayed_message.borrow().clone()
    }

    /// A chave de contexto `treeView.<id>.toggleCollapseAll`.
    pub fn collapse_all_toggled(&self) -> bool {
        self.collapse_all_toggle.get()
    }

    /// As dimensões da última chamada de `layout`.
    pub fn dimensions(&self) -> (f64, f64) {
        (self.height.get(), self.width.get())
    }

    fn has_children(&self, handle: &str) -> bool {
        self.data_provider.borrow().is_some() && self.nodes.borrow().items.get(handle).is_some_and(|item| item.collapsible_state != TreeItemCollapsibleState::None)
    }

    fn update_message(&self) {
        let message = self.message.borrow().clone();
        *self.displayed_message.borrow_mut() = match message {
            Some(message) => Some(message),
            None if self.data_provider.borrow().is_none() => Some(StringOrMarkdown::String(no_data_provider_message())),
            None => None,
        };
    }

    fn update_collapse_all_toggle(&self) {
        if self.show_collapse_all.get() {
            let nodes = self.nodes.borrow();
            let root_children = nodes.children_handles(ROOT_HANDLE);
            self.collapse_all_toggle.set(root_children.iter().any(|handle| nodes.items.get(handle).is_some_and(|item| item.collapsible_state != TreeItemCollapsibleState::None)));
        }
    }

    /// `initialize()`: roda na primeira `setVisibility`.
    fn initialize(&self) {
        if self.is_initialized.replace(true) {
            return;
        }
        let dnd = CustomTreeViewDragAndDrop::new(
            &self.id,
            TreeViewDragAndDropServices {
                label_service: self.services.label_service.clone(),
                tree_views_dnd_service: self.services.tree_views_dnd_service.clone(),
                log_service: self.services.log_service.clone(),
            },
        );
        self.base.register(dnd.clone());
        let controller = self.drag_and_drop_controller.borrow().clone();
        if controller.is_some() {
            dnd.set_controller(controller);
        }
        *self.tree_view_dnd.borrow_mut() = Some(dnd);
    }

    /// `activate()` do `CustomTreeView`.
    fn activate(&self) {
        if self.activated.get() {
            return;
        }
        let Some(extension_id) = &self.extension_id else {
            self.create_tree();
            self.activated.set(true);
            return;
        };
        self.services.telemetry_service.public_log2(
            "Extension:ViewActivate",
            Some(Dynamic::object([
                ("extensionId", TelemetryTrustedValue { value: Dynamic::string(extension_id) }.to_dynamic()),
                ("id", Dynamic::string(&self.id)),
            ])),
        );
        self.create_tree();
        let extension_service = self.services.extension_service.clone();
        let event = format!("onView:{}", self.id);
        let weak = self.this.clone();
        let activation = self.services.progress_service.with_progress(
            ProgressOptions::new(ProgressLocationId::View(self.id.clone())),
            Box::new(move |_| extension_service.activate_by_event(&event, None)),
            None,
        );
        let _ = activation.then_promise(|_| (*timeout(2000)).clone()).then(move |_| {
            if let Some(view) = weak.upgrade() {
                view.update_message();
            }
        });
        self.activated.set(true);
    }

    /// `createTree()`: cria o modelo e carrega a raiz (`tree.setInput(root)`).
    fn create_tree(&self) {
        self.tree_disposables.clear();
        *self.batch.borrow_mut() = None;
        *self.batch_promise.borrow_mut() = None;
        self.tree_created.set(true);
        let _ = self.rc().update_children(ROOT_HANDLE.to_owned());
    }

    /// `withProgress({ location: this.id }, () => task)`.
    fn with_progress<T: Clone + 'static>(&self, task: Promise<T>) -> Promise<T> {
        let inner = task.clone();
        let progress = self.services.progress_service.with_progress(
            ProgressOptions::new(ProgressLocationId::View(self.id.clone())),
            Box::new(move |_| inner.then(|_| ())),
            None,
        );
        progress.then_result_promise(move |result| match result {
            Ok(()) => task,
            Err(error) => Promise::rejected(error),
        })
    }

    /// `TreeDataSource.getChildren(element)`: junta os pedidos do mesmo turno num lote.
    fn data_source_get_children(self: &Rc<Self>, handle: &str) -> Promise<Vec<ITreeItem>> {
        let Some(data_provider) = self.data_provider.borrow().clone() else { return Promise::resolved(Vec::new()) };
        let Some(element) = self.nodes.borrow().item(handle) else { return Promise::resolved(Vec::new()) };
        let index_in_batch = {
            let mut batch = self.batch.borrow_mut();
            match batch.as_mut() {
                None => {
                    *batch = Some(vec![element]);
                    *self.batch_promise.borrow_mut() = None;
                }
                Some(batch) => batch.push(element),
            }
            batch.as_ref().map_or(0, |batch| batch.len() - 1)
        };
        let (promise, resolver) = Promise::new();
        let this = Rc::clone(self);
        set_timeout(
            move || {
                let batch = this.batch.borrow_mut().take();
                if this.batch_promise.borrow().is_none() {
                    let task = this.with_progress(do_get_children_or_batch(&data_provider, batch));
                    *this.batch_promise.borrow_mut() = Some(task);
                }
                let batch_promise = this.batch_promise.borrow().clone().expect("a promessa do lote foi criada acima");
                let _ = batch_promise.then_result(move |result| {
                    match result {
                        Ok(result) => resolver.resolve(result.and_then(|result| result.get(index_in_batch).cloned()).unwrap_or_default()),
                        Err(error) => {
                            if !error.message.starts_with("Bad progress location:") {
                                resolver.reject(error);
                            }
                        }
                    }
                    Ok(())
                });
            },
            0,
        );
        promise
    }

    /// `tree.updateChildren(element, true, true)`: recarrega a raiz ou o nó expandido, e os expandidos abaixo dele.
    ///
    /// Como o `AsyncDataTree.refreshNode`: uma recarga que cruza (mesmo nó, ancestral ou descendente) com outra em andamento
    /// espera por ela e só então começa. Sem isso, as recargas simultâneas da raiz entrariam no mesmo lote do `TreeDataSource`.
    fn update_children(self: &Rc<Self>, handle: String) -> Promise<()> {
        let pending = {
            let nodes = self.nodes.borrow();
            let intersects = |ancestor: &str, descendant: &str| {
                let mut current = descendant;
                loop {
                    if current == ancestor {
                        return true;
                    }
                    match nodes.parents.get(current) {
                        Some(parent) => current = parent,
                        None => return false,
                    }
                }
            };
            self.sub_tree_refreshes.borrow().iter().find(|(_, other, _)| intersects(other, &handle) || intersects(&handle, other)).map(|(_, _, promise)| promise.clone())
        };
        if let Some(previous) = pending {
            let this = Rc::clone(self);
            return previous.then_result_promise(move |result| match result {
                Ok(()) => this.update_children(handle),
                Err(error) => Promise::rejected(error),
            });
        }
        let refresh_id = self.next_refresh_id.get();
        self.next_refresh_id.set(refresh_id + 1);
        let promise = self.do_update_children(handle.clone());
        self.sub_tree_refreshes.borrow_mut().push((refresh_id, handle, promise.clone()));
        let this = Rc::clone(self);
        let _ = promise.then_result(move |_| {
            this.sub_tree_refreshes.borrow_mut().retain(|(id, _, _)| *id != refresh_id);
            Ok(())
        });
        promise
    }

    fn do_update_children(self: &Rc<Self>, handle: String) -> Promise<()> {
        let this = Rc::clone(self);
        async_fn(async move {
            if !this.nodes.borrow().contains(&handle) {
                return Err(tree_element_not_found());
            }
            if handle != ROOT_HANDLE && !this.nodes.borrow().expanded.contains(&handle) {
                // Um nó recolhido não carrega: fica para quando expandir.
                this.nodes.borrow_mut().reset_children(&handle);
                return Ok(());
            }
            let mut level = vec![handle];
            while !level.is_empty() {
                let promises: Vec<Promise<Vec<ITreeItem>>> = level.iter().map(|handle| this.data_source_get_children(handle)).collect();
                let groups = Promise::all(promises).await?;
                let mut next = Vec::new();
                for children in groups {
                    for child in children {
                        if this.nodes.borrow().expanded.contains(&child.handle) && this.has_children(&child.handle) {
                            next.push(child.handle);
                        }
                    }
                }
                level = next;
            }
            this.nodes.borrow_mut().retain_reachable();
            Ok(())
        })
    }

    /// `doRefresh(elements)`.
    fn do_refresh(self: &Rc<Self>, handles: Vec<String>) -> Promise<()> {
        let this = Rc::clone(self);
        async_fn(async move {
            if this.tree_created.get() && this.is_visible.get() {
                this.refreshing.set(true);
                let old_selection = this.nodes.borrow().selection.clone();
                let promises: Vec<Promise<()>> = handles.iter().map(|handle| this.update_children(handle.clone())).collect();
                if let Err(error) = Promise::all(promises).await {
                    this.services.log_service.error(&error.message);
                }
                let new_selection = this.nodes.borrow().selection.clone();
                if old_selection != new_selection {
                    let selection = this.nodes.borrow().items_of(&new_selection);
                    *this.last_selection.borrow_mut() = selection.clone();
                    this.on_did_change_selection_and_focus.fire(TreeSelectionAndFocus { selection, focus: this.last_active.borrow().clone() });
                }
                this.refreshing.set(false);
                this.on_did_complete_refresh.fire(());
                this.update_collapse_all_toggle();
            }
            Ok(())
        })
    }

    /// `tree.reveal(element)`: expande os ancestrais, do mais próximo para cima (`expandTo`).
    fn reveal_handle(&self, handle: &str) {
        let mut ancestors = Vec::new();
        {
            let nodes = self.nodes.borrow();
            let mut current = handle.to_owned();
            while let Some(parent) = nodes.parents.get(&current) {
                if parent == ROOT_HANDLE {
                    break;
                }
                ancestors.push(parent.clone());
                current = parent.clone();
            }
        }
        for ancestor in ancestors {
            let changed = self.nodes.borrow_mut().expanded.insert(ancestor.clone());
            if changed && let Some(item) = self.nodes.borrow().item(&ancestor) {
                self.on_did_expand_item.fire(item);
            }
        }
    }

    /// `focus(reveal, revealItem)`: só a parte lógica (revelar o item); o foco do DOM é da interface.
    fn focus_item(&self, reveal: bool, reveal_item: Option<&ITreeItem>) {
        let has_root_children = !self.nodes.borrow().children_handles(ROOT_HANDLE).is_empty();
        if self.tree_created.get() && has_root_children {
            // Make sure the current selected element is revealed
            let element = reveal_item.map(|item| item.handle.clone()).or_else(|| self.nodes.borrow().selection.first().cloned());
            if let Some(element) = element
                && reveal
            {
                self.reveal_handle(&element);
            }
        }
    }

    /// `CollapseAllAction(tree, true).run()`: recolhe todos os nós.
    pub fn collapse_all(&self) {
        let collapsed: Vec<String> = {
            let mut nodes = self.nodes.borrow_mut();
            let handles: Vec<String> = nodes.expanded.iter().filter(|handle| handle.as_str() != ROOT_HANDLE).cloned().collect();
            nodes.expanded.retain(|handle| handle == ROOT_HANDLE);
            handles
        };
        for handle in collapsed {
            if let Some(item) = self.nodes.borrow().item(&handle) {
                self.on_did_collapse_item.fire(item);
            }
        }
    }
}

impl IDisposable for CustomTreeView {
    fn dispose(&self) {
        self.base.dispose();
    }
}

impl ITreeView for CustomTreeView {
    fn data_provider(&self) -> Option<Rc<dyn ITreeViewDataProvider>> {
        self.data_provider.borrow().clone()
    }

    fn set_data_provider(&self, data_provider: Option<Rc<dyn ITreeViewDataProvider>>) {
        if let Some(inner) = data_provider {
            if self.is_visible.get() {
                self.activate();
            }
            let on_did_change_empty = Emitter::new();
            self.base.register(on_did_change_empty.clone());
            let wrapper = ViewDataProvider { view: self.this.clone(), inner, is_empty: Rc::new(Cell::new(true)), on_did_change_empty: on_did_change_empty.clone() };
            *self.data_provider.borrow_mut() = Some(Rc::new(wrapper));
            let weak = self.this.clone();
            self.base.register(on_did_change_empty.event().listen(move |_| {
                if let Some(view) = weak.upgrade() {
                    view.update_collapse_all_toggle();
                    view.on_did_change_welcome_state.fire(());
                }
            }));
            self.update_message();
            let _ = self.refresh(None, None);
        } else {
            *self.data_provider.borrow_mut() = None;
            self.tree_disposables.clear();
            self.tree_created.set(false);
            self.activated.set(false);
            self.update_message();
        }

        self.on_did_change_welcome_state.fire(());
    }

    fn drag_and_drop_controller(&self) -> Option<Rc<dyn ITreeViewDragAndDropController>> {
        self.drag_and_drop_controller.borrow().clone()
    }

    fn set_drag_and_drop_controller(&self, controller: Option<Rc<dyn ITreeViewDragAndDropController>>) {
        *self.drag_and_drop_controller.borrow_mut() = controller.clone();
        if let Some(dnd) = self.tree_view_dnd.borrow().clone() {
            dnd.set_controller(controller);
        }
    }

    fn show_collapse_all_action(&self) -> bool {
        self.show_collapse_all.get()
    }

    fn set_show_collapse_all_action(&self, show: bool) {
        self.show_collapse_all.set(show);
    }

    fn can_select_many(&self) -> bool {
        self.can_select_many.get()
    }

    fn set_can_select_many(&self, can_select_many: bool) {
        self.can_select_many.set(can_select_many);
    }

    fn manually_manage_checkboxes(&self) -> bool {
        self.manually_manage_checkboxes.get()
    }

    fn set_manually_manage_checkboxes(&self, manually_manage_checkboxes: bool) {
        self.manually_manage_checkboxes.set(manually_manage_checkboxes);
    }

    fn message(&self) -> Option<StringOrMarkdown> {
        self.message.borrow().clone()
    }

    fn set_message(&self, message: Option<StringOrMarkdown>) {
        *self.message.borrow_mut() = message;
        self.update_message();
        self.on_did_change_welcome_state.fire(());
    }

    fn title(&self) -> String {
        self.title.borrow().clone()
    }

    fn set_title(&self, title: &str) {
        *self.title.borrow_mut() = title.to_owned();
        self.on_did_change_title.fire(title.to_owned());
    }

    fn description(&self) -> Option<String> {
        self.description.borrow().clone()
    }

    fn set_description(&self, description: Option<&str>) {
        *self.description.borrow_mut() = description.map(str::to_owned);
        self.on_did_change_description.fire(description.map(str::to_owned));
    }

    fn badge(&self) -> Option<IViewBadge> {
        self.badge.borrow().clone()
    }

    fn set_badge(&self, badge: Option<IViewBadge>) {
        if self.badge.borrow().as_ref().map(|badge| (badge.value, badge.tooltip.clone())) == badge.as_ref().map(|badge| (badge.value, badge.tooltip.clone())) {
            return;
        }
        *self.badge.borrow_mut() = badge.clone();
        match badge {
            Some(badge) => {
                let tooltip = badge.tooltip.clone();
                let activity = IActivity { badge: Rc::new(NumberBadge::new(badge.value, move |_| tooltip.clone())) };
                self.activity.set(Some(self.services.activity_service.show_view_activity(&self.id, activity)));
            }
            None => self.activity.clear(),
        }
    }

    fn visible(&self) -> bool {
        self.is_visible.get()
    }

    fn on_did_expand_item(&self) -> Event<ITreeItem> {
        self.on_did_expand_item.event()
    }

    fn on_did_collapse_item(&self) -> Event<ITreeItem> {
        self.on_did_collapse_item.event()
    }

    fn on_did_change_selection_and_focus(&self) -> Event<TreeSelectionAndFocus> {
        self.on_did_change_selection_and_focus.event()
    }

    fn on_did_change_visibility(&self) -> Event<bool> {
        self.on_did_change_visibility.event()
    }

    fn on_did_change_actions(&self) -> Event<()> {
        self.on_did_change_actions.event()
    }

    fn on_did_change_title(&self) -> Event<String> {
        self.on_did_change_title.event()
    }

    fn on_did_change_description(&self) -> Event<Option<String>> {
        self.on_did_change_description.event()
    }

    fn on_did_change_welcome_state(&self) -> Event<()> {
        self.on_did_change_welcome_state.event()
    }

    fn on_did_change_checkbox_state(&self) -> Event<Vec<ITreeItem>> {
        self.on_did_change_checkbox_state.event()
    }

    fn refresh(&self, tree_items: Option<&[ITreeItem]>, checkboxes_changed: Option<&[ITreeItem]>) -> Promise<()> {
        let this = self.rc();
        let elements = tree_items.map(<[ITreeItem]>::to_vec);
        let checkboxes = checkboxes_changed.map(<[ITreeItem]>::to_vec);
        async_fn(async move {
            if this.data_provider.borrow().is_some() && this.tree_created.get() {
                if this.refreshing.get() {
                    this.on_did_complete_refresh.event().to_promise(None).await?;
                }
                let elements = match elements {
                    Some(elements) => elements,
                    None => {
                        // remove all waiting elements to refresh if root is asked to refresh
                        this.elements_to_refresh.borrow_mut().clear();
                        vec![root_item()]
                    }
                };
                let element_handles = handles_of(&elements);
                {
                    let mut nodes = this.nodes.borrow_mut();
                    for element in elements.iter().chain(checkboxes.iter().flatten()) {
                        nodes.replace(element);
                    }
                    for handle in &element_handles {
                        // element.children = undefined: reset children
                        nodes.reset_children(handle);
                    }
                }
                if this.is_visible.get() {
                    let affected = {
                        let checkbox_handles = handles_of(&checkboxes.unwrap_or_default());
                        let mut nodes = this.nodes.borrow_mut();
                        nodes.cascade_checkboxes(&checkbox_handles)
                    };
                    let mut all = element_handles;
                    all.extend(affected);
                    if let Some(provider) = this.data_provider.borrow().clone() {
                        let snapshots: Vec<ITreeItem> = all.iter().filter_map(|handle| this.nodes.borrow().item_with_children(handle)).collect();
                        provider.did_update_items(&snapshots);
                    }
                    return this.do_refresh(all).await;
                }
                let mut pending = this.elements_to_refresh.borrow_mut();
                let seen: HashSet<String> = pending.iter().cloned().collect();
                if pending.is_empty() {
                    pending.extend(element_handles);
                } else {
                    for handle in element_handles {
                        if !seen.contains(&handle) {
                            pending.push(handle);
                        }
                    }
                }
            }
            Ok(())
        })
    }

    fn set_visibility(&self, visible: bool) {
        // Throughout setVisibility we need to check if the tree view's data provider still exists.
        // This can happen because the `getChildren` call to the extension can return
        // after the tree has been disposed.
        self.initialize();
        if self.is_visible.get() == visible {
            return;
        }
        self.is_visible.set(visible);

        if self.tree_created.get() && visible && !self.elements_to_refresh.borrow().is_empty() && self.data_provider.borrow().is_some() {
            let pending = std::mem::take(&mut *self.elements_to_refresh.borrow_mut());
            let _ = self.rc().do_refresh(pending);
        }

        let weak = self.this.clone();
        let now_visible = self.is_visible.get();
        set_timeout(
            move || {
                if let Some(view) = weak.upgrade()
                    && view.data_provider.borrow().is_some()
                {
                    view.on_did_change_visibility.fire(now_visible);
                }
            },
            0,
        );

        if visible {
            self.activate();
        }
    }

    fn focus(&self) {
        self.focus_item(true, None);
    }

    fn layout(&self, height: f64, width: f64) {
        if height != 0.0 && width != 0.0 {
            self.height.set(height);
            self.width.set(width);
        }
    }

    fn get_optimal_width(&self) -> f64 {
        0.0
    }

    fn reveal(&self, item: &ITreeItem) -> Promise<()> {
        if !self.tree_created.get() {
            return Promise::resolved(());
        }
        if !self.nodes.borrow().contains(&item.handle) {
            return Promise::rejected(tree_element_not_found());
        }
        self.reveal_handle(&item.handle);
        Promise::resolved(())
    }

    fn expand(&self, items: &[ITreeItem]) -> Promise<()> {
        let this = self.rc();
        let handles = handles_of(items);
        async_fn(async move {
            if !this.tree_created.get() {
                return Ok(());
            }
            // The extension could have changed the tree during the reveal.
            // Because of that, we ignore errors.
            for handle in handles {
                if !this.nodes.borrow().contains(&handle) {
                    break;
                }
                if !this.has_children(&handle) {
                    continue;
                }
                let changed = this.nodes.borrow_mut().expanded.insert(handle.clone());
                if changed && let Some(item) = this.nodes.borrow().item(&handle) {
                    this.on_did_expand_item.fire(item);
                }
                if !this.nodes.borrow().children_loaded(&handle) {
                    let _ = this.update_children(handle).await;
                }
            }
            Ok(())
        })
    }

    fn is_collapsed(&self, item: &ITreeItem) -> bool {
        self.tree_created.get() && self.has_children(&item.handle) && !self.nodes.borrow().expanded.contains(&item.handle)
    }

    fn set_selection(&self, items: &[ITreeItem]) {
        if !self.tree_created.get() {
            return;
        }
        let selection: Vec<String> = handles_of(items).into_iter().filter(|handle| self.nodes.borrow().contains(handle)).collect();
        let (elements, focus) = {
            let mut nodes = self.nodes.borrow_mut();
            nodes.selection = selection.clone();
            (nodes.items_of(&selection), nodes.focus.first().and_then(|handle| nodes.item(handle)))
        };
        *self.last_selection.borrow_mut() = elements.clone();
        if let Some(focus) = focus {
            *self.last_active.borrow_mut() = focus;
        }
        self.on_did_change_selection_and_focus.fire(TreeSelectionAndFocus { selection: elements, focus: self.last_active.borrow().clone() });
    }

    fn get_selection(&self) -> Vec<ITreeItem> {
        if !self.tree_created.get() {
            return Vec::new();
        }
        let nodes = self.nodes.borrow();
        nodes.items_of(&nodes.selection)
    }

    fn set_focus(&self, item: Option<&ITreeItem>) {
        if !self.tree_created.get() {
            return;
        }
        let Some(item) = item else {
            if self.nodes.borrow().focus.is_empty() {
                self.nodes.borrow_mut().focus = Vec::new();
            }
            return;
        };
        self.focus_item(true, Some(item));
        let Some(focused) = self.nodes.borrow().item(&item.handle) else { return };
        self.nodes.borrow_mut().focus = vec![item.handle.clone()];
        if focused.handle != self.last_active.borrow().handle {
            *self.last_active.borrow_mut() = focused.clone();
            let selection = self.get_selection();
            *self.last_selection.borrow_mut() = selection.clone();
            self.on_did_change_selection_and_focus.fire(TreeSelectionAndFocus { selection, focus: focused });
        }
    }
}
