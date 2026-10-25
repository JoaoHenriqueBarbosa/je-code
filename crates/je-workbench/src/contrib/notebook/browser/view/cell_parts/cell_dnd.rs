//! Porte de `src/vs/workbench/contrib/notebook/browser/view/cellParts/cellDnd.ts`: o `CellDragAndDropController` (o indicador de
//! inserção, os ouvintes de arrastar e soltar, a rolagem durante o arraste, o arraste explícito dos testes e do webview), a
//! `CellDragAndDropPart` (que liga a classe `cell-dragging` ao container da célula) e a função `performCellDropEdits`, que move
//! as células arrastadas (a seleção inteira, se a célula arrastada faz parte dela) para perto da célula sobre a qual se soltou.
//!
//! Divergências:
//! - **DOM é a árvore do `je-ui`.** Os ouvintes `DOM.addDisposableListener(el, DRAG_*, ...)` são `Ui::listen` sobre os eventos de
//!   `je_ui::events` (`DragStart`, `DragEnd`, `DragOver`, `Drop`, `DragLeave`), e o `DragEvent` é o [`UiEvent`] com o
//!   `DataTransfer` de `je_ui::drag`. `document.body` é a raiz da árvore (`Ui::root`). `clientY` é o `y` do ponteiro (coordenada da
//!   janela), `getBoundingClientRect()` é `absolute_bounds`, e `ctrlKey` e `altKey` vêm das teclas modificadoras do ponteiro.
//! - **`relatedTarget`.** O evento `dragleave` do `je-ui` não carrega o nó para onde o ponteiro foi; ele é o nó sob o ponteiro
//!   (`Tree::hit_test` nas coordenadas do evento), que é o que o navegador informa, e só é calculado no `dragleave`.
//! - **`style.opacity` e `style.top` do indicador** são o `opacity` e o `y` do retângulo do nó. A cor (`notebook.cellInsertionIndicator`
//!   do tema) chega por quem tem o tema, no construtor e em [`CellDragAndDropController::set_insertion_indicator_background`].
//! - **`INotebookEditorDelegate` e `INotebookCellList`.** O controlador lê do editor só o contrato [`ICellDragAndDropEditor`] (o
//!   [`IActiveNotebookEditor`] mais `getDomNode`, `notebookOptions`, `hasModel` e `textModel?.viewType`), e da lista o
//!   [`NotebookCellList`] concreto. `list.onWillScroll` é o `on_did_scroll` da lista, que dispara no mesmo ponto (a posição do
//!   `Scrollable` mudou), porque o `ListView` do porte não separa os dois. `set scrollTop` é `set_scroll_top`.
//! - **`list!` e `notebookEditor`.** Um evento antes de `set_list`, ou depois do `dispose` (que solta o editor como o
//!   `this.notebookEditor = null!`), é ignorado, onde o original lançaria um `TypeError` por referência nula.
//! - **`BaseCellRenderTemplate`.** O que `registerDragHandle` lê do template é o contrato [`ICellDragHandleTemplate`] (`container`,
//!   `templateDisposables` e `currentRenderedCell`); o `dragImageProvider` é um [`DragImageProvider`], que devolve o nó solto da
//!   imagem de arraste. O `setTimeout(() => dragImage.remove(), 0)` é um `set_timeout` de zero que remove o nó da árvore.
//! - `Delayer<void>` é `Delayer<()>`, `ctx: { dragOffsetY, ctrlKey, altKey }` é [`ExplicitDropContext`], `dropDirection:
//!   'above' | 'below'` é [`CellDropDirection`], e `isMacintosh` é lido uma vez, no construtor.
//! - `updateInsertIndicator(dropDirection, pos)` perde o primeiro argumento, que o original não lê, e o cálculo de
//!   `insertionIndicatorTop`, repetido em `updateInsertIndicator` e em `_dropImpl`, é [`CellDragAndDropController::insertion_indicator_top`].
//! - `CellDragAndDropPart` troca a classe pelo `toggle_class` do nó, e `element.dragging` é o `dragging()` da base da célula.
//! - `editor.getNextVisibleCellIndex(index) ?? index` é o `getNextVisibleCellIndex` do view model (que sempre devolve um
//!   índice, e é o que o widget repassa).
//! - `typeof index !== 'number'` (célula fora do modelo) é o índice negativo de `getCellIndex`.

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use je_base::async_::Delayer;
use je_base::lifecycle::{Disposable, DisposableStore, IDisposable, MutableDisposable};
use je_base::platform::Platform;
use je_base::promise::MaybePromise;
use je_base::runtime::set_timeout;
use je_base::symbols::Delay;
use je_render::Rgba;
use je_render::kurbo::Rect;
use je_ui::{DropEffect, Event as UiEvent, EventKind, NodeId, Role, Ui};
use je_widgets::dom::create_el;

