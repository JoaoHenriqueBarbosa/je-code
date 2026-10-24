//! Porte de `src/vs/workbench/services/authentication/browser/authenticationMcpUsageService.ts`: o serviço que
//! guarda, por provedor e conta, quais servidores MCP usaram a conta e quando, e que mantém o cache de "este
//! servidor MCP usa autenticação". É só armazenamento, sem nada do runtime de MCP.
//!
//! O miolo é o [`UsageStore`] de [`super::authentication_usage_service`]: no original esta classe repete o
//! `AuthenticationUsageService` linha a linha, e muda só a chave de armazenamento
//! (`${providerId}-${accountName}-mcpserver-usages`), os nomes dos campos (`mcpServerId`, `mcpServerName`), a
//! propriedade do produto (`trustedMcpAuthAccess`), e que a fila não é registrada para descarte e não há a
//! flag `_disposed` (aqui a flag existe no miolo e nunca é ligada).
//!
//! Divergências:
//! - **Injeção.** Os parâmetros decorados são a tupla `Deps` do [`Injectable`]; a classe herda de
//!   `Disposable`, aqui um `Disposable` guardado no campo `base`.
//! - **`registerSingleton(..., InstantiationType.Delayed)`** vira [`register`]; o proxy de instanciação
//!   atrasada é o [`AuthenticationMcpUsageServiceProxy`].
//! - **`IAuthenticationMcpUsage.scopes?: string[]`** é `Option<Vec<String>>`; `addAccountUsage` recebe
//!   `&[String]` (`ReadonlyArray<string>`, que a interface não deixa ser `undefined`).
//! - As demais divergências (funções `async`, `JSON.parse`, `Date.now()`) são as de
//!   [`super::authentication_usage_service`].

use std::rc::Rc;

use je_base::lifecycle::{Disposable, IDisposable};
use je_base::promise::Promise;
use je_instantiation::{InstantiationType, Injectable, delayed_proxy, register_singleton, service_id, service_impl};
use je_platform::log::LogService;
use je_platform::product::IProductService;
use je_platform::storage::StorageService;

use super::authentication::IAuthenticationService;
use super::authentication_usage_service::{UsageKind, UsageRecord, UsageStore};

/// O uso por MCP: `${providerId}-${accountName}-mcpserver-usages`.
const MCP_USAGE: UsageKind = UsageKind { key_suffix: "mcpserver-usages", id_field: "mcpServerId", name_field: "mcpServerName" };

/// `IAuthenticationMcpUsage`.
#[derive(Clone, Debug, PartialEq)]
pub struct IAuthenticationMcpUsage {
    pub mcp_server_id: String,
    pub mcp_server_name: String,
    pub last_used: f64,
    pub scopes: Option<Vec<String>>,
}

impl From<UsageRecord> for IAuthenticationMcpUsage {
    fn from(record: UsageRecord) -> Self {
        Self { mcp_server_id: record.id, mcp_server_name: record.name, last_used: record.last_used, scopes: record.scopes }
    }
}

/// `IAuthenticationMcpUsageService`.
pub trait IAuthenticationMcpUsageService {
    /// Initializes the cache of MCP servers that use authentication. Ideally used in a contribution that can
    /// be run eventually after the workspace is loaded.
    fn initialize_usage_cache(&self) -> Promise<()>;

    /// Checks if an MCP server uses authentication.
    ///
    /// - `mcp_server_id`: the id of the MCP server to check.
    fn has_used_auth(&self, mcp_server_id: &str) -> Promise<bool>;

    /// Reads the usages for an account.
    ///
    /// - `provider_id`: the id of the authentication provider to get usages for.
    /// - `account_name`: the name of the account to get usages for.
    fn read_account_usages(&self, provider_id: &str, account_name: &str) -> Vec<IAuthenticationMcpUsage>;

