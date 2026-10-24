//! Porte de `src/vs/base/common/lifecycle.ts`: `IDisposable`, os contêineres de ciclo de vida e o
//! rastreador de vazamento (`DisposableTracker`).
//!
//! Identidade importa no original (`Set` de disposables, `o === Disposable.None`, o `===` do
//! `MutableDisposable`), então o handle compartilhado é `Rc<dyn IDisposable>` ([`DisposableRef`]) e a
//! comparação é por ponteiro ([`id_of`]). Os contêineres que o original rastreia no construtor
//! (`trackDisposable(this)`) nascem com `new() -> Rc<Self>`, porque o rastreador guarda o objeto.
//! `Default` cria o contêiner sem rastreio, para uso interno de tipos que não são `Disposable`.
//!
//! Ficam de fora o `GCBasedDisposableTracker` (depende de `FinalizationRegistry`, que não existe sem
//! coletor de lixo) e o `DisposableResourceMap` (depende de `URI`/`ResourceMap`).
//!
//! O `ReferenceCollection.acquire(key, ...args)` repassa os argumentos extras ao `createReferencedObject`; aqui o
//! `createReferencedObject(key)` do [`ReferencedObjects`] não recebe argumentos, e quem precisa deles chama
//! [`ReferenceCollection::acquire_with`] com o fecho que cria o objeto.

use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, HashMap};
use std::hash::Hash;
use std::rc::Rc;

use crate::async_::async_fn;
use crate::errors::{self, Error};
use crate::promise::Promise;
use crate::stacktrace;

/// `IDisposable`. `dispose` recebe `&self` porque o objeto é compartilhado (`Rc`), como no JS.
pub trait IDisposable {
    fn dispose(&self);

    /// O `constructor.name` do original, para o relatório de vazamento.
    fn constructor_name(&self) -> &'static str {
        std::any::type_name::<Self>()
    }
}

/// Um `Rc` de disposable também é um disposable (no JS o handle e o objeto são a mesma coisa).
impl<T: IDisposable + ?Sized> IDisposable for Rc<T> {
    fn dispose(&self) {
        (**self).dispose();
    }

    fn constructor_name(&self) -> &'static str {
        (**self).constructor_name()
    }
}

/// O handle compartilhado de um disposable.
pub type DisposableRef = Rc<dyn IDisposable>;

/// A identidade de um objeto (o `===` do JS): o endereço do dado dentro do `Rc`.
pub type DisposableId = usize;

/// Identidade do objeto apontado pelo `Rc`.
pub fn id_of<T: ?Sized>(d: &Rc<T>) -> DisposableId {
    Rc::as_ptr(d).cast::<()>() as usize
}

/// Identidade de um objeto a partir de `&self` (vale para o objeto dentro de um `Rc`).
pub fn id_of_ref<T: ?Sized>(d: &T) -> DisposableId {
    std::ptr::from_ref(d).cast::<()>() as usize
}

fn same(a: &DisposableRef, b: &DisposableRef) -> bool {
    id_of(a) == id_of(b)
}

// #region Disposable Tracking

/// `IDisposableTracker`. O original passa o objeto; aqui passa a identidade, e `track_disposable` recebe
/// o handle porque é ele que o rastreador segura.
pub trait IDisposableTracker {
    /// Chamado na construção de um disposable.
    fn track_disposable(&self, disposable: &DisposableRef);
    /// Chamado quando um disposable é registrado como filho de outro. `None` tira do pai anterior.
    fn set_parent(&self, child: DisposableId, parent: Option<DisposableId>);
    /// Chamado depois de um disposable ser descartado.
    fn mark_as_disposed(&self, disposable: DisposableId);
    /// Indica que o objeto é um singleton que não precisa ser descartado.
    fn mark_as_singleton(&self, disposable: DisposableId);
}

thread_local! {
    static TRACKER: RefCell<Option<Rc<dyn IDisposableTracker>>> = const { RefCell::new(None) };
    static TRACKER_IDX: Cell<u64> = const { Cell::new(0) };
}

/// `setDisposableTracker`.
pub fn set_disposable_tracker(tracker: Option<Rc<dyn IDisposableTracker>>) {
    TRACKER.with(|t| *t.borrow_mut() = tracker);
}

fn with_tracker(f: impl FnOnce(&dyn IDisposableTracker)) {
    let tracker = TRACKER.with(|t| t.borrow().clone());
    if let Some(tracker) = tracker {
        f(&*tracker);
    }
}

