//! `vscode.notebooks` (`createNotebookController`, `registerNotebookCellStatusBarItemProvider`), os membros de notebook de
//! `vscode.workspace` (`notebookDocuments`, `openNotebookDocument`, `registerNotebookSerializer`, `onDidOpenNotebookDocument`,
//! `onDidCloseNotebookDocument`, `onDidChangeNotebookDocument`, `onDidSaveNotebookDocument`, `onWillSaveNotebookDocument`), os de
//! `vscode.window` (`activeNotebookEditor`, `visibleNotebookEditors`, `showNotebookDocument`, `onDidChangeActiveNotebookEditor`,
//! `onDidChangeVisibleNotebookEditors`, `onDidChangeNotebookEditorSelection`, `onDidChangeNotebookEditorVisibleRanges`) e as classes
//! `NotebookRange`, `NotebookData`, `NotebookCellData`, `NotebookCellOutput`, `NotebookEdit`,
//! `NotebookCellOutputItem` e `NotebookCellStatusBarItem`, mais as classes nativas `NotebookDocument`, `NotebookCell`, `NotebookEditor`,
//! `NotebookController` e `NotebookCellExecution`.
//!
//! É a ponte para o `ExtHostNotebookController`, o `ExtHostNotebookDocuments`, o `ExtHostNotebookDocumentSaveParticipant` e o
//! `ExtHostNotebookKernels` portados (`extHostNotebook.ts`, `extHostNotebookDocument.ts`, `extHostNotebookDocuments.ts`,
//! `extHostNotebookDocumentSaveParticipant.ts`, `extHostNotebookKernels.ts`). O notebook, a célula, o editor, o controlador e a execução que a
//! extensão segura carregam só um `_id` (um número da [`NotebookObjects`]), com acessores no protótipo sobre o objeto vivo do host; o documento
//! de texto de uma célula é o mesmo `TextDocument` que `workspace.textDocuments` devolve. Os serviços só existem quando o binder registrou
//! `MainThreadNotebook`, `MainThreadNotebookDocuments` e `MainThreadNotebookEditors` (mais os documentos de texto, porque cada célula é um
//! documento de texto); o `createNotebookController` só existe com o cliente `MainThreadNotebookKernels` e o `onWillSaveNotebookDocument` só
//! existe com o cliente `MainThreadBulkEdits` (o participante aplica as edições que os ouvintes devolvem).
//!
//! O `executeHandler` e o `interruptHandler` do controlador, e o `provideCellStatusBarItems` do provedor da barra de status, são chamados
//! com os mesmos objetos que a extensão recebe nos outros pontos (célula, notebook, controlador, token); o provedor chega por REFERÊNCIA
//! (o `this` e os métodos de classe valem), e o `onDidChangeCellStatusBarItems` dele é assinado quando o provedor o define. O serializador de
//! `registerNotebookSerializer` também chega por referência: `deserializeNotebook` e `serializeNotebook` rodam sobre o objeto da extensão.
//!
//! Falta (o serviço do host tem, falta só a ponte, ou depende de proposta que o host nunca habilita):
//! - `NotebookEditor#replOptions` e `NotebookDocumentShowOptions#asRepl` além do que o host já entende (propostas `notebookReplDocument`);
//!   (propostas `notebookReplDocument`);
//! - `createRendererMessaging`, `NotebookController#asWebviewUri`, `onDidReceiveMessage`, `postMessage`,
//!   `variableProvider`, `createNotebookExecution`, `createNotebookControllerDetectionTask` e `registerKernelSourceActionProvider`
//!   (propostas `notebookMessaging`, `notebookVariableProvider`, `notebookExecution`, `notebookKernelSource`);
//! - `NotebookCellStatusBarItem#accessibilityInformation`, o terceiro argumento `executionError` de `NotebookCellExecution#end` e
//!   `NotebookCellExecution#executionOrder` fora de número.
//!
//! Divergências:
//! - O `vscode` é um só para todas as extensões: o controlador, o provedor, o serializador e os ouvintes de gravação são registrados em nome
//!   da extensão `je.exthost`, e as propostas de API nunca estão habilitadas (`rendererScripts` do `createNotebookController` é ignorado, como
//!   o original faz sem `notebookMessaging`; o quarto argumento `registration` de `registerNotebookSerializer` não existe).
//! - O motor entrega objeto comum por cópia, então a identidade (`a === b`) de um notebook, de uma célula, de um editor ou de um controlador
//!   não vale (cada leitura cria um objeto JavaScript novo com o mesmo `_id`); comparar por `uri` ou por `id` funciona.
//! - `NotebookCellOutputItem#data` é uma lista de números (os bytes), não um `Uint8Array`: o `Uint8Array` não atravessa o motor. O construtor
//!   e `NotebookCellOutputItem.bytes` aceitam a lista de bytes (e um `Uint8Array` copiado como objeto de índices); `text`, `json`, `stdout`,
//!   `stderr` e `error` produzem a lista. Para o serializador a ponte faz a conversão do lado JavaScript: `deserializeNotebook` recebe um
//!   `Uint8Array` de verdade (feito com `Uint8Array.from`) e o `Uint8Array` (ou lista) que `serializeNotebook` devolve vira a lista de bytes.
//! - `NotebookController#executeHandler` devolve `undefined` enquanto a extensão não atribuiu nenhum (o original devolve o tratador padrão).
//! - As tabelas de notebooks, células, editores, controladores e execuções não têm coletor (o motor não avisa quando o objeto é descartado): um
//!   notebook fechado continua legível, com `isClosed` verdadeiro.
//! - O objeto do evento de `onWillSaveNotebookDocument` (`{ notebook, reason, token, waitUntil }`) é uma cópia; `waitUntil` só vale durante o
//!   ouvinte (depois lança "waitUntil can NOT be called asynchronous") e as promessas são entregues ao evento do host quando o ouvinte retorna.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use je_base::cancellation::CancellationToken;
use je_base::dynamic::Dynamic;
use je_base::errors::{on_unexpected_error, Error};
use je_base::event::{Event, Listener};
use je_base::json::{Object, Value};
use je_base::lifecycle::{plain_disposable, DisposableRef, IDisposable};
use je_base::promise::{MaybePromise, Promise};
use je_workbench::api::common::ext_host_notebook::{
    ExtHostNotebookController, NotebookAsRepl, NotebookCellStatusBarItemProvider, NotebookCellStatusBarItems, NotebookDocumentShowOptions, NotebookSerializer,
};
use je_workbench::api::common::ext_host_notebook_document::{NotebookCell, NotebookDocument, NotebookDocumentChangeEvent};
use je_workbench::api::common::ext_host_notebook_document_save_participant::{ExtHostNotebookDocumentSaveParticipant, NotebookDocumentWillSaveEvent};
use je_workbench::api::common::ext_host_notebook_documents::ExtHostNotebookDocuments;
use je_workbench::api::common::ext_host_notebook_editor::NotebookEditor;
use je_workbench::api::common::ext_host_notebook_editors::{ExtHostNotebookEditors, NotebookEditorSelectionChangeEvent, NotebookEditorVisibleRangesChangeEvent};
use je_workbench::api::common::ext_host_notebook_kernels::{
    ExtHostNotebookKernels, NotebookCellExecution, NotebookController, NotebookExecuteHandler, NotebookInterruptHandler, NotebookSelectionChange,
};
use je_workbench::api::common::ext_host_types::{
    ErrorLike, IdOrMetadata, NotebookCellData, NotebookCellKind, NotebookCellOutput, NotebookCellOutputItem, NotebookCellStatusBarAlignment,
    NotebookCellStatusBarCommand, NotebookCellStatusBarItem, NotebookControllerAffinity2, NotebookData, NotebookDocumentContentOptions, NotebookEdit, NotebookEditorRevealType,
    NotebookRange,
};
use zjsc::api::embed_node::{ClassSpec, JsData, JsHandle, MethodCallback, NodeRunner};

use crate::api::commands::{call_settled, convert_maybe, promise_to_js};
use crate::api::disposable::DisposableTable;
use crate::api::events::{event_function, event_function_with_epilogue, EventTables};
use crate::api::file_system_events::thenable_edit;
use crate::api::js_object::{JsObjects, JsResult};
use crate::api::language_types::command_from_js;
use crate::api::languages::host_extension;
use crate::api::text_editors::TextObjects;
use crate::api::types::marked;
use crate::api::workspace_edit::WorkspaceEditObjects;
use crate::api::{getter, handle_field, handle_object, setter};
use crate::convert::{bytes_of, bytes_to_js, class_name, dynamic_to_js, exported, field, is_nullish, js_to_dynamic, opt_bool, opt_number, opt_string, string_arg, uri_from_js, uri_to_js};

/// O que `onWillSaveNotebookDocument` usa: o participante de gravação e onde moram as `WorkspaceEdit` que `waitUntil` recebe.
#[derive(Clone)]
pub struct WillSaveApi {
    pub participant: Rc<ExtHostNotebookDocumentSaveParticipant>,
    pub edits: Rc<WorkspaceEditObjects>,
}

/// Os serviços do host que os membros de notebook usam.
#[derive(Clone)]
pub struct NotebookApi {
    pub notebooks: Rc<ExtHostNotebookController>,
    /// Os eventos de mudança e de gravação dos documentos.
    pub documents: Rc<ExtHostNotebookDocuments>,
    /// Os eventos de seleção e de intervalos visíveis dos editores.
    pub editors: Rc<ExtHostNotebookEditors>,
    /// Sem o cliente `MainThreadBulkEdits` não há `onWillSaveNotebookDocument`.
    pub will_save: Option<WillSaveApi>,
    /// Sem o cliente `MainThreadNotebookKernels` não há `createNotebookController`.
    pub kernels: Option<Rc<ExtHostNotebookKernels>>,
    pub objects: Rc<NotebookObjects>,
    pub events: Rc<EventTables>,
    /// Dá o `vscode.TextDocument` de uma célula nos eventos de mudança.
    pub text: Rc<TextObjects>,
}

