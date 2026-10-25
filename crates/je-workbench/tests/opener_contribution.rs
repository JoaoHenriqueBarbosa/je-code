//! Porte de `src/vs/workbench/contrib/opener/test/browser/opener.contribution.test.ts` (suíte
//! `WorkbenchOpenerContribution`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` é o guarda criado no começo de cada caso, e `store.add(...)` é o
//!   `add_rc` dele. Os serviços são passados direto ao construtor, sem contêiner de injeção.
//! - `upcastPartial<I...>({ ... })` são os dublês abaixo: cada um define só os membros do objeto do original, e os outros
//!   falham com `panic!`, como o `TypeError` do método ausente.
//! - `calls: object[]` é um `Vec<Call>` com um variante por forma de objeto empurrado (`{ stat }`, `{ command, uri }`,
//!   `{ external, options }`), e o `assert.deepStrictEqual` é um `assert_eq!`. As opções do abrir externo
//!   (`{ openExternal, fromUserGesture }`) são os dois campos que o original compara.
//! - `registered!.open(uri, openOptions)` dentro do `open` do dublê do serviço confere que o abridor devolve `false` para
//!   a abertura externa, como no original.
//! - Os `for` e as suas combinações viram um teste por combinação, na ordem do original (`sessions` falso e depois
//!   verdadeiro, e as URIs na ordem da lista). Os nomes em template (`${sessions}`, `${uri}`) viram o sufixo do nome.
//! - `assert.rejects(promise, error)` compara a mensagem do erro; `assert.rejects(promise, /regex/)`, o trecho dela.
//! - `await` esvazia as microtarefas dentro de `run_with_faked_timers`, onde elas andam.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use std::cell::RefCell;
use std::rc::Rc;

use common::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite, run_with_faked_timers};
use je_base::JsString;
use je_base::async_::async_fn;
use je_base::dynamic::Dynamic;
use je_base::errors::{self, Error};
use je_base::event::Event;
use je_base::lifecycle::{self, DisposableRef};
use je_base::network::schemas;
use je_base::promise::Promise;
use je_base::uri::{UriChange, UriComponents, URI};
use je_platform::commands::commands::{CommandEvent, ICommandService};
use je_platform::files::file_service::FileContentSource;
use je_platform::files::files::{
    CreateFileOptions, FileChangesEvent, FileContent, FileOperationError, FileOperationEvent, FileOperationResult, FileStat, FileStatWithMetadata,
    FileStreamContent, FileSystemProviderActivationEvent, FileSystemProviderCapabilities, FileSystemProviderCapabilitiesChangeEvent, FileSystemProviderRef,
    FileSystemProviderRegistrationEvent, PartialFileDeleteOptions, ReadFileOptions, ResolveFileOptions, WatchOptions, WriteFileOptions,
};
use je_platform::files::i_file_service::IFileService;
use je_platform::opener::{
    IExternalOpener, IExternalUriResolver, IOpener, IOpenerService, IValidator, OpenOptions, OpenResource, ResolveExternalUriOptions, ResolvedExternalUri,
};
use je_platform::resources::{ext_uri, uri_file};
use je_platform::workspace::{
    CurrentWorkspaceQuery, IWorkspaceContextService, WorkbenchState, Workspace, WorkspaceFolder, WorkspaceFoldersChangeEvent, WorkspaceFoldersWillChangeEvent,
};
use je_workbench::api::common::ext_host_values::ApiValue;
use je_workbench::contrib::files::browser::file_commands::REVEAL_IN_EXPLORER_COMMAND_ID;
use je_workbench::contrib::opener::WorkbenchOpenerContribution;
use je_workbench::services::environment::environment_service::IWorkbenchEnvironmentService;
use je_workbench::test::browser::workbench_test_services::{EnvironmentOverrides, TestEnvironmentService};
use je_workbench::test::common::workbench_test_services::{FileStatOptions, create_file_stat};

/// O membro que o objeto parcial do original não define: chamá-lo é um `TypeError`.
fn not_a_function(member: &str) -> ! {
    errors::throw(Error::type_error(format!("{member} is not a function")))
}

