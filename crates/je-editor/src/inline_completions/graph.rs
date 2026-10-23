//! Porte de `src/vs/editor/contrib/inlineCompletions/browser/model/graph.ts`: um grafo dirigido com remoção
//! de ciclos.
//!
//! Notas de tradução:
//! - O `Set` e o `Map` do JavaScript iteram na ordem de inserção, e `removeCycles` depende dessa ordem (a
//!   busca em profundidade começa pelos nós na ordem em que entraram e percorre as arestas na ordem em que
//!   foram declaradas). Por isso os conjuntos e o mapa de arestas são `OrderedMap`, não `HashMap`.
//! - `T` precisa de `Hash + Eq + Clone`: o JavaScript compara nós por identidade (`SameValueZero`), aqui é a
//!   igualdade do tipo.
//! - `getOutgoing` devolve `Vec<T>` (cópia), como o `Array.from` do original.

use std::collections::HashSet;
use std::hash::Hash;

use je_base::map::OrderedMap;

/// `DirectedGraph<T>`.
pub struct DirectedGraph<T> {
    nodes: OrderedMap<T, ()>,
    outgoing_edges: OrderedMap<T, OrderedMap<T, ()>>,
}

/// O estado compartilhado pela busca em profundidade de `removeCycles`.
struct CycleSearch<T> {
    found_cycles: Vec<T>,
    visited: HashSet<T>,
    recursion_stack: HashSet<T>,
    to_remove: Vec<(T, T)>,
}

impl<T: Hash + Eq + Clone> DirectedGraph<T> {
    /// `new DirectedGraph<T>()`: o construtor do original não recebe argumentos e `from` é o ponto de entrada.
    fn empty() -> Self {
        Self { nodes: OrderedMap::new(), outgoing_edges: OrderedMap::new() }
    }

    /// `DirectedGraph.from(nodes, getOutgoing)`.
    pub fn from(nodes: &[T], get_outgoing: impl Fn(&T) -> Vec<T>) -> Self {
        let mut graph = Self::empty();

        for node in nodes {
            graph.nodes.insert(node.clone(), ());
        }

        for node in nodes {
            let outgoing = get_outgoing(node);
            if !outgoing.is_empty() {
                let mut outgoing_set = OrderedMap::new();
                for target in outgoing {
                    outgoing_set.insert(target, ());
                }
                graph.outgoing_edges.insert(node.clone(), outgoing_set);
            }
        }

        graph
    }

    /// `removeCycles()`: depois dela, o grafo não tem ciclos. Devolve `foundCycles`, o nó de destino de cada
    /// aresta de retorno encontrada.
    pub fn remove_cycles(&mut self) -> Vec<T> {
        let mut search = CycleSearch {
            found_cycles: Vec::new(),
            visited: HashSet::new(),
            recursion_stack: HashSet::new(),
            to_remove: Vec::new(),
        };

        // Run DFS from all unvisited nodes
        for node in self.nodes.keys() {
            if !search.visited.contains(node) {
                self.dfs(node, &mut search);
            }
        }

        // Remove edges that cause cycles
        for (from, to) in &search.to_remove {
            if let Some(outgoing_set) = self.outgoing_edges.get_mut(from) {
                outgoing_set.remove(to);
            }
        }

        search.found_cycles
    }

    fn dfs(&self, node: &T, search: &mut CycleSearch<T>) {
        search.visited.insert(node.clone());
        search.recursion_stack.insert(node.clone());

        if let Some(outgoing) = self.outgoing_edges.get(node) {
            for neighbor in outgoing.keys() {
                if !search.visited.contains(neighbor) {
                    self.dfs(neighbor, search);
                } else if search.recursion_stack.contains(neighbor) {
                    // Found a cycle
                    search.found_cycles.push(neighbor.clone());
                    search.to_remove.push((node.clone(), neighbor.clone()));
                }
            }
        }

        search.recursion_stack.remove(node);
    }

    /// `getOutgoing(node)`.
    pub fn get_outgoing(&self, node: &T) -> Vec<T> {
        match self.outgoing_edges.get(node) {
            Some(outgoing) => outgoing.keys().cloned().collect(),
            None => Vec::new(),
        }
    }
}
