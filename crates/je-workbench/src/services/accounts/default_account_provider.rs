//! Porte do resto de `src/vs/workbench/services/accounts/browser/defaultAccount.ts`: o `DefaultAccountProvider`
//! (a conta padrão do Copilot: sessões de autenticação, direitos, token, registro de MCP e as configurações
//! gerenciadas), a configuração `IDefaultAccountConfig`, o `DefaultAccountStatus` com a chave de contexto
//! `defaultAccountStatus`, a contribuição `DefaultAccountProviderContribution` e a ação de login. A classe
//! `DefaultAccountService` (e os contratos de `platform/defaultAccount/common`) estão em [`super::default_account`].
//!
//! Decisões de tradução:
//! - **Injeção.** O construtor decorado vira [`DefaultAccountProvider::new`] com a configuração e o
//!   [`DefaultAccountProviderServices`] (um campo por serviço decorado, na ordem do original).
//! - **Métodos privados que os testes chamam** (`provider['getManagedSettings']`, `setDefaultAccount`,
//!   `setPolicyData`, `setManagedSettingsFreshness`, `setManagedSettingsCompatibilityError`,
//!   `getDefaultAccountFromAuthenticatedSessions`, `findMatchingProviderSession`, `onManagedSettingsSourceChanged`,
//!   `_rateLimitBackoffUntil`, `initialized`, `_policyData`) são `pub` aqui, já que os testes ficam em outra caixa.
//! - **Retorno de `getManagedSettings`.** `data: Partial<IPolicyData>` só carrega duas chaves (`managedSettings`, que
//!   existe mesmo quando `undefined`, e `managedSettingsActive`, que só existe quando decidida), e o `{ ...policyData,
//!   ...data }` do original só as sobrescreve quando presentes. O tipo é [`IManagedSettingsPatch`], que guarda
//!   exatamente isso.
//! - **`ManagedSettingsRequestResult`**: as variantes `network`, `rateLimited`, `httpError` e `malformed` são
//!   [`ManagedSettingsRequestResult::Failed`] com o [`ManagedSettingsFreshnessFailure`] correspondente, que já é o
//!   que `toBlockedManagedSettingsFreshness` produz.
//! - **`null` e `undefined`** de `IDefaultAccountData | null`, `entitlementsData: IEntitlementsData | undefined | null`
//!   e afins são `Option`; o que o original distingue (`IMcpRegistryProvider | null | undefined`) é
//!   `Option<Option<_>>`.
//! - **Carimbos de tempo** (`Date.now()`) são `u64` em milissegundos, como o do `IManagedSettingsFreshness`; um
//!   carimbo zero é falso, como o `number` do JavaScript nas conferências `x && ...`.
//! - **Direitos (`IEntitlementsData`).** O tipo exige sete campos que o original só declara; uma resposta que os
//!   omite (`{ chat_enabled: true }`) os recebe como o valor vazio do tipo (texto vazio, falso, lista vazia) antes
//!   de virar [`IEntitlementsData`], porque o original não valida a resposta.
//! - **Cache em disco (`defaultAccount.cachedPolicyData`).** O `JSON.parse`/`JSON.stringify` do original usam o
//!   `serde_json`; um `managedSettingsCompatibilityError` guardado com código desconhecido é descartado (o código
//!   é o literal `client_update_required`).
//! - **Registro.** `registerAction2` é [`register_default_account_actions`]; o `run` da ação não espera o `signIn`
//!   (o [`Action2`] do porte não devolve promessa). `registerWorkbenchContribution2` não tem par (o registro de
//!   contribuições do workbench não foi portado): quem monta o workbench cria a [`DefaultAccountProviderContribution`]
//!   na fase `BlockStartup`.
//! - **Log.** Os argumentos extras que o original passa a `trace`/`debug` com a conta inteira (`Updating default
//!   account:`) não são registrados, porque carregam dados pessoais do usuário.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::{Rc, Weak};

use indexmap::IndexMap;
use je_base::arrays::distinct;
use je_base::async_::{RunOnceScheduler, ThrottledDelayer, async_fn, timeout};
use je_base::cancellation::CancellationToken;
use je_base::default_account::{ICopilotTokenInfo, IDefaultAccount, IDefaultAccountAuthenticationProvider, IEntitlementsData, IPolicyData, McpAccess};
use je_base::dynamic::{Dynamic, json_stringify};
use je_base::error_message::to_error_message_of;
use je_base::errors::Error;
use je_base::event::{Emitter, Event};
use je_base::json::{self, ParseOptions};
use je_base::lifecycle::{Disposable, DisposableRef, DisposableStore, IDisposable};
use je_base::parts::request::{IRequestContext, IRequestOptions, HeaderValue, IHeaders};
use je_base::platform::Platform;
use je_base::policy::ManagedSettingsData;
use je_base::product::IDefaultChatAgent;
use je_base::promise::Promise;
use je_base::runtime;
use je_base::uri::{Field, URI, UriChange};
use je_base::JsString;
use je_instantiation::ServicesAccessor;
use je_platform::action::{CommandAction, CommandActionTitle, Title, localize2};
use je_platform::actions::{Action2, Action2Options, register_action2};
use je_platform::commands::ICommandService;
use je_platform::configuration::service::IConfigurationService;
use je_platform::contextkey::{ContextKeyMeta, IContextKey, IContextKeyService, RawContextKey};
use je_platform::js::from_json;
use je_platform::log::{LogArg, LogMessage, LogServiceRef, Logger};
use je_platform::policy::copilot_managed_settings::{
    IFileManagedSettingsService, IForceRemoteSettingsRefreshResolution, INativeManagedSettingsService, ManagedSettingsChannel, resolve_force_remote_settings_refresh,
};
use je_platform::policy::managed_settings_freshness::{
    IManagedSettingsFreshness, IManagedSettingsFreshnessEffective, IManagedSettingsFreshnessScope, ManagedSettingsFreshnessFailure, ManagedSettingsFreshnessState,
    is_managed_settings_freshness_blocking, is_managed_settings_freshness_satisfied_for,
};
use je_platform::product::IProductService;
use je_platform::request::request::{IRequestService, as_json, as_text, is_client_error, is_success, read_header, retry_after_from_headers};
use je_platform::resources::ext_uri;
use je_platform::storage::{StorageScope, StorageService, StorageTarget};
use je_platform::telemetry::telemetry::ITelemetryService;
use je_platform::url::whatwg::{host, new_url, try_new_url};
use je_storage::StorageValue;

use super::default_account::{
    I_DEFAULT_ACCOUNT_SERVICE, IDefaultAccountProvider, IDefaultAccountRefreshOptions, IDefaultAccountService, ISignInOptions, ManagedSettingsFetchStatus,
};
use super::github_enterprise::get_github_enterprise_uri;
use super::managed_settings::{
    IManagedSettingsCompatibilityError, MANAGED_SETTINGS_UPDATE_REQUIRED_ERROR_CODE, adapt_managed_settings, append_managed_settings_client_identity,
    parse_managed_settings_compatibility_error,
};
use crate::services::authentication::authentication::{
    AuthenticationProviderInformation, AuthenticationScopeListOrRequest, AuthenticationSession, AuthenticationSessionAccount, AuthenticationSessionsChange,
    IAuthenticationCreateSessionOptions, IAuthenticationExtensionsService, IAuthenticationGetSessionsOptions, IAuthenticationProviderSessionOptions, IAuthenticationService,
    AccountPreferenceChange,
};
use crate::services::environment::environment_service::IWorkbenchEnvironmentService;
use crate::services::extensions::extensions::IExtensionService;
use crate::services::host::host::IHostService;
use crate::services::policies::policy_value_callback::empty_policy_data;

/// `IDefaultAccountConfig.authenticationProvider.default` e `.enterprise`: o identificador e o nome de um provedor.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IDefaultAccountConfigProvider {
    pub id: String,
    pub name: String,
}

/// `IDefaultAccountConfig.authenticationProvider`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IDefaultAccountConfigAuthenticationProvider {
    pub default: IDefaultAccountConfigProvider,
    pub enterprise: IDefaultAccountConfigProvider,
    pub enterprise_provider_config: String,
    pub scopes: Vec<Vec<String>>,
}

/// `IDefaultAccountConfig`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IDefaultAccountConfig {
    pub preferred_extensions: Vec<String>,
    pub authentication_provider: IDefaultAccountConfigAuthenticationProvider,
    pub token_entitlement_url: String,
    pub entitlement_url: String,
    pub mcp_registry_data_url: String,
    pub managed_settings_url: String,
}

/// `toDefaultAccountConfig(defaultChatAgent)`.
pub fn to_default_account_config(default_chat_agent: &IDefaultChatAgent) -> IDefaultAccountConfig {
    IDefaultAccountConfig {
        preferred_extensions: vec![default_chat_agent.chat_extension_id.clone(), default_chat_agent.extension_id.clone()],
        authentication_provider: IDefaultAccountConfigAuthenticationProvider {
            default: IDefaultAccountConfigProvider { id: default_chat_agent.provider.default.id.clone(), name: default_chat_agent.provider.default.name.clone() },
            enterprise: IDefaultAccountConfigProvider { id: default_chat_agent.provider.enterprise.id.clone(), name: default_chat_agent.provider.enterprise.name.clone() },
            enterprise_provider_config: format!("{}.authProvider", default_chat_agent.completions_advanced_setting),
            scopes: default_chat_agent.provider_scopes.clone(),
        },
        entitlement_url: default_chat_agent.entitlement_url.clone(),
        token_entitlement_url: default_chat_agent.token_entitlement_url.clone(),
        mcp_registry_data_url: default_chat_agent.mcp_registry_data_url.clone(),
        managed_settings_url: default_chat_agent.managed_settings_url.clone(),
    }
}

/// `DEFAULT_ACCOUNT_SIGN_IN_COMMAND`.
pub const DEFAULT_ACCOUNT_SIGN_IN_COMMAND: &str = "workbench.actions.accounts.signIn";

/// `DefaultAccountStatus`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DefaultAccountStatus {
    Uninitialized,
    Unavailable,
    Available,
}

impl DefaultAccountStatus {
    /// O texto do original (`'uninitialized' | 'unavailable' | 'available'`).
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Uninitialized => "uninitialized",
            Self::Unavailable => "unavailable",
            Self::Available => "available",
        }
    }
}

/// `CONTEXT_DEFAULT_ACCOUNT_STATE`: a chave `defaultAccountStatus`.
pub fn context_default_account_state() -> RawContextKey {
    RawContextKey::new("defaultAccountStatus", Dynamic::string(DefaultAccountStatus::Uninitialized.as_str()), ContextKeyMeta::Hide)
}

const CACHED_POLICY_DATA_KEY: &str = "defaultAccount.cachedPolicyData";
/// `ACCOUNT_DATA_POLL_INTERVAL_MS`: 1 hour.
const ACCOUNT_DATA_POLL_INTERVAL_MS: u64 = 60 * 60 * 1000;
const MANAGED_SETTINGS_REQUEST_TIMEOUT_MS: u64 = 5000;

/// `IMcpRegistryProvider` (sem o `owner`, que nenhum consumidor lê). `registry_access` ausente é um valor que o
/// porte não conhece (`allow_all` e `registry_only` são os dois do original).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IMcpRegistryProvider {
    pub url: String,
    pub registry_access: Option<McpAccess>,
}

/// `Partial<IPolicyData>` de `adaptManagedSettings` (ver o topo do arquivo).
#[derive(Clone, Debug, PartialEq)]
pub struct IManagedSettingsPatch {
    /// A chave `managedSettings`, sempre presente: `None` é o `undefined` explícito.
    pub managed_settings: Option<ManagedSettingsData>,
    /// A chave `managedSettingsActive`, presente só quando decidida.
    pub managed_settings_active: Option<bool>,
}

impl IManagedSettingsPatch {
    /// `{ managedSettings }`.
    pub fn of(managed_settings: Option<ManagedSettingsData>) -> Self {
        Self { managed_settings, managed_settings_active: None }
    }

    /// O `IPolicyData` equivalente (`first.data ?? {}` dos testes).
    pub fn into_policy_data(self) -> IPolicyData {
        let mut policy_data = empty_policy_data();
        policy_data.managed_settings = self.managed_settings;
        policy_data.managed_settings_active = self.managed_settings_active;
        policy_data
    }
}

/// `IAccountPolicyData`.
#[derive(Clone, Debug, PartialEq)]
pub struct IAccountPolicyData {
    pub account_id: String,
    pub enterprise_uri: Option<String>,
    pub policy_data: IPolicyData,
    pub entitlements_fetched_at: Option<u64>,
    pub token_entitlements_fetched_at: Option<u64>,
    pub mcp_registry_data_fetched_at: Option<u64>,
    pub managed_settings_fetched_at: Option<u64>,
    pub managed_settings_scope: Option<IManagedSettingsFreshnessScope>,
    pub managed_settings_compatibility_error: Option<IManagedSettingsCompatibilityError>,
}

impl IAccountPolicyData {
    /// Uma `IAccountPolicyData` só com o identificador da conta e os dados de política.
    pub fn new(account_id: impl Into<String>, policy_data: IPolicyData) -> Self {
        Self {
            account_id: account_id.into(),
            enterprise_uri: None,
            policy_data,
            entitlements_fetched_at: None,
            token_entitlements_fetched_at: None,
            mcp_registry_data_fetched_at: None,
            managed_settings_fetched_at: None,
            managed_settings_scope: None,
            managed_settings_compatibility_error: None,
        }
    }

    /// O objeto que o `JSON.stringify` do original escreve (as chaves `undefined` ficam de fora).
    fn to_json(&self) -> serde_json::Value {
        let mut object = serde_json::Map::new();
        object.insert("accountId".to_owned(), self.account_id.clone().into());
        if let Some(enterprise_uri) = &self.enterprise_uri {
            object.insert("enterpriseUri".to_owned(), enterprise_uri.clone().into());
        }
        object.insert("policyData".to_owned(), serde_json::to_value(&self.policy_data).unwrap_or(serde_json::Value::Null));
        for (key, time) in [
            ("entitlementsFetchedAt", self.entitlements_fetched_at),
            ("tokenEntitlementsFetchedAt", self.token_entitlements_fetched_at),
            ("mcpRegistryDataFetchedAt", self.mcp_registry_data_fetched_at),
            ("managedSettingsFetchedAt", self.managed_settings_fetched_at),
        ] {
            if let Some(time) = time {
                object.insert(key.to_owned(), time.into());
            }
        }
        if let Some(scope) = &self.managed_settings_scope {
            let mut scope_object = serde_json::Map::new();
            scope_object.insert("accountId".to_owned(), scope.account_id.clone().into());
            scope_object.insert("authenticationProviderId".to_owned(), scope.authentication_provider_id.clone().into());
            scope_object.insert("endpointOrigin".to_owned(), scope.endpoint_origin.clone().into());
            object.insert("managedSettingsScope".to_owned(), serde_json::Value::Object(scope_object));
        }
        if let Some(error) = &self.managed_settings_compatibility_error {
            let mut error_object = serde_json::Map::new();
            error_object.insert("errorCode".to_owned(), error.error_code.into());
            if let Some(client_version) = &error.client_version {
                error_object.insert("clientVersion".to_owned(), client_version.to_string_lossy().into());
            }
            if let Some(minimum_client_version) = &error.minimum_client_version {
                error_object.insert("minimumClientVersion".to_owned(), minimum_client_version.to_string_lossy().into());
            }
            object.insert("managedSettingsCompatibilityError".to_owned(), serde_json::Value::Object(error_object));
        }
        serde_json::Value::Object(object)
    }

