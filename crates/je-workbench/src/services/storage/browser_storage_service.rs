//! Porte de `src/vs/workbench/services/storage/browser/storageService.ts`: o `BrowserStorageService` e o
//! `IndexedDBStorageDatabase` (o banco de itens do `Storage` sobre o IndexedDB do navegador).
//!
//! Notas de tradução:
//! - **O navegador.** O IndexedDB (`base/browser/indexedDB.ts`), o `BroadcastChannel` (`base/browser/broadcast.ts`), o
//!   `isSafari` e o `getActiveWindow().document.hasFocus()` são o que o ambiente hospedeiro fornece, e entram pelo trait
//!   [`BrowserStorageHost`]. O `IndexedDB` do original só é usado com a tabela `ItemTable` e com os valores de texto, e o
//!   `runInTransaction` genérico (que recebe um fecho sobre o `IDBObjectStore`) é aqui o conjunto de operações que o banco de
//!   itens executa nessa tabela: [`IndexedDb::get_key_values`], [`IndexedDb::get_value`], [`IndexedDb::update_items`] (uma
//!   transação de escrita com as inserções e as remoções), [`IndexedDb::clear`] e [`IndexedDb::compare_and_swap`] (a transação
//!   atômica de leitura e escrita). `IndexedDB.create(name, version, stores)` é [`BrowserStorageHost::create_indexed_db`].
//!   Os valores que não são texto, que o `isValid` do original descarta, não existem no tipo.
//! - **`DBClosedError`** é [`db_closed_error`], o erro que um [`IndexedDb`] fechado deve devolver; o campo `code` do original
//!   é o `name` do erro.
//! - **`AbstractStorageService`** é a struct do `je-platform` com um [`StorageBackend`]: `doInitialize`, `getStorage`,
//!   `getLogDetails`, `switchToProfile`, `switchToWorkspace`, `shouldFlushWhenIdle` e `hasScope` são o backend, e as duas
//!   sobrescritas de `readApplicationSharedValue` e `compareAndSwapApplicationSharedValue` são os ganchos do backend
//!   (`StorageBackend::read_application_shared_value` e `compare_and_swap_application_shared_value`). Os métodos próprios do
//!   `BrowserStorageService` (`hasPendingUpdate`, `getApplicationStorageValue`, `compareAndSwapApplicationStorage`, `close`,
//!   `clear`, `hasScope`) estão na struct [`BrowserStorageService`], e o serviço de armazenamento (`IStorageService`) é o
//!   [`BrowserStorageService::service`].
//! - **`IUserDataProfile` e `IAnyWorkspaceIdentifier`**: o armazenamento só lê `id`, `isDefault` e
//!   `useDefaultFlags.globalState` do perfil e o `id` do workspace, que são o [`StorageProfile`] e o [`StorageWorkspace`] do
//!   `je-platform` ([`storage_profile_of`] converte o perfil de dados do usuário). `isUserDataProfile(scope)` é o próprio
//!   [`StorageSwitchTarget`].
//! - **`Promises.settled`** é `promises::settled` (espera todas e relança o primeiro erro). Os `async` do original são
//!   [`async_fn`], que roda o corpo já até o primeiro `await`.
//! - **`IndexedDBStorageDatabase.create`** descarta o banco que falhou ao conectar antes de cair no banco em memória (o original
//!   o deixa vivo, com o canal de broadcast aberto).

use std::cell::RefCell;
use std::rc::{Rc, Weak};

use je_base::async_::{async_fn, promises};
use je_base::errors::Error;
use je_base::event::{Emitter, Event};
use je_base::lifecycle::{Disposable, DisposableRef, DisposableStore, IDisposable};
use je_base::map::OrderedMap;
use je_base::promise::{DeferredPromise, Promise};
use je_platform::log::{LogServiceRef, Logger};
use je_platform::storage::{
    AbstractStorageService, CompareAndSwapResult, StorageBackend, StorageProfile, StorageScope, StorageService, StorageServiceCoreOptions, StorageSwitchTarget,
    StorageTarget, StorageWorkspace, WillSaveStateReason, IS_NEW_KEY, is_profile_using_default_storage,
};
use je_platform::user_data_profile::user_data_profile::UserDataProfile;
use je_storage::{
    is_storage_items_change_event, IStorage, ItemMap, Recovery, Storage, StorageDatabase, StorageItemsChangeEvent, StorageOptions, StorageValue, UpdateRequest,
};

use crate::services::user_data_profile::user_data_profile::IUserDataProfileService;

const STORAGE_DATABASE_PREFIX: &str = "vscode-web-state-db-";
const STORAGE_OBJECT_STORE: &str = "ItemTable";

/// `BROWSER_DEFAULT_FLUSH_INTERVAL`: a cada 5s, porque operações assíncronas não são permitidas no desligamento.
const BROWSER_DEFAULT_FLUSH_INTERVAL: u64 = 5 * 1000;

