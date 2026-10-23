//! Porte de `src/vs/platform/browserView/test/common/browserHistory.test.ts` (suítes
//! `BrowserHistoryEntriesStore`, `BrowserHistoryEntriesStore.hydrate backwards-compat`,
//! `BrowserFaviconsStore` e `BrowserHistoryStore`), caso a caso e na mesma ordem.
//!
//! Divergências: o dado cru do disco (`unknown`) é um [`Dynamic`] montado com literais no estilo do
//! TypeScript (`j("{...}")`), e os snapshots tipados passam pelo `hydrate` via `to_dynamic()`. Os números
//! (`id`, `time`) são `f64`, `explicit: undefined` é `explicit: false`, `store.items` é `store.items()`
//! (uma cópia) e os argumentos opcionais (`faviconHash`, `favicon`, `userInitiated`, `maxEntries`) são
//! passados explicitamente (`None`, `false`). `ensureNoDisposablesAreLeakedInTestSuite` roda no começo de
//! cada teste, como o `setup` e o `teardown` da suíte: cada teste descarta o que cria.

mod common;

use std::cell::Cell;
use std::collections::HashSet;
use std::rc::Rc;

use common::{ensure_no_disposables_are_leaked_in_test_suite, j, jstr};
use je_base::lifecycle::IDisposable;
use je_platform::browser_view::browser_history::{
    BrowserFaviconsStore, BrowserHistoryEntriesStore, BrowserHistoryEntry, BrowserHistoryEntryPatch, BrowserHistoryStore, BrowserHistoryUpdate,
    SerializedBrowserFaviconsSnapshot,
};

/// `() => count++` como ouvinte de um evento `Event<void>`.
fn counter() -> (Rc<Cell<u32>>, impl Fn(&()) + 'static) {
    let count = Rc::new(Cell::new(0));
    let handle = count.clone();
    (count, move |_: &()| handle.set(handle.get() + 1))
}

fn title_patch(title: &str) -> BrowserHistoryEntryPatch {
    BrowserHistoryEntryPatch { title: Some(title.to_owned()), ..Default::default() }
}

fn entry(id: f64, url: &str, time: f64, title: &str, icon: Option<&str>, explicit: bool) -> BrowserHistoryEntry {
    BrowserHistoryEntry { id, url: url.to_owned(), time, title: title.to_owned(), icon: icon.map(str::to_owned), explicit }
}

// ── BrowserHistoryEntriesStore ──

#[test]
fn add_assigns_monotonic_ids_and_exposes_items_oldest_first() {
    let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
    let store = BrowserHistoryEntriesStore::new(None);
    let a = store.add("https://a/", "A", None, false);
    let b = store.add("https://b/", "B", Some("icon-b"), true);
    let c = store.add("https://c/", "C", None, false);

    assert_eq!([a.id, b.id, c.id], [1.0, 2.0, 3.0]);
    let summary: Vec<(f64, String, Option<String>, bool)> = store.items().into_iter().map(|e| (e.id, e.url, e.icon, e.explicit)).collect();
    assert_eq!(
        summary,
        vec![
            (1.0, "https://a/".to_owned(), None, false),
            (2.0, "https://b/".to_owned(), Some("icon-b".to_owned()), true),
            (3.0, "https://c/".to_owned(), None, false),
        ]
    );

    store.dispose();
}

#[test]
fn explicit_is_omitted_from_the_entry_when_false() {
    let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
    let store = BrowserHistoryEntriesStore::new(None);
    let e = store.add("https://a/", "A", None, false);

    // O `hasOwnProperty('explicit')` do original: o campo só existe na forma em disco quando é `true`.
    assert!(!e.to_dynamic().has_property(&jstr("explicit")));

    store.dispose();
}

#[test]
fn update_changes_title_and_icon_returns_whether_anything_changed() {
    let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
    let store = BrowserHistoryEntriesStore::new(None);
    store.add("https://a/", "", None, false);
    assert!(store.update(1.0, &title_patch("A")));
    assert!(store.update(1.0, &BrowserHistoryEntryPatch { favicon_hash: Some(Some("icon-a".to_owned())), ..Default::default() }));
    assert!(!store.update(1.0, &BrowserHistoryEntryPatch { title: Some("A".to_owned()), favicon_hash: Some(Some("icon-a".to_owned())), ..Default::default() }));

    assert_eq!(store.items()[0].title, "A");
    assert_eq!(store.items()[0].icon.as_deref(), Some("icon-a"));

    store.dispose();
}

