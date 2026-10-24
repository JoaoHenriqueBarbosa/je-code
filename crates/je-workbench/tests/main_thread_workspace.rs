//! Porte de `src/vs/workbench/api/test/browser/mainThreadWorkspace.test.ts` (suíte `MainThreadWorkspace`), caso a caso e
//! na mesma ordem.
//!
//! Notas de tradução:
//! - **`setup`.** `workbenchInstantiationService(undefined, disposables)` é [`setup`], que monta só os serviços de que o
//!   `MainThreadWorkspace` e o `QueryBuilder` dependem, com os mesmos dublês do original (`TestConfigurationService`,
//!   `TestContextService`, `TestFileService`, `TestEditorService`, `TestNotificationService`, os de confiança do
//!   workspace e o `UriIdentityService` real). O `LabelService` real pede armazenamento e ciclo de vida, que o caso
//!   não exercita: o `MockLabelService` do crate o substitui. `configService.setUserConfiguration('search', {})` é feito
//!   no fim do `setup`. O guarda de vazamentos é criado depois dos serviços (como em `query_builder.rs`) e descarta o
//!   `MainThreadWorkspace` ao fim do caso.
//! - **Serviços sem registro.** O `TestInstantiationService` do original não é estrito: `ISearchService` (cada caso o
//!   registra com `stub`), `IEditSessionIdentityService`, `ICanonicalUriService`, `IWorkspaceEditingService` e
//!   `IRequestService` chegam `undefined` ao construtor, e nenhum caso os chama. Em Rust um `Rc` não pode ser nulo, então
//!   cada um é um dublê que falha com `panic!` ao ser chamado (como o `undefined` lançaria `TypeError`); o mesmo vale
//!   para o `ITextFileService` (o `TestTextFileService` completo não foi portado) e para os métodos do proxy do
//!   extension host que o `{ $initializeWorkspace: () => { } }` do original não tem.
//! - **`instantiationService.stub(ISearchService, { fileSearch(query) { ... } })`** é o [`FileSearchStub`], cujo
//!   `fileSearch` roda o fecho do caso e responde `{ results: [], messages: [] }`; os outros métodos falham.
//! - **`SingleProxyRPCProtocol(...)`** é `single_proxy_rpc_protocol` (o proxy único devolvido para qualquer
//!   identificador).
//! - **Casos que devolvem a promessa.** `return mtw.$startFileSearch(...)` é o `await` da promessa dentro de
//!   `run_with_faked_timers`: rejeitar a promessa reprova o caso, como no mocha.
//! - **Opções.** `{ maxResults, includePattern, disregardSearchExcludeSettings }` é o `IFileQueryBuilderOptions` com os
//!   campos comuns em `common`; `{ ...query.includePattern }` e `{ ...query.excludePattern }` são comparados como
//!   expressões sem considerar a ordem das chaves; `deepStrictEqual(x, [])` é `Some(Vec::new())` e
//!   `deepStrictEqual(x, undefined)` é `None`.
//! - **`UriComponents`.** O objeto `{ scheme, path }` do último caso é [`UriComponents`]; o `URI.revive` do original já
//!   foi feito pela camada de RPC, então `URI.isUri(...)` é o tipo do campo.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod support;

#[path = "../../je-editor/tests/support/test_instantiation_service.rs"]
mod test_instantiation_service;

use std::rc::Rc;

