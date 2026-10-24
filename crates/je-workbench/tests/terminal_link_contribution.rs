//! Porte de `src/vs/workbench/contrib/terminalContrib/links/test/browser/terminalLinkContribution.test.ts` (suíte `TerminalLinkContribution`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - **`setup` e `ensureNoDisposablesAreLeakedInTestSuite()`.** O [`Fixture`] cria o guarda de vazamentos antes de tudo, os dois
//!   emissores de provedor de link, o `ITerminalLinkProviderService` falso (`link_providers` vazio e os dois eventos), o
//!   resolvedor (`stubInstance(TerminalLinkResolver, {})`, que aqui é o resolvedor real, que o gerenciador falso nunca usa) e o
//!   gerenciador falso.
//! - **`instantiationService.stubInstance(TerminalLinkManager, { add, setWidgetManager, dispose })`.** É o [`StubLinkManager`], e o
//!   `TerminalLinkManagerFactory` que a contribuição recebe devolve esse dublê (todos eles dividem o `linkManagerStore`).
//! - **`xterm = Object.assign(Object.create(null), { raw: Object.create(null) })`.** É o [`StubXterm`], com um `FakeXterm` como
//!   `raw` (a contribuição só repassa o `raw` ao gerenciador).
//! - **`Object.create(null) as ITerminalProcessManager`.** O `isTerminalProcessManager` do original olha se o objeto tem `write`,
//!   e um objeto nulo não tem: nos dois casos a contribuição vê só um `ITerminalProcessInfo`. O contexto leva o
//!   `TerminalContributionProcess::Info` com o `PartialProcessInfo`.
//! - **`Object.create(TerminalWidgetManager.prototype)`.** É um `TerminalWidgetManager::new()` sem contêiner.
//! - **`listenerCount(emitter)` (`_size`).** É o `size_for_testing` do emissor.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod support;

use std::rc::Rc;

use common::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite};
use je_base::event::{Emitter, Event};
use je_base::lifecycle::{DisposableRef, DisposableStore, IDisposable};
use je_base::promise::Promise;
use je_platform::files::i_file_service::IFileService;
use je_platform::terminal::capabilities::terminal_capability_store::TerminalCapabilityStore;
use je_workbench::contrib::terminal::browser::terminal::{ITerminalExternalLinkProvider, ITerminalInstance};
use je_workbench::contrib::terminal::browser::terminal_extensions::{
    IDetachedTerminalInstance, ITerminalContribution, IXtermRawTerminal, IXtermTerminal, TerminalContributionContext, TerminalContributionInstance, TerminalContributionProcess,
};
use je_workbench::contrib::terminal::browser::widgets::widget_manager::{ITerminalWidgetManager, TerminalWidgetManager};
use je_workbench::contrib::terminal_contrib::links::links::{ITerminalLinkProviderService, ITerminalLinkResolver};
use je_workbench::contrib::terminal_contrib::links::terminal_link_contribution::{TerminalLinkContribution, TerminalLinkManagerFactory};
use je_workbench::contrib::terminal_contrib::links::terminal_link_manager::{
    DetectedLinks, ExternalProvideLinks, ITerminalLinkManager, RecentLinkKind,
};
use je_workbench::contrib::terminal_contrib::links::terminal_link_resolver::TerminalLinkResolver;
use support::fake_xterm::FakeXterm;
use support::terminal_contrib_support::{PartialProcessInfo, PartialTerminalInstance, unmocked};
use support::terminal_link_support::LinkTestFileService;

/// `new class extends mock<ITerminalLinkProviderService>() { linkProviders; onDidAddLinkProvider; onDidRemoveLinkProvider }`.
struct StubLinkProviderService {
    on_did_add_link_provider: Rc<Emitter<Rc<dyn ITerminalExternalLinkProvider>>>,
    on_did_remove_link_provider: Rc<Emitter<Rc<dyn ITerminalExternalLinkProvider>>>,
}

impl ITerminalLinkProviderService for StubLinkProviderService {
    fn link_providers(&self) -> Vec<Rc<dyn ITerminalExternalLinkProvider>> {
        Vec::new()
    }

    fn on_did_add_link_provider(&self) -> Event<Rc<dyn ITerminalExternalLinkProvider>> {
        self.on_did_add_link_provider.event()
    }

    fn on_did_remove_link_provider(&self) -> Event<Rc<dyn ITerminalExternalLinkProvider>> {
        self.on_did_remove_link_provider.event()
    }

    fn register_link_provider(&self, _provider: Rc<dyn ITerminalExternalLinkProvider>) -> DisposableRef {
        unmocked("registerLinkProvider")
    }
}

/// `stubInstance(TerminalLinkManager, { add, setWidgetManager, dispose })`.
struct StubLinkManager {
    link_manager_store: Rc<DisposableStore>,
}

impl IDisposable for StubLinkManager {
    fn dispose(&self) {
        self.link_manager_store.dispose();
    }
}

impl ITerminalLinkManager for StubLinkManager {
    fn add(&self, disposable: DisposableRef) -> DisposableRef {
        self.link_manager_store.add(disposable)
    }

    fn set_widget_manager(&self, _widget_manager: Rc<dyn ITerminalWidgetManager>) {}

    fn set_external_provide_links_cb(&self, _callback: Option<ExternalProvideLinks>) {
        unmocked("externalProvideLinksCb")
    }

