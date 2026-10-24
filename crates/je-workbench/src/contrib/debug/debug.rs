//! Porte parcial de `src/vs/workbench/contrib/debug/common/debug.ts`: só o que `debugSource`, `abstractDebugAdapter`, o host de
//! extensões de depuração (`extHostDebugService.ts`) e `mainThreadDebugService.ts` usam: o esquema `DEBUG_SCHEME`, o contrato
//! `IDebugAdapter`, os descritores de adaptador, os tipos de visualização e o contrato mínimo de `IDebugService`.
//!
//! Notas de tradução:
//! - `IDebugAdapter` é o [`AbstractDebugAdapter`](super::abstract_debug_adapter::AbstractDebugAdapter): todo
//!   implementador do original (executável, socket, pipe nomeado, extension host, dublê de teste) estende a classe
//!   abstrata, que traz os métodos do contrato (`onError`, `onExit`, `onRequest`, `onEvent`, `sendResponse`,
//!   `sendRequest`) prontos; o trait é o contrato e a implementação padrão ao mesmo tempo.
//! - **Contrato do serviço de depuração.** [`IDebugService`], [`IConfigurationManager`], [`IAdapterManager`], [`IDebugModel`],
//!   [`IViewModel`], [`IDebugSession`], [`IThread`] e [`IStackFrame`] trazem só os membros que o
//!   `MainThreadDebugService` chama (o resto das interfaces do original, como `state`, `onDidChangeState`, `restartSession`,
//!   os breakpoints de exceção, o `IExpression`, o REPL e as threads, fica de fora até o porte do modelo de depuração).
//!   Os breakpoints (`IBreakpoint`, `IFunctionBreakpoint`, `IDataBreakpoint`, `IInstructionBreakpoint`) são structs de dados
//!   com o `id` (o `getId()`), reunidos em [`AnyBreakpoint`] (o `'name' in bp` do original é um `match`). `ILaunch` e
//!   `DebugCompoundRoot`, que o serviço só repassa, são opacos. O `IDebugConfigurationProvider` e o
//!   `IDebugAdapterDescriptorFactory` são structs com os fechos do original (os membros opcionais são `Option`).

pub use super::abstract_debug_adapter::AbstractDebugAdapter as IDebugAdapter;

/// `DEBUG_SCHEME`.
pub const DEBUG_SCHEME: &str = "debug";

// ---- configuração e descritores de adaptador

/// `IConfig`: a configuração de uma sessão (`type`, `request`, `name`, `debugServer` e as chaves livres do adaptador).
/// É o objeto JSON cru, como o `[key: string]: any` do original.
pub type IConfig = serde_json::Value;

/// `IDebugAdapterExecutableOptions`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct IDebugAdapterExecutableOptions {
    pub cwd: Option<String>,
    pub env: Option<indexmap::IndexMap<String, String>>,
}

/// `IDebugAdapterExecutable` (`type: 'executable'`).
#[derive(Clone, Debug, PartialEq)]
pub struct IDebugAdapterExecutable {
    pub command: String,
    pub args: Vec<String>,
    pub options: Option<IDebugAdapterExecutableOptions>,
}

/// `IDebugAdapterServer` (`type: 'server'`).
#[derive(Clone, Debug, PartialEq)]
pub struct IDebugAdapterServer {
    pub port: f64,
    pub host: Option<String>,
}

/// `IDebugAdapterNamedPipeServer` (`type: 'pipeServer'`).
#[derive(Clone, Debug, PartialEq)]
pub struct IDebugAdapterNamedPipeServer {
    pub path: String,
}

/// `IDebugAdapterImpl` (`type: 'implementation'`).
#[derive(Clone, Debug, PartialEq)]
pub struct IDebugAdapterImpl;

