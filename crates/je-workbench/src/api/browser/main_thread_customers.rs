//! Os clientes `MainThread*` que o host de extensões em processo usa além do `MainThreadExtensionService`: o
//! `@extHostNamedCustomer(MainContext.MainThreadCommands)` de `mainThreadCommands.ts` e o `@extHostNamedCustomer(MainContext.MainThreadTelemetry)`
//! de `mainThreadTelemetry.ts`, registrados no `ExtHostCustomersRegistry` com os serviços do workbench que o chamador entrega.
//!
//! Divergências:
//! - O `MainThreadTelemetry` do original (`api/browser/main_thread_telemetry.rs`) exige o conjunto completo de serviços (configuração,
//!   ambiente, produto) e devolve metadados ao host. O cliente registrado aqui é o [`MainThreadTelemetryBridge`], que só repassa
//!   `$publicLog` e `$publicLog2` ao `ITelemetryService`, com os dados como vierem, que é o que o `MainThreadCommands` e o
//!   `ExtHostCommands` consomem.

use std::rc::Rc;

use je_base::dynamic::Dynamic;
use je_base::lifecycle::IDisposable;
use je_platform::commands::commands::ICommandService;
use je_platform::telemetry::telemetry::ITelemetryService;

use super::main_thread_commands::MainThreadCommands;
use crate::api::common::ext_host_protocol::{MainContext, MainThreadCommandsShape, MainThreadTelemetryShape};
use crate::services::extensions::ext_host_customers::{CustomerBinding, IInternalExtHostContext, NamedCustomerInstance, ext_host_named_customer};
use crate::services::extensions::extensions::IExtensionService;

/// `@extHostNamedCustomer(MainContext.MainThreadCommands)`.
pub fn register_main_thread_commands(command_service: Rc<dyn ICommandService>, extension_service: Rc<dyn IExtensionService>) {
    ext_host_named_customer(
        MainContext::MAIN_THREAD_COMMANDS.sid,
        Rc::new(move |context: &Rc<dyn IInternalExtHostContext>| {
            let commands = MainThreadCommands::new(&*context.rpc_protocol(), command_service.clone(), extension_service.clone());
            let shape: Rc<dyn MainThreadCommandsShape> = commands.clone();
            Ok(NamedCustomerInstance { disposable: commands, binding: CustomerBinding::Shape(Rc::new(shape)) })
        }),
    );
}

/// O cliente de telemetria que repassa os eventos do host ao `ITelemetryService` (ver a nota do módulo).
pub struct MainThreadTelemetryBridge {
    telemetry_service: Rc<dyn ITelemetryService>,
}

impl IDisposable for MainThreadTelemetryBridge {
    fn dispose(&self) {}
}

impl MainThreadTelemetryShape for MainThreadTelemetryBridge {
    fn rpc_public_log(&self, event_name: &str, data: Option<Dynamic>) {
        self.telemetry_service.public_log2(event_name, data);
    }

    fn rpc_public_log2(&self, event_name: &str, data: Option<Dynamic>) {
        self.telemetry_service.public_log2(event_name, data);
    }
}

/// `@extHostNamedCustomer(MainContext.MainThreadTelemetry)`, com o [`MainThreadTelemetryBridge`].
pub fn register_main_thread_telemetry_bridge(telemetry_service: Rc<dyn ITelemetryService>) {
    ext_host_named_customer(
        MainContext::MAIN_THREAD_TELEMETRY.sid,
        Rc::new(move |_context: &Rc<dyn IInternalExtHostContext>| {
            let bridge = Rc::new(MainThreadTelemetryBridge { telemetry_service: telemetry_service.clone() });
            let shape: Rc<dyn MainThreadTelemetryShape> = bridge.clone();
            Ok(NamedCustomerInstance { disposable: bridge, binding: CustomerBinding::Shape(Rc::new(shape)) })
        }),
    );
}