/// Os notebooks, as células, os editores, os controladores e as execuções que o JavaScript segura, por id (os objetos carregam só o id).
#[derive(Default)]
pub struct NotebookObjects {
    next: Cell<u32>,
    documents: RefCell<Vec<(u32, NotebookDocument)>>,
    cells: RefCell<Vec<(u32, NotebookCell)>>,
    editors: RefCell<Vec<(u32, NotebookEditor)>>,
    controllers: RefCell<HashMap<u32, Rc<NotebookController>>>,
    executions: RefCell<HashMap<u32, Rc<NotebookCellExecution>>>,
    /// O `executeHandler` e o `interruptHandler` que a extensão atribuiu, por controlador (a leitura devolve o que ela passou).
    execute_handlers: RefCell<HashMap<u32, JsData>>,
    interrupt_handlers: RefCell<HashMap<u32, JsData>>,
}

impl NotebookObjects {
    fn next_id(&self) -> u32 {
        let id = self.next.get() + 1;
        self.next.set(id);
        id
    }

    /// O `vscode.NotebookDocument` do documento: o mesmo documento volta com o mesmo `_id`.
    pub fn document_to_js(&self, document: &NotebookDocument) -> JsData {
        let known = self.documents.borrow().iter().find(|(_, candidate)| NotebookDocument::ptr_eq(candidate, document)).map(|(id, _)| *id);
        let id = known.unwrap_or_else(|| {
            let id = self.next_id();
            self.documents.borrow_mut().push((id, document.clone()));
            id
        });
        handle_object("NotebookDocument", id)
    }

    /// O `vscode.NotebookCell` da célula: a mesma célula volta com o mesmo `_id`.
    pub fn cell_to_js(&self, cell: &NotebookCell) -> JsData {
        let known = self.cells.borrow().iter().find(|(_, candidate)| NotebookCell::ptr_eq(candidate, cell)).map(|(id, _)| *id);
        let id = known.unwrap_or_else(|| {
            let id = self.next_id();
            self.cells.borrow_mut().push((id, cell.clone()));
            id
        });
        handle_object("NotebookCell", id)
    }

    /// O `vscode.NotebookEditor` do editor: o mesmo editor volta com o mesmo `_id`.
    pub fn editor_to_js(&self, editor: &NotebookEditor) -> JsData {
        let known = self.editors.borrow().iter().find(|(_, candidate)| NotebookEditor::ptr_eq(candidate, editor)).map(|(id, _)| *id);
        let id = known.unwrap_or_else(|| {
            let id = self.next_id();
            self.editors.borrow_mut().push((id, editor.clone()));
            id
        });
        handle_object("NotebookEditor", id)
    }

    fn editor_of(&self, value: &JsData) -> Result<Rc<NotebookEditor>, String> {
        let id = handle_field(value, "NotebookEditor")?;
        self.editors.borrow().iter().find(|(candidate, _)| *candidate == id).map(|(_, editor)| Rc::new(editor.clone())).ok_or_else(|| "NotebookEditor: editor desconhecido".to_owned())
    }

    fn controller_to_js(&self, controller: &NotebookController) -> JsData {
        let known = self.controllers.borrow().iter().find(|(_, candidate)| NotebookController::ptr_eq(candidate, controller)).map(|(id, _)| *id);
        match known {
            Some(id) => handle_object("NotebookController", id),
            None => self.add_controller(controller.clone(), None),
        }
    }

    fn add_controller(&self, controller: NotebookController, execute_handler: Option<JsData>) -> JsData {
        let id = self.next_id();
        self.controllers.borrow_mut().insert(id, Rc::new(controller));
        if let Some(handler) = execute_handler {
            self.execute_handlers.borrow_mut().insert(id, handler);
        }
        handle_object("NotebookController", id)
    }

    fn add_execution(&self, execution: NotebookCellExecution) -> JsData {
        let id = self.next_id();
        self.executions.borrow_mut().insert(id, Rc::new(execution));
        handle_object("NotebookCellExecution", id)
    }

    fn document_of(&self, value: &JsData) -> Result<Rc<NotebookDocument>, String> {
        let id = handle_field(value, "NotebookDocument")?;
        self.documents.borrow().iter().find(|(candidate, _)| *candidate == id).map(|(_, document)| Rc::new(document.clone())).ok_or_else(|| "NotebookDocument: documento desconhecido".to_owned())
    }

    fn cell_of(&self, value: &JsData) -> Result<Rc<NotebookCell>, String> {
        let id = handle_field(value, "NotebookCell")?;
        self.cells.borrow().iter().find(|(candidate, _)| *candidate == id).map(|(_, cell)| Rc::new(cell.clone())).ok_or_else(|| "NotebookCell: célula desconhecida".to_owned())
    }

    fn controller_of(&self, this: &JsData) -> Result<Rc<NotebookController>, String> {
        self.controllers.borrow().get(&handle_field(this, "NotebookController")?).cloned().ok_or_else(|| "NotebookController: controlador desconhecido".to_owned())
    }

    fn execution_of(&self, this: &JsData) -> Result<Rc<NotebookCellExecution>, String> {
        self.executions.borrow().get(&handle_field(this, "NotebookCellExecution")?).cloned().ok_or_else(|| "NotebookCellExecution: execução desconhecida".to_owned())
    }

    /// `cell?: NotebookCell`: ausente, `undefined` e `null` são "a célula da execução".
    fn optional_cell(&self, value: Option<&JsData>) -> Result<Option<NotebookCell>, String> {
        match value {
            Some(cell) if !is_nullish(Some(cell)) => self.cell_of(cell).map(|cell| Some((*cell).clone())),
            _ => Ok(None),
        }
    }
}

// ---- os valores de saída e de dados

fn output_item_of(value: &JsData) -> Result<NotebookCellOutputItem, String> {
    let mime = opt_string(field(value, "mime")).ok_or("NotebookCellOutputItem.mime precisa ser texto")?;
    NotebookCellOutputItem::new(bytes_of(field(value, "data"))?, &mime).map_err(|error| error.message)
}

fn output_item_fields(item: &NotebookCellOutputItem) -> Vec<(String, JsData)> {
    vec![
        ("mime".to_owned(), JsData::Str(item.mime().to_owned())),
        ("data".to_owned(), bytes_to_js(&item.data)),
    ]
}

fn output_item_to_js(item: &NotebookCellOutputItem) -> JsData {
    marked("NotebookCellOutputItem", output_item_fields(item))
}

fn dynamic_field(value: &JsData, name: &str) -> Option<Dynamic> {
    field(value, name).filter(|candidate| !is_nullish(Some(*candidate))).map(js_to_dynamic)
}

fn output_of(value: &JsData) -> Result<NotebookCellOutput, String> {
    let items = match field(value, "items") {
        Some(JsData::Array(items)) => items.iter().map(output_item_of).collect::<Result<Vec<_>, String>>()?,
        _ => return Err("NotebookCellOutput.items precisa ser uma lista de NotebookCellOutputItem".to_owned()),
    };
    Ok(NotebookCellOutput::new(items, opt_string(field(value, "id")).map(IdOrMetadata::Id), dynamic_field(value, "metadata")))
}

fn output_fields(output: &NotebookCellOutput) -> Vec<(String, JsData)> {
    let mut fields = vec![
        ("id".to_owned(), JsData::Str(output.id.clone())),
        ("items".to_owned(), JsData::Array(output.items.iter().map(output_item_to_js).collect())),
    ];
    if let Some(metadata) = &output.metadata {
        fields.push(("metadata".to_owned(), dynamic_to_js(metadata)));
    }
    fields
}

fn output_to_js(output: &NotebookCellOutput) -> JsData {
    marked("NotebookCellOutput", output_fields(output))
}

/// `NotebookCellOutput | readonly NotebookCellOutput[]`.
fn outputs_of(value: Option<&JsData>) -> Result<Vec<NotebookCellOutput>, String> {
    match value {
        Some(JsData::Array(items)) => items.iter().map(output_of).collect(),
        Some(single @ JsData::Object(_)) => Ok(vec![output_of(single)?]),
        _ => Err("os outputs precisam ser um NotebookCellOutput ou uma lista deles".to_owned()),
    }
}

fn cell_data_of(value: &JsData) -> Result<NotebookCellData, String> {
    let kind = opt_number(field(value, "kind")).and_then(|kind| NotebookCellKind::from_value(kind as i32)).ok_or("NotebookCellData.kind precisa ser um NotebookCellKind")?;
    let outputs = match field(value, "outputs") {
        Some(JsData::Array(items)) => Some(items.iter().map(output_of).collect::<Result<Vec<_>, String>>()?),
        _ => None,
    };
    Ok(NotebookCellData::new(
        kind,
        opt_string(field(value, "value")).ok_or("NotebookCellData.value precisa ser texto")?,
        opt_string(field(value, "languageId")).ok_or("NotebookCellData.languageId precisa ser texto")?,
        opt_string(field(value, "mime")),
        outputs,
        dynamic_field(value, "metadata"),
        dynamic_field(value, "executionSummary"),
    ))
}

/// `vscode.NotebookData`.
fn notebook_data_of(value: &JsData) -> Result<NotebookData, String> {
    let cells = match field(value, "cells") {
        Some(JsData::Array(items)) => items.iter().map(cell_data_of).collect::<Result<Vec<_>, String>>()?,
        _ => return Err("NotebookData.cells precisa ser uma lista de NotebookCellData".to_owned()),
    };
    let mut data = NotebookData::new(cells);
    data.metadata = dynamic_field(value, "metadata");
    Ok(data)
}

/// `vscode.NotebookRange` (uma instância ou um literal `{ start, end }`).
fn notebook_range_of(value: &JsData) -> Result<NotebookRange, String> {
    match (opt_number(field(value, "start")), opt_number(field(value, "end"))) {
        (Some(start), Some(end)) => NotebookRange::new(start, end).map_err(|error| error.message),
        _ => Err("Illegal argument: esperava um NotebookRange".to_owned()),
    }
}

