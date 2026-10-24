//! Porte de `src/vs/platform/configuration/test/common/policyConfiguration.test.ts` (suíte `PolicyConfiguration`), caso a caso e na mesma ordem.
//!
//! Divergências: toda a suíte roda sob o relógio virtual (`async_test`), porque o código assíncrono do
//! porte precisa do executor. O `suiteSetup`/`suiteTeardown` (registrar e desregistrar o nó de
//! configuração) e o `setup` são feitos por teste, no [`setup`] (o registro global é um singleton por
//! thread e cada teste roda no seu). `Event.toPromise(evento)` é `to_promise(None)`. `PolicyCategory.Extensions`
//! é a string `Extensions`. O `TestPolicyService` é um `AbstractPolicyService` com o `_updatePolicyDefinitions`
//! contando as chamadas. No caso do `__proto__`, o `Object.getPrototypeOf(value)` não existe no valor
//! dinâmico (não há protótipos): fica conferido que `__proto__` é uma chave própria e que nada foi poluído.
//! `policyConfigurationNode.properties['policy.settingC'] = ...` (mutar o nó já registrado) é montar o nó com
//! a propriedade a mais antes do `deepClone` que se registra.

mod common;

use je_platform::files::i_file_service::IFileService;
use std::cell::Cell;
use std::rc::Rc;

use common::files::async_test;
use common::*;
use je_base::buffer::VSBuffer;
use je_base::dynamic::Dynamic;
use je_base::lifecycle::{DisposableStore, to_disposable};
use je_base::promise::Promise;
use je_base::uri::{URI, UriChange};
use je_platform::configuration::defaults::DefaultConfiguration;
use je_platform::configuration::policy_configuration::PolicyConfiguration;
use je_platform::configuration::registry::{ConfigurationNode, PropertySchema, configuration_registry};
use je_platform::files::file_service::FileService;
use je_platform::files::in_memory::InMemoryFileSystemProvider;
use je_platform::log::NullLogService;
use je_platform::policy::{AbstractPolicyService, FilePolicyService, PolicyDefinitionType, PolicyService, PolicyValue};

fn policy(name: &str, extra: &str) -> String {
    format!("'policy': {{ 'name': '{name}', 'category': 'Extensions', 'minimumVersion': '1.0.0', {extra} 'localization': {{ 'description': {{ 'key': '', 'value': '' }} }} }}")
}

fn policy_setting(type_: &str, default: &str, policy_name: &str) -> PropertySchema {
    prop(&format!("{{ 'type': {type_}, 'default': {default}, {} }}", policy(policy_name, "")))
}

/// O `policyConfigurationNode` do original; `with_setting_c` é a mutação que o caso `when policy setting is registered` faz.
fn policy_configuration_node(with_setting_c: bool) -> ConfigurationNode {
    let mut properties: Vec<(&str, PropertySchema)> = vec![
        ("policy.settingA", policy_setting("'string'", "'defaultValueA'", "PolicySettingA")),
        ("policy.settingB", policy_setting("'string'", "'defaultValueB'", "PolicySettingB")),
        ("policy.objectSetting", policy_setting("'object'", "{}", "PolicyObjectSetting")),
        ("policy.arraySetting", policy_setting("'object'", "[]", "PolicyArraySetting")),
        ("policy.booleanSetting", policy_setting("'boolean'", "true", "PolicyBooleanSetting")),
        (
            "policy.internalSetting",
            prop(&format!("{{ 'type': 'string', 'default': 'defaultInternalValue', 'included': false, {} }}", policy("PolicyInternalSetting", ""))),
        ),
        (
            "policy.ownerSetting",
            prop(
                "{ 'type': 'boolean', 'default': true, 'policy': { 'name': 'PolicyShared', 'category': 'Extensions', 'minimumVersion': '1.0.0', 'restrictedValue': true, 'localization': { 'description': { 'key': 'shared.owner', 'value': '' } } } }",
            ),
        ),
        ("policy.referenceSetting", prop("{ 'type': 'boolean', 'default': true, 'policyReference': { 'name': 'PolicyShared' } }")),
        ("policy.orphanReferenceSetting", prop("{ 'type': 'boolean', 'default': true, 'policyReference': { 'name': 'PolicyOrphanReference' } }")),
        ("nonPolicy.setting", prop("{ 'type': 'boolean', 'default': true }")),
    ];
    if with_setting_c {
        properties.push(("policy.settingC", policy_setting("'string'", "'defaultValueC'", "PolicySettingC")));
    }
    node_full("policyConfiguration", 1.0, "a", properties)
}

