//! Porte parcial de `src/vs/workbench/services/authentication/common/authentication.ts`: o tipo
//! [`AllowedExtension`], que o `AuthenticationAccessService` lê e grava, e o que os serviços de acesso, de
//! uso e de consulta (`AuthenticationQueryService`) usam do resto do arquivo: `INTERNAL_AUTH_PROVIDER_PREFIX`,
//! [`AuthenticationSessionAccount`], [`AuthenticationProviderInformation`], e as interfaces
//! [`IAuthenticationService`] e [`IAuthenticationExtensionsService`] reduzidas aos membros que esses serviços
//! chamam.
//!
//! Divergências:
//! - **O que falta de `authentication.ts`**: `isAuthenticationSession`, `WORKBENCH_ONLY_SESSION_OPTION_PREFIX`,
//!   `withoutWorkbenchOnlySessionOptions` (nenhum serviço portado o chama; entra com o
//!   `AuthenticationExtensionsService`) e os membros de `IAuthenticationExtensionsService`
//!   (preferência de sessão, `selectSession`, `requestSessionAccess`...). Já estão aqui
//!   `ACCOUNTS_AVATAR_SETTING`, `IAuthenticationConstraint`, `IAuthenticationProvider`,
//!   `IAuthenticationProviderHostDelegate` e `getDynamicAuthenticationProviderId`.
//! - **`IAuthenticationService` em dois traits.** [`IAuthenticationService`] mantém os membros que os serviços
//!   de acesso, de uso e de consulta chamam, e [`IAuthenticationServiceProviders`] (supertrait) tem o resto
//!   (`declaredProviders`, `registerAuthenticationProvider`, `removeSession`...), para os dublês que já
//!   implementam o trait reduzido não quebrarem. O `AuthenticationService` implementa os dois.
//! - **`IAuthenticationProvider`**: os membros opcionais do original são métodos com padrão `None`; um provedor
//!   com `dispose` (`isDisposable`) devolve a si em `as_disposable`. `AuthenticationProviderInformation`,
//!   `AuthenticationSession` e os eventos de mudança ganharam `Debug` e `PartialEq` (o `deepEqual` dos testes).
//! - **`IAuthenticationService.getAccounts`** devolve `Promise<Vec<AuthenticationSessionAccount>>` (o
//!   `ReadonlyArray` é um vetor de cópias). `AuthenticationSessionAccount.icon` é `Option<URI>`.
//! - **Eventos com objeto anônimo** (`{ providerId, accountName }` e `{ extensionIds, providerId }`) são as
//!   structs [`AccountAccessChange`] e [`AccountPreferenceChange`].
//! - **Auxiliares compartilhados** ([`trusted_auth_access_ids`], [`all_trusted_auth_access_ids`],
//!   [`read_stored_items`]): os serviços de acesso e de uso, de extensão e de MCP, repetiam a leitura de
//!   `trustedExtensionAuthAccess`/`trustedMcpAuthAccess` e o `try { JSON.parse(...) } catch { }`; aqui cada
//!   um existe uma vez.
//! - **JSON.** No original o objeto lido do armazenamento (`JSON.parse`) é guardado como veio, com qualquer
//!   propriedade a mais. Aqui só os cinco campos declarados sobrevivem ([`AllowedExtension::from_json`]),
//!   e [`AllowedExtension::to_json`] escreve na ordem `id`, `name`, `allowed`, `lastUsed`, `trusted`, sem os
//!   campos ausentes (o `JSON.stringify` omite `undefined`). Uma entrada que não é objeto ou não tem `id`
//!   textual é descartada (no original ela ficaria na lista, sem nunca casar com uma extensão); um `name`
//!   ausente vira texto vazio, que é falso como o `undefined` nas conferências do serviço.

use std::rc::Rc;

use je_base::JsString;
use je_base::dynamic::{json_stringify, Dynamic};
use je_base::errors::{self, Error};
use je_base::event::Event;
use je_base::json::{Object, Value};
use je_base::lifecycle::DisposableRef;
use je_base::oauth::{IAuthenticationChallenge, IAuthorizationProtectedResourceMetadata, IAuthorizationServerMetadata};
use je_base::jsonc::json_parse;
use je_base::product::StringsOrDictionary;
use je_base::promise::Promise;
use je_base::uri::URI;
use je_instantiation::service_id;
use je_platform::storage::{StorageScope, StorageService};

