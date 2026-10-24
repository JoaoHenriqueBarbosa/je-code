//! Porte de `src/vs/workbench/contrib/markers/test/browser/markersFilterOptions.test.ts` (suíte `MarkersFilterOptions`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - **`setup` e `ensureNoDisposablesAreLeakedInTestSuite()`.** O `setup` (o `FileService` e o `UriIdentityService` que o
//!   `TestInstantiationService` cria) é declarado antes do `ensure...`, então o que ele cria não é rastreado: é o
//!   [`Fixture`], com o serviço nascendo antes do guarda.
//! - **`new FilterOptions(filter, [], true, true, true, uriIdentityService)`.** O `[]` é `FilesExclude::ByRoot(vec![])`.
//! - **`assert.deepStrictEqual(xs, ['ts'])`** é `assert_eq!` sobre `Vec<String>` e `Vec<&str>`; `matchesSourceFilters(undefined)`
//!   é `matches_source_filters(None)`.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use std::rc::Rc;

use common::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite};
use je_instantiation::Injectable;
use je_platform::files::file_service::FileService;
use je_platform::log::NullLogService;
use je_platform::uri_identity::uri_identity_service::UriIdentityService;
use je_workbench::contrib::markers::markers_filter_options::{FilesExclude, FilterOptions};

/// O `setup` (o serviço) e o `ensureNoDisposablesAreLeakedInTestSuite()` (o guarda).
struct Fixture {
    uri_identity_service: Rc<UriIdentityService>,
    _guard: LeakGuard,
}

fn fixture() -> Fixture {
    // setup
    let file_service = FileService::new(NullLogService::shared());
    let uri_identity_service = UriIdentityService::create((), (file_service,));
    // ensureNoDisposablesAreLeakedInTestSuite()
    Fixture { uri_identity_service, _guard: ensure_no_disposables_are_leaked_in_test_suite() }
}

/// `new FilterOptions(filter, [], true, true, true, uriIdentityService)`.
fn filter_options(fixture: &Fixture, filter: &str) -> FilterOptions {
    FilterOptions::new(filter, FilesExclude::ByRoot(Vec::new()), true, true, true, fixture.uri_identity_service.as_ref())
}

fn none() -> Vec<&'static str> {
    Vec::new()
}

#[test]
fn source_filter() {
    let fixture = fixture();
    let filter_options = filter_options(&fixture, "@source:ts");
    assert_eq!(filter_options.include_source_filters, vec!["ts"]);
    assert_eq!(filter_options.exclude_source_filters, none());
    assert_eq!(filter_options.text_filter.text, "");
}

#[test]
fn source_filter_with_negation() {
    let fixture = fixture();
    let filter_options = filter_options(&fixture, "!@source:eslint");
    assert_eq!(filter_options.exclude_source_filters, vec!["eslint"]);
    assert_eq!(filter_options.include_source_filters, none());
    assert_eq!(filter_options.text_filter.text, "");
}

#[test]
fn multiple_source_filters() {
    let fixture = fixture();
    let filter_options = filter_options(&fixture, "@source:eslint @source:ts");
    assert_eq!(filter_options.include_source_filters, vec!["eslint", "ts"]);
    assert_eq!(filter_options.exclude_source_filters, none());
    assert_eq!(filter_options.text_filter.text, "");
}

#[test]
fn source_filter_combined_with_text_filter() {
    let fixture = fixture();
    let filter_options = filter_options(&fixture, "@source:ts error");
    assert_eq!(filter_options.include_source_filters, vec!["ts"]);
    assert_eq!(filter_options.exclude_source_filters, none());
    assert_eq!(filter_options.text_filter.text, "error");
}

#[test]
fn negated_source_filter_combined_with_text_filter() {
    let fixture = fixture();
    let filter_options = filter_options(&fixture, "!@source:ts error");
    assert_eq!(filter_options.exclude_source_filters, vec!["ts"]);
    assert_eq!(filter_options.include_source_filters, none());
    assert_eq!(filter_options.text_filter.text, "error");
}

#[test]
fn no_source_filter_when_not_specified() {
    let fixture = fixture();
    let filter_options = filter_options(&fixture, "some text");
    assert_eq!(filter_options.include_source_filters, none());
    assert_eq!(filter_options.exclude_source_filters, none());
    assert_eq!(filter_options.text_filter.text, "some text");
}

#[test]
fn source_filter_case_insensitive() {
    let fixture = fixture();
    let filter_options = filter_options(&fixture, "@SOURCE:TypeScript");
    assert_eq!(filter_options.include_source_filters, vec!["typescript"]);
    assert_eq!(filter_options.exclude_source_filters, none());
}

