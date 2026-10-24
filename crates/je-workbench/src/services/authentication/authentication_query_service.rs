//! Porte de `src/vs/workbench/services/authentication/browser/authenticationQueryService.ts`: a API de
//! consulta de autenticação, uma fachada encadeada (`provider(id).account(name).extension(id)...`) sobre os
//! serviços de acesso, de uso, de preferência de conta e de provedores.
//!
//! Divergências:
//! - **Injeção.** Os parâmetros decorados são a tupla `Deps` do [`Injectable`], na ordem do original. Os
//!   campos `public readonly` da classe (os serviços e o `logService`) são [`QueryServices`], que o serviço
//!   guarda em `services`. Os objetos de consulta (`BaseQuery` e as classes que herdam dela) guardam um
//!   `Rc<QueryServices>` no lugar da referência ao `AuthenticationQueryService` que o original guarda:
//!   é o mesmo acesso aos mesmos serviços, sem ciclo de referência. `BaseQuery` é a struct [`BaseQuery`]
//!   no campo `base` de cada consulta, e o `abstract class` com o `providerId` vira o [`IBaseQuery`]
//!   que um macro local implementa para todas.
//! - **`registerSingleton(..., InstantiationType.Delayed)`** vira [`register`]; o proxy de instanciação
//!   atrasada é o [`AuthenticationQueryServiceProxy`] (em `authentication_query`).
//! - **Funções `async`** são `async_fn`. O `try { ... } catch { ... }` que engloba o `await getAccounts(...)` e
//!   o trecho síncrono depois dele é [`with_accounts`]: a rejeição de `getAccounts` e um pânico (um `throw`)
//!   do trecho viram `Err`, que cada método trata como o `catch` do original (valor padrão, ou seguir para o
//!   próximo provedor).
//! - **`Set<string>`** (`forEach` das consultas de extensões e de servidores) é [`ordered_unique`]: a ordem
//!   de inserção sem repetição. `Math.max(...lista)` é a dobra com `f64::max`.
//! - **`getProviderIds(includeInternal?)`**: o parâmetro opcional existe na classe e não na interface; aqui o
//!   trait tem só a forma da interface (que não inclui os internos) e a classe usa
//!   [`QueryServices::provider_ids`], que as consultas entre provedores (`ExtensionQuery`, `McpServerQuery`)
//!   também usam no lugar do laço com `continue` do original, que filtra a mesma coisa.
//! - **O que o original repete** em `ExtensionQuery`/`McpServerQuery` (`getProvidersWithAccess`,
//!   `getAllAccountPreferences`) e na remoção dos dados de uma conta (`AccountQuery.remove` e
//!   `clearAllData`) existe uma vez ([`providers_with_access`], [`all_account_preferences`],
//!   [`QueryServices::remove_account_data`]), com a diferença virando parâmetro.

use std::rc::Rc;

use je_base::async_::async_fn;
use je_base::errors::{self, Error};
use je_base::event::{Emitter, Event};
use je_base::lifecycle::{Disposable, IDisposable};
use je_base::map::OrderedMap;
use je_base::promise::Promise;
use je_extensions::manifest::ExtensionIdentifier;
use je_instantiation::{InstantiationType, Injectable, register_singleton, service_impl};
use je_platform::log::{LogArg, LogMessage, LogService, Logger};

use super::authentication::{
    AllowedExtension, AuthenticationSessionAccount, IAuthenticationExtensionsService, IAuthenticationService, INTERNAL_AUTH_PROVIDER_PREFIX,
};
use super::authentication::AccountAccessChange;
use super::authentication_access_service::IAuthenticationAccessService;
use super::authentication_mcp_access_service::{AllowedMcpServer, IAuthenticationMcpAccessService};
use super::authentication_mcp_service::IAuthenticationMcpService;
use super::authentication_mcp_usage_service::IAuthenticationMcpUsageService;
use super::authentication_query::{
    EntityCount, EntityType, ExtensionUsage, IAccountEntitiesQuery, IAccountExtensionQuery, IAccountExtensionsQuery, IAccountMcpServerQuery,
    IAccountMcpServersQuery, IAccountQuery, IActiveEntities, IAuthenticationQueryService, IAuthenticationUsageStats, IBaseQuery, IExtensionQuery,
    IMcpServerQuery, IProviderExtensionQuery, IProviderMcpServerQuery, IProviderQuery, I_AUTHENTICATION_QUERY_SERVICE, McpServerUsage,
    PreferencesChange, RecentActivity,
};
use super::authentication_usage_service::IAuthenticationUsageService;

/// Os serviços de que as consultas dependem: os campos `public readonly` do `AuthenticationQueryService`.
pub struct QueryServices {
    pub authentication_service: Rc<dyn IAuthenticationService>,
    pub authentication_usage_service: Rc<dyn IAuthenticationUsageService>,
    pub authentication_mcp_usage_service: Rc<dyn IAuthenticationMcpUsageService>,
    pub authentication_access_service: Rc<dyn IAuthenticationAccessService>,
    pub authentication_mcp_access_service: Rc<dyn IAuthenticationMcpAccessService>,
    pub authentication_extensions_service: Rc<dyn IAuthenticationExtensionsService>,
    pub authentication_mcp_service: Rc<dyn IAuthenticationMcpService>,
    pub log_service: Rc<dyn LogService>,
}

