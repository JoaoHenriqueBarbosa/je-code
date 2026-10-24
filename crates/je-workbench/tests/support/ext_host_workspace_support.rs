//! Dublês e auxiliares dos testes de `ExtHostWorkspace` e `ExtHostConfigProvider`
//! (`extHostWorkspace.test.ts` e `extHostConfiguration.test.ts`).
//!
//! `TestRPCProtocol` de `src/vs/workbench/api/test/common/testRPCProtocol.ts` devolve, para um identificador sem
//! objeto registrado, um proxy que falha ao ser chamado. [`TestMainContext`] faz o mesmo com os três proxies de que o
//! `ExtHostWorkspace` precisa: o que não foi registrado com [`TestMainContext::set`] é o dublê padrão, em que um método
//! sem resposta falha com `panic!` (como o método ausente do `mock<T>()` do original lançaria `TypeError`). Os objetos
//! têm de ser registrados antes de o `ExtHostWorkspace` ser criado (o original os registra antes, em todos os casos).

use std::any::Any;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use je_base::cancellation::CancellationToken;
use je_base::dynamic::Dynamic;
use je_base::json;
use je_base::lifecycle::IDisposable;
use je_base::promise::Promise;
use je_base::severity::Severity;
use je_base::uri::URI;
use je_platform::files::files::FileSystemProviderCapabilities;
use je_platform::log::NullLogService;
use je_platform::request::request::{AuthInfo, Credentials};
use je_platform::workspace_trust::{ResourceTrustRequestOptions, WorkspaceTrustRequestOptions};
use je_workbench::api::common::ext_host_protocol::{
    EncodingOptionDto, ExtHostWorkspaceShape, IMainContext, ITextSearchComplete, IWorkspaceData, MainContext, MainThreadMessageOptions,
    MainThreadMessageServiceShape, MainThreadTelemetryShape, MainThreadWorkspaceShape, MessageCommandDto, ProxyIdentifier, ResolvedDecodingDto,
    ResolvedEncodingDto, SaveOptionsDto, WorkspaceFolderDataDto, WorkspaceFolderToAdd,
};
use je_workbench::api::common::ext_host_workspace::{ExtHostWorkspace, IExtHostFileSystemInfo, IExtHostInitDataService, IURITransformerService};
use je_workbench::services::search::query_builder::{IFileQueryBuilderOptions, ITextQueryBuilderOptions};
use je_workbench::services::search::search::PatternInfo;

/// Um literal no estilo do TypeScript: JSON com aspas simples. Não tem `undefined`: quem precisa monta o valor na mão.
pub fn j(text: &str) -> Dynamic {
    let mut errors = Vec::new();
    let value = json::parse_str(&text.replace('\'', "\""), &mut errors).unwrap_or_else(|| panic!("literal inválido: {text}"));
    assert!(errors.is_empty(), "literal inválido: {text}");
    Dynamic::from_json(&value)
}

#[path = "../../../je-base/tests/common/mod.rs"]
mod base;

pub use base::assert_deep_strict_equal;

// ---- os proxies ----------------------------------------------------------------------------------------

type StartFileSearchFn = Box<dyn Fn(Option<URI>, IFileQueryBuilderOptions, CancellationToken) -> Promise<Option<Vec<URI>>>>;
type StartTextSearchFn = Box<dyn Fn(PatternInfo, Option<URI>, ITextQueryBuilderOptions, u64, CancellationToken) -> Promise<Option<ITextSearchComplete>>>;

/// O `new class extends mock<MainThreadWorkspace>()`: só o que o caso sobrescreve responde (e `$updateWorkspaceFolders`,
/// que a resposta padrão resolve).
#[derive(Default)]
pub struct MockMainThreadWorkspace {
    pub start_file_search: Option<StartFileSearchFn>,
    pub start_text_search: Option<StartTextSearchFn>,
}

impl IDisposable for MockMainThreadWorkspace {
    fn dispose(&self) {}
}

impl MainThreadWorkspaceShape for MockMainThreadWorkspace {
    fn rpc_start_file_search(&self, include_folder: Option<URI>, options: IFileQueryBuilderOptions, token: CancellationToken) -> Promise<Option<Vec<URI>>> {
        match &self.start_file_search {
            Some(start_file_search) => start_file_search(include_folder, options, token),
            None => panic!("$startFileSearch is not a function"),
        }
    }

    fn rpc_start_text_search(
        &self,
        query: PatternInfo,
        folder: Option<URI>,
        options: ITextQueryBuilderOptions,
        request_id: u64,
        token: CancellationToken,
    ) -> Promise<Option<ITextSearchComplete>> {
        match &self.start_text_search {
            Some(start_text_search) => start_text_search(query, folder, options, request_id, token),
            None => panic!("$startTextSearch is not a function"),
        }
    }

