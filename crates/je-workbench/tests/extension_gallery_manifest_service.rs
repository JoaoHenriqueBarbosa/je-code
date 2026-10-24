//! Porte de `src/vs/workbench/services/extensionManagement/test/electron-browser/extensionGalleryManifestService.test.ts` (suíte `ExtensionGalleryManifestService`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` é o `LeakGuard` do [`Fixture`], criado antes de qualquer serviço; o
//!   `disposableStore` do original é `fixture.guard`. Cada caso monta o seu [`Fixture`] (o `setup`) e o descarta ao fim.
//!   Os casos `async` rodam com o relógio virtual ([`run_with_faked_timers`]), e `new Promise(resolve => setTimeout(resolve, 0))`
//!   é `timeout(0)`.
//! - `TestInstantiationService` (`stub`, `createInstance`) é o [`Fixture`]: ele guarda os serviços que o `setup` registra e
//!   que os casos trocam (`stub(IProductService, ...)`, `stub(IAuthenticationService, ...)`, `stub(IDefaultAccountService, ...)`),
//!   e `createService()` monta o que o original monta por `createInstance`.
//! - `mock<T>()` é um dublê que implementa só o membro que o original sobrescreve; os demais membros do trait entram com
//!   [`not_mocked`], que falha o caso se alguém os chamar (no original seria um `TypeError`, "is not a function").
//! - `IProductService` é o [`ProductService`] com `extensions_gallery` (o `nameShort` do original, indefinido, é o padrão do
//!   porte). `IEnvironmentService` é o `TestEnvironmentService` e `IFileService` um `FileService` sem provedor de arquivo:
//!   o `getServiceMachineId` não acha o arquivo, gera o identificador e ignora a falha de gravação, como com o
//!   `mock<IFileService>()` do original.
//! - `IStorageService` é o `InMemoryStorageService`; `storageData` é o armazenamento lido no escopo da aplicação
//!   (`marketplace.account`). O armazenamento recebe também a chave `storage.serviceMachineId`, que o original grava
//!   no mapa e que nenhum caso lê.
//! - `IRequestService` guarda um `requestHandler` que devolve a promessa da resposta: o `throw new Error(...)` síncrono do
//!   original (que o `async request` transforma em rejeição) é uma promessa rejeitada. `mockResponse` é o
//!   [`mock_response`]. `{ ... } as IEntitlementsData` (objeto parcial) é um `IEntitlementsData` completo com o
//!   `access_type_sku` do caso e o resto vazio.
//! - `NullTelemetryServiceShape` com `publicLog2` sobrescrito é o [`RecordingTelemetryService`]: o `publicLog2(eventName?, data?)`
//!   guarda `{ eventName, data }`, e `deepStrictEqual` sobre `data` é a igualdade de [`Dynamic`]. `IQuickInputService`, que o
//!   original não registra (o provedor da Microsoft o recebe e só o usa no `signIn`), é um dublê sem membros.
//! - `onDidChangeConfigurationEmitter.fire({ affectsConfiguration })` é o evento real de mudança das chaves dadas
//!   ([`Fixture::fire_config_change`]), que responde `affectsConfiguration` pela lista de chaves.
//! - O `for (const [scenario, body] of [...])` é um só caso, que roda os cinco cenários, cada um com o seu [`Fixture`]. Os
//!   nomes dos casos do original trazem travessão e seta, que ficam de fora dos nomes aqui.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use std::cell::{Cell, RefCell};
use std::future::Future;
use std::rc::Rc;

use common::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite, run_with_faked_timers, timeout};
use je_base::JsString;
use je_base::buffer::{VSBuffer, buffer_to_stream, encode_base64};
use je_base::cancellation::CancellationToken;
use je_base::default_account::{IDefaultAccount, IDefaultAccountAuthenticationProvider, IEntitlementsData};
use je_base::dynamic::{Dynamic, json_stringify};
use je_base::errors::Error;
use je_base::event::{Emitter, Event};
use je_base::map::ResourceMap;
use je_base::marshalling::Marshalled;
use je_base::parts::ipc::IChannel;
use je_base::parts::request::{IHeaders, IRequestContext, IRequestOptions, IRequestResponse};
use je_base::product::IExtensionsGallery;
use je_base::promise::Promise;
use je_platform::configuration::ConfigurationChange;
use je_platform::configuration::models::{Configuration, ConfigurationChangeEvent, ConfigurationModel};
use je_platform::configuration::service::IConfigurationService;
use je_platform::configuration::test_configuration_service::TestConfigurationService;
use je_platform::dialogs::{Confirmation, ConfirmationResult, IDialogService, Input, InputResult, Prompt, PromptResult};
use je_platform::environment::environment::EnvironmentService;
use je_platform::extension_management::extension_gallery_manifest::{
    EXTENSION_GALLERY_AUTH_PROVIDER_CONFIG_KEY, EXTENSION_GALLERY_SERVICE_URL_CONFIG_KEY, ExtensionGalleryManifestStatus, IExtensionGalleryManifestService,
};
use je_platform::log::NullLogService;
use je_platform::product::{IProductService, ProductService};
use je_platform::quickinput::quick_input::{IInputOptions, IPickOptions, IQuickInputService, IQuickPickItem};
use je_platform::request::{AuthInfo, Credentials, IRequestService, RequestCompleteEvent};
use je_platform::storage::{InMemoryStorageService, StorageScope, StorageService, StorageTarget};
use je_platform::telemetry::{CommonPropertyValue, ITelemetryData, ITelemetryService, NullTelemetryServiceShape, TelemetryLevel};
use je_storage::StorageValue;
use je_workbench::services::authentication::authentication::AuthenticationSessionAccount;
use je_workbench::services::extension_management::extension_gallery_account::{IExtensionGalleryAccountProvider, IExtensionGalleryAccountService};
use je_workbench::services::extension_management::extension_gallery_account_service::{
    ExtensionGalleryAccountService, GitHubGalleryAccountProvider, MicrosoftGalleryAccountProvider, uses_microsoft_auth_provider,
};
use je_workbench::services::extension_management::extension_gallery_contracts::{
    AuthenticationSession, AuthenticationSessionsChange, AuthenticationSessionsChangeEvent, IGalleryAuthenticationService, IGalleryDefaultAccountService,
    IHostRestartService, IRemoteAgentConnection, IRemoteAgentConnectionProvider, ISharedProcessService,
};
use je_workbench::services::extension_management::extension_gallery_manifest_service::{
    WorkbenchExtensionGalleryManifestService, WorkbenchExtensionGalleryManifestServiceParts,
};
use je_workbench::test::browser::workbench_test_services::TestEnvironmentService;
use serde_json::{Value, json};

// ---------------------------------------------------------------------------------------------------------------------
// Auxiliares da suíte: respostas, contas, tokens e os dublês dos serviços.
// ---------------------------------------------------------------------------------------------------------------------

/// O membro que o `mock<T>()` do original não sobrescreve: chamá-lo falha o caso.
fn not_mocked(member: &str) -> ! {
    panic!("o dublê do original (mock<T>()) não sobrescreve `{member}`")
}

/// `mockResponse(statusCode, body)`.
fn mock_response(status_code: u16, body: Value) -> IRequestContext {
    IRequestContext {
        res: IRequestResponse { headers: IHeaders::new(), status_code: Some(status_code) },
        stream: buffer_to_stream(VSBuffer::from_string(&body.to_string())),
        timings: None,
    }
}

/// `createDefaultAccount({ enterprise, entitlementsData })`: os demais campos são os do original.
fn create_default_account(enterprise: bool, entitlements_data: Option<IEntitlementsData>) -> IDefaultAccount {
    IDefaultAccount {
        authentication_provider: IDefaultAccountAuthenticationProvider { id: "github".to_owned(), name: "GitHub".to_owned(), enterprise: false, extra: Default::default() },
        account_name: "testuser".to_owned(),
        session_id: "session-1".to_owned(),
        enterprise,
        entitlements_data,
        extra: Default::default(),
    }
}

/// `{ access_type_sku } as IEntitlementsData`.
fn entitlements_with_sku(access_type_sku: &str) -> IEntitlementsData {
    serde_json::from_value(json!({
        "access_type_sku": access_type_sku,
        "chat_enabled": false,
        "assigned_date": "",
        "can_signup_for_limited": false,
        "copilot_plan": "",
        "organization_login_list": [],
        "analytics_tracking_id": "",
    }))
    .expect("os campos obrigatórios do IEntitlementsData")
}

