//! Porte de `src/vs/platform/quickinput/browser/tree/quickInputTreeController.ts`: o `QuickInputTreeController`, que guarda a
//! árvore do `QuickTree` (itens, filtro, ordenação, colapso, foco, seleção e marcação com três estados).
//!
//! Notas de tradução:
//! - **`WorkbenchObjectTree`.** O original põe os itens numa árvore de DOM com renderizadores. Aqui a árvore é o modelo de nós
//!   (`TreeNode`, num vetor, com pai e filhos por índice), com as mesmas regras do `IndexTreeModel`: o filtro devolve
//!   `Visible`, `Hidden` ou `Recurse` (um nó `Recurse` é visível se algum descendente for), o `Hidden` não avalia os filhos, e
//!   as linhas visíveis são as dos nós visíveis sem ancestral colapsado. O desenho nativo lê as linhas por
//!   [`QuickInputTreeController::rows`], com os dados do [`super::quick_input_tree_renderer::QuickInputTreeRenderer`]. A
//!   rolagem usa as alturas do [`QuickInputTreeDelegate`] e o `max-height` de `layout()`.
//! - **Itens por valor.** `IQuickTreeItem` é copiado para o modelo (os `children` ficam só como presença: `None` ou vazio) e o
//!   controlador muda o `checked` da cópia. Os eventos entregam cópias, e um item é identificado pelo `id`: o item sem `id`
//!   ganha `__generated_N` (o `QuickInputTreeIdentityProvider`), gravado na cópia. O `collapsed`, o foco e a seleção sobrevivem
//!   a um novo `setTreeData` quando o `id` continua na árvore (`PreserveOrCollapsed` e `PreserveOrExpanded`).
//! - **Foco.** `onDidChangeFocus` dispara a cada `setFocus` e quando o item focado some das linhas; a seleção acompanha o foco
//!   (`registerOnDidChangeFocus`). `domFocus()` é a marca [`QuickInputTreeController::has_dom_focus`], e o `getActiveDescendant()` é o `id` da linha
//!   em foco (`QuickInputTree_<posição>`), porque a lista não existe aqui.
//! - **Páginas.** `focusNextPage` e `focusPreviousPage` repetem a volta do `ListWidget` na hora, sem o `timeout(0)` entre rolar e
//!   focar (o mesmo caminho do [`crate::quickinput::quick_input_list`]).
//! - **Fica de fora** a configuração de `WorkbenchObjectTree` que só afeta DOM (`horizontalScrolling`, `findWidgetEnabled`,
//!   `alwaysConsumeMouseWheel`, `renderIndentGuides`, `expandOnDoubleClick`, `expandOnlyOnTwistieClick`,
//!   `disableExpandOnSpacebar`): o desenho decide, e `hideTwistiesOfChildlessElements` é `QuickTreeRow::collapsible`.

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet, VecDeque};
use std::rc::Rc;

use je_base::event::{Emitter, Event};
use je_base::lifecycle::{Disposable, IDisposable};

use super::quick_input_delegate::QuickInputTreeDelegate;
use super::quick_input_tree::{QuickTreeFilterData, get_parent_node_state};
use super::quick_input_tree_accessibility_provider::QuickTreeAccessibilityProvider;
use super::quick_input_tree_filter::{QuickInputTreeFilter, TreeVisibility};
use super::quick_input_tree_renderer::{QuickInputCheckboxStateHandler, QuickInputTreeRenderer, QuickTreeEntryView};
use super::quick_input_tree_sorter::QuickInputTreeSorter;
use crate::contextview::view_element::ViewElement;
use crate::hover::hover_service::IHoverDelegate;
use crate::quickinput::quick_input::{IQuickTreeCheckboxEvent, IQuickTreeItem, IQuickTreeItemButtonEvent, QuickPickFocus, QuickTreeItemChecked};

const FLAT_HIERARCHY_CLASS: &str = "quick-input-tree-flat";
/// O prefixo do `id` da linha em foco (`aria-activedescendant`).
const ACTIVE_DESCENDANT_PREFIX: &str = "QuickInputTree";

struct TreeNode {
    id: String,
    item: IQuickTreeItem,
    parent: Option<usize>,
    children: Vec<usize>,
    collapsible: bool,
    collapsed: bool,
    visible: bool,
    filter_data: QuickTreeFilterData,
}

#[derive(Default)]
struct TreeState {
    nodes: Vec<TreeNode>,
    roots: Vec<usize>,
    focus: Option<usize>,
    selection: Vec<usize>,
    scroll_top: f64,
    /// O `max-height` de `layout()` (já com o arredondamento e o `+ 6`).
    max_height: Option<f64>,
    generated_counter: usize,
    dom_focused: bool,
}

/// Uma linha visível da árvore.
#[derive(Clone)]
pub struct QuickTreeRow {
    pub item: IQuickTreeItem,
    pub depth: usize,
    /// `hideTwistiesOfChildlessElements`: só há seta quando o item tem filhos.
    pub collapsible: bool,
    pub collapsed: bool,
    pub focused: bool,
    pub selected: bool,
    pub entry: QuickTreeEntryView,
    pub height: f64,
}