    /// Removes the usages of an account.
    ///
    /// - `provider_id`: the id of the authentication provider to get usages for.
    /// - `account_name`: the name of the account to get usages for.
    fn remove_account_usage(&self, provider_id: &str, account_name: &str);

    /// Adds a usage for an account.
    ///
    /// - `provider_id`: the id of the authentication provider to get usages for.
    /// - `account_name`: the name of the account to get usages for.
    /// - `mcp_server_id`: the id of the MCP server to add a usage for.
    /// - `mcp_server_name`: the name of the MCP server to add a usage for.
    fn add_account_usage(&self, provider_id: &str, account_name: &str, scopes: &[String], mcp_server_id: &str, mcp_server_name: &str);
}

delayed_proxy!(pub AuthenticationMcpUsageServiceProxy for dyn IAuthenticationMcpUsageService {
    methods {
        fn initialize_usage_cache(&self) -> Promise<()>;
        fn has_used_auth(&self, mcp_server_id: &str) -> Promise<bool>;
        fn read_account_usages(&self, provider_id: &str, account_name: &str) -> Vec<IAuthenticationMcpUsage>;
        fn remove_account_usage(&self, provider_id: &str, account_name: &str);
        fn add_account_usage(&self, provider_id: &str, account_name: &str, scopes: &[String], mcp_server_id: &str, mcp_server_name: &str);
    }
    events {}
});

service_id!(pub I_AUTHENTICATION_MCP_USAGE_SERVICE: dyn IAuthenticationMcpUsageService = "IAuthenticationMcpUsageService", proxy AuthenticationMcpUsageServiceProxy);

/// `AuthenticationMcpUsageService`.
pub struct AuthenticationMcpUsageService {
    base: Rc<Disposable>,
    store: Rc<UsageStore>,
}

impl Injectable for AuthenticationMcpUsageService {
    type Args = ();
    type Deps = (Rc<dyn StorageService>, Rc<dyn IAuthenticationService>, Rc<dyn LogService>, Rc<dyn IProductService>);

    fn create(_: (), (storage_service, authentication_service, log_service, product_service): Self::Deps) -> Rc<Self> {
        let base = Disposable::new();
        // If an MCP server is listed in `trustedMcpAuthAccess` we should consider it as using auth
        let store = UsageStore::new(MCP_USAGE, storage_service, authentication_service, log_service, product_service.trusted_mcp_auth_access());
        base.register(store.watch_providers());
        Rc::new(Self { base, store })
    }
}

service_impl!(AuthenticationMcpUsageService => dyn IAuthenticationMcpUsageService, disposable);

/// `registerSingleton(IAuthenticationMcpUsageService, AuthenticationMcpUsageService, InstantiationType.Delayed)`.
pub fn register() {
    register_singleton::<AuthenticationMcpUsageService>(I_AUTHENTICATION_MCP_USAGE_SERVICE, InstantiationType::Delayed);
}

impl IAuthenticationMcpUsageService for AuthenticationMcpUsageService {
    fn initialize_usage_cache(&self) -> Promise<()> {
        self.store.initialize_cache()
    }

    fn has_used_auth(&self, mcp_server_id: &str) -> Promise<bool> {
        self.store.has_used(mcp_server_id)
    }

    fn read_account_usages(&self, provider_id: &str, account_name: &str) -> Vec<IAuthenticationMcpUsage> {
        self.store.read(provider_id, account_name).into_iter().map(IAuthenticationMcpUsage::from).collect()
    }

    fn remove_account_usage(&self, provider_id: &str, account_name: &str) {
        self.store.remove(provider_id, account_name);
    }

    fn add_account_usage(&self, provider_id: &str, account_name: &str, scopes: &[String], mcp_server_id: &str, mcp_server_name: &str) {
        self.store.add(provider_id, account_name, Some(scopes), mcp_server_id, mcp_server_name);
    }
}

impl IDisposable for AuthenticationMcpUsageService {
    fn dispose(&self) {
        self.base.dispose();
    }
}
