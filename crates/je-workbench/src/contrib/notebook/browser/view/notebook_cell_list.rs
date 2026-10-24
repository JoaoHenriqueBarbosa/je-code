//! Porte da camada lógica de `src/vs/workbench/contrib/notebook/browser/view/notebookCellList.ts`: a lista de células
//! (`NotebookCellList`), o mapa entre índices do modelo e da view com as áreas escondidas pela dobra, o foco e a seleção em
//! sincronia com o `NotebookViewModel`, e o `ListViewInfoAccessor`.
//!
//! Divergências:
//! - `extends WorkbenchList<CellViewModel>` (que é a `List` sobre um `ListView` de DOM) é a composição com uma lista lógica:
//!   os elementos visíveis, os traits de foco e de seleção (com o `splice` do `Trait` do original) e as alturas pelo
//!   [`NotebookCellsLayout`]. O widget visual, o `NotebookCellListView`, as view zones (`NotebookViewZones`), as sobreposições
//!   de célula (`NotebookCellOverlays`), o webview, as variáveis de contexto de cursor (`NOTEBOOK_EDITOR_CURSOR_BOUNDARY`), a
//!   âncora de rolagem (`NotebookCellAnchor`), o estilo, o foco do DOM e os `reveal*` (que rolam a view e esperam o editor de
//!   código) são Fase 6 e não entram.
//! - A atualização com `synchronous = false` (agendada no próximo quadro de animação pelo `DOM.scheduleAtNextAnimationFrame`)
//!   é aplicada na hora, porque a camada lógica não tem quadro de animação.
//! - `updateElementHeight2` troca o tamanho do item no layout sem ancoragem (a ancoragem é do `ListView`).
//! - `setFocus(indexes, browserEvent?, ignoreTextModelUpdate?)` e `setSelection` recebem `ignore_text_model_update: bool`; o
//!   `browserEvent` não existe. O `List.setFocus`/`setSelection` base valida os índices (`ListError`) e dispara
//!   `onDidChangeFocus`/`onDidChangeSelection` sempre, como o `Trait` do original.
//! - `getModelIndex`, `getModelIndex2`, `getViewIndex` e `getViewIndex2` devolvem `Option<i64>` (o `undefined` do original).
//! - `scrollTop` e `renderHeight` são estado lógico (`set_scroll_top` limita a `[0, scrollHeight - renderHeight]` como o
//!   `Scrollable`); `visibleRanges` só muda pelo setter, e `updateVisibleRanges` (disparado pelo scroll e pela altura do
//!   conteúdo do `ListView`) é [`NotebookCellList::update_visible_ranges`], chamado por `set_scroll_top`, `layout` e pelas
//!   mudanças de altura.

use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::rc::Rc;

use je_base::errors::{self, Error};
use je_base::event::{Emitter, Event};
use je_base::lifecycle::{Disposable, DisposableStore, IDisposable, MutableDisposable};
use je_editor::model::TrackedRangeStickiness;
use je_editor::prefix_sum_computer::PrefixSumComputer;
use je_widgets::list::range_map::{IItem, IRangeMap};

use super::notebook_cell_list_view::NotebookCellsLayout;
use crate::contrib::notebook::browser::view_model::cell_view_model_types::INotebookViewCellsUpdateEvent;
use crate::contrib::notebook::browser::view_model::notebook_view_model_impl::{CellViewModel, NotebookViewModel};
use crate::contrib::notebook::common::notebook_common::{CellKind, ISelectionHandleState, ISelectionIndexState, ISelectionState, diff};
use crate::contrib::notebook::common::notebook_range::{ICellRange, cell_ranges_equal, cell_ranges_to_indexes, reduce_cell_ranges};

/// `getVisibleCells(cells, hiddenRanges)`.
fn get_visible_cells(cells: &[CellViewModel], hidden_ranges: &[ICellRange]) -> Vec<CellViewModel> {
    if hidden_ranges.is_empty() {
        return cells.to_vec();
    }

    let mut start: usize = 0;
    let mut hidden_range_index = 0;
    let mut result = Vec::new();

    while start < cells.len() && hidden_range_index < hidden_ranges.len() {
        let hidden_start = hidden_ranges[hidden_range_index].start.max(0) as usize;
        if start < hidden_start {
            result.extend_from_slice(&cells[start..hidden_start.min(cells.len())]);
        }

        start = (hidden_ranges[hidden_range_index].end + 1).max(0) as usize;
        hidden_range_index += 1;
    }

    if start < cells.len() {
        result.extend_from_slice(&cells[start..]);
    }

    result
}

/// O evento de foco e de seleção da lista: os elementos e os índices.
#[derive(Clone)]
pub struct CellListEvent {
    pub elements: Vec<CellViewModel>,
    pub indexes: Vec<usize>,
}

/// `NotebookCellList`.
pub struct NotebookCellList {
    base: Rc<Disposable>,
    list_user: String,
    delegate: Box<dyn Fn(&CellViewModel) -> f64>,
    elements: RefCell<Vec<CellViewModel>>,
    focus: RefCell<Vec<usize>>,
    selection: RefCell<Vec<usize>>,
    layout: RefCell<NotebookCellsLayout>,
    scroll_top: Cell<f64>,
    render_height: Cell<f64>,

