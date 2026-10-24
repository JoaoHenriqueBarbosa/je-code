//! Porte de `src/vs/platform/terminal/test/node/terminalEnvironment.test.ts` (suíte `platform -
//! terminalEnvironment`), caso a caso e na mesma ordem.
//!
//! Divergências:
//! - O `ensureNoDisposablesAreLeakedInTestSuite` da suíte é o `LeakGuard` criado no começo de cada teste.
//! - O `FileAccess` global do original é um `FileAccess` montado aqui com uma raiz fixa e passado à função.
//! - `(getWindowsBuildNumberSync() < 18309 ? test.skip : test)` e o `suite.skip` equivalente viram um
//!   `return` no começo do teste, quando o número de build é menor que 18309 (sempre que o sistema não é o
//!   Windows). O Rust não tem como pular um teste em tempo de execução.
//! - As suítes `zsh` e `bash` só existem fora do Windows (`if (process.platform !== 'win32')`), então ficam
//!   atrás de `cfg(not(windows))`.
//! - O `Promise.all` do caso do diretório criado em concorrência roda as duas chamadas em sequência, porque o
//!   futuro do porte nunca cede a execução (o `fs` é o síncrono do Rust). O que o caso exercita, as duas
//!   chamadas criarem o mesmo diretório sem falhar, continua valendo: a segunda encontra o diretório da
//!   primeira.
//! - O `undefined` de `ITerminalEnvironment` e de `sanitizeEnvForLogging` é `None`; o `undefined` de um valor
//!   em `sanitizeEnvForLogging` é `None` dentro do mapa. As regexes de destino do `zsh` viram a conferência
//!   do sufixo do caminho.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use std::collections::BTreeMap;
use std::rc::Rc;

use common::{ensure_no_disposables_are_leaked_in_test_suite, run_with_faked_timers};
use je_base::arrays::OneOrMany;
use je_base::network::{FileAccess, FileAccessContext, RemoteAuthorities};
use je_base::node::windows_version::get_windows_build_number_sync;
use je_base::path::{PathModule, ProcessEnvironment as HostEnvironment};
use je_base::process::ProcessEnvironment;
use je_platform::environment::user_data_path::{home_dir, tmp_dir};
use je_platform::log::NullLogService;
use je_platform::product::ProductService;
use je_platform::terminal::node_terminal_environment::{
    get_shell_integration_injection, sanitize_env_for_logging, shell_integration_script_root, user_name, IShellIntegrationConfigInjection,
    OptionalProcessEnvironment, ShellIntegrationInjection,
};
use je_platform::terminal::terminal::{
    IShellLaunchConfig, ITerminalEnvironment, ITerminalProcessOptions, ITerminalProcessShellIntegrationOptions, TerminalEnvironmentValue,
};

fn process_options(enabled: bool) -> ITerminalProcessOptions {
    ITerminalProcessOptions {
        shell_integration: ITerminalProcessShellIntegrationOptions { enabled, suggest_enabled: false, nonce: String::new() },
        windows_use_conpty_dll: false,
        environment_variable_collections: None,
        workspace_folder: None,
        is_screen_reader_optimized: false,
    }
}

fn enabled_process_options() -> ITerminalProcessOptions {
    process_options(true)
}

fn disabled_process_options() -> ITerminalProcessOptions {
    process_options(false)
}

fn pwsh_exe() -> &'static str {
    if cfg!(windows) { "pwsh.exe" } else { "pwsh" }
}

fn file_access() -> FileAccess {
    FileAccess::new(
        Rc::new(HostEnvironment),
        FileAccessContext { is_native: true, web_worker_origin: None, file_root: Some(if cfg!(windows) { "C:\\app\\out" } else { "/app/out" }.to_string()) },
        RemoteAuthorities::new(false),
    )
}

fn script_root() -> String {
    shell_integration_script_root(&file_access())
}

/// `{ applicationName: 'vscode' } as IProductService`.
fn product_service() -> ProductService {
    ProductService { application_name: "vscode".to_owned(), quality: None, ..Default::default() }
}

fn default_environment() -> ITerminalEnvironment {
    Vec::new()
}

fn many(args: &[&str]) -> Option<OneOrMany<String>> {
    Some(OneOrMany::Many(args.iter().map(|arg| (*arg).to_owned()).collect()))
}

fn one(arg: &str) -> Option<OneOrMany<String>> {
    Some(OneOrMany::One(arg.to_owned()))
}

