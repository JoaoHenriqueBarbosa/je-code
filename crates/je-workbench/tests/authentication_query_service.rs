//! Porte de `src/vs/workbench/services/authentication/test/browser/authenticationQueryService.test.ts` (suíte
//! `AuthenticationQueryService Integration Tests` e a suíte `Account entities query`), caso a caso e na mesma
//! ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` vira o guarda criado no começo de cada caso, e o `setup` é
//!   [`setup`], chamado logo depois (o `disposables.add(...)` do original é `add_rc`).
//! - `TestInstantiationService` com `stub(...)` é um `InstantiationService` sobre um `ServiceCollection` com os
//!   serviços; `createInstance` é o `create_instance` real. `TestStorageService` é o `InMemoryStorageService`
//!   e o `NullLogService` é o de `je_platform::log`. Os dublês (`TestUsageService`...) são o porte de
//!   `authenticationQueryServiceMocks.ts`, em `authentication_query_service_mocks`.
//! - **Casos `async`** (`getProvidersWithAccess`, `clearAllData`) rodam em `run_with_faked_timers`, com o guarda e
//!   o `setup` dentro do corpo; `await` é `.await.unwrap()` (uma rejeição reprova o caso).
//! - **`Map<string, string>`** (`getAllAccountPreferences`) é um `OrderedMap`: `get(k)` é [`pref`] e `size` é `len()`.
//! - **`assert.strictEqual(x, undefined)`** é `None`, e `assert.ok(x.includes(...))` é [`has`].
//! - **O argumento de `createProvider({ id, label })`** é [`mocks::create_provider`]: só o `id` e o `label` do
//!   provedor existem (ver os dublês).
//! - **`assert.ok(githubPrefs.size >= 0)`** do caso `cross-provider extension queries work correctly` vale por
//!   construção para um `usize`; o caso chama `len()` e segue para as asserções seguintes.
//! - **`assert.doesNotThrow`** é a própria chamada: um pânico (um `throw`) reprova o caso.
//! - **`typeof ext1.lastUsed === 'number'`** é `last_used.is_some()`.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
#[path = "authentication_query_service_mocks/mod.rs"]
mod mocks;

use std::cell::Cell;
use std::rc::Rc;

use common::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite, run_with_faked_timers};
use je_base::map::OrderedMap;
use je_instantiation::{InstantiationService, ServiceCollection};
use je_platform::log::{I_LOG_SERVICE, NullLogService};
use je_platform::storage::{I_STORAGE_SERVICE, InMemoryStorageService, StorageService};
use je_workbench::services::authentication::authentication::{
    AuthenticationSessionAccount, I_AUTHENTICATION_EXTENSIONS_SERVICE, I_AUTHENTICATION_SERVICE, IAuthenticationExtensionsService, IAuthenticationService,
};
use je_workbench::services::authentication::authentication_access_service::{I_AUTHENTICATION_ACCESS_SERVICE, IAuthenticationAccessService};
use je_workbench::services::authentication::authentication_mcp_access_service::{
    AgentHost, AllowedMcpServer, I_AUTHENTICATION_MCP_ACCESS_SERVICE, IAuthenticationMcpAccessService,
};
use je_workbench::services::authentication::authentication_mcp_service::{I_AUTHENTICATION_MCP_SERVICE, IAuthenticationMcpService};
use je_workbench::services::authentication::authentication_mcp_usage_service::{I_AUTHENTICATION_MCP_USAGE_SERVICE, IAuthenticationMcpUsageService};
use je_workbench::services::authentication::authentication_query::{EntityType, IAuthenticationQueryService};
use je_workbench::services::authentication::authentication_query_service::AuthenticationQueryService;
use je_workbench::services::authentication::authentication_usage_service::{I_AUTHENTICATION_USAGE_SERVICE, IAuthenticationUsageService};
use mocks::{
    TestAccessService, TestAuthenticationService, TestExtensionsService, TestMcpAccessService, TestMcpService, TestMcpUsageService, TestUsageService,
    create_provider,
};

struct Fixture {
    query_service: Rc<AuthenticationQueryService>,
    auth_service: Rc<TestAuthenticationService>,
    usage_service: Rc<TestUsageService>,
    mcp_usage_service: Rc<TestMcpUsageService>,
    access_service: Rc<TestAccessService>,
    mcp_access_service: Rc<TestMcpAccessService>,
}

fn setup(disposables: &LeakGuard) -> Fixture {
    // Set up storage service
    let storage_service = disposables.add_rc(InMemoryStorageService::new());
    let storage: Rc<dyn StorageService> = Rc::new(storage_service.clone());

    // Create and register test services
    let auth_service = disposables.add_rc(TestAuthenticationService::new());
    let usage_service = disposables.add_rc(TestUsageService::new());
    let mcp_usage_service = disposables.add_rc(TestMcpUsageService::new());
    let access_service = disposables.add_rc(TestAccessService::new());
    let mcp_access_service = disposables.add_rc(TestMcpAccessService::new());
    let extensions_service = disposables.add_rc(TestExtensionsService::new());
    let mcp_service = disposables.add_rc(TestMcpService::new());

    let authentication_service: Rc<dyn IAuthenticationService> = auth_service.clone();
    let authentication_usage_service: Rc<dyn IAuthenticationUsageService> = usage_service.clone();
    let authentication_mcp_usage_service: Rc<dyn IAuthenticationMcpUsageService> = mcp_usage_service.clone();
    let authentication_access_service: Rc<dyn IAuthenticationAccessService> = access_service.clone();
    let authentication_mcp_access_service: Rc<dyn IAuthenticationMcpAccessService> = mcp_access_service.clone();
    let authentication_extensions_service: Rc<dyn IAuthenticationExtensionsService> = extensions_service;
    let authentication_mcp_service: Rc<dyn IAuthenticationMcpService> = mcp_service;

    let instantiation_service = disposables.add_rc(InstantiationService::new(
        ServiceCollection::new()
            .with(I_STORAGE_SERVICE, storage)
            // Set up log service
            .with(I_LOG_SERVICE, NullLogService::shared())
            .with(I_AUTHENTICATION_SERVICE, authentication_service)
            .with(I_AUTHENTICATION_USAGE_SERVICE, authentication_usage_service)
            .with(I_AUTHENTICATION_MCP_USAGE_SERVICE, authentication_mcp_usage_service)
            .with(I_AUTHENTICATION_ACCESS_SERVICE, authentication_access_service)
            .with(I_AUTHENTICATION_MCP_ACCESS_SERVICE, authentication_mcp_access_service)
            .with(I_AUTHENTICATION_EXTENSIONS_SERVICE, authentication_extensions_service)
            .with(I_AUTHENTICATION_MCP_SERVICE, authentication_mcp_service),
    ));

    // Create the query service
    let query_service = disposables.add_rc(instantiation_service.create_instance::<AuthenticationQueryService>(()));

    Fixture { query_service, auth_service, usage_service, mcp_usage_service, access_service, mcp_access_service }
}

/// Uma lista de textos: `['read', 'write']`.
fn s(items: &[&str]) -> Vec<String> {
    items.iter().map(|item| (*item).to_owned()).collect()
}

/// `{ id, label }`.
fn account(id: &str, label: &str) -> AuthenticationSessionAccount {
    AuthenticationSessionAccount::new(label, id)
}

/// `array.includes(value)`.
fn has(items: &[String], value: &str) -> bool {
    items.iter().any(|item| item == value)
}

/// `map.get(key)`.
fn pref<'a>(map: &'a OrderedMap<String, String>, key: &str) -> Option<&'a str> {
    map.get(&key.to_owned()).map(String::as_str)
}

/// `authService.registerAuthenticationProvider(id, createProvider({ id, label }))`.
fn register_provider(f: &Fixture, id: &str, label: &str) {
    f.auth_service.register_authentication_provider(id, create_provider(id, label));
}

