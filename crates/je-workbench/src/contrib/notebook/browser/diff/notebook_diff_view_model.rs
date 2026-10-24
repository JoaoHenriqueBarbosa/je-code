//! Porte de `src/vs/workbench/contrib/notebook/browser/diff/notebookDiffViewModel.ts`: o view model do diff de notebooks
//! (`NotebookDiffViewModel`), com o cálculo do diff, os elementos (células, espaço reservado das células sem mudança e
//! metadados), os itens do editor de diff de vários arquivos e a função `prettyChanges` (o `CellDiffInfo` é o de
//! `notebook_diff`).
//!
//! Divergências:
//! - `Disposable` é o campo `base`; o view model nasce como `Rc<Self>` (`Rc::new_cyclic`), e os fechos dos eventos e das
//!   funções `async` guardam um `Weak` ou um `Rc` dele.
//! - O construtor não recebe o `INotebookService` do original (ele só o repassa aos view models das células, que no porte não
//!   o usam).
//! - `INotebookDiffEditorModel` é o `Rc<dyn INotebookDiffEditorModel>` e `INotebookTextModel` é o `Rc<NotebookTextModel>`
//!   concreto.
//! - `computeDiff` do `notebookDiff.ts` recebe as células por fatia de `Rc<dyn ICell>` (o `NotebookCellTextModel` implementa o
//!   `ICell` de `notebook_diff`).
//! - `NotebookMultiDiffEditorItem` e as três subclasses (`NotebookMultiDiffEditorCellItem`, `...MetadataItem`,
//!   `...OutputItem`, que só fixam o `kind` e as chaves de contexto) são a struct [`NotebookMultiDiffEditorItem`] com o
//!   [`NotebookMultiDiffEditorItemKind`] (o `instanceof` do original é o `kind`). Os itens são `Rc` (o `value` devolve o
//!   mesmo objeto que a lista guarda).
//! - `Map<DiffElementPlaceholderViewModel, DiffElementCellViewModelBase[]>` é uma lista de pares comparados por identidade.
//! - `Array.prototype.splice` com `indexOf` sem resultado (`-1`) conta do fim, como no JavaScript ([`splice_items`]).
//! - `boolean | undefined` (`hideOutput`, `ignoreMetadata`, `includeUnchanged`, `hasUnchangedCells`) é `Option<bool>`; o
//!   `getValue<boolean>` ausente é `None`.
//! - `IValueWithChangeEvent<readonly MultiDiffEditorItem[]>` é o par `value()` / `on_did_change()`.

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use je_base::async_::{async_fn, race_cancellation};
use je_base::cancellation::CancellationToken;
use je_base::diff::DiffResult;
use je_base::dynamic::Dynamic;
use je_base::event::{Emitter, Event};
use je_base::lifecycle::{Disposable, DisposableStore, IDisposable};
use je_base::network::schemas;
use je_base::promise::Promise;
use je_base::uri::URI;
use je_editor::config::font_info::FontInfo;
use je_platform::configuration::service::IConfigurationService;

use super::diff_element_view_model::{
    DiffElementCellViewModelBase, DiffElementInitData, DiffElementPlaceholderViewModel, DiffElementType, DiffElementViewModel, NotebookDocumentMetadataViewModel,
};
use super::diff_nested_cell_view_model::DiffNestedCellViewModel;
use super::editor_height_calculator::IDiffEditorHeightCalculatorService;
use super::event_dispatcher::NotebookDiffEditorEventDispatcher;
use super::notebook_diff_editor_browser::{
    INotebookDiffViewModel, INotebookDiffViewModelUpdateEvent, config_flag, notebook_diff_item_diff_state, notebook_diff_item_kind,
};
use crate::contrib::multi_diff_editor::browser::multi_diff_source_resolver_service::MultiDiffEditorItem;
use crate::contrib::notebook::common::model::notebook_cell_text_model::NotebookCellTextModel;
use crate::contrib::notebook::common::model::notebook_text_model::NotebookTextModel;
use crate::contrib::notebook::common::notebook_common::{CellUri, ICell as CommonCell, INotebookDiffEditorModel};
use crate::contrib::notebook::common::notebook_diff::{CellDiffInfo, ICell, compute_diff};
use crate::contrib::notebook::common::services::notebook_worker_service::INotebookEditorWorkerService;

/// O `kind: 'Cell' | 'Metadata' | 'Output'` de `NotebookMultiDiffEditorItem`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NotebookMultiDiffEditorItemKind {
    Cell,
    Metadata,
    Output,
}

