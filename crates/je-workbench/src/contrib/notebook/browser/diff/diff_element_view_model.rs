//! Porte de `src/vs/workbench/contrib/notebook/browser/diff/diffElementViewModel.ts`: os view models dos elementos do diff de
//! notebooks (o espaço reservado das células sem mudança, os metadados do documento e as células), o layout deles, e as
//! comparações e formatações de saída que o diff usa.
//!
//! Divergências:
//! - A hierarquia `DiffElementViewModelBase` > {`DiffElementPlaceholderViewModel`, `NotebookDocumentMetadataViewModel`,
//!   `DiffElementCellViewModelBase` > {`SideBySideDiffElementViewModel`, `SingleSideDiffElementViewModel`}} é composição: cada
//!   classe guarda o [`DiffElementViewModelBase`] e o expõe por `Deref`. As duas subclasses de célula são o mesmo tipo
//!   [`DiffElementCellViewModelBase`] com a variante (`SideBySide` ou `SingleSide`) escolhida pelo construtor
//!   ([`DiffElementCellViewModelBase::new_side_by_side`] e [`DiffElementCellViewModelBase::new_single_side`]); os métodos
//!   abstratos da base e os sobrescritos pela subclasse (`computeInputEditorHeight`, `checkIfInputModified`,
//!   `isOutputEmpty`...) escolhem o corpo pela variante. Os aliases [`SideBySideDiffElementViewModel`] e
//!   [`SingleSideDiffElementViewModel`] documentam a qual classe cada chamada se refere; `original` e `modified`, que a
//!   `SideBySide` declara como obrigatórios, são `Option` e os métodos da variante os leem com `expect`.
//! - `IDiffElementViewModelBase` (a união das três classes) é o enum [`DiffElementViewModel`], e o `type` de cada classe é o
//!   [`DiffElementType`] (`'unchanged' | 'insert' | 'delete' | 'modified' | 'placeholder' | 'unchangedMetadata' |
//!   'modifiedMetadata'`).
//! - Os métodos que o original repete em duas classes são um só: `getComputedCellContainerWidth` mora no
//!   [`DiffElementViewModelBase`], e `getOutputOffsetInCell`, `getOutputOffsetInContainer` e `updateOutputHeight` das duas
//!   variantes de célula passam pelo `getNestedCellViewModel` (o `SingleSide` devolve sempre a sua única célula, que é o que
//!   o original lê nas duas formas).
//! - `get x()` que lança (`rawOutputHeight`, `outputStatusHeight`, `editorHeight`...) não tem leitor e não é portado; os
//!   `set x(v)` são `set_x(v)`.
//! - `editorCommon.ICodeEditorViewState | IDiffEditorViewState` (o estado de vista dos editores, `getSourceEditorViewState`,
//!   `saveSpirceEditorViewState` e os dois pares de saída e metadados) é UI do widget de edição e não é portado.
//! - `DiffEditorWidget.ENTIRE_DIFF_OVERVIEW_WIDTH` é a constante [`ENTIRE_DIFF_OVERVIEW_WIDTH`] (`OverviewRulerFeature.
//!   ENTIRE_DIFF_OVERVIEW_WIDTH`, 30).
//! - `ILayoutInfoDelta` (`{ [K in keyof IDiffElementLayoutInfo]?: number } & { rawOutputHeight, recomputeOutput }`) é o
//!   [`LayoutInfoDelta`] só com os campos que `_layout` lê.
//! - `false | { reason: string | undefined }` é `Option<DiffModification>`; o `kind` que `checkIfOutputsModified` acrescenta é o
//!   campo `kind` da mesma struct.
//! - O construtor não recebe o `INotebookService` do original (ele só o repassa ao `DiffNestedCellViewModel`, que no porte
//!   não o usa).
//! - `computeHeights`, `computeEditorHeights` e as duas `compute...EditorHeight` (`async`) devolvem `Promise<()>` e rodam em
//!   `async_fn`; por isso recebem `self: &Rc<Self>`.
//! - `FontInfo | undefined` é `Option<Rc<FontInfo>>` (como em `NotebookLayoutInfo`).

use std::cell::{Cell, RefCell};
use std::ops::Deref;
use std::rc::Rc;

use je_base::async_::async_fn;
use je_base::dynamic::Dynamic;
use je_base::errors::{self, Error};
use je_base::event::{Emitter, Event};
use je_base::hash::hash;
use je_base::lifecycle::{Disposable, DisposableRef, IDisposable};
use je_base::promise::Promise;
use je_base::uri::URI;
use je_editor::config::font_info::FontInfo;
use je_platform::configuration::service::IConfigurationService;
use je_platform::js::to_json;

use super::diff_cell_editor_options::get_editor_padding;
use super::diff_nested_cell_view_model::DiffNestedCellViewModel;
use super::editor_height_calculator::IDiffEditorHeightCalculatorService;
use super::event_dispatcher::{NotebookCellLayoutChangedEvent, NotebookDiffEditorEventDispatcher, NotebookDiffViewEvent};
use super::notebook_diff_editor_browser::{CellDiffViewModelLayoutChangeEvent, DIFF_CELL_MARGIN, DiffSide, IDiffElementLayoutInfo, config_flag};
use crate::contrib::notebook::browser::notebook_view_events::NotebookLayoutInfo;
use crate::contrib::notebook::browser::view_model::cell_view_model_types::CellLayoutState;
use crate::contrib::notebook::common::model::notebook_cell_text_model::{NotebookCellTextModel, get_formatted_metadata_json};
use crate::contrib::notebook::common::model::notebook_metadata_text_model::NotebookDocumentMetadataTextModel;
use crate::contrib::notebook::common::model::notebook_text_model::NotebookTextModel;
use crate::contrib::notebook::common::notebook_common::{CellUri, ICell, ICellOutput, INotebookDocumentMetadataTextModel, IOutputDto, IOutputItemDto};

const PROPERTY_HEADER_HEIGHT: f64 = 25.0;

/// From `.monaco-editor .diff-hidden-lines .center` in src/vs/editor/browser/widget/diffEditor/style.css.
pub const HEIGHT_OF_HIDDEN_LINES_REGION_IN_DIFF_EDITOR: f64 = 24.0;

pub const DEFAULT_LINE_HEIGHT: f64 = 17.0;

/// `DiffEditorWidget.ENTIRE_DIFF_OVERVIEW_WIDTH`.
pub const ENTIRE_DIFF_OVERVIEW_WIDTH: f64 = 30.0;

pub const OUTPUT_EDITOR_HEIGHT_MAGIC: f64 = 1440.0;

/// `PropertyFoldingState`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PropertyFoldingState {
    Expanded,
    Collapsed,
}

/// O `type` de um `IDiffElementViewModelBase`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DiffElementType {
    Unchanged,
    Insert,
    Delete,
    Modified,
    Placeholder,
    UnchangedMetadata,
    ModifiedMetadata,
}