//#region O navegador

/// `new DBClosedError(dbName)`.
pub fn db_closed_error(db_name: &str) -> Error {
    Error::named("DBClosed", &format!("IndexedDB database '{db_name}' is closed."))
}

/// `IndexedDB`, restrito às operações que o banco de itens faz na tabela `ItemTable`. Depois de `close`, as operações rejeitam
/// com [`db_closed_error`] (e `close` aborta as transações pendentes).
pub trait IndexedDb {
    fn close(&self);

    /// `getKeyValues(store, isValid)`: todos os pares chave e valor de texto.
    fn get_key_values(&self, store: &str) -> Promise<OrderedMap<String, String>>;

    /// `runInTransaction(store, 'readonly', objectStore => objectStore.get(key))`, com o valor que não é texto como `None`.
    fn get_value(&self, store: &str, key: &str) -> Promise<Option<String>>;

    /// `runInTransaction(store, 'readwrite', ...)` com os `put` das inserções e os `delete` das remoções.
    fn update_items(&self, store: &str, inserts: Vec<(String, String)>, deletes: Vec<String>) -> Promise<()>;

    /// `runInTransaction(store, 'readwrite', objectStore => objectStore.clear())`.
    fn clear(&self, store: &str) -> Promise<()>;

    /// `compareAndSwap(store, key, expectedValue, newValue, isValid)`: grava `new_value` quando o valor atual é igual ao
    /// esperado, tudo na mesma transação.
    fn compare_and_swap(&self, store: &str, key: &str, expected_value: Option<&str>, new_value: &str) -> Promise<CompareAndSwapResult>;
}

/// `BroadcastDataChannel<IStorageItemsChangeEvent>`.
pub trait StorageBroadcastChannel: IDisposable {
    fn on_did_receive_data(&self) -> Event<StorageItemsChangeEvent>;

    /// Envia os dados para os outros canais criados com o mesmo nome.
    fn post_data(&self, data: StorageItemsChangeEvent);
}

/// O que o armazenamento do navegador pede ao ambiente hospedeiro (ver as notas do módulo).
pub trait BrowserStorageHost {
    /// `IndexedDB.create(name, version, stores)`.
    fn create_indexed_db(&self, name: &str, version: Option<u32>, stores: &[&str]) -> Promise<Rc<dyn IndexedDb>>;

    /// `new BroadcastDataChannel<IStorageItemsChangeEvent>(name)`.
    fn create_broadcast_channel(&self, name: &str) -> Rc<dyn StorageBroadcastChannel>;

    /// `isSafari`.
    fn is_safari(&self) -> bool;

    /// `getActiveWindow().document.hasFocus()`.
    fn has_focus(&self) -> bool;
}

//#endregion

/// `IIndexedDBStorageDatabase`.
pub trait IIndexedDbStorageDatabase: StorageDatabase + IDisposable {
    /// Name of the database.
    fn name(&self) -> String;

    /// Whether an update in the DB is currently pending (either update or delete operation).
    fn has_pending_update(&self) -> bool;

    fn get_value(&self, key: &str) -> Promise<Option<String>>;

    fn compare_and_swap(&self, key: &str, expected_value: Option<&str>, new_value: &str) -> Promise<CompareAndSwapResult>;

    /// For testing only.
    fn clear(&self) -> Promise<()>;
}

/// `InMemoryIndexedDBStorageDatabase`: o banco em memória em que o `create` cai quando o IndexedDB não conecta.
struct InMemoryIndexedDbStorageDatabase {
    inner: Rc<dyn StorageDatabase>,
}

impl InMemoryIndexedDbStorageDatabase {
    fn new() -> Rc<Self> {
        Rc::new(Self { inner: je_storage::InMemoryStorageDatabase::new() })
    }
}

impl StorageDatabase for InMemoryIndexedDbStorageDatabase {
    fn on_did_change_items_external(&self) -> Event<StorageItemsChangeEvent> {
        self.inner.on_did_change_items_external()
    }

    fn get_items(&self) -> Promise<ItemMap> {
        self.inner.get_items()
    }

    fn update_items(&self, request: UpdateRequest) -> Promise<()> {
        self.inner.update_items(request)
    }

    fn optimize(&self) -> Promise<()> {
        self.inner.optimize()
    }

    fn close(&self, recovery: Option<Recovery>) -> Promise<()> {
        self.inner.close(recovery)
    }
}

impl IDisposable for InMemoryIndexedDbStorageDatabase {
    fn dispose(&self) {
        // No-op
    }
}

impl IIndexedDbStorageDatabase for InMemoryIndexedDbStorageDatabase {
    fn name(&self) -> String {
        "in-memory-indexedb-storage".to_string()
    }

    fn has_pending_update(&self) -> bool {
        false
    }

