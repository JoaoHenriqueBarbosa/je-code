//! Porte de `src/vs/platform/secrets/common/secrets.ts`: o armazenamento de segredos (`ISecretStorageService`), que guarda
//! cada segredo cifrado no `IStorageService` (ou em memória, quando não há cifra disponível), com os eventos de mudança.
//!
//! Decisões de tradução:
//! - **Interfaces.** `ISecretStorageProvider` e `ISecretStorageService` são os traits [`ISecretStorageProvider`] e
//!   [`ISecretStorageService`] (identificador [`I_SECRET_STORAGE_SERVICE`]). O `type: 'in-memory' | 'persisted' |
//!   'unknown'` é [`SecretStorageType`] (método `type_`), o `keys?()` opcional é `keys()` devolvendo `None` quando o
//!   provedor não o tem, e as promessas são [`Promise`].
//! - **Funções `async`.** `readEncryptedSecret`, `writeEncryptedSecret` e os métodos do serviço são `async_fn` do `je-base`:
//!   o corpo roda de forma síncrona até o primeiro `await`, e cada `await` cede uma rodada de microtarefa, como no
//!   JavaScript. O `logService?` opcional é `Option<LogServiceRef>`; o `logService.trace(message, fullKey)` é
//!   `trace_args(message, [fullKey])`.
//! - **`BaseSecretStorageService`** é a struct [`BaseSecretStorageService`], criada em `Rc` (as promessas guardam o
//!   serviço). A herança vira composição: o `useSharedStorage(key)` que a subclasse sobrescreve (o teste do original faz
//!   isso) é a política injetada de [`BaseSecretStorageService::with_shared_storage_policy`] (o padrão é
//!   [`default_use_shared_storage`]); os membros `protected` (`_sequencer`, `_encryptionService`, `_logService`,
//!   `onDidChangeSecretEmitter`, `resolvedStorageService`, `reinitialize`) são métodos públicos. O `Lazy<Promise<...>>` é o
//!   `Lazy` do `je-base`, que o `reinitialize` troca por um novo.
//! - **`isWindows`** é `Platform::detect().is_windows`, calculado uma vez.
//! - **`new InMemoryStorageService()`** (registrado para descartar com o serviço) é o `InMemoryStorageService` do porte;
//!   o `IStorageService` que o serviço recebe é `Rc<dyn StorageService>`.

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};
use std::sync::LazyLock;

use je_base::async_::{SequencerByKey, async_fn};
use je_base::errors::Error;
use je_base::event::{self, Emitter, Event};
use je_base::lazy::Lazy;
use je_base::lifecycle::{Disposable, DisposableStore, IDisposable};
use je_base::platform::Platform;
use je_base::promise::Promise;
use je_instantiation::service_id;
use je_storage::StorageValue;

use crate::encryption::IEncryptionService;
use crate::log::{LogArg, LogMessage, LogService, LogServiceRef};
use crate::storage::{InMemoryStorageService, StorageScope, StorageService, StorageTarget};

/// The storage key prefix used for all secrets.
pub const SECRET_STORAGE_PREFIX: &str = "secret://";

/// Builds the full storage key for a secret.
pub fn secret_storage_key(key: &str) -> String {
    format!("{SECRET_STORAGE_PREFIX}{key}")
}

/// `logService?.trace(message, key)`.
fn trace_key(log_service: Option<&dyn LogService>, message: &str, key: &str) {
    if let Some(log_service) = log_service {
        log_service.trace_args(message, &[LogArg::from(key)]);
    }
}