struct Fixture {
    _registry: RegistryGuard,
    node: ConfigurationNode,
    default_configuration: Rc<DefaultConfiguration>,
    file_service: Rc<dyn IFileService>,
    policy_service: Rc<FilePolicyService>,
    test_object: Rc<PolicyConfiguration>,
    policy_file: URI,
}

/// O `suiteSetup` e o `setup`.
async fn setup(d: &Rc<DisposableStore>) -> Fixture {
    let registry = RegistryGuard::new();
    let node = policy_configuration_node(false);
    configuration_registry().register_configuration(node.clone());

    let policy_file = URI::file("policyFile", je_base::path::Platform::Posix).with(&UriChange { scheme: "vscode-tests".into(), ..Default::default() });
    let default_configuration = d.add_rc(DefaultConfiguration::new(NullLogService::shared()));
    default_configuration.initialize();
    let file_service = d.add_rc(FileService::new(NullLogService::shared()));
    let disk_file_system_provider = d.add_rc(InMemoryFileSystemProvider::new());
    d.add(file_service.register_provider("vscode-tests", disk_file_system_provider).unwrap());
    let policy_service = d.add_rc(FilePolicyService::new(policy_file.clone(), file_service.clone(), NullLogService::shared()));
    let test_object = d.add_rc(PolicyConfiguration::new(default_configuration.clone(), policy_service.clone() as Rc<dyn PolicyService>, NullLogService::shared()));
    Fixture { _registry: registry, node, default_configuration, file_service, policy_service, test_object, policy_file }
}

async fn write_policies(fixture: &Fixture, entries: Vec<(&str, Dynamic)>) {
    let json = stringify(&obj(entries));
    fixture.file_service.write_file(&fixture.policy_file, VSBuffer::from_string(&json).into(), None).await.unwrap();
}

fn keys(model: &je_platform::configuration::models::ConfigurationModel) -> Vec<String> {
    model.keys().iter().map(|k| k.to_string_lossy()).collect()
}

fn strings(items: &[&str]) -> Vec<String> {
    items.iter().map(|s| (*s).to_owned()).collect()
}

fn definition_of(service: &dyn PolicyService, name: &str) -> Option<std::rc::Rc<je_platform::policy::PolicyDefinition>> {
    service.policy_definitions().into_iter().find(|(n, _)| n == name).map(|(_, d)| d)
}

fn value_of(model: &je_platform::configuration::models::ConfigurationModel, key: &str) -> Dynamic {
    model.get_value(key)
}

#[test]
fn initialize_submits_included_and_excluded_policy_definitions_together() {
    async_test(|d| async move {
        let fixture = setup(&d).await;
        let update_count = Rc::new(Cell::new(0usize));
        let counter = update_count.clone();
        let test_policy_service = d.add_rc(AbstractPolicyService::new(Rc::new(move |_| {
            counter.set(counter.get() + 1);
            Promise::resolved(())
        })));
        let test_policy_configuration = d.add_rc(PolicyConfiguration::new(
            fixture.default_configuration.clone(),
            Rc::new(test_policy_service.clone()) as Rc<dyn PolicyService>,
            NullLogService::shared(),
        ));

        test_policy_configuration.initialize().await.unwrap();

        assert_eq!(
            (update_count.get(), test_policy_service.definition("PolicySettingA").is_some(), test_policy_service.definition("PolicyInternalSetting").is_some()),
            (1, true, true)
        );
    });
}