    fn get_value(&self, key: &str) -> Promise<Option<String>> {
        let (inner, key) = (self.inner.clone(), key.to_string());
        async_fn(async move { Ok(inner.get_items().await?.get(&key)) })
    }

    fn compare_and_swap(&self, key: &str, expected_value: Option<&str>, new_value: &str) -> Promise<CompareAndSwapResult> {
        let (inner, key, expected_value, new_value) = (self.inner.clone(), key.to_string(), expected_value.map(str::to_string), new_value.to_string());
        async_fn(async move {
            let items = inner.get_items().await?;
            let current_value = items.get(&key);
            if current_value != expected_value {
                return Ok(CompareAndSwapResult { swapped: false, current_value });
            }

            let mut insert = OrderedMap::new();
            insert.insert(key, new_value.clone());
            inner.update_items(UpdateRequest { insert: Some(insert), delete: None }).await?;
            Ok(CompareAndSwapResult { swapped: true, current_value: Some(new_value) })
        })
    }

    fn clear(&self) -> Promise<()> {
        let inner = self.inner.clone();
        async_fn(async move {
            let items = inner.get_items().await?;
            for (key, _) in items.snapshot().iter() {
                items.delete(key);
            }
            Ok(())
        })
    }
}

/// `IndexedDBStorageDatabaseOptions`.
#[derive(Clone, Debug, Default)]
pub struct IndexedDbStorageDatabaseOptions {
    pub id: String,
    pub broadcast_changes: bool,
}

/// `connect()`: abre o banco e registra o erro de conexão antes de relançá-lo.
fn connect(host: &Rc<dyn BrowserStorageHost>, log_service: &LogServiceRef, name: &str) -> Promise<Rc<dyn IndexedDb>> {
    let (host, log_service, name) = (host.clone(), log_service.clone(), name.to_string());
    async_fn(async move {
        match host.create_indexed_db(&name, None, &[STORAGE_OBJECT_STORE]).await {
            Ok(database) => Ok(database),
            Err(error) => {
                log_service.error(&format!("[IndexedDB Storage {name}] connect() error: {error}"));

                Err(error)
            }
        }
    })
}

/// `IndexedDBStorageDatabase`.
pub struct IndexedDbStorageDatabase {
    me: Weak<IndexedDbStorageDatabase>,
    base: Rc<Disposable>,
    on_did_change_items_external: Rc<Emitter<StorageItemsChangeEvent>>,
    broadcast_channel: Option<Rc<dyn StorageBroadcastChannel>>,
    pending_update: RefCell<Option<Promise<bool>>>,
    name: String,
    when_connected: Promise<Rc<dyn IndexedDb>>,
}

impl IndexedDbStorageDatabase {
    pub fn create_application_storage(log_service: LogServiceRef, host: Rc<dyn BrowserStorageHost>) -> Promise<Rc<dyn IIndexedDbStorageDatabase>> {
        Self::create(IndexedDbStorageDatabaseOptions { id: "global".to_string(), broadcast_changes: true }, log_service, host)
    }

    pub fn create_application_shared_storage(log_service: LogServiceRef, host: Rc<dyn BrowserStorageHost>) -> Promise<Rc<dyn IIndexedDbStorageDatabase>> {
        Self::create(IndexedDbStorageDatabaseOptions { id: "global-shared".to_string(), broadcast_changes: true }, log_service, host)
    }

    pub fn create_profile_storage(profile_id: &str, log_service: LogServiceRef, host: Rc<dyn BrowserStorageHost>) -> Promise<Rc<dyn IIndexedDbStorageDatabase>> {
        Self::create(IndexedDbStorageDatabaseOptions { id: format!("global-{profile_id}"), broadcast_changes: true }, log_service, host)
    }

    pub fn create_workspace_storage(workspace_id: &str, log_service: LogServiceRef, host: Rc<dyn BrowserStorageHost>) -> Promise<Rc<dyn IIndexedDbStorageDatabase>> {
        Self::create(IndexedDbStorageDatabaseOptions { id: workspace_id.to_string(), broadcast_changes: false }, log_service, host)
    }

    pub fn create(options: IndexedDbStorageDatabaseOptions, log_service: LogServiceRef, host: Rc<dyn BrowserStorageHost>) -> Promise<Rc<dyn IIndexedDbStorageDatabase>> {
        async_fn(async move {
            let database = Self::new(&options, log_service.clone(), host);
            match database.when_connected.clone().await {
                Ok(_) => {
                    let database: Rc<dyn IIndexedDbStorageDatabase> = database;
                    Ok(database)
                }
                Err(error) => {
                    log_service.error(&format!("[IndexedDB Storage {}] create(): {error}", options.id));
                    database.dispose();

                    let fallback: Rc<dyn IIndexedDbStorageDatabase> = InMemoryIndexedDbStorageDatabase::new();
                    Ok(fallback)
                }
            }
        })
    }

