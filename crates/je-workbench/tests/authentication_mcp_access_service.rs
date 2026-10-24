//! Porte de `src/vs/workbench/services/authentication/test/browser/authenticationMcpAccessService.test.ts`
//! (suíte `AuthenticationMcpAccessService`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` vira o guarda criado no começo de cada caso, e o `setup` é
//!   [`setup`], chamado logo depois (o `disposables.add(...)` do original é `add_rc`).
//! - `TestInstantiationService` com `stub(IStorageService, ...)` e `stub(IProductService, ...)` é um
//!   `InstantiationService` sobre um `ServiceCollection` com os dois serviços; `createInstance` é o
//!   `create_instance` real.
//! - `TestStorageService` é o `InMemoryStorageService` e `TestProductService` é o `ProductService` de
//!   `support::workbench_test_services`, com `trustedMcpAuthAccess` mutável por dentro
//!   (`set_trusted_mcp_auth_access`). O `productService = { ...TestProductService }` de cada `setup` é o
//!   produto novo de cada caso.
//! - `AllowedMcpServer` sem `allowed` (`allowed property is undefined`) é `allowed: None`.
//! - `JSON.parse(storageService.get(...) || '[]')` é `json_parse`, e `storedData.length` e `.id` leem o
//!   [`Value`] resultante.
//! - `assert.ok(x)` sobre um `find` é `is_some()`, `strictEqual(x, undefined)` sobre `trusted` é `None`, e
//!   `deepStrictEqual(stored?.agentHost, {...})` compara com o [`AgentHost`].
//! - O caso `handles non-array product.json configuration gracefully` atribui `'invalid-string'` (um `any`) a
//!   `trustedMcpAuthAccess`; o tipo `StringsOrDictionary` não guarda um texto, então o caso vira o que o
//!   original faz com ele (`typeof 'invalid-string' === 'object'` é falso, logo a lista confiável é vazia):
//!   `None`.
//! - Os `try { ... } finally { disposable.dispose() }` dos casos de evento são o `disposables.add(...)` da
//!   assinatura, descartada no fim do caso.
//! - Os `assert.doesNotThrow` são a própria chamada: um pânico (um `throw`) reprova o caso.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod support;

use std::cell::RefCell;
use std::rc::Rc;

use common::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite};
use je_base::json::Value;
use je_base::jsonc::json_parse;
use je_base::product::{IStringDictionary, StringsOrDictionary};
use je_instantiation::{InstantiationService, ServiceCollection};
use je_platform::product::{I_PRODUCT_SERVICE, IProductService, ProductService};
use je_platform::storage::{AbstractStorageService, I_STORAGE_SERVICE, InMemoryStorageService, StorageScope, StorageService, StorageTarget};
use je_storage::StorageValue;
use je_workbench::services::authentication::authentication_mcp_access_service::{
    AgentHost, AllowedMcpServer, AuthenticationMcpAccessService, IAuthenticationMcpAccessService, McpSessionAccessChange,
};
use support::workbench_test_services::test_product_service;

struct Fixture {
    storage_service: Rc<AbstractStorageService>,
    product_service: Rc<ProductService>,
    authentication_mcp_access_service: Rc<AuthenticationMcpAccessService>,
}

fn setup(disposables: &LeakGuard) -> Fixture {
    // Set up storage service
    let storage_service = disposables.add_rc(InMemoryStorageService::new());
    let storage: Rc<dyn StorageService> = Rc::new(storage_service.clone());

    // Set up product service with no trusted servers by default
    let product_service = test_product_service();
    let product: Rc<dyn IProductService> = product_service.clone();

    let instantiation_service =
        disposables.add_rc(InstantiationService::new(ServiceCollection::new().with(I_STORAGE_SERVICE, storage).with(I_PRODUCT_SERVICE, product)));

    // Create the service instance
    let authentication_mcp_access_service = disposables.add_rc(instantiation_service.create_instance::<AuthenticationMcpAccessService>(()));

    Fixture { storage_service, product_service, authentication_mcp_access_service }
}

fn server(id: &str, name: &str, allowed: bool) -> AllowedMcpServer {
    AllowedMcpServer { id: id.to_owned(), name: name.to_owned(), allowed: Some(allowed), ..AllowedMcpServer::default() }
}

fn http_server(id: &str, name: &str, allowed: bool, url: &str) -> AllowedMcpServer {
    AllowedMcpServer { url: Some(url.to_owned()), ..server(id, name, allowed) }
}

