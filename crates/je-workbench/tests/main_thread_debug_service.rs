//! Porte de `src/vs/workbench/api/test/browser/mainThreadDebugService.test.ts` (suíte `MainThreadDebugService`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` vira o guarda criado no começo de cada caso, e o `store` é o `DisposableStore` dele.
//!   Os casos `async` rodam em `run_with_faked_timers`.
//! - `mock<T>()` (`workbenchTestServices.ts`: um objeto vazio que o TypeScript trata como `T`, e que falha com `TypeError` ao chamar
//!   um membro que a suíte não definiu) não existe em Rust: cada dublê implementa o contrato inteiro, e o membro que o original não
//!   sobrescreve é `unreachable!`. Os `override readonly onDid... = Event.None` são `Event::none()`.
//! - O `IExtHostContext` do original (`remoteAuthority = ''`, `extensionHostKind = LocalProcess`, `getProxy(): any { return {}; }`)
//!   é o [`EmptyProxyContext`]: o `remoteAuthority` e o `extensionHostKind` não existem no [`IMainContext`] do porte (a classe não os
//!   lê), e o proxy `{}` é o [`EmptyExtHostDebugService`], em que qualquer membro falha.
//! - `registrationDisposed = true` dentro do `toDisposable` do original é um `Rc<Cell<bool>>` posto pelo `to_disposable`.
//! - `new MainThreadDebugService(extHostContext, debugService, new class extends mock<IDebugVisualizerService>() { })` é o
//!   [`create_service`]; o `IDebugVisualizerService` é o [`UnusedVisualizerService`].

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use std::any::Any;
use std::cell::Cell;
use std::rc::Rc;

use common::{ensure_no_disposables_are_leaked_in_test_suite, run_with_faked_timers};
use je_base::cancellation::CancellationToken;
use je_base::event::Event;
use je_base::lifecycle::{DisposableRef, DisposableStore, to_disposable};
use je_base::promise::Promise;
use je_base::uri::URI;
use je_workbench::api::browser::main_thread_debug_service::MainThreadDebugService;
use je_workbench::api::common::ext_host_debug_protocol::{ExtHostDebugServiceShape, IBreakpointsDeltaDto, IDebugSessionDto, MainThreadDebugServiceShape, StackFocusDto};
use je_workbench::api::common::ext_host_protocol::IMainContext;
use je_workbench::contrib::debug::debug::{
    BreakpointIds, ConfigOrName, DebugConfigurationProviderTriggerKind, DebugSessionEndEvent, IAdapterDescriptor, IAdapterManager, IBreakpoint, IBreakpointData,
    IBreakpointsChangeEvent, IConfig, IConfigurationManager, IDataBreakpoint, IDataBreakpointOptions, IDebugAdapterDescriptorFactory, IDebugAdapterFactory,
    IDebugConfigurationProvider, IDebugModel, IDebugService, IDebugSession, IDebugSessionOptions, IDebugVisualization, IDebugVisualizationContext, IDebugVisualizationTreeItem,
    IFunctionBreakpoint, IFunctionBreakpointOptions, ILaunch, IViewModel, MainThreadDebugVisualization, StackFrameFocusEvent, ThreadFocusEvent,
};
use je_workbench::contrib::debug::debug_protocol::ProtocolMessage;
use je_workbench::contrib::debug::debug_visualizers::{IDebugVisualizerService, VisualizerHandle, VisualizerTreeHandle};
use serde_json::Value;

// ---- o `{}` do `getProxy`

/// O proxy `{}`: qualquer membro falha, como o `TypeError` de chamar um membro de um objeto vazio.
struct EmptyExtHostDebugService;

