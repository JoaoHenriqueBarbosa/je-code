//! Porte de `src/vs/workbench/test/browser/parts/statusbar/statusbarModel.test.ts` (suíte
//! `Workbench status bar model`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` vira o guarda criado no começo de cada caso; o
//!   `disposables.add(...)` do original é o `add` do guarda.
//! - `TestStorageService` (do `workbenchTestServices.ts`) é um `InMemoryStorageService` que só acrescenta
//!   `testEmitWillSaveState`, que nenhum caso usa: aqui é o `InMemoryStorageService` direto.
//! - `document.createElement('div')` é o identificador opaco `CONTAINER`, o mesmo `container` e
//!   `labelContainer` de todas as entradas, como no original. O original não observa o DOM neste teste
//!   (nenhum caso lê a visibilidade, as classes `first-visible-item` e `last-visible-item` nem o foco), então o
//!   [`StatusbarEntryDom`] do teste é um dublê que não faz nada ([`NoDom`]).
//! - As entradas são `Rc<StatusbarViewModelEntry>` e `assert.strictEqual(a, b)` entre entradas é `Rc::ptr_eq`.
//! - `Number.MIN_VALUE` (o menor positivo, 5e-324) é `f64::from_bits(1)`, não `f64::MIN_POSITIVE`.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use std::cell::RefCell;
use std::rc::Rc;

use common::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite};
use je_platform::storage::{InMemoryStorageService, StorageService};
use je_workbench::browser::parts::statusbar::statusbar_model::{
    StatusbarElementId, StatusbarEntryDom, StatusbarEntryVisibilityChange, StatusbarViewModel, StatusbarViewModelEntry, VisibleEdgeMark,
};
use je_workbench::services::statusbar::statusbar::{StatusbarAlignment, StatusbarEntryLocation, StatusbarEntryPriority, StatusbarEntryPrimaryPriority, StatusbarLocation};

const CONTAINER: StatusbarElementId = StatusbarElementId(1);

/// O que o teste original não observa: o DOM da barra.
struct NoDom;

impl StatusbarEntryDom for NoDom {
    fn set_visible(&self, _container: StatusbarElementId, _visible: bool) {}

    fn is_ancestor_of_active_element(&self, _container: StatusbarElementId) -> bool {
        false
    }

    fn focus(&self, _label_container: StatusbarElementId) {}

    fn remove_visible_edge_marks(&self, _container: StatusbarElementId) {}

    fn add_visible_edge_mark(&self, _container: StatusbarElementId, _mark: VisibleEdgeMark) {}
}

/// `disposables.add(new StatusbarViewModel(disposables.add(new TestStorageService())))`.
fn new_model(guard: &LeakGuard) -> Rc<StatusbarViewModel> {
    let storage = guard.add_rc(InMemoryStorageService::new());
    let storage_service: Rc<dyn StorageService> = Rc::new(storage);
    guard.add_rc(StatusbarViewModel::new(storage_service, Rc::new(NoDom)))
}

/// `priority: { primary: <number>, secondary }`.
fn number(primary: f64, secondary: f64) -> StatusbarEntryPriority {
    StatusbarEntryPriority { primary: StatusbarEntryPrimaryPriority::Number(primary), secondary }
}

/// `priority: { primary: { location: { id, priority }, alignment }, secondary }`.
fn relative(id: &str, location_priority: f64, alignment: StatusbarAlignment, secondary: f64) -> StatusbarEntryPriority {
    StatusbarEntryPriority {
        primary: StatusbarEntryPrimaryPriority::Location(StatusbarEntryLocation {
            location: StatusbarLocation { id: id.to_owned(), priority: location_priority },
            alignment,
            compact: None,
        }),
        secondary,
    }
}

/// `{ id, alignment, name, priority, container, labelContainer: container, hasCommand: false, extensionId: undefined }`.
fn entry(id: &str, alignment: StatusbarAlignment, name: &str, priority: StatusbarEntryPriority) -> Rc<StatusbarViewModelEntry> {
    Rc::new(StatusbarViewModelEntry {
        id: id.to_owned(),
        alignment,
        name: name.to_owned(),
        priority,
        container: CONTAINER,
        label_container: CONTAINER,
        has_command: false,
        extension_id: None,
    })
}