fn trusted_array(ids: &[&str]) -> Option<StringsOrDictionary> {
    Some(StringsOrDictionary::Strings(ids.iter().map(|id| (*id).to_owned()).collect()))
}

fn trusted_object(entries: &[(&str, &[&str])]) -> Option<StringsOrDictionary> {
    let mut dictionary = IStringDictionary::<Vec<String>>::new();
    for (provider_id, ids) in entries {
        dictionary.insert((*provider_id).to_owned(), ids.iter().map(|id| (*id).to_owned()).collect());
    }
    Some(StringsOrDictionary::Dictionary(dictionary))
}

fn find<'a>(servers: &'a [AllowedMcpServer], id: &str) -> Option<&'a AllowedMcpServer> {
    servers.iter().find(|s| s.id == id)
}

/// `JSON.parse(storageService.get('mcpserver-github-user@example.com', StorageScope.APPLICATION) || '[]')`:
/// os itens do array guardado.
fn parse_stored_items(f: &Fixture) -> Vec<Value> {
    let stored = f.storage_service.get("mcpserver-github-user@example.com", StorageScope::Application, None).unwrap_or_else(|| "[]".to_owned());
    match json_parse(&stored.encode_utf16().collect::<Vec<u16>>()) {
        Ok(Value::Array(items)) => items,
        other => panic!("o dado guardado deveria ser um array JSON, veio {other:?}"),
    }
}

fn stored_item_id(item: &Value) -> String {
    match item {
        Value::Object(object) => match object.get("id") {
            Some(Value::String(id)) => id.to_string_lossy(),
            other => panic!("id inesperado {other:?}"),
        },
        other => panic!("item inesperado {other:?}"),
    }
}

/// Assina `onDidChangeMcpSessionAccess` e guarda cada evento, para o caso conferir (o `eventFired`, o
/// `eventData` e o `events` do original); a assinatura é descartada no fim do caso.
fn record_events(f: &Fixture, disposables: &LeakGuard) -> Rc<RefCell<Vec<McpSessionAccessChange>>> {
    let events: Rc<RefCell<Vec<McpSessionAccessChange>>> = Rc::new(RefCell::new(Vec::new()));
    let sink = events.clone();
    disposables.add(f.authentication_mcp_access_service.on_did_change_mcp_session_access().listen(move |e| sink.borrow_mut().push(e.clone())));
    events
}

mod is_access_allowed {
    use super::*;

    #[test]
    fn returns_undefined_for_unknown_mcp_server_with_no_product_configuration() {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let f = setup(&disposables);
        let result = f.authentication_mcp_access_service.is_access_allowed("github", "user@example.com", "unknown-server");
        assert_eq!(result, None);
    }

    #[test]
    fn returns_true_for_trusted_mcp_server_from_product_json_array_format() {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let f = setup(&disposables);
        f.product_service.set_trusted_mcp_auth_access(trusted_array(&["trusted-server-1", "trusted-server-2"]));

        let result = f.authentication_mcp_access_service.is_access_allowed("github", "user@example.com", "trusted-server-1");
        assert_eq!(result, Some(true));
    }

    #[test]
    fn returns_true_for_trusted_mcp_server_from_product_json_object_format() {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let f = setup(&disposables);
        f.product_service.set_trusted_mcp_auth_access(trusted_object(&[("github", &["github-server"]), ("microsoft", &["microsoft-server"])]));

        let result1 = f.authentication_mcp_access_service.is_access_allowed("github", "user@example.com", "github-server");
        assert_eq!(result1, Some(true));

        let result2 = f.authentication_mcp_access_service.is_access_allowed("microsoft", "user@microsoft.com", "microsoft-server");
        assert_eq!(result2, Some(true));
    }

    #[test]
    fn returns_undefined_for_mcp_server_not_in_trusted_list() {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let f = setup(&disposables);
        f.product_service.set_trusted_mcp_auth_access(trusted_array(&["trusted-server"]));

        let result = f.authentication_mcp_access_service.is_access_allowed("github", "user@example.com", "untrusted-server");
        assert_eq!(result, None);
    }

    #[test]
    fn returns_stored_allowed_state_when_server_is_in_storage() {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let f = setup(&disposables);
        // Add server to storage
        f.authentication_mcp_access_service.update_allowed_mcp_servers("github", "user@example.com", &[server("stored-server", "Stored Server", false)]);

        let result = f.authentication_mcp_access_service.is_access_allowed("github", "user@example.com", "stored-server");
        assert_eq!(result, Some(false));
    }

