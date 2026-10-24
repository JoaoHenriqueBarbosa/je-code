//! Porte de `src/vs/workbench/api/common/extHostWindow.ts`: o lado da extensão do estado da janela (`vscode.window.state`,
//! `onDidChangeWindowState`, `nativeHandle`) e de `vscode.env.openExternal` e `asExternalUri`, sobre o proxy `MainThreadWindow`.
//!
//! Divergências:
//! - `IExtHostInitDataService.handle` (o handle nativo em base64) é o argumento `initial_handle` do construtor.
//! - `WindowState` com getters que leem o estado do instante da chamada (`getState`) é o [`WindowState`] por valor: o estado é uma
//!   cópia, e `onDidChangeWindowState` entrega o estado novo.
//! - Os `Promise.reject('texto')` do original rejeitam com um [`Error`] com o mesmo texto.
//! - `URI.parse` que lança vira a rejeição `Invalid uri - '<texto>'` (como o `try/catch` do original).
//! - `openUri` recebe [`UriOrString`] (`string | URI`).

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use je_base::buffer::decode_base64;
use je_base::errors::{self, Error};
use je_base::event::{Emitter, Event};
use je_base::network::schemas;
use je_base::promise::Promise;
use je_base::uri::URI;

use super::ext_host_protocol::{ExtHostWindowShape, IMainContext, IOpenUriOptions, MainContext, MainThreadWindowShape};

/// `vscode.WindowState`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WindowState {
    pub focused: bool,
    pub active: bool,
}

/// `string | URI`, o argumento de `openUri`.
#[derive(Clone, Debug)]
pub enum UriOrString {
    Text(String),
    Uri(URI),
}

/// `ExtHostWindow`.
pub struct ExtHostWindow {
    proxy: Rc<dyn MainThreadWindowShape>,
    on_did_change_window_state: Rc<Emitter<WindowState>>,
    native_handle: RefCell<Option<Vec<u8>>>,
    state: Cell<WindowState>,
}

impl ExtHostWindow {
    /// `ExtHostWindow.InitialState`.
    const INITIAL_STATE: WindowState = WindowState { focused: true, active: true };

    pub fn new(ext_host_rpc: &dyn IMainContext, initial_handle: Option<&str>) -> Rc<Self> {
        let this = Rc::new(Self {
            proxy: ext_host_rpc.get_proxy(&MainContext::MAIN_THREAD_WINDOW),
            on_did_change_window_state: Emitter::new(),
            native_handle: RefCell::new(initial_handle.filter(|handle| !handle.is_empty()).map(|handle| decode_base64(handle).to_vec())),
            state: Cell::new(Self::INITIAL_STATE),
        });
        let weak = Rc::downgrade(&this);
        let _ = this.proxy.rpc_get_initial_state().then(move |initial| {
            if let Some(this) = weak.upgrade() {
                this.on_did_change_window_property(WindowProperty::Focused, initial.is_focused);
                this.on_did_change_window_property(WindowProperty::Active, initial.is_active);
            }
        });
        this
    }

    /// `onDidChangeWindowState`.
    pub fn on_did_change_window_state(&self) -> Event<WindowState> {
        self.on_did_change_window_state.event()
    }

    /// `getState()`.
    pub fn get_state(&self) -> WindowState {
        self.state.get()
    }

    /// `nativeHandle`.
    pub fn native_handle(&self) -> Option<Vec<u8>> {
        self.native_handle.borrow().clone()
    }

    /// `onDidChangeWindowProperty(property, value)`.
    pub fn on_did_change_window_property(&self, property: WindowProperty, value: bool) {
        let current = self.state.get();
        let changed = match property {
            WindowProperty::Focused if current.focused != value => WindowState { focused: value, ..current },
            WindowProperty::Active if current.active != value => WindowState { active: value, ..current },
            _ => return,
        };
        self.state.set(changed);
        self.on_did_change_window_state.fire(changed);
    }

    /// `openUri(stringOrUri, options)`.
    pub fn open_uri(&self, string_or_uri: UriOrString, options: IOpenUriOptions) -> Promise<bool> {
        let (uri, uri_as_string) = match string_or_uri {
            UriOrString::Text(text) => match errors::catch(|| URI::parse(text.as_str(), false)) {
                Ok(uri) => (uri, Some(text)),
                Err(_) => return Promise::rejected(Error::new(format!("Invalid uri - '{text}'"))),
            },
            UriOrString::Uri(uri) => (uri, None),
        };
        let scheme = uri.scheme().to_string_lossy();
        if scheme.trim().is_empty() {
            return Promise::rejected(Error::new("Invalid scheme - cannot be empty"));
        } else if scheme == schemas::COMMAND {
            return Promise::rejected(Error::new(format!("Invalid scheme '{scheme}'")));
        }
        self.proxy.rpc_open_uri(uri, uri_as_string, options)
    }

    /// `asExternalUri(uri, options)`.
    pub fn as_external_uri(&self, uri: &URI, options: IOpenUriOptions) -> Promise<URI> {
        if uri.scheme().to_string_lossy().trim().is_empty() {
            return Promise::rejected(Error::new("Invalid scheme - cannot be empty"));
        }
        self.proxy.rpc_as_external_uri(uri.clone(), options)
    }
}

/// `keyof WindowState`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WindowProperty {
    Focused,
    Active,
}

impl ExtHostWindowShape for ExtHostWindow {
    fn rpc_on_did_change_active_native_window_handle(&self, handle: Option<String>) {
        *self.native_handle.borrow_mut() = handle.filter(|handle| !handle.is_empty()).map(|handle| decode_base64(&handle).to_vec());
    }

    fn rpc_on_did_change_window_focus(&self, value: bool) {
        self.on_did_change_window_property(WindowProperty::Focused, value);
    }

    fn rpc_on_did_change_window_active(&self, value: bool) {
        self.on_did_change_window_property(WindowProperty::Active, value);
    }
}
