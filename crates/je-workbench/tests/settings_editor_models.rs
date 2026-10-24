//! Testes de `AbstractSettingsModel`, `SettingsEditorModel`, `WorkspaceConfigurationEditorModel` e `Settings2EditorModel`
//! (`src/vs/workbench/services/preferences/common/preferencesModels.ts`). O oráculo não tem teste desses modelos: os casos
//! seguem o comportamento lido do original (as posições que o `parse` calcula, a invalidação por mudança de conteúdo, a
//! remoção de duplicados entre grupos de resultado e o descarte do primeiro grupo no `filterGroups`).
//!
//! Notas de tradução:
//! - O `ITextEditorModel` que o `SettingsEditorModel` recebe é um modelo de texto já resolvido (`ResolvedModel`), numa
//!   referência imortal.
//! - O `Registry` de configurações é por thread: o [`RegistryGuard`] restaura o registro de cada caso.

#[path = "../../je-platform/tests/common/mod.rs"]
mod common;
#[allow(dead_code)]
#[path = "../../je-editor/tests/support/test_language_configuration_service.rs"]
mod test_language_configuration_service;
#[allow(dead_code)]
#[path = "../../je-editor/tests/support/test_text_model.rs"]
mod test_text_model;

use std::any::Any;
use std::cell::{Cell, RefCell};
use std::rc::Rc;

use common::{RegistryGuard, prop};
use je_base::dynamic::Dynamic;
use je_base::event::Event;
use je_base::lifecycle::{IDisposable, Reference};
use je_base::promise::Promise;
use je_editor::model::{RelaxedTextModelCreationOptions, TextSnapshot};
use je_editor::range::Range;
use je_editor::text_model::{TextModel, TextSource};
use je_platform::configuration::ConfigurationTarget;
use je_platform::configuration::registry::{ConfigurationNode, ConfigurationNodeData, configuration_registry};
use je_platform::configuration::service::IConfigurationService;
use je_platform::configuration::test_configuration_service::TestConfigurationService;
use je_workbench::common::editor::editor_model::IEditorModel;
use je_workbench::services::files_configuration::files_configuration_service::Readonly;
use je_workbench::services::preferences::preferences::{ISearchResult, ISearchResultGroup, ISettingMatch, ISettingMatcherResult, ISettingsEditorModel, SettingMatchType};
use je_workbench::services::preferences::preferences_models::{DefaultSettings, Settings2EditorModel, SettingsEditorModel, WorkspaceConfigurationEditorModel};
use je_workbench::services::textmodel_resolver::resolver_service::ITextEditorModel;
use test_text_model::create_text_model_with;

struct ResolvedModel {
    text_editor_model: Rc<TextModel>,
}

impl IDisposable for ResolvedModel {
    fn dispose(&self) {}
}

impl IEditorModel for ResolvedModel {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn on_will_dispose(&self) -> Event<()> {
        Event::none()
    }

    fn resolve(&self) -> Promise<()> {
        Promise::resolved(())
    }

    fn is_resolved(&self) -> bool {
        true
    }

    fn is_disposed(&self) -> bool {
        false
    }
}

impl ITextEditorModel for ResolvedModel {
    fn text_editor_model(&self) -> Option<Rc<TextModel>> {
        Some(self.text_editor_model.clone())
    }

    fn is_readonly(&self) -> Readonly {
        Readonly::Bool(false)
    }

    fn create_snapshot(&self) -> Option<Box<dyn TextSnapshot>> {
        None
    }
}

fn reference_of(content: &str) -> (Rc<TextModel>, Reference<Rc<dyn ITextEditorModel>>) {
    let text_model = create_text_model_with(content, None, RelaxedTextModelCreationOptions::default(), None);
    let object: Rc<dyn ITextEditorModel> = Rc::new(ResolvedModel { text_editor_model: text_model.clone() });
    (text_model, Reference::immortal(object))
}

fn range(start_line: i32, start_column: i32, end_line: i32, end_column: i32) -> Range {
    Range { start_line_number: start_line, start_column, end_line_number: end_line, end_column }
}

fn result_group(id: &str, order: f64, keys: &[&str]) -> ISearchResultGroup {
    let filter_matches = keys
        .iter()
        .map(|key| {
            let model_setting = je_workbench::services::preferences::preferences::ISetting { key: (*key).to_owned(), ..Default::default() };
            ISettingMatch::new(Rc::new(model_setting), None, SettingMatchType::None, 0.0, 0.0)
        })
        .collect();
    ISearchResultGroup { id: id.to_owned(), label: id.to_owned(), result: Rc::new(RefCell::new(ISearchResult { filter_matches, exact_match: false })), order }
}

