//! Porte de `src/vs/base/browser/ui/table/tableWidget.ts` (e de `table.css`): a [`Table`], uma [`List`] virtualizada com um
//! cabeçalho de colunas redimensionáveis (um [`SplitView`] horizontal) por cima.
//!
//! Estrutura de nós (a mesma do original):
//!
//! ```text
//! .monaco-table .table_id_N
//!   .monaco-split-view2                 o cabeçalho: uma view por coluna (.monaco-table-th)
//!   .monaco-list                        a lista; cada linha tem .monaco-table-tr com um .monaco-table-td por coluna
//! ```
//!
//! Notas de tradução:
//! - **`table.css` e o CSS da folha de estilo do dono.** O `display: flex; flex-direction: column` do `.monaco-table` é a
//!   altura explícita do cabeçalho (`headerRowHeight`) e da lista (o resto) em [`Table::layout`]; o `display: flex` do
//!   `.monaco-table-tr` é a posição de cada `.monaco-table-td` pela soma das larguras das colunas anteriores; o
//!   `overflow: hidden` é o recorte do nó. O `padding-left` e o peso da fonte do cabeçalho, que o dono da tabela escreve na
//!   folha dele (`markers.css`), são as opções [`TableOptions::cell_padding_left`] e [`TableOptions::header_font_weight`]; a
//!   caixa de conteúdo que cada renderer de coluna recebe já exclui o recuo (ver [`ITableRenderer::layout_cell`]).
//! - **`:hover` da linha.** Vira a classe `hover` do nó da linha, mantida pelo renderer de linha da tabela com o ponteiro, para os
//!   renderers de coluna lerem (junto com as classes `focused` e `selected` da lista) no `layout_cell`. Os renderers repetem o
//!   `layout_cell` quando o ponteiro, o foco ou a seleção mudam, que o CSS faria sozinho.
//! - **`createStyleSheet` e `style()`.** O CSS que o original gera para a linha vertical dos sashes (`::before`) não existe; a
//!   borda inferior do cabeçalho (`border-bottom: 1px solid transparent`, com `tree.tableColumnsBorder` no `style()`) é um nó
//!   de uma linha logo abaixo dele. O contorno vertical entre colunas e `tableOddRowsBackground` não são desenhados.
//! - **`scrollbarVisibility: Hidden`** do `SplitView` do cabeçalho não existe nas opções do [`SplitView`] do porte (o
//!   `scrollableElement` dele é só um nó de recorte), que já não mostra barra.
//! - **Eventos.** `onMouseOver`, `onMouseOut`, `onMouseMiddleClick`, `onTouchStart`, `onTap` e o `triggerTypeNavigation` não
//!   existem na [`List`] do porte (navegação por digitação, toque e esses eventos ficam de fora até o `je-ui` ter entrada de
//!   texto e toque). O `onContextMenu` é o do ponteiro (ver `table_types`).
//! - **`getContentHeight` e `getContentWidth`.** São o retângulo resolvido do nó (o `je-ui` não tem borda nem recuo de caixa).

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::{Rc, Weak};

use je_base::event::{Emitter, Event as BaseEvent};
use je_base::lifecycle::{Disposable, DisposableRef, IDisposable, to_disposable};
use je_base::scrollable::ScrollEvent;
use je_render::Rgba;
use je_render::kurbo::Rect;
use je_ui::{EventKind, NodeId, Role, Ui};

use crate::common::{registered_emitter, throw};
use crate::dom::{clear_node, create_detached_el, create_el};
use crate::hover_api::{HoverPlacement, ManagedHoverContentOrFactory, get_base_layer_hover_delegate, get_default_hover_delegate};
use crate::list::list_styles::{IListStyles, LIST_FALLBACK_FOREGROUND, unthemed_list_styles};
use crate::list::list_types::{BrowserEvent, ElementFilter, IListRenderer, IListVirtualDelegate, ListElementRenderDetails, TemplateData};
use crate::list::list_view::ListViewOptionsUpdate;
use crate::list::list_widget::{List, ListOptions};
use crate::splitview::{Orientation, SplitView, SplitViewDescriptor, SplitViewOptions, SplitViewView, ViewDescriptor, ViewRef};
use crate::table::table_types::{ITableColumn, ITableContextMenuEvent, ITableEvent, ITableMouseEvent, ITableRenderer, ITableVirtualDelegate};
use crate::text::{UI_FONT_SIZE, ellipsize, set_node_text, ui_font};

