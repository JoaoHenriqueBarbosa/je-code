//! Porte de `src/vs/workbench/contrib/terminal/test/browser/xterm/lineDataEventAddon.test.ts` (suíte `LineDataEventAddon`), caso a
//! caso e na mesma ordem.
//!
//! Notas de tradução:
//! - **`setup` e `ensureNoDisposablesAreLeakedInTestSuite()`.** O [`Fixture`] cria o guarda de vazamentos antes de tudo, o
//!   [`FakeXterm`] de 4 colunas (no lugar do `Terminal` do xterm com `cols: 4`, e 24 linhas, o padrão do xterm), o addon e o
//!   ouvinte de `onLineData` que acumula `events`. O `xterm.loadAddon(lineDataEventAddon)` é [`LineDataEventAddon::activate`], e
//!   o addon e o emulador são descartados pelo guarda (no original o `Terminal` descarta os addons carregados).
//! - **`importAMDNodeModule('@xterm/xterm', ...)` e `logger: TestXtermLogger`.** Não há módulo a importar nem logger a injetar: o
//!   emulador é o dublê.
//! - **`writeP(xterm, data)`.** É `FakeXterm::write_p`, que interpreta o texto de forma síncrona.
//! - **`deepStrictEqual(events, [...])`.** Compara o vetor de textos recebidos.
//! - **`new LineDataEventAddon()`.** Sem promessa de inicialização (`None`).

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod support;

use std::cell::RefCell;
use std::rc::Rc;

use common::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite};
use je_base::platform::OperatingSystem;
use je_workbench::contrib::terminal::browser::xterm::line_data_event_addon::{ILineFeedTerminal, LineDataEventAddon};
use support::fake_xterm::FakeXterm;

struct Fixture {
    xterm: Rc<FakeXterm>,
    line_data_event_addon: Rc<LineDataEventAddon>,
    events: Rc<RefCell<Vec<String>>>,
    // Declarado por último: o `Drop` dele descarta o que o `Fixture` registrou e confere os vazamentos.
    _store: LeakGuard,
}

impl Fixture {
    /// O `setup` da suíte `onLineData`.
    fn new() -> Self {
        let store = ensure_no_disposables_are_leaked_in_test_suite();
        let xterm = store.add_rc(FakeXterm::new(4, 24));
        let line_data_event_addon = store.add_rc(LineDataEventAddon::new(None));
        let terminal: Rc<dyn ILineFeedTerminal> = xterm.clone();
        line_data_event_addon.activate(terminal);

        let events: Rc<RefCell<Vec<String>>> = Rc::default();
        let sink = events.clone();
        store.add(line_data_event_addon.on_line_data().listen(move |e| sink.borrow_mut().push(e.clone())));
        Self { xterm, line_data_event_addon, events, _store: store }
    }

    /// `deepStrictEqual(events, expected)`.
    fn assert_events(&self, expected: &[&str]) {
        assert_eq!(*self.events.borrow(), expected.iter().map(|e| (*e).to_owned()).collect::<Vec<_>>());
    }
}

#[test]
fn should_fire_when_a_non_wrapped_line_ends_with_a_line_feed() {
    let f = Fixture::new();
    f.xterm.write_p("foo");
    f.assert_events(&[]);
    f.xterm.write_p("\n\r");
    f.assert_events(&["foo"]);
    f.xterm.write_p("bar");
    f.assert_events(&["foo"]);
    f.xterm.write_p("\n");
    f.assert_events(&["foo", "bar"]);
}

#[test]
fn should_not_fire_soft_wrapped_lines() {
    let f = Fixture::new();
    f.xterm.write_p("foo.");
    f.assert_events(&[]);
    f.xterm.write_p("bar.");
    f.assert_events(&[]);
    f.xterm.write_p("baz.");
    f.assert_events(&[]);
}

#[test]
fn should_fire_when_a_wrapped_line_ends_with_a_line_feed() {
    let f = Fixture::new();
    f.xterm.write_p("foo.bar.baz.");
    f.assert_events(&[]);
    f.xterm.write_p("\n\r");
    f.assert_events(&["foo.bar.baz."]);
}

#[test]
fn should_not_fire_on_cursor_move_when_the_backing_process_is_not_on_windows() {
    let f = Fixture::new();
    f.xterm.write_p("foo.\x1b[H");
    f.assert_events(&[]);
}

#[test]
fn should_fire_on_cursor_move_when_the_backing_process_is_on_windows() {
    let f = Fixture::new();
    f.line_data_event_addon.set_operating_system(OperatingSystem::Windows);
    f.xterm.write_p("foo\x1b[H");
    f.assert_events(&["foo"]);
}
