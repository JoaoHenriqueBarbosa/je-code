//! Porte de `src/vs/editor/browser/controller/editContext/clipboardUtils.ts`: os dados que uma cópia ou um recorte
//! grava na área de transferência, os metadados do editor (`vscode-editor-data`) que viajam com o texto, o
//! gerenciador de metadados em memória e os eventos de cópia e de colagem.
//!
//! Representação:
//! - O `DataTransfer`, o `File` e o `ClipboardEvent` do DOM são os dublês [`DataTransfer`], [`File`] e
//!   [`ClipboardEvent`], com o que o original observa deles: `getData`, `setData`, `clearData`, `types`, `files`,
//!   `preventDefault` e `stopImmediatePropagation` (os dois últimos viram os marcadores
//!   [`ClipboardEvent::default_prevented`] e [`ClipboardEvent::immediate_propagation_stopped`]).
//! - `ViewContext` só é lido pelo `context.viewModel`: as funções recebem o modelo de visão (qualquer [`IViewModel`]).
//! - Os objetos literais `IClipboardCopyEvent` e `IClipboardPasteEvent` são as structs [`ClipboardCopyEvent`] e
//!   [`ClipboardPasteEvent`], com os mesmos membros como campos e métodos. `IReadableClipboardData` e
//!   `IWritableClipboardData` são [`ReadableClipboardData`] e [`WritableClipboardData`].
//! - O `InMemoryClipboardMetadataManager.INSTANCE` é a instância da thread ([`in_memory_clipboard_metadata_manager`]),
//!   como os demais singletons do crate; `CopyOptions` são os seletores [`CopyOptions`].
//! - `string[] | null | undefined` de `multicursorText` é um `Option<Vec<JsString>>` (`null` e `undefined` coincidem:
//!   o `JSON.stringify` do original grava `null` para o `null` e omite o `undefined`, e a leitura devolve o mesmo
//!   `Option` nos dois casos; os dois usos do original tratam `null` e `undefined` igual).
//!
//! Divergências:
//! - `ILogService.trace` (só diagnóstico) não existe no porte: `createClipboardCopyEvent` não recebe o serviço.
//! - `toExternalVSDataTransfer()` do evento de colagem depende de `DataTransfer.items` e de `getPathForFile`
//!   (Electron) e não é portado aqui: o que o editor consome da colagem é `text`, `metadata` e `clipboardData`.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use je_base::JsString;
use je_base::dynamic::{Dynamic, json_stringify};
use je_base::json;
use je_base::platform::Platform;
use je_base::uuid::generate_uuid;

use crate::config::editor_options::empty_selection_clipboard::EMPTY_SELECTION_CLIPBOARD;
use crate::config::editor_options::registry::COPY_WITH_SYNTAX_HIGHLIGHTING;
use crate::range::Range;
use crate::view_model_types::{IViewModel, PlainTextSource};

/// `Mimes.text`.
const MIME_TEXT: &str = "text/plain";
/// O tipo do metadado do editor no `DataTransfer`.
const MIME_EDITOR_DATA: &str = "vscode-editor-data";

// #region dublês do DOM

/// `File`: o que o original lê dele (`name`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct File {
    pub name: String,
}

/// `DataTransfer`: os dados por tipo (em ordem de inserção) e os arquivos.
#[derive(Default)]
pub struct DataTransfer {
    data: RefCell<Vec<(String, JsString)>>,
    files: RefCell<Vec<File>>,
}

impl DataTransfer {
    pub fn new() -> Self {
        Self::default()
    }

    /// `setData(format, data)`: troca o valor do tipo, se já existe.
    pub fn set_data(&self, format: &str, data: impl Into<JsString>) {
        let format = format.to_lowercase();
        let data = data.into();
        let mut entries = self.data.borrow_mut();
        match entries.iter_mut().find(|(key, _)| *key == format) {
            Some(entry) => entry.1 = data,
            None => entries.push((format, data)),
        }
    }