use crate::contrib::notebook::browser::controller::{IActiveNotebookEditor, expand_cell_ranges_with_hidden_cells};
use crate::contrib::notebook::browser::notebook_options::NotebookOptions;
use crate::contrib::notebook::browser::notebook_view_events::CellViewModelStateChangeEvent;
use crate::contrib::notebook::browser::view::cell_part::{CellPart, CellPartState};
use crate::contrib::notebook::browser::view::notebook_cell_list::NotebookCellList;
use crate::contrib::notebook::browser::view_model::notebook_view_model_impl::CellViewModel;
use crate::contrib::notebook::common::model::notebook_cell_text_model::clone_notebook_cell_text_model;
use crate::contrib::notebook::common::notebook_common::{ICellEditOperation, ICellMoveEdit, ICellReplaceEdit, ISelectionIndexState, ISelectionState};
use crate::contrib::notebook::common::notebook_range::{ICellRange, cell_ranges_to_indexes};

/// `DRAGGING_CLASS`.
pub const DRAGGING_CLASS: &str = "cell-dragging";

/// `GLOBAL_DRAG_CLASS`.
pub const GLOBAL_DRAG_CLASS: &str = "global-drag-active";

/// A classe do indicador de inserção (`.cell-list-insertion-indicator`).
const INSERTION_INDICATOR_CLASS: &str = "cell-list-insertion-indicator";

/// `height: 2px` do `.cell-list-insertion-indicator`.
const INSERTION_INDICATOR_HEIGHT: f64 = 2.0;

/// `new Delayer(200)`: quanto tempo depois da última rolagem o arraste volta a valer.
const SCROLLING_DELAY_MS: u64 = 200;

/// `DragImageProvider`: `() => HTMLElement`, o nó solto da imagem de arraste.
pub type DragImageProvider = Box<dyn Fn() -> NodeId>;

/// O que `registerDragHandle` lê do `BaseCellRenderTemplate`.
pub trait ICellDragHandleTemplate {
    /// `container`.
    fn container(&self) -> NodeId;
    /// `templateDisposables`.
    fn template_disposables(&self) -> Rc<DisposableStore>;
    /// `currentRenderedCell`.
    fn current_rendered_cell(&self) -> Option<CellViewModel>;
}

/// O `INotebookEditorDelegate` no que o `CellDragAndDropController` lê: o editor ativo mais o nó do widget, as opções e o tipo de
/// visão do modelo.
pub trait ICellDragAndDropEditor: IActiveNotebookEditor {
    /// `getDomNode()`.
    fn get_dom_node(&self) -> NodeId;
    /// `notebookOptions`.
    fn notebook_options(&self) -> Rc<NotebookOptions>;
    /// `hasModel()`.
    fn has_model(&self) -> bool;
    /// `textModel?.viewType`.
    fn text_model_view_type(&self) -> Option<String>;
}

/// `CellDragAndDropPart`.
pub struct CellDragAndDropPart {
    state: CellPartState,
    ui: Ui,
    container: NodeId,
}

impl CellDragAndDropPart {
    pub fn new(ui: &Ui, container: NodeId) -> Rc<Self> {
        Rc::new(Self { state: CellPartState::new(), ui: ui.clone(), container })
    }

    fn update(&self, element: &CellViewModel) {
        self.ui.tree_mut().toggle_class(self.container, DRAGGING_CLASS, element.base().dragging());
    }
}

impl CellPart for CellDragAndDropPart {
    fn state(&self) -> &CellPartState {
        &self.state
    }

    fn did_render_cell(&self, element: &CellViewModel) {
        self.update(element);
    }

    fn update_state(&self, element: &CellViewModel, e: &CellViewModelStateChangeEvent) {
        if e.drag_state_changed {
            self.update(element);
        }
    }
}

impl_cell_content_part!(CellDragAndDropPart, "CellDragAndDropPart");

/// `'above' | 'below'`: de que lado da célula sobre a qual se soltou as células arrastadas entram.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CellDropDirection {
    Above,
    Below,
}

/// `ctx: { dragOffsetY, ctrlKey, altKey }` de `explicitDrop`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ExplicitDropContext {
    pub drag_offset_y: f64,
    pub ctrl_key: bool,
    pub alt_key: bool,
}