impl ExtHostDebugServiceShape for EmptyExtHostDebugService {
    fn rpc_substitute_variables(&self, _folder: Option<URI>, _config: IConfig) -> Promise<IConfig> {
        unreachable!("TypeError: proxy.$substituteVariables is not a function")
    }
    fn rpc_run_in_terminal(&self, _args: Value, _session_id: &str) -> Promise<Option<i64>> {
        unreachable!("TypeError: proxy.$runInTerminal is not a function")
    }
    fn rpc_start_da_session(&self, _handle: u32, _session: IDebugSessionDto) -> Promise<()> {
        unreachable!("TypeError: proxy.$startDASession is not a function")
    }
    fn rpc_stop_da_session(&self, _handle: u32) -> Promise<()> {
        unreachable!("TypeError: proxy.$stopDASession is not a function")
    }
    fn rpc_send_da_message(&self, _handle: u32, _message: ProtocolMessage) {
        unreachable!("TypeError: proxy.$sendDAMessage is not a function")
    }
    fn rpc_resolve_debug_configuration(&self, _handle: u32, _folder: Option<URI>, _debug_configuration: IConfig, _token: CancellationToken) -> Promise<Option<IConfig>> {
        unreachable!("TypeError: proxy.$resolveDebugConfiguration is not a function")
    }
    fn rpc_resolve_debug_configuration_with_substituted_variables(
        &self,
        _handle: u32,
        _folder: Option<URI>,
        _debug_configuration: IConfig,
        _token: CancellationToken,
    ) -> Promise<Option<IConfig>> {
        unreachable!("TypeError: proxy.$resolveDebugConfigurationWithSubstitutedVariables is not a function")
    }
    fn rpc_provide_debug_configurations(&self, _handle: u32, _folder: Option<URI>, _token: CancellationToken) -> Promise<Vec<IConfig>> {
        unreachable!("TypeError: proxy.$provideDebugConfigurations is not a function")
    }
    fn rpc_provide_debug_adapter(&self, _handle: u32, _session: IDebugSessionDto) -> Promise<IAdapterDescriptor> {
        unreachable!("TypeError: proxy.$provideDebugAdapter is not a function")
    }
    fn rpc_accept_debug_session_started(&self, _session: IDebugSessionDto) -> Promise<()> {
        unreachable!("TypeError: proxy.$acceptDebugSessionStarted is not a function")
    }
    fn rpc_accept_debug_session_terminated(&self, _session: IDebugSessionDto) -> Promise<()> {
        unreachable!("TypeError: proxy.$acceptDebugSessionTerminated is not a function")
    }
    fn rpc_accept_debug_session_active_changed(&self, _session: Option<IDebugSessionDto>) -> Promise<()> {
        unreachable!("TypeError: proxy.$acceptDebugSessionActiveChanged is not a function")
    }
    fn rpc_accept_debug_session_custom_event(&self, _session: IDebugSessionDto, _event: Value) -> Promise<()> {
        unreachable!("TypeError: proxy.$acceptDebugSessionCustomEvent is not a function")
    }
    fn rpc_accept_breakpoints_delta(&self, _delta: IBreakpointsDeltaDto) {
        unreachable!("TypeError: proxy.$acceptBreakpointsDelta is not a function")
    }
    fn rpc_accept_debug_session_name_changed(&self, _session: IDebugSessionDto, _name: &str) -> Promise<()> {
        unreachable!("TypeError: proxy.$acceptDebugSessionNameChanged is not a function")
    }
    fn rpc_accept_stack_frame_focus(&self, _focus: Option<StackFocusDto>) -> Promise<()> {
        unreachable!("TypeError: proxy.$acceptStackFrameFocus is not a function")
    }
    fn rpc_provide_debug_visualizers(&self, _extension_id: &str, _id: &str, _context: IDebugVisualizationContext, _token: CancellationToken) -> Promise<Vec<IDebugVisualization>> {
        unreachable!("TypeError: proxy.$provideDebugVisualizers is not a function")
    }
    fn rpc_resolve_debug_visualizer(&self, _id: u64, _token: CancellationToken) -> Promise<MainThreadDebugVisualization> {
        unreachable!("TypeError: proxy.$resolveDebugVisualizer is not a function")
    }
    fn rpc_execute_debug_visualizer_command(&self, _id: u64) -> Promise<()> {
        unreachable!("TypeError: proxy.$executeDebugVisualizerCommand is not a function")
    }
    fn rpc_dispose_debug_visualizers(&self, _ids: Vec<u64>) {
        unreachable!("TypeError: proxy.$disposeDebugVisualizers is not a function")
    }
    fn rpc_get_visualizer_tree_item(&self, _tree_id: &str, _element: IDebugVisualizationContext) -> Promise<Option<IDebugVisualizationTreeItem>> {
        unreachable!("TypeError: proxy.$getVisualizerTreeItem is not a function")
    }
    fn rpc_get_visualizer_tree_item_children(&self, _tree_id: &str, _element: u64) -> Promise<Vec<IDebugVisualizationTreeItem>> {
        unreachable!("TypeError: proxy.$getVisualizerTreeItemChildren is not a function")
    }
    fn rpc_edit_visualizer_tree_item(&self, _element: u64, _value: &str) -> Promise<Option<IDebugVisualizationTreeItem>> {
        unreachable!("TypeError: proxy.$editVisualizerTreeItem is not a function")
    }
    fn rpc_dispose_visualized_tree(&self, _element: u64) {
        unreachable!("TypeError: proxy.$disposeVisualizedTree is not a function")
    }
}

/// O `IExtHostContext` do original.
struct EmptyProxyContext;

impl IMainContext for EmptyProxyContext {
    fn get_proxy_erased(&self, _sid: &'static str) -> Rc<dyn Any> {
        let proxy: Rc<dyn ExtHostDebugServiceShape> = Rc::new(EmptyExtHostDebugService);
        Rc::new(proxy)
    }