impl DiffElementType {
    /// O valor de texto da união do original.
    pub fn as_str(self) -> &'static str {
        match self {
            DiffElementType::Unchanged => "unchanged",
            DiffElementType::Insert => "insert",
            DiffElementType::Delete => "delete",
            DiffElementType::Modified => "modified",
            DiffElementType::Placeholder => "placeholder",
            DiffElementType::UnchangedMetadata => "unchangedMetadata",
            DiffElementType::ModifiedMetadata => "modifiedMetadata",
        }
    }
}

/// O `initData` dos view models de diff.
#[derive(Clone)]
pub struct DiffElementInitData {
    pub metadata_status_height: f64,
    pub output_status_height: f64,
    pub font_info: Option<Rc<FontInfo>>,
}

/// `ILayoutInfoDelta`.
#[derive(Clone, Copy, Debug, Default)]
pub struct LayoutInfoDelta {
    pub width: Option<f64>,
    pub editor_height: Option<f64>,
    pub editor_margin: Option<f64>,
    pub metadata_height: Option<f64>,
    pub cell_status_height: Option<f64>,
    pub metadata_status_height: Option<f64>,
    pub raw_output_height: Option<f64>,
    pub output_status_height: Option<f64>,
    pub body_margin: Option<f64>,
    pub output_metadata_height: Option<f64>,
    pub recompute_output: bool,
}

/// `IDiffElementViewModelBase`.
#[derive(Clone)]
pub enum DiffElementViewModel {
    Cell(Rc<DiffElementCellViewModelBase>),
    Placeholder(Rc<DiffElementPlaceholderViewModel>),
    NotebookDocumentMetadata(Rc<NotebookDocumentMetadataViewModel>),
}

impl PartialEq for DiffElementViewModel {
    /// A identidade do objeto (o `===` do original).
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (DiffElementViewModel::Cell(a), DiffElementViewModel::Cell(b)) => Rc::ptr_eq(a, b),
            (DiffElementViewModel::Placeholder(a), DiffElementViewModel::Placeholder(b)) => Rc::ptr_eq(a, b),
            (DiffElementViewModel::NotebookDocumentMetadata(a), DiffElementViewModel::NotebookDocumentMetadata(b)) => Rc::ptr_eq(a, b),
            _ => false,
        }
    }
}

impl DiffElementViewModel {
    /// O campo `type`.
    pub fn type_(&self) -> DiffElementType {
        match self {
            DiffElementViewModel::Cell(cell) => cell.type_(),
            DiffElementViewModel::Placeholder(_) => DiffElementType::Placeholder,
            DiffElementViewModel::NotebookDocumentMetadata(metadata) => metadata.type_(),
        }
    }

    /// A célula (`DiffElementCellViewModelBase`): ler membro de célula de outra classe lança `TypeError` no original.
    pub fn as_cell(&self) -> Rc<DiffElementCellViewModelBase> {
        match self {
            DiffElementViewModel::Cell(cell) => cell.clone(),
            _ => errors::throw(Error::new("TypeError: the element is not a cell")),
        }
    }

    /// `showHiddenCells()` do espaço reservado.
    pub fn show_hidden_cells(&self) {
        match self {
            DiffElementViewModel::Placeholder(placeholder) => placeholder.show_hidden_cells(),
            _ => errors::throw(Error::new("TypeError: item.showHiddenCells is not a function")),
        }
    }

    pub fn original(&self) -> Option<Rc<DiffNestedCellViewModel>> {
        match self {
            DiffElementViewModel::Cell(cell) => cell.original.clone(),
            _ => None,
        }
    }

    pub fn modified(&self) -> Option<Rc<DiffNestedCellViewModel>> {
        match self {
            DiffElementViewModel::Cell(cell) => cell.modified.clone(),
            _ => None,
        }
    }

    pub fn base(&self) -> &DiffElementViewModelBase {
        match self {
            DiffElementViewModel::Cell(cell) => cell,
            DiffElementViewModel::Placeholder(placeholder) => placeholder,
            DiffElementViewModel::NotebookDocumentMetadata(metadata) => metadata,
        }
    }

    pub fn total_height(&self) -> f64 {
        match self {
            DiffElementViewModel::Cell(cell) => cell.total_height(),
            DiffElementViewModel::Placeholder(placeholder) => placeholder.total_height(),
            DiffElementViewModel::NotebookDocumentMetadata(metadata) => metadata.total_height(),
        }
    }

    pub fn get_height(&self, line_height: f64) -> f64 {
        match self {
            DiffElementViewModel::Cell(cell) => cell.get_height(line_height),
            DiffElementViewModel::Placeholder(placeholder) => placeholder.get_height(line_height),
            DiffElementViewModel::NotebookDocumentMetadata(metadata) => metadata.get_height(line_height),
        }
    }

    pub fn layout_change(&self) {
        match self {
            DiffElementViewModel::Cell(cell) => cell.layout_change(),
            DiffElementViewModel::Placeholder(placeholder) => placeholder.layout_change(),
            DiffElementViewModel::NotebookDocumentMetadata(metadata) => metadata.layout_change(),
        }
    }

    pub fn render_output(&self) -> bool {
        match self {
            DiffElementViewModel::Cell(cell) => cell.render_output(),
            DiffElementViewModel::Placeholder(placeholder) => placeholder.render_output,
            DiffElementViewModel::NotebookDocumentMetadata(metadata) => metadata.render_output,
        }
    }

    pub fn dispose(&self) {
        match self {
            DiffElementViewModel::Cell(cell) => cell.dispose(),
            DiffElementViewModel::Placeholder(placeholder) => placeholder.dispose(),
            DiffElementViewModel::NotebookDocumentMetadata(metadata) => metadata.dispose(),
        }
    }
}

/// `DiffElementViewModelBase`.
pub struct DiffElementViewModelBase {
    base: Rc<Disposable>,
    pub main_document_text_model: Rc<NotebookTextModel>,
    pub editor_event_dispatcher: Rc<NotebookDiffEditorEventDispatcher>,
    pub init_data: DiffElementInitData,
    layout_info_emitter: Rc<Emitter<CellDiffViewModelLayoutChangeEvent>>,
}

impl DiffElementViewModelBase {
    pub fn new(main_document_text_model: Rc<NotebookTextModel>, editor_event_dispatcher: Rc<NotebookDiffEditorEventDispatcher>, init_data: DiffElementInitData) -> Self {
        let base = Disposable::new();
        let layout_info_emitter = Emitter::<CellDiffViewModelLayoutChangeEvent>::new();
        base.register(layout_info_emitter.clone());

        let emitter = layout_info_emitter.clone();
        base.register(editor_event_dispatcher.on_did_change_layout().listen(move |_| {
            emitter.fire(CellDiffViewModelLayoutChangeEvent { outer_width: true, ..Default::default() });
        }));

        Self { base, main_document_text_model, editor_event_dispatcher, init_data, layout_info_emitter }
    }

    /// `_register(o)`.
    pub fn register(&self, disposable: DisposableRef) -> DisposableRef {
        self.base.register(disposable)
    }

    pub fn on_did_layout_change(&self) -> Event<CellDiffViewModelLayoutChangeEvent> {
        self.layout_info_emitter.event()
    }

