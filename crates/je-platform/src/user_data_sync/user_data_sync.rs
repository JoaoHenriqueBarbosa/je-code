//! Porte de `src/vs/platform/userDataSync/common/userDataSync.ts`: os tipos, os erros e os contratos de serviço
//! da sincronização de dados do usuário.
//!
//! Fora do porte (e por quê): `registerConfiguration` (pede o registro de contribuições JSON, `jsonContributionRegistry`,
//! ainda não portado), `getIgnoredSettingsForExtension` (pede o `IExtensionManifest` do `je-extensions`, que o `je-platform`
//! não conhece), `ISyncExtension`/`ILocalSyncExtension`/`IRemoteSyncExtension` (pedem `IExtensionIdentifier` de
//! `extensionManagement`) e `IUserDataSyncResourceProviderService` (pede `IUserDataSyncMachine`, de `userDataSyncMachines.ts`).
//!
//! Divergências de tipo:
//! - `IStringDictionary<string>` é `HashMap<JsString, JsString>`; os métodos `async` devolvem [`Promise`], que se espera
//!   com `.await`; `resolveFormattingOptions` recebe `Option<&URI>` porque a implementação de teste do original declara
//!   o parâmetro como opcional (`file?: URI`); `any | undefined` (em `IConflictSetting`) é `Option<Value>`.
//! - `Date` (`donotMakeRequestsUntil`, `cTime`) é `f64` de milissegundos desde a época, como o `getTime()`.
//! - `string | null | undefined` (o `content` de `accept`) é `Option<Option<String>>`: `None` é `undefined` e
//!   `Some(None)` é `null`.
//! - `ref` é o identificador `r#ref`.
//! - `UserDataSyncError` e as subclasses são valores ([`UserDataSyncError`]) que viram um [`Error`] pelo `name`
//!   que o original compõe (`<código> (UserDataSyncError) syncResource:<recurso> operationId:<id>`); o caminho de volta
//!   ([`UserDataSyncError::from_error`]) lê esse `name`, como o `toUserDataSyncError` do original.
//! - `Record<ServerResource, string>` e `IStringDictionary<IUserData>` são `IndexMap`, na ordem em que o original escreve.

use std::collections::HashMap;
use std::rc::Rc;

use indexmap::IndexMap;
use je_base::JsString;
use je_base::arrays::distinct;
use je_base::buffer::VSBufferReadableStream;
use je_base::dynamic::{Dynamic, Truthy};
use je_base::errors::Error;
use je_base::event::Event;
use je_base::json::Value;
use je_base::json_formatter::FormattingOptions;
use je_base::parts::request::IHeaders;
use je_base::promise::Promise;
use je_base::resources::ExtUri;
use je_base::uri::URI;
use je_instantiation::service_id;

use crate::configuration::registry::{ConfigurationScope, PropertySchema, configuration_registry};
use crate::environment::environment::EnvironmentService;
use crate::log::LogServiceRef;
use crate::storage::StorageScope;
use crate::user_data_profile::{UseDefaultProfileFlags, UserDataProfile};

use super::global_state_merge::StorageDictionary;

/// `getDisallowedIgnoredSettings`: as chaves registradas que não podem ser ignoradas na sincronização
/// (`disallowSyncIgnore`).
pub fn get_disallowed_ignored_settings() -> Vec<JsString> {
    configuration_registry()
        .get_configuration_properties()
        .entries()
        .into_iter()
        .filter(|(_, property)| property.get("disallowSyncIgnore").is_truthy())
        .map(|(setting, _)| setting)
        .collect()
}

/// `getDefaultIgnoredSettings(excludeExtensions)`.
pub fn get_default_ignored_settings(exclude_extensions: bool) -> Vec<JsString> {
    let all_settings = configuration_registry().get_configuration_properties().entries();
    let ignored_settings = get_ignored_settings(&all_settings, exclude_extensions);
    let disallowed_settings = get_disallowed_ignored_settings();
    let mut all = ignored_settings;
    all.extend(disallowed_settings);
    distinct(&all, |setting| setting.clone())
}

/// `getIgnoredSettings`: as chaves com `ignoreSync` ou de escopo `MACHINE`/`MACHINE_OVERRIDABLE`. Com
/// `exclude_extensions`, as que têm `source` (vieram de uma extensão) ficam de fora.
pub fn get_ignored_settings(properties: &[(JsString, PropertySchema)], exclude_extensions: bool) -> Vec<JsString> {
    let mut ignored_settings: Vec<JsString> = Vec::new();
    for (key, property) in properties {
        if exclude_extensions && property.source().is_some() {
            continue;
        }
        let scope = property.scope();
        if (property.get("ignoreSync").is_truthy() || scope == Some(ConfigurationScope::Machine) || scope == Some(ConfigurationScope::MachineOverridable))
            && !ignored_settings.contains(key)
        {
            ignored_settings.push(key.clone());
        }
    }
    ignored_settings
}

pub const USER_DATA_SYNC_CONFIGURATION_SCOPE: &str = "settingsSync";

/// `IUserDataSyncConfiguration`: o que o usuário configura em `settingsSync.*`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct IUserDataSyncConfiguration {
    pub keybindings_per_platform: Option<bool>,
    pub ignored_extensions: Option<Vec<String>>,
    pub ignored_settings: Option<Vec<String>>,
}

