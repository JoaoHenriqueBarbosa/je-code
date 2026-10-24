//! Porte de `src/vs/workbench/services/authentication/common/authenticationQuery.ts`: as interfaces da API de
//! consulta de autenticação (provedor, conta, extensão, servidor MCP), e o `IAuthenticationQueryService` com o
//! proxy de instanciação atrasada. A implementação é [`super::authentication_query_service`].
//!
//! Divergências:
//! - **Interfaces são traits, e o que o original devolve como objeto de consulta é `Box<dyn Trait>`**
//!   (`provider(providerId)` devolve um `IProviderQuery`, que o chamador tem sozinho). `extends IBaseQuery` é
//!   o supertrait [`IBaseQuery`], e as propriedades `readonly` (`providerId`, `accountName`, `extensionId`,
//!   `mcpServerId`) são métodos sem argumento que devolvem `&str`.
//! - **Funções de retorno de chamada.** O `forEach(callback)` síncrono recebe `&mut dyn FnMut(...)` e o
//!   `forEachAccount(callback)`, que roda depois de um `await`, recebe `Box<dyn FnMut(&dyn IAccountQuery)>`.
//!   O argumento de objeto de consulta passa por referência (`&dyn Trait`): quem quiser guardá-lo chama os
//!   métodos dele ali.
//! - **Valores.** `boolean | undefined` é `Option<bool>`, `extensionName?: string` é `Option<&str>`,
//!   `includeInternal?: boolean` é `Option<bool>`, `ReadonlyArray<string>` é `&[String]` e `Map<string, string>`
//!   é um [`OrderedMap`] (que mantém a ordem de inserção do `Map`). Os objetos anônimos viram structs:
//!   [`ExtensionUsage`], [`McpServerUsage`], [`EntityCount`], [`PreferencesChange`]. As listas
//!   `{ id; name; allowed?; lastUsed?; trusted? }` de `getAllowedExtensions` e
//!   `{ id; name; allowed?; lastUsed?; trusted?; url?; agentHost? }` de `getAllowedMcpServers` são o
//!   [`AllowedExtension`] e o [`AllowedMcpServer`], que têm exatamente esses campos. O evento
//!   `{ providerId; accountName }` é o [`AccountAccessChange`].
//! - **`'extension' | 'mcpServer'`** é o enum [`EntityType`].
//! - **`clearAllData(confirmation: 'CLEAR_ALL_AUTH_DATA', ...)`** recebe `&str`: o tipo literal do TypeScript
//!   não existe em Rust, e a conferência em tempo de execução que a implementação faz (e que o tipo torna
//!   inalcançável no original) passa a valer.

use je_base::event::Event;
use je_base::map::OrderedMap;
use je_base::promise::Promise;
use je_instantiation::{delayed_proxy, service_id};

use super::authentication::{AccountAccessChange, AllowedExtension, AuthenticationSessionAccount};
use super::authentication_mcp_access_service::AllowedMcpServer;

/// O `recentActivity` de [`IAuthenticationUsageStats`].
#[derive(Clone, Debug, PartialEq)]
pub struct RecentActivity {
    pub account_name: String,
    pub last_used: f64,
    pub usage_count: usize,
}

/// Statistics about authentication usage.
#[derive(Clone, Debug, PartialEq)]
pub struct IAuthenticationUsageStats {
    pub total_sessions: usize,
    pub total_accounts: usize,
    pub recent_activity: Vec<RecentActivity>,
}

/// Information about entities using authentication within a provider.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IActiveEntities {
    pub extensions: Vec<String>,
    pub mcp_servers: Vec<String>,
}

/// `'extension' | 'mcpServer'`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EntityType {
    Extension,
    McpServer,
}

/// O `{ extensions: number; mcpServers: number; total: number }` de `getEntityCount`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EntityCount {
    pub extensions: usize,
    pub mcp_servers: usize,
    pub total: usize,
}

/// O registro de uso de `IAccountExtensionQuery.getUsage`.
#[derive(Clone, Debug, PartialEq)]
pub struct ExtensionUsage {
    pub extension_id: String,
    pub extension_name: String,
    pub scopes: Vec<String>,
    pub last_used: f64,
}

