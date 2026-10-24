//! Porte de `src/vs/workbench/contrib/notebook/browser/viewModel/notebookViewModelImpl.ts`: o view model do notebook
//! (`NotebookViewModel`), com os view models das células, a seleção, as regiões de dobra (áreas escondidas), os intervalos
//! rastreados, o desfazer e refazer e a busca.
//!
//! Divergências:
//! - `CellViewModel = (CodeCellViewModel | MarkupCellViewModel) & ICellViewModel` é o enum [`CellViewModel`] (a igualdade é por
//!   identidade, o `===` do original). `createCellViewModel` é [`NotebookViewModel::create_cell_view_model`].
//! - `NotebookViewModel` é `Rc<NotebookViewModel>` (construído com `Rc::new_cyclic`: os ouvintes guardam `Weak`). O delegate de
//!   dobra que a célula markdown guarda é um adaptador com `Weak` para o view model, o que evita o ciclo de referência que o
//!   `foldingDelegate = null` do `dispose` desfazia.
//! - As células de um evento do `NotebookTextModel` chegam como `Rc<dyn ICell>`; o porte recupera o `Rc<NotebookCellTextModel>`
//!   pelo `handle` em `notebook.cells()` (o `cell as NotebookCellTextModel` do original não tem equivalente sem downcast).
//! - `IBulkEditService` e `ITextModelService` não existem aqui: `replaceOne` e `replaceAll` (que aplicam `ResourceTextEdit`) e o
//!   `_withElement` (que abre referências aos modelos de texto das células antes de desfazer) não são portados; `undo` e `redo`
//!   chamam direto o `IUndoRedoService`, como o `_withElement` do original faz quando não há `ITextModelService` (o ramo
//!   `MultiModelEditStackElement` não existe no porte do editor). Eles são síncronos (o `UndoRedoService` do porte é síncrono).
//! - `IInstantiationService` não existe como argumento: o construtor recebe os serviços que as células usam.
//! - `find` devolve [`CellFindMatchModel`] (o `CellFindMatchModel` de `findModel.ts`, que `implements CellFindMatchWithIndex`):
//!   a célula, o índice e os `contentMatches`.
//! - `changeModelDecorations` não captura o erro do callback (o `onUnexpectedError` do original): o pânico sobe. O acessor
//!   inválido depois do callback lança `Invalid change accessor`.
//! - `INotebookDeltaDecoration` (união entre a decoração de célula e a de régua) é o enum [`NotebookDeltaDecoration`].
//! - `NotebookLayoutChangeEvent.width` é booleano; o `e.source.width !== undefined` da célula de código é lido como `width`.
//! - `updateOptions(newOptions: Partial<NotebookViewModelOptions>)` recebe `is_read_only: Option<bool>`.
//! - `getEditorViewState` e `restoreEditorViewState` usam [`INotebookEditorViewState`] (com o `ICodeEditorViewState` opaco).

use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::rc::{Rc, Weak};

use je_base::dynamic::Dynamic;
use je_base::errors;
use je_base::event::{Emitter, Event};
use je_base::lifecycle::{Disposable, DisposableStore, IDisposable};
use je_base::strings::single_letter_hash;
use je_base::uri::URI;
use je_editor::edit_stack::SingleModelEditStackElement;
use je_editor::folding::folding_ranges::FoldingRegions;
use je_editor::interval_tree::{IntervalArena, IntervalTree, NodeId};
use je_editor::model::{FindMatch, ModelDecorationOptionsInit, ModelDeltaDecoration, TrackedRangeStickiness};
use je_editor::range::Range;
use je_editor::text_model::ModelDecorationOptions;
use je_editor::undo_redo_service::{IUndoRedoService, UndoRedoTarget};
use je_platform::configuration::service::IConfigurationService;

use super::base_cell_view_model::{BaseCellViewModel, INotebookCellDecorationOptions, LineNumbers};
use super::cell_view_model_types::{
    CellFindMatch, CellFoldingState, CellLayoutChangeEvent, EditorFoldingStateDelegate, INotebookEditorViewState, INotebookViewCellsUpdateEvent,
};
use super::code_cell_view_model::CodeCellViewModel;
use super::markup_cell_view_model::MarkupCellViewModel;
use super::cell_selection_collection::NotebookCellSelectionCollection;
use super::view_context::ViewContext;
use crate::contrib::notebook::browser::notebook_browser::CellEditState;
use crate::contrib::notebook::browser::notebook_options::NotebookOptionsChangeEvent;
use crate::contrib::notebook::browser::notebook_view_events::{NotebookLayoutInfo, NotebookMetadataChangedEvent, NotebookViewEvent};
use crate::contrib::notebook::common::model::notebook_cell_text_model::NotebookCellTextModel;
use crate::contrib::notebook::common::model::notebook_text_model::NotebookTextModel;
use crate::contrib::notebook::common::notebook_common::{
    CellKind, ICell, INotebookCellStatusBarItem, INotebookFindOptions, ISelectionHandleState, ISelectionState, NotebookCellsChangeType, NotebookDocumentMetadata,
    NotebookFindScopeType, NotebookRawContentChange, SelectionStateType,
};
use crate::contrib::notebook::common::notebook_execution_state_service::{ExecutionStateChange, INotebookExecutionStateService, NotebookExecutionType};
use crate::contrib::notebook::common::notebook_range::{ICellRange, cell_indexes_to_ranges, cell_ranges_to_indexes, reduce_cell_ranges};

thread_local! {
    static MODEL_ID: Cell<usize> = const { Cell::new(0) };
}

/// `CellViewModel`.
#[derive(Clone)]
pub enum CellViewModel {
    Code(Rc<CodeCellViewModel>),
    Markup(Rc<MarkupCellViewModel>),
}

impl PartialEq for CellViewModel {
    fn eq(&self, other: &Self) -> bool {
        self.ptr_eq(other)
    }
}

impl CellViewModel {
    pub fn ptr_eq(&self, other: &CellViewModel) -> bool {
        match (self, other) {
            (CellViewModel::Code(a), CellViewModel::Code(b)) => Rc::ptr_eq(a, b),
            (CellViewModel::Markup(a), CellViewModel::Markup(b)) => Rc::ptr_eq(a, b),
            _ => false,
        }
    }

    pub fn base(&self) -> &BaseCellViewModel {
        match self {
            CellViewModel::Code(cell) => cell,
            CellViewModel::Markup(cell) => cell,
        }
    }

    pub fn cell_kind(&self) -> CellKind {
        match self {
            CellViewModel::Code(_) => CellKind::Code,
            CellViewModel::Markup(_) => CellKind::Markup,
        }
    }

    pub fn as_code(&self) -> Option<&Rc<CodeCellViewModel>> {
        match self {
            CellViewModel::Code(cell) => Some(cell),
            CellViewModel::Markup(_) => None,
        }
    }