fn launch_config(executable: &str, args: Option<OneOrMany<String>>) -> IShellLaunchConfig {
    IShellLaunchConfig { executable: Some(executable.to_owned()), args, ..Default::default() }
}

/// `getShellIntegrationInjection(slc, options, env, logService, productService, true)`.
async fn inject(shell_launch_config: IShellLaunchConfig, options: &ITerminalProcessOptions, env: Option<&ITerminalEnvironment>) -> ShellIntegrationInjection {
    get_shell_integration_injection(&shell_launch_config, options, env, &NullLogService, &product_service(), &file_access(), true).await.unwrap()
}

/// `getShellIntegrationInjection(...)` com o ambiente padrão (`defaultEnvironment`).
async fn inject_default(shell_launch_config: IShellLaunchConfig, options: &ITerminalProcessOptions) -> ShellIntegrationInjection {
    inject(shell_launch_config, options, Some(&default_environment())).await
}

fn env_of(pairs: &[(&str, &str)]) -> ProcessEnvironment {
    pairs.iter().map(|(key, value)| ((*key).to_owned(), (*value).to_owned())).collect()
}

fn expected_injection(new_args: &[&str], env_mixin: &[(&str, &str)]) -> ShellIntegrationInjection {
    ShellIntegrationInjection::Injection(IShellIntegrationConfigInjection {
        new_args: Some(new_args.iter().map(|arg| (*arg).to_owned()).collect()),
        env_mixin: Some(env_of(env_mixin)),
        files_to_copy: None,
    })
}

fn deep_strict_equal_ignore_stable_var(mut actual: ShellIntegrationInjection, expected: &ShellIntegrationInjection) {
    if let ShellIntegrationInjection::Injection(injection) = &mut actual {
        if let Some(env_mixin) = &mut injection.env_mixin {
            env_mixin.remove("VSCODE_STABLE");
        }
    }
    assert_eq!(&actual, expected);
}

/// Este teste só é esperado no Windows 10 build 18309 ou mais novo (o `test.skip` do original).
fn windows_build_supported() -> bool {
    get_windows_build_number_sync() >= 18309
}

// suite('getShellIntegrationInjection') > suite('should not enable')

#[test]
fn should_not_enable_when_is_feature_terminal_or_when_no_executable_is_provided() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    if !windows_build_supported() {
        return;
    }
    run_with_faked_timers(async move {
        let feature = IShellLaunchConfig { is_feature_terminal: Some(true), ..launch_config(pwsh_exe(), many(&["-l", "-NoLogo"])) };
        assert_eq!(inject_default(feature, &enabled_process_options()).await.type_(), "failure");
        let not_feature = IShellLaunchConfig { is_feature_terminal: Some(false), ..launch_config(pwsh_exe(), many(&["-l", "-NoLogo"])) };
        assert_eq!(inject_default(not_feature, &enabled_process_options()).await.type_(), "injection");
    });
}

// suite('pwsh')

fn expected_ps1() -> String {
    let root = script_root();
    if cfg!(windows) { format!("try {{ . \"{root}\\shellIntegration.ps1\" }} catch {{}}") } else { format!(". \"{root}/shellIntegration.ps1\"") }
}

fn pwsh_override_expected() -> ShellIntegrationInjection {
    expected_injection(&["-noexit", "-command", &expected_ps1()], &[("VSCODE_A11Y_MODE", "0"), ("VSCODE_INJECTION", "1")])
}

fn pwsh_login_expected() -> ShellIntegrationInjection {
    expected_injection(&["-l", "-noexit", "-command", &expected_ps1()], &[("VSCODE_A11Y_MODE", "0"), ("VSCODE_INJECTION", "1")])
}

// suite('should override args')

#[test]
fn pwsh_should_override_args_when_undefined_empty_array() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    if !windows_build_supported() {
        return;
    }
    run_with_faked_timers(async move {
        let expected = pwsh_override_expected();
        deep_strict_equal_ignore_stable_var(inject_default(launch_config(pwsh_exe(), many(&[])), &enabled_process_options()).await, &expected);
        deep_strict_equal_ignore_stable_var(inject_default(launch_config(pwsh_exe(), None), &enabled_process_options()).await, &expected);
    });
}