    /// O objeto lido do cache: `None` quando falta o `accountId` (não vazio) ou o `policyData`.
    fn from_json(value: &serde_json::Value) -> Option<Self> {
        let object = value.as_object()?;
        let account_id = object.get("accountId")?.as_str().filter(|id| !id.is_empty())?.to_owned();
        let policy_data = serde_json::from_value::<IPolicyData>(object.get("policyData")?.clone()).ok()?;
        let time = |key: &str| object.get(key).and_then(serde_json::Value::as_f64).map(|time| time as u64);
        let text = |object: &serde_json::Map<String, serde_json::Value>, key: &str| object.get(key).and_then(serde_json::Value::as_str).map(str::to_owned);
        let managed_settings_scope = object.get("managedSettingsScope").and_then(serde_json::Value::as_object).and_then(|scope| {
            Some(IManagedSettingsFreshnessScope {
                account_id: text(scope, "accountId")?,
                authentication_provider_id: text(scope, "authenticationProviderId")?,
                endpoint_origin: text(scope, "endpointOrigin")?,
            })
        });
        let managed_settings_compatibility_error = object.get("managedSettingsCompatibilityError").and_then(serde_json::Value::as_object).and_then(|error| {
            (text(error, "errorCode").as_deref() == Some(MANAGED_SETTINGS_UPDATE_REQUIRED_ERROR_CODE)).then(|| IManagedSettingsCompatibilityError {
                error_code: MANAGED_SETTINGS_UPDATE_REQUIRED_ERROR_CODE,
                client_version: text(error, "clientVersion").map(JsString::from),
                minimum_client_version: text(error, "minimumClientVersion").map(JsString::from),
            })
        });
        Some(Self {
            account_id,
            enterprise_uri: text(object, "enterpriseUri"),
            policy_data,
            entitlements_fetched_at: time("entitlementsFetchedAt"),
            token_entitlements_fetched_at: time("tokenEntitlementsFetchedAt"),
            mcp_registry_data_fetched_at: time("mcpRegistryDataFetchedAt"),
            managed_settings_fetched_at: time("managedSettingsFetchedAt"),
            managed_settings_scope,
            managed_settings_compatibility_error,
        })
    }
}

/// `ICachedAccountData`.
#[derive(Clone, Debug, PartialEq)]
struct ICachedAccountData {
    account_policy_data: IAccountPolicyData,
    copilot_token_info: Option<ICopilotTokenInfo>,
}

impl ICachedAccountData {
    fn to_json(&self) -> serde_json::Value {
        let mut object = serde_json::Map::new();
        object.insert("accountPolicyData".to_owned(), self.account_policy_data.to_json());
        if let Some(copilot_token_info) = &self.copilot_token_info {
            object.insert("copilotTokenInfo".to_owned(), serde_json::to_value(copilot_token_info).unwrap_or(serde_json::Value::Null));
        }
        serde_json::Value::Object(object)
    }
}

/// `IDefaultAccountData`.
#[derive(Clone, Debug, PartialEq)]
pub struct IDefaultAccountData {
    pub account_id: String,
    pub default_account: IDefaultAccount,
    pub enterprise_uri: Option<URI>,
    pub policy_data: Option<IAccountPolicyData>,
    pub copilot_token_info: Option<ICopilotTokenInfo>,
}

/// `IAccountSessionContext`.
#[derive(Clone, Debug)]
pub struct IAccountSessionContext {
    pub authentication_provider: IDefaultAccountAuthenticationProvider,
    pub enterprise_uri: Option<URI>,
}

/// `ManagedSettingsRequestResult`: ver o topo do arquivo para as variantes de falha.
#[derive(Clone, Debug, PartialEq)]
pub enum ManagedSettingsRequestResult {
    Success { data: IManagedSettingsPatch },
    NoSettings,
    UpdateRequired { error: IManagedSettingsCompatibilityError },
    Failed(ManagedSettingsFreshnessFailure),
}

/// `IManagedSettingsSources`.
#[derive(Clone, Debug, Default)]
pub struct IManagedSettingsSources {
    pub native_mdm: ManagedSettingsData,
    pub file: ManagedSettingsData,
}

/// `IAuthenticatedRequestOptions`.
#[derive(Clone, Debug, Default)]
pub struct IAuthenticatedRequestOptions {
    pub request_timeout_ms: Option<u64>,
    pub retry_not_found: Option<bool>,
    pub session_context: Option<IAccountSessionContext>,
}

/// O retorno de `getManagedSettings`.
#[derive(Clone, Debug, PartialEq)]
pub struct IManagedSettingsResult {
    pub data: Option<IManagedSettingsPatch>,
    pub fetched_at: Option<u64>,
    pub scope: Option<IManagedSettingsFreshnessScope>,
    pub compatibility_error: Option<IManagedSettingsCompatibilityError>,
}

/// Os parâmetros opcionais de `getManagedSettings` depois de `options` (`authenticationProvider`,
/// `managedSettingsSources`, `refreshRequirement` e `sessionContext`), que o original completa quando ausentes.
#[derive(Clone, Debug, Default)]
pub struct IManagedSettingsFetchContext {
    pub authentication_provider: Option<IDefaultAccountAuthenticationProvider>,
    pub managed_settings_sources: Option<IManagedSettingsSources>,
    pub refresh_requirement: Option<IForceRemoteSettingsRefreshResolution>,
    pub session_context: Option<IAccountSessionContext>,
}

/// `{ policyData: Partial<IPolicyData>; copilotTokenInfo: ICopilotTokenInfo }` de `getTokenEntitlements`.
#[derive(Clone, Debug, PartialEq)]
struct ITokenEntitlementsData {
    policy_data: IPolicyData,
    copilot_token_info: ICopilotTokenInfo,
}

#[derive(Clone, Debug, PartialEq)]
struct ITokenEntitlementsResult {
    data: Option<ITokenEntitlementsData>,
    fetched_at: u64,
}

#[derive(Clone, Debug, PartialEq)]
struct IEntitlementsResult {
    data: Option<IEntitlementsData>,
    fetched_at: Option<u64>,
}

#[derive(Clone, Debug, PartialEq)]
struct IMcpRegistryResult {
    data: Option<IMcpRegistryProvider>,
    fetched_at: u64,
}

/// O método HTTP de `request` (`'GET' | 'POST'`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RequestMethod {
    Get,
    Post,
}

impl RequestMethod {
    fn as_str(self) -> &'static str {
        match self {
            Self::Get => "GET",
            Self::Post => "POST",
        }
    }
}

/// Os serviços que o construtor do `DefaultAccountProvider` recebe por injeção, na ordem do original.
pub struct DefaultAccountProviderServices {
    pub configuration_service: Rc<dyn IConfigurationService>,
    pub authentication_service: Rc<dyn IAuthenticationService>,
    pub authentication_extensions_service: Rc<dyn IAuthenticationExtensionsService>,
    pub telemetry_service: Rc<dyn ITelemetryService>,
    pub extension_service: Rc<dyn IExtensionService>,
    pub request_service: Rc<dyn IRequestService>,
    pub log_service: LogServiceRef,
    pub environment_service: Rc<dyn IWorkbenchEnvironmentService>,
    pub product_service: Rc<dyn IProductService>,
    pub context_key_service: Rc<dyn IContextKeyService>,
    pub storage_service: Rc<dyn StorageService>,
    pub host_service: Rc<dyn IHostService>,
    pub command_service: Rc<dyn ICommandService>,
    pub native_managed_settings_service: Rc<dyn INativeManagedSettingsService>,
    pub file_managed_settings_service: Rc<dyn IFileManagedSettingsService>,
}

/// `JSON.parse(text)` como valor dinâmico: lança (`Err`) num texto que não é JSON, como o original.
fn parse_json_text(text: &str) -> Result<Dynamic, Error> {
    let units: Vec<u16> = text.encode_utf16().collect();
    let mut parse_errors = Vec::new();
    let options = ParseOptions { disallow_comments: true, allow_trailing_comma: false, allow_empty_content: false };
    match json::parse(&units, &mut parse_errors, options) {
        Some(value) if parse_errors.is_empty() => Ok(from_json(&value)),
        _ => Err(Error::new(format!("Unexpected token in JSON:\n{text}"))),
    }
}

/// `getErrorMessage(error)`.
fn error_message(error: &Error) -> String {
    to_error_message_of(error, false)
}

/// Um carimbo de tempo é falso quando é zero (a conferência `x && ...` do original).
fn present(time: Option<u64>) -> Option<u64> {
    time.filter(|time| *time != 0)
}

/// `Date.now()`.
fn now() -> u64 {
    runtime::date_now()
}

/// O `source` de um [`IManagedSettingsFreshness`] que tem um (todos menos `NotRequired`).
fn freshness_source(freshness: &IManagedSettingsFreshness) -> Option<ManagedSettingsChannel> {
    match freshness {
        IManagedSettingsFreshness::NotRequired => None,
        IManagedSettingsFreshness::Pending(effective) | IManagedSettingsFreshness::Blocked { effective, .. } => Some(effective.source),
        IManagedSettingsFreshness::Satisfied { source, .. } => Some(*source),
    }
}

/// O `ReturnType<typeof resolveForceRemoteSettingsRefresh>`: `effective` e `source`.
fn requirement_channel(requirement: IForceRemoteSettingsRefreshResolution) -> Option<ManagedSettingsChannel> {
    match requirement {
        IForceRemoteSettingsRefreshResolution::Effective { source } => Some(source),
        IForceRemoteSettingsRefreshResolution::NotEffective => None,
    }
}

/// Um `Blocked` sem escopo nem tentativa (a falha ocorreu antes de qualquer requisição).
fn blocked_without_attempt(source: ManagedSettingsChannel, failure: ManagedSettingsFreshnessFailure) -> IManagedSettingsFreshness {
    IManagedSettingsFreshness::Blocked { effective: IManagedSettingsFreshnessEffective { source, scope: None, last_attempt_at: None }, failure }
}

/// `toBlockedManagedSettingsFreshness`: o `Blocked` de uma requisição que falhou.
fn to_blocked_managed_settings_freshness(
    source: ManagedSettingsChannel,
    failure: ManagedSettingsFreshnessFailure,
    last_attempt_at: u64,
    scope: Option<IManagedSettingsFreshnessScope>,
) -> IManagedSettingsFreshness {
    IManagedSettingsFreshness::Blocked { effective: IManagedSettingsFreshnessEffective { source, scope, last_attempt_at: Some(last_attempt_at) }, failure }
}

/// `getManagedSettingsScopeKey`.
fn managed_settings_scope_key(scope: &IManagedSettingsFreshnessScope) -> String {
    format!("{}\n{}\n{}", scope.authentication_provider_id, scope.account_id, scope.endpoint_origin)
}

/// `createManagedSettingsFreshnessScope`.
fn create_managed_settings_freshness_scope(account_id: &str, authentication_provider_id: &str, managed_settings_url: &str) -> IManagedSettingsFreshnessScope {
    // Preserve a stable scope for a malformed product endpoint; the request will report the failure.
    let endpoint_origin = try_new_url(&JsString::from(managed_settings_url)).map_or_else(|| managed_settings_url.to_owned(), |url| url.origin().ascii_serialization());
    IManagedSettingsFreshnessScope { account_id: account_id.to_owned(), authentication_provider_id: authentication_provider_id.to_owned(), endpoint_origin }
}

/// `isRateLimited`. Detects a rate-limited GitHub response. Mirrors the public-API check in
/// `githubRepoFetcher.ts`:
/// - Canonical `429 Too Many Requests`.
/// - Primary quota exhaustion: `403` with `X-RateLimit-Remaining: 0`.
/// - Secondary throttling: GitHub omits `X-RateLimit-Remaining` but sets `Retry-After` (on a non-2xx response). We
///   treat any non-success status that carries `Retry-After` as a back-off signal.
fn is_rate_limited(response: &IRequestContext) -> bool {
    let status = response.res.status_code;
    if status == Some(429) {
        return true;
    }
    if status == Some(403) && read_header(Some(&response.res.headers), "x-ratelimit-remaining").as_deref() == Some("0") {
        return true;
    }
    // Secondary rate limit: the server explicitly asks the client to wait, regardless of which non-2xx code it
    // returned with.
    !is_success(response) && read_header(Some(&response.res.headers), "retry-after").is_some()
}

/// `scopesMatch`.
fn scopes_match(scopes: &[String], expected_scopes: &[String]) -> bool {
    expected_scopes.iter().all(|scope| scopes.contains(scope))
}

/// `entitlementsData` de uma resposta: o tipo exige sete campos que a resposta pode omitir (ver o topo do arquivo).
fn entitlements_from_dynamic(data: &Dynamic) -> Option<IEntitlementsData> {
    let mut value: serde_json::Value = serde_json::from_str(&json_stringify(data)?).ok()?;
    if let serde_json::Value::Object(object) = &mut value {
        for (key, default) in [
            ("access_type_sku", serde_json::Value::from("")),
            ("chat_enabled", serde_json::Value::from(false)),
            ("assigned_date", serde_json::Value::from("")),
            ("can_signup_for_limited", serde_json::Value::from(false)),
            ("copilot_plan", serde_json::Value::from("")),
            ("organization_login_list", serde_json::Value::Array(Vec::new())),
            ("analytics_tracking_id", serde_json::Value::from("")),
        ] {
            object.entry(key).or_insert(default);
        }
    }
    serde_json::from_value(value).ok()
}

