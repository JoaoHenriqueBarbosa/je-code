//! Porte de `src/vs/workbench/contrib/terminalContrib/links/browser/terminalLinkManager.ts`: o gerenciador do registro dos
//! detectores de links e dos provedores de links do terminal, dos abridores, do `linkHandler` (OSC 8) e do hover.
//!
//! Decisões de tradução:
//! - **`extends DisposableStore`** é o campo `store` (um `DisposableStore`) com [`TerminalLinkManager::add`] e o [`IDisposable`].
//! - **Serviços injetados.** O `IInstantiationService.createInstance` das classes que o construtor monta é a chamada direta ao
//!   `new` de cada uma, com os serviços do [`TerminalLinkManagerServices`]. `IHoverService` é o [`ITerminalHoverService`] (o
//!   frontend implementa a apresentação do hover).
//! - **Abridores.** O original cria os cinco abridores no construtor (`createInstance(TerminalLocalFileLinkOpener)` e os demais).
//!   Aqui quem constrói o gerenciador entrega os abridores prontos, por tipo de link: o `TerminalSearchLinkOpener` lê
//!   `capabilities`, `initialCwd` e o SO do processo, e o `TerminalUrlLinkOpener` lê se o terminal é remoto, e os dois leem sete
//!   serviços que o gerenciador não usa.
//! - **`_processInfo: ITerminalProcessInfo`.** Os detectores recebem o [`TerminalLinkProcessManager`] (um retrato de
//!   `initialCwd`, `os`, `remoteAuthority`, `userHome` e `backend`) tirado quando o gerenciador é criado, e não as leituras ao
//!   vivo do original. O `backend` entra pelo [`BackendWslPath`].
//! - **`protected _getLinksForType` e `private _showHover`.** O teste do original sobrescreve o primeiro numa subclasse e
//!   substitui o segundo por atribuição de propriedade na instância. Aqui são os pontos de substituição
//!   [`TerminalLinkManager::set_links_for_type_override`] e [`TerminalLinkManager::set_show_hover_override`]; o
//!   [`TerminalLinkManager::show_hover`] é público porque o teste o chama (por um cast para um tipo com o membro privado).
//! - **`this._standardLinkProviders.get(id)?.provideLinks(y, r)` dentro de `new Promise(r => ...)`.** Sem o provedor (os
//!   links de arquivo desligados) a promessa nunca resolve, como no original; o porte devolve uma promessa pendente.
//! - **`ILink[]` do xterm** é `Vec<Rc<dyn IXtermLink>>`.
//! - **`bufferRange: null!`** do link sintético do `linkHandler.activate` é o intervalo zerado (o abridor de URL não o lê).
//! - **`MouseEvent`** é o [`TerminalMouseEvent`]; o `TerminalLinkQuickPickEvent` é o evento com `is_link_quick_pick`.
//! - **`this._xterm._core._renderService.dimensions.css.cell`** é o `IXtermRawTerminal::css_cell_dimensions`.

use std::cell::RefCell;
use std::rc::{Rc, Weak};

use je_base::async_::{RunOnceScheduler, async_fn};
use je_base::dynamic::Dynamic;
use je_base::errors::Error;
use je_base::html_content::{MarkdownString, MarkdownStringTextNewlineStyle, TrustedOrOptions, UriOrOwnedString};
use je_base::js_string::JsString;
use je_base::lifecycle::{DisposableRef, DisposableStore, IDisposable, MutableDisposable, plain_disposable};
use je_base::nls::localize;
use je_base::platform::Platform;
use je_base::promise::Promise;
use je_base::severity::Severity;
use je_base::uri::URI;
use je_platform::configuration::service::IConfigurationService;
use je_platform::log::LogServiceRef;
use je_platform::notification::{BasePromptChoice, INotificationService, NotificationMessage, PromptChoice, PromptChoiceKind, PromptOptions};
use je_platform::telemetry::telemetry::ITelemetryService;
use je_platform::terminal::capabilities::capabilities::ITerminalCapabilityStore;
use je_platform::tunnel::ITunnelService;
use je_platform::uri_identity::uri_identity::IUriIdentityService;
use je_platform::workspace::IWorkspaceContextService;
use je_widgets::hover_api::IHoverAction;
use zyarr::RegExp;

use super::links::{
    ITerminalLinkDetector, ITerminalLinkOpener, ITerminalLinkResolver, ITerminalSimpleLink, IWslPathBackend, TerminalBuiltinLinkType, TerminalLinkProcessManager,
    TerminalLinkType, WslPathDirection,
};
use super::terminal_external_link_detector::TerminalExternalLinkDetector;
use super::terminal_link::{IInvalidatableLink, TerminalLink};
use super::terminal_link_detector_adapter::{IActivateLinkEvent, IShowHoverEvent, TerminalLinkDetectorAdapter};
use super::terminal_link_helpers::convert_buffer_range_to_viewport;
use super::terminal_local_link_detector::TerminalLocalLinkDetector;
use super::terminal_multi_line_link_detector::TerminalMultiLineLinkDetector;
use super::terminal_uri_link_detector::TerminalUriLinkDetector;
use super::terminal_word_link_detector::TerminalWordLinkDetector;
use crate::contrib::terminal::browser::terminal::{ITerminalConfigurationService, ITerminalLink};
use crate::contrib::terminal::browser::terminal_extensions::{ILinkProvider, IXtermLink, IXtermRawTerminal, LinkHandler};
use crate::contrib::terminal::browser::terminal_input_events::TerminalMouseEvent;
use crate::contrib::terminal::browser::widgets::terminal_hover_widget::{Dimensions, ILinkHoverTargetOptions, ITerminalHoverService, TerminalHover};
use crate::contrib::terminal::browser::widgets::widget_manager::ITerminalWidgetManager;
use crate::contrib::terminal::common::terminal::{ITerminalProcessInfo, TERMINAL_CONFIG_SECTION};
use crate::contrib::terminal::common::terminal_backend::ITerminalBackend;
use crate::contrib::terminal::common::terminal_environment::{ITerminalBackendWslPath, WslPathDirection as BackendWslPathDirection};
use crate::contrib::terminal::xterm_buffer::{IBufferCellPosition, IBufferRange, ITerminal};