    #[test]
    fn returns_true_for_server_in_storage_with_allowed_true() {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let f = setup(&disposables);
        f.authentication_mcp_access_service.update_allowed_mcp_servers("github", "user@example.com", &[server("allowed-server", "Allowed Server", true)]);

        let result = f.authentication_mcp_access_service.is_access_allowed("github", "user@example.com", "allowed-server");
        assert_eq!(result, Some(true));
    }

    #[test]
    fn returns_true_for_server_in_storage_with_undefined_allowed_property_legacy_behavior() {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let f = setup(&disposables);
        // Simulate legacy data where allowed property didn't exist
        let legacy_server = AllowedMcpServer {
            id: "legacy-server".to_owned(),
            name: "Legacy Server".to_owned(),
            // allowed property is undefined
            ..AllowedMcpServer::default()
        };

        f.authentication_mcp_access_service.update_allowed_mcp_servers("github", "user@example.com", &[legacy_server]);

        let result = f.authentication_mcp_access_service.is_access_allowed("github", "user@example.com", "legacy-server");
        assert_eq!(result, Some(true));
    }

    #[test]
    fn product_json_trusted_servers_take_precedence_over_storage() {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let f = setup(&disposables);
        f.product_service.set_trusted_mcp_auth_access(trusted_array(&["product-trusted-server"]));

        // Try to store the same server as not allowed
        f.authentication_mcp_access_service.update_allowed_mcp_servers(
            "github",
            "user@example.com",
            &[server("product-trusted-server", "Product Trusted Server", false)],
        );

        // Product.json should take precedence
        let result = f.authentication_mcp_access_service.is_access_allowed("github", "user@example.com", "product-trusted-server");
        assert_eq!(result, Some(true));
    }
}

mod is_access_allowed_for_url_url_binding_security {
    use super::*;

    const SERVER_URL: &str = "https://server.example.com/mcp";

    #[test]
    fn grants_access_when_the_supplied_url_matches_the_stored_url() {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let f = setup(&disposables);
        f.authentication_mcp_access_service.update_allowed_mcp_servers("github", "user@example.com", &[http_server("http-server", "HTTP Server", true, SERVER_URL)]);

        let result = f.authentication_mcp_access_service.is_access_allowed_for_url("github", "user@example.com", "http-server", SERVER_URL);
        assert_eq!(result, Some(true));
    }

    #[test]
    fn grants_access_despite_cosmetic_origin_differences_root_slash_host_case() {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let f = setup(&disposables);
        f.authentication_mcp_access_service.update_allowed_mcp_servers(
            "github",
            "user@example.com",
            &[http_server("http-server", "HTTP Server", true, "https://server.example.com")],
        );

        // "foo.com" and "foo.com/" are the same origin, and the host is case-insensitive.
        assert_eq!(
            f.authentication_mcp_access_service.is_access_allowed_for_url("github", "user@example.com", "http-server", "https://server.example.com/"),
            Some(true)
        );
        assert_eq!(
            f.authentication_mcp_access_service.is_access_allowed_for_url("github", "user@example.com", "http-server", "https://SERVER.EXAMPLE.COM"),
            Some(true)
        );
    }

    #[test]
    fn re_prompts_when_a_path_trailing_slash_differs_foo_com_a_vs_foo_com_a_slash() {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let f = setup(&disposables);
        f.authentication_mcp_access_service.update_allowed_mcp_servers(
            "github",
            "user@example.com",
            &[http_server("http-server", "HTTP Server", true, "https://server.example.com/mcp")],
        );

        // A trailing slash on a path points at a different endpoint, so the user must re-consent.
        let result = f.authentication_mcp_access_service.is_access_allowed_for_url("github", "user@example.com", "http-server", "https://server.example.com/mcp/");
        assert_eq!(result, None);
    }

    #[test]
    fn re_prompts_returns_undefined_when_the_url_changed_for_the_same_server_id() {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let f = setup(&disposables);
        f.authentication_mcp_access_service.update_allowed_mcp_servers("github", "user@example.com", &[http_server("http-server", "HTTP Server", true, SERVER_URL)]);

        let result = f.authentication_mcp_access_service.is_access_allowed_for_url("github", "user@example.com", "http-server", "https://evil.example.com/mcp");
        assert_eq!(result, None);
    }

