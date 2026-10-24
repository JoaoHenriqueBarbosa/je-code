//! Porte de `src/vs/workbench/services/assignment/test/common/assignmentContextFilter.test.ts` (suíte `AssignmentContextFilter`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` vira o [`LeakGuard`] do [`Fixture`], criado antes de qualquer serviço; o
//!   `setup` que cria o `InMemoryStorageService` é [`Fixture::new`].
//! - `createFilter` devolve o [`TestFilter`] (o `{ filter, setExclude }` do original): o predicado de exclusão vive num
//!   `RefCell` e [`TestFilter::set_exclude`] o troca e dispara o `onDidChange`. O `store` passado ao `createFilter` guarda o
//!   emissor, como no original.
//! - `readCache()` é [`Fixture::read_cache`]: o `Record<string, string[]>` do JSON guardado vira um `Vec<(String,
//!   Vec<String>)>` na ordem das chaves (o `deepStrictEqual` do original compara os objetos sem ordem de chaves; os casos
//!   só têm entradas cuja ordem coincide com a de inserção), e `undefined` é `None`.
//! - `storageService.store(key, JSON.stringify({...}), APPLICATION, MACHINE)` é o `StorageService::store` com o texto
//!   do JSON.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use common::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite};
use je_base::event::{Emitter, Event};
use je_base::json::{self, Value};
use je_base::lifecycle::DisposableStore;
use je_platform::storage::{InMemoryStorageService, StorageScope, StorageService, StorageTarget};
use je_storage::StorageValue;
use je_workbench::services::assignment::assignment_context_filter::AssignmentContextFilter;
use je_workbench::services::assignment::assignment_service::IAssignmentFilter;

const FILTERED_OUT_IDS_STORAGE_KEY: &str = "workbench.assignment.filteredOutAssignmentContextIds";

/// `{ id, exclude, onDidChange }` e o `setExclude` do `createFilter`.
struct TestFilter {
    id: String,
    exclude_fn: RefCell<Box<dyn Fn(&str) -> bool>>,
    on_did_change: Rc<Emitter<()>>,
}

impl TestFilter {
    /// `createFilter(id, initialExclude, store)`.
    fn new(id: &str, initial_exclude: impl Fn(&str) -> bool + 'static, store: &DisposableStore) -> Rc<Self> {
        let on_did_change = store.add_rc(Emitter::new());
        Rc::new(Self { id: id.to_owned(), exclude_fn: RefCell::new(Box::new(initial_exclude)), on_did_change })
    }

    /// `setExclude(fn)`.
    fn set_exclude(&self, exclude_fn: impl Fn(&str) -> bool + 'static) {
        *self.exclude_fn.borrow_mut() = Box::new(exclude_fn);
        self.on_did_change.fire(());
    }
}

impl IAssignmentFilter for TestFilter {
    fn id(&self) -> &str {
        &self.id
    }

    fn exclude(&self, assignment: &str) -> bool {
        (self.exclude_fn.borrow())(assignment)
    }

    fn on_did_change(&self) -> Event<()> {
        self.on_did_change.event()
    }
}

struct Fixture {
    guard: LeakGuard,
    storage_service: Rc<dyn StorageService>,
}

impl Fixture {
    /// O `setup` da suíte.
    fn new() -> Self {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let storage_service: Rc<dyn StorageService> = Rc::new(guard.add_rc(InMemoryStorageService::new()));
        Self { guard, storage_service }
    }

    /// `createContextFilter()`.
    fn create_context_filter(&self) -> Rc<AssignmentContextFilter> {
        self.guard.add_rc(AssignmentContextFilter::new(self.storage_service.clone()))
    }

    /// `const store = disposables.add(new DisposableStore())`.
    fn new_store(&self) -> Rc<DisposableStore> {
        self.guard.add_rc(DisposableStore::new())
    }

    /// `readCache()`.
    fn read_cache(&self) -> Option<Vec<(String, Vec<String>)>> {
        let raw = self.storage_service.get(FILTERED_OUT_IDS_STORAGE_KEY, StorageScope::Application, None).filter(|raw| !raw.is_empty())?;
        let mut errors = Vec::new();
        let Some(Value::Object(object)) = json::parse_str(&raw, &mut errors) else {
            panic!("o cache guardado não é um objeto JSON: {raw}");
        };
        Some(
            object
                .iter()
                .map(|(filter_id, ids)| {
                    let Value::Array(ids) = ids else { panic!("os ids de {filter_id:?} não são uma lista") };
                    let ids = ids.iter().map(|id| if let Value::String(id) = id { id.to_string_lossy() } else { panic!("id que não é texto") }).collect();
                    (filter_id.to_string_lossy(), ids)
                })
                .collect(),
        )
    }

