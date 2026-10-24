//! Porte de `src/vs/workbench/api/common/extHostWebview.ts`: o lado das extensões de um webview (`vscode.Webview`) e o
//! `ExtHostWebviews`, que guarda os webviews por handle e responde ao protocolo da thread principal (`$onMessage`,
//! `$onMissingCsp`).
//!
//! Divergências:
//! - Os setters e getters do `vscode.Webview` são métodos. Os que lançam `Webview is disposed` (`html`, `set_html`,
//!   `options`, `set_options`) devolvem [`Result`]; o `html` também lança o `URIError` de um `vscode-resource:` com `%`
//!   malformado (`decodeURIComponent` dentro do `replace`).
//! - `vscode.WebviewOptions` tem os mesmos campos de `IWebviewContentOptions` (que o `reviveOptions` só copia, com as
//!   `UriComponents` já revividas): é o alias [`WebviewOptions`] do [`WebviewContentOptions`] do protocolo, e
//!   `objects.equals(this.#options, newOptions)` é a igualdade derivada.
//! - `IExtHostWorkspace` entra pelo trait [`IExtHostWorkspaceFolders`] (só o `getWorkspaceFolders`, que é o que o arquivo
//!   lê dele), implementado pelo [`ExtHostWorkspace`]; `IExtHostApiDeprecationService` é o trait mínimo de
//!   `ext_host_language_features_api` (só o `report`).
//! - `IExtensionDescription.extensionLocation` (um `URI`) é o texto `extension_location` da [`ExtensionDescription`], lido
//!   com [`URI::parse`] onde o original o usa.
//! - `$onMessage` recusa uma mensagem que não se lê (o `JSON.parse` do original lançaria dentro do RPC): o erro vai ao
//!   [`errors::on_unexpected_error`], porque o método não devolve nada.
//! - `postMessage` é `async` no original; aqui devolve a [`Promise`] do proxy (ou `false` já resolvida, com o webview
//!   descartado), o que é o mesmo resultado para quem aguarda.
//! - `#html`, `#options` e os demais campos privados (`#`) são campos privados do struct. `shouldTryRewritingOldResourceUris`,
//!   `serializeWebviewOptions` e `getDefaultLocalResourceRoots` são as funções livres de mesmo nome.
//! - As três expressões regulares de `vscode-resource` rodam sobre o [`zyarr::RegExp`], por unidades UTF-16.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::{Rc, Weak};

use je_base::buffer::VSBuffer;
use je_base::dynamic::Dynamic;
use je_base::errors::{self, Error};
use je_base::event::{Emitter, Event};
use je_base::js_regexp::replace_all;
use je_base::js_string::JsString;
use je_base::lifecycle::{Disposable, DisposableRef, IDisposable};
use je_base::network::schemas;
use je_base::promise::Promise;
use je_base::uri::{URI, UriComponents, decode_uri_component};
use je_extensions::scanner::ExtensionDescription;
use je_extensions::validator::{normalize_version, parse_version};
use je_platform::log::{LogServiceRef, Logger};

use super::ext_host_language_features_api::IExtHostApiDeprecationService;
use super::ext_host_protocol::{
    ExtHostWebviewsShape, IMainContext, MainContext, MainThreadWebviewsShape, SerializableObjectWithBuffers, WebviewContentOptions,
    WebviewExtensionDescription,
};
use super::ext_host_webview_messaging::{deserialize_webview_message, serialize_webview_message};
use super::ext_host_workspace::{ExtHostWorkspace, WorkspaceFolder};
use crate::contrib::webview::webview_common::{WEBVIEW_GENERIC_CSP_SOURCE, WebviewRemoteInfo, as_webview_uri};

/// `vscode.WebviewOptions`: os mesmos campos de `IWebviewContentOptions` (ver a nota do módulo).
pub type WebviewOptions = WebviewContentOptions;

/// `Pick<IExtHostWorkspace, 'getWorkspaceFolders'>`.
pub trait IExtHostWorkspaceFolders {
    /// `getWorkspaceFolders()`.
    fn get_workspace_folders(&self) -> Option<Vec<Rc<WorkspaceFolder>>>;
}

