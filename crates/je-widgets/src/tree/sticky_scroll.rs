//! Porte do sticky scroll de `src/vs/base/browser/ui/tree/abstractTree.ts` (`StickyScrollNode`,
//! `StickyScrollState`, `DefaultStickyScrollDelegate`, `StickyScrollController`, `StickyScrollWidget`,
//! `StickyScrollFocus`): os nós ancestrais fixados no topo da árvore.
//!
//! Do TypeScript para o Rust:
//! - O DOM é a árvore do `je-ui`. O container (`.monaco-tree-sticky-container`, com `empty` sem linhas),
//!   a sombra (`.monaco-tree-sticky-container-shadow`) e as linhas (`.monaco-tree-sticky-row`,
//!   `.monaco-list-row`, `source-node-partially-visible`, `focused`, `passive-focused`) têm as classes do
//!   original e a ordem do DOM do original (a última linha fixada vem primeiro). O CSS de `tree.css` vira
//!   geometria dos nós: container em `z-index` 13 com largura total e altura da última linha, linha com
//!   `top`/`height` em `bounds`, largura total e `overflow: hidden` (`clip`); `display: none` do `.empty` é
//!   `style.visible = false`.
//! - O `Proxy` do nó (para não influenciar o nó original) não existe: o nó é o mesmo `Rc`. O que o `Proxy`
//!   protegia, os mapas por nó do `TreeRenderer`, é resolvido por
//!   [`TreeRenderer::render_element_detached`], que devolve os mapas ao estado anterior.
//! - `data-index`, `data-parity`, `id` e `aria-*` (`setAccessibilityAttributes`, o `autorun` do rótulo
//!   ARIA) não são escritos: o `je-ui` só tem `role` e `label`, e a `AbstractTreeOptions` não tem
//!   `accessibilityProvider`. O `data-index` é substituído por
//!   [`ListView::set_external_row_index`](crate::list::list_view::ListView::set_external_row_index), que
//!   faz os eventos de ponteiro na linha fixada resolverem o elemento da lista.
//! - `tabIndex` do container não existe: o container recebe foco do `je-ui` sempre, e o `blur()` que o
//!   original dispara ao trocar o `tabIndex` por `-1` é o foco voltar à lista em `updateElements`.
//! - `probeDynamicHeights` lê a altura dos limites do nó (`bounds`) onde o original lê `offsetHeight`. O
//!   `je-ui` não dimensiona a linha pelo conteúdo, então "limpar a altura para a linha crescer sozinha" não
//!   muda a medida: vale o que o renderer deu aos limites da linha. `setRowLineHeight` e `lineHeight` não
//!   existem (o texto do `je-ui` se centra no próprio nó).
//! - O menu de contexto (`onContextMenu`, `handleContextMenu`, `ITreeContextMenuEvent`) fica de fora: a
//!   lista do `je-widgets` não tem o evento de menu de contexto.
//! - `onMouseDown` do `StickyScrollFocus` ouve o `PointerDown` do `je-ui` direto no nó da lista, porque o
//!   `BrowserEvent` da lista não carrega `preventDefault`/`stopPropagation`.
//! - Os erros do original viram `throw` (pânico com o `Error`), com o mesmo texto.

use std::cell::{Cell, RefCell};
use std::hash::Hash;
use std::rc::{Rc, Weak};

use je_base::arrays::equals;
use je_base::event::{Emitter, Event as BaseEvent};
use je_base::key_codes::KeyCode;
use je_base::lifecycle::{Disposable, DisposableRef, DisposableStore, IDisposable, to_disposable};
use je_render::kurbo::Rect;
use je_ui::{EventKind, KeyData, NodeId, Role, Ui};

use crate::common::{registered_emitter, throw};
use crate::list::list_types::{IListRenderer, IListVirtualDelegate, ListElementRenderDetails};
use crate::list::list_widget::List;
use crate::tree::abstract_tree::{AbstractTree, AbstractTreeOptions, AbstractTreeOptionsUpdate, ComposedTreeDelegate};
use crate::tree::tree_renderer::TreeRenderer;
use crate::tree::tree_types::{ITreeModel, TreeListSpliceData, TreeNodeApi};

/// `StickyScrollNode<T, TFilterData>`.
pub struct StickyScrollNode<M: ITreeModel> {
    pub node: Rc<M::Node>,
    pub start_index: usize,
    pub end_index: usize,
    pub height: f64,
    pub position: f64,
    pub source_node_end: f64,
    pub source_node_partially_visible: bool,
    pub has_explicit_height: bool,
}

impl<M: ITreeModel> Clone for StickyScrollNode<M> {
    fn clone(&self) -> Self {
        Self {
            node: self.node.clone(),
            start_index: self.start_index,
            end_index: self.end_index,
            height: self.height,
            position: self.position,
            source_node_end: self.source_node_end,
            source_node_partially_visible: self.source_node_partially_visible,
            has_explicit_height: self.has_explicit_height,
        }
    }
}

/// `IStickyScrollNodeSourceRange`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StickyScrollNodeSourceRange {
    pub start: f64,
    pub end: f64,
    pub sticky_node_height: Option<f64>,
    pub estimated: bool,
}

/// `stickyScrollNodeSourceRangeProvider`.
pub type StickyScrollNodeSourceRangeProvider<E> = Rc<dyn Fn(&E, StickyScrollNodeSourceRange) -> Option<StickyScrollNodeSourceRange>>;

/// `IStickyScrollDelegate<T, TFilterData>`.
pub trait IStickyScrollDelegate<M: ITreeModel> {
    fn constrain_sticky_scroll_nodes(&self, sticky_nodes: Vec<StickyScrollNode<M>>, sticky_scroll_max_item_count: usize, max_widget_height: f64) -> Vec<StickyScrollNode<M>>;
}

/// `stickyScrollNodeStateEquals`.
fn sticky_scroll_node_state_equals<M: ITreeModel>(node1: &StickyScrollNode<M>, node2: &StickyScrollNode<M>) -> bool
where
    M::Element: PartialEq,
{
    node1.position == node2.position && sticky_scroll_node_equals(node1, node2)
}