impl TreeState {
    fn find(&self, item: &IQuickTreeItem) -> Option<usize> {
        let id = item.base.id.as_ref()?;
        self.nodes.iter().position(|node| &node.id == id)
    }

    fn children_of(&self, parent: Option<usize>) -> &[usize] {
        match parent {
            Some(parent) => &self.nodes[parent].children,
            None => &self.roots,
        }
    }

    /// As linhas: os nós visíveis sem ancestral colapsado, em profundidade.
    fn rows(&self) -> Vec<usize> {
        fn walk(state: &TreeState, nodes: &[usize], out: &mut Vec<usize>) {
            for &index in nodes {
                let node = &state.nodes[index];
                if !node.visible {
                    continue;
                }
                out.push(index);
                if !node.collapsed {
                    walk(state, &node.children, out);
                }
            }
        }
        let mut out = Vec::new();
        walk(self, &self.roots, &mut out);
        out
    }

    fn depth(&self, mut index: usize) -> usize {
        let mut depth = 0;
        while let Some(parent) = self.nodes[index].parent {
            depth += 1;
            index = parent;
        }
        depth
    }

    fn is_descendant_of(&self, mut index: usize, ancestor: usize) -> bool {
        while let Some(parent) = self.nodes[index].parent {
            if parent == ancestor {
                return true;
            }
            index = parent;
        }
        false
    }
}

pub struct QuickInputTreeController {
    base: Rc<Disposable>,
    renderer: Rc<QuickInputTreeRenderer>,
    checkbox_state_handler: Rc<QuickInputCheckboxStateHandler>,
    filter: QuickInputTreeFilter,
    sorter: Rc<QuickInputTreeSorter>,
    delegate: QuickInputTreeDelegate,
    accessibility_provider: QuickTreeAccessibilityProvider,
    on_did_trigger_button: Rc<Emitter<IQuickTreeItemButtonEvent>>,
    on_did_change_checkbox_state: Rc<Emitter<IQuickTreeCheckboxEvent>>,
    on_did_checked_leaf_items_change: Rc<Emitter<Vec<IQuickTreeItem>>>,
    on_did_change_focus: Rc<Emitter<Vec<IQuickTreeItem>>>,
    on_did_change_content_height: Rc<Emitter<()>>,
    on_leave: Rc<Emitter<()>>,
    on_did_accept: Rc<Emitter<()>>,
    hover_delegate: Option<Rc<dyn IHoverDelegate>>,
    content_height: Cell<f64>,
    container: Rc<ViewElement>,
    tree_element: Rc<ViewElement>,
    state: RefCell<TreeState>,
    next_generated_id: Cell<usize>,
}

impl QuickInputTreeController {
    /// `new QuickInputTreeController(container, hoverDelegate, styles)`. Os estilos do toggle pertencem ao desenho das linhas;
    /// `is_dark` é o `isDark(themeService.getColorTheme().type)` do renderizador.
    pub fn new(container: &Rc<ViewElement>, hover_delegate: Option<Rc<dyn IHoverDelegate>>, is_dark: Rc<dyn Fn() -> bool>) -> Rc<Self> {
        let base = Disposable::new();
        let tree_container = ViewElement::with_classes(&["quick-input-tree"]);
        tree_container.set_parent(Some(container.clone()));
        let tree_element = ViewElement::new();
        tree_element.set_parent(Some(tree_container.clone()));

        let on_did_trigger_button = Emitter::new();
        let on_did_change_checkbox_state = Emitter::new();
        let on_did_checked_leaf_items_change = Emitter::new();
        let on_did_change_focus = Emitter::new();
        let on_did_change_content_height = Emitter::new();
        let on_leave = Emitter::new();
        let on_did_accept = Emitter::new();
        base.register(on_did_trigger_button.clone());
        base.register(on_did_change_checkbox_state.clone());
        base.register(on_did_checked_leaf_items_change.clone());
        base.register(on_did_change_focus.clone());
        base.register(on_did_change_content_height.clone());
        base.register(on_leave.clone());
        base.register(on_did_accept.clone());

        let checkbox_state_handler = QuickInputCheckboxStateHandler::new();
        base.register(checkbox_state_handler.clone());
        let renderer = QuickInputTreeRenderer::new(is_dark, on_did_trigger_button.clone());
        base.register(renderer.clone());
        let sorter = QuickInputTreeSorter::new();
        base.register(sorter.clone());

        let this = Rc::new(Self {
            base,
            renderer,
            checkbox_state_handler,
            filter: QuickInputTreeFilter::new(),
            sorter,
            delegate: QuickInputTreeDelegate,
            accessibility_provider: QuickTreeAccessibilityProvider::new(on_did_change_checkbox_state.event()),
            on_did_trigger_button,
            on_did_change_checkbox_state,
            on_did_checked_leaf_items_change,
            on_did_change_focus,
            on_did_change_content_height,
            on_leave,
            on_did_accept,
            hover_delegate,
            content_height: Cell::new(0.0),
            container: tree_container,
            tree_element,
            state: RefCell::new(TreeState::default()),
            next_generated_id: Cell::new(0),
        });

        let weak = Rc::downgrade(&this);
        this.base.register(this.renderer.on_did_dispose_focused_element().listen({
            let weak = weak.clone();
            move |_| {
                if let Some(this) = weak.upgrade() {
                    this.dom_focus();
                }
            }
        }));
        // registerCheckboxStateListeners
        this.base.register(this.checkbox_state_handler.on_did_change_checkbox_state().listen({
            let weak = weak.clone();
            move |event| {
                let Some(this) = weak.upgrade() else { return };
                let Some(node) = this.state.borrow().find(&event.item) else { return };
                this.update_checkbox_state(node, event.checked == QuickTreeItemChecked::Bool(true), true);
                this.set_focus_node(Some(node));
            }
        }));
        this
    }

