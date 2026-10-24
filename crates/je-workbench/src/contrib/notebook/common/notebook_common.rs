//! Porte de `src/vs/workbench/contrib/notebook/common/notebookCommon.ts`: os identificadores dos editores de
//! notebook, os tipos de célula, saída, evento e edição do modelo de notebook, as chaves de contexto, as
//! configurações (`NotebookSetting`), as URIs de célula (`CellUri`), a ordem de exibição de tipos MIME
//! (`MimeTypeDisplayOrder`), o `diff` de listas e a compressão de saídas de fluxo (`compressOutputItemStreams`).
//!
//! Contratos de outros arquivos de que este depende, e onde estão no porte:
//! - `INotebookTextModelLike` (`notebookKernelService.ts`): [`super::notebook_kernel_service`];
//! - `ICellExecutionError` (`notebookExecutionStateService.ts`): [`super::notebook_execution_state_service`];
//! - `ICellRange` (`notebookRange.ts`): [`super::notebook_range`];
//! - `generate`, `parse`, `generateMetadataUri`, `parseMetadataUri`, `extractCellOutputDetails`
//!   (`services/notebook/common/notebookDocumentService.ts`): [`crate::services::notebook::common::notebook_document_service`];
//! - `SnapshotContext` (`services/workingCopy/common/fileWorkingCopy.ts`): [`crate::services::working_copy::file_working_copy`];
//! - `IAccessibilityInformation` (`platform/accessibility/common/accessibility.ts`): `je_platform::accessibility`.
//!
//! Divergências:
//! - **Metadados abertos** (`NotebookDocumentMetadata`, `NotebookCellMetadata`, o `metadata` de uma saída e os
//!   `NullablePartial...`) são o [`Dynamic`], o valor JavaScript sem tipo do porte; `null` é `Dynamic::Null`. Os mapas
//!   de booleanos (`TransientCellMetadata`, `TransientDocumentMetadata`, `CellContentMetadata`) são `HashMap<String, bool>`.
//! - **`NotebookTextModel`** (`model/notebookTextModel.ts`, a classe) ainda não foi portada; o que o original tipa como
//!   `NotebookTextModel` (`INotebookDiffEditorModel`, `IResolvedNotebookEditorModel`) é `Rc<dyn INotebookTextModel>`, e
//!   a classe o implementará. `IResolvedNotebookEditorModel.notebook` (que estreita o `notebook` do pai) é
//!   [`IResolvedNotebookEditorModel::resolved_notebook`], porque um trait não estreita o tipo de retorno de um método.
//! - **`ITextModel` e `IReadonlyTextBuffer`**: o porte do editor não tem as interfaces; o modelo é o `Rc<TextModel>` e o
//!   buffer é o [`PieceTreeTextBuffer`] concreto, compartilhado como [`TextBufferRef`] (`Rc<RefCell<...>>`), porque o
//!   buffer é um valor com `&mut self` nas escritas.
//! - **Uniões discriminadas por `kind`**: cada interface de evento perde o campo `kind` (que só discrimina a união), e a
//!   união é o enum [`NotebookRawContentChange`] (genérico no tipo de célula, o que cobre `NotebookRawContentEventDto` e
//!   `NotebookRawContentEvent`), com [`NotebookRawContentChange::kind`]. O mesmo vale para [`ICellEditOperation`] e
//!   [`IImmediateCellEditOperation`] (a variante `Output` é a edição por índice e `OutputByHandle` a por handle).
//! - **Propriedades de `ICell`**: `handle`, `outputs` e `cellKind` são graváveis na interface do original, mas só leitura
//!   na classe; `onDidChangeOutputs` e `onDidChangeOutputItems` são opcionais no original. No trait as três primeiras
//!   são só leitura e os dois eventos são obrigatórios (quem não os tem devolve `Event::none()`).
//! - **Números**: `handle`, `versionId` e os índices de saída são `i64`; `executionOrder` e os tempos são `f64`.
//! - **`IFileReadLimits`** é o `FileReadLimits` de `services::textfile::textfiles`.
//! - **Namespaces** (`NotebookMetadataUri`, `CellUri`, `NotebookSetting`) são structs sem campos com constantes e funções
//!   associadas; `CellUri.parse` devolve [`ParsedCellUri`] e `CellUri.parseCellOutputUri` devolve [`CellOutputUriDetails`].
//! - **Texto**: o conteúdo de célula (`source`, `getValue`) é `JsString`; identificadores e nomes são `String`.
//! - **`const enum` e `enum` numéricos** viram enums com os mesmos valores; os de texto têm `as_str`.
//! - **`diff`**: o `equal` opcional (padrão `a === b`) é `Option<&dyn Fn(&T, &T) -> bool>` e, ausente, vale o `PartialEq`.
//! - **`compressOutputItemStreams`** recebe `&[&[u8]]` (os `Uint8Array`) e devolve [`CompressedOutputItemStreams`];
//!   `TextDecoder` é o UTF-8 com perda sem BOM, e as regexes de `fixCarriageReturn` com semântica do `$` na string de
//!   substituição usam o [`JsRegExp`] do je-base e o `replace_first` de `ReplacePattern`.
//! - **Fora do porte**: o `import` de `RegisteredEditorPriority` e de `IUntypedEditorInput` usa os tipos do porte
//!   (`RegisteredEditorPriority` e `UntypedEditorInput`).

use std::cell::{LazyCell, RefCell};
use std::collections::{BTreeSet, HashMap, HashSet};
use std::rc::Rc;
use std::sync::LazyLock;

use indexmap::IndexMap;
use je_base::JsString;
use je_base::buffer::VSBuffer;
use je_base::cancellation::CancellationToken;
use je_base::char_code as cc;
use je_base::diff::DiffResult;
use je_base::dynamic::Dynamic;
use je_base::event::Event;
use je_base::glob::{self, GlobOptions, ParsedPattern, PatternItem, RelativePattern};
use je_base::html_content::IMarkdownString;
use je_base::js_regexp::JsRegExp;
use je_base::lifecycle::IDisposable;
use je_base::mime::mimes;
use je_base::network::schemas;
use je_base::platform::Platform;
use je_base::promise::Promise;
use je_base::sequence::Splice;
use je_base::themables::ThemeColor;
use je_base::uri::{URI, UriComponents};
use je_editor::code_action::languages::{Command, WorkspaceEditMetadata};
use je_editor::piece_tree_text_buffer::PieceTreeTextBuffer;
use je_editor::range::Range;
use je_editor::text_model::TextModel;
use je_editor::undo_redo::UndoRedoGroup;
use je_extensions::manifest::ExtensionIdentifier;
use je_platform::accessibility::IAccessibilityInformation;
use je_platform::contextkey::{ContextKeyMeta, RawContextKey};

use super::notebook_execution_state_service::ICellExecutionError;
use super::notebook_kernel_service::INotebookTextModelLike;
use super::notebook_range::ICellRange;
use crate::common::editor::{RevertOptions, SaveOptions, UntypedEditorInput};
use crate::services::editor::editor_resolver_service::RegisteredEditorPriority;
use crate::services::files_configuration::files_configuration_service::Readonly;
use crate::services::notebook::common::notebook_document_service::{
    self as notebook_document_service, CellOutputUriDetails, ParsedCellUri,
};
use crate::services::search::replace::replace_first;
use je_platform::files::files::FileReadLimits;
use crate::services::working_copy::file_working_copy::SnapshotContext;
use crate::services::working_copy::working_copy::WorkingCopySaveEvent;

pub const NOTEBOOK_EDITOR_ID: &str = "workbench.editor.notebook";
pub const NOTEBOOK_DIFF_EDITOR_ID: &str = "workbench.editor.notebookTextDiffEditor";
pub const NOTEBOOK_MULTI_DIFF_EDITOR_ID: &str = "workbench.editor.notebookMultiTextDiffEditor";
pub const INTERACTIVE_WINDOW_EDITOR_ID: &str = "workbench.editor.interactive";
pub const REPL_EDITOR_ID: &str = "workbench.editor.repl";
pub const NOTEBOOK_OUTPUT_EDITOR_ID: &str = "workbench.editor.notebookOutputEditor";

/// `CellKind`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CellKind {
    Markup = 1,
    Code = 2,
}

pub const NOTEBOOK_DISPLAY_ORDER: &[&str] = &[
    "application/json",
    "application/javascript",
    "text/html",
    "image/svg+xml",
    mimes::LATEX,
    mimes::MARKDOWN,
    "image/png",
    "image/jpeg",
    mimes::TEXT,
];

pub const ACCESSIBLE_NOTEBOOK_DISPLAY_ORDER: &[&str] = &[
    mimes::LATEX,
    mimes::MARKDOWN,
    "application/json",
    "text/html",
    "image/svg+xml",
    "image/png",
    "image/jpeg",
    mimes::TEXT,
];

/// A mapping of extension IDs who contain renderers, to notebook ids who they should be treated as the same in the
/// renderer selection logic. This is used to prefer the 1st party Jupyter renderers even though they're in a separate
/// extension, for instance. See #136247.
pub static RENDERER_EQUIVALENT_EXTENSIONS: LazyLock<HashMap<&'static str, HashSet<&'static str>>> = LazyLock::new(|| {
    HashMap::from([
        ("ms-toolsai.jupyter", HashSet::from(["jupyter-notebook", "interactive"])),
        ("ms-toolsai.jupyter-renderers", HashSet::from(["jupyter-notebook", "interactive"])),
    ])
});

