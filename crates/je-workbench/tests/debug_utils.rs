//! Porte de `src/vs/workbench/contrib/debug/test/browser/debugUtils.test.ts` (suíte `Debug - Utils`), caso a caso e na mesma ordem.
//!
//! Os 4 casos: `formatPII`, `getExactExpressionStartAndEnd`, `getEffectivePresentationForConfig - platform override` e
//! `config presentation`.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` vira o `LeakGuard` de cada caso.
//! - `IConfig` é o objeto JSON; `platformSection(os, value)` é o objeto com a chave `windows`, `osx` ou `linux`, e o
//!   `...platformSection(...)` do espalhamento é a união dos objetos.
//! - `assert.deepStrictEqual(a, b)` entre objetos é `assert_eq!` entre `serde_json::Value` (a comparação dos objetos não
//!   depende da ordem das chaves, como no `deepStrictEqual`).
//! - `getVisibleAndSorted` trabalha sobre as configurações JSON e devolve cópias; o nome é lido de `name`.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use std::collections::HashMap;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::platform::OperatingSystem;
use je_workbench::contrib::debug::debug_utils::{
    format_pii, get_effective_config_for_platform, get_effective_presentation_for_config, get_exact_expression_start_and_end, get_visible_and_sorted,
};
use serde_json::{Map, Value, json};

fn platform_section(os: OperatingSystem, value: Value) -> Map<String, Value> {
    let key = match os {
        OperatingSystem::Windows => "windows",
        OperatingSystem::Macintosh => "osx",
        OperatingSystem::Linux => "linux",
    };
    let mut section = Map::new();
    section.insert(key.to_owned(), value);
    section
}

/// `{ ...base, ...platformSection(os, value) }`.
fn config_with_platform(base: Value, os: OperatingSystem, value: Value) -> Value {
    let mut object = base.as_object().cloned().unwrap();
    object.extend(platform_section(os, value));
    Value::Object(object)
}

fn args(pairs: &[(&str, &str)]) -> HashMap<String, String> {
    pairs.iter().map(|(key, value)| ((*key).to_owned(), (*value).to_owned())).collect()
}

#[test]
fn format_pii_case() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(format_pii("Foo Bar", false, Some(&args(&[]))), "Foo Bar");
    assert_eq!(format_pii("Foo {key} Bar", false, Some(&args(&[]))), "Foo {key} Bar");
    assert_eq!(format_pii("Foo {key} Bar", false, Some(&args(&[("key", "yes")]))), "Foo yes Bar");
    assert_eq!(format_pii("Foo {_0} Bar {_0}", true, Some(&args(&[("_0", "yes")]))), "Foo yes Bar yes");
    assert_eq!(format_pii("Foo {0} Bar {1}{2}", false, Some(&args(&[("0", "yes")]))), "Foo yes Bar {1}{2}");
    assert_eq!(format_pii("Foo {0} Bar {1}{2}", false, Some(&args(&[("0", "yes"), ("1", "undefined")]))), "Foo yes Bar undefined{2}");
    assert_eq!(
        format_pii("Foo {_key0} Bar {key1}{key2}", true, Some(&args(&[("_key0", "yes"), ("key1", "5"), ("key2", "false")]))),
        "Foo yes Bar {key1}{key2}"
    );
    assert_eq!(
        format_pii("Foo {_key0} Bar {key1}{key2}", false, Some(&args(&[("_key0", "yes"), ("key1", "5"), ("key2", "false")]))),
        "Foo yes Bar 5false"
    );
    assert_eq!(
        format_pii("Unable to display threads:\"{e}\"", false, Some(&args(&[("e", "detached from process")]))),
        "Unable to display threads:\"detached from process\""
    );
}

#[test]
fn get_exact_expression_start_and_end_case() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(get_exact_expression_start_and_end("foo", 1, 2), (1, 3));
    assert_eq!(get_exact_expression_start_and_end("!foo", 2, 3), (2, 4));
    assert_eq!(get_exact_expression_start_and_end("foo", 1, 3), (1, 3));
    assert_eq!(get_exact_expression_start_and_end("foo", 1, 4), (1, 3));
    assert_eq!(get_exact_expression_start_and_end("this.name = \"John\"", 1, 10), (1, 9));
    assert_eq!(get_exact_expression_start_and_end("this.name = \"John\"", 6, 10), (1, 9));
    // Hovers over "address" should pick up this->address
    assert_eq!(get_exact_expression_start_and_end("this->address = \"Main street\"", 6, 10), (1, 13));
    // Hovers over "name" should pick up a.b.c.d.name
    assert_eq!(get_exact_expression_start_and_end("var t = a.b.c.d.name", 16, 20), (9, 20));
    assert_eq!(get_exact_expression_start_and_end("MyClass::StaticProp", 10, 20), (1, 19));
    assert_eq!(get_exact_expression_start_and_end("largeNumber = myVar?.prop", 21, 25), (15, 25));

    // For example in expression 'a.b.c.d', hover was under 'b', 'a.b' should be the exact range
    assert_eq!(get_exact_expression_start_and_end("var t = a.b.c.d.name", 11, 12), (9, 11));

    assert_eq!(get_exact_expression_start_and_end("var t = a.b;c.d.name", 16, 20), (13, 20));
    assert_eq!(get_exact_expression_start_and_end("var t = a.b.c-d.name", 16, 20), (15, 20));

    assert_eq!(get_exact_expression_start_and_end("var aøñéå文 = a.b.c-d.name", 5, 5), (5, 10));
    assert_eq!(get_exact_expression_start_and_end("aøñéå文.aøñéå文.aøñéå文", 9, 9), (1, 13));

    // Spread syntax should extract just the identifier
    assert_eq!(get_exact_expression_start_and_end("[...bar]", 5, 7), (5, 7));
    assert_eq!(get_exact_expression_start_and_end("...variable", 5, 5), (4, 11));
}

