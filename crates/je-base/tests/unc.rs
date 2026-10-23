//! Porte de `src/vs/base/test/node/unc.test.ts` (suíte `UNC`), caso a caso e na mesma ordem.

mod common;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::node::unc::get_unc_host;

fn host(path: &str) -> Option<String> {
    get_unc_host(Some(path))
}

#[test]
fn get_unc_host_test() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();

    assert_eq!(get_unc_host(None), None);

    assert_eq!(host("/"), None);
    assert_eq!(host("/foo"), None);

    assert_eq!(host("c:"), None);
    assert_eq!(host("c:\\"), None);
    assert_eq!(host("c:\\foo"), None);
    assert_eq!(host("c:\\foo\\\\server\\path"), None);

    assert_eq!(host("\\"), None);
    assert_eq!(host("\\\\"), None);
    assert_eq!(host("\\\\localhost"), None);

    assert_eq!(host("\\\\localhost\\").as_deref(), Some("localhost"));
    assert_eq!(host("\\\\localhost\\a").as_deref(), Some("localhost"));

    assert_eq!(host("\\\\."), None);
    assert_eq!(host("\\\\?"), None);

    assert_eq!(host("\\\\.\\localhost").as_deref(), Some("."));
    assert_eq!(host("\\\\?\\localhost").as_deref(), Some("?"));

    assert_eq!(host("\\\\.\\UNC\\localhost").as_deref(), Some("."));
    assert_eq!(host("\\\\?\\UNC\\localhost").as_deref(), Some("?"));

    assert_eq!(host("\\\\.\\UNC\\localhost\\").as_deref(), Some("localhost"));
    assert_eq!(host("\\\\?\\UNC\\localhost\\").as_deref(), Some("localhost"));

    assert_eq!(host("\\\\.\\UNC\\localhost\\a").as_deref(), Some("localhost"));
    assert_eq!(host("\\\\?\\UNC\\localhost\\a").as_deref(), Some("localhost"));
}