// Well-known tenant ids used to classify a Microsoft account as work/school (Entra, eligible) vs.
// personal Microsoft Account (MSA, ineligible). Mirrors the production classification in
// `extensionGalleryAccountService.ts`.
const ENTRA_TENANT_ID: &str = "72f988bf-86f1-41af-91ab-2d7cd011db47"; // A work/school (Entra) tenant, eligible.
const MSA_TENANT_ID: &str = "9188040d-6c67-4c5b-b112-36a304b66dad"; // Personal Microsoft Account, ineligible.
const MSA_PASSTHROUGH_TENANT_ID: &str = "f8cdef31-a31e-4b4a-93e4-5f571e91255a"; // MSA pass-through tenant, ineligible.

/// `makeJwt(claims)`: builds a structurally valid (unsigned) JWT whose payload carries `claims`, matching `getClaimsFromJWT`.
fn make_jwt(claims: Value) -> String {
    let encode = |value: Value| encode_base64(&VSBuffer::from_string(&value.to_string()), true, false);
    format!("{}.{}.sig", encode(json!({ "alg": "none", "typ": "JWT" })), encode(claims))
}

/// `createMicrosoftSession(accessToken, accountId, sessionId, tid)`.
///
/// Eligibility is decided locally from the account's ID-token `tid` (tenant) claim, so a session must
/// carry an ID token to be classified.
fn create_microsoft_session_with(access_token: &str, account_id: &str, session_id: &str, tid: &str) -> AuthenticationSession {
    AuthenticationSession {
        id: session_id.to_owned(),
        access_token: access_token.to_owned(),
        account: AuthenticationSessionAccount::new(format!("{account_id}@contoso.com"), account_id),
        scopes: ["openid", "profile", "email", "offline_access"].map(str::to_owned).to_vec(),
        id_token: Some(make_jwt(json!({ "tid": tid, "oid": account_id }))),
    }
}

/// `createMicrosoftSession()`: the tenant defaults to a work/school (Entra) tenant, so eligible.
fn create_microsoft_session() -> AuthenticationSession {
    create_microsoft_session_with("ms-token", "ms-account-1", "ms-session-1", ENTRA_TENANT_ID)
}

/// `createGalleryManifest()`.
fn create_gallery_manifest() -> Value {
    json!({ "version": "1.0", "resources": [], "capabilities": { "extensionQuery": {} } })
}

/// O `IProductService` do `setup` (e dos casos que o trocam): `accessScopes` é o que muda.
fn gallery_product(access_scopes: Option<Vec<String>>) -> Rc<ProductService> {
    Rc::new(ProductService {
        version: "1.0.0".to_owned(),
        name_long: "VS Code Test".to_owned(),
        extensions_gallery: Some(IExtensionsGallery {
            service_url: "https://default-marketplace.example.com".to_owned(),
            control_url: String::new(),
            extension_url_template: String::new(),
            resource_url_template: String::new(),
            nls_base_url: String::new(),
            access_skus: Some(vec!["copilot_business".to_owned()]),
            access_scopes,
            extra: Default::default(),
        }),
        ..ProductService::default()
    })
}

fn default_access_scopes() -> Vec<String> {
    ["openid", "profile", "email", "offline_access"].map(str::to_owned).to_vec()
}

type RequestHandler = Box<dyn Fn(&IRequestOptions) -> Promise<IRequestContext>>;
type SessionsHandler = Rc<dyn Fn(&str, &[String]) -> Promise<Vec<AuthenticationSession>>>;

/// `mock<IRequestService>()` com o `request` sobrescrito.
struct TestRequestService {
    handler: Rc<RefCell<RequestHandler>>,
}

impl IRequestService for TestRequestService {
    fn on_did_complete_request(&self) -> Event<RequestCompleteEvent> {
        not_mocked("onDidCompleteRequest")
    }

    fn request(&self, options: IRequestOptions, _token: CancellationToken) -> Promise<IRequestContext> {
        let handler = self.handler.borrow();
        handler(&options)
    }

    fn resolve_proxy(&self, _url: &str) -> Promise<Option<String>> {
        not_mocked("resolveProxy")
    }

    fn lookup_authorization(&self, _auth_info: AuthInfo) -> Promise<Option<Credentials>> {
        not_mocked("lookupAuthorization")
    }

    fn lookup_kerberos_authorization(&self, _url: &str) -> Promise<Option<String>> {
        not_mocked("lookupKerberosAuthorization")
    }

    fn load_certificates(&self) -> Promise<Vec<String>> {
        not_mocked("loadCertificates")
    }
}

/// `mock<IDefaultAccountService>()` com `onDidChangeDefaultAccount` e `getDefaultAccount` sobrescritos.
struct TestDefaultAccountService {
    on_did_change_default_account: Rc<Emitter<Option<IDefaultAccount>>>,
    default_account: Rc<RefCell<Option<IDefaultAccount>>>,
    /// `getDefaultAccount` lança `Account service unavailable`.
    throws: bool,
}

impl IGalleryDefaultAccountService for TestDefaultAccountService {
    fn on_did_change_default_account(&self) -> Event<Option<IDefaultAccount>> {
        self.on_did_change_default_account.event()
    }

    fn get_default_account(&self) -> Promise<Option<IDefaultAccount>> {
        if self.throws {
            return Promise::rejected(Error::new("Account service unavailable"));
        }
        Promise::resolved(self.default_account.borrow().clone())
    }

    fn sign_in(&self) -> Promise<Option<IDefaultAccount>> {
        not_mocked("signIn")
    }
}

/// `mock<IAuthenticationService>()` com `onDidChangeSessions` e `getSessions` (e, no `setup`, `createSession`) sobrescritos.
struct TestAuthenticationService {
    on_did_change_sessions: Rc<Emitter<AuthenticationSessionsChange>>,
    get_sessions: SessionsHandler,
    /// O `createSession` do `setup` devolve `createMicrosoftSession()`; os dublês dos casos não o definem.
    creates_session: bool,
}

impl IGalleryAuthenticationService for TestAuthenticationService {
    fn on_did_change_sessions(&self) -> Event<AuthenticationSessionsChange> {
        self.on_did_change_sessions.event()
    }

    fn get_accounts(&self, _id: &str) -> Promise<Vec<AuthenticationSessionAccount>> {
        not_mocked("getAccounts")
    }

    fn get_sessions(&self, id: &str, scopes: &[String]) -> Promise<Vec<AuthenticationSession>> {
        (self.get_sessions)(id, scopes)
    }

    fn create_session(&self, _provider_id: &str, _scopes: &[String], _account: Option<&AuthenticationSessionAccount>) -> Promise<AuthenticationSession> {
        if !self.creates_session {
            not_mocked("createSession");
        }
        Promise::resolved(create_microsoft_session())
    }
}

/// `mock<IDialogService>()` com `confirm` sobrescrito: guarda a mensagem e responde `{ confirmed: false }`.
struct TestDialogService {
    restart_prompts: Rc<RefCell<Vec<String>>>,
}

impl IDialogService for TestDialogService {
    fn on_will_show_dialog(&self) -> Event<()> {
        not_mocked("onWillShowDialog")
    }

    fn on_did_show_dialog(&self) -> Event<()> {
        not_mocked("onDidShowDialog")
    }

    fn confirm(&self, confirmation: Confirmation) -> Promise<ConfirmationResult> {
        self.restart_prompts.borrow_mut().push(confirmation.base.message);
        Promise::resolved(ConfirmationResult { checkbox_checked: None, confirmed: false })
    }

    fn prompt(&self, _prompt: Prompt) -> Promise<PromptResult> {
        not_mocked("prompt")
    }

    fn input(&self, _input: Input) -> Promise<InputResult> {
        not_mocked("input")
    }

    fn info(&self, _message: &str, _detail: Option<&str>) -> Promise<()> {
        not_mocked("info")
    }

    fn warn(&self, _message: &str, _detail: Option<&str>) -> Promise<()> {
        not_mocked("warn")
    }

