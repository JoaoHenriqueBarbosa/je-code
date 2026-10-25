//! Porte da camada lógica de `src/vs/workbench/contrib/notebook/browser/view/notebookCellList.ts`: a lista de células
//! (`NotebookCellList`), o mapa entre índices do modelo e da view com as áreas escondidas pela dobra, o foco e a seleção em
//! sincronia com o `NotebookViewModel`, e o `ListViewInfoAccessor`.
//!
//! Divergências:
//! - `extends WorkbenchList<CellViewModel>` (que é a `List` sobre um `ListView` de DOM) é a composição com uma lista lógica:
//!   os elementos visíveis, os traits de foco e de seleção (com o `splice` do `Trait` do original) e as alturas pelo
//!   [`NotebookCellsLayout`]. O widget visual, o `NotebookCellListView`, as sobreposições
//!   de célula (`NotebookCellOverlays`), o webview, as variáveis de contexto de cursor (`NOTEBOOK_EDITOR_CURSOR_BOUNDARY`), a
//!   âncora de rolagem (`NotebookCellAnchor`), o estilo, o foco do DOM e os `reveal*` (que rolam a view e esperam o editor de
//!   código) são Fase 6 e não entram. O `NotebookCellAnchor` entra ([`super::notebook_cell_anchor`]), com o
//!   `onDidScroll` da lista.
//! - A rolagem lógica reproduz as duas dimensões do `ListView`: o `_scrollHeight` (altura do conteúdo mais o
//!   `paddingBottom`, atualizado a cada `splice` e a cada mudança de altura) e a altura que o `Scrollable` conhece, que só
//!   alcança o `_scrollHeight` no quadro de animação seguinte (`eventuallyUpdateScrollDimensions`). Sem quadro de
//!   animação, a alcança no próximo `setScrollTop` ou `layout`, que é o que o original faz ao cancelar o quadro
//!   agendado. É isso que limita a rolagem logo depois de a célula crescer (o `updateElementHeight` rola antes de
//!   atualizar as dimensões). O `forceIntegerValues` do `Scrollable` do `ListView` trunca a posição de rolagem.
//! - `updateElementHeight(index, size, anchorIndex)` do `ListView` é [`NotebookCellList::update_element_height`]; o
//!   `_rerender` que ele chama depois mede a altura dinâmica dos itens no DOM, o que a camada lógica não tem.
//!   `revealCells` é portado; `revealCell` e os `revealRange*` (que esperam o editor de código) não.
//! - `if (focus)` do `updateElementHeight2` é falso para o índice de foco 0 (o `0` do JavaScript), e o porte mantém isso.
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
use je_base::scrollable::ScrollEvent;
use je_editor::model::TrackedRangeStickiness;
use je_editor::prefix_sum_computer::PrefixSumComputer;
use je_platform::configuration::service::IConfigurationService;
use je_widgets::list::range_map::{IItem, IRangeMap};

use super::notebook_cell_anchor::{ICellListView, NotebookCellAnchor};
use super::notebook_cell_list_view::NotebookCellsLayout;
use crate::contrib::notebook::browser::notebook_options::NotebookOptions;
use crate::contrib::notebook::browser::view_parts::notebook_view_zones::{INotebookViewZoneChangeAccessor, NotebookViewZones};
use crate::contrib::notebook::browser::view_model::cell_view_model_types::{CellFocusMode, INotebookViewCellsUpdateEvent};
use crate::contrib::notebook::browser::view_model::notebook_view_model_impl::{CellViewModel, NotebookViewModel};
use crate::contrib::notebook::common::notebook_common::{CellKind, ISelectionHandleState, ISelectionIndexState, ISelectionState, diff};
use crate::contrib::notebook::common::notebook_execution_state_service::INotebookExecutionStateService;
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

/// `CellRevealPosition`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CellRevealPosition {
    Top,
    Center,
    Bottom,
    NearTop,
}

/// `IWorkbenchListOptionsUpdate` (o `IListViewOptionsUpdate` do `ListView`): só o `paddingBottom`, a única opção de
/// layout que a lista lógica guarda.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CellListOptionsUpdate {
    pub padding_bottom: Option<f64>,
}

