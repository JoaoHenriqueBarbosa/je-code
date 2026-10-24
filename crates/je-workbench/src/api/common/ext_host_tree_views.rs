//! Porte de `src/vs/workbench/api/common/extHostTreeViews.ts`: o lado da extensão das árvores de visões
//! (`vscode.window.createTreeView`, `registerTreeDataProvider`): o cache de nós por elemento, os handles, o
//! `getChildren` em lote, o refresh com debounce, o `reveal`, os checkboxes, o arrastar e soltar e a resolução de itens.
//!
//! Divergências:
//! - **Os valores da extensão são [`Dynamic`]** (decisão de arquitetura do host, `wip/notes/exthost-plan.md`): o
//!   elemento `T` do provedor é opaco ([`Dynamic`], comparado por identidade se for objeto e por valor se for
//!   primitivo, como as chaves de um `Map`), e o `vscode.TreeItem` que `getTreeItem` devolve é lido como objeto
//!   qualquer (a interface, com ou sem a classe `TreeItem`), então as propriedades que o original lê com `item.id`,
//!   `item.label` etc. são `get_str`. O `TreeItem` do `ext_host_types` (struct) não é o que cruza aqui.
//! - **`TreeDataProvider<T>`, `TreeViewOptions<T>` e `TreeDragAndDropController<T>`** são structs de fechos
//!   ([`TreeDataProvider`], [`TreeViewOptions`], [`TreeDragAndDropController`]): o método opcional do objeto do original
//!   (`getParent`, `resolveTreeItem`, `onDidChangeTreeData`, `handleDrag`, `handleDrop`) é um `Option`, e
//!   `typeof provider.getParent !== 'function'` é `get_parent.is_none()`. `ProviderResult<T>` é [`MaybePromise`] do
//!   valor; `undefined`/`null` são `Dynamic::Undefined`/`Dynamic::Null` (ou `None` na lista de filhos).
//! - **Exceções**: o que lança no original e não devolve promessa lança como pânico com o [`Error`] (ver
//!   `ext_host_commands`), os métodos `async` rejeitam. `createTreeView` não lança `Options with treeDataProvider is
//!   mandatory`: o provedor é obrigatório pelo tipo de [`TreeViewOptions`]; devolve [`Result`] só pelo erro de
//!   `toTreeItemLabel` com a proposta de API desligada e pelos itens inválidos.
//! - **A `vscode.TreeView<T>`** que `createTreeView` devolve é o [`TreeViewHandle`] (os getters e setters do objeto
//!   literal do original são métodos; `activeItem`, `onDidChangeActiveItem` e a mensagem em markdown devolvem
//!   [`Result`] porque `checkProposedApiEnabled` lança). `dispose` assíncrono é [`TreeViewHandle::dispose_async`], e o
//!   [`IDisposable::dispose`] dispara essa promessa sem esperá-la (como `dispose()` de uma função `async` ignorada).
//! - **Valores `Object.freeze`** dos eventos (`{ element }`, `{ selection }`...) não existem: os structs dos eventos
//!   não têm setters.
//! - Os itens que o host entrega ao protocolo são **cópias** do `node.item` (no original é o mesmo objeto, que o
//!   `resolveTreeItem` muta depois); um [`ITreeItem`] já entregue não vê a resolução posterior.
//! - A política de `itemsToRefresh`, um objeto indexado por handle, é a lista de pares [`ItemsToRefresh`], na ordem
//!   em que cada nó terminou de ser atualizado.
//! - `ExtHostTreeViews` guarda os filhos registrados num [`DisposableStore`] (o `Disposable` base do original, com o
//!   `delete`/`deleteAndLeak` que `dispose` do `view` usa).
//! - O `TreeViewsDnDService` do editor (`_treeDragAndDropService`, privado no original) é lido pela suíte por
//!   [`ExtHostTreeViews::tree_drag_and_drop_service`].
//! - `NoTreeViewError` é o [`Error`] de nome `NoTreeViewError` (`common::views`); `IExtensionDescription` é o
//!   `ExtensionDescription` de `je_extensions`; o `LogLevel`/`ILogService` são os de `je_platform::log`.
//! - Um [`Dynamic`] `MarkdownString` é o objeto marcado com `$$class` (sem o duck typing de `isMarkdownString`), e o
//!   `collapsibleState` que não é `0`, `1` ou `2` lê como `None` (ver `common::views`).

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::rc::{Rc, Weak};

use je_base::arrays::distinct;
use je_base::async_::{async_fn, async_fn_maybe, await_maybe};
use je_base::cancellation::{CancellationToken, CancellationTokenSource};
use je_base::dynamic::{json_stringify, number_to_js_string, Dynamic, Truthy};
use je_base::errors::{self, Error};
use je_base::event::{DebounceOptions, Emitter, Event};
use je_base::js_string::JsString;
use je_base::lifecycle::{dispose_all, id_of, mark_as_disposed, Disposable, DisposableRef, DisposableStore, IDisposable};
use je_base::promise::{MaybePromise, Promise, Resolver};
use je_base::symbols::Delay;
use je_base::uri::URI;
use je_editor::languages::StringOrMarkdown;
use je_editor::tree_views_dnd::{ITreeViewsDnDService, TreeViewsDnDService};
use je_extensions::scanner::ExtensionDescription;
use je_platform::log::{LogLevel, LogMessage, LogServiceRef, Logger};
use je_platform::resources::{ext_uri, uri_file};

use super::ext_host_commands::{ArgumentProcessor, ExtHostCommands};
use super::ext_host_protocol::{
    CheckboxUpdate, DataTransferDto, ExtHostTreeViewsShape, ItemsToRefresh, MainThreadTreeViewsShape, RevealItemInfo, TreeChildrenBatch, TreeViewRegistrationOptions,
};
use super::ext_host_type_converters::{self as type_converters, ICommandsConverter, ResolveFileData};
use super::ext_host_types::{self as types, Command, DataTransfer, Documentation, TreeItemCheckboxState, ViewBadge};
use super::ext_host_values::{is_instance_of, ApiValue};
use crate::common::views::{
    no_tree_view_error, AccessibilityInformation, IRevealOptions, ITreeItem, ITreeItemCheckboxState, ITreeItemLabel, RevealExpand, TreeCommand,
    TreeItemCollapsibleState, TreeItemDescription,
};
use crate::services::extensions::extensions::check_proposed_api_enabled;

type TreeItemHandle = String;

// ---- a API: provedor, opções, controlador de arrastar e soltar, eventos ------------------------------

/// `vscode.TreeDataProvider<T>`.
pub struct TreeDataProvider {
    /// `getTreeItem(element)`: `TreeItem | Thenable<TreeItem>`.
    pub get_tree_item: Rc<dyn Fn(&Dynamic) -> MaybePromise<Dynamic>>,
    /// `getChildren(element?)`: `ProviderResult<T[]>`.
    pub get_children: Rc<dyn Fn(Option<&Dynamic>) -> MaybePromise<Option<Vec<Dynamic>>>>,
    /// `getParent(element)` (opcional): `ProviderResult<T>`.
    pub get_parent: Option<Rc<dyn Fn(&Dynamic) -> MaybePromise<Dynamic>>>,
    /// `resolveTreeItem(item, element, token)` (opcional): `ProviderResult<TreeItem>`.
    pub resolve_tree_item: Option<Rc<dyn Fn(&Dynamic, &Dynamic, &CancellationToken) -> MaybePromise<Dynamic>>>,
    /// `onDidChangeTreeData` (opcional): `T | T[] | undefined | null | void`.
    pub on_did_change_tree_data: Option<Event<Dynamic>>,
}

/// `handleDrag(source, dataTransfer, token)`.
pub type TreeHandleDrag = Rc<dyn Fn(&[Dynamic], &Rc<DataTransfer>, &CancellationToken) -> MaybePromise<()>>;

/// `handleDrop(target, dataTransfer, token)`.
pub type TreeHandleDrop = Rc<dyn Fn(Option<&Dynamic>, &Rc<DataTransfer>, &CancellationToken) -> MaybePromise<()>>;

/// `vscode.TreeDragAndDropController<T>`.
#[derive(Clone, Default)]
pub struct TreeDragAndDropController {
    pub drop_mime_types: Vec<String>,
    pub drag_mime_types: Vec<String>,
    pub handle_drag: Option<TreeHandleDrag>,
    pub handle_drop: Option<TreeHandleDrop>,
}

/// `vscode.TreeViewOptions<T>`.
#[derive(Clone)]
pub struct TreeViewOptions {
    pub tree_data_provider: Rc<TreeDataProvider>,
    pub show_collapse_all: bool,
    pub can_select_many: bool,
    pub drag_and_drop_controller: Option<Rc<TreeDragAndDropController>>,
    pub manage_checkbox_state_manually: bool,
}

impl TreeViewOptions {
    /// `{ treeDataProvider }`.
    pub fn new(tree_data_provider: Rc<TreeDataProvider>) -> TreeViewOptions {
        TreeViewOptions { tree_data_provider, show_collapse_all: false, can_select_many: false, drag_and_drop_controller: None, manage_checkbox_state_manually: false }
    }
}

/// `vscode.TreeViewExpansionEvent<T>`.
#[derive(Clone, Debug)]
pub struct TreeViewExpansionEvent {
    pub element: Dynamic,
}

/// `vscode.TreeViewSelectionChangeEvent<T>`.
#[derive(Clone, Debug)]
pub struct TreeViewSelectionChangeEvent {
    pub selection: Vec<Dynamic>,
}

/// `vscode.TreeViewActiveItemChangeEvent<T>`.
#[derive(Clone, Debug)]
pub struct TreeViewActiveItemChangeEvent {
    pub active_item: Option<Dynamic>,
}

/// `vscode.TreeViewVisibilityChangeEvent`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TreeViewVisibilityChangeEvent {
    pub visible: bool,
}

/// `vscode.TreeCheckboxChangeEvent<T>`: o par `[element, newState]` de cada item.
#[derive(Clone, Debug)]
pub struct TreeCheckboxChangeEvent {
    pub items: Vec<(Dynamic, TreeItemCheckboxState)>,
}

// ---- auxiliares --------------------------------------------------------------------------------------

/// `${value}` num template literal.
fn js_to_string(value: &Dynamic) -> String {
    match value {
        Dynamic::String(text) => text.to_string_lossy(),
        Dynamic::Number(number) => number_to_js_string(*number),
        Dynamic::Bool(flag) => flag.to_string(),
        Dynamic::BigInt(number) => number.to_string(),
        Dynamic::Undefined => "undefined".to_owned(),
        Dynamic::Null => "null".to_owned(),
        _ => "[object Object]".to_owned(),
    }
}

