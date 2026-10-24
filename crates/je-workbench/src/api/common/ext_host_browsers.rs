//! Porte de `src/vs/workbench/api/common/extHostBrowsers.ts`: o lado da extensão da API de abas do
//! navegador integrado (`vscode.window.browserTabs`, `openBrowserTab` e as sessões CDP).
//!
//! Divergências:
//! - Os objetos `value` que o original monta (um literal com getters, para esconder o estado interno da
//!   extensão) não existem: [`BrowserTab`] e [`BrowserCdpSession`] são o próprio `Rc` de
//!   [`ExtHostBrowserTab`] e [`ExtHostBrowserCdpSession`], e o que o original chama de interno (`update`,
//!   `_acceptMessage`, `_acceptClosed`, os construtores) é privado ao módulo, o que dá a mesma garantia.
//!   A identidade do objeto, que o original mantém estável entre atualizações, é a do `Rc` ([`Rc::ptr_eq`]).
//!   Também por isso a aba não tem como ser escrita de fora (no original, `tab.url = ...` lança): não há
//!   setter, e tentar é erro de compilação.
//! - `vscode.BrowserTab`, `BrowserCDPSession` e `BrowserTabShowOptions` (de `vscode.proposed.browser.d.ts`)
//!   são o que os `Rc` acima expõem e a estrutura [`BrowserTabShowOptions`]. Como a API devolve
//!   `Thenable<T>`, os métodos `async` devolvem [`Promise`].
//! - `sendMessage(message: unknown)` recebe um [`Dynamic`], porque a validação do original (`typeof`) é sobre
//!   um valor qualquer que a extensão mandou; o mesmo vale para o `unknown` de `onDidReceiveMessage`, que
//!   entrega [`CdpMessage::to_dynamic`].
//! - A `Map` de abas do original é um `Vec` de pares, para manter a ordem de inserção.
//! - `extHostTypes.ThemeIcon` é o [`ThemeIcon`] do `je-base`, e `Codicon.globe.id` é o id `"globe"`.
//! - `ExtHostBrowsers extends Disposable` é um campo [`Disposable`], e o `$` dos métodos de RPC é `rpc_`
//!   (ver `ext_host_protocol`).

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use je_base::async_::async_fn;
use je_base::dynamic::Dynamic;
use je_base::errors::Error;
use je_base::event::{Emitter, Event};
use je_base::lifecycle::{Disposable, DisposableMap, IDisposable};
use je_base::promise::Promise;
use je_base::themables::ThemeIcon;
use je_base::uri::URI;
use je_base::uuid::generate_uuid;
use je_platform::opener::editor_types::EditorOptions;

use super::ext_host_protocol::{BrowserTabDto, CdpMessage, CdpRequest, ExtHostBrowsersShape, IMainContext, MainContext, MainThreadBrowsersShape};
use super::ext_host_type_converters::ViewColumn;
use super::ext_host_types::IconPath;

/// `Codicon.globe.id`.
const GLOBE_CODICON_ID: &str = "globe";

/// `vscode.BrowserTabShowOptions`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct BrowserTabShowOptions {
    /// A coluna onde mostrar o navegador (`vscode.ViewColumn`).
    pub view_column: Option<f64>,
    /// Quando `true`, a aba não toma o foco.
    pub preserve_focus: Option<bool>,
    /// Quando `true`, a aba abre em segundo plano.
    pub background: Option<bool>,
}

/// `vscode.BrowserTab`.
pub type BrowserTab = Rc<ExtHostBrowserTab>;

/// `vscode.BrowserCDPSession`.
pub type BrowserCdpSession = Rc<ExtHostBrowserCdpSession>;

// #region Internal browser tab object

struct BrowserTabState {
    url: String,
    title: String,
    favicon: Option<String>,
}

/// Troca `slot` por `value` e diz se mudou.
fn assign_if_changed<T: PartialEq + Clone>(slot: &mut T, value: &T) -> bool {
    let changed = slot != value;
    if changed {
        slot.clone_from(value);
    }
    changed
}