#[test]
fn update_ignores_empty_title() {
    let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
    let store = BrowserHistoryEntriesStore::new(None);
    store.add("https://a/", "A", None, false);
    assert!(!store.update(1.0, &title_patch("")));
    assert_eq!(store.items()[0].title, "A");

    store.dispose();
}

#[test]
fn update_of_an_unknown_id_is_a_no_op() {
    let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
    let store = BrowserHistoryEntriesStore::new(None);
    store.add("https://a/", "A", None, false);
    assert!(!store.update(999.0, &title_patch("X")));

    store.dispose();
}

#[test]
fn delete_removes_the_targeted_entry_and_leaves_ids_of_others_intact() {
    let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
    let store = BrowserHistoryEntriesStore::new(None);
    let a = store.add("https://a/", "A", None, false);
    let b = store.add("https://b/", "B", None, false);
    let c = store.add("https://c/", "C", None, false);

    assert!(store.delete(b.id));
    assert!(!store.delete(b.id));
    assert_eq!(store.items().iter().map(|e| e.id).collect::<Vec<_>>(), vec![a.id, c.id]);

    store.dispose();
}

#[test]
fn add_beyond_max_entries_evicts_oldest() {
    let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
    let store = BrowserHistoryEntriesStore::new(Some(2.0));
    store.add("https://a/", "A", None, false);
    store.add("https://b/", "B", None, false);
    store.add("https://c/", "C", None, false);

    assert_eq!(store.items().iter().map(|e| e.url.as_str()).collect::<Vec<_>>(), vec!["https://b/", "https://c/"]);

    store.dispose();
}

#[test]
fn on_did_change_fires_for_add_update_delete_clear() {
    let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
    let store = BrowserHistoryEntriesStore::new(None);
    let (count, listener) = counter();
    let sub = store.on_did_change().listen(listener);

    store.add("https://a/", "A", None, false);
    store.update(1.0, &title_patch("A2"));
    store.delete(1.0);
    store.clear();
    // clear on already-empty store should be a no-op
    store.clear();

    assert_eq!(count.get(), 4);

    sub.dispose();
    store.dispose();
}

#[test]
fn entries_serialize_then_hydrate_round_trips() {
    let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
    let a = BrowserHistoryEntriesStore::new(None);
    a.add("https://a/", "A", Some("icon-a"), true);
    a.add("https://b/", "B", None, false);
    let snapshot = a.serialize();

    let b = BrowserHistoryEntriesStore::new(None);
    b.hydrate(Some(&snapshot.to_dynamic()));
    assert_eq!(b.serialize(), snapshot);

    a.dispose();
    b.dispose();
}

#[test]
fn hydrate_seeds_the_id_counter_from_the_max_restored_id() {
    let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
    let store = BrowserHistoryEntriesStore::new(None);
    store.hydrate(Some(&j("{'items': [{'id': 7, 'url': 'https://a/', 'time': 100, 'title': 'A'}, {'id': 12, 'url': 'https://b/', 'time': 200, 'title': 'B'}]}")));
    let next = store.add("https://c/", "C", None, false);
    assert_eq!(next.id, 13.0);

    store.dispose();
}

// ── BrowserHistoryEntriesStore.hydrate backwards-compat ──

#[test]
fn accepts_data_matching_prior_snapshot_shapes() {
    let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
    // Pretend this came off disk: typed as `unknown`, deliberately untrusted
    // so the test guards against accidental future changes to required fields.
    // IMPORTANT: Don't change the shape of this. It ensures compatibility with the earliest versions of the history interface.
    //            When updating the interface, simply extend or add a test for the new shape.
    let raw = j("{'items': [
        {'id': 1, 'url': 'https://a/', 'time': 100, 'title': 'A'},
        {'id': 2, 'url': 'https://b/', 'time': 200, 'title': 'B', 'icon': 'h1'},
        {'id': 4, 'url': 'https://c/', 'time': 300, 'title': 'C', 'explicit': true}
    ]}");

    let store = BrowserHistoryEntriesStore::new(None);
    store.hydrate(Some(&raw));
    assert_eq!(
        store.items(),
        vec![
            entry(1.0, "https://a/", 100.0, "A", None, false),
            entry(2.0, "https://b/", 200.0, "B", Some("h1"), false),
            entry(4.0, "https://c/", 300.0, "C", None, true),
        ]
    );
    // Next add must not collide with restored ids.
    assert_eq!(store.add("https://d/", "D", None, false).id, 5.0);

    store.dispose();
}

