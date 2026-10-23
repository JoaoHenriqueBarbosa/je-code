//! Porte de `src/vs/editor/common/model/tokens/treeSitter/tokenStore.ts`: o armazenamento de tokens em
//! árvore (uma árvore 2-3 de folhas com comprimento e token) usado pela tokenização por árvore sintática.
//!
//! Decisões de tradução:
//! - Os nós são compartilhados por referência no original (folhas reaproveitadas entre a árvore antiga e a
//!   nova, `markForRefresh` altera a qualidade da folha no lugar), então [`Node`] é um enum de `Rc`:
//!   [`ListNode`] com mutabilidade interior e [`LeafNode`] com a qualidade em `Cell`.
//! - `ListNode.parent` é um `Weak<ListNode>` (o pai é dono dos filhos), e cada lista guarda um `Weak` de si
//!   mesma (`Rc::new_cyclic`) para poder se registrar como pai ao receber um filho.
//! - `isLeaf` (`token !== undefined`) vira a variante do enum.
//! - `_root` depois do `dispose` é `undefined` no original; aqui é `None`, e qualquer uso depois do
//!   `dispose` entra em pânico (o original lançaria `TypeError`).
//! - Os offsets e comprimentos são `usize` (unidades UTF-16 do texto) e o token é `u32`.
//! - `ITextModel` só é usado por `getValueLength()`; o modelo é o [`TextModel`] compartilhado.

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use je_base::lifecycle::IDisposable;

use crate::text_model::TextModel;

/// `TokenQuality`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum TokenQuality {
    None = 0,
    ViewportGuess = 1,
    EditGuess = 2,
    Accurate = 3,
}

/// `TokenUpdate`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TokenUpdate {
    pub start_offset_inclusive: usize,
    pub length: usize,
    pub token: u32,
}

/// O retorno de `getNeedsRefresh()`: `{ startOffset, endOffset }`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NeedsRefreshRange {
    pub start_offset: usize,
    pub end_offset: usize,
}

/// `LeafNode` (exportado para os testes). `height` é sempre 0.
pub struct LeafNode {
    pub length: usize,
    pub token: u32,
    pub token_quality: Cell<TokenQuality>,
}

impl LeafNode {
    pub fn new(length: usize, token: u32, token_quality: TokenQuality) -> Rc<Self> {
        Rc::new(Self { length, token, token_quality: Cell::new(token_quality) })
    }

    /// `height: 0`.
    pub const HEIGHT: usize = 0;
}

/// `type Node = ListNode | LeafNode`.
#[derive(Clone)]
pub enum Node {
    List(Rc<ListNode>),
    Leaf(Rc<LeafNode>),
}

impl Node {
    pub fn length(&self) -> usize {
        match self {
            Node::List(list) => list.length(),
            Node::Leaf(leaf) => leaf.length,
        }
    }

    pub fn height(&self) -> usize {
        match self {
            Node::List(list) => list.height,
            Node::Leaf(_) => LeafNode::HEIGHT,
        }
    }

    /// `isLeaf(node)`.
    pub fn is_leaf(&self) -> bool {
        matches!(self, Node::Leaf(_))
    }

    /// O `node as ListNode` dos testes.
    pub fn as_list(&self) -> &Rc<ListNode> {
        match self {
            Node::List(list) => list,
            Node::Leaf(_) => panic!("esperava um ListNode, veio um LeafNode"),
        }
    }

    /// O `node as LeafNode` dos testes.
    pub fn as_leaf(&self) -> &Rc<LeafNode> {
        match self {
            Node::Leaf(leaf) => leaf,
            Node::List(_) => panic!("esperava um LeafNode, veio um ListNode"),
        }
    }

    /// `node.parent = undefined` (só as listas têm pai).
    fn clear_parent(&self) {
        if let Node::List(list) = self {
            *list.parent.borrow_mut() = Weak::new();
        }
    }
}

/// `ListNode` (exportado para os testes).
pub struct ListNode {
    me: Weak<ListNode>,
    parent: RefCell<Weak<ListNode>>,
    children: RefCell<Vec<Node>>,
    length: Cell<usize>,
    pub height: usize,
}

impl ListNode {
    pub fn new(height: usize) -> Rc<Self> {
        Rc::new_cyclic(|me| Self {
            me: me.clone(),
            parent: RefCell::new(Weak::new()),
            children: RefCell::new(Vec::new()),
            length: Cell::new(0),
            height,
        })
    }