    on_did_change_focus: Rc<Emitter<CellListEvent>>,
    on_did_change_selection: Rc<Emitter<CellListEvent>>,
    on_did_remove_outputs: Rc<Emitter<usize>>,
    on_did_hide_outputs: Rc<Emitter<usize>>,
    on_did_remove_cells_from_view: Rc<Emitter<Vec<CellViewModel>>>,
    on_did_change_visible_ranges: Rc<Emitter<()>>,
    local_disposable_store: Rc<DisposableStore>,
    view_model_store: Rc<DisposableStore>,
    previous_focused_elements: RefCell<Vec<CellViewModel>>,
    previous_focus_listener: Rc<MutableDisposable<dyn IDisposable>>,

    view_model: RefCell<Option<Rc<NotebookViewModel>>>,
    hidden_range_ids: RefCell<Vec<String>>,
    hidden_ranges_prefix_sum: RefCell<Option<PrefixSumComputer>>,
    visible_ranges: RefCell<Vec<ICellRange>>,
    is_disposed: Cell<bool>,
}

impl NotebookCellList {
    /// `new NotebookCellList(listUser, container, notebookOptions, delegate, renderers, contextKeyService, options, ...)`: só o
    /// `listUser` e o `delegate.getHeight` (o `getTemplateId` e os renderizadores são da view).
    pub fn new(list_user: &str, get_height: impl Fn(&CellViewModel) -> f64 + 'static) -> Rc<Self> {
        let base = Disposable::new();
        let on_did_change_focus = Emitter::<CellListEvent>::new();
        let on_did_change_selection = Emitter::<CellListEvent>::new();
        let on_did_remove_outputs = Emitter::<usize>::new();
        let on_did_hide_outputs = Emitter::<usize>::new();
        let on_did_remove_cells_from_view = Emitter::<Vec<CellViewModel>>::new();
        let on_did_change_visible_ranges = Emitter::<()>::new();
        base.register(on_did_change_focus.clone());
        base.register(on_did_change_selection.clone());
        base.register(on_did_remove_outputs.clone());
        base.register(on_did_hide_outputs.clone());
        base.register(on_did_remove_cells_from_view.clone());
        base.register(on_did_change_visible_ranges.clone());
        let local_disposable_store = DisposableStore::new();
        let view_model_store = DisposableStore::new();
        base.register(local_disposable_store.clone());
        base.register(view_model_store.clone());
        let previous_focus_listener = MutableDisposable::<dyn IDisposable>::new();
        base.register(previous_focus_listener.clone());

        let this = Rc::new(Self {
            base,
            list_user: list_user.to_string(),
            delegate: Box::new(get_height),
            elements: RefCell::new(Vec::new()),
            focus: RefCell::new(Vec::new()),
            selection: RefCell::new(Vec::new()),
            layout: RefCell::new(NotebookCellsLayout::new(None)),
            scroll_top: Cell::new(0.0),
            render_height: Cell::new(0.0),
            on_did_change_focus,
            on_did_change_selection,
            on_did_remove_outputs,
            on_did_hide_outputs,
            on_did_remove_cells_from_view,
            on_did_change_visible_ranges,
            local_disposable_store,
            view_model_store,
            previous_focused_elements: RefCell::new(Vec::new()),
            previous_focus_listener,
            view_model: RefCell::new(None),
            hidden_range_ids: RefCell::new(Vec::new()),
            hidden_ranges_prefix_sum: RefCell::new(None),
            visible_ranges: RefCell::new(Vec::new()),
            is_disposed: Cell::new(false),
        });

        let weak = Rc::downgrade(&this);
        this.local_disposable_store.add(this.on_did_change_focus.event().listen(move |e| {
            let Some(this) = weak.upgrade() else { return };
            let previous = this.previous_focused_elements.borrow().clone();
            for element in previous {
                if !e.elements.contains(&element) {
                    element.on_deselect();
                }
            }
            *this.previous_focused_elements.borrow_mut() = e.elements.clone();
        }));

        this
    }

    pub fn on_did_change_focus(&self) -> Event<CellListEvent> {
        self.on_did_change_focus.event()
    }

    pub fn on_did_change_selection(&self) -> Event<CellListEvent> {
        self.on_did_change_selection.event()
    }

    pub fn on_did_remove_outputs(&self) -> Event<usize> {
        self.on_did_remove_outputs.event()
    }

    pub fn on_did_hide_outputs(&self) -> Event<usize> {
        self.on_did_hide_outputs.event()
    }

    pub fn on_did_remove_cells_from_view(&self) -> Event<Vec<CellViewModel>> {
        self.on_did_remove_cells_from_view.event()
    }

    pub fn on_did_change_visible_ranges(&self) -> Event<()> {
        self.on_did_change_visible_ranges.event()
    }

    pub fn view_model(&self) -> Option<Rc<NotebookViewModel>> {
        self.view_model.borrow().clone()
    }