/// `ILineColumnInfo`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ILineColumnInfo {
    pub line_number: i32,
    pub column_number: i32,
}

/// `IDetectedLinks`: os links de uma linha, por tipo.
#[derive(Clone, Default)]
pub struct IDetectedLinks {
    pub word_links: Option<Vec<Rc<dyn IXtermLink>>>,
    pub web_links: Option<Vec<Rc<dyn IXtermLink>>>,
    pub file_links: Option<Vec<Rc<dyn IXtermLink>>>,
    pub folder_links: Option<Vec<Rc<dyn IXtermLink>>>,
}

/// `Required<Pick<IDetectedLinks, 'wordLinks' | 'webLinks' | 'fileLinks' | 'folderLinks'>>`.
#[derive(Clone, Default)]
pub struct DetectedLinkLists {
    pub word_links: Vec<Rc<dyn IXtermLink>>,
    pub web_links: Vec<Rc<dyn IXtermLink>>,
    pub file_links: Vec<Rc<dyn IXtermLink>>,
    pub folder_links: Vec<Rc<dyn IXtermLink>>,
}

impl DetectedLinkLists {
    /// O laço de montagem de `getLinks`: acrescenta, invertidos, os links de uma linha.
    fn extend_reversed(&mut self, links: &IDetectedLinks) {
        let reversed = |source: &Option<Vec<Rc<dyn IXtermLink>>>, target: &mut Vec<Rc<dyn IXtermLink>>| {
            if let Some(source) = source {
                target.extend(source.iter().rev().cloned());
            }
        };
        reversed(&links.word_links, &mut self.word_links);
        reversed(&links.web_links, &mut self.web_links);
        reversed(&links.file_links, &mut self.file_links);
        reversed(&links.folder_links, &mut self.folder_links);
    }
}

/// O resultado de `getLinks`: `{ viewport, all }`.
#[derive(Clone)]
pub struct DetectedLinks {
    pub viewport: DetectedLinkLists,
    pub all: Promise<DetectedLinkLists>,
}

/// O `type` de `_getLinksForType`: `'word' | 'url' | 'localFile' | 'localFolder'`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LinkKind {
    Word,
    Url,
    LocalFile,
    LocalFolder,
}

/// O `type` de `openRecentLink`: `'localFile' | 'url'`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecentLinkKind {
    LocalFile,
    Url,
}

impl From<RecentLinkKind> for LinkKind {
    fn from(kind: RecentLinkKind) -> Self {
        match kind {
            RecentLinkKind::LocalFile => Self::LocalFile,
            RecentLinkKind::Url => Self::Url,
        }
    }
}

/// `Pick<ITerminalBackend, 'getWslPath'>` do `ITerminalBackend` do terminal, no contrato dos detectores.
pub struct BackendWslPath(pub Rc<dyn ITerminalBackend>);

impl IWslPathBackend for BackendWslPath {
    fn get_wsl_path(&self, original: &str, direction: WslPathDirection) -> Promise<String> {
        let direction = match direction {
            WslPathDirection::UnixToWin => BackendWslPathDirection::UnixToWin,
            WslPathDirection::WinToUnix => BackendWslPathDirection::WinToUnix,
        };
        ITerminalBackendWslPath::get_wsl_path(self.0.as_ref(), original, direction)
    }
}

/// Os serviços que o construtor do gerenciador lê (ver o cabeçalho do módulo).
#[derive(Clone)]
pub struct TerminalLinkManagerServices {
    pub configuration_service: Rc<dyn IConfigurationService>,
    pub notification_service: Rc<dyn INotificationService>,
    pub telemetry_service: Rc<dyn ITelemetryService>,
    pub terminal_configuration_service: Rc<dyn ITerminalConfigurationService>,
    pub log_service: LogServiceRef,
    pub tunnel_service: Rc<dyn ITunnelService>,
    pub hover_service: Rc<dyn ITerminalHoverService>,
    pub product_service: Rc<dyn je_platform::product::IProductService>,
    pub uri_identity_service: Rc<dyn IUriIdentityService>,
    pub workspace_context_service: Rc<dyn IWorkspaceContextService>,
}

/// `externalProvideLinksCb`: `OmitFirstArg<ITerminalExternalLinkProvider['provideLinks']>`.
pub type ExternalProvideLinks = Rc<dyn Fn(&str) -> Promise<Option<Vec<ITerminalLink>>>>;

