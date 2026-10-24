//! Porte de `src/vs/workbench/api/common/extHostNotebookDocument.ts`: o documento de notebook e as células
//! (`vscode.NotebookDocument` e `vscode.NotebookCell`) do lado das extensões, atualizados pelas mudanças que a thread
//! principal manda.
//!
//! Divergências:
//! - Os objetos da API (`apiCell`, `apiNotebook`) são os envoltórios [`NotebookCell`] e [`NotebookDocument`], um `Rc` do
//!   objeto do host: a identidade do original (`===`) é [`NotebookCell::ptr_eq`] e [`NotebookDocument::ptr_eq`], e os
//!   getters são métodos. O `Object.freeze` não existe (nenhum método muda o objeto).
//! - A célula guarda um [`Weak`] do documento (o original tem um ciclo, que o coletor de lixo resolve). O `index` de uma
//!   célula cujo documento já não existe é `-1`, o mesmo valor de uma célula removida (`indexOf`).
//! - `Object.freeze(metadata)` e `Object.freeze(executionSummary)` não existem; os metadados são [`Dynamic`] (o objeto é
//!   compartilhado por referência, e quem lê não o muda). Os eventos de mudança são structs com campos `Option` para as
//!   propriedades que o original deixa `undefined`, e não são congelados.
//! - `RelaxedCellChange` é o próprio [`NotebookDocumentCellChange`] com só os campos da mudança preenchidos; compactar é
//!   copiar por cima os campos presentes (o `{ ...existing, ...relaxed }` do original).
//! - `cellAt` devolve [`Result`] (o original lança `TypeError` ao ler `apiCell` de uma posição inexistente, num documento
//!   vazio); um índice de célula fora do documento num evento bruto lança [`Error`] com a mesma causa.
//! - `ExtHostCell.setMime` (corpo vazio no original, nunca chamado) não é portado: a mudança de MIME vai direto em
//!   `apiCell.mime`. `[Symbol.for('debug.description')]` é `debug_description`.
//! - `handle` de `ExtHostNotebookDocument` vem de um contador estático ([`AtomicU32`]), como o `_handlePool`.
//! - `ExtHostNotebookDocument` é criado por [`ExtHostNotebookDocument::new`] como `Rc` (a célula guarda o `Weak`), e
//!   implementa [`IDisposable`] (o original tem `dispose()`, e o teste o registra como descartável).

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};
use std::sync::atomic::{AtomicU32, Ordering};

use je_base::JsString;
use je_base::dynamic::Dynamic;
use je_base::errors::{self, Error};
use je_base::lifecycle::IDisposable;
use je_base::mime::is_text_stream_mime;
use je_base::network::schemas;
use je_base::promise::Promise;
use je_base::uri::URI;
use je_editor::model::Eol;

use super::ext_host_document_data::TextDocument;
use super::ext_host_documents::ExtHostDocuments;
use super::ext_host_documents_and_editors::ExtHostDocumentsAndEditors;
use super::ext_host_protocol::{
    DocumentsAndEditorsDelta, ExtHostDocumentsShape, ExtHostDocumentsAndEditorsShape, INotebookDocumentPropertiesChangeData, INotebookModelAddedData, MainThreadNotebookDocumentsShape,
    ModelAddedData, NotebookCellDto, NotebookCellsChangedEventDto, NotebookOutputDto, NotebookOutputItemDto, NotebookRawContentEventDto,
};
use super::ext_host_type_converters as type_converters;
use super::ext_host_types as types;
use crate::contrib::notebook::common::notebook_common::{
    CellKind, NotebookCellInternalMetadata, NotebookCellMetadata, NotebookCellTextModelSplice, NotebookDocumentMetadata, compress_output_item_streams,
};

/// `Object.create(null)` e `{}`: um objeto vazio.
fn empty_object() -> Dynamic {
    Dynamic::object(Vec::<(&str, Dynamic)>::new())
}

/// `{ ...base, ...extra }`.
fn merge_objects(base: &Dynamic, extra: &Dynamic) -> Dynamic {
    let mut entries = base.own_entries();
    for (key, value) in extra.own_entries() {
        match entries.iter_mut().find(|(existing, _)| *existing == key) {
            Some(entry) => entry.1 = value,
            None => entries.push((key, value)),
        }
    }
    Dynamic::object(entries)
}

