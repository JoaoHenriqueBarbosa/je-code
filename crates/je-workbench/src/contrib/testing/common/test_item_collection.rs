//! Porte de `src/vs/workbench/contrib/testing/common/testItemCollection.ts`: a coleção de itens de teste de um
//! controlador, que transforma as mudanças feitas na árvore de itens (adicionar, remover, atualizar propriedades,
//! resolver filhos) num diff (`TestsDiff`) para a thread principal.
//!
//! Divergências:
//! - **`T`.** O item do controlador (`T extends ITestItemLike`) é um handle barato de clonar (um `Rc`). O que o original
//!   lê dele, que no TypeScript é estrutural, é o trait [`ITestItemLike`] (que estende o [`TestItemLike`] do `testId`),
//!   com [`ITestItemLike::is_same`] para o `===` e [`ITestItemLike::set_can_resolve_children`] para `canResolveChildren = true`.
//! - **`ITestItemApi<T>`** é compartilhado (`Rc`) e os campos que a coleção muta (`parent`, `listener`) ficam em células.
//!   O `listener` captura o item interno e a coleção por referência fraca (o original os captura fortes, e o coletor do
//!   JavaScript resolve o ciclo); `dispose` e a remoção do item os limpam.
//! - **`ExtHostTestItemEvent`** é o enum [`ExtHostTestItemEvent`] e `TestItemEventOp` é [`TestItemEventOp`] (a
//!   discriminante, por [`ExtHostTestItemEvent::op`]). Os `ops` de um `Bulk` são eventos (`Upsert` ou `RemoveChild`).
//! - **`protected diff`** é [`TestItemCollection::diff`] e [`TestItemCollection::set_diff`], públicos porque Rust não tem
//!   herança (o `TestTestCollection` do `testStubs.ts` os expõe como `currentDiff` e `setDiff`).
//! - **`resolveHandler`** é [`TestItemCollection::set_resolve_handler`] e [`TestItemCollection::resolve_handler`]. O retorno
//!   `Promise<void> | void` é [`MaybePromise`]; uma exceção síncrona do tratador é capturada com `errors::catch`.
//! - **`expand`** devolve `Option<Promise<()>>` (`Promise<void> | void`; `None` é `void`).
//! - **Identidade do `resolveBarrier`.** O `Barrier` não tem comparação por identidade; cada barreira criada leva um número
//!   (`resolve_barrier_id`) e `isCurrent` o compara.
//! - **`diffableProps.error`** compara com `strictEqualComparator`; o porte compara o [`TestMessageText`] por valor (um
//!   markdown é objeto no original e compara por identidade).
//! - **`createTestItemChildren(api, getApi, checkCtor)`** não recebe o `checkCtor` (o `instanceof` do original confere o
//!   tipo do item; o tipo `T` já o garante). O `InvalidTestItemError` fica como função para quem cria itens por fora.
//! - **`console.error`** é [`je_base::console::error`] (o texto e o erro em texto), que um teste captura com o
//!   `ErrorSpy` (o `sinon.stub(console, 'error')`).

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::{Rc, Weak};

use indexmap::{IndexMap, IndexSet};
use je_base::async_::Barrier;
use je_base::async_::RunOnceScheduler;
use je_base::error_message::{error_to_dynamic, to_error_message};
use je_base::errors::{self, Error, throw};
use je_base::event::{Emitter, Event};
use je_base::lifecycle::{Disposable, IDisposable};
use je_base::promise::{MaybePromise, Promise};
use je_base::runtime;
use je_base::uri::URI;
use je_editor::range::Range;

use super::test_id::{TestId, TestItemLike};
use super::test_types::{
    ITestItem, ITestItemUpdate, ITestTag, ITestTagDisplayInfo, InternalTestItem, PartialTestItem, TestItemExpandState, TestMessageText, TestsDiff, TestsDiffOp,
    apply_test_item_update, apply_test_item_update_to_update, namespace_test_tag,
};

/// `CollectionItem<T>` (`@private`).
pub struct CollectionItem<T> {
    pub full_id: TestId,
    pub actual: T,
    pub expand: TestItemExpandState,
    /// Number of levels of items below this one that are expanded. May be infinite.
    pub expand_levels: Option<f64>,
    pub resolve_barrier: Option<Barrier>,
    resolve_barrier_id: u64,
    pub resolve_error: Option<String>,
}

pub type CollectionItemRef<T> = Rc<RefCell<CollectionItem<T>>>;

/// `TestItemEventOp`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TestItemEventOp {
    Upsert,
    SetTags,
    UpdateCanResolveChildren,
    RemoveChild,
    SetProp,
    Bulk,
    DocumentSynced,
}

