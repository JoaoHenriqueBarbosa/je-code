//! Porte de `src/vs/workbench/services/accounts/browser/managedSettings.ts`: a identidade do cliente enviada ao
//! serviço de configurações gerenciadas, a leitura do erro de compatibilidade e a adaptação da resposta de
//! `/copilot_internal/managed_settings` para a fatia `managedSettings` do `IPolicyData`.
//!
//! Decisões de tradução:
//! - **`IManagedSettingsResponse`** (uma interface com assinatura de índice, que aceita qualquer chave
//!   desconhecida) é o próprio [`Dynamic`] ([`IManagedSettingsResponse`]): a resposta é o JSON do servidor, e um
//!   tipo fechado perderia as chaves futuras que o original preserva.
//! - **`Pick<IProductConfiguration, 'version' | 'copilotVersions'>`** são dois parâmetros de
//!   [`append_managed_settings_client_identity`]: a versão e as `copilotVersions` opcionais.
//! - **`new URL(url)` e `URLSearchParams`** são a crate `url` pelo módulo WHATWG de `je_platform`
//!   ([`try_new_url`]); um texto que o `URL` rejeitaria volta inalterado, como o `try`/`catch` do original.
//!   `searchParams.set` troca o primeiro par de mesmo nome, remove os demais e acrescenta quando não há nenhum.
//! - **`Partial<IPolicyData>`** é um [`IPolicyData`] com os demais campos ausentes (`None`).
//! - **`IManagedSettingsCompatibilityError` e `MANAGED_SETTINGS_UPDATE_REQUIRED_ERROR_CODE`** vêm de
//!   `platform/defaultAccount/common/defaultAccount.ts`, que ainda não tem porte; ficam aqui e migram para lá
//!   quando ele for portado. O `errorCode` (o tipo literal da constante) é um `&'static str`.
//! - **`hasRawManagedSettings(managedSettings)`**: o original passa o `ManagedSettingsData` onde o tipo pede o
//!   documento bruto; aqui ele vira o objeto dinâmico equivalente ([`managed_settings_as_raw`]).

use je_base::JsString;
use je_base::default_account::IPolicyData;
use je_base::dynamic::Dynamic;
use je_base::policy::{ManagedSettingsData, PolicyValue};
use je_base::product::ICopilotVersions;
use je_platform::policy::copilot_managed_settings::{OnWarn, RawManagedSettingsData, has_raw_managed_settings, normalize_managed_settings};
use je_platform::url::whatwg::{href, try_new_url};

/// Client identity VS Code reports to the managed settings service. It names this codebase's own
/// managed settings implementation, which is distinct from the `copilot-runtime` implementation
/// VS Code relays a subset of these settings into.
const MANAGED_SETTINGS_CLIENT_ID: &str = "vscode";

/// `MANAGED_SETTINGS_UPDATE_REQUIRED_ERROR_CODE`: o código estável do erro de cliente desatualizado.
pub const MANAGED_SETTINGS_UPDATE_REQUIRED_ERROR_CODE: &str = "client_update_required";

/// `IManagedSettingsCompatibilityError`.
#[derive(Clone, Debug, PartialEq)]
pub struct IManagedSettingsCompatibilityError {
    pub error_code: &'static str,
    pub client_version: Option<JsString>,
    pub minimum_client_version: Option<JsString>,
}

/// A single MCP server matcher entry in the `allowedMcpServers` / `deniedMcpServers` managed
/// settings, identifying a server by exactly one strategy: name, remote URL pattern, or local
/// command invocation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum IManagedMcpServerMatcher {
    ServerName(String),
    ServerUrl(String),
    ServerCommand(Vec<String>),
}

/// Response shape from the Copilot `/copilot_internal/managed_settings` endpoint.
/// The endpoint returns `.github/copilot/settings.json` content from the
/// enterprise's source org. An empty response (`{}`) is success and means
/// "no policy file present".
///
/// Unknown keys are accepted so the client is forward-compatible with future additions to the registry
/// schema, which is why the response is a dynamic object. The known keys are `permissions`
/// (`disableBypassPermissionsMode`, `allow`, `ask`, `deny` and the legacy `model`), `model`,
/// `enabledPlugins`, `extraKnownMarketplaces`, `strictKnownMarketplaces`, `allowedMcpServers` and
/// `deniedMcpServers` (lists of [`IManagedMcpServerMatcher`]), `strictPluginOnlyCustomization`,
/// `allowManagedMcpServersOnly`, `allowManagedHooksOnly`, `forceRemoteSettingsRefresh` and `telemetry`.
///
/// Exported for unit-testing the [`adapt_managed_settings`] shape transformation.
pub type IManagedSettingsResponse = Dynamic;