/// Um elemento de `calls: object[]`.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Call {
    /// `{ stat: uri.toString() }`.
    Stat(String),
    /// `{ command: id, uri: uri.toString() }`.
    Command { command: String, uri: String },
    /// `{ external: uri.toString(), options: { openExternal, fromUserGesture } }`.
    External { external: String, open_external: Option<bool>, from_user_gesture: Option<bool> },
}

type Calls = Rc<RefCell<Vec<Call>>>;
type Registered = Rc<RefCell<Option<Rc<dyn IOpener>>>>;

/// `uri.toString()`.
fn uri_text(uri: &URI) -> String {
    uri.to_string(false).to_string_lossy()
}

/// `upcastPartial<IOpenerService>({ registerOpener, open })`.
struct TestOpenerService {
    calls: Calls,
    registered: Registered,
    external_result: bool,
}

impl IOpenerService for TestOpenerService {
    fn register_opener(&self, opener: Rc<dyn IOpener>) -> DisposableRef {
        *self.registered.borrow_mut() = Some(opener);
        let registered = Rc::clone(&self.registered);
        lifecycle::to_disposable(move || {
            *registered.borrow_mut() = None;
        })
    }

    fn register_validator(&self, _validator: Rc<dyn IValidator>) -> DisposableRef {
        not_a_function("registerValidator")
    }

    fn register_external_uri_resolver(&self, _resolver: Rc<dyn IExternalUriResolver>) -> DisposableRef {
        not_a_function("registerExternalUriResolver")
    }

    fn set_default_external_opener(&self, _opener: Rc<dyn IExternalOpener>) {
        not_a_function("setDefaultExternalOpener")
    }

    fn register_external_opener(&self, _opener: Rc<dyn IExternalOpener>) -> DisposableRef {
        not_a_function("registerExternalOpener")
    }

    fn open(&self, resource: &OpenResource, options: Option<&OpenOptions>) -> Promise<bool> {
        let OpenResource::Uri(uri) = resource else { not_a_function("open (string)") };
        let options = options.cloned();
        self.calls.borrow_mut().push(Call::External {
            external: uri_text(uri),
            open_external: options.as_ref().and_then(|options| options.open_external),
            from_user_gesture: options.as_ref().and_then(|options| options.from_user_gesture),
        });
        let registered = self.registered.borrow().clone().expect("o abridor está registrado");
        let resource = resource.clone();
        let external_result = self.external_result;
        async_fn(async move {
            assert_eq!(registered.open(&resource, options.as_ref()).await?, false);
            Ok(external_result)
        })
    }

    fn resolve_external_uri(&self, _resource: &URI, _options: Option<&ResolveExternalUriOptions>) -> Promise<ResolvedExternalUri> {
        not_a_function("resolveExternalUri")
    }
}

/// `upcastPartial<ICommandService>({ executeCommand })`.
struct TestCommandService {
    calls: Calls,
}

impl ICommandService for TestCommandService {
    fn on_will_execute_command(&self) -> Event<CommandEvent> {
        not_a_function("onWillExecuteCommand")
    }

    fn on_did_execute_command(&self) -> Event<CommandEvent> {
        not_a_function("onDidExecuteCommand")
    }

    fn execute_command(&self, command_id: &str, args: Vec<Dynamic>) -> Promise<Dynamic> {
        let uri = URI::from_dynamic(&args[0]).expect("o argumento é uma URI");
        self.calls.borrow_mut().push(Call::Command { command: command_id.to_string(), uri: uri_text(&uri) });
        Promise::resolved(Dynamic::Undefined)
    }
}

/// `upcastPartial<IWorkbenchEnvironmentService>({ isSessionsWindow })`: o `TestEnvironmentService` com o valor trocado.
fn environment_service(sessions: bool) -> Rc<dyn IWorkbenchEnvironmentService> {
    TestEnvironmentService::with_overrides(EnvironmentOverrides { is_sessions_window: sessions, ..EnvironmentOverrides::default() })
}

/// `upcastPartial<IWorkspaceContextService>({ isInsideWorkspace })`.
struct TestWorkspaceContextService {
    roots: Vec<URI>,
}

impl IWorkspaceContextService for TestWorkspaceContextService {
    fn on_did_change_workbench_state(&self) -> Event<WorkbenchState> {
        not_a_function("onDidChangeWorkbenchState")
    }

    fn on_did_change_workspace_name(&self) -> Event<()> {
        not_a_function("onDidChangeWorkspaceName")
    }