    pub fn visible_ranges(&self) -> Vec<ICellRange> {
        self.visible_ranges.borrow().clone()
    }

    pub fn set_visible_ranges(&self, ranges: Vec<ICellRange>) {
        if cell_ranges_equal(&self.visible_ranges.borrow(), &ranges) {
            return;
        }

        *self.visible_ranges.borrow_mut() = ranges;
        self.on_did_change_visible_ranges.fire(());
    }

    pub fn is_disposed(&self) -> bool {
        self.is_disposed.get()
    }

    //#region List base (`List`)
    pub fn length(&self) -> usize {
        self.elements.borrow().len()
    }

    pub fn element(&self, index: usize) -> CellViewModel {
        self.elements.borrow()[index].clone()
    }

    pub fn index_of(&self, element: &CellViewModel) -> i64 {
        self.elements.borrow().iter().position(|candidate| candidate == element).map(|index| index as i64).unwrap_or(-1)
    }

    pub fn get_focus(&self) -> Vec<usize> {
        self.focus.borrow().clone()
    }

    pub fn get_focused_elements(&self) -> Vec<CellViewModel> {
        self.focus.borrow().iter().map(|index| self.element(*index)).collect()
    }

    pub fn get_selection(&self) -> Vec<usize> {
        self.selection.borrow().clone()
    }

    pub fn get_selected_elements(&self) -> Vec<CellViewModel> {
        self.selection.borrow().iter().map(|index| self.element(*index)).collect()
    }

    fn validate_indexes(&self, indexes: &[usize]) {
        for index in indexes {
            if *index >= self.length() {
                errors::throw(Error::new(format!("[{}] Invalid index {}", self.list_user, index)));
            }
        }
    }

    fn base_set_focus(&self, indexes: &[usize]) {
        self.validate_indexes(indexes);
        *self.focus.borrow_mut() = indexes.to_vec();
        self.on_did_change_focus.fire(CellListEvent { elements: self.get_focused_elements(), indexes: indexes.to_vec() });
    }

    fn base_set_selection(&self, indexes: &[usize]) {
        self.validate_indexes(indexes);
        *self.selection.borrow_mut() = indexes.to_vec();
        self.on_did_change_selection.fire(CellListEvent { elements: self.get_selected_elements(), indexes: indexes.to_vec() });
    }

    /// `findNextIndex(index, loop, filter)`.
    fn find_next_index(&self, index: usize, loop_: bool, filter: Option<&dyn Fn(&CellViewModel) -> bool>) -> i64 {
        let mut index = index;
        for _ in 0..self.length() {
            if index >= self.length() && !loop_ {
                return -1;
            }

            index %= self.length();

            if filter.is_none_or(|filter| filter(&self.element(index))) {
                return index as i64;
            }

            index += 1;
        }

        -1
    }

    pub fn focus_next(&self, n: usize, loop_: bool, filter: Option<&dyn Fn(&CellViewModel) -> bool>) {
        if self.length() == 0 {
            return;
        }

        let focus = self.get_focus();
        let index = self.find_next_index(if !focus.is_empty() { focus[0] + n } else { 0 }, loop_, filter);

        if index > -1 {
            self.set_focus(&[index as usize], false);
        }
    }

    pub fn focus_previous(&self, n: usize, loop_: bool, filter: Option<&dyn Fn(&CellViewModel) -> bool>) {
        if self.length() == 0 {
            return;
        }

        let focus = self.get_focus();
        let length = self.length() as i64;
        let mut index = if !focus.is_empty() { focus[0] as i64 - n as i64 } else { 0 };
        for _ in 0..length {
            if index < 0 && !loop_ {
                return;
            }
            index = ((index % length) + length) % length;
            if filter.is_none_or(|filter| filter(&self.element(index as usize))) {
                self.set_focus(&[index as usize], false);
                return;
            }
            index -= 1;
        }
    }

    /// O `splice` do `Trait` do original: índices antes do trecho ficam, os do trecho saem e os depois se deslocam.
    fn splice_trait(indexes: &[usize], start: usize, delete_count: usize, elements_count: usize) -> Vec<usize> {
        let end = start + delete_count;
        let mut result = Vec::new();
        for index in indexes {
            if *index < start {
                result.push(*index);
            } else if *index >= end {
                result.push(index + elements_count - delete_count);
            }
        }
        result
    }

    /// `List.splice(start, deleteCount, elements)`.
    fn base_splice(&self, start: usize, delete_count: usize, elements: &[CellViewModel]) {
        if start > self.length() {
            errors::throw(Error::new(format!("[{}] Invalid start index: {}", self.list_user, start)));
        }
        let delete_count = delete_count.min(self.length() - start);

        let new_focus = Self::splice_trait(&self.focus.borrow(), start, delete_count, elements.len());
        let new_selection = Self::splice_trait(&self.selection.borrow(), start, delete_count, elements.len());
        let focus_changed = new_focus != *self.focus.borrow();
        let selection_changed = new_selection != *self.selection.borrow();

        self.elements.borrow_mut().splice(start..start + delete_count, elements.iter().cloned());
        let items: Vec<IItem> = elements.iter().map(|element| IItem { size: (self.delegate)(element) }).collect();
        self.layout.borrow_mut().splice(start, delete_count, &items);

        *self.focus.borrow_mut() = new_focus.clone();
        *self.selection.borrow_mut() = new_selection.clone();
        if focus_changed {
            self.on_did_change_focus.fire(CellListEvent { elements: self.get_focused_elements(), indexes: new_focus });
        }
        if selection_changed {
            self.on_did_change_selection.fire(CellListEvent { elements: self.get_selected_elements(), indexes: new_selection });
        }
        self.update_visible_ranges();
    }
    //#endregion