/// `DefaultAccountProvider`.
pub struct DefaultAccountProvider {
    me: Weak<DefaultAccountProvider>,
    base: Rc<Disposable>,
    default_account_config: IDefaultAccountConfig,
    configuration_service: Rc<dyn IConfigurationService>,
    authentication_service: Rc<dyn IAuthenticationService>,
    authentication_extensions_service: Rc<dyn IAuthenticationExtensionsService>,
    telemetry_service: Rc<dyn ITelemetryService>,
    extension_service: Rc<dyn IExtensionService>,
    request_service: Rc<dyn IRequestService>,
    log_service: LogServiceRef,
    environment_service: Rc<dyn IWorkbenchEnvironmentService>,
    product_service: Rc<dyn IProductService>,
    storage_service: Rc<dyn StorageService>,
    host_service: Rc<dyn IHostService>,
    command_service: Rc<dyn ICommandService>,
    native_managed_settings_service: Rc<dyn INativeManagedSettingsService>,
    file_managed_settings_service: Rc<dyn IFileManagedSettingsService>,

    default_account: RefCell<Option<IDefaultAccountData>>,
    policy_data: RefCell<Option<IAccountPolicyData>>,
    copilot_token_info: RefCell<Option<ICopilotTokenInfo>>,
    managed_settings_fetch_status: Cell<Option<ManagedSettingsFetchStatus>>,
    managed_settings_raw_response: RefCell<Dynamic>,
    managed_settings_compatibility_error: RefCell<Option<IManagedSettingsCompatibilityError>>,
    managed_settings_freshness: RefCell<IManagedSettingsFreshness>,

    on_did_change_default_account: Rc<Emitter<Option<IDefaultAccount>>>,
    on_did_change_policy_data: Rc<Emitter<Option<IPolicyData>>>,
    on_did_change_copilot_token_info: Rc<Emitter<Option<ICopilotTokenInfo>>>,
    on_did_change_managed_settings_compatibility_error: Rc<Emitter<Option<IManagedSettingsCompatibilityError>>>,
    on_did_change_managed_settings_freshness: Rc<Emitter<IManagedSettingsFreshness>>,

    account_status_context: Rc<dyn IContextKey>,
    initialized: Cell<bool>,
    init_promise: RefCell<Option<Promise<()>>>,
    update_throttler: Rc<ThrottledDelayer<()>>,
    account_data_poll_scheduler: Rc<RunOnceScheduler>,
    failed_managed_settings_freshness: RefCell<HashMap<String, IManagedSettingsFreshness>>,
    rate_limit_backoff_until: Cell<u64>,
}

impl DefaultAccountProvider {
    /// `new DefaultAccountProvider(config, ...services)`.
    pub fn new(default_account_config: IDefaultAccountConfig, services: DefaultAccountProviderServices) -> Rc<Self> {
        let base = Disposable::new();
        let on_did_change_default_account = Emitter::new();
        let on_did_change_policy_data = Emitter::new();
        let on_did_change_copilot_token_info = Emitter::new();
        let on_did_change_managed_settings_compatibility_error = Emitter::new();
        let on_did_change_managed_settings_freshness = Emitter::new();
        base.register(on_did_change_default_account.clone());
        base.register(on_did_change_policy_data.clone());
        base.register(on_did_change_copilot_token_info.clone());
        base.register(on_did_change_managed_settings_compatibility_error.clone());
        base.register(on_did_change_managed_settings_freshness.clone());
        let update_throttler = Rc::new(ThrottledDelayer::<()>::new(100));
        base.register(update_throttler.clone());
        let account_status_context = context_default_account_state().bind_to(&*services.context_key_service);

        let provider = Rc::new_cyclic(|me: &Weak<DefaultAccountProvider>| {
            let poll_target = me.clone();
            let account_data_poll_scheduler = RunOnceScheduler::new(
                move || {
                    if let Some(provider) = poll_target.upgrade() {
                        let _ = provider.refetch_default_account();
                    }
                },
                ACCOUNT_DATA_POLL_INTERVAL_MS,
            );
            Self {
                me: me.clone(),
                base: base.clone(),
                default_account_config,
                configuration_service: services.configuration_service,
                authentication_service: services.authentication_service,
                authentication_extensions_service: services.authentication_extensions_service,
                telemetry_service: services.telemetry_service,
                extension_service: services.extension_service,
                request_service: services.request_service,
                log_service: services.log_service,
                environment_service: services.environment_service,
                product_service: services.product_service,
                storage_service: services.storage_service,
                host_service: services.host_service,
                command_service: services.command_service,
                native_managed_settings_service: services.native_managed_settings_service,
                file_managed_settings_service: services.file_managed_settings_service,
                default_account: RefCell::new(None),
                policy_data: RefCell::new(None),
                copilot_token_info: RefCell::new(None),
                managed_settings_fetch_status: Cell::new(None),
                managed_settings_raw_response: RefCell::new(Dynamic::Null),
                managed_settings_compatibility_error: RefCell::new(None),
                managed_settings_freshness: RefCell::new(IManagedSettingsFreshness::NotRequired),
                on_did_change_default_account,
                on_did_change_policy_data,
                on_did_change_copilot_token_info,
                on_did_change_managed_settings_compatibility_error,
                on_did_change_managed_settings_freshness,
                account_status_context,
                initialized: Cell::new(false),
                init_promise: RefCell::new(None),
                update_throttler,
                account_data_poll_scheduler,
                failed_managed_settings_freshness: RefCell::new(HashMap::new()),
                rate_limit_backoff_until: Cell::new(0),
            }
        });
        provider.base.register(provider.account_data_poll_scheduler.clone());

        let cached_account_data = provider.get_cached_account_data();
        *provider.policy_data.borrow_mut() = cached_account_data.as_ref().map(|cached| cached.account_policy_data.clone());
        *provider.copilot_token_info.borrow_mut() = cached_account_data.as_ref().and_then(|cached| cached.copilot_token_info.clone());
        *provider.managed_settings_compatibility_error.borrow_mut() = cached_account_data.and_then(|cached| cached.account_policy_data.managed_settings_compatibility_error);
        provider.update_managed_settings_freshness_requirement(
            &provider.native_managed_settings_service.managed_settings(),
            provider.get_cached_server_managed_settings(&provider.get_default_account_authentication_provider()).as_ref(),
            &provider.file_managed_settings_service.managed_settings(),
        );
        provider.listen(provider.native_managed_settings_service.on_did_change_managed_settings(), |provider, _| provider.on_managed_settings_source_changed());
        provider.listen(provider.file_managed_settings_service.on_did_change_managed_settings(), |provider, _| provider.on_managed_settings_source_changed());
        let weak = Rc::downgrade(&provider);
        let init_promise = provider.init().finally(move || {
            if let Some(provider) = weak.upgrade() {
                provider.publish_account_status(provider.default_account().is_some(), true);
                provider.initialized.set(true);
            }
        });
        *provider.init_promise.borrow_mut() = Some(init_promise);
        provider
    }

    fn rc(&self) -> Rc<Self> {
        self.me.upgrade().expect("o provedor está vivo enquanto é usado")
    }

