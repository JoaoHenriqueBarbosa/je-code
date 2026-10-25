//! A classe `vscode.WorkspaceEdit` e `vscode.workspace.applyEdit(edit, metadata?)`.
//!
//! É a ponte para o `WorkspaceEdit` portado de `extHostTypes/workspaceEdit.ts` (`ext_host_types::edits`) e para o `ExtHostBulkEdits`
//! (`extHostBulkEdits.ts`), que converte a edição no DTO do protocolo, com a versão de cada documento aberto, e a entrega ao cliente
//! `MainThreadBulkEdits`. O objeto que a extensão segura carrega só o `_id`; as edições moram no lado Rust, numa tabela por id (como o
//! `scm` guarda os grupos), porque o motor entrega o objeto por cópia e um método de classe nunca altera o receptor.
//!
//! O `WorkspaceEdit` tem `replace`, `insert`, `delete`, `set`, `createFile`, `deleteFile`, `renameFile`, `has`, `get`, `entries`,
//! `size` e `toJSON`. `set(uri, edits)` aceita `TextEdit`, `SnippetTextEdit`, `NotebookEdit` e as tuplas `[edit, metadata]`; `set(uri, null)` remove as
//! edições de `uri`. Os metadados (`needsConfirmation`, `label`, `description`, `iconPath`) valem em todos os métodos que os recebem.
//!
//! Falta: `contents` de `createFile` (`Uint8Array` e `DataTransferFile`
//! não atravessam a fronteira do motor; informar `contents` lança `TypeError`).
//!
//! Divergências:
//! - A tabela de edições não tem coletor: a edição de uma `WorkspaceEdit` já criada vive até o fim do host (o motor não avisa quando o
//!   objeto é descartado).
//! - O `extension` de `applyEdit` é o `je.exthost` (o `vscode` é um só para todas as extensões), e o original nem o usa.
//! - `get` e `entries` devolvem cópias dos `TextEdit` (a mutação de um deles não altera a edição).

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use je_base::themables::ThemeIcon;
use je_base::uri::URI;
use je_workbench::api::common::ext_host_bulk_edits::{ExtHostBulkEdits, WorkspaceEditMetadata};
use je_workbench::api::common::ext_host_types::{
    FileOperationOptions, IconPath, SetEditItem, SnippetString, SnippetTextEdit, TextEdit, WorkspaceEdit, WorkspaceEditEntryMetadata,
};
use je_workbench::api::common::ext_host_values::ApiValue;
use zjsc::api::embed_node::{ClassSpec, JsData, MethodCallback, NodeRunner};

use crate::api::commands::promise_to_js;
use crate::api::languages::host_extension;
use crate::api::notebooks::{is_notebook_edit, notebook_edit_of};
use crate::api::language_types::text_edit_from_js;
use crate::api::types::{to_position, to_range};
use crate::convert::{api_to_js, class_name, field, is_nullish, js_to_dynamic, opt_bool, opt_string, uri_from_js, uri_to_js};

/// As `WorkspaceEdit` que as extensões criaram, por id (os objetos JavaScript carregam só o `_id`).
#[derive(Default)]
pub struct WorkspaceEditObjects {
    edits: RefCell<HashMap<u32, WorkspaceEdit>>,
    next: Cell<u32>,
}

impl WorkspaceEditObjects {
    fn create(&self) -> JsData {
        let id = self.next.get() + 1;
        self.next.set(id);
        self.edits.borrow_mut().insert(id, WorkspaceEdit::new());
        JsData::Object(vec![("_id".to_owned(), JsData::Num(f64::from(id)))])
    }

    fn id_of(this: &JsData) -> Result<u32, String> {
        match field(this, "_id") {
            Some(JsData::Num(id)) => Ok(*id as u32),
            _ => Err("WorkspaceEdit: this inválido".to_owned()),
        }
    }

    fn with<T>(&self, this: &JsData, run: impl FnOnce(&mut WorkspaceEdit) -> Result<T, String>) -> Result<T, String> {
        let id = Self::id_of(this)?;
        let mut edits = self.edits.borrow_mut();
        run(edits.get_mut(&id).ok_or("WorkspaceEdit: a edição não existe neste host")?)
    }

    fn snapshot(&self, value: &JsData) -> Result<WorkspaceEdit, String> {
        self.with(value, |edit| Ok(edit.clone()))
    }

    /// A edição que `value` representa, se for uma `WorkspaceEdit` deste host (o valor de uma promessa de `waitUntil`); qualquer
    /// outro valor é `None`.
    pub(crate) fn resolved_edit(&self, value: &JsData) -> Option<WorkspaceEdit> {
        if class_name(value) == Some("WorkspaceEdit") { self.snapshot(value).ok() } else { None }
    }
}

// ---- a leitura dos argumentos

fn required<'a>(args: &'a [JsData], index: usize, what: &str) -> Result<&'a JsData, String> {
    match args.get(index) {
        Some(value) if !is_nullish(Some(value)) => Ok(value),
        _ => Err(format!("Illegal argument: {what}")),
    }
}

fn uri_arg(args: &[JsData], index: usize, what: &str) -> Result<URI, String> {
    uri_from_js(required(args, index, what)?)
}