impl QueryServices {
    /// Os ids dos provedores, sem os internos (prefixo `__`) a menos que `include_internal`.
    pub fn provider_ids(&self, include_internal: bool) -> Vec<String> {
        self.authentication_service
            .get_provider_ids()
            .into_iter()
            // Filter out internal providers unless explicitly included
            .filter(|provider_id| include_internal || !provider_id.starts_with(INTERNAL_AUTH_PROVIDER_PREFIX))
            .collect()
    }

    /// Remove o acesso e o uso, de extensões e de servidores MCP, de uma conta.
    pub fn remove_account_data(&self, provider_id: &str, account_name: &str) {
        // Remove all extension access and usage data
        self.authentication_access_service.remove_allowed_extensions(provider_id, account_name);
        self.authentication_usage_service.remove_account_usage(provider_id, account_name);

        // Remove all MCP server access and usage data
        self.authentication_mcp_access_service.remove_allowed_mcp_servers(provider_id, account_name);
        self.authentication_mcp_usage_service.remove_account_usage(provider_id, account_name);
    }
}

/// O `try { const accounts = await getAccounts(providerId); body(accounts) } catch { ... }` das consultas: `Err`
/// se `getAccounts` rejeita ou se `body` lança.
async fn with_accounts<R>(
    services: &QueryServices,
    provider_id: &str,
    body: impl FnOnce(&[AuthenticationSessionAccount]) -> R,
) -> Result<R, Error> {
    let accounts = services.authentication_service.get_accounts(provider_id).await?;
    errors::catch(|| body(&accounts))
}

/// Os ids sem repetição, na ordem em que chegam (o `Set<string>` do original).
fn ordered_unique(ids: impl IntoIterator<Item = String>) -> Vec<String> {
    let mut unique: Vec<String> = Vec::new();
    for id in ids {
        if !unique.contains(&id) {
            unique.push(id);
        }
    }
    unique
}

/// O `Math.max(...valores)` com piso: o maior dos valores, ou `None` sem nenhum.
fn max_of(values: impl Iterator<Item = f64>) -> Option<f64> {
    values.fold(None, |max, value| Some(max.map_or(value, |max: f64| max.max(value))))
}

/// `getProvidersWithAccess` de `ExtensionQuery` e de `McpServerQuery`: os provedores em que alguma conta
/// permite o acesso (`is_access_allowed` devolve `Some(true)`). Um provedor que dá erro é ignorado.
fn providers_with_access(
    services: Rc<QueryServices>,
    include_internal: bool,
    is_access_allowed: impl Fn(&QueryServices, &str, &str) -> Option<bool> + 'static,
) -> Promise<Vec<String>> {
    async_fn(async move {
        let mut providers_with_access = Vec::new();
        for provider_id in services.provider_ids(include_internal) {
            let has_access = with_accounts(&services, &provider_id, |accounts| {
                accounts.iter().any(|account| is_access_allowed(&services, &provider_id, &account.label) == Some(true))
            })
            .await;

            if matches!(has_access, Ok(true)) {
                providers_with_access.push(provider_id);
            }
        }
        Ok(providers_with_access)
    })
}

/// `getAllAccountPreferences` de `ExtensionQuery` e de `McpServerQuery`: a conta preferida em cada provedor
/// que tem uma.
fn all_account_preferences(
    services: &QueryServices,
    include_internal: bool,
    get_account_preference: impl Fn(&str) -> Option<String>,
) -> OrderedMap<String, String> {
    let mut preferences = OrderedMap::new();
    for provider_id in services.provider_ids(include_internal) {
        if let Some(preferred_account) = get_account_preference(&provider_id).filter(|account| !account.is_empty()) {
            preferences.insert(provider_id, preferred_account);
        }
    }
    preferences
}

/// Base implementation for query interfaces.
#[derive(Clone)]
pub struct BaseQuery {
    provider_id: String,
    services: Rc<QueryServices>,
}

impl BaseQuery {
    fn new(provider_id: &str, services: &Rc<QueryServices>) -> Self {
        Self { provider_id: provider_id.to_owned(), services: services.clone() }
    }

    fn account(&self, account_name: &str) -> AccountQuery {
        AccountQuery { base: self.clone(), account_name: account_name.to_owned() }
    }

    fn account_extension(&self, account_name: &str, extension_id: &str) -> AccountExtensionQuery {
        AccountExtensionQuery { base: self.clone(), account_name: account_name.to_owned(), extension_id: extension_id.to_owned() }
    }

    fn account_mcp_server(&self, account_name: &str, mcp_server_id: &str) -> AccountMcpServerQuery {
        AccountMcpServerQuery { base: self.clone(), account_name: account_name.to_owned(), mcp_server_id: mcp_server_id.to_owned() }
    }

    fn account_extensions(&self, account_name: &str) -> AccountExtensionsQuery {
        AccountExtensionsQuery { base: self.clone(), account_name: account_name.to_owned() }
    }

    fn account_mcp_servers(&self, account_name: &str) -> AccountMcpServersQuery {
        AccountMcpServersQuery { base: self.clone(), account_name: account_name.to_owned() }
    }

    fn account_entities(&self, account_name: &str) -> AccountEntitiesQuery {
        AccountEntitiesQuery { base: self.clone(), account_name: account_name.to_owned() }
    }

    fn provider_extension(&self, extension_id: &str) -> ProviderExtensionQuery {
        ProviderExtensionQuery { base: self.clone(), extension_id: extension_id.to_owned() }
    }

    fn provider_mcp_server(&self, mcp_server_id: &str) -> ProviderMcpServerQuery {
        ProviderMcpServerQuery { base: self.clone(), mcp_server_id: mcp_server_id.to_owned() }
    }
}

