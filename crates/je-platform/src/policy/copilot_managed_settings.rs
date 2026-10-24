//! Porte de `src/vs/platform/policy/common/copilotManagedSettings.ts`: as chaves das configurações
//! gerenciadas, a normalização do documento ([`normalize_managed_settings`]), a resolução entre os canais de
//! entrega ([`pick_managed_settings`], [`resolve_force_remote_settings_refresh`]), os callbacks `value` das
//! políticas ([`managed_setting_value`], [`managed_model_value`], [`managed_settings_disabled_value`],
//! [`third_party_agent_enabled_value`]), o serviço efetivo ([`IManagedSettingsService`]), o serviço nativo
//! ([`INativeManagedSettingsService`]), o serviço de arquivo ([`IFileManagedSettingsService`]) e a projeção
//! por declaração ([`collect_managed_settings_definitions`], [`project_managed_settings`]).
//!
//! Decisões de tradução:
//! - **`RawManagedSettingsData`** (`Readonly<Record<string, unknown>>`) é um [`Dynamic`] que é sempre um
//!   objeto: o documento do arquivo como o `JSON.parse` o devolve.
//! - **`ManagedSettingsData`** é o `IndexMap<String, PolicyValue>` de `je_base::policy`.
//! - **`ManagedSettingsChannel`** (a união de strings `'server' | 'nativeMdm' | 'file'`) é um `enum`, com
//!   [`ManagedSettingsChannel::as_str`] para o texto do original.
//! - **`onWarn`** é um `Option<&dyn Fn(&str)>` ([`OnWarn`]).
//! - O `for...in` sobre o objeto lido (que enumera também as chaves herdadas) é a enumeração das
//!   propriedades próprias: o [`Dynamic`] não tem protótipos, e o documento vem do JSON.
//! - `ref` de uma origem de marketplace só vale se for texto não vazio (o tipo do original é `string`).
//! - `managedModelValue()` devolve sempre o mesmo [`PolicyValueFn`] (a mesma função por `Rc::ptr_eq`), como
//!   o original memoiza o callback; o mesmo vale para [`managed_setting_value`], memoizado por chave.
//! - **`ManagedSettingsData`** não tem o buraco `undefined` do original (`{ a: undefined }`): uma chave ausente
//!   e uma chave indefinida se comportam igual na resolução, e o mapa só guarda valores definidos.
//! - **`managedSettings` de uma `PolicyDefinition`** é um [`Dynamic`] (o tipo da definição de política não
//!   conhece `IManagedSettingsPolicyDefinitions`); [`collect_managed_settings_definitions`] o converte para o
//!   mapa tipado de `je_base::policy`, ignorando uma declaração cujo `type` não é `string`, `number` nem
//!   `boolean` (o tipo do original impede essa forma).
//! - **`MANAGED_SETTINGS_CONTROL_DEFINITIONS`** é um `LazyLock` (o literal constante do original).
//! - **`managedSettingsDisabledValue` e `thirdPartyAgentEnabledValue`** devolvem `Option<PolicyValue>` (o
//!   `boolean | undefined` do original) para plugarem direto em `PolicyValueFn::new`, como o original os passa
//!   como `value` de uma política.
//! - **`IManagedSettingsService.onDidChangeManagedSettings`** (`Event<void>`) é `Event<()>`.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::sync::LazyLock;

use indexmap::IndexMap;
use je_base::JsString;
use je_base::default_account::IPolicyData;
use je_base::dynamic::{Dynamic, json_stringify, number_to_js_string};
use je_base::event::Event;
use je_base::json::{self, ParseOptions, Value};
use je_base::managed_settings::{ExtraKnownMarketplace, ExtraKnownMarketplaceSource, IExtraKnownMarketplaceEntry, extra_known_marketplaces_to_config_dict};
pub use je_base::policy::ManagedSettingsData;
use je_base::policy::{IManagedSettingPolicyDefinition, IManagedSettingsPolicyDefinitions, ManagedSettingType, ManagedSettingValue, PolicyValue, PolicyValueFn};
use je_base::promise::Promise;
use je_base::types::{is_empty_object, is_object};
use je_instantiation::service_id;

use super::policy::{PolicyDefinitionRef, PolicyDefinitions};
use crate::configuration::service::IConfigurationService;
use crate::configuration::{ConfigurationOverrides, ConfigurationValue};
use crate::js::{to_js_string, trim};

/// `RawManagedSettingsData`: o documento como veio do arquivo, sempre um objeto.
pub type RawManagedSettingsData = Dynamic;

/// O `{}` de `RawManagedSettingsData`.
pub fn empty_raw_managed_settings() -> RawManagedSettingsData {
    Dynamic::object(Vec::<(String, Dynamic)>::new())
}

/// `onWarn?: (msg: string) => void`.
pub type OnWarn<'a> = Option<&'a dyn Fn(&str)>;

/// The temporary harness control does not contribute general presence-based governance.
pub fn is_managed_setting_governance_active(key: &str) -> bool {
    key != COPILOT_TEMPORARY_ALLOW_LOCAL_HARNESS_KEY && !key.starts_with(&format!("{COPILOT_TEMPORARY_ALLOW_LOCAL_HARNESS_KEY}."))
}

/// Detect raw governance without counting the Local-only harness control or its containing object.
pub fn has_raw_managed_settings(data: Option<&RawManagedSettingsData>) -> bool {
    let Some(data) = data else { return false };
    data.own_entries().into_iter().any(|(key, value)| {
        let key = key.to_string_lossy();
        if key == "vscode" && is_managed_settings_object(&value) && value.own_keys().iter().any(|k| *k == "temporaryAllowLocalHarness") {
            return value.own_keys().into_iter().any(|key| is_managed_setting_governance_active(&format!("vscode.{}", key.to_string_lossy())));
        }
        is_managed_setting_governance_active(&key)
    })
}

/// Windows registry root for GitHub Copilot policies.
pub const GITHUB_COPILOT_WIN32_REGISTRY_PATH: &str = "SOFTWARE\\Policies\\GitHubCopilot";

/// Windows product name passed to the native policy watcher.
pub const GITHUB_COPILOT_WIN32_POLICY_NAME: &str = "GitHubCopilot";

/// macOS CFPreferences application ID for GitHub Copilot managed preferences.
pub const GITHUB_COPILOT_MACOS_BUNDLE_ID: &str = "com.github.copilot";

/// MDM key for the V0 managed setting.
pub const COPILOT_DISABLE_BYPASS_PERMISSIONS_MODE_KEY: &str = "permissions.disableBypassPermissionsMode";

/// Runtime-owned rule lists that the Local harness cannot enforce.
pub const COPILOT_PERMISSION_RULES_KEYS: [&str; 3] = ["permissions.ask", "permissions.allow", "permissions.deny"];

/// Managed-settings key for enterprise plugin enablement (carried as a JSON-encoded `{ [pluginId]: boolean }`).
pub const COPILOT_ENABLED_PLUGINS_KEY: &str = "enabledPlugins";

/// Managed-settings key for enterprise marketplaces (carried as a JSON-encoded `{ [name]: url-or-shorthand }`).
pub const COPILOT_EXTRA_MARKETPLACES_KEY: &str = "extraKnownMarketplaces";