fn notebook_range_fields(range: &NotebookRange) -> JsData {
    JsData::Object(vec![("start".to_owned(), JsData::Num(range.start())), ("end".to_owned(), JsData::Num(range.end()))])
}

/// O valor JSON de um valor do motor (a `value` de `NotebookCellOutputItem.json`): o que `JSON.stringify` serializaria.
fn json_value_of(value: &JsData) -> Value {
    match value {
        JsData::Undefined | JsData::Null | JsData::Handle(_) => Value::Null,
        JsData::Bool(flag) => Value::Bool(*flag),
        JsData::Num(number) => Value::Number(*number),
        JsData::Str(text) => Value::from(text.as_str()),
        JsData::Array(items) => Value::Array(items.iter().map(json_value_of).collect()),
        JsData::Object(fields) => Value::Object(
            fields
                .iter()
                .filter(|(key, item)| key != zjsc::api::embed_node::CLASS_KEY && !matches!(item, JsData::Undefined))
                .map(|(key, item)| (key.as_str(), json_value_of(item)))
                .collect::<Object>(),
        ),
    }
}

fn cell_data_to_js(cell: &NotebookCellData) -> JsData {
    let mut fields = vec![
        ("kind".to_owned(), JsData::Num(f64::from(cell.kind.value()))),
        ("value".to_owned(), JsData::Str(cell.value.clone())),
        ("languageId".to_owned(), JsData::Str(cell.language_id.clone())),
    ];
    if let Some(mime) = &cell.mime {
        fields.push(("mime".to_owned(), JsData::Str(mime.clone())));
    }
    fields.push(("outputs".to_owned(), JsData::Array(cell.outputs.iter().map(output_to_js).collect())));
    if let Some(metadata) = &cell.metadata {
        fields.push(("metadata".to_owned(), dynamic_to_js(metadata)));
    }
    if let Some(summary) = &cell.execution_summary {
        fields.push(("executionSummary".to_owned(), dynamic_to_js(summary)));
    }
    marked("NotebookCellData", fields)
}

/// O `vscode.NotebookData` que o `serializeNotebook` da extensão recebe.
fn notebook_data_to_js(data: &NotebookData) -> JsData {
    let mut fields = vec![("cells".to_owned(), JsData::Array(data.cells.iter().map(cell_data_to_js).collect()))];
    if let Some(metadata) = &data.metadata {
        fields.push(("metadata".to_owned(), dynamic_to_js(metadata)));
    }
    marked("NotebookData", fields)
}

/// `{ [key: string]: boolean }` das opções de conteúdo: só as chaves com valor booleano.
fn flag_map_of(value: Option<&JsData>) -> Option<HashMap<String, bool>> {
    match value {
        Some(JsData::Object(fields)) => Some(fields.iter().filter_map(|(key, item)| opt_bool(Some(item)).map(|flag| (key.clone(), flag))).collect()),
        _ => None,
    }
}

/// `vscode.NotebookDocumentContentOptions`.
fn content_options_of(value: Option<&JsData>) -> Option<NotebookDocumentContentOptions> {
    let value = value.filter(|options| !is_nullish(Some(*options)))?;
    Some(NotebookDocumentContentOptions {
        transient_outputs: opt_bool(field(value, "transientOutputs")),
        transient_cell_metadata: flag_map_of(field(value, "transientCellMetadata")),
        transient_document_metadata: flag_map_of(field(value, "transientDocumentMetadata")),
        cell_content_metadata: flag_map_of(field(value, "cellContentMetadata")),
    })
}

/// Os valores de uma mudança de documento como o `vscode.NotebookDocumentChangeEvent`.
fn change_event_to_js(api: &NotebookApi, event: &NotebookDocumentChangeEvent) -> JsData {
    let cell_changes = event
        .cell_changes
        .iter()
        .map(|change| {
            let mut fields = vec![("cell".to_owned(), api.objects.cell_to_js(&change.cell))];
            if let Some(document) = &change.document {
                fields.push(("document".to_owned(), api.text.document_to_js(document)));
            }
            if let Some(metadata) = &change.metadata {
                fields.push(("metadata".to_owned(), dynamic_to_js(metadata)));
            }
            if let Some(outputs) = &change.outputs {
                fields.push(("outputs".to_owned(), JsData::Array(outputs.iter().map(output_to_js).collect())));
            }
            if let Some(summary) = &change.execution_summary {
                fields.push(("executionSummary".to_owned(), dynamic_to_js(summary)));
            }
            JsData::Object(fields)
        })
        .collect();
    let content_changes = event
        .content_changes
        .iter()
        .map(|change| {
            JsData::Object(vec![
                ("range".to_owned(), notebook_range_fields(&change.range)),
                ("addedCells".to_owned(), JsData::Array(change.added_cells.iter().map(|cell| api.objects.cell_to_js(cell)).collect())),
                ("removedCells".to_owned(), JsData::Array(change.removed_cells.iter().map(|cell| api.objects.cell_to_js(cell)).collect())),
            ])
        })
        .collect();
    let mut fields = vec![("notebook".to_owned(), api.objects.document_to_js(&event.notebook))];
    if let Some(metadata) = &event.metadata {
        fields.push(("metadata".to_owned(), dynamic_to_js(metadata)));
    }
    fields.push(("cellChanges".to_owned(), JsData::Array(cell_changes)));
    fields.push(("contentChanges".to_owned(), JsData::Array(content_changes)));
    JsData::Object(fields)
}

// ---- o serializador da extensão

/// Adapta o `vscode.NotebookSerializer` da extensão para o motor: `deserializeNotebook` recebe um `Uint8Array` de verdade e
/// `serializeNotebook` devolve a lista de bytes (o `Uint8Array` não atravessa o motor).
const SERIALIZER_ADAPTER: &str = "(function (serializer, sink) { sink({ \
    deserializeNotebook: function (bytes, token) { return serializer.deserializeNotebook(Uint8Array.from(bytes), token); }, \
    serializeNotebook: function (data, token) { \
      return Promise.resolve(serializer.serializeNotebook(data, token)).then(function (bytes) { return Array.from(bytes); }); } }); })";

/// O `vscode.NotebookSerializer` da extensão (por referência, com o `this` e os métodos da classe).
struct JsSerializer {
    js: JsObjects,
    events: Rc<EventTables>,
    /// O adaptador de [`SERIALIZER_ADAPTER`] sobre o serializador.
    target: JsData,
}

impl JsSerializer {
    fn new(js: &JsObjects, events: &Rc<EventTables>, serializer: &Rc<JsHandle>) -> Result<Rc<dyn NotebookSerializer>, String> {
        let original = JsData::Handle(serializer.clone());
        for method in ["deserializeNotebook", "serializeNotebook"] {
            if !js.is_function(&original, method) {
                return Err(format!("registerNotebookSerializer: o serializador precisa de {method}"));
            }
        }
        let adapter = js.runner().eval(SERIALIZER_ADAPTER, "je-exthost/notebook-serializer.js")?;
        let target = js.capture(&adapter, &[original])?;
        Ok(Rc::new(Self { js: js.clone(), events: events.clone(), target }))
    }
}

impl NotebookSerializer for JsSerializer {
    fn deserialize_notebook(&self, content: &[u8], token: CancellationToken) -> MaybePromise<NotebookData> {
        let args = vec![bytes_to_js(content), self.events.token_to_js(token)];
        let runner = self.js.runner().clone();
        convert_maybe(self.js.invoke(&self.target, "deserializeNotebook", args), move |result| match result {
            JsResult::Single(data) => notebook_data_of(&exported(&runner, &data)).map_err(Error::new),
            _ => Err(Error::new("deserializeNotebook precisa devolver um NotebookData")),
        })
    }

    fn serialize_notebook(&self, data: &NotebookData, token: CancellationToken) -> MaybePromise<Vec<u8>> {
        let args = vec![notebook_data_to_js(data), self.events.token_to_js(token)];
        convert_maybe(self.js.invoke(&self.target, "serializeNotebook", args), |result| match result {
            JsResult::Many(items) => bytes_of(Some(&JsData::Array(items))).map_err(Error::new),
            _ => Err(Error::new("serializeNotebook precisa devolver um Uint8Array")),
        })
    }
}

// ---- os tratadores da extensão

/// O `executeHandler(cells, notebook, controller)` da extensão.
fn execute_handler_of(runner: &NodeRunner, objects: &Rc<NotebookObjects>, function: &Rc<JsHandle>) -> NotebookExecuteHandler {
    let (runner, objects, function) = (runner.clone(), objects.clone(), function.clone());
    Rc::new(move |cells: Vec<NotebookCell>, notebook: NotebookDocument, controller: NotebookController| {
        let args = [
            JsData::Array(cells.iter().map(|cell| objects.cell_to_js(cell)).collect()),
            objects.document_to_js(&notebook),
            objects.controller_to_js(&controller),
        ];
        convert_maybe(call_settled(&runner, &function, &args), |_| Ok(()))
    })
}

/// O `interruptHandler(notebook)` da extensão.
fn interrupt_handler_of(runner: &NodeRunner, objects: &Rc<NotebookObjects>, function: &Rc<JsHandle>) -> NotebookInterruptHandler {
    let (runner, objects, function) = (runner.clone(), objects.clone(), function.clone());
    Rc::new(move |notebook: NotebookDocument| convert_maybe(call_settled(&runner, &function, &[objects.document_to_js(&notebook)]), |_| Ok(())))
}

fn status_bar_item_of(value: &JsData) -> Result<NotebookCellStatusBarItem, String> {
    let text = opt_string(field(value, "text")).ok_or("NotebookCellStatusBarItem.text precisa ser texto")?;
    let alignment = opt_number(field(value, "alignment"))
        .and_then(|alignment| NotebookCellStatusBarAlignment::from_value(alignment as i32))
        .ok_or("NotebookCellStatusBarItem.alignment precisa ser um NotebookCellStatusBarAlignment")?;
    let mut item = NotebookCellStatusBarItem::new(text, alignment);
    item.command = match field(value, "command") {
        Some(JsData::Str(id)) => Some(NotebookCellStatusBarCommand::Id(id.clone())),
        Some(command @ JsData::Object(_)) => Some(NotebookCellStatusBarCommand::Command(command_from_js(command).map_err(|error| error.message)?)),
        _ => None,
    };
    item.tooltip = opt_string(field(value, "tooltip"));
    item.priority = opt_number(field(value, "priority"));
    Ok(item)
}