    fn new(options: &IndexedDbStorageDatabaseOptions, log_service: LogServiceRef, host: Rc<dyn BrowserStorageHost>) -> Rc<Self> {
        let name = format!("{STORAGE_DATABASE_PREFIX}{}", options.id);
        let base = Disposable::new();
        let on_did_change_items_external = Emitter::<StorageItemsChangeEvent>::new();
        base.register(on_did_change_items_external.clone());
        let broadcast_channel = options.broadcast_changes.then(|| {
            let channel = host.create_broadcast_channel(&name);
            base.register(channel.clone());
            channel
        });

        let when_connected = connect(&host, &log_service, &name);

        let this = Rc::new_cyclic(|me| Self { me: me.clone(), base, on_did_change_items_external, broadcast_channel, pending_update: RefCell::new(None), name, when_connected });

        this.register_listeners();

        this
    }

    fn this(&self) -> Rc<Self> {
        self.me.upgrade().expect("o banco está vivo enquanto um método dele roda")
    }

    fn register_listeners(&self) {
        // Check for storage change events from other windows/tabs via `BroadcastChannel` mechanisms.
        if let Some(channel) = &self.broadcast_channel {
            let emitter = self.on_did_change_items_external.clone();
            self.base.register(channel.on_did_receive_data().listen(move |data| {
                let data: &StorageItemsChangeEvent = &data;
                if is_storage_items_change_event(Some(data)) {
                    emitter.fire(data.clone());
                }
            }));
        }
    }

    /// `doUpdateItems`.
    fn do_update_items(&self, request: &UpdateRequest) -> Promise<bool> {
        // Return early if the request is empty
        let insert_size = request.insert.as_ref().map(OrderedMap::len);
        let delete_size = request.delete.as_ref().map(OrderedMap::len);
        if (request.insert.is_none() && request.delete.is_none()) || (insert_size == Some(0) && delete_size == Some(0)) {
            return Promise::resolved(false);
        }

        let connected = self.when_connected.clone();
        let inserts: Vec<(String, String)> = request.insert.as_ref().map(|insert| insert.iter().map(|(key, value)| (key.clone(), value.clone())).collect()).unwrap_or_default();
        let deletes: Vec<String> = request.delete.as_ref().map(|delete| delete.keys().cloned().collect()).unwrap_or_default();
        async_fn(async move {
            let db = connected.await?;

            // Update `ItemTable` with inserts and/or deletes
            db.update_items(STORAGE_OBJECT_STORE, inserts, deletes).await?;

            Ok(true)
        })
    }
}

impl StorageDatabase for IndexedDbStorageDatabase {
    fn on_did_change_items_external(&self) -> Event<StorageItemsChangeEvent> {
        self.on_did_change_items_external.event()
    }

    fn get_items(&self) -> Promise<ItemMap> {
        let connected = self.when_connected.clone();
        async_fn(async move {
            let db = connected.await?;

            let items = db.get_key_values(STORAGE_OBJECT_STORE).await?;
            Ok(ItemMap::from_map(items))
        })
    }

    fn update_items(&self, request: UpdateRequest) -> Promise<()> {
        // Run the update
        let this = self.this();
        let pending = self.do_update_items(&request);
        *self.pending_update.borrow_mut() = Some(pending.clone());
        async_fn(async move {
            let result = pending.await;
            *this.pending_update.borrow_mut() = None;
            let did_update = result?;

            // Broadcast changes to other windows/tabs if enabled and only if we actually did update storage items.
            if let (Some(channel), true) = (&this.broadcast_channel, did_update) {
                channel.post_data(StorageItemsChangeEvent { changed: request.insert, deleted: request.delete });
            }
            Ok(())
        })
    }

    fn optimize(&self) -> Promise<()> {
        // not suported in IndexedDB
        Promise::resolved(())
    }

    fn close(&self, _recovery: Option<Recovery>) -> Promise<()> {
        let (this, connected) = (self.this(), self.when_connected.clone());
        async_fn(async move {
            let db = connected.await?;

            // Wait for pending updates to having finished
            let pending = this.pending_update.borrow().clone();
            if let Some(pending) = pending {
                pending.await?;
            }

            // Finally, close IndexedDB
            db.close();
            Ok(())
        })
    }
}

impl IDisposable for IndexedDbStorageDatabase {
    fn dispose(&self) {
        self.base.dispose();
    }
}

impl IIndexedDbStorageDatabase for IndexedDbStorageDatabase {
    fn name(&self) -> String {
        self.name.clone()
    }

    fn has_pending_update(&self) -> bool {
        self.pending_update.borrow().is_some()
    }