/// `NotebookCellList`.
pub struct NotebookCellList {
    base: Rc<Disposable>,
    list_user: String,
    notebook_options: Rc<NotebookOptions>,
    delegate: Box<dyn Fn(&CellViewModel) -> f64>,
    elements: RefCell<Vec<CellViewModel>>,
    focus: RefCell<Vec<usize>>,
    selection: RefCell<Vec<usize>>,
    layout: RefCell<NotebookCellsLayout>,
    /// A posição do `Scrollable`.
    scroll_top: Cell<f64>,
    /// `lastRenderTop` do `ListView`.
    last_render_top: Cell<f64>,
    render_height: Cell<f64>,
    padding_bottom: Cell<f64>,
    /// `_scrollHeight` do `ListView` (sem o `paddingBottom`).
    view_scroll_height: Cell<f64>,
    /// A altura de rolagem que o `Scrollable` conhece.
    scrollable_scroll_height: Cell<f64>,
    /// Há um `scrollableElementUpdateDisposable` agendado (a atualização das dimensões no próximo quadro).
    scroll_dimensions_update_pending: Cell<bool>,
    notebook_cell_anchor: Rc<NotebookCellAnchor>,
    on_did_scroll: Rc<Emitter<ScrollEvent>>,

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
    view_zones: RefCell<Option<Rc<NotebookViewZones>>>,
    /// `_lastWhitespaceId` do `NotebookCellListView`.
    last_whitespace_id: Cell<u64>,
}