    //#region Layout (`ListView`)
    pub fn get_render_height(&self) -> f64 {
        self.render_height.get()
    }

    pub fn get_scroll_height(&self) -> f64 {
        self.layout.borrow().size()
    }

    pub fn scroll_top(&self) -> f64 {
        self.scroll_top.get()
    }

    pub fn set_scroll_top(&self, scroll_top: f64) {
        let max = (self.get_scroll_height() - self.render_height.get()).max(0.0);
        self.scroll_top.set(scroll_top.clamp(0.0, max));
        self.update_visible_ranges();
    }

    /// `layout(height?, width?)`.
    pub fn layout(&self, height: Option<f64>) {
        if let Some(height) = height {
            self.render_height.set(height);
        }
        self.update_visible_ranges();
    }

    pub fn element_top(&self, index: usize) -> f64 {
        self.layout.borrow().position_at(index as i64)
    }

    pub fn element_height(&self, element: &CellViewModel) -> f64 {
        let index = self.get_view_index_upper_bound(element);
        if index < 0 || index as usize >= self.length() {
            errors::throw(Error::new(format!("[{}] Invalid index {}", self.list_user, index)));
        }

        self.view_element_height(index as usize)
    }

    fn view_element_height(&self, index: usize) -> f64 {
        let layout = self.layout.borrow();
        layout.position_at(index as i64 + 1) - layout.position_at(index as i64)
    }

    pub fn element_at(&self, position: f64) -> Option<CellViewModel> {
        if self.length() == 0 {
            return None;
        }

        let idx = self.layout.borrow().index_at(position);
        Some(self.element(idx.clamp(0, self.length() as i64 - 1) as usize))
    }

    pub fn get_view_scroll_top(&self) -> f64 {
        self.scroll_top.get()
    }

    pub fn get_view_scroll_bottom(&self) -> f64 {
        self.get_view_scroll_top() + self.render_height.get()
    }

    pub fn get_cell_view_scroll_top(&self, cell: &CellViewModel) -> f64 {
        let index = self.get_view_index_upper_bound(cell);
        if index < 0 || index as usize >= self.length() {
            errors::throw(Error::new(format!("[{}] Invalid index {}", self.list_user, index)));
        }

        self.element_top(index as usize)
    }

    pub fn get_cell_view_scroll_bottom(&self, cell: &CellViewModel) -> f64 {
        let index = self.get_view_index_upper_bound(cell);
        if index < 0 || index as usize >= self.length() {
            errors::throw(Error::new(format!("[{}] Invalid index {}", self.list_user, index)));
        }

        self.element_top(index as usize) + self.view_element_height(index as usize)
    }

    pub fn update_element_height2(&self, element: &CellViewModel, size: f64) {
        let index = self.get_view_index_upper_bound(element);
        if index < 0 || index as usize >= self.length() {
            return;
        }

        self.layout.borrow_mut().splice(index as usize, 1, &[IItem { size }]);
        self.update_visible_ranges();
    }

    /// O `updateVisibleRanges` do construtor.
    fn update_visible_ranges(&self) {
        let Some(view_model) = self.view_model() else { return };
        if self.length() == 0 {
            return;
        }

        let top = self.get_view_scroll_top();
        let bottom = self.get_view_scroll_bottom();
        if top >= bottom {
            return;
        }

        let last = self.length() as i64 - 1;
        let top_view_index = self.layout.borrow().index_at(top).clamp(0, last);
        let top_element = self.element(top_view_index as usize);
        let top_model_index = view_model.get_cell_index(&top_element);
        let bottom_view_index = self.layout.borrow().index_at(bottom).clamp(0, last);
        let bottom_element = self.element(bottom_view_index as usize);
        let bottom_model_index = view_model.get_cell_index(&bottom_element);

        if bottom_model_index - top_model_index == bottom_view_index - top_view_index {
            self.set_visible_ranges(vec![ICellRange { start: top_model_index, end: bottom_model_index + 1 }]);
        } else {
            self.set_visible_ranges(self.get_visible_ranges_from_index(top_view_index, top_model_index, bottom_view_index, bottom_model_index));
        }
    }
    //#endregion

    pub fn detach_view_model(&self) {
        self.view_model_store.clear();
        *self.view_model.borrow_mut() = None;
        *self.hidden_ranges_prefix_sum.borrow_mut() = None;
    }

