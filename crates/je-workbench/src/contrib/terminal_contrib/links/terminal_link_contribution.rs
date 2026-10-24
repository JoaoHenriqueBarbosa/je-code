//! Porte de `src/vs/workbench/contrib/terminalContrib/links/browser/terminalLinkContribution.ts`: a contribuição que cria o
//! gerenciador de links do terminal quando o emulador fica pronto, liga o gerenciador de widgets e repassa os provedores de
//! links externos (das extensões).
//!
//! Decisões de tradução:
//! - **`extends DisposableStore`** é o campo `store` com o [`IDisposable`].
//! - **`createInstance(TerminalLinkResolver)` e `createInstance(TerminalLinkManager, xterm.raw, processManager, capabilities,
//!   linkResolver)`.** O resolvedor e o construtor do gerenciador são entregues a [`TerminalLinkContribution::new`] (o teste
//!   do original troca os dois com `stubInstance`): o [`TerminalLinkManagerFactory`] recebe o emulador, o
//!   `ITerminalProcessInfo` (o gerenciador lê o retrato dele), as capacidades e o resolvedor.
//! - **`isTerminalProcessManager(processManager)`** é a variante [`TerminalContributionProcess::Manager`], e
//!   **`isDetachedTerminalInstance(instance)`** é a variante [`TerminalContributionInstance::Detached`].
//! - **`provideLinks.bind(provider, instance)`** é um fecho que repassa a instância.
//! - **`instance.getContribution(TerminalLinkContribution.ID)`** (`static get`) não existe: o `ITerminalInstance` do porte não
//!   guarda contribuições.
//! - **Fora do porte: o quick pick de links** (`showLinkQuickpick` e `TerminalLinkQuickpick`, `terminalLinkQuickpick.ts`),
//!   que depende do quick input, do rótulo e do editor, ainda sem contrato de uso no terminal.

use std::cell::RefCell;
use std::rc::Rc;

use je_base::lifecycle::{DisposableRef, DisposableStore, IDisposable};
use je_base::promise::Promise;
use je_platform::terminal::capabilities::capabilities::ITerminalCapabilityStore;

use super::links::{ITerminalLinkProviderService, ITerminalLinkResolver};
use super::terminal_link_manager::{DetectedLinks, ITerminalLinkManager, RecentLinkKind};
use crate::contrib::terminal::browser::terminal_extensions::{
    ITerminalContribution, IXtermLink, IXtermRawTerminal, IXtermTerminal, TerminalContributionContext, TerminalContributionInstance, TerminalContributionProcess,
    register_terminal_contribution,
};
use crate::contrib::terminal::common::terminal::ITerminalProcessInfo;

/// `createInstance(TerminalLinkManager, ...)`.
pub type TerminalLinkManagerFactory =
    Rc<dyn Fn(Rc<dyn IXtermRawTerminal>, Rc<dyn ITerminalProcessInfo>, Rc<dyn ITerminalCapabilityStore>, Rc<dyn ITerminalLinkResolver>) -> Rc<dyn ITerminalLinkManager>>;

/// `TerminalLinkContribution`.
pub struct TerminalLinkContribution {
    store: Rc<DisposableStore>,
    ctx: TerminalContributionContext,
    link_manager_factory: TerminalLinkManagerFactory,
    link_provider_service: Rc<dyn ITerminalLinkProviderService>,
    link_resolver: Rc<dyn ITerminalLinkResolver>,
    link_manager: RefCell<Option<Rc<dyn ITerminalLinkManager>>>,
}

impl TerminalLinkContribution {
    pub const ID: &'static str = "terminal.link";

    pub fn new(
        ctx: TerminalContributionContext,
        link_manager_factory: TerminalLinkManagerFactory,
        link_provider_service: Rc<dyn ITerminalLinkProviderService>,
        link_resolver: Rc<dyn ITerminalLinkResolver>,
    ) -> Rc<Self> {
        Rc::new(Self { store: DisposableStore::new(), ctx, link_manager_factory, link_provider_service, link_resolver, link_manager: RefCell::new(None) })
    }

    fn process_info(&self) -> Rc<dyn ITerminalProcessInfo> {
        match &self.ctx.process_manager {
            TerminalContributionProcess::Manager(process_manager) => process_manager.clone() as Rc<dyn ITerminalProcessInfo>,
            TerminalContributionProcess::Info(info) => info.clone(),
        }
    }

