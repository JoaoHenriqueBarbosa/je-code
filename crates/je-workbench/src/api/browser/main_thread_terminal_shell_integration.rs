//! Porte de `src/vs/workbench/api/browser/mainThreadTerminalShellIntegration.ts`: a ponte que repassa ao host de extensões a
//! integração com o shell dos terminais (comandos executados, saída em lotes, diretório de trabalho e ambiente do shell).
//!
//! Divergências:
//! - `@extHostNamedCustomer(MainContext.MainThreadTerminalShellIntegration)` e o `IExtHostContext` do construtor: o registro de
//!   clientes não existe ainda; o construtor recebe o [`IMainContext`] e os serviços (o `IWorkbenchEnvironmentService` do original,
//!   que o construtor não usa, não é recebido).
//! - `createOnInstanceEvent` e `createOnInstanceCapabilityEvent` são os do [`ITerminalServiceEvents`] (ver `terminal_events.rs`).
//! - `Object.keys(envValue)` e `Object.values(envValue)` são as chaves e os valores do mapa na ordem de inserção; o valor
//!   `undefined` fica como `None`. `e.data.value && typeof e.data.value === 'object'` é o `Some`.
//! - `e.data === currentCommand` é a identidade do `Rc` do comando.
//! - `$executeCommand` devolve `()`, como a forma.

use std::cell::RefCell;
use std::rc::{Rc, Weak};

use je_base::lifecycle::{DisposableMap, DisposableRef, DisposableStore, IDisposable, combined_disposable};
use je_base::symbols::Delay;
use je_platform::terminal::capabilities::capabilities::{
    CommandDetection, CwdDetection, IBaseTerminalCommand, ITerminalCommand, ShellEnvDetection, TerminalCommandConfidence,
};
use je_platform::terminal::capabilities::terminal_capability_store::TerminalCapabilityStoreExt;
use je_platform::terminal::terminal::TerminalType;

use crate::api::common::ext_host_protocol::{ExtHostContext, ExtHostTerminalShellIntegrationShape, IMainContext, MainThreadTerminalShellIntegrationShape};
use crate::api::common::ext_host_types::TerminalShellExecutionCommandLineConfidence;
use crate::contrib::terminal::browser::terminal::{ITerminalInstance, ITerminalService};
use crate::contrib::terminal::browser::terminal_events::ITerminalServiceEvents;
use crate::services::extensions::extensions::IExtensionService;

/// `MainThreadTerminalShellIntegration`.
pub struct MainThreadTerminalShellIntegration {
    store: Rc<DisposableStore>,
    proxy: Rc<dyn ExtHostTerminalShellIntegrationShape>,
    terminal_service: Rc<dyn ITerminalService>,
    extension_service: Rc<dyn IExtensionService>,
}

