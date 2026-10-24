//! Porte de `src/vs/workbench/services/authentication/test/browser/authenticationQueryServiceMocks.ts`: os
//! dublês de serviço que o teste do `AuthenticationQueryService` usa, com o registro de chamadas
//! (`BaseTestService.trackCall`) para o teste conferir a delegação.
//!
//! Notas de tradução:
//! - **`BaseTestService`** é o [`BaseTestService<V>`] (`data: Map<string, any>` é um `OrderedMap<String, V>`, com
//!   o `V` do que cada dublê guarda) e cada dublê o tem por composição (`Deref`), com o `Disposable` do original
//!   no campo `disposable`. O `args: any[]` de cada chamada é o [`CallArg`], com um acessor por tipo que
//!   o teste lê; o `timestamp`, que nenhum teste lê, não existe. `getKey` é [`BaseTestService::key`].
//! - **`createProvider(overrides)`** é [`create_provider`] (`id`, `label`): a interface `IAuthenticationProvider`
//!   (sessões, `createSession`...) ainda não está portada, e o teste só usa o `id` e o `label` do provedor,
//!   que são o [`AuthenticationProviderInformation`] que o evento de registro carrega. `createSession` não existe
//!   pelo mesmo motivo.
//! - **Membros "stub"** (`updateSessionPreference`, `selectSession`, `getSessions`, `createSession`,
//!   `declaredProviders`...) são dos traits completos do original; os traits portados só têm os membros que os
//!   serviços portados chamam (ver `authentication.rs`), então os stubs não existem. Os eventos
//!   `onDidChangeSessions`, `onDidUnregisterAuthenticationProvider` e `onDidChangeDeclaredProviders` do
//!   `TestAuthenticationService` não existem pelo mesmo motivo.
//! - **`isAccessAllowed` do `TestAccessService`** devolve `extension?.allowed`; `scopes: [...scopes]` é a cópia
//!   de `scopes` (vazia se `None`, que o `addAccountUsage` das consultas nunca passa).
#![allow(dead_code)]

use std::cell::RefCell;
use std::collections::HashMap;
use std::ops::Deref;
use std::rc::Rc;

use je_base::event::{Emitter, Event};
use je_base::lifecycle::{Disposable, IDisposable};
use je_base::map::OrderedMap;
use je_base::promise::Promise;
use je_base::runtime::date_now;
use je_workbench::services::authentication::authentication::{
    AccountAccessChange, AccountPreferenceChange, AllowedExtension, AuthenticationProviderInformation, AuthenticationSessionAccount,
    IAuthenticationExtensionsService, IAuthenticationService,
};
use je_workbench::services::authentication::authentication_access_service::IAuthenticationAccessService;
use je_workbench::services::authentication::authentication_mcp_access_service::{
    AllowedMcpServer, IAuthenticationMcpAccessService, McpSessionAccessChange, urls_equal,
};
use je_workbench::services::authentication::authentication_mcp_service::{IAuthenticationMcpService, McpAccountPreferenceChange};
use je_workbench::services::authentication::authentication_mcp_usage_service::{IAuthenticationMcpUsage, IAuthenticationMcpUsageService};
use je_workbench::services::authentication::authentication_usage_service::{IAccountUsage, IAuthenticationUsageService};

/// Helper function to create a mock authentication provider.
pub fn create_provider(id: &str, label: &str) -> AuthenticationProviderInformation {
    AuthenticationProviderInformation { id: id.to_owned(), label: label.to_owned(), authorization_server_globs: None }
}

/// Um argumento de `MethodCall.args`.
#[derive(Clone, Debug)]
pub enum CallArg {
    Text(String),
    Texts(Vec<String>),
    Extensions(Vec<AllowedExtension>),
    McpServers(Vec<AllowedMcpServer>),
}

impl CallArg {
    pub fn text(&self) -> &str {
        match self {
            Self::Text(text) => text,
            other => panic!("o argumento deveria ser um texto, veio {other:?}"),
        }
    }

    pub fn texts(&self) -> &[String] {
        match self {
            Self::Texts(texts) => texts,
            other => panic!("o argumento deveria ser uma lista de textos, veio {other:?}"),
        }
    }

