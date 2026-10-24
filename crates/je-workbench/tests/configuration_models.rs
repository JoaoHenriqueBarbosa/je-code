//! Porte de `src/vs/workbench/services/configuration/test/common/configurationModels.test.ts` (suítes `FolderSettingsModelParser`, `StandaloneConfigurationModelParser` e `Workspace Configuration`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - **`ensureNoDisposablesAreLeakedInTestSuite()`.** É o `LeakGuard` criado no início de cada corpo e descartado no fim.
//! - **`suiteSetup`.** O registro de configuração é por thread e cada teste tem o seu: o `suiteSetup` da suíte
//!   `FolderSettingsModelParser` roda no início de cada teste dela.
//! - **`Object.create(null)` e `{}`.** São o mesmo valor dinâmico; os esperados saem de literais.
//! - **`assert.deepStrictEqual`.** É `assert_deep_strict_equal` sobre valores dinâmicos, ou `assert_eq!` quando o valor é tipado.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod support;

use std::rc::Rc;

use common::{assert_deep_strict_equal, ensure_no_disposables_are_leaked_in_test_suite};
use je_base::map::ResourceMap;
use je_platform::configuration::models::{ConfigurationModel, ConfigurationModelParser, ConfigurationParseOptions};
use je_platform::configuration::registry::{ConfigurationScope, configuration_registry};
use je_platform::workspace::{WorkspaceFolder, new_test_workspace};
use je_workbench::services::configuration::common::configuration_models::{Configuration, StandaloneConfigurationModelParser};
use support::configuration_models_support::{change_of, j, node, null_log, overrides_dynamic, overrides_of, string_property, stringify_literal, to_configuration_model, uri_file};

mod folder_settings_model_parser {
    use super::*;

    /// O `suiteSetup` da suíte.
    fn suite_setup() {
        configuration_registry().register_configuration(node(
            "FolderSettingsModelParser_1",
            vec![
                ("FolderSettingsModelParser.window", string_property(None)),
                ("FolderSettingsModelParser.resource", string_property(Some(ConfigurationScope::Resource))),
                ("FolderSettingsModelParser.resourceLanguage", string_property(Some(ConfigurationScope::LanguageOverridable))),
                ("FolderSettingsModelParser.application", string_property(Some(ConfigurationScope::Application))),
                ("FolderSettingsModelParser.machine", string_property(Some(ConfigurationScope::Machine))),
            ],
        ));
    }

    fn parse(content: &str, options: &ConfigurationParseOptions) -> ConfigurationModelParser {
        let mut test_object = ConfigurationModelParser::new("settings", null_log());
        test_object.parse(Some(&stringify_literal(content)), Some(options));
        test_object
    }

    #[test]
    fn parse_all_folder_settings() {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        suite_setup();
        let test_object = parse(
            "{ 'FolderSettingsModelParser.window': 'window', 'FolderSettingsModelParser.resource': 'resource', 'FolderSettingsModelParser.application': 'application', 'FolderSettingsModelParser.machine': 'executable' }",
            &ConfigurationParseOptions { scopes: Some(vec![ConfigurationScope::Resource, ConfigurationScope::Window]), ..Default::default() },
        );

        let expected = j("{ 'FolderSettingsModelParser': { 'window': 'window', 'resource': 'resource' } }");
        assert_deep_strict_equal(&test_object.configuration_model().contents(), &expected);
        drop(guard);
    }

    #[test]
    fn parse_resource_folder_settings() {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        suite_setup();
        let test_object = parse(
            "{ 'FolderSettingsModelParser.window': 'window', 'FolderSettingsModelParser.resource': 'resource', 'FolderSettingsModelParser.application': 'application', 'FolderSettingsModelParser.machine': 'executable' }",
            &ConfigurationParseOptions { scopes: Some(vec![ConfigurationScope::Resource]), ..Default::default() },
        );

        let expected = j("{ 'FolderSettingsModelParser': { 'resource': 'resource' } }");
        assert_deep_strict_equal(&test_object.configuration_model().contents(), &expected);
        drop(guard);
    }