/// Use this if you don't want the onDidChangeSessions event to fire in the extension host.
pub const INTERNAL_AUTH_PROVIDER_PREFIX: &str = "__";

/// `AuthenticationSessionAccount`.
#[derive(Clone, Debug, PartialEq)]
pub struct AuthenticationSessionAccount {
    pub label: String,
    pub id: String,
    /// An optional icon for the account. This is typically a URI to a profile image/avatar.
    pub icon: Option<URI>,
}

impl AuthenticationSessionAccount {
    /// Uma conta sem ícone: `{ label, id }`.
    pub fn new(label: impl Into<String>, id: impl Into<String>) -> Self {
        Self { label: label.into(), id: id.into(), icon: None }
    }
}

/// `AuthenticationSession`.
#[derive(Clone, Debug, PartialEq)]
pub struct AuthenticationSession {
    pub id: String,
    pub access_token: String,
    pub account: AuthenticationSessionAccount,
    pub scopes: Vec<String>,
    /// `idToken?`.
    pub id_token: Option<String>,
    /// `expiresAfter?`.
    pub expires_after: Option<f64>,
    /// The issuing OAuth server, not a REST API endpoint or resource audience.
    pub authorization_server: Option<URI>,
}

impl AuthenticationSession {
    /// Uma sessão sem `idToken`, `expiresAfter` e `authorizationServer`.
    pub fn new(id: impl Into<String>, access_token: impl Into<String>, account: AuthenticationSessionAccount, scopes: Vec<String>) -> Self {
        Self { id: id.into(), access_token: access_token.into(), account, scopes, id_token: None, expires_after: None, authorization_server: None }
    }
}

/// `AuthenticationSessionsChangeEvent`: `added`, `removed` e `changed` são `ReadonlyArray | undefined`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AuthenticationSessionsChangeEvent {
    pub added: Option<Vec<AuthenticationSession>>,
    pub removed: Option<Vec<AuthenticationSession>>,
    pub changed: Option<Vec<AuthenticationSession>>,
}

/// O `{ providerId, label, event }` de `IAuthenticationService.onDidChangeSessions`.
#[derive(Clone, Debug, PartialEq)]
pub struct AuthenticationSessionsChange {
    pub provider_id: String,
    pub label: String,
    pub event: AuthenticationSessionsChangeEvent,
}

/// `IAuthenticationProviderSessionOptions`: as opções passadas ao provedor ao pedir sessões.
#[derive(Clone, Default)]
pub struct IAuthenticationProviderSessionOptions {
    /// Whether the provider must avoid user interaction while resolving existing sessions.
    pub silent: Option<bool>,
    /// The account that is being asked about. If this is passed in, the provider should attempt to return the
    /// sessions that are only related to this account.
    pub account: Option<AuthenticationSessionAccount>,
    /// The authorization server that is being asked about.
    pub authorization_server: Option<URI>,
    /// The OAuth client identifier to use instead of the provider's default client.
    pub client_id: Option<String>,
    /// When specified, the authentication provider will request a token bound to this resource URI
    /// (RFC 8707 resource indicator).
    pub resource: Option<String>,
    /// The audience for the requested access token.
    pub audience: Option<String>,
    /// As demais chaves (`[key: string]: any`), específicas de cada provedor.
    pub extra: Option<Dynamic>,
}

/// `IAuthenticationCreateSessionOptions`, que estende [`IAuthenticationProviderSessionOptions`].
#[derive(Clone, Default)]
pub struct IAuthenticationCreateSessionOptions {
    pub session_options: IAuthenticationProviderSessionOptions,
    pub activate_immediate: Option<bool>,
}

/// `IAuthenticationGetSessionsOptions`.
pub type IAuthenticationGetSessionsOptions = IAuthenticationProviderSessionOptions;

