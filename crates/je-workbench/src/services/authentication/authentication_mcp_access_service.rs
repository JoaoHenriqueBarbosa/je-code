//! Porte de `src/vs/workbench/services/authentication/browser/authenticationMcpAccessService.ts`: o serviço
//! que guarda, por provedor e conta, quais servidores MCP podem usar a conta. É só armazenamento de
//! permissões, sem nada do runtime de MCP.
//!
//! Divergências:
//! - **Injeção.** Os parâmetros decorados (`@IStorageService`, `@IProductService`) são a tupla `Deps` do
//!   [`Injectable`]. `IAuthenticationMcpAccessService` é o trait [`IAuthenticationMcpAccessService`]; a
//!   classe herda de `Disposable`, aqui um `Disposable` guardado no campo `base`.
//! - **`registerSingleton(..., InstantiationType.Delayed)`** vira [`register`], que o chamador roda uma vez
//!   na montagem do workbench; o proxy de instanciação atrasada é o [`AuthenticationMcpAccessServiceProxy`].
//! - **Valores.** `AllowedMcpServer[]` é `Vec<AllowedMcpServer>` (cópia, como o `JSON.parse` fresco do
//!   original) e `updateAllowedMcpServers` recebe `&[AllowedMcpServer]`. `boolean | undefined` é
//!   `Option<bool>`. `trustedMcpAuthAccess` (`string[] | Record<string, string[]> | undefined`) é
//!   `Option<StringsOrDictionary>`, lido do `IProductService` a cada chamada. O evento
//!   `onDidChangeMcpSessionAccess` emite o `{ providerId, accountName }` anônimo do original, a struct
//!   [`AccountAccessChange`] (a mesma do serviço de acesso por extensão), com o nome [`McpSessionAccessChange`].
//! - **`isAccessAllowed` e `isAccessAllowedForUrl`** são os dois pontos de entrada de `_isAccessAllowed`, que
//!   recebe `mcpServerUrl: string | undefined`.
//! - **`JSON.parse`.** Um erro de sintaxe ou de leitura no armazenamento dá lista vazia (o `catch` do
//!   original). Um JSON válido que não é array deixa o original lançar `TypeError` ao usar a lista, e aqui
//!   lança o mesmo erro com [`je_base::errors::throw`] (ver [`read_stored_items`]). Das propriedades de cada
//!   servidor só as sete declaradas sobrevivem ([`AllowedMcpServer::from_json`]), na ordem de
//!   [`AllowedMcpServer::to_json`]; uma entrada que não é objeto ou não tem `id` textual é descartada, e um
//!   `name` ausente vira texto vazio, que é falso como o `undefined` nas conferências do serviço.
//! - **`urlsEqual`** usa o `URL` do padrão WHATWG de `je_platform` (`new URL(a).toString()` é o `href`); uma
//!   URL que não é válida dá `false`, como o `catch` do original.
//! - **`agentHost`** só vem do JSON se for um objeto com `authority` e `label` textuais.

use std::rc::Rc;

use je_base::JsString;
use je_base::event::{Emitter, Event};
use je_base::json::{Object, Value};
use je_base::json_formatter::json_stringify;
use je_base::lifecycle::{Disposable, IDisposable};
use je_instantiation::{InstantiationType, Injectable, delayed_proxy, register_singleton, service_id, service_impl};
use je_platform::product::IProductService;
use je_platform::storage::{StorageScope, StorageService, StorageTarget};
use je_platform::url::whatwg::{href, try_new_url};
use je_storage::StorageValue;

use super::authentication::{AccountAccessChange, read_stored_items, trusted_auth_access_ids};

/// O `{ providerId: string; accountName: string }` de `onDidChangeMcpSessionAccess`.
pub type McpSessionAccessChange = AccountAccessChange;

/// Compares two MCP server URLs for the purpose of access binding. They are compared by their canonical
/// WHATWG URL form, so cosmetic differences in the origin (host case, default port, encoding) and a root
/// trailing slash ("foo.com" vs "foo.com/") don't force a spurious re-consent, while a trailing slash on a
/// path ("foo.com/a" vs "foo.com/a/") is preserved as a meaningful difference between endpoints.
pub fn urls_equal(a: Option<&str>, b: Option<&str>) -> bool {
    if a == b {
        return true;
    }
    let (Some(a), Some(b)) = (a, b) else {
        return false;
    };
    match (try_new_url(&JsString::from(a)), try_new_url(&JsString::from(b))) {
        (Some(a), Some(b)) => href(&a).to_string_lossy() == href(&b).to_string_lossy(),
        _ => false,
    }
}

/// O `{ authority: string; label: string }` de `AllowedMcpServer.agentHost`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AgentHost {
    pub authority: String,
    pub label: String,
}