    pub fn as_markup(&self) -> Option<&Rc<MarkupCellViewModel>> {
        match self {
            CellViewModel::Code(_) => None,
            CellViewModel::Markup(cell) => Some(cell),
        }
    }

    pub fn handle(&self) -> i64 {
        self.base().handle()
    }

    pub fn uri(&self) -> URI {
        self.base().uri()
    }

    pub fn id(&self) -> String {
        self.base().id.clone()
    }

    pub fn language(&self) -> String {
        self.base().language()
    }

    pub fn get_text(&self) -> String {
        self.base().get_text()
    }

    pub fn model(&self) -> Rc<NotebookCellTextModel> {
        self.base().model.clone()
    }

    pub fn get_height(&self, line_height: f64) -> f64 {
        match self {
            CellViewModel::Code(cell) => cell.get_height(line_height),
            CellViewModel::Markup(cell) => cell.get_height(line_height),
        }
    }

    pub fn on_deselect(&self) {
        match self {
            CellViewModel::Code(cell) => cell.on_deselect(),
            CellViewModel::Markup(cell) => cell.on_deselect(),
        }
    }

    pub fn layout_change(&self, event: CellLayoutChangeEvent) {
        match self {
            CellViewModel::Code(cell) => cell.layout_change(event),
            CellViewModel::Markup(cell) => cell.layout_change(event),
        }
    }

    pub fn update_options(&self, e: &NotebookOptionsChangeEvent) {
        match self {
            CellViewModel::Code(cell) => cell.update_options(e),
            CellViewModel::Markup(cell) => cell.update_options(e),
        }
    }

    pub fn restore_editor_view_state(&self, editor_view_states: Option<Dynamic>, total_height: Option<f64>) {
        match self {
            CellViewModel::Code(cell) => cell.restore_editor_view_state(editor_view_states, total_height),
            CellViewModel::Markup(cell) => cell.restore_editor_view_state(editor_view_states, total_height),
        }
    }

    pub fn start_find(&self, value: &str, options: &INotebookFindOptions) -> Option<CellFindMatch> {
        match self {
            CellViewModel::Code(cell) => cell.start_find(self, value, options),
            CellViewModel::Markup(cell) => cell.start_find(self, value, options),
        }
    }

    pub fn dispose(&self) {
        match self {
            CellViewModel::Code(cell) => cell.dispose(),
            CellViewModel::Markup(cell) => cell.dispose(),
        }
    }
}

/// `NotebookViewModelOptions`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NotebookViewModelOptions {
    pub is_read_only: bool,
}

/// `ICellModelDecorations`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ICellModelDecorations {
    pub owner_id: i64,
    pub decorations: Vec<String>,
}

/// `ICellModelDeltaDecorations`.
#[derive(Clone)]
pub struct ICellModelDeltaDecorations {
    pub owner_id: i64,
    pub decorations: Vec<ModelDeltaDecoration>,
}

/// `INotebookDeltaViewZoneDecoration`: a decoração da régua de visão geral de uma view zone.
#[derive(Clone, Debug)]
pub struct INotebookDeltaViewZoneDecoration {
    pub view_zone_id: String,
    pub overview_ruler: Dynamic,
}

/// `INotebookDeltaDecoration`: `INotebookDeltaCellDecoration | INotebookDeltaViewZoneDecoration`.
#[derive(Clone, Debug)]
pub enum NotebookDeltaDecoration {
    Cell { handle: i64, options: INotebookCellDecorationOptions },
    ViewZone(INotebookDeltaViewZoneDecoration),
}

/// `INotebookDeltaCellStatusBarItems`.
#[derive(Clone)]
pub struct INotebookDeltaCellStatusBarItems {
    pub handle: i64,
    pub items: Vec<INotebookCellStatusBarItem>,
}

/// `CellFindMatchModel` (de `findModel.ts`).
#[derive(Clone)]
pub struct CellFindMatchModel {
    pub cell: CellViewModel,
    pub index: usize,
    pub content_matches: Vec<FindMatch>,
}

/// O acessor de `changeModelDecorations` (`IModelDecorationsChangeAccessor`).
pub struct ModelDecorationsChangeAccessor<'a> {
    view_model: &'a NotebookViewModel,
    valid: Cell<bool>,
}

impl ModelDecorationsChangeAccessor<'_> {
    pub fn delta_decorations(&self, old_decorations: &[ICellModelDecorations], new_decorations: &[ICellModelDeltaDecorations]) -> Vec<ICellModelDecorations> {
        if !self.valid.get() {
            errors::throw(errors::Error::new("Invalid change accessor"));
        }
        self.view_model.delta_model_decorations_impl(old_decorations, new_decorations)
    }
}

/// O `EditorFoldingStateDelegate` que as células markdown guardam (com `Weak`, ver as divergências).
struct FoldingStateDelegate {
    view_model: Weak<NotebookViewModel>,
}

impl EditorFoldingStateDelegate for FoldingStateDelegate {
    fn get_cell_index(&self, cell: &CellViewModel) -> i64 {
        self.view_model.upgrade().map(|vm| vm.get_cell_index(cell)).unwrap_or(-1)
    }

    fn get_folding_state(&self, cell_index: i64) -> CellFoldingState {
        self.view_model.upgrade().map(|vm| vm.get_folding_state(cell_index)).unwrap_or(CellFoldingState::None)
    }

    fn get_folding_state_by_handle(&self, handle: i64) -> CellFoldingState {
        match self.view_model.upgrade() {
            Some(vm) => vm.get_folding_state(vm.get_cell_index_by_handle(handle)),
            None => CellFoldingState::None,
        }
    }
}

/// `NotebookViewModel`.
pub struct NotebookViewModel {
    base: Rc<Disposable>,
    this: Weak<NotebookViewModel>,
    local_store: Rc<DisposableStore>,
    handle_to_view_cell_mapping: RefCell<HashMap<i64, CellViewModel>>,
    options: RefCell<NotebookViewModelOptions>,
    on_did_change_options: Rc<Emitter<()>>,
    view_cells: RefCell<Vec<CellViewModel>>,
    pub view_type: String,
    notebook: Rc<NotebookTextModel>,
    view_context: Rc<ViewContext>,
    layout_info: RefCell<Option<NotebookLayoutInfo>>,
    configuration_service: Rc<dyn IConfigurationService>,
    undo_service: Rc<dyn IUndoRedoService>,
    notebook_execution_state_service: Rc<dyn INotebookExecutionStateService>,
    on_did_change_view_cells: Rc<Emitter<INotebookViewCellsUpdateEvent>>,
    last_notebook_edit_resource: RefCell<Vec<URI>>,
    on_did_change_selection: Rc<Emitter<String>>,
    selection_collection: Rc<NotebookCellSelectionCollection>,
    decorations_tree: RefCell<IntervalTree>,
    arena: RefCell<IntervalArena>,
    decorations: RefCell<HashMap<String, NodeId>>,
    last_decoration_id: Cell<i64>,
    instance_id: String,
    pub id: String,
    folding_ranges: RefCell<Option<Rc<FoldingRegions>>>,
    on_did_folding_state_changed: Rc<Emitter<()>>,
    hidden_ranges: RefCell<Vec<ICellRange>>,
    focused: Cell<bool>,
    decoration_id_to_cell_map: RefCell<HashMap<String, i64>>,
    status_bar_item_id_to_cell_map: RefCell<HashMap<String, i64>>,
    last_overview_ruler_decoration_id: Cell<i64>,
    overview_ruler_decorations: RefCell<Vec<(String, INotebookDeltaViewZoneDecoration)>>,
    tracked_range_options: Vec<Rc<ModelDecorationOptions>>,
}

