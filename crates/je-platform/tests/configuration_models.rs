//! Porte de `src/vs/platform/configuration/test/common/configurationModels.test.ts` (suíte `ConfigurationModelParser`), caso a caso e na mesma ordem.
//!
//! O arquivo original tem oito suítes, aqui um módulo cada, na ordem: `ConfigurationModelParser`,
//! `ConfigurationModelParser - Excluded Properties`, `ConfigurationModelParser - Nested Restricted Properties`,
//! `ConfigurationModel`, `CustomConfigurationModel`, `Configuration`, `ConfigurationChangeEvent` e
//! `Configuration.Parse`. Diferenças de representação (ver `dynamic.rs` do `je-base`):
//! - não há cadeia de protótipos: no caso do `__proto__` só a parte observável (o valor da chave própria e
//!   a ausência de poluição) é conferida; `Object.getPrototypeOf(raw) === Object.prototype` não tem
//!   equivalente;
//! - `Object.create(null)` e `{}` são o mesmo valor;
//! - `await promise` de `Event.toPromise` não existe: o evento dispara de forma síncrona, e o teste guarda o
//!   primeiro evento recebido.
//! - O `suiteSetup` que registra um nó no registro global roda no início de cada teste do módulo (cada teste
//!   tem o seu thread e, portanto, o seu registro).

mod common;

use std::rc::Rc;

use je_base::dynamic::Dynamic;
use je_platform::configuration::models::{
    Configuration, ConfigurationChangeEvent, ConfigurationModel, ConfigurationModelParser, ConfigurationParseOptions, PreviousConfiguration, Raw, merge_changes,
};
use je_platform::configuration::registry::{ConfigurationNode, ConfigurationScope, configuration_registry};
use je_platform::configuration::{ConfigurationOverrides, ConfigurationValue};
use je_platform::workspace::WorkspaceFolder;

use common::*;

fn parser(name: &str) -> ConfigurationModelParser {
    ConfigurationModelParser::new(name, null_log())
}

fn parse_with(content: &str, options: &ConfigurationParseOptions) -> ConfigurationModelParser {
    let mut parser = parser("test");
    parser.parse(Some(content), Some(options));
    parser
}

fn parse_plain(name: &str, content: &str) -> ConfigurationModelParser {
    let mut parser = parser(name);
    parser.parse(Some(content), None);
    parser
}

fn options() -> ConfigurationParseOptions {
    ConfigurationParseOptions::default()
}

fn sorted_set(keys: &[je_base::JsString]) -> Vec<je_base::JsString> {
    let mut keys = keys.to_vec();
    keys.sort();
    keys.dedup();
    keys
}

mod configuration_model_parser {
    use super::*;

    /// O `suiteSetup` da suíte.
    fn suite_setup() {
        configuration_registry().register_configuration(node(
            "ConfigurationModelParserTest",
            vec![("ConfigurationModelParserTest.windowSetting", prop("{ 'type': 'string', 'default': 'isSet' }"))],
        ));
    }

    #[test]
    fn parse_configuration_model_with_single_override_identifier() {
        let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
        suite_setup();
        let test_object = parse_plain("", &stringify(&j("{ '[x]': { 'a': 1 } }")));

        assert_eq!(
            stringify_overrides(&test_object.configuration_model().overrides()),
            stringify(&arr(vec![obj(vec![("identifiers", dyn_strs(&["x"])), ("keys", dyn_strs(&["a"])), ("contents", j("{ 'a': 1 }"))])]))
        );
    }

    #[test]
    fn parse_configuration_model_with_multiple_override_identifiers() {
        let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
        suite_setup();
        let test_object = parse_plain("", &stringify(&j("{ '[x][y]': { 'a': 1 } }")));

        assert_eq!(
            stringify_overrides(&test_object.configuration_model().overrides()),
            stringify(&arr(vec![obj(vec![("identifiers", dyn_strs(&["x", "y"])), ("keys", dyn_strs(&["a"])), ("contents", j("{ 'a': 1 }"))])]))
        );
    }

    #[test]
    fn parse_configuration_model_with_multiple_duplicate_override_identifiers() {
        let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
        suite_setup();
        let test_object = parse_plain("", &stringify(&j("{ '[x][y][x][z]': { 'a': 1 } }")));

        assert_eq!(
            stringify_overrides(&test_object.configuration_model().overrides()),
            stringify(&arr(vec![obj(vec![("identifiers", dyn_strs(&["x", "y", "z"])), ("keys", dyn_strs(&["a"])), ("contents", j("{ 'a': 1 }"))])]))
        );
    }

    #[test]
    fn parse_configuration_model_with_exclude_option() {
        let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
        suite_setup();
        let test_object = parse_with(&stringify(&j("{ 'a': 1, 'b': 2 }")), &ConfigurationParseOptions { exclude: Some(strs(&["a"])), ..options() });

        assert!(test_object.configuration_model().get_value("a").is_undefined());
        assert_eq!(test_object.configuration_model().get_value("b"), n(2.0));
    }

    #[test]
    fn parse_configuration_model_with_exclude_option_even_included() {
        let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
        suite_setup();
        let test_object = parse_with(
            &stringify(&j("{ 'a': 1, 'b': 2 }")),
            &ConfigurationParseOptions { exclude: Some(strs(&["a"])), include: Some(strs(&["a"])), ..options() },
        );

        assert!(test_object.configuration_model().get_value("a").is_undefined());
        assert_eq!(test_object.configuration_model().get_value("b"), n(2.0));
    }

    #[test]
    fn parse_configuration_model_with_scopes_filter() {
        let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
        suite_setup();
        let test_object = parse_with(
            &stringify(&j("{ 'ConfigurationModelParserTest.windowSetting': '1' }")),
            &ConfigurationParseOptions { scopes: Some(vec![ConfigurationScope::Application]), ..options() },
        );

        assert!(test_object.configuration_model().get_value("ConfigurationModelParserTest.windowSetting").is_undefined());
    }

    #[test]
    fn parse_configuration_model_with_include_option() {
        let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
        suite_setup();
        let test_object = parse_with(
            &stringify(&j("{ 'ConfigurationModelParserTest.windowSetting': '1' }")),
            &ConfigurationParseOptions {
                include: Some(strs(&["ConfigurationModelParserTest.windowSetting"])),
                scopes: Some(vec![ConfigurationScope::Application]),
                ..options()
            },
        );

        assert_eq!(test_object.configuration_model().get_value("ConfigurationModelParserTest.windowSetting"), s("1"));
    }

    #[test]
    fn parse_configuration_model_with_invalid_setting_key() {
        let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
        suite_setup();
        let test_object = parse_plain("", &stringify(&j("{ 'a': null, 'a.b.c': { 'c': 1 } }")));

        assert_eq!(test_object.configuration_model().get_value("a"), Dynamic::Null);
        assert!(test_object.configuration_model().get_value("a.b").is_undefined());
        assert!(test_object.configuration_model().get_value("a.b.c").is_undefined());
    }

    #[test]
    fn parse_configuration_model_with_proto_key_does_not_cause_prototype_pollution() {
        let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
        suite_setup();
        let mut test_object = parser("");

        test_object.parse(Some("{ \"__proto__\": { \"editor.fontSize\": 100 }, \"excluded\": true }"), Some(&ConfigurationParseOptions { exclude: Some(strs(&["excluded"])), ..options() }));

        let raw = test_object.configuration_model().get_value("__proto__");
        // `Object.getPrototypeOf(raw) === Object.prototype` e `Object.prototype['editor.fontSize']` não têm
        // equivalente sem cadeia de protótipos: a chave é só uma propriedade própria.
        assert_deep_strict_equal(
            &obj(vec![
                ("fontSize", raw.get_str("editor.fontSize")),
                ("promotedFontSize", test_object.configuration_model().get_value("editor.fontSize")),
            ]),
            &obj(vec![("fontSize", n(100.0)), ("promotedFontSize", U)]),
        );
    }

    #[test]
    fn parse_configuration_model_ignores_inherited_configuration_schema_fields() {
        let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
        suite_setup();
        let mut test_object = parser("");

        test_object.parse(Some("{ \"constructor\": true, \"toString\": true }"), Some(&ConfigurationParseOptions { skip_unregistered: true, ..options() }));

        assert_deep_strict_equal(
            &obj(vec![
                ("constructor", test_object.configuration_model().get_value("constructor")),
                ("toString", test_object.configuration_model().get_value("toString")),
            ]),
            &obj(vec![("constructor", U), ("toString", U)]),
        );
    }
}

mod configuration_model_parser_excluded_properties {
    use super::*;

    fn register_test_configuration() -> ConfigurationNode {
        let node = node(
            "ExcludedPropertiesTest",
            vec![
                ("regularProperty", prop("{ 'type': 'string', 'default': 'regular', 'restricted': false }")),
                ("restrictedProperty", prop("{ 'type': 'string', 'default': 'restricted', 'restricted': true }")),
                ("excludedProperty", prop("{ 'type': 'string', 'default': 'excluded', 'restricted': true, 'included': false }")),
                ("excludedNonRestrictedProperty", prop("{ 'type': 'string', 'default': 'excludedNonRestricted', 'restricted': false, 'included': false }")),
            ],
        );
        configuration_registry().register_configuration(node.clone());
        node
    }

    fn restricted_contains(parser: &ConfigurationModelParser, key: &str) -> bool {
        parser.restricted_configurations().contains(&jstr(key))
    }

    #[test]
    fn should_handle_excluded_restricted_properties_correctly() {
        let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
        register_test_configuration();

        let test_data = j("{ 'regularProperty': 'regularValue', 'restrictedProperty': 'restrictedValue', 'excludedProperty': 'excludedValue', 'excludedNonRestrictedProperty': 'excludedNonRestrictedValue' }");
        let test_object = parse_with(&stringify(&test_data), &ConfigurationParseOptions { skip_restricted: true, ..options() });

        let model = test_object.configuration_model();
        assert_eq!(model.get_value("regularProperty"), s("regularValue"));
        assert!(model.get_value("restrictedProperty").is_undefined());
        assert_eq!(model.get_value("excludedProperty"), s("excludedValue"));
        assert_eq!(model.get_value("excludedNonRestrictedProperty"), s("excludedNonRestrictedValue"));
        assert!(restricted_contains(&test_object, "restrictedProperty"));
        assert!(!restricted_contains(&test_object, "excludedProperty"));
    }

    #[test]
    fn should_find_excluded_properties_when_checking_for_restricted_settings() {
        let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
        register_test_configuration();

        let test_object = parse_plain("test", &stringify(&j("{ 'excludedProperty': 'excludedValue' }")));

        assert_eq!(test_object.configuration_model().get_value("excludedProperty"), s("excludedValue"));
        assert!(!restricted_contains(&test_object, "excludedProperty"));
    }

    #[test]
    fn should_handle_override_properties_with_excluded_configurations() {
        let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
        register_test_configuration();

        let test_data = j("{ '[typescript]': { 'regularProperty': 'overrideRegular', 'restrictedProperty': 'overrideRestricted', 'excludedProperty': 'overrideExcluded' } }");
        let test_object = parse_with(&stringify(&test_data), &ConfigurationParseOptions { skip_restricted: true, ..options() });

        let override_config = test_object.configuration_model().override_for("typescript");
        assert_eq!(override_config.get_value("regularProperty"), s("overrideRegular"));
        assert!(override_config.get_value("restrictedProperty").is_undefined());
        assert_eq!(override_config.get_value("excludedProperty"), s("overrideExcluded"));
    }

    #[test]
    fn should_handle_scope_filtering_with_excluded_properties() {
        let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
        let scope = |scope: ConfigurationScope| vec![("scope", scope.to_dynamic())];
        let node = node(
            "ScopeExcludedTest",
            vec![
                ("windowProperty", prop_with("{ 'type': 'string', 'default': 'window' }", scope(ConfigurationScope::Window))),
                ("applicationProperty", prop_with("{ 'type': 'string', 'default': 'application' }", scope(ConfigurationScope::Application))),
                (
                    "excludedApplicationProperty",
                    prop_with("{ 'type': 'string', 'default': 'excludedApplication', 'included': false }", scope(ConfigurationScope::Application)),
                ),
            ],
        );
        configuration_registry().register_configuration(node);

        let test_data = j("{ 'windowProperty': 'windowValue', 'applicationProperty': 'applicationValue', 'excludedApplicationProperty': 'excludedApplicationValue' }");
        let test_object = parse_with(&stringify(&test_data), &ConfigurationParseOptions { scopes: Some(vec![ConfigurationScope::Window]), ..options() });

        let model = test_object.configuration_model();
        assert_eq!(model.get_value("windowProperty"), s("windowValue"));
        assert!(model.get_value("applicationProperty").is_undefined());
        assert!(model.get_value("excludedApplicationProperty").is_undefined());
    }

