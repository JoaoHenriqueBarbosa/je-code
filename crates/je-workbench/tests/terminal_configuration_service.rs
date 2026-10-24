//! Porte de `src/vs/workbench/contrib/terminal/test/browser/terminalConfigurationService.test.ts` (suíte `Workbench - TerminalConfigurationService`), caso a caso e na mesma ordem.
//!
//! Divergências:
//! - **`workbenchInstantiationService(undefined, store)`.** Os dois primeiros blocos (`config` e `onConfigChanged`) usam o
//!   serviço criado pelo `InstantiationService` sobre o `TestConfigurationService` (o único serviço de que o
//!   `TerminalConfigurationService` depende), no lugar do `workbenchInstantiationService` inteiro.
//! - **`TestTerminalConfigurationService`.** É o próprio [`TerminalConfigurationService`], que expõe `font_metrics()`.
//! - **`onDidChangeConfigurationEmitter.fire({ affectsConfiguration: c => c.startsWith('terminal.integrated'), ... })`.** É
//!   o evento real da chave `terminal.integrated.fontWeight` (`fire_configuration_change`), cuja `affectsConfiguration`
//!   para `terminal.integrated` é verdadeira.
//! - **`mainWindow.document.body` e `getActiveWindow()`.** Não há DOM: o contêiner do painel é um
//!   [`ITerminalPanelContainer`] que mede o texto com o sistema de texto nativo (`je-text`) e o `devicePixelRatio` é 1.
//! - **`ensureNoDisposablesAreLeakedInTestSuite()`.** É o guarda criado no começo de cada caso.
//! - **`new Promise<void>(r => ...)`** do caso `onConfigChanged` vira um sinalizador que o ouvinte liga (o evento dispara
//!   de forma síncrona).
//! - **`createTerminalConfigationService`.** Cria o `InstantiationService` com o `TestConfigurationService(config)` e o
//!   serviço, como o original.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
#[path = "configuration_event_support/mod.rs"]
mod configuration_event_support;

use std::cell::Cell;
use std::rc::Rc;

use common::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite};
use configuration_event_support::fire_configuration_change;
use je_base::dynamic::Dynamic;
use je_base::platform::Platform;
use je_editor::config::font_info::editor_font_defaults;
use je_instantiation::{InstantiationService, ServiceCollection};
use je_platform::configuration::service::{I_CONFIGURATION_SERVICE, IConfigurationService};
use je_platform::configuration::test_configuration_service::TestConfigurationService;
use je_text::{TextStyle, with_text_system};
use je_workbench::contrib::terminal::browser::terminal::{ClientRect, ITerminalConfigurationService, ITerminalPanelContainer, LinuxDistro};
use je_workbench::contrib::terminal::browser::terminal_configuration_service::TerminalConfigurationService;
use je_workbench::contrib::terminal::common::terminal::default_commands_to_skip_shell;

/// O `devicePixelRatio` da janela ativa (`getActiveWindow()`).
const DEVICE_PIXEL_RATIO: f64 = 1.0;

/// `mainWindow.document.body`: o contêiner que mede a caixa de um texto, como o `div` de medição do original.
struct PanelContainer;

impl ITerminalPanelContainer for PanelContainer {
    fn measure_text(&self, text: &str, font_family: &str, font_size: f64) -> ClientRect {
        let style = TextStyle::new(font_family, font_size as f32, 400);
        let line = with_text_system(|system| system.line(text, &style));
        ClientRect { width: f64::from(line.width), height: f64::from(line.line_height) }
    }

    fn remove_char_measure_element(&self) {}
}

/// Um objeto literal `{ key: value, ... }`.
fn obj<const N: usize>(entries: [(&str, Dynamic); N]) -> Dynamic {
    Dynamic::object(entries)
}