#[test]
fn usage_tracking_stores_and_retrieves_data_correctly() {
    let disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let f = setup(&disposables);
    let extension_query = f.query_service.provider("github").account("user@example.com").extension("my-extension");

    // Initially no usage
    assert_eq!(extension_query.get_usage().len(), 0);

    // Add usage and verify it's stored
    extension_query.add_usage(&s(&["read", "write"]), "My Extension");
    let usage = extension_query.get_usage();
    assert_eq!(usage.len(), 1);
    assert_eq!(usage[0].extension_id, "my-extension");
    assert_eq!(usage[0].extension_name, "My Extension");
    assert_eq!(usage[0].scopes, s(&["read", "write"]));

    // Add more usage and verify accumulation
    extension_query.add_usage(&s(&["admin"]), "My Extension");
    assert_eq!(extension_query.get_usage().len(), 2);
}

#[test]
fn access_control_persists_across_queries() {
    let disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let f = setup(&disposables);
    let extension_query = f.query_service.provider("github").account("user@example.com").extension("my-extension");

    // Set access and verify
    extension_query.set_access_allowed(true, Some("My Extension"));
    assert_eq!(extension_query.is_access_allowed(), Some(true));

    // Create new query object for same target - should persist
    let same_extension_query = f.query_service.provider("github").account("user@example.com").extension("my-extension");
    assert_eq!(same_extension_query.is_access_allowed(), Some(true));

    // Different extension should be unaffected
    let other_extension_query = f.query_service.provider("github").account("user@example.com").extension("other-extension");
    assert_eq!(other_extension_query.is_access_allowed(), None);
}

#[test]
fn account_preferences_work_across_services() {
    let disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let f = setup(&disposables);
    let extension_query = f.query_service.provider("github").extension("my-extension");
    let mcp_query = f.query_service.provider("github").mcp_server("my-server");

    // Set preferences for both
    extension_query.set_preferred_account(&account("user1", "user@example.com"));
    mcp_query.set_preferred_account(&account("user2", "admin@example.com"));

    // Verify different preferences are stored independently
    assert_eq!(extension_query.get_preferred_account().as_deref(), Some("user@example.com"));
    assert_eq!(mcp_query.get_preferred_account().as_deref(), Some("admin@example.com"));

    // Test preference detection
    let user_extension_query = f.query_service.provider("github").account("user@example.com").extension("my-extension");
    let admin_mcp_query = f.query_service.provider("github").account("admin@example.com").mcp_server("my-server");

    assert!(user_extension_query.is_preferred());
    assert!(admin_mcp_query.is_preferred());

    // Test non-preferred accounts
    let wrong_extension_query = f.query_service.provider("github").account("wrong@example.com").extension("my-extension");
    assert!(!wrong_extension_query.is_preferred());
}

#[test]
fn account_removal_cleans_up_all_related_data() {
    let disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let f = setup(&disposables);
    let account_query = f.query_service.provider("github").account("user@example.com");

    // Set up data across multiple services
    account_query.extension("ext1").set_access_allowed(true, Some("Extension 1"));
    account_query.extension("ext1").add_usage(&s(&["read"]), "Extension 1");
    account_query.mcp_server("mcp1").set_access_allowed(true, Some("MCP Server 1"));
    account_query.mcp_server("mcp1").add_usage(&s(&["write"]), "MCP Server 1");

    // Verify data exists
    assert_eq!(account_query.extension("ext1").is_access_allowed(), Some(true));
    assert_eq!(account_query.extension("ext1").get_usage().len(), 1);
    assert_eq!(account_query.mcp_server("mcp1").is_access_allowed(), Some(true));
    assert_eq!(account_query.mcp_server("mcp1").get_usage().len(), 1);

    // Remove account
    account_query.remove();

    // Verify all data is cleaned up
    assert_eq!(account_query.extension("ext1").is_access_allowed(), None);
    assert_eq!(account_query.extension("ext1").get_usage().len(), 0);
    assert_eq!(account_query.mcp_server("mcp1").is_access_allowed(), None);
    assert_eq!(account_query.mcp_server("mcp1").get_usage().len(), 0);
}

#[test]
fn provider_registration_and_listing_works() {
    let disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let f = setup(&disposables);
    // Initially no providers
    assert_eq!(f.query_service.get_provider_ids().len(), 0);

    // Register a provider
    register_provider(&f, "github", "GitHub");

    // Verify provider is listed
    let provider_ids = f.query_service.get_provider_ids();
    assert!(has(&provider_ids, "github"));
    assert!(f.auth_service.is_authentication_provider_registered("github"));
}

#[test]
fn mcp_usage_and_access_work_independently_from_extensions() {
    let disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let f = setup(&disposables);
    let extension_query = f.query_service.provider("github").account("user@example.com").extension("my-extension");
    let mcp_query = f.query_service.provider("github").account("user@example.com").mcp_server("my-server");

    // Set up data for both
    extension_query.set_access_allowed(true, Some("My Extension"));
    extension_query.add_usage(&s(&["read"]), "My Extension");

    mcp_query.set_access_allowed(false, Some("My Server"));
    mcp_query.add_usage(&s(&["write"]), "My Server");

    // Verify they're independent
    assert_eq!(extension_query.is_access_allowed(), Some(true));
    assert_eq!(mcp_query.is_access_allowed(), Some(false));

    assert_eq!(extension_query.get_usage()[0].extension_id, "my-extension");
    assert_eq!(mcp_query.get_usage()[0].mcp_server_id, "my-server");

    // Verify no cross-contamination
    assert_eq!(extension_query.get_usage().len(), 1);
    assert_eq!(mcp_query.get_usage().len(), 1);
}

#[test]
fn get_all_account_preferences_returns_synchronously() {
    let disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let f = setup(&disposables);
    // Register providers for the test
    register_provider(&f, "github", "GitHub");
    register_provider(&f, "azure", "Azure");

    let extension_query = f.query_service.extension("my-extension");
    let mcp_query = f.query_service.mcp_server("my-server");

    // Set preferences for different providers
    extension_query.provider("github").set_preferred_account(&account("user1", "github-user@example.com"));
    extension_query.provider("azure").set_preferred_account(&account("user2", "azure-user@example.com"));
    mcp_query.provider("github").set_preferred_account(&account("user3", "github-mcp@example.com"));

    // Get all preferences synchronously (no await needed)
    let extension_preferences = extension_query.get_all_account_preferences(None);
    let mcp_preferences = mcp_query.get_all_account_preferences(None);

    // Verify extension preferences
    assert_eq!(pref(&extension_preferences, "github"), Some("github-user@example.com"));
    assert_eq!(pref(&extension_preferences, "azure"), Some("azure-user@example.com"));
    assert_eq!(extension_preferences.len(), 2);

    // Verify MCP preferences
    assert_eq!(pref(&mcp_preferences, "github"), Some("github-mcp@example.com"));
    assert_eq!(mcp_preferences.len(), 1);

    // Verify they don't interfere with each other
    assert_ne!(pref(&extension_preferences, "github"), pref(&mcp_preferences, "github"));
}

#[test]
fn for_each_methods_work_synchronously() {
    let disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let f = setup(&disposables);
    let account_query = f.query_service.provider("github").account("user@example.com");

    // Add some usage data first
    account_query.extension("ext1").add_usage(&s(&["read"]), "Extension 1");
    account_query.extension("ext2").add_usage(&s(&["write"]), "Extension 2");
    account_query.mcp_server("mcp1").add_usage(&s(&["admin"]), "MCP Server 1");

    // Test extensions forEach - no await needed
    let mut extension_ids: Vec<String> = Vec::new();
    account_query.extensions().for_each(&mut |extension_query| {
        extension_ids.push(extension_query.extension_id().to_owned());
    });

    assert_eq!(extension_ids.len(), 2);
    assert!(has(&extension_ids, "ext1"));
    assert!(has(&extension_ids, "ext2"));

    // Test MCP servers forEach - no await needed
    let mut mcp_server_ids: Vec<String> = Vec::new();
    account_query.mcp_servers().for_each(&mut |mcp_server_query| {
        mcp_server_ids.push(mcp_server_query.mcp_server_id().to_owned());
    });

    assert_eq!(mcp_server_ids.len(), 1);
    assert!(has(&mcp_server_ids, "mcp1"));
}

