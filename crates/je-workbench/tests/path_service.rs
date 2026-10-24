//! Porte de `src/vs/workbench/services/path/test/browser/pathService.test.ts` (suíte `PathService`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - **`ensureNoDisposablesAreLeakedInTestSuite()`.** É o guarda do começo do caso (`const disposables = ...`), que também
//!   recebe o `disposables.add(...)`.
//! - **`upcastPartial<IRemoteAgentService>` e `upcastPartial<IRemoteAgentEnvironment>`.** São o [`RemoteAgentService`] de teste,
//!   cujo `getEnvironment` devolve `null` (sem sistema operacional) ou um ambiente com o `os` dado; os demais campos do
//!   `RemoteAgentEnvironment` não são lidos pelo `AbstractPathService`, mas a struct do porte os exige, então levam valores
//!   neutros fixos.
//! - **`new class extends AbstractPathService`.** A subclasse anônima só chama o `super` com `URI.file('/home/test')`, o
//!   serviço remoto, o `TestEnvironmentService` e um `TestContextService`: é o [`AbstractPathService::new`] com esses
//!   argumentos (a classe abstrata é a struct concreta, ver o cabeçalho de `path_service.rs`).
//! - **`getPath(resource)?.sep`.** O `IPath` é o [`PathFlavor`] (`Platform`), e o `sep` é o do `PathModule` desse sabor
//!   ([`separator_of`]).
//! - **`IResourcePathProvider`.** O objeto literal é o [`TestPathProvider`], com uma função de `getOperatingSystem`; o
//!   `assert.deepStrictEqual` compara os mesmos valores por `assert_eq!` (o `undefined` é `None`).
//! - **`URI.from({ scheme, authority, path })`.** É `URI::from` com as três componentes.
//! - **`OperatingSystem`.** O de `je_base::platform` (o `Windows`, o `Linux`).
//! - **Relógio.** O original não usa timers falsos: os casos rodam em `run_with_real_timers`.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use std::rc::Rc;

use common::{ensure_no_disposables_are_leaked_in_test_suite, run_with_real_timers};
use je_base::JsString;
use je_base::lifecycle::IDisposable;
use je_base::network::schemas;
use je_base::path::{PathModule, ProcessEnvironment};
use je_base::platform::OperatingSystem;
use je_base::promise::Promise;
use je_base::uri::{URI, UriComponents};
use je_platform::path::path_service::IResourcePathProvider;
use je_platform::remote::remote_agent_environment::{RemoteAgentEnvironment, RemoteAgentProfiles};
use je_platform::resources::uri_file;
use je_platform::workspace::IWorkspaceContextService;
use je_workbench::services::environment::environment_service::IWorkbenchEnvironmentService;
use je_workbench::services::path::path_service::{AbstractPathService, IPathService, PathFlavor};
use je_workbench::services::remote::remote_agent_service::{IRemoteAgentConnection, IRemoteAgentService};
use je_workbench::test::browser::workbench_test_services::TestEnvironmentService;
use je_workbench::test::common::workbench_test_services::TestContextService;

//#region Utilitários dos casos

/// O `IRemoteAgentService` parcial de `createPathService`: `getEnvironment` devolve `null` ou um ambiente só com o `os`.
struct RemoteAgentService {
    remote_operating_system: Option<OperatingSystem>,
}

impl IRemoteAgentService for RemoteAgentService {
    fn get_environment(&self) -> Promise<Option<RemoteAgentEnvironment>> {
        let dir = |path: &str| uri_file(path);
        Promise::resolved(self.remote_operating_system.map(|os| RemoteAgentEnvironment {
            pid: 1,
            connection_token: "token".to_owned(),
            app_root: dir("/remote/app"),
            exec_path: "/remote/app/node".to_owned(),
            tmp_dir: dir("/remote/tmp"),
            settings_path: dir("/remote/settings.json"),
            mcp_resource: dir("/remote/mcp.json"),
            logs_path: dir("/remote/logs"),
            extension_host_logs_path: dir("/remote/ext-logs"),
            global_storage_home: dir("/remote/global-storage"),
            workspace_storage_home: dir("/remote/workspace-storage"),
            local_history_home: dir("/remote/local-history"),
            user_home: dir("/remote/home"),
            os,
            arch: "x64".to_owned(),
            marks: Vec::new(),
            use_host_proxy: false,
            profiles: RemoteAgentProfiles { all: Vec::new(), home: dir("/remote/profiles") },
            is_unsupported_glibc: false,
            reconnection_grace_time: None,
        }))
    }

