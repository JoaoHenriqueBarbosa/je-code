//! Porte de `src/vs/workbench/contrib/terminal/test/browser/xterm/xtermTerminal.test.ts` (suíte `XtermTerminal`), caso a caso e na mesma ordem.
//!
//! Portados (28 de 28): `should use fallback dimensions of 80x30`, `clearBuffer` (2, o laço `normal`/`alternate`), `fontRendering`
//! (6), `detached terminals do not register decoration shutdown listeners`, `keeps a scrollback override when the configuration
//! changes`, `keeps custom glyphs enabled ...` (2), `getContentsAsText` (11), `getBufferReverseIterator` (2) e `theme` (2).
//!
//! Divergências:
//! - **`workbenchInstantiationService`.** O `InstantiationService` do caso tem o `TestConfigurationService`, o
//!   `TerminalConfigurationService` sobre ele, o [`TestThemeService`] (`IThemeService`) e o `TestLifecycleService`
//!   (`ILifecycleService`). O `IContextMenuService`, o `IHoverService` e os demais serviços do `DecorationAddon` que não têm
//!   identificador de serviço no porte chegam em `decoration_services` (os dublês de `support/terminal_decoration_support.rs`). O
//!   `IAccessibilitySignalService` do `XtermTerminal` não é registrado: o sinal `clear` não tem leitor no caso.
//! - **`XTermBaseCtor`, `TestTerminal` e `TestXtermAddonImporter`.** O construtor do xterm é a função que cria o [`FakeXterm`] (em
//!   `tests/support/fake_xterm.rs`) com as opções recebidas. O `registerDecoration` do `TestTerminal` do caso `clearBuffer` é o do
//!   próprio `FakeXterm` (ver `fake_xterm_decoration.rs`). O `TestXtermAddonImporter` entrega o `TestWebglAddon` (em
//!   `tests/support/xterm_test_utils.rs`) como o renderizador por GPU do porte.
//! - **`write(data): Promise<void>`.** O dublê interpreta o texto de forma síncrona e chama o `callback` na hora; o ajudante
//!   [`write`] confere que ele foi chamado.
//! - **`configurationService.onDidChangeConfigurationEmitter.fire({ affectsConfiguration: s => s.startsWith('terminal.integrated') })`.**
//!   É o evento real da chave `terminal.integrated` (`fire_configuration_change`).
//! - **`await configurationService.setUserConfiguration(...)`.** A promessa já nasce resolvida, e os casos são síncronos.
//! - **`await timeout(0)`.** É `runtime::run_until_idle()`, que esvazia as promessas e os temporizadores pendentes.
//! - **`attach(terminal)`, `document.createElement('div')` e `terminal.attachToElement(container, { enableGpu: false })`.** Não há
//!   elemento: é [`XtermTerminal::attach_to_element`] com `enable_gpu: false`. `xterm.raw.element!.classList.contains(
//!   'terminal-font-rendering-grayscale')` é [`IFontRenderingTerminal::grayscale_font_rendering`], o estado nativo que o renderizador lê.
//!   `stub(xterm.raw, 'clearTextureAtlas')` é o registro que o `FakeXterm` faz do estado a cada `clearTextureAtlas`.
//! - **Os dois casos de glifos personalizados (janela auxiliar).** O `iframe`, o `contentDocument` e o `createElement` que lança não
//!   têm equivalente (não há DOM nem janelas auxiliares no porte). O que o caso confere é o ciclo do renderizador: carregado ao
//!   anexar com `gpuAcceleration: 'on'`, ele não é descartado nem recriado quando o terminal é aberto de novo
//!   (`raw.open`, a mudança de contêiner) e atualizado (`refresh`), e o `TestWebglAddon` recebe `customGlyphs: true` uma única vez.
//!   O `WebglAddon` do original é o renderizador por GPU (wgpu) do porte: [`IXtermGpuRenderer`], criado pelo importador com as
//!   mesmas opções.
//! - **`(emitter as unknown as { _size: number })._size`.** É `Emitter::size_for_testing`, lido no emissor de `onWillShutdown` do
//!   `TestLifecycleService`.
//! - **`(xterm.decorationAddon as unknown as { _decorations: Map<number, unknown> })._decorations`.** São [`DecorationAddon::has_decoration`]
//!   e [`DecorationAddon::decoration`], as leituras do mapa privado.
//! - **`ensureNoDisposablesAreLeakedInTestSuite()`.** É o guarda criado no começo de cada caso.
//! - **`isMacintosh`.** É [`Platform::detect`].

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
#[path = "configuration_event_support/mod.rs"]
mod configuration_event_support;
mod support;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use common::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite};
use configuration_event_support::fire_configuration_change;
use je_base::dynamic::Dynamic;
use je_base::lifecycle::IDisposable;
use je_base::platform::Platform;
use je_base::runtime;
use je_editor::services::theme_service::{I_THEME_SERVICE, IThemeService};
use je_instantiation::{InstantiationService, ServiceCollection};
use je_platform::configuration::service::{I_CONFIGURATION_SERVICE, IConfigurationService};
use je_platform::configuration::test_configuration_service::TestConfigurationService;
use je_platform::log::NullLogService;
use je_platform::terminal::capabilities::capabilities::{CommandDetection, ICommandDetectionCapability, IPartialCommandDetectionCapability, ITerminalCapabilityStore, PartialCommandDetection};
use je_platform::terminal::capabilities::command_detection_capability::CommandDetectionCapability;
use je_platform::terminal::capabilities::partial_command_detection_capability::PartialCommandDetectionCapability;
use je_platform::terminal::capabilities::terminal_capability_store::TerminalCapabilityStore;
use je_platform::terminal::xterm_headless::IHeadlessTerminal;
use je_theme::color::{Color, Srgba};
use je_workbench::contrib::terminal::browser::terminal::{I_TERMINAL_CONFIGURATION_SERVICE, ITerminalConfigurationService};
use je_workbench::contrib::terminal::browser::terminal_configuration_service::TerminalConfigurationService;
use je_workbench::contrib::terminal::browser::xterm::decoration_addon::DecorationAddonServices;
use je_workbench::contrib::terminal::browser::xterm::terminal_font_rendering::IFontRenderingTerminal;
use je_workbench::contrib::terminal::browser::xterm::xterm_terminal::{
    IXtermAttachToElementOptions, IXtermColorProvider, IXtermRaw, IXtermTerminalOptions, XtermOptions, XtermTerminal, XtermTerminalArgs, XtermTheme,
};
use je_workbench::contrib::terminal::common::terminal_color_registry::{
    ANSI_COLOR_IDENTIFIERS, TERMINAL_BACKGROUND_COLOR, TERMINAL_CURSOR_BACKGROUND_COLOR, TERMINAL_CURSOR_FOREGROUND_COLOR, TERMINAL_FOREGROUND_COLOR,
    TERMINAL_INACTIVE_SELECTION_BACKGROUND_COLOR, TERMINAL_SELECTION_BACKGROUND_COLOR, TERMINAL_SELECTION_FOREGROUND_COLOR,
};
use je_workbench::services::lifecycle::lifecycle::{I_LIFECYCLE_SERVICE, ILifecycleService};
use je_workbench::test::common::workbench_test_services::TestLifecycleService;
use support::fake_xterm::FakeXterm;
use support::mock_command_service::MockCommandService;
use support::terminal_contrib_support::PanickingNotificationService;
use support::terminal_decoration_support::{
    SilentAccessibilitySignalService, StubHoverService, UnusedClipboardService, UnusedContextMenuService, UnusedOpenerService, UnusedQuickInputService,
};
use support::xterm_test_utils::{TestColorTheme, TestThemeService, TestWebglAddon, TestXtermAddonImporter};

