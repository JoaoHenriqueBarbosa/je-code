//! Porte de `src/vs/workbench/services/authentication/test/browser/authenticationAccessService.test.ts` (suíte
//! `AuthenticationAccessService`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` vira o guarda criado no começo de cada caso, e o `setup` é
//!   [`setup`], chamado logo depois (o `disposables.add(...)` do original é `add_rc`).
//! - `TestInstantiationService` com `stub(IStorageService, ...)` e `stub(IProductService, ...)` é um
//!   `InstantiationService` sobre um `ServiceCollection` com os dois serviços; `createInstance` é o
//!   `create_instance` real.
//! - `TestStorageService` é o `InMemoryStorageService` (o `testEmitWillSaveState` não é usado aqui) e
//!   `TestProductService` está em `support::workbench_test_services`, com `trustedExtensionAuthAccess`
//!   mutável. O `teardown` (que zera essa propriedade) não existe: cada caso cria o seu produto.
//! - `AllowedExtension` sem `allowed` (`allowed property is undefined`) é `allowed: None`.
//! - `JSON.parse(storedData)` é `json_parse`, e `parsedData.length` e `.id` leem o [`Value`] resultante.
//! - `assert.ok(x)` sobre um `find` é `is_some()`, e `strictEqual(x, undefined)` sobre `trusted` é `None`.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod support;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use common::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite};
use je_base::json::Value;
use je_base::jsonc::json_parse;
use je_base::product::{IStringDictionary, StringsOrDictionary};
use je_instantiation::{InstantiationService, ServiceCollection};
use je_platform::product::{I_PRODUCT_SERVICE, IProductService};
use je_platform::storage::{AbstractStorageService, I_STORAGE_SERVICE, InMemoryStorageService, StorageScope, StorageService, StorageTarget};
use je_storage::StorageValue;
use je_workbench::services::authentication::authentication::AllowedExtension;
use je_workbench::services::authentication::authentication_access_service::{
    AuthenticationAccessService, ExtensionSessionAccessChange, IAuthenticationAccessService,
};
use je_platform::product::ProductService;
use support::workbench_test_services::test_product_service;

struct Fixture {
    storage_service: Rc<AbstractStorageService>,
    product_service: Rc<ProductService>,
    authentication_access_service: Rc<AuthenticationAccessService>,
}

fn setup(disposables: &LeakGuard) -> Fixture {
    // Set up storage service
    let storage_service = disposables.add_rc(InMemoryStorageService::new());
    let storage: Rc<dyn StorageService> = Rc::new(storage_service.clone());

    // Set up product service with no trusted extensions by default
    let product_service = test_product_service();
    let product: Rc<dyn IProductService> = product_service.clone();

    let instantiation_service =
        disposables.add_rc(InstantiationService::new(ServiceCollection::new().with(I_STORAGE_SERVICE, storage).with(I_PRODUCT_SERVICE, product)));

    // Create the service instance
    let authentication_access_service = disposables.add_rc(instantiation_service.create_instance::<AuthenticationAccessService>(()));

    Fixture { storage_service, product_service, authentication_access_service }
}

fn extension(id: &str, name: &str, allowed: bool) -> AllowedExtension {
    AllowedExtension { id: id.to_owned(), name: name.to_owned(), allowed: Some(allowed), ..AllowedExtension::default() }
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

fn find<'a>(extensions: &'a [AllowedExtension], id: &str) -> Option<&'a AllowedExtension> {
    extensions.iter().find(|e| e.id == id)
}

/// O texto guardado em `github-user@example.com` (`storageService.get(..., StorageScope.APPLICATION)`).
fn stored_data(fixture: &Fixture) -> Option<String> {
    fixture.storage_service.get("github-user@example.com", StorageScope::Application, None)
}

