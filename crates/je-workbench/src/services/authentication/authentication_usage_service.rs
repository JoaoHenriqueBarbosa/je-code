//! Porte de `src/vs/workbench/services/authentication/browser/authenticationUsageService.ts`: o serviço que
//! guarda, por provedor e conta, quais extensões usaram a conta e quando, e que mantém o cache de "esta
//! extensão usa autenticação".
//!
//! Este arquivo também carrega o [`UsageStore`], o miolo que o `AuthenticationMcpUsageService` repete linha
//! a linha no original (mudam só a chave de armazenamento, os nomes dos campos JSON, de qual propriedade do
//! produto vêm os confiáveis, e se a fila e a flag de descarte existem). Aqui o miolo existe uma vez e cada
//! serviço é a casca com os nomes dele ([`UsageKind`] descreve a diferença).
//!
//! Divergências:
//! - **Injeção.** Os parâmetros decorados (`@IStorageService`, `@IAuthenticationService`, `@ILogService`,
//!   `@IProductService`) são a tupla `Deps` do [`Injectable`]. `IAuthenticationUsageService` é o trait
//!   [`IAuthenticationUsageService`]; a classe herda de `Disposable`, aqui um `Disposable` guardado no
//!   campo `base`.
//! - **`registerSingleton(..., InstantiationType.Delayed)`** vira [`register`]; o proxy de instanciação
//!   atrasada é o [`AuthenticationUsageServiceProxy`].
//! - **Funções `async`** são `async_fn` (o corpo roda já, até o primeiro `await`). A `Queue` do original é
//!   `Queue<()>`: `initializeExtensionUsageCache` enfileira o `Promise.all` e o descarta, então o valor
//!   `void[]` dele vira `()`.
//! - **`IAccountUsage.scopes?: string[]`** é `Option<Vec<String>>`, e `addAccountUsage` recebe
//!   `Option<&[String]>` (`ReadonlyArray<string> | undefined`).
//! - **`JSON.parse`.** Um erro de sintaxe ou de leitura dá lista vazia (o `catch` do original). Um JSON
//!   válido que não é array deixa o original lançar `TypeError` ao usar a lista, e aqui lança o mesmo erro
//!   com [`je_base::errors::throw`] (ver [`read_stored_items`]). Das propriedades de cada uso só as quatro
//!   declaradas sobrevivem, escritas em [`UsageRecord::to_json`] na ordem do literal do original
//!   (`id`, `name`, `scopes`, `lastUsed`); um item que não é objeto ou não tem o id textual é descartado,
//!   um nome ausente vira texto vazio e um `lastUsed` que não é número vira `0`.
//! - **`Date.now()`** é [`je_base::runtime::date_now`].

use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::rc::Rc;

use je_base::JsString;
use je_base::async_::{Queue, async_fn};
use je_base::errors;
use je_base::json::{Object, Value};
use je_base::json_formatter::json_stringify;
use je_base::lifecycle::{Disposable, DisposableRef, IDisposable, to_disposable};
use je_base::product::StringsOrDictionary;
use je_base::promise::Promise;
use je_base::runtime::date_now;
use je_instantiation::{InstantiationType, Injectable, delayed_proxy, register_singleton, service_id, service_impl};
use je_platform::log::{LogMessage, LogService};
use je_platform::product::IProductService;
use je_platform::storage::{StorageScope, StorageService, StorageTarget};
use je_storage::StorageValue;

use super::authentication::{IAuthenticationService, all_trusted_auth_access_ids, read_stored_items};

/// O que muda entre o uso por extensão e o uso por MCP no armazenamento.
#[derive(Clone, Copy)]
pub(super) struct UsageKind {
    /// O sufixo da chave de armazenamento depois de `${providerId}-${accountName}`.
    pub(super) key_suffix: &'static str,
    /// A propriedade JSON com o id da entidade (`extensionId`, `mcpServerId`).
    pub(super) id_field: &'static str,
    /// A propriedade JSON com o nome da entidade (`extensionName`, `mcpServerName`).
    pub(super) name_field: &'static str,
}

/// O uso por extensão: `${providerId}-${accountName}-usages`.
const EXTENSION_USAGE: UsageKind = UsageKind { key_suffix: "usages", id_field: "extensionId", name_field: "extensionName" };

