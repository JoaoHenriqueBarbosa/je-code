//! Porte de `src/vs/workbench/api/common/extHostDebugService.ts`: o lado da extensão do serviço de depuração
//! (`vscode.debug`): sessões, breakpoints, provedores de configuração, fábricas de adaptador, rastreadores,
//! visualizações e o adaptador inline.
//!
//! Divergências:
//! - **Dependências.** `IExtHostRpcService` é o [`IMainContext`] (como nos outros `extHost*.rs`). `IExtHostWorkspace` é o
//!   [`IExtHostWorkspaceProvider`] (só o `resolveWorkspaceFolder` é usado); `IExtHostExtensionService`,
//!   `IExtHostVariableResolverProvider`, `IExtHostCommands` e `IExtHostTesting` são os traits estreitos
//!   [`IExtHostDebugExtensionService`], [`IExtHostVariableResolverProvider`], [`IExtHostDebugCommands`] e
//!   [`IExtHostDebugTesting`] (só o que este arquivo chama; o `IExtHostExtensionService` do porte devolve o registro só
//!   para leitura, sem o `onDidChange` que o host de depuração escuta). `IExtHostConfiguration` e `IExtHostEditorTabs`
//!   são `protected` e só as subclasses (Node) os leem, então ficam de fora do construtor.
//! - **Classe base e subclasses.** `ExtHostDebugServiceBase` é um struct; os métodos `protected` que as subclasses
//!   sobrescrevem (`createDebugAdapter`, `createSignService`, `convert*ToDto`, `daExecutableFromPackage`) são o trait
//!   [`ExtHostDebugServiceHooks`], com [`DefaultExtHostDebugServiceHooks`] como comportamento da base e do
//!   `WorkerExtHostDebugService` (que não sobrescreve nada). `createDebugAdapter` é público porque o teste o chama de uma
//!   subclasse.
//! - **`vscode.DebugSession`.** É o [`DebugSession`] (o objeto congelado de `ExtHostDebugSession.api`); o `name` tem
//!   getter e setter (o setter avisa a thread principal).
//! - `$acceptDebugSession*` e demais métodos `async` do original devolvem `Promise<()>`; uma sessão que o `getSession`
//!   não acha rejeita com `cannot find session`.
//! - A chave do `WeakMap<DebugTreeItem, number>` (`_debugVisualizationTreeItemIds`) é o endereço do `Rc` do item: a
//!   entrada sai junto do elemento (o `dispose` do elemento a remove), então o endereço não é reaproveitado enquanto vale.
//! - `serializeVisualization` não tem o ramo `throw new Error('Unsupported debug visualization type')`: o
//!   [`DebugVisualizationTarget`] só tem os dois tipos suportados. `getIconUris` não trata o ícone em texto (o tipo
//!   [`IconPath`] não o tem).
//! - `registerDebugVisualizationProvider` e `registerDebugAdapterDescriptorFactory` lançam erro, então devolvem
//!   [`Result`]. A verificação de duplicidade de `registerDebugVisualizationTree` olha o mapa de provedores, não o de
//!   árvores, como no original.
//! - O `from` do evento de telemetria `debugProtocolMessageError` não tem a classificação de dados (só o evento).
//! - O `Promise.race` de `getDebugAdapterTrackers` com `setTimeout(..., 1000)` usa [`runtime::set_timeout`].

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::rc::{Rc, Weak};

use indexmap::IndexMap;
use je_base::async_::{async_fn, await_maybe};
use je_base::cancellation::CancellationToken;
use je_base::dynamic::{Dynamic, number_to_js_string};
use je_base::errors::{self, Error};
use je_base::event::{Emitter, Event};
use je_base::lifecycle::{Disposable, DisposableMap, DisposableRef, IDisposable};
use je_base::promise::{MaybePromise, Promise};
use je_base::runtime;
use je_base::uri::{URI, encode_uri_component};
use je_base::JsString;
use je_extensions::manifest::ExtensionIdentifier;
use je_extensions::scanner::ExtensionDescription;
use je_platform::resources::host_platform;
use je_platform::workspace::WorkspaceFolder as PlatformWorkspaceFolder;
use serde_json::{Value, json};

use super::ext_host_debug_protocol::{
    BreakpointDto, DebugSessionUuid, ExtHostDebugServiceShape, IBreakpointDto, IBreakpointsDeltaDto, IDebugSessionDto, IFunctionBreakpointDto,
    ISourceBreakpointDto, ISourceMultiBreakpointDto, IStartDebuggingOptions, MainThreadDebugServiceShape, NameOrConfig, RegisterBreakpointDto, SourceMultiBreakpointLine,
    StackFocusDto,
};
use super::ext_host_debug_types::{
    Breakpoint, BreakpointKind, BreakpointsChangeEvent, DebugAdapterDescriptor, DebugAdapterDescriptorFactory, DebugAdapterExecutable, DebugAdapterInlineImplementation,
    DebugAdapterNamedPipeServer, DebugAdapterServer, DebugAdapterTracker, DebugAdapterTrackerFactory, DebugConfigurationProvider, DebugConsoleMode, DebugStackFrame,
    DebugStackItem, DebugThread, DebugTreeItem, DebugVisualization, DebugVisualizationContext, DebugVisualizationProvider, DebugVisualizationTarget, DebugVisualizationTree,
    DebugSessionCustomEvent, DebugSessionOptions, ProviderResult,
};
use super::ext_host_protocol::{IMainContext, MainContext, MainThreadTelemetryShape};
use super::ext_host_types::{self as types, IconPath, Location};
use super::ext_host_workspace::{IExtHostWorkspaceProvider, WorkspaceFolder};
use crate::contrib::debug::abstract_debug_adapter::{AbstractDebugAdapter, DebugAdapterState};
use crate::contrib::debug::debug::{
    DebugConfigurationProviderTriggerKind, IAdapterDescriptor, IConfig, IDebugAdapterExecutable, IDebugAdapterImpl, IDebugAdapterNamedPipeServer, IDebugAdapterServer,
    IDebugSessionReplMode, IDebugTestRunReference, IDebugVisualization, IDebugVisualizationContext, IDebugVisualizationTreeItem, DebugTreeItemCollapsibleState,
    DebugVisualizationIconPath, MainThreadDebugVisualization,
};
use crate::contrib::debug::debug_protocol::{ProtocolMessage, Request, Response};
use crate::contrib::debug::debug_utils::{convert_to_da_paths, convert_to_vsc_paths, is_debugger_main_contribution};
use crate::services::extensions::extension_description_registry::{ExtensionDescriptionRegistry, IReadOnlyExtensionDescriptionRegistry};

// ---- dependências

/// `IExtHostExtensionService`, só o `getExtensionRegistry()`: o registro concreto, com o `onDidChange`.
pub trait IExtHostDebugExtensionService {
    fn get_extension_registry(&self) -> Promise<Rc<ExtensionDescriptionRegistry>>;
}

/// O `IConfigurationResolverService` (`variableResolver`) que o `$substituteVariables` usa.
pub trait IConfigurationResolver {
    /// `resolveAsync(folder, config)`.
    fn resolve_async(&self, folder: Option<PlatformWorkspaceFolder>, config: IConfig) -> Promise<IConfig>;
}

/// `IExtHostVariableResolverProvider`, só o `getResolver()`.
pub trait IExtHostVariableResolverProvider {
    fn get_resolver(&self) -> Promise<Rc<dyn IConfigurationResolver>>;
}

/// `IExtHostCommands`, só o `executeCommand(id, ...args)`.
pub trait IExtHostDebugCommands {
    fn execute_command(&self, id: &str, args: Vec<Dynamic>) -> Promise<Dynamic>;
}

impl IExtHostDebugCommands for super::ext_host_commands::ExtHostCommands {
    fn execute_command(&self, id: &str, args: Vec<Dynamic>) -> Promise<Dynamic> {
        super::ext_host_commands::ExtHostCommands::execute_command(self, id, args)
    }
}

/// `IExtHostTesting`, só o `getMetadataForRun(run)`: o par `(taskId, runId)`.
pub trait IExtHostDebugTesting {
    fn get_metadata_for_run(&self, run: &Rc<super::ext_host_testing::TestRun>) -> Option<(String, String)>;
}

impl IExtHostDebugTesting for super::ext_host_testing::ExtHostTesting {
    fn get_metadata_for_run(&self, run: &Rc<super::ext_host_testing::TestRun>) -> Option<(String, String)> {
        super::ext_host_testing::ExtHostTesting::get_metadata_for_run(self, run)
    }
}

/// `ISignService`, só o `sign(value)`.
pub trait ISignService {
    fn sign(&self, value: &str) -> Promise<String>;
}

/// Os serviços do construtor de `ExtHostDebugServiceBase`.
pub struct ExtHostDebugServiceDeps {
    pub rpc_service: Rc<dyn IMainContext>,
    pub workspace_service: Rc<dyn IExtHostWorkspaceProvider>,
    pub extension_service: Rc<dyn IExtHostDebugExtensionService>,
    pub variable_resolver: Rc<dyn IExtHostVariableResolverProvider>,
    pub commands: Rc<dyn IExtHostDebugCommands>,
    pub testing: Rc<dyn IExtHostDebugTesting>,
}

/// Os métodos `protected` de `ExtHostDebugServiceBase` que as subclasses sobrescrevem. As respostas padrão são as da
/// classe base.
pub trait ExtHostDebugServiceHooks {
    /// `createDebugAdapter(adapter, session)`.
    fn create_debug_adapter(&self, adapter: &DebugAdapterDescriptor, _session: &ExtHostDebugSession) -> Option<Rc<dyn AbstractDebugAdapter>> {
        match adapter {
            DebugAdapterDescriptor::Inline(inline) => {
                let adapter: Rc<dyn AbstractDebugAdapter> = DirectDebugAdapter::new(inline.implementation.clone());
                Some(adapter)
            }
            _ => None,
        }
    }

