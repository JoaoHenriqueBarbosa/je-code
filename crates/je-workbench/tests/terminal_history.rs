//! Porte de `src/vs/workbench/contrib/terminalContrib/history/test/common/history.test.ts` (suítes `Terminal history`,
//! `TerminalPersistedHistory`, `fetchBashHistory`, `fetchZshHistory`, `fetchPwshHistory`, `fetchFishHistory` e
//! `sanitizeFishHistoryCmd`), caso a caso e na mesma ordem.
//!
//! Divergências:
//! - **`TestInstantiationService`.** `new TestInstantiationService()` com `stub(...)`/`set(...)` é um
//!   `InstantiationService` sobre um `ServiceCollection` com os serviços; `createInstance` é o `create_instance` real e
//!   `invokeFunction(fetchX)` é `invoke_function(fetch_x)`. `TestStorageService` é o `InMemoryStorageService`.
//! - **`ensureNoDisposablesAreLeakedInTestSuite()`.** É o guarda criado no começo de cada caso; o `teardown`
//!   (`instantiationService.dispose()`, a restauração de `env`) é o `Drop` de cada fixture, que roda antes do guarda.
//!   Os casos `async` rodam em `run_with_faked_timers`, com o guarda e o `setup` dentro do corpo.
//! - **`configurationService.onDidChangeConfigurationEmitter.fire({ affectsConfiguration: () => true })`.** É o evento
//!   real da chave `terminal.integrated.shellIntegration.history` (`fire_configuration_change`), cuja
//!   `affectsConfiguration` dessa chave é verdadeira.
//! - **`getConfig(limit)`.** É um objeto [`Dynamic`] com os mesmos níveis do original.
//! - **`process.env`.** `env['HOME'] = ...` e `delete env['HOME']` são `set_env_var`, o ambiente por thread que o
//!   histórico lê, e o `originalEnvValues` do `teardown` é o `Drop` do [`EnvGuard`].
//! - **`mock<IFileService>`.** `{ async readFile(resource) { ... } } as Pick<IFileService, 'readFile'>` é o
//!   [`HistoryFileService`], que implementa o `IFileService` inteiro e falha nos métodos que o original não tem. O
//!   `readFile` do original devolve só `{ value }`; o `IFileContent` do porte também tem o `stat`, que vem de
//!   `create_file_stat`. As asserções do `readFile` rodam dentro dele e um pânico reprova o caso.
//! - **`mock<IRemoteAgentService>`.** `{ getEnvironment, getConnection }` é o [`HistoryRemoteAgentService`]. O
//!   ambiente remoto `{ os }` do original não tem `userHome`, então o histórico lê `env['HOME']`; o
//!   `RemoteAgentEnvironment` do porte tem `user_home` obrigatório, e os casos remotos o recebem como `/home/user`, que
//!   é o mesmo valor que todos eles dão a `env['HOME']`.
//! - **Laço do zsh.** O `for (const { type, content } of fileContentType) suite(type, ...)` é um laço dentro de cada
//!   `#[test]`, com um `setup` e um `teardown` por tipo (`simple` e `extended`).
//! - **Casos que só rodam fora do Windows** (`if (!isWindows)`) e o ramo `if (isWindows)` do caso `current OS` do pwsh
//!   ficam com o mesmo `if` em tempo de execução.
//! - **`assert.strictEqual(x, undefined)`** é `assert_eq!(x, None)`, e `assert.ok(x.includes('\n'))` é `contains('\n')`.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
#[path = "configuration_event_support/mod.rs"]
mod configuration_event_support;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use common::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite, run_with_faked_timers};
use configuration_event_support::fire_configuration_change;
use je_base::JsString;
use je_base::buffer::VSBuffer;
use je_base::cancellation::CancellationToken;
use je_base::dynamic::Dynamic;
use je_base::errors::{self, Error};
use je_base::event::Event;
use je_base::lifecycle::IDisposable;
use je_base::network::schemas;
use je_base::parts::ipc::IChannel;
use je_base::path::{PathModule, ProcessEnvironment};
use je_base::platform::{OperatingSystem, Platform};
use je_base::promise::Promise;
use je_base::uri::{URI, UriComponents};
use je_instantiation::{InstantiationService, ServiceCollection, ServicesAccessor};
use je_platform::configuration::service::{I_CONFIGURATION_SERVICE, IConfigurationService};
use je_platform::configuration::test_configuration_service::TestConfigurationService;
use je_platform::files::files::{
    FileChangesEvent, FileContent, FileStatWithMetadata, FileSystemProviderCapabilities, FileSystemProviderCapabilitiesChangeEvent, FileSystemProviderRef,
    FileSystemProviderRegistrationEvent, ReadFileOptions, ResolveFileOptions,
};
use je_platform::files::i_file_service::{I_FILE_SERVICE, IFileService};
use je_platform::remote::remote_agent_environment::{RemoteAgentEnvironment, RemoteAgentProfiles};
use je_platform::resources::host_platform;
use je_platform::storage::{I_STORAGE_SERVICE, StorageService};
use je_base::process::{env_var, set_env_var};
use je_workbench::contrib::terminal_contrib::history::history::{
    ITerminalPersistedHistory, ShellFileHistoryEntry, TerminalPersistedHistory, fetch_bash_history, fetch_fish_history, fetch_pwsh_history,
    fetch_zsh_history, sanitize_fish_history_cmd,
};
use je_workbench::services::remote::remote_agent_service::{I_REMOTE_AGENT_SERVICE, IRemoteAgentConnection, IRemoteAgentService};
use je_workbench::test::common::workbench_test_services::{FileStatOptions, TestStorageService, create_file_stat};