    #[test]
    fn breaks_legacy_grants_that_have_no_stored_url_when_a_url_is_supplied() {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let f = setup(&disposables);
        // Legacy grant: stored before URL binding existed, so it has no URL.
        f.authentication_mcp_access_service.update_allowed_mcp_servers("github", "user@example.com", &[server("http-server", "HTTP Server", true)]);

        let result = f.authentication_mcp_access_service.is_access_allowed_for_url("github", "user@example.com", "http-server", SERVER_URL);
        assert_eq!(result, None);
    }

    #[test]
    fn inspection_is_access_allowed_returns_the_stored_decision_regardless_of_stored_url() {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let f = setup(&disposables);
        f.authentication_mcp_access_service.update_allowed_mcp_servers("github", "user@example.com", &[http_server("http-server", "HTTP Server", true, SERVER_URL)]);

        let result = f.authentication_mcp_access_service.is_access_allowed("github", "user@example.com", "http-server");
        assert_eq!(result, Some(true));
    }

    #[test]
    fn stdio_servers_inspection_no_url_are_unaffected() {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let f = setup(&disposables);
        f.authentication_mcp_access_service.update_allowed_mcp_servers("github", "user@example.com", &[server("stdio-server", "Stdio Server", true)]);

        let result = f.authentication_mcp_access_service.is_access_allowed("github", "user@example.com", "stdio-server");
        assert_eq!(result, Some(true));
    }

    #[test]
    fn product_json_trusted_servers_bypass_the_url_check() {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let f = setup(&disposables);
        f.product_service.set_trusted_mcp_auth_access(trusted_array(&["trusted-http-server"]));

        let result =
            f.authentication_mcp_access_service.is_access_allowed_for_url("github", "user@example.com", "trusted-http-server", "https://anything.example.com/mcp");
        assert_eq!(result, Some(true));
    }

    #[test]
    fn a_management_toggle_that_omits_the_url_does_not_clear_the_stored_binding() {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let f = setup(&disposables);
        f.authentication_mcp_access_service.update_allowed_mcp_servers("github", "user@example.com", &[http_server("http-server", "HTTP Server", true, SERVER_URL)]);

        // Simulate the management UI toggling access without knowledge of the URL.
        f.authentication_mcp_access_service.update_allowed_mcp_servers("github", "user@example.com", &[server("http-server", "HTTP Server", true)]);

        let servers = f.authentication_mcp_access_service.read_allowed_mcp_servers("github", "user@example.com");
        let stored = find(&servers, "http-server");
        assert_eq!(stored.and_then(|s| s.url.as_deref()), Some(SERVER_URL));
        assert_eq!(f.authentication_mcp_access_service.is_access_allowed_for_url("github", "user@example.com", "http-server", SERVER_URL), Some(true));
    }
}

mod read_allowed_mcp_servers {
    use super::*;

    #[test]
    fn returns_empty_array_when_no_data_exists() {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let f = setup(&disposables);
        let result = f.authentication_mcp_access_service.read_allowed_mcp_servers("github", "user@example.com");
        assert_eq!(result.len(), 0);
    }

