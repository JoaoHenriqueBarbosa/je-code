//! Porte de `src/vs/platform/instantiation/common/graph.ts`.
//!
//! Grafo dirigido com nós identificados por uma chave de texto, que o original calcula com uma função
//! de hash. A ordem de inserção dos nós é observável (`roots()` devolve na ordem do `Map` do
//! JavaScript, e é ela que decide a ordem de criação dos serviços), então o porte a preserva.
//!
//! Divergência de representação: no original cada nó aponta para os vizinhos por referência
//! (`Map<string, Node>`), o que num grafo com ciclos viraria um ciclo de `Rc`. Aqui o nó guarda só as
//! chaves dos vizinhos e o grafo resolve a chave quando precisa do nó. A API observável é a mesma.

use std::collections::HashMap;
use std::fmt;

/// `Node<T>`: a chave, o dado e as chaves das arestas de entrada e de saída (em ordem de inserção).
pub struct Node<T> {
    pub key: String,
    pub data: T,
    incoming: Vec<String>,
    outgoing: Vec<String>,
}

impl<T> Node<T> {
    /// As chaves dos nós que apontam para este.
    pub fn incoming(&self) -> &[String] {
        &self.incoming
    }

    /// As chaves dos nós para os quais este aponta.
    pub fn outgoing(&self) -> &[String] {
        &self.outgoing
    }
}

/// `Graph<T>`.
pub struct Graph<T> {
    /// Em ordem de inserção. Remover deixa `None` e reinserir vai para o fim, como o `Map` do JS.
    nodes: Vec<Option<Node<T>>>,
    index: HashMap<String, usize>,
    hash_fn: Box<dyn Fn(&T) -> String>,
}

impl<T> Graph<T> {
    pub fn new(hash_fn: impl Fn(&T) -> String + 'static) -> Self {
        Self {
            nodes: Vec::new(),
            index: HashMap::new(),
            hash_fn: Box::new(hash_fn),
        }
    }

    fn live(&self) -> impl Iterator<Item = &Node<T>> {
        self.nodes.iter().flatten()
    }

    fn node(&self, key: &str) -> &Node<T> {
        let slot = self.index[key];
        self.nodes[slot].as_ref().expect("nó vivo")
    }

    fn node_mut(&mut self, key: &str) -> &mut Node<T> {
        let slot = self.index[key];
        self.nodes[slot].as_mut().expect("nó vivo")
    }

    /// Os nós sem aresta de saída (os que não dependem de ninguém), na ordem de inserção.
    pub fn roots(&self) -> Vec<&Node<T>> {
        self.live()
            .filter(|node| node.outgoing.is_empty())
            .collect()
    }

    pub fn insert_edge(&mut self, from: T, to: T) {
        let from_key = self.lookup_or_insert_node(from).key.clone();
        let to_key = self.lookup_or_insert_node(to).key.clone();

        let from_node = self.node_mut(&from_key);
        if !from_node.outgoing.contains(&to_key) {
            from_node.outgoing.push(to_key.clone());
        }
        let to_node = self.node_mut(&to_key);
        if !to_node.incoming.contains(&from_key) {
            to_node.incoming.push(from_key);
        }
    }

    pub fn remove_node(&mut self, data: &T) {
        let key = (self.hash_fn)(data);
        if let Some(slot) = self.index.remove(&key) {
            self.nodes[slot] = None;
        }
        for node in self.nodes.iter_mut().flatten() {
            node.outgoing.retain(|k| *k != key);
            node.incoming.retain(|k| *k != key);
        }
    }

    /// Devolve o nó existente (mantendo o dado antigo) ou insere um novo com `data`.
    pub fn lookup_or_insert_node(&mut self, data: T) -> &Node<T> {
        let key = (self.hash_fn)(&data);
        if !self.index.contains_key(&key) {
            self.index.insert(key.clone(), self.nodes.len());
            self.nodes.push(Some(Node {
                key: key.clone(),
                data,
                incoming: Vec::new(),
                outgoing: Vec::new(),
            }));
        }
        self.node(&key)
    }

    pub fn lookup(&self, data: &T) -> Option<&Node<T>> {
        self.index
            .get(&(self.hash_fn)(data))
            .map(|_| self.node(&(self.hash_fn)(data)))
    }

    pub fn is_empty(&self) -> bool {
        self.index.is_empty()
    }

    /// Força bruta e lento, **só** para diagnóstico: a primeira cadeia `a -> b -> a` que achar.
    pub fn find_cycle_slow(&self) -> Option<String> {
        for node in self.live() {
            let mut seen = vec![node.key.clone()];
            if let Some(found) = self.find_cycle(node, &mut seen) {
                return Some(found);
            }
        }
        None
    }

    fn find_cycle(&self, node: &Node<T>, seen: &mut Vec<String>) -> Option<String> {
        for id in &node.outgoing {
            if seen.contains(id) {
                let mut chain = seen.clone();
                chain.push(id.clone());
                return Some(chain.join(" -> "));
            }
            seen.push(id.clone());
            if let Some(found) = self.find_cycle(self.node(id), seen) {
                return Some(found);
            }
            seen.pop();
        }
        None
    }
}

/// O `toString()` do original, usado na mensagem do erro de ciclo quando não se acha a cadeia.
impl<T> fmt::Display for Graph<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let data: Vec<String> = self
            .live()
            .map(|node| {
                format!(
                    "{}\n\t(-> incoming)[{}]\n\t(outgoing ->)[{}]\n",
                    node.key,
                    node.incoming.join(", "),
                    node.outgoing.join(",")
                )
            })
            .collect();
        f.write_str(&data.join("\n"))
    }
}
