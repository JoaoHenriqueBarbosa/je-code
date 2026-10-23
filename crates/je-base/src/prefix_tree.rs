//! Porte de `src/vs/base/common/prefixTree.ts`: uma árvore de prefixos simples, em que o valor fica
//! guardado por segmentos de prefixo bem definidos.
//!
//! O `Symbol('unset')` do original, que distingue "sem valor" de `undefined`, é o `None` do `value`
//! de cada [`Node`]. Os filhos ficam num [`OrderedMap`], porque a ordem de inserção do `Map` do
//! JavaScript é observável em [`WellDefinedPrefixTree::values`] e em
//! [`WellDefinedPrefixTree::delete_recursive`].
//!
//! Diferença de forma: `deleteRecursive` é um gerador no original, e a remoção só acontece quando o
//! chamador consome o iterador (o comentário do original diz que roda mesmo sem consumir, mas o corpo
//! de um gerador só começa no primeiro `next`). Aqui a remoção roda na chamada, como o comentário
//! pretende, e o iterador devolvido já traz os valores removidos; quem consome tudo (o único uso do
//! VS Code) não vê diferença.

use crate::map::OrderedMap;

/// O `onNode` de `insert` e `mutatePath`: chamado para cada nó na descida.
pub type OnNode<'a, V> = &'a mut dyn FnMut(&mut Node<V>);

/// `IPrefixTreeNode` e o `Node` do original: os filhos possíveis e o valor, quando há dado neste nó.
#[derive(Debug, Clone)]
pub struct Node<V> {
    /// Possible children of the node.
    pub children: Option<OrderedMap<String, Node<V>>>,
    /// The value if data exists for this node in the tree. Mutable.
    pub value: Option<V>,
}

impl<V> Default for Node<V> {
    fn default() -> Self {
        Self { children: None, value: None }
    }
}

impl<V> Node<V> {
    pub fn new() -> Self {
        Self::default()
    }

    /// Os filhos, na ordem de inserção.
    pub fn child_entries(&self) -> impl Iterator<Item = (&String, &Node<V>)> {
        self.children.iter().flat_map(OrderedMap::iter)
    }

    fn is_empty(&self) -> bool {
        self.value.is_none() && self.children.as_ref().is_none_or(OrderedMap::is_empty)
    }
}

/// A simple prefix tree implementation where a value is stored based on
/// well-defined prefix segments.
#[derive(Debug, Clone)]
pub struct WellDefinedPrefixTree<V> {
    pub root: Node<V>,
    size: usize,
}

impl<V> Default for WellDefinedPrefixTree<V> {
    fn default() -> Self {
        Self { root: Node::new(), size: 0 }
    }
}

impl<V> WellDefinedPrefixTree<V> {
    pub fn new() -> Self {
        Self::default()
    }

    /// Tree size, not including the root.
    pub fn size(&self) -> usize {
        self.size
    }

    /// Gets the top-level nodes of the tree
    pub fn nodes(&self) -> impl Iterator<Item = &Node<V>> {
        self.root.child_entries().map(|(_, node)| node)
    }

    /// Gets the top-level nodes of the tree
    pub fn entries(&self) -> impl Iterator<Item = (&String, &Node<V>)> {
        self.root.child_entries()
    }

    /// Inserts a new value in the prefix tree.
    /// `on_node` is called for each node as we descend to the insertion point,
    /// including the insertion point itself.
    pub fn insert<S: AsRef<str>>(&mut self, key: impl IntoIterator<Item = S>, value: V, on_node: Option<OnNode<'_, V>>) {
        self.op_node(key, |n| n.value = Some(value), on_node);
    }

    /// Mutates a value in the prefix tree.
    pub fn mutate<S: AsRef<str>>(&mut self, key: impl IntoIterator<Item = S>, mutate: impl FnOnce(Option<V>) -> V) {
        self.op_node(key, |n| n.value = Some(mutate(n.value.take())), None);
    }

    /// Mutates nodes along the path in the prefix tree.
    pub fn mutate_path<S: AsRef<str>>(&mut self, key: impl IntoIterator<Item = S>, mut mutate: impl FnMut(&mut Node<V>)) {
        self.op_node(key, |_| {}, Some(&mut mutate));
    }

    /// Deletes a node from the prefix tree, returning the value it contained.
    pub fn delete<S: AsRef<str>>(&mut self, key: impl IntoIterator<Item = S>) -> Option<V> {
        let parts: Vec<String> = key.into_iter().map(|s| s.as_ref().to_string()).collect();
        let (value, _) = delete_value(&mut self.root, &parts)?;
        self.size -= 1;
        Some(value)
    }