    fn set_erased(&self, _sid: &'static str, _value: Rc<dyn Any>) {}
}

// ---- o `IDebugVisualizerService` do `mock`

struct UnusedVisualizerService;

impl IDebugVisualizerService for UnusedVisualizerService {
    fn register(&self, _handle: VisualizerHandle) -> DisposableRef {
        unreachable!("TypeError: visualizerService.register is not a function")
    }

    fn register_tree(&self, _tree_id: &str, _handle: VisualizerTreeHandle) -> DisposableRef {
        unreachable!("TypeError: visualizerService.registerTree is not a function")
    }
}

// ---- IDebugModel e IViewModel

/// O `IDebugModel` mockado: `onDidChangeBreakpoints = Event.None` e as três listas vazias.
struct TestModel;

impl IDebugModel for TestModel {
    fn get_session(&self, _session_id: &str, _include_inactive: bool) -> Option<Rc<dyn IDebugSession>> {
        unreachable!("TypeError: model.getSession is not a function")
    }

    fn get_breakpoints(&self) -> Vec<IBreakpoint> {
        Vec::new()
    }

    fn get_function_breakpoints(&self) -> Vec<IFunctionBreakpoint> {
        Vec::new()
    }

    fn get_data_breakpoints(&self) -> Vec<IDataBreakpoint> {
        Vec::new()
    }

    fn on_did_change_breakpoints(&self) -> Event<Option<IBreakpointsChangeEvent>> {
        Event::none()
    }
}

/// O `IViewModel` mockado: os três eventos de foco são `Event.None`.
struct TestViewModel;

impl IViewModel for TestViewModel {
    fn focused_session(&self) -> Option<Rc<dyn IDebugSession>> {
        unreachable!("TypeError: viewModel.focusedSession is undefined")
    }

    fn focused_thread(&self) -> Option<Rc<dyn je_workbench::contrib::debug::debug::IThread>> {
        unreachable!("TypeError: viewModel.focusedThread is undefined")
    }

    fn focused_stack_frame(&self) -> Option<Rc<dyn je_workbench::contrib::debug::debug::IStackFrame>> {
        unreachable!("TypeError: viewModel.focusedStackFrame is undefined")
    }

    fn on_did_focus_session(&self) -> Event<Option<Rc<dyn IDebugSession>>> {
        Event::none()
    }

    fn on_did_focus_thread(&self) -> Event<ThreadFocusEvent> {
        Event::none()
    }

    fn on_did_focus_stack_frame(&self) -> Event<StackFrameFocusEvent> {
        Event::none()
    }
}

// ---- IConfigurationManager e IAdapterManager

/// O `IConfigurationManager` do primeiro caso: o registro devolve um `toDisposable` que marca `registrationDisposed`.
struct TestConfigurationManager {
    registration_disposed: Rc<Cell<bool>>,
}

impl IConfigurationManager for TestConfigurationManager {
    fn register_debug_configuration_provider(&self, _provider: Rc<IDebugConfigurationProvider>) -> DisposableRef {
        let registration_disposed = self.registration_disposed.clone();
        to_disposable(move || registration_disposed.set(true))
    }

    fn unregister_debug_configuration_provider(&self, _provider: &Rc<IDebugConfigurationProvider>) {}

    fn get_launch(&self, _workspace_uri: Option<&URI>) -> Option<Rc<dyn ILaunch>> {
        unreachable!("TypeError: configurationManager.getLaunch is not a function")
    }
}

/// O `IAdapterManager` do segundo caso: o registro da fábrica de descritores devolve um `toDisposable` que marca
/// `registrationDisposed`.
struct TestAdapterManager {
    registration_disposed: Rc<Cell<bool>>,
}

impl IAdapterManager for TestAdapterManager {
    fn register_debug_adapter_factory(&self, _debug_types: Vec<String>, _debug_adapter_launcher: Rc<dyn IDebugAdapterFactory>) -> DisposableRef {
        unreachable!("TypeError: adapterManager.registerDebugAdapterFactory is not a function")
    }

    fn register_debug_adapter_descriptor_factory(&self, _factory: Rc<IDebugAdapterDescriptorFactory>) -> DisposableRef {
        let registration_disposed = self.registration_disposed.clone();
        to_disposable(move || registration_disposed.set(true))
    }

    fn unregister_debug_adapter_descriptor_factory(&self, _factory: &Rc<IDebugAdapterDescriptorFactory>) {}
}

// ---- IDebugService

/// O `IDebugService` mockado: `onDidNewSession`, `onWillNewSession` e `onDidEndSession` são `Event.None`, e os `get*` do caso
/// devolvem o que ele montou (os outros falham).
struct TestDebugService {
    configuration_manager: Option<Rc<dyn IConfigurationManager>>,
    adapter_manager: Option<Rc<dyn IAdapterManager>>,
}

impl IDebugService for TestDebugService {
    fn on_will_new_session(&self) -> Event<Rc<dyn IDebugSession>> {
        Event::none()
    }

