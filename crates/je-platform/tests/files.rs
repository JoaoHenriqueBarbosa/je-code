//! Porte de `src/vs/platform/files/test/common/files.test.ts` (suíte `Files`), caso a caso e na mesma ordem.
//!
//! Divergências: o `this.test.fullTitle()` do mocha, que o `toResource` usa para montar o diretório, vira o
//! parâmetro `full_title` de [`common::files::to_resource`] (aqui, `"Files <nome do caso>"`). Os ramos
//! `isWindows` de `isParent` e `isEqualOrParent` não rodam no Linux nem no macOS, como no original. As
//! chamadas com `null`/`undefined` de `testIsEqual` não existem em Rust (os parâmetros são `&str`).

mod common;

use common::files::to_resource;
use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::path::{Platform, ProcessEnvironment};
use je_base::uri::URI;
use je_platform::files::files::*;

use FileChangeType::{Added, Deleted, Updated};

fn change(resource: URI, change_type: FileChangeType) -> FileChange {
    FileChange::new(change_type, resource)
}

fn with_correlation(resource: URI, change_type: FileChangeType, c_id: i64) -> FileChange {
    FileChange { c_id: Some(c_id), ..FileChange::new(change_type, resource) }
}

#[test]
fn file_changes_event_basics() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let r = |path: &str| to_resource("Files FileChangesEvent - basics", path);
    let changes = vec![
        change(r("/foo/updated.txt"), Updated),
        change(r("/foo/otherupdated.txt"), Updated),
        change(r("/added.txt"), Added),
        change(r("/bar/deleted.txt"), Deleted),
        change(r("/bar/folder"), Deleted),
        change(r("/BAR/FOLDER"), Deleted),
    ];

    for ignore_path_casing in [false, true] {
        let event = FileChangesEvent::new(&changes, ignore_path_casing);

        assert!(!event.contains(&r("/foo"), &[Updated]));
        assert!(event.affects(&r("/foo"), &[Updated]));
        assert!(event.contains(&r("/foo/updated.txt"), &[Updated]));
        assert!(event.affects(&r("/foo/updated.txt"), &[Updated]));
        assert!(event.contains(&r("/foo/updated.txt"), &[Updated, Added]));
        assert!(event.affects(&r("/foo/updated.txt"), &[Updated, Added]));
        assert!(event.contains(&r("/foo/updated.txt"), &[Updated, Added, Deleted]));
        assert!(!event.contains(&r("/foo/updated.txt"), &[Added, Deleted]));
        assert!(!event.contains(&r("/foo/updated.txt"), &[Added]));
        assert!(!event.contains(&r("/foo/updated.txt"), &[Deleted]));
        assert!(!event.affects(&r("/foo/updated.txt"), &[Deleted]));

        assert!(event.contains(&r("/bar/folder"), &[Deleted]));
        assert!(event.contains(&r("/BAR/FOLDER"), &[Deleted]));
        assert!(event.affects(&r("/BAR"), &[Deleted]));
        if ignore_path_casing {
            assert!(event.contains(&r("/BAR/folder"), &[Deleted]));
            assert!(event.affects(&r("/bar"), &[Deleted]));
        } else {
            assert!(!event.contains(&r("/BAR/folder"), &[Deleted]));
            assert!(event.affects(&r("/bar"), &[Deleted]));
        }
        assert!(event.contains(&r("/bar/folder/somefile"), &[Deleted]));
        assert!(event.contains(&r("/bar/folder/somefile/test.txt"), &[Deleted]));
        assert!(event.contains(&r("/BAR/FOLDER/somefile/test.txt"), &[Deleted]));
        if ignore_path_casing {
            assert!(event.contains(&r("/BAR/folder/somefile/test.txt"), &[Deleted]));
        } else {
            assert!(!event.contains(&r("/BAR/folder/somefile/test.txt"), &[Deleted]));
        }
        assert!(!event.contains(&r("/bar/folder2/somefile"), &[Deleted]));

        assert_eq!(1, event.raw_added.len());
        assert_eq!(2, event.raw_updated.len());
        assert_eq!(3, event.raw_deleted.len());
        assert!(event.got_added());
        assert!(event.got_updated());
        assert!(event.got_deleted());
    }
}