/// `isWindows`.
fn is_windows() -> bool {
    Platform::detect().is_windows
}

/// `join(...paths)` do `path` nativo.
fn join(paths: &[&str]) -> String {
    PathModule::native(&ProcessEnvironment).join(paths)
}

/// `{ terminal: { integrated: { shellIntegration: { history: limit } } } }` sem o nível `terminal`, o valor de
/// `getConfig(limit).terminal`.
fn get_config_terminal(limit: f64) -> Dynamic {
    Dynamic::object([("integrated", Dynamic::object([("shellIntegration", Dynamic::object([("history", Dynamic::Number(limit))]))]))])
}

/// `getConfig(limit)`.
fn get_config(limit: f64) -> Dynamic {
    Dynamic::object([("terminal", get_config_terminal(limit))])
}

/// `expectedCommands`.
fn expected_commands() -> Vec<String> {
    vec![
        "single line command".to_owned(),
        "git commit -m \"A wrapped line in pwsh history\n\nSome commit description\n\nFixes #xyz\"".to_owned(),
        "git status".to_owned(),
        "two \"\nline\"".to_owned(),
    ]
}

/// `lines.join('\n')`.
fn join_lines(lines: &[&str]) -> String {
    lines.join("\n")
}

/// `originalEnvValues` e a restauração do `teardown`: o valor de cada variável volta (ou é apagada) ao descartar.
struct EnvGuard {
    saved: Vec<(&'static str, Option<String>)>,
}

impl EnvGuard {
    fn save(names: &[&'static str]) -> Self {
        Self { saved: names.iter().map(|name| (*name, env_var(name))).collect() }
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        for (name, value) in &self.saved {
            set_env_var(name, value.as_deref());
        }
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// suite('TerminalPersistedHistory')
// ---------------------------------------------------------------------------------------------------------------------

struct HistoryFixture {
    history: Rc<TerminalPersistedHistory<f64>>,
    instantiation_service: Rc<InstantiationService>,
    configuration_service: Rc<TestConfigurationService>,
}

/// O `setup` da suíte `TerminalPersistedHistory`.
fn history_fixture(store: &LeakGuard) -> HistoryFixture {
    let configuration_service = TestConfigurationService::new(Some(get_config(5.0)));
    // O emissor do `TestConfigurationService` é descartado pelo store do caso (o original não o descarta).
    store.add_rc(configuration_service.on_did_change_configuration_emitter.clone());
    let configuration: Rc<dyn IConfigurationService> = configuration_service.clone();
    let storage_service = store.add_rc(TestStorageService::new());
    let storage: Rc<dyn StorageService> = Rc::new(storage_service.clone());
    let instantiation_service =
        store.add_rc(InstantiationService::new(ServiceCollection::new().with(I_CONFIGURATION_SERVICE, configuration).with(I_STORAGE_SERVICE, storage)));

    let history = store.add_rc(instantiation_service.create_instance::<TerminalPersistedHistory<f64>>("test".to_owned()));
    HistoryFixture { history, instantiation_service, configuration_service }
}

/// `Array.from(history.entries)`.
fn entries(history: &Rc<TerminalPersistedHistory<f64>>) -> Vec<(String, f64)> {
    history.entries()
}

/// `[key, value]`.
fn entry(key: &str, value: f64) -> (String, f64) {
    (key.to_owned(), value)
}

#[test]
fn terminal_persisted_history_should_support_adding_items_to_the_cache_and_respect_lru() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let f = history_fixture(&store);
    let history = &f.history;

    history.add("foo", 1.0);
    assert_eq!(entries(history), vec![entry("foo", 1.0)]);
    history.add("bar", 2.0);
    assert_eq!(entries(history), vec![entry("foo", 1.0), entry("bar", 2.0)]);
    history.add("foo", 1.0);
    assert_eq!(entries(history), vec![entry("bar", 2.0), entry("foo", 1.0)]);
}

#[test]
fn terminal_persisted_history_should_support_removing_specific_items() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let f = history_fixture(&store);
    let history = &f.history;

    history.add("1", 1.0);
    history.add("2", 2.0);
    history.add("3", 3.0);
    history.add("4", 4.0);
    history.add("5", 5.0);
    assert_eq!(entries(history).len(), 5);
    history.add("6", 6.0);
    assert_eq!(entries(history).len(), 5);
}

#[test]
fn terminal_persisted_history_should_limit_the_number_of_entries_based_on_config() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let f = history_fixture(&store);
    let history = &f.history;