impl NotebookViewModel {
    pub fn new(
        view_type: &str,
        notebook: Rc<NotebookTextModel>,
        view_context: Rc<ViewContext>,
        layout_info: Option<NotebookLayoutInfo>,
        options: NotebookViewModelOptions,
        configuration_service: Rc<dyn IConfigurationService>,
        undo_service: Rc<dyn IUndoRedoService>,
        notebook_execution_state_service: Rc<dyn INotebookExecutionStateService>,
    ) -> Rc<Self> {
        let model_id = MODEL_ID.with(|id| {
            id.set(id.get() + 1);
            id.get()
        });

        let base = Disposable::new();
        let on_did_change_options = Emitter::<()>::new();
        let on_did_change_view_cells = Emitter::<INotebookViewCellsUpdateEvent>::new();
        let on_did_change_selection = Emitter::<String>::new();
        let on_did_folding_state_changed = Emitter::<()>::new();
        base.register(on_did_change_options.clone());
        base.register(on_did_change_view_cells.clone());
        base.register(on_did_change_selection.clone());
        base.register(on_did_folding_state_changed.clone());
        let selection_collection = Rc::new(NotebookCellSelectionCollection::new());
        base.register(selection_collection.clone());

        let tracked_range_options = [
            ("notebook-view-model-tracked-range-always-grows-when-typing-at-edges", TrackedRangeStickiness::AlwaysGrowsWhenTypingAtEdges),
            ("notebook-view-model-tracked-range-never-grows-when-typing-at-edges", TrackedRangeStickiness::NeverGrowsWhenTypingAtEdges),
            ("notebook-view-model-tracked-range-grows-only-when-typing-before", TrackedRangeStickiness::GrowsOnlyWhenTypingBefore),
            ("notebook-view-model-tracked-range-grows-only-when-typing-after", TrackedRangeStickiness::GrowsOnlyWhenTypingAfter),
        ]
        .iter()
        .map(|(description, stickiness)| {
            ModelDecorationOptions::register(&ModelDecorationOptionsInit { description: description.to_string(), stickiness: Some(*stickiness), ..Default::default() })
        })
        .collect();

        let this = Rc::new_cyclic(|this| Self {
            base,
            this: this.clone(),
            local_store: DisposableStore::new(),
            handle_to_view_cell_mapping: RefCell::new(HashMap::new()),
            options: RefCell::new(options),
            on_did_change_options,
            view_cells: RefCell::new(Vec::new()),
            view_type: view_type.to_string(),
            notebook,
            view_context,
            layout_info: RefCell::new(layout_info),
            configuration_service,
            undo_service,
            notebook_execution_state_service,
            on_did_change_view_cells,
            last_notebook_edit_resource: RefCell::new(Vec::new()),
            on_did_change_selection,
            selection_collection,
            decorations_tree: RefCell::new(IntervalTree::new()),
            arena: RefCell::new(IntervalArena::new()),
            decorations: RefCell::new(HashMap::new()),
            last_decoration_id: Cell::new(0),
            instance_id: single_letter_hash(model_id).to_string_lossy(),
            id: format!("$notebookViewModel{model_id}"),
            folding_ranges: RefCell::new(None),
            on_did_folding_state_changed,
            hidden_ranges: RefCell::new(Vec::new()),
            focused: Cell::new(true),
            decoration_id_to_cell_map: RefCell::new(HashMap::new()),
            status_bar_item_id_to_cell_map: RefCell::new(HashMap::new()),
            last_overview_ruler_decoration_id: Cell::new(0),
            overview_ruler_decorations: RefCell::new(Vec::new()),
            tracked_range_options,
        });

        this.register_listeners();

        let notebook_cells = this.notebook.cells();
        let view_cell_count = if this.is_repl() { notebook_cells.len().saturating_sub(1) } else { notebook_cells.len() };
        for cell in notebook_cells.iter().take(view_cell_count) {
            let view_cell = this.create_cell_view_model(cell.clone());
            this.view_cells.borrow_mut().push(view_cell);
        }

        for cell in this.view_cells.borrow().iter() {
            this.handle_to_view_cell_mapping.borrow_mut().insert(cell.handle(), cell.clone());
        }

        this
    }

    fn register_listeners(self: &Rc<Self>) {
        {
            let weak = Rc::downgrade(self);
            self.base.register(self.notebook.on_did_change_content().listen(move |e| {
                let Some(this) = weak.upgrade() else { return };
                let synchronous = e.synchronous.unwrap_or(true);
                for raw in &e.raw_events {
                    match &raw.event {
                        NotebookRawContentChange::ModelChange(change) => this.compute(change.changes.clone(), synchronous),
                        NotebookRawContentChange::Initialize(change) => this.compute(change.changes.clone(), synchronous),
                        NotebookRawContentChange::Move(change) => {
                            this.compute(vec![(change.index, change.length, Vec::new())], synchronous);
                            this.compute(vec![(change.new_idx, 0, change.cells.clone())], synchronous);
                        }
                        _ => {}
                    }
                }
            }));
        }

        {
            let weak = Rc::downgrade(self);
            self.base.register(self.notebook.on_did_change_content().listen(move |content_changes| {
                let Some(this) = weak.upgrade() else { return };
                for e in &content_changes.raw_events {
                    if e.event.kind() == NotebookCellsChangeType::ChangeDocumentMetadata {
                        this.view_context.event_dispatcher.emit(vec![NotebookViewEvent::MetadataChanged(NotebookMetadataChangedEvent { source: this.notebook.metadata() })]);
                    }
                }

                if let Some(state) = &content_changes.end_selection_state {
                    this.update_selections_state(state, "model");
                }
            }));
        }

        {
            let weak = Rc::downgrade(self);
            self.base.register(self.view_context.event_dispatcher.on_did_change_layout().listen(move |e| {
                let Some(this) = weak.upgrade() else { return };
                *this.layout_info.borrow_mut() = Some(e.value.clone());

                for cell in this.view_cells.borrow().clone() {
                    let relayout = match cell.cell_kind() {
                        CellKind::Markup => e.source.width || e.source.font_info,
                        CellKind::Code => e.source.width,
                    };
                    if relayout {
                        cell.layout_change(CellLayoutChangeEvent { outer_width: Some(e.value.width), font: Some(e.value.font_info.clone()), ..Default::default() });
                    }
                }
            }));
        }

        {
            let weak = Rc::downgrade(self);
            self.base.register(self.view_context.notebook_options.on_did_change_options().listen(move |e| {
                let Some(this) = weak.upgrade() else { return };
                for cell in this.view_cells.borrow().clone() {
                    cell.update_options(e);
                }
            }));
        }

        {
            let weak = Rc::downgrade(self);
            self.base.register(self.notebook_execution_state_service.on_did_change_execution().listen(move |e| {
                let Some(this) = weak.upgrade() else { return };
                if e.type_() != NotebookExecutionType::Cell {
                    return;
                }
                if let ExecutionStateChange::Cell(event) = e
                    && let Some(CellViewModel::Code(cell)) = this.get_cell_by_handle(event.cell_handle())
                {
                    cell.update_execution_state(event);
                }
            }));
        }

        {
            let emitter = self.on_did_change_selection.clone();
            self.base.register(self.selection_collection.on_did_change_selection().listen(move |e| {
                emitter.fire(e.clone());
            }));
        }
    }

