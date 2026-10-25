//! Porte de `src/vs/platform/storage/common/storage.ts`: `IStorageService` ([`StorageService`]),
//! `StorageScope`, `StorageTarget`, os eventos, `AbstractStorageService` e `InMemoryStorageService`.
//!
//! Decisões de tradução:
//! - **`AbstractStorageService`** é a struct [`AbstractStorageService`], que recebe os métodos abstratos
//!   (`doInitialize`, `getStorage`, `getLogDetails`, `hasScope`, `switchToProfile`, `switchToWorkspace`,
//!   `shouldFlushWhenIdle`) como um [`StorageBackend`]. O [`InMemoryStorageService`] e o
//!   [`SqliteStorageService`](super::SqliteStorageService) são backends diferentes sobre o mesmo núcleo.
//! - **`IUserDataProfile` e `IAnyWorkspaceIdentifier`** (de `userDataProfile` e `workspace`) são os tipos mínimos
//!   [`StorageProfile`] e [`StorageWorkspace`], só com o que o armazenamento lê (o workspace leva também o identificador
//!   completo, para o armazém do main gravar o `workspace.json`).
//! - **Sobrecargas** de `get`, `getBoolean`, `getNumber` e `getObject` (com e sem `fallbackValue`) são um
//!   único método com `fallback_value: Option<...>`, que devolve `Option`; os valores vêm do `je-storage`
//!   (`StorageValue`, e o objeto como [`Value`] JSON).
//! - **`store`** do trait não tem o parâmetro `external`; [`AbstractStorageService::store_with`] tem.
//! - **`mark(...)`** (marcas de desempenho) e `logStorage` (tabelas no console) não existem: `log()` escreve
//!   os itens no stderr.

use std::cell::RefCell;
use std::rc::Rc;

use je_base::async_::{RunOnceScheduler, run_when_global_idle};
use je_base::dynamic::json_quote;
use je_base::JsString;
use je_base::event::{Emitter, Event, PauseableEmitter};
use je_base::json::{Value, parse_str};
use je_base::lifecycle::{Disposable, DisposableStore, IDisposable, MutableDisposable};
use je_base::map::OrderedMap;
use je_base::promise::Promise;
use je_instantiation::service_id;
use je_storage::{IStorage, InMemoryStorageDatabase, Storage, StorageChangeEvent, StorageHint, StorageOptions, StorageValue};

use crate::workspace::AnyWorkspaceIdentifier;

pub const IS_NEW_KEY: &str = "__$__isNewStorageMarker";
pub const TARGET_KEY: &str = "__$__targetStorageMarker";

/// `WillSaveStateReason`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WillSaveStateReason {
    /// No specific reason to save state.
    None,
    /// A hint that the workbench is about to shutdown.
    Shutdown,
}

/// `IWillSaveStateEvent`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WillSaveStateEvent {
    pub reason: WillSaveStateReason,
}

/// `StorageScope` (os valores numéricos são os do original).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StorageScope {
    /// Todos os workspaces de todos os perfis, compartilhado entre o VS Code e o app de sessões.
    ApplicationShared = -2,
    /// Todos os workspaces de todos os perfis.
    Application = -1,
    /// Todos os workspaces do mesmo perfil.
    Profile = 0,
    /// O workspace atual.
    Workspace = 1,
}

/// `StorageTarget`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StorageTarget {
    /// The stored data is user specific and applies across machines.
    User = 0,
    /// The stored data is machine specific.
    Machine = 1,
}

/// `IStorageEntry`.
#[derive(Clone, Debug)]
pub struct StorageEntry {
    pub key: String,
    pub value: StorageValue,
    pub scope: StorageScope,
    pub target: StorageTarget,
}

/// `IStorageValueChangeEvent`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StorageValueChangeEvent {
    /// The scope for the storage entry that changed or was removed.
    pub scope: StorageScope,
    /// The `key` of the storage entry that was changed or was removed.
    pub key: String,
    /// The `target` can be `None` if a key is being removed.
    pub target: Option<StorageTarget>,
    /// Uma dica de como a mudança foi disparada: `true` se veio de fora (outro processo, sincronização,
    /// troca de perfil).
    pub external: bool,
}

