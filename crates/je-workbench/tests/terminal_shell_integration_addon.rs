//! Porte de `src/vs/workbench/contrib/terminal/test/browser/xterm/shellIntegrationAddon.test.ts` (suíte `ShellIntegrationAddon`), caso a caso e na mesma ordem.
//!
//! Os 23 casos que usam o emulador (`cwd detection`, `command tracking` e os quatro `SetMark`) rodam contra o
//! [`FakeXterm`] (em `tests/support/fake_xterm.rs`, no lugar do `Terminal` do xterm): `writeP(xterm, data)` é
//! [`FakeXterm::write_p`], e o `loadAddon` é [`ShellIntegrationAddon::activate`]. Os 26 casos de funções puras
//! (`parseMarkSequence` (4), `deserializeMessage` (22), `serializeVSCodeOscMessage` (21) e `parseKeyValueAssignment` (1))
//! seguem como estavam.
//!
//! Divergências:
//! - **`TestShellIntegrationAddon` e `sinon.mock(capability)`.** A subclasse de teste expõe os métodos `protected`
//!   `_createOrGetCommandDetection` e `_createOrGetCwdDetection`, que no porte são públicos; o `getCommandDetectionMock` e o
//!   `getCwdDectionMock` criam (ou pegam) a capacidade real, a trocam no store por um dublê que repassa tudo à real e que
//!   registra as chamadas dos métodos do mock ([`RecordingCommandDetection`] e [`RecordingCwdDetection`]), e o
//!   `mock.expects(...).once().withExactArgs(...)` e o `mock.verify()` viram a comparação da lista de chamadas gravadas com a
//!   lista esperada. Como no `sinon`, o método com expectativa não chama o original.
//! - **`ensureNoDisposablesAreLeakedInTestSuite()`.** O [`Fixture`] cria o guarda de vazamentos antes de tudo e descarta o
//!   addon no `Drop`.
//! - **`new TestShellIntegrationAddon('', true, undefined, undefined, new NullLogService())`.** São os mesmos argumentos do
//!   [`ShellIntegrationAddon::new`] (nonce vazio, telemetria desligada, sem evento de texto e sem serviço de telemetria).
//! - **`withExactArgs()` de `handleCommandFinished(7)`.** O argumento é `Some(7.0)` (o `number`) e a lista grava o texto da
//!   chamada.
//! - **`cases.forEach(... test(title, ...))`.** Cada elemento do vetor de casos é um `#[test]` gerado pela macro
//!   `deserialize_case!`/`serialize_case!`, com o título no nome da função.
//! - **`Backslash`, `Newline` e `Semicolon`.** Os casos escrevem as barras invertidas diretamente em literais `r"..."` ou
//!   com `\\`, o que o original evita pelas constantes por clareza; os valores são os mesmos.
//! - **`deepEqual(parseMarkSequence(...), { id: undefined, hidden: false })`** é `assert_eq!` com [`MarkSequence`]
//!   (`id: None` é o `undefined`).

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod support;

use std::cell::RefCell;
use std::rc::Rc;

use common::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite};
use je_base::event::Event;
use je_platform::log::NullLogService;
use je_platform::terminal::capabilities::capabilities::{
    CommandDetection, CwdDetection, ICommandDetectionCapability, ICommandInvalidationRequest, ICurrentPartialCommand, ICwdDetectionCapability, IHandleCommandOptions,
    IPromptInputModel, ISerializedCommandDetectionCapability, ITerminalCapabilityStore, ITerminalCommand, TerminalCapability, TerminalCommandConfidence,
    TerminalCommandForLine,
};
use je_platform::terminal::capabilities::terminal_capability_store::TerminalCapabilityStore;
use je_platform::terminal::shell_integration_addon::{
    KeyValueAssignment, MarkSequence, ShellIntegrationAddon, deserialize_vscode_osc_message, parse_key_value_assignment, parse_mark_sequence,
    serialize_vscode_osc_message,
};
use je_platform::terminal::xterm_headless::IHeadlessTerminal;
use support::fake_xterm::FakeXterm;

/// A lista de chamadas que o dublê grava no lugar da expectativa do `sinon`.
type Calls = Rc<RefCell<Vec<String>>>;

/// O dublê de `sinon.mock(cwdDetection)` com `expects('updateCwd')`: repassa tudo à capacidade real, menos o
/// `updateCwd`, que só grava a chamada.
struct RecordingCwdDetection {
    inner: Rc<dyn ICwdDetectionCapability>,
    calls: Calls,
}

