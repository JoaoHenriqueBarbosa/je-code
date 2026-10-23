//! Porte de `src/vs/base/browser/ui/tree/indexTreeModel.ts`: o modelo de árvore endereçado por índices
//! (`[0, 2, 1]` é o segundo filho do terceiro filho do primeiro nó), com colapso, filtro e a lista
//! "renderizada" (os nós visíveis e não colapsados, em ordem de pré-ordem) que a lista da UI consome.
//!
//! Do TypeScript para o Rust: o nó é um `Rc<TreeNode<T, F>>` com os campos que mudam em `Cell`/`RefCell`
//! (o original muta o objeto, e quem recebe o nó num evento o vê mudar); o pai é `Weak`. A raiz guarda o
//! `rootElement` que o chamador passou. O `Iterable<ITreeElement>` é `Vec`.

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use je_base::async_::Delayer;
use je_base::diff::{ISequence, LcsDiff, SequenceElements};
use je_base::event::{Emitter, Event as BaseEvent, EventBufferer};
use je_base::lifecycle::IDisposable;
use je_base::promise::MaybePromise;
use je_base::symbols::Delay;

use crate::common::throw;
use crate::list::list_types::IIdentityProvider;
use crate::tree::tree_types::{
    CollapseStateChangeEvent, ITreeElement, ITreeFilter, TreeFilterResult, TreeListSpliceData, TreeModelSpliceEvent, TreeVisibility, VisibilityValue, tree_error,
};

/// O callback de criação e de remoção de nó.
pub type NodeCallback<T, F> = Rc<dyn Fn(&Rc<TreeNode<T, F>>)>;

/// `IIndexTreeNode<T, TFilterData>` (que estende `ITreeNode`).
pub struct TreeNode<T, F = ()> {
    pub parent: RefCell<Weak<TreeNode<T, F>>>,
    pub element: T,
    pub children: RefCell<Vec<Rc<TreeNode<T, F>>>>,
    pub depth: usize,
    pub visible_children_count: Cell<usize>,
    pub visible_child_index: Cell<i64>,
    pub collapsible: Cell<bool>,
    pub collapsed: Cell<bool>,
    pub render_node_count: Cell<usize>,
    pub visibility: Cell<TreeVisibility>,
    pub visible: Cell<bool>,
    pub filter_data: RefCell<Option<F>>,
    pub last_diff_ids: RefCell<Option<Vec<String>>>,
}

impl<T, F> TreeNode<T, F> {
    pub fn parent(&self) -> Option<Rc<TreeNode<T, F>>> {
        self.parent.borrow().upgrade()
    }
}

/// O tipo do nó como o `ITreeNode` do original o expõe.
pub type ITreeNode<T, F = ()> = TreeNode<T, F>;

/// `isFilterResult`: o resultado tem `visibility`?
pub fn is_filter_result<F>(result: &TreeFilterResult<F>) -> bool {
    matches!(result, TreeFilterResult::Data { .. })
}

/// `getVisibleState`.
pub fn get_visible_state(visibility: VisibilityValue) -> TreeVisibility {
    match visibility {
        VisibilityValue::Bool(true) => TreeVisibility::Visible,
        VisibilityValue::Bool(false) => TreeVisibility::Hidden,
        VisibilityValue::Visibility(v) => v,
    }
}

/// `IIndexTreeModelOptions<T, TFilterData>`.
pub struct IndexTreeModelOptions<T, F> {
    pub collapse_by_default: bool,
    pub allow_non_collapsible_parents: bool,
    pub filter: Option<Rc<dyn ITreeFilter<T, F>>>,
    pub auto_expand_single_children: bool,
}

impl<T, F> Default for IndexTreeModelOptions<T, F> {
    fn default() -> Self {
        Self { collapse_by_default: false, allow_non_collapsible_parents: false, filter: None, auto_expand_single_children: false }
    }
}

/// `IIndexTreeModelSpliceOptions<T, TFilterData>`.
pub struct IndexTreeModelSpliceOptions<T, F> {
    pub diff_depth: Option<usize>,
    pub diff_identity_provider: Option<Rc<dyn IIdentityProvider<T>>>,
    pub on_did_create_node: Option<NodeCallback<T, F>>,
    pub on_did_delete_node: Option<NodeCallback<T, F>>,
}