pub const RENDERER_NOT_AVAILABLE: &str = "_notAvailable";

/// `ContributedNotebookRendererEntrypoint`: `string | { readonly extends: string; readonly path: string }`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ContributedNotebookRendererEntrypoint {
    Path(String),
    Extends { extends: String, path: String },
}

/// `NotebookDocumentMetadata`: `Record<string, unknown>`.
pub type NotebookDocumentMetadata = Dynamic;

/// `NotebookCellExecutionState`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NotebookCellExecutionState {
    Unconfirmed = 1,
    Pending = 2,
    Executing = 3,
}

/// `NotebookExecutionState`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NotebookExecutionState {
    Unconfirmed = 1,
    Pending = 2,
    Executing = 3,
}

/// `NotebookCellMetadata`: metadados customizados, `[key: string]: unknown`.
pub type NotebookCellMetadata = Dynamic;

/// `NotebookCellInternalMetadata`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct NotebookCellInternalMetadata {
    /// Used only for diffing of Notebooks. This is not persisted and generally useful only when diffing two notebooks.
    /// Useful only after we've manually matched a few cells together so we know which cells are matching.
    pub internal_id: Option<String>,
    pub execution_id: Option<String>,
    pub execution_order: Option<f64>,
    pub last_run_success: Option<bool>,
    pub run_start_time: Option<f64>,
    pub run_start_time_adjustment: Option<f64>,
    pub run_end_time: Option<f64>,
    pub render_duration: Option<IndexMap<String, f64>>,
    pub error: Option<ICellExecutionError>,
}

/// `NotebookCellCollapseState`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct NotebookCellCollapseState {
    pub input_collapsed: Option<bool>,
    pub output_collapsed: Option<bool>,
}

/// `NotebookCellDefaultCollapseConfig`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct NotebookCellDefaultCollapseConfig {
    pub code_cell: Option<NotebookCellCollapseState>,
    pub markup_cell: Option<NotebookCellCollapseState>,
}

/// `InteractiveWindowCollapseCodeCells`: `'always' | 'never' | 'fromEditor'`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InteractiveWindowCollapseCodeCells {
    Always,
    Never,
    FromEditor,
}

impl InteractiveWindowCollapseCodeCells {
    /// O valor de texto do original.
    pub fn as_str(self) -> &'static str {
        match self {
            InteractiveWindowCollapseCodeCells::Always => "always",
            InteractiveWindowCollapseCodeCells::Never => "never",
            InteractiveWindowCollapseCodeCells::FromEditor => "fromEditor",
        }
    }
}

/// `TransientCellMetadata`: `{ readonly [K in keyof NotebookCellMetadata]?: boolean }`.
pub type TransientCellMetadata = HashMap<String, bool>;
/// `CellContentMetadata`.
pub type CellContentMetadata = HashMap<String, bool>;
/// `TransientDocumentMetadata`.
pub type TransientDocumentMetadata = HashMap<String, bool>;

/// `TransientOptions`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TransientOptions {
    pub transient_outputs: bool,
    pub transient_cell_metadata: TransientCellMetadata,
    pub transient_document_metadata: TransientDocumentMetadata,
    pub cell_content_metadata: CellContentMetadata,
}

/// `NotebookRendererMatch`. Note: enum values are used for sorting.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum NotebookRendererMatch {
    /// Renderer has a hard dependency on an available kernel
    WithHardKernelDependency = 0,
    /// Renderer works better with an available kernel
    WithOptionalKernelDependency = 1,
    /// Renderer is kernel-agnostic
    Pure = 2,
    /// Renderer is for a different mimeType or has a hard dependency which is unsatisfied
    Never = 3,
}

/// Renderer messaging requirement. While this allows for 'optional' messaging, VS Code effectively treats it the same
/// as true right now. "Partial activation" of extensions is a very tricky problem, which could allow solving this. But
/// for now, optional is mostly only honored for aznb.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RendererMessagingSpec {
    Always,
    Never,
    Optional,
}

impl RendererMessagingSpec {
    /// O valor de texto do `const enum` do original.
    pub fn as_str(self) -> &'static str {
        match self {
            RendererMessagingSpec::Always => "always",
            RendererMessagingSpec::Never => "never",
            RendererMessagingSpec::Optional => "optional",
        }
    }
}

/// `NotebookRendererEntrypoint`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NotebookRendererEntrypoint {
    pub extends: Option<String>,
    pub path: URI,
}

/// `INotebookRendererInfo`.
pub trait INotebookRendererInfo {
    fn id(&self) -> String;
    fn display_name(&self) -> String;
    fn entrypoint(&self) -> NotebookRendererEntrypoint;
    fn extension_location(&self) -> URI;
    fn extension_id(&self) -> ExtensionIdentifier;
    fn messaging(&self) -> RendererMessagingSpec;

    fn mime_types(&self) -> Vec<String>;

    fn is_builtin(&self) -> bool;

    fn matches_without_kernel(&self, mime_type: &str) -> NotebookRendererMatch;
    fn matches(&self, mime_type: &str, kernel_provides: &[String]) -> NotebookRendererMatch;
}

/// `INotebookStaticPreloadInfo`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct INotebookStaticPreloadInfo {
    /// O campo `type` do original.
    pub type_: String,
    pub entrypoint: URI,
    pub extension_location: URI,
    pub local_resource_roots: Vec<URI>,
}

/// `IOrderedMimeType`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IOrderedMimeType {
    pub mime_type: String,
    pub renderer_id: String,
    pub is_trusted: bool,
}

/// `IOutputItemDto`.
#[derive(Clone, Debug)]
pub struct IOutputItemDto {
    pub mime: String,
    pub data: VSBuffer,
}

/// `IOutputDto`.
#[derive(Clone, Debug)]
pub struct IOutputDto {
    pub outputs: Vec<IOutputItemDto>,
    pub output_id: String,
    /// `Record<string, any>`.
    pub metadata: Option<Dynamic>,
}

/// `ICellOutput`.
pub trait ICellOutput: IDisposable {
    fn version_id(&self) -> i64;
    fn outputs(&self) -> Vec<IOutputItemDto>;
    fn metadata(&self) -> Option<Dynamic>;
    fn output_id(&self) -> String;
    /// Alternative output id that's reused when the output is updated.
    fn alternative_output_id(&self) -> String;
    fn on_did_change_data(&self) -> Event<()>;
    fn replace_data(&self, items: IOutputDto);
    fn append_data(&self, items: Vec<IOutputItemDto>);
    fn appended_since_version(&self, version_id: i64, mime: &str) -> Option<VSBuffer>;
    fn as_dto(&self) -> IOutputDto;
    fn bump_version(&self);
}

/// `CellInternalMetadataChangedEvent`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CellInternalMetadataChangedEvent {
    pub last_run_success_changed: Option<bool>,
}

/// O buffer de texto compartilhado de uma célula (`IReadonlyTextBuffer`/`ITextBuffer`). Ver as divergências.
pub type TextBufferRef = Rc<RefCell<PieceTreeTextBuffer>>;

/// `INotebookDocumentMetadataTextModel`.
pub trait INotebookDocumentMetadataTextModel {
    /// Notebook Metadata Uri.
    fn uri(&self) -> URI;
    /// Triggered when the Notebook Metadata changes.
    fn on_did_change(&self) -> Event<()>;
    fn metadata(&self) -> NotebookDocumentMetadata;
    fn text_buffer(&self) -> TextBufferRef;
    /// Text representation of the Notebook Metadata
    fn get_value(&self) -> String;
    fn get_hash(&self) -> String;
}

/// `ICell`.
pub trait ICell {
    fn uri(&self) -> URI;
    fn handle(&self) -> i64;
    fn language(&self) -> String;
    fn set_language(&self, new_language: &str);
    fn cell_kind(&self) -> CellKind;
    fn outputs(&self) -> Vec<Rc<dyn ICellOutput>>;
    fn metadata(&self) -> NotebookCellMetadata;
    fn set_metadata(&self, new_metadata: NotebookCellMetadata);
    fn internal_metadata(&self) -> NotebookCellInternalMetadata;
    fn set_internal_metadata(&self, new_internal_metadata: NotebookCellInternalMetadata);
    fn get_hash_value(&self) -> i32;
    fn text_buffer(&self) -> TextBufferRef;
    fn text_model(&self) -> Option<Rc<TextModel>>;
    fn set_text_model(&self, text_model: Option<Rc<TextModel>>);
    fn on_did_change_text_model(&self) -> Event<()>;
    fn get_value(&self) -> JsString;
    fn on_did_change_outputs(&self) -> Event<NotebookCellOutputsSplice>;
    fn on_did_change_output_items(&self) -> Event<()>;
    fn on_did_change_language(&self) -> Event<String>;
    fn on_did_change_metadata(&self) -> Event<()>;
    fn on_did_change_internal_metadata(&self) -> Event<CellInternalMetadataChangedEvent>;
}

/// `INotebookSnapshotOptions`.
#[derive(Clone, Debug)]
pub struct INotebookSnapshotOptions {
    pub context: SnapshotContext,
    pub output_size_limit: u64,
    pub transient_options: Option<TransientOptions>,
}

