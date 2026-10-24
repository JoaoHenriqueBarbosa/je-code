//! Porte de `src/vs/workbench/api/browser/mainThreadTreeViews.ts`: a ponte entre as árvores que as extensões registram
//! (`createTreeView`) e o `CustomTreeView` do workbench: o [`MainThreadTreeViews`], o `TreeViewDataProvider` e o
//! `TreeViewDragAndDropController`.
//!
//! Divergências:
//! - `@extHostNamedCustomer(MainContext.MainThreadTreeViews)`: o registro de clientes não existe ainda. O `IExtHostContext`
//!   do construtor é o [`IMainContext`], que dá o `getProxy`.
//! - O `Registry.as<IViewsRegistry>(Extensions.ViewsRegistry).getView(id)` com o `ITreeViewDescriptor` é o
//!   [`views_registry`] e o campo `tree_view` do descritor.
//! - Os objetos `ResolvableTreeItem` do `itemsMap` são `Rc` ([`ResolvableTreeItem`]); o que cruza o contrato do provedor são
//!   cópias dos itens, e as mudanças que a árvore faz neles (filhos carregados, checkbox) voltam por
//!   [`ITreeViewDataProvider::did_update_items`] (ver `tree_view_contracts`).
//! - `$resolveDropFileData` lança `Unknown tree` de forma síncrona ([`errors::throw`]), como o original. O `requestId` é
//!   `f64` no protocolo e `u32` no cache.
//! - O `$refresh` sem objeto (`itemsToRefreshByHandle` ausente) é `None`, que o original trata como vazio.
//! - `this._dataProviders.get(treeViewId)!` de `$reveal` rejeita a promessa quando o provedor não existe (o `TypeError`
//!   do original, dentro de um `then`).
//! - `notificationService.error(err)` é `error(vec![NotificationMessage::Error(err)])`.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::{Rc, Weak};

use je_base::async_::async_fn;
use je_base::buffer::VSBuffer;
use je_base::cancellation::CancellationToken;
use je_base::data_transfer::{StringOrPromise, VSDataTransfer, create_string_data_transfer_item};
use je_base::dynamic::Dynamic;
use je_base::errors::{self, Error};
use je_base::lifecycle::{Disposable, DisposableMap, DisposableStore, IDisposable};
use je_base::mime::mimes;
use je_base::promise::Promise;
use je_editor::languages::StringOrMarkdown;
use je_platform::log::{LogServiceRef, Logger};
use je_platform::notification::{INotificationService, NotificationMessage};
use je_platform::telemetry::telemetry::ITelemetryService;

use crate::api::common::ext_host_protocol::{
    CheckboxUpdate, ExtHostContext, ExtHostTreeViewsShape, IMainContext, ItemsToRefresh, MainThreadTreeViewsShape, RevealItemInfo, TreeChildrenBatch, TreeViewRegistrationOptions,
};
use crate::api::common::ext_host_type_converters::DataTransfer;
use crate::api::common::ext_host_type_converters::tree_view::revive_uri_list;
use crate::api::common::shared::data_transfer_cache::DataTransferFileCache;
use crate::common::tree_view_contracts::{ITreeView, ITreeViewDataProvider, ITreeViewDragAndDropController};
use crate::common::views::{IRevealOptions, ITreeItem, IViewBadge, ResolvableTreeItem, RevealExpand, TreeItemResolver, is_no_tree_view_error, views_registry};
use crate::services::extensions::extensions::IExtensionService;
use crate::services::views::views_service::IViewsService;

/// O valor do `_dataProviders`: `{ dataProvider, dispose }`.
struct ProviderEntry {
    data_provider: Rc<TreeViewDataProvider>,
    disposables: Rc<DisposableStore>,
}

impl IDisposable for ProviderEntry {
    fn dispose(&self) {
        self.disposables.dispose();
    }
}