fn text_arg(args: &[JsData], index: usize, what: &str) -> Result<String, String> {
    match args.get(index) {
        Some(JsData::Str(text)) => Ok(text.clone()),
        _ => Err(format!("Illegal argument: {what}")),
    }
}

fn icon_path_of(value: &JsData) -> Result<Option<IconPath>, String> {
    match value {
        JsData::Object(_) if class_name(value) == Some("ThemeIcon") => Ok(Some(IconPath::ThemeIcon(ThemeIcon::from_dynamic(&js_to_dynamic(value)).map_err(|error| error.message)?))),
        JsData::Object(_) if class_name(value) == Some("Uri") => Ok(Some(IconPath::Uri(uri_from_js(value)?))),
        JsData::Object(_) => match (field(value, "light"), field(value, "dark")) {
            (Some(light), Some(dark)) => Ok(Some(IconPath::LightDark { light: uri_from_js(light)?, dark: uri_from_js(dark)? })),
            _ => Err("WorkspaceEditEntryMetadata.iconPath precisa ser Uri, { light, dark } ou ThemeIcon".to_owned()),
        },
        _ => Ok(None),
    }
}

/// `vscode.WorkspaceEditEntryMetadata`; `undefined` e `null` são "sem metadados".
fn metadata_of(value: Option<&JsData>) -> Result<Option<WorkspaceEditEntryMetadata>, String> {
    match value {
        Some(metadata @ JsData::Object(_)) => Ok(Some(WorkspaceEditEntryMetadata {
            needs_confirmation: opt_bool(field(metadata, "needsConfirmation")).unwrap_or(false),
            label: opt_string(field(metadata, "label")).unwrap_or_default(),
            description: opt_string(field(metadata, "description")),
            icon_path: match field(metadata, "iconPath") {
                Some(icon) => icon_path_of(icon)?,
                None => None,
            },
        })),
        _ => Ok(None),
    }
}

/// As opções de `createFile`, `deleteFile` e `renameFile`.
fn file_options_of(value: Option<&JsData>) -> Result<Option<FileOperationOptions>, String> {
    let Some(options @ JsData::Object(_)) = value else { return Ok(None) };
    if !is_nullish(field(options, "contents")) {
        return Err("createFile: as opções 'contents' ainda não são suportadas pelo host".to_owned());
    }
    Ok(Some(FileOperationOptions {
        overwrite: opt_bool(field(options, "overwrite")),
        ignore_if_exists: opt_bool(field(options, "ignoreIfExists")),
        ignore_if_not_exists: opt_bool(field(options, "ignoreIfNotExists")),
        recursive: opt_bool(field(options, "recursive")),
        contents: None,
    }))
}

/// Uma edição de `set`: `TextEdit`, `SnippetTextEdit` ou a tupla `[edit, metadata]`.
fn set_item_of(value: &JsData) -> Result<Option<SetEditItem>, String> {
    if is_nullish(Some(value)) {
        return Ok(None);
    }
    let (edit, metadata) = match value {
        JsData::Array(tuple) => (tuple.first().unwrap_or(&JsData::Undefined), metadata_of(tuple.get(1))?),
        other => (other, None),
    };
    if is_nullish(Some(edit)) {
        return Ok(None);
    }
    if is_notebook_edit(edit) {
        return Ok(Some(SetEditItem::from((notebook_edit_of(edit)?, metadata))));
    }
    if let Some(snippet) = field(edit, "snippet") {
        let range = to_range(field(edit, "range").ok_or("Illegal argument: SnippetTextEdit sem range")?)?;
        let mut snippet_edit = SnippetTextEdit::new(range, SnippetString::new(opt_string(field(snippet, "value")).as_deref()));
        snippet_edit.keep_whitespace = opt_bool(field(edit, "keepWhitespace"));
        return Ok(Some(SetEditItem::from((snippet_edit, metadata))));
    }
    let text_edit = text_edit_from_js(edit).map_err(|error| error.message)?;
    Ok(Some(SetEditItem::from((text_edit, metadata))))
}

fn text_edits_to_js(edits: &[TextEdit]) -> JsData {
    JsData::Array(edits.iter().map(api_to_js).collect())
}

// ---- a classe

fn method(name: &str, length: u32, body: impl Fn(&JsData, &[JsData]) -> Result<JsData, String> + 'static) -> (String, u32, MethodCallback) {
    (name.to_owned(), length, Rc::new(body))
}

fn edit_method(
    objects: &Rc<WorkspaceEditObjects>,
    name: &str,
    length: u32,
    body: impl Fn(&mut WorkspaceEdit, &[JsData]) -> Result<JsData, String> + 'static,
) -> (String, u32, MethodCallback) {
    let objects = objects.clone();
    method(name, length, move |this, args| objects.with(this, |edit| body(edit, args)))
}

