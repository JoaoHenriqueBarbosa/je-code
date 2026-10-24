//! Porte de `src/vs/workbench/contrib/terminal/test/common/terminalEnvironment.test.ts` (suíte `Workbench -
//! TerminalEnvironment`), caso a caso e na mesma ordem.
//!
//! Divergências:
//! - O `ensureNoDisposablesAreLeakedInTestSuite` da suíte é o `LeakGuard` criado no começo de cada teste.
//! - Os `(!isWindows ? test.skip : test)` viram `#[cfg(windows)]`: o Rust não pula um teste em tempo de execução.
//! - O `wslPathBackend` do original é o [`WslPathBackend`] (as duas expressões regulares com grupos nomeados
//!   viram a varredura equivalente). O `TestHistoryService` é o [`TestHistoryService`] daqui, porque o do porte
//!   de `workbenchTestServices` depende do `EditorInput`, que ainda não existe (`IHistoryService.getHistory`).
//! - O `undefined` de `getLangEnvVariable`, de `getWorkspaceForTerminal` e dos parâmetros opcionais é `None`.
//!   Os literais `{ executable: undefined, args: [] }` são o [`launch_config`].
//! - `deepStrictEqual` sobre o ambiente é `assert_eq!` sobre o [`ProcessEnvironment`] (mapa ordenado, sem
//!   ordem de inserção).

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use common::{ensure_no_disposables_are_leaked_in_test_suite, run_with_faked_timers};
use je_base::arrays::OneOrMany;
use je_base::platform::OperatingSystem;
use je_base::process::ProcessEnvironment;
use je_base::promise::Promise;
use je_base::uri::URI;
use je_platform::resources::{fs_path, uri_file};
use je_platform::terminal::terminal::{
    GeneralShellType, IShellLaunchConfig, ITerminalEnvironment, PosixShellType, ShellType, TerminalEnvironmentValue, WindowsShellType,
};
use je_platform::workspace::test_workspace;
use je_workbench::contrib::terminal::common::terminal_environment::{
    DetectLocale, IHistoryService, ITerminalBackendWslPath, ShellResource, WslPathDirection, add_terminal_environment_keys,
    create_terminal_environment, get_cwd, get_lang_env_variable, get_uri_label_for_shell, get_workspace_for_terminal, merge_environments,
    prepare_path_for_shell, should_set_lang_env_variable,
};
use je_workbench::test::common::workbench_test_services::TestContextService;

/// O `wslPathBackend` do original.
struct WslPathBackend;

/// `/[\n\r\u2028\u2029]/`: os terminadores de linha, que o `.` não casa.
fn is_line_terminator(c: char) -> bool {
    matches!(c, '\n' | '\r' | '\u{2028}' | '\u{2029}')
}

/// `original.match(/^\/mnt\/(?<drive>[a-zA-Z])\/(?<path>.+)$/)` e a troca que o original faz com o resultado.
fn unix_to_win(original: &str) -> String {
    let converted = original.strip_prefix("/mnt/").and_then(|rest| {
        let mut chars = rest.chars();
        let drive = chars.next().filter(char::is_ascii_alphabetic)?;
        if chars.next() != Some('/') {
            return None;
        }
        let path = chars.as_str();
        (!path.is_empty() && !path.chars().any(is_line_terminator)).then(|| format!("{drive}:\\{}", path.replace('/', "\\")))
    });
    converted.unwrap_or_else(|| original.to_owned())
}

/// `original.match(/(?<drive>[a-zA-Z]):\\(?<path>.+)/)` (sem âncoras) e a troca que o original faz com o resultado.
fn win_to_unix(original: &str) -> String {
    let chars: Vec<char> = original.chars().collect();
    for start in 0..chars.len().saturating_sub(3) {
        if !(chars[start].is_ascii_alphabetic() && chars[start + 1] == ':' && chars[start + 2] == '\\') {
            continue;
        }
        let path: String = chars[start + 3..].iter().copied().take_while(|c| !is_line_terminator(*c)).collect();
        if !path.is_empty() {
            return format!("/mnt/{}/{}", chars[start].to_lowercase(), path.replace('\\', "/"));
        }
    }
    original.to_owned()
}