#[test]
fn initialize_with_policies() {
    async_test(|d| async move {
        let fixture = setup(&d).await;
        write_policies(&fixture, vec![("PolicySettingA", s("policyValueA"))]).await;

        fixture.test_object.initialize().await.unwrap();
        let acutal = fixture.test_object.configuration_model();

        assert_eq!(value_of(&acutal, "policy.settingA"), s("policyValueA"));
        assert_eq!(value_of(&acutal, "policy.settingB"), Dynamic::Undefined);
        assert_eq!(value_of(&acutal, "nonPolicy.setting"), Dynamic::Undefined);
        assert_eq!(keys(&acutal), strings(&["policy.settingA"]));
        assert!(acutal.overrides().is_empty());
    });
}

#[test]
fn initialize_no_policies() {
    async_test(|d| async move {
        let fixture = setup(&d).await;

        fixture.test_object.initialize().await.unwrap();
        let acutal = fixture.test_object.configuration_model();

        assert!(keys(&acutal).is_empty());
        assert!(acutal.overrides().is_empty());
        assert_eq!(value_of(&acutal, "policy.settingA"), Dynamic::Undefined);
        assert_eq!(value_of(&acutal, "policy.settingB"), Dynamic::Undefined);
        assert_eq!(value_of(&acutal, "nonPolicy.setting"), Dynamic::Undefined);
    });
}

#[test]
fn initialize_with_policies_but_not_registered() {
    async_test(|d| async move {
        let fixture = setup(&d).await;
        write_policies(&fixture, vec![("PolicySettingA", s("policyValueA")), ("PolicySettingB", s("policyValueB")), ("PolicySettingC", s("policyValueC"))]).await;

        fixture.test_object.initialize().await.unwrap();
        let acutal = fixture.test_object.configuration_model();

        assert_eq!(value_of(&acutal, "policy.settingA"), s("policyValueA"));
        assert_eq!(value_of(&acutal, "policy.settingB"), s("policyValueB"));
        assert_eq!(value_of(&acutal, "nonPolicy.setting"), Dynamic::Undefined);
        assert_eq!(keys(&acutal), strings(&["policy.settingA", "policy.settingB"]));
        assert!(acutal.overrides().is_empty());
    });
}

#[test]
fn initialize_with_object_type_policy() {
    async_test(|d| async move {
        let fixture = setup(&d).await;
        let expected = j("{ 'microsoft': true, 'github': 'stable', 'other': 1, 'complex': { 'key': 'value' }, 'array': [1, 2, 3] }");
        write_policies(&fixture, vec![("PolicyObjectSetting", s(&stringify(&expected)))]).await;

        fixture.test_object.initialize().await.unwrap();
        let acutal = fixture.test_object.configuration_model();

        assert_deep_strict_equal(&value_of(&acutal, "policy.objectSetting"), &expected);
    });
}

#[test]
fn initialize_with_object_type_policy_preserves_proto_as_own_data() {
    async_test(|d| async move {
        let fixture = setup(&d).await;
        write_policies(&fixture, vec![("PolicyObjectSetting", s("{ \"__proto__\": { \"polluted\": true } }"))]).await;

        fixture.test_object.initialize().await.unwrap();
        let value = value_of(&fixture.test_object.configuration_model(), "policy.objectSetting");

        assert!(matches!(value, Dynamic::Object(_)));
        assert_eq!(
            (value.has_property(&jstr("__proto__")), value.get_str("polluted")),
            (true, Dynamic::Undefined)
        );
    });
}

#[test]
fn initialize_with_array_type_policy() {
    async_test(|d| async move {
        let fixture = setup(&d).await;
        write_policies(&fixture, vec![("PolicyArraySetting", s(&stringify(&j("[1]"))))]).await;

        fixture.test_object.initialize().await.unwrap();
        let acutal = fixture.test_object.configuration_model();

        assert_deep_strict_equal(&value_of(&acutal, "policy.arraySetting"), &j("[1]"));
    });
}