/// `{ label: accountName, id: accountName }`: a conta que `setAsPreferred` grava.
fn account_named(account_name: &str) -> AuthenticationSessionAccount {
    AuthenticationSessionAccount::new(account_name, account_name)
}

/// O nome que `setAccessAllowed` grava: `extensionName || this.extensionId`.
fn name_or_id<'a>(name: Option<&'a str>, id: &'a str) -> &'a str {
    name.filter(|name| !name.is_empty()).unwrap_or(id)
}

macro_rules! impl_base_query {
    ($($query:ty),* $(,)?) => {
        $(
            impl IBaseQuery for $query {
                fn provider_id(&self) -> &str {
                    &self.base.provider_id
                }
            }
        )*
    };
}

/// Implementation of account-extension query operations.
struct AccountExtensionQuery {
    base: BaseQuery,
    account_name: String,
    extension_id: String,
}

impl IAccountExtensionQuery for AccountExtensionQuery {
    fn account_name(&self) -> &str {
        &self.account_name
    }

    fn extension_id(&self) -> &str {
        &self.extension_id
    }

    fn is_access_allowed(&self) -> Option<bool> {
        self.base.services.authentication_access_service.is_access_allowed(&self.base.provider_id, &self.account_name, &self.extension_id)
    }

    fn set_access_allowed(&self, allowed: bool, extension_name: Option<&str>) {
        self.base.services.authentication_access_service.update_allowed_extensions(
            &self.base.provider_id,
            &self.account_name,
            &[AllowedExtension {
                id: self.extension_id.clone(),
                name: name_or_id(extension_name, &self.extension_id).to_owned(),
                allowed: Some(allowed),
                ..AllowedExtension::default()
            }],
        );
    }

    fn add_usage(&self, scopes: &[String], extension_name: &str) {
        self.base.services.authentication_usage_service.add_account_usage(
            &self.base.provider_id,
            &self.account_name,
            Some(scopes),
            &self.extension_id,
            extension_name,
        );
    }

    fn get_usage(&self) -> Vec<ExtensionUsage> {
        let all_usages = self.base.services.authentication_usage_service.read_account_usages(&self.base.provider_id, &self.account_name);
        let extension_key = ExtensionIdentifier::to_key(&self.extension_id);
        all_usages
            .into_iter()
            .filter(|usage| usage.extension_id == extension_key)
            .map(|usage| ExtensionUsage {
                extension_id: usage.extension_id,
                extension_name: usage.extension_name,
                scopes: usage.scopes.unwrap_or_default(),
                last_used: usage.last_used,
            })
            .collect()
    }

    fn remove_usage(&self) {
        // Get current usages, filter out this extension, and store the rest
        let services = &self.base.services;
        let all_usages = services.authentication_usage_service.read_account_usages(&self.base.provider_id, &self.account_name);
        let filtered_usages: Vec<_> = all_usages.into_iter().filter(|usage| usage.extension_id != self.extension_id).collect();

        // Clear all usages and re-add the filtered ones
        services.authentication_usage_service.remove_account_usage(&self.base.provider_id, &self.account_name);
        for usage in filtered_usages {
            services.authentication_usage_service.add_account_usage(
                &self.base.provider_id,
                &self.account_name,
                Some(&usage.scopes.unwrap_or_default()),
                &usage.extension_id,
                &usage.extension_name,
            );
        }
    }

    fn set_as_preferred(&self) {
        self.base.services.authentication_extensions_service.update_account_preference(
            &self.extension_id,
            &self.base.provider_id,
            &account_named(&self.account_name),
        );
    }

    fn is_preferred(&self) -> bool {
        let preferred_account = self.base.services.authentication_extensions_service.get_account_preference(&self.extension_id, &self.base.provider_id);
        preferred_account.as_deref() == Some(self.account_name.as_str())
    }

    fn is_trusted(&self) -> bool {
        let allowed_extensions =
            self.base.services.authentication_access_service.read_allowed_extensions(&self.base.provider_id, &self.account_name);
        allowed_extensions.iter().find(|ext| ext.id == self.extension_id).is_some_and(|extension| extension.trusted == Some(true))
    }
}

/// Implementation of account-MCP server query operations.
struct AccountMcpServerQuery {
    base: BaseQuery,
    account_name: String,
    mcp_server_id: String,
}

impl IAccountMcpServerQuery for AccountMcpServerQuery {
    fn account_name(&self) -> &str {
        &self.account_name
    }

    fn mcp_server_id(&self) -> &str {
        &self.mcp_server_id
    }

    fn is_access_allowed(&self) -> Option<bool> {
        self.base.services.authentication_mcp_access_service.is_access_allowed(&self.base.provider_id, &self.account_name, &self.mcp_server_id)
    }

    fn set_access_allowed(&self, allowed: bool, mcp_server_name: Option<&str>) {
        self.base.services.authentication_mcp_access_service.update_allowed_mcp_servers(
            &self.base.provider_id,
            &self.account_name,
            &[AllowedMcpServer {
                id: self.mcp_server_id.clone(),
                name: name_or_id(mcp_server_name, &self.mcp_server_id).to_owned(),
                allowed: Some(allowed),
                ..AllowedMcpServer::default()
            }],
        );
    }

    fn add_usage(&self, scopes: &[String], mcp_server_name: &str) {
        self.base.services.authentication_mcp_usage_service.add_account_usage(
            &self.base.provider_id,
            &self.account_name,
            scopes,
            &self.mcp_server_id,
            mcp_server_name,
        );
    }

