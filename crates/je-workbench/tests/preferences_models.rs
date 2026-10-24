//! Porte de `src/vs/workbench/services/preferences/test/common/preferencesModels.test.ts` (suíte `DefaultSettings`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` e o `disposables`. O guarda de vazamentos é criado antes do [`Fixture`]; o
//!   `Drop` do [`Fixture`] tira do registro as configurações que o caso registrou (o `toDisposable(() =>
//!   configurationRegistry.deregisterConfigurations(...))`), descarta o `disposables` e depois o [`RegistryGuard`] restaura o
//!   registro (que é por thread, e cada caso roda na sua).
//! - `IConfigurationNode` com `extensionInfo` é o `ConfigurationNodeData` com [`ExtensionInfo`]; `configurationService` é o
//!   `TestConfigurationService`.
//! - `assert.notStrictEqual(group1, group2)` entre grupos (objetos distintos no original) compara as posições na lista.

#[path = "../../je-platform/tests/common/mod.rs"]
mod common;

use std::rc::Rc;

use common::{RegistryGuard, ensure_no_disposables_are_leaked_in_test_suite, prop};
use je_base::dynamic::Dynamic;
use je_base::lifecycle::DisposableStore;
use je_platform::configuration::ConfigurationTarget;
use je_platform::configuration::registry::{ConfigurationNode, ConfigurationNodeData, ExtensionInfo, PropertySchema, configuration_registry};
use je_platform::configuration::service::IConfigurationService;
use je_platform::configuration::test_configuration_service::TestConfigurationService;
use je_workbench::services::preferences::preferences::ISettingsGroup;
use je_workbench::services::preferences::preferences_models::DefaultSettings;

/// O `setup` da suíte e o que os casos usam dele.
struct Fixture {
    disposables: Rc<DisposableStore>,
    configuration_service: Rc<TestConfigurationService>,
    registered: Vec<ConfigurationNode>,
    _registry: RegistryGuard,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        configuration_registry().deregister_configurations(&self.registered);
        self.disposables.dispose();
    }
}

impl Fixture {
    fn new() -> Fixture {
        let registry = RegistryGuard::new();
        Fixture { disposables: DisposableStore::new(), configuration_service: TestConfigurationService::new(None), registered: Vec::new(), _registry: registry }
    }

    /// `configurationRegistry.registerConfiguration(config)` e o `deregisterConfigurations` do descarte.
    fn register(&mut self, config: ConfigurationNode) {
        configuration_registry().register_configuration(config.clone());
        self.registered.push(config);
    }

    /// `disposables.add(new DefaultSettings([], ConfigurationTarget.USER, configurationService))`.
    fn default_settings(&self) -> Rc<DefaultSettings> {
        let configuration_service: Rc<dyn IConfigurationService> = self.configuration_service.clone();
        self.disposables.add_rc(DefaultSettings::new(Vec::new(), ConfigurationTarget::User, configuration_service))
    }
}

/// `{ type: 'string', default, description }`.
fn string_setting(default: &str, description: &str) -> PropertySchema {
    prop(&format!("{{ 'type': 'string', 'default': '{default}', 'description': '{description}' }}"))
}

/// `{ id, title?, order?, type: 'object', properties: { [key]: { type: 'string', default, description } }, extensionInfo: { id } }`.
fn config(id: &str, title: Option<&str>, order: Option<f64>, key: &str, default: &str, description: &str, extension_id: &str) -> ConfigurationNode {
    ConfigurationNode::new(
        ConfigurationNodeData {
            id: Some(id.to_owned()),
            title: title.map(str::to_owned),
            order,
            type_: Some(Dynamic::string("object")),
            extension_info: Some(ExtensionInfo::new(extension_id)),
            ..ConfigurationNodeData::default()
        }
        .with_properties(vec![(key, string_setting(default, description))]),
    )
}

/// `groups.filter(g => g.extensionInfo?.id === extensionId)`.
fn groups_of(groups: &[ISettingsGroup], extension_id: &str) -> Vec<ISettingsGroup> {
    groups.iter().filter(|group| group.extension_info.as_ref().map(|info| info.id.as_str()) == Some(extension_id)).cloned().collect()
}