#[test]
fn initialize_with_boolean_type_policy_as_false() {
    async_test(|d| async move {
        let fixture = setup(&d).await;
        write_policies(&fixture, vec![("PolicyBooleanSetting", Dynamic::Bool(false))]).await;

        fixture.test_object.initialize().await.unwrap();
        let acutal = fixture.test_object.configuration_model();

        assert_eq!(value_of(&acutal, "policy.booleanSetting"), Dynamic::Bool(false));
    });
}

#[test]
fn initialize_with_boolean_type_policy_as_true() {
    async_test(|d| async move {
        let fixture = setup(&d).await;
        write_policies(&fixture, vec![("PolicyBooleanSetting", Dynamic::Bool(true))]).await;

        fixture.test_object.initialize().await.unwrap();
        let acutal = fixture.test_object.configuration_model();

        assert_eq!(value_of(&acutal, "policy.booleanSetting"), Dynamic::Bool(true));
    });
}

#[test]
fn initialize_with_object_type_policy_ignores_policy_if_value_is_not_valid() {
    async_test(|d| async move {
        let fixture = setup(&d).await;
        write_policies(&fixture, vec![("PolicyObjectSetting", s("{\"a\": \"b\", \"hello\": }"))]).await;

        fixture.test_object.initialize().await.unwrap();
        let acutal = fixture.test_object.configuration_model();

        assert_eq!(value_of(&acutal, "policy.objectSetting"), Dynamic::Undefined);
    });
}

#[test]
fn initialize_with_object_type_policy_ignores_policy_if_there_are_duplicate_keys() {
    async_test(|d| async move {
        let fixture = setup(&d).await;
        write_policies(&fixture, vec![("PolicyObjectSetting", s("{\"microsoft\": true, \"microsoft\": false }"))]).await;

        fixture.test_object.initialize().await.unwrap();
        let acutal = fixture.test_object.configuration_model();

        assert_eq!(value_of(&acutal, "policy.objectSetting"), Dynamic::Undefined);
    });
}

#[test]
fn change_when_policy_is_added() {
    async_test(|d| async move {
        let fixture = setup(&d).await;
        write_policies(&fixture, vec![("PolicySettingA", s("policyValueA"))]).await;
        fixture.test_object.initialize().await.unwrap();

        let promise = fixture.test_object.on_did_change_configuration().to_promise(None);
        write_policies(&fixture, vec![("PolicySettingA", s("policyValueA")), ("PolicySettingB", s("policyValueB")), ("PolicySettingC", s("policyValueC"))]).await;
        promise.await.unwrap();

        let acutal = fixture.test_object.configuration_model();
        assert_eq!(value_of(&acutal, "policy.settingA"), s("policyValueA"));
        assert_eq!(value_of(&acutal, "policy.settingB"), s("policyValueB"));
        assert_eq!(value_of(&acutal, "nonPolicy.setting"), Dynamic::Undefined);
        assert_eq!(keys(&acutal), strings(&["policy.settingA", "policy.settingB"]));
        assert!(acutal.overrides().is_empty());
    });
}

#[test]
fn change_when_policy_is_updated() {
    async_test(|d| async move {
        let fixture = setup(&d).await;
        write_policies(&fixture, vec![("PolicySettingA", s("policyValueA"))]).await;
        fixture.test_object.initialize().await.unwrap();

        let promise = fixture.test_object.on_did_change_configuration().to_promise(None);
        write_policies(&fixture, vec![("PolicySettingA", s("policyValueAChanged"))]).await;
        promise.await.unwrap();

        let acutal = fixture.test_object.configuration_model();
        assert_eq!(value_of(&acutal, "policy.settingA"), s("policyValueAChanged"));
        assert_eq!(value_of(&acutal, "policy.settingB"), Dynamic::Undefined);
        assert_eq!(value_of(&acutal, "nonPolicy.setting"), Dynamic::Undefined);
        assert_eq!(keys(&acutal), strings(&["policy.settingA"]));
        assert!(acutal.overrides().is_empty());
    });
}