/// `IAdapterDescriptor` (e o `Dto<IAdapterDescriptor>`, que não tem nenhum campo a converter): o `type` é a variante.
#[derive(Clone, Debug, PartialEq)]
pub enum IAdapterDescriptor {
    Executable(IDebugAdapterExecutable),
    Server(IDebugAdapterServer),
    PipeServer(IDebugAdapterNamedPipeServer),
    Implementation(IDebugAdapterImpl),
}

// ---- opções de sessão

/// `IDebugSessionReplMode`: `'separate' | 'mergeWithParent'`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IDebugSessionReplMode {
    Separate,
    MergeWithParent,
}

/// `IDebugTestRunReference`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IDebugTestRunReference {
    pub run_id: String,
    pub task_id: String,
}

/// `DebugConfigurationProviderTriggerKind`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DebugConfigurationProviderTriggerKind {
    /// `DebugConfigurationProvider.provideDebugConfigurations` is called to provide the initial debug configurations for a
    /// newly created launch.json.
    Initial = 1,
    /// `DebugConfigurationProvider.provideDebugConfigurations` is called to provide dynamically generated debug
    /// configurations when the user asks for them through the UI.
    Dynamic = 2,
}

// ---- visualização

/// `IDebugVisualizationContext`.
#[derive(Clone, Debug, PartialEq)]
pub struct IDebugVisualizationContext {
    pub variable: super::debug_protocol::Variable,
    pub container_id: Option<i64>,
    pub frame_id: Option<i64>,
    pub thread_id: i64,
    pub session_id: String,
}

/// `MainThreadDebugVisualization`: `{ type: Command } | { type: Tree, id }`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MainThreadDebugVisualization {
    Command,
    Tree { id: String },
}

/// `DebugTreeItemCollapsibleState`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DebugTreeItemCollapsibleState {
    None = 0,
    Collapsed = 1,
    Expanded = 2,
}

/// `IDebugVisualizationTreeItem` (e o `IDebugVisualizationTreeItem.Serialized`, que é o mesmo tipo).
#[derive(Clone, Debug, PartialEq)]
pub struct IDebugVisualizationTreeItem {
    pub id: u64,
    pub label: String,
    pub description: Option<String>,
    pub collapsible_state: DebugTreeItemCollapsibleState,
    pub context_value: Option<String>,
    pub can_edit: Option<bool>,
}

/// O `{ light?: URI; dark: URI }` de `IDebugVisualization.iconPath` (e o `{ light?: UriComponents; dark: UriComponents }`
/// de `Serialized`: os `UriComponents` do RPC já chegam como [`URI`](je_base::uri::URI)).
#[derive(Clone, Debug, PartialEq)]
pub struct DebugVisualizationIconPath {
    pub light: Option<je_base::uri::URI>,
    pub dark: je_base::uri::URI,
}

/// `IDebugVisualization.Serialized`; o `IDebugVisualization` é o mesmo tipo (o `deserialize` só revive os URIs, que aqui
/// já são [`URI`](je_base::uri::URI)).
#[derive(Clone, Debug, PartialEq)]
pub struct IDebugVisualization {
    pub id: u64,
    pub name: String,
    pub icon_path: Option<DebugVisualizationIconPath>,
    pub icon_class: Option<String>,
    pub visualization: Option<MainThreadDebugVisualization>,
}

// ---- breakpoints

/// `IBaseBreakpoint` (a parte que o host de extensões lê) com o `id` do `getId()`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct IBaseBreakpoint {
    pub id: String,
    pub enabled: bool,
    pub condition: Option<String>,
    pub hit_condition: Option<String>,
    pub log_message: Option<String>,
    pub mode: Option<String>,
}

impl IBaseBreakpoint {
    /// `getId()`.
    pub fn get_id(&self) -> String {
        self.id.clone()
    }
}

/// `IBreakpoint`.
#[derive(Clone, Debug, PartialEq)]
pub struct IBreakpoint {
    pub base: IBaseBreakpoint,
    pub uri: je_base::uri::URI,
    pub line_number: i64,
    pub column: Option<i64>,
}