/// O registro de uso de `IAccountMcpServerQuery.getUsage`.
#[derive(Clone, Debug, PartialEq)]
pub struct McpServerUsage {
    pub mcp_server_id: String,
    pub mcp_server_name: String,
    pub scopes: Vec<String>,
    pub last_used: f64,
}

/// O `{ providerId; entityType; entityIds }` de `onDidChangePreferences`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PreferencesChange {
    pub provider_id: String,
    pub entity_type: EntityType,
    pub entity_ids: Vec<String>,
}

/// Base query interface with common properties.
pub trait IBaseQuery {
    fn provider_id(&self) -> &str;
}

/// Query interface for operations on a specific account within a provider.
pub trait IAccountQuery: IBaseQuery {
    fn account_name(&self) -> &str;

    /// Get operations for a specific extension on this account.
    fn extension(&self, extension_id: &str) -> Box<dyn IAccountExtensionQuery>;

    /// Get operations for a specific MCP server on this account.
    fn mcp_server(&self, mcp_server_id: &str) -> Box<dyn IAccountMcpServerQuery>;

    /// Get operations for all extensions on this account.
    fn extensions(&self) -> Box<dyn IAccountExtensionsQuery>;

    /// Get operations for all MCP servers on this account.
    fn mcp_servers(&self) -> Box<dyn IAccountMcpServersQuery>;

    /// Get operations for all entities (extensions and MCP servers) on this account, for type-agnostic
    /// operations.
    fn entities(&self) -> Box<dyn IAccountEntitiesQuery>;

    /// Remove all authentication data for this account.
    fn remove(&self);
}

/// Query interface for operations on a specific extension within a specific account.
pub trait IAccountExtensionQuery: IBaseQuery {
    fn account_name(&self) -> &str;
    fn extension_id(&self) -> &str;

    /// Check if this extension is allowed to access this account. `Some(true)` if allowed, `Some(false)` if
    /// denied, `None` if not yet decided.
    fn is_access_allowed(&self) -> Option<bool>;

    /// Set access permission for this extension on this account.
    ///
    /// - `allowed`: true to allow, false to deny access.
    /// - `extension_name`: optional extension name for display purposes.
    fn set_access_allowed(&self, allowed: bool, extension_name: Option<&str>);

    /// Add usage record for this extension on this account.
    ///
    /// - `scopes`: the scopes that were used.
    /// - `extension_name`: the extension name for display purposes.
    fn add_usage(&self, scopes: &[String], extension_name: &str);

    /// Get usage history for this extension on this account.
    fn get_usage(&self) -> Vec<ExtensionUsage>;

    /// Remove all usage data for this extension on this account.
    fn remove_usage(&self);

    /// Set this account as the preferred account for this extension.
    fn set_as_preferred(&self);

    /// Check if this account is the preferred account for this extension.
    fn is_preferred(&self) -> bool;

    /// Check if this extension is trusted (defined in product.json).
    fn is_trusted(&self) -> bool;
}

/// Query interface for operations on a specific MCP server within a specific account.
pub trait IAccountMcpServerQuery: IBaseQuery {
    fn account_name(&self) -> &str;
    fn mcp_server_id(&self) -> &str;

    /// Check if this MCP server is allowed to access this account. `Some(true)` if allowed, `Some(false)` if
    /// denied, `None` if not yet decided.
    fn is_access_allowed(&self) -> Option<bool>;

    /// Set access permission for this MCP server on this account.
    ///
    /// - `allowed`: true to allow, false to deny access.
    /// - `mcp_server_name`: optional MCP server name for display purposes.
    fn set_access_allowed(&self, allowed: bool, mcp_server_name: Option<&str>);

    /// Add usage record for this MCP server on this account.
    ///
    /// - `scopes`: the scopes that were used.
    /// - `mcp_server_name`: the MCP server name for display purposes.
    fn add_usage(&self, scopes: &[String], mcp_server_name: &str);

    /// Get usage history for this MCP server on this account.
    fn get_usage(&self) -> Vec<McpServerUsage>;

    /// Remove all usage data for this MCP server on this account.
    fn remove_usage(&self);

    /// Set this account as the preferred account for this MCP server.
    fn set_as_preferred(&self);

    /// Check if this account is the preferred account for this MCP server.
    fn is_preferred(&self) -> bool;

    /// Check if this MCP server is trusted (defined in product.json).
    fn is_trusted(&self) -> bool;
}