/// Troca o valor do parâmetro `name` como `URLSearchParams#set`: substitui o primeiro par, remove os outros e
/// acrescenta ao fim quando não há nenhum.
fn set_search_param(pairs: &mut Vec<(String, String)>, name: &str, value: &str) {
    match pairs.iter().position(|(key, _)| key == name) {
        Some(first) => {
            pairs[first].1 = value.to_owned();
            let mut index = 0;
            pairs.retain(|(key, _)| {
                index += 1;
                index - 1 == first || key != name
            });
        }
        None => pairs.push((name.to_owned(), value.to_owned())),
    }
}

/// Append the client identity to a `managed_settings` request URL, naming the implementations that
/// parse and enforce the response so the service can fail closed when they are too old to honor a
/// setting it would otherwise deliver. `copilot_runtime_version` is present only when a runtime is
/// bundled.
///
/// Existing query parameters are preserved, and the URL is returned unchanged when it cannot be
/// parsed so a malformed `managedSettingsUrl` cannot turn into a thrown request. The identity
/// travels in the query string because neither header channel reaches the service; see the PR and
/// ADR for that rationale.
pub fn append_managed_settings_client_identity(url: &str, version: &str, copilot_versions: Option<&ICopilotVersions>) -> String {
    let Some(mut parsed) = try_new_url(&JsString::from(url)) else {
        return url.to_owned();
    };

    let mut pairs: Vec<(String, String)> = parsed.query_pairs().map(|(name, value)| (name.into_owned(), value.into_owned())).collect();
    set_search_param(&mut pairs, "client_id", MANAGED_SETTINGS_CLIENT_ID);
    set_search_param(&mut pairs, "client_version", version);
    match copilot_versions.map(|versions| versions.runtime.as_str()).filter(|runtime| !runtime.is_empty()) {
        Some(runtime_version) => set_search_param(&mut pairs, "copilot_runtime_version", runtime_version),
        // Never let a value configured on the endpoint URL stand in for a runtime we did not bundle.
        None => pairs.retain(|(name, _)| name != "copilot_runtime_version"),
    }
    parsed.query_pairs_mut().clear().extend_pairs(&pairs);
    href(&parsed).to_string_lossy()
}

/// `parseManagedSettingsCompatibilityError`: o erro estável de cliente desatualizado e as versões opcionais, ou
/// `None` para qualquer outra forma de resposta.
pub fn parse_managed_settings_compatibility_error(response: &Dynamic) -> Option<IManagedSettingsCompatibilityError> {
    // `isManagedSettingsCompatibilityErrorResponse`: `typeof response === 'object' && response !== null`.
    if response.type_of() != "object" || response.is_null() {
        return None;
    }
    if !matches!(response.get_str("error_code"), Dynamic::String(code) if code == MANAGED_SETTINGS_UPDATE_REQUIRED_ERROR_CODE) {
        return None;
    }

    // `isString(value) ? value : undefined`, e `...(version ? { version } : {})` descarta o texto vazio.
    let version = |key: &str| match response.get_str(key) {
        Dynamic::String(text) if !text.is_empty() => Some(text),
        _ => None,
    };
    Some(IManagedSettingsCompatibilityError {
        error_code: MANAGED_SETTINGS_UPDATE_REQUIRED_ERROR_CODE,
        client_version: version("client_version"),
        minimum_client_version: version("minimum_client_version"),
    })
}

/// O `ManagedSettingsData` como o objeto dinâmico (`RawManagedSettingsData`) que `hasRawManagedSettings` lê.
fn managed_settings_as_raw(managed_settings: &ManagedSettingsData) -> RawManagedSettingsData {
    Dynamic::object(managed_settings.iter().map(|(key, value)| {
        let value = match value {
            PolicyValue::Boolean(flag) => Dynamic::Bool(*flag),
            PolicyValue::Number(number) => Dynamic::Number(*number),
            PolicyValue::String(text) => Dynamic::string(text),
        };
        (key.as_str(), value)
    }))
}

/// Adapt the `managed_settings` API response into the `managedSettings` slice of
/// [`IPolicyData`] that the policy framework consumes. This is a thin wrapper
/// around [`normalize_managed_settings`], the single normalization path shared
/// by all delivery channels (server API, file-based, native MDM), so downstream
/// projection and policy `value()` callbacks behave identically regardless of source.
///
/// Exported for unit-testing the shape transformation independently of network I/O.
pub fn adapt_managed_settings(response: &IManagedSettingsResponse, on_warn: OnWarn) -> IPolicyData {
    let managed_settings = normalize_managed_settings(response, on_warn);
    let response_has_governance = has_raw_managed_settings(Some(response));
    let unrepresented_governance = !has_raw_managed_settings(Some(&managed_settings_as_raw(&managed_settings))) && response_has_governance;
    let managed_settings_active = if unrepresented_governance {
        Some(true)
    } else if !response_has_governance {
        Some(false)
    } else {
        None
    };
    IPolicyData {
        mcp: None,
        chat_preview_features_enabled: None,
        chat_agent_enabled: None,
        cloud_session_storage_enabled: None,
        mcp_registry_url: None,
        mcp_access: None,
        managed_settings: Some(managed_settings),
        managed_settings_active,
        extra: Default::default(),
    }
}