/// `ExtHostTestItemEvent`.
pub enum ExtHostTestItemEvent<T> {
    /// `ITestItemSetTags`.
    SetTags { new: Vec<ITestTag>, old: Vec<ITestTag> },
    /// `ITestItemUpsertChild`.
    Upsert { item: T },
    /// `ITestItemRemoveChild`.
    RemoveChild { id: String },
    /// `ITestItemUpdateCanResolveChildren`.
    UpdateCanResolveChildren { state: bool },
    /// `ITestItemSetProp`.
    SetProp { update: PartialTestItem },
    /// `ITestItemBulkReplace`: os `ops` são `Upsert` ou `RemoveChild`.
    Bulk { ops: Vec<ExtHostTestItemEvent<T>> },
    /// `ITestItemDocumentSynced`.
    DocumentSynced,
}

impl<T> ExtHostTestItemEvent<T> {
    pub fn op(&self) -> TestItemEventOp {
        match self {
            ExtHostTestItemEvent::SetTags { .. } => TestItemEventOp::SetTags,
            ExtHostTestItemEvent::Upsert { .. } => TestItemEventOp::Upsert,
            ExtHostTestItemEvent::RemoveChild { .. } => TestItemEventOp::RemoveChild,
            ExtHostTestItemEvent::UpdateCanResolveChildren { .. } => TestItemEventOp::UpdateCanResolveChildren,
            ExtHostTestItemEvent::SetProp { .. } => TestItemEventOp::SetProp,
            ExtHostTestItemEvent::Bulk { .. } => TestItemEventOp::Bulk,
            ExtHostTestItemEvent::DocumentSynced => TestItemEventOp::DocumentSynced,
        }
    }
}

pub type TestItemListener<T> = Rc<dyn Fn(&ExtHostTestItemEvent<T>)>;

/// `ITestItemApi<T>`.
pub struct ITestItemApi<T> {
    pub controller_id: String,
    pub parent: RefCell<Option<T>>,
    pub listener: RefCell<Option<TestItemListener<T>>>,
}

impl<T> ITestItemApi<T> {
    pub fn new(controller_id: &str) -> Rc<Self> {
        Rc::new(Self { controller_id: controller_id.to_string(), parent: RefCell::new(None), listener: RefCell::new(None) })
    }

    /// `api.listener?.(evt)`.
    pub fn notify(&self, evt: &ExtHostTestItemEvent<T>) {
        let listener = self.listener.borrow().clone();
        if let Some(listener) = listener {
            listener(evt);
        }
    }
}

/// `ITestItemCollectionOptions<T>`.
pub struct ITestItemCollectionOptions<T> {
    /// Controller ID to use to prefix these test items.
    pub controller_id: String,

    /// Gets the document version at the given URI, if it's open
    pub get_document_version: Rc<dyn Fn(Option<&URI>) -> Option<f64>>,

    /// Gets API for the given test item, used to listen for events and set parents.
    pub get_api_for: Rc<dyn Fn(&T) -> Rc<ITestItemApi<T>>>,

    /// Converts the full test item to the common interface.
    pub to_i_test_item: Rc<dyn Fn(&T) -> ITestItem>,

    /// Gets children for the item.
    pub get_children: Rc<dyn Fn(&T) -> Rc<dyn ITestChildrenLike<T>>>,

    /// Root to use for the new test collection.
    pub root: T,
}

fn range_equals(a: &Option<Range>, b: &Option<Range>) -> bool {
    a == b
}

fn tags_equal(a: &[String], b: &[String]) -> bool {
    if a.len() != b.len() {
        return false;
    }

    if a.iter().any(|t1| !b.contains(t1)) {
        return false;
    }

    true
}

/// `diffTestItems`.
fn diff_test_items(a: &ITestItem, b: &ITestItem) -> Option<PartialTestItem> {
    let mut output: Option<PartialTestItem> = None;
    {
        let mut set = |apply: &dyn Fn(&mut PartialTestItem)| apply(output.get_or_insert_with(PartialTestItem::default));
        if !range_equals(&a.range, &b.range) {
            set(&|o| o.range = Some(b.range));
        }
        if a.busy != b.busy {
            set(&|o| o.busy = Some(b.busy));
        }
        if a.label != b.label {
            set(&|o| o.label = Some(b.label.clone()));
        }
        if a.description != b.description {
            set(&|o| o.description = Some(b.description.clone()));
        }
        if a.error != b.error {
            set(&|o| o.error = Some(b.error.clone()));
        }
        if a.sort_text != b.sort_text {
            set(&|o| o.sort_text = Some(b.sort_text.clone()));
        }
        if !tags_equal(&a.tags, &b.tags) {
            set(&|o| o.tags = Some(b.tags.clone()));
        }
    }

    output
}

/// `ITestChildrenLike<T>`: `Iterable<[string, T]>` mais `get` e `delete`.
pub trait ITestChildrenLike<T> {
    fn entries(&self) -> Vec<(String, T)>;
    fn get(&self, id: &str) -> Option<T>;
    fn delete(&self, id: &str);
}

/// `ITestItemLike`.
pub trait ITestItemLike: TestItemLike + Clone + 'static {
    fn tags(&self) -> Vec<ITestTag>;
    fn uri(&self) -> Option<URI>;
    fn can_resolve_children(&self) -> bool;
    fn set_can_resolve_children(&self, value: bool);
    /// `a === b`.
    fn is_same(&self, other: &Self) -> bool;
}

