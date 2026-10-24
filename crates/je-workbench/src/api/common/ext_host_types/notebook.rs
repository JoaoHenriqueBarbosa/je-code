//! `extHostTypes/notebooks.ts`.
//!
//! Divergências:
//! - `Record<string, unknown>` (metadados) é [`Dynamic`].
//! - `NotebookCellData.validate` confere `typeof` de `kind`, `value` e `languageId`: o tipo já garante,
//!   então o construtor não lança. `isNotebookCellData`/`isNotebookCellDataArray` (sempre `true` no
//!   original) e os demais `is*` com duck typing não existem.
//! - `NotebookCellOutput.ensureUniqueMimeTypes(items, warn)` não tem `warn`: não há `console.warn`.
//! - `new NotebookCellOutput(items, idOrMetadata?, metadata?)` recebe [`IdOrMetadata`].
//! - `NotebookCellOutputItem.error` aceita [`ErrorLike`] (um `Error` ou `{ name, message?, stack? }`).
//! - `NotebookCellOutputItem.json` recebe um [`Value`] e serializa com tabulação, como o
//!   `JSON.stringify(value, undefined, '\t')` do original.

use je_base::dynamic::Dynamic;
use je_base::errors::Error;
use je_base::json::Value;
use je_base::mime::{is_text_stream_mime, mimes, normalize_mime_type, normalize_mime_type_strict};
use je_base::uuid::generate_uuid;

use super::{illegal_argument, json_object, json_stringify_tabs};

api_enum!(NotebookCellKind { Markup = 1, Code = 2 });

/// `NotebookRange`: índices de células, ordenados, nunca negativos.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NotebookRange {
    start: f64,
    end: f64,
}

impl NotebookRange {
    pub fn new(start: f64, end: f64) -> Result<NotebookRange, Error> {
        if start < 0.0 {
            return Err(illegal_argument(Some("start must be positive")));
        }
        if end < 0.0 {
            return Err(illegal_argument(Some("end must be positive")));
        }
        Ok(if start <= end { NotebookRange { start, end } } else { NotebookRange { start: end, end: start } })
    }

    pub fn start(&self) -> f64 {
        self.start
    }

    pub fn end(&self) -> f64 {
        self.end
    }

    pub fn is_empty(&self) -> bool {
        self.start == self.end
    }

    /// `with({ start?, end? })`; sem mudança devolve o próprio intervalo.
    pub fn with(&self, start: Option<f64>, end: Option<f64>) -> Result<NotebookRange, Error> {
        let start = start.unwrap_or(self.start);
        let end = end.unwrap_or(self.end);
        if start == self.start && end == self.end {
            return Ok(*self);
        }
        NotebookRange::new(start, end)
    }
}

/// `NotebookCellOutputItem`: bytes e tipo MIME normalizado.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NotebookCellOutputItem {
    pub data: Vec<u8>,
    mime: String,
}

/// O argumento de `NotebookCellOutputItem.error`: `Error | { name: string; message?: string; stack?: string }`.
#[derive(Clone, Debug, Default)]
pub struct ErrorLike {
    pub name: String,
    pub message: Option<String>,
    pub stack: Option<String>,
}

impl From<&Error> for ErrorLike {
    fn from(error: &Error) -> Self {
        ErrorLike { name: error.name.clone(), message: Some(error.message.clone()), stack: error.stack.clone() }
    }
}

impl NotebookCellOutputItem {
    /// `new NotebookCellOutputItem(data, mime)`: lança se o tipo MIME não for `tipo/subtipo[;parâmetro]`.
    pub fn new(data: Vec<u8>, mime: &str) -> Result<NotebookCellOutputItem, Error> {
        let mime = normalize_mime_type_strict(mime).ok_or_else(|| {
            Error::new(format!("INVALID mime type: {mime}. Must be in the format \"type/subtype[;optionalparameter]\""))
        })?;
        Ok(NotebookCellOutputItem { data, mime })
    }

    pub fn mime(&self) -> &str {
        &self.mime
    }

