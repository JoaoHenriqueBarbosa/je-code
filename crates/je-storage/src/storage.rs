//! Porte de `src/vs/base/parts/storage/common/storage.ts`: `Storage`, `InMemoryStorageDatabase`,
//! `MigratingStorage` e os tipos que ligam o `Storage` ao banco (`IStorageDatabase`).
//!
//! Onde o original devolve `Promise`, o porte devolve `je_base::promise::Promise`; as funções `async`
//! do TypeScript viram [`spawn_promise`], que roda o primeiro trecho de forma síncrona (os efeitos de
//! `set`/`delete` no cache e o evento acontecem na chamada, antes do primeiro `await`).

use std::any::Any;
use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use je_base::dynamic::number_to_js_string;
use je_base::errors;
use je_base::event::{Event, PauseableEmitter};
use je_base::json::Value;
use je_base::lifecycle::{Disposable, IDisposable};
use je_base::map::OrderedMap;
use je_base::promise::{Promise, Resolver};

use crate::async_utils::{ThrottledDelayer, spawn_promise};
use crate::marshalling;

/// `StorageHint`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StorageHint {
    /// O storage ainda não existe em disco: o `init` pula a leitura e a partida fica mais rápida.
    StorageDoesNotExist,
    /// O storage é sustentado por um banco em memória.
    StorageInMemory,
}

/// `IStorageOptions`.
#[derive(Clone, Copy, Debug, Default)]
pub struct StorageOptions {
    pub hint: Option<StorageHint>,
}

/// O `Map<string, string>` do original: uma referência compartilhada. O `InMemoryStorageDatabase`
/// devolve o próprio mapa no `getItems`, e o `Storage` passa a escrever nele, então o alias é observável.
#[derive(Clone, Debug, Default)]
pub struct ItemMap(Rc<RefCell<OrderedMap<String, String>>>);

impl ItemMap {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn from_map(map: OrderedMap<String, String>) -> Self {
        Self(Rc::new(RefCell::new(map)))
    }

    pub fn get(&self, key: &str) -> Option<String> {
        self.0.borrow().get(&key.to_string()).cloned()
    }

    pub fn has(&self, key: &str) -> bool {
        self.0.borrow().contains_key(&key.to_string())
    }

    pub fn set(&self, key: &str, value: &str) {
        self.0
            .borrow_mut()
            .insert(key.to_string(), value.to_string());
    }

    /// `Map#delete`: devolve se a chave existia.
    pub fn delete(&self, key: &str) -> bool {
        self.0.borrow_mut().remove(&key.to_string()).is_some()
    }

    pub fn len(&self) -> usize {
        self.0.borrow().len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.borrow().is_empty()
    }

    /// Uma cópia do conteúdo, na ordem de inserção.
    pub fn snapshot(&self) -> OrderedMap<String, String> {
        self.0.borrow().clone()
    }

    pub fn ptr_eq(&self, other: &ItemMap) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }
}

/// O `Set<string>` do original, com a ordem de inserção.
pub type KeySet = OrderedMap<String, ()>;

/// `IUpdateRequest`.
#[derive(Clone, Debug, Default)]
pub struct UpdateRequest {
    pub insert: Option<OrderedMap<String, String>>,
    pub delete: Option<KeySet>,
}

/// `IStorageItemsChangeEvent`.
#[derive(Clone, Debug, Default)]
pub struct StorageItemsChangeEvent {
    pub changed: Option<OrderedMap<String, String>>,
    pub deleted: Option<KeySet>,
}

/// `isStorageItemsChangeEvent(thing)`: `thing` é `None` para o `undefined`; qualquer valor de outro tipo
/// (no original, um objeto cujos campos não são `Map` nem `Set`) não é um evento de mudança.
pub fn is_storage_items_change_event(thing: Option<&dyn Any>) -> bool {
    thing
        .and_then(|thing| thing.downcast_ref::<StorageItemsChangeEvent>())
        .is_some_and(|event| event.changed.is_some() || event.deleted.is_some())
}

