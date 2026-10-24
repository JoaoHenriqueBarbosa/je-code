//! Porte de `src/vs/workbench/contrib/webview/browser/webview.ts`: os tipos lógicos de um webview (opções, descrição da
//! extensão, o contrato [`IWebview`] e [`IOverlayWebview`]) e as lojas de origem (`WebviewOriginStore`).
//!
//! Divergências:
//! - O webview em si (o elemento DOM, o foco, a busca, a área de transferência, o `claim` de um `CodeWindow`, a âncora e o
//!   `IObservable` do tamanho do conteúdo) é recorte da Fase 6: [`IWebview`] e [`IOverlayWebview`] trazem só os membros
//!   lógicos que o workbench lê e escreve (`providedViewType`, `origin`, `setHtml`, `setTitle`, `contentOptions`,
//!   `localResourcesRoot`, `extension`, `initialScrollProgress`, `state`, `options`, os eventos e `postMessage`).
//!   `IWebviewService` (que cria os elementos e as sobreposições) também fica de fora: nada neste porte o chama.
//! - Os campos `readonly` de `WebviewContentOptions`, `WebviewOptions` e `WebviewInitInfo` são campos públicos, e cada
//!   campo opcional é `Option`. `WebviewOptions.transformCssVariables?` é um `Rc<dyn Fn>` sobre [`WebviewStyles`] (uma lista
//!   de pares, na ordem de inserção), por isso [`WebviewOptions`] não tem igualdade.
//! - `WebviewContentPurpose` (um `const enum` de textos) é o enum de mesmo nome, com [`WebviewContentPurpose::as_str`].
//! - `enableCommandUris?: boolean | readonly string[]` e `portMapping` são os tipos do protocolo ([`EnableCommandUris`] e
//!   [`WebviewPortMapping`], que têm os mesmos campos de `IWebviewPortMapping`).
//! - `WebviewMessageReceivedEvent.transfer` (`readonly ArrayBuffer[]`) é uma lista de buffers de bytes.
//! - `WebviewOriginStore` e `ExtensionKeyedWebviewOriginStore` recebem o [`StorageService`] direto (o `@IStorageService`).

use std::rc::Rc;

use je_base::dynamic::{Dynamic, ObjectRef, json_stringify};
use je_base::event::Event;
use je_base::js_string::JsString;
use je_base::lifecycle::IDisposable;
use je_base::promise::Promise;
use je_base::uri::URI;
use je_base::uuid::generate_uuid;
use je_extensions::manifest::ExtensionIdentifier;
use je_platform::resources::ext_uri;
use je_platform::storage::{StorageScope, StorageService, StorageTarget};

use crate::api::common::ext_host_protocol::{EnableCommandUris, WebviewPortMapping};
use crate::common::memento::Memento;

/// `WebviewContentPurpose`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WebviewContentPurpose {
    NotebookRenderer,
    CustomEditor,
    WebviewView,
    ChatOutputItem,
}

impl WebviewContentPurpose {
    /// O texto do `const enum` do original.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::NotebookRenderer => "notebookRenderer",
            Self::CustomEditor => "customEditor",
            Self::WebviewView => "webviewView",
            Self::ChatOutputItem => "chatOutputItem",
        }
    }
}

/// `WebviewStyles`: `{ readonly [key: string]: string | number }`, na ordem de inserção.
pub type WebviewStyles = Vec<(String, WebviewStyleValue)>;

/// O `string | number` de um valor de `WebviewStyles`.
#[derive(Clone, Debug, PartialEq)]
pub enum WebviewStyleValue {
    Text(String),
    Number(f64),
}

/// `WebviewOptions`.
#[derive(Clone, Default)]
pub struct WebviewOptions {
    /// Metadata indicating the purpose of the webview.
    pub purpose: Option<WebviewContentPurpose>,
    /// Additional classes to add to the webview's container element.
    pub custom_classes: Option<String>,
    pub enable_find_widget: Option<bool>,
    /// Disable the service worker used for loading local resources in the webview.
    pub disable_service_worker: Option<bool>,
    /// Try to restore the scroll position of the webview after a reload.
    pub try_restore_scroll_position: Option<bool>,
    pub retain_context_when_hidden: Option<bool>,
    /// `transformCssVariables?(styles)`.
    pub transform_css_variables: Option<Rc<dyn Fn(&WebviewStyles) -> WebviewStyles>>,
}