    /// `createCellViewModel(instantiationService, notebookViewModel, cell, viewContext)`.
    pub fn create_cell_view_model(&self, cell: Rc<NotebookCellTextModel>) -> CellViewModel {
        let layout_info = self.layout_info.borrow().clone();
        if cell.cell_kind() == CellKind::Code {
            CellViewModel::Code(CodeCellViewModel::new(
                &self.view_type,
                cell,
                layout_info.as_ref(),
                self.view_context.clone(),
                self.configuration_service.clone(),
                self.undo_service.clone(),
            ))
        } else {
            let delegate: Rc<dyn EditorFoldingStateDelegate> = Rc::new(FoldingStateDelegate { view_model: self.this.clone() });
            CellViewModel::Markup(MarkupCellViewModel::new(
                &self.view_type,
                cell,
                layout_info.as_ref(),
                delegate,
                self.view_context.clone(),
                self.configuration_service.clone(),
                self.undo_service.clone(),
            ))
        }
    }

    fn cell_text_model_by_handle(&self, cell: &Rc<dyn ICell>) -> Rc<NotebookCellTextModel> {
        let handle = cell.handle();
        self.notebook
            .cells()
            .into_iter()
            .find(|candidate| candidate.handle() == handle)
            .unwrap_or_else(|| errors::throw(errors::Error::new(format!("Cell with handle {handle} not found in the notebook"))))
    }

    fn compute(&self, changes: Vec<(usize, usize, Vec<Rc<dyn ICell>>)>, synchronous: bool) {
        let mut diffs: Vec<(usize, usize, Vec<CellViewModel>)> = changes
            .iter()
            .map(|splice| (splice.0, splice.1, splice.2.iter().map(|cell| self.create_cell_view_model(self.cell_text_model_by_handle(cell))).collect()))
            .collect();

        diffs.reverse();
        for diff in &diffs {
            let deleted_cells: Vec<CellViewModel> = {
                let mut view_cells = self.view_cells.borrow_mut();
                let start = diff.0.min(view_cells.len());
                let end = (start + diff.1).min(view_cells.len());
                view_cells.splice(start..end, diff.2.iter().cloned()).collect()
            };

            self.decorations_tree.borrow_mut().accept_replace(&mut self.arena.borrow_mut(), diff.0 as i64, diff.1 as i64, diff.2.len() as i64, true);
            for cell in &deleted_cells {
                self.handle_to_view_cell_mapping.borrow_mut().remove(&cell.handle());
                // dispose the cell to release ref to the cell text document
                cell.dispose();
            }

            for cell in &diff.2 {
                self.handle_to_view_cell_mapping.borrow_mut().insert(cell.handle(), cell.clone());
            }
        }

        let selection_handles = self.selection_handles();

        self.on_did_change_view_cells.fire(INotebookViewCellsUpdateEvent { synchronous, splices: diffs.clone() });

        let mut end_selection_handles: Vec<i64> = Vec::new();
        if !selection_handles.is_empty() {
            let primary_handle = selection_handles[0];
            let primary_selection_index = self.get_cell_index_by_handle(primary_handle);
            end_selection_handles = vec![primary_handle];
            let mut delta: i64 = 0;

            for _ in 0..diffs.len() {
                let diff = &diffs[0];
                if (diff.0 + diff.1) as i64 <= primary_selection_index {
                    delta += diff.2.len() as i64 - diff.1 as i64;
                    continue;
                }

                if diff.0 as i64 > primary_selection_index {
                    end_selection_handles = vec![primary_handle];
                    break;
                }

                if (diff.0 + diff.1) as i64 > primary_selection_index {
                    end_selection_handles = vec![self.view_cells.borrow()[(diff.0 as i64 + delta) as usize].handle()];
                    break;
                }
            }
        }

        // TODO@rebornix
        let selection_indexes: Vec<i64> = end_selection_handles.iter().map(|handle| self.get_cell_index_by_handle(*handle)).collect();
        let primary = selection_indexes.first().and_then(|index| cell_indexes_to_ranges(&[*index]).first().copied());
        self.selection_collection.set_state(primary, &cell_indexes_to_ranges(&selection_indexes), true, "model");
    }

    fn is_repl(&self) -> bool {
        self.view_type == "repl"
    }

    pub fn options(&self) -> NotebookViewModelOptions {
        *self.options.borrow()
    }

    pub fn on_did_change_options(&self) -> Event<()> {
        self.on_did_change_options.event()
    }

    pub fn view_cells(&self) -> Vec<CellViewModel> {
        self.view_cells.borrow().clone()
    }

    pub fn length(&self) -> usize {
        self.view_cells.borrow().len()
    }

    pub fn notebook_document(&self) -> Rc<NotebookTextModel> {
        self.notebook.clone()
    }

    pub fn uri(&self) -> URI {
        self.notebook.uri()
    }

    pub fn metadata(&self) -> NotebookDocumentMetadata {
        self.notebook.metadata()
    }

    pub fn on_did_change_view_cells(&self) -> Event<INotebookViewCellsUpdateEvent> {
        self.on_did_change_view_cells.event()
    }

    pub fn last_notebook_edit_resource(&self) -> Option<URI> {
        self.last_notebook_edit_resource.borrow().last().cloned()
    }