#[test]
fn file_changes_event_supports_multiple_changes_on_file_tree() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let r = |path: &str| to_resource("Files FileChangesEvent - supports multiple changes on file tree", path);
    for change_type in [Added, Updated, Deleted] {
        let changes: Vec<FileChange> = [
            "/foo/bar/updated.txt",
            "/foo/bar/otherupdated.txt",
            "/foo/bar",
            "/foo",
            "/bar",
            "/bar/foo",
            "/bar/foo/updated.txt",
            "/bar/foo/otherupdated.txt",
        ]
        .iter()
        .map(|path| change(r(path), change_type))
        .collect();

        for ignore_path_casing in [false, true] {
            let event = FileChangesEvent::new(&changes, ignore_path_casing);

            for change in &changes {
                assert!(event.contains(&change.resource, &[change_type]));
                assert!(event.affects(&change.resource, &[change_type]));
            }

            assert!(event.affects(&r("/foo"), &[change_type]));
            assert!(event.affects(&r("/bar"), &[change_type]));
            assert!(event.affects(&r("/"), &[change_type]));
            assert!(!event.affects(&r("/foobar"), &[change_type]));

            assert!(!event.contains(&r("/some/foo/bar"), &[change_type]));
            assert!(!event.affects(&r("/some/foo/bar"), &[change_type]));
            assert!(!event.contains(&r("/some/bar"), &[change_type]));
            assert!(!event.affects(&r("/some/bar"), &[change_type]));

            match change_type {
                Added => assert_eq!(8, event.raw_added.len()),
                Deleted => assert_eq!(8, event.raw_deleted.len()),
                Updated => {}
            }
        }
    }
}

#[test]
fn file_changes_event_correlation() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let r = |path: &str| to_resource("Files FileChangesEvent - correlation", path);

    let mut changes = vec![change(r("/foo/updated.txt"), Updated), change(r("/foo/otherupdated.txt"), Updated), change(r("/added.txt"), Added)];

    let mut event = FileChangesEvent::new(&changes, true);
    assert!(!event.has_correlation());
    assert!(!event.correlates(100));

    changes = vec![
        with_correlation(r("/foo/updated.txt"), Updated, 100),
        with_correlation(r("/foo/otherupdated.txt"), Updated, 100),
        with_correlation(r("/added.txt"), Added, 100),
    ];

    event = FileChangesEvent::new(&changes, true);
    assert!(event.has_correlation());
    assert!(event.correlates(100));
    assert!(!event.correlates(120));

    changes = vec![
        with_correlation(r("/foo/updated.txt"), Updated, 100),
        change(r("/foo/otherupdated.txt"), Updated),
        with_correlation(r("/added.txt"), Added, 100),
    ];

    event = FileChangesEvent::new(&changes, true);
    assert!(!event.has_correlation());
    assert!(!event.correlates(100));
    assert!(!event.correlates(120));

    changes = vec![
        with_correlation(r("/foo/updated.txt"), Updated, 100),
        with_correlation(r("/foo/otherupdated.txt"), Updated, 120),
        with_correlation(r("/added.txt"), Added, 100),
    ];

    event = FileChangesEvent::new(&changes, true);
    assert!(!event.has_correlation());
    assert!(!event.correlates(100));
    assert!(!event.correlates(120));
}

/// `testIsEqual(testMethod)`.
fn test_is_equal(test_method: impl Fn(&str, &str, bool) -> bool) {
    // corner cases
    assert!(test_method("", "", true));

    // basics (string)
    assert!(test_method("/", "/", true));
    assert!(test_method("/some", "/some", true));
    assert!(test_method("/some/path", "/some/path", true));

    assert!(test_method("c:\\", "c:\\", true));
    assert!(test_method("c:\\some", "c:\\some", true));
    assert!(test_method("c:\\some\\path", "c:\\some\\path", true));

    assert!(test_method("/someöäü/path", "/someöäü/path", true));
    assert!(test_method("c:\\someöäü\\path", "c:\\someöäü\\path", true));

    assert!(!test_method("/some/path", "/some/other/path", true));
    assert!(!test_method("c:\\some\\path", "c:\\some\\other\\path", true));
    assert!(!test_method("c:\\some\\path", "d:\\some\\path", true));

    assert!(test_method("/some/path", "/some/PATH", true));
    assert!(test_method("/someöäü/path", "/someÖÄÜ/PATH", true));
    assert!(test_method("c:\\some\\path", "c:\\some\\PATH", true));
    assert!(test_method("c:\\someöäü\\path", "c:\\someÖÄÜ\\PATH", true));
    assert!(test_method("c:\\some\\path", "C:\\some\\PATH", true));
}