    fn get_value(&self, key: &str) -> Promise<Option<String>> {
        let (connected, key) = (self.when_connected.clone(), key.to_string());
        async_fn(async move {
            let db = connected.await?;
            Ok(db.get_value(STORAGE_OBJECT_STORE, &key).await?)
        })
    }

    fn compare_and_swap(&self, key: &str, expected_value: Option<&str>, new_value: &str) -> Promise<CompareAndSwapResult> {
        let (this, connected) = (self.this(), self.when_connected.clone());
        let (key, expected_value, new_value) = (key.to_string(), expected_value.map(str::to_string), new_value.to_string());
        async_fn(async move {
            let db = connected.await?;
            let result = db.compare_and_swap(STORAGE_OBJECT_STORE, &key, expected_value.as_deref(), &new_value).await?;
            if result.swapped {
                let mut changed = OrderedMap::new();
                changed.insert(key, new_value);
                let event = StorageItemsChangeEvent { changed: Some(changed), deleted: None };
                this.on_did_change_items_external.fire(event.clone());
                if let Some(channel) = &this.broadcast_channel {
                    channel.post_data(event);
                }
            }
            Ok(result)
        })
    }

    fn clear(&self) -> Promise<()> {
        let connected = self.when_connected.clone();
        async_fn(async move {
            let db = connected.await?;

            db.clear(STORAGE_OBJECT_STORE).await?;
            Ok(())
        })
    }
}

/// O que o armazenamento lê do `IUserDataProfile` (ver as notas do módulo).
pub fn storage_profile_of(profile: &UserDataProfile) -> StorageProfile {
    StorageProfile {
        id: profile.id.clone(),
        is_default: profile.is_default,
        use_default_global_state: profile.use_default_flags.is_some_and(|flags| flags.global_state == Some(true)),
        global_storage_home: profile.global_storage_home.clone(),
    }
}

type ApplicationStorage = (Rc<dyn IIndexedDbStorageDatabase>, Rc<Storage>);

#[derive(Default)]
struct State {
    application_storage: Option<Rc<Storage>>,
    application_storage_database: Option<Rc<dyn IIndexedDbStorageDatabase>>,
    application_shared_storage: Option<Rc<Storage>>,
    application_shared_storage_database: Option<Rc<dyn IIndexedDbStorageDatabase>>,
    profile_storage: Option<Rc<Storage>>,
    profile_storage_database: Option<Rc<dyn IIndexedDbStorageDatabase>>,
    workspace_storage: Option<Rc<Storage>>,
    workspace_storage_database: Option<Rc<dyn IIndexedDbStorageDatabase>>,
}

/// Os métodos abstratos do `AbstractStorageService` que o `BrowserStorageService` implementa, mais o estado dos bancos.
struct BrowserBackend {
    me: Weak<BrowserBackend>,
    workspace: StorageWorkspace,
    log_service: LogServiceRef,
    host: Rc<dyn BrowserStorageHost>,
    state: RefCell<State>,
    profile_storage_profile: RefCell<StorageProfile>,
    application_storage_promise: DeferredPromise<ApplicationStorage>,
    profile_storage_disposables: Rc<DisposableStore>,
    service: RefCell<Weak<AbstractStorageService>>,
}

impl BrowserBackend {
    fn this(&self) -> Rc<Self> {
        self.me.upgrade().expect("o backend está vivo enquanto um método dele roda")
    }

    fn service(&self) -> Rc<AbstractStorageService> {
        self.service.borrow().upgrade().expect("o serviço está vivo enquanto o backend roda")
    }

    /// `this.emitDidChangeValue(scope, e)` ligado ao `onDidChangeStorage` de um `Storage`.
    fn wire(&self, scope: StorageScope, storage: &Rc<Storage>) -> DisposableRef {
        let weak = self.service.borrow().clone();
        storage.on_did_change_storage().listen(move |e| {
            if let Some(service) = weak.upgrade() {
                service.emit_did_change_value(scope, e);
            }
        })
    }

    fn has_pending_update(&self) -> bool {
        let state = self.state.borrow();
        [&state.application_storage_database, &state.application_shared_storage_database, &state.profile_storage_database, &state.workspace_storage_database]
            .into_iter()
            .any(|database| database.as_ref().is_some_and(|database| database.has_pending_update()))
    }

    /// Os quatro bancos, na ordem em que o `clear` os limpa.
    fn databases(&self) -> [Option<Rc<dyn IIndexedDbStorageDatabase>>; 4] {
        let state = self.state.borrow();
        [
            state.application_storage_database.clone(),
            state.application_shared_storage_database.clone(),
            state.profile_storage_database.clone(),
            state.workspace_storage_database.clone(),
        ]
    }

