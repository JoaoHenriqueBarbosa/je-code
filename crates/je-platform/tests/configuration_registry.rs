//! Porte de `src/vs/platform/configuration/test/common/configurationRegistry.test.ts` (suíte `ConfigurationRegistry`), caso a caso e na mesma ordem.
//!
//! A suíte aninhada `isConfigurationDefaultSourceEquals` é o módulo `is_configuration_default_source_equals`,
//! no fim, como no original. O registro global é um singleton por thread (ver `registry.rs`), e cada teste
//! roda no seu thread: o `restoreConfigurationRegistryAfterSuite` (`RegistryGuard`) fica por teste.
//! `PolicyCategory.Extensions` é a string `Extensions` (`vs/base/common/policy.ts` não está portado).

mod common;

use je_base::dynamic::Dynamic;
use je_platform::configuration::registry::{
    ConfigurationDefaultSource, ConfigurationDefaultValueSource, ConfigurationDefaults, ExtensionInfo, configuration_registry,
    is_configuration_default_source_equals,
};

use common::*;

fn ext(id: &str, display_name: &str) -> ConfigurationDefaultSource {
    ConfigurationDefaultSource::Extension(ExtensionInfo { id: id.to_string(), display_name: Some(display_name.to_string()) })
}

fn text(source: &str) -> ConfigurationDefaultSource {
    ConfigurationDefaultSource::Text(source.to_string())
}

fn with_source(overrides: &str, source: ConfigurationDefaultSource) -> ConfigurationDefaults {
    ConfigurationDefaults::new(j(overrides), Some(source))
}

fn default_of(key: &str) -> Dynamic {
    configuration_registry().get_configuration_properties().get(key).expect("propriedade registrada").default_value()
}

fn policy_property(name: &str, description_key: &str) -> je_platform::configuration::registry::PropertySchema {
    prop(&format!(
        "{{ 'type': 'object', 'policy': {{ 'name': '{name}', 'category': 'Extensions', 'minimumVersion': '1.0.0', 'localization': {{ 'description': {{ 'key': '{description_key}', 'value': '' }} }} }} }}"
    ))
}

#[test]
fn configuration_override() {
    let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
    let guard = RegistryGuard::new();
    guard.reset();
    let registry = configuration_registry();

    registry.register_configuration(node("_test_default", vec![("config", prop("{ 'type': 'object' }"))]));
    registry.register_default_configurations(&[defaults(j("{ 'config': { 'a': 1, 'b': 2 } }"))]);
    registry.register_default_configurations(&[defaults(j("{ '[lang]': { 'a': 2, 'c': 3 } }"))]);

    assert_deep_strict_equal(&default_of("config"), &j("{ 'a': 1, 'b': 2 }"));
    assert_deep_strict_equal(&default_of("[lang]"), &j("{ 'a': 2, 'c': 3 }"));
}

#[test]
fn configuration_override_defaults_prevent_overriding_default_value() {
    let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
    let guard = RegistryGuard::new();
    guard.reset();
    let registry = configuration_registry();

    registry.register_configuration(node(
        "_test_default",
        vec![("config.preventDefaultValueOverride", prop("{ 'type': 'object', 'default': { 'a': 0 }, 'disallowConfigurationDefault': true }"))],
    ));

    registry.register_default_configurations(&[defaults(j("{ 'config.preventDefaultValueOverride': { 'a': 1, 'b': 2 } }"))]);

    assert_deep_strict_equal(&default_of("config.preventDefaultValueOverride"), &j("{ 'a': 0 }"));
}

#[test]
fn configuration_override_defaults_merges_defaults() {
    let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
    let guard = RegistryGuard::new();
    guard.reset();
    let registry = configuration_registry();

    registry.register_default_configurations(&[defaults(j("{ '[lang]': { 'a': 1, 'b': 2 } }"))]);
    registry.register_default_configurations(&[defaults(j("{ '[lang]': { 'a': 2, 'c': 3 } }"))]);

    assert_deep_strict_equal(&default_of("[lang]"), &j("{ 'a': 2, 'b': 2, 'c': 3 }"));
}

#[test]
fn configuration_defaults_merge_object_default_overrides() {
    let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
    let guard = RegistryGuard::new();
    guard.reset();
    let registry = configuration_registry();

    registry.register_configuration(node("_test_default", vec![("config", prop("{ 'type': 'object' }"))]));
    registry.register_default_configurations(&[defaults(j("{ 'config': { 'a': 1, 'b': 2 } }"))]);
    registry.register_default_configurations(&[defaults(j("{ 'config': { 'a': 2, 'c': 3 } }"))]);

    assert_deep_strict_equal(&default_of("config"), &j("{ 'a': 2, 'b': 2, 'c': 3 }"));
}

