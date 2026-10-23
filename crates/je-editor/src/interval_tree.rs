//! Porte de `src/vs/editor/common/model/intervalTree.ts`: a árvore de intervalos das decorações do modelo,
//! uma árvore rubro-negra aumentada (Cormen, Leiserson e Rivest) com `max_end` por subárvore e deslocamento
//! relativo (`delta`) aplicado à subárvore direita de cada nó.
//!
//! O original liga nós por ponteiro e compartilha um `SENTINEL` global entre todas as árvores. Aqui os nós
//! moram numa arena ([`IntervalArena`]) que o modelo compartilha entre as suas três árvores, e se ligam por
//! [`NodeId`]; o índice 0 de cada arena é o sentinela. Como no original, o algoritmo escreve no sentinela
//! (pai, `delta`, `start`, `end`, cor e o bit de visitado), então ele é um nó comum da arena, nunca lido como
//! constante. [`IntervalTree`] guarda só a raiz e o pedido de normalização, e cada operação recebe a arena.
//!
//! Um nó removido da árvore ganha [`DETACHED`] em `parent`, `left` e `right` (o `null` do `detach()`
//! original); ler através dele dá pânico, como o `TypeError` do JavaScript. Posições são `i64`: o original usa
//! `number` com inteiros exatos até 2^53, que cabem com folga.

use std::rc::Rc;

use crate::model::TrackedRangeStickiness;
use crate::range::Range;
use crate::text_model::ModelDecorationOptions;

/// `ClassName`: as classes de decoração que marcam um nó como de validação (squiggly).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClassName {
    EditorHintDecoration,
    EditorInfoDecoration,
    EditorWarningDecoration,
    EditorErrorDecoration,
    EditorUnnecessaryDecoration,
    EditorUnnecessaryInlineDecoration,
    EditorDeprecatedInlineDecoration,
}

impl ClassName {
    /// O valor de texto do membro do `const enum`.
    pub const fn as_str(self) -> &'static str {
        match self {
            ClassName::EditorHintDecoration => "squiggly-hint",
            ClassName::EditorInfoDecoration => "squiggly-info",
            ClassName::EditorWarningDecoration => "squiggly-warning",
            ClassName::EditorErrorDecoration => "squiggly-error",
            ClassName::EditorUnnecessaryDecoration => "squiggly-unnecessary",
            ClassName::EditorUnnecessaryInlineDecoration => "squiggly-inline-unnecessary",
            ClassName::EditorDeprecatedInlineDecoration => "squiggly-inline-deprecated",
        }
    }
}

/// `NodeColor`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NodeColor {
    Black = 0,
    Red = 1,
}

// `Constants`: as máscaras dos bits de `metadata`. A máscara inversa do original é `!MASK`.
const COLOR_MASK: u8 = 0b0000_0001;
const IS_VISITED_MASK: u8 = 0b0000_0010;
const IS_FOR_VALIDATION_MASK: u8 = 0b0000_0100;
const STICKINESS_MASK: u8 = 0b0001_1000;
const STICKINESS_OFFSET: u8 = 3;
const COLLAPSE_ON_REPLACE_EDIT_MASK: u8 = 0b0010_0000;
const IS_MARGIN_MASK: u8 = 0b0100_0000;
const AFFECTS_FONT_MASK: u8 = 0b1000_0000;

/// Por causa de como a remoção funciona (para não percorrer sempre a subárvore direita do nó removido), os
/// deltas dos nós podem crescer e encolher muito. O original corrige os deltas antes que passem do menor SMI
/// do V8 (30 bits com sinal); o porte mantém o mesmo limiar, para normalizar nos mesmos momentos.
pub const MIN_SAFE_DELTA: i64 = -(1 << 30);
/// O maior SMI do V8 (ver [`MIN_SAFE_DELTA`]).
pub const MAX_SAFE_DELTA: i64 = 1 << 30;

fn is_unsafe_delta(delta: i64) -> bool {
    !(MIN_SAFE_DELTA..=MAX_SAFE_DELTA).contains(&delta)
}

/// Índice de um nó na [`IntervalArena`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct NodeId(pub u32);

/// `SENTINEL`: o nó sentinela (as folhas nulas), índice 0 de toda arena.
pub const SENTINEL: NodeId = NodeId(0);

/// Os ponteiros de um nó depois do `detach()` (o `null` do original).
pub const DETACHED: NodeId = NodeId(u32::MAX);

/// `getNodeColor(node)`.
pub fn get_node_color(node: &IntervalNode) -> NodeColor {
    if node.flag(COLOR_MASK) {
        NodeColor::Red
    } else {
        NodeColor::Black
    }
}

/// `setNodeStickiness(node, stickiness)`.
pub fn set_node_stickiness(node: &mut IntervalNode, stickiness: TrackedRangeStickiness) {
    node.metadata = (node.metadata & !STICKINESS_MASK) | ((stickiness as u8) << STICKINESS_OFFSET);
}

fn get_node_stickiness(node: &IntervalNode) -> TrackedRangeStickiness {
    TrackedRangeStickiness::from_u8((node.metadata & STICKINESS_MASK) >> STICKINESS_OFFSET)
}

/// `IntervalNode`.
#[derive(Clone, Debug)]
pub struct IntervalNode {
    /// Cor, visitado, validação, stickiness, `collapseOnReplaceEdit`, margem e `affectsFont`, em bits.
    pub metadata: u8,