/// `PANEL_BACKGROUND` e `SIDE_BAR_BACKGROUND` (`workbench/common/theme.ts`).
const PANEL_BACKGROUND: &str = "panel.background";
const SIDE_BAR_BACKGROUND: &str = "sideBar.background";

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

/// O `xtermColorProvider` do original: `{ getBackgroundColor: () => color }`.
struct FixedBackgroundProvider(Option<Color>);

impl IXtermColorProvider for FixedBackgroundProvider {
    fn get_background_color(&self, _theme: &dyn je_editor::services::theme_service::IColorTheme) -> Option<Color> {
        self.0.clone()
    }
}

/// O que o `setup` entrega aos casos, menos o `XtermTerminal`.
struct Env {
    configuration_service: Rc<TestConfigurationService>,
    instantiation_service: Rc<InstantiationService>,
    theme_service: Rc<TestThemeService>,
    lifecycle_service: Rc<TestLifecycleService>,
}

struct Fixture {
    env: Env,
    xterm: Rc<XtermTerminal>,
    /// `lifecycleListenerCountBeforeXterm`.
    lifecycle_listener_count_before_xterm: usize,
}

/// Os argumentos que variam entre as chamadas de `createInstance(XtermTerminal, ...)` do original.
struct XtermSpec {
    capabilities: Rc<TerminalCapabilityStore>,
    color_provider: Option<Color>,
    scrollback: Option<i32>,
    detached: bool,
}

