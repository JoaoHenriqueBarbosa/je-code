//! Porte de `src/vs/platform/configuration/test/common/configurationService.test.ts` (suíte `ConfigurationService.test.ts`), caso a caso e na mesma ordem.
//!
//! Divergências: toda a suíte roda sob o relógio virtual (`async_test`), porque o código assíncrono do
//! porte precisa do executor; no original só os casos de `runWithFakedTimers` o têm. O registro global de
//! configurações é um singleton por thread: o `registeredBefore` e o `teardown` do original são o
//! `RegistryGuard`, que registra o que havia e restaura ao fim do teste. `getValue<T>()` devolve um
//! valor dinâmico, lido com `get_str`. `assert.fail('Should fail with error')` dentro do `try` é
//! `assert!(resultado.is_err())`.

mod common;

use je_platform::files::i_file_service::IFileService;
use std::rc::Rc;

use common::files::async_test;
use common::*;
use je_base::buffer::VSBuffer;
use je_base::dynamic::Dynamic;
use je_base::event::Event;
use je_base::lifecycle::DisposableStore;
use je_base::uri::URI;
use je_platform::configuration::registry::configuration_registry;
use je_platform::configuration::service::ConfigurationService;
use je_platform::configuration::{ConfigurationOverrides, ConfigurationTarget, ConfigurationValue, is_configured};
use je_platform::files::file_service::FileService;
use je_platform::files::in_memory::InMemoryFileSystemProvider;
use je_platform::log::NullLogService;
use je_platform::policy::{FilePolicyService, NullPolicyService, PolicyService};

struct Fixture {
    _registry: RegistryGuard,
    file_service: Rc<dyn IFileService>,
    settings_resource: URI,
}

/// `setup(...)`.
fn setup(d: &Rc<DisposableStore>) -> Fixture {
    let registry = RegistryGuard::new();
    let file_service = d.add_rc(FileService::new(NullLogService::shared()));
    let disk_file_system_provider = d.add_rc(InMemoryFileSystemProvider::new());
    d.add(file_service.register_provider("file", disk_file_system_provider).unwrap());
    Fixture { _registry: registry, file_service, settings_resource: uri_file("settings.json") }
}

async fn write(file_service: &Rc<dyn IFileService>, resource: &URI, text: &str) {
    file_service.write_file(resource, VSBuffer::from_string(text).into(), None).await.unwrap();
}

fn new_service(d: &Rc<DisposableStore>, fixture: &Fixture, resource: &URI) -> Rc<ConfigurationService> {
    d.add_rc(ConfigurationService::new(resource.clone(), fixture.file_service.clone(), Rc::new(NullPolicyService), NullLogService::shared()))
}

fn register(key_and_property: Vec<(&str, je_platform::configuration::registry::PropertySchema)>) {
    configuration_registry().register_configuration(node("_test", key_and_property));
}

fn setting(default: &str) -> je_platform::configuration::registry::PropertySchema {
    prop(&format!("{{ 'type': 'string', 'default': '{default}' }}"))
}

#[test]
fn simple() {
    async_test(|d| async move {
        let fixture = setup(&d);
        write(&fixture.file_service, &fixture.settings_resource, "{ \"foo\": \"bar\" }").await;
        let test_object = new_service(&d, &fixture, &fixture.settings_resource);
        test_object.initialize().await.unwrap();
        let config = test_object.get_value(None, None);

        assert!(matches!(config, Dynamic::Object(_)));
        assert_eq!(config.get_str("foo"), s("bar"));
    });
}

#[test]
fn config_gets_flattened() {
    async_test(|d| async move {
        let fixture = setup(&d);
        write(&fixture.file_service, &fixture.settings_resource, "{ \"testworkbench.editor.tabs\": true }").await;

        let test_object = new_service(&d, &fixture, &fixture.settings_resource);
        test_object.initialize().await.unwrap();
        let config = test_object.get_value(None, None);

        assert!(matches!(config, Dynamic::Object(_)));
        let testworkbench = config.get_str("testworkbench");
        assert!(matches!(testworkbench, Dynamic::Object(_)));
        let editor = testworkbench.get_str("editor");
        assert!(matches!(editor, Dynamic::Object(_)));
        assert_eq!(editor.get_str("tabs"), Dynamic::Bool(true));
    });
}