    pub parent: NodeId,
    pub left: NodeId,
    pub right: NodeId,

    pub start: i64,
    pub end: i64,
    pub delta: i64,
    pub max_end: i64,

    pub id: String,
    pub owner_id: i32,
    /// `null!` no construtor do original; o modelo sempre chama [`IntervalNode::set_options`] em seguida.
    pub options: Option<Rc<ModelDecorationOptions>>,

    pub cached_version_id: i64,
    pub cached_absolute_start: i64,
    pub cached_absolute_end: i64,
    pub range: Option<Range>,
}

impl IntervalNode {
    /// `new IntervalNode(id, start, end)`, com os ponteiros em `this` (o índice que a arena der ao nó).
    fn new(this: NodeId, id: String, start: i64, end: i64) -> Self {
        let mut node = IntervalNode {
            metadata: 0,
            parent: this,
            left: this,
            right: this,
            start,
            end,
            delta: 0,
            max_end: end,
            id,
            owner_id: 0,
            options: None,
            cached_version_id: 0,
            cached_absolute_start: start,
            cached_absolute_end: end,
            range: None,
        };
        node.set_color(NodeColor::Red);
        set_node_stickiness(
            &mut node,
            TrackedRangeStickiness::NeverGrowsWhenTypingAtEdges,
        );
        node
    }

    fn flag(&self, mask: u8) -> bool {
        self.metadata & mask != 0
    }

    fn set_flag(&mut self, mask: u8, value: bool) {
        self.metadata = (self.metadata & !mask) | if value { mask } else { 0 };
    }

    fn set_color(&mut self, color: NodeColor) {
        self.set_flag(COLOR_MASK, color == NodeColor::Red);
    }

    /// `reset(versionId, start, end, range)`.
    pub fn reset(&mut self, version_id: i64, start: i64, end: i64, range: Option<Range>) {
        self.start = start;
        self.end = end;
        self.max_end = end;
        self.cached_version_id = version_id;
        self.cached_absolute_start = start;
        self.cached_absolute_end = end;
        self.range = range;
    }

    /// `setOptions(options)`.
    pub fn set_options(&mut self, options: Rc<ModelDecorationOptions>) {
        let class_name = options.class_name.as_deref();
        let is_for_validation = [
            ClassName::EditorErrorDecoration,
            ClassName::EditorWarningDecoration,
            ClassName::EditorInfoDecoration,
        ]
        .iter()
        .any(|validation| class_name == Some(validation.as_str()));
        self.set_flag(IS_FOR_VALIDATION_MASK, is_for_validation);
        self.set_flag(IS_MARGIN_MASK, options.glyph_margin_class_name.is_some());
        set_node_stickiness(self, options.stickiness);
        self.set_flag(
            COLLAPSE_ON_REPLACE_EDIT_MASK,
            options.collapse_on_replace_edit,
        );
        self.set_flag(AFFECTS_FONT_MASK, options.affects_font.unwrap_or(false));
        self.options = Some(options);
    }

    /// `setCachedOffsets(absoluteStart, absoluteEnd, cachedVersionId)`.
    pub fn set_cached_offsets(
        &mut self,
        absolute_start: i64,
        absolute_end: i64,
        cached_version_id: i64,
    ) {
        if self.cached_version_id != cached_version_id {
            self.range = None;
        }
        self.cached_version_id = cached_version_id;
        self.cached_absolute_start = absolute_start;
        self.cached_absolute_end = absolute_end;
    }

    /// `detach()`.
    pub fn detach(&mut self) {
        self.parent = DETACHED;
        self.left = DETACHED;
        self.right = DETACHED;
    }
}

/// A arena dos nós, compartilhada pelas árvores de um modelo; o índice 0 é o [`SENTINEL`].
#[derive(Debug)]
pub struct IntervalArena {
    nodes: Vec<IntervalNode>,
    free: Vec<NodeId>,
}

impl Default for IntervalArena {
    fn default() -> Self {
        Self::new()
    }
}

impl IntervalArena {
    /// Uma arena só com o sentinela: preto, ponteiros nele mesmo, `id` vazio (o `null!` do original).
    pub fn new() -> Self {
        let mut sentinel = IntervalNode::new(SENTINEL, String::new(), 0, 0);
        sentinel.set_color(NodeColor::Black);
        IntervalArena {
            nodes: vec![sentinel],
            free: Vec::new(),
        }
    }

    /// `new IntervalNode(id, start, end)`: um nó solto, vermelho, com os ponteiros nele mesmo.
    pub fn alloc(&mut self, id: String, start: i64, end: i64) -> NodeId {
        match self.free.pop() {
            Some(this) => {
                self.nodes[this.0 as usize] = IntervalNode::new(this, id, start, end);
                this
            }
            None => {
                let this = NodeId(self.nodes.len() as u32);
                self.nodes.push(IntervalNode::new(this, id, start, end));
                this
            }
        }
    }

    /// Devolve o slot de um nó que já saiu de todas as árvores; o índice pode voltar num `alloc` futuro.
    pub fn free(&mut self, node: NodeId) {
        assert_ne!(node, SENTINEL, "o sentinela não sai da arena");
        let slot = &mut self.nodes[node.0 as usize];
        slot.detach();
        slot.options = None;
        slot.range = None;
        self.free.push(node);
    }