/// `IStorageTargetChangeEvent`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StorageTargetChangeEvent {
    /// O escopo cujo alvo mudou. Quem ouve deve usar `keys(scope, target)` para a lista atualizada.
    pub scope: StorageScope,
}

/// `IUserDataProfile`, só com o que o armazenamento lê.
#[derive(Clone, Debug, PartialEq)]
pub struct StorageProfile {
    pub id: String,
    pub is_default: bool,
    /// `useDefaultFlags?.globalState`.
    pub use_default_global_state: bool,
    /// `globalStorageHome`: a pasta com o `state.vscdb` do perfil.
    pub global_storage_home: je_base::uri::URI,
}

/// `IAnyWorkspaceIdentifier`: o `id` que o armazenamento lê, mais o identificador completo quando quem troca de
/// workspace o tem (o armazém do main grava o `workspace.json` com a origem do workspace, que o `id` sozinho não traz).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StorageWorkspace {
    pub id: String,
    /// O identificador completo; `None` para quem só conhece o `id` (o armazém do navegador, que não tem pasta).
    pub identifier: Option<AnyWorkspaceIdentifier>,
}

impl StorageWorkspace {
    /// Um workspace conhecido só pelo `id`.
    pub fn from_id(id: impl Into<String>) -> Self {
        Self { id: id.into(), identifier: None }
    }
}

impl From<&AnyWorkspaceIdentifier> for StorageWorkspace {
    fn from(workspace: &AnyWorkspaceIdentifier) -> Self {
        Self { id: workspace.id().to_string(), identifier: Some(workspace.clone()) }
    }
}

/// O `IAnyWorkspaceIdentifier | IUserDataProfile` de `hasScope` e `switch`.
#[derive(Clone, Debug, PartialEq)]
pub enum StorageSwitchTarget {
    Profile(StorageProfile),
    Workspace(StorageWorkspace),
}

/// `isProfileUsingDefaultStorage(profile)`.
pub fn is_profile_using_default_storage(profile: &StorageProfile) -> bool {
    profile.is_default || profile.use_default_global_state
}

/// O retorno de `compareAndSwapApplicationSharedValue`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompareAndSwapResult {
    pub swapped: bool,
    pub current_value: Option<String>,
}

/// `IStorageService`.
pub trait StorageService {
    /// `onDidChangeValue(scope, key, disposable)`: emitido quando um dado do escopo (e da chave, se dada)
    /// muda ou é removido.
    fn on_did_change_value(&self, scope: StorageScope, key: Option<&str>, disposable: &DisposableStore) -> Event<StorageValueChangeEvent>;

    /// Emitted whenever target of a storage entry changes.
    fn on_did_change_target(&self) -> Event<StorageTargetChangeEvent>;

    /// Emitted when the storage is about to persist. Note: this event may be fired many times, not only on
    /// shutdown to prevent loss of state in situations where the shutdown is not sufficient to persist the
    /// data properly.
    fn on_will_save_state(&self) -> Event<WillSaveStateEvent>;

    /// `get(key, scope, fallbackValue?)`.
    fn get(&self, key: &str, scope: StorageScope, fallback_value: Option<&str>) -> Option<String>;

    /// Read authoritative shared state, bypassing the renderer cache.
    fn read_application_shared_value(&self, key: &str) -> Promise<Option<String>>;

    /// Atomically update local, machine-only shared state across windows.
    fn compare_and_swap_application_shared_value(&self, key: &str, expected_value: Option<&str>, new_value: &str) -> Promise<CompareAndSwapResult>;

    /// `getBoolean(key, scope, fallbackValue?)`.
    fn get_boolean(&self, key: &str, scope: StorageScope, fallback_value: Option<bool>) -> Option<bool>;

    /// `getNumber(key, scope, fallbackValue?)`: o `parseInt(..., 10)` do original.
    fn get_number(&self, key: &str, scope: StorageScope, fallback_value: Option<f64>) -> Option<f64>;

    /// `getObject(key, scope, fallbackValue?)`: o `JSON.parse` do original.
    fn get_object(&self, key: &str, scope: StorageScope, fallback_value: Option<Value>) -> Option<Value>;