/// `IAuthenticationWwwAuthenticateRequest`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IAuthenticationWwwAuthenticateRequest {
    /// The raw WWW-Authenticate header value that triggered this challenge.
    pub www_authenticate: String,
    /// Optional scopes for the session.
    pub fallback_scopes: Option<Vec<String>>,
}

/// `ReadonlyArray<string> | IAuthenticationWwwAuthenticateRequest` (`scopeListOrRequest`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AuthenticationScopeListOrRequest {
    Scopes(Vec<String>),
    Request(IAuthenticationWwwAuthenticateRequest),
}

/// `getAuthenticationSessionRequestKey`: builds a value-sensitive request key without splitting opaque provider scope strings.
///
/// As chaves a mais de `options` (`[key: string]: any`, aqui `extra`) entram na chave como no `Object.entries` do original, e as
/// entradas `undefined` ficam de fora; `JSON.stringify` e o `sort` padrão (unidades UTF-16) são os do JavaScript.
pub fn get_authentication_session_request_key(scope_list_or_request: &AuthenticationScopeListOrRequest, options: &IAuthenticationProviderSessionOptions) -> String {
    let sorted_scopes = |scopes: &[String]| {
        let mut scopes: Vec<JsString> = scopes.iter().map(|scope| JsString::from(scope.as_str())).collect();
        scopes.sort();
        Dynamic::array(scopes.into_iter().map(Dynamic::String))
    };
    let request = match scope_list_or_request {
        AuthenticationScopeListOrRequest::Scopes(scopes) => sorted_scopes(scopes),
        AuthenticationScopeListOrRequest::Request(request) => Dynamic::object([
            ("wwwAuthenticate", Dynamic::string(&request.www_authenticate)),
            ("fallbackScopes", request.fallback_scopes.as_ref().map_or(Dynamic::Undefined, |scopes| sorted_scopes(scopes))),
        ]),
    };

    // `{ ...options, account: options.account?.id, authorizationServer: options.authorizationServer?.toString(true) }`
    let mut entries: Vec<(JsString, Dynamic)> = Vec::new();
    let mut put = |name: JsString, value: Dynamic| match entries.iter_mut().find(|(key, _)| *key == name) {
        Some(existing) => existing.1 = value,
        None => entries.push((name, value)),
    };
    if let Some(Dynamic::Object(extra)) = &options.extra {
        for (name, value) in extra.entries() {
            put(name, value);
        }
    }
    let text = |value: &Option<String>| value.as_deref().map_or(Dynamic::Undefined, Dynamic::string);
    put(JsString::from("silent"), options.silent.map_or(Dynamic::Undefined, Dynamic::Bool));
    put(JsString::from("clientId"), text(&options.client_id));
    put(JsString::from("resource"), text(&options.resource));
    put(JsString::from("audience"), text(&options.audience));
    put(JsString::from("account"), options.account.as_ref().map_or(Dynamic::Undefined, |account| Dynamic::string(&account.id)));
    put(JsString::from("authorizationServer"), options.authorization_server.as_ref().map_or(Dynamic::Undefined, |uri| Dynamic::String(uri.to_string(true))));
    entries.retain(|(_, value)| !value.is_undefined());
    entries.sort_by(|(first, _), (second, _)| first.cmp(second));
    let entries = Dynamic::array(entries.into_iter().map(|(name, value)| Dynamic::array([Dynamic::String(name), value])));
    json_stringify(&Dynamic::array([request, entries])).unwrap_or_default()
}

/// `AuthenticationProviderInformation`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AuthenticationProviderInformation {
    pub id: String,
    pub label: String,
    pub authorization_server_globs: Option<Vec<String>>,
}

/// O `{ providerId: string; accountName: string }` dos eventos de mudança de acesso.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AccountAccessChange {
    pub provider_id: String,
    pub account_name: String,
}

/// O `{ extensionIds: string[]; providerId: string }` de `onDidChangeAccountPreference`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AccountPreferenceChange {
    pub extension_ids: Vec<String>,
    pub provider_id: String,
}

/// `IAuthenticationService`, reduzido aos membros que os serviços portados chamam (ver o topo do arquivo).
pub trait IAuthenticationService {
    /// Fires when an authentication provider has been registered.
    fn on_did_register_authentication_provider(&self) -> Event<AuthenticationProviderInformation>;