fn ids(entries: &[Rc<StatusbarViewModelEntry>]) -> Vec<&str> {
    entries.iter().map(|entry| entry.id.as_str()).collect()
}

#[test]
fn basics() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let model = new_model(&guard);

    assert_eq!(model.entries().len(), 0);

    let entry1 = entry("3", StatusbarAlignment::Left, "3", number(3.0, 1.0));
    model.add(entry1.clone());
    let entry2 = entry("2", StatusbarAlignment::Left, "2", number(2.0, 1.0));
    model.add(entry2.clone());
    let entry3 = entry("1", StatusbarAlignment::Left, "1", number(1.0, 1.0));
    model.add(entry3.clone());
    let entry4 = entry("1-right", StatusbarAlignment::Right, "1-right", number(1.0, 1.0));
    model.add(entry4.clone());

    assert_eq!(model.entries().len(), 4);

    let left_entries = model.get_entries(StatusbarAlignment::Left);
    assert_eq!(left_entries.len(), 3);
    assert_eq!(model.get_entries(StatusbarAlignment::Right).len(), 1);

    assert_eq!(left_entries[0].id, "3");
    assert_eq!(left_entries[1].id, "2");
    assert_eq!(left_entries[2].id, "1");

    let entries = model.entries();
    assert_eq!(entries[0].id, "3");
    assert_eq!(entries[1].id, "2");
    assert_eq!(entries[2].id, "1");
    assert_eq!(entries[3].id, "1-right");

    assert!(model.find_entry(CONTAINER).is_some());

    let did_change_entry_visibility = Rc::new(RefCell::new(StatusbarEntryVisibilityChange { id: String::new(), visible: false }));
    let recorded = did_change_entry_visibility.clone();
    guard.add(model.on_did_change_entry_visibility().listen(move |e| {
        *recorded.borrow_mut() = e.clone();
    }));

    assert!(!model.is_hidden("1"));
    model.hide("1");
    assert_eq!(did_change_entry_visibility.borrow().id, "1");
    assert!(!did_change_entry_visibility.borrow().visible);
    assert!(model.is_hidden("1"));

    *did_change_entry_visibility.borrow_mut() = StatusbarEntryVisibilityChange { id: String::new(), visible: false };

    model.show("1");
    assert_eq!(did_change_entry_visibility.borrow().id, "1");
    assert!(did_change_entry_visibility.borrow().visible);
    assert!(!model.is_hidden("1"));

    model.remove(&entry1);
    model.remove(&entry4);
    assert_eq!(model.entries().len(), 2);

    model.remove(&entry2);
    model.remove(&entry3);
    assert_eq!(model.entries().len(), 0);
}

#[test]
fn sorting_with_infinity_and_max_number() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let model = new_model(&guard);

    assert_eq!(model.entries().len(), 0);

    model.add(entry("3", StatusbarAlignment::Left, "3", number(f64::MAX, 1.0)));
    // Number.MIN_VALUE
    model.add(entry("2", StatusbarAlignment::Left, "2", number(f64::from_bits(1), 1.0)));
    model.add(entry("1", StatusbarAlignment::Left, "1", number(f64::INFINITY, 1.0)));
    model.add(entry("0", StatusbarAlignment::Left, "0", number(f64::NEG_INFINITY, 1.0)));
    model.add(entry("4", StatusbarAlignment::Left, "4", number(100.0, 1.0)));

    let entries = model.entries();
    assert_eq!(entries[0].id, "1");
    assert_eq!(entries[1].id, "3");
    assert_eq!(entries[2].id, "4");
    assert_eq!(entries[3].id, "2");
    assert_eq!(entries[4].id, "0");
}

#[test]
fn secondary_priority_used_when_primary_is_same() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let model = new_model(&guard);

    assert_eq!(model.entries().len(), 0);

    model.add(entry("1", StatusbarAlignment::Left, "1", number(1.0, 1.0)));
    model.add(entry("2", StatusbarAlignment::Left, "2", number(1.0, 2.0)));
    model.add(entry("3", StatusbarAlignment::Left, "3", number(1.0, 3.0)));

    let entries = model.entries();
    assert_eq!(entries[0].id, "3");
    assert_eq!(entries[1].id, "2");
    assert_eq!(entries[2].id, "1");
}

