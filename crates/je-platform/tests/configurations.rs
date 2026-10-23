//! Porte de `src/vs/platform/configuration/test/common/configurations.test.ts` (suíte `DefaultConfiguration`), caso a caso e na mesma ordem.
//!
//! `await defaults.initialize()` é a chamada síncrona `initialize()` (o corpo original não tem `await`), e
//! `Event.toPromise(evento)` é o primeiro evento guardado por um listener, porque o registro dispara os
//! eventos de forma síncrona. O `restoreConfigurationRegistryAfterSuite` é o `RegistryGuard`, por teste.

mod common;

use std::cell::RefCell;
use std::rc::Rc;

use je_base::JsString;
use je_base::dynamic::Dynamic;
use je_base::objects;
use je_platform::configuration::defaults::{DefaultConfiguration, DefaultConfigurationChange};
use je_platform::configuration::registry::{ConfigurationDefaultSource, ConfigurationDefaults, ExtensionInfo, configuration_registry, settings_schemas};

use common::*;

/// `Event.toPromise(defaults.onDidChangeConfiguration)`: guarda o primeiro evento.
fn first_change(defaults: &Rc<DefaultConfiguration>, leaks: &impl Fn(je_base::lifecycle::DisposableRef)) -> Rc<RefCell<Option<DefaultConfigurationChange>>> {
    let captured: Rc<RefCell<Option<DefaultConfigurationChange>>> = Rc::new(RefCell::new(None));
    let sink = captured.clone();
    leaks(defaults.on_did_change_configuration().listen(move |change| {
        sink.borrow_mut().get_or_insert_with(|| change.clone());
    }));
    captured
}

fn sorted(mut keys: Vec<JsString>) -> Vec<JsString> {
    keys.sort();
    keys
}

fn overrides_dynamic(model: &je_platform::configuration::models::ConfigurationModel) -> Dynamic {
    Dynamic::array(model.overrides().iter().map(|o| o.to_dynamic()))
}

fn ext(id: &str) -> ConfigurationDefaultSource {
    ConfigurationDefaultSource::Extension(ExtensionInfo { id: id.to_string(), display_name: Some(id.to_string()) })
}

#[test]
fn hidden_experimental_defaults_update_and_unregister_without_entering_settings_schemas() {
    let leaks = ensure_no_disposables_are_leaked_in_test_suite();
    let guard = RegistryGuard::new();
    guard.reset();
    let registry = configuration_registry();

    let key = "test.hiddenExperimentalDefault";
    let other_key = "test.hiddenNonExperimentalDefault";
    let configuration = node(
        "test.hiddenDefaults",
        vec![
            (key, prop("{ 'type': 'boolean', 'default': false, 'included': false, 'experiment': { 'mode': 'auto' } }")),
            (other_key, prop("{ 'type': 'boolean', 'default': true, 'included': false }")),
        ],
    );
    registry.register_configuration(configuration.clone());
    let defaults = leaks.add_rc(DefaultConfiguration::new(null_log()));
    defaults.initialize();
    let other_default = defaults.configuration_model().get_value(other_key);
    let mut values = vec![defaults.configuration_model().get_value(key)];
    let changes: Rc<RefCell<Vec<Dynamic>>> = Rc::new(RefCell::new(Vec::new()));
    let sink = changes.clone();
    let key_js = jstr(key);
    leaks.add(defaults.on_did_change_configuration().listen(move |e| {
        if e.properties.contains(&key_js) {
            sink.borrow_mut().push(e.defaults.get_value(&key_js));
        }
    }));
    let override_ = ConfigurationDefaults::new(j("{ 'test.hiddenExperimentalDefault': true }"), Some(ConfigurationDefaultSource::Text("experiments".to_string())));
    registry.register_default_configurations(std::slice::from_ref(&override_));
    values.push(defaults.configuration_model().get_value(key));
    values.push(defaults.reload().get_value(key));
    let visible_during_override = registry.get_configuration_properties().get(key).is_some() || settings_schemas().all_settings.properties.get(&jstr(key)).is_some();
    registry.deregister_default_configurations(&[override_]);
    values.push(defaults.configuration_model().get_value(key));
    registry.deregister_configurations(&[configuration]);
    values.push(defaults.configuration_model().get_value(key));

    assert_deep_strict_equal(&arr(values), &arr(vec![Dynamic::Bool(false), Dynamic::Bool(true), Dynamic::Bool(true), Dynamic::Bool(false), U]));
    assert_deep_strict_equal(&arr(changes.borrow().clone()), &arr(vec![Dynamic::Bool(true), Dynamic::Bool(false), U]));
    assert!(!visible_during_override);
    assert_deep_strict_equal(&other_default, &U);
    assert!(registry.get_excluded_configuration_properties().get(key).is_none());
}