impl IUserDataSyncConfiguration {
    /// A seção `settingsSync` lida do serviço de configuração (`getValue(USER_DATA_SYNC_CONFIGURATION_SCOPE)`).
    pub fn from_dynamic(value: &Dynamic) -> Self {
        if !matches!(value, Dynamic::Object(_)) {
            return Self::default();
        }
        let strings = |name: &str| match value.get_str(name) {
            Dynamic::Array(items) => Some(
                items
                    .to_vec()
                    .iter()
                    .filter_map(|item| match item {
                        Dynamic::String(text) => Some(text.to_string_lossy()),
                        _ => None,
                    })
                    .collect(),
            ),
            _ => None,
        };
        Self {
            keybindings_per_platform: match value.get_str("keybindingsPerPlatform") {
                Dynamic::Bool(flag) => Some(flag),
                _ => None,
            },
            ignored_extensions: strings("ignoredExtensions"),
            ignored_settings: strings("ignoredSettings"),
        }
    }
}

pub const CONFIG_SYNC_KEYBINDINGS_PER_PLATFORM: &str = "settingsSync.keybindingsPerPlatform";

// region: User Data Sync Store

pub const NON_EXISTING_RESOURCE_REF: &str = "0";

/// `IUserData`: a versão (`ref`) e o conteúdo de um recurso no servidor; `None` é o `null` (não existe).
#[derive(Clone, Debug, PartialEq)]
pub struct IUserData {
    pub r#ref: String,
    pub content: Option<String>,
}

/// `IAuthenticationProvider`.
#[derive(Clone, Debug, PartialEq)]
pub struct IAuthenticationProvider {
    pub id: String,
    pub scopes: Vec<String>,
}

/// `UserDataSyncStoreType`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UserDataSyncStoreType {
    Insiders,
    Stable,
}

impl UserDataSyncStoreType {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Insiders => "insiders",
            Self::Stable => "stable",
        }
    }
}

/// `IUserDataSyncStore`.
#[derive(Clone, Debug, PartialEq)]
pub struct IUserDataSyncStore {
    pub url: URI,
    pub r#type: UserDataSyncStoreType,
    pub default_url: URI,
    pub stable_url: URI,
    pub insiders_url: URI,
    pub can_switch: bool,
    pub authentication_providers: Vec<IAuthenticationProvider>,
}

/// `isAuthenticationProvider`: um objeto com `id` texto e `scopes` array.
pub fn is_authentication_provider(thing: &Value) -> bool {
    match thing {
        Value::Object(object) => matches!(object.get("id"), Some(Value::String(_))) && matches!(object.get("scopes"), Some(Value::Array(_))),
        _ => false,
    }
}

/// `SyncResource` (os valores de texto são os do original).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SyncResource {
    Settings,
    Keybindings,
    Snippets,
    Prompts,
    Tasks,
    Mcp,
    Extensions,
    GlobalState,
    Profiles,
    WorkspaceState,
}

impl SyncResource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Settings => "settings",
            Self::Keybindings => "keybindings",
            Self::Snippets => "snippets",
            Self::Prompts => "prompts",
            Self::Tasks => "tasks",
            Self::Mcp => "mcp",
            Self::Extensions => "extensions",
            Self::GlobalState => "globalState",
            Self::Profiles => "profiles",
            Self::WorkspaceState => "workspaceState",
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        [
            Self::Settings,
            Self::Keybindings,
            Self::Snippets,
            Self::Prompts,
            Self::Tasks,
            Self::Mcp,
            Self::Extensions,
            Self::GlobalState,
            Self::Profiles,
            Self::WorkspaceState,
        ]
        .into_iter()
        .find(|resource| resource.as_str() == text)
    }
}

/// `ALL_SYNC_RESOURCES`, na ordem do original.
pub const ALL_SYNC_RESOURCES: [SyncResource; 9] = [
    SyncResource::Settings,
    SyncResource::Keybindings,
    SyncResource::Snippets,
    SyncResource::Prompts,
    SyncResource::Tasks,
    SyncResource::Extensions,
    SyncResource::GlobalState,
    SyncResource::Profiles,
    SyncResource::Mcp,
];

/// `getPathSegments(collection, ...paths)`: a coleção (quando não é vazia) e os caminhos.
pub fn get_path_segments(collection: Option<&str>, paths: &[&str]) -> Vec<String> {
    let mut segments: Vec<String> = collection.filter(|collection| !collection.is_empty()).map(str::to_string).into_iter().collect();
    segments.extend(paths.iter().map(|path| path.to_string()));
    segments
}

/// `getLastSyncResourceUri`.
pub fn get_last_sync_resource_uri(collection: Option<&str>, sync_resource: SyncResource, environment_service: &dyn EnvironmentService, ext_uri: &ExtUri) -> URI {
    let last_sync_file = format!("lastSync{}.json", sync_resource.as_str());
    let segments = get_path_segments(collection, &[sync_resource.as_str(), &last_sync_file]);
    ext_uri.join_path(&environment_service.user_data_sync_home(), &segments.iter().map(String::as_str).collect::<Vec<_>>())
}

/// `ServerResource`: `SyncResource | 'machines' | 'editSessions' | 'workspaceState'` (`workspaceState` já é um
/// [`SyncResource`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ServerResource {
    Sync(SyncResource),
    Machines,
    EditSessions,
}

