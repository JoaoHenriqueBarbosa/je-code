//! Porte de `src/vs/base/common/lifecycle.ts`: `IDisposable` e os contêineres de ciclo de vida.
//!
//! Identidade importa no original (`Set` de disposables, `o === Disposable.None`, o `===` do
//! `MutableDisposable`), então o handle compartilhado é `Rc<dyn IDisposable>` ([`DisposableRef`]) e a
//! comparação é por ponteiro. O rastreador de vazamento (`DisposableTracker`) entra com os testes
//! que dependem dele.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::hash::Hash;
use std::rc::Rc;

use crate::errors::{self, Error};

/// `IDisposable`. `dispose` recebe `&self` porque o objeto é compartilhado (`Rc`), como no JS.
pub trait IDisposable {
    fn dispose(&self);
}

/// O handle compartilhado de um disposable.
pub type DisposableRef = Rc<dyn IDisposable>;

fn same(a: &DisposableRef, b: &DisposableRef) -> bool {
    std::ptr::addr_eq(Rc::as_ptr(a), Rc::as_ptr(b))
}

struct Noop;

impl IDisposable for Noop {
    fn dispose(&self) {}
}

thread_local! {
    static NONE: DisposableRef = Rc::new(Noop);
}

/// `Disposable.None`: o disposable que não faz nada, com identidade única por thread.
pub fn none() -> DisposableRef {
    NONE.with(Rc::clone)
}

pub fn is_none(d: &DisposableRef) -> bool {
    NONE.with(|n| same(n, d))
}

/// `dispose(iterable)`: descarta todos, mesmo que algum lance; um erro é relançado como veio, mais de
/// um vira `AggregateError('Encountered errors while disposing of store')`.
pub fn dispose_all<'a>(disposables: impl IntoIterator<Item = &'a DisposableRef>) {
    let mut failures: Vec<Error> = Vec::new();
    for d in disposables {
        if let Err(e) = errors::catch(|| d.dispose()) {
            failures.push(e);
        }
    }
    match failures.len() {
        0 => {}
        1 => errors::throw(failures.remove(0)),
        _ => errors::throw(Error::aggregate(failures, "Encountered errors while disposing of store")),
    }
}

struct FunctionDisposable {
    f: Cell<Option<Box<dyn FnOnce()>>>,
}

impl IDisposable for FunctionDisposable {
    fn dispose(&self) {
        // `take` garante a chamada única do original (`_isDisposed`).
        if let Some(f) = self.f.take() {
            f();
        }
    }
}

/// `toDisposable(fn)`: `fn` roda no máximo uma vez.
pub fn to_disposable(f: impl FnOnce() + 'static) -> DisposableRef {
    Rc::new(FunctionDisposable { f: Cell::new(Some(Box::new(f))) })
}

/// `combinedDisposable(...disposables)`.
pub fn combined_disposable(disposables: Vec<DisposableRef>) -> DisposableRef {
    to_disposable(move || dispose_all(&disposables))
}

/// `DisposableStore`.
#[derive(Default)]
pub struct DisposableStore {
    // Conjunto com a ordem de inserção, como o `Set` do JS.
    to_dispose: RefCell<Vec<DisposableRef>>,
    is_disposed: Cell<bool>,
}

thread_local! {
    static DISABLE_DISPOSED_WARNING: Cell<bool> = const { Cell::new(false) };
}

impl DisposableStore {
    pub fn new() -> Rc<Self> {
        Rc::new(Self::default())
    }

    /// `DisposableStore.DISABLE_DISPOSED_WARNING`.
    pub fn set_disable_disposed_warning(value: bool) {
        DISABLE_DISPOSED_WARNING.with(|w| w.set(value));
    }

    pub fn is_disposed(&self) -> bool {
        self.is_disposed.get()
    }

    pub fn len(&self) -> usize {
        self.to_dispose.borrow().len()
    }

    pub fn is_empty(&self) -> bool {
        self.to_dispose.borrow().is_empty()
    }

