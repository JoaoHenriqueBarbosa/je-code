//! Porte de `src/vs/platform/extensionManagement/common/extensionEnablementService.ts`: o
//! `GlobalExtensionEnablementService`, que guarda no armazenamento do perfil as extensões desabilitadas em todos os
//! workspaces, e o `StorageManager`, que lê e grava as listas de identificadores de extensão no armazenamento.
//!
//! Divergências:
//! - `extends Disposable` vira o campo `base` ([`Disposable`]); `this._store` do `StorageManager` (onde a inscrição em
//!   `onDidChangeValue` é registrada) é o campo `store`, descartado junto com a base.
//! - `JSON.parse(value)` de `StorageManager.get` com um texto que não é JSON (o original lançaria) devolve a lista
//!   vazia: o texto só é escrito por este módulo, e um valor corrompido não deve derrubar a leitura das extensões.
//! - `async enableExtension` e `async disableExtension` fazem todo o trabalho no mesmo trecho síncrono (como uma função
//!   `async` sem `await`) e devolvem a promessa já resolvida.
//! - `IExtensionManagementService` é o recorte de [`super::extension_management`].

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use je_base::event::{Emitter, Event};
use je_base::lifecycle::{Disposable, DisposableStore, IDisposable};
use je_base::promise::Promise;
use je_extensions::management::{DISABLED_EXTENSIONS_STORAGE_PATH, InstallOperation};
use je_extensions::manifest::ExtensionIdentifierInfo;
use je_extensions::util::are_same_extensions;
use je_platform::storage::{StorageScope, StorageService, StorageTarget, StorageValueChangeEvent};
use je_storage::StorageValue;

use super::extension_management::{DidChangeEnablementEvent, IExtensionManagementService, IGlobalExtensionEnablementService};

/// `GlobalExtensionEnablementService`.
pub struct GlobalExtensionEnablementService {
    base: Rc<Disposable>,
    on_did_change_enablement: Rc<Emitter<DidChangeEnablementEvent>>,
    storage_manager: Rc<StorageManager>,
}

impl GlobalExtensionEnablementService {
    pub fn new(storage_service: Rc<dyn StorageService>, extension_management_service: Rc<dyn IExtensionManagementService>) -> Rc<Self> {
        let base = Disposable::new();
        let on_did_change_enablement = Emitter::new();
        base.register(on_did_change_enablement.clone());
        let storage_manager = StorageManager::new(storage_service);
        base.register(storage_manager.clone());

        let this = Rc::new(Self { base, on_did_change_enablement, storage_manager });

        let emitter = this.on_did_change_enablement.clone();
        this.base.register(this.storage_manager.on_did_change().listen(move |extensions| {
            emitter.fire(DidChangeEnablementEvent { extensions: extensions.clone(), source: Some("storage".to_owned()) });
        }));

        let weak = Rc::downgrade(&this);
        this.base.register(extension_management_service.on_did_install_extensions().listen(move |results| {
            let Some(this) = weak.upgrade() else { return };
            for result in results {
                if let Some(local) = &result.local
                    && result.operation == InstallOperation::Migrate
                {
                    this.remove_from_disabled_extensions(&local.identifier); /* Reset migrated extensions */
                }
            }
        }));

        this
    }

    /// `getDisabledExtensions()`.
    pub fn get_disabled_extensions(&self) -> Vec<ExtensionIdentifierInfo> {
        self.get_extensions(DISABLED_EXTENSIONS_STORAGE_PATH)
    }

    fn add_to_disabled_extensions(&self, identifier: &ExtensionIdentifierInfo) -> bool {
        let mut disabled_extensions = self.get_disabled_extensions();
        if disabled_extensions.iter().all(|e| !are_same_extensions(e, identifier)) {
            disabled_extensions.push(identifier.clone());
            self.set_disabled_extensions(&disabled_extensions);
            return true;
        }
        false
    }

    fn remove_from_disabled_extensions(&self, identifier: &ExtensionIdentifierInfo) -> bool {
        let mut disabled_extensions = self.get_disabled_extensions();
        if let Some(index) = disabled_extensions.iter().position(|disabled_extension| are_same_extensions(disabled_extension, identifier)) {
            disabled_extensions.remove(index);
            self.set_disabled_extensions(&disabled_extensions);
            return true;
        }
        false
    }

    fn set_disabled_extensions(&self, disabled_extensions: &[ExtensionIdentifierInfo]) {
        self.set_extensions(DISABLED_EXTENSIONS_STORAGE_PATH, disabled_extensions);
    }

    fn get_extensions(&self, storage_id: &str) -> Vec<ExtensionIdentifierInfo> {
        self.storage_manager.get(storage_id, StorageScope::Profile)
    }

    fn set_extensions(&self, storage_id: &str, extensions: &[ExtensionIdentifierInfo]) {
        self.storage_manager.set(storage_id, extensions, StorageScope::Profile);
    }
}

impl IDisposable for GlobalExtensionEnablementService {
    fn dispose(&self) {
        self.base.dispose();
    }
}

impl IGlobalExtensionEnablementService for GlobalExtensionEnablementService {
    fn on_did_change_enablement(&self) -> Event<DidChangeEnablementEvent> {
        self.on_did_change_enablement.event()
    }

    fn get_disabled_extensions(&self) -> Vec<ExtensionIdentifierInfo> {
        GlobalExtensionEnablementService::get_disabled_extensions(self)
    }