    fn error(&self, _message: &str, _detail: Option<&str>) -> Promise<()> {
        not_mocked("error")
    }

    fn about(&self) -> Promise<()> {
        not_mocked("about")
    }
}

/// `mock<IHostService>()` com `restart` sobrescrito (`async restart() { }`).
struct TestHostRestartService;

impl IHostRestartService for TestHostRestartService {
    fn restart(&self) -> Promise<()> {
        Promise::resolved(())
    }
}

/// `mock<IRemoteAgentService>()` com `getConnection() { return null; }`.
struct NoRemoteConnection;

impl IRemoteAgentConnectionProvider for NoRemoteConnection {
    fn get_connection(&self) -> Option<Rc<dyn IRemoteAgentConnection>> {
        None
    }
}

/// O canal `{ call: () => Promise.resolve(), listen: () => Event.None }`.
struct NoopChannel;

impl IChannel for NoopChannel {
    fn call(&self, _command: &str, _arg: Marshalled, _cancellation_token: CancellationToken) -> Promise<Marshalled> {
        Promise::resolved(Marshalled::Undefined)
    }

    fn listen(&self, _event: &str, _arg: Marshalled) -> Event<Marshalled> {
        Event::none()
    }
}

/// `mock<ISharedProcessService>()` com `getChannel` sobrescrito.
struct TestSharedProcessService;

impl ISharedProcessService for TestSharedProcessService {
    fn get_channel(&self, _channel_name: &str) -> Rc<dyn IChannel> {
        Rc::new(NoopChannel)
    }
}

/// `IQuickInputService`, que o `setup` não registra: o provedor da Microsoft só o usa no `signIn`.
struct UnusedQuickInputService;

impl IQuickInputService for UnusedQuickInputService {
    fn input(&self, _options: Option<&IInputOptions>) -> Promise<Option<String>> {
        not_mocked("input")
    }

    fn pick(&self, _picks: Vec<IQuickPickItem>, _options: Option<&IPickOptions>) -> Promise<Option<IQuickPickItem>> {
        not_mocked("pick")
    }
}

/// `RecordingTelemetryService`: captures emitted telemetry events so tests can assert on event names and dimensions.
struct RecordingTelemetryService {
    inner: NullTelemetryServiceShape,
    events: RefCell<Vec<(String, Option<Dynamic>)>>,
}

impl ITelemetryService for RecordingTelemetryService {
    fn telemetry_level(&self) -> TelemetryLevel {
        self.inner.telemetry_level()
    }

    fn session_id(&self) -> String {
        self.inner.session_id()
    }

    fn machine_id(&self) -> String {
        self.inner.machine_id()
    }

    fn sqm_id(&self) -> String {
        self.inner.sqm_id()
    }

    fn dev_device_id(&self) -> String {
        self.inner.dev_device_id()
    }

    fn first_session_date(&self) -> String {
        self.inner.first_session_date()
    }

    fn send_error_telemetry(&self) -> bool {
        self.inner.send_error_telemetry()
    }

    fn public_log(&self, event_name: &str, data: Option<ITelemetryData>) {
        self.inner.public_log(event_name, data);
    }

    fn public_log2(&self, event_name: &str, data: Option<Dynamic>) {
        if !event_name.is_empty() {
            self.events.borrow_mut().push((event_name.to_owned(), data));
        }
    }

    fn public_log_error(&self, error_event_name: &str, data: Option<ITelemetryData>) {
        self.inner.public_log_error(error_event_name, data);
    }

    fn public_log_error2(&self, event_name: &str, data: Option<Dynamic>) {
        self.inner.public_log_error2(event_name, data);
    }

    fn set_experiment_property(&self, name: &str, value: &str) {
        self.inner.set_experiment_property(name, value);
    }

    fn set_common_property(&self, name: &str, value: Option<CommonPropertyValue>) {
        self.inner.set_common_property(name, value);
    }
}

/// `ConfigurationModel` vazio, para a configuração do evento de mudança.
fn empty_model() -> ConfigurationModel {
    ConfigurationModel::create_empty_model(NullLogService::shared())
}

/// Uma `Configuration` vazia, a que o evento de mudança consulta.
fn empty_configuration() -> Configuration {
    Configuration::new(
        empty_model(),
        empty_model(),
        empty_model(),
        empty_model(),
        empty_model(),
        empty_model(),
        ResourceMap::new(),
        empty_model(),
        ResourceMap::new(),
        NullLogService::shared(),
    )
}

/// `TestInstantiationService` com o `setup` da suíte: os serviços registrados, os emissores `onDidChangeDefaultAccount` e
/// `onDidChangeSessions`, e as variáveis `requestHandler`, `defaultAccount`, `microsoftSessions`, `storageData` e `restartPrompts`.
struct Fixture {
    /// `disposableStore`.
    guard: LeakGuard,
    on_did_change_default_account: Rc<Emitter<Option<IDefaultAccount>>>,
    on_did_change_sessions: Rc<Emitter<AuthenticationSessionsChange>>,
    request_handler: Rc<RefCell<RequestHandler>>,
    default_account: Rc<RefCell<Option<IDefaultAccount>>>,
    microsoft_sessions: Rc<RefCell<Vec<AuthenticationSession>>>,
    configuration: Rc<TestConfigurationService>,
    /// `storageData`, como o armazenamento que o serviço lê e grava.
    storage: Rc<dyn StorageService>,
    telemetry: Rc<RecordingTelemetryService>,
    restart_prompts: Rc<RefCell<Vec<String>>>,
    environment: Rc<dyn EnvironmentService>,
    file_service: Rc<je_platform::files::file_service::FileService>,
    /// `instantiationService.stub(IProductService, ...)`.
    product: RefCell<Rc<ProductService>>,
    /// `instantiationService.stub(IAuthenticationService, ...)`.
    authentication_service: RefCell<Rc<dyn IGalleryAuthenticationService>>,
    /// `instantiationService.stub(IDefaultAccountService, ...)`.
    default_account_service: RefCell<Rc<dyn IGalleryDefaultAccountService>>,
}

impl Fixture {
    /// O `setup`.
    fn new() -> Rc<Self> {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();

        let on_did_change_default_account: Rc<Emitter<Option<IDefaultAccount>>> = guard.add_rc(Emitter::new());
        let on_did_change_sessions: Rc<Emitter<AuthenticationSessionsChange>> = guard.add_rc(Emitter::new());

        let default_account: Rc<RefCell<Option<IDefaultAccount>>> = Rc::new(RefCell::new(None));
        let microsoft_sessions: Rc<RefCell<Vec<AuthenticationSession>>> = Rc::new(RefCell::new(Vec::new()));
        let request_handler: Rc<RefCell<RequestHandler>> = Rc::new(RefCell::new(Box::new(|_| Promise::resolved(mock_response(200, create_gallery_manifest())))));

        let configuration = TestConfigurationService::new(Some(Dynamic::object([(EXTENSION_GALLERY_SERVICE_URL_CONFIG_KEY, Dynamic::string("https://marketplace.example.com"))])));
        guard.add_rc(configuration.on_did_change_configuration_emitter.clone());

        let storage_core = guard.add_rc(InMemoryStorageService::new());
        let storage: Rc<dyn StorageService> = Rc::new(storage_core);

        let sessions_of_microsoft = microsoft_sessions.clone();
        let authentication_service: Rc<dyn IGalleryAuthenticationService> = Rc::new(TestAuthenticationService {
            on_did_change_sessions: on_did_change_sessions.clone(),
            get_sessions: Rc::new(move |provider_id, _scopes| {
                if provider_id == "microsoft" {
                    return Promise::resolved(sessions_of_microsoft.borrow().clone());
                }
                Promise::resolved(Vec::new())
            }),
            creates_session: true,
        });
        let default_account_service: Rc<dyn IGalleryDefaultAccountService> = Rc::new(TestDefaultAccountService {
            on_did_change_default_account: on_did_change_default_account.clone(),
            default_account: default_account.clone(),
            throws: false,
        });

        let environment: Rc<dyn EnvironmentService> = TestEnvironmentService::new();
        let file_service = guard.add_rc(je_platform::files::file_service::FileService::new(NullLogService::shared()));

        Rc::new(Self {
            guard,
            on_did_change_default_account,
            on_did_change_sessions,
            request_handler,
            default_account,
            microsoft_sessions,
            configuration,
            storage,
            telemetry: Rc::new(RecordingTelemetryService { inner: NullTelemetryServiceShape, events: RefCell::new(Vec::new()) }),
            restart_prompts: Rc::new(RefCell::new(Vec::new())),
            environment,
            file_service,
            product: RefCell::new(gallery_product(Some(default_access_scopes()))),
            authentication_service: RefCell::new(authentication_service),
            default_account_service: RefCell::new(default_account_service),
        })
    }