/// `trackDisposable(x)`: devolve o próprio objeto.
pub fn track_disposable<T: IDisposable + 'static>(x: Rc<T>) -> Rc<T> {
    with_tracker(|t| t.track_disposable(&(x.clone() as DisposableRef)));
    x
}

/// `markAsDisposed(disposable)`.
pub fn mark_as_disposed(disposable: DisposableId) {
    with_tracker(|t| t.mark_as_disposed(disposable));
}

fn set_parent_of_disposable(child: DisposableId, parent: Option<DisposableId>) {
    with_tracker(|t| t.set_parent(child, parent));
}

/// `markAsSingleton(singleton)`: devolve o próprio objeto.
pub fn mark_as_singleton<T: ?Sized>(singleton: Rc<T>) -> Rc<T> {
    with_tracker(|t| t.mark_as_singleton(id_of(&singleton)));
    singleton
}

/// A pilha do ponto de alocação, formatada só quando o relatório pede (o `source: Error` do original,
/// cujo `stack` só é lido ao relatar vazamento).
#[derive(Clone)]
pub struct StackSource(Rc<dyn Fn() -> String>);

impl StackSource {
    pub fn new(stack: impl Fn() -> String + 'static) -> Self {
        Self(Rc::new(stack))
    }

    /// `new Error()` agora, formatado depois.
    pub fn capture() -> Self {
        Self(stacktrace::capture_lazy())
    }

    pub fn stack(&self) -> String {
        (self.0)()
    }
}

/// `DisposableInfo`.
#[derive(Clone)]
pub struct DisposableInfo {
    pub value: Option<DisposableRef>,
    pub name: &'static str,
    /// O ponto de alocação; `None` enquanto o objeto só apareceu como pai ou filho.
    pub source: Option<StackSource>,
    pub parent: Option<DisposableId>,
    pub is_singleton: bool,
    pub idx: u64,
}

/// O resultado de `computeLeakingDisposables`.
pub struct LeakReport {
    pub leaks: Vec<DisposableInfo>,
    pub details: String,
}

/// `DisposableTracker`.
#[derive(Default)]
pub struct DisposableTracker {
    living: RefCell<HashMap<DisposableId, DisposableInfo>>,
}

impl DisposableTracker {
    pub fn new() -> Self {
        Self::default()
    }

    fn with_data<R>(&self, id: DisposableId, f: impl FnOnce(&mut DisposableInfo) -> R) -> R {
        let mut living = self.living.borrow_mut();
        let data = living.entry(id).or_insert_with(|| {
            let idx = TRACKER_IDX.with(|i| i.replace(i.get() + 1));
            DisposableInfo { value: None, name: "", source: None, parent: None, is_singleton: false, idx }
        });
        f(data)
    }

    /// `trackDisposable(d, source?)`: o primeiro ponto de alocação vale, o de chamadas repetidas não.
    pub fn track_disposable_from(&self, d: &DisposableRef, source: Option<StackSource>) {
        self.with_data(id_of(d), |data| {
            if data.value.is_none() {
                data.value = Some(d.clone());
                data.name = d.constructor_name();
            }
            if data.source.is_none() {
                data.source = Some(source.unwrap_or_else(StackSource::capture));
            }
        });
    }

    fn root_parent(living: &HashMap<DisposableId, DisposableInfo>, id: DisposableId, cache: &mut HashMap<DisposableId, DisposableId>) -> DisposableId {
        if let Some(root) = cache.get(&id) {
            return *root;
        }
        let root = match living.get(&id).and_then(|d| d.parent) {
            Some(parent) => Self::root_parent(living, parent, cache),
            None => id,
        };
        cache.insert(id, root);
        root
    }

    /// Os rastreados que vazam (têm ponto de alocação e a raiz não é singleton), na ordem de criação.
    fn leaking(&self) -> Vec<DisposableInfo> {
        let living = self.living.borrow();
        let mut cache = HashMap::new();
        let mut leaking: Vec<DisposableInfo> = living
            .iter()
            .filter(|(id, v)| v.source.is_some() && !living.get(&Self::root_parent(&living, **id, &mut cache)).is_some_and(|r| r.is_singleton))
            .map(|(_, v)| v.clone())
            .collect();
        leaking.sort_by_key(|l| l.idx);
        leaking
    }

    /// `getTrackedDisposables()`.
    pub fn get_tracked_disposables(&self) -> Vec<DisposableRef> {
        self.leaking().into_iter().filter_map(|l| l.value).collect()
    }

    /// `computeLeakingDisposables()` com o limite padrão de 10 relatados.
    pub fn compute_leaking_disposables(&self) -> Option<LeakReport> {
        self.compute_leaking_disposables_with(10, None)
    }