/// `INotebookTextModel`.
pub trait INotebookTextModel: INotebookTextModelLike + IDisposable {
    fn view_type(&self) -> String;
    fn metadata(&self) -> NotebookDocumentMetadata;
    fn set_metadata(&self, metadata: NotebookDocumentMetadata);
    fn transient_options(&self) -> TransientOptions;
    fn version_id(&self) -> i64;
    fn length(&self) -> usize;
    fn cells(&self) -> Vec<Rc<dyn ICell>>;
    fn reset(&self, cells: Vec<ICellDto2>, metadata: NotebookDocumentMetadata, transient_options: TransientOptions);
    fn create_snapshot(&self, options: &INotebookSnapshotOptions) -> NotebookData;
    fn restore_snapshot(&self, snapshot: NotebookData, transient_options: Option<TransientOptions>);
    fn apply_edits(
        &self,
        raw_edits: &[ICellEditOperation],
        synchronous: bool,
        begin_selection_state: Option<ISelectionState>,
        end_selections_computer: &dyn Fn() -> Option<ISelectionState>,
        undo_redo_group: Option<&UndoRedoGroup>,
        compute_undo_redo: Option<bool>,
    ) -> bool;
    fn on_did_change_content(&self) -> Event<NotebookTextModelChangedEvent>;
    fn on_will_dispose(&self) -> Event<()>;
}

/// `NotebookCellTextModelSplice<T>`: `[start, deleteCount, newItems]`.
pub type NotebookCellTextModelSplice<T> = (usize, usize, Vec<T>);

/// `NotebookCellOutputsSplice`.
#[derive(Clone)]
pub struct NotebookCellOutputsSplice {
    /// start
    pub start: usize,
    /// delete count
    pub delete_count: usize,
    pub new_outputs: Vec<Rc<dyn ICellOutput>>,
}

/// `IMainCellDto`.
#[derive(Clone, Debug)]
pub struct IMainCellDto {
    pub handle: i64,
    pub url: String,
    pub source: Vec<String>,
    pub eol: String,
    pub version_id: i64,
    pub language: String,
    pub cell_kind: CellKind,
    pub outputs: Vec<IOutputDto>,
    pub metadata: Option<NotebookCellMetadata>,
    pub internal_metadata: Option<NotebookCellInternalMetadata>,
}

/// `NotebookCellsChangeType`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NotebookCellsChangeType {
    ModelChange = 1,
    Move = 2,
    ChangeCellLanguage = 5,
    Initialize = 6,
    ChangeCellMetadata = 7,
    Output = 8,
    OutputItem = 9,
    ChangeCellContent = 10,
    ChangeDocumentMetadata = 11,
    ChangeCellInternalMetadata = 12,
    ChangeCellMime = 13,
    Unknown = 100,
}

/// `NotebookCellsInitializeEvent<T>`.
#[derive(Clone, Debug)]
pub struct NotebookCellsInitializeEvent<T> {
    pub changes: Vec<NotebookCellTextModelSplice<T>>,
}

/// `NotebookCellContentChangeEvent`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NotebookCellContentChangeEvent {
    pub index: usize,
}

/// `NotebookCellsModelChangedEvent<T>`.
#[derive(Clone, Debug)]
pub struct NotebookCellsModelChangedEvent<T> {
    pub changes: Vec<NotebookCellTextModelSplice<T>>,
}

/// `NotebookCellsModelMoveEvent<T>`.
#[derive(Clone, Debug)]
pub struct NotebookCellsModelMoveEvent<T> {
    pub index: usize,
    pub length: usize,
    pub new_idx: usize,
    pub cells: Vec<T>,
}

/// `NotebookOutputChangedEvent`.
#[derive(Clone, Debug)]
pub struct NotebookOutputChangedEvent {
    pub index: usize,
    pub outputs: Vec<IOutputDto>,
    pub append: bool,
}

/// `NotebookOutputItemChangedEvent`.
#[derive(Clone, Debug)]
pub struct NotebookOutputItemChangedEvent {
    pub index: usize,
    pub output_id: String,
    pub output_items: Vec<IOutputItemDto>,
    pub append: bool,
}

/// `NotebookCellsChangeLanguageEvent`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NotebookCellsChangeLanguageEvent {
    pub index: usize,
    pub language: String,
}

/// `NotebookCellsChangeMimeEvent`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NotebookCellsChangeMimeEvent {
    pub index: usize,
    pub mime: Option<String>,
}

/// `NotebookCellsChangeMetadataEvent`.
#[derive(Clone, Debug)]
pub struct NotebookCellsChangeMetadataEvent {
    pub index: usize,
    pub metadata: NotebookCellMetadata,
}

/// `NotebookCellsChangeInternalMetadataEvent`.
#[derive(Clone, Debug)]
pub struct NotebookCellsChangeInternalMetadataEvent {
    pub index: usize,
    pub internal_metadata: NotebookCellInternalMetadata,
}

/// `NotebookDocumentChangeMetadataEvent`.
#[derive(Clone, Debug)]
pub struct NotebookDocumentChangeMetadataEvent {
    pub metadata: NotebookDocumentMetadata,
}

/// `NotebookDocumentUnknownChangeEvent`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NotebookDocumentUnknownChangeEvent;

/// A união discriminada por `kind` dos eventos de mudança do conteúdo, genérica no tipo de célula
/// (`IMainCellDto` em `NotebookRawContentEventDto` e `Rc<dyn ICell>` em `NotebookRawContentEvent`).
#[derive(Clone, Debug)]
pub enum NotebookRawContentChange<T> {
    Initialize(NotebookCellsInitializeEvent<T>),
    ChangeDocumentMetadata(NotebookDocumentChangeMetadataEvent),
    ChangeCellContent(NotebookCellContentChangeEvent),
    ModelChange(NotebookCellsModelChangedEvent<T>),
    Move(NotebookCellsModelMoveEvent<T>),
    Output(NotebookOutputChangedEvent),
    OutputItem(NotebookOutputItemChangedEvent),
    ChangeCellLanguage(NotebookCellsChangeLanguageEvent),
    ChangeCellMime(NotebookCellsChangeMimeEvent),
    ChangeCellMetadata(NotebookCellsChangeMetadataEvent),
    ChangeCellInternalMetadata(NotebookCellsChangeInternalMetadataEvent),
    Unknown(NotebookDocumentUnknownChangeEvent),
}

impl<T> NotebookRawContentChange<T> {
    /// O campo `kind` do evento no original.
    pub fn kind(&self) -> NotebookCellsChangeType {
        match self {
            NotebookRawContentChange::Initialize(_) => NotebookCellsChangeType::Initialize,
            NotebookRawContentChange::ChangeDocumentMetadata(_) => NotebookCellsChangeType::ChangeDocumentMetadata,
            NotebookRawContentChange::ChangeCellContent(_) => NotebookCellsChangeType::ChangeCellContent,
            NotebookRawContentChange::ModelChange(_) => NotebookCellsChangeType::ModelChange,
            NotebookRawContentChange::Move(_) => NotebookCellsChangeType::Move,
            NotebookRawContentChange::Output(_) => NotebookCellsChangeType::Output,
            NotebookRawContentChange::OutputItem(_) => NotebookCellsChangeType::OutputItem,
            NotebookRawContentChange::ChangeCellLanguage(_) => NotebookCellsChangeType::ChangeCellLanguage,
            NotebookRawContentChange::ChangeCellMime(_) => NotebookCellsChangeType::ChangeCellMime,
            NotebookRawContentChange::ChangeCellMetadata(_) => NotebookCellsChangeType::ChangeCellMetadata,
            NotebookRawContentChange::ChangeCellInternalMetadata(_) => NotebookCellsChangeType::ChangeCellInternalMetadata,
            NotebookRawContentChange::Unknown(_) => NotebookCellsChangeType::Unknown,
        }
    }
}

/// `NotebookRawContentEventDto`.
pub type NotebookRawContentEventDto = NotebookRawContentChange<IMainCellDto>;

/// `NotebookCellsChangedEventDto`.
#[derive(Clone, Debug)]
pub struct NotebookCellsChangedEventDto {
    pub raw_events: Vec<NotebookRawContentEventDto>,
    pub version_id: i64,
}

/// `NotebookRawContentEvent`: o evento com células `ICell` mais a marca `transient`.
#[derive(Clone)]
pub struct NotebookRawContentEvent {
    pub event: NotebookRawContentChange<Rc<dyn ICell>>,
    pub transient: bool,
}

/// `SelectionStateType`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SelectionStateType {
    Handle = 0,
    Index = 1,
}

/// `ISelectionHandleState`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ISelectionHandleState {
    pub primary: Option<i64>,
    pub selections: Vec<i64>,
}

/// `ISelectionIndexState`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ISelectionIndexState {
    pub focus: ICellRange,
    pub selections: Vec<ICellRange>,
}

/// `ISelectionState`: `ISelectionHandleState | ISelectionIndexState`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ISelectionState {
    Handle(ISelectionHandleState),
    Index(ISelectionIndexState),
}

impl ISelectionState {
    /// O campo `kind` do original.
    pub fn kind(&self) -> SelectionStateType {
        match self {
            ISelectionState::Handle(_) => SelectionStateType::Handle,
            ISelectionState::Index(_) => SelectionStateType::Index,
        }
    }
}

/// `NotebookTextModelChangedEvent`.
#[derive(Clone)]
pub struct NotebookTextModelChangedEvent {
    pub raw_events: Vec<NotebookRawContentEvent>,
    pub version_id: i64,
    pub synchronous: Option<bool>,
    pub end_selection_state: Option<ISelectionState>,
}

/// `NotebookTextModelWillAddRemoveEvent`.
#[derive(Clone)]
pub struct NotebookTextModelWillAddRemoveEvent {
    pub raw_event: NotebookCellsModelChangedEvent<Rc<dyn ICell>>,
}