    /// `getData(format)`: a string vazia quando não há o tipo.
    pub fn get_data(&self, format: &str) -> JsString {
        let format = format.to_lowercase();
        self.data.borrow().iter().find(|(key, _)| *key == format).map(|(_, value)| value.clone()).unwrap_or_default()
    }

    /// `clearData(format)`.
    pub fn clear_data(&self, format: &str) {
        let format = format.to_lowercase();
        self.data.borrow_mut().retain(|(key, _)| *key != format);
    }

    /// `types`: os tipos presentes, mais `Files` quando há arquivos.
    pub fn types(&self) -> Vec<String> {
        let mut types: Vec<String> = self.data.borrow().iter().map(|(key, _)| key.clone()).collect();
        if !self.files.borrow().is_empty() {
            types.push("Files".to_string());
        }
        types
    }

    /// `files`.
    pub fn files(&self) -> Vec<File> {
        self.files.borrow().clone()
    }

    /// Acrescenta um arquivo (o que o navegador faz quando o usuário cola arquivos).
    pub fn add_file(&self, file: File) {
        self.files.borrow_mut().push(file);
    }
}

/// `ClipboardEvent`: o tipo, o `clipboardData` e os marcadores de `preventDefault` e `stopImmediatePropagation`.
pub struct ClipboardEvent {
    pub event_type: String,
    pub clipboard_data: Option<Rc<DataTransfer>>,
    default_prevented: Cell<bool>,
    immediate_propagation_stopped: Cell<bool>,
}

impl ClipboardEvent {
    /// `new ClipboardEvent(type, { clipboardData })`.
    pub fn new(event_type: &str, clipboard_data: Option<Rc<DataTransfer>>) -> Rc<Self> {
        Rc::new(Self {
            event_type: event_type.to_string(),
            clipboard_data,
            default_prevented: Cell::new(false),
            immediate_propagation_stopped: Cell::new(false),
        })
    }

    /// `preventDefault()`.
    pub fn prevent_default(&self) {
        self.default_prevented.set(true);
    }

    /// `defaultPrevented`.
    pub fn default_prevented(&self) -> bool {
        self.default_prevented.get()
    }

    /// `stopImmediatePropagation()`.
    pub fn stop_immediate_propagation(&self) {
        self.immediate_propagation_stopped.set(true);
    }

    /// Se `stopImmediatePropagation()` foi chamado.
    pub fn immediate_propagation_stopped(&self) -> bool {
        self.immediate_propagation_stopped.get()
    }
}

// #endregion

// #region tipos de dados

/// `ClipboardDataToCopy`.
#[derive(Clone, Debug, PartialEq)]
pub struct ClipboardDataToCopy {
    pub is_from_empty_selection: bool,
    pub is_block: bool,
    pub source_ranges: Vec<Range>,
    pub multicursor_text: Option<Vec<JsString>>,
    pub text: JsString,
    pub html: Option<String>,
    pub mode: Option<String>,
}

/// `ClipboardStoredMetadata` (a `version` vale sempre `1`).
#[derive(Clone, Debug, PartialEq)]
pub struct ClipboardStoredMetadata {
    pub id: Option<String>,
    pub is_from_empty_selection: Option<bool>,
    pub is_block: Option<bool>,
    pub multicursor_text: Option<Vec<JsString>>,
    pub mode: Option<String>,
}

impl ClipboardStoredMetadata {
    /// O objeto que o `JSON.stringify` do original serializa (as chaves `undefined` são omitidas).
    fn to_dynamic(&self) -> Dynamic {
        let mut entries: Vec<(&str, Dynamic)> = vec![("version", Dynamic::from(1_i32))];
        if let Some(id) = &self.id {
            entries.push(("id", Dynamic::from(id.as_str())));
        }
        if let Some(value) = self.is_from_empty_selection {
            entries.push(("isFromEmptySelection", Dynamic::from(value)));
        }
        if let Some(value) = self.is_block {
            entries.push(("isBlock", Dynamic::from(value)));
        }
        entries.push((
            "multicursorText",
            match &self.multicursor_text {
                Some(lines) => Dynamic::array(lines.iter().cloned().map(Dynamic::from)),
                None => Dynamic::Null,
            },
        ));
        entries.push((
            "mode",
            match &self.mode {
                Some(mode) => Dynamic::from(mode.as_str()),
                None => Dynamic::Null,
            },
        ));
        Dynamic::object(entries)
    }

