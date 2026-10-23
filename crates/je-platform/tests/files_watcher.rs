//! Porte de `src/vs/platform/files/test/common/watcher.test.ts` (suíte `Watcher`), caso a caso e na mesma ordem.
//!
//! Cobre as duas suítes do arquivo original: `Watcher` (`parseWatcherPatterns`) e `Watcher Events Normalizer`
//! (`coalesceEvents`, `isFiltered`). Os casos `(isWindows ? test.skip : test)` e `(!isWindows ? test.skip :
//! test)` viram `#[ignore]` quando a plataforma não é a deles, mantendo a contagem do original.
//!
//! Divergências: o `done` do mocha vira um `Cell` conferido depois do `report` (o listener roda de forma
//! síncrona); `reviveFileChanges` não existe no porte (ele só reconstrói `URI` de objetos serializados, e
//! aqui os `URI` já chegam como valores); `toFileChangesEvent` usa `!isLinux` como no original.

mod common;

use std::cell::Cell;
use std::rc::Rc;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::event::{Emitter, Event};
use je_base::glob::PatternItem;
use je_base::path::ProcessEnvironment;
use je_base::resources::ExtUri;
use je_base::uri::URI;
use je_platform::files::files::*;
use je_platform::files::watcher::{coalesce_events, is_filtered, parse_watcher_patterns};

use FileChangeType::{Added, Deleted, Updated};

/// Um `parseWatcherPatterns(path, [pattern], ignoreCase)[0]` aplicado a `candidate`.
fn parsed(path: &str, pattern: &str, ignore_case: bool) -> impl Fn(&str) -> bool {
    let pattern = parse_watcher_patterns(path, &[PatternItem::from(pattern)], ignore_case).remove(0);
    move |candidate| pattern.matches(candidate, None)
}

/// Confere `foo.js`, `foo.ts` e `bar/foo.js` (sob `dir`, com `sep`) contra o padrão.
fn check_foo(pattern: &dyn Fn(&str) -> bool, dir: &str, sep: &str, expected: [bool; 3]) {
    assert_eq!(pattern(&format!("{dir}{sep}foo.js")), expected[0]);
    assert_eq!(pattern(&format!("{dir}{sep}foo.ts")), expected[1]);
    assert_eq!(pattern(&format!("{dir}{sep}bar{sep}foo.js")), expected[2]);
}

#[test]
#[cfg_attr(windows, ignore = "test.skip no Windows, como no original")]
fn parse_watcher_patterns_posix() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let path = "/users/data/src";
    check_foo(&parsed(path, "*.js", false), path, "/", [true, false, false]);
    check_foo(&parsed(path, "/users/data/src/*.js", false), path, "/", [true, false, false]);
    check_foo(&parsed(path, "/users/data/src/bar/*.js", false), path, "/", [false, false, true]);
    check_foo(&parsed(path, "**/*.js", false), path, "/", [true, false, true]);
}

#[test]
#[cfg_attr(not(windows), ignore = "test.skip fora do Windows, como no original")]
fn parse_watcher_patterns_windows() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let path = "c:\\users\\data\\src";
    // O original tem `bar/foo.js` com barra normal no primeiro padrão e `bar\foo.js` nos demais.
    let first = parsed(path, "*.js", true);
    assert!(first("c:\\users\\data\\src\\foo.js"));
    assert!(!first("c:\\users\\data\\src\\foo.ts"));
    assert!(!first("c:\\users\\data\\src\\bar/foo.js"));

    check_foo(&parsed(path, "c:\\users\\data\\src\\*.js", true), path, "\\", [true, false, false]);
    check_foo(&parsed(path, "c:\\users\\data\\src\\bar/*.js", true), path, "\\", [false, false, true]);
    check_foo(&parsed(path, "**/*.js", true), path, "\\", [true, false, true]);
}

