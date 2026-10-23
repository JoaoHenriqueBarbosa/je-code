//! Porte de `src/vs/platform/policy/test/common/policy.test.ts` (suíte `AbstractPolicyService`), caso a caso e
//! na mesma ordem.
//!
//! Divergências: o `TestPolicyService` é um `AbstractPolicyService` cujo `_updatePolicyDefinitions` não faz
//! nada, e `update(name, value, source)` com `source` omitido usa o padrão do original (`Device`). O
//! `structuredClone(serialized)` não existe em Rust: o que ele garante (nada que não se clona, ou seja, nenhuma
//! função, atravessa a serialização) é conferido percorrendo o valor dinâmico de cada definição serializada.
//! O objeto `{ changed, value, source }` é uma tupla nessa ordem.

mod common;

use std::rc::Rc;

use common::files::async_test;
use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::dynamic::{Dynamic, FunctionRef};
use je_base::lifecycle::IDisposable;
use je_base::promise::Promise;
use je_platform::policy::{AbstractPolicyService, PolicyDefinition, PolicyDefinitionType, PolicyService, PolicyValue, PolicyValueSource, SerializedPolicy};

struct TestPolicyService {
    inner: Rc<AbstractPolicyService>,
}

impl TestPolicyService {
    fn new() -> Self {
        // no-op: the OS/file watcher is irrelevant for serialization tests
        Self { inner: AbstractPolicyService::new(Rc::new(|_| Promise::resolved(()))) }
    }

    fn update(&self, name: &str, value: Option<PolicyValue>, source: Option<PolicyValueSource>) -> bool {
        self.inner.update_policy_value(name, value, source.unwrap_or(PolicyValueSource::Device))
    }
}

fn serialized<'a>(list: &'a [(String, SerializedPolicy)], name: &str) -> &'a SerializedPolicy {
    &list.iter().find(|(key, _)| key == name).unwrap_or_else(|| panic!("falta a política {name}")).1
}

/// Se algum valor alcançável a partir de `value` é uma função (o que `structuredClone` recusa).
fn contains_function(value: &Dynamic) -> bool {
    match value {
        Dynamic::Function(_) => true,
        Dynamic::Array(items) => items.to_vec().iter().any(contains_function),
        Dynamic::Object(object) => object.entries().iter().any(|(_, item)| contains_function(item)),
        _ => false,
    }
}

fn is_structured_clone_safe(serialized: &SerializedPolicy) -> bool {
    let definition = &serialized.definition;
    ![&definition.value, &definition.managed_settings].into_iter().flatten().any(contains_function)
}

#[test]
fn serialize_omits_the_non_cloneable_value_callback_so_policies_data_can_be_sent_over_ipc() {
    async_test(|_d| async move {
        let service = TestPolicyService::new().inner;

        let with_callback = PolicyDefinition {
            type_: PolicyDefinitionType::Boolean,
            value: Some(Dynamic::Function(FunctionRef::native("value", 1, |args| {
                let flag = args.first().map_or(Dynamic::Undefined, |policy_data| policy_data.get_str("chat_preview_features_enabled"));
                if matches!(flag, Dynamic::Bool(false)) { Dynamic::Bool(false) } else { Dynamic::Undefined }
            }))),
            managed_settings: None,
            restricted_value: Some(PolicyValue::Boolean(false)),
        };
        service
            .update_policy_definitions(vec![
                ("WithCallback".to_owned(), Rc::new(with_callback)),
                ("PlainDefinition".to_owned(), Rc::new(PolicyDefinition::new(PolicyDefinitionType::String))),
            ])
            .await
            .unwrap();

        let serialized_policies = service.serialize().expect("serialize() de um AbstractPolicyService");

        // The callback must not survive serialization...
        assert!(serialized(&serialized_policies, "WithCallback").definition.value.is_none());
        // ...while the structured-clone-safe metadata is preserved.
        assert_eq!(serialized(&serialized_policies, "WithCallback").definition.type_, PolicyDefinitionType::Boolean);
        assert_eq!(serialized(&serialized_policies, "WithCallback").definition.restricted_value, Some(PolicyValue::Boolean(false)));
        assert_eq!(serialized(&serialized_policies, "PlainDefinition").definition.type_, PolicyDefinitionType::String);

        // The whole payload must be structured-clone-safe (this is how it is delivered to the
        // renderer as part of the window configuration's policiesData).
        assert!(serialized_policies.iter().all(|(_, policy)| is_structured_clone_safe(policy)));

        service.dispose();
    });
}

#[test]
fn tracks_value_and_source_changes_together() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let service = TestPolicyService::new();
    let mut states: Vec<(bool, Option<PolicyValue>, Option<PolicyValueSource>)> = Vec::new();
    let mut update = |value: Option<PolicyValue>, source: Option<PolicyValueSource>| {
        let changed = service.update("Policy", value, source);
        states.push((changed, service.inner.get_policy_value("Policy"), service.inner.get_policy_value_source("Policy")));
    };

    update(Some(PolicyValue::Boolean(false)), None);
    update(Some(PolicyValue::Boolean(false)), Some(PolicyValueSource::Account));
    update(Some(PolicyValue::Boolean(false)), Some(PolicyValueSource::AccountGate));
    update(Some(PolicyValue::Boolean(false)), Some(PolicyValueSource::AccountGate));
    update(None, None);

    assert_eq!(
        states,
        vec![
            (true, Some(PolicyValue::Boolean(false)), Some(PolicyValueSource::Device)),
            (true, Some(PolicyValue::Boolean(false)), Some(PolicyValueSource::Account)),
            (true, Some(PolicyValue::Boolean(false)), Some(PolicyValueSource::AccountGate)),
            (false, Some(PolicyValue::Boolean(false)), Some(PolicyValueSource::AccountGate)),
            (true, None, None),
        ]
    );

    service.inner.dispose();
}