    /// `createSignService()`.
    fn create_sign_service(&self) -> Option<Rc<dyn ISignService>> {
        None
    }

    /// `convertExecutableToDto(x)`.
    fn convert_executable_to_dto(&self, x: &DebugAdapterExecutable) -> IDebugAdapterExecutable {
        IDebugAdapterExecutable { command: x.command.clone(), args: x.args.clone(), options: x.options.clone() }
    }

    /// `convertServerToDto(x)`.
    fn convert_server_to_dto(&self, x: &DebugAdapterServer) -> IDebugAdapterServer {
        IDebugAdapterServer { port: x.port, host: x.host.clone() }
    }

    /// `convertPipeServerToDto(x)`.
    fn convert_pipe_server_to_dto(&self, x: &DebugAdapterNamedPipeServer) -> IDebugAdapterNamedPipeServer {
        IDebugAdapterNamedPipeServer { path: x.path.clone() }
    }

    /// `convertImplementationToDto(x)`.
    fn convert_implementation_to_dto(&self, _x: &DebugAdapterInlineImplementation) -> IDebugAdapterImpl {
        IDebugAdapterImpl
    }

    /// `daExecutableFromPackage(session, extensionRegistry)`.
    fn da_executable_from_package(&self, _session: &ExtHostDebugSession, _extension_registry: &ExtensionDescriptionRegistry) -> Option<DebugAdapterExecutable> {
        None
    }
}

/// O comportamento da classe base (e do `WorkerExtHostDebugService`, que não sobrescreve nada).
pub struct DefaultExtHostDebugServiceHooks;

impl ExtHostDebugServiceHooks for DefaultExtHostDebugServiceHooks {}

// ---- sessão

struct SessionState {
    debug_service_proxy: Rc<dyn MainThreadDebugServiceShape>,
    id: DebugSessionUuid,
    r#type: String,
    name: RefCell<String>,
    workspace_folder: Option<Rc<WorkspaceFolder>>,
    configuration: IConfig,
    parent_session: Option<Rc<DebugSession>>,
}

/// `vscode.DebugSession`: o objeto que `ExtHostDebugSession.api` devolve.
pub struct DebugSession {
    state: Rc<SessionState>,
}

impl DebugSession {
    pub fn id(&self) -> String {
        self.state.id.clone()
    }

    /// `type`.
    pub fn session_type(&self) -> String {
        self.state.r#type.clone()
    }

    /// `get name()`.
    pub fn name(&self) -> String {
        self.state.name.borrow().clone()
    }

    /// `set name(name)`: avisa a thread principal.
    pub fn set_name(&self, name: &str) {
        *self.state.name.borrow_mut() = name.to_owned();
        self.state.debug_service_proxy.rpc_set_debug_session_name(&self.state.id, name);
    }

    pub fn parent_session(&self) -> Option<Rc<DebugSession>> {
        self.state.parent_session.clone()
    }

    pub fn workspace_folder(&self) -> Option<Rc<WorkspaceFolder>> {
        self.state.workspace_folder.clone()
    }

    pub fn configuration(&self) -> IConfig {
        self.state.configuration.clone()
    }

    /// `customRequest(command, args)`.
    pub fn custom_request(&self, command: &str, args: Option<Value>) -> Promise<Option<Value>> {
        self.state.debug_service_proxy.rpc_custom_debug_adapter_request(&self.state.id, command, args)
    }

    /// `getDebugProtocolBreakpoint(breakpoint)`.
    pub fn get_debug_protocol_breakpoint(&self, breakpoint: &Breakpoint) -> Promise<Option<Value>> {
        self.state.debug_service_proxy.rpc_get_debug_protocol_breakpoint(&self.state.id, &breakpoint.id())
    }
}

/// `ExtHostDebugSession`.
pub struct ExtHostDebugSession {
    state: Rc<SessionState>,
    api_session: RefCell<Option<Rc<DebugSession>>>,
}

impl ExtHostDebugSession {
    pub fn new(
        debug_service_proxy: Rc<dyn MainThreadDebugServiceShape>,
        id: &str,
        r#type: &str,
        name: &str,
        workspace_folder: Option<Rc<WorkspaceFolder>>,
        configuration: IConfig,
        parent_session: Option<Rc<DebugSession>>,
    ) -> Rc<ExtHostDebugSession> {
        Rc::new(ExtHostDebugSession {
            state: Rc::new(SessionState {
                debug_service_proxy,
                id: id.to_owned(),
                r#type: r#type.to_owned(),
                name: RefCell::new(name.to_owned()),
                workspace_folder,
                configuration,
                parent_session,
            }),
            api_session: RefCell::new(None),
        })
    }

    /// `get api()`: o objeto da API, criado no primeiro uso.
    pub fn api(&self) -> Rc<DebugSession> {
        self.api_session.borrow_mut().get_or_insert_with(|| Rc::new(DebugSession { state: self.state.clone() })).clone()
    }

    pub fn id(&self) -> String {
        self.state.id.clone()
    }

    /// `type`.
    pub fn session_type(&self) -> String {
        self.state.r#type.clone()
    }

    /// `_acceptNameChanged(name)`.
    pub fn accept_name_changed(&self, name: &str) {
        *self.state.name.borrow_mut() = name.to_owned();
    }

    pub fn configuration(&self) -> IConfig {
        self.state.configuration.clone()
    }
}

/// `vscode.DebugConsole`.
pub struct DebugConsole {
    proxy: Rc<dyn MainThreadDebugServiceShape>,
}

impl DebugConsole {
    /// `append(value)`.
    pub fn append(&self, value: &str) {
        self.proxy.rpc_append_debug_console(value);
    }

    /// `appendLine(value)`.
    pub fn append_line(&self, value: &str) {
        self.append(&format!("{value}\n"));
    }
}

/// `ExtHostDebugConsole`.
pub struct ExtHostDebugConsole {
    pub value: Rc<DebugConsole>,
}

impl ExtHostDebugConsole {
    pub fn new(proxy: Rc<dyn MainThreadDebugServiceShape>) -> Self {
        Self { value: Rc::new(DebugConsole { proxy }) }
    }
}

// ---- adaptador direto

/// `DirectDebugAdapter`: call directly into a debug adapter implementation.
pub struct DirectDebugAdapter {
    state: Rc<DebugAdapterState>,
    implementation: Rc<dyn super::ext_host_debug_types::DebugAdapter>,
    message_listener: DisposableRef,
}

impl DirectDebugAdapter {
    pub fn new(implementation: Rc<dyn super::ext_host_debug_types::DebugAdapter>) -> Rc<DirectDebugAdapter> {
        Rc::new_cyclic(|this: &Weak<DirectDebugAdapter>| {
            let weak = this.clone();
            let message_listener = implementation.on_did_send_message().listen(move |message: &ProtocolMessage| {
                if let Some(adapter) = weak.upgrade() {
                    adapter.accept_message(message.clone());
                }
            });
            DirectDebugAdapter { state: DebugAdapterState::new(), implementation, message_listener }
        })
    }
}

impl IDisposable for DirectDebugAdapter {
    fn dispose(&self) {
        self.message_listener.dispose();
        self.state.dispose();
    }
}

impl AbstractDebugAdapter for DirectDebugAdapter {
    fn state(&self) -> &Rc<DebugAdapterState> {
        &self.state
    }

    fn start_session(&self) -> Promise<()> {
        Promise::resolved(())
    }

    fn send_message(&self, message: &ProtocolMessage) {
        self.implementation.handle_message(message);
    }

    fn stop_session(&self) -> Promise<()> {
        self.dispose();
        self.implementation.dispose();
        Promise::resolved(())
    }
}

// ---- rastreadores

/// `MultiTracker`.
struct MultiTracker {
    trackers: Vec<Rc<dyn DebugAdapterTracker>>,
}

impl DebugAdapterTracker for MultiTracker {
    fn on_will_start_session(&self) {
        self.trackers.iter().for_each(|t| t.on_will_start_session());
    }

    fn on_will_receive_message(&self, message: &ProtocolMessage) {
        self.trackers.iter().for_each(|t| t.on_will_receive_message(message));
    }

    fn on_did_send_message(&self, message: &ProtocolMessage) {
        self.trackers.iter().for_each(|t| t.on_did_send_message(message));
    }

    fn on_will_stop_session(&self) {
        self.trackers.iter().for_each(|t| t.on_will_stop_session());
    }

    fn on_error(&self, error: &Error) {
        self.trackers.iter().for_each(|t| t.on_error(error));
    }

    fn on_exit(&self, code: Option<i32>, signal: Option<&str>) {
        self.trackers.iter().for_each(|t| t.on_exit(code, signal));
    }
}

struct ConfigProviderTuple {
    r#type: String,
    handle: u32,
    provider: Rc<dyn DebugConfigurationProvider>,
}

struct DescriptorFactoryTuple {
    r#type: String,
    handle: u32,
    factory: Rc<dyn DebugAdapterDescriptorFactory>,
}

struct TrackerFactoryTuple {
    r#type: String,
    factory: Rc<dyn DebugAdapterTrackerFactory>,
}

// ---- visualização

/// O valor de `_debugVisualizationElements`: `IDisposable & { provider, item, sessionIds, children? }`.
struct VisualizationElement {
    provider: String,
    item: Rc<DebugTreeItem>,
    session_ids: RefCell<HashSet<String>>,
    children: RefCell<Option<Vec<u64>>>,
    /// O `_debugVisualizationTreeItemIds`, de onde o `dispose` tira o item.
    item_ids: Rc<RefCell<HashMap<usize, u64>>>,
}

