//! Porte parcial de `src/vs/workbench/contrib/notebook/browser/diff/notebookDiffEditorBrowser.ts`: o que a lógica do diff de
//! notebooks lê dele, que é `DiffSide`, o layout de um elemento de diff (`IDiffElementLayoutInfo`,
//! `CellDiffViewModelLayoutChangeEvent`), a margem, as chaves de contexto e o contrato do view model do diff
//! (`INotebookDiffViewModel`, `INotebookDiffViewModelUpdateEvent`).
//!
//! Divergências:
//! - `INotebookTextDiffEditor` e os `*RenderTemplate` (interfaces do widget, que falam de `HTMLElement`, `CodeEditorWidget`,
//!   `DiffEditorWidget` e `WorkbenchToolBar`) não são portados: a UI do porte é nativa e desenhada por outra camada.
//! - `IDiffNestedCellViewModel` (interface vazia) é o trait marcador [`IDiffNestedCellViewModel`].
//! - `IDiffElementSelfLayoutChangeEvent` (`{ [K in keyof IDiffElementLayoutInfo]?: boolean }`) é o
//!   [`CellDiffViewModelLayoutChangeEvent`], que também tem `font`, `outerWidth`, `metadataEditor`, `outputEditor` e
//!   `outputView`; campo ausente é `false` (ou `None`).
//! - As chaves de contexto (`RawContextKey`) são funções, como `notebook_editor_cursor_boundary`; as sem valor padrão no
//!   original (`undefined`) usam [`Dynamic::Undefined`].
//! - `INotebookDiffViewModel.computeDiff` devolve `Promise<()>`.

use je_base::cancellation::CancellationToken;
use je_base::dynamic::Dynamic;
use je_base::event::Event;
use je_base::lifecycle::IDisposable;
use je_base::nls::localize;
use je_base::promise::Promise;
use je_editor::config::font_info::BareFontInfo;
use je_platform::contextkey::{ContextKeyMeta, RawContextKey};

use super::diff_element_view_model::DiffElementViewModel;
use crate::contrib::notebook::browser::view_model::cell_view_model_types::CellLayoutState;

/// `DiffSide`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DiffSide {
    Original = 0,
    Modified = 1,
}

/// `IDiffNestedCellViewModel`.
pub trait IDiffNestedCellViewModel {}

/// `IDiffElementLayoutInfo`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct IDiffElementLayoutInfo {
    pub total_height: f64,
    pub width: f64,
    pub editor_height: f64,
    pub editor_margin: f64,
    pub metadata_height: f64,
    pub cell_status_height: f64,
    pub metadata_status_height: f64,
    pub raw_output_height: f64,
    pub output_metadata_height: f64,
    pub output_total_height: f64,
    pub output_status_height: f64,
    pub body_margin: f64,
    pub layout_state: CellLayoutState,
}

/// `CellDiffViewModelLayoutChangeEvent`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CellDiffViewModelLayoutChangeEvent {
    pub total_height: bool,
    pub width: bool,
    pub editor_height: bool,
    pub editor_margin: bool,
    pub metadata_height: bool,
    pub cell_status_height: bool,
    pub metadata_status_height: bool,
    pub raw_output_height: bool,
    pub output_metadata_height: bool,
    pub output_total_height: bool,
    pub output_status_height: bool,
    pub body_margin: bool,
    pub layout_state: bool,
    pub font: Option<BareFontInfo>,
    pub outer_width: bool,
    pub metadata_editor: bool,
    pub output_editor: bool,
    pub output_view: bool,
}

/// `DIFF_CELL_MARGIN`.
pub const DIFF_CELL_MARGIN: f64 = 16.0;

/// `getValue<boolean>(key)` lido de um [`Dynamic`]: só o booleano `true` liga (ausente é falso). É a leitura de configuração
/// que a lógica do diff repete (`notebook.diff.ignoreOutputs`, `notebook.diff.ignoreMetadata`...).
pub(super) fn config_flag(value: &Dynamic) -> bool {
    matches!(value, Dynamic::Bool(true))
}

