//! Porte de `src/vs/platform/externalTerminal/test/node/externalTerminalService.test.ts` (suíte `ExternalTerminalService`),
//! caso a caso e na mesma ordem.
//!
//! Divergências: o `mockSpawner` do original (`{ spawn: (command, args, opts) => { strictEqual(...); done(); return {
//! on: (evt) => evt }; } }`) é o [`MockSpawner`], que implementa o `Spawner` do porte e só grava a chamada (o filho que
//! devolve ignora os tratadores, como o `on: (evt) => evt`); cada teste confere a chamada depois, e o `done()` é a
//! conferência de que houve exatamente uma. O `Linux` chama o `spawn` dentro de um `then`, então o teste esvazia as
//! microtarefas antes de conferir. O original muta o `mockConfig` compartilhado (`windowsExec = undefined`, `linuxExec =
//! undefined`) e deixa o teste seguinte herdar isso; aqui cada teste monta a sua configuração, com `None` onde o caso
//! pede "usa o terminal padrão".

mod common;

use std::cell::RefCell;
use std::rc::Rc;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::errors::Error;
use je_base::runtime;
use je_platform::external_terminal::{
    ChildProcess, DEFAULT_TERMINAL_OSX, ExplorerKind, ExternalTerminalConfigurationTerminal, IExternalTerminalConfiguration, IExternalTerminalSettings,
    LinuxExternalTerminalService, MacExternalTerminalService, SpawnOptions, Spawner, WindowsExternalTerminalService,
};

fn mock_config() -> IExternalTerminalConfiguration {
    IExternalTerminalConfiguration {
        terminal: ExternalTerminalConfigurationTerminal {
            explorer_kind: ExplorerKind::External,
            external: IExternalTerminalSettings {
                windows_exec: Some("testWindowsShell".to_owned()),
                osx_exec: Some("testOSXShell".to_owned()),
                linux_exec: Some("testLinuxShell".to_owned()),
            },
        },
    }
}

/// O filho do `mockSpawner`: `{ on: (evt) => evt }`.
struct NullChild;

impl ChildProcess for NullChild {
    fn on_error(&self, _handler: Box<dyn FnOnce(Error)>) {}
    fn on_exit(&self, _handler: Box<dyn FnOnce(Option<i32>)>) {}
    fn on_stderr_data(&self, _handler: Box<dyn FnMut(&str)>) {}
}

#[derive(Clone, Debug)]
struct SpawnCall {
    command: String,
    args: Option<Vec<String>>,
    options: Option<SpawnOptions>,
}

/// O `mockSpawner`: grava os argumentos de cada `spawn`.
#[derive(Default)]
struct MockSpawner {
    calls: RefCell<Vec<SpawnCall>>,
}

impl Spawner for MockSpawner {
    fn spawn(&self, command: &str, args: Option<&[String]>, options: Option<&SpawnOptions>) -> Rc<dyn ChildProcess> {
        self.calls.borrow_mut().push(SpawnCall { command: command.to_owned(), args: args.map(<[String]>::to_vec), options: options.cloned() });
        Rc::new(NullChild)
    }
}

impl MockSpawner {
    /// O `done()`: houve exatamente um `spawn`.
    fn only_call(&self) -> SpawnCall {
        let calls = self.calls.borrow();
        assert_eq!(calls.len(), 1, "esperava um spawn, houve {}", calls.len());
        calls[0].clone()
    }
}

#[test]
fn win_terminal_service_uses_terminal_from_configuration() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let test_shell = "cmd";
    let test_cwd = "path/to/workspace";
    let config = mock_config();
    let spawner = Rc::new(MockSpawner::default());
    let test_service = WindowsExternalTerminalService::new();
    test_service.spawn_terminal(spawner.clone(), &config.terminal.external, test_shell, Some(test_cwd));
    let call = spawner.only_call();
    assert_eq!(call.command, test_shell, "shell should equal expected");
    assert_eq!(call.args.as_ref().and_then(|args| args.last()), config.terminal.external.windows_exec.as_ref());
    assert_eq!(call.options.expect("opções").cwd.as_deref(), Some(test_cwd));
}

#[test]
fn win_terminal_service_uses_default_terminal_when_configuration_terminal_external_windows_exec_is_undefined() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let test_shell = "cmd";
    let test_cwd = "path/to/workspace";
    let mut config = mock_config();
    config.terminal.external.windows_exec = None;
    let spawner = Rc::new(MockSpawner::default());
    let test_service = WindowsExternalTerminalService::new();
    test_service.spawn_terminal(spawner.clone(), &config.terminal.external, test_shell, Some(test_cwd));
    let call = spawner.only_call();
    assert_eq!(call.args.as_ref().and_then(|args| args.last()), Some(&WindowsExternalTerminalService::get_default_terminal_windows()));
}

#[test]
fn win_terminal_service_cwd_is_correct_regardless_of_case() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let test_shell = "cmd";
    let test_cwd = "c:/foo";
    let config = mock_config();
    let spawner = Rc::new(MockSpawner::default());
    let test_service = WindowsExternalTerminalService::new();
    test_service.spawn_terminal(spawner.clone(), &config.terminal.external, test_shell, Some(test_cwd));
    let call = spawner.only_call();
    assert_eq!(call.options.expect("opções").cwd.as_deref(), Some("C:/foo"), "cwd should be uppercase regardless of the case that's passed in");
}