    /// Fires when the list of sessions for a provider has been added, removed or changed.
    fn on_did_change_sessions(&self) -> Event<AuthenticationSessionsChange>;

    /// Gets the provider ids of all registered authentication providers.
    fn get_provider_ids(&self) -> Vec<String>;

    /// Gets all accounts that are currently logged in across all sessions.
    ///
    /// - `id`: the id of the provider to ask for accounts.
    fn get_accounts(&self, id: &str) -> Promise<Vec<AuthenticationSessionAccount>>;

    /// Gets all sessions that satisfy the given scopes from the provider with the given id.
    ///
    /// - `id`: the id of the provider to ask for a session.
    /// - `scope_list_or_request`: the scopes for the session.
    /// - `options`: additional options for getting sessions.
    /// - `activate_immediate`: if true, the provider should activate immediately if it is not already.
    fn get_sessions(
        &self,
        id: &str,
        scope_list_or_request: Option<AuthenticationScopeListOrRequest>,
        options: Option<IAuthenticationGetSessionsOptions>,
        activate_immediate: Option<bool>,
    ) -> Promise<Vec<AuthenticationSession>>;

    /// Creates an AuthenticationSession with the given provider and scopes.
    ///
    /// - `provider_id`: the id of the provider.
    /// - `scope_list_or_request`: the scopes to request.
    /// - `options`: additional options for creating the session.
    fn create_session(
        &self,
        provider_id: &str,
        scope_list_or_request: AuthenticationScopeListOrRequest,
        options: Option<IAuthenticationCreateSessionOptions>,
    ) -> Promise<AuthenticationSession>;

    /// `declaredProviders`: the providers that extensions declare in their manifests, registered or not. O padrão
    /// (nenhum declarado) é o dos dublês que só conhecem os provedores registrados.
    fn declared_providers(&self) -> Vec<AuthenticationProviderInformation> {
        Vec::new()
    }

    /// `onDidChangeDeclaredProviders`: fires when the list of declared providers changes. O padrão nunca dispara.
    fn on_did_change_declared_providers(&self) -> Event<()> {
        Event::none()
    }

    /// `onDidUnregisterAuthenticationProvider`: fires when an authentication provider has been unregistered. O
    /// padrão nunca dispara.
    fn on_did_unregister_authentication_provider(&self) -> Event<AuthenticationProviderInformation> {
        Event::none()
    }

    /// `isAuthenticationProviderRegistered(id)`: whether a provider with that id is registered. O padrão consulta
    /// [`get_provider_ids`](Self::get_provider_ids), que é o que o serviço real faz.
    fn is_authentication_provider_registered(&self, id: &str) -> bool {
        self.get_provider_ids().iter().any(|provider_id| provider_id == id)
    }
}

service_id!(pub I_AUTHENTICATION_SERVICE: dyn IAuthenticationService = "IAuthenticationService");

/// Setting that controls whether the profile image (avatar) of a signed-in account is shown in account related UI.
pub const ACCOUNTS_AVATAR_SETTING: &str = "workbench.accounts.showAvatar";

/// `IAuthenticationConstraint`: represents constraints for authentication, including challenges and optional
/// scopes. This is used when creating or retrieving sessions that must satisfy specific authentication
/// requirements from WWW-Authenticate headers.
#[derive(Clone, Debug)]
pub struct IAuthenticationConstraint {
    /// Array of authentication challenges parsed from WWW-Authenticate headers.
    pub challenges: Vec<IAuthenticationChallenge>,
    /// Optional scopes for the session. If not provided, the authentication provider may extract scopes from
    /// the challenges or use default scopes.
    pub fallback_scopes: Option<Vec<String>>,
}

/// `IAuthenticationProviderHostDelegate`.
pub trait IAuthenticationProviderHostDelegate {
    /// Priority for this delegate, delegates are tested in descending priority order.
    fn priority(&self) -> f64;

    fn create(
        &self,
        authorization_server: &URI,
        server_metadata: &IAuthorizationServerMetadata,
        resource: Option<&IAuthorizationProtectedResourceMetadata>,
        client_id: Option<&str>,
        client_secret: Option<&str>,
    ) -> Promise<String>;

