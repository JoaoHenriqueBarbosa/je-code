//! Porte de `src/vs/platform/remoteTunnel/test/node/codeTunnelCliProcess.test.ts`.
//!
//! Os 6 casos saem um a um e na mesma ordem do original, com os mesmos esperados literais.
//!
//! Divergências:
//! - O processo filho do original é um `EventEmitter` com `PassThrough` em `stdout` e `stderr`; aqui é o `TestChild`
//!   (`tests/common/remote_tunnel.rs`), que escreve nos ouvintes que a CLI registrou. `sinon.spy()` no `kill` é a contagem
//!   de chamadas do `TestChild` (que, como o spy, não faz o processo sair).
//! - `ensureNoDisposablesAreLeakedInTestSuite()` é o `LeakGuard` criado no começo de cada caso; os casos assíncronos
//!   rodam em `run_with_faked_timers` (o `await` de uma promessa pendente do original é resolvido pelo laço).
//! - `assert.rejects(promise, validação)` é o erro da promessa rejeitada conferido caso a caso.
#![cfg(not(target_arch = "wasm32"))]

mod common;
#[path = "common/remote_tunnel.rs"]
mod remote_tunnel_support;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use common::{ensure_no_disposables_are_leaked_in_test_suite, join, run_with_faked_timers};
use je_base::errors::Error;
use je_platform::environment::user_data_path::home_dir;
use je_platform::remote_tunnel::code_tunnel_cli_process::{
    CodeTunnelCli, CodeTunnelCliOptions, CodeTunnelCliOutput, CodeTunnelLog, CodeTunnelSpawn, SpawnOptions, TunnelChildHandlers, TunnelChildProcess, TunnelPlatform,
    resolve_tunnel_command_location,
};
use remote_tunnel_support::TestChild;

struct SpawnCall {
    command: String,
    args: Vec<String>,
    options: SpawnOptions,
    process: Rc<TestChild>,
}

type SpawnCalls = Rc<RefCell<Vec<SpawnCall>>>;

fn args(items: &[&str]) -> Vec<String> {
    items.iter().map(|item| (*item).to_owned()).collect()
}

fn ignore_output() -> CodeTunnelCliOutput {
    Rc::new(|_: &str, _: bool| {})
}

fn create_test_cli(is_built: bool, app_root: &str, on_log: Option<CodeTunnelLog>) -> (CodeTunnelCli, SpawnCalls) {
    let spawn_calls: SpawnCalls = Rc::default();
    let spawn: CodeTunnelSpawn = {
        let spawn_calls = spawn_calls.clone();
        Rc::new(move |command: &str, args: &[String], options: &SpawnOptions, handlers: TunnelChildHandlers| -> Result<Rc<dyn TunnelChildProcess>, Error> {
            let process = TestChild::new(handlers, 123, false);
            spawn_calls.borrow_mut().push(SpawnCall { command: command.to_owned(), args: args.to_vec(), options: options.clone(), process: process.clone() });
            Ok(process)
        })
    };
    let cli = CodeTunnelCli::new(CodeTunnelCliOptions {
        app_root: app_root.to_owned(),
        is_built,
        tunnel_application_name: Some("code-tunnel".to_owned()),
        win32_versioned_update: false,
        spawn: Some(spawn),
        on_log,
    });
    (cli, spawn_calls)
}

/// `spawnCalls[0].process`.
fn first_process(spawn_calls: &SpawnCalls) -> Rc<TestChild> {
    spawn_calls.borrow()[0].process.clone()
}

/// `{ command, args, cwd }` de `spawnCalls[0]`.
fn first_call(spawn_calls: &SpawnCalls) -> (String, Vec<String>, Option<String>) {
    let calls = spawn_calls.borrow();
    (calls[0].command.clone(), calls[0].args.clone(), calls[0].options.cwd.clone())
}

#[test]
fn resolves_command_locations_for_supported_installation_layouts() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let mac_app_root = join(&["installation", "mac", "Contents", "Resources", "app"]);
    let windows_app_root = join(&["installation", "windows", "resources", "app"]);
    let versioned_windows_app_root = join(&["installation", "versioned-windows", "1.0.0", "resources", "app"]);
    let linux_app_root = join(&["installation", "linux", "resources", "app"]);

    assert_eq!(
        vec![
            resolve_tunnel_command_location(&mac_app_root, TunnelPlatform::Darwin, Some("code-tunnel"), false),
            resolve_tunnel_command_location(&windows_app_root, TunnelPlatform::Win32, Some("code-tunnel"), false),
            resolve_tunnel_command_location(&versioned_windows_app_root, TunnelPlatform::Win32, Some("code-tunnel"), true),
            resolve_tunnel_command_location(&linux_app_root, TunnelPlatform::Linux, Some("code-tunnel"), false),
        ],
        vec![
            join(&[mac_app_root.as_str(), "bin", "code-tunnel"]),
            join(&["installation", "windows", "bin", "code-tunnel.exe"]),
            join(&["installation", "versioned-windows", "bin", "code-tunnel.exe"]),
            join(&["installation", "linux", "bin", "code-tunnel"]),
        ]
    );
}