    /// `this._register(event(handler))`, com o provedor como `Weak` no ouvinte.
    fn listen<T: 'static>(&self, event: Event<T>, handler: impl Fn(&Rc<Self>, &T) + 'static) {
        let weak = self.me.clone();
        self.base.register(event.listen(move |value| {
            if let Some(provider) = weak.upgrade() {
                handler(&provider, value);
            }
        }));
    }

    fn current_session_id(&self) -> Option<String> {
        self.default_account.borrow().as_ref().map(|data| data.default_account.session_id.clone())
    }

    fn publish_account_status(&self, available: bool, initial: bool) {
        self.telemetry_service.public_log2(
            "defaultaccount:status",
            Some(Dynamic::object([
                ("status", Dynamic::string(if available { "available" } else { "unavailable" })),
                ("initial", Dynamic::Bool(initial)),
            ])),
        );
    }

    fn get_cached_account_data(&self) -> Option<ICachedAccountData> {
        let cached = self.storage_service.get(CACHED_POLICY_DATA_KEY, StorageScope::Application, None).filter(|cached| !cached.is_empty())?;
        let parsed: serde_json::Value = match serde_json::from_str(&cached) {
            Ok(parsed) => parsed,
            Err(error) => {
                self.log_service.error_args(&LogMessage::from("[DefaultAccount] Failed to parse cached policy data"), &[LogArg::from(error.to_string().as_str())]);
                return None;
            }
        };

        // TODO: Remove old format migration after August 2026.
        // Previously, the cache stored a flat IAccountPolicyData shape (e.g. { accountId, policyData, ... }). We now
        // wrap it inside ICachedAccountData ({ accountPolicyData, copilotTokenInfo }). This branch migrates the old
        // flat format to the new shape and re-stores it so subsequent reads use the new format directly.
        if let Some(legacy) = IAccountPolicyData::from_json(&parsed) {
            self.log_service.debug("[DefaultAccount] Initializing with cached policy data (migrating old format)");
            let result = ICachedAccountData {
                account_policy_data: IAccountPolicyData {
                    token_entitlements_fetched_at: legacy.token_entitlements_fetched_at,
                    mcp_registry_data_fetched_at: legacy.mcp_registry_data_fetched_at,
                    ..IAccountPolicyData::new(legacy.account_id, legacy.policy_data)
                },
                copilot_token_info: parsed.get("copilotTokenInfo").and_then(|info| serde_json::from_value(info.clone()).ok()),
            };
            self.storage_service.store(CACHED_POLICY_DATA_KEY, StorageValue::String(result.to_json().to_string()), StorageScope::Application, StorageTarget::Machine);
            return Some(result);
        }

        // New format
        let account_policy_data = IAccountPolicyData::from_json(parsed.get("accountPolicyData")?)?;
        self.log_service.debug("[DefaultAccount] Initializing with cached policy data");
        Some(ICachedAccountData { account_policy_data, copilot_token_info: parsed.get("copilotTokenInfo").and_then(|info| serde_json::from_value(info.clone()).ok()) })
    }

    fn init(&self) -> Promise<()> {
        let this = self.rc();
        async_fn(async move {
            // Skip initialization for classic web-no-remote (vscode.dev editor), but still initialize for the agents
            // web workbench (vscode.dev/agents) where account state drives the title bar and the welcome walkthrough.
            if Platform::detect().is_web && this.environment_service.remote_authority().is_none_or(|authority| authority.is_empty()) && !this.environment_service.is_sessions_window() {
                this.log_service.debug("[DefaultAccount] Running in web without remote, skipping initialization");
                return Ok(());
            }

            // Wait until the default account authentication provider is available instead of waiting for all
            // installed extensions to be registered. In desktop remote connections extensions are only registered
            // after the connection is established, so waiting for `whenInstalledExtensionsRegistered` can deadlock
            // initialization.
            this.when_default_account_authentication_provider_available().await?;

            this.log_service.debug("[DefaultAccount] Starting initialization");
            this.do_update_default_account(None).await?;
            this.log_service.debug("[DefaultAccount] Initialization complete");

            this.listen(this.on_did_change_default_account.event(), |provider, account| provider.publish_account_status(account.is_some(), false));

            this.listen(this.authentication_service.on_did_change_sessions(), |provider, change| provider.on_did_change_sessions(change));

            this.listen(this.authentication_extensions_service.on_did_change_account_preference(), |provider, change: &AccountPreferenceChange| {
                let default_account_provider = provider.get_default_account_authentication_provider();
                if change.provider_id != default_account_provider.id {
                    return;
                }
                provider.log_service.debug("[DefaultAccount] Account preference changed for default account provider, updating default account");
                let _ = provider.update_default_account(None);
            });

            this.listen(this.authentication_service.on_did_register_authentication_provider(), |provider, registered: &AuthenticationProviderInformation| {
                let default_account_provider = provider.get_default_account_authentication_provider();
                if registered.id != default_account_provider.id {
                    return;
                }
                provider.log_service.debug("[DefaultAccount] Default account provider registered, updating default account");
                let _ = provider.update_default_account(None);
            });

            this.listen(this.authentication_service.on_did_unregister_authentication_provider(), |provider, unregistered: &AuthenticationProviderInformation| {
                let default_account_provider = provider.get_default_account_authentication_provider();
                if unregistered.id != default_account_provider.id {
                    return;
                }
                provider.log_service.debug("[DefaultAccount] Default account provider unregistered, updating default account");
                let _ = provider.update_default_account(None);
            });

            this.listen(this.host_service.on_did_change_focus(), |provider, focused| {
                if *focused {
                    let _ = provider.refetch_default_account();
                }
            });
            Ok(())
        })
    }

    /// O ouvinte de `authenticationService.onDidChangeSessions`.
    fn on_did_change_sessions(&self, change: &AuthenticationSessionsChange) {
        let default_account_provider = self.get_default_account_authentication_provider();
        if change.provider_id != default_account_provider.id {
            return;
        }
        // Providers can atomically replace a session, so only removal-only events represent an immediate sign-out.
        let added = change.event.added.as_ref().map_or(0, Vec::len);
        let removed = change.event.removed.as_ref().map_or(0, Vec::len);
        let changed = change.event.changed.as_ref().map_or(0, Vec::len);
        let current_session_id = self.current_session_id();
        let current_session_removed = current_session_id.as_ref().is_some_and(|id| change.event.removed.as_ref().is_some_and(|sessions| sessions.iter().any(|session| &session.id == id)));
        let has_replacement_candidates = added > 0 || changed > 0;
        let clear_default_account = current_session_removed && !has_replacement_candidates;
        let message = format!(
            "[DefaultAccount] Authentication sessions changed: added={added}, removed={removed}, changed={changed}, currentSessionRemoved={current_session_removed}, hasReplacementCandidates={has_replacement_candidates}, decision={}",
            if clear_default_account { "clear" } else { "reconcile" }
        );
        if current_session_removed || (added > 0 && removed > 0) {
            self.log_service.info(&message);
        } else {
            self.log_service.trace(&message);
        }
        if clear_default_account {
            self.set_default_account(None);
        } else {
            let _ = self.update_default_account(None);
        }
    }

    async fn when_default_account_authentication_provider_available(&self) -> Result<(), Error> {
        let provider = self.get_default_account_authentication_provider();

        self.log_service.debug("[DefaultAccount] Waiting for default account authentication provider to be available.");
        let disposables = DisposableStore::new();
        let (available, resolver) = Promise::<()>::new();
        // Check if the provider is available. If available, resolve immediately. Otherwise, wait for it to be
        // declared or registered.
        if self.is_account_provider_available(&provider) {
            self.log_service.debug("[DefaultAccount] Default account authentication provider is now available.");
            resolver.resolve(());
        } else {
            // Resolve as soon as the default account authentication provider is declared or registered, but wait no
            // longer than installed extensions being registered.
            let declared = self.authentication_service.on_did_change_declared_providers();
            let registered = self.authentication_service.on_did_register_authentication_provider();
            let weak = self.me.clone();
            let (listener_provider, listener_resolver) = (provider.clone(), resolver.clone());
            let when_available: Rc<dyn Fn()> = Rc::new(move || {
                let Some(this) = weak.upgrade() else { return };
                if this.is_account_provider_available(&listener_provider) {
                    this.log_service.debug("[DefaultAccount] Default account authentication provider is now available.");
                    listener_resolver.resolve(());
                }
            });
            // `Event.any(onDidChangeDeclaredProviders, onDidRegisterAuthenticationProvider)`: os dois eventos levam
            // cargas diferentes, e o ouvinte não lê nenhuma.
            disposables.add(declared.listen({
                let when_available = when_available.clone();
                move |_| when_available()
            }));
            disposables.add(registered.listen(move |_| when_available()));

            // Explicitly activate the provider's extension so that the authentication provider gets registered. In
            // desktop remote connections extensions are only registered after the connection is established, so
            // without this the provider would never become available.
            if self.environment_service.remote_authority().is_some_and(|authority| !authority.is_empty()) {
                let _ = self.authentication_service.get_sessions(&provider.id, None, Some(IAuthenticationGetSessionsOptions::default()), Some(true));
            }

            let (settle_disposables, settle_resolver) = (disposables.clone(), resolver.clone());
            let weak = self.me.clone();
            let _ = self.extension_service.when_installed_extensions_registered().then_result(move |result| {
                if let Some(this) = weak.upgrade() {
                    match result {
                        Ok(_) => {
                            settle_disposables.dispose();
                            this.log_service.debug("[DefaultAccount] Installed extensions registered.");
                        }
                        Err(error) => this.log_service.error_args(
                            &LogMessage::from("[DefaultAccount] Error while waiting for installed extensions to be registered"),
                            &[LogArg::from(error_message(&error).as_str())],
                        ),
                    }
                }
                settle_resolver.resolve(());
                Ok(())
            });
        }
        let result = available.await;
        disposables.dispose();
        result
    }

    fn refetch_default_account(&self) -> Promise<()> {
        let this = self.rc();
        async_fn(async move {
            if this.account_data_poll_scheduler.is_scheduled() {
                this.account_data_poll_scheduler.cancel();
            }
            if !this.host_service.has_focus() || this.default_account.borrow().is_none() {
                this.schedule_account_data_poll();
                this.log_service.debug("[DefaultAccount] Skipping refetching default account. Host is not focused or default account is not set");
                return Ok(());
            }
            this.log_service.debug("[DefaultAccount] Refetching default account");
            this.update_default_account(None).await
        })
    }

    fn update_default_account(&self, options: Option<IDefaultAccountRefreshOptions>) -> Promise<()> {
        let this = self.rc();
        self.update_throttler.trigger(move |_token| this.do_update_default_account(options), None)
    }

    fn do_update_default_account(&self, options: Option<IDefaultAccountRefreshOptions>) -> Promise<()> {
        let this = self.rc();
        async_fn(async move {
            let current_session_id = this.current_session_id();
            match this.fetch_default_account(options).await {
                Ok(default_account) => {
                    if current_session_id.is_some() && this.current_session_id() != current_session_id {
                        this.log_service.info("[DefaultAccount] Discarding default account update because the current session changed while the update was in progress");
                        return Ok(());
                    }
                    this.set_default_account(default_account);
                    this.schedule_account_data_poll();
                }
                Err(error) => {
                    this.log_service.error_args(&LogMessage::from("[DefaultAccount] Error while updating default account"), &[LogArg::from(error_message(&error).as_str())]);
                    this.block_pending_managed_settings_freshness();
                }
            }
            Ok(())
        })
    }

    async fn fetch_default_account(&self, options: Option<IDefaultAccountRefreshOptions>) -> Result<Option<IDefaultAccountData>, Error> {
        let default_account_provider = self.get_default_account_authentication_provider();
        self.log_service.debug_args("[DefaultAccount] Default account provider ID:", &[LogArg::from(default_account_provider.id.as_str())]);
        let managed_settings_sources = self.initialize_managed_settings_sources().await;
        let refresh_requirement = resolve_force_remote_settings_refresh(
            Some(&managed_settings_sources.native_mdm),
            self.get_cached_server_managed_settings(&default_account_provider).as_ref(),
            Some(&managed_settings_sources.file),
        );
        if requirement_channel(refresh_requirement).is_none() {
            self.set_managed_settings_freshness(IManagedSettingsFreshness::NotRequired);
        }

        if !self.is_account_provider_available(&default_account_provider) {
            self.log_service.info_args("[DefaultAccount] Authentication provider is not available.", &[LogArg::from(default_account_provider.id.as_str())]);
            self.block_managed_settings_freshness_without_token(refresh_requirement);
            return Ok(None);
        }

        self.get_default_account_for_authentication_provider(default_account_provider, managed_settings_sources, refresh_requirement, options).await
    }

    fn is_account_provider_available(&self, account_provider: &IDefaultAccountAuthenticationProvider) -> bool {
        self.authentication_service.declared_providers().iter().any(|provider| provider.id == account_provider.id)
            || self.authentication_service.is_authentication_provider_registered(&account_provider.id)
    }

    /// `setDefaultAccount(account)`.
    pub fn set_default_account(&self, account: Option<IDefaultAccountData>) {
        if *self.default_account.borrow() == account {
            return;
        }

        self.log_service.trace("[DefaultAccount] Updating default account");
        if let Some(account) = account {
            *self.default_account.borrow_mut() = Some(account.clone());
            self.set_copilot_token_info(account.copilot_token_info.clone());
            self.set_policy_data(account.policy_data.clone());
            self.set_managed_settings_compatibility_error(account.policy_data.as_ref().and_then(|policy_data| policy_data.managed_settings_compatibility_error.clone()));
            self.on_did_change_default_account.fire(Some(account.default_account.clone()));
            self.account_status_context.set(Dynamic::string(DefaultAccountStatus::Available.as_str()));
            self.log_service.debug("[DefaultAccount] Account status set to Available");
        } else {
            *self.default_account.borrow_mut() = None;
            let refresh_requirement = resolve_force_remote_settings_refresh(
                Some(&self.native_managed_settings_service.managed_settings()),
                self.get_cached_server_managed_settings(&self.get_default_account_authentication_provider()).as_ref(),
                Some(&self.file_managed_settings_service.managed_settings()),
            );
            self.block_managed_settings_freshness_without_token(refresh_requirement);
            let freshness = self.managed_settings_freshness.borrow().clone();
            let retained_policy_data = if freshness.state() != ManagedSettingsFreshnessState::NotRequired
                && is_managed_settings_freshness_blocking(&freshness)
                && freshness_source(&freshness) == Some(ManagedSettingsChannel::Server)
            {
                self.policy_data.borrow().clone()
            } else {
                None
            };
            self.set_policy_data(retained_policy_data);
            self.set_managed_settings_compatibility_error(None);
            self.set_copilot_token_info(None);
            self.on_did_change_default_account.fire(None);
            self.account_data_poll_scheduler.cancel();
            self.account_status_context.set(Dynamic::string(DefaultAccountStatus::Unavailable.as_str()));
            self.log_service.debug("[DefaultAccount] Account status set to Unavailable");
        }
    }

    /// `setPolicyData(accountPolicyData)`.
    pub fn set_policy_data(&self, account_policy_data: Option<IAccountPolicyData>) {
        if *self.policy_data.borrow() == account_policy_data {
            return;
        }
        *self.policy_data.borrow_mut() = account_policy_data.clone();
        self.cache_policy_data(account_policy_data.as_ref());
        self.on_did_change_policy_data.fire(account_policy_data.map(|data| data.policy_data));
    }

    /// `_policyData`.
    pub fn account_policy_data(&self) -> Option<IAccountPolicyData> {
        self.policy_data.borrow().clone()
    }

    /// `setManagedSettingsCompatibilityError(error)`.
    pub fn set_managed_settings_compatibility_error(&self, error: Option<IManagedSettingsCompatibilityError>) {
        if *self.managed_settings_compatibility_error.borrow() == error {
            return;
        }
        *self.managed_settings_compatibility_error.borrow_mut() = error.clone();
        self.on_did_change_managed_settings_compatibility_error.fire(error);
    }

    /// `setManagedSettingsFreshness(freshness)`.
    pub fn set_managed_settings_freshness(&self, freshness: IManagedSettingsFreshness) {
        if *self.managed_settings_freshness.borrow() == freshness {
            return;
        }
        *self.managed_settings_freshness.borrow_mut() = freshness.clone();
        self.on_did_change_managed_settings_freshness.fire(freshness);
    }

    fn block_pending_managed_settings_freshness(&self) {
        let current = self.managed_settings_freshness.borrow().clone();
        if let IManagedSettingsFreshness::Pending(effective) = current {
            self.set_managed_settings_freshness(IManagedSettingsFreshness::Blocked { effective, failure: ManagedSettingsFreshnessFailure::Network });
        }
    }

    fn update_managed_settings_freshness_requirement(&self, native_mdm: &ManagedSettingsData, server: Option<&ManagedSettingsData>, file: &ManagedSettingsData) {
        let requirement = resolve_force_remote_settings_refresh(Some(native_mdm), server, Some(file));
        self.set_managed_settings_freshness(match requirement_channel(requirement) {
            Some(source) => IManagedSettingsFreshness::Pending(IManagedSettingsFreshnessEffective { source, scope: None, last_attempt_at: None }),
            None => IManagedSettingsFreshness::NotRequired,
        });
    }

    /// `onManagedSettingsSourceChanged()`.
    pub fn on_managed_settings_source_changed(&self) {
        if self.initialized.get() {
            let _ = self.update_default_account(None);
        }
    }

    /// `initialized`: o teste a escreve (`provider['initialized'] = false`).
    pub fn set_initialized(&self, initialized: bool) {
        self.initialized.set(initialized);
    }

    /// `_rateLimitBackoffUntil`: o teste a escreve.
    pub fn set_rate_limit_backoff_until(&self, until: u64) {
        self.rate_limit_backoff_until.set(until);
    }

    async fn initialize_managed_settings_sources(&self) -> IManagedSettingsSources {
        let mut native_mdm = self.native_managed_settings_service.managed_settings();
        match self.native_managed_settings_service.initialize().await {
            Ok(initialized) => native_mdm = initialized,
            Err(error) => self.log_service.warn_args(
                &LogMessage::from("[DefaultAccount] Failed to initialize native managed settings before resolving forceRemoteSettingsRefresh; using available values"),
                &[LogArg::from(error_message(&error).as_str())],
            ),
        }

        let mut file = self.file_managed_settings_service.managed_settings();
        match self.file_managed_settings_service.initialize().await {
            Ok(initialized) => file = initialized,
            Err(error) => self.log_service.warn_args(
                &LogMessage::from("[DefaultAccount] Failed to initialize file managed settings before resolving forceRemoteSettingsRefresh; using available values"),
                &[LogArg::from(error_message(&error).as_str())],
            ),
        }
        IManagedSettingsSources { native_mdm, file }
    }

    fn block_managed_settings_freshness_without_token(&self, requirement: IForceRemoteSettingsRefreshResolution) {
        if let Some(source) = requirement_channel(requirement) {
            self.set_managed_settings_freshness(blocked_without_attempt(source, ManagedSettingsFreshnessFailure::NoToken));
        }
    }

    fn get_cached_server_managed_settings(&self, authentication_provider: &IDefaultAccountAuthenticationProvider) -> Option<ManagedSettingsData> {
        let policy_data = self.policy_data.borrow().clone();
        self.get_scoped_server_managed_settings(policy_data.as_ref(), authentication_provider, policy_data.as_ref().map(|data| data.account_id.as_str()), None)
    }

    fn get_scoped_server_managed_settings(
        &self,
        account_policy_data: Option<&IAccountPolicyData>,
        authentication_provider: &IDefaultAccountAuthenticationProvider,
        account_id: Option<&str>,
        session_context: Option<&IAccountSessionContext>,
    ) -> Option<ManagedSettingsData> {
        let account_policy_data = account_policy_data.filter(|data| Some(data.account_id.as_str()) == account_id)?;
        if let Some(scope) = &account_policy_data.managed_settings_scope {
            let session_context = session_context.cloned().unwrap_or_else(|| self.get_account_session_context(None, authentication_provider));
            let managed_settings_url = self.get_managed_settings_url(&session_context);
            if Some(scope.account_id.as_str()) != account_id
                || scope.authentication_provider_id != authentication_provider.id
                || managed_settings_url.is_some_and(|url| scope.endpoint_origin != create_managed_settings_freshness_scope(&scope.account_id, &authentication_provider.id, &url).endpoint_origin)
            {
                return None;
            }
        }
        account_policy_data.policy_data.managed_settings.clone()
    }

    fn set_copilot_token_info(&self, copilot_token_info: Option<ICopilotTokenInfo>) {
        if *self.copilot_token_info.borrow() == copilot_token_info {
            return;
        }
        *self.copilot_token_info.borrow_mut() = copilot_token_info.clone();
        self.on_did_change_copilot_token_info.fire(copilot_token_info);
    }

    fn cache_policy_data(&self, account_policy_data: Option<&IAccountPolicyData>) {
        if let Some(account_policy_data) = account_policy_data {
            self.log_service.debug_args("[DefaultAccount] Caching policy data for account:", &[LogArg::from(account_policy_data.account_id.as_str())]);
            let cached_account_data = ICachedAccountData { account_policy_data: account_policy_data.clone(), copilot_token_info: self.copilot_token_info.borrow().clone() };
            self.storage_service.store(CACHED_POLICY_DATA_KEY, StorageValue::String(cached_account_data.to_json().to_string()), StorageScope::Application, StorageTarget::Machine);
        } else {
            self.log_service.debug("[DefaultAccount] Removing cached policy data");
            self.storage_service.remove(CACHED_POLICY_DATA_KEY, StorageScope::Application);
        }
    }

    fn schedule_account_data_poll(&self) {
        if self.default_account.borrow().is_none() {
            return;
        }
        self.account_data_poll_scheduler.schedule(Some(ACCOUNT_DATA_POLL_INTERVAL_MS));
    }

    /// `extractFromToken(token)`: os campos `chave=valor` da primeira parte do token (antes do `:`); um campo sem
    /// `=` guarda a chave sem valor.
    fn extract_from_token(&self, token: &str) -> IndexMap<String, Option<String>> {
        let mut result = IndexMap::new();
        let first_part = token.split(':').next().unwrap_or_default();
        for field in first_part.split(';') {
            let mut parts = field.split('=');
            let key = parts.next().unwrap_or_default();
            result.insert(key.to_owned(), parts.next().map(str::to_owned));
        }
        let fields: serde_json::Map<String, serde_json::Value> = result.iter().filter_map(|(key, value)| value.as_ref().map(|value| (key.clone(), value.clone().into()))).collect();
        self.log_service.debug(&format!("[DefaultAccount] extractFromToken: {}", serde_json::Value::Object(fields)));
        result
    }

    async fn get_default_account_for_authentication_provider(
        &self,
        authentication_provider: IDefaultAccountAuthenticationProvider,
        managed_settings_sources: IManagedSettingsSources,
        refresh_requirement: IForceRemoteSettingsRefreshResolution,
        options: Option<IDefaultAccountRefreshOptions>,
    ) -> Result<Option<IDefaultAccountData>, Error> {
        let lookup = async {
            self.log_service.debug_args("[DefaultAccount] Getting Default Account from authenticated sessions for provider:", &[LogArg::from(authentication_provider.id.as_str())]);
            let sessions = self.find_matching_provider_session(&authentication_provider.id, self.default_account_config.authentication_provider.scopes.clone()).await?;

            let Some(sessions) = sessions.filter(|sessions| !sessions.is_empty()) else {
                self.log_service.debug_args("[DefaultAccount] No matching session found for provider:", &[LogArg::from(authentication_provider.id.as_str())]);
                self.block_managed_settings_freshness_without_token(refresh_requirement);
                return Ok(None);
            };
            self.get_default_account_from_authenticated_sessions(authentication_provider.clone(), sessions, options, Some(managed_settings_sources.clone())).await
        };
        match lookup.await {
            Ok(account) => Ok(account),
            Err(error) => {
                self.log_service.error_args(
                    &LogMessage::from("[DefaultAccount] Failed to get default account for provider:"),
                    &[LogArg::from(authentication_provider.id.as_str()), LogArg::from(error_message(&error).as_str())],
                );
                // A failed lookup (for example while an extension host restart re-registers the provider) is not a
                // sign-out. Keep the known account with its policy and freshness; session removals still clear it
                // through onDidChangeSessions.
                let current = self.default_account.borrow().clone();
                if let Some(current) = current.filter(|current| current.default_account.authentication_provider.id == authentication_provider.id) {
                    self.log_service.info("[DefaultAccount] Keeping the current default account after the session lookup failed");
                    return Ok(Some(current));
                }
                self.block_managed_settings_freshness_without_token(refresh_requirement);
                Ok(None)
            }
        }
    }

    /// `getDefaultAccountFromAuthenticatedSessions(authenticationProvider, sessions, options?, managedSettingsSources?)`.
    pub fn get_default_account_from_authenticated_sessions(
        &self,
        authentication_provider: IDefaultAccountAuthenticationProvider,
        sessions: Vec<AuthenticationSession>,
        options: Option<IDefaultAccountRefreshOptions>,
        managed_settings_sources: Option<IManagedSettingsSources>,
    ) -> Promise<Option<IDefaultAccountData>> {
        let this = self.rc();
        async_fn(async move {
            match this.build_default_account(&authentication_provider, sessions, options, managed_settings_sources).await {
                Ok(account) => Ok(account),
                Err(error) => {
                    this.log_service.error_args(
                        &LogMessage::from("[DefaultAccount] Failed to create default account for provider:"),
                        &[LogArg::from(authentication_provider.id.as_str()), LogArg::from(error_message(&error).as_str())],
                    );
                    this.block_pending_managed_settings_freshness();
                    Ok(None)
                }
            }
        })
    }

    /// O corpo do `try` de `getDefaultAccountFromAuthenticatedSessions`.
    async fn build_default_account(
        &self,
        authentication_provider: &IDefaultAccountAuthenticationProvider,
        sessions: Vec<AuthenticationSession>,
        options: Option<IDefaultAccountRefreshOptions>,
        managed_settings_sources: Option<IManagedSettingsSources>,
    ) -> Result<Option<IDefaultAccountData>, Error> {
        let session = sessions.first().ok_or_else(|| Error::type_error("Cannot read properties of undefined (reading 'account')"))?;
        let session_context = self.get_account_session_context(Some(session), authentication_provider);
        if authentication_provider.enterprise && session_context.enterprise_uri.is_none() {
            self.log_service.warn("[DefaultAccount] Cannot resolve the selected enterprise session authorization server");
            return Ok(None);
        }
        let account_id = session.account.id.clone();
        let enterprise_uri = session_context.enterprise_uri.as_ref().map(|uri| ext_uri().get_comparison_key(uri, false).to_string_lossy());
        let account_policy_data = self.policy_data.borrow().clone().filter(|data| data.account_id == account_id && data.enterprise_uri == enterprise_uri);
        let sources = match managed_settings_sources {
            Some(sources) => sources,
            None => self.initialize_managed_settings_sources().await,
        };
        let requirement = resolve_force_remote_settings_refresh(
            Some(&sources.native_mdm),
            self.get_scoped_server_managed_settings(account_policy_data.as_ref(), authentication_provider, Some(&account_id), Some(&session_context)).as_ref(),
            Some(&sources.file),
        );
        let managed_settings_url = self.get_managed_settings_url(&session_context);
        let scope = managed_settings_url.map(|url| create_managed_settings_freshness_scope(&account_id, &authentication_provider.id, &url));
        match requirement_channel(requirement) {
            None => self.set_managed_settings_freshness(IManagedSettingsFreshness::NotRequired),
            Some(source) => {
                let satisfied = scope.as_ref().is_some_and(|scope| is_managed_settings_freshness_satisfied_for(&self.managed_settings_freshness.borrow(), scope));
                if !satisfied && self.can_request_managed_settings(options, scope.as_ref()) {
                    self.set_managed_settings_freshness(IManagedSettingsFreshness::Pending(IManagedSettingsFreshnessEffective { source, scope: scope.clone(), last_attempt_at: None }));
                }
            }
        }

        let entitlements_result = self.get_entitlements(&sessions, account_policy_data.as_ref(), options, &session_context).await?;
        let entitlements_data = entitlements_result.data;
        let entitlements_fetched_at = entitlements_result.fetched_at;
        let chat_enabled = entitlements_data.as_ref().is_some_and(|data| data.chat_enabled);
        let token_entitlements_promise = chat_enabled.then(|| self.get_token_entitlements(sessions.clone(), account_policy_data.clone(), options, session_context.clone()));
        let managed_settings_promise = (chat_enabled || requirement_channel(requirement).is_some()).then(|| {
            self.get_managed_settings(
                sessions.clone(),
                account_policy_data.clone(),
                options,
                IManagedSettingsFetchContext {
                    authentication_provider: Some(authentication_provider.clone()),
                    managed_settings_sources: Some(sources.clone()),
                    refresh_requirement: Some(requirement),
                    session_context: Some(session_context.clone()),
                },
            )
        });
        let token_entitlements_result = match token_entitlements_promise {
            Some(promise) => Some(promise.await?),
            None => None,
        };
        let managed_settings_result = match managed_settings_promise {
            Some(promise) => Some(promise.await?),
            None => None,
        };

        let token_entitlements_fetched_at = token_entitlements_result.as_ref().map(|result| result.fetched_at);
        let managed_settings_fetched_at = managed_settings_result.as_ref().and_then(|result| result.fetched_at);
        let managed_settings_scope = managed_settings_result.as_ref().and_then(|result| result.scope.clone()).or_else(|| account_policy_data.as_ref().and_then(|data| data.managed_settings_scope.clone()));
        let managed_settings_compatibility_error = match &managed_settings_result {
            Some(result) => result.compatibility_error.clone(),
            None => self.managed_settings_compatibility_error.borrow().clone(),
        };
        let mut mcp_registry_data_fetched_at: Option<u64> = None;
        let mut policy_data: Option<IPolicyData> = account_policy_data.as_ref().map(|data| data.policy_data.clone());
        if let Some(entitlements_data) = &entitlements_data {
            policy_data.get_or_insert_with(empty_policy_data).cloud_session_storage_enabled = entitlements_data.cloud_session_storage_enabled;
        }
        if let Some(token_entitlements_data) = token_entitlements_result.as_ref().and_then(|result| result.data.as_ref()) {
            let policy = policy_data.get_or_insert_with(empty_policy_data);
            policy.chat_agent_enabled = token_entitlements_data.policy_data.chat_agent_enabled;
            policy.chat_preview_features_enabled = token_entitlements_data.policy_data.chat_preview_features_enabled;
            policy.mcp = token_entitlements_data.policy_data.mcp;
            if policy.mcp.unwrap_or(false) {
                let mcp_registry_result = self.get_mcp_registry_provider(&sessions, account_policy_data.as_ref(), options, &session_context).await?;
                mcp_registry_data_fetched_at = mcp_registry_result.as_ref().map(|result| result.fetched_at);
                let registry = mcp_registry_result.and_then(|result| result.data);
                let policy = policy_data.get_or_insert_with(empty_policy_data);
                policy.mcp_registry_url = registry.as_ref().map(|registry| registry.url.clone());
                policy.mcp_access = registry.and_then(|registry| registry.registry_access);
            } else {
                policy.mcp_registry_url = None;
                policy.mcp_access = None;
            }
        }
        if let Some(patch) = managed_settings_result.as_ref().and_then(|result| result.data.as_ref()) {
            let mut merged = policy_data.take().unwrap_or_else(empty_policy_data);
            merged.managed_settings = patch.managed_settings.clone();
            if let Some(active) = patch.managed_settings_active {
                merged.managed_settings_active = Some(active);
            }
            policy_data = Some(merged);
        }

        let default_account = IDefaultAccount {
            authentication_provider: authentication_provider.clone(),
            account_name: session.account.label.clone(),
            session_id: session.id.clone(),
            enterprise: authentication_provider.enterprise || session.account.label.contains('_'),
            entitlements_data,
            extra: Default::default(),
        };
        self.log_service.debug_args("[DefaultAccount] Successfully created default account for provider:", &[LogArg::from(authentication_provider.id.as_str())]);
        let account_policy_result = (policy_data.is_some() || present(entitlements_fetched_at).is_some()).then(|| IAccountPolicyData {
            account_id: account_id.clone(),
            enterprise_uri,
            policy_data: policy_data.unwrap_or_else(empty_policy_data),
            entitlements_fetched_at,
            token_entitlements_fetched_at,
            mcp_registry_data_fetched_at,
            managed_settings_fetched_at,
            managed_settings_scope,
            managed_settings_compatibility_error,
        });
        Ok(Some(IDefaultAccountData {
            default_account,
            account_id,
            enterprise_uri: session_context.enterprise_uri,
            policy_data: account_policy_result,
            copilot_token_info: token_entitlements_result.and_then(|result| result.data).map(|data| data.copilot_token_info),
        }))
    }

    /// `findMatchingProviderSession(authProviderId, allScopes)`.
    pub fn find_matching_provider_session(&self, auth_provider_id: &str, all_scopes: Vec<Vec<String>>) -> Promise<Option<Vec<AuthenticationSession>>> {
        let this = self.rc();
        let auth_provider_id = auth_provider_id.to_owned();
        async_fn(async move {
            let sessions = this.get_sessions(&auth_provider_id).await?;
            let matching_sessions: Vec<AuthenticationSession> = sessions
                .into_iter()
                .filter(|session| {
                    this.log_service.debug_args(
                        "[DefaultAccount] Checking session with scopes",
                        &[LogArg::from(Dynamic::array(session.scopes.iter().map(|scope| Dynamic::string(scope))))],
                    );
                    all_scopes.iter().any(|scopes| scopes_match(&session.scopes, scopes))
                })
                .collect();
            Ok((!matching_sessions.is_empty()).then_some(matching_sessions))
        })
    }

    async fn get_sessions(&self, auth_provider_id: &str) -> Result<Vec<AuthenticationSession>, Error> {
        for attempt in 1..=3 {
            let lookup = async {
                let mut preferred_account_name: Option<String> = None;
                for preferred_extension in &self.default_account_config.preferred_extensions {
                    preferred_account_name = self.authentication_extensions_service.get_account_preference(preferred_extension, auth_provider_id);
                    if preferred_account_name.as_deref().is_some_and(|name| !name.is_empty()) {
                        break;
                    }
                }
                let mut preferred_account: Option<AuthenticationSessionAccount> = None;
                for account in self.authentication_service.get_accounts(auth_provider_id).await? {
                    if Some(account.label.as_str()) == preferred_account_name.as_deref() {
                        preferred_account = Some(account);
                        break;
                    }
                }

                self.authentication_service
                    .get_sessions(auth_provider_id, None, Some(IAuthenticationProviderSessionOptions { account: preferred_account, ..Default::default() }), Some(true))
                    .await
            };
            match lookup.await {
                Ok(sessions) => return Ok(sessions),
                Err(error) => {
                    self.log_service.warn_args(&LogMessage::from(format!("[DefaultAccount] Attempt {attempt} to get sessions failed:").as_str()), &[LogArg::from(error_message(&error).as_str())]);
                    if attempt == 3 {
                        return Err(error);
                    }
                    timeout(500).promise.await?;
                }
            }
        }
        Err(Error::new("Unable to get sessions after multiple attempts"))
    }

    fn get_token_entitlements(
        &self,
        sessions: Vec<AuthenticationSession>,
        account_policy_data: Option<IAccountPolicyData>,
        options: Option<IDefaultAccountRefreshOptions>,
        session_context: IAccountSessionContext,
    ) -> Promise<ITokenEntitlementsResult> {
        let this = self.rc();
        async_fn(async move {
            let force_refresh = options.and_then(|options| options.force_refresh).unwrap_or(false);
            if let Some(account_policy_data) = &account_policy_data
                && !force_refresh
                && let Some(fetched_at) = present(account_policy_data.token_entitlements_fetched_at)
                && !this.is_data_stale(fetched_at)
            {
                this.log_service.debug("[DefaultAccount] Using last fetched token entitlements data");
                let copilot_token_info = this.copilot_token_info.borrow().clone().unwrap_or(ICopilotTokenInfo { sn: None, fcv1: None, extra: Default::default() });
                return Ok(ITokenEntitlementsResult { data: Some(ITokenEntitlementsData { policy_data: account_policy_data.policy_data.clone(), copilot_token_info }), fetched_at });
            }
            let data = this.request_token_entitlements(sessions, &session_context).await?;
            Ok(ITokenEntitlementsResult { data, fetched_at: now() })
        })
    }

    async fn request_token_entitlements(&self, sessions: Vec<AuthenticationSession>, session_context: &IAccountSessionContext) -> Result<Option<ITokenEntitlementsData>, Error> {
        let Some(token_entitlements_url) = self.get_token_entitlement_url(session_context) else {
            self.log_service.debug("[DefaultAccount] No token entitlements URL found");
            return Ok(None);
        };

        self.log_service.debug_args("[DefaultAccount] Fetching token entitlements from:", &[LogArg::from(token_entitlements_url.as_str())]);
        let response = self
            .request(
                &token_entitlements_url,
                RequestMethod::Get,
                None,
                sessions,
                CancellationToken::None,
                "defaultAccount.tokenEntitlements",
                IAuthenticatedRequestOptions { session_context: Some(session_context.clone()), ..Default::default() },
            )
            .await?;
        let Some(response) = response else {
            return Ok(None);
        };

        if let Some(status) = response.res.status_code.filter(|status| *status != 0 && *status != 200) {
            self.log_service.trace(&format!("[DefaultAccount] unexpected status code {status} while fetching token entitlements"));
            return Ok(None);
        }

        let parsed: Result<Option<ITokenEntitlementsData>, Error> = async {
            let chat_data = as_json(&response).await?.filter(|data| !data.is_nullish());
            if let Some(chat_data) = chat_data {
                let Dynamic::String(token) = chat_data.get_str("token") else {
                    return Err(Error::type_error("Cannot read properties of undefined (reading 'split')"));
                };
                let token_map = self.extract_from_token(&token.to_string_lossy());
                let value_of = |key: &str| token_map.get(key).and_then(Option::as_deref);
                return Ok(Some(ITokenEntitlementsData {
                    policy_data: {
                        let mut policy_data = empty_policy_data();
                        // Editor preview features are disabled if the flag is present and set to 0
                        policy_data.chat_preview_features_enabled = Some(value_of("editor_preview_features") != Some("0"));
                        policy_data.chat_agent_enabled = Some(value_of("agent_mode") != Some("0"));
                        // MCP is only enabled if the flag is explicitly present and set to 1
                        policy_data.mcp = Some(value_of("mcp") == Some("1"));
                        policy_data
                    },
                    copilot_token_info: ICopilotTokenInfo { sn: value_of("sn").map(str::to_owned), fcv1: value_of("fcv1").map(str::to_owned), extra: Default::default() },
                }));
            }
            self.log_service.error_args(&LogMessage::from("Failed to fetch token entitlements"), &[LogArg::from("No data returned")]);
            Ok(None)
        }
        .await;
        match parsed {
            Ok(data) => Ok(data),
            Err(error) => {
                self.log_service.error_args(&LogMessage::from("Failed to fetch token entitlements"), &[LogArg::from(error_message(&error).as_str())]);
                Ok(None)
            }
        }
    }

    async fn get_entitlements(
        &self,
        sessions: &[AuthenticationSession],
        account_policy_data: Option<&IAccountPolicyData>,
        options: Option<IDefaultAccountRefreshOptions>,
        session_context: &IAccountSessionContext,
    ) -> Result<IEntitlementsResult, Error> {
        let account_id = sessions.first().ok_or_else(|| Error::type_error("Cannot read properties of undefined (reading 'account')"))?.account.id.clone();
        let existing_data = self
            .default_account
            .borrow()
            .as_ref()
            .filter(|data| data.account_id == account_id && ext_uri().is_equal(data.enterprise_uri.as_ref(), session_context.enterprise_uri.as_ref(), false))
            .and_then(|data| data.default_account.entitlements_data.clone());
        let options = options.unwrap_or_default();
        if !options.force_refresh.unwrap_or(false)
            && !options.refresh_entitlements.unwrap_or(false)
            && let Some(existing_data) = existing_data
            && let Some(fetched_at) = account_policy_data.and_then(|data| present(data.entitlements_fetched_at))
            && !self.is_data_stale(fetched_at)
        {
            self.log_service.debug("[DefaultAccount] Using last fetched entitlements data");
            return Ok(IEntitlementsResult { data: Some(existing_data), fetched_at: Some(fetched_at) });
        }

        let Some(entitlement_url) = self.get_entitlement_url(session_context) else {
            self.log_service.debug("[DefaultAccount] No chat entitlements URL found");
            return Ok(IEntitlementsResult { data: None, fetched_at: None });
        };

        self.log_service.debug_args("[DefaultAccount] Fetching entitlements from:", &[LogArg::from(entitlement_url.as_str())]);
        let response = self
            .request(
                &entitlement_url,
                RequestMethod::Get,
                None,
                sessions.to_vec(),
                CancellationToken::None,
                "defaultAccount.entitlements",
                IAuthenticatedRequestOptions { session_context: Some(session_context.clone()), ..Default::default() },
            )
            .await?;
        let Some(response) = response else {
            return Ok(IEntitlementsResult { data: None, fetched_at: Some(now()) });
        };

        if let Some(status) = response.res.status_code.filter(|status| *status != 0 && *status != 200) {
            // 401: oauth token being unavailable (expired/revoked). 404: missing scopes/permissions, service pretends
            // the endpoint doesn't exist. Both are `null` in the original, and `undefined` otherwise; as the result
            // here is an `Option`, they collapse into `None`.
            self.log_service.trace(&format!("[DefaultAccount] unexpected status code {status} while fetching entitlements"));
            return Ok(IEntitlementsResult { data: None, fetched_at: Some(now()) });
        }

        let parsed: Result<Option<IEntitlementsData>, Error> = async {
            let data = as_json(&response).await?.filter(|data| !data.is_nullish());
            match data {
                Some(data) => entitlements_from_dynamic(&data).map(Some).ok_or_else(|| Error::new("Unexpected entitlements response")),
                None => Ok(None),
            }
        }
        .await;
        match parsed {
            Ok(Some(data)) => return Ok(IEntitlementsResult { data: Some(data), fetched_at: Some(now()) }),
            Ok(None) => self.log_service.error_args(&LogMessage::from("[DefaultAccount] Failed to fetch entitlements"), &[LogArg::from("No data returned")]),
            Err(error) => self.log_service.error_args(&LogMessage::from("[DefaultAccount] Failed to fetch entitlements"), &[LogArg::from(error_message(&error).as_str())]),
        }
        Ok(IEntitlementsResult { data: None, fetched_at: Some(now()) })
    }

    async fn get_mcp_registry_provider(
        &self,
        sessions: &[AuthenticationSession],
        account_policy_data: Option<&IAccountPolicyData>,
        options: Option<IDefaultAccountRefreshOptions>,
        session_context: &IAccountSessionContext,
    ) -> Result<Option<IMcpRegistryResult>, Error> {
        let force_refresh = options.and_then(|options| options.force_refresh).unwrap_or(false);
        if let Some(account_policy_data) = account_policy_data
            && !force_refresh
            && let Some(fetched_at) = present(account_policy_data.mcp_registry_data_fetched_at)
            && !self.is_data_stale(fetched_at)
        {
            self.log_service.debug("[DefaultAccount] Using last fetched MCP registry data");
            let policy_data = &account_policy_data.policy_data;
            let data = match (&policy_data.mcp_registry_url, policy_data.mcp_access) {
                (Some(url), Some(access)) if !url.is_empty() => Some(IMcpRegistryProvider { url: url.clone(), registry_access: Some(access) }),
                _ => None,
            };
            return Ok(Some(IMcpRegistryResult { data, fetched_at }));
        }
        let data = self.request_mcp_registry_provider(sessions, session_context).await?;
        Ok(data.map(|data| IMcpRegistryResult { data, fetched_at: now() }))
    }

    /// `requestMcpRegistryProvider`: `Some(None)` é o `null` do original (sem registro) e `None` o `undefined`.
    async fn request_mcp_registry_provider(&self, sessions: &[AuthenticationSession], session_context: &IAccountSessionContext) -> Result<Option<Option<IMcpRegistryProvider>>, Error> {
        let Some(mcp_registry_data_url) = self.get_mcp_registry_data_url(session_context) else {
            self.log_service.debug("[DefaultAccount] No MCP registry data URL found");
            return Ok(Some(None));
        };

        self.log_service.debug_args("[DefaultAccount] Fetching MCP registry data from:", &[LogArg::from(mcp_registry_data_url.as_str())]);
        let response = self
            .request(
                &mcp_registry_data_url,
                RequestMethod::Get,
                None,
                sessions.to_vec(),
                CancellationToken::None,
                "defaultAccount.mcpRegistryProvider",
                IAuthenticatedRequestOptions { session_context: Some(session_context.clone()), ..Default::default() },
            )
            .await?;
        let Some(response) = response else {
            return Ok(None);
        };

        if !is_success(&response) {
            if is_client_error(&response) {
                self.log_service.debug(&format!(
                    "[DefaultAccount] Received {} for MCP registry data, treating as no registry available.",
                    response.res.status_code.map_or_else(|| "undefined".to_owned(), |status| status.to_string())
                ));
                return Ok(Some(None));
            }
            self.log_service.debug(&format!(
                "[DefaultAccount] unexpected status code {} while fetching MCP registry data",
                response.res.status_code.map_or_else(|| "undefined".to_owned(), |status| status.to_string())
            ));
            return Ok(None);
        }

        let parsed: Result<Option<IMcpRegistryProvider>, Error> = async {
            let data = as_json(&response).await?.filter(|data| !data.is_nullish());
            let Some(data) = data else {
                self.log_service.debug("No MCP registry providers content found in response");
                return Ok(None);
            };
            let Dynamic::Array(registries) = data.get_str("mcp_registries") else {
                return Err(Error::type_error("Cannot read properties of undefined (reading '0')"));
            };
            self.log_service.debug_args("Fetched MCP registry providers", &[LogArg::from(Dynamic::Array(registries.clone()))]);
            let first = registries.get(0);
            if first.is_nullish() {
                return Ok(None);
            }
            Ok(Some(IMcpRegistryProvider {
                url: match first.get_str("url") {
                    Dynamic::String(url) => url.to_string_lossy(),
                    _ => String::new(),
                },
                registry_access: match first.get_str("registry_access") {
                    Dynamic::String(access) if access.to_string_lossy() == "allow_all" => Some(McpAccess::AllowAll),
                    Dynamic::String(access) if access.to_string_lossy() == "registry_only" => Some(McpAccess::RegistryOnly),
                    _ => None,
                },
            }))
        }
        .await;
        match parsed {
            Ok(provider) => Ok(Some(provider)),
            Err(error) => {
                self.log_service.error_args(&LogMessage::from("Failed to fetch MCP registry providers"), &[LogArg::from(error_message(&error).as_str())]);
                Ok(None)
            }
        }
    }

    /// `getManagedSettings(sessions, accountPolicyData, options?, authenticationProvider?, managedSettingsSources?,
    /// refreshRequirement?, sessionContext?)`: os quatro últimos vão em [`IManagedSettingsFetchContext`].
    pub fn get_managed_settings(
        &self,
        sessions: Vec<AuthenticationSession>,
        account_policy_data: Option<IAccountPolicyData>,
        options: Option<IDefaultAccountRefreshOptions>,
        context: IManagedSettingsFetchContext,
    ) -> Promise<IManagedSettingsResult> {
        let this = self.rc();
        async_fn(async move {
            let authentication_provider = context.authentication_provider.unwrap_or_else(|| this.get_default_account_authentication_provider());
            let session_context = context.session_context.unwrap_or_else(|| this.get_account_session_context(sessions.first(), &authentication_provider));
            let account_id = sessions.first().ok_or_else(|| Error::type_error("Cannot read properties of undefined (reading 'account')"))?.account.id.clone();
            let sources = match context.managed_settings_sources {
                Some(sources) => sources,
                None => this.initialize_managed_settings_sources().await,
            };
            let requirement = context.refresh_requirement.unwrap_or_else(|| {
                resolve_force_remote_settings_refresh(
                    Some(&sources.native_mdm),
                    this.get_scoped_server_managed_settings(account_policy_data.as_ref(), &authentication_provider, Some(&account_id), Some(&session_context)).as_ref(),
                    Some(&sources.file),
                )
            });
            let requirement_source = requirement_channel(requirement);
            let options = options.unwrap_or_default();
            let force_refresh = options.force_refresh.unwrap_or(false);
            let retry_managed_settings = options.retry_managed_settings.unwrap_or(false);
            let cached_managed_settings: Option<(IManagedSettingsPatch, u64)> = account_policy_data
                .as_ref()
                .and_then(|data| data.managed_settings_fetched_at.filter(|fetched_at| !this.is_data_stale(*fetched_at)).map(|fetched_at| (IManagedSettingsPatch::of(data.policy_data.managed_settings.clone()), fetched_at)));
            let Some(managed_settings_url) = this.get_managed_settings_url(&session_context) else {
                this.log_service.debug("[DefaultAccount] No managed settings URL configured; skipping enterprise policy fetch");
                this.managed_settings_fetch_status.set(Some(ManagedSettingsFetchStatus::NoUrl));
                if let Some(source) = requirement_source {
                    this.set_managed_settings_freshness(blocked_without_attempt(source, ManagedSettingsFreshnessFailure::NoUrl));
                }
                let retained: Option<(IManagedSettingsPatch, Option<u64>)> = if requirement_source.is_some() {
                    Some((IManagedSettingsPatch::of(account_policy_data.as_ref().and_then(|data| data.policy_data.managed_settings.clone())), account_policy_data.as_ref().and_then(|data| data.managed_settings_fetched_at)))
                } else {
                    cached_managed_settings.map(|(data, fetched_at)| (data, Some(fetched_at)))
                };
                return Ok(IManagedSettingsResult {
                    data: retained.as_ref().map(|(data, _)| data.clone()),
                    fetched_at: retained.and_then(|(_, fetched_at)| fetched_at),
                    scope: account_policy_data.as_ref().and_then(|data| data.managed_settings_scope.clone()),
                    compatibility_error: this.managed_settings_compatibility_error.borrow().clone(),
                });
            };

            let scope = create_managed_settings_freshness_scope(&account_id, &authentication_provider.id, &managed_settings_url);
            let cached_scope = account_policy_data.as_ref().and_then(|data| data.managed_settings_scope.clone());
            // Only reuse a cache captured for the current provider and endpoint (a legacy cache with no recorded
            // scope is trusted), so a previous GitHub Enterprise host's policy is not applied after a scope switch.
            let cache_scope_matches = cached_scope.is_none_or(|cached_scope| managed_settings_scope_key(&cached_scope) == managed_settings_scope_key(&scope));
            let scoped_managed_settings = if cache_scope_matches { account_policy_data.as_ref().and_then(|data| data.policy_data.managed_settings.clone()) } else { None };
            let scoped_managed_settings_fetched_at = if cache_scope_matches { account_policy_data.as_ref().and_then(|data| data.managed_settings_fetched_at) } else { None };
            let scoped_cached_managed_settings = if cache_scope_matches { cached_managed_settings } else { None };
            if requirement_source.is_some() && !this.can_request_managed_settings(Some(options), Some(&scope)) {
                this.log_service.debug("[DefaultAccount] Skipping automatic managed settings retry after a prior failure");
                let failed_freshness = this.failed_managed_settings_freshness.borrow().get(&managed_settings_scope_key(&scope)).cloned();
                if let (Some(IManagedSettingsFreshness::Blocked { effective, failure }), Some(source)) = (failed_freshness, requirement_source) {
                    this.set_managed_settings_freshness(IManagedSettingsFreshness::Blocked { effective: IManagedSettingsFreshnessEffective { source, ..effective }, failure });
                }
                return Ok(IManagedSettingsResult {
                    data: Some(IManagedSettingsPatch::of(scoped_managed_settings)),
                    fetched_at: scoped_managed_settings_fetched_at,
                    scope: Some(scope),
                    compatibility_error: this.managed_settings_compatibility_error.borrow().clone(),
                });
            }
            let freshness_satisfied = requirement_source.is_some() && is_managed_settings_freshness_satisfied_for(&this.managed_settings_freshness.borrow(), &scope);
            // When forceRemoteSettingsRefresh is effective, reuse also requires this scope's freshness to be
            // satisfied; an outstanding compatibility error always forces revalidation.
            if !force_refresh
                && !retry_managed_settings
                && let Some((data, fetched_at)) = &scoped_cached_managed_settings
                && (requirement_source.is_none() || freshness_satisfied)
                && this.managed_settings_compatibility_error.borrow().is_none()
            {
                this.log_service.debug("[DefaultAccount] Using last fetched managed settings data");
                return Ok(IManagedSettingsResult { data: Some(data.clone()), fetched_at: Some(*fetched_at), scope: Some(scope), compatibility_error: None });
            }

            let last_attempt_at = now();
            // Keep the accepted policy active during a same-scope refresh; failures below still close the gate.
            if let Some(source) = requirement_source
                && !freshness_satisfied
            {
                this.set_managed_settings_freshness(IManagedSettingsFreshness::Pending(IManagedSettingsFreshnessEffective { source, scope: Some(scope.clone()), last_attempt_at: Some(last_attempt_at) }));
            }
            let shared_backoff_active = now() < this.rate_limit_backoff_until.get();
            let sessions_for_request = if requirement_source.is_some() { vec![sessions[0].clone()] } else { sessions.clone() };
            let result = this.request_managed_settings(sessions_for_request, &managed_settings_url, &session_context).await?;
            if let Some(source) = requirement_source
                && !shared_backoff_active
            {
                this.update_failed_managed_settings_freshness(&scope, source, &result, last_attempt_at);
            }
            match result {
                ManagedSettingsRequestResult::Success { data } => {
                    let fetched_at = now();
                    this.resolve_managed_settings_freshness_after_success(data.managed_settings.clone(), &scope, last_attempt_at, fetched_at);
                    Ok(IManagedSettingsResult { data: Some(data), fetched_at: Some(fetched_at), scope: Some(scope), compatibility_error: None })
                }
                ManagedSettingsRequestResult::NoSettings => {
                    let fetched_at = now();
                    this.resolve_managed_settings_freshness_after_success(None, &scope, last_attempt_at, fetched_at);
                    Ok(IManagedSettingsResult { data: Some(IManagedSettingsPatch::of(None)), fetched_at: Some(fetched_at), scope: Some(scope), compatibility_error: None })
                }
                ManagedSettingsRequestResult::UpdateRequired { error } => {
                    if let Some(source) = requirement_source {
                        this.set_managed_settings_freshness(to_blocked_managed_settings_freshness(source, ManagedSettingsFreshnessFailure::UpdateRequired, last_attempt_at, Some(scope.clone())));
                    }
                    Ok(IManagedSettingsResult {
                        data: Some(IManagedSettingsPatch::of(if requirement_source.is_some() { scoped_managed_settings } else { None })),
                        fetched_at: if requirement_source.is_some() { scoped_managed_settings_fetched_at } else { Some(now()) },
                        scope: Some(scope),
                        compatibility_error: Some(error),
                    })
                }
                ManagedSettingsRequestResult::Failed(failure) => {
                    if let Some(source) = requirement_source {
                        this.set_managed_settings_freshness(to_blocked_managed_settings_freshness(source, failure, last_attempt_at, Some(scope.clone())));
                        return Ok(IManagedSettingsResult {
                            data: Some(IManagedSettingsPatch::of(scoped_managed_settings)),
                            fetched_at: scoped_managed_settings_fetched_at,
                            scope: Some(scope),
                            compatibility_error: this.managed_settings_compatibility_error.borrow().clone(),
                        });
                    }
                    // A failed fetch (including a 401/403, which may be transient) is not evidence that policy was
                    // withdrawn: keep the last successful response even once stale, with its original timestamp so the
                    // failure does not count as a refresh and it is still refetched.
                    let compatibility_error = this.managed_settings_compatibility_error.borrow().clone();
                    let retained = if compatibility_error.is_some() { None } else { Some((scoped_managed_settings, scoped_managed_settings_fetched_at)) };
                    Ok(IManagedSettingsResult {
                        data: Some(IManagedSettingsPatch::of(retained.as_ref().and_then(|(managed_settings, _)| managed_settings.clone()))),
                        fetched_at: retained.and_then(|(_, fetched_at)| fetched_at),
                        scope: Some(scope),
                        compatibility_error,
                    })
                }
            }
        })
    }

    fn resolve_managed_settings_freshness_after_success(&self, server: Option<ManagedSettingsData>, scope: &IManagedSettingsFreshnessScope, last_attempt_at: u64, satisfied_at: u64) {
        let fresh_requirement = resolve_force_remote_settings_refresh(
            Some(&self.native_managed_settings_service.managed_settings()),
            server.as_ref(),
            Some(&self.file_managed_settings_service.managed_settings()),
        );
        self.set_managed_settings_freshness(match requirement_channel(fresh_requirement) {
            Some(source) => IManagedSettingsFreshness::Satisfied { source, scope: scope.clone(), last_attempt_at: Some(last_attempt_at), satisfied_at },
            None => IManagedSettingsFreshness::NotRequired,
        });
    }

    async fn request_managed_settings(
        &self,
        sessions: Vec<AuthenticationSession>,
        managed_settings_url: &str,
        session_context: &IAccountSessionContext,
    ) -> Result<ManagedSettingsRequestResult, Error> {
        let request_url = append_managed_settings_client_identity(managed_settings_url, self.product_service.version(), self.product_service.copilot_versions());
        self.log_service.debug_args("[DefaultAccount] Fetching managed settings from:", &[LogArg::from(request_url.as_str())]);
        let rate_limit_backoff_active = now() < self.rate_limit_backoff_until.get();
        let response = self
            .request(
                &request_url,
                RequestMethod::Get,
                None,
                sessions,
                CancellationToken::None,
                "defaultAccount.managedSettings",
                IAuthenticatedRequestOptions {
                    request_timeout_ms: Some(MANAGED_SETTINGS_REQUEST_TIMEOUT_MS),
                    retry_not_found: Some(false),
                    session_context: Some(session_context.clone()),
                },
            )
            .await?;
        let Some(response) = response else {
            self.log_service.debug("[DefaultAccount] Managed settings fetch returned no response (network error, all selected sessions rejected, or active rate-limit backoff); falling back to local-only policy");
            self.report_managed_settings_outcome(ManagedSettingsFetchStatus::NoResponse, rate_limit_backoff_active);
            return Ok(ManagedSettingsRequestResult::Failed(if rate_limit_backoff_active { ManagedSettingsFreshnessFailure::RateLimited } else { ManagedSettingsFreshnessFailure::Network }));
        };

        let status = u32::from(response.res.status_code.unwrap_or(0));
        if status == 404 {
            self.report_managed_settings_outcome(ManagedSettingsFetchStatus::Http(status), rate_limit_backoff_active);
            return Ok(ManagedSettingsRequestResult::NoSettings);
        }
        if status == 466 {
            let error = self.read_managed_settings_compatibility_error(&response).await;
            self.set_managed_settings_compatibility_error(Some(error.clone()));
            self.report_managed_settings_outcome(ManagedSettingsFetchStatus::Http(status), rate_limit_backoff_active);
            return Ok(ManagedSettingsRequestResult::UpdateRequired { error });
        }
        if is_rate_limited(&response) {
            self.report_managed_settings_outcome(ManagedSettingsFetchStatus::Http(status), rate_limit_backoff_active);
            return Ok(ManagedSettingsRequestResult::Failed(ManagedSettingsFreshnessFailure::RateLimited));
        }

        if !is_success(&response) {
            self.log_service.warn(&format!("[DefaultAccount] Managed settings fetch returned non-success status {status}; falling back to local-only policy"));
            self.report_managed_settings_outcome(ManagedSettingsFetchStatus::Http(status), rate_limit_backoff_active);
            return Ok(ManagedSettingsRequestResult::Failed(ManagedSettingsFreshnessFailure::HttpError { http_status: status }));
        }

        match as_json(&response).await {
            Ok(data) => {
                let data = data.unwrap_or(Dynamic::Null);
                self.log_service.trace_args("[DefaultAccount] Managed settings raw response:", &[LogArg::from(json_stringify(&data).unwrap_or_else(|| "null".to_owned()).as_str())]);
                *self.managed_settings_raw_response.borrow_mut() = data.clone();
                let response_object = if data.is_nullish() { Dynamic::object(Vec::<(&str, Dynamic)>::new()) } else { data };
                let log_service = self.log_service.clone();
                let on_warn = move |message: &str| log_service.warn(message);
                let adapted = adapt_managed_settings(&response_object, Some(&on_warn));
                // An empty response (`{}`) is a successful "no policy file present" signal.
                let managed_settings_count = adapted.managed_settings.as_ref().map_or(0, IndexMap::len);
                if managed_settings_count == 0 && adapted.managed_settings_active != Some(true) {
                    self.log_service.debug("[DefaultAccount] Managed settings fetched (empty response, no enterprise policy file present)");
                } else {
                    self.log_service.info("[DefaultAccount] Managed settings applied");
                    self.log_service.trace_args("[DefaultAccount] Managed settings payload:", &[LogArg::from(serde_json::to_string(&adapted).unwrap_or_default().as_str())]);
                }
                self.report_managed_settings_outcome(ManagedSettingsFetchStatus::Ok, rate_limit_backoff_active);
                Ok(ManagedSettingsRequestResult::Success {
                    data: IManagedSettingsPatch { managed_settings: adapted.managed_settings, managed_settings_active: adapted.managed_settings_active },
                })
            }
            Err(error) => {
                self.log_service.error_args(&LogMessage::from("[DefaultAccount] Failed to parse managed settings response"), &[LogArg::from(error_message(&error).as_str())]);
                self.report_managed_settings_outcome(ManagedSettingsFetchStatus::ParseError, rate_limit_backoff_active);
                Ok(ManagedSettingsRequestResult::Failed(ManagedSettingsFreshnessFailure::Malformed))
            }
        }
    }

    fn can_request_managed_settings(&self, options: Option<IDefaultAccountRefreshOptions>, scope: Option<&IManagedSettingsFreshnessScope>) -> bool {
        options.and_then(|options| options.retry_managed_settings) == Some(true)
            || scope.is_none_or(|scope| !self.failed_managed_settings_freshness.borrow().contains_key(&managed_settings_scope_key(scope)))
    }

    fn update_failed_managed_settings_freshness(&self, scope: &IManagedSettingsFreshnessScope, source: ManagedSettingsChannel, result: &ManagedSettingsRequestResult, last_attempt_at: u64) {
        let scope_key = managed_settings_scope_key(scope);
        let mut failed = self.failed_managed_settings_freshness.borrow_mut();
        match result {
            ManagedSettingsRequestResult::Success { .. } | ManagedSettingsRequestResult::NoSettings => {
                failed.remove(&scope_key);
            }
            ManagedSettingsRequestResult::UpdateRequired { .. } => {
                failed.insert(scope_key, to_blocked_managed_settings_freshness(source, ManagedSettingsFreshnessFailure::UpdateRequired, last_attempt_at, Some(scope.clone())));
            }
            ManagedSettingsRequestResult::Failed(failure) => {
                failed.insert(scope_key, to_blocked_managed_settings_freshness(source, *failure, last_attempt_at, Some(scope.clone())));
            }
        }
    }

    async fn read_managed_settings_compatibility_error(&self, response: &IRequestContext) -> IManagedSettingsCompatibilityError {
        let parsed: Result<Option<IManagedSettingsCompatibilityError>, Error> = async {
            let text = as_text(response).await?;
            let body = match text.filter(|text| !text.is_empty()) {
                Some(text) => parse_json_text(&text)?,
                None => Dynamic::Undefined,
            };
            Ok(parse_managed_settings_compatibility_error(&body))
        }
        .await;
        match parsed {
            Ok(Some(error)) => return error,
            Ok(None) => self.log_service.error("[DefaultAccount] Managed settings compatibility response did not contain the expected error code"),
            Err(error) => self.log_service.error_args(
                &LogMessage::from("[DefaultAccount] Failed to parse managed settings compatibility response"),
                &[LogArg::from(error_message(&error).as_str())],
            ),
        }
        IManagedSettingsCompatibilityError { error_code: MANAGED_SETTINGS_UPDATE_REQUIRED_ERROR_CODE, client_version: None, minimum_client_version: None }
    }

    fn report_managed_settings_outcome(&self, status: ManagedSettingsFetchStatus, rate_limit_backoff_active: bool) {
        self.managed_settings_fetch_status.set(Some(status));
        let outcome = match status {
            ManagedSettingsFetchStatus::Http(code) => format!("status:{code}"),
            ManagedSettingsFetchStatus::Ok => "ok".to_owned(),
            ManagedSettingsFetchStatus::NoUrl => "no-url".to_owned(),
            ManagedSettingsFetchStatus::NoResponse => "no-response".to_owned(),
            ManagedSettingsFetchStatus::ParseError => "parse-error".to_owned(),
        };
        self.telemetry_service.public_log2(
            "defaultaccount:managedSettings:fetch",
            Some(Dynamic::object([("outcome", Dynamic::string(&outcome)), ("rateLimitBackoffActive", Dynamic::Bool(rate_limit_backoff_active))])),
        );
    }

    /// `request(url, type, body, sessions, token, callSite, options?)`: tenta cada sessão que vale para o contexto até
    /// uma resposta que não seja 401 (nem 404, quando `retryNotFound` não é falso); devolve a última resposta recusada
    /// quando todas recusam.
    #[allow(clippy::too_many_arguments)]
    async fn request(
        &self,
        url: &str,
        method: RequestMethod,
        body: Option<Dynamic>,
        sessions: Vec<AuthenticationSession>,
        token: CancellationToken,
        call_site: &str,
        options: IAuthenticatedRequestOptions,
    ) -> Result<Option<IRequestContext>, Error> {
        if now() < self.rate_limit_backoff_until.get() {
            let remaining_sec = self.rate_limit_backoff_until.get().saturating_sub(now()).div_ceil(1000);
            self.log_service.debug(&format!("[DefaultAccount] Skipping request to {url}, rate-limit backoff active for {remaining_sec}s more"));
            return Ok(None);
        }

        let mut last_response: Option<IRequestContext> = None;

        let session_context = options.session_context.clone().unwrap_or_else(|| self.get_account_session_context(sessions.first(), &self.get_default_account_authentication_provider()));
        for session in self.get_sessions_for_context(&sessions, &session_context) {
            if token.is_cancellation_requested() {
                return Ok(last_response);
            }

            let mut headers = IHeaders::new();
            headers.insert("Authorization".to_owned(), HeaderValue::One(format!("Bearer {}", session.access_token)));
            let request_options = IRequestOptions {
                r#type: Some(method.as_str().to_owned()),
                url: Some(url.to_owned()),
                data: if method == RequestMethod::Post { body.as_ref().and_then(json_stringify) } else { None },
                disable_cache: Some(true),
                timeout: options.request_timeout_ms,
                headers: Some(headers),
                call_site: call_site.to_owned(),
                ..Default::default()
            };
            match self.request_service.request(request_options, token.clone()).await {
                Ok(response) => {
                    let status = response.res.status_code;
                    if is_rate_limited(&response) {
                        let retry_after_sec = retry_after_from_headers(Some(&response.res.headers)).unwrap_or(60);
                        self.rate_limit_backoff_until.set(now() + u64::try_from(retry_after_sec).unwrap_or(60) * 1000);
                        self.log_service.warn(&format!(
                            "[DefaultAccount] Rate limited by {url} (status {}); backing off for {retry_after_sec}s",
                            status.map_or_else(|| "undefined".to_owned(), |status| status.to_string())
                        ));
                        return Ok(Some(response));
                    }
                    if status == Some(401) || (status == Some(404) && options.retry_not_found != Some(false)) {
                        self.log_service.debug_args(
                            &format!(
                                "[DefaultAccount] Received {} for URL {url} with session {}, likely due to expired/revoked token or insufficient permissions.",
                                status.map_or_else(|| "undefined".to_owned(), |status| status.to_string()),
                                session.id
                            ),
                            &[LogArg::from("Trying next session if available.")],
                        );
                        last_response = Some(response);
                        continue; // try next session
                    }

                    return Ok(Some(response));
                }
                Err(error) => {
                    if !token.is_cancellation_requested() {
                        self.log_service.error_args(&LogMessage::from(format!("[DefaultAccount] request: error {error}").as_str()), &[LogArg::from(url)]);
                    }
                }
            }
        }

        if last_response.is_none() {
            self.log_service.trace_args("[DefaultAccount]: No response received for request", &[LogArg::from(url)]);
        }
        Ok(last_response)
    }

    fn is_data_stale(&self, fetched_at: u64) -> bool {
        now().saturating_sub(fetched_at) >= ACCOUNT_DATA_POLL_INTERVAL_MS
    }

    fn get_entitlement_url(&self, session_context: &IAccountSessionContext) -> Option<String> {
        self.get_account_url("/copilot_internal/user", &self.default_account_config.entitlement_url, session_context)
    }

    fn get_token_entitlement_url(&self, session_context: &IAccountSessionContext) -> Option<String> {
        self.get_account_url("/copilot_internal/v2/token", &self.default_account_config.token_entitlement_url, session_context)
    }

    fn get_mcp_registry_data_url(&self, session_context: &IAccountSessionContext) -> Option<String> {
        self.get_account_url("/copilot/mcp_registry", &self.default_account_config.mcp_registry_data_url, session_context)
    }

    fn get_managed_settings_url(&self, session_context: &IAccountSessionContext) -> Option<String> {
        self.get_account_url("/copilot_internal/managed_settings", &self.default_account_config.managed_settings_url, session_context)
    }

    /// `getAccountUrl(path, defaultUrl, sessionContext)`. Um endereço vazio é falso no original (`!url`), então vale
    /// como ausente.
    fn get_account_url(&self, path: &str, default_url: &str, session_context: &IAccountSessionContext) -> Option<String> {
        if !session_context.authentication_provider.enterprise {
            return Some(default_url.to_owned()).filter(|url| !url.is_empty());
        }
        let enterprise_uri = session_context.enterprise_uri.as_ref()?;
        let enterprise_url = new_url(&enterprise_uri.to_string(false));
        let origin = URI::parse(enterprise_url.origin().ascii_serialization(), false);
        Some(
            origin
                .with(&UriChange { authority: Field::Set(JsString::from(format!("api.{}", host(&enterprise_url).to_string_lossy()))), path: Field::Set(JsString::from(path)), ..UriChange::default() })
                .to_string(false)
                .to_string_lossy(),
        )
    }

    /// `getAccountSessionContext(session, authenticationProvider)`.
    fn get_account_session_context(&self, session: Option<&AuthenticationSession>, authentication_provider: &IDefaultAccountAuthenticationProvider) -> IAccountSessionContext {
        IAccountSessionContext {
            authentication_provider: authentication_provider.clone(),
            enterprise_uri: if authentication_provider.enterprise { get_github_enterprise_uri(session.and_then(|session| session.authorization_server.as_ref())) } else { None },
        }
    }

    fn get_sessions_for_context(&self, sessions: &[AuthenticationSession], session_context: &IAccountSessionContext) -> Vec<AuthenticationSession> {
        if !session_context.authentication_provider.enterprise {
            return sessions.to_vec();
        }
        let Some(enterprise_uri) = &session_context.enterprise_uri else {
            return Vec::new();
        };
        sessions
            .iter()
            .filter(|session| ext_uri().is_equal(self.get_account_session_context(Some(*session), &session_context.authentication_provider).enterprise_uri.as_ref(), Some(enterprise_uri), false))
            .cloned()
            .collect()
    }
}