#[test]
fn drops_malformed_entries_and_accepts_the_rest() {
    let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
    let raw = j("{'items': [
        {'id': 1, 'url': 'https://a/', 'time': 100, 'title': 'A'},
        {'id': 'bad', 'url': 'https://b/', 'time': 200, 'title': 'B'},
        null,
        {'id': 2},
        {'id': 3, 'url': 'https://c/', 'time': 300, 'title': 'C', 'explicit': 'yes'}
    ]}");
    // O 4º item tem campos obrigatórios faltando e o 5º tem `explicit` inválido.

    let store = BrowserHistoryEntriesStore::new(None);
    store.hydrate(Some(&raw));
    assert_eq!(store.items().iter().map(|e| e.id).collect::<Vec<_>>(), vec![1.0]);

    store.dispose();
}

#[test]
fn undefined_snapshot_resets_to_an_empty_store() {
    let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
    let store = BrowserHistoryEntriesStore::new(None);
    store.add("https://a/", "A", None, false);

    store.hydrate(None);
    assert_eq!(store.items(), vec![]);
    assert_eq!(store.add("https://b/", "B", None, false).id, 1.0);

    store.dispose();
}

// ── BrowserFaviconsStore ──

#[test]
fn register_dedups_by_content_and_returns_the_same_hash() {
    let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
    let store = BrowserFaviconsStore::new();
    let h1 = store.register("data:image/png;base64,AAA");
    let h2 = store.register("data:image/png;base64,AAA");
    let h3 = store.register("data:image/png;base64,BBB");

    assert_eq!(h1, h2);
    assert_ne!(h1, h3);
    assert_eq!(store.get(&h1).as_deref(), Some("data:image/png;base64,AAA"));
    assert_eq!(store.get(&h3).as_deref(), Some("data:image/png;base64,BBB"));

    store.dispose();
}

#[test]
fn favicons_on_did_change_fires_only_when_a_new_favicon_is_added() {
    let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
    let store = BrowserFaviconsStore::new();
    let (count, listener) = counter();
    let sub = store.on_did_change().listen(listener);

    store.register("a");
    store.register("a"); // duplicate: no event
    store.register("b");

    assert_eq!(count.get(), 2);

    sub.dispose();
    store.dispose();
}

#[test]
fn gc_drops_orphans_and_fires_on_did_change_only_when_something_changes() {
    let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
    let store = BrowserFaviconsStore::new();
    let h1 = store.register("a");
    let h2 = store.register("b");
    let (count, listener) = counter();
    let sub = store.on_did_change().listen(listener);

    store.gc(&HashSet::from([h1.clone()]));
    assert_eq!(store.get(&h2), None);
    assert_eq!(store.get(&h1).as_deref(), Some("a"));
    assert_eq!(count.get(), 1);

    // Nothing to remove: no event.
    store.gc(&HashSet::from([h1]));
    assert_eq!(count.get(), 1);

    sub.dispose();
    store.dispose();
}

#[test]
fn favicons_serialize_then_hydrate_round_trips() {
    let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
    let a = BrowserFaviconsStore::new();
    a.register("one");
    a.register("two");
    let snapshot = a.serialize();

    let b = BrowserFaviconsStore::new();
    b.hydrate(Some(&snapshot.to_dynamic()));
    assert_eq!(b.serialize(), snapshot);

    a.dispose();
    b.dispose();
}

#[test]
fn hydrate_accepts_unknown_typed_data_matching_the_current_snapshot_shape() {
    let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
    let raw = j("{'map': {
        'abc': 'data:image/png;base64,AAA',
        'def': 'data:image/png;base64,BBB',
        'bad': 123
    }}");
    // `bad` não é texto: descartado em silêncio.

    let store = BrowserFaviconsStore::new();
    store.hydrate(Some(&raw));
    assert_eq!(store.get("abc").as_deref(), Some("data:image/png;base64,AAA"));
    assert_eq!(store.get("def").as_deref(), Some("data:image/png;base64,BBB"));
    assert_eq!(store.get("bad"), None);

    store.dispose();
}

// ── BrowserHistoryStore ──

