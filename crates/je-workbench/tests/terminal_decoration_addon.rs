//! Porte de `src/vs/workbench/contrib/terminal/test/browser/xterm/decorationAddon.test.ts` (suíte `DecorationAddon`), caso a caso e na
//! mesma ordem.
//!
//! Notas de tradução:
//! - **`setup` e `ensureNoDisposablesAreLeakedInTestSuite()`.** O [`Fixture`] cria o guarda de vazamentos antes de tudo e depois
//!   monta o `TestTerminal` (o [`FakeXterm`] de 80 colunas e 30 linhas, cujo `registerDecoration` devolve a decoração e o elemento
//!   de mentira de `support/fake_xterm_decoration.rs`, sem decoração para marcador descartado, como o `override` do original), o
//!   store de capacidades com a `CommandDetectionCapability` real e o addon. O `xterm.loadAddon(decorationAddon)` é
//!   [`DecorationAddon::activate`], e o addon e o emulador são descartados pelo guarda.
//! - **`importAMDNodeModule(...)` e `logger: TestXtermLogger`.** Não há módulo a importar nem logger a injetar.
//! - **`workbenchInstantiationService({ configurationService: ... }, store)` e `createInstance(DecorationAddon, undefined,
//!   capabilities)`.** O contêiner de injeção não entra: o addon recebe o `TestConfigurationService` do `setup` (com
//!   `decorationsEnabled: 'both'`) e o `IHoverService` do `instantiationService.stub` ([`StubHoverService`], que liga
//!   `hoverDisposed` ao descartar). Os demais serviços do `workbenchInstantiationService` que o addon recebe e nenhum caso usa são
//!   os dublês de `support/terminal_decoration_support.rs`. O `resource` `undefined` não existe (ver o topo de
//!   `decoration_addon.rs`: o chat está fora do porte).
//! - **`removeEventListener` do `div`.** O elemento de teste registra o tipo de cada `removeEventListener` num vetor do emulador
//!   (`removedEventListeners`), que o caso lê por [`FakeXterm::removed_event_listeners`].
//! - **`(decorationAddon as unknown as { _decorations: Map<number, unknown> })._decorations`.** É [`DecorationAddon::has_decoration`],
//!   a leitura do mapa privado.
//! - **`{ command, timestamp: Date.now(), hasOutput: () => false } as ITerminalCommand`.** É o [`DecorationCommandLiteral`]: o que o
//!   literal não define vale `undefined`.
//! - **`throws(() => ...)`.** O `Result` de `register_command_decoration` é `Err`; `strictEqual(..., undefined)` e `notEqual(...,
//!   undefined)` são `Ok(None)` e `Ok(Some(_))`.
//! - **`Date.now()`.** É `runtime::date_now()`.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod support;

use std::cell::Cell;
use std::rc::Rc;

use common::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite};
use je_base::dynamic::Dynamic;
use je_base::runtime;
use je_platform::configuration::service::IConfigurationService;
use je_platform::configuration::test_configuration_service::TestConfigurationService;
use je_platform::log::NullLogService;
use je_platform::terminal::capabilities::capabilities::{CommandDetection, ICommandDetectionCapability, IMarkProperties, ITerminalCapabilityStore};
use je_platform::terminal::capabilities::command_detection_capability::CommandDetectionCapability;
use je_platform::terminal::capabilities::terminal_capability_store::TerminalCapabilityStore;
use je_platform::terminal::xterm_headless::IHeadlessTerminal;
use je_workbench::contrib::terminal::browser::xterm::decoration_addon::{DecorationAddon, DecorationAddonServices, IDecorationAddonTerminal};
use support::fake_xterm::FakeXterm;
use support::mock_command_service::MockCommandService;
use support::terminal_contrib_support::PanickingNotificationService;
use support::terminal_decoration_support::{
    DecorationCommandLiteral, EmptyThemeService, SilentAccessibilitySignalService, StubHoverService, UnusedClipboardService, UnusedContextMenuService,
    UnusedOpenerService, UnusedQuickInputService,
};

/// Um objeto literal `{ key: value, ... }`.
fn obj<const N: usize>(entries: [(&str, Dynamic); N]) -> Dynamic {
    Dynamic::object(entries)
}

struct Fixture {
    decoration_addon: Rc<DecorationAddon>,
    xterm: Rc<FakeXterm>,
    hover_disposed: Rc<Cell<bool>>,
    // Declarado por último: o `Drop` dele descarta o que o `Fixture` registrou e confere os vazamentos.
    _store: LeakGuard,
}

