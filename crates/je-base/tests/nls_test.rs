//! Porte de `src/vs/base/test/common/nls.test.ts` (suíte `NLS`), caso a caso e na mesma ordem.
//!
//! Divergência: o argumento `{} as unknown as string` do terceiro caso é [`NlsArg::Unreplaceable`], o
//! valor que o `_format` do original não substitui.

mod common;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::nls::{NlsArg, localize, localize2, localize_args};

fn strings(values: &[&str]) -> Vec<NlsArg> {
    values.iter().map(|value| NlsArg::Str((*value).to_owned())).collect()
}

#[test]
fn localize_replaces_placeholders_by_index() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    assert_eq!(localize("key0", "{0} {1} {2}", &["a", "b", "c"]), "a b c");
}

#[test]
fn localize_replaces_multi_digit_placeholders() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    let args = ["a0", "a1", "a2", "a3", "a4", "a5", "a6", "a7", "a8", "a9", "a10", "a11"];
    assert_eq!(localize("key1", "{0} {1} {9} {10} {11}", &args), "a0 a1 a9 a10 a11");
}

#[test]
fn localize_keeps_placeholders_that_are_not_replaceable() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    let args = [NlsArg::Str("a".to_owned()), NlsArg::Unreplaceable];
    assert_eq!(localize_args("key2", "{0} {1}", &args), "a {1}");
}

#[test]
fn localize2_replaces_multi_digit_placeholders() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    let args = strings(&["a0", "a1", "a2", "a3", "a4", "a5", "a6", "a7", "a8", "a9", "a10"]);
    assert_eq!(localize2("key3", "{10}", &args).value, "a10");
}
