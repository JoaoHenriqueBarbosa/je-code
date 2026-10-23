//! Porte de `src/vs/base/common/event.ts`, núcleo: `Event<T>`, `Emitter<T>`, `EmitterOptions` e a fila
//! de entrega compartilhada (`EventDeliveryQueue`).
//!
//! A semântica de reentrância é a do original: um `fire` chamado enquanto outra entrega da mesma fila
//! está no meio termina primeiro a entrega pendente; listener adicionado durante uma entrega não a
//! recebe; listener removido durante uma entrega deixa de recebê-la. Erro lançado por listener vai
//! para `onListenerError` ou, sem ele, para o `onUnexpectedError`, e a entrega segue.

use std::any::Any;
use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use crate::errors::{self, Error};
use crate::lifecycle::{self, DisposableRef, DisposableStore};

/// Um listener. `Rc` porque o mesmo listener pode ser compartilhado por combinadores.
pub type Listener<T> = Rc<dyn Fn(&T)>;

/// `Event<T>`: a função que inscreve um listener e devolve o disposable da inscrição.
#[derive(Clone)]
pub struct Event<T: 'static> {
    subscribe: Rc<dyn Fn(Listener<T>) -> DisposableRef>,
}

impl<T: 'static> Event<T> {
    pub fn new(subscribe: impl Fn(Listener<T>) -> DisposableRef + 'static) -> Self {
        Self { subscribe: Rc::new(subscribe) }
    }

    /// `event(listener)`.
    pub fn listen(&self, listener: impl Fn(&T) + 'static) -> DisposableRef {
        (self.subscribe)(Rc::new(listener))
    }

    /// `event(listener)` com um listener já em `Rc`.
    pub fn listen_rc(&self, listener: Listener<T>) -> DisposableRef {
        (self.subscribe)(listener)
    }

    /// `event(listener, undefined, store)`: a inscrição vai para o store.
    pub fn listen_in(&self, listener: impl Fn(&T) + 'static, store: &DisposableStore) -> DisposableRef {
        store.add(self.listen(listener))
    }

    /// `Event.None`.
    pub fn none() -> Self {
        Self::new(|_| lifecycle::none())
    }
}

/// `EmitterOptions`. Os ganchos recebem nada (o original passa o emitter, que nenhum chamador usa).
#[derive(Default, Clone)]
pub struct EmitterOptions {
    pub on_will_add_first_listener: Option<Rc<dyn Fn()>>,
    pub on_did_add_first_listener: Option<Rc<dyn Fn()>>,
    pub on_did_add_listener: Option<Rc<dyn Fn()>>,
    pub on_did_remove_last_listener: Option<Rc<dyn Fn()>>,
    pub on_will_remove_listener: Option<Rc<dyn Fn()>>,
    pub on_listener_error: Option<Rc<dyn Fn(&Error)>>,
    pub delivery_queue: Option<EventDeliveryQueue>,
}

/// O que a fila de entrega precisa de um emitter de qualquer tipo para terminar a entrega pendente.
trait PendingDelivery {
    fn deliver_pending(&self);
}

#[derive(Default)]
struct QueueState {
    i: usize,
    end: usize,
    current: Option<Weak<dyn PendingDelivery>>,
    current_id: usize,
    value: Option<Rc<dyn Any>>,
}

/// `EventDeliveryQueue` (`createEventDeliveryQueue`): pode ser compartilhada entre emitters para que
/// um `fire` de um termine primeiro a entrega pendente do outro.
#[derive(Clone, Default)]
pub struct EventDeliveryQueue(Rc<RefCell<QueueState>>);

impl EventDeliveryQueue {
    pub fn new() -> Self {
        Self::default()
    }

    fn reset(&self) {
        let mut q = self.0.borrow_mut();
        q.i = q.end;
        q.current = None;
        q.current_id = 0;
        q.value = None;
    }
}

struct Inner<T: 'static> {
    options: EmitterOptions,
    disposed: Cell<bool>,
    // Lista esparsa como no original: remoção vira `None` e a compactação só acontece quando mais
    // da metade está vazia (`compactionThreshold = 2`).
    listeners: RefCell<Vec<Option<Listener<T>>>>,
    size: Cell<usize>,
    queue: RefCell<Option<EventDeliveryQueue>>,
    me: RefCell<Weak<Inner<T>>>,
}

const COMPACTION_THRESHOLD: usize = 2;