impl<T, F> Default for IndexTreeModelSpliceOptions<T, F> {
    fn default() -> Self {
        Self { diff_depth: None, diff_identity_provider: None, on_did_create_node: None, on_did_delete_node: None }
    }
}

impl<T, F> Clone for IndexTreeModelSpliceOptions<T, F> {
    fn clone(&self) -> Self {
        Self {
            diff_depth: self.diff_depth,
            diff_identity_provider: self.diff_identity_provider.clone(),
            on_did_create_node: self.on_did_create_node.clone(),
            on_did_delete_node: self.on_did_delete_node.clone(),
        }
    }
}

#[derive(Clone, Copy)]
enum CollapseStateUpdate {
    Collapsible { collapsible: bool },
    Collapsed { collapsed: bool, recursive: bool },
}

struct IdSequence(Vec<String>);

impl ISequence for IdSequence {
    fn get_elements(&self) -> SequenceElements {
        SequenceElements::Strings(self.0.iter().map(|s| s.as_str().into()).collect())
    }
}

struct ListIndexInfo<T, F> {
    parent_node: Rc<TreeNode<T, F>>,
    list_index: i64,
    revealed: bool,
    visible: bool,
}

pub struct IndexTreeModel<T: Clone + 'static, F: Clone + 'static = ()> {
    weak_self: Weak<IndexTreeModel<T, F>>,
    user: String,
    /// O `rootRef`: `[]`.
    pub root_ref: Vec<usize>,
    root: Rc<TreeNode<T, F>>,
    event_bufferer: Rc<EventBufferer>,
    on_did_splice_model: Rc<Emitter<TreeModelSpliceEvent<TreeNode<T, F>>>>,
    on_did_splice_rendered_nodes: Rc<Emitter<TreeListSpliceData<TreeNode<T, F>>>>,
    on_did_change_collapse_state_raw: Rc<Emitter<CollapseStateChangeEvent<TreeNode<T, F>>>>,
    on_did_change_render_node_count_raw: Rc<Emitter<Rc<TreeNode<T, F>>>>,
    collapse_by_default: bool,
    allow_non_collapsible_parents: bool,
    filter: Option<Rc<dyn ITreeFilter<T, F>>>,
    auto_expand_single_children: bool,
    refilter_delayer: Delayer<()>,
}

impl<T: Clone + 'static, F: Clone + 'static> IndexTreeModel<T, F> {
    pub fn new(user: &str, root_element: T, options: IndexTreeModelOptions<T, F>) -> Rc<Self> {
        let root = Rc::new(TreeNode {
            parent: RefCell::new(Weak::new()),
            element: root_element,
            children: RefCell::new(Vec::new()),
            depth: 0,
            visible_children_count: Cell::new(0),
            visible_child_index: Cell::new(-1),
            collapsible: Cell::new(false),
            collapsed: Cell::new(false),
            render_node_count: Cell::new(0),
            visibility: Cell::new(TreeVisibility::Visible),
            visible: Cell::new(true),
            filter_data: RefCell::new(None),
            last_diff_ids: RefCell::new(None),
        });
        Rc::new_cyclic(|weak| Self {
            weak_self: weak.clone(),
            user: user.to_owned(),
            root_ref: Vec::new(),
            root,
            event_bufferer: EventBufferer::new(),
            on_did_splice_model: Emitter::new(),
            on_did_splice_rendered_nodes: Emitter::new(),
            on_did_change_collapse_state_raw: Emitter::new(),
            on_did_change_render_node_count_raw: Emitter::new(),
            collapse_by_default: options.collapse_by_default,
            allow_non_collapsible_parents: options.allow_non_collapsible_parents,
            filter: options.filter,
            auto_expand_single_children: options.auto_expand_single_children,
            refilter_delayer: Delayer::new(Delay::Microtask),
        })
    }

    pub fn on_did_splice_model(&self) -> BaseEvent<TreeModelSpliceEvent<TreeNode<T, F>>> {
        self.on_did_splice_model.event()
    }

    pub fn on_did_splice_rendered_nodes(&self) -> BaseEvent<TreeListSpliceData<TreeNode<T, F>>> {
        self.on_did_splice_rendered_nodes.event()
    }

    pub fn on_did_change_collapse_state(&self) -> BaseEvent<CollapseStateChangeEvent<TreeNode<T, F>>> {
        self.event_bufferer.wrap_event(&self.on_did_change_collapse_state_raw.event())
    }

