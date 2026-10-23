//! Porte de `src/vs/base/test/node/ps.test.ts` (suíte `Process Utils` > `JS file regex`), caso a caso e na
//! mesma ordem.
//!
//! Notas de tradução: `findJsFiles` percorre `JS_FILENAME_PATTERN.exec` até o fim, que é o
//! `js_filename_matches` (a regex global virou função escrita à mão).

mod common;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::node::ps::js_filename_matches as find_js_files;

fn strings(items: &[&str]) -> Vec<String> {
    items.iter().map(|s| (*s).to_owned()).collect()
}

#[test]
fn should_match_simple_js_files() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(find_js_files("node bootstrap.js"), strings(&["bootstrap.js"]));
}

#[test]
fn should_match_multiple_js_files() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(find_js_files("node server.js --require helper.js"), strings(&["server.js", "helper.js"]));
}

#[test]
fn should_match_js_files_with_hyphens() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(find_js_files("node my-script.js"), strings(&["my-script.js"]));
}

#[test]
fn should_not_match_json_files() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(find_js_files("cat package.json"), Vec::<String>::new());
}

#[test]
fn should_not_match_js_prefix_in_json_extension_regression_test_for_b_fix() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    // Without the \b word boundary, the regex would incorrectly match "package.js" from "package.json"
    assert_eq!(find_js_files("node --config tsconfig.json"), Vec::<String>::new());
    assert_eq!(find_js_files("eslint.json"), Vec::<String>::new());
}

#[test]
fn should_not_match_jsx_files() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(find_js_files("node component.jsx"), Vec::<String>::new());
}

#[test]
fn should_match_js_but_not_json_in_same_command() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(find_js_files("node app.js --config settings.json"), strings(&["app.js"]));
}

#[test]
fn should_not_match_partial_matches_inside_other_extensions() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(find_js_files("file.jsmith"), Vec::<String>::new());
}

#[test]
fn should_match_js_at_end_of_command() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(find_js_files("/path/to/script.js"), strings(&["script.js"]));
}