    /// Store a value under the given key to storage. The value will be converted to a `string`. Storing
    /// either `undefined` or `null` will remove the entry under the key.
    fn store(&self, key: &str, value: StorageValue, scope: StorageScope, target: StorageTarget);

    /// Allows to store multiple values in a bulk operation. Events will only be emitted when all values have
    /// been stored. `external` is a hint to indicate the source of the operation is external, such as
    /// settings sync or profile changes.
    fn store_all(&self, entries: &[StorageEntry], external: bool);

    /// Delete an element stored under the provided key from storage.
    fn remove(&self, key: &str, scope: StorageScope);

    /// Returns all the keys used in the storage for the provided `scope` and `target`. Note: this will NOT
    /// return all keys stored in the storage layer: some keys may not have an associated `StorageTarget`.
    fn keys(&self, scope: StorageScope, target: StorageTarget) -> Vec<String>;

    /// Log the contents of the storage.
    fn log(&self) -> Promise<()>;

    /// Returns true if the storage service handles the provided scope.
    fn has_scope(&self, scope: &StorageSwitchTarget) -> bool;

    /// Switch storage to another workspace or profile. Optionally preserve the current data to the new storage.
    fn switch(&self, to: StorageSwitchTarget, preserve_data: bool) -> Promise<()>;

    /// Whether the storage for the given scope was created during this session or existed before.
    fn is_new(&self, scope: StorageScope) -> bool;

    /// Attempts to reduce the DB size via optimization commands if supported.
    fn optimize(&self, scope: StorageScope) -> Promise<()>;

    /// Allows to flush state, e.g. in cases where a shutdown is imminent. This will send out the
    /// `onWillSaveState` to ask everyone for latest state. The returned promise resolves when all updates to
    /// the underlying storage have been flushed.
    fn flush(&self, reason: WillSaveStateReason) -> Promise<()>;
}

service_id!(pub I_STORAGE_SERVICE: dyn StorageService = "storageService");

/// `IKeyTargets`.
type KeyTargets = OrderedMap<String, StorageTarget>;

/// `loadKeyTargets(storage)`.
pub fn load_key_targets(storage: &dyn IStorage) -> OrderedMap<String, StorageTarget> {
    let mut targets = OrderedMap::new();
    if let Some(keys_raw) = storage.get(TARGET_KEY, None).filter(|raw| !raw.is_empty()) {
        // Fail gracefully
        let mut errors = Vec::new();
        if let Some(Value::Object(object)) = parse_str(&keys_raw, &mut errors).filter(|_| errors.is_empty()) {
            for (key, value) in object.iter() {
                match value {
                    Value::Number(n) if *n == 0.0 => targets.insert(key.to_string_lossy(), StorageTarget::User),
                    Value::Number(n) if *n == 1.0 => targets.insert(key.to_string_lossy(), StorageTarget::Machine),
                    _ => None,
                };
            }
        }
    }

    targets
}

/// `JSON.stringify(keyTargets)`.
fn stringify_key_targets(targets: &KeyTargets) -> String {
    let entries: Vec<String> = targets.iter().map(|(key, target)| format!("{}:{}", json_quote(&JsString::from(key.as_str())), *target as i32)).collect();
    format!("{{{}}}", entries.join(","))
}

fn scope_index(scope: StorageScope) -> usize {
    match scope {
        StorageScope::ApplicationShared => 0,
        StorageScope::Application => 1,
        StorageScope::Profile => 2,
        StorageScope::Workspace => 3,
    }
}

/// Os métodos abstratos de `AbstractStorageService`.
pub trait StorageBackend {
    fn has_scope(&self, scope: &StorageSwitchTarget) -> bool;

    fn do_initialize(&self) -> Promise<()>;

    fn get_storage(&self, scope: StorageScope) -> Option<Rc<dyn IStorage>>;

    fn get_log_details(&self, scope: StorageScope) -> Option<String>;

    fn switch_to_profile(&self, service: &Rc<AbstractStorageService>, to_profile: &StorageProfile, preserve_data: bool) -> Promise<()>;

    fn switch_to_workspace(&self, service: &Rc<AbstractStorageService>, to_workspace: &StorageWorkspace, preserve_data: bool) -> Promise<()>;

    /// `shouldFlushWhenIdle()`.
    fn should_flush_when_idle(&self) -> bool {
        true
    }