impl XtermSpec {
    /// `capabilities: store.add(new TerminalCapabilityStore())` e `xtermColorProvider: { getBackgroundColor: () => undefined }`.
    fn new(store: &LeakGuard) -> Self {
        Self { capabilities: store.add_rc(TerminalCapabilityStore::new()), color_provider: None, scrollback: None, detached: false }
    }
}

struct CreatedXterm {
    xterm: Rc<XtermTerminal>,
    fake: Rc<FakeXterm>,
}

/// Os serviços do `DecorationAddon` que o `workbenchInstantiationService` entrega e nenhum caso usa.
fn decoration_services(env: &Env) -> DecorationAddonServices {
    let configuration: Rc<dyn IConfigurationService> = env.configuration_service.clone();
    DecorationAddonServices {
        clipboard_service: Rc::new(UnusedClipboardService),
        context_menu_service: Rc::new(UnusedContextMenuService),
        configuration_service: configuration,
        theme_service: env.theme_service.clone(),
        opener_service: Rc::new(UnusedOpenerService),
        quick_input_service: Rc::new(UnusedQuickInputService),
        command_service: Rc::new(MockCommandService::default()),
        accessibility_signal_service: Rc::new(SilentAccessibilitySignalService),
        notification_service: Rc::new(PanickingNotificationService),
        hover_service: Rc::new(StubHoverService { hover_disposed: Rc::new(Cell::new(false)) }),
    }
}

