//! Auxiliares dos testes portados: o `utils.ts` do VS Code (via o `common` do `je-base`), o
//! `assert.deepStrictEqual` sobre valores dinâmicos, o
//! `configurationRegistryTestUtils.ts` e construtores de valores no estilo dos literais do original.
//! (O `testWorkspace.ts` é `je_platform::workspace::new_test_workspace`.)
#![allow(dead_code)]

use std::cell::RefCell;
use std::rc::Rc;

use je_base::JsString;
use je_base::dynamic::{Dynamic, ObjectRef};
use je_base::json;
use je_base::map::ResourceMap;
use je_base::path::{Platform, ProcessEnvironment};
use je_base::uri::URI;
use je_platform::configuration::models::{Configuration, ConfigurationModel, ConfigurationModelParser, Raw};
use je_platform::configuration::registry::{
    ConfigurationDefaults, ConfigurationNode, ConfigurationNodeData, ConfigurationRegistry, PropertySchema, configuration_registry,
};
use je_platform::configuration::Overrides;
use je_platform::js::from_json;
use je_platform::log::NullLogService;
use je_platform::workspace::{WorkspaceFolder, to_workspace_folder};

pub mod contextkey;
pub mod keybinding;

#[path = "../../../je-base/tests/common/mod.rs"]
mod base;

#[allow(unused_imports)]
pub use base::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite, run_with_faked_timers};

/// Um literal no estilo do TypeScript: JSON com aspas simples. Não tem `undefined`: quem precisa monta
/// o valor na mão.
pub fn j(text: &str) -> Dynamic {
    let mut errors = Vec::new();
    let value = json::parse_str(&text.replace('\'', "\""), &mut errors).unwrap_or_else(|| panic!("literal inválido: {text}"));
    assert!(errors.is_empty(), "literal inválido: {text}");
    from_json(&value)
}

pub fn s(text: &str) -> Dynamic {
    Dynamic::string(text)
}

pub fn n(value: f64) -> Dynamic {
    Dynamic::Number(value)
}

pub fn obj<K: Into<JsString>>(entries: Vec<(K, Dynamic)>) -> Dynamic {
    Dynamic::object(entries)
}

pub fn arr(items: Vec<Dynamic>) -> Dynamic {
    Dynamic::array(items)
}

pub fn strs(items: &[&str]) -> Vec<JsString> {
    items.iter().map(|i| JsString::from(*i)).collect()
}

pub fn jstr(text: &str) -> JsString {
    JsString::from(text)
}

pub fn dyn_strs(items: &[&str]) -> Dynamic {
    Dynamic::array(items.iter().map(|i| Dynamic::string(i)))
}

/// `assert.deepStrictEqual` sobre valores dinâmicos: a mesma forma e os mesmos valores (a ordem das
/// chaves não conta; protótipos não existem no `Dynamic`).
pub fn deep_strict_equal(a: &Dynamic, b: &Dynamic) -> bool {
    match (a, b) {
        (Dynamic::Array(x), Dynamic::Array(y)) => {
            let (x, y) = (x.to_vec(), y.to_vec());
            x.len() == y.len() && x.iter().zip(&y).all(|(p, q)| deep_strict_equal(p, q))
        }
        (Dynamic::Object(x), Dynamic::Object(y)) => {
            let (ex, ey) = (x.entries(), y.entries());
            ex.len() == ey.len() && ex.iter().all(|(k, v)| y.get(k).is_some_and(|w| deep_strict_equal(v, &w)))
        }
        (Dynamic::Number(x), Dynamic::Number(y)) if x.is_nan() && y.is_nan() => true,
        _ => a.strict_equals(b),
    }
}

#[track_caller]
pub fn assert_deep_strict_equal(actual: &Dynamic, expected: &Dynamic) {
    assert!(deep_strict_equal(actual, expected), "deepStrictEqual falhou\n  atual:    {actual:?}\n  esperado: {expected:?}");
}

/// `JSON.stringify(value)`.
pub fn stringify(value: &Dynamic) -> String {
    je_base::objects::safe_stringify(value).expect("JSON.stringify devolveu undefined")
}

/// O `JSON.stringify` de um bloco de overrides (`IOverrides[]`).
pub fn stringify_overrides(overrides: &[Overrides]) -> String {
    stringify(&Dynamic::array(overrides.iter().map(Overrides::to_dynamic)))
}

pub fn uri_file(path: &str) -> URI {
    URI::file(path, Platform::Posix)
}

/// `join` do `path` do ambiente de teste (Linux).
pub fn join(paths: &[&str]) -> String {
    je_base::path::PathModule::native(&ProcessEnvironment).join(paths)
}

pub fn null_log() -> je_platform::log::LogServiceRef {
    NullLogService::shared()
}

