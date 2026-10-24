//! O host de extensões: o programa Node (`NodeRunner`), os serviços `ExtHost*` e o objeto `vscode` que eles alimentam.

use std::rc::Rc;

use je_base::event::Event;
use je_base::lifecycle::IDisposable;
use je_base::promise::Promise;
use je_platform::log::NullLogService;
use je_workbench::api::browser::main_thread_commands::MainThreadCommands;
use je_workbench::api::common::ext_host_commands::ExtHostCommands;
use je_workbench::api::common::ext_host_protocol::{
    ExtHostCommandsShape, ExtHostContext, ICommandMetadataDto, IExtHostTelemetry, IMainContext, MainContext, MainThreadTelemetryShape,
};
use je_workbench::services::extensions::extensions::IExtensionService;
use zjsc::api::embed_node::{JsData, NodeRunner};

use crate::api::{commands, disposable, l10n, types};
use crate::local_context::LocalContext;

/// O `ExtHostCommandsShape` que ainda não existe quando o lado do workbench é criado: encaminha para o real depois de
/// ligado (os dois lados do RPC nascem um dentro do outro).
#[derive(Default)]
struct LateExtHostCommands(std::cell::RefCell<Option<Rc<dyn ExtHostCommandsShape>>>);

impl ExtHostCommandsShape for LateExtHostCommands {
    fn rpc_execute_contributed_command(&self, id: &str, args: Vec<je_base::dynamic::Dynamic>) -> Promise<je_base::dynamic::Dynamic> {
        self.0.borrow().as_ref().expect("ExtHostCommands ainda não ligado").rpc_execute_contributed_command(id, args)
    }

    fn rpc_get_contributed_command_metadata(&self) -> Promise<Vec<(String, ICommandMetadataDto)>> {
        self.0.borrow().as_ref().expect("ExtHostCommands ainda não ligado").rpc_get_contributed_command_metadata()
    }
}

/// A telemetria que descarta tudo (o lado do workbench real entra quando a telemetria for montada).
struct NullTelemetry;

impl IDisposable for NullTelemetry {
    fn dispose(&self) {}
}

impl MainThreadTelemetryShape for NullTelemetry {
    fn rpc_public_log(&self, _event_name: &str, _data: Option<je_base::dynamic::Dynamic>) {}
    fn rpc_public_log2(&self, _event_name: &str, _data: Option<je_base::dynamic::Dynamic>) {}
}

impl IExtHostTelemetry for NullTelemetry {
    fn on_extension_error(&self, _extension: &je_extensions::manifest::ExtensionIdentifier, _error: &je_base::errors::Error) -> bool {
        false
    }
}

/// Os serviços do workbench de que o host depende e que o chamador monta.
pub struct WorkbenchServices {
    pub command_service: Rc<dyn je_platform::commands::commands::ICommandService>,
    pub extension_service: Rc<dyn IExtensionService>,
}

/// O host de extensões.
pub struct ExtensionHost {
    runner: NodeRunner,
    commands: Rc<ExtHostCommands>,
    vscode: JsData,
    _main_thread_commands: Rc<MainThreadCommands>,
}

impl ExtensionHost {
    /// Monta os serviços e registra `require('vscode')` em `runner`.
    pub fn new(runner: &NodeRunner, services: WorkbenchServices) -> ExtensionHost {
        let context = LocalContext::new();
        let context_ref: &dyn IMainContext = &*context;
        let late = Rc::new(LateExtHostCommands::default());
        let late_shape: Rc<dyn ExtHostCommandsShape> = late.clone();
        context_ref.set_erased(ExtHostContext::EXT_HOST_COMMANDS.sid, Rc::new(late_shape));
        let telemetry: Rc<dyn MainThreadTelemetryShape> = Rc::new(NullTelemetry);
        context_ref.set_erased(MainContext::MAIN_THREAD_TELEMETRY.sid, Rc::new(telemetry));
        let main_thread_commands = MainThreadCommands::new(context_ref, services.command_service, services.extension_service);
        let main_shape: Rc<dyn je_workbench::api::common::ext_host_protocol::MainThreadCommandsShape> = main_thread_commands.clone();
        context_ref.set_erased(MainContext::MAIN_THREAD_COMMANDS.sid, Rc::new(main_shape));
        let commands = ExtHostCommands::new(context_ref, NullLogService::shared(), Rc::new(NullTelemetry));
        let shape: Rc<dyn ExtHostCommandsShape> = commands.clone();
        *late.0.borrow_mut() = Some(shape);

        let table = disposable::DisposableTable::new();
        let disposable_class = disposable::define(runner, &table);
        let mut members = types::classes(runner);
        members.push(("Disposable".to_owned(), disposable_class));
        members.extend([
            ("commands".to_owned(), commands::namespace(runner, &commands, &table)),
            ("l10n".to_owned(), l10n::namespace(runner)),
        ]);
        let vscode = JsData::Object(members);
        runner.register_module("vscode", &vscode);
        ExtensionHost { runner: runner.clone(), commands, vscode, _main_thread_commands: main_thread_commands }
    }

    pub fn commands(&self) -> &Rc<ExtHostCommands> {
        &self.commands
    }

    pub fn vscode(&self) -> &JsData {
        &self.vscode
    }

    /// Esvazia as microtarefas dos dois mundos (as do motor e as das `Promise` do Rust) até estabilizar.
    pub fn pump(&self) {
        for _ in 0..64 {
            self.runner.drain_microtasks();
            je_base::runtime::run_microtasks();
        }
    }

    /// `Event::none()` serve de marcador para o laço do host quando não há evento externo.
    pub fn idle_event() -> Event<()> {
        Event::none()
    }
}