/// Um valor que o original lançaria como exceção.
fn unwrap_or_throw<T>(result: Result<T, Error>) -> T {
    result.unwrap_or_else(|error| errors::throw(error))
}

/// `RawContentChangeEvent`.
struct RawContentChangeEvent {
    start: usize,
    deleted_count: usize,
    deleted_items: Vec<NotebookCell>,
    items: Vec<Rc<ExtHostCell>>,
}

impl RawContentChangeEvent {
    fn as_api_event(&self) -> NotebookDocumentContentChange {
        NotebookDocumentContentChange {
            range: unwrap_or_throw(types::NotebookRange::new(self.start as f64, (self.start + self.deleted_count) as f64)),
            added_cells: self.items.iter().map(ExtHostCell::api_cell).collect(),
            removed_cells: self.deleted_items.clone(),
        }
    }
}

/// `vscode.NotebookDocumentContentChange`.
#[derive(Clone)]
pub struct NotebookDocumentContentChange {
    pub range: types::NotebookRange,
    pub added_cells: Vec<NotebookCell>,
    pub removed_cells: Vec<NotebookCell>,
}

/// `vscode.NotebookDocumentCellChange`: as propriedades ausentes são `None` (`undefined`).
#[derive(Clone)]
pub struct NotebookDocumentCellChange {
    pub cell: NotebookCell,
    pub document: Option<TextDocument>,
    pub metadata: Option<Dynamic>,
    pub outputs: Option<Vec<types::NotebookCellOutput>>,
    pub execution_summary: Option<Dynamic>,
}

impl NotebookDocumentCellChange {
    fn new(cell: NotebookCell) -> NotebookDocumentCellChange {
        NotebookDocumentCellChange { cell, document: None, metadata: None, outputs: None, execution_summary: None }
    }

    /// `{ ...this, ...relaxed }`: as propriedades presentes em `relaxed` vencem.
    fn merge(&mut self, relaxed: NotebookDocumentCellChange) {
        if relaxed.document.is_some() {
            self.document = relaxed.document;
        }
        if relaxed.metadata.is_some() {
            self.metadata = relaxed.metadata;
        }
        if relaxed.outputs.is_some() {
            self.outputs = relaxed.outputs;
        }
        if relaxed.execution_summary.is_some() {
            self.execution_summary = relaxed.execution_summary;
        }
    }
}

/// `vscode.NotebookDocumentChangeEvent`.
#[derive(Clone)]
pub struct NotebookDocumentChangeEvent {
    pub notebook: NotebookDocument,
    pub metadata: Option<NotebookDocumentMetadata>,
    pub cell_changes: Vec<NotebookDocumentCellChange>,
    pub content_changes: Vec<NotebookDocumentContentChange>,
}

/// `ExtHostCell`.
pub struct ExtHostCell {
    pub notebook: Weak<ExtHostNotebookDocument>,
    ext_host_document: Rc<ExtHostDocumentsAndEditors>,
    cell_data: NotebookCellDto,

    outputs: RefCell<Vec<types::NotebookCellOutput>>,
    metadata: RefCell<NotebookCellMetadata>,
    previous_result: RefCell<Dynamic>,
    internal_metadata: RefCell<NotebookCellInternalMetadata>,
    pub handle: i64,
    pub uri: URI,
    pub cell_kind: CellKind,

    /// O `document` do `apiCell`, criado na primeira vez em que a API da célula é pedida.
    api_document: RefCell<Option<TextDocument>>,
    mime: RefCell<Option<String>>,
}

impl ExtHostCell {
    /// `ExtHostCell.asModelAddData(cell)`.
    pub fn as_model_add_data(cell: &NotebookCellDto) -> ModelAddedData {
        ModelAddedData {
            eol: if cell.eol == "\r\n" { Eol::CrLf } else { Eol::Lf },
            lines: cell.source.iter().map(|line| JsString::from(line.as_str())).collect(),
            language_id: cell.language.clone(),
            uri: cell.uri.clone(),
            is_dirty: false,
            version_id: 1,
            encoding: "utf8".to_owned(),
        }
    }