type LinksForTypeOverride = Rc<dyn Fn(i32, LinkKind) -> Promise<Option<Vec<Rc<dyn IXtermLink>>>>>;
type ShowHoverOverride =
    Rc<dyn Fn(ILinkHoverTargetOptions, MarkdownString, Option<Vec<IHoverAction>>, Rc<dyn Fn(&str)>, Option<Rc<dyn IInvalidatableLink>>) -> Option<DisposableRef>>;

/// O hover ativo do `linkHandler` (`activeHoverDisposable`, `activeTooltipScheduler` e `activeHoverListeners`).
#[derive(Default)]
struct ActiveLinkHover {
    hover: RefCell<Option<DisposableRef>>,
    tooltip_scheduler: RefCell<Option<Rc<RunOnceScheduler>>>,
    hover_listeners: RefCell<Option<Rc<DisposableStore>>>,
}

impl ActiveLinkHover {
    /// `clearActiveLinkHover()`.
    fn clear(&self) {
        if let Some(hover) = self.hover.borrow_mut().take() {
            hover.dispose();
        }
        if let Some(scheduler) = self.tooltip_scheduler.borrow_mut().take() {
            scheduler.dispose();
        }
        if let Some(listeners) = self.hover_listeners.borrow_mut().take() {
            listeners.dispose();
        }
    }
}

/// `TerminalLinkManager`: an object responsible for managing registration of link matchers and link providers.
pub struct TerminalLinkManager {
    store: Rc<DisposableStore>,
    this: Weak<TerminalLinkManager>,
    xterm: Rc<dyn IXtermRawTerminal>,
    process_info: Rc<dyn ITerminalProcessInfo>,
    services: TerminalLinkManagerServices,
    widget_manager: RefCell<Option<Rc<dyn ITerminalWidgetManager>>>,
    standard_link_providers: RefCell<Vec<(String, Rc<TerminalLinkDetectorAdapter>)>>,
    link_providers_disposables: RefCell<Vec<DisposableRef>>,
    openers: RefCell<Vec<(TerminalLinkType, Rc<dyn ITerminalLinkOpener>)>>,
    link_hover_invalidation: Rc<MutableDisposable<DisposableStore>>,
    external_provide_links_cb: RefCell<Option<ExternalProvideLinks>>,
    links_for_type_override: RefCell<Option<LinksForTypeOverride>>,
    show_hover_override: RefCell<Option<ShowHoverOverride>>,
}

