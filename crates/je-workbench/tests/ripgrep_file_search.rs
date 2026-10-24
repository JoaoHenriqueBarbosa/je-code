//! Porte de `src/vs/workbench/services/search/test/node/ripgrepFileSearch.test.ts` (suíte `RipgrepFileSearch - etc`),
//! caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` vira o `LeakGuard` criado no começo de cada caso.
//! - `(!platform.isWindows ? test.skip : test)` é `#[cfg_attr(not(windows), ignore)]` no caso do Windows, e
//!   `(platform.isWindows ? test.skip : test)` é `#[cfg_attr(windows, ignore)]` no do posix.
//! - `testGetAbsGlob(params)` recebe `[folder, glob, expectedResult]`; a mensagem do `assert.strictEqual` é o
//!   `JSON.stringify(params)`, aqui o `{params:?}`.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_workbench::services::search::ripgrep_file_search::{fix_drive_c, get_absolute_glob};

fn test_get_abs_glob(params: [&str; 3]) {
    let [folder, glob, expected_result] = params;
    assert_eq!(fix_drive_c(&get_absolute_glob(folder, glob)), expected_result, "{params:?}");
}

#[test]
#[cfg_attr(not(windows), ignore = "só roda no Windows")]
fn get_absolute_glob_win() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    [
        ["C:/foo/bar", "glob/**", "/foo\\bar\\glob\\**"],
        ["c:/", "glob/**", "/glob\\**"],
        ["C:\\foo\\bar", "glob\\**", "/foo\\bar\\glob\\**"],
        ["c:\\foo\\bar", "glob\\**", "/foo\\bar\\glob\\**"],
        ["c:\\", "glob\\**", "/glob\\**"],
        ["\\\\localhost\\c$\\foo\\bar", "glob/**", "\\\\localhost\\c$\\foo\\bar\\glob\\**"],
        // absolute paths are not resolved further
        ["c:/foo/bar", "/path/something", "/path/something"],
        ["c:/foo/bar", "c:\\project\\folder", "/project\\folder"],
    ]
    .into_iter()
    .for_each(test_get_abs_glob);
}

#[test]
#[cfg_attr(windows, ignore = "não roda no Windows")]
fn get_absolute_glob_posix() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    [
        ["/foo/bar", "glob/**", "/foo/bar/glob/**"],
        ["/", "glob/**", "/glob/**"],
        // absolute paths are not resolved further
        ["/", "/project/folder", "/project/folder"],
    ]
    .into_iter()
    .for_each(test_get_abs_glob);
}