/// O `isEqualOrParent` de `extpath` com os argumentos do original (sem `forcePosixSemantics`).
fn is_equal_or_parent(base: &str, parent_candidate: &str, ignore_case: bool) -> bool {
    je_base::extpath::is_equal_or_parent(base, parent_candidate, ignore_case, false, &ProcessEnvironment)
}

#[test]
fn is_equal_ignore_case() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    test_is_equal(je_base::extpath::is_equal);

    // basics (uris)
    let fs_path = |path: &str| URI::file(path, Platform::Posix).fs_path(Platform::Posix).to_string_lossy();
    let is_equal = je_base::extpath::is_equal;
    assert!(is_equal(&fs_path("/some/path"), &fs_path("/some/path"), true));
    assert!(is_equal(&fs_path("c:\\some\\path"), &fs_path("c:\\some\\path"), true));

    assert!(is_equal(&fs_path("/someöäü/path"), &fs_path("/someöäü/path"), true));
    assert!(is_equal(&fs_path("c:\\someöäü\\path"), &fs_path("c:\\someöäü\\path"), true));

    assert!(!is_equal(&fs_path("/some/path"), &fs_path("/some/other/path"), true));
    assert!(!is_equal(&fs_path("c:\\some\\path"), &fs_path("c:\\some\\other\\path"), true));

    assert!(is_equal(&fs_path("/some/path"), &fs_path("/some/PATH"), true));
    assert!(is_equal(&fs_path("/someöäü/path"), &fs_path("/someÖÄÜ/PATH"), true));
    assert!(is_equal(&fs_path("c:\\some\\path"), &fs_path("c:\\some\\PATH"), true));
    assert!(is_equal(&fs_path("c:\\someöäü\\path"), &fs_path("c:\\someÖÄÜ\\PATH"), true));
    assert!(is_equal(&fs_path("c:\\some\\path"), &fs_path("C:\\some\\PATH"), true));
}

#[test]
fn is_parent_ignorecase() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    if cfg!(windows) {
        assert!(is_parent("c:\\some\\path", "c:\\", true));
        assert!(is_parent("c:\\some\\path", "c:\\some", true));
        assert!(is_parent("c:\\some\\path", "c:\\some\\", true));
        assert!(is_parent("c:\\someöäü\\path", "c:\\someöäü", true));
        assert!(is_parent("c:\\someöäü\\path", "c:\\someöäü\\", true));
        assert!(is_parent("c:\\foo\\bar\\test.ts", "c:\\foo\\bar", true));
        assert!(is_parent("c:\\foo\\bar\\test.ts", "c:\\foo\\bar\\", true));

        assert!(is_parent("c:\\some\\path", "C:\\", true));
        assert!(is_parent("c:\\some\\path", "c:\\SOME", true));
        assert!(is_parent("c:\\some\\path", "c:\\SOME\\", true));

        assert!(!is_parent("c:\\some\\path", "d:\\", true));
        assert!(!is_parent("c:\\some\\path", "c:\\some\\path", true));
        assert!(!is_parent("c:\\some\\path", "d:\\some\\path", true));
        assert!(!is_parent("c:\\foo\\bar\\test.ts", "c:\\foo\\barr", true));
        assert!(!is_parent("c:\\foo\\bar\\test.ts", "c:\\foo\\bar\\test", true));
    } else {
        assert!(is_parent("/some/path", "/", true));
        assert!(is_parent("/some/path", "/some", true));
        assert!(is_parent("/some/path", "/some/", true));
        assert!(is_parent("/someöäü/path", "/someöäü", true));
        assert!(is_parent("/someöäü/path", "/someöäü/", true));
        assert!(is_parent("/foo/bar/test.ts", "/foo/bar", true));
        assert!(is_parent("/foo/bar/test.ts", "/foo/bar/", true));

        assert!(is_parent("/some/path", "/SOME", true));
        assert!(is_parent("/some/path", "/SOME/", true));
        assert!(is_parent("/someöäü/path", "/SOMEÖÄÜ", true));
        assert!(is_parent("/someöäü/path", "/SOMEÖÄÜ/", true));

        assert!(!is_parent("/some/path", "/some/path", true));
        assert!(!is_parent("/foo/bar/test.ts", "/foo/barr", true));
        assert!(!is_parent("/foo/bar/test.ts", "/foo/bar/test", true));
    }
}