/// Reads an encrypted secret from storage and decrypts it.
///
/// - `key`: the secret key (without the `secret://` prefix).
/// - `storage_get`: reads the encrypted value from storage given a full storage key.
/// - `decrypt`: decrypts the encrypted value.
/// - `log_service`: optional logger for trace output.
pub fn read_encrypted_secret(
    key: &str,
    storage_get: impl FnOnce(&str) -> Option<String> + 'static,
    decrypt: impl FnOnce(String) -> Promise<String> + 'static,
    log_service: Option<LogServiceRef>,
) -> Promise<Option<String>> {
    let key = key.to_owned();
    async_fn(async move {
        let full_key = secret_storage_key(&key);
        trace_key(log_service.as_deref(), "[secrets] getting secret for key:", &full_key);
        // `!encrypted`: an empty string is also "no secret"
        let Some(encrypted) = storage_get(&full_key).filter(|encrypted| !encrypted.is_empty()) else {
            trace_key(log_service.as_deref(), "[secrets] no secret found for key:", &full_key);
            return Ok(None);
        };
        trace_key(log_service.as_deref(), "[secrets] decrypting secret for key:", &full_key);
        let result = decrypt(encrypted).await?;
        trace_key(log_service.as_deref(), "[secrets] decrypted secret for key:", &full_key);
        Ok(Some(result))
    })
}

/// Encrypts a secret value and writes it to storage.
///
/// - `key`: the secret key (without the `secret://` prefix).
/// - `value`: the plaintext secret value.
/// - `storage_set`: writes the encrypted value to storage given a full storage key.
/// - `encrypt`: encrypts the plaintext value.
/// - `log_service`: optional logger for trace output.
pub fn write_encrypted_secret(
    key: &str,
    value: &str,
    storage_set: impl FnOnce(&str, &str) + 'static,
    encrypt: impl FnOnce(String) -> Promise<String> + 'static,
    log_service: Option<LogServiceRef>,
) -> Promise<()> {
    let (key, value) = (key.to_owned(), value.to_owned());
    async_fn(async move {
        trace_key(log_service.as_deref(), "[secrets] encrypting secret for key:", &key);
        let encrypted = encrypt(value).await?;
        let full_key = secret_storage_key(&key);
        trace_key(log_service.as_deref(), "[secrets] storing encrypted secret for key:", &full_key);
        storage_set(&full_key, &encrypted);
        trace_key(log_service.as_deref(), "[secrets] stored encrypted secret for key:", &full_key);
        Ok(())
    })
}

/// Secret keys that should be shared between the VS Code app and the agents app.
/// When the agents app starts and doesn't have these secrets, it requests them
/// from VS Code via crossAppIPC.
pub const CROSS_APP_SHARED_SECRET_KEYS: &[&str] = &["{\"extensionId\":\"vscode.github-authentication\",\"key\":\"github.auth\"}"];

/// `ISecretStorageProvider.type`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SecretStorageType {
    InMemory,
    Persisted,
    Unknown,
}

impl SecretStorageType {
    /// O valor textual do original (`'in-memory'`, `'persisted'` ou `'unknown'`).
    pub fn as_str(self) -> &'static str {
        match self {
            Self::InMemory => "in-memory",
            Self::Persisted => "persisted",
            Self::Unknown => "unknown",
        }
    }
}

/// `ISecretStorageProvider`.
pub trait ISecretStorageProvider {
    /// `type`.
    fn type_(&self) -> SecretStorageType;

    fn get(&self, key: &str) -> Promise<Option<String>>;

    fn set(&self, key: &str, value: &str) -> Promise<()>;

    fn delete(&self, key: &str) -> Promise<()>;

    /// `keys?()`: `None` quando o provedor não sabe listar as chaves.
    fn keys(&self) -> Option<Promise<Vec<String>>> {
        None
    }
}

/// `ISecretStorageService`.
pub trait ISecretStorageService: ISecretStorageProvider {
    /// `onDidChangeSecret`: a chave do segredo (sem o prefixo `secret://`) que mudou.
    fn on_did_change_secret(&self) -> Event<String>;
}

service_id!(pub I_SECRET_STORAGE_SERVICE: dyn ISecretStorageService = "secretStorageService");

/// O `IStorageService` que o serviço usa.
type StorageServiceRef = Rc<dyn StorageService>;

/// O `useSharedStorage(key)` do serviço: quais chaves vão para o escopo `ApplicationShared`.
pub type UseSharedStoragePolicy = Rc<dyn Fn(&str) -> bool>;