/// `NOTEBOOK_DIFF_CELL_IGNORE_WHITESPACE_KEY`.
pub const NOTEBOOK_DIFF_CELL_IGNORE_WHITESPACE_KEY: &str = "notebook.diffEditor.cell.ignoreWhitespace";

thread_local! {
    static NOTEBOOK_DIFF_CELL_INPUT: RawContextKey = RawContextKey::new("notebook.diffEditor.cell.inputChanged", Dynamic::Bool(false), ContextKeyMeta::None);
    static NOTEBOOK_DIFF_METADATA: RawContextKey = RawContextKey::new("notebook.diffEditor.metadataChanged", Dynamic::Bool(false), ContextKeyMeta::None);
    static NOTEBOOK_DIFF_CELL_IGNORE_WHITESPACE: RawContextKey = RawContextKey::new(NOTEBOOK_DIFF_CELL_IGNORE_WHITESPACE_KEY, Dynamic::Bool(false), ContextKeyMeta::None);
    static NOTEBOOK_DIFF_CELL_PROPERTY: RawContextKey = RawContextKey::new("notebook.diffEditor.cell.property.changed", Dynamic::Bool(false), ContextKeyMeta::None);
    static NOTEBOOK_DIFF_CELL_PROPERTY_EXPANDED: RawContextKey =
        RawContextKey::new("notebook.diffEditor.cell.property.expanded", Dynamic::Bool(false), ContextKeyMeta::None);
    static NOTEBOOK_DIFF_CELLS_COLLAPSED: RawContextKey = RawContextKey::new(
        "notebook.diffEditor.allCollapsed",
        Dynamic::Undefined,
        ContextKeyMeta::Description(localize("notebook.diffEditor.allCollapsed", "Whether all cells in notebook diff editor are collapsed", &[])),
    );
    static NOTEBOOK_DIFF_HAS_UNCHANGED_CELLS: RawContextKey = RawContextKey::new(
        "notebook.diffEditor.hasUnchangedCells",
        Dynamic::Undefined,
        ContextKeyMeta::Description(localize("notebook.diffEditor.hasUnchangedCells", "Whether there are unchanged cells in the notebook diff editor", &[])),
    );
    static NOTEBOOK_DIFF_UNCHANGED_CELLS_HIDDEN: RawContextKey = RawContextKey::new(
        "notebook.diffEditor.unchangedCellsAreHidden",
        Dynamic::Undefined,
        ContextKeyMeta::Description(localize("notebook.diffEditor.unchangedCellsAreHidden", "Whether the unchanged cells in the notebook diff editor are hidden", &[])),
    );
    static NOTEBOOK_DIFF_ITEM_KIND: RawContextKey = RawContextKey::new(
        "notebook.diffEditor.item.kind",
        Dynamic::Undefined,
        ContextKeyMeta::Description(localize("notebook.diffEditor.item.kind", "The kind of item in the notebook diff editor, Cell, Metadata or Output", &[])),
    );
    static NOTEBOOK_DIFF_ITEM_DIFF_STATE: RawContextKey = RawContextKey::new(
        "notebook.diffEditor.item.state",
        Dynamic::Undefined,
        ContextKeyMeta::Description(localize("notebook.diffEditor.item.state", "The diff state of item in the notebook diff editor, delete, insert, modified or unchanged", &[])),
    );
}

/// `NOTEBOOK_DIFF_CELL_INPUT`.
pub fn notebook_diff_cell_input() -> RawContextKey {
    NOTEBOOK_DIFF_CELL_INPUT.with(Clone::clone)
}

/// `NOTEBOOK_DIFF_METADATA`.
pub fn notebook_diff_metadata() -> RawContextKey {
    NOTEBOOK_DIFF_METADATA.with(Clone::clone)
}