/// Um registro de uso, de extensão ou de MCP: o que `IAccountUsage` e `IAuthenticationMcpUsage` têm em comum.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct UsageRecord {
    pub(super) id: String,
    pub(super) name: String,
    pub(super) last_used: f64,
    pub(super) scopes: Option<Vec<String>>,
}

impl UsageRecord {
    /// O objeto do `JSON.parse` como [`UsageRecord`]; `None` se não é objeto ou não tem o id textual.
    fn from_json(kind: &UsageKind, value: &Value) -> Option<Self> {
        let Value::Object(object) = value else { return None };
        let Some(Value::String(id)) = object.get(kind.id_field) else { return None };
        Some(Self {
            id: id.to_string_lossy(),
            name: match object.get(kind.name_field) {
                Some(Value::String(name)) => name.to_string_lossy(),
                _ => String::new(),
            },
            last_used: match object.get("lastUsed") {
                Some(Value::Number(last_used)) => *last_used,
                _ => 0.0,
            },
            scopes: match object.get("scopes") {
                Some(Value::Array(items)) => Some(
                    items
                        .iter()
                        .filter_map(|item| match item {
                            Value::String(scope) => Some(scope.to_string_lossy()),
                            _ => None,
                        })
                        .collect(),
                ),
                _ => None,
            },
        })
    }

    /// O objeto que o `JSON.stringify` escreveria: id, nome, `scopes` (omitido se ausente) e `lastUsed`.
    fn to_json(&self, kind: &UsageKind) -> Value {
        let mut object = Object::new();
        object.insert(JsString::from(kind.id_field), Value::String(JsString::from(self.id.as_str())));
        object.insert(JsString::from(kind.name_field), Value::String(JsString::from(self.name.as_str())));
        if let Some(scopes) = &self.scopes {
            object.insert(
                JsString::from("scopes"),
                Value::Array(scopes.iter().map(|scope| Value::String(JsString::from(scope.as_str()))).collect()),
            );
        }
        object.insert(JsString::from("lastUsed"), Value::Number(self.last_used));
        Value::Object(object)
    }
}

/// O miolo dos serviços de uso: o armazenamento de `readAccountUsages`/`addAccountUsage`/`removeAccountUsage`,
/// o conjunto de entidades que usam autenticação e a fila que o mantém.
pub(super) struct UsageStore {
    kind: UsageKind,
    storage_service: Rc<dyn StorageService>,
    authentication_service: Rc<dyn IAuthenticationService>,
    log_service: Rc<dyn LogService>,
    queue: Rc<Queue<()>>,
    entities_using_auth: RefCell<HashSet<String>>,
    disposed: Cell<bool>,
}

impl UsageStore {
    /// `trusted_access` é o `trustedExtensionAuthAccess`/`trustedMcpAuthAccess` do produto: quem está nele
    /// já conta como quem usa autenticação.
    pub(super) fn new(
        kind: UsageKind,
        storage_service: Rc<dyn StorageService>,
        authentication_service: Rc<dyn IAuthenticationService>,
        log_service: Rc<dyn LogService>,
        trusted_access: Option<StringsOrDictionary>,
    ) -> Rc<Self> {
        Rc::new(Self {
            kind,
            storage_service,
            authentication_service,
            log_service,
            queue: Rc::new(Queue::new()),
            entities_using_auth: RefCell::new(all_trusted_auth_access_ids(trusted_access).into_iter().collect()),
            disposed: Cell::new(false),
        })
    }

    /// A fila, para o serviço que a descarta junto com ele registrá-la.
    pub(super) fn queue(&self) -> Rc<Queue<()>> {
        self.queue.clone()
    }

    /// O `_disposed = true` do serviço que o tem: depois disso o cache não é mais atualizado.
    pub(super) fn mark_disposed(&self) {
        self.disposed.set(true);
    }

    /// A assinatura de `onDidRegisterAuthenticationProvider`: cada provedor novo entra na fila de cache.
    pub(super) fn watch_providers(self: &Rc<Self>) -> DisposableRef {
        let this = self.clone();
        self.authentication_service.on_did_register_authentication_provider().listen(move |provider| {
            let store = this.clone();
            let provider_id = provider.id.clone();
            let _ = this.queue.queue(move || store.add_to_cache(&provider_id));
        })
    }