    /// Quantos nós vivos a arena tem, sem o sentinela.
    pub fn len(&self) -> usize {
        self.nodes.len() - 1 - self.free.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    #[inline]
    pub fn get(&self, node: NodeId) -> &IntervalNode {
        &self.nodes[node.0 as usize]
    }

    #[inline]
    pub fn get_mut(&mut self, node: NodeId) -> &mut IntervalNode {
        &mut self.nodes[node.0 as usize]
    }

    fn color(&self, node: NodeId) -> NodeColor {
        get_node_color(self.get(node))
    }

    fn set_color(&mut self, node: NodeId, color: NodeColor) {
        self.get_mut(node).set_color(color);
    }

    fn is_visited(&self, node: NodeId) -> bool {
        self.get(node).flag(IS_VISITED_MASK)
    }

    fn set_visited(&mut self, node: NodeId, value: bool) {
        self.get_mut(node).set_flag(IS_VISITED_MASK, value);
    }

    /// `node === node.parent.right`.
    fn is_right_child(&self, node: NodeId) -> bool {
        node == self.get(self.get(node).parent).right
    }

    /// O passo de subida comum aos percursos com bit de visitado: limpa os filhos e devolve o pai.
    fn leave_visited(&mut self, node: NodeId) -> NodeId {
        let IntervalNode {
            left,
            right,
            parent,
            ..
        } = *self.get(node);
        self.set_visited(left, false);
        self.set_visited(right, false);
        parent
    }

    /// Mantém o sentinela no estado que os testes de invariante conferem (`resetSentinel`).
    fn reset_sentinel(&mut self) {
        let sentinel = self.get_mut(SENTINEL);
        sentinel.parent = SENTINEL;
        sentinel.delta = 0; // opcional
        sentinel.start = 0; // opcional
        sentinel.end = 0; // opcional
    }

    fn leftest(&self, mut node: NodeId) -> NodeId {
        while self.get(node).left != SENTINEL {
            node = self.get(node).left;
        }
        node
    }

    fn compute_max_end(&self, node: NodeId) -> i64 {
        let n = self.get(node);
        let mut max_end = n.end;
        if n.left != SENTINEL {
            max_end = max_end.max(self.get(n.left).max_end);
        }
        if n.right != SENTINEL {
            max_end = max_end.max(self.get(n.right).max_end + n.delta);
        }
        max_end
    }

    fn recompute_max_end_walk_to_root(&mut self, mut node: NodeId) {
        while node != SENTINEL {
            let max_end = self.compute_max_end(node);
            if self.get(node).max_end == max_end {
                // não precisa subir mais
                return;
            }
            self.get_mut(node).max_end = max_end;
            node = self.get(node).parent;
        }
    }
}

/// `recomputeMaxEnd(node)`.
pub fn recompute_max_end(arena: &mut IntervalArena, node: NodeId) {
    arena.get_mut(node).max_end = arena.compute_max_end(node);
}

/// `IntervalTree`: a raiz de uma árvore cujos nós moram numa [`IntervalArena`].
#[derive(Clone, Copy, Debug)]
pub struct IntervalTree {
    pub root: NodeId,
    pub request_normalize_delta: bool,
}

impl Default for IntervalTree {
    fn default() -> Self {
        Self::new()
    }
}

/// O filtro de `search` e `intervalSearch`, com o `cachedVersionId` gravado nos nós visitados.
#[derive(Clone, Copy)]
struct SearchFilter {
    owner_id: i32,
    out_validation: bool,
    font_decorations: bool,
    cached_version_id: i64,
    only_margin_decorations: bool,
}

impl SearchFilter {
    fn accept(&self, node: &mut IntervalNode, node_start: i64, node_end: i64) -> bool {
        node.set_cached_offsets(node_start, node_end, self.cached_version_id);
        !(self.owner_id != 0 && node.owner_id != 0 && node.owner_id != self.owner_id)
            && !(self.out_validation && node.flag(IS_FOR_VALIDATION_MASK))
            && !(self.font_decorations && node.flag(AFFECTS_FONT_MASK))
            && !(self.only_margin_decorations && !node.flag(IS_MARGIN_MASK))
    }
}

impl IntervalTree {
    pub fn new() -> Self {
        IntervalTree {
            root: SENTINEL,
            request_normalize_delta: false,
        }
    }

    /// `intervalSearch(...)`: os nós que intersectam `[start, end]`, em ordem, com os deslocamentos absolutos
    /// gravados em `cached_absolute_start`/`cached_absolute_end`.
    #[allow(clippy::too_many_arguments)]
    pub fn interval_search(
        &self,
        arena: &mut IntervalArena,
        start: i64,
        end: i64,
        filter_owner_id: i32,
        filter_out_validation: bool,
        filter_font_decorations: bool,
        cached_version_id: i64,
        only_margin_decorations: bool,
    ) -> Vec<NodeId> {
        if self.root == SENTINEL {
            return Vec::new();
        }
        let filter = SearchFilter {
            owner_id: filter_owner_id,
            out_validation: filter_out_validation,
            font_decorations: filter_font_decorations,
            cached_version_id,
            only_margin_decorations,
        };
        let mut result = Vec::new();
        self.overlap_walk(arena, start, end, |id, node, node_start, node_end| {
            if filter.accept(node, node_start, node_end) {
                result.push(id);
            }
        });
        result
    }