    /// `clear()`: descarta tudo sem marcar o store como descartado. A lista é esvaziada antes de
    /// descartar (o `finally` do original), então um disposable que adiciona outro durante o
    /// descarte não é perdido nem descartado duas vezes.
    pub fn clear(&self) {
        let items = std::mem::take(&mut *self.to_dispose.borrow_mut());
        if !items.is_empty() {
            dispose_all(&items);
        }
    }

    /// `add(o)`. Devolve o próprio `o`, como o original.
    pub fn add(&self, o: DisposableRef) -> DisposableRef {
        if is_none(&o) {
            return o;
        }
        if self.is_disposed.get() {
            if !DISABLE_DISPOSED_WARNING.with(Cell::get) {
                eprintln!(
                    "Error: Trying to add a disposable to a DisposableStore that has already been disposed of. The added object will be leaked!"
                );
            }
        } else {
            let mut items = self.to_dispose.borrow_mut();
            if !items.iter().any(|d| same(d, &o)) {
                items.push(o.clone());
            }
        }
        o
    }

    /// `add` para um tipo concreto, devolvendo o `Rc` tipado.
    pub fn add_rc<T: IDisposable + 'static>(&self, o: Rc<T>) -> Rc<T> {
        self.add(o.clone());
        o
    }

    /// `delete(o)`: tira do store e descarta, esteja ou não no store.
    pub fn delete(&self, o: &DisposableRef) {
        self.to_dispose.borrow_mut().retain(|d| !same(d, o));
        o.dispose();
    }

    /// `deleteAndLeak(o)`: tira do store sem descartar.
    pub fn delete_and_leak(&self, o: &DisposableRef) {
        self.to_dispose.borrow_mut().retain(|d| !same(d, o));
    }

    /// `assertNotDisposed()`.
    pub fn assert_not_disposed(&self) {
        if self.is_disposed.get() {
            errors::on_unexpected_error(&Error::named("BugIndicatingError", "Object disposed"));
        }
    }
}

impl IDisposable for DisposableStore {
    fn dispose(&self) {
        if self.is_disposed.replace(true) {
            return;
        }
        self.clear();
    }
}

/// `MutableDisposable<T>`.
pub struct MutableDisposable<T: IDisposable + ?Sized> {
    value: RefCell<Option<Rc<T>>>,
    is_disposed: Cell<bool>,
}

impl<T: IDisposable + ?Sized> Default for MutableDisposable<T> {
    fn default() -> Self {
        Self { value: RefCell::new(None), is_disposed: Cell::new(false) }
    }
}

impl<T: IDisposable + ?Sized> MutableDisposable<T> {
    pub fn new() -> Self {
        Self::default()
    }

    /// `get value()`: `None` depois de descartado.
    pub fn value(&self) -> Option<Rc<T>> {
        if self.is_disposed.get() { None } else { self.value.borrow().clone() }
    }

    /// `set value(v)`: descartado, ou o mesmo objeto, não faz nada; senão descarta o anterior.
    pub fn set(&self, value: Option<Rc<T>>) {
        if self.is_disposed.get() {
            return;
        }
        let unchanged = match (&*self.value.borrow(), &value) {
            (Some(a), Some(b)) => std::ptr::addr_eq(Rc::as_ptr(a), Rc::as_ptr(b)),
            (None, None) => true,
            _ => false,
        };
        if unchanged {
            return;
        }
        let old = self.value.replace(None);
        if let Some(old) = old {
            old.dispose();
        }
        *self.value.borrow_mut() = value;
    }

    /// `clear()`.
    pub fn clear(&self) {
        self.set(None);
    }

    /// `clearAndLeak()`: devolve o valor sem descartar.
    pub fn clear_and_leak(&self) -> Option<Rc<T>> {
        self.value.replace(None)
    }
}

impl<T: IDisposable + ?Sized> IDisposable for MutableDisposable<T> {
    fn dispose(&self) {
        self.is_disposed.set(true);
        let old = self.value.replace(None);
        if let Some(old) = old {
            old.dispose();
        }
    }
}