/// `Promise<void> | void` de volta para o `MaybePromise` do `then`.
fn maybe(promise: Option<Promise<()>>) -> MaybePromise<()> {
    match promise {
        Some(promise) => MaybePromise::Promise(promise),
        None => MaybePromise::Value(()),
    }
}

/// O `_resolveHandler`: `(item: T | undefined) => Promise<void> | void`.
pub type ResolveHandler<T> = Rc<dyn Fn(Option<&T>) -> MaybePromise<()>>;

struct TagRef {
    ref_count: usize,
}

/// Maintains a collection of test items for a single controller.
pub struct TestItemCollection<T: ITestItemLike> {
    this: Weak<TestItemCollection<T>>,
    base: Rc<Disposable>,
    debounce_send_diff: Rc<RunOnceScheduler>,
    diff_op_emitter: Rc<Emitter<TestsDiff>>,
    resolve_handler: RefCell<Option<ResolveHandler<T>>>,
    options: ITestItemCollectionOptions<T>,
    barrier_counter: Cell<u64>,

    pub tree: RefCell<IndexMap<String, CollectionItemRef<T>>>,
    tags: RefCell<HashMap<String, TagRef>>,

    diff: RefCell<TestsDiff>,
}

impl<T: ITestItemLike> TestItemCollection<T> {
    pub fn new(options: ITestItemCollectionOptions<T>) -> Rc<Self> {
        let base = Disposable::new();
        let diff_op_emitter = Emitter::new();
        base.register(diff_op_emitter.clone());

        let this = Rc::new_cyclic(|me: &Weak<TestItemCollection<T>>| {
            let flush_target = me.clone();
            let debounce_send_diff = RunOnceScheduler::new(
                move || {
                    if let Some(collection) = flush_target.upgrade() {
                        collection.flush_diff();
                    }
                },
                200,
            );
            base.register(debounce_send_diff.clone());

            Self {
                this: me.clone(),
                base: base.clone(),
                debounce_send_diff,
                diff_op_emitter,
                resolve_handler: RefCell::new(None),
                options,
                barrier_counter: Cell::new(0),
                tree: RefCell::new(IndexMap::new()),
                tags: RefCell::new(HashMap::new()),
                diff: RefCell::new(Vec::new()),
            }
        });

        this.root().set_can_resolve_children(true);
        this.upsert_item(&this.root().clone(), None);
        this
    }

    pub fn root(&self) -> &T {
        &self.options.root
    }

    /// Handler used for expanding test items.
    pub fn set_resolve_handler(&self, handler: Option<ResolveHandler<T>>) {
        let changed = match (&*self.resolve_handler.borrow(), &handler) {
            (None, None) => false,
            (Some(current), Some(next)) => !Rc::ptr_eq(current, next),
            _ => true,
        };
        *self.resolve_handler.borrow_mut() = handler;
        let tests: Vec<CollectionItemRef<T>> = self.tree.borrow().values().cloned().collect();
        for test in tests {
            let needs_reset = {
                let test = test.borrow();
                test.resolve_error.is_some() || (changed && test.resolve_barrier.as_ref().is_some_and(|barrier| !barrier.is_open()))
            };
            if needs_reset {
                test.borrow_mut().resolve_barrier = None;
                self.set_resolve_error(&test, None);
            }
            self.update_expandability(&test);
        }
    }

    pub fn resolve_handler(&self) -> Option<ResolveHandler<T>> {
        self.resolve_handler.borrow().clone()
    }

    /// Fires when an operation happens that should result in a diff.
    pub fn on_did_generate_diff(&self) -> Event<TestsDiff> {
        self.diff_op_emitter.event()
    }

    /// `protected diff`: o diff acumulado.
    pub fn diff(&self) -> TestsDiff {
        self.diff.borrow().clone()
    }

    /// `this.diff = diff`.
    pub fn set_diff(&self, diff: TestsDiff) {
        *self.diff.borrow_mut() = diff;
    }

    /// Gets a diff of all changes that have been made, and clears the diff queue.
    pub fn collect_diff(&self) -> TestsDiff {
        std::mem::take(&mut *self.diff.borrow_mut())
    }