/// `JSON.parse(storedData)`: os itens do array guardado.
fn parse_stored_items(stored: &str) -> Vec<Value> {
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

/// Um `e => e.id === id` sobre os itens do `JSON.parse`.
fn stored_has_id(items: &[Value], id: &str) -> bool {
    items.iter().any(|item| stored_item_id(item) == id)
}

mod is_access_allowed {
    use super::*;

    #[test]
    fn returns_undefined_for_unknown_extension_with_no_product_configuration() {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let f = setup(&disposables);
        let result = f.authentication_access_service.is_access_allowed("github", "user@example.com", "unknown-extension");
        assert_eq!(result, None);
    }

    #[test]
    fn returns_true_for_trusted_extension_from_product_json_array_format() {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let f = setup(&disposables);
        f.product_service.set_trusted_extension_auth_access(trusted_array(&["trusted-extension-1", "trusted-extension-2"]));

        let result = f.authentication_access_service.is_access_allowed("github", "user@example.com", "trusted-extension-1");
        assert_eq!(result, Some(true));
    }

    #[test]
    fn returns_true_for_trusted_extension_from_product_json_object_format() {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let f = setup(&disposables);
        f.product_service.set_trusted_extension_auth_access(trusted_object(&[("github", &["github-extension"]), ("microsoft", &["microsoft-extension"])]));

        let result1 = f.authentication_access_service.is_access_allowed("github", "user@example.com", "github-extension");
        assert_eq!(result1, Some(true));

        let result2 = f.authentication_access_service.is_access_allowed("microsoft", "user@microsoft.com", "microsoft-extension");
        assert_eq!(result2, Some(true));
    }

    #[test]
    fn returns_undefined_for_extension_not_in_trusted_list() {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let f = setup(&disposables);
        f.product_service.set_trusted_extension_auth_access(trusted_array(&["trusted-extension"]));

        let result = f.authentication_access_service.is_access_allowed("github", "user@example.com", "untrusted-extension");
        assert_eq!(result, None);
    }

    #[test]
    fn returns_stored_allowed_state_when_extension_is_in_storage() {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let f = setup(&disposables);
        // Add extension to storage
        f.authentication_access_service.update_allowed_extensions("github", "user@example.com", &[extension("stored-extension", "Stored Extension", false)]);

        let result = f.authentication_access_service.is_access_allowed("github", "user@example.com", "stored-extension");
        assert_eq!(result, Some(false));
    }

    #[test]
    fn returns_true_for_extension_in_storage_with_allowed_true() {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let f = setup(&disposables);
        f.authentication_access_service.update_allowed_extensions("github", "user@example.com", &[extension("allowed-extension", "Allowed Extension", true)]);

        let result = f.authentication_access_service.is_access_allowed("github", "user@example.com", "allowed-extension");
        assert_eq!(result, Some(true));
    }

    #[test]
    fn returns_true_for_extension_in_storage_with_undefined_allowed_property_legacy_behavior() {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let f = setup(&disposables);
        // Simulate legacy data where allowed property didn't exist
        let legacy_extension = AllowedExtension {
            id: "legacy-extension".to_owned(),
            name: "Legacy Extension".to_owned(),
            // allowed property is undefined
            ..AllowedExtension::default()
        };

        f.authentication_access_service.update_allowed_extensions("github", "user@example.com", &[legacy_extension]);

        let result = f.authentication_access_service.is_access_allowed("github", "user@example.com", "legacy-extension");
        assert_eq!(result, Some(true));
    }

    #[test]
    fn product_json_trusted_extensions_take_precedence_over_storage() {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let f = setup(&disposables);
        f.product_service.set_trusted_extension_auth_access(trusted_array(&["product-trusted-extension"]));

        // Try to store the same extension as not allowed
        f.authentication_access_service.update_allowed_extensions(
            "github",
            "user@example.com",
            &[extension("product-trusted-extension", "Product Trusted Extension", false)],
        );

        // Product.json should take precedence
        let result = f.authentication_access_service.is_access_allowed("github", "user@example.com", "product-trusted-extension");
        assert_eq!(result, Some(true));
    }
}

mod read_allowed_extensions {
    use super::*;

    #[test]
    fn returns_empty_array_when_no_data_exists() {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let f = setup(&disposables);
        let result = f.authentication_access_service.read_allowed_extensions("github", "user@example.com");
        assert_eq!(result.len(), 0);
    }

    #[test]
    fn returns_stored_extensions() {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let f = setup(&disposables);
        let extensions = [extension("extension1", "Extension 1", true), extension("extension2", "Extension 2", false)];

        f.authentication_access_service.update_allowed_extensions("github", "user@example.com", &extensions);

        let result = f.authentication_access_service.read_allowed_extensions("github", "user@example.com");
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].id, "extension1");
        assert_eq!(result[0].allowed, Some(true));
        assert_eq!(result[1].id, "extension2");
        assert_eq!(result[1].allowed, Some(false));
    }

    #[test]
    fn includes_trusted_extensions_from_product_json_array_format() {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let f = setup(&disposables);
        f.product_service.set_trusted_extension_auth_access(trusted_array(&["trusted-extension-1", "trusted-extension-2"]));

        let result = f.authentication_access_service.read_allowed_extensions("github", "user@example.com");
        assert_eq!(result.len(), 2);

        let trusted_extension1 = find(&result, "trusted-extension-1");
        assert!(trusted_extension1.is_some());
        let trusted_extension1 = trusted_extension1.unwrap();
        assert_eq!(trusted_extension1.allowed, Some(true));
        assert_eq!(trusted_extension1.trusted, Some(true));
        assert_eq!(trusted_extension1.name, "trusted-extension-1"); // Should default to ID

        let trusted_extension2 = find(&result, "trusted-extension-2");
        assert!(trusted_extension2.is_some());
        let trusted_extension2 = trusted_extension2.unwrap();
        assert_eq!(trusted_extension2.allowed, Some(true));
        assert_eq!(trusted_extension2.trusted, Some(true));
    }

    #[test]
    fn includes_trusted_extensions_from_product_json_object_format() {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let f = setup(&disposables);
        f.product_service.set_trusted_extension_auth_access(trusted_object(&[("github", &["github-extension"]), ("microsoft", &["microsoft-extension"])]));

        let github_result = f.authentication_access_service.read_allowed_extensions("github", "user@example.com");
        assert_eq!(github_result.len(), 1);
        assert_eq!(github_result[0].id, "github-extension");
        assert_eq!(github_result[0].trusted, Some(true));

        let microsoft_result = f.authentication_access_service.read_allowed_extensions("microsoft", "user@microsoft.com");
        assert_eq!(microsoft_result.len(), 1);
        assert_eq!(microsoft_result[0].id, "microsoft-extension");
        assert_eq!(microsoft_result[0].trusted, Some(true));

        // Provider not in trusted list should return empty (no stored extensions)
        let unknown_result = f.authentication_access_service.read_allowed_extensions("unknown", "user@unknown.com");
        assert_eq!(unknown_result.len(), 0);
    }

    #[test]
    fn merges_stored_extensions_with_trusted_extensions_from_product_json() {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let f = setup(&disposables);
        f.product_service.set_trusted_extension_auth_access(trusted_array(&["trusted-extension"]));

        // Add some stored extensions
        f.authentication_access_service.update_allowed_extensions("github", "user@example.com", &[extension("stored-extension", "Stored Extension", false)]);

        let result = f.authentication_access_service.read_allowed_extensions("github", "user@example.com");
        assert_eq!(result.len(), 2);

        let trusted_extension = find(&result, "trusted-extension");
        assert!(trusted_extension.is_some());
        let trusted_extension = trusted_extension.unwrap();
        assert_eq!(trusted_extension.trusted, Some(true));
        assert_eq!(trusted_extension.allowed, Some(true));

        let stored_extension = find(&result, "stored-extension");
        assert!(stored_extension.is_some());
        let stored_extension = stored_extension.unwrap();
        assert_eq!(stored_extension.trusted, None);
        assert_eq!(stored_extension.allowed, Some(false));
    }

    #[test]
    fn updates_existing_stored_extension_to_trusted_when_found_in_product_json() {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let f = setup(&disposables);
        // First add an extension to storage
        f.authentication_access_service.update_allowed_extensions("github", "user@example.com", &[extension("extension1", "Extension 1", false)]);

        // Then add it to trusted list
        f.product_service.set_trusted_extension_auth_access(trusted_array(&["extension1"]));

        let result = f.authentication_access_service.read_allowed_extensions("github", "user@example.com");
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].id, "extension1");
        assert_eq!(result[0].trusted, Some(true));
        assert_eq!(result[0].allowed, Some(true)); // Should be marked as allowed due to being trusted
    }

    #[test]
    fn handles_malformed_storage_data_gracefully() {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let f = setup(&disposables);
        // Directly store malformed data in storage
        f.storage_service.store("github-user@example.com", StorageValue::String("invalid-json".to_owned()), StorageScope::Application, StorageTarget::User);

        let result = f.authentication_access_service.read_allowed_extensions("github", "user@example.com");
        assert_eq!(result.len(), 0); // Should return empty array instead of throwing
    }
}