    fn fire_layout_change_event(&self, state: CellDiffViewModelLayoutChangeEvent, layout_info: IDiffElementLayoutInfo) {
        self.layout_info_emitter.fire(state);
        self.editor_event_dispatcher.emit(vec![NotebookDiffViewEvent::CellLayoutChanged(NotebookCellLayoutChangedEvent { source: layout_info })]);
    }

    pub fn get_computed_cell_container_width(&self, layout_info: &NotebookLayoutInfo, diff_editor: bool, full_width: bool) -> f64 {
        let overview_width = if diff_editor { ENTIRE_DIFF_OVERVIEW_WIDTH } else { 0.0 };
        if full_width {
            return layout_info.width - 2.0 * DIFF_CELL_MARGIN + overview_width - 2.0;
        }

        (layout_info.width - 2.0 * DIFF_CELL_MARGIN + overview_width) / 2.0 - 18.0 - 2.0
    }
}

impl IDisposable for DiffElementViewModelBase {
    fn dispose(&self) {
        self.base.dispose();
    }
}

/// `DiffElementPlaceholderViewModel`.
pub struct DiffElementPlaceholderViewModel {
    base: DiffElementViewModelBase,
    pub hidden_cells: RefCell<Vec<Rc<DiffElementCellViewModelBase>>>,
    unfold_hidden_cells: Rc<Emitter<()>>,
    pub render_output: bool,
}

impl Deref for DiffElementPlaceholderViewModel {
    type Target = DiffElementViewModelBase;

    fn deref(&self) -> &DiffElementViewModelBase {
        &self.base
    }
}

impl DiffElementPlaceholderViewModel {
    pub fn new(main_document_text_model: Rc<NotebookTextModel>, editor_event_dispatcher: Rc<NotebookDiffEditorEventDispatcher>, init_data: DiffElementInitData) -> Rc<Self> {
        let base = DiffElementViewModelBase::new(main_document_text_model, editor_event_dispatcher, init_data);
        let unfold_hidden_cells = Emitter::<()>::new();
        base.register(unfold_hidden_cells.clone());
        Rc::new(Self { base, hidden_cells: RefCell::new(Vec::new()), unfold_hidden_cells, render_output: false })
    }

    pub fn type_(&self) -> DiffElementType {
        DiffElementType::Placeholder
    }

    pub fn on_unfold_hidden_cells(&self) -> Event<()> {
        self.unfold_hidden_cells.event()
    }

    pub fn total_height(&self) -> f64 {
        24.0 + (2.0 * DIFF_CELL_MARGIN)
    }

    pub fn get_height(&self, _line_height: f64) -> f64 {
        self.total_height()
    }

    pub fn layout_change(&self) {}

    pub fn show_hidden_cells(&self) {
        self.unfold_hidden_cells.fire(());
    }
}

impl IDisposable for DiffElementPlaceholderViewModel {
    fn dispose(&self) {
        self.base.dispose();
    }

    fn constructor_name(&self) -> &'static str {
        "DiffElementPlaceholderViewModel"
    }
}

/// `NotebookDocumentMetadataViewModel`.
pub struct NotebookDocumentMetadataViewModel {
    base: DiffElementViewModelBase,
    pub original_metadata: Rc<NotebookDocumentMetadataTextModel>,
    pub modified_metadata: Rc<NotebookDocumentMetadataTextModel>,
    pub original_document_text_model: Rc<NotebookTextModel>,
    pub modified_document_text_model: Rc<NotebookTextModel>,
    type_: DiffElementType,
    pub cell_folding_state: Cell<PropertyFoldingState>,
    layout_info: RefCell<IDiffElementLayoutInfo>,
    pub render_output: bool,
    editor_height_calculator: Rc<dyn IDiffEditorHeightCalculatorService>,
}

impl Deref for NotebookDocumentMetadataViewModel {
    type Target = DiffElementViewModelBase;

    fn deref(&self) -> &DiffElementViewModelBase {
        &self.base
    }
}

impl NotebookDocumentMetadataViewModel {
    /// `type` é `UnchangedMetadata` ou `ModifiedMetadata`.
    pub fn new(
        original_document_text_model: Rc<NotebookTextModel>,
        modified_document_text_model: Rc<NotebookTextModel>,
        type_: DiffElementType,
        editor_event_dispatcher: Rc<NotebookDiffEditorEventDispatcher>,
        init_data: DiffElementInitData,
        editor_height_calculator: Rc<dyn IDiffEditorHeightCalculatorService>,
    ) -> Rc<Self> {
        let base = DiffElementViewModelBase::new(original_document_text_model.clone(), editor_event_dispatcher, init_data);

        let cell_status_height = PROPERTY_HEADER_HEIGHT;
        let layout_info = IDiffElementLayoutInfo {
            width: 0.0,
            editor_height: 0.0,
            editor_margin: 0.0,
            metadata_height: 0.0,
            cell_status_height,
            metadata_status_height: 0.0,
            raw_output_height: 0.0,
            output_total_height: 0.0,
            output_status_height: 0.0,
            output_metadata_height: 0.0,
            body_margin: 32.0,
            total_height: 82.0 + cell_status_height + 0.0,
            layout_state: CellLayoutState::Uninitialized,
        };

        let cell_folding_state = if type_ == DiffElementType::ModifiedMetadata { PropertyFoldingState::Expanded } else { PropertyFoldingState::Collapsed };
        let original_metadata = NotebookDocumentMetadataTextModel::new(original_document_text_model.clone());
        base.register(original_metadata.clone());
        let modified_metadata = NotebookDocumentMetadataTextModel::new(modified_document_text_model.clone());
        base.register(modified_metadata.clone());

        Rc::new(Self {
            base,
            original_metadata,
            modified_metadata,
            original_document_text_model,
            modified_document_text_model,
            type_,
            cell_folding_state: Cell::new(cell_folding_state),
            layout_info: RefCell::new(layout_info),
            render_output: false,
            editor_height_calculator,
        })
    }

    pub fn type_(&self) -> DiffElementType {
        self.type_
    }

    pub fn layout_info(&self) -> IDiffElementLayoutInfo {
        *self.layout_info.borrow()
    }

    pub fn total_height(&self) -> f64 {
        self.layout_info().total_height
    }

    pub fn set_editor_height(&self, height: f64) {
        self.layout(LayoutInfoDelta { editor_height: Some(height), ..Default::default() });
    }

    pub fn set_editor_margin(&self, margin: f64) {
        self.layout(LayoutInfoDelta { editor_margin: Some(margin), ..Default::default() });
    }

    pub fn compute_heights(self: &Rc<Self>) -> Promise<()> {
        let this = self.clone();
        async_fn(async move {
            if this.type_ == DiffElementType::UnchangedMetadata {
                let line_count = this.original_metadata.text_buffer().borrow().get_line_count() as i64;
                this.set_editor_height(this.editor_height_calculator.compute_height_from_lines(line_count));
            } else {
                let original = this.original_metadata.uri();
                let modified = this.modified_metadata.uri();
                let height = this.editor_height_calculator.diff_and_compute_height(&original, &modified).await?;
                this.set_editor_height(height);
            }
            Ok(())
        })
    }

