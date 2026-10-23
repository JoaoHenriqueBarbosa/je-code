//! Porte de `src/vs/platform/terminal/test/common/terminalRecorder.test.ts` (suíte `TerminalRecorder`), caso
//! a caso e na mesma ordem.
//!
//! Divergências: `ensureNoDisposablesAreLeakedInTestSuite` não tem o que vigiar (nada aqui é descartável);
//! `await recorder.generateReplayEvent()` lê o resultado da [`Promise`] já resolvida; `number` é `f64`.

use je_base::JsString;
use je_platform::terminal::terminal_recorder::{ReplayEntry, TerminalRecorder};

fn entry(cols: f64, rows: f64, data: &str) -> ReplayEntry {
    ReplayEntry { cols, rows, data: JsString::from(data) }
}

fn events_equal(recorder: &mut TerminalRecorder, expected: Vec<ReplayEntry>) {
    let actual = match recorder.generate_replay_event().result() {
        Some(Ok(event)) => event.events,
        _ => panic!("generateReplayEvent não resolveu"),
    };
    for (i, expected_entry) in expected.iter().enumerate() {
        assert_eq!(actual.get(i), Some(expected_entry));
    }
}

#[test]
fn should_record_dimensions() {
    let mut recorder = TerminalRecorder::new(1.0, 2.0);
    events_equal(&mut recorder, vec![entry(1.0, 2.0, "")]);
    recorder.handle_data(JsString::from("a"));
    recorder.handle_resize(3.0, 4.0);
    events_equal(&mut recorder, vec![entry(1.0, 2.0, "a"), entry(3.0, 4.0, "")]);
}

#[test]
fn should_ignore_resize_events_without_data() {
    let mut recorder = TerminalRecorder::new(1.0, 2.0);
    events_equal(&mut recorder, vec![entry(1.0, 2.0, "")]);
    recorder.handle_resize(3.0, 4.0);
    events_equal(&mut recorder, vec![entry(3.0, 4.0, "")]);
}

#[test]
fn should_record_data_and_combine_it_into_the_previous_resize_event() {
    let mut recorder = TerminalRecorder::new(1.0, 2.0);
    recorder.handle_data(JsString::from("a"));
    recorder.handle_data(JsString::from("b"));
    recorder.handle_resize(3.0, 4.0);
    recorder.handle_data(JsString::from("c"));
    recorder.handle_data(JsString::from("d"));
    events_equal(&mut recorder, vec![entry(1.0, 2.0, "ab"), entry(3.0, 4.0, "cd")]);
}