/// A recuperação que o `close` recebe: devolve os itens para recriar um banco que não está saudável.
pub type Recovery = Rc<dyn Fn() -> OrderedMap<String, String>>;

/// `IStorageDatabase`.
pub trait StorageDatabase {
    fn on_did_change_items_external(&self) -> Event<StorageItemsChangeEvent>;

    fn get_items(&self) -> Promise<ItemMap>;
    fn update_items(&self, request: UpdateRequest) -> Promise<()>;

    fn optimize(&self) -> Promise<()>;

    fn close(&self, recovery: Option<Recovery>) -> Promise<()>;
}

/// `IStorageChangeEvent`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StorageChangeEvent {
    /// A chave do item que mudou ou foi removido.
    pub key: String,
    /// `true` quando a mudança veio de fora (outro processo, sincronização, troca de perfil).
    pub external: bool,
}

/// `StorageValue`: o que o `set` aceita (`string | boolean | number | undefined | null | object`).
#[derive(Clone, Debug, PartialEq)]
pub enum StorageValue {
    Undefined,
    Null,
    Bool(bool),
    Number(f64),
    String(String),
    /// Objeto ou array, guardado como o `stringify` do `marshalling.ts` o escreve.
    Object(Value),
}

impl From<&str> for StorageValue {
    fn from(value: &str) -> Self {
        Self::String(value.to_string())
    }
}

impl From<bool> for StorageValue {
    fn from(value: bool) -> Self {
        Self::Bool(value)
    }
}

impl From<f64> for StorageValue {
    fn from(value: f64) -> Self {
        Self::Number(value)
    }
}

impl From<Value> for StorageValue {
    fn from(value: Value) -> Self {
        Self::Object(value)
    }
}

/// `StorageState`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StorageState {
    None,
    Initialized,
    Closed,
}

/// `IStorage`. Os métodos de leitura devolvem `None` onde o original devolve `undefined`; `fallback_value`
/// é o segundo argumento opcional.
pub trait IStorage: IDisposable {
    fn on_did_change_storage(&self) -> Event<StorageChangeEvent>;

    fn items(&self) -> ItemMap;
    fn size(&self) -> usize;

    /// `isInMemory()` de `Storage`: o Rust não tem a herança de `MigratingStorage extends Storage`, então a
    /// pergunta entra na interface para quem guarda um `IStorage` (o `BaseStorageMain` do `electron-main`).
    fn is_in_memory(&self) -> bool;

    fn init(&self) -> Promise<()>;

    fn get(&self, key: &str, fallback_value: Option<&str>) -> Option<String>;
    fn get_boolean(&self, key: &str, fallback_value: Option<bool>) -> Option<bool>;
    fn get_number(&self, key: &str, fallback_value: Option<f64>) -> Option<f64>;
    fn get_object(&self, key: &str, fallback_value: Option<Value>) -> Option<Value>;

    fn set(&self, key: &str, value: StorageValue, external: bool) -> Promise<()>;
    fn delete(&self, key: &str, external: bool) -> Promise<()>;

    fn flush(&self, delay: Option<u64>) -> Promise<()>;
    fn when_flushed(&self) -> Promise<()>;

    fn optimize(&self) -> Promise<()>;

    fn close(&self) -> Promise<()>;
}

/// `parseInt(value, 10)`: espaço à esquerda, sinal opcional e o maior prefixo de dígitos; `NaN` sem dígitos.
fn parse_int_10(value: &str) -> f64 {
    let trimmed = value.trim_start_matches(|c: char| c.is_whitespace() || c == '\u{feff}');
    let (sign, rest) = match trimmed.strip_prefix('-') {
        Some(rest) => (-1.0, rest),
        None => (1.0, trimmed.strip_prefix('+').unwrap_or(trimmed)),
    };
    let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
    if digits.is_empty() {
        return f64::NAN;
    }
    sign * digits.parse::<f64>().unwrap_or(f64::NAN)
}

