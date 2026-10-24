//! `vscode.window.showQuickPick` e `vscode.window.showInputBox`, ligados ao `ExtHostQuickOpen` portado (`extHostQuickOpen.ts`).
//!
//! Este módulo é a fronteira com o motor: converte os itens e as opções que a extensão passa (texto ou objeto, `canPickMany`,
//! `onDidSelectItem`, `validateInput`, `password`, o token de cancelamento) para os tipos do `ExtHostQuickOpen`, e devolve ao
//! JavaScript o que o original devolve (o item, ou a lista deles com `canPickMany`, ou `undefined` se o usuário fechou).
//!
//! Divergências:
//! - O item que a promessa devolve (e o que `onDidSelectItem` recebe) é a cópia que atravessou a fronteira: o motor copia objetos
//!   comuns, então `chosen === items[1]` falha para itens objeto (comparar por `label` funciona). Para itens de texto não muda nada.
//! - `QuickPickItem#tooltip` aceita só texto (a `MarkdownString` ainda não é classe do `vscode`) e `buttons` não é lido (o
//!   `showQuickPick` do original também não os envia; só o `createQuickPick` usa botões de item).
//! - `createQuickPick` e `createInputBox` não estão expostos (os objetos de sessão pedem acessores de leitura e escrita para as quase
//!   trinta propriedades deles, uma entrega à parte).
//! - O `validateInput` que lança, ou cuja promessa rejeita, rejeita a validação no lado do workbench (como o `$validateInput`
//!   `async` do original). Um resultado falso (`undefined`, `null`, `''`, `false`) é "sem mensagem".

use std::cell::RefCell;
use std::rc::Rc;

use je_base::errors::{on_unexpected_error, Error};
use je_base::promise::{MaybePromise, Promise};
use je_base::themables::ThemeIcon;
use je_workbench::api::common::ext_host_protocol::{IInputBoxOptions, QuickOpenShowResult};
use je_workbench::api::common::ext_host_quick_open::{ExtHostQuickOpen, InputBoxOptions, InputBoxValidation, QuickPickEntry, QuickPickItem, QuickPickOptions};
use je_workbench::api::common::ext_host_type_converters::icon_path::{IconPathInput, UriOrPath};
use je_workbench::api::common::ext_host_types::{Documentation, InputBoxValidationSeverity};
use je_workbench::api::common::ext_host_values::ApiValue;
use zjsc::api::embed_node::{JsData, JsHandle, NodeRunner};

use crate::api::commands::{promise_to_js, settle_js_result};
use crate::api::events::EventTables;
use crate::convert::{class_name, dynamic_to_js, field, js_to_dynamic, js_to_error, opt_bool, opt_number, opt_string, uri_from_js};

/// Os itens de uma chamada de `showQuickPick`, como a extensão os passou: o `handle` que o workbench devolve é o índice aqui.
type ItemStore = Rc<RefCell<Vec<JsData>>>;

/// O `validateInput` como o `ExtHostQuickOpen` o espera.
type ValidateInput = Rc<dyn Fn(&str) -> MaybePromise<InputBoxValidation>>;

/// Uma promessa já assentada com o resultado de uma conversão.
fn settled<T: Clone + 'static>(result: Result<T, Error>) -> Promise<T> {
    match result {
        Ok(value) => Promise::resolved(value),
        Err(error) => Promise::rejected(error),
    }
}

/// `{ light?, dark? }` de um `iconPath`: cada lado é `Uri` ou texto.
fn uri_or_path(value: Option<&JsData>) -> Result<Option<UriOrPath>, String> {
    match value {
        None | Some(JsData::Undefined) | Some(JsData::Null) => Ok(None),
        Some(JsData::Str(path)) => Ok(Some(UriOrPath::Path(path.clone()))),
        Some(uri @ JsData::Object(_)) => Ok(Some(UriOrPath::Uri(uri_from_js(uri)?))),
        Some(_) => Err("iconPath: light e dark precisam ser Uri ou texto".to_owned()),
    }
}