    #[test]
    fn filter_should_handle_include_exclude_options_with_excluded_properties() {
        let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
        register_test_configuration();

        let test_data = j("{ 'regularProperty': 'regularValue', 'excludedProperty': 'excludedValue' }");
        let test_object = parse_with(&stringify(&test_data), &ConfigurationParseOptions { include: Some(strs(&["excludedProperty"])), ..options() });

        let model = test_object.configuration_model();
        assert_eq!(model.get_value("regularProperty"), s("regularValue"));
        assert_eq!(model.get_value("excludedProperty"), s("excludedValue"));
    }

    #[test]
    fn should_handle_exclude_options_with_excluded_properties() {
        let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
        register_test_configuration();

        let test_data = j("{ 'regularProperty': 'regularValue', 'excludedProperty': 'excludedValue' }");
        let test_object = parse_with(&stringify(&test_data), &ConfigurationParseOptions { exclude: Some(strs(&["regularProperty"])), ..options() });

        let model = test_object.configuration_model();
        assert!(model.get_value("regularProperty").is_undefined());
        assert_eq!(model.get_value("excludedProperty"), s("excludedValue"));
    }

    #[test]
    fn should_report_has_excluded_properties_correctly_when_excluded_properties_are_filtered() {
        let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
        register_test_configuration();

        let test_data = j("{ 'regularProperty': 'regularValue', 'restrictedProperty': 'restrictedValue', 'excludedProperty': 'excludedValue' }");
        let test_object = parse_with(&stringify(&test_data), &ConfigurationParseOptions { skip_restricted: true, ..options() });

        let model = test_object.configuration_model();

        assert!(model.raw().is_some(), "Raw should be set when properties are excluded");
    }

    #[test]
    fn skip_unregistered_should_exclude_unregistered_properties() {
        let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
        register_test_configuration();

        let test_object = parse_with(&stringify(&j("{ 'unregisteredProperty': 'value3' }")), &ConfigurationParseOptions { skip_unregistered: true, ..options() });

        assert!(test_object.configuration_model().get_value("unregisteredProperty").is_undefined());
    }

    #[test]
    fn should_include_method_works_correctly_with_excluded_properties_for_skip_unregistered() {
        let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
        register_test_configuration();

        let test_object = parse_with(
            &stringify(&j("{ 'regularProperty': 'value1', 'excludedProperty': 'value2', 'unregisteredProperty': 'value3' }")),
            &ConfigurationParseOptions { skip_unregistered: true, ..options() },
        );

        let model = test_object.configuration_model();
        assert_eq!(model.get_value("regularProperty"), s("value1"));
        assert!(model.get_value("excludedProperty").is_undefined());
        assert!(model.get_value("unregisteredProperty").is_undefined());
    }

    #[test]
    fn excluded_properties_are_found_during_property_schema_lookup() {
        let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
        register_test_configuration();

        let registry = configuration_registry();

        let excluded_properties = registry.get_excluded_configuration_properties();
        assert!(excluded_properties.get("excludedProperty").is_some(), "Excluded property should be in excluded properties map");
        assert!(excluded_properties.get("excludedNonRestrictedProperty").is_some(), "Excluded non-restricted property should be in excluded properties map");

        let regular_properties = registry.get_configuration_properties();
        assert!(regular_properties.get("excludedProperty").is_none(), "Excluded property should not be in regular properties map");
        assert!(regular_properties.get("excludedNonRestrictedProperty").is_none(), "Excluded non-restricted property should not be in regular properties map");

        assert!(regular_properties.get("regularProperty").is_some(), "Regular property should be in regular properties map");
        assert!(regular_properties.get("restrictedProperty").is_some(), "Restricted property should be in regular properties map");
    }

    #[test]
    fn should_correctly_use_should_include_with_excluded_properties_for_scope_and_unregistered_filtering() {
        let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
        register_test_configuration();

        let test_data = j(
            "{ 'regularProperty': 'regularValue', 'restrictedProperty': 'restrictedValue', 'excludedProperty': 'excludedValue', 'excludedNonRestrictedProperty': 'excludedNonRestrictedValue', 'unknownProperty': 'unknownValue' }",
        );
        let test_object = parse_with(&stringify(&test_data), &ConfigurationParseOptions { skip_restricted: true, ..options() });

        let model = test_object.configuration_model();
        assert_eq!(model.get_value("regularProperty"), s("regularValue"));
        assert!(model.get_value("restrictedProperty").is_undefined());
        assert!(restricted_contains(&test_object, "restrictedProperty"));
        assert_eq!(model.get_value("excludedProperty"), s("excludedValue"));
        assert!(!restricted_contains(&test_object, "excludedProperty"));
        assert_eq!(model.get_value("excludedNonRestrictedProperty"), s("excludedNonRestrictedValue"));
        assert_eq!(model.get_value("unknownProperty"), s("unknownValue"));
    }
}

mod configuration_model_parser_nested_restricted_properties {
    use super::*;

    fn register_terminal_like_configuration() -> ConfigurationNode {
        let node = node(
            "NestedRestrictedTest",
            vec![
                ("terminal.integrated.allowInUntrustedWorkspace", prop("{ 'type': 'boolean', 'default': false, 'restricted': true }")),
                ("terminal.integrated.defaultProfile.windows", prop("{ 'type': ['string', 'null'], 'default': null, 'restricted': true }")),
                ("terminal.integrated.profiles.windows", prop("{ 'type': 'object', 'default': {}, 'restricted': true }")),
                ("terminal.integrated.fontSize", prop_with("{ 'type': 'number', 'default': 12 }", vec![("scope", ConfigurationScope::Machine.to_dynamic())])),
                ("terminal.integrated.cwd", prop("{ 'type': 'string', 'default': '' }")),
            ],
        );
        configuration_registry().register_configuration(node.clone());
        node
    }

    #[test]
    fn nested_restricted_settings_are_removed_with_skip_restricted_sibling_remains() {
        let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
        register_terminal_like_configuration();

        let test_data = j(
            "{ 'terminal': { 'integrated': { 'allowInUntrustedWorkspace': true, 'profiles': { 'windows': { 'auto-poc': { 'path': 'powershell.exe', 'args': ['-Command', 'Start-Process calc.exe'] } } }, 'defaultProfile': { 'windows': 'auto-poc' }, 'cwd': '/safe' } } }",
        );
        let test_object = parse_with(&stringify(&test_data), &ConfigurationParseOptions { skip_restricted: true, ..options() });

        let model = test_object.configuration_model();
        assert!(model.get_value("terminal.integrated.allowInUntrustedWorkspace").is_undefined());
        assert!(model.get_value("terminal.integrated.profiles.windows").is_undefined());
        assert!(model.get_value("terminal.integrated.defaultProfile.windows").is_undefined());
        assert_eq!(model.get_value("terminal.integrated.cwd"), s("/safe"));
        assert_eq!(
            sorted_set(test_object.restricted_configurations()),
            sorted_set(&strs(&[
                "terminal.integrated.allowInUntrustedWorkspace",
                "terminal.integrated.profiles.windows",
                "terminal.integrated.defaultProfile.windows",
            ]))
        );
    }

    #[test]
    fn nested_restricted_settings_remain_when_trusted_no_skip_restricted() {
        let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
        register_terminal_like_configuration();

        let test_data = j("{ 'terminal': { 'integrated': { 'allowInUntrustedWorkspace': true, 'cwd': '/safe' } } }");
        let test_object = parse_with(&stringify(&test_data), &ConfigurationParseOptions { skip_restricted: false, scopes: Some(vec![ConfigurationScope::Window]), ..options() });

        let model = test_object.configuration_model();
        assert_eq!(model.get_value("terminal.integrated.allowInUntrustedWorkspace"), Dynamic::Bool(true));
        assert_eq!(model.get_value("terminal.integrated.cwd"), s("/safe"));
        assert!(restricted_contains(&test_object, "terminal.integrated.allowInUntrustedWorkspace"));
    }

    fn restricted_contains(parser: &ConfigurationModelParser, key: &str) -> bool {
        parser.restricted_configurations().contains(&jstr(key))
    }

    #[test]
    fn dotted_and_nested_forms_produce_identical_filtered_results() {
        let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
        register_terminal_like_configuration();

        let dotted_object = parse_with(
            &stringify(&j("{ 'terminal.integrated.allowInUntrustedWorkspace': true, 'terminal.integrated.cwd': '/safe' }")),
            &ConfigurationParseOptions { skip_restricted: true, ..options() },
        );

        let nested_object = parse_with(
            &stringify(&j("{ 'terminal': { 'integrated': { 'allowInUntrustedWorkspace': true, 'cwd': '/safe' } } }")),
            &ConfigurationParseOptions { skip_restricted: true, ..options() },
        );

        assert_eq!(stringify(&dotted_object.configuration_model().contents()), stringify(&nested_object.configuration_model().contents()));
        assert_eq!(sorted_set(dotted_object.restricted_configurations()), sorted_set(nested_object.restricted_configurations()));
    }

    #[test]
    fn nested_machine_scoped_settings_respect_scope_filtering() {
        let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
        register_terminal_like_configuration();

        let test_data = j("{ 'terminal': { 'integrated': { 'fontSize': 20, 'cwd': '/safe' } } }");
        let test_object = parse_with(&stringify(&test_data), &ConfigurationParseOptions { scopes: Some(vec![ConfigurationScope::Window]), ..options() });

        let model = test_object.configuration_model();
        assert!(model.get_value("terminal.integrated.fontSize").is_undefined());
        assert_eq!(model.get_value("terminal.integrated.cwd"), s("/safe"));
    }

    #[test]
    fn nested_restricted_settings_inside_language_override_blocks_are_filtered() {
        let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
        register_terminal_like_configuration();

        let test_data = j("{ '[typescript]': { 'terminal': { 'integrated': { 'allowInUntrustedWorkspace': true, 'cwd': '/safe' } } } }");
        let test_object = parse_with(&stringify(&test_data), &ConfigurationParseOptions { skip_restricted: true, ..options() });

        let override_config = test_object.configuration_model().override_for("typescript");
        assert!(override_config.get_value("terminal.integrated.allowInUntrustedWorkspace").is_undefined());
        assert_eq!(override_config.get_value("terminal.integrated.cwd"), s("/safe"));
    }

    #[test]
    fn partial_nested_exclusion_reports_has_excluded_properties_and_preserves_unfiltered_raw() {
        let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
        register_terminal_like_configuration();

        let test_data = j("{ 'terminal': { 'integrated': { 'allowInUntrustedWorkspace': true, 'cwd': '/safe' } } }");
        let test_object = parse_with(&stringify(&test_data), &ConfigurationParseOptions { skip_restricted: true, ..options() });

        let model = test_object.configuration_model();
        // The restricted nested setting is filtered out while a sibling remains, so the model must
        // still flag that raw was filtered and retain the unfiltered raw for trust re-evaluation.
        assert!(model.raw().is_some());
        assert!(model.get_value("terminal.integrated.allowInUntrustedWorkspace").is_undefined());
        assert_eq!(model.raw_configuration().get_value("terminal.integrated.allowInUntrustedWorkspace"), Dynamic::Bool(true));
    }

    #[test]
    fn nested_object_whose_parent_key_is_explicitly_excluded_is_dropped_entirely() {
        let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
        register_terminal_like_configuration();

        let test_data = j("{ 'terminal': { 'integrated': { 'cwd': '/safe' } } }");
        let test_object = parse_with(&stringify(&test_data), &ConfigurationParseOptions { exclude: Some(strs(&["terminal"])), ..options() });

        let model = test_object.configuration_model();
        assert!(model.get_value("terminal.integrated.cwd").is_undefined());
        assert!(model.raw().is_some());
    }
}

mod configuration_model {
    use super::*;

    fn keys_of(model: &ConfigurationModel) -> Vec<je_base::JsString> {
        model.keys()
    }