/// `Storage`.
pub struct Storage {
    me: Weak<Storage>,
    base: Rc<Disposable>,
    /// `protected readonly database`: os testes de subclasse do original leem o banco por aqui.
    pub database: Rc<dyn StorageDatabase>,
    options: StorageOptions,
    on_did_change_storage: Rc<PauseableEmitter<StorageChangeEvent>>,
    state: Cell<StorageState>,
    cache: RefCell<ItemMap>,
    flush_delayer: Rc<ThrottledDelayer<()>>,
    pending_deletes: RefCell<KeySet>,
    pending_inserts: RefCell<OrderedMap<String, String>>,
    pending_close: RefCell<Option<Promise<()>>>,
    when_flushed_callbacks: RefCell<Vec<Resolver<()>>>,
}

impl Storage {
    const DEFAULT_FLUSH_DELAY: u64 = 100;

    pub fn new(database: Rc<dyn StorageDatabase>, options: StorageOptions) -> Rc<Self> {
        let storage = Rc::new_cyclic(|me| Self {
            me: me.clone(),
            base: Disposable::new(),
            database,
            options,
            on_did_change_storage: PauseableEmitter::new(),
            state: Cell::new(StorageState::None),
            cache: RefCell::new(ItemMap::new()),
            flush_delayer: Rc::new(ThrottledDelayer::new(Self::DEFAULT_FLUSH_DELAY)),
            pending_deletes: RefCell::new(KeySet::new()),
            pending_inserts: RefCell::new(OrderedMap::new()),
            pending_close: RefCell::new(None),
            when_flushed_callbacks: RefCell::new(Vec::new()),
        });
        storage.base.register(storage.on_did_change_storage.clone());
        storage.base.register(storage.flush_delayer.clone());
        storage.register_listeners();
        storage
    }

    fn this(&self) -> Rc<Self> {
        self.me
            .upgrade()
            .expect("o storage está vivo enquanto um método dele roda")
    }

    fn register_listeners(&self) {
        let me = self.me.clone();
        let subscription = self
            .database
            .on_did_change_items_external()
            .listen(move |e| {
                if let Some(storage) = me.upgrade() {
                    storage.on_did_change_items_external(e);
                }
            });
        self.base.register(subscription);
    }

    fn on_did_change_items_external(&self, e: &StorageItemsChangeEvent) {
        self.on_did_change_storage.pause();

        // try/finally: o evento retoma mesmo que um listener lance.
        let result = errors::catch(|| {
            // Itens que mudam por fora pedem que atualizemos os caches com os valores. Aceitamos o
            // valor e emitimos o evento se houve mudança.
            if let Some(changed) = &e.changed {
                for (key, value) in changed.iter() {
                    self.accept_external(key, Some(value));
                }
            }
            if let Some(deleted) = &e.deleted {
                for key in deleted.keys() {
                    self.accept_external(key, None);
                }
            }
        });
        self.on_did_change_storage.resume();
        if let Err(error) = result {
            errors::throw(error);
        }
    }

    fn accept_external(&self, key: &str, value: Option<&str>) {
        if self.state.get() == StorageState::Closed {
            return; // sai cedo se já fechamos
        }

        let cache = self.cache.borrow().clone();
        let changed = match value {
            // Item removido: confere a exclusão.
            None => cache.delete(key),
            // Item atualizado: confere a mudança.
            Some(value) => {
                if cache.get(key).as_deref() == Some(value) {
                    false
                } else {
                    cache.set(key, value);
                    true
                }
            }
        };

        // Avisa os de fora.
        if changed {
            self.on_did_change_storage.fire(StorageChangeEvent {
                key: key.to_string(),
                external: true,
            });
        }
    }