    history.add("1", 1.0);
    history.add("2", 2.0);
    history.add("3", 3.0);
    history.add("4", 4.0);
    history.add("5", 5.0);
    assert_eq!(entries(history).len(), 5);
    history.add("6", 6.0);
    assert_eq!(entries(history).len(), 5);
    let _ = f.configuration_service.set_user_configuration("terminal", get_config_terminal(2.0), None);
    fire_configuration_change(&f.configuration_service, &["terminal.integrated.shellIntegration.history"]);
    assert_eq!(entries(history).len(), 2);
    history.add("7", 7.0);
    assert_eq!(entries(history).len(), 2);
    let _ = f.configuration_service.set_user_configuration("terminal", get_config_terminal(3.0), None);
    fire_configuration_change(&f.configuration_service, &["terminal.integrated.shellIntegration.history"]);
    assert_eq!(entries(history).len(), 2);
    history.add("8", 8.0);
    assert_eq!(entries(history).len(), 3);
    history.add("9", 9.0);
    assert_eq!(entries(history).len(), 3);
}

#[test]
fn terminal_persisted_history_should_reload_from_storage_service_after_recreation() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let f = history_fixture(&store);
    let history = &f.history;

    history.add("1", 1.0);
    history.add("2", 2.0);
    history.add("3", 3.0);
    assert_eq!(entries(history).len(), 3);
    let history2 = store.add_rc(f.instantiation_service.create_instance::<TerminalPersistedHistory<f64>>("test".to_owned()));
    assert_eq!(entries(&history2).len(), 3);
}

// ---------------------------------------------------------------------------------------------------------------------
// Dublês das suítes `fetch*History`
// ---------------------------------------------------------------------------------------------------------------------

/// As variáveis de suíte do original: `fileScheme`, `filePath`, `remoteConnection` e `remoteEnvironment` (`null` no
/// início).
struct FileState {
    file_scheme: RefCell<String>,
    file_path: RefCell<String>,
    /// `remoteConnection?.remoteAuthority`.
    remote_connection: RefCell<Option<String>>,
    /// `remoteEnvironment?.os`.
    remote_environment: Cell<Option<OperatingSystem>>,
}

impl FileState {
    fn new() -> Rc<Self> {
        Rc::new(Self {
            file_scheme: RefCell::new(String::new()),
            file_path: RefCell::new(String::new()),
            remote_connection: RefCell::new(None),
            remote_environment: Cell::new(None),
        })
    }

    /// `remoteConnection = { remoteAuthority: 'some-remote' }; fileScheme = Schemas.vscodeRemote; filePath = path`.
    fn use_remote_file(&self, path: &str) {
        *self.remote_connection.borrow_mut() = Some("some-remote".to_owned());
        *self.file_scheme.borrow_mut() = schemas::VSCODE_REMOTE.to_owned();
        self.set_file_path(path);
    }

    /// `filePath = path`.
    fn set_file_path(&self, path: &str) {
        *self.file_path.borrow_mut() = path.to_owned();
    }
}

/// `strictEqual(resource.scheme, expected.scheme); strictEqual(resource.path, expected.path)` com
/// `expected = URI.from({ scheme: fileScheme, path: filePath })`.
fn check_scheme_and_path(resource: &URI, state: &FileState) {
    let expected = URI::from(
        &UriComponents {
            scheme: Some(JsString::from(state.file_scheme.borrow().as_str())),
            path: Some(JsString::from(state.file_path.borrow().as_str())),
            ..UriComponents::default()
        },
        false,
    );
    assert_eq!(resource.scheme().to_string_lossy(), expected.scheme().to_string_lossy());
    assert_eq!(resource.path().to_string_lossy(), expected.path().to_string_lossy());
}

