//! Porte de `src/vs/workbench/contrib/terminalContrib/typeAhead/test/browser/terminalTypeAhead.test.ts` (suíte `Workbench - Terminal Typeahead`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - **`createMockTerminal`.** O `Terminal` falso do original (um literal com `cols`, `rows`, `onData`, `onTitleChange`, `parser.registerCsiHandler`, `write`,
//!   `_core._inputHandler._curAttrData` e `buffer.active`) é o [`MockTerminal`], que implementa o recorte `ITypeAheadTerminal` que o addon lê. As linhas, o cursor
//!   (1-based, como no original), o `getCell(x)` com `s[x - 1]` e o `translateToString` são os do original, e o `mockCell` (um `Proxy`) é o [`MockCell`]: os
//!   `is*` valem `false` e os `get*` valem 0, exceto o `isAttributeDefault` (`true`) e o que o `cursorAttrs` sobrescreve. O `registerCsiHandler` do original
//!   registra o ouvinte no `ds` do mock e devolve `undefined`; o porte devolve um descarte vazio.
//! - **`setup` e `ensureNoDisposablesAreLeakedInTestSuite()`.** O [`Fixture`] cria o guarda, o emissor `onBeforeProcessData`, o `TestConfigurationService` com
//!   `terminal.integrated` e o addon (a subclasse `TestTypeAheadAddon` do original expõe os membros `protected`, que no porte são públicos), e o `teardown` descarta
//!   o addon.
//! - **`ITerminalProcessManager` parcial.** O addon recebe só o evento `onBeforeProcessData`; o `ITelemetryService` com `publicLog` do `sinon` é o serviço nulo.
//! - **`expectProcessed`.** `JSON.stringify` dos dois lados é a comparação direta das strings.
//! - **`useFakeTimers()` / `clock.tick(n)`.** Os casos de `PredictionStats` rodam em `run_with_faked_timers` e `clock.tick` é `runtime::advance_fake_time`.
//! - **`createPredictionStubs`.** O literal `{ apply, rollback, matches, rollForwards }` é a [`StubPrediction`].
//! - **`new PredictionStats({ onPredictionAdded, ... } as any)`.** O construtor do porte recebe os três eventos.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use common::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite, run_with_faked_timers};
use je_base::dynamic::Dynamic;
use je_base::event::{Emitter, Event};
use je_base::lifecycle::{DisposableRef, DisposableStore, IDisposable, plain_disposable};
use je_base::runtime;
use je_platform::configuration::test_configuration_service::TestConfigurationService;
use je_platform::telemetry::telemetry_utils::null_telemetry_service;
use je_workbench::contrib::terminal::common::terminal::IBeforeProcessDataEvent;
use je_workbench::contrib::terminal_contrib::type_ahead::browser::terminal_type_ahead_addon::{
    CharPredictState, Cursor, IPrediction, ITypeAheadBuffer, ITypeAheadCell, ITypeAheadLine, ITypeAheadTerminal, IXtermAttributes, LastRow, LatencyStats, MatchResult,
    PredictionStats, SgrParam, StringReader, TypeAheadAddon,
};
use je_workbench::contrib::terminal_contrib::type_ahead::common::terminal_type_ahead_configuration::DEFAULT_LOCAL_ECHO_EXCLUDE;

const CSI: &str = "\x1b[";

/// `const enum CursorMoveDirection`.
const CURSOR_MOVE_BACK: &str = "D";
const CURSOR_MOVE_FORWARDS: &str = "C";

/// O `mockCell(char, attrs)`.
#[derive(Clone, Default)]
struct CellAttrs {
    is_attribute_default: Option<bool>,
    is_bold: bool,
    is_fg_palette: bool,
    fg_color: u32,
}

struct MockCell {
    chars: String,
    attrs: CellAttrs,
}

impl IXtermAttributes for MockCell {
    fn is_attribute_default(&self) -> bool {
        self.attrs.is_attribute_default.unwrap_or(true)
    }

    fn is_bold(&self) -> bool {
        self.attrs.is_bold
    }

    fn is_dim(&self) -> bool {
        false
    }

    fn is_italic(&self) -> bool {
        false
    }

    fn is_underline(&self) -> bool {
        false
    }

    fn is_blink(&self) -> bool {
        false
    }

    fn is_inverse(&self) -> bool {
        false
    }

