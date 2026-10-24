//! Porte de `src/vs/workbench/contrib/testing/test/common/testExplorerFilterState.test.ts` (suíte
//! `TestExplorerFilterState`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - **`setup`, `teardown` e `ensureNoDisposablesAreLeakedInTestSuite()`.** O guarda é criado no começo de cada caso e o
//!   `setup` monta o `TestExplorerFilterState` sobre um `InMemoryStorageService` (o `TestStorageService` do porte); o
//!   `ds` do original é o store do guarda.
//! - **`assert.deepStrictEqual(t.includeTags, new Set(...))`** compara os `IndexSet` pelo conteúdo e pela ordem de
//!   inserção, que é o que o `deepStrictEqual` sobre `Set` e a leitura de `[...t.includeTags]` observam no original.
//! - **`assertFilteringFor({ ...termFiltersOff, [TestFilterTerm.CurrentDoc]: true })`** é [`assert_filtering_for`] com a
//!   lista de pares `(termo, esperado)`.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use std::rc::Rc;

use common::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite};
use indexmap::IndexSet;
use je_platform::storage::StorageService;
use je_workbench::contrib::testing::common::test_explorer_filter_state::{GlobFilter, ITestExplorerFilterState, TestExplorerFilterState, TestFilterTerm};
use je_workbench::test::common::workbench_test_services::TestStorageService;

/// O que o `setup` monta: o guarda de vazamentos e o estado do filtro.
fn setup() -> (LeakGuard, Rc<TestExplorerFilterState>) {
    let ds = ensure_no_disposables_are_leaked_in_test_suite();
    let storage_service: Rc<dyn StorageService> = Rc::new(ds.add_rc(TestStorageService::new()));
    let t = ds.add_rc(TestExplorerFilterState::new(storage_service));
    (ds, t)
}

/// `assertFilteringFor(expected)`.
fn assert_filtering_for(t: &TestExplorerFilterState, expected: &[(TestFilterTerm, bool)]) {
    for (term, expectation) in expected {
        assert_eq!(t.is_filtering_for(*term), *expectation, "expected filtering for {} === {expectation}", term.as_str());
    }
}

/// `termFiltersOff`, com o `CurrentDoc` ligado quando `current_doc` é verdadeiro (`{ ...termFiltersOff, CurrentDoc: true }`).
fn term_filters(current_doc: bool) -> Vec<(TestFilterTerm, bool)> {
    vec![
        (TestFilterTerm::Failed, false),
        (TestFilterTerm::Executed, false),
        (TestFilterTerm::CurrentDoc, current_doc),
        (TestFilterTerm::Hidden, false),
    ]
}

fn glob(text: &str, include: bool) -> GlobFilter {
    GlobFilter { text: text.to_owned(), include }
}

fn set(items: &[&str]) -> IndexSet<String> {
    items.iter().map(|item| (*item).to_owned()).collect()
}

#[test]
fn filters_simple_globs() {
    let (_ds, t) = setup();
    t.set_text("hello, !world");
    assert_eq!(t.glob_list(), vec![glob("hello", true), glob("world", false)]);
    assert_eq!(t.include_tags(), set(&[]));
    assert_eq!(t.exclude_tags(), set(&[]));
    assert_filtering_for(&t, &term_filters(false));
}

#[test]
fn filters_to_patterns() {
    let (_ds, t) = setup();
    t.set_text("@doc");
    assert_eq!(t.glob_list(), vec![]);
    assert_eq!(t.include_tags(), set(&[]));
    assert_eq!(t.exclude_tags(), set(&[]));
    assert_filtering_for(&t, &term_filters(true));
}

#[test]
fn filters_to_tags() {
    let (_ds, t) = setup();
    t.set_text("@hello:world !@foo:bar");
    assert_eq!(t.glob_list(), vec![]);
    assert_eq!(t.include_tags(), set(&["hello\0world"]));
    assert_eq!(t.exclude_tags(), set(&["foo\0bar"]));
    assert_filtering_for(&t, &term_filters(false));
}

#[test]
fn filters_to_mixed_terms_and_tags() {
    let (_ds, t) = setup();
    t.set_text("@hello:world foo, !bar @doc !@foo:bar");
    assert_eq!(t.glob_list(), vec![glob("foo", true), glob("bar", false)]);
    assert_eq!(t.include_tags(), set(&["hello\0world"]));
    assert_eq!(t.exclude_tags(), set(&["foo\0bar"]));
    assert_filtering_for(&t, &term_filters(true));
}

#[test]
fn parses_quotes() {
    let (_ds, t) = setup();
    t.set_text("@hello:\"world\" @foo:'bar' baz");
    assert_eq!(t.glob_list(), vec![glob("baz", true)]);
    assert_eq!(t.include_tags().into_iter().collect::<Vec<_>>(), vec!["hello\0world".to_owned(), "foo\0bar".to_owned()]);
    assert_eq!(t.exclude_tags(), set(&[]));
}

#[test]
fn parses_quotes_with_escapes() {
    let (_ds, t) = setup();
    t.set_text("@hello:\"world\\\"1\" foo");
    assert_eq!(t.glob_list(), vec![glob("foo", true)]);
    assert_eq!(t.include_tags().into_iter().collect::<Vec<_>>(), vec!["hello\0world\"1".to_owned()]);
    assert_eq!(t.exclude_tags(), set(&[]));
}

#[test]
fn treats_unrecognized_at_prefixed_text_as_regular_filter_text() {
    let (_ds, t) = setup();
    t.set_text("@smoke");
    assert_eq!(t.glob_list(), vec![glob("@smoke", true)]);
    assert_eq!(t.include_tags(), set(&[]));
    assert_eq!(t.exclude_tags(), set(&[]));
    assert_filtering_for(&t, &term_filters(false));
}

#[test]
fn treats_unrecognized_at_prefixed_text_as_filter_text_in_mixed_input() {
    let (_ds, t) = setup();
    t.set_text("@smoke @doc hello");
    assert_eq!(t.glob_list(), vec![glob("@smoke hello", true)]);
    assert_eq!(t.include_tags(), set(&[]));
    assert_eq!(t.exclude_tags(), set(&[]));
    assert_filtering_for(&t, &term_filters(true));
}

#[test]
fn negated_unrecognized_at_prefixed_text_works_as_exclusion_filter() {
    let (_ds, t) = setup();
    t.set_text("!@smoke");
    assert_eq!(t.glob_list(), vec![glob("@smoke", false)]);
    assert_eq!(t.include_tags(), set(&[]));
    assert_eq!(t.exclude_tags(), set(&[]));
    assert_filtering_for(&t, &term_filters(false));
}