    pub fn on_did_change_render_node_count(&self) -> BaseEvent<Rc<TreeNode<T, F>>> {
        self.event_bufferer.wrap_event(&self.on_did_change_render_node_count_raw.event())
    }

    pub fn splice(&self, location: &[usize], delete_count: usize, to_insert: Vec<ITreeElement<T>>, options: &IndexTreeModelSpliceOptions<T, F>) {
        if location.is_empty() {
            throw(&tree_error(&self.user, "Invalid tree location").to_string());
        }

        if let Some(identity) = options.diff_identity_provider.clone() {
            self.splice_smart(&identity, location, delete_count, to_insert, options, options.diff_depth.unwrap_or(0));
        } else {
            self.splice_simple(location, delete_count, to_insert, options);
        }
    }

    fn splice_smart(
        &self,
        identity: &Rc<dyn IIdentityProvider<T>>,
        location: &[usize],
        delete_count: usize,
        to_insert: Vec<ITreeElement<T>>,
        options: &IndexTreeModelSpliceOptions<T, F>,
        recurse_levels: usize,
    ) {
        let parent_node = self.get_parent_node_with_list_index(location).parent_node;

        let Some(last_diff_ids) = parent_node.last_diff_ids.borrow().clone() else {
            return self.splice_simple(location, delete_count, to_insert, options);
        };

        let index = location[location.len() - 1];
        let modified: Vec<String> = {
            let children = parent_node.children.borrow();
            children[..index.min(children.len())]
                .iter()
                .map(|c| identity.get_id(&c.element))
                .chain(to_insert.iter().map(|e| identity.get_id(&e.element)))
                .chain(children[index.saturating_add(delete_count).min(children.len())..].iter().map(|c| identity.get_id(&c.element)))
                .collect()
        };

        let mut diff = LcsDiff::new(Rc::new(IdSequence(last_diff_ids)), Rc::new(IdSequence(modified)), None);
        let result = diff.compute_diff(false);

        // Esta função ordena os elementos em O(n log n), então pode ser lenta: com `quitEarly`, volta ao simples.
        if result.quit_early {
            *parent_node.last_diff_ids.borrow_mut() = None;
            return self.splice_simple(location, delete_count, to_insert, options);
        }

        let location_prefix = &location[..location.len() - 1];

        let recurse_splice = |mut from_original: usize, mut from_modified: usize, count: usize| {
            if recurse_levels > 0 {
                for _ in 0..count {
                    from_original -= 1;
                    from_modified -= 1;

                    let mut child_location = location_prefix.to_vec();
                    child_location.push(from_original);
                    child_location.push(0);
                    self.splice_smart(identity, &child_location, usize::MAX >> 11, to_insert[from_modified].children.clone().unwrap_or_default(), options, recurse_levels - 1);
                }
            }
        };

        let mut last_start_o = parent_node.children.borrow().len().min(index.saturating_add(delete_count));
        let mut last_start_m = to_insert.len();
        let mut changes = result.changes;
        changes.sort_by_key(|c| std::cmp::Reverse(c.original_start));

        for change in changes {
            let (original_start, original_length) = (change.original_start as usize, change.original_length as usize);
            recurse_splice(last_start_o, last_start_m, last_start_o - (original_start + original_length));

            last_start_o = original_start;
            last_start_m = change.modified_start as usize - index;

            let mut at = location_prefix.to_vec();
            at.push(last_start_o);
            self.splice_simple(&at, original_length, to_insert[last_start_m..last_start_m + change.modified_length as usize].to_vec(), options);
        }

        recurse_splice(last_start_o, last_start_m, last_start_o);
    }