    #[test]
    fn returns_stored_mcp_servers() {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let f = setup(&disposables);
        let servers = [server("server1", "Server 1", true), server("server2", "Server 2", false)];

        f.authentication_mcp_access_service.update_allowed_mcp_servers("github", "user@example.com", &servers);

        let result = f.authentication_mcp_access_service.read_allowed_mcp_servers("github", "user@example.com");
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].id, "server1");
        assert_eq!(result[0].allowed, Some(true));
        assert_eq!(result[1].id, "server2");
        assert_eq!(result[1].allowed, Some(false));
    }

    #[test]
    fn includes_trusted_servers_from_product_json_array_format() {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let f = setup(&disposables);
        f.product_service.set_trusted_mcp_auth_access(trusted_array(&["trusted-server-1", "trusted-server-2"]));

        let result = f.authentication_mcp_access_service.read_allowed_mcp_servers("github", "user@example.com");
        assert_eq!(result.len(), 2);

        let trusted_server1 = find(&result, "trusted-server-1");
        assert!(trusted_server1.is_some());
        let trusted_server1 = trusted_server1.unwrap();
        assert_eq!(trusted_server1.allowed, Some(true));
        assert_eq!(trusted_server1.trusted, Some(true));
        assert_eq!(trusted_server1.name, "trusted-server-1"); // Should default to ID

        let trusted_server2 = find(&result, "trusted-server-2");
        assert!(trusted_server2.is_some());
        let trusted_server2 = trusted_server2.unwrap();
        assert_eq!(trusted_server2.allowed, Some(true));
        assert_eq!(trusted_server2.trusted, Some(true));
    }

    #[test]
    fn includes_trusted_servers_from_product_json_object_format() {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let f = setup(&disposables);
        f.product_service.set_trusted_mcp_auth_access(trusted_object(&[("github", &["github-server"]), ("microsoft", &["microsoft-server"])]));

        let github_result = f.authentication_mcp_access_service.read_allowed_mcp_servers("github", "user@example.com");
        assert_eq!(github_result.len(), 1);
        assert_eq!(github_result[0].id, "github-server");
        assert_eq!(github_result[0].trusted, Some(true));

        let microsoft_result = f.authentication_mcp_access_service.read_allowed_mcp_servers("microsoft", "user@microsoft.com");
        assert_eq!(microsoft_result.len(), 1);
        assert_eq!(microsoft_result[0].id, "microsoft-server");
        assert_eq!(microsoft_result[0].trusted, Some(true));

        // Provider not in trusted list should return empty (no stored servers)
        let unknown_result = f.authentication_mcp_access_service.read_allowed_mcp_servers("unknown", "user@unknown.com");
        assert_eq!(unknown_result.len(), 0);
    }

    #[test]
    fn merges_stored_servers_with_trusted_servers_from_product_json() {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let f = setup(&disposables);
        f.product_service.set_trusted_mcp_auth_access(trusted_array(&["trusted-server"]));

        // Add some stored servers
        f.authentication_mcp_access_service.update_allowed_mcp_servers("github", "user@example.com", &[server("stored-server", "Stored Server", false)]);

        let result = f.authentication_mcp_access_service.read_allowed_mcp_servers("github", "user@example.com");
        assert_eq!(result.len(), 2);

        let trusted_server = find(&result, "trusted-server");
        assert!(trusted_server.is_some());
        let trusted_server = trusted_server.unwrap();
        assert_eq!(trusted_server.trusted, Some(true));
        assert_eq!(trusted_server.allowed, Some(true));

        let stored_server = find(&result, "stored-server");
        assert!(stored_server.is_some());
        let stored_server = stored_server.unwrap();
        assert_eq!(stored_server.trusted, None);
        assert_eq!(stored_server.allowed, Some(false));
    }

    #[test]
    fn updates_existing_stored_server_to_be_trusted_when_it_appears_in_product_json() {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let f = setup(&disposables);
        // First add a server as stored (not trusted)
        f.authentication_mcp_access_service.update_allowed_mcp_servers("github", "user@example.com", &[server("server-1", "Server 1", false)]);

        // Then make it trusted via product.json
        f.product_service.set_trusted_mcp_auth_access(trusted_array(&["server-1"]));

        let result = f.authentication_mcp_access_service.read_allowed_mcp_servers("github", "user@example.com");
        assert_eq!(result.len(), 1);

        let server = &result[0];
        assert_eq!(server.id, "server-1");
        assert_eq!(server.allowed, Some(true)); // Should be overridden to true
        assert_eq!(server.trusted, Some(true)); // Should be marked as trusted
        assert_eq!(server.name, "Server 1"); // Should keep existing name
    }

    #[test]
    fn handles_malformed_json_in_storage_gracefully() {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let f = setup(&disposables);
        // Manually corrupt the storage
        f.storage_service.store("mcpserver-github-user@example.com", StorageValue::String("invalid json".to_owned()), StorageScope::Application, StorageTarget::User);

        // Should return empty array instead of throwing
        let result = f.authentication_mcp_access_service.read_allowed_mcp_servers("github", "user@example.com");
        assert_eq!(result.len(), 0);
    }

    #[test]
    fn handles_non_array_product_json_configuration_gracefully() {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let f = setup(&disposables);
        // Set up invalid configuration (um texto no lugar da lista: sem lista confiável, ver o topo do arquivo)
        f.product_service.set_trusted_mcp_auth_access(None);

        let result = f.authentication_mcp_access_service.read_allowed_mcp_servers("github", "user@example.com");
        assert_eq!(result.len(), 0);
    }
}

mod update_allowed_mcp_servers {
    use super::*;

