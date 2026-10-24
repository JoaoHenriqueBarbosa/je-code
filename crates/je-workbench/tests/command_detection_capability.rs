//! Porte de `src/vs/workbench/contrib/terminal/test/browser/capabilities/commandDetectionCapability.test.ts` (suíte
//! `CommandDetectionCapability`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - **`setup` e `ensureNoDisposablesAreLeakedInTestSuite()`.** O [`Fixture`] cria o guarda de vazamentos antes de tudo, o
//!   `Terminal` do xterm (o [`FakeXterm`] de 80 colunas, registrado no store), a `CommandDetectionCapability` (o
//!   `instantiationService.createInstance` do original passa o `ILogService`, que é o [`NullLogService`]) e o ouvinte de
//!   `onCommandFinished` que acumula `addEvents`; por fim confere `assertCommands([])`.
//! - **`TestCommandDetectionCapability.clearCommandsForTest`.** É `set_commands(Vec::new())`, o `_commands.length = 0` da
//!   subclasse do original (o campo é `protected`).
//! - **`writeP(xterm, data)`.** É `FakeXterm::write_p`, que interpreta o texto de forma síncrona.
//! - **`xterm.input('\x03')`.** O `input` do xterm entrega o texto ao `onData`; é `FakeXterm::fire_data`.
//! - **`deepStrictEqual(addEvents, capability.commands)`.** Os dois lados são os mesmos objetos: compara a identidade
//!   (`Rc::ptr_eq`) na mesma ordem.
//! - **`marker?.line`.** É `Option<f64>`: o `undefined` do original é `None`.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod support;

use std::cell::RefCell;
use std::rc::Rc;

use common::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite};
use je_base::lifecycle::IDisposable;
use je_base::runtime;
use je_platform::log::NullLogService;
use je_platform::terminal::capabilities::capabilities::{IBaseTerminalCommand, ICommandDetectionCapability, ITerminalCommand};
use je_platform::terminal::capabilities::command_detection_capability::CommandDetectionCapability;
use je_platform::terminal::xterm_headless::IHeadlessTerminal;
use support::fake_xterm::FakeXterm;

/// `TestTerminalCommandMatch`: `Pick<ITerminalCommand, 'command' | 'cwd' | 'exitCode'> & { marker: { line: number } }`.
struct TestTerminalCommandMatch {
    command: &'static str,
    cwd: Option<&'static str>,
    exit_code: Option<f64>,
    line: f64,
}

fn expected(command: &'static str, exit_code: Option<f64>, cwd: Option<&'static str>, line: f64) -> TestTerminalCommandMatch {
    TestTerminalCommandMatch { command, cwd, exit_code, line }
}

struct Fixture {
    xterm: Rc<FakeXterm>,
    capability: Rc<CommandDetectionCapability>,
    add_events: Rc<RefCell<Vec<Rc<dyn ITerminalCommand>>>>,
    // Declarado por último: o `Drop` dele descarta o que o `Fixture` registrou e confere os vazamentos.
    store: LeakGuard,
}

impl Fixture {
    fn new() -> Self {
        let store = ensure_no_disposables_are_leaked_in_test_suite();
        let xterm = store.add_rc(FakeXterm::new(80, 30));
        let terminal: Rc<dyn IHeadlessTerminal> = xterm.clone();
        let capability = store.add_rc(CommandDetectionCapability::new(terminal, NullLogService::shared()));
        let add_events: Rc<RefCell<Vec<Rc<dyn ITerminalCommand>>>> = Rc::default();
        let sink = add_events.clone();
        store.add(capability.on_command_finished().listen(move |e| sink.borrow_mut().push(e.clone())));
        let fixture = Self { xterm, capability, add_events, store };
        fixture.assert_commands(&[]);
        fixture
    }