    fn get_usage(&self) -> Vec<McpServerUsage> {
        let all_usages = self.base.services.authentication_mcp_usage_service.read_account_usages(&self.base.provider_id, &self.account_name);
        all_usages
            .into_iter()
            .filter(|usage| usage.mcp_server_id == self.mcp_server_id)
            .map(|usage| McpServerUsage {
                mcp_server_id: usage.mcp_server_id,
                mcp_server_name: usage.mcp_server_name,
                scopes: usage.scopes.unwrap_or_default(),
                last_used: usage.last_used,
            })
            .collect()
    }

    fn remove_usage(&self) {
        // Get current usages, filter out this MCP server, and store the rest
        let services = &self.base.services;
        let all_usages = services.authentication_mcp_usage_service.read_account_usages(&self.base.provider_id, &self.account_name);
        let filtered_usages: Vec<_> = all_usages.into_iter().filter(|usage| usage.mcp_server_id != self.mcp_server_id).collect();

        // Clear all usages and re-add the filtered ones
        services.authentication_mcp_usage_service.remove_account_usage(&self.base.provider_id, &self.account_name);
        for usage in filtered_usages {
            services.authentication_mcp_usage_service.add_account_usage(
                &self.base.provider_id,
                &self.account_name,
                &usage.scopes.unwrap_or_default(),
                &usage.mcp_server_id,
                &usage.mcp_server_name,
            );
        }
    }

    fn set_as_preferred(&self) {
        self.base.services.authentication_mcp_service.update_account_preference(
            &self.mcp_server_id,
            &self.base.provider_id,
            &account_named(&self.account_name),
        );
    }

    fn is_preferred(&self) -> bool {
        let preferred_account = self.base.services.authentication_mcp_service.get_account_preference(&self.mcp_server_id, &self.base.provider_id);
        preferred_account.as_deref() == Some(self.account_name.as_str())
    }

    fn is_trusted(&self) -> bool {
        let allowed_mcp_servers =
            self.base.services.authentication_mcp_access_service.read_allowed_mcp_servers(&self.base.provider_id, &self.account_name);
        allowed_mcp_servers.iter().find(|server| server.id == self.mcp_server_id).is_some_and(|server| server.trusted == Some(true))
    }
}

/// Implementation of account-extensions query operations.
struct AccountExtensionsQuery {
    base: BaseQuery,
    account_name: String,
}

impl AccountExtensionsQuery {
    /// `allowAccess` e `removeAccess`: grava `allowed` para cada extensão, com o próprio id como nome.
    fn update_access(&self, extension_ids: &[String], allowed: bool) {
        let extensions: Vec<AllowedExtension> = extension_ids
            .iter()
            .map(|id| AllowedExtension { id: id.clone(), name: id.clone(), allowed: Some(allowed), ..AllowedExtension::default() })
            .collect();
        self.base.services.authentication_access_service.update_allowed_extensions(&self.base.provider_id, &self.account_name, &extensions);
    }
}

impl IAccountExtensionsQuery for AccountExtensionsQuery {
    fn account_name(&self) -> &str {
        &self.account_name
    }

    fn get_allowed_extensions(&self) -> Vec<AllowedExtension> {
        let services = &self.base.services;
        let allowed_extensions = services.authentication_access_service.read_allowed_extensions(&self.base.provider_id, &self.account_name);
        let usages = services.authentication_usage_service.read_account_usages(&self.base.provider_id, &self.account_name);

        allowed_extensions
            .into_iter()
            .filter(|ext| ext.allowed != Some(false))
            .map(|ext| {
                // Find the most recent usage for this extension
                let last_used = max_of(usages.iter().filter(|usage| usage.extension_id == ext.id).map(|usage| usage.last_used));

                // Check if trusted through the extension query
                let trusted = self.base.account_extension(&self.account_name, &ext.id).is_trusted();

                AllowedExtension { id: ext.id, name: ext.name, allowed: ext.allowed, last_used, trusted: Some(trusted) }
            })
            .collect()
    }

    fn allow_access(&self, extension_ids: &[String]) {
        self.update_access(extension_ids, true);
    }

    fn remove_access(&self, extension_ids: &[String]) {
        self.update_access(extension_ids, false);
    }

    fn for_each(&self, callback: &mut dyn FnMut(&dyn IAccountExtensionQuery)) {
        let services = &self.base.services;
        let usages = services.authentication_usage_service.read_account_usages(&self.base.provider_id, &self.account_name);
        let allowed_extensions = services.authentication_access_service.read_allowed_extensions(&self.base.provider_id, &self.account_name);

        // Combine extensions from both usage and access data
        let extension_ids = ordered_unique(usages.into_iter().map(|usage| usage.extension_id).chain(allowed_extensions.into_iter().map(|ext| ext.id)));

        for extension_id in extension_ids {
            callback(&self.base.account_extension(&self.account_name, &extension_id));
        }
    }
}

/// Implementation of account-MCP servers query operations.
struct AccountMcpServersQuery {
    base: BaseQuery,
    account_name: String,
}

impl AccountMcpServersQuery {
    /// `allowAccess` e `removeAccess`: grava `allowed` para cada servidor, com o próprio id como nome.
    fn update_access(&self, mcp_server_ids: &[String], allowed: bool) {
        let mcp_servers: Vec<AllowedMcpServer> = mcp_server_ids
            .iter()
            .map(|id| AllowedMcpServer { id: id.clone(), name: id.clone(), allowed: Some(allowed), ..AllowedMcpServer::default() })
            .collect();
        self.base.services.authentication_mcp_access_service.update_allowed_mcp_servers(&self.base.provider_id, &self.account_name, &mcp_servers);
    }
}