#[test]
fn add_returns_a_handle_whose_id_matches_the_underlying_entry() {
    let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
    let store = BrowserHistoryStore::new(None);
    let handle = store.add("https://a/", "A", None, false);

    assert_eq!(handle.id(), store.entries.items()[0].id);

    store.dispose();
}

#[test]
fn add_is_a_no_op_when_max_entries_is_0() {
    let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
    let store = BrowserHistoryStore::new(Some(0.0));
    let handle = store.add("https://a/", "A", Some("data:image/png;base64,XXX"), false);

    assert_eq!(store.entries.items(), vec![]);
    assert_eq!(store.favicons.serialize(), SerializedBrowserFaviconsSnapshot::default());
    // Handle should be safely callable.
    handle.update(&BrowserHistoryUpdate { title: Some("B".to_owned()), ..Default::default() });
    handle.delete();

    store.dispose();
}

#[test]
fn handle_update_propagates_to_entry_and_registers_the_favicon() {
    let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
    let store = BrowserHistoryStore::new(None);
    let handle = store.add("https://a/", "", None, false);
    handle.update(&BrowserHistoryUpdate { title: Some("A".to_owned()), favicon: Some(Some("data:image/png;base64,XXX".to_owned())), ..Default::default() });

    let entry = store.entries.items().remove(0);
    assert_eq!(entry.title, "A");
    assert_ne!(entry.icon, None);
    assert_eq!(store.favicons.get(&entry.icon.unwrap()).as_deref(), Some("data:image/png;base64,XXX"));

    store.dispose();
}

#[test]
fn handle_update_with_explicit_favicon_null_clears_the_entry_icon() {
    let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
    let store = BrowserHistoryStore::new(None);
    let handle = store.add("https://a/", "A", Some("data:image/png;base64,XXX"), false);
    assert_ne!(store.entries.items()[0].icon, None);

    handle.update(&BrowserHistoryUpdate { favicon: Some(None), ..Default::default() });
    assert_eq!(store.entries.items()[0].icon, None);

    store.dispose();
}

#[test]
fn handle_delete_removes_the_entry_and_gcs_the_orphaned_favicon() {
    let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
    let store = BrowserHistoryStore::new(None);
    let handle = store.add("https://a/", "A", Some("data:image/png;base64,XXX"), false);
    let icon_hash = store.entries.items()[0].icon.clone().unwrap();
    assert_eq!(store.favicons.get(&icon_hash).as_deref(), Some("data:image/png;base64,XXX"));

    handle.delete();
    assert_eq!(store.entries.items(), vec![]);
    assert_eq!(store.favicons.get(&icon_hash), None);

    store.dispose();
}

#[test]
fn favicons_referenced_by_other_entries_are_kept_on_delete() {
    let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
    let store = BrowserHistoryStore::new(None);
    let a = store.add("https://a/", "A", Some("data:image/png;base64,XXX"), false);
    store.add("https://b/", "B", Some("data:image/png;base64,XXX"), false);
    let icon_hash = store.entries.items()[0].icon.clone().unwrap();

    a.delete();
    assert_eq!(store.favicons.get(&icon_hash).as_deref(), Some("data:image/png;base64,XXX"));

    store.dispose();
}

#[test]
fn clear_wipes_entries_and_favicons_together() {
    let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
    let store = BrowserHistoryStore::new(None);
    store.add("https://a/", "A", Some("data:image/png;base64,XXX"), false);
    store.add("https://b/", "B", Some("data:image/png;base64,YYY"), false);

    store.clear();
    assert_eq!(store.entries.items(), vec![]);
    assert_eq!(store.favicons.serialize(), SerializedBrowserFaviconsSnapshot::default());

    store.dispose();
}

#[test]
fn store_on_did_change_fires_for_changes_in_either_sub_store() {
    let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
    let store = BrowserHistoryStore::new(None);
    let (count, listener) = counter();
    let sub = store.on_did_change().listen(listener);

    let handle = store.add("https://a/", "A", Some("data:image/png;base64,XXX"), false);
    // add fired: register favicon (+1), add entry (+1), favicon GC may also fire
    let after1 = count.get();
    assert!(after1 >= 2);

    handle.update(&BrowserHistoryUpdate { title: Some("A2".to_owned()), ..Default::default() }); // entry change: at least one more
    assert!(count.get() > after1);

    sub.dispose();
    store.dispose();
}