#[test]
fn change_when_policy_is_removed() {
    async_test(|d| async move {
        let fixture = setup(&d).await;
        write_policies(&fixture, vec![("PolicySettingA", s("policyValueA"))]).await;
        fixture.test_object.initialize().await.unwrap();

        let promise = fixture.test_object.on_did_change_configuration().to_promise(None);
        write_policies(&fixture, vec![]).await;
        promise.await.unwrap();

        let acutal = fixture.test_object.configuration_model();
        assert_eq!(value_of(&acutal, "policy.settingA"), Dynamic::Undefined);
        assert_eq!(value_of(&acutal, "policy.settingB"), Dynamic::Undefined);
        assert_eq!(value_of(&acutal, "nonPolicy.setting"), Dynamic::Undefined);
        assert!(keys(&acutal).is_empty());
        assert!(acutal.overrides().is_empty());
    });
}

#[test]
fn initialize_an_owning_policy_applies_to_both_the_owner_and_its_references() {
    async_test(|d| async move {
        let fixture = setup(&d).await;
        write_policies(&fixture, vec![("PolicyShared", Dynamic::Bool(false))]).await;

        fixture.test_object.initialize().await.unwrap();
        let actual = fixture.test_object.configuration_model();

        assert_eq!(value_of(&actual, "policy.ownerSetting"), Dynamic::Bool(false));
        assert_eq!(value_of(&actual, "policy.referenceSetting"), Dynamic::Bool(false));
        let mut sorted = keys(&actual);
        sorted.sort();
        assert_eq!(sorted, strings(&["policy.ownerSetting", "policy.referenceSetting"]));
    });
}

#[test]
fn initialize_a_reference_resolves_even_when_its_owner_is_not_registered() {
    async_test(|d| async move {
        let fixture = setup(&d).await;
        write_policies(&fixture, vec![("PolicyOrphanReference", Dynamic::Bool(false))]).await;

        fixture.test_object.initialize().await.unwrap();
        let actual = fixture.test_object.configuration_model();

        assert_eq!(value_of(&actual, "policy.orphanReferenceSetting"), Dynamic::Bool(false));
        assert_eq!(keys(&actual), strings(&["policy.orphanReferenceSetting"]));
    });
}

#[test]
fn initialize_nullable_boolean_reference_preserves_an_explicit_false_policy_and_an_unset_default() {
    async_test(|d| async move {
        let fixture = setup(&d).await;
        let reference = node(
            "_test_nullable_policy_reference",
            vec![("policy.nullableReference", prop("{ 'type': ['boolean', 'null'], 'default': null, 'policyReference': { 'name': 'PolicyShared' } }"))],
        );
        let registry = configuration_registry();
        registry.register_configuration(reference.clone());
        write_policies(&fixture, vec![("PolicyShared", Dynamic::Bool(false))]).await;
        fixture.test_object.initialize().await.unwrap();
        let policy_type = definition_of(fixture.policy_service.as_ref(), "PolicyShared").unwrap().type_;
        assert_eq!(
            (
                value_of(&fixture.default_configuration.configuration_model(), "policy.nullableReference"),
                value_of(&fixture.test_object.configuration_model(), "policy.nullableReference"),
                policy_type
            ),
            (Dynamic::Null, Dynamic::Bool(false), PolicyDefinitionType::Boolean)
        );
        registry.deregister_configurations(&[reference]);
    });
}