#[test]
fn settings_editor_model_parses_keys_values_and_ranges() {
    let (text_model, reference) = reference_of("{\n  \"a\": 1,\n  \"b\": [1, 2]\n}");
    let model = SettingsEditorModel::new(reference, ConfigurationTarget::User);

    let groups = model.settings_groups();
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].range, range(1, 1, 4, 1));
    let settings = &groups[0].sections[0].settings;
    assert_eq!(settings.iter().map(|setting| setting.key.as_str()).collect::<Vec<_>>(), vec!["a", "b"]);
    assert_eq!(settings[0].key_range, range(2, 4, 2, 6));
    assert_eq!(settings[0].value_range, range(2, 8, 2, 9));
    assert_eq!(settings[0].range, range(2, 3, 2, 9));
    assert!(matches!(settings[0].value, Dynamic::Number(value) if value == 1.0));
    assert_eq!(settings[1].value_range, range(3, 8, 3, 14));
    assert!(matches!(&settings[1].value, Dynamic::Array(items) if items.len() == 2));
    assert_eq!(model.get_preference("b").map(|setting| setting.range), Some(range(3, 3, 3, 14)));
    assert!(model.get_preference("c").is_none());
    text_model.dispose();
}

#[test]
fn settings_editor_model_reparses_after_a_content_change() {
    let (text_model, reference) = reference_of("{ \"a\": 1 }");
    let model = SettingsEditorModel::new(reference, ConfigurationTarget::User);
    let changes = Rc::new(Cell::new(0));
    let counter = changes.clone();
    model.on_did_change_groups().listen(move |_| counter.set(counter.get() + 1));
    assert_eq!(model.settings_groups()[0].sections[0].settings.len(), 1);

    text_model.set_value(TextSource::from("{ \"a\": 1, \"b\": 2 }"));

    assert_eq!(changes.get(), 1);
    assert_eq!(model.settings_groups()[0].sections[0].settings.len(), 2);
    assert_eq!(model.content(), "{ \"a\": 1, \"b\": 2 }");
    text_model.dispose();
}

#[test]
fn settings_editor_model_without_settings_has_no_group() {
    let (text_model, reference) = reference_of("[]");
    let model = SettingsEditorModel::new(reference, ConfigurationTarget::User);
    assert!(model.settings_groups().is_empty());
    text_model.dispose();
}

#[test]
fn settings_editor_model_nests_language_overrides() {
    let (text_model, reference) = reference_of("{\n  \"[json]\": {\n    \"editor.tabSize\": 2\n  }\n}");
    let model = SettingsEditorModel::new(reference, ConfigurationTarget::User);

    let groups = model.settings_groups();
    let settings = &groups[0].sections[0].settings;
    assert_eq!(settings.len(), 1);
    assert_eq!(settings[0].key, "[json]");
    let overrides = settings[0].overrides.as_ref().expect("as sobrescritas");
    assert_eq!(overrides.len(), 1);
    assert_eq!(overrides[0].key, "editor.tabSize");
    assert_eq!(overrides[0].override_of.as_ref().map(|setting| setting.key.as_str()), Some("[json]"));
    assert_eq!(overrides[0].value_range, range(3, 23, 3, 24));
    text_model.dispose();
}

#[test]
fn workspace_configuration_model_reads_the_settings_property() {
    let (text_model, reference) = reference_of("{\n  \"folders\": [],\n  \"settings\": {\n    \"x\": true\n  }\n}");
    let model = WorkspaceConfigurationEditorModel::new(reference, ConfigurationTarget::Workspace);

    let groups = model.settings_groups();
    assert_eq!(groups[0].sections[0].settings.iter().map(|setting| setting.key.as_str()).collect::<Vec<_>>(), vec!["x"]);
    let configuration_groups = model.configuration_groups();
    assert_eq!(configuration_groups[0].sections[0].settings.iter().map(|setting| setting.key.as_str()).collect::<Vec<_>>(), vec!["folders", "settings"]);
    text_model.dispose();
}

#[test]
fn update_result_group_removes_duplicates_preferring_earlier_groups() {
    let (text_model, reference) = reference_of("{ \"a\": 1, \"b\": 2 }");
    let model = SettingsEditorModel::new(reference, ConfigurationTarget::User);
    assert!(model.update_result_group("later", None).is_none());

    let later = result_group("later", 1.0, &["a", "b"]);
    model.update_result_group("later", Some(later.clone()));
    let earlier = result_group("earlier", 0.0, &["a"]);
    let result = model.update_result_group("earlier", Some(earlier.clone())).expect("há resultado");

    assert_eq!(later.result.borrow().filter_matches.iter().map(|m| m.setting.key.as_str()).collect::<Vec<_>>(), vec!["b"]);
    assert_eq!(earlier.result.borrow().filter_matches.len(), 1);
    assert_eq!(result.all_groups.len(), 1);
    assert_eq!(result.filtered_groups.len(), 1);
    assert_eq!(result.filtered_groups[0].sections[0].settings.len(), 2);

    assert!(model.update_result_group("earlier", None).is_some());
    assert!(model.update_result_group("later", None).is_none());
    text_model.dispose();
}