thread_local! {
    static INSTANCE_COUNT: Cell<u64> = const { Cell::new(0) };
}

/// `TableListRenderer.TemplateId`.
const ROW_TEMPLATE_ID: &str = "row";

/// `ITableOptions<TRow>` (que estende `IListOptions<TRow>`) com as duas regras de `markers.css` que o dono da tabela escreve
/// na folha de estilo dele (ver as notas do módulo).
#[derive(Clone)]
pub struct TableOptions<TRow> {
    pub list: ListOptions<TRow>,
    /// `padding-left` do `.monaco-table-th` e do `.monaco-table-td`.
    pub cell_padding_left: f64,
    /// `font-weight` do `.monaco-table-th` (`bold` em `table.css`).
    pub header_font_weight: u16,
}

impl<TRow> Default for TableOptions<TRow> {
    fn default() -> Self {
        Self { list: ListOptions::default(), cell_padding_left: 0.0, header_font_weight: 700 }
    }
}

/// `ITableOptionsUpdate` (que estende `IListOptionsUpdate`).
pub type ITableOptionsUpdate = ListViewOptionsUpdate;

/// `ITableStyles` (que estende `IListStyles`).
pub type ITableStyles = IListStyles;

// ---------------------------------------------------------------------------------------------------------------------
// TableListRenderer
// ---------------------------------------------------------------------------------------------------------------------

/// `RowTemplateData`.
struct RowTemplateData {
    container: NodeId,
    cell_containers: Vec<NodeId>,
    cell_template_data: Vec<TemplateData>,
    /// O ponteiro entrando e saindo da linha (o `:hover`).
    listeners: Vec<DisposableRef>,
}

/// `TableListRenderer<TRow>`.
struct TableListRenderer<TRow: 'static, TCell: 'static> {
    weak_self: Weak<Self>,
    ui: Ui,
    columns: Rc<Vec<ITableColumn<TRow, TCell>>>,
    renderers: Vec<Rc<dyn ITableRenderer<TCell>>>,
    /// A largura de cada coluna: a que o `getColumnSize` dá na criação e a do último `layoutColumn`.
    column_sizes: RefCell<Vec<f64>>,
    cell_padding_left: f64,
    rendered_templates: RefCell<Vec<Rc<RowTemplateData>>>,
}

impl<TRow: 'static, TCell: 'static> TableListRenderer<TRow, TCell> {
    fn new(ui: &Ui, columns: Rc<Vec<ITableColumn<TRow, TCell>>>, renderers: Vec<Rc<dyn ITableRenderer<TCell>>>, get_column_size: impl Fn(usize) -> f64, cell_padding_left: f64) -> Rc<Self> {
        let renderer_map: HashMap<String, Rc<dyn ITableRenderer<TCell>>> = renderers.into_iter().map(|renderer| (renderer.template_id(), renderer)).collect();
        let mut column_renderers = Vec::new();

        for column in columns.iter() {
            let Some(renderer) = renderer_map.get(&column.template_id) else {
                throw(&format!("Table cell renderer for template id {} not found.", column.template_id));
            };

            column_renderers.push(renderer.clone());
        }

        let column_sizes = (0..columns.len()).map(get_column_size).collect();
        Rc::new_cyclic(|weak| Self {
            weak_self: weak.clone(),
            ui: ui.clone(),
            columns,
            renderers: column_renderers,
            column_sizes: RefCell::new(column_sizes),
            cell_padding_left,
            rendered_templates: RefCell::new(Vec::new()),
        })
    }

    fn data(template_data: &TemplateData) -> &RowTemplateData {
        template_data.downcast_ref::<RowTemplateData>().expect("o template do TableListRenderer")
    }

