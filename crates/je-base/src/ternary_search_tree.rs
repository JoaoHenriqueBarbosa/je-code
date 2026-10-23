//! Porte de `src/vs/base/common/ternarySearchTree.ts`: árvore de busca ternária com balanceamento AVL,
//! usada para achar o prefixo mais longo de caminhos, URIs e chaves de configuração.
//!
//! Os nós moram numa arena (`Vec` com lista de livres) e se referenciam por índice, o que permite portar
//! literalmente a pilha de `(direção, nó)` do original, inclusive as rotações que trocam o nó da pilha.
//!
//! Diferenças de semântica em relação ao original, todas em valores "falsy" do JavaScript:
//! - `findSubstr` usa `valor || candidato`: aqui um valor presente (`Some`) sempre vence, mesmo `0`, `''`
//!   ou `false`.
//! - A poda de `delete` testa `!node.value` e `min.key`: aqui o teste é de presença, não de verdade.
//! - O `set` devolve `Option<V>` com o valor antigo: se `V` já é `Option<_>`, "valor `undefined`
//!   gravado" aparece como `Some(None)` e "chave nova" como `None`, o que o JavaScript não distingue.

use std::cell::RefCell;
use std::rc::Rc;

use crate::errors::{self, Error};
use crate::arrays;
use crate::assert::assert;
use crate::strings::{compare, compare_ignore_case, compare_substring, compare_substring_ignore_case};
use crate::uri::URI;
use crate::{char_code as cc, JsString};

/// `IKeyIterator<K>`: percorre a chave em segmentos.
pub trait KeyIterator<K> {
    fn reset(&mut self, key: &K);
    fn next(&mut self);
    fn has_next(&self) -> bool;
    /// Compara o segmento `a` (de um nó) com o segmento atual da chave.
    fn cmp(&self, a: &[u16]) -> i32;
    fn value(&self) -> JsString;
}

fn units(s: &str) -> Vec<u16> {
    s.encode_utf16().collect()
}

#[derive(Default)]
pub struct StringIterator {
    value: Vec<u16>,
    pos: usize,
}

impl StringIterator {
    pub fn new() -> Self {
        Self::default()
    }
}

impl KeyIterator<String> for StringIterator {
    fn reset(&mut self, key: &String) {
        self.value = units(key);
        self.pos = 0;
    }

    fn next(&mut self) {
        self.pos += 1;
    }

    fn has_next(&self) -> bool {
        self.pos + 1 < self.value.len()
    }

    fn cmp(&self, a: &[u16]) -> i32 {
        // `a.charCodeAt(0) - this._value.charCodeAt(this._pos)`: `NaN` (sem unidade) se comporta como 0.
        match (a.first(), self.value.get(self.pos)) {
            (Some(&a_code), Some(&this_code)) => i32::from(a_code) - i32::from(this_code),
            _ => 0,
        }
    }

    fn value(&self) -> JsString {
        JsString::from_units(self.value.get(self.pos).map(|&u| vec![u]).unwrap_or_default())
    }
}

pub struct ConfigKeysIterator {
    value: Vec<u16>,
    from: usize,
    to: usize,
    case_sensitive: bool,
}

impl ConfigKeysIterator {
    pub fn new(case_sensitive: bool) -> Self {
        Self { value: Vec::new(), from: 0, to: 0, case_sensitive }
    }
}

impl Default for ConfigKeysIterator {
    fn default() -> Self {
        Self::new(true)
    }
}

impl KeyIterator<String> for ConfigKeysIterator {
    fn reset(&mut self, key: &String) {
        self.value = units(key);
        self.from = 0;
        self.to = 0;
        self.next();
    }

    fn has_next(&self) -> bool {
        self.to < self.value.len()
    }

    fn next(&mut self) {
        self.from = self.to;
        let mut just_seps = true;
        while self.to < self.value.len() {
            if self.value[self.to] == cc::Period {
                if just_seps {
                    self.from += 1;
                } else {
                    break;
                }
            } else {
                just_seps = false;
            }
            self.to += 1;
        }
    }