    fn splice_simple(&self, location: &[usize], delete_count: usize, to_insert: Vec<ITreeElement<T>>, options: &IndexTreeModelSpliceOptions<T, F>) {
        let ListIndexInfo { parent_node, list_index, revealed, visible } = self.get_parent_node_with_list_index(location);
        let mut tree_list_elements_to_insert: Vec<Rc<TreeNode<T, F>>> = Vec::new();
        let parent_visibility = if parent_node.visible.get() { TreeVisibility::Visible } else { TreeVisibility::Hidden };
        let nodes_to_insert_iterator: Vec<Rc<TreeNode<T, F>>> =
            to_insert.iter().map(|el| self.create_tree_node(el, &parent_node, parent_visibility, revealed, &mut tree_list_elements_to_insert, options.on_did_create_node.as_ref())).collect();

        let last_index = location[location.len() - 1];

        // Acha o índice de filho visível a partir do qual inserir (o do primeiro visível de `last_index` para trás).
        let mut visible_child_start_index = 0;
        {
            let children = parent_node.children.borrow();
            let mut i = last_index as i64;
            while i >= 0 && (i as usize) < children.len() {
                let child = &children[i as usize];
                if child.visible.get() {
                    visible_child_start_index = child.visible_child_index.get();
                    break;
                }
                i -= 1;
            }
        }

        let mut nodes_to_insert: Vec<Rc<TreeNode<T, F>>> = Vec::new();
        let mut inserted_visible_children_count = 0;
        let mut render_node_count = 0;

        for child in nodes_to_insert_iterator {
            render_node_count += child.render_node_count.get();

            if child.visible.get() {
                child.visible_child_index.set(visible_child_start_index + inserted_visible_children_count);
                inserted_visible_children_count += 1;
            }
            nodes_to_insert.push(child);
        }

        let deleted_nodes = je_base::arrays::splice(&mut parent_node.children.borrow_mut(), last_index as i64, delete_count, &nodes_to_insert);

        // Atualiza o `lastDiffIds` do pai.
        match &options.diff_identity_provider {
            None => *parent_node.last_diff_ids.borrow_mut() = None,
            Some(identity) => {
                let mut last = parent_node.last_diff_ids.borrow_mut();
                if let Some(ids) = last.as_mut() {
                    let new_ids: Vec<String> = nodes_to_insert.iter().map(|n| identity.get_id(&n.element)).collect();
                    je_base::arrays::splice(ids, last_index as i64, delete_count, &new_ids);
                } else {
                    *last = Some(parent_node.children.borrow().iter().map(|n| identity.get_id(&n.element)).collect());
                }
            }
        }

        // Atualiza o `visibleChildIndex` dos irmãos depois dos removidos.
        let deleted_visible_children_count = deleted_nodes.iter().filter(|c| c.visible.get()).count();

        if deleted_visible_children_count != 0 {
            let children = parent_node.children.borrow();
            for child in &children[(last_index + nodes_to_insert.len()).min(children.len())..] {
                if child.visible.get() {
                    child.visible_child_index.set(child.visible_child_index.get() - deleted_visible_children_count as i64);
                }
            }
        }

        parent_node.visible_children_count.set((parent_node.visible_children_count.get() as i64 + inserted_visible_children_count - deleted_visible_children_count as i64) as usize);

        if let (false, Some(on_did_delete_node)) = (deleted_nodes.is_empty(), options.on_did_delete_node.as_ref()) {
            fn visit<T, F>(node: &Rc<TreeNode<T, F>>, f: &NodeCallback<T, F>) {
                f(node);
                for child in node.children.borrow().iter() {
                    visit(child, f);
                }
            }
            for node in &deleted_nodes {
                visit(node, on_did_delete_node);
            }
        }

        if revealed && visible {
            let visible_delete_count: usize = deleted_nodes.iter().map(|node| if node.visible.get() { node.render_node_count.get() } else { 0 }).sum();

            self.update_ancestors_render_node_count(Some(&parent_node), render_node_count as i64 - visible_delete_count as i64);
            self.on_did_splice_rendered_nodes.fire(TreeListSpliceData { start: list_index as usize, delete_count: visible_delete_count, elements: tree_list_elements_to_insert });
        }

        self.on_did_splice_model.fire(TreeModelSpliceEvent { inserted_nodes: nodes_to_insert, deleted_nodes });

        let mut node = Some(parent_node);
        while let Some(n) = node {
            if n.visibility.get() == TreeVisibility::Recurse {
                let weak = self.weak_self.clone();
                let _ = self.refilter_delayer.trigger(
                    move || {
                        if let Some(model) = weak.upgrade() {
                            model.refilter();
                        }
                        MaybePromise::Value(())
                    },
                    None,
                );
                break;
            }
            node = n.parent();
        }
    }

    pub fn rerender(&self, location: &[usize]) {
        if location.is_empty() {
            throw(&tree_error(&self.user, "Invalid tree location").to_string());
        }

        let (node, list_index, revealed, _) = self.get_tree_node_with_list_index(location);

        if node.visible.get() && revealed {
            self.on_did_splice_rendered_nodes.fire(TreeListSpliceData { start: list_index as usize, delete_count: 1, elements: vec![node] });
        }
    }

