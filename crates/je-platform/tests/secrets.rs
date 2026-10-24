//! Porte de `src/vs/platform/secrets/test/common/secrets.test.ts` (suíte `secrets`), caso a caso e na mesma ordem.
//!
//! Divergências: o `sinon.spy(new TestEncryptionService())` (que conta as chamadas de cada método e as repassa) é o
//! [`SpyEncryptionService`], um dublê que envolve o serviço e conta `encrypt` e `decrypt`; o `TestFailingEncryptionService`
//! (subclasse que sobrescreve `decrypt`) é um serviço próprio que compõe o `TestEncryptionService`. O
//! `TestSharedSecretStorageService` (subclasse que sobrescreve `useSharedStorage`) é o
//! `BaseSecretStorageService::with_shared_storage_policy` com a política do teste. O `store.add(new NullLogService())` não
//! registra nada (o `NullLogService` não é descartável, e a guarda de vazamento só vigia o que se registra). As suítes
//! `setup`/`teardown` aninhadas são a função `setup` de cada suíte, e o `sandbox.restore()` não existe (o espião morre com
//! o teste). Nos casos `onDidChangeSecret` o `assert.strictEqual(changedKey, key)` do ouvinte é conferido depois, sobre as
//! chaves gravadas, para o erro não ser engolido pelo emissor de eventos.

mod common;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use common::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite, run_with_faked_timers};
use je_base::errors::{self, Error};
use je_base::promise::Promise;
use je_platform::encryption::{ICommonEncryptionService, IEncryptionService, KnownStorageProvider};
use je_platform::log::NullLogService;
use je_platform::secrets::{
    BaseSecretStorageService, CROSS_APP_SHARED_SECRET_KEYS, ISecretStorageProvider, ISecretStorageService, SecretStorageType, secret_storage_key,
};
use je_platform::storage::{AbstractStorageService, InMemoryStorageService, StorageScope, StorageService, StorageTarget};
use je_storage::StorageValue;

/// `TestEncryptionService`.
#[derive(Default)]
struct TestEncryptionService;

/// prefix to simulate encryption
const ENCRYPTED_PREFIX: &str = "encrypted+";

impl ICommonEncryptionService for TestEncryptionService {
    fn encrypt(&self, value: &str) -> Promise<String> {
        Promise::resolved(format!("{ENCRYPTED_PREFIX}{value}"))
    }

    fn decrypt(&self, value: &str) -> Promise<String> {
        Promise::resolved(value.get(ENCRYPTED_PREFIX.len()..).unwrap_or_default().to_owned())
    }

    fn is_encryption_available(&self) -> Promise<bool> {
        Promise::resolved(true)
    }
}

impl IEncryptionService for TestEncryptionService {
    fn set_use_plain_text_encryption(&self) -> Promise<()> {
        Promise::resolved(())
    }

    fn get_key_storage_provider(&self) -> Promise<KnownStorageProvider> {
        Promise::resolved(KnownStorageProvider::BasicText)
    }
}

/// `TestFailingEncryptionService`: o `decrypt` rejeita e conta as chamadas.
#[derive(Default)]
struct TestFailingEncryptionService {
    base: TestEncryptionService,
    decrypt_calls: Cell<usize>,
}

impl ICommonEncryptionService for TestFailingEncryptionService {
    fn encrypt(&self, value: &str) -> Promise<String> {
        self.base.encrypt(value)
    }

    fn decrypt(&self, _value: &str) -> Promise<String> {
        self.decrypt_calls.set(self.decrypt_calls.get() + 1);
        Promise::rejected(Error::new("Cannot decrypt stale secret"))
    }

    fn is_encryption_available(&self) -> Promise<bool> {
        self.base.is_encryption_available()
    }
}

impl IEncryptionService for TestFailingEncryptionService {
    fn set_use_plain_text_encryption(&self) -> Promise<()> {
        self.base.set_use_plain_text_encryption()
    }

    fn get_key_storage_provider(&self) -> Promise<KnownStorageProvider> {
        self.base.get_key_storage_provider()
    }
}

