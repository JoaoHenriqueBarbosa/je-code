//! Porte de `src/vs/workbench/services/configuration/test/browser/configuration.test.ts` (suíte `DefaultConfiguration`),
//! caso a caso e na mesma ordem (9 casos).
//!
//! Notas de tradução:
//! - **`ensureNoDisposablesAreLeakedInTestSuite()` e `restoreConfigurationRegistryAfterSuite()`.** O primeiro é o `async_test`,
//!   que entrega o store dos disposables ao corpo; o segundo é o [`RegistryGuard`], criado antes do `setup` e que esvazia e
//!   restaura o registro ao ser descartado (o registro é um singleton por thread e cada teste roda na sua).
//! - **`BrowserWorkbenchEnvironmentService(..., { configurationDefaults })` e `TestEnvironmentService`.** O serviço de ambiente do
//!   navegador não foi portado: o `DefaultConfiguration` recebe só o `configurationDefaults` (`Some` no primeiro e no quarto caso,
//!   `None` onde o original usa o `TestEnvironmentService`, que não os tem).
//! - **`localStorage.setItem`.** É `local_storage_set`, o mapa por thread que a produção consulta.
//! - **`ConfigurationCache`.** Implementa `IConfigurationCache` sobre um mapa; `read`, `write` e `remove` completam na hora.
//! - **`Event.toPromise(onDidChangeConfiguration)`.** É `Event::to_promise(None)`, assinado antes de registrar a configuração.

#[path = "../../je-platform/tests/common/mod.rs"]
mod common;

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use common::files::async_test;
use common::{RegistryGuard, defaults, node, obj, prop, s};
use je_base::dynamic::Dynamic;
use je_base::promise::Promise;
use je_base::uri::URI;
use je_platform::configuration::registry::configuration_registry;
use je_platform::log::NullLogService;
use je_workbench::services::configuration::browser::configuration::{DEFAULT_OVERRIDES_CACHE_EXISTS_KEY, DefaultConfiguration, local_storage_set};
use je_workbench::services::configuration::common::configuration::{ConfigurationKey, ConfigurationKeyType, IConfigurationCache};

#[derive(Default)]
struct ConfigurationCache {
    cache: RefCell<HashMap<String, String>>,
}

impl ConfigurationCache {
    fn entry(key: &ConfigurationKey) -> String {
        format!("{}:{}", key.type_.as_str(), key.key)
    }
}

impl IConfigurationCache for ConfigurationCache {
    fn needs_caching(&self, _resource: &URI) -> bool {
        false
    }

    fn read(&self, key: &ConfigurationKey) -> Promise<String> {
        Promise::resolved(self.cache.borrow().get(&Self::entry(key)).cloned().unwrap_or_default())
    }

    fn write(&self, key: &ConfigurationKey, content: &str) -> Promise<()> {
        self.cache.borrow_mut().insert(Self::entry(key), content.to_owned());
        Promise::resolved(())
    }

    fn remove(&self, key: &ConfigurationKey) -> Promise<()> {
        self.cache.borrow_mut().remove(&Self::entry(key));
        Promise::resolved(())
    }
}

fn cache_key() -> ConfigurationKey {
    ConfigurationKey { type_: ConfigurationKeyType::Defaults, key: "default-configurationDefaultsOverrides".to_owned() }
}

fn override_node(property: &str) -> je_platform::configuration::registry::ConfigurationNode {
    node("test.configurationDefaultsOverride", vec![(property, prop("{ 'type': 'string', 'default': 'defaultValue' }"))])
}

/// `suiteSetup` (guarda do registro) e `setup` (cache novo e a configuração `test.configurationDefaultsOverride`).
fn setup() -> (RegistryGuard, Rc<ConfigurationCache>) {
    let registry_guard = RegistryGuard::new();
    let configuration_cache = Rc::new(ConfigurationCache::default());
    configuration_registry().register_configuration(override_node("test.configurationDefaultsOverride"));
    (registry_guard, configuration_cache)
}

fn new_default_configuration(
    d: &Rc<je_base::lifecycle::DisposableStore>,
    configuration_cache: &Rc<ConfigurationCache>,
    configuration_defaults: Option<Dynamic>,
) -> Rc<DefaultConfiguration> {
    let cache: Rc<dyn IConfigurationCache> = configuration_cache.clone();
    d.add_rc(DefaultConfiguration::new("default", cache, configuration_defaults, NullLogService::shared()))
}

fn env_overrides() -> Option<Dynamic> {
    Some(obj(vec![("test.configurationDefaultsOverride", s("envOverrideValue"))]))
}

fn cached_overrides() -> String {
    r#"{"test.configurationDefaultsOverride":"overrideValue"}"#.to_owned()
}

#[test]
fn configuration_default_overrides_are_read_from_environment() {
    async_test(|d| async move {
        let (_registry, configuration_cache) = setup();
        let test_object = new_default_configuration(&d, &configuration_cache, env_overrides());
        test_object.initialize().await.unwrap();
        assert_eq!(test_object.configuration_model().get_value("test.configurationDefaultsOverride"), s("envOverrideValue"));
    });
}