    /// Creates an XAA (enterprise-managed, ID-JAG) authentication provider for the given SSO issuer. The returned
    /// string is the provider id. `None` é o `createXaa?` ausente do original.
    fn create_xaa(&self, _issuer: &URI) -> Option<Promise<String>> {
        None
    }

    /// `!!delegate.createXaa`: se o delegado tem o membro opcional `createXaa`.
    fn supports_xaa(&self) -> bool {
        false
    }
}

/// `getDynamicAuthenticationProviderId`.
pub fn get_dynamic_authentication_provider_id(authorization_server: &URI, resource: Option<&IAuthorizationProtectedResourceMetadata>) -> String {
    match resource {
        Some(resource) => format!("{} {}", authorization_server.to_string(true).to_string_lossy(), resource.resource),
        None => authorization_server.to_string(true).to_string_lossy(),
    }
}

/// `IAuthenticationProvider`: represents an authentication provider.
///
/// Os membros opcionais do original (`resourceServer?`, `authorizationServers?`, `confirmation?`,
/// `getSessionsFromChallenges?`, `createSessionFromChallenges?`) são métodos com padrão que devolve `None`, o
/// membro ausente.
pub trait IAuthenticationProvider {
    /// The unique identifier of the authentication provider.
    fn id(&self) -> String;

    /// The display label of the authentication provider.
    fn label(&self) -> String;

    /// The resource server URI that this provider is responsible for, if any.
    fn resource_server(&self) -> Option<URI> {
        None
    }

    /// The resolved authorization servers. These can still contain globs, but should be concrete URIs.
    fn authorization_servers(&self) -> Option<Vec<URI>> {
        None
    }

    /// Indicates whether the authentication provider supports multiple accounts.
    fn supports_multiple_accounts(&self) -> bool;

    /// Optional function to provide a custom confirmation message for authentication prompts. If not implemented
    /// (ou devolve `None`), the default confirmation messages will be used.
    fn confirmation(&self, _extension_name: &str, _recreating_session: bool) -> Option<String> {
        None
    }

    /// An event which fires when the array of sessions has changed, or data within a session has changed.
    fn on_did_change_sessions(&self) -> Event<AuthenticationSessionsChangeEvent>;

    /// Retrieves a list of authentication sessions. If `scopes` is provided, the sessions returned should match
    /// these permissions, otherwise all sessions should be returned.
    fn get_sessions(&self, scopes: Option<Vec<String>>, options: IAuthenticationProviderSessionOptions) -> Promise<Vec<AuthenticationSession>>;

    /// Prompts the user to log in. If login is successful, the `onDidChangeSessions` event should be fired. If
    /// login fails, a rejected promise should be returned. If the provider does not support multiple accounts,
    /// this method should not be called if there is already an existing session matching the provided scopes.
    fn create_session(&self, scopes: Vec<String>, options: IAuthenticationProviderSessionOptions) -> Promise<AuthenticationSession>;

    /// Get existing sessions that match the given authentication constraints. `None` é o membro ausente.
    fn get_sessions_from_challenges(
        &self,
        _constraint: IAuthenticationConstraint,
        _options: IAuthenticationProviderSessionOptions,
    ) -> Option<Promise<Vec<AuthenticationSession>>> {
        None
    }

    /// Create a new session based on authentication constraints. This is called when no existing session matches
    /// the constraint requirements. `None` é o membro ausente.
    fn create_session_from_challenges(
        &self,
        _constraint: IAuthenticationConstraint,
        _options: IAuthenticationProviderSessionOptions,
    ) -> Option<Promise<AuthenticationSession>> {
        None
    }

    /// Removes the session corresponding to the specified session ID. If the removal is successful, the
    /// `onDidChangeSessions` event should be fired. If a session cannot be removed, the provider should reject.
    fn remove_session(&self, session_id: &str) -> Promise<()>;

    /// O `isDisposable(authenticationProvider)` do `registerAuthenticationProvider`: um provedor com `dispose`
    /// devolve a si mesmo como descartável, e o serviço o descarta junto com o registro.
    fn as_disposable(&self) -> Option<DisposableRef> {
        None
    }
}

