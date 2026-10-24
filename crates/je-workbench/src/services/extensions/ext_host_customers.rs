//! Porte de `src/vs/workbench/services/extensions/common/extHostCustomers.ts`: o registro dos "clientes" do host de extensões (as
//! classes `MainThread*` que o `ExtensionHostManager` instancia para cada host e liga ao RPC), com o `IExtHostContext` que eles
//! recebem.
//!
//! Divergências:
//! - **Decoradores.** `@extHostNamedCustomer(id)` e `@extHostCustomer` são as funções [`ext_host_named_customer`] e
//!   [`ext_host_customer`], chamadas na inicialização por quem fornece o cliente. O registro (`ExtHostCustomersRegistryImpl.INSTANCE`)
//!   é um `thread_local`, como o resto do porte.
//! - **Construtor por injeção de dependências.** O `IConstructorSignature<T, [IExtHostContext]>` instanciado por
//!   `instantiationService.createInstance(ctor, extHostContext)` é um fecho que recebe o contexto e captura por conta própria os
//!   serviços de que precisa. Ele pode falhar (`Err`), como o construtor que lança.
//! - **`IExtHostContext` sem forma tipada no RPC.** Nenhum serviço do protocolo tem `RpcProtocol::register_shape`, então o objeto
//!   registrado por um cliente nomeado é uma das duas ligações de [`CustomerBinding`]: a forma tipada (`set_erased`) ou o ator que
//!   despacha pelo nome do método ([`IRpcActor`]). O contexto expõe o [`RpcProtocol`] ([`IExtHostContext::rpc_protocol`]) para quem
//!   pede proxies por nome (`getProxy(identifier)` sem tipo).
//! - `_setAllMainProxyIdentifiers(ProxyIdentifier[])` recebe os `sid`.

use std::any::Any;
use std::cell::RefCell;
use std::rc::Rc;

use je_base::errors::Error;
use je_base::lifecycle::IDisposable;

use super::extension_host_kind::ExtensionHostKind;
use super::extension_host_proxy::IExtensionHostProxy;
use super::extensions::IInternalExtensionService;
use super::rpc_protocol::{IRPCProtocol, IRpcActor, RpcProtocol};

/// `IExtHostContext`.
pub trait IExtHostContext: IRPCProtocol {
    fn remote_authority(&self) -> Option<String>;
    fn extension_host_kind(&self) -> ExtensionHostKind;
    /// O protocolo por baixo (ver a nota do módulo).
    fn rpc_protocol(&self) -> Rc<RpcProtocol>;
}

/// `IInternalExtHostContext`.
pub trait IInternalExtHostContext: IExtHostContext {
    fn internal_extension_service(&self) -> Rc<dyn IInternalExtensionService>;
    fn set_extension_host_proxy(&self, extension_host_proxy: Rc<dyn IExtensionHostProxy>);
    fn set_all_main_proxy_identifiers(&self, main_proxy_identifiers: Vec<&'static str>);
}

/// O que o cliente nomeado entrega ao `rpcProtocol.set(id, instance)`.
#[derive(Clone)]
pub enum CustomerBinding {
    /// Um `Rc<T>` guardado como `Rc<dyn Any>`, para o `set_erased` (exige `register_shape` do identificador).
    Shape(Rc<dyn Any>),
    /// Um ator que despacha por nome de método.
    Actor(Rc<dyn IRpcActor>),
}

/// A instância que o construtor de um cliente nomeado devolve: o descartável que o gerenciador guarda em `_customers` e a ligação
/// com o RPC.
#[derive(Clone)]
pub struct NamedCustomerInstance {
    pub disposable: Rc<dyn IDisposable>,
    pub binding: CustomerBinding,
}

/// `IExtHostCustomerCtor<IDisposable>`.
pub type ExtHostCustomerCtor = Rc<dyn Fn(&Rc<dyn IInternalExtHostContext>) -> Result<Rc<dyn IDisposable>, Error>>;

/// `IExtHostCustomerCtor<T>` de um cliente nomeado.
pub type ExtHostNamedCustomerCtor = Rc<dyn Fn(&Rc<dyn IInternalExtHostContext>) -> Result<NamedCustomerInstance, Error>>;

/// `IExtHostNamedCustomer<IDisposable>`: `[ProxyIdentifier, ctor]`, com o `sid` do identificador.
#[derive(Clone)]
pub struct ExtHostNamedCustomer {
    pub id: &'static str,
    pub ctor: ExtHostNamedCustomerCtor,
}

#[derive(Default)]
struct ExtHostCustomersRegistryImpl {
    named_customers: RefCell<Vec<ExtHostNamedCustomer>>,
    customers: RefCell<Vec<ExtHostCustomerCtor>>,
}

thread_local! {
    static INSTANCE: ExtHostCustomersRegistryImpl = ExtHostCustomersRegistryImpl::default();
}

/// `extHostNamedCustomer(id)(ctor)`.
pub fn ext_host_named_customer(id: &'static str, ctor: ExtHostNamedCustomerCtor) {
    INSTANCE.with(|registry| registry.named_customers.borrow_mut().push(ExtHostNamedCustomer { id, ctor }));
}

/// `extHostCustomer(ctor)`.
pub fn ext_host_customer(ctor: ExtHostCustomerCtor) {
    INSTANCE.with(|registry| registry.customers.borrow_mut().push(ctor));
}

/// `ExtHostCustomersRegistry`.
pub struct ExtHostCustomersRegistry;

impl ExtHostCustomersRegistry {
    pub fn get_named_customers() -> Vec<ExtHostNamedCustomer> {
        INSTANCE.with(|registry| registry.named_customers.borrow().clone())
    }

    pub fn get_customers() -> Vec<ExtHostCustomerCtor> {
        INSTANCE.with(|registry| registry.customers.borrow().clone())
    }
}