use common::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite, run_with_faked_timers};
use je_base::cancellation::CancellationToken;
use je_base::dynamic::Dynamic;
use je_base::event::Event;
use je_base::glob::Expression;
use je_base::lifecycle::DisposableRef;
use je_base::parts::request::{IRequestContext, IRequestOptions};
use je_base::promise::Promise;
use je_base::uri::{URI, UriComponents};
use je_base::JsString;
use je_instantiation::Injectable;
use je_platform::configuration::service::I_CONFIGURATION_SERVICE;
use je_platform::configuration::test_configuration_service::TestConfigurationService;
use je_platform::canonical_uri::{I_CANONICAL_URI_SERVICE, ICanonicalUriProvider, ICanonicalUriService};
use je_platform::edit_sessions::{
    EditSessionIdentityMatch, I_EDIT_SESSION_IDENTITY_SERVICE, IEditSessionIdentityCreateParticipant, IEditSessionIdentityProvider, IEditSessionIdentityService,
};
use je_platform::environment::environment::I_ENVIRONMENT_SERVICE;
use je_platform::files::files::FileStatWithMetadata;
use je_platform::files::i_file_service::{I_FILE_SERVICE, IFileService};
use je_platform::label::label::I_LABEL_SERVICE;
use je_platform::log::{I_LOG_SERVICE, NullLogger};
use je_platform::notification::notification::I_NOTIFICATION_SERVICE;
use je_platform::path::path_service::I_PATH_SERVICE;
use je_platform::request::request::{AuthInfo, Credentials, I_REQUEST_SERVICE, IRequestService, RequestCompleteEvent};
use je_platform::uri_identity::uri_identity::I_URI_IDENTITY_SERVICE;
use je_platform::uri_identity::uri_identity_service::UriIdentityService;
use je_platform::workspace::I_WORKSPACE_CONTEXT_SERVICE;
use je_platform::workspace::WorkspaceFolder;
use je_platform::workspace_trust::{I_WORKSPACE_TRUST_MANAGEMENT_SERVICE, I_WORKSPACE_TRUST_REQUEST_SERVICE};
use je_platform::workspaces::workspaces::WorkspaceFolderCreationData;
use je_workbench::api::browser::main_thread_workspace::MainThreadWorkspace;
use je_workbench::api::common::ext_host_protocol::{
    ExtHostWorkspaceShape, IRawFileMatch2, IWorkspaceData,
};
use je_workbench::services::editor::editor_groups_service::I_EDITOR_GROUPS_SERVICE;
use je_workbench::services::editor::editor_service::I_EDITOR_SERVICE;
use je_workbench::services::search::query_builder::{ICommonQueryBuilderOptions, IFileQueryBuilderOptions, SearchPathPatternBuilder, SearchPatternBuilder};
use je_workbench::services::search::search::{
    I_SEARCH_SERVICE, IAITextQuery, IFileQuery, ISearchComplete, ISearchResultProvider, ISearchService, ITextQuery, SearchProgressCallback, SearchProviderType,
    SharedResourceSet, TextSearchSplitSyncAsync,
};
use je_workbench::services::textfile::collaborators::FileOperationUndoRedoInfo;
use je_workbench::services::textfile::textfiles::{
    EncodedContent, I_TEXT_FILE_SERVICE, IResourceEncodings, ITextFileEditorModelManager, ITextFileService, ReadTextFileEncodingOptions, ReadTextFileOptions, ResolvedDecoding,
    ResolvedEncoding, TextFileContent, TextFileCreateOperation, TextFileSaveAsOptions, TextFileSaveOptions, TextFileStreamContent, TextOrSnapshot, WriteTextFileOptions,
};
use je_workbench::services::untitled::untitled_text_editor_service::IUntitledTextEditorModelManager;
use je_workbench::services::workspaces::workspace_editing::{DidEnterWorkspaceEvent, I_WORKSPACE_EDITING_SERVICE, IWorkspaceEditingService};
use je_workbench::common::editor::RevertOptions;
use je_workbench::test::browser::workbench_test_services::{MockLabelService, TestEditorGroupsService, TestEditorService, TestEnvironmentService, TestPathService};
use je_workbench::test::common::workbench_test_services::{
    TestContextService, TestFileService, TestNotificationService, TestWorkspaceTrustManagementService, TestWorkspaceTrustRequestService,
};
use support::test_rpc_protocol::single_proxy_rpc_protocol;
use test_instantiation_service::TestInstantiationService;

use je_base::buffer::VSBufferReadableStream;
use je_base::stream::ReadableStream;

// ---- os dublês --------------------------------------------------------------------------------------

/// `{ fileSearch(query) { ... } }`: o `ISearchService` que só responde `fileSearch`.
struct FileSearchStub {
    on_file_search: Box<dyn Fn(&IFileQuery)>,
}

impl FileSearchStub {
    fn new(on_file_search: impl Fn(&IFileQuery) + 'static) -> Rc<Self> {
        Rc::new(Self { on_file_search: Box::new(on_file_search) })
    }
}