/// `WebviewContentOptions`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct WebviewContentOptions {
    /// Should the webview allow `acquireVsCodeApi` to be called multiple times? Defaults to false.
    pub allow_multiple_api_acquire: Option<bool>,
    /// Should scripts be enabled in the webview? Defaults to false.
    pub allow_scripts: Option<bool>,
    /// Should forms be enabled in the webview? Defaults to the value of `allowScripts`.
    pub allow_forms: Option<bool>,
    /// Should the webview forward untrusted keypress events to the host? (always true for extensions).
    pub forward_untrusted_keypress_events: Option<bool>,
    /// Set of root paths from which the webview can load local resources.
    pub local_resource_roots: Option<Vec<URI>>,
    /// Mappings of localhost ports used inside the webview.
    pub port_mapping: Option<Vec<WebviewPortMapping>>,
    /// Are command uris enabled in the webview? `true` enables all, a list enables only those commands.
    pub enable_command_uris: Option<EnableCommandUris>,
}

/// `WebviewExtensionDescription`.
#[derive(Clone, Debug, PartialEq)]
pub struct WebviewExtensionDescription {
    pub location: Option<URI>,
    pub id: ExtensionIdentifier,
}

/// `WebviewInitInfo`.
#[derive(Clone, Default)]
pub struct WebviewInitInfo {
    pub provided_view_type: Option<String>,
    pub origin: Option<String>,
    pub title: Option<String>,
    pub options: WebviewOptions,
    pub content_options: WebviewContentOptions,
    pub extension: Option<WebviewExtensionDescription>,
}

/// Check if two [`WebviewContentOptions`] are equal (`areWebviewContentOptionsEqual`).
pub fn are_webview_content_options_equal(a: &WebviewContentOptions, b: &WebviewContentOptions) -> bool {
    a.allow_multiple_api_acquire == b.allow_multiple_api_acquire
        && a.allow_scripts == b.allow_scripts
        && a.allow_forms == b.allow_forms
        && a.forward_untrusted_keypress_events == b.forward_untrusted_keypress_events
        && optional_arrays_equal(&a.local_resource_roots, &b.local_resource_roots, |a, b| ext_uri().is_equal(Some(a), Some(b), false))
        && optional_arrays_equal(&a.port_mapping, &b.port_mapping, |a, b| a.extension_host_port == b.extension_host_port && a.webview_port == b.webview_port)
        && are_enable_command_uris_equal(a, b)
}

/// `equals(a, b, itemEquals)` de `arrays.ts`: dois `undefined` são iguais, um só é diferente, e as listas comparam
/// item a item.
fn optional_arrays_equal<T>(a: &Option<Vec<T>>, b: &Option<Vec<T>>, item_equals: impl Fn(&T, &T) -> bool) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(a), Some(b)) => a.len() == b.len() && a.iter().zip(b).all(|(a, b)| item_equals(a, b)),
        _ => false,
    }
}

fn are_enable_command_uris_equal(a: &WebviewContentOptions, b: &WebviewContentOptions) -> bool {
    match (&a.enable_command_uris, &b.enable_command_uris) {
        (None, None) => true,
        (Some(EnableCommandUris::All(a)), Some(EnableCommandUris::All(b))) => a == b,
        (Some(EnableCommandUris::Commands(a)), Some(EnableCommandUris::Commands(b))) => a == b,
        _ => false,
    }
}

/// `WebviewMessageReceivedEvent`.
#[derive(Clone, Debug)]
pub struct WebviewMessageReceivedEvent {
    pub message: Dynamic,
    pub transfer: Option<Vec<Vec<u8>>>,
}