    pub fn layout_info(&self) -> Option<NotebookLayoutInfo> {
        self.layout_info.borrow().clone()
    }

    pub fn on_did_change_selection(&self) -> Event<String> {
        self.on_did_change_selection.event()
    }

    fn selection_handles(&self) -> Vec<i64> {
        let mut handles_set = HashSet::new();
        let mut handles = Vec::new();
        for index in cell_ranges_to_indexes(&self.selection_collection.selections()) {
            let cell = if index >= 0 && (index as usize) < self.length() { self.cell_at(index) } else { None };
            if let Some(cell) = cell
                && handles_set.insert(cell.handle())
            {
                handles.push(cell.handle());
            }
        }

        handles
    }

    pub fn on_did_folding_state_changed(&self) -> Event<()> {
        self.on_did_folding_state_changed.event()
    }

    pub fn focused(&self) -> bool {
        self.focused.get()
    }

    pub fn update_options(&self, is_read_only: Option<bool>) {
        if let Some(is_read_only) = is_read_only {
            self.options.borrow_mut().is_read_only = is_read_only;
        }
        let readonly = self.options.borrow().is_read_only;
        for cell in self.view_cells.borrow().iter() {
            cell.update_options(&NotebookOptionsChangeEvent { readonly: Some(readonly), ..Default::default() });
        }
        self.on_did_change_options.fire(());
    }

    pub fn get_focus(&self) -> ICellRange {
        self.selection_collection.focus()
    }

    pub fn get_selections(&self) -> Vec<ICellRange> {
        self.selection_collection.selections()
    }

    pub fn get_most_recently_executed_cell(&self) -> Option<CellViewModel> {
        let handle = self.notebook_execution_state_service.get_last_completed_cell_for_notebook(&self.notebook.uri());
        handle.and_then(|handle| self.get_cell_by_handle(handle))
    }

    pub fn set_editor_focus(&self, focused: bool) {
        self.focused.set(focused);
    }

    pub fn validate_range(&self, cell_range: Option<ICellRange>) -> Option<ICellRange> {
        let cell_range = cell_range?;

        let length = self.length() as i64;
        let start = cell_range.start.clamp(0, length);
        let end = cell_range.end.clamp(0, length);

        if start <= end { Some(ICellRange { start, end }) } else { Some(ICellRange { start: end, end: start }) }
    }

    // selection change from list view's `setFocus` and `setSelection` should always use `source: view` to prevent events breaking the list view focus/selection change transaction
    pub fn update_selections_state(&self, state: &ISelectionState, source: &str) {
        if self.focused.get() || source == "model" {
            match state {
                ISelectionState::Handle(ISelectionHandleState { primary, selections }) => {
                    let primary_index = primary.map(|primary| self.get_cell_index_by_handle(primary));
                    let primary_selection = primary_index.and_then(|index| self.validate_range(Some(ICellRange { start: index, end: index + 1 })));
                    let indexes: Vec<i64> = selections.iter().map(|sel| self.get_cell_index_by_handle(*sel)).collect();
                    let valid: Vec<ICellRange> = cell_indexes_to_ranges(&indexes).into_iter().filter_map(|range| self.validate_range(Some(range))).collect();
                    self.selection_collection.set_state(primary_selection, &reduce_cell_ranges(&valid), true, source);
                }
                ISelectionState::Index(index_state) => {
                    let primary_selection = self.validate_range(Some(index_state.focus));
                    let selections: Vec<ICellRange> = index_state.selections.iter().filter_map(|range| self.validate_range(Some(*range))).collect();
                    self.selection_collection.set_state(primary_selection, &reduce_cell_ranges(&selections), true, source);
                }
            }
        }
    }

    pub fn get_folding_start_index(&self, index: i64) -> i64 {
        let Some(ranges) = self.folding_ranges.borrow().clone() else { return -1 };

        let range = ranges.find_range((index + 1) as u32);
        if range < 0 {
            return -1;
        }
        ranges.get_start_line_number(range as usize) as i64 - 1
    }

    pub fn get_folding_state(&self, index: i64) -> CellFoldingState {
        let Some(ranges) = self.folding_ranges.borrow().clone() else { return CellFoldingState::None };

        let range = ranges.find_range((index + 1) as u32);
        if range < 0 {
            return CellFoldingState::None;
        }
        let start_index = ranges.get_start_line_number(range as usize) as i64 - 1;

        if start_index != index {
            return CellFoldingState::None;
        }

        if ranges.is_collapsed(range as usize) { CellFoldingState::Collapsed } else { CellFoldingState::Expanded }
    }

    pub fn get_folded_length(&self, index: i64) -> i64 {
        let Some(ranges) = self.folding_ranges.borrow().clone() else { return 0 };

        let range = ranges.find_range((index + 1) as u32);
        if range < 0 {
            return 0;
        }
        let start_index = ranges.get_start_line_number(range as usize) as i64 - 1;
        let end_index = ranges.get_end_line_number(range as usize) as i64 - 1;

        end_index - start_index
    }

    pub fn update_folding_ranges(&self, ranges: Rc<FoldingRegions>) {
        *self.folding_ranges.borrow_mut() = Some(ranges.clone());
        let mut update_hidden_areas = false;
        let mut new_hidden_areas: Vec<ICellRange> = Vec::new();

        let mut k = 0;

        let mut last_collapsed_start = i64::MAX;
        let mut last_collapsed_end: i64 = -1;

        let old_hidden = self.hidden_ranges.borrow().clone();
        for i in 0..ranges.length() {
            if !ranges.is_collapsed(i) {
                continue;
            }

            let start_line_number = ranges.get_start_line_number(i) as i64 + 1; // the first line is not hidden
            let end_line_number = ranges.get_end_line_number(i) as i64;
            if last_collapsed_start <= start_line_number && end_line_number <= last_collapsed_end {
                // ignore ranges contained in collapsed regions
                continue;
            }

            if !update_hidden_areas && k < old_hidden.len() && old_hidden[k].start + 1 == start_line_number && (old_hidden[k].end + 1) == end_line_number {
                // reuse the old ranges
                new_hidden_areas.push(old_hidden[k]);
                k += 1;
            } else {
                update_hidden_areas = true;
                new_hidden_areas.push(ICellRange { start: start_line_number - 1, end: end_line_number - 1 });
            }
            last_collapsed_start = start_line_number;
            last_collapsed_end = end_line_number;
        }

        if update_hidden_areas || k < old_hidden.len() {
            *self.hidden_ranges.borrow_mut() = new_hidden_areas;
            self.on_did_folding_state_changed.fire(());
        }

        for cell in self.view_cells.borrow().clone() {
            if let CellViewModel::Markup(cell) = &cell {
                cell.trigger_folding_state_change();
            }
        }
    }