/// `resource.toString().replaceAll('%5C', '/')`.
fn sanitized_text(resource: &URI) -> String {
    resource.to_string(false).to_string_lossy().replace("%5C", "/")
}

/// `strictEqual(resource.toString().replaceAll('%5C', '/'), expected.toString().replaceAll('%5C', '/'))` com
/// `expected = URI.from({ scheme: fileScheme, authority: remoteConnection?.remoteAuthority, path: URI.file(filePath).path })`.
fn check_full_uri(resource: &URI, state: &FileState) {
    let expected = URI::from(
        &UriComponents {
            scheme: Some(JsString::from(state.file_scheme.borrow().as_str())),
            authority: state.remote_connection.borrow().as_deref().map(JsString::from),
            path: Some(URI::file(JsString::from(state.file_path.borrow().as_str()), host_platform()).path().clone()),
            ..UriComponents::default()
        },
        false,
    );
    // Sanitize the encoded `/` chars as they don't impact behavior
    assert_eq!(sanitized_text(resource), sanitized_text(&expected));
}

/// `{ async readFile(resource) { ... } } as Pick<IFileService, 'readFile'>`.
struct HistoryFileService {
    content: String,
    state: Rc<FileState>,
    check: fn(&URI, &FileState),
}

impl HistoryFileService {
    fn not_mocked(method: &str) -> ! {
        errors::throw(Error::type_error(format!("fileService.{method} is not a function")))
    }
}

impl IFileService for HistoryFileService {
    fn on_did_files_change(&self) -> Event<Rc<FileChangesEvent>> {
        Self::not_mocked("onDidFilesChange")
    }

    fn get_provider(&self, _scheme: &str) -> Option<FileSystemProviderRef> {
        Self::not_mocked("getProvider")
    }

    fn resolve_with_metadata(&self, _resource: &URI, _options: Option<ResolveFileOptions>) -> Promise<FileStatWithMetadata> {
        Self::not_mocked("resolve")
    }

    fn exists(&self, _resource: &URI) -> Promise<bool> {
        Self::not_mocked("exists")
    }

    fn on_did_change_file_system_provider_registrations(&self) -> Event<FileSystemProviderRegistrationEvent> {
        Self::not_mocked("onDidChangeFileSystemProviderRegistrations")
    }

    fn on_did_change_file_system_provider_capabilities(&self) -> Event<FileSystemProviderCapabilitiesChangeEvent> {
        Self::not_mocked("onDidChangeFileSystemProviderCapabilities")
    }

    fn has_provider(&self, _resource: &URI) -> bool {
        Self::not_mocked("hasProvider")
    }

    fn has_capability(&self, _resource: &URI, _capability: FileSystemProviderCapabilities) -> bool {
        Self::not_mocked("hasCapability")
    }

    fn read_file(&self, resource: &URI, _options: Option<ReadFileOptions>, _token: Option<CancellationToken>) -> Promise<FileContent> {
        (self.check)(resource, &self.state);
        Promise::resolved(FileContent { stat: create_file_stat(resource, &FileStatOptions::default()), value: VSBuffer::from_string(&self.content) })
    }
}

/// `{ remoteAuthority }`, o `Pick<IRemoteAgentConnection, 'remoteAuthority'>`.
struct RemoteConnection {
    remote_authority: String,
}

impl IRemoteAgentConnection for RemoteConnection {
    fn remote_authority(&self) -> String {
        self.remote_authority.clone()
    }

    fn get_channel(&self, _channel_name: &str) -> Rc<dyn IChannel> {
        errors::throw(Error::type_error("connection.getChannel is not a function"))
    }
}

/// `{ os }`, o `Pick<IRemoteAgentEnvironment, 'os'>` (ver as divergências sobre o `user_home`).
fn remote_environment(os: OperatingSystem) -> RemoteAgentEnvironment {
    let uri = URI::file("/home/user", host_platform());
    RemoteAgentEnvironment {
        pid: 0,
        connection_token: String::new(),
        app_root: uri.clone(),
        exec_path: String::new(),
        tmp_dir: uri.clone(),
        settings_path: uri.clone(),
        mcp_resource: uri.clone(),
        logs_path: uri.clone(),
        extension_host_logs_path: uri.clone(),
        global_storage_home: uri.clone(),
        workspace_storage_home: uri.clone(),
        local_history_home: uri.clone(),
        user_home: uri.clone(),
        os,
        arch: String::new(),
        marks: Vec::new(),
        use_host_proxy: false,
        profiles: RemoteAgentProfiles { all: Vec::new(), home: uri },
        is_unsupported_glibc: false,
        reconnection_grace_time: None,
    }
}