    fn on_will_change_workspace_folders(&self) -> Event<WorkspaceFoldersWillChangeEvent> {
        not_a_function("onWillChangeWorkspaceFolders")
    }

    fn on_did_change_workspace_folders(&self) -> Event<WorkspaceFoldersChangeEvent> {
        not_a_function("onDidChangeWorkspaceFolders")
    }

    fn get_complete_workspace(&self) -> Promise<Rc<Workspace>> {
        not_a_function("getCompleteWorkspace")
    }

    fn get_workspace(&self) -> Rc<Workspace> {
        not_a_function("getWorkspace")
    }

    fn get_workbench_state(&self) -> WorkbenchState {
        not_a_function("getWorkbenchState")
    }

    fn get_workspace_folder(&self, _resource: &URI) -> Option<WorkspaceFolder> {
        not_a_function("getWorkspaceFolder")
    }

    fn is_current_workspace(&self, _workspace_id_or_folder: &CurrentWorkspaceQuery) -> bool {
        not_a_function("isCurrentWorkspace")
    }

    fn is_inside_workspace(&self, resource: &URI) -> bool {
        self.roots.iter().any(|folder| ext_uri().is_equal_or_parent(resource, folder, false, false))
    }

    fn has_workspace_data(&self) -> bool {
        not_a_function("hasWorkspaceData")
    }
}

/// `upcastPartial<IFileService>({ hasProvider, stat })`.
struct TestFileService {
    calls: Calls,
    is_directory: bool,
    stat_error: Option<Error>,
}

impl IFileService for TestFileService {
    fn on_did_files_change(&self) -> Event<Rc<FileChangesEvent>> {
        not_a_function("onDidFilesChange")
    }

    fn get_provider(&self, _scheme: &str) -> Option<FileSystemProviderRef> {
        not_a_function("getProvider")
    }

    fn resolve_with_metadata(&self, _resource: &URI, _options: Option<ResolveFileOptions>) -> Promise<FileStatWithMetadata> {
        not_a_function("resolve")
    }

    fn stat(&self, resource: &URI) -> Promise<FileStatWithMetadata> {
        self.calls.borrow_mut().push(Call::Stat(uri_text(resource)));
        if let Some(error) = &self.stat_error {
            return Promise::rejected(error.clone());
        }
        Promise::resolved(create_file_stat(
            resource,
            &FileStatOptions { is_file: Some(!self.is_directory), is_directory: Some(self.is_directory), ..FileStatOptions::default() },
        ))
    }

    fn exists(&self, _resource: &URI) -> Promise<bool> {
        not_a_function("exists")
    }

    fn on_did_change_file_system_provider_registrations(&self) -> Event<FileSystemProviderRegistrationEvent> {
        not_a_function("onDidChangeFileSystemProviderRegistrations")
    }

    fn on_did_change_file_system_provider_capabilities(&self) -> Event<FileSystemProviderCapabilitiesChangeEvent> {
        not_a_function("onDidChangeFileSystemProviderCapabilities")
    }

    fn has_provider(&self, resource: &URI) -> bool {
        let scheme = resource.scheme().to_string_lossy();
        scheme == schemas::FILE || scheme == schemas::VSCODE_REMOTE
    }

    fn has_capability(&self, _resource: &URI, _capability: FileSystemProviderCapabilities) -> bool {
        not_a_function("hasCapability")
    }

    fn read_file(&self, _resource: &URI, _options: Option<ReadFileOptions>, _token: Option<je_base::cancellation::CancellationToken>) -> Promise<FileContent> {
        not_a_function("readFile")
    }

    fn resolve(&self, _resource: &URI, _options: Option<ResolveFileOptions>) -> Promise<FileStat> {
        not_a_function("resolve")
    }

    fn create_folder(&self, _resource: &URI) -> Promise<FileStatWithMetadata> {
        not_a_function("createFolder")
    }

    fn watch(&self, _resource: &URI, _options: Option<WatchOptions>) -> DisposableRef {
        not_a_function("watch")
    }

    fn read_file_stream(&self, _resource: &URI, _options: Option<ReadFileOptions>, _token: Option<je_base::cancellation::CancellationToken>) -> Promise<FileStreamContent> {
        not_a_function("readFileStream")
    }

