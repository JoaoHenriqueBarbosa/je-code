//! Porte de `src/vs/workbench/contrib/terminal/test/node/terminalProfiles.test.ts` (suíte `Workbench -
//! TerminalProfiles`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - O `ensureNoDisposablesAreLeakedInTestSuite` da suíte é o `LeakGuard` criado no começo de cada teste.
//! - O `if (isWindows) { ... } else { ... }` que declara os casos de cada sistema vira um `return` no começo de
//!   cada teste que não é do sistema atual (o Rust não declara teste em tempo de execução); os dois conjuntos
//!   compilam em qualquer sistema.
//! - O `ITestTerminalConfig` e o `as ITerminalConfiguration` viram o objeto dinâmico que o
//!   `TestConfigurationService` guarda (`{ terminal: { integrated: { profiles, useWslProfiles } } }`).
//! - `process.env` é a cópia do ambiente do processo (`SafeProcess::native`). O `createFsProvider` é o
//!   [`FsProvider`], que implementa `IFsProvider`; o `fail('Unexepected path')` é um pânico.
//! - `profilesEqual` compara `args` pelo valor (`undefined` é `None`, um vetor é `Some`) e ignora a ordem dos
//!   perfis, como o original.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use common::{ensure_no_disposables_are_leaked_in_test_suite, run_with_faked_timers};
use je_base::arrays::OneOrMany;
use je_base::dynamic::Dynamic;
use je_base::errors::Error;
use je_base::node::pfs::LocalFuture;
use je_base::process::SafeProcess;
use je_platform::configuration::test_configuration_service::TestConfigurationService;
use je_platform::terminal::node_terminal_profiles::{IFsProvider, detect_available_profiles};
use je_platform::terminal::terminal::{ITerminalProfile, ProfileSource};
use std::rc::Rc;

/// O perfil esperado: só os campos que o `profilesEqual` confere.
struct ExpectedProfile {
    profile_name: &'static str,
    path: &'static str,
    args: Option<Vec<&'static str>>,
    override_name: Option<bool>,
}

fn expected(profile_name: &'static str, path: &'static str) -> ExpectedProfile {
    ExpectedProfile { profile_name, path, args: None, override_name: None }
}

/// Assets that two profiles objects are equal, this will treat explicit undefined and unset
/// properties the same. Order of the profiles is ignored.
fn profiles_equal(actual_profiles: &[ITerminalProfile], expected_profiles: &[ExpectedProfile]) {
    let names = |names: Vec<&str>| names.join(",");
    assert_eq!(
        actual_profiles.len(),
        expected_profiles.len(),
        "Actual: {}\nExpected: {}",
        names(actual_profiles.iter().map(|profile| profile.profile_name.as_str()).collect()),
        names(expected_profiles.iter().map(|profile| profile.profile_name).collect())
    );
    for expected in expected_profiles {
        let actual = actual_profiles
            .iter()
            .find(|profile| profile.profile_name == expected.profile_name)
            .unwrap_or_else(|| panic!("Expected profile {} not found", expected.profile_name));
        assert_eq!(actual.profile_name, expected.profile_name);
        assert_eq!(actual.path, expected.path);
        let actual_args = match &actual.args {
            None => None,
            Some(OneOrMany::Many(args)) => Some(args.iter().map(String::as_str).collect::<Vec<_>>()),
            Some(OneOrMany::One(args)) => Some(vec![args.as_str()]),
        };
        assert_eq!(actual_args, expected.args);
        // `strictEqual(actual.isAutoDetected, expected.isAutoDetected)`: nenhum caso espera um valor definido.
        assert_eq!(actual.is_auto_detected, None);
        assert_eq!(actual.override_name, expected.override_name);
    }
}

/// `createFsProvider(expectedPaths, etcShellsContent)`.
struct FsProvider {
    expected_paths: Vec<String>,
    etc_shells_content: String,
}