/// Query interface for operations on all extensions within a specific account.
pub trait IAccountExtensionsQuery: IBaseQuery {
    fn account_name(&self) -> &str;

    /// Get all extensions that have access to this account with their trusted state.
    fn get_allowed_extensions(&self) -> Vec<AllowedExtension>;

    /// Grant access to this account for all specified extensions.
    fn allow_access(&self, extension_ids: &[String]);

    /// Remove access to this account for all specified extensions.
    fn remove_access(&self, extension_ids: &[String]);

    /// Execute a callback for each extension that has used this account.
    fn for_each(&self, callback: &mut dyn FnMut(&dyn IAccountExtensionQuery));
}

/// Query interface for operations on all MCP servers within a specific account.
pub trait IAccountMcpServersQuery: IBaseQuery {
    fn account_name(&self) -> &str;

    /// Get all MCP servers that have access to this account with their trusted state.
    fn get_allowed_mcp_servers(&self) -> Vec<AllowedMcpServer>;

    /// Grant access to this account for all specified MCP servers.
    fn allow_access(&self, mcp_server_ids: &[String]);

    /// Remove access to this account for all specified MCP servers.
    fn remove_access(&self, mcp_server_ids: &[String]);

    /// Execute a callback for each MCP server that has used this account.
    fn for_each(&self, callback: &mut dyn FnMut(&dyn IAccountMcpServerQuery));
}

/// Query interface for type-agnostic operations on all entities (extensions and MCP servers) within a
/// specific account.
pub trait IAccountEntitiesQuery: IBaseQuery {
    fn account_name(&self) -> &str;

    /// Check if this account has been used by any entity (extension or MCP server).
    fn has_any_usage(&self) -> bool;

    /// Get the total count of entities that have used this account.
    fn get_entity_count(&self) -> EntityCount;

    /// Remove access to this account for all entities (extensions and MCP servers).
    fn remove_all_access(&self);

    /// Execute a callback for each entity that has used this account.
    fn for_each(&self, callback: &mut dyn FnMut(&str, EntityType));
}

/// Query interface for operations on a specific extension within a provider.
pub trait IProviderExtensionQuery: IBaseQuery {
    fn extension_id(&self) -> &str;

    /// Get the preferred account for this extension within this provider, or `None` if no preference is set.
    fn get_preferred_account(&self) -> Option<String>;

    /// Set the preferred account for this extension within this provider.
    fn set_preferred_account(&self, account: &AuthenticationSessionAccount);

    /// Remove the account preference for this extension within this provider.
    fn remove_account_preference(&self);
}

/// Query interface for operations on a specific MCP server within a provider.
pub trait IProviderMcpServerQuery: IBaseQuery {
    fn mcp_server_id(&self) -> &str;

    /// Get the last used account for this MCP server within a provider.
    fn get_last_used_account(&self) -> Promise<Option<String>>;

    /// Get the preferred account for this MCP server within a provider, or `None` if no preference is set.
    fn get_preferred_account(&self) -> Option<String>;

    /// Set the preferred account for this MCP server within a provider.
    fn set_preferred_account(&self, account: &AuthenticationSessionAccount);

    /// Remove the account preference for this MCP server within a provider.
    fn remove_account_preference(&self);

    /// Get all accounts that this MCP server has used within this provider.
    fn get_used_accounts(&self) -> Promise<Vec<String>>;
}

/// Query interface for provider-scoped operations.
pub trait IProviderQuery: IBaseQuery {
    /// Get operations for a specific account within this provider.
    fn account(&self, account_name: &str) -> Box<dyn IAccountQuery>;

    /// Get operations for a specific extension within this provider.
    fn extension(&self, extension_id: &str) -> Box<dyn IProviderExtensionQuery>;

    /// Get operations for a specific MCP server within this provider.
    fn mcp_server(&self, mcp_server_id: &str) -> Box<dyn IProviderMcpServerQuery>;

    /// Get information about active entities (extensions and MCP servers) within this provider.
    fn get_active_entities(&self) -> Promise<IActiveEntities>;

    /// Get all account names for this provider.
    fn get_account_names(&self) -> Promise<Vec<String>>;

    /// Get usage statistics for this provider.
    fn get_usage_stats(&self) -> Promise<IAuthenticationUsageStats>;