impl ITerminalBackendWslPath for WslPathBackend {
    fn get_wsl_path(&self, original: &str, direction: WslPathDirection) -> Promise<String> {
        Promise::resolved(match direction {
            WslPathDirection::UnixToWin => unix_to_win(original),
            WslPathDirection::WinToUnix => win_to_unix(original),
        })
    }
}

/// `TestHistoryService`: o `getLastActiveWorkspaceRoot` devolve a raiz do construtor.
struct TestHistoryService {
    root: Option<URI>,
}

impl IHistoryService for TestHistoryService {
    fn get_last_active_workspace_root(&self, _scheme_filter: Option<&str>, _authority_filter: Option<&str>) -> Option<URI> {
        self.root.clone()
    }
}

fn process_env(pairs: &[(&str, &str)]) -> ProcessEnvironment {
    pairs.iter().map(|(key, value)| ((*key).to_owned(), (*value).to_owned())).collect()
}

fn terminal_env(pairs: &[(&str, TerminalEnvironmentValue)]) -> ITerminalEnvironment {
    pairs.iter().map(|(key, value)| ((*key).to_owned(), value.clone())).collect()
}

fn value(text: &str) -> TerminalEnvironmentValue {
    TerminalEnvironmentValue::Value(text.to_owned())
}

/// `{ executable: undefined, args: [] }` com os campos opcionais do teste.
fn launch_config(ignore_configuration_cwd: Option<bool>) -> IShellLaunchConfig {
    IShellLaunchConfig { executable: None, args: Some(OneOrMany::Many(Vec::new())), ignore_configuration_cwd, ..Default::default() }
}

/// `preparePathForShell(path, executable, title, shellType, wslPathBackend, os, isWindowsFrontend)`.
async fn prepare(path: &str, executable: &str, title: &str, shell_type: impl Into<ShellType>, os: OperatingSystem, is_windows_frontend: bool) -> String {
    let backend: &dyn ITerminalBackendWslPath = &WslPathBackend;
    prepare_path_for_shell(ShellResource::Path(path), Some(executable), title, Some(shell_type.into()), Some(backend), Some(os), Some(is_windows_frontend))
        .await
        .unwrap()
}

/// `getUriLabelForShell(path, wslPathBackend, shellType, os, isWindowsFrontend)`.
async fn uri_label(path: &str, shell_type: impl Into<ShellType>, os: OperatingSystem, is_windows_frontend: bool) -> String {
    get_uri_label_for_shell(ShellResource::Path(path), &WslPathBackend, Some(shell_type.into()), Some(os), Some(is_windows_frontend)).await.unwrap()
}

/// `assertPathsMatch`: confere os caminhos de forma que funcione em qualquer plataforma.
fn assert_paths_match(a: &str, b: &str) {
    assert_eq!(fs_path(&uri_file(a)), fs_path(&uri_file(b)));
}

// suite('addTerminalEnvironmentKeys')

#[test]
fn add_terminal_environment_keys_should_set_expected_variables() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let mut env = ProcessEnvironment::new();
    add_terminal_environment_keys(&mut env, Some("1.2.3"), Some("en"), DetectLocale::On);
    assert_eq!(env["TERM_PROGRAM"], "vscode");
    assert_eq!(env["TERM_PROGRAM_VERSION"], "1.2.3");
    assert_eq!(env["COLORTERM"], "truecolor");
    assert_eq!(env["LANG"], "en_US.UTF-8");
}

#[test]
fn add_terminal_environment_keys_should_use_language_variant_for_lang_that_is_provided_in_locale() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let mut env = ProcessEnvironment::new();
    add_terminal_environment_keys(&mut env, Some("1.2.3"), Some("en-au"), DetectLocale::On);
    assert_eq!(env["LANG"], "en_AU.UTF-8", "LANG is equal to the requested locale with UTF-8");
}

#[test]
fn add_terminal_environment_keys_should_fallback_to_en_us_when_no_locale_is_provided() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let mut env2 = process_env(&[("FOO", "bar")]);
    add_terminal_environment_keys(&mut env2, Some("1.2.3"), None, DetectLocale::On);
    assert_eq!(env2["LANG"], "en_US.UTF-8", "LANG is equal to en_US.UTF-8 as fallback."); // More info on issue #14586
}