    pub fn attach_view_model(self: &Rc<Self>, model: Rc<NotebookViewModel>) {
        *self.view_model.borrow_mut() = Some(model.clone());
        {
            let weak = Rc::downgrade(self);
            self.view_model_store.add(model.on_did_change_view_cells().listen(move |e: &INotebookViewCellsUpdateEvent| {
                let Some(this) = weak.upgrade() else { return };
                if this.is_disposed.get() {
                    return;
                }
                let Some(view_model) = this.view_model() else { return };

                let current_ranges: Vec<ICellRange> = this.hidden_range_ids.borrow().iter().filter_map(|id| view_model.get_tracked_range(id)).collect();
                let new_visible_view_cells = get_visible_cells(&view_model.view_cells(), &current_ranges);

                let old_visible_view_cells: Vec<CellViewModel> = this.elements.borrow().clone();
                let old_view_cell_mapping: HashSet<String> = old_visible_view_cells.iter().map(|cell| cell.uri().to_string(false).to_string_lossy()).collect();

                let view_diffs = diff(&old_visible_view_cells, &new_visible_view_cells, |a| old_view_cell_mapping.contains(&a.uri().to_string(false).to_string_lossy()), None);

                // `e.synchronous` falso é aplicado na hora (ver as divergências).
                this.update_elements_in_webview(view_diffs.into_iter().map(|d| (d.start, d.delete_count, d.to_insert)).collect());
            }));
        }

        {
            let weak = Rc::downgrade(self);
            self.view_model_store.add(model.on_did_change_selection().listen(move |e| {
                let Some(this) = weak.upgrade() else { return };
                if e == "view" {
                    return;
                }
                let Some(model) = this.view_model() else { return };

                // convert model selections to view selections
                let view_selections: Vec<usize> = cell_ranges_to_indexes(&model.get_selections())
                    .into_iter()
                    .filter_map(|index| model.cell_at(index))
                    .map(|cell| this.get_view_index_upper_bound(&cell))
                    .filter(|index| *index >= 0)
                    .map(|index| index as usize)
                    .collect();
                this.set_selection(&view_selections, true);
                let primary: Vec<usize> = cell_ranges_to_indexes(&[model.get_focus()])
                    .into_iter()
                    .filter_map(|index| model.cell_at(index))
                    .map(|cell| this.get_view_index_upper_bound(&cell))
                    .filter(|index| *index >= 0)
                    .map(|index| index as usize)
                    .collect();

                if !primary.is_empty() {
                    this.set_focus(&primary, true);
                }
            }));
        }

        let hidden_ranges = model.get_hidden_ranges();
        self.set_hidden_areas(&hidden_ranges, false);
        let new_ranges = reduce_cell_ranges(&hidden_ranges);
        let mut view_cells = model.view_cells();
        for range in new_ranges.iter().rev() {
            let start = (range.start.max(0) as usize).min(view_cells.len());
            let end = ((range.end + 1).max(0) as usize).min(view_cells.len()).max(start);
            let removed_cells: Vec<CellViewModel> = view_cells.splice(start..end, std::iter::empty()).collect();
            self.on_did_remove_cells_from_view.fire(removed_cells);
        }

        self.splice2(0, 0, &view_cells);
    }

    fn update_elements_in_webview(&self, mut view_diffs: Vec<(usize, usize, Vec<CellViewModel>)>) {
        view_diffs.reverse();
        for diff in view_diffs {
            let mut hidden_outputs = 0;
            let mut deleted_outputs = 0;
            let mut removed_markdown_cells: Vec<CellViewModel> = Vec::new();

            let view_model = self.view_model();
            for i in diff.0..diff.0 + diff.1 {
                let cell = self.element(i);
                match &cell {
                    CellViewModel::Code(code) => {
                        if view_model.as_ref().is_some_and(|view_model| view_model.has_cell(&cell)) {
                            hidden_outputs += code.outputs_view_models().len();
                        } else {
                            deleted_outputs += code.outputs_view_models().len();
                        }
                    }
                    CellViewModel::Markup(_) => removed_markdown_cells.push(cell.clone()),
                }
            }

            self.splice2(diff.0, diff.1, &diff.2);

            self.on_did_hide_outputs.fire(hidden_outputs);
            self.on_did_remove_outputs.fire(deleted_outputs);
            self.on_did_remove_cells_from_view.fire(removed_markdown_cells);
        }
    }

    pub fn clear(&self) {
        self.base_splice(0, self.length(), &[]);
    }

