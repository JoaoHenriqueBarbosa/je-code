//! Porte de `src/vs/workbench/contrib/multiDiffEditor/test/browser/multiDiffEditorLayoutDebug.test.ts` (suíte
//! `MultiDiffEditorLayoutDebug`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` vira o guarda criado no começo de cada caso.
//! - Os objetos literais do original são lidos de texto JSON (`json`). Uma propriedade `undefined` (caso `treats
//!   undefined object properties as absent JSON properties`) é uma chave ausente: o algoritmo as trata do mesmo
//!   jeito (ver as divergências de `multi_diff_editor_layout_debug`), e o `JSON.parse(JSON.stringify(next))` do
//!   original também a descarta.
//! - `JSON.stringify(x, undefined, '\t')` é `json_stringify(x, Some("\t"))`; `JSON.stringify(x)` é `json_stringify(x, None)`.
//! - O `deepStrictEqual` do Node não olha a ordem das chaves de um objeto; `same_json` faz a mesma comparação
//!   estrutural (o `PartialEq` de `Value` é sensível à ordem).

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::JsString;
use je_base::json::{Value, parse_str};
use je_base::json_formatter::json_stringify;
use je_base::jsonc::json_parse;
use je_workbench::contrib::multi_diff_editor::multi_diff_editor_layout_debug::{
    JsonPatch, JsonPatchEntry, JsonPatchOperation, apply_json_patch_to_text, create_json_patch,
};

fn json(text: &str) -> Value {
    parse_str(text, &mut Vec::new()).expect("JSON de teste válido")
}

fn text(value: &str) -> Value {
    Value::String(JsString::from(value))
}

fn parse_applied(applied: &str) -> Value {
    let units: Vec<u16> = applied.encode_utf16().collect();
    json_parse(&units).expect("o texto aplicado é JSON válido")
}

fn same_json(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Array(a), Value::Array(b)) => a.len() == b.len() && a.iter().zip(b).all(|(a, b)| same_json(a, b)),
        (Value::Object(a), Value::Object(b)) => {
            a.len() == b.len() && a.iter().all(|(key, a)| b.get_js(key).is_some_and(|b| same_json(a, b)))
        }
        _ => a == b,
    }
}

fn operations(entries: Vec<JsonPatchOperation>) -> JsonPatch {
    entries.into_iter().map(JsonPatchEntry::Operation).collect()
}

#[test]
fn creates_and_applies_structural_json_patches() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let previous = json(
        r#"{
            "scrollTop": 10,
            "items": [
                { "label": "a", "height": 20, "visible": true },
                { "label": "b", "height": 30, "visible": false }
            ],
            "obsolete": true
        }"#,
    );
    let next = json(
        r#"{
            "scrollTop": 15,
            "items": [
                { "label": "a", "height": 25, "visible": true },
                { "label": "b", "height": 30, "visible": true }
            ],
            "added": "value"
        }"#,
    );

    let patch = create_json_patch(&previous, &next, &[]);
    let result = parse_applied(&apply_json_patch_to_text(&json_stringify(&previous, Some("\t")), &patch).unwrap());
    assert_eq!(
        patch,
        operations(vec![
            JsonPatchOperation::Remove { path: "/obsolete".to_string() },
            JsonPatchOperation::Replace { path: "/scrollTop".to_string(), value: Value::Number(15.0) },
            JsonPatchOperation::Replace { path: "/items/0/height".to_string(), value: Value::Number(25.0) },
            JsonPatchOperation::Replace { path: "/items/1/visible".to_string(), value: Value::Bool(true) },
            JsonPatchOperation::Add { path: "/added".to_string(), value: text("value") },
        ])
    );
    assert!(same_json(&result, &next), "{result:?} difere de {next:?}");
}

#[test]
fn escapes_json_pointer_segments() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(
        create_json_patch(&json(r#"{ "a/b": { "~key": 1 } }"#), &json(r#"{ "a/b": { "~key": 2 } }"#), &[]),
        operations(vec![JsonPatchOperation::Replace { path: "/a~1b/~0key".to_string(), value: Value::Number(2.0) }])
    );
}

#[test]
fn treats_undefined_object_properties_as_absent_json_properties() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let previous = json(r#"{ "removed": { "value": 1 }, "nested": { "removed": { "value": 2 } } }"#);
    let next = json(r#"{ "added": { "value": 3 }, "nested": { "added": { "value": 4 } } }"#);

    let patch = create_json_patch(&previous, &next, &[]);
    let result = parse_applied(&apply_json_patch_to_text(&json_stringify(&previous, None), &patch).unwrap());
    assert_eq!(
        patch,
        operations(vec![
            JsonPatchOperation::Remove { path: "/removed".to_string() },
            JsonPatchOperation::Add { path: "/added".to_string(), value: json(r#"{ "value": 3 }"#) },
            JsonPatchOperation::Remove { path: "/nested/removed".to_string() },
            JsonPatchOperation::Add { path: "/nested/added".to_string(), value: json(r#"{ "value": 4 }"#) },
        ])
    );
    assert!(same_json(&result, &next), "{result:?} difere de {next:?}");
}

#[test]
fn composes_nested_patches() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let initial = json(r#"{ "value": 1, "nested": { "enabled": false } }"#);
    let updated_value = json(r#"{ "value": 2, "nested": { "enabled": false } }"#);
    let last = json(r#"{ "value": 2, "nested": { "enabled": true } }"#);
    let patch: JsonPatch = vec![
        JsonPatchEntry::Nested(create_json_patch(&initial, &updated_value, &[])),
        JsonPatchEntry::Nested(vec![JsonPatchEntry::Nested(create_json_patch(&updated_value, &last, &[]))]),
    ];

    let result = parse_applied(&apply_json_patch_to_text(&json_stringify(&initial, None), &patch).unwrap());
    assert!(same_json(&result, &last), "{result:?} difere de {last:?}");
}