#[test]
fn notifies_when_a_hidden_experimental_setting_is_registered_after_initialization() {
    let leaks = ensure_no_disposables_are_leaked_in_test_suite();
    let guard = RegistryGuard::new();
    guard.reset();
    let registry = configuration_registry();

    let defaults = leaks.add_rc(DefaultConfiguration::new(null_log()));
    defaults.initialize();
    let key = "test.lateHiddenExperimentalDefault";
    let changes: Rc<RefCell<Vec<Dynamic>>> = Rc::new(RefCell::new(Vec::new()));
    let sink = changes.clone();
    let key_js = jstr(key);
    leaks.add(defaults.on_did_change_configuration().listen(move |e| {
        if e.properties.contains(&key_js) {
            sink.borrow_mut().push(e.defaults.get_value(&key_js));
        }
    }));
    let configuration = je_platform::configuration::registry::ConfigurationNode::new(
        je_platform::configuration::registry::ConfigurationNodeData::default()
            .with_properties(vec![(key, prop("{ 'type': 'boolean', 'default': false, 'included': false, 'experiment': { 'mode': 'auto' } }"))]),
    );
    registry.register_configuration(configuration.clone());
    registry.deregister_configurations(&[configuration]);
    assert_deep_strict_equal(&arr(changes.borrow().clone()), &arr(vec![Dynamic::Bool(false), U]));
}

#[test]
fn test_registering_a_property_before_initialize() {
    let leaks = ensure_no_disposables_are_leaked_in_test_suite();
    let guard = RegistryGuard::new();
    guard.reset();
    let registry = configuration_registry();

    let test_object = leaks.add_rc(DefaultConfiguration::new(null_log()));
    registry.register_configuration(node_full("a", 1.0, "a", vec![("a", prop("{ 'description': 'a', 'type': 'boolean', 'default': false }"))]));
    let actual = test_object.initialize();
    assert_eq!(actual.get_value("a"), Dynamic::Bool(false));
}

#[test]
fn test_registering_a_property_and_do_not_initialize() {
    let leaks = ensure_no_disposables_are_leaked_in_test_suite();
    let guard = RegistryGuard::new();
    guard.reset();
    let registry = configuration_registry();

    let test_object = leaks.add_rc(DefaultConfiguration::new(null_log()));
    registry.register_configuration(node_full("a", 1.0, "a", vec![("a", prop("{ 'description': 'a', 'type': 'boolean', 'default': false }"))]));
    assert!(test_object.configuration_model().get_value("a").is_undefined());
}

#[test]
fn test_registering_a_property_after_initialize() {
    let leaks = ensure_no_disposables_are_leaked_in_test_suite();
    let guard = RegistryGuard::new();
    guard.reset();
    let registry = configuration_registry();

    let test_object = leaks.add_rc(DefaultConfiguration::new(null_log()));
    test_object.initialize();
    let promise = first_change(&test_object, &|d| {
        leaks.add(d);
    });
    registry.register_configuration(node_full(
        "a",
        1.0,
        "a",
        vec![("defaultConfiguration.testSetting1", prop("{ 'description': 'a', 'type': 'boolean', 'default': false }"))],
    ));
    let change = promise.borrow().clone().expect("o evento disparou");
    assert_eq!(change.defaults.get_value("defaultConfiguration.testSetting1"), Dynamic::Bool(false));
    assert_eq!(change.properties, strs(&["defaultConfiguration.testSetting1"]));
}