#[test]
fn add_terminal_environment_keys_should_fallback_to_en_us_when_an_invalid_locale_is_provided() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let mut env3 = process_env(&[("LANG", "replace")]);
    add_terminal_environment_keys(&mut env3, Some("1.2.3"), None, DetectLocale::On);
    assert_eq!(env3["LANG"], "en_US.UTF-8", "LANG is set to the fallback LANG");
}

#[test]
fn add_terminal_environment_keys_should_override_existing_lang() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let mut env4 = process_env(&[("LANG", "en_AU.UTF-8")]);
    add_terminal_environment_keys(&mut env4, Some("1.2.3"), None, DetectLocale::On);
    assert_eq!(env4["LANG"], "en_US.UTF-8", "LANG is equal to the parent environment's LANG");
}

// suite('shouldSetLangEnvVariable')

#[test]
fn should_set_lang_env_variable_auto() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    assert!(should_set_lang_env_variable(&process_env(&[]), DetectLocale::Auto));
    assert!(should_set_lang_env_variable(&process_env(&[("LANG", "en-US")]), DetectLocale::Auto));
    assert!(should_set_lang_env_variable(&process_env(&[("LANG", "en-US.utf")]), DetectLocale::Auto));
    assert!(!should_set_lang_env_variable(&process_env(&[("LANG", "en-US.utf8")]), DetectLocale::Auto));
    assert!(!should_set_lang_env_variable(&process_env(&[("LANG", "en-US.UTF-8")]), DetectLocale::Auto));
}

#[test]
fn should_set_lang_env_variable_off() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    assert!(!should_set_lang_env_variable(&process_env(&[]), DetectLocale::Off));
    assert!(!should_set_lang_env_variable(&process_env(&[("LANG", "en-US")]), DetectLocale::Off));
    assert!(!should_set_lang_env_variable(&process_env(&[("LANG", "en-US.utf")]), DetectLocale::Off));
    assert!(!should_set_lang_env_variable(&process_env(&[("LANG", "en-US.utf8")]), DetectLocale::Off));
    assert!(!should_set_lang_env_variable(&process_env(&[("LANG", "en-US.UTF-8")]), DetectLocale::Off));
}

#[test]
fn should_set_lang_env_variable_on() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    assert!(should_set_lang_env_variable(&process_env(&[]), DetectLocale::On));
    assert!(should_set_lang_env_variable(&process_env(&[("LANG", "en-US")]), DetectLocale::On));
    assert!(should_set_lang_env_variable(&process_env(&[("LANG", "en-US.utf")]), DetectLocale::On));
    assert!(should_set_lang_env_variable(&process_env(&[("LANG", "en-US.utf8")]), DetectLocale::On));
    assert!(should_set_lang_env_variable(&process_env(&[("LANG", "en-US.UTF-8")]), DetectLocale::On));
}

// suite('getLangEnvVariable')

#[test]
fn get_lang_env_variable_should_fallback_to_en_us_when_no_locale_is_provided() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(get_lang_env_variable(None), "en_US.UTF-8");
    assert_eq!(get_lang_env_variable(Some("")), "en_US.UTF-8");
}