    // -----------------------------------------------------------------------------------------
    // Contratos e eventos.
    // -----------------------------------------------------------------------------------------

    pub fn renderer(&self) -> &Rc<QuickInputTreeRenderer> {
        &self.renderer
    }

    pub fn accessibility_provider(&self) -> &QuickTreeAccessibilityProvider {
        &self.accessibility_provider
    }

    pub fn delegate(&self) -> &QuickInputTreeDelegate {
        &self.delegate
    }

    /// O elemento `.quick-input-tree`.
    pub fn container(&self) -> &Rc<ViewElement> {
        &self.container
    }

    /// `this._tree.getHTMLElement()`.
    pub fn tree_element(&self) -> &Rc<ViewElement> {
        &self.tree_element
    }

    pub fn on_did_trigger_button(&self) -> Event<IQuickTreeItemButtonEvent> {
        self.on_did_trigger_button.event()
    }

    pub fn on_did_change_checkbox_state(&self) -> Event<IQuickTreeCheckboxEvent> {
        self.on_did_change_checkbox_state.event()
    }

    pub fn on_did_change_checked_leaf_items(&self) -> Event<Vec<IQuickTreeItem>> {
        self.on_did_checked_leaf_items_change.event()
    }

    /// `this._tree.onDidChangeFocus`: os itens em foco.
    pub fn on_did_change_focus(&self) -> Event<Vec<IQuickTreeItem>> {
        self.on_did_change_focus.event()
    }

    /// `this._tree.onDidChangeContentHeight`.
    pub fn on_did_change_content_height(&self) -> Event<()> {
        self.on_did_change_content_height.event()
    }

    /// O `hoverDelegate` do construtor, para o desenho das linhas (o `IconLabel` e a `ToolBar`).
    pub fn hover_delegate(&self) -> Option<&Rc<dyn IHoverDelegate>> {
        self.hover_delegate.as_ref()
    }

    /// Event that is fired when the tree would no longer have focus.
    pub fn on_leave(&self) -> Event<()> {
        self.on_leave.event()
    }

    /// Event that is fired when a non-pickable item is clicked, indicating acceptance.
    pub fn on_did_accept(&self) -> Event<()> {
        self.on_did_accept.event()
    }

    pub fn displayed(&self) -> bool {
        !self.container.is_hidden()
    }

    pub fn set_displayed(&self, value: bool) {
        if value {
            self.container.show();
        } else {
            self.container.hide();
        }
    }

    pub fn sort_by_label(&self) -> bool {
        self.sorter.sort_by_label()
    }

    pub fn set_sort_by_label(&self, value: bool) {
        self.sorter.set_sort_by_label(value);
        // this._tree.resort(null, true)
        self.resort_state(&mut self.state.borrow_mut());
        self.after_rows_change();
    }

    /// `getActiveDescendant()`: o `id` da linha em foco, ou `None`.
    pub fn get_active_descendant(&self) -> Option<String> {
        let state = self.state.borrow();
        let focus = state.focus?;
        state.rows().iter().position(|&row| row == focus).map(|position| format!("{ACTIVE_DESCENDANT_PREFIX}_{position}"))
    }

    pub fn filter(&self, input: &str) {
        self.filter.set_filter_value(input);
        self.refilter();
    }

    pub fn update_filter_options(&self, match_on_label: Option<bool>, match_on_description: Option<bool>) {
        if let Some(value) = match_on_label {
            self.filter.set_match_on_label(value);
        }
        if let Some(value) = match_on_description {
            self.filter.set_match_on_description(value);
        }
        self.refilter();
    }

    // -----------------------------------------------------------------------------------------
    // O modelo da árvore.
    // -----------------------------------------------------------------------------------------

    pub fn set_tree_data(&self, tree_data: &[IQuickTreeItem]) {
        let mut has_nested_items = false;
        {
            let mut state = self.state.borrow_mut();
            let previous: HashMap<String, bool> = state.nodes.iter().map(|node| (node.id.clone(), node.collapsed)).collect();
            let focused_id = state.focus.map(|focus| state.nodes[focus].id.clone());
            let selected_ids: Vec<String> = state.selection.iter().map(|&selected| state.nodes[selected].id.clone()).collect();
            state.nodes.clear();
            state.roots.clear();
            let mut roots = Vec::new();
            for item in tree_data {
                roots.push(self.create_tree_element(&mut state, item, None, &previous, &mut has_nested_items));
            }
            state.roots = roots;
            self.resort_state(&mut state);
            state.focus = focused_id.and_then(|id| state.nodes.iter().position(|node| node.id == id));
            let selection: Vec<usize> = selected_ids.iter().filter_map(|id| state.nodes.iter().position(|node| &node.id == id)).collect();
            state.selection = selection;
            self.refilter_state(&mut state);
        }
        if has_nested_items {
            self.container.class_list_remove(FLAT_HIERARCHY_CLASS);
        } else {
            self.container.class_list_add(FLAT_HIERARCHY_CLASS);
        }
        self.after_rows_change();
    }