    #[test]
    fn stores_new_mcp_servers() {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let f = setup(&disposables);
        let servers = [server("server1", "Server 1", true), server("server2", "Server 2", false)];

        f.authentication_mcp_access_service.update_allowed_mcp_servers("github", "user@example.com", &servers);

        let result = f.authentication_mcp_access_service.read_allowed_mcp_servers("github", "user@example.com");
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].id, "server1");
        assert_eq!(result[1].id, "server2");
    }

    #[test]
    fn updates_existing_mcp_server_allowed_status() {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let f = setup(&disposables);
        // First add a server
        f.authentication_mcp_access_service.update_allowed_mcp_servers("github", "user@example.com", &[server("server1", "Server 1", true)]);

        // Then update its allowed status
        f.authentication_mcp_access_service.update_allowed_mcp_servers("github", "user@example.com", &[server("server1", "Server 1", false)]);

        let result = f.authentication_mcp_access_service.read_allowed_mcp_servers("github", "user@example.com");
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].allowed, Some(false));
    }

    #[test]
    fn updates_existing_mcp_server_name_when_new_name_is_provided() {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let f = setup(&disposables);
        // First add a server with default name
        f.authentication_mcp_access_service.update_allowed_mcp_servers("github", "user@example.com", &[server("server1", "server1", true)]);

        // Then update with a proper name
        f.authentication_mcp_access_service.update_allowed_mcp_servers("github", "user@example.com", &[server("server1", "My Server", true)]);

        let result = f.authentication_mcp_access_service.read_allowed_mcp_servers("github", "user@example.com");
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].name, "My Server");
    }

    #[test]
    fn does_not_update_name_when_new_name_is_same_as_id() {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let f = setup(&disposables);
        // First add a server with a proper name
        f.authentication_mcp_access_service.update_allowed_mcp_servers("github", "user@example.com", &[server("server1", "My Server", true)]);

        // Then try to update with ID as name (should keep existing name)
        f.authentication_mcp_access_service.update_allowed_mcp_servers("github", "user@example.com", &[server("server1", "server1", false)]);

        let result = f.authentication_mcp_access_service.read_allowed_mcp_servers("github", "user@example.com");
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].name, "My Server"); // Should keep original name
        assert_eq!(result[0].allowed, Some(false)); // But allowed status should update
    }

    #[test]
    fn adds_new_servers_while_preserving_existing_ones() {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let f = setup(&disposables);
        // First add one server
        f.authentication_mcp_access_service.update_allowed_mcp_servers("github", "user@example.com", &[server("server1", "Server 1", true)]);

        // Then add another server
        f.authentication_mcp_access_service.update_allowed_mcp_servers("github", "user@example.com", &[server("server2", "Server 2", false)]);

        let result = f.authentication_mcp_access_service.read_allowed_mcp_servers("github", "user@example.com");
        assert_eq!(result.len(), 2);

        let server1 = find(&result, "server1");
        let server2 = find(&result, "server2");
        assert!(server1.is_some());
        assert!(server2.is_some());
        assert_eq!(server1.unwrap().allowed, Some(true));
        assert_eq!(server2.unwrap().allowed, Some(false));
    }

    #[test]
    fn does_not_store_trusted_servers_from_product_json() {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let f = setup(&disposables);
        f.product_service.set_trusted_mcp_auth_access(trusted_array(&["trusted-server"]));

        // Try to update a trusted server
        f.authentication_mcp_access_service.update_allowed_mcp_servers(
            "github",
            "user@example.com",
            &[AllowedMcpServer { trusted: Some(true), ..server("trusted-server", "Trusted Server", false) }, server("user-server", "User Server", true)],
        );

        // Check what's actually stored in storage (not including product.json servers)
        let stored_data = parse_stored_items(&f);

        // Should only contain the user-managed server, not the trusted one
        assert_eq!(stored_data.len(), 1);
        assert_eq!(stored_item_id(&stored_data[0]), "user-server");

        // But readAllowedMcpServers should return both (including trusted from product.json)
        let all_servers = f.authentication_mcp_access_service.read_allowed_mcp_servers("github", "user@example.com");
        assert_eq!(all_servers.len(), 2);
    }

    #[test]
    fn persists_agent_host_metadata_and_preserves_it_when_a_later_toggle_omits_it() {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let f = setup(&disposables);
        let id = "agent-host-mcp:remote/GitHub/https://api.example/mcp";
        f.authentication_mcp_access_service.update_allowed_mcp_servers(
            "github",
            "user@example.com",
            &[AllowedMcpServer {
                url: Some("https://api.example/mcp".to_owned()),
                agent_host: Some(AgentHost { authority: "remote".to_owned(), label: "SSH: my-host".to_owned() }),
                ..server(id, "GitHub", true)
            }],
        );

        // A management toggle (no agentHost/url) must not clear the stored metadata.
        f.authentication_mcp_access_service.update_allowed_mcp_servers("github", "user@example.com", &[server(id, "GitHub", true)]);

        let servers = f.authentication_mcp_access_service.read_allowed_mcp_servers("github", "user@example.com");
        let stored = find(&servers, id);
        assert_eq!(stored.and_then(|s| s.agent_host.clone()), Some(AgentHost { authority: "remote".to_owned(), label: "SSH: my-host".to_owned() }));
    }

    #[test]
    fn fires_on_did_change_mcp_session_access_event() {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let f = setup(&disposables);
        let events = record_events(&f, &disposables);

        f.authentication_mcp_access_service.update_allowed_mcp_servers("github", "user@example.com", &[server("server1", "Server 1", true)]);

        let events = events.borrow();
        assert!(!events.is_empty());
        let event_data = events.last();
        assert!(event_data.is_some());
        let event_data = event_data.unwrap();
        assert_eq!(event_data.provider_id, "github");
        assert_eq!(event_data.account_name, "user@example.com");
    }
}