/// `AllowedMcpServer`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AllowedMcpServer {
    pub id: String,
    pub name: String,
    /// If true or undefined, the extension is allowed to use the account. If false, the extension is not
    /// allowed to use the account. (`undefined` não deveria ser um valor válido, mas é, por enquanto.)
    pub allowed: Option<bool>,
    pub last_used: Option<f64>,
    /// If true, this comes from the product.json.
    pub trusted: Option<bool>,
    /// The MCP server URL the grant was made for. A token is only released to a server whose current URL
    /// matches this, so changing the URL while keeping the same id requires the user to re-consent.
    /// `None` for stdio servers, which have no URL.
    pub url: Option<String>,
    /// Set when the grant is for an MCP server hosted by an agent host (which runs the server in the
    /// CLI/agent-host process rather than registering it with the workbench `IMcpService`). Carries the
    /// host `authority` and its display `label` so management surfaces can present agent-host servers in
    /// their own section instead of filtering them out as "unknown". Agent hosts may be ephemeral and not
    /// always connected, but we still want to be able to display grants we know about and will respect.
    pub agent_host: Option<AgentHost>,
}

fn json_string(value: &str) -> Value {
    Value::String(JsString::from(value))
}

fn json_text(object: &Object, key: &str) -> Option<String> {
    match object.get(key) {
        Some(Value::String(text)) => Some(text.to_string_lossy()),
        _ => None,
    }
}

impl AllowedMcpServer {
    /// O objeto do `JSON.parse` como [`AllowedMcpServer`]; `None` se não é objeto ou não tem `id` textual.
    pub fn from_json(value: &Value) -> Option<Self> {
        let Value::Object(object) = value else { return None };
        let id = json_text(object, "id")?;
        Some(Self {
            id,
            name: json_text(object, "name").unwrap_or_default(),
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
            url: json_text(object, "url"),
            agent_host: match object.get("agentHost") {
                Some(Value::Object(agent_host)) => {
                    json_text(agent_host, "authority").zip(json_text(agent_host, "label")).map(|(authority, label)| AgentHost { authority, label })
                }
                _ => None,
            },
        })
    }

    /// O objeto que o `JSON.stringify` escreveria: `id`, `name`, `allowed`, `lastUsed`, `trusted`, `url` e
    /// `agentHost`, sem os campos ausentes (o `JSON.stringify` omite `undefined`).
    pub fn to_json(&self) -> Value {
        let mut object = Object::new();
        object.insert(JsString::from("id"), json_string(&self.id));
        object.insert(JsString::from("name"), json_string(&self.name));
        if let Some(allowed) = self.allowed {
            object.insert(JsString::from("allowed"), Value::Bool(allowed));
        }
        if let Some(last_used) = self.last_used {
            object.insert(JsString::from("lastUsed"), Value::Number(last_used));
        }
        if let Some(trusted) = self.trusted {
            object.insert(JsString::from("trusted"), Value::Bool(trusted));
        }
        if let Some(url) = &self.url {
            object.insert(JsString::from("url"), json_string(url));
        }
        if let Some(agent_host) = &self.agent_host {
            let mut host = Object::new();
            host.insert(JsString::from("authority"), json_string(&agent_host.authority));
            host.insert(JsString::from("label"), json_string(&agent_host.label));
            object.insert(JsString::from("agentHost"), Value::Object(host));
        }
        Value::Object(object)
    }
}

/// `IAuthenticationMcpAccessService`.
pub trait IAuthenticationMcpAccessService {
    fn on_did_change_mcp_session_access(&self) -> Event<McpSessionAccessChange>;

    /// Inspect the stored access decision for an MCP server, keyed by id alone. Used by management and
    /// inspection surfaces (e.g. the "Manage Trusted MCP Servers" UI) that operate on a server id without a
    /// live URL. For the security-critical token-release gate use [`Self::is_access_allowed_for_url`] instead.
    ///
    /// - `provider_id`: the id of the authentication provider.
    /// - `account_name`: the account name that access is checked for.
    /// - `mcp_server_id`: the id of the MCP server requesting access.
    ///
    /// Returns `Some(true)` or `Some(false)` if the user has opted to permanently grant or disallow access,
    /// and `None` if they haven't made a choice yet.
    fn is_access_allowed(&self, provider_id: &str, account_name: &str, mcp_server_id: &str) -> Option<bool>;