/// `{ async getEnvironment() { return remoteEnvironment; }, getConnection() { return remoteConnection; } } as
/// Pick<IRemoteAgentService, 'getConnection' | 'getEnvironment'>`.
struct HistoryRemoteAgentService {
    state: Rc<FileState>,
}

impl IRemoteAgentService for HistoryRemoteAgentService {
    fn get_environment(&self) -> Promise<Option<RemoteAgentEnvironment>> {
        Promise::resolved(self.state.remote_environment.get().map(remote_environment))
    }

    fn get_raw_environment(&self) -> Promise<Option<RemoteAgentEnvironment>> {
        errors::throw(Error::type_error("remoteAgentService.getRawEnvironment is not a function"))
    }

    fn get_connection(&self) -> Option<Rc<dyn IRemoteAgentConnection>> {
        let remote_authority = self.state.remote_connection.borrow().clone()?;
        let connection: Rc<dyn IRemoteAgentConnection> = Rc::new(RemoteConnection { remote_authority });
        Some(connection)
    }
}

/// O `setup` das suítes `fetch*History`: o serviço de instanciação com os dois dublês e o `teardown` que o descarta.
struct Fixture {
    instantiation_service: Rc<InstantiationService>,
    state: Rc<FileState>,
}

impl Fixture {
    fn new(content: &str, check: fn(&URI, &FileState)) -> Self {
        let state = FileState::new();
        let file_service: Rc<dyn IFileService> = Rc::new(HistoryFileService { content: content.to_owned(), state: state.clone(), check });
        let remote_agent_service: Rc<dyn IRemoteAgentService> = Rc::new(HistoryRemoteAgentService { state: state.clone() });
        let instantiation_service =
            InstantiationService::new(ServiceCollection::new().with(I_FILE_SERVICE, file_service).with(I_REMOTE_AGENT_SERVICE, remote_agent_service));
        Self { instantiation_service, state }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        self.instantiation_service.dispose();
    }
}

type FetchHistory = fn(&ServicesAccessor) -> Promise<Option<ShellFileHistoryEntry>>;

/// `await instantiationService.invokeFunction(fetchX)`.
async fn fetch(fixture: &Fixture, fetch_history: FetchHistory) -> Option<ShellFileHistoryEntry> {
    fixture.instantiation_service.invoke_function(fetch_history).await.unwrap()
}

/// `(await instantiationService.invokeFunction(fetchX))!.commands`.
async fn commands(fixture: &Fixture, fetch_history: FetchHistory) -> Vec<String> {
    fetch(fixture, fetch_history).await.unwrap().commands
}

// ---------------------------------------------------------------------------------------------------------------------
// suite('fetchBashHistory')
// ---------------------------------------------------------------------------------------------------------------------

fn bash_file_content() -> String {
    join_lines(&[
        "single line command",
        "git commit -m \"A wrapped line in pwsh history",
        "",
        "Some commit description",
        "",
        "Fixes #xyz\"",
        "git status",
        "two \"",
        "line\"",
    ])
}

/// O `setup` das suítes `local` e `remote` do bash.
fn bash_suite_setup() -> (EnvGuard, Fixture) {
    let env = EnvGuard::save(&["HOME"]);
    let fixture = Fixture::new(&bash_file_content(), check_scheme_and_path);
    set_env_var("HOME", Some("/home/user"));
    fixture.state.use_remote_file("/home/user/.bash_history");
    (env, fixture)
}

#[test]
fn fetch_bash_history_local_current_os() {
    run_with_faked_timers(async {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        if !is_windows() {
            let (_env, f) = bash_suite_setup();
            f.state.set_file_path("/home/user/.bash_history");
            assert_eq!(commands(&f, fetch_bash_history).await, expected_commands());
        }
    });
}

#[test]
fn fetch_bash_history_remote_windows() {
    run_with_faked_timers(async {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let (_env, f) = bash_suite_setup();
        f.state.remote_environment.set(Some(OperatingSystem::Windows));
        assert_eq!(fetch(&f, fetch_bash_history).await, None);
    });
}

