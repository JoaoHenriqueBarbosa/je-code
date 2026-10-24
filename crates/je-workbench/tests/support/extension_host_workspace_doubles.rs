//! Os dublês de workspace e configuração que os testes de ponta a ponta da API `vscode` reaproveitam quando precisam das pastas e da
//! configuração do workbench (por exemplo, `createFileSystemWatcher` lê as pastas e `files.watcherExclude`).
//!
//! - `WorkspaceClient` no lugar do `MainThreadWorkspace`: o real pede quinze serviços do workbench (busca, editores, confiança,
//!   arquivos de texto...) que esses testes não exercem; o dublê manda `$initializeWorkspace` ao `ExtHostWorkspace` (e, a pedido do
//!   teste, `$acceptWorkspaceData` e `$onDidGrantWorkspaceTrust`) e falha com `panic!` se o host chamar qualquer outro método.
//! - `StaticConfigurationService` embrulha o `TestConfigurationService` (que registra o `updateValue`) e devolve os dados de
//!   configuração semeados (`getConfigurationData`), que o serviço de teste não tem, para o `MainThreadConfiguration` REAL iniciar o
//!   `ExtHostConfiguration`. Sem semente, os dados são vazios.
//! - `MainThreadMessageService` (REAL) sobre `TestDialogService` e `TestNotificationService`, que o `ExtHostWorkspace` exige ao nascer.

#![allow(dead_code)]

use std::cell::RefCell;
use std::rc::Rc;

use je_base::cancellation::CancellationToken;
use je_base::dynamic::Dynamic;
use je_base::event::Event;
use je_base::lifecycle::IDisposable;
use je_base::promise::Promise;
use je_base::uri::URI;
use je_base::JsString;
use je_platform::configuration::models::{ConfigurationChangeEvent, ConfigurationKeys};
use je_platform::configuration::service::IConfigurationService;
use je_platform::configuration::test_configuration_service::TestConfigurationService;
use je_platform::configuration::{
    ConfigurationData, ConfigurationModelData, ConfigurationOverrides, ConfigurationTarget, ConfigurationUpdateOverrides, ConfigurationValue,
};
use je_platform::request::request::{AuthInfo, Credentials};
use je_platform::workspace_trust::{ResourceTrustRequestOptions, WorkspaceTrustRequestOptions};
use je_workbench::api::browser::main_thread_configuration::MainThreadConfiguration;
use je_workbench::api::browser::main_thread_message_service::MainThreadMessageService;
use je_workbench::api::common::ext_host_protocol::{
    EncodingOptionDto, ExtHostContext, ExtHostWorkspaceShape, IMainContext, ITextSearchComplete, IWorkspaceData, MainContext, MainThreadConfigurationShape, MainThreadMessageServiceShape,
    MainThreadWorkspaceShape, ResolvedDecodingDto, ResolvedEncodingDto, SaveOptionsDto, WorkspaceFolderDataDto, WorkspaceFolderToAdd,
};
use je_workbench::services::search::query_builder::{IFileQueryBuilderOptions, ITextQueryBuilderOptions};
use je_workbench::services::search::search::PatternInfo;
use je_workbench::test::browser::workbench_test_services::TestEnvironmentService;
use je_workbench::test::common::workbench_test_services::{TestContextService, TestNotificationService};

use super::harness::doubles::TestDialogService;
use super::harness::Workbench;

fn unexercised(method: &str) -> ! {
    panic!("o teste não exercita MainThreadWorkspace.{method}")
}

/// O dublê do `MainThreadWorkspace`: guarda o proxy do `ExtHostWorkspace`, por onde o teste entrega as mudanças do workbench.
pub struct WorkspaceClient {
    proxy: Rc<dyn ExtHostWorkspaceShape>,
}

impl WorkspaceClient {
    /// O workbench troca o workspace (`$acceptWorkspaceData`).
    pub fn publish(&self, data: IWorkspaceData) {
        self.proxy.rpc_accept_workspace_data(Some(data));
    }

    /// O workbench concede a confiança no workspace (`$onDidGrantWorkspaceTrust`).
    pub fn grant_trust(&self) {
        self.proxy.rpc_on_did_grant_workspace_trust();
    }
}

impl IDisposable for WorkspaceClient {
    fn dispose(&self) {}
}

impl MainThreadWorkspaceShape for WorkspaceClient {
    fn rpc_start_file_search(&self, _include_folder: Option<URI>, _options: IFileQueryBuilderOptions, _token: CancellationToken) -> Promise<Option<Vec<URI>>> {
        unexercised("$startFileSearch")
    }

