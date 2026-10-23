//! Porte de `src/vs/base/common/resourceTree.ts`: uma árvore de recursos (arquivos e pastas) indexada
//! pelo caminho relativo à raiz, com um elemento opcional em cada nó.
//!
//! Diferenças de forma:
//! - Os nós são `Rc<Node>` com o pai por `Weak` (o original usa referência forte nos dois sentidos, o
//!   que em Rust viraria um ciclo que nunca é liberado). Um nó solto da árvore (removido) perde o pai.
//! - `T` e `C` são `Clone`: o contexto é compartilhado entre os nós como no original e `delete`
//!   devolve o elemento removido.
//! - `ResourceTree.isResourceNode` (um `instanceof`) não existe: o tipo já diz que é um nó.
//! - `IResourceNode` é o próprio [`Node`], que só expõe leitura (o `element` muda por dentro da árvore).

use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap};
use std::rc::{Rc, Weak};

use crate::decorators::Memoize;
use crate::path::{PathModule, ProcessEnvironment};
use crate::resources::ExtUri;
use crate::ternary_search_tree::{KeyIterator, PathIterator};
use crate::uri::URI;

/// O `Map<string, Node>` de filhos: iteração na ordem de inserção.
struct Children<T, C> {
    index: HashMap<String, u64>,
    ordered: BTreeMap<u64, (String, Rc<Node<T, C>>)>,
    next_seq: u64,
}

impl<T, C> Children<T, C> {
    fn new() -> Self {
        Self { index: HashMap::new(), ordered: BTreeMap::new(), next_seq: 0 }
    }
}

/// `IResourceNode`/`Node`.
pub struct Node<T, C> {
    uri: URI,
    relative_path: String,
    context: C,
    element: RefCell<Option<T>>,
    parent: Option<Weak<Node<T, C>>>,
    children: RefCell<Children<T, C>>,
    name: Memoize<String>,
}

pub type NodeRef<T, C> = Rc<Node<T, C>>;

impl<T: Clone, C: Clone> Node<T, C> {
    fn new(uri: URI, relative_path: String, context: C, element: Option<T>, parent: Option<&NodeRef<T, C>>) -> NodeRef<T, C> {
        Rc::new(Self {
            uri,
            relative_path,
            context,
            element: RefCell::new(element),
            parent: parent.map(Rc::downgrade),
            children: RefCell::new(Children::new()),
            name: Memoize::new(),
        })
    }

    pub fn uri(&self) -> &URI {
        &self.uri
    }

    pub fn relative_path(&self) -> &str {
        &self.relative_path
    }

    /// `name`: o último segmento de `relativePath`.
    pub fn name(&self) -> &str {
        self.name.get(|| PathModule::posix(&ProcessEnvironment).basename(&self.relative_path, None))
    }

    pub fn context(&self) -> &C {
        &self.context
    }

    pub fn element(&self) -> Option<T> {
        self.element.borrow().clone()
    }

    pub fn parent(&self) -> Option<NodeRef<T, C>> {
        self.parent.as_ref().and_then(Weak::upgrade)
    }

    pub fn children_count(&self) -> usize {
        self.children.borrow().index.len()
    }

    /// `children`: os filhos na ordem de inserção.
    pub fn children(&self) -> Vec<NodeRef<T, C>> {
        self.children.borrow().ordered.values().map(|(_, node)| node.clone()).collect()
    }

    /// `get(childName)`.
    pub fn get(&self, child_name: &str) -> Option<NodeRef<T, C>> {
        let children = self.children.borrow();
        let seq = children.index.get(child_name)?;
        children.ordered.get(seq).map(|(_, node)| node.clone())
    }

    fn set(&self, path: &str, child: NodeRef<T, C>) {
        let mut children = self.children.borrow_mut();
        if let Some(&seq) = children.index.get(path) {
            // `Map.set` de chave existente mantém a posição.
            children.ordered.insert(seq, (path.to_string(), child));
            return;
        }
        let seq = children.next_seq;
        children.next_seq += 1;
        children.index.insert(path.to_string(), seq);
        children.ordered.insert(seq, (path.to_string(), child));
    }

    fn delete(&self, path: &str) {
        let mut children = self.children.borrow_mut();
        if let Some(seq) = children.index.remove(path) {
            children.ordered.remove(&seq);
        }
    }