impl ISearchService for FileSearchStub {
    fn text_search(&self, _query: &ITextQuery, _token: Option<CancellationToken>, _on_progress: Option<SearchProgressCallback>) -> Promise<ISearchComplete> {
        panic!("textSearch is not a function")
    }

    fn ai_text_search(&self, _query: &IAITextQuery, _token: Option<CancellationToken>, _on_progress: Option<SearchProgressCallback>) -> Promise<ISearchComplete> {
        panic!("aiTextSearch is not a function")
    }

    fn get_ai_name(&self) -> Promise<Option<String>> {
        panic!("getAIName is not a function")
    }

    fn text_search_split_sync_async(
        &self,
        _query: &ITextQuery,
        _token: Option<CancellationToken>,
        _on_progress: Option<SearchProgressCallback>,
        _notebook_files_to_ignore: Option<SharedResourceSet>,
        _async_notebook_files_to_ignore: Option<Promise<SharedResourceSet>>,
    ) -> TextSearchSplitSyncAsync {
        panic!("textSearchSplitSyncAsync is not a function")
    }

    fn file_search(&self, query: &IFileQuery, _token: Option<CancellationToken>) -> Promise<ISearchComplete> {
        (self.on_file_search)(query);
        Promise::resolved(ISearchComplete { limit_hit: None, messages: Vec::new(), stats: None, results: Vec::new(), exit: None, ai_keywords: None })
    }

    fn scheme_has_file_search_provider(&self, _scheme: &str) -> bool {
        panic!("schemeHasFileSearchProvider is not a function")
    }

    fn clear_cache(&self, _cache_key: &str) -> Promise<()> {
        panic!("clearCache is not a function")
    }

    fn register_search_result_provider(&self, _scheme: &str, _provider_type: SearchProviderType, _provider: Rc<dyn ISearchResultProvider>) -> DisposableRef {
        panic!("registerSearchResultProvider is not a function")
    }
}

/// O `IRequestService` que o original deixa `undefined`.
struct UnregisteredRequestService;

impl IRequestService for UnregisteredRequestService {
    fn on_did_complete_request(&self) -> Event<RequestCompleteEvent> {
        panic!("Cannot read properties of undefined (reading 'onDidCompleteRequest')")
    }

    fn request(&self, _options: IRequestOptions, _token: CancellationToken) -> Promise<IRequestContext> {
        panic!("Cannot read properties of undefined (reading 'request')")
    }

    fn resolve_proxy(&self, _url: &str) -> Promise<Option<String>> {
        panic!("Cannot read properties of undefined (reading 'resolveProxy')")
    }

    fn lookup_authorization(&self, _auth_info: AuthInfo) -> Promise<Option<Credentials>> {
        panic!("Cannot read properties of undefined (reading 'lookupAuthorization')")
    }

    fn lookup_kerberos_authorization(&self, _url: &str) -> Promise<Option<String>> {
        panic!("Cannot read properties of undefined (reading 'lookupKerberosAuthorization')")
    }

    fn load_certificates(&self) -> Promise<Vec<String>> {
        panic!("Cannot read properties of undefined (reading 'loadCertificates')")
    }
}

/// O `IEditSessionIdentityService` que o original deixa `undefined`.
struct UnregisteredEditSessionIdentityService;

impl IEditSessionIdentityService for UnregisteredEditSessionIdentityService {
    fn register_edit_session_identity_provider(&self, _provider: Rc<dyn IEditSessionIdentityProvider>) -> DisposableRef {
        panic!("Cannot read properties of undefined (reading 'registerEditSessionIdentityProvider')")
    }

    fn get_edit_session_identifier(&self, _workspace_folder: &WorkspaceFolder, _cancellation_token: CancellationToken) -> Promise<Option<String>> {
        panic!("Cannot read properties of undefined (reading 'getEditSessionIdentifier')")
    }

    fn provide_edit_session_identity_match(
        &self,
        _workspace_folder: &WorkspaceFolder,
        _identity1: &str,
        _identity2: &str,
        _cancellation_token: CancellationToken,
    ) -> Promise<Option<EditSessionIdentityMatch>> {
        panic!("Cannot read properties of undefined (reading 'provideEditSessionIdentityMatch')")
    }