    /// `requestHandler = ...`.
    fn set_request_handler(&self, handler: impl Fn(&IRequestOptions) -> Promise<IRequestContext> + 'static) {
        *self.request_handler.borrow_mut() = Box::new(handler);
    }

    /// `defaultAccount = ...`.
    fn set_default_account(&self, account: Option<IDefaultAccount>) {
        *self.default_account.borrow_mut() = account;
    }

    /// `microsoftSessions = ...`.
    fn set_microsoft_sessions(&self, sessions: Vec<AuthenticationSession>) {
        *self.microsoft_sessions.borrow_mut() = sessions;
    }

    /// `configurationService.setUserConfiguration(key, value)`.
    fn set_configuration(&self, key: &str, value: &str) {
        self.configuration.set_user_configuration(key, Dynamic::string(value), None);
    }

    /// `instantiationService.stub(IProductService, ...)`.
    fn stub_product(&self, product: Rc<ProductService>) {
        *self.product.borrow_mut() = product;
    }

    /// `instantiationService.stub(IAuthenticationService, { onDidChangeSessions, getSessions })`.
    fn stub_get_sessions(&self, get_sessions: impl Fn(&str, &[String]) -> Promise<Vec<AuthenticationSession>> + 'static) {
        *self.authentication_service.borrow_mut() = Rc::new(TestAuthenticationService {
            on_did_change_sessions: self.on_did_change_sessions.clone(),
            get_sessions: Rc::new(get_sessions),
            creates_session: false,
        });
    }

    /// `instantiationService.stub(IDefaultAccountService, { onDidChangeDefaultAccount, getDefaultAccount() { throw } })`.
    fn stub_default_account_service_throwing(&self) {
        *self.default_account_service.borrow_mut() = Rc::new(TestDefaultAccountService {
            on_did_change_default_account: self.on_did_change_default_account.clone(),
            default_account: self.default_account.clone(),
            throws: true,
        });
    }

    /// `createService()`: built here (not in setup) so the provider is chosen after each test sets the config;
    /// registered to the store because it is injected, not owned by the manifest service.
    fn create_service(&self) -> Rc<WorkbenchExtensionGalleryManifestService> {
        let account_service = self.guard.add_rc(ExtensionGalleryAccountService::new());
        let product: Rc<dyn IProductService> = self.product.borrow().clone();
        let telemetry: Rc<RecordingTelemetryService> = self.telemetry.clone();
        let log_service = NullLogService::shared();
        // Play the role of the production contribution, which builds the auth-dependent provider
        // outside the service graph and hands it over.
        let provider: Rc<dyn IExtensionGalleryAccountProvider> = if uses_microsoft_auth_provider(&*self.configuration) {
            self.guard.add_rc(Rc::new(MicrosoftGalleryAccountProvider::new(
                self.authentication_service.borrow().clone(),
                Rc::new(UnusedQuickInputService),
                product.clone(),
                self.storage.clone(),
                telemetry.clone(),
                log_service.clone(),
            )))
        } else {
            self.guard.add_rc(Rc::new(GitHubGalleryAccountProvider::new(self.default_account_service.borrow().clone(), product.clone(), telemetry.clone(), log_service.clone())))
        };
        account_service.set_account_provider(provider);
        let gallery_account_service: Rc<dyn IExtensionGalleryAccountService> = account_service;

        self.guard.add_rc(WorkbenchExtensionGalleryManifestService::new(WorkbenchExtensionGalleryManifestServiceParts {
            product_service: product,
            environment_service: self.environment.clone(),
            file_service: self.file_service.clone(),
            telemetry_service: telemetry,
            storage_service: self.storage.clone(),
            remote_agent_service: Rc::new(NoRemoteConnection),
            shared_process_service: Rc::new(TestSharedProcessService),
            configuration_service: self.configuration.clone(),
            request_service: Rc::new(TestRequestService { handler: self.request_handler.clone() }),
            gallery_account_service,
            log_service,
            dialog_service: Rc::new(TestDialogService { restart_prompts: self.restart_prompts.clone() }),
            host_service: Rc::new(TestHostRestartService),
        }))
    }

    /// `onDidChangeSessions.fire({ providerId: 'microsoft', label: 'Microsoft', event: { added: [], removed: [], changed: [] } })`.
    fn fire_microsoft_sessions_changed(&self) {
        self.on_did_change_sessions.fire(AuthenticationSessionsChange {
            provider_id: "microsoft".to_owned(),
            label: "Microsoft".to_owned(),
            event: AuthenticationSessionsChangeEvent { added: Some(Vec::new()), removed: Some(Vec::new()), changed: Some(Vec::new()) },
        });
    }

    /// `fireConfigChange(...keys)`.
    fn fire_config_change(&self, keys: &[&str]) {
        let change = ConfigurationChange { keys: keys.iter().map(|key| JsString::from(*key)).collect(), overrides: Vec::new() };
        let event = ConfigurationChangeEvent::new(change, None, Rc::new(empty_configuration()), None, NullLogService::shared());
        self.configuration.on_did_change_configuration_emitter.fire(Rc::new(event));
    }

    /// `storageData.get('marketplace.account')`.
    fn stored_account(&self) -> Option<String> {
        self.storage.get("marketplace.account", StorageScope::Application, None)
    }

    /// `storageData.set('marketplace.account', value)`.
    fn store_account(&self, value: &str) {
        self.storage.store("marketplace.account", StorageValue::from(value), StorageScope::Application, StorageTarget::Machine);
    }

    /// `authCheckedEvents()`: o dado de cada evento como o JSON que o `deepStrictEqual` do original compara (o objeto de
    /// [`Dynamic`] compara por identidade).
    fn auth_checked_events(&self) -> Vec<Option<String>> {
        self.telemetry
            .events
            .borrow()
            .iter()
            .filter(|(name, _)| name == "marketplace:auth:checked")
            .map(|(_, data)| data.as_ref().and_then(json_stringify))
            .collect()
    }

    /// `customMarketplaceCount()`.
    fn custom_marketplace_count(&self) -> usize {
        self.telemetry.events.borrow().iter().filter(|(name, _)| name == "galleryservice:custom:marketplace").count()
    }
}

/// Roda o caso: o `setup`, o corpo, e o `teardown` (descartar o `disposableStore`, que confere os vazamentos ao soltar o `Fixture`).
fn run<F: Future<Output = ()> + 'static>(body: impl FnOnce(Rc<Fixture>) -> F) {
    let fixture = Fixture::new();
    run_with_faked_timers(body(fixture.clone()));
}

/// `await service.getExtensionGalleryManifest()`.
async fn get_manifest(service: &WorkbenchExtensionGalleryManifestService) -> Option<je_platform::extension_management::extension_gallery_manifest::IExtensionGalleryManifest> {
    service.get_extension_gallery_manifest().await.expect("getExtensionGalleryManifest resolve")
}

/// `await new Promise(resolve => setTimeout(resolve, 0))`.
async fn tick() {
    timeout(0).await.expect("timeout resolve");
}

fn assert_status(service: &WorkbenchExtensionGalleryManifestService, expected: ExtensionGalleryManifestStatus) {
    assert_eq!(service.extension_gallery_manifest_status(), expected);
}

/// Um contador de requisições ao índice, e o `requestHandler` que o incrementa e responde com `respond`.
fn count_index_requests(fixture: &Fixture, respond: impl Fn() -> IRequestContext + 'static) -> Rc<Cell<usize>> {
    let index_requests = Rc::new(Cell::new(0usize));
    let counter = index_requests.clone();
    fixture.set_request_handler(move |_| {
        counter.set(counter.get() + 1);
        Promise::resolved(respond())
    });
    index_requests
}