    /// O `createTreeElement` de `setTreeData`.
    fn create_tree_element(&self, state: &mut TreeState, item: &IQuickTreeItem, parent: Option<usize>, previous: &HashMap<String, bool>, has_nested_items: &mut bool) -> usize {
        let id = match &item.base.id {
            Some(id) => id.clone(),
            None => {
                let generated = self.next_generated_id.get();
                self.next_generated_id.set(generated + 1);
                format!("__generated_{generated}")
            }
        };
        let mut stored = item.clone();
        stored.base.id = Some(id.clone());
        // Só a presença dos filhos fica no item guardado.
        stored.children = item.children.as_ref().map(|_| Vec::new());
        let index = state.nodes.len();
        state.nodes.push(TreeNode { id: id.clone(), item: stored, parent, children: Vec::new(), collapsible: false, collapsed: false, visible: true, filter_data: QuickTreeFilterData::default() });

        let has_children = item.children.as_ref().is_some_and(|children| !children.is_empty());
        if has_children {
            *has_nested_items = true;
            let mut children = Vec::new();
            for child in item.children.iter().flatten() {
                children.push(self.create_tree_element(state, child, Some(index), previous, has_nested_items));
            }
            let checked = get_parent_node_state(children.iter().map(|&child| state.nodes[child].item.checked));
            state.nodes[index].item.checked = Some(checked);
            state.nodes[index].children = children;
        }
        let node = &mut state.nodes[index];
        node.collapsible = has_children;
        node.collapsed = has_children && previous.get(&id).copied().unwrap_or(item.collapsed == Some(true));
        index
    }

    /// `tree.resort(null, true)`: ordena a raiz e todos os níveis abaixo dela.
    fn resort_state(&self, state: &mut TreeState) {
        let roots = state.roots.clone();
        self.sort_level(state, &roots, true);
        state.roots = self.sorted(state, roots);
    }

    /// Ordena os filhos de `parents` com o `sorter` (e, se `recursive`, os dos descendentes).
    fn sort_level(&self, state: &mut TreeState, parents: &[usize], recursive: bool) {
        for &parent in parents {
            let children = state.nodes[parent].children.clone();
            let sorted = self.sorted(state, children);
            state.nodes[parent].children = sorted.clone();
            if recursive {
                self.sort_level(state, &sorted, true);
            }
        }
    }

    /// Os `nodes` em ordem estável de `sorter.compare`.
    fn sorted(&self, state: &TreeState, mut nodes: Vec<usize>) -> Vec<usize> {
        nodes.sort_by(|&a, &b| self.sorter.compare(&state.nodes[a].item, &state.nodes[b].item).cmp(&0));
        nodes
    }

    pub fn refilter(&self) {
        {
            let mut state = self.state.borrow_mut();
            self.refilter_state(&mut state);
        }
        self.after_rows_change();
    }

    fn refilter_state(&self, state: &mut TreeState) {
        let roots = state.roots.clone();
        for root in roots {
            // Na raiz, o `parentVisibility` não é `Visible` nem `Recurse`.
            self.filter_node(state, root, TreeVisibility::Hidden);
        }
    }

    fn filter_node(&self, state: &mut TreeState, index: usize, parent_visibility: TreeVisibility) -> bool {
        let result = self.filter.filter(&state.nodes[index].item, parent_visibility);
        state.nodes[index].filter_data = result.data;
        if result.visibility == TreeVisibility::Hidden {
            state.nodes[index].visible = false;
            return false;
        }
        let mut has_visible_descendants = false;
        for child in state.nodes[index].children.clone() {
            has_visible_descendants = self.filter_node(state, child, result.visibility) || has_visible_descendants;
        }
        let visible = if result.visibility == TreeVisibility::Recurse { has_visible_descendants } else { result.visibility == TreeVisibility::Visible };
        state.nodes[index].visible = visible;
        visible
    }

    /// Depois de uma mudança nas linhas: o foco e a seleção que saíram das linhas se vão, a rolagem cabe no conteúdo e
    /// `onDidChangeContentHeight` avisa quando a altura do conteúdo mudou.
    fn after_rows_change(&self) {
        let (lost_focus, content_height) = {
            let mut state = self.state.borrow_mut();
            let rows: HashSet<usize> = state.rows().into_iter().collect();
            state.selection.retain(|selected| rows.contains(selected));
            let lost = state.focus.is_some_and(|focus| !rows.contains(&focus));
            if lost {
                state.focus = None;
            }
            self.clamp_scroll(&mut state);
            let ordered = state.rows();
            (lost, self.row_heights(&state, &ordered).iter().sum::<f64>())
        };
        if lost_focus {
            self.on_did_change_focus.fire(Vec::new());
        }
        if self.content_height.replace(content_height) != content_height {
            self.on_did_change_content_height.fire(());
        }
    }

