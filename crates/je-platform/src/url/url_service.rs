//! Porte de `src/vs/platform/url/common/url.ts` (`IURLService`, `IURLHandler`) e `urlService.ts`
//! (`AbstractURLService`, `NativeURLService`).
//!
//! Fora do porte (e por quê): `urlIpc.ts` (os canais de IPC entre processos do Electron; o IPC é
//! `je_base::parts::ipc`) e `electron-main/*` (o `app.setAsDefaultProtocolClient` e o `open-url` do
//! Electron, que são do sistema operacional do desktop).

use std::cell::RefCell;
use std::rc::Rc;

use je_base::JsString;
use je_base::async_::{PromiseFactory, first};
use je_base::lifecycle::{DisposableRef, to_disposable};
use je_base::promise::Promise;
use je_base::uri::{URI, UriComponents};
use je_instantiation::service_id;

use crate::product::IProductService;

/// `IOpenURLOptions`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct OpenUrlOptions {
    /// Quando ausente ou `false`, indica que a URL não nasceu no produto, e por isso um diálogo de
    /// confirmação pode aparecer.
    pub trusted: Option<bool>,
    pub original_url: Option<JsString>,
}

/// `IURLHandler`.
pub trait IUrlHandler {
    fn handle_url(&self, uri: &URI, options: Option<&OpenUrlOptions>) -> Promise<bool>;
}

/// `IURLService`.
pub trait IUrlService {
    /// Cria uma URL que, ao ser chamada, aciona os `IURLHandler`s.
    fn create(&self, options: Option<&UriComponents>) -> URI;
    fn open(&self, url: &URI, options: Option<OpenUrlOptions>) -> Promise<bool>;
    fn register_handler(&self, handler: Rc<dyn IUrlHandler>) -> DisposableRef;
}

service_id!(pub I_URL_SERVICE: dyn IUrlService = "urlService");

/// O estado de `AbstractURLService`: o conjunto de tratadores, na ordem de registro.
#[derive(Default)]
pub struct UrlHandlers {
    handlers: Rc<RefCell<Vec<Rc<dyn IUrlHandler>>>>,
}

impl UrlHandlers {
    /// `open`: o primeiro tratador que devolve `true` (os tratadores rodam em sequência).
    pub fn open(&self, uri: &URI, options: Option<OpenUrlOptions>) -> Promise<bool> {
        let handlers: Vec<Rc<dyn IUrlHandler>> = self.handlers.borrow().clone();
        let factories: Vec<PromiseFactory<bool>> = handlers
            .into_iter()
            .map(|handler| {
                let uri = uri.clone();
                let options = options.clone();
                Box::new(move || handler.handle_url(&uri, options.as_ref())) as PromiseFactory<bool>
            })
            .collect();
        first(factories, |value| *value, Some(false)).then(|value| value.unwrap_or(false))
    }

    /// `registerHandler`. O `Set` do original não duplica: registrar o mesmo tratador duas vezes é uma só.
    pub fn register_handler(&self, handler: Rc<dyn IUrlHandler>) -> DisposableRef {
        {
            let mut handlers = self.handlers.borrow_mut();
            if !handlers.iter().any(|h| Rc::ptr_eq(h, &handler)) {
                handlers.push(handler.clone());
            }
        }
        let handlers = self.handlers.clone();
        to_disposable(move || handlers.borrow_mut().retain(|h| !Rc::ptr_eq(h, &handler)))
    }
}

/// `NativeURLService`.
pub struct NativeUrlService {
    state: UrlHandlers,
    product_service: Rc<dyn IProductService>,
}

impl NativeUrlService {
    pub fn new(product_service: Rc<dyn IProductService>) -> Self {
        Self { state: UrlHandlers::default(), product_service }
    }
}

impl IUrlService for NativeUrlService {
    fn create(&self, options: Option<&UriComponents>) -> URI {
        let default = UriComponents::default();
        let options = options.unwrap_or(&default);
        let authority = options.authority.clone();
        let mut path = options.path.clone();
        if let (Some(authority), Some(path)) = (&authority, &mut path)
            && !authority.is_empty()
            && !path.is_empty()
            && path.units()[0] != b'/' as u16
        {
            // a validação de URI exige um caminho quando há autoridade
            let mut with_slash = JsString::from("/");
            with_slash.push_units(path.units());
            *path = with_slash;
        }
        URI::from(
            &UriComponents {
                scheme: Some(JsString::from(self.product_service.url_protocol())),
                authority,
                path,
                query: options.query.clone(),
                fragment: options.fragment.clone(),
            },
            false,
        )
    }

    fn open(&self, url: &URI, options: Option<OpenUrlOptions>) -> Promise<bool> {
        self.state.open(url, options)
    }

    fn register_handler(&self, handler: Rc<dyn IUrlHandler>) -> DisposableRef {
        self.state.register_handler(handler)
    }
}