impl ServerResource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Sync(resource) => resource.as_str(),
            Self::Machines => "machines",
            Self::EditSessions => "editSessions",
        }
    }
}

impl From<SyncResource> for ServerResource {
    fn from(resource: SyncResource) -> Self {
        Self::Sync(resource)
    }
}

/// `IUserDataResourceManifest` (`Record<ServerResource, string>`): o `ref` mais recente de cada recurso.
pub type IUserDataResourceManifest = IndexMap<String, String>;

/// A entrada de `IUserDataCollectionManifest`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct IUserDataCollectionManifestEntry {
    pub latest: Option<IUserDataResourceManifest>,
}

/// `IUserDataCollectionManifest`.
pub type IUserDataCollectionManifest = IndexMap<String, IUserDataCollectionManifestEntry>;

/// `IUserDataManifest`.
#[derive(Clone, Debug, PartialEq)]
pub struct IUserDataManifest {
    pub latest: Option<IUserDataResourceManifest>,
    pub session: String,
    pub r#ref: String,
    pub collections: Option<IUserDataCollectionManifest>,
}

/// `isUserDataManifest`: `session` e `ref` texto, `latest` e `collections` objetos ou ausentes.
pub fn is_user_data_manifest(thing: &Value) -> bool {
    let Value::Object(object) = thing else { return false };
    let is_object_or_absent = |name: &str| matches!(object.get(name), None | Some(Value::Object(_)));
    matches!(object.get("session"), Some(Value::String(_))) && matches!(object.get("ref"), Some(Value::String(_))) && is_object_or_absent("latest") && is_object_or_absent("collections")
}

/// Uma entrada de `IUserDataSyncActivityData`: `{ created, content }`.
#[derive(Clone, Debug, PartialEq)]
pub struct IUserDataSyncActivityEntry {
    pub created: f64,
    pub content: String,
}

/// `{ [resourceId: string]: { created, content }[] }`.
pub type IUserDataSyncActivityResources = IndexMap<String, Vec<IUserDataSyncActivityEntry>>;

/// A entrada de `collections` de `IUserDataSyncActivityData`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct IUserDataSyncActivityCollection {
    pub resources: Option<IUserDataSyncActivityResources>,
}

/// `IUserDataSyncActivityData`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct IUserDataSyncActivityData {
    pub resources: Option<IUserDataSyncActivityResources>,
    pub collections: Option<IndexMap<String, IUserDataSyncActivityCollection>>,
}

/// A entrada de `collections` de `IUserDataSyncLatestData`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct IUserDataSyncLatestCollection {
    pub resources: Option<IndexMap<String, IUserData>>,
}

/// `IUserDataSyncLatestData`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct IUserDataSyncLatestData {
    pub resources: Option<IndexMap<String, IUserData>>,
    pub collections: Option<IndexMap<String, IUserDataSyncLatestCollection>>,
}

/// `IResourceRefHandle`.
#[derive(Clone, Debug, PartialEq)]
pub struct IResourceRefHandle {
    pub r#ref: String,
    pub created: f64,
}

/// `IUserDataSyncStoreManagementService`.
pub trait IUserDataSyncStoreManagementService {
    fn on_did_change_user_data_sync_store(&self) -> Event<()>;
    fn user_data_sync_store(&self) -> Option<IUserDataSyncStore>;
    fn switch(&self, r#type: UserDataSyncStoreType) -> Promise<()>;
    fn get_previous_user_data_sync_store(&self) -> Promise<Option<IUserDataSyncStore>>;
}

service_id!(pub I_USER_DATA_SYNC_STORE_MANAGEMENT_SERVICE: dyn IUserDataSyncStoreManagementService = "IUserDataSyncStoreManagementService");

/// `IUserDataSyncStoreService`. `collection` ausente é `undefined`; `headers` ausentes são `undefined`.
pub trait IUserDataSyncStoreService {
    fn on_did_change_donot_make_requests_until(&self) -> Event<()>;
    fn donot_make_requests_until(&self) -> Option<f64>;