#[test]
fn complex_filter_with_multiple_source_filters_and_text() {
    let fixture = fixture();
    let filter_options = filter_options(&fixture, "text1 @source:eslint @source:ts text2");
    assert_eq!(filter_options.include_source_filters, vec!["eslint", "ts"]);
    assert_eq!(filter_options.exclude_source_filters, none());
    assert_eq!(filter_options.text_filter.text, "text1 text2");
}

#[test]
fn source_filter_at_the_beginning() {
    let fixture = fixture();
    let filter_options = filter_options(&fixture, "@source:eslint foo");
    assert_eq!(filter_options.include_source_filters, vec!["eslint"]);
    assert_eq!(filter_options.text_filter.text, "foo");
}

#[test]
fn source_filter_at_the_end() {
    let fixture = fixture();
    let filter_options = filter_options(&fixture, "foo @source:eslint");
    assert_eq!(filter_options.include_source_filters, vec!["eslint"]);
    assert_eq!(filter_options.text_filter.text, "foo");
}

#[test]
fn source_filter_in_the_middle() {
    let fixture = fixture();
    let filter_options = filter_options(&fixture, "foo @source:eslint bar");
    assert_eq!(filter_options.include_source_filters, vec!["eslint"]);
    assert_eq!(filter_options.text_filter.text, "foo bar");
}

#[test]
fn source_filter_with_leading_spaces() {
    let fixture = fixture();
    let filter_options = filter_options(&fixture, "  @source:eslint foo");
    assert_eq!(filter_options.include_source_filters, vec!["eslint"]);
    assert_eq!(filter_options.text_filter.text, "foo");
}

#[test]
fn source_filter_with_trailing_spaces() {
    let fixture = fixture();
    let filter_options = filter_options(&fixture, "foo @source:eslint  ");
    assert_eq!(filter_options.include_source_filters, vec!["eslint"]);
    assert_eq!(filter_options.text_filter.text, "foo");
}

#[test]
fn multiple_consecutive_source_filters() {
    let fixture = fixture();
    let filter_options = filter_options(&fixture, "@source:eslint @source:ts foo");
    assert_eq!(filter_options.include_source_filters, vec!["eslint", "ts"]);
    assert_eq!(filter_options.text_filter.text, "foo");
}

#[test]
fn only_source_filter_with_no_text() {
    let fixture = fixture();
    let filter_options = filter_options(&fixture, "@source:eslint");
    assert_eq!(filter_options.include_source_filters, vec!["eslint"]);
    assert_eq!(filter_options.text_filter.text, "");
}

#[test]
fn multiple_source_filters_with_no_text() {
    let fixture = fixture();
    let filter_options = filter_options(&fixture, "@source:eslint @source:ts");
    assert_eq!(filter_options.include_source_filters, vec!["eslint", "ts"]);
    assert_eq!(filter_options.text_filter.text, "");
}

#[test]
fn negated_source_filter_at_different_positions() {
    let fixture = fixture();
    let filter_options = filter_options(&fixture, "foo !@source:eslint bar");
    assert_eq!(filter_options.exclude_source_filters, vec!["eslint"]);
    assert_eq!(filter_options.include_source_filters, none());
    assert_eq!(filter_options.text_filter.text, "foo bar");
}

#[test]
fn mixed_negated_and_positive_source_filters() {
    let fixture = fixture();
    let filter_options = filter_options(&fixture, "@source:eslint !@source:ts foo");
    assert_eq!(filter_options.include_source_filters, vec!["eslint"]);
    assert_eq!(filter_options.exclude_source_filters, vec!["ts"]);
    assert_eq!(filter_options.text_filter.text, "foo");
}

#[test]
fn single_quoted_source_with_spaces() {
    let fixture = fixture();
    let filter_options = filter_options(&fixture, "@source:\"hello world\"");
    assert_eq!(filter_options.include_source_filters, vec!["hello world"]);
    assert_eq!(filter_options.text_filter.text, "");
}

#[test]
fn quoted_source_combined_with_text_filter() {
    let fixture = fixture();
    let filter_options = filter_options(&fixture, "@source:\"hello world\" foo");
    assert_eq!(filter_options.include_source_filters, vec!["hello world"]);
    assert_eq!(filter_options.text_filter.text, "foo");
}

#[test]
fn mixed_quoted_and_unquoted_sources_or_logic() {
    let fixture = fixture();
    let filter_options = filter_options(&fixture, "@source:\"hello world\" @source:eslint @source:ts");
    assert_eq!(filter_options.include_source_filters, vec!["hello world", "eslint", "ts"]);
}