#[test]
fn configuration_default_overrides_are_read_from_cache() {
    async_test(|d| async move {
        let (_registry, configuration_cache) = setup();
        local_storage_set(DEFAULT_OVERRIDES_CACHE_EXISTS_KEY, "yes");
        configuration_cache.write(&cache_key(), &cached_overrides()).await.unwrap();
        let test_object = new_default_configuration(&d, &configuration_cache, None);

        let actual = test_object.initialize().await.unwrap();

        assert_eq!(actual.get_value("test.configurationDefaultsOverride"), s("overrideValue"));
        assert_eq!(test_object.configuration_model().get_value("test.configurationDefaultsOverride"), s("overrideValue"));
    });
}

#[test]
fn configuration_default_overrides_are_not_read_from_cache_when_model_is_read_before_initialize() {
    async_test(|d| async move {
        let (_registry, configuration_cache) = setup();
        local_storage_set(DEFAULT_OVERRIDES_CACHE_EXISTS_KEY, "yes");
        configuration_cache.write(&cache_key(), &cached_overrides()).await.unwrap();
        let test_object = new_default_configuration(&d, &configuration_cache, None);
        assert_eq!(test_object.configuration_model().get_value("test.configurationDefaultsOverride"), Dynamic::Undefined);
    });
}

#[test]
fn configuration_default_overrides_read_from_cache_override_environment() {
    async_test(|d| async move {
        let (_registry, configuration_cache) = setup();
        local_storage_set(DEFAULT_OVERRIDES_CACHE_EXISTS_KEY, "yes");
        configuration_cache.write(&cache_key(), &cached_overrides()).await.unwrap();
        let test_object = new_default_configuration(&d, &configuration_cache, env_overrides());

        let actual = test_object.initialize().await.unwrap();

        assert_eq!(actual.get_value("test.configurationDefaultsOverride"), s("overrideValue"));
    });
}

#[test]
fn configuration_default_overrides_are_read_from_cache_when_default_configuration_changed() {
    async_test(|d| async move {
        let (_registry, configuration_cache) = setup();
        local_storage_set(DEFAULT_OVERRIDES_CACHE_EXISTS_KEY, "yes");
        configuration_cache.write(&cache_key(), &cached_overrides()).await.unwrap();
        let test_object = new_default_configuration(&d, &configuration_cache, None);
        test_object.initialize().await.unwrap();

        let promise = test_object.on_did_change_configuration().to_promise(None);
        configuration_registry().register_configuration(override_node("test.configurationDefaultsOverride1"));

        let actual = promise.await.unwrap().defaults;
        assert_eq!(actual.get_value("test.configurationDefaultsOverride"), s("overrideValue"));
    });
}

#[test]
fn configuration_default_overrides_are_not_read_from_cache_after_reload() {
    async_test(|d| async move {
        let (_registry, configuration_cache) = setup();
        local_storage_set(DEFAULT_OVERRIDES_CACHE_EXISTS_KEY, "yes");
        configuration_cache.write(&cache_key(), &cached_overrides()).await.unwrap();
        let test_object = new_default_configuration(&d, &configuration_cache, None);

        test_object.initialize().await.unwrap();
        let actual = test_object.reload();

        assert_eq!(actual.get_value("test.configurationDefaultsOverride"), s("defaultValue"));
    });
}

#[test]
fn cache_is_reset_after_reload() {
    async_test(|d| async move {
        let (_registry, configuration_cache) = setup();
        local_storage_set(DEFAULT_OVERRIDES_CACHE_EXISTS_KEY, "yes");
        configuration_cache.write(&cache_key(), &cached_overrides()).await.unwrap();
        let test_object = new_default_configuration(&d, &configuration_cache, None);

        test_object.initialize().await.unwrap();
        test_object.reload();

        assert_eq!(configuration_cache.read(&cache_key()).await.unwrap(), "");
    });
}

#[test]
fn configuration_default_overrides_are_written_in_cache() {
    async_test(|d| async move {
        let (_registry, configuration_cache) = setup();
        let test_object = new_default_configuration(&d, &configuration_cache, None);
        test_object.initialize().await.unwrap();
        test_object.reload();
        let promise = test_object.on_did_change_configuration().to_promise(None);
        configuration_registry().register_default_configurations(&[defaults(obj(vec![("test.configurationDefaultsOverride", s("newoverrideValue"))]))]);
        promise.await.unwrap();

        let actual = configuration_cache.read(&cache_key()).await.unwrap();
        assert_eq!(actual, r#"{"test.configurationDefaultsOverride":"newoverrideValue"}"#);
    });
}

#[test]
fn configuration_default_overrides_are_removed_from_cache_if_there_are_no_overrides() {
    async_test(|d| async move {
        let (_registry, configuration_cache) = setup();
        let test_object = new_default_configuration(&d, &configuration_cache, None);
        test_object.initialize().await.unwrap();
        let promise = test_object.on_did_change_configuration().to_promise(None);
        configuration_registry().register_configuration(override_node("test.configurationDefaultsOverride1"));
        promise.await.unwrap();

        assert_eq!(configuration_cache.read(&cache_key()).await.unwrap(), "");
    });
}