impl MainThreadTerminalShellIntegration {
    pub fn new(
        ext_host_context: &dyn IMainContext,
        terminal_service: Rc<dyn ITerminalService>,
        extension_service: Rc<dyn IExtensionService>,
    ) -> Rc<Self> {
        let proxy = ext_host_context.get_proxy(&ExtHostContext::EXT_HOST_TERMINAL_SHELL_INTEGRATION);
        let store = DisposableStore::new();
        let this = Rc::new(Self { store: store.clone(), proxy: proxy.clone(), terminal_service: terminal_service.clone(), extension_service });

        let instance_data_listeners = store.add_rc(DisposableMap::<u32, dyn IDisposable>::new());

        // onDidChangeTerminalShellIntegration initial state
        for terminal in terminal_service.instances() {
            let cmd_detection = terminal.capabilities().get::<CommandDetection>();
            if cmd_detection.is_some() {
                this.enable_shell_integration(&terminal);
            }
        }

        // onDidChangeTerminalShellIntegration via command detection
        let on_did_add_command_detection = terminal_service.create_on_instance_event(|instance| {
            let instance = instance.clone();
            instance.capabilities().on_did_add_command_detection_capability().map({
                let instance = instance.clone();
                move |_| instance.clone()
            }, None)
        });
        let on_did_add_command_detection_event = on_did_add_command_detection.event.clone();
        store.add(Rc::new(on_did_add_command_detection));
        {
            let weak: Weak<Self> = Rc::downgrade(&this);
            store.add(on_did_add_command_detection_event.listen(move |e| {
                if let Some(this) = weak.upgrade() {
                    this.enable_shell_integration(e);
                }
            }));
        }

        // onDidChangeTerminalShellIntegration via cwd
        let cwd_change_event = terminal_service.create_on_instance_capability_event::<CwdDetection, _>(|capability| capability.on_did_change_cwd());
        let cwd_change_listener = {
            let proxy = proxy.clone();
            cwd_change_event.event.listen(move |e| proxy.rpc_cwd_change(e.instance.instance_id(), Some(e.data.as_str())))
        };
        store.add(Rc::new(cwd_change_event));
        store.add(cwd_change_listener);

        // onDidChangeTerminalShellIntegration via env
        let env_change_event = terminal_service.create_on_instance_capability_event::<ShellEnvDetection, _>(|capability| capability.on_did_change_env());
        let env_change_listener = {
            let proxy = proxy.clone();
            env_change_event.event.listen(move |e| {
                if let Some(env_value) = &e.data.value {
                    // Extract keys and values
                    let keys_arr: Vec<String> = env_value.keys().cloned().collect();
                    let values_arr: Vec<Option<String>> = env_value.values().cloned().collect();
                    proxy.rpc_shell_env_change(e.instance.instance_id(), &keys_arr, &values_arr, e.data.is_trusted);
                }
            })
        };
        store.add(Rc::new(env_change_event));
        store.add(env_change_listener);

        // onDidStartTerminalShellExecution
        let command_detection_start_event = terminal_service.create_on_instance_capability_event::<CommandDetection, _>(|capability| capability.on_command_executed());
        let current_command: Rc<RefCell<Option<Rc<dyn ITerminalCommand>>>> = Rc::default();
        let command_detection_start_listener = {
            let (proxy, current_command, instance_data_listeners) = (proxy.clone(), current_command.clone(), instance_data_listeners.clone());
            command_detection_start_event.event.listen(move |e| {
                // Prevent duplicate events from being sent in case command detection double fires the
                // event
                let is_same = current_command.borrow().as_ref().is_some_and(|current| Rc::ptr_eq(current, &e.data));
                if is_same {
                    return;
                }
                // String paths are not exposed in the extension API
                *current_command.borrow_mut() = Some(e.data.clone());
                let instance_id = e.instance.instance_id();
                proxy.rpc_shell_execution_start(
                    instance_id,
                    instance_supports_execute_command_api(&e.instance),
                    &e.data.command(),
                    convert_to_ext_host_command_line_confidence(&*e.data),
                    e.data.is_trusted(),
                    e.data.cwd().as_deref(),
                );

                // TerminalShellExecution.createDataStream
                // Debounce events to reduce the message count - when this listener is disposed the events will be flushed
                instance_data_listeners.delete_and_dispose(&instance_id);
                let data_disposables = DisposableStore::new();
                let listener = {
                    let proxy = proxy.clone();
                    e.instance.on_data().accumulate(Delay::Millis(50), Some(true), Some(&data_disposables)).listen(move |events| {
                        proxy.rpc_shell_execution_data(instance_id, &events.join(""));
                    })
                };
                // Flush buffered data by removing the listener before disposing the accumulator.
                instance_data_listeners.set(instance_id, combined_disposable(vec![listener, data_disposables as DisposableRef]), false);
            })
        };
        store.add(Rc::new(command_detection_start_event));
        store.add(command_detection_start_listener);

        // onDidEndTerminalShellExecution
        let command_detection_end_event = terminal_service.create_on_instance_capability_event::<CommandDetection, _>(|capability| capability.on_command_finished());
        let command_detection_end_listener = {
            let (proxy, instance_data_listeners) = (proxy.clone(), instance_data_listeners.clone());
            command_detection_end_event.event.listen(move |e| {
                *current_command.borrow_mut() = None;
                let instance_id = e.instance.instance_id();
                instance_data_listeners.delete_and_dispose(&instance_id);
                // Shell integration C (executed) and D (command finished) sequences should always be in
                // their own events, so send this immediately. This means that the D sequence will not
                // be included as it's currently being parsed when the command finished event fires.
                proxy.rpc_shell_execution_end(
                    instance_id,
                    &e.data.command(),
                    convert_to_ext_host_command_line_confidence(&*e.data),
                    e.data.is_trusted(),
                    e.data.exit_code().map(|exit_code| exit_code as i32),
                );
            })
        };
        store.add(Rc::new(command_detection_end_event));
        store.add(command_detection_end_listener);

        // Clean up after dispose
        {
            let (proxy, instance_data_listeners) = (proxy.clone(), instance_data_listeners.clone());
            store.add(terminal_service.on_did_dispose_instance().listen(move |e| {
                instance_data_listeners.delete_and_dispose(&e.instance_id());
                proxy.rpc_close_terminal(e.instance_id());
            }));
        }

        this
    }

    /// `_enableShellIntegration(instance)`.
    fn enable_shell_integration(&self, instance: &Rc<dyn ITerminalInstance>) {
        drop(self.extension_service.activate_by_event("onTerminalShellIntegration:*", None));
        if let Some(shell_type) = instance.shell_type() {
            drop(self.extension_service.activate_by_event(&format!("onTerminalShellIntegration:{}", shell_type.as_str()), None));
        }
        self.proxy.rpc_shell_integration_change(instance.instance_id(), instance_supports_execute_command_api(instance));
        if let Some(cwd_detection) = instance.capabilities().get::<CwdDetection>() {
            self.proxy.rpc_cwd_change(instance.instance_id(), Some(cwd_detection.get_cwd().as_str()));
        }
    }
}

impl IDisposable for MainThreadTerminalShellIntegration {
    fn dispose(&self) {
        self.store.dispose();
    }
}

impl MainThreadTerminalShellIntegrationShape for MainThreadTerminalShellIntegration {
    fn rpc_execute_command(&self, terminal_id: u32, command_line: &str) {
        if let Some(instance) = self.terminal_service.get_instance_from_id(terminal_id) {
            drop(instance.run_command(command_line, true));
        }
    }
}

/// `convertToExtHostCommandLineConfidence(command)`.
fn convert_to_ext_host_command_line_confidence(command: &dyn ITerminalCommand) -> TerminalShellExecutionCommandLineConfidence {
    match command.command_line_confidence() {
        TerminalCommandConfidence::High => TerminalShellExecutionCommandLineConfidence::High,
        TerminalCommandConfidence::Medium => TerminalShellExecutionCommandLineConfidence::Medium,
        TerminalCommandConfidence::Low => TerminalShellExecutionCommandLineConfidence::Low,
    }
}

/// `instanceSupportsExecuteCommandApi(instance)`.
fn instance_supports_execute_command_api(instance: &Rc<dyn ITerminalInstance>) -> bool {
    instance.shell_launch_config().borrow().type_ != Some(TerminalType::Task)
}