/// Os membros de `IAuthenticationService` que os serviços portados antes do `AuthenticationService` não chamam
/// (ver o topo do arquivo). Ficam num trait à parte, supertrait [`IAuthenticationService`], para os dublês dos
/// outros serviços, que implementam só o trait reduzido, continuarem valendo; o `AuthenticationService`
/// implementa os dois, e juntos são o `IAuthenticationService` do original.
pub trait IAuthenticationServiceProviders: IAuthenticationService {
    /// Registers that an extension has declared an authentication provider in their package.json. Lança se o id ou
    /// o rótulo são vazios, ou se o id já foi declarado.
    fn register_declared_authentication_provider(&self, provider: AuthenticationProviderInformation);

    /// Unregisters a declared authentication provider.
    fn unregister_declared_authentication_provider(&self, id: &str);

    /// Checks if an authentication provider is dynamic.
    fn is_dynamic_authentication_provider(&self, id: &str) -> bool;

    /// Registers an authentication provider.
    fn register_authentication_provider(&self, id: &str, provider: Rc<dyn IAuthenticationProvider>);

    /// Unregisters an authentication provider.
    fn unregister_authentication_provider(&self, id: &str);

    /// Gets the provider with the given id. Lança se o provedor não está registrado.
    fn get_provider(&self, id: &str) -> Rc<dyn IAuthenticationProvider>;

    /// Removes the session with the given id from the provider with the given id.
    fn remove_session(&self, provider_id: &str, session_id: &str) -> Promise<()>;

    /// Gets a provider id for a specified authorization server. `resource_server` is the resource server URI that
    /// should match the provider's resourceServer (if defined).
    fn get_or_activate_provider_id_for_server(&self, authorization_server: &URI, resource_server: Option<&URI>) -> Promise<Option<String>>;

    /// Allows the ability register a delegate that will be used to start authentication providers.
    fn register_authentication_provider_host_delegate(&self, delegate: Rc<dyn IAuthenticationProviderHostDelegate>) -> DisposableRef;

    /// Creates a dynamic authentication provider for the given server metadata.
    fn create_dynamic_authentication_provider(
        &self,
        authorization_server: &URI,
        server_metadata: &IAuthorizationServerMetadata,
        resource: Option<&IAuthorizationProtectedResourceMetadata>,
        client_id: Option<&str>,
        client_secret: Option<&str>,
    ) -> Promise<Option<Rc<dyn IAuthenticationProvider>>>;

    /// Gets or creates a built-in XAA (enterprise-managed, ID-JAG) authentication provider for the given SSO issuer.
    /// Subsequent calls with the same issuer return the existing provider.
    fn create_or_get_xaa_provider(&self, issuer: &URI) -> Promise<Option<String>>;
}

// Pendência anotada no original: mover isto para o `MainThreadAuthentication`.
/// `IAuthenticationExtensionsService`, reduzido às preferências de conta (ver o topo do arquivo).
pub trait IAuthenticationExtensionsService {
    /// Fires when an account preference for a specific provider has changed for the specified extensions.
    /// Does not fire when an account preference is removed.
    fn on_did_change_account_preference(&self) -> Event<AccountPreferenceChange>;

    /// Returns the accountName (also known as account.label) to pair with `IAuthenticationAccessService`
    /// to get the account preference, or `None` if there is no preference set.
    fn get_account_preference(&self, extension_id: &str, provider_id: &str) -> Option<String>;

    /// Sets the account preference for the given provider and extension.
    fn update_account_preference(&self, extension_id: &str, provider_id: &str, account: &AuthenticationSessionAccount);

    /// Removes the account preference for the given provider and extension.
    fn remove_account_preference(&self, extension_id: &str, provider_id: &str);
}

service_id!(pub I_AUTHENTICATION_EXTENSIONS_SERVICE: dyn IAuthenticationExtensionsService = "IAuthenticationExtensionsService");

