//! Porte de `src/vs/base/browser/ui/list/listView.ts`: a lista virtualizada. Só as linhas visíveis existem
//! como nós da árvore; as posições vêm do [`RangeMap`], a rolagem do `SmoothScrollableElement`.
//!
//! Do DOM para o `je-ui`:
//! - `domNode` (`.monaco-list`) é um nó que preenche o contêiner; o elemento rolável o preenche; o
//!   `rowsContainer` (`.monaco-list-rows`) é o conteúdo do elemento rolável, posicionado em `top: -renderTop`
//!   como no original, com a altura de rolagem.
//! - Cada linha é um nó `.monaco-list-row` com `top` e `height` (`setRowHeight`) e largura de 100%. O
//!   `data-index` do original é o mapa nó para índice de [`ListView::index_of_row_node`], que serve ao
//!   `getItemIndexFromEventTarget`. `aria-setsize`, `aria-posinset`, `aria-checked` e o `id` do nó não
//!   existem no `je-ui` (só `role` e `label`); o id do elemento é [`ListView::get_element_dom_id`].
//! - `scheduleAtNextAnimationFrame` é o do [`Ui`] (o dono da janela chama [`Ui::run_animation_frame`]).
//! - Os eventos de mouse vêm dos ouvintes de ponteiro do [`Ui`] no `domNode`.
//!
//! Fica de fora, por falta do que o `je-ui` ainda não tem (cada item é marcado `// TODO(integração)` onde
//! entra): arrastar e soltar (`dnd`), toque, seleção de texto do usuário (`userSelection`), rolagem
//! horizontal (`horizontalScrolling`, que mede a largura de conteúdo dos nós) e a medição de altura
//! dinâmica pela caixa da linha (`offsetHeight`). A altura dinâmica fornecida pelo delegate
//! (`getDynamicHeight`) funciona, com o mesmo `_rerender` do original.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::{Rc, Weak};

use je_base::event::{Emitter, Event as BaseEvent};
use je_base::lifecycle::{Disposable, DisposableRef, IDisposable};
use je_base::range::{IRange, intersect, relative_complement};
use je_base::scrollable::{INewScrollDimensions, INewScrollPosition, IScrollableOptions, ScrollEvent, Scrollable, ScrollbarVisibility};
use je_render::kurbo::Rect;
use je_ui::{EventKind, NodeId, PointerButton, PointerData, Role, Ui};

use crate::common::{registered_emitter, throw};
use crate::list::list_types::{BrowserEvent, IListRenderer, IListVirtualDelegate, ListElementRenderDetails, ListMouseEvent, RendererMap};
use crate::list::range_map::{IItem, IRangeMap, RangeMap, shift};
use crate::list::row_cache::{Row, RowCache};
use crate::scrollbar::scrollable_element::AbstractScrollableElement;
use crate::scrollbar::scrollable_element_options::{ScrollableElementChangeOptions, ScrollableElementCreationOptions};

struct Item<T> {
    #[allow(dead_code)]
    id: u64,
    element: T,
    template_id: Rc<str>,
    row: Option<Row>,
    size: f64,
    has_dynamic_height: bool,
    last_dynamic_height_width: Option<f64>,
    stale: bool,
}

/// `IListViewOptionsUpdate`.
#[derive(Clone, Debug, Default)]
pub struct ListViewOptionsUpdate {
    pub smooth_scrolling: Option<bool>,
    pub scroll_by_page: Option<bool>,
    pub mouse_wheel_scroll_sensitivity: Option<f64>,
    pub fast_scroll_sensitivity: Option<f64>,
    pub padding_top: Option<f64>,
    pub padding_bottom: Option<f64>,
}

/// `IListViewOptions<T>`. O `dnd`, `userSelection`, `horizontalScrolling` e `transformOptimization` (que é
/// só um truque de composição do navegador) não existem aqui; `useShadows`, `verticalScrollMode`,
/// `setRowLineHeight`, `setRowHeight`, `supportDynamicHeights`, `mouseSupport`, `alwaysConsumeMouseWheel`,
/// `initialSize` e `scrollToActiveElement` têm os padrões do original.
#[derive(Clone)]
pub struct ListViewOptions {
    pub update: ListViewOptionsUpdate,
    pub use_shadows: bool,
    pub vertical_scroll_mode: ScrollbarVisibility,
    pub set_row_line_height: bool,
    pub set_row_height: bool,
    pub support_dynamic_heights: bool,
    pub mouse_support: bool,
    pub always_consume_mouse_wheel: bool,
    /// `initialSize`: `(width, height)`.
    pub initial_size: Option<(f64, f64)>,
}

impl Default for ListViewOptions {
    fn default() -> Self {
        Self {
            update: ListViewOptionsUpdate::default(),
            use_shadows: true,
            vertical_scroll_mode: ScrollbarVisibility::Auto,
            set_row_line_height: true,
            set_row_height: true,
            support_dynamic_heights: false,
            mouse_support: true,
            always_consume_mouse_wheel: true,
            initial_size: None,
        }
    }
}

thread_local! {
    static INSTANCE_COUNT: Cell<u64> = const { Cell::new(0) };
}