    fn get_raw_environment(&self) -> Promise<Option<RemoteAgentEnvironment>> {
        self.get_environment()
    }

    fn get_connection(&self) -> Option<Rc<dyn IRemoteAgentConnection>> {
        None
    }
}

/// `createPathService(remoteOperatingSystem)`.
fn create_path_service(remote_operating_system: Option<OperatingSystem>) -> AbstractPathService {
    let remote_agent_service: Rc<dyn IRemoteAgentService> = Rc::new(RemoteAgentService { remote_operating_system });
    let environment_service: Rc<dyn IWorkbenchEnvironmentService> = TestEnvironmentService::new();
    let context_service: Rc<dyn IWorkspaceContextService> = TestContextService::new(None, None);

    AbstractPathService::new(uri_file("/home/test"), remote_agent_service, environment_service, context_service)
}

/// `path.sep` do `IPath` do sabor dado.
fn separator_of(flavor: PathFlavor) -> &'static str {
    match flavor {
        PathFlavor::Win32 => PathModule::win32(&ProcessEnvironment).sep(),
        PathFlavor::Posix => PathModule::posix(&ProcessEnvironment).sep(),
    }
}

/// `URI.from({ scheme, authority, path })`.
fn uri_of(scheme: &str, authority: &str, path: &str) -> URI {
    URI::from(
        &UriComponents { scheme: Some(JsString::from(scheme)), authority: Some(JsString::from(authority)), path: Some(JsString::from(path)), ..UriComponents::default() },
        false,
    )
}

/// `{ getOperatingSystem: async resource => resource.authority === authority ? os : undefined }`.
struct TestPathProvider {
    authority: &'static str,
    operating_system: OperatingSystem,
}

impl IResourcePathProvider for TestPathProvider {
    fn get_operating_system(&self, resource: &URI) -> Promise<Option<OperatingSystem>> {
        Promise::resolved((resource.authority().to_string_lossy() == self.authority).then_some(self.operating_system))
    }
}

//#endregion

#[test]
fn resolves_ambient_vscode_remote_semantics_from_the_remote_environment() {
    run_with_real_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();

        let path_service = create_path_service(Some(OperatingSystem::Windows));
        let resource = uri_of(schemas::VSCODE_REMOTE, "ssh-remote+host", "/workspace");

        assert_eq!(
            (
                path_service.get_operating_system(&resource).await.unwrap(),
                path_service.get_path(&resource).await.unwrap().map(separator_of),
            ),
            (Some(OperatingSystem::Windows), Some("\\")),
        );

        drop(disposables);
    });
}

#[test]
fn resolves_mixed_registered_authorities_and_falls_back_when_semantics_are_unknown() {
    run_with_real_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();

        let path_service = create_path_service(None);
        let first = disposables.add(path_service.register_path_provider("test-remote", Rc::new(TestPathProvider { authority: "windows", operating_system: OperatingSystem::Windows })));
        let second = path_service.register_path_provider("test-remote", Rc::new(TestPathProvider { authority: "linux", operating_system: OperatingSystem::Linux }));
        let windows = uri_of("test-remote", "windows", "/");
        let linux = uri_of("test-remote", "linux", "/");
        let unknown = uri_of("test-remote", "unknown", "/");

        let windows_actual = (path_service.get_operating_system(&windows).await.unwrap(), path_service.get_path(&windows).await.unwrap().map(separator_of));
        let linux_actual = (path_service.get_operating_system(&linux).await.unwrap(), path_service.get_path(&linux).await.unwrap().map(separator_of));
        let unknown_actual = path_service.get_path(&unknown).await.unwrap();

        assert_eq!(windows_actual, (Some(OperatingSystem::Windows), Some("\\")));
        assert_eq!(linux_actual, (Some(OperatingSystem::Linux), Some("/")));
        assert_eq!(unknown_actual, None);

        second.dispose();
        first.dispose();
        drop(disposables);
    });
}
