//! A ponte entre o callback `value(policyData)` de uma política e o [`PolicyDefinition`] do porte.
//!
//! No original, `PolicyDefinition.value` é uma função TypeScript que recebe o `IPolicyData`. A
//! [`PolicyDefinition`] de `je_platform` guarda o `value` como [`Dynamic`] opaco (o tipo da definição não conhece o
//! `IPolicyData`), e o registro de configuração também carrega o callback como uma função dinâmica. Este módulo
//! faz as duas pontas:
//! - [`policy_value_dynamic`] embrulha um [`PolicyValueFn`] (o callback tipado de `je_base::policy`) numa função
//!   dinâmica, para pendurar num [`PolicyDefinition`] ou numa propriedade do registro;
//! - [`call_policy_value`] chama essa função com um [`IPolicyData`], como `policy.value(policyData)`.
//!
//! O `IPolicyData` atravessa a função dinâmica como um objeto com as mesmas chaves do tipo original
//! (`chat_preview_features_enabled`, `mcpRegistryUrl`, `managedSettings`...).

use je_base::default_account::{IPolicyData, McpAccess};
use je_base::dynamic::{Dynamic, FunctionRef};
use je_base::policy::{ManagedSettingsData, PolicyValue as BasePolicyValue, PolicyValueFn};
use je_platform::policy::PolicyValue;

/// Um `IPolicyData` sem nenhum campo (o `{}` do original).
pub fn empty_policy_data() -> IPolicyData {
    IPolicyData {
        mcp: None,
        chat_preview_features_enabled: None,
        chat_agent_enabled: None,
        cloud_session_storage_enabled: None,
        mcp_registry_url: None,
        mcp_access: None,
        managed_settings: None,
        managed_settings_active: None,
        extra: Default::default(),
    }
}

fn base_value_to_dynamic(value: &BasePolicyValue) -> Dynamic {
    match value {
        BasePolicyValue::Boolean(flag) => Dynamic::Bool(*flag),
        BasePolicyValue::Number(number) => Dynamic::Number(*number),
        BasePolicyValue::String(text) => Dynamic::string(text),
    }
}

fn base_value_from_dynamic(value: &Dynamic) -> Option<BasePolicyValue> {
    match value {
        Dynamic::Bool(flag) => Some(BasePolicyValue::Boolean(*flag)),
        Dynamic::Number(number) => Some(BasePolicyValue::Number(*number)),
        Dynamic::String(text) => Some(BasePolicyValue::String(text.to_string_lossy())),
        _ => None,
    }
}

/// O `PolicyValue` do `je_base` (o que o `ManagedSettingsData` e o `PolicyValueFn` usam) como o de `je_platform`
/// (o que o `PolicyService` usa): a mesma união `string | number | boolean`.
pub fn to_platform_policy_value(value: &BasePolicyValue) -> PolicyValue {
    match value {
        BasePolicyValue::Boolean(flag) => PolicyValue::Boolean(*flag),
        BasePolicyValue::Number(number) => PolicyValue::Number(*number),
        BasePolicyValue::String(text) => PolicyValue::String(text.clone()),
    }
}

/// O inverso de [`to_platform_policy_value`].
pub fn to_base_policy_value(value: &PolicyValue) -> BasePolicyValue {
    match value {
        PolicyValue::Boolean(flag) => BasePolicyValue::Boolean(*flag),
        PolicyValue::Number(number) => BasePolicyValue::Number(*number),
        PolicyValue::String(text) => BasePolicyValue::String(text.clone()),
    }
}

fn managed_settings_to_dynamic(managed_settings: &ManagedSettingsData) -> Dynamic {
    Dynamic::object(managed_settings.iter().map(|(key, value)| (key.as_str(), base_value_to_dynamic(value))))
}