    pub fn has(&self, location: &[usize]) -> bool {
        self.has_tree_node(location, &self.root)
    }

    pub fn get_list_index(&self, location: &[usize]) -> i64 {
        let (_, list_index, revealed, visible) = self.get_tree_node_with_list_index(location);
        if visible && revealed { list_index } else { -1 }
    }

    pub fn get_list_render_count(&self, location: &[usize]) -> usize {
        self.get_tree_node(location).render_node_count.get()
    }

    pub fn is_collapsible(&self, location: &[usize]) -> bool {
        self.get_tree_node(location).collapsible.get()
    }

    pub fn set_collapsible(&self, location: &[usize], collapsible: Option<bool>) -> bool {
        let node = self.get_tree_node(location);
        let collapsible = collapsible.unwrap_or(!node.collapsible.get());
        let update = CollapseStateUpdate::Collapsible { collapsible };
        self.event_bufferer.buffer_events(|| self.set_collapse_state(location, update))
    }

    pub fn is_collapsed(&self, location: &[usize]) -> bool {
        self.get_tree_node(location).collapsed.get()
    }

    pub fn set_collapsed(&self, location: &[usize], collapsed: Option<bool>, recursive: Option<bool>) -> bool {
        let node = self.get_tree_node(location);
        let collapsed = collapsed.unwrap_or(!node.collapsed.get());
        let update = CollapseStateUpdate::Collapsed { collapsed, recursive: recursive.unwrap_or(false) };
        self.event_bufferer.buffer_events(|| self.set_collapse_state(location, update))
    }

    fn set_collapse_state(&self, location: &[usize], update: CollapseStateUpdate) -> bool {
        let (node, list_index, revealed, _) = self.get_tree_node_with_list_index(location);

        let result = self.set_list_node_collapse_state(&node, list_index, revealed, update);

        if !Rc::ptr_eq(&node, &self.root)
            && self.auto_expand_single_children
            && result
            && let CollapseStateUpdate::Collapsed { recursive: false, .. } = update
            && node.collapsible.get()
            && !node.collapsed.get()
        {
            let mut only_visible_child_index: i64 = -1;

            for (i, child) in node.children.borrow().iter().enumerate() {
                if child.visible.get() {
                    if only_visible_child_index > -1 {
                        only_visible_child_index = -1;
                        break;
                    } else {
                        only_visible_child_index = i as i64;
                    }
                }
            }

            if only_visible_child_index > -1 {
                let mut child_location = location.to_vec();
                child_location.push(only_visible_child_index as usize);
                self.set_collapse_state(&child_location, update);
            }
        }

        result
    }

    fn set_list_node_collapse_state(&self, node: &Rc<TreeNode<T, F>>, list_index: i64, revealed: bool, update: CollapseStateUpdate) -> bool {
        let result = self.set_node_collapse_state(node, update, false);

        if !revealed || !node.visible.get() || !result {
            return result;
        }

        let previous_render_node_count = node.render_node_count.get() as i64;
        let to_insert = self.update_node_after_collapse_change(node);
        let delete_count = previous_render_node_count - if list_index == -1 { 0 } else { 1 };

        self.on_did_splice_rendered_nodes.fire(TreeListSpliceData { start: (list_index + 1) as usize, delete_count: delete_count as usize, elements: to_insert[1..].to_vec() });
        result
    }

    fn set_node_collapse_state(&self, node: &Rc<TreeNode<T, F>>, update: CollapseStateUpdate, deep: bool) -> bool {
        let mut result: bool;

        if Rc::ptr_eq(node, &self.root) {
            result = false;
        } else {
            match update {
                CollapseStateUpdate::Collapsible { collapsible } => {
                    result = node.collapsible.get() != collapsible;
                    node.collapsible.set(collapsible);
                }
                CollapseStateUpdate::Collapsed { collapsed, .. } => {
                    if !node.collapsible.get() {
                        result = false;
                    } else {
                        result = node.collapsed.get() != collapsed;
                        node.collapsed.set(collapsed);
                    }
                }
            }

            if result {
                self.on_did_change_collapse_state_raw.fire(CollapseStateChangeEvent { node: node.clone(), deep });
            }
        }

        if let CollapseStateUpdate::Collapsed { recursive: true, .. } = update {
            let children = node.children.borrow().clone();
            for child in &children {
                result = self.set_node_collapse_state(child, update, true) || result;
            }
        }

        result
    }