    /// `readApplicationSharedValue(key)` sobrescrito pela subclasse (o `BrowserStorageService` lê direto do banco). `None`
    /// mantém o corpo da classe base, que lê o cache do escopo.
    fn read_application_shared_value(&self, _service: &Rc<AbstractStorageService>, _key: &str) -> Option<Promise<Option<String>>> {
        None
    }

    /// `compareAndSwapApplicationSharedValue(key, expectedValue, newValue)` sobrescrito pela subclasse. `None` mantém o
    /// corpo da classe base.
    fn compare_and_swap_application_shared_value(
        &self,
        _service: &Rc<AbstractStorageService>,
        _key: &str,
        _expected_value: Option<&str>,
        _new_value: &str,
    ) -> Option<Promise<CompareAndSwapResult>> {
        None
    }
}

/// `IStorageServiceOptions`.
#[derive(Clone, Copy, Debug)]
pub struct StorageServiceCoreOptions {
    pub flush_interval: u64,
}

impl Default for StorageServiceCoreOptions {
    fn default() -> Self {
        Self { flush_interval: AbstractStorageService::DEFAULT_FLUSH_INTERVAL }
    }
}

/// `AbstractStorageService`.
pub struct AbstractStorageService {
    base: Rc<Disposable>,
    backend: Rc<dyn StorageBackend>,
    on_did_change_value: Rc<PauseableEmitter<StorageValueChangeEvent>>,
    on_did_change_target: Rc<PauseableEmitter<StorageTargetChangeEvent>>,
    on_will_save_state: Rc<Emitter<WillSaveStateEvent>>,
    initialization_promise: RefCell<Option<Promise<()>>>,
    flush_when_idle_scheduler: Rc<RunOnceScheduler>,
    run_flush_when_idle: Rc<MutableDisposable<dyn IDisposable>>,
    key_targets: [RefCell<Option<KeyTargets>>; 4],
}

impl AbstractStorageService {
    pub const DEFAULT_FLUSH_INTERVAL: u64 = 60 * 1000; // every minute

    pub fn new(backend: Rc<dyn StorageBackend>, options: StorageServiceCoreOptions) -> Rc<Self> {
        let base = Disposable::new();
        let on_did_change_value = PauseableEmitter::new();
        let on_did_change_target = PauseableEmitter::new();
        let on_will_save_state = Emitter::new();
        base.register(on_did_change_value.clone());
        base.register(on_did_change_target.clone());
        base.register(on_will_save_state.clone());
        let run_flush_when_idle = MutableDisposable::new();
        base.register(run_flush_when_idle.clone());

        Rc::new_cyclic(|weak: &std::rc::Weak<Self>| {
            let weak = weak.clone();
            let flush_when_idle_scheduler = RunOnceScheduler::new(
                move || {
                    if let Some(this) = weak.upgrade() {
                        this.do_flush_when_idle();
                    }
                },
                options.flush_interval,
            );
            base.register(flush_when_idle_scheduler.clone());
            Self {
                base,
                backend,
                on_did_change_value,
                on_did_change_target,
                on_will_save_state,
                initialization_promise: RefCell::new(None),
                flush_when_idle_scheduler,
                run_flush_when_idle,
                key_targets: Default::default(),
            }
        })
    }

    /// `this._register(...)`: o que a subclasse registra para descartar junto.
    pub fn register(&self, disposable: je_base::lifecycle::DisposableRef) {
        self.base.register(disposable);
    }

    fn do_flush_when_idle(self: &Rc<Self>) {
        let this = self.clone();
        self.run_flush_when_idle.set(Some(run_when_global_idle(
            move |_| {
                if this.backend.should_flush_when_idle() {
                    let _ = StorageService::flush(&this, WillSaveStateReason::None);
                }

                // repeat
                this.flush_when_idle_scheduler.schedule(None);
            },
            None,
        )));
    }

    /// `stopFlushWhenIdle()`.
    pub fn stop_flush_when_idle(&self) {
        self.run_flush_when_idle.dispose();
        self.flush_when_idle_scheduler.dispose();
    }