static IS_WINDOWS: LazyLock<bool> = LazyLock::new(|| Platform::detect().is_windows);

/// `isWindows && CROSS_APP_SHARED_SECRET_KEYS.includes(key)`: o `useSharedStorage` padrão.
pub fn default_use_shared_storage(key: &str) -> bool {
    *IS_WINDOWS && CROSS_APP_SHARED_SECRET_KEYS.iter().any(|shared| *shared == key)
}

type LazyStorageService = Lazy<Promise<StorageServiceRef>>;

/// `BaseSecretStorageService`.
pub struct BaseSecretStorageService {
    base: Rc<Disposable>,
    on_did_change_secret_emitter: Rc<Emitter<String>>,
    sequencer: SequencerByKey<String>,
    type_: Cell<SecretStorageType>,
    on_did_change_value_disposable: Rc<DisposableStore>,
    use_in_memory_storage: bool,
    storage_service: StorageServiceRef,
    encryption_service: Rc<dyn IEncryptionService>,
    log_service: LogServiceRef,
    use_shared_storage_policy: UseSharedStoragePolicy,
    lazy_storage_service: RefCell<Rc<LazyStorageService>>,
    me: Weak<Self>,
}

impl BaseSecretStorageService {
    pub fn new(use_in_memory_storage: bool, storage_service: StorageServiceRef, encryption_service: Rc<dyn IEncryptionService>, log_service: LogServiceRef) -> Rc<Self> {
        Self::with_shared_storage_policy(use_in_memory_storage, storage_service, encryption_service, log_service, Rc::new(default_use_shared_storage))
    }

    /// O serviço de uma subclasse que sobrescreve o `useSharedStorage(key)`.
    pub fn with_shared_storage_policy(
        use_in_memory_storage: bool,
        storage_service: StorageServiceRef,
        encryption_service: Rc<dyn IEncryptionService>,
        log_service: LogServiceRef,
        use_shared_storage_policy: UseSharedStoragePolicy,
    ) -> Rc<Self> {
        let base = Disposable::new();
        let on_did_change_secret_emitter = Emitter::new();
        base.register(on_did_change_secret_emitter.clone());
        let on_did_change_value_disposable = DisposableStore::new();
        base.register(on_did_change_value_disposable.clone());

        Rc::new_cyclic(|me| Self {
            base,
            on_did_change_secret_emitter,
            sequencer: SequencerByKey::new(),
            type_: Cell::new(SecretStorageType::Unknown),
            on_did_change_value_disposable,
            use_in_memory_storage,
            storage_service,
            encryption_service,
            log_service,
            use_shared_storage_policy,
            lazy_storage_service: RefCell::new(Self::new_lazy_storage_service(me)),
            me: me.clone(),
        })
    }

    fn new_lazy_storage_service(me: &Weak<Self>) -> Rc<LazyStorageService> {
        let me = me.clone();
        Rc::new(Lazy::new(move || me.upgrade().expect("o serviço de segredos foi descartado antes de inicializar").initialize()))
    }

    fn rc(&self) -> Rc<Self> {
        self.me.upgrade().expect("o serviço de segredos está vivo enquanto um método dele roda")
    }

    /// `useSharedStorage(key)`.
    pub fn use_shared_storage(&self, key: &str) -> bool {
        (self.use_shared_storage_policy)(key)
    }

    /// `_sequencer`.
    pub fn sequencer(&self) -> &SequencerByKey<String> {
        &self.sequencer
    }

    /// `_encryptionService`.
    pub fn encryption_service(&self) -> &Rc<dyn IEncryptionService> {
        &self.encryption_service
    }

    /// `_logService`.
    pub fn log_service(&self) -> &LogServiceRef {
        &self.log_service
    }

    /// `onDidChangeSecretEmitter`.
    pub fn on_did_change_secret_emitter(&self) -> &Rc<Emitter<String>> {
        &self.on_did_change_secret_emitter
    }

    /// `this._register(...)`: o que a subclasse registra para descartar junto.
    pub fn register(&self, disposable: je_base::lifecycle::DisposableRef) -> je_base::lifecycle::DisposableRef {
        self.base.register(disposable)
    }