    /// O `JSON.parse` seguido do teste `metadata.version !== 1`: `None` quando não é um objeto com `version` 1.
    fn from_dynamic(value: &Dynamic) -> Option<Self> {
        if !matches!(value, Dynamic::Object(_)) {
            return None;
        }
        match value.get_str("version") {
            Dynamic::Number(version) if version == 1.0 => {}
            _ => return None,
        }
        let text = |key: &str| match value.get_str(key) {
            Dynamic::String(text) => Some(text.to_string_lossy()),
            _ => None,
        };
        let flag = |key: &str| match value.get_str(key) {
            Dynamic::Bool(flag) => Some(flag),
            _ => None,
        };
        let multicursor_text = match value.get_str("multicursorText") {
            Dynamic::Array(lines) => Some(
                lines
                    .to_vec()
                    .into_iter()
                    .map(|line| match line {
                        Dynamic::String(text) => text,
                        _ => JsString::default(),
                    })
                    .collect(),
            ),
            _ => None,
        };
        Some(Self {
            id: text("id"),
            is_from_empty_selection: flag("isFromEmptySelection"),
            is_block: flag("isBlock"),
            multicursor_text,
            mode: text("mode"),
        })
    }
}

// #endregion

// #region CopyOptions

thread_local! {
    static FORCE_COPY_WITH_SYNTAX_HIGHLIGHTING: Cell<bool> = const { Cell::new(false) };
    static ELECTRON_BUG_WORKAROUND_COPY_EVENT_HAS_FIRED: Cell<bool> = const { Cell::new(false) };
}

/// `CopyOptions`: `forceCopyWithSyntaxHighlighting` e `electronBugWorkaroundCopyEventHasFired`.
pub struct CopyOptions;

impl CopyOptions {
    pub fn force_copy_with_syntax_highlighting() -> bool {
        FORCE_COPY_WITH_SYNTAX_HIGHLIGHTING.with(Cell::get)
    }

    pub fn set_force_copy_with_syntax_highlighting(value: bool) {
        FORCE_COPY_WITH_SYNTAX_HIGHLIGHTING.with(|cell| cell.set(value));
    }

    pub fn electron_bug_workaround_copy_event_has_fired() -> bool {
        ELECTRON_BUG_WORKAROUND_COPY_EVENT_HAS_FIRED.with(Cell::get)
    }

    pub fn set_electron_bug_workaround_copy_event_has_fired(value: bool) {
        ELECTRON_BUG_WORKAROUND_COPY_EVENT_HAS_FIRED.with(|cell| cell.set(value));
    }
}

// #endregion

// #region InMemoryClipboardMetadataManager

/// `InMemoryClipboardMetadata`.
struct InMemoryClipboardMetadata {
    last_copied_value: JsString,
    data: ClipboardStoredMetadata,
}

/// Toda vez que se escreve na área de transferência, guarda-se aqui um pouco de metadado extra. Toda vez que se
/// lê, se o texto bate com o último escrito, devolve-se o metadado anterior.
#[derive(Default)]
pub struct InMemoryClipboardMetadataManager {
    last_state: RefCell<Option<InMemoryClipboardMetadata>>,
}

impl InMemoryClipboardMetadataManager {
    pub fn new() -> Self {
        Self::default()
    }

    /// `set(lastCopiedValue, data)`.
    pub fn set(&self, last_copied_value: JsString, data: ClipboardStoredMetadata) {
        *self.last_state.borrow_mut() = Some(InMemoryClipboardMetadata { last_copied_value, data });
    }