    pub fn new(notebook: Weak<ExtHostNotebookDocument>, ext_host_document: Rc<ExtHostDocumentsAndEditors>, cell_data: NotebookCellDto) -> ExtHostCell {
        let outputs = cell_data.outputs.iter().map(|output| unwrap_or_throw(type_converters::notebook::NotebookCellOutput::to(output))).collect();
        let internal_metadata = cell_data.internal_metadata.clone().unwrap_or_default();
        ExtHostCell {
            notebook,
            ext_host_document,
            outputs: RefCell::new(outputs),
            metadata: RefCell::new(cell_data.metadata.clone().unwrap_or_else(empty_object)),
            previous_result: RefCell::new(type_converters::notebook::NotebookCellExecutionSummary::to(&internal_metadata)),
            internal_metadata: RefCell::new(internal_metadata),
            handle: cell_data.handle,
            uri: cell_data.uri.clone(),
            cell_kind: cell_data.cell_kind,
            api_document: RefCell::new(None),
            mime: RefCell::new(None),
            cell_data,
        }
    }

    pub fn internal_metadata(&self) -> NotebookCellInternalMetadata {
        self.internal_metadata.borrow().clone()
    }

    /// `apiCell`: cria o `document` da API na primeira vez; lança se o documento de texto da célula não existe.
    pub fn api_cell(self: &Rc<Self>) -> NotebookCell {
        if self.api_document.borrow().is_none() {
            let data = self
                .ext_host_document
                .get_document(&self.uri)
                .unwrap_or_else(|| errors::throw(Error::new(format!("MISSING extHostDocument for notebook cell: {}", self.uri.to_string(false)))));
            *self.api_document.borrow_mut() = Some(data.document());
        }
        NotebookCell(self.clone())
    }

    pub fn set_outputs(&self, new_outputs: &[NotebookOutputDto]) {
        *self.outputs.borrow_mut() = new_outputs.iter().map(|output| unwrap_or_throw(type_converters::notebook::NotebookCellOutput::to(output))).collect();
    }

    pub fn set_output_items(&self, output_id: &str, append: bool, new_output_items: &[NotebookOutputItemDto]) {
        let new_items: Vec<types::NotebookCellOutputItem> =
            new_output_items.iter().map(|item| unwrap_or_throw(type_converters::notebook::NotebookCellOutputItem::to(item))).collect();
        let mut outputs = self.outputs.borrow_mut();
        let Some(output) = outputs.iter_mut().find(|output| output.id == output_id) else {
            return;
        };
        if !append {
            output.items.clear();
        }
        output.items.extend(new_items);

        if output.items.len() > 1 && output.items.iter().all(|item| is_text_stream_mime(item.mime())) {
            // Look for the mimes in the items, and keep track of their order.
            // Merge the streams into one output item, per mime type.
            let mut mime_outputs: Vec<(String, Vec<Vec<u8>>)> = Vec::new();
            for item in &output.items {
                match mime_outputs.iter_mut().find(|(mime, _)| mime == item.mime()) {
                    Some((_, items)) => items.push(item.data.clone()),
                    None => mime_outputs.push((item.mime().to_owned(), vec![item.data.clone()])),
                }
            }
            output.items.clear();
            for (mime, items) in mime_outputs {
                let chunks: Vec<&[u8]> = items.iter().map(Vec::as_slice).collect();
                let compressed = compress_output_item_streams(&chunks);
                output.items.push(unwrap_or_throw(types::NotebookCellOutputItem::new(compressed.data.to_vec(), &mime)));
            }
        }
    }

    pub fn set_metadata(&self, new_metadata: NotebookCellMetadata) {
        *self.metadata.borrow_mut() = new_metadata;
    }

    pub fn set_internal_metadata(&self, new_internal_metadata: NotebookCellInternalMetadata) {
        *self.previous_result.borrow_mut() = type_converters::notebook::NotebookCellExecutionSummary::to(&new_internal_metadata);
        *self.internal_metadata.borrow_mut() = new_internal_metadata;
    }
}

/// `vscode.NotebookCell`.
#[derive(Clone)]
pub struct NotebookCell(Rc<ExtHostCell>);