impl VisualizationElement {
    /// A chave do item em `_debugVisualizationTreeItemIds` (a identidade do objeto).
    fn item_key(&self) -> usize {
        Rc::as_ptr(&self.item) as usize
    }
}

impl IDisposable for VisualizationElement {
    fn dispose(&self) {
        self.item_ids.borrow_mut().remove(&self.item_key());
    }
}

struct VisualizerEntry {
    v: DebugVisualization,
    provider: Rc<dyn DebugVisualizationProvider>,
    extension_id: String,
}

// ---- utilitários

/// `asPromise(() => value)` para um retorno `T | Promise<T>`: o erro lançado rejeita.
fn as_promise_maybe<T: Clone + 'static>(callback: impl FnOnce() -> MaybePromise<T>) -> Promise<T> {
    Promise::with_executor(|resolver| resolver.resolve_maybe(callback()))
}

/// `a === b` para objetos de trait compartilhados.
fn same_object<T: ?Sized>(a: &Rc<T>, b: &Rc<T>) -> bool {
    std::ptr::addr_eq(Rc::as_ptr(a), Rc::as_ptr(b))
}

/// `encodeURIComponent(text)`.
fn encode_component(text: &str) -> String {
    JsString::from_units(encode_uri_component(&text.encode_utf16().collect::<Vec<u16>>())).to_string_lossy()
}

/// `typeof config.type`: o `type` da configuração (texto vazio quando ausente).
fn config_type(config: &IConfig) -> String {
    config.get("type").and_then(Value::as_str).unwrap_or_default().to_owned()
}

/// Um `Disposable` da API que roda `on_dispose`.
fn disposable(on_dispose: impl Fn() + 'static) -> DisposableRef {
    Rc::new(types::Disposable::new(on_dispose))
}

fn empty_disposable() -> DisposableRef {
    Rc::new(types::Disposable::new(|| {}))
}

fn register_emitter<T: 'static>(base: &Disposable) -> Rc<Emitter<T>> {
    let emitter: Rc<Emitter<T>> = Emitter::new();
    base.register(emitter.clone());
    emitter
}

fn folder_to_platform(folder: &WorkspaceFolder) -> PlatformWorkspaceFolder {
    PlatformWorkspaceFolder::new(folder.uri().clone(), folder.name(), folder.index(), None)
}

// ---- serviço

/// `ExtHostDebugServiceBase`.
pub struct ExtHostDebugServiceBase {
    this: Weak<ExtHostDebugServiceBase>,
    base: Rc<Disposable>,
    hooks: Rc<dyn ExtHostDebugServiceHooks>,
    workspace_service: Rc<dyn IExtHostWorkspaceProvider>,
    extension_service: Rc<dyn IExtHostDebugExtensionService>,
    variable_resolver: Rc<dyn IExtHostVariableResolverProvider>,
    commands: Rc<dyn IExtHostDebugCommands>,
    testing: Rc<dyn IExtHostDebugTesting>,

    config_provider_handle_counter: Cell<u32>,
    config_providers: RefCell<Vec<ConfigProviderTuple>>,
    adapter_factory_handle_counter: Cell<u32>,
    adapter_factories: RefCell<Vec<DescriptorFactoryTuple>>,
    tracker_factories: RefCell<Vec<TrackerFactoryTuple>>,

    debug_service_proxy: Rc<dyn MainThreadDebugServiceShape>,
    debug_sessions: RefCell<HashMap<DebugSessionUuid, Rc<ExtHostDebugSession>>>,

    on_did_start_debug_session: Rc<Emitter<Rc<DebugSession>>>,
    on_did_terminate_debug_session: Rc<Emitter<Rc<DebugSession>>>,
    on_did_change_active_debug_session: Rc<Emitter<Option<Rc<DebugSession>>>>,
    active_debug_session: RefCell<Option<Rc<ExtHostDebugSession>>>,
    on_did_receive_debug_session_custom_event: Rc<Emitter<DebugSessionCustomEvent>>,
    active_debug_console: ExtHostDebugConsole,
    breakpoints: RefCell<IndexMap<String, Rc<Breakpoint>>>,
    on_did_change_breakpoints: Rc<Emitter<BreakpointsChangeEvent>>,
    active_stack_item: RefCell<Option<DebugStackItem>>,
    on_did_change_active_stack_item: Rc<Emitter<Option<DebugStackItem>>>,

    debug_adapters: RefCell<HashMap<u32, Rc<dyn AbstractDebugAdapter>>>,
    debug_adapters_trackers: RefCell<HashMap<u32, Rc<dyn DebugAdapterTracker>>>,

    debug_visualization_tree_item_ids_counter: Cell<u64>,
    debug_visualization_providers: RefCell<HashMap<String, Rc<dyn DebugVisualizationProvider>>>,
    debug_visualization_trees: RefCell<HashMap<String, Rc<dyn DebugVisualizationTree>>>,
    debug_visualization_tree_item_ids: Rc<RefCell<HashMap<usize, u64>>>,
    debug_visualization_elements: Rc<DisposableMap<u64, VisualizationElement>>,

    sign_service: RefCell<Option<Rc<dyn ISignService>>>,

    visualizers: RefCell<HashMap<u64, VisualizerEntry>>,
    visualizer_id_counter: Cell<u64>,

    telemetry_proxy: Rc<dyn MainThreadTelemetryShape>,
}

impl ExtHostDebugServiceBase {
    pub fn new(deps: ExtHostDebugServiceDeps, hooks: Rc<dyn ExtHostDebugServiceHooks>) -> Rc<ExtHostDebugServiceBase> {
        let base = Disposable::new();
        let debug_service_proxy = deps.rpc_service.get_proxy(&MainContext::MAIN_THREAD_DEBUG_SERVICE);
        let elements: Rc<DisposableMap<u64, VisualizationElement>> = DisposableMap::new();
        base.register(elements.clone());
        let service = Rc::new_cyclic(|this: &Weak<ExtHostDebugServiceBase>| ExtHostDebugServiceBase {
            this: this.clone(),
            hooks,
            workspace_service: deps.workspace_service,
            extension_service: deps.extension_service,
            variable_resolver: deps.variable_resolver,
            commands: deps.commands,
            testing: deps.testing,
            config_provider_handle_counter: Cell::new(0),
            config_providers: RefCell::new(Vec::new()),
            adapter_factory_handle_counter: Cell::new(0),
            adapter_factories: RefCell::new(Vec::new()),
            tracker_factories: RefCell::new(Vec::new()),
            active_debug_console: ExtHostDebugConsole::new(debug_service_proxy.clone()),
            debug_service_proxy,
            debug_sessions: RefCell::new(HashMap::new()),
            on_did_start_debug_session: register_emitter(&base),
            on_did_terminate_debug_session: register_emitter(&base),
            on_did_change_active_debug_session: register_emitter(&base),
            active_debug_session: RefCell::new(None),
            on_did_receive_debug_session_custom_event: register_emitter(&base),
            breakpoints: RefCell::new(IndexMap::new()),
            on_did_change_breakpoints: register_emitter(&base),
            active_stack_item: RefCell::new(None),
            on_did_change_active_stack_item: register_emitter(&base),
            debug_adapters: RefCell::new(HashMap::new()),
            debug_adapters_trackers: RefCell::new(HashMap::new()),
            debug_visualization_tree_item_ids_counter: Cell::new(0),
            debug_visualization_providers: RefCell::new(HashMap::new()),
            debug_visualization_trees: RefCell::new(HashMap::new()),
            debug_visualization_tree_item_ids: Rc::new(RefCell::new(HashMap::new())),
            debug_visualization_elements: elements,
            sign_service: RefCell::new(None),
            visualizers: RefCell::new(HashMap::new()),
            visualizer_id_counter: Cell::new(0),
            telemetry_proxy: deps.rpc_service.get_proxy(&MainContext::MAIN_THREAD_TELEMETRY),
            base,
        });

        let this = service.clone();
        service.extension_service.get_extension_registry().then(move |extension_registry| {
            let listener_this = this.clone();
            let listener_registry = extension_registry.clone();
            let listener = extension_registry.on_did_change().listen(move |_| listener_this.register_all_debug_types(&*listener_registry));
            this.base.register(listener);
            this.register_all_debug_types(&*extension_registry);
        });

        service
    }

    /// O `Rc` deste serviço, para as operações assíncronas que precisam dele depois do retorno.
    fn rc(&self) -> Rc<ExtHostDebugServiceBase> {
        self.this.upgrade().expect("o ExtHostDebugService está vivo enquanto um método dele roda")
    }

    // ---- API de visualização

    /// `registerDebugVisualizationTree(manifest, id, provider)`.
    pub fn register_debug_visualization_tree(&self, manifest: &ExtensionDescription, id: &str, provider: Rc<dyn DebugVisualizationTree>) -> Result<DisposableRef, Error> {
        let extension_id = ExtensionIdentifier::to_key(&manifest.identifier.value);
        let key = Self::extension_vis_key(&extension_id, id);
        if self.debug_visualization_providers.borrow().contains_key(&key) {
            return Err(Error::new(format!("A debug visualization provider with id '{id}' is already registered")));
        }

        self.debug_visualization_trees.borrow_mut().insert(key.clone(), provider.clone());
        self.debug_service_proxy.rpc_register_debug_visualizer_tree(&key, provider.has_edit_item());
        let this = self.rc();
        Ok(disposable(move || {
            this.debug_service_proxy.rpc_unregister_debug_visualizer_tree(&key);
            this.debug_visualization_trees.borrow_mut().remove(&key);
        }))
    }