    pub fn extensions(&self) -> &[AllowedExtension] {
        match self {
            Self::Extensions(extensions) => extensions,
            other => panic!("o argumento deveria ser uma lista de extensões, veio {other:?}"),
        }
    }

    pub fn mcp_servers(&self) -> &[AllowedMcpServer] {
        match self {
            Self::McpServers(mcp_servers) => mcp_servers,
            other => panic!("o argumento deveria ser uma lista de servidores MCP, veio {other:?}"),
        }
    }
}

fn text(value: &str) -> CallArg {
    CallArg::Text(value.to_owned())
}

/// Interface for tracking method calls in mock services.
#[derive(Clone, Debug)]
pub struct MethodCall {
    pub method: &'static str,
    pub args: Vec<CallArg>,
}

/// Base class for test services with common functionality and call tracking.
pub struct BaseTestService<V> {
    pub disposable: Rc<Disposable>,
    data: RefCell<OrderedMap<String, V>>,
    method_calls: RefCell<Vec<MethodCall>>,
}

impl<V: Clone> BaseTestService<V> {
    pub fn new() -> Self {
        Self { disposable: Disposable::new(), data: RefCell::new(OrderedMap::new()), method_calls: RefCell::new(Vec::new()) }
    }

    pub fn key(parts: &[&str]) -> String {
        parts.join("::")
    }

    pub fn data_get(&self, key: &str) -> Option<V> {
        self.data.borrow().get(&key.to_owned()).cloned()
    }

    pub fn data_set(&self, key: String, value: V) {
        self.data.borrow_mut().insert(key, value);
    }

    pub fn data_delete(&self, key: &str) {
        self.data.borrow_mut().remove(&key.to_owned());
    }

    pub fn data_keys(&self) -> Vec<String> {
        self.data.borrow().keys().cloned().collect()
    }

    /// Track a method call for verification in tests.
    pub fn track_call(&self, method: &'static str, args: Vec<CallArg>) {
        self.method_calls.borrow_mut().push(MethodCall { method, args });
    }

    /// Get all method calls for verification.
    pub fn get_method_calls(&self) -> Vec<MethodCall> {
        self.method_calls.borrow().clone()
    }

    /// Get calls for a specific method.
    pub fn get_calls_for(&self, method: &str) -> Vec<MethodCall> {
        self.method_calls.borrow().iter().filter(|call| call.method == method).cloned().collect()
    }

    /// Clear method call history.
    pub fn clear_call_history(&self) {
        self.method_calls.borrow_mut().clear();
    }

    /// Get the last call for a specific method.
    pub fn get_last_call_for(&self, method: &str) -> Option<MethodCall> {
        self.get_calls_for(method).pop()
    }
}

impl<V: Clone> Default for BaseTestService<V> {
    fn default() -> Self {
        Self::new()
    }
}

impl<V> IDisposable for BaseTestService<V> {
    fn dispose(&self) {
        self.disposable.dispose();
    }
}

/// Faz o dublê herdar de [`BaseTestService`]: `Deref` para o que o teste chama (`clear_call_history`,
/// `get_calls_for`) e o descarte do `Disposable`.
macro_rules! extends_base_test_service {
    ($($service:ident => $value:ty),* $(,)?) => {
        $(
            impl Deref for $service {
                type Target = BaseTestService<$value>;

                fn deref(&self) -> &BaseTestService<$value> {
                    &self.base
                }
            }

            impl IDisposable for $service {
                fn dispose(&self) {
                    self.base.dispose();
                }
            }
        )*
    };
}

/// Test implementation that actually stores and retrieves data.
pub struct TestUsageService {
    base: BaseTestService<Vec<IAccountUsage>>,
}

impl TestUsageService {
    pub fn new() -> Rc<Self> {
        Rc::new(Self { base: BaseTestService::new() })
    }
}

impl IAuthenticationUsageService for TestUsageService {
    fn read_account_usages(&self, provider_id: &str, account_name: &str) -> Vec<IAccountUsage> {
        self.track_call("readAccountUsages", vec![text(provider_id), text(account_name)]);
        self.data_get(&BaseTestService::<Vec<IAccountUsage>>::key(&[provider_id, account_name])).unwrap_or_default()
    }