impl ICwdDetectionCapability for RecordingCwdDetection {
    fn capability_type(&self) -> TerminalCapability {
        self.inner.capability_type()
    }

    fn on_did_change_cwd(&self) -> Event<String> {
        self.inner.on_did_change_cwd()
    }

    fn cwds(&self) -> Vec<String> {
        self.inner.cwds()
    }

    fn is_trusted(&self) -> bool {
        self.inner.is_trusted()
    }

    fn get_cwd(&self) -> String {
        self.inner.get_cwd()
    }

    fn update_cwd(&self, cwd: &str, is_trusted: Option<bool>) {
        self.calls.borrow_mut().push(format!("updateCwd({cwd:?}, {is_trusted:?})"));
    }
}

/// O dublê de `sinon.mock(commandDetection)` com as expectativas dos métodos que os casos usam (`handlePromptStart`,
/// `handleCommandStart`, `handleCommandExecuted`, `handleCommandFinished`, `setCommandLine` e `setCwd`): repassa tudo à
/// capacidade real, menos esses seis, que só gravam a chamada.
struct RecordingCommandDetection {
    inner: Rc<dyn ICommandDetectionCapability>,
    calls: Calls,
}

impl RecordingCommandDetection {
    fn record(&self, call: String) {
        self.calls.borrow_mut().push(call);
    }
}

impl ICommandDetectionCapability for RecordingCommandDetection {
    fn capability_type(&self) -> TerminalCapability {
        self.inner.capability_type()
    }

    fn prompt_input_model(&self) -> Rc<dyn IPromptInputModel> {
        self.inner.prompt_input_model()
    }

    fn commands(&self) -> Vec<Rc<dyn ITerminalCommand>> {
        self.inner.commands()
    }

    fn executing_command(&self) -> Option<String> {
        self.inner.executing_command()
    }

    fn executing_command_object(&self) -> Option<Rc<dyn ITerminalCommand>> {
        self.inner.executing_command_object()
    }

    fn executing_command_confidence(&self) -> Option<TerminalCommandConfidence> {
        self.inner.executing_command_confidence()
    }

    fn cwd(&self) -> Option<String> {
        self.inner.cwd()
    }

    fn has_rich_command_detection(&self) -> bool {
        self.inner.has_rich_command_detection()
    }

    fn current_command(&self) -> Option<Rc<dyn ICurrentPartialCommand>> {
        self.inner.current_command()
    }

    fn on_command_started(&self) -> Event<Rc<dyn ITerminalCommand>> {
        self.inner.on_command_started()
    }

    fn on_command_finished(&self) -> Event<Rc<dyn ITerminalCommand>> {
        self.inner.on_command_finished()
    }

    fn on_command_executed(&self) -> Event<Rc<dyn ITerminalCommand>> {
        self.inner.on_command_executed()
    }

    fn on_command_invalidated(&self) -> Event<Vec<Rc<dyn ITerminalCommand>>> {
        self.inner.on_command_invalidated()
    }

    fn on_current_command_invalidated(&self) -> Event<ICommandInvalidationRequest> {
        self.inner.on_current_command_invalidated()
    }

    fn on_set_rich_command_detection(&self) -> Event<bool> {
        self.inner.on_set_rich_command_detection()
    }

    fn set_continuation_prompt(&self, value: &str) {
        self.inner.set_continuation_prompt(value);
    }

    fn set_prompt_terminator(&self, value: &str, last_prompt_line: &str) {
        self.inner.set_prompt_terminator(value, last_prompt_line);
    }

    fn set_cwd(&self, value: &str) {
        self.record(format!("setCwd({value:?})"));
    }

    fn set_is_windows_pty(&self, value: bool) {
        self.inner.set_is_windows_pty(value);
    }

    fn set_is_command_storage_disabled(&self) {
        self.inner.set_is_command_storage_disabled();
    }

    fn get_cwd_for_line(&self, line: f64) -> Option<String> {
        self.inner.get_cwd_for_line(line)
    }

    fn get_command_for_line(&self, line: f64) -> Option<TerminalCommandForLine> {
        self.inner.get_command_for_line(line)
    }

    fn clear_commands(&self) {
        self.inner.clear_commands();
    }