#[test]
fn is_equal_or_parent_ignorecase() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    // same assertions apply as with isEqual()
    test_is_equal(is_equal_or_parent);

    if cfg!(windows) {
        assert!(is_equal_or_parent("c:\\some\\path", "c:\\", true));
        assert!(is_equal_or_parent("c:\\some\\path", "c:\\some", true));
        assert!(is_equal_or_parent("c:\\some\\path", "c:\\some\\", true));
        assert!(is_equal_or_parent("c:\\someöäü\\path", "c:\\someöäü", true));
        assert!(is_equal_or_parent("c:\\someöäü\\path", "c:\\someöäü\\", true));
        assert!(is_equal_or_parent("c:\\foo\\bar\\test.ts", "c:\\foo\\bar", true));
        assert!(is_equal_or_parent("c:\\foo\\bar\\test.ts", "c:\\foo\\bar\\", true));
        assert!(is_equal_or_parent("c:\\some\\path", "c:\\some\\path", true));
        assert!(is_equal_or_parent("c:\\foo\\bar\\test.ts", "c:\\foo\\bar\\test.ts", true));

        assert!(is_equal_or_parent("c:\\some\\path", "C:\\", true));
        assert!(is_equal_or_parent("c:\\some\\path", "c:\\SOME", true));
        assert!(is_equal_or_parent("c:\\some\\path", "c:\\SOME\\", true));

        assert!(!is_equal_or_parent("c:\\some\\path", "d:\\", true));
        assert!(!is_equal_or_parent("c:\\some\\path", "d:\\some\\path", true));
        assert!(!is_equal_or_parent("c:\\foo\\bar\\test.ts", "c:\\foo\\barr", true));
        assert!(!is_equal_or_parent("c:\\foo\\bar\\test.ts", "c:\\foo\\bar\\test", true));
        assert!(!is_equal_or_parent("c:\\foo\\bar\\test.ts", "c:\\foo\\bar\\test.", true));
        assert!(!is_equal_or_parent("c:\\foo\\bar\\test.ts", "c:\\foo\\BAR\\test.", true));
    } else {
        assert!(is_equal_or_parent("/some/path", "/", true));
        assert!(is_equal_or_parent("/some/path", "/some", true));
        assert!(is_equal_or_parent("/some/path", "/some/", true));
        assert!(is_equal_or_parent("/someöäü/path", "/someöäü", true));
        assert!(is_equal_or_parent("/someöäü/path", "/someöäü/", true));
        assert!(is_equal_or_parent("/foo/bar/test.ts", "/foo/bar", true));
        assert!(is_equal_or_parent("/foo/bar/test.ts", "/foo/bar/", true));
        assert!(is_equal_or_parent("/some/path", "/some/path", true));

        assert!(is_equal_or_parent("/some/path", "/SOME", true));
        assert!(is_equal_or_parent("/some/path", "/SOME/", true));
        assert!(is_equal_or_parent("/someöäü/path", "/SOMEÖÄÜ", true));
        assert!(is_equal_or_parent("/someöäü/path", "/SOMEÖÄÜ/", true));

        assert!(!is_equal_or_parent("/foo/bar/test.ts", "/foo/barr", true));
        assert!(!is_equal_or_parent("/foo/bar/test.ts", "/foo/bar/test", true));
        assert!(!is_equal_or_parent("foo/bar/test.ts", "foo/bar/test.", true));
        assert!(!is_equal_or_parent("foo/bar/test.ts", "foo/BAR/test.", true));
    }
}