fn define_class(runner: &NodeRunner, objects: &Rc<WorkspaceEditObjects>) -> JsData {
    let methods: Vec<(String, u32, MethodCallback)> = vec![
        edit_method(objects, "replace", 4, |edit, args| {
            edit.replace(&uri_arg(args, 0, "replace: a Uri")?, to_range(required(args, 1, "replace: o Range")?)?, &text_arg(args, 2, "replace: o newText")?, metadata_of(args.get(3))?);
            Ok(JsData::Undefined)
        }),
        edit_method(objects, "insert", 4, |edit, args| {
            edit.insert(&uri_arg(args, 0, "insert: a Uri")?, to_position(required(args, 1, "insert: a Position")?)?, &text_arg(args, 2, "insert: o newText")?, metadata_of(args.get(3))?);
            Ok(JsData::Undefined)
        }),
        edit_method(objects, "delete", 3, |edit, args| {
            edit.delete(&uri_arg(args, 0, "delete: a Uri")?, to_range(required(args, 1, "delete: o Range")?)?, metadata_of(args.get(2))?);
            Ok(JsData::Undefined)
        }),
        edit_method(objects, "set", 2, |edit, args| {
            let uri = uri_arg(args, 0, "set: a Uri")?;
            match args.get(1) {
                Some(JsData::Array(items)) => {
                    let items = items.iter().map(set_item_of).collect::<Result<Vec<_>, String>>()?;
                    edit.set(&uri, items.into_iter().flatten().collect());
                }
                _ => edit.unset(&uri),
            }
            Ok(JsData::Undefined)
        }),
        edit_method(objects, "createFile", 3, |edit, args| {
            edit.create_file(uri_arg(args, 0, "createFile: a Uri")?, file_options_of(args.get(1))?, metadata_of(args.get(2))?);
            Ok(JsData::Undefined)
        }),
        edit_method(objects, "deleteFile", 3, |edit, args| {
            edit.delete_file(uri_arg(args, 0, "deleteFile: a Uri")?, file_options_of(args.get(1))?, metadata_of(args.get(2))?);
            Ok(JsData::Undefined)
        }),
        edit_method(objects, "renameFile", 4, |edit, args| {
            edit.rename_file(uri_arg(args, 0, "renameFile: a Uri de origem")?, uri_arg(args, 1, "renameFile: a Uri de destino")?, file_options_of(args.get(2))?, metadata_of(args.get(3))?);
            Ok(JsData::Undefined)
        }),
        edit_method(objects, "has", 1, |edit, args| Ok(JsData::Bool(edit.has(&uri_arg(args, 0, "has: a Uri")?)))),
        edit_method(objects, "get", 1, |edit, args| Ok(text_edits_to_js(&edit.get(&uri_arg(args, 0, "get: a Uri")?)))),
        edit_method(objects, "entries", 0, |edit, _| Ok(entries_to_js(edit))),
        edit_method(objects, "toJSON", 0, |edit, _| Ok(entries_to_js(edit))),
    ];
    let size_objects = objects.clone();
    let getters: Vec<(String, MethodCallback)> =
        vec![("size".to_owned(), Rc::new(move |this, _| size_objects.with(this, |edit| Ok(JsData::Num(edit.size() as f64)))))];
    let create_objects = objects.clone();
    let constructor = runner.define_class(ClassSpec {
        name: "WorkspaceEdit".to_owned(),
        length: 0,
        construct: Rc::new(move |_| Ok(create_objects.create())),
        methods,
        getters,
        statics: vec![],
        parent: None,
    });
    JsData::Handle(Rc::new(constructor))
}

fn entries_to_js(edit: &WorkspaceEdit) -> JsData {
    JsData::Array(edit.entries().iter().map(|(uri, edits)| JsData::Array(vec![uri_to_js(uri), text_edits_to_js(edits)])).collect())
}

/// Define a classe `WorkspaceEdit` (o `new vscode.WorkspaceEdit()`).
pub fn classes(runner: &NodeRunner, objects: &Rc<WorkspaceEditObjects>) -> Vec<(String, JsData)> {
    vec![("WorkspaceEdit".to_owned(), define_class(runner, objects))]
}

// ---- workspace.applyEdit

/// `vscode.WorkspaceEditMetadata`.
fn apply_metadata_of(value: Option<&JsData>) -> Option<WorkspaceEditMetadata> {
    match value {
        Some(metadata @ JsData::Object(_)) => Some(WorkspaceEditMetadata { is_refactoring: opt_bool(field(metadata, "isRefactoring")) }),
        _ => None,
    }
}

/// O membro `applyEdit(edit, metadata?)` do namespace `workspace`.
pub fn workspace_members(runner: &NodeRunner, bulk_edits: &Rc<ExtHostBulkEdits>, objects: &Rc<WorkspaceEditObjects>) -> Vec<(String, u32, MethodCallback)> {
    let (runner, bulk_edits, objects) = (runner.clone(), bulk_edits.clone(), objects.clone());
    vec![method("applyEdit", 2, move |_, args| {
        let edit = objects.snapshot(required(args, 0, "applyEdit: a WorkspaceEdit")?)?;
        let applied = bulk_edits.apply_workspace_edit(&edit, &host_extension(), apply_metadata_of(args.get(1)).as_ref());
        Ok(promise_to_js(&runner, applied, |applied| JsData::Bool(*applied)))
    })]
}