impl IDefaultAccountProvider for DefaultAccountProvider {
    fn default_account(&self) -> Option<IDefaultAccount> {
        self.default_account.borrow().as_ref().map(|data| data.default_account.clone())
    }

    fn on_did_change_default_account(&self) -> Event<Option<IDefaultAccount>> {
        self.on_did_change_default_account.event()
    }

    fn policy_data(&self) -> Option<IPolicyData> {
        self.policy_data.borrow().as_ref().map(|data| data.policy_data.clone())
    }

    fn on_did_change_policy_data(&self) -> Event<Option<IPolicyData>> {
        self.on_did_change_policy_data.event()
    }

    fn copilot_token_info(&self) -> Option<ICopilotTokenInfo> {
        self.copilot_token_info.borrow().clone()
    }

    fn on_did_change_copilot_token_info(&self) -> Event<Option<ICopilotTokenInfo>> {
        self.on_did_change_copilot_token_info.event()
    }

    fn managed_settings_fetch_status(&self) -> Option<ManagedSettingsFetchStatus> {
        self.managed_settings_fetch_status.get()
    }

    fn managed_settings_fetched_at(&self) -> Option<f64> {
        self.policy_data.borrow().as_ref().and_then(|data| data.managed_settings_fetched_at).map(|fetched_at| fetched_at as f64)
    }

