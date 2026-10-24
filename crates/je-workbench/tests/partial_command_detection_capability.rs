//! Porte de `src/vs/workbench/contrib/terminal/test/browser/capabilities/partialCommandDetectionCapability.test.ts` (suíte
//! `PartialCommandDetectionCapability`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - **`setup` e `ensureNoDisposablesAreLeakedInTestSuite()`.** O [`Fixture`] cria o guarda de vazamentos antes de tudo, o
//!   `Terminal` (o [`FakeXterm`] de 80 colunas e 24 linhas, o tamanho padrão do xterm, registrado no store), o `Emitter<void>`
//!   `onDidExecuteTextEmitter`, a capacidade e o ouvinte de `onCommandFinished` que acumula `addEvents`.
//! - **`writeP(xterm, data)`.** É `FakeXterm::write_p`, que interpreta o texto de forma síncrona.
//! - **`xterm.input(data)`.** O `input` do xterm entrega o texto ao `onData`; é `FakeXterm::fire_data`.
//! - **`e.line`.** Lê a linha de cada marcador (`f64`).

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod support;

use std::cell::RefCell;
use std::rc::Rc;

use common::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite};
use je_base::event::Emitter;
use je_platform::terminal::capabilities::capabilities::{IMarker, IPartialCommandDetectionCapability};
use je_platform::terminal::capabilities::partial_command_detection_capability::PartialCommandDetectionCapability;
use je_platform::terminal::xterm_headless::{IBuffer, IHeadlessTerminal};
use support::fake_xterm::FakeXterm;

struct Fixture {
    xterm: Rc<FakeXterm>,
    capability: Rc<PartialCommandDetectionCapability>,
    add_events: Rc<RefCell<Vec<Rc<dyn IMarker>>>>,
    on_did_execute_text_emitter: Rc<Emitter<()>>,
    // Declarado por último: o `Drop` dele descarta o que o `Fixture` registrou e confere os vazamentos.
    _store: LeakGuard,
}

impl Fixture {
    fn new() -> Self {
        let store = ensure_no_disposables_are_leaked_in_test_suite();
        let xterm = store.add_rc(FakeXterm::new(80, 24));
        let on_did_execute_text_emitter = store.add_rc(Emitter::<()>::new());
        let terminal: Rc<dyn IHeadlessTerminal> = xterm.clone();
        let capability = store.add_rc(PartialCommandDetectionCapability::new(terminal, Some(on_did_execute_text_emitter.event())));
        let add_events: Rc<RefCell<Vec<Rc<dyn IMarker>>>> = Rc::default();
        let sink = add_events.clone();
        store.add(capability.on_command_finished().listen(move |e| sink.borrow_mut().push(e.clone())));
        Self { xterm, capability, add_events, on_did_execute_text_emitter, _store: store }
    }

    fn assert_commands(&self, expected_lines: &[f64]) {
        assert_eq!(self.capability.commands().iter().map(|e| e.line()).collect::<Vec<_>>(), expected_lines);
        assert_eq!(self.add_events.borrow().iter().map(|e| e.line()).collect::<Vec<_>>(), expected_lines);
    }
}

#[test]
fn should_not_add_commands_when_the_cursor_position_is_too_close_to_the_left_side() {
    let f = Fixture::new();
    f.assert_commands(&[]);
    f.xterm.fire_data("\r");
    f.xterm.write_p("\r\n");
    f.assert_commands(&[]);
    f.xterm.write_p("a");
    f.xterm.fire_data("\r");
    f.xterm.write_p("\r\n");
    f.assert_commands(&[]);
}

#[test]
fn should_add_commands_when_the_cursor_position_is_not_too_close_to_the_left_side() {
    let f = Fixture::new();
    f.assert_commands(&[]);
    f.xterm.write_p("ab");
    f.xterm.fire_data("\r");
    f.xterm.write_p("\r\n\r\n");
    f.assert_commands(&[0.0]);
    f.xterm.write_p("cd");
    f.xterm.fire_data("\r");
    f.xterm.write_p("\r\n");
    f.assert_commands(&[0.0, 2.0]);
}

#[test]
fn on_did_execute_text_should_cause_on_did_command_finished_to_fire() {
    let f = Fixture::new();
    f.xterm.write_p("cd");
    f.on_did_execute_text_emitter.fire(());
    f.xterm.write_p("pwd");
    f.on_did_execute_text_emitter.fire(());
    assert_eq!(f.add_events.borrow().len(), 2);
}

#[test]
fn should_clear_all_commands_including_scrollback() {
    let f = Fixture::new();
    f.xterm.write_p("ab");
    f.xterm.fire_data("\r");
    f.xterm.write_p("\r\n\r\n");
    f.xterm.write_p("cd");
    f.xterm.fire_data("\r");
    f.xterm.write_p("\r\n");
    assert_eq!(f.capability.commands().iter().map(|e| e.line()).collect::<Vec<_>>(), vec![0.0, 2.0]);
    let rows = IHeadlessTerminal::rows(&*f.xterm);
    f.xterm.write_p(&"line\r\n".repeat(usize::try_from(rows).unwrap()));
    assert!(IHeadlessTerminal::buffer_active(&*f.xterm).base_y() > 0);

    f.capability.clear_commands();

    assert!(f.capability.commands().is_empty());
}