impl<T: 'static> Inner<T> {
    fn id(&self) -> usize {
        std::ptr::from_ref(self) as usize
    }

    fn deliver(&self, listener: &Listener<T>, value: &T) {
        if let Err(e) = errors::catch(|| listener(value)) {
            match &self.options.on_listener_error {
                Some(handler) => handler(&e),
                None => errors::on_unexpected_error(&e),
            }
        }
    }

    fn deliver_queue(&self, dq: &EventDeliveryQueue) {
        let value = dq.0.borrow().value.clone();
        let Some(value) = value.and_then(|v| v.downcast::<T>().ok()) else {
            dq.reset();
            return;
        };
        loop {
            let listener = {
                let mut q = dq.0.borrow_mut();
                if q.i >= q.end || q.current_id != self.id() {
                    break;
                }
                // `i` avança antes da entrega porque ela pode reentrar aqui.
                let i = q.i;
                q.i += 1;
                self.listeners.borrow().get(i).cloned().flatten()
            };
            if let Some(listener) = listener {
                self.deliver(&listener, &value);
            }
        }
        if dq.0.borrow().current_id == self.id() {
            dq.reset();
        }
    }

    fn remove_listener(&self, listener: &Listener<T>) {
        if let Some(f) = &self.options.on_will_remove_listener {
            f();
        }
        if self.listeners.borrow().is_empty() || self.size.get() == 0 {
            return;
        }
        if self.size.get() == 1 {
            self.listeners.borrow_mut().clear();
            self.size.set(0);
            if let Some(f) = &self.options.on_did_remove_last_listener {
                f();
            }
            return;
        }
        let mut listeners = self.listeners.borrow_mut();
        let Some(index) = listeners.iter().position(|l| l.as_ref().is_some_and(|l| Rc::ptr_eq(l, listener))) else {
            drop(listeners);
            errors::throw(Error::new("Attempted to dispose unknown listener"));
        };
        self.size.set(self.size.get() - 1);
        listeners[index] = None;

        let queue = self.queue.borrow().clone();
        let adjust = queue.as_ref().is_some_and(|q| q.0.borrow().current_id == self.id());
        if self.size.get() * COMPACTION_THRESHOLD <= listeners.len() {
            let mut n = 0;
            for i in 0..listeners.len() {
                if listeners[i].is_some() {
                    listeners.swap(n, i);
                    n += 1;
                } else if adjust
                    && let Some(q) = &queue
                {
                    let mut q = q.0.borrow_mut();
                    if n < q.end {
                        q.end -= 1;
                        if n < q.i {
                            q.i -= 1;
                        }
                    }
                }
            }
            listeners.truncate(n);
        }
    }
}

impl<T: 'static> PendingDelivery for Inner<T> {
    fn deliver_pending(&self) {
        if let Some(queue) = self.queue.borrow().clone() {
            self.deliver_queue(&queue);
        }
    }
}

/// `Emitter<T>`.
pub struct Emitter<T: 'static> {
    inner: Rc<Inner<T>>,
}

impl<T: 'static> Default for Emitter<T> {
    fn default() -> Self {
        Self::with_options(EmitterOptions::default())
    }
}

impl<T: 'static> Emitter<T> {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_options(options: EmitterOptions) -> Self {
        let queue = options.delivery_queue.clone();
        let inner = Rc::new(Inner {
            options,
            disposed: Cell::new(false),
            listeners: RefCell::new(Vec::new()),
            size: Cell::new(0),
            queue: RefCell::new(queue),
            me: RefCell::new(Weak::new()),
        });
        *inner.me.borrow_mut() = Rc::downgrade(&inner);
        Self { inner }
    }

    /// `get event()`.
    pub fn event(&self) -> Event<T> {
        let weak = Rc::downgrade(&self.inner);
        Event::new(move |listener| {
            let Some(inner) = weak.upgrade() else { return lifecycle::none() };
            if inner.disposed.get() {
                return lifecycle::none();
            }
            let first = inner.size.get() == 0;
            if first && let Some(f) = &inner.options.on_will_add_first_listener {
                f();
            }
            if !first && inner.queue.borrow().is_none() {
                *inner.queue.borrow_mut() = Some(EventDeliveryQueue::new());
            }
            inner.listeners.borrow_mut().push(Some(listener.clone()));
            if first && let Some(f) = &inner.options.on_did_add_first_listener {
                f();
            }
            if let Some(f) = &inner.options.on_did_add_listener {
                f();
            }
            inner.size.set(inner.size.get() + 1);
            let weak = Rc::downgrade(&inner);
            lifecycle::to_disposable(move || {
                if let Some(inner) = weak.upgrade() {
                    inner.remove_listener(&listener);
                }
            })
        })
    }

    /// `fire(event)`.
    pub fn fire(&self, event: T) {
        let inner = &self.inner;
        let queue = inner.queue.borrow().clone();
        if let Some(q) = &queue {
            let pending = q.0.borrow().current.as_ref().and_then(Weak::upgrade);
            if let Some(pending) = pending {
                pending.deliver_pending();
            }
        }
        let count = inner.listeners.borrow().len();
        if inner.size.get() == 0 {
            return;
        }
        if count == 1 && queue.is_none() {
            let listener = inner.listeners.borrow()[0].clone();
            if let Some(listener) = listener {
                inner.deliver(&listener, &event);
            }
            return;
        }
        let q = match queue {
            Some(q) => q,
            None => {
                let q = EventDeliveryQueue::new();
                *inner.queue.borrow_mut() = Some(q.clone());
                q
            }
        };
        {
            let me: Weak<dyn PendingDelivery> = inner.me.borrow().clone();
            let mut state = q.0.borrow_mut();
            state.i = 0;
            state.end = count;
            state.current = Some(me);
            state.current_id = inner.id();
            state.value = Some(Rc::new(event));
        }
        inner.deliver_queue(&q);
    }

    pub fn has_listeners(&self) -> bool {
        self.inner.size.get() > 0
    }

    /// `dispose()`: solta os listeners e chama `onDidRemoveLastListener`.
    pub fn dispose(&self) {
        let inner = &self.inner;
        if inner.disposed.replace(true) {
            return;
        }
        if let Some(q) = inner.queue.borrow().clone()
            && q.0.borrow().current_id == inner.id()
        {
            q.reset();
        }
        inner.listeners.borrow_mut().clear();
        inner.size.set(0);
        if let Some(f) = &inner.options.on_did_remove_last_listener {
            f();
        }
    }
}

impl<T: 'static> lifecycle::IDisposable for Emitter<T> {
    fn dispose(&self) {
        Emitter::dispose(self);
    }
}