#[test]
fn remove_method_works_synchronously() {
    let disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let f = setup(&disposables);
    let account_query = f.query_service.provider("github").account("user@example.com");

    // Set up data
    account_query.extension("ext1").set_access_allowed(true, Some("Extension 1"));
    account_query.mcp_server("mcp1").set_access_allowed(true, Some("MCP Server 1"));

    // Remove synchronously - no await needed
    account_query.remove();

    // Verify data is gone
    assert_eq!(account_query.extension("ext1").is_access_allowed(), None);
    assert_eq!(account_query.mcp_server("mcp1").is_access_allowed(), None);
}

#[test]
fn cross_provider_extension_queries_work_correctly() {
    let disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let f = setup(&disposables);
    // Register multiple providers
    register_provider(&f, "github", "GitHub");
    register_provider(&f, "azure", "Azure");

    // Set up data using provider-first approach
    f.query_service.provider("github").account("user@example.com").extension("my-extension").set_access_allowed(true, Some("My Extension"));
    f.query_service.provider("azure").account("admin@example.com").extension("my-extension").set_access_allowed(false, Some("My Extension"));

    // Query using extension-first approach should return all providers
    let extension_query = f.query_service.extension("my-extension");
    let github_prefs = extension_query.get_all_account_preferences(None);

    // Should include both providers
    let _ = github_prefs.len(); // Extension query should work across providers

    // Test preferences using extension-first query pattern
    extension_query.provider("github").set_preferred_account(&account("user1", "user@example.com"));
    extension_query.provider("azure").set_preferred_account(&account("user2", "admin@example.com"));

    assert_eq!(extension_query.provider("github").get_preferred_account().as_deref(), Some("user@example.com"));
    assert_eq!(extension_query.provider("azure").get_preferred_account().as_deref(), Some("admin@example.com"));
}

#[test]
fn event_forwarding_from_authentication_service_works() {
    let disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let f = setup(&disposables);
    let event_fired = Rc::new(Cell::new(false));

    // Listen for access change events through the query service
    let subscription = {
        let event_fired = event_fired.clone();
        f.query_service.on_did_change_access().listen(move |_| event_fired.set(true))
    };
    disposables.add(subscription);

    // Trigger an access change that should fire an event
    f.query_service.provider("github").account("user@example.com").extension("my-extension").set_access_allowed(true, Some("My Extension"));

    // Verify the event was fired
    assert!(event_fired.get());
}

#[test]
fn error_handling_for_invalid_inputs_works_correctly() {
    let disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let f = setup(&disposables);
    // Test with non-existent provider
    let invalid_provider_query = f.query_service.provider("non-existent-provider");

    // Should not throw, but should handle gracefully
    invalid_provider_query.account("user@example.com").extension("my-extension").is_access_allowed();

    // Test with empty/invalid account names
    let empty_account_query = f.query_service.provider("github").account("").extension("my-extension");
    empty_account_query.is_access_allowed();

    // Test with empty extension IDs
    let empty_extension_query = f.query_service.provider("github").account("user@example.com").extension("");
    empty_extension_query.is_access_allowed();
}

#[test]
fn bulk_operations_work_correctly() {
    let disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let f = setup(&disposables);
    let account_query = f.query_service.provider("github").account("user@example.com");

    // Set up multiple extensions with different access levels
    account_query.extension("ext1").set_access_allowed(true, Some("Extension 1"));
    account_query.extension("ext2").set_access_allowed(false, Some("Extension 2"));
    account_query.extension("ext3").set_access_allowed(true, Some("Extension 3"));

    // Add usage for some extensions
    account_query.extension("ext1").add_usage(&s(&["read"]), "Extension 1");
    account_query.extension("ext3").add_usage(&s(&["write"]), "Extension 3");

    // Test bulk enumeration
    let mut extension_count = 0;
    let mut allowed_count = 0;
    let mut usage_count = 0;

    account_query.extensions().for_each(&mut |extension_query| {
        extension_count += 1;
        if extension_query.is_access_allowed() == Some(true) {
            allowed_count += 1;
        }
        if !extension_query.get_usage().is_empty() {
            usage_count += 1;
        }
    });

    // Verify bulk operation results
    assert_eq!(extension_count, 3);
    assert_eq!(allowed_count, 2); // ext1 and ext3
    assert_eq!(usage_count, 2); // ext1 and ext3

    // Test bulk operations for MCP servers
    account_query.mcp_server("mcp1").set_access_allowed(true, Some("MCP 1"));
    account_query.mcp_server("mcp2").set_access_allowed(false, Some("MCP 2"));

    let mut mcp_count = 0;
    account_query.mcp_servers().for_each(&mut |_mcp_query| {
        mcp_count += 1;
    });

    assert_eq!(mcp_count, 2);
}

#[test]
fn data_consistency_across_different_query_paths() {
    let disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let f = setup(&disposables);
    // Set up data using one query path
    let extension_query1 = f.query_service.provider("github").account("user@example.com").extension("my-extension");
    extension_query1.set_access_allowed(true, Some("My Extension"));
    extension_query1.add_usage(&s(&["read", "write"]), "My Extension");

    // Access same data using different query path (cross-provider query)
    let extension_query2 = f.query_service.extension("my-extension").provider("github");

    // Data should be consistent through provider preference access
    assert_eq!(extension_query1.is_access_allowed(), Some(true));
    assert_eq!(extension_query1.get_usage().len(), 1);

    // Set preferences and check consistency
    extension_query2.set_preferred_account(&account("user", "user@example.com"));
    assert_eq!(extension_query2.get_preferred_account().as_deref(), Some("user@example.com"));

    // Modify through one path
    extension_query1.set_access_allowed(false, Some("My Extension"));

    // Should be reflected when accessing through provider->account path
    assert_eq!(extension_query1.is_access_allowed(), Some(false));
}

#[test]
fn preference_management_handles_complex_scenarios() {
    let disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let f = setup(&disposables);
    // Register multiple providers
    register_provider(&f, "github", "GitHub");
    register_provider(&f, "azure", "Azure");

    let extension_query = f.query_service.extension("my-extension");

    // Set different preferences for different providers
    extension_query.provider("github").set_preferred_account(&account("user1", "github-user@example.com"));
    extension_query.provider("azure").set_preferred_account(&account("user2", "azure-user@example.com"));

    // Test preference retrieval
    assert_eq!(extension_query.provider("github").get_preferred_account().as_deref(), Some("github-user@example.com"));
    assert_eq!(extension_query.provider("azure").get_preferred_account().as_deref(), Some("azure-user@example.com"));

    // Test account preference detection through provider->account queries
    assert!(f.query_service.provider("github").account("github-user@example.com").extension("my-extension").is_preferred());
    assert!(f.query_service.provider("azure").account("azure-user@example.com").extension("my-extension").is_preferred());
    assert!(!f.query_service.provider("github").account("wrong@example.com").extension("my-extension").is_preferred());

    // Test getAllAccountPreferences with multiple providers
    let all_prefs = extension_query.get_all_account_preferences(None);
    assert_eq!(pref(&all_prefs, "github"), Some("github-user@example.com"));
    assert_eq!(pref(&all_prefs, "azure"), Some("azure-user@example.com"));
    assert_eq!(all_prefs.len(), 2);
}