fn create_fs_provider(expected_paths: &[&str], etc_shells_content: &str) -> FsProvider {
    FsProvider { expected_paths: expected_paths.iter().map(|path| (*path).to_owned()).collect(), etc_shells_content: etc_shells_content.to_owned() }
}

impl IFsProvider for FsProvider {
    fn exists_file<'a>(&self, path: &'a str) -> LocalFuture<'a, bool> {
        let exists = self.expected_paths.iter().any(|expected| expected == path);
        Box::pin(async move { exists })
    }

    fn read_file<'a>(&self, path: &'a str) -> LocalFuture<'a, Result<Vec<u8>, Error>> {
        if path != "/etc/shells" {
            panic!("Unexepected path");
        }
        let content = self.etc_shells_content.clone().into_bytes();
        Box::pin(async move { Ok(content) })
    }
}

fn path_profile(path: &str) -> Dynamic {
    Dynamic::object([("path", Dynamic::string(path))])
}

fn source_profile(source: ProfileSource) -> Dynamic {
    Dynamic::object([("source", Dynamic::string(source.as_str()))])
}

fn strings(values: &[&str]) -> Dynamic {
    Dynamic::array(values.iter().map(|value| Dynamic::string(value)))
}

fn empty() -> Dynamic {
    Dynamic::object(Vec::<(&str, Dynamic)>::new())
}

/// `new TestConfigurationService({ terminal: { integrated: { profiles: { windows, linux, osx }, useWslProfiles: false } } })`.
fn configuration_service(windows: Dynamic, linux: Dynamic, osx: Dynamic) -> Rc<TestConfigurationService> {
    let profiles = Dynamic::object([("windows", windows), ("linux", linux), ("osx", osx)]);
    let integrated = Dynamic::object([("profiles", profiles), ("useWslProfiles", Dynamic::Bool(false))]);
    TestConfigurationService::new(Some(Dynamic::object([("terminal", Dynamic::object([("integrated", integrated)]))])))
}

/// `detectAvailableProfiles(undefined, undefined, includeDetectedProfiles, configurationService, process.env, fsProvider,
/// undefined, undefined, testPwshSourcePaths)`.
async fn detect(
    include_detected_profiles: bool,
    configuration_service: &TestConfigurationService,
    fs_provider: &FsProvider,
    test_pwsh_source_paths: Option<&[String]>,
) -> Vec<ITerminalProfile> {
    detect_available_profiles(
        None,
        None,
        include_detected_profiles,
        configuration_service,
        &SafeProcess::native().env,
        Some(fs_provider),
        None,
        None,
        test_pwsh_source_paths,
    )
    .await
}

fn paths(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_owned()).collect()
}

// suite('detectAvailableProfiles') > if (isWindows)

#[test]
fn should_detect_git_bash_and_provide_login_args() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    if !cfg!(windows) {
        return;
    }
    run_with_faked_timers(async move {
        let fs_provider = create_fs_provider(&["C:\\Program Files\\Git\\bin\\bash.exe"], "");
        let windows = Dynamic::object([("Git Bash", source_profile(ProfileSource::GitBash))]);
        let configuration_service = configuration_service(windows, empty(), empty());
        let profiles = detect(false, &configuration_service, &fs_provider, None).await;
        let expected = [ExpectedProfile { args: Some(vec!["--login", "-i"]), ..expected("Git Bash", "C:\\Program Files\\Git\\bin\\bash.exe") }];
        profiles_equal(&profiles, &expected);
    });
}