    fn on_did_new_session(&self) -> Event<Rc<dyn IDebugSession>> {
        Event::none()
    }

    fn on_did_end_session(&self) -> Event<DebugSessionEndEvent> {
        Event::none()
    }

    fn get_configuration_manager(&self) -> Rc<dyn IConfigurationManager> {
        self.configuration_manager.clone().expect("TypeError: debugService.getConfigurationManager is not a function")
    }

    fn get_adapter_manager(&self) -> Rc<dyn IAdapterManager> {
        self.adapter_manager.clone().expect("TypeError: debugService.getAdapterManager is not a function")
    }

    fn get_model(&self) -> Rc<dyn IDebugModel> {
        Rc::new(TestModel)
    }

    fn get_view_model(&self) -> Rc<dyn IViewModel> {
        Rc::new(TestViewModel)
    }

    fn add_breakpoints(&self, _uri: URI, _raw_breakpoints: Vec<IBreakpointData>) -> Promise<Vec<IBreakpoint>> {
        unreachable!("TypeError: debugService.addBreakpoints is not a function")
    }

    fn remove_breakpoints(&self, _id: Option<BreakpointIds>) -> Promise<()> {
        unreachable!("TypeError: debugService.removeBreakpoints is not a function")
    }

    fn add_function_breakpoint(&self, _opts: Option<IFunctionBreakpointOptions>, _id: Option<String>) {
        unreachable!("TypeError: debugService.addFunctionBreakpoint is not a function")
    }

    fn remove_function_breakpoints(&self, _id: Option<&str>) -> Promise<()> {
        unreachable!("TypeError: debugService.removeFunctionBreakpoints is not a function")
    }

    fn add_data_breakpoint(&self, _opts: IDataBreakpointOptions) -> Promise<()> {
        unreachable!("TypeError: debugService.addDataBreakpoint is not a function")
    }

    fn remove_data_breakpoints(&self, _id: Option<&str>) -> Promise<()> {
        unreachable!("TypeError: debugService.removeDataBreakpoints is not a function")
    }

    fn start_debugging(
        &self,
        _launch: Option<Rc<dyn ILaunch>>,
        _config_or_name: Option<ConfigOrName>,
        _options: Option<IDebugSessionOptions>,
        _save_before_start: Option<bool>,
    ) -> Promise<bool> {
        unreachable!("TypeError: debugService.startDebugging is not a function")
    }

    fn stop_session(&self, _session: Option<Rc<dyn IDebugSession>>, _disconnect: bool) -> Promise<()> {
        unreachable!("TypeError: debugService.stopSession is not a function")
    }
}

/// `new MainThreadDebugService(extHostContext, debugService, new class extends mock<IDebugVisualizerService>() { })`.
fn create_service(store: &DisposableStore, debug_service: TestDebugService) -> Rc<MainThreadDebugService> {
    store.add_rc(MainThreadDebugService::new(&EmptyProxyContext, Rc::new(debug_service), Rc::new(UnusedVisualizerService)))
}

#[test]
fn disposes_debug_configuration_provider_registrations_when_they_are_unregistered() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let store = guard.store();
    run_with_faked_timers(async move {
        let registration_disposed = Rc::new(Cell::new(false));
        let configuration_manager: Rc<dyn IConfigurationManager> = Rc::new(TestConfigurationManager { registration_disposed: registration_disposed.clone() });
        let service = create_service(&store, TestDebugService { configuration_manager: Some(configuration_manager), adapter_manager: None });

        service.rpc_register_debug_configuration_provider("test", DebugConfigurationProviderTriggerKind::Initial, true, false, false, 1).await.unwrap();
        service.rpc_unregister_debug_configuration_provider(1);

        assert!(registration_disposed.get());
    });
}

#[test]
fn disposes_debug_adapter_descriptor_factory_registrations_when_they_are_unregistered() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let store = guard.store();
    run_with_faked_timers(async move {
        let registration_disposed = Rc::new(Cell::new(false));
        let adapter_manager: Rc<dyn IAdapterManager> = Rc::new(TestAdapterManager { registration_disposed: registration_disposed.clone() });
        let service = create_service(&store, TestDebugService { configuration_manager: None, adapter_manager: Some(adapter_manager) });

        service.rpc_register_debug_adapter_descriptor_factory("test", 1).await.unwrap();
        service.rpc_unregister_debug_adapter_descriptor_factory(1);

        assert!(registration_disposed.get());
    });
}