impl NotebookCell {
    /// A identidade (`a === b`).
    pub fn ptr_eq(a: &NotebookCell, b: &NotebookCell) -> bool {
        Rc::ptr_eq(&a.0, &b.0)
    }

    /// O objeto do host por trás da célula.
    pub fn ext_host(&self) -> &Rc<ExtHostCell> {
        &self.0
    }

    pub fn index(&self) -> f64 {
        match self.0.notebook.upgrade() {
            Some(notebook) => notebook.get_cell_index(&self.0),
            None => -1.0,
        }
    }

    pub fn notebook(&self) -> NotebookDocument {
        match self.0.notebook.upgrade() {
            Some(notebook) => NotebookDocument(notebook),
            None => errors::throw(Error::new("the notebook of the cell is gone")),
        }
    }

    pub fn kind(&self) -> types::NotebookCellKind {
        type_converters::notebook::NotebookCellKind::to(self.0.cell_data.cell_kind)
    }

    pub fn document(&self) -> TextDocument {
        self.0.api_document.borrow().clone().expect("o document é criado por api_cell")
    }

    pub fn mime(&self) -> Option<String> {
        self.0.mime.borrow().clone()
    }

    pub fn set_mime(&self, value: Option<String>) {
        *self.0.mime.borrow_mut() = value;
    }

    /// `outputs`: uma cópia da lista (`slice(0)`).
    pub fn outputs(&self) -> Vec<types::NotebookCellOutput> {
        self.0.outputs.borrow().clone()
    }

    pub fn metadata(&self) -> NotebookCellMetadata {
        self.0.metadata.borrow().clone()
    }

    pub fn execution_summary(&self) -> Dynamic {
        self.0.previous_result.borrow().clone()
    }
}

/// `ExtHostNotebookDocument`.
pub struct ExtHostNotebookDocument {
    this: Weak<ExtHostNotebookDocument>,
    pub handle: u32,

    cells: RefCell<Vec<Rc<ExtHostCell>>>,

    notebook_type: String,

    metadata: RefCell<Dynamic>,
    version_id: Cell<i64>,
    is_dirty: Cell<bool>,
    disposed: Cell<bool>,

    proxy: Rc<dyn MainThreadNotebookDocumentsShape>,
    text_documents_and_editors: Rc<ExtHostDocumentsAndEditors>,
    text_documents: Rc<ExtHostDocuments>,
    pub uri: URI,
}

/// `ExtHostNotebookDocument._handlePool`.
static HANDLE_POOL: AtomicU32 = AtomicU32::new(0);

impl ExtHostNotebookDocument {
    pub fn new(
        proxy: Rc<dyn MainThreadNotebookDocumentsShape>,
        text_documents_and_editors: Rc<ExtHostDocumentsAndEditors>,
        text_documents: Rc<ExtHostDocuments>,
        uri: URI,
        data: INotebookModelAddedData,
    ) -> Rc<ExtHostNotebookDocument> {
        Rc::new_cyclic(|this| {
            let document = ExtHostNotebookDocument {
                this: this.clone(),
                handle: HANDLE_POOL.fetch_add(1, Ordering::SeqCst),
                cells: RefCell::new(Vec::new()),
                notebook_type: data.view_type.clone(),
                metadata: RefCell::new(data.metadata.clone().unwrap_or_else(empty_object)),
                version_id: Cell::new(0),
                is_dirty: Cell::new(false),
                disposed: Cell::new(false),
                proxy,
                text_documents_and_editors,
                text_documents,
                uri,
            };
            document.splice_notebook_cells(vec![(0, 0, data.cells)], true /* init -> no event*/, None);
            document.version_id.set(data.version_id);
            document
        })
    }

    pub fn version_id(&self) -> i64 {
        self.version_id.get()
    }

    /// `apiNotebook`.
    pub fn api_notebook(self: &Rc<Self>) -> NotebookDocument {
        NotebookDocument(self.clone())
    }

    pub fn accept_document_properties_changed(&self, data: INotebookDocumentPropertiesChangeData) {
        if let Some(metadata) = data.metadata.filter(|metadata| !metadata.is_nullish()) {
            let merged = merge_objects(&self.metadata.borrow(), &metadata);
            *self.metadata.borrow_mut() = merged;
        }
    }

