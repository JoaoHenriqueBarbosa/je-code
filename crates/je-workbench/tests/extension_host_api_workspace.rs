//! Teste de ponta a ponta de `vscode.workspace` (não é porte do oráculo): uma extensão de exemplo em JavaScript, ativada pelo host
//! local (`AbstractExtensionService` + `LocalExtensionHost` + `RPCProtocol`), exerce a API de verdade, e o efeito aparece no lado do
//! workbench.
//!
//! - **Pastas, nome, `rootPath`, confiança e `asRelativePath`/`getWorkspaceFolder`** vêm do `IWorkspaceData` que o lado do
//!   workbench entrega por `$initializeWorkspace` ao `ExtHostWorkspace` portado; `onDidChangeWorkspaceFolders` e
//!   `onDidGrantWorkspaceTrust` disparam quando o workbench manda `$acceptWorkspaceData` e `$onDidGrantWorkspaceTrust`.
//! - **Configuração** passa pelo `MainThreadConfiguration` REAL: os dados iniciais (`$initializeConfiguration`) alimentam
//!   `getConfiguration().get/has/inspect`, `update` chega ao `IConfigurationService` do workbench, e uma mudança de configuração do
//!   workbench dispara `onDidChangeConfiguration` na extensão com o `affectsConfiguration` certo.
//!
//! Dublês (o que o teste não monta de verdade):
//! - `WorkspaceClient` no lugar do `MainThreadWorkspace`: o real pede quinze serviços do workbench (busca, editores, confiança,
//!   arquivos de texto...) que o teste não exerce; o dublê manda os mesmos `$initializeWorkspace` e `$acceptWorkspaceData` e falha com
//!   `panic!` se o host chamar outro método (nenhum caso chama).
//! - `SeededConfigurationService` embrulha o `TestConfigurationService` e devolve dados de configuração (`getConfigurationData`), que o
//!   serviço de teste não tem.
//! - `TestDialogService` e `TestNotificationService` do `MainThreadMessageService` (que o `ExtHostWorkspace` exige ao nascer).

#[path = "support/extension_host_api_harness.rs"]
mod harness;

use std::cell::RefCell;
use std::rc::Rc;

use harness::{run_extension, Workbench};
use je_base::cancellation::CancellationToken;
use je_base::dynamic::Dynamic;
use je_base::event::Event;
use je_base::lifecycle::IDisposable;
use je_base::promise::Promise;
use je_base::uri::URI;
use je_base::JsString;
use je_exthost::in_process::IMainThreadBinder;
use je_platform::configuration::models::{Configuration, ConfigurationChangeEvent, ConfigurationKeys};
use je_platform::configuration::service::IConfigurationService;
use je_platform::configuration::test_configuration_service::TestConfigurationService;
use je_platform::configuration::{
    ConfigurationChange, ConfigurationData, ConfigurationModelData, ConfigurationOverrides, ConfigurationTarget, ConfigurationUpdateOverrides, ConfigurationValue,
};
use je_platform::log::NullLogService;
use je_platform::request::request::{AuthInfo, Credentials};
use je_platform::workspace_trust::{ResourceTrustRequestOptions, WorkspaceTrustRequestOptions};
use je_workbench::api::browser::main_thread_configuration::MainThreadConfiguration;
use je_workbench::api::browser::main_thread_message_service::MainThreadMessageService;
use je_workbench::api::common::ext_host_protocol::{
    EncodingOptionDto, ExtHostContext, ExtHostWorkspaceShape, IMainContext, ITextSearchComplete, IWorkspaceData, MainContext, MainThreadWorkspaceShape, ResolvedDecodingDto,
    ResolvedEncodingDto, SaveOptionsDto, WorkspaceFolderDataDto, WorkspaceFolderToAdd,
};
use je_workbench::services::search::query_builder::{IFileQueryBuilderOptions, ITextQueryBuilderOptions};
use je_workbench::services::search::search::PatternInfo;
use je_workbench::test::browser::workbench_test_services::TestEnvironmentService;
use je_workbench::test::common::workbench_test_services::{TestContextService, TestNotificationService};