/// `instantiationService.createInstance(XtermTerminal, undefined, XTermBaseCtor, { cols: 80, rows: 30, ... }, undefined)`.
fn create_xterm(store: &LeakGuard, env: &Env, spec: XtermSpec) -> CreatedXterm {
    let slot: Rc<RefCell<Option<Rc<FakeXterm>>>> = Rc::default();
    let ctor_slot = slot.clone();
    let capabilities: Rc<dyn ITerminalCapabilityStore> = spec.capabilities;
    let xterm = store.add_rc(env.instantiation_service.create_instance::<XtermTerminal>(XtermTerminalArgs {
        xterm_ctor: Box::new(move |options: &XtermOptions| -> Rc<dyn IXtermRaw> {
            let fake = FakeXterm::from_options(options);
            *ctor_slot.borrow_mut() = Some(fake.clone());
            fake
        }),
        options: IXtermTerminalOptions {
            cols: 80,
            rows: 30,
            xterm_color_provider: Some(Rc::new(FixedBackgroundProvider(spec.color_provider))),
            capabilities: Some(capabilities),
            decoration_services: Some(decoration_services(env)),
            gpu_renderer_importer: Some(Rc::new(TestXtermAddonImporter)),
            scrollback: spec.scrollback,
            detached: spec.detached,
        },
    }));
    let fake = slot.borrow().clone().expect("o construtor do xterm não foi chamado");
    CreatedXterm { xterm, fake }
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
    let theme_service = store.add_rc(TestThemeService::new());
    let theme: Rc<dyn IThemeService> = theme_service.clone();
    let lifecycle_service = store.add_rc(TestLifecycleService::new());
    let lifecycle: Rc<dyn ILifecycleService> = lifecycle_service.clone();
    let instantiation_service = store.add_rc(InstantiationService::new(
        ServiceCollection::new()
            .with(I_CONFIGURATION_SERVICE, configuration)
            .with(I_TERMINAL_CONFIGURATION_SERVICE, terminal_configuration_service)
            .with(I_THEME_SERVICE, theme)
            .with(I_LIFECYCLE_SERVICE, lifecycle),
    ));
    let env = Env { configuration_service, instantiation_service, theme_service, lifecycle_service };
    let lifecycle_listener_count_before_xterm = env.lifecycle_service.on_will_shutdown_emitter().size_for_testing();

    let xterm = create_xterm(store, &env, XtermSpec::new(store)).xterm;

    TestWebglAddon::set_should_throw(false);
    TestWebglAddon::set_is_enabled(false);
    TestWebglAddon::clear_custom_glyph_options();
    Fixture { env, xterm, lifecycle_listener_count_before_xterm }
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
    let _ = f.env.configuration_service.set_user_configuration("terminal.integrated", default_terminal_config(configuration), None);
    fire_configuration_change(&f.env.configuration_service, &["terminal.integrated"]);
}

/// `Color.toString()` de um tom opaco.
fn rgb(r: u8, g: u8, b: u8) -> Color {
    Color::from(Srgba::opaque(r, g, b))
}

/// `strictEqual(a, b)` entre dois marcadores ou comandos pela identidade do objeto.
fn same<T: ?Sized>(a: &Rc<T>, b: &Rc<T>) -> bool {
    std::ptr::addr_eq(Rc::as_ptr(a), Rc::as_ptr(b))
}

#[test]
fn should_use_fallback_dimensions_of_80x30() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let f = setup(&store);
    assert_eq!(f.xterm.raw().cols(), 80);
    assert_eq!(f.xterm.raw().rows(), 30);
}

// ---------------------------------------------------------------------------------------------------------------------
// clearBuffer (o laço `normal` e `alternate`)
// ---------------------------------------------------------------------------------------------------------------------