/// `CellEditType`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CellEditType {
    Replace = 1,
    Output = 2,
    Metadata = 3,
    CellLanguage = 4,
    DocumentMetadata = 5,
    Move = 6,
    OutputItems = 7,
    PartialMetadata = 8,
    PartialInternalMetadata = 9,
}

/// `ICellDto2`.
#[derive(Clone, Debug)]
pub struct ICellDto2 {
    pub source: JsString,
    pub language: String,
    pub mime: Option<String>,
    pub cell_kind: CellKind,
    pub outputs: Vec<IOutputDto>,
    pub metadata: Option<NotebookCellMetadata>,
    pub internal_metadata: Option<NotebookCellInternalMetadata>,
    pub collapse_state: Option<NotebookCellCollapseState>,
}

/// `ICellReplaceEdit`.
#[derive(Clone, Debug)]
pub struct ICellReplaceEdit {
    pub index: usize,
    pub count: usize,
    pub cells: Vec<ICellDto2>,
}

/// `ICellOutputEdit`.
#[derive(Clone, Debug)]
pub struct ICellOutputEdit {
    pub index: usize,
    pub outputs: Vec<IOutputDto>,
    pub append: Option<bool>,
}

/// `ICellOutputEditByHandle`.
#[derive(Clone, Debug)]
pub struct ICellOutputEditByHandle {
    pub handle: i64,
    pub outputs: Vec<IOutputDto>,
    pub append: Option<bool>,
}

/// `ICellOutputItemEdit`.
#[derive(Clone, Debug)]
pub struct ICellOutputItemEdit {
    pub output_id: String,
    pub items: Vec<IOutputItemDto>,
    pub append: Option<bool>,
}

/// `ICellMetadataEdit`.
#[derive(Clone, Debug)]
pub struct ICellMetadataEdit {
    pub index: usize,
    pub metadata: NotebookCellMetadata,
}

/// `NullablePartialNotebookCellMetadata`: These types are nullable because we need to use 'null' on the EH side so it
/// is JSON-stringified. É um objeto cujas propriedades ausentes são `undefined` e as limpas são `Dynamic::Null`.
pub type NullablePartialNotebookCellMetadata = Dynamic;

/// `ICellPartialMetadataEdit`.
#[derive(Clone, Debug)]
pub struct ICellPartialMetadataEdit {
    pub index: usize,
    pub metadata: NullablePartialNotebookCellMetadata,
}

/// `ICellPartialMetadataEditByHandle`.
#[derive(Clone, Debug)]
pub struct ICellPartialMetadataEditByHandle {
    pub handle: i64,
    pub metadata: NullablePartialNotebookCellMetadata,
}

/// `NullablePartialNotebookCellInternalMetadata`: cada campo é `None` quando ausente (`undefined`), `Some(None)` quando
/// limpo (`null`) e `Some(Some(valor))` quando definido.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct NullablePartialNotebookCellInternalMetadata {
    pub internal_id: Option<Option<String>>,
    pub execution_id: Option<Option<String>>,
    pub execution_order: Option<Option<f64>>,
    pub last_run_success: Option<Option<bool>>,
    pub run_start_time: Option<Option<f64>>,
    pub run_start_time_adjustment: Option<Option<f64>>,
    pub run_end_time: Option<Option<f64>>,
    pub render_duration: Option<Option<IndexMap<String, f64>>>,
    pub error: Option<Option<ICellExecutionError>>,
}

/// `ICellPartialInternalMetadataEdit`.
#[derive(Clone, Debug)]
pub struct ICellPartialInternalMetadataEdit {
    pub index: usize,
    pub internal_metadata: NullablePartialNotebookCellInternalMetadata,
}

/// `ICellPartialInternalMetadataEditByHandle`.
#[derive(Clone, Debug)]
pub struct ICellPartialInternalMetadataEditByHandle {
    pub handle: i64,
    pub internal_metadata: NullablePartialNotebookCellInternalMetadata,
}

/// `ICellLanguageEdit`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ICellLanguageEdit {
    pub index: usize,
    pub language: String,
}

/// `IDocumentMetadataEdit`.
#[derive(Clone, Debug)]
pub struct IDocumentMetadataEdit {
    pub metadata: NotebookDocumentMetadata,
}

/// `ICellMoveEdit`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ICellMoveEdit {
    pub index: usize,
    pub length: usize,
    pub new_idx: usize,
}

/// `IImmediateCellEditOperation`.
#[derive(Clone, Debug)]
pub enum IImmediateCellEditOperation {
    OutputByHandle(ICellOutputEditByHandle),
    PartialMetadataByHandle(ICellPartialMetadataEditByHandle),
    OutputItems(ICellOutputItemEdit),
    PartialInternalMetadata(ICellPartialInternalMetadataEdit),
    PartialInternalMetadataByHandle(ICellPartialInternalMetadataEditByHandle),
    PartialMetadata(ICellPartialMetadataEdit),
}

/// `ICellEditOperation`.
#[derive(Clone, Debug)]
pub enum ICellEditOperation {
    // IImmediateCellEditOperation
    OutputByHandle(ICellOutputEditByHandle),
    PartialMetadataByHandle(ICellPartialMetadataEditByHandle),
    OutputItems(ICellOutputItemEdit),
    PartialInternalMetadata(ICellPartialInternalMetadataEdit),
    PartialInternalMetadataByHandle(ICellPartialInternalMetadataEditByHandle),
    PartialMetadata(ICellPartialMetadataEdit),
    // O resto da união
    Replace(ICellReplaceEdit),
    Output(ICellOutputEdit),
    Metadata(ICellMetadataEdit),
    DocumentMetadata(IDocumentMetadataEdit),
    Move(ICellMoveEdit),
    CellLanguage(ICellLanguageEdit),
}

impl From<IImmediateCellEditOperation> for ICellEditOperation {
    fn from(operation: IImmediateCellEditOperation) -> Self {
        match operation {
            IImmediateCellEditOperation::OutputByHandle(edit) => ICellEditOperation::OutputByHandle(edit),
            IImmediateCellEditOperation::PartialMetadataByHandle(edit) => ICellEditOperation::PartialMetadataByHandle(edit),
            IImmediateCellEditOperation::OutputItems(edit) => ICellEditOperation::OutputItems(edit),
            IImmediateCellEditOperation::PartialInternalMetadata(edit) => ICellEditOperation::PartialInternalMetadata(edit),
            IImmediateCellEditOperation::PartialInternalMetadataByHandle(edit) => {
                ICellEditOperation::PartialInternalMetadataByHandle(edit)
            }
            IImmediateCellEditOperation::PartialMetadata(edit) => ICellEditOperation::PartialMetadata(edit),
        }
    }
}

impl ICellEditOperation {
    /// O campo `editType` do original.
    pub fn edit_type(&self) -> CellEditType {
        match self {
            ICellEditOperation::OutputByHandle(_) | ICellEditOperation::Output(_) => CellEditType::Output,
            ICellEditOperation::PartialMetadataByHandle(_) | ICellEditOperation::PartialMetadata(_) => {
                CellEditType::PartialMetadata
            }
            ICellEditOperation::OutputItems(_) => CellEditType::OutputItems,
            ICellEditOperation::PartialInternalMetadata(_) | ICellEditOperation::PartialInternalMetadataByHandle(_) => {
                CellEditType::PartialInternalMetadata
            }
            ICellEditOperation::Replace(_) => CellEditType::Replace,
            ICellEditOperation::Metadata(_) => CellEditType::Metadata,
            ICellEditOperation::DocumentMetadata(_) => CellEditType::DocumentMetadata,
            ICellEditOperation::Move(_) => CellEditType::Move,
            ICellEditOperation::CellLanguage(_) => CellEditType::CellLanguage,
        }
    }
}

/// `ICellPartialMetadataEdit | IDocumentMetadataEdit | ICellReplaceEdit`: o `cellEdit` de `IWorkspaceNotebookCellEdit`.
#[derive(Clone, Debug)]
pub enum WorkspaceNotebookCellEditOperation {
    PartialMetadata(ICellPartialMetadataEdit),
    DocumentMetadata(IDocumentMetadataEdit),
    Replace(ICellReplaceEdit),
}

/// `IWorkspaceNotebookCellEdit`.
#[derive(Clone)]
pub struct IWorkspaceNotebookCellEdit {
    pub metadata: Option<WorkspaceEditMetadata>,
    pub resource: URI,
    pub notebook_version_id: Option<i64>,
    pub cell_edit: WorkspaceNotebookCellEditOperation,
}

/// `NotebookData`.
#[derive(Clone, Debug)]
pub struct NotebookData {
    pub cells: Vec<ICellDto2>,
    pub metadata: NotebookDocumentMetadata,
}

/// `string | IRelativePattern | INotebookExclusiveDocumentFilter`: um item de `filenamePattern`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NotebookFilenamePattern {
    Pattern(String),
    Relative(RelativePattern),
    Exclusive(INotebookExclusiveDocumentFilter),
}

/// `INotebookContributionData`.
#[derive(Clone, Debug)]
pub struct INotebookContributionData {
    pub extension: Option<ExtensionIdentifier>,
    pub provider_display_name: String,
    pub display_name: String,
    pub filename_pattern: Vec<NotebookFilenamePattern>,
    pub priority: Option<RegisteredEditorPriority>,
}

/// `NotebookMetadataUri`.
pub struct NotebookMetadataUri;

impl NotebookMetadataUri {
    pub const SCHEME: &'static str = schemas::VSCODE_NOTEBOOK_METADATA;