impl NotebookMultiDiffEditorItemKind {
    /// O valor de texto de `kind`.
    pub fn as_str(self) -> &'static str {
        match self {
            NotebookMultiDiffEditorItemKind::Cell => "Cell",
            NotebookMultiDiffEditorItemKind::Metadata => "Metadata",
            NotebookMultiDiffEditorItemKind::Output => "Output",
        }
    }
}

/// `NotebookMultiDiffEditorItem` (e as subclasses `Cell`, `Metadata` e `Output`).
#[derive(Clone, Debug)]
pub struct NotebookMultiDiffEditorItem {
    base: MultiDiffEditorItem,
    pub type_: DiffElementType,
    pub container_type: DiffElementType,
    pub kind: NotebookMultiDiffEditorItemKind,
}

impl std::ops::Deref for NotebookMultiDiffEditorItem {
    type Target = MultiDiffEditorItem;

    fn deref(&self) -> &MultiDiffEditorItem {
        &self.base
    }
}

impl NotebookMultiDiffEditorItem {
    /// O construtor das três subclasses: `goToFileUri` é `modifiedUri || originalUri` e as chaves de contexto são o tipo de
    /// item e o estado do diff.
    pub fn new(
        kind: NotebookMultiDiffEditorItemKind,
        original_uri: Option<URI>,
        modified_uri: Option<URI>,
        type_: DiffElementType,
        container_type: DiffElementType,
    ) -> Rc<Self> {
        let go_to_file_uri = modified_uri.clone().or_else(|| original_uri.clone());
        let context_keys = vec![
            (notebook_diff_item_kind().key().to_string(), Dynamic::string(kind.as_str())),
            (notebook_diff_item_diff_state().key().to_string(), Dynamic::string(type_.as_str())),
        ];
        Rc::new(Self { base: MultiDiffEditorItem::new(original_uri, modified_uri, go_to_file_uri, None, Some(context_keys)), type_, container_type, kind })
    }
}

/// `Array.prototype.splice(start, deleteCount, ...items)`: `start` negativo conta do fim.
fn splice_items<T>(list: &mut Vec<T>, start: i64, delete_count: usize, items: Vec<T>) {
    let len = list.len() as i64;
    let actual_start = if start < 0 { (len + start).max(0) } else { start.min(len) } as usize;
    let end = (actual_start + delete_count).min(list.len());
    list.splice(actual_start..end, items);
}

/// `getValue<boolean>(key)`: `None` quando ausente.
fn config_option_flag(value: &Dynamic) -> Option<bool> {
    match value {
        Dynamic::Bool(flag) => Some(*flag),
        _ => None,
    }
}

/// `NotebookDiffViewModel`.
pub struct NotebookDiffViewModel {
    base: Rc<Disposable>,
    this: Weak<NotebookDiffViewModel>,
    placeholder_and_related_cells: RefCell<Vec<(Rc<DiffElementPlaceholderViewModel>, Vec<Rc<DiffElementCellViewModelBase>>)>>,
    items: RefCell<Vec<DiffElementViewModel>>,
    on_did_change_items: Rc<Emitter<INotebookDiffViewModelUpdateEvent>>,
    disposables: Rc<DisposableStore>,
    on_did_change: Rc<Emitter<()>>,
    diff_editor_items: RefCell<Vec<Rc<NotebookMultiDiffEditorItem>>>,
    notebook_metadata_view_model: RefCell<Option<Rc<NotebookDocumentMetadataViewModel>>>,
    has_unchanged_cells: Cell<Option<bool>>,
    include_unchanged: Cell<Option<bool>>,
    hide_output: Cell<Option<bool>>,
    ignore_metadata: Cell<Option<bool>>,
    original_cell_view_models: RefCell<Vec<DiffElementViewModel>>,
    model: Rc<dyn INotebookDiffEditorModel>,
    notebook_editor_worker_service: Rc<dyn INotebookEditorWorkerService>,
    configuration_service: Rc<dyn IConfigurationService>,
    event_dispatcher: Rc<NotebookDiffEditorEventDispatcher>,
    diff_editor_height_calculator: Rc<dyn IDiffEditorHeightCalculatorService>,
    font_info: Option<Rc<FontInfo>>,
    exclude_unchanged_placeholder: Option<bool>,
}

