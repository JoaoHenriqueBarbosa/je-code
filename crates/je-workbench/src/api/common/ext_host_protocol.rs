//! Porte parcial de `src/vs/workbench/api/common/extHost.protocol.ts` e do que ele pede do RPC
//! (`proxyIdentifier.ts`), só o que `extHostBrowsers.ts` usa.
//!
//! Divergências:
//! - É um porte parcial: do arquivo original entram `BrowserTabDto`, `MainThreadBrowsersShape`,
//!   `ExtHostBrowsersShape`, `IMainContext` e o identificador `MainContext.MainThreadBrowsers`. O resto dos
//!   DTOs, das formas e dos identificadores entra junto de cada arquivo `extHost*.ts` que os pedir.
//! - O prefixo `$` dos métodos de RPC vira `rpc_` (`$openBrowserTab` é `rpc_open_browser_tab`), porque
//!   `onDidOpenBrowserTab` (o evento da API) e `$onDidOpenBrowserTab` (o retorno da thread principal) ficariam
//!   com o mesmo nome em snake case.
//! - `IMainContext` é um trait sem genéricos (objeto de trait) com `get_proxy_erased`/`set_erased`; o
//!   `getProxy<T>(identifier)` e o `set<T, R extends T>(identifier, value)` genéricos do original são os
//!   métodos [`dyn IMainContext::get_proxy`](IMainContext) e [`dyn IMainContext::set`](IMainContext), que
//!   fazem o `downcast` do proxy guardado pelo `sid` do identificador. `Proxied<T>` é `Rc<T>` (a thread
//!   principal e a de extensões dividem o processo aqui, e o proxy de teste responde direto). `drain`,
//!   `assertRegistered`, `dispose` e a conexão remota do `IRPCProtocol` ficam para o porte do protocolo.
//! - `MainThreadBrowsersShape extends IDisposable` é o supertrait [`IDisposable`].
//! - `CDPRequest`, `CDPResponse` e `CDPEvent` (`platform/browserView/common/cdp/types.ts`) são definidos
//!   aqui até esse arquivo ser portado. Os `unknown` e `object` do protocolo são [`Dynamic`]. O tipo
//!   união `CDPResponse | CDPEvent` é [`CdpMessage`].
//! - `EditorGroupColumn` e `IEditorOptions` vêm de [`super::ext_host_type_converters`] e de
//!   [`je_platform::opener::editor_types::EditorOptions`].

use std::any::Any;
use std::marker::PhantomData;
use std::rc::Rc;

use je_base::dynamic::Dynamic;
use je_base::lifecycle::IDisposable;
use je_base::promise::Promise;
use je_platform::opener::editor_types::EditorOptions;

pub use super::ext_host_type_converters::EditorGroupColumn;

// #region Mensagens CDP

/// `CDPRequest`.
#[derive(Clone)]
pub struct CdpRequest {
    pub id: f64,
    pub method: String,
    pub params: Option<Dynamic>,
    pub session_id: Option<String>,
}

/// O `{ code: number; message: string }` de `CDPResponse.error`.
#[derive(Clone, Debug, PartialEq)]
pub struct CdpResponseError {
    pub code: f64,
    pub message: String,
}

/// `CDPResponse`.
#[derive(Clone)]
pub struct CdpResponse {
    pub id: f64,
    pub result: Option<Dynamic>,
    pub error: Option<CdpResponseError>,
    pub session_id: Option<String>,
}

/// `CDPEvent`.
#[derive(Clone)]
pub struct CdpEvent {
    pub method: String,
    pub params: Dynamic,
    pub session_id: Option<String>,
}

/// `CDPResponse | CDPEvent`.
#[derive(Clone)]
pub enum CdpMessage {
    Response(CdpResponse),
    Event(CdpEvent),
}