    pub fn state(&self) -> StorageState {
        self.state.get()
    }

    fn has_pending(&self) -> bool {
        !self.pending_inserts.borrow().is_empty() || !self.pending_deletes.borrow().is_empty()
    }

    fn do_close(&self) -> Promise<()> {
        let this = self.this();
        spawn_promise(async move {
            // Atualiza o estado.
            this.state.set(StorageState::Closed);

            // Dispara um novo flush para garantir a persistência e fecha mesmo que o flush dê erro:
            // o banco precisa sempre ser fechado para evitar corrupção.
            //
            // Recuperação: passamos o cache como opção, para o caso de o banco não estar saudável.
            let _ = this.do_flush(Some(0) /* o quanto antes */).await;

            let cache = this.cache.borrow().clone();
            this.database
                .close(Some(Rc::new(move || cache.snapshot())))
                .await
        })
    }

    fn flush_pending(&self) -> Promise<()> {
        let this = self.this();
        spawn_promise(async move {
            if !this.has_pending() {
                return Ok(()); // sai cedo se não há o que fazer
            }

            // Pega os dados pendentes e zera para a próxima rodada.
            let request = UpdateRequest {
                insert: Some(std::mem::take(&mut *this.pending_inserts.borrow_mut())),
                delete: Some(std::mem::take(&mut *this.pending_deletes.borrow_mut())),
            };

            // Atualiza o armazenamento e libera quem espera quando terminar.
            let storage = this.clone();
            this.database
                .update_items(request)
                .finally(move || {
                    if !storage.has_pending() {
                        loop {
                            let callback = storage.when_flushed_callbacks.borrow_mut().pop();
                            match callback {
                                Some(resolver) => resolver.resolve(()),
                                None => break,
                            }
                        }
                    }
                })
                .await
        })
    }

    fn do_flush(&self, delay: Option<u64>) -> Promise<()> {
        let this = self.this();
        spawn_promise(async move {
            if this.is_in_memory() {
                return this.flush_pending().await; // sai cedo se está em memória
            }
            let storage = this.clone();
            this.flush_delayer
                .trigger(move || storage.flush_pending(), delay)
                .await
        })
    }
}

impl IDisposable for Storage {
    fn dispose(&self) {
        self.base.dispose();
    }
}

impl IStorage for Storage {
    fn on_did_change_storage(&self) -> Event<StorageChangeEvent> {
        self.on_did_change_storage.event()
    }

    fn items(&self) -> ItemMap {
        self.cache.borrow().clone()
    }

    fn size(&self) -> usize {
        self.cache.borrow().len()
    }

    fn is_in_memory(&self) -> bool {
        self.options.hint == Some(StorageHint::StorageInMemory)
    }

    fn init(&self) -> Promise<()> {
        let this = self.this();
        spawn_promise(async move {
            if this.state.get() != StorageState::None {
                return Ok(()); // fechado ou já inicializado
            }

            this.state.set(StorageState::Initialized);

            if this.options.hint == Some(StorageHint::StorageDoesNotExist) {
                // Sai cedo se sabemos que o arquivo não existe: otimização para não carregar os itens do
                // banco quando não pode haver nenhum.
                return Ok(());
            }

            let items = this.database.get_items().await?;
            *this.cache.borrow_mut() = items;
            Ok(())
        })
    }

    fn get(&self, key: &str, fallback_value: Option<&str>) -> Option<String> {
        self.cache
            .borrow()
            .get(key)
            .or_else(|| fallback_value.map(str::to_string))
    }

    fn get_boolean(&self, key: &str, fallback_value: Option<bool>) -> Option<bool> {
        match self.get(key, None) {
            None => fallback_value,
            Some(value) => Some(value == "true"),
        }
    }

    fn get_number(&self, key: &str, fallback_value: Option<f64>) -> Option<f64> {
        match self.get(key, None) {
            None => fallback_value,
            Some(value) => Some(parse_int_10(&value)),
        }
    }