#[test]
fn test_registering_nested_properties() {
    let leaks = ensure_no_disposables_are_leaked_in_test_suite();
    let guard = RegistryGuard::new();
    guard.reset();
    let registry = configuration_registry();

    let test_object = leaks.add_rc(DefaultConfiguration::new(null_log()));
    registry.register_configuration(node_full(
        "a",
        1.0,
        "a",
        vec![
            ("a.b", prop("{ 'description': '1', 'type': 'object', 'default': {} }")),
            ("a.b.c", prop("{ 'description': '2', 'type': 'object', 'default': '2' }")),
        ],
    ));

    let actual = test_object.initialize();

    assert!(objects::equals(&actual.get_value("a"), &j("{ 'b': { 'c': '2' } }")));
    assert!(objects::equals(&actual.contents(), &j("{ 'a': { 'b': { 'c': '2' } } }")));
    assert_eq!(sorted(actual.keys()), strs(&["a.b", "a.b.c"]));
}

#[test]
fn test_registering_the_same_property_again() {
    let leaks = ensure_no_disposables_are_leaked_in_test_suite();
    let guard = RegistryGuard::new();
    guard.reset();
    let registry = configuration_registry();

    let test_object = leaks.add_rc(DefaultConfiguration::new(null_log()));
    registry.register_configuration(node_full("a", 1.0, "a", vec![("a", prop("{ 'description': 'a', 'type': 'boolean', 'default': true }"))]));
    registry.register_configuration(node_full("a", 1.0, "a", vec![("a", prop("{ 'description': 'a', 'type': 'boolean', 'default': false }"))]));
    let actual = test_object.initialize();
    assert_eq!(Dynamic::Bool(true), actual.get_value("a"));
}

#[test]
fn test_registering_an_override_identifier() {
    let leaks = ensure_no_disposables_are_leaked_in_test_suite();
    let guard = RegistryGuard::new();
    guard.reset();
    let registry = configuration_registry();

    let test_object = leaks.add_rc(DefaultConfiguration::new(null_log()));
    registry.register_default_configurations(&[defaults(j("{ '[a]': { 'b': true } }"))]);
    let actual = test_object.initialize();
    assert!(objects::equals(&actual.get_value("[a]"), &j("{ 'b': true }")));
    assert!(objects::equals(&actual.contents(), &j("{ '[a]': { 'b': true } }")));
    assert!(objects::equals(&overrides_dynamic(&actual), &arr(vec![obj(vec![("contents", j("{ 'b': true }")), ("identifiers", dyn_strs(&["a"])), ("keys", dyn_strs(&["b"]))])])));
    assert_eq!(sorted(actual.keys()), strs(&["[a]"]));
    assert_eq!(actual.get_override_value(Some("b"), "a"), Dynamic::Bool(true));
}

#[test]
fn test_registering_a_normal_property_and_override_identifier() {
    let leaks = ensure_no_disposables_are_leaked_in_test_suite();
    let guard = RegistryGuard::new();
    guard.reset();
    let registry = configuration_registry();

    let test_object = leaks.add_rc(DefaultConfiguration::new(null_log()));
    registry.register_configuration(node_full("a", 1.0, "a", vec![("b", prop("{ 'description': 'b', 'type': 'boolean', 'default': false }"))]));

    registry.register_default_configurations(&[defaults(j("{ '[a]': { 'b': true } }"))]);

    let actual = test_object.initialize();
    assert_deep_strict_equal(&actual.get_value("b"), &Dynamic::Bool(false));
    assert!(objects::equals(&actual.get_value("[a]"), &j("{ 'b': true }")));
    assert!(objects::equals(&actual.contents(), &j("{ 'b': false, '[a]': { 'b': true } }")));
    assert!(objects::equals(&overrides_dynamic(&actual), &arr(vec![obj(vec![("contents", j("{ 'b': true }")), ("identifiers", dyn_strs(&["a"])), ("keys", dyn_strs(&["b"]))])])));
    assert_eq!(sorted(actual.keys()), strs(&["[a]", "b"]));
    assert_eq!(actual.get_override_value(Some("b"), "a"), Dynamic::Bool(true));
}