    fn get_disabled_extensions_async(&self) -> Promise<Vec<ExtensionIdentifierInfo>> {
        Promise::resolved(GlobalExtensionEnablementService::get_disabled_extensions(self))
    }

    fn enable_extension(&self, extension: &ExtensionIdentifierInfo, source: Option<&str>) -> Promise<bool> {
        if self.remove_from_disabled_extensions(extension) {
            self.on_did_change_enablement.fire(DidChangeEnablementEvent { extensions: vec![extension.clone()], source: source.map(str::to_owned) });
            return Promise::resolved(true);
        }
        Promise::resolved(false)
    }

    fn disable_extension(&self, extension: &ExtensionIdentifierInfo, source: Option<&str>) -> Promise<bool> {
        if self.add_to_disabled_extensions(extension) {
            self.on_did_change_enablement.fire(DidChangeEnablementEvent { extensions: vec![extension.clone()], source: source.map(str::to_owned) });
            return Promise::resolved(true);
        }
        Promise::resolved(false)
    }
}

/// `StorageManager`: o cache (só do escopo do perfil) e a gravação das listas de identificadores de extensão.
pub struct StorageManager {
    base: Rc<Disposable>,
    /// `this._store`.
    store: Rc<DisposableStore>,
    storage_service: Rc<dyn StorageService>,
    /// `storage`: o texto de cada chave já lida no escopo do perfil.
    storage: RefCell<HashMap<String, String>>,
    on_did_change: Rc<Emitter<Vec<ExtensionIdentifierInfo>>>,
}

impl StorageManager {
    pub fn new(storage_service: Rc<dyn StorageService>) -> Rc<Self> {
        let base = Disposable::new();
        let store = DisposableStore::new();
        base.register(store.clone());
        let on_did_change = Emitter::new();
        base.register(on_did_change.clone());

        let this = Rc::new(Self { base, store, storage_service, storage: RefCell::new(HashMap::new()), on_did_change });

        let weak = Rc::downgrade(&this);
        let on_did_change_value = this.storage_service.on_did_change_value(StorageScope::Profile, None, &this.store);
        this.base.register(on_did_change_value.listen(move |e| {
            if let Some(this) = weak.upgrade() {
                this.on_did_storage_change(e);
            }
        }));

        this
    }

    /// `onDidChange`.
    pub fn on_did_change(&self) -> Event<Vec<ExtensionIdentifierInfo>> {
        self.on_did_change.event()
    }

    /// `get(key, scope)`.
    pub fn get(&self, key: &str, scope: StorageScope) -> Vec<ExtensionIdentifierInfo> {
        let value = if scope == StorageScope::Profile {
            let cached = self.storage.borrow().get(key).cloned();
            match cached {
                Some(value) => value,
                None => {
                    let value = self.read(key, scope);
                    self.storage.borrow_mut().insert(key.to_owned(), value.clone());
                    value
                }
            }
        } else {
            self.read(key, scope)
        };
        serde_json::from_str(&value).unwrap_or_default()
    }

    /// `set(key, value, scope)`.
    pub fn set(&self, key: &str, value: &[ExtensionIdentifierInfo], scope: StorageScope) {
        // `value.map(({ id, uuid }) => ({ id, uuid }))`: o `uuid` ausente some do texto, como o `undefined` no `JSON.stringify`.
        let new_value = serde_json::to_string(value).unwrap_or_else(|_| "[]".to_owned());
        let old_value = self.read(key, scope);
        if old_value != new_value {
            if scope == StorageScope::Profile {
                if value.is_empty() {
                    self.storage.borrow_mut().remove(key);
                } else {
                    self.storage.borrow_mut().insert(key.to_owned(), new_value.clone());
                }
            }
            self.write(key, if value.is_empty() { None } else { Some(new_value) }, scope);
        }
    }

    fn on_did_storage_change(&self, storage_change_event: &StorageValueChangeEvent) {
        let cached = self.storage.borrow().get(&storage_change_event.key).cloned();
        if let Some(cached) = cached {
            let new_value = self.read(&storage_change_event.key, storage_change_event.scope);
            if new_value != cached {
                let old_values = self.get(&storage_change_event.key, storage_change_event.scope);
                self.storage.borrow_mut().remove(&storage_change_event.key);
                let new_values = self.get(&storage_change_event.key, storage_change_event.scope);
                let added: Vec<ExtensionIdentifierInfo> = old_values.iter().filter(|old_value| !new_values.iter().any(|new_value| are_same_extensions(old_value, new_value))).cloned().collect();
                let removed: Vec<ExtensionIdentifierInfo> = new_values.iter().filter(|new_value| !old_values.iter().any(|old_value| are_same_extensions(old_value, new_value))).cloned().collect();
                if !added.is_empty() || !removed.is_empty() {
                    self.on_did_change.fire(added.into_iter().chain(removed).collect());
                }
            }
        }
    }

    /// `_get(key, scope)`.
    fn read(&self, key: &str, scope: StorageScope) -> String {
        self.storage_service.get(key, scope, Some("[]")).unwrap_or_else(|| "[]".to_owned())
    }

    /// `_set(key, value, scope)`.
    fn write(&self, key: &str, value: Option<String>, scope: StorageScope) {
        match value {
            // Enablement state is synced separately through extensions
            Some(value) if !value.is_empty() => self.storage_service.store(key, StorageValue::String(value), scope, StorageTarget::Machine),
            _ => self.storage_service.remove(key, scope),
        }
    }
}

impl IDisposable for StorageManager {
    fn dispose(&self) {
        self.base.dispose();
    }
}