    fn account_key(&self, provider_id: &str, account_name: &str) -> String {
        format!("{provider_id}-{account_name}-{}", self.kind.key_suffix)
    }

    pub(super) fn initialize_cache(self: &Rc<Self>) -> Promise<()> {
        let this = self.clone();
        async_fn(async move {
            let store = this.clone();
            this.queue
                .queue(move || {
                    let promises: Vec<Promise<()>> =
                        store.authentication_service.get_provider_ids().iter().map(|provider_id| store.add_to_cache(provider_id)).collect();
                    Promise::all(promises).then(|_| ())
                })
                .await?;
            Ok(())
        })
    }

    pub(super) fn has_used(self: &Rc<Self>, entity_id: &str) -> Promise<bool> {
        let this = self.clone();
        let entity_id = entity_id.to_owned();
        async_fn(async move {
            this.queue.when_idle().await?;
            Ok(this.entities_using_auth.borrow().contains(&entity_id))
        })
    }

    pub(super) fn read(&self, provider_id: &str, account_name: &str) -> Vec<UsageRecord> {
        let key = self.account_key(provider_id, account_name);
        // `usages.findIndex` não existe fora de um array.
        let items = read_stored_items(self.storage_service.as_ref(), &key, "usages.findIndex is not a function");
        items.iter().filter_map(|item| UsageRecord::from_json(&self.kind, item)).collect()
    }

    pub(super) fn remove(&self, provider_id: &str, account_name: &str) {
        self.storage_service.remove(&self.account_key(provider_id, account_name), StorageScope::Application);
    }

    pub(super) fn add(&self, provider_id: &str, account_name: &str, scopes: Option<&[String]>, entity_id: &str, entity_name: &str) {
        let mut usages = self.read(provider_id, account_name);

        let usage = UsageRecord {
            id: entity_id.to_owned(),
            name: entity_name.to_owned(),
            scopes: scopes.map(<[String]>::to_vec),
            last_used: date_now() as f64,
        };
        match usages.iter_mut().find(|existing| existing.id == entity_id) {
            Some(existing) => *existing = usage,
            None => usages.push(usage),
        }

        self.storage_service.store(
            &self.account_key(provider_id, account_name),
            StorageValue::String(json_stringify(&Value::Array(usages.iter().map(|usage| usage.to_json(&self.kind)).collect()), None)),
            StorageScope::Application,
            StorageTarget::Machine,
        );
        self.entities_using_auth.borrow_mut().insert(entity_id.to_owned());
    }

    fn add_to_cache(self: &Rc<Self>, provider_id: &str) -> Promise<()> {
        let this = self.clone();
        let provider_id = provider_id.to_owned();
        async_fn(async move {
            if this.disposed.get() {
                return Ok(());
            }
            let cached: Result<(), errors::Error> = async {
                let accounts = this.authentication_service.get_accounts(&provider_id).await?;
                errors::catch(|| {
                    for account in &accounts {
                        for usage in this.read(&provider_id, &account.label) {
                            this.entities_using_auth.borrow_mut().insert(usage.id);
                        }
                    }
                })
            }
            .await;
            if let Err(error) = cached {
                this.log_service.error_args(&LogMessage::from(&error), &[]);
            }
            Ok(())
        })
    }
}

/// `IAccountUsage`.
#[derive(Clone, Debug, PartialEq)]
pub struct IAccountUsage {
    pub extension_id: String,
    pub extension_name: String,
    pub last_used: f64,
    pub scopes: Option<Vec<String>>,
}

impl From<UsageRecord> for IAccountUsage {
    fn from(record: UsageRecord) -> Self {
        Self { extension_id: record.id, extension_name: record.name, last_used: record.last_used, scopes: record.scopes }
    }
}

/// `IAuthenticationUsageService`.
pub trait IAuthenticationUsageService {
    /// Initializes the cache of extensions that use authentication. Ideally used in a contribution that can
    /// be run eventually after the workspace is loaded.
    fn initialize_extension_usage_cache(&self) -> Promise<()>;

    /// Checks if an extension uses authentication.
    ///
    /// - `extension_id`: the id of the extension to check.
    fn extension_uses_auth(&self, extension_id: &str) -> Promise<bool>;

