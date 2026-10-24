//! Porte de `src/vs/workbench/contrib/extensions/test/common/extensionQuery.test.ts` (suíte `Extension query`),
//! caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` vira o guarda criado no começo de cada caso.
//! - `query.toString()` é `Query::to_js_string`; `query1.equals(query2)` é `query1 == query2` (o `equals` do
//!   original compara `value` e `sortBy`, que é o `PartialEq` derivado).
//! - O caso `autocomplete` do original chama `Array.prototype.some` e `every` sem asserção sobre o resultado;
//!   o porte chama `suggestions` e descarta o resultado do mesmo jeito.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_workbench::contrib::extensions::extension_query::Query;

#[test]
fn parse() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let mut query = Query::parse("");
    assert_eq!(query.value, "");
    assert_eq!(query.sort_by, "");

    query = Query::parse("hello");
    assert_eq!(query.value, "hello");
    assert_eq!(query.sort_by, "");

    query = Query::parse("   hello world ");
    assert_eq!(query.value, "hello world");
    assert_eq!(query.sort_by, "");

    query = Query::parse("@sort");
    assert_eq!(query.value, "@sort");
    assert_eq!(query.sort_by, "");

    query = Query::parse("@sort:");
    assert_eq!(query.value, "@sort:");
    assert_eq!(query.sort_by, "");

    query = Query::parse("  @sort:  ");
    assert_eq!(query.value, "@sort:");
    assert_eq!(query.sort_by, "");

    query = Query::parse("@sort:installs");
    assert_eq!(query.value, "");
    assert_eq!(query.sort_by, "installs");

    query = Query::parse("   @sort:installs   ");
    assert_eq!(query.value, "");
    assert_eq!(query.sort_by, "installs");

    query = Query::parse("@sort:installs-");
    assert_eq!(query.value, "");
    assert_eq!(query.sort_by, "installs");

    query = Query::parse("@sort:installs-foo");
    assert_eq!(query.value, "");
    assert_eq!(query.sort_by, "installs");

    query = Query::parse("@sort:installs");
    assert_eq!(query.value, "");
    assert_eq!(query.sort_by, "installs");

    query = Query::parse("@sort:installs");
    assert_eq!(query.value, "");
    assert_eq!(query.sort_by, "installs");

    query = Query::parse("vs @sort:installs");
    assert_eq!(query.value, "vs");
    assert_eq!(query.sort_by, "installs");

    query = Query::parse("vs @sort:installs code");
    assert_eq!(query.value, "vs  code");
    assert_eq!(query.sort_by, "installs");

    query = Query::parse("@sort:installs @sort:ratings");
    assert_eq!(query.value, "");
    assert_eq!(query.sort_by, "ratings");
}

#[test]
fn to_string() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let mut query = Query::new("hello", "");
    assert_eq!(query.to_js_string(), "hello");

    query = Query::new("hello world", "");
    assert_eq!(query.to_js_string(), "hello world");

    query = Query::new("  hello    ", "");
    assert_eq!(query.to_js_string(), "hello");

    query = Query::new("", "installs");
    assert_eq!(query.to_js_string(), "@sort:installs");

    query = Query::new("", "installs");
    assert_eq!(query.to_js_string(), "@sort:installs");

    query = Query::new("", "installs");
    assert_eq!(query.to_js_string(), "@sort:installs");

    query = Query::new("hello", "installs");
    assert_eq!(query.to_js_string(), "hello @sort:installs");

    query = Query::new("  hello      ", "installs");
    assert_eq!(query.to_js_string(), "hello @sort:installs");
}

#[test]
fn is_valid() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let mut query = Query::new("hello", "");
    assert!(query.is_valid());

    query = Query::new("hello world", "");
    assert!(query.is_valid());

    query = Query::new("  hello    ", "");
    assert!(query.is_valid());

    query = Query::new("", "installs");
    assert!(query.is_valid());

    query = Query::new("", "installs");
    assert!(query.is_valid());

    query = Query::new("", "installs");
    assert!(query.is_valid());

    query = Query::new("", "installs");
    assert!(query.is_valid());

    query = Query::new("hello", "installs");
    assert!(query.is_valid());

    query = Query::new("  hello      ", "installs");
    assert!(query.is_valid());
}

#[test]
fn equals() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let query1 = Query::new("hello", "");
    let mut query2 = Query::new("hello", "");
    assert!(query1 == query2);

    query2 = Query::new("hello world", "");
    assert!(query1 != query2);

    query2 = Query::new("hello", "installs");
    assert!(query1 != query2);

    query2 = Query::new("hello", "installs");
    assert!(query1 != query2);
}

#[test]
fn autocomplete() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let _ = Query::suggestions("@sort:in", None).iter().any(|x| x == "@sort:installs ");
    let _ = Query::suggestions("@sort:installs", None).iter().all(|x| x != "@sort:rating ");

    let _ = Query::suggestions("@category:blah", None).iter().any(|x| x == "@category:\"extension packs\" ");
    let _ = Query::suggestions("@category:\"extension packs\"", None).iter().all(|x| x != "@category:formatters ");
}