/// `{ editor, terminal: { integrated } }` (sem `editor` quando é `None`).
fn config_of(editor: Option<Dynamic>, integrated: Dynamic) -> Dynamic {
    let terminal = obj([("integrated", integrated)]);
    match editor {
        Some(editor) => obj([("editor", editor), ("terminal", terminal)]),
        None => obj([("terminal", terminal)]),
    }
}

struct Fixture {
    configuration_service: Rc<TestConfigurationService>,
    service: Rc<TerminalConfigurationService>,
}

/// `createTerminalConfigationService(config, linuxDistro?)`.
fn create_terminal_configuration_service(store: &LeakGuard, config: Dynamic, linux_distro: Option<LinuxDistro>) -> Fixture {
    let configuration_service = TestConfigurationService::new(Some(config));
    // O emissor do `TestConfigurationService` é descartado pelo store do caso (o original não o descarta).
    store.add_rc(configuration_service.on_did_change_configuration_emitter.clone());
    let configuration: Rc<dyn IConfigurationService> = configuration_service.clone();
    let instantiation_service = store.add_rc(InstantiationService::new(ServiceCollection::new().with(I_CONFIGURATION_SERVICE, configuration)));
    let service = store.add_rc(instantiation_service.create_instance::<TerminalConfigurationService>(()));
    service.set_panel_container(Rc::new(PanelContainer));
    if let Some(linux_distro) = linux_distro {
        service.font_metrics().set_linux_distro(linux_distro);
    }
    Fixture { configuration_service, service }
}

/// O `setup` da suíte: o serviço sobre um `TestConfigurationService` vazio.
fn setup(store: &LeakGuard) -> Fixture {
    create_terminal_configuration_service(store, obj([]), None)
}

// ---------------------------------------------------------------------------------------------------------------------
// suite('config')
// ---------------------------------------------------------------------------------------------------------------------

#[test]
fn config_should_update_on_any_change_to_terminal_integrated() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let f = setup(&store);
    let original_config = f.service.config();
    fire_configuration_change(&f.configuration_service, &["terminal.integrated.fontWeight"]);
    assert!(!f.service.config().strict_equals(&original_config), "Object reference must change");
}

#[test]
fn config_on_config_changed_should_fire_on_any_change_to_terminal_integrated() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let f = setup(&store);
    let fired = Rc::new(Cell::new(false));
    let flag = fired.clone();
    store.add(f.service.on_config_changed().listen(move |_| flag.set(true)));
    fire_configuration_change(&f.configuration_service, &["terminal.integrated.fontWeight"]);
    assert!(fired.get());
}

// ---------------------------------------------------------------------------------------------------------------------
// suite('shouldCommandSkipShell')
// ---------------------------------------------------------------------------------------------------------------------

#[test]
fn should_command_skip_shell_should_include_defaults_and_added_commands() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let command = "test.command";
    let f = create_terminal_configuration_service(
        &store,
        config_of(None, obj([("commandsToSkipShell", Dynamic::array([Dynamic::string(command)]))])),
        None,
    );
    assert!(f.service.should_command_skip_shell(command));
    assert!(f.service.should_command_skip_shell(default_commands_to_skip_shell()[0]));
}

#[test]
fn should_command_skip_shell_should_remove_excluded_defaults() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let default_command = default_commands_to_skip_shell()[0];
    let f = create_terminal_configuration_service(
        &store,
        config_of(None, obj([("commandsToSkipShell", Dynamic::array([Dynamic::string(&format!("-{default_command}"))]))])),
        None,
    );
    assert!(!f.service.should_command_skip_shell(default_command));
}

// ---------------------------------------------------------------------------------------------------------------------
// suite('getFont')
// ---------------------------------------------------------------------------------------------------------------------

#[test]
fn get_font_font_family() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let f = create_terminal_configuration_service(
        &store,
        config_of(Some(obj([("fontFamily", "foo".into())])), obj([("fontFamily", "bar".into())])),
        None,
    );
    assert!(
        f.service.get_font(DEVICE_PIXEL_RATIO, None, false).font_family.starts_with("bar"),
        "terminal.integrated.fontFamily should be selected over editor.fontFamily"
    );
}