    /// `search(...)`: todos os nós, em ordem, filtrados, com os deslocamentos absolutos gravados.
    pub fn search(
        &self,
        arena: &mut IntervalArena,
        filter_owner_id: i32,
        filter_out_validation: bool,
        filter_font_decorations: bool,
        cached_version_id: i64,
        only_margin_decorations: bool,
    ) -> Vec<NodeId> {
        if self.root == SENTINEL {
            return Vec::new();
        }
        let filter = SearchFilter {
            owner_id: filter_owner_id,
            out_validation: filter_out_validation,
            font_decorations: filter_font_decorations,
            cached_version_id,
            only_margin_decorations,
        };
        let mut node = self.root;
        let mut delta = 0;
        let mut result = Vec::new();
        while node != SENTINEL {
            if arena.is_visited(node) {
                // subindo a partir deste nó
                if arena.is_right_child(node) {
                    delta -= arena.get(arena.get(node).parent).delta;
                }
                node = arena.leave_visited(node);
                continue;
            }

            let IntervalNode { left, right, .. } = *arena.get(node);
            if left != SENTINEL && !arena.is_visited(left) {
                // vai para a esquerda
                node = left;
                continue;
            }

            // trata o nó corrente
            let n = arena.get_mut(node);
            let (node_start, node_end) = (delta + n.start, delta + n.end);
            if filter.accept(n, node_start, node_end) {
                result.push(node);
            }
            n.set_flag(IS_VISITED_MASK, true);

            if right != SENTINEL && !arena.is_visited(right) {
                // vai para a direita
                delta += arena.get(node).delta;
                node = right;
                continue;
            }
        }
        arena.set_visited(self.root, false);
        result
    }

    /// `collectNodesFromOwner(ownerId)`: em ordem. Não grava `cached_absolute_start` nem `cached_absolute_end`.
    pub fn collect_nodes_from_owner(&self, arena: &IntervalArena, owner_id: i32) -> Vec<NodeId> {
        let mut result = Vec::new();
        let mut node = arena.leftest(self.root);
        while node != SENTINEL {
            if arena.get(node).owner_id == owner_id {
                result.push(node);
            }
            node = in_order_next(arena, node);
        }
        result
    }

    /// `collectNodesPostOrder()`. Não grava `cached_absolute_start` nem `cached_absolute_end`.
    pub fn collect_nodes_post_order(&self, arena: &IntervalArena) -> Vec<NodeId> {
        let mut result = Vec::new();
        let mut node = post_order_first(arena, self.root);
        while node != SENTINEL {
            result.push(node);
            let parent = arena.get(node).parent;
            node = if parent == SENTINEL {
                SENTINEL
            } else {
                let IntervalNode { left, right, .. } = *arena.get(parent);
                if node == left && right != SENTINEL {
                    post_order_first(arena, right)
                } else {
                    parent
                }
            };
        }
        result
    }

    /// `insert(node)`.
    pub fn insert(&mut self, arena: &mut IntervalArena, node: NodeId) {
        self.rb_tree_insert(arena, node);
        self.normalize_delta_if_necessary(arena);
    }

    /// `delete(node)`.
    pub fn delete(&mut self, arena: &mut IntervalArena, node: NodeId) {
        self.rb_tree_delete(arena, node);
        self.normalize_delta_if_necessary(arena);
    }

    /// `resolveNode(node, cachedVersionId)`: grava os deslocamentos absolutos do nó.
    pub fn resolve_node(&self, arena: &mut IntervalArena, node: NodeId, cached_version_id: i64) {
        let mut delta = 0;
        let mut current = node;
        while current != self.root {
            if arena.is_right_child(current) {
                delta += arena.get(arena.get(current).parent).delta;
            }
            current = arena.get(current).parent;
        }

        let n = arena.get_mut(node);
        let (node_start, node_end) = (n.start + delta, n.end + delta);
        n.set_cached_offsets(node_start, node_end, cached_version_id);
    }

    /// `acceptReplace(offset, length, textLength, forceMoveMarkers)`.
    pub fn accept_replace(
        &mut self,
        arena: &mut IntervalArena,
        offset: i64,
        length: i64,
        text_length: i64,
        force_move_markers: bool,
    ) {
        // A estratégia é tirar todos os nós diretamente atingidos e depois devolvê-los à árvore.

        // (1) junta como nós de interesse todos os que intersectam esta edição
        let nodes_of_interest = self.search_for_editing(arena, offset, offset + length);

        // (2) tira da árvore todos os nós que intersectam esta edição
        for &node in &nodes_of_interest {
            self.rb_tree_delete(arena, node);
        }
        self.normalize_delta_if_necessary(arena);

        // (3) edita todos os nós da árvore, menos os de interesse
        self.no_overlap_replace(arena, offset, offset + length, text_length);
        self.normalize_delta_if_necessary(arena);

        // (4) edita os nós de interesse e os devolve à árvore
        for &node in &nodes_of_interest {
            let n = arena.get_mut(node);
            n.start = n.cached_absolute_start;
            n.end = n.cached_absolute_end;
            node_accept_edit(
                arena,
                node,
                offset,
                offset + length,
                text_length,
                force_move_markers,
            );
            let n = arena.get_mut(node);
            n.max_end = n.end;
            self.rb_tree_insert(arena, node);
        }
        self.normalize_delta_if_necessary(arena);
    }