    fn write_file(&self, _resource: &URI, _content: FileContentSource, _options: Option<WriteFileOptions>) -> Promise<FileStatWithMetadata> {
        not_a_function("writeFile")
    }

    fn del(&self, _resource: &URI, _options: Option<PartialFileDeleteOptions>) -> Promise<()> {
        not_a_function("del")
    }

    fn on_did_run_operation(&self) -> Event<FileOperationEvent> {
        not_a_function("onDidRunOperation")
    }

    fn create_file(&self, _resource: &URI, _content: Option<FileContentSource>, _options: Option<CreateFileOptions>) -> Promise<FileStatWithMetadata> {
        not_a_function("createFile")
    }

    fn on_will_activate_file_system_provider(&self) -> Event<FileSystemProviderActivationEvent> {
        not_a_function("onWillActivateFileSystemProvider")
    }

    fn register_provider(&self, _scheme: &str, _provider: FileSystemProviderRef) -> Result<DisposableRef, Error> {
        not_a_function("registerProvider")
    }
}

/// `options` de `createOpener`; `undefined` é o valor padrão do original.
struct OpenerOptions {
    sessions: bool,
    roots: Vec<URI>,
    is_directory: bool,
    stat_error: Option<Error>,
    external_result: bool,
}

impl OpenerOptions {
    fn new() -> Self {
        Self { sessions: true, roots: vec![root()], is_directory: true, stat_error: None, external_result: true }
    }

    fn sessions(mut self, sessions: bool) -> Self {
        self.sessions = sessions;
        self
    }

    fn roots(mut self, roots: Vec<URI>) -> Self {
        self.roots = roots;
        self
    }

    fn is_directory(mut self, is_directory: bool) -> Self {
        self.is_directory = is_directory;
        self
    }

    fn stat_error(mut self, stat_error: Error) -> Self {
        self.stat_error = Some(stat_error);
        self
    }

    fn external_result(mut self, external_result: bool) -> Self {
        self.external_result = external_result;
        self
    }
}

/// `root`.
fn root() -> URI {
    uri_file("/workspace/project")
}

/// O retorno de `createOpener`: `{ opener, calls, registered }`.
struct Fixture {
    opener: Rc<WorkbenchOpenerContribution>,
    calls: Calls,
    registered: Registered,
}

/// `createOpener(options)`.
fn create_opener(store: &LeakGuard, options: OpenerOptions) -> Fixture {
    let calls: Calls = Rc::new(RefCell::new(Vec::new()));
    let registered: Registered = Rc::new(RefCell::new(None));
    let opener_service: Rc<dyn IOpenerService> =
        Rc::new(TestOpenerService { calls: Rc::clone(&calls), registered: Rc::clone(&registered), external_result: options.external_result });
    let command_service: Rc<dyn ICommandService> = Rc::new(TestCommandService { calls: Rc::clone(&calls) });
    let workspace_context_service: Rc<dyn IWorkspaceContextService> = Rc::new(TestWorkspaceContextService { roots: options.roots });
    let file_service: Rc<dyn IFileService> =
        Rc::new(TestFileService { calls: Rc::clone(&calls), is_directory: options.is_directory, stat_error: options.stat_error });
    let opener = store.add_rc(WorkbenchOpenerContribution::new(
        opener_service,
        command_service,
        file_service,
        workspace_context_service,
        environment_service(options.sessions),
    ));
    Fixture { opener, calls, registered }
}

/// `uri.with({ path })`.
fn with_path(uri: &URI, path: &str) -> URI {
    uri.with(&UriChange { path: path.into(), ..UriChange::default() })
}

/// `URI.from({ scheme: Schemas.vscodeRemote, authority: 'ssh-remote+host', path: '/project/src' })`.
fn remote_uri() -> URI {
    URI::from(
        &UriComponents {
            scheme: Some(JsString::from(schemas::VSCODE_REMOTE)),
            authority: Some(JsString::from("ssh-remote+host")),
            path: Some(JsString::from("/project/src")),
            ..UriComponents::default()
        },
        false,
    )
}

/// `opener.open(link, options)`, esperado.
async fn open(fixture: &Fixture, link: impl Into<OpenResource>, options: Option<OpenOptions>) -> Result<bool, Error> {
    fixture.opener.open(&link.into(), options.as_ref()).await
}