impl IExtHostWorkspaceFolders for ExtHostWorkspace {
    fn get_workspace_folders(&self) -> Option<Vec<Rc<WorkspaceFolder>>> {
        ExtHostWorkspace::get_workspace_folders(self)
    }
}

/// `extension.extensionLocation`.
fn extension_location(extension: &ExtensionDescription) -> URI {
    URI::parse(extension.extension_location.as_str(), false)
}

/// `/(["'])vscode-resource:([^\s'"]+?)(["'])/i.test(value)`.
fn has_old_resource_uri(value: &str) -> bool {
    let regexp = zyarr::RegExp::from_str(r#"(["'])vscode-resource:([^\s'"]+?)(["'])"#, "i").expect("o padrão fixo é válido");
    let units: Vec<u16> = value.encode_utf16().collect();
    regexp.exec(&units, 0).is_some()
}

pub struct ExtHostWebview {
    handle: String,
    proxy: Rc<dyn MainThreadWebviewsShape>,

    remote_info: WebviewRemoteInfo,
    workspace: Option<Rc<dyn IExtHostWorkspaceFolders>>,
    extension: ExtensionDescription,
    deprecation_service: Rc<dyn IExtHostApiDeprecationService>,

    html: RefCell<String>,
    options: RefCell<WebviewOptions>,
    is_disposed: Cell<bool>,
    has_called_as_webview_uri: Cell<bool>,

    serialize_buffers_for_post_message: bool,
    should_rewrite_old_resource_uris: bool,

    on_message_emitter: Rc<Emitter<Dynamic>>,
    on_did_dispose_emitter: Rc<Emitter<()>>,
}

impl ExtHostWebview {
    pub fn new(
        handle: &str,
        proxy: Rc<dyn MainThreadWebviewsShape>,
        options: WebviewOptions,
        remote_info: WebviewRemoteInfo,
        workspace: Option<Rc<dyn IExtHostWorkspaceFolders>>,
        extension: &ExtensionDescription,
        deprecation_service: Rc<dyn IExtHostApiDeprecationService>,
    ) -> Rc<Self> {
        Rc::new(Self {
            handle: handle.to_owned(),
            proxy,
            remote_info,
            workspace,
            serialize_buffers_for_post_message: should_serialize_buffers_for_post_message(extension),
            should_rewrite_old_resource_uris: should_try_rewriting_old_resource_uris(extension),
            extension: extension.clone(),
            deprecation_service,
            html: RefCell::new(String::new()),
            options: RefCell::new(options),
            is_disposed: Cell::new(false),
            has_called_as_webview_uri: Cell::new(false),
            on_message_emitter: Emitter::new(),
            on_did_dispose_emitter: Emitter::new(),
        })
    }

    /// `onDidReceiveMessage`.
    pub fn on_did_receive_message(&self) -> Event<Dynamic> {
        self.on_message_emitter.event()
    }

    /// `_onMessageEmitter.fire(message)` (interno: o `ExtHostWebviews` entrega a mensagem da thread principal).
    pub fn fire_message(&self, message: Dynamic) {
        self.on_message_emitter.fire(message);
    }

    /// `_onDidDispose` (interno).
    pub fn on_did_dispose(&self) -> Event<()> {
        self.on_did_dispose_emitter.event()
    }

    /// `asWebviewUri(resource)`.
    pub fn as_webview_uri(&self, resource: &URI) -> URI {
        self.has_called_as_webview_uri.set(true);
        as_webview_uri(resource, Some(&self.remote_info))
    }

    /// `cspSource`.
    pub fn csp_source(&self) -> String {
        let extension_location = extension_location(&self.extension);
        if extension_location.scheme() == schemas::HTTPS || extension_location.scheme() == schemas::HTTP {
            // The extension is being served up from a CDN.
            // Also include the CDN in the default csp.
            let mut extension_csp_rule = extension_location.to_string(false).to_string_lossy();
            if !extension_csp_rule.ends_with('/') {
                // Always treat the location as a directory so that we allow all content under it
                extension_csp_rule.push('/');
            }
            return format!("{extension_csp_rule} {WEBVIEW_GENERIC_CSP_SOURCE}");
        }
        WEBVIEW_GENERIC_CSP_SOURCE.to_owned()
    }

    /// `html` (getter).
    pub fn html(&self) -> Result<String, Error> {
        self.assert_not_disposed()?;
        Ok(self.html.borrow().clone())
    }

    /// `html` (setter).
    pub fn set_html(&self, value: &str) -> Result<(), Error> {
        self.assert_not_disposed()?;
        if *self.html.borrow() != value {
            *self.html.borrow_mut() = value.to_owned();
            if self.should_rewrite_old_resource_uris && !self.has_called_as_webview_uri.get() && has_old_resource_uri(value) {
                self.has_called_as_webview_uri.set(true);
                self.deprecation_service.report(
                    "Webview vscode-resource: uris",
                    &self.extension,
                    "Please migrate to use the 'webview.asWebviewUri' api instead: https://aka.ms/vscode-webview-use-aswebviewuri",
                );
            }
            let rewritten = self.rewrite_old_resource_urls_if_needed(value)?;
            self.proxy.rpc_set_html(&self.handle, &rewritten);
        }
        Ok(())
    }

    /// `options` (getter).
    pub fn options(&self) -> Result<WebviewOptions, Error> {
        self.assert_not_disposed()?;
        Ok(self.options.borrow().clone())
    }

    /// `options` (setter).
    pub fn set_options(&self, new_options: WebviewOptions) -> Result<(), Error> {
        self.assert_not_disposed()?;

        if *self.options.borrow() != new_options {
            self.proxy.rpc_set_options(&self.handle, serialize_webview_options(&self.extension, self.workspace.as_deref(), &new_options));
        }

        *self.options.borrow_mut() = new_options;
        Ok(())
    }

    /// `postMessage(message)`.
    pub fn post_message(&self, message: &Dynamic) -> Promise<bool> {
        if self.is_disposed.get() {
            return Promise::resolved(false);
        }
        let serialized = serialize_webview_message(message, self.serialize_buffers_for_post_message);
        self.proxy.rpc_post_message(&self.handle, &serialized.message, serialized.buffers)
    }

    fn assert_not_disposed(&self) -> Result<(), Error> {
        if self.is_disposed.get() {
            return Err(Error::new("Webview is disposed"));
        }
        Ok(())
    }

    fn rewrite_old_resource_urls_if_needed(&self, value: &str) -> Result<String, Error> {
        if !self.should_rewrite_old_resource_uris {
            return Ok(value.to_owned());
        }

        let location = extension_location(&self.extension);
        let is_remote = location.scheme() == schemas::VSCODE_REMOTE;
        let remote_authority = is_remote.then(|| location.authority().to_string_lossy());
        let remote_info = WebviewRemoteInfo { is_remote, authority: remote_authority };

        let failure: RefCell<Option<Error>> = RefCell::new(None);
        let rewrite = |regexp: &str, input: &JsString| -> JsString {
            let regexp = zyarr::RegExp::from_str(regexp, "gi").expect("o padrão fixo é válido");
            replace_all(&regexp, input.units(), |captures| {
                let start_quote = captures.group(1).unwrap_or_default();
                let scheme = captures.group(3).filter(|scheme| !scheme.is_empty());
                let path = captures.group(4).unwrap_or_default();
                let end_quote = captures.group(5).unwrap_or_default();
                let decoded_path = match decode_uri_component(path) {
                    Ok(decoded_path) => decoded_path,
                    Err(error) => {
                        failure.borrow_mut().get_or_insert(error);
                        return JsString::from(captures.whole());
                    }
                };
                let uri = URI::from(
                    &UriComponents {
                        scheme: Some(scheme.map_or_else(|| JsString::from("file"), JsString::from)),
                        path: Some(JsString::from_units(decoded_path)),
                        ..UriComponents::default()
                    },
                    false,
                );
                let webview_uri = as_webview_uri(&uri, Some(&remote_info)).to_string(false);
                let mut replacement = JsString::from(start_quote);
                replacement.push_units(webview_uri.units());
                replacement.push_units(end_quote);
                replacement
            })
        };

        let input = JsString::from(value);
        let rewritten = rewrite(r#"(["'])(?:vscode-resource):(\/\/([^\s\/'"]+?)(?=\/))?([^\s'"]+?)(["'])"#, &input);
        let rewritten = rewrite(r#"(["'])(?:vscode-webview-resource):(\/\/[^\s\/'"]+\/([^\s\/'"]+?)(?=\/))?([^\s'"]+?)(["'])"#, &rewritten);
        if let Some(error) = failure.into_inner() {
            return Err(error);
        }
        Ok(rewritten.to_string_lossy())
    }
}

impl IDisposable for ExtHostWebview {
    fn dispose(&self) {
        self.is_disposed.set(true);

        self.on_did_dispose_emitter.fire(());

        self.on_did_dispose_emitter.dispose();
        self.on_message_emitter.dispose();
    }
}

/// `shouldSerializeBuffersForPostMessage(extension)`: a extensão declara `engines.vscode >= 1.57`.
pub fn should_serialize_buffers_for_post_message(extension: &ExtensionDescription) -> bool {
    let parsed = extension.manifest.engines.vscode.as_deref().and_then(parse_version);
    let version = normalize_version(parsed.as_ref());
    version.is_some_and(|version| version.major_base >= 1 && version.minor_base >= 57)
}

/// `shouldTryRewritingOldResourceUris(extension)`: a extensão declara `engines.vscode < 1.60`.
fn should_try_rewriting_old_resource_uris(extension: &ExtensionDescription) -> bool {
    let parsed = extension.manifest.engines.vscode.as_deref().and_then(parse_version);
    let Some(version) = normalize_version(parsed.as_ref()) else {
        return false;
    };

    version.major_base < 1 || (version.major_base == 1 && version.minor_base < 60)
}

/// `toExtensionData(extension)`.
pub fn to_extension_data(extension: &ExtensionDescription) -> WebviewExtensionDescription {
    WebviewExtensionDescription { id: extension.identifier.clone(), location: extension_location(extension) }
}

/// `serializeWebviewOptions(extension, workspace, options)`.
pub fn serialize_webview_options(extension: &ExtensionDescription, workspace: Option<&dyn IExtHostWorkspaceFolders>, options: &WebviewOptions) -> WebviewContentOptions {
    WebviewContentOptions {
        enable_command_uris: options.enable_command_uris.clone(),
        enable_scripts: options.enable_scripts,
        enable_forms: options.enable_forms,
        port_mapping: options.port_mapping.clone(),
        local_resource_roots: Some(options.local_resource_roots.clone().unwrap_or_else(|| get_default_local_resource_roots(extension, workspace))),
    }
}

/// `getDefaultLocalResourceRoots(extension, workspace)`: as pastas do workspace e a pasta de instalação da extensão.
fn get_default_local_resource_roots(extension: &ExtensionDescription, workspace: Option<&dyn IExtHostWorkspaceFolders>) -> Vec<URI> {
    let mut roots: Vec<URI> = workspace.and_then(|workspace| workspace.get_workspace_folders()).unwrap_or_default().iter().map(|folder| folder.uri().clone()).collect();
    roots.push(extension_location(extension));
    roots
}

pub struct ExtHostWebviews {
    this: Weak<ExtHostWebviews>,
    base: Rc<Disposable>,

    webview_proxy: Rc<dyn MainThreadWebviewsShape>,

    webviews: RefCell<HashMap<String, Rc<ExtHostWebview>>>,

    remote_info: WebviewRemoteInfo,
    workspace: Option<Rc<dyn IExtHostWorkspaceFolders>>,
    log_service: LogServiceRef,
    deprecation_service: Rc<dyn IExtHostApiDeprecationService>,
}

impl ExtHostWebviews {
    pub fn new(
        main_context: &dyn IMainContext,
        remote_info: WebviewRemoteInfo,
        workspace: Option<Rc<dyn IExtHostWorkspaceFolders>>,
        log_service: LogServiceRef,
        deprecation_service: Rc<dyn IExtHostApiDeprecationService>,
    ) -> Rc<Self> {
        let webview_proxy = main_context.get_proxy(&MainContext::MAIN_THREAD_WEBVIEWS);
        Rc::new_cyclic(|this| Self {
            this: this.clone(),
            base: Disposable::new(),
            webview_proxy,
            webviews: RefCell::new(HashMap::new()),
            remote_info,
            workspace,
            log_service,
            deprecation_service,
        })
    }

    /// `createNewWebview(handle, options, extension)`.
    pub fn create_new_webview(&self, handle: &str, options: &WebviewContentOptions, extension: &ExtensionDescription) -> Rc<ExtHostWebview> {
        let webview = ExtHostWebview::new(
            handle,
            self.webview_proxy.clone(),
            options.clone(),
            self.remote_info.clone(),
            self.workspace.clone(),
            extension,
            self.deprecation_service.clone(),
        );
        self.webviews.borrow_mut().insert(handle.to_owned(), webview.clone());

        let subscription: Rc<RefCell<Option<DisposableRef>>> = Rc::default();
        let listener_subscription = subscription.clone();
        let this = self.this.clone();
        let handle = handle.to_owned();
        let listener = webview.on_did_dispose().listen(move |_| {
            let subscription = listener_subscription.borrow_mut().take();
            if let Some(subscription) = subscription {
                subscription.dispose();
            }
            if let Some(this) = this.upgrade() {
                this.delete_webview(&handle);
            }
        });
        *subscription.borrow_mut() = Some(listener);

        webview
    }

    /// Ensures that the main thread side of the webview has `localResourceRoots`
    /// populated when the caller did not supply any.
    ///
    /// This honors the documented `WebviewOptions.localResourceRoots` contract
    /// ("Default to ... the workspace folders and the extension's install
    /// directory") for code paths that construct the webview's content options
    /// outside of the extension host (e.g. custom editors), where the main
    /// thread starts with empty content options.
    pub fn ensure_default_content_options(&self, handle: &str, content_options: &WebviewContentOptions, extension: &ExtensionDescription) {
        if content_options.local_resource_roots.is_some() {
            return;
        }
        self.webview_proxy.rpc_set_options(
            handle,
            WebviewContentOptions {
                local_resource_roots: Some(get_default_local_resource_roots(extension, self.workspace.as_deref())),
                ..content_options.clone()
            },
        );
    }

    /// `deleteWebview(handle)`.
    pub fn delete_webview(&self, handle: &str) {
        self.webviews.borrow_mut().remove(handle);
    }

    fn get_webview(&self, handle: &str) -> Option<Rc<ExtHostWebview>> {
        self.webviews.borrow().get(handle).cloned()
    }
}

impl IDisposable for ExtHostWebviews {
    fn dispose(&self) {
        self.base.dispose();

        let webviews: Vec<Rc<ExtHostWebview>> = self.webviews.borrow().values().cloned().collect();
        for webview in webviews {
            webview.dispose();
        }
        self.webviews.borrow_mut().clear();
    }
}

impl ExtHostWebviewsShape for ExtHostWebviews {
    fn rpc_on_message(&self, handle: &str, json_message: &str, buffers: SerializableObjectWithBuffers<Vec<VSBuffer>>) {
        let Some(webview) = self.get_webview(handle) else {
            return;
        };
        match deserialize_webview_message(json_message, &buffers.value) {
            Ok(deserialized) => webview.fire_message(deserialized.message),
            Err(error) => errors::on_unexpected_error(&error),
        }
    }

    fn rpc_on_missing_csp(&self, _handle: &str, extension_id: &str) {
        self.log_service.warn(&format!("{extension_id} created a webview without a content security policy: https://aka.ms/vscode-webview-missing-csp"));
    }
}
