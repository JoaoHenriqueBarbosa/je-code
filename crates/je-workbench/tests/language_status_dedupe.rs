//! Porte de `src/vs/workbench/contrib/languageStatus/test/common/languageStatusDedupe.test.ts` (suíte `LanguageStatus -
//! Dedicated Entry Deduplication`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` vira o guarda criado no começo de cada caso.
//! - O original testa um padrão, não uma classe: `runDedicatedEntryUpdate` e `runDedicatedEntryUpdateBuggy` são funções do
//!   próprio arquivo de teste e continuam sendo, aqui, funções do arquivo de teste.
//! - `MockAccessor` é uma struct com `Cell`s (o objeto literal do original muta os próprios campos); a identidade que o
//!   original compara com `strictEqual` é `Rc::ptr_eq`.
//! - O `Map<string, MockAccessor>` é `HashMap<String, Rc<MockAccessor>>`; `dispose(existingEntries.values())` é
//!   `dispose_all`, que descarta cada valor que sobrou no mapa.
//! - O `createdEntries.push(...)` dos callbacks é um `RefCell<Vec<_>>` compartilhado, preenchido por `create_tracking`.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::lifecycle::{IDisposable, dispose_all};

/// `MockAccessor`.
struct MockAccessor {
    id: String,
    update_count: Cell<u32>,
    disposed: Cell<bool>,
}

impl MockAccessor {
    fn update(&self) {
        self.update_count.set(self.update_count.get() + 1);
    }
}

impl IDisposable for MockAccessor {
    fn dispose(&self) {
        self.disposed.set(true);
    }
}

type Entries = HashMap<String, Rc<MockAccessor>>;

/// `createMockAccessor`.
fn create_mock_accessor(id: &str) -> Rc<MockAccessor> {
    Rc::new(MockAccessor { id: id.to_string(), update_count: Cell::new(0), disposed: Cell::new(false) })
}

/// Os callbacks `(id) => { const e = createMockAccessor(id); createdEntries.push(e); return e; }` dos casos que contam as
/// entradas criadas.
fn create_tracking(created_entries: &Rc<RefCell<Vec<Rc<MockAccessor>>>>) -> impl FnMut(&str) -> Rc<MockAccessor> {
    let created_entries = created_entries.clone();
    move |id| {
        let entry = create_mock_accessor(id);
        created_entries.borrow_mut().push(entry.clone());
        entry
    }
}

/// Simulates the dedicated-entry update loop from LanguageStatus._update, using the FIXED logic that checks
/// newDedicatedEntries before creating.
fn run_dedicated_entry_update(
    model_dedicated_ids: &[&str],
    mut existing_entries: Entries,
    mut create_entry: impl FnMut(&str) -> Rc<MockAccessor>,
) -> Entries {
    let mut new_dedicated_entries = Entries::new();
    for id in model_dedicated_ids {
        let found = new_dedicated_entries.get(*id).cloned().or_else(|| existing_entries.get(*id).cloned());
        let entry = match found {
            None => create_entry(*id),
            Some(entry) => {
                entry.update();
                existing_entries.remove(*id);
                entry
            }
        };
        new_dedicated_entries.insert(id.to_string(), entry);
    }
    dispose_all(existing_entries.values());
    new_dedicated_entries
}

/// Simulates the OLD (buggy) dedicated-entry update loop that only checks existingEntries, not newDedicatedEntries.
fn run_dedicated_entry_update_buggy(
    model_dedicated_ids: &[&str],
    mut existing_entries: Entries,
    mut create_entry: impl FnMut(&str) -> Rc<MockAccessor>,
) -> Entries {
    let mut new_dedicated_entries = Entries::new();
    for id in model_dedicated_ids {
        let entry = match existing_entries.get(*id).cloned() {
            None => create_entry(*id),
            Some(entry) => {
                entry.update();
                existing_entries.remove(*id);
                entry
            }
        };
        new_dedicated_entries.insert(id.to_string(), entry);
    }
    dispose_all(existing_entries.values());
    new_dedicated_entries
}

#[test]
fn reuses_existing_entry_from_previous_update() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let mut existing = Entries::new();
    let old_entry = create_mock_accessor("status-A");
    existing.insert("status-A".to_string(), old_entry.clone());

    let result = run_dedicated_entry_update(&["status-A"], existing, create_mock_accessor);

    assert!(Rc::ptr_eq(&result["status-A"], &old_entry), "should reuse the existing entry");
    assert_eq!(old_entry.update_count.get(), 1, "should have updated the entry");
    assert!(!old_entry.disposed.get(), "should not dispose reused entry");
}