    fn get_object(&self, key: &str, fallback_value: Option<Value>) -> Option<Value> {
        match self.get(key, None) {
            None => fallback_value,
            Some(value) => Some(marshalling::parse(&value)),
        }
    }

    fn set(&self, key: &str, value: StorageValue, external: bool) -> Promise<()> {
        let this = self.this();
        let key = key.to_string();
        spawn_promise(async move {
            if this.state.get() == StorageState::Closed {
                return Ok(()); // sai cedo se já fechamos
            }

            // Removemos a chave para `undefined` e `null`.
            let value_str = match value {
                StorageValue::Undefined | StorageValue::Null => {
                    return this.delete(&key, external).await;
                }
                // Senão, converte para texto e guarda.
                StorageValue::Object(object) => marshalling::stringify(&object),
                StorageValue::Bool(b) => b.to_string(),
                StorageValue::Number(n) => number_to_js_string(n),
                StorageValue::String(s) => s,
            };

            // Sai cedo se o valor já está guardado.
            if this.cache.borrow().get(&key).as_deref() == Some(value_str.as_str()) {
                return Ok(());
            }

            // Atualiza o cache e os pendentes.
            this.cache.borrow().set(&key, &value_str);
            this.pending_inserts
                .borrow_mut()
                .insert(key.clone(), value_str);
            this.pending_deletes.borrow_mut().remove(&key);

            // Evento.
            this.on_did_change_storage
                .fire(StorageChangeEvent { key, external });

            // Acumula trabalho agendando depois de um prazo.
            this.do_flush(None).await
        })
    }

    fn delete(&self, key: &str, external: bool) -> Promise<()> {
        let this = self.this();
        let key = key.to_string();
        spawn_promise(async move {
            if this.state.get() == StorageState::Closed {
                return Ok(()); // sai cedo se já fechamos
            }

            // Remove do cache e põe nos pendentes.
            let was_deleted = this.cache.borrow().delete(&key);
            if !was_deleted {
                return Ok(()); // sai cedo se o valor já foi removido
            }

            if !this.pending_deletes.borrow().contains_key(&key) {
                this.pending_deletes.borrow_mut().insert(key.clone(), ());
            }

            this.pending_inserts.borrow_mut().remove(&key);

            // Evento.
            this.on_did_change_storage
                .fire(StorageChangeEvent { key, external });

            // Acumula trabalho agendando depois de um prazo.
            this.do_flush(None).await
        })
    }

    fn optimize(&self) -> Promise<()> {
        let this = self.this();
        spawn_promise(async move {
            if this.state.get() == StorageState::Closed {
                return Ok(()); // sai cedo se já fechamos
            }

            // Espera os dados pendentes irem para o banco antes de tentar otimizá-lo.
            this.flush(Some(0)).await?;

            this.database.optimize().await
        })
    }

    fn close(&self) -> Promise<()> {
        let this = self.this();
        spawn_promise(async move {
            let pending = this.pending_close.borrow().clone();
            let pending = match pending {
                Some(pending) => pending,
                None => {
                    let pending = this.do_close();
                    *this.pending_close.borrow_mut() = Some(pending.clone());
                    pending
                }
            };
            pending.await
        })
    }

    fn flush(&self, delay: Option<u64>) -> Promise<()> {
        let this = self.this();
        spawn_promise(async move {
            if this.state.get() == StorageState::Closed // sai cedo se já fechamos
                || this.pending_close.borrow().is_some()
            // sai cedo se não há o que fazer
            {
                return Ok(());
            }

            this.do_flush(delay).await
        })
    }

    fn when_flushed(&self) -> Promise<()> {
        let this = self.this();
        spawn_promise(async move {
            if !this.has_pending() {
                return Ok(()); // sai cedo se não há o que fazer
            }

            let (promise, resolver) = Promise::new();
            this.when_flushed_callbacks.borrow_mut().push(resolver);
            promise.await
        })
    }
}

