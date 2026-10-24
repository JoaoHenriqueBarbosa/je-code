//! Porte de `src/vs/workbench/api/common/extHostWebviewView.ts`: o lado das extensões das views de webview
//! (`vscode.WebviewView`, `window.registerWebviewViewProvider`) e o `ExtHostWebviewViews`, que guarda os provedores por
//! `viewType` e responde ao protocolo da thread principal (`$resolveWebviewView`, `$onDidChangeWebviewViewVisibility`,
//! `$disposeWebviewView`).
//!
//! Divergências:
//! - Os setters e getters do `vscode.WebviewView` são métodos; os que lançam `Webview is disposed` (`title`, `set_title`,
//!   `description`, `set_description`, `badge`, `set_badge`, `show`) devolvem [`Result`].
//! - `vscode.WebviewViewProvider` é o trait [`WebviewViewProvider`], cujo `resolveWebviewView` devolve `Thenable<void> | void`
//!   ([`MaybePromise`]). `registerWebviewViewProvider` lança (`viewType` já registrado): devolve [`Result`], com o
//!   `vscode.Disposable` de `ext_host_types`.
//! - O setter de `badge` guarda o resultado de `ViewBadge.from(badge)` (o `IViewBadge`), como o original, e manda ao proxy o
//!   mesmo valor (no original, o `badge` bruto, que tem os mesmos campos).
//! - `$onDidChangeWebviewViewVisibility` e `$disposeWebviewView` lançam `No webview found` no original quando o handle é
//!   desconhecido; como os dois métodos não devolvem nada, o erro vai ao [`errors::on_unexpected_error`].
//! - `ExtHostWebviewView extends Disposable` é um campo [`Disposable`] com os dois emissores registrados.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use je_base::async_::{async_fn, await_maybe};
use je_base::cancellation::CancellationToken;
use je_base::dynamic::Dynamic;
use je_base::errors::{self, Error};
use je_base::event::{Emitter, Event};
use je_base::lifecycle::{Disposable, IDisposable};
use je_base::promise::{MaybePromise, Promise};
use je_extensions::scanner::ExtensionDescription;

use super::ext_host_protocol::{
    ExtHostWebviewViewsShape, IMainContext, MainContext, MainThreadWebviewViewsShape, WebviewViewProviderOptions,
};
use super::ext_host_type_converters::ViewBadge;
use super::ext_host_types::{Disposable as ApiDisposable, ViewBadge as ApiViewBadge};
use super::ext_host_webview::{ExtHostWebview, ExtHostWebviews, should_serialize_buffers_for_post_message, to_extension_data};
use crate::common::views::IViewBadge;

pub struct ExtHostWebviewView {
    base: Rc<Disposable>,
    handle: String,
    proxy: Rc<dyn MainThreadWebviewViewsShape>,

    view_type: String,
    webview: Rc<ExtHostWebview>,

    is_disposed: Cell<bool>,
    is_visible: Cell<bool>,
    title: RefCell<Option<String>>,
    description: RefCell<Option<String>>,
    badge: RefCell<Option<IViewBadge>>,

    on_did_change_visibility_emitter: Rc<Emitter<()>>,
    on_did_dispose_emitter: Rc<Emitter<()>>,
}

impl ExtHostWebviewView {
    pub fn new(
        handle: &str,
        proxy: Rc<dyn MainThreadWebviewViewsShape>,
        view_type: &str,
        title: Option<String>,
        webview: Rc<ExtHostWebview>,
        is_visible: bool,
    ) -> Rc<Self> {
        let base = Disposable::new();
        let on_did_change_visibility_emitter = Emitter::new();
        base.register(on_did_change_visibility_emitter.clone());
        let on_did_dispose_emitter = Emitter::new();
        base.register(on_did_dispose_emitter.clone());
        Rc::new(Self {
            base,
            handle: handle.to_owned(),
            proxy,
            view_type: view_type.to_owned(),
            webview,
            is_disposed: Cell::new(false),
            is_visible: Cell::new(is_visible),
            title: RefCell::new(title),
            description: RefCell::new(None),
            badge: RefCell::new(None),
            on_did_change_visibility_emitter,
            on_did_dispose_emitter,
        })
    }

    /// `onDidChangeVisibility`.
    pub fn on_did_change_visibility(&self) -> Event<()> {
        self.on_did_change_visibility_emitter.event()
    }