/// `MainThreadTreeViews`.
pub struct MainThreadTreeViews {
    base: Rc<Disposable>,
    this: Weak<MainThreadTreeViews>,
    proxy: Rc<dyn ExtHostTreeViewsShape>,
    data_providers: Rc<DisposableMap<String, ProviderEntry>>,
    dnd_controllers: RefCell<HashMap<String, Rc<TreeViewDragAndDropController>>>,
    views_service: Rc<dyn IViewsService>,
    notification_service: Rc<dyn INotificationService>,
    extension_service: Rc<dyn IExtensionService>,
    log_service: LogServiceRef,
    telemetry_service: Rc<dyn ITelemetryService>,
}

impl MainThreadTreeViews {
    pub fn new(
        ext_host_context: &Rc<dyn IMainContext>,
        views_service: Rc<dyn IViewsService>,
        notification_service: Rc<dyn INotificationService>,
        extension_service: Rc<dyn IExtensionService>,
        log_service: LogServiceRef,
        telemetry_service: Rc<dyn ITelemetryService>,
    ) -> Rc<Self> {
        let base = Disposable::new();
        let data_providers = DisposableMap::new();
        base.register(data_providers.clone());
        let proxy = ext_host_context.get_proxy(&ExtHostContext::EXT_HOST_TREE_VIEWS);
        Rc::new_cyclic(|this| Self {
            base,
            this: this.clone(),
            proxy,
            data_providers,
            dnd_controllers: RefCell::new(HashMap::new()),
            views_service,
            notification_service,
            extension_service,
            log_service,
            telemetry_service,
        })
    }

    /// `getTreeView(treeViewId)`.
    fn get_tree_view(&self, tree_view_id: &str) -> Option<Rc<dyn ITreeView>> {
        views_registry().get_view(tree_view_id).and_then(|view_descriptor| view_descriptor.tree_view.clone())
    }

    /// `reveal(treeView, dataProvider, itemIn, parentChain, options)`.
    fn reveal(
        tree_view: Rc<dyn ITreeView>,
        data_provider: Rc<TreeViewDataProvider>,
        item_in: ITreeItem,
        parent_chain: Vec<ITreeItem>,
        options: IRevealOptions,
    ) -> Promise<()> {
        async_fn(async move {
            let select = options.select.unwrap_or(false);
            let focus = options.focus.unwrap_or(false);
            let mut expand = match options.expand {
                Some(RevealExpand::Depth(depth)) => depth,
                Some(RevealExpand::Bool(true)) => 1.0,
                _ => 0.0,
            }
            .min(3.0);

            if data_provider.is_empty() {
                // Refresh if empty
                tree_view.refresh(None, None).await?;
            }
            for parent in &parent_chain {
                if let Some(parent_item) = data_provider.get_item(&parent.handle) {
                    tree_view.expand(&[parent_item]).await?;
                }
            }
            if let Some(item) = data_provider.get_item(&item_in.handle) {
                tree_view.reveal(&item).await?;
                if select {
                    tree_view.set_selection(std::slice::from_ref(&item));
                }
                if !focus {
                    tree_view.set_focus(None);
                } else {
                    tree_view.set_focus(Some(&item));
                }
                let mut items_to_expand = vec![item];
                while !items_to_expand.is_empty() && expand > 0.0 {
                    tree_view.expand(&items_to_expand).await?;
                    items_to_expand = items_to_expand
                        .iter()
                        .filter_map(|item_value| data_provider.get_item(&item_value.handle))
                        .filter_map(|item| item.children.filter(|children| !children.is_empty()))
                        .flatten()
                        .collect();
                    expand -= 1.0;
                }
            }
            Ok(())
        })
    }