mod update_allowed_extensions {
    use super::*;

    #[test]
    fn adds_new_extensions_to_storage() {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let f = setup(&disposables);
        let extensions = [extension("extension1", "Extension 1", true), extension("extension2", "Extension 2", false)];

        f.authentication_access_service.update_allowed_extensions("github", "user@example.com", &extensions);

        let result = f.authentication_access_service.read_allowed_extensions("github", "user@example.com");
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].id, "extension1");
        assert_eq!(result[1].id, "extension2");
    }

    #[test]
    fn updates_existing_extension_allowed_status() {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let f = setup(&disposables);
        // First add an extension
        f.authentication_access_service.update_allowed_extensions("github", "user@example.com", &[extension("extension1", "Extension 1", true)]);

        // Then update its allowed status
        f.authentication_access_service.update_allowed_extensions("github", "user@example.com", &[extension("extension1", "Extension 1", false)]);

        let result = f.authentication_access_service.read_allowed_extensions("github", "user@example.com");
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].allowed, Some(false));
    }

    #[test]
    fn updates_existing_extension_name_when_new_name_is_provided() {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let f = setup(&disposables);
        // First add an extension with default name
        f.authentication_access_service.update_allowed_extensions("github", "user@example.com", &[extension("extension1", "extension1", true)]);

        // Then update with a proper name
        f.authentication_access_service.update_allowed_extensions("github", "user@example.com", &[extension("extension1", "My Extension", true)]);

        let result = f.authentication_access_service.read_allowed_extensions("github", "user@example.com");
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].name, "My Extension");
    }

    #[test]
    fn does_not_update_name_when_new_name_is_same_as_id() {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let f = setup(&disposables);
        // First add an extension with a proper name
        f.authentication_access_service.update_allowed_extensions("github", "user@example.com", &[extension("extension1", "My Extension", true)]);

        // Then try to update with ID as name (should keep existing name)
        f.authentication_access_service.update_allowed_extensions("github", "user@example.com", &[extension("extension1", "extension1", false)]);

        let result = f.authentication_access_service.read_allowed_extensions("github", "user@example.com");
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].name, "My Extension"); // Should keep the original name
        assert_eq!(result[0].allowed, Some(false)); // But update the allowed status
    }

    #[test]
    fn does_not_store_trusted_extensions_they_should_only_come_from_product_json() {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let f = setup(&disposables);
        f.product_service.set_trusted_extension_auth_access(trusted_array(&["trusted-extension"]));

        // Try to store a trusted extension along with regular extensions
        f.authentication_access_service.update_allowed_extensions(
            "github",
            "user@example.com",
            &[extension("regular-extension", "Regular Extension", true), extension("trusted-extension", "Trusted Extension", false)],
        );

        // Check what's actually stored in storage (should only be the regular extension)
        let stored = stored_data(&f);
        assert!(stored.is_some());
        let parsed_data = parse_stored_items(&stored.unwrap());
        assert_eq!(parsed_data.len(), 1);
        assert_eq!(stored_item_id(&parsed_data[0]), "regular-extension");

        // But when we read, we should get both (trusted from product.json + stored)
        let result = f.authentication_access_service.read_allowed_extensions("github", "user@example.com");
        assert_eq!(result.len(), 2);

        let trusted_ext = find(&result, "trusted-extension");
        assert!(trusted_ext.is_some());
        let trusted_ext = trusted_ext.unwrap();
        assert_eq!(trusted_ext.trusted, Some(true));
        assert_eq!(trusted_ext.allowed, Some(true)); // Should be true from product.json, not false from storage

        let regular_ext = find(&result, "regular-extension");
        assert!(regular_ext.is_some());
        let regular_ext = regular_ext.unwrap();
        assert_eq!(regular_ext.trusted, None);
        assert_eq!(regular_ext.allowed, Some(true));
    }

    #[test]
    fn filters_out_trusted_extensions_before_storing() {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let f = setup(&disposables);
        f.product_service.set_trusted_extension_auth_access(trusted_array(&["trusted-ext-1", "trusted-ext-2"]));

        // Add both trusted and regular extensions
        let extensions = [
            extension("regular-ext", "Regular Extension", true),
            extension("trusted-ext-1", "Trusted Extension 1", false),
            extension("another-regular-ext", "Another Regular Extension", false),
            extension("trusted-ext-2", "Trusted Extension 2", true),
        ];

        f.authentication_access_service.update_allowed_extensions("github", "user@example.com", &extensions);

        // Check storage - should only contain regular extensions
        let stored = stored_data(&f);
        assert!(stored.is_some());
        let parsed_data = parse_stored_items(&stored.unwrap());
        assert_eq!(parsed_data.len(), 2);
        assert!(stored_has_id(&parsed_data, "regular-ext"));
        assert!(stored_has_id(&parsed_data, "another-regular-ext"));
        assert!(!stored_has_id(&parsed_data, "trusted-ext-1"));
        assert!(!stored_has_id(&parsed_data, "trusted-ext-2"));
    }

    #[test]
    fn fires_on_did_change_extension_session_access_event() {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let f = setup(&disposables);
        let event_fired = Rc::new(Cell::new(false));
        let event_data: Rc<RefCell<Option<ExtensionSessionAccessChange>>> = Rc::new(RefCell::new(None));

        let subscription = {
            let (event_fired, event_data) = (event_fired.clone(), event_data.clone());
            f.authentication_access_service.on_did_change_extension_session_access().listen(move |e| {
                event_fired.set(true);
                *event_data.borrow_mut() = Some(e.clone());
            })
        };
        disposables.add(subscription);

        f.authentication_access_service.update_allowed_extensions("github", "user@example.com", &[extension("extension1", "Extension 1", true)]);

        assert!(event_fired.get());
        let event_data = event_data.borrow();
        assert!(event_data.is_some());
        let event_data = event_data.as_ref().unwrap();
        assert_eq!(event_data.provider_id, "github");
        assert_eq!(event_data.account_name, "user@example.com");
    }
}