// ---------------------------------------------------------------------------------------------------------------------
// Os casos da suíte (no original, `WorkbenchExtensionGalleryManifestService`).
// ---------------------------------------------------------------------------------------------------------------------

// --- Provider routing ---

#[test]
fn test_github_provider_enterprise_account_available() {
    run(|f| async move {
        f.set_configuration(EXTENSION_GALLERY_AUTH_PROVIDER_CONFIG_KEY, "github");
        f.set_default_account(Some(create_default_account(true, None)));

        let service = f.create_service();
        get_manifest(&service).await;

        assert_status(&service, ExtensionGalleryManifestStatus::Available);
    });
}

#[test]
fn test_github_provider_no_account_requires_sign_in() {
    run(|f| async move {
        f.set_configuration(EXTENSION_GALLERY_AUTH_PROVIDER_CONFIG_KEY, "github");
        f.set_default_account(None);

        let service = f.create_service();
        get_manifest(&service).await;

        assert_status(&service, ExtensionGalleryManifestStatus::RequiresSignIn);
    });
}

#[test]
fn test_github_provider_non_enterprise_account_without_sku_access_denied() {
    run(|f| async move {
        f.set_configuration(EXTENSION_GALLERY_AUTH_PROVIDER_CONFIG_KEY, "github");
        f.set_default_account(Some(create_default_account(false, None)));

        let service = f.create_service();
        get_manifest(&service).await;

        assert_status(&service, ExtensionGalleryManifestStatus::AccessDenied);
    });
}

#[test]
fn test_github_provider_a_denied_account_that_is_later_granted_entitlement_becomes_available() {
    run(|f| async move {
        // A denial is a verdict about the account as it is now, not a durable one: nothing may
        // outlive the condition that produced it and keep the user locked out after it changes.
        f.set_configuration(EXTENSION_GALLERY_AUTH_PROVIDER_CONFIG_KEY, "github");
        f.set_default_account(Some(create_default_account(false, None)));
        f.set_request_handler(|_| Promise::resolved(mock_response(200, create_gallery_manifest())));

        let service = f.create_service();
        get_manifest(&service).await;
        assert_status(&service, ExtensionGalleryManifestStatus::AccessDenied);

        // The same account gains entitlement, with no sign-out in between.
        let granted = create_default_account(true, None);
        f.set_default_account(Some(granted.clone()));
        f.on_did_change_default_account.fire(Some(granted));
        tick().await;

        assert_status(&service, ExtensionGalleryManifestStatus::Available);
    });
}

#[test]
fn test_github_provider_account_with_matching_sku_available() {
    run(|f| async move {
        f.set_configuration(EXTENSION_GALLERY_AUTH_PROVIDER_CONFIG_KEY, "github");
        f.set_default_account(Some(create_default_account(false, Some(entitlements_with_sku("copilot_business")))));

        let service = f.create_service();
        get_manifest(&service).await;

        assert_status(&service, ExtensionGalleryManifestStatus::Available);
    });
}

#[test]
fn test_default_no_auth_provider_uses_github_path() {
    run(|f| async move {
        // No authProvider config set
        f.set_default_account(Some(create_default_account(true, None)));

        let service = f.create_service();
        get_manifest(&service).await;

        assert_status(&service, ExtensionGalleryManifestStatus::Available);
    });
}

#[test]
fn test_microsoft_provider_no_session_requires_sign_in() {
    run(|f| async move {
        f.set_configuration(EXTENSION_GALLERY_AUTH_PROVIDER_CONFIG_KEY, "microsoft");
        f.set_microsoft_sessions(Vec::new());

        let service = f.create_service();
        get_manifest(&service).await;

        assert_status(&service, ExtensionGalleryManifestStatus::RequiresSignIn);
    });
}

#[test]
fn test_microsoft_provider_eligible_work_school_session_available() {
    run(|f| async move {
        f.set_configuration(EXTENSION_GALLERY_AUTH_PROVIDER_CONFIG_KEY, "microsoft");
        // A work/school (Entra) tenant is eligible; the index is then fetched with the session token.
        f.set_microsoft_sessions(vec![create_microsoft_session()]);
        f.set_request_handler(|_| Promise::resolved(mock_response(200, create_gallery_manifest())));

        let service = f.create_service();
        get_manifest(&service).await;

        assert_status(&service, ExtensionGalleryManifestStatus::Available);
    });
}

#[test]
fn test_microsoft_provider_personal_msa_account_access_denied_without_touching_the_index() {
    run(|f| async move {
        f.set_configuration(EXTENSION_GALLERY_AUTH_PROVIDER_CONFIG_KEY, "microsoft");
        // A personal Microsoft Account (MSA) is ineligible. The verdict is decided locally from the
        // token's tenant claim BEFORE any index fetch, so an ineligible account never probes the index.
        f.set_microsoft_sessions(vec![create_microsoft_session_with("ms-token", "ms-account-1", "ms-session-1", MSA_TENANT_ID)]);
        let index_requests = count_index_requests(&f, || mock_response(200, create_gallery_manifest()));

        let service = f.create_service();
        get_manifest(&service).await;

        assert_status(&service, ExtensionGalleryManifestStatus::AccessDenied);
        assert_eq!(index_requests.get(), 0);
    });
}

#[test]
fn test_microsoft_provider_msa_pass_through_tenant_access_denied_without_touching_the_index() {
    run(|f| async move {
        f.set_configuration(EXTENSION_GALLERY_AUTH_PROVIDER_CONFIG_KEY, "microsoft");
        // The MSA pass-through tenant is also classified as a personal account, so ineligible.
        f.set_microsoft_sessions(vec![create_microsoft_session_with("ms-token", "ms-account-1", "ms-session-1", MSA_PASSTHROUGH_TENANT_ID)]);
        let index_requests = count_index_requests(&f, || mock_response(200, create_gallery_manifest()));

        let service = f.create_service();
        get_manifest(&service).await;

        assert_status(&service, ExtensionGalleryManifestStatus::AccessDenied);
        assert_eq!(index_requests.get(), 0);
    });
}

#[test]
fn test_microsoft_provider_no_id_token_access_token_carries_tenant_eligible_fallback() {
    run(|f| async move {
        f.set_configuration(EXTENSION_GALLERY_AUTH_PROVIDER_CONFIG_KEY, "microsoft");
        // The ID token is preferred, but when it is absent the access token is decoded as a fallback.
        // Here the access token is a JWT carrying a work/school tenant, so eligible.
        let access_token = make_jwt(json!({ "tid": ENTRA_TENANT_ID, "oid": "ms-account-1" }));
        f.set_microsoft_sessions(vec![AuthenticationSession { id_token: None, ..create_microsoft_session_with(&access_token, "ms-account-1", "ms-session-1", ENTRA_TENANT_ID) }]);
        f.set_request_handler(|_| Promise::resolved(mock_response(200, create_gallery_manifest())));

        let service = f.create_service();
        get_manifest(&service).await;

        assert_status(&service, ExtensionGalleryManifestStatus::Available);
    });
}

#[test]
fn test_microsoft_provider_undecodable_token_access_denied_without_touching_the_index() {
    run(|f| async move {
        f.set_configuration(EXTENSION_GALLERY_AUTH_PROVIDER_CONFIG_KEY, "microsoft");
        // An opaque/undecodable token cannot confirm a work/school account, so it is treated as
        // ineligible rather than wrongly granting access, and the index is never probed.
        f.set_microsoft_sessions(vec![AuthenticationSession { id_token: Some("not-a-jwt".to_owned()), ..create_microsoft_session() }]);
        let index_requests = count_index_requests(&f, || mock_response(200, create_gallery_manifest()));

        let service = f.create_service();
        get_manifest(&service).await;

        assert_status(&service, ExtensionGalleryManifestStatus::AccessDenied);
        assert_eq!(index_requests.get(), 0);
    });
}

#[test]
fn test_microsoft_provider_token_without_a_tenant_claim_access_denied_without_touching_the_index() {
    run(|f| async move {
        f.set_configuration(EXTENSION_GALLERY_AUTH_PROVIDER_CONFIG_KEY, "microsoft");
        // A decodable token that carries no `tid` cannot be confirmed as a work/school account, so it
        // is treated as ineligible.
        f.set_microsoft_sessions(vec![AuthenticationSession { id_token: Some(make_jwt(json!({ "oid": "ms-account-1" }))), ..create_microsoft_session() }]);
        let index_requests = count_index_requests(&f, || mock_response(200, create_gallery_manifest()));

        let service = f.create_service();
        get_manifest(&service).await;

        assert_status(&service, ExtensionGalleryManifestStatus::AccessDenied);
        assert_eq!(index_requests.get(), 0);
    });
}

