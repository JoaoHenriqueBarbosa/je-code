//! Porte de `src/vs/workbench/contrib/terminal/test/common/terminalConfiguration.test.ts` (suíte `Terminal configuration`),
//! caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` vira o guarda criado no começo de cada caso.
//! - O `suiteSetup`/`suiteTeardown` que registra e remove o nó no registro global roda no começo e no fim de cada caso (cada
//!   teste tem o seu thread e, portanto, o seu registro).
//! - `assert.deepStrictEqual({ value, restricted }, { value: undefined, restricted: [...] })` compara o `value` com
//!   `undefined` e a lista de restritas por igualdade.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::dynamic::{Dynamic, json_stringify};
use je_base::js_string::JsString;
use je_platform::configuration::models::{ConfigurationModelParser, ConfigurationParseOptions};
use je_platform::configuration::registry::{ConfigurationNode, ConfigurationNodeData, configuration_registry};
use je_platform::log::NullLogService;
use je_platform::terminal::terminal_settings::TerminalSettingId;
use je_workbench::contrib::terminal::common::terminal_configuration::terminal_configuration_property;
use je_workbench::services::configuration::common::configuration_models::WorkspaceConfigurationModelParser;

fn allowed_link_schemes() -> &'static str {
    TerminalSettingId::AllowedLinkSchemes.as_str()
}

/// `configurationNode`.
fn configuration_node() -> ConfigurationNode {
    ConfigurationNode::new(
        ConfigurationNodeData { id: Some("terminalConfigurationTest".to_string()), type_: Some(Dynamic::string("object")), ..ConfigurationNodeData::default() }
            .with_properties([(
                allowed_link_schemes(),
                terminal_configuration_property(allowed_link_schemes()).expect("a configuração está no dicionário"),
            )]),
    )
}

/// `workspaceValues`: a malicious workspace widening the terminal link scheme allowlist, e.g. to reach SMB URIs.
fn workspace_values() -> Dynamic {
    Dynamic::object([(allowed_link_schemes(), Dynamic::array(["file", "http", "https", "search-ms"].map(Dynamic::string)))])
}

/// `suiteSetup`, o corpo do caso e `suiteTeardown`.
fn with_registered_configuration(body: impl FnOnce()) {
    let registry = configuration_registry();
    let node = configuration_node();
    registry.register_configuration(node.clone());
    body();
    registry.deregister_configurations(&[node]);
}

fn skip_restricted() -> ConfigurationParseOptions {
    ConfigurationParseOptions { skip_restricted: true, ..ConfigurationParseOptions::default() }
}

#[test]
fn registers_allowed_link_schemes_as_a_restricted_setting() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    with_registered_configuration(|| {
        let property = terminal_configuration_property(allowed_link_schemes()).expect("a configuração está no dicionário");
        assert!(property.get("restricted").strict_equals(&Dynamic::Bool(true)));
    });
}

#[test]
fn filters_allowed_link_schemes_from_an_untrusted_single_folder_workspace() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    with_registered_configuration(|| {
        let mut parser = ConfigurationModelParser::new("terminalLinkSchemes", NullLogService::shared());
        let content = json_stringify(&workspace_values()).expect("o objeto serializa");
        parser.parse(Some(content.as_str()), Some(&skip_restricted()));

        let value = parser.configuration_model().get_value(allowed_link_schemes());
        assert!(value.is_undefined());
        assert_eq!(parser.restricted_configurations(), [JsString::from(allowed_link_schemes())]);
    });
}

#[test]
fn filters_allowed_link_schemes_from_an_untrusted_workspace_file() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    with_registered_configuration(|| {
        let mut parser = WorkspaceConfigurationModelParser::new("terminalLinkSchemes", NullLogService::shared());
        let content = json_stringify(&Dynamic::object([("folders", Dynamic::array([])), ("settings", workspace_values())])).expect("o objeto serializa");
        parser.parse(Some(content.as_str()), Some(&skip_restricted()));

        assert!(parser.settings_model().get_value(allowed_link_schemes()).is_undefined());
    });
}