#[test]
fn pwsh_should_override_args_when_no_logo_array_case_insensitive() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    if !windows_build_supported() {
        return;
    }
    run_with_faked_timers(async move {
        let expected = pwsh_override_expected();
        for arg in ["-NoLogo", "-NOLOGO", "-nol", "-NOL"] {
            deep_strict_equal_ignore_stable_var(inject_default(launch_config(pwsh_exe(), many(&[arg])), &enabled_process_options()).await, &expected);
        }
    });
}

#[test]
fn pwsh_should_override_args_when_no_logo_string_case_insensitive() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    if !windows_build_supported() {
        return;
    }
    run_with_faked_timers(async move {
        let expected = pwsh_override_expected();
        for arg in ["-NoLogo", "-NOLOGO", "-nol", "-NOL"] {
            deep_strict_equal_ignore_stable_var(inject_default(launch_config(pwsh_exe(), one(arg)), &enabled_process_options()).await, &expected);
        }
    });
}

// suite('should incorporate login arg')

#[test]
fn pwsh_should_incorporate_login_arg_when_array_contains_no_logo_and_login() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    if !windows_build_supported() {
        return;
    }
    run_with_faked_timers(async move {
        deep_strict_equal_ignore_stable_var(
            inject_default(launch_config(pwsh_exe(), many(&["-l", "-NoLogo"])), &enabled_process_options()).await,
            &pwsh_login_expected(),
        );
    });
}

#[test]
fn pwsh_should_incorporate_login_arg_when_string() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    if !windows_build_supported() {
        return;
    }
    run_with_faked_timers(async move {
        deep_strict_equal_ignore_stable_var(inject_default(launch_config(pwsh_exe(), one("-l")), &enabled_process_options()).await, &pwsh_login_expected());
    });
}

// suite('should not modify args')

#[test]
fn pwsh_should_not_modify_args_when_shell_integration_is_disabled() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    if !windows_build_supported() {
        return;
    }
    run_with_faked_timers(async move {
        for args in [many(&["-l"]), one("-l"), None] {
            assert_eq!(inject_default(launch_config(pwsh_exe(), args), &disabled_process_options()).await.type_(), "failure");
        }
    });
}

#[test]
fn pwsh_should_not_modify_args_when_using_unrecognized_arg() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    if !windows_build_supported() {
        return;
    }
    run_with_faked_timers(async move {
        assert_eq!(inject_default(launch_config(pwsh_exe(), many(&["-l", "-NoLogo", "-i"])), &disabled_process_options()).await.type_(), "failure");
    });
}

#[test]
fn pwsh_should_not_modify_args_when_using_unrecognized_arg_string() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    if !windows_build_supported() {
        return;
    }
    run_with_faked_timers(async move {
        assert_eq!(inject_default(launch_config(pwsh_exe(), one("-i")), &disabled_process_options()).await.type_(), "failure");
    });
}

// if (process.platform !== 'win32') { suite('zsh') ... }

#[cfg(not(windows))]
mod zsh {
    use super::*;

    const CUSTOM_ZDOTDIR: &str = "/custom/zsh/dotdir";

    fn assert_is_enabled(result: &ShellIntegrationInjection, global_zdotdir: &str) {
        let username = user_name();
        let expected_dir = format!("/{username}-vscode-zsh");
        let expected_dests = [".zshrc", ".zprofile", ".zshenv", ".zlogin"].map(|file| format!("/{username}-vscode-zsh/{file}"));
        let root = script_root();
        let expected_sources = ["shellIntegration-rc.zsh", "shellIntegration-profile.zsh", "shellIntegration-env.zsh", "shellIntegration-login.zsh"].map(|file| format!("{root}/{file}"));

        let ShellIntegrationInjection::Injection(result) = result else {
            panic!("a injeção deveria ter sido habilitada: {result:?}");
        };
        let env_mixin = result.env_mixin.as_ref().unwrap();
        assert_eq!(env_mixin.len(), 3);
        // `.+/user-vscode-zsh`: ao menos um caractere antes do sufixo
        assert!(env_mixin["ZDOTDIR"].strip_suffix(&expected_dir).is_some_and(|prefix| !prefix.is_empty()));
        assert_eq!(env_mixin["USER_ZDOTDIR"], global_zdotdir);
        assert_eq!(env_mixin["VSCODE_INJECTION"], "1");
        let files_to_copy = result.files_to_copy.as_ref().unwrap();
        assert_eq!(files_to_copy.len(), 4);
        for (index, file) in files_to_copy.iter().enumerate() {
            assert!(file.dest.strip_suffix(&expected_dests[index]).is_some_and(|prefix| !prefix.is_empty()), "{}", file.dest);
            assert_eq!(file.source, expected_sources[index]);
        }
    }