    fn is_invisible(&self) -> bool {
        false
    }

    fn is_fg_rgb(&self) -> bool {
        false
    }

    fn is_fg_palette(&self) -> bool {
        self.attrs.is_fg_palette
    }

    fn is_fg_default(&self) -> bool {
        false
    }

    fn get_fg_color(&self) -> u32 {
        self.attrs.fg_color
    }

    fn is_bg_rgb(&self) -> bool {
        false
    }

    fn is_bg_palette(&self) -> bool {
        false
    }

    fn is_bg_default(&self) -> bool {
        false
    }

    fn get_bg_color(&self) -> u32 {
        0
    }
}

impl ITypeAheadCell for MockCell {
    fn get_chars(&self) -> String {
        self.chars.clone()
    }

    fn get_code(&self) -> u32 {
        self.chars.encode_utf16().next().map_or(0, u32::from)
    }
}

/// `{ length, getCell, translateToString }` de `buffer.active.getLine(y)`.
struct MockLine {
    s: Vec<char>,
}

impl ITypeAheadLine for MockLine {
    fn translate_to_string(&self, trim_right: Option<bool>, start_column: Option<i32>, end_column: Option<i32>) -> String {
        let start = usize::try_from(start_column.unwrap_or(0)).unwrap_or(0).min(self.s.len());
        let end = usize::try_from(end_column.unwrap_or(self.s.len() as i32)).unwrap_or(0).min(self.s.len());
        let out: String = if start < end { self.s[start..end].iter().collect() } else { String::new() };
        if trim_right == Some(true) { out.trim_end().to_owned() } else { out }
    }

    fn get_cell(&self, x: i32) -> Option<Rc<dyn ITypeAheadCell>> {
        let chars = usize::try_from(x - 1).ok().and_then(|index| self.s.get(index)).map(char::to_string).unwrap_or_default();
        Some(Rc::new(MockCell { chars, attrs: CellAttrs::default() }))
    }
}

/// O cursor compartilhado do mock (`const cursor = { y: 1, x: 1 }`).
struct MockCursor {
    x: Cell<i32>,
    y: Cell<i32>,
}

struct MockBuffer {
    lines: Vec<String>,
    cursor: Rc<MockCursor>,
}

impl ITypeAheadBuffer for MockBuffer {
    fn is_normal(&self) -> bool {
        true
    }

    fn base_y(&self) -> i32 {
        0
    }

    fn cursor_x(&self) -> i32 {
        self.cursor.x.get()
    }

    fn cursor_y(&self) -> i32 {
        self.cursor.y.get()
    }

    fn get_line(&self, y: i32) -> Option<Rc<dyn ITypeAheadLine>> {
        let s = usize::try_from(y - 1).ok().and_then(|index| self.lines.get(index)).cloned().unwrap_or_default();
        Some(Rc::new(MockLine { s: s.chars().collect() }))
    }
}

struct MockTerminal {
    buffer: Rc<MockBuffer>,
    written: Rc<RefCell<Vec<String>>>,
    on_title_change: Rc<Emitter<String>>,
    on_data: Rc<Emitter<String>>,
    csi_emitter: Rc<Emitter<Vec<SgrParam>>>,
    cur_attr_data: Rc<MockCell>,
    ds: Rc<DisposableStore>,
}

impl ITypeAheadTerminal for MockTerminal {
    fn cols(&self) -> i32 {
        80
    }

    fn rows(&self) -> i32 {
        5
    }

    fn buffer_active(&self) -> Rc<dyn ITypeAheadBuffer> {
        self.buffer.clone()
    }

    fn write(&self, data: &str) {
        self.written.borrow_mut().push(data.to_owned());
    }

    fn on_data(&self) -> Event<String> {
        self.on_data.event()
    }

    fn on_title_change(&self) -> Event<String> {
        self.on_title_change.event()
    }

    fn on_resize(&self) -> Event<()> {
        Event::none()
    }

    fn register_csi_handler_sgr(&self, handler: Rc<dyn Fn(&[SgrParam]) -> bool>) -> DisposableRef {
        self.ds.add(self.csi_emitter.event().listen(move |args| {
            handler(args);
        }));
        plain_disposable(|| {})
    }

    fn cur_attr_data(&self) -> Rc<dyn IXtermAttributes> {
        self.cur_attr_data.clone()
    }
}

