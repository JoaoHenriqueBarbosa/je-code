//! O `IMainContext` em processo: os dois lados do RPC (o host de extensões e o workbench) são Rust no mesmo processo, então o
//! "proxy" de um serviço é o próprio objeto registrado, sem serialização. É o que o `RPCProtocol` do original faz com o
//! transporte tirado.

use std::any::Any;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use je_workbench::api::common::ext_host_protocol::IMainContext;

/// O registro de proxies em processo.
#[derive(Default)]
pub struct LocalContext {
    proxies: RefCell<HashMap<&'static str, Rc<dyn Any>>>,
}

impl LocalContext {
    pub fn new() -> Rc<Self> {
        Rc::new(Self::default())
    }
}

impl IMainContext for LocalContext {
    fn get_proxy_erased(&self, sid: &'static str) -> Rc<dyn Any> {
        self.proxies.borrow().get(sid).cloned().unwrap_or_else(|| panic!("o proxy '{sid}' não foi registrado no contexto"))
    }

    fn set_erased(&self, sid: &'static str, value: Rc<dyn Any>) {
        self.proxies.borrow_mut().insert(sid, value);
    }
}