    /// `registerDebugVisualizationProvider(manifest, id, provider)`.
    pub fn register_debug_visualization_provider(&self, manifest: &ExtensionDescription, id: &str, provider: Rc<dyn DebugVisualizationProvider>) -> Result<DisposableRef, Error> {
        let contributes_id = manifest
            .manifest
            .contributes
            .as_ref()
            .and_then(|contributes| contributes.debug_visualizers.as_ref())
            .is_some_and(|visualizers| visualizers.iter().any(|visualizer| visualizer.id.as_deref() == Some(id)));
        if !contributes_id {
            return Err(Error::new(format!("Extensions may only call registerDebugVisualizationProvider() for renderers they contribute (got {id})")));
        }

        let extension_id = ExtensionIdentifier::to_key(&manifest.identifier.value);
        let key = Self::extension_vis_key(&extension_id, id);
        if self.debug_visualization_providers.borrow().contains_key(&key) {
            return Err(Error::new(format!("A debug visualization provider with id '{id}' is already registered")));
        }

        self.debug_visualization_providers.borrow_mut().insert(key.clone(), provider);
        self.debug_service_proxy.rpc_register_debug_visualizer(&extension_id, id);
        let this = self.rc();
        let id = id.to_owned();
        Ok(disposable(move || {
            this.debug_service_proxy.rpc_unregister_debug_visualizer(&extension_id, &id);
            this.debug_visualization_providers.borrow_mut().remove(&key);
        }))
    }

    fn convert_visualizer_tree_item(&self, tree_id: &str, item: Rc<DebugTreeItem>, session_ids: Vec<String>) -> IDebugVisualizationTreeItem {
        let key = Rc::as_ptr(&item) as usize;
        let existing = self.debug_visualization_tree_item_ids.borrow().get(&key).copied();
        let id = match existing {
            Some(id) => id,
            None => {
                let id = self.debug_visualization_tree_item_ids_counter.get();
                self.debug_visualization_tree_item_ids_counter.set(id + 1);
                self.debug_visualization_tree_item_ids.borrow_mut().insert(key, id);
                self.debug_visualization_elements.set(
                    id,
                    Rc::new(VisualizationElement {
                        provider: tree_id.to_owned(),
                        item: item.clone(),
                        session_ids: RefCell::new(HashSet::new()),
                        children: RefCell::new(None),
                        item_ids: self.debug_visualization_tree_item_ids.clone(),
                    }),
                    false,
                );
                id
            }
        };
        let entry = self.debug_visualization_elements.get(&id).expect("o elemento acabou de ser guardado");
        entry.session_ids.borrow_mut().extend(session_ids);

        // `Convert.DebugTreeItem.from(item, id)`
        IDebugVisualizationTreeItem {
            id,
            label: item.label.clone(),
            description: item.description.clone(),
            can_edit: item.can_edit,
            collapsible_state: item.collapsible_state.unwrap_or(DebugTreeItemCollapsibleState::None),
            context_value: item.context_value.clone(),
        }
    }

    fn hydrate_visualization_context(&self, context: &IDebugVisualizationContext) -> Option<DebugVisualizationContext> {
        let session = self.debug_sessions.borrow().get(&context.session_id).cloned()?;
        Some(DebugVisualizationContext {
            session: session.api(),
            variable: context.variable.clone(),
            container_id: context.container_id,
            frame_id: context.frame_id,
            thread_id: context.thread_id,
        })
    }

    fn extension_vis_key(extension_id: &str, id: &str) -> String {
        format!("{extension_id}\0{id}")
    }

    fn serialize_visualization(extension_id: &str, viz: &Option<DebugVisualizationTarget>) -> Option<MainThreadDebugVisualization> {
        match viz.as_ref()? {
            DebugVisualizationTarget::Command(_) => Some(MainThreadDebugVisualization::Command),
            DebugVisualizationTarget::Tree { tree_id } => Some(MainThreadDebugVisualization::Tree { id: format!("{extension_id}\0{tree_id}") }),
        }
    }

    /// `getIconPathOrClass(icon)`.
    fn get_icon_path_or_class(icon: &IconPath) -> (Option<DebugVisualizationIconPath>, Option<String>) {
        match icon {
            IconPath::ThemeIcon(theme_icon) => (None, Some(theme_icon.as_class_name())),
            IconPath::Uri(uri) => (Some(DebugVisualizationIconPath { dark: uri.clone(), light: Some(uri.clone()) }), None),
            IconPath::LightDark { light, dark } => (Some(DebugVisualizationIconPath { dark: dark.clone(), light: Some(light.clone()) }), None),
        }
    }

    // ---- API da extensão

    /// `asDebugSourceUri(src, session)`.
    pub fn as_debug_source_uri(&self, source: &Value, session: Option<&DebugSession>) -> Result<URI, Error> {
        let source_reference = source.get("sourceReference").and_then(Value::as_f64).filter(|reference| *reference > 0.0);
        let path = source.get("path").and_then(Value::as_str).filter(|path| !path.is_empty());
        if let Some(reference) = source_reference {
            // src can be retrieved via DAP's "source" request
            let mut debug = format!("debug:{}", encode_component(path.unwrap_or("")));
            let mut sep = '?';
            if let Some(session) = session {
                debug.push_str(&format!("{sep}session={}", encode_component(&session.id())));
                sep = '&';
            }
            debug.push_str(&format!("{sep}ref={}", number_to_js_string(reference)));
            Ok(URI::parse(debug.as_str(), false))
        } else if let Some(path) = path {
            // src is just a local file path
            Ok(URI::file(path, host_platform()))
        } else {
            Err(Error::new("cannot create uri from DAP 'source' object; properties 'path' and 'sourceReference' are both missing."))
        }
    }