/// O corpo do laço `for (const buffer of ['normal', 'alternate'])`.
fn clear_buffer_case(alternate: bool) {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let f = setup(&store);
    let capability_store = store.add_rc(TerminalCapabilityStore::new());
    let xterm = create_xterm(
        &store,
        &f.env,
        XtermSpec { capabilities: capability_store.clone(), color_provider: None, scrollback: None, detached: false },
    )
    .xterm;
    let headless: Rc<dyn IHeadlessTerminal> = xterm.raw().clone();
    let command_detection = store.add_rc(CommandDetectionCapability::new(headless.clone(), NullLogService::shared()));
    let on_did_execute_text = store.add_rc(je_base::event::Emitter::<()>::new());
    let partial_command_detection = store.add_rc(PartialCommandDetectionCapability::new(headless, Some(on_did_execute_text.event())));
    let command_detection_capability: Rc<dyn ICommandDetectionCapability> = command_detection.clone();
    capability_store.add::<CommandDetection>(command_detection_capability);
    let partial_capability: Rc<dyn IPartialCommandDetectionCapability> = partial_command_detection.clone();
    capability_store.add::<PartialCommandDetection>(partial_capability);

    let _ = xterm.raw().register_marker(0);
    command_detection.handle_prompt_start(None);
    write(&xterm, "$ ");
    command_detection.handle_command_start(None);
    write(&xterm, "echo test");
    command_detection.handle_command_executed(None);
    write(&xterm, "\r\noutput\r\n");
    command_detection.handle_command_finished(Some(0.0), None);

    write(&xterm, "partial");
    xterm.raw().input("\r");
    write(&xterm, "\r\n");
    write(&xterm, &"line\r\n".repeat(usize::try_from(xterm.raw().rows()).unwrap()));
    command_detection.handle_prompt_start(None);
    write(&xterm, "$ ");
    command_detection.handle_command_start(None);

    assert!(xterm.raw().buffer_active().base_y() > 0);
    assert_eq!(command_detection.commands().len(), 1);
    assert_eq!(partial_command_detection.commands().len(), 1);
    let decoration_addon = xterm.decoration_addon().expect("o XtermTerminal não criou o DecorationAddon");
    let cleared_command_marker_id = command_detection.commands()[0].marker().expect("o comando não tem marcador").id();
    assert!(decoration_addon.has_decoration(cleared_command_marker_id));
    let invalidated_commands: Rc<RefCell<Vec<String>>> = Rc::default();
    let sink = invalidated_commands.clone();
    store.add(command_detection.on_command_invalidated().listen(move |commands| sink.borrow_mut().extend(commands.iter().map(|command| command.command()))));

    let command = command_detection.commands()[0].clone();
    let partial_marker = partial_command_detection.commands()[0].clone();
    let decoration = decoration_addon.decoration(cleared_command_marker_id);
    let normal_buffer_lines = |xterm: &XtermTerminal| -> Vec<String> {
        let normal = xterm.raw().buffer_normal();
        (0..xterm.raw().normal_buffer_length())
            .map(|index| normal.get_line(index).expect("linha do buffer normal").translate_to_string(None, None, None).to_string_lossy())
            .collect()
    };
    let normal_buffer_lines_before = normal_buffer_lines(&*xterm);
    let current_command_start_marker = command_detection.current_command().borrow().command_start_marker.clone().expect("commandStartMarker");
    if alternate {
        write(&xterm, "\x1b[?1049h");
        write(&xterm, "alternate output\r\n");
        assert!(!xterm.raw().is_active_buffer_normal());
    }

    xterm.clear_buffer();

    if alternate {
        assert_eq!(xterm.raw().buffer_active().cursor_y(), 0);
        write(&xterm, "\x1b[?1049l");
        assert!(xterm.raw().is_active_buffer_normal());
        assert_eq!(normal_buffer_lines(&*xterm), normal_buffer_lines_before);
        assert_eq!(command_detection.commands().len(), 1);
        assert!(same(&command_detection.commands()[0], &command));
        assert_eq!(partial_command_detection.commands().len(), 1);
        assert!(same(&partial_command_detection.commands()[0], &partial_marker));
        assert_eq!(*invalidated_commands.borrow(), Vec::<String>::new());
        assert!(!command.marker().expect("marcador").is_disposed());
        assert!(!partial_marker.is_disposed());
        let decoration_after = decoration_addon.decoration(cleared_command_marker_id);
        assert!(matches!((&decoration_after, &decoration), (Some(after), Some(before)) if same(after, before)));
        let start_marker_after = command_detection.current_command().borrow().command_start_marker.clone().expect("commandStartMarker");
        assert!(same(&start_marker_after, &current_command_start_marker));
        return;
    }

    assert!(command_detection.commands().is_empty());
    assert!(partial_command_detection.commands().is_empty());
    assert_eq!(*invalidated_commands.borrow(), vec!["echo test".to_owned()]);
    assert!(!decoration_addon.has_decoration(cleared_command_marker_id));
}

#[test]
fn clear_buffer_should_clear_rich_and_partial_command_history_including_scrollback() {
    clear_buffer_case(false);
}