#[test]
fn creates_new_entry_when_none_exists() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let existing = Entries::new();

    let result = run_dedicated_entry_update(&["status-A"], existing, create_mock_accessor);

    let entry = result.get("status-A").expect("should create a new entry");
    assert_eq!(entry.id, "status-A");
    assert_eq!(entry.update_count.get(), 0, "should not have called update on new entry");
    assert!(!entry.disposed.get(), "new entry should not be disposed");
}

#[test]
fn disposes_entries_no_longer_in_model() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let mut existing = Entries::new();
    let old_entry = create_mock_accessor("status-A");
    existing.insert("status-A".to_string(), old_entry.clone());

    let result = run_dedicated_entry_update(&[], existing, create_mock_accessor);

    assert_eq!(result.len(), 0, "should have no entries");
    assert!(old_entry.disposed.get(), "old entry should be disposed");
}

#[test]
fn duplicate_status_ids_fixed_version_reuses_entry_from_current_update() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    // This is the core regression test: when model.dedicated contains duplicate IDs (which can happen momentarily when a
    // status is re-registered via $setLanguageStatus), the fixed code should reuse the entry created for the first
    // occurrence instead of creating a second entry that orphans the first.
    let existing = Entries::new();
    let created_entries = Rc::new(RefCell::new(Vec::new()));

    let result = run_dedicated_entry_update(
        &["status-A", "status-A"], // duplicate IDs
        existing,
        create_tracking(&created_entries),
    );

    // Fixed: only one entry should be created, and it should be updated when the duplicate is encountered
    let created = created_entries.borrow();
    assert_eq!(created.len(), 1, "should create only one entry");
    assert_eq!(result.len(), 1, "result map should have one entry");
    assert_eq!(created[0].update_count.get(), 1, "entry should be updated once for the duplicate");
    assert!(!created[0].disposed.get(), "the entry should not be disposed");
}

#[test]
fn duplicate_status_ids_buggy_version_leaks_entry() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    // Demonstrates that the old (buggy) code creates two entries for duplicate IDs, orphaning the first one.
    let existing = Entries::new();
    let created_entries = Rc::new(RefCell::new(Vec::new()));

    let result = run_dedicated_entry_update_buggy(
        &["status-A", "status-A"], // duplicate IDs
        existing,
        create_tracking(&created_entries),
    );

    // Buggy: two entries are created, the first is orphaned (overwritten in map)
    let created = created_entries.borrow();
    assert_eq!(created.len(), 2, "buggy version creates two entries");
    assert_eq!(result.len(), 1, "result map has one entry (second overwrites first)");
    // The first entry is orphaned - it's not in the result map and not disposed
    assert!(!created[0].disposed.get(), "first entry is NOT disposed (leaked!)");
    assert!(!Rc::ptr_eq(&result["status-A"], &created[0]), "first entry is not in the result");
    assert!(Rc::ptr_eq(&result["status-A"], &created[1]), "second entry is in the result");
}

#[test]
fn duplicate_ids_with_existing_entry_fixed_version_reuses_existing() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    // When an existing entry exists and duplicates appear, the fixed code should reuse the existing entry for the first
    // occurrence and then reuse it again for the duplicate.
    let mut existing = Entries::new();
    let old_entry = create_mock_accessor("status-A");
    existing.insert("status-A".to_string(), old_entry.clone());
    let created_entries = Rc::new(RefCell::new(Vec::new()));

    let result = run_dedicated_entry_update(
        &["status-A", "status-A"], // duplicate IDs
        existing,
        create_tracking(&created_entries),
    );

    assert_eq!(created_entries.borrow().len(), 0, "should not create any new entries");
    assert_eq!(result.len(), 1, "result map should have one entry");
    assert!(Rc::ptr_eq(&result["status-A"], &old_entry), "should reuse the existing entry");
    assert_eq!(old_entry.update_count.get(), 2, "should be updated twice (once per duplicate)");
    assert!(!old_entry.disposed.get(), "should not be disposed");
}

#[test]
fn mixed_unique_and_duplicate_ids() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let mut existing = Entries::new();
    let existing_b = create_mock_accessor("status-B");
    existing.insert("status-B".to_string(), existing_b.clone());
    let created_entries = Rc::new(RefCell::new(Vec::new()));

    let result = run_dedicated_entry_update(
        &["status-A", "status-B", "status-A"], // A appears twice, B once
        existing,
        create_tracking(&created_entries),
    );

    let created = created_entries.borrow();
    assert_eq!(created.len(), 1, "should create one new entry (for first status-A)");
    assert_eq!(result.len(), 2, "result map should have two entries");
    assert!(Rc::ptr_eq(&result["status-A"], &created[0]), "status-A should use created entry");
    assert_eq!(created[0].update_count.get(), 1, "status-A entry updated once for duplicate");
    assert!(Rc::ptr_eq(&result["status-B"], &existing_b), "status-B should reuse existing entry");
    assert_eq!(existing_b.update_count.get(), 1, "status-B entry updated once");
}