pub struct ExtHostBrowserTab {
    id: String,
    proxy: Rc<dyn MainThreadBrowsersShape>,
    sessions: Rc<DisposableMap<String, ExtHostBrowserCdpSession>>,
    state: RefCell<BrowserTabState>,
}

impl ExtHostBrowserTab {
    fn new(id: String, proxy: Rc<dyn MainThreadBrowsersShape>, sessions: Rc<DisposableMap<String, ExtHostBrowserCdpSession>>, data: &BrowserTabDto) -> Rc<Self> {
        Rc::new(Self {
            id,
            proxy,
            sessions,
            state: RefCell::new(BrowserTabState { url: data.url.clone(), title: data.title.clone(), favicon: data.favicon.clone() }),
        })
    }

    /// A URL atual da página.
    pub fn url(&self) -> String {
        self.state.borrow().url.clone()
    }

    /// O título atual da página.
    pub fn title(&self) -> String {
        self.state.borrow().title.clone()
    }

    /// O ícone da página (o favicon ou o globo padrão).
    pub fn icon(&self) -> IconPath {
        match self.state.borrow().favicon.as_deref().filter(|favicon| !favicon.is_empty()) {
            Some(favicon) => IconPath::Uri(URI::parse(favicon, false)),
            None => IconPath::ThemeIcon(ThemeIcon::from_id(GLOBE_CODICON_ID)),
        }
    }

    /// Cria uma sessão CDP que expõe esta aba.
    pub fn start_cdp_session(self: &Rc<Self>) -> Promise<BrowserCdpSession> {
        let this = self.clone();
        async_fn(async move {
            let session_id = generate_uuid();
            this.proxy.rpc_start_cdp_session(&session_id, &this.id).await?;
            let session = ExtHostBrowserCdpSession::new(session_id.clone(), this.proxy.clone());
            this.sessions.set(session_id, session.clone(), false);
            Ok(session)
        })
    }

    /// Fecha esta aba.
    pub fn close(self: &Rc<Self>) -> Promise<()> {
        let this = self.clone();
        async_fn(async move { this.proxy.rpc_close_browser_tab(&this.id).await })
    }

    fn update(&self, data: &BrowserTabDto) -> bool {
        let mut state = self.state.borrow_mut();
        // sem `||`: os três campos são atualizados mesmo que o primeiro já tenha mudado
        let url_changed = assign_if_changed(&mut state.url, &data.url);
        let title_changed = assign_if_changed(&mut state.title, &data.title);
        let favicon_changed = assign_if_changed(&mut state.favicon, &data.favicon);
        url_changed || title_changed || favicon_changed
    }
}

// #endregion

// #region CDP Session

/// Valida a mensagem que a extensão mandou e monta o [`CdpRequest`].
fn parse_cdp_request(message: &Dynamic) -> Result<CdpRequest, Error> {
    if message.is_null() || message.type_of() != "object" {
        return Err(Error::new("Message must be an object"));
    }
    let Dynamic::Number(id) = message.get_str("id") else {
        return Err(Error::new("Message must have a numeric id"));
    };
    let Dynamic::String(method) = message.get_str("method") else {
        return Err(Error::new("Message must have a method string"));
    };
    let params = message.get_str("params");
    if !params.is_undefined() && params.type_of() != "object" {
        return Err(Error::new("Message params must be an object"));
    }
    let session_id = match message.get_str("sessionId") {
        Dynamic::Undefined => None,
        Dynamic::String(session_id) => Some(session_id.to_string_lossy()),
        _ => return Err(Error::new("Message sessionId must be a string")),
    };
    Ok(CdpRequest { id, method: method.to_string_lossy(), params: (!params.is_undefined()).then_some(params), session_id })
}

pub struct ExtHostBrowserCdpSession {
    id: String,
    proxy: Rc<dyn MainThreadBrowsersShape>,
    message_emitter: Rc<Emitter<Dynamic>>,
    close_emitter: Rc<Emitter<()>>,
    closed: Cell<bool>,
}