    fn register_all_debug_types(&self, extension_registry: &dyn IReadOnlyExtensionDescriptionRegistry) {
        let mut debug_types: Vec<String> = Vec::new();

        for ed in extension_registry.get_all_extension_descriptions() {
            if let Some(contributes) = &ed.manifest.contributes
                && let Some(debuggers) = &contributes.debuggers
            {
                for dbg in debuggers {
                    if is_debugger_main_contribution(dbg) {
                        debug_types.push(dbg.r#type.clone().unwrap_or_default());
                    }
                }
            }
        }

        self.debug_service_proxy.rpc_register_debug_types(debug_types);
    }

    // ---- extension debug API

    pub fn active_stack_item(&self) -> Option<DebugStackItem> {
        self.active_stack_item.borrow().clone()
    }

    pub fn on_did_change_active_stack_item(&self) -> Event<Option<DebugStackItem>> {
        self.on_did_change_active_stack_item.event()
    }

    pub fn on_did_change_breakpoints(&self) -> Event<BreakpointsChangeEvent> {
        self.on_did_change_breakpoints.event()
    }

    pub fn on_did_start_debug_session(&self) -> Event<Rc<DebugSession>> {
        self.on_did_start_debug_session.event()
    }

    pub fn on_did_terminate_debug_session(&self) -> Event<Rc<DebugSession>> {
        self.on_did_terminate_debug_session.event()
    }

    pub fn on_did_change_active_debug_session(&self) -> Event<Option<Rc<DebugSession>>> {
        self.on_did_change_active_debug_session.event()
    }

    pub fn on_did_receive_debug_session_custom_event(&self) -> Event<DebugSessionCustomEvent> {
        self.on_did_receive_debug_session_custom_event.event()
    }

    pub fn active_debug_session(&self) -> Option<Rc<DebugSession>> {
        self.active_debug_session.borrow().as_ref().map(|session| session.api())
    }

    pub fn active_debug_console(&self) -> Rc<DebugConsole> {
        self.active_debug_console.value.clone()
    }

    /// `get breakpoints()`.
    pub fn breakpoints(&self) -> Vec<Rc<Breakpoint>> {
        self.breakpoints.borrow().values().cloned().collect()
    }

    fn fire_breakpoint_changes(&self, added: Vec<Rc<Breakpoint>>, removed: Vec<Rc<Breakpoint>>, changed: Vec<Rc<Breakpoint>>) {
        if !added.is_empty() || !removed.is_empty() || !changed.is_empty() {
            self.on_did_change_breakpoints.fire(BreakpointsChangeEvent { added, removed, changed });
        }
    }

    /// `addBreakpoints(breakpoints0)`.
    pub fn add_breakpoints(&self, breakpoints0: Vec<Rc<Breakpoint>>) -> Promise<()> {
        // filter only new breakpoints
        let breakpoints: Vec<Rc<Breakpoint>> = breakpoints0
            .into_iter()
            .filter(|bp| {
                let id = bp.id();
                let mut known = self.breakpoints.borrow_mut();
                if known.contains_key(&id) {
                    false
                } else {
                    known.insert(id, bp.clone());
                    true
                }
            })
            .collect();

        // send notification for added breakpoints
        self.fire_breakpoint_changes(breakpoints.clone(), Vec::new(), Vec::new());

        // convert added breakpoints to DTOs
        let mut dtos: Vec<RegisterBreakpointDto> = Vec::new();
        let mut map: HashMap<String, usize> = HashMap::new();
        for bp in &breakpoints {
            let base = || IBreakpointDto {
                id: Some(bp.id()),
                enabled: bp.enabled(),
                condition: bp.condition(),
                hit_condition: bp.hit_condition(),
                log_message: bp.log_message(),
                mode: bp.mode(),
            };
            match bp.kind() {
                BreakpointKind::Source { location } => {
                    let uri = location.uri.clone().expect("a SourceBreakpoint tem uri");
                    let start = location.range.as_ref().map(|range| range.start());
                    let index = *map.entry(uri.to_string(false).to_string_lossy()).or_insert_with(|| {
                        dtos.push(RegisterBreakpointDto::SourceMulti(ISourceMultiBreakpointDto { uri: uri.clone(), lines: Vec::new() }));
                        dtos.len() - 1
                    });
                    if let RegisterBreakpointDto::SourceMulti(dto) = &mut dtos[index] {
                        dto.lines.push(SourceMultiBreakpointLine {
                            id: bp.id(),
                            enabled: bp.enabled(),
                            condition: bp.condition(),
                            hit_condition: bp.hit_condition(),
                            log_message: bp.log_message(),
                            line: start.map(|start| start.line() as i64).unwrap_or_default(),
                            character: start.map(|start| start.character() as i64).unwrap_or_default(),
                            mode: bp.mode(),
                        });
                    }
                }
                BreakpointKind::Function { function_name } => {
                    dtos.push(RegisterBreakpointDto::Function(IFunctionBreakpointDto { base: base(), function_name }));
                }
                // O `addBreakpoints` do original só converte `SourceBreakpoint` e `FunctionBreakpoint`.
                BreakpointKind::Data { .. } => {}
            }
        }

        // send DTOs to VS Code
        self.debug_service_proxy.rpc_register_breakpoints(dtos)
    }

    /// `removeBreakpoints(breakpoints0)`.
    pub fn remove_breakpoints(&self, breakpoints0: Vec<Rc<Breakpoint>>) -> Promise<()> {
        // remove from array
        let breakpoints: Vec<Rc<Breakpoint>> = breakpoints0.into_iter().filter(|b| self.breakpoints.borrow_mut().shift_remove(&b.id()).is_some()).collect();

        // send notification
        self.fire_breakpoint_changes(Vec::new(), breakpoints.clone(), Vec::new());

        // unregister with VS Code
        let ids_of = |matches: fn(&BreakpointKind) -> bool| -> Vec<String> { breakpoints.iter().filter(|bp| matches(&bp.kind())).map(|bp| bp.id()).collect() };
        let ids = ids_of(|kind| matches!(kind, BreakpointKind::Source { .. }));
        let fids = ids_of(|kind| matches!(kind, BreakpointKind::Function { .. }));
        let dids = ids_of(|kind| matches!(kind, BreakpointKind::Data { .. }));
        self.debug_service_proxy.rpc_unregister_breakpoints(ids, fids, dids)
    }

    /// `startDebugging(folder, nameOrConfig, options)`.
    pub fn start_debugging(&self, folder: Option<Rc<WorkspaceFolder>>, name_or_config: NameOrConfig, options: DebugSessionOptions) -> Promise<bool> {
        let test_run_meta = options.test_run.as_ref().and_then(|run| self.testing.get_metadata_for_run(run));

        self.debug_service_proxy.rpc_start_debugging(
            folder.map(|folder| folder.uri().clone()),
            name_or_config,
            IStartDebuggingOptions {
                parent_session_id: options.parent_session.as_ref().map(|session| session.id()),
                lifecycle_managed_by_parent: options.lifecycle_managed_by_parent,
                repl: Some(if options.console_mode == Some(DebugConsoleMode::MergeWithParent) { IDebugSessionReplMode::MergeWithParent } else { IDebugSessionReplMode::Separate }),
                no_debug: options.no_debug,
                compact: options.compact,
                suppress_save_before_start: options.suppress_save_before_start,
                test_run: test_run_meta.map(|(task_id, run_id)| IDebugTestRunReference { run_id, task_id }),

                // Check debugUI for back-compat, #147264
                suppress_debug_statusbar: options.suppress_debug_statusbar.or(options.debug_ui_simple),
                suppress_debug_toolbar: options.suppress_debug_toolbar.or(options.debug_ui_simple),
                suppress_debug_view: options.suppress_debug_view.or(options.debug_ui_simple),
            },
        )
    }

    /// `stopDebugging(session?)`.
    pub fn stop_debugging(&self, session: Option<&DebugSession>) -> Promise<()> {
        let session_id = session.map(|session| session.id());
        self.debug_service_proxy.rpc_stop_debugging(session_id.as_deref())
    }

    /// `registerDebugConfigurationProvider(type, provider, trigger)`: sem provedor, devolve um `Disposable` vazio.
    pub fn register_debug_configuration_provider(&self, r#type: &str, provider: Option<Rc<dyn DebugConfigurationProvider>>, trigger: DebugConfigurationProviderTriggerKind) -> DisposableRef {
        let Some(provider) = provider else {
            return empty_disposable();
        };

        let handle = self.config_provider_handle_counter.get();
        self.config_provider_handle_counter.set(handle + 1);
        self.config_providers.borrow_mut().push(ConfigProviderTuple { r#type: r#type.to_owned(), handle, provider: provider.clone() });

        // O `Promise` devolvido só confirma o registro.
        let _ = self.debug_service_proxy.rpc_register_debug_configuration_provider(
            r#type,
            trigger,
            provider.has_provide_debug_configurations(),
            provider.has_resolve_debug_configuration(),
            provider.has_resolve_debug_configuration_with_substituted_variables(),
            handle,
        );

        let this = self.rc();
        disposable(move || {
            this.config_providers.borrow_mut().retain(|p| !same_object(&p.provider, &provider)); // remove
            this.debug_service_proxy.rpc_unregister_debug_configuration_provider(handle);
        })
    }

    /// `registerDebugAdapterDescriptorFactory(extension, type, factory)`.
    pub fn register_debug_adapter_descriptor_factory(
        &self,
        extension: &ExtensionDescription,
        r#type: &str,
        factory: Option<Rc<dyn DebugAdapterDescriptorFactory>>,
    ) -> Result<DisposableRef, Error> {
        let Some(factory) = factory else {
            return Ok(empty_disposable());
        };

        // a DebugAdapterDescriptorFactory can only be registered in the extension that contributes the debugger
        if !Self::defines_debug_type(extension, r#type) {
            return Err(Error::new(format!("a DebugAdapterDescriptorFactory can only be registered from the extension that defines the '{}' debugger.", r#type)));
        }

        // make sure that only one factory for this type is registered
        if self.get_adapter_descriptor_factory_by_type(r#type).is_some() {
            return Err(Error::new("a DebugAdapterDescriptorFactory can only be registered once per a type."));
        }

        let handle = self.adapter_factory_handle_counter.get();
        self.adapter_factory_handle_counter.set(handle + 1);
        self.adapter_factories.borrow_mut().push(DescriptorFactoryTuple { r#type: r#type.to_owned(), handle, factory: factory.clone() });

        let _ = self.debug_service_proxy.rpc_register_debug_adapter_descriptor_factory(r#type, handle);

        let this = self.rc();
        Ok(disposable(move || {
            this.adapter_factories.borrow_mut().retain(|p| !same_object(&p.factory, &factory)); // remove
            this.debug_service_proxy.rpc_unregister_debug_adapter_descriptor_factory(handle);
        }))
    }

    /// `registerDebugAdapterTrackerFactory(type, factory)`.
    pub fn register_debug_adapter_tracker_factory(&self, r#type: &str, factory: Option<Rc<dyn DebugAdapterTrackerFactory>>) -> DisposableRef {
        let Some(factory) = factory else {
            return empty_disposable();
        };

        self.tracker_factories.borrow_mut().push(TrackerFactoryTuple { r#type: r#type.to_owned(), factory: factory.clone() });

        let this = self.rc();
        disposable(move || {
            this.tracker_factories.borrow_mut().retain(|p| !same_object(&p.factory, &factory)); // remove
        })
    }

    // ---- hooks e auxiliares

    /// `createDebugAdapter(adapter, session)` (`protected` no original).
    pub fn create_debug_adapter(&self, adapter: &DebugAdapterDescriptor, session: &ExtHostDebugSession) -> Option<Rc<dyn AbstractDebugAdapter>> {
        self.hooks.create_debug_adapter(adapter, session)
    }

    fn convert_to_dto(&self, x: &DebugAdapterDescriptor) -> IAdapterDescriptor {
        match x {
            DebugAdapterDescriptor::Executable(executable) => IAdapterDescriptor::Executable(self.hooks.convert_executable_to_dto(executable)),
            DebugAdapterDescriptor::Server(server) => IAdapterDescriptor::Server(self.hooks.convert_server_to_dto(server)),
            DebugAdapterDescriptor::NamedPipeServer(pipe) => IAdapterDescriptor::PipeServer(self.hooks.convert_pipe_server_to_dto(pipe)),
            DebugAdapterDescriptor::Inline(inline) => IAdapterDescriptor::Implementation(self.hooks.convert_implementation_to_dto(inline)),
        }
    }

    fn get_adapter_descriptor_factory_by_type(&self, r#type: &str) -> Option<Rc<dyn DebugAdapterDescriptorFactory>> {
        self.adapter_factories.borrow().iter().find(|p| p.r#type == r#type).map(|p| p.factory.clone())
    }

    fn get_adapter_descriptor_factory_by_handle(&self, handle: u32) -> Option<Rc<dyn DebugAdapterDescriptorFactory>> {
        self.adapter_factories.borrow().iter().find(|p| p.handle == handle).map(|p| p.factory.clone())
    }

    fn get_config_provider_by_handle(&self, handle: u32) -> Option<Rc<dyn DebugConfigurationProvider>> {
        self.config_providers.borrow().iter().find(|p| p.handle == handle).map(|p| p.provider.clone())
    }

    fn defines_debug_type(ed: &ExtensionDescription, r#type: &str) -> bool {
        if let Some(contributes) = &ed.manifest.contributes
            && let Some(debuggers) = &contributes.debuggers
        {
            for dbg in debuggers {
                // only debugger contributions with a "label" are considered a "defining" debugger contribution
                let has_label = dbg.label.as_deref().is_some_and(|label| !label.is_empty());
                let dbg_type = dbg.r#type.as_deref().filter(|dbg_type| !dbg_type.is_empty());
                if has_label && let Some(dbg_type) = dbg_type && dbg_type == r#type {
                    return true;
                }
            }
        }
        false
    }

    fn get_debug_adapter_trackers(&self, session: &Rc<ExtHostDebugSession>) -> Promise<Option<Rc<dyn DebugAdapterTracker>>> {
        let session_type = config_type(&session.configuration());

        let promises: Vec<Promise<Option<Rc<dyn DebugAdapterTracker>>>> = self
            .tracker_factories
            .borrow()
            .iter()
            .filter(|tuple| tuple.r#type == session_type || tuple.r#type == "*")
            .map(|tuple| {
                let factory = tuple.factory.clone();
                let api = session.api();
                as_promise_maybe(move || factory.create_debug_adapter_tracker(api)).then_result(|result| Ok(result.unwrap_or(None)))
            })
            .collect();

        let all = Promise::all(promises).then(|result| {
            let trackers: Vec<Rc<dyn DebugAdapterTracker>> = result.into_iter().flatten().collect(); // filter null
            if trackers.is_empty() { None } else { Some(Rc::new(MultiTracker { trackers }) as Rc<dyn DebugAdapterTracker>) }
        });
        let (timeout_promise, resolver) = Promise::new();
        runtime::set_timeout(move || resolver.resolve(None), 1000);

        // ignore errors
        Promise::race(vec![all, timeout_promise]).then_result(|result| Ok(result.unwrap_or(None)))
    }

    fn get_adapter_descriptor(&self, adapter_descriptor_factory: Option<Rc<dyn DebugAdapterDescriptorFactory>>, session: Rc<ExtHostDebugSession>) -> Promise<Option<DebugAdapterDescriptor>> {
        let this = self.rc();
        async_fn(async move {
            // a "debugServer" attribute in the launch config takes precedence
            if let Some(server_port) = session.configuration().get("debugServer").and_then(Value::as_f64) {
                return Ok(Some(DebugAdapterDescriptor::Server(DebugAdapterServer { port: server_port, host: None })));
            }

            if let Some(factory) = adapter_descriptor_factory {
                let extension_registry = this.extension_service.get_extension_registry().await?;
                let executable = this.hooks.da_executable_from_package(&session, &extension_registry);
                let api = session.api();
                return as_promise_maybe(move || factory.create_debug_adapter_descriptor(api, executable)).await;
            }

            // fallback: use executable information from package.json
            let extension_registry = this.extension_service.get_extension_registry().await?;
            Ok(this.hooks.da_executable_from_package(&session, &extension_registry).map(DebugAdapterDescriptor::Executable))
        })
    }

    fn get_session(&self, dto: IDebugSessionDto) -> Promise<Rc<ExtHostDebugSession>> {
        let this = self.rc();
        async_fn(async move {
            match dto {
                IDebugSessionDto::Id(id) => {
                    let known = this.debug_sessions.borrow().get(&id).cloned();
                    known.ok_or_else(|| Error::new("cannot find session"))
                }
                IDebugSessionDto::Full(dto) => {
                    let known = this.debug_sessions.borrow().get(&dto.id).cloned();
                    if let Some(known) = known {
                        return Ok(known);
                    }
                    let folder = this.get_folder(dto.folder_uri.clone()).await?;
                    let parent = dto.parent.as_ref().and_then(|parent| this.debug_sessions.borrow().get(parent).cloned());
                    let session = ExtHostDebugSession::new(
                        this.debug_service_proxy.clone(),
                        &dto.id,
                        &dto.r#type,
                        &dto.name,
                        folder,
                        dto.configuration.clone(),
                        parent.map(|parent| parent.api()),
                    );
                    this.debug_sessions.borrow_mut().insert(session.id(), session.clone());
                    this.debug_service_proxy.rpc_session_cached(&session.id());
                    Ok(session)
                }
            }
        })
    }

    fn get_folder(&self, folder_uri: Option<URI>) -> Promise<Option<Rc<WorkspaceFolder>>> {
        match folder_uri {
            Some(folder_uri) => self.workspace_service.resolve_workspace_folder(&folder_uri),
            None => Promise::resolved(None),
        }
    }
}

impl IDisposable for ExtHostDebugServiceBase {
    fn dispose(&self) {
        self.base.dispose();
    }
}

/// `WorkerExtHostDebugService`: a base sem nenhuma sobrescrita.
pub struct WorkerExtHostDebugService;

impl WorkerExtHostDebugService {
    pub fn new(deps: ExtHostDebugServiceDeps) -> Rc<ExtHostDebugServiceBase> {
        ExtHostDebugServiceBase::new(deps, Rc::new(DefaultExtHostDebugServiceHooks))
    }
}

// ---- métodos de RPC (ExtHostDebugServiceShape)

impl ExtHostDebugServiceShape for ExtHostDebugServiceBase {
    fn rpc_run_in_terminal(&self, _args: Value, _session_id: &str) -> Promise<Option<i64>> {
        Promise::resolved(None)
    }

    fn rpc_substitute_variables(&self, folder_uri: Option<URI>, config: IConfig) -> Promise<IConfig> {
        let this = self.rc();
        async_fn(async move {
            let folder = this.get_folder(folder_uri).await?;
            let ws = folder.map(|folder| folder_to_platform(&folder));
            let variable_resolver = this.variable_resolver.get_resolver().await?;
            variable_resolver.resolve_async(ws, config).await
        })
    }

    fn rpc_start_da_session(&self, debug_adapter_handle: u32, session_dto: IDebugSessionDto) -> Promise<()> {
        let this = self.rc();
        async_fn(async move {
            let session = this.get_session(session_dto).await?;
            let session_type = session.session_type();

            let factory = this.get_adapter_descriptor_factory_by_type(&session_type);
            let da_descriptor = this.get_adapter_descriptor(factory, session.clone()).await?;
            let Some(da_descriptor) = da_descriptor else {
                return Err(Error::new(format!("Couldn't find a debug adapter descriptor for debug type '{session_type}' (extension might have failed to activate)")));
            };

            let Some(debug_adapter) = this.create_debug_adapter(&da_descriptor, &session) else {
                return Err(Error::new(format!("Couldn't create a debug adapter for type '{session_type}'.")));
            };

            this.debug_adapters.borrow_mut().insert(debug_adapter_handle, debug_adapter.clone());

            let tracker = this.get_debug_adapter_trackers(&session).await?;
            if let Some(tracker) = &tracker {
                this.debug_adapters_trackers.borrow_mut().insert(debug_adapter_handle, tracker.clone());
            }

            let message_this = this.clone();
            let message_tracker = tracker.clone();
            let message_adapter = Rc::downgrade(&debug_adapter);
            let message_session_type = session_type.clone();
            debug_adapter.on_message(Box::new(move |message: &ProtocolMessage| {
                if let ProtocolMessage::Request(request) = message
                    && request.command == "handshake"
                {
                    if let Some(adapter) = message_adapter.upgrade() {
                        message_this.answer_handshake(adapter, request.clone());
                    }
                } else {
                    if let Some(tracker) = &message_tracker {
                        tracker.on_did_send_message(message);
                    }

                    // DA -> VS Code
                    // Try to catch details for #233167
                    match errors::catch(|| convert_to_vsc_paths(message, true)) {
                        Ok(message) => message_this.debug_service_proxy.rpc_accept_da_message(debug_adapter_handle, message),
                        Err(error) => {
                            let (command, event) = match message {
                                ProtocolMessage::Request(request) => (Some(request.command.clone()), None),
                                ProtocolMessage::Response(response) => (Some(response.command.clone()), None),
                                ProtocolMessage::Event(event) => (None, Some(event.event.clone())),
                                ProtocolMessage::Other(_) => (None, None),
                            };
                            let message_type = format!("{}_{}", message.message_type(), command.or(event).unwrap_or_default());
                            message_this.telemetry_proxy.rpc_public_log2(
                                "debugProtocolMessageError",
                                Some(Dynamic::object([("type", Dynamic::string(&message_type)), ("from", Dynamic::string(&message_session_type))])),
                            );
                            errors::on_unexpected_error(&error);
                        }
                    }
                }
            }));

            let error_this = this.clone();
            let error_tracker = tracker.clone();
            debug_adapter.on_error().listen(move |err: &Error| {
                if let Some(tracker) = &error_tracker {
                    tracker.on_error(err);
                }
                error_this.debug_service_proxy.rpc_accept_da_error(debug_adapter_handle, &err.name, &err.message, err.stack.as_deref());
            });

            let exit_this = this.clone();
            let exit_tracker = tracker.clone();
            debug_adapter.on_exit().listen(move |code: &Option<i32>| {
                if let Some(tracker) = &exit_tracker {
                    tracker.on_exit(*code, None);
                }
                exit_this.debug_service_proxy.rpc_accept_da_exit(debug_adapter_handle, *code, None);
            });

            if let Some(tracker) = &tracker {
                tracker.on_will_start_session();
            }

            debug_adapter.start_session().await
        })
    }

    fn rpc_send_da_message(&self, debug_adapter_handle: u32, message: ProtocolMessage) {
        // VS Code -> DA
        let message = convert_to_da_paths(&message, false);

        let tracker = self.debug_adapters_trackers.borrow().get(&debug_adapter_handle).cloned(); // TODO@AW: same handle?
        if let Some(tracker) = tracker {
            tracker.on_will_receive_message(&message);
        }

        let da = self.debug_adapters.borrow().get(&debug_adapter_handle).cloned();
        if let Some(da) = da {
            da.send_message(&message);
        }
    }

    fn rpc_stop_da_session(&self, debug_adapter_handle: u32) -> Promise<()> {
        let tracker = self.debug_adapters_trackers.borrow_mut().remove(&debug_adapter_handle);
        if let Some(tracker) = tracker {
            tracker.on_will_stop_session();
        }

        let da = self.debug_adapters.borrow_mut().remove(&debug_adapter_handle);
        match da {
            Some(da) => da.stop_session(),
            None => Promise::resolved(()),
        }
    }

    fn rpc_accept_breakpoints_delta(&self, delta: IBreakpointsDeltaDto) {
        let mut a: Vec<Rc<Breakpoint>> = Vec::new();
        let mut r: Vec<Rc<Breakpoint>> = Vec::new();
        let mut c: Vec<Rc<Breakpoint>> = Vec::new();

        if let Some(added) = delta.added {
            for bpd in added {
                let base = match &bpd {
                    BreakpointDto::Source(dto) => &dto.base,
                    BreakpointDto::Function(dto) => &dto.base,
                    BreakpointDto::Data(dto) => &dto.base,
                };
                let Some(id) = base.id.clone().filter(|id| !id.is_empty()) else { continue };
                if self.breakpoints.borrow().contains_key(&id) {
                    continue;
                }
                let bp = match &bpd {
                    BreakpointDto::Function(dto) => Breakpoint::new_function(&dto.function_name, Some(dto.base.enabled), dto.base.condition.clone(), dto.base.hit_condition.clone(), dto.base.log_message.clone(), dto.base.mode.clone()),
                    BreakpointDto::Data(dto) => Breakpoint::new_data(
                        &dto.label,
                        &dto.data_id,
                        dto.can_persist,
                        Some(dto.base.enabled),
                        dto.base.hit_condition.clone(),
                        dto.base.condition.clone(),
                        dto.base.log_message.clone(),
                        dto.base.mode.clone(),
                    ),
                    BreakpointDto::Source(dto) => Breakpoint::new_source(
                        source_location(dto),
                        Some(dto.base.enabled),
                        dto.base.condition.clone(),
                        dto.base.hit_condition.clone(),
                        dto.base.log_message.clone(),
                        dto.base.mode.clone(),
                    ),
                };
                bp.set_id(&id);
                self.breakpoints.borrow_mut().insert(id, bp.clone());
                a.push(bp);
            }
        }

        if let Some(removed) = delta.removed {
            for id in removed {
                let bp = self.breakpoints.borrow_mut().shift_remove(&id);
                if let Some(bp) = bp {
                    r.push(bp);
                }
            }
        }

        if let Some(changed) = delta.changed {
            for bpd in changed {
                let base = match &bpd {
                    BreakpointDto::Source(dto) => &dto.base,
                    BreakpointDto::Function(dto) => &dto.base,
                    BreakpointDto::Data(dto) => &dto.base,
                };
                let Some(id) = base.id.clone().filter(|id| !id.is_empty()) else { continue };
                let bp = self.breakpoints.borrow().get(&id).cloned();
                let Some(bp) = bp else { continue };
                match (&bp.kind(), &bpd) {
                    (BreakpointKind::Function { .. }, BreakpointDto::Function(dto)) => {
                        bp.update_common(dto.base.enabled, dto.base.condition.clone(), dto.base.hit_condition.clone(), dto.base.log_message.clone());
                        bp.set_kind(BreakpointKind::Function { function_name: dto.function_name.clone() });
                    }
                    (BreakpointKind::Source { .. }, BreakpointDto::Source(dto)) => {
                        bp.update_common(dto.base.enabled, dto.base.condition.clone(), dto.base.hit_condition.clone(), dto.base.log_message.clone());
                        bp.set_kind(BreakpointKind::Source { location: source_location(dto) });
                    }
                    _ => {}
                }
                c.push(bp);
            }
        }

        self.fire_breakpoint_changes(a, r, c);
    }

    fn rpc_accept_stack_frame_focus(&self, focus_dto: Option<StackFocusDto>) -> Promise<()> {
        let this = self.rc();
        async_fn(async move {
            let mut focus: Option<DebugStackItem> = None;
            if let Some(focus_dto) = focus_dto {
                match focus_dto {
                    StackFocusDto::Thread(dto) => {
                        let session = this.get_session(IDebugSessionDto::Id(dto.session_id)).await?;
                        focus = Some(DebugStackItem::Thread(DebugThread { session: session.api(), thread_id: dto.thread_id }));
                    }
                    StackFocusDto::StackFrame(dto) => {
                        let session = this.get_session(IDebugSessionDto::Id(dto.session_id)).await?;
                        focus = Some(DebugStackItem::StackFrame(DebugStackFrame { session: session.api(), thread_id: dto.thread_id, frame_id: dto.frame_id }));
                    }
                }
            }

            *this.active_stack_item.borrow_mut() = focus.clone();
            this.on_did_change_active_stack_item.fire(focus);
            Ok(())
        })
    }

    fn rpc_provide_debug_configurations(&self, config_provider_handle: u32, folder_uri: Option<URI>, token: CancellationToken) -> Promise<Vec<IConfig>> {
        let this = self.rc();
        async_fn(async move {
            let provider = this.get_config_provider_by_handle(config_provider_handle).ok_or_else(|| Error::new("no DebugConfigurationProvider found"))?;
            if !provider.has_provide_debug_configurations() {
                return Err(Error::new("DebugConfigurationProvider has no method provideDebugConfigurations"));
            }
            let folder = this.get_folder(folder_uri).await?;
            let debug_configurations = await_maybe(provider.provide_debug_configurations(folder, token)).await?;
            debug_configurations.ok_or_else(|| Error::new("nothing returned from DebugConfigurationProvider.provideDebugConfigurations"))
        })
    }

    fn rpc_resolve_debug_configuration(&self, config_provider_handle: u32, folder_uri: Option<URI>, debug_configuration: IConfig, token: CancellationToken) -> Promise<Option<IConfig>> {
        let this = self.rc();
        async_fn(async move {
            let provider = this.get_config_provider_by_handle(config_provider_handle).ok_or_else(|| Error::new("no DebugConfigurationProvider found"))?;
            if !provider.has_resolve_debug_configuration() {
                return Err(Error::new("DebugConfigurationProvider has no method resolveDebugConfiguration"));
            }
            let folder = this.get_folder(folder_uri).await?;
            await_maybe(provider.resolve_debug_configuration(folder, debug_configuration, token)).await
        })
    }

    fn rpc_resolve_debug_configuration_with_substituted_variables(
        &self,
        config_provider_handle: u32,
        folder_uri: Option<URI>,
        debug_configuration: IConfig,
        token: CancellationToken,
    ) -> Promise<Option<IConfig>> {
        let this = self.rc();
        async_fn(async move {
            let provider = this.get_config_provider_by_handle(config_provider_handle).ok_or_else(|| Error::new("no DebugConfigurationProvider found"))?;
            if !provider.has_resolve_debug_configuration_with_substituted_variables() {
                return Err(Error::new("DebugConfigurationProvider has no method resolveDebugConfigurationWithSubstitutedVariables"));
            }
            let folder = this.get_folder(folder_uri).await?;
            await_maybe(provider.resolve_debug_configuration_with_substituted_variables(folder, debug_configuration, token)).await
        })
    }

    fn rpc_provide_debug_adapter(&self, adapter_factory_handle: u32, session_dto: IDebugSessionDto) -> Promise<IAdapterDescriptor> {
        let this = self.rc();
        async_fn(async move {
            let adapter_descriptor_factory = this
                .get_adapter_descriptor_factory_by_handle(adapter_factory_handle)
                .ok_or_else(|| Error::new("no adapter descriptor factory found for handle"))?;
            let session = this.get_session(session_dto).await?;
            let adapter_descriptor = this.get_adapter_descriptor(Some(adapter_descriptor_factory), session.clone()).await?;
            let Some(adapter_descriptor) = adapter_descriptor else {
                return Err(Error::new(format!("Couldn't find a debug adapter descriptor for debug type '{}'", session.session_type())));
            };
            Ok(this.convert_to_dto(&adapter_descriptor))
        })
    }

    fn rpc_accept_debug_session_started(&self, session_dto: IDebugSessionDto) -> Promise<()> {
        let this = self.rc();
        async_fn(async move {
            let session = this.get_session(session_dto).await?;
            this.on_did_start_debug_session.fire(session.api());
            Ok(())
        })
    }

    fn rpc_accept_debug_session_terminated(&self, session_dto: IDebugSessionDto) -> Promise<()> {
        let this = self.rc();
        async_fn(async move {
            let session = this.get_session(session_dto).await?;
            this.on_did_terminate_debug_session.fire(session.api());
            this.debug_sessions.borrow_mut().remove(&session.id());
            for id in this.debug_visualization_elements.keys() {
                let Some(entry) = this.debug_visualization_elements.get(&id) else { continue };
                entry.session_ids.borrow_mut().remove(&session.id());
                if entry.session_ids.borrow().is_empty() {
                    this.debug_visualization_elements.delete_and_dispose(&id);
                }
            }
            Ok(())
        })
    }

    fn rpc_accept_debug_session_active_changed(&self, session_dto: Option<IDebugSessionDto>) -> Promise<()> {
        let this = self.rc();
        async_fn(async move {
            let active = match session_dto {
                Some(session_dto) => Some(this.get_session(session_dto).await?),
                None => None,
            };
            *this.active_debug_session.borrow_mut() = active.clone();
            this.on_did_change_active_debug_session.fire(active.map(|session| session.api()));
            Ok(())
        })
    }

    fn rpc_accept_debug_session_name_changed(&self, session_dto: IDebugSessionDto, name: &str) -> Promise<()> {
        let this = self.rc();
        let name = name.to_owned();
        async_fn(async move {
            let session = this.get_session(session_dto).await?;
            session.accept_name_changed(&name);
            Ok(())
        })
    }

    fn rpc_accept_debug_session_custom_event(&self, session_dto: IDebugSessionDto, event: Value) -> Promise<()> {
        let this = self.rc();
        async_fn(async move {
            let session = this.get_session(session_dto).await?;
            let ee = DebugSessionCustomEvent {
                session: session.api(),
                event: event.get("event").and_then(Value::as_str).unwrap_or_default().to_owned(),
                body: event.get("body").cloned(),
            };
            this.on_did_receive_debug_session_custom_event.fire(ee);
            Ok(())
        })
    }

    fn rpc_get_visualizer_tree_item(&self, tree_id: &str, element: IDebugVisualizationContext) -> Promise<Option<IDebugVisualizationTreeItem>> {
        let this = self.rc();
        let tree_id = tree_id.to_owned();
        async_fn(async move {
            let Some(context) = this.hydrate_visualization_context(&element) else {
                return Ok(None);
            };

            let tree = this.debug_visualization_trees.borrow().get(&tree_id).cloned();
            let item = await_maybe(match tree {
                Some(tree) => tree.get_tree_item(context.clone()),
                None => MaybePromise::Value(None),
            })
            .await?;
            let session_id = context.session.id();
            Ok(match item {
                Some(item) if this.debug_sessions.borrow().contains_key(&session_id) => Some(this.convert_visualizer_tree_item(&tree_id, item, vec![session_id])),
                _ => None,
            })
        })
    }

    fn rpc_get_visualizer_tree_item_children(&self, tree_id: &str, element: u64) -> Promise<Vec<IDebugVisualizationTreeItem>> {
        let this = self.rc();
        let tree_id = tree_id.to_owned();
        async_fn(async move {
            let Some(entry) = this.debug_visualization_elements.get(&element) else {
                return Ok(Vec::new());
            };

            let tree = this.debug_visualization_trees.borrow().get(&tree_id).cloned();
            let children = await_maybe(match tree {
                Some(tree) => tree.get_children(entry.item.clone()),
                None => MaybePromise::Value(None),
            })
            .await?;
            if !this.debug_visualization_elements.has(&element) {
                return Ok(Vec::new());
            }
            let session_ids: Vec<String> = entry.session_ids.borrow().iter().cloned().collect();
            Ok(children
                .map(|children| children.into_iter().map(|child| this.convert_visualizer_tree_item(&tree_id, child, session_ids.clone())).collect())
                .unwrap_or_default())
        })
    }

    fn rpc_edit_visualizer_tree_item(&self, element: u64, value: &str) -> Promise<Option<IDebugVisualizationTreeItem>> {
        let this = self.rc();
        let value = value.to_owned();
        async_fn(async move {
            let Some(e) = this.debug_visualization_elements.get(&element) else {
                return Ok(None);
            };

            let tree = this.debug_visualization_trees.borrow().get(&e.provider).cloned();
            let r = await_maybe(match tree {
                Some(tree) => tree.edit_item(e.item.clone(), &value),
                None => MaybePromise::Value(None),
            })
            .await?;
            if !this.debug_visualization_elements.has(&element) {
                return Ok(None);
            }
            let session_ids: Vec<String> = e.session_ids.borrow().iter().cloned().collect();
            Ok(Some(this.convert_visualizer_tree_item(&e.provider, r.unwrap_or_else(|| e.item.clone()), session_ids)))
        })
    }

    fn rpc_dispose_visualized_tree(&self, element: u64) {
        let Some(root) = self.debug_visualization_elements.get(&element) else {
            return;
        };

        let mut queue: Vec<Option<Vec<u64>>> = vec![root.children.borrow().clone()];
        let mut index = 0;
        while index < queue.len() {
            if let Some(children) = queue[index].clone() {
                for child in children {
                    queue.push(self.debug_visualization_elements.get(&child).and_then(|entry| entry.children.borrow().clone()));
                    self.debug_visualization_elements.delete_and_dispose(&child);
                }
            }
            index += 1;
        }
    }

    fn rpc_resolve_debug_visualizer(&self, id: u64, token: CancellationToken) -> Promise<MainThreadDebugVisualization> {
        let this = self.rc();
        async_fn(async move {
            let (v, provider, extension_id) = {
                let visualizers = this.visualizers.borrow();
                let visualizer = visualizers.get(&id).ok_or_else(|| Error::new(format!("No debug visualizer found with id '{id}'")))?;
                (visualizer.v.clone(), visualizer.provider.clone(), visualizer.extension_id.clone())
            };

            let mut v = v;
            if v.visualization.is_none() {
                let resolved = await_maybe(provider.resolve_debug_visualization(v.clone(), token)).await?;
                v = resolved.unwrap_or(v);
                if let Some(visualizer) = this.visualizers.borrow_mut().get_mut(&id) {
                    visualizer.v = v.clone();
                }
            }

            if v.visualization.is_none() {
                return Err(Error::new("No visualization returned from resolveDebugVisualization in '[object Object]'"));
            }

            Ok(Self::serialize_visualization(&extension_id, &v.visualization).expect("a visualização existe"))
        })
    }

    fn rpc_execute_debug_visualizer_command(&self, id: u64) -> Promise<()> {
        let this = self.rc();
        async_fn(async move {
            let command = {
                let visualizers = this.visualizers.borrow();
                let visualizer = visualizers.get(&id).ok_or_else(|| Error::new(format!("No debug visualizer found with id '{id}'")))?;
                visualizer.v.visualization.clone()
            };

            if let Some(DebugVisualizationTarget::Command(command)) = command {
                // O `executeCommand` do original não é esperado.
                let _ = this.commands.execute_command(&command.command, command.arguments.clone().unwrap_or_default());
            }
            Ok(())
        })
    }

    fn rpc_provide_debug_visualizers(&self, extension_id: &str, id: &str, context: IDebugVisualizationContext, token: CancellationToken) -> Promise<Vec<IDebugVisualization>> {
        let this = self.rc();
        let extension_id = extension_id.to_owned();
        let id = id.to_owned();
        async_fn(async move {
            let context_hydrated = this.hydrate_visualization_context(&context);
            let key = Self::extension_vis_key(&extension_id, &id);
            let provider = this.debug_visualization_providers.borrow().get(&key).cloned();
            let (Some(context_hydrated), Some(provider)) = (context_hydrated, provider) else {
                return Ok(Vec::new()); // probably ended in the meantime
            };

            let visualizations = await_maybe(provider.provide_debug_visualization(context_hydrated, token)).await?;

            let Some(visualizations) = visualizations else {
                return Ok(Vec::new());
            };

            Ok(visualizations
                .into_iter()
                .map(|v| {
                    let id = this.visualizer_id_counter.get() + 1;
                    this.visualizer_id_counter.set(id);
                    this.visualizers.borrow_mut().insert(id, VisualizerEntry { v: v.clone(), provider: provider.clone(), extension_id: extension_id.clone() });
                    let (icon_path, icon_class) = match &v.icon_path {
                        Some(icon) => Self::get_icon_path_or_class(icon),
                        None => (None, None),
                    };
                    IDebugVisualization {
                        id,
                        name: v.name.clone(),
                        icon_class,
                        icon_path,
                        visualization: Self::serialize_visualization(&extension_id, &v.visualization),
                    }
                })
                .collect())
        })
    }

    fn rpc_dispose_debug_visualizers(&self, ids: Vec<u64>) {
        for id in ids {
            self.visualizers.borrow_mut().remove(&id);
        }
    }
}

impl ExtHostDebugServiceBase {
    /// A resposta do `handshake` do adaptador: assina o valor com o `ISignService`, ou responde com falha.
    fn answer_handshake(&self, adapter: Rc<dyn AbstractDebugAdapter>, request: Request) {
        let this = self.rc();
        let _ = async_fn(async move {
            let mut response = Response { seq: 0, request_seq: request.seq, success: true, command: request.command.clone(), message: None, body: None };

            if this.sign_service.borrow().is_none() {
                let created = this.hooks.create_sign_service();
                *this.sign_service.borrow_mut() = created;
            }

            let sign_service = this.sign_service.borrow().clone();
            let signature = match sign_service {
                Some(sign_service) => {
                    let value = request.arguments.as_ref().and_then(|arguments| arguments.get("value")).and_then(Value::as_str).unwrap_or_default().to_owned();
                    sign_service.sign(&value).await
                }
                None => Err(Error::new("no signer")),
            };
            match signature {
                Ok(signature) => response.body = Some(json!({ "signature": signature })),
                Err(error) => {
                    response.success = false;
                    response.message = Some(error.message);
                }
            }
            adapter.send_response(&mut response);
            Ok(())
        });
    }
}

/// O `new Location(uri, new Position(bpd.line, bpd.character))` de um DTO de breakpoint de código-fonte.
fn source_location(dto: &ISourceBreakpointDto) -> Location {
    let position = types::Position::new(dto.line as f64, dto.character as f64).unwrap_or_else(|error| errors::throw(error));
    Location::new(Some(dto.uri.clone()), Some(types::RangeOrPosition::Position(position)))
}
