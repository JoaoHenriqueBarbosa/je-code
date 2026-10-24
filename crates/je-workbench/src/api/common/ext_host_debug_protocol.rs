//! Porte da região de depuração de `src/vs/workbench/api/common/extHost.protocol.ts`: `MainThreadDebugServiceShape`,
//! `ExtHostDebugServiceShape`, os DTOs de sessão, de breakpoint e de foco, e os identificadores
//! `MainContext.MainThreadDebugService` e `ExtHostContext.ExtHostDebugService`. O `ext_host_protocol.rs` reexporta tudo
//! daqui.
//!
//! Divergências:
//! - O prefixo `$` dos métodos de RPC vira `rpc_` (`$acceptDAMessage` é `rpc_accept_da_message`). Os métodos que a
//!   implementação do original declara `async` (`$acceptDebugSessionStarted`, `$acceptStackFrameFocus` e os demais
//!   `$accept*` de sessão) devolvem `Promise<()>` em vez do `void` da interface, para o chamador poder esperá-los (o
//!   `await` dos testes).
//! - `UriComponents` já chega como [`URI`]. `IConfig` é o objeto JSON cru ([`IConfig`]). Os `DebugProtocol.Breakpoint`,
//!   `RunInTerminalRequestArguments`, o `args: any` do `$customDebugAdapterRequest` e o `event: any` do
//!   `$acceptDebugSessionCustomEvent` são [`serde_json::Value`].
//! - `IDebugSessionDto = IDebugSessionFullDto | DebugSessionUUID` é [`IDebugSessionDto`]; `IThreadFocusDto |
//!   IStackFrameFocusDto` é [`StackFocusDto`]; `string | IDebugConfiguration` é [`NameOrConfig`].
//! - `IBreakpointDto` é o struct [`IBreakpointDto`] que os DTOs concretos guardam em `base` (o `extends` do original).
//!   `line` e `character` são inteiros.
//! - `null | undefined` do resultado de `$resolveDebugConfiguration` são `None`.

use je_base::cancellation::CancellationToken;
use je_base::lifecycle::IDisposable;
use je_base::promise::Promise;
use je_base::uri::URI;
use serde_json::Value;

use super::ext_host_protocol::{ExtHostContext, MainContext, ProxyIdentifier};
use crate::contrib::debug::debug::{
    DebugConfigurationProviderTriggerKind, IAdapterDescriptor, IConfig, IDebugSessionReplMode, IDebugTestRunReference, IDebugVisualization,
    IDebugVisualizationContext, IDebugVisualizationTreeItem, MainThreadDebugVisualization,
};
use crate::contrib::debug::debug_protocol::{DataBreakpointAccessType, ProtocolMessage};

/// `DebugSessionUUID`.
pub type DebugSessionUuid = String;

/// `string | IDebugConfiguration`: o nome da configuração ou a configuração inteira (`type`, `name`, `request` e as
/// chaves livres).
#[derive(Clone, Debug, PartialEq)]
pub enum NameOrConfig {
    Name(String),
    Config(IConfig),
}

/// `IStartDebuggingOptions`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct IStartDebuggingOptions {
    pub parent_session_id: Option<DebugSessionUuid>,
    pub lifecycle_managed_by_parent: Option<bool>,
    pub repl: Option<IDebugSessionReplMode>,
    pub no_debug: Option<bool>,
    pub compact: Option<bool>,
    pub suppress_debug_toolbar: Option<bool>,
    pub suppress_debug_statusbar: Option<bool>,
    pub suppress_debug_view: Option<bool>,
    pub suppress_save_before_start: Option<bool>,
    pub test_run: Option<IDebugTestRunReference>,
}

/// `IBreakpointDto` sem o `type` (que é a variante de [`BreakpointDto`]).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct IBreakpointDto {
    pub id: Option<String>,
    pub enabled: bool,
    pub condition: Option<String>,
    pub hit_condition: Option<String>,
    pub log_message: Option<String>,
    pub mode: Option<String>,
}

/// `IFunctionBreakpointDto`.
#[derive(Clone, Debug, PartialEq)]
pub struct IFunctionBreakpointDto {
    pub base: IBreakpointDto,
    pub function_name: String,
}

/// `IDataBreakpointDto`.
#[derive(Clone, Debug, PartialEq)]
pub struct IDataBreakpointDto {
    pub base: IBreakpointDto,
    pub data_id: String,
    pub can_persist: bool,
    pub label: String,
    pub access_types: Option<Vec<DataBreakpointAccessType>>,
    pub access_type: DataBreakpointAccessType,
}

