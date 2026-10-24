//! Porte de `src/vs/platform/userDataSync/common/userDataSyncStoreService.ts`: o servidor do Settings Sync
//! (`UserDataSyncStoreManagementService`), o cliente HTTP dele (`UserDataSyncStoreClient`), o serviço
//! (`UserDataSyncStoreService`) e o limitador de requisições por sessão (`RequestsSession`).
//!
//! Decisões de tradução:
//! - `UserDataSyncStoreService extends UserDataSyncStoreClient` só acrescenta o construtor que lê a URL do
//!   serviço de gerenciamento e a acompanha; é [`UserDataSyncStoreService`], um apelido de
//!   [`UserDataSyncStoreClient`] com o construtor [`UserDataSyncStoreClient::new_service`].
//! - `AbstractUserDataSyncStoreManagementService` e `UserDataSyncStoreManagementService` são uma struct só (a
//!   abstrata não tem outra subclasse no porte). O `IConfigurationService` que o construtor do original recebe
//!   e não usa não é pedido. `isWeb` é falso: não há `web` no produto e o alvo de `sync.store.url.type` é
//!   `Machine`.
//! - `productService['configurationSync.store']` é o [`ConfigurationSyncStore`] do produto; o que o original
//!   guarda em `sync.previous.store` (o `JSON.stringify` do objeto do produto) sai com as chaves na ordem
//!   `url`, `stableUrl`, `insidersUrl`, `canSwitch`, `authenticationProviders`. Um tipo guardado que não é
//!   `insiders` nem `stable` vale o tipo padrão (o original cairia na URL padrão sem tipo reconhecível).
//! - `headers['etag']` e `headers['retry-after']` usam o [`read_header`] da requisição (mesma chave, e também a
//!   forma minúscula). `parseInt(retryAfter)` não numérico, que o original transforma numa data inválida, é
//!   tratado como cabeçalho ausente (`TooManyRequests`), como o `retry_after_from_headers` do serviço de
//!   requisição.
//! - Os métodos `async` do original viram [`Promise`], e `throw` dentro deles vira a promessa rejeitada. Os
//!   erros do servidor são [`UserDataSyncStoreError`], convertidos em [`Error`] pelo `name` que o original compõe.

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use indexmap::IndexMap;
use je_base::JsString;
use je_base::async_::{async_fn, create_cancelable_promise, timeout_with_token};
use je_base::buffer::VSBufferReadableStream;
use je_base::cancellation::CancellationToken;
use je_base::dynamic::{Dynamic, number_to_js_string};
use je_base::errors::{Error, is_cancellation_error};
use je_base::event::{Emitter, Event};
use je_base::json::{Object, Value};
use je_base::lifecycle::{Disposable, IDisposable, to_disposable};
use je_base::parts::request::{HeaderValue, IHeaders, IRequestContext, IRequestOptions};
use je_base::product::{ConfigurationSyncStore, IAuthenticationProviderScopes};
use je_base::promise::{CancelablePromise, Promise};
use je_base::resources::ExtUri;
use je_base::runtime::date_now;
use je_base::uri::{Field, URI, UriChange};
use je_base::uuid::generate_uuid;
use je_storage::StorageValue;

use super::abstract_synchronizer::parse_json;
use super::user_data_sync::{
    HEADER_EXECUTION_ID, HEADER_OPERATION_ID, IAuthenticationProvider, IResourceRefHandle, IUserData, IUserDataCollectionManifest, IUserDataCollectionManifestEntry, IUserDataManifest,
    IUserDataResourceManifest, IUserDataSyncLatestCollection, IUserDataSyncLatestData, IUserDataSyncLogService, IUserDataSyncStore, IUserDataSyncStoreManagementService, IUserDataSyncStoreService,
    SYNC_SERVICE_URL_TYPE, ServerResource, UserDataSyncErrorCode, UserDataSyncStoreError, UserDataSyncStoreType,
};
use crate::environment::environment::EnvironmentService;
use crate::external_services::service_machine_id::get_service_machine_id;
use crate::files::file_service::FileService;
use crate::log::{LogArg};
use crate::product::IProductService;
use crate::request::{IRequestService, as_json, as_text, as_text_or_error, has_no_content, is_success, read_header, retry_after_from_headers};
use crate::resources::{host_environment, join_path};
use crate::storage::{StorageScope, StorageService, StorageTarget};

const SYNC_PREVIOUS_STORE: &str = "sync.previous.store";
const DONOT_MAKE_REQUESTS_UNTIL_KEY: &str = "sync.donot-make-requests-until";
const USER_SESSION_ID_KEY: &str = "sync.user-session-id";
const MACHINE_SESSION_ID_KEY: &str = "sync.machine-session-id";
const REQUEST_SESSION_LIMIT: usize = 100;
const REQUEST_SESSION_INTERVAL: u64 = 1000 * 60 * 5; /* 5 minutes */

// region: JSON

fn json_string(text: &str) -> Value {
    Value::String(JsString::from(text))
}

fn string_field(object: &Object, name: &str) -> Option<String> {
    match object.get(name) {
        Some(Value::String(text)) => Some(text.to_string_lossy()),
        _ => None,
    }
}

/// `JSON.stringify(configurationSyncStore)`.
fn configuration_sync_store_to_json(store: &ConfigurationSyncStore) -> Value {
    let mut object = Object::new();
    object.insert(JsString::from("url"), json_string(&store.url));
    object.insert(JsString::from("stableUrl"), json_string(&store.stable_url));
    object.insert(JsString::from("insidersUrl"), json_string(&store.insiders_url));
    if let Some(can_switch) = store.can_switch {
        object.insert(JsString::from("canSwitch"), Value::Bool(can_switch));
    }
    let mut providers = Object::new();
    for (id, provider) in &store.authentication_providers {
        let mut entry = Object::new();
        entry.insert(JsString::from("scopes"), Value::Array(provider.scopes.iter().map(|scope| json_string(scope)).collect()));
        providers.insert(JsString::from(id.as_str()), Value::Object(entry));
    }
    object.insert(JsString::from("authenticationProviders"), Value::Object(providers));
    Value::Object(object)
}