pub struct ListView<T: Clone + 'static> {
    base: Rc<Disposable>,
    weak_self: Weak<ListView<T>>,
    ui: Ui,
    pub dom_id: String,
    /// O `domNode`.
    pub dom_node: NodeId,
    virtual_delegate: Rc<dyn IListVirtualDelegate<T>>,
    items: RefCell<Vec<Item<T>>>,
    item_id: Cell<u64>,
    range_map: RefCell<RangeMap>,
    cache: RowCache<T>,
    renderers: RendererMap<T>,
    template_ids: RefCell<HashMap<String, Rc<str>>>,
    last_render_top: Cell<f64>,
    last_render_height: Cell<f64>,
    render_width: Cell<f64>,
    pub(crate) rows_container: NodeId,
    scrollable: Rc<Scrollable>,
    scrollable_element: Rc<AbstractScrollableElement>,
    scroll_height_value: Cell<f64>,
    scrollable_element_update_disposable: RefCell<Option<DisposableRef>>,
    splicing: Cell<bool>,
    set_row_line_height: bool,
    set_row_height: bool,
    support_dynamic_heights: bool,
    padding_bottom: Cell<f64>,
    row_indexes: RefCell<HashMap<NodeId, usize>>,
    on_did_change_content_height: Rc<Emitter<f64>>,
    on_mouse_click: Rc<Emitter<ListMouseEvent<T>>>,
    on_mouse_dbl_click: Rc<Emitter<ListMouseEvent<T>>>,
    on_context_menu: Rc<Emitter<ListMouseEvent<T>>>,
    on_mouse_up: Rc<Emitter<ListMouseEvent<T>>>,
    on_mouse_down: Rc<Emitter<ListMouseEvent<T>>>,
    on_mouse_move: Rc<Emitter<ListMouseEvent<T>>>,
    disposed: Cell<bool>,
}