    fn create_application_storage(&self) -> Promise<()> {
        let this = self.this();
        async_fn(async move {
            let database = IndexedDbStorageDatabase::create_application_storage(this.log_service.clone(), this.host.clone()).await?;
            let service = this.service();

            service.register(database.clone());
            let storage = Storage::new(database.clone(), StorageOptions::default());
            service.register(storage.clone());
            {
                let mut state = this.state.borrow_mut();
                state.application_storage_database = Some(database.clone());
                state.application_storage = Some(storage.clone());
            }

            service.register(this.wire(StorageScope::Application, &storage));

            storage.init().await?;

            this.update_is_new(&storage);

            this.application_storage_promise.complete((database, storage));
            Ok(())
        })
    }

    fn create_application_shared_storage(&self) -> Promise<()> {
        let this = self.this();
        async_fn(async move {
            let database = IndexedDbStorageDatabase::create_application_shared_storage(this.log_service.clone(), this.host.clone()).await?;
            let service = this.service();

            service.register(database.clone());
            let storage = Storage::new(database.clone(), StorageOptions::default());
            service.register(storage.clone());
            {
                let mut state = this.state.borrow_mut();
                state.application_shared_storage_database = Some(database);
                state.application_shared_storage = Some(storage.clone());
            }

            service.register(this.wire(StorageScope::ApplicationShared, &storage));

            storage.init().await?;

            this.update_is_new(&storage);
            Ok(())
        })
    }

    fn create_profile_storage(&self, profile: StorageProfile) -> Promise<()> {
        let this = self.this();
        async_fn(async move {
            // First clear any previously associated disposables
            this.profile_storage_disposables.clear();

            // Remember profile associated to profile storage
            *this.profile_storage_profile.borrow_mut() = profile.clone();

            if is_profile_using_default_storage(&profile) {
                // If we are using default profile storage, the profile storage is actually the same as application storage. As such we
                // avoid creating the storage library a second time on the same DB.

                let (application_storage_indexed_db, application_storage) = this.application_storage_promise.promise.clone().await?;

                {
                    let mut state = this.state.borrow_mut();
                    state.profile_storage_database = Some(application_storage_indexed_db);
                    state.profile_storage = Some(application_storage.clone());
                }

                this.profile_storage_disposables.add(this.wire(StorageScope::Profile, &application_storage));
            } else {
                let profile_storage_indexed_db = IndexedDbStorageDatabase::create_profile_storage(&profile.id, this.log_service.clone(), this.host.clone()).await?;

                this.profile_storage_disposables.add(profile_storage_indexed_db.clone());
                let storage = Storage::new(profile_storage_indexed_db.clone(), StorageOptions::default());
                this.profile_storage_disposables.add_rc(storage.clone());
                {
                    let mut state = this.state.borrow_mut();
                    state.profile_storage_database = Some(profile_storage_indexed_db);
                    state.profile_storage = Some(storage.clone());
                }

                this.profile_storage_disposables.add(this.wire(StorageScope::Profile, &storage));

                storage.init().await?;

                this.update_is_new(&storage);
            }
            Ok(())
        })
    }

    fn create_workspace_storage(&self) -> Promise<()> {
        let this = self.this();
        async_fn(async move {
            let database = IndexedDbStorageDatabase::create_workspace_storage(&this.workspace.id, this.log_service.clone(), this.host.clone()).await?;
            let service = this.service();

            service.register(database.clone());
            let storage = Storage::new(database.clone(), StorageOptions::default());
            service.register(storage.clone());
            {
                let mut state = this.state.borrow_mut();
                state.workspace_storage_database = Some(database);
                state.workspace_storage = Some(storage.clone());
            }

            service.register(this.wire(StorageScope::Workspace, &storage));

            storage.init().await?;

            this.update_is_new(&storage);
            Ok(())
        })
    }

    fn update_is_new(&self, storage: &Rc<Storage>) {
        match storage.get_boolean(IS_NEW_KEY, None) {
            None => {
                let _ = storage.set(IS_NEW_KEY, StorageValue::Bool(true), false);
            }
            Some(true) => {
                let _ = storage.set(IS_NEW_KEY, StorageValue::Bool(false), false);
            }
            Some(false) => {}
        }
    }

    /// `switchToProfile(toProfile)`.
    fn do_switch_to_profile(&self, service: &Rc<AbstractStorageService>, to_profile: StorageProfile) -> Promise<()> {
        let (this, service) = (self.this(), service.clone());
        async_fn(async move {
            let from_profile = this.profile_storage_profile.borrow().clone();
            if !service.can_switch_profile(&from_profile, &to_profile) {
                return Ok(());
            }

            let (old_profile_storage, application_storage) = {
                let state = this.state.borrow();
                (state.profile_storage.clone().expect("o armazém do perfil existe depois de initialize"), state.application_storage.clone())
            };
            let old_items = old_profile_storage.items().snapshot();

            // Close old profile storage but only if this is different from application storage!
            if !application_storage.as_ref().is_some_and(|application_storage| Rc::ptr_eq(&old_profile_storage, application_storage)) {
                old_profile_storage.close().await?;
            }

            // Create new profile storage & init
            this.create_profile_storage(to_profile).await?;

            // Handle data switch and eventing
            let new_profile_storage = this.state.borrow().profile_storage.clone().expect("o armazém do perfil acabou de ser criado");
            service.switch_data(&old_items, new_profile_storage.as_ref(), StorageScope::Profile);
            Ok(())
        })
    }
}