#[test]
fn fetch_bash_history_remote_macos() {
    run_with_faked_timers(async {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let (_env, f) = bash_suite_setup();
        f.state.remote_environment.set(Some(OperatingSystem::Macintosh));
        assert_eq!(commands(&f, fetch_bash_history).await, expected_commands());
    });
}

#[test]
fn fetch_bash_history_remote_linux() {
    run_with_faked_timers(async {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let (_env, f) = bash_suite_setup();
        f.state.remote_environment.set(Some(OperatingSystem::Linux));
        assert_eq!(commands(&f, fetch_bash_history).await, expected_commands());
    });
}

// ---------------------------------------------------------------------------------------------------------------------
// suite('fetchZshHistory')
// ---------------------------------------------------------------------------------------------------------------------

/// `fileContentType`: o tipo (`simple` e `extended`) e o conteúdo do arquivo.
fn zsh_file_content_types() -> Vec<(&'static str, String)> {
    vec![
        (
            "simple",
            join_lines(&[
                "single line command",
                "git commit -m \"A wrapped line in pwsh history\\",
                "\\",
                "Some commit description\\",
                "\\",
                "Fixes #xyz\"",
                "git status",
                "two \"\\",
                "line\"",
            ]),
        ),
        (
            "extended",
            join_lines(&[
                ": 1655252330:0;single line command",
                ": 1655252330:0;git commit -m \"A wrapped line in pwsh history\\",
                "\\",
                "Some commit description\\",
                "\\",
                "Fixes #xyz\"",
                ": 1655252330:0;git status",
                ": 1655252330:0;two \"\\",
                "line\"",
            ]),
        ),
    ]
}

/// O `setup` das suítes `local` e `remote` do zsh para um tipo de conteúdo.
fn zsh_suite_setup(content: &str) -> (EnvGuard, Fixture) {
    let env = EnvGuard::save(&["HOME"]);
    let fixture = Fixture::new(content, check_scheme_and_path);
    set_env_var("HOME", Some("/home/user"));
    fixture.state.use_remote_file("/home/user/.bash_history");
    (env, fixture)
}

#[test]
fn fetch_zsh_history_local_current_os() {
    run_with_faked_timers(async {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        for (_type, content) in zsh_file_content_types() {
            if !is_windows() {
                let (_env, f) = zsh_suite_setup(&content);
                f.state.set_file_path("/home/user/.zsh_history");
                assert_eq!(commands(&f, fetch_zsh_history).await, expected_commands());
            }
        }
    });
}

#[test]
fn fetch_zsh_history_remote_windows() {
    run_with_faked_timers(async {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        for (_type, content) in zsh_file_content_types() {
            let (_env, f) = zsh_suite_setup(&content);
            f.state.set_file_path("/home/user/.zsh_history");
            f.state.remote_environment.set(Some(OperatingSystem::Windows));
            assert_eq!(fetch(&f, fetch_zsh_history).await, None);
        }
    });
}

#[test]
fn fetch_zsh_history_remote_macos() {
    run_with_faked_timers(async {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        for (_type, content) in zsh_file_content_types() {
            let (_env, f) = zsh_suite_setup(&content);
            f.state.set_file_path("/home/user/.zsh_history");
            f.state.remote_environment.set(Some(OperatingSystem::Macintosh));
            assert_eq!(commands(&f, fetch_zsh_history).await, expected_commands());
        }
    });
}

#[test]
fn fetch_zsh_history_remote_linux() {
    run_with_faked_timers(async {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        for (_type, content) in zsh_file_content_types() {
            let (_env, f) = zsh_suite_setup(&content);
            f.state.set_file_path("/home/user/.zsh_history");
            f.state.remote_environment.set(Some(OperatingSystem::Linux));
            assert_eq!(commands(&f, fetch_zsh_history).await, expected_commands());
        }
    });
}

// ---------------------------------------------------------------------------------------------------------------------
// suite('fetchPwshHistory')
// ---------------------------------------------------------------------------------------------------------------------

fn pwsh_file_content() -> String {
    join_lines(&[
        "single line command",
        "git commit -m \"A wrapped line in pwsh history`",
        "`",
        "Some commit description`",
        "`",
        "Fixes #xyz\"",
        "git status",
        "two \"`",
        "line\"",
    ])
}

/// O `setup` da suíte `local` do pwsh.
fn pwsh_local_setup() -> (EnvGuard, Fixture) {
    let env = EnvGuard::save(&["HOME", "APPDATA"]);
    let fixture = Fixture::new(&pwsh_file_content(), check_full_uri);
    set_env_var("HOME", Some("/home/user"));
    set_env_var("APPDATA", Some("C:\\AppData"));
    fixture.state.use_remote_file("/home/user/.zsh_history");
    (env, fixture)
}