#[test]
fn insertion_order_preserved_when_priorites_are_the_same() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let model = new_model(&guard);

    assert_eq!(model.entries().len(), 0);

    model.add(entry("1", StatusbarAlignment::Left, "1", number(1.0, 1.0)));
    model.add(entry("2", StatusbarAlignment::Left, "2", number(1.0, 1.0)));
    model.add(entry("3", StatusbarAlignment::Left, "3", number(1.0, 1.0)));

    let entries = model.entries();
    assert_eq!(entries[0].id, "1");
    assert_eq!(entries[1].id, "2");
    assert_eq!(entries[2].id, "3");
}

#[test]
fn entry_with_reference_to_other_entry_existing() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let model = new_model(&guard);

    // Existing reference, Alignment: left
    model.add(entry("a", StatusbarAlignment::Left, "1", number(2.0, 1.0)));
    model.add(entry("b", StatusbarAlignment::Left, "2", number(1.0, 1.0)));

    let mut entry_c = entry("c", StatusbarAlignment::Left, "3", relative("a", 2.0, StatusbarAlignment::Left, 1.0));
    model.add(entry_c.clone());

    let mut entries = model.entries();
    assert_eq!(entries.len(), 3);
    assert_eq!(entries[0].id, "c");
    assert_eq!(entries[1].id, "a");
    assert_eq!(entries[2].id, "b");

    model.remove(&entry_c);

    // Existing reference, Alignment: right
    entry_c = entry("c", StatusbarAlignment::Right, "3", relative("a", 2.0, StatusbarAlignment::Right, 1.0));
    model.add(entry_c.clone());

    entries = model.entries();
    assert_eq!(entries.len(), 3);
    assert_eq!(entries[0].id, "a");
    assert_eq!(entries[1].id, "c");
    assert_eq!(entries[2].id, "b");
}

#[test]
fn entry_with_reference_to_other_entry_nonexistent() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let model = new_model(&guard);

    // Nonexistent reference, Alignment: left
    model.add(entry("a", StatusbarAlignment::Left, "1", number(2.0, 1.0)));
    model.add(entry("b", StatusbarAlignment::Left, "2", number(1.0, 1.0)));

    let mut entry_c = entry("c", StatusbarAlignment::Left, "3", relative("not-existing", 0.0, StatusbarAlignment::Left, 1.0));
    model.add(entry_c.clone());

    let mut entries = model.entries();
    assert_eq!(entries.len(), 3);
    assert_eq!(entries[0].id, "a");
    assert_eq!(entries[1].id, "b");
    assert_eq!(entries[2].id, "c");

    model.remove(&entry_c);

    // Nonexistent reference, Alignment: different fallback priority
    entry_c = entry("c", StatusbarAlignment::Left, "3", relative("not-existing", 3.0, StatusbarAlignment::Left, 1.0));
    model.add(entry_c.clone());

    entries = model.entries();
    assert_eq!(entries.len(), 3);
    assert_eq!(entries[0].id, "c");
    assert_eq!(entries[1].id, "a");
    assert_eq!(entries[2].id, "b");

    model.remove(&entry_c);

    // Nonexistent reference, Alignment: right
    entry_c = entry("c", StatusbarAlignment::Right, "3", relative("not-existing", 3.0, StatusbarAlignment::Right, 1.0));
    model.add(entry_c.clone());

    entries = model.entries();
    assert_eq!(entries.len(), 3);
    assert_eq!(entries[0].id, "a");
    assert_eq!(entries[1].id, "b");
    assert_eq!(entries[2].id, "c");
}