#[test]
fn get_lang_env_variable_should_fallback_to_default_language_variants_when_variant_isnt_provided() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let expected = [
        ("af", "af_ZA.UTF-8"),
        ("am", "am_ET.UTF-8"),
        ("be", "be_BY.UTF-8"),
        ("bg", "bg_BG.UTF-8"),
        ("ca", "ca_ES.UTF-8"),
        ("cs", "cs_CZ.UTF-8"),
        ("da", "da_DK.UTF-8"),
        ("de", "de_DE.UTF-8"),
        ("el", "el_GR.UTF-8"),
        ("en", "en_US.UTF-8"),
        ("es", "es_ES.UTF-8"),
        ("et", "et_EE.UTF-8"),
        ("eu", "eu_ES.UTF-8"),
        ("fi", "fi_FI.UTF-8"),
        ("fr", "fr_FR.UTF-8"),
        ("he", "he_IL.UTF-8"),
        ("hr", "hr_HR.UTF-8"),
        ("hu", "hu_HU.UTF-8"),
        ("hy", "hy_AM.UTF-8"),
        ("is", "is_IS.UTF-8"),
        ("it", "it_IT.UTF-8"),
        ("ja", "ja_JP.UTF-8"),
        ("kk", "kk_KZ.UTF-8"),
        ("ko", "ko_KR.UTF-8"),
        ("lt", "lt_LT.UTF-8"),
        ("nl", "nl_NL.UTF-8"),
        ("no", "no_NO.UTF-8"),
        ("pl", "pl_PL.UTF-8"),
        ("pt", "pt_BR.UTF-8"),
        ("ro", "ro_RO.UTF-8"),
        ("ru", "ru_RU.UTF-8"),
        ("sk", "sk_SK.UTF-8"),
        ("sl", "sl_SI.UTF-8"),
        ("sr", "sr_YU.UTF-8"),
        ("sv", "sv_SE.UTF-8"),
        ("tr", "tr_TR.UTF-8"),
        ("uk", "uk_UA.UTF-8"),
        ("zh", "zh_CN.UTF-8"),
    ];
    for (locale, lang) in expected {
        assert_eq!(get_lang_env_variable(Some(locale)), lang);
    }
}

#[test]
fn get_lang_env_variable_should_set_language_variant_based_on_full_locale() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(get_lang_env_variable(Some("en-AU")), "en_AU.UTF-8");
    assert_eq!(get_lang_env_variable(Some("en-au")), "en_AU.UTF-8");
    assert_eq!(get_lang_env_variable(Some("fa-ke")), "fa_KE.UTF-8");
}

// suite('mergeEnvironments')

#[test]
fn merge_environments_should_add_keys() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let mut parent = process_env(&[("a", "b")]);
    let other = terminal_env(&[("c", value("d"))]);
    merge_environments(&mut parent, Some(&other));
    assert_eq!(parent, process_env(&[("a", "b"), ("c", "d")]));
}

#[cfg(windows)]
#[test]
fn merge_environments_should_add_keys_ignoring_case_on_windows() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let mut parent = process_env(&[("a", "b")]);
    let other = terminal_env(&[("A", value("c"))]);
    merge_environments(&mut parent, Some(&other));
    assert_eq!(parent, process_env(&[("a", "c")]));
}

#[test]
fn merge_environments_null_values_should_delete_keys_from_the_parent_env() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let mut parent = process_env(&[("a", "b"), ("c", "d")]);
    let other = terminal_env(&[("a", TerminalEnvironmentValue::Null)]);
    merge_environments(&mut parent, Some(&other));
    assert_eq!(parent, process_env(&[("c", "d")]));
}

#[cfg(windows)]
#[test]
fn merge_environments_null_values_should_delete_keys_from_the_parent_env_ignoring_case_on_windows() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let mut parent = process_env(&[("a", "b"), ("c", "d")]);
    let other = terminal_env(&[("A", TerminalEnvironmentValue::Null)]);
    merge_environments(&mut parent, Some(&other));
    assert_eq!(parent, process_env(&[("c", "d")]));
}

// suite('getCwd')

#[test]
fn get_cwd_should_default_to_user_home_for_an_empty_workspace() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    run_with_faked_timers(async move {
        assert_paths_match(&get_cwd(&launch_config(None), Some("/userHome/"), None, None, None, None).await, "/userHome/");
    });
}

#[test]
fn get_cwd_should_use_to_the_workspace_if_it_exists() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    run_with_faked_timers(async move {
        assert_paths_match(&get_cwd(&launch_config(None), Some("/userHome/"), None, Some(&uri_file("/foo")), None, None).await, "/foo");
    });
}

#[test]
fn get_cwd_should_use_an_absolute_custom_cwd_as_is() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    run_with_faked_timers(async move {
        assert_paths_match(&get_cwd(&launch_config(None), Some("/userHome/"), None, None, Some("/foo"), None).await, "/foo");
    });
}