    pub fn accept_dirty(&self, is_dirty: bool) {
        self.is_dirty.set(is_dirty);
    }

    pub fn accept_model_changed(
        self: &Rc<Self>,
        event: NotebookCellsChangedEventDto,
        is_dirty: bool,
        new_metadata: Option<NotebookDocumentMetadata>,
    ) -> NotebookDocumentChangeEvent {
        self.version_id.set(event.version_id);
        self.is_dirty.set(is_dirty);
        self.accept_document_properties_changed(INotebookDocumentPropertiesChangeData { metadata: new_metadata.clone() });

        let mut content_changes: Vec<NotebookDocumentContentChange> = Vec::new();
        let mut relaxed_cell_changes: Vec<NotebookDocumentCellChange> = Vec::new();

        // -- apply change and populate content changes

        for raw_event in event.raw_events {
            match raw_event {
                NotebookRawContentEventDto::ModelChange { changes } => {
                    self.splice_notebook_cells(changes, false, Some(&mut content_changes));
                }
                NotebookRawContentEventDto::Move { index, length, new_idx } => {
                    self.move_cells(index, length, new_idx, &mut content_changes);
                }
                NotebookRawContentEventDto::Output { index, outputs } => {
                    let cell = self.cell_at_index(index);
                    cell.set_outputs(&outputs);
                    let api_cell = cell.api_cell();
                    let mut change = NotebookDocumentCellChange::new(api_cell.clone());
                    change.outputs = Some(api_cell.outputs());
                    relaxed_cell_changes.push(change);
                }
                NotebookRawContentEventDto::OutputItem { index, output_id, output_items, append } => {
                    let cell = self.cell_at_index(index);
                    cell.set_output_items(&output_id, append, &output_items);
                    let api_cell = cell.api_cell();
                    let mut change = NotebookDocumentCellChange::new(api_cell.clone());
                    change.outputs = Some(api_cell.outputs());
                    relaxed_cell_changes.push(change);
                }
                NotebookRawContentEventDto::ChangeCellLanguage { index, language } => {
                    self.change_cell_language(index, &language);
                    let api_cell = self.cell_at_index(index).api_cell();
                    let mut change = NotebookDocumentCellChange::new(api_cell.clone());
                    change.document = Some(api_cell.document());
                    relaxed_cell_changes.push(change);
                }
                NotebookRawContentEventDto::ChangeCellContent { index } => {
                    let api_cell = self.cell_at_index(index).api_cell();
                    let mut change = NotebookDocumentCellChange::new(api_cell.clone());
                    change.document = Some(api_cell.document());
                    relaxed_cell_changes.push(change);
                }
                NotebookRawContentEventDto::ChangeCellMime { index, mime } => {
                    self.cell_at_index(index).api_cell().set_mime(mime);
                }
                NotebookRawContentEventDto::ChangeCellMetadata { index, metadata } => {
                    let cell = self.cell_at_index(index);
                    cell.set_metadata(metadata);
                    let api_cell = cell.api_cell();
                    let mut change = NotebookDocumentCellChange::new(api_cell.clone());
                    change.metadata = Some(api_cell.metadata());
                    relaxed_cell_changes.push(change);
                }
                NotebookRawContentEventDto::ChangeCellInternalMetadata { index, internal_metadata } => {
                    let cell = self.cell_at_index(index);
                    cell.set_internal_metadata(internal_metadata);
                    let api_cell = cell.api_cell();
                    let mut change = NotebookDocumentCellChange::new(api_cell.clone());
                    change.execution_summary = Some(api_cell.execution_summary());
                    relaxed_cell_changes.push(change);
                }
            }
        }

        // -- compact cellChanges

        let mut cell_changes: Vec<NotebookDocumentCellChange> = Vec::new();
        for relaxed_cell_change in relaxed_cell_changes {
            let existing = cell_changes.iter_mut().find(|change| NotebookCell::ptr_eq(&change.cell, &relaxed_cell_change.cell));
            match existing {
                None => cell_changes.push(relaxed_cell_change),
                Some(existing) => existing.merge(relaxed_cell_change),
            }
        }

        NotebookDocumentChangeEvent { notebook: self.api_notebook(), metadata: new_metadata, cell_changes, content_changes }
    }