/// A lista de `trustedExtensionAuthAccess`/`trustedMcpAuthAccess` que vale para o provedor: o próprio array,
/// ou o `dicionário[provedor]`, ou vazia.
pub(super) fn trusted_auth_access_ids(access: Option<StringsOrDictionary>, provider_id: &str) -> Vec<String> {
    match access {
        // Case 1: the setting is an array
        Some(StringsOrDictionary::Strings(ids)) => ids,
        // Case 2: the setting is an object
        Some(StringsOrDictionary::Dictionary(dictionary)) => dictionary.get(provider_id).cloned().unwrap_or_default(),
        None => Vec::new(),
    }
}

/// Todos os identificadores de `trustedExtensionAuthAccess`/`trustedMcpAuthAccess`, de qualquer provedor: o
/// array, ou a concatenação dos valores do dicionário (`Object.values`).
pub(super) fn all_trusted_auth_access_ids(access: Option<StringsOrDictionary>) -> Vec<String> {
    match access {
        Some(StringsOrDictionary::Strings(ids)) => ids,
        Some(StringsOrDictionary::Dictionary(dictionary)) => dictionary.into_values().flatten().collect(),
        None => Vec::new(),
    }
}

/// O trecho `try { JSON.parse(storageService.get(key, StorageScope.APPLICATION)) } catch { }` dos serviços de
/// acesso e de uso: os itens do array guardado em `key`, ou vazio se não há nada guardado ou o texto não é
/// JSON. Um JSON válido que não é array deixa o original lançar `TypeError` ao usar o valor como lista, e
/// aqui lança [`Error`] com `not_array_message`, o texto que o motor JS daria.
pub(super) fn read_stored_items(storage_service: &dyn StorageService, key: &str, not_array_message: &str) -> Vec<Value> {
    let Some(source) = storage_service.get(key, StorageScope::Application, None).filter(|source| !source.is_empty()) else {
        return Vec::new();
    };
    match json_parse(&source.encode_utf16().collect::<Vec<u16>>()) {
        Ok(Value::Array(items)) => items,
        Ok(_) => errors::throw(Error::new(not_array_message)),
        Err(_) => Vec::new(),
    }
}

/// `AllowedExtension`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AllowedExtension {
    pub id: String,
    pub name: String,
    /// If true or undefined, the extension is allowed to use the account. If false, the extension is not
    /// allowed to use the account. (`undefined` não deveria ser um valor válido, mas é, por enquanto.)
    pub allowed: Option<bool>,
    pub last_used: Option<f64>,
    /// If true, this comes from the product.json.
    pub trusted: Option<bool>,
}

impl AllowedExtension {
    /// O objeto do `JSON.parse` como [`AllowedExtension`]; `None` se não é objeto ou não tem `id` textual.
    pub fn from_json(value: &Value) -> Option<Self> {
        let Value::Object(object) = value else { return None };
        let Some(Value::String(id)) = object.get("id") else { return None };
        Some(Self {
            id: id.to_string_lossy(),
            name: match object.get("name") {
                Some(Value::String(name)) => name.to_string_lossy(),
                _ => String::new(),
            },
            allowed: match object.get("allowed") {
                Some(Value::Bool(allowed)) => Some(*allowed),
                _ => None,
            },
            last_used: match object.get("lastUsed") {
                Some(Value::Number(last_used)) => Some(*last_used),
                _ => None,
            },
            trusted: match object.get("trusted") {
                Some(Value::Bool(trusted)) => Some(*trusted),
                _ => None,
            },
        })
    }

    /// O objeto que o `JSON.stringify` escreveria.
    pub fn to_json(&self) -> Value {
        let mut object = Object::new();
        object.insert(JsString::from("id"), Value::String(JsString::from(self.id.as_str())));
        object.insert(JsString::from("name"), Value::String(JsString::from(self.name.as_str())));
        if let Some(allowed) = self.allowed {
            object.insert(JsString::from("allowed"), Value::Bool(allowed));
        }
        if let Some(last_used) = self.last_used {
            object.insert(JsString::from("lastUsed"), Value::Number(last_used));
        }
        if let Some(trusted) = self.trusted {
            object.insert(JsString::from("trusted"), Value::Bool(trusted));
        }
        Value::Object(object)
    }
}