    /// Reads the usages for an account.
    ///
    /// - `provider_id`: the id of the authentication provider to get usages for.
    /// - `account_name`: the name of the account to get usages for.
    fn read_account_usages(&self, provider_id: &str, account_name: &str) -> Vec<IAccountUsage>;

    /// Removes the usages of an account.
    ///
    /// - `provider_id`: the id of the authentication provider to get usages for.
    /// - `account_name`: the name of the account to get usages for.
    fn remove_account_usage(&self, provider_id: &str, account_name: &str);

    /// Adds a usage for an account.
    ///
    /// - `provider_id`: the id of the authentication provider to get usages for.
    /// - `account_name`: the name of the account to get usages for.
    /// - `extension_id`: the id of the extension to add a usage for.
    /// - `extension_name`: the name of the extension to add a usage for.
    fn add_account_usage(&self, provider_id: &str, account_name: &str, scopes: Option<&[String]>, extension_id: &str, extension_name: &str);
}

delayed_proxy!(pub AuthenticationUsageServiceProxy for dyn IAuthenticationUsageService {
    methods {
        fn initialize_extension_usage_cache(&self) -> Promise<()>;
        fn extension_uses_auth(&self, extension_id: &str) -> Promise<bool>;
        fn read_account_usages(&self, provider_id: &str, account_name: &str) -> Vec<IAccountUsage>;
        fn remove_account_usage(&self, provider_id: &str, account_name: &str);
        fn add_account_usage(&self, provider_id: &str, account_name: &str, scopes: Option<&[String]>, extension_id: &str, extension_name: &str);
    }
    events {}
});

service_id!(pub I_AUTHENTICATION_USAGE_SERVICE: dyn IAuthenticationUsageService = "IAuthenticationUsageService", proxy AuthenticationUsageServiceProxy);

/// `AuthenticationUsageService`.
pub struct AuthenticationUsageService {
    base: Rc<Disposable>,
    store: Rc<UsageStore>,
}

impl Injectable for AuthenticationUsageService {
    type Args = ();
    type Deps = (Rc<dyn StorageService>, Rc<dyn IAuthenticationService>, Rc<dyn LogService>, Rc<dyn IProductService>);

    fn create(_: (), (storage_service, authentication_service, log_service, product_service): Self::Deps) -> Rc<Self> {
        let base = Disposable::new();
        // If an extension is listed in `trustedExtensionAuthAccess` we should consider it as using auth
        let store = UsageStore::new(EXTENSION_USAGE, storage_service, authentication_service, log_service, product_service.trusted_extension_auth_access());
        base.register(store.queue());
        let disposing = store.clone();
        base.register(to_disposable(move || disposing.mark_disposed()));
        base.register(store.watch_providers());
        Rc::new(Self { base, store })
    }
}

service_impl!(AuthenticationUsageService => dyn IAuthenticationUsageService, disposable);

/// `registerSingleton(IAuthenticationUsageService, AuthenticationUsageService, InstantiationType.Delayed)`.
pub fn register() {
    register_singleton::<AuthenticationUsageService>(I_AUTHENTICATION_USAGE_SERVICE, InstantiationType::Delayed);
}

impl IAuthenticationUsageService for AuthenticationUsageService {
    fn initialize_extension_usage_cache(&self) -> Promise<()> {
        self.store.initialize_cache()
    }

    fn extension_uses_auth(&self, extension_id: &str) -> Promise<bool> {
        self.store.has_used(extension_id)
    }

    fn read_account_usages(&self, provider_id: &str, account_name: &str) -> Vec<IAccountUsage> {
        self.store.read(provider_id, account_name).into_iter().map(IAccountUsage::from).collect()
    }

    fn remove_account_usage(&self, provider_id: &str, account_name: &str) {
        self.store.remove(provider_id, account_name);
    }

    fn add_account_usage(&self, provider_id: &str, account_name: &str, scopes: Option<&[String]>, extension_id: &str, extension_name: &str) {
        self.store.add(provider_id, account_name, scopes, extension_id, extension_name);
    }
}

impl IDisposable for AuthenticationUsageService {
    fn dispose(&self) {
        self.base.dispose();
    }
}