    // -----------------------------------------------------------------------------------------
    // Linhas e rolagem.
    // -----------------------------------------------------------------------------------------

    /// As linhas visíveis, com o que o desenho precisa para pintar cada uma.
    pub fn rows(&self) -> Vec<QuickTreeRow> {
        let state = self.state.borrow();
        state
            .rows()
            .into_iter()
            .map(|index| {
                let node = &state.nodes[index];
                QuickTreeRow {
                    item: node.item.clone(),
                    depth: state.depth(index),
                    collapsible: node.collapsible,
                    collapsed: node.collapsed,
                    focused: state.focus == Some(index),
                    selected: state.selection.contains(&index),
                    entry: self.renderer.render_element(&node.item, Some(&node.filter_data)),
                    height: self.delegate.get_height(&node.item),
                }
            })
            .collect()
    }

    fn row_heights(&self, state: &TreeState, rows: &[usize]) -> Vec<f64> {
        rows.iter().map(|&row| self.delegate.get_height(&state.nodes[row].item)).collect()
    }

    pub fn scroll_height(&self) -> f64 {
        let state = self.state.borrow();
        self.row_heights(&state, &state.rows()).iter().sum()
    }

    /// A altura visível: o conteúdo, limitado pelo `max-height`.
    pub fn render_height(&self) -> f64 {
        let state = self.state.borrow();
        self.view_height(&state, &state.rows())
    }

    fn view_height(&self, state: &TreeState, rows: &[usize]) -> f64 {
        let content: f64 = self.row_heights(state, rows).iter().sum();
        state.max_height.map_or(content, |max| content.min(max))
    }

    pub fn scroll_top(&self) -> f64 {
        self.state.borrow().scroll_top
    }

    pub fn set_scroll_top(&self, scroll_top: f64) {
        let mut state = self.state.borrow_mut();
        state.scroll_top = scroll_top;
        self.clamp_scroll(&mut state);
    }

    fn clamp_scroll(&self, state: &mut TreeState) {
        let rows = state.rows();
        let content: f64 = self.row_heights(state, &rows).iter().sum();
        let max_scroll = (content - self.view_height(state, &rows)).max(0.0);
        state.scroll_top = state.scroll_top.clamp(0.0, max_scroll);
    }

    /// `layout(maxHeight)`.
    pub fn layout(&self, max_height: Option<f64>) {
        let mut state = self.state.borrow_mut();
        state.max_height = max_height.filter(|&max| max != 0.0).map(|max| {
            // Make sure height aligns with list item heights
            (max / 44.0).floor() * 44.0
                // Add some extra height so that it's clear there's more to scroll
                + 6.0
        });
        match state.max_height {
            Some(max) => self.tree_element.style_set_property("max-height", &format!("{max}px")),
            None => self.tree_element.style_remove_property("max-height"),
        }
        self.clamp_scroll(&mut state);
    }

    /// O índice da linha que contém `y` (o número de linhas, se `y` passa do fim): o `indexAt` do `RangeMap`.
    fn index_at(&self, state: &TreeState, rows: &[usize], y: f64) -> usize {
        if y < 0.0 {
            return 0;
        }
        let mut top = 0.0;
        for (position, height) in self.row_heights(state, rows).into_iter().enumerate() {
            if y < top + height {
                return position;
            }
            top += height;
        }
        rows.len()
    }

    fn row_top(&self, state: &TreeState, rows: &[usize], position: usize) -> f64 {
        self.row_heights(state, &rows[..position]).iter().sum()
    }

    /// `reveal(element)` da árvore: expande os ancestrais e rola o mínimo para mostrar a linha.
    fn reveal_node(&self, node: usize) {
        let mut state = self.state.borrow_mut();
        // expandTo
        let mut ancestor = state.nodes[node].parent;
        while let Some(current) = ancestor {
            state.nodes[current].collapsed = false;
            ancestor = state.nodes[current].parent;
        }
        let rows = state.rows();
        let Some(position) = rows.iter().position(|&row| row == node) else { return };
        let top = self.row_top(&state, &rows, position);
        let bottom = top + self.delegate.get_height(&state.nodes[node].item);
        let view_height = self.view_height(&state, &rows);
        if top < state.scroll_top {
            state.scroll_top = top;
        } else if bottom > state.scroll_top + view_height {
            state.scroll_top = bottom - view_height;
        }
        self.clamp_scroll(&mut state);
    }

    // -----------------------------------------------------------------------------------------
    // Foco.
    // -----------------------------------------------------------------------------------------

    /// `tree.domFocus()`.
    pub fn dom_focus(&self) {
        self.state.borrow_mut().dom_focused = true;
    }

    /// O foco do documento está na árvore (`tree.domFocus()` foi o último pedido de foco).
    pub fn has_dom_focus(&self) -> bool {
        self.state.borrow().dom_focused
    }