    pub fn expand_to(&self, location: &[usize]) {
        self.event_bufferer.buffer_events(|| {
            let mut node = self.get_tree_node(location);
            let mut location = location.to_vec();

            while let Some(parent) = node.parent() {
                node = parent;
                location.pop();

                if node.collapsed.get() {
                    self.set_collapse_state(&location, CollapseStateUpdate::Collapsed { collapsed: false, recursive: false });
                }
            }
        });
    }

    pub fn refilter(&self) {
        let previous_render_node_count = self.root.render_node_count.get();
        let to_insert = self.update_node_after_filter_change(&self.root);
        self.on_did_splice_rendered_nodes.fire(TreeListSpliceData { start: 0, delete_count: previous_render_node_count, elements: to_insert });
        self.refilter_delayer.cancel();
    }

    fn create_tree_node(
        &self,
        tree_element: &ITreeElement<T>,
        parent: &Rc<TreeNode<T, F>>,
        parent_visibility: TreeVisibility,
        revealed: bool,
        tree_list_elements: &mut Vec<Rc<TreeNode<T, F>>>,
        on_did_create_node: Option<&NodeCallback<T, F>>,
    ) -> Rc<TreeNode<T, F>> {
        let node = Rc::new(TreeNode {
            parent: RefCell::new(Rc::downgrade(parent)),
            element: tree_element.element.clone(),
            children: RefCell::new(Vec::new()),
            depth: parent.depth + 1,
            visible_children_count: Cell::new(0),
            visible_child_index: Cell::new(-1),
            collapsible: Cell::new(tree_element.collapsible.unwrap_or(tree_element.collapsed.is_some())),
            collapsed: Cell::new(tree_element.collapsed.unwrap_or(self.collapse_by_default)),
            render_node_count: Cell::new(1),
            visibility: Cell::new(TreeVisibility::Visible),
            visible: Cell::new(true),
            filter_data: RefCell::new(None),
            last_diff_ids: RefCell::new(None),
        });

        let visibility = self.filter_node(&node, parent_visibility);
        node.visibility.set(visibility);

        if revealed {
            tree_list_elements.push(node.clone());
        }

        let child_revealed = revealed && visibility != TreeVisibility::Hidden && !node.collapsed.get();

        let mut visible_children_count = 0;
        let mut render_node_count = 1;

        for el in tree_element.children.iter().flatten() {
            let child = self.create_tree_node(el, &node, visibility, child_revealed, tree_list_elements, on_did_create_node);
            render_node_count += child.render_node_count.get();

            if child.visible.get() {
                child.visible_child_index.set(visible_children_count);
                visible_children_count += 1;
            }
            node.children.borrow_mut().push(child);
        }

        if !self.allow_non_collapsible_parents {
            node.collapsible.set(node.collapsible.get() || !node.children.borrow().is_empty());
        }

        node.visible_children_count.set(visible_children_count as usize);
        node.visible.set(if visibility == TreeVisibility::Recurse { visible_children_count > 0 } else { visibility == TreeVisibility::Visible });

        if !node.visible.get() {
            node.render_node_count.set(0);

            if revealed {
                tree_list_elements.pop();
            }
        } else if !node.collapsed.get() {
            node.render_node_count.set(render_node_count);
        }

        if let Some(f) = on_did_create_node {
            f(&node);
        }

        node
    }

    fn update_node_after_collapse_change(&self, node: &Rc<TreeNode<T, F>>) -> Vec<Rc<TreeNode<T, F>>> {
        let previous_render_node_count = node.render_node_count.get() as i64;
        let mut result = Vec::new();
        self.do_update_node_after_collapse_change(node, &mut result);
        self.update_ancestors_render_node_count(node.parent().as_ref(), result.len() as i64 - previous_render_node_count);
        result
    }

    fn do_update_node_after_collapse_change(&self, node: &Rc<TreeNode<T, F>>, result: &mut Vec<Rc<TreeNode<T, F>>>) -> usize {
        if !node.visible.get() {
            return 0;
        }

        result.push(node.clone());
        node.render_node_count.set(1);

        if !node.collapsed.get() {
            let children = node.children.borrow().clone();
            for child in &children {
                let n = self.do_update_node_after_collapse_change(child, result);
                node.render_node_count.set(node.render_node_count.get() + n);
            }
        }

        self.on_did_change_render_node_count_raw.fire(node.clone());
        node.render_node_count.get()
    }