    pub fn create(node1: Node, node2: Node) -> Rc<Self> {
        let list = ListNode::new(node1.height() + 1);
        list.append_child(node1);
        list.append_child(node2);
        list
    }

    /// `children` (cópia rasa: os nós são `Rc`).
    pub fn children(&self) -> Vec<Node> {
        self.children.borrow().clone()
    }

    pub fn children_len(&self) -> usize {
        self.children.borrow().len()
    }

    pub fn length(&self) -> usize {
        self.length.get()
    }

    pub fn can_append_child(&self) -> bool {
        self.children.borrow().len() < 3
    }

    pub fn append_child(&self, node: Node) {
        if !self.can_append_child() {
            panic!("Cannot insert more than 3 children in a ListNode");
        }
        let node_length = node.length();
        self.adopt(&node);
        self.children.borrow_mut().push(node);
        self.add_length(node_length as isize);
    }

    pub fn unappend_child(&self) -> Node {
        let child = self.children.borrow_mut().pop().expect("ListNode sem filhos");
        self.add_length(-(child.length() as isize));
        child
    }

    pub fn prepend_child(&self, node: Node) {
        if self.children.borrow().len() >= 3 {
            panic!("Cannot prepend more than 3 children in a ListNode");
        }
        let node_length = node.length();
        self.adopt(&node);
        self.children.borrow_mut().insert(0, node);
        self.add_length(node_length as isize);
    }

    pub fn unprepend_child(&self) -> Node {
        let child = {
            let mut children = self.children.borrow_mut();
            assert!(!children.is_empty(), "ListNode sem filhos");
            children.remove(0)
        };
        self.add_length(-(child.length() as isize));
        child
    }

    pub fn first_child(&self) -> Node {
        self.children.borrow().first().expect("ListNode sem filhos").clone()
    }

    pub fn last_child(&self) -> Node {
        self.children.borrow().last().expect("ListNode sem filhos").clone()
    }

    /// `if (!isLeaf(node)) node.parent = this`.
    fn adopt(&self, node: &Node) {
        if let Node::List(list) = node {
            *list.parent.borrow_mut() = self.me.clone();
        }
    }

    /// Soma `delta` ao próprio comprimento e ao de todos os ancestrais (`_length += ...` e
    /// `_updateParentLength`).
    fn add_length(&self, delta: isize) {
        self.length.set(self.length.get().wrapping_add_signed(delta));
        let mut updated_parent = self.parent.borrow().upgrade();
        while let Some(parent) = updated_parent {
            parent.length.set(parent.length.get().wrapping_add_signed(delta));
            updated_parent = parent.parent.borrow().upgrade();
        }
    }
}

impl IDisposable for ListNode {
    fn dispose(&self) {
        self.children.borrow_mut().clear();
    }
}

// Heavily inspired by concat23Trees.ts do bracketPairsTree
fn append(node: &Node, node_to_append: Node) -> Node {
    let mut cur_node = node.clone();
    let mut parents: Vec<Rc<ListNode>> = Vec::new();
    while node_to_append.height() != cur_node.height() {
        let list = match &cur_node {
            Node::Leaf(_) => panic!("unexpected"),
            Node::List(list) => list.clone(),
        };
        cur_node = list.last_child();
        parents.push(list);
    }
    let mut node_to_append_of_correct_height = Some(node_to_append);
    for parent in parents.iter().rev() {
        if let Some(pending) = node_to_append_of_correct_height.take() {
            // Can we take the element?
            if parent.children_len() >= 3 {
                // we need to split to maintain (2,3)-tree property.
                // Send the third element + the new element to the parent.
                node_to_append_of_correct_height = Some(Node::List(ListNode::create(parent.unappend_child(), pending)));
            } else {
                parent.append_child(pending);
            }
        }
    }
    match node_to_append_of_correct_height {
        Some(pending) => {
            let new_list = ListNode::new(pending.height() + 1);
            new_list.append_child(node.clone());
            new_list.append_child(pending);
            Node::List(new_list)
        }
        None => node.clone(),
    }
}