/// `TestNoEncryptionService`: nada funciona, e a cifra não está disponível.
struct TestNoEncryptionService;

impl ICommonEncryptionService for TestNoEncryptionService {
    fn encrypt(&self, _value: &str) -> Promise<String> {
        errors::throw(Error::new("Method not implemented."))
    }

    fn decrypt(&self, _value: &str) -> Promise<String> {
        errors::throw(Error::new("Method not implemented."))
    }

    fn is_encryption_available(&self) -> Promise<bool> {
        Promise::resolved(false)
    }
}

impl IEncryptionService for TestNoEncryptionService {
    fn set_use_plain_text_encryption(&self) -> Promise<()> {
        errors::throw(Error::new("Method not implemented."))
    }

    fn get_key_storage_provider(&self) -> Promise<KnownStorageProvider> {
        errors::throw(Error::new("Method not implemented."))
    }
}

/// `sandbox.spy(service)`: repassa as chamadas ao serviço e conta `encrypt` e `decrypt`.
struct SpyEncryptionService<E> {
    inner: E,
    encrypt_call_count: Cell<usize>,
    decrypt_call_count: Cell<usize>,
}

impl<E> SpyEncryptionService<E> {
    fn new(inner: E) -> Self {
        Self { inner, encrypt_call_count: Cell::new(0), decrypt_call_count: Cell::new(0) }
    }
}

impl<E: IEncryptionService> ICommonEncryptionService for SpyEncryptionService<E> {
    fn encrypt(&self, value: &str) -> Promise<String> {
        self.encrypt_call_count.set(self.encrypt_call_count.get() + 1);
        self.inner.encrypt(value)
    }

    fn decrypt(&self, value: &str) -> Promise<String> {
        self.decrypt_call_count.set(self.decrypt_call_count.get() + 1);
        self.inner.decrypt(value)
    }

    fn is_encryption_available(&self) -> Promise<bool> {
        self.inner.is_encryption_available()
    }
}

impl<E: IEncryptionService> IEncryptionService for SpyEncryptionService<E> {
    fn set_use_plain_text_encryption(&self) -> Promise<()> {
        self.inner.set_use_plain_text_encryption()
    }

    fn get_key_storage_provider(&self) -> Promise<KnownStorageProvider> {
        self.inner.get_key_storage_provider()
    }
}

struct Fixture<E: IEncryptionService + 'static> {
    guard: LeakGuard,
    service: Rc<BaseSecretStorageService>,
    spy: Rc<SpyEncryptionService<E>>,
}

/// O `setup` de cada suíte: o serviço sobre um `InMemoryStorageService` novo e o serviço de cifra espiado.
fn setup<E: IEncryptionService + 'static>(use_in_memory_storage: bool, encryption_service: E) -> Fixture<E> {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let spy = Rc::new(SpyEncryptionService::new(encryption_service));
    let storage = guard.add_rc(InMemoryStorageService::new());
    let service = guard.add_rc(BaseSecretStorageService::new(use_in_memory_storage, Rc::new(storage), spy.clone(), NullLogService::shared()));
    Fixture { guard, service, spy }
}

/// `store.add(service.onDidChangeSecret(changedKey => ...))`: as chaves que o evento entregou.
fn collect_changed_secrets<E: IEncryptionService + 'static>(fixture: &Fixture<E>) -> Rc<RefCell<Vec<String>>> {
    let changed = Rc::new(RefCell::new(Vec::new()));
    let sink = changed.clone();
    fixture.guard.add(fixture.service.on_did_change_secret().listen(move |changed_key| sink.borrow_mut().push(changed_key.clone())));
    changed
}

#[track_caller]
fn assert_event_fired_for(changed: &RefCell<Vec<String>>, key: &str) {
    let changed = changed.borrow();
    assert!(!changed.is_empty(), "o evento não disparou");
    assert!(changed.iter().all(|changed_key| changed_key == key), "chaves entregues: {changed:?}");
}

// BaseSecretStorageService useInMemoryStorage=true