    fn assert_commands(&self, expected_commands: &[TestTerminalCommandMatch]) {
        let commands = self.capability.commands();
        assert_eq!(commands.iter().map(|e| e.command()).collect::<Vec<_>>(), expected_commands.iter().map(|e| e.command.to_owned()).collect::<Vec<_>>());
        assert_eq!(commands.iter().map(|e| e.cwd()).collect::<Vec<_>>(), expected_commands.iter().map(|e| e.cwd.map(str::to_owned)).collect::<Vec<_>>());
        assert_eq!(commands.iter().map(|e| e.exit_code()).collect::<Vec<_>>(), expected_commands.iter().map(|e| e.exit_code).collect::<Vec<_>>());
        assert_eq!(
            commands.iter().map(|e| e.marker().map(|marker| marker.line())).collect::<Vec<_>>(),
            expected_commands.iter().map(|e| Some(e.line)).collect::<Vec<_>>()
        );
        // Ensure timestamps are set and were captured recently
        for command in &commands {
            assert!((runtime::date_now() as f64 - command.timestamp()).abs() < 2000.0);
            assert!(command.id().is_some_and(|id| !id.is_empty()), "Expected command to have an assigned id");
        }
        {
            let add_events = self.add_events.borrow();
            assert_eq!(add_events.len(), commands.len());
            assert!(add_events.iter().zip(commands.iter()).all(|(added, command)| Rc::ptr_eq(added, command)));
        }
        // Clear the commands to avoid re-asserting past commands
        self.add_events.borrow_mut().clear();
        self.capability.set_commands(Vec::new());
    }

    fn print_standard_command(&self, prompt: &str, command: &str, output: &str, cwd: Option<&str>, exit_code: f64) {
        if let Some(cwd) = cwd {
            self.capability.set_cwd(cwd);
        }
        self.capability.handle_prompt_start(None);
        self.xterm.write_p(&format!("\r{prompt}"));
        self.capability.handle_command_start(None);
        self.xterm.write_p(command);
        self.capability.handle_command_executed(None);
        self.xterm.write_p(&format!("\r\n{output}\r\n"));
        self.capability.handle_command_finished(Some(exit_code), None);
    }

    fn print_command_start(&self, prompt: &str) {
        self.capability.handle_prompt_start(None);
        self.xterm.write_p(&format!("\r{prompt}"));
        self.capability.handle_command_start(None);
    }
}

#[test]
fn should_not_add_commands_when_no_capability_methods_are_triggered() {
    let f = Fixture::new();
    f.xterm.write_p("foo\r\nbar\r\n");
    f.assert_commands(&[]);
    f.xterm.write_p("baz\r\n");
    f.assert_commands(&[]);
}

#[test]
fn should_add_commands_for_expected_capability_method_calls() {
    let f = Fixture::new();
    f.print_standard_command("$ ", "echo foo", "foo", None, 0.0);
    f.print_command_start("$ ");
    f.assert_commands(&[expected("echo foo", Some(0.0), None, 0.0)]);
}

#[test]
fn should_invalidate_all_commands_when_cleared() {
    let f = Fixture::new();
    f.print_standard_command("$ ", "echo foo", "foo", None, 0.0);
    f.print_standard_command("$ ", "echo bar", "bar", None, 0.0);
    assert_eq!(f.capability.commands().len(), 2);

    let invalidated_commands: Rc<RefCell<Vec<Rc<dyn ITerminalCommand>>>> = Rc::default();
    let sink = invalidated_commands.clone();
    f.store.add(f.capability.on_command_invalidated().listen(move |commands| sink.borrow_mut().extend(commands.iter().cloned())));
    f.capability.clear_commands();

    assert!(f.capability.commands().is_empty());
    assert_eq!(invalidated_commands.borrow().iter().map(|e| e.command()).collect::<Vec<_>>(), vec!["echo foo".to_owned(), "echo bar".to_owned()]);
}

#[test]
fn should_trim_the_command_when_command_executed_appears_on_the_following_line() {
    let f = Fixture::new();
    f.print_standard_command("$ ", "echo foo\r\n", "foo", None, 0.0);
    f.print_command_start("$ ");
    f.assert_commands(&[expected("echo foo", Some(0.0), None, 0.0)]);
}