    fn add_account_usage(&self, provider_id: &str, account_name: &str, scopes: Option<&[String]>, extension_id: &str, extension_name: &str) {
        self.track_call(
            "addAccountUsage",
            vec![text(provider_id), text(account_name), CallArg::Texts(scopes.unwrap_or(&[]).to_vec()), text(extension_id), text(extension_name)],
        );
        let key = BaseTestService::<Vec<IAccountUsage>>::key(&[provider_id, account_name]);
        let mut usages = self.data_get(&key).unwrap_or_default();
        usages.push(IAccountUsage {
            extension_id: extension_id.to_owned(),
            extension_name: extension_name.to_owned(),
            scopes: Some(scopes.unwrap_or(&[]).to_vec()),
            last_used: date_now() as f64,
        });
        self.data_set(key, usages);
    }

    fn remove_account_usage(&self, provider_id: &str, account_name: &str) {
        self.track_call("removeAccountUsage", vec![text(provider_id), text(account_name)]);
        self.data_delete(&BaseTestService::<Vec<IAccountUsage>>::key(&[provider_id, account_name]));
    }

    // Stub implementations for missing methods
    fn initialize_extension_usage_cache(&self) -> Promise<()> {
        Promise::resolved(())
    }

    fn extension_uses_auth(&self, _extension_id: &str) -> Promise<bool> {
        Promise::resolved(false)
    }
}

pub struct TestMcpUsageService {
    base: BaseTestService<Vec<IAuthenticationMcpUsage>>,
}

impl TestMcpUsageService {
    pub fn new() -> Rc<Self> {
        Rc::new(Self { base: BaseTestService::new() })
    }
}

impl IAuthenticationMcpUsageService for TestMcpUsageService {
    fn read_account_usages(&self, provider_id: &str, account_name: &str) -> Vec<IAuthenticationMcpUsage> {
        self.track_call("readAccountUsages", vec![text(provider_id), text(account_name)]);
        self.data_get(&BaseTestService::<Vec<IAuthenticationMcpUsage>>::key(&[provider_id, account_name])).unwrap_or_default()
    }

    fn add_account_usage(&self, provider_id: &str, account_name: &str, scopes: &[String], mcp_server_id: &str, mcp_server_name: &str) {
        self.track_call(
            "addAccountUsage",
            vec![text(provider_id), text(account_name), CallArg::Texts(scopes.to_vec()), text(mcp_server_id), text(mcp_server_name)],
        );
        let key = BaseTestService::<Vec<IAuthenticationMcpUsage>>::key(&[provider_id, account_name]);
        let mut usages = self.data_get(&key).unwrap_or_default();
        usages.push(IAuthenticationMcpUsage {
            mcp_server_id: mcp_server_id.to_owned(),
            mcp_server_name: mcp_server_name.to_owned(),
            scopes: Some(scopes.to_vec()),
            last_used: date_now() as f64,
        });
        self.data_set(key, usages);
    }

    fn remove_account_usage(&self, provider_id: &str, account_name: &str) {
        self.track_call("removeAccountUsage", vec![text(provider_id), text(account_name)]);
        self.data_delete(&BaseTestService::<Vec<IAuthenticationMcpUsage>>::key(&[provider_id, account_name]));
    }

    // Stub implementations for missing methods
    fn initialize_usage_cache(&self) -> Promise<()> {
        Promise::resolved(())
    }

    fn has_used_auth(&self, _mcp_server_id: &str) -> Promise<bool> {
        Promise::resolved(false)
    }
}

pub struct TestAccessService {
    base: BaseTestService<Vec<AllowedExtension>>,
    on_did_change_extension_session_access: Rc<Emitter<AccountAccessChange>>,
}

impl TestAccessService {
    pub fn new() -> Rc<Self> {
        let base = BaseTestService::new();
        let on_did_change_extension_session_access = Emitter::new();
        base.disposable.register(on_did_change_extension_session_access.clone());
        Rc::new(Self { base, on_did_change_extension_session_access })
    }
}