#[test]
fn clear_buffer_should_preserve_normal_buffer_command_history_and_decorations_when_clearing_the_alternate_buffer() {
    clear_buffer_case(true);
}

// ---------------------------------------------------------------------------------------------------------------------
// suite('fontRendering')
// ---------------------------------------------------------------------------------------------------------------------

/// `setFontRendering(fontRendering)`: `undefined` é [`Dynamic::Undefined`].
fn set_font_rendering(f: &Fixture, font_rendering: Dynamic) {
    set_terminal_configuration(f, vec![("fontRendering", font_rendering)]);
}

/// `attach(terminal)`.
fn attach(terminal: &XtermTerminal) {
    terminal.attach_to_element(Some(IXtermAttachToElementOptions { enable_gpu: false }));
}

/// `{ grayscaleClass, allowTransparency }` do terminal.
fn font_rendering_state(terminal: &XtermTerminal) -> (bool, bool) {
    (terminal.raw().grayscale_font_rendering(), terminal.raw().options().borrow().allow_transparency)
}

#[test]
fn font_rendering_inherits_the_workbench_font_policy_by_default() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let f = setup(&store);
    attach(&f.xterm);
    assert_eq!(font_rendering_state(&f.xterm), (false, false));
}

#[test]
fn font_rendering_applies_the_configured_policy_when_the_terminal_is_opened() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let f = setup(&store);
    set_font_rendering(&f, "grayscale".into());
    attach(&f.xterm);
    assert_eq!(font_rendering_state(&f.xterm), (Platform::detect().is_macintosh, false));
}

#[test]
fn font_rendering_restores_the_inherited_policy_when_the_optional_setting_is_unset() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let f = setup(&store);
    set_font_rendering(&f, "grayscale".into());
    attach(&f.xterm);
    let initially_grayscale = f.xterm.raw().grayscale_font_rendering();
    set_font_rendering(&f, Dynamic::Undefined);
    assert_eq!((initially_grayscale, font_rendering_state(&f.xterm)), (Platform::detect().is_macintosh, (false, false)));
}

#[test]
fn font_rendering_updates_the_css_class_and_clears_glyph_bitmaps_without_changing_font_options() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let f = setup(&store);
    let created = create_xterm(&store, &f.env, XtermSpec::new(&store));
    attach(&created.xterm);
    let (initial_font_family, initial_font_size, initial_font_weight, initial_theme) = {
        let options = created.xterm.raw().options().borrow();
        (options.font_family.clone(), options.font_size, options.font_weight.clone(), options.theme.clone())
    };
    // O `stub(xterm.raw, 'clearTextureAtlas')` só enxerga as chamadas depois de instalado.
    let _ = created.fake.take_texture_atlas_clears();
    set_font_rendering(&f, "grayscale".into());
    set_font_rendering(&f, "grayscale".into());
    set_font_rendering(&f, "inherit".into());
    let states_at_redraw = created.fake.take_texture_atlas_clears();
    let options = created.xterm.raw().options().borrow();
    assert_eq!(states_at_redraw, if Platform::detect().is_macintosh { vec![true, false] } else { Vec::new() });
    assert!(!options.allow_transparency);
    assert_eq!(options.font_family, initial_font_family);
    assert_eq!(options.font_size, initial_font_size);
    assert!(options.font_weight.strict_equals(&initial_font_weight));
    assert_eq!(options.theme, initial_theme);
}

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
fn font_rendering_applies_configuration_changes_when_a_detached_terminal_is_updated() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let f = setup(&store);
    let terminal = create_xterm(&store, &f.env, XtermSpec { detached: true, ..XtermSpec::new(&store) }).xterm;
    attach(&terminal);
    set_font_rendering(&f, "grayscale".into());
    terminal.update_config();
    assert_eq!(font_rendering_state(&terminal), (Platform::detect().is_macintosh, false));
    set_font_rendering(&f, "inherit".into());
    terminal.update_config();
    assert_eq!(font_rendering_state(&terminal), (false, false));
}