    /// `get(pastedText)`: o metadado se o texto bate com o último copiado; senão esquece o estado.
    pub fn get(&self, pasted_text: &JsString) -> Option<ClipboardStoredMetadata> {
        let mut last_state = self.last_state.borrow_mut();
        if let Some(state) = last_state.as_ref() {
            if state.last_copied_value == *pasted_text {
                // match!
                return Some(state.data.clone());
            }
        }
        *last_state = None;
        None
    }
}

thread_local! {
    static INSTANCE: Rc<InMemoryClipboardMetadataManager> = Rc::new(InMemoryClipboardMetadataManager::new());
}

/// `InMemoryClipboardMetadataManager.INSTANCE`.
pub fn in_memory_clipboard_metadata_manager() -> Rc<InMemoryClipboardMetadataManager> {
    INSTANCE.with(Rc::clone)
}

// #endregion

// #region geração dos dados a copiar

/// `generateDataToCopyAndStoreInMemory(viewModel, id, isFirefox)`.
pub fn generate_data_to_copy_and_store_in_memory<V: IViewModel>(
    view_model: &V,
    is_firefox: bool,
) -> (ClipboardDataToCopy, ClipboardStoredMetadata) {
    let (data_to_copy, metadata) = generate_data_to_copy(view_model);
    store_metadata_in_memory(&data_to_copy.text, &metadata, is_firefox);
    (data_to_copy, metadata)
}

fn store_metadata_in_memory(text_to_copy: &JsString, metadata: &ClipboardStoredMetadata, is_firefox: bool) {
    // When writing "LINE\r\n" to the clipboard and then pasting,
    // Firefox pastes "LINE\n", so let's work around this quirk
    let value = if is_firefox { replace_crlf_with_lf(text_to_copy) } else { text_to_copy.clone() };
    in_memory_clipboard_metadata_manager().set(value, metadata.clone());
}

/// `text.replace(/\r\n/g, '\n')`.
fn replace_crlf_with_lf(text: &JsString) -> JsString {
    let units = text.units();
    let mut result: Vec<u16> = Vec::with_capacity(units.len());
    let mut index = 0;
    while index < units.len() {
        if units[index] == 13 && units.get(index + 1) == Some(&10) {
            result.push(10);
            index += 2;
        } else {
            result.push(units[index]);
            index += 1;
        }
    }
    JsString::from_units(result)
}

fn generate_data_to_copy<V: IViewModel>(view_model: &V) -> (ClipboardDataToCopy, ClipboardStoredMetadata) {
    let options = view_model.computed_editor_options();
    let empty_selection_clipboard = *options.get(EMPTY_SELECTION_CLIPBOARD);
    let copy_with_syntax_highlighting = *options.get(COPY_WITH_SYNTAX_HIGHLIGHTING);
    let selections: Vec<Range> = view_model.get_cursor_states().iter().map(|state| *state.model_state.selection).collect();
    let data_to_copy = get_data_to_copy(view_model, &selections, empty_selection_clipboard, copy_with_syntax_highlighting);
    let metadata = ClipboardStoredMetadata {
        id: Some(generate_uuid()),
        is_from_empty_selection: Some(data_to_copy.is_from_empty_selection),
        is_block: Some(data_to_copy.is_block),
        multicursor_text: data_to_copy.multicursor_text.clone(),
        mode: data_to_copy.mode.clone(),
    };
    (data_to_copy, metadata)
}