    fn add_edit_session_identity_create_participant(&self, _participants: Rc<dyn IEditSessionIdentityCreateParticipant>) -> DisposableRef {
        panic!("Cannot read properties of undefined (reading 'addEditSessionIdentityCreateParticipant')")
    }

    fn on_will_create_edit_session_identity(&self, _workspace_folder: &WorkspaceFolder, _cancellation_token: CancellationToken) -> Promise<()> {
        panic!("Cannot read properties of undefined (reading 'onWillCreateEditSessionIdentity')")
    }
}

/// O `ICanonicalUriService` que o original deixa `undefined`.
struct UnregisteredCanonicalUriService;

impl ICanonicalUriService for UnregisteredCanonicalUriService {
    fn register_canonical_uri_provider(&self, _provider: Rc<dyn ICanonicalUriProvider>) -> DisposableRef {
        panic!("Cannot read properties of undefined (reading 'registerCanonicalUriProvider')")
    }
}

/// O `IWorkspaceEditingService` que o original deixa `undefined`.
struct UnregisteredWorkspaceEditingService;

impl IWorkspaceEditingService for UnregisteredWorkspaceEditingService {
    fn on_did_enter_workspace(&self) -> Event<DidEnterWorkspaceEvent> {
        panic!("Cannot read properties of undefined (reading 'onDidEnterWorkspace')")
    }

    fn add_folders(&self, _folders: Vec<WorkspaceFolderCreationData>, _donot_notify_error: Option<bool>) -> Promise<()> {
        panic!("Cannot read properties of undefined (reading 'addFolders')")
    }

    fn remove_folders(&self, _folders: Vec<URI>, _donot_notify_error: Option<bool>) -> Promise<()> {
        panic!("Cannot read properties of undefined (reading 'removeFolders')")
    }

    fn update_folders(&self, _index: f64, _delete_count: Option<f64>, _folders_to_add: Option<Vec<WorkspaceFolderCreationData>>, _donot_notify_error: Option<bool>) -> Promise<()> {
        panic!("Cannot read properties of undefined (reading 'updateFolders')")
    }

    fn enter_workspace(&self, _path: &URI) -> Promise<()> {
        panic!("Cannot read properties of undefined (reading 'enterWorkspace')")
    }

    fn create_and_enter_workspace(&self, _folders: Vec<WorkspaceFolderCreationData>, _path: Option<URI>) -> Promise<()> {
        panic!("Cannot read properties of undefined (reading 'createAndEnterWorkspace')")
    }

    fn save_and_enter_workspace(&self, _path: &URI) -> Promise<()> {
        panic!("Cannot read properties of undefined (reading 'saveAndEnterWorkspace')")
    }

    fn copy_workspace_settings(&self, _to_workspace: &je_platform::workspace::WorkspaceIdentifier) -> Promise<()> {
        panic!("Cannot read properties of undefined (reading 'copyWorkspaceSettings')")
    }

    fn pick_new_workspace_path(&self) -> Promise<Option<URI>> {
        panic!("Cannot read properties of undefined (reading 'pickNewWorkspacePath')")
    }
}

/// O `ITextFileService` que o caso não exercita (o `TestTextFileService` completo entra com o `TextFileService` abstrato).
struct UnexercisedTextFileService;

impl ITextFileService for UnexercisedTextFileService {
    fn files(&self) -> Rc<dyn ITextFileEditorModelManager> {
        unreachable!("TestTextFileService.files não é exercitado por esta suíte")
    }

    fn untitled(&self) -> Rc<dyn IUntitledTextEditorModelManager> {
        unreachable!("TestTextFileService.untitled não é exercitado por esta suíte")
    }

    fn encoding(&self) -> Rc<dyn IResourceEncodings> {
        unreachable!("TestTextFileService.encoding não é exercitado por esta suíte")
    }

    fn is_dirty(&self, _resource: &URI) -> bool {
        unreachable!("TestTextFileService.isDirty não é exercitado por esta suíte")
    }