    fn rpc_check_exists(&self, _folders: Vec<URI>, _includes: Vec<String>, _token: CancellationToken) -> Promise<bool> {
        panic!("$checkExists is not a function")
    }

    fn rpc_save(&self, _uri: &URI, _options: SaveOptionsDto) -> Promise<Option<URI>> {
        panic!("$save is not a function")
    }

    fn rpc_save_all(&self, _include_untitled: Option<bool>) -> Promise<bool> {
        panic!("$saveAll is not a function")
    }

    fn rpc_update_workspace_folders(&self, _extension_name: &str, _index: f64, _delete_count: f64, _workspace_folders_to_add: Vec<WorkspaceFolderToAdd>) -> Promise<()> {
        Promise::resolved(())
    }

    fn rpc_resolve_proxy(&self, _url: &str) -> Promise<Option<String>> {
        panic!("$resolveProxy is not a function")
    }

    fn rpc_lookup_authorization(&self, _auth_info: AuthInfo) -> Promise<Option<Credentials>> {
        panic!("$lookupAuthorization is not a function")
    }

    fn rpc_lookup_kerberos_authorization(&self, _url: &str) -> Promise<Option<String>> {
        panic!("$lookupKerberosAuthorization is not a function")
    }

    fn rpc_load_certificates(&self) -> Promise<Vec<String>> {
        panic!("$loadCertificates is not a function")
    }

    fn rpc_request_resource_trust(&self, _options: ResourceTrustRequestOptions) -> Promise<Option<bool>> {
        panic!("$requestResourceTrust is not a function")
    }

    fn rpc_request_workspace_trust(&self, _options: Option<WorkspaceTrustRequestOptions>) -> Promise<Option<bool>> {
        panic!("$requestWorkspaceTrust is not a function")
    }

    fn rpc_is_resource_trusted(&self, _resource: &URI) -> Promise<bool> {
        panic!("$isResourceTrusted is not a function")
    }

    fn rpc_register_edit_session_identity_provider(&self, _handle: u64, _scheme: &str) {
        panic!("$registerEditSessionIdentityProvider is not a function")
    }

    fn rpc_unregister_edit_session_identity_provider(&self, _handle: u64) {
        panic!("$unregisterEditSessionIdentityProvider is not a function")
    }

    fn rpc_register_canonical_uri_provider(&self, _handle: u64, _scheme: &str) {
        panic!("$registerCanonicalUriProvider is not a function")
    }

    fn rpc_unregister_canonical_uri_provider(&self, _handle: u64) {
        panic!("$unregisterCanonicalUriProvider is not a function")
    }

    fn rpc_resolve_decoding(&self, _resource: Option<URI>, _options: Option<EncodingOptionDto>) -> Promise<ResolvedDecodingDto> {
        panic!("$resolveDecoding is not a function")
    }

    fn rpc_validate_detected_encoding(&self, _resource: Option<URI>, _detected_encoding: &str, _options: Option<EncodingOptionDto>) -> Promise<String> {
        panic!("$validateDetectedEncoding is not a function")
    }

    fn rpc_resolve_encoding(&self, _resource: Option<URI>, _options: Option<EncodingOptionDto>) -> Promise<ResolvedEncodingDto> {
        panic!("$resolveEncoding is not a function")
    }
}

/// O `MainThreadMessageService` do contexto de teste: não mostra nada.
pub struct MockMainThreadMessageService;

impl IDisposable for MockMainThreadMessageService {
    fn dispose(&self) {}
}

impl MainThreadMessageServiceShape for MockMainThreadMessageService {
    fn rpc_show_message(&self, _severity: Severity, _message: &str, _options: MainThreadMessageOptions, _commands: Vec<MessageCommandDto>) -> Promise<Option<f64>> {
        Promise::resolved(None)
    }
}

/// `new class extends mock<MainThreadTelemetryShape>() { override $publicLog2(): void { } }`.
pub struct MockMainThreadTelemetry;

impl IDisposable for MockMainThreadTelemetry {
    fn dispose(&self) {}
}

impl MainThreadTelemetryShape for MockMainThreadTelemetry {
    fn rpc_public_log(&self, _event_name: &str, _data: Option<Dynamic>) {}

    fn rpc_public_log2(&self, _event_name: &str, _data: Option<Dynamic>) {}
}

/// O `TestRPCProtocol` (ver as notas do módulo).
#[derive(Default)]
pub struct TestMainContext {
    proxies: RefCell<HashMap<&'static str, Rc<dyn Any>>>,
}

