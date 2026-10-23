//! Ajudantes compartilhados pelos widgets: configurações globais com inscritos (o `Emitter` de módulo do
//! original), o `_register(new Emitter())` e o `_register(new DisposableStore())`, o `throw` e o
//! `Math.round` do JavaScript.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use je_base::errors::{self, Error};
use je_base::event::Emitter;
use je_base::lifecycle::{Disposable, DisposableRef, DisposableStore, to_disposable};

/// Uma configuração global que avisa quem se inscreveu (o `Emitter` de módulo do original). Não é um
/// `Emitter` porque o rastreador de vazamento do `je-base` veria um disposable criado dentro de um teste.
type Subscriber<T> = (u64, Rc<dyn Fn(T)>);

pub(crate) struct GlobalSetting<T: Copy + 'static> {
    pub(crate) value: Cell<T>,
    subscribers: RefCell<Vec<Subscriber<T>>>,
    next: Cell<u64>,
}

impl<T: Copy + 'static> GlobalSetting<T> {
    pub(crate) fn new(value: T) -> Self {
        Self { value: Cell::new(value), subscribers: RefCell::new(Vec::new()), next: Cell::new(0) }
    }

    pub(crate) fn set(&self, value: T) {
        self.value.set(value);
        let subscribers: Vec<_> = self.subscribers.borrow().iter().map(|(_, f)| f.clone()).collect();
        for subscriber in subscribers {
            subscriber(value);
        }
    }
}

pub(crate) fn subscribe<T: Copy + 'static>(setting: &'static std::thread::LocalKey<GlobalSetting<T>>, f: impl Fn(T) + 'static) -> DisposableRef {
    let id = setting.with(|s| {
        let id = s.next.get();
        s.next.set(id + 1);
        s.subscribers.borrow_mut().push((id, Rc::new(f)));
        id
    });
    to_disposable(move || setting.with(|s| s.subscribers.borrow_mut().retain(|(i, _)| *i != id)))
}

/// `this._register(new Emitter<T>())`.
pub(crate) fn registered_emitter<T: 'static>(base: &Rc<Disposable>) -> Rc<Emitter<T>> {
    let emitter = Emitter::new();
    base.register(emitter.clone());
    emitter
}

/// `disposables.add(new Emitter<T>())`: um emissor que o `store` descarta.
pub(crate) fn store_emitter<T: 'static>(store: &Rc<DisposableStore>) -> Rc<Emitter<T>> {
    let emitter = Emitter::new();
    store.add_rc(emitter.clone());
    emitter
}

/// `this._register(new DisposableStore())`.
pub(crate) fn registered_store(base: &Rc<Disposable>) -> Rc<DisposableStore> {
    let store = DisposableStore::new();
    base.register(store.clone());
    store
}

/// `throw new Error(message)`.
pub(crate) fn throw(message: &str) -> ! {
    errors::throw(Error::new(message))
}

/// `Math.round`: arredonda a metade para cima (`-0.5` vira `-0`, `2.5` vira `3`).
pub(crate) fn js_round(x: f64) -> f64 {
    (x + 0.5).floor()
}

/// Se o sistema é o Windows (o `isWindows`).
// TODO(integração): ler do `je_base::platform` quando houver a plataforma global.
pub(crate) fn is_windows() -> bool {
    cfg!(target_os = "windows")
}