impl TerminalLinkManager {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        xterm: Rc<dyn IXtermRawTerminal>,
        process_info: Rc<dyn ITerminalProcessInfo>,
        capabilities: Rc<dyn ITerminalCapabilityStore>,
        link_resolver: Rc<dyn ITerminalLinkResolver>,
        openers: Vec<(TerminalBuiltinLinkType, Rc<dyn ITerminalLinkOpener>)>,
        services: TerminalLinkManagerServices,
    ) -> Rc<Self> {
        let store = DisposableStore::new();
        let link_hover_invalidation = MutableDisposable::<DisposableStore>::new();
        store.add(link_hover_invalidation.clone());

        let this = Rc::new_cyclic(|this| Self {
            store,
            this: this.clone(),
            xterm,
            process_info,
            services,
            widget_manager: RefCell::new(None),
            standard_link_providers: RefCell::new(Vec::new()),
            link_providers_disposables: RefCell::new(Vec::new()),
            openers: RefCell::new(openers.into_iter().map(|(link_type, opener)| (TerminalLinkType::Builtin(link_type), opener)).collect()),
            link_hover_invalidation,
            external_provide_links_cb: RefCell::new(None),
            links_for_type_override: RefCell::new(None),
            show_hover_override: RefCell::new(None),
        });

        let mut enable_file_links = true;
        let enable_file_links_config = this.services.configuration_service.get_value(Some(TERMINAL_CONFIG_SECTION), None).get_str("enableFileLinks");
        match &enable_file_links_config {
            Dynamic::Bool(false) => enable_file_links = false, // legacy from v1.75
            Dynamic::String(value) if *value == "off" => enable_file_links = false,
            Dynamic::String(value) if *value == "notRemote" => enable_file_links = this.process_info.remote_authority().is_none(),
            _ => {}
        }

        // Setup link detectors in their order of priority
        let process_manager = this.link_process_manager();
        let terminal: Rc<dyn ITerminal> = this.xterm.clone();
        if enable_file_links {
            this.setup_link_detector(
                TerminalMultiLineLinkDetector::ID,
                Rc::new(TerminalMultiLineLinkDetector::new(
                    terminal.clone(),
                    process_manager.clone(),
                    link_resolver.clone(),
                    this.services.log_service.clone(),
                    this.services.uri_identity_service.clone(),
                    this.services.workspace_context_service.clone(),
                )),
                false,
            );
            this.setup_link_detector(
                TerminalLocalLinkDetector::ID,
                Rc::new(TerminalLocalLinkDetector::new(
                    terminal.clone(),
                    capabilities,
                    process_manager.clone(),
                    link_resolver.clone(),
                    this.services.log_service.clone(),
                    this.services.uri_identity_service.clone(),
                    this.services.workspace_context_service.clone(),
                )),
                false,
            );
        }
        this.setup_link_detector(
            TerminalUriLinkDetector::ID,
            Rc::new(TerminalUriLinkDetector::new(
                terminal.clone(),
                process_manager,
                link_resolver,
                this.services.log_service.clone(),
                this.services.uri_identity_service.clone(),
                this.services.workspace_context_service.clone(),
            )),
            false,
        );
        let word_detector = TerminalWordLinkDetector::new(terminal, this.services.configuration_service.clone(), this.services.product_service.clone());
        this.store.add(word_detector.clone());
        this.setup_link_detector(TerminalWordLinkDetector::ID, word_detector, false);

        this.register_standard_link_providers();

        let active_hover = Rc::new(ActiveLinkHover::default());
        let clear_hover = active_hover.clone();
        this.store.add(plain_disposable({
            let this = this.this.clone();
            move || {
                if let Some(this) = this.upgrade() {
                    this.clear_link_providers();
                }
                clear_hover.clear();
            }
        }));
        this.xterm.set_link_handler(Some(this.create_link_handler(active_hover)));

        this
    }

    /// O retrato de `_processInfo` que os detectores recebem.
    fn link_process_manager(&self) -> TerminalLinkProcessManager {
        TerminalLinkProcessManager {
            initial_cwd: self.process_info.initial_cwd(),
            os: self.process_info.os(),
            remote_authority: self.process_info.remote_authority(),
            user_home: self.process_info.user_home(),
            backend: self.process_info.backend().map(|backend| Rc::new(BackendWslPath(backend)) as Rc<dyn IWslPathBackend>),
        }
    }

    /// A sobrescrita do `protected _getLinksForType` (a subclasse `TestLinkManager` do teste do original).
    pub fn set_links_for_type_override(&self, hook: Option<LinksForTypeOverride>) {
        *self.links_for_type_override.borrow_mut() = hook;
    }

    /// A atribuição de propriedade `linkManager._showHover = ...` do teste do original.
    pub fn set_show_hover_override(&self, hook: Option<ShowHoverOverride>) {
        *self.show_hover_override.borrow_mut() = hook;
    }

    /// `this._openers.get(type)`.
    fn opener_for(&self, link_type: &TerminalLinkType) -> Option<Rc<dyn ITerminalLinkOpener>> {
        self.openers.borrow().iter().find(|(opener_type, _)| opener_type == link_type).map(|(_, opener)| opener.clone())
    }

    fn setup_link_detector(&self, id: &str, detector: Rc<dyn ITerminalLinkDetector>, is_external: bool) -> Rc<TerminalLinkDetectorAdapter> {
        let detector_adapter = TerminalLinkDetectorAdapter::new(detector, self.xterm.clone(), self.services.configuration_service.clone());
        self.store.add(detector_adapter.clone());
        let this = self.this.clone();
        self.store.add(detector_adapter.on_did_activate_link().listen(move |event| {
            let Some(this) = this.upgrade() else { return };
            this.on_did_activate_link(event);
        }));
        let this = self.this.clone();
        self.store.add(detector_adapter.on_did_show_hover().listen(move |event| {
            let Some(this) = this.upgrade() else { return };
            this.tooltip_callback(event);
        }));
        if !is_external {
            self.standard_link_providers.borrow_mut().push((id.to_owned(), detector_adapter.clone()));
        }
        detector_adapter
    }

    fn on_did_activate_link(self: &Rc<Self>, event: &IActivateLinkEvent) {
        // Prevent default electron link handling so Alt+Click mode works normally
        if let Some(mouse_event) = &event.event {
            mouse_event.prevent_default();
            // Require correct modifier on click unless event is coming from linkQuickPick selection
            if !mouse_event.is_link_quick_pick && !self.is_link_activation_modifier_down(mouse_event) {
                return;
            }
        }
        // Just call the handler if there is no before listener
        if let Some(activate) = &event.link.activate {
            // Custom activate call (external links only)
            activate(&event.link.text);
        } else {
            let this = self.clone();
            let link = event.link.clone();
            let _ = async_fn(async move { this.open_link(&link).await });
        }
    }

    async fn open_link(&self, link: &ITerminalSimpleLink) -> Result<(), Error> {
        self.services.log_service.debug(&format!("Opening link {}", link.text));
        let opener = self.opener_for(&link.type_);
        let Some(opener) = opener else {
            return Err(Error::new(format!("No matching opener for link type \"{}\"", link_type_name(&link.type_))));
        };
        self.services.telemetry_service.public_log2(
            "terminal/openLink",
            Some(Dynamic::object([("linkType", Dynamic::string(&match &link.type_ {
                TerminalLinkType::Builtin(builtin) => builtin.as_str().to_owned(),
                TerminalLinkType::External(external) => format!("extension:{}", external.id),
            }))])),
        );
        opener.open(link).await
    }

}

impl ITerminalLinkManager for TerminalLinkManager {
    /// `this.add(disposable)` do `DisposableStore` que o gerenciador estende.
    fn add(&self, disposable: DisposableRef) -> DisposableRef {
        self.store.add(disposable)
    }

    /// `setWidgetManager(widgetManager)`.
    fn set_widget_manager(&self, widget_manager: Rc<dyn ITerminalWidgetManager>) {
        *self.widget_manager.borrow_mut() = Some(widget_manager);
    }