    fn get_links(&self) -> Promise<DetectedLinks> {
        unmocked("getLinks")
    }

    fn open_recent_link(&self, _kind: RecentLinkKind) -> Promise<Option<Rc<dyn je_workbench::contrib::terminal::browser::terminal_extensions::IXtermLink>>> {
        unmocked("openRecentLink")
    }
}

/// `xterm`: o `IXtermTerminal` com um `raw`.
struct StubXterm {
    raw: Rc<FakeXterm>,
}

impl IXtermTerminal for StubXterm {
    fn raw(&self) -> Rc<dyn IXtermRawTerminal> {
        self.raw.clone()
    }

    fn on_did_request_copy_as_html(&self) -> Event<Rc<dyn je_platform::terminal::capabilities::capabilities::ITerminalCommand>> {
        unmocked("onDidRequestCopyAsHtml")
    }

    fn copy_selection(&self, _as_html: bool, _command: Option<Rc<dyn je_platform::terminal::capabilities::capabilities::ITerminalCommand>>) {
        unmocked("copySelection")
    }
}

/// `Object.assign(Object.create(null) as IDetachedTerminalInstance, { capabilities })`.
struct StubDetachedInstance {
    capabilities: Rc<TerminalCapabilityStore>,
}

impl IDetachedTerminalInstance for StubDetachedInstance {
    fn capabilities(&self) -> Rc<dyn je_platform::terminal::capabilities::capabilities::ITerminalCapabilityStore> {
        self.capabilities.clone()
    }
}

/// O `setup` da suíte.
struct Fixture {
    on_did_add_link_provider: Rc<Emitter<Rc<dyn ITerminalExternalLinkProvider>>>,
    on_did_remove_link_provider: Rc<Emitter<Rc<dyn ITerminalExternalLinkProvider>>>,
    link_provider_service: Rc<StubLinkProviderService>,
    link_resolver: Rc<dyn ITerminalLinkResolver>,
    link_manager_factory: TerminalLinkManagerFactory,
    xterm: Rc<StubXterm>,
    // Declarado por último: o `Drop` dele descarta o que o `Fixture` registrou e confere os vazamentos.
    guard: LeakGuard,
}

impl Fixture {
    fn new() -> Self {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let on_did_add_link_provider = guard.add_rc(Emitter::new());
        let on_did_remove_link_provider = guard.add_rc(Emitter::new());
        let link_provider_service =
            Rc::new(StubLinkProviderService { on_did_add_link_provider: on_did_add_link_provider.clone(), on_did_remove_link_provider: on_did_remove_link_provider.clone() });
        let file_service = LinkTestFileService::new();
        let link_resolver: Rc<dyn ITerminalLinkResolver> = Rc::new(TerminalLinkResolver::new(file_service as Rc<dyn IFileService>));

        let link_manager_store = guard.add_rc(DisposableStore::new());
        let link_manager_factory: TerminalLinkManagerFactory =
            Rc::new(move |_raw, _process_info, _capabilities, _resolver| Rc::new(StubLinkManager { link_manager_store: link_manager_store.clone() }) as Rc<dyn ITerminalLinkManager>);
        let xterm = Rc::new(StubXterm { raw: guard.add_rc(FakeXterm::new(80, 30)) });
        Self { on_did_add_link_provider, on_did_remove_link_provider, link_provider_service, link_resolver, link_manager_factory, xterm, guard }
    }

    /// `createContribution(detached)`.
    fn create_contribution(&self, detached: bool) -> Rc<TerminalLinkContribution> {
        let capabilities = self.guard.add_rc(TerminalCapabilityStore::new());
        let widget_manager = TerminalWidgetManager::new();
        let instance = if detached {
            TerminalContributionInstance::Detached(Rc::new(StubDetachedInstance { capabilities }))
        } else {
            TerminalContributionInstance::Full(Rc::new(PartialTerminalInstance { instance_id: Some(1), capabilities: Some(capabilities), ..PartialTerminalInstance::default() }) as Rc<dyn ITerminalInstance>)
        };
        let context = TerminalContributionContext {
            instance,
            process_manager: TerminalContributionProcess::Info(Rc::new(PartialProcessInfo { initial_cwd: String::new() })),
            widget_manager,
        };
        let contribution = self.guard.add_rc(TerminalLinkContribution::new(context, self.link_manager_factory.clone(), self.link_provider_service.clone(), self.link_resolver.clone()));
        contribution.xterm_ready(self.xterm.clone());
        contribution
    }

    /// `{ addedListeners: listenerCount(onDidAddLinkProvider), removedListeners: listenerCount(onDidRemoveLinkProvider) }`.
    fn listener_counts(&self) -> (usize, usize) {
        (self.on_did_add_link_provider.size_for_testing(), self.on_did_remove_link_provider.size_for_testing())
    }
}

#[test]
fn does_not_register_external_link_provider_listeners_for_detached_terminals() {
    let f = Fixture::new();
    for _ in 0..50 {
        f.create_contribution(true);
    }

    assert_eq!(f.listener_counts(), (0, 0));
}

#[test]
fn registers_and_disposes_external_link_provider_listeners_for_regular_terminals() {
    let f = Fixture::new();
    let contribution = f.create_contribution(false);
    let counts_after_registration = f.listener_counts();

    contribution.dispose();

    assert_eq!((counts_after_registration, f.listener_counts()), ((1, 1), (0, 0)));
}