    /// Pushes a new diff entry onto the collected diff list.
    pub fn push_diff(&self, diff: TestsDiffOp) {
        match &diff {
            TestsDiffOp::DocumentSynced { uri, docv } => {
                for existing in self.diff.borrow_mut().iter_mut() {
                    if let TestsDiffOp::DocumentSynced { uri: existing_uri, docv: existing_docv } = existing
                        && URI::ptr_eq(existing_uri, uri)
                    {
                        *existing_docv = *docv;
                        return;
                    }
                }
            }
            TestsDiffOp::Update { item } => {
                // Try to merge updates, since they're invoked per-property
                let mut current = self.diff.borrow_mut();
                if let Some(last) = current.last_mut() {
                    match last {
                        TestsDiffOp::Update { item: last_item } if last_item.ext_id == item.ext_id => {
                            apply_test_item_update_to_update(last_item, item);
                            return;
                        }
                        TestsDiffOp::Add { item: last_item } if last_item.item.ext_id == item.ext_id => {
                            apply_test_item_update(last_item, item);
                            return;
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
        }

        self.diff.borrow_mut().push(diff);

        if !self.debounce_send_diff.is_scheduled() {
            self.debounce_send_diff.schedule(None);
        }
    }

    /// Expands the test and the given number of `levels` of children. If levels
    /// is < 0, then all children will be expanded. If it's 0, then only this
    /// item will be expanded.
    pub fn expand(&self, test_id: &str, levels: f64) -> Option<Promise<()>> {
        let internal = self.tree.borrow().get(test_id).cloned()?;

        {
            let mut internal = internal.borrow_mut();
            if internal.expand_levels.is_none_or(|current| levels > current) {
                internal.expand_levels = Some(levels);
            }
        }

        let this = self.this.upgrade().expect("a coleção está viva enquanto é usada");

        // try to avoid awaiting things if the provider returns synchronously in
        // order to keep everything in a single diff and DOM update.
        let expand = internal.borrow().expand;
        if expand == TestItemExpandState::Expandable {
            let r = self.resolve_children(&internal);
            if !r.is_open() {
                let internal = internal.clone();
                return Some(r.wait().then_maybe(move |_| maybe(this.expand_children(&internal, levels - 1.0))));
            }
            return self.expand_children(&internal, levels - 1.0);
        } else if expand == TestItemExpandState::Expanded || expand == TestItemExpandState::BusyExpanding {
            let barrier = internal.borrow().resolve_barrier.clone();
            return match barrier {
                Some(barrier) if !barrier.is_open() => {
                    let internal = internal.clone();
                    Some(barrier.wait().then_maybe(move |_| maybe(this.expand_children(&internal, levels - 1.0))))
                }
                _ => self.expand_children(&internal, levels - 1.0),
            };
        }

        None
    }

    fn on_test_item_event(&self, internal: &CollectionItemRef<T>, evt: &ExtHostTestItemEvent<T>) {
        match evt {
            ExtHostTestItemEvent::RemoveChild { id } => {
                let full_id = internal.borrow().full_id.clone();
                self.remove_item(&TestId::join_to_string(&full_id, id));
            }

            ExtHostTestItemEvent::Upsert { item } => self.upsert_item(item, Some(internal)),

            ExtHostTestItemEvent::Bulk { ops } => {
                for op in ops {
                    self.on_test_item_event(internal, op);
                }
            }

            ExtHostTestItemEvent::SetTags { new, old } => {
                let full_id = internal.borrow().full_id.to_string();
                self.diff_tag_refs(new, old, &full_id);
            }

            ExtHostTestItemEvent::UpdateCanResolveChildren { .. } => self.update_expandability(internal),

            ExtHostTestItemEvent::SetProp { update } => {
                let (ext_id, resolve_error) = {
                    let internal = internal.borrow();
                    (internal.full_id.to_string(), internal.resolve_error.clone())
                };
                let item = match (&update.error, resolve_error) {
                    (Some(None), Some(resolve_error)) => PartialTestItem { error: Some(Some(TestMessageText::Text(resolve_error))), ..update.clone() },
                    _ => update.clone(),
                };
                self.push_diff(TestsDiffOp::Update { item: ITestItemUpdate { ext_id, expand: None, item: Some(item) } });
            }

            ExtHostTestItemEvent::DocumentSynced => {
                let uri = internal.borrow().actual.uri();
                self.document_synced(uri.as_ref());
            }
        }
    }

    fn document_synced(&self, uri: Option<&URI>) {
        if let Some(uri) = uri {
            self.push_diff(TestsDiffOp::DocumentSynced { uri: uri.clone(), docv: (self.options.get_document_version)(Some(uri)) });
        }
    }

    fn api_for(&self, item: &T) -> Rc<ITestItemApi<T>> {
        (self.options.get_api_for)(item)
    }

    fn children_of(&self, item: &T) -> Rc<dyn ITestChildrenLike<T>> {
        (self.options.get_children)(item)
    }

    fn to_i_test_item(&self, item: &T) -> ITestItem {
        (self.options.to_i_test_item)(item)
    }

    fn upsert_item(&self, actual: &T, parent: Option<&CollectionItemRef<T>>) {
        let parent_actual = parent.map(|p| p.borrow().actual.clone());
        let parent_dyn: Option<Rc<dyn TestItemLike>> = parent_actual.clone().map(|p| Rc::new(p) as Rc<dyn TestItemLike>);
        let full_id = TestId::from_ext_host_test_item(actual, &self.root().id(), parent_dyn);
        let full_id_string = full_id.to_string();

        // If this test item exists elsewhere in the tree already (exists at an
        // old ID with an existing parent), remove that old item.
        let private_api = self.api_for(actual);
        let existing_parent = private_api.parent.borrow().clone();
        if let Some(existing_parent) = existing_parent {
            let same_parent = parent_actual.as_ref().is_some_and(|p| p.is_same(&existing_parent));
            if !same_parent {
                self.children_of(&existing_parent).delete(&actual.id());
            }
        }

        let existing = self.tree.borrow().get(&full_id_string).cloned();
        // Case 1: a brand new item
        let Some(internal) = existing else {
            let parent_levels = parent.and_then(|p| p.borrow().expand_levels);
            let internal: CollectionItemRef<T> = Rc::new(RefCell::new(CollectionItem {
                full_id: full_id.clone(),
                actual: actual.clone(),
                // intentionally undefined or 0
                expand_levels: parent_levels.filter(|levels| *levels != 0.0 && !levels.is_nan()).map(|levels| levels - 1.0),
                expand: TestItemExpandState::NotExpandable, // updated by `connectItemAndChildren`
                resolve_barrier: None,
                resolve_barrier_id: 0,
                resolve_error: None,
            }));

            for tag in actual.tags() {
                self.increment_tag_refs(&tag);
            }
            self.tree.borrow_mut().insert(full_id_string.clone(), internal.clone());
            self.set_item_parent(actual, parent);
            let expand = internal.borrow().expand;
            self.push_diff(TestsDiffOp::Add {
                item: InternalTestItem { controller_id: self.options.controller_id.clone(), expand, item: self.to_i_test_item(actual) },
            });

            self.connect_item_and_children(actual, &internal, parent);
            return;
        };

        // Case 2: re-insertion of an existing item, no-op
        let current_actual = internal.borrow().actual.clone();
        if current_actual.is_same(actual) {
            self.connect_item(actual, &internal, parent); // re-connect in case the parent changed
            return; // no-op
        }

        // Case 3: upsert of an existing item by ID, with a new instance
        if current_actual.uri().map(|uri| uri.to_string(false)) != actual.uri().map(|uri| uri.to_string(false)) {
            // If the item has a new URI, re-insert it; we don't support updating
            // URIs on existing test items.
            self.remove_item(&full_id_string);
            return self.upsert_item(actual, parent);
        }
        let old_children = self.children_of(&current_actual);
        let old_actual = current_actual;
        let mut old_item = self.to_i_test_item(&old_actual);
        if old_item.error.is_none() {
            old_item.error = internal.borrow().resolve_error.clone().map(TestMessageText::Text);
        }
        let update = diff_test_items(&old_item, &self.to_i_test_item(actual));
        *self.api_for(&old_actual).listener.borrow_mut() = None;

        {
            let mut internal = internal.borrow_mut();
            internal.actual = actual.clone();
            internal.resolve_barrier = None;
            internal.resolve_error = None;
            internal.expand = TestItemExpandState::NotExpandable; // updated by `connectItemAndChildren`
        }

        if let Some(mut update) = update {
            // tags are handled in a special way
            if update.tags.is_some() {
                self.diff_tag_refs(&actual.tags(), &old_actual.tags(), &full_id_string);
                update.tags = None;
            }
            self.on_test_item_event(&internal, &ExtHostTestItemEvent::SetProp { update });
        }

        self.connect_item_and_children(actual, &internal, parent);

        // Remove any orphaned children.
        for (_, child) in old_children.entries() {
            if self.children_of(actual).get(&child.id()).is_none() {
                self.remove_item(&TestId::join_to_string(&full_id, &child.id()));
            }
        }

        // Re-expand the element if it was previous expanded (#207574)
        let expand_levels = internal.borrow().expand_levels;
        if let Some(expand_levels) = expand_levels {
            // Wait until a microtask to allow the extension to finish setting up
            // properties of the element and children before we ask it to expand.
            let collection = self.this.clone();
            let internal = internal.clone();
            let id = full_id_string.clone();
            runtime::queue_microtask(move || {
                if internal.borrow().expand == TestItemExpandState::Expandable {
                    internal.borrow_mut().expand_levels = None;
                    if let Some(collection) = collection.upgrade() {
                        collection.expand(&id, expand_levels);
                    }
                }
            });
        }

        // Mark ranges in the document as synced (#161320)
        let uri = internal.borrow().actual.uri();
        self.document_synced(uri.as_ref());
    }

    fn diff_tag_refs(&self, new_tags: &[ITestTag], old_tags: &[ITestTag], ext_id: &str) {
        let mut to_delete: IndexSet<String> = old_tags.iter().map(|t| t.id.clone()).collect();
        for tag in new_tags {
            if !to_delete.shift_remove(&tag.id) {
                self.increment_tag_refs(tag);
            }
        }

        self.push_diff(TestsDiffOp::Update {
            item: ITestItemUpdate {
                ext_id: ext_id.to_string(),
                expand: None,
                item: Some(PartialTestItem {
                    tags: Some(new_tags.iter().map(|v| namespace_test_tag(&self.options.controller_id, &v.id)).collect()),
                    ..PartialTestItem::default()
                }),
            },
        });

        for tag_id in to_delete {
            self.decrement_tag_refs(&tag_id);
        }
    }

    fn increment_tag_refs(&self, tag: &ITestTag) {
        let existing = {
            let mut tags = self.tags.borrow_mut();
            match tags.get_mut(&tag.id) {
                Some(existing) => {
                    existing.ref_count += 1;
                    true
                }
                None => {
                    tags.insert(tag.id.clone(), TagRef { ref_count: 1 });
                    false
                }
            }
        };
        if !existing {
            self.push_diff(TestsDiffOp::AddTag { tag: ITestTagDisplayInfo { id: namespace_test_tag(&self.options.controller_id, &tag.id) } });
        }
    }

    fn decrement_tag_refs(&self, tag_id: &str) {
        let removed = {
            let mut tags = self.tags.borrow_mut();
            match tags.get_mut(tag_id) {
                Some(existing) => {
                    existing.ref_count -= 1;
                    if existing.ref_count == 0 {
                        tags.remove(tag_id);
                        true
                    } else {
                        false
                    }
                }
                None => false,
            }
        };
        if removed {
            self.push_diff(TestsDiffOp::RemoveTag { id: namespace_test_tag(&self.options.controller_id, tag_id) });
        }
    }

    fn set_item_parent(&self, actual: &T, parent: Option<&CollectionItemRef<T>>) {
        let parent_actual = parent.map(|p| p.borrow().actual.clone()).filter(|p| !p.is_same(self.root()));
        *self.api_for(actual).parent.borrow_mut() = parent_actual;
    }

    fn connect_item(&self, actual: &T, internal: &CollectionItemRef<T>, parent: Option<&CollectionItemRef<T>>) {
        self.set_item_parent(actual, parent);
        let api = self.api_for(actual);
        *api.parent.borrow_mut() = parent.map(|p| p.borrow().actual.clone());
        let collection = self.this.clone();
        let target = Rc::downgrade(internal);
        *api.listener.borrow_mut() = Some(Rc::new(move |evt: &ExtHostTestItemEvent<T>| {
            if let (Some(collection), Some(internal)) = (collection.upgrade(), target.upgrade()) {
                collection.on_test_item_event(&internal, evt);
            }
        }));
        self.update_expandability(internal);
    }

    fn connect_item_and_children(&self, actual: &T, internal: &CollectionItemRef<T>, parent: Option<&CollectionItemRef<T>>) {
        self.connect_item(actual, internal, parent);

        // Discover any existing children that might have already been added
        for (_, child) in self.children_of(actual).entries() {
            self.upsert_item(&child, Some(internal));
        }
    }

    /// Updates the `expand` state of the item. Should be called whenever the
    /// resolved state of the item changes. Can automatically expand the item
    /// if requested by a consumer.
    fn update_expandability(&self, internal: &CollectionItemRef<T>) {
        let new_state = {
            let internal = internal.borrow();
            if self.resolve_handler.borrow().is_none() {
                TestItemExpandState::NotExpandable
            } else if let Some(barrier) = &internal.resolve_barrier {
                if barrier.is_open() { TestItemExpandState::Expanded } else { TestItemExpandState::BusyExpanding }
            } else if internal.actual.can_resolve_children() {
                TestItemExpandState::Expandable
            } else {
                TestItemExpandState::NotExpandable
            }
        };

        let (ext_id, expand_levels) = {
            let mut internal = internal.borrow_mut();
            if new_state == internal.expand {
                return;
            }

            internal.expand = new_state;
            (internal.full_id.to_string(), internal.expand_levels)
        };
        self.push_diff(TestsDiffOp::Update { item: ITestItemUpdate { ext_id, expand: Some(new_state), item: None } });

        if new_state == TestItemExpandState::Expandable && expand_levels.is_some() {
            self.resolve_children(internal);
        }
    }

    /// Expands all children of the item, "levels" deep. If levels is 0, only
    /// the children will be expanded. If it's 1, the children and their children
    /// will be expanded. If it's <0, it's a no-op.
    fn expand_children(&self, internal: &CollectionItemRef<T>, levels: f64) -> Option<Promise<()>> {
        if levels < 0.0 {
            return None;
        }

        let (actual, full_id) = {
            let internal = internal.borrow();
            (internal.actual.clone(), internal.full_id.clone())
        };
        let mut expand_requests: Vec<Promise<()>> = Vec::new();
        for (_, child) in self.children_of(&actual).entries() {
            if let Some(promise) = self.expand(&TestId::join_to_string(&full_id, &child.id()), levels) {
                expand_requests.push(promise);
            }
        }

        if !expand_requests.is_empty() {
            return Some(Promise::all(expand_requests).then(|_| ()));
        }

        None
    }

    /// Calls `discoverChildren` on the item, refreshing all its tests.
    fn resolve_children(&self, internal: &CollectionItemRef<T>) -> Barrier {
        let existing = internal.borrow().resolve_barrier.clone();
        if let Some(existing) = existing {
            return existing;
        }

        let handler = self.resolve_handler.borrow().clone();
        let Some(handler) = handler else {
            let b = Barrier::new();
            b.open();
            return b;
        };

        internal.borrow_mut().expand = TestItemExpandState::BusyExpanding;
        self.push_expand_state_update(internal);

        let barrier = Barrier::new();
        let barrier_id = self.barrier_counter.get() + 1;
        self.barrier_counter.set(barrier_id);
        {
            let mut internal = internal.borrow_mut();
            internal.resolve_barrier = Some(barrier.clone());
            internal.resolve_barrier_id = barrier_id;
        }
        let this = self.this.upgrade().expect("a coleção está viva enquanto é usada");
        let is_current = {
            let this = this.clone();
            let internal = internal.clone();
            move || {
                let same_in_tree = {
                    let full_id = internal.borrow().full_id.to_string();
                    this.tree.borrow().get(&full_id).is_some_and(|current| Rc::ptr_eq(current, &internal))
                };
                let internal = internal.borrow();
                same_in_tree && internal.resolve_barrier.is_some() && internal.resolve_barrier_id == barrier_id
            }
        };
        let apply_error = {
            let this = this.clone();
            let internal = internal.clone();
            let is_current = is_current.clone();
            move |err: Error| {
                je_base::console::error(&format!("Unhandled error in resolveHandler of test controller \"{}\"", this.options.controller_id), &err.to_string());
                if is_current() {
                    this.set_resolve_error(&internal, Some(to_error_message(&error_to_dynamic(&err), false)));
                }
            }
        };
        let finish = {
            let this = this.clone();
            let internal = internal.clone();
            let barrier = barrier.clone();
            let is_current = is_current.clone();
            move || {
                barrier.open();
                if is_current() {
                    this.update_expandability(&internal);
                }
            }
        };

        let argument = {
            let actual = internal.borrow().actual.clone();
            if actual.is_same(self.root()) { None } else { Some(actual) }
        };
        match errors::catch(|| handler(argument.as_ref())) {
            Ok(MaybePromise::Promise(promise)) => {
                promise.catch(move |err| apply_error(err)).then(move |_| finish());
            }
            Ok(MaybePromise::Value(())) => finish(),
            Err(err) => {
                apply_error(err);
                finish();
            }
        }

        barrier
    }

    fn set_resolve_error(&self, internal: &CollectionItemRef<T>, error: Option<String>) {
        let actual = internal.borrow().actual.clone();
        let provider_error = self.to_i_test_item(&actual).error;
        let previous_error = provider_error.clone().or_else(|| internal.borrow().resolve_error.clone().map(TestMessageText::Text));
        internal.borrow_mut().resolve_error = error.clone();
        let current_error = provider_error.or_else(|| error.map(TestMessageText::Text));
        if current_error != previous_error {
            let ext_id = internal.borrow().full_id.to_string();
            self.push_diff(TestsDiffOp::Update {
                item: ITestItemUpdate { ext_id, expand: None, item: Some(PartialTestItem { error: Some(current_error), ..PartialTestItem::default() }) },
            });
        }
    }

    fn push_expand_state_update(&self, internal: &CollectionItemRef<T>) {
        let (ext_id, expand) = {
            let internal = internal.borrow();
            (internal.full_id.to_string(), internal.expand)
        };
        self.push_diff(TestsDiffOp::Update { item: ITestItemUpdate { ext_id, expand: Some(expand), item: None } });
    }

    fn remove_item(&self, child_id: &str) {
        let child_item = self.tree.borrow().get(child_id).cloned();
        let Some(child_item) = child_item else {
            throw(Error::new("attempting to remove non-existent child"));
        };

        self.push_diff(TestsDiffOp::Remove { item_id: child_id.to_string() });

        let mut queue: Vec<Option<CollectionItemRef<T>>> = vec![Some(child_item)];
        while let Some(item) = queue.pop() {
            let Some(item) = item else {
                continue;
            };

            let (actual, full_id) = {
                let item = item.borrow();
                (item.actual.clone(), item.full_id.clone())
            };
            *self.api_for(&actual).listener.borrow_mut() = None;

            for tag in actual.tags() {
                self.decrement_tag_refs(&tag.id);
            }

            self.tree.borrow_mut().shift_remove(&full_id.to_string());
            for (_, child) in self.children_of(&actual).entries() {
                let next = self.tree.borrow().get(&TestId::join_to_string(&full_id, &child.id())).cloned();
                queue.push(next);
            }
        }
    }

    /// Immediately emits any pending diffs on the collection.
    pub fn flush_diff(&self) {
        let diff = self.collect_diff();
        if !diff.is_empty() {
            self.diff_op_emitter.fire(diff);
        }
    }
}

impl<T: ITestItemLike> IDisposable for TestItemCollection<T> {
    fn dispose(&self) {
        let items: Vec<CollectionItemRef<T>> = self.tree.borrow().values().cloned().collect();
        for item in items {
            let actual = item.borrow().actual.clone();
            *self.api_for(&actual).listener.borrow_mut() = None;
        }

        self.tree.borrow_mut().clear();
        self.diff.borrow_mut().clear();
        self.base.dispose();
    }
}

/// Implementation of vscode.TestItemCollection. O `delete`, o `get` e o iterador (`entries`) vêm do
/// [`ITestChildrenLike`].
pub trait ITestItemChildren<T: ITestItemLike>: ITestChildrenLike<T> {
    fn size(&self) -> usize;
    fn replace(&self, items: &[T]);
    fn for_each(&self, callback: &mut dyn FnMut(&T, &dyn ITestItemChildren<T>));
    fn add(&self, item: T);

    fn to_json(&self) -> Vec<T>;
}

/// `DuplicateTestItemError`.
pub fn duplicate_test_item_error(id: &str) -> Error {
    Error::named("DuplicateTestItemError", format!("Attempted to insert a duplicate test item ID {id}"))
}

/// `InvalidTestItemError`.
pub fn invalid_test_item_error(id: &str) -> Error {
    Error::named(
        "InvalidTestItemError",
        format!("TestItem with ID \"{id}\" is invalid. Make sure to create it from the createTestItem method."),
    )
}

/// `MixedTestItemController`.
pub fn mixed_test_item_controller(id: &str, ctrl_a: &str, ctrl_b: &str) -> Error {
    Error::named(
        "MixedTestItemController",
        format!("TestItem with ID \"{id}\" is from controller \"{ctrl_a}\" and cannot be added as a child of an item from controller \"{ctrl_b}\"."),
    )
}

struct TestItemChildren<T: ITestItemLike> {
    api: Rc<ITestItemApi<T>>,
    get_api: Rc<dyn Fn(&T) -> Rc<ITestItemApi<T>>>,
    mapped: RefCell<IndexMap<String, T>>,
}

/// `createTestItemChildren(api, getApi, checkCtor)`.
pub fn create_test_item_children<T: ITestItemLike>(api: Rc<ITestItemApi<T>>, get_api: Rc<dyn Fn(&T) -> Rc<ITestItemApi<T>>>) -> Rc<dyn ITestItemChildren<T>> {
    Rc::new(TestItemChildren { api, get_api, mapped: RefCell::new(IndexMap::new()) })
}

impl<T: ITestItemLike> ITestChildrenLike<T> for TestItemChildren<T> {
    /// `[Symbol.iterator]`.
    fn entries(&self) -> Vec<(String, T)> {
        self.mapped.borrow().iter().map(|(id, item)| (id.clone(), item.clone())).collect()
    }

    fn get(&self, item_id: &str) -> Option<T> {
        self.mapped.borrow().get(item_id).cloned()
    }

    fn delete(&self, id: &str) {
        let removed = self.mapped.borrow_mut().shift_remove(id).is_some();
        if removed {
            self.api.notify(&ExtHostTestItemEvent::RemoveChild { id: id.to_string() });
        }
    }
}

impl<T: ITestItemLike> ITestItemChildren<T> for TestItemChildren<T> {
    /// @inheritdoc
    fn size(&self) -> usize {
        self.mapped.borrow().len()
    }

    /// @inheritdoc
    fn for_each(&self, callback: &mut dyn FnMut(&T, &dyn ITestItemChildren<T>)) {
        let items: Vec<T> = self.mapped.borrow().values().cloned().collect();
        for item in &items {
            callback(item, self);
        }
    }

    /// @inheritdoc
    fn replace(&self, items: &[T]) {
        let mut new_mapped: IndexMap<String, T> = IndexMap::new();
        let mut to_delete: IndexSet<String> = self.mapped.borrow().keys().cloned().collect();
        let mut bulk: Vec<ExtHostTestItemEvent<T>> = Vec::new();

        for item in items {
            let item_controller = (self.get_api)(item).controller_id.clone();
            if item_controller != self.api.controller_id {
                throw(mixed_test_item_controller(&item.id(), &item_controller, &self.api.controller_id));
            }

            if new_mapped.contains_key(&item.id()) {
                throw(duplicate_test_item_error(&item.id()));
            }

            new_mapped.insert(item.id(), item.clone());
            to_delete.shift_remove(&item.id());
            bulk.push(ExtHostTestItemEvent::Upsert { item: item.clone() });
        }

        for id in to_delete {
            bulk.push(ExtHostTestItemEvent::RemoveChild { id });
        }

        self.api.notify(&ExtHostTestItemEvent::Bulk { ops: bulk });

        // important mutations come after firing, so if an error happens no
        // changes will be "saved":
        *self.mapped.borrow_mut() = new_mapped;
    }

    /// @inheritdoc
    fn add(&self, item: T) {
        self.mapped.borrow_mut().insert(item.id(), item.clone());
        self.api.notify(&ExtHostTestItemEvent::Upsert { item });
    }

    /// JSON serialization function.
    fn to_json(&self) -> Vec<T> {
        self.mapped.borrow().values().cloned().collect()
    }
}