    /// `registerListeners(treeViewId, treeView, disposables)`.
    fn register_listeners(&self, tree_view_id: &str, tree_view: &Rc<dyn ITreeView>, disposables: &DisposableStore) {
        let id = tree_view_id.to_owned();
        let proxy = self.proxy.clone();
        disposables.add(tree_view.on_did_expand_item().listen({
            let (proxy, id) = (proxy.clone(), id.clone());
            move |item| proxy.rpc_set_expanded(&id, &item.handle, true)
        }));
        disposables.add(tree_view.on_did_collapse_item().listen({
            let (proxy, id) = (proxy.clone(), id.clone());
            move |item| proxy.rpc_set_expanded(&id, &item.handle, false)
        }));
        disposables.add(tree_view.on_did_change_selection_and_focus().listen({
            let (proxy, id) = (proxy.clone(), id.clone());
            move |items| proxy.rpc_set_selection_and_focus(&id, items.selection.iter().map(|item| item.handle.clone()).collect(), &items.focus.handle)
        }));
        disposables.add(tree_view.on_did_change_visibility().listen({
            let (proxy, id) = (proxy.clone(), id.clone());
            move |is_visible| proxy.rpc_set_visible(&id, *is_visible)
        }));
        disposables.add(tree_view.on_did_change_checkbox_state().listen(move |items| {
            proxy.rpc_change_checkbox_state(
                &id,
                items.iter().map(|item| CheckboxUpdate { tree_item_handle: item.handle.clone(), new_state: item.checkbox.as_ref().is_some_and(|checkbox| checkbox.is_checked) }).collect(),
            );
        }));
    }
}

impl IDisposable for MainThreadTreeViews {
    fn dispose(&self) {
        for tree_view_id in self.data_providers.keys() {
            if let Some(tree_view) = self.get_tree_view(&tree_view_id) {
                tree_view.set_drag_and_drop_controller(None);
                tree_view.set_data_provider(None);
            }
        }
        self.data_providers.dispose();

        self.dnd_controllers.borrow_mut().clear();

        self.base.dispose();
    }
}

impl MainThreadTreeViewsShape for MainThreadTreeViews {
    fn rpc_register_tree_view_data_provider(&self, tree_view_id: &str, options: TreeViewRegistrationOptions) -> Promise<()> {
        self.log_service.trace(&format!("MainThreadTreeViews#$registerTreeViewDataProvider {tree_view_id} {options:?}"));

        let (this, tree_view_id) = (self.this.clone(), tree_view_id.to_owned());
        let _ = self.extension_service.when_installed_extensions_registered().then(move |_| {
            let Some(this) = this.upgrade() else { return };
            let data_provider = TreeViewDataProvider::new(&tree_view_id, this.proxy.clone(), this.notification_service.clone());
            let disposables = DisposableStore::new();
            this.data_providers.set(tree_view_id.clone(), Rc::new(ProviderEntry { data_provider: data_provider.clone(), disposables: disposables.clone() }), false);
            let dnd_controller = (options.has_handle_drag || options.has_handle_drop).then(|| {
                Rc::new(TreeViewDragAndDropController::new(
                    &tree_view_id,
                    options.drop_mime_types.clone(),
                    options.drag_mime_types.clone(),
                    options.has_handle_drag,
                    this.proxy.clone(),
                ))
            });
            match this.get_tree_view(&tree_view_id) {
                Some(viewer) => {
                    // Order is important here. The internal tree isn't created until the dataProvider is set.
                    // Set all other properties first!
                    viewer.set_show_collapse_all_action(options.show_collapse_all);
                    viewer.set_can_select_many(options.can_select_many);
                    viewer.set_manually_manage_checkboxes(options.manually_manage_checkboxes);
                    viewer.set_drag_and_drop_controller(dnd_controller.clone().map(|controller| controller as Rc<dyn ITreeViewDragAndDropController>));
                    if let Some(controller) = dnd_controller {
                        this.dnd_controllers.borrow_mut().insert(tree_view_id.clone(), controller);
                    }
                    viewer.set_data_provider(Some(data_provider as Rc<dyn ITreeViewDataProvider>));
                    this.register_listeners(&tree_view_id, &viewer, &disposables);
                    this.proxy.rpc_set_visible(&tree_view_id, viewer.visible());
                }
                None => this.notification_service.error(vec![NotificationMessage::Text(format!("No view is registered with id: {tree_view_id}"))]),
            }
        });
        Promise::resolved(())
    }