/// Os campos do `DragEvent` que o controlador lê.
struct BrowserDragEvent {
    /// `ctrlKey`.
    ctrl_key: bool,
    /// `altKey`.
    alt_key: bool,
    /// `dataTransfer`.
    data_transfer: Option<je_ui::DataTransfer>,
    /// `relatedTarget`: só preenchido no `dragleave`.
    related_target: Option<NodeId>,
}

/// `CellDragEvent`.
struct CellDragEvent {
    browser_event: BrowserDragEvent,
    dragged_over_cell: CellViewModel,
    cell_top: f64,
    cell_height: f64,
    drag_pos_ratio: f64,
}

/// `CellDragAndDropController`.
pub struct CellDragAndDropController {
    base: Rc<Disposable>,
    ui: Ui,
    me: Weak<Self>,
    notebook_editor: RefCell<Option<Rc<dyn ICellDragAndDropEditor>>>,
    notebook_list_container: NodeId,
    // Nota do original (roblourens): o ideal seria usar o `dataTransfer` aqui, mas qualquer `dataTransfer` escrito faz o editor
    // achar que se está soltando um arquivo, e falta descobrir como evitar isso.
    current_dragged_cell: RefCell<Option<CellViewModel>>,
    dragged_cells: RefCell<Vec<CellViewModel>>,
    list_insertion_indicator: NodeId,
    list: RefCell<Option<Rc<NotebookCellList>>>,
    is_scrolling: Rc<Cell<bool>>,
    scrolling_delayer: Rc<Delayer<()>>,
    list_on_will_scroll_listener: Rc<MutableDisposable<dyn IDisposable>>,
    is_macintosh: bool,
}

impl CellDragAndDropController {
    /// `new CellDragAndDropController(notebookEditor, notebookListContainer)`; `insertion_indicator_background` é o
    /// `notebook.cellInsertionIndicator` do tema.
    pub fn new(ui: &Ui, notebook_editor: Rc<dyn ICellDragAndDropEditor>, notebook_list_container: NodeId, insertion_indicator_background: Rgba) -> Rc<Self> {
        let list_insertion_indicator = {
            let mut tree = ui.tree_mut();
            let indicator = create_el(&mut tree, notebook_list_container, Role::Generic, INSERTION_INDICATOR_CLASS);
            tree.set_bounds(indicator, Rect::new(0.0, 0.0, 0.0, INSERTION_INDICATOR_HEIGHT));
            tree.set_fill(indicator, true, false);
            let style = tree.style_mut(indicator);
            style.background = Some(insertion_indicator_background);
            style.opacity = 0.0;
            indicator
        };

        let base = Disposable::new();
        let scrolling_delayer = Rc::new(Delayer::<()>::new(Delay::Millis(SCROLLING_DELAY_MS)));
        base.register(scrolling_delayer.clone());
        let list_on_will_scroll_listener = MutableDisposable::<dyn IDisposable>::new();
        base.register(list_on_will_scroll_listener.clone());

        let editor_dom_node = notebook_editor.get_dom_node();
        Rc::new_cyclic(|me: &Weak<Self>| {
            for (kind, active) in [(EventKind::DragStart, true), (EventKind::DragEnd, false)] {
                let me = me.clone();
                base.register(ui.listen(ui.root(), kind, true, move |_| {
                    if let Some(this) = me.upgrade() {
                        this.set_global_drag_class(active);
                    }
                }));
            }

            Self::add_cell_drag_listener(&base, ui, me, editor_dom_node, EventKind::DragOver, true, |this, event, raw| {
                if this.current_dragged_cell.borrow().is_none() {
                    return;
                }
                raw.prevent_default();
                this.on_cell_dragover(event);
            });
            Self::add_cell_drag_listener(&base, ui, me, editor_dom_node, EventKind::Drop, false, |this, event, raw| {
                if this.current_dragged_cell.borrow().is_none() {
                    return;
                }
                raw.prevent_default();
                this.on_cell_drop(event);
            });
            Self::add_cell_drag_listener(&base, ui, me, editor_dom_node, EventKind::DragLeave, false, |this, event, raw| {
                raw.prevent_default();
                this.on_cell_drag_leave(event);
            });

            Self {
                base,
                ui: ui.clone(),
                me: me.clone(),
                notebook_editor: RefCell::new(Some(notebook_editor)),
                notebook_list_container,
                current_dragged_cell: RefCell::new(None),
                dragged_cells: RefCell::new(Vec::new()),
                list_insertion_indicator,
                list: RefCell::new(None),
                is_scrolling: Rc::new(Cell::new(false)),
                scrolling_delayer,
                list_on_will_scroll_listener,
                is_macintosh: Platform::detect().is_macintosh,
            }
        })
    }