    /// `computeLeakingDisposables(maxReported, preComputedLeaks)`.
    pub fn compute_leaking_disposables_with(&self, max_reported: usize, pre_computed_leaks: Option<Vec<DisposableInfo>>) -> Option<LeakReport> {
        let mut uncovered = match pre_computed_leaks {
            Some(leaks) => leaks,
            None => {
                let leaking = self.leaking();
                if leaking.is_empty() {
                    return None;
                }
                let ids: Vec<DisposableId> = leaking.iter().filter_map(|l| l.value.as_ref().map(id_of)).collect();
                // Tira os filhos de outros vazamentos. Supõe que não há ciclos.
                let uncovered: Vec<DisposableInfo> = leaking.into_iter().filter(|l| !l.parent.is_some_and(|p| ids.contains(&p))).collect();
                if uncovered.is_empty() {
                    errors::throw(Error::new("There are cyclic diposable chains!"));
                }
                uncovered
            }
        };

        fn stack_trace_path(leaking: &DisposableInfo) -> Vec<String> {
            let stack = leaking.source.as_ref().map(StackSource::stack).unwrap_or_default();
            let mut lines: Vec<String> = stack.split('\n').map(|p| p.trim().replacen("at ", "", 1)).filter(|l| !l.is_empty()).collect();
            // Os quadros do próprio rastreio (o `removePrefix` do original): a captura, o rastreador e os
            // fechos do Rust no caminho. O quadro de quem criou o objeto (`DisposableStore::new`) fica.
            const TRACKING_MARKERS: [&str; 7] = ["track_disposable", "with_tracker", "StackSource", "with_data", "unwrap_or_else", "core::ops::function", "stacktrace::"];
            let is_tracking_frame = |line: &str| line == "Error" || TRACKING_MARKERS.iter().any(|marker| line.contains(marker));
            let skip = lines.iter().take_while(|l| is_tracking_frame(l)).count();
            lines.drain(..skip);
            lines.reverse();
            lines
        }

        let paths: HashMap<u64, Vec<String>> = uncovered.iter().map(|l| (l.idx, stack_trace_path(l))).collect();
        let path_of = |l: &DisposableInfo| &paths[&l.idx];

        // `SetMap<string, DisposableInfo>`: o prefixo do caminho e quem passa por ele.
        let mut stack_trace_starts: HashMap<String, Vec<u64>> = HashMap::new();
        for leaking in &uncovered {
            let path = path_of(leaking);
            for i in 0..=path.len() {
                let starts = stack_trace_starts.entry(path[..i].join("\n")).or_default();
                if !starts.contains(&leaking.idx) {
                    starts.push(leaking.idx);
                }
            }
        }

        // Os mais antigos primeiro.
        uncovered.sort_by_key(|l| l.idx);
        let by_idx: HashMap<u64, &DisposableInfo> = uncovered.iter().map(|l| (l.idx, l)).collect();

        let total = uncovered.len();
        let mut message = String::new();
        for (n, leaking) in uncovered.iter().take(max_reported).enumerate() {
            let path = path_of(leaking);
            let mut formatted: Vec<String> = Vec::new();
            for i in 0..path.len() {
                let starts = &stack_trace_starts[&path[..=i].join("\n")];
                let line = format!("(shared with {}/{} leaks) at {}", starts.len(), total, path[i]);

                let prev_starts = &stack_trace_starts[&path[..i].join("\n")];
                // `groupBy(prevStarts.map(d => path(d)[i]))` sem a chave do próprio caminho.
                let mut continuations: Vec<(String, usize)> = Vec::new();
                for d in prev_starts {
                    let key = path_of(by_idx[d]).get(i).cloned().unwrap_or_else(|| "undefined".to_owned());
                    match continuations.iter_mut().find(|(k, _)| *k == key) {
                        Some(entry) => entry.1 += 1,
                        None => continuations.push((key, 1)),
                    }
                }
                continuations.retain(|(k, _)| *k != path[i]);
                for (cont, count) in continuations {
                    formatted.insert(0, format!("    - stacktraces of {count} other leaks continue with {cont}"));
                }
                formatted.insert(0, line);
            }
            message += &format!(
                "\n\n\n==================== Leaking disposable {}/{}: {} ====================\n{}\n============================================================\n\n",
                n + 1,
                total,
                leaking.name,
                formatted.join("\n")
            );
        }
        if total > max_reported {
            message += &format!("\n\n\n... and {} more leaking disposables\n\n", total - max_reported);
        }
        Some(LeakReport { leaks: uncovered, details: message })
    }
}