/// `MandatoryMutableDisposable<T>`.
pub struct MandatoryMutableDisposable<T: IDisposable + ?Sized> {
    disposable: MutableDisposable<T>,
    is_disposed: Cell<bool>,
}

impl<T: IDisposable + ?Sized> MandatoryMutableDisposable<T> {
    pub fn new(initial: Rc<T>) -> Self {
        let disposable = MutableDisposable::new();
        disposable.set(Some(initial));
        Self { disposable, is_disposed: Cell::new(false) }
    }

    /// `get value()`. Depois do descarte o original devolve `undefined` com `!`; aqui, `None`.
    pub fn value(&self) -> Option<Rc<T>> {
        self.disposable.value()
    }

    pub fn set(&self, value: Rc<T>) {
        if !self.is_disposed.get() {
            self.disposable.set(Some(value));
        }
    }
}

impl<T: IDisposable + ?Sized> IDisposable for MandatoryMutableDisposable<T> {
    fn dispose(&self) {
        self.is_disposed.set(true);
        self.disposable.dispose();
    }
}

/// `RefCountedDisposable`.
pub struct RefCountedDisposable {
    counter: Cell<u32>,
    disposable: DisposableRef,
}

impl RefCountedDisposable {
    pub fn new(disposable: DisposableRef) -> Self {
        Self { counter: Cell::new(1), disposable }
    }

    pub fn acquire(&self) -> &Self {
        self.counter.set(self.counter.get() + 1);
        self
    }

    pub fn release(&self) -> &Self {
        let n = self.counter.get().saturating_sub(1);
        self.counter.set(n);
        if n == 0 {
            self.disposable.dispose();
        }
        self
    }
}

/// `IReference<T>`: o objeto e o descarte da referência.
pub struct Reference<T> {
    pub object: T,
    release: DisposableRef,
}

impl<T> Reference<T> {
    /// `ImmortalReference`: referência cujo descarte não faz nada.
    pub fn immortal(object: T) -> Self {
        Self { object, release: none() }
    }
}

impl<T> IDisposable for Reference<T> {
    fn dispose(&self) {
        self.release.dispose();
    }
}

/// O lado abstrato do `ReferenceCollection<T>`: criar e destruir o objeto referenciado.
pub trait ReferencedObjects<T> {
    fn create_referenced_object(&self, key: &str) -> T;
    fn destroy_referenced_object(&self, key: &str, object: &T);
}

/// `ReferenceCollection<T>`: um objeto por chave, vivo enquanto houver referência.
pub struct ReferenceCollection<T: Clone + 'static, F: ReferencedObjects<T> + 'static> {
    references: Rc<RefCell<HashMap<String, (T, u32)>>>,
    factory: Rc<F>,
}

impl<T: Clone + 'static, F: ReferencedObjects<T> + 'static> ReferenceCollection<T, F> {
    pub fn new(factory: F) -> Self {
        Self { references: Rc::new(RefCell::new(HashMap::new())), factory: Rc::new(factory) }
    }

    /// `acquire(key)`. O descarte da referência é de chamada única (`createSingleCallFunction`).
    pub fn acquire(&self, key: &str) -> Reference<T> {
        let object = {
            let existing = self.references.borrow().get(key).map(|(o, _)| o.clone());
            match existing {
                Some(o) => o,
                None => {
                    let o = self.factory.create_referenced_object(key);
                    self.references.borrow_mut().insert(key.to_owned(), (o.clone(), 0));
                    o
                }
            }
        };
        if let Some(entry) = self.references.borrow_mut().get_mut(key) {
            entry.1 += 1;
        }
        let references = Rc::downgrade(&self.references);
        let factory = self.factory.clone();
        let key = key.to_owned();
        let release = to_disposable(move || {
            let Some(references) = references.upgrade() else { return };
            let finished = {
                let mut map = references.borrow_mut();
                match map.get_mut(&key) {
                    Some(entry) => {
                        entry.1 -= 1;
                        (entry.1 == 0).then(|| entry.0.clone())
                    }
                    None => None,
                }
            };
            if let Some(object) = finished {
                factory.destroy_referenced_object(&key, &object);
                references.borrow_mut().remove(&key);
            }
        });
        Reference { object, release }
    }
}