/// O `JSON.parse` de um `ConfigurationSyncStore`, com a validação de `toUserDataSyncStore`: `url` texto,
/// `authenticationProviders` objeto e `scopes` de cada um um array. `None` se não passa.
fn configuration_sync_store_from_json(value: &Value) -> Option<ConfigurationSyncStore> {
    let Value::Object(object) = value else { return None };
    let Some(Value::Object(providers)) = object.get("authenticationProviders") else { return None };
    let mut authentication_providers = IndexMap::new();
    for (id, provider) in providers.iter() {
        let Value::Object(provider) = provider else { return None };
        let Some(Value::Array(scopes)) = provider.get("scopes") else { return None };
        let scopes = scopes
            .iter()
            .filter_map(|scope| match scope {
                Value::String(text) => Some(text.to_string_lossy()),
                _ => None,
            })
            .collect();
        authentication_providers.insert(id.to_string_lossy(), IAuthenticationProviderScopes { scopes, extra: Default::default() });
    }
    Some(ConfigurationSyncStore {
        url: string_field(object, "url")?,
        insiders_url: string_field(object, "insidersUrl")?,
        stable_url: string_field(object, "stableUrl")?,
        authentication_providers,
        can_switch: match object.get("canSwitch") {
            Some(Value::Bool(flag)) => Some(*flag),
            _ => None,
        },
        extra: Default::default(),
    })
}

fn parse_resource_manifest(value: Option<&Value>) -> Option<IUserDataResourceManifest> {
    let Some(Value::Object(object)) = value else { return None };
    Some(
        object
            .iter()
            .filter_map(|(key, value)| match value {
                Value::String(text) => Some((key.to_string_lossy(), text.to_string_lossy())),
                _ => None,
            })
            .collect(),
    )
}