#[test]
fn mcp_server_vs_extension_data_isolation_is_complete() {
    let disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let f = setup(&disposables);
    let account_query = f.query_service.provider("github").account("user@example.com");

    // Set up similar data for extension and MCP server with same IDs
    let same_id = "same-identifier";
    account_query.extension(same_id).set_access_allowed(true, Some("Extension"));
    account_query.extension(same_id).add_usage(&s(&["ext-scope"]), "Extension");

    account_query.mcp_server(same_id).set_access_allowed(false, Some("MCP Server"));
    account_query.mcp_server(same_id).add_usage(&s(&["mcp-scope"]), "MCP Server");

    // Verify complete isolation
    assert_eq!(account_query.extension(same_id).is_access_allowed(), Some(true));
    assert_eq!(account_query.mcp_server(same_id).is_access_allowed(), Some(false));

    let ext_usage = account_query.extension(same_id).get_usage();
    let mcp_usage = account_query.mcp_server(same_id).get_usage();

    assert_eq!(ext_usage.len(), 1);
    assert_eq!(mcp_usage.len(), 1);
    assert_eq!(ext_usage[0].extension_id, same_id);
    assert_eq!(mcp_usage[0].mcp_server_id, same_id);
    assert_ne!(ext_usage[0].scopes, mcp_usage[0].scopes);

    // Test preference isolation
    f.query_service.extension(same_id).provider("github").set_preferred_account(&account("ext-user", "ext@example.com"));
    f.query_service.mcp_server(same_id).provider("github").set_preferred_account(&account("mcp-user", "mcp@example.com"));

    assert_eq!(f.query_service.extension(same_id).provider("github").get_preferred_account().as_deref(), Some("ext@example.com"));
    assert_eq!(f.query_service.mcp_server(same_id).provider("github").get_preferred_account().as_deref(), Some("mcp@example.com"));
}

#[test]
fn provider_listing_and_registration_integration() {
    let disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let f = setup(&disposables);
    // Initially should have providers from setup (if any)
    let initial_providers = f.query_service.get_provider_ids();
    let initial_count = initial_providers.len();

    // Register a new provider
    register_provider(&f, "test-provider", "Test Provider");

    // Should now appear in listing
    let updated_providers = f.query_service.get_provider_ids();
    assert_eq!(updated_providers.len(), initial_count + 1);
    assert!(has(&updated_providers, "test-provider"));

    // Should be able to query the new provider
    let provider_query = f.query_service.provider("test-provider");
    assert_eq!(provider_query.provider_id(), "test-provider");

    // Should integrate with authentication service state
    assert!(f.auth_service.is_authentication_provider_registered("test-provider"));
}

// Service Call Verification Tests
// These tests verify that the AuthenticationQueryService properly delegates to underlying services
// with the correct parameters. This is important for ensuring the facade works correctly.

#[test]
fn set_access_allowed_calls_update_allowed_extensions_with_correct_parameters() {
    let disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let f = setup(&disposables);
    let extension_query = f.query_service.provider("github").account("user@example.com").extension("my-extension");

    // Clear any previous calls
    f.access_service.clear_call_history();

    // Call setAccessAllowed
    extension_query.set_access_allowed(true, Some("My Extension"));

    // Verify the underlying service was called correctly
    let calls = f.access_service.get_calls_for("updateAllowedExtensions");
    assert_eq!(calls.len(), 1);

    let args = &calls[0].args;
    assert_eq!(args[0].text(), "github");
    assert_eq!(args[1].text(), "user@example.com");
    let extensions = args[2].extensions();
    assert_eq!(extensions.len(), 1);
    assert_eq!(extensions[0].id, "my-extension");
    assert_eq!(extensions[0].name, "My Extension");
    assert_eq!(extensions[0].allowed, Some(true));
}

#[test]
fn add_usage_calls_add_account_usage_with_correct_parameters() {
    let disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let f = setup(&disposables);
    let extension_query = f.query_service.provider("azure").account("admin@company.com").extension("test-extension");

    // Clear any previous calls
    f.usage_service.clear_call_history();

    // Call addUsage
    extension_query.add_usage(&s(&["read", "write"]), "Test Extension");

    // Verify the underlying service was called correctly
    let calls = f.usage_service.get_calls_for("addAccountUsage");
    assert_eq!(calls.len(), 1);

    let args = &calls[0].args;
    assert_eq!(args[0].text(), "azure");
    assert_eq!(args[1].text(), "admin@company.com");
    assert_eq!(args[2].texts(), s(&["read", "write"]));
    assert_eq!(args[3].text(), "test-extension");
    assert_eq!(args[4].text(), "Test Extension");
}

#[test]
fn is_access_allowed_calls_underlying_service_with_correct_parameters() {
    let disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let f = setup(&disposables);
    let extension_query = f.query_service.provider("github").account("user@example.com").extension("my-extension");

    // Clear any previous calls
    f.access_service.clear_call_history();

    // Call isAccessAllowed
    extension_query.is_access_allowed();

    // Verify the underlying service was called correctly
    let calls = f.access_service.get_calls_for("isAccessAllowed");
    assert_eq!(calls.len(), 1);

    let args = &calls[0].args;
    assert_eq!(args[0].text(), "github");
    assert_eq!(args[1].text(), "user@example.com");
    assert_eq!(args[2].text(), "my-extension");
}

#[test]
fn get_usage_calls_read_account_usages_with_correct_parameters() {
    let disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let f = setup(&disposables);
    let extension_query = f.query_service.provider("github").account("user@example.com").extension("my-extension");

    // Clear any previous calls
    f.usage_service.clear_call_history();

    // Call getUsage
    extension_query.get_usage();

    // Verify the underlying service was called correctly
    let calls = f.usage_service.get_calls_for("readAccountUsages");
    assert_eq!(calls.len(), 1);

    let args = &calls[0].args;
    assert_eq!(args[0].text(), "github");
    assert_eq!(args[1].text(), "user@example.com");
}

#[test]
fn mcp_set_access_allowed_calls_update_allowed_mcp_servers_with_correct_parameters() {
    let disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let f = setup(&disposables);
    let mcp_query = f.query_service.provider("github").account("user@example.com").mcp_server("my-server");

    // Clear any previous calls
    f.mcp_access_service.clear_call_history();

    // Call setAccessAllowed
    mcp_query.set_access_allowed(false, Some("My MCP Server"));

    // Verify the underlying service was called correctly
    let calls = f.mcp_access_service.get_calls_for("updateAllowedMcpServers");
    assert_eq!(calls.len(), 1);

    let args = &calls[0].args;
    assert_eq!(args[0].text(), "github");
    assert_eq!(args[1].text(), "user@example.com");
    let servers = args[2].mcp_servers();
    assert_eq!(servers.len(), 1);
    assert_eq!(servers[0].id, "my-server");
    assert_eq!(servers[0].name, "My MCP Server");
    assert_eq!(servers[0].allowed, Some(false));
}

#[test]
fn mcp_add_usage_calls_add_account_usage_with_correct_parameters() {
    let disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let f = setup(&disposables);
    let mcp_query = f.query_service.provider("azure").account("admin@company.com").mcp_server("test-server");

    // Clear any previous calls
    f.mcp_usage_service.clear_call_history();

    // Call addUsage
    mcp_query.add_usage(&s(&["admin"]), "Test MCP Server");

    // Verify the underlying service was called correctly
    let calls = f.mcp_usage_service.get_calls_for("addAccountUsage");
    assert_eq!(calls.len(), 1);

    let args = &calls[0].args;
    assert_eq!(args[0].text(), "azure");
    assert_eq!(args[1].text(), "admin@company.com");
    assert_eq!(args[2].texts(), s(&["admin"]));
    assert_eq!(args[3].text(), "test-server");
    assert_eq!(args[4].text(), "Test MCP Server");
}