/// `ProviderResult<NotebookCellStatusBarItem | NotebookCellStatusBarItem[]>` que o provedor devolveu.
fn status_bar_items_of(runner: &NodeRunner, result: JsResult) -> Result<Option<NotebookCellStatusBarItems>, String> {
    match result {
        JsResult::Nullish => Ok(None),
        JsResult::Single(item) => Ok(Some(NotebookCellStatusBarItems::One(status_bar_item_of(&exported(runner, &item))?))),
        JsResult::Many(items) => items
            .iter()
            .map(|item| status_bar_item_of(&exported(runner, item)))
            .collect::<Result<Vec<_>, String>>()
            .map(|items| Some(NotebookCellStatusBarItems::Many(items))),
    }
}

/// O `vscode.NotebookCellStatusBarItemProvider` da extensão (por referência, com o `this` e os métodos da classe).
struct JsStatusBarProvider {
    js: JsObjects,
    table: Rc<DisposableTable>,
    objects: Rc<NotebookObjects>,
    events: Rc<EventTables>,
    target: JsData,
    /// A função de evento `onDidChangeCellStatusBarItems` do provedor, se ele a define.
    change_event: Option<Rc<JsHandle>>,
}

impl JsStatusBarProvider {
    fn new(js: &JsObjects, table: &Rc<DisposableTable>, api: &NotebookApi, provider: &Rc<JsHandle>) -> Result<Rc<dyn NotebookCellStatusBarItemProvider>, String> {
        let target = JsData::Handle(provider.clone());
        if !js.is_function(&target, "provideCellStatusBarItems") {
            return Err("registerNotebookCellStatusBarItemProvider: o provedor precisa de provideCellStatusBarItems".to_owned());
        }
        let change_event = match js.property(&target, "onDidChangeCellStatusBarItems") {
            JsData::Handle(event) if js.is_function(&target, "onDidChangeCellStatusBarItems") => Some(event),
            _ => None,
        };
        Ok(Rc::new(Self { js: js.clone(), table: table.clone(), objects: api.objects.clone(), events: api.events.clone(), target, change_event }))
    }
}

impl NotebookCellStatusBarItemProvider for JsStatusBarProvider {
    fn on_did_change_cell_status_bar_items(&self) -> Option<Event<()>> {
        let event = self.change_event.clone()?;
        let (js, table) = (self.js.clone(), self.table.clone());
        Some(Event::new(move |listener: Listener<()>| -> DisposableRef {
            match js.listen(&event, move |_| listener(&())) {
                Ok(Some(id)) => {
                    let table = table.clone();
                    plain_disposable(move || table.dispose(id))
                }
                Ok(None) => plain_disposable(|| {}),
                Err(message) => {
                    on_unexpected_error(&Error::new(message));
                    plain_disposable(|| {})
                }
            }
        }))
    }

    fn provide_cell_status_bar_items(&self, cell: &NotebookCell, token: CancellationToken) -> MaybePromise<Option<NotebookCellStatusBarItems>> {
        let args = vec![self.objects.cell_to_js(cell), self.events.token_to_js(token)];
        let runner = self.js.runner().clone();
        convert_maybe(self.js.invoke(&self.target, "provideCellStatusBarItems", args), move |result| status_bar_items_of(&runner, result).map_err(Error::new))
    }
}

// ---- as classes nativas

fn define_document(runner: &NodeRunner, objects: &Rc<NotebookObjects>) {
    let get = |name: &str, read: fn(&NotebookObjects, &Rc<NotebookDocument>) -> JsData| getter(objects, name, NotebookObjects::document_of, read);
    let getters: Vec<(String, MethodCallback)> = vec![
        get("uri", |_, document| uri_to_js(document.uri())),
        get("version", |_, document| JsData::Num(document.version() as f64)),
        get("notebookType", |_, document| JsData::Str(document.notebook_type().to_owned())),
        get("isDirty", |_, document| JsData::Bool(document.is_dirty())),
        get("isUntitled", |_, document| JsData::Bool(document.is_untitled())),
        get("isClosed", |_, document| JsData::Bool(document.is_closed())),
        get("metadata", |_, document| dynamic_to_js(&document.metadata())),
        get("cellCount", |_, document| JsData::Num(document.cell_count() as f64)),
    ];
    let (at_objects, cells_objects, save_runner) = (objects.clone(), objects.clone(), runner.clone());
    let (at_lookup, cells_lookup, save_lookup) = (objects.clone(), objects.clone(), objects.clone());
    let methods: Vec<(String, u32, MethodCallback)> = vec![
        (
            "cellAt".to_owned(),
            1,
            Rc::new(move |this, args| {
                let document = at_lookup.document_of(this)?;
                let index = opt_number(args.first()).ok_or("cellAt: o índice precisa ser número")?;
                document.cell_at(index).map(|cell| at_objects.cell_to_js(&cell)).map_err(|error| error.message)
            }),
        ),
        (
            "getCells".to_owned(),
            1,
            Rc::new(move |this, args| {
                let document = cells_lookup.document_of(this)?;
                let range = match args.first() {
                    Some(range) if !is_nullish(Some(range)) => Some(notebook_range_of(range)?),
                    _ => None,
                };
                Ok(JsData::Array(document.get_cells(range.as_ref()).iter().map(|cell| cells_objects.cell_to_js(cell)).collect()))
            }),
        ),
        (
            "save".to_owned(),
            0,
            Rc::new(move |this, _| {
                let document = save_lookup.document_of(this)?;
                Ok(promise_to_js(&save_runner, document.save(), |saved| JsData::Bool(*saved)))
            }),
        ),
    ];
    runner.define_accessor_class(
        ClassSpec {
            name: "NotebookDocument".to_owned(),
            length: 0,
            construct: Rc::new(|_| Err("NotebookDocument não tem construtor público: use workspace.openNotebookDocument".to_owned())),
            methods,
            getters,
            statics: vec![],
            parent: None,
        },
        vec![],
    );
}

fn define_cell(runner: &NodeRunner, objects: &Rc<NotebookObjects>, text: &Rc<TextObjects>) {
    let text = text.clone();
    let get = |name: &str, read: fn(&NotebookObjects, &Rc<NotebookCell>) -> JsData| getter(objects, name, NotebookObjects::cell_of, read);
    let getters: Vec<(String, MethodCallback)> = vec![
        get("index", |_, cell| JsData::Num(cell.index())),
        get("notebook", |objects, cell| objects.document_to_js(&cell.notebook())),
        get("kind", |_, cell| JsData::Num(f64::from(cell.kind().value()))),
        getter(objects, "document", NotebookObjects::cell_of, move |_, cell| text.document_to_js(&cell.document())),
        get("metadata", |_, cell| dynamic_to_js(&cell.metadata())),
        get("outputs", |_, cell| JsData::Array(cell.outputs().iter().map(output_to_js).collect())),
        get("executionSummary", |_, cell| dynamic_to_js(&cell.execution_summary())),
    ];
    runner.define_accessor_class(
        ClassSpec {
            name: "NotebookCell".to_owned(),
            length: 0,
            construct: Rc::new(|_| Err("NotebookCell não tem construtor público".to_owned())),
            methods: vec![],
            getters,
            statics: vec![],
            parent: None,
        },
        vec![],
    );
}

fn promise_unit(runner: &NodeRunner, result: Result<Promise<()>, Error>) -> Result<JsData, String> {
    result.map(|promise| promise_to_js(runner, promise, |()| JsData::Undefined)).map_err(|error| error.message)
}

fn define_execution(runner: &NodeRunner, objects: &Rc<NotebookObjects>, events: &Rc<EventTables>) {
    let token_events = events.clone();
    let getters: Vec<(String, MethodCallback)> = vec![
        getter(objects, "token", NotebookObjects::execution_of, move |_, execution| token_events.token_to_js(execution.token())),
        getter(objects, "cell", NotebookObjects::execution_of, |objects, execution| objects.cell_to_js(&execution.cell())),
        getter(objects, "executionOrder", NotebookObjects::execution_of, |_, execution| execution.execution_order().map_or(JsData::Undefined, JsData::Num)),
    ];
    let setters: Vec<(String, MethodCallback)> = vec![setter(objects, "executionOrder", NotebookObjects::execution_of, |_, execution, value| {
        execution.set_execution_order(opt_number(Some(value)));
        Ok(())
    })];
    let method = |name: &str, length: u32, body: fn(&NotebookCellExecution, &NodeRunner, &NotebookObjects, &[JsData]) -> Result<JsData, String>| -> (String, u32, MethodCallback) {
        let (objects, runner) = (objects.clone(), runner.clone());
        (
            name.to_owned(),
            length,
            Rc::new(move |this, args| {
                let execution = objects.execution_of(this)?;
                body(&execution, &runner, &objects, args)
            }),
        )
    };
    let methods: Vec<(String, u32, MethodCallback)> = vec![
        method("start", 1, |execution, _, _, args| {
            execution.start(opt_number(args.first())).map_err(|error| error.message)?;
            Ok(JsData::Undefined)
        }),
        method("end", 2, |execution, _, _, args| {
            execution.end(opt_bool(args.first()), opt_number(args.get(1)), None).map_err(|error| error.message)?;
            Ok(JsData::Undefined)
        }),
        method("clearOutput", 1, |execution, runner, objects, args| promise_unit(runner, execution.clear_output(objects.optional_cell(args.first())?.as_ref()))),
        method("appendOutput", 2, |execution, runner, objects, args| {
            promise_unit(runner, execution.append_output(outputs_of(args.first())?, objects.optional_cell(args.get(1))?.as_ref()))
        }),
        method("replaceOutput", 2, |execution, runner, objects, args| {
            promise_unit(runner, execution.replace_output(outputs_of(args.first())?, objects.optional_cell(args.get(1))?.as_ref()))
        }),
        method("appendOutputItems", 2, |execution, runner, _, args| {
            promise_unit(runner, execution.append_output_items(item_list_of(args.first())?, &output_of(args.get(1).ok_or("appendOutputItems: falta o output")?)?))
        }),
        method("replaceOutputItems", 2, |execution, runner, _, args| {
            promise_unit(runner, execution.replace_output_items(item_list_of(args.first())?, &output_of(args.get(1).ok_or("replaceOutputItems: falta o output")?)?))
        }),
    ];
    runner.define_accessor_class(
        ClassSpec {
            name: "NotebookCellExecution".to_owned(),
            length: 0,
            construct: Rc::new(|_| Err("NotebookCellExecution não tem construtor público: use NotebookController#createNotebookCellExecution".to_owned())),
            methods,
            getters,
            statics: vec![],
            parent: None,
        },
        setters,
    );
}