#[test]
fn error_case_does_not_explode() {
    async_test(|d| async move {
        let fixture = setup(&d);
        write(&fixture.file_service, &fixture.settings_resource, ",,,,").await;

        let test_object = new_service(&d, &fixture, &fixture.settings_resource);
        test_object.initialize().await.unwrap();
        let config = test_object.get_value(None, None);

        assert!(matches!(config, Dynamic::Object(_)));
    });
}

#[test]
fn missing_file_does_not_explode() {
    async_test(|d| async move {
        let fixture = setup(&d);
        let test_object = new_service(&d, &fixture, &uri_file("__testFile"));
        test_object.initialize().await.unwrap();

        let config = test_object.get_value(None, None);

        assert!(matches!(config, Dynamic::Object(_)));
    });
}

#[test]
fn trigger_configuration_change_event_when_file_does_not_exist() {
    async_test(|d| async move {
        let fixture = setup(&d);
        let test_object = new_service(&d, &fixture, &fixture.settings_resource);
        test_object.initialize().await.unwrap();
        let changed = Event::filter(&test_object.on_did_change_configuration(), |e| e.source.get() == Some(ConfigurationTarget::User), None).to_promise(None);
        write(&fixture.file_service, &fixture.settings_resource, "{ \"foo\": \"bar\" }").await;

        changed.await.unwrap();

        assert_eq!(test_object.get_value(Some("foo"), None), s("bar"));
    });
}

#[test]
fn trigger_configuration_change_event_when_file_exists() {
    async_test(|d| async move {
        let fixture = setup(&d);
        let test_object = new_service(&d, &fixture, &fixture.settings_resource);
        write(&fixture.file_service, &fixture.settings_resource, "{ \"foo\": \"bar\" }").await;
        test_object.initialize().await.unwrap();

        let changed = Event::filter(&test_object.on_did_change_configuration(), |e| e.source.get() == Some(ConfigurationTarget::User), None).to_promise(None);
        write(&fixture.file_service, &fixture.settings_resource, "{ \"foo\": \"barz\" }").await;

        changed.await.unwrap();
        assert_eq!(test_object.get_value(Some("foo"), None), s("barz"));
    });
}

#[test]
fn reload_configuration() {
    async_test(|d| async move {
        let fixture = setup(&d);
        write(&fixture.file_service, &fixture.settings_resource, "{ \"foo\": \"bar\" }").await;

        let test_object = new_service(&d, &fixture, &fixture.settings_resource);
        test_object.initialize().await.unwrap();
        let config = test_object.get_value(None, None);
        assert!(matches!(config, Dynamic::Object(_)));
        assert_eq!(config.get_str("foo"), s("bar"));
        write(&fixture.file_service, &fixture.settings_resource, "{ \"foo\": \"changed\" }").await;

        // force a reload to get latest
        test_object.reload_configuration().await.unwrap();
        let config = test_object.get_value(None, None);
        assert!(matches!(config, Dynamic::Object(_)));
        assert_eq!(config.get_str("foo"), s("changed"));
    });
}