fn prepend(list: &Node, node_to_append: Node) -> Node {
    let mut cur_node = list.clone();
    let mut parents: Vec<Rc<ListNode>> = Vec::new();
    while node_to_append.height() != cur_node.height() {
        let parent = match &cur_node {
            Node::Leaf(_) => panic!("unexpected"),
            Node::List(parent) => parent.clone(),
        };
        cur_node = parent.first_child();
        parents.push(parent);
    }
    let mut node_to_prepend_of_correct_height = Some(node_to_append);
    for parent in parents.iter().rev() {
        if let Some(pending) = node_to_prepend_of_correct_height.take() {
            // Can we take the element?
            if parent.children_len() >= 3 {
                // we need to split to maintain (2,3)-tree property.
                // Send the third element + the new element to the parent.
                node_to_prepend_of_correct_height = Some(Node::List(ListNode::create(pending, parent.unprepend_child())));
            } else {
                parent.prepend_child(pending);
            }
        }
    }
    match node_to_prepend_of_correct_height {
        Some(pending) => Node::List(ListNode::create(pending, list.clone())),
        None => list.clone(),
    }
}

fn concat(node1: Node, node2: Node) -> Node {
    if node1.height() == node2.height() {
        Node::List(ListNode::create(node1, node2))
    } else if node1.height() > node2.height() {
        // node1 is the tree we want to insert into
        append(&node1, node2)
    } else {
        prepend(&node2, node1)
    }
}

/// `TokenStore`.
pub struct TokenStore {
    root: RefCell<Option<Node>>,
    text_model: Rc<TextModel>,
}

impl TokenStore {
    pub fn new(text_model: Rc<TextModel>) -> Self {
        let store = Self { root: RefCell::new(None), text_model };
        *store.root.borrow_mut() = Some(store.create_empty_root());
        store
    }

    /// `root` (o `Rc` do nó, barato de clonar).
    pub fn root(&self) -> Node {
        self.root.borrow().clone().expect("TokenStore descartado")
    }

    fn set_root(&self, root: Node) {
        *self.root.borrow_mut() = Some(root);
    }

    /// `this._textModel.getValueLength()`.
    fn value_length(&self) -> usize {
        usize::try_from(self.text_model.get_value_length()).expect("comprimento do modelo fora de usize")
    }

    fn create_empty_root(&self) -> Node {
        Node::Leaf(LeafNode::new(self.value_length(), 0, TokenQuality::None))
    }

    /// `buildStore(tokens, tokenQuality)`: todos os tokens do documento em sequência.
    pub fn build_store(&self, tokens: &[TokenUpdate], token_quality: TokenQuality) {
        self.set_root(self.create_from_updates(tokens, token_quality));
    }

    fn create_from_updates(&self, tokens: &[TokenUpdate], token_quality: TokenQuality) -> Node {
        let Some((first, rest)) = tokens.split_first() else {
            return self.create_empty_root();
        };
        let mut new_root = Node::Leaf(LeafNode::new(first.length, first.token, token_quality));
        for token in rest {
            new_root = append(&new_root, Node::Leaf(LeafNode::new(token.length, token.token, token_quality)));
        }
        new_root
    }

    /// `update(length, tokens, tokenQuality)`: os tokens estão em sequência no documento.
    pub fn update(&self, length: usize, tokens: &[TokenUpdate], token_quality: TokenQuality) {
        let Some(first) = tokens.first() else {
            return;
        };
        self.replace(length, first.start_offset_inclusive, tokens, token_quality);
    }

    /// `delete(length, startOffset)`.
    pub fn delete(&self, length: usize, start_offset: usize) {
        self.replace(length, start_offset, &[], TokenQuality::EditGuess);
    }

