//! Porte de `src/vs/workbench/contrib/terminal/test/browser/xterm/xtermTerminal.test.ts` (suíte `XtermTerminal`), caso a caso e na mesma ordem.
//!
//! Casos de fora (12 de 28), que dependem de DOM, de GPU, do tema e dos addons do xterm.js, ainda sem equivalente no porte:
//! - `clearBuffer should ...` (2, o laço `normal`/`alternate`): precisam do `DecorationAddon` (`decorationAddon._decorations`),
//!   do `registerDecoration` do `Terminal` e do `CommandDetectionCapability` ligado ao emulador;
//! - `fontRendering` (5 de 6): `inherits the workbench font policy by default`, `applies the configured policy when the
//!   terminal is opened`, `restores the inherited policy when the optional setting is unset`, `updates the CSS class and clears
//!   glyph bitmaps without changing font options` e `applies configuration changes when a detached terminal is updated` leem a
//!   classe CSS `terminal-font-rendering-grayscale` do elemento do xterm (`attachToElement`, `raw.element.classList`);
//! - `detached terminals do not register decoration shutdown listeners`: conta os ouvintes do `onWillShutdown` do
//!   `ILifecycleService`, que o `XtermTerminal` do porte não recebe (o `DecorationAddon` que os registra está fora);
//! - `keeps custom glyphs enabled when moved out of an auxiliary window` e `... moved during addon import`: janela auxiliar
//!   (`iframe`), `attachToElement` e o `TestWebglAddon`;
//! - `theme` (2): `should apply correct background color based on getBackgroundColor` e `should react to and apply theme
//!   changes` dependem do `IThemeService`, do `TestColorTheme` e do `terminalColorRegistry`, que ainda não existem no porte.
//!
//! Portados (16): `should use fallback dimensions of 80x30`, `fontRendering/keeps transparency controlled by images rather
//! than font rendering`, `keeps a scrollback override when the configuration changes`, `getContentsAsText` (11) e
//! `getBufferReverseIterator` (2).
//!
//! Divergências:
//! - **`workbenchInstantiationService`.** O `XtermTerminal` do porte só lê o `IConfigurationService` e o
//!   `ITerminalConfigurationService`; o `InstantiationService` do caso tem os dois, com o `TestConfigurationService` e o
//!   `TerminalConfigurationService` sobre ele.
//! - **`XTermBaseCtor` e `TestXtermAddonImporter`.** O construtor do xterm é a função que cria o [`FakeXterm`] (em
//!   `tests/support/fake_xterm.rs`) com as opções recebidas; os addons e o `xtermColorProvider` não existem no porte.
//! - **`write(data): Promise<void>`.** O dublê interpreta o texto de forma síncrona e chama o `callback` na hora; o ajudante
//!   [`write`] confere que ele foi chamado.
//! - **`configurationService.onDidChangeConfigurationEmitter.fire({ affectsConfiguration: s => s.startsWith('terminal.integrated') })`.**
//!   É o evento real da chave `terminal.integrated` (`fire_configuration_change`).
//! - **`await configurationService.setUserConfiguration(...)`.** A promessa já nasce resolvida, e os casos são síncronos.
//! - **`ensureNoDisposablesAreLeakedInTestSuite()`.** É o guarda criado no começo de cada caso.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
#[path = "configuration_event_support/mod.rs"]
mod configuration_event_support;
mod support;

use std::cell::Cell;
use std::rc::Rc;

use common::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite};
use configuration_event_support::fire_configuration_change;
use je_base::dynamic::Dynamic;
use je_base::lifecycle::IDisposable;
use je_instantiation::{InstantiationService, ServiceCollection};
use je_platform::configuration::service::{I_CONFIGURATION_SERVICE, IConfigurationService};
use je_platform::configuration::test_configuration_service::TestConfigurationService;
use je_platform::terminal::xterm_headless::IHeadlessTerminal;
use je_workbench::contrib::terminal::browser::terminal::{I_TERMINAL_CONFIGURATION_SERVICE, ITerminalConfigurationService};
use je_workbench::contrib::terminal::browser::terminal_configuration_service::TerminalConfigurationService;
use je_workbench::contrib::terminal::browser::xterm::xterm_terminal::{IXtermRaw, IXtermTerminalOptions, XtermOptions, XtermTerminal, XtermTerminalArgs};
use support::fake_xterm::FakeXterm;