/// Managed-settings key for the strict-marketplace allowlist (carried as a JSON-encoded array of source entries; absent = no restrictions, `[]` = lockdown).
pub const COPILOT_STRICT_MARKETPLACES_KEY: &str = "strictKnownMarketplaces";

/// Managed-settings key for the per-server MCP allowlist (carried as a JSON-encoded array of matcher entries; absent = no allow restriction, `[]` = only servers matching an entry, i.e. block all).
pub const COPILOT_ALLOWED_MCP_SERVERS_KEY: &str = "allowedMcpServers";

/// Managed-settings key for the per-server MCP denylist (carried as a JSON-encoded array of matcher entries; deny always takes precedence over allow).
pub const COPILOT_DENIED_MCP_SERVERS_KEY: &str = "deniedMcpServers";

/// Managed-settings key that blocks standalone user/workspace customizations.
pub const COPILOT_STRICT_PLUGIN_ONLY_CUSTOMIZATION_KEY: &str = "strictPluginOnlyCustomization";

/// Managed-settings key that makes the enterprise MCP allowlist authoritative.
pub const COPILOT_ALLOW_MANAGED_MCP_SERVERS_ONLY_KEY: &str = "allowManagedMcpServersOnly";

/// Managed-settings key that allows hooks only from managed sources.
pub const COPILOT_ALLOW_MANAGED_HOOKS_ONLY_KEY: &str = "allowManagedHooksOnly";

/// Managed-settings transport control that requires a fresh server fetch on startup.
pub const COPILOT_FORCE_REMOTE_SETTINGS_REFRESH_KEY: &str = "forceRemoteSettingsRefresh";

/// Enterprise escape hatch for automatic Copilot harness routing in VS Code.
pub const COPILOT_TEMPORARY_DEFAULT_TO_LOCAL_HARNESS_KEY: &str = "vscode.temporaryDefaultToLocalHarness";

/// Temporary managed permission for Local, Claude, and Codex harnesses; only false prohibits them.
pub const COPILOT_TEMPORARY_ALLOW_LOCAL_HARNESS_KEY: &str = "vscode.temporaryAllowLocalHarness";

/// Policy-only configuration projection of the Local harness escape hatch.
pub const TEMPORARY_DEFAULT_TO_LOCAL_HARNESS_SETTING_ID: &str = "chat.temporaryDefaultToLocalHarness";

/// Enterprise-mandated sandbox floor (`sandbox.enabled` in the runtime's managed-settings schema).
/// The runtime owns composing and enforcing this floor: it is `force-on-wins`, so a managed `true`
/// cannot be loosened by the user. VS Code only *reads* it to decide which chat harness to offer,
/// and deliberately declares no configuration policy for it.
pub const COPILOT_SANDBOX_ENABLED_KEY: &str = "sandbox.enabled";

/// Managed-settings key that requires MCP servers to remain sandboxed.
pub const COPILOT_SANDBOX_MCP_SERVERS_KEY: &str = "sandbox.sandboxMcpServers";

/// Managed-settings key that requires LSP servers to remain sandboxed.
pub const COPILOT_SANDBOX_LSP_SERVERS_KEY: &str = "sandbox.sandboxLspServers";

/// Managed-settings key that restricts HTTPS Git authentication inside the sandbox.
pub const COPILOT_SANDBOX_AUTH_GIT_KEY: &str = "sandbox.auth.git";

/// Managed-settings key that restricts GitHub CLI authentication inside the sandbox.
pub const COPILOT_SANDBOX_AUTH_GH_KEY: &str = "sandbox.auth.gh";

/// Managed-settings key that permits explicitly bypassing the sandbox.
pub const COPILOT_SANDBOX_ALLOW_BYPASS_KEY: &str = "sandbox.allowBypass";

/// Managed-settings key that restricts outbound sandbox network access.
pub const COPILOT_SANDBOX_ALLOW_OUTBOUND_KEY: &str = "sandbox.userPolicy.network.allowOutbound";

/// Managed-settings key that restricts local-network access from the sandbox.
pub const COPILOT_SANDBOX_ALLOW_LOCAL_NETWORK_KEY: &str = "sandbox.userPolicy.network.allowLocalNetwork";

/// Managed-settings key that supplies the sandbox's network allowlist.
pub const COPILOT_SANDBOX_ALLOWED_HOSTS_KEY: &str = "sandbox.userPolicy.network.allowedHosts";

/// Managed-settings key that supplies writable sandbox paths.
pub const COPILOT_SANDBOX_READWRITE_PATHS_KEY: &str = "sandbox.userPolicy.filesystem.readwritePaths";

/// Managed-settings key that supplies read-only sandbox paths.
pub const COPILOT_SANDBOX_READONLY_PATHS_KEY: &str = "sandbox.userPolicy.filesystem.readonlyPaths";

/// Managed-settings key that supplies denied sandbox paths.
pub const COPILOT_SANDBOX_DENIED_PATHS_KEY: &str = "sandbox.userPolicy.filesystem.deniedPaths";

/// Managed-settings key that restricts developer tool access from the sandbox.
pub const COPILOT_SANDBOX_ALLOW_DEV_TOOL_ACCESS_KEY: &str = "sandbox.allowDevToolAccess";

/// Auto startup default, read as one atomic value across managed delivery channels.
pub const COPILOT_AUTO_TIER_KEY: &str = "autoTier";