    fn new_args_of(result: &ShellIntegrationInjection) -> Option<Vec<String>> {
        match result {
            ShellIntegrationInjection::Injection(injection) => injection.new_args.clone(),
            ShellIntegrationInjection::Failure(_) => None,
        }
    }

    fn strings(args: &[&str]) -> Option<Vec<String>> {
        Some(args.iter().map(|arg| (*arg).to_owned()).collect())
    }

    /// O `finally { rmSync(zdotdir, { recursive: true, force: true }) }`.
    struct RemoveDir(String);

    impl Drop for RemoveDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    // suite('zsh') > suite('should override args')

    #[test]
    fn zsh_should_override_args_when_undefined_empty_array() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        run_with_faked_timers(async move {
            let result1 = inject_default(launch_config("zsh", many(&[])), &enabled_process_options()).await;
            assert_eq!(new_args_of(&result1), strings(&["-i"]));
            assert_is_enabled(&result1, &home_dir());
            let result2 = inject_default(launch_config("zsh", None), &enabled_process_options()).await;
            assert_eq!(new_args_of(&result2), strings(&["-i"]));
            assert_is_enabled(&result2, &home_dir());
        });
    }

    #[test]
    fn zsh_should_override_args_when_shell_integration_directory_is_created_concurrently() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        run_with_faked_timers(async move {
            let application_name = format!("vscode-zsh-test-{}", std::process::id());
            let real_tmp_dir = std::fs::canonicalize(tmp_dir()).unwrap().to_string_lossy().into_owned();
            let zdotdir = PathModule::native(&HostEnvironment).join(&[real_tmp_dir.as_str(), format!("{}-{application_name}-zsh", user_name()).as_str()]);
            let _ = std::fs::remove_dir_all(&zdotdir);
            let _cleanup = RemoveDir(zdotdir);
            let product_service = ProductService { application_name, quality: None, ..Default::default() };
            let mut types = Vec::new();
            for _ in 0..2 {
                let result = get_shell_integration_injection(
                    &launch_config("zsh", many(&[])),
                    &enabled_process_options(),
                    Some(&default_environment()),
                    &NullLogService,
                    &product_service,
                    &file_access(),
                    false,
                )
                .await
                .unwrap();
                types.push(result.type_());
            }
            assert_eq!(types, ["injection", "injection"]);
        });
    }

    // suite('should incorporate login arg')

    #[test]
    fn zsh_should_incorporate_login_arg_when_array() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        run_with_faked_timers(async move {
            let result = inject_default(launch_config("zsh", many(&["-l"])), &enabled_process_options()).await;
            assert_eq!(new_args_of(&result), strings(&["-il"]));
            assert_is_enabled(&result, &home_dir());
        });
    }

    // suite('should not modify args')

    #[test]
    fn zsh_should_not_modify_args_when_shell_integration_is_disabled() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        run_with_faked_timers(async move {
            assert_eq!(inject_default(launch_config("zsh", many(&["-l"])), &disabled_process_options()).await.type_(), "failure");
            assert_eq!(inject_default(launch_config("zsh", None), &disabled_process_options()).await.type_(), "failure");
        });
    }

    #[test]
    fn zsh_should_not_modify_args_when_using_unrecognized_arg() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        run_with_faked_timers(async move {
            assert_eq!(inject_default(launch_config("zsh", many(&["-l", "-fake"])), &disabled_process_options()).await.type_(), "failure");
        });
    }

    // suite('should incorporate global ZDOTDIR env variable')

    #[test]
    fn zsh_should_incorporate_global_zdotdir_env_variable_when_custom_zdotdir() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        run_with_faked_timers(async move {
            let environment: ITerminalEnvironment = vec![("ZDOTDIR".to_owned(), TerminalEnvironmentValue::Value(CUSTOM_ZDOTDIR.to_owned()))];
            let result1 = inject(launch_config("zsh", many(&[])), &enabled_process_options(), Some(&environment)).await;
            assert_eq!(new_args_of(&result1), strings(&["-i"]));
            assert_is_enabled(&result1, CUSTOM_ZDOTDIR);
        });
    }

    #[test]
    fn zsh_should_incorporate_global_zdotdir_env_variable_when_undefined() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        run_with_faked_timers(async move {
            let result1 = inject(launch_config("zsh", many(&[])), &enabled_process_options(), None).await;
            assert_eq!(new_args_of(&result1), strings(&["-i"]));
            assert_is_enabled(&result1, &home_dir());
        });
    }
}