    fn update_node_after_filter_change(&self, node: &Rc<TreeNode<T, F>>) -> Vec<Rc<TreeNode<T, F>>> {
        let previous_render_node_count = node.render_node_count.get() as i64;
        let mut result = Vec::new();
        self.do_update_node_after_filter_change(node, if node.visible.get() { TreeVisibility::Visible } else { TreeVisibility::Hidden }, &mut result, true);
        self.update_ancestors_render_node_count(node.parent().as_ref(), result.len() as i64 - previous_render_node_count);
        result
    }

    fn do_update_node_after_filter_change(&self, node: &Rc<TreeNode<T, F>>, parent_visibility: TreeVisibility, result: &mut Vec<Rc<TreeNode<T, F>>>, revealed: bool) -> bool {
        let is_root = Rc::ptr_eq(node, &self.root);
        let mut visibility = None;

        if !is_root {
            let v = self.filter_node(node, parent_visibility);

            if v == TreeVisibility::Hidden {
                node.visible.set(false);
                node.render_node_count.set(0);
                return false;
            }

            if revealed {
                result.push(node.clone());
            }
            visibility = Some(v);
        }

        let result_start_length = result.len();
        node.render_node_count.set(if is_root { 0 } else { 1 });

        let mut has_visible_descendants = false;
        // `visibility!` no original é `undefined` na raiz, e `undefined !== Hidden`.
        if !node.collapsed.get() || visibility != Some(TreeVisibility::Hidden) {
            let mut visible_child_index = 0;

            let children = node.children.borrow().clone();
            for child in &children {
                // Na raiz, `visibility` é `undefined`, que o `_filterNode` do filho trata como nem Visible nem Recurse.
                has_visible_descendants = self.do_update_node_after_filter_change(child, visibility.unwrap_or(TreeVisibility::Hidden), result, revealed && !node.collapsed.get()) || has_visible_descendants;

                if child.visible.get() {
                    child.visible_child_index.set(visible_child_index);
                    visible_child_index += 1;
                }
            }

            node.visible_children_count.set(visible_child_index as usize);
        } else {
            node.visible_children_count.set(0);
        }

        if !is_root {
            let v = visibility.expect("só a raiz não tem visibilidade");
            node.visible.set(if v == TreeVisibility::Recurse { has_visible_descendants } else { v == TreeVisibility::Visible });
            node.visibility.set(v);
        }

        if !node.visible.get() {
            node.render_node_count.set(0);

            if revealed {
                result.pop();
            }
        } else if !node.collapsed.get() {
            node.render_node_count.set(node.render_node_count.get() + result.len() - result_start_length);
        }

        self.on_did_change_render_node_count_raw.fire(node.clone());
        node.visible.get()
    }

    fn update_ancestors_render_node_count(&self, node: Option<&Rc<TreeNode<T, F>>>, diff: i64) {
        if diff == 0 {
            return;
        }

        let mut node = node.cloned();
        while let Some(n) = node {
            n.render_node_count.set((n.render_node_count.get() as i64 + diff) as usize);
            self.on_did_change_render_node_count_raw.fire(n.clone());
            node = n.parent();
        }
    }

    fn filter_node(&self, node: &Rc<TreeNode<T, F>>, parent_visibility: TreeVisibility) -> TreeVisibility {
        let result = match &self.filter {
            Some(filter) => filter.filter(&node.element, parent_visibility),
            None => TreeFilterResult::Visibility(TreeVisibility::Visible),
        };

        match result {
            TreeFilterResult::Bool(b) => {
                *node.filter_data.borrow_mut() = None;
                if b { TreeVisibility::Visible } else { TreeVisibility::Hidden }
            }
            TreeFilterResult::Data { visibility, data } => {
                *node.filter_data.borrow_mut() = Some(data);
                get_visible_state(visibility)
            }
            TreeFilterResult::Visibility(v) => {
                *node.filter_data.borrow_mut() = None;
                v
            }
        }
    }