/// `IManagedSettingPolicyDefinition` com o `type` dado.
pub fn managed_setting_definition(type_: ManagedSettingType) -> IManagedSettingPolicyDefinition {
    IManagedSettingPolicyDefinition { r#type: type_, extra: Default::default() }
}

/// Managed-settings controls consumed by the delivery pipeline itself rather than by a
/// configuration policy. Native MDM must watch these even though no setting declares them.
pub static MANAGED_SETTINGS_CONTROL_DEFINITIONS: LazyLock<IManagedSettingsPolicyDefinitions> = LazyLock::new(|| {
    use ManagedSettingType::{Boolean, String as Text};
    let permission_rules = COPILOT_PERMISSION_RULES_KEYS.into_iter().map(|key| (key, Text));
    let controls = [
        ("permissions.defaultMode", Text),
        ("permissions.disableAssistedPermissionsMode", Boolean),
        (COPILOT_DISABLE_BYPASS_PERMISSIONS_MODE_KEY, Text),
        ("permissions.limitTo", Text),
        ("permissions.model", Text),
        (COPILOT_FORCE_REMOTE_SETTINGS_REFRESH_KEY, Boolean),
        (COPILOT_STRICT_MARKETPLACES_KEY, Text),
        (COPILOT_TEMPORARY_ALLOW_LOCAL_HARNESS_KEY, Boolean),
        (COPILOT_SANDBOX_ENABLED_KEY, Boolean),
        (COPILOT_SANDBOX_MCP_SERVERS_KEY, Boolean),
        (COPILOT_SANDBOX_LSP_SERVERS_KEY, Boolean),
        (COPILOT_SANDBOX_AUTH_GIT_KEY, Boolean),
        (COPILOT_SANDBOX_AUTH_GH_KEY, Boolean),
        (COPILOT_SANDBOX_ALLOW_BYPASS_KEY, Boolean),
        (COPILOT_SANDBOX_ALLOW_OUTBOUND_KEY, Boolean),
        (COPILOT_SANDBOX_ALLOW_LOCAL_NETWORK_KEY, Boolean),
        (COPILOT_SANDBOX_ALLOWED_HOSTS_KEY, Text),
        (COPILOT_SANDBOX_READWRITE_PATHS_KEY, Text),
        (COPILOT_SANDBOX_READONLY_PATHS_KEY, Text),
        (COPILOT_SANDBOX_DENIED_PATHS_KEY, Text),
        (COPILOT_SANDBOX_ALLOW_DEV_TOOL_ACCESS_KEY, Boolean),
        (COPILOT_AUTO_TIER_KEY, Text),
        // Observe these only for whole-block source selection; Local does not implement their capture semantics.
        ("telemetry.capture.prompts", Boolean),
        ("telemetry.capture.responses", Boolean),
        ("telemetry.capture.toolArguments", Boolean),
        ("telemetry.capture.toolOutput", Boolean),
        ("telemetry.capture.policyDetail", Boolean),
    ];
    permission_rules.chain(controls).map(|(key, type_)| (key.to_owned(), managed_setting_definition(type_))).collect()
});

/// Policy-only configuration delivery slot for [`COPILOT_STRICT_PLUGIN_ONLY_CUSTOMIZATION_KEY`].
pub const COPILOT_STRICT_PLUGIN_ONLY_CUSTOMIZATION_CONFIG: &str = "chat.customizations.strictPluginOnlyCustomization";

/// Policy-only configuration delivery slot for [`COPILOT_ALLOW_MANAGED_MCP_SERVERS_ONLY_KEY`].
pub const COPILOT_ALLOW_MANAGED_MCP_SERVERS_ONLY_CONFIG: &str = "chat.mcp.allowManagedServersOnly";

/// Policy-only configuration delivery slot for [`COPILOT_ALLOW_MANAGED_HOOKS_ONLY_KEY`].
pub const COPILOT_ALLOW_MANAGED_HOOKS_ONLY_CONFIG: &str = "chat.hooks.allowManagedOnly";

/// Legacy managed-settings key for the default chat model, nested under `permissions` so it flattens
/// to `permissions.model`. Retained for original-schema deployments; superseded by the top-level
/// [`COPILOT_TOP_LEVEL_MODEL_KEY`], which wins when both are present (see [`managed_model_value`]).
pub const COPILOT_MODEL_KEY: &str = "permissions.model";

/// Canonical top-level managed-settings key for the default chat model (flattens to the bag key
/// `model`). Supersedes the legacy nested [`COPILOT_MODEL_KEY`] when both are present.
pub const COPILOT_TOP_LEVEL_MODEL_KEY: &str = "model";

/// Enterprise OTel managed-settings keys. These are the scalar leaves of the canonical
/// `telemetry` block from the cross-client managed-settings schema; they flatten to dot-path bag keys via
/// [`normalize_managed_settings`], so no structured-table entry is needed. The
/// `telemetry.resourceAttributes` and `telemetry.headers` map fields are structured (rows of the structured
/// table carry them as JSON-encoded objects under their nested keys); `telemetry.serviceName` is a scalar.
///
/// Managed-settings key for enterprise OTel enablement.
pub const COPILOT_OTEL_ENABLED_KEY: &str = "telemetry.enabled";

/// Managed-settings key for the enterprise OTLP collector endpoint.
pub const COPILOT_OTEL_ENDPOINT_KEY: &str = "telemetry.endpoint";

/// Managed-settings key for the enterprise OTel file export destination.
pub const COPILOT_OTEL_FILE_PATH_KEY: &str = "telemetry.filePath";

/// Managed-settings key for the enterprise OTLP protocol (`http/json`, `http/protobuf`, or `grpc`).
pub const COPILOT_OTEL_PROTOCOL_KEY: &str = "telemetry.protocol";

/// Managed-settings key for enterprise OTel content capture.
pub const COPILOT_OTEL_CAPTURE_CONTENT_KEY: &str = "telemetry.captureContent";

/// Managed-settings key for independently governed OTel identity capture.
pub const COPILOT_OTEL_CAPTURE_IDENTITY_KEY: &str = "telemetry.capture.identity";

/// Managed-settings key that prevents users from enabling OTel content capture themselves.
pub const COPILOT_OTEL_LOCK_CAPTURE_CONTENT_KEY: &str = "telemetry.lockCaptureContent";

/// Managed-settings key for the OTel `service.name` resource attribute.
pub const COPILOT_OTEL_SERVICE_NAME_KEY: &str = "telemetry.serviceName";

/// Managed-settings key for the OTel `resourceAttributes` map (a `{ [k]: string }` map).
pub const COPILOT_OTEL_RESOURCE_ATTRIBUTES_KEY: &str = "telemetry.resourceAttributes";

/// Managed-settings key for extra OTLP exporter headers (a `{ [k]: string }` map).
pub const COPILOT_OTEL_HEADERS_KEY: &str = "telemetry.headers";

/// An empty normalized telemetry block is carried as the JSON string `{}` under `telemetry`.
/// This preserves block presence across IPC and caching without inventing a policy leaf. Like
/// undeclared telemetry leaves, this anchor is discarded by policy projection, not by selection.
const TELEMETRY_BLOCK_KEY: &str = "telemetry";

/// Whether a canonical bag key belongs to the telemetry block.
fn is_telemetry_setting_key(key: &str) -> bool {
    key == TELEMETRY_BLOCK_KEY || key.starts_with(&format!("{TELEMETRY_BLOCK_KEY}."))
}

thread_local! {
    static MANAGED_SETTING_VALUE_CALLBACKS: RefCell<HashMap<String, PolicyValueFn>> = RefCell::new(HashMap::new());
}

/// Standard pass-through `value` callback for a managed-settings-driven policy: locks the setting
/// to the managed value when the enterprise has set it, and returns `undefined` otherwise so the
/// user's own setting falls through. Use for the common case; policies that combine the managed
/// value with other conditions (e.g. `chat_preview_features_enabled`) keep a custom callback.
///
/// The callback is memoized per key, so repeated calls for the same key return the SAME function
/// (the same `Rc`, compared with `==`). That identity is what lets `isSamePolicyDefinition` skip needless
/// re-registration, and memoizing makes the guarantee hold regardless of where the helper is called.
pub fn managed_setting_value(key: &str) -> PolicyValueFn {
    MANAGED_SETTING_VALUE_CALLBACKS.with(|callbacks| {
        callbacks
            .borrow_mut()
            .entry(key.to_owned())
            .or_insert_with(|| {
                let key = key.to_owned();
                PolicyValueFn::new(move |policy_data: &IPolicyData| policy_data.managed_settings.as_ref().and_then(|settings| settings.get(&key)).cloned())
            })
            .clone()
    })
}

/// `IForceRemoteSettingsRefreshResolution`: `{ effective: true, source } | { effective: false }`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IForceRemoteSettingsRefreshResolution {
    Effective { source: ManagedSettingsChannel },
    NotEffective,
}