    #[test]
    fn set_value_for_a_key_that_has_no_sections_and_not_defined() {
        let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
        let test_object = model(j("{ 'a': { 'b': 1 } }"), &["a.b"], vec![], None);

        test_object.set_value("f", n(1.0));

        assert_deep_strict_equal(&test_object.contents(), &j("{ 'a': { 'b': 1 }, 'f': 1 }"));
        assert_eq!(keys_of(&test_object), strs(&["a.b", "f"]));
    }

    #[test]
    fn set_value_for_a_key_that_has_no_sections_and_defined() {
        let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
        let test_object = model(j("{ 'a': { 'b': 1 }, 'f': 1 }"), &["a.b", "f"], vec![], None);

        test_object.set_value("f", n(3.0));

        assert_deep_strict_equal(&test_object.contents(), &j("{ 'a': { 'b': 1 }, 'f': 3 }"));
        assert_eq!(keys_of(&test_object), strs(&["a.b", "f"]));
    }

    #[test]
    fn set_value_for_a_key_that_has_sections_and_not_defined() {
        let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
        let test_object = model(j("{ 'a': { 'b': 1 }, 'f': 1 }"), &["a.b", "f"], vec![], None);

        test_object.set_value("b.c", n(1.0));

        // O original monta `expected.b` com `Object.create(null)`, porque o valor da árvore é assim.
        assert_deep_strict_equal(&test_object.contents(), &j("{ 'a': { 'b': 1 }, 'f': 1, 'b': { 'c': 1 } }"));
        assert_eq!(keys_of(&test_object), strs(&["a.b", "f", "b.c"]));
    }

    #[test]
    fn set_value_for_a_key_that_has_sections_and_defined() {
        let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
        let test_object = model(j("{ 'a': { 'b': 1 }, 'b': { 'c': 1 }, 'f': 1 }"), &["a.b", "b.c", "f"], vec![], None);

        test_object.set_value("b.c", n(3.0));

        assert_deep_strict_equal(&test_object.contents(), &j("{ 'a': { 'b': 1 }, 'b': { 'c': 3 }, 'f': 1 }"));
        assert_eq!(keys_of(&test_object), strs(&["a.b", "b.c", "f"]));
    }

    #[test]
    fn set_value_for_a_key_that_has_sections_and_sub_section_not_defined() {
        let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
        let test_object = model(j("{ 'a': { 'b': 1 }, 'f': 1 }"), &["a.b", "f"], vec![], None);

        test_object.set_value("a.c", n(1.0));

        assert_deep_strict_equal(&test_object.contents(), &j("{ 'a': { 'b': 1, 'c': 1 }, 'f': 1 }"));
        assert_eq!(keys_of(&test_object), strs(&["a.b", "f", "a.c"]));
    }

    #[test]
    fn set_value_for_a_key_that_has_sections_and_sub_section_defined() {
        let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
        let test_object = model(j("{ 'a': { 'b': 1, 'c': 1 }, 'f': 1 }"), &["a.b", "a.c", "f"], vec![], None);

        test_object.set_value("a.c", n(3.0));

        assert_deep_strict_equal(&test_object.contents(), &j("{ 'a': { 'b': 1, 'c': 3 }, 'f': 1 }"));
        assert_eq!(keys_of(&test_object), strs(&["a.b", "a.c", "f"]));
    }

    #[test]
    fn set_value_for_a_key_that_has_sections_and_last_section_is_added() {
        let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
        let test_object = model(j("{ 'a': { 'b': {} }, 'f': 1 }"), &["a.b", "f"], vec![], None);

        test_object.set_value("a.b.c", n(1.0));

        assert_deep_strict_equal(&test_object.contents(), &j("{ 'a': { 'b': { 'c': 1 } }, 'f': 1 }"));
        assert_eq!(keys_of(&test_object), strs(&["a.b", "f", "a.b.c"]));
    }

    #[test]
    fn remove_value_remove_a_non_existing_key() {
        let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
        let test_object = model(j("{ 'a': { 'b': 2 } }"), &["a.b"], vec![], None);

        test_object.remove_value("a.b.c");

        assert_deep_strict_equal(&test_object.contents(), &j("{ 'a': { 'b': 2 } }"));
        assert_eq!(keys_of(&test_object), strs(&["a.b"]));
    }

    #[test]
    fn remove_value_remove_a_single_segmented_key() {
        let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
        let test_object = model(j("{ 'a': 1 }"), &["a"], vec![], None);

        test_object.remove_value("a");

        assert_deep_strict_equal(&test_object.contents(), &j("{}"));
        assert_eq!(keys_of(&test_object), strs(&[]));
    }

    #[test]
    fn remove_value_remove_a_multi_segmented_key() {
        let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
        let test_object = model(j("{ 'a': { 'b': 1 } }"), &["a.b"], vec![], None);

        test_object.remove_value("a.b");

        assert_deep_strict_equal(&test_object.contents(), &j("{}"));
        assert_eq!(keys_of(&test_object), strs(&[]));
    }

    #[test]
    fn get_overriding_configuration_model_for_an_existing_identifier() {
        let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
        let test_object = model(j("{ 'a': { 'b': 1 }, 'f': 1 }"), &[], vec![ov(&["c"], j("{ 'a': { 'd': 1 } }"), &["a"])], Some(Raw::List(vec![])));

        assert_deep_strict_equal(&test_object.override_for("c").contents(), &j("{ 'a': { 'b': 1, 'd': 1 }, 'f': 1 }"));
    }

    #[test]
    fn get_overriding_configuration_model_for_an_identifier_that_does_not_exist() {
        let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
        let test_object = model(j("{ 'a': { 'b': 1 }, 'f': 1 }"), &[], vec![ov(&["c"], j("{ 'a': { 'd': 1 } }"), &["a"])], Some(Raw::List(vec![])));

        assert_deep_strict_equal(&test_object.override_for("xyz").contents(), &j("{ 'a': { 'b': 1 }, 'f': 1 }"));
    }

    #[test]
    fn get_overriding_configuration_when_one_of_the_keys_does_not_exist_in_base() {
        let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
        let test_object = model(j("{ 'a': { 'b': 1 }, 'f': 1 }"), &[], vec![ov(&["c"], j("{ 'a': { 'd': 1 }, 'g': 1 }"), &["a", "g"])], Some(Raw::List(vec![])));

        assert_deep_strict_equal(&test_object.override_for("c").contents(), &j("{ 'a': { 'b': 1, 'd': 1 }, 'f': 1, 'g': 1 }"));
    }

    #[test]
    fn get_overriding_configuration_when_one_of_the_key_in_base_is_not_of_object_type() {
        let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
        let test_object = model(j("{ 'a': { 'b': 1 }, 'f': 1 }"), &[], vec![ov(&["c"], j("{ 'a': { 'd': 1 }, 'f': { 'g': 1 } }"), &["a", "f"])], Some(Raw::List(vec![])));

        assert_deep_strict_equal(&test_object.override_for("c").contents(), &j("{ 'a': { 'b': 1, 'd': 1 }, 'f': { 'g': 1 } }"));
    }

    #[test]
    fn get_overriding_configuration_when_one_of_the_key_in_overriding_contents_is_not_of_object_type() {
        let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
        let test_object = model(j("{ 'a': { 'b': 1 }, 'f': { 'g': 1 } }"), &[], vec![ov(&["c"], j("{ 'a': { 'd': 1 }, 'f': 1 }"), &["a", "f"])], Some(Raw::List(vec![])));

        assert_deep_strict_equal(&test_object.override_for("c").contents(), &j("{ 'a': { 'b': 1, 'd': 1 }, 'f': 1 }"));
    }

    #[test]
    fn get_overriding_configuration_if_the_value_of_overriding_identifier_is_not_object() {
        let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
        let test_object = model(j("{ 'a': { 'b': 1 }, 'f': { 'g': 1 } }"), &[], vec![ov(&["c"], s("abc"), &[])], Some(Raw::List(vec![])));

        assert_deep_strict_equal(&test_object.override_for("c").contents(), &j("{ 'a': { 'b': 1 }, 'f': { 'g': 1 } }"));
    }

    #[test]
    fn get_overriding_configuration_if_the_value_of_overriding_identifier_is_an_empty_object() {
        let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
        let test_object = model(j("{ 'a': { 'b': 1 }, 'f': { 'g': 1 } }"), &[], vec![ov(&["c"], j("{}"), &[])], Some(Raw::List(vec![])));

        assert_deep_strict_equal(&test_object.override_for("c").contents(), &j("{ 'a': { 'b': 1 }, 'f': { 'g': 1 } }"));
    }

    #[test]
    fn simple_merge() {
        let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
        let base = model(j("{ 'a': 1, 'b': 2 }"), &["a", "b"], vec![], None);
        let add = model(j("{ 'a': 3, 'c': 4 }"), &["a", "c"], vec![], None);
        let result = base.merge(&[add]);

        assert_deep_strict_equal(&result.contents(), &j("{ 'a': 3, 'b': 2, 'c': 4 }"));
        assert_eq!(result.keys(), strs(&["a", "b", "c"]));
    }

    #[test]
    fn recursive_merge() {
        let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
        let base = model(j("{ 'a': { 'b': 1 } }"), &["a.b"], vec![], None);
        let add = model(j("{ 'a': { 'b': 2 } }"), &["a.b"], vec![], None);
        let result = base.merge(&[add]);

        assert_deep_strict_equal(&result.contents(), &j("{ 'a': { 'b': 2 } }"));
        assert_deep_strict_equal(&result.get_value("a"), &j("{ 'b': 2 }"));
        assert_eq!(result.keys(), strs(&["a.b"]));
    }

    #[test]
    fn simple_merge_overrides() {
        let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
        let base = model(j("{ 'a': { 'b': 1 } }"), &["a.b"], vec![ov(&["c"], j("{ 'a': 2 }"), &["a"])], None);
        let add = model(j("{ 'a': { 'b': 2 } }"), &["a.b"], vec![ov(&["c"], j("{ 'b': 2 }"), &["b"])], None);
        let result = base.merge(&[add]);

        assert_deep_strict_equal(&result.contents(), &j("{ 'a': { 'b': 2 } }"));
        assert_deep_strict_equal(&arr(result.overrides().iter().map(|o| o.to_dynamic()).collect()), &arr(vec![ov(&["c"], j("{ 'a': 2, 'b': 2 }"), &["a", "b"]).to_dynamic()]));
        assert_deep_strict_equal(&result.override_for("c").contents(), &j("{ 'a': 2, 'b': 2 }"));
        assert_eq!(result.keys(), strs(&["a.b"]));
    }

    #[test]
    fn recursive_merge_overrides() {
        let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
        let base = model(j("{ 'a': { 'b': 1 }, 'f': 1 }"), &["a.b", "f"], vec![ov(&["c"], j("{ 'a': { 'd': 1 } }"), &["a"])], None);
        let add = model(j("{ 'a': { 'b': 2 } }"), &["a.b"], vec![ov(&["c"], j("{ 'a': { 'e': 2 } }"), &["a"])], None);
        let result = base.merge(&[add]);

        assert_deep_strict_equal(&result.contents(), &j("{ 'a': { 'b': 2 }, 'f': 1 }"));
        assert_deep_strict_equal(&arr(result.overrides().iter().map(|o| o.to_dynamic()).collect()), &arr(vec![ov(&["c"], j("{ 'a': { 'd': 1, 'e': 2 } }"), &["a"]).to_dynamic()]));
        assert_deep_strict_equal(&result.override_for("c").contents(), &j("{ 'a': { 'b': 2, 'd': 1, 'e': 2 }, 'f': 1 }"));
        assert_eq!(result.keys(), strs(&["a.b", "f"]));
    }

    #[test]
    fn test_contents_while_getting_an_existing_property() {
        let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
        let test_object = model(j("{ 'a': 1 }"), &[], vec![], None);
        assert_deep_strict_equal(&test_object.get_value("a"), &n(1.0));

        let test_object = model(j("{ 'a': { 'b': 1 } }"), &[], vec![], None);
        assert_deep_strict_equal(&test_object.get_value("a"), &j("{ 'b': 1 }"));
    }

    #[test]
    fn test_contents_are_undefined_for_non_existing_properties() {
        let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
        let test_object = model(j("{ 'awesome': true }"), &[], vec![], None);

        assert_deep_strict_equal(&test_object.get_value("unknownproperty"), &U);
    }

