//! Porte de `src/vs/platform/extensionManagement/common/extensionStorage.ts`: o `IExtensionStorageService`, que guarda no
//! armazenamento o estado de cada extensão (por perfil ou por workspace), as chaves que a sincronização envia e a lista de
//! migração de identificadores.
//!
//! Divergências:
//! - `extends Disposable` vira o campo `base` ([`Disposable`]); a `this._store` onde a inscrição em `onDidChangeValue` é
//!   registrada é o campo `store`, descartado junto com a base.
//! - `IExtension | IGalleryExtension | string` é o [`ExtensionStorageTarget`]: o `getExtensionId` só lê o identificador
//!   (texto) ou o par `publisher` e `name`, e é só isso que o tipo guarda ([`ExtensionStorageTarget::Parts`], criado também
//!   pelo `From<&ExtensionManifest>`).
//! - `IStringDictionary<unknown>` (o estado) é o [`serde_json::Value`] do que o `JSON.parse` devolve; `object | undefined`
//!   de `setExtensionState` é `Option<&Value>`.
//! - `JSON.parse` do `getKeysForSync` e da lista de migração com um texto que não é uma lista de textos (o original
//!   lançaria no primeiro e devolveria a lista como estivesse no segundo) é lido como ausente e como lista vazia: o texto só
//!   é escrito por este módulo, e um valor corrompido não deve derrubar a leitura.
//! - `static removeOutdatedExtensionVersions` é [`ExtensionStorageService::remove_outdated_extension_versions`], que devolve a
//!   promessa de `getInstalled()` seguida das remoções.
//! - `productService.extensionSyncedKeys` é [`IProductService::extension_synced_keys`] (padrão: o produto não declara).

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use je_base::async_::async_fn;
use je_base::event::{Emitter, Event};
use je_base::lifecycle::{Disposable, DisposableStore, IDisposable};
use je_base::promise::Promise;
use je_extensions::manifest::{ExtensionIdentifierInfo, ExtensionManifest};
use je_extensions::util::{adopt_to_gallery_extension_id, are_same_extensions, get_extension_id};
use je_instantiation::{Injectable, service_id, service_impl};
use je_platform::log::{LogService, Logger};
use je_platform::product::IProductService;
use je_platform::storage::{StorageScope, StorageService, StorageTarget, StorageValue, StorageValueChangeEvent};
use serde_json::Value;

use super::extension_management::IExtensionManagementService;

/// `IExtensionIdWithVersion`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExtensionIdWithVersion {
    pub id: String,
    pub version: String,
}

/// `IExtension | IGalleryExtension | string`, reduzido ao que o `getExtensionId` lê (ver o cabeçalho).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ExtensionStorageTarget {
    /// O identificador (`publisher.name`).
    Id(String),
    /// `publisher` e `name` do manifesto da extensão.
    Parts { publisher: String, name: String },
}

impl From<&str> for ExtensionStorageTarget {
    fn from(id: &str) -> Self {
        Self::Id(id.to_owned())
    }
}

impl From<&ExtensionManifest> for ExtensionStorageTarget {
    fn from(manifest: &ExtensionManifest) -> Self {
        Self::Parts { publisher: manifest.publisher.clone(), name: manifest.name.clone() }
    }
}

impl ExtensionStorageTarget {
    /// `getExtensionId(extension)`.
    fn extension_id(&self) -> String {
        match self {
            Self::Id(id) => id.clone(),
            Self::Parts { publisher, name } => get_extension_id(publisher, name),
        }
    }
}

/// `IExtensionStorageService`.
pub trait IExtensionStorageService {
    fn get_extension_state(&self, extension: &ExtensionStorageTarget, global: bool) -> Option<Value>;
    fn get_extension_state_raw(&self, extension: &ExtensionStorageTarget, global: bool) -> Option<String>;
    fn set_extension_state(&self, extension: &ExtensionStorageTarget, state: Option<&Value>, global: bool);

    fn on_did_change_extension_storage_to_sync(&self) -> Event<()>;
    fn set_keys_for_sync(&self, extension_id_with_version: &ExtensionIdWithVersion, keys: &[String]);
    fn get_keys_for_sync(&self, extension_id_with_version: &ExtensionIdWithVersion) -> Option<Vec<String>>;