    pub fn generate(notebook: &URI) -> URI {
        notebook_document_service::generate_metadata_uri(notebook)
    }

    pub fn parse(metadata: &URI) -> Option<URI> {
        notebook_document_service::parse_metadata_uri(metadata)
    }
}

/// `CellUri`.
pub struct CellUri;

impl CellUri {
    pub const SCHEME: &'static str = schemas::VSCODE_NOTEBOOK_CELL;

    pub fn generate(notebook: &URI, handle: i64) -> URI {
        notebook_document_service::generate(notebook, handle)
    }

    pub fn parse(cell: &URI) -> Option<ParsedCellUri> {
        notebook_document_service::parse(cell)
    }

    /// Generates a URI for a cell output in a notebook using the output ID. Used when URI should be opened as text in
    /// the editor.
    pub fn generate_cell_output_uri_with_id(notebook: &URI, output_id: Option<&str>) -> URI {
        let notebook_scheme = if notebook.scheme() != schemas::FILE { notebook.scheme().to_string_lossy() } else { String::new() };
        let mut query = je_base::oauth::UrlSearchParams::new();
        query.append("openIn", "editor");
        query.append("outputId", output_id.unwrap_or(""));
        query.append("notebookScheme", &notebook_scheme);
        notebook.with(&je_base::uri::UriChange {
            scheme: je_base::uri::Field::from(schemas::VSCODE_NOTEBOOK_CELL_OUTPUT),
            query: je_base::uri::Field::from(query.to_query_string().as_str()),
            ..je_base::uri::UriChange::default()
        })
    }

    /// Generates a URI for a cell output in a notebook using the output index. Used when URI should be opened in
    /// notebook editor.
    pub fn generate_cell_output_uri_with_index(notebook: &URI, cell_uri: &URI, output_index: i64) -> URI {
        let mut query = je_base::oauth::UrlSearchParams::new();
        query.append("openIn", "notebook");
        query.append("outputIndex", &output_index.to_string());
        notebook.with(&je_base::uri::UriChange {
            scheme: je_base::uri::Field::from(schemas::VSCODE_NOTEBOOK_CELL_OUTPUT),
            fragment: je_base::uri::Field::from(cell_uri.fragment().clone()),
            query: je_base::uri::Field::from(query.to_query_string().as_str()),
            ..je_base::uri::UriChange::default()
        })
    }

    /// O `cellId` do original não entra na URI (só `cellIndex`, `outputId` e `outputIndex` entram).
    pub fn generate_output_editor_uri(notebook: &URI, _cell_id: &str, cell_index: i64, output_id: &str, output_index: i64) -> URI {
        let mut query = je_base::oauth::UrlSearchParams::new();
        query.append("openIn", "notebookOutputEditor");
        query.append("notebook", &notebook.to_string(false).to_string_lossy());
        query.append("cellIndex", &cell_index.to_string());
        query.append("outputId", output_id);
        query.append("outputIndex", &output_index.to_string());
        notebook.with(&je_base::uri::UriChange {
            scheme: je_base::uri::Field::from(schemas::VSCODE_NOTEBOOK_CELL_OUTPUT),
            query: je_base::uri::Field::from(query.to_query_string().as_str()),
            ..je_base::uri::UriChange::default()
        })
    }

    pub fn parse_cell_output_uri(uri: &URI) -> Option<CellOutputUriDetails> {
        notebook_document_service::extract_cell_output_details(uri)
    }

    pub fn generate_cell_property_uri(notebook: &URI, handle: i64, scheme: &str) -> URI {
        CellUri::generate(notebook, handle).with(&je_base::uri::UriChange {
            scheme: je_base::uri::Field::from(scheme),
            ..je_base::uri::UriChange::default()
        })
    }

    pub fn parse_cell_property_uri(uri: &URI, property_scheme: &str) -> Option<ParsedCellUri> {
        if uri.scheme() != property_scheme {
            return None;
        }

        CellUri::parse(&uri.with(&je_base::uri::UriChange {
            scheme: je_base::uri::Field::from(CellUri::SCHEME),
            ..je_base::uri::UriChange::default()
        }))
    }
}

/// `normalizeSlashes`.
fn normalize_slashes(text: &str) -> String {
    if Platform::detect().is_windows { text.replace('/', "\\") } else { text.to_string() }
}

/// `IMimeTypeWithMatcher`.
#[derive(Clone)]
struct MimeTypeWithMatcher {
    pattern: String,
    matches: ParsedPattern,
}

/// `{ pattern, matches: glob.parse(normalizeSlashes(pattern), { ignoreCase: true }) }`.
fn mime_type_with_matcher(pattern: &str) -> MimeTypeWithMatcher {
    let options = GlobOptions { ignore_case: Some(true), ..GlobOptions::default() };
    let normalized = normalize_slashes(pattern);
    MimeTypeWithMatcher { pattern: pattern.to_string(), matches: glob::parse_pattern(&normalized, &options) }
}

/// `MimeTypeDisplayOrder`.
pub struct MimeTypeDisplayOrder {
    order: Vec<MimeTypeWithMatcher>,
    default_order: &'static [&'static str],
}

impl Default for MimeTypeDisplayOrder {
    /// `new MimeTypeDisplayOrder()`.
    fn default() -> Self {
        Self::new::<&str>(&[], None)
    }
}

impl MimeTypeDisplayOrder {
    /// `new MimeTypeDisplayOrder(initialValue, defaultOrder = NOTEBOOK_DISPLAY_ORDER)`.
    pub fn new<S: AsRef<str>>(initial_value: &[S], default_order: Option<&'static [&'static str]>) -> Self {
        // `[...new Set(initialValue)]`
        let mut seen: HashSet<&str> = HashSet::new();
        let order = initial_value
            .iter()
            .map(AsRef::as_ref)
            .filter(|pattern| seen.insert(*pattern))
            .map(mime_type_with_matcher)
            .collect();
        Self { order, default_order: default_order.unwrap_or(NOTEBOOK_DISPLAY_ORDER) }
    }

    /// Returns a sorted array of the input mimeTypes.
    pub fn sort<S: AsRef<str>>(&self, mime_types: impl IntoIterator<Item = S>) -> Vec<String> {
        let mut remaining: IndexMap<String, String> = IndexMap::new();
        for mime_type in mime_types {
            let mime_type = mime_type.as_ref();
            remaining.insert(mime_type.to_string(), normalize_slashes(mime_type));
        }
        let mut sorted: Vec<String> = Vec::new();

        for entry in &self.order {
            let found = remaining
                .iter()
                .find(|(_, normalized)| entry.matches.matches(normalized, None))
                .map(|(original, _)| original.clone());
            if let Some(original) = found {
                remaining.shift_remove(&original);
                sorted.push(original);
            }
        }

        if !remaining.is_empty() {
            let mut rest: Vec<String> = remaining.into_keys().collect();
            // `indexOf` dá -1 para o que não está na ordem padrão; o `sort` é estável.
            rest.sort_by_key(|mime| {
                self.default_order.iter().position(|default| *default == mime.as_str()).map_or(-1, |index| index as i64)
            });
            sorted.extend(rest);
        }

        sorted
    }

    /// Records that the user selected the given mimetype over the other possible mimeTypes, prioritizing it for future
    /// reference.
    pub fn prioritize<S: AsRef<str>>(&mut self, chosen_mimetype: &str, other_mime_types: &[S]) {
        let Some(chosen_index) = self.find_index(chosen_mimetype, None) else {
            // always first, nothing more to do
            self.order.insert(0, mime_type_with_matcher(chosen_mimetype));
            return;
        };

        // Get the other mimeTypes that are before the chosenMimetype. Then, move them after it, retaining order.
        let other_indices: BTreeSet<usize> =
            other_mime_types.iter().filter_map(|mime_type| self.find_index(mime_type.as_ref(), Some(chosen_index))).collect();
        let moved: Vec<MimeTypeWithMatcher> = other_indices.iter().map(|&index| self.order[index].clone()).collect();
        self.order.splice(chosen_index + 1..chosen_index + 1, moved).for_each(drop);

        for &index in other_indices.iter().rev() {
            self.order.remove(index);
        }
    }

    /// Gets an array of in-order mimetype preferences.
    pub fn to_array(&self) -> Vec<String> {
        self.order.iter().map(|entry| entry.pattern.clone()).collect()
    }

    fn find_index(&self, mime_type: &str, max_index: Option<usize>) -> Option<usize> {
        let normalized = normalize_slashes(mime_type);
        let max_index = max_index.unwrap_or(self.order.len());
        (0..max_index).find(|&index| self.order[index].matches.matches(&normalized, None))
    }
}