    /// `addCellDragListener(eventType, handler, useCapture)`: o ouvinte de `node` só chama o `handler` se o ponteiro está sobre uma
    /// célula; o `handler` recebe também o evento cru, para o `preventDefault`.
    fn add_cell_drag_listener(base: &Disposable, ui: &Ui, me: &Weak<Self>, node: NodeId, kind: EventKind, capture: bool, handler: fn(&Self, &CellDragEvent, &mut UiEvent)) {
        let me = me.clone();
        base.register(ui.listen(node, kind, capture, move |raw| {
            let Some(this) = me.upgrade() else { return };
            if let Some(cell_drag_event) = this.to_cell_drag_event(raw) {
                handler(&this, &cell_drag_event, raw);
            }
        }));
    }

    /// O editor, ou `None` depois do `dispose` (ver as divergências do módulo).
    fn editor(&self) -> Option<Rc<dyn ICellDragAndDropEditor>> {
        self.notebook_editor.borrow().clone()
    }

    /// A lista, ou `None` antes de `set_list` (ver as divergências do módulo).
    fn list(&self) -> Option<Rc<NotebookCellList>> {
        self.list.borrow().clone()
    }

    /// `setList(value)`.
    pub fn set_list(&self, value: Rc<NotebookCellList>) {
        *self.list.borrow_mut() = Some(value.clone());

        let (me, is_scrolling, scrolling_delayer) = (self.me.clone(), self.is_scrolling.clone(), self.scrolling_delayer.clone());
        self.list_on_will_scroll_listener.set(Some(value.on_did_scroll().listen(move |e| {
            if !e.scroll_top_changed {
                return;
            }

            if let Some(this) = me.upgrade() {
                this.set_insert_indicator_visibility(false);
            }
            is_scrolling.set(true);
            let is_scrolling = is_scrolling.clone();
            let _ = scrolling_delayer.trigger(
                move || {
                    is_scrolling.set(false);
                    MaybePromise::Value(())
                },
                None,
            );
        })));
    }

    /// A cor do indicador de inserção (`notebook.cellInsertionIndicator`), quando o tema muda.
    pub fn set_insertion_indicator_background(&self, background: Rgba) {
        self.ui.tree_mut().style_mut(self.list_insertion_indicator).background = Some(background);
    }

    /// `listInsertionIndicator.style.opacity = visible ? '1' : '0'`.
    fn set_insert_indicator_visibility(&self, visible: bool) {
        self.ui.tree_mut().style_mut(self.list_insertion_indicator).opacity = if visible { 1.0 } else { 0.0 };
    }

    /// `toCellDragEvent(event)`.
    fn to_cell_drag_event(&self, event: &UiEvent) -> Option<CellDragEvent> {
        let pointer = event.pointer?;
        let list = self.list()?;
        let target_top = self.ui.tree().absolute_bounds(self.notebook_list_container).y0;
        let drag_offset = list.scroll_top() + pointer.y - target_top;
        let dragged_over_cell = list.element_at(drag_offset)?;

        let cell_top = list.get_cell_view_scroll_top(&dragged_over_cell);
        let cell_height = list.element_height(&dragged_over_cell);

        let drag_pos_in_element = drag_offset - cell_top;
        let drag_pos_ratio = drag_pos_in_element / cell_height;

        let related_target = if event.kind == EventKind::DragLeave { self.ui.tree().hit_test(pointer.x, pointer.y) } else { None };
        Some(CellDragEvent {
            browser_event: BrowserDragEvent {
                ctrl_key: pointer.modifiers.ctrl,
                alt_key: pointer.modifiers.alt,
                data_transfer: event.data_transfer.clone(),
                related_target,
            },
            dragged_over_cell,
            cell_top,
            cell_height,
            drag_pos_ratio,
        })
    }

    /// `clearGlobalDragState()`.
    pub fn clear_global_drag_state(&self) {
        self.set_global_drag_class(false);
    }

    /// `classList.add(GLOBAL_DRAG_CLASS)` (`onGlobalDragStart`) ou `classList.remove(GLOBAL_DRAG_CLASS)` (`onGlobalDragEnd` e
    /// `clearGlobalDragState`) no nó do editor.
    fn set_global_drag_class(&self, active: bool) {
        if let Some(editor) = self.editor() {
            self.ui.tree_mut().toggle_class(editor.get_dom_node(), GLOBAL_DRAG_CLASS, active);
        }
    }

    /// `!dragAndDropEnabled || !!isReadOnly`, a condição que os quatro pontos de entrada do arraste checam.
    fn is_drag_and_drop_disabled(editor: &dyn ICellDragAndDropEditor) -> bool {
        !editor.notebook_options().get_display_options().drag_and_drop_enabled || editor.is_read_only()
    }