/// `{ ...JSON.parse(content), ref }`.
fn parse_manifest(content: &str, r#ref: String) -> Result<IUserDataManifest, Error> {
    let value = parse_json(content)?;
    let Value::Object(object) = &value else { return Ok(IUserDataManifest { latest: None, session: String::new(), r#ref, collections: None }) };
    let collections: Option<IUserDataCollectionManifest> = match object.get("collections") {
        Some(Value::Object(collections)) => Some(
            collections
                .iter()
                .map(|(id, collection)| {
                    let latest = match collection {
                        Value::Object(collection) => parse_resource_manifest(collection.get("latest")),
                        _ => None,
                    };
                    (id.to_string_lossy(), IUserDataCollectionManifestEntry { latest })
                })
                .collect(),
        ),
        _ => None,
    };
    Ok(IUserDataManifest { latest: parse_resource_manifest(object.get("latest")), session: string_field(object, "session").unwrap_or_default(), r#ref, collections })
}

/// O `{ content, ref }` do primeiro item de `[IUserData]` que `download/latest` devolve.
fn user_data_of(items: &Dynamic) -> IUserData {
    let first = match items {
        Dynamic::Array(items) => items.get(0),
        _ => Dynamic::Undefined,
    };
    let text = |name: &str| match first.get_str(name) {
        Dynamic::String(text) => Some(text.to_string_lossy()),
        _ => None,
    };
    IUserData { r#ref: text("ref").unwrap_or_default(), content: text("content") }
}

fn user_data_resources(resources: &Dynamic) -> IndexMap<String, IUserData> {
    resources.own_entries().into_iter().map(|(resource, items)| (resource.to_string_lossy(), user_data_of(&items))).collect()
}

fn set_header(headers: &mut IHeaders, name: &str, value: &str) {
    headers.insert(name.to_string(), HeaderValue::One(value.to_string()));
}

fn header_dynamic(value: &HeaderValue) -> Dynamic {
    match value {
        HeaderValue::One(text) => Dynamic::string(text),
        HeaderValue::Many(items) => Dynamic::array(items.iter().map(|item| Dynamic::string(item))),
    }
}

fn optional_dynamic(value: Option<&str>) -> Dynamic {
    value.map_or(Dynamic::Undefined, Dynamic::string)
}

fn store_error(message: &str, url: &str, code: UserDataSyncErrorCode, server_code: Option<f64>, operation_id: Option<String>) -> Error {
    UserDataSyncStoreError::new(message, url, code, server_code, operation_id).into_error()
}

// endregion

// region: gerenciamento do servidor

/// `UserDataSyncStoreManagementService`.
pub struct UserDataSyncStoreManagementService {
    base: Rc<Disposable>,
    product_service: Rc<dyn IProductService>,
    storage_service: Rc<dyn StorageService>,
    on_did_change_user_data_sync_store: Rc<Emitter<()>>,
    user_data_sync_store: RefCell<Option<IUserDataSyncStore>>,
    previous_configuration_sync_store: Option<ConfigurationSyncStore>,
}

impl UserDataSyncStoreManagementService {
    pub fn new(product_service: Rc<dyn IProductService>, storage_service: Rc<dyn StorageService>) -> Rc<Self> {
        let base = Disposable::new();
        let on_did_change_user_data_sync_store = Emitter::new();
        base.register(on_did_change_user_data_sync_store.clone());

        let previous_configuration_sync_store = storage_service
            .get(SYNC_PREVIOUS_STORE, StorageScope::Application, None)
            .filter(|previous| !previous.is_empty())
            .and_then(|previous| parse_json(&previous).ok())
            .and_then(|previous| configuration_sync_store_from_json(&previous));

        let this = Rc::new(Self { base, product_service, storage_service, on_did_change_user_data_sync_store, user_data_sync_store: RefCell::new(None), previous_configuration_sync_store });
        this.update_user_data_sync_store();
        let store = je_base::lifecycle::DisposableStore::new();
        this.base.register(store.clone());
        let weak = Rc::downgrade(&this);
        let listener = this
            .storage_service
            .on_did_change_value(StorageScope::Application, Some(SYNC_SERVICE_URL_TYPE), &store)
            .filter(
                {
                    let weak = weak.clone();
                    move |_| weak.upgrade().is_some_and(|this| this.user_data_sync_store_type().as_deref() != this.user_data_sync_store.borrow().as_ref().map(|store| store.r#type.as_str()))
                },
                Some(&*store),
            )
            .listen(move |_| {
                if let Some(this) = weak.upgrade() {
                    this.update_user_data_sync_store();
                }
            });
        store.add(listener);

        match this.product_service.configuration_sync_store() {
            Some(sync_store) => {
                let json = je_base::json_formatter::json_stringify(&configuration_sync_store_to_json(sync_store), None);
                this.storage_service.store(SYNC_PREVIOUS_STORE, StorageValue::from(json.as_str()), StorageScope::Application, StorageTarget::Machine);
            }
            None => this.storage_service.remove(SYNC_PREVIOUS_STORE, StorageScope::Application),
        }
        this
    }

    fn user_data_sync_store_type(&self) -> Option<String> {
        self.storage_service.get(SYNC_SERVICE_URL_TYPE, StorageScope::Application, None)
    }

    fn update_user_data_sync_store(&self) {
        let store = self.to_user_data_sync_store(self.product_service.configuration_sync_store());
        *self.user_data_sync_store.borrow_mut() = store;
        self.on_did_change_user_data_sync_store.fire(());
    }

    fn to_user_data_sync_store(&self, configuration_sync_store: Option<&ConfigurationSyncStore>) -> Option<IUserDataSyncStore> {
        let sync_store = configuration_sync_store?;
        let can_switch = sync_store.can_switch.unwrap_or(false);
        let default_type = if sync_store.url == sync_store.insiders_url { UserDataSyncStoreType::Insiders } else { UserDataSyncStoreType::Stable };
        let stored_type = if can_switch {
            match self.user_data_sync_store_type().as_deref() {
                Some("insiders") => Some(UserDataSyncStoreType::Insiders),
                Some("stable") => Some(UserDataSyncStoreType::Stable),
                _ => None,
            }
        } else {
            None
        };
        let r#type = stored_type.unwrap_or(default_type);
        let url = match r#type {
            UserDataSyncStoreType::Insiders => &sync_store.insiders_url,
            UserDataSyncStoreType::Stable => &sync_store.stable_url,
        };
        Some(IUserDataSyncStore {
            url: URI::parse(url.as_str(), false),
            r#type,
            default_url: URI::parse(sync_store.url.as_str(), false),
            stable_url: URI::parse(sync_store.stable_url.as_str(), false),
            insiders_url: URI::parse(sync_store.insiders_url.as_str(), false),
            can_switch,
            authentication_providers: sync_store.authentication_providers.iter().map(|(id, provider)| IAuthenticationProvider { id: id.clone(), scopes: provider.scopes.clone() }).collect(),
        })
    }
}

impl IDisposable for UserDataSyncStoreManagementService {
    fn dispose(&self) {
        self.base.dispose();
    }
}

impl IUserDataSyncStoreManagementService for UserDataSyncStoreManagementService {
    fn on_did_change_user_data_sync_store(&self) -> Event<()> {
        self.on_did_change_user_data_sync_store.event()
    }

    fn user_data_sync_store(&self) -> Option<IUserDataSyncStore> {
        self.user_data_sync_store.borrow().clone()
    }

    fn switch(&self, r#type: UserDataSyncStoreType) -> Promise<()> {
        if self.user_data_sync_store_type().as_deref() != Some(r#type.as_str()) {
            self.storage_service.store(SYNC_SERVICE_URL_TYPE, StorageValue::from(r#type.as_str()), StorageScope::Application, StorageTarget::Machine);
            self.update_user_data_sync_store();
        }
        Promise::resolved(())
    }

    fn get_previous_user_data_sync_store(&self) -> Promise<Option<IUserDataSyncStore>> {
        Promise::resolved(self.to_user_data_sync_store(self.previous_configuration_sync_store.as_ref()))
    }
}

// endregion

// region: limite de requisições

#[derive(Default)]
struct SessionState {
    requests: Vec<String>,
    start_time: Option<f64>,
}

/// `RequestsSession`: limita as requisições feitas numa janela de tempo.
pub struct RequestsSession {
    limit: usize,
    /// Em milissegundos.
    interval: u64,
    request_service: Rc<dyn IRequestService>,
    log_service: IUserDataSyncLogService,
    state: RefCell<SessionState>,
}

impl RequestsSession {
    pub fn new(limit: usize, interval: u64, request_service: Rc<dyn IRequestService>, log_service: IUserDataSyncLogService) -> Self {
        Self { limit, interval, request_service, log_service, state: RefCell::new(SessionState::default()) }
    }

    /// `request`: o que o original lança de forma síncrona (o limite estourado) é o `Err`.
    pub fn request(&self, url: &str, mut options: IRequestOptions, token: CancellationToken) -> Result<Promise<IRequestContext>, Error> {
        if self.is_expired() {
            self.reset();
        }

        options.url = Some(url.to_string());

        if self.state.borrow().requests.len() >= self.limit {
            let requests: Vec<LogArg> = self.state.borrow().requests.iter().map(|request| LogArg::from(request.as_str())).collect();
            self.log_service.info_args("Too many requests", &requests);
            return Err(store_error(
                &format!("Too many requests. Only {} requests allowed in {} minutes.", self.limit, number_to_js_string(self.interval as f64 / (1000.0 * 60.0))),
                url,
                UserDataSyncErrorCode::LocalTooManyRequests,
                None,
                None,
            ));
        }

        {
            let mut state = self.state.borrow_mut();
            state.start_time = state.start_time.or(Some(date_now() as f64));
            state.requests.push(url.to_string());
        }

        Ok(self.request_service.request(options, token))
    }

    fn is_expired(&self) -> bool {
        self.state.borrow().start_time.is_some_and(|start_time| date_now() as f64 - start_time > self.interval as f64)
    }

    fn reset(&self) {
        *self.state.borrow_mut() = SessionState::default();
    }
}

// endregion

// region: cliente

struct AuthToken {
    token: String,
    r#type: String,
}

/// Os serviços que o `UserDataSyncStoreClient` recebe por injeção (`@IProductService` e companhia).
#[derive(Clone)]
pub struct UserDataSyncStoreClientServices {
    pub product_service: Rc<dyn IProductService>,
    pub request_service: Rc<dyn IRequestService>,
    pub log_service: IUserDataSyncLogService,
    pub environment_service: Rc<dyn EnvironmentService>,
    pub file_service: Rc<FileService>,
    pub storage_service: Rc<dyn StorageService>,
}

/// `UserDataSyncStoreClient`.
pub struct UserDataSyncStoreClient {
    me: Weak<Self>,
    base: Rc<Disposable>,
    user_data_sync_store_url: RefCell<Option<URI>>,
    auth_token: RefCell<Option<AuthToken>>,
    common_headers: Promise<IHeaders>,
    session: RequestsSession,
    log_service: IUserDataSyncLogService,
    storage_service: Rc<dyn StorageService>,
    on_token_failed: Rc<Emitter<UserDataSyncErrorCode>>,
    on_token_succeed: Rc<Emitter<()>>,
    /// `Date` em milissegundos desde a época.
    donot_make_requests_until: Cell<Option<f64>>,
    on_did_change_donot_make_requests_until: Rc<Emitter<()>>,
    reset_donot_make_requests_until_promise: RefCell<Option<CancelablePromise<()>>>,
}

/// `UserDataSyncStoreService`: o cliente que acompanha a URL do serviço de gerenciamento.
pub type UserDataSyncStoreService = UserDataSyncStoreClient;

fn ext_uri() -> ExtUri {
    ExtUri::unbiased(host_environment())
}

impl UserDataSyncStoreClient {
    pub fn new(user_data_sync_store_url: Option<&URI>, services: UserDataSyncStoreClientServices) -> Rc<Self> {
        let UserDataSyncStoreClientServices { product_service, request_service, log_service, environment_service, file_service, storage_service } = services;
        let base = Disposable::new();
        let on_token_failed = Emitter::new();
        base.register(on_token_failed.clone());
        let on_token_succeed = Emitter::new();
        base.register(on_token_succeed.clone());
        let on_did_change_donot_make_requests_until = Emitter::new();
        base.register(on_did_change_donot_make_requests_until.clone());

        let (application_name, version, commit) = (product_service.application_name().to_string(), product_service.version().to_string(), product_service.commit().map(str::to_string));
        let common_headers = get_service_machine_id(environment_service.as_ref(), &file_service, Some(storage_service.clone())).then(move |_uuid| {
            let mut headers = IHeaders::new();
            set_header(&mut headers, "X-Client-Name", &application_name);
            set_header(&mut headers, "X-Client-Version", &version);
            if let Some(commit) = commit.as_deref().filter(|commit| !commit.is_empty()) {
                set_header(&mut headers, "X-Client-Commit", commit);
            }
            headers
        });

        /* A requests session that limits requests per sessions */
        let session = RequestsSession::new(REQUEST_SESSION_LIMIT, REQUEST_SESSION_INTERVAL, request_service, log_service.clone());
        let this = Rc::new_cyclic(|me| Self {
            me: me.clone(),
            base,
            user_data_sync_store_url: RefCell::new(None),
            auth_token: RefCell::new(None),
            common_headers,
            session,
            log_service,
            storage_service,
            on_token_failed,
            on_token_succeed,
            donot_make_requests_until: Cell::new(None),
            on_did_change_donot_make_requests_until,
            reset_donot_make_requests_until_promise: RefCell::new(None),
        });
        this.update_user_data_sync_store_url(user_data_sync_store_url);
        this.init_donot_make_requests_until();
        let weak = Rc::downgrade(&this);
        this.base.register(to_disposable(move || {
            let promise = weak.upgrade().and_then(|this| this.reset_donot_make_requests_until_promise.borrow_mut().take());
            if let Some(promise) = promise {
                promise.cancel();
            }
        }));
        this
    }

    /// `new UserDataSyncStoreService(...)`: o cliente com a URL do serviço de gerenciamento, que acompanha as
    /// mudanças dela.
    pub fn new_service(user_data_sync_store_management_service: Rc<dyn IUserDataSyncStoreManagementService>, services: UserDataSyncStoreClientServices) -> Rc<Self> {
        let this = Self::new(user_data_sync_store_management_service.user_data_sync_store().map(|store| store.url).as_ref(), services);
        let weak = Rc::downgrade(&this);
        let management_service = user_data_sync_store_management_service.clone();
        this.base.register(user_data_sync_store_management_service.on_did_change_user_data_sync_store().listen(move |_| {
            if let Some(this) = weak.upgrade() {
                this.update_user_data_sync_store_url(management_service.user_data_sync_store().map(|store| store.url).as_ref());
            }
        }));
        this
    }

    fn rc(&self) -> Rc<Self> {
        self.me.upgrade().expect("o cliente está vivo enquanto é usado")
    }

    fn update_user_data_sync_store_url(&self, user_data_sync_store_url: Option<&URI>) {
        *self.user_data_sync_store_url.borrow_mut() = user_data_sync_store_url.map(|url| join_path(url, &["v1"]));
    }

    fn store_url(&self) -> Result<URI, Error> {
        self.user_data_sync_store_url.borrow().clone().ok_or_else(|| Error::new("No settings sync store url configured."))
    }

    fn init_donot_make_requests_until(&self) {
        let donot_make_requests_until = self.storage_service.get_number(DONOT_MAKE_REQUESTS_UNTIL_KEY, StorageScope::Application, None);
        if let Some(until) = donot_make_requests_until.filter(|until| *until != 0.0 && (date_now() as f64) < *until) {
            self.set_donot_make_requests_until(Some(until));
        }
    }

    fn set_donot_make_requests_until(&self, donot_make_requests_until: Option<f64>) {
        if self.donot_make_requests_until.get() == donot_make_requests_until {
            return;
        }
        self.donot_make_requests_until.set(donot_make_requests_until);

        let previous = self.reset_donot_make_requests_until_promise.borrow_mut().take();
        if let Some(promise) = previous {
            promise.cancel();
        }

        match donot_make_requests_until {
            Some(until) => {
                self.storage_service.store(DONOT_MAKE_REQUESTS_UNTIL_KEY, StorageValue::from(until), StorageScope::Application, StorageTarget::Machine);
                let this = self.rc();
                let promise = create_cancelable_promise(move |token| {
                    let delay = (until - date_now() as f64).max(0.0) as u64;
                    timeout_with_token(delay, &token).then(move |()| this.set_donot_make_requests_until(None))
                });
                // `.then(null, e => null)`: o cancelamento é ignorado
                let _ = promise.then_result(|_| Ok(()));
                *self.reset_donot_make_requests_until_promise.borrow_mut() = Some(promise);
            }
            None => self.storage_service.remove(DONOT_MAKE_REQUESTS_UNTIL_KEY, StorageScope::Application),
        }

        self.on_did_change_donot_make_requests_until.fire(());
    }

    fn get_resource_url(user_data_sync_store_url: &URI, collection: Option<&str>, resource: ServerResource) -> URI {
        match collection.filter(|collection| !collection.is_empty()) {
            Some(collection) => join_path(user_data_sync_store_url, &["collection", collection, "resource", resource.as_str()]),
            None => join_path(user_data_sync_store_url, &["resource", resource.as_str()]),
        }
    }

    fn clear_session(&self) {
        self.storage_service.remove(USER_SESSION_ID_KEY, StorageScope::Application);
        self.storage_service.remove(MACHINE_SESSION_ID_KEY, StorageScope::Application);
    }

    fn add_session_headers(&self, headers: &mut IHeaders) {
        let machine_session_id = match self.storage_service.get(MACHINE_SESSION_ID_KEY, StorageScope::Application, None) {
            Some(machine_session_id) => machine_session_id,
            None => {
                let machine_session_id = generate_uuid();
                self.storage_service.store(MACHINE_SESSION_ID_KEY, StorageValue::from(machine_session_id.as_str()), StorageScope::Application, StorageTarget::Machine);
                machine_session_id
            }
        };
        set_header(headers, "X-Machine-Session-Id", &machine_session_id);

        if let Some(user_session_id) = self.storage_service.get(USER_SESSION_ID_KEY, StorageScope::Application, None) {
            set_header(headers, "X-User-Session-Id", &user_session_id);
        }
    }

    /// Monta as opções e faz a requisição (ver [`Self::request`]).
    fn send(&self, url: &URI, r#type: &str, headers: IHeaders, data: Option<String>, call_site: &str, success_codes: &'static [u16]) -> Promise<IRequestContext> {
        let options = IRequestOptions { r#type: Some(r#type.to_string()), headers: Some(headers), data, call_site: call_site.to_string(), ..IRequestOptions::default() };
        self.request(url.to_string(false).to_string_lossy(), options, success_codes, CancellationToken::None)
    }

    /// A falha de rede (não do servidor) vira um [`UserDataSyncStoreError`] com o código que a mensagem indica.
    fn to_request_error(&self, error: Error, url: &str) -> Error {
        let error = if UserDataSyncStoreError::from_error(&error).is_none() {
            let message = error.message.to_lowercase();
            let code = if message.contains("xhr timeout") {
                UserDataSyncErrorCode::RequestTimeout
            } else if message.contains("protocol") && message.contains("not supported") {
                UserDataSyncErrorCode::RequestProtocolNotSupported
            } else if message.contains("request path contains unescaped characters") {
                UserDataSyncErrorCode::RequestPathNotEscaped
            } else if message.contains("headers must be an object") {
                UserDataSyncErrorCode::RequestHeadersNotObject
            } else if is_cancellation_error(&error) {
                UserDataSyncErrorCode::RequestCanceled
            } else {
                UserDataSyncErrorCode::RequestFailed
            };
            store_error(&format!("Connection refused for the request '{url}'."), url, code, None, None)
        } else {
            error
        };
        self.log_service.info_args("Request failed", &[LogArg::from(url)]);
        error
    }

    fn request(&self, url: String, mut options: IRequestOptions, success_codes: &'static [u16], token: CancellationToken) -> Promise<IRequestContext> {
        let this = self.rc();
        async_fn(async move {
            let type_name = options.r#type.clone().unwrap_or_else(|| "undefined".to_string());
            let auth_token = this.auth_token.borrow().as_ref().map(|auth_token| (auth_token.token.clone(), auth_token.r#type.clone()));
            let Some((auth_token, auth_type)) = auth_token else {
                return Err(store_error("No Auth Token Available", &url, UserDataSyncErrorCode::Unauthorized, None, None));
            };

            if this.donot_make_requests_until.get().is_some_and(|until| (date_now() as f64) < until) {
                return Err(store_error(
                    &format!("{type_name} request '{url}' failed because of too many requests (429)."),
                    &url,
                    UserDataSyncErrorCode::TooManyRequestsAndRetryAfter,
                    None,
                    None,
                ));
            }
            this.set_donot_make_requests_until(None);

            let common_headers = this.common_headers.clone().await?;
            let mut headers = options.headers.take().unwrap_or_default();
            headers.extend(common_headers);
            set_header(&mut headers, "X-Account-Type", &auth_type);
            set_header(&mut headers, "authorization", &format!("Bearer {auth_token}"));

            // Add session headers
            this.add_session_headers(&mut headers);
            options.headers = Some(headers.clone());

            let logged_headers = Dynamic::object(headers.iter().map(|(name, value)| (name.as_str(), if name == "authorization" { Dynamic::Undefined } else { header_dynamic(value) })));
            this.log_service.trace_args(
                "Sending request to server",
                &[LogArg::from(Dynamic::object([("url", Dynamic::string(&url)), ("type", Dynamic::string(&type_name)), ("headers", logged_headers)]))],
            );

            let sent = match this.session.request(&url, options, token) {
                Ok(promise) => promise.await,
                Err(error) => Err(error),
            };
            let context = match sent {
                Ok(context) => context,
                Err(error) => return Err(this.to_request_error(error, &url)),
            };

            let operation_id = read_header(Some(&context.res.headers), HEADER_OPERATION_ID);
            let status = context.res.status_code;
            let status_dynamic = status.map_or(Dynamic::Undefined, |code| Dynamic::Number(f64::from(code)));
            let request_info = Dynamic::object([
                ("url", Dynamic::string(&url)),
                ("status", status_dynamic),
                ("execution-id", headers.get(HEADER_EXECUTION_ID).map_or(Dynamic::Undefined, header_dynamic)),
                ("operation-id", optional_dynamic(operation_id.as_deref())),
            ]);
            let succeeded = is_success(&context) || status.is_some_and(|code| success_codes.contains(&code));
            let mut failure_message = String::new();
            if succeeded {
                this.log_service.trace_args("Request succeeded", &[LogArg::from(request_info)]);
            } else {
                failure_message = as_text(&context).await?.unwrap_or_default();
                this.log_service.info_args("Request failed", &[LogArg::from(request_info), LogArg::from(failure_message.as_str())]);
            }

            let fail = |message: String, code: UserDataSyncErrorCode| store_error(&message, &url, code, status.map(f64::from), operation_id.clone());

            if status == Some(401) || status == Some(403) {
                *this.auth_token.borrow_mut() = None;
                if status == Some(401) {
                    this.on_token_failed.fire(UserDataSyncErrorCode::Unauthorized);
                    return Err(fail(format!("{type_name} request '{url}' failed because of Unauthorized (401)."), UserDataSyncErrorCode::Unauthorized));
                }
                this.on_token_failed.fire(UserDataSyncErrorCode::Forbidden);
                return Err(fail(format!("{type_name} request '{url}' failed because the access is forbidden (403)."), UserDataSyncErrorCode::Forbidden));
            }

            this.on_token_succeed.fire(());

            match status {
                Some(404) => return Err(fail(format!("{type_name} request '{url}' failed because the requested resource is not found (404)."), UserDataSyncErrorCode::NotFound)),
                Some(405) => {
                    return Err(fail(format!("{type_name} request '{url}' failed because the requested endpoint is not found (405). {failure_message}"), UserDataSyncErrorCode::MethodNotFound));
                }
                Some(409) => {
                    return Err(fail(
                        format!("{type_name} request '{url}' failed because of Conflict (409). There is new data for this resource. Make the request again with latest data."),
                        UserDataSyncErrorCode::Conflict,
                    ));
                }
                Some(410) => return Err(fail(format!("{type_name} request '{url}' failed because the requested resource is not longer available (410)."), UserDataSyncErrorCode::Gone)),
                Some(412) => {
                    return Err(fail(
                        format!("{type_name} request '{url}' failed because of Precondition Failed (412). There is new data for this resource. Make the request again with latest data."),
                        UserDataSyncErrorCode::PreconditionFailed,
                    ));
                }
                Some(413) => return Err(fail(format!("{type_name} request '{url}' failed because of too large payload (413)."), UserDataSyncErrorCode::TooLarge)),
                Some(426) => {
                    return Err(fail(
                        format!("{type_name} request '{url}' failed with status Upgrade Required (426). Please upgrade the client and try again."),
                        UserDataSyncErrorCode::UpgradeRequired,
                    ));
                }
                Some(429) => {
                    let message = format!("{type_name} request '{url}' failed because of too many requests (429).");
                    return match retry_after_from_headers(Some(&context.res.headers)) {
                        Some(retry_after) => {
                            this.set_donot_make_requests_until(Some(date_now() as f64 + retry_after as f64 * 1000.0));
                            Err(fail(message, UserDataSyncErrorCode::TooManyRequestsAndRetryAfter))
                        }
                        None => Err(fail(message, UserDataSyncErrorCode::TooManyRequests)),
                    };
                }
                _ => {}
            }

            if !succeeded {
                return Err(fail(format!("Server returned {}", status.map_or("undefined".to_string(), |code| code.to_string())), UserDataSyncErrorCode::Unknown));
            }

            Ok(context)
        })
    }

    /// `deleteResources`: apaga os recursos de todos os perfis (a coleção padrão).
    pub fn delete_resources(&self) -> Promise<()> {
        let this = self.rc();
        async_fn(async move {
            let url = join_path(&this.store_url()?, &["resource"]);
            let mut headers = IHeaders::new();
            set_header(&mut headers, "Content-Type", "text/plain");
            this.send(&url, "DELETE", headers, None, "userDataSync.deleteResources", &[]).await?;
            Ok(())
        })
    }
}

impl IDisposable for UserDataSyncStoreClient {
    fn dispose(&self) {
        self.base.dispose();
    }
}

impl IUserDataSyncStoreService for UserDataSyncStoreClient {
    fn on_did_change_donot_make_requests_until(&self) -> Event<()> {
        self.on_did_change_donot_make_requests_until.event()
    }

    fn donot_make_requests_until(&self) -> Option<f64> {
        self.donot_make_requests_until.get()
    }

    fn on_token_failed(&self) -> Event<UserDataSyncErrorCode> {
        self.on_token_failed.event()
    }

    fn on_token_succeed(&self) -> Event<()> {
        self.on_token_succeed.event()
    }

    fn set_auth_token(&self, token: &str, r#type: &str) {
        *self.auth_token.borrow_mut() = Some(AuthToken { token: token.to_string(), r#type: r#type.to_string() });
    }

    fn manifest(&self, old_value: Option<IUserDataManifest>, headers: Option<IHeaders>) -> Promise<Option<IUserDataManifest>> {
        let this = self.rc();
        async_fn(async move {
            let url = join_path(&this.store_url()?, &["manifest"]);
            let mut headers = headers.unwrap_or_default();
            set_header(&mut headers, "Content-Type", "application/json");
            if let Some(old_value) = &old_value {
                set_header(&mut headers, "If-None-Match", &old_value.r#ref);
            }

            let context = this.send(&url, "GET", headers, None, "userDataSync.manifest", &[304]).await?;
            let url = url.to_string(false).to_string_lossy();
            let operation_id = || read_header(Some(&context.res.headers), HEADER_OPERATION_ID);
            let status = context.res.status_code;

            let mut manifest = if status == Some(304) { old_value } else { None };

            if manifest.is_none() {
                let Some(r#ref) = read_header(Some(&context.res.headers), "etag") else {
                    return Err(store_error("Server did not return the ref", &url, UserDataSyncErrorCode::NoRef, status.map(f64::from), operation_id()));
                };

                let content = as_text_or_error(&context).await?.filter(|content| !content.is_empty());
                if content.is_none() && status == Some(304) {
                    return Err(store_error("Empty response", &url, UserDataSyncErrorCode::EmptyResponse, status.map(f64::from), operation_id()));
                }

                if let Some(content) = content {
                    manifest = Some(parse_manifest(&content, r#ref)?);
                }
            }

            let current_session_id = this.storage_service.get(USER_SESSION_ID_KEY, StorageScope::Application, None).filter(|session| !session.is_empty());

            if let (Some(current_session_id), Some(manifest)) = (&current_session_id, &manifest)
                && *current_session_id != manifest.session
            {
                // Server session is different from client session so clear cached session.
                this.clear_session();
            }

            if manifest.is_none() && current_session_id.is_some() {
                // server session is cleared so clear cached session.
                this.clear_session();
            }

            if let Some(manifest) = &manifest {
                // update session
                this.storage_service.store(USER_SESSION_ID_KEY, StorageValue::from(manifest.session.as_str()), StorageScope::Application, StorageTarget::Machine);
            }

            Ok(manifest)
        })
    }

    fn read_resource(&self, resource: ServerResource, old_value: Option<IUserData>, collection: Option<&str>, headers: Option<IHeaders>) -> Promise<IUserData> {
        let this = self.rc();
        let collection = collection.map(str::to_string);
        async_fn(async move {
            let url = join_path(&Self::get_resource_url(&this.store_url()?, collection.as_deref(), resource), &["latest"]);
            let mut headers = headers.unwrap_or_default();
            // Disable caching as they are cached by synchronisers
            set_header(&mut headers, "Cache-Control", "no-cache");
            if let Some(old_value) = &old_value {
                set_header(&mut headers, "If-None-Match", &old_value.r#ref);
            }

            let context = this.send(&url, "GET", headers, None, "userDataSync.readResource", &[304]).await?;
            let url = url.to_string(false).to_string_lossy();
            let status = context.res.status_code;

            let user_data = if status == Some(304) { old_value } else { None };
            match user_data {
                Some(user_data) => Ok(user_data),
                None => {
                    let operation_id = || read_header(Some(&context.res.headers), HEADER_OPERATION_ID);
                    let Some(r#ref) = read_header(Some(&context.res.headers), "etag") else {
                        return Err(store_error("Server did not return the ref", &url, UserDataSyncErrorCode::NoRef, status.map(f64::from), operation_id()));
                    };

                    let content = as_text_or_error(&context).await?;
                    if content.as_deref().is_none_or(str::is_empty) && status == Some(304) {
                        return Err(store_error("Empty response", &url, UserDataSyncErrorCode::EmptyResponse, status.map(f64::from), operation_id()));
                    }

                    Ok(IUserData { r#ref, content })
                }
            }
        })
    }

    fn write_resource(&self, resource: ServerResource, content: &str, r#ref: Option<&str>, collection: Option<&str>, headers: Option<IHeaders>) -> Promise<String> {
        let this = self.rc();
        let (data, r#ref, collection) = (content.to_string(), r#ref.map(str::to_string), collection.map(str::to_string));
        async_fn(async move {
            let url = Self::get_resource_url(&this.store_url()?, collection.as_deref(), resource);
            let mut headers = headers.unwrap_or_default();
            set_header(&mut headers, "Content-Type", "text/plain");
            if let Some(r#ref) = r#ref.as_deref().filter(|r#ref| !r#ref.is_empty()) {
                set_header(&mut headers, "If-Match", r#ref);
            }

            let context = this.send(&url, "POST", headers, Some(data), "userDataSync.writeResource", &[]).await?;

            match read_header(Some(&context.res.headers), "etag") {
                Some(new_ref) => Ok(new_ref),
                None => Err(store_error(
                    "Server did not return the ref",
                    &url.to_string(false).to_string_lossy(),
                    UserDataSyncErrorCode::NoRef,
                    context.res.status_code.map(f64::from),
                    read_header(Some(&context.res.headers), HEADER_OPERATION_ID),
                )),
            }
        })
    }

    fn delete_resource(&self, resource: ServerResource, r#ref: Option<&str>, collection: Option<&str>) -> Promise<()> {
        let this = self.rc();
        let (r#ref, collection) = (r#ref.map(str::to_string), collection.map(str::to_string));
        async_fn(async move {
            let resource_url = Self::get_resource_url(&this.store_url()?, collection.as_deref(), resource);
            let url = match &r#ref {
                Some(r#ref) => join_path(&resource_url, &[r#ref.as_str()]),
                None => resource_url,
            };
            this.send(&url, "DELETE", IHeaders::new(), None, "userDataSync.deleteResource", &[]).await?;
            Ok(())
        })
    }

    fn get_all_resource_refs(&self, resource: ServerResource, collection: Option<&str>) -> Promise<Vec<IResourceRefHandle>> {
        let this = self.rc();
        let collection = collection.map(str::to_string);
        async_fn(async move {
            let uri = Self::get_resource_url(&this.store_url()?, collection.as_deref(), resource);
            let context = this.send(&uri, "GET", IHeaders::new(), None, "userDataSync.getAllResourceRefs", &[]).await?;

            let result = match as_json(&context).await? {
                Some(Dynamic::Array(items)) => items.to_vec(),
                _ => Vec::new(),
            };
            Ok(result
                .iter()
                .map(|item| {
                    let url = match item.get_str("url") {
                        Dynamic::String(url) => url,
                        _ => JsString::from(""),
                    };
                    let created = match item.get_str("created") {
                        Dynamic::Number(created) => created,
                        _ => f64::NAN,
                    };
                    let path = uri.with(&UriChange { path: Field::Set(url), ..UriChange::default() });
                    IResourceRefHandle { r#ref: ext_uri().relative_path(&uri, &path).unwrap_or_default(), created: created * 1000.0 /* Server returns in seconds */ }
                })
                .collect())
        })
    }

    fn resolve_resource_content(&self, resource: ServerResource, r#ref: &str, collection: Option<&str>, headers: Option<IHeaders>) -> Promise<Option<String>> {
        let this = self.rc();
        let (r#ref, collection) = (r#ref.to_string(), collection.map(str::to_string));
        async_fn(async move {
            let url = join_path(&Self::get_resource_url(&this.store_url()?, collection.as_deref(), resource), &[r#ref.as_str()]);
            let mut headers = headers.unwrap_or_default();
            set_header(&mut headers, "Cache-Control", "no-cache");

            let context = this.send(&url, "GET", headers, None, "userDataSync.resolveResourceContent", &[]).await?;
            as_text_or_error(&context).await
        })
    }

    fn get_all_collections(&self, headers: Option<IHeaders>) -> Promise<Vec<String>> {
        let this = self.rc();
        async_fn(async move {
            let url = join_path(&this.store_url()?, &["collection"]);
            let mut headers = headers.unwrap_or_default();
            set_header(&mut headers, "Content-Type", "application/json");

            let context = this.send(&url, "GET", headers, None, "userDataSync.getAllCollections", &[]).await?;
            let collections = match as_json(&context).await? {
                Some(Dynamic::Array(items)) => items.to_vec(),
                _ => Vec::new(),
            };
            Ok(collections
                .iter()
                .map(|collection| match collection.get_str("id") {
                    Dynamic::String(id) => id.to_string_lossy(),
                    _ => String::new(),
                })
                .collect())
        })
    }

    fn create_collection(&self, headers: Option<IHeaders>) -> Promise<String> {
        let this = self.rc();
        async_fn(async move {
            let url = join_path(&this.store_url()?, &["collection"]);
            let mut headers = headers.unwrap_or_default();
            set_header(&mut headers, "Content-Type", "text/plain");

            let context = this.send(&url, "POST", headers, None, "userDataSync.createCollection", &[]).await?;
            match as_text_or_error(&context).await?.filter(|collection_id| !collection_id.is_empty()) {
                Some(collection_id) => Ok(collection_id),
                None => Err(store_error(
                    "Server did not return the collection id",
                    &url.to_string(false).to_string_lossy(),
                    UserDataSyncErrorCode::NoCollection,
                    context.res.status_code.map(f64::from),
                    read_header(Some(&context.res.headers), HEADER_OPERATION_ID),
                )),
            }
        })
    }

    fn delete_collection(&self, collection: Option<&str>, headers: Option<IHeaders>) -> Promise<()> {
        let this = self.rc();
        let collection = collection.map(str::to_string);
        async_fn(async move {
            let base = this.store_url()?;
            let url = match collection.as_deref().filter(|collection| !collection.is_empty()) {
                Some(collection) => join_path(&base, &["collection", collection]),
                None => join_path(&base, &["collection"]),
            };
            this.send(&url, "DELETE", headers.unwrap_or_default(), None, "userDataSync.deleteCollection", &[]).await?;
            Ok(())
        })
    }

    fn get_latest_data(&self, headers: Option<IHeaders>) -> Promise<Option<IUserDataSyncLatestData>> {
        let this = self.rc();
        async_fn(async move {
            let url = join_path(&this.store_url()?, &["download", "latest"]);
            let mut headers = headers.unwrap_or_default();
            set_header(&mut headers, "Content-Type", "application/json");
            let context = this.send(&url, "GET", headers, None, "userDataSync.getLatestData", &[]).await?;
            let url = url.to_string(false).to_string_lossy();

            if !is_success(&context) {
                return Err(store_error(
                    &format!("Server returned {}", context.res.status_code.map_or("undefined".to_string(), |code| code.to_string())),
                    &url,
                    UserDataSyncErrorCode::EmptyResponse,
                    context.res.status_code.map(f64::from),
                    read_header(Some(&context.res.headers), HEADER_OPERATION_ID),
                ));
            }

            let Some(server_data) = as_json(&context).await? else { return Ok(None) };

            let mut result = IUserDataSyncLatestData::default();
            let resources = server_data.get_str("resources");
            if matches!(resources, Dynamic::Object(_)) {
                result.resources = Some(user_data_resources(&resources));
            }

            let collections = server_data.get_str("collections");
            if matches!(collections, Dynamic::Object(_)) {
                let mut result_collections = IndexMap::new();
                for (collection, data) in collections.own_entries() {
                    let resources = data.get_str("resources");
                    result_collections.insert(collection.to_string_lossy(), IUserDataSyncLatestCollection { resources: Some(user_data_resources(&resources)) });
                }
                result.collections = Some(result_collections);
            }

            Ok(Some(result))
        })
    }

    fn get_activity_data(&self) -> Promise<VSBufferReadableStream> {
        let this = self.rc();
        async_fn(async move {
            let url = join_path(&this.store_url()?, &["download"]);
            let context = this.send(&url, "GET", IHeaders::new(), None, "userDataSync.getActivityData", &[]).await?;
            let url = url.to_string(false).to_string_lossy();
            let (status, operation_id) = (context.res.status_code.map(f64::from), read_header(Some(&context.res.headers), HEADER_OPERATION_ID));

            if !is_success(&context) {
                return Err(store_error(
                    &format!("Server returned {}", context.res.status_code.map_or("undefined".to_string(), |code| code.to_string())),
                    &url,
                    UserDataSyncErrorCode::EmptyResponse,
                    status,
                    operation_id,
                ));
            }

            if has_no_content(&context) {
                return Err(store_error("Empty response", &url, UserDataSyncErrorCode::EmptyResponse, status, operation_id));
            }

            let stream: VSBufferReadableStream = context.stream.clone();
            Ok(stream)
        })
    }

    fn clear(&self) -> Promise<()> {
        let this = self.rc();
        async_fn(async move {
            this.store_url()?;

            this.delete_collection(None, None).await?;
            this.delete_resources().await?;

            // clear cached session.
            this.clear_session();
            Ok(())
        })
    }
}

// endregion