#[test]
fn detached_terminals_do_not_register_decoration_shutdown_listeners() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let f = setup(&store);
    let on_will_shutdown = f.env.lifecycle_service.on_will_shutdown_emitter();
    let listener_count_after_regular_xterm = on_will_shutdown.size_for_testing();
    for _ in 0..50 {
        create_xterm(&store, &f.env, XtermSpec { detached: true, ..XtermSpec::new(&store) });
    }

    assert_eq!(
        (listener_count_after_regular_xterm - f.lifecycle_listener_count_before_xterm, on_will_shutdown.size_for_testing() - listener_count_after_regular_xterm),
        (1, 0)
    );
}

#[test]
fn keeps_a_scrollback_override_when_the_configuration_changes() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let f = setup(&store);
    let detached = create_xterm(&store, &f.env, XtermSpec { scrollback: Some(5000), detached: true, ..XtermSpec::new(&store) }).xterm;
    let initial = (f.xterm.raw().options().borrow().scrollback, detached.raw().options().borrow().scrollback);
    let _ = f.env.configuration_service.set_user_configuration("terminal.integrated", default_terminal_config(vec![("scrollback", Dynamic::Number(20.0))]), None);
    fire_configuration_change(&f.env.configuration_service, &["terminal.integrated"]);
    f.xterm.update_config();
    detached.update_config();

    let updated = (f.xterm.raw().options().borrow().scrollback, detached.raw().options().borrow().scrollback);
    assert_eq!((initial, updated), ((10.0, 5000.0), (20.0, 5000.0)));
}

// ---------------------------------------------------------------------------------------------------------------------
// Os casos de glifos personalizados (ver o cabeçalho sobre a janela auxiliar)
// ---------------------------------------------------------------------------------------------------------------------

/// A configuração `gpuAcceleration: 'on'` e `customGlyphs: true` dos dois casos.
fn enable_custom_glyphs_on_gpu(f: &Fixture) {
    set_terminal_configuration(f, vec![("gpuAcceleration", "on".into()), ("customGlyphs", Dynamic::Bool(true))]);
}

#[test]
fn keeps_custom_glyphs_enabled_when_moved_out_of_an_auxiliary_window() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let f = setup(&store);
    enable_custom_glyphs_on_gpu(&f);

    f.xterm.attach_to_element(None);
    runtime::run_until_idle();

    f.xterm.raw().open();
    f.xterm.refresh();
    runtime::run_until_idle();

    assert_eq!(TestWebglAddon::custom_glyph_options(), vec![Some(true)]);
    assert!(f.xterm.is_gpu_accelerated());
}

#[test]
fn keeps_custom_glyphs_enabled_when_moved_during_addon_import() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let f = setup(&store);
    enable_custom_glyphs_on_gpu(&f);

    f.xterm.attach_to_element(None);

    f.xterm.raw().open();
    f.xterm.refresh();
    runtime::run_until_idle();

    assert_eq!(TestWebglAddon::custom_glyph_options(), vec![Some(true)]);
    assert!(f.xterm.is_gpu_accelerated());
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

// ---------------------------------------------------------------------------------------------------------------------
// suite('theme')
// ---------------------------------------------------------------------------------------------------------------------

/// O `ITheme` esperado: `background` indefinido, `overviewRulerBorder` e os `scrollbarSlider*` indefinidos, e o restante dado.
fn expected_theme(
    foreground: &str,
    cursor: &str,
    cursor_accent: &str,
    selection_background: &str,
    selection_inactive_background: &str,
    selection_foreground: Option<&str>,
    ansi: [&str; 16],
) -> XtermTheme {
    let text = |value: &str| Some(value.to_owned());
    XtermTheme {
        background: None,
        foreground: text(foreground),
        cursor: text(cursor),
        cursor_accent: text(cursor_accent),
        selection_background: text(selection_background),
        selection_inactive_background: text(selection_inactive_background),
        selection_foreground: selection_foreground.and_then(text),
        overview_ruler_border: None,
        scrollbar_slider_active_background: None,
        scrollbar_slider_background: None,
        scrollbar_slider_hover_background: None,
        ansi: ansi.map(|color| Some(color.to_owned())),
    }
}