impl IDisposableTracker for DisposableTracker {
    fn track_disposable(&self, d: &DisposableRef) {
        self.track_disposable_from(d, None);
    }

    fn set_parent(&self, child: DisposableId, parent: Option<DisposableId>) {
        self.with_data(child, |data| data.parent = parent);
    }

    fn mark_as_disposed(&self, d: DisposableId) {
        self.living.borrow_mut().remove(&d);
    }

    fn mark_as_singleton(&self, d: DisposableId) {
        self.with_data(d, |data| data.is_singleton = true);
    }
}

// #endregion

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

/// Descarta cada item mesmo que algum lance; um erro é relançado como veio, mais de um vira
/// `AggregateError('Encountered errors while disposing of store')`.
fn throw_collected(mut failures: Vec<Error>) {
    match failures.len() {
        0 => {}
        1 => errors::throw(failures.remove(0)),
        _ => errors::throw(Error::aggregate(failures, "Encountered errors while disposing of store")),
    }
}

/// `dispose(iterable)`.
pub fn dispose_all<'a, D: IDisposable + ?Sized + 'a>(disposables: impl IntoIterator<Item = &'a D>) {
    let failures: Vec<Error> = disposables.into_iter().filter_map(|d| errors::catch(|| d.dispose()).err()).collect();
    throw_collected(failures);
}

/// `dispose(array)`: descarta e devolve um array novo e vazio, sem mexer no recebido (#100273).
pub fn dispose_array(disposables: &[DisposableRef]) -> Vec<DisposableRef> {
    dispose_all(disposables);
    Vec::new()
}

/// `dispose(iterator)`: consome o iterador e devolve o mesmo iterador (#100273).
pub fn dispose_iterator<I: Iterator<Item = DisposableRef>>(mut disposables: I) -> I {
    let all: Vec<DisposableRef> = disposables.by_ref().collect();
    dispose_all(&all);
    disposables
}

struct FunctionDisposable {
    f: Cell<Option<Box<dyn FnOnce()>>>,
}

impl IDisposable for FunctionDisposable {
    fn dispose(&self) {
        // `take` garante a chamada única do original (`_isDisposed`).
        if let Some(f) = self.f.take() {
            mark_as_disposed(id_of_ref(self));
            f();
        }
    }
}

/// `toDisposable(fn)`: `fn` roda no máximo uma vez.
pub fn to_disposable(f: impl FnOnce() + 'static) -> DisposableRef {
    let d: DisposableRef = plain_disposable(f);
    with_tracker(|t| t.track_disposable(&d));
    d
}

/// O objeto literal `{ dispose() { ... } }` do original: um disposable que ninguém rastreia (também é o
/// `createSingleCallFunction` usado como `dispose`).
pub fn plain_disposable(f: impl FnOnce() + 'static) -> DisposableRef {
    Rc::new(FunctionDisposable { f: Cell::new(Some(Box::new(f))) })
}

/// `combinedDisposable(...disposables)`.
pub fn combined_disposable(disposables: Vec<DisposableRef>) -> DisposableRef {
    let children: Vec<DisposableId> = disposables.iter().map(id_of).collect();
    let parent = to_disposable(move || dispose_all(&disposables));
    let parent_id = id_of(&parent);
    for child in children {
        set_parent_of_disposable(child, Some(parent_id));
    }
    parent
}

/// Os itens de uma coleção com a ordem de inserção e a iteração do `Set`/`Map` do JS: quem entra durante
/// a iteração é visitado, quem sai antes de ser visitado é pulado, e trocar o valor de uma chave
/// existente mantém a posição.
struct LiveMap<K: Eq + Hash + Clone, V: Clone> {
    next_seq: u64,
    by_key: HashMap<K, (u64, V)>,
    order: BTreeMap<u64, K>,
}

impl<K: Eq + Hash + Clone, V: Clone> Default for LiveMap<K, V> {
    fn default() -> Self {
        Self { next_seq: 1, by_key: HashMap::new(), order: BTreeMap::new() }
    }
}

impl<K: Eq + Hash + Clone, V: Clone> LiveMap<K, V> {
    fn len(&self) -> usize {
        self.by_key.len()
    }

    fn get(&self, key: &K) -> Option<&V> {
        self.by_key.get(key).map(|(_, v)| v)
    }