/// Resolve the fail-closed startup refresh control across every delivery channel, reusing
/// [`pick_managed_settings`] precedence rather than re-implementing it. A non-boolean value is
/// treated as absent, so a malformed high-precedence value cannot mask a well-formed lower one.
pub fn resolve_force_remote_settings_refresh(
    native_mdm: Option<&ManagedSettingsData>,
    server: Option<&ManagedSettingsData>,
    file: Option<&ManagedSettingsData>,
) -> IForceRemoteSettingsRefreshResolution {
    let pick = pick_managed_settings(native_mdm, server, file);
    let contribution = pick
        .resolutions
        .get(COPILOT_FORCE_REMOTE_SETTINGS_REFRESH_KEY)
        .and_then(|resolution| resolution.contributions.iter().find(|candidate| matches!(candidate.value, PolicyValue::Boolean(_))));
    match contribution {
        Some(contribution) if contribution.value == PolicyValue::Boolean(true) => IForceRemoteSettingsRefreshResolution::Effective { source: contribution.channel },
        _ => IForceRemoteSettingsRefreshResolution::NotEffective,
    }
}

service_id!(pub I_MANAGED_SETTINGS_SERVICE: dyn IManagedSettingsService = "managedSettingsService");

/// Read-only access to effective managed settings after channel resolution.
pub trait IManagedSettingsService {
    fn on_did_change_managed_settings(&self) -> Event<()>;
    fn get_managed_settings(&self) -> ManagedSettingsData;
    fn get_managed_setting_value(&self, key: &str) -> Option<ManagedSettingValue>;
}

/// `NullManagedSettingsService`.
#[derive(Clone, Copy, Debug, Default)]
pub struct NullManagedSettingsService;

impl IManagedSettingsService for NullManagedSettingsService {
    fn on_did_change_managed_settings(&self) -> Event<()> {
        Event::none()
    }

    fn get_managed_settings(&self) -> ManagedSettingsData {
        ManagedSettingsData::new()
    }

    fn get_managed_setting_value(&self, _key: &str) -> Option<ManagedSettingValue> {
        None
    }
}

/// Non-empty or malformed managed deny lists require runtime enforcement; rule interpretation remains runtime-owned.
pub fn has_managed_permission_denials(service: &dyn IManagedSettingsService) -> bool {
    let Some(value) = service.get_managed_setting_value("permissions.deny") else { return false };
    let PolicyValue::String(text) = value else { return true };
    let units: Vec<u16> = text.encode_utf16().collect();
    let mut errors = Vec::new();
    let rules = json::parse(&units, &mut errors, ParseOptions { disallow_comments: true, allow_trailing_comma: false, allow_empty_content: false });
    !errors.is_empty() || !matches!(rules, Some(Value::Array(ref items)) if items.is_empty())
}

/// O `policyValue` da configuração do escape hatch Local: `undefined` quando nenhuma política o define.
fn temporary_default_to_local_harness_policy_value(configuration_service: &dyn IConfigurationService) -> Dynamic {
    configuration_service.inspect(TEMPORARY_DEFAULT_TO_LOCAL_HARNESS_SETTING_ID, &ConfigurationOverrides::default()).policy_value()
}

/// Whether an administrator has enabled the temporary Local harness escape hatch.
pub fn is_temporary_default_to_local_harness_enabled(configuration_service: &dyn IConfigurationService) -> bool {
    temporary_default_to_local_harness_policy_value(configuration_service).strict_equals(&Dynamic::Bool(true))
}

/// Whether effective policy requires Agent Host; explicit Local disablement overrides the temporary Local escape hatch.
pub fn requires_copilot_agent_host(service: &dyn IManagedSettingsService, configuration_service: &dyn IConfigurationService) -> bool {
    if service.get_managed_setting_value(COPILOT_TEMPORARY_ALLOW_LOCAL_HARNESS_KEY) == Some(PolicyValue::Boolean(false)) {
        return true;
    }
    // `policyValue ?? managedSettingValue`: um `policyValue` nulo ou `undefined` cai para o valor gerenciado.
    let policy_value = temporary_default_to_local_harness_policy_value(configuration_service);
    let temporary_default_to_local = if policy_value.is_nullish() {
        service.get_managed_setting_value(COPILOT_TEMPORARY_DEFAULT_TO_LOCAL_HARNESS_KEY) == Some(PolicyValue::Boolean(true))
    } else {
        policy_value.strict_equals(&Dynamic::Bool(true))
    };
    if temporary_default_to_local {
        return false;
    }
    service.get_managed_setting_value(COPILOT_SANDBOX_ENABLED_KEY) == Some(PolicyValue::Boolean(true)) || has_managed_permission_denials(service)
}

/// Normalize a managed-settings model value: trim, treating a blank/whitespace-only string as unset.
fn normalize_model_value(value: Option<&ManagedSettingValue>) -> Option<String> {
    let PolicyValue::String(text) = value? else { return None };
    let trimmed = trim(&JsString::from(text.as_str())).to_string_lossy();
    (!trimmed.is_empty()).then_some(trimmed)
}

thread_local! {
    static MANAGED_MODEL_VALUE: PolicyValueFn = PolicyValueFn::new(|policy_data: &IPolicyData| {
        let setting = |key: &str| policy_data.managed_settings.as_ref().and_then(|settings| settings.get(key));
        normalize_model_value(setting(COPILOT_TOP_LEVEL_MODEL_KEY)).or_else(|| normalize_model_value(setting(COPILOT_MODEL_KEY))).map(PolicyValue::String)
    });
}

/// `value` callback for the default-chat-model managed setting: resolves the top-level
/// [`COPILOT_TOP_LEVEL_MODEL_KEY`] first, falling back to the legacy nested [`COPILOT_MODEL_KEY`]
/// (each trimmed, blank treated as unset), so the top-level value wins when both are present. Memoized
/// so repeated calls return the same reference.
pub fn managed_model_value() -> PolicyValueFn {
    MANAGED_MODEL_VALUE.with(PolicyValueFn::clone)
}

/// Forces a boolean setting off while the user is governed by managed settings.
pub fn managed_settings_disabled_value(policy_data: &IPolicyData) -> Option<PolicyValue> {
    (policy_data.managed_settings_active == Some(true)).then_some(PolicyValue::Boolean(false))
}

/// `value` callback shared by the third-party agent harness policies (`Claude3PIntegration`,
/// `Codex3PIntegration`): forces the harness off when the account disables chat preview features,
/// when the user is governed by managed settings, or when Local harnesses are explicitly prohibited.
///
/// Managed settings are composed and enforced by the Copilot runtime and never reach the Claude or
/// Codex harnesses, so leaving them available would hand a governed user an ungoverned path around
/// every managed control the enterprise set.
pub fn third_party_agent_enabled_value(policy_data: &IPolicyData) -> Option<PolicyValue> {
    let local_harness_prohibited = policy_data.managed_settings.as_ref().and_then(|settings| settings.get(COPILOT_TEMPORARY_ALLOW_LOCAL_HARNESS_KEY)) == Some(&PolicyValue::Boolean(false));
    if policy_data.chat_preview_features_enabled == Some(false) || local_harness_prohibited {
        Some(PolicyValue::Boolean(false))
    } else {
        managed_settings_disabled_value(policy_data)
    }
}

service_id!(pub I_NATIVE_MANAGED_SETTINGS_SERVICE: dyn INativeManagedSettingsService = "nativeManagedSettingsService");

pub trait INativeManagedSettingsService {
    fn managed_settings(&self) -> ManagedSettingsData;
    fn on_did_change_managed_settings(&self) -> Event<ManagedSettingsData>;
    fn initialize(&self) -> Promise<ManagedSettingsData>;
    fn update_policy_definitions(&self, policy_definitions: PolicyDefinitions) -> Promise<ManagedSettingsData>;
}