    fn rpc_start_text_search(&self, _query: PatternInfo, _folder: Option<URI>, _options: ITextQueryBuilderOptions, _request_id: u64, _token: CancellationToken) -> Promise<Option<ITextSearchComplete>> {
        unexercised("$startTextSearch")
    }

    fn rpc_check_exists(&self, _folders: Vec<URI>, _includes: Vec<String>, _token: CancellationToken) -> Promise<bool> {
        unexercised("$checkExists")
    }

    fn rpc_save(&self, _uri: &URI, _options: SaveOptionsDto) -> Promise<Option<URI>> {
        unexercised("$save")
    }

    fn rpc_save_all(&self, _include_untitled: Option<bool>) -> Promise<bool> {
        unexercised("$saveAll")
    }

    fn rpc_update_workspace_folders(&self, _extension_name: &str, _index: f64, _delete_count: f64, _workspace_folders_to_add: Vec<WorkspaceFolderToAdd>) -> Promise<()> {
        unexercised("$updateWorkspaceFolders")
    }

    fn rpc_resolve_proxy(&self, _url: &str) -> Promise<Option<String>> {
        unexercised("$resolveProxy")
    }

    fn rpc_lookup_authorization(&self, _auth_info: AuthInfo) -> Promise<Option<Credentials>> {
        unexercised("$lookupAuthorization")
    }

    fn rpc_lookup_kerberos_authorization(&self, _url: &str) -> Promise<Option<String>> {
        unexercised("$lookupKerberosAuthorization")
    }

    fn rpc_load_certificates(&self) -> Promise<Vec<String>> {
        unexercised("$loadCertificates")
    }

    fn rpc_request_resource_trust(&self, _options: ResourceTrustRequestOptions) -> Promise<Option<bool>> {
        unexercised("$requestResourceTrust")
    }

    fn rpc_request_workspace_trust(&self, _options: Option<WorkspaceTrustRequestOptions>) -> Promise<Option<bool>> {
        unexercised("$requestWorkspaceTrust")
    }

    fn rpc_is_resource_trusted(&self, _resource: &URI) -> Promise<bool> {
        unexercised("$isResourceTrusted")
    }

    fn rpc_register_edit_session_identity_provider(&self, _handle: u64, _scheme: &str) {
        unexercised("$registerEditSessionIdentityProvider")
    }

    fn rpc_unregister_edit_session_identity_provider(&self, _handle: u64) {
        unexercised("$unregisterEditSessionIdentityProvider")
    }

    fn rpc_register_canonical_uri_provider(&self, _handle: u64, _scheme: &str) {
        unexercised("$registerCanonicalUriProvider")
    }

    fn rpc_unregister_canonical_uri_provider(&self, _handle: u64) {
        unexercised("$unregisterCanonicalUriProvider")
    }

    fn rpc_resolve_decoding(&self, _resource: Option<URI>, _options: Option<EncodingOptionDto>) -> Promise<ResolvedDecodingDto> {
        unexercised("$resolveDecoding")
    }

    fn rpc_validate_detected_encoding(&self, _resource: Option<URI>, _detected_encoding: &str, _options: Option<EncodingOptionDto>) -> Promise<String> {
        unexercised("$validateDetectedEncoding")
    }

    fn rpc_resolve_encoding(&self, _resource: Option<URI>, _options: Option<EncodingOptionDto>) -> Promise<ResolvedEncodingDto> {
        unexercised("$resolveEncoding")
    }
}

/// O `IConfigurationService` do teste: o `TestConfigurationService` mais os dados de configuração semeados.
pub struct StaticConfigurationService {
    /// O serviço de teste por baixo (o teste lê dele o `update_value_calls` e dispara o `on_did_change_configuration_emitter`).
    pub inner: Rc<TestConfigurationService>,
    data: RefCell<ConfigurationData>,
}

impl StaticConfigurationService {
    /// Semeado com `data`.
    pub fn seeded(data: ConfigurationData) -> Rc<Self> {
        Rc::new(Self { inner: TestConfigurationService::new(None), data: RefCell::new(data) })
    }

    /// Sem semente: todas as camadas vazias.
    pub fn empty() -> Rc<Self> {
        Self::seeded(configuration_data(empty_model()))
    }

    /// Troca os dados que `getConfigurationData` devolve (a próxima leitura da extensão vê o valor novo).
    pub fn set_data(&self, data: ConfigurationData) {
        *self.data.borrow_mut() = data;
    }
}

/// O modelo de uma camada com `contents` e as `keys` que ele define.
pub fn model(contents: Dynamic, keys: &[&str]) -> ConfigurationModelData {
    ConfigurationModelData { contents, keys: keys.iter().map(|key| JsString::from(*key)).collect(), overrides: Vec::new(), raw: None }
}