mod remove_allowed_mcp_servers {
    use super::*;

    #[test]
    fn removes_all_stored_mcp_servers_for_account() {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let f = setup(&disposables);
        // First add some servers
        f.authentication_mcp_access_service.update_allowed_mcp_servers(
            "github",
            "user@example.com",
            &[server("server1", "Server 1", true), server("server2", "Server 2", false)],
        );

        // Verify they exist
        let mut result = f.authentication_mcp_access_service.read_allowed_mcp_servers("github", "user@example.com");
        assert_eq!(result.len(), 2);

        // Remove them
        f.authentication_mcp_access_service.remove_allowed_mcp_servers("github", "user@example.com");

        // Verify they're gone
        result = f.authentication_mcp_access_service.read_allowed_mcp_servers("github", "user@example.com");
        assert_eq!(result.len(), 0);
    }

    #[test]
    fn does_not_affect_trusted_servers_from_product_json() {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let f = setup(&disposables);
        f.product_service.set_trusted_mcp_auth_access(trusted_array(&["trusted-server"]));

        // Add some user-managed servers
        f.authentication_mcp_access_service.update_allowed_mcp_servers("github", "user@example.com", &[server("user-server", "User Server", true)]);

        // Verify both trusted and user servers exist
        let mut result = f.authentication_mcp_access_service.read_allowed_mcp_servers("github", "user@example.com");
        assert_eq!(result.len(), 2);

        // Remove user servers
        f.authentication_mcp_access_service.remove_allowed_mcp_servers("github", "user@example.com");

        // Should still have trusted server
        result = f.authentication_mcp_access_service.read_allowed_mcp_servers("github", "user@example.com");
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].id, "trusted-server");
        assert_eq!(result[0].trusted, Some(true));
    }

    #[test]
    fn fires_on_did_change_mcp_session_access_event() {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let f = setup(&disposables);
        let events = record_events(&f, &disposables);

        f.authentication_mcp_access_service.remove_allowed_mcp_servers("github", "user@example.com");

        let events = events.borrow();
        assert!(!events.is_empty());
        let event_data = events.last();
        assert!(event_data.is_some());
        let event_data = event_data.unwrap();
        assert_eq!(event_data.provider_id, "github");
        assert_eq!(event_data.account_name, "user@example.com");
    }

    #[test]
    fn handles_removal_of_non_existent_data_gracefully() {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let f = setup(&disposables);
        // Should not throw when trying to remove data that doesn't exist
        f.authentication_mcp_access_service.remove_allowed_mcp_servers("nonexistent", "user@example.com");
    }
}

mod on_did_change_mcp_session_access_event {
    use super::*;

    #[test]
    fn event_is_fired_for_each_update_operation() {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let f = setup(&disposables);
        let events = record_events(&f, &disposables);

        // Should fire for update
        f.authentication_mcp_access_service.update_allowed_mcp_servers("github", "user@example.com", &[server("server1", "Server 1", true)]);

        // Should fire for remove
        f.authentication_mcp_access_service.remove_allowed_mcp_servers("github", "user@example.com");

        // Should fire for different account
        f.authentication_mcp_access_service.update_allowed_mcp_servers("microsoft", "admin@company.com", &[server("server2", "Server 2", false)]);

        let events = events.borrow();
        assert_eq!(events.len(), 3);
        assert_eq!(events[0].provider_id, "github");
        assert_eq!(events[0].account_name, "user@example.com");
        assert_eq!(events[1].provider_id, "github");
        assert_eq!(events[1].account_name, "user@example.com");
        assert_eq!(events[2].provider_id, "microsoft");
        assert_eq!(events[2].account_name, "admin@company.com");
    }