impl IAccountMcpServersQuery for AccountMcpServersQuery {
    fn account_name(&self) -> &str {
        &self.account_name
    }

    fn get_allowed_mcp_servers(&self) -> Vec<AllowedMcpServer> {
        self.base
            .services
            .authentication_mcp_access_service
            .read_allowed_mcp_servers(&self.base.provider_id, &self.account_name)
            .into_iter()
            .filter(|server| server.allowed != Some(false))
            .collect()
    }

    fn allow_access(&self, mcp_server_ids: &[String]) {
        self.update_access(mcp_server_ids, true);
    }

    fn remove_access(&self, mcp_server_ids: &[String]) {
        self.update_access(mcp_server_ids, false);
    }

    fn for_each(&self, callback: &mut dyn FnMut(&dyn IAccountMcpServerQuery)) {
        let services = &self.base.services;
        let usages = services.authentication_mcp_usage_service.read_account_usages(&self.base.provider_id, &self.account_name);
        let allowed_mcp_servers = services.authentication_mcp_access_service.read_allowed_mcp_servers(&self.base.provider_id, &self.account_name);

        // Combine MCP servers from both usage and access data
        let mcp_server_ids =
            ordered_unique(usages.into_iter().map(|usage| usage.mcp_server_id).chain(allowed_mcp_servers.into_iter().map(|server| server.id)));

        for mcp_server_id in mcp_server_ids {
            callback(&self.base.account_mcp_server(&self.account_name, &mcp_server_id));
        }
    }
}

/// Implementation of account-entities query operations for type-agnostic operations.
struct AccountEntitiesQuery {
    base: BaseQuery,
    account_name: String,
}

impl IAccountEntitiesQuery for AccountEntitiesQuery {
    fn account_name(&self) -> &str {
        &self.account_name
    }

    fn has_any_usage(&self) -> bool {
        let services = &self.base.services;
        let (provider_id, account_name) = (&self.base.provider_id, &self.account_name);

        // Check extension usage
        if !services.authentication_usage_service.read_account_usages(provider_id, account_name).is_empty() {
            return true;
        }

        // Check MCP server usage
        if !services.authentication_mcp_usage_service.read_account_usages(provider_id, account_name).is_empty() {
            return true;
        }

        // Check extension access
        if services.authentication_access_service.read_allowed_extensions(provider_id, account_name).iter().any(|ext| ext.allowed != Some(false)) {
            return true;
        }

        // Check MCP server access
        services.authentication_mcp_access_service.read_allowed_mcp_servers(provider_id, account_name).iter().any(|server| server.allowed != Some(false))
    }

    fn get_entity_count(&self) -> EntityCount {
        // Use the same logic as getAllEntities to count all entities with usage or access
        let services = &self.base.services;
        let (provider_id, account_name) = (&self.base.provider_id, &self.account_name);

        let extension_usages = services.authentication_usage_service.read_account_usages(provider_id, account_name);
        let allowed_extensions = services.authentication_access_service.read_allowed_extensions(provider_id, account_name);
        let extension_ids = ordered_unique(
            extension_usages
                .into_iter()
                .map(|usage| usage.extension_id)
                .chain(allowed_extensions.into_iter().filter(|ext| ext.allowed == Some(true)).map(|ext| ext.id)),
        );

        let mcp_usages = services.authentication_mcp_usage_service.read_account_usages(provider_id, account_name);
        let allowed_mcp_servers = services.authentication_mcp_access_service.read_allowed_mcp_servers(provider_id, account_name);
        let mcp_server_ids = ordered_unique(
            mcp_usages
                .into_iter()
                .map(|usage| usage.mcp_server_id)
                .chain(allowed_mcp_servers.into_iter().filter(|server| server.allowed == Some(true)).map(|server| server.id)),
        );

        let extension_count = extension_ids.len();
        let mcp_server_count = mcp_server_ids.len();

        EntityCount { extensions: extension_count, mcp_servers: mcp_server_count, total: extension_count + mcp_server_count }
    }

    fn remove_all_access(&self) {
        // Remove all extension access
        let extensions_query = self.base.account_extensions(&self.account_name);
        let extension_ids: Vec<String> = extensions_query.get_allowed_extensions().into_iter().map(|ext| ext.id).collect();
        if !extension_ids.is_empty() {
            extensions_query.remove_access(&extension_ids);
        }

        // Remove all MCP server access
        let mcp_servers_query = self.base.account_mcp_servers(&self.account_name);
        let mcp_server_ids: Vec<String> = mcp_servers_query.get_allowed_mcp_servers().into_iter().map(|server| server.id).collect();
        if !mcp_server_ids.is_empty() {
            mcp_servers_query.remove_access(&mcp_server_ids);
        }
    }

    fn for_each(&self, callback: &mut dyn FnMut(&str, EntityType)) {
        // Iterate over extensions
        self.base.account_extensions(&self.account_name).for_each(&mut |extension_query: &dyn IAccountExtensionQuery| {
            callback(extension_query.extension_id(), EntityType::Extension);
        });

        // Iterate over MCP servers
        self.base.account_mcp_servers(&self.account_name).for_each(&mut |mcp_server_query: &dyn IAccountMcpServerQuery| {
            callback(mcp_server_query.mcp_server_id(), EntityType::McpServer);
        });
    }
}