// ---------------------------------------------------------------------------------------------------------------------
// suite('cwd')
// ---------------------------------------------------------------------------------------------------------------------

#[test]
fn cwd_should_add_cwd_to_commands_when_its_set() {
    let f = Fixture::new();
    f.print_standard_command("$ ", "echo foo", "foo", Some("/home"), 0.0);
    f.print_standard_command("$ ", "echo bar", "bar", Some("/home/second"), 0.0);
    f.print_command_start("$ ");
    f.assert_commands(&[expected("echo foo", Some(0.0), Some("/home"), 0.0), expected("echo bar", Some(0.0), Some("/home/second"), 2.0)]);
}

#[test]
fn cwd_should_add_old_cwd_to_commands_if_no_cwd_sequence_is_output() {
    let f = Fixture::new();
    f.print_standard_command("$ ", "echo foo", "foo", Some("/home"), 0.0);
    f.print_standard_command("$ ", "echo bar", "bar", None, 0.0);
    f.print_command_start("$ ");
    f.assert_commands(&[expected("echo foo", Some(0.0), Some("/home"), 0.0), expected("echo bar", Some(0.0), Some("/home"), 2.0)]);
}

#[test]
fn cwd_should_use_an_undefined_cwd_if_its_not_set_initially() {
    let f = Fixture::new();
    f.print_standard_command("$ ", "echo foo", "foo", None, 0.0);
    f.print_standard_command("$ ", "echo bar", "bar", Some("/home"), 0.0);
    f.print_command_start("$ ");
    f.assert_commands(&[expected("echo foo", Some(0.0), None, 0.0), expected("echo bar", Some(0.0), Some("/home"), 2.0)]);
}

#[test]
fn should_not_inherit_the_previous_exit_code_when_a_duplicate_command_is_interrupted() {
    let f = Fixture::new();
    f.print_standard_command("$ ", "echo test", "test", None, 0.0);

    f.capability.handle_prompt_start(None);
    f.xterm.write_p("\r$ ");
    f.capability.handle_command_start(None);
    f.xterm.write_p("echo test");
    f.xterm.fire_data("\u{3}");
    f.xterm.write_p("^C");
    f.capability.set_command_line("echo test", true);
    f.capability.handle_command_executed(None);
    f.xterm.write_p("\r\n");
    f.capability.handle_command_finished(None, None);

    f.print_command_start("$ ");

    f.assert_commands(&[expected("echo test", Some(0.0), None, 0.0), expected("echo test", None, None, 2.0)]);
}

#[test]
fn should_inherit_the_previous_exit_code_for_duplicate_commands_without_interruption() {
    let f = Fixture::new();
    f.print_standard_command("$ ", "echo ^C", "test", None, 0.0);

    f.capability.handle_prompt_start(None);
    f.xterm.write_p("\r$ ");
    f.capability.handle_command_start(None);
    f.xterm.write_p("echo ^C");
    f.capability.set_command_line("echo ^C", true);
    f.capability.handle_command_executed(None);
    f.xterm.write_p("\r\ntest\r\n");
    f.capability.handle_command_finished(None, None);

    f.print_command_start("$ ");

    f.assert_commands(&[expected("echo ^C", Some(0.0), None, 0.0), expected("echo ^C", Some(0.0), None, 2.0)]);
}

#[test]
fn should_preserve_explicit_newlines_at_80_column_wrap_boundaries_in_command_output() {
    let f = Fixture::new();
    let boundary_width_line = "A".repeat(80);
    f.print_standard_command("$ ", "cat content.txt", &format!("{boundary_width_line}\r\nafter"), None, 0.0);
    f.print_command_start("$ ");

    let commands = f.capability.commands();
    assert_eq!(commands.len(), 1);
    let output = commands[0].get_output();
    let output = output.expect("o comando tem saída");
    assert!(!output.is_empty());
    assert!(output.contains(&format!("{boundary_width_line}\nafter\n")));
    assert!(!output.contains(&format!("{boundary_width_line}after")));
}