#[test]
fn should_allow_source_to_have_args() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    if !cfg!(windows) {
        return;
    }
    run_with_faked_timers(async move {
        let pwsh_source_paths = paths(&["C:\\Program Files\\PowerShell\\7\\pwsh.exe"]);
        let fs_provider = create_fs_provider(&["C:\\Program Files\\PowerShell\\7\\pwsh.exe"], "");
        let power_shell = Dynamic::object([
            ("source", Dynamic::string(ProfileSource::Pwsh.as_str())),
            ("args", strings(&["-NoProfile"])),
            ("overrideName", Dynamic::Bool(true)),
        ]);
        let windows = Dynamic::object([("PowerShell", power_shell)]);
        let configuration_service = configuration_service(windows, empty(), empty());
        let profiles = detect(false, &configuration_service, &fs_provider, Some(&pwsh_source_paths)).await;
        let expected = [ExpectedProfile {
            override_name: Some(true),
            args: Some(vec!["-NoProfile"]),
            ..expected("PowerShell", "C:\\Program Files\\PowerShell\\7\\pwsh.exe")
        }];
        profiles_equal(&profiles, &expected);
    });
}

#[test]
fn configured_args_should_override_default_source_ones() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    if !cfg!(windows) {
        return;
    }
    run_with_faked_timers(async move {
        let fs_provider = create_fs_provider(&["C:\\Program Files\\Git\\bin\\bash.exe"], "");
        let git_bash = Dynamic::object([("source", Dynamic::string(ProfileSource::GitBash.as_str())), ("args", strings(&[]))]);
        let windows = Dynamic::object([("Git Bash", git_bash)]);
        let configuration_service = configuration_service(windows, empty(), empty());
        let profiles = detect(false, &configuration_service, &fs_provider, None).await;
        let expected = [ExpectedProfile { args: Some(vec![]), ..expected("Git Bash", "C:\\Program Files\\Git\\bin\\bash.exe") }];
        profiles_equal(&profiles, &expected);
    });
}

// suite('detectAvailableProfiles') > if (isWindows) > suite('pwsh source detection/fallback')

/// `pwshSourceConfig`: `{ profiles: { windows: { 'PowerShell': { source: ProfileSource.Pwsh } }, linux: {}, osx: {} }, useWslProfiles: false }`.
fn pwsh_source_configuration_service() -> Rc<TestConfigurationService> {
    configuration_service(Dynamic::object([("PowerShell", source_profile(ProfileSource::Pwsh))]), empty(), empty())
}

#[test]
fn should_prefer_pwsh_7_to_windows_powershell() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    if !cfg!(windows) {
        return;
    }
    run_with_faked_timers(async move {
        let pwsh_source_paths = paths(&[
            "C:\\Program Files\\PowerShell\\7\\pwsh.exe",
            "C:\\Sysnative\\WindowsPowerShell\\v1.0\\powershell.exe",
            "C:\\System32\\WindowsPowerShell\\v1.0\\powershell.exe",
        ]);
        let fs_provider = FsProvider { expected_paths: pwsh_source_paths.clone(), etc_shells_content: String::new() };
        let configuration_service = pwsh_source_configuration_service();
        let profiles = detect(false, &configuration_service, &fs_provider, Some(&pwsh_source_paths)).await;
        let expected = [expected("PowerShell", "C:\\Program Files\\PowerShell\\7\\pwsh.exe")];
        profiles_equal(&profiles, &expected);
    });
}

#[test]
fn should_prefer_pwsh_7_to_pwsh_6() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    if !cfg!(windows) {
        return;
    }
    run_with_faked_timers(async move {
        let pwsh_source_paths = paths(&[
            "C:\\Program Files\\PowerShell\\7\\pwsh.exe",
            "C:\\Program Files\\PowerShell\\6\\pwsh.exe",
            "C:\\Sysnative\\WindowsPowerShell\\v1.0\\powershell.exe",
            "C:\\System32\\WindowsPowerShell\\v1.0\\powershell.exe",
        ]);
        let fs_provider = FsProvider { expected_paths: pwsh_source_paths.clone(), etc_shells_content: String::new() };
        let configuration_service = pwsh_source_configuration_service();
        let profiles = detect(false, &configuration_service, &fs_provider, Some(&pwsh_source_paths)).await;
        let expected = [expected("PowerShell", "C:\\Program Files\\PowerShell\\7\\pwsh.exe")];
        profiles_equal(&profiles, &expected);
    });
}

