//! Porte de `src/vs/workbench/browser/parts/notifications/pendingNotificationToasts.ts`: os toasts que já foram
//! pedidos mas ainda não foram desenhados (a criação é adiada para o próximo quadro, #107935).
//!
//! Notas de tradução:
//! - **`pendingToast.item === item`** é a comparação de identidade que o chamador passa em `is_same` (o `Rc::ptr_eq`
//!   dos itens de notificação), porque o tipo genérico `T` não tem identidade própria.
//! - **`queueMicrotask`** é [`je_base::runtime::queue_microtask`].
//! - **`Set<IPendingNotificationToast<T>>`** é uma lista de `Rc<PendingToast<T>>`, comparados por `Rc::ptr_eq`.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use je_base::lifecycle::{DisposableRef, DisposableStore, IDisposable, to_disposable};
use je_base::runtime::queue_microtask;

/// `IPendingNotificationToast<T>`.
struct PendingToast<T> {
    item: RefCell<T>,
    disposables: Rc<DisposableStore>,
    cleanup_scheduled: Cell<bool>,
}

/// O estado compartilhado com os fechos que o original escreve com `this`.
struct Shared<T> {
    pending_toasts: RefCell<Vec<Rc<PendingToast<T>>>>,
    is_current: Box<dyn Fn(&T) -> bool>,
    equals: Box<dyn Fn(&T, &T) -> bool>,
    is_same: Box<dyn Fn(&T, &T) -> bool>,
    scheduler: Box<dyn Fn(Box<dyn FnOnce()>) -> DisposableRef>,
}

impl<T> Shared<T> {
    fn delete(&self, pending_toast: &Rc<PendingToast<T>>) {
        self.pending_toasts.borrow_mut().retain(|candidate| !Rc::ptr_eq(candidate, pending_toast));
    }

    fn has(&self, pending_toast: &Rc<PendingToast<T>>) -> bool {
        self.pending_toasts.borrow().iter().any(|candidate| Rc::ptr_eq(candidate, pending_toast))
    }

    fn snapshot(&self) -> Vec<Rc<PendingToast<T>>> {
        self.pending_toasts.borrow().clone()
    }
}

/// `PendingNotificationToasts<T>`.
pub struct PendingNotificationToasts<T: Clone + 'static> {
    shared: Rc<Shared<T>>,
}

impl<T: Clone + 'static> PendingNotificationToasts<T> {
    /// `new PendingNotificationToasts(isCurrent, equals, scheduler)`, com `is_same` para o `===` de [`Self::remove`].
    pub fn new(
        is_current: impl Fn(&T) -> bool + 'static,
        equals: impl Fn(&T, &T) -> bool + 'static,
        is_same: impl Fn(&T, &T) -> bool + 'static,
        scheduler: impl Fn(Box<dyn FnOnce()>) -> DisposableRef + 'static,
    ) -> Self {
        Self {
            shared: Rc::new(Shared {
                pending_toasts: RefCell::new(Vec::new()),
                is_current: Box::new(is_current),
                equals: Box::new(equals),
                is_same: Box::new(is_same),
                scheduler: Box::new(scheduler),
            }),
        }
    }

    pub fn try_replace(&self, item: &T) -> bool {
        for pending_toast in self.shared.snapshot() {
            let matches = (self.shared.equals)(&pending_toast.item.borrow(), item);
            if matches {
                *pending_toast.item.borrow_mut() = item.clone();
                return true;
            }
        }

        false
    }

    pub fn add(&self, item: &T, render: impl FnOnce(T, Rc<DisposableStore>) + 'static) {
        let disposables = DisposableStore::new();
        let pending_toast = Rc::new(PendingToast { item: RefCell::new(item.clone()), disposables: disposables.clone(), cleanup_scheduled: Cell::new(false) });
        self.shared.pending_toasts.borrow_mut().push(pending_toast.clone());
        {
            let (shared, pending_toast) = (self.shared.clone(), pending_toast.clone());
            disposables.add(to_disposable(move || shared.delete(&pending_toast)));
        }
        // Defer toast creation to avoid animation work while the renderer is busy (#107935).
        let shared = self.shared.clone();
        let scheduled_disposables = disposables.clone();
        disposables.add((self.shared.scheduler)(Box::new(move || {
            let pending_item = pending_toast.item.borrow().clone();
            if !(shared.is_current)(&pending_item) {
                scheduled_disposables.dispose();
                return;
            }

            shared.delete(&pending_toast);
            render(pending_item, scheduled_disposables);
        })));
    }

    pub fn remove(&self, item: &T) {
        for pending_toast in self.shared.snapshot() {
            let is_item = (self.shared.is_same)(&pending_toast.item.borrow(), item);
            if is_item && !pending_toast.cleanup_scheduled.get() {
                pending_toast.cleanup_scheduled.set(true);
                // Allow a synchronous duplicate ADD to retarget the pending toast before cleanup.
                let shared = self.shared.clone();
                queue_microtask(move || {
                    pending_toast.cleanup_scheduled.set(false);
                    let still_pending = shared.has(&pending_toast);
                    let current = (shared.is_current)(&pending_toast.item.borrow());
                    if still_pending && !current {
                        pending_toast.disposables.dispose();
                    }
                });
                break;
            }
        }
    }

    pub fn clear(&self) {
        for pending_toast in self.shared.snapshot() {
            pending_toast.disposables.dispose();
        }
        self.shared.pending_toasts.borrow_mut().clear();
    }
}

impl<T: Clone + 'static> IDisposable for PendingNotificationToasts<T> {
    fn dispose(&self) {
        self.clear();
    }
}