#[test]
fn in_memory_type() {
    let Fixture { guard: _guard, service, .. } = setup(true, TestEncryptionService);
    run_with_faked_timers(async move {
        assert_eq!(service.type_(), SecretStorageType::Unknown);
        // trigger lazy initialization
        service.set("my-secret", "my-secret-value").await.expect("set");

        assert_eq!(service.type_(), SecretStorageType::InMemory);
    });
}

#[test]
fn in_memory_set_and_get() {
    let Fixture { guard: _guard, service, spy } = setup(true, TestEncryptionService);
    run_with_faked_timers(async move {
        let key = "my-secret";
        let value = "my-secret-value";
        service.set(key, value).await.expect("set");
        let result = service.get(key).await.expect("get");
        assert_eq!(result.as_deref(), Some(value));

        // Additionally ensure the encryptionservice was not used
        assert_eq!(spy.encrypt_call_count.get(), 0);
        assert_eq!(spy.decrypt_call_count.get(), 0);
    });
}

#[test]
fn in_memory_delete() {
    let Fixture { guard: _guard, service, .. } = setup(true, TestEncryptionService);
    run_with_faked_timers(async move {
        let key = "my-secret";
        let value = "my-secret-value";
        service.set(key, value).await.expect("set");
        service.delete(key).await.expect("delete");
        let result = service.get(key).await.expect("get");
        assert_eq!(result, None);
    });
}

#[test]
fn in_memory_on_did_change_secret() {
    let fixture = setup(true, TestEncryptionService);
    let changed = collect_changed_secrets(&fixture);
    let Fixture { guard: _guard, service, .. } = fixture;
    run_with_faked_timers(async move {
        service.set("my-secret", "my-secret-value").await.expect("set");
    });
    assert_event_fired_for(&changed, "my-secret");
}

// BaseSecretStorageService useInMemoryStorage=false

#[test]
fn persisted_type() {
    let Fixture { guard: _guard, service, .. } = setup(false, TestEncryptionService);
    run_with_faked_timers(async move {
        assert_eq!(service.type_(), SecretStorageType::Unknown);
        // trigger lazy initialization
        service.set("my-secret", "my-secret-value").await.expect("set");

        assert_eq!(service.type_(), SecretStorageType::Persisted);
    });
}

#[test]
fn persisted_set_and_get() {
    let Fixture { guard: _guard, service, spy } = setup(false, TestEncryptionService);
    run_with_faked_timers(async move {
        let key = "my-secret";
        let value = "my-secret-value";
        service.set(key, value).await.expect("set");
        let result = service.get(key).await.expect("get");
        assert_eq!(result.as_deref(), Some(value));

        // Additionally ensure the encryptionservice was not used
        assert_eq!(spy.encrypt_call_count.get(), 1);
        assert_eq!(spy.decrypt_call_count.get(), 1);
    });
}

#[test]
fn persisted_delete() {
    let Fixture { guard: _guard, service, .. } = setup(false, TestEncryptionService);
    run_with_faked_timers(async move {
        let key = "my-secret";
        let value = "my-secret-value";
        service.set(key, value).await.expect("set");
        service.delete(key).await.expect("delete");
        let result = service.get(key).await.expect("get");
        assert_eq!(result, None);
    });
}

#[test]
fn persisted_get_removes_stale_persisted_secret_when_decryption_fails() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let key = "my-secret";
    let full_key = secret_storage_key(key);
    let storage_service: Rc<AbstractStorageService> = guard.add_rc(InMemoryStorageService::new());
    let encryption_service = Rc::new(TestFailingEncryptionService::default());
    let failing_service = guard.add_rc(BaseSecretStorageService::new(false, Rc::new(storage_service.clone()), encryption_service.clone(), NullLogService::shared()));

    StorageService::store(&storage_service, &full_key, StorageValue::from("encrypted+my-secret-value"), StorageScope::Application, StorageTarget::Machine);
    run_with_faked_timers(async move {
        assert_eq!(failing_service.get(key).await.expect("get"), None);
        assert_eq!(encryption_service.decrypt_calls.get(), 1);
        assert_eq!(StorageService::get(&storage_service, &full_key, StorageScope::Application, None), None);

        assert_eq!(failing_service.get(key).await.expect("get"), None);
        assert_eq!(encryption_service.decrypt_calls.get(), 1);
    });
}

