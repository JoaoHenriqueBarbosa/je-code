//! Os tipos de `extHostTypes.ts` que a API de árvores de visões usa: `ViewBadge`, `TreeItem.isTreeItem` e a família
//! `DataTransfer` (`DataTransferItem`, `InternalDataTransferItem`, `InternalFileDataTransferItem`, `DataTransferFile`
//! e `DataTransfer`).
//!
//! Divergências:
//! - `ViewBadge.isViewBadge` não existe: o [`ViewBadge`] é um struct, então `value` ser número e `tooltip` ser texto é
//!   garantido pelo tipo.
//! - [`TreeItem::is_tree_item`] lê o item como [`Dynamic`], porque o provedor da extensão devolve um objeto qualquer
//!   (a interface `vscode.TreeItem`, não só instâncias da classe). A classe `TreeItem` é a marcada com `$$class`
//!   (`instanceof TreeItem`), e o `instanceof MarkdownString` do `tooltip` é a marca `MarkdownString`. O duck typing
//!   de `MarkdownString.isMarkdownString` (objetos com `appendText` etc.) não existe, como no resto de
//!   `ext_host_values`. O teste `collapsibleState < None && collapsibleState > Expanded` do original nunca é
//!   verdadeiro e não é portado.
//! - `DataTransferItem` e `DataTransfer` são objetos com identidade (a suíte compara `dropped === original`), então
//!   são `Rc`. `DataTransferItem` cobre as três classes do original por um tipo interno ([`DataTransferItem::new`] é
//!   a classe da API; [`DataTransferItem::new_internal`] e [`DataTransferItem::new_internal_file`] são as duas
//!   `Internal...`, que o VS Code cria e as extensões não). O `asString` de um valor que o `JSON.stringify` não
//!   serializa (`undefined`, função) resolve com texto vazio em vez de `undefined`.
//! - `DataTransferFile` guarda o `_getData` como fecho; `#file` e `#items` privados viram campos privados.

use std::cell::RefCell;
use std::rc::Rc;

use je_base::async_::async_fn;
use je_base::dynamic::{json_stringify, Dynamic, Truthy};
use je_base::promise::Promise;
use je_base::uri::URI;
use je_extensions::scanner::ExtensionDescription;

use super::{TreeItem, TreeItemCheckboxState};
use crate::api::common::ext_host_values::is_instance_of;

// ---- ViewBadge --------------------------------------------------------------------------------------

/// `vscode.ViewBadge`.
#[derive(Clone, Debug, PartialEq)]
pub struct ViewBadge {
    pub value: f64,
    pub tooltip: String,
}

// ---- TreeItem.isTreeItem ----------------------------------------------------------------------------

fn is_string(value: &Dynamic) -> bool {
    matches!(value, Dynamic::String(_))
}

/// `isNumber`: `typeof obj === 'number' && !isNaN(obj)`.
fn is_number(value: &Dynamic) -> bool {
    matches!(value, Dynamic::Number(number) if !number.is_nan())
}

/// `URI.isUri`: o valor marcado como `Uri`.
fn is_uri(value: &Dynamic) -> bool {
    is_instance_of(value, "Uri")
}

/// `isObject`: um objeto que não é `null`, nem array, nem `RegExp`, nem `Date`.
fn is_object(value: &Dynamic) -> bool {
    matches!(value, Dynamic::Object(_) | Dynamic::TypedArray(_))
}

/// `value?.key`: `undefined` quando o valor é `undefined` ou `null`.
fn optional_chain(value: &Dynamic, key: &str) -> Dynamic {
    if value.is_nullish() { Dynamic::Undefined } else { value.get_str(key) }
}

fn log_invalid(what: &str, value: &Dynamic) {
    eprintln!("INVALID tree item, invalid {what} {value:?}");
}