/// `IWebview` (ver a nota do módulo).
pub trait IWebview: IDisposable {
    /// The original view type of the webview.
    fn provided_view_type(&self) -> Option<String>;

    /// The origin this webview itself is loaded from. May not be unique.
    fn origin(&self) -> String;

    /// Set html content.
    fn set_html(&self, html: &str);

    /// Set the title of the webview. This is used for accessibility and does not show up visually.
    fn set_title(&self, title: &str);

    /// Control what content is allowed/blocked inside the webview (`contentOptions`).
    fn content_options(&self) -> WebviewContentOptions;

    /// `contentOptions` (setter).
    fn set_content_options(&self, options: WebviewContentOptions);

    /// The extension that created the webview (`extension`).
    fn extension(&self) -> Option<WebviewExtensionDescription>;

    /// `extension` (setter).
    fn set_extension(&self, extension: Option<WebviewExtensionDescription>);

    /// `state`: the serialized state of the webview.
    fn state(&self) -> Option<String>;

    /// `state` (setter).
    fn set_state(&self, state: Option<String>);

    /// `onDidDispose`.
    fn on_did_dispose(&self) -> Event<()>;

    /// `onDidClickLink`.
    fn on_did_click_link(&self) -> Event<String>;

    /// `onMissingCsp`.
    fn on_missing_csp(&self) -> Event<ExtensionIdentifier>;

    /// `onMessage`.
    fn on_message(&self) -> Event<WebviewMessageReceivedEvent>;

    /// `postMessage(message, transfer?)`.
    fn post_message(&self, message: Dynamic, transfer: Option<Vec<Vec<u8>>>) -> Promise<bool>;
}

/// `IOverlayWebview`: a sobreposição de um webview, com a origem e as opções que a thread principal ajusta.
pub trait IOverlayWebview: IWebview {
    /// `options`.
    fn options(&self) -> WebviewOptions;

    /// `options` (setter).
    fn set_options(&self, options: WebviewOptions);
}

/// Stores the unique origins for a webview.
///
/// These are randomly generated
pub struct WebviewOriginStore {
    memento: Memento,
    state: ObjectRef,
}

impl WebviewOriginStore {
    pub fn new(root_storage_key: &str, storage_service: Rc<dyn StorageService>) -> Self {
        let memento = Memento::new(root_storage_key, storage_service);
        let state = memento.get_memento(StorageScope::Application, StorageTarget::Machine);
        Self { memento, state }
    }

    /// `getOrigin(viewType, additionalKey)`.
    pub fn get_origin(&self, view_type: &str, additional_key: Option<&str>) -> String {
        let key = Self::get_key(view_type, additional_key);

        if let Some(Dynamic::String(existing)) = self.state.get(&key)
            && !existing.is_empty()
        {
            return existing.to_string_lossy();
        }

        let new_origin = generate_uuid();
        self.state.set(&key, Dynamic::string(&new_origin));
        self.memento.save_memento();
        new_origin
    }

    /// `JSON.stringify({ viewType, key: additionalKey })`.
    fn get_key(view_type: &str, additional_key: Option<&str>) -> JsString {
        let key = additional_key.map_or(Dynamic::Undefined, Dynamic::string);
        JsString::from(json_stringify(&Dynamic::object([("viewType", Dynamic::string(view_type)), ("key", key)])).unwrap_or_default())
    }
}

/// Stores the unique origins for a webview.
///
/// These are randomly generated, but keyed on extension and webview viewType.
pub struct ExtensionKeyedWebviewOriginStore {
    store: WebviewOriginStore,
}

impl ExtensionKeyedWebviewOriginStore {
    pub fn new(root_storage_key: &str, storage_service: Rc<dyn StorageService>) -> Self {
        Self { store: WebviewOriginStore::new(root_storage_key, storage_service) }
    }

    /// `getOrigin(viewType, extId)`.
    pub fn get_origin(&self, view_type: &str, ext_id: &ExtensionIdentifier) -> String {
        self.store.get_origin(view_type, Some(&ext_id.value))
    }
}