/// O `iconPath` de um item: `ThemeIcon`, `Uri`, texto ou `{ light, dark }`.
fn icon_path_of(value: &JsData) -> Result<Option<IconPathInput>, String> {
    match value {
        JsData::Undefined | JsData::Null => Ok(None),
        JsData::Str(path) => Ok(Some(IconPathInput::Path(path.clone()))),
        JsData::Object(_) => match class_name(value) {
            Some("Uri") => Ok(Some(IconPathInput::Uri(uri_from_js(value)?))),
            Some("ThemeIcon") => Ok(Some(IconPathInput::ThemeIcon(ThemeIcon::from_dynamic(&js_to_dynamic(value)).map_err(|error| error.message)?))),
            _ => Ok(Some(IconPathInput::Object { light: uri_or_path(field(value, "light"))?, dark: uri_or_path(field(value, "dark"))? })),
        },
        _ => Err("iconPath precisa ser ThemeIcon, Uri, texto ou { light, dark }".to_owned()),
    }
}

/// `string | QuickPickItem`: o item de texto ou o objeto (com `kind` separador ou não).
fn quick_pick_entry_of(item: &JsData) -> Result<QuickPickEntry, String> {
    match item {
        JsData::Str(label) => Ok(QuickPickEntry::Text(label.clone())),
        JsData::Object(_) => {
            let icon_path = match field(item, "iconPath") {
                Some(icon_path) => icon_path_of(icon_path)?,
                None => None,
            };
            let resource_uri = match field(item, "resourceUri") {
                Some(uri @ JsData::Object(_)) => Some(uri_from_js(uri)?),
                _ => None,
            };
            Ok(QuickPickEntry::Item(Rc::new(QuickPickItem {
                label: opt_string(field(item, "label")).unwrap_or_default(),
                // `item.kind === QuickPickItemKind.Separator`
                is_separator: opt_number(field(item, "kind")) == Some(-1.0),
                icon_path,
                description: opt_string(field(item, "description")),
                detail: opt_string(field(item, "detail")),
                tooltip: opt_string(field(item, "tooltip")).map(Documentation::Text),
                picked: opt_bool(field(item, "picked")),
                always_show: opt_bool(field(item, "alwaysShow")),
                resource_uri,
                ..QuickPickItem::default()
            })))
        }
        _ => Err("showQuickPick: cada item precisa ser texto ou um objeto com label".to_owned()),
    }
}

/// A lista de itens da extensão como entradas do `ExtHostQuickOpen`; guarda a lista original em `store` para resolver os `handle`.
fn read_entries(items: &JsData, store: &ItemStore) -> Result<Vec<QuickPickEntry>, Error> {
    let JsData::Array(list) = items else {
        return Err(Error::new("showQuickPick: os itens precisam ser uma lista ou uma promessa de lista"));
    };
    let entries = list.iter().map(quick_pick_entry_of).collect::<Result<Vec<_>, String>>().map_err(Error::new)?;
    *store.borrow_mut() = list.clone();
    Ok(entries)
}

/// `Promise.resolve(itemsOrItemsPromise)`: a lista, ou a promessa dela, como promessa do Rust.
fn items_promise(runner: &NodeRunner, items: &JsData, store: &ItemStore) -> Promise<Vec<QuickPickEntry>> {
    let read_store = store.clone();
    let read = move |value: &JsData| read_entries(value, &read_store);
    match items {
        JsData::Handle(_) => match settle_js_result(runner, items.clone()) {
            MaybePromise::Promise(promise) => promise.then_result(move |outcome| read(&dynamic_to_js(&outcome?))),
            MaybePromise::Value(value) => settled(read(&dynamic_to_js(&value))),
        },
        other => settled(read(other)),
    }
}

/// O item que o `handle` indica na lista que a extensão passou.
fn item_at(store: &ItemStore, handle: u32) -> JsData {
    store.borrow().get(handle as usize).cloned().unwrap_or(JsData::Undefined)
}

/// As `QuickPickOptions` da extensão; `onDidSelectItem` chama a função dela com o item (`options.onDidSelectItem(items[handle])`).
fn quick_pick_options_of(runner: &NodeRunner, options: Option<&JsData>, store: &ItemStore) -> QuickPickOptions {
    let Some(options @ JsData::Object(_)) = options else { return QuickPickOptions::default() };
    let on_did_select_item = match field(options, "onDidSelectItem") {
        Some(JsData::Handle(callback)) => {
            let (runner, callback, store) = (runner.clone(), callback.clone(), store.clone());
            let on_did_select_item: Rc<dyn Fn(u32)> = Rc::new(move |handle| {
                if let Err(thrown) = runner.call_data_throwing(&callback, &[item_at(&store, handle)]) {
                    on_unexpected_error(&js_to_error(&thrown));
                }
            });
            Some(on_did_select_item)
        }
        _ => None,
    };
    QuickPickOptions {
        title: opt_string(field(options, "title")),
        place_holder: opt_string(field(options, "placeHolder")),
        prompt: opt_string(field(options, "prompt")),
        match_on_description: opt_bool(field(options, "matchOnDescription")),
        match_on_detail: opt_bool(field(options, "matchOnDetail")),
        ignore_focus_out: opt_bool(field(options, "ignoreFocusOut")),
        can_pick_many: opt_bool(field(options, "canPickMany")),
        on_did_select_item,
    }
}