    /// `this.currentDraggedCell === cell`.
    fn is_current_dragged_cell(&self, cell: &CellViewModel) -> bool {
        self.current_dragged_cell.borrow().as_ref().is_some_and(|current| current.ptr_eq(cell))
    }

    fn on_cell_dragover(&self, event: &CellDragEvent) {
        let Some(data_transfer) = &event.browser_event.data_transfer else {
            return;
        };

        if self.current_dragged_cell.borrow().is_none() {
            data_transfer.set_drop_effect(DropEffect::None);
            return;
        }

        if self.is_scrolling.get() || self.is_current_dragged_cell(&event.dragged_over_cell) {
            self.set_insert_indicator_visibility(false);
            return;
        }

        let drop_direction = Self::get_drop_insert_direction(event.drag_pos_ratio);
        let insertion_indicator_absolute_pos = if drop_direction == CellDropDirection::Above { event.cell_top } else { event.cell_top + event.cell_height };
        self.update_insert_indicator(insertion_indicator_absolute_pos);
    }

    /// `insertionIndicatorAbsolutePos - this.list.scrollTop + bottomToolbarGap / 2`.
    fn insertion_indicator_top(editor: &dyn ICellDragAndDropEditor, list: &NotebookCellList, insertion_indicator_absolute_pos: f64) -> f64 {
        let view_type = editor.text_model_view_type();
        let bottom_toolbar_gap = editor.notebook_options().compute_bottom_toolbar_dimensions(view_type.as_deref()).bottom_toolbar_gap;
        insertion_indicator_absolute_pos - list.scroll_top() + bottom_toolbar_gap / 2.0
    }

    fn update_insert_indicator(&self, insertion_indicator_absolute_pos: f64) {
        let (Some(editor), Some(list)) = (self.editor(), self.list()) else {
            return;
        };
        let insertion_indicator_top = Self::insertion_indicator_top(&*editor, &list, insertion_indicator_absolute_pos);
        if insertion_indicator_top >= 0.0 {
            let mut tree = self.ui.tree_mut();
            let bounds = tree.bounds(self.list_insertion_indicator);
            tree.set_bounds(self.list_insertion_indicator, Rect::new(bounds.x0, insertion_indicator_top, bounds.x1, insertion_indicator_top + bounds.height()));
            drop(tree);
            self.set_insert_indicator_visibility(true);
        } else {
            self.set_insert_indicator_visibility(false);
        }
    }

    fn get_drop_insert_direction(drag_pos_ratio: f64) -> CellDropDirection {
        if drag_pos_ratio < 0.5 { CellDropDirection::Above } else { CellDropDirection::Below }
    }

    fn on_cell_drop(&self, event: &CellDragEvent) {
        let Some(dragged_cell) = self.current_dragged_cell.borrow().clone() else {
            return;
        };

        if self.is_scrolling.get() || self.is_current_dragged_cell(&event.dragged_over_cell) {
            return;
        }

        self.drag_cleanup();

        let drop_direction = Self::get_drop_insert_direction(event.drag_pos_ratio);
        self.drop_impl(&dragged_cell, drop_direction, event.browser_event.ctrl_key, event.browser_event.alt_key, &event.dragged_over_cell);
    }

    fn get_cell_range_around_drag_target(editor: &dyn ICellDragAndDropEditor, dragged_cell_index: i64) -> ICellRange {
        let selections = editor.get_selections();
        let model_ranges = expand_cell_ranges_with_hidden_cells(editor, &selections);
        let nearest_range = model_ranges.into_iter().find(|range| range.start <= dragged_cell_index && dragged_cell_index < range.end);

        nearest_range.unwrap_or(ICellRange { start: dragged_cell_index, end: dragged_cell_index + 1 })
    }