    /// `NotebookCellOutputItem.error(err)`.
    pub fn error(err: &ErrorLike) -> NotebookCellOutputItem {
        let value = json_object(vec![
            ("name", Some(Value::from(err.name.as_str()))),
            ("message", err.message.as_deref().map(Value::from)),
            ("stack", err.stack.as_deref().map(Value::from)),
        ]);
        NotebookCellOutputItem::json(&value, Some("application/vnd.code.notebook.error"))
    }

    pub fn stdout(value: &str) -> NotebookCellOutputItem {
        NotebookCellOutputItem::text(value, Some("application/vnd.code.notebook.stdout"))
    }

    pub fn stderr(value: &str) -> NotebookCellOutputItem {
        NotebookCellOutputItem::text(value, Some("application/vnd.code.notebook.stderr"))
    }

    /// `NotebookCellOutputItem.bytes(value, mime = 'application/octet-stream')`.
    pub fn bytes(value: Vec<u8>, mime: Option<&str>) -> Result<NotebookCellOutputItem, Error> {
        NotebookCellOutputItem::new(value, mime.unwrap_or("application/octet-stream"))
    }

    /// `NotebookCellOutputItem.text(value, mime = Mimes.text)`. O MIME padrão e os fixos dos atalhos são
    /// válidos, então só um MIME inválido passado pelo chamador lançaria no original; aqui um MIME inválido
    /// é um erro de programação do chamador (`panic`), igual à exceção do original.
    pub fn text(value: &str, mime: Option<&str>) -> NotebookCellOutputItem {
        let mime = mime.unwrap_or(mimes::TEXT);
        NotebookCellOutputItem::new(value.as_bytes().to_vec(), mime).unwrap_or_else(|error| je_base::errors::throw(error))
    }

    /// `NotebookCellOutputItem.json(value, mime = 'text/x-json')`.
    pub fn json(value: &Value, mime: Option<&str>) -> NotebookCellOutputItem {
        NotebookCellOutputItem::text(&json_stringify_tabs(value), Some(mime.unwrap_or("text/x-json")))
    }
}

/// O segundo argumento do construtor de `NotebookCellOutput`: `string | Record<string, unknown>`.
#[derive(Clone)]
pub enum IdOrMetadata {
    Id(String),
    Metadata(Dynamic),
}

/// `NotebookCellOutput`.
#[derive(Clone)]
pub struct NotebookCellOutput {
    pub id: String,
    pub items: Vec<NotebookCellOutputItem>,
    pub metadata: Option<Dynamic>,
}

impl NotebookCellOutput {
    /// Mantém o primeiro item de cada tipo MIME (o normalizado), exceto os de fluxo de texto
    /// (`stdout`, `stderr`), que podem se repetir.
    pub fn ensure_unique_mime_types(items: Vec<NotebookCellOutputItem>) -> Vec<NotebookCellOutputItem> {
        let mut seen: Vec<String> = Vec::new();
        let mut kept = Vec::with_capacity(items.len());
        for item in items {
            let normal_mime = normalize_mime_type(item.mime());
            // Pode haver vários tipos de fluxo de texto na mesma saída.
            if !seen.contains(&normal_mime) || is_text_stream_mime(&normal_mime) {
                seen.push(normal_mime);
                kept.push(item);
            }
            // tipo MIME duplicado: o primeiro vence
        }
        kept
    }

    pub fn new(items: Vec<NotebookCellOutputItem>, id_or_metadata: Option<IdOrMetadata>, metadata: Option<Dynamic>) -> NotebookCellOutput {
        let items = NotebookCellOutput::ensure_unique_mime_types(items);
        match id_or_metadata {
            Some(IdOrMetadata::Id(id)) => NotebookCellOutput { id, items, metadata },
            Some(IdOrMetadata::Metadata(value)) => NotebookCellOutput { id: generate_uuid(), items, metadata: Some(value) },
            None => NotebookCellOutput { id: generate_uuid(), items, metadata },
        }
    }
}

/// `NotebookCellData`.
#[derive(Clone)]
pub struct NotebookCellData {
    pub kind: NotebookCellKind,
    pub value: String,
    pub language_id: String,
    pub mime: Option<String>,
    pub outputs: Vec<NotebookCellOutput>,
    pub metadata: Option<Dynamic>,
    pub execution_summary: Option<Dynamic>,
}

