//! Porte de `src/vs/platform/userDataSync/test/common/userDataSyncClient.ts`: o cliente do Settings Sync para
//! os testes (`UserDataSyncClient`), o servidor HTTP em memória (`UserDataSyncTestServer`) e o serviço de
//! utilidades de teste (`TestUserDataSyncUtilService`).
//!
//! Divergências:
//! - O `TestInstantiationService` não existe: o cliente monta os serviços na mão, na mesma ordem de
//!   `setUp`, e os guarda em campos públicos (o `instantiationService.get(IFoo)` do original é o campo).
//!   `new UserDataSyncClient(server)` e `setUp(empty)` são uma função só, [`UserDataSyncClient::set_up`].
//! - `registerConfiguration()` (o registro das chaves `settingsSync.*` e do esquema JSON de configurações
//!   ignoradas) não é chamado: o registro de contribuições JSON não foi portado e o teste de perfis não lê
//!   essas chaves.
//! - O `TestStorageService` (um `InMemoryStorageService` com `hasScope` que só aceita o perfil padrão) é o
//!   `InMemoryStorageService` comum: `hasScope` só é lido pelo `IUserDataProfileStorageService`, que o
//!   cliente não monta porque os sincronizadores de estado global não foram portados. Também ficam de fora o
//!   `IMeteredConnectionService`, o `IExtensionManagementService`, o `IGlobalExtensionEnablementService`, o
//!   `IExtensionStorageService`, o `IIgnoredExtensionsManagementService`, o `IExtensionGalleryService` e o
//!   `IUserDataSyncMachinesService`, que só os sincronizadores não portados e o serviço de máquinas leem.
//! - O stub de `IEnvironmentService` do original declara quatro campos e deixa o resto `undefined`;
//!   [`TestEnvironmentService`] tem os quatro e valores neutros (caminhos sob `userRoamingDataHome`) para o
//!   resto. O `serviceMachineIdResource`, `undefined` no original (a leitura e a gravação falham e são
//!   ignoradas), é `machineid` sob `userRoamingDataHome`.
//! - O `UriIdentityService` é o `ExtUri` sem viés (o provider em memória distingue maiúsculas).
//! - `getSynchronizer(source)` devolve o [`Synchroniser`] do porte; o `as UserDataProfilesManifestSynchroniser`
//!   do teste é [`Synchroniser::into_profiles_manifest`].
//! - `UserDataSyncTestServer.requests` e as outras listas são métodos que devolvem cópias; o
//!   `Number.MAX_SAFE_INTEGER` do `rateLimit` padrão é `usize::MAX`.
#![allow(dead_code)]

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use indexmap::IndexMap;
use je_base::JsString;
use je_base::buffer::{VSBuffer, buffer_to_stream};
use je_base::cancellation::CancellationToken;
use je_base::errors::Error;
use je_base::event::Event;
use je_base::json::{Object, Value};
use je_base::json_formatter::FormattingOptions;
use je_base::lifecycle::{Disposable, IDisposable};
use je_base::network::schemas;
use je_base::parts::request::{HeaderValue, IHeaders, IRequestContext, IRequestOptions, IRequestResponse};
use je_base::product::{ConfigurationSyncStore, IAuthenticationProviderScopes};
use je_base::promise::Promise;
use je_base::resources::ExtUri;
use je_base::uri::{Field, URI, UriChange};
use je_base::uuid::generate_uuid;
use je_platform::configuration::service::ConfigurationService;
use je_platform::environment::environment::{DisableExtensions, EnvironmentService, ExtensionHostDebugParams, ExtensionKind, SyncMode};
use je_platform::files::file_service::{FileContentSource, FileService};
use je_platform::files::in_memory::InMemoryFileSystemProvider;
use je_platform::log::NullLogService;
use je_platform::policy::NullPolicyService;
use je_platform::product::ProductService;
use je_platform::request::{AuthInfo, Credentials, IRequestService, RequestCompleteEvent};
use je_platform::resources::{host_environment, uri_file};
use je_platform::storage::{InMemoryStorageService, StorageService};
use je_platform::telemetry::null_telemetry_service;
use je_platform::user_data_profile::{IUserDataProfilesService, InMemoryProfilesStore, UserDataProfilesEnvironment, UserDataProfilesService};
use je_platform::user_data_sync::abstract_synchronizer::{SynchroniserServices, stringify_json};
use je_platform::user_data_sync::user_data_sync::{
    ALL_SYNC_RESOURCES, IUserData, IUserDataResourceManifest, IUserDataSyncEnablementService, IUserDataSyncStoreService, IUserDataSyncUtilService, ServerResource, SyncResource,
    USER_DATA_SYNC_SCHEME, get_default_ignored_settings,
};
use je_platform::user_data_sync::user_data_sync_account::{IUserDataSyncAccount, IUserDataSyncAccountService, UserDataSyncAccountService};
use je_platform::user_data_sync::user_data_sync_enablement_service::UserDataSyncEnablementService;
use je_platform::user_data_sync::user_data_sync_local_store_service::UserDataSyncLocalStoreService;
use je_platform::user_data_sync::user_data_sync_service::{Synchroniser, UserDataSyncService};
use je_platform::user_data_sync::user_data_sync_store_service::{UserDataSyncStoreClientServices, UserDataSyncStoreManagementService, UserDataSyncStoreService};