    /// `getAllInOrder()`.
    pub fn get_all_in_order(&self, arena: &mut IntervalArena) -> Vec<NodeId> {
        self.search(arena, 0, false, false, 0, false)
    }

    fn normalize_delta_if_necessary(&mut self, arena: &mut IntervalArena) {
        if !self.request_normalize_delta {
            return;
        }
        self.request_normalize_delta = false;
        self.normalize_delta(arena);
    }

    //#region Normalização do delta

    fn normalize_delta(&self, arena: &mut IntervalArena) {
        let mut node = self.root;
        let mut delta = 0;
        while node != SENTINEL {
            let IntervalNode { left, right, .. } = *arena.get(node);
            if left != SENTINEL && !arena.is_visited(left) {
                // vai para a esquerda
                node = left;
                continue;
            }

            if right != SENTINEL && !arena.is_visited(right) {
                // vai para a direita
                delta += arena.get(node).delta;
                node = right;
                continue;
            }

            // trata o nó corrente
            let n = arena.get_mut(node);
            n.start += delta;
            n.end += delta;
            n.delta = 0;
            recompute_max_end(arena, node);

            arena.set_visited(node, true);

            // subindo a partir deste nó
            if arena.is_right_child(node) {
                delta -= arena.get(arena.get(node).parent).delta;
            }
            node = arena.leave_visited(node);
        }

        arena.set_visited(self.root, false);
    }

    //#endregion

    //#region Edição

    /// O percurso com poda de `searchForEditing` e `intervalSearch`: `on_overlap` recebe cada nó que
    /// intersecta `[start, end]`, com os deslocamentos absolutos dele.
    fn overlap_walk(
        &self,
        arena: &mut IntervalArena,
        start: i64,
        end: i64,
        mut on_overlap: impl FnMut(NodeId, &mut IntervalNode, i64, i64),
    ) {
        self.pruned_walk(
            arena,
            start,
            end,
            false,
            |_| {},
            |id, n, node_start, delta| {
                let node_end = delta + n.end;
                if node_end >= start {
                    // há sobreposição
                    on_overlap(id, n, node_start, node_end);
                }
            },
        );
    }

    /// O laço comum de `searchForEditing`, `noOverlapReplace` e `intervalSearch`.
    ///
    /// Dois intervalos A e B se sobrepõem só quando `A.low <= B.high` e `A.high >= B.low`. Ao procurar os
    /// nós que se sobrepõem a um intervalo dado, dá para pular de cara:
    ///  a) todos os nós à direita de nós cujo início passa do fim do intervalo dado;
    ///  b) todos os nós cujo `max_end` fica antes do início do intervalo dado.
    ///
    /// `past_end` é chamado no nó do caso a); `visit`, no nó corrente que não passa de `end`, com o início
    /// absoluto e o `delta` acumulado. Com `recompute_on_leave`, o `max_end` é recalculado ao subir de cada
    /// nó (o `noOverlapReplace`).
    fn pruned_walk(
        &self,
        arena: &mut IntervalArena,
        start: i64,
        end: i64,
        recompute_on_leave: bool,
        mut past_end: impl FnMut(&mut IntervalNode),
        mut visit: impl FnMut(NodeId, &mut IntervalNode, i64, i64),
    ) {
        let mut node = self.root;
        let mut delta = 0;
        while node != SENTINEL {
            if arena.is_visited(node) {
                // subindo a partir deste nó
                if arena.is_right_child(node) {
                    delta -= arena.get(arena.get(node).parent).delta;
                }
                if recompute_on_leave {
                    recompute_max_end(arena, node);
                }
                node = arena.leave_visited(node);
                continue;
            }

            let IntervalNode { left, right, .. } = *arena.get(node);
            if !arena.is_visited(left) {
                // primeira vez neste nó
                let node_max_end = delta + arena.get(node).max_end;
                if node_max_end < start {
                    // caso b): não precisa olhar este nó nem os filhos
                    arena.set_visited(node, true);
                    continue;
                }

                if left != SENTINEL {
                    // vai para a esquerda
                    node = left;
                    continue;
                }
            }

            // trata o nó corrente
            let n = arena.get_mut(node);
            let node_start = delta + n.start;
            if node_start > end {
                past_end(n);
                // caso a): não precisa olhar este nó nem a subárvore direita
                n.set_flag(IS_VISITED_MASK, true);
                continue;
            }

            visit(node, n, node_start, delta);
            n.set_flag(IS_VISITED_MASK, true);

            if right != SENTINEL && !arena.is_visited(right) {
                // vai para a direita
                delta += arena.get(node).delta;
                node = right;
                continue;
            }
        }

        arena.set_visited(self.root, false);
    }

    /// `searchForEditing(T, start, end)`: grava os deslocamentos absolutos com `cachedVersionId` 0.
    fn search_for_editing(&self, arena: &mut IntervalArena, start: i64, end: i64) -> Vec<NodeId> {
        let mut result = Vec::new();
        self.overlap_walk(arena, start, end, |id, node, node_start, node_end| {
            node.set_cached_offsets(node_start, node_end, 0);
            result.push(id);
        });
        result
    }