    fn handle_prompt_start(&self, options: Option<&IHandleCommandOptions>) {
        assert!(options.is_none(), "o original chama handlePromptStart sem argumentos");
        self.record("handlePromptStart()".to_owned());
    }

    fn handle_continuation_start(&self) {
        self.inner.handle_continuation_start();
    }

    fn handle_continuation_end(&self) {
        self.inner.handle_continuation_end();
    }

    fn handle_right_prompt_start(&self) {
        self.inner.handle_right_prompt_start();
    }

    fn handle_right_prompt_end(&self) {
        self.inner.handle_right_prompt_end();
    }

    fn handle_command_start(&self, options: Option<&IHandleCommandOptions>) {
        assert!(options.is_none(), "o original chama handleCommandStart sem argumentos");
        self.record("handleCommandStart()".to_owned());
    }

    fn handle_command_executed(&self, options: Option<&IHandleCommandOptions>) {
        assert!(options.is_none(), "o original chama handleCommandExecuted sem argumentos");
        self.record("handleCommandExecuted()".to_owned());
    }

    fn handle_command_finished(&self, exit_code: Option<f64>, options: Option<&IHandleCommandOptions>) {
        assert!(options.is_none(), "o original chama handleCommandFinished só com o código de saída");
        self.record(format!("handleCommandFinished({exit_code:?})"));
    }

    fn set_has_rich_command_detection(&self, value: bool) {
        self.inner.set_has_rich_command_detection(value);
    }

    fn set_command_line(&self, command_line: &str, is_trusted: bool) {
        self.record(format!("setCommandLine({command_line:?}, {is_trusted})"));
    }

    fn set_next_command_id(&self, command: &str, command_id: &str) {
        self.inner.set_next_command_id(command, command_id);
    }

    fn serialize(&self) -> ISerializedCommandDetectionCapability {
        self.inner.serialize()
    }

    fn deserialize(&self, serialized: &ISerializedCommandDetectionCapability) {
        self.inner.deserialize(serialized);
    }
}

/// O `setup` da suíte: o emulador (80 colunas por 30 linhas), o addon ativado nele e o store de capacidades.
struct Fixture {
    xterm: Rc<FakeXterm>,
    addon: Rc<ShellIntegrationAddon>,
    capabilities: Rc<TerminalCapabilityStore>,
    // Declarado por último: o `Drop` dele descarta o addon e confere os vazamentos depois dos demais campos.
    _guard: LeakGuard,
}

/// O `teardown` do original descarta o `xterm`, que descarta os marcadores registrados; o guarda confere os vazamentos depois.
impl Drop for Fixture {
    fn drop(&mut self) {
        self.xterm.dispose();
    }
}

impl Fixture {
    fn new() -> Self {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let xterm = FakeXterm::new(80, 30);
        let addon = guard.add_rc(ShellIntegrationAddon::new("", Some(true), None, None, NullLogService::shared()));
        addon.activate(xterm.clone());
        let capabilities = addon.capabilities();
        Self { xterm, addon, capabilities, _guard: guard }
    }

    fn terminal(&self) -> Rc<dyn IHeadlessTerminal> {
        self.xterm.clone()
    }

    /// `shellIntegrationAddon.getCommandDetectionMock(xterm)`.
    fn get_command_detection_mock(&self) -> Calls {
        let capability = self.addon.create_or_get_command_detection(&self.terminal());
        let calls = Calls::default();
        self.capabilities.add::<CommandDetection>(Rc::new(RecordingCommandDetection { inner: capability, calls: calls.clone() }));
        calls
    }

    /// `shellIntegrationAddon.getCwdDectionMock()`.
    fn get_cwd_detection_mock(&self) -> Calls {
        let capability = self.addon.create_or_get_cwd_detection();
        let calls = Calls::default();
        self.capabilities.add::<CwdDetection>(Rc::new(RecordingCwdDetection { inner: capability, calls: calls.clone() }));
        calls
    }

    fn write_p(&self, data: &str) {
        self.xterm.write_p(data);
    }

    fn has(&self, capability: TerminalCapability) -> bool {
        self.capabilities.has(capability)
    }

    /// O trio `has(false)`, `writeP('foo')`, `has(false)` que abre os casos de ativação, e depois a escrita de `sequence`.
    fn assert_activates_on(&self, capability: TerminalCapability, sequence: &str) {
        assert!(!self.has(capability));
        self.write_p("foo");
        assert!(!self.has(capability));
        self.write_p(sequence);
        assert!(self.has(capability));
    }
}