/// `InMemoryStorageDatabase`.
#[derive(Default)]
pub struct InMemoryStorageDatabase {
    items: ItemMap,
}

impl InMemoryStorageDatabase {
    pub fn new() -> Rc<Self> {
        Rc::new(Self::default())
    }
}

impl StorageDatabase for InMemoryStorageDatabase {
    fn on_did_change_items_external(&self) -> Event<StorageItemsChangeEvent> {
        Event::none()
    }

    fn get_items(&self) -> Promise<ItemMap> {
        let items = self.items.clone();
        spawn_promise(async move { Ok(items) })
    }

    fn update_items(&self, request: UpdateRequest) -> Promise<()> {
        let items = self.items.clone();
        spawn_promise(async move {
            if let Some(insert) = &request.insert {
                for (key, value) in insert.iter() {
                    items.set(key, value);
                }
            }
            if let Some(delete) = &request.delete {
                for key in delete.keys() {
                    items.delete(key);
                }
            }
            Ok(())
        })
    }

    fn optimize(&self) -> Promise<()> {
        spawn_promise(async { Ok(()) })
    }

    fn close(&self, _recovery: Option<Recovery>) -> Promise<()> {
        spawn_promise(async { Ok(()) })
    }
}

pub const MIGRATED_KEY: &str = "__$__migratedStorageMarker";

const INTERNAL_KEY_PREFIX: &str = "__$__";

/// `MigratingStorage extends Storage`. O Rust não tem herança: a estrutura guarda o `Storage` e o
/// `Deref` expõe o resto; `init` e `get` são os métodos que o original sobrescreve.
pub struct MigratingStorage {
    me: Weak<MigratingStorage>,
    storage: Rc<Storage>,
    migrated_keys: RefCell<OrderedMap<String, ()>>,
    fallback_storage: RefCell<Option<Rc<dyn IStorage>>>,
    is_fallback_storage_readonly: Cell<bool>,
}

impl std::ops::Deref for MigratingStorage {
    type Target = Storage;

    fn deref(&self) -> &Storage {
        &self.storage
    }
}

impl MigratingStorage {
    pub fn new(database: Rc<dyn StorageDatabase>, options: StorageOptions) -> Rc<Self> {
        Rc::new_cyclic(|me| Self {
            me: me.clone(),
            storage: Storage::new(database, options),
            migrated_keys: RefCell::new(OrderedMap::new()),
            fallback_storage: RefCell::new(None),
            is_fallback_storage_readonly: Cell::new(false),
        })
    }

    pub fn init(self: &Rc<Self>) -> Promise<()> {
        let this = self.clone();
        spawn_promise(async move {
            this.storage.init().await?;

            // Carrega o conjunto de chaves já migradas do fallback.
            *this.migrated_keys.borrow_mut() = this.load_migrated_keys();
            Ok(())
        })
    }

    pub fn set_fallback_storage(&self, storage: Rc<dyn IStorage>, is_readonly: bool) {
        *self.fallback_storage.borrow_mut() = Some(storage);
        self.is_fallback_storage_readonly.set(is_readonly);
    }

    pub fn get(&self, key: &str, fallback_value: Option<&str>) -> Option<String> {
        if !key.starts_with(INTERNAL_KEY_PREFIX)
            && !self.migrated_keys.borrow().contains_key(&key.to_string())
            && self.storage.get(key, None).is_none()
        {
            // Confere o storage de fallback e migra sozinho se achar. A chave é marcada como migrada
            // já, para não conferir o fallback de novo e para uma chave removida de propósito depois da
            // migração não ressuscitar do fallback.
            self.migrated_keys.borrow_mut().insert(key.to_string(), ());
            let fallback = self.fallback_storage.borrow().clone();
            let value = fallback
                .as_ref()
                .and_then(|fallback| fallback.items().get(key));
            if let Some(value) = value {
                drop(self.storage.set(key, StorageValue::String(value), false));
                if !self.is_fallback_storage_readonly.get()
                    && let Some(fallback) = &fallback
                {
                    drop(fallback.delete(key, false));
                }
                self.persist_migrated_keys();
            }
        }
        self.storage.get(key, fallback_value)
    }