/// O JavaScript da extensão de exemplo: cada comando devolve um valor simples que o teste confere.
const EXTENSION_SOURCE: &str = r#"
const vscode = require('vscode');
exports.activate = (context) => {
  globalThis.__activated = true;
  const command = (id, handler) => context.subscriptions.push(vscode.commands.registerCommand(id, handler));
  command('t.folders', () => (vscode.workspace.workspaceFolders || []).map((folder) => ({
    name: folder.name, path: folder.uri.fsPath, scheme: folder.uri.scheme, index: folder.index, isUri: folder.uri instanceof vscode.Uri,
  })));
  command('t.info', () => ({
    name: vscode.workspace.name, rootPath: vscode.workspace.rootPath, trusted: vscode.workspace.isTrusted,
  }));
  command('t.relative', (path) => vscode.workspace.asRelativePath(path));
  command('t.folderOf', (path) => {
    const folder = vscode.workspace.getWorkspaceFolder(vscode.Uri.file(path));
    return folder ? folder.name : null;
  });
  command('t.config', () => {
    const config = vscode.workspace.getConfiguration('exthost');
    const inspected = config.inspect('greeting');
    return {
      has: config.has('greeting'), missing: config.has('nope'), greeting: config.get('greeting'), fallback: config.get('nope', 'padrão'),
      nested: config.get('nested.level'), viaProperty: config.greeting, inspectedKey: inspected ? inspected.key : null,
    };
  });
  command('t.update', async () => {
    await vscode.workspace.getConfiguration('exthost').update('greeting', 'oi', vscode.ConfigurationTarget.Global);
    return 'atualizado';
  });
  command('t.watch', () => {
    globalThis.__changes = [];
    globalThis.__folderEvents = [];
    globalThis.__trust = [];
    context.subscriptions.push(vscode.workspace.onDidChangeConfiguration((event) => {
      globalThis.__changes.push(event.affectsConfiguration('exthost.greeting') + ':' + event.affectsConfiguration('outra'));
    }));
    context.subscriptions.push(vscode.workspace.onDidChangeWorkspaceFolders((event) => {
      globalThis.__folderEvents.push(event.added.map((f) => f.name).join(',') + '|' + event.removed.map((f) => f.name).join(','));
    }));
    context.subscriptions.push(vscode.workspace.onDidGrantWorkspaceTrust(() => globalThis.__trust.push(vscode.workspace.isTrusted)));
    return 'observando';
  });
};
"#;

/// O dublê do `MainThreadWorkspace` (ver o cabeçalho do arquivo).
struct WorkspaceClient {
    proxy: Rc<dyn ExtHostWorkspaceShape>,
}

impl WorkspaceClient {
    fn publish(&self, data: IWorkspaceData) {
        self.proxy.rpc_accept_workspace_data(Some(data));
    }

    fn grant_trust(&self) {
        self.proxy.rpc_on_did_grant_workspace_trust();
    }
}