    /// Gate for releasing a token to an HTTP MCP server. Access is only allowed if `mcp_server_url` matches
    /// the URL stored when access was granted, so re-pointing a server at a new endpoint (while keeping the
    /// same id) requires the user to re-consent. `product.json`-trusted servers bypass the URL check. Only
    /// HTTP servers authenticate, so the URL is always known and therefore required.
    ///
    /// - `provider_id`: the id of the authentication provider.
    /// - `account_name`: the account name that access is checked for.
    /// - `mcp_server_id`: the id of the MCP server requesting access.
    /// - `mcp_server_url`: the MCP server's current URL.
    ///
    /// Returns `Some(true)` or `Some(false)` if the user has opted to permanently grant or disallow access,
    /// and `None` if they haven't made a choice yet (or the URL no longer matches the granted one).
    fn is_access_allowed_for_url(&self, provider_id: &str, account_name: &str, mcp_server_id: &str, mcp_server_url: &str) -> Option<bool>;
    fn read_allowed_mcp_servers(&self, provider_id: &str, account_name: &str) -> Vec<AllowedMcpServer>;
    fn update_allowed_mcp_servers(&self, provider_id: &str, account_name: &str, mcp_servers: &[AllowedMcpServer]);
    fn remove_allowed_mcp_servers(&self, provider_id: &str, account_name: &str);
}

delayed_proxy!(pub AuthenticationMcpAccessServiceProxy for dyn IAuthenticationMcpAccessService {
    methods {
        fn is_access_allowed(&self, provider_id: &str, account_name: &str, mcp_server_id: &str) -> Option<bool>;
        fn is_access_allowed_for_url(&self, provider_id: &str, account_name: &str, mcp_server_id: &str, mcp_server_url: &str) -> Option<bool>;
        fn read_allowed_mcp_servers(&self, provider_id: &str, account_name: &str) -> Vec<AllowedMcpServer>;
        fn update_allowed_mcp_servers(&self, provider_id: &str, account_name: &str, mcp_servers: &[AllowedMcpServer]);
        fn remove_allowed_mcp_servers(&self, provider_id: &str, account_name: &str);
    }
    events {
        fn on_did_change_mcp_session_access(&self) -> McpSessionAccessChange;
    }
});

service_id!(pub I_AUTHENTICATION_MCP_ACCESS_SERVICE: dyn IAuthenticationMcpAccessService = "IAuthenticationMcpAccessService", proxy AuthenticationMcpAccessServiceProxy);

// Pendência anotada no original por Tyler Leonhardt: decidir se esta classe deveria guardar só o que é
// permitido e descartar o que é negado.
/// `AuthenticationMcpAccessService`.
pub struct AuthenticationMcpAccessService {
    base: Rc<Disposable>,
    on_did_change_mcp_session_access: Rc<Emitter<McpSessionAccessChange>>,
    storage_service: Rc<dyn StorageService>,
    product_service: Rc<dyn IProductService>,
}

impl Injectable for AuthenticationMcpAccessService {
    type Args = ();
    type Deps = (Rc<dyn StorageService>, Rc<dyn IProductService>);

    fn create(_: (), (storage_service, product_service): Self::Deps) -> Rc<Self> {
        let base = Disposable::new();
        let on_did_change_mcp_session_access = Emitter::new();
        base.register(on_did_change_mcp_session_access.clone());
        Rc::new(Self { base, on_did_change_mcp_session_access, storage_service, product_service })
    }
}

service_impl!(AuthenticationMcpAccessService => dyn IAuthenticationMcpAccessService, disposable);

/// `registerSingleton(IAuthenticationMcpAccessService, AuthenticationMcpAccessService, InstantiationType.Delayed)`.
pub fn register() {
    register_singleton::<AuthenticationMcpAccessService>(I_AUTHENTICATION_MCP_ACCESS_SERVICE, InstantiationType::Delayed);
}

impl AuthenticationMcpAccessService {
    /// A chave de `mcpserver-${providerId}-${accountName}` no armazenamento.
    fn storage_key(provider_id: &str, account_name: &str) -> String {
        format!("mcpserver-{provider_id}-{account_name}")
    }

    /// A lista de `trustedMcpAuthAccess` que vale para o provedor: o próprio array, ou o
    /// `dicionário[provedor]`, ou vazia.
    fn trusted_mcp_server_ids(&self, provider_id: &str) -> Vec<String> {
        trusted_auth_access_ids(self.product_service.trusted_mcp_auth_access(), provider_id)
    }

    fn is_access_allowed_for(&self, provider_id: &str, account_name: &str, mcp_server_id: &str, mcp_server_url: Option<&str>) -> Option<bool> {
        if self.trusted_mcp_server_ids(provider_id).iter().any(|id| id == mcp_server_id) {
            return Some(true);
        }

        let allow_list = self.read_allowed_mcp_servers(provider_id, account_name);
        let mcp_server_data = allow_list.iter().find(|mcp_server| mcp_server.id == mcp_server_id)?;
        // A grant is bound to the URL it was made for: if the server now has a different URL, the user
        // must re-consent before a token is released to it.
        if mcp_server_url.is_some() && !urls_equal(mcp_server_data.url.as_deref(), mcp_server_url) {
            return None;
        }
        // This property didn't exist on this data previously, inclusion in the list at all indicates allowance
        Some(mcp_server_data.allowed.unwrap_or(true))
    }