impl ExtHostBrowserCdpSession {
    fn new(id: String, proxy: Rc<dyn MainThreadBrowsersShape>) -> Rc<Self> {
        Rc::new(Self { id, proxy, message_emitter: Emitter::new(), close_emitter: Emitter::new(), closed: Cell::new(false) })
    }

    /// Dispara quando chega uma mensagem CDP de um alvo anexado.
    pub fn on_did_receive_message(&self) -> Event<Dynamic> {
        self.message_emitter.event()
    }

    /// Dispara quando a sessão é fechada.
    pub fn on_did_close(&self) -> Event<()> {
        self.close_emitter.event()
    }

    /// Manda uma mensagem de requisição CDP a um alvo anexado.
    pub fn send_message(self: &Rc<Self>, message: &Dynamic) -> Promise<()> {
        let this = self.clone();
        let message = message.clone();
        async_fn(async move {
            if this.closed.get() {
                return Err(Error::new("Session is closed"));
            }
            let request = parse_cdp_request(&message)?;
            this.proxy.rpc_send_cdp_message(&this.id, request).await
        })
    }

    /// Fecha a sessão e desanexa todos os alvos.
    pub fn close(self: &Rc<Self>) -> Promise<()> {
        let this = self.clone();
        async_fn(async move {
            this.closed.set(true);
            this.proxy.rpc_close_cdp_session(&this.id).await
        })
    }

    // Chamado pela thread principal
    fn accept_message(&self, message: &CdpMessage) {
        self.message_emitter.fire(message.to_dynamic());
    }

    fn accept_closed(&self) {
        self.closed.set(true);
        self.close_emitter.fire(());
    }
}

impl IDisposable for ExtHostBrowserCdpSession {
    fn dispose(&self) {
        self.message_emitter.dispose();
        self.close_emitter.dispose();
    }
}

// #endregion

/// Um [`Emitter`] registrado em `base`, para ser descartado junto.
fn register_emitter<T: 'static>(base: &Disposable) -> Rc<Emitter<T>> {
    let emitter: Rc<Emitter<T>> = Emitter::new();
    base.register(emitter.clone());
    emitter
}

pub struct ExtHostBrowsers {
    base: Rc<Disposable>,
    proxy: Rc<dyn MainThreadBrowsersShape>,
    browser_tabs: RefCell<Vec<(String, BrowserTab)>>,
    sessions: Rc<DisposableMap<String, ExtHostBrowserCdpSession>>,
    active_browser_tab_id: RefCell<Option<String>>,
    open_emitter: Rc<Emitter<BrowserTab>>,
    close_emitter: Rc<Emitter<BrowserTab>>,
    change_active_emitter: Rc<Emitter<Option<BrowserTab>>>,
    change_state_emitter: Rc<Emitter<BrowserTab>>,
}

impl ExtHostBrowsers {
    pub fn new(main_context: &dyn IMainContext) -> Rc<Self> {
        let base = Disposable::new();
        let sessions: Rc<DisposableMap<String, ExtHostBrowserCdpSession>> = DisposableMap::new();
        base.register(sessions.clone());
        Rc::new(Self {
            proxy: main_context.get_proxy(&MainContext::MAIN_THREAD_BROWSERS),
            browser_tabs: RefCell::new(Vec::new()),
            sessions,
            active_browser_tab_id: RefCell::new(None),
            open_emitter: register_emitter(&base),
            close_emitter: register_emitter(&base),
            change_active_emitter: register_emitter(&base),
            change_state_emitter: register_emitter(&base),
            base,
        })
    }

    // #region Public API (called from extension code)

    /// `onDidOpenBrowserTab`.
    pub fn on_did_open_browser_tab(&self) -> Event<BrowserTab> {
        self.open_emitter.event()
    }

    /// `onDidCloseBrowserTab`.
    pub fn on_did_close_browser_tab(&self) -> Event<BrowserTab> {
        self.close_emitter.event()
    }

    /// `onDidChangeActiveBrowserTab`.
    pub fn on_did_change_active_browser_tab(&self) -> Event<Option<BrowserTab>> {
        self.change_active_emitter.event()
    }