    pub fn layout_change(&self) {
        self.layout(LayoutInfoDelta { recompute_output: true, ..Default::default() });
    }

    fn layout(&self, delta: LayoutInfoDelta) {
        let current = self.layout_info();
        let width = delta.width.unwrap_or(current.width);
        let editor_height = delta.editor_height.unwrap_or(current.editor_height);
        let editor_margin = delta.editor_margin.unwrap_or(current.editor_margin);
        let cell_status_height = delta.cell_status_height.unwrap_or(current.cell_status_height);
        let body_margin = delta.body_margin.unwrap_or(current.body_margin);

        let total_height = editor_height + editor_margin + cell_status_height + body_margin;

        let new_layout = IDiffElementLayoutInfo {
            width,
            editor_height,
            editor_margin,
            metadata_height: 0.0,
            cell_status_height,
            metadata_status_height: 0.0,
            output_total_height: 0.0,
            output_status_height: 0.0,
            body_margin,
            raw_output_height: 0.0,
            output_metadata_height: 0.0,
            total_height,
            layout_state: CellLayoutState::Measured,
        };

        let change_event = CellDiffViewModelLayoutChangeEvent {
            width: new_layout.width != current.width,
            editor_height: new_layout.editor_height != current.editor_height,
            editor_margin: new_layout.editor_margin != current.editor_margin,
            cell_status_height: new_layout.cell_status_height != current.cell_status_height,
            body_margin: new_layout.body_margin != current.body_margin,
            total_height: new_layout.total_height != current.total_height,
            ..Default::default()
        };
        let something_changed = change_event.width
            || change_event.editor_height
            || change_event.editor_margin
            || change_event.cell_status_height
            || change_event.body_margin
            || change_event.total_height;

        if something_changed {
            *self.layout_info.borrow_mut() = new_layout;
            self.base.fire_layout_change_event(change_event, new_layout);
        }
    }

    pub fn get_height(&self, line_height: f64) -> f64 {
        let layout_info = self.layout_info();
        if layout_info.layout_state == CellLayoutState::Uninitialized {
            let editor_height = if self.cell_folding_state.get() == PropertyFoldingState::Collapsed { 0.0 } else { self.compute_input_editor_height(line_height) };
            self.compute_total_height(editor_height)
        } else {
            layout_info.total_height
        }
    }

    fn compute_total_height(&self, editor_height: f64) -> f64 {
        let layout_info = self.layout_info();
        editor_height
            + layout_info.editor_margin
            + layout_info.metadata_height
            + layout_info.cell_status_height
            + layout_info.metadata_status_height
            + layout_info.output_total_height
            + layout_info.output_status_height
            + layout_info.output_metadata_height
            + layout_info.body_margin
    }

    pub fn compute_input_editor_height(&self, _line_height: f64) -> f64 {
        self.editor_height_calculator
            .compute_height_from_lines(self.original_metadata.text_buffer().borrow().get_line_count().max(self.modified_metadata.text_buffer().borrow().get_line_count()) as i64)
    }
}

impl IDisposable for NotebookDocumentMetadataViewModel {
    fn dispose(&self) {
        self.base.dispose();
    }

    fn constructor_name(&self) -> &'static str {
        "NotebookDocumentMetadataViewModel"
    }
}

/// `{ reason: string | undefined }` (e o `kind` que `checkIfOutputsModified` acrescenta).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DiffModification {
    pub reason: Option<&'static str>,
    pub kind: Option<OutputComparison>,
}

/// `OutputComparison`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OutputComparison {
    Unchanged = 0,
    Metadata = 1,
    Other = 2,
}

/// O `{ renderOutput }` de `_stateChangeEmitter`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DiffElementStateChange {
    pub render_output: bool,
}

enum CellVariant {
    /// `SideBySideDiffElementViewModel`.
    SideBySide {
        other_document_text_model: Rc<NotebookTextModel>,
        /// The height of the editor when the unchanged lines are collapsed.
        editor_height_with_unchanged_lines_collapsed: Cell<Option<f64>>,
    },
    /// `SingleSideDiffElementViewModel`.
    SingleSide { other_document_text_model: Rc<NotebookTextModel> },
}

/// `DiffElementCellViewModelBase` (e, pela variante, `SideBySideDiffElementViewModel` e `SingleSideDiffElementViewModel`).
pub struct DiffElementCellViewModelBase {
    base: DiffElementViewModelBase,
    variant: CellVariant,
    type_: DiffElementType,
    pub index: usize,
    pub cell_folding_state: Cell<PropertyFoldingState>,
    pub metadata_folding_state: Cell<PropertyFoldingState>,
    pub output_folding_state: Cell<PropertyFoldingState>,
    state_change_emitter: Rc<Emitter<DiffElementStateChange>>,
    layout_info: RefCell<IDiffElementLayoutInfo>,
    pub display_icon_to_hide_unmodified_cells: Cell<Option<bool>>,
    hide_unchanged_cells: Rc<Emitter<()>>,
    render_output: Cell<bool>,
    configuration_service: Rc<dyn IConfigurationService>,
    pub diff_editor_height_calculator: Rc<dyn IDiffEditorHeightCalculatorService>,
    pub original: Option<Rc<DiffNestedCellViewModel>>,
    pub modified: Option<Rc<DiffNestedCellViewModel>>,
}

/// `SideBySideDiffElementViewModel`.
pub type SideBySideDiffElementViewModel = DiffElementCellViewModelBase;

/// `SingleSideDiffElementViewModel`.
pub type SingleSideDiffElementViewModel = DiffElementCellViewModelBase;

impl Deref for DiffElementCellViewModelBase {
    type Target = DiffElementViewModelBase;

    fn deref(&self) -> &DiffElementViewModelBase {
        &self.base
    }
}