    fn rpc_reveal(&self, tree_view_id: &str, item_info: Option<RevealItemInfo>, options: IRevealOptions) -> Promise<()> {
        self.log_service.trace(&format!(
            "MainThreadTreeViews#$reveal {tree_view_id} {:?} {:?} {options:?}",
            item_info.as_ref().map(|info| &info.item.handle),
            item_info.as_ref().map(|info| info.parent_chain.iter().map(|item| item.handle.clone()).collect::<Vec<_>>())
        ));

        let (this, tree_view_id) = (self.this.upgrade().expect("o cliente está vivo"), tree_view_id.to_owned());
        async_fn(async move {
            this.views_service.open_view(&tree_view_id, options.focus.unwrap_or(false)).await?;
            let viewer = this.get_tree_view(&tree_view_id);
            if let (Some(viewer), Some(item_info)) = (viewer, item_info) {
                let data_provider = this
                    .data_providers
                    .get(&tree_view_id)
                    .map(|entry| entry.data_provider.clone())
                    .ok_or_else(|| Error::new(format!("No data provider registered for tree view '{tree_view_id}'")))?;
                return Self::reveal(viewer, data_provider, item_info.item, item_info.parent_chain, options).await;
            }
            Ok(())
        })
    }

    fn rpc_refresh(&self, tree_view_id: &str, items_to_refresh: Option<ItemsToRefresh>) -> Promise<()> {
        self.log_service.trace(&format!("MainThreadTreeViews#$refresh {tree_view_id}"));

        let viewer = self.get_tree_view(tree_view_id);
        let data_provider = self.data_providers.get(&tree_view_id.to_owned());
        if let (Some(viewer), Some(data_provider)) = (viewer, data_provider) {
            let items_to_refresh = data_provider.data_provider.get_items_to_refresh(items_to_refresh.unwrap_or_default());
            return viewer.refresh(
                (!items_to_refresh.0.is_empty()).then_some(items_to_refresh.0.as_slice()),
                (!items_to_refresh.1.is_empty()).then_some(items_to_refresh.1.as_slice()),
            );
        }
        Promise::resolved(())
    }

    fn rpc_set_message(&self, tree_view_id: &str, message: StringOrMarkdown) {
        self.log_service.trace(&format!("MainThreadTreeViews#$setMessage {tree_view_id}"));

        if let Some(viewer) = self.get_tree_view(tree_view_id) {
            viewer.set_message(Some(message));
        }
    }

    fn rpc_set_title(&self, tree_view_id: &str, title: &str, description: Option<&str>) {
        self.log_service.trace(&format!("MainThreadTreeViews#$setTitle {tree_view_id} {title} {description:?}"));

        if let Some(viewer) = self.get_tree_view(tree_view_id) {
            viewer.set_title(title);
            viewer.set_description(description);
        }
    }

    fn rpc_set_badge(&self, tree_view_id: &str, badge: Option<IViewBadge>) {
        self.log_service.trace(&format!("MainThreadTreeViews#$setBadge {tree_view_id} {:?}", badge.as_ref().map(|badge| (badge.value, badge.tooltip.clone()))));

        if let Some(viewer) = self.get_tree_view(tree_view_id) {
            viewer.set_badge(badge);
        }
    }

    fn rpc_resolve_drop_file_data(&self, destination_view_id: &str, request_id: f64, data_item_id: &str) -> Promise<VSBuffer> {
        let controller = self.dnd_controllers.borrow().get(destination_view_id).cloned();
        let Some(controller) = controller else { errors::throw(Error::new("Unknown tree")) };
        controller.resolve_drop_file_data(request_id, data_item_id)
    }

    fn rpc_dispose_tree(&self, tree_view_id: &str) -> Promise<()> {
        if let Some(viewer) = self.get_tree_view(tree_view_id) {
            viewer.set_drag_and_drop_controller(None);
            viewer.set_data_provider(None);
        }

        self.dnd_controllers.borrow_mut().remove(tree_view_id);
        self.data_providers.delete_and_dispose(&tree_view_id.to_owned());
        Promise::resolved(())
    }

    fn rpc_log_resolve_tree_node_failure(&self, extension_id: &str) {
        self.telemetry_service.public_log2("treeView.resolveFailure", Some(Dynamic::object([("extensionId", Dynamic::string(extension_id))])));
    }
}