    /// `externalProvideLinksCb = cb`.
    fn set_external_provide_links_cb(&self, callback: Option<ExternalProvideLinks>) {
        *self.external_provide_links_cb.borrow_mut() = callback;
    }

    /// `openRecentLink(type)`.
    fn open_recent_link(&self, kind: RecentLinkKind) -> Promise<Option<Rc<dyn IXtermLink>>> {
        let this = self.this.upgrade().expect("o gerenciador está vivo");
        async_fn(async move {
            let mut links: Option<Vec<Rc<dyn IXtermLink>>> = None;
            let buffer = this.xterm.buffer_active();
            let mut i = buffer.length();
            while links.as_ref().is_none_or(|links| links.is_empty()) && i >= buffer.viewport_y() {
                links = this.get_links_for_type(i, kind.into()).await?;
                i -= 1;
            }

            let Some(links) = links.filter(|links| !links.is_empty()) else {
                return Ok(None);
            };
            let event = TerminalMouseEvent::link_quick_pick("click");
            links[0].activate(Some(&event), &links[0].text());
            Ok(Some(links[0].clone()))
        })
    }

    /// `getLinks()`.
    fn get_links(&self) -> Promise<DetectedLinks> {
        let this = self.this.upgrade().expect("o gerenciador está vivo");
        async_fn(async move {
            let buffer = this.xterm.buffer_active();
            let rows = this.xterm.rows();
            let viewport_y = buffer.viewport_y();

            // Fetch and await the viewport results
            let mut viewport_links_by_line_promises = Vec::new();
            for i in (viewport_y..viewport_y + rows).rev() {
                viewport_links_by_line_promises.push(this.get_links_for_line(i));
            }
            let mut viewport_links_by_line = Vec::new();
            for promise in viewport_links_by_line_promises {
                viewport_links_by_line.push(promise.await?);
            }

            // Assemble viewport links
            let mut viewport_links = DetectedLinkLists::default();
            for links in viewport_links_by_line.iter().flatten() {
                viewport_links.extend_reversed(links);
            }

            // Fetch the remaining results async
            let above_viewport_links_promises: Vec<_> = (0..viewport_y).rev().map(|i| this.get_links_for_line(i)).collect();
            let below_viewport_links_promises: Vec<_> = (viewport_y + rows..buffer.length()).rev().map(|i| this.get_links_for_line(i)).collect();

            // Assemble all links in results
            let (all, all_resolver) = Promise::<DetectedLinkLists>::new();
            let viewport_for_all = viewport_links.clone();
            let _ = async_fn(async move {
                let mut above_viewport_links = Vec::new();
                for promise in above_viewport_links_promises {
                    above_viewport_links.push(promise.await?);
                }
                let mut below_viewport_links = Vec::new();
                for promise in below_viewport_links_promises {
                    below_viewport_links.push(promise.await?);
                }
                let mut all_results = viewport_for_all;
                for links in below_viewport_links.iter().chain(above_viewport_links.iter()).flatten() {
                    all_results.extend_reversed(links);
                }
                all_resolver.resolve(all_results);
                Ok(())
            });

            Ok(DetectedLinks { viewport: viewport_links, all })
        })
    }

}

impl TerminalLinkManager {
    fn get_links_for_line(self: &Rc<Self>, y: i32) -> Promise<Option<IDetectedLinks>> {
        let this = self.clone();
        async_fn(async move {
            let unfiltered_word_links = this.get_links_for_type(y, LinkKind::Word).await?;
            let web_links = this.get_links_for_type(y, LinkKind::Url).await?;
            let file_links = this.get_links_for_type(y, LinkKind::LocalFile).await?;
            let folder_links = this.get_links_for_type(y, LinkKind::LocalFolder).await?;
            let mut words = std::collections::HashSet::new();
            let word_links = unfiltered_word_links.map(|unfiltered| {
                let mut word_links = Vec::new();
                for link in unfiltered {
                    let text = link.text();
                    if !words.contains(&text) && text.encode_utf16().count() > 1 {
                        words.insert(text);
                        word_links.push(link);
                    }
                }
                word_links
            });
            Ok(Some(IDetectedLinks { word_links, web_links, file_links, folder_links }))
        })
    }

    /// `_getLinksForType(y, type)`.
    fn get_links_for_type(self: &Rc<Self>, y: i32, kind: LinkKind) -> Promise<Option<Vec<Rc<dyn IXtermLink>>>> {
        if let Some(hook) = self.links_for_type_override.borrow().clone() {
            return hook(y, kind);
        }
        let this = self.clone();
        async_fn(async move {
            match kind {
                LinkKind::Word => this.provide_standard_links(TerminalWordLinkDetector::ID, y).await,
                LinkKind::Url => this.provide_standard_links(TerminalUriLinkDetector::ID, y).await,
                LinkKind::LocalFile => {
                    let links = this.provide_standard_links(TerminalLocalLinkDetector::ID, y).await?;
                    Ok(links.map(|links| links.into_iter().filter(|link| link.link_type() == Some(TerminalLinkType::Builtin(TerminalBuiltinLinkType::LocalFile))).collect()))
                }
                LinkKind::LocalFolder => {
                    let links = this.provide_standard_links(TerminalLocalLinkDetector::ID, y).await?;
                    Ok(links
                        .map(|links| links.into_iter().filter(|link| link.link_type() == Some(TerminalLinkType::Builtin(TerminalBuiltinLinkType::LocalFolderInWorkspace))).collect()))
                }
            }
        })
    }

