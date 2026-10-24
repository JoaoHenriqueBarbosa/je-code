//! Porte de `src/vs/workbench/services/search/test/common/queryBuilder.test.ts` (suíte `QueryBuilderCommon`), caso a
//! caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` vira o `LeakGuard` criado no começo de cada caso, depois do `setup`.
//! - `glob.match(pattern, path)` com um texto é `match_pattern` e devolve `bool`.
//! - `isWindows` é o `is_windows` da plataforma detectada.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use std::rc::Rc;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::glob::{self, GlobOptions};
use je_base::platform::Platform;
use je_platform::resources::uri_file;
use je_platform::workspace::{IWorkspaceContextService, test_workspace};
use je_workbench::services::search::query_builder::{escape_glob_pattern, resolve_resources_for_search_includes};
use je_workbench::test::common::workbench_test_services::TestContextService;

/// `setup`: o `TestContextService` com o workspace `testWorkspace(URI.file('/testWorkspace'))`.
fn setup() -> Rc<dyn IWorkspaceContextService> {
    let is_windows = Platform::detect().is_windows;
    let workspace = test_workspace(&[uri_file(if is_windows { "C:\\testWorkspace" } else { "/testWorkspace" })]);
    TestContextService::new(Some(workspace), None)
}

fn path_of(posix: &str, windows: &str) -> String {
    (if Platform::detect().is_windows { windows } else { posix }).to_string()
}

#[test]
fn resolve_resources_for_search_includes_passes_through_paths_without_special_glob_characters() {
    let context = setup();
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let actual = resolve_resources_for_search_includes(&[uri_file(&path_of("/testWorkspace/pages/blog", "C:\\testWorkspace\\pages\\blog"))], context.as_ref());
    assert_eq!(actual, vec!["./pages/blog".to_string()]);
}

#[test]
fn resolve_resources_for_search_includes_escapes_paths_with_special_characters() {
    let context = setup();
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let actual = resolve_resources_for_search_includes(&[uri_file(&path_of("/testWorkspace/pages/blog/[postId]", "C:\\testWorkspace\\pages\\blog\\[postId]"))], context.as_ref());
    assert_eq!(actual, vec!["./pages/blog/[[]postId[]]".to_string()]);
}

#[test]
fn escape_glob_pattern_properly_escapes_square_brackets_for_literal_matching() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    // This test verifies the fix for issue #233049 where files with square brackets in names
    // were not found when using "Search Only in Open Editors"

    // Test file name with square brackets
    let file_name = "file[test].txt";

    // Without escaping, the pattern treats [test] as a character class
    let unescaped_result = glob::match_pattern(file_name, file_name, &GlobOptions::default());
    assert!(!unescaped_result, "Unescaped pattern should not match due to character class interpretation");

    // With escaping, the pattern matches literally
    let escaped_pattern = escape_glob_pattern(file_name);
    let escaped_result = glob::match_pattern(escaped_pattern.as_str(), file_name, &GlobOptions::default());
    assert!(escaped_result, "Escaped pattern should match literally");
    assert_eq!(escaped_pattern, "file[[]test[]].txt", "Pattern should have escaped brackets");
}