impl NotebookDiffViewModel {
    pub fn new(
        model: Rc<dyn INotebookDiffEditorModel>,
        notebook_editor_worker_service: Rc<dyn INotebookEditorWorkerService>,
        configuration_service: Rc<dyn IConfigurationService>,
        event_dispatcher: Rc<NotebookDiffEditorEventDispatcher>,
        diff_editor_height_calculator: Rc<dyn IDiffEditorHeightCalculatorService>,
        font_info: Option<Rc<FontInfo>>,
        exclude_unchanged_placeholder: Option<bool>,
    ) -> Rc<Self> {
        let this = Rc::new_cyclic(|this| {
            let base = Disposable::new();
            let on_did_change_items = Emitter::<INotebookDiffViewModelUpdateEvent>::new();
            base.register(on_did_change_items.clone());
            let disposables = DisposableStore::new();
            base.register(disposables.clone());
            let on_did_change = Emitter::<()>::new();
            base.register(on_did_change.clone());

            let transient_outputs = model.modified().notebook.transient_options().transient_outputs;
            let hide_output = if transient_outputs { Some(true) } else { config_option_flag(&configuration_service.get_value(Some("notebook.diff.ignoreOutputs"), None)) };
            let ignore_metadata = config_option_flag(&configuration_service.get_value(Some("notebook.diff.ignoreMetadata"), None));

            Self {
                base,
                this: this.clone(),
                placeholder_and_related_cells: RefCell::new(Vec::new()),
                items: RefCell::new(Vec::new()),
                on_did_change_items,
                disposables,
                on_did_change,
                diff_editor_items: RefCell::new(Vec::new()),
                notebook_metadata_view_model: RefCell::new(None),
                has_unchanged_cells: Cell::new(None),
                include_unchanged: Cell::new(None),
                hide_output: Cell::new(hide_output),
                ignore_metadata: Cell::new(ignore_metadata),
                original_cell_view_models: RefCell::new(Vec::new()),
                model,
                notebook_editor_worker_service,
                configuration_service,
                event_dispatcher,
                diff_editor_height_calculator,
                font_info,
                exclude_unchanged_placeholder,
            }
        });

        let weak = Rc::downgrade(&this);
        this.base.register(this.configuration_service.on_did_change_configuration().listen(move |e| {
            let Some(this) = weak.upgrade() else { return };
            let mut trigger_change = false;
            let mut metadata_changed = false;
            if e.affects_configuration("notebook.diff.ignoreMetadata", None) {
                let new_value = config_option_flag(&this.configuration_service.get_value(Some("notebook.diff.ignoreMetadata"), None));

                if new_value.is_some() && this.ignore_metadata.get() != new_value {
                    this.ignore_metadata.set(new_value);
                    trigger_change = true;
                    metadata_changed = true;
                }
            }

            if e.affects_configuration("notebook.diff.ignoreOutputs", None) {
                let new_value = config_option_flag(&this.configuration_service.get_value(Some("notebook.diff.ignoreOutputs"), None));
                let transient_outputs = this.model.modified().notebook.transient_options().transient_outputs;

                if let Some(new_value) = new_value
                    && this.hide_output.get() != Some(new_value || transient_outputs)
                {
                    this.hide_output.set(Some(new_value || transient_outputs));
                    trigger_change = true;
                }
            }

            if metadata_changed {
                this.toggle_notebook_metadata();
            }
            if trigger_change {
                this.on_did_change.fire(());
            }
        }));

        this
    }

    /// `get value()`: os itens do editor de diff de vários arquivos que estão à mostra.
    pub fn value(&self) -> Vec<Rc<NotebookMultiDiffEditorItem>> {
        let include_unchanged = self.include_unchanged.get() == Some(true);
        let hide_output = self.hide_output.get() == Some(true);
        let ignore_metadata = self.ignore_metadata.get() == Some(true);
        self.diff_editor_items
            .borrow()
            .iter()
            .filter(|item| item.type_ != DiffElementType::Placeholder)
            .filter(|item| {
                if include_unchanged {
                    return true;
                }
                !(item.type_ == DiffElementType::Unchanged && item.container_type == DiffElementType::Unchanged)
            })
            .filter(|item| item.kind != NotebookMultiDiffEditorItemKind::Output || !hide_output)
            .filter(|item| item.kind != NotebookMultiDiffEditorItemKind::Metadata || !ignore_metadata)
            .cloned()
            .collect()
    }

    pub fn on_did_change(&self) -> Event<()> {
        self.on_did_change.event()
    }

    pub fn has_unchanged_cells(&self) -> bool {
        self.has_unchanged_cells.get() == Some(true)
    }