    /// `noOverlapReplace(T, start, end, textLength)`.
    fn no_overlap_replace(
        &mut self,
        arena: &mut IntervalArena,
        start: i64,
        end: i64,
        text_length: i64,
    ) {
        let edit_delta = text_length - (end - start);
        let mut request_normalize_delta = false;
        self.pruned_walk(
            arena,
            start,
            end,
            true,
            |n| {
                n.start += edit_delta;
                n.end += edit_delta;
                n.delta += edit_delta;
                if is_unsafe_delta(n.delta) {
                    request_normalize_delta = true;
                }
            },
            |_, _, _, _| {},
        );
        self.request_normalize_delta |= request_normalize_delta;
    }

    //#endregion

    //#region Inserção

    fn rb_tree_insert(&mut self, arena: &mut IntervalArena, new_node: NodeId) -> NodeId {
        if self.root == SENTINEL {
            let n = arena.get_mut(new_node);
            n.parent = SENTINEL;
            n.left = SENTINEL;
            n.right = SENTINEL;
            n.set_color(NodeColor::Black);
            self.root = new_node;
            return self.root;
        }

        self.tree_insert(arena, new_node);

        arena.recompute_max_end_walk_to_root(arena.get(new_node).parent);

        // conserta a árvore
        let mut x = new_node;
        while x != self.root && arena.color(parent(arena, x)) == NodeColor::Red {
            let xp = parent(arena, x);
            let xpp = parent(arena, xp);
            let parent_is_left = xp == arena.get(xpp).left;
            let y = child(arena, xpp, !parent_is_left);

            if arena.color(y) == NodeColor::Red {
                arena.set_color(xp, NodeColor::Black);
                arena.set_color(y, NodeColor::Black);
                arena.set_color(xpp, NodeColor::Red);
                x = xpp;
            } else {
                if x == child(arena, xp, !parent_is_left) {
                    x = xp;
                    self.rotate(arena, x, parent_is_left);
                }
                let xp = parent(arena, x);
                let xpp = parent(arena, xp);
                arena.set_color(xp, NodeColor::Black);
                arena.set_color(xpp, NodeColor::Red);
                self.rotate(arena, xpp, !parent_is_left);
            }
        }

        arena.set_color(self.root, NodeColor::Black);

        new_node
    }

    fn tree_insert(&self, arena: &mut IntervalArena, z: NodeId) {
        let mut delta = 0;
        let mut x = self.root;
        let IntervalNode {
            start: z_absolute_start,
            end: z_absolute_end,
            ..
        } = *arena.get(z);
        loop {
            let IntervalNode {
                start,
                end,
                left,
                right,
                delta: x_delta,
                ..
            } = *arena.get(x);
            let cmp =
                interval_compare(z_absolute_start, z_absolute_end, start + delta, end + delta);
            if cmp < 0 {
                // o nó vai à esquerda, então não é afetado pelo delta do nó
                if left == SENTINEL {
                    shift_node(arena.get_mut(z), -delta);
                    arena.get_mut(x).left = z;
                    break;
                }
                x = left;
            } else {
                // o nó vai à direita, deslocado pelo delta do nó
                if right == SENTINEL {
                    shift_node(arena.get_mut(z), -(delta + x_delta));
                    arena.get_mut(x).right = z;
                    break;
                }
                delta += x_delta;
                x = right;
            }
        }

        let n = arena.get_mut(z);
        n.parent = x;
        n.left = SENTINEL;
        n.right = SENTINEL;
        n.set_color(NodeColor::Red);
    }

    //#endregion

    //#region Remoção