/// `asPromise(callback)`: o valor resolve já; a promessa devolvida é seguida (`item.then(resolve, reject)`); o que o
/// `callback` lança rejeita.
fn as_promise_maybe<T: Clone + 'static>(callback: impl FnOnce() -> MaybePromise<T>) -> Promise<T> {
    Promise::with_executor(|resolver: Resolver<T>| match callback() {
        MaybePromise::Value(value) => resolver.resolve(value),
        MaybePromise::Promise(promise) => promise.pipe_to(&resolver),
    })
}

/// `Promise.resolve(value)` para o que pode ou não ser promessa.
fn maybe_to_promise<T: Clone + 'static>(value: MaybePromise<T>) -> Promise<T> {
    match value {
        MaybePromise::Value(value) => Promise::resolved(value),
        MaybePromise::Promise(promise) => promise,
    }
}

/// A chave de um elemento no `Map` do original: `SameValueZero` (primitivos por valor, objetos por identidade).
#[derive(Clone, PartialEq, Eq, Hash)]
enum ElementKey {
    Undefined,
    Null,
    Bool(bool),
    Number(u64),
    BigInt(i128),
    String(Vec<u16>),
    Identity(usize),
}

fn element_key(value: &Dynamic) -> ElementKey {
    match value {
        Dynamic::Undefined => ElementKey::Undefined,
        Dynamic::Null => ElementKey::Null,
        Dynamic::Bool(flag) => ElementKey::Bool(*flag),
        Dynamic::Number(number) => {
            let bits = if number.is_nan() {
                0x7ff8_0000_0000_0000
            } else if *number == 0.0 {
                0.0_f64.to_bits()
            } else {
                number.to_bits()
            };
            ElementKey::Number(bits)
        }
        Dynamic::BigInt(number) => ElementKey::BigInt(*number),
        Dynamic::String(text) => ElementKey::String(text.units().to_vec()),
        other => ElementKey::Identity(other.identity().expect("os valores que não são primitivos têm identidade")),
    }
}

/// `Map<T, V>`: guarda o elemento junto da chave, para o endereço de um objeto não ser reaproveitado enquanto a
/// entrada existe.
struct ElementMap<V: Clone> {
    entries: HashMap<ElementKey, (Dynamic, V)>,
}

impl<V: Clone> ElementMap<V> {
    fn new() -> ElementMap<V> {
        ElementMap { entries: HashMap::new() }
    }

    fn get(&self, element: &Dynamic) -> Option<V> {
        self.entries.get(&element_key(element)).map(|(_, value)| value.clone())
    }

    fn insert(&mut self, element: &Dynamic, value: V) {
        self.entries.insert(element_key(element), (element.clone(), value));
    }

    fn remove(&mut self, element: &Dynamic) -> Option<V> {
        self.entries.remove(&element_key(element)).map(|(_, value)| value)
    }

    fn clear(&mut self) {
        self.entries.clear();
    }

    fn len(&self) -> usize {
        self.entries.len()
    }

    fn values(&self) -> Vec<V> {
        self.entries.values().map(|(_, value)| value.clone()).collect()
    }
}

/// `string | undefined` de uma propriedade que o original passa adiante sem validar.
fn optional_string(value: &Dynamic) -> Option<String> {
    match value {
        Dynamic::String(text) => Some(text.to_string_lossy()),
        _ => None,
    }
}

/// `accessibilityInformation` de um item: `{ label, role? }`.
fn to_accessibility_information(value: &Dynamic) -> Option<AccessibilityInformation> {
    if value.is_nullish() {
        return None;
    }
    Some(AccessibilityInformation { label: optional_string(&value.get_str("label")).unwrap_or_default(), role: optional_string(&value.get_str("role")) })
}

fn to_tree_item_label(label: &Dynamic, extension: &ExtensionDescription) -> Result<Option<ITreeItemLabel>, Error> {
    if let Dynamic::String(text) = label {
        return Ok(Some(ITreeItemLabel { label: StringOrMarkdown::String(text.to_string_lossy()), highlights: None }));
    }

    if label.is_truthy() && label.type_of() == "object" {
        let inner = label.get_str("label");
        if inner.is_truthy() {
            let mut highlights: Option<Vec<(f64, f64)>> = None;
            if let Dynamic::Array(items) = label.get_str("highlights") {
                let filtered: Vec<(f64, f64)> = items
                    .to_vec()
                    .iter()
                    .filter_map(|highlight| match highlight {
                        Dynamic::Array(pair) if pair.len() == 2 => match (pair.get(0), pair.get(1)) {
                            (Dynamic::Number(start), Dynamic::Number(end)) => Some((start, end)),
                            _ => None,
                        },
                        _ => None,
                    })
                    .collect();
                highlights = if filtered.is_empty() { None } else { Some(filtered) };
            }
            if let Dynamic::String(text) = &inner {
                return Ok(Some(ITreeItemLabel { label: StringOrMarkdown::String(text.to_string_lossy()), highlights }));
            } else if is_instance_of(&inner, "MarkdownString") {
                check_proposed_api_enabled(extension, "treeItemMarkdownLabel")?;
                let markdown = types::MarkdownString::from_dynamic(&inner)?;
                return Ok(Some(ITreeItemLabel { label: StringOrMarkdown::Markdown(type_converters::MarkdownString::from_markdown(&markdown)), highlights }));
            }
        }
    }

    Ok(None)
}

// ---- ExtHostTreeViews ---------------------------------------------------------------------------------

/// `isTreeViewConvertableItem(arg)`.
fn is_tree_view_convertable_item(arg: &Dynamic) -> bool {
    arg.is_truthy()
        && arg.get_str("$treeViewId").is_truthy()
        && (arg.get_str("$treeItemHandle").is_truthy() || arg.get_str("$selectedTreeItems").is_truthy() || arg.get_str("$focusedTreeItem").is_truthy())
}

/// O processador de argumentos de comando que o construtor de `ExtHostTreeViews` registra.
struct TreeViewArgumentProcessor {
    views: Weak<ExtHostTreeViews>,
}

impl ArgumentProcessor for TreeViewArgumentProcessor {
    fn process_argument(&self, arg: &Dynamic, _extension: Option<&ExtensionDescription>) -> Dynamic {
        let Some(views) = self.views.upgrade() else {
            return arg.clone();
        };
        if is_tree_view_convertable_item(arg) {
            return views.convert_argument(arg);
        } else if let Dynamic::Array(items) = arg
            && !items.is_empty()
        {
            return Dynamic::array(items.to_vec().iter().map(|item| if is_tree_view_convertable_item(item) { views.convert_argument(item) } else { item.clone() }));
        }
        arg.clone()
    }
}

/// `ExtHostTreeViews`.
pub struct ExtHostTreeViews {
    this: Weak<ExtHostTreeViews>,
    store: Rc<DisposableStore>,
    tree_views: RefCell<HashMap<String, Rc<ExtHostTreeView>>>,
    tree_drag_and_drop_service: Rc<TreeViewsDnDService<Rc<DataTransfer>>>,
    proxy: Rc<dyn MainThreadTreeViewsShape>,
    commands: Rc<ExtHostCommands>,
    log_service: LogServiceRef,
}

impl ExtHostTreeViews {
    pub fn new(proxy: Rc<dyn MainThreadTreeViewsShape>, commands: &Rc<ExtHostCommands>, log_service: LogServiceRef) -> Rc<ExtHostTreeViews> {
        let views = Rc::new_cyclic(|this| ExtHostTreeViews {
            this: this.clone(),
            store: DisposableStore::new(),
            tree_views: RefCell::new(HashMap::new()),
            tree_drag_and_drop_service: Rc::new(TreeViewsDnDService::new()),
            proxy,
            commands: commands.clone(),
            log_service,
        });
        commands.register_argument_processor(Rc::new(TreeViewArgumentProcessor { views: Rc::downgrade(&views) }));
        views
    }

    /// O `Rc` deste host, para as operações assíncronas que precisam dele depois do retorno.
    fn rc(&self) -> Rc<ExtHostTreeViews> {
        self.this.upgrade().expect("o ExtHostTreeViews está vivo enquanto um método dele roda")
    }

    /// `_treeDragAndDropService`.
    pub fn tree_drag_and_drop_service(&self) -> Rc<TreeViewsDnDService<Rc<DataTransfer>>> {
        self.tree_drag_and_drop_service.clone()
    }

    fn tree_view(&self, tree_view_id: &str) -> Option<Rc<ExtHostTreeView>> {
        self.tree_views.borrow().get(tree_view_id).cloned()
    }

    /// `registerTreeDataProvider(id, treeDataProvider, extension)`.
    pub fn register_tree_data_provider(&self, id: &str, tree_data_provider: Rc<TreeDataProvider>, extension: &ExtensionDescription) -> Result<types::Disposable, Error> {
        let tree_view = self.create_tree_view(id, TreeViewOptions::new(tree_data_provider), extension)?;
        Ok(types::Disposable::new(move || tree_view.dispose()))
    }

    /// `createTreeView(viewId, options, extension)`.
    pub fn create_tree_view(&self, view_id: &str, options: TreeViewOptions, extension: &ExtensionDescription) -> Result<Rc<TreeViewHandle>, Error> {
        let controller = options.drag_and_drop_controller.clone();
        let drop_mime_types = controller.as_ref().map(|controller| controller.drop_mime_types.clone()).unwrap_or_default();
        let drag_mime_types = controller.as_ref().map(|controller| controller.drag_mime_types.clone()).unwrap_or_default();
        let has_handle_drag = controller.as_ref().is_some_and(|controller| controller.handle_drag.is_some());
        let has_handle_drop = controller.as_ref().is_some_and(|controller| controller.handle_drop.is_some());
        let tree_view = self.create_ext_host_tree_view(view_id, &options, extension);
        let proxy_options = TreeViewRegistrationOptions {
            show_collapse_all: options.show_collapse_all,
            can_select_many: options.can_select_many,
            drop_mime_types,
            drag_mime_types,
            has_handle_drag,
            has_handle_drop,
            manually_manage_checkboxes: options.manage_checkbox_state_manually,
        };
        let register_promise = self.proxy.rpc_register_tree_view_data_provider(view_id, proxy_options);
        let views = self.this.clone();
        let view = Rc::new_cyclic(|this| TreeViewHandle {
            this: this.clone(),
            view_id: view_id.to_owned(),
            tree_view,
            extension: extension.clone(),
            views,
            register_promise,
        });
        self.store.add(view.clone());
        Ok(view)
    }

    fn create_ext_host_tree_view(&self, id: &str, options: &TreeViewOptions, extension: &ExtensionDescription) -> Rc<ExtHostTreeView> {
        let tree_view = ExtHostTreeView::new(id, options, self.proxy.clone(), self.commands.converter(), self.log_service.clone(), extension.clone());
        self.store.add(tree_view.clone());
        self.tree_views.borrow_mut().insert(id.to_owned(), tree_view.clone());
        tree_view
    }