fn get_data_to_copy<V: IViewModel>(
    view_model: &V,
    model_selections: &[Range],
    empty_selection_clipboard: bool,
    copy_with_syntax_highlighting: bool,
) -> ClipboardDataToCopy {
    let is_block = view_model.get_cursor_column_select_data().is_real;
    let is_windows = Platform::detect().is_windows;
    let plain = view_model.get_plain_text_to_copy(model_selections, empty_selection_clipboard, is_windows);
    let new_line_character = view_model.model().get_eol();

    let is_from_empty_selection = empty_selection_clipboard && model_selections.len() == 1 && model_selections[0].is_empty();
    let (multicursor_text, text, source_length) = match plain.source_text {
        PlainTextSource::Lines(lines) => {
            let mut text = JsString::default();
            for (index, line) in lines.iter().enumerate() {
                if index > 0 {
                    text.push_units(new_line_character.as_units());
                }
                text.push_units(line.units());
            }
            let length = lines.len();
            (Some(lines), text, length)
        }
        PlainTextSource::Text(text) => {
            let length = text.len();
            (None, text, length)
        }
    };

    let mut html: Option<String> = None;
    let mut mode: Option<String> = None;
    if CopyOptions::force_copy_with_syntax_highlighting() || (copy_with_syntax_highlighting && source_length < 65536) {
        if let Some(rich_text) = view_model.get_rich_text_to_copy(model_selections, empty_selection_clipboard) {
            html = Some(rich_text.html);
            mode = Some(rich_text.mode);
        }
    }
    ClipboardDataToCopy {
        is_from_empty_selection,
        is_block,
        source_ranges: plain.source_ranges,
        multicursor_text,
        text,
        html,
        mode,
    }
}

// #endregion

// #region ClipboardEventUtils

/// `ClipboardEventUtils.getTextData(clipboardData)`.
fn get_text_data(clipboard_data: &DataTransfer) -> (JsString, Option<ClipboardStoredMetadata>) {
    let text = clipboard_data.get_data(MIME_TEXT);
    let raw_metadata = clipboard_data.get_data(MIME_EDITOR_DATA);
    let mut errors = Vec::new();
    let parsed = json::parse_str(&raw_metadata.to_string_lossy(), &mut errors);
    // `JSON.parse` lança com texto inválido (inclusive o vazio): o `catch` deixa o metadado nulo.
    let metadata = match parsed {
        Some(value) if errors.is_empty() => ClipboardStoredMetadata::from_dynamic(&Dynamic::from_json(&value)),
        _ => None,
    };
    let files = clipboard_data.files();
    if text.is_empty() && metadata.is_none() && !files.is_empty() {
        // no textual data pasted, generate text from file names
        let names: Vec<String> = files.into_iter().map(|file| file.name).collect();
        return (JsString::from(names.join("\n")), None);
    }
    (text, metadata)
}

/// `ClipboardEventUtils.setTextData(clipboardData, text, html, metadata)`.
fn set_text_data(clipboard_data: &DataTransfer, text: &JsString, html: Option<&str>, metadata: &ClipboardStoredMetadata) {
    clipboard_data.set_data(MIME_TEXT, text.clone());
    if let Some(html) = html {
        clipboard_data.set_data("text/html", html);
    }
    let serialized = json_stringify(&metadata.to_dynamic()).unwrap_or_default();
    clipboard_data.set_data(MIME_EDITOR_DATA, serialized);
}

// #endregion

// #region dados legíveis e graváveis

/// `IReadableClipboardData`.
pub struct ReadableClipboardData {
    pub types: Vec<String>,
    pub files: Vec<File>,
    data_transfer: Option<Rc<DataTransfer>>,
}

impl ReadableClipboardData {
    /// `getData(type)`: a string vazia sem dados.
    pub fn get_data(&self, mime_type: &str) -> JsString {
        self.data_transfer.as_ref().map(|transfer| transfer.get_data(mime_type)).unwrap_or_default()
    }
}

/// `IWritableClipboardData`.
pub struct WritableClipboardData {
    data_transfer: Option<Rc<DataTransfer>>,
}

impl WritableClipboardData {
    /// `setData(type, value)`.
    pub fn set_data(&self, mime_type: &str, value: impl Into<JsString>) {
        if let Some(transfer) = &self.data_transfer {
            transfer.set_data(mime_type, value);
        }
    }
}