    fn rb_tree_delete(&mut self, arena: &mut IntervalArena, z: NodeId) {
        let x: NodeId;
        let y: NodeId;

        // RB-DELETE, só que sem trocar z e y no caso c): sempre se remove o nó apontado por z.
        let IntervalNode {
            left: z_left,
            right: z_right,
            delta: z_delta,
            ..
        } = *arena.get(z);

        if z_left == SENTINEL {
            x = z_right;
            y = z;

            // o delta de x deixa de ser influenciado pelo de z
            let xn = arena.get_mut(x);
            xn.delta += z_delta;
            if is_unsafe_delta(xn.delta) {
                self.request_normalize_delta = true;
            }
            xn.start += z_delta;
            xn.end += z_delta;
        } else if z_right == SENTINEL {
            x = z_left;
            y = z;
        } else {
            y = arena.leftest(z_right);
            x = arena.get(y).right;

            // o delta de y deixa de ser influenciado pelo de z, mas não se quer percorrer toda a
            // subárvore direita de x; por isso o delta de z fica em y, e só x é ajustado
            let y_delta = arena.get(y).delta;
            let xn = arena.get_mut(x);
            xn.start += y_delta;
            xn.end += y_delta;
            xn.delta += y_delta;
            if is_unsafe_delta(xn.delta) {
                self.request_normalize_delta = true;
            }

            let yn = arena.get_mut(y);
            yn.start += z_delta;
            yn.end += z_delta;
            yn.delta = z_delta;
            if is_unsafe_delta(yn.delta) {
                self.request_normalize_delta = true;
            }
        }

        if y == self.root {
            self.root = x;
            arena.set_color(x, NodeColor::Black);

            arena.get_mut(z).detach();
            arena.reset_sentinel();
            recompute_max_end(arena, x);
            arena.get_mut(self.root).parent = SENTINEL;
            return;
        }

        let y_was_red = arena.color(y) == NodeColor::Red;

        let y_parent = parent(arena, y);
        if y == arena.get(y_parent).left {
            arena.get_mut(y_parent).left = x;
        } else {
            arena.get_mut(y_parent).right = x;
        }

        if y == z {
            arena.get_mut(x).parent = y_parent;
        } else {
            arena.get_mut(x).parent = if y_parent == z { y } else { y_parent };

            let IntervalNode {
                left,
                right,
                parent: z_parent,
                ..
            } = *arena.get(z);
            let z_color = arena.color(z);
            let yn = arena.get_mut(y);
            yn.left = left;
            yn.right = right;
            yn.parent = z_parent;
            yn.set_color(z_color);

            if z == self.root {
                self.root = y;
            } else if z == arena.get(z_parent).left {
                arena.get_mut(z_parent).left = y;
            } else {
                arena.get_mut(z_parent).right = y;
            }

            let IntervalNode { left, right, .. } = *arena.get(y);
            if left != SENTINEL {
                arena.get_mut(left).parent = y;
            }
            if right != SENTINEL {
                arena.get_mut(right).parent = y;
            }
        }

        arena.get_mut(z).detach();

        if y_was_red {
            arena.recompute_max_end_walk_to_root(parent(arena, x));
            if y != z {
                arena.recompute_max_end_walk_to_root(y);
                arena.recompute_max_end_walk_to_root(parent(arena, y));
            }
            arena.reset_sentinel();
            return;
        }

        arena.recompute_max_end_walk_to_root(x);
        arena.recompute_max_end_walk_to_root(parent(arena, x));
        if y != z {
            arena.recompute_max_end_walk_to_root(y);
            arena.recompute_max_end_walk_to_root(parent(arena, y));
        }

        // RB-DELETE-FIXUP
        let mut x = x;
        while x != self.root && arena.color(x) == NodeColor::Black {
            let xp = parent(arena, x);
            let x_is_left = x == arena.get(xp).left;
            let mut w = child(arena, xp, !x_is_left);

            if arena.color(w) == NodeColor::Red {
                arena.set_color(w, NodeColor::Black);
                arena.set_color(xp, NodeColor::Red);
                self.rotate(arena, xp, x_is_left);
                w = child(arena, parent(arena, x), !x_is_left);
            }

            let IntervalNode {
                left: w_left,
                right: w_right,
                ..
            } = *arena.get(w);
            if arena.color(w_left) == NodeColor::Black && arena.color(w_right) == NodeColor::Black {
                arena.set_color(w, NodeColor::Red);
                x = parent(arena, x);
            } else {
                // o sobrinho do lado de fora (`w.right` quando x está à esquerda) e o de dentro
                if arena.color(child(arena, w, !x_is_left)) == NodeColor::Black {
                    arena.set_color(child(arena, w, x_is_left), NodeColor::Black);
                    arena.set_color(w, NodeColor::Red);
                    self.rotate(arena, w, !x_is_left);
                    w = child(arena, parent(arena, x), !x_is_left);
                }

                let xp = parent(arena, x);
                let xp_color = arena.color(xp);
                arena.set_color(w, xp_color);
                arena.set_color(xp, NodeColor::Black);
                arena.set_color(child(arena, w, !x_is_left), NodeColor::Black);
                self.rotate(arena, xp, x_is_left);
                x = self.root;
            }
        }

        arena.set_color(x, NodeColor::Black);
        arena.reset_sentinel();
    }

    //#endregion

    //#region Rotações

    /// `leftRotate(T, x)` com `left`, `rightRotate(T, x)` sem. As duas são espelho uma da outra, com o
    /// ajuste de delta trocando de sinal: o delta de um nó vale para a subárvore direita dele.
    fn rotate(&mut self, arena: &mut IntervalArena, x: NodeId, left: bool) {
        // y é o filho que sobe
        let y = child(arena, x, !left);

        // na rotação à esquerda, y deixa de ser influenciado pelo delta de x; na rotação à direita, x
        // (que desce à direita de y) passa a ser
        let (target, by) = if left {
            (y, arena.get(x).delta)
        } else {
            (x, -arena.get(y).delta)
        };
        let tn = arena.get_mut(target);
        tn.delta += by;
        if is_unsafe_delta(tn.delta) {
            self.request_normalize_delta = true;
        }
        tn.start += by;
        tn.end += by;

        // a subárvore interna de y passa para x
        let inner = child(arena, y, left);
        set_child(arena, x, !left, inner);
        if inner != SENTINEL {
            arena.get_mut(inner).parent = x;
        }
        let x_parent = parent(arena, x);
        arena.get_mut(y).parent = x_parent;
        if x_parent == SENTINEL {
            self.root = y;
        } else {
            let x_is_left = x == arena.get(x_parent).left;
            set_child(arena, x_parent, x_is_left, y);
        }

        set_child(arena, y, left, x);
        arena.get_mut(x).parent = y;

        recompute_max_end(arena, x);
        recompute_max_end(arena, y);
    }