impl NotebookCellData {
    pub fn new(
        kind: NotebookCellKind,
        value: impl Into<String>,
        language_id: impl Into<String>,
        mime: Option<String>,
        outputs: Option<Vec<NotebookCellOutput>>,
        metadata: Option<Dynamic>,
        execution_summary: Option<Dynamic>,
    ) -> NotebookCellData {
        NotebookCellData {
            kind,
            value: value.into(),
            language_id: language_id.into(),
            mime,
            outputs: outputs.unwrap_or_default(),
            metadata,
            execution_summary,
        }
    }
}

/// `NotebookData`.
#[derive(Clone)]
pub struct NotebookData {
    pub cells: Vec<NotebookCellData>,
    pub metadata: Option<Dynamic>,
}

impl NotebookData {
    pub fn new(cells: Vec<NotebookCellData>) -> NotebookData {
        NotebookData { cells, metadata: None }
    }
}

/// `string | vscode.Command`: o `command` de `NotebookCellStatusBarItem`.
#[derive(Clone, Debug)]
pub enum NotebookCellStatusBarCommand {
    Id(String),
    Command(super::Command),
}

/// `NotebookCellStatusBarItem`.
#[derive(Clone, Debug)]
pub struct NotebookCellStatusBarItem {
    pub text: String,
    pub alignment: super::NotebookCellStatusBarAlignment,
    pub command: Option<NotebookCellStatusBarCommand>,
    pub tooltip: Option<String>,
    pub priority: Option<f64>,
    pub accessibility_information: Option<je_platform::accessibility::IAccessibilityInformation>,
}

impl NotebookCellStatusBarItem {
    pub fn new(text: impl Into<String>, alignment: super::NotebookCellStatusBarAlignment) -> NotebookCellStatusBarItem {
        NotebookCellStatusBarItem { text: text.into(), alignment, command: None, tooltip: None, priority: None, accessibility_information: None }
    }
}

/// `vscode.NotebookDocumentContentOptions`: os quatro campos são opcionais.
#[derive(Clone, Debug, Default)]
pub struct NotebookDocumentContentOptions {
    pub transient_outputs: Option<bool>,
    pub transient_cell_metadata: Option<std::collections::HashMap<String, bool>>,
    pub transient_document_metadata: Option<std::collections::HashMap<String, bool>>,
    pub cell_content_metadata: Option<std::collections::HashMap<String, bool>>,
}

/// `NotebookEdit`.
#[derive(Clone)]
pub struct NotebookEdit {
    pub range: NotebookRange,
    pub new_cells: Vec<NotebookCellData>,
    pub new_cell_metadata: Option<Dynamic>,
    pub new_notebook_metadata: Option<Dynamic>,
}

impl NotebookEdit {
    pub fn new(range: NotebookRange, new_cells: Vec<NotebookCellData>) -> NotebookEdit {
        NotebookEdit { range, new_cells, new_cell_metadata: None, new_notebook_metadata: None }
    }

    /// `NotebookEdit.replaceCells(range, newCells)` é [`NotebookEdit::new`] (o original só o repassa).
    pub fn insert_cells(index: f64, new_cells: Vec<NotebookCellData>) -> Result<NotebookEdit, Error> {
        Ok(NotebookEdit::new(NotebookRange::new(index, index)?, new_cells))
    }

    pub fn delete_cells(range: NotebookRange) -> NotebookEdit {
        NotebookEdit::new(range, Vec::new())
    }

    pub fn update_cell_metadata(index: f64, new_metadata: Dynamic) -> Result<NotebookEdit, Error> {
        let mut edit = NotebookEdit::new(NotebookRange::new(index, index)?, Vec::new());
        edit.new_cell_metadata = Some(new_metadata);
        Ok(edit)
    }

    pub fn update_notebook_metadata(new_metadata: Dynamic) -> NotebookEdit {
        let mut edit = NotebookEdit::new(NotebookRange { start: 0.0, end: 0.0 }, Vec::new());
        edit.new_notebook_metadata = Some(new_metadata);
        edit
    }
}