    /// Devolve o valor anterior, se havia.
    fn insert(&mut self, key: K, value: V) -> Option<V> {
        if let Some(entry) = self.by_key.get_mut(&key) {
            return Some(std::mem::replace(&mut entry.1, value));
        }
        let seq = self.next_seq;
        self.next_seq += 1;
        self.order.insert(seq, key.clone());
        self.by_key.insert(key, (seq, value));
        None
    }

    fn remove(&mut self, key: &K) -> Option<V> {
        let (seq, value) = self.by_key.remove(key)?;
        self.order.remove(&seq);
        Some(value)
    }

    fn clear(&mut self) {
        self.by_key.clear();
        self.order.clear();
    }

    /// O primeiro item com posição depois de `after`.
    fn next_after(&self, after: u64) -> Option<(u64, K, V)> {
        let (seq, key) = self.order.range(after + 1..).next()?;
        Some((*seq, key.clone(), self.by_key[key].1.clone()))
    }

    fn values(&self) -> Vec<V> {
        self.order.values().map(|k| self.by_key[k].1.clone()).collect()
    }

    fn keys(&self) -> Vec<K> {
        self.order.values().cloned().collect()
    }
}

/// `try { dispose(items) } finally { items.clear() }`: descarta cada item (os que entram durante o
/// descarte também), limpa a coleção e só então relança o que falhou.
fn dispose_live_and_clear<K: Eq + Hash + Clone, V: Clone>(list: &RefCell<LiveMap<K, V>>, dispose_one: impl Fn(&V)) {
    let mut failures = Vec::new();
    let mut last = 0;
    loop {
        let next = list.borrow().next_after(last);
        let Some((seq, _, value)) = next else { break };
        last = seq;
        if let Err(e) = errors::catch(|| dispose_one(&value)) {
            failures.push(e);
        }
    }
    list.borrow_mut().clear();
    throw_collected(failures);
}

/// `DisposableStore`.
#[derive(Default)]
pub struct DisposableStore {
    to_dispose: RefCell<LiveMap<DisposableId, DisposableRef>>,
    is_disposed: Cell<bool>,
}

thread_local! {
    static DISABLE_DISPOSED_WARNING: Cell<bool> = const { Cell::new(false) };
}

impl DisposableStore {
    pub fn new() -> Rc<Self> {
        track_disposable(Rc::new(Self::default()))
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
        self.len() == 0
    }

    /// `clear()`: descarta tudo sem marcar o store como descartado.
    pub fn clear(&self) {
        if self.is_empty() {
            return;
        }
        dispose_live_and_clear(&self.to_dispose, |d| d.dispose());
    }

    /// `add(o)`. Devolve o próprio `o`, como o original.
    pub fn add(&self, o: DisposableRef) -> DisposableRef {
        if is_none(&o) {
            return o;
        }
        if id_of(&o) == id_of_ref(self) {
            errors::throw(Error::new("Cannot register a disposable on itself!"));
        }
        set_parent_of_disposable(id_of(&o), Some(id_of_ref(self)));
        if self.is_disposed.get() {
            if !DISABLE_DISPOSED_WARNING.with(Cell::get) {
                eprintln!(
                    "Error: Trying to add a disposable to a DisposableStore that has already been disposed of. The added object will be leaked!"
                );
            }
        } else if self.to_dispose.borrow().get(&id_of(&o)).is_none() {
            self.to_dispose.borrow_mut().insert(id_of(&o), o.clone());
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
        if id_of(o) == id_of_ref(self) {
            errors::throw(Error::new("Cannot dispose a disposable on itself!"));
        }
        self.to_dispose.borrow_mut().remove(&id_of(o));
        o.dispose();
    }

    /// `deleteAndLeak(o)`: tira do store sem descartar.
    pub fn delete_and_leak(&self, o: &DisposableRef) {
        if self.to_dispose.borrow_mut().remove(&id_of(o)).is_some() {
            set_parent_of_disposable(id_of(o), None);
        }
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
        if self.is_disposed.get() {
            return;
        }
        mark_as_disposed(id_of_ref(self));
        self.is_disposed.set(true);
        self.clear();
    }
}

/// `Disposable` (a classe abstrata): quem herda registra os filhos com [`Disposable::register`], e eles
/// são descartados junto. O Rust não tem herança, então o tipo que "herda" guarda um `Disposable`.
pub struct Disposable {
    store: Rc<DisposableStore>,
}

impl Disposable {
    pub fn new() -> Rc<Self> {
        let this = track_disposable(Rc::new(Self { store: DisposableStore::new() }));
        set_parent_of_disposable(id_of(&this.store), Some(id_of(&this)));
        this
    }