/// `new ConfigurationModel(contents, keys, overrides, raw, new NullLogService())`.
pub fn model(contents: Dynamic, keys: &[&str], overrides: Vec<Overrides>, raw: Option<Raw>) -> ConfigurationModel {
    ConfigurationModel::new(contents, strs(keys), overrides, raw, null_log())
}

/// `{ identifiers, contents, keys }`.
pub fn ov(identifiers: &[&str], contents: Dynamic, keys: &[&str]) -> Overrides {
    Overrides::new(identifiers, contents, keys)
}

pub fn empty_model() -> ConfigurationModel {
    ConfigurationModel::create_empty_model(null_log())
}

/// `parser.parse(JSON.stringify(obj))` e o modelo resultante.
pub fn to_configuration_model(content: &str) -> ConfigurationModel {
    let mut parser = ConfigurationModelParser::new("test", null_log());
    parser.parse(Some(&content.replace('\'', "\"")), None);
    parser.configuration_model()
}

/// `TestConfiguration` de `configurationModels.test.ts`.
pub fn test_configuration(
    default: ConfigurationModel,
    policy: ConfigurationModel,
    application: ConfigurationModel,
    local_user: ConfigurationModel,
    remote_user: Option<ConfigurationModel>,
) -> Configuration {
    Configuration::new(
        default,
        policy,
        application,
        local_user,
        remote_user.unwrap_or_else(empty_model),
        empty_model(),
        ResourceMap::new(),
        empty_model(),
        ResourceMap::new(),
        null_log(),
    )
}

pub fn empty_test_configuration() -> Configuration {
    test_configuration(empty_model(), empty_model(), empty_model(), empty_model(), None)
}

pub fn workspace_folder(index: usize, name: &str, uri: URI) -> WorkspaceFolder {
    WorkspaceFolder::new(uri, name, index, None)
}

pub fn folder_of(uri: URI) -> WorkspaceFolder {
    let name = uri.path().to_string_lossy().rsplit('/').next().unwrap_or_default().to_string();
    to_workspace_folder(&uri, name)
}

// ---- Nós e propriedades de configuração ----

/// Uma propriedade a partir de um literal `{ 'type': 'string', 'default': 'x' }`.
pub fn prop(literal: &str) -> PropertySchema {
    match j(literal) {
        Dynamic::Object(o) => PropertySchema::new(o),
        other => panic!("a propriedade deve ser um objeto: {other:?}"),
    }
}

/// Uma propriedade com um campo do esquema que é valor dinâmico na mão (`scope`, `experiment`...).
pub fn prop_with(literal: &str, extra: Vec<(&str, Dynamic)>) -> PropertySchema {
    let property = prop(literal);
    for (k, v) in extra {
        property.set(k, v);
    }
    property
}

/// Um nó `{ id, type: 'object', properties }`.
pub fn node(id: &str, properties: Vec<(&str, PropertySchema)>) -> ConfigurationNode {
    ConfigurationNode::new(ConfigurationNodeData { id: Some(id.to_string()), type_: Some(Dynamic::string("object")), ..Default::default() }.with_properties(properties))
}

/// Um nó completo como os de `registerConfiguration({ id, order, title, type, properties })`.
pub fn node_full(id: &str, order: f64, title: &str, properties: Vec<(&str, PropertySchema)>) -> ConfigurationNode {
    ConfigurationNode::new(
        ConfigurationNodeData { id: Some(id.to_string()), order: Some(order), title: Some(title.to_string()), type_: Some(Dynamic::string("object")), ..Default::default() }
            .with_properties(properties),
    )
}

/// `{ overrides: {...} }` (sem origem).
pub fn defaults(overrides: Dynamic) -> ConfigurationDefaults {
    ConfigurationDefaults::new(overrides, None)
}

// ---- configurationRegistryTestUtils.ts ----

/// O que `restoreConfigurationRegistryAfterSuite` faz com `suiteSetup`/`suiteTeardown`: grava o registro
/// ao criar e o restaura ao descartar. `reset()` esvazia o registro (o `setup` e o `teardown` de cada
/// teste).
pub struct RegistryGuard {
    restore: RefCell<Option<Box<dyn FnOnce()>>>,
    registry: Rc<ConfigurationRegistry>,
}

impl RegistryGuard {
    pub fn new() -> Self {
        let registry = configuration_registry();
        Self { restore: RefCell::new(Some(record_configuration_registry())), registry }
    }

    /// `reset()`.
    pub fn reset(&self) {
        empty_configuration_registry(&self.registry);
    }
}

impl Drop for RegistryGuard {
    fn drop(&mut self) {
        // teardown(() => reset()) e depois suiteTeardown(() => restore())
        if std::thread::panicking() {
            return;
        }
        self.reset();
        if let Some(restore) = self.restore.borrow_mut().take() {
            restore();
        }
    }
}