    /// `new Promise<ILink[] | undefined>(r => this._standardLinkProviders.get(id)?.provideLinks(y, r))`.
    fn provide_standard_links(&self, id: &str, y: i32) -> Promise<Option<Vec<Rc<dyn IXtermLink>>>> {
        let provider = self.standard_link_providers.borrow().iter().find(|(provider_id, _)| provider_id == id).map(|(_, provider)| provider.clone());
        let (promise, resolver) = Promise::new();
        if let Some(provider) = provider {
            let _ = provider.provide_links(y, Box::new(move |links| resolver.resolve(links)));
        }
        promise
    }

    fn tooltip_callback(&self, event: &IShowHoverEvent) {
        if self.widget_manager.borrow().is_none() {
            return;
        }

        let cell = self.xterm.css_cell_dimensions();
        let cell_dimensions = Dimensions { width: cell.width, height: cell.height };
        let terminal_dimensions = Dimensions { width: f64::from(self.xterm.cols()), height: f64::from(self.xterm.rows()) };

        // Don't pass the mouse event as this avoids the modifier check
        let link = event.link.clone();
        self.show_hover(
            ILinkHoverTargetOptions {
                viewport_range: event.viewport_range,
                cell_dimensions,
                terminal_dimensions,
                modifier_down_callback: event.modifier_down_callback.clone(),
                modifier_up_callback: event.modifier_up_callback.clone(),
            },
            self.get_link_hover_string(&link.text, link.label.as_deref()),
            link.actions.clone(),
            {
                let link = link.clone();
                Rc::new(move |text: &str| link.activate(None, text))
            },
            Some(link),
        );
    }

    /// `_showHover(targetOptions, text, actions, linkHandler, link?)`.
    pub fn show_hover(
        &self,
        target_options: ILinkHoverTargetOptions,
        text: MarkdownString,
        actions: Option<Vec<IHoverAction>>,
        link_handler: Rc<dyn Fn(&str)>,
        link: Option<Rc<dyn IInvalidatableLink>>,
    ) -> Option<DisposableRef> {
        let hook = self.show_hover_override.borrow().clone();
        if let Some(hook) = hook {
            return hook(target_options, text, actions, link_handler, link);
        }
        let widget_manager = self.widget_manager.borrow().clone()?;
        let widget = TerminalHover::new(target_options, text, actions, link_handler, self.services.hover_service.clone(), self.services.configuration_service.clone());
        let attached = widget_manager.attach_widget(widget)?;
        let store = DisposableStore::new();
        store.add(attached);
        if let Some(link) = link {
            let weak_store = Rc::downgrade(&store);
            store.add(link.on_invalidated().listen(move |_| {
                if let Some(store) = weak_store.upgrade() {
                    store.dispose();
                }
            }));
        }
        self.link_hover_invalidation.set(Some(store.clone()));
        Some(store)
    }

    fn clear_link_providers(&self) {
        for disposable in self.link_providers_disposables.borrow_mut().drain(..) {
            disposable.dispose();
        }
    }

    fn register_standard_link_providers(self: &Rc<Self>) {
        // Forward any external link provider requests to the registered provider if it exists. This
        // helps maintain the relative priority of the link providers as it's defined by the order
        // in which they're registered in xterm.js.
        //
        // There's a bit going on here but here's another view:
        // - `externalProvideLinksCb`: The external callback that gives the links (eg. from exthost)
        // - `proxy_link_provider`: A proxy that forwards the call over to `externalProvideLinksCb`
        // - `wrapped_link_provider`: Wraps the above in an `TerminalLinkDetectorAdapter`
        let this = self.clone();
        let proxy_link_provider: ExternalProvideLinks = Rc::new(move |buffer_line_text: &str| {
            let callback = this.external_provide_links_cb.borrow().clone();
            match callback {
                Some(callback) => callback(buffer_line_text),
                None => Promise::resolved(None),
            }
        });
        // `extension-${this._externalLinkProviders.length}`: a lista de provedores externos nunca recebe elementos.
        let detector_id = "extension-0";
        let external_detector = TerminalExternalLinkDetector::new(detector_id, self.xterm.clone(), proxy_link_provider);
        let wrapped_link_provider = self.setup_link_detector(detector_id, Rc::new(external_detector), true);
        let registration = self.xterm.register_link_provider(wrapped_link_provider);
        self.link_providers_disposables.borrow_mut().push(registration);

        let providers: Vec<Rc<TerminalLinkDetectorAdapter>> = self.standard_link_providers.borrow().iter().map(|(_, provider)| provider.clone()).collect();
        for provider in providers {
            let registration = self.xterm.register_link_provider(provider as Rc<dyn ILinkProvider>);
            self.link_providers_disposables.borrow_mut().push(registration);
        }
    }

    fn is_link_activation_modifier_down(&self, event: &TerminalMouseEvent) -> bool {
        let editor_conf = self.services.configuration_service.get_value(Some("editor"), None);
        if editor_conf.get_str("multiCursorModifier").strict_equals(&Dynamic::string("ctrlCmd")) {
            return event.alt_key;
        }
        if Platform::detect().is_macintosh { event.meta_key } else { event.ctrl_key }
    }

