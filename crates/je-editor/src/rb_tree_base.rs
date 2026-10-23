//! Porte de `src/vs/editor/common/model/pieceTreeTextBuffer/rbTreeBase.ts`: a árvore rubro-negra do piece
//! tree, com os metadados `size_left` (tamanho da subárvore esquerda) e `lf_left` (quebras de linha na
//! subárvore esquerda).
//!
//! O original liga nós por ponteiro e usa um nó sentinela global. Aqui os nós moram numa arena
//! (`Vec<TreeNode>`) e se ligam por índice `u32`; o índice 0 é o sentinela, preto, e, como no original,
//! o algoritmo escreve no pai do sentinela durante a remoção (`x.parent = ...` com `x` sentinela), então
//! o sentinela é um nó comum da arena e não é lido como constante. Um nó removido ganha `parent ==
//! DETACHED` (o `null` do original) e o slot só volta para a lista livre quando o dono da árvore chama
//! [`Tree::release_detached`], depois de validar o cache de busca, para que nenhum índice velho do cache
//! aponte para um nó reaproveitado.

use crate::piece_tree_base::Piece;

/// Índice de um nó na arena.
pub type NodeId = u32;

/// O nó sentinela (as folhas nulas da árvore rubro-negra).
pub const SENTINEL: NodeId = 0;

/// O `parent` de um nó removido (o `null` do original).
pub const DETACHED: NodeId = NodeId::MAX;

/// `NodeColor`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NodeColor {
    Black = 0,
    Red = 1,
}

/// `TreeNode`.
#[derive(Clone, Copy, Debug)]
pub struct TreeNode {
    pub parent: NodeId,
    pub left: NodeId,
    pub right: NodeId,
    pub color: NodeColor,
    pub piece: Piece,
    /// Tamanho da subárvore esquerda (não da árvore em ordem).
    pub size_left: usize,
    /// Quebras de linha na subárvore esquerda (não da árvore em ordem).
    pub lf_left: usize,
}

impl TreeNode {
    fn new(piece: Piece, color: NodeColor) -> Self {
        TreeNode {
            parent: SENTINEL,
            left: SENTINEL,
            right: SENTINEL,
            color,
            piece,
            size_left: 0,
            lf_left: 0,
        }
    }
}

fn add_signed(value: usize, delta: isize) -> usize {
    (value as isize + delta) as usize
}

/// A árvore: a arena de nós e a raiz.
#[derive(Debug)]
pub struct Tree {
    nodes: Vec<TreeNode>,
    pub root: NodeId,
    free: Vec<NodeId>,
    pending_free: Vec<NodeId>,
}

impl Default for Tree {
    fn default() -> Self {
        Self::new()
    }
}

impl Tree {
    pub fn new() -> Self {
        Tree {
            nodes: vec![TreeNode::new(Piece::default(), NodeColor::Black)],
            root: SENTINEL,
            free: Vec::new(),
            pending_free: Vec::new(),
        }
    }

    /// Esvazia a árvore (o `this.root = SENTINEL` do `create`), descartando os nós.
    pub fn clear(&mut self) {
        *self = Tree::new();
    }

    #[inline]
    pub fn node(&self, id: NodeId) -> &TreeNode {
        &self.nodes[id as usize]
    }

    #[inline]
    pub fn node_mut(&mut self, id: NodeId) -> &mut TreeNode {
        &mut self.nodes[id as usize]
    }

    /// Cria um nó vermelho solto (folhas e pai no sentinela).
    pub fn alloc(&mut self, piece: Piece) -> NodeId {
        let node = TreeNode::new(piece, NodeColor::Red);
        match self.free.pop() {
            Some(id) => {
                self.nodes[id as usize] = node;
                id
            }
            None => {
                self.nodes.push(node);
                (self.nodes.len() - 1) as NodeId
            }
        }
    }

    /// `detach()`: o nó sai da árvore; o slot fica reservado até [`Tree::release_detached`].
    fn detach(&mut self, id: NodeId) {
        let node = self.node_mut(id);
        node.parent = DETACHED;
        node.left = DETACHED;
        node.right = DETACHED;
        self.pending_free.push(id);
    }

    /// Devolve à lista livre os slots dos nós removidos.
    pub fn release_detached(&mut self) {
        self.free.append(&mut self.pending_free);
    }

