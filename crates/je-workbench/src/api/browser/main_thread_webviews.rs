//! Porte de `src/vs/workbench/api/browser/mainThreadWebviews.ts`: o lado do workbench do RPC `ExtHostWebviews`, que liga
//! cada webview ao host de extensões (mensagens, links clicados, CSP ausente) e responde a `$setHtml`, `$setOptions` e
//! `$postMessage`.
//!
//! Divergências:
//! - **Injeção.** O construtor `(context, @IOpenerService, @IProductService)` é [`MainThreadWebviews::new`]; `IExtHostContext` é
//!   o [`IMainContext`] (só o `getProxy`).
//! - **`IMainThreadWebviews`.** O `MainThreadWebviewPanels` (e o `MainThreadCustomEditors`) usam do `MainThreadWebviews` só
//!   `addWebview` e `getWebviewResolvedFailedContent`; os testes do original os trocam por um `mock<MainThreadWebviews>()`, que em
//!   Rust é um objeto de trait ([`IMainThreadWebviews`]), implementado pelo [`MainThreadWebviews`].
//! - **`addWebview`** lança no original (`Webview already registered`): devolve [`Result`].
//! - **`webview.onMessage`** entrega o `message.message` do evento ao `serializeWebviewMessage`; a lista `transfer` que
//!   acompanha o evento não é lida (o original também só lê `message.message`).
//! - **Erros dos ouvintes.** `getWebview(handle)` lança `Unknown webview handle` num clique de link de um webview que não está
//!   registrado, e `deserializeWebviewMessage` lança numa mensagem ilegível; como o ouvinte e o `$postMessage` não têm a quem
//!   devolver, o erro de link vai ao [`errors::on_unexpected_error`] e o de `$postMessage` é a promessa rejeitada.
//! - **`isWeb`** é `Platform::detect().is_web`; `localize('errorMessage', ...)` é [`localize`].
//! - **`reviveWebviewContentOptions`** e **`reviveWebviewExtension`** são as funções livres de mesmo nome.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::{Rc, Weak};

use je_base::buffer::VSBuffer;
use je_base::errors::{self, Error};
use je_base::js_string::JsString;
use je_base::lifecycle::{Disposable, DisposableStore, IDisposable};
use je_base::network::schemas;
use je_base::nls::localize;
use je_base::platform::Platform;
use je_base::promise::Promise;
use je_base::strings::escape;
use je_base::uri::URI;
use je_platform::opener::opener::{AllowCommands, AllowContributedOpeners, IOpenerService, OpenOptions, OpenResource};
use je_platform::product::IProductService;

use crate::api::common::ext_host_protocol::{
    EnableCommandUris, ExtHostContext, ExtHostWebviewsShape, IMainContext, MainThreadWebviewsShape, SerializableObjectWithBuffers,
    WebviewContentOptions as WebviewContentOptionsDto, WebviewExtensionDescription as WebviewExtensionDescriptionDto,
};
use crate::api::common::ext_host_webview_messaging::{deserialize_webview_message, serialize_webview_message};
use crate::contrib::webview::browser_webview::{IWebview, WebviewContentOptions, WebviewExtensionDescription};

/// `standardSupportedLinkSchemes`.
const STANDARD_SUPPORTED_LINK_SCHEMES: [&str; 5] = [schemas::HTTP, schemas::HTTPS, schemas::MAILTO, schemas::VSCODE, "vscode-insider"];

/// O que os outros `MainThread*` usam do [`MainThreadWebviews`] (ver a nota do módulo).
pub trait IMainThreadWebviews {
    /// `addWebview(handle, webview, options)`.
    fn add_webview(&self, handle: &str, webview: Rc<dyn IWebview>, options: MainThreadWebviewOptions) -> Result<(), Error>;

    /// `getWebviewResolvedFailedContent(viewType)`.
    fn get_webview_resolved_failed_content(&self, view_type: &str) -> String;
}

/// O `options` de `addWebview`: `{ serializeBuffersForPostMessage: boolean }`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MainThreadWebviewOptions {
    pub serialize_buffers_for_post_message: bool,
}

pub struct MainThreadWebviews {
    this: Weak<MainThreadWebviews>,
    base: Rc<Disposable>,
    proxy: Rc<dyn ExtHostWebviewsShape>,
    webviews: RefCell<HashMap<String, Rc<dyn IWebview>>>,
    opener_service: Rc<dyn IOpenerService>,
    product_service: Rc<dyn IProductService>,
}

impl MainThreadWebviews {
    pub fn new(context: &dyn IMainContext, opener_service: Rc<dyn IOpenerService>, product_service: Rc<dyn IProductService>) -> Rc<Self> {
        let proxy = context.get_proxy(&ExtHostContext::EXT_HOST_WEBVIEWS);
        Rc::new_cyclic(|this| Self { this: this.clone(), base: Disposable::new(), proxy, webviews: RefCell::new(HashMap::new()), opener_service, product_service })
    }

    fn hookup_webview_event_delegate(&self, handle: &str, webview: &Rc<dyn IWebview>, options: MainThreadWebviewOptions) {
        let disposables = DisposableStore::new();

        let this = self.this.clone();
        let link_handle = handle.to_owned();
        disposables.add(webview.on_did_click_link().listen(move |uri| {
            if let Some(this) = this.upgrade() {
                this.on_did_click_link(&link_handle, uri);
            }
        }));

        let proxy = self.proxy.clone();
        let message_handle = handle.to_owned();
        disposables.add(webview.on_message().listen(move |message| {
            let serialized = serialize_webview_message(&message.message, options.serialize_buffers_for_post_message);
            proxy.rpc_on_message(&message_handle, &serialized.message, SerializableObjectWithBuffers::new(serialized.buffers));
        }));

        let proxy = self.proxy.clone();
        let csp_handle = handle.to_owned();
        disposables.add(webview.on_missing_csp().listen(move |extension| proxy.rpc_on_missing_csp(&csp_handle, &extension.value)));

        let this = self.this.clone();
        let dispose_handle = handle.to_owned();
        let store = disposables.clone();
        disposables.add(webview.on_did_dispose().listen(move |_| {
            store.dispose();
            if let Some(this) = this.upgrade() {
                this.webviews.borrow_mut().remove(&dispose_handle);
            }
        }));
    }