    pub fn include_unchanged(&self) -> bool {
        self.include_unchanged.get() == Some(true)
    }

    pub fn set_include_unchanged(&self, value: bool) {
        self.include_unchanged.set(Some(value));
        self.on_did_change.fire(());
    }

    fn clear(&self) {
        self.disposables.clear();
        let placeholders: Vec<Rc<DiffElementPlaceholderViewModel>> = std::mem::take(&mut *self.placeholder_and_related_cells.borrow_mut()).into_iter().map(|(placeholder, _)| placeholder).collect();
        for placeholder in &placeholders {
            placeholder.dispose();
        }
        let original_cell_view_models = std::mem::take(&mut *self.original_cell_view_models.borrow_mut());
        for view_model in &original_cell_view_models {
            view_model.dispose();
        }
        let items = std::mem::take(&mut *self.items.borrow_mut());
        for item in &items {
            item.dispose();
        }
    }

    /// `private toggleNotebookMetadata()`.
    fn toggle_notebook_metadata(&self) {
        let Some(notebook_metadata_view_model) = self.notebook_metadata_view_model.borrow().clone() else {
            return;
        };
        let metadata_item = DiffElementViewModel::NotebookDocumentMetadata(notebook_metadata_view_model);

        if self.ignore_metadata.get() == Some(true) {
            let is_first = self.items.borrow().first().is_some_and(|first| *first == metadata_item);
            if is_first {
                self.items.borrow_mut().remove(0);
                self.on_did_change_items.fire(INotebookDiffViewModelUpdateEvent { start: 0, delete_count: 1, elements: Vec::new(), first_change_index: None });
            }
        } else {
            let is_first = self.items.borrow().first().is_some_and(|first| *first == metadata_item);
            if !is_first {
                self.items.borrow_mut().insert(0, metadata_item.clone());
                self.on_did_change_items.fire(INotebookDiffViewModelUpdateEvent { start: 0, delete_count: 0, elements: vec![metadata_item], first_change_index: None });
            }
        }
    }

    fn update_diff_editor_items(&self) {
        let mut diff_editor_items: Vec<Rc<NotebookMultiDiffEditorItem>> = Vec::new();
        let original_source_uri = self.model.original().resource;
        let modified_source_uri = self.model.modified().resource;
        self.has_unchanged_cells.set(Some(false));
        let items = self.items.borrow().clone();
        let property_item = |kind: NotebookMultiDiffEditorItemKind,
                             scheme: &str,
                             original: Option<&Rc<DiffNestedCellViewModel>>,
                             modified: Option<&Rc<DiffNestedCellViewModel>>,
                             type_: DiffElementType,
                             container_type: DiffElementType| {
            let original_uri = original.map(|cell| CellUri::generate_cell_property_uri(&original_source_uri, cell.text_model.handle(), scheme));
            let modified_uri = modified.map(|cell| CellUri::generate_cell_property_uri(&modified_source_uri, cell.text_model.handle(), scheme));
            NotebookMultiDiffEditorItem::new(kind, original_uri, modified_uri, type_, container_type)
        };
        for item in &items {
            let type_ = item.type_();
            let DiffElementViewModel::Cell(cell) = item else { continue };
            let (original, modified) = (cell.original.as_ref(), cell.modified.as_ref());
            match type_ {
                DiffElementType::Delete => {
                    diff_editor_items.push(NotebookMultiDiffEditorItem::new(NotebookMultiDiffEditorItemKind::Cell, original.map(|cell| cell.text_model.uri()), None, type_, type_));
                    diff_editor_items.push(property_item(NotebookMultiDiffEditorItemKind::Metadata, schemas::VSCODE_NOTEBOOK_CELL_METADATA, original, None, type_, type_));
                    diff_editor_items.push(property_item(NotebookMultiDiffEditorItemKind::Output, schemas::VSCODE_NOTEBOOK_CELL_OUTPUT, original, None, type_, type_));
                }
                DiffElementType::Insert => {
                    diff_editor_items.push(NotebookMultiDiffEditorItem::new(NotebookMultiDiffEditorItemKind::Cell, None, modified.map(|cell| cell.text_model.uri()), type_, type_));
                    diff_editor_items.push(property_item(NotebookMultiDiffEditorItemKind::Metadata, schemas::VSCODE_NOTEBOOK_CELL_METADATA, None, modified, type_, type_));
                    diff_editor_items.push(property_item(NotebookMultiDiffEditorItemKind::Output, schemas::VSCODE_NOTEBOOK_CELL_OUTPUT, None, modified, type_, type_));
                }
                DiffElementType::Modified => {
                    let input_modified = cell.check_if_input_modified().is_some();
                    let metadata_modified = cell.check_metadata_if_modified().is_some();
                    let outputs_modified = cell.check_if_outputs_modified().is_some();
                    let cell_type = if input_modified { type_ } else { DiffElementType::Unchanged };
                    let container_changed = if input_modified || metadata_modified || outputs_modified { type_ } else { DiffElementType::Unchanged };
                    diff_editor_items.push(NotebookMultiDiffEditorItem::new(
                        NotebookMultiDiffEditorItemKind::Cell,
                        original.map(|cell| cell.text_model.uri()),
                        modified.map(|cell| cell.text_model.uri()),
                        cell_type,
                        container_changed,
                    ));
                    diff_editor_items.push(property_item(
                        NotebookMultiDiffEditorItemKind::Metadata,
                        schemas::VSCODE_NOTEBOOK_CELL_METADATA,
                        original,
                        modified,
                        if metadata_modified { type_ } else { DiffElementType::Unchanged },
                        container_changed,
                    ));
                    diff_editor_items.push(property_item(
                        NotebookMultiDiffEditorItemKind::Output,
                        schemas::VSCODE_NOTEBOOK_CELL_OUTPUT,
                        original,
                        modified,
                        if outputs_modified { type_ } else { DiffElementType::Unchanged },
                        container_changed,
                    ));
                }
                DiffElementType::Unchanged => {
                    self.has_unchanged_cells.set(Some(true));
                    diff_editor_items.push(NotebookMultiDiffEditorItem::new(
                        NotebookMultiDiffEditorItemKind::Cell,
                        original.map(|cell| cell.text_model.uri()),
                        modified.map(|cell| cell.text_model.uri()),
                        type_,
                        type_,
                    ));
                    diff_editor_items.push(property_item(NotebookMultiDiffEditorItemKind::Metadata, schemas::VSCODE_NOTEBOOK_CELL_METADATA, original, modified, type_, type_));
                    diff_editor_items.push(property_item(NotebookMultiDiffEditorItemKind::Output, schemas::VSCODE_NOTEBOOK_CELL_OUTPUT, original, modified, type_, type_));
                }
                _ => {}
            }
        }

        *self.diff_editor_items.borrow_mut() = diff_editor_items;
        self.on_did_change.fire(());
    }

