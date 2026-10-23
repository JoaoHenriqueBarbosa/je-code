//! Porte de `src/vs/base/common/linkedList.ts`.
//!
//! Lista duplamente encadeada com remoção O(1) por um [`Removal`] devolvido em `push`/`unshift`. Os nós
//! são `Rc<RefCell<_>>` (`next` forte, `prev` fraco), como o grafo de objetos do original, e a lista
//! em si é um `Rc` para o fecho de remoção poder alcançá-la.
//!
//! Escolhas e divergências:
//! - `shift` e `pop` TIRAM o elemento do nó (não exigem `E: Clone`); `peek` e a iteração clonam.
//!   Um iterador que atravessa um nó já removido por `shift`/`pop` (ponteiro `next` antigo, como no
//!   original) pula esse nó em vez de devolver o elemento, que já saiu.
//! - O [`Removal`] de `push`/`unshift` não faz nada se o nó já saiu da lista por outro caminho
//!   (`shift`, `pop`, `clear`). No original isso desfaz a contagem (`size` fica negativo), um defeito
//!   que o porte não reproduz.
//! - A iteração é preguiçosa, como o gerador do original: lê o `next` do último nó só quando
//!   pedida a próxima, então enxerga o que for inserido no fim durante a iteração.
//! - Além do `Iterator` por clonagem ([`LinkedList::iter`]), há [`LinkedList::for_each_ref`] para
//!   elementos que não são `Clone`.

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

struct Node<E> {
    element: Option<E>,
    next: Option<NodeRef<E>>,
    prev: Option<Weak<RefCell<Node<E>>>>,
    alive: bool,
}

type NodeRef<E> = Rc<RefCell<Node<E>>>;

struct Inner<E> {
    first: Option<NodeRef<E>>,
    last: Option<NodeRef<E>>,
    size: usize,
}

impl<E> Drop for Inner<E> {
    /// Desfaz a cadeia em laço para uma lista longa não estourar a pilha na destruição recursiva.
    fn drop(&mut self) {
        self.last = None;
        let mut current = self.first.take();
        while let Some(node) = current {
            current = node.borrow_mut().next.take();
        }
    }
}

impl<E> Inner<E> {
    fn remove(&mut self, node: &NodeRef<E>) {
        if !node.borrow().alive {
            return;
        }
        node.borrow_mut().alive = false;
        let (prev, next) = {
            let n = node.borrow();
            (n.prev.as_ref().and_then(Weak::upgrade), n.next.clone())
        };
        match (prev, next) {
            (Some(anchor), Some(next)) => {
                // meio
                next.borrow_mut().prev = Some(Rc::downgrade(&anchor));
                anchor.borrow_mut().next = Some(next);
            }
            (None, None) => {
                // único nó
                self.first = None;
                self.last = None;
            }
            (Some(prev), None) => {
                // último
                prev.borrow_mut().next = None;
                self.last = Some(prev);
            }
            (None, Some(next)) => {
                // primeiro
                next.borrow_mut().prev = None;
                self.first = Some(next);
            }
        }
        self.size -= 1;
    }
}

pub struct LinkedList<E> {
    inner: Rc<RefCell<Inner<E>>>,
}

impl<E: 'static> Default for LinkedList<E> {
    fn default() -> Self {
        Self::new()
    }
}