/// `NullNativeManagedSettingsService`.
#[derive(Clone, Copy, Debug, Default)]
pub struct NullNativeManagedSettingsService;

impl INativeManagedSettingsService for NullNativeManagedSettingsService {
    fn managed_settings(&self) -> ManagedSettingsData {
        ManagedSettingsData::new()
    }

    fn on_did_change_managed_settings(&self) -> Event<ManagedSettingsData> {
        Event::none()
    }

    fn initialize(&self) -> Promise<ManagedSettingsData> {
        Promise::resolved(self.managed_settings())
    }

    fn update_policy_definitions(&self, _policy_definitions: PolicyDefinitions) -> Promise<ManagedSettingsData> {
        Promise::resolved(self.managed_settings())
    }
}

/// O `type` declarado de uma entrada de `managedSettings`; `None` para uma forma que o tipo do original não admite.
fn declared_managed_setting_type(declared: &Dynamic) -> Option<ManagedSettingType> {
    match declared.get_str("type") {
        Dynamic::String(text) => match text.to_string_lossy().as_str() {
            "string" => Some(ManagedSettingType::String),
            "number" => Some(ManagedSettingType::Number),
            "boolean" => Some(ManagedSettingType::Boolean),
            _ => None,
        },
        _ => None,
    }
}

/// `typeof` de um valor gerenciado e o `type` declarado: o mesmo vocabulário (`string`, `number`, `boolean`).
fn managed_setting_type_name(type_: ManagedSettingType) -> &'static str {
    match type_ {
        ManagedSettingType::String => "string",
        ManagedSettingType::Number => "number",
        ManagedSettingType::Boolean => "boolean",
    }
}

fn managed_setting_value_type_name(value: &ManagedSettingValue) -> &'static str {
    match value {
        PolicyValue::String(_) => "string",
        PolicyValue::Number(_) => "number",
        PolicyValue::Boolean(_) => "boolean",
    }
}

/// Aggregate the `managedSettings` declarations of every policy definition into a single
/// key -> definition map. This is the single source of truth for policy-backed Copilot
/// managed-settings keys and drives both server projection and the declaration-driven portion of
/// the native MDM watcher. Transport controls are declared separately.
pub fn collect_managed_settings_definitions(policy_definitions: &[(String, PolicyDefinitionRef)]) -> IManagedSettingsPolicyDefinitions {
    let mut definitions = IManagedSettingsPolicyDefinitions::new();
    for (_, definition) in policy_definitions {
        let Some(policy_managed_settings) = &definition.managed_settings else { continue };
        for (key, declared) in policy_managed_settings.own_entries() {
            if let Some(type_) = declared_managed_setting_type(&declared) {
                definitions.insert(key.to_string_lossy(), managed_setting_definition(type_));
            }
        }
    }
    definitions
}

/// Whether any policy in `policy_definitions` declares at least one managed-settings key. Cheap
/// existence check (short-circuits) used to decide whether the declaration-driven portion of the
/// native MDM watcher needs updating, without aggregating the full
/// [`collect_managed_settings_definitions`] map.
pub fn has_managed_settings_definitions(policy_definitions: &[(String, PolicyDefinitionRef)]) -> bool {
    policy_definitions.iter().any(|(_, definition)| definition.managed_settings.as_ref().is_some_and(|managed_settings| !is_empty_object(managed_settings)))
}

/// Project a raw managed-settings bag onto the declared schema: keep only keys declared by a
/// policy definition whose runtime value matches the declared type. Undeclared keys and
/// type-mismatched values are dropped (with an optional warning). Values are validated, never
/// coerced, so a key declared as `string` keeps its string value untouched.
///
/// This keeps the server endpoint and native MDM delivery aligned on the same
/// declaration-driven key set and value types.
pub fn project_managed_settings(values: &ManagedSettingsData, definitions: &IManagedSettingsPolicyDefinitions, on_warn: OnWarn) -> ManagedSettingsData {
    let mut projected = ManagedSettingsData::new();
    for (key, definition) in definitions {
        let Some(value) = values.get(key) else { continue };
        let expected = managed_setting_type_name(definition.r#type);
        let actual = managed_setting_value_type_name(value);
        if actual == expected {
            projected.insert(key.clone(), value.clone());
        } else if let Some(on_warn) = on_warn {
            on_warn(&format!("Ignoring managed setting \"{key}\": expected {expected}, got {actual}"));
        }
    }
    projected
}

/// A delivery channel that can provide managed settings. Managed settings can be delivered by more
/// than one channel, so this names the known sources to give policy evaluation and the Policy
/// Diagnostics report one shared vocabulary.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ManagedSettingsChannel {
    /// GitHub `/copilot_internal/managed_settings` endpoint (server-delivered).
    Server,
    /// Native MDM: OS registry (Windows) / managed preferences (macOS) via `@vscode/policy-watcher`.
    NativeMdm,
    /// File on a well-known disk path (`managed-settings.json`).
    File,
}

impl ManagedSettingsChannel {
    /// O texto do original (`'server' | 'nativeMdm' | 'file'`).
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Server => "server",
            Self::NativeMdm => "nativeMdm",
            Self::File => "file",
        }
    }
}

/// The source attributed to an effective managed setting (or to the overall report). A
/// [`ManagedSettingsChannel`] once a channel has won, or `None` (`'none'`) when no channel contributes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ManagedSettingsSource {
    Channel(ManagedSettingsChannel),
    None,
}

/// The delivery channels in fixed precedence order (highest first): native MDM, server-delivered,
/// file on disk. This single ordered list drives the per-key resolution in [`pick_managed_settings`]
/// and is the one place to extend when a new channel is introduced. Rationale for the order: the
/// server is harder to bypass than local MDM, and a local file is the most easily tampered with.
pub const MANAGED_SETTINGS_CHANNELS: [ManagedSettingsChannel; 3] = [ManagedSettingsChannel::NativeMdm, ManagedSettingsChannel::Server, ManagedSettingsChannel::File];

/// A single channel's contribution to a managed-settings key, for provenance in the resolution.
#[derive(Clone, Debug, PartialEq)]
pub struct IManagedSettingsContribution {
    /// The channel that supplied this value.
    pub channel: ManagedSettingsChannel,
    /// The value the channel supplied for the key.
    pub value: ManagedSettingValue,
}

/// How a single managed-settings key was resolved across the delivery channels.
#[derive(Clone, Debug, PartialEq)]
pub struct IManagedSettingResolution {
    /// The effective (winning) value applied for the key.
    pub value: ManagedSettingValue,
    /// The highest-precedence channel supplying the effective value.
    pub source: ManagedSettingsChannel,
    /// Every channel that supplied this key, in delivery precedence order.
    pub contributions: Vec<IManagedSettingsContribution>,
}

/// The result of resolving managed settings across the delivery channels.
#[derive(Clone, Debug, PartialEq)]
pub struct IManagedSettingsPick {
    /// The effective bag, excluding telemetry leaves found only in weaker managed sources.
    pub values: ManagedSettingsData,
    /// Per-key provenance: how each key resolved and which channels were overridden.
    pub resolutions: IndexMap<String, IManagedSettingResolution>,
    /// Telemetry keys supplied only by weaker blocks, retained for diagnostics but never applied.
    pub suppressed_telemetry: IndexMap<String, Vec<IManagedSettingsContribution>>,
    /// The channels that supplied at least one *winning* key, in precedence order.
    pub active_sources: Vec<ManagedSettingsChannel>,
}

