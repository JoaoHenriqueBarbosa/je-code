//! Casos que NÃO são do original: o `configuration.ts`, o `configurationRegistry.ts` e o
//! `configurationModels.ts` têm funções que os `.test.ts` não exercitam (mensagens de conflito de
//! `toValuesTree`, `getLanguageTagSettingPlainKey`, `overrideIdentifiersFromKey`, `getDefaultValue`...).
//! Os valores esperados foram gerados rodando o mesmo código do original no Node 26 (`node k.mjs`,
//! `node v.mjs`, cópias literais das funções do TypeScript).

mod common;

use std::cell::RefCell;

use je_base::dynamic::{Dynamic, ObjectRef};
use je_platform::configuration::registry::{
    ConfigurationScope, get_default_value, is_override_property, key_from_override_identifiers, override_identifiers_from_key, parse_scope,
};
use je_platform::configuration::{
    ConfigurationTarget, add_to_value_tree, configuration_target_to_string, get_configuration_value, get_language_tag_setting_plain_key, to_values_tree,
};

use common::*;

#[test]
fn get_language_tag_setting_plain_key_matches_node() {
    let plain = |key: &str| get_language_tag_setting_plain_key(key).to_string_lossy();
    assert_eq!(plain("[a]"), "a");
    assert_eq!(plain("[a][b]"), "a, b");
    assert_eq!(plain("[[a]]"), "[a]");
    assert_eq!(plain("[a]]"), "a]");
    assert_eq!(plain("a"), "a");
    assert_eq!(plain("[]"), "");
    assert_eq!(plain("]["), ", ");
}

#[test]
fn override_identifiers_and_regex_match_node() {
    let ids = |key: &str| override_identifiers_from_key(&jstr(key));
    let cases: [(&str, bool, &[&str]); 9] = [
        ("[a]", true, &["a"]),
        ("[ a ][b][a]", true, &["a", "b"]),
        ("[[a]]", false, &[]),
        ("[a]x", false, &[]),
        ("[]", false, &[]),
        ("[ ]", true, &[]),
        ("[a][ ]", true, &["a"]),
        ("[a\n]", true, &["a"]),
        ("", false, &[]),
    ];
    for (key, is_override, expected) in cases {
        assert_eq!(is_override_property(&jstr(key)), is_override, "regex em {key:?}");
        assert_eq!(ids(key), strs(expected), "identificadores de {key:?}");
    }
    assert_eq!(key_from_override_identifiers(&strs(&["a", "b"])), jstr("[a][b]"));
}

#[test]
fn to_values_tree_reports_conflicts_like_node() {
    let messages: RefCell<Vec<String>> = RefCell::new(Vec::new());
    let report = |message: &str| messages.borrow_mut().push(message.to_string());
    let tree = empty_obj();
    add_to_value_tree(&tree, &jstr("a"), n(1.0), &report);
    add_to_value_tree(&tree, &jstr("a.b"), n(2.0), &report);
    add_to_value_tree(&tree, &jstr("n"), Dynamic::Null, &report);
    add_to_value_tree(&tree, &jstr("n.x.y"), n(2.0), &report);
    add_to_value_tree(&tree, &jstr("s"), s("str"), &report);
    add_to_value_tree(&tree, &jstr("s.x"), n(1.0), &report);
    add_to_value_tree(&tree, &jstr("o.p.q"), j("{ 'z': 1 }"), &report);
    add_to_value_tree(&tree, &jstr("o.p"), n(3.0), &report);
    add_to_value_tree(&tree, &jstr("f"), Dynamic::Bool(true), &report);
    add_to_value_tree(&tree, &jstr("f.g.h"), n(1.0), &report);
    let frozen = ObjectRef::new();
    frozen.freeze();
    let root = obj(vec![("k", Dynamic::Object(frozen))]);
    add_to_value_tree(&root, &jstr("k.w"), n(1.0), &report);

    assert_eq!(
        *messages.borrow(),
        vec![
            "Ignoring a.b as a is 1",
            "Ignoring n.x.y as n is null",
            "Ignoring s.x as s is \"str\"",
            "Ignoring f.g.h as f is true",
            "Ignoring k.w as k is {}",
        ]
    );
    assert_eq!(stringify(&tree), "{\"a\":1,\"n\":null,\"s\":\"str\",\"o\":{\"p\":3},\"f\":true}");
}

#[test]
fn to_values_tree_and_get_configuration_value_walk_dotted_keys() {
    let tree = to_values_tree(&j("{ 'a.b.c': 1, 'a.b.d': 2, 'e': { 'f': 3 } }"), &|_| {});
    assert_deep_strict_equal(&tree, &j("{ 'a': { 'b': { 'c': 1, 'd': 2 } }, 'e': { 'f': 3 } }"));
    assert_eq!(get_configuration_value(&tree, "a.b.d", None), n(2.0));
    assert_eq!(get_configuration_value(&tree, "a.x", Some(n(9.0))), n(9.0));
    assert!(get_configuration_value(&tree, "a.b.c.d", None).is_undefined());
    assert!(get_configuration_value(&tree, "e.f.g", None).is_undefined());
}

#[test]
fn default_value_by_type_and_scope_parsing() {
    assert_deep_strict_equal(&get_default_value(&s("boolean")), &Dynamic::Bool(false));
    assert_deep_strict_equal(&get_default_value(&s("integer")), &n(0.0));
    assert_deep_strict_equal(&get_default_value(&s("number")), &n(0.0));
    assert_deep_strict_equal(&get_default_value(&s("string")), &s(""));
    assert_deep_strict_equal(&get_default_value(&s("array")), &arr(vec![]));
    assert_deep_strict_equal(&get_default_value(&s("object")), &empty_obj());
    assert_deep_strict_equal(&get_default_value(&s("null")), &Dynamic::Null);
    assert_deep_strict_equal(&get_default_value(&Dynamic::Undefined), &Dynamic::Null);
    assert_deep_strict_equal(&get_default_value(&j("['array', 'null']")), &arr(vec![]));

    assert_eq!(parse_scope("application"), ConfigurationScope::Application);
    assert_eq!(parse_scope("machine"), ConfigurationScope::Machine);
    assert_eq!(parse_scope("resource"), ConfigurationScope::Resource);
    assert_eq!(parse_scope("machine-overridable"), ConfigurationScope::MachineOverridable);
    assert_eq!(parse_scope("language-overridable"), ConfigurationScope::LanguageOverridable);
    assert_eq!(parse_scope("window"), ConfigurationScope::Window);
    assert_eq!(parse_scope("qualquer outro"), ConfigurationScope::Window);
}

#[test]
fn configuration_target_names_and_numbers_are_the_originals() {
    assert_eq!(configuration_target_to_string(ConfigurationTarget::Application), "APPLICATION");
    assert_eq!(configuration_target_to_string(ConfigurationTarget::WorkspaceFolder), "WORKSPACE_FOLDER");
    assert_eq!(configuration_target_to_string(ConfigurationTarget::Memory), "MEMORY");
    assert_eq!(ConfigurationTarget::Application as u8, 1);
    assert_eq!(ConfigurationTarget::Default as u8, 7);
    assert_eq!(ConfigurationTarget::Memory as u8, 8);
    assert_eq!(ConfigurationScope::Application as u8, 1);
    assert_eq!(ConfigurationScope::MachineOverridable as u8, 7);
}