#[test]
#[cfg_attr(windows, ignore = "test.skip no Windows, como no original")]
fn parse_watcher_patterns_posix_case_insensitive() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let path = "/users/data/src";
    let mut parsed_pattern = parsed(path, "*.JS", false);

    // Case sensitive by default on posix
    assert!(!parsed_pattern("/users/data/src/foo.js"));
    assert!(parsed_pattern("/users/data/src/foo.JS"));
    assert!(!parsed_pattern("/users/data/src/foo.Js"));

    // Now test with GlobCaseSensitivity.caseInsensitive
    parsed_pattern = parsed(path, "*.JS", true);

    assert!(parsed_pattern("/users/data/src/foo.js"));
    assert!(parsed_pattern("/users/data/src/foo.JS"));
    assert!(parsed_pattern("/users/data/src/foo.Js"));
    assert!(!parsed_pattern("/users/data/src/foo.ts"));

    parsed_pattern = parsed(path, "/users/data/src/*.JS", true);

    assert!(parsed_pattern("/users/data/src/foo.js"));
    assert!(parsed_pattern("/users/data/src/foo.JS"));
    assert!(!parsed_pattern("/users/data/src/foo.ts"));
    assert!(!parsed_pattern("/users/data/src/bar/foo.js"));

    parsed_pattern = parsed(path, "**/Test*.JS", true);

    assert!(parsed_pattern("/users/data/src/test1.js"));
    assert!(parsed_pattern("/users/data/src/Test1.js"));
    assert!(parsed_pattern("/users/data/src/TEST1.JS"));
    assert!(parsed_pattern("/users/data/src/bar/test2.js"));
    assert!(parsed_pattern("/users/data/src/bar/TEST2.JS"));
    assert!(!parsed_pattern("/users/data/src/foo.js"));
}

#[test]
#[cfg_attr(not(windows), ignore = "test.skip fora do Windows, como no original")]
fn parse_watcher_patterns_windows_case_insensitive() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let path = "c:\\users\\data\\src";
    let mut parsed_pattern = parsed(path, "*.JS", true);

    // Windows is case insensitive by default
    assert!(parsed_pattern("c:\\users\\data\\src\\foo.js"));
    assert!(parsed_pattern("c:\\users\\data\\src\\foo.JS"));
    assert!(parsed_pattern("c:\\users\\data\\src\\foo.Js"));
    assert!(!parsed_pattern("c:\\users\\data\\src\\foo.ts"));

    // Explicit GlobCaseSensitivity.caseInsensitive should work the same
    parsed_pattern = parsed(path, "*.JS", true);

    assert!(parsed_pattern("c:\\users\\data\\src\\foo.js"));
    assert!(parsed_pattern("c:\\users\\data\\src\\foo.JS"));
    assert!(parsed_pattern("c:\\users\\data\\src\\foo.Js"));
    assert!(!parsed_pattern("c:\\users\\data\\src\\foo.ts"));

    parsed_pattern = parsed(path, "c:\\users\\data\\src\\*.JS", true);

    assert!(parsed_pattern("c:\\users\\data\\src\\foo.js"));
    assert!(parsed_pattern("c:\\users\\data\\src\\foo.JS"));
    assert!(!parsed_pattern("c:\\users\\data\\src\\foo.ts"));
    assert!(!parsed_pattern("c:\\users\\data\\src\\bar\\foo.js"));

    parsed_pattern = parsed(path, "**/Test*.JS", true);

    assert!(parsed_pattern("c:\\users\\data\\src\\test1.js"));
    assert!(parsed_pattern("c:\\users\\data\\src\\Test1.js"));
    assert!(parsed_pattern("c:\\users\\data\\src\\TEST1.JS"));
    assert!(parsed_pattern("c:\\users\\data\\src\\bar\\test2.js"));
    assert!(parsed_pattern("c:\\users\\data\\src\\bar\\TEST2.JS"));
    assert!(!parsed_pattern("c:\\users\\data\\src\\foo.js"));

    // Test with case sensitive mode explicitly
    parsed_pattern = parsed(path, "*.JS", false);

    assert!(!parsed_pattern("c:\\users\\data\\src\\foo.js"));
    assert!(parsed_pattern("c:\\users\\data\\src\\foo.JS"));
    assert!(!parsed_pattern("c:\\users\\data\\src\\foo.Js"));
}

/// `TestFileWatcher`: o que o `report` coalesce é entregue por `onDidFilesChange`.
struct TestFileWatcher {
    on_did_files_change: Rc<Emitter<(Vec<FileChange>, FileChangesEvent)>>,
}

impl TestFileWatcher {
    fn new(guard: &common::LeakGuard) -> Self {
        Self { on_did_files_change: guard.add_rc(Emitter::new()) }
    }

    fn on_did_files_change(&self) -> Event<(Vec<FileChange>, FileChangesEvent)> {
        self.on_did_files_change.event()
    }