/// `createReadableClipboardData(dataTransfer)`.
pub fn create_readable_clipboard_data(data_transfer: Option<Rc<DataTransfer>>) -> ReadableClipboardData {
    ReadableClipboardData {
        types: data_transfer.as_ref().map(|transfer| transfer.types()).unwrap_or_default(),
        files: data_transfer.as_ref().map(|transfer| transfer.files()).unwrap_or_default(),
        data_transfer,
    }
}

/// `createWritableClipboardData(dataTransfer)`.
pub fn create_writable_clipboard_data(data_transfer: Option<Rc<DataTransfer>>) -> WritableClipboardData {
    WritableClipboardData { data_transfer }
}

// #endregion

// #region eventos

/// `IClipboardCopyEvent`.
pub struct ClipboardCopyEvent {
    /// `isCut`.
    pub is_cut: bool,
    /// `clipboardData`.
    pub clipboard_data: WritableClipboardData,
    /// `dataToCopy`.
    pub data_to_copy: ClipboardDataToCopy,
    metadata: ClipboardStoredMetadata,
    event: Rc<ClipboardEvent>,
    is_firefox: bool,
    handled: Cell<bool>,
}

impl ClipboardCopyEvent {
    /// `ensureClipboardGetsEditorData()`.
    pub fn ensure_clipboard_gets_editor_data(&self) {
        self.event.prevent_default();
        if let Some(clipboard_data) = &self.event.clipboard_data {
            set_text_data(clipboard_data, &self.data_to_copy.text, self.data_to_copy.html.as_deref(), &self.metadata);
        }
        store_metadata_in_memory(&self.data_to_copy.text, &self.metadata, self.is_firefox);
    }

    /// `setHandled()`.
    pub fn set_handled(&self) {
        self.handled.set(true);
        self.event.prevent_default();
        self.event.stop_immediate_propagation();
    }

    /// `isHandled`.
    pub fn is_handled(&self) -> bool {
        self.handled.get()
    }
}

/// `IClipboardPasteEvent`.
pub struct ClipboardPasteEvent {
    /// `clipboardData`.
    pub clipboard_data: ReadableClipboardData,
    /// `metadata`.
    pub metadata: Option<ClipboardStoredMetadata>,
    /// `text`.
    pub text: JsString,
    /// `browserEvent`.
    pub browser_event: Rc<ClipboardEvent>,
    handled: Cell<bool>,
}

impl ClipboardPasteEvent {
    /// `setHandled()`.
    pub fn set_handled(&self) {
        self.handled.set(true);
        self.browser_event.prevent_default();
        self.browser_event.stop_immediate_propagation();
    }

    /// `isHandled`.
    pub fn is_handled(&self) -> bool {
        self.handled.get()
    }
}

/// `createClipboardCopyEvent(e, isCut, context, logService, isFirefox)`: `view_model` é o `context.viewModel`.
pub fn create_clipboard_copy_event<V: IViewModel>(
    e: Rc<ClipboardEvent>,
    is_cut: bool,
    view_model: &V,
    is_firefox: bool,
) -> ClipboardCopyEvent {
    let (data_to_copy, metadata) = generate_data_to_copy(view_model);
    ClipboardCopyEvent {
        is_cut,
        clipboard_data: create_writable_clipboard_data(e.clipboard_data.clone()),
        data_to_copy,
        metadata,
        event: e,
        is_firefox,
        handled: Cell::new(false),
    }
}

/// `createClipboardPasteEvent(e)`.
pub fn create_clipboard_paste_event(e: Rc<ClipboardEvent>) -> ClipboardPasteEvent {
    let (text, metadata) = match &e.clipboard_data {
        Some(clipboard_data) => get_text_data(clipboard_data),
        None => (JsString::default(), None),
    };
    let metadata = metadata.or_else(|| in_memory_clipboard_metadata_manager().get(&text));
    ClipboardPasteEvent {
        clipboard_data: create_readable_clipboard_data(e.clipboard_data.clone()),
        metadata,
        text,
        browser_event: e,
        handled: Cell::new(false),
    }
}

// #endregion