impl TreeItem {
    /// `TreeItem.isTreeItem(thing, extension)`.
    pub fn is_tree_item(thing: &Dynamic, _extension: &ExtensionDescription) -> bool {
        let checkbox_state = thing.get_str("checkboxState");
        if !checkbox_state.is_undefined() {
            let checkbox = if is_number(&checkbox_state) {
                Some(checkbox_state.clone())
            } else if is_object(&checkbox_state) && is_number(&checkbox_state.get_str("state")) {
                Some(checkbox_state.get_str("state"))
            } else {
                None
            };
            let tooltip = if !matches!(checkbox_state, Dynamic::Number(_)) && is_object(&checkbox_state) { checkbox_state.get_str("tooltip") } else { Dynamic::Undefined };
            let known_state = |state: &Dynamic| matches!(state, Dynamic::Number(number) if *number == f64::from(TreeItemCheckboxState::Checked.value()) || *number == f64::from(TreeItemCheckboxState::Unchecked.value()));
            if !checkbox.as_ref().is_some_and(known_state) || (!tooltip.is_undefined() && !is_string(&tooltip)) {
                log_invalid("checkboxState", &checkbox_state);
                return false;
            }
        }

        if is_instance_of(thing, "TreeItem") {
            return true;
        }

        let label = thing.get_str("label");
        if !label.is_undefined() && !is_string(&label) && !optional_chain(&label, "label").is_truthy() {
            log_invalid("label", &label);
            return false;
        }
        let id = thing.get_str("id");
        if !id.is_undefined() && !is_string(&id) {
            log_invalid("id", &id);
            return false;
        }
        let icon_path = thing.get_str("iconPath");
        if !icon_path.is_undefined() && !is_string(&icon_path) && !is_uri(&icon_path) && (!icon_path.is_truthy() || !is_string(&icon_path.get_str("id"))) {
            let as_light_and_dark_thing = icon_path.clone();
            if !as_light_and_dark_thing.is_truthy() {
                log_invalid("iconPath", &icon_path);
                return false;
            }
            let (light, dark) = (as_light_and_dark_thing.get_str("light"), as_light_and_dark_thing.get_str("dark"));
            if !is_string(&light) && !is_uri(&light) && !is_string(&dark) && !is_uri(&dark) {
                log_invalid("iconPath", &icon_path);
                return false;
            }
        }
        let description = thing.get_str("description");
        if !description.is_undefined() && !is_string(&description) && !matches!(description, Dynamic::Bool(_)) {
            log_invalid("description", &description);
            return false;
        }
        let resource_uri = thing.get_str("resourceUri");
        if !resource_uri.is_undefined() && !is_uri(&resource_uri) {
            log_invalid("resourceUri", &resource_uri);
            return false;
        }
        let tooltip = thing.get_str("tooltip");
        if !tooltip.is_undefined() && !is_string(&tooltip) && !is_instance_of(&tooltip, "MarkdownString") {
            log_invalid("tooltip", &tooltip);
            return false;
        }
        let command = thing.get_str("command");
        if !command.is_undefined() && !command.get_str("command").is_truthy() {
            log_invalid("command", &command);
            return false;
        }
        let context_value = thing.get_str("contextValue");
        if !context_value.is_undefined() && !is_string(&context_value) {
            log_invalid("contextValue", &context_value);
            return false;
        }
        let accessibility_information = thing.get_str("accessibilityInformation");
        if !accessibility_information.is_undefined() && !optional_chain(&accessibility_information, "label").is_truthy() {
            log_invalid("accessibilityInformation", &accessibility_information);
            return false;
        }

        true
    }
}

// ---- DataTransfer -----------------------------------------------------------------------------------

/// `DataTransferFile`: o arquivo de um item (só o VS Code os cria).
pub struct DataTransferFile {
    pub name: String,
    pub uri: Option<URI>,
    /// `_itemId`.
    pub item_id: String,
    get_data: Rc<dyn Fn() -> Promise<Vec<u8>>>,
}

impl DataTransferFile {
    pub fn new(name: impl Into<String>, uri: Option<URI>, item_id: impl Into<String>, get_data: impl Fn() -> Promise<Vec<u8>> + 'static) -> DataTransferFile {
        DataTransferFile { name: name.into(), uri, item_id: item_id.into(), get_data: Rc::new(get_data) }
    }

    /// `data()`.
    pub fn data(&self) -> Promise<Vec<u8>> {
        (self.get_data)()
    }
}

/// Qual das três classes do original o item é.
enum ItemKind {
    /// `DataTransferItem`: criado pela extensão.
    Extension,
    /// `InternalDataTransferItem`: criado pelo VS Code.
    Internal,
    /// `InternalFileDataTransferItem`: criado pelo VS Code para um arquivo.
    InternalFile(Rc<DataTransferFile>),
}