/// `NotebookCellOutputItem | readonly NotebookCellOutputItem[]`.
fn item_list_of(value: Option<&JsData>) -> Result<Vec<NotebookCellOutputItem>, String> {
    match value {
        Some(JsData::Array(items)) => items.iter().map(output_item_of).collect(),
        Some(single @ JsData::Object(_)) => Ok(vec![output_item_of(single)?]),
        _ => Err("os itens precisam ser um NotebookCellOutputItem ou uma lista deles".to_owned()),
    }
}

fn define_controller(runner: &NodeRunner, objects: &Rc<NotebookObjects>, table: &Rc<DisposableTable>) {
    let (event_runner, event_table, event_objects) = (runner.clone(), table.clone(), objects.clone());
    let get = |name: &str, read: fn(&NotebookObjects, &Rc<NotebookController>) -> JsData| getter(objects, name, NotebookObjects::controller_of, read);
    let getters: Vec<(String, MethodCallback)> = vec![
        get("id", |_, controller| JsData::Str(controller.id().to_owned())),
        get("notebookType", |_, controller| JsData::Str(controller.notebook_type())),
        get("label", |_, controller| JsData::Str(controller.label())),
        get("detail", |_, controller| JsData::Str(controller.detail())),
        get("description", |_, controller| JsData::Str(controller.description())),
        get("supportedLanguages", |_, controller| {
            controller.supported_languages().map_or(JsData::Undefined, |languages| JsData::Array(languages.into_iter().map(JsData::Str).collect()))
        }),
        get("supportsExecutionOrder", |_, controller| JsData::Bool(controller.supports_execution_order())),
        get("rendererScripts", |_, controller| {
            JsData::Array(
                controller
                    .renderer_scripts()
                    .iter()
                    .map(|script| {
                        JsData::Object(vec![
                            ("uri".to_owned(), uri_to_js(&script.uri)),
                            ("provides".to_owned(), JsData::Array(script.provides.iter().cloned().map(JsData::Str).collect())),
                        ])
                    })
                    .collect(),
            )
        }),
        get("executeHandler", |objects, controller| objects.execute_handlers.borrow().get(&handle_id_of(objects, controller)).cloned().unwrap_or(JsData::Undefined)),
        get("interruptHandler", |objects, controller| objects.interrupt_handlers.borrow().get(&handle_id_of(objects, controller)).cloned().unwrap_or(JsData::Undefined)),
        getter(objects, "onDidChangeSelectedNotebooks", NotebookObjects::controller_of, move |_, controller| {
            let objects = event_objects.clone();
            event_function(&event_runner, &event_table, "onDidChangeSelectedNotebooks", controller.on_did_change_selected_notebooks(), move |change: &NotebookSelectionChange| {
                JsData::Object(vec![("selected".to_owned(), JsData::Bool(change.selected)), ("notebook".to_owned(), objects.document_to_js(&change.notebook))])
            })
        }),
    ];

    let (execute_runner, interrupt_runner) = (runner.clone(), runner.clone());
    let (execute_objects, interrupt_objects) = (objects.clone(), objects.clone());
    let setters: Vec<(String, MethodCallback)> = vec![
        setter(objects, "label", NotebookObjects::controller_of, |_, controller, value| {
            controller.set_label(opt_string(Some(value)).as_deref());
            Ok(())
        }),
        setter(objects, "detail", NotebookObjects::controller_of, |_, controller, value| {
            controller.set_detail(opt_string(Some(value)));
            Ok(())
        }),
        setter(objects, "description", NotebookObjects::controller_of, |_, controller, value| {
            controller.set_description(opt_string(Some(value)));
            Ok(())
        }),
        setter(objects, "supportedLanguages", NotebookObjects::controller_of, |_, controller, value| {
            controller.set_supported_languages(match value {
                JsData::Array(items) => Some(items.iter().filter_map(|item| opt_string(Some(item))).collect()),
                _ => None,
            });
            Ok(())
        }),
        setter(objects, "supportsExecutionOrder", NotebookObjects::controller_of, |_, controller, value| {
            controller.set_supports_execution_order(opt_bool(Some(value)));
            Ok(())
        }),
        setter(objects, "executeHandler", NotebookObjects::controller_of, move |objects, controller, value| {
            let id = handle_id_of(objects, controller);
            match value {
                JsData::Handle(function) => {
                    controller.set_execute_handler(Some(execute_handler_of(&execute_runner, &execute_objects, function)));
                    objects.execute_handlers.borrow_mut().insert(id, value.clone());
                }
                _ => {
                    controller.set_execute_handler(None);
                    objects.execute_handlers.borrow_mut().remove(&id);
                }
            }
            Ok(())
        }),
        setter(objects, "interruptHandler", NotebookObjects::controller_of, move |objects, controller, value| {
            let id = handle_id_of(objects, controller);
            match value {
                JsData::Handle(function) => {
                    controller.set_interrupt_handler(Some(interrupt_handler_of(&interrupt_runner, &interrupt_objects, function)));
                    objects.interrupt_handlers.borrow_mut().insert(id, value.clone());
                }
                _ => {
                    controller.set_interrupt_handler(None);
                    objects.interrupt_handlers.borrow_mut().remove(&id);
                }
            }
            Ok(())
        }),
    ];

    let (create_objects, affinity_objects, dispose_objects) = (objects.clone(), objects.clone(), objects.clone());
    let methods: Vec<(String, u32, MethodCallback)> = vec![
        (
            "createNotebookCellExecution".to_owned(),
            1,
            Rc::new(move |this, args| {
                let controller = create_objects.controller_of(this)?;
                let cell = create_objects.cell_of(args.first().ok_or("createNotebookCellExecution: falta a célula")?)?;
                let execution = controller.create_notebook_cell_execution(&cell).map_err(|error| error.message)?;
                Ok(create_objects.add_execution(execution))
            }),
        ),
        (
            "updateNotebookAffinity".to_owned(),
            2,
            Rc::new(move |this, args| {
                let controller = affinity_objects.controller_of(this)?;
                let notebook = affinity_objects.document_of(args.first().ok_or("updateNotebookAffinity: falta o notebook")?)?;
                let affinity = opt_number(args.get(1))
                    .and_then(|affinity| NotebookControllerAffinity2::from_value(affinity as i32))
                    .ok_or("updateNotebookAffinity: a prioridade precisa ser um NotebookControllerAffinity")?;
                controller.update_notebook_affinity(&notebook, affinity).map_err(|error| error.message)?;
                Ok(JsData::Undefined)
            }),
        ),
        (
            "dispose".to_owned(),
            0,
            Rc::new(move |this, _| {
                dispose_objects.controller_of(this)?.dispose();
                Ok(JsData::Undefined)
            }),
        ),
    ];
    runner.define_accessor_class(
        ClassSpec {
            name: "NotebookController".to_owned(),
            length: 0,
            construct: Rc::new(|_| Err("NotebookController não tem construtor público: use notebooks.createNotebookController".to_owned())),
            methods,
            getters,
            statics: vec![],
            parent: None,
        },
        setters,
    );
}

/// O id (na tabela) do controlador, para achar o tratador que a extensão atribuiu.
fn handle_id_of(objects: &NotebookObjects, controller: &Rc<NotebookController>) -> u32 {
    objects.controllers.borrow().iter().find(|(_, candidate)| NotebookController::ptr_eq(candidate, controller)).map_or(0, |(id, _)| *id)
}

/// Define as classes nativas `NotebookDocument`, `NotebookCell`, `NotebookEditor`, `NotebookController` e `NotebookCellExecution` (protótipos
/// dos objetos que o host entrega). `text` é a tabela que dá o `vscode.TextDocument` de uma célula.
pub fn define_classes(runner: &NodeRunner, objects: &Rc<NotebookObjects>, text: &Rc<TextObjects>, table: &Rc<DisposableTable>, events: &Rc<EventTables>) {
    define_document(runner, objects);
    define_cell(runner, objects, text);
    define_editor(runner, objects);
    define_controller(runner, objects, table);
    define_execution(runner, objects, events);
}