/// `stickyScrollNodeEquals`.
fn sticky_scroll_node_equals<M: ITreeModel>(node1: &StickyScrollNode<M>, node2: &StickyScrollNode<M>) -> bool
where
    M::Element: PartialEq,
{
    node1.node.element() == node2.node.element()
        && node1.start_index == node2.start_index
        && node1.height == node2.height
        && node1.end_index == node2.end_index
        && node1.source_node_end == node2.source_node_end
        && node1.source_node_partially_visible == node2.source_node_partially_visible
        && node1.has_explicit_height == node2.has_explicit_height
}

/// `StickyScrollState<T, TFilterData, TRef>`.
struct StickyScrollState<M: ITreeModel> {
    sticky_nodes: Vec<StickyScrollNode<M>>,
}

impl<M: ITreeModel> StickyScrollState<M>
where
    M::Element: PartialEq,
{
    fn count(&self) -> usize {
        self.sticky_nodes.len()
    }

    fn equal(&self, state: &StickyScrollState<M>) -> bool {
        equals(Some(&self.sticky_nodes), Some(&state.sticky_nodes), sticky_scroll_node_state_equals)
    }

    fn contains(&self, node: &Rc<M::Node>) -> bool {
        let element = node.element();
        self.sticky_nodes.iter().any(|sticky| sticky.node.element() == element)
    }

    fn last_node_partially_visible(&self) -> bool {
        let Some(last_sticky_node) = self.sticky_nodes.last() else { return false };

        if self.count() == 1 {
            return last_sticky_node.position != 0.0;
        }

        let second_last_sticky_node = &self.sticky_nodes[self.count() - 2];
        second_last_sticky_node.position + second_last_sticky_node.height != last_sticky_node.position
    }

    fn animation_state_changed(&self, previous_state: &StickyScrollState<M>) -> bool {
        if !equals(Some(&self.sticky_nodes), Some(&previous_state.sticky_nodes), sticky_scroll_node_equals) {
            return false;
        }

        let (Some(last_sticky_node), Some(previous_last_sticky_node)) = (self.sticky_nodes.last(), previous_state.sticky_nodes.last()) else { return false };

        last_sticky_node.position != previous_last_sticky_node.position
    }
}

/// `DefaultStickyScrollDelegate<T, TFilterData>`.
struct DefaultStickyScrollDelegate;

impl<M: ITreeModel> IStickyScrollDelegate<M> for DefaultStickyScrollDelegate {
    fn constrain_sticky_scroll_nodes(&self, mut sticky_nodes: Vec<StickyScrollNode<M>>, sticky_scroll_max_item_count: usize, max_widget_height: f64) -> Vec<StickyScrollNode<M>> {
        let cut = sticky_nodes.iter().enumerate().find(|(i, sticky_node)| sticky_node.position + sticky_node.height > max_widget_height || *i >= sticky_scroll_max_item_count).map(|(i, _)| i);

        if let Some(cut) = cut {
            sticky_nodes.truncate(cut);
        }

        sticky_nodes
    }
}

/// `validateStickySettings`: o padrão é 7 e o mínimo é 1.
fn validate_sticky_settings(sticky_scroll_max_item_count: Option<usize>) -> usize {
    sticky_scroll_max_item_count.map_or(7, |n| n.max(1))
}

/// `Math.min(height, stickyScrollMaxNodeHeight)` quando a opção existe.
fn clamp_node_height(max_node_height: Option<f64>, height: f64) -> f64 {
    max_node_height.map_or(height, |max| height.min(max))
}

/// O `tree.options` do original: um instantâneo (a árvore guarda as opções por trás de um `RefCell`).
fn tree_options<M: ITreeModel>(tree: &Weak<AbstractTree<M>>) -> Option<AbstractTreeOptions<M>>
where
    M::Element: Eq + Hash,
{
    tree.upgrade().map(|tree| tree.options())
}

/// Move o `top` do nó sem mexer na altura (o `style.top = ...`).
fn set_top(ui: &Ui, node: NodeId, top: f64) {
    let mut tree = ui.tree_mut();
    let bounds = tree.bounds(node);
    tree.set_bounds(node, Rect::new(bounds.x0, top, bounds.x1, top + bounds.height()));
}

/// `{ index, height }` de `_onDidChangeHeight`.
type HeightChange = (usize, f64);

const MAX_WIDGET_VIEW_RATIO: f64 = 0.4;

/// `StickyScrollController<T, TFilterData, TRef>`.
pub struct StickyScrollController<M: ITreeModel>
where
    M::Element: Eq + Hash,
{
    base: Rc<Disposable>,
    tree: Weak<AbstractTree<M>>,
    model: Rc<M>,
    view: Rc<List<Rc<M::Node>>>,
    tree_delegate: Rc<ComposedTreeDelegate<M>>,
    sticky_scroll_delegate: Rc<dyn IStickyScrollDelegate<M>>,
    sticky_scroll_max_item_count: Cell<usize>,
    padding_top: Cell<f64>,
    widget: Rc<StickyScrollWidget<M>>,
}