#[test]
fn persisted_on_did_change_secret() {
    let fixture = setup(false, TestEncryptionService);
    let changed = collect_changed_secrets(&fixture);
    let Fixture { guard: _guard, service, .. } = fixture;
    run_with_faked_timers(async move {
        service.set("my-secret", "my-secret-value").await.expect("set");
    });
    assert_event_fired_for(&changed, "my-secret");
}

// BaseSecretStorageService useInMemoryStorage=false, encryption not available

#[test]
fn no_encryption_type() {
    let Fixture { guard: _guard, service, .. } = setup(false, TestNoEncryptionService);
    run_with_faked_timers(async move {
        assert_eq!(service.type_(), SecretStorageType::Unknown);
        // trigger lazy initialization
        service.set("my-secret", "my-secret-value").await.expect("set");

        assert_eq!(service.type_(), SecretStorageType::InMemory);
    });
}

#[test]
fn no_encryption_set_and_get() {
    let Fixture { guard: _guard, service, spy } = setup(false, TestNoEncryptionService);
    run_with_faked_timers(async move {
        let key = "my-secret";
        let value = "my-secret-value";
        service.set(key, value).await.expect("set");
        let result = service.get(key).await.expect("get");
        assert_eq!(result.as_deref(), Some(value));

        // Additionally ensure the encryptionservice was not used
        assert_eq!(spy.encrypt_call_count.get(), 0);
        assert_eq!(spy.decrypt_call_count.get(), 0);
    });
}

// BaseSecretStorageService cross-app shared secrets

/// O `setup` da suíte: o `TestSharedSecretStorageService`, que sobrescreve `useSharedStorage`.
fn setup_shared() -> Fixture<TestEncryptionService> {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let spy = Rc::new(SpyEncryptionService::new(TestEncryptionService));
    let storage_service = guard.add_rc(InMemoryStorageService::new());
    let service = guard.add_rc(BaseSecretStorageService::with_shared_storage_policy(
        false,
        Rc::new(storage_service),
        spy.clone(),
        NullLogService::shared(),
        Rc::new(|key: &str| CROSS_APP_SHARED_SECRET_KEYS.iter().any(|shared| *shared == key)),
    ));
    Fixture { guard, service, spy }
}

#[test]
fn shared_keys_are_stored_and_read_from_application_shared() {
    let Fixture { guard: _guard, service, .. } = setup_shared();
    run_with_faked_timers(async move {
        let shared_key = CROSS_APP_SHARED_SECRET_KEYS[0];
        let value = "shared-secret-value";
        service.set(shared_key, value).await.expect("set");
        let result = service.get(shared_key).await.expect("get");
        assert_eq!(result.as_deref(), Some(value));

        // Non-shared key should still work via APPLICATION scope
        let regular_key = "regular-secret";
        service.set(regular_key, "regular-value").await.expect("set");
        assert_eq!(service.get(regular_key).await.expect("get").as_deref(), Some("regular-value"));
    });
}

#[test]
fn on_did_change_secret_fires_for_application_shared_changes() {
    let fixture = setup_shared();
    let changed = collect_changed_secrets(&fixture);
    let Fixture { guard: _guard, service, .. } = fixture;
    let shared_key = CROSS_APP_SHARED_SECRET_KEYS[0];
    run_with_faked_timers(async move {
        service.set(shared_key, "value").await.expect("set");
    });
    assert_event_fired_for(&changed, shared_key);
}

#[test]
fn deleting_a_shared_key_removes_it() {
    let Fixture { guard: _guard, service, .. } = setup_shared();
    run_with_faked_timers(async move {
        let shared_key = CROSS_APP_SHARED_SECRET_KEYS[0];
        service.set(shared_key, "value").await.expect("set");
        assert_eq!(service.get(shared_key).await.expect("get").as_deref(), Some("value"));
        service.delete(shared_key).await.expect("delete");
        assert_eq!(service.get(shared_key).await.expect("get"), None);
    });
}
