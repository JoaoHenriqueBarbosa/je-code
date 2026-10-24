//! Porte parcial de `src/vs/workbench/contrib/terminalContrib/chatAgentTools/common/terminalChatAgentToolsConfiguration.ts`:
//! as configurações do sandbox do agente que a interface mostra com a restrição gerenciada aplicada, as que declaram
//! `managedSettingsPresentation` (`chat.agent.sandbox.enabled`, `...network.allowNetwork`, `...network.allowLocalNetwork`,
//! `...allowUnsandboxedCommands`, `...mcpServers`, `...lspServers`, `...credentials.authenticategit`,
//! `...credentials.authenticategh`, `...fileSystem.allowDevToolAccess` e `...fileSystem.userConfiguredPaths`). O resto do
//! arquivo (a aprovação automática de comandos, os perfis de terminal, os tempos de espera, as regras do Linux e do macOS e
//! a migração `sandboxAllowNetworkMigration`) depende das regras de aprovação do `platform/terminal`, do esquema de perfis
//! e do registro de migrações de configuração, e entra com eles.
//!
//! Divergências:
//! - **`IStringDictionary<IConfigurationPropertySchema>`** é a lista de pares `(chave, propriedade)` na ordem do
//!   original; cada propriedade é uma [`PropertySchema`] (esquema como objeto dinâmico, ver `registry.rs`).
//! - **`managedSettingsPresentation: read => ...`** é uma função dinâmica com `(read, localValue)`. O `console.warn` é o
//!   [`je_base::console::warn`] e a leitura da lista de caminhos é o [`read_managed_string_array`].
//! - **`SandboxSettingsResolutionHelper`** devolve `Option`; o `undefined` do original é `Dynamic::Undefined` no valor da
//!   apresentação.
//! - **`enum: [Off, On]`** e o valor padrão `Off` são os textos `off` e `on` de `AgentSandboxEnabledValue`.

use je_base::dynamic::{Dynamic, FunctionRef};
use je_base::nls::localize;
use je_base::policy::PolicyCategory;
use je_base::types;
use je_platform::configuration::registry::PropertySchema;
use je_platform::policy::copilot_managed_settings::{
    COPILOT_SANDBOX_ALLOW_BYPASS_KEY, COPILOT_SANDBOX_ALLOW_DEV_TOOL_ACCESS_KEY, COPILOT_SANDBOX_ALLOW_LOCAL_NETWORK_KEY, COPILOT_SANDBOX_ALLOW_OUTBOUND_KEY,
    COPILOT_SANDBOX_AUTH_GH_KEY, COPILOT_SANDBOX_AUTH_GIT_KEY, COPILOT_SANDBOX_DENIED_PATHS_KEY, COPILOT_SANDBOX_ENABLED_KEY, COPILOT_SANDBOX_LSP_SERVERS_KEY,
    COPILOT_SANDBOX_MCP_SERVERS_KEY, COPILOT_SANDBOX_READONLY_PATHS_KEY, COPILOT_SANDBOX_READWRITE_PATHS_KEY,
};
use je_platform::sandbox::sandbox_settings_resolution_helper::SandboxSettingsResolutionHelper;
use je_platform::sandbox::settings::{AgentSandboxEnabledValue, AgentSandboxSettingId, AgentSandboxUserConfiguredPaths};

use crate::contrib::chat::chat_network_configuration::policy_schema;
use crate::services::configuration::common::managed_settings_presentation::{read_managed_setting, read_managed_string_array};

/// `value === undefined ? undefined : value` para um booleano opcional.
fn optional_bool(value: Option<bool>) -> Dynamic {
    value.map_or(Dynamic::Undefined, Dynamic::Bool)
}

/// O primeiro argumento de uma função de apresentação (`read`).
fn read_argument(args: &[Dynamic]) -> Dynamic {
    args.first().cloned().unwrap_or(Dynamic::Undefined)
}

/// `read(key) === true`.
fn managed_is_true(read: &Dynamic, key: &str) -> bool {
    read_managed_setting(read, key).strict_equals(&Dynamic::Bool(true))
}

/// `read(key) !== false`.
fn managed_is_not_false(read: &Dynamic, key: &str) -> bool {
    !read_managed_setting(read, key).strict_equals(&Dynamic::Bool(false))
}