    fn add_to_migration_list(&self, from: &str, to: &str);
    fn get_source_extension_to_migrate(&self, target: &str) -> Option<String>;
}

service_id!(pub I_EXTENSION_STORAGE_SERVICE: dyn IExtensionStorageService = "IExtensionStorageService");

/// `ExtensionStorageService.LARGE_STATE_WARNING_THRESHOLD`.
const LARGE_STATE_WARNING_THRESHOLD: usize = 512 * 1024;

/// `'extensionStorage.migrationList'`.
const MIGRATION_LIST_KEY: &str = "extensionStorage.migrationList";

/// `ExtensionStorageService.toKey(extension)`.
fn to_key(extension: &ExtensionIdWithVersion) -> String {
    format!("extensionKeys/{}@{}", adopt_to_gallery_extension_id(&extension.id), extension.version)
}

/// `[^.]+\..+`: o identificador tem um ponto depois de ao menos um caractere que não seja ponto, e algo depois dele.
fn is_id_with_publisher(id: &str) -> bool {
    id.find('.').is_some_and(|dot| dot > 0 && dot + 1 < id.len())
}

/// `\d+\.\d+\.\d+(-.*)?`.
fn is_version(version: &str) -> bool {
    let numbers = version.split_once('-').map_or(version, |(numbers, _)| numbers);
    let parts: Vec<&str> = numbers.split('.').collect();
    parts.len() == 3 && parts.iter().all(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()))
}

/// `ExtensionStorageService.fromKey(key)`: `^extensionKeys\/([^.]+\..+)@(\d+\.\d+\.\d+(-.*)?)$`. O `.+` guloso do
/// identificador recua até o último `@` que deixa o resto casar com a versão.
fn from_key(key: &str) -> Option<ExtensionIdWithVersion> {
    let rest = key.strip_prefix("extensionKeys/")?;
    rest.match_indices('@').rev().find_map(|(at, _)| {
        let (id, version) = (&rest[..at], &rest[at + 1..]);
        (is_id_with_publisher(id) && is_version(version)).then(|| ExtensionIdWithVersion { id: id.to_owned(), version: version.to_owned() })
    })
}

/// `ExtensionStorageService.readAllExtensionsWithKeysForSync(storageService)`.
fn read_all_extensions_with_keys_for_sync(storage_service: &dyn StorageService) -> HashMap<String, Vec<String>> {
    let mut extensions_with_keys_for_sync: HashMap<String, Vec<String>> = HashMap::new();
    for key in storage_service.keys(StorageScope::Profile, StorageTarget::Machine) {
        if let Some(extension_id_with_version) = from_key(&key) {
            extensions_with_keys_for_sync.entry(extension_id_with_version.id.to_lowercase()).or_default().push(extension_id_with_version.version);
        }
    }
    extensions_with_keys_for_sync
}

/// `ExtensionStorageService`.
pub struct ExtensionStorageService {
    base: Rc<Disposable>,
    /// `this._store`.
    store: Rc<DisposableStore>,
    storage_service: Rc<dyn StorageService>,
    product_service: Rc<dyn IProductService>,
    log_service: Rc<dyn LogService>,
    on_did_change_extension_storage_to_sync: Rc<Emitter<()>>,
    extensions_with_keys_for_sync: RefCell<HashMap<String, Vec<String>>>,
}

impl Injectable for ExtensionStorageService {
    type Args = ();
    type Deps = (Rc<dyn StorageService>, Rc<dyn IProductService>, Rc<dyn LogService>);