/// O `IPolicyData` como o objeto que o callback `value` recebe.
pub fn policy_data_to_dynamic(policy_data: &IPolicyData) -> Dynamic {
    let mut entries: Vec<(&str, Dynamic)> = Vec::new();
    if let Some(mcp) = policy_data.mcp {
        entries.push(("mcp", Dynamic::Bool(mcp)));
    }
    if let Some(enabled) = policy_data.chat_preview_features_enabled {
        entries.push(("chat_preview_features_enabled", Dynamic::Bool(enabled)));
    }
    if let Some(enabled) = policy_data.chat_agent_enabled {
        entries.push(("chat_agent_enabled", Dynamic::Bool(enabled)));
    }
    if let Some(enabled) = policy_data.cloud_session_storage_enabled {
        entries.push(("cloud_session_storage_enabled", Dynamic::Bool(enabled)));
    }
    if let Some(url) = &policy_data.mcp_registry_url {
        entries.push(("mcpRegistryUrl", Dynamic::string(url)));
    }
    if let Some(access) = policy_data.mcp_access {
        let text = match access {
            McpAccess::AllowAll => "allow_all",
            McpAccess::RegistryOnly => "registry_only",
        };
        entries.push(("mcpAccess", Dynamic::string(text)));
    }
    if let Some(managed_settings) = &policy_data.managed_settings {
        entries.push(("managedSettings", managed_settings_to_dynamic(managed_settings)));
    }
    if let Some(active) = policy_data.managed_settings_active {
        entries.push(("managedSettingsActive", Dynamic::Bool(active)));
    }
    Dynamic::object(entries)
}

/// O inverso de [`policy_data_to_dynamic`]: chaves ausentes ou de tipo errado ficam `None`.
pub fn policy_data_from_dynamic(value: &Dynamic) -> IPolicyData {
    let flag = |key: &str| match value.get_str(key) {
        Dynamic::Bool(flag) => Some(flag),
        _ => None,
    };
    let text = |key: &str| match value.get_str(key) {
        Dynamic::String(text) => Some(text.to_string_lossy()),
        _ => None,
    };
    let managed_settings = match value.get_str("managedSettings") {
        settings @ Dynamic::Object(_) => Some(settings.own_entries().into_iter().filter_map(|(key, entry)| base_value_from_dynamic(&entry).map(|entry| (key.to_string_lossy(), entry))).collect()),
        _ => None,
    };
    IPolicyData {
        mcp: flag("mcp"),
        chat_preview_features_enabled: flag("chat_preview_features_enabled"),
        chat_agent_enabled: flag("chat_agent_enabled"),
        cloud_session_storage_enabled: flag("cloud_session_storage_enabled"),
        mcp_registry_url: text("mcpRegistryUrl"),
        mcp_access: text("mcpAccess").and_then(|access| match access.as_str() {
            "allow_all" => Some(McpAccess::AllowAll),
            "registry_only" => Some(McpAccess::RegistryOnly),
            _ => None,
        }),
        managed_settings,
        managed_settings_active: flag("managedSettingsActive"),
        extra: Default::default(),
    }
}

/// O `PolicyValueFn` como a função dinâmica que o `PolicyDefinition.value` (e a propriedade `policy.value` do
/// registro) carrega.
pub fn policy_value_dynamic(callback: PolicyValueFn) -> Dynamic {
    Dynamic::Function(FunctionRef::native("value", 1, move |args| {
        let policy_data = args.first().map(policy_data_from_dynamic).unwrap_or_else(empty_policy_data);
        callback.call(&policy_data).map_or(Dynamic::Undefined, |value| base_value_to_dynamic(&value))
    }))
}

/// `policy.value(policyData)`: o valor do callback, ou `None` (o `undefined`) quando a função não existe ou não
/// devolve um valor de política.
pub fn call_policy_value(value: &Option<Dynamic>, policy_data: &IPolicyData) -> Option<PolicyValue> {
    match value {
        Some(Dynamic::Function(function)) => PolicyValue::from_dynamic(&function.call(&[policy_data_to_dynamic(policy_data)])),
        _ => None,
    }
}