    /// `_register(o)`.
    pub fn register(&self, o: DisposableRef) -> DisposableRef {
        if id_of(&o) == id_of_ref(self) {
            errors::throw(Error::new("Cannot register a disposable on itself!"));
        }
        self.store.add(o)
    }
}

impl IDisposable for Disposable {
    fn dispose(&self) {
        mark_as_disposed(id_of_ref(self));
        self.store.dispose();
    }
}

/// `MutableDisposable<T>`.
pub struct MutableDisposable<T: IDisposable + ?Sized + 'static> {
    value: RefCell<Option<Rc<T>>>,
    is_disposed: Cell<bool>,
}

impl<T: IDisposable + ?Sized + 'static> Default for MutableDisposable<T> {
    fn default() -> Self {
        Self { value: RefCell::new(None), is_disposed: Cell::new(false) }
    }
}

impl<T: IDisposable + ?Sized + 'static> MutableDisposable<T> {
    pub fn new() -> Rc<Self> {
        track_disposable(Rc::new(Self::default()))
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
        let current = self.value.borrow().clone();
        let unchanged = match (&current, &value) {
            (Some(a), Some(b)) => id_of(a) == id_of(b),
            (None, None) => true,
            _ => false,
        };
        if unchanged {
            return;
        }
        if let Some(old) = current {
            old.dispose();
        }
        if let Some(new) = &value {
            set_parent_of_disposable(id_of(new), Some(id_of_ref(self)));
        }
        *self.value.borrow_mut() = value;
    }

    /// `clear()`.
    pub fn clear(&self) {
        self.set(None);
    }

    /// `clearAndLeak()`: devolve o valor sem descartar e tira o pai dele.
    pub fn clear_and_leak(&self) -> Option<Rc<T>> {
        let old = self.value.replace(None);
        if let Some(old) = &old {
            set_parent_of_disposable(id_of(old), None);
        }
        old
    }
}

impl<T: IDisposable + ?Sized + 'static> IDisposable for MutableDisposable<T> {
    fn dispose(&self) {
        self.is_disposed.set(true);
        mark_as_disposed(id_of_ref(self));
        let current = self.value.borrow().clone();
        if let Some(old) = current {
            old.dispose();
        }
        *self.value.borrow_mut() = None;
    }
}

/// `MandatoryMutableDisposable<T>`.
pub struct MandatoryMutableDisposable<T: IDisposable + ?Sized + 'static> {
    disposable: Rc<MutableDisposable<T>>,
    is_disposed: Cell<bool>,
}

impl<T: IDisposable + ?Sized + 'static> MandatoryMutableDisposable<T> {
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

impl<T: IDisposable + ?Sized + 'static> IDisposable for MandatoryMutableDisposable<T> {
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
    /// `{ object, dispose }`: o `release` é de chamada única, como o `dispose` de uma `IReference`.
    pub fn new(object: T, release: DisposableRef) -> Self {
        Self { object, release }
    }

    /// `ImmortalReference`: referência cujo descarte não faz nada.
    pub fn immortal(object: T) -> Self {
        Self { object, release: none() }
    }
}

impl<T: Clone> Clone for Reference<T> {
    /// A cópia divide o descarte (o mesmo `dispose` de chamada única).
    fn clone(&self) -> Self {
        Self { object: self.object.clone(), release: self.release.clone() }
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
        Self::with_factory(Rc::new(factory))
    }

    /// Como [`ReferenceCollection::new`], com a fábrica compartilhada: quem a cria (a subclasse do original, que é a
    /// coleção e a fábrica ao mesmo tempo) continua com o próprio `Rc` para ler o estado dela.
    pub fn with_factory(factory: Rc<F>) -> Self {
        Self { references: Rc::new(RefCell::new(HashMap::new())), factory }
    }

    /// `acquire(key)`. O descarte da referência é de chamada única (`createSingleCallFunction`).
    pub fn acquire(&self, key: &str) -> Reference<T> {
        self.acquire_with(key, || self.factory.create_referenced_object(key))
    }

