//! Porte de `src/vs/platform/terminal/test/node/terminalProcess.test.ts` (suíte `TerminalProcess startup disposal`).
//!
//! Os casos saem na mesma ordem do original: o laço `for (const immediate of [true, false])` são os dois primeiros testes, e
//! o `(isWindows ? test.skip : test)` do processo nativo vivo é `#[cfg(unix)]`.
//!
//! Divergências:
//! - O executável padrão do original é o `process.execPath` (o Node) com `-e 'setTimeout(() => process.exit(0), 250)'`; aqui
//!   é `/bin/sh -c 'sleep 0.25'`, o mesmo processo curto de 250 ms. O `ELECTRON_RUN_AS_NODE` não existe.
//! - `class Log extends NullLogService` que conta o `trace('node-pty.IPty#spawn')` é o `CountingLog`; a subclasse que
//!   sobrescreve `_throttleKillSpawn` é `TerminalProcess::override_throttle_kill_spawn`; o `Reflect.get(terminal,
//!   '_ptyProcess')` e `'_titleInterval'` são `TerminalProcess::observe`.
//! - `cleanup` do original mata o pty, o intervalo e o monitor por reflexão se uma regressão os recriar depois do descarte;
//!   aqui o `dispose` do terminal descarta tudo isso (o monitor é registrado no próprio terminal) e o `LeakGuard` confere
//!   que nada ficou.
//! - O `finally` do original roda mesmo se a conferência falhar; aqui a limpeza vem depois da conferência (uma falha já
//!   reprova o teste).
#![cfg(not(target_arch = "wasm32"))]

mod common;

use std::cell::Cell;
use std::rc::Rc;

use common::{ensure_no_disposables_are_leaked_in_test_suite, run_with_real_timers};
use je_base::arrays::OneOrMany;
use je_base::event::Event;
use je_base::lifecycle::{DisposableStore, IDisposable};
use je_base::network::{FileAccess, FileAccessContext, RemoteAuthorities};
use je_base::path::ProcessEnvironment as HostEnvironment;
use je_base::process::ProcessEnvironment;
use je_base::promise::{DeferredPromise, Promise};
use je_platform::log::{LogArg, LogLevel, LogMessage, LogServiceRef, Logger};
use je_platform::product::ProductService;
use je_platform::terminal::terminal::{IShellLaunchConfig, ITerminalChildProcess, ITerminalLaunchOutcome, ITerminalProcessOptions};
use je_platform::terminal::terminal_process::TerminalProcess;

/// `class Log extends NullLogService`: conta os `trace('node-pty.IPty#spawn')`.
struct CountingLog {
    spawn_attempts: Rc<Cell<u32>>,
}

impl Logger for CountingLog {
    fn on_did_change_log_level(&self) -> Event<LogLevel> {
        Event::none()
    }

    fn get_level(&self) -> LogLevel {
        LogLevel::Info
    }

    fn set_level(&self, _level: LogLevel) {}

    fn trace_args(&self, message: &str, _args: &[LogArg]) {
        if message == "node-pty.IPty#spawn" {
            self.spawn_attempts.set(self.spawn_attempts.get() + 1);
        }
    }

    fn debug_args(&self, _message: &str, _args: &[LogArg]) {}
    fn info_args(&self, _message: &str, _args: &[LogArg]) {}
    fn warn_args(&self, _message: &LogMessage, _args: &[LogArg]) {}
    fn error_args(&self, _message: &LogMessage, _args: &[LogArg]) {}
    fn flush(&self) {}
}

impl IDisposable for CountingLog {
    fn dispose(&self) {}
}

/// `{ spawnAttempts, nativePtyPresent, titlePolling }`.
#[derive(Debug, PartialEq, Eq)]
struct Observed {
    spawn_attempts: u32,
    native_pty_present: bool,
    title_polling: bool,
}

const NOT_SPAWNED: Observed = Observed { spawn_attempts: 0, native_pty_present: false, title_polling: false };

