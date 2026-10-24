//! Porte de `src/vs/workbench/contrib/terminalContrib/accessibility/test/browser/bufferContentTracker.test.ts` (suíte `Buffer Content Tracker`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - **`setup` e `ensureNoDisposablesAreLeakedInTestSuite()`.** O [`Fixture`] cria o guarda de vazamentos antes de tudo, o
//!   `TestConfigurationService` com `terminal.integrated.scrollback` igual a 1000, o `FakeXterm` de 80 colunas por 30 linhas com
//!   esse scrollback (no lugar do `Terminal` do xterm dentro do `XtermTerminal`) e o `BufferContentTracker`. O `XtermTerminal`
//!   do original só entrega o `raw` ao rastreador (o `getFont` não é lido), então o rastreador recebe o `FakeXterm`.
//! - **`xterm.clearBuffer()`.** O `clearBuffer` do `XtermTerminal` é o `raw.clear()` mais os gatilhos da detecção de comandos
//!   (`handlePromptStart` e `handleCommandStart`), que não existem neste teste (só há `NaiveCwdDetection`): é `FakeXterm::clear`.
//! - **`writeP(xterm.raw, text)`.** É `FakeXterm::write_p`.
//! - **`capabilities.add(TerminalCapability.NaiveCwdDetection, null!)` e o `container`.** O rastreador não os lê (a capacidade
//!   serve ao `XtermTerminal`, e o `xterm.raw.open(container)` ao renderizador), então não entram.
//! - **`\x1b[nA`.** O `FakeXterm` lê o `CSI n A` (sobe o cursor), que os casos de atualização do viewport usam.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod support;

use std::rc::Rc;

use common::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite};
use je_base::dynamic::Dynamic;
use je_platform::configuration::test_configuration_service::TestConfigurationService;
use je_platform::log::NullLogService;
use je_workbench::contrib::terminal::browser::xterm::xterm_terminal::XtermOptions;
use je_workbench::contrib::terminal_contrib::accessibility::buffer_content_tracker::BufferContentTracker;
use support::fake_xterm::FakeXterm;

const PROMPT: &str = "vscode-git:(prompt/more-tests)";
const PROMPT_PLUS_DATA: &str = "vscode-git:(prompt/more-tests) some data";

/// O `setup` da suíte.
struct Fixture {
    xterm: Rc<FakeXterm>,
    buffer_tracker: Rc<BufferContentTracker>,
    // Declarado por último: o `Drop` dele descarta o que o `Fixture` registrou e confere os vazamentos.
    _guard: LeakGuard,
}

impl Fixture {
    fn new() -> Self {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        // `defaultTerminalConfig` (só o `scrollback` é lido pelo rastreador)
        let configuration_service = TestConfigurationService::new(Some(Dynamic::object([(
            "terminal",
            Dynamic::object([("integrated", Dynamic::object([("scrollback", Dynamic::Number(1000.0))]))]),
        )])));
        let mut options = XtermOptions::defaults(80, 30);
        options.scrollback = 1000.0;
        let xterm = guard.add_rc(FakeXterm::from_options(&options));
        let buffer_tracker = guard.add_rc(BufferContentTracker::new(xterm.clone(), configuration_service, NullLogService::shared()));
        Self { xterm, buffer_tracker, _guard: guard }
    }

    /// `writeAndAssertBufferState(data, rows, terminal, bufferTracker)`.
    fn write_and_assert_buffer_state(&self, data: &str, rows: usize) {
        let content = format!("{data}\r\n").repeat(rows).trim_end().to_owned();
        self.xterm.write_p(&content);
        self.buffer_tracker.update();
        assert_eq!(self.buffer_tracker.lines().len(), rows);
        assert_eq!(self.buffer_tracker.lines(), content.split("\r\n").map(str::to_owned).collect::<Vec<String>>());
    }
}

#[test]
fn should_not_clear_the_prompt_line() {
    let f = Fixture::new();
    assert_eq!(f.buffer_tracker.lines().len(), 0);
    f.xterm.write_p(PROMPT);
    f.xterm.clear();
    f.buffer_tracker.update();
    assert_eq!(f.buffer_tracker.lines(), vec![PROMPT.to_owned()]);
}

#[test]
fn repeated_updates_should_not_change_the_content() {
    let f = Fixture::new();
    assert_eq!(f.buffer_tracker.lines().len(), 0);
    f.xterm.write_p(PROMPT);
    f.buffer_tracker.update();
    assert_eq!(f.buffer_tracker.lines(), vec![PROMPT.to_owned()]);
    f.buffer_tracker.update();
    assert_eq!(f.buffer_tracker.lines(), vec![PROMPT.to_owned()]);
    f.buffer_tracker.update();
    assert_eq!(f.buffer_tracker.lines(), vec![PROMPT.to_owned()]);
}

#[test]
fn should_add_lines_in_the_viewport_and_scrollback() {
    let f = Fixture::new();
    f.write_and_assert_buffer_state(PROMPT_PLUS_DATA, 38);
}

#[test]
fn should_add_lines_in_the_viewport_and_full_scrollback() {
    let f = Fixture::new();
    f.write_and_assert_buffer_state(PROMPT_PLUS_DATA, 1030);
}

#[test]
fn should_refresh_viewport() {
    let f = Fixture::new();
    f.write_and_assert_buffer_state(PROMPT_PLUS_DATA, 6);
    f.xterm.write_p("\x1b[3Ainserteddata");
    f.buffer_tracker.update();
    let inserted = format!("{PROMPT_PLUS_DATA}inserteddata");
    let expected: Vec<String> = [PROMPT_PLUS_DATA, PROMPT_PLUS_DATA, inserted.as_str(), PROMPT_PLUS_DATA, PROMPT_PLUS_DATA, PROMPT_PLUS_DATA]
        .iter()
        .map(|line| (*line).to_owned())
        .collect();
    assert_eq!(f.buffer_tracker.lines(), expected);
}

#[test]
fn should_refresh_viewport_with_full_scrollback() {
    let f = Fixture::new();
    let content = format!("{PROMPT}\r\n").repeat(1030).trim_end().to_owned();
    f.xterm.write_p(&content);
    f.buffer_tracker.update();
    f.xterm.write_p("\x1b[4Ainsertion");
    f.buffer_tracker.update();
    let mut expected: Vec<String> = content.split("\r\n").map(str::to_owned).collect();
    expected[1025] = format!("{PROMPT}insertion");
    assert_eq!(f.buffer_tracker.lines()[1025], format!("{PROMPT}insertion"));
}

#[test]
fn should_cap_the_size_of_the_cached_lines_removing_old_lines_in_favor_of_new_lines() {
    let f = Fixture::new();
    let content = format!("{PROMPT}\r\n").repeat(1036).trim_end().to_owned();
    f.xterm.write_p(&content);
    f.buffer_tracker.update();
    let mut expected: Vec<String> = content.split("\r\n").map(str::to_owned).collect();
    // delete the 6 lines that should be trimmed
    for _ in 0..6 {
        expected.pop();
    }
    // insert a new character
    f.xterm.write_p("\x1b[2Ainsertion");
    f.buffer_tracker.update();
    expected[1027] = format!("{PROMPT}insertion");
    assert_eq!(f.buffer_tracker.lines().len(), expected.len());
    assert_eq!(f.buffer_tracker.lines(), expected);
}