#[test]
fn test_normal_property_is_registered_after_override_identifier() {
    let leaks = ensure_no_disposables_are_leaked_in_test_suite();
    let guard = RegistryGuard::new();
    guard.reset();
    let registry = configuration_registry();

    let test_object = leaks.add_rc(DefaultConfiguration::new(null_log()));
    let promise = first_change(&test_object, &|d| {
        leaks.add(d);
    });
    registry.register_default_configurations(&[defaults(j("{ '[a]': { 'b': true } }"))]);

    test_object.initialize();

    registry.register_configuration(node_full("a", 1.0, "a", vec![("b", prop("{ 'description': 'b', 'type': 'boolean', 'default': false }"))]));

    let change = promise.borrow().clone().expect("o evento disparou");
    let actual = change.defaults;
    assert_deep_strict_equal(&actual.get_value("b"), &Dynamic::Bool(false));
    assert!(objects::equals(&actual.get_value("[a]"), &j("{ 'b': true }")));
    assert!(objects::equals(&actual.contents(), &j("{ 'b': false, '[a]': { 'b': true } }")));
    assert!(objects::equals(&overrides_dynamic(&actual), &arr(vec![obj(vec![("contents", j("{ 'b': true }")), ("identifiers", dyn_strs(&["a"])), ("keys", dyn_strs(&["b"]))])])));
    assert_eq!(sorted(actual.keys()), strs(&["[a]", "b"]));
    assert_eq!(actual.get_override_value(Some("b"), "a"), Dynamic::Bool(true));
    assert_eq!(change.properties, strs(&["b"]));
}

#[test]
fn test_override_identifier_is_registered_after_property() {
    let leaks = ensure_no_disposables_are_leaked_in_test_suite();
    let guard = RegistryGuard::new();
    guard.reset();
    let registry = configuration_registry();

    let test_object = leaks.add_rc(DefaultConfiguration::new(null_log()));
    let promise = first_change(&test_object, &|d| {
        leaks.add(d);
    });
    registry.register_configuration(node_full("a", 1.0, "a", vec![("b", prop("{ 'description': 'b', 'type': 'boolean', 'default': false }"))]));
    test_object.initialize();

    registry.register_default_configurations(&[defaults(j("{ '[a]': { 'b': true } }"))]);

    let change = promise.borrow().clone().expect("o evento disparou");
    let actual = change.defaults;
    assert_deep_strict_equal(&actual.get_value("b"), &Dynamic::Bool(false));
    assert!(objects::equals(&actual.get_value("[a]"), &j("{ 'b': true }")));
    assert!(objects::equals(&actual.contents(), &j("{ 'b': false, '[a]': { 'b': true } }")));
    assert!(objects::equals(&overrides_dynamic(&actual), &arr(vec![obj(vec![("contents", j("{ 'b': true }")), ("identifiers", dyn_strs(&["a"])), ("keys", dyn_strs(&["b"]))])])));
    assert_eq!(sorted(actual.keys()), strs(&["[a]", "b"]));
    assert_eq!(actual.get_override_value(Some("b"), "a"), Dynamic::Bool(true));
    assert_eq!(change.properties, strs(&["[a]"]));
}

#[test]
fn test_register_override_identifier_and_property_after_initialize() {
    let leaks = ensure_no_disposables_are_leaked_in_test_suite();
    let guard = RegistryGuard::new();
    guard.reset();
    let registry = configuration_registry();

    let test_object = leaks.add_rc(DefaultConfiguration::new(null_log()));

    test_object.initialize();

    registry.register_configuration(node_full("a", 1.0, "a", vec![("b", prop("{ 'description': 'b', 'type': 'boolean', 'default': false }"))]));
    registry.register_default_configurations(&[defaults(j("{ '[a]': { 'b': true } }"))]);

    let actual = test_object.configuration_model();
    assert_deep_strict_equal(&actual.get_value("b"), &Dynamic::Bool(false));
    assert!(objects::equals(&actual.get_value("[a]"), &j("{ 'b': true }")));
    assert!(objects::equals(&actual.contents(), &j("{ 'b': false, '[a]': { 'b': true } }")));
    assert!(objects::equals(&overrides_dynamic(&actual), &arr(vec![obj(vec![("contents", j("{ 'b': true }")), ("identifiers", dyn_strs(&["a"])), ("keys", dyn_strs(&["b"]))])])));
    assert_eq!(sorted(actual.keys()), strs(&["[a]", "b"]));
    assert_eq!(actual.get_override_value(Some("b"), "a"), Dynamic::Bool(true));
}