/// `IFunctionBreakpoint`.
#[derive(Clone, Debug, PartialEq)]
pub struct IFunctionBreakpoint {
    pub base: IBaseBreakpoint,
    pub name: String,
}

/// `DataBreakpointSource`: `{ type: Variable, dataId } | { type: Address, address, bytes? }`.
#[derive(Clone, Debug, PartialEq)]
pub enum DataBreakpointSource {
    Variable { data_id: String },
    Address { address: String, bytes: Option<i64> },
}

/// `IDataBreakpoint`.
#[derive(Clone, Debug, PartialEq)]
pub struct IDataBreakpoint {
    pub base: IBaseBreakpoint,
    pub description: String,
    pub can_persist: bool,
    pub src: DataBreakpointSource,
    pub access_type: super::debug_protocol::DataBreakpointAccessType,
}

/// `IInstructionBreakpoint`.
#[derive(Clone, Debug, PartialEq)]
pub struct IInstructionBreakpoint {
    pub base: IBaseBreakpoint,
    pub instruction_reference: String,
}

/// `IBreakpoint | IFunctionBreakpoint | IDataBreakpoint | IInstructionBreakpoint`.
#[derive(Clone, Debug, PartialEq)]
pub enum AnyBreakpoint {
    Breakpoint(IBreakpoint),
    Function(IFunctionBreakpoint),
    Data(IDataBreakpoint),
    Instruction(IInstructionBreakpoint),
}

/// `IBreakpointsChangeEvent`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct IBreakpointsChangeEvent {
    pub added: Option<Vec<AnyBreakpoint>>,
    pub removed: Option<Vec<AnyBreakpoint>>,
    pub changed: Option<Vec<AnyBreakpoint>>,
    pub session_only: bool,
}

/// `IBreakpointData`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct IBreakpointData {
    pub id: Option<String>,
    pub line_number: i64,
    pub column: Option<i64>,
    pub enabled: Option<bool>,
    pub condition: Option<String>,
    pub log_message: Option<String>,
    pub hit_condition: Option<String>,
    pub mode: Option<String>,
}

/// `IFunctionBreakpointOptions`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct IFunctionBreakpointOptions {
    pub name: String,
    pub mode: Option<String>,
    pub condition: Option<String>,
    pub hit_condition: Option<String>,
    pub enabled: Option<bool>,
    pub log_message: Option<String>,
}

/// `IDataBreakpointOptions`.
#[derive(Clone, Debug, PartialEq)]
pub struct IDataBreakpointOptions {
    pub description: String,
    pub src: DataBreakpointSource,
    pub can_persist: bool,
    pub access_types: Option<Vec<super::debug_protocol::DataBreakpointAccessType>>,
    pub access_type: super::debug_protocol::DataBreakpointAccessType,
    pub mode: Option<String>,
}

/// `string | string[]`: o `id` de `removeBreakpoints`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BreakpointIds {
    One(String),
    Many(Vec<String>),
}

// ---- sessão, modelo e modelo de visão

/// `DebugCompoundRoot` (`debugCompoundRoot.ts`): o serviço o repassa de uma sessão para a filha e a sessão ouve o fim.
pub struct DebugCompoundRoot {
    stopped: std::cell::Cell<bool>,
    stop_emitter: std::rc::Rc<je_base::event::Emitter<()>>,
}

impl Default for DebugCompoundRoot {
    fn default() -> Self {
        Self { stopped: std::cell::Cell::new(false), stop_emitter: je_base::event::Emitter::new() }
    }
}

impl DebugCompoundRoot {
    pub fn new() -> Self {
        Self::default()
    }

    /// `onDidSessionStop`.
    pub fn on_did_session_stop(&self) -> je_base::event::Event<()> {
        self.stop_emitter.event()
    }

    pub fn session_stopped(&self) {
        if !self.stopped.get() {
            // avoid sending extranous terminate events
            self.stopped.set(true);
            self.stop_emitter.fire(());
        }
    }
}