    fn on_token_failed(&self) -> Event<UserDataSyncErrorCode>;
    fn on_token_succeed(&self) -> Event<()>;
    fn set_auth_token(&self, token: &str, r#type: &str);

    fn manifest(&self, old_value: Option<IUserDataManifest>, headers: Option<IHeaders>) -> Promise<Option<IUserDataManifest>>;
    fn read_resource(&self, resource: ServerResource, old_value: Option<IUserData>, collection: Option<&str>, headers: Option<IHeaders>) -> Promise<IUserData>;
    fn write_resource(&self, resource: ServerResource, content: &str, r#ref: Option<&str>, collection: Option<&str>, headers: Option<IHeaders>) -> Promise<String>;
    fn delete_resource(&self, resource: ServerResource, r#ref: Option<&str>, collection: Option<&str>) -> Promise<()>;
    fn get_all_resource_refs(&self, resource: ServerResource, collection: Option<&str>) -> Promise<Vec<IResourceRefHandle>>;
    fn resolve_resource_content(&self, resource: ServerResource, r#ref: &str, collection: Option<&str>, headers: Option<IHeaders>) -> Promise<Option<String>>;

    fn get_all_collections(&self, headers: Option<IHeaders>) -> Promise<Vec<String>>;
    fn create_collection(&self, headers: Option<IHeaders>) -> Promise<String>;
    fn delete_collection(&self, collection: Option<&str>, headers: Option<IHeaders>) -> Promise<()>;

    fn get_latest_data(&self, headers: Option<IHeaders>) -> Promise<Option<IUserDataSyncLatestData>>;
    fn get_activity_data(&self) -> Promise<VSBufferReadableStream>;

    fn clear(&self) -> Promise<()>;
}

service_id!(pub I_USER_DATA_SYNC_STORE_SERVICE: dyn IUserDataSyncStoreService = "IUserDataSyncStoreService");

/// `IUserDataSyncLocalStoreService`: o histórico local dos recursos (o backup antes de aplicar uma mudança).
pub trait IUserDataSyncLocalStoreService {
    fn write_resource(&self, resource: ServerResource, content: &str, c_time: f64, collection: Option<&str>, root: Option<&URI>) -> Promise<()>;
    fn get_all_resource_refs(&self, resource: ServerResource, collection: Option<&str>, root: Option<&URI>) -> Promise<Vec<IResourceRefHandle>>;
    fn resolve_resource_content(&self, resource: ServerResource, r#ref: &str, collection: Option<&str>, root: Option<&URI>) -> Promise<Option<String>>;
}

service_id!(pub I_USER_DATA_SYNC_LOCAL_STORE_SERVICE: dyn IUserDataSyncLocalStoreService = "IUserDataSyncLocalStoreService");

// endregion

// region: User Data Sync Headers

pub const HEADER_OPERATION_ID: &str = "x-operation-id";
pub const HEADER_EXECUTION_ID: &str = "X-Execution-Id";

/// `createSyncHeaders`.
pub fn create_sync_headers(execution_id: &str) -> IHeaders {
    let mut headers = IHeaders::new();
    headers.insert(HEADER_EXECUTION_ID.to_string(), je_base::parts::request::HeaderValue::One(execution_id.to_string()));
    headers
}

// endregion

// region: User Data Sync Error

/// `UserDataSyncErrorCode` (os valores de texto são os do original).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum UserDataSyncErrorCode {
    // Client Errors (>= 400)
    Unauthorized,
    Forbidden,
    NotFound,
    MethodNotFound,
    Conflict,
    Gone,
    PreconditionFailed,
    TooLarge,
    UpgradeRequired,
    PreconditionRequired,
    TooManyRequests,
    TooManyRequestsAndRetryAfter,

    // Local Errors
    RequestFailed,
    RequestCanceled,
    RequestTimeout,
    RequestProtocolNotSupported,
    RequestPathNotEscaped,
    RequestHeadersNotObject,
    NoCollection,
    NoRef,
    EmptyResponse,
    TurnedOff,
    SessionExpired,
    ServiceChanged,
    DefaultServiceChanged,
    LocalTooManyProfiles,
    LocalTooManyRequests,
    LocalPreconditionFailed,
    LocalInvalidContent,
    LocalError,
    IncompatibleLocalContent,
    IncompatibleRemoteContent,

    Unknown,
}

const ALL_ERROR_CODES: [UserDataSyncErrorCode; 33] = [
    UserDataSyncErrorCode::Unauthorized,
    UserDataSyncErrorCode::Forbidden,
    UserDataSyncErrorCode::NotFound,
    UserDataSyncErrorCode::MethodNotFound,
    UserDataSyncErrorCode::Conflict,
    UserDataSyncErrorCode::Gone,
    UserDataSyncErrorCode::PreconditionFailed,
    UserDataSyncErrorCode::TooLarge,
    UserDataSyncErrorCode::UpgradeRequired,
    UserDataSyncErrorCode::PreconditionRequired,
    UserDataSyncErrorCode::TooManyRequests,
    UserDataSyncErrorCode::TooManyRequestsAndRetryAfter,
    UserDataSyncErrorCode::RequestFailed,
    UserDataSyncErrorCode::RequestCanceled,
    UserDataSyncErrorCode::RequestTimeout,
    UserDataSyncErrorCode::RequestProtocolNotSupported,
    UserDataSyncErrorCode::RequestPathNotEscaped,
    UserDataSyncErrorCode::RequestHeadersNotObject,
    UserDataSyncErrorCode::NoCollection,
    UserDataSyncErrorCode::NoRef,
    UserDataSyncErrorCode::EmptyResponse,
    UserDataSyncErrorCode::TurnedOff,
    UserDataSyncErrorCode::SessionExpired,
    UserDataSyncErrorCode::ServiceChanged,
    UserDataSyncErrorCode::DefaultServiceChanged,
    UserDataSyncErrorCode::LocalTooManyProfiles,
    UserDataSyncErrorCode::LocalTooManyRequests,
    UserDataSyncErrorCode::LocalPreconditionFailed,
    UserDataSyncErrorCode::LocalInvalidContent,
    UserDataSyncErrorCode::LocalError,
    UserDataSyncErrorCode::IncompatibleLocalContent,
    UserDataSyncErrorCode::IncompatibleRemoteContent,
    UserDataSyncErrorCode::Unknown,
];

impl UserDataSyncErrorCode {
    /// O valor de texto do `enum` do original.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Unauthorized => "Unauthorized",
            Self::Forbidden => "Forbidden",
            Self::NotFound => "NotFound",
            Self::MethodNotFound => "MethodNotFound",
            Self::Conflict => "Conflict",
            Self::Gone => "Gone",
            Self::PreconditionFailed => "PreconditionFailed",
            Self::TooLarge => "TooLarge",
            Self::UpgradeRequired => "UpgradeRequired",
            Self::PreconditionRequired => "PreconditionRequired",
            Self::TooManyRequests => "RemoteTooManyRequests",
            Self::TooManyRequestsAndRetryAfter => "TooManyRequestsAndRetryAfter",
            Self::RequestFailed => "RequestFailed",
            Self::RequestCanceled => "RequestCanceled",
            Self::RequestTimeout => "RequestTimeout",
            Self::RequestProtocolNotSupported => "RequestProtocolNotSupported",
            Self::RequestPathNotEscaped => "RequestPathNotEscaped",
            Self::RequestHeadersNotObject => "RequestHeadersNotObject",
            Self::NoCollection => "NoCollection",
            Self::NoRef => "NoRef",
            Self::EmptyResponse => "EmptyResponse",
            Self::TurnedOff => "TurnedOff",
            Self::SessionExpired => "SessionExpired",
            Self::ServiceChanged => "ServiceChanged",
            Self::DefaultServiceChanged => "DefaultServiceChanged",
            Self::LocalTooManyProfiles => "LocalTooManyProfiles",
            Self::LocalTooManyRequests => "LocalTooManyRequests",
            Self::LocalPreconditionFailed => "LocalPreconditionFailed",
            Self::LocalInvalidContent => "LocalInvalidContent",
            Self::LocalError => "LocalError",
            Self::IncompatibleLocalContent => "IncompatibleLocalContent",
            Self::IncompatibleRemoteContent => "IncompatibleRemoteContent",
            Self::Unknown => "Unknown",
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        ALL_ERROR_CODES.into_iter().find(|code| code.as_str() == text)
    }
}

const ERROR_NAME_MARKER: &str = " (UserDataSyncError) syncResource:";
const ERROR_NAME_OPERATION_MARKER: &str = " operationId:";

/// `UserDataSyncError`.
#[derive(Clone, Debug, PartialEq)]
pub struct UserDataSyncError {
    pub message: String,
    pub code: UserDataSyncErrorCode,
    pub resource: Option<SyncResource>,
    pub operation_id: Option<String>,
}

impl UserDataSyncError {
    pub fn new(message: impl Into<String>, code: UserDataSyncErrorCode, resource: Option<SyncResource>) -> Self {
        Self { message: message.into(), code, resource, operation_id: None }
    }

    pub fn with_operation_id(mut self, operation_id: Option<String>) -> Self {
        self.operation_id = operation_id;
        self
    }

    /// O `name` do original: `<código> (UserDataSyncError) syncResource:<recurso> operationId:<id>`, com `unknown`
    /// para o que falta.
    pub fn name(&self) -> String {
        format!(
            "{}{ERROR_NAME_MARKER}{}{ERROR_NAME_OPERATION_MARKER}{}",
            self.code.as_str(),
            self.resource.map_or("unknown", SyncResource::as_str),
            self.operation_id.as_deref().filter(|id| !id.is_empty()).unwrap_or("unknown")
        )
    }

    pub fn into_error(self) -> Error {
        Error::named(self.name(), self.message)
    }

    /// O erro lido de volta pelo `name`: `None` se o `name` não tem a forma do original. Um código desconhecido
    /// vira [`UserDataSyncErrorCode::Unknown`] (o original só faz o cast do texto).
    pub fn from_error(error: &Error) -> Option<Self> {
        let marker = error.name.find(ERROR_NAME_MARKER)?;
        let code = &error.name[..marker];
        if code.is_empty() {
            return None;
        }
        let rest = &error.name[marker + ERROR_NAME_MARKER.len()..];
        let operation = rest.rfind(ERROR_NAME_OPERATION_MARKER)?;
        let (resource, operation_id) = (&rest[..operation], &rest[operation + ERROR_NAME_OPERATION_MARKER.len()..]);
        if resource.is_empty() || operation_id.is_empty() {
            return None;
        }
        Some(Self {
            message: error.message.clone(),
            code: UserDataSyncErrorCode::parse(code).unwrap_or(UserDataSyncErrorCode::Unknown),
            resource: if resource == "unknown" { None } else { SyncResource::parse(resource) },
            operation_id: if operation_id == "unknown" { None } else { Some(operation_id.to_string()) },
        })
    }
}

impl From<UserDataSyncError> for Error {
    fn from(error: UserDataSyncError) -> Error {
        error.into_error()
    }
}

/// `UserDataSyncError.toUserDataSyncError(error)`: o próprio erro, o erro que o `name` descreve, ou um
/// [`UserDataSyncErrorCode::Unknown`] com a mensagem.
pub fn to_user_data_sync_error(error: &Error) -> UserDataSyncError {
    UserDataSyncError::from_error(error).unwrap_or_else(|| UserDataSyncError::new(error.message.clone(), UserDataSyncErrorCode::Unknown, None))
}

/// `UserDataSyncStoreError`: o erro do servidor, com a URL e o código que ele devolveu.
#[derive(Clone, Debug, PartialEq)]
pub struct UserDataSyncStoreError {
    pub error: UserDataSyncError,
    pub url: String,
    pub server_code: Option<f64>,
}

impl UserDataSyncStoreError {
    pub fn new(message: impl Into<String>, url: impl Into<String>, code: UserDataSyncErrorCode, server_code: Option<f64>, operation_id: Option<String>) -> Self {
        Self { error: UserDataSyncError::new(message, code, None).with_operation_id(operation_id), url: url.into(), server_code }
    }

    pub fn into_error(self) -> Error {
        let mut error = self.error.into_error().with_prop("url", self.url);
        if let Some(server_code) = self.server_code {
            error = error.with_prop("serverCode", server_code);
        }
        error
    }

    /// `None` se o erro não tem a `url` que o `UserDataSyncStoreError` escreve.
    pub fn from_error(error: &Error) -> Option<Self> {
        let url = error.prop("url")?.to_string();
        Some(Self { error: UserDataSyncError::from_error(error)?, url, server_code: error.prop("serverCode").and_then(|code| code.parse().ok()) })
    }
}

/// `UserDataAutoSyncError`: o erro da sincronização automática, uma [`UserDataSyncError`] sem recurso.
#[derive(Clone, Debug, PartialEq)]
pub struct UserDataAutoSyncError {
    pub error: UserDataSyncError,
}

impl UserDataAutoSyncError {
    const MARKER: &'static str = "userDataAutoSyncError";

    pub fn new(message: impl Into<String>, code: UserDataSyncErrorCode) -> Self {
        Self { error: UserDataSyncError::new(message, code, None) }
    }

    pub fn into_error(self) -> Error {
        self.error.into_error().with_prop(Self::MARKER, "true")
    }

    /// `instanceof UserDataAutoSyncError`.
    pub fn from_error(error: &Error) -> Option<Self> {
        error.prop(Self::MARKER)?;
        Some(Self { error: UserDataSyncError::from_error(error)? })
    }
}

// endregion

// region: User Data Synchroniser

/// `ISyncUserDataProfile`: um perfil como a sincronização o guarda (com a coleção do servidor). `icon` e
/// `useDefaultFlags` ausentes são `None`.
#[derive(Clone, Debug, PartialEq)]
pub struct ISyncUserDataProfile {
    pub id: String,
    pub collection: String,
    pub name: String,
    pub icon: Option<String>,
    pub use_default_flags: Option<UseDefaultProfileFlags>,
}

/// `IStorageValue`.
#[derive(Clone, Debug, PartialEq)]
pub struct IStorageValue {
    pub version: f64,
    pub value: JsString,
    pub scope: Option<StorageScope>,
}

/// `IGlobalState`: `IStringDictionary<IStorageValue>` é o [`StorageDictionary`] do merge (a ordem das chaves decide a
/// ordem do que o merge devolve).
#[derive(Clone, Debug)]
pub struct IGlobalState {
    pub storage: StorageDictionary,
}

/// `IWorkspaceStateFolder`.
#[derive(Clone, Debug, PartialEq)]
pub struct IWorkspaceStateFolder {
    pub resource_uri: String,
    pub workspace_folder_identity: String,
}

/// `IWorkspaceState`.
#[derive(Clone, Debug, PartialEq)]
pub struct IWorkspaceState {
    pub folders: Vec<IWorkspaceStateFolder>,
    pub storage: IndexMap<String, String>,
    pub version: f64,
}

/// `SyncStatus` (os valores de texto são os do original).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SyncStatus {
    Uninitialized,
    Idle,
    Syncing,
    HasConflicts,
}

impl SyncStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Uninitialized => "uninitialized",
            Self::Idle => "idle",
            Self::Syncing => "syncing",
            Self::HasConflicts => "hasConflicts",
        }
    }
}