fn define_editor(runner: &NodeRunner, objects: &Rc<NotebookObjects>) {
    let get = |name: &str, read: fn(&NotebookObjects, &Rc<NotebookEditor>) -> JsData| getter(objects, name, NotebookObjects::editor_of, read);
    let getters: Vec<(String, MethodCallback)> = vec![
        get("notebook", |objects, editor| objects.document_to_js(&editor.notebook())),
        get("selection", |_, editor| editor.selection().map_or(JsData::Undefined, |range| notebook_range_fields(&range))),
        get("selections", |_, editor| JsData::Array(editor.selections().iter().map(notebook_range_fields).collect())),
        get("visibleRanges", |_, editor| JsData::Array(editor.visible_ranges().iter().map(notebook_range_fields).collect())),
        get("viewColumn", |_, editor| editor.view_column().map_or(JsData::Undefined, JsData::Num)),
    ];
    let setters: Vec<(String, MethodCallback)> = vec![
        setter(objects, "selection", NotebookObjects::editor_of, |_, editor, value| {
            editor.set_selection(notebook_range_of(value)?);
            Ok(())
        }),
        setter(objects, "selections", NotebookObjects::editor_of, |_, editor, value| {
            let JsData::Array(items) = value else {
                return Err("NotebookEditor.selections precisa ser uma lista de NotebookRange".to_owned());
            };
            editor.set_selections(items.iter().map(notebook_range_of).collect::<Result<Vec<_>, String>>()?);
            Ok(())
        }),
    ];
    let reveal_objects = objects.clone();
    let methods: Vec<(String, u32, MethodCallback)> = vec![(
        "revealRange".to_owned(),
        2,
        Rc::new(move |this, args| {
            let editor = reveal_objects.editor_of(this)?;
            let range = notebook_range_of(args.first().ok_or("revealRange: falta o intervalo")?)?;
            let reveal_type = match args.get(1) {
                Some(value) if !is_nullish(Some(value)) => {
                    Some(opt_number(Some(value)).and_then(|kind| NotebookEditorRevealType::from_value(kind as i32)).ok_or("revealRange: o tipo precisa ser um NotebookEditorRevealType")?)
                }
                _ => None,
            };
            editor.reveal_range(&range, reveal_type);
            Ok(JsData::Undefined)
        }),
    )];
    runner.define_accessor_class(
        ClassSpec {
            name: "NotebookEditor".to_owned(),
            length: 0,
            construct: Rc::new(|_| Err("NotebookEditor não tem construtor público: use window.showNotebookDocument".to_owned())),
            methods,
            getters,
            statics: vec![],
            parent: None,
        },
        setters,
    );
}

// ---- as classes de valor

/// As classes de valor e a lista de membros de `vscode` que elas ocupam (os enums estão em `enums`).
pub fn classes(runner: &NodeRunner) -> Vec<(String, JsData)> {
    let range = runner.define_class(ClassSpec {
        name: "NotebookRange".to_owned(),
        length: 2,
        construct: Rc::new(|args| {
            let (Some(start), Some(end)) = (opt_number(args.first()), opt_number(args.get(1))) else {
                return Err("Illegal argument: start e end precisam ser números".to_owned());
            };
            Ok(notebook_range_fields(&NotebookRange::new(start, end).map_err(|error| error.message)?))
        }),
        methods: vec![(
            "with".to_owned(),
            1,
            Rc::new(|this, args| {
                let range = notebook_range_of(this)?;
                let change = args.first().unwrap_or(&JsData::Undefined);
                Ok(notebook_range_fields(&range.with(opt_number(field(change, "start")), opt_number(field(change, "end"))).map_err(|error| error.message)?))
            }),
        )],
        getters: vec![("isEmpty".to_owned(), Rc::new(|this, _| Ok(JsData::Bool(notebook_range_of(this)?.is_empty()))))],
        statics: vec![],
        parent: None,
    });
    let cell_data = runner.define_class(ClassSpec {
        name: "NotebookCellData".to_owned(),
        length: 3,
        construct: Rc::new(|args| {
            let (Some(JsData::Num(kind)), Some(JsData::Str(value)), Some(JsData::Str(language_id))) = (args.first(), args.get(1), args.get(2)) else {
                return Err("NotebookCellData: kind, value e languageId são obrigatórios".to_owned());
            };
            Ok(JsData::Object(vec![
                ("kind".to_owned(), JsData::Num(*kind)),
                ("value".to_owned(), JsData::Str(value.clone())),
                ("languageId".to_owned(), JsData::Str(language_id.clone())),
            ]))
        }),
        methods: vec![],
        getters: vec![],
        statics: vec![],
        parent: None,
    });
    let data = runner.define_class(ClassSpec {
        name: "NotebookData".to_owned(),
        length: 1,
        construct: Rc::new(|args| match args.first() {
            Some(cells @ JsData::Array(_)) => Ok(JsData::Object(vec![("cells".to_owned(), cells.clone())])),
            _ => Err("NotebookData: as células precisam ser uma lista de NotebookCellData".to_owned()),
        }),
        methods: vec![],
        getters: vec![],
        statics: vec![],
        parent: None,
    });
    let output = runner.define_class(ClassSpec {
        name: "NotebookCellOutput".to_owned(),
        length: 3,
        construct: Rc::new(|args| {
            let items = match args.first() {
                Some(JsData::Array(items)) => items.iter().map(output_item_of).collect::<Result<Vec<_>, String>>()?,
                _ => return Err("NotebookCellOutput: os itens precisam ser uma lista de NotebookCellOutputItem".to_owned()),
            };
            let id_or_metadata = match args.get(1) {
                Some(JsData::Str(id)) => Some(IdOrMetadata::Id(id.clone())),
                Some(metadata @ JsData::Object(_)) => Some(IdOrMetadata::Metadata(js_to_dynamic(metadata))),
                _ => None,
            };
            let metadata = args.get(2).filter(|metadata| !is_nullish(Some(*metadata))).map(js_to_dynamic);
            Ok(JsData::Object(output_fields(&NotebookCellOutput::new(items, id_or_metadata, metadata))))
        }),
        methods: vec![],
        getters: vec![],
        statics: vec![],
        parent: None,
    });
    let item = runner.define_class(ClassSpec {
        name: "NotebookCellOutputItem".to_owned(),
        length: 2,
        construct: Rc::new(|args| {
            let mime = string_arg(args, 1, "NotebookCellOutputItem: o mime")?;
            let item = NotebookCellOutputItem::new(bytes_of(args.first())?, &mime).map_err(|error| error.message)?;
            Ok(JsData::Object(output_item_fields(&item)))
        }),
        methods: vec![],
        getters: vec![],
        statics: vec![
            ("text".to_owned(), 2, Rc::new(|args| text_item(args, "text/plain"))),
            ("json".to_owned(), 2, Rc::new(json_item)),
            ("stdout".to_owned(), 1, Rc::new(|args| Ok(output_item_to_js(&NotebookCellOutputItem::stdout(&string_arg(args, 0, "stdout: o valor")?))))),
            ("stderr".to_owned(), 1, Rc::new(|args| Ok(output_item_to_js(&NotebookCellOutputItem::stderr(&string_arg(args, 0, "stderr: o valor")?))))),
            (
                "error".to_owned(),
                1,
                Rc::new(|args| {
                    let error = args.first().unwrap_or(&JsData::Undefined);
                    let error = ErrorLike {
                        name: opt_string(field(error, "name")).unwrap_or_default(),
                        message: opt_string(field(error, "message")),
                        stack: opt_string(field(error, "stack")),
                    };
                    Ok(output_item_to_js(&NotebookCellOutputItem::error(&error)))
                }),
            ),
            (
                "bytes".to_owned(),
                2,
                Rc::new(|args| {
                    let item = NotebookCellOutputItem::bytes(bytes_of(args.first())?, opt_string(args.get(1)).as_deref()).map_err(|error| error.message)?;
                    Ok(output_item_to_js(&item))
                }),
            ),
        ],
        parent: None,
    });
    let status_bar_item = runner.define_class(ClassSpec {
        name: "NotebookCellStatusBarItem".to_owned(),
        length: 2,
        construct: Rc::new(|args| {
            let (Some(JsData::Str(text)), Some(JsData::Num(alignment))) = (args.first(), args.get(1)) else {
                return Err("NotebookCellStatusBarItem: text e alignment são obrigatórios".to_owned());
            };
            Ok(JsData::Object(vec![("text".to_owned(), JsData::Str(text.clone())), ("alignment".to_owned(), JsData::Num(*alignment))]))
        }),
        methods: vec![],
        getters: vec![],
        statics: vec![],
        parent: None,
    });
    let edit = runner.define_class(ClassSpec {
        name: "NotebookEdit".to_owned(),
        length: 2,
        construct: Rc::new(|args| notebook_edit_to_js(&args.first().map(notebook_range_of).transpose()?.ok_or("NotebookEdit: falta o intervalo")?, new_cells_arg(args.get(1))?, None, None)),
        methods: vec![],
        getters: vec![],
        statics: vec![
            (
                "isNotebookCellEdit".to_owned(),
                1,
                Rc::new(|args| Ok(JsData::Bool(args.first().is_some_and(is_notebook_edit)))),
            ),
            (
                "replaceCells".to_owned(),
                2,
                Rc::new(|args| notebook_edit_to_js(&notebook_range_of(args.first().ok_or("replaceCells: falta o intervalo")?)?, new_cells_arg(args.get(1))?, None, None)),
            ),
            (
                "insertCells".to_owned(),
                2,
                Rc::new(|args| {
                    let index = opt_number(args.first()).ok_or("insertCells: o índice precisa ser um número")?;
                    let range = NotebookRange::new(index, index).map_err(|error| error.message)?;
                    notebook_edit_to_js(&range, new_cells_arg(args.get(1))?, None, None)
                }),
            ),
            (
                "deleteCells".to_owned(),
                1,
                Rc::new(|args| notebook_edit_to_js(&notebook_range_of(args.first().ok_or("deleteCells: falta o intervalo")?)?, JsData::Array(Vec::new()), None, None)),
            ),
            (
                "updateCellMetadata".to_owned(),
                2,
                Rc::new(|args| {
                    let index = opt_number(args.first()).ok_or("updateCellMetadata: o índice precisa ser um número")?;
                    let range = NotebookRange::new(index, index).map_err(|error| error.message)?;
                    notebook_edit_to_js(&range, JsData::Array(Vec::new()), Some(args.get(1).cloned().unwrap_or(JsData::Undefined)), None)
                }),
            ),
            (
                "updateNotebookMetadata".to_owned(),
                1,
                Rc::new(|args| {
                    let range = NotebookRange::new(0.0, 0.0).map_err(|error| error.message)?;
                    notebook_edit_to_js(&range, JsData::Array(Vec::new()), None, Some(args.first().cloned().unwrap_or(JsData::Undefined)))
                }),
            ),
        ],
        parent: None,
    });
    [
        ("NotebookRange", range),
        ("NotebookCellData", cell_data),
        ("NotebookData", data),
        ("NotebookCellOutput", output),
        ("NotebookCellOutputItem", item),
        ("NotebookCellStatusBarItem", status_bar_item),
        ("NotebookEdit", edit),
    ]
    .into_iter()
    .map(|(name, constructor)| (name.to_owned(), JsData::Handle(Rc::new(constructor))))
    .collect()
}