/// Implementation of account query operations.
struct AccountQuery {
    base: BaseQuery,
    account_name: String,
}

impl IAccountQuery for AccountQuery {
    fn account_name(&self) -> &str {
        &self.account_name
    }

    fn extension(&self, extension_id: &str) -> Box<dyn IAccountExtensionQuery> {
        Box::new(self.base.account_extension(&self.account_name, extension_id))
    }

    fn mcp_server(&self, mcp_server_id: &str) -> Box<dyn IAccountMcpServerQuery> {
        Box::new(self.base.account_mcp_server(&self.account_name, mcp_server_id))
    }

    fn extensions(&self) -> Box<dyn IAccountExtensionsQuery> {
        Box::new(self.base.account_extensions(&self.account_name))
    }

    fn mcp_servers(&self) -> Box<dyn IAccountMcpServersQuery> {
        Box::new(self.base.account_mcp_servers(&self.account_name))
    }

    fn entities(&self) -> Box<dyn IAccountEntitiesQuery> {
        Box::new(self.base.account_entities(&self.account_name))
    }

    fn remove(&self) {
        self.base.services.remove_account_data(&self.base.provider_id, &self.account_name);
    }
}

/// Implementation of provider-extension query operations.
struct ProviderExtensionQuery {
    base: BaseQuery,
    extension_id: String,
}

impl IProviderExtensionQuery for ProviderExtensionQuery {
    fn extension_id(&self) -> &str {
        &self.extension_id
    }

    fn get_preferred_account(&self) -> Option<String> {
        self.base.services.authentication_extensions_service.get_account_preference(&self.extension_id, &self.base.provider_id)
    }

    fn set_preferred_account(&self, account: &AuthenticationSessionAccount) {
        self.base.services.authentication_extensions_service.update_account_preference(&self.extension_id, &self.base.provider_id, account);
    }

    fn remove_account_preference(&self) {
        self.base.services.authentication_extensions_service.remove_account_preference(&self.extension_id, &self.base.provider_id);
    }
}

/// Implementation of provider-MCP server query operations.
struct ProviderMcpServerQuery {
    base: BaseQuery,
    mcp_server_id: String,
}

impl IProviderMcpServerQuery for ProviderMcpServerQuery {
    fn mcp_server_id(&self) -> &str {
        &self.mcp_server_id
    }

    fn get_last_used_account(&self) -> Promise<Option<String>> {
        let base = self.base.clone();
        let mcp_server_id = self.mcp_server_id.clone();
        async_fn(async move {
            let last_used_account = with_accounts(&base.services, &base.provider_id, |accounts| {
                let mut last_used_account: Option<String> = None;
                let mut last_used_time = 0.0;

                for account in accounts {
                    let usages = base.services.authentication_mcp_usage_service.read_account_usages(&base.provider_id, &account.label);

                    for usage in usages.iter().filter(|usage| usage.mcp_server_id == mcp_server_id) {
                        if usage.last_used > last_used_time {
                            last_used_time = usage.last_used;
                            last_used_account = Some(account.label.clone());
                        }
                    }
                }

                last_used_account
            })
            .await;
            Ok(last_used_account.unwrap_or(None))
        })
    }

    fn get_preferred_account(&self) -> Option<String> {
        self.base.services.authentication_mcp_service.get_account_preference(&self.mcp_server_id, &self.base.provider_id)
    }

    fn set_preferred_account(&self, account: &AuthenticationSessionAccount) {
        self.base.services.authentication_mcp_service.update_account_preference(&self.mcp_server_id, &self.base.provider_id, account);
    }

    fn remove_account_preference(&self) {
        self.base.services.authentication_mcp_service.remove_account_preference(&self.mcp_server_id, &self.base.provider_id);
    }

    fn get_used_accounts(&self) -> Promise<Vec<String>> {
        let base = self.base.clone();
        let mcp_server_id = self.mcp_server_id.clone();
        async_fn(async move {
            let used_accounts = with_accounts(&base.services, &base.provider_id, |accounts| {
                let mut used_accounts = Vec::new();

                for account in accounts {
                    let usages = base.services.authentication_mcp_usage_service.read_account_usages(&base.provider_id, &account.label);
                    if usages.iter().any(|usage| usage.mcp_server_id == mcp_server_id) {
                        used_accounts.push(account.label.clone());
                    }
                }

                used_accounts
            })
            .await;
            Ok(used_accounts.unwrap_or_default())
        })
    }
}

/// Implementation of provider query operations.
struct ProviderQuery {
    base: BaseQuery,
}

impl IProviderQuery for ProviderQuery {
    fn account(&self, account_name: &str) -> Box<dyn IAccountQuery> {
        Box::new(self.base.account(account_name))
    }

    fn extension(&self, extension_id: &str) -> Box<dyn IProviderExtensionQuery> {
        Box::new(self.base.provider_extension(extension_id))
    }

    fn mcp_server(&self, mcp_server_id: &str) -> Box<dyn IProviderMcpServerQuery> {
        Box::new(self.base.provider_mcp_server(mcp_server_id))
    }