#[test]
fn get_font_font_family_linux_fedora() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let f = create_terminal_configuration_service(
        &store,
        config_of(Some(obj([("fontFamily", "foo".into())])), obj([("fontFamily", Dynamic::Null)])),
        Some(LinuxDistro::Fedora),
    );
    assert!(
        f.service.get_font(DEVICE_PIXEL_RATIO, None, false).font_family.starts_with("'DejaVu Sans Mono'"),
        "Fedora should have its font overridden when terminal.integrated.fontFamily not set"
    );
}

#[test]
fn get_font_font_family_linux_ubuntu() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let f = create_terminal_configuration_service(
        &store,
        config_of(Some(obj([("fontFamily", "foo".into())])), obj([("fontFamily", Dynamic::Null)])),
        Some(LinuxDistro::Ubuntu),
    );
    assert!(
        f.service.get_font(DEVICE_PIXEL_RATIO, None, false).font_family.starts_with("'Ubuntu Mono'"),
        "Ubuntu should have its font overridden when terminal.integrated.fontFamily not set"
    );
}

#[test]
fn get_font_font_family_linux_unknown() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let f = create_terminal_configuration_service(
        &store,
        config_of(Some(obj([("fontFamily", "foo".into())])), obj([("fontFamily", Dynamic::Null)])),
        None,
    );
    assert!(
        f.service.get_font(DEVICE_PIXEL_RATIO, None, false).font_family.starts_with("foo"),
        "editor.fontFamily should be the fallback when terminal.integrated.fontFamily not set"
    );
}

#[test]
fn get_font_font_size_10() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let f = create_terminal_configuration_service(
        &store,
        config_of(
            Some(obj([("fontFamily", "foo".into()), ("fontSize", 9.0.into())])),
            obj([("fontFamily", "bar".into()), ("fontSize", 10.0.into())]),
        ),
        None,
    );
    assert_eq!(f.service.get_font(DEVICE_PIXEL_RATIO, None, false).font_size, 10.0, "terminal.integrated.fontSize should be selected over editor.fontSize");
}

#[test]
fn get_font_font_size_0() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let editor = || Some(obj([("fontFamily", "foo".into())]));
    let integrated = || obj([("fontFamily", Dynamic::Null), ("fontSize", 0.0.into())]);

    let f = create_terminal_configuration_service(&store, config_of(editor(), integrated()), Some(LinuxDistro::Ubuntu));
    assert_eq!(
        f.service.get_font(DEVICE_PIXEL_RATIO, None, false).font_size,
        8.0,
        "The minimum terminal font size (with adjustment) should be used when terminal.integrated.fontSize less than it"
    );

    let f = create_terminal_configuration_service(&store, config_of(editor(), integrated()), None);
    assert_eq!(
        f.service.get_font(DEVICE_PIXEL_RATIO, None, false).font_size,
        6.0,
        "The minimum terminal font size should be used when terminal.integrated.fontSize less than it"
    );
}

#[test]
fn get_font_font_size_1500() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let f = create_terminal_configuration_service(
        &store,
        config_of(Some(obj([("fontFamily", "foo".into())])), obj([("fontFamily", 0.0.into()), ("fontSize", 1500.0.into())])),
        None,
    );
    assert_eq!(
        f.service.get_font(DEVICE_PIXEL_RATIO, None, false).font_size,
        100.0,
        "The maximum terminal font size should be used when terminal.integrated.fontSize more than it"
    );
}