/// `{ handled: await opener.open(uri), calls }`.
async fn handled_and_calls(fixture: &Fixture, uri: &URI, options: Option<OpenOptions>) -> (bool, Vec<Call>) {
    let handled = open(fixture, uri.clone(), options).await.expect("open");
    (handled, fixture.calls.borrow().clone())
}

/// O caso de `reveals workspace folders through the existing command (sessions: ..., uri)`.
fn reveals_workspace_folders_through_the_existing_command(sessions: bool, uri: URI) {
    run_with_faked_timers(async move {
        let store = ensure_no_disposables_are_leaked_in_test_suite();
        let fixture = create_opener(&store, OpenerOptions::new().sessions(sessions).roots(vec![root(), with_path(&uri, "/project")]));
        let actual = handled_and_calls(&fixture, &uri, None).await;
        assert_eq!(
            actual,
            (true, vec![Call::Stat(uri_text(&uri)), Call::Command { command: REVEAL_IN_EXPLORER_COMMAND_ID.to_string(), uri: uri_text(&uri) }])
        );
    });
}

/// O caso de `preserves file opening (sessions: ...)`.
fn preserves_file_opening(sessions: bool) {
    run_with_faked_timers(async move {
        let store = ensure_no_disposables_are_leaked_in_test_suite();
        let fixture = create_opener(&store, OpenerOptions::new().sessions(sessions).is_directory(false));
        let actual = handled_and_calls(&fixture, &root(), None).await;
        assert_eq!(actual, (false, vec![Call::Stat(uri_text(&root()))]));
    });
}

#[test]
fn reveals_workspace_folders_through_the_existing_command_sessions_false_root() {
    reveals_workspace_folders_through_the_existing_command(false, root());
}

#[test]
fn reveals_workspace_folders_through_the_existing_command_sessions_false_nested() {
    reveals_workspace_folders_through_the_existing_command(false, uri_file("/workspace/project/nested/"));
}

#[test]
fn reveals_workspace_folders_through_the_existing_command_sessions_false_remote() {
    reveals_workspace_folders_through_the_existing_command(false, remote_uri());
}

#[test]
fn preserves_file_opening_sessions_false() {
    preserves_file_opening(false);
}

#[test]
fn reveals_workspace_folders_through_the_existing_command_sessions_true_root() {
    reveals_workspace_folders_through_the_existing_command(true, root());
}

#[test]
fn reveals_workspace_folders_through_the_existing_command_sessions_true_nested() {
    reveals_workspace_folders_through_the_existing_command(true, uri_file("/workspace/project/nested/"));
}

#[test]
fn reveals_workspace_folders_through_the_existing_command_sessions_true_remote() {
    reveals_workspace_folders_through_the_existing_command(true, remote_uri());
}

#[test]
fn preserves_file_opening_sessions_true() {
    preserves_file_opening(true);
}

#[test]
fn normalizes_paths_and_strips_line_selections_before_checking_folders() {
    run_with_faked_timers(async move {
        let store = ensure_no_disposables_are_leaked_in_test_suite();
        let fixture = create_opener(&store, OpenerOptions::new());
        let link = uri_file("/workspace/project/src/../nested").with(&UriChange { fragment: "L12".into(), ..UriChange::default() });
        open(&fixture, link.to_string(false), None).await.expect("open");
        let uri = uri_text(&uri_file("/workspace/project/nested"));
        assert_eq!(*fixture.calls.borrow(), vec![Call::Stat(uri.clone()), Call::Command { command: REVEAL_IN_EXPLORER_COMMAND_ID.to_string(), uri }]);
    });
}

/// O caso de `opens external local folders only in Sessions (uri, roots: n)`.
fn opens_external_local_folders_only_in_sessions(uri: URI, roots: Vec<URI>) {
    run_with_faked_timers(async move {
        let store = ensure_no_disposables_are_leaked_in_test_suite();
        let mut results = Vec::new();
        for sessions in [false, true] {
            let fixture = create_opener(&store, OpenerOptions::new().sessions(sessions).roots(roots.clone()));
            let options = OpenOptions { from_user_gesture: Some(true), ..OpenOptions::default() };
            results.push(handled_and_calls(&fixture, &uri, Some(options)).await);
        }
        assert_eq!(
            results,
            vec![
                (false, vec![]),
                (
                    true,
                    vec![
                        Call::Stat(uri_text(&uri)),
                        Call::External { external: uri_text(&uri), open_external: Some(true), from_user_gesture: Some(true) },
                    ]
                ),
            ]
        );
    });
}

