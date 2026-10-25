//! Porte de `src/vs/platform/shell/node/shellEnv.ts`: `getResolvedShellEnv`, que resolve o ambiente do shell do
//! usuário subindo um shell de login e lendo o ambiente dele.
//!
//! Divergências:
//! - O original roda `process.execPath -p 'JSON.stringify(process.env)'` (o executável do aplicativo funcionando
//!   como Node, com `ELECTRON_RUN_AS_NODE`). O porte é um binário Rust sem modo `-p`: o shell imprime o ambiente
//!   com `env -0` (PowerShell e xonsh, que não têm o `env`, imprimem o próprio ambiente), entre as mesmas marcas
//!   de 12 caracteres, e a saída é lida como pares `NOME=valor` separados por NUL. O resto (o shell por nome, os
//!   argumentos `-i -l -c`, `-Login -Command`, `-ic`, as variáveis `ELECTRON_*` e `VSCODE_RESOLVING_ENVIRONMENT` no
//!   ambiente do filho e a limpeza delas no resultado) é o do original.
//! - `Promises.withAsyncBody` com o `CancellationTokenSource` é uma thread que espera o filho e uma bandeira
//!   atômica de cancelamento: o timeout de `MAX_SHELL_RESOLVE_TIME` (configurável) liga a bandeira, a thread mata o
//!   filho e a promessa é rejeitada com a mesma mensagem. A conferência da bandeira e da promessa é um timer de 25 ms
//!   no laço de eventos.
//! - `unixShellEnvPromise` (variável de módulo) é um `thread_local`.
//! - O `NativeParsedArgs` é lido com `get_bool` (`args['force-disable-user-env']`).

use std::cell::RefCell;
use std::io::Read;
use std::process::{Command, ExitStatus, Stdio};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use je_base::dynamic::Dynamic;
use je_base::errors::Error;
use je_base::nls::localize;
use je_base::path::{FixedEnvironment, PathModule, Platform as PathPlatform, ProcessEnvironment as HostEnvironment};
use je_base::platform::Platform;
use je_base::process::ProcessEnvironment;
use je_base::promise::{Promise, Resolver};
use je_base::runtime;
use je_base::uuid::generate_uuid;

use crate::configuration::service::IConfigurationService;
use crate::environment::argv_helper::is_launched_from_cli;
use crate::environment::native_parsed_args::NativeParsedArgs;
use crate::log::{LogArg, LogServiceRef};

thread_local! {
    static UNIX_SHELL_ENV_PROMISE: RefCell<Option<Promise<ProcessEnvironment>>> = const { RefCell::new(None) };
}

type SpawnOutcome = Result<ProcessEnvironment, Error>;

/// Resolves the shell environment by spawning a shell. This call will cache
/// the shell spawning so that subsequent invocations use that cached result.
///
/// Will throw an error if:
/// - we hit a timeout of `MAX_SHELL_RESOLVE_TIME`
/// - any other error from spawning a shell to figure out the environment
pub fn get_resolved_shell_env(
    configuration_service: &Rc<dyn IConfigurationService>,
    log_service: &LogServiceRef,
    args: &NativeParsedArgs,
    env: &ProcessEnvironment,
) -> Promise<ProcessEnvironment> {
    // Skip if --force-disable-user-env
    if args.get_bool("force-disable-user-env") {
        log_service.trace("resolveShellEnv(): skipped (--force-disable-user-env)");
        return Promise::resolved(ProcessEnvironment::new());
    }

    // Skip on windows
    if Platform::detect().is_windows {
        log_service.trace("resolveShellEnv(): skipped (Windows)");
        return Promise::resolved(ProcessEnvironment::new());
    }

    // Skip if running from CLI already
    let mut fixed = FixedEnvironment::new(PathPlatform::Posix, "");
    fixed.vars = env.iter().map(|(key, value)| (key.clone(), value.clone())).collect();
    let launched_from_cli = is_launched_from_cli(&fixed);
    if launched_from_cli && !args.get_bool("force-user-env") {
        log_service.trace("resolveShellEnv(): skipped (VSCODE_CLI is set)");
        return Promise::resolved(ProcessEnvironment::new());
    }

    // Otherwise resolve (macOS, Linux)
    if launched_from_cli {
        log_service.trace("resolveShellEnv(): running (--force-user-env)");
    } else {
        log_service.trace("resolveShellEnv(): running (macOS/Linux)");
    }

    // Call this only once and cache the promise for
    // subsequent calls since this operation can be
    // expensive (spawns a process).
    if let Some(promise) = UNIX_SHELL_ENV_PROMISE.with(|cached| cached.borrow().clone()) {
        return promise;
    }

    let mut timeout_value: u64 = 10000; // default to 10 seconds
    if let Dynamic::Number(configured) = configuration_service.get_value(Some("application.shellEnvironmentResolutionTimeout"), None) {
        timeout_value = (configured.clamp(1.0, 120.0) * 1000.0) as u64; // convert from seconds
    }

    let (promise, resolver) = Promise::new();
    UNIX_SHELL_ENV_PROMISE.with(|cached| *cached.borrow_mut() = Some(promise.clone()));

    let cancelled = Arc::new(AtomicBool::new(false));
    let outcome: Arc<Mutex<Option<SpawnOutcome>>> = Arc::new(Mutex::new(None));
    let started = runtime::now();
    start_resolution(log_service.clone(), env.clone(), cancelled.clone(), outcome.clone());
    watch_resolution(resolver, cancelled, outcome, started, timeout_value);
    promise
}

