//! Porte de `src/vs/workbench/contrib/terminalContrib/links/browser/terminalLinkDetectorAdapter.ts`: embrulha um detector de
//! links para o xterm usar (`ILinkProvider`), criando os [`TerminalLink`] de cada linha e repassando a ativação e o hover.
//!
//! Decisões de tradução:
//! - **`_detector.xterm` (um `Terminal`)** é o [`ITerminalLinkDetector::xterm`] para a leitura do buffer e o
//!   [`IXtermRawTerminal`] passado ao construtor para criar os `TerminalLink`. Os dois são o mesmo emulador.
//! - **`@IInstantiationService` / `createInstance(TerminalLink, ...)`** é [`TerminalLink::new`], com o
//!   `IConfigurationService` guardado pelo adaptador.
//! - **`Map<number, Promise<TerminalLink[]>>`** é um `HashMap` de promessas de `Vec<Rc<TerminalLink>>`.
//! - **`this._store.isDisposed`** é o campo `disposed` (o `Disposable` do porte não expõe o estado do armazém).
//! - O trecho que remove o `:` final do texto altera o link recebido do detector (como o original, que muda o objeto): o
//!   evento de ativação carrega o link já alterado.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::{Rc, Weak};

use je_base::async_::async_fn;
use je_base::event::{Emitter, Event};
use je_base::lifecycle::{Disposable, DisposableStore, IDisposable};
use je_base::nls::localize;
use je_base::promise::Promise;
use je_platform::configuration::service::IConfigurationService;

use super::links::{ITerminalLinkDetector, ITerminalSimpleLink, TerminalBuiltinLinkType, TerminalLinkType};
use super::terminal_link::{LinkActivateCallback, LinkTooltipCallback, TerminalLink};
use crate::contrib::terminal::browser::terminal_extensions::{ILinkProvider, IXtermLink, IXtermRawTerminal};
use crate::contrib::terminal::browser::terminal_input_events::TerminalMouseEvent;
use crate::contrib::terminal::xterm_buffer::{IBufferLine, IViewportRange};

/// `IActivateLinkEvent`.
#[derive(Clone)]
pub struct IActivateLinkEvent {
    pub link: ITerminalSimpleLink,
    pub event: Option<TerminalMouseEvent>,
}

/// `IShowHoverEvent`.
#[derive(Clone)]
pub struct IShowHoverEvent {
    pub link: Rc<TerminalLink>,
    pub viewport_range: IViewportRange,
    pub modifier_down_callback: Option<Rc<dyn Fn()>>,
    pub modifier_up_callback: Option<Rc<dyn Fn()>>,
}

/// `TerminalLinkDetectorAdapter`.
pub struct TerminalLinkDetectorAdapter {
    base: Rc<Disposable>,
    this: Weak<TerminalLinkDetectorAdapter>,
    detector: Rc<dyn ITerminalLinkDetector>,
    xterm: Rc<dyn IXtermRawTerminal>,
    configuration_service: Rc<dyn IConfigurationService>,
    disposed: Cell<bool>,
    active_links_store: Rc<DisposableStore>,
    on_did_activate_link: Rc<Emitter<IActivateLinkEvent>>,
    on_did_show_hover: Rc<Emitter<IShowHoverEvent>>,
    active_provide_link_requests: RefCell<HashMap<i32, Promise<Vec<Rc<TerminalLink>>>>>,
}

impl TerminalLinkDetectorAdapter {
    pub fn new(detector: Rc<dyn ITerminalLinkDetector>, xterm: Rc<dyn IXtermRawTerminal>, configuration_service: Rc<dyn IConfigurationService>) -> Rc<Self> {
        let base = Disposable::new();
        let active_links_store = DisposableStore::new();
        let on_did_activate_link = Emitter::new();
        let on_did_show_hover = Emitter::new();
        base.register(active_links_store.clone());
        base.register(on_did_activate_link.clone());
        base.register(on_did_show_hover.clone());
        Rc::new_cyclic(|this| Self {
            base,
            this: this.clone(),
            detector,
            xterm,
            configuration_service,
            disposed: Cell::new(false),
            active_links_store,
            on_did_activate_link,
            on_did_show_hover,
            active_provide_link_requests: RefCell::new(HashMap::new()),
        })
    }

    /// `onDidActivateLink`.
    pub fn on_did_activate_link(&self) -> Event<IActivateLinkEvent> {
        self.on_did_activate_link.event()
    }

    /// `onDidShowHover`.
    pub fn on_did_show_hover(&self) -> Event<IShowHoverEvent> {
        self.on_did_show_hover.event()
    }