#[test]
fn opens_external_local_folders_only_in_sessions_folder_with_spaces() {
    opens_external_local_folders_only_in_sessions(uri_file("/elsewhere/folder with spaces"), vec![root()]);
}

#[test]
fn opens_external_local_folders_only_in_sessions_sibling_with_same_prefix() {
    opens_external_local_folders_only_in_sessions(uri_file("/workspace/project-other"), vec![root()]);
}

#[test]
fn opens_external_local_folders_only_in_sessions_without_roots() {
    opens_external_local_folders_only_in_sessions(root(), vec![]);
}

/// O caso de `does not intercept unrelated links: uri`.
fn does_not_intercept_unrelated_links(text: &str) {
    let uri = URI::parse(text, false);
    run_with_faked_timers(async move {
        let store = ensure_no_disposables_are_leaked_in_test_suite();
        let fixture = create_opener(&store, OpenerOptions::new());
        let actual = handled_and_calls(&fixture, &uri, None).await;
        assert_eq!(actual, (false, vec![]));
    });
}

#[test]
fn does_not_intercept_unrelated_links_https() {
    does_not_intercept_unrelated_links("https://example.com");
}

#[test]
fn does_not_intercept_unrelated_links_command() {
    does_not_intercept_unrelated_links("command:example");
}

#[test]
fn does_not_intercept_unrelated_links_untitled() {
    does_not_intercept_unrelated_links("untitled:example");
}

#[test]
fn does_not_intercept_unrelated_links_vscode_remote() {
    does_not_intercept_unrelated_links("vscode-remote://host/elsewhere");
}

#[test]
fn does_not_intercept_explicitly_external_links() {
    run_with_faked_timers(async move {
        let store = ensure_no_disposables_are_leaked_in_test_suite();
        let fixture = create_opener(&store, OpenerOptions::new());
        let options = OpenOptions { open_external: Some(true), ..OpenOptions::default() };
        let actual = handled_and_calls(&fixture, &root(), Some(options)).await;
        assert_eq!(actual, (false, vec![]));
    });
}

#[test]
fn leaves_missing_paths_to_the_default_opener() {
    run_with_faked_timers(async move {
        let store = ensure_no_disposables_are_leaked_in_test_suite();
        let fixture = create_opener(&store, OpenerOptions::new().stat_error(FileOperationError::new("Missing", FileOperationResult::FileNotFound).into_error()));
        assert_eq!(open(&fixture, root(), None).await.expect("open"), false);
    });
}

#[test]
fn propagates_unexpected_stat_errors() {
    run_with_faked_timers(async move {
        let store = ensure_no_disposables_are_leaked_in_test_suite();
        let error = FileOperationError::new("Permission denied", FileOperationResult::FilePermissionDenied).into_error();
        let fixture = create_opener(&store, OpenerOptions::new().stat_error(error));
        let rejected = open(&fixture, root(), None).await.expect_err("o abridor propaga o erro do stat");
        assert_eq!(rejected.message, "Permission denied");
    });
}

#[test]
fn reports_external_opening_failures() {
    run_with_faked_timers(async move {
        let store = ensure_no_disposables_are_leaked_in_test_suite();
        let fixture = create_opener(&store, OpenerOptions::new().roots(vec![]).external_result(false));
        let rejected = open(&fixture, root(), None).await.expect_err("a abertura externa falhou");
        assert!(rejected.message.contains("Unable to open the folder"), "{}", rejected.message);
    });
}

#[test]
fn unregisters_on_disposal() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let fixture = create_opener(&store, OpenerOptions::new());
    let registered = fixture.registered.borrow().clone().expect("o abridor está registrado");
    assert!(std::ptr::addr_eq(Rc::as_ptr(&registered), Rc::as_ptr(&fixture.opener)));
    drop(registered);
    je_base::lifecycle::IDisposable::dispose(&*fixture.opener);
    assert!(fixture.registered.borrow().is_none());
}
