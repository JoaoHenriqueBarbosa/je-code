//! Porte de `src/vs/workbench/contrib/terminal/test/browser/xterm/markNavigationAddon.test.ts` (suíte `MarkNavigationAddon`), caso a
//! caso e na mesma ordem.
//!
//! Notas de tradução:
//! - **`ensureNoDisposablesAreLeakedInTestSuite()`.** É o guarda criado no começo do caso; o emulador, o store de capacidades, a
//!   capacidade de marcas e o addon são descartados por ele (no original, `store.add(...)`).
//! - **`importAMDNodeModule('@xterm/xterm', ...)`, `allowProposedApi` e `logger: TestXtermLogger`.** Não há módulo a importar nem
//!   logger a injetar: o emulador é o [`FakeXterm`] de 80 colunas e 30 linhas.
//! - **`TestBufferMarkCapability extends BufferMarkCapability`.** A capacidade do porte não é uma classe estendível: o
//!   [`TestBufferMarkCapability`] a embrulha, conta as chamadas de `markers()` e falha na terceira, como a subclasse do original
//!   (`throw new Error('Buffer mark iterator was requested repeatedly')`).
//! - **`workbenchInstantiationService(...)` e `instantiationService.createInstance(MarkNavigationAddon, capabilities)`.** O
//!   contêiner de injeção não entra: o addon recebe o `TestConfigurationService` vazio (o `new TestConfigurationService()` do caso)
//!   e o tema vazio (o `TestThemeService` do `workbenchInstantiationService`, sem cor definida).
//! - **`terminal.loadAddon(addon)`.** É [`MarkNavigationAddon::activate`].
//! - **`addon.scrollToNextMark(undefined, undefined, false)`.** Os parâmetros ausentes são `None`, e o método devolve `Result`
//!   (os `throw` do original).

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod support;

use std::cell::Cell;
use std::rc::Rc;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::event::Event;
use je_base::lifecycle::IDisposable;
use je_platform::configuration::service::{IConfigurationService};
use je_platform::configuration::test_configuration_service::TestConfigurationService;
use je_platform::terminal::capabilities::buffer_mark_capability::BufferMarkCapability;
use je_platform::terminal::capabilities::capabilities::{BufferMarkDetection, IBufferMarkCapability, IMarkProperties, IMarker, ITerminalCapabilityStore, TerminalCapability};
use je_platform::terminal::capabilities::terminal_capability_store::TerminalCapabilityStore;
use je_platform::terminal::xterm_headless::IHeadlessTerminal;
use je_workbench::contrib::terminal::browser::xterm::mark_navigation_addon::{IMarkNavigationTerminal, MarkNavigationAddon};
use support::fake_xterm::FakeXterm;
use support::terminal_decoration_support::EmptyThemeService;

/// `TestBufferMarkCapability`: conta as chamadas de `markers()` e falha se passarem de duas.
struct TestBufferMarkCapability {
    inner: Rc<BufferMarkCapability>,
    marker_iterator_calls: Cell<u32>,
}

impl TestBufferMarkCapability {
    fn new(terminal: Rc<dyn IHeadlessTerminal>) -> Rc<Self> {
        Rc::new(Self { inner: BufferMarkCapability::new(terminal), marker_iterator_calls: Cell::new(0) })
    }
}

impl IDisposable for TestBufferMarkCapability {
    fn dispose(&self) {
        self.inner.dispose();
    }
}

impl IBufferMarkCapability for TestBufferMarkCapability {
    fn capability_type(&self) -> TerminalCapability {
        self.inner.capability_type()
    }

    fn markers(&self) -> Box<dyn Iterator<Item = Rc<dyn IMarker>>> {
        self.marker_iterator_calls.set(self.marker_iterator_calls.get() + 1);
        if self.marker_iterator_calls.get() > 2 {
            panic!("Buffer mark iterator was requested repeatedly");
        }
        self.inner.markers()
    }

    fn on_mark_added(&self) -> Event<IMarkProperties> {
        self.inner.on_mark_added()
    }

    fn add_mark(&self, properties: Option<IMarkProperties>) {
        self.inner.add_mark(properties);
    }

    fn get_mark(&self, id: &str) -> Option<Rc<dyn IMarker>> {
        self.inner.get_mark(id)
    }
}

#[test]
fn should_consume_buffer_mark_iterators_when_navigating() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let terminal = store.add_rc(FakeXterm::new(80, 30));
    let capabilities = store.add_rc(TerminalCapabilityStore::new());
    let headless: Rc<dyn IHeadlessTerminal> = terminal.clone();
    let mark_capability = store.add_rc(TestBufferMarkCapability::new(headless));
    mark_capability.add_mark(None);
    let buffer_mark_capability: Rc<dyn IBufferMarkCapability> = mark_capability.clone();
    capabilities.add::<BufferMarkDetection>(buffer_mark_capability);

    let configuration_service = TestConfigurationService::new(None);
    // O emissor do `TestConfigurationService` é descartado pelo store do caso (o original não o descarta).
    store.add_rc(configuration_service.on_did_change_configuration_emitter.clone());
    let configuration: Rc<dyn IConfigurationService> = configuration_service.clone();
    let capability_store: Rc<dyn ITerminalCapabilityStore> = capabilities.clone();
    let addon = store.add_rc(MarkNavigationAddon::new(capability_store, configuration, Rc::new(EmptyThemeService)));
    let navigation_terminal: Rc<dyn IMarkNavigationTerminal> = terminal.clone();
    addon.activate(navigation_terminal);

    addon.scroll_to_next_mark(None, None, Some(false)).expect("a navegação não falha");

    assert_eq!(mark_capability.marker_iterator_calls.get(), 2);
}