#[test]
fn get_font_font_size_null() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let editor = || Some(obj([("fontFamily", "foo".into())]));
    let integrated = || obj([("fontFamily", 0.0.into()), ("fontSize", Dynamic::Null)]);
    let default_font_size = editor_font_defaults().font_size;

    let f = create_terminal_configuration_service(&store, config_of(editor(), integrated()), Some(LinuxDistro::Ubuntu));
    assert_eq!(
        f.service.get_font(DEVICE_PIXEL_RATIO, None, false).font_size,
        default_font_size + 2.0,
        "The default editor font size (with adjustment) should be used when terminal.integrated.fontSize is not set"
    );

    let f = create_terminal_configuration_service(&store, config_of(editor(), integrated()), None);
    assert_eq!(
        f.service.get_font(DEVICE_PIXEL_RATIO, None, false).font_size,
        default_font_size,
        "The default editor font size should be used when terminal.integrated.fontSize is not set"
    );
}

#[test]
fn get_font_line_height_2() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let f = create_terminal_configuration_service(
        &store,
        config_of(
            Some(obj([("fontFamily", "foo".into()), ("lineHeight", 1.0.into())])),
            obj([("fontFamily", 0.0.into()), ("lineHeight", 2.0.into())]),
        ),
        None,
    );
    assert_eq!(f.service.get_font(DEVICE_PIXEL_RATIO, None, false).line_height, 2.0, "terminal.integrated.lineHeight should be selected over editor.lineHeight");
}

#[test]
fn get_font_line_height_0() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let f = create_terminal_configuration_service(
        &store,
        config_of(
            Some(obj([("fontFamily", "foo".into()), ("lineHeight", 1.0.into())])),
            obj([("fontFamily", 0.0.into()), ("lineHeight", 0.0.into())]),
        ),
        None,
    );
    assert_eq!(
        f.service.get_font(DEVICE_PIXEL_RATIO, None, false).line_height,
        if Platform::detect().is_linux { 1.1 } else { 1.0 },
        "editor.lineHeight should be the default when terminal.integrated.lineHeight not set"
    );
}

// ---------------------------------------------------------------------------------------------------------------------
// suite('configFontIsMonospace')
// ---------------------------------------------------------------------------------------------------------------------

#[test]
fn config_font_is_monospace_is_monospace_monospace() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let f = create_terminal_configuration_service(&store, config_of(None, obj([("fontFamily", "monospace".into())])), None);
    assert!(f.service.config_font_is_monospace(), "monospace is monospaced");
}

#[test]
fn config_font_is_monospace_is_monospace_sans_serif() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let f = create_terminal_configuration_service(&store, config_of(None, obj([("fontFamily", "sans-serif".into())])), None);
    assert!(!f.service.config_font_is_monospace(), "sans-serif is not monospaced");
}

#[test]
fn config_font_is_monospace_is_monospace_serif() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let f = create_terminal_configuration_service(&store, config_of(None, obj([("fontFamily", "serif".into())])), None);
    assert!(!f.service.config_font_is_monospace(), "serif is not monospaced");
}

#[test]
fn config_font_is_monospace_is_monospace_monospace_falls_back_to_editor_font_family() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let f = create_terminal_configuration_service(
        &store,
        config_of(Some(obj([("fontFamily", "monospace".into())])), obj([("fontFamily", Dynamic::Null)])),
        None,
    );
    assert!(f.service.config_font_is_monospace(), "monospace is monospaced");
}

#[test]
fn config_font_is_monospace_is_monospace_sans_serif_falls_back_to_editor_font_family() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let f = create_terminal_configuration_service(
        &store,
        config_of(Some(obj([("fontFamily", "sans-serif".into())])), obj([("fontFamily", Dynamic::Null)])),
        None,
    );
    assert!(!f.service.config_font_is_monospace(), "sans-serif is not monospaced");
}

#[test]
fn config_font_is_monospace_is_monospace_serif_falls_back_to_editor_font_family() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let f = create_terminal_configuration_service(
        &store,
        config_of(Some(obj([("fontFamily", "serif".into())])), obj([("fontFamily", Dynamic::Null)])),
        None,
    );
    assert!(!f.service.config_font_is_monospace(), "serif is not monospaced");
}