/// `ISyncResourceHandle`.
#[derive(Clone, Debug, PartialEq)]
pub struct ISyncResourceHandle {
    pub created: f64,
    pub uri: URI,
}

/// `ISyncData`: a versão do formato, a máquina que gravou e o conteúdo do recurso.
#[derive(Clone, Debug, PartialEq)]
pub struct ISyncData {
    pub version: f64,
    pub machine_id: Option<String>,
    pub content: String,
}

/// `IRemoteUserData`: a versão (`ref`) e os dados sincronizados (`None` é o `null`: o recurso não existe).
/// `extras` são as propriedades a mais que o estado da última sincronização guarda e o `getLastSyncUserData`
/// espalha no resultado (`{ ...lastSyncUserDataState, syncData }`): `version` e o que `updateLastSyncUserData`
/// recebe em `additionalProps`, na ordem em que foram gravadas.
#[derive(Clone, Debug, PartialEq)]
pub struct IRemoteUserData {
    pub r#ref: String,
    pub sync_data: Option<ISyncData>,
    pub extras: Vec<(String, Value)>,
}

impl IRemoteUserData {
    /// `{ ref, syncData }`.
    pub fn new(r#ref: impl Into<String>, sync_data: Option<ISyncData>) -> Self {
        Self { r#ref: r#ref.into(), sync_data, extras: Vec::new() }
    }
}

/// `Change`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Change {
    None,
    Added,
    Modified,
    Deleted,
}