impl<E: 'static> LinkedList<E> {
    pub fn new() -> Self {
        Self { inner: Rc::new(RefCell::new(Inner { first: None, last: None, size: 0 })) }
    }

    pub fn size(&self) -> usize {
        self.inner.borrow().size
    }

    pub fn is_empty(&self) -> bool {
        self.inner.borrow().first.is_none()
    }

    pub fn clear(&self) {
        let mut inner = self.inner.borrow_mut();
        let mut node = inner.first.take();
        while let Some(n) = node {
            let next = {
                let mut b = n.borrow_mut();
                b.prev = None;
                b.alive = false;
                b.next.take()
            };
            node = next;
        }
        inner.last = None;
        inner.size = 0;
    }

    /// Insere no início. Devolve o [`Removal`] que remove o elemento (idempotente).
    pub fn unshift(&self, element: E) -> Removal<E> {
        self.insert(element, false)
    }

    /// Insere no fim. Devolve o [`Removal`] que remove o elemento (idempotente).
    pub fn push(&self, element: E) -> Removal<E> {
        self.insert(element, true)
    }

    fn insert(&self, element: E, at_the_end: bool) -> Removal<E> {
        let new_node: NodeRef<E> = Rc::new(RefCell::new(Node { element: Some(element), next: None, prev: None, alive: true }));
        {
            let mut inner = self.inner.borrow_mut();
            match (inner.first.clone(), inner.last.clone()) {
                (Some(first), Some(last)) => {
                    if at_the_end {
                        new_node.borrow_mut().prev = Some(Rc::downgrade(&last));
                        last.borrow_mut().next = Some(new_node.clone());
                        inner.last = Some(new_node.clone());
                    } else {
                        new_node.borrow_mut().next = Some(first.clone());
                        first.borrow_mut().prev = Some(Rc::downgrade(&new_node));
                        inner.first = Some(new_node.clone());
                    }
                }
                _ => {
                    inner.first = Some(new_node.clone());
                    inner.last = Some(new_node.clone());
                }
            }
            inner.size += 1;
        }
        Removal { list: self.inner.clone(), node: new_node, did_remove: Cell::new(false) }
    }

    /// Remove e devolve o primeiro elemento.
    pub fn shift(&self) -> Option<E> {
        let mut inner = self.inner.borrow_mut();
        let node = inner.first.clone()?;
        let element = node.borrow_mut().element.take();
        inner.remove(&node);
        element
    }

    /// Remove e devolve o último elemento.
    pub fn pop(&self) -> Option<E> {
        let mut inner = self.inner.borrow_mut();
        let node = inner.last.clone()?;
        let element = node.borrow_mut().element.take();
        inner.remove(&node);
        element
    }

    /// O ÚLTIMO elemento, sem remover (como o `peek` do original, que olha `_last`).
    pub fn peek(&self) -> Option<E>
    where
        E: Clone,
    {
        self.inner.borrow().last.as_ref().and_then(|n| n.borrow().element.clone())
    }

    /// Visita os elementos em ordem sem clonar. A lista não pode ser alterada pelo callback.
    pub fn for_each_ref(&self, mut f: impl FnMut(&E)) {
        let mut node = self.inner.borrow().first.clone();
        while let Some(n) = node {
            if let Some(element) = n.borrow().element.as_ref() {
                f(element);
            }
            node = n.borrow().next.clone();
        }
    }

    /// Iterador preguiçoso que clona cada elemento (o `[Symbol.iterator]` do original).
    pub fn iter(&self) -> Iter<E>
    where
        E: Clone,
    {
        Iter { inner: self.inner.clone(), started: false, current: None }
    }
}

/// O `() => void` que `push`/`unshift` devolvem: tira o elemento da lista, uma vez só. Descartar o
/// valor sem chamar [`Removal::remove`] deixa o elemento na lista (como ignorar o retorno no TS).
pub struct Removal<E> {
    list: Rc<RefCell<Inner<E>>>,
    node: NodeRef<E>,
    did_remove: Cell<bool>,
}

impl<E> Removal<E> {
    pub fn remove(&self) {
        if !self.did_remove.replace(true) {
            self.list.borrow_mut().remove(&self.node);
        }
    }
}

pub struct Iter<E> {
    inner: Rc<RefCell<Inner<E>>>,
    started: bool,
    current: Option<NodeRef<E>>,
}

impl<E: Clone> Iterator for Iter<E> {
    type Item = E;

    fn next(&mut self) -> Option<E> {
        loop {
            let node = if self.started {
                self.current.as_ref().and_then(|n| n.borrow().next.clone())
            } else {
                self.started = true;
                self.inner.borrow().first.clone()
            };
            self.current = node.clone();
            let node = node?;
            let element = node.borrow().element.clone();
            if element.is_some() {
                return element;
            }
        }
    }
}