#[test]
fn get_cwd_should_normalize_a_relative_custom_cwd_against_the_workspace_path() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    run_with_faked_timers(async move {
        let bar = uri_file("/bar");
        assert_paths_match(&get_cwd(&launch_config(None), Some("/userHome/"), None, Some(&bar), Some("foo"), None).await, "/bar/foo");
        assert_paths_match(&get_cwd(&launch_config(None), Some("/userHome/"), None, Some(&bar), Some("./foo"), None).await, "/bar/foo");
        assert_paths_match(&get_cwd(&launch_config(None), Some("/userHome/"), None, Some(&bar), Some("../foo"), None).await, "/foo");
    });
}

#[test]
fn get_cwd_should_fall_back_for_relative_a_custom_cwd_that_doesnt_have_a_workspace() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    run_with_faked_timers(async move {
        assert_paths_match(&get_cwd(&launch_config(None), Some("/userHome/"), None, None, Some("foo"), None).await, "/userHome/");
        assert_paths_match(&get_cwd(&launch_config(None), Some("/userHome/"), None, None, Some("./foo"), None).await, "/userHome/");
        assert_paths_match(&get_cwd(&launch_config(None), Some("/userHome/"), None, None, Some("../foo"), None).await, "/userHome/");
    });
}

#[test]
fn get_cwd_should_ignore_custom_cwd_when_told_to_ignore() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    run_with_faked_timers(async move {
        assert_paths_match(&get_cwd(&launch_config(Some(true)), Some("/userHome/"), None, Some(&uri_file("/bar")), Some("/foo"), None).await, "/bar");
    });
}

// suite('preparePathForShell') > suite('Windows frontend, Windows backend')

#[test]
fn prepare_path_for_shell_windows_frontend_windows_backend_command_prompt() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    run_with_faked_timers(async move {
        let shell = WindowsShellType::CommandPrompt;
        let os = OperatingSystem::Windows;
        assert_eq!(prepare("c:\\foo\\bar", "cmd", "cmd", shell, os, true).await, "c:\\foo\\bar");
        assert_eq!(prepare("c:\\foo\\bar'baz", "cmd", "cmd", shell, os, true).await, "c:\\foo\\bar'baz");
        assert_eq!(prepare("c:\\foo\\bar$(echo evil)baz", "cmd", "cmd", shell, os, true).await, "\"c:\\foo\\bar$(echo evil)baz\"");
    });
}

#[test]
fn prepare_path_for_shell_windows_frontend_windows_backend_power_shell() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    run_with_faked_timers(async move {
        let shell = GeneralShellType::PowerShell;
        let os = OperatingSystem::Windows;
        assert_eq!(prepare("c:\\foo\\bar", "pwsh", "pwsh", shell, os, true).await, "c:\\foo\\bar");
        assert_eq!(prepare("c:\\foo\\bar'baz", "pwsh", "pwsh", shell, os, true).await, "& 'c:\\foo\\bar''baz'");
        assert_eq!(prepare("c:\\foo\\bar$(echo evil)baz", "pwsh", "pwsh", shell, os, true).await, "& 'c:\\foo\\bar$(echo evil)baz'");
    });
}

#[test]
fn prepare_path_for_shell_windows_frontend_windows_backend_git_bash() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    run_with_faked_timers(async move {
        let shell = WindowsShellType::GitBash;
        let os = OperatingSystem::Windows;
        assert_eq!(prepare("c:\\foo\\bar", "bash", "bash", shell, os, true).await, "'c:/foo/bar'");
        assert_eq!(prepare("c:\\foo\\bar'baz", "bash", "bash", shell, os, true).await, "'c:/foo/bar'\\''baz'");
        assert_eq!(prepare("c:\\foo\\bar$(echo evil)baz", "bash", "bash", shell, os, true).await, "'c:/foo/bar(echo evil)baz'");
    });
}

#[test]
fn prepare_path_for_shell_windows_frontend_windows_backend_wsl() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    run_with_faked_timers(async move {
        assert_eq!(prepare("c:\\foo\\bar", "bash", "bash", WindowsShellType::Wsl, OperatingSystem::Windows, true).await, "/mnt/c/foo/bar");
    });
}