#[test]
fn should_fallback_to_windows_powershell() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    if !cfg!(windows) {
        return;
    }
    run_with_faked_timers(async move {
        let pwsh_source_paths = paths(&[
            "C:\\Windows\\Sysnative\\WindowsPowerShell\\v1.0\\powershell.exe",
            "C:\\Windows\\System32\\WindowsPowerShell\\v1.0\\powershell.exe",
        ]);
        let fs_provider = FsProvider { expected_paths: pwsh_source_paths.clone(), etc_shells_content: String::new() };
        let configuration_service = pwsh_source_configuration_service();
        let profiles = detect(false, &configuration_service, &fs_provider, Some(&pwsh_source_paths)).await;
        assert_eq!(profiles.len(), 1);
        assert_eq!(profiles[0].profile_name, "PowerShell");
    });
}

// suite('detectAvailableProfiles') > else

/// `absoluteConfig`: `fakeshell1`, `fakeshell2` e `fakeshell3` por caminho absoluto, em `osx` e `linux`.
fn absolute_configuration_service() -> Rc<TestConfigurationService> {
    let shells = || {
        Dynamic::object([
            ("fakeshell1", path_profile("/bin/fakeshell1")),
            ("fakeshell2", path_profile("/bin/fakeshell2")),
            ("fakeshell3", path_profile("/bin/fakeshell3")),
        ])
    };
    configuration_service(empty(), shells(), shells())
}

/// `onPathConfig`: os mesmos três shells, só pelo nome (achados no `PATH`).
fn on_path_configuration_service() -> Rc<TestConfigurationService> {
    let shells = || {
        Dynamic::object([("fakeshell1", path_profile("fakeshell1")), ("fakeshell2", path_profile("fakeshell2")), ("fakeshell3", path_profile("fakeshell3"))])
    };
    configuration_service(empty(), shells(), shells())
}

#[test]
fn should_detect_shells_via_absolute_paths() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    if cfg!(windows) {
        return;
    }
    run_with_faked_timers(async move {
        let fs_provider = create_fs_provider(&["/bin/fakeshell1", "/bin/fakeshell3"], "");
        let configuration_service = absolute_configuration_service();
        let profiles = detect(false, &configuration_service, &fs_provider, None).await;
        let expected = [expected("fakeshell1", "/bin/fakeshell1"), expected("fakeshell3", "/bin/fakeshell3")];
        profiles_equal(&profiles, &expected);
    });
}

#[test]
fn should_auto_detect_shells_via_etc_shells() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    if cfg!(windows) {
        return;
    }
    run_with_faked_timers(async move {
        let fs_provider = create_fs_provider(&["/bin/fakeshell1", "/bin/fakeshell3"], "/bin/fakeshell1\n/bin/fakeshell3");
        let configuration_service = on_path_configuration_service();
        let profiles = detect(true, &configuration_service, &fs_provider, None).await;
        let expected = [expected("fakeshell1", "/bin/fakeshell1"), expected("fakeshell3", "/bin/fakeshell3")];
        profiles_equal(&profiles, &expected);
    });
}

#[test]
fn should_validate_auto_detected_shells_from_etc_shells_exist() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    if cfg!(windows) {
        return;
    }
    run_with_faked_timers(async move {
        // fakeshell3 exists in /etc/shells but not on FS
        let fs_provider = create_fs_provider(&["/bin/fakeshell1"], "/bin/fakeshell1\n/bin/fakeshell3");
        let configuration_service = on_path_configuration_service();
        let profiles = detect(true, &configuration_service, &fs_provider, None).await;
        let expected = [expected("fakeshell1", "/bin/fakeshell1")];
        profiles_equal(&profiles, &expected);
    });
}