impl<M: ITreeModel> StickyScrollController<M>
where
    M::Element: Eq + Hash,
{
    pub fn new(tree: &Rc<AbstractTree<M>>) -> Rc<Self> {
        let options = tree.options();
        let base = Disposable::new();
        let widget = StickyScrollWidget::new(&tree.ui, Rc::downgrade(tree), tree.view.clone(), tree.renderers.clone(), tree.tree_delegate.clone());
        base.register(widget.clone());

        let sticky_scroll_delegate: Rc<dyn IStickyScrollDelegate<M>> = options.sticky_scroll_delegate.clone().unwrap_or_else(|| Rc::new(DefaultStickyScrollDelegate));

        let this = Rc::new(Self {
            base,
            tree: Rc::downgrade(tree),
            model: tree.model.clone(),
            view: tree.view.clone(),
            tree_delegate: tree.tree_delegate.clone(),
            sticky_scroll_delegate,
            sticky_scroll_max_item_count: Cell::new(validate_sticky_settings(options.update.sticky_scroll_max_item_count)),
            padding_top: Cell::new(options.update.padding_top.unwrap_or(0.0)),
            widget,
        });

        fn subscribe<M: ITreeModel, T: 'static>(this: &Rc<StickyScrollController<M>>, event: BaseEvent<T>, handler: impl Fn(&StickyScrollController<M>, &T) + 'static)
        where
            M::Element: Eq + Hash,
        {
            let weak = Rc::downgrade(this);
            this.base.register(event.listen(move |e| {
                if let Some(this) = weak.upgrade() {
                    handler(&this, e);
                }
            }));
        }

        subscribe(&this, tree.view.view.on_did_scroll(), |this, _| this.update());
        subscribe(&this, tree.view.view.on_did_change_content_height(), |this, _| this.update());
        subscribe(&this, tree.on_did_change_collapse_state(), |this, _| this.update());
        subscribe(&this, this.widget.on_did_change_height.event(), |this, height_changes: &Vec<HeightChange>| {
            // Atualiza as alturas das linhas da lista com os valores medidos.
            for (index, height) in height_changes {
                this.view.update_element_height(*index, Some(*height));
            }
        });
        subscribe(&this, tree.model.on_did_splice_rendered_nodes(), |this, e: &TreeListSpliceData<M::Node>| this.on_did_splice_rendered_nodes(e));

        this.update();
        this
    }

    fn on_did_splice_rendered_nodes(&self, e: &TreeListSpliceData<M::Node>) {
        let Some(state) = self.widget.state() else { return };

        // Se um nó fixado foi removido, recalcula o estado.
        let has_removed_sticky_node = e.delete_count > 0 && state.sticky_nodes.iter().any(|sticky_node| !self.model.has(&self.model.get_node_location(&sticky_node.node)));

        if has_removed_sticky_node {
            self.update();
            return;
        }

        // Se um nó fixado foi atualizado, redesenha o widget.
        let should_rerender_sticky_nodes = state.sticky_nodes.iter().any(|sticky_node| {
            let list_index = self.model.get_list_index(&self.model.get_node_location(&sticky_node.node));
            list_index >= e.start as i64 && list_index < (e.start + e.delete_count) as i64 && state.contains(&sticky_node.node)
        });

        if should_rerender_sticky_nodes {
            self.widget.rerender();
        }
    }

    pub fn dom_node(&self) -> NodeId {
        self.widget.root_dom_node
    }

    pub fn height(&self) -> f64 {
        self.widget.height()
    }

    pub fn count(&self) -> usize {
        self.widget.count()
    }

    pub fn get_node(&self, node: &Rc<M::Node>) -> Option<StickyScrollNode<M>> {
        self.widget.get_node(node)
    }

    fn get_node_at_height(&self, height: f64) -> Option<Rc<M::Node>> {
        let index = if height == 0.0 { self.view.first_visible_index() } else { self.view.index_at(height + self.view.scroll_top()) };

        if index < 0 || index >= self.view.length() as i64 {
            return None;
        }

        Some(self.view.element(index as usize))
    }

    fn update(&self) {
        let first_visible_node = self.get_node_at_height(self.padding_top.get());

        // Não renderiza nada se não há elementos.
        let Some(first_visible_node) = first_visible_node.filter(|_| self.view.scroll_top() > self.padding_top.get() && self.view.render_height() != 0.0) else {
            self.widget.set_state(None);
            return;
        };

        let sticky_state = self.find_sticky_state(&first_visible_node);
        self.widget.set_state(sticky_state.map(Rc::new));
    }

    fn find_sticky_state(&self, first_visible_node: &Rc<M::Node>) -> Option<StickyScrollState<M>> {
        let mut sticky_nodes: Vec<StickyScrollNode<M>> = Vec::new();
        let mut first_visible_node_under_widget = first_visible_node.clone();
        let mut sticky_nodes_height = 0.0;

        let mut next_sticky_node = self.get_next_sticky_node(&first_visible_node_under_widget, None, sticky_nodes_height);

        while let Some(next) = next_sticky_node {
            sticky_nodes_height += next.height;
            sticky_nodes.push(next.clone());

            if sticky_nodes.len() <= self.sticky_scroll_max_item_count.get() {
                match self.get_next_visible_node(&next) {
                    Some(node) => first_visible_node_under_widget = node,
                    None => break,
                }
            }

            next_sticky_node = self.get_next_sticky_node(&first_visible_node_under_widget, Some(&next.node), sticky_nodes_height);
        }

        let constrained_sticky_nodes = self.constrain_sticky_nodes(sticky_nodes);

        if constrained_sticky_nodes.is_empty() { None } else { Some(StickyScrollState { sticky_nodes: constrained_sticky_nodes }) }
    }

    fn get_next_visible_node(&self, previous_sticky_node: &StickyScrollNode<M>) -> Option<Rc<M::Node>> {
        self.get_node_at_height(previous_sticky_node.position + previous_sticky_node.height)
    }

    fn get_next_sticky_node(&self, first_visible_node_under_widget: &Rc<M::Node>, previous_sticky_node: Option<&Rc<M::Node>>, sticky_nodes_height: f64) -> Option<StickyScrollNode<M>> {
        let next_sticky_node = self.get_ancestor_under_previous(first_visible_node_under_widget, previous_sticky_node)?;

        if Rc::ptr_eq(&next_sticky_node, first_visible_node_under_widget) && !self.node_is_uncollapsed_parent(first_visible_node_under_widget) {
            return None;
        }

        let source_range = self.get_sticky_scroll_node_source_range(&next_sticky_node)?;

        if !source_range.start.is_finite() || !source_range.end.is_finite() || source_range.end <= source_range.start {
            return None;
        }

        let source_node_top = self.view.get_element_top(self.get_node_index(&next_sticky_node));
        let sticky_viewport_bottom = self.view.scroll_top() + sticky_nodes_height;

        if sticky_viewport_bottom <= source_node_top + source_range.start {
            return None;
        }

        let height = self.get_sticky_scroll_node_height(&source_range, sticky_nodes_height);
        let source_node_partially_visible = sticky_viewport_bottom + height < source_node_top + source_range.end;

        Some(self.create_sticky_scroll_node(&next_sticky_node, sticky_nodes_height, height, source_range.end, source_node_partially_visible, source_range.sticky_node_height.is_some()))
    }

    fn get_sticky_scroll_node_source_range(&self, node: &Rc<M::Node>) -> Option<StickyScrollNodeSourceRange> {
        let default_range = StickyScrollNodeSourceRange { start: 0.0, end: self.get_node_height(node), sticky_node_height: None, estimated: false };
        let source_range_provider = tree_options(&self.tree).and_then(|options| options.sticky_scroll_node_source_range_provider);

        match source_range_provider {
            Some(provider) => provider(&node.element(), default_range),
            None => Some(default_range),
        }
    }

    fn get_sticky_scroll_node_height(&self, source_range: &StickyScrollNodeSourceRange, current_sticky_nodes_height: f64) -> f64 {
        let max_node_height = tree_options(&self.tree).and_then(|options| options.update.sticky_scroll_max_node_height);
        let height = clamp_node_height(max_node_height, source_range.sticky_node_height.unwrap_or(source_range.end - source_range.start));

        if !source_range.estimated {
            return height;
        }

        let available_height = (self.view.render_height() * MAX_WIDGET_VIEW_RATIO - current_sticky_nodes_height).max(1.0);
        height.min(available_height)
    }

    fn get_node_height(&self, node: &Rc<M::Node>) -> f64 {
        let index = self.model.get_list_index(&self.model.get_node_location(node));

        if index >= 0 {
            return self.view.get_element_height(index as usize);
        }

        self.tree_delegate.get_height(node)
    }

    fn create_sticky_scroll_node(
        &self,
        node: &Rc<M::Node>,
        current_sticky_nodes_height: f64,
        height: f64,
        source_node_end: f64,
        source_node_partially_visible: bool,
        has_explicit_height: bool,
    ) -> StickyScrollNode<M> {
        let (start_index, end_index) = self.get_node_range(node);
        let position = self.calculate_sticky_node_position(end_index, current_sticky_nodes_height, height);

        StickyScrollNode { node: node.clone(), position, height, start_index, end_index, source_node_end, source_node_partially_visible, has_explicit_height }
    }

    fn get_ancestor_under_previous(&self, node: &Rc<M::Node>, previous_ancestor: Option<&Rc<M::Node>>) -> Option<Rc<M::Node>> {
        let mut current_ancestor = node.clone();
        let mut parent_of_current_ancestor = self.get_parent_node(&current_ancestor);

        while let Some(parent) = parent_of_current_ancestor {
            if previous_ancestor.is_some_and(|previous| Rc::ptr_eq(&parent, previous)) {
                return Some(current_ancestor);
            }

            parent_of_current_ancestor = self.get_parent_node(&parent);
            current_ancestor = parent;
        }

        if previous_ancestor.is_none() { Some(current_ancestor) } else { None }
    }

    fn calculate_sticky_node_position(&self, last_descendant_index: usize, sticky_row_position_top: f64, sticky_node_height: f64) -> f64 {
        let last_child_height = self.view.get_element_height(last_descendant_index);
        let bottom_of_last_child = self.view.get_element_top(last_descendant_index) + last_child_height - self.view.scroll_top();

        if sticky_row_position_top + sticky_node_height > bottom_of_last_child && sticky_row_position_top <= bottom_of_last_child {
            return bottom_of_last_child - sticky_node_height;
        }

        sticky_row_position_top
    }

    fn constrain_sticky_nodes(&self, sticky_nodes: Vec<StickyScrollNode<M>>) -> Vec<StickyScrollNode<M>> {
        let Some(last_sticky_node) = sticky_nodes.last() else { return Vec::new() };

        // Confere se os nós fixados precisam ser limitados.
        let maximum_sticky_widget_height = self.view.render_height() * MAX_WIDGET_VIEW_RATIO;
        let max_item_count = self.sticky_scroll_max_item_count.get();

        if sticky_nodes.len() <= max_item_count && last_sticky_node.position + last_sticky_node.height <= maximum_sticky_widget_height {
            return sticky_nodes;
        }

        // Limita os nós fixados.
        let constrained_sticky_nodes = self.sticky_scroll_delegate.constrain_sticky_scroll_nodes(sticky_nodes, max_item_count, maximum_sticky_widget_height);

        let Some(last_constrained_sticky_node) = constrained_sticky_nodes.last() else { return Vec::new() };

        // Valida as restrições.
        if constrained_sticky_nodes.len() > max_item_count || last_constrained_sticky_node.position + last_constrained_sticky_node.height > maximum_sticky_widget_height {
            throw("stickyScrollDelegate violates constraints");
        }

        constrained_sticky_nodes
    }

    fn get_parent_node(&self, node: &Rc<M::Node>) -> Option<Rc<M::Node>> {
        let node_location = self.model.get_node_location(node);
        let parent_location = self.model.get_parent_node_location(&node_location)?;

        if self.model.ref_is_falsy(&parent_location) { None } else { Some(self.model.get_node(&parent_location)) }
    }

    fn node_is_uncollapsed_parent(&self, node: &Rc<M::Node>) -> bool {
        self.model.get_list_render_count(&self.model.get_node_location(node)) > 1
    }

    /// `getNodeIndex`: o índice do nó na lista (o nó fixado é sempre ancestral de uma linha visível).
    fn get_node_index(&self, node: &Rc<M::Node>) -> usize {
        let node_index = self.model.get_list_index(&self.model.get_node_location(node));

        if node_index < 0 {
            throw("Node not found in tree");
        }

        node_index as usize
    }

    fn get_node_range(&self, node: &Rc<M::Node>) -> (usize, usize) {
        let start_index = self.get_node_index(node);
        let render_count = self.model.get_list_render_count(&self.model.get_node_location(node));

        (start_index, start_index + render_count - 1)
    }

    pub fn node_position_top_below_widget(&self, node: &Rc<M::Node>) -> f64 {
        let mut ancestors = Vec::new();
        let mut current_ancestor = self.get_parent_node(node);

        while let Some(ancestor) = current_ancestor {
            current_ancestor = self.get_parent_node(&ancestor);
            ancestors.push(ancestor);
        }

        let mut widget_height = 0.0;

        for ancestor in ancestors.iter().take(self.sticky_scroll_max_item_count.get()) {
            if let Some(source_range) = self.get_sticky_scroll_node_source_range(ancestor) {
                widget_height += self.get_sticky_scroll_node_height(&source_range, widget_height);
            }
        }

        widget_height
    }

    pub fn get_focus(&self) -> Option<M::Element> {
        self.widget.sticky_scroll_focus.get_focus()
    }

    pub fn dom_focus(&self) {
        self.widget.sticky_scroll_focus.dom_focus();
    }

    /// Se o sticky scroll foi a última parte da árvore com foco.
    pub fn focused_last(&self) -> bool {
        self.widget.sticky_scroll_focus.focused_last()
    }

    pub fn update_options(&self, options_update: &AbstractTreeOptionsUpdate<M::Element>) {
        if let Some(padding_top) = options_update.padding_top {
            self.padding_top.set(padding_top);
        }

        if let Some(sticky_scroll_max_item_count) = options_update.sticky_scroll_max_item_count {
            let validated = validate_sticky_settings(Some(sticky_scroll_max_item_count));

            if self.sticky_scroll_max_item_count.get() != validated {
                self.sticky_scroll_max_item_count.set(validated);
                self.update();
            }
        }
    }

    pub fn refresh(&self) {
        self.update();
    }

    pub fn rerender(&self) {
        self.widget.rerender();
        self.update();
    }

    pub fn on_did_change_has_focus(&self) -> BaseEvent<bool> {
        self.widget.sticky_scroll_focus.on_did_change_has_focus.event()
    }
}