    fn has_tree_node(&self, location: &[usize], node: &Rc<TreeNode<T, F>>) -> bool {
        let Some((index, rest)) = location.split_first() else {
            return true;
        };
        let children = node.children.borrow();
        // `index > node.children.length` (e não `>=`): o original deixa `children[length]` passar e o próximo
        // nível então acessa `undefined`; aqui isso é "não tem".
        if *index > children.len() {
            return false;
        }
        match children.get(*index) {
            Some(child) => self.has_tree_node(rest, child),
            None => false,
        }
    }

    fn get_tree_node(&self, location: &[usize]) -> Rc<TreeNode<T, F>> {
        let mut node = self.root.clone();
        for index in location {
            let next = {
                let children = node.children.borrow();
                match children.get(*index) {
                    Some(c) => c.clone(),
                    None => throw(&tree_error(&self.user, "Invalid tree location").to_string()),
                }
            };
            node = next;
        }
        node
    }

    fn get_tree_node_with_list_index(&self, location: &[usize]) -> (Rc<TreeNode<T, F>>, i64, bool, bool) {
        if location.is_empty() {
            return (self.root.clone(), -1, true, false);
        }

        let ListIndexInfo { parent_node, list_index, revealed, visible } = self.get_parent_node_with_list_index(location);
        let index = location[location.len() - 1];

        let node = match parent_node.children.borrow().get(index) {
            Some(n) => n.clone(),
            None => throw(&tree_error(&self.user, "Invalid tree location").to_string()),
        };

        let node_visible = node.visible.get();
        (node, list_index, revealed, visible && node_visible)
    }

    fn get_parent_node_with_list_index(&self, location: &[usize]) -> ListIndexInfo<T, F> {
        let mut node = self.root.clone();
        let mut list_index: i64 = 0;
        let mut revealed = true;
        let mut visible = true;
        let mut rest = location;

        loop {
            let (index, tail) = rest.split_first().expect("a localização não é vazia");

            if *index > node.children.borrow().len() {
                throw(&tree_error(&self.user, "Invalid tree location").to_string());
            }

            for i in 0..*index {
                list_index += node.children.borrow()[i].render_node_count.get() as i64;
            }

            revealed = revealed && !node.collapsed.get();
            visible = visible && node.visible.get();

            if tail.is_empty() {
                return ListIndexInfo { parent_node: node, list_index, revealed, visible };
            }

            let next = match node.children.borrow().get(*index) {
                Some(c) => c.clone(),
                None => throw(&tree_error(&self.user, "Invalid tree location").to_string()),
            };
            node = next;
            list_index += 1;
            rest = tail;
        }
    }

    pub fn get_node(&self, location: &[usize]) -> Rc<TreeNode<T, F>> {
        self.get_tree_node(location)
    }

    pub fn get_node_location(&self, node: &Rc<TreeNode<T, F>>) -> Vec<usize> {
        let mut location = Vec::new();
        let mut current = node.clone();

        while let Some(parent) = current.parent() {
            let index = parent.children.borrow().iter().position(|c| Rc::ptr_eq(c, &current)).expect("o nó está nos filhos do pai");
            location.push(index);
            current = parent;
        }

        location.reverse();
        location
    }

    pub fn get_parent_node_location(&self, location: &[usize]) -> Option<Vec<usize>> {
        match location.len() {
            0 => None,
            1 => Some(Vec::new()),
            _ => Some(location[..location.len() - 1].to_vec()),
        }
    }

    pub fn get_first_element_child(&self, location: &[usize]) -> Option<T> {
        self.get_tree_node(location).children.borrow().first().map(|c| c.element.clone())
    }

    pub fn get_last_element_ancestor(&self, location: &[usize]) -> Option<T> {
        let node = self.get_tree_node(location);

        if node.children.borrow().is_empty() {
            return None;
        }

        Some(Self::do_get_last_element_ancestor(&node))
    }

    fn do_get_last_element_ancestor(node: &Rc<TreeNode<T, F>>) -> T {
        match node.children.borrow().last() {
            None => node.element.clone(),
            Some(last) => Self::do_get_last_element_ancestor(last),
        }
    }
}

impl<T: Clone + 'static, F: Clone + 'static> IDisposable for IndexTreeModel<T, F> {
    fn dispose(&self) {
        self.refilter_delayer.cancel();
        self.on_did_splice_model.dispose();
        self.on_did_splice_rendered_nodes.dispose();
        self.on_did_change_collapse_state_raw.dispose();
        self.on_did_change_render_node_count_raw.dispose();
    }

    fn constructor_name(&self) -> &'static str {
        "IndexTreeModel"
    }
}