#[test]
fn test_microsoft_provider_service_index_returns_500_with_json_body_access_denied_not_parsed_as_manifest() {
    run(|f| async move {
        f.set_configuration(EXTENSION_GALLERY_AUTH_PROVIDER_CONFIG_KEY, "microsoft");
        f.set_microsoft_sessions(vec![create_microsoft_session()]);
        // A 5xx error body is valid JSON and truthy; it must be rejected outright rather than
        // mistaken for a manifest.
        f.set_request_handler(|_| Promise::resolved(mock_response(500, json!({ "error": "internal" }))));

        let service = f.create_service();
        get_manifest(&service).await;

        assert_status(&service, ExtensionGalleryManifestStatus::AccessDenied);
    });
}

/// O `for (const [scenario, body] of [...])`: cada cenário monta o seu `Fixture`, como cada `test` do original.
#[test]
fn test_github_provider_eligible_account_200_with_an_invalid_manifest_access_denied() {
    let manifest = create_gallery_manifest();
    let scenarios: Vec<(&str, Value)> = vec![
        ("with a non-manifest body", json!({ "foo": "bar" })),
        ("with malformed resources", {
            let mut body = manifest.clone();
            body["resources"] = json!([{}]);
            body
        }),
        ("without capabilities", json!({ "version": "1.0", "resources": [] })),
        ("without the extensionQuery capability", {
            let mut body = manifest.clone();
            body["capabilities"] = json!({});
            body
        }),
        ("with malformed extensionQuery capabilities", {
            let mut body = manifest.clone();
            body["capabilities"] = json!({ "extensionQuery": { "filtering": [{}] } });
            body
        }),
    ];
    for (scenario, body) in scenarios {
        run(move |f| async move {
            f.set_default_account(Some(create_default_account(true, None)));
            f.set_request_handler(move |_| Promise::resolved(mock_response(200, body.clone())));

            let service = f.create_service();
            get_manifest(&service).await;

            assert_eq!(service.extension_gallery_manifest_status(), ExtensionGalleryManifestStatus::AccessDenied, "GitHub provider, eligible account, 200 {scenario}");
        });
    }
}

#[test]
fn test_github_provider_eligible_account_200_without_a_version_available() {
    run(|f| async move {
        f.set_default_account(Some(create_default_account(true, None)));
        f.set_request_handler(|_| Promise::resolved(mock_response(200, json!({ "resources": [], "capabilities": { "extensionQuery": {} } }))));

        let service = f.create_service();
        let manifest = get_manifest(&service).await;

        assert_eq!(
            (service.extension_gallery_manifest_status(), manifest.map(|manifest| manifest.version)),
            (ExtensionGalleryManifestStatus::Available, Some(String::new()))
        );
    });
}

#[test]
fn test_microsoft_provider_manifest_fetch_fails_access_denied() {
    run(|f| async move {
        f.set_configuration(EXTENSION_GALLERY_AUTH_PROVIDER_CONFIG_KEY, "microsoft");
        f.set_microsoft_sessions(vec![create_microsoft_session()]);
        // Manifest discovery fails transiently (network error)
        f.set_request_handler(|_| Promise::rejected(Error::new("network down")));

        let service = f.create_service();
        get_manifest(&service).await;

        // A configured marketplace whose manifest can't be fetched is reported as denied, as on main.
        assert_status(&service, ExtensionGalleryManifestStatus::AccessDenied);
    });
}

#[test]
fn test_microsoft_provider_no_session_requires_sign_in_without_probing_the_service_index() {
    run(|f| async move {
        f.set_configuration(EXTENSION_GALLERY_AUTH_PROVIDER_CONFIG_KEY, "microsoft");
        f.set_microsoft_sessions(Vec::new());
        // When 'microsoft' is configured and there is no session, we must NOT issue an
        // anonymous request to the (possibly auth-gated) service index: that request is a
        // guaranteed 401. We go straight to sign-in and only touch the index once a token
        // exists. Assert no request was made.
        let index_requests = count_index_requests(&f, || mock_response(401, json!({ "message": "auth required" })));

        let service = f.create_service();
        get_manifest(&service).await;

        assert_status(&service, ExtensionGalleryManifestStatus::RequiresSignIn);
        assert_eq!(index_requests.get(), 0);
    });
}

#[test]
fn test_microsoft_provider_no_session_stays_requires_sign_in_even_when_the_index_would_fail() {
    run(|f| async move {
        // Pins the invariant that makes "not signed in" a stable, actionable state: with no session
        // the index is never probed, so a failing marketplace cannot mask the sign-in affordance.
        // Covers the post-startup re-validation triggered when authentication connects.
        f.set_configuration(EXTENSION_GALLERY_AUTH_PROVIDER_CONFIG_KEY, "microsoft");
        f.set_microsoft_sessions(Vec::new());
        let index_requests = count_index_requests(&f, || mock_response(400, json!({ "message": "client rejected" })));

        let service = f.create_service();
        get_manifest(&service).await;
        assert_status(&service, ExtensionGalleryManifestStatus::RequiresSignIn);

        // A session change arrives (as it does when the Microsoft provider registers post-startup)
        // and triggers a re-validation.
        f.fire_microsoft_sessions_changed();
        tick().await;

        assert_status(&service, ExtensionGalleryManifestStatus::RequiresSignIn);
        assert_eq!(index_requests.get(), 0);
    });
}

#[test]
fn test_microsoft_provider_service_index_rejects_the_client_400_access_denied() {
    run(|f| async move {
        // A marketplace that refuses this client outright (e.g. below its minimum supported
        // version) is a durable rejection, so retrying cannot help. `main` reports any failed
        // fetch of a configured marketplace as AccessDenied ("contact your administrator"); keep
        // that for this case rather than the transient "check your network connection".
        f.set_configuration(EXTENSION_GALLERY_AUTH_PROVIDER_CONFIG_KEY, "microsoft");
        f.set_microsoft_sessions(vec![create_microsoft_session()]);
        f.set_request_handler(|_| Promise::resolved(mock_response(400, json!({ "message": "Only VS Code clients version 1.104.2 or later are allowed." }))));

        let service = f.create_service();
        get_manifest(&service).await;

        assert_status(&service, ExtensionGalleryManifestStatus::AccessDenied);
    });
}

#[test]
fn test_microsoft_single_signed_in_account_no_stored_preference_adopted_and_persisted() {
    run(|f| async move {
        f.set_configuration(EXTENSION_GALLERY_AUTH_PROVIDER_CONFIG_KEY, "microsoft");
        // Exactly one signed-in account and no remembered choice: the selection is unambiguous, so
        // it is adopted for the check AND persisted so later windows reuse the same account instead
        // of re-deriving it.
        f.set_microsoft_sessions(vec![create_microsoft_session()]);
        f.set_request_handler(|_| Promise::resolved(mock_response(200, create_gallery_manifest())));

        let service = f.create_service();
        get_manifest(&service).await;

        assert_status(&service, ExtensionGalleryManifestStatus::Available);
        let stored: Value = serde_json::from_str(&f.stored_account().expect("a conta foi guardada")).expect("o JSON da conta");
        assert_eq!(stored, json!({ "authProvider": "microsoft", "id": "ms-account-1" }));
    });
}

#[test]
fn test_microsoft_multiple_signed_in_accounts_no_stored_preference_requires_sign_in_never_guesses() {
    run(|f| async move {
        f.set_configuration(EXTENSION_GALLERY_AUTH_PROVIDER_CONFIG_KEY, "microsoft");
        // Several accounts are signed in and none was ever chosen. Picking one arbitrarily could grant
        // access under the wrong identity, so selection is refused: no index request is made, no
        // account is persisted, and the user is sent to an explicit sign-in.
        f.set_microsoft_sessions(vec![
            create_microsoft_session_with("token-1", "ms-account-1", "ms-session-1", ENTRA_TENANT_ID),
            create_microsoft_session_with("token-2", "ms-account-2", "ms-session-2", ENTRA_TENANT_ID),
        ]);
        let index_requests = count_index_requests(&f, || mock_response(200, create_gallery_manifest()));

        let service = f.create_service();
        get_manifest(&service).await;

        assert_status(&service, ExtensionGalleryManifestStatus::RequiresSignIn);
        assert_eq!(index_requests.get(), 0);
        assert!(f.stored_account().is_none());
    });
}

