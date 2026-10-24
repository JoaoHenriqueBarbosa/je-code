//! Porte reduzido de `src/vs/workbench/api/browser/mainThreadExtensionService.ts`: o cliente nomeado `MainThreadExtensionService`, o
//! lado do workbench do RPC de ativação e de erros de extensões, que liga o `IExtensionHostProxy` ao `ExtensionHostManager`
//! (`_setExtensionHostProxy`).
//!
//! Divergências:
//! - **Serviços.** Só o `IExtensionService` entra (`$getExtension`). `INotificationService`, `IExtensionsWorkbenchService`,
//!   `IHostService`, `IWorkbenchExtensionEnablementService`, `ITimerService`, `ICommandService` e `IWorkbenchEnvironmentService` servem à
//!   interface de dependência ausente (`_handleMissingInstalledDependency`, `_handleMissingNotInstalledDependency`, os botões de
//!   instalar, habilitar e recarregar), que depende de partes do workbench ainda não portadas e NÃO está portada: um
//!   `$onExtensionActivationError` com dependência ausente só registra o erro de ativação no serviço, sem a notificação.
//! - `$getExtension` devolve o `ExtensionDescription` inteiro, procurado em `IExtensionService.extensions` (o `getExtension` do contrato
//!   reduzido devolve só o `extensionLocation`).
//! - `$setPerformanceMarks` não faz nada: o `ITimerService` (`setPerformanceMarks('localExtHost' | 'remoteExtHost', marks)`) não existe.
//!   `$asBrowserUri` devolve a própria URI (o `FileAccess.uriToBrowserUri` só reescreve esquemas do navegador).
//! - `_setAllMainProxyIdentifiers(Object.keys(MainContext).map(...))`: o porte não tem todos os clientes `MainThread*`, então a lista é a
//!   dos clientes nomeados registrados ([`ExtHostCustomersRegistry::get_named_customers`]): o gerenciador confere que cada um foi
//!   instanciado e ligado ao RPC (`Missing proxy instance ...` se algum falhou).
//! - O cliente é registrado por [`register_main_thread_extension_service`] (o `@extHostNamedCustomer(MainContext.MainThreadExtensionService)`);
//!   o RPC o enxerga pela forma tipada (`ext_host_rpc_shapes`), que decodifica os argumentos de cada `$método`.

use std::rc::Rc;

use je_base::errors::{SerializedError, transform_error_from_serialization};
use je_base::lifecycle::IDisposable;
use je_base::performance::PerformanceMark;
use je_base::promise::Promise;
use je_base::uri::URI;
use je_extensions::manifest::ExtensionIdentifier;
use je_extensions::scanner::ExtensionDescription;

use crate::api::common::ext_host_protocol::{MainContext, MainThreadExtensionServiceShape};
use crate::services::extensions::ext_host_customers::{CustomerBinding, ExtHostCustomersRegistry, IInternalExtHostContext, NamedCustomerInstance, ext_host_named_customer};
use crate::services::extensions::extension_activation::{ExtensionActivationReason, MissingExtensionDependency};
use crate::services::extensions::extension_host_proxy::{EXT_HOST_EXTENSION_SERVICE_SID, ExtensionHostProxy};
use crate::services::extensions::extensions::{IExtensionService, IInternalExtensionService};

/// `MainThreadExtensionService`.
pub struct MainThreadExtensionService {
    internal_extension_service: Rc<dyn IInternalExtensionService>,
    extension_service: Rc<dyn IExtensionService>,
}

impl MainThreadExtensionService {
    pub fn new(ext_host_context: &Rc<dyn IInternalExtHostContext>, extension_service: Rc<dyn IExtensionService>) -> Rc<Self> {
        let internal_extension_service = ext_host_context.internal_extension_service();
        ext_host_context.set_extension_host_proxy(ExtensionHostProxy::new(ext_host_context.rpc_protocol().get_proxy_untyped(EXT_HOST_EXTENSION_SERVICE_SID)));
        ext_host_context.set_all_main_proxy_identifiers(ExtHostCustomersRegistry::get_named_customers().iter().map(|customer| customer.id).collect());
        Rc::new(Self { internal_extension_service, extension_service })
    }
}

impl IDisposable for MainThreadExtensionService {
    fn dispose(&self) {}
}

impl MainThreadExtensionServiceShape for MainThreadExtensionService {
    fn rpc_get_extension(&self, extension_id: &str) -> Promise<Option<ExtensionDescription>> {
        let wanted = ExtensionIdentifier::new(extension_id);
        Promise::resolved(self.extension_service.extensions().into_iter().find(|extension| extension.identifier == wanted))
    }

    fn rpc_activate_extension(&self, extension_id: &ExtensionIdentifier, reason: &ExtensionActivationReason) -> Promise<()> {
        self.internal_extension_service.activate_by_id(extension_id, reason.clone())
    }

    fn rpc_on_will_activate_extension(&self, extension_id: &ExtensionIdentifier) -> Promise<()> {
        self.internal_extension_service.on_will_activate_extension(extension_id);
        Promise::resolved(())
    }

    fn rpc_on_did_activate_extension(
        &self,
        extension_id: &ExtensionIdentifier,
        code_loading_time: f64,
        activate_call_time: f64,
        activate_resolved_time: f64,
        activation_reason: &ExtensionActivationReason,
    ) {
        self.internal_extension_service.on_did_activate_extension(extension_id, code_loading_time, activate_call_time, activate_resolved_time, activation_reason.clone());
    }

    fn rpc_on_extension_activation_error(
        &self,
        extension_id: &ExtensionIdentifier,
        error: SerializedError,
        _missing_extension_dependency: Option<MissingExtensionDependency>,
    ) -> Promise<()> {
        let error = transform_error_from_serialization(&error);
        self.internal_extension_service.on_did_activate_extension_error(extension_id, &error);
        Promise::resolved(())
    }

    fn rpc_on_extension_runtime_error(&self, extension_id: &ExtensionIdentifier, data: SerializedError) {
        let error = transform_error_from_serialization(&data);
        self.internal_extension_service.on_extension_runtime_error(extension_id, &error);
        eprintln!("[{}]{}", extension_id.value, error.message);
        if let Some(stack) = &error.stack {
            eprintln!("{stack}");
        }
    }

    fn rpc_set_performance_marks(&self, _marks: Vec<PerformanceMark>) -> Promise<()> {
        Promise::resolved(())
    }

    fn rpc_as_browser_uri(&self, uri: &URI) -> Promise<URI> {
        Promise::resolved(uri.clone())
    }
}

/// `@extHostNamedCustomer(MainContext.MainThreadExtensionService)`: registra o cliente, que lê o `IExtensionService` dado.
pub fn register_main_thread_extension_service(extension_service: Rc<dyn IExtensionService>) {
    ext_host_named_customer(
        MainContext::MAIN_THREAD_EXTENSION_SERVICE.sid,
        Rc::new(move |context: &Rc<dyn IInternalExtHostContext>| {
            let service = MainThreadExtensionService::new(context, extension_service.clone());
            let shape: Rc<dyn MainThreadExtensionServiceShape> = service.clone();
            Ok(NamedCustomerInstance { disposable: service, binding: CustomerBinding::Shape(Rc::new(shape)) })
        }),
    );
}