#[test]
fn settings_editor_model_filters_by_group_and_by_setting() {
    let (text_model, reference) = reference_of("{ \"a\": 1, \"b\": 2 }");
    let model = SettingsEditorModel::new(reference, ConfigurationTarget::User);
    let matcher = |setting: &je_workbench::services::preferences::preferences::ISetting, _: &_| {
        (setting.key == "b").then(|| ISettingMatcherResult { matches: vec![range(1, 1, 1, 2)], match_type: SettingMatchType::ContiguousQueryInSettingId, key_match_score: 2.0, score: 3.0 })
    };

    let only_matcher = model.filter_settings("b", &|_| false, &matcher);
    assert_eq!(only_matcher.len(), 1);
    assert_eq!(only_matcher[0].setting.key, "b");
    assert_eq!((only_matcher[0].key_match_score, only_matcher[0].score), (2.0, 3.0));
    assert_eq!(only_matcher[0].matches, Some(vec![range(1, 1, 1, 2)]));

    let whole_group = model.filter_settings("b", &|_| true, &matcher);
    assert_eq!(whole_group.len(), 2);
    assert_eq!(whole_group[0].matches, None);
    assert_eq!(whole_group[0].match_type, SettingMatchType::None);
    text_model.dispose();
}

fn registered_string_setting(key: &str) -> ConfigurationNode {
    ConfigurationNode::new(
        ConfigurationNodeData { id: Some(format!("{key}.node")), title: Some(key.to_owned()), type_: Some(Dynamic::string("object")), ..ConfigurationNodeData::default() }
            .with_properties(vec![(key, prop("{ 'type': 'string', 'default': 'x', 'description': 'd' }"))]),
    )
}

#[test]
fn settings2_editor_model_skips_the_commonly_used_group_when_filtering() {
    let _registry = RegistryGuard::new();
    let node = registered_string_setting("test.settings2.a");
    configuration_registry().register_configuration(node.clone());
    let configuration_service: Rc<dyn IConfigurationService> = TestConfigurationService::new(None);
    let default_settings = DefaultSettings::new(vec!["test.settings2.a".to_owned()], ConfigurationTarget::User, configuration_service.clone());
    let model = Settings2EditorModel::new(default_settings.clone(), configuration_service);

    let groups = model.settings_groups();
    let count_of = |groups: &[je_workbench::services::preferences::preferences::ISettingsGroup]| {
        groups.iter().flat_map(|group| group.sections.iter()).flat_map(|section| section.settings.iter()).filter(|setting| setting.key == "test.settings2.a").count()
    };
    assert_eq!(count_of(&groups), 2);

    let matches = model.filter_settings("", &|_| false, &|setting, _| {
        (setting.key == "test.settings2.a").then(|| ISettingMatcherResult { matches: Vec::new(), match_type: SettingMatchType::None, key_match_score: 0.0, score: 0.0 })
    });
    assert_eq!(matches.len(), 1);
    assert!(model.get_preference("test.settings2.a").is_some());

    model.dispose();
    default_settings.dispose();
    configuration_registry().deregister_configurations(&[node]);
}

#[test]
fn settings2_editor_model_appends_additional_groups() {
    let _registry = RegistryGuard::new();
    let configuration_service: Rc<dyn IConfigurationService> = TestConfigurationService::new(None);
    let default_settings = DefaultSettings::new(Vec::new(), ConfigurationTarget::User, configuration_service.clone());
    let model = Settings2EditorModel::new(default_settings.clone(), configuration_service);
    let before = model.settings_groups().len();

    let mut group = model.settings_groups()[0].clone();
    group.id = "additional".to_owned();
    model.set_additional_groups(vec![group]);

    let groups = model.settings_groups();
    assert_eq!(groups.len(), before + 1);
    assert_eq!(groups.last().map(|group| group.id.as_str()), Some("additional"));
    model.dispose();
    default_settings.dispose();
}

#[test]
fn settings2_editor_model_fires_on_schema_change() {
    let _registry = RegistryGuard::new();
    let configuration_service: Rc<dyn IConfigurationService> = TestConfigurationService::new(None);
    let default_settings = DefaultSettings::new(Vec::new(), ConfigurationTarget::User, configuration_service.clone());
    let model = Settings2EditorModel::new(default_settings.clone(), configuration_service);
    let fired = Rc::new(Cell::new(0));
    let counter = fired.clone();
    model.on_did_change_groups().listen(move |_| counter.set(counter.get() + 1));

    let node = registered_string_setting("test.settings2.schema");
    configuration_registry().register_configuration(node.clone());

    assert!(fired.get() >= 1);
    assert!(model.get_preference("test.settings2.schema").is_some());
    configuration_registry().deregister_configurations(&[node]);
    model.dispose();
    default_settings.dispose();
}

#[test]
#[should_panic]
fn settings2_editor_model_update_is_not_supported() {
    let _registry = RegistryGuard::new();
    let configuration_service: Rc<dyn IConfigurationService> = TestConfigurationService::new(None);
    let default_settings = DefaultSettings::new(Vec::new(), ConfigurationTarget::User, configuration_service.clone());
    let model = Settings2EditorModel::new(default_settings, configuration_service);
    model.update_result_group("x", None);
}