    /// `onDidChangeBrowserTabState`.
    pub fn on_did_change_browser_tab_state(&self) -> Event<BrowserTab> {
        self.change_state_emitter.event()
    }

    /// `browserTabs`: uma cópia das abas abertas, não um vetor vivo.
    pub fn browser_tabs(&self) -> Vec<BrowserTab> {
        self.browser_tabs.borrow().iter().map(|(_, tab)| tab.clone()).collect()
    }

    /// `activeBrowserTab`.
    pub fn active_browser_tab(&self) -> Option<BrowserTab> {
        let active_id = self.active_browser_tab_id.borrow().clone().filter(|id| !id.is_empty())?;
        self.tab_by_id(&active_id)
    }

    /// `openBrowserTab`.
    pub fn open_browser_tab(self: &Rc<Self>, url: &str, options: Option<BrowserTabShowOptions>) -> Promise<BrowserTab> {
        let this = self.clone();
        let url = url.to_owned();
        async_fn(async move {
            let options = options.unwrap_or_default();
            let view_column = ViewColumn::from(options.view_column);
            let dto = this
                .proxy
                .rpc_open_browser_tab(&url, Some(view_column), Some(EditorOptions { preserve_focus: options.preserve_focus, inactive: options.background, ..EditorOptions::default() }))
                .await?;

            Ok(this.get_or_create_tab(&dto))
        })
    }

    // #endregion

    // #region Internal helpers

    fn tab_by_id(&self, browser_id: &str) -> Option<BrowserTab> {
        self.browser_tabs.borrow().iter().find(|(id, _)| id == browser_id).map(|(_, tab)| tab.clone())
    }

    fn get_or_create_tab(&self, dto: &BrowserTabDto) -> BrowserTab {
        match self.tab_by_id(&dto.id) {
            Some(tab) => {
                tab.update(dto);
                tab
            }
            None => {
                let tab = ExtHostBrowserTab::new(dto.id.clone(), self.proxy.clone(), self.sessions.clone(), dto);
                self.browser_tabs.borrow_mut().push((dto.id.clone(), tab.clone()));
                self.open_emitter.fire(tab.clone());
                tab
            }
        }
    }

    // #endregion
}

impl IDisposable for ExtHostBrowsers {
    fn dispose(&self) {
        self.base.dispose();
    }
}

// #region Main thread callbacks

impl ExtHostBrowsersShape for ExtHostBrowsers {
    fn rpc_on_did_open_browser_tab(&self, dto: BrowserTabDto) {
        self.get_or_create_tab(&dto);
    }

    fn rpc_on_did_close_browser_tab(&self, browser_id: &str) {
        let removed = {
            let mut tabs = self.browser_tabs.borrow_mut();
            tabs.iter().position(|(id, _)| id == browser_id).map(|index| tabs.remove(index).1)
        };
        if let Some(tab) = removed {
            let mut active = self.active_browser_tab_id.borrow_mut();
            if active.as_deref() == Some(browser_id) {
                *active = None;
            }
            drop(active);
            self.close_emitter.fire(tab);
        }
    }

    fn rpc_on_did_change_active_browser_tab(&self, browser_id: Option<&str>) {
        *self.active_browser_tab_id.borrow_mut() = browser_id.map(str::to_owned);
        self.change_active_emitter.fire(self.active_browser_tab());
    }

    fn rpc_on_did_change_browser_tab_state(&self, data: BrowserTabDto) {
        if let Some(tab) = self.tab_by_id(&data.id)
            && tab.update(&data)
        {
            self.change_state_emitter.fire(tab);
        }
    }

    fn rpc_on_cdp_session_message(&self, session_id: &str, message: CdpMessage) {
        if let Some(session) = self.sessions.get(&session_id.to_owned()) {
            session.accept_message(&message);
        }
    }

    fn rpc_on_cdp_session_closed(&self, session_id: &str) {
        let session_id = session_id.to_owned();
        if let Some(session) = self.sessions.get(&session_id) {
            session.accept_closed();
            self.sessions.delete_and_dispose(&session_id);
        }
    }
}

// #endregion