#[test]
fn registering_multiple_settings_with_same_policy() {
    let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
    let guard = RegistryGuard::new();
    guard.reset();
    let registry = configuration_registry();

    registry.register_configuration(node("_test_default", vec![("policy1", policy_property("policy", "")), ("policy2", policy_property("policy", ""))]));
    let actual = registry.get_configuration_properties();
    assert!(actual.get("policy1").is_some());
    assert!(actual.get("policy2").is_none());
}

#[test]
fn a_policy_reference_attaches_a_subordinate_setting_to_an_owning_policy() {
    let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
    let guard = RegistryGuard::new();
    guard.reset();
    let registry = configuration_registry();

    let owner = prop_with(
        "{ 'type': 'boolean' }",
        vec![("policy", j("{ 'name': 'sharedPolicy', 'category': 'Extensions', 'minimumVersion': '1.0.0', 'localization': { 'description': { 'key': 'owner', 'value': '' } } }"))],
    );
    let subordinate = prop_with("{ 'type': 'boolean' }", vec![("policyReference", j("{ 'name': 'sharedPolicy' }"))]);
    registry.register_configuration(node("_test_default", vec![("policy.owner", owner), ("policy.subordinate", subordinate)]));
    let actual = registry.get_configuration_properties();
    assert!(actual.get("policy.owner").is_some());
    assert!(actual.get("policy.subordinate").is_some());
    assert_eq!(registry.get_policy_configurations().get(&"sharedPolicy".to_string()), Some(&jstr("policy.owner")));
    assert_eq!(registry.get_policy_reference_configurations().get(&"sharedPolicy".to_string()).cloned().unwrap_or_default(), strs(&["policy.subordinate"]));
}

#[test]
fn a_policy_reference_does_not_require_its_owner_to_be_registered() {
    let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
    let guard = RegistryGuard::new();
    guard.reset();
    let registry = configuration_registry();

    let orphan = prop_with("{ 'type': 'boolean' }", vec![("policyReference", j("{ 'name': 'externallyOwnedPolicy' }"))]);
    registry.register_configuration(node("_test_default", vec![("policy.orphanReference", orphan)]));
    let actual = registry.get_configuration_properties();
    assert!(actual.get("policy.orphanReference").is_some());
    assert_eq!(registry.get_policy_configurations().get(&"externallyOwnedPolicy".to_string()), None);
    assert_eq!(
        registry.get_policy_reference_configurations().get(&"externallyOwnedPolicy".to_string()).cloned().unwrap_or_default(),
        strs(&["policy.orphanReference"])
    );
}

#[test]
fn a_setting_declaring_both_policy_and_policy_reference_is_rejected() {
    let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
    let guard = RegistryGuard::new();
    guard.reset();
    let registry = configuration_registry();

    let both = prop_with(
        "{ 'type': 'boolean' }",
        vec![
            ("policy", j("{ 'name': 'policyBoth', 'category': 'Extensions', 'minimumVersion': '1.0.0', 'localization': { 'description': { 'key': 'both', 'value': '' } } }")),
            ("policyReference", j("{ 'name': 'policyBothReference' }")),
        ],
    );
    registry.register_configuration(node("_test_default", vec![("policy.both", both)]));
    let actual = registry.get_configuration_properties();
    assert!(actual.get("policy.both").is_none());
    assert_eq!(registry.get_policy_configurations().get(&"policyBoth".to_string()), None);
    assert!(registry.get_policy_reference_configurations().get(&"policyBothReference".to_string()).is_none());
}

#[test]
fn configuration_defaults_deregister_merged_object_default_override() {
    let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
    let guard = RegistryGuard::new();
    guard.reset();
    let registry = configuration_registry();

    registry.register_configuration(node("_test_default", vec![("config", prop("{ 'type': 'object' }"))]));

    let overrides1 = [with_source("{ 'config': { 'a': 1, 'b': 2 } }", ext("source1", "source1"))];
    let overrides2 = [with_source("{ 'config': { 'a': 2, 'c': 3 } }", ext("source2", "source2"))];

    registry.register_default_configurations(&overrides1);
    registry.register_default_configurations(&overrides2);

    assert_deep_strict_equal(&default_of("config"), &j("{ 'a': 2, 'b': 2, 'c': 3 }"));

    registry.deregister_default_configurations(&overrides2);

    assert_deep_strict_equal(&default_of("config"), &j("{ 'a': 1, 'b': 2 }"));

    registry.deregister_default_configurations(&overrides1);

    assert_deep_strict_equal(&default_of("config"), &j("{}"));
}