/// `window.showQuickPick(items, options?, token?)`.
pub fn show_quick_pick(runner: &NodeRunner, quick_open: &ExtHostQuickOpen, events: &EventTables, args: &[JsData]) -> Result<JsData, String> {
    let store = ItemStore::default();
    let items = items_promise(runner, args.first().unwrap_or(&JsData::Undefined), &store);
    let options = quick_pick_options_of(runner, args.get(1), &store);
    let picked = quick_open.show_quick_pick(items, Some(options), events.token_arg(args.get(2)));
    Ok(promise_to_js(runner, picked, move |result| match result {
        Some(QuickOpenShowResult::One(handle)) => item_at(&store, *handle),
        Some(QuickOpenShowResult::Many(handles)) => JsData::Array(handles.iter().map(|handle| item_at(&store, *handle)).collect()),
        None => JsData::Undefined,
    }))
}

/// O que `validateInput` devolveu (`string | InputBoxValidationMessage | undefined | null`).
fn validation_of(value: &JsData) -> InputBoxValidation {
    match value {
        JsData::Str(text) if !text.is_empty() => InputBoxValidation::Text(text.clone()),
        JsData::Object(_) => InputBoxValidation::Message {
            message: opt_string(field(value, "message")).unwrap_or_default(),
            severity: opt_number(field(value, "severity")).and_then(|severity| InputBoxValidationSeverity::from_value(severity as i32)),
        },
        _ => InputBoxValidation::None,
    }
}

/// A função `validateInput` da extensão como o validador do `ExtHostQuickOpen`.
fn validate_input_of(runner: &NodeRunner, function: &Rc<JsHandle>) -> ValidateInput {
    let (runner, function) = (runner.clone(), function.clone());
    Rc::new(move |input: &str| match runner.call_data_throwing(&function, &[JsData::Str(input.to_owned())]) {
        Ok(result) => match settle_js_result(&runner, result) {
            MaybePromise::Value(value) => MaybePromise::Value(validation_of(&dynamic_to_js(&value))),
            MaybePromise::Promise(promise) => MaybePromise::Promise(promise.then(|value| validation_of(&dynamic_to_js(&value)))),
        },
        Err(thrown) => MaybePromise::Promise(Promise::rejected(js_to_error(&thrown))),
    })
}

/// As `InputBoxOptions` da extensão.
fn input_box_options_of(runner: &NodeRunner, options: Option<&JsData>) -> InputBoxOptions {
    let Some(options @ JsData::Object(_)) = options else { return InputBoxOptions::default() };
    let value_selection = match field(options, "valueSelection") {
        Some(JsData::Array(range)) => match (opt_number(range.first()), opt_number(range.get(1))) {
            (Some(start), Some(end)) => Some((start as u32, end as u32)),
            _ => None,
        },
        _ => None,
    };
    let validate_input = match field(options, "validateInput") {
        Some(JsData::Handle(function)) => Some(validate_input_of(runner, function)),
        _ => None,
    };
    InputBoxOptions {
        options: IInputBoxOptions {
            title: opt_string(field(options, "title")),
            value: opt_string(field(options, "value")),
            value_selection,
            prompt: opt_string(field(options, "prompt")),
            place_holder: opt_string(field(options, "placeHolder")),
            password: opt_bool(field(options, "password")),
            ignore_focus_out: opt_bool(field(options, "ignoreFocusOut")),
        },
        validate_input,
    }
}

/// `window.showInputBox(options?, token?)`.
pub fn show_input_box(runner: &NodeRunner, quick_open: &ExtHostQuickOpen, events: &EventTables, args: &[JsData]) -> Result<JsData, String> {
    let typed = quick_open.show_input(Some(input_box_options_of(runner, args.first())), events.token_arg(args.get(1)));
    Ok(promise_to_js(runner, typed, |value| match value {
        Some(text) => JsData::Str(text.clone()),
        None => JsData::Undefined,
    }))
}