    /// Execute a callback for each account in this provider.
    fn for_each_account(&self, callback: Box<dyn FnMut(&dyn IAccountQuery)>) -> Promise<()>;
}

/// Query interface for extension-scoped operations (cross-provider).
pub trait IExtensionQuery {
    fn extension_id(&self) -> &str;

    /// Get all providers where this extension has access.
    ///
    /// - `include_internal`: whether to include internal providers (starting with
    ///   `INTERNAL_AUTH_PROVIDER_PREFIX`).
    fn get_providers_with_access(&self, include_internal: Option<bool>) -> Promise<Vec<String>>;

    /// Get account preferences for this extension across all providers, as a map of provider ID to account
    /// name.
    ///
    /// - `include_internal`: whether to include internal providers (starting with
    ///   `INTERNAL_AUTH_PROVIDER_PREFIX`).
    fn get_all_account_preferences(&self, include_internal: Option<bool>) -> OrderedMap<String, String>;

    /// Get operations for this extension within a specific provider.
    fn provider(&self, provider_id: &str) -> Box<dyn IProviderExtensionQuery>;
}

/// Query interface for MCP server-scoped operations (cross-provider).
pub trait IMcpServerQuery {
    fn mcp_server_id(&self) -> &str;

    /// Get all providers where this MCP server has access.
    ///
    /// - `include_internal`: whether to include internal providers (starting with
    ///   `INTERNAL_AUTH_PROVIDER_PREFIX`).
    fn get_providers_with_access(&self, include_internal: Option<bool>) -> Promise<Vec<String>>;

    /// Get account preferences for this MCP server across all providers, as a map of provider ID to account
    /// name.
    ///
    /// - `include_internal`: whether to include internal providers (starting with
    ///   `INTERNAL_AUTH_PROVIDER_PREFIX`).
    fn get_all_account_preferences(&self, include_internal: Option<bool>) -> OrderedMap<String, String>;

    /// Get operations for this MCP server within a specific provider.
    fn provider(&self, provider_id: &str) -> Box<dyn IProviderMcpServerQuery>;
}

/// Main authentication query service interface.
pub trait IAuthenticationQueryService {
    /// Fires when authentication preferences change.
    fn on_did_change_preferences(&self) -> Event<PreferencesChange>;

    /// Fires when authentication access permissions change.
    fn on_did_change_access(&self) -> Event<AccountAccessChange>;

    /// Get operations for a specific authentication provider.
    fn provider(&self, provider_id: &str) -> Box<dyn IProviderQuery>;

    /// Get operations for a specific extension across all providers.
    fn extension(&self, extension_id: &str) -> Box<dyn IExtensionQuery>;

    /// Get operations for a specific MCP server across all providers.
    fn mcp_server(&self, mcp_server_id: &str) -> Box<dyn IMcpServerQuery>;

    /// Get all available provider IDs (sem os provedores internos).
    fn get_provider_ids(&self) -> Vec<String>;

    /// Clear all authentication data (for testing/debugging purposes).
    ///
    /// - `confirmation`: must be `CLEAR_ALL_AUTH_DATA` to confirm.
    /// - `include_internal`: whether to include internal providers (defaults to true for complete clearing).
    fn clear_all_data(&self, confirmation: &str, include_internal: Option<bool>) -> Promise<()>;
}

delayed_proxy!(pub AuthenticationQueryServiceProxy for dyn IAuthenticationQueryService {
    methods {
        fn provider(&self, provider_id: &str) -> Box<dyn IProviderQuery>;
        fn extension(&self, extension_id: &str) -> Box<dyn IExtensionQuery>;
        fn mcp_server(&self, mcp_server_id: &str) -> Box<dyn IMcpServerQuery>;
        fn get_provider_ids(&self) -> Vec<String>;
        fn clear_all_data(&self, confirmation: &str, include_internal: Option<bool>) -> Promise<()>;
    }
    events {
        fn on_did_change_preferences(&self) -> PreferencesChange;
        fn on_did_change_access(&self) -> AccountAccessChange;
    }
});

service_id!(pub I_AUTHENTICATION_QUERY_SERVICE: dyn IAuthenticationQueryService = "IAuthenticationQueryService", proxy AuthenticationQueryServiceProxy);