/// `ISourceBreakpointDto`.
#[derive(Clone, Debug, PartialEq)]
pub struct ISourceBreakpointDto {
    pub base: IBreakpointDto,
    pub uri: URI,
    pub line: i64,
    pub character: i64,
}

/// `ISourceBreakpointDto | IFunctionBreakpointDto | IDataBreakpointDto`.
#[derive(Clone, Debug, PartialEq)]
pub enum BreakpointDto {
    Source(ISourceBreakpointDto),
    Function(IFunctionBreakpointDto),
    Data(IDataBreakpointDto),
}

/// `IBreakpointsDeltaDto`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct IBreakpointsDeltaDto {
    pub added: Option<Vec<BreakpointDto>>,
    pub removed: Option<Vec<String>>,
    pub changed: Option<Vec<BreakpointDto>>,
}

/// Um item de `ISourceMultiBreakpointDto.lines`.
#[derive(Clone, Debug, PartialEq)]
pub struct SourceMultiBreakpointLine {
    pub id: String,
    pub enabled: bool,
    pub condition: Option<String>,
    pub hit_condition: Option<String>,
    pub log_message: Option<String>,
    pub line: i64,
    pub character: i64,
    pub mode: Option<String>,
}

/// `ISourceMultiBreakpointDto`.
#[derive(Clone, Debug, PartialEq)]
pub struct ISourceMultiBreakpointDto {
    pub uri: URI,
    pub lines: Vec<SourceMultiBreakpointLine>,
}

/// `ISourceMultiBreakpointDto | IFunctionBreakpointDto | IDataBreakpointDto`, o item de `$registerBreakpoints`.
#[derive(Clone, Debug, PartialEq)]
pub enum RegisterBreakpointDto {
    SourceMulti(ISourceMultiBreakpointDto),
    Function(IFunctionBreakpointDto),
    Data(IDataBreakpointDto),
}

/// `IDebugSessionFullDto`.
#[derive(Clone, Debug, PartialEq)]
pub struct IDebugSessionFullDto {
    pub id: DebugSessionUuid,
    pub r#type: String,
    pub name: String,
    pub parent: Option<DebugSessionUuid>,
    pub folder_uri: Option<URI>,
    pub configuration: IConfig,
}

/// `IDebugSessionDto`: a sessão inteira ou só o `DebugSessionUUID` (quando o host de extensões já a guardou).
#[derive(Clone, Debug, PartialEq)]
pub enum IDebugSessionDto {
    Id(DebugSessionUuid),
    Full(IDebugSessionFullDto),
}

/// `IThreadFocusDto`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IThreadFocusDto {
    pub session_id: String,
    pub thread_id: i64,
}

/// `IStackFrameFocusDto`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IStackFrameFocusDto {
    pub session_id: String,
    pub thread_id: i64,
    pub frame_id: i64,
}

/// `IThreadFocusDto | IStackFrameFocusDto` (o `kind` é a variante).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StackFocusDto {
    Thread(IThreadFocusDto),
    StackFrame(IStackFrameFocusDto),
}