/// Sandbox enablement keys are force-on-wins.
fn is_force_on_wins_key(key: &str) -> bool {
    key == COPILOT_SANDBOX_ENABLED_KEY || key == COPILOT_SANDBOX_MCP_SERVERS_KEY || key == COPILOT_SANDBOX_LSP_SERVERS_KEY
}

/// Sandbox bypass, access and credential authentication keys are deny-wins.
fn is_deny_wins_key(key: &str) -> bool {
    [
        COPILOT_SANDBOX_ALLOW_BYPASS_KEY,
        COPILOT_SANDBOX_ALLOW_OUTBOUND_KEY,
        COPILOT_SANDBOX_ALLOW_LOCAL_NETWORK_KEY,
        COPILOT_SANDBOX_ALLOW_DEV_TOOL_ACCESS_KEY,
        COPILOT_SANDBOX_AUTH_GIT_KEY,
        COPILOT_SANDBOX_AUTH_GH_KEY,
    ]
    .contains(&key)
}

/// Merge the managed-settings bags from every delivery channel.
///
/// Precedence (highest first): native MDM, server-delivered, file on disk. Unlike a single
/// authoritative source, the channels *are* merged key-by-key: for each key the highest-precedence
/// channel that supplies it wins, but a key that the higher channels never set is still filled in by
/// a lower channel. Telemetry instead selects the highest-priority block in its entirety, including
/// empty or unrecognized server/file object blocks; native delivery observes declared flat keys only.
/// Omitted leaves cannot inherit from a weaker managed source.
/// Sandbox enablement is force-on-wins; bypass, access and credential authentication are deny-wins so UI consumers
/// cannot discard a sandbox restriction from another channel.
///
/// The parameter order matches the precedence so call sites read top-to-bottom. Empty or absent channels
/// contribute nothing.
pub fn pick_managed_settings(native_mdm: Option<&ManagedSettingsData>, server: Option<&ManagedSettingsData>, file: Option<&ManagedSettingsData>) -> IManagedSettingsPick {
    // Preserve delivery order for provenance even when a sandbox requirement wins from a later channel.
    let mut resolutions: IndexMap<String, IManagedSettingResolution> = IndexMap::new();
    let mut suppressed_telemetry: IndexMap<String, Vec<IManagedSettingsContribution>> = IndexMap::new();
    let mut telemetry_source: Option<ManagedSettingsChannel> = None;
    for channel in MANAGED_SETTINGS_CHANNELS {
        let bag = match channel {
            ManagedSettingsChannel::NativeMdm => native_mdm,
            ManagedSettingsChannel::Server => server,
            ManagedSettingsChannel::File => file,
        };
        let Some(bag) = bag else { continue };
        for (key, value) in bag {
            let existing = resolutions.contains_key(key);
            if is_telemetry_setting_key(key) {
                let source = *telemetry_source.get_or_insert(channel);
                if channel != source && !existing {
                    suppressed_telemetry.entry(key.clone()).or_default().push(IManagedSettingsContribution { channel, value: value.clone() });
                    continue;
                }
            }
            if let Some(resolution) = resolutions.get_mut(key) {
                resolution.contributions.push(IManagedSettingsContribution { channel, value: value.clone() });
                if (is_force_on_wins_key(key) && *value == PolicyValue::Boolean(true) && resolution.value != PolicyValue::Boolean(true))
                    || (is_deny_wins_key(key) && *value == PolicyValue::Boolean(false) && resolution.value != PolicyValue::Boolean(false))
                {
                    resolution.value = value.clone();
                    resolution.source = channel;
                }
            } else {
                resolutions.insert(
                    key.clone(),
                    IManagedSettingResolution { value: value.clone(), source: channel, contributions: vec![IManagedSettingsContribution { channel, value: value.clone() }] },
                );
            }
        }
    }

    let values: ManagedSettingsData = resolutions.iter().map(|(key, resolution)| (key.clone(), resolution.value.clone())).collect();
    let active_sources: HashSet<ManagedSettingsChannel> = resolutions.values().map(|resolution| resolution.source).collect();
    IManagedSettingsPick {
        values,
        suppressed_telemetry,
        resolutions,
        // Preserve precedence order for a stable, readable report.
        active_sources: MANAGED_SETTINGS_CHANNELS.into_iter().filter(|channel| active_sources.contains(channel)).collect(),
    }
}

/// `isManagedSettingsObject`: `typeof value === 'object' && value !== null && !Array.isArray(value)`.
fn is_managed_settings_object(value: &Dynamic) -> bool {
    value.type_of() == "object" && !value.is_null() && !value.is_array()
}

/// A propriedade própria `key` de `value`, se existe.
fn own_property(value: &Dynamic, key: &str) -> Option<Dynamic> {
    value.own_entries().into_iter().find(|(name, _)| *name == key).map(|(_, property)| property)
}

fn flatten_managed_settings(object: &Dynamic) -> ManagedSettingsData {
    let mut result = ManagedSettingsData::new();
    flatten_managed_settings_value(object, None, &mut result);
    result
}

fn flatten_managed_settings_value(value: &Dynamic, prefix: Option<&str>, result: &mut ManagedSettingsData) {
    let leaf = match value {
        Dynamic::String(text) => Some(PolicyValue::String(text.to_string_lossy())),
        Dynamic::Number(number) => Some(PolicyValue::Number(*number)),
        Dynamic::Bool(flag) => Some(PolicyValue::Boolean(*flag)),
        _ => None,
    };
    if let Some(leaf) = leaf {
        // `result[prefix] = value` num objeto literal: com a chave `__proto__` a atribuição passa pelo
        // setter herdado, que ignora valor primitivo, então nada é gravado.
        if let Some(prefix) = prefix
            && prefix != "__proto__"
        {
            result.insert(prefix.to_owned(), leaf);
        }
        return;
    }

    if !is_managed_settings_object(value) {
        return;
    }

    for (key, child) in value.own_entries() {
        let key = key.to_string_lossy();
        // `prefix ? `${prefix}.${key}` : key`: um prefixo vazio conta como ausente.
        let path = match prefix {
            Some(prefix) if !prefix.is_empty() => format!("{prefix}.{key}"),
            _ => key,
        };
        flatten_managed_settings_value(&child, Some(&path), result);
    }
}

/// macOS well-known path for file-based managed settings.
pub const MANAGED_SETTINGS_MACOS_FILE_PATH: &str = "/Library/Application Support/GitHubCopilot/managed-settings.json";

/// Linux well-known path for file-based managed settings.
pub const MANAGED_SETTINGS_LINUX_FILE_PATH: &str = "/etc/github-copilot/managed-settings.json";

/// Windows directory name under %ProgramFiles% for file-based managed settings.
pub const MANAGED_SETTINGS_WINDOWS_DIR: &str = "GitHubCopilot";

/// Managed settings file name.
pub const MANAGED_SETTINGS_FILE_NAME: &str = "managed-settings.json";

/// `(value, onWarn) => unknown`: o valor canônico antes do `JSON.stringify`; `undefined` omite a chave.
type Encode = for<'a> fn(&Dynamic, OnWarn<'a>) -> Dynamic;