    #[test]
    fn parse_resource_and_resource_language_settings() {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        suite_setup();
        let test_object = parse(
            "{ '[json]': { 'FolderSettingsModelParser.window': 'window', 'FolderSettingsModelParser.resource': 'resource', 'FolderSettingsModelParser.resourceLanguage': 'resourceLanguage', 'FolderSettingsModelParser.application': 'application', 'FolderSettingsModelParser.machine': 'executable' } }",
            &ConfigurationParseOptions { scopes: Some(vec![ConfigurationScope::Resource, ConfigurationScope::LanguageOverridable]), ..Default::default() },
        );

        let expected = j("{ 'FolderSettingsModelParser': { 'resource': 'resource', 'resourceLanguage': 'resourceLanguage' } }");
        assert_deep_strict_equal(
            &overrides_dynamic(&test_object.configuration_model().overrides()),
            &overrides_dynamic(&[overrides_of(&["json"], expected, &["FolderSettingsModelParser.resource", "FolderSettingsModelParser.resourceLanguage"])]),
        );
        drop(guard);
    }

    #[test]
    fn reparse_folder_settings_excludes_application_and_machine_setting() {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        suite_setup();
        let parse_options = ConfigurationParseOptions { scopes: Some(vec![ConfigurationScope::Resource, ConfigurationScope::Window]), ..Default::default() };
        let mut test_object = parse("{ 'FolderSettingsModelParser.resource': 'resource', 'FolderSettingsModelParser.anotherApplicationSetting': 'executable' }", &parse_options);

        let expected = j("{ 'FolderSettingsModelParser': { 'resource': 'resource', 'anotherApplicationSetting': 'executable' } }");
        assert_deep_strict_equal(&test_object.configuration_model().contents(), &expected);

        configuration_registry().register_configuration(node(
            "FolderSettingsModelParser_2",
            vec![
                ("FolderSettingsModelParser.anotherApplicationSetting", string_property(Some(ConfigurationScope::Application))),
                ("FolderSettingsModelParser.anotherMachineSetting", string_property(Some(ConfigurationScope::Machine))),
            ],
        ));

        test_object.reparse(&parse_options);

        let expected = j("{ 'FolderSettingsModelParser': { 'resource': 'resource' } }");
        assert_deep_strict_equal(&test_object.configuration_model().contents(), &expected);
        drop(guard);
    }
}

mod standalone_configuration_model_parser {
    use super::*;

    #[test]
    fn parse_tasks_stand_alone_configuration_model() {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let mut test_object = StandaloneConfigurationModelParser::new("tasks", "tasks", null_log());

        test_object.parse(Some(&stringify_literal("{ 'version': '1.1.1', 'tasks': [] }")), None);

        let expected = j("{ 'tasks': { 'version': '1.1.1', 'tasks': [] } }");
        assert_deep_strict_equal(&test_object.configuration_model().contents(), &expected);
        drop(guard);
    }
}

mod workspace_configuration {
    use super::*;

    fn default_configuration_model() -> ConfigurationModel {
        to_configuration_model(
            "{ 'editor.lineNumbers': 'on', 'editor.fontSize': 12, 'window.zoomLevel': 1, '[markdown]': { 'editor.wordWrap': 'off' }, 'window.title': 'custom', 'workbench.enableTabs': false, 'editor.insertSpaces': true }",
        )
    }

    /// `new Configuration(empty x6, new ResourceMap(), empty, new ResourceMap(), workspace, new NullLogService())`.
    fn new_configuration(workspace: &Rc<je_platform::workspace::Workspace>) -> Configuration {
        let empty = || ConfigurationModel::create_empty_model(null_log());
        Configuration::new(empty(), empty(), empty(), empty(), empty(), empty(), ResourceMap::new(), empty(), ResourceMap::new(), Some(workspace.clone()), null_log())
    }