// region: servidor de teste

/// `{ url, type, headers }` de `UserDataSyncTestServer.requests`: só os cabeçalhos `If-None-Match` e `If-Match`.
#[derive(Clone, Debug, PartialEq)]
pub struct RecordedRequest {
    pub url: String,
    pub r#type: String,
    pub headers: IHeaders,
}

/// `{ url, type, headers }` de `UserDataSyncTestServer.requestsWithAllHeaders`.
#[derive(Clone, Debug, PartialEq)]
pub struct RecordedRequestWithAllHeaders {
    pub url: String,
    pub r#type: String,
    pub headers: Option<IHeaders>,
}

#[derive(Default)]
struct ServerState {
    session: Option<String>,
    collections: IndexMap<String, IndexMap<String, IUserData>>,
    data: IndexMap<String, IUserData>,
    requests: Vec<RecordedRequest>,
    requests_with_all_headers: Vec<RecordedRequestWithAllHeaders>,
    responses: Vec<u16>,
    manifest_ref: u64,
    collection_counter: u64,
}

/// `ALL_SERVER_RESOURCES`: `[...ALL_SYNC_RESOURCES, 'machines']`.
fn is_server_resource(resource: &str) -> bool {
    ALL_SYNC_RESOURCES.iter().any(|sync_resource| sync_resource.as_str() == resource) || resource == "machines"
}

fn json_string(text: &str) -> Value {
    Value::String(JsString::from(text))
}

/// `headers['<nome>']` como texto (um valor em lista não é igual a texto).
fn header_text<'a>(headers: &'a IHeaders, name: &str) -> Option<&'a str> {
    match headers.get(name) {
        Some(HeaderValue::One(text)) => Some(text),
        _ => None,
    }
}

/// `toResponse(statusCode, headers, data)`.
fn to_response(status_code: u16, headers: &[(&str, &str)], data: &str) -> IRequestContext {
    IRequestContext {
        res: IRequestResponse { headers: headers.iter().map(|(name, value)| (name.to_string(), HeaderValue::One(value.to_string()))).collect(), status_code: Some(status_code) },
        stream: buffer_to_stream(VSBuffer::from_string(data)),
        timings: None,
    }
}

/// `collection ? this.collections.get(collection) : this.data`.
fn collection_data<'a>(state: &'a mut ServerState, collection: Option<&str>) -> Option<&'a mut IndexMap<String, IUserData>> {
    match collection.filter(|collection| !collection.is_empty()) {
        Some(collection) => state.collections.get_mut(collection),
        None => Some(&mut state.data),
    }
}