impl DiffElementCellViewModelBase {
    /// O construtor da `DiffElementCellViewModelBase`.
    #[allow(clippy::too_many_arguments)]
    fn construct(
        main_document_text_model: Rc<NotebookTextModel>,
        variant: CellVariant,
        original: Option<Rc<NotebookCellTextModel>>,
        modified: Option<Rc<NotebookCellTextModel>>,
        type_: DiffElementType,
        editor_event_dispatcher: Rc<NotebookDiffEditorEventDispatcher>,
        init_data: DiffElementInitData,
        index: usize,
        configuration_service: Rc<dyn IConfigurationService>,
        diff_editor_height_calculator: Rc<dyn IDiffEditorHeightCalculatorService>,
    ) -> Rc<Self> {
        let font_info = init_data.font_info.clone();
        let base = DiffElementViewModelBase::new(main_document_text_model, editor_event_dispatcher, init_data);
        let state_change_emitter = Emitter::<DiffElementStateChange>::new();
        let hide_unchanged_cells = Emitter::<()>::new();
        base.register(state_change_emitter.clone());
        base.register(hide_unchanged_cells.clone());

        let cell_folding_state = if modified.as_ref().map(|cell| cell.get_text_buffer_hash()) != original.as_ref().map(|cell| cell.get_text_buffer_hash()) {
            PropertyFoldingState::Expanded
        } else {
            PropertyFoldingState::Collapsed
        };
        let original_view_model = original.map(|cell| {
            let view_model = DiffNestedCellViewModel::new(cell);
            base.register(view_model.clone());
            view_model
        });
        let modified_view_model = modified.map(|cell| {
            let view_model = DiffNestedCellViewModel::new(cell);
            base.register(view_model.clone());
            view_model
        });

        let editor_height = Self::estimate_editor_height(type_, original_view_model.as_ref(), modified_view_model.as_ref(), font_info.as_deref());
        let cell_status_height = PROPERTY_HEADER_HEIGHT;
        let this = Rc::new(Self {
            base,
            variant,
            type_,
            index,
            cell_folding_state: Cell::new(cell_folding_state),
            metadata_folding_state: Cell::new(PropertyFoldingState::Collapsed),
            output_folding_state: Cell::new(PropertyFoldingState::Collapsed),
            state_change_emitter,
            layout_info: RefCell::new(IDiffElementLayoutInfo {
                width: 0.0,
                editor_height,
                editor_margin: 0.0,
                metadata_height: 0.0,
                cell_status_height,
                metadata_status_height: 0.0,
                raw_output_height: 0.0,
                output_total_height: 0.0,
                output_status_height: 0.0,
                output_metadata_height: 0.0,
                body_margin: 32.0,
                total_height: 82.0 + cell_status_height + editor_height,
                layout_state: CellLayoutState::Uninitialized,
            }),
            display_icon_to_hide_unmodified_cells: Cell::new(None),
            hide_unchanged_cells,
            render_output: Cell::new(true),
            configuration_service,
            diff_editor_height_calculator,
            original: original_view_model,
            modified: modified_view_model,
        });
        // `metadataStatusHeight` e `outputStatusHeight` dependem de `ignoreMetadata` e `ignoreOutputs`, que leem a
        // configuração e o documento principal.
        {
            let mut layout_info = this.layout_info.borrow_mut();
            layout_info.metadata_status_height = if this.ignore_metadata() { 0.0 } else { PROPERTY_HEADER_HEIGHT };
            layout_info.output_status_height = if this.ignore_outputs() { 0.0 } else { PROPERTY_HEADER_HEIGHT };
        }
        this
    }

    /// `new SideBySideDiffElementViewModel(...)`.
    #[allow(clippy::too_many_arguments)]
    pub fn new_side_by_side(
        main_document_text_model: Rc<NotebookTextModel>,
        other_document_text_model: Rc<NotebookTextModel>,
        original: Rc<NotebookCellTextModel>,
        modified: Rc<NotebookCellTextModel>,
        type_: DiffElementType,
        editor_event_dispatcher: Rc<NotebookDiffEditorEventDispatcher>,
        init_data: DiffElementInitData,
        configuration_service: Rc<dyn IConfigurationService>,
        index: usize,
        diff_editor_height_calculator: Rc<dyn IDiffEditorHeightCalculatorService>,
    ) -> Rc<Self> {
        let this = Self::construct(
            main_document_text_model.clone(),
            CellVariant::SideBySide { other_document_text_model, editor_height_with_unchanged_lines_collapsed: Cell::new(None) },
            Some(original),
            Some(modified),
            type_,
            editor_event_dispatcher,
            init_data,
            index,
            configuration_service,
            diff_editor_height_calculator,
        );
        let (original, modified) = this.side_by_side_pair();

        this.cell_folding_state.set(if modified.text_model.get_value() != original.text_model.get_value() {
            PropertyFoldingState::Expanded
        } else {
            PropertyFoldingState::Collapsed
        });
        this.metadata_folding_state.set(PropertyFoldingState::Collapsed);
        this.output_folding_state.set(PropertyFoldingState::Collapsed);

        if this.check_metadata_if_modified().is_some() {
            this.metadata_folding_state.set(PropertyFoldingState::Expanded);
        }

        if this.check_if_outputs_modified().is_some() {
            this.output_folding_state.set(PropertyFoldingState::Expanded);
        }

        for nested in [original, modified] {
            let weak = Rc::downgrade(&this);
            this.base.register(nested.on_did_change_output_layout().listen(move |_| {
                if let Some(this) = weak.upgrade() {
                    this.layout(LayoutInfoDelta { recompute_output: true, ..Default::default() });
                }
            }));
        }

        let weak = Rc::downgrade(&this);
        this.base.register(modified.text_model.on_did_change_content().listen(move |_| {
            let Some(this) = weak.upgrade() else { return };
            // `if (mainDocumentTextModel.transientOptions.cellContentMetadata)`: o objeto (mesmo vazio) é sempre verdadeiro.
            let cell_content_metadata = this.main_document_text_model.transient_options().cell_content_metadata;
            {
                let (original, modified) = this.side_by_side_pair();
                let cell_metadata_keys: Vec<String> = cell_content_metadata.keys().cloned().collect();
                // `Object.assign({}, this.modified.metadata)`.
                let mut modified_metadata_raw: Vec<(je_base::JsString, Dynamic)> = modified.text_model.metadata().own_entries();
                let original_cell_metadata = original.text_model.metadata();
                for key in cell_metadata_keys {
                    let key = je_base::JsString::from(key.as_str());
                    if original_cell_metadata.own_keys().contains(&key) {
                        let value = original_cell_metadata.get(&key);
                        match modified_metadata_raw.iter_mut().find(|(existing, _)| *existing == key) {
                            Some(existing) => existing.1 = value,
                            None => modified_metadata_raw.push((key, value)),
                        }
                    }
                }

                modified.text_model.set_metadata(Dynamic::object(modified_metadata_raw));
            }
        }));

        this
    }

    /// `new SingleSideDiffElementViewModel(...)`.
    #[allow(clippy::too_many_arguments)]
    pub fn new_single_side(
        main_document_text_model: Rc<NotebookTextModel>,
        other_document_text_model: Rc<NotebookTextModel>,
        original: Option<Rc<NotebookCellTextModel>>,
        modified: Option<Rc<NotebookCellTextModel>>,
        type_: DiffElementType,
        editor_event_dispatcher: Rc<NotebookDiffEditorEventDispatcher>,
        init_data: DiffElementInitData,
        configuration_service: Rc<dyn IConfigurationService>,
        diff_editor_height_calculator: Rc<dyn IDiffEditorHeightCalculatorService>,
        index: usize,
    ) -> Rc<Self> {
        let this = Self::construct(
            main_document_text_model,
            CellVariant::SingleSide { other_document_text_model },
            original,
            modified,
            type_,
            editor_event_dispatcher,
            init_data,
            index,
            configuration_service,
            diff_editor_height_calculator,
        );

        let weak = Rc::downgrade(&this);
        this.base.register(this.cell_view_model().on_did_change_output_layout().listen(move |_| {
            if let Some(this) = weak.upgrade() {
                this.layout(LayoutInfoDelta { recompute_output: true, ..Default::default() });
            }
        }));

        this
    }