/// O `timeout` + o `try/catch/finally` do corpo do `withAsyncBody`: confere a thread a cada 25 ms.
fn watch_resolution(resolver: Resolver<ProcessEnvironment>, cancelled: Arc<AtomicBool>, outcome: Arc<Mutex<Option<SpawnOutcome>>>, started: u64, timeout_value: u64) {
    runtime::set_timeout(
        move || {
            let finished = outcome.lock().unwrap_or_else(std::sync::PoisonError::into_inner).take();
            match finished {
                Some(Ok(env)) => resolver.resolve(env),
                Some(Err(error)) => resolver.reject(error),
                None => {
                    if runtime::now().saturating_sub(started) >= timeout_value {
                        // Give up resolving shell env after some time
                        cancelled.store(true, Ordering::SeqCst);
                        resolver.reject(Error::new(localize(
                            "resolveShellEnvTimeout",
                            "Unable to resolve your shell environment in a reasonable time. Please review your shell configuration and restart.",
                            &[],
                        )));
                    } else {
                        watch_resolution(resolver, cancelled, outcome, started, timeout_value);
                    }
                }
            }
        },
        25,
    );
}

fn start_resolution(log_service: LogServiceRef, process_env: ProcessEnvironment, cancelled: Arc<AtomicBool>, outcome: Arc<Mutex<Option<SpawnOutcome>>>) {
    let run_as_node = process_env.get("ELECTRON_RUN_AS_NODE").cloned();
    log_service.trace_args("getUnixShellEnvironment#runAsNode", &[LogArg::Value(run_as_node.as_deref().map_or(Dynamic::Undefined, Dynamic::string))]);
    let no_attach = process_env.get("ELECTRON_NO_ATTACH_CONSOLE").cloned();
    log_service.trace_args("getUnixShellEnvironment#noAttach", &[LogArg::Value(no_attach.as_deref().map_or(Dynamic::Undefined, Dynamic::string))]);

    let mark: String = generate_uuid().replace('-', "").chars().take(12).collect();

    let mut env = process_env.clone();
    env.insert("ELECTRON_RUN_AS_NODE".to_owned(), "1".to_owned());
    env.insert("ELECTRON_NO_ATTACH_CONSOLE".to_owned(), "1".to_owned());
    env.insert("VSCODE_RESOLVING_ENVIRONMENT".to_owned(), "1".to_owned());

    let platform = Platform::detect();
    let shell_log = log_service.clone();
    // `getSystemShell` é `async`: roda até o fim aqui, porque no Unix ele não espera nada.
    let system_shell = je_base::async_::async_fn({
        let env = env.clone();
        async move { Ok(je_base::node::shell::get_system_shell(platform.os, &env).await) }
    });
    let system_shell = match system_shell.result() {
        Some(Ok(shell)) => shell,
        _ => String::new(),
    };
    shell_log.trace_args("getUnixShellEnvironment#shell", &[LogArg::from(system_shell.as_str())]);

    // handle popular non-POSIX shells
    let host = HostEnvironment;
    let name = PathModule::native(&host).basename(&system_shell, None);
    let (command, shell_args): (String, Vec<&str>) = if matches!(name.as_str(), "pwsh" | "powershell" | "pwsh-preview" | "powershell-preview") {
        // Older versions of PowerShell removes double quotes sometimes so we use "double single quotes" which is how
        // you escape single quotes inside of a single quoted string.
        (
            format!("Write-Host -NoNewline '{mark}'; [Environment]::GetEnvironmentVariables().GetEnumerator() | ForEach-Object {{ Write-Host -NoNewline ($_.Key + '=' + $_.Value + [char]0) }}; Write-Host -NoNewline '{mark}'"),
            vec!["-Login", "-Command"],
        )
    } else if name == "nu" {
        // nushell requires ^ before quoted path to treat it as a command
        (format!("^printf '%s' '{mark}'; ^env -0; ^printf '%s' '{mark}'"), vec!["-i", "-l", "-c"])
    } else if name == "xonsh" {
        // #200374: native implementation is shorter
        (format!("import os; print('{mark}' + chr(0).join(k + '=' + v for k, v in os.environ.items()) + '{mark}', end='')"), vec!["-i", "-l", "-c"])
    } else {
        let command = format!("printf '%s' '{mark}'; env -0; printf '%s' '{mark}'");
        if name == "tcsh" || name == "csh" { (command, vec!["-ic"]) } else { (command, vec!["-i", "-l", "-c"]) }
    };

    shell_log.trace_args(
        "getUnixShellEnvironment#spawn",
        &[LogArg::from(format!("[{}]", shell_args.iter().map(|arg| format!("\"{arg}\"")).collect::<Vec<_>>().join(",")).as_str()), LogArg::from(command.as_str())],
    );

    let mut spawn_args: Vec<String> = shell_args.iter().map(|arg| (*arg).to_owned()).collect();
    spawn_args.push(command);

    std::thread::spawn(move || {
        let result = spawn_and_read(&system_shell, &spawn_args, &env, &cancelled).and_then(|(stdout, status)| {
            // `getUnixShellEnvironment#raw` e `#stderr` são só de depuração e não saem da thread.
            if !status.success() {
                let code = status.code().map_or_else(|| "null".to_owned(), |code| code.to_string());
                #[cfg(unix)]
                let signal = std::os::unix::process::ExitStatusExt::signal(&status).map_or_else(|| "null".to_owned(), |signal| signal.to_string());
                #[cfg(not(unix))]
                let signal = "null".to_owned();
                return Err(Error::new(localize("resolveShellEnvExitError", "Unexpected exit code from spawned shell (code {0}, signal {1})", &[&code, &signal])));
            }
            Ok(parse_environment(&stdout, &mark, run_as_node.as_deref(), no_attach.as_deref()))
        });
        let result = if cancelled.load(Ordering::SeqCst) { Ok(ProcessEnvironment::new()) } else { result };
        *outcome.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = Some(result);
    });
}