/// A função `managedSettingsPresentation` de uma configuração booleana de acesso: `resolveAllowAccess(undefined, read(key) !== false)`.
fn allow_access_presentation(key: &'static str) -> Dynamic {
    Dynamic::Function(FunctionRef::native("managedSettingsPresentation", 1, move |args| {
        optional_bool(SandboxSettingsResolutionHelper::resolve_allow_access(None, Some(managed_is_not_false(&read_argument(args), key))))
    }))
}

/// A função `managedSettingsPresentation` de uma configuração de servidores no sandbox: `resolveSandboxServers(undefined, read(key) === true)`.
fn sandbox_servers_presentation(key: &'static str) -> Dynamic {
    Dynamic::Function(FunctionRef::native("managedSettingsPresentation", 1, move |args| {
        optional_bool(SandboxSettingsResolutionHelper::resolve_sandbox_servers(None, Some(managed_is_true(&read_argument(args), key))))
    }))
}

/// `['Sandbox', 'sandboxing']`.
fn sandbox_keywords() -> Dynamic {
    Dynamic::array([Dynamic::string("Sandbox"), Dynamic::string("sandboxing")])
}

/// Os textos de uma lista de `string`s como array dinâmico.
fn string_array(items: &[String]) -> Dynamic {
    Dynamic::array(items.iter().map(|item| Dynamic::string(item)))
}

/// Os textos de um array dinâmico de textos (`isStringArray` já conferido).
fn string_vec(value: &Dynamic) -> Vec<String> {
    match value {
        Dynamic::Array(items) => items.to_vec().iter().filter_map(|item| if let Dynamic::String(text) = item { Some(text.to_string_lossy()) } else { None }).collect(),
        _ => Vec::new(),
    }
}

/// O `managedSettingsPresentation` de `chat.agent.sandbox.fileSystem.userConfiguredPaths`.
fn user_configured_paths_presentation(args: &[Dynamic]) -> Dynamic {
    let read = read_argument(args);
    let local_value = args.get(1).cloned().unwrap_or(Dynamic::Undefined);
    let mut managed = AgentSandboxUserConfiguredPaths::default();
    let mut managed_count = 0;
    for (property, key) in [
        ("readwritePaths", COPILOT_SANDBOX_READWRITE_PATHS_KEY),
        ("readonlyPaths", COPILOT_SANDBOX_READONLY_PATHS_KEY),
        ("deniedPaths", COPILOT_SANDBOX_DENIED_PATHS_KEY),
    ] {
        let value = read_managed_setting(&read, key);
        if value.is_undefined() {
            continue;
        }
        let Some(paths) = read_managed_string_array(
            &value,
            "Failed to parse managed sandbox filesystem paths; ignoring the presentation override.",
            "Managed sandbox filesystem paths must be a string array; ignoring the presentation override.",
        ) else {
            return Dynamic::Undefined;
        };
        let paths = Some(string_vec(&paths));
        match property {
            "readwritePaths" => managed.readwrite_paths = paths,
            "readonlyPaths" => managed.readonly_paths = paths,
            _ => managed.denied_paths = paths,
        }
        managed_count += 1;
    }
    if managed_count == 0 {
        return Dynamic::Undefined;
    }
    let mut local = AgentSandboxUserConfiguredPaths::default();
    if matches!(local_value, Dynamic::Object(_)) {
        for (property, paths) in local_value.own_entries() {
            if !types::is_string_array(&paths) {
                continue;
            }
            match property.to_string_lossy().as_str() {
                "readwritePaths" => local.readwrite_paths = Some(string_vec(&paths)),
                "readonlyPaths" => local.readonly_paths = Some(string_vec(&paths)),
                "deniedPaths" => local.denied_paths = Some(string_vec(&paths)),
                _ => {}
            }
        }
    }
    let resolved = SandboxSettingsResolutionHelper::resolve_file_system_paths(Some(&local), Some(&managed));
    Dynamic::object([
        ("readwritePaths", string_array(&resolved.readwrite_paths.unwrap_or_default())),
        ("readonlyPaths", string_array(&resolved.readonly_paths.unwrap_or_default())),
        ("deniedPaths", string_array(&resolved.denied_paths.unwrap_or_default())),
    ])
}

/// O esquema de uma das três listas de `userConfiguredPaths`: `{ type: 'array', description, items: { type: 'string' }, default: [] }`.
fn paths_list_schema(description: String) -> Dynamic {
    Dynamic::object([
        ("type", Dynamic::string("array")),
        ("description", Dynamic::string(&description)),
        ("items", Dynamic::object([("type", Dynamic::string("string"))])),
        ("default", Dynamic::array([])),
    ])
}