#[test]
fn uses_built_and_source_cli_invocation_locations() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    run_with_faked_timers(async move {
        let app_root = join(&["installation", "resources", "app"]);
        let (built_cli, built_calls) = create_test_cli(true, &app_root, None);
        let (source_cli, source_calls) = create_test_cli(false, &app_root, None);
        let built_run = built_cli.run("built", &args(&["tunnel", "--name", "host"]), ignore_output(), None);
        let source_run = source_cli.run("source", &args(&["tunnel", "--name", "host"]), ignore_output(), None);

        first_process(&built_calls).emit_exit(Some(0));
        first_process(&source_calls).emit_exit(Some(0));
        built_run.result.promise.clone().await.unwrap();
        source_run.result.promise.clone().await.unwrap();

        assert_eq!(
            vec![first_call(&built_calls), first_call(&source_calls)],
            vec![
                (built_cli.command_location(), args(&["tunnel", "--name", "host"]), Some(home_dir())),
                ("cargo".to_owned(), args(&["run", "--", "tunnel", "--name", "host"]), Some(join(&[app_root.as_str(), "cli"]))),
            ]
        );
    });
}

#[test]
fn splits_standard_output_and_error_output_into_lines() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    run_with_faked_timers(async move {
        let (cli, spawn_calls) = create_test_cli(true, &join(&["installation", "resources", "app"]), None);
        let output: Rc<RefCell<Vec<(String, bool)>>> = Rc::default();
        let on_output: CodeTunnelCliOutput = {
            let output = output.clone();
            Rc::new(move |message: &str, is_error: bool| output.borrow_mut().push((message.to_owned(), is_error)))
        };
        let run = cli.run("serve", &args(&["tunnel", "--name", "host"]), on_output, None);
        let process = first_process(&spawn_calls);

        process.write_stdout("standard one\nstandard two\n");
        process.write_stderr("error one\nerror two\n");
        process.emit_exit(Some(7));

        let result = run.result.promise.clone().await.unwrap();
        let output = output.borrow().clone();
        let line = |message: &str, is_error: bool| (message.to_owned(), is_error);
        assert_eq!(
            (result, output),
            (
                7,
                vec![
                    line("Running tunnel CLI\n", false),
                    line(&format!("serve Spawning: {} tunnel --name host\n", cli.command_location()), false),
                    line("standard one\n", false),
                    line("standard two\n", false),
                    line("error one\n", true),
                    line("error two\n", true),
                    line("serve exit(123): + 7 ", false),
                ]
            )
        );
    });
}

#[test]
fn rejects_with_the_underlying_spawn_error() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    run_with_faked_timers(async move {
        let (cli, spawn_calls) = create_test_cli(true, &join(&["installation", "resources", "app"]), None);
        let run = cli.run("serve", &args(&["tunnel", "--name", "host"]), ignore_output(), None);
        let spawn_error = Error::new("spawn code-tunnel ENOENT");

        first_process(&spawn_calls).emit_error(spawn_error.clone());

        // An undefined rejection loses the actionable cause, such as a missing
        // or non-executable tunnel binary.
        assert_eq!(run.result.promise.clone().await, Err(spawn_error));
    });
}

#[test]
fn kills_the_cli_process_when_cancelled() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    run_with_faked_timers(async move {
        let logs: Rc<RefCell<Vec<String>>> = Rc::default();
        let on_log: CodeTunnelLog = {
            let logs = logs.clone();
            Rc::new(move |message: &str| logs.borrow_mut().push(message.to_owned()))
        };
        let (cli, spawn_calls) = create_test_cli(true, &join(&["installation", "resources", "app"]), Some(on_log));
        let run = cli.run("serve", &args(&["tunnel"]), ignore_output(), None);

        run.result.cancel();
        assert!(run.result.promise.clone().await.is_err());
        first_process(&spawn_calls).emit_exit(None);

        let process = first_process(&spawn_calls);
        let logs = logs.borrow().clone();
        assert_eq!((process.kill_calls(), logs), (1, vec!["serve terminating(123)".to_owned()]));
    });
}

#[test]
fn waits_for_actual_process_exit_when_stopped() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    run_with_faked_timers(async move {
        let (cli, spawn_calls) = create_test_cli(true, &join(&["installation", "resources", "app"]), None);
        let run = cli.run("serve", &args(&["tunnel"]), ignore_output(), None);
        let stopped = Rc::new(Cell::new(false));
        let stop = {
            let stopped = stopped.clone();
            run.stop().then(move |()| stopped.set(true))
        };

        assert_eq!((first_process(&spawn_calls).kill_calls(), stopped.get()), (1, false));
        first_process(&spawn_calls).emit_exit(None);
        stop.await.unwrap();
        assert!(stopped.get());
    });
}