    /// `_addAdditionalTransferItems(...)`: junta à transferência os itens originais da operação de arrastar em andamento
    /// (ou, se o `drop` chegou antes de alguém pedir, monta a transferência do `handleDrag` e a guarda).
    fn add_additional_transfer_items(
        &self,
        tree_data_transfer: Rc<DataTransfer>,
        tree_view: Rc<ExtHostTreeView>,
        source_tree_item_handles: Vec<String>,
        token: CancellationToken,
        operation_uuid: Option<String>,
    ) -> Promise<Option<Rc<DataTransfer>>> {
        let service = self.tree_drag_and_drop_service.clone();
        async_fn(async move {
            let existing_transfer_operation = service.remove_drag_operation_transfer(operation_uuid.as_deref());
            if let Some(existing_transfer_operation) = existing_transfer_operation {
                if let Some(existing_transfer) = existing_transfer_operation.await? {
                    existing_transfer.for_each(|value, key, _| tree_data_transfer.set(key, value.clone()));
                }
            } else if let Some(operation_uuid) = operation_uuid.as_deref().filter(|uuid| !uuid.is_empty()) {
                // `treeView.handleDrag` é um método do `ExtHostTreeView`, sempre definido: só o `operationUuid` decide.
                let will_drop_promise = tree_view.handle_drag(&source_tree_item_handles, &tree_data_transfer, &token);
                service.add_drag_operation_transfer(operation_uuid, will_drop_promise.clone());
                will_drop_promise.await?;
            }
            Ok(Some(tree_data_transfer))
        })
    }

    /// `_convertArgument(arg)`.
    fn convert_argument(&self, arg: &Dynamic) -> Dynamic {
        let tree_view = match arg.get_str("$treeViewId") {
            Dynamic::String(id) => self.tree_view(&id.to_string_lossy()),
            _ => None,
        };
        if let Some(tree_view) = &tree_view {
            if let Dynamic::String(handle) = arg.get_str("$treeItemHandle") {
                if !handle.is_empty() {
                    return tree_view.get_extension_element(&handle.to_string_lossy()).unwrap_or(Dynamic::Undefined);
                }
            } else if arg.get_str("$treeItemHandle").is_truthy() {
                return Dynamic::Undefined;
            }
            if arg.get_str("$focusedTreeItem").is_truthy() {
                return tree_view.focused_element().unwrap_or(Dynamic::Undefined);
            }
        }
        Dynamic::Null
    }
}

impl IDisposable for ExtHostTreeViews {
    fn dispose(&self) {
        self.store.dispose();
    }
}

impl ExtHostTreeViewsShape for ExtHostTreeViews {
    fn rpc_get_children(&self, tree_view_id: &str, tree_item_handles: Option<Vec<String>>) -> Promise<Option<Vec<TreeChildrenBatch>>> {
        let this = self.rc();
        let tree_view_id = tree_view_id.to_owned();
        async_fn_maybe(async move {
            let Some(tree_view) = this.tree_view(&tree_view_id) else {
                return Ok(MaybePromise::Promise(Promise::rejected(no_tree_view_error(&tree_view_id))));
            };
            let Some(tree_item_handles) = tree_item_handles else {
                let children = tree_view.get_children(None).await?;
                return Ok(MaybePromise::Value(children.map(|children| vec![TreeChildrenBatch { index: 0, children }])));
            };
            // Keep order of treeItemHandles in case extension trees already depend on this
            let mut result = Vec::new();
            for (index, tree_item_handle) in tree_item_handles.iter().enumerate() {
                let children = tree_view.get_children(Some(tree_item_handle)).await?;
                if let Some(children) = children {
                    result.push(TreeChildrenBatch { index, children });
                }
            }
            Ok(MaybePromise::Value(Some(result)))
        })
    }

    #[allow(clippy::too_many_arguments)]
    fn rpc_handle_drop(
        &self,
        destination_view_id: &str,
        request_id: f64,
        tree_data_transfer_dto: DataTransferDto,
        target_item_handle: Option<String>,
        token: CancellationToken,
        operation_uuid: Option<String>,
        source_view_id: Option<String>,
        source_tree_item_handles: Option<Vec<String>>,
    ) -> Promise<()> {
        let this = self.rc();
        let destination_view_id = destination_view_id.to_owned();
        async_fn_maybe(async move {
            let Some(tree_view) = this.tree_view(&destination_view_id) else {
                return Ok(MaybePromise::Promise(Promise::rejected(no_tree_view_error(&destination_view_id))));
            };

            let (proxy, destination) = (this.proxy.clone(), destination_view_id.clone());
            let resolve_file_data: ResolveFileData = Rc::new(move |data_item_index: &str| {
                let (proxy, destination, data_item_index) = (proxy.clone(), destination.clone(), data_item_index.to_owned());
                async_fn(async move { Ok(proxy.rpc_resolve_drop_file_data(&destination, request_id, &data_item_index).await?.to_vec()) })
            });
            let tree_data_transfer = type_converters::DataTransfer::to_data_transfer(&tree_data_transfer_dto, &resolve_file_data);
            if source_view_id.as_deref() == Some(destination_view_id.as_str())
                && let Some(source_tree_item_handles) = source_tree_item_handles
            {
                this.add_additional_transfer_items(tree_data_transfer.clone(), tree_view.clone(), source_tree_item_handles, token.clone(), operation_uuid).await?;
            }
            Ok(MaybePromise::Promise(tree_view.on_drop(&tree_data_transfer, target_item_handle.as_deref(), &token)))
        })
    }

    fn rpc_handle_drag(&self, source_view_id: &str, source_tree_item_handles: Vec<String>, operation_uuid: &str, token: CancellationToken) -> Promise<Option<DataTransferDto>> {
        let this = self.rc();
        let (source_view_id, operation_uuid) = (source_view_id.to_owned(), operation_uuid.to_owned());
        async_fn_maybe(async move {
            let Some(tree_view) = this.tree_view(&source_view_id) else {
                return Ok(MaybePromise::Promise(Promise::rejected(no_tree_view_error(&source_view_id))));
            };

            let attempt: Result<Option<DataTransferDto>, Error> = async {
                let tree_data_transfer = this
                    .add_additional_transfer_items(DataTransfer::new(Vec::new()), tree_view.clone(), source_tree_item_handles, token.clone(), Some(operation_uuid.clone()))
                    .await?;
                let Some(tree_data_transfer) = tree_data_transfer else {
                    this.rpc_release_drag_operation(&operation_uuid);
                    return Ok(None);
                };
                if token.is_cancellation_requested() {
                    this.rpc_release_drag_operation(&operation_uuid);
                    return Ok(None);
                }
                Ok(Some(type_converters::DataTransfer::from(&tree_data_transfer).await?))
            }
            .await;
            match attempt {
                Ok(result) => Ok(MaybePromise::Value(result)),
                Err(error) => {
                    this.rpc_release_drag_operation(&operation_uuid);
                    Err(error)
                }
            }
        })
    }

    fn rpc_release_drag_operation(&self, operation_uuid: &str) {
        self.tree_drag_and_drop_service.remove_drag_operation_transfer(Some(operation_uuid));
    }

    fn rpc_has_resolve(&self, tree_view_id: &str) -> Promise<bool> {
        let tree_view = self.tree_view(tree_view_id);
        let tree_view_id = tree_view_id.to_owned();
        async_fn(async move {
            match tree_view {
                Some(tree_view) => Ok(tree_view.has_resolve()),
                None => Err(no_tree_view_error(&tree_view_id)),
            }
        })
    }

    fn rpc_resolve(&self, tree_view_id: &str, tree_item_handle: &str, token: CancellationToken) -> Promise<Option<ITreeItem>> {
        let Some(tree_view) = self.tree_view(tree_view_id) else {
            errors::throw(no_tree_view_error(tree_view_id));
        };
        tree_view.resolve_tree_item(tree_item_handle, token)
    }

    fn rpc_set_expanded(&self, tree_view_id: &str, tree_item_handle: &str, expanded: bool) {
        let Some(tree_view) = self.tree_view(tree_view_id) else {
            errors::throw(no_tree_view_error(tree_view_id));
        };
        tree_view.set_expanded(tree_item_handle, expanded);
    }

    fn rpc_set_selection_and_focus(&self, tree_view_id: &str, selected_handles: Vec<String>, focused_handle: &str) {
        let Some(tree_view) = self.tree_view(tree_view_id) else {
            errors::throw(no_tree_view_error(tree_view_id));
        };
        tree_view.set_selection_and_focus(selected_handles, focused_handle);
    }

    fn rpc_set_visible(&self, tree_view_id: &str, is_visible: bool) {
        let Some(tree_view) = self.tree_view(tree_view_id) else {
            if !is_visible {
                return;
            }
            errors::throw(no_tree_view_error(tree_view_id));
        };
        tree_view.set_visible(is_visible);
    }

    fn rpc_change_checkbox_state(&self, tree_view_id: &str, checkbox_update: Vec<CheckboxUpdate>) {
        let Some(tree_view) = self.tree_view(tree_view_id) else {
            errors::throw(no_tree_view_error(tree_view_id));
        };
        tree_view.set_checkbox_state(checkbox_update);
    }
}

// ---- a TreeView da API ---------------------------------------------------------------------------------

/// A `vscode.TreeView<T>` que `createTreeView` devolve.
pub struct TreeViewHandle {
    this: Weak<TreeViewHandle>,
    view_id: String,
    tree_view: Rc<ExtHostTreeView>,
    extension: ExtensionDescription,
    views: Weak<ExtHostTreeViews>,
    register_promise: Promise<()>,
}

impl TreeViewHandle {
    /// `onDidCollapseElement`.
    pub fn on_did_collapse_element(&self) -> Event<TreeViewExpansionEvent> {
        self.tree_view.on_did_collapse_element.event()
    }

    /// `onDidExpandElement`.
    pub fn on_did_expand_element(&self) -> Event<TreeViewExpansionEvent> {
        self.tree_view.on_did_expand_element.event()
    }

    /// `selection`.
    pub fn selection(&self) -> Vec<Dynamic> {
        self.tree_view.selected_elements()
    }

    /// `onDidChangeSelection`.
    pub fn on_did_change_selection(&self) -> Event<TreeViewSelectionChangeEvent> {
        self.tree_view.on_did_change_selection.event()
    }

    /// `activeItem` (proposta `treeViewActiveItem`).
    pub fn active_item(&self) -> Result<Option<Dynamic>, Error> {
        check_proposed_api_enabled(&self.extension, "treeViewActiveItem")?;
        Ok(self.tree_view.focused_element())
    }