    /// `initialize()`.
    pub fn initialize(self: &Rc<Self>) -> Promise<()> {
        if self.initialization_promise.borrow().is_none() {
            let this = self.clone();
            // Init all storage locations: ask subclasses to initialize storage
            let promise = self.backend.do_initialize().then(move |()| {
                // On some OS we do not get enough time to persist state on shutdown (e.g. when Windows
                // restarts after applying updates). In other cases, VSCode might crash, so we periodically
                // save state to reduce the chance of loosing any state. The library will be clever enough to
                // only save state that has actually changed.
                this.flush_when_idle_scheduler.schedule(None);
            });
            *self.initialization_promise.borrow_mut() = Some(promise);
        }

        self.initialization_promise.borrow().clone().expect("definido acima")
    }

    /// `emitDidChangeValue(scope, event)`.
    pub fn emit_did_change_value(&self, scope: StorageScope, event: &StorageChangeEvent) {
        let StorageChangeEvent { key, external } = event;

        // Specially handle `TARGET_KEY`
        if key == TARGET_KEY {
            // Clear our cached version which is now out of date
            *self.key_targets[scope_index(scope)].borrow_mut() = None;

            // Emit as `didChangeTarget` event
            self.on_did_change_target.fire(StorageTargetChangeEvent { scope });
        }
        // Emit any other key to outside
        else {
            let target = self.with_key_targets(scope, |targets| targets.get(key).copied());
            self.on_did_change_value.fire(StorageValueChangeEvent { scope, key: key.clone(), target, external: *external });
        }
    }

    /// `emitWillSaveState(reason)`.
    pub fn emit_will_save_state(&self, reason: WillSaveStateReason) {
        self.on_will_save_state.fire(WillSaveStateEvent { reason });
    }

    fn storage(&self, scope: StorageScope) -> Option<Rc<dyn IStorage>> {
        self.backend.get_storage(scope)
    }

    fn with_paused_emitters<R>(&self, f: impl FnOnce() -> R) -> R {
        // Pause emitters
        self.on_did_change_value.pause();
        self.on_did_change_target.pause();

        let result = je_base::errors::catch(f);

        // Resume emitters
        self.on_did_change_value.resume();
        self.on_did_change_target.resume();

        match result {
            Ok(value) => value,
            Err(error) => je_base::errors::throw(error),
        }
    }

    /// `store(key, value, scope, target, external)`.
    pub fn store_with(&self, key: &str, value: StorageValue, scope: StorageScope, target: StorageTarget, external: bool) {
        // We remove the key for undefined/null values
        if matches!(value, StorageValue::Undefined | StorageValue::Null) {
            self.remove_with(key, scope, external);
            return;
        }

        // Update our datastructures but send events only after
        self.with_paused_emitters(|| {
            // Update key-target map
            self.update_key_target(key, scope, Some(target), false);

            // Store actual value
            if let Some(storage) = self.storage(scope) {
                let _ = storage.set(key, value, external);
            }
        });
    }

    /// `remove(key, scope, external)`.
    pub fn remove_with(&self, key: &str, scope: StorageScope, external: bool) {
        // Update our datastructures but send events only after
        self.with_paused_emitters(|| {
            // Update key-target map
            self.update_key_target(key, scope, None, false);

            // Remove actual key
            if let Some(storage) = self.storage(scope) {
                let _ = storage.delete(key, external);
            }
        });
    }

    /// `updateKeyTarget(key, scope, target, external)`.
    pub fn update_key_target(&self, key: &str, scope: StorageScope, target: Option<StorageTarget>, external: bool) {
        let serialized = self.with_key_targets_mut(scope, |targets| match target {
            // Add
            Some(target) => {
                if targets.get(&key.to_owned()) != Some(&target) {
                    targets.insert(key.to_owned(), target);
                    return Some(stringify_key_targets(targets));
                }
                None
            }
            // Remove
            None => {
                if targets.remove(&key.to_owned()).is_some() {
                    return Some(stringify_key_targets(targets));
                }
                None
            }
        });
        if let Some(serialized) = serialized
            && let Some(storage) = self.storage(scope)
        {
            let _ = storage.set(TARGET_KEY, StorageValue::String(serialized), external);
        }
    }