    fn replace(&self, length: usize, update_offset_start: usize, tokens: &[TokenUpdate], token_quality: TokenQuality) {
        let first_unchanged_offset_after_update = update_offset_start + length;
        // Find the last unchanged node preceding the update
        let mut preceding_nodes: Vec<Node> = Vec::new();
        // Find the first unchanged node after the update
        let mut postceding_nodes: Vec<Node> = Vec::new();
        let mut stack: Vec<(Node, usize)> = vec![(self.root(), 0)];

        while let Some((node, current_offset)) = stack.pop() {
            let node_length = node.length();

            if current_offset < update_offset_start && current_offset + node_length <= update_offset_start {
                node.clear_parent();
                preceding_nodes.push(node);
                continue;
            } else if node.is_leaf() && current_offset < update_offset_start {
                // We have a partial preceding node
                let leaf = node.as_leaf();
                preceding_nodes.push(Node::Leaf(LeafNode::new(
                    update_offset_start - current_offset,
                    leaf.token,
                    leaf.token_quality.get(),
                )));
                // Node could also be postceeding, so don't continue
            }

            if update_offset_start <= current_offset && current_offset + node_length <= first_unchanged_offset_after_update {
                continue;
            }

            if current_offset >= first_unchanged_offset_after_update {
                node.clear_parent();
                postceding_nodes.push(node);
                continue;
            } else if node.is_leaf() && current_offset + node_length > first_unchanged_offset_after_update {
                // we have a partial postceeding node
                let leaf = node.as_leaf();
                postceding_nodes.push(Node::Leaf(LeafNode::new(
                    current_offset + node_length - first_unchanged_offset_after_update,
                    leaf.token,
                    leaf.token_quality.get(),
                )));
                continue;
            }

            if let Node::List(list) = &node {
                // Push children in reverse order to process them left-to-right when popping
                let mut child_offset = current_offset + node_length;
                for child in list.children().into_iter().rev() {
                    child_offset -= child.length();
                    stack.push((child, child_offset));
                }
            }
        }

        let mut all_nodes: Vec<Node> = preceding_nodes;
        if !tokens.is_empty() {
            all_nodes.push(self.create_from_updates(tokens, token_quality));
        }
        all_nodes.extend(postceding_nodes);

        let mut nodes = all_nodes.into_iter();
        let new_root = nodes.next().map(|first| nodes.fold(first, concat));
        self.set_root(new_root.unwrap_or_else(|| self.create_empty_root()));
    }

    /// `traverseInOrderInRange`: o visitante devolve `true` para sair cedo.
    fn traverse_in_order_in_range(
        &self,
        start_offset_inclusive: usize,
        end_offset_exclusive: usize,
        mut visitor: impl FnMut(&Node, usize) -> bool,
    ) {
        let mut stack: Vec<(Node, usize)> = vec![(self.root(), 0)];

        while let Some((node, offset)) = stack.pop() {
            let node_end = offset + node.length();

            // Skip nodes that are completely before or after the range
            if node_end <= start_offset_inclusive || offset >= end_offset_exclusive {
                continue;
            }

            if visitor(&node, offset) {
                return;
            }

            if let Node::List(list) = &node {
                // Push children in reverse order to process them left-to-right when popping
                let mut child_offset = offset + node.length();
                for child in list.children().into_iter().rev() {
                    child_offset -= child.length();
                    stack.push((child, child_offset));
                }
            }
        }
    }

    /// `getTokenAt(offset)`.
    pub fn get_token_at(&self, offset: usize) -> Option<TokenUpdate> {
        let mut result = None;
        self.traverse_in_order_in_range(offset, self.root().length(), |node, offset| {
            if let Node::Leaf(leaf) = node {
                result = Some(TokenUpdate { token: leaf.token, start_offset_inclusive: offset, length: leaf.length });
                return true;
            }
            false
        });
        result
    }

    /// `getTokensInRange(startOffsetInclusive, endOffsetExclusive)`.
    pub fn get_tokens_in_range(&self, start_offset_inclusive: usize, end_offset_exclusive: usize) -> Vec<TokenUpdate> {
        let mut result = Vec::new();
        self.traverse_in_order_in_range(start_offset_inclusive, end_offset_exclusive, |node, offset| {
            if let Node::Leaf(leaf) = node {
                let mut clipped_length = leaf.length;
                let mut clipped_offset = offset;
                if offset < start_offset_inclusive && offset + leaf.length > end_offset_exclusive {
                    clipped_offset = start_offset_inclusive;
                    clipped_length = end_offset_exclusive - start_offset_inclusive;
                } else if offset < start_offset_inclusive {
                    clipped_length -= start_offset_inclusive - offset;
                    clipped_offset = start_offset_inclusive;
                } else if offset + leaf.length > end_offset_exclusive {
                    clipped_length -= offset + leaf.length - end_offset_exclusive;
                }
                result.push(TokenUpdate { token: leaf.token, start_offset_inclusive: clipped_offset, length: clipped_length });
            }
            false
        });
        result
    }

    /// `markForRefresh(startOffsetInclusive, endOffsetExclusive)`.
    pub fn mark_for_refresh(&self, start_offset_inclusive: usize, end_offset_exclusive: usize) {
        self.traverse_in_order_in_range(start_offset_inclusive, end_offset_exclusive, |node, _| {
            if let Node::Leaf(leaf) = node {
                leaf.token_quality.set(TokenQuality::None);
            }
            false
        });
    }