    /// `onDidChangeActiveItem` (proposta `treeViewActiveItem`).
    pub fn on_did_change_active_item(&self) -> Result<Event<TreeViewActiveItemChangeEvent>, Error> {
        check_proposed_api_enabled(&self.extension, "treeViewActiveItem")?;
        Ok(self.tree_view.on_did_change_active_item.event())
    }

    /// `visible`.
    pub fn visible(&self) -> bool {
        self.tree_view.visible.get()
    }

    /// `onDidChangeVisibility`.
    pub fn on_did_change_visibility(&self) -> Event<TreeViewVisibilityChangeEvent> {
        self.tree_view.on_did_change_visibility.event()
    }

    /// `onDidChangeCheckboxState`.
    pub fn on_did_change_checkbox_state(&self) -> Event<TreeCheckboxChangeEvent> {
        self.tree_view.on_did_change_checkbox_state.event()
    }

    /// `message`.
    pub fn message(&self) -> Documentation {
        self.tree_view.message.borrow().clone()
    }

    /// `message = message` (a mensagem em markdown pede a proposta `treeViewMarkdownMessage`).
    pub fn set_message(&self, message: Documentation) -> Result<(), Error> {
        if matches!(message, Documentation::Markdown(_)) {
            check_proposed_api_enabled(&self.extension, "treeViewMarkdownMessage")?;
        }
        self.tree_view.set_message(message);
        Ok(())
    }

    /// `title`.
    pub fn title(&self) -> String {
        self.tree_view.title.borrow().clone()
    }

    /// `title = title`.
    pub fn set_title(&self, title: &str) {
        self.tree_view.set_title(title);
    }

    /// `description`.
    pub fn description(&self) -> Option<String> {
        self.tree_view.description.borrow().clone()
    }

    /// `description = description`.
    pub fn set_description(&self, description: Option<&str>) {
        self.tree_view.set_description(description);
    }

    /// `badge`.
    pub fn badge(&self) -> Option<ViewBadge> {
        self.tree_view.badge.borrow().clone()
    }

    /// `badge = badge`: o valor é arredondado para um inteiro não negativo (`Math.floor(Math.abs(value))`).
    pub fn set_badge(&self, badge: Option<ViewBadge>) {
        self.tree_view.set_badge(badge.map(|badge| ViewBadge { value: badge.value.abs().floor(), tooltip: badge.tooltip }));
    }

    /// `reveal(element, options?)`.
    pub fn reveal(&self, element: Dynamic, options: Option<IRevealOptions>) -> Promise<()> {
        self.tree_view.reveal(element, options)
    }

    /// O `dispose` assíncrono do original: espera o registro na thread principal terminar, e só avisa a thread principal
    /// se a visão não foi substituída por um novo registro do mesmo id.
    pub fn dispose_async(&self) -> Promise<()> {
        let handle = self.this.upgrade().expect("o TreeViewHandle está vivo enquanto um método dele roda");
        async_fn(async move {
            // Wait for the registration promise to finish before doing the dispose.
            handle.register_promise.clone().await?;
            let Some(views) = handle.views.upgrade() else {
                return Ok(());
            };
            // Only notify the main thread if this view was not replaced by a new registration.
            // When an extension disposes a view and immediately re-registers it, the new
            // registration may have already updated _treeViews before this async dispose runs.
            let current = views.tree_view(&handle.view_id);
            if current.is_some_and(|current| Rc::ptr_eq(&current, &handle.tree_view)) {
                views.tree_views.borrow_mut().remove(&handle.view_id);
                views.proxy.rpc_dispose_tree(&handle.view_id);
            }
            let view: DisposableRef = handle.clone();
            views.store.delete_and_leak(&view);
            mark_as_disposed(id_of(&handle));
            let tree_view: DisposableRef = handle.tree_view.clone();
            views.store.delete(&tree_view);
            Ok(())
        })
    }
}

impl IDisposable for TreeViewHandle {
    fn dispose(&self) {
        self.dispose_async();
    }
}

// ---- ExtHostTreeView ------------------------------------------------------------------------------------

/// `TreeData<T>`: `{ message: boolean; element: T | T[] | Root | false }` (o `false` é `None`).
#[derive(Clone)]
struct TreeData {
    message: bool,
    element: Option<Dynamic>,
}

/// O resultado do debounce: `{ message: boolean; elements: (T | Root)[] }`.
#[derive(Clone)]
struct DebouncedChange {
    message: bool,
    elements: Vec<Dynamic>,
}

/// `TreeNode`.
struct TreeNode {
    item: RefCell<ITreeItem>,
    extension_item: Dynamic,
    parent: Option<Rc<TreeNode>>,
    children: RefCell<Option<Vec<Rc<TreeNode>>>>,
    disposable_store: Rc<DisposableStore>,
}

impl TreeNode {
    fn handle(&self) -> String {
        self.item.borrow().handle.clone()
    }

    fn has_handle(&self, handle: &str) -> bool {
        self.item.borrow().handle == handle
    }
}

impl IDisposable for TreeNode {
    fn dispose(&self) {
        self.disposable_store.dispose();
    }
}

/// `Set<TreeNode>`: na ordem de inserção, por identidade.
#[derive(Default)]
struct NodeSet {
    nodes: Vec<Rc<TreeNode>>,
    ids: HashSet<usize>,
}

impl NodeSet {
    fn add(&mut self, node: &Rc<TreeNode>) {
        if self.ids.insert(Rc::as_ptr(node) as usize) {
            self.nodes.push(node.clone());
        }
    }

    fn take_all(&mut self) -> Vec<Rc<TreeNode>> {
        self.ids.clear();
        std::mem::take(&mut self.nodes)
    }
}

/// O item que `setCheckboxState` junta (`CheckboxUpdateWithItem`).
#[derive(Clone)]
struct CheckboxUpdateWithItem {
    extension_item: Dynamic,
    tree_item: Dynamic,
    new_state: TreeItemCheckboxState,
}

fn register_emitter<T: 'static>(base: &Disposable) -> Rc<Emitter<T>> {
    let emitter: Rc<Emitter<T>> = Emitter::new();
    base.register(emitter.clone());
    emitter
}

struct ExtHostTreeView {
    this: Weak<ExtHostTreeView>,
    base: Rc<Disposable>,
    view_id: String,
    proxy: Rc<dyn MainThreadTreeViewsShape>,
    commands: Rc<dyn ICommandsConverter>,
    log_service: LogServiceRef,
    extension: ExtensionDescription,
    data_provider: Rc<TreeDataProvider>,
    dnd_controller: Option<Rc<TreeDragAndDropController>>,

    roots: RefCell<Option<Vec<Rc<TreeNode>>>>,
    elements: RefCell<HashMap<TreeItemHandle, Dynamic>>,
    nodes: RefCell<ElementMap<Rc<TreeNode>>>,
    /// Track the latest child-fetch per element so that refresh-triggered cache clears ignore stale results.
    /// Without these tokens, an earlier getChildren promise resolving after refresh would re-register handles and hit the
    /// duplicate-id guard. A chave `ROOT_FETCH_KEY` do original é `root_fetch_token`.
    children_fetch_tokens: RefCell<ElementMap<u64>>,
    root_fetch_token: Cell<Option<u64>>,
    /// Global counter for fetch tokens. Using a monotonically increasing counter ensures that even after
    /// `_childrenFetchTokens.clear()` during a root refresh, old in-flight fetches will have requestIds that
    /// can never match new fetches.
    global_fetch_token_counter: Cell<u64>,

    visible: Cell<bool>,
    selected_handles: RefCell<Vec<TreeItemHandle>>,
    focused_handle: RefCell<Option<TreeItemHandle>>,

    on_did_expand_element: Rc<Emitter<TreeViewExpansionEvent>>,
    on_did_collapse_element: Rc<Emitter<TreeViewExpansionEvent>>,
    on_did_change_selection: Rc<Emitter<TreeViewSelectionChangeEvent>>,
    on_did_change_active_item: Rc<Emitter<TreeViewActiveItemChangeEvent>>,
    on_did_change_visibility: Rc<Emitter<TreeViewVisibilityChangeEvent>>,
    on_did_change_checkbox_state: Rc<Emitter<TreeCheckboxChangeEvent>>,
    on_did_change_data: Rc<Emitter<TreeData>>,

    refresh_promise: RefCell<Promise<()>>,
    refresh_queue: RefCell<Promise<()>>,
    nodes_to_clear: RefCell<NodeSet>,

    message: RefCell<Documentation>,
    title: RefCell<String>,
    description: RefCell<Option<String>>,
    badge: RefCell<Option<ViewBadge>>,

    refresh_cancellation_source: RefCell<CancellationTokenSource>,
}

const LABEL_HANDLE_PREFIX: &str = "0";
const ID_HANDLE_PREFIX: &str = "1";

impl ExtHostTreeView {
    fn new(
        view_id: &str,
        options: &TreeViewOptions,
        proxy: Rc<dyn MainThreadTreeViewsShape>,
        commands: Rc<dyn ICommandsConverter>,
        log_service: LogServiceRef,
        extension: ExtensionDescription,
    ) -> Rc<ExtHostTreeView> {
        let base = Disposable::new();
        let mut title = String::new();
        if let Some(contributes) = &extension.manifest.contributes {
            for views in contributes.views.values() {
                for view in views {
                    if view.id == view_id {
                        title = view.name.clone();
                    }
                }
            }
        }

        let tree_view = Rc::new_cyclic(|this| ExtHostTreeView {
            this: this.clone(),
            view_id: view_id.to_owned(),
            proxy,
            commands,
            log_service,
            extension,
            data_provider: options.tree_data_provider.clone(),
            dnd_controller: options.drag_and_drop_controller.clone(),
            roots: RefCell::new(None),
            elements: RefCell::new(HashMap::new()),
            nodes: RefCell::new(ElementMap::new()),
            children_fetch_tokens: RefCell::new(ElementMap::new()),
            root_fetch_token: Cell::new(None),
            global_fetch_token_counter: Cell::new(0),
            visible: Cell::new(false),
            selected_handles: RefCell::new(Vec::new()),
            focused_handle: RefCell::new(None),
            on_did_expand_element: register_emitter(&base),
            on_did_collapse_element: register_emitter(&base),
            on_did_change_selection: register_emitter(&base),
            on_did_change_active_item: register_emitter(&base),
            on_did_change_visibility: register_emitter(&base),
            on_did_change_checkbox_state: register_emitter(&base),
            on_did_change_data: register_emitter(&base),
            refresh_promise: RefCell::new(Promise::resolved(())),
            refresh_queue: RefCell::new(Promise::resolved(())),
            nodes_to_clear: RefCell::new(NodeSet::default()),
            message: RefCell::new(Documentation::Text(String::new())),
            title: RefCell::new(title),
            description: RefCell::new(None),
            badge: RefCell::new(None),
            refresh_cancellation_source: RefCell::new(CancellationTokenSource::new(None)),
            base,
        });
        tree_view.subscribe_to_data_changes();
        tree_view
    }