/// O que `createMockTerminal` devolve.
struct MockTerminalHandle {
    written: Rc<RefCell<Vec<String>>>,
    cursor: Rc<MockCursor>,
    on_title_change: Rc<Emitter<String>>,
    on_data: Rc<Emitter<String>>,
    terminal: Rc<MockTerminal>,
    ds: Rc<DisposableStore>,
}

impl IDisposable for MockTerminalHandle {
    fn dispose(&self) {
        self.ds.dispose();
    }
}

impl MockTerminalHandle {
    /// `expectWritten(s)`.
    fn expect_written(&self, s: &str) {
        assert_eq!(self.written.borrow().join(""), s);
        self.written.borrow_mut().clear();
    }

    fn clear_written(&self) {
        self.written.borrow_mut().clear();
    }

    /// `onData(s)`.
    fn on_data(&self, s: &str) {
        self.on_data.fire(s.to_owned());
    }
}

/// `createMockTerminal({ lines, cursorAttrs })`.
fn create_mock_terminal(guard: &LeakGuard, lines: &[&str], cursor_attrs: Option<CellAttrs>) -> Rc<MockTerminalHandle> {
    let ds = DisposableStore::new();
    let written: Rc<RefCell<Vec<String>>> = Rc::default();
    let cursor = Rc::new(MockCursor { x: Cell::new(1), y: Cell::new(1) });
    let on_title_change = Emitter::new();
    ds.add_rc(on_title_change.clone());
    let on_data = Emitter::new();
    ds.add_rc(on_data.clone());
    let csi_emitter = Emitter::new();
    ds.add_rc(csi_emitter.clone());

    let mut lines: Vec<String> = lines.iter().map(|line| (*line).to_owned()).collect();
    for y in 0..lines.len() {
        if let Some(index) = lines[y].find('|') {
            cursor.y.set(y as i32 + 1);
            cursor.x.set(lines[y][..index].chars().count() as i32 + 1);
            lines[y] = lines[y].replacen('|', "", 1);
            break;
        }
    }

    let buffer = Rc::new(MockBuffer { lines, cursor: cursor.clone() });
    let terminal = Rc::new(MockTerminal {
        buffer,
        written: written.clone(),
        on_title_change: on_title_change.clone(),
        on_data: on_data.clone(),
        csi_emitter,
        cur_attr_data: Rc::new(MockCell { chars: String::new(), attrs: cursor_attrs.unwrap_or_default() }),
        ds: ds.clone(),
    });
    guard.add_rc(Rc::new(MockTerminalHandle { written, cursor, on_title_change, on_data, terminal, ds }))
}

/// `stubPrediction()`.
struct StubPrediction;

impl IPrediction for StubPrediction {
    fn apply(&self, _buffer: &Rc<dyn ITypeAheadBuffer>, _cursor: &Cursor) -> String {
        String::new()
    }

    fn rollback(&self, _cursor: &Cursor) -> String {
        String::new()
    }

    fn matches(&self, _input: &mut StringReader, _look_behind: Option<&Rc<dyn IPrediction>>) -> MatchResult {
        MatchResult::Success
    }

