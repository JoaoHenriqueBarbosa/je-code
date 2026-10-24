//! Porte parcial de `src/vs/workbench/services/authentication/common/authentication.ts`: o tipo
//! [`AllowedExtension`], que o `AuthenticationAccessService` lê e grava, e o que os serviços de acesso, de
//! uso e de consulta (`AuthenticationQueryService`) usam do resto do arquivo: `INTERNAL_AUTH_PROVIDER_PREFIX`,
//! [`AuthenticationSessionAccount`], [`AuthenticationProviderInformation`], e as interfaces
//! [`IAuthenticationService`] e [`IAuthenticationExtensionsService`] reduzidas aos membros que esses serviços
//! chamam.
//!
//! Divergências:
//! - **O que falta de `authentication.ts`** (entra quando o `AuthenticationService` for portado):
//!   `ACCOUNTS_AVATAR_SETTING`, `AuthenticationSession`, `AuthenticationSessionsChangeEvent`,
//!   `IAuthenticationCreateSessionOptions`, `IAuthenticationProviderSessionOptions`,
//!   `IAuthenticationConstraint`, `IAuthenticationWwwAuthenticateRequest`,
//!   `IAuthenticationGetSessionsOptions`, `IAuthenticationProvider`, `IAuthenticationProviderHostDelegate`,
//!   `getDynamicAuthenticationProviderId`, `isAuthenticationSession`,
//!   `isAuthenticationWwwAuthenticateRequest`, `WORKBENCH_ONLY_SESSION_OPTION_PREFIX`,
//!   `withoutWorkbenchOnlySessionOptions`, `getAuthenticationSessionRequestKey`, e os membros de
//!   `IAuthenticationService` (`onDidUnregisterAuthenticationProvider`, `onDidChangeSessions`,
//!   `declaredProviders`, `getSessions`, `createSession`...) e de `IAuthenticationExtensionsService`
//!   (preferência de sessão, `selectSession`, `requestSessionAccess`...) que nenhum serviço portado chama.
//!   Os dois traits crescem com eles.
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

use je_base::JsString;
use je_base::errors::{self, Error};
use je_base::event::Event;
use je_base::json::{Object, Value};
use je_base::jsonc::json_parse;
use je_base::product::StringsOrDictionary;
use je_base::promise::Promise;
use je_base::uri::URI;
use je_instantiation::service_id;
use je_platform::storage::{StorageScope, StorageService};

/// Use this if you don't want the onDidChangeSessions event to fire in the extension host.
pub const INTERNAL_AUTH_PROVIDER_PREFIX: &str = "__";

/// `AuthenticationSessionAccount`.
#[derive(Clone)]
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

    /// Gets the provider ids of all registered authentication providers.
    fn get_provider_ids(&self) -> Vec<String>;

    /// Gets all accounts that are currently logged in across all sessions.
    ///
    /// - `id`: the id of the provider to ask for accounts.
    fn get_accounts(&self, id: &str) -> Promise<Vec<AuthenticationSessionAccount>>;
}

service_id!(pub I_AUTHENTICATION_SERVICE: dyn IAuthenticationService = "IAuthenticationService");

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