/// O objeto JavaScript só com as propriedades definidas (uma propriedade `undefined` não aparece).
fn defined_object(entries: impl IntoIterator<Item = (&'static str, Option<Dynamic>)>) -> Dynamic {
    Dynamic::object(entries.into_iter().filter_map(|(key, value)| value.map(|value| (key, value))))
}

impl CdpMessage {
    /// A mensagem como o `unknown` que a extensão recebe em `onDidReceiveMessage`.
    pub fn to_dynamic(&self) -> Dynamic {
        let session_id = |session_id: &Option<String>| session_id.as_deref().map(Dynamic::string);
        match self {
            Self::Response(response) => defined_object([
                ("id", Some(Dynamic::Number(response.id))),
                ("result", response.result.clone()),
                (
                    "error",
                    response.error.as_ref().map(|error| Dynamic::object([("code", Dynamic::Number(error.code)), ("message", Dynamic::string(&error.message))])),
                ),
                ("sessionId", session_id(&response.session_id)),
            ]),
            Self::Event(event) => defined_object([
                ("method", Some(Dynamic::string(&event.method))),
                ("params", Some(event.params.clone())),
                ("sessionId", session_id(&event.session_id)),
            ]),
        }
    }
}

// #endregion

// #region Proxies

/// `ProxyIdentifier<T>`: o identificador de um proxy de RPC, tipado pela forma `T`.
pub struct ProxyIdentifier<T: ?Sized + 'static> {
    pub sid: &'static str,
    marker: PhantomData<fn() -> Rc<T>>,
}

impl<T: ?Sized + 'static> ProxyIdentifier<T> {
    /// `createProxyIdentifier<T>(identifier)`.
    pub const fn new(sid: &'static str) -> Self {
        Self { sid, marker: PhantomData }
    }
}

/// `IMainContext`: de onde o lado das extensões pega o proxy de cada serviço da thread principal.
pub trait IMainContext {
    /// O proxy registrado em `sid`: um `Rc<T>` guardado como `Rc<dyn Any>` (ver [`dyn IMainContext::get_proxy`](IMainContext)).
    fn get_proxy_erased(&self, sid: &'static str) -> Rc<dyn Any>;

    /// Registra o objeto local `value` (um `Rc<T>` guardado como `Rc<dyn Any>`) em `sid`.
    fn set_erased(&self, sid: &'static str, value: Rc<dyn Any>);
}

impl<'a> dyn IMainContext + 'a {
    /// `getProxy<T>(identifier)`. Levanta um erro se o proxy guardado não tem a forma `T`.
    pub fn get_proxy<T: ?Sized + 'static>(&self, identifier: &ProxyIdentifier<T>) -> Rc<T> {
        self.get_proxy_erased(identifier.sid)
            .downcast_ref::<Rc<T>>()
            .cloned()
            .unwrap_or_else(|| panic!("o proxy '{}' não tem a forma pedida", identifier.sid))
    }

    /// `set<T, R extends T>(identifier, value)`: registra `value` e o devolve.
    pub fn set<T: ?Sized + 'static>(&self, identifier: &ProxyIdentifier<T>, value: Rc<T>) -> Rc<T> {
        self.set_erased(identifier.sid, Rc::new(value.clone()));
        value
    }
}

// #endregion

// #region Browsers

/// `BrowserTabDto`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BrowserTabDto {
    pub id: String,
    pub url: String,
    pub title: String,
    pub favicon: Option<String>,
}

/// `MainThreadBrowsersShape`.
pub trait MainThreadBrowsersShape: IDisposable {
    fn rpc_open_browser_tab(&self, url: &str, view_column: Option<EditorGroupColumn>, options: Option<EditorOptions>) -> Promise<BrowserTabDto>;
    fn rpc_close_browser_tab(&self, browser_id: &str) -> Promise<()>;
    fn rpc_start_cdp_session(&self, session_id: &str, browser_id: &str) -> Promise<()>;
    fn rpc_close_cdp_session(&self, session_id: &str) -> Promise<()>;
    fn rpc_send_cdp_message(&self, session_id: &str, message: CdpRequest) -> Promise<()>;
}

/// `ExtHostBrowsersShape`.
pub trait ExtHostBrowsersShape {
    fn rpc_on_did_open_browser_tab(&self, browser: BrowserTabDto);
    fn rpc_on_did_close_browser_tab(&self, browser_id: &str);
    fn rpc_on_did_change_active_browser_tab(&self, browser_id: Option<&str>);
    fn rpc_on_did_change_browser_tab_state(&self, browser: BrowserTabDto);
    fn rpc_on_cdp_session_message(&self, session_id: &str, message: CdpMessage);
    fn rpc_on_cdp_session_closed(&self, session_id: &str);
}

// #endregion

/// `MainContext`: os identificadores dos proxies da thread principal.
pub struct MainContext;

impl MainContext {
    pub const MAIN_THREAD_BROWSERS: ProxyIdentifier<dyn MainThreadBrowsersShape> = ProxyIdentifier::new("MainThreadBrowsers");
}