/// `disposeOnReturn(fn)`.
pub fn dispose_on_return(f: impl FnOnce(&DisposableStore)) {
    let store = DisposableStore::default();
    let result = errors::catch(|| f(&store));
    store.dispose();
    if let Err(e) = result {
        errors::throw(e);
    }
}

/// `DisposableMap<K, V>`: um mapa que descarta os valores que guarda.
pub struct DisposableMap<K: Eq + Hash + Clone, V: IDisposable + 'static> {
    store: RefCell<Vec<(K, Rc<V>)>>,
    is_disposed: Cell<bool>,
}

impl<K: Eq + Hash + Clone, V: IDisposable + 'static> Default for DisposableMap<K, V> {
    fn default() -> Self {
        Self { store: RefCell::new(Vec::new()), is_disposed: Cell::new(false) }
    }
}

impl<K: Eq + Hash + Clone, V: IDisposable + 'static> DisposableMap<K, V> {
    pub fn new() -> Self {
        Self::default()
    }

    /// `clearAndDisposeAll()`.
    pub fn clear_and_dispose_all(&self) {
        let items = std::mem::take(&mut *self.store.borrow_mut());
        if items.is_empty() {
            return;
        }
        let refs: Vec<DisposableRef> = items.into_iter().map(|(_, v)| v as Rc<dyn IDisposable>).collect();
        dispose_all(&refs);
    }

    pub fn has(&self, key: &K) -> bool {
        self.store.borrow().iter().any(|(k, _)| k == key)
    }

    pub fn len(&self) -> usize {
        self.store.borrow().len()
    }

    pub fn is_empty(&self) -> bool {
        self.store.borrow().is_empty()
    }

    pub fn get(&self, key: &K) -> Option<Rc<V>> {
        self.store.borrow().iter().find(|(k, _)| k == key).map(|(_, v)| v.clone())
    }

    /// `set(key, value, skipDisposeOnOverwrite)`.
    pub fn set(&self, key: K, value: Rc<V>, skip_dispose_on_overwrite: bool) {
        if self.is_disposed.get() {
            eprintln!("Error: Trying to add a disposable to a DisposableMap that has already been disposed of. The added object will be leaked!");
        }
        let old = {
            let mut store = self.store.borrow_mut();
            match store.iter_mut().find(|(k, _)| *k == key) {
                Some(slot) => Some(std::mem::replace(&mut slot.1, value)),
                None => {
                    store.push((key, value));
                    None
                }
            }
        };
        if let Some(old) = old
            && !skip_dispose_on_overwrite
        {
            old.dispose();
        }
    }

    /// `deleteAndDispose(key)`.
    pub fn delete_and_dispose(&self, key: &K) {
        let removed = {
            let mut store = self.store.borrow_mut();
            store.iter().position(|(k, _)| k == key).map(|i| store.remove(i))
        };
        if let Some((_, v)) = removed {
            v.dispose();
        }
    }

    /// `deleteAndLeak(key)`.
    pub fn delete_and_leak(&self, key: &K) -> Option<Rc<V>> {
        let mut store = self.store.borrow_mut();
        store.iter().position(|(k, _)| k == key).map(|i| store.remove(i).1)
    }

    pub fn keys(&self) -> Vec<K> {
        self.store.borrow().iter().map(|(k, _)| k.clone()).collect()
    }

    pub fn values(&self) -> Vec<Rc<V>> {
        self.store.borrow().iter().map(|(_, v)| v.clone()).collect()
    }
}

impl<K: Eq + Hash + Clone, V: IDisposable + 'static> IDisposable for DisposableMap<K, V> {
    fn dispose(&self) {
        if self.is_disposed.replace(true) {
            return;
        }
        self.clear_and_dispose_all();
    }
}