    fn save(&self, _resource: &URI, _options: Option<TextFileSaveOptions>) -> Promise<Option<URI>> {
        unreachable!("TestTextFileService.save não é exercitado por esta suíte")
    }

    fn save_as(&self, _resource: &URI, _target_resource: Option<&URI>, _options: Option<TextFileSaveAsOptions>) -> Promise<Option<URI>> {
        unreachable!("TestTextFileService.saveAs não é exercitado por esta suíte")
    }

    fn revert(&self, _resource: &URI, _options: Option<RevertOptions>) -> Promise<()> {
        unreachable!("TestTextFileService.revert não é exercitado por esta suíte")
    }

    fn read(&self, _resource: &URI, _options: Option<ReadTextFileOptions>) -> Promise<TextFileContent> {
        unreachable!("TestTextFileService.read não é exercitado por esta suíte")
    }

    fn read_stream(&self, _resource: &URI, _options: Option<ReadTextFileOptions>) -> Promise<TextFileStreamContent> {
        unreachable!("TestTextFileService.readStream não é exercitado por esta suíte")
    }

    fn write(&self, _resource: &URI, _value: TextOrSnapshot, _options: Option<WriteTextFileOptions>) -> Promise<FileStatWithMetadata> {
        unreachable!("TestTextFileService.write não é exercitado por esta suíte")
    }

    fn create(&self, _operations: Vec<TextFileCreateOperation>, _undo_info: Option<FileOperationUndoRedoInfo>) -> Promise<Vec<FileStatWithMetadata>> {
        unreachable!("TestTextFileService.create não é exercitado por esta suíte")
    }

    fn get_encoded_readable(&self, _resource: Option<&URI>, _value: Option<TextOrSnapshot>, _options: Option<WriteTextFileOptions>) -> Promise<Option<EncodedContent>> {
        unreachable!("TestTextFileService.getEncodedReadable não é exercitado por esta suíte")
    }

    fn get_decoded_stream(&self, _resource: Option<&URI>, _value: VSBufferReadableStream, _options: Option<ReadTextFileEncodingOptions>) -> Promise<Rc<dyn ReadableStream<String>>> {
        unreachable!("TestTextFileService.getDecodedStream não é exercitado por esta suíte")
    }

    fn get_encoding(&self, _resource: &URI) -> String {
        unreachable!("TestTextFileService.getEncoding não é exercitado por esta suíte")
    }

    fn resolve_decoding(&self, _resource: Option<&URI>, _options: Option<ReadTextFileEncodingOptions>) -> Promise<ResolvedDecoding> {
        unreachable!("TestTextFileService.resolveDecoding não é exercitado por esta suíte")
    }

    fn resolve_encoding(&self, _resource: Option<&URI>, _options: Option<WriteTextFileOptions>) -> Promise<ResolvedEncoding> {
        unreachable!("TestTextFileService.resolveEncoding não é exercitado por esta suíte")
    }

    fn validate_detected_encoding(&self, _resource: Option<&URI>, _detected_encoding: &str, _options: Option<ReadTextFileEncodingOptions>) -> Promise<String> {
        unreachable!("TestTextFileService.validateDetectedEncoding não é exercitado por esta suíte")
    }
}

/// `{ $initializeWorkspace: () => { } }`: um método sem resposta falha como o método ausente do objeto lançaria
/// `TypeError`.
struct InitializeWorkspaceOnlyProxy;

impl ExtHostWorkspaceShape for InitializeWorkspaceOnlyProxy {
    fn rpc_initialize_workspace(&self, _workspace: Option<IWorkspaceData>, _trusted: bool) {}

    fn rpc_accept_workspace_data(&self, _workspace: Option<IWorkspaceData>) {
        panic!("this._proxy.$acceptWorkspaceData is not a function")
    }

    fn rpc_handle_text_search_result(&self, _result: IRawFileMatch2, _request_id: u64) {
        panic!("this._proxy.$handleTextSearchResult is not a function")
    }

    fn rpc_on_did_grant_workspace_trust(&self) {
        panic!("this._proxy.$onDidGrantWorkspaceTrust is not a function")
    }

    fn rpc_on_did_change_workspace_trusted_folders(&self) {
        panic!("this._proxy.$onDidChangeWorkspaceTrustedFolders is not a function")
    }