    fn update_view_models(self: &Rc<Self>, cell_diff_info: Vec<CellDiffInfo>, metadata_changed: bool, first_change_index: i64) -> Promise<()> {
        let this = self.clone();
        async_fn(async move {
            let cell_view_models = this.create_diff_view_models(cell_diff_info, metadata_changed).await?;
            let old_length = this.items.borrow().len();
            this.clear();
            this.items.borrow_mut().clear();

            let mut placeholder: Option<Rc<DiffElementPlaceholderViewModel>> = None;
            *this.original_cell_view_models.borrow_mut() = cell_view_models.clone();
            for vm in cell_view_models {
                if vm.type_() == DiffElementType::Unchanged && this.exclude_unchanged_placeholder != Some(true) {
                    let cell = vm.as_cell();
                    let placeholder_item = match placeholder.clone() {
                        Some(placeholder_item) => placeholder_item,
                        None => {
                            cell.display_icon_to_hide_unmodified_cells.set(Some(true));
                            let placeholder_item =
                                DiffElementPlaceholderViewModel::new(cell.main_document_text_model.clone(), cell.editor_event_dispatcher.clone(), cell.init_data.clone());
                            this.items.borrow_mut().push(DiffElementViewModel::Placeholder(placeholder_item.clone()));
                            this.listen_to_placeholder(&placeholder_item, &cell);
                            placeholder = Some(placeholder_item.clone());
                            placeholder_item
                        }
                    };
                    {
                        let mut related_cells = this.placeholder_and_related_cells.borrow_mut();
                        match related_cells.iter_mut().find(|(candidate, _)| Rc::ptr_eq(candidate, &placeholder_item)) {
                            Some((_, hidden_cell_view_models)) => hidden_cell_view_models.push(cell.clone()),
                            None => related_cells.push((placeholder_item.clone(), vec![cell.clone()])),
                        }
                    }
                    placeholder_item.hidden_cells.borrow_mut().push(cell);
                } else {
                    placeholder = None;
                    this.items.borrow_mut().push(vm);
                }
            }

            // Note, ensure all of the height calculations are done before firing the event.
            // This is to ensure that the diff editor is not resized multiple times, thereby avoiding flickering.
            let elements = this.items.borrow().clone();
            this.on_did_change_items.fire(INotebookDiffViewModelUpdateEvent { start: 0, delete_count: old_length, elements, first_change_index: Some(first_change_index) });
            Ok(())
        })
    }