#[test]
fn test_microsoft_multiple_accounts_stored_preference_selects_that_account_not_sessions_0() {
    run(|f| async move {
        f.set_configuration(EXTENSION_GALLERY_AUTH_PROVIDER_CONFIG_KEY, "microsoft");
        // Two accounts signed in. The first is a personal account (ineligible), the remembered choice
        // is the second (eligible). Landing on Available therefore proves the remembered account was
        // used; picking `sessions[0]` would have produced AccessDenied.
        f.store_account(&json!({ "authProvider": "microsoft", "id": "ms-account-2" }).to_string());
        f.set_microsoft_sessions(vec![
            create_microsoft_session_with("token-1", "ms-account-1", "ms-session-1", MSA_TENANT_ID),
            create_microsoft_session_with("token-2", "ms-account-2", "ms-session-2", ENTRA_TENANT_ID),
        ]);
        f.set_request_handler(|_| Promise::resolved(mock_response(200, create_gallery_manifest())));

        let service = f.create_service();
        get_manifest(&service).await;

        assert_status(&service, ExtensionGalleryManifestStatus::Available);
    });
}

#[test]
fn test_microsoft_stored_preference_no_longer_signed_in_several_remain_requires_sign_in_no_silent_switch() {
    run(|f| async move {
        f.set_configuration(EXTENSION_GALLERY_AUTH_PROVIDER_CONFIG_KEY, "microsoft");
        // The remembered account is gone, but two others remain. Rather than silently switching to a
        // different identity, selection is refused and the user must choose again.
        f.store_account(&json!({ "authProvider": "microsoft", "id": "ms-account-gone" }).to_string());
        f.set_microsoft_sessions(vec![
            create_microsoft_session_with("token-1", "ms-account-1", "ms-session-1", ENTRA_TENANT_ID),
            create_microsoft_session_with("token-2", "ms-account-2", "ms-session-2", ENTRA_TENANT_ID),
        ]);
        let index_requests = count_index_requests(&f, || mock_response(200, create_gallery_manifest()));

        let service = f.create_service();
        get_manifest(&service).await;

        assert_status(&service, ExtensionGalleryManifestStatus::RequiresSignIn);
        assert_eq!(index_requests.get(), 0);
    });
}

#[test]
fn test_github_provider_eligible_account_manifest_fetch_fails_access_denied() {
    run(|f| async move {
        f.set_configuration(EXTENSION_GALLERY_AUTH_PROVIDER_CONFIG_KEY, "github");
        f.set_default_account(Some(create_default_account(true, None)));
        f.set_request_handler(|_| Promise::rejected(Error::new("network down")));

        let service = f.create_service();
        get_manifest(&service).await;

        assert_status(&service, ExtensionGalleryManifestStatus::AccessDenied);
    });
}

#[test]
fn test_auth_provider_is_matched_case_sensitively_a_differently_cased_value_uses_the_github_path() {
    run(|f| async move {
        f.set_configuration(EXTENSION_GALLERY_AUTH_PROVIDER_CONFIG_KEY, "Microsoft");
        f.set_microsoft_sessions(vec![create_microsoft_session()]);
        f.set_default_account(Some(create_default_account(true, None)));
        f.set_request_handler(|_| Promise::resolved(mock_response(200, create_gallery_manifest())));

        let service = f.create_service();
        get_manifest(&service).await;

        // Not the 'microsoft' literal, so GitHub path with enterprise account, so Available
        assert_status(&service, ExtensionGalleryManifestStatus::Available);
    });
}

#[test]
fn test_microsoft_product_json_access_scopes_are_the_scopes_requested() {
    run(|f| async move {
        f.set_configuration(EXTENSION_GALLERY_AUTH_PROVIDER_CONFIG_KEY, "microsoft");
        f.stub_product(gallery_product(Some(vec!["api://marketplace.example.com/.default".to_owned(), "offline_access".to_owned()])));
        let requested_scopes: Rc<RefCell<Option<Vec<String>>>> = Rc::new(RefCell::new(None));
        let recorded = requested_scopes.clone();
        f.stub_get_sessions(move |_provider_id, scopes| {
            *recorded.borrow_mut() = Some(scopes.to_vec());
            Promise::resolved(vec![create_microsoft_session()])
        });

        let service = f.create_service();
        get_manifest(&service).await;

        assert_eq!(*requested_scopes.borrow(), Some(vec!["api://marketplace.example.com/.default".to_owned(), "offline_access".to_owned()]));
    });
}

#[test]
fn test_microsoft_no_access_scopes_configured_no_session_is_requested_and_access_is_not_granted() {
    run(|f| async move {
        f.set_configuration(EXTENSION_GALLERY_AUTH_PROVIDER_CONFIG_KEY, "microsoft");
        f.stub_product(gallery_product(None));
        let sessions_requested = Rc::new(Cell::new(false));
        let requested = sessions_requested.clone();
        f.stub_get_sessions(move |_provider_id, _scopes| {
            requested.set(true);
            Promise::resolved(vec![create_microsoft_session()])
        });

        let service = f.create_service();
        get_manifest(&service).await;

        // An unconfigured deployment must not fall back to scopes it did not ask for, and an
        // eligible session must not be adopted on the strength of a guess.
        assert!(!sessions_requested.get());
        assert_status(&service, ExtensionGalleryManifestStatus::RequiresSignIn);
    });
}

#[test]
fn test_microsoft_get_sessions_throws_requires_sign_in_not_silent_unavailable() {
    run(|f| async move {
        f.set_configuration(EXTENSION_GALLERY_AUTH_PROVIDER_CONFIG_KEY, "microsoft");
        f.stub_get_sessions(|_provider_id, _scopes| Promise::rejected(Error::new("Auth service unavailable")));

        let service = f.create_service();
        get_manifest(&service).await;

        // The account couldn't be resolved: the configured marketplace must report a definite
        // state rather than a blank (Unavailable) view.
        assert_status(&service, ExtensionGalleryManifestStatus::RequiresSignIn);
    });
}

#[test]
fn test_github_get_default_account_throws_requires_sign_in_not_silent_unavailable() {
    run(|f| async move {
        f.set_configuration(EXTENSION_GALLERY_AUTH_PROVIDER_CONFIG_KEY, "github");
        f.stub_default_account_service_throwing();

        let service = f.create_service();
        get_manifest(&service).await;

        assert_status(&service, ExtensionGalleryManifestStatus::RequiresSignIn);
    });
}

// --- Already-available marketplace ---

#[test]
fn test_microsoft_switching_to_a_different_eligible_account_publishes_that_account_catalog() {
    run(|f| async move {
        f.set_configuration(EXTENSION_GALLERY_AUTH_PROVIDER_CONFIG_KEY, "microsoft");
        f.set_microsoft_sessions(vec![create_microsoft_session_with("token-a", "ms-account-1", "ms-session-1", ENTRA_TENANT_ID)]);
        f.set_request_handler(|_| {
            let mut body = create_gallery_manifest();
            body["resources"] = json!([{ "id": "tenantA", "type": "ExtensionQueryService" }]);
            Promise::resolved(mock_response(200, body))
        });

        let service = f.create_service();
        let first = get_manifest(&service).await;
        assert_status(&service, ExtensionGalleryManifestStatus::Available);
        assert_eq!(first.expect("manifesto").resources[0].id, "tenantA");

        // A private marketplace is account-scoped, so a different eligible account can be served a
        // different catalog. The already-available status must not suppress the new one.
        f.set_microsoft_sessions(vec![create_microsoft_session_with("token-b", "ms-account-2", "ms-session-2", ENTRA_TENANT_ID)]);
        f.set_request_handler(|_| {
            let mut body = create_gallery_manifest();
            body["resources"] = json!([{ "id": "tenantB", "type": "ExtensionQueryService" }]);
            Promise::resolved(mock_response(200, body))
        });
        f.fire_microsoft_sessions_changed();
        tick().await;

        let second = get_manifest(&service).await;
        assert_eq!(second.expect("manifesto").resources[0].id, "tenantB");
    });
}