/// `TreeViewDragAndDropController`.
struct TreeViewDragAndDropController {
    tree_view_id: String,
    drop_mime_types: Vec<String>,
    drag_mime_types: Vec<String>,
    has_will_drop: bool,
    proxy: Rc<dyn ExtHostTreeViewsShape>,
    data_transfers_cache: Rc<DataTransferFileCache>,
}

impl TreeViewDragAndDropController {
    fn new(tree_view_id: &str, drop_mime_types: Vec<String>, drag_mime_types: Vec<String>, has_will_drop: bool, proxy: Rc<dyn ExtHostTreeViewsShape>) -> Self {
        Self { tree_view_id: tree_view_id.to_owned(), drop_mime_types, drag_mime_types, has_will_drop, proxy, data_transfers_cache: Rc::new(DataTransferFileCache::default()) }
    }

    /// `resolveDropFileData(requestId, dataItemId)`.
    fn resolve_drop_file_data(&self, request_id: f64, data_item_id: &str) -> Promise<VSBuffer> {
        self.data_transfers_cache.resolve_file_data(request_id as u32, data_item_id)
    }
}

impl ITreeViewDragAndDropController for TreeViewDragAndDropController {
    fn drop_mime_types(&self) -> Vec<String> {
        self.drop_mime_types.clone()
    }

    fn drag_mime_types(&self) -> Vec<String> {
        self.drag_mime_types.clone()
    }

    fn handle_drag(&self, source_tree_item_handles: Vec<String>, operation_uuid: &str, token: CancellationToken) -> Promise<Option<Rc<VSDataTransfer>>> {
        if !self.has_will_drop {
            return Promise::resolved(None);
        }
        let additional_data_transfer_dto = self.proxy.rpc_handle_drag(&self.tree_view_id, source_tree_item_handles, operation_uuid, token);
        additional_data_transfer_dto.then(|dto| {
            let dto = dto?;
            let mut additional_data_transfer = VSDataTransfer::new();
            for (type_, item) in &dto.items {
                // For text/uri-list, reconstruct from uriListData which has been transformed by the URI transformer
                let value = match &item.uri_list_data {
                    Some(parts) if type_ == mimes::URI_LIST => revive_uri_list(parts),
                    _ => item.as_string.clone(),
                };
                additional_data_transfer.replace(type_, create_string_data_transfer_item(StringOrPromise::String(value), None));
            }
            Some(Rc::new(additional_data_transfer))
        })
    }

    fn handle_drag_end(&self, operation_uuid: &str) {
        self.proxy.rpc_release_drag_operation(operation_uuid);
    }

    fn handle_drop(
        &self,
        elements: Rc<VSDataTransfer>,
        target: Option<ITreeItem>,
        token: CancellationToken,
        operation_uuid: Option<&str>,
        source_tree_id: Option<&str>,
        source_tree_item_handles: Option<Vec<String>>,
    ) -> Promise<()> {
        let request = self.data_transfers_cache.add(&elements);
        let (proxy, tree_view_id) = (self.proxy.clone(), self.tree_view_id.clone());
        let (operation_uuid, source_tree_id) = (operation_uuid.map(str::to_owned), source_tree_id.map(str::to_owned));
        async_fn(async move {
            let result: Result<(), Error> = async {
                let data_transfer_dto = DataTransfer::from_list(&elements).await?;
                if token.is_cancellation_requested() {
                    return Ok(());
                }
                proxy
                    .rpc_handle_drop(&tree_view_id, f64::from(request.id), data_transfer_dto, target.map(|target| target.handle), token, operation_uuid, source_tree_id, source_tree_item_handles)
                    .await
            }
            .await;
            request.dispose();
            result
        })
    }
}

/// `TreeViewDataProvider`.
pub struct TreeViewDataProvider {
    tree_view_id: String,
    proxy: Rc<dyn ExtHostTreeViewsShape>,
    notification_service: Rc<dyn INotificationService>,
    items_map: RefCell<HashMap<String, Rc<ResolvableTreeItem>>>,
    has_resolve: Promise<bool>,
    this: Weak<TreeViewDataProvider>,
}