/// `spawn(systemShellUnix, [...shellArgs, command], { detached: true, stdio: ['ignore', 'pipe', 'pipe'], env })`: devolve a
/// saída padrão e o estado de saída; a bandeira `cancelled` mata o filho.
fn spawn_and_read(shell: &str, args: &[String], env: &ProcessEnvironment, cancelled: &AtomicBool) -> Result<(String, ExitStatus), Error> {
    let mut command = Command::new(shell);
    command.args(args).env_clear().envs(env).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = command.spawn().map_err(|error| Error::new(error.to_string()))?;

    fn read_all(pipe: Option<impl Read + Send + 'static>) -> std::thread::JoinHandle<String> {
        std::thread::spawn(move || {
            let mut buffer = Vec::new();
            if let Some(mut pipe) = pipe {
                let _ = pipe.read_to_end(&mut buffer);
            }
            String::from_utf8_lossy(&buffer).into_owned()
        })
    }
    let stdout = read_all(child.stdout.take());
    let stderr = read_all(child.stderr.take());

    let status = loop {
        if cancelled.load(Ordering::SeqCst) {
            let _ = child.kill();
            break child.wait();
        }
        match child.try_wait() {
            Ok(Some(status)) => break Ok(status),
            Ok(None) => std::thread::sleep(Duration::from_millis(10)),
            Err(error) => break Err(error),
        }
    };
    let status = status.map_err(|error| Error::new(error.to_string()))?;
    let _ = stderr.join();
    Ok((stdout.join().unwrap_or_default(), status))
}

/// O trecho `try { JSON.parse(rawStripped) ... }`: o texto entre as duas marcas (`{}` se não houver), a restauração das
/// variáveis `ELECTRON_*` e a limpeza das que o original apaga.
fn parse_environment(raw: &str, mark: &str, run_as_node: Option<&str>, no_attach: Option<&str>) -> ProcessEnvironment {
    let stripped = raw.find(mark).and_then(|start| {
        let rest = &raw[start + mark.len()..];
        rest.find(mark).map(|end| &rest[..end])
    });
    let mut env: ProcessEnvironment = stripped
        .unwrap_or_default()
        .split('\0')
        .filter_map(|pair| pair.split_once('=').map(|(key, value)| (key.to_owned(), value.to_owned())))
        .filter(|(key, _)| !key.is_empty())
        .collect();

    match run_as_node {
        Some(value) => env.insert("ELECTRON_RUN_AS_NODE".to_owned(), value.to_owned()),
        None => env.remove("ELECTRON_RUN_AS_NODE"),
    };
    match no_attach {
        Some(value) => env.insert("ELECTRON_NO_ATTACH_CONSOLE".to_owned(), value.to_owned()),
        None => env.remove("ELECTRON_NO_ATTACH_CONSOLE"),
    };
    env.remove("VSCODE_RESOLVING_ENVIRONMENT");

    // https://github.com/microsoft/vscode/issues/22593#issuecomment-336050758
    env.remove("XDG_RUNTIME_DIR");
    env
}