// suite('bash')

#[cfg(not(windows))]
mod bash {
    use super::*;

    fn bash_args(root: &str) -> [String; 2] {
        ["--init-file".to_owned(), format!("{root}/shellIntegration-bash.sh")]
    }

    fn bash_expected(env_mixin: &[(&str, &str)]) -> ShellIntegrationInjection {
        let [flag, file] = bash_args(&script_root());
        expected_injection(&[&flag, &file], env_mixin)
    }

    // suite('forceShellIntegration')

    #[test]
    fn bash_should_inject_when_is_feature_terminal_is_true_but_force_shell_integration_overrides_it() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        run_with_faked_timers(async move {
            let slc = IShellLaunchConfig { is_feature_terminal: Some(true), force_shell_integration: Some(true), ..launch_config("bash", many(&[])) };
            assert_eq!(inject_default(slc, &enabled_process_options()).await.type_(), "injection");
        });
    }

    #[test]
    fn bash_should_not_inject_when_is_feature_terminal_is_true_and_force_shell_integration_is_false() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        run_with_faked_timers(async move {
            let slc = IShellLaunchConfig { is_feature_terminal: Some(true), force_shell_integration: Some(false), ..launch_config("bash", many(&[])) };
            assert_eq!(inject_default(slc, &enabled_process_options()).await.type_(), "failure");
        });
    }

    #[test]
    fn bash_should_not_inject_when_is_feature_terminal_is_true_and_force_shell_integration_is_not_set() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        run_with_faked_timers(async move {
            let slc = IShellLaunchConfig { is_feature_terminal: Some(true), ..launch_config("bash", many(&[])) };
            assert_eq!(inject_default(slc, &enabled_process_options()).await.type_(), "failure");
        });
    }

    // suite('should override args')

    #[test]
    fn bash_should_override_args_when_undefined_empty_string() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        run_with_faked_timers(async move {
            let expected = bash_expected(&[("VSCODE_INJECTION", "1")]);
            deep_strict_equal_ignore_stable_var(inject_default(launch_config("bash", many(&[])), &enabled_process_options()).await, &expected);
            deep_strict_equal_ignore_stable_var(inject_default(launch_config("bash", one("")), &enabled_process_options()).await, &expected);
            deep_strict_equal_ignore_stable_var(inject_default(launch_config("bash", None), &enabled_process_options()).await, &expected);
        });
    }

    // suite('should set login env variable and not modify args')

    #[test]
    fn bash_should_set_login_env_variable_and_not_modify_args_when_array() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        run_with_faked_timers(async move {
            let expected = bash_expected(&[("VSCODE_INJECTION", "1"), ("VSCODE_SHELL_LOGIN", "1")]);
            deep_strict_equal_ignore_stable_var(inject_default(launch_config("bash", many(&["-l"])), &enabled_process_options()).await, &expected);
        });
    }

    // suite('should not modify args')

    #[test]
    fn bash_should_not_modify_args_when_shell_integration_is_disabled() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        run_with_faked_timers(async move {
            assert_eq!(inject_default(launch_config("bash", many(&["-l"])), &disabled_process_options()).await.type_(), "failure");
            assert_eq!(inject_default(launch_config("bash", None), &disabled_process_options()).await.type_(), "failure");
        });
    }

    #[test]
    fn bash_should_not_modify_args_when_custom_array_entry() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        run_with_faked_timers(async move {
            assert_eq!(inject_default(launch_config("bash", many(&["-l", "-i"])), &disabled_process_options()).await.type_(), "failure");
        });
    }
}

// suite('custom shell integration nonce')

#[test]
fn custom_shell_integration_nonce_should_fail_for_unsupported_shell_but_nonce_should_still_be_available() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    run_with_faked_timers(async move {
        let mut custom_process_options = enabled_process_options();
        custom_process_options.shell_integration = ITerminalProcessShellIntegrationOptions { enabled: true, suggest_enabled: false, nonce: "custom-nonce-12345".to_owned() };

        // Test with an unsupported shell (julia)
        let result = inject_default(launch_config("julia", many(&["-i"])), &custom_process_options).await;

        // Should fail due to unsupported shell
        assert_eq!(result.type_(), "failure");

        // But the nonce should be available in the process options for the terminal process to use
        assert_eq!(custom_process_options.shell_integration.nonce, "custom-nonce-12345");
    });
}