impl StorageBackend for BrowserBackend {
    fn has_scope(&self, scope: &StorageSwitchTarget) -> bool {
        match scope {
            StorageSwitchTarget::Profile(profile) => self.profile_storage_profile.borrow().id == profile.id,
            StorageSwitchTarget::Workspace(workspace) => self.workspace.id == workspace.id,
        }
    }

    fn do_initialize(&self) -> Promise<()> {
        // Init storages
        let this = self.this();
        let profile = self.profile_storage_profile.borrow().clone();
        async_fn(async move {
            promises::settled(vec![
                this.create_application_storage(),
                this.create_application_shared_storage(),
                this.create_profile_storage(profile),
                this.create_workspace_storage(),
            ])
            .await?;
            Ok(())
        })
    }

    fn get_storage(&self, scope: StorageScope) -> Option<Rc<dyn IStorage>> {
        let state = self.state.borrow();
        let storage = match scope {
            StorageScope::ApplicationShared => &state.application_shared_storage,
            StorageScope::Application => &state.application_storage,
            StorageScope::Profile => &state.profile_storage,
            StorageScope::Workspace => &state.workspace_storage,
        };
        storage.clone().map(|storage| storage as Rc<dyn IStorage>)
    }

    fn get_log_details(&self, scope: StorageScope) -> Option<String> {
        let state = self.state.borrow();
        let database = match scope {
            StorageScope::ApplicationShared => &state.application_shared_storage_database,
            StorageScope::Application => &state.application_storage_database,
            StorageScope::Profile => &state.profile_storage_database,
            StorageScope::Workspace => &state.workspace_storage_database,
        };
        database.as_ref().map(|database| database.name())
    }

    fn switch_to_profile(&self, service: &Rc<AbstractStorageService>, to_profile: &StorageProfile, _preserve_data: bool) -> Promise<()> {
        self.do_switch_to_profile(service, to_profile.clone())
    }

    fn switch_to_workspace(&self, _service: &Rc<AbstractStorageService>, _to_workspace: &StorageWorkspace, _preserve_data: bool) -> Promise<()> {
        Promise::rejected(Error::new("Migrating storage is currently unsupported in Web"))
    }

    fn should_flush_when_idle(&self) -> bool {
        // this flush() will potentially cause new state to be stored since new state will only be created while the document has
        // focus, one optimization is to not run this when the document has no focus, assuming that state has not changed
        //
        // another optimization is to not collect more state if we have a pending update already running which indicates that the
        // connection is either slow or disconnected and thus unhealthy.
        self.host.has_focus() && !self.has_pending_update()
    }

    fn read_application_shared_value(&self, service: &Rc<AbstractStorageService>, key: &str) -> Option<Promise<Option<String>>> {
        let (this, service, key) = (self.this(), service.clone(), key.to_string());
        Some(async_fn(async move {
            service.initialize().await?;
            let database = this.state.borrow().application_shared_storage_database.clone().expect("o banco compartilhado existe depois de initialize");
            Ok(database.get_value(&key).await?)
        }))
    }

    fn compare_and_swap_application_shared_value(
        &self,
        service: &Rc<AbstractStorageService>,
        key: &str,
        expected_value: Option<&str>,
        new_value: &str,
    ) -> Option<Promise<CompareAndSwapResult>> {
        let (this, service) = (self.this(), service.clone());
        let (key, expected_value, new_value) = (key.to_string(), expected_value.map(str::to_string), new_value.to_string());
        Some(async_fn(async move {
            service.initialize().await?;
            service.update_key_target(&key, StorageScope::ApplicationShared, Some(StorageTarget::Machine), false);
            let database = this.state.borrow().application_shared_storage_database.clone().expect("o banco compartilhado existe depois de initialize");
            Ok(database.compare_and_swap(&key, expected_value.as_deref(), &new_value).await?)
        }))
    }
}

/// `BrowserStorageService`.
pub struct BrowserStorageService {
    base: Rc<Disposable>,
    service: Rc<AbstractStorageService>,
    backend: Rc<BrowserBackend>,
    host: Rc<dyn BrowserStorageHost>,
}