/// Descriptor for a structured (object/array) managed setting: one carried across every delivery
/// channel as a canonical JSON string under a single key. This table is the single place that
/// knows how to turn a managed-settings schema field into that canonical value, so adding a
/// structured key is one row here (plus the policy declaration that reads the bag key).
///
/// `key` is both the source field name read from the parsed input and the canonical bag key the
/// JSON string is stored under: for structured settings these are identical by contract.
struct IStructuredManagedSetting {
    /// Source field name read from the parsed input, and the canonical bag key the JSON string is stored under.
    key: &'static str,
    /// Normalize the raw value into the canonical pre-stringify shape an admin authors via native
    /// MDM. Return `undefined` to omit the key (absent or malformed value). Note an empty array (the
    /// `strictKnownMarketplaces` lockdown case) is returned as-is, not omitted.
    encode: Encode,
}

/// `value => value`.
fn encode_identity(value: &Dynamic, _on_warn: OnWarn) -> Dynamic {
    value.clone()
}

/// Encode a managed-settings value into a canonical `{ [k]: string }` map: keeps string values
/// as-is and coerces number/boolean values to strings; drops keys with non-primitive values.
/// Returns `undefined` for a non-object input so the structured key is omitted.
fn encode_string_map(value: &Dynamic, _on_warn: OnWarn) -> Dynamic {
    if !is_object(value) {
        return Dynamic::Undefined;
    }
    let mut out: Vec<(JsString, Dynamic)> = Vec::new();
    for (key, item) in value.own_entries() {
        if key == "__proto__" || key == "constructor" || key == "prototype" {
            continue; // defend the shared normalizer against prototype pollution
        }
        match item {
            Dynamic::String(_) => out.push((key, item)),
            Dynamic::Number(number) => out.push((key, Dynamic::string(&number_to_js_string(number)))),
            Dynamic::Bool(flag) => out.push((key, Dynamic::string(&flag.to_string()))),
            _ => {}
        }
    }
    Dynamic::object(out)
}

/// Pass an object value through unchanged; omit the key for any non-object value.
fn encode_object(value: &Dynamic, _on_warn: OnWarn) -> Dynamic {
    if is_object(value) { value.clone() } else { Dynamic::Undefined }
}

/// Pass an array value through unchanged (including an empty array); omit the key otherwise.
fn encode_array(value: &Dynamic, _on_warn: OnWarn) -> Dynamic {
    if value.is_array() { value.clone() } else { Dynamic::Undefined }
}

/// Encode the schema's `{ [id]: { source, autoUpdate? } }` marketplace map into the canonical
/// policy dict; drops malformed entries (with an optional warning) and omits
/// the key when there are none.
fn encode_extra_marketplaces(value: &Dynamic, on_warn: OnWarn) -> Dynamic {
    let entries = normalize_extra_known_marketplaces(value, on_warn);
    match extra_known_marketplaces_to_config_dict(entries.as_deref()) {
        Some(dict) => Dynamic::object(dict.into_iter().map(|(name, source)| (name, Dynamic::string(&source)))),
        None => Dynamic::Undefined,
    }
}

const STRUCTURED_MANAGED_SETTINGS: [IStructuredManagedSetting; 15] = [
    IStructuredManagedSetting { key: COPILOT_PERMISSION_RULES_KEYS[0], encode: encode_identity },
    IStructuredManagedSetting { key: COPILOT_PERMISSION_RULES_KEYS[1], encode: encode_identity },
    IStructuredManagedSetting { key: COPILOT_PERMISSION_RULES_KEYS[2], encode: encode_identity },
    IStructuredManagedSetting { key: COPILOT_SANDBOX_ALLOWED_HOSTS_KEY, encode: encode_array },
    IStructuredManagedSetting { key: COPILOT_SANDBOX_READWRITE_PATHS_KEY, encode: encode_array },
    IStructuredManagedSetting { key: COPILOT_SANDBOX_READONLY_PATHS_KEY, encode: encode_array },
    IStructuredManagedSetting { key: COPILOT_SANDBOX_DENIED_PATHS_KEY, encode: encode_array },
    IStructuredManagedSetting { key: COPILOT_AUTO_TIER_KEY, encode: encode_identity },
    IStructuredManagedSetting { key: COPILOT_ENABLED_PLUGINS_KEY, encode: encode_object },
    IStructuredManagedSetting { key: COPILOT_STRICT_MARKETPLACES_KEY, encode: encode_array },
    IStructuredManagedSetting { key: COPILOT_ALLOWED_MCP_SERVERS_KEY, encode: encode_array },
    IStructuredManagedSetting { key: COPILOT_DENIED_MCP_SERVERS_KEY, encode: encode_array },
    IStructuredManagedSetting { key: COPILOT_EXTRA_MARKETPLACES_KEY, encode: encode_extra_marketplaces },
    // Nested under `telemetry`; carried as a JSON-encoded `{ [k]: string }` map. Non-string
    // primitive values are coerced to strings; non-primitive values are dropped.
    IStructuredManagedSetting { key: COPILOT_OTEL_RESOURCE_ATTRIBUTES_KEY, encode: encode_string_map },
    // Nested under `telemetry`; carried as a JSON-encoded `{ [k]: string }` map of OTLP headers.
    IStructuredManagedSetting { key: COPILOT_OTEL_HEADERS_KEY, encode: encode_string_map },
];

/// Read a (possibly nested) dot-separated key from a parsed managed-settings object, e.g.
/// `telemetry.resourceAttributes`. Returns `undefined` if any path segment is missing or not an
/// object. Single-segment keys behave like a plain property read.
fn read_nested_managed_key(object: &Dynamic, dotted_key: &str) -> Dynamic {
    let mut current = object.clone();
    for segment in dotted_key.split('.') {
        if !is_object(&current) {
            return Dynamic::Undefined;
        }
        current = current.get_str(segment);
    }
    current
}

/// Return a copy of `object` with the (possibly nested) dot-separated key removed, cloning only the
/// objects along the touched path so the original (and any shared sub-objects) stay untouched.
fn with_nested_managed_key_deleted(object: &Dynamic, dotted_key: &str) -> Dynamic {
    let Some(dot) = dotted_key.find('.') else {
        return Dynamic::object(object.own_entries().into_iter().filter(|(name, _)| *name != dotted_key));
    };
    let head = &dotted_key[..dot];
    let child = object.get_str(head);
    if !is_object(&child) {
        return object.clone();
    }
    let rest = with_nested_managed_key_deleted(&child, &dotted_key[dot + 1..]);
    let mut entries = object.own_entries();
    match entries.iter_mut().find(|(name, _)| *name == head) {
        Some(entry) => entry.1 = rest,
        None => entries.push((JsString::from(head), rest)),
    }
    Dynamic::object(entries)
}