/// As `newCells` de `NotebookEdit`: a lista de `NotebookCellData`, validada célula a célula (o original não valida, mas uma célula
/// inválida só falharia mais tarde, ao converter a edição).
fn new_cells_arg(value: Option<&JsData>) -> Result<JsData, String> {
    match value {
        Some(cells @ JsData::Array(items)) => {
            items.iter().try_for_each(|item| cell_data_of(item).map(drop))?;
            Ok(cells.clone())
        }
        _ => Err("NotebookEdit: as novas células precisam ser uma lista de NotebookCellData".to_owned()),
    }
}

/// O objeto `vscode.NotebookEdit` (os campos são atribuíveis, como no original).
fn notebook_edit_to_js(range: &NotebookRange, new_cells: JsData, new_cell_metadata: Option<JsData>, new_notebook_metadata: Option<JsData>) -> Result<JsData, String> {
    let mut fields = vec![("range".to_owned(), notebook_range_fields(range)), ("newCells".to_owned(), new_cells)];
    fields.extend(new_cell_metadata.map(|metadata| ("newCellMetadata".to_owned(), metadata)));
    fields.extend(new_notebook_metadata.map(|metadata| ("newNotebookMetadata".to_owned(), metadata)));
    Ok(marked("NotebookEdit", fields))
}

/// `NotebookEdit.isNotebookCellEdit`: uma instância, ou um objeto com `range` de notebook e `newCells` em lista.
pub(crate) fn is_notebook_edit(value: &JsData) -> bool {
    class_name(value) == Some("NotebookEdit")
        || (matches!(field(value, "newCells"), Some(JsData::Array(_)))
            && field(value, "range").is_some_and(|range| opt_number(field(range, "start")).is_some() && opt_number(field(range, "end")).is_some()))
}

/// A `NotebookEdit` do host que `value` representa.
pub(crate) fn notebook_edit_of(value: &JsData) -> Result<NotebookEdit, String> {
    let range = notebook_range_of(field(value, "range").ok_or("NotebookEdit.range precisa ser um NotebookRange")?)?;
    let new_cells = match field(value, "newCells") {
        Some(JsData::Array(items)) => items.iter().map(cell_data_of).collect::<Result<Vec<_>, String>>()?,
        _ => return Err("NotebookEdit.newCells precisa ser uma lista de NotebookCellData".to_owned()),
    };
    let mut edit = NotebookEdit::new(range, new_cells);
    edit.new_cell_metadata = dynamic_field(value, "newCellMetadata");
    edit.new_notebook_metadata = dynamic_field(value, "newNotebookMetadata");
    Ok(edit)
}

/// `NotebookCellOutputItem.text(value, mime = 'text/plain')`.
fn text_item(args: &[JsData], default_mime: &str) -> Result<JsData, String> {
    let value = string_arg(args, 0, "text: o valor")?;
    let mime = opt_string(args.get(1)).unwrap_or_else(|| default_mime.to_owned());
    NotebookCellOutputItem::new(value.into_bytes(), &mime).map(|item| output_item_to_js(&item)).map_err(|error| error.message)
}

/// `NotebookCellOutputItem.json(value, mime = 'text/x-json')`.
fn json_item(args: &[JsData]) -> Result<JsData, String> {
    let mime = opt_string(args.get(1));
    // O mime do chamador é conferido antes: o `json` do host lança com um mime inválido.
    NotebookCellOutputItem::new(Vec::new(), mime.as_deref().unwrap_or("text/x-json")).map_err(|error| error.message)?;
    Ok(output_item_to_js(&NotebookCellOutputItem::json(&json_value_of(args.first().unwrap_or(&JsData::Undefined)), mime.as_deref())))
}

// ---- os namespaces

/// O namespace `notebooks`: `createNotebookController` (com o cliente de kernels) e `registerNotebookCellStatusBarItemProvider`.
pub fn namespace(runner: &NodeRunner, js: &JsObjects, api: &NotebookApi, table: &Rc<DisposableTable>) -> JsData {
    let mut members: Vec<(String, JsData)> = Vec::new();
    if let Some(kernels) = &api.kernels {
        let (create_runner, kernels, objects) = (runner.clone(), kernels.clone(), api.objects.clone());
        let create = runner.make_function(
            "createNotebookController",
            5,
            Rc::new(move |args| {
                let id = string_arg(args, 0, "createNotebookController: o id")?;
                let notebook_type = string_arg(args, 1, "createNotebookController: o notebookType")?;
                let label = string_arg(args, 2, "createNotebookController: o label")?;
                let handler = match args.get(3) {
                    Some(JsData::Handle(function)) => Some((execute_handler_of(&create_runner, &objects, function), JsData::Handle(function.clone()))),
                    _ => None,
                };
                let controller = kernels
                    .create_notebook_controller(&host_extension(), &id, &notebook_type, &label, handler.as_ref().map(|(handler, _)| handler.clone()), None)
                    .map_err(|error| error.message)?;
                Ok(objects.add_controller(controller, handler.map(|(_, function)| function)))
            }),
        );
        members.push(("createNotebookController".to_owned(), JsData::Handle(Rc::new(create))));
    }
    let (js, table, api) = (js.clone(), table.clone(), api.clone());
    let register = runner.make_function_by_reference(
        "registerNotebookCellStatusBarItemProvider",
        2,
        Rc::new(move |args| {
            let notebook_type = string_arg(args, 0, "registerNotebookCellStatusBarItemProvider: o notebookType")?;
            let Some(JsData::Handle(provider)) = args.get(1) else {
                return Err("registerNotebookCellStatusBarItemProvider: o provedor precisa ser um objeto".to_owned());
            };
            let provider = JsStatusBarProvider::new(&js, &table, &api, provider)?;
            Ok(table.to_js(Rc::new(api.notebooks.register_notebook_cell_status_bar_item_provider(&host_extension(), &notebook_type, provider))))
        }),
    );
    members.push(("registerNotebookCellStatusBarItemProvider".to_owned(), JsData::Handle(Rc::new(register))));
    JsData::Object(members)
}

/// Os membros de notebook de `vscode.workspace`: os métodos e os acessores que o namespace junta aos demais, e `registerNotebookSerializer`,
/// que recebe o serializador da extensão (uma função por referência, própria do namespace).
pub fn workspace_members(runner: &NodeRunner, api: &NotebookApi, table: &Rc<DisposableTable>) -> (Vec<(String, u32, MethodCallback)>, Vec<(String, MethodCallback)>, Vec<(String, JsData)>) {
    let (open_runner, open_api) = (runner.clone(), api.clone());
    let methods: Vec<(String, u32, MethodCallback)> = vec![(
        "openNotebookDocument".to_owned(),
        2,
        Rc::new(move |_, args| {
            let (notebooks, objects) = (open_api.notebooks.clone(), open_api.objects.clone());
            match args.first() {
                Some(uri @ JsData::Object(_)) if class_name(uri) == Some("Uri") => {
                    let opened = notebooks.open_notebook_document(&uri_from_js(uri)?);
                    Ok(promise_to_js(&open_runner, opened, move |document| objects.document_to_js(document)))
                }
                Some(JsData::Str(notebook_type)) => {
                    let content = match args.get(1) {
                        Some(content) if !is_nullish(Some(content)) => Some(notebook_data_of(content)?),
                        _ => None,
                    };
                    let lookup = notebooks.clone();
                    let opened = notebooks.create_notebook_document(notebook_type, content.as_ref()).then_result(move |outcome| {
                        let uri = outcome?;
                        lookup
                            .try_get_notebook_document(&uri)
                            .map(|document| document.api_notebook())
                            .ok_or_else(|| Error::new(format!("NO notebook document for '{}'", uri.to_string(false))))
                    });
                    Ok(promise_to_js(&open_runner, opened, move |document| objects.document_to_js(document)))
                }
                _ => Err("Invalid arguments".to_owned()),
            }
        }),
    )];

    let (list_api, change_api) = (api.clone(), api.clone());
    let (open_event_runner, open_event_table, open_event_api) = (runner.clone(), table.clone(), api.clone());
    let (close_event_runner, close_event_table, close_event_api) = (runner.clone(), table.clone(), api.clone());
    let (change_runner, change_table) = (runner.clone(), table.clone());
    let (save_runner, save_table, save_api) = (runner.clone(), table.clone(), api.clone());
    let mut getters: Vec<(String, MethodCallback)> = vec![
        (
            "notebookDocuments".to_owned(),
            Rc::new(move |_, _| Ok(JsData::Array(list_api.notebooks.notebook_documents().iter().map(|document| list_api.objects.document_to_js(&document.api_notebook())).collect()))),
        ),
        (
            "onDidOpenNotebookDocument".to_owned(),
            Rc::new(move |_, _| {
                let objects = open_event_api.objects.clone();
                Ok(event_function(&open_event_runner, &open_event_table, "onDidOpenNotebookDocument", open_event_api.notebooks.on_did_open_notebook_document(), move |document| {
                    objects.document_to_js(document)
                }))
            }),
        ),
        (
            "onDidCloseNotebookDocument".to_owned(),
            Rc::new(move |_, _| {
                let objects = close_event_api.objects.clone();
                Ok(event_function(&close_event_runner, &close_event_table, "onDidCloseNotebookDocument", close_event_api.notebooks.on_did_close_notebook_document(), move |document| {
                    objects.document_to_js(document)
                }))
            }),
        ),
        (
            "onDidChangeNotebookDocument".to_owned(),
            Rc::new(move |_, _| {
                let api = change_api.clone();
                Ok(event_function(&change_runner, &change_table, "onDidChangeNotebookDocument", change_api.documents.on_did_change_notebook_document(), move |event| {
                    change_event_to_js(&api, event)
                }))
            }),
        ),
        (
            "onDidSaveNotebookDocument".to_owned(),
            Rc::new(move |_, _| {
                let objects = save_api.objects.clone();
                Ok(event_function(&save_runner, &save_table, "onDidSaveNotebookDocument", save_api.documents.on_did_save_notebook_document(), move |document| {
                    objects.document_to_js(document)
                }))
            }),
        ),
    ];
    if let Some(will_save) = api.will_save.clone() {
        let (will_runner, will_table, will_api) = (runner.clone(), table.clone(), api.clone());
        getters.push((
            "onWillSaveNotebookDocument".to_owned(),
            Rc::new(move |_, _| Ok(will_save_event(&will_runner, &will_table, &will_api, &will_save))),
        ));
    }

    let (register_js, register_table, register_api) = (JsObjects::new(runner), table.clone(), api.clone());
    let register_serializer = runner.make_function_by_reference(
        "registerNotebookSerializer",
        3,
        Rc::new(move |args| {
            let notebook_type = string_arg(args, 0, "registerNotebookSerializer: o notebookType")?;
            let Some(JsData::Handle(serializer)) = args.get(1) else {
                return Err("registerNotebookSerializer: o serializador precisa ser um objeto".to_owned());
            };
            let serializer = JsSerializer::new(&register_js, &register_api.events, serializer)?;
            // As opções chegam por referência: `exported` as entrega por valor.
            let options = args.get(2).map(|options| exported(register_js.runner(), options));
            let registration = register_api
                .notebooks
                .register_notebook_serializer(&host_extension(), &notebook_type, serializer, content_options_of(options.as_ref()), None)
                .map_err(|error| error.message)?;
            Ok(register_table.to_js(registration))
        }),
    );
    let references = vec![("registerNotebookSerializer".to_owned(), JsData::Handle(Rc::new(register_serializer)))];
    (methods, getters, references)
}