    fn report(&self, changes: Vec<FileChange>) {
        // Coalesce
        let coalesced_events = coalesce_events(changes);

        // Emit through event emitter
        if !coalesced_events.is_empty() {
            let event = FileChangesEvent::new(&coalesced_events, !cfg!(target_os = "linux"));
            self.on_did_files_change.fire((coalesced_events, event));
        }
    }
}

/// Registra o listener de `watch.onDidFilesChange` e devolve o flag que faz o papel do `done()` do mocha.
fn expect_change(guard: &common::LeakGuard, watch: &TestFileWatcher, check: impl Fn(&[FileChange], &FileChangesEvent) + 'static) -> Rc<Cell<bool>> {
    let done = Rc::new(Cell::new(false));
    let finished = done.clone();
    guard.add(watch.on_did_files_change().listen(move |(raw, event)| {
        check(raw, event);
        finished.set(true);
    }));
    done
}

fn change(resource: &URI, change_type: FileChangeType) -> FileChange {
    FileChange::new(change_type, resource.clone())
}

fn file(path: &str) -> URI {
    common::uri_file(path)
}

#[test]
fn simple_add_update_delete() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let watch = TestFileWatcher::new(&guard);

    let added = file("/users/data/src/added.txt");
    let updated = file("/users/data/src/updated.txt");
    let deleted = file("/users/data/src/deleted.txt");

    let raw = vec![change(&added, Added), change(&updated, Updated), change(&deleted, Deleted)];

    let done = expect_change(&guard, &watch, {
        let (added, updated, deleted) = (added.clone(), updated.clone(), deleted.clone());
        move |raw, event| {
            assert_eq!(raw.len(), 3);
            assert!(event.contains(&added, &[Added]));
            assert!(event.contains(&updated, &[Updated]));
            assert!(event.contains(&deleted, &[Deleted]));
        }
    });

    watch.report(raw);
    assert!(done.get());
}

#[test]
fn delete_only_reported_for_top_level_folder_unix() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let watch = TestFileWatcher::new(&guard);

    // No Windows o original roda as variantes WINDOWS e UNC; aqui só há a variante UNIX.
    let deleted_folder_a = file("/users/data/src/todelete1");
    let deleted_folder_b = file("/users/data/src/todelete2");
    let deleted_folder_bf1 = file("/users/data/src/todelete2/file.txt");
    let deleted_folder_bf2 = file("/users/data/src/todelete2/more/test.txt");
    let deleted_folder_bf3 = file("/users/data/src/todelete2/super/bar/foo.txt");
    let deleted_file_a = file("/users/data/src/deleteme.txt");

    let added_file = file("/users/data/src/added.txt");
    let updated_file = file("/users/data/src/updated.txt");

    let raw = vec![
        change(&deleted_folder_a, Deleted),
        change(&deleted_folder_b, Deleted),
        change(&deleted_folder_bf1, Deleted),
        change(&deleted_folder_bf2, Deleted),
        change(&deleted_folder_bf3, Deleted),
        change(&deleted_file_a, Deleted),
        change(&added_file, Added),
        change(&updated_file, Updated),
    ];

    let done = expect_change(&guard, &watch, move |raw, event| {
        assert_eq!(raw.len(), 5);

        assert!(event.contains(&deleted_folder_a, &[Deleted]));
        assert!(event.contains(&deleted_folder_b, &[Deleted]));
        assert!(event.contains(&deleted_file_a, &[Deleted]));
        assert!(event.contains(&added_file, &[Added]));
        assert!(event.contains(&updated_file, &[Updated]));
    });

    watch.report(raw);
    assert!(done.get());
}

#[test]
fn event_coalescer_ignore_create_followed_by_delete() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let watch = TestFileWatcher::new(&guard);

    let created = file("/users/data/src/related");
    let deleted = file("/users/data/src/related");
    let unrelated = file("/users/data/src/unrelated");

    let raw = vec![change(&created, Added), change(&deleted, Deleted), change(&unrelated, Updated)];

    let done = expect_change(&guard, &watch, move |raw, event| {
        assert_eq!(raw.len(), 1);

        assert!(event.contains(&unrelated, &[Updated]));
    });

    watch.report(raw);
    assert!(done.get());
}