// suite('preparePathForShell') > suite('Windows frontend, Linux backend')

#[test]
fn prepare_path_for_shell_windows_frontend_linux_backend_bash() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    run_with_faked_timers(async move {
        let shell = PosixShellType::Bash;
        let os = OperatingSystem::Linux;
        assert_eq!(prepare("/foo/bar", "bash", "bash", shell, os, true).await, "'/foo/bar'");
        assert_eq!(prepare("/foo/bar'baz", "bash", "bash", shell, os, true).await, "'/foo/bar'\\''baz'");
        assert_eq!(prepare("/foo/bar$(echo evil)baz", "bash", "bash", shell, os, true).await, "'/foo/bar(echo evil)baz'");
    });
}

#[test]
fn prepare_path_for_shell_windows_frontend_linux_backend_zsh() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    run_with_faked_timers(async move {
        let shell = PosixShellType::Zsh;
        let os = OperatingSystem::Linux;
        assert_eq!(prepare("/foo/bar", "zsh", "zsh", shell, os, true).await, "'/foo/bar'");
        assert_eq!(prepare("/foo/bar'baz", "zsh", "zsh", shell, os, true).await, "'/foo/bar'\\''baz'");
        assert_eq!(prepare("/foo/bar$(echo evil)baz", "zsh", "zsh", shell, os, true).await, "'/foo/bar(echo evil)baz'");
    });
}

#[test]
fn prepare_path_for_shell_windows_frontend_linux_backend_fish() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    run_with_faked_timers(async move {
        let shell = PosixShellType::Fish;
        let os = OperatingSystem::Linux;
        assert_eq!(prepare("/foo/bar", "fish", "fish", shell, os, true).await, "'/foo/bar'");
        assert_eq!(prepare("/foo/bar'baz", "fish", "fish", shell, os, true).await, "'/foo/bar\\'baz'");
        assert_eq!(prepare("/foo/bar$(echo evil)baz", "fish", "fish", shell, os, true).await, "'/foo/bar(echo evil)baz'");
    });
}

// suite('preparePathForShell') > suite('Linux frontend, Windows backend')

#[test]
fn prepare_path_for_shell_linux_frontend_windows_backend_command_prompt() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    run_with_faked_timers(async move {
        let shell = WindowsShellType::CommandPrompt;
        let os = OperatingSystem::Windows;
        assert_eq!(prepare("c:\\foo\\bar", "cmd", "cmd", shell, os, false).await, "c:\\foo\\bar");
        assert_eq!(prepare("c:\\foo\\bar'baz", "cmd", "cmd", shell, os, false).await, "c:\\foo\\bar'baz");
        assert_eq!(prepare("c:\\foo\\bar$(echo evil)baz", "cmd", "cmd", shell, os, false).await, "\"c:\\foo\\bar$(echo evil)baz\"");
    });
}

#[test]
fn prepare_path_for_shell_linux_frontend_windows_backend_power_shell() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    run_with_faked_timers(async move {
        let shell = GeneralShellType::PowerShell;
        let os = OperatingSystem::Windows;
        assert_eq!(prepare("c:\\foo\\bar", "pwsh", "pwsh", shell, os, false).await, "c:\\foo\\bar");
        assert_eq!(prepare("c:\\foo\\bar'baz", "pwsh", "pwsh", shell, os, false).await, "& 'c:\\foo\\bar''baz'");
        assert_eq!(prepare("c:\\foo\\bar$(echo evil)baz", "pwsh", "pwsh", shell, os, false).await, "& 'c:\\foo\\bar$(echo evil)baz'");
    });
}

#[test]
fn prepare_path_for_shell_linux_frontend_windows_backend_git_bash() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    run_with_faked_timers(async move {
        let shell = WindowsShellType::GitBash;
        let os = OperatingSystem::Windows;
        assert_eq!(prepare("c:\\foo\\bar", "bash", "bash", shell, os, false).await, "'c:/foo/bar'");
        assert_eq!(prepare("c:\\foo\\bar'baz", "bash", "bash", shell, os, false).await, "'c:/foo/bar'\\''baz'");
        assert_eq!(prepare("c:\\foo\\bar$(echo evil)baz", "bash", "bash", shell, os, false).await, "'c:/foo/bar(echo evil)baz'");
    });
}