    fn managed_settings_raw_response(&self) -> Dynamic {
        self.managed_settings_raw_response.borrow().clone()
    }

    fn managed_settings_compatibility_error(&self) -> Option<IManagedSettingsCompatibilityError> {
        self.managed_settings_compatibility_error.borrow().clone()
    }

    fn on_did_change_managed_settings_compatibility_error(&self) -> Event<Option<IManagedSettingsCompatibilityError>> {
        self.on_did_change_managed_settings_compatibility_error.event()
    }

    fn managed_settings_freshness(&self) -> IManagedSettingsFreshness {
        self.managed_settings_freshness.borrow().clone()
    }

    fn on_did_change_managed_settings_freshness(&self) -> Event<IManagedSettingsFreshness> {
        self.on_did_change_managed_settings_freshness.event()
    }

    fn get_default_account_authentication_provider(&self) -> IDefaultAccountAuthenticationProvider {
        let provider = &self.default_account_config.authentication_provider;
        let configured = self.configuration_service.get_value(Some(&provider.enterprise_provider_config), None);
        let (identity, enterprise) = match configured {
            Dynamic::String(id) if id.to_string_lossy() == provider.enterprise.id => (&provider.enterprise, true),
            _ => (&provider.default, false),
        };
        IDefaultAccountAuthenticationProvider { id: identity.id.clone(), name: identity.name.clone(), enterprise, extra: Default::default() }
    }