/// As propriedades do sandbox do agente que declaram `managedSettingsPresentation`, na ordem do original.
pub fn terminal_chat_agent_tools_managed_presentation_properties() -> Vec<(String, PropertySchema)> {
    let sandbox_enabled_description = localize(
        "agentSandbox.enabledSetting",
        "Controls whether agent mode uses sandboxing to restrict what tools can do. When enabled, tools like the terminal are run in a sandboxed environment to limit access to the system. The Local harness supports sandboxing on Linux and macOS only. The Copilot Agent Host harness uses the SDK runtime's sandbox, including on supported Windows hosts.",
        &[],
    );
    let enabled_off = localize("agentSandbox.enabledSetting.offDescription", "Disable sandboxing for agent mode tools.", &[]);
    let enabled_on = localize("agentSandbox.enabledSetting.onDescription", "Enable sandboxing for agent mode tools.", &[]);
    let enabled_policy_description = localize(
        "agentSandbox.enabledPolicy",
        "Deprecated. Controls whether the Local agent uses sandboxing to restrict what tools can do. In Copilot Agent Host, this legacy policy supplies an overridable sandbox default, not a mandatory requirement. Use the runtime-native sandbox.enabled managed setting to require sandboxing in Copilot Agent Host.",
        &[],
    );
    let allow_network_policy_description = localize(
        "agentSandbox.allowNetworkPolicy",
        "Deprecated. Controls whether the Local agent sandbox allows outbound connections to all network domains while preserving file system restrictions. Use the sandbox.userPolicy.network.allowOutbound managed setting to enforce outbound network restrictions in Copilot Agent Host.",
        &[],
    );
    let allow_unsandboxed_policy_description = localize(
        "agentSandbox.allowUnsandboxedCommandsPolicy",
        "Deprecated. Controls whether Local agent terminal commands can run outside the sandbox after user confirmation when sandbox restrictions would block the command. Use the sandbox.allowBypass managed setting to control sandbox bypass in Copilot Agent Host.",
        &[],
    );

    let presentation = |function: Dynamic| ("managedSettingsPresentation", function);
    let restricted = ("restricted", Dynamic::Bool(true));
    let keywords = || ("keywords", sandbox_keywords());

    let enabled = PropertySchema::from_entries([
        presentation(Dynamic::Function(FunctionRef::native("managedSettingsPresentation", 1, |args| {
            match SandboxSettingsResolutionHelper::resolve_enabled(None, Some(managed_is_true(&read_argument(args), COPILOT_SANDBOX_ENABLED_KEY))) {
                Some(value) => Dynamic::string(value.as_str()),
                None => Dynamic::Undefined,
            }
        }))),
        ("order", Dynamic::Number(10.0)),
        keywords(),
        ("markdownDescription", Dynamic::string(&sandbox_enabled_description)),
        ("type", Dynamic::string("string")),
        ("enum", Dynamic::array([Dynamic::string(AgentSandboxEnabledValue::Off.as_str()), Dynamic::string(AgentSandboxEnabledValue::On.as_str())])),
        ("enumDescriptions", Dynamic::array([Dynamic::string(&enabled_off), Dynamic::string(&enabled_on)])),
        ("default", Dynamic::string(AgentSandboxEnabledValue::Off.as_str())),
        restricted.clone(),
        ("experiment", Dynamic::object([("mode", Dynamic::string("auto"))])),
        (
            "policy",
            policy_schema(
                "ChatAgentSandboxEnabled",
                PolicyCategory::IntegratedTerminal,
                "1.116",
                ("agentSandbox.enabledPolicy", enabled_policy_description.as_str()),
                &[("agentSandbox.enabledSetting.offDescription", enabled_off.as_str()), ("agentSandbox.enabledSetting.onDescription", enabled_on.as_str())],
            ),
        ),
    ]);

    let allow_network = PropertySchema::from_entries([
        presentation(allow_access_presentation(COPILOT_SANDBOX_ALLOW_OUTBOUND_KEY)),
        ("order", Dynamic::Number(20.0)),
        keywords(),
        (
            "markdownDescription",
            Dynamic::string(&localize(
                "agentSandbox.allowNetwork",
                "Allow outbound network access in the sandbox (`allowOutbound` in the SDK). Configured allowed and denied network domains still restrict destinations. When disabled, outbound network access is blocked, including for the integrated browser.",
                &[],
            )),
        ),
        ("type", Dynamic::string("boolean")),
        ("default", Dynamic::Bool(true)),
        restricted.clone(),
        (
            "policy",
            policy_schema(
                "ChatAgentSandboxAllowNetwork",
                PolicyCategory::IntegratedTerminal,
                "1.127",
                ("agentSandbox.allowNetworkPolicy", allow_network_policy_description.as_str()),
                &[],
            ),
        ),
    ]);

    let allow_local_network = PropertySchema::from_entries([
        presentation(allow_access_presentation(COPILOT_SANDBOX_ALLOW_LOCAL_NETWORK_KEY)),
        ("order", Dynamic::Number(25.0)),
        keywords(),
        (
            "markdownDescription",
            Dynamic::string(&localize(
                "agentSandbox.allowLocalNetwork",
                "Allow the sandboxed process to reach hosts on the local network. Windows needs this on when a proxy, host rules, or credential masking is used: it reaches the local listener over host loopback, and a sandboxed command will not launch without it. The local proxy checks destination addresses for direct connections. An upstream proxy resolves its own destinations.",
                &[],
            )),
        ),
        ("type", Dynamic::string("boolean")),
        ("default", Dynamic::Bool(false)),
        restricted.clone(),
    ]);

    let allow_unsandboxed_commands = PropertySchema::from_entries([
        presentation(Dynamic::Function(FunctionRef::native("managedSettingsPresentation", 1, |args| {
            let read = read_argument(args);
            let allow_bypass = read_managed_setting(&read, COPILOT_SANDBOX_ALLOW_BYPASS_KEY);
            let allow_bypass = if let Dynamic::Bool(allow_bypass) = allow_bypass { Some(allow_bypass) } else { None };
            optional_bool(SandboxSettingsResolutionHelper::resolve_allow_bypass(None, allow_bypass, Some(managed_is_true(&read, COPILOT_SANDBOX_ENABLED_KEY))))
        }))),
        ("order", Dynamic::Number(30.0)),
        keywords(),
        (
            "markdownDescription",
            Dynamic::string(&localize(
                "agentSandbox.allowUnsandboxedCommands",
                "Controls whether agent mode terminal commands can run outside the sandbox after user confirmation when a sandboxed command fails or when sandbox restrictions would block the command.",
                &[],
            )),
        ),
        ("type", Dynamic::string("boolean")),
        ("default", Dynamic::Bool(true)),
        restricted.clone(),
        (
            "policy",
            policy_schema(
                "ChatAgentSandboxAllowUnsandboxedCommands",
                PolicyCategory::IntegratedTerminal,
                "1.116",
                ("agentSandbox.allowUnsandboxedCommandsPolicy", allow_unsandboxed_policy_description.as_str()),
                &[],
            ),
        ),
    ]);

    let mcp_servers = PropertySchema::from_entries([
        presentation(sandbox_servers_presentation(COPILOT_SANDBOX_MCP_SERVERS_KEY)),
        ("order", Dynamic::Number(40.0)),
        keywords(),
        (
            "markdownDescription",
            Dynamic::string(&localize("agentSandbox.mcpServers", "Controls whether MCP servers run in the sandbox when sandboxing is enabled.", &[])),
        ),
        ("type", Dynamic::string("boolean")),
        ("default", Dynamic::Bool(true)),
        restricted.clone(),
    ]);

    let lsp_servers = PropertySchema::from_entries([
        presentation(sandbox_servers_presentation(COPILOT_SANDBOX_LSP_SERVERS_KEY)),
        ("order", Dynamic::Number(50.0)),
        keywords(),
        (
            "markdownDescription",
            Dynamic::string(&localize("agentSandbox.lspServers", "Controls whether LSP servers run in the sandbox when sandboxing is enabled.", &[])),
        ),
        ("type", Dynamic::string("boolean")),
        ("default", Dynamic::Bool(true)),
        restricted.clone(),
    ]);

    let authenticate_git = PropertySchema::from_entries([
        presentation(allow_access_presentation(COPILOT_SANDBOX_AUTH_GIT_KEY)),
        ("order", Dynamic::Number(51.0)),
        keywords(),
        (
            "markdownDescription",
            Dynamic::string(&localize(
                "agentSandbox.authenticateGit",
                "Use your credentials for HTTPS git operations inside the sandbox. Applies only to the Copilot Agent Host harness.",
                &[],
            )),
        ),
        ("type", Dynamic::string("boolean")),
        ("default", Dynamic::Bool(true)),
        restricted.clone(),
    ]);

    let authenticate_gh = PropertySchema::from_entries([
        presentation(allow_access_presentation(COPILOT_SANDBOX_AUTH_GH_KEY)),
        ("order", Dynamic::Number(52.0)),
        keywords(),
        (
            "markdownDescription",
            Dynamic::string(&localize(
                "agentSandbox.authenticateGh",
                "Use your GitHub account for gh commands inside the sandbox. Applies only to the Copilot Agent Host harness.",
                &[],
            )),
        ),
        ("type", Dynamic::string("boolean")),
        ("default", Dynamic::Bool(true)),
        restricted.clone(),
    ]);

    let allow_dev_tool_access = PropertySchema::from_entries([
        presentation(allow_access_presentation(COPILOT_SANDBOX_ALLOW_DEV_TOOL_ACCESS_KEY)),
        ("order", Dynamic::Number(60.0)),
        keywords(),
        (
            "markdownDescription",
            Dynamic::string(&localize(
                "agentSandbox.allowDevToolAccess",
                "Grant read access to the tool directories found on PATH, in toolchain env vars (CARGO_HOME, GOROOT, and similar) and under your user profile, plus dev-tool config and caches (including registry tokens), and read-write to shared build caches.",
                &[],
            )),
        ),
        ("type", Dynamic::string("boolean")),
        ("default", Dynamic::Bool(true)),
        restricted.clone(),
    ]);

    let user_configured_paths = PropertySchema::from_entries([
        presentation(Dynamic::Function(FunctionRef::native("managedSettingsPresentation", 2, user_configured_paths_presentation))),
        ("order", Dynamic::Number(65.0)),
        keywords(),
        ("markdownDescription", Dynamic::string(&localize("agentSandbox.userConfiguredPaths", "Customize file path permissions in the sandbox.", &[]))),
        ("type", Dynamic::array([Dynamic::string("object")])),
        (
            "properties",
            Dynamic::object([
                (
                    "readwritePaths",
                    paths_list_schema(localize(
                        "agentSandbox.userConfiguredPaths.readwritePaths",
                        "Read/Write: paths the Copilot Agent Host sandbox can read and write.",
                        &[],
                    )),
                ),
                (
                    "readonlyPaths",
                    paths_list_schema(localize(
                        "agentSandbox.userConfiguredPaths.readonlyPaths",
                        "Read-Only: paths the Copilot Agent Host sandbox can read but not write.",
                        &[],
                    )),
                ),
                (
                    "deniedPaths",
                    paths_list_schema(localize("agentSandbox.userConfiguredPaths.deniedPaths", "Denied: paths the Copilot Agent Host sandbox cannot access.", &[])),
                ),
            ]),
        ),
        ("additionalProperties", Dynamic::Bool(false)),
        (
            "default",
            Dynamic::object([("readwritePaths", Dynamic::array([])), ("readonlyPaths", Dynamic::array([])), ("deniedPaths", Dynamic::array([]))]),
        ),
        restricted,
    ]);

    [
        (AgentSandboxSettingId::AgentSandboxEnabled, enabled),
        (AgentSandboxSettingId::AgentSandboxAllowNetwork, allow_network),
        (AgentSandboxSettingId::AgentSandboxAllowLocalNetwork, allow_local_network),
        (AgentSandboxSettingId::AgentSandboxAllowUnsandboxedCommands, allow_unsandboxed_commands),
        (AgentSandboxSettingId::AgentSandboxMcpServers, mcp_servers),
        (AgentSandboxSettingId::AgentSandboxLspServers, lsp_servers),
        (AgentSandboxSettingId::AgentSandboxAuthenticateGit, authenticate_git),
        (AgentSandboxSettingId::AgentSandboxAuthenticateGh, authenticate_gh),
        (AgentSandboxSettingId::AgentSandboxAllowDevToolAccess, allow_dev_tool_access),
        (AgentSandboxSettingId::AgentSandboxUserConfiguredPaths, user_configured_paths),
    ]
    .into_iter()
    .map(|(id, property)| (id.as_str().to_owned(), property))
    .collect()
}