    fn load_migrated_keys(&self) -> OrderedMap<String, ()> {
        let mut keys = OrderedMap::new();
        if let Some(raw) = self
            .storage
            .get(MIGRATED_KEY, None)
            .filter(|raw| !raw.is_empty())
        {
            // `try { new Set(JSON.parse(raw)) } catch { fail gracefully }`
            if let Ok(Value::Array(items)) = errors::catch(|| marshalling::parse(&raw)) {
                for item in items {
                    if let Value::String(key) = item {
                        keys.insert(key.to_string_lossy(), ());
                    }
                }
            }
        }
        keys
    }

    fn persist_migrated_keys(&self) {
        let keys: Vec<Value> = self
            .migrated_keys
            .borrow()
            .keys()
            .map(|key| Value::String(key.as_str().into()))
            .collect();
        drop(self.storage.set(
            MIGRATED_KEY,
            StorageValue::String(marshalling::stringify(&Value::Array(keys))),
            false,
        ));
    }
}

impl IDisposable for MigratingStorage {
    fn dispose(&self) {
        self.storage.dispose();
    }
}

/// `MigratingStorage` como `IStorage`: o `init` e o `get` são os sobrescritos; o resto é o do `Storage`. As leituras
/// tipadas (`getBoolean`, `getNumber`, `getObject`) passam pelo `get` sobrescrito, como no original (o `Storage` as
/// implementa com `this.get`): a migração acontece na primeira leitura da chave, e a leitura tipada vem depois dela.
impl IStorage for MigratingStorage {
    fn on_did_change_storage(&self) -> Event<StorageChangeEvent> {
        self.storage.on_did_change_storage()
    }

    fn items(&self) -> ItemMap {
        self.storage.items()
    }

    fn size(&self) -> usize {
        self.storage.size()
    }

    fn is_in_memory(&self) -> bool {
        self.storage.is_in_memory()
    }

    fn init(&self) -> Promise<()> {
        MigratingStorage::init(&self.me.upgrade().expect("o storage está vivo enquanto um método dele roda"))
    }

    fn get(&self, key: &str, fallback_value: Option<&str>) -> Option<String> {
        MigratingStorage::get(self, key, fallback_value)
    }

    fn get_boolean(&self, key: &str, fallback_value: Option<bool>) -> Option<bool> {
        MigratingStorage::get(self, key, None);
        self.storage.get_boolean(key, fallback_value)
    }

    fn get_number(&self, key: &str, fallback_value: Option<f64>) -> Option<f64> {
        MigratingStorage::get(self, key, None);
        self.storage.get_number(key, fallback_value)
    }

    fn get_object(&self, key: &str, fallback_value: Option<Value>) -> Option<Value> {
        MigratingStorage::get(self, key, None);
        self.storage.get_object(key, fallback_value)
    }

    fn set(&self, key: &str, value: StorageValue, external: bool) -> Promise<()> {
        self.storage.set(key, value, external)
    }

    fn delete(&self, key: &str, external: bool) -> Promise<()> {
        self.storage.delete(key, external)
    }

    fn flush(&self, delay: Option<u64>) -> Promise<()> {
        self.storage.flush(delay)
    }

    fn when_flushed(&self) -> Promise<()> {
        self.storage.when_flushed()
    }

    fn optimize(&self) -> Promise<()> {
        self.storage.optimize()
    }

    fn close(&self) -> Promise<()> {
        self.storage.close()
    }
}