    #[test]
    fn test_override_gives_all_content_merged_with_overrides() {
        let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
        let test_object = model(j("{ 'a': 1, 'c': 1 }"), &[], vec![ov(&["b"], j("{ 'a': 2 }"), &["a"])], None);

        assert_deep_strict_equal(&test_object.override_for("b").contents(), &j("{ 'a': 2, 'c': 1 }"));
    }

    #[test]
    fn test_override_when_an_override_has_multiple_identifiers() {
        let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
        let test_object = model(j("{ 'a': 1, 'c': 1 }"), &["a", "c"], vec![ov(&["x", "y"], j("{ 'a': 2 }"), &["a"])], None);

        let mut actual = test_object.override_for("x");
        assert_deep_strict_equal(&actual.contents(), &j("{ 'a': 2, 'c': 1 }"));
        assert_eq!(actual.keys(), strs(&["a", "c"]));
        assert_eq!(test_object.get_keys_for_override_identifier("x"), strs(&["a"]));

        actual = test_object.override_for("y");
        assert_deep_strict_equal(&actual.contents(), &j("{ 'a': 2, 'c': 1 }"));
        assert_eq!(actual.keys(), strs(&["a", "c"]));
        assert_eq!(test_object.get_keys_for_override_identifier("y"), strs(&["a"]));
    }

    #[test]
    fn test_override_when_an_identifier_is_defined_in_multiple_overrides() {
        let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
        let test_object = model(
            j("{ 'a': 1, 'c': 1 }"),
            &["a", "c"],
            vec![ov(&["x"], j("{ 'a': 3, 'b': 1 }"), &["a", "b"]), ov(&["x", "y"], j("{ 'a': 2 }"), &["a"])],
            None,
        );

        let actual = test_object.override_for("x");
        assert_deep_strict_equal(&actual.contents(), &j("{ 'a': 3, 'c': 1, 'b': 1 }"));
        assert_eq!(actual.keys(), strs(&["a", "c"]));

        assert_eq!(test_object.get_keys_for_override_identifier("x"), strs(&["a", "b"]));
    }

    #[test]
    fn test_merge_when_configuration_models_have_multiple_identifiers() {
        let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
        let test_object = model(
            j("{ 'a': 1, 'c': 1 }"),
            &["a", "c"],
            vec![ov(&["y"], j("{ 'c': 1 }"), &["c"]), ov(&["x", "y"], j("{ 'a': 2 }"), &["a"])],
            None,
        );
        let target = model(
            j("{ 'a': 2, 'b': 1 }"),
            &["a", "b"],
            vec![ov(&["x"], j("{ 'a': 3, 'b': 2 }"), &["a", "b"]), ov(&["x", "y"], j("{ 'b': 3 }"), &["b"])],
            None,
        );

        let actual = test_object.merge(&[target]);

        assert_deep_strict_equal(&actual.contents(), &j("{ 'a': 2, 'c': 1, 'b': 1 }"));
        assert_eq!(actual.keys(), strs(&["a", "c", "b"]));
        assert_deep_strict_equal(
            &arr(actual.overrides().iter().map(|o| o.to_dynamic()).collect()),
            &arr(vec![
                ov(&["y"], j("{ 'c': 1 }"), &["c"]).to_dynamic(),
                ov(&["x", "y"], j("{ 'a': 2, 'b': 3 }"), &["a", "b"]).to_dynamic(),
                ov(&["x"], j("{ 'a': 3, 'b': 2 }"), &["a", "b"]).to_dynamic(),
            ]),
        );
    }

    fn inspect_of(model: &ConfigurationModel, section: &str, override_identifier: Option<&str>) -> Dynamic {
        model.inspect(Some(&jstr(section)), override_identifier.map(jstr).as_ref()).to_dynamic()
    }

    #[test]
    fn inspect_when_raw_is_same() {
        let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
        let test_object = model(j("{ 'a': 1, 'c': 1 }"), &["a", "c"], vec![ov(&["x", "y"], j("{ 'a': 2, 'b': 1 }"), &["a"])], None);

        assert_deep_strict_equal(&inspect_of(&test_object, "a", None), &insp(n(1.0), U, n(1.0), ovs(vec![(&["x", "y"], n(2.0))])));
        assert_deep_strict_equal(&inspect_of(&test_object, "a", Some("x")), &insp(n(1.0), n(2.0), n(2.0), ovs(vec![(&["x", "y"], n(2.0))])));
        assert_deep_strict_equal(&inspect_of(&test_object, "b", Some("x")), &insp(U, n(1.0), n(1.0), ovs(vec![(&["x", "y"], n(1.0))])));
        assert_deep_strict_equal(&inspect_of(&test_object, "d", None), &insp(U, U, U, U));
    }

    #[test]
    fn inspect_when_raw_is_not_same() {
        let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
        let test_object = model(
            j("{ 'a': 1, 'c': 1 }"),
            &["a", "c"],
            vec![ov(&["x", "y"], j("{ 'a': 2 }"), &["a"])],
            Some(Raw::Dictionary(j("{ 'a': 1, 'b': 2, 'c': 1, 'd': 3, '[x][y]': { 'a': 2, 'b': 1 } }"))),
        );

        assert_deep_strict_equal(&inspect_of(&test_object, "a", None), &insp(n(1.0), U, n(1.0), ovs(vec![(&["x", "y"], n(2.0))])));
        assert_deep_strict_equal(&inspect_of(&test_object, "a", Some("x")), &insp(n(1.0), n(2.0), n(2.0), ovs(vec![(&["x", "y"], n(2.0))])));
        assert_deep_strict_equal(&inspect_of(&test_object, "b", Some("x")), &insp(n(2.0), n(1.0), n(1.0), ovs(vec![(&["x", "y"], n(1.0))])));
        assert_deep_strict_equal(&inspect_of(&test_object, "d", None), &insp(n(3.0), U, n(3.0), U));
        assert_deep_strict_equal(&inspect_of(&test_object, "e", None), &insp(U, U, U, U));
    }

    #[test]
    fn inspect_in_merged_configuration_when_raw_is_same() {
        let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
        let target1 = model(j("{ 'a': 1 }"), &["a"], vec![ov(&["x", "y"], j("{ 'a': 2 }"), &["a"])], None);
        let target2 = model(j("{ 'b': 3 }"), &["b"], vec![], None);
        let test_object = target1.merge(&[target2]);

        assert_deep_strict_equal(&inspect_of(&test_object, "a", None), &insp(n(1.0), U, n(1.0), ovs(vec![(&["x", "y"], n(2.0))])));
        assert_deep_strict_equal(&inspect_of(&test_object, "a", Some("x")), &insp(n(1.0), n(2.0), n(2.0), ovs(vec![(&["x", "y"], n(2.0))])));
        assert_deep_strict_equal(&inspect_of(&test_object, "b", None), &insp(n(3.0), U, n(3.0), U));
        assert_deep_strict_equal(&inspect_of(&test_object, "b", Some("y")), &insp(n(3.0), U, n(3.0), U));
        assert_deep_strict_equal(&inspect_of(&test_object, "c", None), &insp(U, U, U, U));
    }

    #[test]
    fn inspect_in_merged_configuration_when_raw_is_not_same_for_one_model() {
        let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
        let target1 = model(
            j("{ 'a': 1 }"),
            &["a"],
            vec![ov(&["x", "y"], j("{ 'a': 2 }"), &["a"])],
            Some(Raw::Dictionary(j("{ 'a': 1, 'b': 2, 'c': 3, '[x][y]': { 'a': 2, 'b': 4 } }"))),
        );
        let target2 = model(j("{ 'b': 3 }"), &["b"], vec![], None);
        let test_object = target1.merge(&[target2]);

        assert_deep_strict_equal(&inspect_of(&test_object, "a", None), &insp(n(1.0), U, n(1.0), ovs(vec![(&["x", "y"], n(2.0))])));
        assert_deep_strict_equal(&inspect_of(&test_object, "a", Some("x")), &insp(n(1.0), n(2.0), n(2.0), ovs(vec![(&["x", "y"], n(2.0))])));
        assert_deep_strict_equal(&inspect_of(&test_object, "b", None), &insp(n(3.0), U, n(3.0), ovs(vec![(&["x", "y"], n(4.0))])));
        assert_deep_strict_equal(&inspect_of(&test_object, "b", Some("y")), &insp(n(3.0), n(4.0), n(4.0), ovs(vec![(&["x", "y"], n(4.0))])));
        assert_deep_strict_equal(&inspect_of(&test_object, "c", None), &insp(n(3.0), U, n(3.0), U));
    }

    #[test]
    fn inspect_return_all_overrides() {
        let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
        let test_object = model(
            j("{ 'a': 1, 'c': 1 }"),
            &["a", "c"],
            vec![
                ov(&["x", "y"], j("{ 'a': 2, 'b': 1 }"), &["a", "b"]),
                ov(&["x"], j("{ 'a': 3 }"), &["a"]),
                ov(&["y"], j("{ 'b': 3 }"), &["b"]),
            ],
            None,
        );

        assert_deep_strict_equal(
            &je_platform::configuration::models::overrides_to_dynamic(test_object.inspect(Some(&jstr("a")), None).overrides()),
            &ovs(vec![(&["x", "y"], n(2.0)), (&["x"], n(3.0))]),
        );
    }

    #[test]
    fn inspect_when_no_overrides() {
        let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
        let test_object = model(j("{ 'a': 1, 'c': 1 }"), &["a", "c"], vec![], None);

        assert!(test_object.inspect(Some(&jstr("a")), None).overrides().is_none());
    }
}

mod custom_configuration_model {
    use super::*;

    #[test]
    fn simple_merge_using_models() {
        let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
        let base = parse_plain("base", &stringify(&j("{ 'a': 1, 'b': 2 }")));
        let add = parse_plain("add", &stringify(&j("{ 'a': 3, 'c': 4 }")));

        let result = base.configuration_model().merge(&[add.configuration_model()]);
        assert_deep_strict_equal(&result.contents(), &j("{ 'a': 3, 'b': 2, 'c': 4 }"));
    }

    #[test]
    fn simple_merge_with_an_undefined_contents() {
        let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
        let base = parse_plain("base", &stringify(&j("{ 'a': 1, 'b': 2 }")));
        let add = parser("add");
        let result = base.configuration_model().merge(&[add.configuration_model()]);
        assert_deep_strict_equal(&result.contents(), &j("{ 'a': 1, 'b': 2 }"));

        let base = parser("base");
        let add = parse_plain("add", &stringify(&j("{ 'a': 1, 'b': 2 }")));
        let result = base.configuration_model().merge(&[add.configuration_model()]);
        assert_deep_strict_equal(&result.contents(), &j("{ 'a': 1, 'b': 2 }"));

        let base = parser("base");
        let add = parser("add");
        let result = base.configuration_model().merge(&[add.configuration_model()]);
        assert_deep_strict_equal(&result.contents(), &j("{}"));
    }

    #[test]
    fn recursive_merge_using_config_models() {
        let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
        let base = parse_plain("base", &stringify(&j("{ 'a': { 'b': 1 } }")));
        let add = parse_plain("add", &stringify(&j("{ 'a': { 'b': 2 } }")));
        let result = base.configuration_model().merge(&[add.configuration_model()]);
        assert_deep_strict_equal(&result.contents(), &j("{ 'a': { 'b': 2 } }"));
    }

    #[test]
    fn test_contents_while_getting_an_existing_property() {
        let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
        let mut test_object = parser("test");
        test_object.parse(Some(&stringify(&j("{ 'a': 1 }"))), None);
        assert_deep_strict_equal(&test_object.configuration_model().get_value("a"), &n(1.0));

        test_object.parse(Some(&stringify(&j("{ 'a': { 'b': 1 } }"))), None);
        assert_deep_strict_equal(&test_object.configuration_model().get_value("a"), &j("{ 'b': 1 }"));
    }

    #[test]
    fn test_contents_are_undefined_for_non_existing_properties() {
        let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
        let test_object = parse_plain("test", &stringify(&j("{ 'awesome': true }")));

        assert_deep_strict_equal(&test_object.configuration_model().get_value("unknownproperty"), &U);
    }