/// A função de evento de `onWillSaveNotebookDocument`: o ouvinte recebe `{ notebook, reason, token, waitUntil }`.
fn will_save_event(runner: &NodeRunner, table: &Rc<DisposableTable>, api: &NotebookApi, will_save: &WillSaveApi) -> JsData {
    let event = will_save.participant.get_on_will_save_notebook_document_event(&host_extension());
    let (event_runner, api, edits) = (runner.clone(), api.clone(), will_save.edits.clone());
    event_function_with_epilogue(runner, table, "onWillSaveNotebookDocument", event, move |event: &NotebookDocumentWillSaveEvent| {
        // `None` marca o fim do despacho: depois dele `waitUntil` não vale mais.
        let thenables: Rc<RefCell<Option<Vec<JsData>>>> = Rc::new(RefCell::new(Some(Vec::new())));
        let collected = thenables.clone();
        let wait_until = event_runner.make_function(
            "waitUntil",
            1,
            Rc::new(move |args| {
                let mut slot = collected.borrow_mut();
                let list = slot.as_mut().ok_or("waitUntil can NOT be called asynchronous")?;
                list.push(args.first().cloned().unwrap_or(JsData::Undefined));
                Ok(JsData::Undefined)
            }),
        );
        let payload = JsData::Object(vec![
            ("notebook".to_owned(), api.objects.document_to_js(&event.notebook)),
            ("reason".to_owned(), JsData::Num(f64::from(event.reason.value()))),
            ("token".to_owned(), api.events.token_to_js(event.token.clone())),
            ("waitUntil".to_owned(), JsData::Handle(Rc::new(wait_until))),
        ]);
        let (runner, edits) = (event_runner.clone(), edits.clone());
        let epilogue = move |event: &NotebookDocumentWillSaveEvent| {
            for thenable in thenables.borrow_mut().take().unwrap_or_default() {
                if let Err(error) = event.wait_until(thenable_edit(&runner, &edits, thenable)) {
                    on_unexpected_error(&error);
                }
            }
        };
        (payload, epilogue)
    })
}

/// `NotebookDocumentShowOptions` da extensão.
fn show_options_of(value: &JsData) -> Result<NotebookDocumentShowOptions, String> {
    let selections = match field(value, "selections") {
        Some(JsData::Array(items)) => Some(items.iter().map(notebook_range_of).collect::<Result<Vec<_>, String>>()?),
        _ => None,
    };
    let as_repl = match field(value, "asRepl") {
        Some(JsData::Bool(flag)) => Some(NotebookAsRepl::Flag(*flag)),
        Some(JsData::Str(label)) => Some(NotebookAsRepl::Label(label.clone())),
        Some(options @ JsData::Object(_)) => Some(NotebookAsRepl::Label(opt_string(field(options, "label")).ok_or("asRepl.label precisa ser texto")?)),
        _ => None,
    };
    Ok(NotebookDocumentShowOptions {
        view_column: opt_number(field(value, "viewColumn")),
        preserve_focus: opt_bool(field(value, "preserveFocus")),
        selections,
        preview: opt_bool(field(value, "preview")),
        as_repl,
    })
}

/// Os membros de notebook de `vscode.window`: `activeNotebookEditor`, `visibleNotebookEditors`, `showNotebookDocument` e os dois eventos de editor.
pub fn window_members(runner: &NodeRunner, api: &NotebookApi, table: &Rc<DisposableTable>) -> (Vec<(String, u32, MethodCallback)>, Vec<(String, MethodCallback)>) {
    let (show_runner, show_api) = (runner.clone(), api.clone());
    let methods: Vec<(String, u32, MethodCallback)> = vec![(
        "showNotebookDocument".to_owned(),
        2,
        Rc::new(move |_, args| {
            let document = show_api.objects.document_of(args.first().ok_or("showNotebookDocument: falta o notebook")?)?;
            let options = match args.get(1) {
                Some(options) if !is_nullish(Some(options)) => Some(show_options_of(options)?),
                _ => None,
            };
            let objects = show_api.objects.clone();
            Ok(promise_to_js(&show_runner, show_api.notebooks.show_notebook_document(&document, options), move |editor| objects.editor_to_js(editor)))
        }),
    )];

    let (active_api, visible_api) = (api.clone(), api.clone());
    let (active_runner, active_table, active_event_api) = (runner.clone(), table.clone(), api.clone());
    let (visible_runner, visible_table, visible_event_api) = (runner.clone(), table.clone(), api.clone());
    let (selection_runner, selection_table, selection_event_api) = (runner.clone(), table.clone(), api.clone());
    let (ranges_runner, ranges_table, ranges_event_api) = (runner.clone(), table.clone(), api.clone());
    let getters: Vec<(String, MethodCallback)> = vec![
        (
            "activeNotebookEditor".to_owned(),
            Rc::new(move |_, _| Ok(active_api.notebooks.active_notebook_editor().map_or(JsData::Undefined, |editor| active_api.objects.editor_to_js(&editor)))),
        ),
        (
            "visibleNotebookEditors".to_owned(),
            Rc::new(move |_, _| Ok(JsData::Array(visible_api.notebooks.visible_notebook_editors().iter().map(|editor| visible_api.objects.editor_to_js(editor)).collect()))),
        ),
        (
            "onDidChangeActiveNotebookEditor".to_owned(),
            Rc::new(move |_, _| {
                let objects = active_event_api.objects.clone();
                Ok(event_function(
                    &active_runner,
                    &active_table,
                    "onDidChangeActiveNotebookEditor",
                    active_event_api.notebooks.on_did_change_active_notebook_editor(),
                    move |editor: &Option<NotebookEditor>| editor.as_ref().map_or(JsData::Undefined, |editor| objects.editor_to_js(editor)),
                ))
            }),
        ),
        (
            "onDidChangeVisibleNotebookEditors".to_owned(),
            Rc::new(move |_, _| {
                let objects = visible_event_api.objects.clone();
                Ok(event_function(
                    &visible_runner,
                    &visible_table,
                    "onDidChangeVisibleNotebookEditors",
                    visible_event_api.notebooks.on_did_change_visible_notebook_editors(),
                    move |editors: &Vec<NotebookEditor>| JsData::Array(editors.iter().map(|editor| objects.editor_to_js(editor)).collect()),
                ))
            }),
        ),
        (
            "onDidChangeNotebookEditorSelection".to_owned(),
            Rc::new(move |_, _| {
                let objects = selection_event_api.objects.clone();
                Ok(event_function(
                    &selection_runner,
                    &selection_table,
                    "onDidChangeNotebookEditorSelection",
                    selection_event_api.editors.on_did_change_notebook_editor_selection(),
                    move |event: &NotebookEditorSelectionChangeEvent| {
                        JsData::Object(vec![
                            ("notebookEditor".to_owned(), objects.editor_to_js(&event.notebook_editor)),
                            ("selections".to_owned(), JsData::Array(event.selections.iter().map(notebook_range_fields).collect())),
                        ])
                    },
                ))
            }),
        ),
        (
            "onDidChangeNotebookEditorVisibleRanges".to_owned(),
            Rc::new(move |_, _| {
                let objects = ranges_event_api.objects.clone();
                Ok(event_function(
                    &ranges_runner,
                    &ranges_table,
                    "onDidChangeNotebookEditorVisibleRanges",
                    ranges_event_api.editors.on_did_change_notebook_editor_visible_ranges(),
                    move |event: &NotebookEditorVisibleRangesChangeEvent| {
                        JsData::Object(vec![
                            ("notebookEditor".to_owned(), objects.editor_to_js(&event.notebook_editor)),
                            ("visibleRanges".to_owned(), JsData::Array(event.visible_ranges.iter().map(notebook_range_fields).collect())),
                        ])
                    },
                ))
            }),
        ),
    ];
    (methods, getters)
}