    fn ensure_key_targets(&self, scope: StorageScope) {
        let index = scope_index(scope);
        if self.key_targets[index].borrow().is_none() {
            let loaded = self.storage(scope).map_or_else(OrderedMap::new, |storage| load_key_targets(storage.as_ref()));
            *self.key_targets[index].borrow_mut() = Some(loaded);
        }
    }

    fn with_key_targets<R>(&self, scope: StorageScope, f: impl FnOnce(&KeyTargets) -> R) -> R {
        self.ensure_key_targets(scope);
        f(self.key_targets[scope_index(scope)].borrow().as_ref().expect("carregado acima"))
    }

    fn with_key_targets_mut<R>(&self, scope: StorageScope, f: impl FnOnce(&mut KeyTargets) -> R) -> R {
        self.ensure_key_targets(scope);
        f(self.key_targets[scope_index(scope)].borrow_mut().as_mut().expect("carregado acima"))
    }

    /// `switchData(oldStorage, newStorage, scope)`.
    pub fn switch_data(&self, old_storage: &OrderedMap<String, String>, new_storage: &dyn IStorage, scope: StorageScope) {
        self.with_paused_emitters(|| {
            // Signal storage keys that have changed
            let mut handled_keys: Vec<String> = Vec::new();
            for (key, old_value) in old_storage.iter() {
                handled_keys.push(key.clone());

                let new_value = new_storage.get(key, None);
                if new_value.as_ref() != Some(old_value) {
                    self.emit_did_change_value(scope, &StorageChangeEvent { key: key.clone(), external: true });
                }
            }

            for (key, _) in new_storage.items().snapshot().iter() {
                if !handled_keys.contains(key) {
                    self.emit_did_change_value(scope, &StorageChangeEvent { key: key.clone(), external: true });
                }
            }
        });
    }

    /// `canSwitchProfile(from, to)`.
    pub fn can_switch_profile(&self, from: &StorageProfile, to: &StorageProfile) -> bool {
        if from.id == to.id {
            return false; // both profiles are same
        }

        if is_profile_using_default_storage(to) && is_profile_using_default_storage(from) {
            return false; // both profiles are using default
        }

        true
    }

    /// `switch(to, preserveData)`, com o `Rc` do serviço (os backends precisam dele).
    pub fn switch_to(self: &Rc<Self>, to: StorageSwitchTarget, preserve_data: bool) -> Promise<()> {
        // Signal as event so that clients can store data before we switch
        self.emit_will_save_state(WillSaveStateReason::None);

        match to {
            StorageSwitchTarget::Profile(profile) => self.backend.switch_to_profile(self, &profile, preserve_data),
            StorageSwitchTarget::Workspace(workspace) => self.backend.switch_to_workspace(self, &workspace, preserve_data),
        }
    }
}

impl StorageService for Rc<AbstractStorageService> {
    fn on_did_change_value(&self, scope: StorageScope, key: Option<&str>, disposable: &DisposableStore) -> Event<StorageValueChangeEvent> {
        let key = key.map(str::to_owned);
        self.on_did_change_value.event().filter(move |e| e.scope == scope && key.as_ref().is_none_or(|key| e.key == *key), Some(disposable))
    }

    fn on_did_change_target(&self) -> Event<StorageTargetChangeEvent> {
        self.on_did_change_target.event()
    }

    fn on_will_save_state(&self) -> Event<WillSaveStateEvent> {
        self.on_will_save_state.event()
    }

    fn get(&self, key: &str, scope: StorageScope, fallback_value: Option<&str>) -> Option<String> {
        self.storage(scope).and_then(|storage| storage.get(key, fallback_value))
    }

    fn read_application_shared_value(&self, key: &str) -> Promise<Option<String>> {
        if let Some(overridden) = self.backend.read_application_shared_value(self, key) {
            return overridden;
        }
        Promise::resolved(StorageService::get(self, key, StorageScope::ApplicationShared, None))
    }

