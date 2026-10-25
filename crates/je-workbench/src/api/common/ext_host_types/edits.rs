//! `extHostTypes/textEdit.ts`, `snippetString.ts`, `snippetTextEdit.ts` e `workspaceEdit.ts`.
//!
//! Divergências:
//! - Os setters de `TextEdit` que só validam `typeof` (`range`, `newText`, `newEol`) viram setters sem
//!   validação, porque o tipo já garante; `new TextEdit(range, null | undefined)` é `new_text: None`, que lê
//!   como `''`. `isTextEdit`, `isSnippetString` e `isSnippetTextEdit` (duck typing) não existem.
//! - `SnippetString#appendPlaceholder`, `#appendVariable` recebem [`SnippetContent`] (`string | (snippet) => unknown`);
//!   o número de tabstop (`number`) é [`f64`].
//! - `WorkspaceEdit#set(uri, edits)` recebe itens [`SetEditItem`] (a aresta é um [`TextEdit`], um
//!   [`SnippetTextEdit`] ou um [`NotebookEdit`], com metadados opcionais) e `set(uri, undefined)` é
//!   [`WorkspaceEdit::unset`].
//! - `ICellMetadataEdit | IDocumentMetadataEdit` (de `notebookCommon`, ainda não portado) é [`CellMetadataEdit`].
//! - `IFileOperationOptions.contents` é `Uint8Array | DataTransferFile`: só os bytes são aceitos, porque a
//!   `DataTransferFile` (assíncrona, interna) ainda não foi portada.
//! - O `ResourceMap` de `entries()` agrupa por `uri.toString()`, na ordem da primeira ocorrência.

use je_base::dynamic::{Dynamic, number_to_js_string};
use je_base::json::Value;
use je_base::uri::URI;

use super::notebook::{NotebookCellData, NotebookEdit};
use super::position_range::{Position, Range};
use super::{IconPath, json_object, uri_to_json};

api_enum!(EndOfLine { LF = 1, CRLF = 2 });

/// `TextEdit`.
#[derive(Clone, Debug, PartialEq)]
pub struct TextEdit {
    range: Range,
    new_text: Option<String>,
    new_eol: Option<EndOfLine>,
}

impl TextEdit {
    pub fn replace(range: Range, new_text: &str) -> TextEdit {
        TextEdit::new(range, Some(new_text))
    }

    pub fn insert(position: Position, new_text: &str) -> TextEdit {
        TextEdit::replace(Range::new(position, position), new_text)
    }

    pub fn delete(range: Range) -> TextEdit {
        TextEdit::replace(range, "")
    }

    pub fn set_end_of_line(eol: EndOfLine) -> TextEdit {
        let origin = Position::new(0.0, 0.0).expect("(0, 0) é válida");
        let mut edit = TextEdit::new(Range::new(origin, origin), Some(""));
        edit.new_eol = Some(eol);
        edit
    }

    /// `new TextEdit(range, newText)`; `None` é `null`/`undefined`.
    pub fn new(range: Range, new_text: Option<&str>) -> TextEdit {
        TextEdit { range, new_text: new_text.map(str::to_owned), new_eol: None }
    }

    pub fn range(&self) -> Range {
        self.range
    }

    pub fn set_range(&mut self, range: Range) {
        self.range = range;
    }

    /// `newText`: `''` quando não definido.
    pub fn new_text(&self) -> &str {
        self.new_text.as_deref().unwrap_or("")
    }

    pub fn set_new_text(&mut self, new_text: Option<&str>) {
        self.new_text = new_text.map(str::to_owned);
    }

    pub fn new_eol(&self) -> Option<EndOfLine> {
        self.new_eol
    }

    pub fn set_new_eol(&mut self, new_eol: Option<EndOfLine>) {
        self.new_eol = new_eol;
    }

    /// `toJSON()`: `{ range, newText, newEol }`, sem os campos `undefined`.
    pub fn to_json(&self) -> Value {
        json_object(vec![
            ("range", Some(self.range.to_json())),
            ("newText", Some(Value::from(self.new_text()))),
            ("newEol", self.new_eol.map(|eol| Value::Number(f64::from(eol.value())))),
        ])
    }
}