    /// `tree.setFocus([item])` (e `setSelection`, que o `registerOnDidChangeFocus` faz seguir o foco).
    fn set_focus_node(&self, node: Option<usize>) {
        let items = {
            let mut state = self.state.borrow_mut();
            state.focus = node;
            state.selection = node.into_iter().collect();
            node.map(|node| state.nodes[node].item.clone()).into_iter().collect::<Vec<_>>()
        };
        self.on_did_change_focus.fire(items);
    }

    /// `tree.setFocus([item])`: `None` se o item não está na árvore.
    pub fn set_focus(&self, item: &IQuickTreeItem) -> Option<()> {
        let node = self.state.borrow().find(item)?;
        self.set_focus_node(Some(node));
        Some(())
    }

    /// `getActiveItems()`: `this._tree.getFocus().filter(item => item !== null)`.
    pub fn get_active_items(&self) -> Vec<IQuickTreeItem> {
        let state = self.state.borrow();
        state.focus.map(|focus| state.nodes[focus].item.clone()).into_iter().collect()
    }

    /// `findNextIndex(index, loop, filter)` do `ListWidget`, sobre as linhas.
    fn find_next_index(rows: &[usize], mut index: i64, loop_: bool, filter: &mut dyn FnMut(usize) -> bool) -> Option<usize> {
        let length = rows.len() as i64;
        for _ in 0..length {
            if index >= length && !loop_ {
                return None;
            }
            index %= length;
            if filter(rows[index as usize]) {
                return Some(index as usize);
            }
            index += 1;
        }
        None
    }

    /// `findPreviousIndex(index, loop, filter)` do `ListWidget`.
    fn find_previous_index(rows: &[usize], mut index: i64, loop_: bool, filter: &mut dyn FnMut(usize) -> bool) -> Option<usize> {
        let length = rows.len() as i64;
        for _ in 0..length {
            if index < 0 && !loop_ {
                return None;
            }
            index = (length + (index % length)) % length;
            if filter(rows[index as usize]) {
                return Some(index as usize);
            }
            index -= 1;
        }
        None
    }

    fn focused_position(&self, rows: &[usize]) -> Option<usize> {
        let focus = self.state.borrow().focus?;
        rows.iter().position(|&row| row == focus)
    }

    fn rows_snapshot(&self) -> Vec<usize> {
        self.state.borrow().rows()
    }

    /// O filtro de `focusNext` e companhia: `this._tree.reveal(e.element); return true;`.
    fn reveal_and_accept(&self, node: usize) -> bool {
        self.reveal_node(node);
        true
    }

    pub fn focus(&self, what: QuickPickFocus) {
        match what {
            QuickPickFocus::First => {
                self.set_scroll_top(0.0);
                let rows = self.rows_snapshot();
                if let Some(position) = Self::find_next_index(&rows, 0, false, &mut |_| true) {
                    self.set_focus_node(Some(rows[position]));
                }
            }
            QuickPickFocus::Second => {
                self.set_scroll_top(0.0);
                let rows = self.rows_snapshot();
                let mut is_second_item = false;
                let found = Self::find_next_index(&rows, 0, false, &mut |_| {
                    if is_second_item {
                        return true;
                    }
                    is_second_item = !is_second_item;
                    false
                });
                if let Some(position) = found {
                    self.set_focus_node(Some(rows[position]));
                }
            }
            QuickPickFocus::Last => {
                self.set_scroll_top(self.scroll_height());
                let rows = self.rows_snapshot();
                if let Some(position) = Self::find_previous_index(&rows, rows.len() as i64 - 1, false, &mut |_| true) {
                    self.set_focus_node(Some(rows[position]));
                }
            }
            QuickPickFocus::Next | QuickPickFocus::Previous => {
                let rows = self.rows_snapshot();
                let previous_focus = self.focused_position(&rows);
                let reveal = &mut |node| self.reveal_and_accept(node);
                let target = if what == QuickPickFocus::Next {
                    Self::find_next_index(&rows, previous_focus.map_or(0, |focus| focus as i64 + 1), false, reveal)
                } else {
                    Self::find_previous_index(&rows, previous_focus.map_or(0, |focus| focus as i64 - 1), false, reveal)
                };
                if let Some(position) = target {
                    self.set_focus_node(Some(rows[position]));
                }
                let current_focus = self.focused_position(&rows);
                if previous_focus.is_some() && previous_focus == current_focus {
                    self.on_leave.fire(());
                }
            }
            QuickPickFocus::NextPage => self.focus_next_page(),
            QuickPickFocus::PreviousPage => self.focus_previous_page(),
            QuickPickFocus::NextSeparator | QuickPickFocus::PreviousSeparator => {
                // These don't make sense for the tree
            }
        }
    }