    #[test]
    fn test_contents_are_undefined_for_undefined_config() {
        let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
        let test_object = parser("test");

        assert_deep_strict_equal(&test_object.configuration_model().get_value("unknownproperty"), &U);
    }

    #[test]
    fn test_config_with_overrides_gives_all_content_merged_with_overrides() {
        let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
        let test_object = parse_plain("test", &stringify(&j("{ 'a': 1, 'c': 1, '[b]': { 'a': 2 } }")));

        assert_deep_strict_equal(&test_object.configuration_model().override_for("b").contents(), &j("{ 'a': 2, 'c': 1, '[b]': { 'a': 2 } }"));
    }

    #[test]
    fn test_config_with_overrides_gives_empty_contents() {
        let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
        let test_object = parser("test");

        assert_deep_strict_equal(&test_object.configuration_model().override_for("b").contents(), &j("{}"));
    }

    #[test]
    fn test_update_with_empty_data() {
        let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
        let mut test_object = parser("test");
        test_object.parse(Some(""), None);

        assert_deep_strict_equal(&test_object.configuration_model().contents(), &empty_obj());
        assert_eq!(test_object.configuration_model().keys(), strs(&[]));

        test_object.parse(None, None);

        assert_deep_strict_equal(&test_object.configuration_model().contents(), &empty_obj());
        assert_eq!(test_object.configuration_model().keys(), strs(&[]));

        // `parse(undefined)` e `parse(null)` são o mesmo `None`.
        test_object.parse(None, None);

        assert_deep_strict_equal(&test_object.configuration_model().contents(), &empty_obj());
        assert_eq!(test_object.configuration_model().keys(), strs(&[]));
    }

    #[test]
    fn test_empty_property_is_not_ignored() {
        let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
        let test_object = parse_plain("test", &stringify(&obj(vec![("", n(1.0))])));

        // deepStrictEqual seems to ignore empty properties, fall back
        // to comparing the output of JSON.stringify
        assert_eq!(stringify(&test_object.configuration_model().contents()), stringify(&obj(vec![("", n(1.0))])));
        assert_eq!(test_object.configuration_model().keys(), strs(&[""]));
    }
}

mod configuration {
    use super::*;

    fn parse_configuration_model(content: &str) -> ConfigurationModel {
        let mut parser = parser("test");
        parser.parse(Some(&stringify(&j(content))), None);
        parser.configuration_model()
    }

    fn user_overrides() -> ConfigurationOverrides {
        ConfigurationOverrides::default()
    }

    #[test]
    fn test_inspect_for_override_identifiers() {
        let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
        let default_configuration_model = parse_configuration_model("{ '[l1]': { 'a': 1 }, '[l2]': { 'b': 1 } }");
        let user_configuration_model = parse_configuration_model("{ '[l3]': { 'a': 2 } }");
        let workspace_configuration_model = parse_configuration_model("{ '[l1]': { 'a': 3 }, '[l4]': { 'a': 3 } }");
        let test_object = test_configuration(default_configuration_model, empty_model(), user_configuration_model, workspace_configuration_model, None);

        let override_identifiers = test_object.inspect("a", &user_overrides(), None).override_identifiers();

        assert_eq!(override_identifiers, Some(strs(&["l1", "l3", "l4"])));
    }

    #[test]
    fn test_update_value() {
        let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
        let mut parser = parser("test");
        parser.parse(Some(&stringify(&j("{ 'a': 1 }"))), None);
        let test_object = test_configuration(parser.configuration_model(), empty_model(), empty_model(), empty_model(), None);

        test_object.update_value("a", n(2.0), &Default::default());

        assert_eq!(test_object.get_value(Some(&jstr("a")), &user_overrides(), None), n(2.0));
    }

    #[test]
    fn test_update_value_after_inspect() {
        let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
        let mut parser = parser("test");
        parser.parse(Some(&stringify(&j("{ 'a': 1 }"))), None);
        let test_object = test_configuration(parser.configuration_model(), empty_model(), empty_model(), empty_model(), None);

        test_object.inspect("a", &user_overrides(), None);
        test_object.update_value("a", n(2.0), &Default::default());

        assert_eq!(test_object.get_value(Some(&jstr("a")), &user_overrides(), None), n(2.0));
    }

    #[test]
    fn test_compare_and_update_default_configuration() {
        let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
        let test_object = empty_test_configuration();
        test_object.update_default_configuration(to_configuration_model("{ 'editor.lineNumbers': 'on' }"));

        let actual = test_object.compare_and_update_default_configuration(
            to_configuration_model("{ 'editor.lineNumbers': 'off', '[markdown]': { 'editor.wordWrap': 'off' } }"),
            Some(strs(&["editor.lineNumbers", "[markdown]"])),
        );

        assert_eq!(actual, change(&["editor.lineNumbers", "[markdown]"], &[("markdown", &["editor.wordWrap"])]));
    }

    #[test]
    fn test_compare_and_update_application_configuration() {
        let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
        let test_object = empty_test_configuration();
        test_object.update_application_configuration(to_configuration_model("{ 'update.mode': 'on' }"));

        let actual = test_object.compare_and_update_application_configuration(to_configuration_model("{ 'update.mode': 'none', '[typescript]': { 'editor.wordWrap': 'off' } }"));

        assert_eq!(actual, change(&["[typescript]", "update.mode"], &[("typescript", &["editor.wordWrap"])]));
    }

    #[test]
    fn test_compare_and_update_user_configuration() {
        let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
        let test_object = empty_test_configuration();
        test_object.update_local_user_configuration(to_configuration_model("{ 'editor.lineNumbers': 'off', 'editor.fontSize': 12, '[typescript]': { 'editor.wordWrap': 'off' } }"));

        let actual = test_object.compare_and_update_local_user_configuration(to_configuration_model(
            "{ 'editor.lineNumbers': 'on', 'window.zoomLevel': 1, '[typescript]': { 'editor.wordWrap': 'on', 'editor.insertSpaces': false } }",
        ));

        assert_eq!(
            actual,
            change(&["window.zoomLevel", "editor.lineNumbers", "[typescript]", "editor.fontSize"], &[("typescript", &["editor.insertSpaces", "editor.wordWrap"])])
        );
    }

    #[test]
    fn test_compare_and_update_workspace_configuration() {
        let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
        let test_object = empty_test_configuration();
        test_object.update_workspace_configuration(to_configuration_model("{ 'editor.lineNumbers': 'off', 'editor.fontSize': 12, '[typescript]': { 'editor.wordWrap': 'off' } }"));

        let actual = test_object.compare_and_update_workspace_configuration(to_configuration_model(
            "{ 'editor.lineNumbers': 'on', 'window.zoomLevel': 1, '[typescript]': { 'editor.wordWrap': 'on', 'editor.insertSpaces': false } }",
        ));

        assert_eq!(
            actual,
            change(&["window.zoomLevel", "editor.lineNumbers", "[typescript]", "editor.fontSize"], &[("typescript", &["editor.insertSpaces", "editor.wordWrap"])])
        );
    }

    #[test]
    fn test_compare_and_update_workspace_folder_configuration() {
        let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
        let test_object = empty_test_configuration();
        test_object.update_folder_configuration(
            &uri_file("file1"),
            to_configuration_model("{ 'editor.lineNumbers': 'off', 'editor.fontSize': 12, '[typescript]': { 'editor.wordWrap': 'off' } }"),
        );

        let actual = test_object.compare_and_update_folder_configuration(
            &uri_file("file1"),
            to_configuration_model("{ 'editor.lineNumbers': 'on', 'window.zoomLevel': 1, '[typescript]': { 'editor.wordWrap': 'on', 'editor.insertSpaces': false } }"),
        );

        assert_eq!(
            actual,
            change(&["window.zoomLevel", "editor.lineNumbers", "[typescript]", "editor.fontSize"], &[("typescript", &["editor.insertSpaces", "editor.wordWrap"])])
        );
    }

    #[test]
    fn test_compare_and_delete_workspace_folder_configuration() {
        let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
        let test_object = empty_test_configuration();
        test_object.update_folder_configuration(
            &uri_file("file1"),
            to_configuration_model("{ 'editor.lineNumbers': 'off', 'editor.fontSize': 12, '[typescript]': { 'editor.wordWrap': 'off' } }"),
        );

        let actual = test_object.compare_and_delete_folder_configuration(&uri_file("file1"));

        assert_eq!(actual, change(&["editor.lineNumbers", "editor.fontSize", "[typescript]"], &[("typescript", &["editor.wordWrap"])]));
    }
}

mod configuration_change_event {
    use super::*;

    fn resource(path: &str) -> ConfigurationOverrides {
        ConfigurationOverrides::with_resource(uri_file(path))
    }

    fn resource_in(folder: &str, file: &str) -> ConfigurationOverrides {
        ConfigurationOverrides::with_resource(uri_file(&join(&[folder, file])))
    }

    fn identifier(identifier: &str) -> ConfigurationOverrides {
        ConfigurationOverrides::with_identifier(identifier)
    }

    fn resource_and_identifier(path: &str, identifier: &str) -> ConfigurationOverrides {
        ConfigurationOverrides { override_identifier: Some(jstr(identifier)), resource: Some(uri_file(path)) }
    }

    fn event(change: je_platform::configuration::ConfigurationChange, previous: Option<PreviousConfiguration>, configuration: &Rc<Configuration>, workspace: Option<Rc<je_platform::workspace::Workspace>>) -> ConfigurationChangeEvent {
        ConfigurationChangeEvent::new(change, previous, configuration.clone(), workspace, null_log())
    }

    #[test]
    fn change_event_affecting_keys_with_new_configuration() {
        let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
        let configuration = Rc::new(empty_test_configuration());
        let change = configuration.compare_and_update_local_user_configuration(to_configuration_model("{ 'window.zoomLevel': 1, 'workbench.editor.enablePreview': false, 'files.autoSave': 'off' }"));
        let test_object = event(change, None, &configuration, None);

        assert_eq!(test_object.affected_keys, strs(&["window.zoomLevel", "workbench.editor.enablePreview", "files.autoSave"]));

        assert!(test_object.affects_configuration("window.zoomLevel", None));
        assert!(test_object.affects_configuration("window", None));

        assert!(test_object.affects_configuration("workbench.editor.enablePreview", None));
        assert!(test_object.affects_configuration("workbench.editor", None));
        assert!(test_object.affects_configuration("workbench", None));

        assert!(test_object.affects_configuration("files", None));
        assert!(test_object.affects_configuration("files.autoSave", None));
        assert!(!test_object.affects_configuration("files.exclude", None));

        assert!(!test_object.affects_configuration("[markdown]", None));
        assert!(!test_object.affects_configuration("editor", None));
    }

    #[test]
    fn change_event_affecting_keys_when_configuration_changed() {
        let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
        let configuration = Rc::new(empty_test_configuration());
        configuration.update_local_user_configuration(to_configuration_model("{ 'window.zoomLevel': 2, 'workbench.editor.enablePreview': true, 'files.autoSave': 'off' }"));
        let data = configuration.to_data();
        let change = configuration.compare_and_update_local_user_configuration(to_configuration_model("{ 'window.zoomLevel': 1, 'workbench.editor.enablePreview': false, 'files.autoSave': 'off' }"));
        let test_object = event(change, Some(PreviousConfiguration { workspace: None, data }), &configuration, None);

        assert_eq!(test_object.affected_keys, strs(&["window.zoomLevel", "workbench.editor.enablePreview"]));

        assert!(test_object.affects_configuration("window.zoomLevel", None));
        assert!(test_object.affects_configuration("window", None));

        assert!(test_object.affects_configuration("workbench.editor.enablePreview", None));
        assert!(test_object.affects_configuration("workbench.editor", None));
        assert!(test_object.affects_configuration("workbench", None));

        assert!(!test_object.affects_configuration("files", None));
        assert!(!test_object.affects_configuration("[markdown]", None));
        assert!(!test_object.affects_configuration("editor", None));
    }

