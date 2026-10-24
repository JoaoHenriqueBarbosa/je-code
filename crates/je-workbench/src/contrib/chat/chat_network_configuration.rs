//! Porte parcial de `src/vs/workbench/contrib/chat/browser/chatNetworkConfiguration.ts`: as propriedades de
//! configuração dos domínios de rede permitidos e negados do agente (`chatNetworkDomainConfigurationProperties`). A
//! lista de migrações (`chatNetworkDomainConfigurationMigrations`) fica de fora: ela usa `ConfigurationMigration` e
//! `ConfigurationKeyValuePairs` (`workbench/common/configuration.ts`), que ainda não foram portados, e entra com o
//! registro de migrações de configuração.
//!
//! Divergências:
//! - **`Record<string, IConfigurationPropertySchema>`** é a lista [`chat_network_domain_configuration_properties`] de pares
//!   `(chave, propriedade)`, na ordem do original; cada propriedade é uma [`PropertySchema`] (o esquema é um objeto
//!   dinâmico, ver `registry.rs`).
//! - **`managedSettingsPresentation: read => ...`** é uma função dinâmica que recebe a função `read`; o `console.warn` é o
//!   [`je_base::console::warn`] e o `JSON.parse` com a lista de textos é o [`read_managed_string_array`].
//! - **`PolicyCategory.InteractiveSession`** é o texto `InteractiveSession` (o valor do `enum` do original, como no esquema
//!   que o registro lê). O objeto `policy` sai de [`policy_schema`], também usado pela configuração do sandbox do
//!   terminal.
//! - **`nls.localize(key, message, ...args)`** é o `localize` de `je_base`, e a mensagem do esquema e a da política são o
//!   mesmo texto, como no original.

use je_base::dynamic::{Dynamic, FunctionRef};
use je_base::nls::localize;
use je_base::policy::PolicyCategory;
use je_platform::configuration::registry::{ConfigurationScope, PropertySchema};
use je_platform::network_filter::settings::AgentNetworkDomainSettingId;
use je_platform::policy::copilot_managed_settings::COPILOT_SANDBOX_ALLOWED_HOSTS_KEY;
use je_platform::sandbox::settings::AgentSandboxSettingId;

use crate::services::configuration::common::managed_settings_presentation::{read_managed_setting, read_managed_string_array};

/// O objeto `policy` de uma propriedade: `{ name, category, minimumVersion, localization: { description, enumDescriptions? } }`.
/// `description` e cada uma das `enum_descriptions` são o par `(chave, texto)` do `localization`.
pub(crate) fn policy_schema(name: &str, category: PolicyCategory, minimum_version: &str, description: (&str, &str), enum_descriptions: &[(&str, &str)]) -> Dynamic {
    let localized = |(key, value): (&str, &str)| Dynamic::object([("key", Dynamic::string(key)), ("value", Dynamic::string(value))]);
    let mut localization = vec![("description", localized(description))];
    if !enum_descriptions.is_empty() {
        localization.push(("enumDescriptions", Dynamic::array(enum_descriptions.iter().map(|description| localized(*description)))));
    }
    Dynamic::object([
        ("name", Dynamic::string(name)),
        ("category", Dynamic::string(&format!("{category:?}"))),
        ("minimumVersion", Dynamic::string(minimum_version)),
        ("localization", Dynamic::object(localization)),
    ])
}

/// `` `#${settingId}#` ``: a referência a outra configuração dentro de uma descrição em markdown.
pub(crate) fn setting_reference(setting_id: &str) -> String {
    format!("`#{setting_id}#`")
}

/// `{ type: 'string' }`.
fn string_items() -> Dynamic {
    Dynamic::object([("type", Dynamic::string("string"))])
}