impl<M: ITreeModel> IDisposable for StickyScrollController<M>
where
    M::Element: Eq + Hash,
{
    fn dispose(&self) {
        self.base.dispose();
    }

    fn constructor_name(&self) -> &'static str {
        "StickyScrollController"
    }
}

/// `StickyScrollWidget<T, TFilterData, TRef>`.
struct StickyScrollWidget<M: ITreeModel>
where
    M::Element: Eq + Hash,
{
    ui: Ui,
    tree: Weak<AbstractTree<M>>,
    view: Rc<List<Rc<M::Node>>>,
    tree_renderers: Vec<Rc<TreeRenderer<M>>>,
    tree_delegate: Rc<ComposedTreeDelegate<M>>,
    root_dom_node: NodeId,
    previous_state: RefCell<Option<Rc<StickyScrollState<M>>>>,
    previous_elements: RefCell<Vec<NodeId>>,
    previous_element_heights: RefCell<Vec<f64>>,
    previous_state_disposables: Rc<DisposableStore>,
    sticky_scroll_focus: Rc<StickyScrollFocus<M>>,
    on_did_change_height: Rc<Emitter<Vec<HeightChange>>>,
}

impl<M: ITreeModel> StickyScrollWidget<M>
where
    M::Element: Eq + Hash,
{
    fn new(ui: &Ui, tree: Weak<AbstractTree<M>>, view: Rc<List<Rc<M::Node>>>, tree_renderers: Vec<Rc<TreeRenderer<M>>>, tree_delegate: Rc<ComposedTreeDelegate<M>>) -> Rc<Self> {
        let container = view.view.scrollable_element_dom_node();

        let root_dom_node = {
            let mut tree = ui.tree_mut();
            let root = tree.create(container, Role::Generic);
            tree.add_class(root, "monaco-tree-sticky-container");
            tree.add_class(root, "empty");
            tree.set_bounds(root, Rect::ZERO);
            tree.set_fill(root, true, false);
            tree.style_mut(root).z_index = 13;
            // `.empty { display: none }`.
            tree.style_mut(root).visible = false;

            let shadow = tree.create(root, Role::Generic);
            tree.add_class(shadow, "monaco-tree-sticky-container-shadow");
            tree.set_fill(shadow, true, false);
            root
        };

        let sticky_scroll_focus = StickyScrollFocus::new(ui, root_dom_node, view.clone());

        Rc::new(Self {
            ui: ui.clone(),
            tree,
            view,
            tree_renderers,
            tree_delegate,
            root_dom_node,
            previous_state: RefCell::new(None),
            previous_elements: RefCell::new(Vec::new()),
            previous_element_heights: RefCell::new(Vec::new()),
            previous_state_disposables: DisposableStore::new(),
            sticky_scroll_focus,
            on_did_change_height: Emitter::new(),
        })
    }

    fn state(&self) -> Option<Rc<StickyScrollState<M>>> {
        self.previous_state.borrow().clone()
    }

    fn height(&self) -> f64 {
        self.state().map_or(0.0, |state| self.get_root_height(&state))
    }

    fn count(&self) -> usize {
        self.state().map_or(0, |state| state.count())
    }

    fn get_node(&self, node: &Rc<M::Node>) -> Option<StickyScrollNode<M>> {
        self.state()?.sticky_nodes.iter().find(|sticky_node| Rc::ptr_eq(&sticky_node.node, node)).cloned()
    }

    fn set_state(&self, state: Option<Rc<StickyScrollState<M>>>) {
        let previous_state = self.state();
        let was_visible = previous_state.as_ref().is_some_and(|state| state.count() > 0);
        let is_visible = state.as_ref().is_some_and(|state| state.count() > 0);

        // Se o estado não mudou, não faz nada.
        let unchanged = match (&previous_state, &state) {
            (Some(previous), Some(state)) if was_visible && is_visible => previous.equal(state),
            _ => !was_visible && !is_visible,
        };

        if unchanged {
            self.update_source_node_visibility(state.as_deref());
            return;
        }

        // Atualiza a visibilidade do widget se mudou.
        if was_visible != is_visible {
            self.set_visible(is_visible);
        }

        let Some(state) = state.filter(|_| is_visible) else {
            self.update_source_node_visibility(None);
            *self.previous_state.borrow_mut() = None;
            self.previous_elements.borrow_mut().clear();
            self.previous_element_heights.borrow_mut().clear();
            self.previous_state_disposables.clear();
            return;
        };

        let last_sticky_node = &state.sticky_nodes[state.count() - 1];

        // Se o novo estado só muda a posição do último nó, atualiza o `top` do último elemento.
        match previous_state.as_ref().filter(|previous| state.animation_state_changed(previous)) {
            Some(previous) => {
                let element = self.previous_elements.borrow()[previous.count() - 1];
                set_top(&self.ui, element, last_sticky_node.position);
            }
            // Cria os nós novos.
            None => self.render_state(&state),
        }

        *self.previous_state.borrow_mut() = Some(state.clone());

        self.update_source_node_visibility(Some(&state));
        self.update_root_height(&state);
    }

    fn update_source_node_visibility(&self, state: Option<&StickyScrollState<M>>) {
        let mut source_node_partially_visible = false;

        if let Some(state) = state {
            for (i, sticky_node) in state.sticky_nodes.iter().enumerate() {
                let sticky_element = self.previous_elements.borrow().get(i).copied();
                let mut node_partially_visible = sticky_node.source_node_partially_visible;

                if sticky_element.is_some() {
                    let source_node_bottom = self.view.get_element_top(sticky_node.start_index) + sticky_node.source_node_end;
                    let sticky_element_height = self.get_rendered_node_height(sticky_node, i);
                    let sticky_node_bottom = self.view.scroll_top() + sticky_node.position + sticky_element_height;
                    node_partially_visible = sticky_node_bottom < source_node_bottom;
                }

                if let Some(sticky_element) = sticky_element {
                    self.ui.tree_mut().toggle_class(sticky_element, "source-node-partially-visible", node_partially_visible);
                }

                source_node_partially_visible |= node_partially_visible;
            }
        }

        self.ui.tree_mut().toggle_class(self.root_dom_node, "source-node-partially-visible", source_node_partially_visible);
    }

    fn render_state(&self, state: &Rc<StickyScrollState<M>>) {
        self.previous_state_disposables.clear();

        // O original anexa do último nó para o primeiro: a ordem no DOM é a inversa da do estado.
        let mut elements: Vec<NodeId> = Vec::with_capacity(state.count());

        for sticky_index in (0..state.count()).rev() {
            let sticky_node = &state.sticky_nodes[sticky_index];

            let (element, disposable) = self.create_element(sticky_node);
            elements.push(element);

            self.ui.tree_mut().append_child(self.root_dom_node, element);
            self.previous_state_disposables.add(disposable);
        }

        elements.reverse();

        self.sticky_scroll_focus.update_elements(&elements, Some(state.clone()));

        *self.previous_elements.borrow_mut() = elements.clone();

        let element_heights = self.probe_dynamic_heights(state, &elements);
        *self.previous_element_heights.borrow_mut() = element_heights;
    }

    fn rerender(&self) {
        if let Some(previous_state) = self.state() {
            self.render_state(&previous_state);
            self.update_source_node_visibility(Some(&previous_state));
            self.update_root_height(&previous_state);
        }
    }

    fn update_root_height(&self, state: &StickyScrollState<M>) {
        let height = self.get_root_height(state);
        self.ui.tree_mut().set_bounds(self.root_dom_node, Rect::new(0.0, 0.0, 0.0, height));
    }

    fn get_root_height(&self, state: &StickyScrollState<M>) -> f64 {
        let last_sticky_node = &state.sticky_nodes[state.count() - 1];
        let last_sticky_element_height = self.get_rendered_node_height(last_sticky_node, state.count() - 1);

        (last_sticky_node.position + last_sticky_element_height).max(0.0)
    }

    fn get_rendered_node_height(&self, sticky_node: &StickyScrollNode<M>, index: usize) -> f64 {
        self.previous_element_heights.borrow().get(index).copied().unwrap_or(sticky_node.height).min(sticky_node.height)
    }

    fn probe_dynamic_heights(&self, state: &StickyScrollState<M>, elements: &[NodeId]) -> Vec<f64> {
        let mut height_changes: Vec<HeightChange> = Vec::new();
        let mut element_heights: Vec<f64> = state.sticky_nodes.iter().map(|node| node.height).collect();
        let options = tree_options(&self.tree);
        let max_node_height = options.as_ref().and_then(|options| options.update.sticky_scroll_max_node_height);
        let set_row_height = options.as_ref().is_none_or(|options| options.view.set_row_height);

        for (i, sticky_node) in state.sticky_nodes.iter().enumerate() {
            let element = elements[i];

            if !self.tree_delegate.has_dynamic_height(&sticky_node.node).unwrap_or(false) {
                let measured_height = self.ui.tree().bounds(element).height();

                if measured_height > 0.0 {
                    element_heights[i] = measured_height;
                }

                continue;
            }

            let measured_height = self.ui.tree().bounds(element).height();

            if measured_height <= 0.0 {
                continue;
            }

            let clamped_measured_height = clamp_node_height(max_node_height, measured_height);
            let rendered_height = if sticky_node.has_explicit_height { sticky_node.height } else { clamped_measured_height };
            element_heights[i] = rendered_height;

            // Sempre atualiza a altura visual do elemento fixado para a do conteúdo medido.
            if set_row_height {
                let bounds = self.ui.tree().bounds(element);
                self.ui.tree_mut().set_bounds(element, Rect::new(bounds.x0, bounds.y0, bounds.x1, bounds.y0 + rendered_height));
            }

            // Uma linha fixada pode representar só parte da origem: nunca encolhe a linha de origem.
            if !sticky_node.has_explicit_height && clamped_measured_height > self.view.get_element_height(sticky_node.start_index) {
                height_changes.push((sticky_node.start_index, clamped_measured_height));
            }
        }

        if !height_changes.is_empty() {
            self.on_did_change_height.fire(height_changes);
        }

        element_heights
    }

    fn create_element(&self, sticky_node: &StickyScrollNode<M>) -> (NodeId, DisposableRef) {
        let node_index = sticky_node.start_index;

        let options = tree_options(&self.tree);
        let max_node_height = options.as_ref().and_then(|options| options.update.sticky_scroll_max_node_height);
        let set_row_height = options.as_ref().is_none_or(|options| options.view.set_row_height);
        let clamped_height = clamp_node_height(max_node_height, sticky_node.height);

        // O contêiner da linha fixada.
        let sticky_element = {
            let mut tree = self.ui.tree_mut();
            let element = tree.create_detached(Role::ListItem);
            let height = if set_row_height { clamped_height } else { 0.0 };
            tree.set_bounds(element, Rect::new(0.0, sticky_node.position, 0.0, sticky_node.position + height));
            tree.set_fill(element, true, false);
            tree.style_mut(element).clip = true;
            tree.add_class(element, "monaco-tree-sticky-row");
            tree.add_class(element, "monaco-list-row");
            tree.toggle_class(element, "source-node-partially-visible", sticky_node.source_node_partially_visible);
            element
        };

        self.view.view.set_external_row_index(sticky_element, Some(node_index));

        // O renderer do nó.
        let node_template_id = self.tree_delegate.get_template_id(&sticky_node.node);
        let Some(renderer) = self.tree_renderers.iter().find(|renderer| renderer.template_id() == node_template_id).cloned() else {
            throw(&format!("No renderer found for template id {node_template_id}"));
        };

        // Renderiza o elemento.
        let details = ListElementRenderDetails { height: Some(sticky_node.height), on_scroll: None };
        let template_data = IListRenderer::render_template(&*renderer, sticky_element);
        renderer.render_element_detached(&sticky_node.node, node_index, &template_data, Some(&details));

        // Remove o elemento do DOM quando o estado é descartado.
        let ui = self.ui.clone();
        let view = self.view.clone();
        let node = sticky_node.node.clone();
        let disposable = to_disposable(move || {
            IListRenderer::dispose_element(&*renderer, &node, node_index as i64, &template_data, Some(&details));
            IListRenderer::dispose_template(&*renderer, &template_data);
            view.view.set_external_row_index(sticky_element, None);
            ui.remove(sticky_element);
        });

        (sticky_element, disposable)
    }

    fn set_visible(&self, visible: bool) {
        {
            let mut tree = self.ui.tree_mut();
            tree.toggle_class(self.root_dom_node, "empty", !visible);
            tree.style_mut(self.root_dom_node).visible = visible;
        }

        if !visible {
            self.sticky_scroll_focus.update_elements(&[], None);
        }
    }
}