    /// `original` e `modified` da `SideBySideDiffElementViewModel` (declarados sem `undefined` no original).
    fn side_by_side_pair(&self) -> (&Rc<DiffNestedCellViewModel>, &Rc<DiffNestedCellViewModel>) {
        (self.original.as_ref().expect("original"), self.modified.as_ref().expect("modified"))
    }

    pub fn type_(&self) -> DiffElementType {
        self.type_
    }

    pub fn on_did_state_change(&self) -> Event<DiffElementStateChange> {
        self.state_change_emitter.event()
    }

    pub fn on_hide_unchanged_cells(&self) -> Event<()> {
        self.hide_unchanged_cells.event()
    }

    pub fn hide_unchanged_cells(&self) {
        self.hide_unchanged_cells.fire(());
    }

    pub fn set_raw_output_height(&self, height: f64) {
        self.layout(LayoutInfoDelta { raw_output_height: Some(OUTPUT_EDITOR_HEIGHT_MAGIC.min(height)), ..Default::default() });
    }

    pub fn set_output_status_height(&self, height: f64) {
        self.layout(LayoutInfoDelta { output_status_height: Some(height), ..Default::default() });
    }

    pub fn set_output_metadata_height(&self, height: f64) {
        self.layout(LayoutInfoDelta { output_metadata_height: Some(height), ..Default::default() });
    }

    pub fn set_editor_height(&self, height: f64) {
        self.layout(LayoutInfoDelta { editor_height: Some(height), ..Default::default() });
    }

    pub fn set_editor_margin(&self, margin: f64) {
        self.layout(LayoutInfoDelta { editor_margin: Some(margin), ..Default::default() });
    }

    pub fn set_metadata_status_height(&self, height: f64) {
        self.layout(LayoutInfoDelta { metadata_status_height: Some(height), ..Default::default() });
    }

    pub fn set_metadata_height(&self, height: f64) {
        self.layout(LayoutInfoDelta { metadata_height: Some(height), ..Default::default() });
    }

    pub fn set_render_output(&self, value: bool) {
        self.render_output.set(value);
        self.layout(LayoutInfoDelta { recompute_output: true, ..Default::default() });
        self.state_change_emitter.fire(DiffElementStateChange { render_output: value });
    }

    pub fn render_output(&self) -> bool {
        self.render_output.get()
    }

    pub fn layout_info(&self) -> IDiffElementLayoutInfo {
        *self.layout_info.borrow()
    }

    pub fn total_height(&self) -> f64 {
        self.layout_info().total_height
    }

    fn ignore_outputs(&self) -> bool {
        config_flag(&self.configuration_service.get_value(Some("notebook.diff.ignoreOutputs"), None)) || self.main_document_text_model.transient_options().transient_outputs
    }

    fn ignore_metadata(&self) -> bool {
        config_flag(&self.configuration_service.get_value(Some("notebook.diff.ignoreMetadata"), None))
    }

    /// `this.modified!` / `this.original!` do `SingleSide`: a única célula.
    fn cell_view_model(&self) -> &Rc<DiffNestedCellViewModel> {
        match self.type_ {
            DiffElementType::Insert => self.modified.as_ref().expect("modified"),
            _ => self.original.as_ref().expect("original"),
        }
    }

    pub fn other_document_text_model(&self) -> &Rc<NotebookTextModel> {
        match &self.variant {
            CellVariant::SideBySide { other_document_text_model, .. } | CellVariant::SingleSide { other_document_text_model } => other_document_text_model,
        }
    }

    pub fn original_document(&self) -> Rc<NotebookTextModel> {
        match &self.variant {
            CellVariant::SideBySide { .. } => self.other_document_text_model().clone(),
            CellVariant::SingleSide { .. } => {
                if self.type_ == DiffElementType::Insert {
                    self.other_document_text_model().clone()
                } else {
                    self.main_document_text_model.clone()
                }
            }
        }
    }

    pub fn modified_document(&self) -> Rc<NotebookTextModel> {
        match &self.variant {
            CellVariant::SideBySide { .. } => self.main_document_text_model.clone(),
            CellVariant::SingleSide { .. } => {
                if self.type_ == DiffElementType::Insert {
                    self.main_document_text_model.clone()
                } else {
                    self.other_document_text_model().clone()
                }
            }
        }
    }

    pub fn layout_change(&self) {
        self.layout(LayoutInfoDelta { recompute_output: true, ..Default::default() });
    }

    fn estimate_editor_height(
        type_: DiffElementType,
        original: Option<&Rc<DiffNestedCellViewModel>>,
        modified: Option<&Rc<DiffNestedCellViewModel>>,
        font_info: Option<&FontInfo>,
    ) -> f64 {
        let line_height = font_info.map_or(17.0, |font_info| font_info.line_height);

        let cell = match type_ {
            DiffElementType::Unchanged | DiffElementType::Insert => modified.expect("modified"),
            _ => original.expect("original"),
        };
        let line_count = cell.text_model.text_buffer().borrow().get_line_count() as i64;
        let padding = get_editor_padding(line_count);
        line_count as f64 * line_height + padding.top as f64 + padding.bottom as f64
    }

    fn layout(&self, delta: LayoutInfoDelta) {
        let current = self.layout_info();
        let width = delta.width.unwrap_or(current.width);
        let editor_height = delta.editor_height.unwrap_or(current.editor_height);
        let editor_margin = delta.editor_margin.unwrap_or(current.editor_margin);
        let metadata_height = delta.metadata_height.unwrap_or(current.metadata_height);
        let cell_status_height = delta.cell_status_height.unwrap_or(current.cell_status_height);
        let metadata_status_height = delta.metadata_status_height.unwrap_or(current.metadata_status_height);
        let raw_output_height = delta.raw_output_height.unwrap_or(current.raw_output_height);
        let output_status_height = delta.output_status_height.unwrap_or(current.output_status_height);
        let body_margin = delta.body_margin.unwrap_or(current.body_margin);
        let output_metadata_height = delta.output_metadata_height.unwrap_or(current.output_metadata_height);
        let output_height = if self.ignore_outputs() {
            0.0
        } else if delta.recompute_output || delta.raw_output_height.is_some() || delta.output_metadata_height.is_some() {
            self.get_output_total_height(raw_output_height, output_metadata_height)
        } else {
            current.output_total_height
        };

        let total_height = editor_height + editor_margin + cell_status_height + metadata_height + metadata_status_height + output_height + output_status_height + body_margin;

        let new_layout = IDiffElementLayoutInfo {
            width,
            editor_height,
            editor_margin,
            metadata_height,
            cell_status_height,
            metadata_status_height,
            output_total_height: output_height,
            output_status_height,
            body_margin,
            raw_output_height,
            output_metadata_height,
            total_height,
            layout_state: CellLayoutState::Measured,
        };

        let change_event = CellDiffViewModelLayoutChangeEvent {
            width: new_layout.width != current.width,
            editor_height: new_layout.editor_height != current.editor_height,
            editor_margin: new_layout.editor_margin != current.editor_margin,
            metadata_height: new_layout.metadata_height != current.metadata_height,
            cell_status_height: new_layout.cell_status_height != current.cell_status_height,
            metadata_status_height: new_layout.metadata_status_height != current.metadata_status_height,
            output_total_height: new_layout.output_total_height != current.output_total_height,
            output_status_height: new_layout.output_status_height != current.output_status_height,
            body_margin: new_layout.body_margin != current.body_margin,
            output_metadata_height: new_layout.output_metadata_height != current.output_metadata_height,
            total_height: new_layout.total_height != current.total_height,
            ..Default::default()
        };
        let something_changed = change_event.width
            || change_event.editor_height
            || change_event.editor_margin
            || change_event.metadata_height
            || change_event.cell_status_height
            || change_event.metadata_status_height
            || change_event.output_total_height
            || change_event.output_status_height
            || change_event.body_margin
            || change_event.output_metadata_height
            || change_event.total_height;

        if something_changed {
            *self.layout_info.borrow_mut() = new_layout;
            self.base.fire_layout_change_event(change_event, new_layout);
        }
    }