    fn get_link_hover_string(&self, uri: &str, label: Option<&str>) -> MarkdownString {
        let editor_conf = self.services.configuration_service.get_value(Some("editor"), None);
        let is_macintosh = Platform::detect().is_macintosh;

        let click_label = if editor_conf.get_str("multiCursorModifier").strict_equals(&Dynamic::string("ctrlCmd")) {
            if is_macintosh {
                localize("terminalLinkHandler.followLinkAlt.mac", "option + click", &[])
            } else {
                localize("terminalLinkHandler.followLinkAlt", "alt + click", &[])
            }
        } else if is_macintosh {
            localize("terminalLinkHandler.followLinkCmd", "cmd + click", &[])
        } else {
            localize("terminalLinkHandler.followLinkCtrl", "ctrl + click", &[])
        };

        let mut fallback_label = localize("followLink", "Follow link", &[]);
        // `try { ... } catch { }`: o `URI.parse` do porte não lança.
        if self.services.tunnel_service.can_tunnel(&URI::parse(uri, false)) {
            fallback_label = localize("followForwardedLink", "Follow link using forwarded port", &[]);
        }

        let mut markdown = MarkdownString::new(None, TrustedOrOptions::Trusted(true));
        // Escapes markdown in label & uri
        let mut label = label.filter(|label| !label.is_empty()).map(|label| {
            markdown.append_text(&utf16(label), MarkdownStringTextNewlineStyle::Paragraph);
            let escaped = markdown.value.to_string_lossy();
            markdown.value = JsString::new();
            escaped
        });
        let mut uri = if uri.is_empty() {
            String::new()
        } else {
            markdown.append_text(&utf16(uri), MarkdownStringTextNewlineStyle::Paragraph);
            let escaped = markdown.value.to_string_lossy();
            markdown.value = JsString::new();
            escaped
        };

        let label = label.take().filter(|label| !label.is_empty()).unwrap_or(fallback_label);
        // Use the label when uri is '' so the link displays correctly
        if uri.is_empty() {
            uri = label.clone();
        }
        // Although if there is a space in the uri, just replace it completely
        let whitespace = RegExp::from_str(r"(\s|&nbsp;)", "").expect("a expressão de espaço é válida");
        if whitespace.exec(&utf16(&uri), 0).is_some() {
            uri = localize("followLinkUrl", "Link", &[]);
        }

        markdown.append_link(UriOrOwnedString::Str(&utf16(&uri)), &utf16(&label), None);
        markdown.append_markdown(&utf16(&format!(" ({click_label})")));
        markdown
    }

    /// O `options.linkHandler` do xterm: ativar (com o aviso de esquema), `hover` e `leave`.
    fn create_link_handler(self: &Rc<Self>, active_hover: Rc<ActiveLinkHover>) -> LinkHandler {
        let activate_this = self.clone();
        let hover_this = self.clone();
        let hover_state = active_hover.clone();
        let leave_state = active_hover;
        LinkHandler {
            allow_non_http_protocols: true,
            activate: Rc::new(move |event: &TerminalMouseEvent, text: &str, _range: &IBufferRange| {
                let this = activate_this.clone();
                let event = event.clone();
                let text = text.to_owned();
                async_fn(async move { this.activate_osc8_link(&event, &text).await })
            }),
            hover: Some(Rc::new(move |event: &TerminalMouseEvent, text: &str, range: &IBufferRange| {
                hover_state.clear();
                let this = hover_this.clone();
                let (text, range, event) = (text.to_owned(), *range, event.clone());
                let delay = match hover_this.services.configuration_service.get_value(Some("workbench.hover.delay"), None) {
                    Dynamic::Number(delay) if delay > 0.0 => delay as u64,
                    _ => 0,
                };
                let state = hover_state.clone();
                let scheduler = RunOnceScheduler::new(
                    move || {
                        let cell = this.xterm.css_cell_dimensions();
                        let cell_dimensions = Dimensions { width: cell.width, height: cell.height };
                        let terminal_dimensions = Dimensions { width: f64::from(this.xterm.cols()), height: f64::from(this.xterm.rows()) };
                        let hover_viewport_y = this.xterm.buffer_active().viewport_y();
                        let activate_this = this.clone();
                        let (activate_event, activate_range) = (event.clone(), range);
                        let hover = this.show_hover(
                            ILinkHoverTargetOptions {
                                viewport_range: convert_buffer_range_to_viewport(&range, hover_viewport_y),
                                cell_dimensions,
                                terminal_dimensions,
                                modifier_down_callback: None,
                                modifier_up_callback: None,
                            },
                            this.get_link_hover_string(&text, Some(&text)),
                            None,
                            Rc::new(move |text: &str| {
                                if let Some(handler) = activate_this.xterm.link_handler() {
                                    let _ = (handler.activate)(&activate_event, text, &activate_range);
                                }
                            }),
                            None,
                        );
                        *state.hover.borrow_mut() = hover;
                        let listeners = DisposableStore::new();
                        let scroll_state = state.clone();
                        listeners.add(this.xterm.on_scroll().listen(move |_| scroll_state.clear()));
                        let render_state = state.clone();
                        listeners.add(this.xterm.on_render().listen(move |rendered_range| {
                            // Convert buffer range to viewport range and check if the
                            // rendered range intersects any row of the link
                            let viewport_range = convert_buffer_range_to_viewport(&range, hover_viewport_y);
                            if viewport_range.start.y <= rendered_range.end && viewport_range.end.y >= rendered_range.start {
                                render_state.clear();
                            }
                        }));
                        *state.hover_listeners.borrow_mut() = Some(listeners);
                        // Clear out scheduler until next hover event
                        if let Some(scheduler) = state.tooltip_scheduler.borrow_mut().take() {
                            scheduler.dispose();
                        }
                    },
                    delay,
                );
                *hover_state.tooltip_scheduler.borrow_mut() = Some(scheduler.clone());
                scheduler.schedule(None);
            })),
            leave: Some(Rc::new(move |_event: &TerminalMouseEvent, _text: &str, _range: &IBufferRange| {
                leave_state.clear();
            })),
        }
    }