    fn create(_: (), (storage_service, product_service, log_service): Self::Deps) -> Rc<Self> {
        let base = Disposable::new();
        let store = DisposableStore::new();
        base.register(store.clone());
        let on_did_change_extension_storage_to_sync = Emitter::new();
        base.register(on_did_change_extension_storage_to_sync.clone());

        let extensions_with_keys_for_sync = read_all_extensions_with_keys_for_sync(storage_service.as_ref());
        let this = Rc::new(Self {
            base,
            store,
            storage_service,
            product_service,
            log_service,
            on_did_change_extension_storage_to_sync,
            extensions_with_keys_for_sync: RefCell::new(extensions_with_keys_for_sync),
        });

        let weak = Rc::downgrade(&this);
        let on_did_change_value = this.storage_service.on_did_change_value(StorageScope::Profile, None, &this.store);
        this.base.register(on_did_change_value.listen(move |e| {
            if let Some(this) = weak.upgrade() {
                this.on_did_change_storage_value(e);
            }
        }));

        this
    }
}

service_impl!(ExtensionStorageService => dyn IExtensionStorageService, disposable);

impl ExtensionStorageService {
    /// Nota do original: isto ainda tem de ser feito em todos os perfis.
    /// `static removeOutdatedExtensionVersions(extensionManagementService, storageService)`.
    pub fn remove_outdated_extension_versions(extension_management_service: Rc<dyn IExtensionManagementService>, storage_service: Rc<dyn StorageService>) -> Promise<()> {
        async_fn(async move {
            let extensions = extension_management_service.get_installed().await?;
            let mut extension_versions_to_remove: Vec<String> = Vec::new();
            for (id, versions) in read_all_extensions_with_keys_for_sync(storage_service.as_ref()) {
                let identifier = ExtensionIdentifierInfo::new(id.clone());
                let extension_version = extensions.iter().find(|extension| are_same_extensions(&extension.identifier, &identifier)).map(|extension| extension.manifest.version.clone());
                for version in versions {
                    if extension_version.as_deref() != Some(version.as_str()) {
                        extension_versions_to_remove.push(to_key(&ExtensionIdWithVersion { id: id.clone(), version }));
                    }
                }
            }
            for key in extension_versions_to_remove {
                storage_service.remove(&key, StorageScope::Profile);
            }
            Ok(())
        })
    }

    /// `onDidChangeStorageValue(e)`.
    fn on_did_change_storage_value(&self, e: &StorageValueChangeEvent) {
        // State of extension with keys for sync has changed
        if self.extensions_with_keys_for_sync.borrow().contains_key(&e.key.to_lowercase()) {
            self.on_did_change_extension_storage_to_sync.fire(());
            return;
        }

        // Keys for sync of an extension has changed
        if let Some(extension_id_with_version) = from_key(&e.key) {
            let id = extension_id_with_version.id.to_lowercase();
            if self.storage_service.get(&e.key, StorageScope::Profile, None).is_none() {
                self.extensions_with_keys_for_sync.borrow_mut().remove(&id);
            } else {
                self.extensions_with_keys_for_sync.borrow_mut().entry(id).or_default().push(extension_id_with_version.version);
                self.on_did_change_extension_storage_to_sync.fire(());
            }
        }
    }

    fn storage_scope(global: bool) -> StorageScope {
        if global { StorageScope::Profile } else { StorageScope::Workspace }
    }

    /// `get migrationList()`.
    fn migration_list(&self) -> Vec<(String, String)> {
        let value = self.storage_service.get(MIGRATION_LIST_KEY, StorageScope::Application, Some("[]")).unwrap_or_else(|| "[]".to_owned());
        serde_json::from_str(&value).unwrap_or_default()
    }

    /// `set migrationList(migrationList)`.
    fn set_migration_list(&self, migration_list: &[(String, String)]) {
        if migration_list.is_empty() {
            self.storage_service.remove(MIGRATION_LIST_KEY, StorageScope::Application);
        } else {
            let value = serde_json::to_string(migration_list).unwrap_or_else(|_| "[]".to_owned());
            self.storage_service.store(MIGRATION_LIST_KEY, StorageValue::String(value), StorageScope::Application, StorageTarget::Machine);
        }
    }
}

impl IDisposable for ExtensionStorageService {
    fn dispose(&self) {
        self.base.dispose();
    }
}