    fn rpc_get_edit_session_identifier(&self, _folder: URI, _token: CancellationToken) -> Promise<Option<String>> {
        panic!("this._proxy.$getEditSessionIdentifier is not a function")
    }

    fn rpc_provide_edit_session_identity_match(&self, _folder: URI, _identity1: &str, _identity2: &str, _token: CancellationToken) -> Promise<Option<EditSessionIdentityMatch>> {
        panic!("this._proxy.$provideEditSessionIdentityMatch is not a function")
    }

    fn rpc_on_will_create_edit_session_identity(&self, _folder: URI, _token: CancellationToken, _timeout: f64) -> Promise<()> {
        panic!("this._proxy.$onWillCreateEditSessionIdentity is not a function")
    }

    fn rpc_provide_canonical_uri(&self, _uri: URI, _target_scheme: &str, _token: CancellationToken) -> Promise<Option<URI>> {
        panic!("this._proxy.$provideCanonicalUri is not a function")
    }
}

// ---- o setup ----------------------------------------------------------------------------------------

/// `setup()`: o `workbenchInstantiationService` com a configuração `search` vazia, e o guarda de vazamentos.
fn setup() -> (LeakGuard, Rc<TestInstantiationService>, Rc<TestConfigurationService>) {
    let instantiation_service = TestInstantiationService::new();

    let config_service = TestConfigurationService::new(None);
    let file_service = TestFileService::new();
    instantiation_service.stub(I_CONFIGURATION_SERVICE, config_service.clone());
    instantiation_service.stub(I_WORKSPACE_CONTEXT_SERVICE, TestContextService::new(None, None));
    instantiation_service.stub(I_EDITOR_GROUPS_SERVICE, TestEditorGroupsService::new(None));
    instantiation_service.stub(I_LOG_SERVICE, Rc::new(NullLogger::new()));
    instantiation_service.stub(I_PATH_SERVICE, TestPathService::new(None, None));
    instantiation_service.stub(I_FILE_SERVICE, file_service.clone());
    let file_service_contract: Rc<dyn IFileService> = file_service;
    instantiation_service.stub(I_URI_IDENTITY_SERVICE, UriIdentityService::create((), (file_service_contract,)));
    instantiation_service.stub(I_ENVIRONMENT_SERVICE, TestEnvironmentService::new());
    instantiation_service.stub(I_NOTIFICATION_SERVICE, Rc::new(TestNotificationService));
    instantiation_service.stub(I_LABEL_SERVICE, MockLabelService::new(None));
    instantiation_service.stub(I_EDITOR_SERVICE, Rc::new(TestEditorService));
    instantiation_service.stub(I_WORKSPACE_TRUST_MANAGEMENT_SERVICE, TestWorkspaceTrustManagementService::new(None, None));
    instantiation_service.stub(I_WORKSPACE_TRUST_REQUEST_SERVICE, TestWorkspaceTrustRequestService::new(false));
    instantiation_service.stub(I_TEXT_FILE_SERVICE, Rc::new(UnexercisedTextFileService));
    instantiation_service.stub(I_REQUEST_SERVICE, Rc::new(UnregisteredRequestService));
    instantiation_service.stub(I_EDIT_SESSION_IDENTITY_SERVICE, Rc::new(UnregisteredEditSessionIdentityService));
    instantiation_service.stub(I_CANONICAL_URI_SERVICE, Rc::new(UnregisteredCanonicalUriService));
    instantiation_service.stub(I_WORKSPACE_EDITING_SERVICE, Rc::new(UnregisteredWorkspaceEditingService));

    let _ = config_service.set_user_configuration("search", Dynamic::object(Vec::<(&str, Dynamic)>::new()), None);

    (ensure_no_disposables_are_leaked_in_test_suite(), instantiation_service, config_service)
}

/// `disposables.add(instantiationService.createInstance(MainThreadWorkspace, SingleProxyRPCProtocol({ $initializeWorkspace: () => { } })))`.
fn create_main_thread_workspace(guard: &LeakGuard, instantiation_service: &TestInstantiationService) -> Rc<MainThreadWorkspace> {
    let proxy: Rc<dyn ExtHostWorkspaceShape> = Rc::new(InitializeWorkspaceOnlyProxy);
    guard.add_rc(instantiation_service.create_instance::<MainThreadWorkspace>(single_proxy_rpc_protocol(proxy)))
}