struct Fixture {
    terminal: Rc<TerminalProcess>,
    spawn_attempts: Rc<Cell<u32>>,
}

impl Fixture {
    fn observe(&self) -> Observed {
        let observation = self.terminal.observe();
        Observed { spawn_attempts: self.spawn_attempts.get(), native_pty_present: observation.native_pty_present, title_polling: observation.title_polling }
    }

    fn cleanup(&self) {
        self.terminal.dispose();
    }
}

type ThrottleHook = Rc<dyn Fn() -> Promise<()>>;

/// O `/bin/sh -c 'sleep 0.25'` do `shellLaunchConfig` padrão.
fn default_shell_launch_config() -> IShellLaunchConfig {
    IShellLaunchConfig {
        executable: Some("/bin/sh".to_owned()),
        args: Some(OneOrMany::Many(vec!["-c".to_owned(), "sleep 0.25".to_owned()])),
        ..IShellLaunchConfig::default()
    }
}

fn current_dir() -> String {
    std::env::current_dir().expect("o diretório atual").to_string_lossy().into_owned()
}

/// `createTerminal(shellLaunchConfig?, cwd = process.cwd(), throttle?)`.
fn create_terminal(store: &Rc<DisposableStore>, shell_launch_config: Option<IShellLaunchConfig>, cwd: Option<String>, throttle: Option<ThrottleHook>) -> Fixture {
    let spawn_attempts = Rc::new(Cell::new(0));
    let log: LogServiceRef = Rc::new(CountingLog { spawn_attempts: spawn_attempts.clone() });
    let env: ProcessEnvironment = std::env::vars().collect();
    let product = Rc::new(ProductService { application_name: "vscode".to_owned(), ..ProductService::default() });
    let file_access = Rc::new(FileAccess::new(Rc::new(HostEnvironment), FileAccessContext::default(), RemoteAuthorities::new(false)));
    let terminal = store.add_rc(TerminalProcess::new(
        shell_launch_config.unwrap_or_else(default_shell_launch_config),
        &cwd.unwrap_or_else(current_dir),
        80,
        24,
        env.clone(),
        env,
        ITerminalProcessOptions::default(),
        log,
        product,
        file_access,
    ));
    if let Some(throttle) = throttle {
        terminal.override_throttle_kill_spawn(throttle);
    }
    Fixture { terminal, spawn_attempts }
}

/// `shutdown(terminal, immediate)`: espera o `onProcessExit` do `terminal.shutdown(immediate)`.
fn shutdown(store: &Rc<DisposableStore>, terminal: &Rc<TerminalProcess>, immediate: bool) -> Promise<()> {
    let exited = Promise::with_executor({
        let (store, terminal) = (store.clone(), terminal.clone());
        move |resolver| {
            store.add(terminal.on_process_exit().once().listen(move |_| resolver.resolve(())));
        }
    });
    terminal.shutdown(immediate);
    exited
}

async fn does_not_spawn_after_shutdown_during_asynchronous_validation(immediate: bool) {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let store = guard.store();
    let throttle: Rc<DeferredPromise<()>> = Rc::new(DeferredPromise::new());
    let hook: ThrottleHook = {
        let throttle = throttle.clone();
        Rc::new(move || throttle.promise.clone())
    };
    let fixture = create_terminal(&store, None, None, Some(hook));
    let startup = fixture.terminal.start();
    shutdown(&store, &fixture.terminal, immediate).await.unwrap();
    drop(throttle.complete(()));
    startup.await.unwrap();
    assert_eq!(fixture.observe(), NOT_SPAWNED);
    drop(throttle.complete(()));
    fixture.cleanup();
}

#[test]
fn does_not_spawn_after_shutdown_during_asynchronous_validation_immediate_true() {
    run_with_real_timers(does_not_spawn_after_shutdown_during_asynchronous_validation(true));
}