    /// `resolvedStorageService`: o armazenamento, depois de decidir entre o persistido e o em memória.
    pub fn resolved_storage_service(&self) -> Promise<StorageServiceRef> {
        let lazy = self.lazy_storage_service.borrow().clone();
        lazy.value()
    }

    /// `reinitialize()`: a próxima operação decide de novo entre o persistido e o em memória.
    pub fn reinitialize(&self) {
        *self.lazy_storage_service.borrow_mut() = Self::new_lazy_storage_service(&self.me);
    }

    fn get_value_from_storage(&self, key: &str, full_key: &str, storage_service: &dyn StorageService) -> Option<String> {
        if self.use_shared_storage(key) {
            self.log_service.trace(&format!("[SecretStorageService] Fetching value for cross-app shared secret: {full_key}"));
            return storage_service.get(full_key, StorageScope::ApplicationShared, None);
        }
        storage_service.get(full_key, StorageScope::Application, None)
    }

    fn remove_value_from_storage(&self, key: &str, full_key: &str, storage_service: &dyn StorageService) {
        let scope = if self.use_shared_storage(key) { StorageScope::ApplicationShared } else { StorageScope::Application };
        storage_service.remove(full_key, scope);
    }

    fn set_value_in_storage(&self, key: &str, full_key: &str, value: &str, storage_service: &dyn StorageService) {
        if self.use_shared_storage(key) {
            self.log_service.trace(&format!("[SecretStorageService] Setting value for cross-app shared secret: {full_key}"));
            storage_service.store(full_key, StorageValue::String(value.to_owned()), StorageScope::ApplicationShared, StorageTarget::Machine);
            return;
        }
        storage_service.store(full_key, StorageValue::String(value.to_owned()), StorageScope::Application, StorageTarget::Machine);
    }

    fn initialize(self: &Rc<Self>) -> Promise<StorageServiceRef> {
        let this = self.clone();
        async_fn(async move {
            let storage_service: StorageServiceRef;
            if !this.use_in_memory_storage && this.encryption_service.is_encryption_available().await? {
                this.log_service.trace("[SecretStorageService] Encryption is available, using persisted storage");
                this.type_.set(SecretStorageType::Persisted);
                storage_service = this.storage_service.clone();
            } else {
                // If we already have an in-memory storage service, we don't need to recreate it
                if this.type_.get() == SecretStorageType::InMemory {
                    return Ok(this.storage_service.clone());
                }
                this.log_service.trace("[SecretStorageService] Encryption is not available, falling back to in-memory storage");
                this.type_.set(SecretStorageType::InMemory);
                let in_memory = InMemoryStorageService::new();
                this.base.register(in_memory.clone());
                storage_service = Rc::new(in_memory);
            }

            this.on_did_change_value_disposable.clear();
            let weak = Rc::downgrade(&this);
            let on_did_change_value = event::any(vec![
                storage_service.on_did_change_value(StorageScope::Application, None, &this.on_did_change_value_disposable),
                storage_service.on_did_change_value(StorageScope::ApplicationShared, None, &this.on_did_change_value_disposable),
            ]);
            this.on_did_change_value_disposable.add(on_did_change_value.listen(move |change| {
                if let Some(this) = weak.upgrade() {
                    this.on_did_change_value(&change.key);
                }
            }));
            Ok(storage_service)
        })
    }

    fn on_did_change_value(&self, key: &str) {
        let Some(secret_key) = key.strip_prefix(SECRET_STORAGE_PREFIX) else {
            return;
        };

        self.log_service.trace(&format!("[SecretStorageService] Notifying change in value for secret: {secret_key}"));
        self.on_did_change_secret_emitter.fire(secret_key.to_owned());
    }
}

impl ISecretStorageProvider for BaseSecretStorageService {
    /// `@Note initialize must be called first so that this can be resolved properly otherwise it will return 'unknown'.`
    fn type_(&self) -> SecretStorageType {
        self.type_.get()
    }