#[test]
fn model_defaults() {
    async_test(|d| async move {
        let fixture = setup(&d);
        register(vec![("configuration.service.testSetting", setting("isSet"))]);
        let test_setting = |service: &Rc<ConfigurationService>| service.get_value(None, None).get_str("configuration").get_str("service").get_str("testSetting");

        let test_object = new_service(&d, &fixture, &uri_file("__testFile"));
        test_object.initialize().await.unwrap();
        let setting = test_object.get_value(None, None);

        assert!(matches!(setting, Dynamic::Object(_)));
        assert_eq!(test_setting(&test_object), s("isSet"));

        write(&fixture.file_service, &fixture.settings_resource, "{ \"testworkbench.editor.tabs\": true }").await;
        let test_object = new_service(&d, &fixture, &fixture.settings_resource);
        test_object.initialize().await.unwrap();

        assert!(matches!(test_object.get_value(None, None), Dynamic::Object(_)));
        assert_eq!(test_setting(&test_object), s("isSet"));

        write(&fixture.file_service, &fixture.settings_resource, "{ \"configuration.service.testSetting\": \"isChanged\" }").await;

        test_object.reload_configuration().await.unwrap();
        assert!(matches!(test_object.get_value(None, None), Dynamic::Object(_)));
        assert_eq!(test_setting(&test_object), s("isChanged"));
    });
}

#[test]
fn lookup() {
    async_test(|d| async move {
        let fixture = setup(&d);
        register(vec![("lookup.service.testSetting", setting("isSet"))]);

        let test_object = new_service(&d, &fixture, &fixture.settings_resource);
        test_object.initialize().await.unwrap();

        let mut res = test_object.inspect("something.missing", &ConfigurationOverrides::default());
        assert_eq!(res.value(), Dynamic::Undefined);
        assert_eq!(res.default_value(), Dynamic::Undefined);
        assert_eq!(res.user_value(), Dynamic::Undefined);
        assert!(!is_configured(&res));

        res = test_object.inspect("lookup.service.testSetting", &ConfigurationOverrides::default());
        assert_eq!(res.default_value(), s("isSet"));
        assert_eq!(res.value(), s("isSet"));
        assert_eq!(res.user_value(), Dynamic::Undefined);
        assert!(!is_configured(&res));

        write(&fixture.file_service, &fixture.settings_resource, "{ \"lookup.service.testSetting\": \"bar\" }").await;

        test_object.reload_configuration().await.unwrap();
        res = test_object.inspect("lookup.service.testSetting", &ConfigurationOverrides::default());
        assert_eq!(res.default_value(), s("isSet"));
        assert_eq!(res.user_value(), s("bar"));
        assert_eq!(res.value(), s("bar"));
        assert!(is_configured(&res));
    });
}

#[test]
fn lookup_with_null() {
    async_test(|d| async move {
        let fixture = setup(&d);
        configuration_registry().register_configuration(node("_testNull", vec![("lookup.service.testNullSetting", prop("{ 'type': 'null' }"))]));

        let test_object = new_service(&d, &fixture, &fixture.settings_resource);
        test_object.initialize().await.unwrap();

        let mut res = test_object.inspect("lookup.service.testNullSetting", &ConfigurationOverrides::default());
        assert_eq!(res.default_value(), Dynamic::Null);
        assert_eq!(res.value(), Dynamic::Null);
        assert_eq!(res.user_value(), Dynamic::Undefined);

        write(&fixture.file_service, &fixture.settings_resource, "{ \"lookup.service.testNullSetting\": null }").await;

        test_object.reload_configuration().await.unwrap();

        res = test_object.inspect("lookup.service.testNullSetting", &ConfigurationOverrides::default());
        assert_eq!(res.default_value(), Dynamic::Null);
        assert_eq!(res.value(), Dynamic::Null);
        assert_eq!(res.user_value(), Dynamic::Null);
    });
}

#[test]
fn update_configuration() {
    async_test(|d| async move {
        let fixture = setup(&d);
        register(vec![("configurationService.testSetting", setting("isSet"))]);
        let test_object = new_service(&d, &fixture, &fixture.settings_resource);
        test_object.initialize().await.unwrap();

        test_object.update_value("configurationService.testSetting", s("value"), None, None).await.unwrap();
        assert_eq!(test_object.get_value(Some("configurationService.testSetting"), None), s("value"));
    });
}