/// `ILaunch`: o serviço só o repassa do `IConfigurationManager` ao `startDebugging`.
pub trait ILaunch {}

/// `INewReplElementData` (`output` e `sev`: o `expression` e o `source` ficam de fora).
#[derive(Clone, Debug)]
pub struct INewReplElementData {
    pub output: String,
    pub sev: je_base::severity::Severity,
}

/// `IDebugSessionOptions`.
#[derive(Clone, Default)]
pub struct IDebugSessionOptions {
    pub no_debug: Option<bool>,
    pub parent_session: Option<std::rc::Rc<dyn IDebugSession>>,
    pub lifecycle_managed_by_parent: Option<bool>,
    pub repl: Option<IDebugSessionReplMode>,
    pub compound_root: Option<std::rc::Rc<DebugCompoundRoot>>,
    pub compact: Option<bool>,
    pub started_by_user: Option<bool>,
    pub save_before_restart: Option<bool>,
    pub suppress_debug_toolbar: Option<bool>,
    pub suppress_debug_statusbar: Option<bool>,
    pub suppress_debug_view: Option<bool>,
    pub test_run: Option<IDebugTestRunReference>,
}

/// `IDebugSession`, só os membros que o `MainThreadDebugService` usa.
pub trait IDebugSession {
    fn get_id(&self) -> String;
    fn configuration(&self) -> IConfig;
    fn name(&self) -> String;
    fn set_name(&self, name: &str);
    /// `root`.
    fn root(&self) -> Option<je_platform::workspace::WorkspaceFolder>;
    /// `parentSession`.
    fn parent_session(&self) -> Option<std::rc::Rc<dyn IDebugSession>>;
    /// `compoundRoot`.
    fn compound_root(&self) -> Option<std::rc::Rc<DebugCompoundRoot>>;
    /// `onDidChangeName`.
    fn on_did_change_name(&self) -> je_base::event::Event<String>;
    /// `onDidCustomEvent`.
    fn on_did_custom_event(&self) -> je_base::event::Event<super::debug_protocol::Event>;
    /// `appendToRepl(data)`.
    fn append_to_repl(&self, data: INewReplElementData);
    /// `customRequest(request, args)`.
    fn custom_request(&self, request: &str, args: Option<serde_json::Value>) -> je_base::promise::Promise<Option<super::debug_protocol::Response>>;
    /// `getDebugProtocolBreakpoint(breakpointId)`.
    fn get_debug_protocol_breakpoint(&self, breakpoint_id: &str) -> Option<serde_json::Value>;
}

/// `IThread`, só o `threadId` e a `session`.
pub trait IThread {
    fn thread_id(&self) -> i64;
    fn session(&self) -> std::rc::Rc<dyn IDebugSession>;
}

/// `IStackFrame`, só o `thread` e o `frameId`.
pub trait IStackFrame {
    fn thread(&self) -> std::rc::Rc<dyn IThread>;
    fn frame_id(&self) -> i64;
}

/// `{ stackFrame, explicit, session }` de `IViewModel.onDidFocusStackFrame`.
#[derive(Clone)]
pub struct StackFrameFocusEvent {
    pub stack_frame: Option<std::rc::Rc<dyn IStackFrame>>,
    pub explicit: bool,
    pub session: Option<std::rc::Rc<dyn IDebugSession>>,
}

/// `{ thread, explicit, session }` de `IViewModel.onDidFocusThread`.
#[derive(Clone)]
pub struct ThreadFocusEvent {
    pub thread: Option<std::rc::Rc<dyn IThread>>,
    pub explicit: bool,
    pub session: Option<std::rc::Rc<dyn IDebugSession>>,
}