#[test]
fn prepare_path_for_shell_linux_frontend_windows_backend_wsl() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    run_with_faked_timers(async move {
        assert_eq!(prepare("c:\\foo\\bar", "bash", "bash", WindowsShellType::Wsl, OperatingSystem::Windows, false).await, "/mnt/c/foo/bar");
    });
}

// suite('preparePathForShell') > suite('Linux frontend, Linux backend')

#[test]
fn prepare_path_for_shell_linux_frontend_linux_backend_bash() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    run_with_faked_timers(async move {
        let shell = PosixShellType::Bash;
        let os = OperatingSystem::Linux;
        assert_eq!(prepare("/foo/bar'\rtouch pwned\r'", "bash", "bash", shell, os, false).await, "'/foo/bar'\\''touch pwned'\\'''");
        assert_eq!(prepare("/foo/bar", "bash", "bash", shell, os, false).await, "'/foo/bar'");
        assert_eq!(prepare("/foo/bar'baz", "bash", "bash", shell, os, false).await, "'/foo/bar'\\''baz'");
        assert_eq!(prepare("/foo/bar$(echo evil)baz", "bash", "bash", shell, os, false).await, "'/foo/bar(echo evil)baz'");
    });
}

#[test]
fn prepare_path_for_shell_linux_frontend_linux_backend_zsh() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    run_with_faked_timers(async move {
        let shell = PosixShellType::Zsh;
        let os = OperatingSystem::Linux;
        assert_eq!(prepare("/foo/bar", "zsh", "zsh", shell, os, false).await, "'/foo/bar'");
        assert_eq!(prepare("/foo/bar'baz", "zsh", "zsh", shell, os, false).await, "'/foo/bar'\\''baz'");
        assert_eq!(prepare("/foo/bar$(echo evil)baz", "zsh", "zsh", shell, os, false).await, "'/foo/bar(echo evil)baz'");
    });
}

#[test]
fn prepare_path_for_shell_linux_frontend_linux_backend_fish() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    run_with_faked_timers(async move {
        let shell = PosixShellType::Fish;
        let os = OperatingSystem::Linux;
        assert_eq!(prepare("/foo/bar", "fish", "fish", shell, os, false).await, "'/foo/bar'");
        assert_eq!(prepare("/foo/bar'baz", "fish", "fish", shell, os, false).await, "'/foo/bar\\'baz'");
        assert_eq!(prepare("/foo/bar$(echo evil)baz", "fish", "fish", shell, os, false).await, "'/foo/bar(echo evil)baz'");
    });
}

// suite('createTerminalEnvironment')

#[test]
fn create_terminal_environment_should_retain_variables_equal_to_the_empty_string() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    run_with_faked_timers(async move {
        let common_variables = [("COLORTERM", "truecolor"), ("TERM_PROGRAM", "vscode")];
        let base_env = process_env(&[("foo", "bar"), ("empty", "")]);
        let mut expected = base_env.clone();
        expected.extend(process_env(&common_variables));
        assert_eq!(create_terminal_environment(&mut IShellLaunchConfig::default(), None, None, None, DetectLocale::Off, &base_env).await, expected);
    });
}

// suite('getWorkspaceForTerminal')

#[test]
fn get_workspace_for_terminal_should_resolve_workspace_folder_from_cwd_not_last_active_workspace() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let folder_a = uri_file("/workspace/proj1");
    let folder_b = uri_file("/workspace/proj2");
    let context_service = TestContextService::new(Some(test_workspace(&[folder_a.clone(), folder_b.clone()])), None);
    let history_service = TestHistoryService { root: Some(folder_a) };
    let result = get_workspace_for_terminal(Some(ShellResource::Uri(&folder_b)), context_service.as_ref(), &history_service);
    assert_eq!(result.map(|folder| fs_path(&folder.uri)), Some(fs_path(&folder_b)));
}