/// `groups.find(g => g.extensionInfo?.id === extensionId)`, como a posição na lista.
fn position_of(groups: &[ISettingsGroup], extension_id: &str) -> Option<usize> {
    groups.iter().position(|group| group.extension_info.as_ref().map(|info| info.id.as_str()) == Some(extension_id))
}

#[test]
fn preserves_deprecation_message_presentation() {
    let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
    let mut f = Fixture::new();
    let node = ConfigurationNode::new(
        ConfigurationNodeData { id: Some("deprecatedSettings".to_owned()), title: Some("Deprecated Settings".to_owned()), type_: Some(Dynamic::string("object")), ..ConfigurationNodeData::default() }
            .with_properties(vec![(
                "test.deprecatedSetting",
                prop(
                    "{ 'type': 'boolean', 'default': false, 'deprecationMessage': 'Use the replacement setting.', 'deprecationMessageSeverity': 'info', 'deprecationMessageShowInSettings': true }",
                ),
            )]),
    );
    f.register(node);

    let default_settings = f.default_settings();
    let setting = default_settings
        .get_registered_groups()
        .into_iter()
        .flat_map(|group| group.sections)
        .flat_map(|section| section.settings)
        .find(|setting| setting.key == "test.deprecatedSetting");

    assert_eq!(
        (setting.as_ref().and_then(|setting| setting.deprecation_message_severity.clone()), setting.as_ref().and_then(|setting| setting.deprecation_message_show_in_settings)),
        (Some("info".to_owned()), Some(true))
    );
}

#[test]
fn groups_settings_by_title_when_they_share_the_same_extension_id() {
    let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
    let mut f = Fixture::new();
    let extension_id = "test.extension";
    f.register(config("config1", Some("Group 1"), None, "test.setting1", "value1", "Setting 1", extension_id));
    f.register(config("config2", Some("Group 2"), None, "test.setting2", "value2", "Setting 2", extension_id));

    let default_settings = f.default_settings();
    let groups = default_settings.get_registered_groups();

    let extension_groups = groups_of(&groups, extension_id);

    assert_eq!(extension_groups.len(), 2, "Should have 2 groups");
    assert_eq!(extension_groups[0].title, "Group 1");
    assert_eq!(extension_groups[1].title, "Group 2");

    assert_eq!(extension_groups[0].sections[0].settings.len(), 1);
    assert_eq!(extension_groups[0].sections[0].settings[0].key, "test.setting1");

    assert_eq!(extension_groups[1].sections[0].settings.len(), 1);
    assert_eq!(extension_groups[1].sections[0].settings[0].key, "test.setting2");
}

#[test]
fn groups_settings_by_id_when_they_share_the_same_extension_id_and_have_no_title() {
    let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
    let mut f = Fixture::new();
    let extension_id = "test.extension";
    f.register(config("group1", None, None, "test.setting1", "value1", "Setting 1", extension_id));
    f.register(config("group1", None, None, "test.setting2", "value2", "Setting 2", extension_id));

    let default_settings = f.default_settings();
    let groups = default_settings.get_registered_groups();

    let extension_groups = groups_of(&groups, extension_id);

    assert_eq!(extension_groups.len(), 1, "Should have 1 group");
    assert_eq!(extension_groups[0].id, "group1");
    assert_eq!(extension_groups[0].sections[0].settings.len(), 2);
}

#[test]
fn separates_groups_with_same_id_but_different_titles() {
    let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
    let mut f = Fixture::new();
    let extension_id = "test.extension";
    f.register(config("group1", Some("Title 1"), None, "test.setting1", "value1", "Setting 1", extension_id));
    f.register(config("group1", Some("Title 2"), None, "test.setting2", "value2", "Setting 2", extension_id));

    let default_settings = f.default_settings();
    let groups = default_settings.get_registered_groups();

    let extension_groups = groups_of(&groups, extension_id);

    assert_eq!(extension_groups.len(), 2, "Should have 2 groups");
    assert_eq!(extension_groups[0].title, "Title 1");
    assert_eq!(extension_groups[1].title, "Title 2");
}