#[test]
fn update_configuration_when_exist() {
    async_test(|d| async move {
        let fixture = setup(&d);
        register(vec![("configurationService.testSetting", setting("isSet"))]);
        let test_object = new_service(&d, &fixture, &fixture.settings_resource);
        test_object.initialize().await.unwrap();

        test_object.update_value("configurationService.testSetting", s("value"), None, None).await.unwrap();
        test_object.update_value("configurationService.testSetting", s("updatedValue"), None, None).await.unwrap();
        assert_eq!(test_object.get_value(Some("configurationService.testSetting"), None), s("updatedValue"));
    });
}

#[test]
fn update_configuration_to_default_value_should_remove() {
    async_test(|d| async move {
        let fixture = setup(&d);
        register(vec![("configurationService.testSetting", setting("isSet"))]);
        let test_object = new_service(&d, &fixture, &fixture.settings_resource);
        test_object.initialize().await.unwrap();

        test_object.update_value("configurationService.testSetting", s("value"), None, None).await.unwrap();
        test_object.update_value("configurationService.testSetting", s("isSet"), None, None).await.unwrap();
        let inspect = test_object.inspect("configurationService.testSetting", &ConfigurationOverrides::default());

        assert_eq!(inspect.user_value(), Dynamic::Undefined);
    });
}

#[test]
fn update_configuration_should_remove_when_undefined_is_passed() {
    async_test(|d| async move {
        let fixture = setup(&d);
        register(vec![("configurationService.testSetting", setting("isSet"))]);
        let test_object = new_service(&d, &fixture, &fixture.settings_resource);
        test_object.initialize().await.unwrap();

        test_object.update_value("configurationService.testSetting", s("value"), None, None).await.unwrap();
        test_object.update_value("configurationService.testSetting", Dynamic::Undefined, None, None).await.unwrap();
        let inspect = test_object.inspect("configurationService.testSetting", &ConfigurationOverrides::default());

        assert_eq!(inspect.user_value(), Dynamic::Undefined);
    });
}

#[test]
fn update_unknown_configuration() {
    async_test(|d| async move {
        let fixture = setup(&d);
        let test_object = new_service(&d, &fixture, &fixture.settings_resource);
        test_object.initialize().await.unwrap();

        test_object.update_value("configurationService.unknownSetting", s("value"), None, None).await.unwrap();
        assert_eq!(test_object.get_value(Some("configurationService.unknownSetting"), None), s("value"));
    });
}

#[test]
fn update_configuration_in_non_user_target_throws_error() {
    async_test(|d| async move {
        let fixture = setup(&d);
        register(vec![("configurationService.testSetting", setting("isSet"))]);
        let test_object = new_service(&d, &fixture, &fixture.settings_resource);
        test_object.initialize().await.unwrap();

        let result = test_object.update_value("configurationService.testSetting", s("value"), None, Some(ConfigurationTarget::Workspace)).await;
        assert!(result.is_err(), "Should fail with error");
    });
}

#[test]
fn update_configuration_throws_error_for_policy_setting() {
    async_test(|d| async move {
        let fixture = setup(&d);
        register(vec![(
            "configurationService.policySetting",
            prop(
                "{ 'type': 'string', 'default': 'isSet', 'policy': { 'name': 'configurationService.policySetting', 'category': 'Extensions', 'minimumVersion': '1.0.0', 'localization': { 'description': { 'key': '', 'value': '' } } } }",
            ),
        )]);

        let log_service = NullLogService::shared();
        let policy_file = uri_file("policies.json");
        write(&fixture.file_service, &policy_file, "{ \"configurationService.policySetting\": \"policyValue\" }").await;
        let policy_service = d.add_rc(FilePolicyService::new(policy_file, fixture.file_service.clone(), log_service.clone()));
        let test_object = d.add_rc(ConfigurationService::new(
            fixture.settings_resource.clone(),
            fixture.file_service.clone(),
            policy_service as Rc<dyn PolicyService>,
            log_service,
        ));
        test_object.initialize().await.unwrap();

        let result = test_object.update_value("configurationService.policySetting", s("value"), None, None).await;
        assert!(result.is_err(), "Should throw error");
    });
}