/// `mock.expects('updateCwd').once().withExactArgs(cwd, isTrusted)`.
fn update_cwd_call(cwd: &str, is_trusted: bool) -> String {
    format!("updateCwd({cwd:?}, Some({is_trusted}))")
}

// ---------------------------------------------------------------------------------------------------------------------
// suite('cwd detection')
// ---------------------------------------------------------------------------------------------------------------------

#[test]
fn should_activate_capability_on_the_cwd_sequence_osc_633_p_cwd_cwd_st() {
    let f = Fixture::new();
    f.assert_activates_on(TerminalCapability::CwdDetection, "\x1b]633;P;Cwd=/foo\x07");
}

#[test]
fn should_pass_cwd_sequence_to_the_capability_as_trusted_when_nonce_matches() {
    let f = Fixture::new();
    let calls = f.get_cwd_detection_mock();
    // The addon is constructed with nonce '' so a trailing ';' produces args[1]==='' which matches
    f.write_p("\x1b]633;P;Cwd=/foo;\x07");
    assert_eq!(*calls.borrow(), [update_cwd_call("/foo", true)]);
}

#[test]
fn should_treat_cwd_sequence_as_untrusted_when_nonce_is_missing() {
    let f = Fixture::new();
    let calls = f.get_cwd_detection_mock();
    f.write_p("\x1b]633;P;Cwd=/foo\x07");
    assert_eq!(*calls.borrow(), [update_cwd_call("/foo", false)]);
}

#[test]
fn should_treat_cwd_sequence_as_untrusted_when_nonce_does_not_match() {
    let f = Fixture::new();
    let calls = f.get_cwd_detection_mock();
    f.write_p("\x1b]633;P;Cwd=/foo;invalid-nonce\x07");
    assert_eq!(*calls.borrow(), [update_cwd_call("/foo", false)]);
}

#[test]
fn detect_iterm_sequence_osc_1337_current_dir_cwd_st() {
    let f = Fixture::new();
    // [title, input, expected]
    let cases = [("root", "/", "/"), ("non-root", "/some/path", "/some/path")];
    for (title, input, expected) in cases {
        let calls = f.get_cwd_detection_mock();
        f.write_p(&format!("\x1b]1337;CurrentDir={input}\x07"));
        assert_eq!(*calls.borrow(), [update_cwd_call(expected, false)], "{title}");
    }
}

#[test]
fn detect_set_cwd_sequence_should_accept_well_formatted_urls() {
    let f = Fixture::new();
    // [title, input, expected]
    let cases = [
        // Different hostname values:
        ("empty hostname, pointing root", "file:///", "/"),
        ("empty hostname", "file:///test-root/local", "/test-root/local"),
        ("non-empty hostname", "file://some-hostname/test-root/local", "/test-root/local"),
        // URL-encoded chars:
        ("URL-encoded value (1)", "file:///test-root/%6c%6f%63%61%6c", "/test-root/local"),
        ("URL-encoded value (2)", "file:///test-root/local%22", "/test-root/local\""),
        ("URL-encoded value (3)", "file:///test-root/local\"", "/test-root/local\""),
    ];
    for (title, input, expected) in cases {
        let calls = f.get_cwd_detection_mock();
        f.write_p(&format!("\x1b]7;{input}\x07"));
        assert_eq!(*calls.borrow(), [update_cwd_call(expected, false)], "{title}");
    }
}

#[test]
fn detect_set_cwd_sequence_should_ignore_ill_formatted_urls() {
    let f = Fixture::new();
    // [title, input]
    let cases = [
        // Different hostname values:
        ("no hostname, pointing root", "file://"),
        // Non-`file` scheme values:
        ("no scheme (1)", "/test-root"),
        ("no scheme (2)", "//test-root"),
        ("no scheme (3)", "///test-root"),
        ("no scheme (4)", ":///test-root"),
        ("http", "http:///test-root"),
        ("ftp", "ftp:///test-root"),
        ("ssh", "ssh:///test-root"),
    ];
    for (title, input) in cases {
        let calls = f.get_cwd_detection_mock();
        f.write_p(&format!("\x1b]7;{input}\x07"));
        assert!(calls.borrow().is_empty(), "{title}");
    }
}