    fn cmp(&self, a: &[u16]) -> i32 {
        if self.case_sensitive {
            compare_substring(a, &self.value, 0, a.len(), self.from, self.to)
        } else {
            compare_substring_ignore_case(a, &self.value, 0, a.len(), self.from, self.to)
        }
    }

    fn value(&self) -> JsString {
        JsString::from(&self.value[self.from.min(self.to)..self.to])
    }
}

pub struct PathIterator {
    value: Vec<u16>,
    value_len: usize,
    from: usize,
    to: usize,
    split_on_backslash: bool,
    case_sensitive: bool,
}

impl PathIterator {
    pub fn new(split_on_backslash: bool, case_sensitive: bool) -> Self {
        Self { value: Vec::new(), value_len: 0, from: 0, to: 0, split_on_backslash, case_sensitive }
    }

    fn is_separator(&self, unit: u16) -> bool {
        unit == cc::Slash || (self.split_on_backslash && unit == cc::Backslash)
    }
}

impl Default for PathIterator {
    fn default() -> Self {
        Self::new(true, true)
    }
}

impl KeyIterator<String> for PathIterator {
    fn reset(&mut self, key: &String) {
        self.from = 0;
        self.to = 0;
        self.value = units(key);
        self.reset_len();
        self.next();
    }

    fn has_next(&self) -> bool {
        self.to < self.value_len
    }

    fn next(&mut self) {
        self.from = self.to;
        let mut just_seps = true;
        while self.to < self.value_len {
            if self.is_separator(self.value[self.to]) {
                if just_seps {
                    self.from += 1;
                } else {
                    break;
                }
            } else {
                just_seps = false;
            }
            self.to += 1;
        }
    }

    fn cmp(&self, a: &[u16]) -> i32 {
        if self.case_sensitive {
            compare_substring(a, &self.value, 0, a.len(), self.from, self.to)
        } else {
            compare_substring_ignore_case(a, &self.value, 0, a.len(), self.from, self.to)
        }
    }

    fn value(&self) -> JsString {
        JsString::from(&self.value[self.from.min(self.to)..self.to])
    }
}

impl PathIterator {
    /// O comprimento sem as barras finais.
    fn reset_len(&mut self) {
        self.value_len = self.value.len();
        while self.value_len > 0 && self.is_separator(self.value[self.value_len - 1]) {
            self.value_len -= 1;
        }
    }