    /// `index | 0` e os limites de `_validateIndex`.
    fn validate_index(&self, index: f64) -> i64 {
        let index = index as i32 as i64;
        let len = self.cells.borrow().len() as i64;
        if index < 0 {
            0
        } else if index >= len {
            len - 1
        } else {
            index
        }
    }

    /// `_validateRange`.
    fn validate_range(&self, range: &types::NotebookRange) -> types::NotebookRange {
        let mut start = range.start() as i32 as i64;
        let mut end = range.end() as i32 as i64;
        if start < 0 {
            start = 0;
        }
        let len = self.cells.borrow().len() as i64;
        if end > len {
            end = len;
        }
        unwrap_or_throw(range.with(Some(start as f64), Some(end as f64)))
    }

    /// `_getCells`.
    fn get_cells(&self, range: &types::NotebookRange) -> Vec<Rc<ExtHostCell>> {
        let range = self.validate_range(range);
        let cells = self.cells.borrow();
        (range.start() as usize..range.end() as usize).filter_map(|index| cells.get(index).cloned()).collect()
    }

    fn save(&self) -> Promise<bool> {
        if self.disposed.get() {
            return Promise::rejected(Error::new("Notebook has been closed"));
        }
        self.proxy.rpc_try_save_notebook(&self.uri)
    }

    /// `this._cells[index]` de um evento bruto; lança quando o índice está fora do documento.
    fn cell_at_index(&self, index: usize) -> Rc<ExtHostCell> {
        self.cells
            .borrow()
            .get(index)
            .cloned()
            .unwrap_or_else(|| errors::throw(Error::new(format!("Cannot read properties of undefined: no cell at index {index}"))))
    }

    fn splice_notebook_cells(
        &self,
        splices: Vec<NotebookCellTextModelSplice<NotebookCellDto>>,
        initialization: bool,
        bucket: Option<&mut Vec<NotebookDocumentContentChange>>,
    ) {
        if self.disposed.get() {
            return;
        }

        let mut content_change_events: Vec<RawContentChangeEvent> = Vec::new();
        let mut added_cell_documents: Vec<ModelAddedData> = Vec::new();
        let mut removed_cell_documents: Vec<URI> = Vec::new();

        for (start, delete_count, cell_dtos) in splices.into_iter().rev() {
            let new_cells: Vec<Rc<ExtHostCell>> = cell_dtos
                .iter()
                .map(|cell| {
                    let ext_cell = Rc::new(ExtHostCell::new(self.this.clone(), self.text_documents_and_editors.clone(), cell.clone()));
                    if !initialization {
                        added_cell_documents.push(ExtHostCell::as_model_add_data(cell));
                    }
                    ext_cell
                })
                .collect();

            // `Array.prototype.splice` limita o início e a contagem ao tamanho da lista
            let deleted_items: Vec<Rc<ExtHostCell>> = {
                let mut cells = self.cells.borrow_mut();
                let begin = start.min(cells.len());
                let end = begin.saturating_add(delete_count).min(cells.len());
                cells.splice(begin..end, new_cells.iter().cloned()).collect()
            };
            let mut deleted_api_cells = Vec::with_capacity(deleted_items.len());
            for cell in &deleted_items {
                removed_cell_documents.push(cell.uri.clone());
                deleted_api_cells.push(cell.api_cell());
            }
            content_change_events.push(RawContentChangeEvent { start, deleted_count: delete_count, deleted_items: deleted_api_cells, items: new_cells });
        }

        self.text_documents_and_editors.rpc_accept_documents_and_editors_delta(DocumentsAndEditorsDelta {
            added_documents: Some(added_cell_documents),
            removed_documents: Some(removed_cell_documents),
            ..DocumentsAndEditorsDelta::default()
        });

        if let Some(bucket) = bucket {
            for change_event in &content_change_events {
                bucket.push(change_event.as_api_event());
            }
        }
    }