#[test]
fn detect_set_windows_friendly_cwd_sequence_osc_9_9_cwd_st() {
    let f = Fixture::new();
    // [title, input, expected]
    let cases = [("root", "/", "/"), ("non-root", "/some/path", "/some/path")];
    for (title, input, expected) in cases {
        let calls = f.get_cwd_detection_mock();
        f.write_p(&format!("\x1b]9;9;{input}\x07"));
        assert_eq!(*calls.borrow(), [update_cwd_call(expected, false)], "{title}");
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// suite('command tracking')
// ---------------------------------------------------------------------------------------------------------------------

#[test]
fn should_activate_capability_on_the_prompt_start_sequence_osc_633_a_st() {
    let f = Fixture::new();
    f.assert_activates_on(TerminalCapability::CommandDetection, "\x1b]633;A\x07");
}

#[test]
fn should_pass_prompt_start_sequence_to_the_capability() {
    let f = Fixture::new();
    let calls = f.get_command_detection_mock();
    f.write_p("\x1b]633;A\x07");
    assert_eq!(*calls.borrow(), ["handlePromptStart()"]);
}

#[test]
fn should_activate_capability_on_the_command_start_sequence_osc_633_b_st() {
    let f = Fixture::new();
    f.assert_activates_on(TerminalCapability::CommandDetection, "\x1b]633;B\x07");
}

#[test]
fn should_pass_command_start_sequence_to_the_capability() {
    let f = Fixture::new();
    let calls = f.get_command_detection_mock();
    f.write_p("\x1b]633;B\x07");
    assert_eq!(*calls.borrow(), ["handleCommandStart()"]);
}

#[test]
fn should_activate_capability_on_the_command_executed_sequence_osc_633_c_st() {
    let f = Fixture::new();
    f.assert_activates_on(TerminalCapability::CommandDetection, "\x1b]633;C\x07");
}

#[test]
fn should_pass_command_executed_sequence_to_the_capability() {
    let f = Fixture::new();
    let calls = f.get_command_detection_mock();
    f.write_p("\x1b]633;C\x07");
    assert_eq!(*calls.borrow(), ["handleCommandExecuted()"]);
}

#[test]
fn should_activate_capability_on_the_command_finished_sequence_osc_633_d_exit_code_st() {
    let f = Fixture::new();
    f.assert_activates_on(TerminalCapability::CommandDetection, "\x1b]633;D;7\x07");
}

#[test]
fn should_pass_command_finished_sequence_to_the_capability() {
    let f = Fixture::new();
    let calls = f.get_command_detection_mock();
    f.write_p("\x1b]633;D;7\x07");
    assert_eq!(*calls.borrow(), ["handleCommandFinished(Some(7.0))"]);
}

#[test]
fn should_pass_command_line_sequence_to_the_capability() {
    let f = Fixture::new();
    let calls = f.get_command_detection_mock();
    f.write_p("\x1b]633;E\x07");
    assert_eq!(*calls.borrow(), ["setCommandLine(\"\", false)"]);

    let calls2 = f.get_command_detection_mock();
    f.write_p("\x1b]633;E;cmd\x07");
    f.write_p("\x1b]633;E;cmd;invalid-nonce\x07");
    assert_eq!(*calls2.borrow(), ["setCommandLine(\"cmd\", false)", "setCommandLine(\"cmd\", false)"]);
}

#[test]
fn should_not_activate_capability_on_the_cwd_sequence_osc_633_p_cwd_cwd_st() {
    let f = Fixture::new();
    assert!(!f.has(TerminalCapability::CommandDetection));
    f.write_p("foo");
    assert!(!f.has(TerminalCapability::CommandDetection));
    f.write_p("\x1b]633;P;Cwd=/foo\x07");
    assert!(!f.has(TerminalCapability::CommandDetection));
}

#[test]
fn should_pass_cwd_sequence_to_the_capability_if_its_initialized() {
    let f = Fixture::new();
    let calls = f.get_command_detection_mock();
    f.write_p("\x1b]633;P;Cwd=/foo\x07");
    assert_eq!(*calls.borrow(), ["setCwd(\"/foo\")"]);
}

// ---------------------------------------------------------------------------------------------------------------------
// suite('BufferMarkCapability')
// ---------------------------------------------------------------------------------------------------------------------

#[test]
fn set_mark() {
    let f = Fixture::new();
    f.assert_activates_on(TerminalCapability::BufferMarkDetection, "\x1b]633;SetMark;\x07");
}

#[test]
fn set_mark_id() {
    let f = Fixture::new();
    f.assert_activates_on(TerminalCapability::BufferMarkDetection, "\x1b]633;SetMark;1;\x07");
}

#[test]
fn set_mark_hidden() {
    let f = Fixture::new();
    f.assert_activates_on(TerminalCapability::BufferMarkDetection, "\x1b]633;SetMark;;Hidden\x07");
}

#[test]
fn set_mark_hidden_and_id() {
    let f = Fixture::new();
    f.assert_activates_on(TerminalCapability::BufferMarkDetection, "\x1b]633;SetMark;1;Hidden\x07");
}

// ---------------------------------------------------------------------------------------------------------------------
// suite('BufferMarkCapability') > suite('parseMarkSequence')
// ---------------------------------------------------------------------------------------------------------------------

#[test]
fn parse_mark_sequence_basic() {
    assert_eq!(parse_mark_sequence(&[Some(""), Some("")]), MarkSequence { id: None, hidden: false });
}

#[test]
fn parse_mark_sequence_id() {
    assert_eq!(parse_mark_sequence(&[Some("Id=3"), Some("")]), MarkSequence { id: Some("3".to_owned()), hidden: false });
}

#[test]
fn parse_mark_sequence_hidden() {
    assert_eq!(parse_mark_sequence(&[Some(""), Some("Hidden")]), MarkSequence { id: None, hidden: true });
}

#[test]
fn parse_mark_sequence_id_hidden() {
    assert_eq!(parse_mark_sequence(&[Some("Id=4555"), Some("Hidden")]), MarkSequence { id: Some("4555".to_owned()), hidden: true });
}

// ---------------------------------------------------------------------------------------------------------------------
// suite('deserializeMessage')
// ---------------------------------------------------------------------------------------------------------------------

/// `test(title, () => strictEqual(deserializeVSCodeOscMessage(input), expected))`.
macro_rules! deserialize_case {
    ($name:ident, $input:expr, $expected:expr) => {
        #[test]
        fn $name() {
            assert_eq!(deserialize_vscode_osc_message($input), $expected);
        }
    };
}

deserialize_case!(deserialize_message_empty, "", "");
deserialize_case!(deserialize_message_basic, "value", "value");
deserialize_case!(deserialize_message_space, "some thing", "some thing");
deserialize_case!(deserialize_message_escaped_backslash, r"\\", r"\");
deserialize_case!(deserialize_message_non_initial_escaped_backslash, r"foo\\", r"foo\");
deserialize_case!(deserialize_message_two_escaped_backslashes, r"\\\\", r"\\");
deserialize_case!(deserialize_message_escaped_backslash_amidst_text, r"Hello\\there", r"Hello\there");
deserialize_case!(deserialize_message_backslash_escaped_literally_and_as_hex, r"\\ is same as \x5c", r"\ is same as \");
deserialize_case!(deserialize_message_escaped_semicolon, r"\x3b", ";");
deserialize_case!(deserialize_message_non_initial_escaped_semicolon, r"foo\x3b", "foo;");
deserialize_case!(deserialize_message_escaped_semicolon_upper_hex, r"\x3B", ";");
deserialize_case!(deserialize_message_escaped_backslash_followed_by_literal_x3b_is_not_a_semicolon, r"\\x3b", r"\x3b");
deserialize_case!(deserialize_message_non_initial_escaped_backslash_followed_by_literal_x3b_is_not_a_semicolon, r"foo\\x3b", r"foo\x3b");
deserialize_case!(deserialize_message_escaped_backslash_followed_by_escaped_semicolon, r"\\\x3b", r"\;");
deserialize_case!(deserialize_message_escaped_semicolon_amidst_text, r"some\x3bthing", "some;thing");
deserialize_case!(deserialize_message_escaped_newline, r"\x0a", "\n");
deserialize_case!(deserialize_message_non_initial_escaped_newline, r"foo\x0a", "foo\n");
deserialize_case!(deserialize_message_escaped_newline_upper_hex, r"\x0A", "\n");
deserialize_case!(deserialize_message_escaped_backslash_followed_by_literal_x0a_is_not_a_newline, r"\\x0a", r"\x0a");
deserialize_case!(deserialize_message_non_initial_escaped_backslash_followed_by_literal_x0a_is_not_a_newline, r"foo\\x0a", r"foo\x0a");
deserialize_case!(deserialize_message_ps1_simple, r"[\u@\h \W]\$", r"[\u@\h \W]\$");
deserialize_case!(
    deserialize_message_ps1_vsc_si,
    r"\x1b]633;A\x07\[\x1b]0;\u@\h:\w\a\]\x1b]633;B\x07",
    "\x1b]633;A\x07\\[\x1b]0;\\u@\\h:\\w\\a\\]\x1b]633;B\x07"
);

// ---------------------------------------------------------------------------------------------------------------------
// suite('serializeVSCodeOscMessage')
// ---------------------------------------------------------------------------------------------------------------------

/// `test(title, () => strictEqual(serializeVSCodeOscMessage(input), expected))`.
macro_rules! serialize_case {
    ($name:ident, $input:expr, $expected:expr) => {
        #[test]
        fn $name() {
            assert_eq!(serialize_vscode_osc_message($input), $expected);
        }
    };
}

serialize_case!(serialize_vscode_osc_message_empty, "", "");
serialize_case!(serialize_vscode_osc_message_basic, "value", "value");
serialize_case!(serialize_vscode_osc_message_space, "some thing", r"some\x20thing");
serialize_case!(serialize_vscode_osc_message_backslash, r"\", r"\\");
serialize_case!(serialize_vscode_osc_message_non_initial_backslash, r"foo\", r"foo\\");
serialize_case!(serialize_vscode_osc_message_two_backslashes, r"\\", r"\\\\");
serialize_case!(serialize_vscode_osc_message_backslash_amidst_text, r"Hello\there", r"Hello\\there");
serialize_case!(serialize_vscode_osc_message_semicolon, ";", r"\x3b");
serialize_case!(serialize_vscode_osc_message_non_initial_semicolon, "foo;", r"foo\x3b");
serialize_case!(serialize_vscode_osc_message_semicolon_amidst_text, "some;thing", r"some\x3bthing");
serialize_case!(serialize_vscode_osc_message_newline, "\n", r"\x0a");
serialize_case!(serialize_vscode_osc_message_non_initial_newline, "foo\n", r"foo\x0a");
serialize_case!(serialize_vscode_osc_message_newline_amidst_text, "some\nthing", r"some\x0athing");
serialize_case!(serialize_vscode_osc_message_tab_character, "\t", r"\x09");
serialize_case!(serialize_vscode_osc_message_carriage_return, "\r", r"\x0d");
serialize_case!(serialize_vscode_osc_message_null_character, "\x00", r"\x00");
serialize_case!(serialize_vscode_osc_message_space_character_0x20, " ", r"\x20");
serialize_case!(serialize_vscode_osc_message_character_above_0x20, "!", "!");
serialize_case!(serialize_vscode_osc_message_multiple_special_chars, "hello\nworld;test\\end", r"hello\x0aworld\x3btest\\end");
serialize_case!(
    serialize_vscode_osc_message_ps1_with_escape_sequences,
    "\x1b]633;A\x07\\[\x1b]0;\\u@\\h:\\w\\a\\]\x1b]633;B\x07",
    r"\x1b]633\x3bA\x07\\[\x1b]0\x3b\\u@\\h:\\w\\a\\]\x1b]633\x3bB\x07"
);

// ---------------------------------------------------------------------------------------------------------------------
// test('parseKeyValueAssignment')
// ---------------------------------------------------------------------------------------------------------------------

#[test]
fn parse_key_value_assignment_cases() {
    // [title, input, [key, value]]
    let cases: [(&str, &str, (&str, Option<&str>)); 9] = [
        ("empty", "", ("", None)),
        ("no \"=\" sign", "some-text", ("some-text", None)),
        ("empty value", "key=", ("key", Some(""))),
        ("empty key", "=value", ("", Some("value"))),
        ("normal", "key=value", ("key", Some("value"))),
        ("multiple \"=\" signs (1)", "key==value", ("key", Some("=value"))),
        ("multiple \"=\" signs (2)", "key=value===true", ("key", Some("value===true"))),
        ("just a \"=\"", "=", ("", Some(""))),
        ("just a \"==\"", "==", ("", Some("="))),
    ];

    for (title, input, (key, value)) in cases {
        assert_eq!(parse_key_value_assignment(input), KeyValueAssignment { key: key.to_owned(), value: value.map(str::to_owned) }, "{title}");
    }
}