/// `IViewModel`, só o foco.
pub trait IViewModel {
    fn focused_session(&self) -> Option<std::rc::Rc<dyn IDebugSession>>;
    fn focused_thread(&self) -> Option<std::rc::Rc<dyn IThread>>;
    fn focused_stack_frame(&self) -> Option<std::rc::Rc<dyn IStackFrame>>;
    fn on_did_focus_session(&self) -> je_base::event::Event<Option<std::rc::Rc<dyn IDebugSession>>>;
    fn on_did_focus_thread(&self) -> je_base::event::Event<ThreadFocusEvent>;
    fn on_did_focus_stack_frame(&self) -> je_base::event::Event<StackFrameFocusEvent>;
}

/// `IDebugModel`, só os breakpoints e a busca de sessão.
pub trait IDebugModel {
    fn get_session(&self, session_id: &str, include_inactive: bool) -> Option<std::rc::Rc<dyn IDebugSession>>;
    fn get_breakpoints(&self) -> Vec<IBreakpoint>;
    fn get_function_breakpoints(&self) -> Vec<IFunctionBreakpoint>;
    fn get_data_breakpoints(&self) -> Vec<IDataBreakpoint>;
    fn on_did_change_breakpoints(&self) -> je_base::event::Event<Option<IBreakpointsChangeEvent>>;
}

// ---- provedores e gerenciadores

/// O `resolveDebugConfiguration` e o `resolveDebugConfigurationWithSubstitutedVariables` de `IDebugConfigurationProvider`.
pub type ResolveDebugConfiguration = std::rc::Rc<
    dyn Fn(Option<je_base::uri::URI>, IConfig, je_base::cancellation::CancellationToken) -> je_base::promise::Promise<Option<IConfig>>,
>;

/// `provideDebugConfigurations` de `IDebugConfigurationProvider`.
pub type ProvideDebugConfigurations =
    std::rc::Rc<dyn Fn(Option<je_base::uri::URI>, je_base::cancellation::CancellationToken) -> je_base::promise::Promise<Vec<IConfig>>>;

/// `IDebugConfigurationProvider`.
#[derive(Clone)]
pub struct IDebugConfigurationProvider {
    pub r#type: String,
    pub trigger_kind: DebugConfigurationProviderTriggerKind,
    pub resolve_debug_configuration: Option<ResolveDebugConfiguration>,
    pub resolve_debug_configuration_with_substituted_variables: Option<ResolveDebugConfiguration>,
    pub provide_debug_configurations: Option<ProvideDebugConfigurations>,
}

/// `IDebugAdapterDescriptorFactory`.
#[derive(Clone)]
pub struct IDebugAdapterDescriptorFactory {
    pub r#type: String,
    pub create_debug_adapter_descriptor: std::rc::Rc<dyn Fn(std::rc::Rc<dyn IDebugSession>) -> je_base::promise::Promise<IAdapterDescriptor>>,
}

/// `IDebugAdapterFactory`.
pub trait IDebugAdapterFactory {
    fn create_debug_adapter(&self, session: std::rc::Rc<dyn IDebugSession>) -> std::rc::Rc<dyn IDebugAdapter>;
    fn substitute_variables(&self, folder: Option<je_platform::workspace::WorkspaceFolder>, config: IConfig) -> je_base::promise::Promise<IConfig>;
    fn run_in_terminal(&self, args: serde_json::Value, session_id: &str) -> je_base::promise::Promise<Option<i64>>;
}

/// `IConfigurationManager`, só o que o `MainThreadDebugService` chama.
pub trait IConfigurationManager {
    /// `registerDebugConfigurationProvider(debugConfigurationProvider)`.
    fn register_debug_configuration_provider(&self, provider: std::rc::Rc<IDebugConfigurationProvider>) -> je_base::lifecycle::DisposableRef;
    /// `unregisterDebugConfigurationProvider(debugConfigurationProvider)`.
    fn unregister_debug_configuration_provider(&self, provider: &std::rc::Rc<IDebugConfigurationProvider>);
    /// `getLaunch(workspaceUri)`.
    fn get_launch(&self, workspace_uri: Option<&je_base::uri::URI>) -> Option<std::rc::Rc<dyn ILaunch>>;
}