/// O `TestColorTheme` dos dois temas do caso `should react to and apply theme changes`: as cores do terminal terminam em `low`
/// (`#0001` + `00` ou `0f`) e as ANSI são `#NN00` + `low`.
fn terminal_theme(low: &str, with_selection_foreground: bool) -> Rc<TestColorTheme> {
    let ansi_digits = ["01", "02", "03", "04", "05", "06", "07", "08", "09", "10", "11", "12", "13", "14", "15", "16"];
    let mut entries: Vec<(String, Option<String>)> = vec![
        (TERMINAL_BACKGROUND_COLOR.to_owned(), Some(format!("#0001{low}"))),
        (TERMINAL_FOREGROUND_COLOR.to_owned(), Some(format!("#0002{low}"))),
        (TERMINAL_CURSOR_FOREGROUND_COLOR.to_owned(), Some(format!("#0003{low}"))),
        (TERMINAL_CURSOR_BACKGROUND_COLOR.to_owned(), Some(format!("#0004{low}"))),
        (TERMINAL_SELECTION_BACKGROUND_COLOR.to_owned(), Some(format!("#0005{low}"))),
        (TERMINAL_INACTIVE_SELECTION_BACKGROUND_COLOR.to_owned(), Some(format!("#0006{low}"))),
        (TERMINAL_SELECTION_FOREGROUND_COLOR.to_owned(), with_selection_foreground.then(|| format!("#0007{low}"))),
    ];
    for (id, digits) in ANSI_COLOR_IDENTIFIERS.iter().zip(ansi_digits) {
        entries.push(((*id).to_owned(), Some(format!("#{digits}00{low}"))));
    }
    TestColorTheme::new(entries.iter().map(|(id, value)| (id.as_str(), value.as_deref())).collect())
}

#[test]
fn theme_should_apply_correct_background_color_based_on_get_background_color() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let f = setup(&store);
    f.env.theme_service.set_theme(TestColorTheme::new(vec![(PANEL_BACKGROUND, Some("#ff0000")), (SIDE_BAR_BACKGROUND, Some("#00ff00"))]));
    let xterm = create_xterm(&store, &f.env, XtermSpec { color_provider: Some(rgb(255, 0, 0)), ..XtermSpec::new(&store) }).xterm;
    assert_eq!(xterm.raw().options().borrow().theme.background.as_deref(), Some("#ff0000"));
}

#[test]
fn theme_should_react_to_and_apply_theme_changes() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let f = setup(&store);
    f.env.theme_service.set_theme(terminal_theme("00", false));
    let xterm = create_xterm(&store, &f.env, XtermSpec::new(&store)).xterm;
    assert_eq!(
        xterm.raw().options().borrow().theme,
        expected_theme(
            "#000200",
            "#000300",
            "#000400",
            "#000500",
            "#000600",
            None,
            [
                "#010000", "#020000", "#030000", "#040000", "#050000", "#060000", "#070000", "#080000", "#090000", "#100000", "#110000", "#120000", "#130000", "#140000",
                "#150000", "#160000",
            ],
        )
    );
    f.env.theme_service.set_theme(terminal_theme("0f", true));
    assert_eq!(
        xterm.raw().options().borrow().theme,
        expected_theme(
            "#00020f",
            "#00030f",
            "#00040f",
            "#00050f",
            "#00060f",
            Some("#00070f"),
            [
                "#01000f", "#02000f", "#03000f", "#04000f", "#05000f", "#06000f", "#07000f", "#08000f", "#09000f", "#10000f", "#11000f", "#12000f", "#13000f", "#14000f",
                "#15000f", "#16000f",
            ],
        )
    );
}