    #[test]
    fn change_event_affecting_overrides_with_new_configuration() {
        let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
        let configuration = Rc::new(empty_test_configuration());
        let change = configuration.compare_and_update_local_user_configuration(to_configuration_model(
            "{ 'files.autoSave': 'off', '[markdown]': { 'editor.wordWrap': 'off' }, '[typescript][jsonc]': { 'editor.lineNumbers': 'off' } }",
        ));
        let test_object = event(change, None, &configuration, None);

        assert_eq!(test_object.affected_keys, strs(&["files.autoSave", "[markdown]", "[typescript][jsonc]", "editor.wordWrap", "editor.lineNumbers"]));

        assert!(test_object.affects_configuration("files", None));
        assert!(test_object.affects_configuration("files.autoSave", None));
        assert!(!test_object.affects_configuration("files.exclude", None));

        assert!(test_object.affects_configuration("[markdown]", None));
        assert!(!test_object.affects_configuration("[markdown].editor", None));
        assert!(!test_object.affects_configuration("[markdown].workbench", None));

        assert!(test_object.affects_configuration("editor", None));
        assert!(test_object.affects_configuration("editor.wordWrap", None));
        assert!(test_object.affects_configuration("editor.lineNumbers", None));
        assert!(test_object.affects_configuration("editor", Some(&identifier("markdown"))));
        assert!(test_object.affects_configuration("editor", Some(&identifier("jsonc"))));
        assert!(test_object.affects_configuration("editor", Some(&identifier("typescript"))));
        assert!(test_object.affects_configuration("editor.wordWrap", Some(&identifier("markdown"))));
        assert!(!test_object.affects_configuration("editor.wordWrap", Some(&identifier("jsonc"))));
        assert!(!test_object.affects_configuration("editor.wordWrap", Some(&identifier("typescript"))));
        assert!(!test_object.affects_configuration("editor.lineNumbers", Some(&identifier("markdown"))));
        assert!(test_object.affects_configuration("editor.lineNumbers", Some(&identifier("typescript"))));
        assert!(test_object.affects_configuration("editor.lineNumbers", Some(&identifier("jsonc"))));
        assert!(!test_object.affects_configuration("editor", Some(&identifier("json"))));
        assert!(!test_object.affects_configuration("editor.fontSize", Some(&identifier("markdown"))));

        assert!(!test_object.affects_configuration("editor.fontSize", None));
        assert!(!test_object.affects_configuration("window", None));
    }

    #[test]
    fn change_event_affecting_overrides_when_configuration_changed() {
        let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
        let configuration = Rc::new(empty_test_configuration());
        configuration.update_local_user_configuration(to_configuration_model(
            "{ 'workbench.editor.enablePreview': true, '[markdown]': { 'editor.fontSize': 12, 'editor.wordWrap': 'off' }, '[css][scss]': { 'editor.lineNumbers': 'off', 'css.lint.emptyRules': 'error' }, 'files.autoSave': 'off' }",
        ));
        let data = configuration.to_data();
        let change = configuration.compare_and_update_local_user_configuration(to_configuration_model(
            "{ 'files.autoSave': 'off', '[markdown]': { 'editor.fontSize': 13, 'editor.wordWrap': 'off' }, '[css][scss]': { 'editor.lineNumbers': 'relative', 'css.lint.emptyRules': 'error' }, 'window.zoomLevel': 1 }",
        ));
        let test_object = event(change, Some(PreviousConfiguration { workspace: None, data }), &configuration, None);

        assert_eq!(test_object.affected_keys, strs(&["window.zoomLevel", "[markdown]", "[css][scss]", "workbench.editor.enablePreview", "editor.fontSize", "editor.lineNumbers"]));

        assert!(!test_object.affects_configuration("files", None));

        assert!(test_object.affects_configuration("[markdown]", None));
        assert!(!test_object.affects_configuration("[markdown].editor", None));
        assert!(!test_object.affects_configuration("[markdown].editor.fontSize", None));
        assert!(!test_object.affects_configuration("[markdown].editor.wordWrap", None));
        assert!(!test_object.affects_configuration("[markdown].workbench", None));
        assert!(test_object.affects_configuration("[css][scss]", None));

        assert!(test_object.affects_configuration("editor", None));
        assert!(test_object.affects_configuration("editor", Some(&identifier("markdown"))));
        assert!(test_object.affects_configuration("editor", Some(&identifier("css"))));
        assert!(test_object.affects_configuration("editor", Some(&identifier("scss"))));
        assert!(test_object.affects_configuration("editor.fontSize", Some(&identifier("markdown"))));
        assert!(!test_object.affects_configuration("editor.fontSize", Some(&identifier("css"))));
        assert!(!test_object.affects_configuration("editor.fontSize", Some(&identifier("scss"))));
        assert!(test_object.affects_configuration("editor.lineNumbers", Some(&identifier("scss"))));
        assert!(test_object.affects_configuration("editor.lineNumbers", Some(&identifier("css"))));
        assert!(!test_object.affects_configuration("editor.lineNumbers", Some(&identifier("markdown"))));
        assert!(!test_object.affects_configuration("editor.wordWrap", None));
        assert!(!test_object.affects_configuration("editor.wordWrap", Some(&identifier("markdown"))));
        assert!(!test_object.affects_configuration("editor", Some(&identifier("json"))));
        assert!(!test_object.affects_configuration("editor.fontSize", Some(&identifier("json"))));

        assert!(test_object.affects_configuration("window", None));
        assert!(test_object.affects_configuration("window.zoomLevel", None));
        assert!(test_object.affects_configuration("window", Some(&identifier("markdown"))));
        assert!(test_object.affects_configuration("window.zoomLevel", Some(&identifier("markdown"))));

        assert!(test_object.affects_configuration("workbench", None));
        assert!(test_object.affects_configuration("workbench.editor", None));
        assert!(test_object.affects_configuration("workbench.editor.enablePreview", None));
        assert!(test_object.affects_configuration("workbench", Some(&identifier("markdown"))));
        assert!(test_object.affects_configuration("workbench.editor", Some(&identifier("markdown"))));
    }

    #[test]
    fn change_event_affecting_workspace_folders() {
        let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
        let configuration = Rc::new(empty_test_configuration());
        configuration.update_workspace_configuration(to_configuration_model("{ 'window.title': 'custom' }"));
        configuration.update_folder_configuration(&uri_file("folder1"), to_configuration_model("{ 'window.zoomLevel': 2, 'window.restoreFullscreen': true }"));
        configuration.update_folder_configuration(&uri_file("folder2"), to_configuration_model("{ 'workbench.editor.enablePreview': true, 'window.restoreWindows': true }"));
        let data = configuration.to_data();
        let workspace = test_workspace_with_folders(
            "a",
            vec![
                WorkspaceFolder::new(uri_file("folder1"), "a", 0, None),
                WorkspaceFolder::new(uri_file("folder2"), "b", 1, None),
                WorkspaceFolder::new(uri_file("folder3"), "c", 2, None),
            ],
        );
        let change = merge_changes(&[
            configuration.compare_and_update_workspace_configuration(to_configuration_model("{ 'window.title': 'native' }")),
            configuration.compare_and_update_folder_configuration(&uri_file("folder1"), to_configuration_model("{ 'window.zoomLevel': 1, 'window.restoreFullscreen': false }")),
            configuration.compare_and_update_folder_configuration(&uri_file("folder2"), to_configuration_model("{ 'workbench.editor.enablePreview': false, 'window.restoreWindows': false }")),
        ]);
        let test_object = event(change, Some(PreviousConfiguration { workspace: Some(workspace.clone()), data }), &configuration, Some(workspace));

        assert_eq!(
            test_object.affected_keys,
            strs(&["window.title", "window.zoomLevel", "window.restoreFullscreen", "workbench.editor.enablePreview", "window.restoreWindows"])
        );

        let a = |section: &str, overrides: &ConfigurationOverrides| test_object.affects_configuration(section, Some(overrides));

        assert!(test_object.affects_configuration("window.zoomLevel", None));
        assert!(a("window.zoomLevel", &resource("folder1")));
        assert!(a("window.zoomLevel", &resource_in("folder1", "file1")));
        assert!(!a("window.zoomLevel", &resource("file1")));
        assert!(!a("window.zoomLevel", &resource("file2")));
        assert!(!a("window.zoomLevel", &resource_in("folder2", "file2")));
        assert!(!a("window.zoomLevel", &resource_in("folder3", "file3")));

        assert!(test_object.affects_configuration("window.restoreFullscreen", None));
        assert!(a("window.restoreFullscreen", &resource_in("folder1", "file1")));
        assert!(a("window.restoreFullscreen", &resource("folder1")));
        assert!(!a("window.restoreFullscreen", &resource("file1")));
        assert!(!a("window.restoreFullscreen", &resource("file2")));
        assert!(!a("window.restoreFullscreen", &resource_in("folder2", "file2")));
        assert!(!a("window.restoreFullscreen", &resource_in("folder3", "file3")));

        assert!(test_object.affects_configuration("window.restoreWindows", None));
        assert!(a("window.restoreWindows", &resource("folder2")));
        assert!(a("window.restoreWindows", &resource_in("folder2", "file2")));
        assert!(!a("window.restoreWindows", &resource("file2")));
        assert!(!a("window.restoreWindows", &resource_in("folder1", "file1")));
        assert!(!a("window.restoreWindows", &resource_in("folder3", "file3")));

        assert!(test_object.affects_configuration("window.title", None));
        assert!(a("window.title", &resource("folder1")));
        assert!(a("window.title", &resource_in("folder1", "file1")));
        assert!(a("window.title", &resource("folder2")));
        assert!(a("window.title", &resource_in("folder2", "file2")));
        assert!(a("window.title", &resource("folder3")));
        assert!(a("window.title", &resource_in("folder3", "file3")));
        assert!(a("window.title", &resource("file1")));
        assert!(a("window.title", &resource("file2")));
        assert!(a("window.title", &resource("file3")));

        assert!(test_object.affects_configuration("window", None));
        assert!(a("window", &resource("folder1")));
        assert!(a("window", &resource_in("folder1", "file1")));
        assert!(a("window", &resource("folder2")));
        assert!(a("window", &resource_in("folder2", "file2")));
        assert!(a("window", &resource("folder3")));
        assert!(a("window", &resource_in("folder3", "file3")));
        assert!(a("window", &resource("file1")));
        assert!(a("window", &resource("file2")));
        assert!(a("window", &resource("file3")));

        assert!(test_object.affects_configuration("workbench.editor.enablePreview", None));
        assert!(a("workbench.editor.enablePreview", &resource("folder2")));
        assert!(a("workbench.editor.enablePreview", &resource_in("folder2", "file2")));
        assert!(!a("workbench.editor.enablePreview", &resource("folder1")));
        assert!(!a("workbench.editor.enablePreview", &resource_in("folder1", "file1")));
        assert!(!a("workbench.editor.enablePreview", &resource("folder3")));

        assert!(test_object.affects_configuration("workbench.editor", None));
        assert!(a("workbench.editor", &resource("folder2")));
        assert!(a("workbench.editor", &resource_in("folder2", "file2")));
        assert!(!a("workbench.editor", &resource("folder1")));
        assert!(!a("workbench.editor", &resource_in("folder1", "file1")));
        assert!(!a("workbench.editor", &resource("folder3")));

        assert!(test_object.affects_configuration("workbench", None));
        assert!(a("workbench", &resource("folder2")));
        assert!(a("workbench", &resource_in("folder2", "file2")));
        assert!(!a("workbench", &resource("folder1")));
        assert!(!a("workbench", &resource("folder3")));

        assert!(!test_object.affects_configuration("files", None));
        assert!(!a("files", &resource("folder1")));
        assert!(!a("files", &resource_in("folder1", "file1")));
        assert!(!a("files", &resource("folder2")));
        assert!(!a("files", &resource_in("folder2", "file2")));
        assert!(!a("files", &resource("folder3")));
        assert!(!a("files", &resource_in("folder3", "file3")));
    }