#[test]
fn get_workspace_for_terminal_should_fall_back_to_last_active_workspace_when_cwd_is_not_in_any_workspace_folder() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let folder_a = uri_file("/workspace/proj1");
    let context_service = TestContextService::new(Some(test_workspace(&[folder_a.clone()])), None);
    let history_service = TestHistoryService { root: Some(folder_a.clone()) };
    let result = get_workspace_for_terminal(Some(ShellResource::Uri(&uri_file("/other/path"))), context_service.as_ref(), &history_service);
    assert_eq!(result.map(|folder| fs_path(&folder.uri)), Some(fs_path(&folder_a)));
}

#[test]
fn get_workspace_for_terminal_should_fall_back_to_last_active_workspace_when_cwd_is_undefined() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let folder_a = uri_file("/workspace/proj1");
    let context_service = TestContextService::new(Some(test_workspace(&[folder_a.clone()])), None);
    let history_service = TestHistoryService { root: Some(folder_a.clone()) };
    assert_eq!(get_workspace_for_terminal(None, context_service.as_ref(), &history_service).map(|folder| fs_path(&folder.uri)), Some(fs_path(&folder_a)));
}

#[test]
fn get_workspace_for_terminal_should_return_undefined_when_cwd_and_history_are_both_unavailable() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let context_service = TestContextService::new(Some(test_workspace(&[uri_file("/workspace/proj1")])), None);
    let history_service = TestHistoryService { root: None };
    assert!(get_workspace_for_terminal(None, context_service.as_ref(), &history_service).is_none());
}

// suite('formatUriForShellDisplay')

#[test]
fn format_uri_for_shell_display_wsl() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    run_with_faked_timers(async move {
        let os = OperatingSystem::Windows;
        assert_eq!(uri_label("c:\\foo\\bar", WindowsShellType::Wsl, os, true).await, "/mnt/c/foo/bar");
        assert_eq!(uri_label("c:/foo/bar", WindowsShellType::Wsl, os, false).await, "/mnt/c/foo/bar");
    });
}

#[test]
fn format_uri_for_shell_display_git_bash() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    run_with_faked_timers(async move {
        let os = OperatingSystem::Windows;
        assert_eq!(uri_label("c:\\foo\\bar", WindowsShellType::GitBash, os, true).await, "/c/foo/bar");
        assert_eq!(uri_label("c:/foo/bar", WindowsShellType::GitBash, os, false).await, "/c/foo/bar");
    });
}

// suite('formatUriForShellDisplay') > suite('PowerShell')

#[test]
fn format_uri_for_shell_display_power_shell_windows_frontend() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    run_with_faked_timers(async move {
        let os = OperatingSystem::Windows;
        assert_eq!(uri_label("c:\\foo\\bar", GeneralShellType::PowerShell, os, true).await, "c:\\foo\\bar");
        assert_eq!(uri_label("C:\\Foo\\Bar", GeneralShellType::PowerShell, os, true).await, "C:\\Foo\\Bar");
    });
}

#[test]
fn format_uri_for_shell_display_power_shell_non_windows_frontend() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    run_with_faked_timers(async move {
        let os = OperatingSystem::Windows;
        assert_eq!(uri_label("c:/foo/bar", GeneralShellType::PowerShell, os, false).await, "c:\\foo\\bar");
        assert_eq!(uri_label("C:/Foo/Bar", GeneralShellType::PowerShell, os, false).await, "C:\\Foo\\Bar");
    });
}

// suite('formatUriForShellDisplay') > suite('Bash')

#[test]
fn format_uri_for_shell_display_bash_windows_frontend() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    run_with_faked_timers(async move {
        let os = OperatingSystem::Linux;
        assert_eq!(uri_label("\\foo\\bar", PosixShellType::Bash, os, true).await, "/foo/bar");
        assert_eq!(uri_label("/foo/bar", PosixShellType::Bash, os, true).await, "/foo/bar");
    });
}

#[test]
fn format_uri_for_shell_display_bash_non_windows_frontend() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    run_with_faked_timers(async move {
        let os = OperatingSystem::Linux;
        assert_eq!(uri_label("\\foo\\bar", PosixShellType::Bash, os, false).await, "\\foo\\bar");
        assert_eq!(uri_label("/foo/bar", PosixShellType::Bash, os, false).await, "/foo/bar");
    });
}