/// `DataTransferItem` (e as classes internas que herdam dela).
pub struct DataTransferItem {
    value: Dynamic,
    kind: ItemKind,
}

impl DataTransferItem {
    /// `new DataTransferItem(value)`.
    pub fn new(value: Dynamic) -> Rc<DataTransferItem> {
        Rc::new(DataTransferItem { value, kind: ItemKind::Extension })
    }

    /// `new InternalDataTransferItem(value)`.
    pub fn new_internal(value: Dynamic) -> Rc<DataTransferItem> {
        Rc::new(DataTransferItem { value, kind: ItemKind::Internal })
    }

    /// `new InternalFileDataTransferItem(file)`: o valor é `''`.
    pub fn new_internal_file(file: Rc<DataTransferFile>) -> Rc<DataTransferItem> {
        Rc::new(DataTransferItem { value: Dynamic::string(""), kind: ItemKind::InternalFile(file) })
    }

    /// `value`.
    pub fn value(&self) -> &Dynamic {
        &self.value
    }

    /// O item foi criado pelo VS Code (`instanceof InternalDataTransferItem`)?
    pub fn is_internal(&self) -> bool {
        !matches!(self.kind, ItemKind::Extension)
    }

    /// `asString()`: o próprio texto, ou o `JSON.stringify` do valor.
    pub fn as_string(&self) -> Promise<String> {
        let value = self.value.clone();
        async_fn(async move {
            Ok(match &value {
                Dynamic::String(text) => text.to_string_lossy(),
                other => json_stringify(other).unwrap_or_default(),
            })
        })
    }

    /// `asFile()`.
    pub fn as_file(&self) -> Option<Rc<DataTransferFile>> {
        match &self.kind {
            ItemKind::InternalFile(file) => Some(file.clone()),
            _ => None,
        }
    }
}

/// `DataTransfer`.
pub struct DataTransfer {
    items: RefCell<Vec<(String, Vec<Rc<DataTransferItem>>)>>,
}

/// `#normalizeMime`.
fn normalize_mime(mime_type: &str) -> String {
    mime_type.to_lowercase()
}

impl DataTransfer {
    /// `new DataTransfer(init?)`.
    pub fn new(init: impl IntoIterator<Item = (String, Rc<DataTransferItem>)>) -> Rc<DataTransfer> {
        let transfer = Rc::new(DataTransfer { items: RefCell::new(Vec::new()) });
        for (mime, item) in init {
            let key = normalize_mime(&mime);
            let mut items = transfer.items.borrow_mut();
            match items.iter_mut().find(|(existing, _)| *existing == key) {
                Some((_, existing)) => existing.push(item),
                None => items.push((key, vec![item])),
            }
        }
        transfer
    }

    /// `get(mimeType)`.
    pub fn get(&self, mime_type: &str) -> Option<Rc<DataTransferItem>> {
        let key = normalize_mime(mime_type);
        self.items.borrow().iter().find(|(existing, _)| *existing == key).and_then(|(_, items)| items.first().cloned())
    }

    /// `set(mimeType, value)`: sobrescreve todas as entradas do tipo (como o `DataTransfer` do DOM).
    pub fn set(&self, mime_type: &str, value: Rc<DataTransferItem>) {
        let key = normalize_mime(mime_type);
        let mut items = self.items.borrow_mut();
        match items.iter_mut().find(|(existing, _)| *existing == key) {
            Some((_, existing)) => *existing = vec![value],
            None => items.push((key, vec![value])),
        }
    }

    /// `[Symbol.iterator]()`: todos os pares `(mime, item)`, na ordem de inserção dos tipos.
    pub fn iter(&self) -> Vec<(String, Rc<DataTransferItem>)> {
        self.items.borrow().iter().flat_map(|(mime, items)| items.iter().map(move |item| (mime.clone(), item.clone()))).collect()
    }

    /// `forEach(callbackfn)`: o `callbackfn` recebe o item, o tipo e o próprio `DataTransfer`.
    pub fn for_each(self: &Rc<Self>, mut callback: impl FnMut(&Rc<DataTransferItem>, &str, &Rc<DataTransfer>)) {
        for (mime, item) in self.iter() {
            callback(&item, &mime, self);
        }
    }
}