    /// `storageService.store(KEY, rawJson, APPLICATION, MACHINE)`.
    fn store_cache(&self, raw: &str) {
        self.storage_service.store(FILTERED_OUT_IDS_STORAGE_KEY, StorageValue::String(raw.to_owned()), StorageScope::Application, StorageTarget::Machine);
    }
}

/// O literal `{ filterId: [ids...] }` que `readCache()` devolve.
fn cache(entries: &[(&str, &[&str])]) -> Option<Vec<(String, Vec<String>)>> {
    Some(entries.iter().map(|(filter_id, ids)| ((*filter_id).to_owned(), ids.iter().map(|id| (*id).to_owned()).collect())).collect())
}

#[test]
fn persists_filtered_out_ids_keyed_by_filter_id_and_removes_them_from_the_context() {
    let f = Fixture::new();
    let store = f.new_store();
    let context_filter = f.create_context_filter();
    context_filter.add_filter(TestFilter::new("onboarding", |a| a == "onb-test1", &store));

    let result = context_filter.filter("onb-test1;onb-test2;keep");

    assert_eq!(result, "onb-test2;keep");
    assert_eq!(f.read_cache(), cache(&[("onboarding", &["onb-test1"])]));
}

#[test]
fn honors_cached_ids_across_reload_before_the_owning_filter_is_registered() {
    let f = Fixture::new();
    // Simulate a previous session that persisted a filtered-out id.
    f.store_cache(r#"{"onboarding":["onb-test1"]}"#);

    let context_filter = f.create_context_filter();

    assert_eq!(context_filter.filter("onb-test1;keep"), "keep");
}

#[test]
fn registering_a_filter_drops_only_its_own_cached_state_and_preserves_other_filters() {
    let f = Fixture::new();
    f.store_cache(r#"{"onboarding":["onb-test1"],"unification":["cmp-ext-a"]}"#);

    let store = f.new_store();
    let context_filter = f.create_context_filter();
    // The onboarding filter is registered but no longer excludes onb-test1.
    context_filter.add_filter(TestFilter::new("onboarding", |_| false, &store));

    let result = context_filter.filter("onb-test1;cmp-ext-a;keep");

    // onb-test1 is no longer excluded (its filter took over), but cmp-ext-a stays excluded
    // because the unification filter has not been registered yet.
    assert_eq!(result, "onb-test1;keep");
    assert_eq!(f.read_cache(), cache(&[("unification", &["cmp-ext-a"])]));
}

#[test]
fn removes_an_id_from_storage_once_it_is_no_longer_filtered_out() {
    let f = Fixture::new();
    let store = f.new_store();
    let context_filter = f.create_context_filter();
    let onboarding = TestFilter::new("onboarding", |a| a == "onb-test1", &store);
    context_filter.add_filter(onboarding.clone());

    context_filter.filter("onb-test1;keep");
    assert_eq!(f.read_cache(), cache(&[("onboarding", &["onb-test1"])]));

    // The gate opens: the filter no longer excludes the id.
    onboarding.set_exclude(|_| false);

    assert_eq!(context_filter.filter("onb-test1;keep"), "onb-test1;keep");
    assert_eq!(f.read_cache(), None);
}

#[test]
fn tracks_multiple_registered_filters_independently() {
    let f = Fixture::new();
    let store = f.new_store();
    let context_filter = f.create_context_filter();
    context_filter.add_filter(TestFilter::new("onboarding", |a| a.starts_with("onb-"), &store));
    context_filter.add_filter(TestFilter::new("unification", |a| a.starts_with("cmp-"), &store));

    let result = context_filter.filter("onb-a;cmp-b;keep");

    assert_eq!(result, "keep");
    assert_eq!(f.read_cache(), cache(&[("onboarding", &["onb-a"]), ("unification", &["cmp-b"])]));
}

#[test]
fn fires_on_did_change_when_a_filter_is_added_and_when_a_filter_changes() {
    let f = Fixture::new();
    let store = f.new_store();
    let context_filter = f.create_context_filter();
    let changes = Rc::new(Cell::new(0));
    f.guard.add(context_filter.on_did_change().listen({
        let changes = changes.clone();
        move |()| changes.set(changes.get() + 1)
    }));

    let onboarding = TestFilter::new("onboarding", |_| false, &store);
    context_filter.add_filter(onboarding.clone());
    onboarding.set_exclude(|a| a == "onb-test1");

    assert_eq!(changes.get(), 2);
}