/// `MainThreadDebugServiceShape`.
pub trait MainThreadDebugServiceShape: IDisposable {
    fn rpc_register_debug_types(&self, debug_types: Vec<String>);
    fn rpc_session_cached(&self, session_id: &str);
    fn rpc_accept_da_message(&self, handle: u32, message: ProtocolMessage);
    fn rpc_accept_da_error(&self, handle: u32, name: &str, message: &str, stack: Option<&str>);
    fn rpc_accept_da_exit(&self, handle: u32, code: Option<i32>, signal: Option<&str>);
    fn rpc_register_debug_configuration_provider(
        &self,
        r#type: &str,
        trigger_kind: DebugConfigurationProviderTriggerKind,
        has_provide_method: bool,
        has_resolve_method: bool,
        has_resolve2_method: bool,
        handle: u32,
    ) -> Promise<()>;
    fn rpc_register_debug_adapter_descriptor_factory(&self, r#type: &str, handle: u32) -> Promise<()>;
    fn rpc_unregister_debug_configuration_provider(&self, handle: u32);
    fn rpc_unregister_debug_adapter_descriptor_factory(&self, handle: u32);
    fn rpc_start_debugging(&self, folder: Option<URI>, name_or_config: NameOrConfig, options: IStartDebuggingOptions) -> Promise<bool>;
    fn rpc_stop_debugging(&self, session_id: Option<&str>) -> Promise<()>;
    fn rpc_set_debug_session_name(&self, id: &str, name: &str);
    fn rpc_custom_debug_adapter_request(&self, id: &str, command: &str, args: Option<Value>) -> Promise<Option<Value>>;
    fn rpc_get_debug_protocol_breakpoint(&self, id: &str, breakpoint_id: &str) -> Promise<Option<Value>>;
    fn rpc_append_debug_console(&self, value: &str);
    fn rpc_register_breakpoints(&self, breakpoints: Vec<RegisterBreakpointDto>) -> Promise<()>;
    fn rpc_unregister_breakpoints(&self, breakpoint_ids: Vec<String>, function_breakpoint_ids: Vec<String>, data_breakpoint_ids: Vec<String>) -> Promise<()>;
    fn rpc_register_debug_visualizer(&self, extension_id: &str, id: &str);
    fn rpc_unregister_debug_visualizer(&self, extension_id: &str, id: &str);
    fn rpc_register_debug_visualizer_tree(&self, tree_id: &str, can_edit: bool);
    fn rpc_unregister_debug_visualizer_tree(&self, tree_id: &str);
}

/// `ExtHostDebugServiceShape`.
pub trait ExtHostDebugServiceShape {
    fn rpc_substitute_variables(&self, folder: Option<URI>, config: IConfig) -> Promise<IConfig>;
    fn rpc_run_in_terminal(&self, args: Value, session_id: &str) -> Promise<Option<i64>>;
    fn rpc_start_da_session(&self, handle: u32, session: IDebugSessionDto) -> Promise<()>;
    fn rpc_stop_da_session(&self, handle: u32) -> Promise<()>;
    fn rpc_send_da_message(&self, handle: u32, message: ProtocolMessage);
    fn rpc_resolve_debug_configuration(&self, handle: u32, folder: Option<URI>, debug_configuration: IConfig, token: CancellationToken) -> Promise<Option<IConfig>>;
    fn rpc_resolve_debug_configuration_with_substituted_variables(
        &self,
        handle: u32,
        folder: Option<URI>,
        debug_configuration: IConfig,
        token: CancellationToken,
    ) -> Promise<Option<IConfig>>;
    fn rpc_provide_debug_configurations(&self, handle: u32, folder: Option<URI>, token: CancellationToken) -> Promise<Vec<IConfig>>;
    fn rpc_provide_debug_adapter(&self, handle: u32, session: IDebugSessionDto) -> Promise<IAdapterDescriptor>;
    fn rpc_accept_debug_session_started(&self, session: IDebugSessionDto) -> Promise<()>;
    fn rpc_accept_debug_session_terminated(&self, session: IDebugSessionDto) -> Promise<()>;
    fn rpc_accept_debug_session_active_changed(&self, session: Option<IDebugSessionDto>) -> Promise<()>;
    fn rpc_accept_debug_session_custom_event(&self, session: IDebugSessionDto, event: Value) -> Promise<()>;
    fn rpc_accept_breakpoints_delta(&self, delta: IBreakpointsDeltaDto);
    fn rpc_accept_debug_session_name_changed(&self, session: IDebugSessionDto, name: &str) -> Promise<()>;
    fn rpc_accept_stack_frame_focus(&self, focus: Option<StackFocusDto>) -> Promise<()>;
    fn rpc_provide_debug_visualizers(&self, extension_id: &str, id: &str, context: IDebugVisualizationContext, token: CancellationToken) -> Promise<Vec<IDebugVisualization>>;
    fn rpc_resolve_debug_visualizer(&self, id: u64, token: CancellationToken) -> Promise<MainThreadDebugVisualization>;
    fn rpc_execute_debug_visualizer_command(&self, id: u64) -> Promise<()>;
    fn rpc_dispose_debug_visualizers(&self, ids: Vec<u64>);
    fn rpc_get_visualizer_tree_item(&self, tree_id: &str, element: IDebugVisualizationContext) -> Promise<Option<IDebugVisualizationTreeItem>>;
    fn rpc_get_visualizer_tree_item_children(&self, tree_id: &str, element: u64) -> Promise<Vec<IDebugVisualizationTreeItem>>;
    fn rpc_edit_visualizer_tree_item(&self, element: u64, value: &str) -> Promise<Option<IDebugVisualizationTreeItem>>;
    fn rpc_dispose_visualized_tree(&self, element: u64);
}

impl MainContext {
    pub const MAIN_THREAD_DEBUG_SERVICE: ProxyIdentifier<dyn MainThreadDebugServiceShape> = ProxyIdentifier::new("MainThreadDebugService");
}

impl ExtHostContext {
    pub const EXT_HOST_DEBUG_SERVICE: ProxyIdentifier<dyn ExtHostDebugServiceShape> = ProxyIdentifier::new("ExtHostDebugService");
}
