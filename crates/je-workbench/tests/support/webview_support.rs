//! Dublês e auxiliares dos testes de webview (`extHostWebview.test.ts`, `mainThreadWebviewPanels.test.ts` e
//! `extHostCustomEditors.test.ts`).
//!
//! `SingleProxyRPCProtocol(shape)` devolve o mesmo objeto a qualquer identificador de proxy. O [`IMainContext`] do porte
//! entrega o proxy pela forma pedida (`dyn MainThreadWebviewsShape`, `dyn MainThreadWebviewPanelsShape`, ...), então o
//! [`MapMainContext`] guarda o dublê registrado em cada identificador com [`IMainContext::set`], e o
//! [`MockMainThreadWebviewManager`] (o `mock<MainThreadWebviewManager>()` do original, uma classe que implementa os
//! `MainThreadWebviewsShape` e `MainThreadWebviewPanelsShape`) é registrado nos dois.
#![allow(dead_code)]

use std::any::Any;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use je_base::buffer::VSBuffer;
use je_base::lifecycle::IDisposable;
use je_base::promise::Promise;
use je_extensions::scanner::ExtensionDescription;
use je_workbench::api::common::ext_host_language_features_api::IExtHostApiDeprecationService;
use je_workbench::api::common::ext_host_protocol::{
    IMainContext, MainContext, MainThreadWebviewPanelsShape, MainThreadWebviewsShape, WebviewContentOptions, WebviewExtensionDescription, WebviewIconPathDto,
    WebviewInitData, WebviewPanelShowOptions, WebviewSerializerOptions,
};

/// `NullApiDeprecationService`.
pub struct NullApiDeprecationService;

impl IExtHostApiDeprecationService for NullApiDeprecationService {
    fn report(&self, _api_id: &str, _extension: &ExtensionDescription, _migration_suggestion: &str) {}
}

/// O `IMainContext` em que cada proxy é o objeto registrado por `set` (o que não foi registrado falha ao ser pedido).
#[derive(Default)]
pub struct MapMainContext {
    objects: RefCell<HashMap<&'static str, Rc<dyn Any>>>,
}

impl IMainContext for MapMainContext {
    fn get_proxy_erased(&self, sid: &'static str) -> Rc<dyn Any> {
        self.objects.borrow().get(sid).cloned().unwrap_or_else(|| panic!("o proxy '{sid}' não foi registrado"))
    }

    fn set_erased(&self, sid: &'static str, value: Rc<dyn Any>) {
        self.objects.borrow_mut().insert(sid, value);
    }
}

/// O que `MockMainThreadWebviewManager` responde: os métodos que o `mock<MainThreadWebviewManager>()` do original
/// sobrescreve respondem (os `$disposeWebview`, `$createWebviewPanel`, `$registerSerializer` e `$unregisterSerializer`
/// vazios, o `$setOptions` que guarda as chamadas), e o resto falha com `panic!`, como o método ausente do objeto vazio
/// lançaria `TypeError`.
#[derive(Default)]
pub struct MockMainThreadWebviewManager {
    /// O `setOptionsCalls`: o `$setOptions` só responde quando `capture_set_options` é verdadeiro.
    pub capture_set_options: bool,
    pub set_options_calls: RefCell<Vec<(String, WebviewContentOptions)>>,
    /// Os `$disposeWebview`, `$createWebviewPanel`, `$registerSerializer` e `$unregisterSerializer` do `createNoopMainThreadWebviews`.
    pub noop_panels: bool,
}

impl MockMainThreadWebviewManager {
    /// O `createNoopMainThreadWebviews()`.
    pub fn noop() -> Rc<Self> {
        Rc::new(Self { noop_panels: true, ..Self::default() })
    }

    /// O `new class extends mock<MainThreadWebviewManager>() { $setOptions(...) { setOptionsCalls.push(...) } }`.
    pub fn capturing_set_options() -> Rc<Self> {
        Rc::new(Self { capture_set_options: true, ..Self::default() })
    }

    /// Registra o dublê nos dois identificadores que o `MainThreadWebviewManager` atende.
    pub fn register_in(self: &Rc<Self>, context: &dyn IMainContext) {
        let webviews: Rc<dyn MainThreadWebviewsShape> = self.clone();
        context.set(&MainContext::MAIN_THREAD_WEBVIEWS, webviews);
        let panels: Rc<dyn MainThreadWebviewPanelsShape> = self.clone();
        context.set(&MainContext::MAIN_THREAD_WEBVIEW_PANELS, panels);
    }
}

impl IDisposable for MockMainThreadWebviewManager {
    fn dispose(&self) {}
}

impl MainThreadWebviewsShape for MockMainThreadWebviewManager {
    fn rpc_set_html(&self, _handle: &str, _value: &str) {
        panic!("$setHtml is not a function");
    }

    fn rpc_set_options(&self, handle: &str, options: WebviewContentOptions) {
        if !self.capture_set_options {
            panic!("$setOptions is not a function");
        }
        self.set_options_calls.borrow_mut().push((handle.to_owned(), options));
    }

    fn rpc_post_message(&self, _handle: &str, _value: &str, _buffers: Vec<VSBuffer>) -> Promise<bool> {
        panic!("$postMessage is not a function");
    }
}

impl MainThreadWebviewPanelsShape for MockMainThreadWebviewManager {
    fn rpc_create_webview_panel(
        &self,
        _extension: WebviewExtensionDescription,
        _handle: &str,
        _view_type: &str,
        _init_data: WebviewInitData,
        _show_options: WebviewPanelShowOptions,
    ) {
        if !self.noop_panels {
            panic!("$createWebviewPanel is not a function");
        }
    }

    fn rpc_dispose_webview(&self, _handle: &str) {
        if !self.noop_panels {
            panic!("$disposeWebview is not a function");
        }
    }

    fn rpc_reveal(&self, _handle: &str, _show_options: WebviewPanelShowOptions) {
        panic!("$reveal is not a function");
    }

    fn rpc_set_title(&self, _handle: &str, _value: &str) {
        panic!("$setTitle is not a function");
    }

    fn rpc_set_icon_path(&self, _handle: &str, _value: Option<WebviewIconPathDto>) {
        panic!("$setIconPath is not a function");
    }

    fn rpc_register_serializer(&self, _view_type: &str, _options: WebviewSerializerOptions) {
        if !self.noop_panels {
            panic!("$registerSerializer is not a function");
        }
    }

    fn rpc_unregister_serializer(&self, _view_type: &str) {
        if !self.noop_panels {
            panic!("$unregisterSerializer is not a function");
        }
    }
}