    #[test]
    fn change_event_all() {
        let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
        let configuration = Rc::new(empty_test_configuration());
        configuration.update_folder_configuration(&uri_file("file1"), to_configuration_model("{ 'window.zoomLevel': 2, 'window.restoreFullscreen': true }"));
        let data = configuration.to_data();
        let change = merge_changes(&[
            configuration.compare_and_update_default_configuration(
                to_configuration_model("{ 'editor.lineNumbers': 'off', '[markdown]': { 'editor.wordWrap': 'off' } }"),
                Some(strs(&["editor.lineNumbers", "[markdown]"])),
            ),
            configuration.compare_and_update_local_user_configuration(to_configuration_model("{ '[json]': { 'editor.lineNumbers': 'relative' } }")),
            configuration.compare_and_update_workspace_configuration(to_configuration_model("{ 'window.title': 'custom' }")),
            configuration.compare_and_delete_folder_configuration(&uri_file("file1")),
            configuration.compare_and_update_folder_configuration(&uri_file("file2"), to_configuration_model("{ 'workbench.editor.enablePreview': true, 'window.restoreWindows': true }")),
        ]);
        let workspace = test_workspace_with_folders(
            "a",
            vec![
                WorkspaceFolder::new(uri_file("file1"), "a", 0, None),
                WorkspaceFolder::new(uri_file("file2"), "b", 1, None),
                WorkspaceFolder::new(uri_file("folder3"), "c", 2, None),
            ],
        );
        let test_object = event(change, Some(PreviousConfiguration { workspace: Some(workspace.clone()), data }), &configuration, Some(workspace));

        assert_eq!(
            test_object.affected_keys,
            strs(&[
                "editor.lineNumbers",
                "[markdown]",
                "[json]",
                "window.title",
                "window.zoomLevel",
                "window.restoreFullscreen",
                "workbench.editor.enablePreview",
                "window.restoreWindows",
                "editor.wordWrap"
            ])
        );

        let a = |section: &str, overrides: &ConfigurationOverrides| test_object.affects_configuration(section, Some(overrides));

        assert!(test_object.affects_configuration("window.title", None));
        assert!(a("window.title", &resource("file1")));
        assert!(a("window.title", &resource("file2")));

        assert!(test_object.affects_configuration("window", None));
        assert!(a("window", &resource("file1")));
        assert!(a("window", &resource("file2")));

        assert!(test_object.affects_configuration("window.zoomLevel", None));
        assert!(a("window.zoomLevel", &resource("file1")));
        assert!(!a("window.zoomLevel", &resource("file2")));

        assert!(test_object.affects_configuration("window.restoreFullscreen", None));
        assert!(a("window.restoreFullscreen", &resource("file1")));
        assert!(!a("window.restoreFullscreen", &resource("file2")));

        assert!(test_object.affects_configuration("window.restoreWindows", None));
        assert!(a("window.restoreWindows", &resource("file2")));
        assert!(!a("window.restoreWindows", &resource("file1")));

        assert!(test_object.affects_configuration("workbench.editor.enablePreview", None));
        assert!(a("workbench.editor.enablePreview", &resource("file2")));
        assert!(!a("workbench.editor.enablePreview", &resource("file1")));

        assert!(test_object.affects_configuration("workbench.editor", None));
        assert!(a("workbench.editor", &resource("file2")));
        assert!(!a("workbench.editor", &resource("file1")));

        assert!(test_object.affects_configuration("workbench", None));
        assert!(a("workbench", &resource("file2")));
        assert!(!a("workbench", &resource("file1")));

        assert!(!test_object.affects_configuration("files", None));
        assert!(!a("files", &resource("file1")));
        assert!(!a("files", &resource("file2")));

        assert!(test_object.affects_configuration("editor", None));
        assert!(a("editor", &resource("file1")));
        assert!(a("editor", &resource("file2")));
        assert!(a("editor", &resource_and_identifier("file1", "json")));
        assert!(a("editor", &resource_and_identifier("file1", "markdown")));
        assert!(a("editor", &resource_and_identifier("file1", "typescript")));
        assert!(a("editor", &resource_and_identifier("file2", "json")));
        assert!(a("editor", &resource_and_identifier("file2", "markdown")));
        assert!(a("editor", &resource_and_identifier("file2", "typescript")));

        assert!(test_object.affects_configuration("editor.lineNumbers", None));
        assert!(a("editor.lineNumbers", &resource("file1")));
        assert!(a("editor.lineNumbers", &resource("file2")));
        assert!(a("editor.lineNumbers", &resource_and_identifier("file1", "json")));
        assert!(a("editor.lineNumbers", &resource_and_identifier("file1", "markdown")));
        assert!(a("editor.lineNumbers", &resource_and_identifier("file1", "typescript")));
        assert!(a("editor.lineNumbers", &resource_and_identifier("file2", "json")));
        assert!(a("editor.lineNumbers", &resource_and_identifier("file2", "markdown")));
        assert!(a("editor.lineNumbers", &resource_and_identifier("file2", "typescript")));

        assert!(test_object.affects_configuration("editor.wordWrap", None));
        assert!(!a("editor.wordWrap", &resource("file1")));
        assert!(!a("editor.wordWrap", &resource("file2")));
        assert!(!a("editor.wordWrap", &resource_and_identifier("file1", "json")));
        assert!(a("editor.wordWrap", &resource_and_identifier("file1", "markdown")));
        assert!(!a("editor.wordWrap", &resource_and_identifier("file1", "typescript")));
        assert!(!a("editor.wordWrap", &resource_and_identifier("file2", "json")));
        assert!(a("editor.wordWrap", &resource_and_identifier("file2", "markdown")));
        assert!(!a("editor.wordWrap", &resource_and_identifier("file2", "typescript")));

        assert!(!test_object.affects_configuration("editor.fontSize", None));
        assert!(!a("editor.fontSize", &resource("file1")));
        assert!(!a("editor.fontSize", &resource("file2")));
    }

    #[test]
    fn change_event_affecting_tasks_and_launches() {
        let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
        let configuration = Rc::new(empty_test_configuration());
        let change = configuration.compare_and_update_local_user_configuration(to_configuration_model("{ 'launch': { 'configuraiton': {} }, 'launch.version': 1, 'tasks': { 'version': 2 } }"));
        let test_object = event(change, None, &configuration, None);

        assert_eq!(test_object.affected_keys, strs(&["launch", "launch.version", "tasks"]));
        assert!(test_object.affects_configuration("launch", None));
        assert!(test_object.affects_configuration("launch.version", None));
        assert!(test_object.affects_configuration("tasks", None));
    }

    #[test]
    fn affects_configuration_returns_false_for_empty_string() {
        let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
        let configuration = Rc::new(empty_test_configuration());
        let change = configuration.compare_and_update_local_user_configuration(to_configuration_model("{ 'window.zoomLevel': 1 }"));
        let test_object = event(change, None, &configuration, None);

        assert!(!test_object.affects_configuration("", None));
    }
}

mod configuration_parse {
    use super::*;

    fn raw_dictionary(literal: &str) -> Option<Raw> {
        Some(Raw::Dictionary(j(literal)))
    }

    fn parsed(configuration: &Configuration) -> Configuration {
        Configuration::parse(&configuration.to_data(), null_log())
    }

    fn inspect(configuration: &Configuration, key: &str, override_identifier: Option<&str>) -> je_platform::configuration::models::ConfigurationInspectValue {
        let overrides = override_identifier.map_or_else(ConfigurationOverrides::default, ConfigurationOverrides::with_identifier);
        configuration.inspect(key, &overrides, None)
    }

    #[test]
    fn parsing_configuration_only_with_local_user_configuration_and_raw_is_same() {
        let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
        let configuration = test_configuration(
            empty_model(),
            empty_model(),
            empty_model(),
            model(j("{ 'a': 1, 'c': 1 }"), &["a", "c"], vec![ov(&["x", "y"], j("{ 'a': 2, 'b': 1 }"), &["a"])], None),
            None,
        );

        let actual = parsed(&configuration);

        assert_deep_strict_equal(&inspect_dynamic(inspect(&actual, "a", None).user_local()), &insp(n(1.0), U, n(1.0), ovs(vec![(&["x", "y"], n(2.0))])));
        assert_deep_strict_equal(&inspect_dynamic(inspect(&actual, "a", Some("x")).user_local()), &insp(n(1.0), n(2.0), n(2.0), ovs(vec![(&["x", "y"], n(2.0))])));
        assert_deep_strict_equal(&inspect_dynamic(inspect(&actual, "b", Some("x")).user_local()), &insp(U, n(1.0), n(1.0), ovs(vec![(&["x", "y"], n(1.0))])));
        assert_deep_strict_equal(&inspect_dynamic(inspect(&actual, "d", None).user_local()), &U);

        assert_deep_strict_equal(&inspect_dynamic(inspect(&actual, "a", None).user_remote()), &U);
        assert_deep_strict_equal(&inspect_dynamic(inspect(&actual, "a", Some("x")).user_remote()), &U);
        assert_deep_strict_equal(&inspect_dynamic(inspect(&actual, "b", Some("x")).user_remote()), &U);
        assert_deep_strict_equal(&inspect_dynamic(inspect(&actual, "d", None).user_remote()), &U);

        assert_deep_strict_equal(&inspect_dynamic(inspect(&actual, "a", None).user()), &insp(n(1.0), U, n(1.0), ovs(vec![(&["x", "y"], n(2.0))])));
        assert_deep_strict_equal(&inspect_dynamic(inspect(&actual, "a", Some("x")).user()), &insp(n(1.0), n(2.0), n(2.0), ovs(vec![(&["x", "y"], n(2.0))])));
        assert_deep_strict_equal(&inspect_dynamic(inspect(&actual, "b", Some("x")).user()), &insp(U, n(1.0), n(1.0), ovs(vec![(&["x", "y"], n(1.0))])));
        assert_deep_strict_equal(&inspect_dynamic(inspect(&actual, "d", None).user()), &U);
    }

    #[test]
    fn parsing_configuration_only_with_local_user_configuration_and_raw_is_not_same() {
        let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
        let configuration = test_configuration(
            empty_model(),
            empty_model(),
            empty_model(),
            model(
                j("{ 'a': 1, 'c': 1 }"),
                &["a", "c"],
                vec![ov(&["x", "y"], j("{ 'a': 2 }"), &["a"])],
                raw_dictionary("{ 'a': 1, 'b': 2, 'c': 1, 'd': 3, '[x][y]': { 'a': 2, 'b': 1 } }"),
            ),
            None,
        );

        let actual = parsed(&configuration);

        assert_deep_strict_equal(&inspect_dynamic(inspect(&actual, "a", None).user_local()), &insp(n(1.0), U, n(1.0), ovs(vec![(&["x", "y"], n(2.0))])));
        assert_deep_strict_equal(&inspect_dynamic(inspect(&actual, "a", Some("x")).user_local()), &insp(n(1.0), n(2.0), n(2.0), ovs(vec![(&["x", "y"], n(2.0))])));
        assert_deep_strict_equal(&inspect_dynamic(inspect(&actual, "b", Some("x")).user_local()), &insp(n(2.0), n(1.0), n(1.0), ovs(vec![(&["x", "y"], n(1.0))])));
        assert_deep_strict_equal(&inspect_dynamic(inspect(&actual, "d", None).user_local()), &insp(n(3.0), U, n(3.0), U));
        assert_deep_strict_equal(&inspect_dynamic(inspect(&actual, "e", None).user_local()), &U);

        assert_deep_strict_equal(&inspect_dynamic(inspect(&actual, "a", None).user_remote()), &U);
        assert_deep_strict_equal(&inspect_dynamic(inspect(&actual, "a", Some("x")).user_remote()), &U);
        assert_deep_strict_equal(&inspect_dynamic(inspect(&actual, "b", Some("x")).user_remote()), &U);
        assert_deep_strict_equal(&inspect_dynamic(inspect(&actual, "d", None).user_remote()), &U);
        assert_deep_strict_equal(&inspect_dynamic(inspect(&actual, "e", None).user_remote()), &U);

        assert_deep_strict_equal(&inspect_dynamic(inspect(&actual, "a", None).user()), &insp(n(1.0), U, n(1.0), ovs(vec![(&["x", "y"], n(2.0))])));
        assert_deep_strict_equal(&inspect_dynamic(inspect(&actual, "a", Some("x")).user()), &insp(n(1.0), n(2.0), n(2.0), ovs(vec![(&["x", "y"], n(2.0))])));
        assert_deep_strict_equal(&inspect_dynamic(inspect(&actual, "b", Some("x")).user()), &insp(n(2.0), n(1.0), n(1.0), ovs(vec![(&["x", "y"], n(1.0))])));
        assert_deep_strict_equal(&inspect_dynamic(inspect(&actual, "d", None).user()), &insp(n(3.0), U, n(3.0), U));
        assert_deep_strict_equal(&inspect_dynamic(inspect(&actual, "e", None).user()), &U);
    }