#[test]
fn account_removal_calls_all_appropriate_cleanup_methods() {
    let disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let f = setup(&disposables);
    let account_query = f.query_service.provider("github").account("user@example.com");

    // Set up some data first
    account_query.extension("ext1").set_access_allowed(true, Some("Extension 1"));
    account_query.extension("ext1").add_usage(&s(&["read"]), "Extension 1");
    account_query.mcp_server("mcp1").set_access_allowed(true, Some("MCP Server 1"));
    account_query.mcp_server("mcp1").add_usage(&s(&["write"]), "MCP Server 1");

    // Clear call history to focus on removal calls
    f.usage_service.clear_call_history();
    f.mcp_usage_service.clear_call_history();
    f.access_service.clear_call_history();
    f.mcp_access_service.clear_call_history();

    // Call remove
    account_query.remove();

    // Verify all cleanup methods were called
    let extension_usage_removal = f.usage_service.get_calls_for("removeAccountUsage");
    let mcp_usage_removal = f.mcp_usage_service.get_calls_for("removeAccountUsage");
    let extension_access_removal = f.access_service.get_calls_for("removeAllowedExtensions");
    let mcp_access_removal = f.mcp_access_service.get_calls_for("removeAllowedMcpServers");

    assert_eq!(extension_usage_removal.len(), 1);
    assert_eq!(mcp_usage_removal.len(), 1);
    assert_eq!(extension_access_removal.len(), 1);
    assert_eq!(mcp_access_removal.len(), 1);

    // Verify all calls use correct parameters
    for call in [&extension_usage_removal[0], &mcp_usage_removal[0], &extension_access_removal[0], &mcp_access_removal[0]] {
        assert_eq!(call.args[0].text(), "github");
        assert_eq!(call.args[1].text(), "user@example.com");
    }
}

#[test]
fn bulk_operations_call_read_account_usages_and_read_allowed_extensions() {
    let disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let f = setup(&disposables);
    let account_query = f.query_service.provider("github").account("user@example.com");

    // Set up some data
    account_query.extension("ext1").set_access_allowed(true, Some("Extension 1"));
    account_query.extension("ext2").add_usage(&s(&["read"]), "Extension 2");

    // Clear call history
    f.usage_service.clear_call_history();
    f.access_service.clear_call_history();

    // Perform bulk operation
    account_query.extensions().for_each(&mut |_extension_query| {
        // Just iterate to trigger the underlying service calls
    });

    // Verify the underlying services were called for bulk enumeration
    let usage_calls = f.usage_service.get_calls_for("readAccountUsages");
    let access_calls = f.access_service.get_calls_for("readAllowedExtensions");

    assert_eq!(usage_calls.len(), 1);
    assert_eq!(access_calls.len(), 1);

    // Verify parameters
    for call in usage_calls.iter().chain(access_calls.iter()) {
        assert_eq!(call.args[0].text(), "github");
        assert_eq!(call.args[1].text(), "user@example.com");
    }
}

#[test]
fn multiple_operations_accumulate_service_calls_correctly() {
    let disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let f = setup(&disposables);
    let extension_query = f.query_service.provider("github").account("user@example.com").extension("my-extension");

    // Clear call history
    f.access_service.clear_call_history();
    f.usage_service.clear_call_history();

    // Perform multiple operations
    extension_query.set_access_allowed(true, Some("My Extension"));
    extension_query.add_usage(&s(&["read"]), "My Extension");
    extension_query.is_access_allowed();
    extension_query.get_usage();
    extension_query.set_access_allowed(false, Some("My Extension"));

    // Verify call counts
    assert_eq!(f.access_service.get_calls_for("updateAllowedExtensions").len(), 2);
    assert_eq!(f.access_service.get_calls_for("isAccessAllowed").len(), 1);
    assert_eq!(f.usage_service.get_calls_for("addAccountUsage").len(), 1);
    assert_eq!(f.usage_service.get_calls_for("readAccountUsages").len(), 1);
}

#[test]
fn get_providers_with_access_filters_internal_providers_by_default() {
    run_with_faked_timers(async move {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let f = setup(&disposables);
        // Register multiple providers including internal ones
        register_provider(&f, "github", "GitHub");
        register_provider(&f, "azure", "Azure");
        register_provider(&f, "__internal1", "Internal Provider 1");
        register_provider(&f, "__internal2", "Internal Provider 2");

        // Add accounts to all providers
        f.auth_service.add_accounts("github", vec![account("user1", "user@github.com")]);
        f.auth_service.add_accounts("azure", vec![account("user2", "user@azure.com")]);
        f.auth_service.add_accounts("__internal1", vec![account("user3", "internal1@example.com")]);
        f.auth_service.add_accounts("__internal2", vec![account("user4", "internal2@example.com")]);

        // Set up access for all providers
        f.query_service.provider("github").account("user@github.com").extension("my-extension").set_access_allowed(true, Some("My Extension"));
        f.query_service.provider("azure").account("user@azure.com").extension("my-extension").set_access_allowed(true, Some("My Extension"));
        f.query_service.provider("__internal1").account("internal1@example.com").extension("my-extension").set_access_allowed(true, Some("My Extension"));
        f.query_service.provider("__internal2").account("internal2@example.com").extension("my-extension").set_access_allowed(true, Some("My Extension"));

        // Test extension query - should exclude internal providers by default
        let extension_query = f.query_service.extension("my-extension");
        let providers_with_access = extension_query.get_providers_with_access(None).await.unwrap();

        assert_eq!(providers_with_access.len(), 2);
        assert!(has(&providers_with_access, "github"));
        assert!(has(&providers_with_access, "azure"));
        assert!(!has(&providers_with_access, "__internal1"));
        assert!(!has(&providers_with_access, "__internal2"));
    });
}

#[test]
fn get_providers_with_access_includes_internal_providers_when_requested() {
    run_with_faked_timers(async move {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let f = setup(&disposables);
        // Register multiple providers including internal ones
        register_provider(&f, "github", "GitHub");
        register_provider(&f, "__internal1", "Internal Provider");

        // Add accounts to all providers
        f.auth_service.add_accounts("github", vec![account("user1", "user@github.com")]);
        f.auth_service.add_accounts("__internal1", vec![account("user2", "internal@example.com")]);

        // Set up access for all providers
        f.query_service.provider("github").account("user@github.com").extension("my-extension").set_access_allowed(true, Some("My Extension"));
        f.query_service.provider("__internal1").account("internal@example.com").extension("my-extension").set_access_allowed(true, Some("My Extension"));

        // Test extension query - should include internal providers when requested
        let extension_query = f.query_service.extension("my-extension");
        let providers_with_access = extension_query.get_providers_with_access(Some(true)).await.unwrap();

        assert_eq!(providers_with_access.len(), 2);
        assert!(has(&providers_with_access, "github"));
        assert!(has(&providers_with_access, "__internal1"));
    });
}

#[test]
fn mcp_server_get_providers_with_access_filters_internal_providers_by_default() {
    run_with_faked_timers(async move {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let f = setup(&disposables);
        // Register multiple providers including internal ones
        register_provider(&f, "github", "GitHub");
        register_provider(&f, "azure", "Azure");
        register_provider(&f, "__internal1", "Internal Provider 1");
        register_provider(&f, "__internal2", "Internal Provider 2");

        // Add accounts to all providers
        f.auth_service.add_accounts("github", vec![account("user1", "user@github.com")]);
        f.auth_service.add_accounts("azure", vec![account("user2", "user@azure.com")]);
        f.auth_service.add_accounts("__internal1", vec![account("user3", "internal1@example.com")]);
        f.auth_service.add_accounts("__internal2", vec![account("user4", "internal2@example.com")]);

        // Set up MCP access for all providers
        f.query_service.provider("github").account("user@github.com").mcp_server("my-server").set_access_allowed(true, Some("My Server"));
        f.query_service.provider("azure").account("user@azure.com").mcp_server("my-server").set_access_allowed(true, Some("My Server"));
        f.query_service.provider("__internal1").account("internal1@example.com").mcp_server("my-server").set_access_allowed(true, Some("My Server"));
        f.query_service.provider("__internal2").account("internal2@example.com").mcp_server("my-server").set_access_allowed(true, Some("My Server"));

        // Test MCP server query - should exclude internal providers by default
        let mcp_server_query = f.query_service.mcp_server("my-server");
        let providers_with_access = mcp_server_query.get_providers_with_access(None).await.unwrap();

        assert_eq!(providers_with_access.len(), 2);
        assert!(has(&providers_with_access, "github"));
        assert!(has(&providers_with_access, "azure"));
        assert!(!has(&providers_with_access, "__internal1"));
        assert!(!has(&providers_with_access, "__internal2"));
    });
}