#[test]
fn initialize_the_owner_definition_is_authoritative_a_reference_only_contributes_the_policy_name() {
    async_test(|d| async move {
        let fixture = setup(&d).await;
        write_policies(&fixture, vec![("PolicyShared", Dynamic::Bool(false))]).await;

        fixture.test_object.initialize().await.unwrap();

        // The owner declares restrictedValue; the reference is a pure pointer. The registered
        // definition must be the owner's.
        let definition = definition_of(fixture.policy_service.as_ref(), "PolicyShared").unwrap();
        assert_eq!(definition.type_, PolicyDefinitionType::Boolean);
        assert_eq!(definition.restricted_value, Some(PolicyValue::Boolean(true)));
    });
}

fn owner_node(id: &str, key: &str, policy_name: &str, key_name: &str) -> ConfigurationNode {
    node(
        id,
        vec![(
            key,
            prop(&format!(
                "{{ 'type': 'boolean', 'default': true, 'policy': {{ 'name': '{policy_name}', 'category': 'Extensions', 'minimumVersion': '1.0.0', 'restrictedValue': true, 'localization': {{ 'description': {{ 'key': '{key_name}', 'value': '' }} }} }} }}"
            )),
        )],
    )
}

#[test]
fn change_a_late_registering_owner_supersedes_an_earlier_reference_definition() {
    async_test(|d| async move {
        let fixture = setup(&d).await;
        // Only the reference for `PolicyOrphanReference` is registered initially (models the editor
        // window: the agent-host reference loads eagerly while the extension policy owner loads later).
        write_policies(&fixture, vec![("PolicyOrphanReference", Dynamic::Bool(false))]).await;
        fixture.test_object.initialize().await.unwrap();

        // The synthesized reference definition carries no restrictedValue.
        assert_eq!(value_of(&fixture.test_object.configuration_model(), "policy.orphanReferenceSetting"), Dynamic::Bool(false));
        assert_eq!(definition_of(fixture.policy_service.as_ref(), "PolicyOrphanReference").unwrap().restricted_value, None);

        let owner = owner_node("_test_late_owner", "policy.lateOwner", "PolicyOrphanReference", "late.owner");

        let promise = fixture.test_object.on_did_change_configuration().to_promise(None);
        configuration_registry().register_configuration(owner.clone());
        promise.await.unwrap();

        // The owner's definition (with restrictedValue) must now supersede the reference's, and
        // both settings remain gated by the same policy value.
        assert_eq!(definition_of(fixture.policy_service.as_ref(), "PolicyOrphanReference").unwrap().restricted_value, Some(PolicyValue::Boolean(true)));
        assert_eq!(value_of(&fixture.test_object.configuration_model(), "policy.lateOwner"), Dynamic::Bool(false));
        assert_eq!(value_of(&fixture.test_object.configuration_model(), "policy.orphanReferenceSetting"), Dynamic::Bool(false));
        configuration_registry().deregister_configurations(&[owner]);
    });
}

#[test]
fn change_deregistering_the_owner_falls_back_to_a_surviving_reference_definition() {
    async_test(|d| async move {
        let fixture = setup(&d).await;
        write_policies(&fixture, vec![("PolicyOrphanReference", Dynamic::Bool(false))]).await;
        fixture.test_object.initialize().await.unwrap();

        let owner = owner_node("_test_owner_removal", "policy.removableOwner", "PolicyOrphanReference", "removable.owner");
        let registry = configuration_registry();

        let mut promise = fixture.test_object.on_did_change_configuration().to_promise(None);
        registry.register_configuration(owner.clone());
        promise.await.unwrap();
        assert_eq!(definition_of(fixture.policy_service.as_ref(), "PolicyOrphanReference").unwrap().restricted_value, Some(PolicyValue::Boolean(true)));

        // Removing the owner must re-resolve the policy and fall back to the surviving reference,
        // so the owner-only restrictedValue no longer applies.
        promise = fixture.test_object.on_did_change_configuration().to_promise(None);
        registry.deregister_configurations(&[owner]);
        promise.await.unwrap();
        assert_eq!(definition_of(fixture.policy_service.as_ref(), "PolicyOrphanReference").unwrap().restricted_value, None);
        assert_eq!(value_of(&fixture.test_object.configuration_model(), "policy.orphanReferenceSetting"), Dynamic::Bool(false));
    });
}