impl<M: ITreeModel> IDisposable for StickyScrollWidget<M>
where
    M::Element: Eq + Hash,
{
    fn dispose(&self) {
        self.on_did_change_height.dispose();
        self.sticky_scroll_focus.dispose();
        self.previous_state_disposables.dispose();
        self.ui.remove(self.root_dom_node);
    }

    fn constructor_name(&self) -> &'static str {
        "StickyScrollWidget"
    }
}

/// `StickyScrollFocus<T, TFilterData, TRef>`.
struct StickyScrollFocus<M: ITreeModel>
where
    M::Element: Eq + Hash,
{
    base: Rc<Disposable>,
    ui: Ui,
    container: NodeId,
    view: Rc<List<Rc<M::Node>>>,
    focused_index: Cell<i64>,
    elements: RefCell<Vec<NodeId>>,
    state: RefCell<Option<Rc<StickyScrollState<M>>>>,
    on_did_change_has_focus: Rc<Emitter<bool>>,
    dom_has_focus: Cell<bool>,
}

impl<M: ITreeModel> StickyScrollFocus<M>
where
    M::Element: Eq + Hash,
{
    fn new(ui: &Ui, container: NodeId, view: Rc<List<Rc<M::Node>>>) -> Rc<Self> {
        let base = Disposable::new();
        let on_did_change_has_focus = registered_emitter(&base);
        let list_node = view.get_html_element();

        let this = Rc::new(Self {
            base,
            ui: ui.clone(),
            container,
            view,
            focused_index: Cell::new(-1),
            elements: RefCell::new(Vec::new()),
            state: RefCell::new(None),
            on_did_change_has_focus,
            dom_has_focus: Cell::new(false),
        });

        fn listen<M: ITreeModel>(this: &Rc<StickyScrollFocus<M>>, node: NodeId, kind: EventKind, capture: bool, handler: impl Fn(&StickyScrollFocus<M>, &mut je_ui::Event) + 'static)
        where
            M::Element: Eq + Hash,
        {
            let weak = Rc::downgrade(this);
            this.base.register(this.ui.listen(node, kind, capture, move |e| {
                if let Some(this) = weak.upgrade() {
                    handler(&this, e);
                }
            }));
        }

        listen(&this, container, EventKind::Focus, false, |this, _| this.on_focus());
        listen(&this, container, EventKind::Blur, false, |this, _| this.on_blur());
        // `view.onDidFocus` (o `focus` do nó da lista, em captura: também dispara para o container).
        listen(&this, list_node, EventKind::Focus, true, |this, _| this.toggle_sticky_scroll_focused(false));
        listen(&this, list_node, EventKind::KeyDown, false, |this, e| this.on_key_down(e));
        listen(&this, list_node, EventKind::PointerDown, false, |this, e| this.on_mouse_down(e));

        this
    }

    fn dom_has_focus(&self) -> bool {
        self.dom_has_focus.get()
    }

    fn set_dom_has_focus(&self, has_focus: bool) {
        if has_focus != self.dom_has_focus.get() {
            self.on_did_change_has_focus.fire(has_focus);
            self.dom_has_focus.set(has_focus);
        }
    }

    fn state(&self) -> Option<Rc<StickyScrollState<M>>> {
        self.state.borrow().clone()
    }

    fn on_key_down(&self, e: &mut je_ui::Event) {
        let Some(KeyData { key_code, .. }) = e.key else { return };

        // Navegação do sticky scroll.
        let Some(state) = self.state().filter(|_| self.dom_has_focus()) else { return };

        match key_code {
            // Para cima.
            KeyCode::UpArrow => {
                self.set_focused_element((self.focused_index.get() - 1).max(0));
                e.stop(true);
            }
            // Para baixo: se o último nó fixado está com foco, o foco vai para o primeiro filho dele.
            KeyCode::DownArrow | KeyCode::RightArrow => {
                if self.focused_index.get() >= state.count() as i64 - 1 {
                    let node_index_to_focus = state.sticky_nodes[state.count() - 1].start_index + 1;
                    self.view.dom_focus();
                    self.view.set_focus(vec![node_index_to_focus], None);
                    self.scroll_node_under_widget(node_index_to_focus, &state);
                } else {
                    self.set_focused_element(self.focused_index.get() + 1);
                }

                e.stop(true);
            }
            _ => {}
        }
    }

    fn on_mouse_down(&self, e: &mut je_ui::Event) {
        let tree = self.ui.tree();
        let is_sticky = tree.has_class(e.target, "monaco-tree-sticky-container") || tree.has_class(e.target, "monaco-tree-sticky-row");
        drop(tree);

        if !is_sticky {
            return;
        }

        e.stop(true);
    }

    fn update_elements(&self, elements: &[NodeId], state: Option<Rc<StickyScrollState<M>>>) {
        if let Some(state) = &state {
            if state.count() == 0 {
                throw("Sticky scroll state must be undefined when there are no sticky nodes");
            }

            if state.count() != elements.len() {
                throw("Sticky scroll focus received illigel state");
            }
        }

        let previous_index = self.focused_index.get();
        self.remove_focus();

        *self.elements.borrow_mut() = elements.to_vec();
        *self.state.borrow_mut() = state.clone();

        match state {
            Some(state) => {
                let new_focused_index = previous_index.clamp(0, state.count() as i64 - 1);
                self.set_focus(new_focused_index);
            }
            None => {
                if self.dom_has_focus() {
                    self.view.dom_focus();
                }
            }
        }
    }

    fn set_focused_element(&self, sticky_index: i64) {
        // Não implica que o widget tem (ou vai ter) o foco.
        let Some(state) = self.state() else { throw("Cannot set focus when state is undefined") };

        self.set_focus(sticky_index);

        if sticky_index < state.count() as i64 - 1 {
            return;
        }

        // Se o último nó fixado não está inteiro visível, rola até ele.
        if state.last_node_partially_visible() {
            let last_sticky_node = &state.sticky_nodes[sticky_index as usize];
            self.scroll_node_under_widget(last_sticky_node.end_index + 1, &state);
        }
    }

    fn scroll_node_under_widget(&self, node_index: usize, state: &StickyScrollState<M>) {
        let last_sticky_node = &state.sticky_nodes[state.count() - 1];
        let second_last_sticky_node = if state.count() > 1 { state.sticky_nodes.get(state.count() - 2) } else { None };

        let element_scroll_top = self.view.get_element_top(node_index);
        let element_target_view_top = second_last_sticky_node.map_or(last_sticky_node.height, |second_last| second_last.position + second_last.height + last_sticky_node.height);
        self.view.set_scroll_top(element_scroll_top - element_target_view_top);
    }

    fn get_focus(&self) -> Option<M::Element> {
        let state = self.state()?;
        let index = usize::try_from(self.focused_index.get()).ok()?;

        Some(state.sticky_nodes[index].node.element())
    }

    fn dom_focus(&self) {
        if self.state().is_none() {
            throw("Cannot focus when state is undefined");
        }

        self.ui.focus(self.container);
    }

    fn focused_last(&self) -> bool {
        self.state().is_some() && self.ui.tree().has_class(self.view.get_html_element(), "sticky-scroll-focused")
    }

    fn remove_focus(&self) {
        let focused_index = self.focused_index.get();

        if focused_index == -1 {
            return;
        }

        let element = self.elements.borrow()[focused_index as usize];
        self.toggle_element_focus(element, false);
        self.focused_index.set(-1);
    }

    fn set_focus(&self, new_focus_index: i64) {
        let state = self.state();

        if new_focus_index < 0 {
            throw("addFocus() can not remove focus");
        }

        let Some(state) = state else { throw("Cannot set focus index when state is undefined") };

        if new_focus_index >= state.count() as i64 {
            throw("Cannot set focus index to an index that does not exist");
        }

        let old_index = self.focused_index.get();

        if old_index >= 0 {
            let element = self.elements.borrow()[old_index as usize];
            self.toggle_element_focus(element, false);
        }

        let element = self.elements.borrow()[new_focus_index as usize];
        self.toggle_element_focus(element, true);
        self.focused_index.set(new_focus_index);
    }

    fn toggle_element_focus(&self, element: NodeId, focused: bool) {
        self.toggle_element_active_focus(element, focused && self.dom_has_focus());
        self.toggle_element_passive_focus(element, focused);
    }

    fn toggle_current_element_active_focus(&self, focused: bool) {
        let focused_index = self.focused_index.get();

        if focused_index == -1 {
            return;
        }

        let element = self.elements.borrow()[focused_index as usize];
        self.toggle_element_active_focus(element, focused);
    }

    /// O foco ativo vale quando o sticky scroll tem o foco.
    fn toggle_element_active_focus(&self, element: NodeId, focused: bool) {
        self.ui.tree_mut().toggle_class(element, "focused", focused);
    }

    /// O foco passivo mostra o foco quando o sticky scroll não o tem (por exemplo, com o menu de contexto).
    fn toggle_element_passive_focus(&self, element: NodeId, focused: bool) {
        self.ui.tree_mut().toggle_class(element, "passive-focused", focused);
    }

    /// Se o último foco da árvore foi o sticky scroll e não a lista; só some quando o foco volta à árvore.
    fn toggle_sticky_scroll_focused(&self, focused: bool) {
        self.ui.tree_mut().toggle_class(self.view.get_html_element(), "sticky-scroll-focused", focused);
    }

    fn on_focus(&self) {
        if self.state().is_none() || self.elements.borrow().is_empty() {
            throw("Cannot focus when state is undefined or elements are empty");
        }

        self.set_dom_has_focus(true);
        self.toggle_sticky_scroll_focused(true);
        self.toggle_current_element_active_focus(true);

        if self.focused_index.get() == -1 {
            self.set_focus(0);
        }
    }

    fn on_blur(&self) {
        self.set_dom_has_focus(false);
        self.toggle_current_element_active_focus(false);
    }
}

impl<M: ITreeModel> IDisposable for StickyScrollFocus<M>
where
    M::Element: Eq + Hash,
{
    fn dispose(&self) {
        self.toggle_sticky_scroll_focused(false);
        self.on_did_change_has_focus.fire(false);
        self.base.dispose();
    }

    fn constructor_name(&self) -> &'static str {
        "StickyScrollFocus"
    }
}