#[test]
fn get_effective_presentation_for_config_platform_override() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    // No platform override: returns base presentation
    let config1 = json!({ "type": "node", "request": "launch", "name": "a", "presentation": { "hidden": false } });
    assert_eq!(get_effective_presentation_for_config(&config1, Some(OperatingSystem::Macintosh)), Some(json!({ "hidden": false })));

    // Platform-specific presentation overrides base hidden value
    let config2 = config_with_platform(
        json!({ "type": "node", "request": "launch", "name": "b", "presentation": { "hidden": false } }),
        OperatingSystem::Windows,
        json!({ "presentation": { "hidden": true } }),
    );
    assert_eq!(get_effective_presentation_for_config(&config2, Some(OperatingSystem::Windows)), Some(json!({ "hidden": true })));

    // Non-matching platform override does not affect result
    let config3 = config_with_platform(
        json!({ "type": "node", "request": "launch", "name": "c", "presentation": { "hidden": false } }),
        OperatingSystem::Windows,
        json!({ "presentation": { "hidden": true } }),
    );
    assert_eq!(get_effective_presentation_for_config(&config3, Some(OperatingSystem::Linux)), Some(json!({ "hidden": false })));

    // No base presentation, platform-specific sets hidden
    let config4 = config_with_platform(
        json!({ "type": "node", "request": "launch", "name": "d" }),
        OperatingSystem::Macintosh,
        json!({ "presentation": { "hidden": true } }),
    );
    assert_eq!(get_effective_presentation_for_config(&config4, Some(OperatingSystem::Macintosh)), Some(json!({ "hidden": true })));

    // Platform-specific merges with base (group and order preserved)
    let config5 = config_with_platform(
        json!({ "type": "node", "request": "launch", "name": "e", "presentation": { "group": "myGroup", "order": 2 } }),
        OperatingSystem::Linux,
        json!({ "presentation": { "hidden": true } }),
    );
    assert_eq!(get_effective_presentation_for_config(&config5, Some(OperatingSystem::Linux)), Some(json!({ "group": "myGroup", "order": 2, "hidden": true })));

    // Platform-specific config overrides other launch attributes while preserving nested sections
    let config6 = config_with_platform(
        json!({ "type": "node", "request": "launch", "name": "f", "preLaunchTask": "base-task", "presentation": { "group": "base" } }),
        OperatingSystem::Windows,
        json!({ "preLaunchTask": "windows-task", "presentation": { "hidden": true } }),
    );
    let mut expected = config6.as_object().cloned().unwrap();
    expected.insert("preLaunchTask".to_owned(), json!("windows-task"));
    expected.insert("presentation".to_owned(), json!({ "group": "base", "hidden": true }));
    assert_eq!(get_effective_config_for_platform(&config6, Some(OperatingSystem::Windows)), Value::Object(expected));
}

#[test]
fn config_presentation() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let config = |name: &str, presentation: Option<Value>| -> Value {
        let mut object = json!({ "type": "node", "request": "launch", "name": name }).as_object().cloned().unwrap();
        if let Some(presentation) = presentation {
            object.insert("presentation".to_owned(), presentation);
        }
        Value::Object(object)
    };
    let configs = vec![
        config("p", None),
        config("a", None),
        config("b", Some(json!({ "hidden": false }))),
        config("c", Some(json!({ "hidden": true }))),
        config("d", Some(json!({ "group": "2_group", "order": 5 }))),
        config("e", Some(json!({ "group": "2_group", "order": 52 }))),
        config("f", Some(json!({ "group": "1_group", "order": 500 }))),
        config("g", Some(json!({ "group": "5_group", "order": 500 }))),
        config("h", Some(json!({ "order": 700 }))),
        config("i", Some(json!({ "order": 1000 }))),
    ];

    let sorted = get_visible_and_sorted(&configs);
    assert_eq!(sorted.len(), 9);
    let names: Vec<&str> = sorted.iter().map(|c| c["name"].as_str().unwrap()).collect();
    assert_eq!(names, vec!["f", "d", "e", "g", "h", "i", "b", "p", "a"]);
}