/// `string | (snippet: SnippetString) => unknown`: o conteúdo de um placeholder ou o valor padrão de uma
/// variável.
#[derive(Clone, Copy)]
pub enum SnippetContent<'a> {
    Text(&'a str),
    Build(&'a dyn Fn(&mut SnippetString)),
}

/// `SnippetString`: texto de snippet montado com escapes corretos.
#[derive(Clone, Debug, PartialEq)]
pub struct SnippetString {
    tabstop: f64,
    pub value: String,
}

impl Default for SnippetString {
    fn default() -> Self {
        SnippetString { tabstop: 1.0, value: String::new() }
    }
}

/// `value.replace(/\$|}|\\/g, '\\$&')`.
fn escape_text(value: &str) -> String {
    escape_chars(value, &['$', '}', '\\'])
}

fn escape_chars(value: &str, special: &[char]) -> String {
    let mut out = String::with_capacity(value.len());
    for c in value.chars() {
        if special.contains(&c) {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

impl SnippetString {
    pub fn new(value: Option<&str>) -> SnippetString {
        SnippetString { tabstop: 1.0, value: value.unwrap_or("").to_owned() }
    }

    /// Um snippet que continua de onde parou: o `value` e o contador `_tabstop` de um objeto que a extensão já vinha montando.
    pub fn resume(value: &str, tabstop: f64) -> SnippetString {
        SnippetString { tabstop, value: value.to_owned() }
    }

    /// O contador `_tabstop`: o número que o próximo `appendTabstop()` sem argumento usa.
    pub fn tabstop(&self) -> f64 {
        self.tabstop
    }

    /// O `number = this._tabstop++` do parâmetro padrão: só consome o contador quando não há número.
    fn take_tabstop(&mut self, number: Option<f64>) -> f64 {
        number.unwrap_or_else(|| {
            let current = self.tabstop;
            self.tabstop += 1.0;
            current
        })
    }

    /// Roda `build` num `SnippetString` aninhado que continua a contagem de tabstops deste.
    fn build_nested(&mut self, build: &dyn Fn(&mut SnippetString)) -> String {
        let mut nested = SnippetString::new(None);
        nested.tabstop = self.tabstop;
        build(&mut nested);
        self.tabstop = nested.tabstop;
        nested.value
    }

    pub fn append_text(&mut self, string: &str) -> &mut Self {
        self.value.push_str(&escape_text(string));
        self
    }

    pub fn append_tabstop(&mut self, number: Option<f64>) -> &mut Self {
        let number = self.take_tabstop(number);
        self.value.push('$');
        self.value.push_str(&number_to_js_string(number));
        self
    }

    pub fn append_placeholder(&mut self, value: SnippetContent<'_>, number: Option<f64>) -> &mut Self {
        let number = self.take_tabstop(number);
        let value = match value {
            SnippetContent::Build(build) => self.build_nested(build),
            SnippetContent::Text(text) => escape_text(text),
        };
        self.value.push_str("${");
        self.value.push_str(&number_to_js_string(number));
        self.value.push(':');
        self.value.push_str(&value);
        self.value.push('}');
        self
    }

    pub fn append_choice(&mut self, values: &[&str], number: Option<f64>) -> &mut Self {
        let number = self.take_tabstop(number);
        let value = values.iter().map(|s| escape_chars(s, &['|', '\\', ','])).collect::<Vec<_>>().join(",");
        self.value.push_str("${");
        self.value.push_str(&number_to_js_string(number));
        self.value.push('|');
        self.value.push_str(&value);
        self.value.push_str("|}");
        self
    }

    pub fn append_variable(&mut self, name: &str, default_value: Option<SnippetContent<'_>>) -> &mut Self {
        let default_value = match default_value {
            Some(SnippetContent::Build(build)) => self.build_nested(build),
            // as barras invertidas não são escapadas aqui, de propósito
            Some(SnippetContent::Text(text)) => escape_chars(text, &['$', '}']),
            None => String::new(),
        };
        self.value.push_str("${");
        self.value.push_str(name);
        if !default_value.is_empty() {
            self.value.push(':');
            self.value.push_str(&default_value);
        }
        self.value.push('}');
        self
    }
}

/// `SnippetTextEdit`.
#[derive(Clone, Debug, PartialEq)]
pub struct SnippetTextEdit {
    pub range: Range,
    pub snippet: SnippetString,
    pub keep_whitespace: Option<bool>,
}

impl SnippetTextEdit {
    /// `SnippetTextEdit.replace(range, snippet)` é [`SnippetTextEdit::new`] (o original só o repassa).
    pub fn insert(position: Position, snippet: SnippetString) -> SnippetTextEdit {
        SnippetTextEdit::new(Range::new(position, position), snippet)
    }

    pub fn new(range: Range, snippet: SnippetString) -> SnippetTextEdit {
        SnippetTextEdit { range, snippet, keep_whitespace: None }
    }
}

// ---- WorkspaceEdit -----------------------------------------------------------------------------------

api_enum!(FileEditType { File = 1, Text = 2, Cell = 3, CellReplace = 5, Snippet = 6 });

/// `vscode.WorkspaceEditEntryMetadata`.
#[derive(Clone)]
pub struct WorkspaceEditEntryMetadata {
    pub needs_confirmation: bool,
    pub label: String,
    pub description: Option<String>,
    pub icon_path: Option<IconPath>,
}

/// `IFileOperationOptions`.
#[derive(Clone, Debug, Default)]
pub struct FileOperationOptions {
    pub overwrite: Option<bool>,
    pub ignore_if_exists: Option<bool>,
    pub ignore_if_not_exists: Option<bool>,
    pub recursive: Option<bool>,
    pub contents: Option<Vec<u8>>,
}

/// `IFileOperation`.
#[derive(Clone)]
pub struct FileOperation {
    pub from: Option<URI>,
    pub to: Option<URI>,
    pub options: Option<FileOperationOptions>,
    pub metadata: Option<WorkspaceEditEntryMetadata>,
}

/// `IFileTextEdit`.
#[derive(Clone)]
pub struct FileTextEdit {
    pub uri: URI,
    pub edit: TextEdit,
    pub metadata: Option<WorkspaceEditEntryMetadata>,
}

/// `IFileSnippetTextEdit`.
#[derive(Clone)]
pub struct FileSnippetTextEdit {
    pub uri: URI,
    pub range: Range,
    pub edit: SnippetString,
    pub metadata: Option<WorkspaceEditEntryMetadata>,
    pub keep_whitespace: Option<bool>,
}

/// `ICellMetadataEdit | IDocumentMetadataEdit`.
#[derive(Clone)]
pub enum CellMetadataEdit {
    Document { metadata: Dynamic },
    Cell { index: f64, metadata: Dynamic },
}

/// `IFileCellEdit`.
#[derive(Clone)]
pub struct FileCellEdit {
    pub uri: URI,
    pub edit: CellMetadataEdit,
    pub metadata: Option<WorkspaceEditEntryMetadata>,
}

/// `ICellEdit`.
#[derive(Clone)]
pub struct CellReplaceEdit {
    pub metadata: Option<WorkspaceEditEntryMetadata>,
    pub uri: URI,
    pub index: f64,
    pub count: f64,
    pub cells: Vec<NotebookCellData>,
}

/// `WorkspaceEditEntry`: uma das edições, na ordem em que foram registradas.
#[derive(Clone)]
pub enum WorkspaceEditEntry {
    File(FileOperation),
    Text(FileTextEdit),
    Snippet(FileSnippetTextEdit),
    Cell(FileCellEdit),
    CellReplace(CellReplaceEdit),
}

impl WorkspaceEditEntry {
    /// O discriminante `_type`.
    pub fn edit_type(&self) -> FileEditType {
        match self {
            Self::File(_) => FileEditType::File,
            Self::Text(_) => FileEditType::Text,
            Self::Snippet(_) => FileEditType::Snippet,
            Self::Cell(_) => FileEditType::Cell,
            Self::CellReplace(_) => FileEditType::CellReplace,
        }
    }

    pub fn metadata(&self) -> Option<&WorkspaceEditEntryMetadata> {
        match self {
            Self::File(entry) => entry.metadata.as_ref(),
            Self::Text(entry) => entry.metadata.as_ref(),
            Self::Snippet(entry) => entry.metadata.as_ref(),
            Self::Cell(entry) => entry.metadata.as_ref(),
            Self::CellReplace(entry) => entry.metadata.as_ref(),
        }
    }

    /// O recurso da edição de conteúdo; as operações de arquivo (`File`) não têm um só.
    fn content_uri(&self) -> Option<&URI> {
        match self {
            Self::File(_) => None,
            Self::Text(entry) => Some(&entry.uri),
            Self::Snippet(entry) => Some(&entry.uri),
            Self::Cell(entry) => Some(&entry.uri),
            Self::CellReplace(entry) => Some(&entry.uri),
        }
    }
}

/// O que `WorkspaceEdit#set` aceita como edição: `TextEdit | SnippetTextEdit | NotebookEdit`.
#[derive(Clone)]
pub enum SetEdit {
    Text(TextEdit),
    Snippet(SnippetTextEdit),
    Notebook(NotebookEdit),
}

/// Uma edição de `WorkspaceEdit#set`, com ou sem a tupla de metadados.
#[derive(Clone)]
pub struct SetEditItem {
    pub edit: SetEdit,
    pub metadata: Option<WorkspaceEditEntryMetadata>,
}

impl From<TextEdit> for SetEditItem {
    fn from(edit: TextEdit) -> Self {
        SetEditItem { edit: SetEdit::Text(edit), metadata: None }
    }
}

impl From<SnippetTextEdit> for SetEditItem {
    fn from(edit: SnippetTextEdit) -> Self {
        SetEditItem { edit: SetEdit::Snippet(edit), metadata: None }
    }
}

impl From<NotebookEdit> for SetEditItem {
    fn from(edit: NotebookEdit) -> Self {
        SetEditItem { edit: SetEdit::Notebook(edit), metadata: None }
    }
}

impl From<(TextEdit, Option<WorkspaceEditEntryMetadata>)> for SetEditItem {
    fn from((edit, metadata): (TextEdit, Option<WorkspaceEditEntryMetadata>)) -> Self {
        SetEditItem { edit: SetEdit::Text(edit), metadata }
    }
}

impl From<(SnippetTextEdit, Option<WorkspaceEditEntryMetadata>)> for SetEditItem {
    fn from((edit, metadata): (SnippetTextEdit, Option<WorkspaceEditEntryMetadata>)) -> Self {
        SetEditItem { edit: SetEdit::Snippet(edit), metadata }
    }
}

impl From<(NotebookEdit, Option<WorkspaceEditEntryMetadata>)> for SetEditItem {
    fn from((edit, metadata): (NotebookEdit, Option<WorkspaceEditEntryMetadata>)) -> Self {
        SetEditItem { edit: SetEdit::Notebook(edit), metadata }
    }
}

/// O `uri.toString()` com que o original compara recursos.
fn same_resource(a: &URI, b: &URI) -> bool {
    a.to_string(false) == b.to_string(false)
}

/// `WorkspaceEdit`.
#[derive(Clone, Default)]
pub struct WorkspaceEdit {
    edits: Vec<WorkspaceEditEntry>,
}

impl WorkspaceEdit {
    pub fn new() -> WorkspaceEdit {
        WorkspaceEdit::default()
    }

    pub fn all_entries(&self) -> &[WorkspaceEditEntry] {
        &self.edits
    }

    // --- file

    pub fn rename_file(&mut self, from: URI, to: URI, options: Option<FileOperationOptions>, metadata: Option<WorkspaceEditEntryMetadata>) {
        self.edits.push(WorkspaceEditEntry::File(FileOperation { from: Some(from), to: Some(to), options, metadata }));
    }

    pub fn create_file(&mut self, uri: URI, options: Option<FileOperationOptions>, metadata: Option<WorkspaceEditEntryMetadata>) {
        self.edits.push(WorkspaceEditEntry::File(FileOperation { from: None, to: Some(uri), options, metadata }));
    }

    pub fn delete_file(&mut self, uri: URI, options: Option<FileOperationOptions>, metadata: Option<WorkspaceEditEntryMetadata>) {
        self.edits.push(WorkspaceEditEntry::File(FileOperation { from: Some(uri), to: None, options, metadata }));
    }

    // --- notebook

    fn replace_notebook_metadata(&mut self, uri: &URI, value: Dynamic, metadata: Option<WorkspaceEditEntryMetadata>) {
        self.edits.push(WorkspaceEditEntry::Cell(FileCellEdit {
            uri: uri.clone(),
            edit: CellMetadataEdit::Document { metadata: value },
            metadata,
        }));
    }

    fn replace_notebook_cells(
        &mut self,
        uri: &URI,
        range: super::notebook::NotebookRange,
        cell_data: Vec<NotebookCellData>,
        metadata: Option<WorkspaceEditEntryMetadata>,
    ) {
        let (start, end) = (range.start(), range.end());
        if start != end || !cell_data.is_empty() {
            self.edits.push(WorkspaceEditEntry::CellReplace(CellReplaceEdit {
                uri: uri.clone(),
                index: start,
                count: end - start,
                cells: cell_data,
                metadata,
            }));
        }
    }

    fn replace_notebook_cell_metadata(&mut self, uri: &URI, index: f64, cell_metadata: Dynamic, metadata: Option<WorkspaceEditEntryMetadata>) {
        self.edits.push(WorkspaceEditEntry::Cell(FileCellEdit {
            uri: uri.clone(),
            edit: CellMetadataEdit::Cell { index, metadata: cell_metadata },
            metadata,
        }));
    }

    // --- text

    pub fn replace(&mut self, uri: &URI, range: Range, new_text: &str, metadata: Option<WorkspaceEditEntryMetadata>) {
        self.edits.push(WorkspaceEditEntry::Text(FileTextEdit { uri: uri.clone(), edit: TextEdit::new(range, Some(new_text)), metadata }));
    }

    pub fn insert(&mut self, resource: &URI, position: Position, new_text: &str, metadata: Option<WorkspaceEditEntryMetadata>) {
        self.replace(resource, Range::new(position, position), new_text, metadata);
    }

    pub fn delete(&mut self, resource: &URI, range: Range, metadata: Option<WorkspaceEditEntryMetadata>) {
        self.replace(resource, range, "", metadata);
    }

    // --- text (Maplike)

    pub fn has(&self, uri: &URI) -> bool {
        self.edits.iter().any(|edit| matches!(edit, WorkspaceEditEntry::Text(text) if same_resource(&text.uri, uri)))
    }

    /// `set(uri, undefined | null)`: remove todas as edições de texto, de snippet e de notebook de `uri`.
    pub fn unset(&mut self, uri: &URI) {
        self.edits.retain(|entry| !entry.content_uri().is_some_and(|candidate| same_resource(candidate, uri)));
    }

    /// `set(uri, edits)`: acrescenta as edições ao fim.
    pub fn set<I: Into<SetEditItem>>(&mut self, uri: &URI, edits: Vec<I>) {
        for item in edits {
            let SetEditItem { edit, metadata } = item.into();
            match edit {
                SetEdit::Notebook(notebook_edit) => {
                    let NotebookEdit { range, new_cells, new_cell_metadata, new_notebook_metadata } = notebook_edit;
                    if let Some(cell_metadata) = new_cell_metadata {
                        self.replace_notebook_cell_metadata(uri, range.start(), cell_metadata, metadata);
                    } else if let Some(notebook_metadata) = new_notebook_metadata {
                        self.replace_notebook_metadata(uri, notebook_metadata, metadata);
                    } else {
                        self.replace_notebook_cells(uri, range, new_cells, metadata);
                    }
                }
                SetEdit::Snippet(snippet_edit) => {
                    self.edits.push(WorkspaceEditEntry::Snippet(FileSnippetTextEdit {
                        uri: uri.clone(),
                        range: snippet_edit.range,
                        edit: snippet_edit.snippet,
                        metadata,
                        keep_whitespace: snippet_edit.keep_whitespace,
                    }));
                }
                SetEdit::Text(text_edit) => {
                    self.edits.push(WorkspaceEditEntry::Text(FileTextEdit { uri: uri.clone(), edit: text_edit, metadata }));
                }
            }
        }
    }

    pub fn get(&self, uri: &URI) -> Vec<TextEdit> {
        self.edits
            .iter()
            .filter_map(|candidate| match candidate {
                WorkspaceEditEntry::Text(text) if same_resource(&text.uri, uri) => Some(text.edit.clone()),
                _ => None,
            })
            .collect()
    }

    pub fn entries(&self) -> Vec<(URI, Vec<TextEdit>)> {
        let mut grouped: Vec<(URI, Vec<TextEdit>)> = Vec::new();
        for candidate in &self.edits {
            if let WorkspaceEditEntry::Text(text) = candidate {
                match grouped.iter_mut().find(|group| same_resource(&group.0, &text.uri)) {
                    Some(group) => group.1.push(text.edit.clone()),
                    None => grouped.push((text.uri.clone(), vec![text.edit.clone()])),
                }
            }
        }
        grouped
    }

    pub fn size(&self) -> usize {
        self.entries().len()
    }

    /// `toJSON()`: `[[uri, [edit, ...]], ...]`.
    pub fn to_json(&self) -> Value {
        Value::Array(
            self.entries()
                .iter()
                .map(|(uri, edits)| Value::Array(vec![uri_to_json(uri), Value::Array(edits.iter().map(TextEdit::to_json).collect())]))
                .collect(),
        )
    }
}