    fn move_cells(&self, index: usize, length: usize, new_idx: usize, bucket: &mut Vec<NotebookDocumentContentChange>) {
        let cells: Vec<Rc<ExtHostCell>> = {
            let mut all = self.cells.borrow_mut();
            let begin = index.min(all.len());
            let end = begin.saturating_add(length).min(all.len());
            let moved: Vec<Rc<ExtHostCell>> = all.splice(begin..end, std::iter::empty()).collect();
            let at = new_idx.min(all.len());
            all.splice(at..at, moved.iter().cloned());
            moved
        };
        let changes = [
            RawContentChangeEvent { start: index, deleted_count: length, deleted_items: cells.iter().map(ExtHostCell::api_cell).collect(), items: Vec::new() },
            RawContentChangeEvent { start: new_idx, deleted_count: 0, deleted_items: Vec::new(), items: cells },
        ];
        for change in &changes {
            bucket.push(change.as_api_event());
        }
    }

    fn change_cell_language(&self, index: usize, new_language_id: &str) {
        let cell = self.cell_at_index(index);
        if cell.api_cell().document().language_id() != new_language_id {
            self.text_documents.rpc_accept_model_language_changed(&cell.uri, new_language_id);
        }
    }

    pub fn get_cell_from_api_cell(&self, api_cell: &NotebookCell) -> Option<Rc<ExtHostCell>> {
        self.cells.borrow().iter().find(|cell| Rc::ptr_eq(cell, api_cell.ext_host())).cloned()
    }

    pub fn get_cell_from_index(&self, index: usize) -> Option<Rc<ExtHostCell>> {
        self.cells.borrow().get(index).cloned()
    }

    pub fn get_cell(&self, cell_handle: i64) -> Option<Rc<ExtHostCell>> {
        self.cells.borrow().iter().find(|cell| cell.handle == cell_handle).cloned()
    }

    /// `getCellIndex(cell)`: `-1` se a célula não está mais no documento.
    pub fn get_cell_index(&self, cell: &Rc<ExtHostCell>) -> f64 {
        self.cells.borrow().iter().position(|candidate| Rc::ptr_eq(candidate, cell)).map_or(-1.0, |index| index as f64)
    }
}

impl IDisposable for ExtHostNotebookDocument {
    fn dispose(&self) {
        self.disposed.set(true);
    }
}

/// `vscode.NotebookDocument`.
#[derive(Clone)]
pub struct NotebookDocument(Rc<ExtHostNotebookDocument>);

impl NotebookDocument {
    /// A identidade (`a === b`).
    pub fn ptr_eq(a: &NotebookDocument, b: &NotebookDocument) -> bool {
        Rc::ptr_eq(&a.0, &b.0)
    }

    /// O objeto do host por trás do documento.
    pub fn ext_host(&self) -> &Rc<ExtHostNotebookDocument> {
        &self.0
    }

    pub fn uri(&self) -> &URI {
        &self.0.uri
    }

    pub fn version(&self) -> i64 {
        self.0.version_id.get()
    }

    pub fn notebook_type(&self) -> &str {
        &self.0.notebook_type
    }

    pub fn is_dirty(&self) -> bool {
        self.0.is_dirty.get()
    }

    pub fn is_untitled(&self) -> bool {
        self.0.uri.scheme() == schemas::UNTITLED
    }

    pub fn is_closed(&self) -> bool {
        self.0.disposed.get()
    }

    pub fn metadata(&self) -> Dynamic {
        self.0.metadata.borrow().clone()
    }

    pub fn cell_count(&self) -> usize {
        self.0.cells.borrow().len()
    }

    pub fn cell_at(&self, index: f64) -> Result<NotebookCell, Error> {
        let index = self.0.validate_index(index);
        let cell = usize::try_from(index).ok().and_then(|index| self.0.get_cell_from_index(index));
        match cell {
            Some(cell) => Ok(cell.api_cell()),
            None => Err(Error::new("Cannot read properties of undefined (reading 'apiCell')")),
        }
    }

    pub fn get_cells(&self, range: Option<&types::NotebookRange>) -> Vec<NotebookCell> {
        let cells = match range {
            Some(range) => self.0.get_cells(range),
            None => self.0.cells.borrow().clone(),
        };
        cells.iter().map(ExtHostCell::api_cell).collect()
    }

    pub fn save(&self) -> Promise<bool> {
        self.0.save()
    }

    pub fn debug_description(&self) -> String {
        format!("NotebookDocument({})", self.0.uri.to_string(false))
    }
}