    fn new_workspace() -> Rc<je_platform::workspace::Workspace> {
        new_test_workspace(
            "a",
            vec![
                WorkspaceFolder::new(uri_file("folder1"), "a", 0, None),
                WorkspaceFolder::new(uri_file("folder2"), "b", 1, None),
                WorkspaceFolder::new(uri_file("folder3"), "c", 2, None),
            ],
            None,
            None,
        )
    }

    #[test]
    fn test_compare_same_configurations() {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let workspace = new_workspace();
        let configuration1 = new_configuration(&workspace);
        configuration1.update_default_configuration(default_configuration_model());
        configuration1.update_local_user_configuration(to_configuration_model("{ 'window.title': 'native', '[typescript]': { 'editor.insertSpaces': false } }"));
        configuration1.update_workspace_configuration(to_configuration_model("{ 'editor.lineNumbers': 'on' }"));
        configuration1.update_folder_configuration(&uri_file("folder1"), to_configuration_model("{ 'editor.fontSize': 14 }"));
        configuration1.update_folder_configuration(&uri_file("folder2"), to_configuration_model("{ 'editor.wordWrap': 'on' }"));

        let configuration2 = new_configuration(&workspace);
        configuration2.update_default_configuration(default_configuration_model());
        configuration2.update_local_user_configuration(to_configuration_model("{ 'window.title': 'native', '[typescript]': { 'editor.insertSpaces': false } }"));
        configuration2.update_workspace_configuration(to_configuration_model("{ 'editor.lineNumbers': 'on' }"));
        configuration2.update_folder_configuration(&uri_file("folder1"), to_configuration_model("{ 'editor.fontSize': 14 }"));
        configuration2.update_folder_configuration(&uri_file("folder2"), to_configuration_model("{ 'editor.wordWrap': 'on' }"));

        let actual = configuration2.compare(&configuration1);

        assert_eq!(actual, change_of(&[], &[]));
        drop(guard);
    }

    #[test]
    fn test_compare_different_configurations() {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let workspace = new_workspace();
        let configuration1 = new_configuration(&workspace);
        configuration1.update_default_configuration(default_configuration_model());
        configuration1.update_local_user_configuration(to_configuration_model("{ 'window.title': 'native', '[typescript]': { 'editor.insertSpaces': false } }"));
        configuration1.update_workspace_configuration(to_configuration_model("{ 'editor.lineNumbers': 'on' }"));
        configuration1.update_folder_configuration(&uri_file("folder1"), to_configuration_model("{ 'editor.fontSize': 14 }"));
        configuration1.update_folder_configuration(&uri_file("folder2"), to_configuration_model("{ 'editor.wordWrap': 'on' }"));

        let configuration2 = new_configuration(&workspace);
        configuration2.update_default_configuration(default_configuration_model());
        configuration2.update_local_user_configuration(to_configuration_model("{ 'workbench.enableTabs': true, '[typescript]': { 'editor.insertSpaces': true } }"));
        configuration2.update_workspace_configuration(to_configuration_model("{ 'editor.fontSize': 11 }"));
        configuration2.update_folder_configuration(&uri_file("folder1"), to_configuration_model("{ 'editor.insertSpaces': true }"));
        configuration2.update_folder_configuration(
            &uri_file("folder2"),
            to_configuration_model("{ '[markdown]': { 'editor.wordWrap': 'on', 'editor.lineNumbers': 'relative' } }"),
        );

        let actual = configuration2.compare(&configuration1);

        assert_eq!(
            actual,
            change_of(
                &["editor.wordWrap", "editor.fontSize", "[markdown]", "window.title", "workbench.enableTabs", "[typescript]"],
                &[("markdown", &["editor.lineNumbers", "editor.wordWrap"]), ("typescript", &["editor.insertSpaces"])],
            )
        );
        drop(guard);
    }
}