impl IExtensionStorageService for ExtensionStorageService {
    fn get_extension_state(&self, extension: &ExtensionStorageTarget, global: bool) -> Option<Value> {
        let extension_id = extension.extension_id();
        let json_value = self.get_extension_state_raw(extension, global)?;
        if json_value.is_empty() {
            return None;
        }
        match serde_json::from_str(&json_value) {
            Ok(state) => Some(state),
            Err(error) => {
                // Do not fail this call but log it for diagnostics
                // https://github.com/microsoft/vscode/issues/132777
                self.log_service.error(&format!("[mainThreadStorage] unexpected error parsing storage contents (extensionId: {extension_id}, global: {global}): {error}"));
                None
            }
        }
    }

    fn get_extension_state_raw(&self, extension: &ExtensionStorageTarget, global: bool) -> Option<String> {
        let extension_id = extension.extension_id();
        let raw_state = self.storage_service.get(&extension_id, Self::storage_scope(global), None);

        if let Some(raw_state) = raw_state.as_deref() {
            let length = raw_state.encode_utf16().count();
            if length > LARGE_STATE_WARNING_THRESHOLD {
                self.log_service.warn(&format!(
                    "[mainThreadStorage] large extension state detected (extensionId: {extension_id}, global: {global}): {}kb. Consider to use 'storageUri' or 'globalStorageUri' to store this data on disk instead.",
                    length as f64 / 1024.0
                ));
            }
        }

        raw_state
    }

    fn set_extension_state(&self, extension: &ExtensionStorageTarget, state: Option<&Value>, global: bool) {
        let extension_id = extension.extension_id();
        let scope = Self::storage_scope(global);
        match state {
            None => self.storage_service.remove(&extension_id, scope),
            Some(state) => {
                // Extension state is synced separately through extensions
                self.storage_service.store(&extension_id, StorageValue::String(state.to_string()), scope, StorageTarget::Machine);
            }
        }
    }

    fn on_did_change_extension_storage_to_sync(&self) -> Event<()> {
        self.on_did_change_extension_storage_to_sync.event()
    }

    fn set_keys_for_sync(&self, extension_id_with_version: &ExtensionIdWithVersion, keys: &[String]) {
        let value = serde_json::to_string(keys).unwrap_or_else(|_| "[]".to_owned());
        self.storage_service.store(&to_key(extension_id_with_version), StorageValue::String(value), StorageScope::Profile, StorageTarget::Machine);
    }

    fn get_keys_for_sync(&self, extension_id_with_version: &ExtensionIdWithVersion) -> Option<Vec<String>> {
        let extension_keys_for_sync_from_product = self
            .product_service
            .extension_synced_keys()
            .and_then(|synced_keys| synced_keys.get(&extension_id_with_version.id.to_lowercase()))
            .cloned();
        let extension_keys_for_sync_from_storage_value = self.storage_service.get(&to_key(extension_id_with_version), StorageScope::Profile, None);
        let extension_keys_for_sync_from_storage: Option<Vec<String>> =
            extension_keys_for_sync_from_storage_value.filter(|value| !value.is_empty()).and_then(|value| serde_json::from_str(&value).ok());

        match (extension_keys_for_sync_from_storage, extension_keys_for_sync_from_product) {
            // `distinct([...fromStorage, ...fromProduct])`
            (Some(from_storage), Some(from_product)) => {
                let mut distinct: Vec<String> = Vec::new();
                for key in from_storage.into_iter().chain(from_product) {
                    if !distinct.contains(&key) {
                        distinct.push(key);
                    }
                }
                Some(distinct)
            }
            (from_storage, from_product) => from_storage.or(from_product),
        }
    }

    fn add_to_migration_list(&self, from: &str, to: &str) {
        if from != to {
            // remove the duplicates
            let mut migration_list: Vec<(String, String)> =
                self.migration_list().into_iter().filter(|(entry_from, entry_to)| !(entry_from == from || entry_to == from) && !(entry_from == to || entry_to == to)).collect();
            migration_list.push((from.to_owned(), to.to_owned()));
            self.set_migration_list(&migration_list);
        }
    }

    fn get_source_extension_to_migrate(&self, to_extension_id: &str) -> Option<String> {
        self.migration_list().into_iter().find(|(_, to)| to_extension_id == to).map(|(from, _)| from)
    }
}