#[test]
fn does_not_spawn_after_shutdown_during_asynchronous_validation_immediate_false() {
    run_with_real_timers(does_not_spawn_after_shutdown_during_asynchronous_validation(false));
}

#[test]
fn does_not_spawn_when_started_after_shutdown() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let store = guard.store();
    run_with_real_timers(async move {
        let fixture = create_terminal(&store, None, None, None);
        shutdown(&store, &fixture.terminal, true).await.unwrap();
        fixture.terminal.start().await.unwrap();
        assert_eq!(fixture.observe(), NOT_SPAWNED);
        fixture.cleanup();
    });
}

#[test]
fn does_not_spawn_after_disposal_during_asynchronous_validation() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let store = guard.store();
    run_with_real_timers(async move {
        let fixture = create_terminal(&store, None, None, None);
        let startup = fixture.terminal.start();
        fixture.terminal.dispose();
        startup.await.unwrap();
        assert_eq!(fixture.observe(), NOT_SPAWNED);
        fixture.cleanup();
    });
}

#[test]
fn does_not_spawn_after_disposal_while_spawn_throttling_is_pending() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let store = guard.store();
    run_with_real_timers(async move {
        let started: Rc<DeferredPromise<()>> = Rc::new(DeferredPromise::new());
        let throttle: Rc<DeferredPromise<()>> = Rc::new(DeferredPromise::new());
        let hook: ThrottleHook = {
            let (started, throttle) = (started.clone(), throttle.clone());
            Rc::new(move || {
                drop(started.complete(()));
                throttle.promise.clone()
            })
        };
        let fixture = create_terminal(&store, None, None, Some(hook));
        let startup = fixture.terminal.start();
        started.promise.clone().await.unwrap();
        fixture.terminal.dispose();
        drop(throttle.complete(()));
        startup.await.unwrap();
        assert_eq!(fixture.observe(), NOT_SPAWNED);
        drop(throttle.complete(()));
        fixture.cleanup();
    });
}

#[cfg(unix)]
#[test]
fn starts_a_live_native_process_and_cleans_up_on_normal_shutdown() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let store = guard.store();
    run_with_real_timers(async move {
        let fixture = create_terminal(&store, None, None, None);
        let pid: Rc<Cell<Option<i32>>> = Rc::default();
        store.add(fixture.terminal.on_process_ready().listen({
            let pid = pid.clone();
            move |event| pid.set(Some(event.pid))
        }));
        assert!(fixture.terminal.start().await.unwrap().is_none());
        assert!(pid.get().is_some_and(|pid| pid > 0));
        shutdown(&store, &fixture.terminal, true).await.unwrap();
        assert_eq!(fixture.observe(), Observed { spawn_attempts: 1, native_pty_present: false, title_polling: false });
        fixture.cleanup();
    });
}

#[test]
fn preserves_invalid_executable_launch_errors_without_spawning() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let store = guard.store();
    run_with_real_timers(async move {
        let shell_launch_config = IShellLaunchConfig { executable: Some("/nonexistent/terminal-startup-disposal-test".to_owned()), ..IShellLaunchConfig::default() };
        let fixture = create_terminal(&store, Some(shell_launch_config), None, None);
        let result = fixture.terminal.start().await.unwrap();
        assert!(matches!(result, Some(ITerminalLaunchOutcome::Error(error)) if !error.message.is_empty()));
        assert_eq!(fixture.observe(), NOT_SPAWNED);
        fixture.cleanup();
    });
}

#[test]
fn preserves_invalid_cwd_launch_errors_without_spawning() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let store = guard.store();
    run_with_real_timers(async move {
        let fixture = create_terminal(&store, None, Some("/nonexistent/terminal-startup-disposal-test".to_owned()), None);
        let result = fixture.terminal.start().await.unwrap();
        assert!(matches!(result, Some(ITerminalLaunchOutcome::Error(error)) if !error.message.is_empty()));
        assert_eq!(fixture.observe(), NOT_SPAWNED);
        fixture.cleanup();
    });
}