    fn roll_forwards(&self, _cursor: &Cursor, _with_input: &str) -> String {
        String::new()
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

/// `createPredictionStubs(n)`.
fn create_prediction_stubs(n: usize) -> Vec<Rc<dyn IPrediction>> {
    (0..n).map(|_| Rc::new(StubPrediction) as Rc<dyn IPrediction>).collect()
}

// ---------------------------------------------------------------------------------------------------------------------
// suite('PredictionStats')
// ---------------------------------------------------------------------------------------------------------------------

/// O `setup` da suíte `PredictionStats`.
struct StatsFixture {
    add: Rc<Emitter<Rc<dyn IPrediction>>>,
    succeed: Rc<Emitter<Rc<dyn IPrediction>>>,
    fail: Rc<Emitter<Rc<dyn IPrediction>>>,
    stats: Rc<PredictionStats>,
    _guard: LeakGuard,
}

impl StatsFixture {
    fn new() -> Self {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let add = guard.add_rc(Emitter::new());
        let succeed = guard.add_rc(Emitter::new());
        let fail = guard.add_rc(Emitter::new());
        let stats = guard.add_rc(PredictionStats::new(add.event(), succeed.event(), fail.event()));
        Self { add, succeed, fail, stats, _guard: guard }
    }
}

#[test]
fn prediction_stats_creates_sane_data() {
    run_with_faked_timers(async {
        let f = StatsFixture::new();
        let stubs = create_prediction_stubs(5);
        for s in &stubs {
            f.add.fire(s.clone());
        }

        for (i, stub) in stubs.iter().enumerate() {
            runtime::advance_fake_time(100);
            (if i % 2 == 1 { &f.fail } else { &f.succeed }).fire(stub.clone());
        }

        assert_eq!(f.stats.accuracy(), 3.0 / 5.0);
        assert_eq!(f.stats.sample_size(), 5);
        assert_eq!(f.stats.latency(), LatencyStats { count: 3, min: Some(100.0), max: Some(500.0), median: Some(300.0) });
    });
}

#[test]
fn prediction_stats_sorts_latencies_numerically() {
    run_with_faked_timers(async {
        let f = StatsFixture::new();
        let stubs = create_prediction_stubs(3);
        for s in &stubs {
            f.add.fire(s.clone());
        }

        runtime::advance_fake_time(5);
        f.succeed.fire(stubs[0].clone());
        runtime::advance_fake_time(45);
        f.succeed.fire(stubs[1].clone());
        runtime::advance_fake_time(150);
        f.succeed.fire(stubs[2].clone());

        assert_eq!(f.stats.latency(), LatencyStats { count: 3, min: Some(5.0), max: Some(200.0), median: Some(50.0) });
    });
}

#[test]
fn prediction_stats_circular_buffer() {
    let f = StatsFixture::new();
    let buffer_size = 24;
    let stubs = create_prediction_stubs(buffer_size * 2);

    for s in &stubs[..buffer_size] {
        f.add.fire(s.clone());
        f.succeed.fire(s.clone());
    }
    assert_eq!(f.stats.accuracy(), 1.0);

    for s in &stubs[buffer_size..buffer_size * 3 / 2] {
        f.add.fire(s.clone());
        f.fail.fire(s.clone());
    }
    assert_eq!(f.stats.accuracy(), 0.5);

    for s in &stubs[buffer_size * 3 / 2..] {
        f.add.fire(s.clone());
        f.fail.fire(s.clone());
    }
    assert_eq!(f.stats.accuracy(), 0.0);
}

// ---------------------------------------------------------------------------------------------------------------------
// suite('timeline')
// ---------------------------------------------------------------------------------------------------------------------

/// `predictedHelloo`.
fn predicted_helloo() -> String {
    [
        format!("{CSI}?25l"), // hide cursor
        format!("{CSI}2;7H"), // move cursor
        "o".to_owned(),       // new character
        format!("{CSI}2;8H"), // place cursor back at end of line
        format!("{CSI}?25h"), // show cursor
    ]
    .join("")
}

/// O `setup` da suíte `timeline` (e o `teardown`, no `Drop`).
struct Fixture {
    on_before_process_data: Rc<Emitter<Rc<IBeforeProcessDataEvent>>>,
    addon: TypeAheadAddon,
    // Declarado por último: o `Drop` dele descarta o que o `Fixture` registrou e confere os vazamentos.
    guard: LeakGuard,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        self.addon.dispose();
    }
}

impl Fixture {
    fn new() -> Self {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let on_before_process_data = guard.add_rc(Emitter::new());
        let configuration = TestConfigurationService::new(Some(Dynamic::object([(
            "terminal",
            Dynamic::object([(
                "integrated",
                Dynamic::object([
                    ("localEchoStyle", Dynamic::string("italic")),
                    ("localEchoLatencyThreshold", Dynamic::Number(0.0)),
                    ("localEchoExcludePrograms", Dynamic::array(DEFAULT_LOCAL_ECHO_EXCLUDE.iter().map(|program| Dynamic::string(program)))),
                ]),
            )]),
        )])));
        guard.add_rc(configuration.on_did_change_configuration_emitter.clone());
        let addon = TypeAheadAddon::new(on_before_process_data.event(), configuration, null_telemetry_service());
        let fixture = Self { on_before_process_data, addon, guard };
        fixture.unlock_making_predictions();
        fixture
    }

    /// `createMockTerminal({ lines, cursorAttrs })`, registrado no guarda (`ds.add(...)`).
    fn mock(&self, lines: &[&str], cursor_attrs: Option<CellAttrs>) -> Rc<MockTerminalHandle> {
        create_mock_terminal(&self.guard, lines, cursor_attrs)
    }