    /// `acquire(key, ...args)`: `create` é o `createReferencedObject(key, ...args)`, chamado só quando a chave ainda não
    /// tem objeto.
    pub fn acquire_with(&self, key: &str, create: impl FnOnce() -> T) -> Reference<T> {
        let object = {
            let existing = self.references.borrow().get(key).map(|(o, _)| o.clone());
            match existing {
                Some(o) => o,
                None => {
                    let o = create();
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
        let release = plain_disposable(move || {
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

/// `AsyncReferenceCollection<T>`: uma `ReferenceCollection` de promessas, com a referência já resolvida.
pub struct AsyncReferenceCollection<T: Clone + 'static, F: ReferencedObjects<Promise<T>> + 'static> {
    reference_collection: Rc<ReferenceCollection<Promise<T>, F>>,
}

impl<T: Clone + 'static, F: ReferencedObjects<Promise<T>> + 'static> AsyncReferenceCollection<T, F> {
    pub fn new(reference_collection: Rc<ReferenceCollection<Promise<T>, F>>) -> Self {
        Self { reference_collection }
    }

    /// `acquire(key)`: a referência é adquirida já (de forma síncrona) e a promessa resolve quando o objeto resolve.
    pub fn acquire(&self, key: &str) -> Promise<Reference<T>> {
        Self::await_object(self.reference_collection.acquire(key))
    }

    /// `acquire(key, ...args)`, com o `create` de [`ReferenceCollection::acquire_with`].
    pub fn acquire_with(&self, key: &str, create: impl FnOnce() -> Promise<T>) -> Promise<Reference<T>> {
        Self::await_object(self.reference_collection.acquire_with(key, create))
    }

    /// O corpo `async` do `acquire`: se o objeto falha, a referência é solta antes de o erro subir.
    fn await_object(reference: Reference<Promise<T>>) -> Promise<Reference<T>> {
        async_fn(async move {
            match reference.object.clone().await {
                Ok(object) => {
                    let release: DisposableRef = Rc::new(reference);
                    Ok(Reference::new(object, release))
                }
                Err(error) => {
                    reference.dispose();
                    Err(error)
                }
            }
        })
    }
}

/// `disposeOnReturn(fn)`.
pub fn dispose_on_return(f: impl FnOnce(&DisposableStore)) {
    let store = DisposableStore::new();
    let result = errors::catch(|| f(&store));
    store.dispose();
    if let Err(e) = result {
        errors::throw(e);
    }
}

/// Chave por identidade do objeto (o `Map`/`Set` do JS com objetos como chave): igualdade e hash pelo
/// endereço do `Rc`, não pelo valor.
pub struct ById<T: ?Sized>(pub Rc<T>);

impl<T: ?Sized> Clone for ById<T> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl<T: ?Sized> PartialEq for ById<T> {
    fn eq(&self, other: &Self) -> bool {
        id_of(&self.0) == id_of(&other.0)
    }
}

impl<T: ?Sized> Eq for ById<T> {}

impl<T: ?Sized> Hash for ById<T> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        id_of(&self.0).hash(state);
    }
}

/// `DisposableMap<K, V>`: um mapa que descarta os valores que guarda.
pub struct DisposableMap<K: Eq + Hash + Clone + 'static, V: IDisposable + ?Sized + 'static> {
    store: RefCell<LiveMap<K, Rc<V>>>,
    is_disposed: Cell<bool>,
}

impl<K: Eq + Hash + Clone + 'static, V: IDisposable + ?Sized + 'static> Default for DisposableMap<K, V> {
    fn default() -> Self {
        Self { store: RefCell::new(LiveMap::default()), is_disposed: Cell::new(false) }
    }
}

impl<K: Eq + Hash + Clone + 'static, V: IDisposable + ?Sized + 'static> DisposableMap<K, V> {
    pub fn new() -> Rc<Self> {
        track_disposable(Rc::new(Self::default()))
    }

    /// `clearAndDisposeAll()`.
    pub fn clear_and_dispose_all(&self) {
        if self.is_empty() {
            return;
        }
        dispose_live_and_clear(&self.store, |v| v.dispose());
    }

    pub fn has(&self, key: &K) -> bool {
        self.store.borrow().get(key).is_some()
    }

    pub fn len(&self) -> usize {
        self.store.borrow().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn get(&self, key: &K) -> Option<Rc<V>> {
        self.store.borrow().get(key).cloned()
    }

    /// `set(key, value, skipDisposeOnOverwrite)`: descarta o valor anterior antes de guardar o novo.
    pub fn set(&self, key: K, value: Rc<V>, skip_dispose_on_overwrite: bool) {
        if self.is_disposed.get() {
            eprintln!("Error: Trying to add a disposable to a DisposableMap that has already been disposed of. The added object will be leaked!");
        }
        if !skip_dispose_on_overwrite && let Some(old) = self.get(&key) {
            old.dispose();
        }
        set_parent_of_disposable(id_of(&value), Some(id_of_ref(self)));
        self.store.borrow_mut().insert(key, value);
    }

    /// `deleteAndDispose(key)`: descarta com o item ainda no mapa e só depois tira.
    pub fn delete_and_dispose(&self, key: &K) {
        if let Some(value) = self.get(key) {
            value.dispose();
        }
        self.store.borrow_mut().remove(key);
    }

    /// `deleteAndLeak(key)`.
    pub fn delete_and_leak(&self, key: &K) -> Option<Rc<V>> {
        let value = self.get(key);
        if let Some(value) = &value {
            set_parent_of_disposable(id_of(value), None);
        }
        self.store.borrow_mut().remove(key);
        value
    }

    pub fn keys(&self) -> Vec<K> {
        self.store.borrow().keys()
    }

    pub fn values(&self) -> Vec<Rc<V>> {
        self.store.borrow().values()
    }
}

impl<K: Eq + Hash + Clone + 'static, V: IDisposable + ?Sized + 'static> IDisposable for DisposableMap<K, V> {
    fn dispose(&self) {
        mark_as_disposed(id_of_ref(self));
        self.is_disposed.set(true);
        self.clear_and_dispose_all();
    }
}

/// `DisposableSet<V>`: um conjunto que descarta os valores que guarda. A identidade do valor é a do `Rc`.
pub struct DisposableSet<V: IDisposable + ?Sized + 'static> {
    store: RefCell<LiveMap<DisposableId, Rc<V>>>,
    is_disposed: Cell<bool>,
}

impl<V: IDisposable + ?Sized + 'static> Default for DisposableSet<V> {
    fn default() -> Self {
        Self { store: RefCell::new(LiveMap::default()), is_disposed: Cell::new(false) }
    }
}

impl<V: IDisposable + ?Sized + 'static> DisposableSet<V> {
    pub fn new() -> Rc<Self> {
        track_disposable(Rc::new(Self::default()))
    }

    /// `clearAndDisposeAll()`.
    pub fn clear_and_dispose_all(&self) {
        if self.size() == 0 {
            return;
        }
        dispose_live_and_clear(&self.store, |v| v.dispose());
    }

    pub fn has(&self, value: &Rc<V>) -> bool {
        self.store.borrow().get(&id_of(value)).is_some()
    }

    /// `size`.
    pub fn size(&self) -> usize {
        self.store.borrow().len()
    }

    pub fn add(&self, value: Rc<V>) {
        if self.is_disposed.get() {
            eprintln!("Error: Trying to add a disposable to a DisposableSet that has already been disposed of. The added object will be leaked!");
        }
        set_parent_of_disposable(id_of(&value), Some(id_of_ref(self)));
        let id = id_of(&value);
        self.store.borrow_mut().insert(id, value);
    }

    /// `deleteAndDispose(value)`: tira do conjunto e descarta.
    pub fn delete_and_dispose(&self, value: &Rc<V>) {
        if self.store.borrow_mut().remove(&id_of(value)).is_some() {
            value.dispose();
        }
    }

    /// `deleteAndLeak(value)`: tira do conjunto sem descartar; quem recebe descarta.
    pub fn delete_and_leak(&self, value: &Rc<V>) -> Option<Rc<V>> {
        let removed = self.store.borrow_mut().remove(&id_of(value));
        if removed.is_some() {
            set_parent_of_disposable(id_of(value), None);
        }
        removed
    }

    /// `values()` (e o `Symbol.iterator`).
    pub fn values(&self) -> Vec<Rc<V>> {
        self.store.borrow().values()
    }
}

impl<V: IDisposable + ?Sized + 'static> IDisposable for DisposableSet<V> {
    fn dispose(&self) {
        mark_as_disposed(id_of_ref(self));
        self.is_disposed.set(true);
        self.clear_and_dispose_all();
    }
}

/// `thenIfNotDisposed(promise, then)`: chama `then` no resultado, a menos que o disposable devolvido
/// seja descartado antes.
pub fn then_if_not_disposed<T: Clone + 'static>(promise: &Promise<T>, then: impl FnOnce(T) + 'static) -> DisposableRef {
    let disposed = Rc::new(Cell::new(false));
    let flag = disposed.clone();
    promise.then(move |result| {
        if !flag.get() {
            then(result);
        }
    });
    to_disposable(move || disposed.set(true))
}

/// `thenRegisterOrDispose(promise, store)`: registra o disposable no store, ou descarta se o store já
/// foi descartado.
pub fn then_register_or_dispose(promise: &Promise<DisposableRef>, store: Rc<DisposableStore>) -> Promise<DisposableRef> {
    promise.then(move |disposable| {
        if store.is_disposed() {
            disposable.dispose();
        } else {
            store.add(disposable.clone());
        }
        disposable
    })
}