/// `diff`: as emendas que levam `before` a `after`. `contains` diz se um elemento de `after` já existe em `before`; o
/// `equal` ausente é `a === b`.
pub fn diff<T: Clone + PartialEq>(
    before: &[T],
    after: &[T],
    contains: impl Fn(&T) -> bool,
    equal: Option<&dyn Fn(&T, &T) -> bool>,
) -> Vec<Splice<T>> {
    fn push_splice<T>(result: &mut Vec<Splice<T>>, start: usize, delete_count: usize, to_insert: Vec<T>) {
        if delete_count == 0 && to_insert.is_empty() {
            return;
        }

        match result.last_mut() {
            Some(latest) if latest.start + latest.delete_count == start => {
                latest.delete_count += delete_count;
                latest.to_insert.extend(to_insert);
            }
            _ => result.push(Splice { start, delete_count, to_insert }),
        }
    }

    let mut result: Vec<Splice<T>> = Vec::new();
    let mut before_idx = 0;
    let mut after_idx = 0;

    loop {
        if before_idx == before.len() {
            push_splice(&mut result, before_idx, 0, after[after_idx..].to_vec());
            break;
        }

        if after_idx == after.len() {
            push_splice(&mut result, before_idx, before.len() - before_idx, Vec::new());
            break;
        }

        let before_element = &before[before_idx];
        let after_element = &after[after_idx];

        let is_equal = match equal {
            Some(equal) => equal(before_element, after_element),
            None => before_element == after_element,
        };
        if is_equal {
            // equal
            before_idx += 1;
            after_idx += 1;
            continue;
        }

        if contains(after_element) {
            // `afterElement` exists before, which means some elements before `afterElement` are deleted
            push_splice(&mut result, before_idx, 1, Vec::new());
            before_idx += 1;
        } else {
            // `afterElement` added
            push_splice(&mut result, before_idx, 0, vec![after_element.clone()]);
            after_idx += 1;
        }
    }

    result
}

thread_local! {
    static NOTEBOOK_EDITOR_CURSOR_BOUNDARY: RawContextKey =
        RawContextKey::new("notebookEditorCursorAtBoundary", "none", ContextKeyMeta::None);
    static NOTEBOOK_EDITOR_CURSOR_LINE_BOUNDARY: RawContextKey =
        RawContextKey::new("notebookEditorCursorAtLineBoundary", "none", ContextKeyMeta::None);
}

/// `NOTEBOOK_EDITOR_CURSOR_BOUNDARY`: `'none' | 'top' | 'bottom' | 'both'`.
pub fn notebook_editor_cursor_boundary() -> RawContextKey {
    NOTEBOOK_EDITOR_CURSOR_BOUNDARY.with(Clone::clone)
}

/// `NOTEBOOK_EDITOR_CURSOR_LINE_BOUNDARY`: `'none' | 'start' | 'end' | 'both'`.
pub fn notebook_editor_cursor_line_boundary() -> RawContextKey {
    NOTEBOOK_EDITOR_CURSOR_LINE_BOUNDARY.with(Clone::clone)
}

/// `INotebookLoadOptions`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct INotebookLoadOptions {
    /// Go to disk bypassing any cache of the model if any.
    pub force_read_from_file: Option<bool>,
    /// If provided, the size of the file will be checked against the limits and an error will be thrown if any limit is
    /// exceeded.
    pub limits: Option<FileReadLimits>,
}

/// `NotebookEditorModelCreationOptions`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct NotebookEditorModelCreationOptions {
    pub limits: Option<FileReadLimits>,
    pub scratchpad: Option<bool>,
    pub view_type: Option<String>,
}

/// `INotebookEditorModel`.
pub trait INotebookEditorModel: IDisposable {
    fn on_did_change_dirty(&self) -> Event<()>;
    fn on_did_save(&self) -> Event<WorkingCopySaveEvent>;
    fn on_did_change_orphaned(&self) -> Event<()>;
    fn on_did_change_readonly(&self) -> Event<()>;
    fn on_did_revert_untitled(&self) -> Event<()>;
    fn resource(&self) -> URI;
    fn view_type(&self) -> String;
    fn notebook(&self) -> Option<Rc<dyn INotebookTextModel>>;
    fn has_error_state(&self) -> bool;
    fn is_resolved(&self) -> bool;
    fn is_dirty(&self) -> bool;
    fn is_modified(&self) -> bool;
    fn is_readonly(&self) -> Readonly;
    fn is_orphaned(&self) -> bool;
    fn has_associated_file_path(&self) -> bool;
    fn load(&self, options: Option<INotebookLoadOptions>) -> Promise<Rc<dyn IResolvedNotebookEditorModel>>;
    fn save(&self, options: Option<SaveOptions>) -> Promise<bool>;
    fn save_as(&self, target: &URI) -> Promise<Option<UntypedEditorInput>>;
    fn revert(&self, options: Option<RevertOptions>) -> Promise<()>;
}

/// `IResolvedNotebookEditorModel`.
pub trait IResolvedNotebookEditorModel: INotebookEditorModel {
    /// O `notebook: NotebookTextModel` do original, que estreita o `notebook` do pai.
    fn resolved_notebook(&self) -> Rc<dyn INotebookTextModel>;
}

/// Um dos lados (`original` ou `modified`) de `INotebookDiffEditorModel`.
#[derive(Clone)]
pub struct NotebookDiffEditorModelSide {
    pub notebook: Rc<dyn INotebookTextModel>,
    pub resource: URI,
    pub view_type: String,
}

/// `INotebookDiffEditorModel`.
pub trait INotebookDiffEditorModel: IDisposable {
    fn original(&self) -> NotebookDiffEditorModelSide;
    fn modified(&self) -> NotebookDiffEditorModelSide;
}

/// `NotebookEditorPriority`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NotebookEditorPriority {
    Default,
    Option,
}

impl NotebookEditorPriority {
    /// O valor de texto do `enum` do original.
    pub fn as_str(self) -> &'static str {
        match self {
            NotebookEditorPriority::Default => "default",
            NotebookEditorPriority::Option => "option",
        }
    }
}

/// `INotebookFindOptions`.
#[derive(Clone, Debug, Default)]
pub struct INotebookFindOptions {
    pub regex: Option<bool>,
    pub whole_word: Option<bool>,
    pub case_sensitive: Option<bool>,
    pub word_separators: Option<String>,
    pub include_markup_input: Option<bool>,
    pub include_markup_preview: Option<bool>,
    pub include_code_input: Option<bool>,
    pub include_output: Option<bool>,
    pub find_scope: Option<INotebookFindScope>,
}

/// `INotebookFindScope`.
#[derive(Clone, Debug)]
pub struct INotebookFindScope {
    pub find_scope_type: NotebookFindScopeType,
    pub selected_cell_ranges: Option<Vec<ICellRange>>,
    pub selected_text_ranges: Option<Vec<Range>>,
}

/// `NotebookFindScopeType`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NotebookFindScopeType {
    Cells,
    Text,
    None,
}

impl NotebookFindScopeType {
    /// O valor de texto do `enum` do original.
    pub fn as_str(self) -> &'static str {
        match self {
            NotebookFindScopeType::Cells => "cells",
            NotebookFindScopeType::Text => "text",
            NotebookFindScopeType::None => "none",
        }
    }
}

/// `INotebookExclusiveDocumentFilter`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct INotebookExclusiveDocumentFilter {
    pub include: Option<PatternItem>,
    pub exclude: Option<PatternItem>,
}

/// `string | string[]`: o `viewType` de `INotebookDocumentFilter`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NotebookViewTypeFilter {
    One(String),
    Many(Vec<String>),
}

/// `INotebookDocumentFilter`.
#[derive(Clone, Debug, Default)]
pub struct INotebookDocumentFilter {
    pub view_type: Option<NotebookViewTypeFilter>,
    pub filename_pattern: Option<NotebookFilenamePattern>,
}

//TODO@rebornix test

/// `isDocumentExcludePattern`: o padrão é um `INotebookExclusiveDocumentFilter` com `include` e `exclude`. Um texto ou
/// um `IRelativePattern` não tem `include` nem `exclude`, então nunca é.
pub fn is_document_exclude_pattern(filename_pattern: &NotebookFilenamePattern) -> bool {
    match filename_pattern {
        NotebookFilenamePattern::Exclusive(arg) => arg.include.is_some() && arg.exclude.is_some(),
        NotebookFilenamePattern::Pattern(_) | NotebookFilenamePattern::Relative(_) => false,
    }
}

/// `INotebookCellStatusBarItemProvider`.
pub trait INotebookCellStatusBarItemProvider {
    fn view_type(&self) -> String;
    fn on_did_change_status_bar_items(&self) -> Option<Event<()>>;
    fn provide_cell_status_bar_items(
        &self,
        uri: &URI,
        index: usize,
        token: &CancellationToken,
    ) -> Promise<Option<INotebookCellStatusBarItemList>>;
}

/// `INotebookDiffResult`.
#[derive(Clone, Debug)]
pub struct INotebookDiffResult {
    pub cells_diff: DiffResult,
    pub metadata_changed: bool,
}

/// `string | ThemeColor`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StringOrThemeColor {
    String(String),
    ThemeColor(ThemeColor),
}

/// `string | IMarkdownString`.
#[derive(Clone, Debug)]
pub enum StringOrMarkdown {
    String(String),
    Markdown(IMarkdownString),
}

/// `string | Command`.
#[derive(Clone, Debug)]
pub enum StringOrCommand {
    String(String),
    Command(Command),
}

/// `INotebookCellStatusBarItem`.
#[derive(Clone, Debug)]
pub struct INotebookCellStatusBarItem {
    pub alignment: CellStatusbarAlignment,
    pub priority: Option<f64>,
    pub text: String,
    pub color: Option<StringOrThemeColor>,
    pub background_color: Option<StringOrThemeColor>,
    pub tooltip: Option<StringOrMarkdown>,
    pub command: Option<StringOrCommand>,
    pub accessibility_information: Option<IAccessibilityInformation>,
    pub opacity: Option<String>,
    pub only_show_when_active: Option<bool>,
}

/// `INotebookCellStatusBarItemList`: o `dispose` opcional é um fecho.
#[derive(Clone)]
pub struct INotebookCellStatusBarItemList {
    pub items: Vec<INotebookCellStatusBarItem>,
    pub dispose: Option<Rc<dyn Fn()>>,
}