    fn unlock_making_predictions(&self) {
        self.addon.set_last_row(Some(LastRow { y: 1, starting_x: 100, ending_x: 100, char_state: CharPredictState::Validated }));
    }

    fn lock_making_predictions(&self) {
        self.addon.set_last_row(None);
    }

    fn unlock_navigating(&self) {
        self.addon.set_last_row(Some(LastRow { y: 1, starting_x: 1, ending_x: 1, char_state: CharPredictState::Validated }));
    }

    fn is_showing(&self) -> bool {
        self.addon.timeline().is_some_and(|timeline| timeline.is_showing_predictions())
    }

    fn undo_all_predictions(&self) {
        if let Some(timeline) = self.addon.timeline() {
            timeline.undo_all_predictions();
        }
    }

    fn physical_cursor_x(&self, t: &MockTerminalHandle) -> Option<i32> {
        self.addon.timeline().map(|timeline| timeline.physical_cursor(&t.terminal.buffer_active()).x())
    }

    fn accuracy(&self) -> Option<f64> {
        self.addon.stats().map(|stats| stats.accuracy())
    }

    /// `activate(t.terminal)`.
    fn activate(&self, t: &MockTerminalHandle) {
        self.addon.activate(t.terminal.clone());
    }

    /// `expectProcessed(input, output)`.
    fn expect_processed(&self, input: &str, output: &str) {
        let evt = Rc::new(IBeforeProcessDataEvent { data: RefCell::new(input.to_owned()) });
        self.on_before_process_data.fire(evt.clone());
        assert_eq!(*evt.data.borrow(), output);
    }