#[test]
fn mcp_server_get_providers_with_access_includes_internal_providers_when_requested() {
    run_with_faked_timers(async move {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let f = setup(&disposables);
        // Register multiple providers including internal ones
        register_provider(&f, "github", "GitHub");
        register_provider(&f, "__internal1", "Internal Provider");

        // Add accounts to all providers
        f.auth_service.add_accounts("github", vec![account("user1", "user@github.com")]);
        f.auth_service.add_accounts("__internal1", vec![account("user2", "internal@example.com")]);

        // Set up MCP access for all providers
        f.query_service.provider("github").account("user@github.com").mcp_server("my-server").set_access_allowed(true, Some("My Server"));
        f.query_service.provider("__internal1").account("internal@example.com").mcp_server("my-server").set_access_allowed(true, Some("My Server"));

        // Test MCP server query - should include internal providers when requested
        let mcp_server_query = f.query_service.mcp_server("my-server");
        let providers_with_access = mcp_server_query.get_providers_with_access(Some(true)).await.unwrap();

        assert_eq!(providers_with_access.len(), 2);
        assert!(has(&providers_with_access, "github"));
        assert!(has(&providers_with_access, "__internal1"));
    });
}

#[test]
fn internal_provider_filtering_works_with_mixed_access_patterns() {
    run_with_faked_timers(async move {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let f = setup(&disposables);
        // Register mixed providers
        register_provider(&f, "normal", "Normal Provider");
        register_provider(&f, "__internal", "Internal Provider");
        register_provider(&f, "no-access", "No Access Provider");

        // Add accounts to all providers
        f.auth_service.add_accounts("normal", vec![account("user1", "user@normal.com")]);
        f.auth_service.add_accounts("__internal", vec![account("user2", "internal@example.com")]);
        f.auth_service.add_accounts("no-access", vec![account("user3", "user@noaccess.com")]);

        // Set up access only for normal and internal providers
        f.query_service.provider("normal").account("user@normal.com").extension("my-extension").set_access_allowed(true, Some("My Extension"));
        f.query_service.provider("__internal").account("internal@example.com").extension("my-extension").set_access_allowed(true, Some("My Extension"));
        // Note: no-access provider deliberately has no access set

        let extension_query = f.query_service.extension("my-extension");

        // Without includeInternal: should only return normal provider
        let providers_without_internal = extension_query.get_providers_with_access(Some(false)).await.unwrap();
        assert_eq!(providers_without_internal.len(), 1);
        assert!(has(&providers_without_internal, "normal"));
        assert!(!has(&providers_without_internal, "__internal"));
        assert!(!has(&providers_without_internal, "no-access"));

        // With includeInternal: should return both normal and internal
        let providers_with_internal = extension_query.get_providers_with_access(Some(true)).await.unwrap();
        assert_eq!(providers_with_internal.len(), 2);
        assert!(has(&providers_with_internal, "normal"));
        assert!(has(&providers_with_internal, "__internal"));
        assert!(!has(&providers_with_internal, "no-access"));
    });
}

#[test]
fn internal_provider_filtering_respects_the_double_underscore_prefix_exactly() {
    run_with_faked_timers(async move {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let f = setup(&disposables);
        // Register providers with various naming patterns
        register_provider(&f, "regular", "Regular Provider");
        register_provider(&f, "_single", "Single Underscore Provider");
        register_provider(&f, "__double", "Double Underscore Provider");
        register_provider(&f, "___triple", "Triple Underscore Provider");
        register_provider(&f, "mid_underscore", "Middle Underscore Provider");

        // Add accounts to all providers
        f.auth_service.add_accounts("regular", vec![account("user1", "user@regular.com")]);
        f.auth_service.add_accounts("_single", vec![account("user2", "user@single.com")]);
        f.auth_service.add_accounts("__double", vec![account("user3", "user@double.com")]);
        f.auth_service.add_accounts("___triple", vec![account("user4", "user@triple.com")]);
        f.auth_service.add_accounts("mid_underscore", vec![account("user5", "user@middle.com")]);

        // Set up access for all providers
        f.query_service.provider("regular").account("user@regular.com").extension("my-extension").set_access_allowed(true, Some("My Extension"));
        f.query_service.provider("_single").account("user@single.com").extension("my-extension").set_access_allowed(true, Some("My Extension"));
        f.query_service.provider("__double").account("user@double.com").extension("my-extension").set_access_allowed(true, Some("My Extension"));
        f.query_service.provider("___triple").account("user@triple.com").extension("my-extension").set_access_allowed(true, Some("My Extension"));
        f.query_service.provider("mid_underscore").account("user@middle.com").extension("my-extension").set_access_allowed(true, Some("My Extension"));

        let extension_query = f.query_service.extension("my-extension");

        // Without includeInternal: should exclude only providers starting with exactly "__"
        let providers_without_internal = extension_query.get_providers_with_access(Some(false)).await.unwrap();
        assert_eq!(providers_without_internal.len(), 3);
        assert!(has(&providers_without_internal, "regular"));
        assert!(has(&providers_without_internal, "_single"));
        assert!(!has(&providers_without_internal, "__double"));
        assert!(!has(&providers_without_internal, "___triple")); // This starts with __, so should be filtered
        assert!(has(&providers_without_internal, "mid_underscore"));

        // With includeInternal: should include all providers
        let providers_with_internal = extension_query.get_providers_with_access(Some(true)).await.unwrap();
        assert_eq!(providers_with_internal.len(), 5);
        assert!(has(&providers_with_internal, "regular"));
        assert!(has(&providers_with_internal, "_single"));
        assert!(has(&providers_with_internal, "__double"));
        assert!(has(&providers_with_internal, "___triple"));
        assert!(has(&providers_with_internal, "mid_underscore"));
    });
}

#[test]
fn get_all_account_preferences_filters_internal_providers_by_default_for_extensions() {
    let disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let f = setup(&disposables);
    // Register providers
    register_provider(&f, "github", "GitHub");
    register_provider(&f, "azure", "Azure");
    register_provider(&f, "__internal", "Internal");

    // Set preferences
    let extension_query = f.query_service.extension("my-extension");
    extension_query.provider("github").set_preferred_account(&account("user1", "user@github.com"));
    extension_query.provider("azure").set_preferred_account(&account("user2", "user@azure.com"));
    extension_query.provider("__internal").set_preferred_account(&account("user3", "internal@example.com"));

    // Without includeInternal: should exclude internal providers
    let prefs_without_internal = extension_query.get_all_account_preferences(Some(false));
    assert_eq!(prefs_without_internal.len(), 2);
    assert_eq!(pref(&prefs_without_internal, "github"), Some("user@github.com"));
    assert_eq!(pref(&prefs_without_internal, "azure"), Some("user@azure.com"));
    assert_eq!(pref(&prefs_without_internal, "__internal"), None);

    // With includeInternal: should include all providers
    let prefs_with_internal = extension_query.get_all_account_preferences(Some(true));
    assert_eq!(prefs_with_internal.len(), 3);
    assert_eq!(pref(&prefs_with_internal, "github"), Some("user@github.com"));
    assert_eq!(pref(&prefs_with_internal, "azure"), Some("user@azure.com"));
    assert_eq!(pref(&prefs_with_internal, "__internal"), Some("internal@example.com"));

    // Default behavior: should exclude internal providers
    let prefs_default = extension_query.get_all_account_preferences(None);
    assert_eq!(prefs_default.len(), 2);
    assert_eq!(pref(&prefs_default, "__internal"), None);
}