    /// Os dois ouvintes que o primeiro `placeholder` de uma sequência de células sem mudança registra: mostrar as células
    /// escondidas (no lugar do espaço reservado) e escondê-las de novo (no lugar delas).
    fn listen_to_placeholder(&self, placeholder_item: &Rc<DiffElementPlaceholderViewModel>, first_cell: &Rc<DiffElementCellViewModelBase>) {
        let weak = self.this.clone();
        let weak_placeholder = Rc::downgrade(placeholder_item);
        self.disposables.add(placeholder_item.on_unfold_hidden_cells().listen(move |_| {
            let (Some(this), Some(placeholder_item)) = (weak.upgrade(), weak_placeholder.upgrade()) else { return };
            let Some(hidden_cell_view_models) = this.hidden_cells_of(&placeholder_item) else { return };
            let start = this.index_of(&DiffElementViewModel::Placeholder(placeholder_item));
            let elements: Vec<DiffElementViewModel> = hidden_cell_view_models.into_iter().map(DiffElementViewModel::Cell).collect();
            splice_items(&mut this.items.borrow_mut(), start, 1, elements.clone());
            this.on_did_change_items.fire(INotebookDiffViewModelUpdateEvent { start, delete_count: 1, elements, first_change_index: None });
        }));

        let weak = self.this.clone();
        let weak_placeholder = Rc::downgrade(placeholder_item);
        let weak_cell = Rc::downgrade(first_cell);
        self.disposables.add(first_cell.on_hide_unchanged_cells().listen(move |_| {
            let (Some(this), Some(placeholder_item), Some(vm)) = (weak.upgrade(), weak_placeholder.upgrade(), weak_cell.upgrade()) else { return };
            let Some(hidden_cell_view_models) = this.hidden_cells_of(&placeholder_item) else { return };
            let start = this.index_of(&DiffElementViewModel::Cell(vm));
            let placeholder_element = DiffElementViewModel::Placeholder(placeholder_item);
            splice_items(&mut this.items.borrow_mut(), start, hidden_cell_view_models.len(), vec![placeholder_element.clone()]);
            this.on_did_change_items.fire(INotebookDiffViewModelUpdateEvent {
                start,
                delete_count: hidden_cell_view_models.len(),
                elements: vec![placeholder_element],
                first_change_index: None,
            });
        }));
    }

    /// `this.placeholderAndRelatedCells.get(placeholderItem)`.
    fn hidden_cells_of(&self, placeholder_item: &Rc<DiffElementPlaceholderViewModel>) -> Option<Vec<Rc<DiffElementCellViewModelBase>>> {
        self.placeholder_and_related_cells.borrow().iter().find(|(candidate, _)| Rc::ptr_eq(candidate, placeholder_item)).map(|(_, hidden)| hidden.clone())
    }

    /// `this._items.indexOf(item)`.
    fn index_of(&self, item: &DiffElementViewModel) -> i64 {
        self.items.borrow().iter().position(|candidate| candidate == item).map_or(-1, |index| index as i64)
    }