    /// `onBeforeProcessData.fire({ data })`.
    fn fire_before_process_data(&self, data: &str) {
        self.on_before_process_data.fire(Rc::new(IBeforeProcessDataEvent { data: RefCell::new(data.to_owned()) }));
    }
}

#[test]
fn timeline_predicts_a_single_character() {
    let f = Fixture::new();
    let t = f.mock(&["hello|"], None);
    f.activate(&t);
    t.on_data("o");
    t.expect_written(&format!("{CSI}3mo{CSI}23m"));
}

#[test]
fn timeline_validates_character_prediction() {
    let f = Fixture::new();
    let t = f.mock(&["hello|"], None);
    f.activate(&t);
    t.on_data("o");
    f.expect_processed("o", &predicted_helloo());
    assert_eq!(f.accuracy(), Some(1.0));
}

#[test]
fn timeline_validates_zsh_prediction_112842() {
    let f = Fixture::new();
    let t = f.mock(&["hello|"], None);
    f.activate(&t);
    t.on_data("o");
    f.expect_processed("o", &predicted_helloo());

    t.on_data("x");
    f.expect_processed(
        "\u{8}ox",
        &[
            format!("{CSI}?25l"), // hide cursor
            format!("{CSI}2;8H"), // move cursor
            "\u{8}ox".to_owned(), // new data
            format!("{CSI}2;9H"), // place cursor back at end of line
            format!("{CSI}?25h"), // show cursor
        ]
        .join(""),
    );
    assert_eq!(f.accuracy(), Some(1.0));
}

#[test]
fn timeline_does_not_validate_zsh_prediction_on_differing_lookbehindn_112842() {
    let f = Fixture::new();
    let t = f.mock(&["hello|"], None);
    f.activate(&t);
    t.on_data("o");
    f.expect_processed("o", &predicted_helloo());

    t.on_data("x");
    f.expect_processed(
        "\u{8}qx",
        &[
            format!("{CSI}?25l"), // hide cursor
            format!("{CSI}2;8H"), // move cursor cursor
            format!("{CSI}X"),    // delete character
            format!("{CSI}0m"),   // reset style
            "\u{8}qx".to_owned(), // new data
            format!("{CSI}?25h"), // show cursor
        ]
        .join(""),
    );
    assert_eq!(f.accuracy(), Some(0.5));
}

#[test]
fn timeline_rolls_back_character_prediction() {
    let f = Fixture::new();
    let t = f.mock(&["hello|"], None);
    f.activate(&t);
    t.on_data("o");

    f.expect_processed(
        "q",
        &[
            format!("{CSI}?25l"), // hide cursor
            format!("{CSI}2;7H"), // move cursor cursor
            format!("{CSI}X"),    // delete character
            format!("{CSI}0m"),   // reset style
            "q".to_owned(),       // new character
            format!("{CSI}?25h"), // show cursor
        ]
        .join(""),
    );
    assert_eq!(f.accuracy(), Some(0.0));
}

#[test]
fn timeline_handles_left_arrow_when_we_hit_the_boundary() {
    let f = Fixture::new();
    let t = f.mock(&["|"], None);
    f.activate(&t);
    f.unlock_navigating();

    let cursor_x_before = f.physical_cursor_x(&t);
    t.on_data(&format!("{CSI}{CURSOR_MOVE_BACK}"));
    t.expect_written("");

    // Trigger rollback because we don't expect this data
    f.fire_before_process_data("xy");

    // The cursor should not have changed because we've hit the boundary (start of prompt)
    assert_eq!(f.physical_cursor_x(&t), cursor_x_before);
}

#[test]
fn timeline_handles_right_arrow_when_we_hit_the_boundary() {
    let f = Fixture::new();
    let t = f.mock(&["|"], None);
    f.activate(&t);
    f.unlock_navigating();

    let cursor_x_before = f.physical_cursor_x(&t);
    t.on_data(&format!("{CSI}{CURSOR_MOVE_FORWARDS}"));
    t.expect_written("");

    // Trigger rollback because we don't expect this data
    f.fire_before_process_data("xy");

    // The cursor should not have changed because we've hit the boundary (end of prompt)
    assert_eq!(f.physical_cursor_x(&t), cursor_x_before);
}

#[test]
fn timeline_internal_cursor_state_is_reset_when_all_predictions_are_undone() {
    let f = Fixture::new();
    let t = f.mock(&["|"], None);
    f.activate(&t);
    f.unlock_navigating();

    let cursor_x_before = f.physical_cursor_x(&t);
    t.on_data(&format!("{CSI}{CURSOR_MOVE_BACK}"));
    t.expect_written("");
    f.undo_all_predictions();

    // The cursor should not have changed because we've hit the boundary (start of prompt)
    assert_eq!(f.physical_cursor_x(&t), cursor_x_before);
}

#[test]
fn timeline_restores_cursor_graphics_mode() {
    let f = Fixture::new();
    let t = f.mock(&["hello|"], Some(CellAttrs { is_attribute_default: Some(false), is_bold: true, is_fg_palette: true, fg_color: 1 }));
    f.activate(&t);
    t.on_data("o");

    f.expect_processed(
        "q",
        &[
            format!("{CSI}?25l"),     // hide cursor
            format!("{CSI}2;7H"),     // move cursor cursor
            format!("{CSI}X"),        // delete character
            format!("{CSI}1;38;5;1m"), // reset style
            "q".to_owned(),           // new character
            format!("{CSI}?25h"),     // show cursor
        ]
        .join(""),
    );
    assert_eq!(f.accuracy(), Some(0.0));
}

#[test]
fn timeline_validates_against_and_applies_graphics_mode_on_predicted() {
    let f = Fixture::new();
    let t = f.mock(&["hello|"], None);
    f.activate(&t);
    t.on_data("o");
    f.expect_processed(
        &format!("{CSI}4mo"),
        &[
            format!("{CSI}?25l"), // hide cursor
            format!("{CSI}2;7H"), // move cursor
            format!("{CSI}4m"),   // new PTY's style
            "o".to_owned(),       // new character
            format!("{CSI}2;8H"), // place cursor back at end of line
            format!("{CSI}?25h"), // show cursor
        ]
        .join(""),
    );
    assert_eq!(f.accuracy(), Some(1.0));
}

#[test]
fn timeline_ignores_cursor_hides_or_shows() {
    let f = Fixture::new();
    let t = f.mock(&["hello|"], None);
    f.activate(&t);
    t.on_data("o");
    f.expect_processed(
        &format!("{CSI}?25lo{CSI}?25h"),
        &[
            format!("{CSI}?25l"), // hide cursor from PTY
            format!("{CSI}?25l"), // hide cursor
            format!("{CSI}2;7H"), // move cursor
            "o".to_owned(),       // new character
            format!("{CSI}?25h"), // show cursor from PTY
            format!("{CSI}2;8H"), // place cursor back at end of line
            format!("{CSI}?25h"), // show cursor
        ]
        .join(""),
    );
    assert_eq!(f.accuracy(), Some(1.0));
}

#[test]
fn timeline_matches_backspace_at_eol_bash_style() {
    let f = Fixture::new();
    let t = f.mock(&["hello|"], None);
    f.activate(&t);
    t.on_data("\x7F");
    f.expect_processed(&format!("\u{8}{CSI}K"), &format!("\u{8}{CSI}K"));
    assert_eq!(f.accuracy(), Some(1.0));
}

#[test]
fn timeline_matches_backspace_at_eol_zsh_style() {
    let f = Fixture::new();
    let t = f.mock(&["hello|"], None);
    f.activate(&t);
    t.on_data("\x7F");
    f.expect_processed("\u{8} \u{8}", "\u{8} \u{8}");
    assert_eq!(f.accuracy(), Some(1.0));
}

#[test]
fn timeline_gradually_matches_backspace() {
    let f = Fixture::new();
    let t = f.mock(&["hello|"], None);
    f.activate(&t);
    t.on_data("\x7F");
    f.expect_processed("\u{8}", "");
    f.expect_processed(" \u{8}", "\u{8} \u{8}");
    assert_eq!(f.accuracy(), Some(1.0));
}

#[test]
fn timeline_restores_old_character_after_invalid_backspace() {
    let f = Fixture::new();
    let t = f.mock(&["hel|lo"], None);
    f.activate(&t);
    f.unlock_navigating();
    t.on_data("\x7F");
    t.expect_written(&format!("{CSI}2;4H{CSI}X"));
    f.expect_processed("x", &format!("{CSI}?25l{CSI}0ml{CSI}2;5H{CSI}0mx{CSI}?25h"));
    assert_eq!(f.accuracy(), Some(0.0));
}

#[test]
fn timeline_waits_for_validation_before_deleting_to_left_of_cursor() {
    let f = Fixture::new();
    let t = f.mock(&["hello|"], None);
    f.activate(&t);

    // initially should not backspace (until the server confirms it)
    t.on_data("\x7F");
    t.expect_written("");
    f.expect_processed("\u{8} \u{8}", "\u{8} \u{8}");
    t.cursor.x.set(t.cursor.x.get() - 1);

    // enter input on the column...
    t.on_data("o");
    f.fire_before_process_data("o");
    t.cursor.x.set(t.cursor.x.get() + 1);
    t.clear_written();

    // now that the column is 'unlocked', we should be able to predict backspace on it
    t.on_data("\x7F");
    t.expect_written(&format!("{CSI}2;6H{CSI}X"));
}

#[test]
fn timeline_waits_for_first_valid_prediction_on_a_line() {
    let f = Fixture::new();
    let t = f.mock(&["hello|"], None);
    f.lock_making_predictions();
    f.activate(&t);

    t.on_data("o");
    t.expect_written("");
    f.expect_processed("o", "o");

    t.on_data("o");
    t.expect_written(&format!("{CSI}3mo{CSI}23m"));
}

#[test]
fn timeline_disables_on_title_change() {
    let f = Fixture::new();
    let t = f.mock(&["hello|"], None);
    f.activate(&t);

    f.addon.reevaluate_predictor_state_now();
    assert!(f.is_showing(), "expected to show initially");

    t.on_title_change.fire("foo - VIM.exe".to_owned());
    f.addon.reevaluate_predictor_state_now();
    assert!(!f.is_showing(), "expected to hide when vim is open");

    t.on_title_change.fire("foo - git.exe".to_owned());
    f.addon.reevaluate_predictor_state_now();
    assert!(f.is_showing(), "expected to show again after vim closed");
}

#[test]
fn timeline_adds_line_wrap_prediction_even_if_behind_a_boundary() {
    let f = Fixture::new();
    let t = f.mock(&["hello|"], None);
    f.lock_making_predictions();
    f.activate(&t);

    t.on_data(&"hi".repeat(50));
    t.expect_written("");
    f.expect_processed(
        "hi",
        &[
            format!("{CSI}?25l"), // hide cursor
            "hi".to_owned(),      // this greeting characters
            format!("{CSI}3mh{CSI}23m{CSI}3mi{CSI}23m").repeat(36), // rest of the greetings that fit on this line
            format!("{CSI}2;81H"), // move to end of line
            format!("{CSI}?25h"),
        ]
        .join(""),
    );
}