impl TreeViewDataProvider {
    fn new(tree_view_id: &str, proxy: Rc<dyn ExtHostTreeViewsShape>, notification_service: Rc<dyn INotificationService>) -> Rc<Self> {
        let has_resolve = proxy.rpc_has_resolve(tree_view_id);
        Rc::new_cyclic(|this| Self {
            tree_view_id: tree_view_id.to_owned(),
            proxy,
            notification_service,
            items_map: RefCell::new(HashMap::new()),
            has_resolve,
            this: this.clone(),
        })
    }

    fn rc(&self) -> Rc<Self> {
        self.this.upgrade().expect("o provedor está vivo enquanto um método dele roda")
    }

    /// `getChildrenBatch(treeItems?)`.
    fn get_children_batch_impl(&self, tree_items: Option<Vec<ITreeItem>>) -> Promise<Option<Vec<Vec<ITreeItem>>>> {
        if tree_items.is_none() {
            self.items_map.borrow_mut().clear();
        }
        let parents_len = tree_items.as_ref().map_or(0, Vec::len);
        let handles = tree_items.map(|items| items.into_iter().map(|item| item.handle).collect());
        let this = self.rc();
        self.proxy.rpc_get_children(&self.tree_view_id, handles).then_result_promise(move |result| match result {
            Ok(children) => {
                let converted_children = Self::convert_transfer_children(parents_len, children);
                this.post_get_children(converted_children)
            }
            Err(err) => {
                // It can happen that a tree view is disposed right as `getChildren` is called. This results in an error because the data provider gets removed.
                // The tree will shortly get cleaned up in this case. We just need to handle the error here.
                if !is_no_tree_view_error(&err) {
                    this.notification_service.error(vec![NotificationMessage::Error(err)]);
                }
                Promise::resolved(Some(Vec::new()))
            }
        })
    }

    /// `convertTransferChildren(parents, children)`: o vetor esparso do original, com `None` nos buracos.
    fn convert_transfer_children(parents_len: usize, children: Option<Vec<TreeChildrenBatch>>) -> Vec<Option<Vec<ITreeItem>>> {
        let mut converted_children: Vec<Option<Vec<ITreeItem>>> = vec![None; parents_len];
        for child_group in children.into_iter().flatten() {
            if child_group.index >= converted_children.len() {
                converted_children.resize(child_group.index + 1, None);
            }
            converted_children[child_group.index] = Some(child_group.children);
        }
        converted_children
    }

    /// `getItemsToRefresh(itemsToRefreshByHandle)`: os itens e os itens de checkbox mudado.
    fn get_items_to_refresh(&self, items_to_refresh_by_handle: ItemsToRefresh) -> (Vec<ITreeItem>, Vec<ITreeItem>) {
        let mut items_to_refresh: Vec<ITreeItem> = Vec::new();
        let mut checkboxes_to_refresh: Vec<ITreeItem> = Vec::new();
        for (new_tree_item_handle, new_tree_item) in items_to_refresh_by_handle {
            let current_tree_item = self.items_map.borrow().get(&new_tree_item_handle).cloned();
            if let Some(current_tree_item) = current_tree_item {
                // Refresh only if the item exists
                let current_checked = current_tree_item.to_item().checkbox.map(|checkbox| checkbox.is_checked);
                if current_checked != new_tree_item.checkbox.as_ref().map(|checkbox| checkbox.is_checked) {
                    checkboxes_to_refresh.push(current_tree_item.to_item());
                }
                // Update the current item with refreshed item
                let new_handle = new_tree_item.handle.clone();
                let new_parent_handle = new_tree_item.parent_handle.clone();
                Self::update_tree_item(&current_tree_item, new_tree_item);
                if new_tree_item_handle == new_handle {
                    items_to_refresh.push(current_tree_item.to_item());
                } else {
                    // Update maps when handle is changed and refresh parent
                    let mut items_map = self.items_map.borrow_mut();
                    items_map.remove(&new_tree_item_handle);
                    items_map.insert(new_handle, current_tree_item.clone());
                    let parent = new_parent_handle.and_then(|handle| items_map.get(&handle).cloned());
                    if let Some(parent) = parent {
                        items_to_refresh.push(parent.to_item());
                    }
                }
            }
        }
        (items_to_refresh, checkboxes_to_refresh)
    }