    fn on_did_click_link(&self, handle: &str, link: &str) {
        let webview = match self.get_webview(handle) {
            Ok(webview) => webview,
            Err(error) => {
                errors::on_unexpected_error(&error);
                return;
            }
        };
        if self.is_supported_link(&webview, &URI::parse(link, false)) {
            let enable_command_uris = webview.content_options().enable_command_uris;
            let allow_commands = matches!(enable_command_uris, Some(EnableCommandUris::Commands(_)) | Some(EnableCommandUris::All(true)));
            self.opener_service.open(
                &OpenResource::Text(JsString::from(link)),
                Some(&OpenOptions {
                    from_user_gesture: Some(true),
                    allow_contributed_openers: Some(AllowContributedOpeners::Bool(true)),
                    allow_commands: Some(AllowCommands::Bool(allow_commands)),
                    from_workspace: Some(true),
                    ..OpenOptions::default()
                }),
            );
        }
    }

    fn is_supported_link(&self, webview: &Rc<dyn IWebview>, link: &URI) -> bool {
        if STANDARD_SUPPORTED_LINK_SCHEMES.iter().any(|scheme| *link.scheme() == *scheme) {
            return true;
        }

        if !Platform::detect().is_web && link.scheme() == self.product_service.url_protocol() {
            return true;
        }

        if link.scheme() == schemas::COMMAND {
            return match webview.content_options().enable_command_uris {
                Some(EnableCommandUris::Commands(commands)) => commands.iter().any(|command| link.path() == command.as_str()),
                Some(EnableCommandUris::All(all)) => all,
                None => false,
            };
        }

        false
    }

    fn try_get_webview(&self, handle: &str) -> Option<Rc<dyn IWebview>> {
        self.webviews.borrow().get(handle).cloned()
    }

    fn get_webview(&self, handle: &str) -> Result<Rc<dyn IWebview>, Error> {
        self.try_get_webview(handle).ok_or_else(|| Error::new(format!("Unknown webview handle:{handle}")))
    }
}

impl IMainThreadWebviews for MainThreadWebviews {
    fn add_webview(&self, handle: &str, webview: Rc<dyn IWebview>, options: MainThreadWebviewOptions) -> Result<(), Error> {
        if self.webviews.borrow().contains_key(handle) {
            return Err(Error::new("Webview already registered"));
        }

        self.webviews.borrow_mut().insert(handle.to_owned(), webview.clone());
        self.hookup_webview_event_delegate(handle, &webview, options);
        Ok(())
    }

    fn get_webview_resolved_failed_content(&self, view_type: &str) -> String {
        let units: Vec<u16> = view_type.encode_utf16().collect();
        let message = localize("errorMessage", "An error occurred while loading view: {0}", &[escape(&units).to_string_lossy().as_str()]);
        format!(
            "<!DOCTYPE html>\n\t\t<html>\n\t\t\t<head>\n\t\t\t\t<meta http-equiv=\"Content-type\" content=\"text/html;charset=UTF-8\">\n\t\t\t\t<meta http-equiv=\"Content-Security-Policy\" content=\"default-src 'none';\">\n\t\t\t</head>\n\t\t\t<body>{message}</body>\n\t\t</html>"
        )
    }
}

impl IDisposable for MainThreadWebviews {
    fn dispose(&self) {
        self.base.dispose();
    }
}

impl MainThreadWebviewsShape for MainThreadWebviews {
    fn rpc_set_html(&self, handle: &str, value: &str) {
        if let Some(webview) = self.try_get_webview(handle) {
            webview.set_html(value);
        }
    }

    fn rpc_set_options(&self, handle: &str, options: WebviewContentOptionsDto) {
        if let Some(webview) = self.try_get_webview(handle) {
            webview.set_content_options(revive_webview_content_options(&options));
        }
    }

    fn rpc_post_message(&self, handle: &str, json_message: &str, buffers: Vec<VSBuffer>) -> Promise<bool> {
        let Some(webview) = self.try_get_webview(handle) else {
            return Promise::resolved(false);
        };
        match deserialize_webview_message(json_message, &buffers) {
            Ok(deserialized) => webview.post_message(deserialized.message, Some(deserialized.array_buffers)),
            Err(error) => Promise::rejected(error),
        }
    }
}

/// `reviveWebviewExtension(extensionData)`.
pub fn revive_webview_extension(extension_data: &WebviewExtensionDescriptionDto) -> WebviewExtensionDescription {
    WebviewExtensionDescription { id: extension_data.id.clone(), location: Some(extension_data.location.clone()) }
}

/// `reviveWebviewContentOptions(webviewOptions)`.
pub fn revive_webview_content_options(webview_options: &WebviewContentOptionsDto) -> WebviewContentOptions {
    WebviewContentOptions {
        allow_multiple_api_acquire: None,
        allow_scripts: webview_options.enable_scripts,
        allow_forms: webview_options.enable_forms,
        forward_untrusted_keypress_events: Some(true), // This is always enabled for extensions
        enable_command_uris: webview_options.enable_command_uris.clone(),
        local_resource_roots: webview_options.local_resource_roots.clone(),
        port_mapping: webview_options.port_mapping.clone(),
    }
}