#[test]
fn get_all_account_preferences_filters_internal_providers_by_default_for_mcp_servers() {
    let disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let f = setup(&disposables);
    // Register providers
    register_provider(&f, "github", "GitHub");
    register_provider(&f, "azure", "Azure");
    register_provider(&f, "__internal", "Internal");

    // Set preferences
    let mcp_query = f.query_service.mcp_server("my-server");
    mcp_query.provider("github").set_preferred_account(&account("user1", "user@github.com"));
    mcp_query.provider("azure").set_preferred_account(&account("user2", "user@azure.com"));
    mcp_query.provider("__internal").set_preferred_account(&account("user3", "internal@example.com"));

    // Without includeInternal: should exclude internal providers
    let prefs_without_internal = mcp_query.get_all_account_preferences(Some(false));
    assert_eq!(prefs_without_internal.len(), 2);
    assert_eq!(pref(&prefs_without_internal, "github"), Some("user@github.com"));
    assert_eq!(pref(&prefs_without_internal, "azure"), Some("user@azure.com"));
    assert_eq!(pref(&prefs_without_internal, "__internal"), None);

    // With includeInternal: should include all providers
    let prefs_with_internal = mcp_query.get_all_account_preferences(Some(true));
    assert_eq!(prefs_with_internal.len(), 3);
    assert_eq!(pref(&prefs_with_internal, "github"), Some("user@github.com"));
    assert_eq!(pref(&prefs_with_internal, "azure"), Some("user@azure.com"));
    assert_eq!(pref(&prefs_with_internal, "__internal"), Some("internal@example.com"));

    // Default behavior: should exclude internal providers
    let prefs_default = mcp_query.get_all_account_preferences(None);
    assert_eq!(prefs_default.len(), 2);
    assert_eq!(pref(&prefs_default, "__internal"), None);
}

#[test]
fn clear_all_data_includes_internal_providers_by_default() {
    run_with_faked_timers(async move {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let f = setup(&disposables);
        // Register providers
        register_provider(&f, "github", "GitHub");
        register_provider(&f, "__internal", "Internal");

        // Add accounts
        f.auth_service.add_accounts("github", vec![account("user1", "user@github.com")]);
        f.auth_service.add_accounts("__internal", vec![account("user2", "internal@example.com")]);

        // Set up some data
        f.query_service.provider("github").account("user@github.com").extension("my-extension").set_access_allowed(true, Some("My Extension"));
        f.query_service.provider("__internal").account("internal@example.com").extension("my-extension").set_access_allowed(true, Some("My Extension"));

        // Verify data exists
        assert_eq!(f.query_service.provider("github").account("user@github.com").extension("my-extension").is_access_allowed(), Some(true));
        assert_eq!(f.query_service.provider("__internal").account("internal@example.com").extension("my-extension").is_access_allowed(), Some(true));

        // Clear all data (should include internal providers by default)
        f.query_service.clear_all_data("CLEAR_ALL_AUTH_DATA", None).await.unwrap();

        // Verify all data is cleared
        assert_eq!(f.query_service.provider("github").account("user@github.com").extension("my-extension").is_access_allowed(), None);
        assert_eq!(f.query_service.provider("__internal").account("internal@example.com").extension("my-extension").is_access_allowed(), None);
    });
}

#[test]
fn clear_all_data_can_exclude_internal_providers_when_specified() {
    run_with_faked_timers(async move {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let f = setup(&disposables);
        // Register providers
        register_provider(&f, "github", "GitHub");
        register_provider(&f, "__internal", "Internal");

        // Add accounts
        f.auth_service.add_accounts("github", vec![account("user1", "user@github.com")]);
        f.auth_service.add_accounts("__internal", vec![account("user2", "internal@example.com")]);

        // Set up some data
        f.query_service.provider("github").account("user@github.com").extension("my-extension").set_access_allowed(true, Some("My Extension"));
        f.query_service.provider("__internal").account("internal@example.com").extension("my-extension").set_access_allowed(true, Some("My Extension"));

        // Clear data excluding internal providers
        f.query_service.clear_all_data("CLEAR_ALL_AUTH_DATA", Some(false)).await.unwrap();

        // Verify only non-internal data is cleared
        assert_eq!(f.query_service.provider("github").account("user@github.com").extension("my-extension").is_access_allowed(), None);
        assert_eq!(f.query_service.provider("__internal").account("internal@example.com").extension("my-extension").is_access_allowed(), Some(true));
    });
}

#[test]
fn is_trusted_method_works_with_mock_service() {
    let disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let f = setup(&disposables);
    // Register provider and add account
    register_provider(&f, "github", "GitHub");
    f.auth_service.add_accounts("github", vec![account("user1", "user@github.com")]);

    // Add a server with trusted state manually to the mock
    f.mcp_access_service.update_allowed_mcp_servers(
        "github",
        "user@github.com",
        &[AllowedMcpServer {
            id: "trusted-server".to_owned(),
            name: "Trusted Server".to_owned(),
            allowed: Some(true),
            trusted: Some(true),
            ..AllowedMcpServer::default()
        }],
    );

    // Add a non-trusted server
    f.mcp_access_service.update_allowed_mcp_servers(
        "github",
        "user@github.com",
        &[AllowedMcpServer { id: "non-trusted-server".to_owned(), name: "Non-Trusted Server".to_owned(), allowed: Some(true), ..AllowedMcpServer::default() }],
    );

    // Test trusted server
    let trusted_query = f.query_service.provider("github").account("user@github.com").mcp_server("trusted-server");
    assert!(trusted_query.is_trusted());

    // Test non-trusted server
    let non_trusted_query = f.query_service.provider("github").account("user@github.com").mcp_server("non-trusted-server");
    assert!(!non_trusted_query.is_trusted());
}

#[test]
fn get_allowed_mcp_servers_method_returns_servers_with_trusted_state() {
    let disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let f = setup(&disposables);
    // Register provider and add account
    register_provider(&f, "github", "GitHub");
    f.auth_service.add_accounts("github", vec![account("user1", "user@github.com")]);

    // Add servers manually to the mock
    f.mcp_access_service.update_allowed_mcp_servers(
        "github",
        "user@github.com",
        &[
            AllowedMcpServer {
                id: "trusted-server".to_owned(),
                name: "Trusted Server".to_owned(),
                allowed: Some(true),
                trusted: Some(true),
                ..AllowedMcpServer::default()
            },
            AllowedMcpServer { id: "user-server".to_owned(), name: "User Server".to_owned(), allowed: Some(true), ..AllowedMcpServer::default() },
        ],
    );

    // Get all allowed servers
    let allowed_servers = f.query_service.provider("github").account("user@github.com").mcp_servers().get_allowed_mcp_servers();

    // Should have both servers
    assert_eq!(allowed_servers.len(), 2);

    // Find the trusted server
    let trusted_server = allowed_servers.iter().find(|s| s.id == "trusted-server");
    assert!(trusted_server.is_some());
    let trusted_server = trusted_server.unwrap();
    assert_eq!(trusted_server.trusted, Some(true));
    assert_eq!(trusted_server.allowed, Some(true));

    // Find the user-allowed server
    let user_server = allowed_servers.iter().find(|s| s.id == "user-server");
    assert!(user_server.is_some());
    let user_server = user_server.unwrap();
    assert_eq!(user_server.trusted, None);
    assert_eq!(user_server.allowed, Some(true));
}