    pub fn set_hidden_areas(&self, ranges: &[ICellRange], trigger_view_update: bool) -> bool {
        let Some(view_model) = self.view_model() else { return false };

        let new_ranges = reduce_cell_ranges(ranges);
        // delete old tracking ranges
        let old_ranges: Vec<ICellRange> = self.hidden_range_ids.borrow().iter().filter_map(|id| view_model.get_tracked_range(id)).collect();
        if new_ranges.len() == old_ranges.len() {
            let has_difference = new_ranges.iter().zip(old_ranges.iter()).any(|(new, old)| !(new.start == old.start && new.end == old.end));

            if !has_difference {
                // they call 'setHiddenAreas' for a reason, even if the ranges are still the same, it's possible that the hiddenRangeSum is not update to date
                self.update_hidden_range_prefix_sum(&new_ranges);
                return false;
            }
        }

        for id in self.hidden_range_ids.borrow().iter() {
            view_model.set_tracked_range(Some(id.as_str()), None, TrackedRangeStickiness::GrowsOnlyWhenTypingAfter);
        }
        let hidden_area_ids: Vec<String> =
            new_ranges.iter().filter_map(|range| view_model.set_tracked_range(None, Some(*range), TrackedRangeStickiness::GrowsOnlyWhenTypingAfter)).collect();

        *self.hidden_range_ids.borrow_mut() = hidden_area_ids;

        // set hidden ranges prefix sum
        self.update_hidden_range_prefix_sum(&new_ranges);

        if trigger_view_update {
            self.update_hidden_areas_in_view(&old_ranges, &new_ranges);
        }

        true
    }

    fn update_hidden_range_prefix_sum(&self, new_ranges: &[ICellRange]) {
        let view_model_length = self.view_model().map(|view_model| view_model.length()).unwrap_or(0) as i64;
        let mut start: i64 = 0;
        let mut index = 0;
        let mut ret: Vec<u32> = Vec::new();

        while index < new_ranges.len() {
            let mut j = start;
            while j < new_ranges[index].start - 1 {
                ret.push(1);
                j += 1;
            }

            ret.push((new_ranges[index].end - new_ranges[index].start + 1 + 1) as u32);
            start = new_ranges[index].end + 1;
            index += 1;
        }

        let mut i = start;
        while i < view_model_length {
            ret.push(1);
            i += 1;
        }

        *self.hidden_ranges_prefix_sum.borrow_mut() = Some(PrefixSumComputer::new(ret));
    }

    /// oldRanges and newRanges are all reduced and sorted.
    pub fn update_hidden_areas_in_view(&self, old_ranges: &[ICellRange], new_ranges: &[ICellRange]) {
        let Some(view_model) = self.view_model() else { return };
        let view_cells = view_model.view_cells();
        let old_view_cell_entries = get_visible_cells(&view_cells, old_ranges);
        let old_view_cell_mapping: HashSet<String> = old_view_cell_entries.iter().map(|cell| cell.uri().to_string(false).to_string_lossy()).collect();

        let new_view_cell_entries = get_visible_cells(&view_cells, new_ranges);

        let view_diffs = diff(&old_view_cell_entries, &new_view_cell_entries, |a| old_view_cell_mapping.contains(&a.uri().to_string(false).to_string_lossy()), None);

        self.update_elements_in_webview(view_diffs.into_iter().map(|d| (d.start, d.delete_count, d.to_insert)).collect());
    }

    pub fn splice2(&self, start: usize, delete_count: usize, elements: &[CellViewModel]) {
        // we need to convert start and delete count based on hidden ranges
        if start > self.length() {
            return;
        }

        self.base_splice(start, delete_count, elements);

        let Some(view_model) = self.view_model() else { return };
        let selections_left = self.get_selected_elements().iter().filter(|el| view_model.has_cell(el)).count();

        if selections_left == 0 && !view_model.view_cells().is_empty() {
            // after splice, the selected cells are deleted
            view_model.update_selections_state(
                &ISelectionState::Index(ISelectionIndexState { focus: ICellRange { start: 0, end: 1 }, selections: vec![ICellRange { start: 0, end: 1 }] }),
                "model",
            );
        }
    }

    pub fn get_model_index(&self, cell: &CellViewModel) -> Option<i64> {
        let view_index = self.index_of(cell);
        self.get_model_index2(view_index)
    }

    pub fn get_model_index2(&self, view_index: i64) -> Option<i64> {
        let sum = self.hidden_ranges_prefix_sum.borrow();
        let Some(sum) = sum.as_ref() else { return Some(view_index) };

        Some(sum.get_prefix_sum(view_index - 1) as i64)
    }

    pub fn get_view_index(&self, cell: &CellViewModel) -> Option<i64> {
        let model_index = self.view_model()?.get_cell_index(cell);
        self.get_view_index2(model_index)
    }

    pub fn get_view_index2(&self, model_index: i64) -> Option<i64> {
        let sum = self.hidden_ranges_prefix_sum.borrow();
        let Some(sum) = sum.as_ref() else { return Some(model_index) };

        let view_index_info = sum.get_index_of(model_index);

        if view_index_info.remainder != 0 {
            if model_index >= sum.get_total_sum() as i64 {
                // it's already after the last hidden range
                return Some(model_index - (sum.get_total_sum() as i64 - sum.get_count() as i64));
            }
            None
        } else {
            Some(view_index_info.index as i64)
        }
    }

    pub fn convert_model_index_to_view_index(&self, model_index: i64) -> i64 {
        let sum = self.hidden_ranges_prefix_sum.borrow();
        let Some(sum) = sum.as_ref() else { return model_index };

        if model_index >= sum.get_total_sum() as i64 {
            // it's already after the last hidden range
            return (self.length() as i64).min(sum.get_total_sum() as i64);
        }

        sum.get_index_of(model_index).index as i64
    }