    pub fn get_height(&self, line_height: f64) -> f64 {
        let layout_info = self.layout_info();
        if layout_info.layout_state == CellLayoutState::Uninitialized {
            let editor_height = if self.cell_folding_state.get() == PropertyFoldingState::Collapsed { 0.0 } else { self.compute_input_editor_height(line_height) };
            self.compute_total_height(editor_height)
        } else {
            layout_info.total_height
        }
    }

    fn compute_total_height(&self, editor_height: f64) -> f64 {
        let layout_info = self.layout_info();
        editor_height
            + layout_info.editor_margin
            + layout_info.metadata_height
            + layout_info.cell_status_height
            + layout_info.metadata_status_height
            + layout_info.output_total_height
            + layout_info.output_status_height
            + layout_info.output_metadata_height
            + layout_info.body_margin
    }

    /// `computeInputEditorHeight(lineHeight)`, com a sobrescrita da `SideBySide`.
    pub fn compute_input_editor_height(&self, _line_height: f64) -> f64 {
        if let CellVariant::SideBySide { editor_height_with_unchanged_lines_collapsed, .. } = &self.variant
            && self.type_ == DiffElementType::Modified
            && let Some(height) = editor_height_with_unchanged_lines_collapsed.get()
            && self.check_if_input_modified().is_some()
        {
            return height;
        }

        let original_line_count = self.original.as_ref().map_or(1, |cell| cell.text_model.text_buffer().borrow().get_line_count());
        let modified_line_count = self.modified.as_ref().map_or(1, |cell| cell.text_model.text_buffer().borrow().get_line_count());
        self.diff_editor_height_calculator.compute_height_from_lines(original_line_count.max(modified_line_count) as i64)
    }

    fn get_output_total_height(&self, raw_output_height: f64, metadata_height: f64) -> f64 {
        if self.output_folding_state.get() == PropertyFoldingState::Collapsed {
            return 0.0;
        }

        if self.render_output() {
            if self.is_output_empty() {
                // single line;
                return 24.0;
            }
            self.get_rich_output_total_height() + metadata_height
        } else {
            raw_output_height
        }
    }

    pub fn check_if_input_modified(&self) -> Option<DiffModification> {
        match &self.variant {
            CellVariant::SideBySide { .. } => {
                let (original, modified) = self.side_by_side_pair();
                if original.text_model.get_text_buffer_hash() == modified.text_model.get_text_buffer_hash() {
                    return None;
                }
                Some(DiffModification { reason: Some("Cell content has changed"), kind: None })
            }
            CellVariant::SingleSide { .. } => Some(DiffModification { reason: Some("Cell content has changed"), kind: None }),
        }
    }

    pub fn check_if_outputs_modified(&self) -> Option<DiffModification> {
        match &self.variant {
            CellVariant::SideBySide { .. } => {
                if self.main_document_text_model.transient_options().transient_outputs || self.ignore_outputs() {
                    return None;
                }

                let (original, modified) = self.side_by_side_pair();
                let ret = outputs_equal(&original.outputs(), &modified.outputs());

                if ret == OutputComparison::Unchanged {
                    return None;
                }

                Some(DiffModification { reason: if ret == OutputComparison::Metadata { Some("Output metadata has changed") } else { None }, kind: Some(ret) })
            }
            CellVariant::SingleSide { .. } => None,
        }
    }

    pub fn check_metadata_if_modified(&self) -> Option<DiffModification> {
        match &self.variant {
            CellVariant::SideBySide { .. } => {
                if self.ignore_metadata() {
                    return None;
                }
                let (original, modified) = self.side_by_side_pair();
                let transient_cell_metadata = self.main_document_text_model.transient_options().transient_cell_metadata;
                let hash_of = |cell: &Rc<DiffNestedCellViewModel>| {
                    let language = cell.language();
                    hash(&Dynamic::string(&get_formatted_metadata_json(Some(&transient_cell_metadata), &cell.text_model.metadata(), Some(language.as_str()), None)))
                };
                if hash_of(original) != hash_of(modified) { Some(DiffModification { reason: None, kind: None }) } else { None }
            }
            CellVariant::SingleSide { .. } => None,
        }
    }

    pub fn get_nested_cell_view_model(&self, diff_side: DiffSide) -> &Rc<DiffNestedCellViewModel> {
        match &self.variant {
            CellVariant::SideBySide { .. } => {
                let (original, modified) = self.side_by_side_pair();
                if diff_side == DiffSide::Original { original } else { modified }
            }
            CellVariant::SingleSide { .. } => self.cell_view_model(),
        }
    }

    pub fn update_output_height(&self, diff_side: DiffSide, index: usize, height: f64) {
        use crate::contrib::notebook::browser::notebook_browser::IGenericCellViewModel;
        self.get_nested_cell_view_model(diff_side).update_output_height(index, height, None);
    }

    pub fn get_output_offset_in_container(&self, diff_side: DiffSide, index: usize) -> f64 {
        use crate::contrib::notebook::browser::notebook_browser::IGenericCellViewModel;
        self.get_nested_cell_view_model(diff_side).get_output_offset(index)
    }

    pub fn get_output_offset_in_cell(&self, diff_side: DiffSide, index: usize) -> f64 {
        let offset_in_outputs_container = self.get_output_offset_in_container(diff_side, index);
        let layout_info = self.layout_info();

        layout_info.editor_height
            + layout_info.editor_margin
            + layout_info.metadata_height
            + layout_info.cell_status_height
            + layout_info.metadata_status_height
            + layout_info.output_status_height
            + layout_info.body_margin / 2.0
            + offset_in_outputs_container
    }