#[test]
fn merges_untitled_group_into_titled_group_if_id_matches() {
    let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
    let mut f = Fixture::new();
    let extension_id = "test.extension";
    f.register(config("group1", None, None, "test.setting1", "value1", "Setting 1", extension_id));
    f.register(config("group1", Some("Title 1"), None, "test.setting2", "value2", "Setting 2", extension_id));

    let default_settings = f.default_settings();
    let groups = default_settings.get_registered_groups();

    let extension_groups = groups_of(&groups, extension_id);

    assert_eq!(extension_groups.len(), 1, "Should have 1 group");
    assert_eq!(extension_groups[0].title, "Title 1");
    assert_eq!(extension_groups[0].sections[0].settings.len(), 2);
}

#[test]
fn separates_groups_with_same_id_and_title_but_different_extension_ids() {
    let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
    let mut f = Fixture::new();
    let extension_id1 = "test.extension1";
    let extension_id2 = "test.extension2";
    f.register(config("group1", Some("Title 1"), None, "test.setting1", "value1", "Setting 1", extension_id1));
    f.register(config("group1", Some("Title 1"), None, "test.setting2", "value2", "Setting 2", extension_id2));

    let default_settings = f.default_settings();
    let groups = default_settings.get_registered_groups();

    let group1 = position_of(&groups, extension_id1);
    let group2 = position_of(&groups, extension_id2);

    assert!(group1.is_some());
    assert!(group2.is_some());
    assert_ne!(group1, group2);
    assert_eq!(groups[group1.unwrap()].title, "Title 1");
    assert_eq!(groups[group2.unwrap()].title, "Title 1");
}

#[test]
fn separates_groups_with_same_id_no_title_but_different_extension_ids() {
    let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
    let mut f = Fixture::new();
    let extension_id1 = "test.extension1";
    let extension_id2 = "test.extension2";
    f.register(config("group1", None, None, "test.setting1", "value1", "Setting 1", extension_id1));
    f.register(config("group1", None, None, "test.setting2", "value2", "Setting 2", extension_id2));

    let default_settings = f.default_settings();
    let groups = default_settings.get_registered_groups();

    let group1 = position_of(&groups, extension_id1);
    let group2 = position_of(&groups, extension_id2);

    assert!(group1.is_some());
    assert!(group2.is_some());
    assert_ne!(group1, group2);
}

#[test]
fn groups_settings_correctly_when_extension_id_is_same_as_group_id() {
    let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
    let mut f = Fixture::new();
    let extension_id = "test.extension";
    f.register(config(extension_id, Some("Group 1"), None, "test.setting1", "value1", "Setting 1", extension_id));
    f.register(config(extension_id, Some("Group 2"), None, "test.setting2", "value2", "Setting 2", extension_id));

    let default_settings = f.default_settings();
    let groups = default_settings.get_registered_groups();

    let extension_groups = groups_of(&groups, extension_id);

    assert_eq!(extension_groups.len(), 2, "Should have 2 groups");
    assert_eq!(extension_groups[0].title, "Group 1");
    assert_eq!(extension_groups[1].title, "Group 2");
}

#[test]
fn sorts_groups_by_order() {
    let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
    let mut f = Fixture::new();
    let extension_id = "test.extension";
    f.register(config("group1", Some("Group 1"), Some(2.0), "test.setting1", "value1", "Setting 1", extension_id));
    f.register(config("group2", Some("Group 2"), Some(1.0), "test.setting2", "value2", "Setting 2", extension_id));

    let default_settings = f.default_settings();
    let groups = default_settings.get_registered_groups();

    let extension_groups = groups_of(&groups, extension_id);

    assert_eq!(extension_groups.len(), 2);
    assert_eq!(extension_groups[0].title, "Group 2");
    assert_eq!(extension_groups[1].title, "Group 1");
}