#[test]
fn change_an_owning_policy_update_propagates_to_both_the_owner_and_its_references() {
    async_test(|d| async move {
        let fixture = setup(&d).await;
        write_policies(&fixture, vec![("PolicyShared", Dynamic::Bool(false))]).await;
        fixture.test_object.initialize().await.unwrap();

        let promise = fixture.test_object.on_did_change_configuration().to_promise(None);
        write_policies(&fixture, vec![]).await;
        promise.await.unwrap();

        let acutal = fixture.test_object.configuration_model();
        assert_eq!(value_of(&acutal, "policy.ownerSetting"), Dynamic::Undefined);
        assert_eq!(value_of(&acutal, "policy.referenceSetting"), Dynamic::Undefined);
    });
}

#[test]
fn change_when_policy_setting_is_registered() {
    async_test(|d| async move {
        let fixture = setup(&d).await;
        write_policies(&fixture, vec![("PolicySettingC", s("policyValueC"))]).await;
        fixture.test_object.initialize().await.unwrap();

        let promise = fixture.test_object.on_did_change_configuration().to_promise(None);
        let registered_node = policy_configuration_node(true);
        configuration_registry().register_configuration(registered_node.clone());
        d.add(to_disposable(move || configuration_registry().deregister_configurations(&[registered_node])));
        promise.await.unwrap();

        let acutal = fixture.test_object.configuration_model();
        assert_eq!(value_of(&acutal, "policy.settingC"), s("policyValueC"));
        assert_eq!(value_of(&acutal, "policy.settingA"), Dynamic::Undefined);
        assert_eq!(value_of(&acutal, "policy.settingB"), Dynamic::Undefined);
        assert_eq!(value_of(&acutal, "nonPolicy.setting"), Dynamic::Undefined);
        assert_eq!(keys(&acutal), strings(&["policy.settingC"]));
        assert!(acutal.overrides().is_empty());
    });
}

#[test]
fn change_when_policy_setting_is_deregistered() {
    async_test(|d| async move {
        let fixture = setup(&d).await;
        write_policies(&fixture, vec![("PolicySettingA", s("policyValueA"))]).await;
        fixture.test_object.initialize().await.unwrap();

        let promise = fixture.test_object.on_did_change_configuration().to_promise(None);
        configuration_registry().deregister_configurations(std::slice::from_ref(&fixture.node));
        promise.await.unwrap();

        let acutal = fixture.test_object.configuration_model();
        assert_eq!(value_of(&acutal, "policy.settingA"), Dynamic::Undefined);
        assert_eq!(value_of(&acutal, "policy.settingB"), Dynamic::Undefined);
        assert_eq!(value_of(&acutal, "nonPolicy.setting"), Dynamic::Undefined);
        assert!(keys(&acutal).is_empty());
        assert!(acutal.overrides().is_empty());
    });
}

#[test]
fn initialize_with_internal_policies() {
    async_test(|d| async move {
        let fixture = setup(&d).await;
        write_policies(&fixture, vec![("PolicyInternalSetting", s("internalValue"))]).await;

        fixture.test_object.initialize().await.unwrap();
        let acutal = fixture.test_object.configuration_model();

        assert_eq!(value_of(&acutal, "policy.settingA"), Dynamic::Undefined);
        assert_eq!(value_of(&acutal, "policy.settingB"), Dynamic::Undefined);
        assert_eq!(value_of(&acutal, "policy.internalSetting"), s("internalValue"));
        assert_eq!(value_of(&acutal, "nonPolicy.setting"), Dynamic::Undefined);
        assert_eq!(keys(&acutal), strings(&["policy.internalSetting"]));
        assert!(acutal.overrides().is_empty());
    });
}