impl BrowserStorageService {
    pub fn new(
        workspace: StorageWorkspace,
        user_data_profile_service: Rc<dyn IUserDataProfileService>,
        log_service: LogServiceRef,
        host: Rc<dyn BrowserStorageHost>,
    ) -> Rc<Self> {
        let profile_storage_profile = storage_profile_of(&user_data_profile_service.current_profile());

        let backend = Rc::new_cyclic(|me| BrowserBackend {
            me: me.clone(),
            workspace,
            log_service,
            host: host.clone(),
            state: RefCell::new(State::default()),
            profile_storage_profile: RefCell::new(profile_storage_profile),
            application_storage_promise: DeferredPromise::new(),
            profile_storage_disposables: DisposableStore::new(),
            service: RefCell::new(Weak::new()),
        });
        let service = AbstractStorageService::new(backend.clone(), StorageServiceCoreOptions { flush_interval: BROWSER_DEFAULT_FLUSH_INTERVAL });
        *backend.service.borrow_mut() = Rc::downgrade(&service);
        service.register(backend.profile_storage_disposables.clone());

        let base = Disposable::new();
        base.register(service.clone());

        // registerListeners
        let (weak_backend, weak_service) = (Rc::downgrade(&backend), Rc::downgrade(&service));
        base.register(user_data_profile_service.on_did_change_current_profile().listen(move |e| {
            if let (Some(backend), Some(service)) = (weak_backend.upgrade(), weak_service.upgrade()) {
                e.join(backend.do_switch_to_profile(&service, storage_profile_of(&e.profile)));
            }
        }));

        Rc::new(Self { base, service, backend, host })
    }

    /// O serviço de armazenamento (`IStorageService`).
    pub fn service(&self) -> Rc<AbstractStorageService> {
        self.service.clone()
    }

    /// `initialize()`.
    pub fn initialize(&self) -> Promise<()> {
        self.service.initialize()
    }

    pub fn has_pending_update(&self) -> bool {
        self.backend.has_pending_update()
    }

    pub fn get_application_storage_value(&self, key: &str) -> Promise<Option<String>> {
        let (application_storage, key) = (self.backend.application_storage_promise.promise.clone(), key.to_string());
        async_fn(async move {
            let (indexed_db, _) = application_storage.await?;
            Ok(indexed_db.get_value(&key).await?)
        })
    }

    pub fn compare_and_swap_application_storage(&self, key: &str, expected_value: Option<&str>, new_value: &str) -> Promise<CompareAndSwapResult> {
        let (application_storage, key, expected_value, new_value) =
            (self.backend.application_storage_promise.promise.clone(), key.to_string(), expected_value.map(str::to_string), new_value.to_string());
        async_fn(async move {
            let (indexed_db, _) = application_storage.await?;
            Ok(indexed_db.compare_and_swap(&key, expected_value.as_deref(), &new_value).await?)
        })
    }

    pub fn close(&self) {
        // Safari: there is an issue where the page can hang on load when a previous session has kept IndexedDB transactions
        // running. The only fix seems to be to cancel any pending transactions (https://github.com/microsoft/vscode/issues/136295)
        //
        // On all other browsers, we keep the databases opened because we expect data to be written when the unload happens.
        if self.host.is_safari() {
            let (application_storage, shared, profile, workspace) = {
                let state = self.backend.state.borrow();
                (
                    state.application_storage.clone(),
                    state.application_shared_storage_database.clone(),
                    state.profile_storage_database.clone(),
                    state.workspace_storage_database.clone(),
                )
            };
            if let Some(application_storage) = application_storage {
                let _ = application_storage.close();
            }
            for database in [shared, profile, workspace].into_iter().flatten() {
                let _ = database.close(None);
            }
        }

        // Always dispose to ensure that no timeouts or callbacks get triggered in this phase.
        self.dispose();
    }

    pub fn clear(&self) -> Promise<()> {
        let (service, backend) = (self.service.clone(), self.backend.clone());
        async_fn(async move {
            // Clear key/values
            for scope in [StorageScope::Application, StorageScope::ApplicationShared, StorageScope::Profile, StorageScope::Workspace] {
                for target in [StorageTarget::User, StorageTarget::Machine] {
                    for key in StorageService::keys(&service, scope, target) {
                        StorageService::remove(&service, &key, scope);
                    }
                }

                if let Some(storage) = backend.get_storage(scope) {
                    storage.when_flushed().await?;
                }
            }

            // Clear databases
            let clears: Vec<Promise<()>> = backend.databases().into_iter().map(|database| database.map_or_else(|| Promise::resolved(()), |database| database.clear())).collect();
            promises::settled(clears).await?;
            Ok(())
        })
    }

    pub fn has_scope(&self, scope: &StorageSwitchTarget) -> bool {
        self.backend.has_scope(scope)
    }

    /// `flush()` do `AbstractStorageService`, com a razão padrão.
    pub fn flush(&self) -> Promise<()> {
        StorageService::flush(&self.service, WillSaveStateReason::None)
    }
}

impl IDisposable for BrowserStorageService {
    fn dispose(&self) {
        self.base.dispose();
    }
}
