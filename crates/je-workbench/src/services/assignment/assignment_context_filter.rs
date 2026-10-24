//! Porte de `src/vs/workbench/services/assignment/common/assignmentContextFilter.ts`: aplica os filtros de atribuição
//! registrados ao contexto de atribuição e lembra, por filtro, quais ids cada um excluiu.
//!
//! Divergências:
//! - **Injeção.** O `IStorageService` do construtor é o `Rc<dyn StorageService>` de [`AssignmentContextFilter::new`].
//! - **`extends Disposable`** é um [`Disposable`] guardado em `base`, com o `DisposableStore` dos filtros e o emissor
//!   registrados.
//! - **`Map<string, Set<string>>`** é o `IndexMap<String, IndexSet<String>>` (a ordem de inserção é a do `Map` e do
//!   `Set`); o `Record<string, string[]>` serializado é um [`Object`] do `je-base`, que ordena as chaves como o
//!   JavaScript (índices de array primeiro).
//! - **`JSON.parse` com `try/catch`**: um texto malformado (qualquer erro do analisador) é o cache vazio. `Object.entries`
//!   de um array enumera os índices como chaves, como no original. As chaves do cache são texto UTF-8 (uma chave com
//!   substituto isolado vira o caractere de substituição).

use std::cell::RefCell;
use std::collections::HashSet;
use std::rc::Rc;

use indexmap::{IndexMap, IndexSet};
use je_base::JsString;
use je_base::event::{Emitter, Event};
use je_base::json::{self, Object, ParseOptions, Value};
use je_base::json_formatter::json_stringify;
use je_base::lifecycle::{Disposable, DisposableStore, IDisposable};
use je_platform::storage::{StorageScope, StorageService, StorageTarget};
use je_storage::StorageValue;

use super::assignment_service::IAssignmentFilter;

const STORAGE_KEY: &str = "workbench.assignment.filteredOutAssignmentContextIds";

type FilteredOutIds = IndexMap<String, IndexSet<String>>;

/// `AssignmentContextFilter`.
pub struct AssignmentContextFilter {
    base: Rc<Disposable>,
    storage_service: Rc<dyn StorageService>,
    filters: RefCell<Vec<Rc<dyn IAssignmentFilter>>>,
    filter_disposables: Rc<DisposableStore>,
    /// Assignment-context ids that were filtered out, keyed by the id of the filter that excluded them. Entries owned by a
    /// filter that has not been registered yet are honored across reloads so those ids stay excluded until the filter takes
    /// over.
    cached_filtered_out_ids: RefCell<FilteredOutIds>,
    /// Ids of filters that have been registered this session. Once a filter is registered it becomes authoritative for its
    /// own id and the persisted cache is repopulated from the ids it actually excludes.
    registered_filter_ids: RefCell<HashSet<String>>,
    on_did_change: Rc<Emitter<()>>,
}

impl AssignmentContextFilter {
    pub fn new(storage_service: Rc<dyn StorageService>) -> Rc<Self> {
        let base = Disposable::new();
        let filter_disposables = DisposableStore::new();
        base.register(filter_disposables.clone());
        let on_did_change = Emitter::new();
        base.register(on_did_change.clone());
        let cached = load_filtered_out_ids(storage_service.as_ref());
        Rc::new(Self {
            base,
            storage_service,
            filters: RefCell::new(Vec::new()),
            filter_disposables,
            cached_filtered_out_ids: RefCell::new(cached),
            registered_filter_ids: RefCell::new(HashSet::new()),
            on_did_change,
        })
    }

    /// `onDidChange`: fires when a filter is added or an already registered filter changes its exclusions.
    pub fn on_did_change(&self) -> Event<()> {
        self.on_did_change.event()
    }

    pub fn add_filter(&self, filter: Rc<dyn IAssignmentFilter>) {
        self.filters.borrow_mut().push(filter.clone());

        // This filter now owns exclusion decisions for its id: drop the state persisted for it in a previous session and
        // let it repopulate the cache as ids actually get filtered out.
        self.registered_filter_ids.borrow_mut().insert(filter.id().to_owned());
        let owned_by_filter = self.cached_filtered_out_ids.borrow().contains_key(filter.id());
        if owned_by_filter {
            let mut next = self.cached_filtered_out_ids.borrow().clone();
            next.shift_remove(filter.id());
            self.store_filtered_out_ids(next);
        }

        let on_did_change = self.on_did_change.clone();
        self.filter_disposables.add(filter.on_did_change().listen(move |()| on_did_change.fire(())));
        self.on_did_change.fire(());
    }