/// O `setup` da suíte `remote` do pwsh.
fn pwsh_remote_setup() -> (EnvGuard, Fixture) {
    let env = EnvGuard::save(&["HOME", "APPDATA"]);
    let fixture = Fixture::new(&pwsh_file_content(), check_full_uri);
    *fixture.state.remote_connection.borrow_mut() = Some("some-remote".to_owned());
    *fixture.state.file_scheme.borrow_mut() = schemas::VSCODE_REMOTE.to_owned();
    (env, fixture)
}

#[test]
fn fetch_pwsh_history_local_current_os() {
    run_with_faked_timers(async {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let (_env, f) = pwsh_local_setup();
        if is_windows() {
            f.state.set_file_path(&join(&[&env_var("APPDATA").unwrap(), "Microsoft\\Windows\\PowerShell\\PSReadLine\\ConsoleHost_history.txt"]));
        } else {
            f.state.set_file_path(&join(&[&env_var("HOME").unwrap(), ".local/share/powershell/PSReadline/ConsoleHost_history.txt"]));
        }
        assert_eq!(commands(&f, fetch_pwsh_history).await, expected_commands());
    });
}

#[test]
fn fetch_pwsh_history_remote_windows() {
    run_with_faked_timers(async {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let (_env, f) = pwsh_remote_setup();
        f.state.remote_environment.set(Some(OperatingSystem::Windows));
        set_env_var("APPDATA", Some("C:\\AppData"));
        f.state.set_file_path("C:\\AppData\\Microsoft\\Windows\\PowerShell\\PSReadLine\\ConsoleHost_history.txt");
        assert_eq!(commands(&f, fetch_pwsh_history).await, expected_commands());
    });
}

#[test]
fn fetch_pwsh_history_remote_macos() {
    run_with_faked_timers(async {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let (_env, f) = pwsh_remote_setup();
        f.state.remote_environment.set(Some(OperatingSystem::Macintosh));
        set_env_var("HOME", Some("/home/user"));
        f.state.set_file_path("/home/user/.local/share/powershell/PSReadline/ConsoleHost_history.txt");
        assert_eq!(commands(&f, fetch_pwsh_history).await, expected_commands());
    });
}

#[test]
fn fetch_pwsh_history_remote_linux() {
    run_with_faked_timers(async {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let (_env, f) = pwsh_remote_setup();
        f.state.remote_environment.set(Some(OperatingSystem::Linux));
        set_env_var("HOME", Some("/home/user"));
        f.state.set_file_path("/home/user/.local/share/powershell/PSReadline/ConsoleHost_history.txt");
        assert_eq!(commands(&f, fetch_pwsh_history).await, expected_commands());
    });
}

// ---------------------------------------------------------------------------------------------------------------------
// suite('fetchFishHistory')
// ---------------------------------------------------------------------------------------------------------------------

fn fish_file_content() -> String {
    join_lines(&[
        "- cmd: single line command",
        "  when: 1650000000",
        "- cmd: git commit -m \"A wrapped line in pwsh history\\n\\nSome commit description\\n\\nFixes #xyz\"",
        "  when: 1650000010",
        "- cmd: git status",
        "  when: 1650000020",
        "- cmd: two \"\\nline\"",
        "  when: 1650000030",
    ])
}

/// O `setup` das suítes `local` e `remote` do fish: `HOME` definido e `XDG_DATA_HOME` apagado.
fn fish_suite_setup() -> (EnvGuard, Fixture) {
    let env = EnvGuard::save(&["HOME", "XDG_DATA_HOME"]);
    let fixture = Fixture::new(&fish_file_content(), check_scheme_and_path);
    set_env_var("HOME", Some("/home/user"));
    set_env_var("XDG_DATA_HOME", None);
    fixture.state.use_remote_file("/home/user/.local/share/fish/fish_history");
    (env, fixture)
}

/// O `setup` das suítes `local (overriden path)` e `remote (overriden path)` do fish.
fn fish_overriden_path_setup() -> (EnvGuard, Fixture) {
    let env = EnvGuard::save(&["XDG_DATA_HOME"]);
    let fixture = Fixture::new(&fish_file_content(), check_scheme_and_path);
    set_env_var("XDG_DATA_HOME", Some("/home/user/data-home"));
    fixture.state.use_remote_file("/home/user/data-home/fish/fish_history");
    (env, fixture)
}