    pub fn get_hidden_ranges(&self) -> Vec<ICellRange> {
        self.hidden_ranges.borrow().clone()
    }

    pub fn get_overview_ruler_decorations(&self) -> Vec<INotebookDeltaViewZoneDecoration> {
        self.overview_ruler_decorations.borrow().iter().map(|(_, decoration)| decoration.clone()).collect()
    }

    pub fn get_cell_by_handle(&self, handle: i64) -> Option<CellViewModel> {
        self.handle_to_view_cell_mapping.borrow().get(&handle).cloned()
    }

    pub fn get_cell_index_by_handle(&self, handle: i64) -> i64 {
        self.view_cells.borrow().iter().position(|cell| cell.handle() == handle).map(|index| index as i64).unwrap_or(-1)
    }

    pub fn get_cell_index(&self, cell: &CellViewModel) -> i64 {
        self.view_cells.borrow().iter().position(|candidate| candidate == cell).map(|index| index as i64).unwrap_or(-1)
    }

    pub fn cell_at(&self, index: i64) -> Option<CellViewModel> {
        if index < 0 {
            return None;
        }
        self.view_cells.borrow().get(index as usize).cloned()
    }

    pub fn get_cells_in_range(&self, range: Option<ICellRange>) -> Vec<CellViewModel> {
        let Some(range) = range else { return self.view_cells.borrow().clone() };

        let validated_range = self.validate_range(Some(range));

        if let Some(validated_range) = validated_range {
            let view_cells = self.view_cells.borrow();
            return (validated_range.start..validated_range.end).filter_map(|i| view_cells.get(i as usize).cloned()).collect();
        }

        Vec::new()
    }

    /// If this._viewCells[index] is visible then return index
    pub fn get_nearest_visible_cell_index_upwards(&self, index: i64) -> i64 {
        let hidden_ranges = self.hidden_ranges.borrow();
        for cell_range in hidden_ranges.iter().rev() {
            let fold_start = cell_range.start - 1;
            let fold_end = cell_range.end;

            if fold_start > index {
                continue;
            }

            if fold_start <= index && fold_end >= index {
                return index;
            }

            // foldStart <= index, foldEnd < index
            break;
        }

        index
    }

    pub fn get_next_visible_cell_index(&self, index: i64) -> i64 {
        let hidden_ranges = self.hidden_ranges.borrow();
        for cell_range in hidden_ranges.iter() {
            let fold_start = cell_range.start - 1;
            let fold_end = cell_range.end;

            if fold_end < index {
                continue;
            }

            // foldEnd >= index
            if fold_start <= index {
                return fold_end + 1;
            }

            break;
        }

        index + 1
    }

    pub fn get_previous_visible_cell_index(&self, index: i64) -> i64 {
        let hidden_ranges = self.hidden_ranges.borrow();
        for cell_range in hidden_ranges.iter().rev() {
            let fold_start = cell_range.start - 1;
            let fold_end = cell_range.end;

            if fold_end < index {
                return index;
            }

            if fold_start <= index {
                return fold_start;
            }
        }

        index
    }

    pub fn has_cell(&self, cell: &CellViewModel) -> bool {
        self.handle_to_view_cell_mapping.borrow().contains_key(&cell.handle())
    }

    pub fn get_version_id(&self) -> i64 {
        self.notebook.version_id()
    }

    pub fn get_alternative_id(&self) -> String {
        self.notebook.alternative_version_id()
    }

    pub fn get_tracked_range(&self, id: &str) -> Option<ICellRange> {
        self.get_decoration_range(id)
    }

    fn get_decoration_range(&self, decoration_id: &str) -> Option<ICellRange> {
        let node_id = *self.decorations.borrow().get(decoration_id)?;
        let version_id = self.get_version_id();
        let cached_version_id = self.arena.borrow().get(node_id).cached_version_id;
        if cached_version_id != version_id {
            self.decorations_tree.borrow().resolve_node(&mut self.arena.borrow_mut(), node_id, version_id);
        }
        let arena = self.arena.borrow();
        let node = arena.get(node_id);
        match &node.range {
            None => Some(ICellRange { start: node.cached_absolute_start - 1, end: node.cached_absolute_end - 1 }),
            Some(range) => Some(ICellRange { start: range.start_line_number as i64 - 1, end: range.end_line_number as i64 - 1 }),
        }
    }

    pub fn set_tracked_range(&self, id: Option<&str>, new_range: Option<ICellRange>, new_stickiness: TrackedRangeStickiness) -> Option<String> {
        let node = id.and_then(|id| self.decorations.borrow().get(id).copied());

        let Some(node) = node else {
            let new_range = new_range?;

            return self
                .delta_cell_decorations_impl(
                    0,
                    &[],
                    vec![ModelDeltaDecoration {
                        range: Range::new(new_range.start as i32 + 1, 1, new_range.end as i32 + 1, 1),
                        options: self.tracked_range_options[new_stickiness as usize].clone(),
                    }],
                )
                .into_iter()
                .next();
        };

        let node_key = self.arena.borrow().get(node).id.clone();
        let Some(new_range) = new_range else {
            // node exists, the request is to delete => delete node
            self.decorations_tree.borrow_mut().delete(&mut self.arena.borrow_mut(), node);
            self.decorations.borrow_mut().remove(&node_key);
            return None;
        };

        self.decorations_tree.borrow_mut().delete(&mut self.arena.borrow_mut(), node);
        {
            let mut arena = self.arena.borrow_mut();
            let node_ref = arena.get_mut(node);
            node_ref.reset(
                self.notebook.version_id(),
                new_range.start,
                new_range.end + 1,
                Some(Range::new(new_range.start as i32 + 1, 1, new_range.end as i32 + 1, 1)),
            );
            node_ref.set_options(self.tracked_range_options[new_stickiness as usize].clone());
        }
        self.decorations_tree.borrow_mut().insert(&mut self.arena.borrow_mut(), node);
        Some(node_key)
    }