fn search_configuration(exclude: Option<Dynamic>) -> Dynamic {
    match exclude {
        Some(exclude) => Dynamic::object([("exclude", exclude)]),
        None => Dynamic::object(Vec::<(&str, Dynamic)>::new()),
    }
}

fn exclude_configuration(key: &'static str) -> Dynamic {
    Dynamic::object([(key, Dynamic::Bool(true))])
}

fn expression_of(patterns: &[&'static str]) -> Expression {
    Expression::from_entries(patterns.iter().map(|pattern| (*pattern, true)))
}

fn file_options(common: ICommonQueryBuilderOptions) -> IFileQueryBuilderOptions {
    IFileQueryBuilderOptions { common, ..IFileQueryBuilderOptions::default() }
}

/// O `{ ...expression }`: as chaves em ordem, que o `deepStrictEqual` do JavaScript não olha.
fn sorted(expression: &Expression) -> Expression {
    let mut entries = expression.entries().to_vec();
    entries.sort_by(|a, b| a.0.cmp(&b.0));
    Expression::from_entries(entries)
}

/// Roda `mtw.$startFileSearch(null, options, CancellationToken.None)` e espera a promessa.
fn start_file_search(mtw: Rc<MainThreadWorkspace>, include_folder: Option<URI>, options: IFileQueryBuilderOptions) {
    use je_workbench::api::common::ext_host_protocol::MainThreadWorkspaceShape;
    run_with_faked_timers(async move {
        mtw.rpc_start_file_search(include_folder, options, CancellationToken::None).await.expect("a busca de arquivos rejeitou");
    });
}

// ---- os casos ---------------------------------------------------------------------------------------

#[test]
fn simple() {
    let (guard, instantiation_service, _config_service) = setup();
    instantiation_service.stub(
        I_SEARCH_SERVICE,
        FileSearchStub::new(|query| {
            assert_eq!(query.common.folder_queries.len(), 1);
            assert_eq!(query.common.folder_queries[0].disregard_ignore_files, Some(true));

            assert_eq!(sorted(query.common.include_pattern.as_ref().expect("includePattern")), expression_of(&["foo"]));
            assert_eq!(query.common.max_results, Some(10));
        }),
    );

    let mtw = create_main_thread_workspace(&guard, &instantiation_service);
    start_file_search(
        mtw,
        None,
        file_options(ICommonQueryBuilderOptions {
            max_results: Some(10),
            include_pattern: Some(SearchPathPatternBuilder::from("foo")),
            disregard_search_exclude_settings: Some(true),
            ..Default::default()
        }),
    );
}

#[test]
fn exclude_defaults() {
    let (guard, instantiation_service, config_service) = setup();
    let _ = config_service.set_user_configuration("search", search_configuration(Some(exclude_configuration("searchExclude"))), None);
    let _ = config_service.set_user_configuration("files", search_configuration(Some(exclude_configuration("filesExclude"))), None);

    instantiation_service.stub(
        I_SEARCH_SERVICE,
        FileSearchStub::new(|query| {
            assert_eq!(query.common.folder_queries.len(), 1);
            assert_eq!(query.common.folder_queries[0].disregard_ignore_files, Some(true));
            let exclude_pattern = query.common.folder_queries[0].exclude_pattern.as_ref().expect("excludePattern");
            assert_eq!(exclude_pattern.len(), 1);
            assert_eq!(sorted(&exclude_pattern[0].pattern), expression_of(&["filesExclude"]));
        }),
    );

    let mtw = create_main_thread_workspace(&guard, &instantiation_service);
    start_file_search(
        mtw,
        None,
        file_options(ICommonQueryBuilderOptions {
            max_results: Some(10),
            include_pattern: Some(SearchPathPatternBuilder::from("")),
            disregard_search_exclude_settings: Some(true),
            ..Default::default()
        }),
    );
}

#[test]
fn disregard_excludes() {
    let (guard, instantiation_service, config_service) = setup();
    let _ = config_service.set_user_configuration("search", search_configuration(Some(exclude_configuration("searchExclude"))), None);
    let _ = config_service.set_user_configuration("files", search_configuration(Some(exclude_configuration("filesExclude"))), None);

    instantiation_service.stub(
        I_SEARCH_SERVICE,
        FileSearchStub::new(|query| {
            assert_eq!(query.common.folder_queries[0].exclude_pattern, Some(Vec::new()));
            assert_eq!(query.common.exclude_pattern, None);
        }),
    );

    let mtw = create_main_thread_workspace(&guard, &instantiation_service);
    start_file_search(
        mtw,
        None,
        file_options(ICommonQueryBuilderOptions {
            max_results: Some(10),
            include_pattern: Some(SearchPathPatternBuilder::from("")),
            disregard_search_exclude_settings: Some(true),
            disregard_exclude_settings: Some(true),
            ..Default::default()
        }),
    );
}

#[test]
fn do_not_disregard_anything_if_disregard_exclude_settings_is_true() {
    let (guard, instantiation_service, config_service) = setup();
    let _ = config_service.set_user_configuration("search", search_configuration(Some(exclude_configuration("searchExclude"))), None);
    let _ = config_service.set_user_configuration("files", search_configuration(Some(exclude_configuration("filesExclude"))), None);

    instantiation_service.stub(
        I_SEARCH_SERVICE,
        FileSearchStub::new(|query| {
            assert_eq!(query.common.folder_queries.len(), 1);
            assert_eq!(query.common.folder_queries[0].disregard_ignore_files, Some(true));
            assert_eq!(query.common.folder_queries[0].exclude_pattern, Some(Vec::new()));
        }),
    );

    let mtw = create_main_thread_workspace(&guard, &instantiation_service);
    start_file_search(
        mtw,
        None,
        file_options(ICommonQueryBuilderOptions {
            max_results: Some(10),
            include_pattern: Some(SearchPathPatternBuilder::from("")),
            disregard_exclude_settings: Some(true),
            disregard_search_exclude_settings: Some(false),
            ..Default::default()
        }),
    );
}

#[test]
fn exclude_string() {
    let (guard, instantiation_service, _config_service) = setup();
    instantiation_service.stub(
        I_SEARCH_SERVICE,
        FileSearchStub::new(|query| {
            assert_eq!(query.common.folder_queries[0].exclude_pattern, Some(Vec::new()));
            assert_eq!(sorted(query.common.exclude_pattern.as_ref().expect("excludePattern")), expression_of(&["exclude/**"]));
        }),
    );

    let mtw = create_main_thread_workspace(&guard, &instantiation_service);
    start_file_search(
        mtw,
        None,
        file_options(ICommonQueryBuilderOptions {
            max_results: Some(10),
            include_pattern: Some(SearchPathPatternBuilder::from("")),
            exclude_pattern: Some(vec![SearchPatternBuilder { uri: None, pattern: SearchPathPatternBuilder::from("exclude/**") }]),
            disregard_search_exclude_settings: Some(true),
            ..Default::default()
        }),
    );
}

#[test]
fn valid_revived_uri_after_moving_to_eh() {
    let (guard, instantiation_service, _config_service) = setup();
    let uri_components = UriComponents { scheme: Some(JsString::from("test")), path: Some(JsString::from("/Users/username/Downloads")), ..UriComponents::default() };
    let folder = URI::from(&uri_components, false);
    instantiation_service.stub(
        I_SEARCH_SERVICE,
        FileSearchStub::new(|query| {
            assert_eq!(query.common.folder_queries.len(), 1);
            assert_eq!(query.common.folder_queries[0].folder.path().to_string_lossy(), "/Users/username/Downloads");
            assert_eq!(query.common.folder_queries[0].folder.scheme().to_string_lossy(), "test");
        }),
    );

    let mtw = create_main_thread_workspace(&guard, &instantiation_service);
    start_file_search(mtw, Some(folder), IFileQueryBuilderOptions { file_pattern: Some("*.md".to_string()), ..IFileQueryBuilderOptions::default() });
}