#[test]
fn win_terminal_service_cmder_should_be_spawned_differently() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let test_shell = "cmd";
    let test_cwd = "c:/foo";
    let spawner = Rc::new(MockSpawner::default());
    let test_service = WindowsExternalTerminalService::new();
    test_service.spawn_terminal(
        spawner.clone(),
        &IExternalTerminalSettings { windows_exec: Some("cmder".to_owned()), ..IExternalTerminalSettings::default() },
        test_shell,
        Some(test_cwd),
    );
    let call = spawner.only_call();
    assert_eq!(call.args, Some(vec!["C:/foo".to_owned()]));
    assert_eq!(call.options, None);
}

#[test]
fn win_terminal_service_windows_terminal_should_open_workspace_directory() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let test_shell = "wt";
    let test_cwd = "c:/foo";
    let config = mock_config();
    let spawner = Rc::new(MockSpawner::default());
    let test_service = WindowsExternalTerminalService::new();
    test_service.spawn_terminal(spawner.clone(), &config.terminal.external, test_shell, Some(test_cwd));
    let call = spawner.only_call();
    assert_eq!(call.options.expect("opções").cwd.as_deref(), Some("C:/foo"));
}

#[test]
fn mac_terminal_service_uses_terminal_from_configuration() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let test_cwd = "path/to/workspace";
    let config = mock_config();
    let spawner = Rc::new(MockSpawner::default());
    let test_service = MacExternalTerminalService::new();
    test_service.spawn_terminal(spawner.clone(), &config.terminal.external, Some(test_cwd));
    let call = spawner.only_call();
    assert_eq!(call.args.expect("argumentos")[1], config.terminal.external.osx_exec.expect("osxExec"));
}

#[test]
fn mac_terminal_service_uses_default_terminal_when_configuration_terminal_external_osx_exec_is_undefined() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let test_cwd = "path/to/workspace";
    let spawner = Rc::new(MockSpawner::default());
    let test_service = MacExternalTerminalService::new();
    test_service.spawn_terminal(spawner.clone(), &IExternalTerminalSettings { osx_exec: None, ..IExternalTerminalSettings::default() }, Some(test_cwd));
    let call = spawner.only_call();
    assert_eq!(call.args.expect("argumentos")[1], DEFAULT_TERMINAL_OSX);
}

#[test]
fn mac_terminal_service_ghostty_app_should_be_spawned_correctly() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let test_cwd = "path/to/workspace";
    let spawner = Rc::new(MockSpawner::default());
    let test_service = MacExternalTerminalService::new();
    test_service.spawn_terminal(spawner.clone(), &IExternalTerminalSettings { osx_exec: Some("Ghostty.app".to_owned()), ..IExternalTerminalSettings::default() }, Some(test_cwd));
    let call = spawner.only_call();
    assert_eq!(call.command, "/usr/bin/open");
    let args = call.args.expect("argumentos");
    assert_eq!(args[0], "-a");
    assert_eq!(args[1], "Ghostty.app");
    assert_eq!(args[2], test_cwd);
    assert_eq!(call.options.expect("opções").cwd.as_deref(), Some(test_cwd));
}

#[test]
fn linux_terminal_service_uses_terminal_from_configuration() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let test_cwd = "path/to/workspace";
    let config = mock_config();
    let spawner = Rc::new(MockSpawner::default());
    let test_service = LinuxExternalTerminalService::new();
    test_service.spawn_terminal(spawner.clone(), &config.terminal.external, Some(test_cwd));
    runtime::run_microtasks();
    let call = spawner.only_call();
    assert_eq!(Some(call.command), config.terminal.external.linux_exec);
    assert_eq!(call.options.expect("opções").cwd.as_deref(), Some(test_cwd));
}

#[test]
fn linux_terminal_service_ghostty_should_be_spawned_with_working_directory() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let test_cwd = "path/to/workspace";
    let spawner = Rc::new(MockSpawner::default());
    let test_service = LinuxExternalTerminalService::new();
    test_service.spawn_terminal(spawner.clone(), &IExternalTerminalSettings { linux_exec: Some("ghostty".to_owned()), ..IExternalTerminalSettings::default() }, Some(test_cwd));
    runtime::run_microtasks();
    let call = spawner.only_call();
    assert_eq!(call.command, "ghostty");
    assert_eq!(call.args, Some(vec![format!("--working-directory={test_cwd}")]));
    assert_eq!(call.options.expect("opções").cwd.as_deref(), Some(test_cwd));
}

#[test]
fn linux_terminal_service_uses_default_terminal_when_configuration_terminal_external_linux_exec_is_undefined() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let default_terminal_linux = LinuxExternalTerminalService::get_default_terminal_linux_ready();
    runtime::run_microtasks();
    let default_terminal_linux = default_terminal_linux.result().expect("assentada").expect("sem erro");

    let test_cwd = "path/to/workspace";
    let mut config = mock_config();
    config.terminal.external.linux_exec = None;
    let spawner = Rc::new(MockSpawner::default());
    let test_service = LinuxExternalTerminalService::new();
    test_service.spawn_terminal(spawner.clone(), &config.terminal.external, Some(test_cwd));
    runtime::run_microtasks();
    let call = spawner.only_call();
    assert_eq!(call.command, default_terminal_linux);
}