impl NotebookCellList {
    /// `new NotebookCellList(listUser, container, notebookOptions, delegate, renderers, contextKeyService, options, ...)`: o
    /// `listUser`, o `notebookOptions`, o `delegate.getHeight` (o `getTemplateId` e os renderizadores são da view) e os dois
    /// serviços que o `NotebookCellAnchor` lê.
    pub fn new(
        list_user: &str,
        notebook_options: Rc<NotebookOptions>,
        get_height: impl Fn(&CellViewModel) -> f64 + 'static,
        configuration_service: Rc<dyn IConfigurationService>,
        notebook_execution_state_service: Rc<dyn INotebookExecutionStateService>,
    ) -> Rc<Self> {
        let base = Disposable::new();
        let on_did_scroll = Emitter::<ScrollEvent>::new();
        base.register(on_did_scroll.clone());
        let notebook_cell_anchor = NotebookCellAnchor::new(notebook_execution_state_service, configuration_service, on_did_scroll.event());
        base.register(notebook_cell_anchor.clone());
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
            notebook_options,
            delegate: Box::new(get_height),
            elements: RefCell::new(Vec::new()),
            focus: RefCell::new(Vec::new()),
            selection: RefCell::new(Vec::new()),
            layout: RefCell::new(NotebookCellsLayout::new(None)),
            scroll_top: Cell::new(0.0),
            last_render_top: Cell::new(0.0),
            render_height: Cell::new(0.0),
            padding_bottom: Cell::new(0.0),
            view_scroll_height: Cell::new(0.0),
            scrollable_scroll_height: Cell::new(0.0),
            scroll_dimensions_update_pending: Cell::new(false),
            notebook_cell_anchor,
            on_did_scroll,
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
            view_zones: RefCell::new(None),
            last_whitespace_id: Cell::new(0),
        });
        *this.view_zones.borrow_mut() = Some(NotebookViewZones::new(Rc::downgrade(&this)));

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
        self.eventually_update_scroll_dimensions();

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

    /// `scrollHeight` do `ListView`: `_scrollHeight + paddingBottom`.
    pub fn get_scroll_height(&self) -> f64 {
        self.view_scroll_height.get() + self.padding_bottom.get()
    }

    pub fn scroll_top(&self) -> f64 {
        self.scroll_top.get()
    }

    /// `eventuallyUpdateScrollDimensions()`: `_scrollHeight` passa a ser a altura do conteúdo e o `Scrollable` é
    /// atualizado no próximo quadro (ver as divergências).
    fn eventually_update_scroll_dimensions(&self) {
        self.view_scroll_height.set(self.layout.borrow().size());
        self.scroll_dimensions_update_pending.set(true);
    }

    /// O quadro agendado por `eventuallyUpdateScrollDimensions` é cancelado e o `Scrollable` recebe o `scrollHeight` agora.
    fn flush_scroll_dimensions(&self) {
        if self.scroll_dimensions_update_pending.replace(false) {
            self.scrollable_scroll_height.set(self.get_scroll_height());
        }
    }

    /// `Scrollable.setScrollPosition({ scrollTop })` com o `forceIntegerValues` do `ListView`: valida contra as dimensões que
    /// o `Scrollable` conhece e dispara o `onScroll` (e o `render` do `ListView`, que atualiza o `lastRenderTop`) se mudou.
    fn scrollable_set_scroll_top(&self, scroll_top: f64) {
        let max = (self.scrollable_scroll_height.get() - self.render_height.get()).max(0.0);
        let new_scroll_top = scroll_top.clamp(0.0, max).trunc();
        let old_scroll_top = self.scroll_top.get();
        if new_scroll_top == old_scroll_top {
            return;
        }

        self.scroll_top.set(new_scroll_top);
        self.last_render_top.set(new_scroll_top);
        self.on_did_scroll.fire(ScrollEvent {
            in_smooth_scrolling: false,
            old_width: 0.0,
            old_scroll_width: 0.0,
            old_scroll_left: 0.0,
            width: 0.0,
            scroll_width: 0.0,
            scroll_left: 0.0,
            old_height: self.render_height.get(),
            old_scroll_height: self.scrollable_scroll_height.get(),
            old_scroll_top,
            height: self.render_height.get(),
            scroll_height: self.scrollable_scroll_height.get(),
            scroll_top: new_scroll_top,
            width_changed: false,
            scroll_width_changed: false,
            scroll_left_changed: false,
            height_changed: false,
            scroll_height_changed: false,
            scroll_top_changed: true,
        });
    }

    /// `ListView.setScrollTop(scrollTop)`.
    fn view_set_scroll_top(&self, scroll_top: f64) {
        self.flush_scroll_dimensions();
        self.scrollable_set_scroll_top(scroll_top);
    }

    pub fn set_scroll_top(&self, scroll_top: f64) {
        self.view_set_scroll_top(scroll_top);
        self.update_visible_ranges();
    }

    /// `onDidScroll`.
    pub fn on_did_scroll(&self) -> Event<ScrollEvent> {
        self.on_did_scroll.event()
    }

    /// `layout(height?, width?)`: a `width` só serve ao `_rerender` das alturas dinâmicas e não é lida.
    pub fn layout(&self, height: Option<f64>) {
        self.flush_scroll_dimensions();
        if let Some(height) = height {
            self.render_height.set(height);
        }
        // o `Scrollable` valida a posição contra as novas dimensões
        self.scrollable_set_scroll_top(self.scroll_top.get());
        self.update_visible_ranges();
    }

    /// `updateOptions(options)`: o `paddingBottom` vai direto ao `Scrollable`.
    pub fn update_options(&self, options: &CellListOptionsUpdate) {
        if let Some(padding_bottom) = options.padding_bottom {
            self.padding_bottom.set(padding_bottom);
            self.scrollable_scroll_height.set(self.get_scroll_height());
            self.scrollable_set_scroll_top(self.scroll_top.get());
        }
    }

    /// `getVisibleRange(renderTop, renderHeight)` do `ListView`: `[start, end)` dos itens visíveis.
    fn get_visible_range(&self, render_top: f64, render_height: f64) -> (i64, i64) {
        let layout = self.layout.borrow();
        (layout.index_at(render_top), layout.index_after(render_top + render_height - 1.0))
    }

    /// `firstMostlyVisibleIndex` do `ListView`.
    fn first_mostly_visible_index(&self) -> i64 {
        let first_visible_index = self.get_visible_range(self.last_render_top.get(), self.render_height.get()).0;
        let layout = self.layout.borrow();
        let first_el_top = layout.position_at(first_visible_index);
        let next_el_top = layout.position_at(first_visible_index + 1);
        if next_el_top != -1.0 {
            let first_el_midpoint = (next_el_top - first_el_top) / 2.0 + first_el_top;
            if first_el_midpoint < self.scroll_top.get() {
                return first_visible_index + 1;
            }
        }

        first_visible_index
    }

    /// `updateElementHeight(index, size, anchorIndex)` do `ListView`.
    pub fn update_element_height(&self, index: usize, size: f64, anchor_index: Option<usize>) {
        if index >= self.length() {
            return;
        }

        let original_size = self.layout.borrow().item_size(index);
        if original_size == size {
            return;
        }

        let (render_start, render_end) = self.get_visible_range(self.last_render_top.get(), self.render_height.get());

        let height_diff = if (index as i64) < render_start {
            // do not scroll the viewport if resized element is out of viewport
            size - original_size
        } else if anchor_index.is_some_and(|anchor| anchor > index && (anchor as i64) < render_end) {
            // anchor in viewport
            // resized element in viewport and above the anchor
            size - original_size
        } else {
            0.0
        };

        self.layout.borrow_mut().splice(index, 1, &[IItem { size }]);

        self.last_render_top.set((self.last_render_top.get() + height_diff).max(0.0));
        self.view_set_scroll_top(self.last_render_top.get());

        self.eventually_update_scroll_dimensions();
        self.update_visible_ranges();
    }

    /// `updateElementHeight(index, size)` do `List`: sem âncora.
    pub fn update_element_height_by_index(&self, index: usize, size: f64) {
        self.update_element_height(index, size, None);
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

    /// `isElementAboveViewport(index)`.
    fn is_element_above_viewport(&self, index: usize) -> bool {
        let element_top = self.element_top(index);
        let element_bottom = element_top + self.layout.borrow().item_size(index);

        element_bottom < self.scroll_top.get()
    }

    /// `updateElementHeight2(element, size, anchorElementIndex = null)`. As view zones e as sobreposições de célula
    /// (`viewZones.layout()`, `cellOverlays.layout()`) e o ajuste do webview são da view (Fase 6).
    pub fn update_element_height2(&self, element: &CellViewModel, size: f64, anchor_element_index: Option<usize>) {
        let index = self.get_view_index_upper_bound(element);
        if index < 0 || index as usize >= self.length() {
            return;
        }
        let index = index as usize;

        if self.is_element_above_viewport(index) {
            // update element above viewport
            self.update_element_height(index, size, anchor_element_index);
            self.view_zones().layout();
            return;
        }

        if anchor_element_index.is_some() {
            self.update_element_height(index, size, anchor_element_index);
            self.view_zones().layout();
            return;
        }

        let focused = self.get_focus();
        let focus = focused.first().copied();

        // `if (focus)`: o índice 0 é falso em JavaScript
        if let Some(focus) = focus.filter(|focus| *focus != 0) {
            // If the cell is growing, we should favor anchoring to the focused cell
            let height_delta = size - self.layout.borrow().item_size(index);

            if self.notebook_cell_anchor.should_anchor(&ListViewMetrics { list: self }, focus, height_delta, &self.element(index)) {
                self.update_element_height(index, size, Some(focus));
                self.view_zones().layout();
                return;
            }
        }

        self.update_element_height(index, size, None);
        self.view_zones().layout();
    }

    //#region View zones (`NotebookCellListView` e `NotebookViewZones`)
    /// As view zones da lista (criadas junto com ela).
    fn view_zones(&self) -> Rc<NotebookViewZones> {
        self.view_zones.borrow().clone().expect("as view zones são criadas com a lista")
    }

    /// `insertWhitespace(afterPosition, size)` do `NotebookCellListView`.
    pub fn insert_whitespace(&self, after_position: usize, size: f64) -> String {
        let scroll_top = self.scroll_top.get();
        self.last_whitespace_id.set(self.last_whitespace_id.get() + 1);
        let id = self.last_whitespace_id.get().to_string();
        let element_position = self.element_top(after_position);
        let above_scroll_top = scroll_top > element_position;
        self.layout.borrow_mut().insert_whitespace(&id, after_position, size);

        let new_scroll_top = if above_scroll_top { scroll_top + size } else { scroll_top };
        self.last_render_top.set(new_scroll_top);
        self.eventually_update_scroll_dimensions();
        self.update_visible_ranges();

        id
    }

    /// `changeOneWhitespace(id, newAfterPosition, newSize)` do `NotebookCellListView`.
    pub fn change_one_whitespace(&self, id: &str, new_after_position: usize, new_size: f64) {
        let scroll_top = self.scroll_top.get();
        let current_position = self.layout.borrow().get_whitespace_position(id);

        self.layout.borrow_mut().change_one_whitespace(id, new_after_position, new_size);
        if current_position > scroll_top {
            self.last_render_top.set(scroll_top);
        }
        self.eventually_update_scroll_dimensions();
        self.update_visible_ranges();
    }

    /// `removeWhitespace(id)` do `NotebookCellListView`.
    pub fn remove_whitespace(&self, id: &str) {
        let scroll_top = self.scroll_top.get();

        self.layout.borrow_mut().remove_whitespace(id);
        self.last_render_top.set(scroll_top);
        self.eventually_update_scroll_dimensions();
        self.update_visible_ranges();
    }

    /// `getWhitespacePosition(id)` do `NotebookCellListView` (o `_getView().getWhitespacePosition(id)` dos testes).
    pub fn get_whitespace_position(&self, id: &str) -> f64 {
        self.layout.borrow().get_whitespace_position(id)
    }

    /// `changeViewZones(callback)`.
    pub fn change_view_zones(&self, callback: impl FnOnce(&INotebookViewZoneChangeAccessor)) {
        let view_zones = self.view_zones();
        if view_zones.change_view_zones(callback) {
            view_zones.layout();
        }
    }

    /// `getViewZoneLayoutInfo(viewZoneId)`: `(height, top)`.
    pub fn get_view_zone_layout_info(&self, view_zone_id: &str) -> Option<(f64, f64)> {
        self.view_zones().get_view_zone_layout_info(view_zone_id)
    }
    //#endregion

    /// `revealCells(range)`: a faixa é revelada com o mínimo de rolagem possível.
    pub fn reveal_cells(&self, range: ICellRange) {
        let start_index = self.get_view_index_upper_bound2(range.start);

        if start_index < 0 {
            return;
        }
        let start_index = start_index as usize;

        let end_index = self.get_view_index_upper_bound2(range.end - 1);
        // o `elementTop(endIndex)` de um índice fora da lista é `undefined` no original; aqui o índice é o último
        let end_index = (end_index.max(0) as usize).min(self.length().saturating_sub(1));

        let scroll_top = self.get_view_scroll_top();
        let wrapper_bottom = self.get_view_scroll_bottom();
        let element_top = self.element_top(start_index);
        if element_top >= scroll_top && element_top < wrapper_bottom {
            // start element is visible
            // check end

            let end_element_top = self.element_top(end_index);
            let end_element_height = self.layout.borrow().item_size(end_index);

            if end_element_top + end_element_height <= wrapper_bottom {
                // fully visible
                return;
            }

            if end_element_top >= wrapper_bottom {
                return self.reveal_internal(end_index, false, CellRevealPosition::Bottom, false);
            }

            if end_element_top < wrapper_bottom {
                // end element partially visible
                if end_element_top + end_element_height - wrapper_bottom < element_top - scroll_top {
                    // there is enough space to just scroll up a little bit to make the end element visible
                    return self.set_scroll_top(scroll_top + end_element_top + end_element_height - wrapper_bottom);
                } else {
                    // don't even try it
                    return self.reveal_internal(start_index, false, CellRevealPosition::Top, false);
                }
            }
        }

        self.reveal_in_view_with_minimal_scrolling(start_index, false);
    }

    fn reveal_in_view_with_minimal_scrolling(&self, view_index: usize, first_line: bool) {
        let first_index = self.first_mostly_visible_index();
        let element_height = self.layout.borrow().item_size(view_index);

        if (view_index as i64) <= first_index || (!first_line && element_height >= self.render_height.get()) {
            self.reveal_internal(view_index, true, CellRevealPosition::Top, false);
        } else {
            self.reveal_internal(view_index, true, CellRevealPosition::Bottom, first_line);
        }
    }

    fn reveal_internal(&self, view_index: usize, ignore_if_inside_viewport: bool, reveal_position: CellRevealPosition, first_line: bool) {
        if view_index >= self.length() {
            return;
        }

        let scroll_top = self.get_view_scroll_top();
        let wrapper_bottom = self.get_view_scroll_bottom();
        let element_top = self.element_top(view_index);
        let element_bottom = self.layout.borrow().item_size(view_index) + element_top;

        if ignore_if_inside_viewport && element_top >= scroll_top && element_bottom < wrapper_bottom {
            // element is already fully visible
            return;
        }

        match reveal_position {
            CellRevealPosition::Top => {
                self.set_scroll_top(element_top);
                self.set_scroll_top(self.element_top(view_index));
            }
            CellRevealPosition::Center | CellRevealPosition::NearTop => {
                // reveal the cell top in the viewport center initially
                self.set_scroll_top(element_top - self.render_height.get() / 2.0);
                // cell rendered already, we now have a more accurate cell height
                let new_element_top = self.element_top(view_index);
                let new_element_height = self.layout.borrow().item_size(view_index);
                let render_height = self.get_view_scroll_bottom() - self.get_view_scroll_top();
                if new_element_height >= render_height {
                    // cell is larger than viewport, reveal top
                    self.set_scroll_top(new_element_top);
                } else if reveal_position == CellRevealPosition::Center {
                    self.set_scroll_top(new_element_top + (new_element_height / 2.0) - (render_height / 2.0));
                } else {
                    self.set_scroll_top(new_element_top - (render_height / 5.0));
                }
            }
            CellRevealPosition::Bottom => {
                if first_line {
                    let line_height = self.view_model().and_then(|view_model| view_model.layout_info()).map(|info| info.font_info.line_height).unwrap_or(15.0);
                    let layout_configuration = self.notebook_options.get_layout_configuration();
                    let padding = layout_configuration.cell_top_margin + layout_configuration.editor_top_padding;
                    let first_line_location = element_top + line_height + padding;
                    if first_line_location < wrapper_bottom {
                        // first line is already visible
                        return;
                    }

                    self.set_scroll_top(self.scroll_top.get() + (first_line_location - wrapper_bottom));
                    return;
                }
                self.set_scroll_top(self.scroll_top.get() + (element_bottom - wrapper_bottom));
                self.set_scroll_top(self.scroll_top.get() + (self.element_top(view_index) + self.layout.borrow().item_size(view_index) - self.get_view_scroll_bottom()));
            }
        }
    }

    /// `scrollToBottom()`.
    pub fn scroll_to_bottom(&self) {
        let scroll_height = self.get_scroll_height();
        let scroll_top = self.get_view_scroll_top();
        let wrapper_bottom = self.get_view_scroll_bottom();

        self.set_scroll_top(scroll_height - (wrapper_bottom - scroll_top));
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

                // update whitespaces which are anchored to the model indexes
                this.view_zones().on_cells_changed(e);

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
                self.view_zones().on_hidden_ranges_change();
                self.view_zones().layout();
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
        // Update view zone positions after hidden ranges change
        self.view_zones().on_hidden_ranges_change();

        if trigger_view_update {
            self.update_hidden_areas_in_view(&old_ranges, &new_ranges);
        }

        self.view_zones().layout();
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

        self.view_zones().layout();
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
        self.view_zones().dispose();
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

/// O `this.view` (o `ListView`) da lista, como o `NotebookCellAnchor` o lê.
struct ListViewMetrics<'a> {
    list: &'a NotebookCellList,
}

impl ICellListView for ListViewMetrics<'_> {
    fn element_focus_mode(&self, index: usize) -> CellFocusMode {
        self.list.element(index).base().focus_mode()
    }

    fn element_top(&self, index: usize) -> f64 {
        self.list.element_top(index)
    }

    fn element_height(&self, index: usize) -> f64 {
        self.list.layout.borrow().item_size(index)
    }

    fn render_height(&self) -> f64 {
        self.list.render_height.get()
    }

    fn get_scroll_top(&self) -> f64 {
        self.list.scroll_top.get()
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