fn unexercised(method: &str) -> ! {
    panic!("o teste não exercita MainThreadWorkspace.{method}")
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

/// O `IConfigurationService` do teste: o `TestConfigurationService` (que registra o `updateValue`) mais dados de configuração.
struct SeededConfigurationService {
    inner: Rc<TestConfigurationService>,
    data: RefCell<ConfigurationData>,
}

fn model(contents: Dynamic, keys: &[&str]) -> ConfigurationModelData {
    ConfigurationModelData { contents, keys: keys.iter().map(|key| JsString::from(*key)).collect(), overrides: Vec::new(), raw: None }
}

fn empty_model() -> ConfigurationModelData {
    model(Dynamic::object(Vec::<(&str, Dynamic)>::new()), &[])
}

/// Os dados em que `exthost.greeting` vale `texto` e `exthost.nested.level` vale 3, na camada do usuário.
fn configuration_data(greeting: &str) -> ConfigurationData {
    let contents = Dynamic::object([(
        "exthost",
        Dynamic::object([("greeting", Dynamic::string(greeting)), ("nested", Dynamic::object([("level", Dynamic::Number(3.0))]))]),
    )]);
    ConfigurationData {
        defaults: empty_model(),
        policy: empty_model(),
        application: empty_model(),
        user_local: model(contents, &["exthost.greeting", "exthost.nested.level"]),
        user_remote: empty_model(),
        workspace: empty_model(),
        folders: Vec::new(),
    }
}

impl IConfigurationService for SeededConfigurationService {
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

/// O que o teste observa do lado do workbench depois do `bind`.
struct Observed {
    configuration_service: Rc<SeededConfigurationService>,
    workspace_client: Rc<RefCell<Option<Rc<WorkspaceClient>>>>,
}

/// Liga `MainThreadWorkspace` (dublê), `MainThreadConfiguration` e `MainThreadMessageService` (reais) ao host.
struct Binder {
    configuration_service: Rc<SeededConfigurationService>,
    context_service: Rc<TestContextService>,
    workspace_client: Rc<RefCell<Option<Rc<WorkspaceClient>>>>,
    message_service: Rc<MainThreadMessageService>,
    initial_data: IWorkspaceData,
}

impl IMainThreadBinder for Binder {
    fn bind(&self, context: Rc<dyn IMainContext>) -> Vec<Rc<dyn IDisposable>> {
        let proxy = context.get_proxy(&ExtHostContext::EXT_HOST_WORKSPACE);
        let client = Rc::new(WorkspaceClient { proxy: proxy.clone() });
        proxy.rpc_initialize_workspace(Some(self.initial_data.clone()), false);
        *self.workspace_client.borrow_mut() = Some(client.clone());
        let client_shape: Rc<dyn MainThreadWorkspaceShape> = client.clone();
        context.set(&MainContext::MAIN_THREAD_WORKSPACE, client_shape);

        let configuration = MainThreadConfiguration::new(&*context, self.context_service.clone(), self.configuration_service.clone(), TestEnvironmentService::new());
        let configuration_shape: Rc<dyn je_workbench::api::common::ext_host_protocol::MainThreadConfigurationShape> = configuration.clone();
        context.set(&MainContext::MAIN_THREAD_CONFIGURATION, configuration_shape);

        let message_shape: Rc<dyn je_workbench::api::common::ext_host_protocol::MainThreadMessageServiceShape> = self.message_service.clone();
        context.set(&MainContext::MAIN_THREAD_MESSAGE_SERVICE, message_shape);
        let clients: [Rc<dyn IDisposable>; 3] = [client, configuration, self.message_service.clone()];
        clients.into_iter().collect()
    }
}

fn folder(path: &str, name: &str, index: usize) -> WorkspaceFolderDataDto {
    WorkspaceFolderDataDto { uri: URI::file(path, je_platform::resources::host_platform()), name: name.to_owned(), index }
}

fn workspace_data(folders: Vec<WorkspaceFolderDataDto>) -> IWorkspaceData {
    IWorkspaceData { id: "ws-1".to_owned(), name: "meu-projeto".to_owned(), transient: None, configuration: None, is_untitled: None, folders }
}

fn text(value: &Dynamic, key: &str) -> String {
    match value.get_str(key) {
        Dynamic::String(text) => text.to_string_lossy(),
        other => panic!("esperava texto em '{key}', veio {other:?}"),
    }
}

fn setup(workbench: &Workbench) -> (Rc<dyn IMainThreadBinder>, Observed) {
    let inner = TestConfigurationService::new(None);
    let configuration_service = Rc::new(SeededConfigurationService { inner, data: RefCell::new(configuration_data("olá")) });
    let message_service = MainThreadMessageService::new(
        Rc::new(TestNotificationService),
        workbench.command_service.clone(),
        Rc::new(harness::doubles::TestDialogService),
        workbench.extension_service.clone(),
    );
    let workspace_client = Rc::new(RefCell::new(None));
    let binder = Binder {
        configuration_service: configuration_service.clone(),
        context_service: TestContextService::new(None, None),
        workspace_client: workspace_client.clone(),
        message_service,
        initial_data: workspace_data(vec![folder("/work/app", "app", 0), folder("/work/lib", "lib", 1)]),
    };
    (Rc::new(binder), Observed { configuration_service, workspace_client })
}

#[test]
fn workspace_folders_name_root_path_and_relative_paths_come_from_the_workbench() {
    run_extension(EXTENSION_SOURCE, setup, |workbench, _observed| {
        let folders = workbench.execute("t.folders", Vec::new());
        let Dynamic::Array(folders) = folders else { panic!("workspaceFolders deveria ser uma lista") };
        let folders = folders.to_vec();
        assert_eq!(folders.len(), 2);
        assert_eq!(text(&folders[0], "name"), "app");
        assert_eq!(text(&folders[0], "path"), "/work/app");
        assert_eq!(text(&folders[0], "scheme"), "file");
        assert_eq!(folders[0].get_str("index"), Dynamic::Number(0.0));
        assert_eq!(folders[0].get_str("isUri"), Dynamic::Bool(true), "uri deveria ser uma instância de vscode.Uri");
        assert_eq!(text(&folders[1], "name"), "lib");

        let info = workbench.execute("t.info", Vec::new());
        assert_eq!(text(&info, "name"), "meu-projeto");
        assert_eq!(text(&info, "rootPath"), "/work/app", "rootPath é a primeira pasta");
        assert_eq!(info.get_str("trusted"), Dynamic::Bool(false));

        // Com duas pastas, `asRelativePath` inclui o nome da pasta (`includeWorkspace` padrão), como o original.
        assert_eq!(workbench.execute("t.relative", vec![Dynamic::string("/work/lib/src/main.rs")]), Dynamic::string("lib/src/main.rs"));
        assert_eq!(workbench.execute("t.relative", vec![Dynamic::string("/fora/arquivo.txt")]), Dynamic::string("/fora/arquivo.txt"));
        assert_eq!(workbench.execute("t.folderOf", vec![Dynamic::string("/work/lib/x.rs")]), Dynamic::string("lib"));
        assert_eq!(workbench.execute("t.folderOf", vec![Dynamic::string("/fora/x.rs")]), Dynamic::Null);
    });
}

#[test]
fn configuration_reads_the_workbench_data_and_update_reaches_the_configuration_service() {
    run_extension(EXTENSION_SOURCE, setup, |workbench, observed| {
        let config = workbench.execute("t.config", Vec::new());
        assert_eq!(config.get_str("has"), Dynamic::Bool(true));
        assert_eq!(config.get_str("missing"), Dynamic::Bool(false));
        assert_eq!(text(&config, "greeting"), "olá");
        assert_eq!(text(&config, "fallback"), "padrão");
        assert_eq!(config.get_str("nested"), Dynamic::Number(3.0));
        assert_eq!(text(&config, "viaProperty"), "olá", "o valor também é propriedade do objeto");
        assert_eq!(text(&config, "inspectedKey"), "exthost.greeting");

        assert_eq!(workbench.execute("t.update", Vec::new()), Dynamic::string("atualizado"));
        let calls = observed.configuration_service.inner.update_value_calls();
        assert_eq!(calls.len(), 1, "o update da extensão deveria chegar ao IConfigurationService do workbench");
        assert_eq!(calls[0].key, "exthost.greeting");
        assert_eq!(calls[0].value, Dynamic::string("oi"));
        assert_eq!(calls[0].target, Some(ConfigurationTarget::User), "ConfigurationTarget.Global é a camada do usuário");
    });
}

#[test]
fn change_events_fire_for_configuration_workspace_folders_and_trust() {
    run_extension(EXTENSION_SOURCE, setup, |workbench, observed| {
        assert_eq!(workbench.execute("t.watch", Vec::new()), Dynamic::string("observando"));

        // O workbench muda `exthost.greeting`: a extensão recebe o evento com o `affectsConfiguration` certo.
        let log = NullLogService::shared();
        let data = configuration_data("novo");
        *observed.configuration_service.data.borrow_mut() = data.clone();
        let change = ConfigurationChange { keys: vec![JsString::from("exthost.greeting")], overrides: Vec::new() };
        let event = ConfigurationChangeEvent::new(change, None, Rc::new(Configuration::parse(&data, log.clone())), None, log);
        observed.configuration_service.inner.on_did_change_configuration_emitter.fire(Rc::new(event));
        workbench.pump();
        assert_eq!(workbench.probe("JSON.stringify(globalThis.__changes)"), r#"["true:false"]"#);
        let after = workbench.execute("t.config", Vec::new());
        assert_eq!(text(&after, "greeting"), "novo", "a leitura seguinte vê o valor novo");

        // O workbench troca as pastas: `onDidChangeWorkspaceFolders` entrega o que entrou e o que saiu.
        let client = observed.workspace_client.borrow().clone().expect("o cliente de workspace foi ligado");
        client.publish(workspace_data(vec![folder("/work/app", "app", 0), folder("/work/docs", "docs", 1)]));
        workbench.pump();
        assert_eq!(workbench.probe("JSON.stringify(globalThis.__folderEvents)"), r#"["docs|lib"]"#);
        let folders = workbench.execute("t.folders", Vec::new());
        let Dynamic::Array(folders) = folders else { panic!("workspaceFolders deveria ser uma lista") };
        assert_eq!(folders.len(), 2);

        // A confiança concedida chega, e `isTrusted` passa a valer `true` já dentro do listener.
        client.grant_trust();
        workbench.pump();
        assert_eq!(workbench.probe("JSON.stringify(globalThis.__trust)"), "[true]");
    });
}
