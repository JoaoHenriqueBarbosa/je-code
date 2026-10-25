//! Porte de `src/vs/workbench/api/browser/mainThreadStorage.ts`: a ponte entre o `IExtensionStorageService` do workbench e o
//! `globalState` e o `workspaceState` que as extensões leem e gravam pelo `ExtensionContext`.
//!
//! Divergências:
//! - `@extHostNamedCustomer(MainContext.MainThreadStorage)` e o `IExtHostContext` do construtor: o registro de clientes não
//!   existe ainda; o construtor recebe o [`IMainContext`] e pede o proxy de `ExtHostStorage`. `_storageListener` é o
//!   [`DisposableStore`] do campo `storage_listener`.
//! - `IExtensionStorageService` e `IStorageService` são o [`IExtensionStorageService`] e o [`StorageService`]; o `IStorageService` só
//!   serve à assinatura de `onDidChangeValue`.
//! - O `value: object` de `$setValue` é um [`Dynamic`] e vira o [`serde_json::Value`] que o serviço guarda (via `JSON.stringify`);
//!   um valor sem representação em JSON apaga o estado.
//! - `getExtensionState(...)` verdadeiro é um valor presente que não é `null`.
//! - `isWeb` é `Platform::detect().is_web`.
//! - O `IInstantiationService` é o [`InstantiationService`], repassado a `migrateExtensionStorage`.

use std::cell::RefCell;
use std::collections::HashSet;
use std::rc::Rc;

use je_base::async_::async_fn;
use je_base::dynamic::{json_stringify, Dynamic};
use je_base::lifecycle::{Disposable, DisposableStore, IDisposable};
use je_base::platform::Platform;
use je_base::promise::Promise;
use je_instantiation::InstantiationService;
use je_platform::log::{LogService, Logger};
use je_platform::storage::{StorageScope, StorageService};
use serde_json::Value;

use crate::api::common::ext_host_protocol::{ExtHostContext, ExtHostStorageShape, IExtensionIdWithVersion, IMainContext, MainThreadStorageShape};
use crate::services::extension_management::extension_storage::{ExtensionIdWithVersion, ExtensionStorageTarget, IExtensionStorageService};
use crate::services::extensions::extension_storage_migration::migrate_extension_storage;

/// `MainThreadStorage`.
pub struct MainThreadStorage {
    proxy: Rc<dyn ExtHostStorageShape>,
    storage_listener: Rc<DisposableStore>,
    shared_storage_keys_to_watch: Rc<RefCell<HashSet<String>>>,
    extension_storage_service: Rc<dyn IExtensionStorageService>,
    instantiation_service: Rc<InstantiationService>,
    log_service: Rc<dyn LogService>,
}

impl MainThreadStorage {
    pub fn new(
        ext_host_context: &dyn IMainContext,
        extension_storage_service: Rc<dyn IExtensionStorageService>,
        storage_service: Rc<dyn StorageService>,
        instantiation_service: Rc<InstantiationService>,
        log_service: Rc<dyn LogService>,
    ) -> Rc<Self> {
        let proxy: Rc<dyn ExtHostStorageShape> = ext_host_context.get_proxy(&ExtHostContext::EXT_HOST_STORAGE);
        let storage_listener = DisposableStore::new();
        let shared_storage_keys_to_watch: Rc<RefCell<HashSet<String>>> = Rc::default();

        let on_did_change_value = storage_service.on_did_change_value(StorageScope::Profile, None, &storage_listener);
        {
            let (proxy, keys, service) = (proxy.clone(), shared_storage_keys_to_watch.clone(), extension_storage_service.clone());
            storage_listener.add(on_did_change_value.listen(move |e| {
                if keys.borrow().contains(&e.key) {
                    let raw_state = service.get_extension_state_raw(&ExtensionStorageTarget::from(e.key.as_str()), true);
                    if let Some(raw_state) = raw_state {
                        proxy.rpc_accept_value(true, &e.key, &raw_state);
                    }
                }
            }));
        }

        Rc::new(Self { proxy, storage_listener, shared_storage_keys_to_watch, extension_storage_service, instantiation_service, log_service })
    }

    /// `checkAndMigrateExtensionStorage(extensionId, shared)`.
    fn check_and_migrate_extension_storage(&self, extension_id: String, shared: bool) -> Promise<()> {
        let (service, instantiation_service, log_service) = (self.extension_storage_service.clone(), self.instantiation_service.clone(), self.log_service.clone());
        async_fn(async move {
            let has_state = |id: &str| service.get_extension_state(&ExtensionStorageTarget::from(id), shared).is_some_and(|state| !state.is_null());
            let is_web = Platform::detect().is_web;
            let mut source_extension_id = service.get_source_extension_to_migrate(&extension_id);

            // TODO: @sandy081 - Remove it after 6 months
            // If current extension does not have any migration requested
            // Then check if the extension has to be migrated for using lower case in web
            // If so, migrate the extension state from lower case id to its normal id.
            if source_extension_id.is_none() && is_web && extension_id != extension_id.to_lowercase() {
                source_extension_id = Some(extension_id.to_lowercase());
            }

            if let Some(mut source) = source_extension_id {
                // In Web, extension state was used to be stored in lower case extension id.
                // Hence check that if the lower cased source extension was not yet migrated in web
                // If not take the lower cased source extension id for migration
                if is_web && source != source.to_lowercase() && has_state(&source.to_lowercase()) && !has_state(&source) {
                    source = source.to_lowercase();
                }
                if let Err(error) = migrate_extension_storage(&source, &extension_id, shared, &instantiation_service).await {
                    log_service.error(&error.message);
                }
            }
            Ok(())
        })
    }
}

impl IDisposable for MainThreadStorage {
    fn dispose(&self) {
        self.storage_listener.dispose();
    }
}

impl MainThreadStorageShape for MainThreadStorage {
    fn rpc_initialize_extension_storage(&self, shared: bool, extension_id: &str) -> Promise<Option<String>> {
        let (service, keys, extension_id) = (self.extension_storage_service.clone(), self.shared_storage_keys_to_watch.clone(), extension_id.to_owned());
        self.check_and_migrate_extension_storage(extension_id.clone(), shared).then(move |()| {
            if shared {
                keys.borrow_mut().insert(extension_id.clone());
            }
            service.get_extension_state_raw(&ExtensionStorageTarget::from(extension_id.as_str()), shared)
        })
    }

    fn rpc_set_value(&self, shared: bool, extension_id: &str, value: Dynamic) -> Promise<()> {
        let state: Option<Value> = json_stringify(&value).and_then(|text| serde_json::from_str(&text).ok());
        self.extension_storage_service.set_extension_state(&ExtensionStorageTarget::from(extension_id), state.as_ref(), shared);
        Promise::resolved(())
    }

    fn rpc_register_extension_storage_keys_to_sync(&self, extension: IExtensionIdWithVersion, keys: Vec<String>) {
        self.extension_storage_service.set_keys_for_sync(&ExtensionIdWithVersion { id: extension.id, version: extension.version }, &keys);
    }
}
