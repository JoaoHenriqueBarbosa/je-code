//! Porte de `src/vs/workbench/contrib/policyExport/test/common/policyReference.test.ts` (suíte
//! `Policy export reference types`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` vira o guarda criado no começo de cada caso.
//! - O `assert.deepStrictEqual({ boolean: ..., ... }, { boolean: true, ... })` do original compara dois objetos
//!   de mesmas chaves; o porte compara duas listas de pares `(chave, valor)` na mesma ordem de chaves.
//! - `'boolean'` e `['boolean', 'null']` viram `PolicyTypes` por `.into()`; `undefined` é `None`.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_workbench::contrib::policy_export::policy_dto::PolicyTypes;
use je_workbench::contrib::policy_export::policy_reference::are_policy_reference_types_compatible;

fn compatible(policy_type: impl Into<PolicyTypes>, reference_type: impl Into<PolicyTypes>) -> bool {
    are_policy_reference_types_compatible(Some(&policy_type.into()), &reference_type.into())
}

#[test]
fn matches_equivalent_policy_types_including_nullable_settings() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(
        [
            ("boolean", compatible("boolean", "boolean")),
            ("nullableReference", compatible("boolean", ["boolean", "null"])),
            ("nullableOwner", compatible(["boolean", "null"], "boolean")),
            ("nullableArray", compatible("array", ["null", "array"])),
            ("unionOrder", compatible(["string", "null", "boolean"], ["boolean", "string", "null"])),
        ],
        [
            ("boolean", true),
            ("nullableReference", true),
            ("nullableOwner", true),
            ("nullableArray", true),
            ("unionOrder", true),
        ]
    );
}

#[test]
fn rejects_mismatched_non_null_policy_types() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(
        [
            ("differentType", compatible("boolean", "string")),
            ("differentNullableType", compatible("boolean", ["string", "null"])),
            ("additionalType", compatible("boolean", ["boolean", "string", "null"])),
            ("nullOnly", compatible("boolean", "null")),
            ("missingOwnerType", are_policy_reference_types_compatible(None, &["boolean", "null"].into())),
        ],
        [
            ("differentType", false),
            ("differentNullableType", false),
            ("additionalType", false),
            ("nullOnly", false),
            ("missingOwnerType", false),
        ]
    );
}