impl IAuthenticationAccessService for TestAccessService {
    fn on_did_change_extension_session_access(&self) -> Event<AccountAccessChange> {
        self.on_did_change_extension_session_access.event()
    }

    fn is_access_allowed(&self, provider_id: &str, account_name: &str, extension_id: &str) -> Option<bool> {
        self.track_call("isAccessAllowed", vec![text(provider_id), text(account_name), text(extension_id)]);
        let extensions = self.data_get(&BaseTestService::<Vec<AllowedExtension>>::key(&[provider_id, account_name])).unwrap_or_default();
        extensions.iter().find(|e| e.id == extension_id).and_then(|extension| extension.allowed)
    }

    fn read_allowed_extensions(&self, provider_id: &str, account_name: &str) -> Vec<AllowedExtension> {
        self.track_call("readAllowedExtensions", vec![text(provider_id), text(account_name)]);
        self.data_get(&BaseTestService::<Vec<AllowedExtension>>::key(&[provider_id, account_name])).unwrap_or_default()
    }

    fn update_allowed_extensions(&self, provider_id: &str, account_name: &str, extensions: &[AllowedExtension]) {
        self.track_call("updateAllowedExtensions", vec![text(provider_id), text(account_name), CallArg::Extensions(extensions.to_vec())]);
        let key = BaseTestService::<Vec<AllowedExtension>>::key(&[provider_id, account_name]);
        let existing = self.data_get(&key).unwrap_or_default();

        // Merge with existing data, updating or adding extensions
        let mut merged = existing;
        for ext in extensions {
            match merged.iter().position(|e| e.id == ext.id) {
                Some(existing_index) => merged[existing_index] = ext.clone(),
                None => merged.push(ext.clone()),
            }
        }

        self.data_set(key, merged);
        self.on_did_change_extension_session_access
            .fire(AccountAccessChange { provider_id: provider_id.to_owned(), account_name: account_name.to_owned() });
    }

    fn remove_allowed_extensions(&self, provider_id: &str, account_name: &str) {
        self.track_call("removeAllowedExtensions", vec![text(provider_id), text(account_name)]);
        self.data_delete(&BaseTestService::<Vec<AllowedExtension>>::key(&[provider_id, account_name]));
    }
}

pub struct TestMcpAccessService {
    base: BaseTestService<Vec<AllowedMcpServer>>,
    on_did_change_mcp_session_access: Rc<Emitter<McpSessionAccessChange>>,
}

impl TestMcpAccessService {
    pub fn new() -> Rc<Self> {
        let base = BaseTestService::new();
        let on_did_change_mcp_session_access = Emitter::new();
        base.disposable.register(on_did_change_mcp_session_access.clone());
        Rc::new(Self { base, on_did_change_mcp_session_access })
    }

    fn is_access_allowed_for(&self, provider_id: &str, account_name: &str, mcp_server_id: &str, mcp_server_url: Option<&str>) -> Option<bool> {
        let servers = self.data_get(&BaseTestService::<Vec<AllowedMcpServer>>::key(&[provider_id, account_name])).unwrap_or_default();
        let server = servers.iter().find(|s| s.id == mcp_server_id)?;
        if mcp_server_url.is_some() && !urls_equal(server.url.as_deref(), mcp_server_url) {
            return None;
        }
        // Matches production: presence in the list with an undefined `allowed` indicates allowance.
        Some(server.allowed.unwrap_or(true))
    }
}

impl IAuthenticationMcpAccessService for TestMcpAccessService {
    fn on_did_change_mcp_session_access(&self) -> Event<McpSessionAccessChange> {
        self.on_did_change_mcp_session_access.event()
    }

    fn is_access_allowed(&self, provider_id: &str, account_name: &str, mcp_server_id: &str) -> Option<bool> {
        self.track_call("isAccessAllowed", vec![text(provider_id), text(account_name), text(mcp_server_id)]);
        self.is_access_allowed_for(provider_id, account_name, mcp_server_id, None)
    }