/// O `defaultTerminalConfig` do original, com as `overrides` por cima (`{ ...defaultTerminalConfig, ...configuration }`).
fn default_terminal_config(overrides: Vec<(&'static str, Dynamic)>) -> Dynamic {
    let mut entries: Vec<(&'static str, Dynamic)> = vec![
        ("enableImages", Dynamic::Bool(false)),
        ("fontFamily", "monospace".into()),
        ("fontRendering", "inherit".into()),
        ("fontWeight", "normal".into()),
        ("fontWeightBold", "normal".into()),
        ("gpuAcceleration", "off".into()),
        ("scrollback", Dynamic::Number(10.0)),
        ("fastScrollSensitivity", Dynamic::Number(2.0)),
        ("mouseWheelScrollSensitivity", Dynamic::Number(1.0)),
        ("unicodeVersion", "6".into()),
        ("shellIntegration", Dynamic::object([("enabled", Dynamic::Bool(true)), ("decorationsEnabled", "both".into())])),
    ];
    for (key, value) in overrides {
        match entries.iter_mut().find(|(entry_key, _)| *entry_key == key) {
            Some(entry) => entry.1 = value,
            None => entries.push((key, value)),
        }
    }
    Dynamic::object(entries)
}

struct Fixture {
    configuration_service: Rc<TestConfigurationService>,
    instantiation_service: Rc<InstantiationService>,
    xterm: Rc<XtermTerminal>,
}

/// `new XtermTerminal(undefined, XTermBaseCtor, { cols: 80, rows: 30, ... }, undefined)`.
fn create_xterm(store: &LeakGuard, instantiation_service: &InstantiationService, scrollback: Option<i32>, detached: bool) -> Rc<XtermTerminal> {
    store.add_rc(instantiation_service.create_instance::<XtermTerminal>(XtermTerminalArgs {
        xterm_ctor: Box::new(|options: &XtermOptions| -> Rc<dyn IXtermRaw> { FakeXterm::from_options(options) }),
        options: IXtermTerminalOptions { cols: 80, rows: 30, scrollback, detached },
    }))
}

/// O `setup` da suíte.
fn setup(store: &LeakGuard) -> Fixture {
    let configuration_service = TestConfigurationService::new(Some(Dynamic::object([
        ("editor", Dynamic::object([("fastScrollSensitivity", Dynamic::Number(2.0)), ("mouseWheelScrollSensitivity", Dynamic::Number(1.0))])),
        ("files", Dynamic::object(Vec::<(&str, Dynamic)>::new())),
        ("terminal", Dynamic::object([("integrated", default_terminal_config(Vec::new()))])),
    ])));
    // O emissor do `TestConfigurationService` é descartado pelo store do caso (o original não o descarta).
    store.add_rc(configuration_service.on_did_change_configuration_emitter.clone());
    let configuration: Rc<dyn IConfigurationService> = configuration_service.clone();
    let configuration_instantiation = store.add_rc(InstantiationService::new(ServiceCollection::new().with(I_CONFIGURATION_SERVICE, configuration.clone())));
    let terminal_configuration_concrete = store.add_rc(configuration_instantiation.create_instance::<TerminalConfigurationService>(()));
    let terminal_configuration_service: Rc<dyn ITerminalConfigurationService> = terminal_configuration_concrete;
    let instantiation_service = store.add_rc(InstantiationService::new(
        ServiceCollection::new().with(I_CONFIGURATION_SERVICE, configuration).with(I_TERMINAL_CONFIGURATION_SERVICE, terminal_configuration_service),
    ));
    let xterm = create_xterm(store, &instantiation_service, None, false);
    Fixture { configuration_service, instantiation_service, xterm }
}

/// `write(data)`.
fn write(xterm: &XtermTerminal, data: &str) {
    let written = Rc::new(Cell::new(false));
    let flag = written.clone();
    xterm.write(data, Some(Box::new(move || flag.set(true))));
    assert!(written.get(), "o callback de write não foi chamado");
}

/// `configurationService.setUserConfiguration('terminal.integrated', { ...defaultTerminalConfig, ...configuration })` e o
/// disparo de `onDidChangeConfigurationEmitter`.
fn set_terminal_configuration(f: &Fixture, configuration: Vec<(&'static str, Dynamic)>) {
    let _ = f.configuration_service.set_user_configuration("terminal.integrated", default_terminal_config(configuration), None);
    fire_configuration_change(&f.configuration_service, &["terminal.integrated"]);
}

#[test]
fn should_use_fallback_dimensions_of_80x30() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let f = setup(&store);
    assert_eq!(f.xterm.raw().cols(), 80);
    assert_eq!(f.xterm.raw().rows(), 30);
}

// ---------------------------------------------------------------------------------------------------------------------
// suite('fontRendering')
// ---------------------------------------------------------------------------------------------------------------------

#[test]
fn font_rendering_keeps_transparency_controlled_by_images_rather_than_font_rendering() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let f = setup(&store);
    let mut transparency_states: Vec<bool> = Vec::new();
    for enable_images in [false, true] {
        for font_rendering in ["grayscale", "inherit"] {
            set_terminal_configuration(&f, vec![("enableImages", Dynamic::Bool(enable_images)), ("fontRendering", font_rendering.into())]);
            transparency_states.push(f.xterm.raw().options().borrow().allow_transparency);
        }
    }
    assert_eq!(transparency_states, vec![false, false, true, true]);
}