mod remove_allowed_extensions {
    use super::*;

    #[test]
    fn removes_all_extensions_from_storage() {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let f = setup(&disposables);
        // First add some extensions
        f.authentication_access_service.update_allowed_extensions(
            "github",
            "user@example.com",
            &[extension("extension1", "Extension 1", true), extension("extension2", "Extension 2", false)],
        );

        // Verify they exist
        let result = f.authentication_access_service.read_allowed_extensions("github", "user@example.com");
        assert!(!result.is_empty());

        // Remove them
        f.authentication_access_service.remove_allowed_extensions("github", "user@example.com");

        // Verify storage is empty (but trusted extensions from product.json might still be there)
        assert_eq!(stored_data(&f), None);
    }

    #[test]
    fn fires_on_did_change_extension_session_access_event() {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let f = setup(&disposables);
        let event_fired = Rc::new(Cell::new(false));
        let event_data: Rc<RefCell<Option<ExtensionSessionAccessChange>>> = Rc::new(RefCell::new(None));

        // First add an extension
        f.authentication_access_service.update_allowed_extensions("github", "user@example.com", &[extension("extension1", "Extension 1", true)]);

        // Then listen for the remove event
        let subscription = {
            let (event_fired, event_data) = (event_fired.clone(), event_data.clone());
            f.authentication_access_service.on_did_change_extension_session_access().listen(move |e| {
                event_fired.set(true);
                *event_data.borrow_mut() = Some(e.clone());
            })
        };
        disposables.add(subscription);

        f.authentication_access_service.remove_allowed_extensions("github", "user@example.com");

        assert!(event_fired.get());
        let event_data = event_data.borrow();
        assert!(event_data.is_some());
        let event_data = event_data.as_ref().unwrap();
        assert_eq!(event_data.provider_id, "github");
        assert_eq!(event_data.account_name, "user@example.com");
    }