#[test]
fn event_coalescer_flatten_delete_followed_by_create_into_change() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let watch = TestFileWatcher::new(&guard);

    let deleted = file("/users/data/src/related");
    let created = file("/users/data/src/related");
    let unrelated = file("/users/data/src/unrelated");

    let raw = vec![change(&deleted, Deleted), change(&created, Added), change(&unrelated, Updated)];

    let done = expect_change(&guard, &watch, {
        let deleted = deleted.clone();
        move |raw, event| {
            assert_eq!(raw.len(), 2);

            assert!(event.contains(&deleted, &[Updated]));
            assert!(event.contains(&unrelated, &[Updated]));
        }
    });

    watch.report(raw);
    assert!(done.get());
}

#[test]
fn event_coalescer_ignore_update_when_create_received() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let watch = TestFileWatcher::new(&guard);

    let created = file("/users/data/src/related");
    let updated = file("/users/data/src/related");
    let unrelated = file("/users/data/src/unrelated");

    let raw = vec![change(&created, Added), change(&updated, Updated), change(&unrelated, Updated)];

    let done = expect_change(&guard, &watch, move |raw, event| {
        assert_eq!(raw.len(), 2);

        assert!(event.contains(&created, &[Added]));
        assert!(!event.contains(&created, &[Updated]));
        assert!(event.contains(&unrelated, &[Updated]));
    });

    watch.report(raw);
    assert!(done.get());
}

#[test]
fn event_coalescer_apply_delete() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let watch = TestFileWatcher::new(&guard);

    let updated = file("/users/data/src/related");
    let updated2 = file("/users/data/src/related");
    let deleted = file("/users/data/src/related");
    let unrelated = file("/users/data/src/unrelated");

    let raw = vec![change(&updated, Updated), change(&updated2, Updated), change(&unrelated, Updated), change(&updated, Deleted)];

    let done = expect_change(&guard, &watch, move |raw, event| {
        assert_eq!(raw.len(), 2);

        assert!(event.contains(&deleted, &[Deleted]));
        assert!(!event.contains(&updated, &[Updated]));
        assert!(event.contains(&unrelated, &[Updated]));
    });

    watch.report(raw);
    assert!(done.get());
}

#[test]
fn event_coalescer_track_case_renames() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let watch = TestFileWatcher::new(&guard);

    let old_path = file("/users/data/src/added");
    let new_path = file("/users/data/src/ADDED");

    let raw = vec![change(&new_path, Added), change(&old_path, Deleted)];

    let done = expect_change(&guard, &watch, {
        let (old_path, new_path) = (old_path.clone(), new_path.clone());
        move |raw, _event| {
            assert_eq!(raw.len(), 2);

            let ext_uri = ExtUri::unbiased(Rc::new(ProcessEnvironment));
            for r in raw {
                if ext_uri.is_equal(Some(&r.resource), Some(&old_path), false) {
                    assert_eq!(r.change_type, Deleted);
                } else if ext_uri.is_equal(Some(&r.resource), Some(&new_path), false) {
                    assert_eq!(r.change_type, Added);
                } else {
                    panic!("assert.fail()");
                }
            }
        }
    });

    watch.report(raw);
    assert!(done.get());
}

#[test]
fn event_type_filter() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let resource = file("/users/data/src/related");
    let filtered = |change_type: FileChangeType, filter: Option<FileChangeFilter>| is_filtered(&change(&resource, change_type), filter);
    let (u, a, d) = (FileChangeFilter::UPDATED, FileChangeFilter::ADDED, FileChangeFilter::DELETED);

    assert!(!filtered(Added, None));
    assert!(!filtered(Updated, None));
    assert!(!filtered(Deleted, None));

    assert!(filtered(Added, Some(u)));
    assert!(filtered(Added, Some(u | d)));

    assert!(!filtered(Added, Some(a)));
    assert!(!filtered(Added, Some(a | u)));
    assert!(!filtered(Added, Some(a | u | d)));

    assert!(filtered(Deleted, Some(u)));
    assert!(filtered(Deleted, Some(u | a)));

    assert!(!filtered(Deleted, Some(d)));
    assert!(!filtered(Deleted, Some(d | u)));
    assert!(!filtered(Deleted, Some(a | d | u)));

    assert!(filtered(Updated, Some(a)));
    assert!(filtered(Updated, Some(d | a)));

    assert!(!filtered(Updated, Some(u)));
    assert!(!filtered(Updated, Some(d | u)));
    assert!(!filtered(Updated, Some(a | d | u)));
}