    fn is_access_allowed_for_url(&self, provider_id: &str, account_name: &str, mcp_server_id: &str, mcp_server_url: &str) -> Option<bool> {
        self.track_call("isAccessAllowedForUrl", vec![text(provider_id), text(account_name), text(mcp_server_id), text(mcp_server_url)]);
        self.is_access_allowed_for(provider_id, account_name, mcp_server_id, Some(mcp_server_url))
    }

    fn read_allowed_mcp_servers(&self, provider_id: &str, account_name: &str) -> Vec<AllowedMcpServer> {
        self.track_call("readAllowedMcpServers", vec![text(provider_id), text(account_name)]);
        self.data_get(&BaseTestService::<Vec<AllowedMcpServer>>::key(&[provider_id, account_name])).unwrap_or_default()
    }

    fn update_allowed_mcp_servers(&self, provider_id: &str, account_name: &str, mcp_servers: &[AllowedMcpServer]) {
        self.track_call("updateAllowedMcpServers", vec![text(provider_id), text(account_name), CallArg::McpServers(mcp_servers.to_vec())]);
        let key = BaseTestService::<Vec<AllowedMcpServer>>::key(&[provider_id, account_name]);
        let existing = self.data_get(&key).unwrap_or_default();

        // Merge with existing data, updating or adding MCP servers
        let mut merged = existing;
        for server in mcp_servers {
            match merged.iter().position(|s| s.id == server.id) {
                Some(existing_index) => merged[existing_index] = server.clone(),
                None => merged.push(server.clone()),
            }
        }

        self.data_set(key, merged);
        self.on_did_change_mcp_session_access.fire(McpSessionAccessChange { provider_id: provider_id.to_owned(), account_name: account_name.to_owned() });
    }

    fn remove_allowed_mcp_servers(&self, provider_id: &str, account_name: &str) {
        self.track_call("removeAllowedMcpServers", vec![text(provider_id), text(account_name)]);
        self.data_delete(&BaseTestService::<Vec<AllowedMcpServer>>::key(&[provider_id, account_name]));
        self.on_did_change_mcp_session_access.fire(McpSessionAccessChange { provider_id: provider_id.to_owned(), account_name: account_name.to_owned() });
    }
}

/// O `TestPreferencesService` do original: guarda a conta preferida (o `label`) por `clientId::providerId`. O
/// evento nunca é disparado. `E` é o que o evento carrega, que muda entre a extensão e o MCP.
pub struct TestPreferencesService<E: 'static> {
    base: BaseTestService<String>,
    on_did_change_account_preference: Rc<Emitter<E>>,
}

impl<E: 'static> TestPreferencesService<E> {
    fn new() -> Self {
        let base = BaseTestService::new();
        let on_did_change_account_preference = Emitter::new();
        base.disposable.register(on_did_change_account_preference.clone());
        Self { base, on_did_change_account_preference }
    }

    fn event(&self) -> Event<E> {
        self.on_did_change_account_preference.event()
    }

    fn get_account_preference(&self, client_id: &str, provider_id: &str) -> Option<String> {
        self.base.data_get(&BaseTestService::<String>::key(&[client_id, provider_id]))
    }

    fn update_account_preference(&self, client_id: &str, provider_id: &str, account: &AuthenticationSessionAccount) {
        self.base.data_set(BaseTestService::<String>::key(&[client_id, provider_id]), account.label.clone());
    }

    fn remove_account_preference(&self, client_id: &str, provider_id: &str) {
        self.base.data_delete(&BaseTestService::<String>::key(&[client_id, provider_id]));
    }
}

impl<E: 'static> IDisposable for TestPreferencesService<E> {
    fn dispose(&self) {
        self.base.dispose();
    }
}

pub struct TestExtensionsService {
    preferences: TestPreferencesService<AccountPreferenceChange>,
}

impl TestExtensionsService {
    pub fn new() -> Rc<Self> {
        Rc::new(Self { preferences: TestPreferencesService::new() })
    }
}

impl IAuthenticationExtensionsService for TestExtensionsService {
    fn on_did_change_account_preference(&self) -> Event<AccountPreferenceChange> {
        self.preferences.event()
    }