    /// O resto do construtor: a inscrição em `onDidChangeTreeData` e o debounce das mudanças.
    fn subscribe_to_data_changes(self: &Rc<Self>) {
        if let Some(on_did_change_tree_data) = &self.data_provider.on_did_change_tree_data {
            let this = self.this.clone();
            self.base.register(on_did_change_tree_data.listen(move |element_or_elements| {
                if let Dynamic::Array(elements) = element_or_elements
                    && elements.is_empty()
                {
                    return;
                }
                if let Some(this) = this.upgrade() {
                    this.on_did_change_data.fire(TreeData { message: false, element: Some(element_or_elements.clone()) });
                }
            }));
        }

        let refreshing_promise: Rc<RefCell<Option<Promise<()>>>> = Rc::default();
        let promise_callback: Rc<RefCell<Option<Resolver<()>>>> = Rc::default();
        let on_did_change_data = {
            let (this, refreshing_promise, promise_callback) = (self.this.clone(), refreshing_promise.clone(), promise_callback.clone());
            self.on_did_change_data.event().debounce(
                move |result: Option<DebouncedChange>, current: &TreeData| {
                    let mut result = result.unwrap_or(DebouncedChange { message: false, elements: Vec::new() });
                    if let Some(element) = &current.element {
                        if refreshing_promise.borrow().is_none() {
                            // New refresh has started
                            let (promise, resolver) = Promise::new();
                            *refreshing_promise.borrow_mut() = Some(promise);
                            *promise_callback.borrow_mut() = Some(resolver);
                            if let Some(this) = this.upgrade() {
                                let previous = this.refresh_promise.borrow().clone();
                                let refreshing_promise = refreshing_promise.clone();
                                *this.refresh_promise.borrow_mut() = previous.then_maybe(move |_| match refreshing_promise.borrow().clone() {
                                    Some(promise) => MaybePromise::Promise(promise),
                                    None => MaybePromise::Value(()),
                                });
                            }
                        }
                        match element {
                            Dynamic::Array(elements) => result.elements.extend(elements.to_vec()),
                            other => result.elements.push(other.clone()),
                        }
                    }
                    if current.message {
                        result.message = true;
                    }
                    result
                },
                DebounceOptions { delay: Delay::Millis(200), leading: true, ..DebounceOptions::default() },
                None,
            )
        };
        let this = self.this.clone();
        self.base.register(on_did_change_data.listen(move |change: &DebouncedChange| {
            let Some(this) = this.upgrade() else {
                return;
            };
            if !change.elements.is_empty() {
                let elements = Rc::new(distinct(&change.elements, element_key));
                let previous = this.refresh_queue.borrow().clone();
                let (queue_this, refreshing_promise, promise_callback) = (this.clone(), refreshing_promise.clone(), promise_callback.clone());
                *this.refresh_queue.borrow_mut() = previous.then_promise(move |_| {
                    let promise_callback = promise_callback.borrow().clone();
                    *refreshing_promise.borrow_mut() = None;
                    let children_to_clear = Rc::new(queue_this.nodes_to_clear.borrow_mut().take_all());
                    queue_this.debug_log_refresh("start", &elements, &children_to_clear);
                    let (done_this, done_elements, done_children, done_callback) = (queue_this.clone(), elements.clone(), children_to_clear.clone(), promise_callback.clone());
                    let (error_this, error_elements, error_children) = (queue_this.clone(), elements.clone(), children_to_clear.clone());
                    queue_this
                        .refresh(&elements)
                        .then(move |_| {
                            done_this.debug_log_refresh("done", &done_elements, &done_children);
                            done_this.clear_nodes(&done_children);
                            if let Some(callback) = &done_callback {
                                callback.resolve(());
                            }
                        })
                        .catch(move |error| {
                            let message = error.message.clone();
                            error_this.debug_log_refresh("error", &error_elements, &error_children);
                            error_this.clear_nodes(&error_children);
                            error_this.log_service.error(&format!("Unable to refresh tree view {}: {message}", error_this.view_id));
                            if let Some(callback) = &promise_callback {
                                callback.resolve(());
                            }
                        })
                });
            }
            if change.message {
                let current_message = this.message.borrow().clone();
                let message = type_converters::MarkdownString::from_strict(Some(&current_message)).unwrap_or_else(|| StringOrMarkdown::String(String::new()));
                this.proxy.rpc_set_message(&this.view_id, message);
            }
        }));
    }

    fn debug_collect_handles(&self, elements: &[Dynamic]) -> (Vec<String>, Vec<String>) {
        let mut changed = Vec::new();
        for element in elements {
            if !element.is_truthy() {
                changed.push("<root>".to_owned());
                continue;
            }
            if let Some(node) = self.nodes.borrow().get(element) {
                changed.push(node.handle());
            }
        }
        let roots = self.roots.borrow().as_ref().map(|roots| roots.iter().map(|root| root.handle()).collect()).unwrap_or_default();
        (changed, roots)
    }

    fn debug_log_refresh(&self, phase: &str, elements: &[Dynamic], children_to_clear: &[Rc<TreeNode>]) {
        if !self.is_debug_logging() {
            return;
        }
        let strings = |handles: &[String]| Dynamic::array(handles.iter().map(|handle| Dynamic::string(handle)));
        let message = errors::catch(|| {
            let (changed, roots) = self.debug_collect_handles(elements);
            let clearing: Vec<String> = children_to_clear.iter().map(|node| node.handle()).collect();
            let snapshot = Dynamic::object([("changed", strings(&changed)), ("roots", strings(&roots)), ("clearing", strings(&clearing))]);
            format!(
                "[TreeView:{}] refresh {phase} changed={} nodesToClear={} elements.size={} nodes.size={} handles={}",
                self.view_id,
                changed.len(),
                children_to_clear.len(),
                self.elements.borrow().len(),
                self.nodes.borrow().len(),
                json_stringify(&snapshot).unwrap_or_default()
            )
        })
        .unwrap_or_else(|_| format!("[TreeView:{}] refresh {phase} (snapshot failed)", self.view_id));
        self.log_service.debug(&message);
    }

    fn is_debug_logging(&self) -> bool {
        match errors::catch(|| self.log_service.get_level()) {
            Ok(level) => level == LogLevel::Debug || level == LogLevel::Trace,
            Err(_) => false,
        }
    }

    // ---- consulta ---------------------------------------------------------------------------------

    /// `getChildren(parentHandle)`.
    fn get_children(self: &Rc<Self>, parent_handle: Option<&str>) -> Promise<Option<Vec<ITreeItem>>> {
        let this = self.clone();
        let parent_handle = parent_handle.filter(|handle| !handle.is_empty()).map(str::to_owned);
        async_fn_maybe(async move {
            let parent_element = parent_handle.as_deref().and_then(|handle| this.get_extension_element(handle)).filter(|element| element.is_truthy());
            if let Some(parent_handle) = &parent_handle
                && parent_element.is_none()
            {
                this.log_service.error(&format!("No tree item with id '{parent_handle}' found."));
                return Ok(MaybePromise::Promise(Promise::resolved(Some(Vec::new()))));
            }

            let mut children_nodes = this.children_nodes_for_handle(parent_handle.as_deref()); // Get it from cache

            if children_nodes.is_none() {
                children_nodes = this.fetch_children_nodes(parent_element).await?;
            }

            Ok(MaybePromise::Value(children_nodes.map(|nodes| nodes.iter().map(|node| node.item.borrow().clone()).collect())))
        })
    }

    /// `getExtensionElement(treeItemHandle)`.
    fn get_extension_element(&self, tree_item_handle: &str) -> Option<Dynamic> {
        self.elements.borrow().get(tree_item_handle).cloned()
    }

    /// `selectedElements`.
    fn selected_elements(&self) -> Vec<Dynamic> {
        let handles = self.selected_handles.borrow().clone();
        handles.iter().filter_map(|handle| self.get_extension_element(handle)).filter(|element| !element.is_nullish()).collect()
    }

    /// `focusedElement`.
    fn focused_element(&self) -> Option<Dynamic> {
        let handle = self.focused_handle.borrow().clone().filter(|handle| !handle.is_empty())?;
        self.get_extension_element(&handle)
    }

    // ---- reveal -----------------------------------------------------------------------------------

    /// `reveal(element, options)`.
    fn reveal(self: &Rc<Self>, element: Dynamic, options: Option<IRevealOptions>) -> Promise<()> {
        let options = options.unwrap_or(IRevealOptions { select: Some(true), focus: Some(false), expand: None });
        let select = options.select.unwrap_or(true);
        let focus = options.focus.unwrap_or(false);
        let expand = options.expand.unwrap_or(RevealExpand::Bool(false));
        let reveal_options = IRevealOptions { select: Some(select), focus: Some(focus), expand: Some(expand) };

        if self.data_provider.get_parent.is_none() {
            return Promise::rejected(Error::new("Required registered TreeDataProvider to implement 'getParent' method to access 'reveal' method"));
        }

        if element.is_truthy() {
            let this = self.clone();
            let refresh_promise = self.refresh_promise.borrow().clone();
            let chain_this = this.clone();
            let chain_element = element.clone();
            refresh_promise.then_promise(move |_| chain_this.resolve_unknown_parent_chain(&chain_element)).then_result_maybe(move |result| match result {
                Ok(parent_chain) => {
                    let reveal_this = this.clone();
                    let parent_chain_items: Vec<Rc<TreeNode>> = parent_chain.clone();
                    MaybePromise::Promise(this.resolve_tree_node(&element, parent_chain.last().cloned()).then_promise(move |tree_node| {
                        let item_info = RevealItemInfo {
                            item: tree_node.item.borrow().clone(),
                            parent_chain: parent_chain_items.iter().map(|parent| parent.item.borrow().clone()).collect(),
                        };
                        reveal_this.proxy.rpc_reveal(&reveal_this.view_id, Some(item_info), reveal_options)
                    }))
                }
                Err(error) => {
                    this.log_service.error_args(&LogMessage::Error(error), &[]);
                    MaybePromise::Value(())
                }
            })
        } else {
            self.proxy.rpc_reveal(&self.view_id, None, reveal_options)
        }
    }

    // ---- mensagem, título, descrição e selo -------------------------------------------------------

    fn set_message(&self, message: Documentation) {
        *self.message.borrow_mut() = message;
        self.on_did_change_data.fire(TreeData { message: true, element: None });
    }

    fn set_title(&self, title: &str) {
        *self.title.borrow_mut() = title.to_owned();
        let description = self.description.borrow().clone();
        self.proxy.rpc_set_title(&self.view_id, title, description.as_deref());
    }