#[test]
fn multiple_quoted_sources_or_logic() {
    let fixture = fixture();
    let filter_options = filter_options(&fixture, "@source:\"hello world\" @source:\"foo bar\"");
    assert_eq!(filter_options.include_source_filters, vec!["hello world", "foo bar"]);
}

#[test]
fn quoted_source_with_negation() {
    let fixture = fixture();
    let filter_options = filter_options(&fixture, "!@source:\"hello world\"");
    assert_eq!(filter_options.exclude_source_filters, vec!["hello world"]);
}

#[test]
fn quoted_source_in_the_middle_of_filter() {
    let fixture = fixture();
    let filter_options = filter_options(&fixture, "foo @source:\"hello world\" bar");
    assert_eq!(filter_options.include_source_filters, vec!["hello world"]);
    assert_eq!(filter_options.text_filter.text, "foo bar");
}

#[test]
fn complex_filter_with_quoted_and_unquoted_mixed() {
    let fixture = fixture();
    let filter_options = filter_options(&fixture, "@source:\"TypeScript Compiler\" @source:eslint !@source:\"My Extension\" text");
    assert_eq!(filter_options.include_source_filters, vec!["typescript compiler", "eslint"]);
    assert_eq!(filter_options.exclude_source_filters, vec!["my extension"]);
    assert_eq!(filter_options.text_filter.text, "text");
}

#[test]
fn no_filters_always_matches() {
    let fixture = fixture();
    let filter_options = filter_options(&fixture, "foo");
    assert!(filter_options.matches_source_filters(Some("eslint")));
    assert!(filter_options.matches_source_filters(None));
}

#[test]
fn positive_filter_exact_match_only() {
    let fixture = fixture();
    let filter_options = filter_options(&fixture, "@source:eslint");
    assert!(filter_options.matches_source_filters(Some("eslint")));
    assert!(filter_options.matches_source_filters(Some("ESLint")));
    assert!(!filter_options.matches_source_filters(Some("ts")));
    assert!(!filter_options.matches_source_filters(Some("eslint-plugin")));
    assert!(!filter_options.matches_source_filters(Some("es")));
}

#[test]
fn positive_filter_no_source_in_marker() {
    let fixture = fixture();
    let filter_options = filter_options(&fixture, "@source:eslint");
    assert!(!filter_options.matches_source_filters(None));
}

#[test]
fn negative_filter_excludes_exact_source() {
    let fixture = fixture();
    let filter_options = filter_options(&fixture, "!@source:eslint");
    assert!(!filter_options.matches_source_filters(Some("eslint")));
    assert!(filter_options.matches_source_filters(Some("ts")));
    assert!(filter_options.matches_source_filters(Some("eslint-plugin")));
}

#[test]
fn negative_filter_no_source_in_marker() {
    let fixture = fixture();
    let filter_options = filter_options(&fixture, "!@source:eslint");
    assert!(filter_options.matches_source_filters(None));
}

#[test]
fn or_logic_multiple_source_filters() {
    let fixture = fixture();
    let filter_options = filter_options(&fixture, "@source:eslint @source:ts");
    assert!(filter_options.matches_source_filters(Some("eslint")));
    assert!(filter_options.matches_source_filters(Some("ts")));
    assert!(!filter_options.matches_source_filters(Some("python")));
}

#[test]
fn or_logic_with_negation() {
    let fixture = fixture();
    let filter_options = filter_options(&fixture, "@source:eslint @source:ts !@source:error");
    assert!(filter_options.matches_source_filters(Some("eslint")));
    assert!(filter_options.matches_source_filters(Some("ts")));
    assert!(!filter_options.matches_source_filters(Some("error")));
    assert!(!filter_options.matches_source_filters(Some("python")));
}

#[test]
fn only_negative_filters_excludes_specified_sources() {
    let fixture = fixture();
    let filter_options = filter_options(&fixture, "!@source:eslint !@source:ts");
    assert!(!filter_options.matches_source_filters(Some("eslint")));
    assert!(!filter_options.matches_source_filters(Some("ts")));
    assert!(filter_options.matches_source_filters(Some("python")));
    assert!(filter_options.matches_source_filters(None));
}

#[test]
fn case_insensitivity() {
    let fixture = fixture();
    let filter_options = filter_options(&fixture, "@source:ESLint");
    assert!(filter_options.matches_source_filters(Some("eslint")));
    assert!(filter_options.matches_source_filters(Some("ESLINT")));
    assert!(filter_options.matches_source_filters(Some("EsLiNt")));
}