    pub fn model_index_is_visible(&self, model_index: i64) -> bool {
        let sum = self.hidden_ranges_prefix_sum.borrow();
        let Some(sum) = sum.as_ref() else { return true };

        let view_index_info = sum.get_index_of(model_index);
        if view_index_info.remainder != 0 {
            // it's already after the last hidden range
            model_index >= sum.get_total_sum() as i64
        } else {
            true
        }
    }

    fn get_visible_ranges_from_index(&self, top_view_index: i64, top_model_index: i64, bottom_view_index: i64, _bottom_model_index: i64) -> Vec<ICellRange> {
        let mut stack: Vec<i64> = Vec::new();
        let mut ranges: Vec<ICellRange> = Vec::new();
        // there are hidden ranges
        let mut index = top_view_index;
        let mut model_index = top_model_index;

        let sum = self.hidden_ranges_prefix_sum.borrow();
        let sum = sum.as_ref().expect("hiddenRangesPrefixSum");
        while index <= bottom_view_index {
            let accu = sum.get_prefix_sum(index) as i64;
            if accu == model_index + 1 {
                // no hidden area after it
                if let Some(last) = stack.last() {
                    if *last == model_index - 1 {
                        ranges.push(ICellRange { start: *last, end: model_index + 1 });
                    } else {
                        ranges.push(ICellRange { start: *last, end: *last + 1 });
                    }
                }

                stack.push(model_index);
                index += 1;
                model_index += 1;
            } else {
                // there are hidden ranges after it
                if let Some(last) = stack.last() {
                    if *last == model_index - 1 {
                        ranges.push(ICellRange { start: *last, end: model_index + 1 });
                    } else {
                        ranges.push(ICellRange { start: *last, end: *last + 1 });
                    }
                }

                stack.push(model_index);
                index += 1;
                model_index = accu;
            }
        }

        if let Some(last) = stack.last() {
            ranges.push(ICellRange { start: *last, end: *last + 1 });
        }

        reduce_cell_ranges(&ranges)
    }

    pub fn get_visible_ranges_plus_viewport_above_and_below(&self) -> Vec<ICellRange> {
        let Some(view_model) = self.view_model() else { return Vec::new() };
        if self.length() == 0 {
            return Vec::new();
        }

        let render_height = self.render_height.get();
        let top = (self.get_view_scroll_top() - render_height).max(0.0);
        let top_view_index = self.layout.borrow().index_at(top).max(0);
        let top_element = self.element((top_view_index as usize).min(self.length() - 1));
        let top_model_index = view_model.get_cell_index(&top_element);
        let bottom = (self.get_view_scroll_bottom() + render_height).clamp(0.0, self.get_scroll_height());
        let bottom_view_index = self.layout.borrow().index_at(bottom).clamp(0, self.length() as i64 - 1);
        let bottom_element = self.element(bottom_view_index as usize);
        let bottom_model_index = view_model.get_cell_index(&bottom_element);

        if bottom_model_index - top_model_index == bottom_view_index - top_view_index {
            vec![ICellRange { start: top_model_index, end: bottom_model_index }]
        } else {
            self.get_visible_ranges_from_index(top_view_index, top_model_index, bottom_view_index, bottom_model_index)
        }
    }

    fn get_view_index_upper_bound(&self, cell: &CellViewModel) -> i64 {
        let Some(view_model) = self.view_model() else { return -1 };

        let model_index = view_model.get_cell_index(cell);
        if model_index == -1 {
            return -1;
        }

        self.get_view_index_upper_bound2(model_index)
    }

    fn get_view_index_upper_bound2(&self, model_index: i64) -> i64 {
        let sum = self.hidden_ranges_prefix_sum.borrow();
        let Some(sum) = sum.as_ref() else { return model_index };

        let view_index_info = sum.get_index_of(model_index);

        if view_index_info.remainder != 0 && model_index >= sum.get_total_sum() as i64 {
            return model_index - (sum.get_total_sum() as i64 - sum.get_count() as i64);
        }

        view_index_info.index as i64
    }

    pub fn focus_element(&self, cell: &CellViewModel) {
        let index = self.get_view_index_upper_bound(cell);

        if index >= 0
            && let Some(view_model) = self.view_model()
        {
            // update view model first, which will update both `focus` and `selection` in a single transaction
            let focused_element_handle = self.element(index as usize).handle();
            view_model.update_selections_state(&ISelectionState::Handle(ISelectionHandleState { primary: Some(focused_element_handle), selections: vec![focused_element_handle] }), "view");

            // update the view as previous model update will not trigger event
            self.set_focus(&[index as usize], false);
        }
    }

    pub fn select_elements(&self, elements: &[CellViewModel]) {
        let indices: Vec<usize> = elements.iter().map(|cell| self.get_view_index_upper_bound(cell)).filter(|index| *index >= 0).map(|index| index as usize).collect();
        self.set_selection(&indices, false);
    }

