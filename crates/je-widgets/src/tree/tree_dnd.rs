//! Porte de `TreeNodeListDragAndDrop` e das conversões de dados de arrasto de
//! `src/vs/base/browser/ui/tree/abstractTree.ts`: o `ITreeDragAndDrop<T>` do usuário visto como o
//! `IListDragAndDrop<ITreeNode<T>>` da lista, com a expansão automática do nó sob o ponteiro e o `bubble` da
//! reação (`Up` pergunta ao pai, `Down` marca o nó e os descendentes).
//!
//! Do TypeScript para o Rust:
//! - `modelProvider()` é o próprio modelo (`Rc<M>`): a árvore não troca de modelo depois do construtor.
//! - `disposableTimeout(..., 500, this.disposables)` é um `TimeoutTimer`; `autoExpandDisposable.dispose()` é
//!   `cancel`.
//! - O `TreeElementsDragAndDropData` (que lê o `context` do dado original) é o
//!   [`ElementsDragAndDropData::derive`], que compartilha o `context`.
//! - O `parentRef && model.getListIndex(parentRef)` é `None` quando a referência do pai é a raiz
//!   ([`ITreeModel::ref_is_falsy`]); a raiz como pai é o `getNode(parentRef)` do modelo.

use std::cell::RefCell;
use std::rc::Rc;

use je_base::async_::TimeoutTimer;

use crate::dnd::{DragEvent, IDragAndDropData};
use crate::list::list_dnd::{ElementsDragAndDropData, IListDragAndDrop, IListDragOverReaction, ListDragOverResult, ListViewTargetSector};
use crate::tree::tree_types::{ITreeDragAndDrop, ITreeModel, TreeDragOverBubble, TreeDragOverResult, TreeNodeApi};

/// `asTreeDragAndDropData`: os dados com os elementos dos nós, e o mesmo `context`.
fn as_tree_drag_and_drop_data<M: ITreeModel>(data: &Rc<dyn IDragAndDropData>) -> Rc<dyn IDragAndDropData> {
    match data.as_any().downcast_ref::<ElementsDragAndDropData<Rc<M::Node>>>() {
        Some(elements) => Rc::new(elements.derive(elements.elements.iter().map(|node| node.element()).collect::<Vec<M::Element>>())),
        None => data.clone(),
    }
}

/// O `ITreeDragAndDrop` do usuário como `IListDragAndDrop` da lista, com `TreeDragOverResult` visto como `ListDragOverResult`
/// (a lista só lê `accept`, `effect` e `feedback`).
fn as_list_result(result: TreeDragOverResult) -> ListDragOverResult {
    match result {
        TreeDragOverResult::Bool(accept) => ListDragOverResult::Bool(accept),
        TreeDragOverResult::Reaction(reaction) => ListDragOverResult::Reaction(reaction.reaction),
    }
}

/// `TreeNodeListDragAndDrop<T, TFilterData, TRef>`.
pub(crate) struct TreeNodeListDragAndDrop<M: ITreeModel> {
    model: Rc<M>,
    dnd: Rc<dyn ITreeDragAndDrop<M::Element>>,
    auto_expand_node: Rc<RefCell<Option<Rc<M::Node>>>>,
    auto_expand_timer: Rc<TimeoutTimer>,
}

impl<M: ITreeModel> TreeNodeListDragAndDrop<M> {
    pub(crate) fn new(model: Rc<M>, dnd: Rc<dyn ITreeDragAndDrop<M::Element>>) -> Self {
        Self { model, dnd, auto_expand_node: Rc::default(), auto_expand_timer: TimeoutTimer::new() }
    }

    /// `autoExpandNode !== targetNode`.
    fn is_auto_expand_node(&self, target_node: Option<&Rc<M::Node>>) -> bool {
        match (&*self.auto_expand_node.borrow(), target_node) {
            (None, None) => true,
            (Some(current), Some(target)) => Rc::ptr_eq(current, target),
            _ => false,
        }
    }