impl TestMainContext {
    pub fn new() -> Self {
        Self::default()
    }

    /// `rpcProtocol.set(identifier, value)`.
    pub fn set<T: ?Sized + 'static>(&self, identifier: &ProxyIdentifier<T>, value: Rc<T>) {
        self.proxies.borrow_mut().insert(identifier.sid, Rc::new(value));
    }
}

impl IMainContext for TestMainContext {
    fn get_proxy_erased(&self, sid: &'static str) -> Rc<dyn Any> {
        if let Some(proxy) = self.proxies.borrow().get(sid) {
            return proxy.clone();
        }
        match sid {
            "MainThreadWorkspace" => Rc::new(Rc::new(MockMainThreadWorkspace::default()) as Rc<dyn MainThreadWorkspaceShape>),
            "MainThreadMessageService" => Rc::new(Rc::new(MockMainThreadMessageService) as Rc<dyn MainThreadMessageServiceShape>),
            "MainThreadTelemetry" => Rc::new(Rc::new(MockMainThreadTelemetry) as Rc<dyn MainThreadTelemetryShape>),
            other => panic!("proxy inesperado: {other}"),
        }
    }

    fn set_erased(&self, sid: &'static str, value: Rc<dyn Any>) {
        self.proxies.borrow_mut().insert(sid, value);
    }
}

// ---- `sync()` do TestRPCProtocol (bloco próprio: `extHostCommentThreads.test.ts`) ----------------------------

impl TestMainContext {
    /// `rpcProtocol.sync()`: espera uma macrotarefa (`setTimeout(c, 0)`) e depois as chamadas remotas em voo. Aqui os proxies
    /// respondem na própria chamada, então nunca há chamada em voo e só resta a macrotarefa.
    pub fn sync(&self) -> Promise<()> {
        base::timeout(0)
    }
}

// ---- os serviços ---------------------------------------------------------------------------------------

/// `new class extends mock<IExtHostFileSystemInfo>() { override getCapabilities() { return isLinux ? PathCaseSensitive : undefined } }`.
pub struct TestFileSystemInfo;

impl IExtHostFileSystemInfo for TestFileSystemInfo {
    fn get_capabilities(&self, _scheme: &str) -> Option<FileSystemProviderCapabilities> {
        cfg!(target_os = "linux").then_some(FileSystemProviderCapabilities::PATH_CASE_SENSITIVE)
    }
}

/// `new class extends mock<IExtHostInitDataService>() { override workspace = data; }`.
pub struct TestInitData(pub Option<IWorkspaceData>);

impl IExtHostInitDataService for TestInitData {
    fn workspace(&self) -> Option<IWorkspaceData> {
        self.0.clone()
    }
}

/// `new class extends mock<IURITransformerService>() { }`.
pub struct TestUriTransformer;

impl IURITransformerService for TestUriTransformer {
    fn transform_outgoing_scheme(&self, _scheme: &str) -> String {
        panic!("transformOutgoingScheme is not a function")
    }
}

/// `new ExtHostWorkspace(new TestRPCProtocol(), ...)` sem o `$initializeWorkspace` (o que `createExtHostWorkspace` de
/// `extHostConfiguration.test.ts` faz).
pub fn new_ext_host_workspace(main_context: &TestMainContext, data: Option<IWorkspaceData>) -> Rc<ExtHostWorkspace> {
    ExtHostWorkspace::new(main_context, &TestInitData(data), Rc::new(TestFileSystemInfo), NullLogService::shared(), Rc::new(TestUriTransformer))
}

/// `createExtHostWorkspace(mainContext, data, logService)` de `extHostWorkspace.test.ts`: registra o telemetria vazia,
/// cria o host e o inicializa como confiável.
pub fn create_ext_host_workspace(main_context: &TestMainContext, data: Option<IWorkspaceData>) -> Rc<ExtHostWorkspace> {
    main_context.set(&MainContext::MAIN_THREAD_TELEMETRY, Rc::new(MockMainThreadTelemetry) as Rc<dyn MainThreadTelemetryShape>);
    let result = new_ext_host_workspace(main_context, data.clone());
    result.rpc_initialize_workspace(data, true);
    result
}

/// `aWorkspaceFolderData(uri, index, name = '')`: o nome padrão é o `basename` do caminho.
pub fn a_workspace_folder_data(uri: URI, index: usize, name: &str) -> WorkspaceFolderDataDto {
    let name = if name.is_empty() { uri.path().to_string_lossy().rsplit('/').next().unwrap_or_default().to_owned() } else { name.to_owned() };
    WorkspaceFolderDataDto { uri, name, index }
}