    /// `onDidDispose`.
    pub fn on_did_dispose(&self) -> Event<()> {
        self.on_did_dispose_emitter.event()
    }

    /// `title` (getter).
    pub fn title(&self) -> Result<Option<String>, Error> {
        self.assert_not_disposed()?;
        Ok(self.title.borrow().clone())
    }

    /// `title` (setter).
    pub fn set_title(&self, value: Option<&str>) -> Result<(), Error> {
        self.assert_not_disposed()?;
        if self.title.borrow().as_deref() != value {
            *self.title.borrow_mut() = value.map(str::to_owned);
            self.proxy.rpc_set_webview_view_title(&self.handle, value);
        }
        Ok(())
    }

    /// `description` (getter).
    pub fn description(&self) -> Result<Option<String>, Error> {
        self.assert_not_disposed()?;
        Ok(self.description.borrow().clone())
    }

    /// `description` (setter).
    pub fn set_description(&self, value: Option<&str>) -> Result<(), Error> {
        self.assert_not_disposed()?;
        if self.description.borrow().as_deref() != value {
            *self.description.borrow_mut() = value.map(str::to_owned);
            self.proxy.rpc_set_webview_view_description(&self.handle, value);
        }
        Ok(())
    }

    /// `visible`.
    pub fn visible(&self) -> bool {
        self.is_visible.get()
    }

    /// `webview`.
    pub fn webview(&self) -> Rc<ExtHostWebview> {
        self.webview.clone()
    }

    /// `viewType`.
    pub fn view_type(&self) -> String {
        self.view_type.clone()
    }

    /// `_setVisible(visible)`.
    pub fn set_visible(&self, visible: bool) {
        if visible == self.is_visible.get() || self.is_disposed.get() {
            return;
        }

        self.is_visible.set(visible);
        self.on_did_change_visibility_emitter.fire(());
    }

    /// `badge` (getter).
    pub fn badge(&self) -> Result<Option<IViewBadge>, Error> {
        self.assert_not_disposed()?;
        Ok(self.badge.borrow().clone())
    }

    /// `badge` (setter).
    pub fn set_badge(&self, badge: Option<&ApiViewBadge>) -> Result<(), Error> {
        self.assert_not_disposed()?;

        let current = self.badge.borrow().clone();
        if badge.map(|badge| badge.value) == current.as_ref().map(|current| current.value)
            && badge.map(|badge| badge.tooltip.as_str()) == current.as_ref().map(|current| current.tooltip.as_str())
        {
            return Ok(());
        }

        let converted = ViewBadge::from(badge);
        *self.badge.borrow_mut() = converted.clone();
        self.proxy.rpc_set_webview_view_badge(&self.handle, converted);
        Ok(())
    }

    /// `show(preserveFocus?)`.
    pub fn show(&self, preserve_focus: Option<bool>) -> Result<(), Error> {
        self.assert_not_disposed()?;
        self.proxy.rpc_show(&self.handle, preserve_focus.unwrap_or(false));
        Ok(())
    }

    fn assert_not_disposed(&self) -> Result<(), Error> {
        if self.is_disposed.get() {
            return Err(Error::new("Webview is disposed"));
        }
        Ok(())
    }
}

impl IDisposable for ExtHostWebviewView {
    fn dispose(&self) {
        if self.is_disposed.get() {
            return;
        }

        self.is_disposed.set(true);
        self.on_did_dispose_emitter.fire(());

        self.webview.dispose();

        self.base.dispose();
    }
}

/// `vscode.WebviewViewResolveContext`.
#[derive(Clone, Debug)]
pub struct WebviewViewResolveContext {
    pub state: Dynamic,
}

/// `vscode.WebviewViewProvider`.
pub trait WebviewViewProvider {
    /// `resolveWebviewView(webviewView, context, token)`.
    fn resolve_webview_view(&self, webview_view: Rc<ExtHostWebviewView>, context: WebviewViewResolveContext, token: CancellationToken) -> MaybePromise<()>;
}

/// O `webviewOptions` de `registerWebviewViewProvider`: `{ retainContextWhenHidden? }`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct WebviewViewProviderRegistrationOptions {
    pub retain_context_when_hidden: Option<bool>,
}

struct ViewProviderEntry {
    provider: Rc<dyn WebviewViewProvider>,
    extension: ExtensionDescription,
}