/// `IAdapterManager`, só o que o `MainThreadDebugService` chama.
pub trait IAdapterManager {
    /// `registerDebugAdapterFactory(debugTypes, debugAdapterLauncher)`.
    fn register_debug_adapter_factory(&self, debug_types: Vec<String>, debug_adapter_launcher: std::rc::Rc<dyn IDebugAdapterFactory>) -> je_base::lifecycle::DisposableRef;
    /// `registerDebugAdapterDescriptorFactory(debugAdapterDescriptorFactory)`.
    fn register_debug_adapter_descriptor_factory(&self, factory: std::rc::Rc<IDebugAdapterDescriptorFactory>) -> je_base::lifecycle::DisposableRef;
    /// `unregisterDebugAdapterDescriptorFactory(debugAdapterDescriptorFactory)`.
    fn unregister_debug_adapter_descriptor_factory(&self, factory: &std::rc::Rc<IDebugAdapterDescriptorFactory>);
}

/// O `startDebugging(launch, configOrName, options, saveBeforeStart)` aceita `IConfig | string | undefined`.
#[derive(Clone, Debug, PartialEq)]
pub enum ConfigOrName {
    Name(String),
    Config(IConfig),
}

/// `{ session, restart }` de `IDebugService.onDidEndSession`.
#[derive(Clone)]
pub struct DebugSessionEndEvent {
    pub session: std::rc::Rc<dyn IDebugSession>,
    pub restart: bool,
}

/// `IDebugService`, só o que o `MainThreadDebugService` chama.
pub trait IDebugService {
    fn on_will_new_session(&self) -> je_base::event::Event<std::rc::Rc<dyn IDebugSession>>;
    fn on_did_new_session(&self) -> je_base::event::Event<std::rc::Rc<dyn IDebugSession>>;
    fn on_did_end_session(&self) -> je_base::event::Event<DebugSessionEndEvent>;
    fn get_configuration_manager(&self) -> std::rc::Rc<dyn IConfigurationManager>;
    fn get_adapter_manager(&self) -> std::rc::Rc<dyn IAdapterManager>;
    fn get_model(&self) -> std::rc::Rc<dyn IDebugModel>;
    fn get_view_model(&self) -> std::rc::Rc<dyn IViewModel>;
    /// `addBreakpoints(uri, rawBreakpoints)`.
    fn add_breakpoints(&self, uri: je_base::uri::URI, raw_breakpoints: Vec<IBreakpointData>) -> je_base::promise::Promise<Vec<IBreakpoint>>;
    /// `removeBreakpoints(id)`.
    fn remove_breakpoints(&self, id: Option<BreakpointIds>) -> je_base::promise::Promise<()>;
    /// `addFunctionBreakpoint(opts, id)`.
    fn add_function_breakpoint(&self, opts: Option<IFunctionBreakpointOptions>, id: Option<String>);
    /// `removeFunctionBreakpoints(id)`.
    fn remove_function_breakpoints(&self, id: Option<&str>) -> je_base::promise::Promise<()>;
    /// `addDataBreakpoint(opts)`.
    fn add_data_breakpoint(&self, opts: IDataBreakpointOptions) -> je_base::promise::Promise<()>;
    /// `removeDataBreakpoints(id)`.
    fn remove_data_breakpoints(&self, id: Option<&str>) -> je_base::promise::Promise<()>;
    /// `startDebugging(launch, configOrName, options, saveBeforeStart)`.
    fn start_debugging(
        &self,
        launch: Option<std::rc::Rc<dyn ILaunch>>,
        config_or_name: Option<ConfigOrName>,
        options: Option<IDebugSessionOptions>,
        save_before_start: Option<bool>,
    ) -> je_base::promise::Promise<bool>;
    /// `stopSession(session, disconnect)`.
    fn stop_session(&self, session: Option<std::rc::Rc<dyn IDebugSession>>, disconnect: bool) -> je_base::promise::Promise<()>;
}