/// `MergeState` (os valores de texto são os do original).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MergeState {
    Preview,
    Conflict,
    Accepted,
}

impl MergeState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Preview => "preview",
            Self::Conflict => "conflict",
            Self::Accepted => "accepted",
        }
    }
}

/// `IResourcePreview` (o de `userDataSync.ts`, a visão que o resto do sistema tem do preview).
#[derive(Clone, Debug, PartialEq)]
pub struct IResourcePreview {
    pub base_resource: URI,
    pub remote_resource: URI,
    pub local_resource: URI,
    pub preview_resource: URI,
    pub accepted_resource: URI,
    pub local_change: Change,
    pub remote_change: Change,
    pub merge_state: MergeState,
}

/// `IUserDataSyncResource`.
#[derive(Clone, Debug, PartialEq)]
pub struct IUserDataSyncResource {
    pub sync_resource: SyncResource,
    pub profile: UserDataProfile,
}

/// `IUserDataSyncResourceConflicts` (os campos de `IUserDataSyncResource` mais os conflitos).
#[derive(Clone, Debug, PartialEq)]
pub struct IUserDataSyncResourceConflicts {
    pub sync_resource: SyncResource,
    pub profile: UserDataProfile,
    pub conflicts: Vec<IResourcePreview>,
}

/// `IUserDataSyncResourcePreview` (os campos de `IUserDataSyncResource` mais os previews).
#[derive(Clone, Debug, PartialEq)]
pub struct IUserDataSyncResourcePreview {
    pub sync_resource: SyncResource,
    pub profile: UserDataProfile,
    pub is_last_sync_from_current_machine: bool,
    pub resource_previews: Vec<IResourcePreview>,
}