/// O estado que o `ExtHostWebviewViews` e a `Disposable` devolvida por `registerWebviewViewProvider` dividem.
struct Inner {
    proxy: Rc<dyn MainThreadWebviewViewsShape>,
    view_providers: RefCell<HashMap<String, Rc<ViewProviderEntry>>>,
    webview_views: RefCell<HashMap<String, Rc<ExtHostWebviewView>>>,
    ext_host_webview: Rc<ExtHostWebviews>,
}

impl Inner {
    fn get_webview_view(&self, handle: &str) -> Result<Rc<ExtHostWebviewView>, Error> {
        self.webview_views.borrow().get(handle).cloned().ok_or_else(|| Error::new("No webview found"))
    }
}

pub struct ExtHostWebviewViews {
    inner: Rc<Inner>,
}

impl ExtHostWebviewViews {
    pub fn new(main_context: &dyn IMainContext, ext_host_webview: Rc<ExtHostWebviews>) -> Self {
        Self {
            inner: Rc::new(Inner {
                proxy: main_context.get_proxy(&MainContext::MAIN_THREAD_WEBVIEW_VIEWS),
                view_providers: RefCell::new(HashMap::new()),
                webview_views: RefCell::new(HashMap::new()),
                ext_host_webview,
            }),
        }
    }

    /// `registerWebviewViewProvider(extension, viewType, provider, webviewOptions?)`.
    pub fn register_webview_view_provider(
        &self,
        extension: &ExtensionDescription,
        view_type: &str,
        provider: Rc<dyn WebviewViewProvider>,
        webview_options: Option<WebviewViewProviderRegistrationOptions>,
    ) -> Result<ApiDisposable, Error> {
        let inner = &self.inner;
        if inner.view_providers.borrow().contains_key(view_type) {
            return Err(Error::new(format!("View provider for '{view_type}' already registered")));
        }

        inner.view_providers.borrow_mut().insert(view_type.to_owned(), Rc::new(ViewProviderEntry { provider, extension: extension.clone() }));
        inner.proxy.rpc_register_webview_view_provider(
            to_extension_data(extension),
            view_type,
            WebviewViewProviderOptions {
                retain_context_when_hidden: webview_options.and_then(|options| options.retain_context_when_hidden),
                serialize_buffers_for_post_message: should_serialize_buffers_for_post_message(extension),
            },
        );

        let inner = inner.clone();
        let view_type = view_type.to_owned();
        Ok(ApiDisposable::new(move || {
            inner.view_providers.borrow_mut().remove(&view_type);
            inner.proxy.rpc_unregister_webview_view_provider(&view_type);
        }))
    }
}

impl ExtHostWebviewViewsShape for ExtHostWebviewViews {
    fn rpc_resolve_webview_view(&self, webview_handle: &str, view_type: &str, title: Option<String>, state: Dynamic, cancellation: CancellationToken) -> Promise<()> {
        let inner = self.inner.clone();
        let (webview_handle, view_type) = (webview_handle.to_owned(), view_type.to_owned());
        async_fn(async move {
            let entry = inner.view_providers.borrow().get(&view_type).cloned();
            let Some(entry) = entry else {
                return Err(Error::new(format!("No view provider found for '{view_type}'")));
            };

            let webview = inner.ext_host_webview.create_new_webview(&webview_handle, &Default::default(), &entry.extension);
            let revived_view = ExtHostWebviewView::new(&webview_handle, inner.proxy.clone(), &view_type, title, webview, true);

            inner.webview_views.borrow_mut().insert(webview_handle, revived_view.clone());

            await_maybe(entry.provider.resolve_webview_view(revived_view, WebviewViewResolveContext { state }, cancellation)).await
        })
    }

    fn rpc_on_did_change_webview_view_visibility(&self, webview_handle: &str, visible: bool) {
        match self.inner.get_webview_view(webview_handle) {
            Ok(webview_view) => webview_view.set_visible(visible),
            Err(error) => errors::on_unexpected_error(&error),
        }
    }

    fn rpc_dispose_webview_view(&self, webview_handle: &str) {
        match self.inner.get_webview_view(webview_handle) {
            Ok(webview_view) => {
                self.inner.webview_views.borrow_mut().remove(webview_handle);
                webview_view.dispose();

                self.inner.ext_host_webview.delete_webview(webview_handle);
            }
            Err(error) => errors::on_unexpected_error(&error),
        }
    }
}