/// Normalize a parsed managed-settings object (from the server `managed_settings` API, a file on
/// disk, or any other source using the managed-settings schema) into the canonical
/// `ManagedSettingsData` bag that the policy framework consumes. This is the **single**
/// normalization path for all delivery channels, so downstream projection and policy `value()`
/// callbacks behave identically regardless of source. It does not enforce the declared
/// `managedSettings` schema: dropping undeclared or type-mismatched keys happens later, at the
/// projection.
///
/// - Scalar leaves (`permissions.*` and any forward-compatible scalar keys) are flattened into
///   dot-separated keys.
/// - Structured settings (declared in the structured table) are carried as canonical
///   JSON strings under a single key each, the same shape an admin authors via native MDM.
///   `extraKnownMarketplaces` is normalized from the schema's `{ [id]: { source, autoUpdate? } }`
///   map to the policy-backed marketplace dict.
/// - Telemetry blocks with no representable leaves retain a JSON `{}` anchor under `telemetry`,
///   so an empty or future-only block still takes precedence over lower managed sources.
///
/// Malformed marketplace entries are dropped (with an optional warning via `on_warn`) rather
/// than throwing, so a bad enterprise settings file degrades gracefully instead of blocking startup.
pub fn normalize_managed_settings(parsed: &Dynamic, on_warn: OnWarn) -> ManagedSettingsData {
    // Structured keys may be nested (e.g. `telemetry.resourceAttributes`), so removal clones only the touched path.
    let mut scalar_rest = Dynamic::object(parsed.own_entries());
    let has_telemetry_block = own_property(parsed, TELEMETRY_BLOCK_KEY).is_some_and(|block| is_object(&block));
    if !has_telemetry_block {
        // A scalar named telemetry is not a block (nor a serialized presence anchor in raw input).
        scalar_rest = with_nested_managed_key_deleted(&scalar_rest, TELEMETRY_BLOCK_KEY);
    }
    for setting in &STRUCTURED_MANAGED_SETTINGS {
        scalar_rest = with_nested_managed_key_deleted(&scalar_rest, setting.key);
    }

    let mut result = flatten_managed_settings(&scalar_rest);

    for setting in &STRUCTURED_MANAGED_SETTINGS {
        let encoded = (setting.encode)(&read_nested_managed_key(parsed, setting.key), on_warn);
        if !encoded.is_undefined()
            && let Some(text) = json_stringify(&encoded)
        {
            result.insert(setting.key.to_owned(), PolicyValue::String(text));
        }
    }

    if has_telemetry_block && !result.keys().any(|key| is_telemetry_setting_key(key)) {
        result.insert(TELEMETRY_BLOCK_KEY.to_owned(), PolicyValue::String("{}".to_owned()));
    }

    // Preserve future permission keys even when their values contain no scalar leaves.
    if let Some(permissions) = own_property(parsed, "permissions").filter(is_object) {
        for (key, value) in permissions.own_entries() {
            let flat_key = format!("permissions.{}", key.to_string_lossy());
            let nested_prefix = format!("{flat_key}.");
            if !value.is_undefined()
                && !result.keys().any(|key| *key == flat_key || key.starts_with(&nested_prefix))
                && let Some(encoded) = json_stringify(&value)
            {
                result.insert(flat_key, PolicyValue::String(encoded));
            }
        }
    }

    result
}

/// Normalize the schema's `{ [id]: { source, autoUpdate? } }` marketplace map into an
/// [`IExtraKnownMarketplaceEntry`] list, preserving the marketplace `name`,
/// source discriminator, and any `ref`. Malformed or off-spec entries are dropped
/// (with an optional warning via `on_warn`).
fn normalize_extra_known_marketplaces(value: &Dynamic, on_warn: OnWarn) -> Option<Vec<ExtraKnownMarketplace>> {
    if !is_object(value) {
        return None;
    }
    let warn = |message: &str| {
        if let Some(on_warn) = on_warn {
            on_warn(message);
        }
    };
    let mut seen: HashSet<String> = HashSet::new();
    let mut entries = Vec::new();
    for (name, entry) in value.own_entries() {
        let name = name.to_string_lossy();
        if !(is_object(&entry) && is_object(&entry.get_str("source"))) {
            warn(&format!("Skipping malformed extraKnownMarketplaces entry \"{name}\": expected {{ source: {{ source, repo|url }} }}"));
            continue;
        }
        let src = entry.get_str("source");
        let raw_auto_update = entry.get_str("autoUpdate");
        let auto_update = match raw_auto_update {
            Dynamic::Bool(flag) => Some(flag),
            _ => None,
        };
        if !raw_auto_update.is_undefined() && auto_update.is_none() {
            warn(&format!("Ignoring invalid autoUpdate for extraKnownMarketplaces entry \"{name}\": expected boolean"));
        }
        let kind = src.get_str("source");
        let text = |key: &str| match src.get_str(key) {
            Dynamic::String(text) => Some(text.to_string_lossy()),
            _ => None,
        };
        let git_ref = text("ref").filter(|git_ref| !git_ref.is_empty());
        let is_kind = |expected: &str| matches!(&kind, Dynamic::String(actual) if *actual == expected);
        let normalized = if is_kind("github")
            && let Some(repo) = text("repo")
        {
            Some(IExtraKnownMarketplaceEntry { name: name.clone(), auto_update, source: ExtraKnownMarketplaceSource::Github { repo, r#ref: git_ref } })
        } else if is_kind("git")
            && let Some(url) = text("url")
        {
            Some(IExtraKnownMarketplaceEntry { name: name.clone(), auto_update, source: ExtraKnownMarketplaceSource::Git { url, r#ref: git_ref } })
        } else if is_kind("github") || is_kind("git") {
            warn(&format!("Skipping extraKnownMarketplaces entry \"{name}\": source \"{}\" requires {}", to_js_string(&kind).to_string_lossy(), if is_kind("github") { "\"repo\"" } else { "\"url\"" }));
            None
        } else {
            warn(&format!("Skipping extraKnownMarketplaces entry \"{name}\": unknown source type \"{}\"", to_js_string(&kind).to_string_lossy()));
            None
        };
        if let Some(normalized) = normalized
            && seen.insert(name)
        {
            entries.push(ExtraKnownMarketplace::Entry(normalized));
        }
    }
    Some(entries)
}

service_id!(pub I_FILE_MANAGED_SETTINGS_SERVICE: dyn IFileManagedSettingsService = "fileManagedSettingsService");

/// `IFileManagedSettingsService`: as configurações gerenciadas de um arquivo (`managed-settings.json`).
pub trait IFileManagedSettingsService {
    fn raw_managed_settings(&self) -> RawManagedSettingsData;
    fn managed_settings(&self) -> ManagedSettingsData;
    fn on_did_change_raw_managed_settings(&self) -> Event<RawManagedSettingsData>;
    fn on_did_change_managed_settings(&self) -> Event<ManagedSettingsData>;
    fn initialize(&self) -> Promise<ManagedSettingsData>;
}

/// `NullFileManagedSettingsService`.
#[derive(Clone, Copy, Debug, Default)]
pub struct NullFileManagedSettingsService;

impl IFileManagedSettingsService for NullFileManagedSettingsService {
    fn raw_managed_settings(&self) -> RawManagedSettingsData {
        empty_raw_managed_settings()
    }

    fn managed_settings(&self) -> ManagedSettingsData {
        ManagedSettingsData::new()
    }

    fn on_did_change_raw_managed_settings(&self) -> Event<RawManagedSettingsData> {
        Event::none()
    }

    fn on_did_change_managed_settings(&self) -> Event<ManagedSettingsData> {
        Event::none()
    }

    fn initialize(&self) -> Promise<ManagedSettingsData> {
        Promise::resolved(ManagedSettingsData::new())
    }
}