    #[test]
    fn multiple_listeners_receive_events() {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let f = setup(&disposables);
        let listener1 = record_events(&f, &disposables);
        let listener2 = record_events(&f, &disposables);

        f.authentication_mcp_access_service.update_allowed_mcp_servers("github", "user@example.com", &[server("server1", "Server 1", true)]);

        assert!(!listener1.borrow().is_empty());
        assert!(!listener2.borrow().is_empty());
    }
}

mod integration_scenarios {
    use super::*;

    #[test]
    fn complete_workflow_add_update_query_remove() {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let f = setup(&disposables);
        let provider_id = "github";
        let account_name = "user@example.com";
        let server_id = "test-server";

        // Initially unknown
        assert_eq!(f.authentication_mcp_access_service.is_access_allowed(provider_id, account_name, server_id), None);

        // Add server as allowed
        f.authentication_mcp_access_service.update_allowed_mcp_servers(provider_id, account_name, &[server(server_id, "Test Server", true)]);

        assert_eq!(f.authentication_mcp_access_service.is_access_allowed(provider_id, account_name, server_id), Some(true));

        // Update to disallowed
        f.authentication_mcp_access_service.update_allowed_mcp_servers(provider_id, account_name, &[server(server_id, "Test Server", false)]);

        assert_eq!(f.authentication_mcp_access_service.is_access_allowed(provider_id, account_name, server_id), Some(false));

        // Remove all
        f.authentication_mcp_access_service.remove_allowed_mcp_servers(provider_id, account_name);

        assert_eq!(f.authentication_mcp_access_service.is_access_allowed(provider_id, account_name, server_id), None);
    }

    #[test]
    fn multiple_providers_and_accounts_are_isolated() {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let f = setup(&disposables);
        // Add data for different combinations
        f.authentication_mcp_access_service.update_allowed_mcp_servers("github", "user1@example.com", &[server("server1", "Server 1", true)]);

        f.authentication_mcp_access_service.update_allowed_mcp_servers("github", "user2@example.com", &[server("server1", "Server 1", false)]);

        f.authentication_mcp_access_service.update_allowed_mcp_servers("microsoft", "user1@example.com", &[server("server1", "Server 1", true)]);

        // Verify isolation
        assert_eq!(f.authentication_mcp_access_service.is_access_allowed("github", "user1@example.com", "server1"), Some(true));
        assert_eq!(f.authentication_mcp_access_service.is_access_allowed("github", "user2@example.com", "server1"), Some(false));
        assert_eq!(f.authentication_mcp_access_service.is_access_allowed("microsoft", "user1@example.com", "server1"), Some(true));

        // Non-existent combinations should return undefined
        assert_eq!(f.authentication_mcp_access_service.is_access_allowed("microsoft", "user2@example.com", "server1"), None);
    }

    #[test]
    fn product_json_configuration_takes_precedence_in_all_scenarios() {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let f = setup(&disposables);
        f.product_service.set_trusted_mcp_auth_access(trusted_object(&[("github", &["trusted-server"]), ("microsoft", &["microsoft-trusted"])]));

        // Trusted servers should always return true regardless of storage
        assert_eq!(f.authentication_mcp_access_service.is_access_allowed("github", "user@example.com", "trusted-server"), Some(true));

        // Try to override via storage
        f.authentication_mcp_access_service.update_allowed_mcp_servers("github", "user@example.com", &[server("trusted-server", "Trusted Server", false)]);

        // Should still return true
        assert_eq!(f.authentication_mcp_access_service.is_access_allowed("github", "user@example.com", "trusted-server"), Some(true));

        // But non-trusted servers should still respect storage
        f.authentication_mcp_access_service.update_allowed_mcp_servers("github", "user@example.com", &[server("user-server", "User Server", false)]);

        assert_eq!(f.authentication_mcp_access_service.is_access_allowed("github", "user@example.com", "user-server"), Some(false));
    }

    #[test]
    fn handles_edge_cases_with_empty_or_null_values() {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let f = setup(&disposables);
        // Empty provider/account names
        f.authentication_mcp_access_service.is_access_allowed("", "", "server1");

        // Empty server arrays
        f.authentication_mcp_access_service.update_allowed_mcp_servers("github", "user@example.com", &[]);

        // Empty server ID/name
        f.authentication_mcp_access_service.update_allowed_mcp_servers("github", "user@example.com", &[server("", "", true)]);
    }
}