/// `ShowCellStatusBarType`: `'hidden' | 'visible' | 'visibleAfterExecute'`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShowCellStatusBarType {
    Hidden,
    Visible,
    VisibleAfterExecute,
}

impl ShowCellStatusBarType {
    /// O valor de texto do original.
    pub fn as_str(self) -> &'static str {
        match self {
            ShowCellStatusBarType::Hidden => "hidden",
            ShowCellStatusBarType::Visible => "visible",
            ShowCellStatusBarType::VisibleAfterExecute => "visibleAfterExecute",
        }
    }
}

/// `NotebookSetting`.
pub struct NotebookSetting;

impl NotebookSetting {
    pub const DISPLAY_ORDER: &'static str = "notebook.displayOrder";
    pub const CELL_TOOLBAR_LOCATION: &'static str = "notebook.cellToolbarLocation";
    pub const CELL_TOOLBAR_VISIBILITY: &'static str = "notebook.cellToolbarVisibility";
    pub const SHOW_CELL_STATUS_BAR: &'static str = "notebook.showCellStatusBar";
    pub const CELL_EXECUTION_TIME_VERBOSITY: &'static str = "notebook.cellExecutionTimeVerbosity";
    pub const TEXT_DIFF_EDITOR_PREVIEW: &'static str = "notebook.diff.enablePreview";
    pub const DIFF_OVERVIEW_RULER: &'static str = "notebook.diff.overviewRuler";
    pub const EXPERIMENTAL_INSERT_TOOLBAR_ALIGNMENT: &'static str = "notebook.experimental.insertToolbarAlignment";
    pub const COMPACT_VIEW: &'static str = "notebook.compactView";
    pub const FOCUS_INDICATOR: &'static str = "notebook.cellFocusIndicator";
    pub const INSERT_TOOLBAR_LOCATION: &'static str = "notebook.insertToolbarLocation";
    pub const GLOBAL_TOOLBAR: &'static str = "notebook.globalToolbar";
    pub const STICKY_SCROLL_ENABLED: &'static str = "notebook.stickyScroll.enabled";
    pub const STICKY_SCROLL_MODE: &'static str = "notebook.stickyScroll.mode";
    pub const UNDO_REDO_PER_CELL: &'static str = "notebook.undoRedoPerCell";
    pub const CONSOLIDATED_OUTPUT_BUTTON: &'static str = "notebook.consolidatedOutputButton";
    pub const SHOW_FOLDING_CONTROLS: &'static str = "notebook.showFoldingControls";
    pub const DRAG_AND_DROP_ENABLED: &'static str = "notebook.dragAndDropEnabled";
    pub const CELL_EDITOR_OPTIONS_CUSTOMIZATIONS: &'static str = "notebook.editorOptionsCustomizations";
    pub const CONSOLIDATED_RUN_BUTTON: &'static str = "notebook.consolidatedRunButton";
    pub const OPEN_GETTING_STARTED: &'static str = "notebook.experimental.openGettingStarted";
    pub const GLOBAL_TOOLBAR_SHOW_LABEL: &'static str = "notebook.globalToolbarShowLabel";
    pub const MARKUP_FONT_SIZE: &'static str = "notebook.markup.fontSize";
    pub const MARKDOWN_LINE_HEIGHT: &'static str = "notebook.markdown.lineHeight";
    pub const INTERACTIVE_WINDOW_COLLAPSE_CODE_CELLS: &'static str = "interactiveWindow.collapseCellInputCode";
    pub const OUTPUT_SCROLLING: &'static str = "notebook.output.scrolling";
    pub const TEXT_OUTPUT_LINE_LIMIT: &'static str = "notebook.output.textLineLimit";
    pub const LINKIFY_OUTPUT_FILE_PATHS: &'static str = "notebook.output.linkifyFilePaths";
    pub const MINIMAL_ERROR_RENDERING: &'static str = "notebook.output.minimalErrorRendering";
    pub const FORMAT_ON_SAVE: &'static str = "notebook.formatOnSave.enabled";
    pub const INSERT_FINAL_NEWLINE: &'static str = "notebook.insertFinalNewline";
    pub const DEFAULT_FORMATTER: &'static str = "notebook.defaultFormatter";
    pub const FORMAT_ON_CELL_EXECUTION: &'static str = "notebook.formatOnCellExecution";
    pub const CODE_ACTIONS_ON_SAVE: &'static str = "notebook.codeActionsOnSave";
    pub const OUTPUT_WORD_WRAP: &'static str = "notebook.output.wordWrap";
    pub const OUTPUT_LINE_HEIGHT: &'static str = "notebook.output.lineHeight";
    pub const OUTPUT_FONT_SIZE: &'static str = "notebook.output.fontSize";
    pub const OUTPUT_FONT_FAMILY: &'static str = "notebook.output.fontFamily";
    pub const FIND_FILTERS: &'static str = "notebook.find.filters";
    pub const CONFIRM_DELETE_RUNNING_CELL: &'static str = "notebook.confirmDeleteRunningCell";
    pub const REMOTE_SAVING: &'static str = "notebook.experimental.remoteSave";
    pub const GOTO_SYMBOLS_ALL_SYMBOLS: &'static str = "notebook.gotoSymbols.showAllSymbols";
    pub const OUTLINE_SHOW_MARKDOWN_HEADERS_ONLY: &'static str = "notebook.outline.showMarkdownHeadersOnly";
    pub const OUTLINE_SHOW_CODE_CELLS: &'static str = "notebook.outline.showCodeCells";
    pub const OUTLINE_SHOW_CODE_CELL_SYMBOLS: &'static str = "notebook.outline.showCodeCellSymbols";
    pub const BREADCRUMBS_SHOW_CODE_CELLS: &'static str = "notebook.breadcrumbs.showCodeCells";
    pub const SCROLL_TO_REVEAL_CELL: &'static str = "notebook.scrolling.revealNextCellOnExecute";
    pub const CELL_CHAT: &'static str = "notebook.experimental.cellChat";
    pub const CELL_GENERATE: &'static str = "notebook.experimental.generate";
    pub const NOTEBOOK_VARIABLES_VIEW: &'static str = "notebook.variablesView";
    pub const NOTEBOOK_INLINE_VALUES: &'static str = "notebook.inlineValues";
    pub const INTERACTIVE_WINDOW_PROMPT_TO_SAVE: &'static str = "interactiveWindow.promptToSaveOnClose";
    pub const CELL_FAILURE_DIAGNOSTICS: &'static str = "notebook.cellFailureDiagnostics";
    pub const OUTPUT_BACKUP_SIZE_LIMIT: &'static str = "notebook.backup.sizeLimit";
    pub const MULTI_CURSOR: &'static str = "notebook.multiCursor.enabled";
    pub const MARKUP_FONT_FAMILY: &'static str = "notebook.markup.fontFamily";
}

/// `CellStatusbarAlignment`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CellStatusbarAlignment {
    Left = 1,
    Right = 2,
}

/// O resultado de [`NotebookWorkingCopyTypeIdentifier::parse`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NotebookWorkingCopyType {
    pub notebook_type: String,
    pub view_type: String,
}

/// `NotebookWorkingCopyTypeIdentifier`.
pub struct NotebookWorkingCopyTypeIdentifier;

impl NotebookWorkingCopyTypeIdentifier {
    const PREFIX: &'static str = "notebook/";

    pub fn create(notebook_type: &str, view_type: Option<&str>) -> String {
        format!("{}{}/{}", Self::PREFIX, notebook_type, view_type.unwrap_or(notebook_type))
    }

    pub fn parse(candidate: &str) -> Option<NotebookWorkingCopyType> {
        let rest = candidate.strip_prefix(Self::PREFIX)?;
        let split: Vec<&str> = rest.split('/').collect();
        if let [notebook_type, view_type] = split[..] {
            return Some(NotebookWorkingCopyType { notebook_type: notebook_type.to_string(), view_type: view_type.to_string() });
        }
        None
    }
}

/// `NotebookExtensionDescription`.
#[derive(Clone, Debug)]
pub struct NotebookExtensionDescription {
    pub id: ExtensionIdentifier,
    pub location: Option<UriComponents>,
}

/// O resultado de [`compress_output_item_streams`]: o buffer único com os dados concatenados e comprimidos, e se
/// houve alguma compressão.
pub struct CompressedOutputItemStreams {
    pub data: VSBuffer,
    pub did_compression: bool,
}

/// Given a stream of individual stdout outputs, this function will return the compressed lines, escaping some of the
/// common terminal escape codes. E.g. some terminal escape codes would result in the previous line getting cleared,
/// such if we had 3 lines and last line contained such a code, then the result string would be just the first two
/// lines.
pub fn compress_output_item_streams(outputs: &[&[u8]]) -> CompressedOutputItemStreams {
    let mut buffers: Vec<&[u8]> = Vec::new();
    let mut start_appending = false;

    // Pick the first set of outputs with the same mime type.
    for output in outputs {
        if buffers.is_empty() || start_appending {
            buffers.push(output);
            start_appending = true;
        }
    }

    let mut did_compression = compress_stream_buffer(&mut buffers);
    let concatenated = VSBuffer::wrap(buffers.concat());
    let data = format_stream_text(&concatenated);
    did_compression = did_compression || data.byte_length() != concatenated.byte_length();
    CompressedOutputItemStreams { data, did_compression }
}