    fn drop_impl(&self, dragged_cell: &CellViewModel, drop_direction: CellDropDirection, ctrl_key: bool, alt_key: bool, dragged_over_cell: &CellViewModel) {
        let (Some(editor), Some(list)) = (self.editor(), self.list()) else {
            return;
        };
        let cell_top = list.get_cell_view_scroll_top(dragged_over_cell);
        let cell_height = list.element_height(dragged_over_cell);
        let insertion_indicator_absolute_pos = if drop_direction == CellDropDirection::Above { cell_top } else { cell_top + cell_height };
        let insertion_indicator_top = Self::insertion_indicator_top(&*editor, &list, insertion_indicator_absolute_pos);
        let editor_height = self.ui.tree().absolute_bounds(editor.get_dom_node()).height();
        if insertion_indicator_top < 0.0 || insertion_indicator_top > editor_height {
            // Ignore drop, insertion point is off-screen
            return;
        }

        let is_copy = (ctrl_key && !self.is_macintosh) || (alt_key && self.is_macintosh);

        if !editor.has_model() {
            return;
        }

        if is_copy {
            let dragged_cell_index = editor.get_cell_index(dragged_cell);
            let range = Self::get_cell_range_around_drag_target(&*editor, dragged_cell_index);

            let mut original_to_idx = editor.get_cell_index(dragged_over_cell);
            if drop_direction == CellDropDirection::Below {
                original_to_idx = editor.get_view_model().get_next_visible_cell_index(original_to_idx);
            }

            let final_selection: ICellRange;
            let final_focus: ICellRange;

            if original_to_idx <= range.start {
                final_selection = ICellRange { start: original_to_idx, end: original_to_idx + range.end - range.start };
                final_focus = ICellRange { start: original_to_idx + dragged_cell_index - range.start, end: original_to_idx + dragged_cell_index - range.start + 1 };
            } else {
                let delta = original_to_idx - range.start;
                final_selection = ICellRange { start: range.start + delta, end: range.end + delta };
                final_focus = ICellRange { start: dragged_cell_index + delta, end: dragged_cell_index + delta + 1 };
            }

            let cells = cell_ranges_to_indexes(&[range])
                .into_iter()
                .map(|index| clone_notebook_cell_text_model(&editor.cell_at(index).expect("a célula do intervalo arrastado existe").model()))
                .collect();
            let begin_selection_state = ISelectionState::Index(ISelectionIndexState { focus: editor.get_focus(), selections: editor.get_selections() });
            editor.text_model().apply_edits(
                &[ICellEditOperation::Replace(ICellReplaceEdit { index: original_to_idx as usize, count: 0, cells })],
                true,
                Some(begin_selection_state),
                &|| Some(ISelectionState::Index(ISelectionIndexState { focus: final_focus, selections: vec![final_selection] })),
                None,
                true,
            );
            editor.reveal_cell_range_in_view(final_selection);
        } else {
            perform_cell_drop_edits(&*editor, dragged_cell, drop_direction, dragged_over_cell);
        }
    }

    fn on_cell_drag_leave(&self, event: &CellDragEvent) {
        let Some(editor) = self.editor() else {
            return;
        };
        let editor_node = editor.get_dom_node();
        let is_inside_editor = event.browser_event.related_target.is_some_and(|related_target| self.ui.tree().is_ancestor(editor_node, related_target));
        if !is_inside_editor {
            self.set_insert_indicator_visibility(false);
        }
    }

    fn drag_cleanup(&self) {
        if self.current_dragged_cell.borrow().is_some() {
            // O `dragging = false` dispara eventos de estado que podem voltar ao controlador: nenhum empréstimo fica aberto.
            let dragged_cells = std::mem::take(&mut *self.dragged_cells.borrow_mut());
            for cell in &dragged_cells {
                cell.base().set_dragging(false);
            }
            *self.current_dragged_cell.borrow_mut() = None;
        }

        self.set_insert_indicator_visibility(false);
    }

    /// Registra `handler` como ouvinte de `kind` em cada alça, até o `store` do template ser descartado.
    fn listen_on_drag_handles(&self, store: &DisposableStore, drag_handles: &[NodeId], kind: EventKind, handler: impl Fn(&mut UiEvent) + 'static) {
        let handler: Rc<dyn Fn(&mut UiEvent)> = Rc::new(handler);
        for drag_handle in drag_handles {
            let handler = handler.clone();
            store.add(self.ui.listen(*drag_handle, kind, false, move |e| handler(e)));
        }
    }