    fn get_active_entities(&self) -> Promise<IActiveEntities> {
        let base = self.base.clone();
        async_fn(async move {
            let mut extensions: Vec<String> = Vec::new();
            let mut mcp_servers: Vec<String> = Vec::new();

            // Return empty arrays if there's an error
            let _ = with_accounts(&base.services, &base.provider_id, |accounts| {
                for account in accounts {
                    // Get extension usages
                    for usage in base.services.authentication_usage_service.read_account_usages(&base.provider_id, &account.label) {
                        if !extensions.contains(&usage.extension_id) {
                            extensions.push(usage.extension_id);
                        }
                    }

                    // Get MCP server usages
                    for usage in base.services.authentication_mcp_usage_service.read_account_usages(&base.provider_id, &account.label) {
                        if !mcp_servers.contains(&usage.mcp_server_id) {
                            mcp_servers.push(usage.mcp_server_id);
                        }
                    }
                }
            })
            .await;

            Ok(IActiveEntities { extensions, mcp_servers })
        })
    }

    fn get_account_names(&self) -> Promise<Vec<String>> {
        let base = self.base.clone();
        async_fn(async move {
            let account_names = with_accounts(&base.services, &base.provider_id, |accounts| {
                accounts.iter().map(|account| account.label.clone()).collect::<Vec<String>>()
            })
            .await;
            Ok(account_names.unwrap_or_default())
        })
    }

    fn get_usage_stats(&self) -> Promise<IAuthenticationUsageStats> {
        let base = self.base.clone();
        async_fn(async move {
            let mut recent_activity: Vec<RecentActivity> = Vec::new();
            let mut total_sessions = 0;
            let mut total_accounts = 0;

            // Return default stats if there's an error
            let _ = with_accounts(&base.services, &base.provider_id, |accounts| {
                total_accounts = accounts.len();

                for account in accounts {
                    let extension_usages = base.services.authentication_usage_service.read_account_usages(&base.provider_id, &account.label);
                    let mcp_usages = base.services.authentication_mcp_usage_service.read_account_usages(&base.provider_id, &account.label);

                    let usage_count = extension_usages.len() + mcp_usages.len();
                    let last_used = extension_usages
                        .iter()
                        .map(|usage| usage.last_used)
                        .chain(mcp_usages.iter().map(|usage| usage.last_used))
                        .fold(0.0, f64::max);

                    if usage_count > 0 {
                        recent_activity.push(RecentActivity { account_name: account.label.clone(), last_used, usage_count });
                    }
                }

                // Sort by most recent activity
                recent_activity.sort_by(|a, b| b.last_used.partial_cmp(&a.last_used).unwrap_or(std::cmp::Ordering::Equal));

                // Count total sessions (approximate)
                total_sessions = recent_activity.iter().map(|activity| activity.usage_count).sum();
            })
            .await;

            Ok(IAuthenticationUsageStats { total_sessions, total_accounts, recent_activity })
        })
    }

    fn for_each_account(&self, callback: Box<dyn FnMut(&dyn IAccountQuery)>) -> Promise<()> {
        let base = self.base.clone();
        let mut callback = callback;
        async_fn(async move {
            // Silently handle errors in enumeration
            let _ = with_accounts(&base.services, &base.provider_id, |accounts| {
                for account in accounts {
                    callback(&base.account(&account.label));
                }
            })
            .await;
            Ok(())
        })
    }
}

/// Implementation of extension query operations (cross-provider).
struct ExtensionQuery {
    extension_id: String,
    services: Rc<QueryServices>,
}

impl IExtensionQuery for ExtensionQuery {
    fn extension_id(&self) -> &str {
        &self.extension_id
    }

    fn get_providers_with_access(&self, include_internal: Option<bool>) -> Promise<Vec<String>> {
        let extension_id = self.extension_id.clone();
        providers_with_access(self.services.clone(), include_internal.unwrap_or(false), move |services, provider_id, account_name| {
            services.authentication_access_service.is_access_allowed(provider_id, account_name, &extension_id)
        })
    }

    fn get_all_account_preferences(&self, include_internal: Option<bool>) -> OrderedMap<String, String> {
        all_account_preferences(&self.services, include_internal.unwrap_or(false), |provider_id| {
            self.services.authentication_extensions_service.get_account_preference(&self.extension_id, provider_id)
        })
    }

    fn provider(&self, provider_id: &str) -> Box<dyn IProviderExtensionQuery> {
        Box::new(BaseQuery::new(provider_id, &self.services).provider_extension(&self.extension_id))
    }
}

/// Implementation of MCP server query operations (cross-provider).
struct McpServerQuery {
    mcp_server_id: String,
    services: Rc<QueryServices>,
}

impl IMcpServerQuery for McpServerQuery {
    fn mcp_server_id(&self) -> &str {
        &self.mcp_server_id
    }

    fn get_providers_with_access(&self, include_internal: Option<bool>) -> Promise<Vec<String>> {
        let mcp_server_id = self.mcp_server_id.clone();
        providers_with_access(self.services.clone(), include_internal.unwrap_or(false), move |services, provider_id, account_name| {
            services.authentication_mcp_access_service.is_access_allowed(provider_id, account_name, &mcp_server_id)
        })
    }

    fn get_all_account_preferences(&self, include_internal: Option<bool>) -> OrderedMap<String, String> {
        all_account_preferences(&self.services, include_internal.unwrap_or(false), |provider_id| {
            self.services.authentication_mcp_service.get_account_preference(&self.mcp_server_id, provider_id)
        })
    }

    fn provider(&self, provider_id: &str) -> Box<dyn IProviderMcpServerQuery> {
        Box::new(BaseQuery::new(provider_id, &self.services).provider_mcp_server(&self.mcp_server_id))
    }
}