    /// `focusNextPage(browserEvent, filter)` do `ListWidget`.
    fn focus_next_page(&self) {
        loop {
            let rows = self.rows_snapshot();
            if rows.is_empty() {
                return;
            }
            let (scroll_top, render_height) = (self.scroll_top(), self.render_height());
            let last_page_index = {
                let state = self.state.borrow();
                self.index_at(&state, &rows, scroll_top + render_height)
            };
            let last_page_index = (if last_page_index == 0 { 0 } else { last_page_index - 1 }).min(rows.len() - 1);
            let current = self.focused_position(&rows);
            if current != Some(last_page_index) && current.is_none_or(|current| last_page_index > current) {
                let last_good = Self::find_previous_index(&rows, last_page_index as i64, false, &mut |node| self.reveal_and_accept(node));
                let target = match last_good {
                    Some(good) if current != Some(good) => good,
                    _ => last_page_index,
                };
                self.set_focus_node(Some(rows[target]));
                return;
            }
            let previous_scroll_top = scroll_top;
            let mut next_page_scroll_top = previous_scroll_top + render_height;
            if current.is_some_and(|current| last_page_index > current) {
                next_page_scroll_top -= self.delegate.get_height(&self.state.borrow().nodes[rows[last_page_index]].item);
            }
            self.set_scroll_top(next_page_scroll_top);
            if self.scroll_top() == previous_scroll_top {
                return;
            }
            self.clear_focus();
        }
    }

    /// `focusPreviousPage(browserEvent, filter)` do `ListWidget`.
    fn focus_previous_page(&self) {
        loop {
            let rows = self.rows_snapshot();
            if rows.is_empty() {
                return;
            }
            let (scroll_top, render_height) = (self.scroll_top(), self.render_height());
            let first_page_index = {
                let state = self.state.borrow();
                if scroll_top == 0.0 { self.index_at(&state, &rows, scroll_top) } else { (self.index_at(&state, &rows, scroll_top - 1.0) + 1).min(rows.len()) }
            }
            .min(rows.len() - 1);
            let current = self.focused_position(&rows);
            if current != Some(first_page_index) && current.is_none_or(|current| current >= first_page_index) {
                let first_good = Self::find_next_index(&rows, first_page_index as i64, false, &mut |node| self.reveal_and_accept(node));
                let target = match first_good {
                    Some(good) if current != Some(good) => good,
                    _ => first_page_index,
                };
                self.set_focus_node(Some(rows[target]));
                return;
            }
            let previous_scroll_top = scroll_top;
            self.set_scroll_top(scroll_top - render_height);
            if self.scroll_top() == previous_scroll_top {
                return;
            }
            self.clear_focus();
        }
    }

    /// `setFocus([])`.
    pub fn clear_focus(&self) {
        self.set_focus_node(None);
    }

    // -----------------------------------------------------------------------------------------
    // Estrutura: pai, colapso e `reveal`.
    // -----------------------------------------------------------------------------------------

    /// `tree.getParentElement(item)`: `None` para um item da raiz (o `null`) ou fora da árvore.
    pub fn get_parent_element(&self, item: &IQuickTreeItem) -> Option<IQuickTreeItem> {
        let state = self.state.borrow();
        let parent = state.nodes[state.find(item)?].parent?;
        Some(state.nodes[parent].item.clone())
    }

    /// `tree.expand(item)`.
    pub fn expand(&self, item: &IQuickTreeItem) {
        self.set_collapsed(item, false);
    }

    /// `tree.collapse(item)`.
    pub fn collapse(&self, item: &IQuickTreeItem) {
        self.set_collapsed(item, true);
    }

    fn set_collapsed(&self, item: &IQuickTreeItem, collapsed: bool) {
        let focus_moved = {
            let mut state = self.state.borrow_mut();
            let Some(node) = state.find(item) else { return };
            if !state.nodes[node].collapsible || state.nodes[node].collapsed == collapsed {
                return;
            }
            state.nodes[node].collapsed = collapsed;
            // O foco dentro do que colapsou vai para o próprio nó.
            let moved = collapsed && state.focus.is_some_and(|focus| state.is_descendant_of(focus, node));
            if moved {
                state.focus = Some(node);
                state.selection = vec![node];
            }
            moved.then(|| state.nodes[node].item.clone())
        };
        self.after_rows_change();
        if let Some(item) = focus_moved {
            self.on_did_change_focus.fire(vec![item]);
        }
    }

    /// `tree.isCollapsed(item)`.
    pub fn is_collapsed(&self, item: &IQuickTreeItem) -> bool {
        let state = self.state.borrow();
        state.find(item).is_some_and(|node| state.nodes[node].collapsed)
    }

    /// `tree.reveal(item)` seguido de `tree.setFocus([item])`, do `QuickTree.reveal`.
    pub fn reveal(&self, item: &IQuickTreeItem) {
        let Some(node) = self.state.borrow().find(item) else { return };
        self.reveal_node(node);
        self.set_focus_node(Some(node));
    }

    /// `getParentNodeState([...tree.getNode().children])`: o estado dos itens da raiz.
    pub fn get_check_all_state(&self) -> QuickTreeItemChecked {
        let state = self.state.borrow();
        get_parent_node_state(state.roots.iter().map(|&root| state.nodes[root].item.checked))
    }

    // -----------------------------------------------------------------------------------------
    // Marcação.
    // -----------------------------------------------------------------------------------------