    pub fn is_output_empty(&self) -> bool {
        if self.main_document_text_model.transient_options().transient_outputs {
            return true;
        }

        match &self.variant {
            CellVariant::SideBySide { .. } => {
                if self.check_if_outputs_modified().is_some() {
                    return false;
                }

                // outputs are not changed

                self.original.as_ref().is_none_or(|original| original.outputs().is_empty())
            }
            CellVariant::SingleSide { .. } => {
                // outputs are not changed

                match (&self.original, &self.modified) {
                    (Some(original), _) => original.outputs().is_empty(),
                    (None, Some(modified)) => modified.outputs().is_empty(),
                    (None, None) => true,
                }
            }
        }
    }

    pub fn get_rich_output_total_height(&self) -> f64 {
        match &self.variant {
            CellVariant::SideBySide { .. } => {
                let (original, modified) = self.side_by_side_pair();
                original.get_output_total_height().max(modified.get_output_total_height())
            }
            CellVariant::SingleSide { .. } => self.cell_view_model().get_output_total_height(),
        }
    }

    pub fn get_cell_by_uri(&self, cell_uri: &URI) -> Rc<DiffNestedCellViewModel> {
        use crate::contrib::notebook::browser::notebook_browser::IGenericCellViewModel;
        match &self.variant {
            CellVariant::SideBySide { .. } => {
                let (original, modified) = self.side_by_side_pair();
                if cell_uri.to_string(false) == original.uri().to_string(false) { original.clone() } else { modified.clone() }
            }
            CellVariant::SingleSide { .. } => self.cell_view_model().clone(),
        }
    }

    async fn compute_modified_input_editor_height(self: Rc<Self>) -> Result<(), Error> {
        if self.check_if_input_modified().is_some() {
            let (original, modified) = self.side_by_side_pair();
            let height = self.diff_editor_height_calculator.diff_and_compute_height(&original.text_model.uri(), &modified.text_model.uri()).await?;
            self.layout_info.borrow_mut().editor_height = height;
            if let CellVariant::SideBySide { editor_height_with_unchanged_lines_collapsed, .. } = &self.variant {
                editor_height_with_unchanged_lines_collapsed.set(Some(height));
            }
        }
        Ok(())
    }

    async fn compute_modified_metadata_editor_height(self: Rc<Self>) -> Result<(), Error> {
        if self.check_metadata_if_modified().is_some() {
            let (original, modified) = self.side_by_side_pair();
            let original_metadata_uri = CellUri::generate_cell_property_uri(
                &self.original_document().uri(),
                original.text_model.handle(),
                je_base::network::schemas::VSCODE_NOTEBOOK_CELL_METADATA,
            );
            let modified_metadata_uri = CellUri::generate_cell_property_uri(
                &self.modified_document().uri(),
                modified.text_model.handle(),
                je_base::network::schemas::VSCODE_NOTEBOOK_CELL_METADATA,
            );
            let height = self.diff_editor_height_calculator.diff_and_compute_height(&original_metadata_uri, &modified_metadata_uri).await?;
            self.layout_info.borrow_mut().metadata_height = height;
        }
        Ok(())
    }

    pub fn compute_editor_heights(self: &Rc<Self>) -> Promise<()> {
        let this = self.clone();
        async_fn(async move {
            if this.type_ == DiffElementType::Unchanged {
                return Ok(());
            }

            let input = async_fn(this.clone().compute_modified_input_editor_height());
            let metadata = async_fn(this.clone().compute_modified_metadata_editor_height());
            Promise::all(vec![input, metadata]).await?;
            Ok(())
        })
    }
}

impl IDisposable for DiffElementCellViewModelBase {
    fn dispose(&self) {
        self.base.dispose();
    }

    fn constructor_name(&self) -> &'static str {
        "DiffElementCellViewModelBase"
    }
}

/// `outputEqual`.
pub fn output_equal(a: &dyn ICellOutput, b: &dyn ICellOutput) -> OutputComparison {
    if hash(&a.metadata().unwrap_or(Dynamic::Undefined)) == hash(&b.metadata().unwrap_or(Dynamic::Undefined)) {
        return OutputComparison::Other;
    }

    // metadata not equal
    let (a_outputs, b_outputs) = (a.outputs(), b.outputs());
    for (a_output_item, b_output_item) in a_outputs.iter().zip(b_outputs.iter()) {
        if a_output_item.mime != b_output_item.mime {
            return OutputComparison::Other;
        }

        if !output_item_data_equal(a_output_item, b_output_item) {
            return OutputComparison::Other;
        }
    }

    OutputComparison::Metadata
}

/// O tamanho e os bytes de dois itens de saída, que o original compara byte a byte.
fn output_item_data_equal(a: &IOutputItemDto, b: &IOutputItemDto) -> bool {
    if a.data.byte_length() != b.data.byte_length() {
        return false;
    }

    a.data.to_vec() == b.data.to_vec()
}

fn outputs_equal(original: &[Rc<dyn ICellOutput>], modified: &[Rc<dyn ICellOutput>]) -> OutputComparison {
    if original.len() != modified.len() {
        return OutputComparison::Other;
    }

    for (a, b) in original.iter().zip(modified.iter()) {
        if hash(&a.metadata().unwrap_or(Dynamic::Undefined)) != hash(&b.metadata().unwrap_or(Dynamic::Undefined)) {
            return OutputComparison::Metadata;
        }

        let (a_outputs, b_outputs) = (a.outputs(), b.outputs());
        if a_outputs.len() != b_outputs.len() {
            return OutputComparison::Other;
        }

        for (a_output_item, b_output_item) in a_outputs.iter().zip(b_outputs.iter()) {
            if a_output_item.mime != b_output_item.mime {
                return OutputComparison::Other;
            }

            if !output_item_data_equal(a_output_item, b_output_item) {
                return OutputComparison::Other;
            }
        }
    }

    OutputComparison::Unchanged
}

/// `getStreamOutputData`.
pub fn get_stream_output_data(outputs: &[IOutputItemDto]) -> Option<String> {
    let first = outputs.first()?;
    let mime = &first.mime;
    let same_stream = !outputs.iter().any(|op| &op.mime != mime);

    if same_stream { Some(outputs.iter().map(|opit| opit.data.to_string()).collect::<Vec<_>>().join("")) } else { None }
}

/// `getFormattedOutputJSON`.
pub fn get_formatted_output_json(outputs: &[IOutputDto]) -> String {
    if outputs.len() == 1
        && let Some(stream_output_data) = get_stream_output_data(&outputs[0].outputs).filter(|data| !data.is_empty())
    {
        return stream_output_data;
    }

    let value = Dynamic::array(outputs.iter().map(|output| {
        Dynamic::object([
            ("metadata", output.metadata.clone().unwrap_or(Dynamic::Undefined)),
            (
                "outputItems",
                Dynamic::array(output.outputs.iter().map(|opit| Dynamic::object([("mimeType", Dynamic::string(&opit.mime)), ("data", Dynamic::string(&opit.data.to_string()))]))),
            ),
        ])
    }));
    let json = to_json(&value).unwrap_or(je_base::json::Value::Null);
    je_base::json_formatter::to_formatted_string(&json, &je_base::json_formatter::FormattingOptions::default()).to_string_lossy()
}