#[test]
fn keeps_a_scrollback_override_when_the_configuration_changes() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let f = setup(&store);
    let detached = create_xterm(&store, &f.instantiation_service, Some(5000), true);
    let initial = (f.xterm.raw().options().borrow().scrollback, detached.raw().options().borrow().scrollback);
    let _ = f.configuration_service.set_user_configuration("terminal.integrated", default_terminal_config(vec![("scrollback", Dynamic::Number(20.0))]), None);
    fire_configuration_change(&f.configuration_service, &["terminal.integrated"]);
    f.xterm.update_config();
    detached.update_config();

    let updated = (f.xterm.raw().options().borrow().scrollback, detached.raw().options().borrow().scrollback);
    assert_eq!((initial, updated), ((10.0, 5000.0), (20.0, 5000.0)));
}

// ---------------------------------------------------------------------------------------------------------------------
// suite('getContentsAsText')
// ---------------------------------------------------------------------------------------------------------------------

#[test]
fn get_contents_as_text_should_return_all_buffer_contents_when_no_markers_provided() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let f = setup(&store);
    write(&f.xterm, "line 1\r\nline 2\r\nline 3\r\nline 4\r\nline 5");

    let result = f.xterm.get_contents_as_text(None, None).unwrap();
    assert!(result.starts_with("line 1\nline 2\nline 3\nline 4\nline 5"), "Should include the content plus empty lines up to buffer length");
    let lines: Vec<&str> = result.split('\n').collect();
    assert_eq!(lines.len() as i32, f.xterm.raw().buffer_length(), "Should end with empty lines (total buffer size is 30 rows)");
}

#[test]
fn get_contents_as_text_should_return_contents_from_start_marker_to_end() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let f = setup(&store);
    write(&f.xterm, "line 1\r\n");
    let start_marker = f.xterm.raw().register_marker(0).unwrap();
    write(&f.xterm, "line 2\r\nline 3\r\nline 4\r\nline 5");

    let result = f.xterm.get_contents_as_text(Some(&*start_marker), None).unwrap();
    assert!(result.starts_with("line 2\nline 3\nline 4\nline 5"), "Should start with line 2 and include empty lines");
}

#[test]
fn get_contents_as_text_should_return_contents_from_start_to_end_marker() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let f = setup(&store);
    write(&f.xterm, "line 1\r\n");
    let start_marker = f.xterm.raw().register_marker(0).unwrap();
    write(&f.xterm, "line 2\r\nline 3\r\n");
    let end_marker = f.xterm.raw().register_marker(0).unwrap();
    write(&f.xterm, "line 4\r\nline 5");

    let result = f.xterm.get_contents_as_text(Some(&*start_marker), Some(&*end_marker)).unwrap();
    assert_eq!(result, "line 2\nline 3\nline 4");
}

#[test]
fn get_contents_as_text_should_return_single_line_when_start_and_end_markers_are_the_same() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let f = setup(&store);
    write(&f.xterm, "line 1\r\nline 2\r\n");
    let marker = f.xterm.raw().register_marker(0).unwrap();
    write(&f.xterm, "line 3\r\nline 4\r\nline 5");

    let result = f.xterm.get_contents_as_text(Some(&*marker), Some(&*marker)).unwrap();
    assert_eq!(result, "line 3");
}

#[test]
fn get_contents_as_text_should_return_empty_string_when_start_marker_is_beyond_end_marker() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let f = setup(&store);
    write(&f.xterm, "line 1\r\n");
    let end_marker = f.xterm.raw().register_marker(0).unwrap();
    write(&f.xterm, "line 2\r\nline 3\r\n");
    let start_marker = f.xterm.raw().register_marker(0).unwrap();
    write(&f.xterm, "line 4\r\nline 5");

    let result = f.xterm.get_contents_as_text(Some(&*start_marker), Some(&*end_marker)).unwrap();
    assert_eq!(result, "");
}

#[test]
fn get_contents_as_text_should_handle_empty_buffer() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let f = setup(&store);
    let result = f.xterm.get_contents_as_text(None, None).unwrap();
    let lines: Vec<&str> = result.split('\n').collect();
    assert_eq!(lines.len() as i32, f.xterm.raw().buffer_length(), "Empty terminal should have empty lines equal to buffer length");
    assert!(lines.iter().all(|line| line.is_empty()), "All lines should be empty");
}