    fn resolve_git_hub_url(&self, path: &str) -> Option<String> {
        let authentication_provider = self
            .default_account
            .borrow()
            .as_ref()
            .map(|data| data.default_account.authentication_provider.clone())
            .unwrap_or_else(|| self.get_default_account_authentication_provider());
        if authentication_provider.enterprise {
            let enterprise_uri = self.default_account.borrow().as_ref().and_then(|data| data.enterprise_uri.clone())?;
            let enterprise_url = new_url(&enterprise_uri.to_string(false));
            return Some(format!("{}://{}/{path}", enterprise_url.scheme(), host(&enterprise_url).to_string_lossy()));
        }

        Some(format!("https://github.com/{path}"))
    }

    fn refresh(&self, options: Option<IDefaultAccountRefreshOptions>) -> Promise<Option<IDefaultAccount>> {
        let this = self.rc();
        async_fn(async move {
            if !this.initialized.get() {
                let init_promise = this.init_promise.borrow().clone();
                if let Some(init_promise) = init_promise {
                    init_promise.await?;
                }
                return Ok(this.default_account());
            }

            this.log_service.debug("[DefaultAccount] Refreshing default account");

            this.update_default_account(options).await?;
            Ok(this.default_account())
        })
    }

    fn sign_in(&self, options: Option<ISignInOptions>) -> Promise<Option<IDefaultAccount>> {
        let this = self.rc();
        async_fn(async move {
            let auth_provider = this.get_default_account_authentication_provider();
            let ISignInOptions { additional_scopes, session_options } = options.unwrap_or_default();
            let default_account_scopes = this
                .default_account_config
                .authentication_provider
                .scopes
                .first()
                .cloned()
                .ok_or_else(|| Error::type_error("Cannot read properties of undefined (reading 'length')"))?;
            let scopes = match additional_scopes {
                Some(additional_scopes) => distinct(&[default_account_scopes, additional_scopes].concat(), |scope| scope.clone()),
                None => default_account_scopes,
            };
            let session = this
                .authentication_service
                .create_session(
                    &auth_provider.id,
                    AuthenticationScopeListOrRequest::Scopes(scopes),
                    Some(IAuthenticationCreateSessionOptions {
                        session_options: IAuthenticationProviderSessionOptions {
                            extra: Some(session_options.unwrap_or_else(|| Dynamic::object(Vec::<(&str, Dynamic)>::new()))),
                            ..Default::default()
                        },
                        activate_immediate: None,
                    }),
                )
                .await?;
            for preferred_extension in &this.default_account_config.preferred_extensions {
                this.authentication_extensions_service.update_account_preference(preferred_extension, &auth_provider.id, &session.account);
            }
            this.update_default_account(None).await?;
            Ok(this.default_account())
        })
    }

