//! Porte de `src/vs/workbench/contrib/testing/test/common/testingUri.test.ts` (suíte
//! `Workbench - Testing URIs`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` vira o guarda criado no começo de cada caso.
//! - `taskIndex` e `messageIndex` são `number` no original, então são `f64` (`1.0` e `42.0`).
//! - `assert.deepStrictEqual(uri, parseTestUri(serialized))` compara o `ParsedTestUri` com o `Option` devolvido
//!   pelo `parseTestUri` (`ParsedTestUri | undefined`).

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_workbench::contrib::testing::testing_uri::{ParsedTestUri, build_test_uri, parse_test_uri};

#[test]
fn round_trip() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let uris: Vec<ParsedTestUri> = vec![
        ParsedTestUri::ResultActualOutput { task_index: 1.0, message_index: 42.0, result_id: "r".into(), test_ext_id: "t".into() },
        ParsedTestUri::ResultExpectedOutput { task_index: 1.0, message_index: 42.0, result_id: "r".into(), test_ext_id: "t".into() },
        ParsedTestUri::ResultMessage { task_index: 1.0, message_index: 42.0, result_id: "r".into(), test_ext_id: "t".into() },
    ];

    for uri in uris {
        let serialized = build_test_uri(&uri);
        assert_eq!(Some(uri), parse_test_uri(&serialized));
    }
}