    fn set_description(&self, description: Option<&str>) {
        *self.description.borrow_mut() = description.map(str::to_owned);
        let title = self.title.borrow().clone();
        self.proxy.rpc_set_title(&self.view_id, &title, description);
    }

    fn set_badge(&self, badge: Option<ViewBadge>) {
        {
            let current = self.badge.borrow();
            if current.as_ref().map(|current| current.value) == badge.as_ref().map(|badge| badge.value)
                && current.as_ref().map(|current| &current.tooltip) == badge.as_ref().map(|badge| &badge.tooltip)
            {
                return;
            }
        }

        *self.badge.borrow_mut() = badge.clone();
        self.proxy.rpc_set_badge(&self.view_id, type_converters::ViewBadge::from(badge.as_ref()));
    }

    // ---- eventos vindos da thread principal -------------------------------------------------------

    fn set_expanded(&self, tree_item_handle: &str, expanded: bool) {
        if let Some(element) = self.get_extension_element(tree_item_handle) {
            if expanded {
                self.on_did_expand_element.fire(TreeViewExpansionEvent { element });
            } else {
                self.on_did_collapse_element.fire(TreeViewExpansionEvent { element });
            }
        }
    }

    fn set_selection_and_focus(&self, selected_handles: Vec<TreeItemHandle>, focused_handle: &str) {
        let changed_selection = *self.selected_handles.borrow() != selected_handles;
        *self.selected_handles.borrow_mut() = selected_handles;

        let changed_focus = self.focused_handle.borrow().as_deref() != Some(focused_handle);
        *self.focused_handle.borrow_mut() = Some(focused_handle.to_owned());

        if changed_selection {
            self.on_did_change_selection.fire(TreeViewSelectionChangeEvent { selection: self.selected_elements() });
        }

        if changed_focus {
            self.on_did_change_active_item.fire(TreeViewActiveItemChangeEvent { active_item: self.focused_element() });
        }
    }

    fn set_visible(&self, visible: bool) {
        if visible != self.visible.get() {
            self.visible.set(visible);
            self.on_did_change_visibility.fire(TreeViewVisibilityChangeEvent { visible });
        }
    }

    /// `setCheckboxState(checkboxUpdates)`.
    fn set_checkbox_state(self: &Rc<Self>, checkbox_updates: Vec<CheckboxUpdate>) -> Promise<()> {
        let this = self.clone();
        async_fn(async move {
            let promises: Vec<Promise<Option<CheckboxUpdateWithItem>>> = checkbox_updates
                .into_iter()
                .map(|checkbox_update| {
                    let this = this.clone();
                    async_fn(async move {
                        let extension_item = this.get_extension_element(&checkbox_update.tree_item_handle).filter(|element| element.is_truthy());
                        if let Some(extension_item) = extension_item {
                            let tree_item = await_maybe((this.data_provider.get_tree_item)(&extension_item)).await?;
                            return Ok(Some(CheckboxUpdateWithItem {
                                extension_item,
                                tree_item,
                                new_state: if checkbox_update.new_state { TreeItemCheckboxState::Checked } else { TreeItemCheckboxState::Unchecked },
                            }));
                        }
                        Ok(None)
                    })
                })
                .collect();
            let items: Vec<CheckboxUpdateWithItem> = Promise::all(promises).await?.into_iter().flatten().collect();

            for item in &items {
                if let Dynamic::Object(tree_item) = &item.tree_item {
                    tree_item.set(&JsString::from("checkboxState"), Dynamic::from(item.new_state.value()));
                }
            }

            this.on_did_change_checkbox_state.fire(TreeCheckboxChangeEvent { items: items.iter().map(|item| (item.extension_item.clone(), item.new_state)).collect() });
            Ok(())
        })
    }

    // ---- arrastar e soltar ------------------------------------------------------------------------

    /// `handleDrag(sourceTreeItemHandles, treeDataTransfer, token)`.
    fn handle_drag(self: &Rc<Self>, source_tree_item_handles: &[TreeItemHandle], tree_data_transfer: &Rc<DataTransfer>, token: &CancellationToken) -> Promise<Option<Rc<DataTransfer>>> {
        let this = self.clone();
        let (source_tree_item_handles, tree_data_transfer, token) = (source_tree_item_handles.to_vec(), tree_data_transfer.clone(), token.clone());
        async_fn(async move {
            let mut extension_tree_items = Vec::new();
            for source_handle in &source_tree_item_handles {
                if let Some(extension_item) = this.get_extension_element(source_handle).filter(|element| element.is_truthy()) {
                    extension_tree_items.push(extension_item);
                }
            }

            let Some(handle_drag) = this.dnd_controller.as_ref().and_then(|controller| controller.handle_drag.clone()) else {
                return Ok(None);
            };
            if extension_tree_items.is_empty() {
                return Ok(None);
            }
            await_maybe(handle_drag(&extension_tree_items, &tree_data_transfer, &token)).await?;
            Ok(Some(tree_data_transfer))
        })
    }

    /// `onDrop(treeDataTransfer, targetHandleOrNode, token)`.
    fn on_drop(self: &Rc<Self>, tree_data_transfer: &Rc<DataTransfer>, target_handle: Option<&str>, token: &CancellationToken) -> Promise<()> {
        let this = self.clone();
        let (tree_data_transfer, token) = (tree_data_transfer.clone(), token.clone());
        let target_handle = target_handle.filter(|handle| !handle.is_empty()).map(str::to_owned);
        async_fn_maybe(async move {
            let target = target_handle.as_deref().and_then(|handle| this.get_extension_element(handle));
            let handle_drop = this.dnd_controller.as_ref().and_then(|controller| controller.handle_drop.clone());
            let target_is_known = target.as_ref().is_some_and(|target| target.is_truthy());
            let Some(handle_drop) = handle_drop else {
                return Ok(MaybePromise::Value(()));
            };
            if !target_is_known && target_handle.is_some() {
                return Ok(MaybePromise::Value(()));
            }
            Ok(MaybePromise::Promise(as_promise_maybe(move || handle_drop(target.as_ref(), &tree_data_transfer, &token))))
        })
    }

    // ---- resolução --------------------------------------------------------------------------------

    fn has_resolve(&self) -> bool {
        self.data_provider.resolve_tree_item.is_some()
    }

    /// `resolveTreeItem(treeItemHandle, token)`.
    fn resolve_tree_item(self: &Rc<Self>, tree_item_handle: &str, token: CancellationToken) -> Promise<Option<ITreeItem>> {
        let this = self.clone();
        let tree_item_handle = tree_item_handle.to_owned();
        async_fn(async move {
            let Some(resolve_tree_item) = this.data_provider.resolve_tree_item.clone() else {
                return Ok(None);
            };
            let Some(element) = this.elements.borrow().get(&tree_item_handle).cloned().filter(|element| element.is_truthy()) else {
                return Ok(None);
            };
            let node = this.nodes.borrow().get(&element);
            let Some(node) = node else {
                return Ok(None);
            };
            let resolved = await_maybe(resolve_tree_item(&node.extension_item, &element, &token)).await?;
            let resolve = if resolved.is_nullish() { node.extension_item.clone() } else { resolved };
            this.validate_tree_item(&resolve)?;
            // Resolvable elements. Currently only tooltip and command.
            let tooltip = this.get_tooltip(&resolve.get_str("tooltip"))?;
            let command = this.get_command(&node.disposable_store, &resolve.get_str("command"))?;
            {
                let mut item = node.item.borrow_mut();
                item.tooltip = tooltip;
                item.command = command;
            }
            let item = node.item.borrow().clone();
            Ok(Some(item))
        })
    }

    fn resolve_unknown_parent_chain(self: &Rc<Self>, element: &Dynamic) -> Promise<Vec<Rc<TreeNode>>> {
        let this = self.clone();
        self.resolve_parent(element).then_promise(move |parent| {
            if !parent.is_truthy() {
                return Promise::resolved(Vec::new());
            }
            let (inner_this, inner_parent) = (this.clone(), parent.clone());
            this.resolve_unknown_parent_chain(&parent).then_promise(move |mut result| {
                let last = result.last().cloned();
                inner_this.resolve_tree_node(&inner_parent, last).then(move |parent_node| {
                    result.push(parent_node);
                    result
                })
            })
        })
    }

    fn resolve_parent(&self, element: &Dynamic) -> Promise<Dynamic> {
        let node = self.nodes.borrow().get(element);
        if let Some(node) = node {
            let parent = node.parent.as_ref().and_then(|parent| self.elements.borrow().get(&parent.handle()).cloned()).unwrap_or(Dynamic::Undefined);
            return Promise::resolved(parent);
        }
        let get_parent = self.data_provider.get_parent.clone().expect("o getParent está definido: o reveal confere antes");
        let element = element.clone();
        as_promise_maybe(move || get_parent(&element))
    }

    fn resolve_tree_node(self: &Rc<Self>, element: &Dynamic, parent: Option<Rc<TreeNode>>) -> Promise<Rc<TreeNode>> {
        let this = self.clone();
        let element = element.clone();
        async_fn(async move {
            let existing = this.nodes.borrow().get(&element);
            if let Some(node) = existing {
                return Ok(node);
            }
            let get_tree_item = this.data_provider.get_tree_item.clone();
            let ext_tree_item = as_promise_maybe({
                let element = element.clone();
                move || get_tree_item(&element)
            })
            .await?;
            let handle = this.create_handle(&element, &ext_tree_item, parent.as_ref(), true)?;
            let parent_handle = parent.as_ref().map(|parent| parent.handle());
            this.get_children(parent_handle.as_deref()).await?;
            if let Some(cached_element) = this.get_extension_element(&handle).filter(|element| element.is_truthy()) {
                let node = this.nodes.borrow().get(&cached_element);
                if let Some(node) = node {
                    return Ok(node);
                }
            }
            this.log_service.error(&format!("[TreeView:{}] Failed to resolve tree node for element {handle}", this.view_id));
            this.proxy.rpc_log_resolve_tree_node_failure(&this.extension.identifier.value);
            Err(Error::new(format!("Cannot resolve tree item for element {handle} from extension {}", this.extension.identifier.value)))
        })
    }

    // ---- cache de filhos --------------------------------------------------------------------------

    /// `_getChildrenNodes(handle)`: os filhos em cache do pai com esse handle (ou as raízes, sem handle).
    fn children_nodes_for_handle(&self, parent_handle: Option<&str>) -> Option<Vec<Rc<TreeNode>>> {
        match parent_handle.filter(|handle| !handle.is_empty()) {
            Some(handle) => {
                let parent_element = self.get_extension_element(handle).filter(|element| element.is_truthy());
                let parent_node = parent_element.and_then(|element| self.nodes.borrow().get(&element));
                parent_node.and_then(|node| node.children.borrow().clone())
            }
            None => self.roots.borrow().clone(),
        }
    }