#[test]
fn configuration_defaults_deregister_merged_object_default_override_without_source() {
    let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
    let guard = RegistryGuard::new();
    guard.reset();
    let registry = configuration_registry();

    registry.register_configuration(node("_test_default", vec![("config", prop("{ 'type': 'object' }"))]));

    let overrides1 = [defaults(j("{ 'config': { 'a': 1, 'b': 2 } }"))];
    let overrides2 = [defaults(j("{ 'config': { 'a': 2, 'c': 3 } }"))];

    registry.register_default_configurations(&overrides1);
    registry.register_default_configurations(&overrides2);

    assert_deep_strict_equal(&default_of("config"), &j("{ 'a': 2, 'b': 2, 'c': 3 }"));

    registry.deregister_default_configurations(&overrides2);

    assert_deep_strict_equal(&default_of("config"), &j("{ 'a': 1, 'b': 2 }"));

    registry.deregister_default_configurations(&overrides1);

    assert_deep_strict_equal(&default_of("config"), &j("{}"));
}

#[test]
fn configuration_defaults_deregister_merged_object_default_language_overrides() {
    let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
    let guard = RegistryGuard::new();
    guard.reset();
    let registry = configuration_registry();

    registry.register_configuration(node("_test_default", vec![("config", prop("{ 'type': 'object' }"))]));

    let overrides1 = [with_source("{ '[lang]': { 'config': { 'a': 1, 'b': 2 } } }", ext("source1", "source1"))];
    let overrides2 = [with_source("{ '[lang]': { 'config': { 'a': 2, 'c': 3 } } }", ext("source2", "source2"))];

    registry.register_default_configurations(&overrides1);
    registry.register_default_configurations(&overrides2);

    assert_deep_strict_equal(&default_of("[lang]"), &j("{ 'config': { 'a': 2, 'b': 2, 'c': 3 } }"));

    registry.deregister_default_configurations(&overrides2);

    assert_deep_strict_equal(&default_of("[lang]"), &j("{ 'config': { 'a': 1, 'b': 2 } }"));

    registry.deregister_default_configurations(&overrides1);

    assert!(registry.get_configuration_properties().get("[lang]").is_none());
}

#[test]
fn configuration_defaults_string_source() {
    let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
    let guard = RegistryGuard::new();
    guard.reset();
    let registry = configuration_registry();

    registry.register_configuration(node("_test_default", vec![("config", prop("{ 'type': 'object' }"))]));

    let overrides1 = [with_source("{ 'config': { 'a': 1, 'b': 2 } }", text("source1"))];
    let overrides2 = [with_source("{ 'config': { 'a': 2, 'c': 3 } }", text("source2"))];

    registry.register_default_configurations(&overrides1);
    registry.register_default_configurations(&overrides2);

    assert_deep_strict_equal(&default_of("config"), &j("{ 'a': 2, 'b': 2, 'c': 3 }"));
    let property = registry.get_configuration_properties().get("config").unwrap();
    assert!(matches!(property.default_value_source(), Some(ConfigurationDefaultValueSource::Map(_))));

    registry.deregister_default_configurations(&overrides2);

    assert_deep_strict_equal(&default_of("config"), &j("{ 'a': 1, 'b': 2 }"));

    registry.deregister_default_configurations(&overrides1);

    assert_deep_strict_equal(&default_of("config"), &j("{}"));
}

#[test]
fn configuration_defaults_deregister_with_string_source_and_extension_source() {
    let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
    let guard = RegistryGuard::new();
    guard.reset();
    let registry = configuration_registry();

    registry.register_configuration(node("_test_default", vec![("config", prop("{ 'type': 'object' }"))]));

    let overrides1 = [with_source("{ 'config': { 'a': 1, 'b': 2 } }", text("stringSource"))];
    let overrides2 = [with_source("{ 'config': { 'a': 2, 'c': 3 } }", ext("extSource", "Extension Source"))];

    registry.register_default_configurations(&overrides1);
    registry.register_default_configurations(&overrides2);

    assert_deep_strict_equal(&default_of("config"), &j("{ 'a': 2, 'b': 2, 'c': 3 }"));

    registry.deregister_default_configurations(&overrides1);

    assert_deep_strict_equal(&default_of("config"), &j("{ 'a': 2, 'c': 3 }"));

    registry.deregister_default_configurations(&overrides2);

    assert_deep_strict_equal(&default_of("config"), &j("{}"));
}