/// `NOTEBOOK_DIFF_CELL_IGNORE_WHITESPACE`.
pub fn notebook_diff_cell_ignore_whitespace() -> RawContextKey {
    NOTEBOOK_DIFF_CELL_IGNORE_WHITESPACE.with(Clone::clone)
}

/// `NOTEBOOK_DIFF_CELL_PROPERTY`.
pub fn notebook_diff_cell_property() -> RawContextKey {
    NOTEBOOK_DIFF_CELL_PROPERTY.with(Clone::clone)
}

/// `NOTEBOOK_DIFF_CELL_PROPERTY_EXPANDED`.
pub fn notebook_diff_cell_property_expanded() -> RawContextKey {
    NOTEBOOK_DIFF_CELL_PROPERTY_EXPANDED.with(Clone::clone)
}

/// `NOTEBOOK_DIFF_CELLS_COLLAPSED`.
pub fn notebook_diff_cells_collapsed() -> RawContextKey {
    NOTEBOOK_DIFF_CELLS_COLLAPSED.with(Clone::clone)
}

/// `NOTEBOOK_DIFF_HAS_UNCHANGED_CELLS`.
pub fn notebook_diff_has_unchanged_cells() -> RawContextKey {
    NOTEBOOK_DIFF_HAS_UNCHANGED_CELLS.with(Clone::clone)
}

/// `NOTEBOOK_DIFF_UNCHANGED_CELLS_HIDDEN`.
pub fn notebook_diff_unchanged_cells_hidden() -> RawContextKey {
    NOTEBOOK_DIFF_UNCHANGED_CELLS_HIDDEN.with(Clone::clone)
}

/// `NOTEBOOK_DIFF_ITEM_KIND`.
pub fn notebook_diff_item_kind() -> RawContextKey {
    NOTEBOOK_DIFF_ITEM_KIND.with(Clone::clone)
}

/// `NOTEBOOK_DIFF_ITEM_DIFF_STATE`.
pub fn notebook_diff_item_diff_state() -> RawContextKey {
    NOTEBOOK_DIFF_ITEM_DIFF_STATE.with(Clone::clone)
}

/// `INotebookDiffViewModelUpdateEvent`.
#[derive(Clone)]
pub struct INotebookDiffViewModelUpdateEvent {
    /// O `number` do original: `i64`, porque o `indexOf` sem resultado (`-1`) chega ao evento como `start`.
    pub start: i64,
    pub delete_count: usize,
    pub elements: Vec<DiffElementViewModel>,
    pub first_change_index: Option<i64>,
}

impl PartialEq for INotebookDiffViewModelUpdateEvent {
    /// A igualdade do `assert.deepStrictEqual` dos casos: os mesmos números e os mesmos elementos (por identidade).
    fn eq(&self, other: &Self) -> bool {
        self.start == other.start
            && self.delete_count == other.delete_count
            && self.first_change_index == other.first_change_index
            && self.elements == other.elements
    }
}

impl std::fmt::Debug for INotebookDiffViewModelUpdateEvent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("INotebookDiffViewModelUpdateEvent")
            .field("start", &self.start)
            .field("delete_count", &self.delete_count)
            .field("elements", &self.elements.iter().map(DiffElementViewModel::type_).collect::<Vec<_>>())
            .field("first_change_index", &self.first_change_index)
            .finish()
    }
}

/// `INotebookDiffViewModel`.
pub trait INotebookDiffViewModel: IDisposable {
    fn items(&self) -> Vec<DiffElementViewModel>;

    /// Triggered when ever there's a change in the view model items.
    fn on_did_change_items(&self) -> Event<INotebookDiffViewModelUpdateEvent>;

    /// Computes the differences and generates the viewmodel. If view models are generated, then the `onDidChangeItems` is
    /// triggered.
    fn compute_diff(&self, token: CancellationToken) -> Promise<()>;
}