    /// `(this._getChildrenNodes(parent) || []).length`.
    fn children_len(&self, parent: Option<&Rc<TreeNode>>) -> usize {
        match parent {
            Some(parent) => parent.children.borrow().as_ref().map_or(0, Vec::len),
            None => self.roots.borrow().as_ref().map_or(0, Vec::len),
        }
    }

    fn next_fetch_token(&self, parent_element: Option<&Dynamic>) -> u64 {
        let request_id = self.global_fetch_token_counter.get() + 1;
        self.global_fetch_token_counter.set(request_id);
        match parent_element {
            Some(parent_element) => self.children_fetch_tokens.borrow_mut().insert(parent_element, request_id),
            None => self.root_fetch_token.set(Some(request_id)),
        }
        request_id
    }

    fn fetch_token(&self, parent_element: Option<&Dynamic>) -> Option<u64> {
        match parent_element {
            Some(parent_element) => self.children_fetch_tokens.borrow().get(parent_element),
            None => self.root_fetch_token.get(),
        }
    }

    fn fetch_children_nodes(self: &Rc<Self>, parent_element: Option<Dynamic>) -> Promise<Option<Vec<Rc<TreeNode>>>> {
        let this = self.clone();
        async_fn(async move {
            // clear children cache
            this.add_children_to_clear(parent_element.as_ref());
            let request_id = this.next_fetch_token(parent_element.as_ref());

            let cts = CancellationTokenSource::new(Some(&this.refresh_cancellation_source.borrow().token()));

            let result = this.fetch_children_nodes_with(parent_element.as_ref(), request_id, &cts).await;
            cts.dispose();
            result
        })
    }

    async fn fetch_children_nodes_with(self: &Rc<Self>, parent_element: Option<&Dynamic>, request_id: u64, cts: &CancellationTokenSource) -> Result<Option<Vec<Rc<TreeNode>>>, Error> {
        let get_children = self.data_provider.get_children.clone();
        let elements = await_maybe(get_children(parent_element)).await?;
        if self.fetch_token(parent_element) != Some(request_id) {
            return Ok(None);
        }
        let parent_node = parent_element.filter(|element| element.is_truthy()).and_then(|element| self.nodes.borrow().get(element));

        if cts.token().is_cancellation_requested() {
            return Ok(None);
        }

        let coalesced_elements: Vec<Dynamic> = elements.unwrap_or_default().into_iter().filter(|element| element.is_truthy()).collect();
        let get_tree_item = self.data_provider.get_tree_item.clone();
        let tree_items = Promise::all(coalesced_elements.iter().map(|element| maybe_to_promise(get_tree_item(element))).collect()).await?;
        if self.fetch_token(parent_element) != Some(request_id) {
            return Ok(None);
        }
        if cts.token().is_cancellation_requested() {
            return Ok(None);
        }

        // createAndRegisterTreeNodes adds the nodes to a cache. This must be done sync so that they get added in the correct order.
        let mut items = Vec::with_capacity(tree_items.len());
        for (index, item) in tree_items.iter().enumerate() {
            items.push(if item.is_truthy() { Some(self.create_and_register_tree_node(&coalesced_elements[index], item, parent_node.as_ref())?) } else { None });
        }
        if self.fetch_token(parent_element) != Some(request_id) {
            return Ok(None);
        }

        Ok(Some(items.into_iter().flatten().collect()))
    }

    // ---- refresh ----------------------------------------------------------------------------------

    fn refresh(self: &Rc<Self>, elements: &[Dynamic]) -> Promise<()> {
        let has_root = elements.iter().any(|element| !element.is_truthy());
        if has_root {
            // Cancel any pending children fetches
            self.refresh_cancellation_source.borrow().dispose_with(true);
            *self.refresh_cancellation_source.borrow_mut() = CancellationTokenSource::new(None);

            self.add_children_to_clear(None);
            return self.proxy.rpc_refresh(&self.view_id, None);
        }
        let handles_to_refresh = self.get_handles_to_refresh(elements);
        if !handles_to_refresh.is_empty() {
            return self.refresh_handles(handles_to_refresh);
        }
        Promise::resolved(())
    }

    fn get_handles_to_refresh(&self, elements: &[Dynamic]) -> Vec<TreeItemHandle> {
        let mut elements_to_update: Vec<TreeItemHandle> = Vec::new();
        let element_nodes: Vec<Option<Rc<TreeNode>>> = elements.iter().map(|element| self.nodes.borrow().get(element)).collect();
        for element_node in element_nodes.iter().flatten() {
            if elements_to_update.contains(&element_node.handle()) {
                continue;
            }
            // check if an ancestor of extElement is already in the elements list
            let mut current_node = Some(element_node.clone());
            while let Some(current) = &current_node {
                let Some(current_parent) = &current.parent else {
                    break;
                };
                let parent_handle = current_parent.handle();
                if element_nodes.iter().flatten().any(|node| node.has_handle(&parent_handle)) {
                    break;
                }
                let parent_element = self.elements.borrow().get(&parent_handle).cloned().filter(|element| element.is_truthy());
                current_node = parent_element.and_then(|element| self.nodes.borrow().get(&element));
            }
            if current_node.as_ref().is_some_and(|current| current.parent.is_none()) {
                elements_to_update.push(element_node.handle());
            }
        }

        let mut handles_to_update = Vec::new();
        // Take only top level elements
        for handle in &elements_to_update {
            let element = self.elements.borrow().get(handle).cloned().filter(|element| element.is_truthy());
            if let Some(element) = element {
                let node = self.nodes.borrow().get(&element);
                if let Some(node) = node
                    && node.parent.as_ref().is_none_or(|parent| !elements_to_update.contains(&parent.handle()))
                {
                    handles_to_update.push(handle.clone());
                }
            }
        }

        handles_to_update
    }

    fn refresh_handles(self: &Rc<Self>, item_handles: Vec<TreeItemHandle>) -> Promise<()> {
        let items_to_refresh: Rc<RefCell<ItemsToRefresh>> = Rc::default();
        let promises: Vec<Promise<()>> = item_handles
            .into_iter()
            .map(|tree_item_handle| {
                let items_to_refresh = items_to_refresh.clone();
                let handle = tree_item_handle.clone();
                self.refresh_node(&tree_item_handle).then(move |node| {
                    if let Some(node) = node {
                        items_to_refresh.borrow_mut().push((handle, node.item.borrow().clone()));
                    }
                })
            })
            .collect();
        let this = self.clone();
        Promise::all(promises).then_maybe(move |_| {
            let items = items_to_refresh.borrow().clone();
            if items.is_empty() { MaybePromise::Value(()) } else { MaybePromise::Promise(this.proxy.rpc_refresh(&this.view_id, Some(items))) }
        })
    }

    fn refresh_node(self: &Rc<Self>, tree_item_handle: &str) -> Promise<Option<Rc<TreeNode>>> {
        let ext_element = self.get_extension_element(tree_item_handle).filter(|element| element.is_truthy());
        if let Some(ext_element) = ext_element {
            let existing = self.nodes.borrow().get(&ext_element);
            if let Some(existing) = existing {
                self.add_children_to_clear(Some(&ext_element)); // clear children cache
                let this = self.clone();
                let get_tree_item = self.data_provider.get_tree_item.clone();
                let element = ext_element.clone();
                return as_promise_maybe(move || get_tree_item(&element)).then_result(move |result| {
                    let ext_tree_item = result?;
                    if ext_tree_item.is_truthy() {
                        let new_node = this.create_tree_node(&ext_element, &ext_tree_item, existing.parent.as_ref())?;
                        this.update_node_cache(&ext_element, &new_node, &existing, existing.parent.as_ref());
                        existing.dispose();
                        return Ok(Some(new_node));
                    }
                    Ok(None)
                });
            }
        }
        Promise::resolved(None)
    }

    // ---- criação dos nós --------------------------------------------------------------------------

    fn create_and_register_tree_node(&self, element: &Dynamic, ext_tree_item: &Dynamic, parent_node: Option<&Rc<TreeNode>>) -> Result<Rc<TreeNode>, Error> {
        let id = ext_tree_item.get_str("id");
        let duplicate_handle = if id.is_truthy() { Some(format!("{ID_HANDLE_PREFIX}/{}", js_to_string(&id))) } else { None };
        if let Some(duplicate_handle) = duplicate_handle {
            let existing_element = self.elements.borrow().get(&duplicate_handle).cloned().filter(|element| element.is_truthy());
            if let Some(existing_element) = existing_element {
                let existing_node = self.nodes.borrow().get(&existing_element);
                if !existing_element.strict_equals(element) {
                    // A different element object was registered with the same ID.
                    // This can happen during concurrent tree operations (e.g., tree
                    // being switched to while data is updated). Clean up the stale
                    // element reference before re-registering with the new one.
                    self.nodes.borrow_mut().remove(&existing_element);
                }
                if let Some(existing_node) = existing_node {
                    let new_node = self.create_tree_node(element, ext_tree_item, parent_node)?;
                    self.update_node_cache(element, &new_node, &existing_node, parent_node);
                    existing_node.dispose();
                    return Ok(new_node);
                }
            }
        }
        let node = self.create_tree_node(element, ext_tree_item, parent_node)?;
        self.add_node_to_cache(element, &node);
        self.add_node_to_parent_cache(&node, parent_node);
        Ok(node)
    }

    fn get_tooltip(&self, tooltip: &Dynamic) -> Result<Option<StringOrMarkdown>, Error> {
        if is_instance_of(tooltip, "MarkdownString") {
            let markdown = types::MarkdownString::from_dynamic(tooltip)?;
            return Ok(Some(StringOrMarkdown::Markdown(type_converters::MarkdownString::from_markdown(&markdown))));
        }
        Ok(optional_string(tooltip).map(StringOrMarkdown::String))
    }

    fn get_command(&self, disposable: &DisposableStore, command: &Dynamic) -> Result<Option<TreeCommand>, Error> {
        if !command.is_truthy() {
            return Ok(None);
        }
        let command = Command::from_dynamic(command)?;
        let internal = self.commands.to_internal(Some(&command), disposable)?;
        Ok(internal.map(|internal| TreeCommand { command: internal, original_id: Some(command.command.clone()) }))
    }

    fn get_checkbox(&self, extension_tree_item: &Dynamic) -> Option<ITreeItemCheckboxState> {
        let checkbox_state = extension_tree_item.get_str("checkboxState");
        if checkbox_state.is_undefined() {
            return None;
        }
        let (state, tooltip, accessibility_information) = match &checkbox_state {
            Dynamic::Number(state) => (Some(*state), None, None),
            other => (
                match other.get_str("state") {
                    Dynamic::Number(state) => Some(state),
                    _ => None,
                },
                optional_string(&other.get_str("tooltip")),
                to_accessibility_information(&other.get_str("accessibilityInformation")),
            ),
        };
        Some(ITreeItemCheckboxState { is_checked: state == Some(f64::from(TreeItemCheckboxState::Checked.value())), tooltip, accessibility_information })
    }