impl_base_query!(
    AccountExtensionQuery,
    AccountMcpServerQuery,
    AccountExtensionsQuery,
    AccountMcpServersQuery,
    AccountEntitiesQuery,
    AccountQuery,
    ProviderExtensionQuery,
    ProviderMcpServerQuery,
    ProviderQuery,
);

/// Main implementation of the authentication query service.
pub struct AuthenticationQueryService {
    base: Rc<Disposable>,
    on_did_change_preferences: Rc<Emitter<PreferencesChange>>,
    on_did_change_access: Rc<Emitter<AccountAccessChange>>,
    pub services: Rc<QueryServices>,
}

impl Injectable for AuthenticationQueryService {
    type Args = ();
    type Deps = (
        Rc<dyn IAuthenticationService>,
        Rc<dyn IAuthenticationUsageService>,
        Rc<dyn IAuthenticationMcpUsageService>,
        Rc<dyn IAuthenticationAccessService>,
        Rc<dyn IAuthenticationMcpAccessService>,
        Rc<dyn IAuthenticationExtensionsService>,
        Rc<dyn IAuthenticationMcpService>,
        Rc<dyn LogService>,
    );

    fn create(
        _: (),
        (
            authentication_service,
            authentication_usage_service,
            authentication_mcp_usage_service,
            authentication_access_service,
            authentication_mcp_access_service,
            authentication_extensions_service,
            authentication_mcp_service,
            log_service,
        ): Self::Deps,
    ) -> Rc<Self> {
        let base = Disposable::new();
        let on_did_change_preferences = Emitter::new();
        base.register(on_did_change_preferences.clone());
        let on_did_change_access = Emitter::new();
        base.register(on_did_change_access.clone());

        let services = Rc::new(QueryServices {
            authentication_service,
            authentication_usage_service,
            authentication_mcp_usage_service,
            authentication_access_service,
            authentication_mcp_access_service,
            authentication_extensions_service,
            authentication_mcp_service,
            log_service,
        });

        // Forward events from underlying services
        let preferences = on_did_change_preferences.clone();
        base.register(services.authentication_extensions_service.on_did_change_account_preference().listen(move |e| {
            preferences.fire(PreferencesChange {
                provider_id: e.provider_id.clone(),
                entity_type: EntityType::Extension,
                entity_ids: e.extension_ids.clone(),
            });
        }));

        let preferences = on_did_change_preferences.clone();
        base.register(services.authentication_mcp_service.on_did_change_account_preference().listen(move |e| {
            preferences.fire(PreferencesChange {
                provider_id: e.provider_id.clone(),
                entity_type: EntityType::McpServer,
                entity_ids: e.mcp_server_ids.clone(),
            });
        }));

        for access_event in [
            services.authentication_access_service.on_did_change_extension_session_access(),
            services.authentication_mcp_access_service.on_did_change_mcp_session_access(),
        ] {
            let access = on_did_change_access.clone();
            base.register(access_event.listen(move |e| access.fire(e.clone())));
        }

        Rc::new(Self { base, on_did_change_preferences, on_did_change_access, services })
    }
}

service_impl!(AuthenticationQueryService => dyn IAuthenticationQueryService, disposable);

/// `registerSingleton(IAuthenticationQueryService, AuthenticationQueryService, InstantiationType.Delayed)`.
pub fn register() {
    register_singleton::<AuthenticationQueryService>(I_AUTHENTICATION_QUERY_SERVICE, InstantiationType::Delayed);
}

impl IAuthenticationQueryService for AuthenticationQueryService {
    fn on_did_change_preferences(&self) -> Event<PreferencesChange> {
        self.on_did_change_preferences.event()
    }

    fn on_did_change_access(&self) -> Event<AccountAccessChange> {
        self.on_did_change_access.event()
    }

    fn provider(&self, provider_id: &str) -> Box<dyn IProviderQuery> {
        Box::new(ProviderQuery { base: BaseQuery::new(provider_id, &self.services) })
    }

    fn extension(&self, extension_id: &str) -> Box<dyn IExtensionQuery> {
        Box::new(ExtensionQuery { extension_id: extension_id.to_owned(), services: self.services.clone() })
    }

    fn mcp_server(&self, mcp_server_id: &str) -> Box<dyn IMcpServerQuery> {
        Box::new(McpServerQuery { mcp_server_id: mcp_server_id.to_owned(), services: self.services.clone() })
    }

    fn get_provider_ids(&self) -> Vec<String> {
        self.services.provider_ids(false)
    }

    fn clear_all_data(&self, confirmation: &str, include_internal: Option<bool>) -> Promise<()> {
        let services = self.services.clone();
        let confirmation = confirmation.to_owned();
        async_fn(async move {
            if confirmation != "CLEAR_ALL_AUTH_DATA" {
                return Err(Error::new("Must provide confirmation string to clear all authentication data"));
            }

            for provider_id in services.provider_ids(include_internal.unwrap_or(true)) {
                let cleared = with_accounts(&services, &provider_id, |accounts| {
                    for account in accounts {
                        services.remove_account_data(&provider_id, &account.label);
                    }
                })
                .await;

                if let Err(error) = cleared {
                    let message = format!("Error clearing data for provider {provider_id}:");
                    services.log_service.error_args(&LogMessage::from(&message), &[LogArg::from(&error)]);
                }
            }

            services.log_service.info("All authentication data cleared");
            Ok(())
        })
    }
}

impl IDisposable for AuthenticationQueryService {
    fn dispose(&self) {
        self.base.dispose();
    }
}