    fn compare_and_swap_application_shared_value(&self, key: &str, expected_value: Option<&str>, new_value: &str) -> Promise<CompareAndSwapResult> {
        if let Some(overridden) = self.backend.compare_and_swap_application_shared_value(self, key, expected_value, new_value) {
            return overridden;
        }
        let current_value = StorageService::get(self, key, StorageScope::ApplicationShared, None);
        if current_value.as_deref() != expected_value {
            return Promise::resolved(CompareAndSwapResult { swapped: false, current_value });
        }
        StorageService::store(self, key, StorageValue::String(new_value.to_owned()), StorageScope::ApplicationShared, StorageTarget::Machine);
        Promise::resolved(CompareAndSwapResult { swapped: true, current_value: Some(new_value.to_owned()) })
    }

    fn get_boolean(&self, key: &str, scope: StorageScope, fallback_value: Option<bool>) -> Option<bool> {
        self.storage(scope).and_then(|storage| storage.get_boolean(key, fallback_value))
    }

    fn get_number(&self, key: &str, scope: StorageScope, fallback_value: Option<f64>) -> Option<f64> {
        self.storage(scope).and_then(|storage| storage.get_number(key, fallback_value))
    }

    fn get_object(&self, key: &str, scope: StorageScope, fallback_value: Option<Value>) -> Option<Value> {
        self.storage(scope).and_then(|storage| storage.get_object(key, fallback_value))
    }

    fn store(&self, key: &str, value: StorageValue, scope: StorageScope, target: StorageTarget) {
        self.store_with(key, value, scope, target, false);
    }

    fn store_all(&self, entries: &[StorageEntry], external: bool) {
        self.with_paused_emitters(|| {
            for entry in entries {
                self.store_with(&entry.key, entry.value.clone(), entry.scope, entry.target, external);
            }
        });
    }

    fn remove(&self, key: &str, scope: StorageScope) {
        self.remove_with(key, scope, false);
    }

    fn keys(&self, scope: StorageScope, target: StorageTarget) -> Vec<String> {
        self.with_key_targets(scope, |targets| targets.iter().filter(|(_, key_target)| **key_target == target).map(|(key, _)| key.clone()).collect())
    }

    fn log(&self) -> Promise<()> {
        for (scope, name) in [
            (StorageScope::Application, "Application"),
            (StorageScope::ApplicationShared, "Application Shared"),
            (StorageScope::Profile, "Profile"),
            (StorageScope::Workspace, "Workspace"),
        ] {
            let details = self.backend.get_log_details(scope).unwrap_or_default();
            eprintln!("Storage: {name} (path: {details})");
            if let Some(storage) = self.storage(scope) {
                for (key, value) in storage.items().snapshot().iter() {
                    eprintln!("  {key} = {value}");
                }
            }
        }
        Promise::resolved(())
    }

    fn has_scope(&self, scope: &StorageSwitchTarget) -> bool {
        self.backend.has_scope(scope)
    }

    fn switch(&self, to: StorageSwitchTarget, preserve_data: bool) -> Promise<()> {
        self.switch_to(to, preserve_data)
    }

    fn is_new(&self, scope: StorageScope) -> bool {
        StorageService::get_boolean(self, IS_NEW_KEY, scope, None) == Some(true)
    }

    fn flush(&self, reason: WillSaveStateReason) -> Promise<()> {
        // Signal event to collect changes
        self.on_will_save_state.fire(WillSaveStateEvent { reason });

        let promises: Vec<Promise<()>> = [StorageScope::Application, StorageScope::ApplicationShared, StorageScope::Profile, StorageScope::Workspace]
            .into_iter()
            .map(|scope| match (self.storage(scope), reason) {
                // Unspecific reason: just wait when data is flushed
                (Some(storage), WillSaveStateReason::None) => storage.when_flushed(),
                // Shutdown: we want to flush as soon as possible and not hit any delays that might be there
                (Some(storage), WillSaveStateReason::Shutdown) => storage.flush(Some(0)),
                (None, _) => Promise::resolved(()),
            })
            .collect();
        Promise::all_settled(promises).then(|_| ())
    }

    fn optimize(&self, scope: StorageScope) -> Promise<()> {
        // Await pending data to be flushed to the DB before attempting to optimize the DB
        let this = self.clone();
        StorageService::flush(self, WillSaveStateReason::None).then_promise(move |()| match this.storage(scope) {
            Some(storage) => storage.optimize(),
            None => Promise::resolved(()),
        })
    }
}

impl IDisposable for AbstractStorageService {
    fn dispose(&self) {
        self.base.dispose();
    }
}