#[test]
fn test_microsoft_a_transient_auth_failure_does_not_downgrade_an_available_marketplace() {
    run(|f| async move {
        f.set_configuration(EXTENSION_GALLERY_AUTH_PROVIDER_CONFIG_KEY, "microsoft");
        let auth_fails = Rc::new(Cell::new(false));
        let failing = auth_fails.clone();
        f.stub_get_sessions(move |_provider_id, _scopes| {
            if failing.get() {
                return Promise::rejected(Error::new("Auth service unavailable"));
            }
            Promise::resolved(vec![create_microsoft_session()])
        });
        f.set_request_handler(|_| Promise::resolved(mock_response(200, create_gallery_manifest())));

        let service = f.create_service();
        get_manifest(&service).await;
        assert_status(&service, ExtensionGalleryManifestStatus::Available);

        // The account can no longer be resolved. That is not a sign-out, and it must not retract a
        // marketplace the user already has.
        auth_fails.set(true);
        f.fire_microsoft_sessions_changed();
        tick().await;

        assert_status(&service, ExtensionGalleryManifestStatus::Available);
    });
}

#[test]
fn test_github_provider_sign_out_during_an_in_flight_manifest_fetch_does_not_restore_access() {
    run(|f| async move {
        f.set_configuration(EXTENSION_GALLERY_AUTH_PROVIDER_CONFIG_KEY, "github");
        f.set_default_account(Some(create_default_account(true, None)));

        let (index_gate, release_index) = Promise::<IRequestContext>::new();
        f.set_request_handler(move |_| index_gate.clone());

        let service = f.create_service();
        let inflight = service.get_extension_gallery_manifest();

        tick().await;

        f.set_default_account(None);
        f.on_did_change_default_account.fire(None);
        tick().await;

        release_index.resolve(mock_response(200, create_gallery_manifest()));
        inflight.await.expect("getExtensionGalleryManifest resolve");
        tick().await;

        assert_status(&service, ExtensionGalleryManifestStatus::RequiresSignIn);
    });
}

// --- No configuredServiceUrl ---

#[test]
fn test_no_configured_service_url_uses_default_gallery_manifest() {
    run(|f| async move {
        f.set_configuration(EXTENSION_GALLERY_SERVICE_URL_CONFIG_KEY, "");

        let service = f.create_service();
        get_manifest(&service).await;

        // With no configured marketplace serviceUrl the Entra/private-marketplace path is never
        // engaged; the base class falls back to the product's default gallery, so Available.
        assert_status(&service, ExtensionGalleryManifestStatus::Available);
    });
}

// --- Configuration changes ---

#[test]
fn test_changing_auth_provider_mid_session_prompts_for_restart() {
    run(|f| async move {
        f.set_configuration(EXTENSION_GALLERY_AUTH_PROVIDER_CONFIG_KEY, "microsoft");
        f.set_microsoft_sessions(vec![create_microsoft_session()]);

        let service = f.create_service();
        get_manifest(&service).await;
        assert!(f.restart_prompts.borrow().is_empty());

        // The provider is chosen once at startup, so a later change cannot take effect in this
        // window. It must not be silently ignored.
        f.set_configuration(EXTENSION_GALLERY_AUTH_PROVIDER_CONFIG_KEY, "github");
        f.fire_config_change(&[EXTENSION_GALLERY_AUTH_PROVIDER_CONFIG_KEY]);

        assert_eq!(*f.restart_prompts.borrow(), vec!["The Extensions Marketplace configuration has changed. Please restart to apply the changes.".to_owned()]);
    });
}

#[test]
fn test_changing_service_url_mid_session_keeps_its_own_restart_message() {
    run(|f| async move {
        let service = f.create_service();
        get_manifest(&service).await;

        f.fire_config_change(&[EXTENSION_GALLERY_SERVICE_URL_CONFIG_KEY]);

        // A different marketplace, not a different sign-in: the existing wording still applies.
        assert_eq!(*f.restart_prompts.borrow(), vec!["VS Code Test is now configured to a different Marketplace. Please restart to apply the changes.".to_owned()]);
    });
}

#[test]
fn test_an_unrelated_configuration_change_does_not_prompt_for_restart() {
    run(|f| async move {
        let service = f.create_service();
        get_manifest(&service).await;

        f.fire_config_change(&["editor.fontSize"]);

        assert!(f.restart_prompts.borrow().is_empty());
    });
}

// --- Telemetry ---

/// `{ authProvider, eligible }`, o dado do evento `marketplace:auth:checked`, como JSON.
fn auth_checked(auth_provider: &str, eligible: bool) -> Option<String> {
    Some(json!({ "authProvider": auth_provider, "eligible": eligible }).to_string())
}

#[test]
fn test_telemetry_github_eligible_access_reports_custom_marketplace_and_auth_check() {
    run(|f| async move {
        f.set_configuration(EXTENSION_GALLERY_AUTH_PROVIDER_CONFIG_KEY, "github");
        f.set_default_account(Some(create_default_account(true, None)));

        let service = f.create_service();
        get_manifest(&service).await;

        assert_eq!(f.custom_marketplace_count(), 1);
        assert_eq!(f.auth_checked_events(), vec![auth_checked("github", true)]);
    });
}

#[test]
fn test_telemetry_github_ineligible_access_reports_auth_check_but_not_custom_marketplace() {
    run(|f| async move {
        f.set_configuration(EXTENSION_GALLERY_AUTH_PROVIDER_CONFIG_KEY, "github");
        f.set_default_account(Some(create_default_account(false, None)));

        let service = f.create_service();
        get_manifest(&service).await;

        // Denied access never publishes the manifest, so the custom-marketplace event does not fire.
        assert_eq!(f.custom_marketplace_count(), 0);
        assert_eq!(f.auth_checked_events(), vec![auth_checked("github", false)]);
    });
}

#[test]
fn test_telemetry_microsoft_eligible_access_reports_custom_marketplace_and_auth_check() {
    run(|f| async move {
        f.set_configuration(EXTENSION_GALLERY_AUTH_PROVIDER_CONFIG_KEY, "microsoft");
        f.set_microsoft_sessions(vec![create_microsoft_session()]);
        f.set_request_handler(|_| Promise::resolved(mock_response(200, create_gallery_manifest())));

        let service = f.create_service();
        get_manifest(&service).await;

        // Regression guard: the custom-marketplace event must fire for the Microsoft path too, not
        // just for GitHub. The github/microsoft distinction lives on 'marketplace:auth:checked'.
        assert_eq!(f.custom_marketplace_count(), 1);
        assert_eq!(f.auth_checked_events(), vec![auth_checked("microsoft", true)]);
    });
}

#[test]
fn test_telemetry_microsoft_ineligible_access_reports_auth_check_but_not_custom_marketplace() {
    run(|f| async move {
        f.set_configuration(EXTENSION_GALLERY_AUTH_PROVIDER_CONFIG_KEY, "microsoft");
        // A personal Microsoft Account (MSA) is ineligible under the local tenant check.
        f.set_microsoft_sessions(vec![create_microsoft_session_with("ms-token", "ms-account-1", "ms-session-1", MSA_TENANT_ID)]);
        f.set_request_handler(|_| Promise::resolved(mock_response(200, create_gallery_manifest())));

        let service = f.create_service();
        get_manifest(&service).await;

        assert_eq!(f.custom_marketplace_count(), 0);
        assert_eq!(f.auth_checked_events(), vec![auth_checked("microsoft", false)]);
    });
}

#[test]
fn test_telemetry_requires_sign_in_does_not_report_any_access_verdict() {
    run(|f| async move {
        f.set_configuration(EXTENSION_GALLERY_AUTH_PROVIDER_CONFIG_KEY, "github");
        f.set_default_account(None);

        let service = f.create_service();
        get_manifest(&service).await;

        // No definitive verdict was reached, so nothing is cached and no verdict is reported.
        assert_eq!(f.custom_marketplace_count(), 0);
        assert_eq!(f.auth_checked_events(), Vec::<Option<String>>::new());
    });
}