#[test]
fn test_deregistering_a_property() {
    let leaks = ensure_no_disposables_are_leaked_in_test_suite();
    let guard = RegistryGuard::new();
    guard.reset();
    let registry = configuration_registry();

    let test_object = leaks.add_rc(DefaultConfiguration::new(null_log()));
    let promise = first_change(&test_object, &|d| {
        leaks.add(d);
    });
    let node = node_full("a", 1.0, "a", vec![("a", prop("{ 'description': 'a', 'type': 'boolean', 'default': false }"))]);
    registry.register_configuration(node.clone());
    test_object.initialize();
    registry.deregister_configurations(&[node]);

    let change = promise.borrow().clone().expect("o evento disparou");
    let actual = change.defaults;
    assert!(actual.get_value("a").is_undefined());
    assert!(objects::equals(&actual.contents(), &j("{}")));
    assert_eq!(actual.keys(), strs(&[]));
    assert_eq!(change.properties, strs(&["a"]));
}

#[test]
fn test_deregistering_an_override_identifier() {
    let leaks = ensure_no_disposables_are_leaked_in_test_suite();
    let guard = RegistryGuard::new();
    guard.reset();
    let registry = configuration_registry();

    let test_object = leaks.add_rc(DefaultConfiguration::new(null_log()));
    registry.register_configuration(node_full("a", 1.0, "a", vec![("b", prop("{ 'description': 'b', 'type': 'boolean', 'default': false }"))]));
    let node = defaults(j("{ '[a]': { 'b': true } }"));
    registry.register_default_configurations(std::slice::from_ref(&node));
    test_object.initialize();
    registry.deregister_default_configurations(&[node]);
    assert_deep_strict_equal(&test_object.configuration_model().get_value("[a]"), &U);
    assert!(objects::equals(&test_object.configuration_model().contents(), &j("{ 'b': false }")));
    assert!(objects::equals(&overrides_dynamic(&test_object.configuration_model()), &j("[]")));
    assert_eq!(test_object.configuration_model().keys(), strs(&["b"]));
    assert!(test_object.configuration_model().get_override_value(Some("b"), "a").is_undefined());
}

#[test]
fn test_deregistering_a_merged_language_object_setting() {
    let leaks = ensure_no_disposables_are_leaked_in_test_suite();
    let guard = RegistryGuard::new();
    guard.reset();
    let registry = configuration_registry();

    let test_object = leaks.add_rc(DefaultConfiguration::new(null_log()));
    registry.register_configuration(node_full("b", 1.0, "b", vec![("b", prop("{ 'description': 'b', 'type': 'object', 'default': {} }"))]));
    let node1 = ConfigurationDefaults::new(j("{ '[a]': { 'b': { 'aa': '1', 'bb': '2' } } }"), Some(ext("source1")));

    let node2 = ConfigurationDefaults::new(j("{ '[a]': { 'b': { 'bb': '20', 'cc': '30' } } }"), Some(ext("source2")));
    registry.register_default_configurations(std::slice::from_ref(&node1));
    registry.register_default_configurations(&[node2]);
    test_object.initialize();

    registry.deregister_default_configurations(&[node1]);
    let model = test_object.configuration_model();
    assert!(objects::equals(&model.get_value("[a]"), &j("{ 'b': { 'bb': '20', 'cc': '30' } }")));
    assert!(objects::equals(&model.contents(), &j("{ '[a]': { 'b': { 'bb': '20', 'cc': '30' } }, 'b': {} }")));
    assert!(objects::equals(
        &overrides_dynamic(&model),
        &arr(vec![obj(vec![("contents", j("{ 'b': { 'bb': '20', 'cc': '30' } }")), ("identifiers", dyn_strs(&["a"])), ("keys", dyn_strs(&["b"]))])])
    ));
    assert_eq!(sorted(model.keys()), strs(&["[a]", "b"]));
    assert!(objects::equals(&model.get_override_value(Some("b"), "a"), &j("{ 'bb': '20', 'cc': '30' }")));
}