    /// Deletes a subtree from the prefix tree, returning the values they contained.
    pub fn delete_recursive<S: AsRef<str>>(&mut self, key: impl IntoIterator<Item = S>) -> std::vec::IntoIter<V> {
        let parts: Vec<String> = key.into_iter().map(|s| s.as_ref().to_string()).collect();
        let subtree = if parts.is_empty() {
            // special case for the root note
            Some(std::mem::take(&mut self.root))
        } else {
            detach(&mut self.root, &parts).map(|(node, _)| node)
        };
        let mut removed = Vec::new();
        if let Some(subtree) = subtree {
            for value in bfs_iterate(subtree).flatten() {
                self.size -= 1;
                removed.push(value);
            }
        }
        removed.into_iter()
    }

    fn find_node<S: AsRef<str>>(&self, key: impl IntoIterator<Item = S>) -> Option<&Node<V>> {
        let mut node = &self.root;
        for segment in key {
            node = node.children.as_ref()?.get(&segment.as_ref().to_string())?;
        }
        Some(node)
    }

    /// Gets a value from the tree.
    pub fn find<S: AsRef<str>>(&self, key: impl IntoIterator<Item = S>) -> Option<&V> {
        self.find_node(key)?.value.as_ref()
    }

    /// Gets whether the tree has the key, or a parent of the key, already inserted.
    pub fn has_key_or_parent<S: AsRef<str>>(&self, key: impl IntoIterator<Item = S>) -> bool {
        let mut node = &self.root;
        for segment in key {
            let Some(next) = node.children.as_ref().and_then(|c| c.get(&segment.as_ref().to_string())) else {
                return false;
            };
            if next.value.is_some() {
                return true;
            }
            node = next;
        }
        false
    }

    /// Gets whether the tree has the given key or any children.
    pub fn has_key_or_children<S: AsRef<str>>(&self, key: impl IntoIterator<Item = S>) -> bool {
        self.find_node(key).is_some()
    }

    /// Gets whether the tree has the given key.
    pub fn has_key<S: AsRef<str>>(&self, key: impl IntoIterator<Item = S>) -> bool {
        self.find_node(key).is_some_and(|node| node.value.is_some())
    }

    fn op_node<S: AsRef<str>>(
        &mut self,
        key: impl IntoIterator<Item = S>,
        f: impl FnOnce(&mut Node<V>),
        mut on_descend: Option<OnNode<'_, V>>,
    ) {
        let mut node = &mut self.root;
        for part in key {
            let part = part.as_ref().to_string();
            let children = node.children.get_or_insert_with(OrderedMap::new);
            if !children.contains_key(&part) {
                children.insert(part.clone(), Node::new());
            }
            node = children.get_mut(&part).expect("filho acabou de ser garantido");
            if let Some(on_descend) = on_descend.as_deref_mut() {
                on_descend(node);
            }
        }

        let size_before = usize::from(node.value.is_some());
        f(node);
        let size_after = usize::from(node.value.is_some());
        self.size = self.size + size_after - size_before;
    }

    /// Returns an iterable of the tree values in no defined order.
    pub fn values(&self) -> impl Iterator<Item = &V> {
        let mut stack = vec![&self.root];
        std::iter::from_fn(move || {
            while let Some(node) = stack.pop() {
                stack.extend(node.child_entries().map(|(_, child)| child));
                if let Some(value) = &node.value {
                    return Some(value);
                }
            }
            None
        })
    }
}

/// Esvazia o valor do nó em `parts` e poda os nós que ficaram sem valor e sem filhos. O `bool` diz se o
/// nó percorrido ficou vazio (o pai o remove).
fn delete_value<V>(node: &mut Node<V>, parts: &[String]) -> Option<(V, bool)> {
    let Some((part, rest)) = parts.split_first() else {
        let value = node.value.take()?; // não é um nó de verdade se não tem valor
        return Some((value, node.is_empty()));
    };
    let children = node.children.as_mut()?;
    let (value, child_empty) = delete_value(children.get_mut(part)?, rest)?;
    if child_empty {
        children.remove(part);
    }
    Some((value, node.is_empty()))
}

/// Tira o nó em `parts` da árvore (não vazio: `parts` tem ao menos um segmento) e poda os pais que
/// ficaram vazios.
fn detach<V>(node: &mut Node<V>, parts: &[String]) -> Option<(Node<V>, bool)> {
    let (part, rest) = parts.split_first()?;
    let children = node.children.as_mut()?;
    let detached = if rest.is_empty() {
        children.remove(part)?
    } else {
        let (detached, child_empty) = detach(children.get_mut(part)?, rest)?;
        if child_empty {
            children.remove(part);
        }
        detached
    };
    let empty = node.is_empty();
    Some((detached, empty))
}

/// `bfsIterate`: a travessia em pilha do original (visita o último filho primeiro). Devolve o valor de
/// cada nó, na ordem de visita.
fn bfs_iterate<V>(root: Node<V>) -> impl Iterator<Item = Option<V>> {
    let mut stack = vec![root];
    std::iter::from_fn(move || {
        let node = stack.pop()?;
        if let Some(mut children) = node.children {
            stack.extend(children.values_mut().map(std::mem::take));
        }
        Some(node.value)
    })
}