    /// `registerDragHandle(templateData, cellRoot, dragHandles, dragImageProvider)`.
    pub fn register_drag_handle(&self, template_data: Rc<dyn ICellDragHandleTemplate>, cell_root: NodeId, drag_handles: &[NodeId], drag_image_provider: DragImageProvider) {
        let container = template_data.container();
        {
            let mut tree = self.ui.tree_mut();
            for drag_handle in drag_handles {
                tree.set_attribute(*drag_handle, "draggable", "true");
            }
        }

        let store = template_data.template_disposables();

        let me = self.me.clone();
        self.listen_on_drag_handles(&store, drag_handles, EventKind::DragEnd, move |_| {
            let Some(this) = me.upgrade() else { return };
            let Some(editor) = this.editor() else { return };
            if Self::is_drag_and_drop_disabled(&*editor) {
                return;
            }

            // Note, templateData may have a different element rendered into it by now
            this.ui.tree_mut().remove_class(container, DRAGGING_CLASS);
            this.drag_cleanup();
        });

        let me = self.me.clone();
        self.listen_on_drag_handles(&store, drag_handles, EventKind::DragStart, move |event| {
            let Some(data_transfer) = event.data_transfer.clone() else { return };
            let Some(this) = me.upgrade() else { return };
            let Some(editor) = this.editor() else { return };
            if Self::is_drag_and_drop_disabled(&*editor) {
                return;
            }

            *this.current_dragged_cell.borrow_mut() = template_data.current_rendered_cell();
            let dragged_cells: Vec<CellViewModel> = editor.get_selections().into_iter().flat_map(|range| editor.get_cells_in_range(Some(range))).collect();
            for cell in &dragged_cells {
                cell.base().set_dragging(true);
            }
            *this.dragged_cells.borrow_mut() = dragged_cells;

            let drag_image = drag_image_provider();
            // `cellRoot.parentElement!`: sem pai o original lançaria, aqui não há imagem de arraste.
            let Some(cell_root_parent) = this.ui.tree().parent(cell_root) else { return };
            this.ui.tree_mut().append_child(cell_root_parent, drag_image);
            data_transfer.set_drag_image(drag_image, 0.0, 0.0);
            let ui = this.ui.clone();
            set_timeout(
                move || {
                    if ui.tree().is_alive(drag_image) {
                        ui.remove(drag_image); // Comment this out to debug drag image layout
                    }
                },
                0,
            );
        });
    }

    /// `startExplicitDrag(cell, _dragOffsetY)`.
    pub fn start_explicit_drag(&self, cell: &CellViewModel, _drag_offset_y: f64) {
        let Some(editor) = self.editor() else { return };
        if Self::is_drag_and_drop_disabled(&*editor) {
            return;
        }

        *self.current_dragged_cell.borrow_mut() = Some(cell.clone());
        self.set_insert_indicator_visibility(true);
    }

    /// `explicitDrag(cell, dragOffsetY)`.
    pub fn explicit_drag(&self, cell: &CellViewModel, drag_offset_y: f64) {
        let (Some(editor), Some(list)) = (self.editor(), self.list()) else {
            return;
        };
        if Self::is_drag_and_drop_disabled(&*editor) {
            return;
        }

        if let Some(target) = list.element_at(drag_offset_y)
            && !target.ptr_eq(cell)
        {
            let cell_top = list.get_cell_view_scroll_top(&target);
            let cell_height = list.element_height(&target);

            let drop_direction = Self::get_explicit_drag_drop_direction(drag_offset_y, cell_top, cell_height);
            let insertion_indicator_absolute_pos = if drop_direction == CellDropDirection::Above { cell_top } else { cell_top + cell_height };
            self.update_insert_indicator(insertion_indicator_absolute_pos);
        }

        // Try scrolling list if needed
        if !self.is_current_dragged_cell(cell) {
            return;
        }

        let notebook_view_height = self.ui.tree().absolute_bounds(editor.get_dom_node()).height();
        let event_position_in_view = drag_offset_y - list.scroll_top();

        // Percentage from the top/bottom of the screen where we start scrolling while dragging
        let notebook_view_scroll_margins = 0.2;

        let max_scroll_delta_per_frame = 20.0;

        let event_position_ratio = event_position_in_view / notebook_view_height;
        if event_position_ratio < notebook_view_scroll_margins {
            list.set_scroll_top(list.scroll_top() - max_scroll_delta_per_frame * (1.0 - event_position_ratio / notebook_view_scroll_margins));
        } else if event_position_ratio > 1.0 - notebook_view_scroll_margins {
            list.set_scroll_top(list.scroll_top() + max_scroll_delta_per_frame * (1.0 - ((1.0 - event_position_ratio) / notebook_view_scroll_margins)));
        }
    }

    /// `endExplicitDrag(_cell)`.
    pub fn end_explicit_drag(&self, _cell: &CellViewModel) {
        self.set_insert_indicator_visibility(false);
    }

    /// `explicitDrop(cell, ctx)`.
    pub fn explicit_drop(&self, cell: &CellViewModel, ctx: ExplicitDropContext) {
        *self.current_dragged_cell.borrow_mut() = None;
        self.set_insert_indicator_visibility(false);

        let Some(list) = self.list() else { return };
        let Some(target) = list.element_at(ctx.drag_offset_y) else { return };
        if target.ptr_eq(cell) {
            return;
        }

        let cell_top = list.get_cell_view_scroll_top(&target);
        let cell_height = list.element_height(&target);
        let drop_direction = Self::get_explicit_drag_drop_direction(ctx.drag_offset_y, cell_top, cell_height);
        self.drop_impl(cell, drop_direction, ctx.ctrl_key, ctx.alt_key, &target);
    }