    //#endregion
}

fn parent(arena: &IntervalArena, node: NodeId) -> NodeId {
    arena.get(node).parent
}

/// `node.left` (com `left`) ou `node.right`.
fn child(arena: &IntervalArena, node: NodeId, left: bool) -> NodeId {
    let n = arena.get(node);
    if left { n.left } else { n.right }
}

fn set_child(arena: &mut IntervalArena, node: NodeId, left: bool, value: NodeId) {
    let n = arena.get_mut(node);
    if left {
        n.left = value;
    } else {
        n.right = value;
    }
}

/// Desloca `start`, `end` e `max_end` de um nó solto antes de pendurá-lo.
fn shift_node(node: &mut IntervalNode, by: i64) {
    node.start += by;
    node.end += by;
    node.max_end += by;
}

/// O sucessor em ordem pelos ponteiros de pai, sem bit de visitado (permite percorrer com `&IntervalArena`).
fn in_order_next(arena: &IntervalArena, mut node: NodeId) -> NodeId {
    let right = arena.get(node).right;
    if right != SENTINEL {
        return arena.leftest(right);
    }
    let mut up = parent(arena, node);
    while up != SENTINEL && node == arena.get(up).right {
        node = up;
        up = parent(arena, up);
    }
    up
}

/// O primeiro nó em pós-ordem da subárvore de `node`.
fn post_order_first(arena: &IntervalArena, mut node: NodeId) -> NodeId {
    while node != SENTINEL {
        let IntervalNode { left, right, .. } = *arena.get(node);
        if left != SENTINEL {
            node = left;
        } else if right != SENTINEL {
            node = right;
        } else {
            break;
        }
    }
    node
}

//#region Edição

#[derive(Clone, Copy, PartialEq, Eq)]
enum MarkerMoveSemantics {
    MarkerDefined,
    ForceMove,
    ForceStay,
}

fn adjust_marker_before_column(
    marker_offset: i64,
    marker_stick_to_previous_character: bool,
    check_offset: i64,
    move_semantics: MarkerMoveSemantics,
) -> bool {
    if marker_offset < check_offset {
        return true;
    }
    if marker_offset > check_offset {
        return false;
    }
    match move_semantics {
        MarkerMoveSemantics::ForceMove => false,
        MarkerMoveSemantics::ForceStay => true,
        MarkerMoveSemantics::MarkerDefined => marker_stick_to_previous_character,
    }
}

/// `nodeAcceptEdit(node, start, end, textLength, forceMoveMarkers)`: bem mais complicado que o necessário,
/// para manter o comportamento de quando as decorações eram dois marcadores.
pub fn node_accept_edit(
    arena: &mut IntervalArena,
    node: NodeId,
    start: i64,
    end: i64,
    text_length: i64,
    force_move_markers: bool,
) {
    let n = arena.get_mut(node);
    let node_stickiness = get_node_stickiness(n);
    let start_stick_to_previous_character = matches!(
        node_stickiness,
        TrackedRangeStickiness::AlwaysGrowsWhenTypingAtEdges
            | TrackedRangeStickiness::GrowsOnlyWhenTypingBefore
    );
    let end_stick_to_previous_character = matches!(
        node_stickiness,
        TrackedRangeStickiness::NeverGrowsWhenTypingAtEdges
            | TrackedRangeStickiness::GrowsOnlyWhenTypingBefore
    );

    let deleting_cnt = end - start;
    let inserting_cnt = text_length;
    let common_length = deleting_cnt.min(inserting_cnt);

    let node_start = n.start;
    let mut start_done = false;

    let node_end = n.end;
    let mut end_done = false;

    if start <= node_start && node_end <= end && n.flag(COLLAPSE_ON_REPLACE_EDIT_MASK) {
        // a edição cobre a decoração inteira e a decoração pediu para colapsar
        n.start = start;
        start_done = true;
        n.end = start;
        end_done = true;
    }

    // Cada etapa confere os dois marcadores contra uma coluna; a última move os que ainda faltam para
    // depois do texto inserido.
    let mut step =
        |check_offset: i64, move_semantics: MarkerMoveSemantics, moved_to: Option<i64>| {
            if !start_done
                && adjust_marker_before_column(
                    node_start,
                    start_stick_to_previous_character,
                    check_offset,
                    move_semantics,
                )
            {
                if let Some(offset) = moved_to {
                    n.start = offset;
                }
                start_done = true;
            }
            if !end_done
                && adjust_marker_before_column(
                    node_end,
                    end_stick_to_previous_character,
                    check_offset,
                    move_semantics,
                )
            {
                if let Some(offset) = moved_to {
                    n.end = offset;
                }
                end_done = true;
            }
        };

    step(
        start,
        if force_move_markers {
            MarkerMoveSemantics::ForceMove
        } else if deleting_cnt > 0 {
            MarkerMoveSemantics::ForceStay
        } else {
            MarkerMoveSemantics::MarkerDefined
        },
        None,
    );

    if common_length > 0 && !force_move_markers {
        step(
            start + common_length,
            if deleting_cnt > inserting_cnt {
                MarkerMoveSemantics::ForceStay
            } else {
                MarkerMoveSemantics::MarkerDefined
            },
            None,
        );
    }

    step(
        end,
        if force_move_markers {
            MarkerMoveSemantics::ForceMove
        } else {
            MarkerMoveSemantics::MarkerDefined
        },
        Some(start + inserting_cnt),
    );

    // Fim
    let delta_column = inserting_cnt - deleting_cnt;
    if !start_done {
        n.start = 0.max(node_start + delta_column);
    }
    if !end_done {
        n.end = 0.max(node_end + delta_column);
    }

    if n.start > n.end {
        n.end = n.start;
    }
}

//#endregion

/// `intervalCompare(aStart, aEnd, bStart, bEnd)`.
pub fn interval_compare(a_start: i64, a_end: i64, b_start: i64, b_end: i64) -> i64 {
    if a_start == b_start {
        return a_end - b_end;
    }
    a_start - b_start
}