/// `IUserDataSyncResourceError` (os campos de `IUserDataSyncResource` mais o erro).
#[derive(Clone, Debug, PartialEq)]
pub struct IUserDataSyncResourceError {
    pub sync_resource: SyncResource,
    pub profile: UserDataProfile,
    pub error: UserDataSyncError,
}

/// `IUserDataSyncResourceInitializer`.
pub trait IUserDataSyncResourceInitializer {
    fn initialize(&self, user_data: IUserData) -> Promise<()>;
}

/// O `string | IUserData | null` que `sync` e `getLatestRemoteUserData` recebem (o `null` é `None` no parâmetro
/// `Option<RefOrUserData>`): o `ref` mais recente do servidor ou os dados já lidos.
#[derive(Clone, Debug, PartialEq)]
pub enum RefOrUserData {
    Ref(String),
    UserData(IUserData),
}

impl From<String> for RefOrUserData {
    fn from(r#ref: String) -> Self {
        Self::Ref(r#ref)
    }
}

impl From<IUserData> for RefOrUserData {
    fn from(user_data: IUserData) -> Self {
        Self::UserData(user_data)
    }
}

/// `IUserDataSynchroniser`.
pub trait IUserDataSynchroniser {
    fn resource(&self) -> SyncResource;
    fn status(&self) -> SyncStatus;
    fn on_did_change_status(&self) -> Event<SyncStatus>;

    fn conflicts(&self) -> IUserDataSyncResourceConflicts;
    fn on_did_change_conflicts(&self) -> Event<IUserDataSyncResourceConflicts>;

    fn on_did_change_local(&self) -> Event<()>;

    fn sync(&self, ref_or_user_data: Option<RefOrUserData>, preview: bool, user_data_sync_configuration: IUserDataSyncConfiguration, headers: IHeaders) -> Promise<Option<IUserDataSyncResourcePreview>>;
    fn accept(&self, resource: &URI, content: Option<Option<String>>) -> Promise<Option<IUserDataSyncResourcePreview>>;
    fn apply(&self, force: bool, headers: IHeaders) -> Promise<Option<IUserDataSyncResourcePreview>>;
    fn stop(&self) -> Promise<()>;

    fn has_previously_synced(&self) -> Promise<bool>;
    fn has_local_data(&self) -> Promise<bool>;
    fn reset_local(&self) -> Promise<()>;

    fn resolve_content(&self, resource: &URI) -> Promise<Option<String>>;
    fn replace(&self, content: &str) -> Promise<bool>;
}

// endregion

// region: keys synced only in web

/// `SYNC_SERVICE_URL_TYPE`: chave sincronizada só na web.
pub const SYNC_SERVICE_URL_TYPE: &str = "sync.store.url.type";

/// `getEnablementKey`.
pub fn get_enablement_key(resource: SyncResource) -> String {
    format!("sync.enable.{}", resource.as_str())
}

// endregion

// region: User Data Sync Services

/// `IUserDataSyncEnablementService`.
pub trait IUserDataSyncEnablementService {
    fn on_did_change_enablement(&self) -> Event<bool>;
    fn is_enabled(&self) -> bool;
    fn can_toggle_enablement(&self) -> bool;
    fn set_enablement(&self, enabled: bool);

    fn on_did_change_resource_enablement(&self) -> Event<(SyncResource, bool)>;
    fn is_resource_enabled(&self, resource: SyncResource, default_value: Option<bool>) -> bool;
    fn set_resource_enablement(&self, resource: SyncResource, enabled: bool);

    fn get_resource_sync_state_version(&self, resource: SyncResource) -> Option<String>;