/// `chatNetworkDomainConfigurationProperties`.
pub fn chat_network_domain_configuration_properties() -> Vec<(String, PropertySchema)> {
    let network_filter = setting_reference(AgentNetworkDomainSettingId::NetworkFilter.as_str());
    let sandbox_enabled = setting_reference(AgentSandboxSettingId::AgentSandboxEnabled.as_str());
    let sandbox_allow_network = setting_reference(AgentSandboxSettingId::AgentSandboxAllowNetwork.as_str());
    let allowed_domains = setting_reference(AgentNetworkDomainSettingId::AllowedNetworkDomains.as_str());
    let denied_domains = setting_reference(AgentNetworkDomainSettingId::DeniedNetworkDomains.as_str());
    let wildcard_example = "`*.example.com`";

    let allowed_description = localize(
        "chat.agent.allowedNetworkDomains",
        "Allowed domains for network access by agent tools (fetch tool, integrated browser) when {0} is enabled. In Copilot Agent Host sessions with sandboxing ({1}) enabled, also restricts the integrated browser when outbound network access ({2}) is allowed; an empty allow list adds no sandbox restriction. This list does not enable outbound access. Supports wildcards like {3}. Outside sandboxing, when both allowed and denied lists are empty, all domains are blocked. Denied domains (see {4}) take precedence.\n\nChanges may not take full effect until VS Code is restarted.",
        &[&network_filter, &sandbox_enabled, &sandbox_allow_network, wildcard_example, &denied_domains],
    );
    let denied_description = localize(
        "chat.agent.deniedNetworkDomains",
        "Denied domains for network access by agent tools (fetch tool, integrated browser) when {0} is enabled. Also applies to the integrated browser in sandboxed Copilot Agent Host sessions ({1}); allowing outbound network access ({2}) does not bypass this list. Takes precedence over {3}. Supports wildcards like {4}.\n\nChanges may not take full effect until VS Code is restarted.",
        &[&network_filter, &sandbox_enabled, &sandbox_allow_network, &allowed_domains, wildcard_example],
    );

    let allowed = PropertySchema::from_entries([
        ("order", Dynamic::Number(66.0)),
        ("keywords", Dynamic::array([Dynamic::string("Sandbox"), Dynamic::string("sandboxing")])),
        ("markdownDescription", Dynamic::string(&allowed_description)),
        ("type", Dynamic::string("array")),
        ("items", string_items()),
        ("default", Dynamic::array([])),
        ("scope", ConfigurationScope::Application.to_dynamic()),
        ("restricted", Dynamic::Bool(true)),
        (
            "managedSettingsPresentation",
            Dynamic::Function(FunctionRef::native("managedSettingsPresentation", 1, |args| {
                let read = args.first().cloned().unwrap_or(Dynamic::Undefined);
                let value = read_managed_setting(&read, COPILOT_SANDBOX_ALLOWED_HOSTS_KEY);
                if value.is_undefined() {
                    return Dynamic::Undefined;
                }
                read_managed_string_array(
                    &value,
                    "Failed to parse managed sandbox allowed hosts; ignoring the presentation override.",
                    "Managed sandbox allowed hosts must be a string array; ignoring the presentation override.",
                )
                .unwrap_or(Dynamic::Undefined)
            })),
        ),
        (
            "policy",
            policy_schema("ChatAgentAllowedNetworkDomains", PolicyCategory::InteractiveSession, "1.116", ("chat.agent.allowedNetworkDomains", allowed_description.as_str()), &[]),
        ),
    ]);

    let denied = PropertySchema::from_entries([
        ("order", Dynamic::Number(67.0)),
        ("keywords", Dynamic::array([Dynamic::string("Sandbox"), Dynamic::string("sandboxing")])),
        ("markdownDescription", Dynamic::string(&denied_description)),
        ("type", Dynamic::string("array")),
        ("items", string_items()),
        ("default", Dynamic::array([])),
        ("scope", ConfigurationScope::Application.to_dynamic()),
        ("restricted", Dynamic::Bool(true)),
        (
            "policy",
            policy_schema("ChatAgentDeniedNetworkDomains", PolicyCategory::InteractiveSession, "1.116", ("chat.agent.deniedNetworkDomains", denied_description.as_str()), &[]),
        ),
    ]);

    vec![(AgentNetworkDomainSettingId::AllowedNetworkDomains.as_str().to_owned(), allowed), (AgentNetworkDomainSettingId::DeniedNetworkDomains.as_str().to_owned(), denied)]
}