    /// Quantos nós vivos a árvore tem (sem o sentinela), para os testes.
    pub fn len(&self) -> usize {
        self.nodes.len() - 1 - self.free.len() - self.pending_free.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// `TreeNode.next()`: o próximo nó em ordem, ou o sentinela.
    pub fn next(&self, id: NodeId) -> NodeId {
        if self.node(id).right != SENTINEL {
            return self.leftest(self.node(id).right);
        }

        let mut node = id;
        while self.node(node).parent != SENTINEL {
            if self.node(self.node(node).parent).left == node {
                break;
            }
            node = self.node(node).parent;
        }

        if self.node(node).parent == SENTINEL {
            SENTINEL
        } else {
            self.node(node).parent
        }
    }

    /// `TreeNode.prev()`: o nó anterior em ordem, ou o sentinela.
    pub fn prev(&self, id: NodeId) -> NodeId {
        if self.node(id).left != SENTINEL {
            return self.righttest(self.node(id).left);
        }

        let mut node = id;
        while self.node(node).parent != SENTINEL {
            if self.node(self.node(node).parent).right == node {
                break;
            }
            node = self.node(node).parent;
        }

        if self.node(node).parent == SENTINEL {
            SENTINEL
        } else {
            self.node(node).parent
        }
    }

    pub fn leftest(&self, mut node: NodeId) -> NodeId {
        while self.node(node).left != SENTINEL {
            node = self.node(node).left;
        }
        node
    }

    pub fn righttest(&self, mut node: NodeId) -> NodeId {
        while self.node(node).right != SENTINEL {
            node = self.node(node).right;
        }
        node
    }

    fn calculate_size(&self, mut node: NodeId) -> usize {
        let mut size = 0;
        while node != SENTINEL {
            let n = self.node(node);
            size += n.size_left + n.piece.length;
            node = n.right;
        }
        size
    }

    fn calculate_lf(&self, mut node: NodeId) -> usize {
        let mut lf = 0;
        while node != SENTINEL {
            let n = self.node(node);
            lf += n.lf_left + n.piece.line_feed_cnt;
            node = n.right;
        }
        lf
    }

    fn reset_sentinel(&mut self) {
        self.node_mut(SENTINEL).parent = SENTINEL;
    }

    pub fn left_rotate(&mut self, x: NodeId) {
        let y = self.node(x).right;

        // corrige size_left
        let (x_size_left, x_lf_left) = (self.node(x).size_left, self.node(x).lf_left);
        let (x_len, x_lf) = (self.node(x).piece.length, self.node(x).piece.line_feed_cnt);
        self.node_mut(y).size_left += x_size_left + x_len;
        self.node_mut(y).lf_left += x_lf_left + x_lf;
        let y_left = self.node(y).left;
        self.node_mut(x).right = y_left;

        if y_left != SENTINEL {
            self.node_mut(y_left).parent = x;
        }
        let x_parent = self.node(x).parent;
        self.node_mut(y).parent = x_parent;
        if x_parent == SENTINEL {
            self.root = y;
        } else if self.node(x_parent).left == x {
            self.node_mut(x_parent).left = y;
        } else {
            self.node_mut(x_parent).right = y;
        }
        self.node_mut(y).left = x;
        self.node_mut(x).parent = y;
    }

    pub fn right_rotate(&mut self, y: NodeId) {
        let x = self.node(y).left;
        let x_right = self.node(x).right;
        self.node_mut(y).left = x_right;
        if x_right != SENTINEL {
            self.node_mut(x_right).parent = y;
        }
        let y_parent = self.node(y).parent;
        self.node_mut(x).parent = y_parent;

        // corrige size_left
        let (x_size_left, x_lf_left) = (self.node(x).size_left, self.node(x).lf_left);
        let (x_len, x_lf) = (self.node(x).piece.length, self.node(x).piece.line_feed_cnt);
        self.node_mut(y).size_left -= x_size_left + x_len;
        self.node_mut(y).lf_left -= x_lf_left + x_lf;

        if y_parent == SENTINEL {
            self.root = x;
        } else if y == self.node(y_parent).right {
            self.node_mut(y_parent).right = x;
        } else {
            self.node_mut(y_parent).left = x;
        }

        self.node_mut(x).right = y;
        self.node_mut(y).parent = x;
    }

    pub fn rb_delete(&mut self, z: NodeId) {
        let mut x: NodeId;
        let y: NodeId;

        if self.node(z).left == SENTINEL {
            y = z;
            x = self.node(y).right;
        } else if self.node(z).right == SENTINEL {
            y = z;
            x = self.node(y).left;
        } else {
            y = self.leftest(self.node(z).right);
            x = self.node(y).right;
        }

        if y == self.root {
            self.root = x;

            // se x é nulo, estamos removendo o único nó
            self.node_mut(x).color = NodeColor::Black;
            self.detach(z);
            self.reset_sentinel();
            let root = self.root;
            self.node_mut(root).parent = SENTINEL;

            return;
        }

        let y_was_red = self.node(y).color == NodeColor::Red;

        let y_parent = self.node(y).parent;
        if y == self.node(y_parent).left {
            self.node_mut(y_parent).left = x;
        } else {
            self.node_mut(y_parent).right = x;
        }

        if y == z {
            self.node_mut(x).parent = y_parent;
            self.recompute_tree_metadata(x);
        } else {
            if y_parent == z {
                self.node_mut(x).parent = y;
            } else {
                self.node_mut(x).parent = y_parent;
            }

            // como mudamos a hierarquia de x, atualiza antes o size_left da subárvore
            self.recompute_tree_metadata(x);

            let z_node = *self.node(z);
            {
                let yn = self.node_mut(y);
                yn.left = z_node.left;
                yn.right = z_node.right;
                yn.parent = z_node.parent;
                yn.color = z_node.color;
            }

            if z == self.root {
                self.root = y;
            } else {
                let z_parent = z_node.parent;
                if z == self.node(z_parent).left {
                    self.node_mut(z_parent).left = y;
                } else {
                    self.node_mut(z_parent).right = y;
                }
            }

            let y_left = self.node(y).left;
            if y_left != SENTINEL {
                self.node_mut(y_left).parent = y;
            }
            let y_right = self.node(y).right;
            if y_right != SENTINEL {
                self.node_mut(y_right).parent = y;
            }
            // atualiza metadados: trocamos z por y, então nesta subárvore a mudança de tamanho é o de z
            self.node_mut(y).size_left = z_node.size_left;
            self.node_mut(y).lf_left = z_node.lf_left;
            self.recompute_tree_metadata(y);
        }

        self.detach(z);

        let x_parent = self.node(x).parent;
        if self.node(x_parent).left == x {
            let new_size_left = self.calculate_size(x);
            let new_lf_left = self.calculate_lf(x);
            if new_size_left != self.node(x_parent).size_left || new_lf_left != self.node(x_parent).lf_left {
                let delta = new_size_left as isize - self.node(x_parent).size_left as isize;
                let lf_delta = new_lf_left as isize - self.node(x_parent).lf_left as isize;
                self.node_mut(x_parent).size_left = new_size_left;
                self.node_mut(x_parent).lf_left = new_lf_left;
                self.update_tree_metadata(x_parent, delta, lf_delta);
            }
        }

        let x_parent = self.node(x).parent;
        self.recompute_tree_metadata(x_parent);

        if y_was_red {
            self.reset_sentinel();
            return;
        }

        // RB-DELETE-FIXUP
        let mut w: NodeId;
        while x != self.root && self.node(x).color == NodeColor::Black {
            let xp = self.node(x).parent;
            if x == self.node(xp).left {
                w = self.node(xp).right;

                if self.node(w).color == NodeColor::Red {
                    self.node_mut(w).color = NodeColor::Black;
                    self.node_mut(xp).color = NodeColor::Red;
                    self.left_rotate(xp);
                    w = self.node(self.node(x).parent).right;
                }

                let (wl, wr) = (self.node(w).left, self.node(w).right);
                if self.node(wl).color == NodeColor::Black && self.node(wr).color == NodeColor::Black {
                    self.node_mut(w).color = NodeColor::Red;
                    x = self.node(x).parent;
                } else {
                    if self.node(wr).color == NodeColor::Black {
                        self.node_mut(wl).color = NodeColor::Black;
                        self.node_mut(w).color = NodeColor::Red;
                        self.right_rotate(w);
                        w = self.node(self.node(x).parent).right;
                    }

                    let xp = self.node(x).parent;
                    self.node_mut(w).color = self.node(xp).color;
                    self.node_mut(xp).color = NodeColor::Black;
                    let wr = self.node(w).right;
                    self.node_mut(wr).color = NodeColor::Black;
                    self.left_rotate(xp);
                    x = self.root;
                }
            } else {
                w = self.node(xp).left;

                if self.node(w).color == NodeColor::Red {
                    self.node_mut(w).color = NodeColor::Black;
                    self.node_mut(xp).color = NodeColor::Red;
                    self.right_rotate(xp);
                    w = self.node(self.node(x).parent).left;
                }

                let (wl, wr) = (self.node(w).left, self.node(w).right);
                if self.node(wl).color == NodeColor::Black && self.node(wr).color == NodeColor::Black {
                    self.node_mut(w).color = NodeColor::Red;
                    x = self.node(x).parent;
                } else {
                    if self.node(wl).color == NodeColor::Black {
                        self.node_mut(wr).color = NodeColor::Black;
                        self.node_mut(w).color = NodeColor::Red;
                        self.left_rotate(w);
                        w = self.node(self.node(x).parent).left;
                    }

                    let xp = self.node(x).parent;
                    self.node_mut(w).color = self.node(xp).color;
                    self.node_mut(xp).color = NodeColor::Black;
                    let wl = self.node(w).left;
                    self.node_mut(wl).color = NodeColor::Black;
                    self.right_rotate(xp);
                    x = self.root;
                }
            }
        }
        self.node_mut(x).color = NodeColor::Black;
        self.reset_sentinel();
    }

    pub fn fix_insert(&mut self, mut x: NodeId) {
        self.recompute_tree_metadata(x);

        while x != self.root && self.node(self.node(x).parent).color == NodeColor::Red {
            let xp = self.node(x).parent;
            let xpp = self.node(xp).parent;
            if xp == self.node(xpp).left {
                let y = self.node(xpp).right;

                if self.node(y).color == NodeColor::Red {
                    self.node_mut(xp).color = NodeColor::Black;
                    self.node_mut(y).color = NodeColor::Black;
                    self.node_mut(xpp).color = NodeColor::Red;
                    x = xpp;
                } else {
                    if x == self.node(xp).right {
                        x = xp;
                        self.left_rotate(x);
                    }

                    let xp = self.node(x).parent;
                    let xpp = self.node(xp).parent;
                    self.node_mut(xp).color = NodeColor::Black;
                    self.node_mut(xpp).color = NodeColor::Red;
                    self.right_rotate(xpp);
                }
            } else {
                let y = self.node(xpp).left;

                if self.node(y).color == NodeColor::Red {
                    self.node_mut(xp).color = NodeColor::Black;
                    self.node_mut(y).color = NodeColor::Black;
                    self.node_mut(xpp).color = NodeColor::Red;
                    x = xpp;
                } else {
                    if x == self.node(xp).left {
                        x = xp;
                        self.right_rotate(x);
                    }
                    let xp = self.node(x).parent;
                    let xpp = self.node(xp).parent;
                    self.node_mut(xp).color = NodeColor::Black;
                    self.node_mut(xpp).color = NodeColor::Red;
                    self.left_rotate(xpp);
                }
            }
        }

        let root = self.root;
        self.node_mut(root).color = NodeColor::Black;
    }

    /// `updateTreeMetadata`: o comprimento ou a contagem de quebras de `x` mudou.
    pub fn update_tree_metadata(&mut self, mut x: NodeId, delta: isize, line_feed_cnt_delta: isize) {
        while x != self.root && x != SENTINEL {
            let parent = self.node(x).parent;
            if self.node(parent).left == x {
                let p = self.node_mut(parent);
                p.size_left = add_signed(p.size_left, delta);
                p.lf_left = add_signed(p.lf_left, line_feed_cnt_delta);
            }
            x = parent;
        }
    }

    /// `recomputeTreeMetadata`: a subárvore direita de algum ancestral de `x` mudou.
    pub fn recompute_tree_metadata(&mut self, mut x: NodeId) {
        if x == self.root {
            return;
        }

        // sobe até o nó cuja subárvore esquerda mudou
        while x != self.root && x == self.node(self.node(x).parent).right {
            x = self.node(x).parent;
        }

        if x == self.root {
            // o nó foi acrescentado ao fim (em ordem)
            return;
        }

        // x é o nó cuja subárvore direita mudou
        x = self.node(x).parent;

        let left = self.node(x).left;
        let delta = self.calculate_size(left) as isize - self.node(x).size_left as isize;
        let lf_delta = self.calculate_lf(left) as isize - self.node(x).lf_left as isize;
        {
            let n = self.node_mut(x);
            n.size_left = add_signed(n.size_left, delta);
            n.lf_left = add_signed(n.lf_left, lf_delta);
        }

        // sobe até a raiz, O(log N)
        while x != self.root && (delta != 0 || lf_delta != 0) {
            let parent = self.node(x).parent;
            if self.node(parent).left == x {
                let p = self.node_mut(parent);
                p.size_left = add_signed(p.size_left, delta);
                p.lf_left = add_signed(p.lf_left, lf_delta);
            }
            x = parent;
        }
    }
}