    fn sign_out(&self) -> Promise<()> {
        let this = self.rc();
        async_fn(async move {
            let Some(account) = this.default_account() else {
                return Ok(());
            };
            this.command_service
                .execute_command(
                    "_signOutOfAccount",
                    vec![Dynamic::object([("providerId", Dynamic::string(&account.authentication_provider.id)), ("accountLabel", Dynamic::string(&account.account_name))])],
                )
                .await?;
            Ok(())
        })
    }
}

impl IDisposable for DefaultAccountProvider {
    fn dispose(&self) {
        self.base.dispose();
    }
}

/// `DefaultAccountProviderContribution`: cria o provedor e o entrega ao `IDefaultAccountService`.
pub struct DefaultAccountProviderContribution {
    base: Rc<Disposable>,
}

impl DefaultAccountProviderContribution {
    pub const ID: &'static str = "workbench.contributions.defaultAccountProvider";

    /// `new DefaultAccountProviderContribution(productService, instantiationService, defaultAccountService)`: o
    /// `productService.defaultChatAgent` é o argumento `default_chat_agent` (o `IProductService` do porte só expõe os
    /// identificadores das extensões do agente). Falha quando o serviço já tem um provedor.
    pub fn new(default_chat_agent: &IDefaultChatAgent, services: DefaultAccountProviderServices, default_account_service: &dyn IDefaultAccountService) -> Result<Rc<Self>, Error> {
        let base = Disposable::new();
        let default_account_provider = DefaultAccountProvider::new(to_default_account_config(default_chat_agent), services);
        base.register(default_account_provider.clone());
        default_account_service.set_default_account_provider(default_account_provider)?;
        Ok(Rc::new(Self { base }))
    }
}

impl IDisposable for DefaultAccountProviderContribution {
    fn dispose(&self) {
        self.base.dispose();
    }
}

/// A ação `workbench.actions.accounts.signIn`.
pub struct DefaultAccountSignInAction;

impl Action2 for DefaultAccountSignInAction {
    fn desc(&self) -> Action2Options {
        let title = localize2("signIn", "Sign In");
        Action2Options {
            command: CommandAction::new(DEFAULT_ACCOUNT_SIGN_IN_COMMAND, Title::Localized(CommandActionTitle { value: title.value, original: title.original, mnemonic_title: None })),
            menu: Vec::new(),
            keybinding: Vec::new(),
            f1: false,
        }
    }

    fn run(&self, accessor: &ServicesAccessor, _args: &[Dynamic]) {
        let default_account_service = accessor.get(I_DEFAULT_ACCOUNT_SERVICE);
        let _ = default_account_service.sign_in(None);
    }
}

/// `registerAction2(class extends Action2 { ... })` de `defaultAccount.ts`.
pub fn register_default_account_actions() -> DisposableRef {
    register_action2(Rc::new(DefaultAccountSignInAction))
}