    fn create_diff_view_models(self: &Rc<Self>, computed_cell_diffs: Vec<CellDiffInfo>, metadata_changed: bool) -> Promise<Vec<DiffElementViewModel>> {
        let this = self.clone();
        async_fn(async move {
            let original_model = this.model.original().notebook;
            let modified_model = this.model.modified().notebook;
            let transient_outputs = modified_model.transient_options().transient_outputs;
            let init_data = DiffElementInitData {
                metadata_status_height: if config_flag(&this.configuration_service.get_value(Some("notebook.diff.ignoreMetadata"), None)) { 0.0 } else { 25.0 },
                output_status_height: if config_flag(&this.configuration_service.get_value(Some("notebook.diff.ignoreOutputs"), None)) || transient_outputs { 0.0 } else { 25.0 },
                font_info: this.font_info.clone(),
            };

            let mut view_models: Vec<DiffElementViewModel> = Vec::new();
            let notebook_metadata_view_model = NotebookDocumentMetadataViewModel::new(
                this.model.original().notebook,
                this.model.modified().notebook,
                if metadata_changed { DiffElementType::ModifiedMetadata } else { DiffElementType::UnchangedMetadata },
                this.event_dispatcher.clone(),
                init_data.clone(),
                this.diff_editor_height_calculator.clone(),
            );
            this.base.register(notebook_metadata_view_model.clone());
            *this.notebook_metadata_view_model.borrow_mut() = Some(notebook_metadata_view_model.clone());
            if this.ignore_metadata.get() != Some(true) {
                if metadata_changed {
                    notebook_metadata_view_model.compute_heights().await?;
                }
                view_models.push(DiffElementViewModel::NotebookDocumentMetadata(notebook_metadata_view_model));
            }
            let original_cells: Vec<Rc<NotebookCellTextModel>> = original_model.cells();
            let modified_cells: Vec<Rc<NotebookCellTextModel>> = modified_model.cells();
            let promises: Vec<Promise<DiffElementViewModel>> = computed_cell_diffs
                .into_iter()
                .map(|diff| {
                    let this = this.clone();
                    let init_data = init_data.clone();
                    let original_model = original_model.clone();
                    let modified_model = modified_model.clone();
                    let original_cells = original_cells.clone();
                    let modified_cells = modified_cells.clone();
                    async_fn(async move {
                        Ok(match diff {
                            CellDiffInfo::Delete { original_cell_index } => DiffElementViewModel::Cell(DiffElementCellViewModelBase::new_single_side(
                                original_model,
                                modified_model,
                                Some(original_cells[original_cell_index].clone()),
                                None,
                                DiffElementType::Delete,
                                this.event_dispatcher.clone(),
                                init_data,
                                this.configuration_service.clone(),
                                this.diff_editor_height_calculator.clone(),
                                original_cell_index,
                            )),
                            CellDiffInfo::Insert { modified_cell_index } => DiffElementViewModel::Cell(DiffElementCellViewModelBase::new_single_side(
                                modified_model,
                                original_model,
                                None,
                                Some(modified_cells[modified_cell_index].clone()),
                                DiffElementType::Insert,
                                this.event_dispatcher.clone(),
                                init_data,
                                this.configuration_service.clone(),
                                this.diff_editor_height_calculator.clone(),
                                modified_cell_index,
                            )),
                            CellDiffInfo::Modified { original_cell_index, modified_cell_index } => {
                                let view_model = DiffElementCellViewModelBase::new_side_by_side(
                                    modified_model,
                                    original_model,
                                    original_cells[original_cell_index].clone(),
                                    modified_cells[modified_cell_index].clone(),
                                    DiffElementType::Modified,
                                    this.event_dispatcher.clone(),
                                    init_data,
                                    this.configuration_service.clone(),
                                    original_cell_index,
                                    this.diff_editor_height_calculator.clone(),
                                );
                                // Reduces flicker (compute this before setting the model)
                                // Else when the model is set, the height of the editor will be x, after diff is computed, then height will be y.
                                // & that results in flicker.
                                view_model.compute_editor_heights().await?;
                                DiffElementViewModel::Cell(view_model)
                            }
                            CellDiffInfo::Unchanged { original_cell_index, modified_cell_index } => DiffElementViewModel::Cell(DiffElementCellViewModelBase::new_side_by_side(
                                modified_model,
                                original_model,
                                original_cells[original_cell_index].clone(),
                                modified_cells[modified_cell_index].clone(),
                                DiffElementType::Unchanged,
                                this.event_dispatcher.clone(),
                                init_data,
                                this.configuration_service.clone(),
                                original_cell_index,
                                this.diff_editor_height_calculator.clone(),
                            )),
                        })
                    })
                })
                .collect();
            let cell_view_models = Promise::all(promises).await?;

            view_models.extend(cell_view_models);

            Ok(view_models)
        })
    }
}

impl INotebookDiffViewModel for NotebookDiffViewModel {
    fn items(&self) -> Vec<DiffElementViewModel> {
        self.items.borrow().clone()
    }

    fn on_did_change_items(&self) -> Event<INotebookDiffViewModelUpdateEvent> {
        self.on_did_change_items.event()
    }