    /// Posiciona as células da linha (`.monaco-table-tr { display: flex }`, cada `.monaco-table-td` com a largura da coluna)
    /// e pede aos renderers das colunas que posicionem o conteúdo.
    fn layout_row(&self, data: &RowTemplateData) {
        let height = self.ui.tree().bounds(data.container).height();
        let sizes = self.column_sizes.borrow().clone();
        let mut x = 0.0;

        for (index, cell) in data.cell_containers.iter().enumerate() {
            let width = sizes[index];
            self.ui.tree_mut().set_bounds(*cell, Rect::new(x, 0.0, x + width, height));
            self.renderers[index].layout_cell(&data.cell_template_data[index], Rect::new(self.cell_padding_left.min(width), 0.0, width, height));
            x += width;
        }
    }

    /// `layoutColumn(index, size)`.
    fn layout_column(&self, index: usize, size: f64) {
        self.column_sizes.borrow_mut()[index] = size;
        self.relayout();
    }

    /// Refaz a posição das células de todas as linhas renderizadas (o estado da linha mudou).
    fn relayout(&self) {
        let templates = self.rendered_templates.borrow().clone();

        for data in templates {
            self.layout_row(&data);
        }
    }

    /// O `:hover` da linha como a classe `hover` (ver as notas do módulo).
    fn set_hovered(&self, container: NodeId, hovered: bool) {
        self.ui.tree_mut().toggle_class(container, "hover", hovered);
        let data = self.rendered_templates.borrow().iter().find(|data| data.container == container).cloned();

        if let Some(data) = data {
            self.layout_row(&data);
        }
    }
}

impl<TRow: 'static, TCell: 'static> IListRenderer<TRow> for TableListRenderer<TRow, TCell> {
    fn template_id(&self) -> String {
        ROW_TEMPLATE_ID.to_owned()
    }

    fn render_template(&self, container: NodeId) -> TemplateData {
        let mut cell_containers = Vec::new();

        {
            let mut tree = self.ui.tree_mut();
            let row_container = create_el(&mut tree, container, Role::Generic, "monaco-table-tr");
            tree.set_fill(row_container, true, true);

            for index in 0..self.columns.len() {
                let cell_container = create_el(&mut tree, row_container, Role::Generic, "monaco-table-td");
                tree.set_attribute(cell_container, "data-col-index", index.to_string());
                tree.style_mut(cell_container).clip = true;
                cell_containers.push(cell_container);
            }
        }

        let cell_template_data = cell_containers.iter().enumerate().map(|(index, cell_container)| self.renderers[index].render_template(*cell_container)).collect();

        let listeners = [EventKind::PointerEnter, EventKind::PointerLeave]
            .into_iter()
            .map(|kind| {
                let weak = self.weak_self.clone();
                self.ui.listen(container, kind, false, move |e| {
                    if let Some(this) = weak.upgrade() {
                        this.set_hovered(container, e.kind == EventKind::PointerEnter);
                    }
                })
            })
            .collect();

        let result = Rc::new(RowTemplateData { container, cell_containers, cell_template_data, listeners });
        self.rendered_templates.borrow_mut().push(result.clone());
        self.layout_row(&result);

        result
    }

    fn render_element(&self, element: &TRow, index: usize, template_data: &TemplateData, render_details: Option<&ListElementRenderDetails>) {
        let data = Self::data(template_data);
        self.layout_row(data);

        for (i, column) in self.columns.iter().enumerate() {
            let cell = (column.project)(element);
            self.renderers[i].render_element(&cell, index, &data.cell_template_data[i], render_details);
        }
    }

    fn dispose_element(&self, element: &TRow, index: i64, template_data: &TemplateData, render_details: Option<&ListElementRenderDetails>) {
        let data = Self::data(template_data);

        for (i, column) in self.columns.iter().enumerate() {
            let cell = (column.project)(element);
            self.renderers[i].dispose_element(&cell, index, &data.cell_template_data[i], render_details);
        }
    }