    fn provide_links_for_line(this: Rc<Self>, buffer_line_number: i32) -> Promise<Vec<Rc<TerminalLink>>> {
        async_fn(async move {
            // Dispose of all old links if new links are provided, links are only cached for the current line
            let mut links: Vec<Rc<TerminalLink>> = Vec::new();

            let mut start_line = buffer_line_number - 1;
            let mut end_line = start_line;

            let detector_xterm = this.detector.xterm();
            let buffer = detector_xterm.buffer_active();
            let cols = detector_xterm.cols();
            let mut lines: Vec<Rc<dyn IBufferLine>> = vec![buffer.get_line(start_line).expect("a linha do link existe")];

            // Cap the maximum context on either side of the line being provided, by taking the context
            // around the line being provided for this ensures the line the pointer is on will have
            // links provided.
            let max_character_context = this.detector.max_link_length().max(cols);
            let max_line_context = (f64::from(max_character_context) / f64::from(cols)).ceil() as i32;
            let min_start_line = (start_line - max_line_context).max(0);
            let max_end_line = (end_line + max_line_context).min(buffer.length());

            while start_line >= min_start_line && buffer.get_line(start_line).is_some_and(|line| line.is_wrapped()) {
                lines.insert(0, buffer.get_line(start_line - 1).expect("a linha anterior existe"));
                start_line -= 1;
            }

            while end_line < max_end_line && buffer.get_line(end_line + 1).is_some_and(|line| line.is_wrapped()) {
                lines.push(buffer.get_line(end_line + 1).expect("a próxima linha existe"));
                end_line += 1;
            }

            let detected_links = this.detector.detect(&lines, start_line, end_line).await?;
            if this.disposed.get() {
                return Ok(Vec::new());
            }
            for link in detected_links {
                let activated = link.clone();
                let adapter = this.this.clone();
                let terminal_link = this.create_terminal_link(
                    link,
                    Rc::new(move |event: Option<&TerminalMouseEvent>, _text: &str| {
                        if let Some(adapter) = adapter.upgrade() {
                            adapter.on_did_activate_link.fire(IActivateLinkEvent { link: activated.clone(), event: event.cloned() });
                        }
                        Promise::resolved(())
                    }),
                );
                this.active_links_store.add_rc(terminal_link.clone());
                links.push(terminal_link);
            }

            Ok(links)
        })
    }

    fn create_terminal_link(&self, mut l: ITerminalSimpleLink, activate_callback: LinkActivateCallback) -> Rc<TerminalLink> {
        // Remove trailing colon if there is one so the link is more useful
        if l.disable_trim_colon != Some(true) && l.text.ends_with(':') {
            l.text.pop();
            l.buffer_range.end.x -= 1;
        }
        let adapter = self.this.clone();
        let tooltip_callback: LinkTooltipCallback = Rc::new(move |link, viewport_range, modifier_down_callback, modifier_up_callback| {
            if let Some(adapter) = adapter.upgrade() {
                adapter.on_did_show_hover.fire(IShowHoverEvent { link, viewport_range, modifier_down_callback, modifier_up_callback });
            }
        });
        let label = l.label.clone().filter(|label| !label.is_empty()).or_else(|| Some(Self::get_label(&l.type_)));
        TerminalLink::new(
            self.xterm.clone(),
            l.buffer_range,
            l.text.clone(),
            l.uri.clone(),
            l.parsed_link.clone(),
            l.actions.clone(),
            self.detector.xterm().buffer_active().viewport_y(),
            activate_callback,
            tooltip_callback,
            l.type_ != TerminalLinkType::Builtin(TerminalBuiltinLinkType::Search), // Only search is low confidence
            label,
            l.type_.clone(),
            self.configuration_service.clone(),
        )
    }

    fn get_label(type_: &TerminalLinkType) -> String {
        match type_ {
            TerminalLinkType::Builtin(TerminalBuiltinLinkType::Search) => localize("searchWorkspace", "Search workspace", &[]),
            TerminalLinkType::Builtin(TerminalBuiltinLinkType::LocalFile) => localize("openFile", "Open file in editor", &[]),
            TerminalLinkType::Builtin(TerminalBuiltinLinkType::LocalFolderInWorkspace) => localize("focusFolder", "Focus folder in explorer", &[]),
            TerminalLinkType::Builtin(TerminalBuiltinLinkType::LocalFolderOutsideWorkspace) => localize("openFolder", "Open folder in new window", &[]),
            TerminalLinkType::Builtin(TerminalBuiltinLinkType::Url) | TerminalLinkType::External(_) => localize("followLink", "Follow link", &[]),
        }
    }
}

impl IDisposable for TerminalLinkDetectorAdapter {
    fn dispose(&self) {
        self.disposed.set(true);
        self.base.dispose();
    }
}

fn as_xterm_links(links: &[Rc<TerminalLink>]) -> Vec<Rc<dyn IXtermLink>> {
    links.iter().map(|link| link.clone() as Rc<dyn IXtermLink>).collect()
}

impl ILinkProvider for TerminalLinkDetectorAdapter {
    fn provide_links(&self, buffer_line_number: i32, callback: Box<dyn FnOnce(Option<Vec<Rc<dyn IXtermLink>>>)>) -> Promise<()> {
        let this = self.this.upgrade().expect("o adaptador está vivo");
        async_fn(async move {
            let active_request = this.active_provide_link_requests.borrow().get(&buffer_line_number).cloned();
            if let Some(active_request) = active_request {
                let links = active_request.await?;
                callback(Some(as_xterm_links(&links)));
                return Ok(());
            }
            this.active_links_store.clear();
            let active_request = Self::provide_links_for_line(this.clone(), buffer_line_number);
            this.active_provide_link_requests.borrow_mut().insert(buffer_line_number, active_request.clone());
            let links = active_request.await?;
            this.active_provide_link_requests.borrow_mut().remove(&buffer_line_number);
            callback(Some(as_xterm_links(&links)));
            Ok(())
        })
    }
}