/// `{ [resource]: ref }` como o objeto JSON do manifesto.
fn latest_json(data: &IndexMap<String, IUserData>) -> Value {
    let mut latest = Object::new();
    for (resource, value) in data {
        latest.insert(JsString::from(resource.as_str()), json_string(&value.r#ref));
    }
    Value::Object(latest)
}

/// `UserDataSyncTestServer`.
pub struct UserDataSyncTestServer {
    pub url: String,
    rate_limit: usize,
    retry_after: Option<u32>,
    state: RefCell<ServerState>,
}

impl Default for UserDataSyncTestServer {
    fn default() -> Self {
        Self::new()
    }
}

impl UserDataSyncTestServer {
    pub fn new() -> Self {
        Self::with_rate_limit(usize::MAX, None)
    }

    pub fn with_rate_limit(rate_limit: usize, retry_after: Option<u32>) -> Self {
        Self { url: "http://host:3000".to_string(), rate_limit, retry_after, state: RefCell::new(ServerState::default()) }
    }

    pub fn requests(&self) -> Vec<RecordedRequest> {
        self.state.borrow().requests.clone()
    }

    pub fn requests_with_all_headers(&self) -> Vec<RecordedRequestWithAllHeaders> {
        self.state.borrow().requests_with_all_headers.clone()
    }

    /// Os status das respostas (`responses[].status`).
    pub fn responses(&self) -> Vec<u16> {
        self.state.borrow().responses.clone()
    }

    pub fn reset(&self) {
        let mut state = self.state.borrow_mut();
        state.requests.clear();
        state.responses.clear();
        state.requests_with_all_headers.clear();
    }

    fn do_request(&self, state: &mut ServerState, options: &IRequestOptions) -> IRequestContext {
        let version_url = format!("{}/v1/", self.url);
        let url = options.url.as_deref().unwrap_or_default();
        let relative_path = url.strip_prefix(version_url.as_str()).filter(|relative_path| !relative_path.is_empty());
        let segments: Vec<&str> = relative_path.map_or_else(Vec::new, |relative_path| relative_path.split('/').collect());
        let no_headers = IHeaders::new();
        let headers = options.headers.as_ref().unwrap_or(&no_headers);
        let latest_or_ref = |reference: &str| if reference == "latest" { None } else { Some(reference.to_string()) };
        match (options.r#type.as_deref().unwrap_or_default(), segments.as_slice()) {
            ("GET", ["manifest"]) => Self::get_manifest(state),
            ("GET", ["resource", resource, reference]) => Self::get_resource_data(state, None, resource, latest_or_ref(reference).as_deref(), headers),
            ("POST", ["resource", resource]) => Self::write_data(state, None, resource, options.data.as_deref().unwrap_or_default(), headers),
            // resources in collection
            ("GET", ["collection", collection, "resource", resource, reference]) => Self::get_resource_data(state, Some(*collection), resource, latest_or_ref(reference).as_deref(), headers),
            ("POST", ["collection", collection, "resource", resource]) => Self::write_data(state, Some(*collection), resource, options.data.as_deref().unwrap_or_default(), headers),
            ("DELETE", ["resource", resource]) => Self::delete_resource_data(state, None, resource),
            ("DELETE", ["resource"]) => Self::clear(state),
            ("DELETE", ["collection", ..]) => to_response(204, &[], ""),
            ("POST", ["collection"]) => Self::create_collection(state),
            _ => to_response(501, &[], ""),
        }
    }

    fn get_manifest(state: &mut ServerState) -> IRequestContext {
        let etag = state.manifest_ref.to_string();
        state.manifest_ref += 1;
        let Some(session) = state.session.clone() else { return to_response(204, &[("etag", etag.as_str())], "") };

        let mut manifest = Object::new();
        manifest.insert(JsString::from("session"), json_string(&session));
        manifest.insert(JsString::from("latest"), latest_json(&state.data));
        if state.collection_counter != 0 {
            let mut collections = Object::new();
            for collection_id in 1..=state.collection_counter {
                if let Some(collection_data) = state.collections.get(&collection_id.to_string()) {
                    let mut entry = Object::new();
                    entry.insert(JsString::from("latest"), latest_json(collection_data));
                    collections.insert(JsString::from(collection_id.to_string().as_str()), Value::Object(entry));
                }
            }
            manifest.insert(JsString::from("collections"), Value::Object(collections));
        }
        manifest.insert(JsString::from("ref"), json_string("1"));
        to_response(200, &[("Content-Type", "application/json"), ("etag", etag.as_str())], &stringify_json(&Value::Object(manifest)))
    }

    fn get_resource_data(state: &mut ServerState, collection: Option<&str>, resource: &str, reference: Option<&str>, headers: &IHeaders) -> IRequestContext {
        let Some(collection_data) = collection_data(state, collection) else { return to_response(501, &[], "") };

        if is_server_resource(resource) {
            let data = collection_data.get(resource);
            if let Some(reference) = reference.filter(|reference| !reference.is_empty())
                && data.map(|data| data.r#ref.as_str()) != Some(reference)
            {
                return to_response(404, &[], "");
            }
            let Some(data) = data else { return to_response(204, &[("etag", "0")], "") };
            if header_text(headers, "If-None-Match") == Some(data.r#ref.as_str()) {
                return to_response(304, &[], "");
            }
            return to_response(200, &[("etag", data.r#ref.as_str())], data.content.as_deref().unwrap_or_default());
        }
        to_response(204, &[], "")
    }

    fn write_data(state: &mut ServerState, collection: Option<&str>, resource: &str, content: &str, headers: &IHeaders) -> IRequestContext {
        if state.session.is_none() {
            state.session = Some(generate_uuid());
        }
        let Some(collection_data) = collection_data(state, collection) else { return to_response(501, &[], "") };
        if is_server_resource(resource) {
            let data = collection_data.get(resource);
            if let Some(if_match) = header_text(headers, "If-Match")
                && if_match != data.map_or("0", |data| data.r#ref.as_str())
            {
                return to_response(412, &[], "");
            }
            let current = data.and_then(|data| data.r#ref.parse::<i64>().ok()).unwrap_or(0);
            let reference = (current + 1).to_string();
            collection_data.insert(resource.to_string(), IUserData { r#ref: reference.clone(), content: Some(content.to_string()) });
            return to_response(200, &[("etag", reference.as_str())], "");
        }
        to_response(204, &[], "")
    }

    fn delete_resource_data(state: &mut ServerState, collection: Option<&str>, resource: &str) -> IRequestContext {
        let Some(collection_data) = collection_data(state, collection) else { return to_response(501, &[], "") };

        if is_server_resource(resource) {
            collection_data.shift_remove(resource);
            return to_response(200, &[], "");
        }

        to_response(404, &[], "")
    }

    fn create_collection(state: &mut ServerState) -> IRequestContext {
        state.collection_counter += 1;
        let collection_id = state.collection_counter.to_string();
        state.collections.insert(collection_id.clone(), IndexMap::new());
        to_response(200, &[], &collection_id)
    }

    fn clear(state: &mut ServerState) -> IRequestContext {
        state.collections.clear();
        state.data.clear();
        state.session = None;
        state.collection_counter = 0;
        to_response(204, &[], "")
    }
}

impl IRequestService for UserDataSyncTestServer {
    fn on_did_complete_request(&self) -> Event<RequestCompleteEvent> {
        Event::none()
    }

    fn request(&self, options: IRequestOptions, _token: CancellationToken) -> Promise<IRequestContext> {
        let mut state = self.state.borrow_mut();
        if state.requests.len() == self.rate_limit {
            let retry_after = self.retry_after.filter(|retry_after| *retry_after != 0).map(|retry_after| retry_after.to_string());
            return Promise::resolved(match &retry_after {
                Some(retry_after) => to_response(429, &[("retry-after", retry_after.as_str())], ""),
                None => to_response(429, &[], ""),
            });
        }
        let mut headers = IHeaders::new();
        for name in ["If-None-Match", "If-Match"] {
            if let Some(value) = options.headers.as_ref().and_then(|headers| headers.get(name)).filter(|value| **value != HeaderValue::One(String::new())) {
                headers.insert(name.to_string(), value.clone());
            }
        }
        let (url, r#type) = (options.url.clone().unwrap_or_default(), options.r#type.clone().unwrap_or_default());
        state.requests.push(RecordedRequest { url: url.clone(), r#type: r#type.clone(), headers });
        state.requests_with_all_headers.push(RecordedRequestWithAllHeaders { url, r#type, headers: options.headers.clone() });
        let request_context = self.do_request(&mut state, &options);
        state.responses.push(request_context.res.status_code.unwrap_or_default());
        Promise::resolved(request_context)
    }

    fn resolve_proxy(&self, url: &str) -> Promise<Option<String>> {
        Promise::resolved(Some(url.to_string()))
    }

    fn lookup_authorization(&self, _auth_info: AuthInfo) -> Promise<Option<Credentials>> {
        Promise::resolved(None)
    }

    fn lookup_kerberos_authorization(&self, _url: &str) -> Promise<Option<String>> {
        Promise::resolved(None)
    }

    fn load_certificates(&self) -> Promise<Vec<String>> {
        Promise::resolved(Vec::new())
    }
}

// endregion

// region: serviços de teste

/// `TestUserDataSyncUtilService`.
pub struct TestUserDataSyncUtilService;

impl IUserDataSyncUtilService for TestUserDataSyncUtilService {
    fn resolve_default_core_ignored_settings(&self) -> Promise<Vec<JsString>> {
        Promise::resolved(get_default_ignored_settings(false))
    }

    fn resolve_user_bindings(&self, userbindings: Vec<JsString>) -> Promise<HashMap<JsString, JsString>> {
        Promise::resolved(userbindings.into_iter().map(|keybinding| (keybinding.clone(), keybinding)).collect())
    }

    fn resolve_formatting_options(&self, _file: Option<&URI>) -> Promise<FormattingOptions> {
        Promise::resolved(FormattingOptions { eol: Some(JsString::from("\n")), insert_spaces: Some(false), tab_size: Some(4) })
    }
}

/// O stub de `IEnvironmentService` do `setUp`: `userDataSyncHome`, `userRoamingDataHome`, `cacheHome`,
/// `argvResource` e `sync: 'on'`.
pub struct TestEnvironmentService {
    pub user_data_sync_home: URI,
    pub user_roaming_data_home: URI,
    pub cache_home: URI,
    pub argv_resource: URI,
    ext_uri: ExtUri,
}

impl TestEnvironmentService {
    fn under_roaming_data_home(&self, name: &str) -> URI {
        self.ext_uri.join_path(&self.user_roaming_data_home, &[name])
    }
}

impl EnvironmentService for TestEnvironmentService {
    fn state_resource(&self) -> URI {
        self.under_roaming_data_home("storage.json")
    }
    fn user_roaming_data_home(&self) -> URI {
        self.user_roaming_data_home.clone()
    }
    fn keyboard_layout_resource(&self) -> URI {
        self.under_roaming_data_home("keyboardLayout.json")
    }
    fn argv_resource(&self) -> URI {
        self.argv_resource.clone()
    }
    fn untitled_workspaces_home(&self) -> URI {
        self.under_roaming_data_home("Workspaces")
    }
    fn workspace_storage_home(&self) -> URI {
        self.under_roaming_data_home("workspaceStorage")
    }
    fn local_history_home(&self) -> URI {
        self.under_roaming_data_home("History")
    }
    fn cache_home(&self) -> URI {
        self.cache_home.clone()
    }
    fn app_shared_data_home(&self) -> URI {
        self.under_roaming_data_home("shared-data")
    }
    fn user_data_sync_home(&self) -> URI {
        self.user_data_sync_home.clone()
    }
    fn sync(&self) -> Option<SyncMode> {
        Some(SyncMode::On)
    }
    fn continue_on(&self) -> Option<String> {
        None
    }
    fn edit_session_id(&self) -> Option<String> {
        None
    }
    fn debug_extension_host(&self) -> ExtensionHostDebugParams {
        ExtensionHostDebugParams { port: None, break_: false, debug_id: None, env: None }
    }
    fn is_extension_development(&self) -> bool {
        false
    }
    fn disable_extensions(&self) -> DisableExtensions {
        DisableExtensions::All(false)
    }
    fn skip_builtin_extensions(&self) -> Vec<String> {
        Vec::new()
    }
    fn extension_development_location_uri(&self) -> Option<Vec<URI>> {
        None
    }
    fn extension_development_kind(&self) -> Option<Vec<ExtensionKind>> {
        None
    }
    fn extension_tests_location_uri(&self) -> Option<URI> {
        None
    }
    fn logs_home(&self) -> URI {
        self.under_roaming_data_home("logs")
    }
    fn log_level(&self) -> Option<String> {
        None
    }
    fn extension_log_level(&self) -> Option<Vec<(String, String)>> {
        None
    }
    fn verbose(&self) -> bool {
        false
    }
    fn is_built(&self) -> bool {
        false
    }
    fn disable_telemetry(&self) -> bool {
        false
    }
    fn disable_experiments(&self) -> bool {
        false
    }
    fn service_machine_id_resource(&self) -> URI {
        self.under_roaming_data_home("machineid")
    }
    fn agent_sessions_workspace(&self) -> URI {
        self.under_roaming_data_home("agent-sessions")
    }
    fn policy_file(&self) -> Option<URI> {
        None
    }
}

// endregion

// region: cliente

/// `UserDataSyncClient`: um dispositivo do usuário, com os serviços reais do porte sobre arquivos e
/// armazenamento em memória, falando com o `testServer`.
pub struct UserDataSyncClient {
    base: Rc<Disposable>,
    pub test_server: Rc<UserDataSyncTestServer>,
    pub environment_service: Rc<TestEnvironmentService>,
    pub file_service: Rc<FileService>,
    pub user_data_profiles_service: Rc<UserDataProfilesService>,
    pub storage_service: Rc<dyn StorageService>,
    pub configuration_service: Rc<ConfigurationService>,
    pub user_data_sync_store_management_service: Rc<UserDataSyncStoreManagementService>,
    pub user_data_sync_store_service: Rc<UserDataSyncStoreService>,
    pub user_data_sync_account_service: Rc<UserDataSyncAccountService>,
    pub user_data_sync_local_store_service: Rc<UserDataSyncLocalStoreService>,
    pub user_data_sync_util_service: Rc<TestUserDataSyncUtilService>,
    pub user_data_sync_enablement_service: Rc<UserDataSyncEnablementService>,
    pub user_data_sync_service: Rc<UserDataSyncService>,
}

impl IDisposable for UserDataSyncClient {
    fn dispose(&self) {
        self.base.dispose();
    }
}

impl UserDataSyncClient {
    /// `new UserDataSyncClient(testServer)` seguido de `setUp(empty)`.
    pub async fn set_up(test_server: Rc<UserDataSyncTestServer>, empty: bool) -> Result<Rc<Self>, Error> {
        let base = Disposable::new();
        let log_service = NullLogService::shared();
        let ext_uri = ExtUri::unbiased(host_environment());

        let user_roaming_data_home = uri_file("userdata").with(&UriChange { scheme: Field::from(schemas::IN_MEMORY), ..UriChange::default() });
        let user_data_sync_home = ext_uri.join_path(&user_roaming_data_home, &[".sync"]);
        let environment_service = Rc::new(TestEnvironmentService {
            user_data_sync_home,
            user_roaming_data_home: user_roaming_data_home.clone(),
            cache_home: ext_uri.join_path(&user_roaming_data_home, &["cache"]),
            argv_resource: ext_uri.join_path(&user_roaming_data_home, &["argv.json"]),
            ext_uri: ext_uri.clone(),
        });

        let test_url = test_server.url.clone();
        let product_service = Rc::new(ProductService {
            configuration_sync_store: Some(ConfigurationSyncStore {
                url: test_url.clone(),
                stable_url: test_url.clone(),
                insiders_url: test_url,
                can_switch: Some(false),
                authentication_providers: IndexMap::from([("test".to_string(), IAuthenticationProviderScopes { scopes: Vec::new(), extra: Default::default() })]),
                extra: Default::default(),
            }),
            ..ProductService::default()
        });

        let file_service = FileService::new(log_service.clone());
        base.register(file_service.clone());
        for scheme in [schemas::IN_MEMORY, USER_DATA_SYNC_SCHEME] {
            let provider = InMemoryFileSystemProvider::new();
            base.register(provider.clone());
            base.register(file_service.register_provider(scheme, provider)?);
        }

        let user_data_profiles_service = UserDataProfilesService::new(
            UserDataProfilesEnvironment { user_roaming_data_home: user_roaming_data_home.clone(), cache_home: environment_service.cache_home.clone(), agent_sessions_workspace: None },
            file_service.clone(),
            ext_uri.clone(),
            log_service.clone(),
            Rc::new(InMemoryProfilesStore::default()),
        );
        base.register(user_data_profiles_service.clone());

        let in_memory_storage_service = InMemoryStorageService::new();
        base.register(in_memory_storage_service.clone());
        let storage_service: Rc<dyn StorageService> = Rc::new(in_memory_storage_service);

        let configuration_service = ConfigurationService::new(user_data_profiles_service.default_profile().settings_resource, file_service.clone(), Rc::new(NullPolicyService), log_service.clone());
        base.register(configuration_service.clone());
        configuration_service.initialize().await?;

        let telemetry_service = null_telemetry_service();
        let user_data_sync_store_management_service = UserDataSyncStoreManagementService::new(product_service.clone(), storage_service.clone());
        base.register(user_data_sync_store_management_service.clone());
        let user_data_sync_store_service = UserDataSyncStoreService::new_service(
            user_data_sync_store_management_service.clone(),
            UserDataSyncStoreClientServices {
                product_service: product_service.clone(),
                request_service: test_server.clone(),
                log_service: log_service.clone(),
                environment_service: environment_service.clone(),
                file_service: file_service.clone(),
                storage_service: storage_service.clone(),
            },
        );
        base.register(user_data_sync_store_service.clone());

        let user_data_sync_account_service = UserDataSyncAccountService::new(user_data_sync_store_service.clone(), log_service.clone());
        base.register(user_data_sync_account_service.clone());
        user_data_sync_account_service.update_account(Some(IUserDataSyncAccount { authentication_provider_id: "authenticationProviderId".to_string(), token: "token".to_string() })).await?;

        let user_data_sync_local_store_service = UserDataSyncLocalStoreService::new(
            environment_service.clone(),
            file_service.clone(),
            configuration_service.clone(),
            log_service.clone(),
            user_data_profiles_service.clone(),
        );
        base.register(user_data_sync_local_store_service.clone());
        let user_data_sync_util_service = Rc::new(TestUserDataSyncUtilService);
        let user_data_sync_enablement_service =
            UserDataSyncEnablementService::new(storage_service.clone(), environment_service.clone(), user_data_sync_store_management_service.clone());
        base.register(user_data_sync_enablement_service.clone());

        let user_data_sync_service = UserDataSyncService::new(
            SynchroniserServices {
                file_service: file_service.clone(),
                environment_service: environment_service.clone(),
                storage_service: storage_service.clone(),
                user_data_sync_store_service: user_data_sync_store_service.clone(),
                user_data_sync_local_store_service: user_data_sync_local_store_service.clone(),
                user_data_sync_enablement_service: user_data_sync_enablement_service.clone(),
                telemetry_service,
                log_service,
                configuration_service: configuration_service.clone(),
                ext_uri,
            },
            user_data_sync_store_management_service.clone(),
            user_data_profiles_service.clone(),
        );
        base.register(user_data_sync_service.clone());

        if !empty {
            let default_profile = user_data_profiles_service.default_profile();
            let write = |resource: URI, text: &'static str| {
                let file_service = file_service.clone();
                async move { file_service.write_file(&resource, FileContentSource::from(text), None).await }
            };
            write(default_profile.settings_resource.clone(), "{}").await?;
            write(default_profile.keybindings_resource.clone(), "[]").await?;
            write(environment_service.ext_uri.join_path(&default_profile.snippets_home, &["c.json"]), "{}").await?;
            write(environment_service.ext_uri.join_path(&default_profile.prompts_home, &["c.prompt.md"]), " ").await?;
            write(default_profile.tasks_resource.clone(), "{}").await?;
            write(environment_service.argv_resource.clone(), "{\"locale\":\"en\"}").await?;
        }
        configuration_service.reload_configuration().await?;

        // `prompts` resource is disabled by default, so enable it for tests
        user_data_sync_enablement_service.set_resource_enablement(SyncResource::Prompts, true);

        Ok(Rc::new(Self {
            base,
            test_server,
            environment_service,
            file_service,
            user_data_profiles_service,
            storage_service,
            configuration_service,
            user_data_sync_store_management_service,
            user_data_sync_store_service,
            user_data_sync_account_service,
            user_data_sync_local_store_service,
            user_data_sync_util_service,
            user_data_sync_enablement_service,
            user_data_sync_service,
        }))
    }

    /// `sync()`: `(await createSyncTask(null)).run()`.
    pub async fn sync(&self) -> Result<(), Error> {
        self.user_data_sync_service.create_sync_task(None, None).await?.run().await
    }

    /// `read(resource, collection?)`.
    pub fn read(&self, resource: SyncResource, collection: Option<&str>) -> Promise<IUserData> {
        self.user_data_sync_store_service.read_resource(ServerResource::from(resource), None, collection, None)
    }

    /// `getLatestRef(resource)`: o `ref` mais recente do recurso no manifesto, ou `None`.
    pub async fn get_latest_ref(&self, resource: SyncResource) -> Result<Option<String>, Error> {
        let manifest = self.get_resource_manifest().await?;
        Ok(manifest.and_then(|manifest| manifest.get(resource.as_str()).cloned()))
    }

    /// `_getResourceManifest()`.
    pub async fn get_resource_manifest(&self) -> Result<Option<IUserDataResourceManifest>, Error> {
        let manifest = self.user_data_sync_store_service.manifest(None, None).await?;
        Ok(manifest.and_then(|manifest| manifest.latest))
    }

    /// `getSynchronizer(source)`: o sincronizador ativo do perfil padrão.
    pub fn get_synchronizer(&self, source: SyncResource) -> Synchroniser {
        self.user_data_sync_service
            .get_or_create_active_profile_synchronizer(&self.user_data_profiles_service.default_profile(), None)
            .enabled()
            .into_iter()
            .find(|synchroniser| synchroniser.resource() == source)
            .expect("o sincronizador do recurso está ativo")
    }
}

// endregion