    /// `getItem(treeItemHandle)`.
    pub fn get_item(&self, tree_item_handle: &str) -> Option<ITreeItem> {
        self.items_map.borrow().get(tree_item_handle).map(|item| item.to_item())
    }

    /// `isEmpty()`.
    pub fn is_empty(&self) -> bool {
        self.items_map.borrow().is_empty()
    }

    /// `postGetChildren(elementGroups)`.
    fn post_get_children(&self, element_groups: Vec<Option<Vec<ITreeItem>>>) -> Promise<Option<Vec<Vec<ITreeItem>>>> {
        let this = self.rc();
        async_fn(async move {
            let mut result_groups: Vec<Vec<ITreeItem>> = Vec::new();
            let has_resolve = this.has_resolve.clone().await?;
            for elements in element_groups {
                let mut result: Vec<ITreeItem> = Vec::new();
                for element in elements.unwrap_or_default() {
                    let resolve: Option<TreeItemResolver> = has_resolve.then(|| {
                        let (proxy, tree_view_id, handle) = (this.proxy.clone(), this.tree_view_id.clone(), element.handle.clone());
                        Rc::new(move |token: CancellationToken| proxy.rpc_resolve(&tree_view_id, &handle, token)) as TreeItemResolver
                    });
                    let resolvable = ResolvableTreeItem::new(element.clone(), resolve);
                    this.items_map.borrow_mut().insert(element.handle.clone(), resolvable.clone());
                    result.push(resolvable.to_item());
                }
                result_groups.push(result);
            }
            Ok(Some(result_groups))
        })
    }

    /// `updateTreeItem(current, treeItem)`: copia todas as propriedades do item novo para o atual.
    fn update_tree_item(current: &Rc<ResolvableTreeItem>, tree_item: ITreeItem) {
        current.update(|current| {
            current.handle = tree_item.handle;
            current.parent_handle = tree_item.parent_handle;
            current.collapsible_state = tree_item.collapsible_state;
            current.label = tree_item.label;
            current.description = tree_item.description;
            current.icon = tree_item.icon;
            current.icon_dark = tree_item.icon_dark;
            current.theme_icon = tree_item.theme_icon;
            current.resource_uri = tree_item.resource_uri;
            current.tooltip = tree_item.tooltip;
            current.context_value = tree_item.context_value;
            current.command = tree_item.command;
            current.children = tree_item.children;
            current.accessibility_information = tree_item.accessibility_information;
            // As propriedades que o item novo não tem (`checkbox`, `parent`) ficam como estão.
            if tree_item.checkbox.is_some() {
                current.checkbox = tree_item.checkbox;
            }
            if tree_item.parent.is_some() {
                current.parent = tree_item.parent;
            }
            for (key, value) in tree_item.custom_properties {
                match current.custom_properties.iter_mut().find(|(existing, _)| *existing == key) {
                    Some(entry) => entry.1 = value,
                    None => current.custom_properties.push((key, value)),
                }
            }
        });
        current.reset_resolve();
    }
}

impl ITreeViewDataProvider for TreeViewDataProvider {
    fn get_children(&self, element: Option<&ITreeItem>) -> Promise<Option<Vec<ITreeItem>>> {
        self.get_children_batch_impl(element.map(|element| vec![element.clone()])).then(|batches| batches.and_then(|batches| batches.into_iter().next()))
    }

    fn get_children_batch(&self, elements: Option<&[ITreeItem]>) -> Option<Promise<Option<Vec<Vec<ITreeItem>>>>> {
        Some(self.get_children_batch_impl(elements.map(<[ITreeItem]>::to_vec)))
    }

    fn did_update_items(&self, items: &[ITreeItem]) {
        for item in items {
            if let Some(resolvable) = self.items_map.borrow().get(&item.handle) {
                resolvable.update(|current| {
                    current.children = item.children.clone();
                    current.checkbox = item.checkbox.clone();
                });
            }
        }
    }
}