    fn get_account_preference(&self, extension_id: &str, provider_id: &str) -> Option<String> {
        self.preferences.get_account_preference(extension_id, provider_id)
    }

    fn update_account_preference(&self, extension_id: &str, provider_id: &str, account: &AuthenticationSessionAccount) {
        self.preferences.update_account_preference(extension_id, provider_id, account);
    }

    fn remove_account_preference(&self, extension_id: &str, provider_id: &str) {
        self.preferences.remove_account_preference(extension_id, provider_id);
    }
}

impl IDisposable for TestExtensionsService {
    fn dispose(&self) {
        self.preferences.dispose();
    }
}

pub struct TestMcpService {
    preferences: TestPreferencesService<McpAccountPreferenceChange>,
}

impl TestMcpService {
    pub fn new() -> Rc<Self> {
        Rc::new(Self { preferences: TestPreferencesService::new() })
    }
}

impl IAuthenticationMcpService for TestMcpService {
    fn on_did_change_account_preference(&self) -> Event<McpAccountPreferenceChange> {
        self.preferences.event()
    }

    fn get_account_preference(&self, mcp_server_id: &str, provider_id: &str) -> Option<String> {
        self.preferences.get_account_preference(mcp_server_id, provider_id)
    }

    fn update_account_preference(&self, mcp_server_id: &str, provider_id: &str, account: &AuthenticationSessionAccount) {
        self.preferences.update_account_preference(mcp_server_id, provider_id, account);
    }

    fn remove_account_preference(&self, mcp_server_id: &str, provider_id: &str) {
        self.preferences.remove_account_preference(mcp_server_id, provider_id);
    }
}

impl IDisposable for TestMcpService {
    fn dispose(&self) {
        self.preferences.dispose();
    }
}

/// Minimal authentication service mock that only implements what we need.
pub struct TestAuthenticationService {
    base: BaseTestService<AuthenticationProviderInformation>,
    on_did_register_authentication_provider: Rc<Emitter<AuthenticationProviderInformation>>,
    accounts_map: RefCell<HashMap<String, Vec<AuthenticationSessionAccount>>>,
}

impl TestAuthenticationService {
    pub fn new() -> Rc<Self> {
        let base = BaseTestService::new();
        let on_did_register_authentication_provider = Emitter::new();
        base.disposable.register(on_did_register_authentication_provider.clone());
        Rc::new(Self { base, on_did_register_authentication_provider, accounts_map: RefCell::new(HashMap::new()) })
    }

    pub fn register_authentication_provider(&self, id: &str, provider: AuthenticationProviderInformation) {
        self.data_set(id.to_owned(), provider.clone());
        self.on_did_register_authentication_provider.fire(AuthenticationProviderInformation { id: id.to_owned(), label: provider.label, authorization_server_globs: None });
    }

    pub fn is_authentication_provider_registered(&self, id: &str) -> bool {
        self.data_get(id).is_some()
    }

    pub fn get_provider(&self, id: &str) -> AuthenticationProviderInformation {
        self.data_get(id).expect("o provedor não está registrado")
    }

    pub fn add_accounts(&self, provider_id: &str, accounts: Vec<AuthenticationSessionAccount>) {
        self.accounts_map.borrow_mut().insert(provider_id.to_owned(), accounts);
    }
}

impl IAuthenticationService for TestAuthenticationService {
    fn on_did_register_authentication_provider(&self) -> Event<AuthenticationProviderInformation> {
        self.on_did_register_authentication_provider.event()
    }

    fn get_provider_ids(&self) -> Vec<String> {
        self.data_keys()
    }

    fn get_accounts(&self, id: &str) -> Promise<Vec<AuthenticationSessionAccount>> {
        Promise::resolved(self.accounts_map.borrow().get(id).cloned().unwrap_or_default())
    }
}

extends_base_test_service!(
    TestUsageService => Vec<IAccountUsage>,
    TestMcpUsageService => Vec<IAuthenticationMcpUsage>,
    TestAccessService => Vec<AllowedExtension>,
    TestMcpAccessService => Vec<AllowedMcpServer>,
    TestAuthenticationService => AuthenticationProviderInformation,
);