#[test]
fn get_allowed_mcp_servers_method_exposes_agent_host_metadata() {
    let disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let f = setup(&disposables);
    register_provider(&f, "github", "GitHub");
    f.auth_service.add_accounts("github", vec![account("user1", "user@github.com")]);

    f.mcp_access_service.update_allowed_mcp_servers(
        "github",
        "user@github.com",
        &[AllowedMcpServer {
            id: "agent-host-mcp:remote/GitHub/https://api.example/mcp".to_owned(),
            name: "GitHub".to_owned(),
            allowed: Some(true),
            agent_host: Some(AgentHost { authority: "remote".to_owned(), label: "SSH: my-host".to_owned() }),
            ..AllowedMcpServer::default()
        }],
    );

    let allowed_servers = f.query_service.provider("github").account("user@github.com").mcp_servers().get_allowed_mcp_servers();
    let agent_host_server = allowed_servers.iter().find(|s| s.id == "agent-host-mcp:remote/GitHub/https://api.example/mcp");
    assert_eq!(
        agent_host_server.and_then(|s| s.agent_host.clone()),
        Some(AgentHost { authority: "remote".to_owned(), label: "SSH: my-host".to_owned() })
    );
}

#[test]
fn get_allowed_extensions_returns_extension_data_with_trusted_state() {
    let disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let f = setup(&disposables);
    // Set up some extension access data
    let account_query = f.query_service.provider("github").account("user@example.com");
    account_query.extension("ext1").set_access_allowed(true, Some("Extension One"));
    account_query.extension("ext2").set_access_allowed(true, Some("Extension Two"));
    account_query.extension("ext1").add_usage(&s(&["read"]), "Extension One");

    let allowed_extensions = account_query.extensions().get_allowed_extensions();

    // Should have both extensions
    assert_eq!(allowed_extensions.len(), 2);

    // Find the first extension
    let ext1 = allowed_extensions.iter().find(|e| e.id == "ext1");
    assert!(ext1.is_some());
    let ext1 = ext1.unwrap();
    assert_eq!(ext1.name, "Extension One");
    assert_eq!(ext1.allowed, Some(true));
    assert_eq!(ext1.trusted, Some(false)); // Not in trusted list
    assert!(ext1.last_used.is_some());

    // Find the second extension
    let ext2 = allowed_extensions.iter().find(|e| e.id == "ext2");
    assert!(ext2.is_some());
    let ext2 = ext2.unwrap();
    assert_eq!(ext2.name, "Extension Two");
    assert_eq!(ext2.allowed, Some(true));
    assert_eq!(ext2.trusted, Some(false)); // Not in trusted list
    assert_eq!(ext2.last_used, None); // No usage
}

mod account_entities_query {
    use super::*;

    #[test]
    fn has_any_usage_returns_false_for_clean_account() {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let f = setup(&disposables);
        let entities_query = f.query_service.provider("github").account("clean@example.com").entities();
        assert!(!entities_query.has_any_usage());
    }

    #[test]
    fn has_any_usage_returns_true_when_extension_has_usage() {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let f = setup(&disposables);
        let account_query = f.query_service.provider("github").account("user@example.com");
        account_query.extension("test-ext").add_usage(&s(&["read"]), "Test Extension");

        let entities_query = account_query.entities();
        assert!(entities_query.has_any_usage());
    }

    #[test]
    fn has_any_usage_returns_true_when_mcp_server_has_usage() {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let f = setup(&disposables);
        let account_query = f.query_service.provider("github").account("user@example.com");
        account_query.mcp_server("test-server").add_usage(&s(&["write"]), "Test Server");

        let entities_query = account_query.entities();
        assert!(entities_query.has_any_usage());
    }

    #[test]
    fn has_any_usage_returns_true_when_extension_has_access() {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let f = setup(&disposables);
        let account_query = f.query_service.provider("github").account("user@example.com");
        account_query.extension("test-ext").set_access_allowed(true, Some("Test Extension"));

        let entities_query = account_query.entities();
        assert!(entities_query.has_any_usage());
    }

    #[test]
    fn has_any_usage_returns_true_when_mcp_server_has_access() {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let f = setup(&disposables);
        let account_query = f.query_service.provider("github").account("user@example.com");
        account_query.mcp_server("test-server").set_access_allowed(true, Some("Test Server"));

        let entities_query = account_query.entities();
        assert!(entities_query.has_any_usage());
    }

    #[test]
    fn get_entity_count_returns_correct_counts() {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let f = setup(&disposables);
        let account_query = f.query_service.provider("github").account("user@example.com");

        // Set up test data
        account_query.extension("ext1").set_access_allowed(true, Some("Extension One"));
        account_query.extension("ext2").set_access_allowed(true, Some("Extension Two"));
        account_query.mcp_server("server1").set_access_allowed(true, Some("Server One"));

        let entities_query = account_query.entities();
        let counts = entities_query.get_entity_count();

        assert_eq!(counts.extensions, 2);
        assert_eq!(counts.mcp_servers, 1);
        assert_eq!(counts.total, 3);
    }

    #[test]
    fn get_entity_count_returns_zero_for_clean_account() {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let f = setup(&disposables);
        let entities_query = f.query_service.provider("github").account("clean@example.com").entities();
        let counts = entities_query.get_entity_count();

        assert_eq!(counts.extensions, 0);
        assert_eq!(counts.mcp_servers, 0);
        assert_eq!(counts.total, 0);
    }

    #[test]
    fn remove_all_access_removes_access_for_all_entity_types() {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let f = setup(&disposables);
        let account_query = f.query_service.provider("github").account("user@example.com");

        // Set up test data
        account_query.extension("ext1").set_access_allowed(true, Some("Extension One"));
        account_query.extension("ext2").set_access_allowed(true, Some("Extension Two"));
        account_query.mcp_server("server1").set_access_allowed(true, Some("Server One"));
        account_query.mcp_server("server2").set_access_allowed(true, Some("Server Two"));

        // Verify initial state
        assert_eq!(account_query.extension("ext1").is_access_allowed(), Some(true));
        assert_eq!(account_query.extension("ext2").is_access_allowed(), Some(true));
        assert_eq!(account_query.mcp_server("server1").is_access_allowed(), Some(true));
        assert_eq!(account_query.mcp_server("server2").is_access_allowed(), Some(true));

        // Remove all access
        let entities_query = account_query.entities();
        entities_query.remove_all_access();

        // Verify all access is removed
        assert_eq!(account_query.extension("ext1").is_access_allowed(), Some(false));
        assert_eq!(account_query.extension("ext2").is_access_allowed(), Some(false));
        assert_eq!(account_query.mcp_server("server1").is_access_allowed(), Some(false));
        assert_eq!(account_query.mcp_server("server2").is_access_allowed(), Some(false));
    }

    #[test]
    fn for_each_iterates_over_all_entity_types() {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let f = setup(&disposables);
        let account_query = f.query_service.provider("github").account("user@example.com");

        // Set up test data
        account_query.extension("ext1").set_access_allowed(true, Some("Extension One"));
        account_query.extension("ext2").add_usage(&s(&["read"]), "Extension Two");
        account_query.mcp_server("server1").set_access_allowed(true, Some("Server One"));
        account_query.mcp_server("server2").add_usage(&s(&["write"]), "Server Two");

        let entities_query = account_query.entities();
        let mut visited_entities: Vec<(String, EntityType)> = Vec::new();

        entities_query.for_each(&mut |entity_id, entity_type| {
            visited_entities.push((entity_id.to_owned(), entity_type));
        });

        // Should visit all entities that have usage or access
        assert_eq!(visited_entities.len(), 4);

        let extensions: Vec<_> = visited_entities.iter().filter(|(_, entity_type)| *entity_type == EntityType::Extension).collect();
        let mcp_servers: Vec<_> = visited_entities.iter().filter(|(_, entity_type)| *entity_type == EntityType::McpServer).collect();

        assert_eq!(extensions.len(), 2);
        assert_eq!(mcp_servers.len(), 2);

        // Check specific entities were visited
        let visited = |id: &str, entity_type: EntityType| visited_entities.iter().any(|(visited_id, visited_type)| visited_id == id && *visited_type == entity_type);
        assert!(visited("ext1", EntityType::Extension));
        assert!(visited("ext2", EntityType::Extension));
        assert!(visited("server1", EntityType::McpServer));
        assert!(visited("server2", EntityType::McpServer));
    }
}