#[test]
fn fetch_fish_history_local_current_os() {
    run_with_faked_timers(async {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        if !is_windows() {
            let (_env, f) = fish_suite_setup();
            f.state.set_file_path("/home/user/.local/share/fish/fish_history");
            assert_eq!(commands(&f, fetch_fish_history).await, expected_commands());
        }
    });
}

#[test]
fn fetch_fish_history_local_overriden_path_current_os() {
    run_with_faked_timers(async {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        if !is_windows() {
            let (_env, f) = fish_overriden_path_setup();
            f.state.set_file_path("/home/user/data-home/fish/fish_history");
            assert_eq!(commands(&f, fetch_fish_history).await, expected_commands());
        }
    });
}

#[test]
fn fetch_fish_history_remote_windows() {
    run_with_faked_timers(async {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let (_env, f) = fish_suite_setup();
        f.state.remote_environment.set(Some(OperatingSystem::Windows));
        assert_eq!(fetch(&f, fetch_fish_history).await, None);
    });
}

#[test]
fn fetch_fish_history_remote_macos() {
    run_with_faked_timers(async {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let (_env, f) = fish_suite_setup();
        f.state.remote_environment.set(Some(OperatingSystem::Macintosh));
        assert_eq!(commands(&f, fetch_fish_history).await, expected_commands());
    });
}

#[test]
fn fetch_fish_history_remote_linux() {
    run_with_faked_timers(async {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let (_env, f) = fish_suite_setup();
        f.state.remote_environment.set(Some(OperatingSystem::Linux));
        assert_eq!(commands(&f, fetch_fish_history).await, expected_commands());
    });
}

#[test]
fn fetch_fish_history_remote_overriden_path_windows() {
    run_with_faked_timers(async {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let (_env, f) = fish_overriden_path_setup();
        f.state.remote_environment.set(Some(OperatingSystem::Windows));
        assert_eq!(fetch(&f, fetch_fish_history).await, None);
    });
}

#[test]
fn fetch_fish_history_remote_overriden_path_macos() {
    run_with_faked_timers(async {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let (_env, f) = fish_overriden_path_setup();
        f.state.remote_environment.set(Some(OperatingSystem::Macintosh));
        assert_eq!(commands(&f, fetch_fish_history).await, expected_commands());
    });
}

#[test]
fn fetch_fish_history_remote_overriden_path_linux() {
    run_with_faked_timers(async {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let (_env, f) = fish_overriden_path_setup();
        f.state.remote_environment.set(Some(OperatingSystem::Linux));
        assert_eq!(commands(&f, fetch_fish_history).await, expected_commands());
    });
}

// ---------------------------------------------------------------------------------------------------------------------
// suite('sanitizeFishHistoryCmd')
// ---------------------------------------------------------------------------------------------------------------------

#[test]
fn sanitize_fish_history_cmd_valid_new_lines() {
    // Valid new-lines have odd number of leading backslashes: \n, \\\n, \\\\\n
    let cases = [
        "\\n",
        "\\n at start",
        "some \\n in the middle",
        "at the end \\n",
        "\\\\\\n",
        "\\\\\\n valid at start",
        "valid \\\\\\n in the middle",
        "valid in the end \\\\\\n",
        "\\\\\\\\\\n",
        "\\\\\\\\\\n valid at start",
        "valid \\\\\\\\\\n in the middle",
        "valid in the end \\\\\\\\\\n",
        "mixed valid \\r\\n",
        "mixed valid \\\\\\r\\n",
        "mixed valid \\r\\\\\\n",
    ];

    for x in cases {
        assert!(sanitize_fish_history_cmd(x).contains('\n'), "{x}");
    }
}

#[test]
fn sanitize_fish_history_cmd_invalid_new_lines() {
    // Invalid new-lines have even number of leading backslashes: \\n, \\\\n, \\\\\\n
    let cases = [
        "\\\\n",
        "\\\\n invalid at start",
        "invalid \\\\n in the middle",
        "invalid in the end \\\\n",
        "\\\\\\\\n",
        "\\\\\\\\n invalid at start",
        "invalid \\\\\\\\n in the middle",
        "invalid in the end \\\\\\\\n",
        "mixed invalid \\r\\\\n",
        "mixed invalid \\r\\\\\\\\n",
        "echo \"\\\\n\"",
    ];

    for x in cases {
        assert!(!sanitize_fish_history_cmd(x).contains('\n'), "{x}");
    }
}