    /// O `activate` do `linkHandler`: confere o modificador e o esquema permitido, e abre pelo abridor de URL.
    async fn activate_osc8_link(&self, event: &TerminalMouseEvent, text: &str) -> Result<(), Error> {
        if !self.is_link_activation_modifier_down(event) {
            return Ok(());
        }
        let Some(colon_index) = text.find(':') else {
            return Err(Error::new(format!("Could not find scheme in link \"{text}\"")));
        };
        let scheme = text[..colon_index].to_owned();
        let allowed_link_schemes = self.services.terminal_configuration_service.config().get_str("allowedLinkSchemes");
        let allowed: Vec<String> = match &allowed_link_schemes {
            Dynamic::Array(schemes) => schemes.to_vec().iter().map(|scheme| je_platform::js::to_js_string(scheme).to_string_lossy()).collect(),
            _ => Vec::new(),
        };
        if !allowed.contains(&scheme) {
            let (prompt, resolver) = Promise::<bool>::new();
            let allow_resolver = resolver.clone();
            let cancel_resolver = resolver;
            let configuration_service = self.services.configuration_service.clone();
            let scheme_to_allow = scheme.clone();
            let allowed_for_update = allowed.clone();
            self.services.notification_service.prompt(
                Severity::Warning,
                NotificationMessage::Text(localize("scheme", "Opening URIs can be insecure, do you want to allow opening links with the scheme {0}?", &[&scheme])),
                vec![PromptChoiceKind::Plain(PromptChoice {
                    base: BasePromptChoice {
                        telemetry_id: None,
                        label: localize("allow", "Allow {0}", &[&scheme]),
                        keep_open: false,
                        run: Rc::new(move || {
                            let mut allowed_link_schemes: Vec<Dynamic> = allowed_for_update.iter().map(|scheme| Dynamic::string(scheme)).collect();
                            allowed_link_schemes.push(Dynamic::string(&scheme_to_allow));
                            let _ = configuration_service.update_value("terminal.integrated.allowedLinkSchemes", Dynamic::array(allowed_link_schemes), None, None);
                            allow_resolver.resolve(true);
                        }),
                    },
                    is_secondary: false,
                })],
                Some(PromptOptions { on_cancel: Some(Rc::new(move || cancel_resolver.resolve(false))), ..PromptOptions::default() }),
            );
            if !prompt.await? {
                return Ok(());
            }
        }
        let opener = self.opener_for(&TerminalLinkType::Builtin(TerminalBuiltinLinkType::Url));
        if let Some(opener) = opener {
            let mut link = ITerminalSimpleLink::new(
                text,
                IBufferRange { start: IBufferCellPosition { x: 0, y: 0 }, end: IBufferCellPosition { x: 0, y: 0 } },
                TerminalBuiltinLinkType::Url,
            );
            link.uri = Some(URI::parse(text, false));
            let _ = opener.open(&link);
        }
        Ok(())
    }
}

/// O texto do `type` de um link (`"${link.type}"` da mensagem de erro).
fn link_type_name(link_type: &TerminalLinkType) -> String {
    match link_type {
        TerminalLinkType::Builtin(builtin) => builtin.as_str().to_owned(),
        TerminalLinkType::External(_) => "[object Object]".to_owned(),
    }
}

fn utf16(text: &str) -> Vec<u16> {
    text.encode_utf16().collect()
}

impl IDisposable for TerminalLinkManager {
    fn dispose(&self) {
        self.store.dispose();
    }
}

/// O que a `TerminalLinkContribution` usa do gerenciador (o teste dela troca o gerenciador por `stubInstance`).
pub trait ITerminalLinkManager: IDisposable {
    /// `add(disposable)`.
    fn add(&self, disposable: DisposableRef) -> DisposableRef;
    /// `setWidgetManager(widgetManager)`.
    fn set_widget_manager(&self, widget_manager: Rc<dyn ITerminalWidgetManager>);
    /// `externalProvideLinksCb = cb`.
    fn set_external_provide_links_cb(&self, callback: Option<ExternalProvideLinks>);
    /// `getLinks()`.
    fn get_links(&self) -> Promise<DetectedLinks>;
    /// `openRecentLink(type)`.
    fn open_recent_link(&self, kind: RecentLinkKind) -> Promise<Option<Rc<dyn IXtermLink>>>;
}