pub fn empty_model() -> ConfigurationModelData {
    model(Dynamic::object(Vec::<(&str, Dynamic)>::new()), &[])
}

/// Os dados de configuração com `user_local` na camada do usuário e as demais vazias.
pub fn configuration_data(user_local: ConfigurationModelData) -> ConfigurationData {
    ConfigurationData {
        defaults: empty_model(),
        policy: empty_model(),
        application: empty_model(),
        user_local,
        user_remote: empty_model(),
        workspace: empty_model(),
        folders: Vec::new(),
    }
}

impl IConfigurationService for StaticConfigurationService {
    fn on_did_change_configuration(&self) -> Event<Rc<ConfigurationChangeEvent>> {
        self.inner.on_did_change_configuration()
    }

    fn get_configuration_data(&self) -> Option<ConfigurationData> {
        Some(self.data.borrow().clone())
    }

    fn get_value(&self, section: Option<&str>, overrides: Option<&ConfigurationOverrides>) -> Dynamic {
        self.inner.get_value(section, overrides)
    }

    fn update_value(&self, key: &str, value: Dynamic, overrides: Option<ConfigurationUpdateOverrides>, target: Option<ConfigurationTarget>) -> Promise<()> {
        self.inner.update_value(key, value, overrides, target)
    }

    fn inspect(&self, key: &str, overrides: &ConfigurationOverrides) -> Rc<dyn ConfigurationValue> {
        self.inner.inspect(key, overrides)
    }

    fn reload_configuration(&self) -> Promise<()> {
        self.inner.reload_configuration()
    }

    fn keys(&self) -> ConfigurationKeys {
        self.inner.keys()
    }
}

/// Os dados do workspace com uma pasta por caminho (nome igual ao último segmento).
pub fn workspace_data(folder_paths: &[&str]) -> IWorkspaceData {
    let folders = folder_paths
        .iter()
        .enumerate()
        .map(|(index, path)| WorkspaceFolderDataDto {
            uri: URI::file(*path, je_platform::resources::host_platform()),
            name: path.rsplit('/').next().unwrap_or(path).to_owned(),
            index,
        })
        .collect();
    IWorkspaceData { id: "ws-1".to_owned(), name: "meu-projeto".to_owned(), transient: None, configuration: None, is_untitled: None, folders }
}

/// O `MainThreadMessageService` real do workbench de teste (o `ExtHostWorkspace` exige o cliente ao nascer).
pub fn message_service(workbench: &Workbench) -> Rc<MainThreadMessageService> {
    MainThreadMessageService::new(
        Rc::new(TestNotificationService),
        workbench.command_service.clone(),
        Rc::new(TestDialogService),
        workbench.extension_service.clone(),
    )
}

/// Os clientes que [`bind_workspace_clients`] ligou ao host.
pub struct WorkspaceClients {
    /// O dublê do `MainThreadWorkspace`, para o teste publicar mudanças do workbench.
    pub workspace: Rc<WorkspaceClient>,
    /// Todos os clientes, para o binder devolver ao host (que os descarta ao parar).
    pub disposables: Vec<Rc<dyn IDisposable>>,
}

/// Liga ao host o `MainThreadWorkspace` (dublê), o `MainThreadConfiguration` (real, sobre `configuration_service`) e o
/// `MainThreadMessageService` (real), com `initial_data` como o workspace inicial.
pub fn bind_workspace_clients(
    context: &Rc<dyn IMainContext>,
    message_service: Rc<MainThreadMessageService>,
    initial_data: IWorkspaceData,
    configuration_service: Rc<StaticConfigurationService>,
) -> WorkspaceClients {
    let workspace_proxy = context.get_proxy(&ExtHostContext::EXT_HOST_WORKSPACE);
    workspace_proxy.rpc_initialize_workspace(Some(initial_data), false);
    let client = Rc::new(WorkspaceClient { proxy: workspace_proxy });
    let client_shape: Rc<dyn MainThreadWorkspaceShape> = client.clone();
    context.set(&MainContext::MAIN_THREAD_WORKSPACE, client_shape);

    let configuration = MainThreadConfiguration::new(&**context, TestContextService::new(None, None), configuration_service, TestEnvironmentService::new());
    let configuration_shape: Rc<dyn MainThreadConfigurationShape> = configuration.clone();
    context.set(&MainContext::MAIN_THREAD_CONFIGURATION, configuration_shape);

    let message_shape: Rc<dyn MainThreadMessageServiceShape> = message_service.clone();
    context.set(&MainContext::MAIN_THREAD_MESSAGE_SERVICE, message_shape);

    let clients: [Rc<dyn IDisposable>; 3] = [client.clone(), configuration, message_service];
    WorkspaceClients { workspace: client, disposables: clients.into_iter().collect() }
}