    #[test]
    fn parsing_configuration_with_local_and_remote_user_configuration_and_raw_is_same_for_both() {
        let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
        let configuration = test_configuration(
            empty_model(),
            empty_model(),
            empty_model(),
            model(j("{ 'a': 1 }"), &["a"], vec![ov(&["x", "y"], j("{ 'a': 2 }"), &["a"])], None),
            Some(model(j("{ 'b': 3 }"), &["b"], vec![], None)),
        );

        let actual = parsed(&configuration);

        assert_deep_strict_equal(&inspect_dynamic(inspect(&actual, "a", None).user_local()), &insp(n(1.0), U, n(1.0), ovs(vec![(&["x", "y"], n(2.0))])));
        assert_deep_strict_equal(&inspect_dynamic(inspect(&actual, "a", Some("x")).user_local()), &insp(n(1.0), n(2.0), n(2.0), ovs(vec![(&["x", "y"], n(2.0))])));
        assert_deep_strict_equal(&inspect_dynamic(inspect(&actual, "b", None).user_local()), &U);
        assert_deep_strict_equal(&inspect_dynamic(inspect(&actual, "b", Some("y")).user_local()), &U);
        assert_deep_strict_equal(&inspect_dynamic(inspect(&actual, "c", None).user_local()), &U);

        assert_deep_strict_equal(&inspect_dynamic(inspect(&actual, "a", None).user_remote()), &U);
        assert_deep_strict_equal(&inspect_dynamic(inspect(&actual, "a", Some("x")).user_remote()), &U);
        assert_deep_strict_equal(&inspect_dynamic(inspect(&actual, "b", None).user_remote()), &insp(n(3.0), U, n(3.0), U));
        assert_deep_strict_equal(&inspect_dynamic(inspect(&actual, "b", Some("y")).user_remote()), &insp(n(3.0), U, n(3.0), U));
        assert_deep_strict_equal(&inspect_dynamic(inspect(&actual, "c", None).user_remote()), &U);

        assert_deep_strict_equal(&inspect_dynamic(inspect(&actual, "a", None).user()), &insp(n(1.0), U, n(1.0), ovs(vec![(&["x", "y"], n(2.0))])));
        assert_deep_strict_equal(&inspect_dynamic(inspect(&actual, "a", Some("x")).user()), &insp(n(1.0), n(2.0), n(2.0), ovs(vec![(&["x", "y"], n(2.0))])));
        assert_deep_strict_equal(&inspect_dynamic(inspect(&actual, "b", None).user()), &insp(n(3.0), U, n(3.0), U));
        assert_deep_strict_equal(&inspect_dynamic(inspect(&actual, "b", Some("y")).user()), &insp(n(3.0), U, n(3.0), U));
        assert_deep_strict_equal(&inspect_dynamic(inspect(&actual, "c", None).user()), &U);
    }

    #[test]
    fn parsing_configuration_with_local_and_remote_user_configuration_and_raw_is_not_same_for_local_user() {
        let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
        let configuration = test_configuration(
            empty_model(),
            empty_model(),
            empty_model(),
            model(
                j("{ 'a': 1 }"),
                &["a"],
                vec![ov(&["x", "y"], j("{ 'a': 2 }"), &["a"])],
                raw_dictionary("{ 'a': 1, 'b': 2, 'c': 3, '[x][y]': { 'a': 2, 'b': 4 } }"),
            ),
            Some(model(j("{ 'b': 3 }"), &["b"], vec![], None)),
        );

        let actual = parsed(&configuration);

        assert_deep_strict_equal(&inspect_dynamic(inspect(&actual, "a", None).user_local()), &insp(n(1.0), U, n(1.0), ovs(vec![(&["x", "y"], n(2.0))])));
        assert_deep_strict_equal(&inspect_dynamic(inspect(&actual, "a", Some("x")).user_local()), &insp(n(1.0), n(2.0), n(2.0), ovs(vec![(&["x", "y"], n(2.0))])));
        assert_deep_strict_equal(&inspect_dynamic(inspect(&actual, "b", None).user_local()), &insp(n(2.0), U, n(2.0), ovs(vec![(&["x", "y"], n(4.0))])));
        assert_deep_strict_equal(&inspect_dynamic(inspect(&actual, "b", Some("y")).user_local()), &insp(n(2.0), n(4.0), n(4.0), ovs(vec![(&["x", "y"], n(4.0))])));
        assert_deep_strict_equal(&inspect_dynamic(inspect(&actual, "c", None).user_local()), &insp(n(3.0), U, n(3.0), U));

        assert_deep_strict_equal(&inspect_dynamic(inspect(&actual, "a", None).user_remote()), &U);
        assert_deep_strict_equal(&inspect_dynamic(inspect(&actual, "a", Some("x")).user_remote()), &U);
        assert_deep_strict_equal(&inspect_dynamic(inspect(&actual, "b", None).user_remote()), &insp(n(3.0), U, n(3.0), U));
        assert_deep_strict_equal(&inspect_dynamic(inspect(&actual, "b", Some("y")).user_remote()), &insp(n(3.0), U, n(3.0), U));
        assert_deep_strict_equal(&inspect_dynamic(inspect(&actual, "c", None).user_remote()), &U);

        assert_deep_strict_equal(&inspect_dynamic(inspect(&actual, "a", None).user()), &insp(n(1.0), U, n(1.0), ovs(vec![(&["x", "y"], n(2.0))])));
        assert_deep_strict_equal(&inspect_dynamic(inspect(&actual, "a", Some("x")).user()), &insp(n(1.0), n(2.0), n(2.0), ovs(vec![(&["x", "y"], n(2.0))])));
        assert_deep_strict_equal(&inspect_dynamic(inspect(&actual, "b", None).user()), &insp(n(3.0), U, n(3.0), U));
        assert_deep_strict_equal(&inspect_dynamic(inspect(&actual, "b", Some("y")).user()), &insp(n(3.0), U, n(3.0), U));
        assert_deep_strict_equal(&inspect_dynamic(inspect(&actual, "c", None).user()), &U);
    }

    #[test]
    fn parsing_configuration_with_local_and_remote_user_configuration_and_raw_is_not_same_for_remote_user() {
        let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
        let configuration = test_configuration(
            empty_model(),
            empty_model(),
            empty_model(),
            model(j("{ 'b': 3 }"), &["b"], vec![], None),
            Some(model(
                j("{ 'a': 1 }"),
                &["a"],
                vec![ov(&["x", "y"], j("{ 'a': 2 }"), &["a"])],
                raw_dictionary("{ 'a': 1, 'b': 2, 'c': 3, '[x][y]': { 'a': 2, 'b': 4 } }"),
            )),
        );

        let actual = parsed(&configuration);

        assert_deep_strict_equal(&inspect_dynamic(inspect(&actual, "a", None).user_local()), &U);
        assert_deep_strict_equal(&inspect_dynamic(inspect(&actual, "a", Some("x")).user_local()), &U);
        assert_deep_strict_equal(&inspect_dynamic(inspect(&actual, "b", None).user_local()), &insp(n(3.0), U, n(3.0), U));
        assert_deep_strict_equal(&inspect_dynamic(inspect(&actual, "b", Some("y")).user_local()), &insp(n(3.0), U, n(3.0), U));
        assert_deep_strict_equal(&inspect_dynamic(inspect(&actual, "c", None).user_local()), &U);

        assert_deep_strict_equal(&inspect_dynamic(inspect(&actual, "a", None).user_remote()), &insp(n(1.0), U, n(1.0), ovs(vec![(&["x", "y"], n(2.0))])));
        assert_deep_strict_equal(&inspect_dynamic(inspect(&actual, "a", Some("x")).user_remote()), &insp(n(1.0), n(2.0), n(2.0), ovs(vec![(&["x", "y"], n(2.0))])));
        assert_deep_strict_equal(&inspect_dynamic(inspect(&actual, "b", None).user_remote()), &insp(n(2.0), U, n(2.0), ovs(vec![(&["x", "y"], n(4.0))])));
        assert_deep_strict_equal(&inspect_dynamic(inspect(&actual, "b", Some("y")).user_remote()), &insp(n(2.0), n(4.0), n(4.0), ovs(vec![(&["x", "y"], n(4.0))])));
        assert_deep_strict_equal(&inspect_dynamic(inspect(&actual, "c", None).user_remote()), &insp(n(3.0), U, n(3.0), U));

        assert_deep_strict_equal(&inspect_dynamic(inspect(&actual, "a", None).user()), &insp(n(1.0), U, n(1.0), ovs(vec![(&["x", "y"], n(2.0))])));
        assert_deep_strict_equal(&inspect_dynamic(inspect(&actual, "a", Some("x")).user()), &insp(n(1.0), n(2.0), n(2.0), ovs(vec![(&["x", "y"], n(2.0))])));
        assert_deep_strict_equal(&inspect_dynamic(inspect(&actual, "b", None).user()), &insp(n(3.0), U, n(3.0), U));
        assert_deep_strict_equal(&inspect_dynamic(inspect(&actual, "b", Some("y")).user()), &insp(n(3.0), U, n(3.0), U));
        assert_deep_strict_equal(&inspect_dynamic(inspect(&actual, "c", None).user()), &U);
    }

    #[test]
    fn parsing_configuration_with_local_and_remote_user_configuration_and_raw_is_not_same_for_both() {
        let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
        let configuration = test_configuration(
            empty_model(),
            empty_model(),
            empty_model(),
            model(j("{ 'b': 3 }"), &["b"], vec![], raw_dictionary("{ 'a': 4, 'b': 3 }")),
            Some(model(
                j("{ 'a': 1 }"),
                &["a"],
                vec![ov(&["x", "y"], j("{ 'a': 2 }"), &["a"])],
                raw_dictionary("{ 'a': 1, 'b': 2, 'c': 3, '[x][y]': { 'a': 2, 'b': 4 } }"),
            )),
        );

        let actual = parsed(&configuration);

        assert_deep_strict_equal(&inspect_dynamic(inspect(&actual, "a", None).user_local()), &insp(n(4.0), U, n(4.0), U));
        assert_deep_strict_equal(&inspect_dynamic(inspect(&actual, "a", Some("x")).user_local()), &insp(n(4.0), U, n(4.0), U));
        assert_deep_strict_equal(&inspect_dynamic(inspect(&actual, "b", None).user_local()), &insp(n(3.0), U, n(3.0), U));
        assert_deep_strict_equal(&inspect_dynamic(inspect(&actual, "b", Some("y")).user_local()), &insp(n(3.0), U, n(3.0), U));
        assert_deep_strict_equal(&inspect_dynamic(inspect(&actual, "c", None).user_local()), &U);

        assert_deep_strict_equal(&inspect_dynamic(inspect(&actual, "a", None).user_remote()), &insp(n(1.0), U, n(1.0), ovs(vec![(&["x", "y"], n(2.0))])));
        assert_deep_strict_equal(&inspect_dynamic(inspect(&actual, "a", Some("x")).user_remote()), &insp(n(1.0), n(2.0), n(2.0), ovs(vec![(&["x", "y"], n(2.0))])));
        assert_deep_strict_equal(&inspect_dynamic(inspect(&actual, "b", None).user_remote()), &insp(n(2.0), U, n(2.0), ovs(vec![(&["x", "y"], n(4.0))])));
        assert_deep_strict_equal(&inspect_dynamic(inspect(&actual, "b", Some("y")).user_remote()), &insp(n(2.0), n(4.0), n(4.0), ovs(vec![(&["x", "y"], n(4.0))])));
        assert_deep_strict_equal(&inspect_dynamic(inspect(&actual, "c", None).user_remote()), &insp(n(3.0), U, n(3.0), U));

        assert_deep_strict_equal(&inspect_dynamic(inspect(&actual, "a", None).user()), &insp(n(1.0), U, n(1.0), ovs(vec![(&["x", "y"], n(2.0))])));
        assert_deep_strict_equal(&inspect_dynamic(inspect(&actual, "a", Some("x")).user()), &insp(n(1.0), n(2.0), n(2.0), ovs(vec![(&["x", "y"], n(2.0))])));
        assert_deep_strict_equal(&inspect_dynamic(inspect(&actual, "b", None).user()), &insp(n(3.0), U, n(3.0), U));
        assert_deep_strict_equal(&inspect_dynamic(inspect(&actual, "b", Some("y")).user()), &insp(n(3.0), U, n(3.0), U));
        assert_deep_strict_equal(&inspect_dynamic(inspect(&actual, "c", None).user()), &U);
    }
}