    /// O `onDidOpen` da árvore (clique, Enter, duplo clique): `target_is_checkbox` é
    /// `e.browserEvent?.target.classList.contains(Checkbox.CLASS_NAME)`.
    pub fn open_item(&self, item: &IQuickTreeItem, target_is_checkbox: bool) {
        let (node, disabled, pickable, checked) = {
            let state = self.state.borrow();
            let Some(node) = state.find(item) else { return };
            let element = &state.nodes[node].item;
            (node, element.base.disabled == Some(true), element.pickable, element.checked)
        };
        if disabled {
            return;
        }

        // Check if the item is pickable (defaults to true if not specified)
        if pickable == Some(false) {
            // For non-pickable items, set it as the active item and fire the accept event
            self.set_focus_node(Some(node));
            self.on_did_accept.fire(());
            return;
        }

        if target_is_checkbox {
            return;
        }

        self.update_checkbox_state(node, checked == Some(QuickTreeItemChecked::Bool(true)), false);
    }

    /// `updateCheckboxState(item, newState, skipItemRerender)`: as linhas são recalculadas a cada [`Self::rows`], então
    /// `skip_item_rerender` não tem o que pular.
    fn update_checkbox_state(&self, item: usize, new_state: bool, _skip_item_rerender: bool) {
        let (item_snapshot, checked_item) = {
            let mut state = self.state.borrow_mut();
            let new_checked = QuickTreeItemChecked::Bool(new_state);
            if state.nodes[item].item.checked.unwrap_or(QuickTreeItemChecked::Bool(false)) == new_checked {
                return; // No change
            }

            // Handle checked item
            state.nodes[item].item.checked = Some(new_checked);

            // Handle children of the checked item
            let mut update_set: HashSet<usize> = HashSet::new();
            let mut to_update: VecDeque<usize> = state.nodes[item].children.iter().copied().collect();
            while let Some(pop) = to_update.pop_front() {
                if update_set.insert(pop) {
                    if state.nodes[pop].item.checked.unwrap_or(QuickTreeItemChecked::Bool(false)) != new_checked {
                        state.nodes[pop].item.checked = Some(new_checked);
                    }
                    to_update.extend(state.nodes[pop].children.iter().copied());
                }
            }

            // Handle parents of the checked item
            let mut parent = state.nodes[item].parent;
            while let Some(current) = parent {
                let parent_state = get_parent_node_state(state.nodes[current].children.iter().map(|&child| state.nodes[child].item.checked));
                if state.nodes[current].item.checked.unwrap_or(QuickTreeItemChecked::Bool(false)) != parent_state {
                    state.nodes[current].item.checked = Some(parent_state);
                }
                parent = state.nodes[current].parent;
            }
            (state.nodes[item].item.clone(), new_checked)
        };
        self.on_did_change_checkbox_state.fire(IQuickTreeCheckboxEvent { item: item_snapshot, checked: checked_item });
        self.on_did_checked_leaf_items_change.fire(self.get_checked_leaf_items());
    }

    pub fn get_checked_leaf_items(&self) -> Vec<IQuickTreeItem> {
        let state = self.state.borrow();
        let mut looked_at: HashSet<usize> = HashSet::new();
        let mut to_look_at: VecDeque<usize> = state.roots.iter().copied().collect();
        let mut checked_items = Vec::new();
        while let Some(look_at) = to_look_at.pop_front() {
            if looked_at.contains(&look_at) {
                continue;
            }
            let node = &state.nodes[look_at];
            let truthy = matches!(node.item.checked, Some(QuickTreeItemChecked::Mixed) | Some(QuickTreeItemChecked::Bool(true)));
            if truthy {
                looked_at.insert(look_at);
                to_look_at.extend(node.children.iter().copied());
                if node.item.children.is_none() {
                    checked_items.push(node.item.clone());
                }
            }
        }
        checked_items
    }

    pub fn toggle_checkbox(&self) {
        for element in self.get_active_items() {
            if element.pickable != Some(false) && element.base.disabled != Some(true) {
                let node = self.state.borrow().find(&element);
                if let Some(node) = node {
                    self.update_checkbox_state(node, element.checked != Some(QuickTreeItemChecked::Bool(true)), false);
                }
            }
        }
    }

    pub fn check_all(&self, checked: QuickTreeItemChecked) {
        let mut updated: HashSet<usize> = HashSet::new();
        let mut to_update: VecDeque<usize> = self.state.borrow().roots.iter().copied().collect();
        let mut fire_checked_change_event = false;
        while let Some(update) = to_update.pop_front() {
            let event_item = {
                let mut state = self.state.borrow_mut();
                if updated.contains(&update) || state.nodes[update].item.checked == Some(checked) {
                    continue;
                }
                fire_checked_change_event = true;
                state.nodes[update].item.checked = Some(checked);
                to_update.extend(state.nodes[update].children.iter().copied());
                updated.insert(update);
                state.nodes[update].item.clone()
            };
            self.on_did_change_checkbox_state.fire(IQuickTreeCheckboxEvent { item: event_item, checked });
        }
        if fire_checked_change_event {
            self.on_did_checked_leaf_items_change.fire(self.get_checked_leaf_items());
        }
    }
}

impl IDisposable for QuickInputTreeController {
    fn dispose(&self) {
        self.base.dispose();
    }

    fn constructor_name(&self) -> &'static str {
        "QuickInputTreeController"
    }
}