    fn clear(&self) {
        *self.children.borrow_mut() = Children::new();
    }
}

fn collect_into<T: Clone, C: Clone>(node: &NodeRef<T, C>, result: &mut Vec<T>) {
    if let Some(element) = node.element() {
        result.push(element);
    }
    for child in node.children() {
        collect_into(&child, result);
    }
}

/// `ResourceTree`.
pub struct ResourceTree<T: Clone, C: Clone> {
    pub root: NodeRef<T, C>,
    ext_uri: ExtUri,
}

impl<T: Clone, C: Clone> ResourceTree<T, C> {
    /// `ResourceTree.getRoot`.
    pub fn get_root(node: &NodeRef<T, C>) -> NodeRef<T, C> {
        let mut node = node.clone();
        while let Some(parent) = node.parent() {
            node = parent;
        }
        node
    }

    /// `ResourceTree.collect`: os elementos do nó e dos descendentes, em pré-ordem.
    pub fn collect(node: &NodeRef<T, C>) -> Vec<T> {
        let mut result = Vec::new();
        collect_into(node, &mut result);
        result
    }

    /// `new ResourceTree(context, rootURI, extUri)`.
    pub fn new(context: C, root_uri: URI, ext_uri: ExtUri) -> Self {
        Self { root: Node::new(root_uri, String::new(), context, None, None), ext_uri }
    }

    /// `new ResourceTree(context)`: raiz `URI.file('/')` e o `extUri` do ambiente do processo.
    pub fn with_defaults(context: C) -> Self {
        let env = Rc::new(ProcessEnvironment);
        let platform = crate::path::Environment::platform(&*env);
        Self::new(context, URI::file("/", platform), ExtUri::unbiased(env))
    }

    fn key_iterator(&self, uri: &URI) -> PathIterator {
        let key = self
            .ext_uri
            .relative_path(&self.root.uri, uri)
            .filter(|key| !key.is_empty())
            .unwrap_or_else(|| uri.path().to_string_lossy());
        let mut iterator = PathIterator::new(false, true);
        iterator.reset(&key);
        iterator
    }

    /// `add`.
    pub fn add(&self, uri: &URI, element: T) {
        let mut iterator = self.key_iterator(uri);
        let mut node = self.root.clone();
        let mut path = String::new();

        loop {
            let name = iterator.value().to_string_lossy();
            path = format!("{path}/{name}");

            let child = match node.get(&name) {
                None => {
                    let child = Node::new(
                        self.ext_uri.join_path(&self.root.uri, &[&path]),
                        path.clone(),
                        self.root.context.clone(),
                        if iterator.has_next() { None } else { Some(element.clone()) },
                        Some(&node),
                    );
                    node.set(&name, child.clone());
                    child
                }
                Some(child) => {
                    if !iterator.has_next() {
                        *child.element.borrow_mut() = Some(element.clone());
                    }
                    child
                }
            };
            node = child;

            if !iterator.has_next() {
                return;
            }
            iterator.next();
        }
    }

    /// `delete`: remove o recurso (e as pastas que ficarem vazias) e devolve o elemento removido.
    pub fn delete(&self, uri: &URI) -> Option<T> {
        let mut iterator = self.key_iterator(uri);
        Self::delete_from(&self.root, &mut iterator)
    }

    fn delete_from(node: &NodeRef<T, C>, iterator: &mut PathIterator) -> Option<T> {
        let name = iterator.value().to_string_lossy();
        let child = node.get(&name)?;

        if iterator.has_next() {
            iterator.next();
            let result = Self::delete_from(&child, iterator);
            if result.is_some() && child.children_count() == 0 {
                node.delete(&name);
            }
            return result;
        }

        node.delete(&name);
        child.element()
    }

    /// `clear`.
    pub fn clear(&self) {
        self.root.clear();
    }

    /// `getNode`.
    pub fn get_node(&self, uri: &URI) -> Option<NodeRef<T, C>> {
        let mut iterator = self.key_iterator(uri);
        let mut node = self.root.clone();

        loop {
            let name = iterator.value().to_string_lossy();
            let child = node.get(&name);
            match child {
                Some(child) if iterator.has_next() => {
                    node = child;
                    iterator.next();
                }
                other => return other,
            }
        }
    }
}