    fn get_explicit_drag_drop_direction(client_y: f64, cell_top: f64, cell_height: f64) -> CellDropDirection {
        let drag_pos_in_element = client_y - cell_top;
        let drag_pos_ratio = drag_pos_in_element / cell_height;

        Self::get_drop_insert_direction(drag_pos_ratio)
    }
}

impl IDisposable for CellDragAndDropController {
    fn dispose(&self) {
        *self.notebook_editor.borrow_mut() = None;
        self.base.dispose();
    }

    fn constructor_name(&self) -> &'static str {
        "CellDragAndDropController"
    }
}

/// `performCellDropEdits(editor, draggedCell, dropDirection, draggedOverCell)`.
pub fn perform_cell_drop_edits(
    editor: &dyn IActiveNotebookEditor,
    dragged_cell: &CellViewModel,
    drop_direction: CellDropDirection,
    dragged_over_cell: &CellViewModel,
) {
    let dragged_cell_index = editor.get_cell_index(dragged_cell);
    let mut original_to_idx = editor.get_cell_index(dragged_over_cell);

    if dragged_cell_index < 0 || original_to_idx < 0 {
        return;
    }

    // If dropped on a folded markdown range, insert after the folding range
    if drop_direction == CellDropDirection::Below {
        original_to_idx = editor.get_view_model().get_next_visible_cell_index(original_to_idx);
    }

    let mut selections = editor.get_selections();
    if selections.is_empty() {
        selections = vec![editor.get_focus()];
    }

    let mut original_focus_idx = editor.get_focus().start;

    // If the dragged cell is not focused/selected, ignore the current focus/selection and use the dragged idx
    if !selections.iter().any(|s| s.start <= dragged_cell_index && s.end > dragged_cell_index) {
        selections = vec![ICellRange { start: dragged_cell_index, end: dragged_cell_index + 1 }];
        original_focus_idx = dragged_cell_index;
    }

    if let Some(dropped_in_selection) = selections.iter().find(|range| range.start <= original_to_idx && range.end > original_to_idx) {
        original_to_idx = dropped_in_selection.start;
    }

    let mut num_cells: i64 = 0;
    let mut focus_new_idx = original_to_idx;
    let mut new_insertion_idx = original_to_idx;

    // Compute a set of edits which will be applied in reverse order by the notebook text model.
    // `index`: the starting index of the range, after previous edits have been applied
    // `newIdx`: the destination index, after this edit's range has been removed
    selections.sort_by(|a, b| b.start.cmp(&a.start));
    let edits: Vec<ICellMoveEdit> = selections
        .iter()
        .map(|range| {
            let length = range.end - range.start;

            // If this range is before the insertion point, subtract the cells in this range from the "to" index
            let to_index_delta = if range.end <= new_insertion_idx { -length } else { 0 };

            let new_idx = new_insertion_idx + to_index_delta;

            // If this range contains the focused cell, set the new focus index to the new index of the cell
            if original_focus_idx >= range.start && original_focus_idx <= range.end {
                let offset = original_focus_idx - range.start;
                focus_new_idx = new_idx + offset;
            }

            // If below the insertion point, the original index will have been shifted down
            let from_index_delta = if range.start >= original_to_idx { num_cells } else { 0 };

            let edit = ICellMoveEdit { index: (range.start + from_index_delta) as usize, length: length as usize, new_idx: new_idx as usize };
            num_cells += length;

            // If a range was moved down, the insertion index needs to be adjusted
            if range.end < new_insertion_idx {
                new_insertion_idx -= length;
            }

            edit
        })
        .collect();

    let Some(last_edit) = edits.last() else {
        return;
    };
    let final_selection = ICellRange { start: last_edit.new_idx as i64, end: last_edit.new_idx as i64 + num_cells };
    let final_focus = ICellRange { start: focus_new_idx, end: focus_new_idx + 1 };

    let begin_selection_state = ISelectionState::Index(ISelectionIndexState { focus: editor.get_focus(), selections: editor.get_selections() });
    let operations: Vec<ICellEditOperation> = edits.into_iter().map(ICellEditOperation::Move).collect();
    editor.text_model().apply_edits(
        &operations,
        true,
        Some(begin_selection_state),
        &|| Some(ISelectionState::Index(ISelectionIndexState { focus: final_focus, selections: vec![final_selection] })),
        None,
        true,
    );
    editor.reveal_cell_range_in_view(final_selection);
}