    #[test]
    fn does_not_affect_trusted_extensions_from_product_json() {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let f = setup(&disposables);
        f.product_service.set_trusted_extension_auth_access(trusted_array(&["trusted-extension"]));

        // Add some regular extensions and verify both trusted and regular exist
        f.authentication_access_service.update_allowed_extensions("github", "user@example.com", &[extension("regular-extension", "Regular Extension", true)]);

        let mut result = f.authentication_access_service.read_allowed_extensions("github", "user@example.com");
        assert_eq!(result.len(), 2); // 1 trusted + 1 regular

        // Remove stored extensions
        f.authentication_access_service.remove_allowed_extensions("github", "user@example.com");

        // Trusted extension should still be there
        result = f.authentication_access_service.read_allowed_extensions("github", "user@example.com");
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].id, "trusted-extension");
        assert_eq!(result[0].trusted, Some(true));
    }
}

mod integration_with_product_json_configurations {
    use super::*;

    #[test]
    fn handles_switching_between_array_and_object_format() {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let f = setup(&disposables);
        // Start with array format
        f.product_service.set_trusted_extension_auth_access(trusted_array(&["ext1", "ext2"]));
        let mut result = f.authentication_access_service.read_allowed_extensions("github", "user@example.com");
        assert_eq!(result.len(), 2);

        // Switch to object format
        f.product_service.set_trusted_extension_auth_access(trusted_object(&[("github", &["ext1", "ext3"]), ("microsoft", &["ext4"])]));
        result = f.authentication_access_service.read_allowed_extensions("github", "user@example.com");
        assert_eq!(result.len(), 2); // ext1 and ext3 for github
        assert!(find(&result, "ext1").is_some());
        assert!(find(&result, "ext3").is_some());
        assert!(find(&result, "ext2").is_none()); // Should not be there anymore
    }

    #[test]
    fn handles_empty_trusted_extension_configurations() {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let f = setup(&disposables);
        // Test undefined
        f.product_service.set_trusted_extension_auth_access(None);
        let mut result = f.authentication_access_service.read_allowed_extensions("github", "user@example.com");
        assert_eq!(result.len(), 0);

        // Test empty array
        f.product_service.set_trusted_extension_auth_access(trusted_array(&[]));
        result = f.authentication_access_service.read_allowed_extensions("github", "user@example.com");
        assert_eq!(result.len(), 0);

        // Test empty object
        f.product_service.set_trusted_extension_auth_access(trusted_object(&[]));
        result = f.authentication_access_service.read_allowed_extensions("github", "user@example.com");
        assert_eq!(result.len(), 0);
    }
}