/// `String.fromCharCode(27) + '[A'`.
pub const MOVE_CURSOR_1_LINE_COMMAND: &str = "\u{1b}[A";
const LINE_FEED: u8 = 10;

fn compress_stream_buffer<'a>(streams: &mut [&'a [u8]]) -> bool {
    let command = MOVE_CURSOR_1_LINE_COMMAND.as_bytes();
    let mut did_compress = false;
    for index in 0..streams.len() {
        let stream: &'a [u8] = streams[index];
        if index == 0 || stream.len() < command.len() {
            continue;
        }

        let previous_stream: &'a [u8] = streams[index - 1];

        // Remove the previous line if required.
        if stream.starts_with(command) {
            let Some(last_index_of_line_feed) = previous_stream.iter().rposition(|&byte| byte == LINE_FEED) else {
                continue;
            };

            did_compress = true;
            streams[index - 1] = &previous_stream[..last_index_of_line_feed];
            streams[index] = &stream[command.len()..];
        }
    }
    did_compress
}

/// Took this from jupyter/notebook
/// https://github.com/jupyter/notebook/blob/b8b66332e2023e83d2ee04f83d8814f567e01a4e/notebook/static/base/js/utils.js
/// Remove characters that are overridden by backspace characters
fn fix_backspace(mut txt: Vec<u16>) -> Vec<u16> {
    loop {
        // Cancel out anything-but-newline followed by backspace: `txt.replace(/[^\n]\x08/gm, '')`
        let mut tmp: Vec<u16> = Vec::with_capacity(txt.len());
        let mut i = 0;
        while i < txt.len() {
            if txt[i] != cc::LineFeed && txt.get(i + 1) == Some(&cc::Backspace) {
                i += 2;
            } else {
                tmp.push(txt[i]);
                i += 1;
            }
        }
        if tmp.len() < txt.len() {
            txt = tmp;
        } else {
            return txt;
        }
    }
}

/// As cinco regexes do ramo legado de `fixCarriageReturn` (texto com `$` em linha sobrescrita).
struct OverwritePatterns {
    /// `/\r[^$]/g`, usada só com `search`.
    next_overwrite: JsRegExp,
    /// `/^(.*)\r+/m`
    base: JsRegExp,
    /// `/\r+(.*)$/m`
    insert: JsRegExp,
    /// `/\r+.*$/m`
    drop_overwritten: JsRegExp,
    /// `/^.*\r/m`
    take_prefix: JsRegExp,
}

impl OverwritePatterns {
    fn new() -> Self {
        let compile = |pattern: &str, flags: &str| JsRegExp::from_str(pattern, flags).expect("regex válida");
        Self {
            next_overwrite: compile("\\r[^$]", ""),
            base: compile("^(.*)\\r+", "m"),
            insert: compile("\\r+(.*)$", "m"),
            drop_overwritten: compile("\\r+.*$", "m"),
            take_prefix: compile("^.*\\r", "m"),
        }
    }
}

thread_local! {
    static OVERWRITE_PATTERNS: LazyCell<OverwritePatterns> = LazyCell::new(OverwritePatterns::new);
}

/// O grupo 1 do primeiro casamento (`text.match(regex)![1]`).
fn first_group(regex: &JsRegExp, text: &[u16]) -> Vec<u16> {
    let found = regex.exec(text).expect("há um casamento enquanto houver um retorno de carro");
    found.captures.get(1).and_then(Option::as_ref).map(|range| text[range.clone()].to_vec()).unwrap_or_default()
}

/// O ramo legado: preserva o comportamento da string de substituição com `$` nas linhas sobrescritas.
fn fix_carriage_return_with_dollar(mut txt: Vec<u16>) -> Vec<u16> {
    OVERWRITE_PATTERNS.with(|patterns| {
        while patterns.next_overwrite.exec(&txt).is_some() {
            let base = first_group(&patterns.base, &txt);
            let mut insert = first_group(&patterns.insert, &txt);
            // `insert + base.slice(insert.length, base.length)`
            insert.extend_from_slice(&base[insert.len().min(base.len())..]);
            txt = replace_first(&txt, &patterns.drop_overwritten, &[cc::CarriageReturn]).units().to_vec();
            txt = replace_first(&txt, &patterns.take_prefix, &insert).units().to_vec();
        }
        txt
    })
}

fn is_line_terminator(unit: u16) -> bool {
    unit == cc::LineFeed || unit == cc::LINE_SEPARATOR || unit == cc::PARAGRAPH_SEPARATOR
}

/// `txt.replace(/\r+\n/gm, '\n')`.
fn collapse_carriage_returns_before_newline(txt: &[u16]) -> Vec<u16> {
    let mut out: Vec<u16> = Vec::with_capacity(txt.len());
    let mut i = 0;
    while i < txt.len() {
        if txt[i] == cc::CarriageReturn {
            let mut j = i;
            while j < txt.len() && txt[j] == cc::CarriageReturn {
                j += 1;
            }
            if j < txt.len() && txt[j] == cc::LineFeed {
                out.push(cc::LineFeed);
                i = j + 1;
            } else {
                out.extend_from_slice(&txt[i..j]);
                i = j;
            }
        } else {
            out.push(txt[i]);
            i += 1;
        }
    }
    out
}

/// `line.split(/\r+/)`.
fn split_on_carriage_return_runs(line: &[u16]) -> Vec<&[u16]> {
    let mut segments: Vec<&[u16]> = Vec::new();
    let mut start = 0;
    let mut i = 0;
    while i < line.len() {
        if line[i] == cc::CarriageReturn {
            segments.push(&line[start..i]);
            while i < line.len() && line[i] == cc::CarriageReturn {
                i += 1;
            }
            start = i;
        } else {
            i += 1;
        }
    }
    segments.push(&line[start..]);
    segments
}

/// O fecho do `replace(/[^\n  ]+/g, line => ...)` de `fixCarriageReturn`.
fn overwrite_line(line: &[u16], out: &mut Vec<u16>) {
    if !line.contains(&cc::CarriageReturn) {
        out.extend_from_slice(line);
        return;
    }
    let segments = split_on_carriage_return_runs(line);
    let mut fragments: Vec<&[u16]> = Vec::new();
    let mut length = 0;
    // Later segments overwrite prefixes; keep only the uncovered suffix of each earlier segment.
    for segment in segments.iter().rev() {
        if segment.len() > length {
            fragments.push(&segment[length..]);
            length = segment.len();
        }
    }
    for fragment in fragments {
        out.extend_from_slice(fragment);
    }
}

/// Remove chunks that should be overridden by the effect of carriage return characters.
/// From https://github.com/jupyter/notebook/blob/master/notebook/static/base/js/utils.js
fn fix_carriage_return(txt: &[u16]) -> Vec<u16> {
    let txt = collapse_carriage_returns_before_newline(txt); // \r followed by \n --> newline
    if !txt.contains(&cc::CarriageReturn) {
        return txt;
    }
    // Preserve the legacy replacement-string behavior of dollar text on overwritten lines.
    if txt.contains(&cc::DollarSign)
        && txt
            .split(|&unit| is_line_terminator(unit))
            .any(|line| line.contains(&cc::CarriageReturn) && line.contains(&cc::DollarSign))
    {
        return fix_carriage_return_with_dollar(txt);
    }

    let pending_carriage_return: &[u16] =
        if txt.last() == Some(&cc::CarriageReturn) && !txt.ends_with(&[cc::CarriageReturn, cc::CarriageReturn]) {
            &[cc::CarriageReturn]
        } else {
            &[]
        };
    let body = &txt[..txt.len() - pending_carriage_return.len()];
    let mut out: Vec<u16> = Vec::with_capacity(txt.len());
    let mut i = 0;
    while i < body.len() {
        if is_line_terminator(body[i]) {
            out.push(body[i]);
            i += 1;
            continue;
        }
        let start = i;
        while i < body.len() && !is_line_terminator(body[i]) {
            i += 1;
        }
        overwrite_line(&body[start..i], &mut out);
    }
    out.extend_from_slice(pending_carriage_return);
    out
}

/// `new TextDecoder().decode(bytes)`: UTF-8 com U+FFFD para o inválido e sem `ignoreBOM`, então descarta um BOM no
/// começo.
fn text_decoder_decode(bytes: &[u8]) -> Vec<u16> {
    let bytes = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(bytes);
    String::from_utf8_lossy(bytes).encode_utf16().collect()
}

fn format_stream_text(buffer: &VSBuffer) -> VSBuffer {
    // We have special handling for backspace and carriage return characters. Don't unnecessary decode the bytes if we
    // don't need to perform any processing.
    let needs_processing = buffer.with_bytes(|bytes| bytes.contains(&(cc::Backspace as u8)) || bytes.contains(&(cc::CarriageReturn as u8)));
    if !needs_processing {
        return buffer.clone();
    }
    // Do the same thing jupyter is doing
    let decoded = buffer.with_bytes(text_decoder_decode);
    let fixed = fix_carriage_return(&fix_backspace(decoded));
    VSBuffer::from_string(&String::from_utf16_lossy(&fixed))
}

/// `UriComponents | string`: o `documentation` de `INotebookKernelSourceAction`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UriComponentsOrString {
    UriComponents(UriComponents),
    String(String),
}

/// `INotebookKernelSourceAction`.
#[derive(Clone, Debug)]
pub struct INotebookKernelSourceAction {
    pub label: String,
    pub description: Option<String>,
    pub detail: Option<String>,
    pub command: Option<StringOrCommand>,
    pub documentation: Option<UriComponentsOrString>,
}