#[test]
fn restoring_a_recorded_registry_keeps_the_declared_defaults_and_the_hidden_settings_of_its_configurations() {
    let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
    let guard = RegistryGuard::new();
    guard.reset();
    let registry = configuration_registry();

    let node = node(
        "_test_restore",
        vec![
            ("restore.overridden", prop("{ 'type': 'boolean', 'default': false }")),
            ("restore.hiddenMirrored", prop("{ 'type': 'string', 'default': '', 'included': false, 'agentHost': { 'key': 'restore.hiddenMirrored' } }")),
            ("restore.hiddenExperiment", prop("{ 'type': 'boolean', 'default': false, 'included': false, 'experiment': { 'mode': 'auto' } }")),
        ],
    );
    registry.register_configuration(node.clone());
    registry.register_default_configurations(&[defaults(j("{ 'restore.overridden': true }"))]);
    let state = || {
        let registry = configuration_registry();
        obj(vec![
            ("overriddenDefault", registry.get_configuration_properties().get("restore.overridden").map_or(Dynamic::Undefined, |p| p.default_value())),
            ("hiddenMirrored", registry.get_agent_host_sync_configurations().get(&jstr("restore.hiddenMirrored")).cloned().unwrap_or(Dynamic::Undefined)),
            (
                "hiddenExperiment",
                registry.get_excluded_configuration_properties().get("restore.hiddenExperiment").map_or(Dynamic::Undefined, |p| p.get("experiment")),
            ),
        ])
    };
    let registered = state();

    let restore = record_configuration_registry();
    guard.reset();
    restore();
    let restored = state();
    registry.deregister_default_configurations(&registry.get_registered_default_configurations());
    let without_override = state();
    registry.deregister_configurations(&[node]);
    let without_node = state();

    let hidden = |overridden: Dynamic, mirrored: Dynamic, experiment: Dynamic| {
        obj(vec![("overriddenDefault", overridden), ("hiddenMirrored", mirrored), ("hiddenExperiment", experiment)])
    };
    let mirrored = || j("{ 'key': 'restore.hiddenMirrored' }");
    let experiment = || j("{ 'mode': 'auto' }");
    assert_deep_strict_equal(&registered, &hidden(Dynamic::Bool(true), mirrored(), experiment()));
    assert_deep_strict_equal(&restored, &hidden(Dynamic::Bool(true), mirrored(), experiment()));
    // The declared default, not the overridden one
    assert_deep_strict_equal(&without_override, &hidden(Dynamic::Bool(false), mirrored(), experiment()));
    // The hidden settings belong to their node again
    assert_deep_strict_equal(&without_node, &hidden(Dynamic::Undefined, Dynamic::Undefined, Dynamic::Undefined));
}

mod is_configuration_default_source_equals {
    use super::*;

    fn ext_id(id: &str) -> ConfigurationDefaultSource {
        ConfigurationDefaultSource::Extension(ExtensionInfo::new(id))
    }

    #[test]
    fn both_undefined() {
        let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
        assert!(is_configuration_default_source_equals(None, None));
    }

    #[test]
    fn one_undefined() {
        let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
        assert!(!is_configuration_default_source_equals(Some(&text("source")), None));
        assert!(!is_configuration_default_source_equals(None, Some(&text("source"))));
        assert!(!is_configuration_default_source_equals(Some(&ext_id("ext")), None));
        assert!(!is_configuration_default_source_equals(None, Some(&ext_id("ext"))));
    }

    #[test]
    fn same_string_source() {
        let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
        assert!(is_configuration_default_source_equals(Some(&text("source")), Some(&text("source"))));
    }

    #[test]
    fn different_string_sources() {
        let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
        assert!(!is_configuration_default_source_equals(Some(&text("source1")), Some(&text("source2"))));
    }

    #[test]
    fn same_extension_source() {
        let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
        assert!(is_configuration_default_source_equals(Some(&ext_id("ext")), Some(&ext_id("ext"))));
    }

    #[test]
    fn different_extension_sources() {
        let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
        assert!(!is_configuration_default_source_equals(Some(&ext_id("ext1")), Some(&ext_id("ext2"))));
    }

    #[test]
    fn string_vs_extension_source() {
        let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
        assert!(!is_configuration_default_source_equals(Some(&text("ext")), Some(&ext_id("ext"))));
        assert!(!is_configuration_default_source_equals(Some(&ext_id("ext")), Some(&text("ext"))));
    }

    #[test]
    fn same_reference() {
        let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
        let source = ext("ext", "Extension");
        assert!(is_configuration_default_source_equals(Some(&source), Some(&source)));
    }
}