    /// `reset` sobre unidades já prontas (o caminho de um URI), sem passar por `String`.
    fn reset_units(&mut self, key: &[u16]) {
        self.from = 0;
        self.to = 0;
        self.value = key.to_vec();
        self.reset_len();
        self.next();
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum UriIteratorState {
    Scheme = 1,
    Authority = 2,
    Path = 3,
    Query = 4,
    Fragment = 5,
}

pub type UriPredicate = Rc<dyn Fn(&URI) -> bool>;

pub struct UriIterator {
    ignore_path_casing: UriPredicate,
    ignore_query_and_fragment: UriPredicate,
    path_iterator: PathIterator,
    value: Option<URI>,
    states: Vec<UriIteratorState>,
    state_idx: usize,
}

impl UriIterator {
    pub fn new(ignore_path_casing: UriPredicate, ignore_query_and_fragment: UriPredicate) -> Self {
        Self {
            ignore_path_casing,
            ignore_query_and_fragment,
            path_iterator: PathIterator::new(true, true),
            value: None,
            states: Vec::new(),
            state_idx: 0,
        }
    }

    fn uri(&self) -> &URI {
        self.value.as_ref().expect("UriIterator sem reset")
    }

    fn state(&self) -> Option<UriIteratorState> {
        self.states.get(self.state_idx).copied()
    }
}

impl KeyIterator<URI> for UriIterator {
    fn reset(&mut self, key: &URI) {
        self.value = Some(key.clone());
        self.states = Vec::new();
        if !key.scheme().is_empty() {
            self.states.push(UriIteratorState::Scheme);
        }
        if !key.authority().is_empty() {
            self.states.push(UriIteratorState::Authority);
        }
        if !key.path().is_empty() {
            self.path_iterator = PathIterator::new(false, !(self.ignore_path_casing)(key));
            self.path_iterator.reset_units(key.path().units());
            if !self.path_iterator.value().is_empty() {
                self.states.push(UriIteratorState::Path);
            }
        }
        if !(self.ignore_query_and_fragment)(key) {
            if !key.query().is_empty() {
                self.states.push(UriIteratorState::Query);
            }
            if !key.fragment().is_empty() {
                self.states.push(UriIteratorState::Fragment);
            }
        }
        self.state_idx = 0;
    }

    fn next(&mut self) {
        if self.state() == Some(UriIteratorState::Path) && self.path_iterator.has_next() {
            self.path_iterator.next();
        } else {
            self.state_idx += 1;
        }
    }

    fn has_next(&self) -> bool {
        (self.state() == Some(UriIteratorState::Path) && self.path_iterator.has_next())
            || self.state_idx + 1 < self.states.len()
    }

    fn cmp(&self, a: &[u16]) -> i32 {
        match self.state() {
            Some(UriIteratorState::Scheme) => compare_ignore_case(a, self.uri().scheme().units()),
            Some(UriIteratorState::Authority) => compare_ignore_case(a, self.uri().authority().units()),
            Some(UriIteratorState::Path) => self.path_iterator.cmp(a),
            Some(UriIteratorState::Query) => compare(a, self.uri().query().units()),
            Some(UriIteratorState::Fragment) => compare(a, self.uri().fragment().units()),
            None => errors::throw(Error::new("")),
        }
    }

    fn value(&self) -> JsString {
        match self.state() {
            Some(UriIteratorState::Scheme) => self.uri().scheme().clone(),
            Some(UriIteratorState::Authority) => self.uri().authority().clone(),
            Some(UriIteratorState::Path) => self.path_iterator.value(),
            Some(UriIteratorState::Query) => self.uri().query().clone(),
            Some(UriIteratorState::Fragment) => self.uri().fragment().clone(),
            None => errors::throw(Error::new("")),
        }
    }
}

// ---- árvore ----------------------------------------------------------------------------------------

struct Node<K, V> {
    height: i32,
    segment: JsString,
    value: Option<V>,
    key: Option<K>,
    left: Option<usize>,
    mid: Option<usize>,
    right: Option<usize>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Dir {
    Left,
    Mid,
    Right,
}

type Stack = Vec<(Dir, usize)>;

struct Arena<K, V> {
    nodes: Vec<Option<Node<K, V>>>,
    free: Vec<usize>,
}

impl<K, V> Arena<K, V> {
    fn new_node(&mut self, segment: JsString) -> usize {
        let node = Node { height: 1, segment, value: None, key: None, left: None, mid: None, right: None };
        match self.free.pop() {
            Some(index) => {
                self.nodes[index] = Some(node);
                index
            }
            None => {
                self.nodes.push(Some(node));
                self.nodes.len() - 1
            }
        }
    }

    fn get(&self, index: usize) -> &Node<K, V> {
        self.nodes[index].as_ref().expect("nó vivo")
    }

    fn get_mut(&mut self, index: usize) -> &mut Node<K, V> {
        self.nodes[index].as_mut().expect("nó vivo")
    }

    fn release(&mut self, index: usize) {
        self.nodes[index] = None;
        self.free.push(index);
    }

    /// Libera `index` e tudo que pende dele.
    fn release_subtree(&mut self, index: usize) {
        let mut pending = vec![index];
        while let Some(current) = pending.pop() {
            let node = self.nodes[current].as_ref().expect("nó vivo");
            pending.extend([node.left, node.mid, node.right].into_iter().flatten());
            self.release(current);
        }
    }

    fn height_of(&self, index: Option<usize>) -> i32 {
        index.map_or(0, |i| self.get(i).height)
    }

    fn update_height(&mut self, index: usize) {
        let node = self.get(index);
        let height = 1 + self.height_of(node.left).max(self.height_of(node.right));
        self.get_mut(index).height = height;
    }

    fn balance_factor(&self, index: usize) -> i32 {
        let node = self.get(index);
        self.height_of(node.right) - self.height_of(node.left)
    }

    fn rotate_left(&mut self, index: usize) -> usize {
        let tmp = self.get(index).right.expect("rotateLeft sem filho direito");
        self.get_mut(index).right = self.get(tmp).left;
        self.get_mut(tmp).left = Some(index);
        self.update_height(index);
        self.update_height(tmp);
        tmp
    }

    fn rotate_right(&mut self, index: usize) -> usize {
        let tmp = self.get(index).left.expect("rotateRight sem filho esquerdo");
        self.get_mut(index).left = self.get(tmp).right;
        self.get_mut(tmp).right = Some(index);
        self.update_height(index);
        self.update_height(tmp);
        tmp
    }

    /// Aponta o ramo `dir` do nó `parent` para `child`.
    fn set_child(&mut self, parent: usize, dir: Dir, child: Option<usize>) {
        let node = self.get_mut(parent);
        match dir {
            Dir::Left => node.left = child,
            Dir::Mid => node.mid = child,
            Dir::Right => node.right = child,
        }
    }

    /// `_balanceByStack`: devolve a nova raiz do ramo (o `stack[0]`), se a pilha não é vazia.
    fn balance_by_stack(&mut self, stack: &mut Stack) -> Option<usize> {
        for i in (0..stack.len()).rev() {
            let node = stack[i].1;

            self.update_height(node);
            let bf = self.balance_factor(node);
            if bf > 1 {
                // right heavy
                let right = self.get(node).right.expect("peso à direita sem filho");
                if self.balance_factor(right) >= 0 {
                    // right, right -> rotate left
                    stack[i].1 = self.rotate_left(node);
                } else {
                    // right, left -> double rotate
                    let rotated = self.rotate_right(right);
                    self.get_mut(node).right = Some(rotated);
                    stack[i].1 = self.rotate_left(node);
                }
            } else if bf < -1 {
                // left heavy
                let left = self.get(node).left.expect("peso à esquerda sem filho");
                if self.balance_factor(left) <= 0 {
                    // left, left -> rotate right
                    stack[i].1 = self.rotate_right(node);
                } else {
                    // left, right -> double rotate
                    let rotated = self.rotate_left(left);
                    self.get_mut(node).left = Some(rotated);
                    stack[i].1 = self.rotate_right(node);
                }
            }

            // patch path to parent
            if i > 0 {
                let (dir, parent) = stack[i - 1];
                self.set_child(parent, dir, Some(stack[i].1));
            } else {
                return Some(stack[0].1);
            }
        }

        None
    }

    fn min(&self, mut node: usize, stack: &mut Stack) -> usize {
        while let Some(left) = self.get(node).left {
            stack.push((Dir::Left, node));
            node = left;
        }
        node
    }
}

pub struct TernarySearchTree<K, V> {
    iter: RefCell<Box<dyn KeyIterator<K>>>,
    arena: Arena<K, V>,
    root: Option<usize>,
}

impl<V> TernarySearchTree<URI, V> {
    pub fn for_uris(ignore_path_casing: UriPredicate, ignore_query_and_fragment: UriPredicate) -> Self {
        Self::new(Box::new(UriIterator::new(ignore_path_casing, ignore_query_and_fragment)))
    }
}

impl<V> TernarySearchTree<String, V> {
    pub fn for_paths(ignore_path_casing: bool) -> Self {
        Self::new(Box::new(PathIterator::new(true, !ignore_path_casing)))
    }

    pub fn for_strings() -> Self {
        Self::new(Box::new(StringIterator::new()))
    }

    pub fn for_config_keys() -> Self {
        Self::new(Box::new(ConfigKeysIterator::new(true)))
    }
}

impl<K, V> TernarySearchTree<K, V> {
    pub fn new(segments: Box<dyn KeyIterator<K>>) -> Self {
        Self { iter: RefCell::new(segments), arena: Arena { nodes: Vec::new(), free: Vec::new() }, root: None }
    }

    pub fn clear(&mut self) {
        self.root = None;
        self.arena.nodes.clear();
        self.arena.free.clear();
    }

    /// `set`: devolve o elemento anterior da chave, se havia.
    pub fn set(&mut self, key: K, element: V) -> Option<V> {
        let mut iter = self.iter.borrow_mut();
        iter.reset(&key);
        let arena = &mut self.arena;

        let root = match self.root {
            Some(root) => root,
            None => {
                let root = arena.new_node(iter.value());
                self.root = Some(root);
                root
            }
        };
        let mut stack: Stack = Vec::new();

        // find insert_node
        let mut node = root;
        loop {
            let val = iter.cmp(arena.get(node).segment.units());
            if val > 0 {
                // left
                let child = match arena.get(node).left {
                    Some(child) => child,
                    None => {
                        let child = arena.new_node(iter.value());
                        arena.get_mut(node).left = Some(child);
                        child
                    }
                };
                stack.push((Dir::Left, node));
                node = child;
            } else if val < 0 {
                // right
                let child = match arena.get(node).right {
                    Some(child) => child,
                    None => {
                        let child = arena.new_node(iter.value());
                        arena.get_mut(node).right = Some(child);
                        child
                    }
                };
                stack.push((Dir::Right, node));
                node = child;
            } else if iter.has_next() {
                // mid
                iter.next();
                let child = match arena.get(node).mid {
                    Some(child) => child,
                    None => {
                        let child = arena.new_node(iter.value());
                        arena.get_mut(node).mid = Some(child);
                        child
                    }
                };
                stack.push((Dir::Mid, node));
                node = child;
            } else {
                break;
            }
        }

        // set value
        let old_element = arena.get_mut(node).value.replace(element);
        arena.get_mut(node).key = Some(key);

        // balance
        for i in (0..stack.len()).rev() {
            let node = stack[i].1;

            arena.update_height(node);
            let bf = arena.balance_factor(node);

            if !(-1..=1).contains(&bf) {
                // needs rotate
                let d1 = stack[i].0;
                let d2 = stack.get(i + 1).map(|entry| entry.0);

                match (d1, d2) {
                    (Dir::Right, Some(Dir::Right)) => {
                        // right, right -> rotate left
                        stack[i].1 = arena.rotate_left(node);
                    }
                    (Dir::Left, Some(Dir::Left)) => {
                        // left, left -> rotate right
                        stack[i].1 = arena.rotate_right(node);
                    }
                    (Dir::Right, Some(Dir::Left)) => {
                        // right, left -> double rotate right, left
                        let rotated = arena.rotate_right(stack[i + 1].1);
                        stack[i + 1].1 = rotated;
                        arena.get_mut(node).right = Some(rotated);
                        stack[i].1 = arena.rotate_left(node);
                    }
                    (Dir::Left, Some(Dir::Right)) => {
                        // left, right -> double rotate left, right
                        let rotated = arena.rotate_left(stack[i + 1].1);
                        stack[i + 1].1 = rotated;
                        arena.get_mut(node).left = Some(rotated);
                        stack[i].1 = arena.rotate_right(node);
                    }
                    _ => errors::throw(Error::new("")),
                }

                // patch path to parent
                if i > 0 {
                    let (dir, parent) = stack[i - 1];
                    arena.set_child(parent, dir, Some(stack[i].1));
                } else {
                    self.root = Some(stack[0].1);
                }
            }
        }

        old_element
    }

    pub fn get(&self, key: &K) -> Option<&V> {
        self.get_node(key).and_then(|node| self.arena.get(node).value.as_ref())
    }

    fn get_node(&self, key: &K) -> Option<usize> {
        let mut iter = self.iter.borrow_mut();
        iter.reset(key);
        let mut node = self.root;
        while let Some(current) = node {
            let val = iter.cmp(self.arena.get(current).segment.units());
            if val > 0 {
                // left
                node = self.arena.get(current).left;
            } else if val < 0 {
                // right
                node = self.arena.get(current).right;
            } else if iter.has_next() {
                // mid
                iter.next();
                node = self.arena.get(current).mid;
            } else {
                break;
            }
        }
        node
    }

    pub fn has(&self, key: &K) -> bool {
        match self.get_node(key) {
            Some(node) => {
                let node = self.arena.get(node);
                !(node.value.is_none() && node.mid.is_none())
            }
            None => false,
        }
    }

    pub fn delete(&mut self, key: &K) {
        self.delete_impl(key, false);
    }

    pub fn delete_superstr(&mut self, key: &K) {
        self.delete_impl(key, true);
    }

    fn delete_impl(&mut self, key: &K, super_str: bool) {
        let mut iter = self.iter.borrow_mut();
        iter.reset(key);
        let arena = &mut self.arena;
        let mut stack: Stack = Vec::new();
        let mut node = self.root;

        // find node
        while let Some(current) = node {
            let val = iter.cmp(arena.get(current).segment.units());
            if val > 0 {
                // left
                stack.push((Dir::Left, current));
                node = arena.get(current).left;
            } else if val < 0 {
                // right
                stack.push((Dir::Right, current));
                node = arena.get(current).right;
            } else if iter.has_next() {
                // mid
                iter.next();
                stack.push((Dir::Mid, current));
                node = arena.get(current).mid;
            } else {
                break;
            }
        }

        let Some(node) = node else {
            // node not found
            return;
        };

        if super_str {
            // removing children, reset height
            let n = arena.get_mut(node);
            let children = [n.left.take(), n.mid.take(), n.right.take()];
            n.height = 1;
            for child in children.into_iter().flatten() {
                arena.release_subtree(child);
            }
        } else {
            // removing element
            let n = arena.get_mut(node);
            n.key = None;
            n.value = None;
        }

        // BST node removal
        let (has_mid, has_value) = (arena.get(node).mid.is_some(), arena.get(node).value.is_some());
        if !has_mid && !has_value {
            let (left, right) = (arena.get(node).left, arena.get(node).right);
            if let (Some(_), Some(right)) = (left, right) {
                // full node
                // replace deleted-node with the min-node of the right branch.
                // If there is no true min-node leave things as they are
                let mut stack2: Stack = vec![(Dir::Right, node)];
                let min = arena.min(right, &mut stack2);

                if arena.get(min).key.is_some() {
                    let min_node = arena.get_mut(min);
                    let (min_key, min_value, min_segment, new_child) =
                        (min_node.key.take(), min_node.value.take(), std::mem::take(&mut min_node.segment), min_node.right);
                    let n = arena.get_mut(node);
                    n.key = min_key;
                    n.value = min_value;
                    n.segment = min_segment;

                    // remove NODE (inorder successor can only have right child)
                    if stack2.len() > 1 {
                        let (dir, parent) = stack2[stack2.len() - 1];
                        match dir {
                            Dir::Left => arena.get_mut(parent).left = new_child,
                            Dir::Mid | Dir::Right => assert(false, ()),
                        }
                    } else {
                        arena.get_mut(node).right = new_child;
                    }
                    arena.release(min);

                    // balance right branch and UPDATE parent pointer for stack
                    let new_child2 = arena.balance_by_stack(&mut stack2).expect("pilha não vazia");
                    if let Some(&(dir, parent)) = stack.last() {
                        arena.set_child(parent, dir, Some(new_child2));
                    } else {
                        self.root = Some(new_child2);
                    }
                }
            } else {
                // empty or half empty
                let new_child = left.or(right);
                if let Some(&(dir, parent)) = stack.last() {
                    arena.set_child(parent, dir, new_child);
                } else {
                    self.root = new_child;
                }
                arena.release(node);
            }
        }

        // AVL balance
        self.root = arena.balance_by_stack(&mut stack).or(self.root);
    }

    /// `findSubstr`: o valor do prefixo mais longo da chave presente na árvore.
    pub fn find_substr(&self, key: &K) -> Option<&V> {
        let mut iter = self.iter.borrow_mut();
        iter.reset(key);
        let mut node = self.root;
        let mut candidate: Option<&V> = None;
        while let Some(current) = node {
            let val = iter.cmp(self.arena.get(current).segment.units());
            if val > 0 {
                // left
                node = self.arena.get(current).left;
            } else if val < 0 {
                // right
                node = self.arena.get(current).right;
            } else if iter.has_next() {
                // mid
                iter.next();
                candidate = self.arena.get(current).value.as_ref().or(candidate);
                node = self.arena.get(current).mid;
            } else {
                break;
            }
        }
        node.and_then(|n| self.arena.get(n).value.as_ref()).or(candidate)
    }

    /// `_findSuperstrOrElement`: o nó exato da chave, se a chave termina num nó.
    fn find_exact_node(&self, key: &K) -> Option<usize> {
        let mut iter = self.iter.borrow_mut();
        iter.reset(key);
        let mut node = self.root;
        while let Some(current) = node {
            let val = iter.cmp(self.arena.get(current).segment.units());
            if val > 0 {
                node = self.arena.get(current).left;
            } else if val < 0 {
                node = self.arena.get(current).right;
            } else if iter.has_next() {
                iter.next();
                node = self.arena.get(current).mid;
            } else {
                return Some(current);
            }
        }
        None
    }

    pub fn has_element_or_subtree(&self, key: &K) -> bool {
        match self.find_exact_node(key) {
            Some(node) => {
                let node = self.arena.get(node);
                node.mid.is_some() || node.value.is_some()
            }
            None => false,
        }
    }

    /// `_isBalanced` (para depuração e testes).
    pub fn is_balanced(&self) -> bool {
        fn node_is_balanced<K, V>(arena: &Arena<K, V>, node: Option<usize>) -> bool {
            let Some(node) = node else {
                return true;
            };
            let bf = arena.balance_factor(node);
            if !(-1..=1).contains(&bf) {
                return false;
            }
            node_is_balanced(arena, arena.get(node).left) && node_is_balanced(arena, arena.get(node).right)
        }
        node_is_balanced(&self.arena, self.root)
    }
}

impl<K: Clone, V: Clone> TernarySearchTree<K, V> {
    /// Fill the tree with the same value of the given keys.
    pub fn fill_keys(&mut self, element: V, keys: &[K]) {
        let mut arr = keys.to_vec();
        arrays::shuffle(&mut arr, None);
        for k in arr {
            self.set(k, element.clone());
        }
    }

    /// Fill the tree with given [key,value]-tuples.
    pub fn fill(&mut self, values: &[(K, V)]) {
        let mut arr = values.to_vec();
        arrays::shuffle(&mut arr, None);
        for (key, value) in arr {
            self.set(key, value);
        }
    }

    /// `findSuperstr`: as entradas sob a chave (sem a própria chave), em ordem DFS.
    pub fn find_superstr(&self, key: &K) -> Option<std::vec::IntoIter<(K, V)>> {
        let node = self.find_exact_node(key)?;
        let mid = self.arena.get(node).mid?;
        Some(self.entries_of(Some(mid)).into_iter())
    }

    pub fn for_each(&self, mut callback: impl FnMut(&V, &K)) {
        for (key, value) in self.iter_entries() {
            callback(&value, &key);
        }
    }

    /// `[Symbol.iterator]`: todas as entradas, em ordem DFS.
    pub fn iter_entries(&self) -> std::vec::IntoIter<(K, V)> {
        self.entries_of(self.root).into_iter()
    }

    fn entries_of(&self, node: Option<usize>) -> Vec<(K, V)> {
        let mut bucket = Vec::new();
        self.dfs_entries(node, &mut bucket);
        bucket
    }

    fn dfs_entries(&self, node: Option<usize>, bucket: &mut Vec<(K, V)>) {
        // DFS
        let Some(node) = node else {
            return;
        };
        let n = self.arena.get(node);
        if n.left.is_some() {
            self.dfs_entries(n.left, bucket);
        }
        if let (Some(value), Some(key)) = (&n.value, &n.key) {
            bucket.push((key.clone(), value.clone()));
        }
        if n.mid.is_some() {
            self.dfs_entries(n.mid, bucket);
        }
        if n.right.is_some() {
            self.dfs_entries(n.right, bucket);
        }
    }
}