    /// `onDragOver(data, targetNode, targetIndex, targetSector, originalEvent, raw = true)`.
    fn drag_over(
        &self,
        data: &Rc<dyn IDragAndDropData>,
        target_node: Option<&Rc<M::Node>>,
        target_index: Option<usize>,
        target_sector: Option<ListViewTargetSector>,
        original_event: &DragEvent,
        raw: bool,
    ) -> TreeDragOverResult {
        let target_element = target_node.map(|node| node.element());
        let result = self.dnd.on_drag_over(&as_tree_drag_and_drop_data::<M>(data), target_element.as_ref(), target_index, target_sector, original_event);
        let did_change_auto_expand_node = !self.is_auto_expand_node(target_node);

        if did_change_auto_expand_node {
            self.auto_expand_timer.cancel();
            *self.auto_expand_node.borrow_mut() = target_node.cloned();
        }

        let Some(target_node) = target_node else {
            return result;
        };

        if did_change_auto_expand_node && matches!(&result, TreeDragOverResult::Reaction(reaction) if reaction.auto_expand == Some(true)) {
            let (model, node, auto_expand_node) = (self.model.clone(), target_node.clone(), self.auto_expand_node.clone());
            self.auto_expand_timer.cancel_and_set(
                move || {
                    let reference = model.get_node_location(&node);

                    if model.is_collapsed(&reference) {
                        model.set_collapsed(&reference, Some(false), None);
                    }

                    *auto_expand_node.borrow_mut() = None;
                },
                500,
            );
        }

        let reaction = match &result {
            TreeDragOverResult::Reaction(reaction) if reaction.reaction.accept && reaction.bubble.is_some() && reaction.reaction.feedback.is_none() => reaction.clone(),
            _ => {
                if !raw {
                    let (accept, effect) = match &result {
                        TreeDragOverResult::Bool(accept) => (*accept, None),
                        TreeDragOverResult::Reaction(reaction) => (reaction.reaction.accept, reaction.reaction.effect),
                    };
                    return TreeDragOverResult::Reaction(crate::tree::tree_types::ITreeDragOverReaction {
                        reaction: IListDragOverReaction { accept, effect, feedback: Some(vec![target_index.map_or(-1, |index| index as i64)]) },
                        ..Default::default()
                    });
                }

                return result;
            }
        };

        if reaction.bubble == Some(TreeDragOverBubble::Up) {
            let reference = self.model.get_node_location(target_node);
            let parent_ref = self.model.get_parent_node_location(&reference).unwrap_or_else(|| self.model.root_ref());
            let parent_node = self.model.get_node(&parent_ref);
            let parent_index = if self.model.ref_is_falsy(&parent_ref) { None } else { usize::try_from(self.model.get_list_index(&parent_ref)).ok() };

            return self.drag_over(data, Some(&parent_node), parent_index, target_sector, original_event, false);
        }

        let reference = self.model.get_node_location(target_node);
        let start = self.model.get_list_index(&reference);
        let length = self.model.get_list_render_count(&reference) as i64;

        let mut reaction = reaction;
        reaction.reaction.feedback = Some((start..start + length).collect());
        TreeDragOverResult::Reaction(reaction)
    }
}

impl<M: ITreeModel> IListDragAndDrop<Rc<M::Node>> for TreeNodeListDragAndDrop<M> {
    fn get_drag_uri(&self, node: &Rc<M::Node>) -> Option<String> {
        self.dnd.get_drag_uri(&node.element())
    }

    fn get_drag_label(&self, nodes: &[Rc<M::Node>], original_event: &DragEvent) -> Option<String> {
        let elements: Vec<M::Element> = nodes.iter().map(|node| node.element()).collect();
        self.dnd.get_drag_label(&elements, original_event)
    }

    fn on_drag_start(&self, data: &Rc<dyn IDragAndDropData>, original_event: &DragEvent) {
        self.dnd.on_drag_start(&as_tree_drag_and_drop_data::<M>(data), original_event);
    }

    fn on_drag_over(
        &self,
        data: &Rc<dyn IDragAndDropData>,
        target_node: Option<&Rc<M::Node>>,
        target_index: Option<usize>,
        target_sector: Option<ListViewTargetSector>,
        original_event: &DragEvent,
    ) -> ListDragOverResult {
        as_list_result(self.drag_over(data, target_node, target_index, target_sector, original_event, true))
    }

    fn drop(&self, data: &Rc<dyn IDragAndDropData>, target_node: Option<&Rc<M::Node>>, target_index: Option<usize>, target_sector: Option<ListViewTargetSector>, original_event: &DragEvent) {
        self.auto_expand_timer.cancel();
        *self.auto_expand_node.borrow_mut() = None;

        let target_element = target_node.map(|node| node.element());
        (*self.dnd).drop(&as_tree_drag_and_drop_data::<M>(data), target_element.as_ref(), target_index, target_sector, original_event);
    }

    fn on_drag_end(&self, original_event: &DragEvent) {
        self.dnd.on_drag_end(original_event);
    }

    fn dispose(&self) {
        self.auto_expand_timer.cancel();
        self.dnd.dispose();
    }
}