    /// Checks if resource enabled was explicitly configured before, ignoring its default enablement value
    /// used in [`IUserDataSyncEnablementService::is_resource_enabled`].
    fn is_resource_enablement_configured(&self, resource: SyncResource) -> bool;
}

service_id!(pub I_USER_DATA_SYNC_ENABLEMENT_SERVICE: dyn IUserDataSyncEnablementService = "IUserDataSyncEnablementService");

/// `IUserDataSyncTask`.
pub trait IUserDataSyncTask {
    fn manifest(&self) -> Option<IUserDataManifest>;
    fn run(&self) -> Promise<()>;
    fn stop(&self) -> Promise<()>;
}

/// `IUserDataManualSyncTask`.
pub trait IUserDataManualSyncTask {
    fn id(&self) -> String;
    fn merge(&self) -> Promise<()>;
    fn apply(&self) -> Promise<()>;
    fn stop(&self) -> Promise<()>;
}

/// O `apply: boolean | { force: boolean }` de `IUserDataSyncService.accept`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ApplyMode {
    Apply(bool),
    Force { force: bool },
}

/// `IUserDataSyncService`. `disableCache` ausente é `undefined`.
pub trait IUserDataSyncService {
    fn status(&self) -> SyncStatus;
    fn on_did_change_status(&self) -> Event<SyncStatus>;

    fn conflicts(&self) -> Vec<IUserDataSyncResourceConflicts>;
    fn on_did_change_conflicts(&self) -> Event<Vec<IUserDataSyncResourceConflicts>>;

    fn on_did_change_local(&self) -> Event<SyncResource>;
    fn on_sync_errors(&self) -> Event<Vec<IUserDataSyncResourceError>>;

    fn last_sync_time(&self) -> Option<f64>;
    fn on_did_change_last_sync_time(&self) -> Event<f64>;

    fn on_did_reset_remote(&self) -> Event<()>;
    fn on_did_reset_local(&self) -> Event<()>;

    fn create_sync_task(&self, manifest: Option<IUserDataManifest>, disable_cache: Option<bool>) -> Promise<Rc<dyn IUserDataSyncTask>>;
    fn create_manual_sync_task(&self) -> Promise<Rc<dyn IUserDataManualSyncTask>>;
    fn resolve_content(&self, resource: &URI) -> Promise<Option<String>>;
    fn accept(&self, sync_resource: &IUserDataSyncResource, resource: &URI, content: Option<Option<String>>, apply: ApplyMode) -> Promise<()>;

    fn reset(&self) -> Promise<()>;
    fn reset_remote(&self) -> Promise<()>;
    fn clean_up_remote_data(&self) -> Promise<()>;
    fn reset_local(&self) -> Promise<()>;
    fn has_local_data(&self) -> Promise<bool>;
    fn has_previously_synced(&self) -> Promise<bool>;

    fn replace(&self, sync_resource_handle: &ISyncResourceHandle) -> Promise<()>;

    fn save_remote_activity_data(&self, location: &URI) -> Promise<()>;
    fn extract_activity_data(&self, activity_data_resource: &URI, location: &URI) -> Promise<()>;
}

service_id!(pub I_USER_DATA_SYNC_SERVICE: dyn IUserDataSyncService = "IUserDataSyncService");

/// `SyncOptions`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SyncOptions {
    pub immediately: Option<bool>,
    pub skip_if_synced_recently: Option<bool>,
    pub disable_cache: Option<bool>,
}

/// `IUserDataAutoSyncService`. `onError` entrega o próprio erro (a [`UserDataSyncError`] com as propriedades
/// que ele tiver, como a `url` do [`UserDataSyncStoreError`] e a marca do [`UserDataAutoSyncError`]), que o
/// `instanceof` do original distingue.
pub trait IUserDataAutoSyncService {
    fn on_error(&self) -> Event<Error>;
    fn turn_on(&self) -> Promise<()>;
    fn turn_off(&self, everywhere: bool) -> Promise<()>;
    fn trigger_sync(&self, sources: &[String], options: Option<SyncOptions>) -> Promise<()>;
}

service_id!(pub I_USER_DATA_AUTO_SYNC_SERVICE: dyn IUserDataAutoSyncService = "IUserDataAutoSyncService");

/// `IUserDataSyncUtilService`.
pub trait IUserDataSyncUtilService {
    /// Resolve cada atalho do usuário (`alt+c`, `cmd+k cmd+c`...) para a forma normalizada, que independe
    /// do layout do teclado.
    fn resolve_user_bindings(&self, userbindings: Vec<JsString>) -> Promise<HashMap<JsString, JsString>>;

    fn resolve_formatting_options(&self, resource: Option<&URI>) -> Promise<FormattingOptions>;

    fn resolve_default_core_ignored_settings(&self) -> Promise<Vec<JsString>>;
}

service_id!(pub I_USER_DATA_SYNC_UTIL_SERVICE: dyn IUserDataSyncUtilService = "IUserDataSyncUtilService");

/// `IUserDataSyncLogService`: um `ILogService` (a interface do original não acrescenta nada).
pub type IUserDataSyncLogService = LogServiceRef;

/// `IConflictSetting`: um setting em conflito, com o valor de cada lado (`None` é `undefined`).
#[derive(Clone, Debug, PartialEq)]
pub struct IConflictSetting {
    pub key: JsString,
    pub local_value: Option<Value>,
    pub remote_value: Option<Value>,
}

// endregion

pub const USER_DATA_SYNC_LOG_ID: &str = "userDataSync";
pub const USER_DATA_SYNC_SCHEME: &str = "vscode-userdata-sync";
pub const PREVIEW_DIR_NAME: &str = "preview";