#[test]
fn entry_with_reference_to_other_entry_resorts_based_on_other_entry_being_there_or_not() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let model = new_model(&guard);

    model.add(entry("a", StatusbarAlignment::Left, "1", number(2.0, 1.0)));
    model.add(entry("b", StatusbarAlignment::Left, "2", number(1.0, 1.0)));
    model.add(entry("c", StatusbarAlignment::Left, "3", relative("not-existing", 0.0, StatusbarAlignment::Left, 1.0)));

    let mut entries = model.entries();
    assert_eq!(entries.len(), 3);
    assert_eq!(entries[0].id, "a");
    assert_eq!(entries[1].id, "b");
    assert_eq!(entries[2].id, "c");

    let not_existing = entry("not-existing", StatusbarAlignment::Left, "not-existing", number(3.0, 1.0));
    model.add(not_existing.clone());

    entries = model.entries();
    assert_eq!(entries.len(), 4);
    assert_eq!(entries[0].id, "c");
    assert_eq!(entries[1].id, "not-existing");
    assert_eq!(entries[2].id, "a");
    assert_eq!(entries[3].id, "b");

    model.remove(&not_existing);

    entries = model.entries();
    assert_eq!(entries.len(), 3);
    assert_eq!(entries[0].id, "a");
    assert_eq!(entries[1].id, "b");
    assert_eq!(entries[2].id, "c");
}

#[test]
fn entry_with_reference_to_other_entry_but_different_alignment_does_not_explode() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let model = new_model(&guard);

    model.add(entry("1-left", StatusbarAlignment::Left, "1-left", number(2.0, 1.0)));
    model.add(entry("2-left", StatusbarAlignment::Left, "2-left", number(1.0, 1.0)));

    model.add(entry("1-right", StatusbarAlignment::Right, "1-right", number(2.0, 1.0)));
    model.add(entry("2-right", StatusbarAlignment::Right, "2-right", number(1.0, 1.0)));

    assert_eq!(model.get_entries(StatusbarAlignment::Left).len(), 2);
    assert_eq!(model.get_entries(StatusbarAlignment::Right).len(), 2);

    let relative_entry_left = entry("relative", StatusbarAlignment::Left, "relative", relative("1-right", 2.0, StatusbarAlignment::Left, 1.0));
    model.add(relative_entry_left.clone());

    assert_eq!(model.get_entries(StatusbarAlignment::Left).len(), 3);
    assert!(Rc::ptr_eq(&model.get_entries(StatusbarAlignment::Left)[2], &relative_entry_left));
    assert_eq!(model.get_entries(StatusbarAlignment::Right).len(), 2);

    model.remove(&relative_entry_left);

    let relative_entry_right = entry("relative", StatusbarAlignment::Right, "relative", relative("1-right", 2.0, StatusbarAlignment::Left, 1.0));
    model.add(relative_entry_right);

    assert_eq!(model.get_entries(StatusbarAlignment::Left).len(), 2);
    assert_eq!(model.get_entries(StatusbarAlignment::Right).len(), 3);
}

#[test]
fn entry_with_reference_to_other_entry_respects_secondary_sorting_existent() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let model = new_model(&guard);

    model.add(entry("ref", StatusbarAlignment::Left, "ref", number(0.0, 0.0)));
    model.add(entry("entry2", StatusbarAlignment::Right, "2", relative("ref", 0.0, StatusbarAlignment::Right, 2.0)));
    model.add(entry("entry1", StatusbarAlignment::Right, "1", relative("ref", 0.0, StatusbarAlignment::Right, 1.0)));
    model.add(entry("entry3", StatusbarAlignment::Right, "3", relative("ref", 0.0, StatusbarAlignment::Right, 3.0)));

    let entries = model.entries();
    assert_eq!(entries.len(), 4);
    assert_eq!(ids(&entries), ["ref", "entry3", "entry2", "entry1"]);
}

#[test]
fn entry_with_reference_to_other_entry_respects_secondary_sorting_nonexistent() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let model = new_model(&guard);

    model.add(entry("entry2", StatusbarAlignment::Right, "2", relative("ref", 1.0, StatusbarAlignment::Right, 2.0)));
    model.add(entry("entry1", StatusbarAlignment::Right, "1", relative("ref", 1.0, StatusbarAlignment::Right, 1.0)));
    model.add(entry("entry3", StatusbarAlignment::Right, "3", relative("ref", 1.0, StatusbarAlignment::Right, 3.0)));

    let entries = model.entries();
    assert_eq!(entries.len(), 3);
    assert_eq!(entries[0].id, "entry3");
    assert_eq!(entries[1].id, "entry2");
    assert_eq!(entries[2].id, "entry1");
}