fn empty_configuration_registry(registry: &ConfigurationRegistry) {
    // Deregistering a default override resets the default of the setting it overrides, so do it while the settings are registered
    registry.deregister_default_configurations(&registry.get_registered_default_configurations());
    registry.deregister_configurations(&registry.get_configurations());
}

struct HiddenSetting {
    node: ConfigurationNode,
    key: JsString,
    property: PropertySchema,
}

/// `getForgettableHiddenSettings`.
fn get_forgettable_hidden_settings(registry: &ConfigurationRegistry, configurations: &[ConfigurationNode]) -> Vec<HiddenSetting> {
    fn add_nodes(node: &ConfigurationNode, nodes: &mut Vec<ConfigurationNode>) {
        nodes.push(node.clone());
        for child in &node.all_of {
            add_nodes(child, nodes);
        }
    }
    let mut nodes = Vec::new();
    for configuration in configurations {
        add_nodes(configuration, &mut nodes);
    }

    let visible = registry.get_configuration_properties();
    let mirrored = registry.get_agent_host_sync_configurations();
    let mut result = Vec::new();
    for (key, property) in registry.get_excluded_configuration_properties().entries() {
        let has_experiment = property.get("experiment").is_truthy_dyn();
        let has_agent_host = property.get("agentHost").is_truthy_dyn() && mirrored.contains_key(&key);
        if visible.contains(&key) || !(has_experiment || has_agent_host) {
            continue;
        }
        // The registry does not expose which node registered a setting, but it records the node's section on the setting.
        let section = property.section();
        let found = nodes.iter().find(|node| {
            section.as_ref().is_some_and(|s| node.id == s.id && node.title == s.title && node.order == s.order && node.extension_info == s.extension_info)
                || (section.is_none() && node.id.is_none() && node.title.is_none() && node.order.is_none() && node.extension_info.is_none())
        });
        if let Some(node) = found {
            result.push(HiddenSetting { node: node.clone(), key, property });
        }
    }
    result
}

/// `recordConfigurationRegistry()`: devolve a função que esvazia o registro e registra de novo o que
/// havia.
pub fn record_configuration_registry() -> Box<dyn FnOnce()> {
    let registry = configuration_registry();
    let configurations = registry.get_configurations();
    let defaults = registry.get_registered_default_configurations();
    let hidden_settings = get_forgettable_hidden_settings(&registry, &configurations);

    Box::new(move || {
        let registry = configuration_registry();
        empty_configuration_registry(&registry);
        for HiddenSetting { node, key, property } in hidden_settings {
            // Registration reads the declared default from `default`, which a default override may have replaced
            property.set("default", property.default_default_value());
            node.set_property(&key, property);
        }
        // The recorded configurations were validated when they were first registered
        registry.register_configurations(&configurations, false);
        registry.register_default_configurations(&defaults);
    })
}

trait TruthyDyn {
    fn is_truthy_dyn(&self) -> bool;
}

impl TruthyDyn for Dynamic {
    fn is_truthy_dyn(&self) -> bool {
        use je_base::dynamic::Truthy;
        self.is_truthy()
    }
}

/// `ObjectRef` vazio como valor dinâmico (`{}`).
pub fn empty_obj() -> Dynamic {
    Dynamic::Object(ObjectRef::new())
}

/// `{ keys, overrides }` com os overrides no formato `[['a', ['1', '2']]]`.
pub fn change(keys: &[&str], overrides: &[(&str, &[&str])]) -> je_platform::configuration::ConfigurationChange {
    je_platform::configuration::ConfigurationChange {
        keys: strs(keys),
        overrides: overrides.iter().map(|(identifier, keys)| (jstr(identifier), strs(keys))).collect(),
    }
}

/// `{ value, override, merged, overrides }` esperado de um `inspect`.
pub fn insp(value: Dynamic, override_value: Dynamic, merged: Dynamic, overrides: Dynamic) -> Dynamic {
    obj(vec![("value", value), ("override", override_value), ("merged", merged), ("overrides", overrides)])
}

/// `[{ identifiers: [...], value }, ...]`.
pub fn ovs(items: Vec<(&[&str], Dynamic)>) -> Dynamic {
    Dynamic::array(items.into_iter().map(|(identifiers, value)| obj(vec![("identifiers", dyn_strs(identifiers)), ("value", value)])))
}

pub const U: Dynamic = Dynamic::Undefined;

/// Um `IInspectValue` opcional como valor dinâmico (`undefined` se não há).
pub fn inspect_dynamic(value: Option<je_platform::configuration::models::InspectValue>) -> Dynamic {
    value.map_or(Dynamic::Undefined, |v| v.to_dynamic())
}

pub mod files;
pub mod disk;

pub mod watcher;

pub mod user_data_sync_client;