    fn fire_did_change(&self, provider_id: &str, account_name: &str) {
        self.on_did_change_mcp_session_access.fire(McpSessionAccessChange { provider_id: provider_id.to_owned(), account_name: account_name.to_owned() });
    }
}

impl IAuthenticationMcpAccessService for AuthenticationMcpAccessService {
    fn on_did_change_mcp_session_access(&self) -> Event<McpSessionAccessChange> {
        self.on_did_change_mcp_session_access.event()
    }

    fn is_access_allowed(&self, provider_id: &str, account_name: &str, mcp_server_id: &str) -> Option<bool> {
        self.is_access_allowed_for(provider_id, account_name, mcp_server_id, None)
    }

    fn is_access_allowed_for_url(&self, provider_id: &str, account_name: &str, mcp_server_id: &str, mcp_server_url: &str) -> Option<bool> {
        self.is_access_allowed_for(provider_id, account_name, mcp_server_id, Some(mcp_server_url))
    }

    fn read_allowed_mcp_servers(&self, provider_id: &str, account_name: &str) -> Vec<AllowedMcpServer> {
        // `trustedMCPServers.find` não existe fora de um array.
        let items = read_stored_items(self.storage_service.as_ref(), &Self::storage_key(provider_id, account_name), "trustedMCPServers.find is not a function");
        let mut trusted_mcp_servers: Vec<AllowedMcpServer> = items.iter().filter_map(AllowedMcpServer::from_json).collect();

        // Add trusted MCP servers from product.json if they're not already in the list
        for mcp_server_id in self.trusted_mcp_server_ids(provider_id) {
            match trusted_mcp_servers.iter_mut().find(|server| server.id == mcp_server_id) {
                None => {
                    // Add new trusted server (name will be set by caller if they have server info)
                    trusted_mcp_servers.push(AllowedMcpServer {
                        name: mcp_server_id.clone(), // Default to ID, caller can update with proper name
                        id: mcp_server_id,
                        allowed: Some(true),
                        trusted: Some(true),
                        ..AllowedMcpServer::default()
                    });
                }
                Some(existing_server) => {
                    // Update existing server to be trusted
                    existing_server.allowed = Some(true);
                    existing_server.trusted = Some(true);
                }
            }
        }

        trusted_mcp_servers
    }

    fn update_allowed_mcp_servers(&self, provider_id: &str, account_name: &str, mcp_servers: &[AllowedMcpServer]) {
        let mut allow_list = self.read_allowed_mcp_servers(provider_id, account_name);
        for mcp_server in mcp_servers {
            match allow_list.iter_mut().find(|e| e.id == mcp_server.id) {
                None => allow_list.push(mcp_server.clone()),
                Some(existing) => {
                    existing.allowed = mcp_server.allowed;
                    // Update name if provided and not already set to a proper name
                    if !mcp_server.name.is_empty() && mcp_server.name != mcp_server.id && existing.name != mcp_server.name {
                        existing.name = mcp_server.name.clone();
                    }
                    // Only overwrite the URL when one is provided, so management toggles (which omit it) keep the binding.
                    if mcp_server.url.is_some() {
                        existing.url = mcp_server.url.clone();
                    }
                    // Preserve agent-host metadata across re-grants; only set when provided so management
                    // toggles (which omit it) don't clear it.
                    if mcp_server.agent_host.is_some() {
                        existing.agent_host = mcp_server.agent_host.clone();
                    }
                }
            }
        }

        // Filter out trusted servers before storing - they should only come from product.json, not user storage
        let user_managed_servers: Vec<Value> = allow_list.iter().filter(|server| server.trusted != Some(true)).map(AllowedMcpServer::to_json).collect();
        self.storage_service.store(
            &Self::storage_key(provider_id, account_name),
            StorageValue::String(json_stringify(&Value::Array(user_managed_servers), None)),
            StorageScope::Application,
            StorageTarget::User,
        );
        self.fire_did_change(provider_id, account_name);
    }

    fn remove_allowed_mcp_servers(&self, provider_id: &str, account_name: &str) {
        self.storage_service.remove(&Self::storage_key(provider_id, account_name), StorageScope::Application);
        self.fire_did_change(provider_id, account_name);
    }
}

impl IDisposable for AuthenticationMcpAccessService {
    fn dispose(&self) {
        self.base.dispose();
    }
}