impl Fixture {
    /// O `setup` da suíte.
    fn new() -> Self {
        let store = ensure_no_disposables_are_leaked_in_test_suite();
        let hover_disposed = Rc::new(Cell::new(false));

        let configuration_service = TestConfigurationService::new(Some(obj([
            ("files", obj([])),
            ("workbench", obj([("hover", obj([("delay", Dynamic::Number(5.0))]))])),
            ("terminal", obj([("integrated", obj([("shellIntegration", obj([("decorationsEnabled", Dynamic::string("both"))]))]))])),
        ])));
        // O emissor do `TestConfigurationService` é descartado pelo store do caso (o original não o descarta).
        store.add_rc(configuration_service.on_did_change_configuration_emitter.clone());
        let configuration: Rc<dyn IConfigurationService> = configuration_service.clone();

        let xterm = store.add_rc(FakeXterm::new(80, 30));
        let capabilities = store.add_rc(TerminalCapabilityStore::new());
        let headless: Rc<dyn IHeadlessTerminal> = xterm.clone();
        let command_detection = store.add_rc(CommandDetectionCapability::new(headless, NullLogService::shared()));
        let command_detection_capability: Rc<dyn ICommandDetectionCapability> = command_detection.clone();
        capabilities.add::<CommandDetection>(command_detection_capability);

        let capability_store: Rc<dyn ITerminalCapabilityStore> = capabilities.clone();
        let decoration_addon = store.add_rc(DecorationAddon::new(
            capability_store,
            DecorationAddonServices {
                clipboard_service: Rc::new(UnusedClipboardService),
                context_menu_service: Rc::new(UnusedContextMenuService),
                configuration_service: configuration,
                theme_service: Rc::new(EmptyThemeService),
                opener_service: Rc::new(UnusedOpenerService),
                quick_input_service: Rc::new(UnusedQuickInputService),
                command_service: Rc::new(MockCommandService::default()),
                accessibility_signal_service: Rc::new(SilentAccessibilitySignalService),
                notification_service: Rc::new(PanickingNotificationService),
                hover_service: Rc::new(StubHoverService { hover_disposed: hover_disposed.clone() }),
            },
        ));
        let terminal: Rc<dyn IDecorationAddonTerminal> = xterm.clone();
        decoration_addon.activate(terminal);
        Self { decoration_addon, xterm, hover_disposed, _store: store }
    }
}

/// `Date.now()`.
fn now() -> f64 {
    runtime::date_now() as f64
}

// ---------------------------------------------------------------------------------------------------------------------
// suite('registerDecoration')
// ---------------------------------------------------------------------------------------------------------------------

#[test]
fn register_decoration_should_throw_when_command_has_no_marker() {
    let f = Fixture::new();
    let command = DecorationCommandLiteral::create("cd src", None, None, now());
    assert!(f.decoration_addon.register_command_decoration(Some(command), None, None).is_err());
}

#[test]
fn register_decoration_should_return_undefined_when_marker_has_been_disposed_of() {
    let f = Fixture::new();
    let marker = f.xterm.register_marker(1);
    if let Some(marker) = &marker {
        marker.dispose();
    }
    let command = DecorationCommandLiteral::create("cd src", marker, None, now());
    assert!(matches!(f.decoration_addon.register_command_decoration(Some(command), None, None), Ok(None)));
}

#[test]
fn register_decoration_should_return_decoration_when_marker_has_not_been_disposed_of() {
    let f = Fixture::new();
    let marker = f.xterm.register_marker(2);
    let command = DecorationCommandLiteral::create("cd src", marker, None, now());
    assert!(matches!(f.decoration_addon.register_command_decoration(Some(command), None, None), Ok(Some(_))));
}

#[test]
fn register_decoration_should_return_decoration_with_mark_properties() {
    let f = Fixture::new();
    let marker = f.xterm.register_marker(2);
    let mark_properties = IMarkProperties { marker, ..Default::default() };
    assert!(matches!(f.decoration_addon.register_command_decoration(None, None, Some(mark_properties)), Ok(Some(_))));
}

#[test]
fn register_decoration_should_dispose_decoration_resources_when_the_decoration_is_disposed() {
    let f = Fixture::new();
    let marker = f.xterm.register_marker(2).expect("o marcador é registrado");
    let command = DecorationCommandLiteral::create("cd src", Some(marker.clone()), Some(0.0), now());
    let decoration = f
        .decoration_addon
        .register_command_decoration(Some(command), None, None)
        .expect("o comando tem marcador")
        .expect("o marcador não foi descartado");

    decoration.dispose();

    assert!(f.hover_disposed.get());
    let mut removed_event_listeners = f.xterm.removed_event_listeners();
    removed_event_listeners.sort();
    assert_eq!(removed_event_listeners, vec!["click".to_owned(), "contextmenu".to_owned(), "mousedown".to_owned()]);
    assert!(!f.decoration_addon.has_decoration(marker.id()));
}