    fn compute_diff(&self, token: CancellationToken) -> Promise<()> {
        let this = self.this.upgrade().expect("NotebookDiffViewModel");
        async_fn(async move {
            let original = this.model.original();
            let modified = this.model.modified();
            let diff_result = race_cancellation(&this.notebook_editor_worker_service.compute_diff(&original.resource, &modified.resource), &token).await?;
            let Some(mut diff_result) = diff_result.filter(|_| !token.is_cancellation_requested()) else {
                // after await the editor might be disposed.
                return Ok(());
            };

            pretty_changes(&original.notebook, &modified.notebook, &mut diff_result.cells_diff);

            let original_cells: Vec<Rc<dyn ICell>> = original.notebook.cells().into_iter().map(|cell| -> Rc<dyn ICell> { cell }).collect();
            let modified_cells: Vec<Rc<dyn ICell>> = modified.notebook.cells().into_iter().map(|cell| -> Rc<dyn ICell> { cell }).collect();
            let computed = compute_diff(&original_cells, &modified_cells, &diff_result);
            if is_equal(&computed.cell_diff_info, &this.original_cell_view_models.borrow(), &*this.model) {
                return Ok(());
            }
            race_cancellation(&this.update_view_models(computed.cell_diff_info, diff_result.metadata_changed, computed.first_change_index), &token).await?;
            if token.is_cancellation_requested() {
                return Ok(());
            }
            this.update_diff_editor_items();
            Ok(())
        })
    }
}

impl IDisposable for NotebookDiffViewModel {
    fn dispose(&self) {
        self.clear();
        self.base.dispose();
    }

    fn constructor_name(&self) -> &'static str {
        "NotebookDiffViewModel"
    }
}

/// making sure that swapping cells are always translated to `insert+delete`.
pub fn pretty_changes(original: &NotebookTextModel, modified: &NotebookTextModel, diff_result: &mut DiffResult) {
    let original_cells = original.cells();
    let modified_cells = modified.cells();
    let changes = &mut diff_result.changes;
    let mut i = 0;
    while i + 1 < changes.len() {
        // then we know there is another change after current one
        let curr = changes[i];
        let next = changes[i + 1];
        let x = curr.original_start;
        let y = curr.modified_start;

        if curr.original_length == 1
            && curr.modified_length == 0
            && next.original_start == x + 2
            && next.original_length == 0
            && next.modified_start == y + 1
            && next.modified_length == 1
            && CommonCell::get_hash_value(original_cells[x as usize].as_ref()) == CommonCell::get_hash_value(modified_cells[(y + 1) as usize].as_ref())
            && CommonCell::get_hash_value(original_cells[(x + 1) as usize].as_ref()) == CommonCell::get_hash_value(modified_cells[y as usize].as_ref())
        {
            // this is a swap
            changes[i].original_start = x;
            changes[i].original_length = 0;
            changes[i].modified_start = y;
            changes[i].modified_length = 1;

            changes[i + 1].original_start = x + 1;
            changes[i + 1].original_length = 1;
            changes[i + 1].modified_start = y + 2;
            changes[i + 1].modified_length = 0;

            i += 1;
        }
        i += 1;
    }
}

fn is_equal(cell_diff_info: &[CellDiffInfo], view_models: &[DiffElementViewModel], model: &dyn INotebookDiffEditorModel) -> bool {
    if cell_diff_info.len() != view_models.len() {
        return false;
    }
    let original_cells = model.original().notebook.cells();
    let modified_cells = model.modified().notebook.cells();
    for (a, b) in cell_diff_info.iter().zip(view_models.iter()) {
        let a_type = match a {
            CellDiffInfo::Unchanged { .. } => DiffElementType::Unchanged,
            CellDiffInfo::Modified { .. } => DiffElementType::Modified,
            CellDiffInfo::Delete { .. } => DiffElementType::Delete,
            CellDiffInfo::Insert { .. } => DiffElementType::Insert,
        };
        if a_type != b.type_() {
            return false;
        }
        let original_handle = b.original().map(|cell| cell.text_model.handle());
        let modified_handle = b.modified().map(|cell| cell.text_model.handle());
        match a {
            CellDiffInfo::Delete { original_cell_index } => {
                if Some(original_cells[*original_cell_index].handle()) != original_handle {
                    return false;
                }
            }
            CellDiffInfo::Insert { modified_cell_index } => {
                if Some(modified_cells[*modified_cell_index].handle()) != modified_handle {
                    return false;
                }
            }
            CellDiffInfo::Unchanged { original_cell_index, modified_cell_index } | CellDiffInfo::Modified { original_cell_index, modified_cell_index } => {
                if Some(original_cells[*original_cell_index].handle()) != original_handle {
                    return false;
                }
                if Some(modified_cells[*modified_cell_index].handle()) != modified_handle {
                    return false;
                }
            }
        }
    }

    true
}