    fn delta_cell_decorations_impl(&self, owner_id: i32, old_decorations_ids: &[String], new_decorations: Vec<ModelDeltaDecoration>) -> Vec<String> {
        let version_id = self.get_version_id();

        let old_decorations_len = old_decorations_ids.len();
        let mut old_decoration_index = 0;

        let new_decorations_len = new_decorations.len();
        let mut new_decoration_index = 0;

        let mut result: Vec<String> = vec![String::new(); new_decorations_len];
        while old_decoration_index < old_decorations_len || new_decoration_index < new_decorations_len {
            let mut node: Option<NodeId> = None;

            if old_decoration_index < old_decorations_len {
                // (1) get ourselves an old node
                loop {
                    node = self.decorations.borrow().get(&old_decorations_ids[old_decoration_index]).copied();
                    old_decoration_index += 1;
                    if !(node.is_none() && old_decoration_index < old_decorations_len) {
                        break;
                    }
                }

                // (2) remove the node from the tree (if it exists)
                if let Some(node) = node {
                    self.decorations_tree.borrow_mut().delete(&mut self.arena.borrow_mut(), node);
                }
            }

            if new_decoration_index < new_decorations_len {
                // (3) create a new node if necessary
                let node = match node {
                    Some(node) => node,
                    None => {
                        let internal_decoration_id = self.last_decoration_id.get() + 1;
                        self.last_decoration_id.set(internal_decoration_id);
                        let decoration_id = format!("{};{}", self.instance_id, internal_decoration_id);
                        let node = self.arena.borrow_mut().alloc(decoration_id.clone(), 0, 0);
                        self.decorations.borrow_mut().insert(decoration_id, node);
                        node
                    }
                };

                // (4) initialize node
                let new_decoration = &new_decorations[new_decoration_index];
                let range = new_decoration.range;
                {
                    let mut arena = self.arena.borrow_mut();
                    let node_ref = arena.get_mut(node);
                    node_ref.owner_id = owner_id;
                    node_ref.reset(version_id, range.start_line_number as i64, range.end_line_number as i64, Some(range));
                    node_ref.set_options(new_decoration.options.clone());
                }

                self.decorations_tree.borrow_mut().insert(&mut self.arena.borrow_mut(), node);

                result[new_decoration_index] = self.arena.borrow().get(node).id.clone();

                new_decoration_index += 1;
            } else if let Some(node) = node {
                let node_id = self.arena.borrow().get(node).id.clone();
                self.decorations.borrow_mut().remove(&node_id);
            }
        }

        result
    }

    pub fn delta_cell_decorations(&self, old_decorations: &[String], new_decorations: Vec<NotebookDeltaDecoration>) -> Vec<String> {
        for id in old_decorations {
            let handle = self.decoration_id_to_cell_map.borrow().get(id).copied();

            if let Some(handle) = handle {
                if let Some(cell) = self.get_cell_by_handle(handle) {
                    cell.base().delta_cell_decorations(std::slice::from_ref(id), Vec::new());
                }
                self.decoration_id_to_cell_map.borrow_mut().remove(id);
            }

            self.overview_ruler_decorations.borrow_mut().retain(|(decoration_id, _)| decoration_id != id);
        }

        let mut result = Vec::new();

        for decoration in new_decorations {
            match decoration {
                NotebookDeltaDecoration::Cell { handle, options } => {
                    let ret = self.get_cell_by_handle(handle).map(|cell| cell.base().delta_cell_decorations(&[], vec![options])).unwrap_or_default();
                    for id in &ret {
                        self.decoration_id_to_cell_map.borrow_mut().insert(id.clone(), handle);
                    }
                    result.extend(ret);
                }
                NotebookDeltaDecoration::ViewZone(decoration) => {
                    let id = self.last_overview_ruler_decoration_id.get() + 1;
                    self.last_overview_ruler_decoration_id.set(id);
                    let decoration_id = format!("_overview_{};{}", self.id, id);
                    self.overview_ruler_decorations.borrow_mut().push((decoration_id.clone(), decoration));
                    result.push(decoration_id);
                }
            }
        }

        result
    }

    pub fn delta_cell_status_bar_items(&self, old_items: &[String], new_items: Vec<INotebookDeltaCellStatusBarItems>) -> Vec<String> {
        // `groupBy(oldItems, id => map.get(id) ?? -1)`
        let mut deletes_by_handle: BTreeMap<i64, Vec<String>> = BTreeMap::new();
        for id in old_items {
            let handle = self.status_bar_item_id_to_cell_map.borrow().get(id).copied().unwrap_or(-1);
            deletes_by_handle.entry(handle).or_default().push(id.clone());
        }

        let mut result = Vec::new();
        for item_delta in new_items {
            let cell = self.get_cell_by_handle(item_delta.handle);
            let deleted = deletes_by_handle.remove(&item_delta.handle).unwrap_or_default();
            for id in &deleted {
                self.status_bar_item_id_to_cell_map.borrow_mut().remove(id);
            }

            let ret = cell.map(|cell| cell.base().delta_cell_status_bar_items(&deleted, item_delta.items.clone())).unwrap_or_default();
            for id in &ret {
                self.status_bar_item_id_to_cell_map.borrow_mut().insert(id.clone(), item_delta.handle);
            }

            result.extend(ret);
        }

        for (handle, ids) in deletes_by_handle {
            if let Some(cell) = self.get_cell_by_handle(handle) {
                cell.base().delta_cell_status_bar_items(&ids, Vec::new());
            }
            for id in &ids {
                self.status_bar_item_id_to_cell_map.borrow_mut().remove(id);
            }
        }

        result
    }

    pub fn nearest_code_cell_index(&self, index: i64 /* exclusive */) -> i64 {
        let view_cells = self.view_cells();
        let end = (index.max(0) as usize).min(view_cells.len());
        let nearest = view_cells[..end].iter().rev().position(|cell| cell.cell_kind() == CellKind::Code);
        if let Some(nearest) = nearest {
            index - nearest as i64 - 1
        } else {
            let start = ((index + 1).max(0) as usize).min(view_cells.len());
            let nearest_cell_the_other_direction = view_cells[start..].iter().position(|cell| cell.cell_kind() == CellKind::Code);
            if let Some(other) = nearest_cell_the_other_direction {
                return index + 1 + other as i64;
            }
            -1
        }
    }

    pub fn get_editor_view_state(&self) -> INotebookEditorViewState {
        let mut state = INotebookEditorViewState::default();

        for (i, cell) in self.view_cells.borrow().iter().enumerate() {
            if cell.base().get_edit_state() == CellEditState::Editing {
                state.editing_cells.insert(i, true);
            }

            if cell.base().is_input_collapsed() {
                state.collapsed_input_cells.insert(i, true);
            }

            if let CellViewModel::Code(cell) = cell
                && cell.is_output_collapsed()
            {
                state.collapsed_output_cells.insert(i, true);
            }

            if cell.base().line_numbers() != LineNumbers::Inherit {
                state.cell_line_number_states.insert(i, cell.base().line_numbers().as_str().to_string());
            }
        }
        for (i, cell) in self.view_cells.borrow().iter().enumerate() {
            if let Some(view_state) = cell.base().save_editor_view_state() {
                state.editor_view_states.insert(i, view_state);
            }
        }

        state
    }