#[test]
fn get_contents_as_text_should_handle_mixed_content_with_spaces_and_special_characters() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let f = setup(&store);
    write(&f.xterm, "hello world\r\n  indented line\r\nline with $pecial chars!@#\r\n\r\nempty line above");

    let result = f.xterm.get_contents_as_text(None, None).unwrap();
    assert!(
        result.starts_with("hello world\n  indented line\nline with $pecial chars!@#\n\nempty line above"),
        "Should handle spaces and special characters correctly"
    );
}

#[test]
fn get_contents_as_text_should_fall_back_to_line_0_when_start_marker_is_disposed_line_minus_1() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let f = setup(&store);
    write(&f.xterm, "line 1\r\n");
    let disposed_marker = f.xterm.raw().register_marker(0).unwrap();
    write(&f.xterm, "line 2\r\nline 3\r\nline 4\r\nline 5");

    disposed_marker.dispose();

    let result = f.xterm.get_contents_as_text(Some(&*disposed_marker), None).unwrap();
    // Should return content from line 0 (including line 1) instead of throwing
    assert!(result.starts_with("line 1\nline 2\nline 3\nline 4\nline 5"), "Unexpected result: {result}");
}

#[test]
fn get_contents_as_text_should_throw_error_when_end_marker_is_disposed_line_minus_1() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let f = setup(&store);
    write(&f.xterm, "line 1\r\n");
    let start_marker = f.xterm.raw().register_marker(0).unwrap();
    write(&f.xterm, "line 2\r\n");
    let disposed_end_marker = f.xterm.raw().register_marker(0).unwrap();
    write(&f.xterm, "line 3\r\nline 4\r\nline 5");

    disposed_end_marker.dispose();

    match f.xterm.get_contents_as_text(Some(&*start_marker), Some(&*disposed_end_marker)) {
        Ok(_) => panic!("Expected error was not thrown"),
        Err(error) => assert_eq!(error.message, "Cannot get contents of a disposed endMarker"),
    }
}

#[test]
fn get_contents_as_text_should_handle_markers_at_buffer_boundaries() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let f = setup(&store);
    let start_marker = f.xterm.raw().register_marker(0).unwrap();
    write(&f.xterm, "line 1\r\nline 2\r\nline 3\r\nline 4\r\n");
    let end_marker = f.xterm.raw().register_marker(0).unwrap();
    write(&f.xterm, "line 5");

    let result = f.xterm.get_contents_as_text(Some(&*start_marker), Some(&*end_marker)).unwrap();
    assert_eq!(result, "line 1\nline 2\nline 3\nline 4\nline 5", "Should handle markers at buffer boundaries correctly");
}

#[test]
fn get_contents_as_text_should_handle_terminal_escape_sequences_properly() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let f = setup(&store);
    write(&f.xterm, "\x1b[31mred text\x1b[0m\r\n\x1b[32mgreen text\x1b[0m");

    let result = f.xterm.get_contents_as_text(None, None).unwrap();
    assert!(result.starts_with("red text\ngreen text"), "ANSI escape sequences should be filtered out, but there will be trailing empty lines");
}

// ---------------------------------------------------------------------------------------------------------------------
// suite('getBufferReverseIterator')
// ---------------------------------------------------------------------------------------------------------------------

/// `[...xterm.getBufferReverseIterator()].reverse().join('\r\n')`.
fn reversed_buffer_text(xterm: &XtermTerminal) -> String {
    let mut lines: Vec<String> = xterm.get_buffer_reverse_iterator().collect();
    lines.reverse();
    lines.join("\r\n")
}

#[test]
fn get_buffer_reverse_iterator_should_get_text_properly_within_scrollback_limit() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let f = setup(&store);
    let text = "line 1\r\nline 2\r\nline 3\r\nline 4\r\nline 5";
    write(&f.xterm, text);

    assert_eq!(text, reversed_buffer_text(&f.xterm), "Should equal original text");
}

#[test]
fn get_buffer_reverse_iterator_should_get_text_properly_when_exceed_scrollback_limit() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let f = setup(&store);
    // max buffer lines(40) = rows(30) + scrollback(10)
    let text = "line 1\r\nline 2\r\nline 3\r\nline 4\r\nline 5\r\n".repeat(8).trim().to_owned();
    write(&f.xterm, &text);
    write(&f.xterm, "\r\nline more");

    let expect = format!("{}\r\nline more", &text[8..]);
    assert_eq!(expect, reversed_buffer_text(&f.xterm), "Should equal original text without line 1");
}