    fn get(&self, key: &str) -> Promise<Option<String>> {
        let this = self.rc();
        let key = key.to_owned();
        self.sequencer.queue(key.clone(), move || {
            async_fn(async move {
                let storage_service = this.resolved_storage_service().await?;

                let (read_this, read_service, read_key) = (this.clone(), storage_service.clone(), key.clone());
                let encryption_service = this.encryption_service.clone();
                // If the storage service is in-memory, we don't need to decrypt
                let in_memory = this.type_.get() == SecretStorageType::InMemory;
                let result = read_encrypted_secret(
                    &key,
                    move |full_key| read_this.get_value_from_storage(&read_key, full_key, &*read_service),
                    move |value| if in_memory { Promise::resolved(value) } else { encryption_service.decrypt(&value) },
                    Some(this.log_service.clone()),
                )
                .await;
                match result {
                    Ok(secret) => Ok(secret),
                    Err(error) => {
                        this.log_service.error_args(&LogMessage::Error(error), &[]);
                        let full_key = secret_storage_key(&key);
                        trace_key(Some(&*this.log_service), "[secrets] deleting invalid secret for key:", &full_key);
                        this.remove_value_from_storage(&key, &full_key, &*storage_service);
                        trace_key(Some(&*this.log_service), "[secrets] deleted invalid secret for key:", &full_key);
                        Ok(None)
                    }
                }
            })
        })
    }

    fn set(&self, key: &str, value: &str) -> Promise<()> {
        let this = self.rc();
        let (key, value) = (key.to_owned(), value.to_owned());
        self.sequencer.queue(key.clone(), move || {
            async_fn(async move {
                let storage_service = this.resolved_storage_service().await?;

                let (write_this, write_key) = (this.clone(), key.clone());
                let encryption_service = this.encryption_service.clone();
                // If the storage service is in-memory, we don't need to encrypt
                let in_memory = this.type_.get() == SecretStorageType::InMemory;
                let result = write_encrypted_secret(
                    &key,
                    &value,
                    move |full_key, encrypted| write_this.set_value_in_storage(&write_key, full_key, encrypted, &*storage_service),
                    move |value| if in_memory { Promise::resolved(value) } else { encryption_service.encrypt(&value) },
                    Some(this.log_service.clone()),
                )
                .await;
                if let Err(error) = &result {
                    this.log_service.error_args(&LogMessage::Error(error.clone()), &[]);
                }
                result
            })
        })
    }

    fn delete(&self, key: &str) -> Promise<()> {
        let this = self.rc();
        let key = key.to_owned();
        self.sequencer.queue(key.clone(), move || {
            async_fn(async move {
                let storage_service = this.resolved_storage_service().await?;

                let full_key = secret_storage_key(&key);
                trace_key(Some(&*this.log_service), "[secrets] deleting secret for key:", &full_key);
                this.remove_value_from_storage(&key, &full_key, &*storage_service);
                trace_key(Some(&*this.log_service), "[secrets] deleted secret for key:", &full_key);
                Ok::<(), Error>(())
            })
        })
    }

    fn keys(&self) -> Option<Promise<Vec<String>>> {
        let this = self.rc();
        Some(self.sequencer.queue("__keys__".to_owned(), move || {
            async_fn(async move {
                let storage_service = this.resolved_storage_service().await?;
                this.log_service.trace("[secrets] fetching keys of all secrets");
                let all_keys = storage_service.keys(StorageScope::Application, StorageTarget::Machine);
                this.log_service.trace("[secrets] fetched keys of all secrets");
                Ok::<_, Error>(all_keys.iter().filter_map(|key| key.strip_prefix(SECRET_STORAGE_PREFIX)).map(str::to_owned).collect())
            })
        }))
    }
}

impl ISecretStorageService for BaseSecretStorageService {
    fn on_did_change_secret(&self) -> Event<String> {
        self.on_did_change_secret_emitter.event()
    }
}

impl IDisposable for BaseSecretStorageService {
    fn dispose(&self) {
        self.base.dispose();
    }
}