// suite('sanitizeEnvForLogging')

fn optional_env(pairs: &[(&str, Option<&str>)]) -> OptionalProcessEnvironment {
    pairs.iter().map(|(key, value)| ((*key).to_owned(), value.map(str::to_owned))).collect::<BTreeMap<_, _>>()
}

fn defined_env(pairs: &[(&str, &str)]) -> OptionalProcessEnvironment {
    pairs.iter().map(|(key, value)| ((*key).to_owned(), Some((*value).to_owned()))).collect()
}

#[test]
fn sanitize_env_for_logging_should_return_undefined_for_undefined_input() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(sanitize_env_for_logging(None), None);
}

#[test]
fn sanitize_env_for_logging_should_return_empty_object_for_empty_input() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(sanitize_env_for_logging(Some(&defined_env(&[]))), Some(env_of(&[])));
}

#[test]
fn sanitize_env_for_logging_should_pass_through_non_sensitive_values() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let pairs = [("PATH", "/usr/bin"), ("HOME", "/home/user"), ("TERM", "xterm-256color")];
    assert_eq!(sanitize_env_for_logging(Some(&defined_env(&pairs))), Some(env_of(&pairs)));
}

#[test]
fn sanitize_env_for_logging_should_redact_sensitive_env_var_names() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let input = defined_env(&[
        ("API_KEY", "secret123"),
        ("GITHUB_TOKEN", "ghp_xxxx"),
        ("MY_SECRET", "hidden"),
        ("PASSWORD", "pass123"),
        ("AWS_ACCESS_KEY", "AKIA..."),
        ("DATABASE_PASSWORD", "dbpass"),
        ("CLIENT_SECRET", "client_secret_value"),
        ("AUTH_TOKEN", "auth_value"),
        ("PRIVATE_KEY", "private_key_value"),
    ]);
    let expected = env_of(&[
        ("API_KEY", "<REDACTED>"),
        ("GITHUB_TOKEN", "<REDACTED>"),
        ("MY_SECRET", "<REDACTED>"),
        ("PASSWORD", "<REDACTED>"),
        ("AWS_ACCESS_KEY", "<REDACTED>"),
        ("DATABASE_PASSWORD", "<REDACTED>"),
        ("CLIENT_SECRET", "<REDACTED>"),
        ("AUTH_TOKEN", "<REDACTED>"),
        ("PRIVATE_KEY", "<REDACTED>"),
    ]);
    assert_eq!(sanitize_env_for_logging(Some(&input)), Some(expected));
}

#[test]
fn sanitize_env_for_logging_should_redact_jwt_tokens_by_value_pattern() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let input = defined_env(&[(
        "SOME_VAR",
        "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJzdWIiOiIxMjM0NTY3ODkwIn0.dozjgNryP4J3jVmNHl0w5N_XgL0n3I9PlFUP0THsR8U",
    )]);
    assert_eq!(sanitize_env_for_logging(Some(&input)), Some(env_of(&[("SOME_VAR", "<REDACTED>")])));
}

#[test]
fn sanitize_env_for_logging_should_redact_github_tokens_by_value_pattern() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let input = defined_env(&[("MY_GH", "ghp_xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx")]);
    assert_eq!(sanitize_env_for_logging(Some(&input)), Some(env_of(&[("MY_GH", "<REDACTED>")])));
}

#[test]
fn sanitize_env_for_logging_should_redact_google_api_keys_by_value_pattern() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let input = defined_env(&[("GOOGLE_KEY", "AIzaSyDaGmWKa4JsXZ-HjGw7ISLn_3namBGewQe")]);
    assert_eq!(sanitize_env_for_logging(Some(&input)), Some(env_of(&[("GOOGLE_KEY", "<REDACTED>")])));
}

#[test]
fn sanitize_env_for_logging_should_redact_long_alphanumeric_strings_potential_secrets() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let input = defined_env(&[("LONG_VALUE", "abcdefghijklmnopqrstuvwxyz123456")]);
    assert_eq!(sanitize_env_for_logging(Some(&input)), Some(env_of(&[("LONG_VALUE", "<REDACTED>")])));
}

#[test]
fn sanitize_env_for_logging_should_skip_undefined_values() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let input = optional_env(&[("DEFINED", Some("value")), ("UNDEFINED", None)]);
    assert_eq!(sanitize_env_for_logging(Some(&input)), Some(env_of(&[("DEFINED", "value")])));
}