    fn get_link_manager(&self) -> Option<Rc<dyn ITerminalLinkManager>> {
        self.link_manager.borrow().clone()
    }

    /// `openRecentLink(type)`.
    pub fn open_recent_link(&self, kind: RecentLinkKind) -> Result<Promise<Option<Rc<dyn IXtermLink>>>, je_base::errors::Error> {
        let link_manager = self.get_link_manager().ok_or_else(|| je_base::errors::Error::new("terminal links are not ready, cannot open a link"))?;
        Ok(link_manager.open_recent_link(kind))
    }

    /// `_getLinks()`.
    pub fn get_links(&self) -> Result<Promise<DetectedLinks>, je_base::errors::Error> {
        let link_manager = self.get_link_manager().ok_or_else(|| je_base::errors::Error::new("terminal links are not ready, cannot generate link quick pick"))?;
        Ok(link_manager.get_links())
    }
}

impl IDisposable for TerminalLinkContribution {
    fn dispose(&self) {
        self.store.dispose();
    }
}

impl ITerminalContribution for TerminalLinkContribution {
    fn xterm_ready(&self, xterm: Rc<dyn IXtermTerminal>) {
        let link_manager = (self.link_manager_factory)(xterm.raw(), self.process_info(), self.ctx.instance.capabilities(), self.link_resolver.clone());
        self.store.add(link_manager_disposable(&link_manager));
        *self.link_manager.borrow_mut() = Some(link_manager.clone());

        let widget_manager = self.ctx.widget_manager.clone();
        match &self.ctx.process_manager {
            TerminalContributionProcess::Manager(process_manager) => {
                let slot: Rc<RefCell<Option<DisposableRef>>> = Rc::default();
                let (store, manager, slot_for_listener) = (self.store.clone(), link_manager.clone(), slot.clone());
                let disposable = link_manager.add(process_manager.on_process_ready().once().listen(move |_| {
                    manager.set_widget_manager(widget_manager.clone());
                    if let Some(disposable) = slot_for_listener.borrow_mut().take() {
                        store.delete(&disposable);
                    }
                }));
                *slot.borrow_mut() = Some(disposable);
            }
            TerminalContributionProcess::Info(_) => link_manager.set_widget_manager(widget_manager),
        }

        if let TerminalContributionInstance::Full(instance) = &self.ctx.instance {
            for link_provider in self.link_provider_service.link_providers() {
                let (provider, instance) = (link_provider.clone(), instance.clone());
                link_manager.set_external_provide_links_cb(Some(Rc::new(move |line: &str| provider.provide_links(instance.clone(), line))));
            }
            let (manager, instance) = (link_manager.clone(), instance.clone());
            link_manager.add(self.link_provider_service.on_did_add_link_provider().listen(move |provider| {
                let (provider, instance) = (provider.clone(), instance.clone());
                manager.set_external_provide_links_cb(Some(Rc::new(move |line: &str| provider.provide_links(instance.clone(), line))));
            }));
            let manager = link_manager.clone();
            link_manager.add(self.link_provider_service.on_did_remove_link_provider().listen(move |_| manager.set_external_provide_links_cb(None)));
        }
    }
}

/// `this.add(linkManager)`: o gerenciador é descartado junto com a contribuição.
fn link_manager_disposable(link_manager: &Rc<dyn ITerminalLinkManager>) -> DisposableRef {
    let link_manager = link_manager.clone();
    je_base::lifecycle::plain_disposable(move || link_manager.dispose())
}

/// `registerTerminalContribution(TerminalLinkContribution.ID, TerminalLinkContribution, true)` de
/// `terminal.links.contribution.ts`: quem monta os serviços chama esta função com o que o construtor lê.
pub fn register_terminal_link_contribution(
    link_manager_factory: TerminalLinkManagerFactory,
    link_provider_service: Rc<dyn ITerminalLinkProviderService>,
    link_resolver: Rc<dyn ITerminalLinkResolver>,
) {
    register_terminal_contribution(
        TerminalLinkContribution::ID,
        Rc::new(move |ctx| {
            TerminalLinkContribution::new(ctx, link_manager_factory.clone(), link_provider_service.clone(), link_resolver.clone()) as Rc<dyn ITerminalContribution>
        }),
        true,
    );
}