    /// `rangeHasTokens(startOffsetInclusive, endOffsetExclusive, minimumTokenQuality)`.
    pub fn range_has_tokens(
        &self,
        start_offset_inclusive: usize,
        end_offset_exclusive: usize,
        minimum_token_quality: TokenQuality,
    ) -> bool {
        let mut has_any = true;
        self.traverse_in_order_in_range(start_offset_inclusive, end_offset_exclusive, |node, _| {
            if let Node::Leaf(leaf) = node {
                if leaf.token_quality.get() < minimum_token_quality {
                    has_any = false;
                }
            }
            false
        });
        has_any
    }

    /// `rangeNeedsRefresh(startOffsetInclusive, endOffsetExclusive)`.
    pub fn range_needs_refresh(&self, start_offset_inclusive: usize, end_offset_exclusive: usize) -> bool {
        let mut needs_refresh = false;
        self.traverse_in_order_in_range(start_offset_inclusive, end_offset_exclusive, |node, _| {
            if let Node::Leaf(leaf) = node {
                if leaf.token_quality.get() != TokenQuality::Accurate {
                    needs_refresh = true;
                }
            }
            false
        });
        needs_refresh
    }

    /// `getNeedsRefresh()`.
    pub fn get_needs_refresh(&self) -> Vec<NeedsRefreshRange> {
        let mut result: Vec<NeedsRefreshRange> = Vec::new();
        self.traverse_in_order_in_range(0, self.value_length(), |node, offset| {
            if let Node::Leaf(leaf) = node {
                if leaf.token_quality.get() != TokenQuality::Accurate {
                    match result.last_mut() {
                        Some(last) if last.end_offset == offset => last.end_offset += leaf.length,
                        _ => result.push(NeedsRefreshRange { start_offset: offset, end_offset: offset + leaf.length }),
                    }
                }
            }
            false
        });
        result
    }

    /// `deepCopy()`.
    pub fn deep_copy(&self) -> TokenStore {
        let new_store = TokenStore::new(self.text_model.clone());
        new_store.set_root(Self::copy_node_iterative(&self.root()));
        new_store
    }

    fn copy_node_iterative(root: &Node) -> Node {
        let new_root = Self::shallow_copy(root);
        let mut stack: Vec<(Node, Node)> = vec![(root.clone(), new_root.clone())];

        while let Some((old_node, cloned_node)) = stack.pop() {
            if let Node::List(old_list) = &old_node {
                for child in old_list.children() {
                    let child_copy = Self::shallow_copy(&child);
                    cloned_node.as_list().append_child(child_copy.clone());
                    stack.push((child, child_copy));
                }
            }
        }

        new_root
    }

    /// A folha copiada por inteiro, ou a lista vazia da mesma altura.
    fn shallow_copy(node: &Node) -> Node {
        match node {
            Node::Leaf(leaf) => Node::Leaf(LeafNode::new(leaf.length, leaf.token, leaf.token_quality.get())),
            Node::List(list) => Node::List(ListNode::new(list.height)),
        }
    }

    /// `printTree(root = this._root)`: a representação em texto da árvore, de forma iterativa.
    pub fn print_tree(&self, root: Option<Node>) -> String {
        let root = root.unwrap_or_else(|| self.root());
        let mut result = String::new();
        let mut stack: Vec<(Node, usize)> = vec![(root, 0)];

        while let Some((node, depth)) = stack.pop() {
            let indent = "  ".repeat(depth);

            match &node {
                Node::Leaf(leaf) => {
                    result.push_str(&format!(
                        "{indent}Leaf(length: {}, token: {}, refresh: {})\n",
                        leaf.length,
                        leaf.token,
                        leaf.token_quality.get() as u8
                    ));
                }
                Node::List(list) => {
                    result.push_str(&format!("{indent}List(length: {})\n", list.length()));
                    // Push children in reverse order so they get processed left-to-right
                    for child in list.children().into_iter().rev() {
                        stack.push((child, depth + 1));
                    }
                }
            }
        }

        result
    }
}

impl IDisposable for TokenStore {
    fn dispose(&self) {
        let Some(root) = self.root.borrow_mut().take() else {
            return;
        };
        let mut stack: Vec<(Node, bool)> = vec![(root, false)];
        while let Some((node, visited)) = stack.pop() {
            if let Node::List(list) = &node {
                if !visited {
                    let children = list.children();
                    stack.push((node.clone(), true));
                    for child in children.into_iter().rev() {
                        stack.push((child, false));
                    }
                } else {
                    list.dispose();
                    *list.parent.borrow_mut() = Weak::new();
                }
            }
            // a folha não precisa ser descartada
        }
    }
}