impl<T: Clone + 'static> ListView<T> {
    pub fn new(ui: &Ui, container: NodeId, virtual_delegate: Rc<dyn IListVirtualDelegate<T>>, renderers: Vec<Rc<dyn IListRenderer<T>>>, options: ListViewOptions) -> Rc<Self> {
        let base = Disposable::new();
        let dom_id = INSTANCE_COUNT.with(|c| {
            c.set(c.get() + 1);
            format!("list_id_{}", c.get())
        });
        let renderer_map: RendererMap<T> = Rc::default();
        for renderer in renderers {
            renderer_map.borrow_mut().insert(renderer.template_id(), renderer);
        }

        let (dom_node, rows_container) = {
            let mut tree = ui.tree_mut();
            let dom_node = tree.create_detached(Role::List);
            tree.add_class(dom_node, "monaco-list");
            tree.add_class(dom_node, &dom_id);
            tree.toggle_class(dom_node, "mouse-support", options.mouse_support);
            tree.set_fill(dom_node, true, true);
            let rows_container = tree.create_detached(Role::Generic);
            tree.add_class(rows_container, "monaco-list-rows");
            tree.set_fill(rows_container, true, false);
            (dom_node, rows_container)
        };

        let smooth = options.update.smooth_scrolling.unwrap_or(false);
        let scheduler_ui = ui.clone();
        let scrollable = Scrollable::new(IScrollableOptions {
            force_integer_values: true,
            smooth_scroll_duration: if smooth { 125.0 } else { 0.0 },
            schedule_at_next_animation_frame: Rc::new(move |cb| scheduler_ui.schedule_at_next_animation_frame(0, cb)),
        });
        base.register(scrollable.clone());
        let scrollable_element = AbstractScrollableElement::smooth_scrollable_element(
            ui,
            rows_container,
            ScrollableElementCreationOptions {
                always_consume_mouse_wheel: Some(options.always_consume_mouse_wheel),
                horizontal: Some(ScrollbarVisibility::Auto),
                vertical: Some(options.vertical_scroll_mode),
                use_shadows: Some(options.use_shadows),
                mouse_wheel_scroll_sensitivity: options.update.mouse_wheel_scroll_sensitivity,
                fast_scroll_sensitivity: options.update.fast_scroll_sensitivity,
                scroll_by_page: options.update.scroll_by_page,
                ..Default::default()
            },
            scrollable.clone(),
        );
        base.register(scrollable_element.clone());
        {
            let mut tree = ui.tree_mut();
            tree.set_fill(scrollable_element.get_dom_node(), true, true);
            tree.append_child(dom_node, scrollable_element.get_dom_node());
            tree.append_child(container, dom_node);
        }

        let padding_top = options.update.padding_top.unwrap_or(0.0);
        let this = Rc::new_cyclic(|weak| Self {
            base: base.clone(),
            weak_self: weak.clone(),
            ui: ui.clone(),
            dom_id,
            dom_node,
            virtual_delegate,
            items: RefCell::new(Vec::new()),
            item_id: Cell::new(0),
            range_map: RefCell::new(RangeMap::new(Some(padding_top))),
            cache: RowCache::new(ui, renderer_map.clone()),
            renderers: renderer_map,
            template_ids: RefCell::default(),
            last_render_top: Cell::new(0.0),
            last_render_height: Cell::new(0.0),
            render_width: Cell::new(0.0),
            rows_container,
            scrollable,
            scrollable_element,
            scroll_height_value: Cell::new(0.0),
            scrollable_element_update_disposable: RefCell::new(None),
            splicing: Cell::new(false),
            set_row_line_height: options.set_row_line_height,
            set_row_height: options.set_row_height,
            support_dynamic_heights: options.support_dynamic_heights,
            padding_bottom: Cell::new(options.update.padding_bottom.unwrap_or(0.0)),
            row_indexes: RefCell::default(),
            on_did_change_content_height: registered_emitter(&base),
            on_mouse_click: registered_emitter(&base),
            on_mouse_dbl_click: registered_emitter(&base),
            on_context_menu: registered_emitter(&base),
            on_mouse_up: registered_emitter(&base),
            on_mouse_down: registered_emitter(&base),
            on_mouse_move: registered_emitter(&base),
            disposed: Cell::new(false),
        });

        let weak = this.weak_self.clone();
        base.register(this.scrollable_element.on_scroll().listen(move |e| {
            if let Some(this) = weak.upgrade() {
                this.on_scroll(e);
            }
        }));

        for (kind, emitter) in [
            (EventKind::Click, this.on_mouse_click.clone()),
            (EventKind::DoubleClick, this.on_mouse_dbl_click.clone()),
            (EventKind::ContextMenu, this.on_context_menu.clone()),
            (EventKind::PointerUp, this.on_mouse_up.clone()),
            (EventKind::PointerDown, this.on_mouse_down.clone()),
            (EventKind::PointerMove, this.on_mouse_move.clone()),
        ] {
            let weak = this.weak_self.clone();
            base.register(ui.listen(dom_node, kind, false, move |e| {
                let (Some(this), Some(pointer)) = (weak.upgrade(), e.pointer) else { return };
                emitter.fire(this.to_mouse_event(e.target, &pointer));
            }));
        }

        this.layout(options.initial_size.map(|s| s.1), options.initial_size.map(|s| s.0));
        this
    }

    fn intern_template_id(&self, id: String) -> Rc<str> {
        self.template_ids.borrow_mut().entry(id).or_insert_with_key(|k| Rc::from(k.as_str())).clone()
    }

    pub fn on_did_change_content_height(&self) -> BaseEvent<f64> {
        self.on_did_change_content_height.event()
    }

    pub fn content_height(&self) -> f64 {
        self.range_map.borrow().size()
    }

    pub fn content_width(&self) -> f64 {
        0.0
    }

    pub fn on_did_scroll(&self) -> BaseEvent<ScrollEvent> {
        self.scrollable_element.on_scroll()
    }

    pub fn on_will_scroll(&self) -> BaseEvent<ScrollEvent> {
        self.scrollable_element.on_will_scroll()
    }

    pub fn on_mouse_click(&self) -> BaseEvent<ListMouseEvent<T>> {
        self.on_mouse_click.event()
    }

    pub fn on_mouse_dbl_click(&self) -> BaseEvent<ListMouseEvent<T>> {
        self.on_mouse_dbl_click.event()
    }

    /// `onContextMenu`: o botão secundário sobre uma linha (ou sobre o vazio, com `index` `None`).
    pub fn on_context_menu(&self) -> BaseEvent<ListMouseEvent<T>> {
        self.on_context_menu.event()
    }

    pub fn on_mouse_up(&self) -> BaseEvent<ListMouseEvent<T>> {
        self.on_mouse_up.event()
    }

    pub fn on_mouse_down(&self) -> BaseEvent<ListMouseEvent<T>> {
        self.on_mouse_down.event()
    }

    pub fn on_mouse_move(&self) -> BaseEvent<ListMouseEvent<T>> {
        self.on_mouse_move.event()
    }

    /// O `containerDomNode` (`.monaco-list-rows`).
    pub fn container_dom_node(&self) -> NodeId {
        self.rows_container
    }

    pub fn scrollable_element_dom_node(&self) -> NodeId {
        self.scrollable_element.get_dom_node()
    }

    pub fn update_options(&self, options: &ListViewOptionsUpdate) {
        if let Some(padding_bottom) = options.padding_bottom {
            self.padding_bottom.set(padding_bottom);
            self.scrollable_element.set_scroll_dimensions(&INewScrollDimensions { scroll_height: Some(self.scroll_height()), ..Default::default() });
        }

        if let Some(smooth) = options.smooth_scrolling {
            self.scrollable.set_smooth_scroll_duration(if smooth { 125.0 } else { 0.0 });
        }

        let scrollable_options = ScrollableElementChangeOptions {
            scroll_by_page: options.scroll_by_page,
            mouse_wheel_scroll_sensitivity: options.mouse_wheel_scroll_sensitivity,
            fast_scroll_sensitivity: options.fast_scroll_sensitivity,
            ..Default::default()
        };
        if options.scroll_by_page.is_some() || options.mouse_wheel_scroll_sensitivity.is_some() || options.fast_scroll_sensitivity.is_some() {
            self.scrollable_element.update_options(&scrollable_options);
        }

        if let Some(padding_top) = options.padding_top
            && padding_top != self.range_map.borrow().padding_top()
        {
            // Render com a nova altura, e deixa o scroll top onde estava (o conteúdo desceu).
            let last_render_range = self.get_render_range(self.last_render_top.get(), self.last_render_height.get());
            let offset = padding_top - self.range_map.borrow().padding_top();
            self.range_map.borrow_mut().set_padding_top(padding_top);

            self.render(last_render_range, f64::max(0.0, self.last_render_top.get() + offset), self.last_render_height.get(), true, false);
            self.set_scroll_top(self.last_render_top.get(), false);

            self.eventually_update_scroll_dimensions();

            if self.support_dynamic_heights {
                self.rerender_range(self.last_render_top.get(), self.last_render_height.get(), false);
            }
        }
    }

    /// `delegateScrollFromMouseWheelEvent`.
    pub fn delegate_scroll_from_mouse_wheel_event(&self, wheel: &je_ui::WheelData, target: Option<NodeId>) {
        self.scrollable_element.delegate_scroll_from_mouse_wheel_event(wheel, target);
    }

    /// `delegateVerticalScrollbarPointerDown`.
    pub fn delegate_vertical_scrollbar_pointer_down(&self, pointer: &PointerData, target: NodeId) {
        self.scrollable_element.delegate_vertical_scrollbar_pointer_down(pointer, target);
    }

    pub fn update_element_height(&self, index: usize, size: Option<f64>, anchor_index: Option<usize>) {
        if index >= self.items.borrow().len() {
            return;
        }

        let original_size = self.items.borrow()[index].size;

        let size = match size {
            Some(size) => size,
            None => {
                if !self.support_dynamic_heights {
                    // `console.warn('Dynamic heights not supported')`
                    return;
                }
                self.items.borrow_mut()[index].last_dynamic_height_width = None;
                original_size + self.probe_dynamic_height(index)
            }
        };

        if original_size == size {
            return;
        }

        let last_render_range = self.get_render_range(self.last_render_top.get(), self.last_render_height.get());

        let height_diff = if (index as f64) < last_render_range.start {
            // o item está acima do que está renderizado: o conteúdo andou
            size - original_size
        } else if anchor_index.is_some_and(|a| a > index && (a as f64) < last_render_range.end) {
            // mantém a âncora onde estava
            size - original_size
        } else {
            0.0
        };

        self.range_map.borrow_mut().splice(index, 1, &[IItem { size }]);
        self.items.borrow_mut()[index].size = size;

        self.render(last_render_range, f64::max(0.0, self.last_render_top.get() + height_diff), self.last_render_height.get(), true, false);
        self.set_scroll_top(self.last_render_top.get(), false);

        // Atualiza o scroll height depois do render, para a rolagem não pular.
        self.eventually_update_scroll_dimensions();

        if self.support_dynamic_heights {
            self.rerender_range(self.last_render_top.get(), self.last_render_height.get(), false);
        } else {
            // senão é disparado no `_rerender`
            self.on_did_change_content_height.fire(self.content_height());
        }
    }

    /// `splice(start, deleteCount, elements)`: devolve os elementos removidos.
    pub fn splice(&self, start: usize, delete_count: usize, elements: &[T]) -> Vec<T> {
        if self.splicing.get() {
            throw("Can't run recursive splices.");
        }

        self.splicing.set(true);
        struct Finish<'a, T: Clone + 'static>(&'a ListView<T>);
        impl<T: Clone + 'static> Drop for Finish<'_, T> {
            fn drop(&mut self) {
                self.0.splicing.set(false);
                self.0.on_did_change_content_height.fire(self.0.content_height());
            }
        }
        let _finish = Finish(self);
        self.do_splice(start, delete_count, elements)
    }

    fn do_splice(&self, start: usize, delete_count: usize, elements: &[T]) -> Vec<T> {
        let previous_render_range = self.get_render_range(self.last_render_top.get(), self.last_render_height.get());

        let delete_range = IRange { start: start as f64, end: (start + delete_count) as f64 };
        let remove_range = intersect(previous_render_range, delete_range);

        // O disposeElement é chamado em ordem decrescente e as linhas ficam em ordem crescente por template.
        let mut rows_to_dispose: HashMap<Rc<str>, Vec<Row>> = HashMap::new();

        let mut i = remove_range.end as i64 - 1;
        while i >= remove_range.start as i64 {
            let idx = i as usize;
            let (row, template_id, element, size) = {
                let mut items = self.items.borrow_mut();
                let item = &mut items[idx];
                let row = item.row.take();
                item.stale = true;
                (row, item.template_id.clone(), item.element.clone(), item.size)
            };
            if let Some(row) = row {
                if let Some(renderer) = self.renderer(&template_id) {
                    renderer.dispose_element(&element, idx as i64, &row.template_data, Some(&ListElementRenderDetails { height: Some(size), on_scroll: None }));
                }
                self.row_indexes.borrow_mut().remove(&row.dom_node);
                rows_to_dispose.entry(template_id).or_default().insert(0, row);
            }
            i -= 1;
        }

        let items_len = self.items.borrow().len();
        let previous_rest_range = IRange { start: (start + delete_count) as f64, end: items_len as f64 };
        let previous_rendered_rest_range = intersect(previous_rest_range, previous_render_range);
        let previous_unrendered_rest_ranges = relative_complement(previous_rest_range, previous_render_range);

        let inserted: Vec<Item<T>> = elements
            .iter()
            .map(|element| {
                let id = self.item_id.get();
                self.item_id.set(id + 1);
                let template_id = self.intern_template_id(self.virtual_delegate.get_template_id(element));
                Item {
                    id,
                    element: element.clone(),
                    template_id,
                    size: self.virtual_delegate.get_height(element),
                    has_dynamic_height: self.virtual_delegate.has_dynamic_height(element).unwrap_or(false),
                    last_dynamic_height_width: None,
                    row: None,
                    stale: false,
                }
            })
            .collect();
        let inserted_sizes: Vec<IItem> = inserted.iter().map(|i| IItem { size: i.size }).collect();

        let deleted: Vec<Item<T>> = if start == 0 && delete_count >= items_len {
            let padding_top = self.range_map.borrow().padding_top();
            let mut fresh = RangeMap::new(Some(padding_top));
            fresh.splice(0, 0, &inserted_sizes);
            *self.range_map.borrow_mut() = fresh;
            std::mem::replace(&mut *self.items.borrow_mut(), inserted)
        } else {
            self.range_map.borrow_mut().splice(start, delete_count, &inserted_sizes);
            self.items.borrow_mut().splice(start..start + delete_count, inserted).collect()
        };

        let delta = elements.len() as f64 - delete_count as f64;
        let render_range = self.get_render_range(self.last_render_top.get(), self.last_render_height.get());
        let rendered_rest_range = shift(previous_rendered_rest_range, delta);
        let update_range = intersect(render_range, rendered_rest_range);

        for i in update_range.start as usize..update_range.end as usize {
            if self.items.borrow()[i].row.is_some() {
                self.update_item_in_dom(i);
            }
        }

        let remove_ranges = relative_complement(rendered_rest_range, render_range);

        for range in remove_ranges {
            for i in range.start as usize..range.end as usize {
                self.remove_item_from_dom(i, None);
            }
        }

        let unrendered_rest_ranges: Vec<IRange> = previous_unrendered_rest_ranges.into_iter().map(|r| shift(r, delta)).collect();
        let elements_range = IRange { start: start as f64, end: (start + elements.len()) as f64 };
        let mut insert_ranges: Vec<IRange> = std::iter::once(elements_range).chain(unrendered_rest_ranges).map(|r| intersect(render_range, r)).collect();
        insert_ranges.reverse();

        'insert: for range in insert_ranges {
            let mut i = range.end as i64 - 1;
            while i >= range.start as i64 {
                let idx = i as usize;
                let (template_id, in_range) = {
                    let items = self.items.borrow();
                    (items.get(idx).map(|it| it.template_id.clone()), idx < items.len())
                };
                if !in_range {
                    break 'insert;
                }
                let row = template_id.and_then(|t| rows_to_dispose.get_mut(&t).and_then(Vec::pop));
                self.insert_item_in_dom(idx, row);

                if self.items.borrow()[idx].row.is_none() {
                    break 'insert;
                }
                i -= 1;
            }
        }

        for rows in rows_to_dispose.values() {
            for row in rows {
                self.cache.release(row);
            }
        }

        self.eventually_update_scroll_dimensions();

        if self.support_dynamic_heights {
            self.rerender_range(self.scroll_top(), self.render_height(), false);
        }

        deleted.into_iter().map(|i| i.element).collect()
    }

    fn renderer(&self, template_id: &str) -> Option<Rc<dyn IListRenderer<T>>> {
        self.renderers.borrow().get(template_id).cloned()
    }

    fn eventually_update_scroll_dimensions(&self) {
        self.scroll_height_value.set(self.content_height());
        {
            let mut tree = self.ui.tree_mut();
            let b = tree.bounds(self.rows_container);
            tree.set_bounds(self.rows_container, Rect::new(b.x0, b.y0, b.x0 + b.width(), b.y0 + self.scroll_height_value.get()));
        }

        if self.scrollable_element_update_disposable.borrow().is_none() {
            let weak = self.weak_self.clone();
            let disposable = self.ui.schedule_at_next_animation_frame(0, move || {
                if let Some(this) = weak.upgrade() {
                    this.scrollable_element.set_scroll_dimensions(&INewScrollDimensions { scroll_height: Some(this.scroll_height()), ..Default::default() });
                    this.scrollable_element_update_disposable.borrow_mut().take();
                }
            });
            *self.scrollable_element_update_disposable.borrow_mut() = Some(disposable);
        }
    }

    pub fn rerender(&self) {
        if !self.support_dynamic_heights {
            return;
        }
        for item in self.items.borrow_mut().iter_mut() {
            item.last_dynamic_height_width = None;
        }
        self.rerender_range(self.last_render_top.get(), self.last_render_height.get(), false);
    }

    pub fn length(&self) -> usize {
        self.items.borrow().len()
    }

    pub fn render_height(&self) -> f64 {
        self.scrollable_element.get_scroll_dimensions().height
    }

    pub fn first_visible_index(&self) -> i64 {
        self.get_visible_range(self.last_render_top.get(), self.last_render_height.get()).start as i64
    }

    pub fn first_mostly_visible_index(&self) -> i64 {
        let first_visible_index = self.first_visible_index();
        let first_el_top = self.range_map.borrow().position_at(first_visible_index);
        let next_el_top = self.range_map.borrow().position_at(first_visible_index + 1);

        if next_el_top != -1.0 {
            let first_el_midpoint = (next_el_top - first_el_top) / 2.0 + first_el_top;

            if first_el_midpoint < self.scroll_top() {
                return first_visible_index + 1;
            }
        }

        first_visible_index
    }

    pub fn last_visible_index(&self) -> i64 {
        let range = self.get_render_range(self.last_render_top.get(), self.last_render_height.get());
        range.end as i64 - 1
    }

    pub fn element(&self, index: usize) -> T {
        self.items.borrow()[index].element.clone()
    }

    /// `indexOf`: a identidade do `findIndex(item => item.element === element)` é `eq`.
    pub fn index_of(&self, eq: impl Fn(&T) -> bool) -> Option<usize> {
        self.items.borrow().iter().position(|item| eq(&item.element))
    }

    /// Os nós de todas as linhas renderizadas (as visíveis e as do excesso de desenho).
    pub fn rendered_dom_nodes(&self) -> Vec<NodeId> {
        self.items.borrow().iter().filter_map(|item| item.row.as_ref().map(|r| r.dom_node)).collect()
    }

    /// `domElement(index)`: o nó da linha, se renderizada.
    pub fn dom_element(&self, index: usize) -> Option<NodeId> {
        self.items.borrow()[index].row.as_ref().map(|r| r.dom_node)
    }

    pub fn element_height(&self, index: usize) -> f64 {
        self.items.borrow()[index].size
    }

    pub fn element_top(&self, index: usize) -> f64 {
        self.range_map.borrow().position_at(index as i64)
    }

    pub fn index_at(&self, position: f64) -> i64 {
        self.range_map.borrow().index_at(position)
    }

    pub fn index_after(&self, position: f64) -> i64 {
        self.range_map.borrow().index_after(position)
    }

    /// `layout(height?, width?)`: sem altura, vale a do nó.
    pub fn layout(&self, height: Option<f64>, width: Option<f64>) {
        let mut scroll_dimensions = INewScrollDimensions { height: Some(height.unwrap_or_else(|| self.ui.tree().bounds(self.dom_node).height())), ..Default::default() };

        // Solta o `RefCell` antes de usar o valor: `set_scroll_dimensions` dispara `on_scroll`, que reentra aqui.
        let pending_update = self.scrollable_element_update_disposable.borrow_mut().take();
        if let Some(disposable) = pending_update {
            disposable.dispose();
            scroll_dimensions.scroll_height = Some(self.scroll_height());
        }

        self.scrollable_element.set_scroll_dimensions(&scroll_dimensions);

        if let Some(width) = width {
            self.render_width.set(width);

            if self.support_dynamic_heights {
                self.rerender_range(self.scroll_top(), self.render_height(), false);
            }
        }
    }

    /// `render(previousRenderRange, renderTop, renderHeight, ..., updateItemsInDOM, onScroll)`.
    fn render(&self, previous_render_range: IRange, render_top: f64, render_height: f64, update_items_in_dom: bool, on_scroll: bool) {
        let render_range = self.get_render_range(render_top, render_height);

        let mut ranges_to_insert = relative_complement(render_range, previous_render_range);
        ranges_to_insert.reverse();
        let ranges_to_remove = relative_complement(previous_render_range, render_range);

        if update_items_in_dom {
            let ranges_to_update = intersect(previous_render_range, render_range);

            for i in ranges_to_update.start as usize..ranges_to_update.end as usize {
                self.update_item_in_dom(i);
            }
        }

        let insert_before_remove = self.support_dynamic_heights
            && ranges_to_insert.iter().any(|range| {
                let items = self.items.borrow();
                items[range.start as usize..range.end as usize].iter().any(|item| self.should_probe_dynamic_height(item))
            });

        self.cache.transact(|| {
            let insert_rows = || {
                for range in &ranges_to_insert {
                    let mut i = range.end as i64 - 1;
                    while i >= range.start as i64 {
                        self.insert_item_in_dom(i as usize, None);
                        i -= 1;
                    }
                }
            };

            if insert_before_remove {
                insert_rows();
            }

            for range in &ranges_to_remove {
                for i in range.start as usize..range.end as usize {
                    self.remove_item_from_dom(i, Some(on_scroll));
                }
            }

            if !insert_before_remove {
                insert_rows();
            }
        });

        {
            let mut tree = self.ui.tree_mut();
            let b = tree.bounds(self.rows_container);
            tree.set_bounds(self.rows_container, Rect::new(b.x0, -render_top, b.x0 + b.width(), -render_top + b.height()));
        }

        self.last_render_top.set(render_top);
        self.last_render_height.set(render_height);
    }

    /// Insere a linha do item no DOM. `row` é uma linha liberada pelo `splice` para reaproveitar.
    fn insert_item_in_dom(&self, index: usize, row: Option<Row>) {
        let (needs_row, template_id) = {
            let items = self.items.borrow();
            (items[index].row.is_none(), items[index].template_id.clone())
        };

        if needs_row {
            let mut items = self.items.borrow_mut();
            if let Some(row) = row {
                items[index].row = Some(row);
                items[index].stale = true;
            } else {
                drop(items);
                let (row, is_reusing_connected_dom_node) = self.cache.alloc(&template_id);
                let mut items = self.items.borrow_mut();
                items[index].row = Some(row);
                items[index].stale = items[index].stale || is_reusing_connected_dom_node;
            }
        }

        let (dom_node, stale) = {
            let items = self.items.borrow();
            (items[index].row.as_ref().expect("a linha acabou de ser alocada").dom_node, items[index].stale)
        };

        {
            let mut tree = self.ui.tree_mut();
            tree.set_role(dom_node, Role::ListItem);
            if stale || tree.parent(dom_node).is_none() {
                let reference = self.items.borrow().get(index + 1).and_then(|i| i.row.as_ref()).map(|r| r.dom_node);
                // `insertBefore` só se mudou de lugar
                let next = tree.parent(dom_node).and_then(|p| {
                    let children = tree.children(p);
                    children.iter().position(|c| *c == dom_node).and_then(|i| children.get(i + 1).copied())
                });
                if tree.parent(dom_node) != Some(self.rows_container) || next != reference {
                    tree.insert_before(self.rows_container, dom_node, reference);
                }
                drop(tree);
                self.items.borrow_mut()[index].stale = false;
            }
        }

        self.update_item_in_dom(index);

        let Some(renderer) = self.renderer(&template_id) else {
            throw(&format!("No renderer found for template id {template_id}"));
        };

        let (element, template_data, size) = {
            let items = self.items.borrow();
            let item = &items[index];
            (item.element.clone(), item.row.as_ref().expect("linha presente").template_data.clone(), item.size)
        };
        renderer.render_element(&element, index, &template_data, Some(&ListElementRenderDetails { height: Some(size), on_scroll: None }));
    }

    fn update_item_in_dom(&self, index: usize) {
        let (dom_node, size) = {
            let items = self.items.borrow();
            let item = &items[index];
            (item.row.as_ref().expect("a linha existe").dom_node, item.size)
        };
        let top = self.element_top(index);
        let mut tree = self.ui.tree_mut();
        let height = if self.set_row_height { size } else { tree.bounds(dom_node).height() };
        tree.set_bounds(dom_node, Rect::new(0.0, top, 0.0, top + height));
        tree.set_fill(dom_node, true, false);
        drop(tree);
        // `setRowLineHeight` é só estilo de texto: a linha de texto do `je-ui` centra no próprio nó.
        let _ = self.set_row_line_height;
        self.row_indexes.borrow_mut().insert(dom_node, index);
    }

    fn remove_item_from_dom(&self, index: usize, on_scroll: Option<bool>) {
        let (row, template_id, element, size) = {
            let mut items = self.items.borrow_mut();
            let item = &mut items[index];
            (item.row.take(), item.template_id.clone(), item.element.clone(), item.size)
        };

        if let Some(row) = row {
            if let Some(renderer) = self.renderer(&template_id) {
                renderer.dispose_element(&element, index as i64, &row.template_data, Some(&ListElementRenderDetails { height: Some(size), on_scroll }));
            }
            self.row_indexes.borrow_mut().remove(&row.dom_node);
            self.cache.release(&row);
        }
    }

    pub fn get_scroll_top(&self) -> f64 {
        self.scrollable_element.get_scroll_position().scroll_top
    }

    /// `setScrollTop(scrollTop, reuseAnimation?)`.
    pub fn set_scroll_top(&self, scroll_top: f64, reuse_animation: bool) {
        // Solta o `RefCell` antes de usar o valor: `set_scroll_dimensions` dispara `on_scroll`, que reentra aqui.
        let pending_update = self.scrollable_element_update_disposable.borrow_mut().take();
        if let Some(disposable) = pending_update {
            disposable.dispose();
            self.scrollable_element.set_scroll_dimensions(&INewScrollDimensions { scroll_height: Some(self.scroll_height()), ..Default::default() });
        }

        self.scrollable_element.set_scroll_position_smooth(&INewScrollPosition { scroll_top: Some(scroll_top), scroll_left: None }, reuse_animation);
    }

    pub fn get_scroll_left(&self) -> f64 {
        self.scrollable_element.get_scroll_position().scroll_left
    }

    pub fn set_scroll_left(&self, scroll_left: f64) {
        self.scrollable_element.set_scroll_position(&INewScrollPosition { scroll_left: Some(scroll_left), scroll_top: None });
    }

    pub fn scroll_top(&self) -> f64 {
        self.get_scroll_top()
    }

    pub fn scroll_height(&self) -> f64 {
        self.scroll_height_value.get() + self.padding_bottom.get()
    }

    /// `toMouseEvent`.
    fn to_mouse_event(&self, target: NodeId, pointer: &PointerData) -> ListMouseEvent<T> {
        let index = self.get_item_index_from_event_target(target);
        let element = index.and_then(|i| self.items.borrow().get(i).map(|item| item.element.clone()));
        ListMouseEvent {
            browser_event: BrowserEvent { modifiers: pointer.modifiers, button: pointer.button, target: Some(target), click_count: pointer.click_count, is_keyboard: false, force_event: false },
            is_handled_by_list: Rc::new(Cell::new(false)),
            element,
            index,
        }
    }

    /// O índice da linha que contém `target`, ou nada fora das linhas.
    pub fn index_of_row_node(&self, node: NodeId) -> Option<usize> {
        self.row_indexes.borrow().get(&node).copied()
    }

    /// Registra (ou, com `None`, remove) um nó de fora das linhas como a linha de `index`: o `data-index`
    /// que a linha do sticky scroll da árvore carrega, para os eventos de ponteiro nela resolverem o
    /// elemento (a linha fixada não é uma linha da lista).
    pub fn set_external_row_index(&self, node: NodeId, index: Option<usize>) {
        match index {
            Some(index) => self.row_indexes.borrow_mut().insert(node, index),
            None => self.row_indexes.borrow_mut().remove(&node),
        };
    }

    fn get_item_index_from_event_target(&self, target: NodeId) -> Option<usize> {
        let tree = self.ui.tree();
        let scrollable_element = self.scrollable_element.get_dom_node();
        let mut element = Some(target);

        while let Some(node) = element {
            if node == self.rows_container || !tree.is_ancestor(scrollable_element, node) {
                break;
            }
            if let Some(index) = self.row_indexes.borrow().get(&node) {
                return Some(*index);
            }
            element = tree.parent(node);
        }

        None
    }

    fn on_scroll(&self, e: &ScrollEvent) {
        let previous_render_range = self.get_render_range(self.last_render_top.get(), self.last_render_height.get());
        self.render(previous_render_range, e.scroll_top, e.height, false, true);

        if self.support_dynamic_heights {
            self.rerender_range(e.scroll_top, e.height, e.in_smooth_scrolling);
        }
    }

    fn get_visible_range(&self, render_top: f64, render_height: f64) -> IRange {
        let map = self.range_map.borrow();
        let start = map.index_at(render_top);
        IRange { start: start as f64, end: f64::max(start as f64, map.index_after(render_top + render_height - 1.0) as f64) }
    }

    /// O intervalo de índices visíveis (`getRenderRange`, sem a seleção de texto do usuário).
    fn get_render_range(&self, render_top: f64, render_height: f64) -> IRange {
        self.get_visible_range(render_top, render_height)
    }

    /// `_rerender`: remede as alturas dinâmicas (as do delegate) do intervalo e refaz a renderização.
    fn rerender_range(&self, render_top: f64, render_height: f64, in_smooth_scrolling: bool) {
        let mut previous_render_range = self.get_render_range(render_top, render_height);

        let mut anchor_element_index: Option<usize> = None;
        let mut anchor_element_top_delta = 0.0;

        let update_anchor_element = |previous_render_range: IRange, anchor_element_index: &mut Option<usize>, anchor_element_top_delta: &mut f64| {
            *anchor_element_index = None;
            *anchor_element_top_delta = 0.0;

            // Reserva o elemento do topo; se ele está no topo, âncora nele, senão no seguinte.
            if render_top == self.range_map.borrow().position_at(previous_render_range.start as i64) {
                *anchor_element_index = Some(previous_render_range.start as usize);
                *anchor_element_top_delta = 0.0;
            } else if previous_render_range.end - previous_render_range.start > 1.0 {
                let index = previous_render_range.start as usize + 1;
                *anchor_element_index = Some(index);
                *anchor_element_top_delta = self.element_top(index) - render_top;
            }
        };
        update_anchor_element(previous_render_range, &mut anchor_element_index, &mut anchor_element_top_delta);

        let mut height_diff = 0.0;

        loop {
            let render_range = self.get_render_range(render_top, render_height);

            let mut did_change = false;
            let probed_ids: Vec<u64> = {
                let items = self.items.borrow();
                let end = (render_range.end as usize).min(items.len());
                items[render_range.start as usize..end].iter().map(|i| i.id).collect()
            };
            let dynamic_height_diffs = self.probe_dynamic_heights(render_range);

            let model_did_change = {
                let items = self.items.borrow();
                items.len() < render_range.end as usize
                    || probed_ids.iter().enumerate().any(|(index, id)| items.get(render_range.start as usize + index).map(|i| i.id) != Some(*id))
            };

            if model_did_change {
                // O modelo mudou durante a medição (um renderer reentrou): reaplica o que é seguro e recomeça.
                for (index, id) in probed_ids.iter().enumerate() {
                    let diff = dynamic_height_diffs.get(index).copied().unwrap_or(0.0);
                    let current_index = self.items.borrow().iter().position(|i| i.id == *id);
                    if diff != 0.0
                        && let Some(current_index) = current_index
                    {
                        let size = self.items.borrow()[current_index].size;
                        self.range_map.borrow_mut().splice(current_index, 1, &[IItem { size }]);
                        height_diff += diff;
                    }
                }
                previous_render_range = self.get_render_range(render_top, render_height);
                update_anchor_element(previous_render_range, &mut anchor_element_index, &mut anchor_element_top_delta);
                continue;
            }

            for i in render_range.start as usize..render_range.end as usize {
                let diff = dynamic_height_diffs[i - render_range.start as usize];
                if diff != 0.0 {
                    let size = self.items.borrow()[i].size;
                    self.range_map.borrow_mut().splice(i, 1, &[IItem { size }]);
                }
                height_diff += diff;
                did_change = did_change || diff != 0.0;
            }

            if !did_change {
                if height_diff != 0.0 {
                    self.eventually_update_scroll_dimensions();
                }

                let unrender_ranges = relative_complement(previous_render_range, render_range);

                for range in unrender_ranges {
                    for i in range.start as usize..range.end as usize {
                        if self.items.borrow().get(i).is_some_and(|item| item.row.is_some()) {
                            self.remove_item_from_dom(i, None);
                        }
                    }
                }

                let mut i = render_range.end as i64 - 1;
                while i >= render_range.start as i64 {
                    let idx = i as usize;
                    if idx < self.items.borrow().len() && self.items.borrow()[idx].row.is_none() {
                        self.insert_item_in_dom(idx, None);
                    }
                    i -= 1;
                }

                for i in render_range.start as usize..render_range.end as usize {
                    if self.items.borrow().get(i).is_some_and(|item| item.row.is_some()) {
                        self.update_item_in_dom(i);
                    }
                }

                if let Some(anchor) = anchor_element_index {
                    let delta_scroll_top = self.scrollable.get_future_scroll_position().scroll_top - render_top;
                    let new_scroll_top = self.element_top(anchor) - anchor_element_top_delta + delta_scroll_top;
                    self.set_scroll_top(new_scroll_top, in_smooth_scrolling);
                }

                self.on_did_change_content_height.fire(self.content_height());
                return;
            }
        }
    }

    fn probe_dynamic_height(&self, index: usize) -> f64 {
        self.probe_dynamic_height_from_delegate(index).unwrap_or(0.0)
    }

    /// `probeDynamicHeights`: só a altura que o delegate informa; a medição pela caixa da linha não existe.
    // TODO(integração): medir `offsetHeight` da linha quando o `je-ui` tiver layout intrínseco.
    fn probe_dynamic_heights(&self, range: IRange) -> Vec<f64> {
        let mut diffs = vec![0.0; (range.end - range.start) as usize];
        for index in range.start as usize..range.end as usize {
            if index >= self.items.borrow().len() {
                break;
            }
            if let Some(diff) = self.probe_dynamic_height_from_delegate(index) {
                diffs[index - range.start as usize] = diff;
            }
        }
        diffs
    }

    fn probe_dynamic_height_from_delegate(&self, index: usize) -> Option<f64> {
        let element = self.items.borrow()[index].element.clone();
        let new_size = self.virtual_delegate.get_dynamic_height(&element)?;
        let render_width = self.render_width.get();
        let size = {
            let mut items = self.items.borrow_mut();
            let item = &mut items[index];
            let size = item.size;
            item.size = new_size;
            item.last_dynamic_height_width = Some(render_width);
            size
        };
        if new_size > 0.0 {
            self.virtual_delegate.set_dynamic_height(&element, new_size);
        }
        Some(new_size - size)
    }

    fn should_probe_dynamic_height(&self, item: &Item<T>) -> bool {
        if !item.has_dynamic_height || item.last_dynamic_height_width == Some(self.render_width.get()) {
            return false;
        }
        self.virtual_delegate.has_dynamic_height(&item.element).unwrap_or(true)
    }

    pub fn get_element_dom_id(&self, index: usize) -> String {
        format!("{}_{}", self.dom_id, index)
    }

    /// O botão do ponteiro é primário? (atalho dos controladores).
    pub fn is_primary(button: PointerButton) -> bool {
        button == PointerButton::Primary
    }
}

impl<T: Clone + 'static> IDisposable for ListView<T> {
    fn dispose(&self) {
        if self.disposed.replace(true) {
            return;
        }
        let items = std::mem::take(&mut *self.items.borrow_mut());
        for item in items {
            if let Some(row) = item.row
                && let Some(renderer) = self.renderer(&row.template_id)
            {
                renderer.dispose_element(&item.element, -1, &row.template_data, None);
                renderer.dispose_template(&row.template_data);
            }
        }
        if let Some(disposable) = self.scrollable_element_update_disposable.borrow_mut().take() {
            disposable.dispose();
        }
        self.cache.dispose();
        self.base.dispose();
        self.ui.remove(self.dom_node);
    }

    fn constructor_name(&self) -> &'static str {
        "ListView"
    }
}