fn in_memory_storage() -> Rc<Storage> {
    Storage::new(InMemoryStorageDatabase::new(), StorageOptions { hint: Some(StorageHint::StorageInMemory) })
}

struct InMemoryBackend {
    application_storage: Rc<Storage>,
    application_shared_storage: Rc<Storage>,
    profile_storage: Rc<Storage>,
    workspace_storage: Rc<Storage>,
    /// O perfil que `hasScope` aceita (o `TestStorageService` da sincronização, que só aceita o perfil padrão);
    /// `None` é o `InMemoryStorageService` comum, que não tem escopo de perfil.
    scoped_profile_id: Option<String>,
}

impl StorageBackend for InMemoryBackend {
    fn has_scope(&self, scope: &StorageSwitchTarget) -> bool {
        matches!(scope, StorageSwitchTarget::Profile(profile) if self.scoped_profile_id.as_deref() == Some(profile.id.as_str()))
    }

    fn do_initialize(&self) -> Promise<()> {
        Promise::resolved(())
    }

    fn get_storage(&self, scope: StorageScope) -> Option<Rc<dyn IStorage>> {
        Some(match scope {
            StorageScope::ApplicationShared => self.application_shared_storage.clone(),
            StorageScope::Application => self.application_storage.clone(),
            StorageScope::Profile => self.profile_storage.clone(),
            StorageScope::Workspace => self.workspace_storage.clone(),
        })
    }

    fn get_log_details(&self, scope: StorageScope) -> Option<String> {
        Some(
            match scope {
                StorageScope::ApplicationShared => "inMemory (application-shared)",
                StorageScope::Application => "inMemory (application)",
                StorageScope::Profile => "inMemory (profile)",
                StorageScope::Workspace => "inMemory (workspace)",
            }
            .to_owned(),
        )
    }

    fn switch_to_profile(&self, _service: &Rc<AbstractStorageService>, _to_profile: &StorageProfile, _preserve_data: bool) -> Promise<()> {
        // no-op when in-memory
        Promise::resolved(())
    }

    fn switch_to_workspace(&self, _service: &Rc<AbstractStorageService>, _to_workspace: &StorageWorkspace, _preserve_data: bool) -> Promise<()> {
        // no-op when in-memory
        Promise::resolved(())
    }

    fn should_flush_when_idle(&self) -> bool {
        false
    }
}

/// `InMemoryStorageService`.
pub struct InMemoryStorageService;

impl InMemoryStorageService {
    #[allow(clippy::new_ret_no_self)] // o construtor devolve o núcleo compartilhado, como `new InMemoryStorageService()` no original
    pub fn new() -> Rc<AbstractStorageService> {
        Self::build(None)
    }

    /// O `InMemoryStorageService` cujo `hasScope` aceita só o perfil de id `profile_id`: o `TestStorageService` de
    /// `userDataSyncClient.ts`, que o `IUserDataProfileStorageService` consulta para ler o perfil padrão
    /// direto do armazenamento do cliente.
    pub fn with_profile_scope(profile_id: &str) -> Rc<AbstractStorageService> {
        Self::build(Some(profile_id.to_owned()))
    }

    fn build(scoped_profile_id: Option<String>) -> Rc<AbstractStorageService> {
        let backend = InMemoryBackend {
            application_storage: in_memory_storage(),
            application_shared_storage: in_memory_storage(),
            profile_storage: in_memory_storage(),
            workspace_storage: in_memory_storage(),
            scoped_profile_id,
        };
        let storages = [
            (StorageScope::Workspace, backend.workspace_storage.clone()),
            (StorageScope::Profile, backend.profile_storage.clone()),
            (StorageScope::Application, backend.application_storage.clone()),
            (StorageScope::ApplicationShared, backend.application_shared_storage.clone()),
        ];
        let service = AbstractStorageService::new(Rc::new(backend), StorageServiceCoreOptions::default());
        for (scope, storage) in storages {
            service.register(storage.clone());
            let weak = Rc::downgrade(&service);
            service.register(storage.on_did_change_storage().listen(move |e| {
                if let Some(service) = weak.upgrade() {
                    service.emit_did_change_value(scope, e);
                }
            }));
        }
        service
    }
}