    /// Removes the excluded assignment-context ids from the given context and persists the reconciled per-filter cache.
    pub fn filter(&self, assignment_context: &str) -> String {
        let assignments = assignment_context.split(';');
        let filters: Vec<Rc<dyn IAssignmentFilter>> = self.filters.borrow().clone();

        // Fresh exclusions produced by the currently registered filters, keyed by filter id.
        let mut fresh_filtered_out: FilteredOutIds = IndexMap::new();

        let mut filtered_assignments: Vec<&str> = Vec::new();
        for assignment in assignments {
            let mut excluded = false;
            for filter in &filters {
                if filter.exclude(assignment) {
                    fresh_filtered_out.entry(filter.id().to_owned()).or_default().insert(assignment.to_owned());
                    excluded = true;
                }
            }
            if excluded {
                continue;
            }

            // Honor ids cached by filters that have not been registered yet, so they remain excluded until the owning filter
            // takes over.
            let honored_from_cache =
                self.cached_filtered_out_ids.borrow().iter().any(|(filter_id, ids)| !self.registered_filter_ids.borrow().contains(filter_id) && ids.contains(assignment));
            if honored_from_cache {
                continue;
            }

            filtered_assignments.push(assignment);
        }

        // Persist the reconciled cache: registered filters contribute exactly what they currently exclude (dropping ids that
        // are no longer filtered out), while entries owned by not-yet-registered filters are preserved.
        let mut next: FilteredOutIds = IndexMap::new();
        for (filter_id, ids) in self.cached_filtered_out_ids.borrow().iter() {
            if !self.registered_filter_ids.borrow().contains(filter_id) {
                next.insert(filter_id.clone(), ids.clone());
            }
        }
        for (filter_id, ids) in fresh_filtered_out {
            next.insert(filter_id, ids);
        }
        self.store_filtered_out_ids(next);

        filtered_assignments.join(";")
    }

    fn store_filtered_out_ids(&self, next: FilteredOutIds) {
        // Drop empty entries so an id disappears from storage once nothing is filtered out for it.
        let normalized: FilteredOutIds = next.into_iter().filter(|(_, ids)| !ids.is_empty()).collect();

        if are_caches_equal(&normalized, &self.cached_filtered_out_ids.borrow()) {
            return;
        }

        if normalized.is_empty() {
            *self.cached_filtered_out_ids.borrow_mut() = normalized;
            self.storage_service.remove(STORAGE_KEY, StorageScope::Application);
        } else {
            let mut serializable = Object::new();
            for (filter_id, ids) in &normalized {
                serializable.insert(JsString::from(filter_id.as_str()), Value::Array(ids.iter().map(|id| Value::String(JsString::from(id.as_str()))).collect()));
            }
            *self.cached_filtered_out_ids.borrow_mut() = normalized;
            self.storage_service.store(STORAGE_KEY, StorageValue::String(json_stringify(&Value::Object(serializable), None)), StorageScope::Application, StorageTarget::Machine);
        }
    }
}

impl IDisposable for AssignmentContextFilter {
    fn dispose(&self) {
        self.base.dispose();
    }
}

/// `_loadFilteredOutIds`.
fn load_filtered_out_ids(storage_service: &dyn StorageService) -> FilteredOutIds {
    let mut result = FilteredOutIds::new();
    let Some(raw) = storage_service.get(STORAGE_KEY, StorageScope::Application, None).filter(|raw| !raw.is_empty()) else {
        return result;
    };

    // Ignore malformed cache.
    let mut errors = Vec::new();
    let options = ParseOptions { disallow_comments: true, allow_trailing_comma: false, allow_empty_content: false };
    let parsed = match json::parse(&raw.encode_utf16().collect::<Vec<u16>>(), &mut errors, options) {
        Some(parsed) if errors.is_empty() => parsed,
        _ => return result,
    };
    // `Object.entries(parsed)`: as chaves próprias (os índices, para um array).
    let entries: Vec<(String, &Value)> = match &parsed {
        Value::Object(object) => object.iter().map(|(key, ids)| (key.to_string_lossy(), ids)).collect(),
        Value::Array(items) => items.iter().enumerate().map(|(index, ids)| (index.to_string(), ids)).collect(),
        _ => return result,
    };
    for (filter_id, ids) in entries {
        if let Value::Array(ids) = ids {
            let set: IndexSet<String> = ids.iter().filter_map(|id| if let Value::String(id) = id { Some(id.to_string_lossy()) } else { None }).collect();
            if !set.is_empty() {
                result.insert(filter_id, set);
            }
        }
    }
    result
}

/// `areCachesEqual`.
fn are_caches_equal(a: &FilteredOutIds, b: &FilteredOutIds) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter().all(|(filter_id, ids)| b.get(filter_id).is_some_and(|other| other.len() == ids.len() && ids.iter().all(|id| other.contains(id))))
}