    pub fn restore_editor_view_state(&self, view_state: Option<&INotebookEditorViewState>) {
        let Some(view_state) = view_state else { return };

        for (index, cell) in self.view_cells.borrow().iter().enumerate() {
            let is_editing = view_state.editing_cells.get(&index).copied().unwrap_or(false);
            let editor_view_state = view_state.editor_view_states.get(&index).cloned();

            cell.base().update_edit_state(if is_editing { CellEditState::Editing } else { CellEditState::Preview }, "viewState");
            let cell_height = view_state.cell_total_heights.as_ref().and_then(|heights| heights.get(&index).copied());
            cell.restore_editor_view_state(editor_view_state, cell_height);
            if view_state.collapsed_input_cells.get(&index).copied().unwrap_or(false) {
                cell.base().set_input_collapsed(true);
            }
            if view_state.collapsed_output_cells.get(&index).copied().unwrap_or(false)
                && let CellViewModel::Code(cell) = cell
            {
                cell.set_output_collapsed(true);
            }
            if let Some(line_numbers) = view_state.cell_line_number_states.get(&index).and_then(|value| LineNumbers::parse(value)) {
                cell.base().set_line_numbers(line_numbers);
            }
        }
    }

    /// Editor decorations across cells. For example, find decorations for multiple code cells
    /// The reason that we can't completely delegate this to CodeEditorWidget is most of the time, the editors for cells are not created yet but we already have decorations for them.
    pub fn change_model_decorations<T>(&self, callback: impl FnOnce(&ModelDecorationsChangeAccessor<'_>) -> T) -> Option<T> {
        let change_accessor = ModelDecorationsChangeAccessor { view_model: self, valid: Cell::new(true) };
        let result = callback(&change_accessor);
        change_accessor.valid.set(false);
        Some(result)
    }

    fn delta_model_decorations_impl(&self, old_decorations: &[ICellModelDecorations], new_decorations: &[ICellModelDeltaDecorations]) -> Vec<ICellModelDecorations> {
        struct Mapping {
            cell: CellViewModel,
            old_decorations: Vec<String>,
            new_decorations: Vec<ModelDeltaDecoration>,
        }
        let mut mapping: Vec<(i64, Mapping)> = Vec::new();
        let find = |mapping: &mut Vec<(i64, Mapping)>, owner_id: i64, view_cells: &[CellViewModel]| {
            if !mapping.iter().any(|(id, _)| *id == owner_id)
                && let Some(cell) = view_cells.iter().find(|cell| cell.handle() == owner_id)
            {
                mapping.push((owner_id, Mapping { cell: cell.clone(), old_decorations: Vec::new(), new_decorations: Vec::new() }));
            }
        };
        let view_cells = self.view_cells();

        for old_decoration in old_decorations {
            let owner_id = old_decoration.owner_id;
            find(&mut mapping, owner_id, &view_cells);
            if let Some((_, data)) = mapping.iter_mut().find(|(id, _)| *id == owner_id) {
                data.old_decorations = old_decoration.decorations.clone();
            }
        }

        for new_decoration in new_decorations {
            let owner_id = new_decoration.owner_id;
            find(&mut mapping, owner_id, &view_cells);
            if let Some((_, data)) = mapping.iter_mut().find(|(id, _)| *id == owner_id) {
                data.new_decorations = new_decoration.decorations.clone();
            }
        }

        mapping
            .into_iter()
            .map(|(owner_id, value)| {
                let cell_ret = value.cell.base().delta_model_decorations(&value.old_decorations, value.new_decorations);
                ICellModelDecorations { owner_id, decorations: cell_ret }
            })
            .collect()
    }

    //#region Find
    pub fn find(&self, value: &str, options: &INotebookFindOptions) -> Vec<CellFindMatchModel> {
        let mut matches: Vec<CellFindMatchModel> = Vec::new();
        let view_cells = self.view_cells();
        let find_cells: Vec<CellViewModel> = match &options.find_scope {
            Some(scope) if scope.find_scope_type == NotebookFindScopeType::Cells || scope.find_scope_type == NotebookFindScopeType::Text => {
                let selected_ranges: Vec<ICellRange> =
                    scope.selected_cell_ranges.as_ref().map(|ranges| ranges.iter().filter_map(|range| self.validate_range(Some(*range))).collect()).unwrap_or_default();
                cell_ranges_to_indexes(&selected_ranges).into_iter().filter_map(|index| view_cells.get(index as usize).cloned()).collect()
            }
            _ => view_cells,
        };

        for (index, cell) in find_cells.iter().enumerate() {
            if let Some(cell_matches) = cell.start_find(value, options) {
                matches.push(CellFindMatchModel { cell: cell_matches.cell, index, content_matches: cell_matches.content_matches });
            }
        }

        // filter based on options and editing state

        matches
            .into_iter()
            .filter(|m| {
                if m.cell.cell_kind() == CellKind::Code {
                    // code cell, we only include its match if include input is enabled
                    return options.include_code_input.unwrap_or(false);
                }

                // markup cell, it depends on the editing state
                if m.cell.base().get_edit_state() == CellEditState::Editing {
                    // editing, even if we includeMarkupPreview
                    options.include_markup_input.unwrap_or(false)
                } else {
                    // cell in preview mode, we should only include it if includeMarkupPreview is false but includeMarkupInput is true
                    // if includeMarkupPreview is true, then we should include the webview match result other than this
                    !options.include_markup_preview.unwrap_or(false) && options.include_markup_input.unwrap_or(false)
                }
            })
            .collect()
    }
    //#endregion

    //#region Undo/Redo
    pub fn undo(&self) -> Vec<URI> {
        let edit_stack = self.undo_service.get_elements(&self.uri());
        let element = edit_stack.past.last().cloned();

        if let Some(element) = element
            && let Some(element) = element.as_resource()
            && element.as_any().is::<SingleModelEditStackElement>()
        {
            self.undo_service.undo(UndoRedoTarget::Resource(&self.uri()));

            return vec![element.resource()];
        }

        self.undo_service.undo(UndoRedoTarget::Resource(&self.uri()));
        Vec::new()
    }

    pub fn redo(&self) -> Vec<URI> {
        let edit_stack = self.undo_service.get_elements(&self.uri());
        let element = edit_stack.future.first().cloned();

        if let Some(element) = element
            && let Some(element) = element.as_resource()
            && element.as_any().is::<SingleModelEditStackElement>()
        {
            self.undo_service.redo(UndoRedoTarget::Resource(&self.uri()));

            return vec![element.resource()];
        }

        self.undo_service.redo(UndoRedoTarget::Resource(&self.uri()));

        Vec::new()
    }
    //#endregion

    pub fn equal(&self, notebook: &Rc<NotebookTextModel>) -> bool {
        Rc::ptr_eq(&self.notebook, notebook)
    }
}

impl IDisposable for NotebookViewModel {
    fn dispose(&self) {
        self.local_store.clear();
        for cell in self.view_cells.borrow().iter() {
            cell.dispose();
        }

        self.base.dispose();
    }

    fn constructor_name(&self) -> &'static str {
        "NotebookViewModel"
    }
}