    fn validate_tree_item(&self, extension_tree_item: &Dynamic) -> Result<(), Error> {
        if !types::TreeItem::is_tree_item(extension_tree_item, &self.extension) {
            return Err(Error::new(format!("Extension {} has provided an invalid tree item.", self.extension.identifier.value)));
        }
        Ok(())
    }

    fn create_tree_node(&self, element: &Dynamic, extension_tree_item: &Dynamic, parent: Option<&Rc<TreeNode>>) -> Result<Rc<TreeNode>, Error> {
        self.validate_tree_item(extension_tree_item)?;
        let disposable_store = DisposableStore::new();
        self.base.register(disposable_store.clone());
        let handle = self.create_handle(element, extension_tree_item, parent, false)?;
        let icon = self.get_light_icon_path(extension_tree_item)?;
        let collapsible_state = match extension_tree_item.get_str("collapsibleState") {
            Dynamic::Number(state) if state.fract() == 0.0 => TreeItemCollapsibleState::from_value(state as i32).unwrap_or_default(),
            _ => TreeItemCollapsibleState::None,
        };
        let item = ITreeItem {
            handle,
            parent_handle: parent.map(|parent| parent.handle()),
            label: to_tree_item_label(&extension_tree_item.get_str("label"), &self.extension)?,
            description: match extension_tree_item.get_str("description") {
                Dynamic::String(text) => Some(TreeItemDescription::Text(text.to_string_lossy())),
                Dynamic::Bool(flag) => Some(TreeItemDescription::Bool(flag)),
                _ => None,
            },
            resource_uri: self.get_optional_uri(&extension_tree_item.get_str("resourceUri"))?,
            tooltip: self.get_tooltip(&extension_tree_item.get_str("tooltip"))?,
            command: self.get_command(&disposable_store, &extension_tree_item.get_str("command"))?,
            context_value: optional_string(&extension_tree_item.get_str("contextValue")),
            icon: icon.clone(),
            icon_dark: match self.get_dark_icon_path(extension_tree_item)? {
                Some(dark) => Some(dark),
                None => icon,
            },
            theme_icon: self.get_theme_icon(extension_tree_item)?,
            collapsible_state,
            accessibility_information: to_accessibility_information(&extension_tree_item.get_str("accessibilityInformation")),
            checkbox: self.get_checkbox(extension_tree_item),
            children: None,
            parent: None,
        };

        Ok(Rc::new(TreeNode { item: RefCell::new(item), extension_item: extension_tree_item.clone(), parent: parent.cloned(), children: RefCell::new(None), disposable_store }))
    }

    fn get_optional_uri(&self, value: &Dynamic) -> Result<Option<URI>, Error> {
        if value.is_nullish() { Ok(None) } else { URI::from_dynamic(value).map(Some) }
    }

    fn get_theme_icon(&self, extension_tree_item: &Dynamic) -> Result<Option<je_base::themables::ThemeIcon>, Error> {
        let icon_path = extension_tree_item.get_str("iconPath");
        if is_instance_of(&icon_path, "ThemeIcon") { je_base::themables::ThemeIcon::from_dynamic(&icon_path).map(Some) } else { Ok(None) }
    }

    fn create_handle(&self, element: &Dynamic, extension_tree_item: &Dynamic, parent: Option<&Rc<TreeNode>>, return_first: bool) -> Result<TreeItemHandle, Error> {
        let id = extension_tree_item.get_str("id");
        if id.is_truthy() {
            return Ok(format!("{ID_HANDLE_PREFIX}/{}", js_to_string(&id)));
        }

        let tree_item_label = to_tree_item_label(&extension_tree_item.get_str("label"), &self.extension)?;
        let prefix = match parent {
            Some(parent) => parent.handle(),
            None => LABEL_HANDLE_PREFIX.to_owned(),
        };
        let label_value = match tree_item_label {
            Some(label) => match label.label {
                StringOrMarkdown::Markdown(markdown) => markdown.value.to_string_lossy(),
                StringOrMarkdown::String(text) => text,
            },
            None => String::new(),
        };
        let mut element_id = if !label_value.is_empty() {
            label_value
        } else {
            let resource_uri = extension_tree_item.get_str("resourceUri");
            if resource_uri.is_truthy() { ext_uri().basename(&URI::from_dynamic(&resource_uri)?, None) } else { String::new() }
        };
        if element_id.contains('/') {
            element_id = element_id.replacen('/', "//", 1);
        }
        let existing_handle = self.nodes.borrow().get(element).map(|node| node.handle());
        let children_nodes_len = self.children_len(parent);

        let mut counter = 0usize;
        loop {
            let handle = format!("{prefix}/{counter}:{element_id}");
            if return_first || !self.elements.borrow().contains_key(&handle) || existing_handle.as_deref() == Some(handle.as_str()) {
                // Return first if asked for or
                // Return if handle does not exist or
                // Return if handle is being reused
                return Ok(handle);
            }
            counter += 1;
            if counter > children_nodes_len {
                return Ok(handle);
            }
        }
    }

    fn get_light_icon_path(&self, extension_tree_item: &Dynamic) -> Result<Option<URI>, Error> {
        let icon_path = extension_tree_item.get_str("iconPath");
        if icon_path.is_truthy() && !is_instance_of(&icon_path, "ThemeIcon") {
            if matches!(icon_path, Dynamic::String(_)) || is_instance_of(&icon_path, "Uri") {
                return self.get_icon_path(&icon_path).map(Some);
            }
            return self.get_icon_path(&icon_path.get_str("light")).map(Some);
        }
        Ok(None)
    }

    fn get_dark_icon_path(&self, extension_tree_item: &Dynamic) -> Result<Option<URI>, Error> {
        let icon_path = extension_tree_item.get_str("iconPath");
        if icon_path.is_truthy() && !is_instance_of(&icon_path, "ThemeIcon") && icon_path.get_str("dark").is_truthy() {
            return self.get_icon_path(&icon_path.get_str("dark")).map(Some);
        }
        Ok(None)
    }

    /// `_getIconPath(iconPath)`: um `URI` fica como está e um caminho vira `URI.file(path)`.
    fn get_icon_path(&self, icon_path: &Dynamic) -> Result<URI, Error> {
        if is_instance_of(icon_path, "Uri") {
            return URI::from_dynamic(icon_path);
        }
        match icon_path {
            Dynamic::String(path) => Ok(uri_file(&path.to_string_lossy())),
            _ => Err(Error::type_error("Cannot read properties of undefined (reading '0')")),
        }
    }

    fn add_node_to_cache(&self, element: &Dynamic, node: &Rc<TreeNode>) {
        self.elements.borrow_mut().insert(node.handle(), element.clone());
        self.nodes.borrow_mut().insert(element, node.clone());
    }

    fn update_node_cache(&self, element: &Dynamic, new_node: &Rc<TreeNode>, existing: &Rc<TreeNode>, parent_node: Option<&Rc<TreeNode>>) {
        // Remove from the cache
        self.elements.borrow_mut().remove(&new_node.handle());
        self.nodes.borrow_mut().remove(element);
        if new_node.handle() != existing.handle() {
            self.elements.borrow_mut().remove(&existing.handle());
        }

        // Add the new node to the cache
        self.add_node_to_cache(element, new_node);

        // Replace the node in parent's children nodes
        let existing_handle = existing.handle();
        let mut children_slot = match parent_node {
            Some(parent) => parent.children.borrow_mut(),
            None => self.roots.borrow_mut(),
        };
        if let Some(children_nodes) = children_slot.as_mut()
            && let Some(index) = children_nodes.iter().position(|child| child.has_handle(&existing_handle))
        {
            children_nodes[index] = new_node.clone();
        }
    }

    fn add_node_to_parent_cache(&self, node: &Rc<TreeNode>, parent_node: Option<&Rc<TreeNode>>) {
        match parent_node {
            Some(parent) => parent.children.borrow_mut().get_or_insert_with(Vec::new).push(node.clone()),
            None => self.roots.borrow_mut().get_or_insert_with(Vec::new).push(node.clone()),
        }
    }

    fn add_children_to_clear(&self, parent_element: Option<&Dynamic>) {
        match parent_element.filter(|element| element.is_truthy()) {
            Some(parent_element) => {
                let node = self.nodes.borrow().get(parent_element);
                if let Some(node) = node {
                    let children = node.children.borrow().clone();
                    for child in children.iter().flatten() {
                        self.nodes_to_clear.borrow_mut().add(child);
                        let child_handle = child.handle();
                        let child_element = self.elements.borrow().get(&child_handle).cloned().filter(|element| element.is_truthy());
                        if let Some(child_element) = child_element {
                            self.add_children_to_clear(Some(&child_element));
                            self.nodes.borrow_mut().remove(&child_element);
                            self.elements.borrow_mut().remove(&child_handle);
                        }
                    }
                    *node.children.borrow_mut() = None;
                }
            }
            None => self.add_all_to_clear(),
        }
    }

    fn add_all_to_clear(&self) {
        *self.roots.borrow_mut() = None;
        let nodes = self.nodes.borrow().values();
        for node in &nodes {
            self.nodes_to_clear.borrow_mut().add(node);
        }
        self.nodes.borrow_mut().clear();
        self.elements.borrow_mut().clear();
        self.children_fetch_tokens.borrow_mut().clear();
        self.root_fetch_token.set(None);
    }

    fn clear_nodes(&self, nodes: &[Rc<TreeNode>]) {
        dispose_all(nodes.iter());
    }

    fn clear_all(&self) {
        *self.roots.borrow_mut() = None;
        self.elements.borrow_mut().clear();
        let nodes = self.nodes.borrow().values();
        dispose_all(nodes.iter());
        self.nodes.borrow_mut().clear();
        let nodes_to_clear = self.nodes_to_clear.borrow_mut().take_all();
        dispose_all(nodes_to_clear.iter());
        // Desfaz os ciclos filho -> pai -> filhos de `Rc`: no original o coletor de lixo os recolhe.
        for node in nodes.iter().chain(nodes_to_clear.iter()) {
            node.children.borrow_mut().take();
        }
        self.children_fetch_tokens.borrow_mut().clear();
        self.root_fetch_token.set(None);
    }
}

impl IDisposable for ExtHostTreeView {
    fn dispose(&self) {
        self.base.dispose();
        self.refresh_cancellation_source.borrow().dispose();

        self.clear_all();
    }
}