    /// `setFocus(indexes, browserEvent?, ignoreTextModelUpdate?)`.
    pub fn set_focus(&self, indexes: &[usize], ignore_text_model_update: bool) {
        if ignore_text_model_update {
            self.base_set_focus(indexes);
            return;
        }

        if indexes.is_empty() {
            if let Some(view_model) = self.view_model() {
                if self.length() > 0 {
                    // Don't allow clearing focus, #121129
                    return;
                }

                view_model.update_selections_state(&ISelectionState::Handle(ISelectionHandleState { primary: None, selections: Vec::new() }), "view");
            }
        } else if let Some(view_model) = self.view_model() {
            let focused_element_handle = self.element(indexes[0]).handle();
            view_model.update_selections_state(
                &ISelectionState::Handle(ISelectionHandleState {
                    primary: Some(focused_element_handle),
                    selections: self.get_selection().iter().map(|selection| self.element(*selection).handle()).collect(),
                }),
                "view",
            );
        }

        self.base_set_focus(indexes);
    }

    /// `setSelection(indexes, browserEvent?, ignoreTextModelUpdate?)`.
    pub fn set_selection(&self, indexes: &[usize], ignore_text_model_update: bool) {
        if ignore_text_model_update {
            self.base_set_selection(indexes);
            return;
        }

        let primary = self.get_focused_elements().first().map(|element| element.handle());
        if indexes.is_empty() {
            if let Some(view_model) = self.view_model() {
                view_model.update_selections_state(&ISelectionState::Handle(ISelectionHandleState { primary, selections: Vec::new() }), "view");
            }
        } else if let Some(view_model) = self.view_model() {
            view_model.update_selections_state(
                &ISelectionState::Handle(ISelectionHandleState { primary, selections: indexes.iter().map(|index| self.element(*index).handle()).collect() }),
                "view",
            );
        }

        self.base_set_selection(indexes);
    }

    pub fn focus_container(&self, clear_selection: bool) {
        if clear_selection {
            // allow focus to be between cells
            if let Some(view_model) = self.view_model() {
                view_model.update_selections_state(&ISelectionState::Handle(ISelectionHandleState { primary: None, selections: Vec::new() }), "view");
            }
            self.set_focus(&[], true);
            self.set_selection(&[], true);
        }
    }

    pub fn dispose(&self) {
        self.is_disposed.set(true);
        self.view_model_store.dispose();
        self.local_disposable_store.dispose();
        self.base.dispose();

        // un-ref
        self.previous_focused_elements.borrow_mut().clear();
        *self.view_model.borrow_mut() = None;
        self.hidden_range_ids.borrow_mut().clear();
        *self.hidden_ranges_prefix_sum.borrow_mut() = None;
        self.visible_ranges.borrow_mut().clear();
        self.previous_focus_listener.clear();
    }
}

impl IDisposable for NotebookCellList {
    fn dispose(&self) {
        NotebookCellList::dispose(self);
    }

    fn constructor_name(&self) -> &'static str {
        "NotebookCellList"
    }
}

/// `ListViewInfoAccessor`.
pub struct ListViewInfoAccessor {
    pub list: Rc<NotebookCellList>,
}

impl ListViewInfoAccessor {
    pub fn new(list: Rc<NotebookCellList>) -> Self {
        Self { list }
    }

    pub fn get_view_index(&self, cell: &CellViewModel) -> i64 {
        self.list.get_view_index(cell).unwrap_or(-1)
    }

    pub fn get_view_height(&self, cell: &CellViewModel) -> f64 {
        if self.list.view_model().is_none() {
            return -1.0;
        }

        self.list.element_height(cell)
    }

    pub fn get_cell_range_from_view_range(&self, start_index: i64, end_index: i64) -> Option<ICellRange> {
        let view_model = self.list.view_model()?;

        let Some(model_index) = self.list.get_model_index2(start_index) else {
            errors::throw(Error::new(format!("startIndex {start_index} out of boundary")))
        };

        if end_index >= self.list.length() as i64 {
            // it's the end
            let end_model_index = view_model.length() as i64;
            Some(ICellRange { start: model_index, end: end_model_index })
        } else {
            let Some(end_model_index) = self.list.get_model_index2(end_index) else {
                errors::throw(Error::new(format!("endIndex {end_index} out of boundary")))
            };
            Some(ICellRange { start: model_index, end: end_model_index })
        }
    }

    pub fn get_cells_from_view_range(&self, start_index: i64, end_index: i64) -> Vec<CellViewModel> {
        let Some(view_model) = self.list.view_model() else { return Vec::new() };

        let Some(range) = self.get_cell_range_from_view_range(start_index, end_index) else { return Vec::new() };

        view_model.get_cells_in_range(Some(range))
    }

    pub fn get_cells_in_range(&self, range: Option<ICellRange>) -> Vec<CellViewModel> {
        self.list.view_model().map(|view_model| view_model.get_cells_in_range(range)).unwrap_or_default()
    }

    pub fn get_visible_ranges_plus_viewport_above_and_below(&self) -> Vec<ICellRange> {
        self.list.get_visible_ranges_plus_viewport_above_and_below()
    }
}