    fn dispose_template(&self, template_data: &TemplateData) {
        let data = Self::data(template_data);

        for (index, renderer) in self.renderers.iter().enumerate() {
            renderer.dispose_template(&data.cell_template_data[index]);
        }

        for listener in &data.listeners {
            listener.dispose();
        }

        clear_node(&mut self.ui.tree_mut(), data.container);
        self.ui.tree_mut().remove_class(data.container, "hover");
        self.rendered_templates.borrow_mut().retain(|rendered| !std::ptr::addr_eq(Rc::as_ptr(rendered), data as *const RowTemplateData));
    }
}

/// `asListVirtualDelegate(delegate)`.
struct TableListVirtualDelegate<TRow> {
    delegate: Rc<dyn ITableVirtualDelegate<TRow>>,
}

impl<TRow> IListVirtualDelegate<TRow> for TableListVirtualDelegate<TRow> {
    fn get_height(&self, row: &TRow) -> f64 {
        self.delegate.get_height(row)
    }

    fn get_template_id(&self, _row: &TRow) -> String {
        ROW_TEMPLATE_ID.to_owned()
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// ColumnHeader
// ---------------------------------------------------------------------------------------------------------------------

/// `ColumnHeader<TRow, TCell>`: uma view do `SplitView` do cabeçalho.
struct ColumnHeader {
    base: Rc<Disposable>,
    ui: Ui,
    index: usize,
    label: String,
    /// `.monaco-table-th`.
    element: NodeId,
    /// O texto, dentro do `.monaco-table-th` depois do `padding-left`.
    label_node: NodeId,
    header_height: f64,
    cell_padding_left: f64,
    font_weight: u16,
    minimum_width: Option<f64>,
    maximum_width: Option<f64>,
    on_did_change_width_constraints: Option<BaseEvent<()>>,
    on_did_layout: Rc<Emitter<(usize, f64)>>,
    foreground: Cell<Rgba>,
    size: Cell<f64>,
}

impl ColumnHeader {
    fn new<TRow, TCell>(ui: &Ui, index: usize, column: &ITableColumn<TRow, TCell>, header_height: f64, options_cell_padding_left: f64, font_weight: u16) -> Rc<Self> {
        let base = Disposable::new();
        let (element, label_node) = {
            let mut tree = ui.tree_mut();
            let element = create_detached_el(&mut tree, Role::Generic, "monaco-table-th");
            tree.set_attribute(element, "data-col-index", index.to_string());
            tree.set_fill(element, true, true);
            tree.style_mut(element).clip = true;
            let label_node = create_el(&mut tree, element, Role::Generic, "");
            (element, label_node)
        };

        if let Some(tooltip) = column.tooltip.as_deref().filter(|tooltip| !tooltip.is_empty()) {
            let hover = get_base_layer_hover_delegate().setup_managed_hover(get_default_hover_delegate(HoverPlacement::Mouse), element, ManagedHoverContentOrFactory::from(tooltip), None);
            base.register(to_disposable(move || hover.dispose()));
        }

        let on_did_layout = registered_emitter(&base);
        let this = Rc::new(Self {
            base,
            ui: ui.clone(),
            index,
            label: column.label.clone(),
            element,
            label_node,
            header_height,
            cell_padding_left: options_cell_padding_left,
            font_weight,
            minimum_width: column.minimum_width,
            maximum_width: column.maximum_width,
            on_did_change_width_constraints: column.on_did_change_width_constraints.clone(),
            on_did_layout,
            foreground: Cell::new(LIST_FALLBACK_FOREGROUND),
            size: Cell::new(0.0),
        });
        this.render_label();
        this
    }

    /// O texto do cabeçalho, cortado com `…` na largura da coluna (`text-overflow: ellipsis`), centrado na altura dele.
    fn render_label(&self) {
        let font = ui_font(UI_FONT_SIZE, self.font_weight);
        let width = (self.size.get() - self.cell_padding_left).max(0.0);
        let text = ellipsize(&self.label, &font, width);
        let mut tree = self.ui.tree_mut();
        set_node_text(&mut tree, self.label_node, &text, &font, self.foreground.get(), self.header_height);
        tree.set_bounds(self.label_node, Rect::new(self.cell_padding_left, 0.0, self.cell_padding_left + width, self.header_height));
    }

    fn set_foreground(&self, foreground: Rgba) {
        self.foreground.set(foreground);
        self.render_label();
    }
}

impl SplitViewView<()> for ColumnHeader {
    fn element(&self) -> NodeId {
        self.element
    }

    fn minimum_size(&self) -> f64 {
        self.minimum_width.unwrap_or(120.0)
    }

    fn maximum_size(&self) -> f64 {
        self.maximum_width.unwrap_or(f64::INFINITY)
    }

    fn on_did_change(&self) -> BaseEvent<Option<f64>> {
        match &self.on_did_change_width_constraints {
            Some(event) => event.map(|_| None, None),
            None => BaseEvent::none(),
        }
    }

    fn layout(&self, size: f64, _offset: f64, _context: Option<&()>) {
        self.size.set(size);
        self.render_label();
        self.on_did_layout.fire((self.index, size));
    }
}

impl IDisposable for ColumnHeader {
    fn dispose(&self) {
        self.base.dispose();
    }

    fn constructor_name(&self) -> &'static str {
        "ColumnHeader"
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// Table
// ---------------------------------------------------------------------------------------------------------------------

/// `Table<TRow>`.
pub struct Table<TRow: Clone + 'static, TCell: 'static> {
    base: Rc<Disposable>,
    ui: Ui,
    /// `domId`.
    pub dom_id: String,
    /// `domNode`.
    pub dom_node: NodeId,
    virtual_delegate: Rc<dyn ITableVirtualDelegate<TRow>>,
    columns: Rc<Vec<ITableColumn<TRow, TCell>>>,
    headers: Vec<Rc<ColumnHeader>>,
    splitview: Rc<SplitView<()>>,
    list: Rc<List<TRow>>,
    /// A borda inferior do cabeçalho.
    header_border: NodeId,
    cached_width: Rc<Cell<f64>>,
    cached_height: Rc<Cell<f64>>,
}

impl<TRow: Clone + 'static, TCell: 'static> Table<TRow, TCell> {
    pub fn new(
        user: &str,
        ui: &Ui,
        container: NodeId,
        virtual_delegate: Rc<dyn ITableVirtualDelegate<TRow>>,
        columns: Vec<ITableColumn<TRow, TCell>>,
        renderers: Vec<Rc<dyn ITableRenderer<TCell>>>,
        options: TableOptions<TRow>,
    ) -> Rc<Self> {
        let base = Disposable::new();
        let dom_id = INSTANCE_COUNT.with(|count| {
            count.set(count.get() + 1);
            format!("table_id_{}", count.get())
        });
        let dom_node = {
            let mut tree = ui.tree_mut();
            let dom_node = create_el(&mut tree, container, Role::Generic, "monaco-table");
            tree.add_class(dom_node, &dom_id);
            tree.set_fill(dom_node, true, true);
            tree.style_mut(dom_node).clip = true;
            dom_node
        };
        let header_row_height = virtual_delegate.header_row_height();
        let columns = Rc::new(columns);

        let headers: Vec<Rc<ColumnHeader>> = columns
            .iter()
            .enumerate()
            .map(|(index, column)| {
                let header = ColumnHeader::new(ui, index, column, header_row_height, options.cell_padding_left, options.header_font_weight);
                base.register(header.clone());
                header
            })
            .collect();
        let descriptor = SplitViewDescriptor {
            size: columns.iter().map(|column| column.weight).sum(),
            views: headers.iter().zip(columns.iter()).map(|(header, column)| ViewDescriptor { visible: None, size: column.weight, view: header.clone() as ViewRef<()> }).collect(),
        };

        let cached_width = Rc::new(Cell::new(0.0));
        let cached_height = Rc::new(Cell::new(0.0));
        let splitview = {
            let cached_height = cached_height.clone();
            SplitView::<()>::new(
                ui,
                dom_node,
                SplitViewOptions {
                    orientation: Some(Orientation::Horizontal),
                    descriptor: Some(descriptor),
                    get_sash_orthogonal_size: Some(Rc::new(move || cached_height.get())),
                    ..SplitViewOptions::default()
                },
            )
        };
        base.register(splitview.clone());

        // this.splitview.el.style.height = `${virtualDelegate.headerRowHeight}px`
        let header_border = {
            let mut tree = ui.tree_mut();
            let element = splitview.element();
            tree.set_fill(element, true, false);
            tree.set_bounds(element, Rect::new(0.0, 0.0, 0.0, header_row_height));
            let header_border = create_el(&mut tree, dom_node, Role::Generic, "");
            tree.set_fill(header_border, true, false);
            header_border
        };

        let renderer = {
            let splitview = splitview.clone();
            TableListRenderer::new(ui, columns.clone(), renderers, move |index| splitview.get_view_size(index), options.cell_padding_left)
        };
        let list = List::new(
            user,
            ui,
            dom_node,
            Rc::new(TableListVirtualDelegate { delegate: virtual_delegate.clone() }),
            vec![renderer.clone() as Rc<dyn IListRenderer<TRow>>],
            options.list,
        );
        base.register(list.clone());

        for header in &headers {
            let renderer = renderer.clone();
            base.register(header.on_did_layout.event().listen(move |&(index, size)| renderer.layout_column(index, size)));
        }

        {
            let (weak_splitview, columns, cached_width) = (Rc::downgrade(&splitview), columns.clone(), cached_width.clone());
            base.register(splitview.on_did_sash_reset().listen(move |&index| {
                let Some(splitview) = weak_splitview.upgrade() else { return };
                let total_weight: f64 = columns.iter().map(|column| column.weight).sum();
                let size = columns[index].weight / total_weight * cached_width.get();
                splitview.resize_view(index, size);
            }));
        }

        // O CSS reagia ao foco, à seleção e ao ponteiro da linha; aqui a linha reposiciona o conteúdo das células.
        {
            let renderer = renderer.clone();
            base.register(list.on_did_change_focus().listen(move |_| renderer.relayout()));
        }
        {
            let renderer = renderer.clone();
            base.register(list.on_did_change_selection().listen(move |_| renderer.relayout()));
        }

        let this = Rc::new(Self {
            base,
            ui: ui.clone(),
            dom_id,
            dom_node,
            virtual_delegate,
            columns,
            headers,
            splitview,
            list,
            header_border,
            cached_width,
            cached_height,
        });
        this.style(&unthemed_list_styles());
        this
    }

    fn node_event(&self, kind: EventKind) -> BaseEvent<()> {
        let (ui, node) = (self.ui.clone(), self.list.get_html_element());
        BaseEvent::new(move |listener| ui.listen(node, kind, false, move |_| listener(&())))
    }

    /// `onDidChangeFocus`.
    pub fn on_did_change_focus(&self) -> BaseEvent<ITableEvent<TRow>> {
        self.list.on_did_change_focus()
    }

    /// `onDidChangeSelection`.
    pub fn on_did_change_selection(&self) -> BaseEvent<ITableEvent<TRow>> {
        self.list.on_did_change_selection()
    }

    /// `onDidScroll`.
    pub fn on_did_scroll(&self) -> BaseEvent<ScrollEvent> {
        self.list.view.on_did_scroll()
    }

    /// `onMouseClick`.
    pub fn on_mouse_click(&self) -> BaseEvent<ITableMouseEvent<TRow>> {
        self.list.view.on_mouse_click()
    }

    /// `onMouseDblClick`.
    pub fn on_mouse_dbl_click(&self) -> BaseEvent<ITableMouseEvent<TRow>> {
        self.list.view.on_mouse_dbl_click()
    }

    /// `onPointer`.
    pub fn on_pointer(&self) -> BaseEvent<ITableMouseEvent<TRow>> {
        self.list.on_pointer()
    }

    /// `onMouseUp`.
    pub fn on_mouse_up(&self) -> BaseEvent<ITableMouseEvent<TRow>> {
        self.list.view.on_mouse_up()
    }

    /// `onMouseDown`.
    pub fn on_mouse_down(&self) -> BaseEvent<ITableMouseEvent<TRow>> {
        self.list.view.on_mouse_down()
    }

    /// `onMouseMove`.
    pub fn on_mouse_move(&self) -> BaseEvent<ITableMouseEvent<TRow>> {
        self.list.view.on_mouse_move()
    }

    /// `onContextMenu`.
    pub fn on_context_menu(&self) -> BaseEvent<ITableContextMenuEvent<TRow>> {
        self.list.view.on_context_menu()
    }

    /// `onDidFocus`.
    pub fn on_did_focus(&self) -> BaseEvent<()> {
        self.node_event(EventKind::Focus)
    }

    /// `onDidBlur`.
    pub fn on_did_blur(&self) -> BaseEvent<()> {
        self.node_event(EventKind::Blur)
    }

    /// `onDidDispose`.
    pub fn on_did_dispose(&self) -> BaseEvent<()> {
        self.list.on_did_dispose()
    }

    /// `get scrollTop()`.
    pub fn scroll_top(&self) -> f64 {
        self.list.scroll_top()
    }

    /// `set scrollTop(scrollTop)`.
    pub fn set_scroll_top(&self, scroll_top: f64) {
        self.list.set_scroll_top(scroll_top);
    }

    /// `get scrollLeft()`.
    pub fn scroll_left(&self) -> f64 {
        self.list.view.get_scroll_left()
    }

    /// `set scrollLeft(scrollLeft)`.
    pub fn set_scroll_left(&self, scroll_left: f64) {
        self.list.view.set_scroll_left(scroll_left);
    }

    /// `get scrollHeight()`.
    pub fn scroll_height(&self) -> f64 {
        self.list.scroll_height()
    }

    /// `get renderHeight()`.
    pub fn render_height(&self) -> f64 {
        self.list.render_height()
    }

    /// `getColumnLabels()`.
    pub fn get_column_labels(&self) -> Vec<String> {
        self.columns.iter().map(|column| column.label.clone()).collect()
    }

    /// `resizeColumn(index, percentage)`.
    pub fn resize_column(&self, index: usize, percentage: f64) {
        let size = crate::common::js_round(percentage / 100.0 * self.cached_width.get());
        self.splitview.resize_view(index, size);
    }

    /// `updateOptions(options)`.
    pub fn update_options(&self, options: &ITableOptionsUpdate) {
        self.list.view.update_options(options);
    }

    /// `splice(start, deleteCount, elements)`.
    pub fn splice(&self, start: usize, delete_count: usize, elements: &[TRow]) {
        self.list.splice(start, delete_count, elements);
    }

    /// `rerender()`.
    pub fn rerender(&self) {
        self.list.rerender();
    }

    /// `row(index)`.
    pub fn row(&self, index: usize) -> TRow {
        self.list.element(index)
    }

    /// `indexOf(element)`: o primeiro índice cujo elemento satisfaz `is_element` (o original compara por identidade).
    pub fn index_of(&self, is_element: impl Fn(&TRow) -> bool) -> Option<usize> {
        self.list.view.index_of(is_element)
    }

    /// `get length()`.
    pub fn length(&self) -> usize {
        self.list.length()
    }

    /// `getHTMLElement()`.
    pub fn get_html_element(&self) -> NodeId {
        self.dom_node
    }

    /// O `.monaco-list` da tabela (o `domNode.querySelector('.monaco-list')`, de que o dono da tabela pega a lista e as linhas).
    pub fn list_html_element(&self) -> NodeId {
        self.list.get_html_element()
    }

    /// `layout(height?, width?)`: sem altura ou largura valem as do nó.
    pub fn layout(&self, height: Option<f64>, width: Option<f64>) {
        let bounds = self.ui.tree().bounds(self.dom_node);
        let height = height.unwrap_or(bounds.height());
        let width = width.unwrap_or(bounds.width());

        self.cached_width.set(width);
        self.cached_height.set(height);
        self.splitview.layout(width, None);

        let header_row_height = self.virtual_delegate.header_row_height();
        let list_height = height - header_row_height;
        {
            let mut tree = self.ui.tree_mut();
            tree.set_bounds(self.header_border, Rect::new(0.0, header_row_height - 1.0, 0.0, header_row_height));
            let list_node = self.list.get_html_element();
            tree.set_fill(list_node, true, false);
            tree.set_bounds(list_node, Rect::new(0.0, header_row_height, 0.0, header_row_height + list_height));
        }
        self.list.layout(Some(list_height), Some(width));
    }

    /// `style(styles)`.
    pub fn style(&self, styles: &ITableStyles) {
        let foreground = styles.foreground.unwrap_or(LIST_FALLBACK_FOREGROUND);

        for header in &self.headers {
            header.set_foreground(foreground);
        }

        self.ui.tree_mut().style_mut(self.header_border).background = styles.table_columns_border;
        self.list.style(styles);
    }

    /// `domFocus()`.
    pub fn dom_focus(&self) {
        self.list.dom_focus();
    }

    /// `setAnchor(index)`.
    pub fn set_anchor(&self, index: Option<usize>) {
        self.list.set_anchor(index);
    }

    /// `getAnchor()`.
    pub fn get_anchor(&self) -> Option<usize> {
        self.list.get_anchor()
    }

    /// `getSelectedElements()`.
    pub fn get_selected_elements(&self) -> Vec<TRow> {
        self.list.get_selected_elements()
    }

    /// `setSelection(indexes, browserEvent?)`.
    pub fn set_selection(&self, indexes: Vec<usize>, browser_event: Option<BrowserEvent>) {
        self.list.set_selection(indexes, browser_event);
    }

    /// `getSelection()`.
    pub fn get_selection(&self) -> Vec<usize> {
        self.list.get_selection()
    }

    /// `setFocus(indexes, browserEvent?)`.
    pub fn set_focus(&self, indexes: Vec<usize>, browser_event: Option<BrowserEvent>) {
        self.list.set_focus(indexes, browser_event);
    }

    /// `focusNext(n = 1, loop = false, browserEvent?)`.
    pub fn focus_next(&self, n: usize, do_loop: bool, browser_event: Option<BrowserEvent>) {
        self.list.focus_next(n, do_loop, browser_event, None);
    }

    /// `focusPrevious(n = 1, loop = false, browserEvent?)`.
    pub fn focus_previous(&self, n: usize, do_loop: bool, browser_event: Option<BrowserEvent>) {
        self.list.focus_previous(n, do_loop, browser_event, None);
    }

    /// `focusNextPage(browserEvent?)` (a promessa do original não existe, ver a lista).
    pub fn focus_next_page(&self, browser_event: Option<BrowserEvent>) {
        self.list.focus_next_page(browser_event, None::<ElementFilter<TRow>>);
    }

    /// `focusPreviousPage(browserEvent?)` (a promessa do original não existe, ver a lista).
    pub fn focus_previous_page(&self, browser_event: Option<BrowserEvent>) {
        self.list.focus_previous_page(browser_event, None::<ElementFilter<TRow>>, Rc::new(|| 0.0));
    }

    /// `focusFirst(browserEvent?)`.
    pub fn focus_first(&self, browser_event: Option<BrowserEvent>) {
        self.list.focus_first(browser_event, None);
    }

    /// `focusLast(browserEvent?)`.
    pub fn focus_last(&self, browser_event: Option<BrowserEvent>) {
        self.list.focus_last(browser_event, None);
    }

    /// `getFocus()`.
    pub fn get_focus(&self) -> Vec<usize> {
        self.list.get_focus()
    }

    /// `getFocusedElements()`.
    pub fn get_focused_elements(&self) -> Vec<TRow> {
        self.list.get_focused_elements()
    }

    /// `getRelativeTop(index)`: lança para um índice fora da tabela, como a lista.
    pub fn get_relative_top(&self, index: usize) -> Option<f64> {
        self.list.get_relative_top(index, 0.0)
    }

    /// `reveal(index, relativeTop?)`.
    pub fn reveal(&self, index: usize, relative_top: Option<f64>) {
        self.list.reveal(index, relative_top, 0.0);
    }
}

impl<TRow: Clone + 'static, TCell: 'static> IDisposable for Table<TRow, TCell> {
    fn dispose(&self) {
        self.base.dispose();
    }

    fn constructor_name(&self) -> &'static str {
        "Table"
    }
}
