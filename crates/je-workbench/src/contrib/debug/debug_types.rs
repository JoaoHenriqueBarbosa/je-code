//! Porte do restante de `src/vs/workbench/contrib/debug/common/debug.ts` que o modelo (`debugModel.ts`), a sessão lógica
//! (`debugSession.ts`), o `ViewModel` e o `ReplModel` usam: `State`, os dados brutos de atualização, as interfaces de
//! expressão e o contrato completo da sessão.
//!
//! Notas de tradução:
//! - **Tipos do DAP.** O `debugProtocol.d.ts` inteiro não foi portado (só as mensagens-base em `debug_protocol.rs`). Os
//!   argumentos e corpos dos pedidos, as capacidades (`DebugProtocol.Capabilities`) e os eventos tipados (`StoppedEvent`,
//!   `OutputEvent`...) são [`serde_json::Value`], como o `any` do original; as respostas são [`Response`] (o `body` é o `Value`).
//!   Capacidade ausente é `false`: [`capability`].
//! - **Contrato da sessão.** [`IDebugSessionFull`] estende o [`IDebugSession`] mínimo de `debug.rs` (que o
//!   `MainThreadDebugService` usa) com o resto da interface de `debug.ts`; os métodos do contrato mínimo não se repetem.
//! - **`IThread` e `IStackFrame`** do original são as structs [`Thread`](super::debug_model::Thread) e
//!   [`StackFrame`](super::debug_model::StackFrame) (únicas implementações; os testes só as estendem para trocar o
//!   `getCallStack`, o que no porte é [`Thread::set_call_stack`](super::debug_model::Thread::set_call_stack)).
//! - **`stoppedDetails` compartilhado.** O objeto é o mesmo na thread e na lista da sessão (o `totalFrames` que a thread
//!   grava aparece em `session.getStoppedDetails()`), então é [`StoppedDetails`], um `Rc<RefCell<..>>`.

use std::cell::RefCell;
use std::rc::Rc;

use je_base::buffer::VSBuffer;
use je_base::cancellation::CancellationToken;
use je_base::event::Event;
use je_base::lifecycle::IDisposable;
use je_base::promise::Promise;
use je_base::uri::URI;
use serde_json::Value;

use super::debug::{IConfig, IDebugAdapter, IDebugSession};
use super::debug_model::{
    Breakpoint, DataBreakpoint, ExceptionBreakpoint, FunctionBreakpoint, InstructionBreakpoint, MemoryRegion, StackFrame, Thread,
};
use super::debug_protocol::{Event as ProtocolEvent, Response, Source as ProtocolSource};
use super::debug_source::Source;
use super::repl_model::{NewReplElementData, ReplElement};

/// `State`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    Inactive,
    Initializing,
    Stopped,
    Running,
}

/// `getStateLabel(state)`.
pub fn get_state_label(state: State) -> &'static str {
    match state {
        State::Initializing => "initializing",
        State::Stopped => "stopped",
        State::Running => "running",
        State::Inactive => "inactive",
    }
}

/// `!!capabilities[key]`: capacidade ausente ou `false`.
pub fn capability(capabilities: &Value, key: &str) -> bool {
    match capabilities.get(key) {
        Some(Value::Bool(value)) => *value,
        Some(Value::Null) | None => false,
        Some(Value::Number(number)) => number.as_f64().is_some_and(|n| n != 0.0),
        Some(Value::String(text)) => !text.is_empty(),
        Some(_) => true,
    }
}

/// `IRawStoppedDetails`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct IRawStoppedDetails {
    pub reason: Option<String>,
    pub description: Option<String>,
    pub thread_id: Option<i64>,
    pub text: Option<String>,
    pub total_frames: Option<i64>,
    pub all_threads_stopped: Option<bool>,
    pub preserve_focus_hint: Option<bool>,
    pub frames_error_message: Option<String>,
    pub hit_breakpoint_ids: Option<Vec<i64>>,
}

impl IRawStoppedDetails {
    /// O corpo de um `StoppedEvent` do DAP.
    pub fn from_value(body: &Value) -> Self {
        let text = |key: &str| body.get(key).and_then(Value::as_str).map(str::to_owned);
        let number = |key: &str| body.get(key).and_then(Value::as_i64);
        let flag = |key: &str| body.get(key).and_then(Value::as_bool);
        Self {
            reason: text("reason"),
            description: text("description"),
            thread_id: number("threadId"),
            text: text("text"),
            total_frames: number("totalFrames"),
            all_threads_stopped: flag("allThreadsStopped"),
            preserve_focus_hint: flag("preserveFocusHint"),
            frames_error_message: text("framesErrorMessage"),
            hit_breakpoint_ids: body.get("hitBreakpointIds").and_then(Value::as_array).map(|ids| ids.iter().filter_map(Value::as_i64).collect()),
        }
    }
}

/// O `IRawStoppedDetails` que thread e sessão compartilham por referência.
pub type StoppedDetails = Rc<RefCell<IRawStoppedDetails>>;

/// `{ ...details }` como o objeto compartilhado: o `stoppedDetails` de um `IRawModelUpdate` é o mesmo objeto que a sessão
/// guarda na lista e a thread recebe (o `totalFrames` que a thread grava aparece na sessão).
pub fn new_stopped_details(details: IRawStoppedDetails) -> StoppedDetails {
    Rc::new(RefCell::new(details))
}

/// `DebugProtocol.Thread`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DapThread {
    pub id: i64,
    pub name: String,
}

/// `IRawModelUpdate`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct IRawModelUpdate {
    pub session_id: String,
    pub threads: Vec<DapThread>,
    pub stopped_details: Option<StoppedDetails>,
}

/// `IBreakpointUpdateData`: cada campo `Option<Option<..>>` distingue "ausente" (`None`, o `hasOwnProperty` falso) de
/// "presente com `undefined`" (`Some(None)`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct IBreakpointUpdateData {
    pub condition: Option<Option<String>>,
    pub hit_condition: Option<Option<String>>,
    pub log_message: Option<Option<String>>,
    pub line_number: Option<Option<i64>>,
    pub column: Option<Option<i64>>,
    pub mode: Option<Option<String>>,
    pub mode_label: Option<Option<String>>,
    pub triggered_by: Option<Option<String>>,
}

/// `IExceptionInfo`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct IExceptionInfo {
    pub id: Option<String>,
    pub description: Option<String>,
    pub break_mode: Option<String>,
    pub details: Option<Value>,
}

/// `IDebugEvaluatePosition`.
#[derive(Clone, Debug, PartialEq)]
pub struct IDebugEvaluatePosition {
    pub line: i64,
    pub column: i64,
    pub source: ProtocolSource,
}

/// `IDebugLocationReferenced`.
#[derive(Clone)]
pub struct IDebugLocationReferenced {
    pub line: i64,
    pub column: i64,
    pub end_line: Option<i64>,
    pub end_column: Option<i64>,
    pub source: Rc<Source>,
}

/// `IDataBreakpointInfoResponse`.
#[derive(Clone, Debug, PartialEq)]
pub struct IDataBreakpointInfoResponse {
    pub data_id: Option<String>,
    pub description: String,
    pub can_persist: Option<bool>,
    pub access_types: Option<Vec<super::debug_protocol::DataBreakpointAccessType>>,
}

/// `AdapterEndEvent`.
#[derive(Clone, Debug)]
pub struct AdapterEndEvent {
    pub error: Option<je_base::errors::Error>,
    pub session_length_in_seconds: f64,
    pub emitted_stopped: bool,
}

/// `LoadedSourceEvent.reason`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LoadedSourceReason {
    New,
    Changed,
    Removed,
}

/// `LoadedSourceEvent`.
#[derive(Clone)]
pub struct LoadedSourceEvent {
    pub reason: LoadedSourceReason,
    pub source: Rc<Source>,
}

/// `IMemoryInvalidationEvent`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IMemoryInvalidationEvent {
    pub from_offset: i64,
    pub to_offset: i64,
}

/// `MemoryRange` (`IValidMemoryRange | IUnreadableMemoryRange | IErrorMemoryRange`): o `type` é a variante. Sem `PartialEq`
/// porque o `VSBuffer` não o tem: os testes comparam [`MemoryRange::summary`].
#[derive(Clone, Debug)]
pub enum MemoryRange {
    Valid { offset: i64, length: i64, data: VSBuffer },
    Unreadable { offset: i64, length: i64 },
    Error { offset: i64, length: i64, error: String },
}

impl MemoryRange {
    /// Uma forma comparável: `(tipo, offset, length, bytes do dado ou mensagem de erro)`.
    pub fn summary(&self) -> (&'static str, i64, i64, Vec<u8>) {
        match self {
            MemoryRange::Valid { offset, length, data } => ("valid", *offset, *length, data.to_vec()),
            MemoryRange::Unreadable { offset, length } => ("unreadable", *offset, *length, Vec::new()),
            MemoryRange::Error { offset, length, error } => ("error", *offset, *length, error.clone().into_bytes()),
        }
    }
}

/// `ITreeElement`.
pub trait ITreeElement {
    fn get_id(&self) -> String;
}

/// `IExpressionContainer` (inclui o `IExpressionValue`).
pub trait IExpressionContainer {
    fn get_id(&self) -> String;
    fn value(&self) -> String;
    fn expr_type(&self) -> Option<String>;
    fn value_changed(&self) -> bool;
    fn has_children(&self) -> bool;
    fn get_session(&self) -> Option<Rc<dyn IDebugSessionFull>>;
    fn evaluate_lazy(&self) -> Promise<()>;
    fn get_children(&self) -> Promise<Vec<Rc<dyn IExpression>>>;
    fn reference(&self) -> Option<i64>;
    fn memory_reference(&self) -> Option<String>;
    fn presentation_hint(&self) -> Option<Value>;
    fn value_location_reference(&self) -> Option<i64>;
    /// `toString()`.
    fn display(&self) -> String {
        self.value()
    }
    /// O `available` das classes que o têm (`Expression`, `Variable`); `None` para as demais.
    fn availability(&self) -> Option<bool> {
        None
    }
    /// `valueChanged = false`: o renderizador consome o realce de "mudou".
    fn clear_value_changed(&self) {}
}

/// `IExpression`.
pub trait IExpression: IExpressionContainer {
    fn name(&self) -> String;

    /// `element instanceof Variable` (o único tipo concreto que o filtro do REPL distingue entre as expressões).
    fn is_variable(&self) -> bool {
        false
    }

    /// `element as Variable`, quando a expressão é uma `Variable`.
    fn as_variable(&self) -> Option<&super::debug_model::Variable> {
        None
    }

    /// `element instanceof Variable`, com a posse do `Rc` (as chamadas assíncronas, como `setVariable`, precisam dele).
    fn into_variable(self: Rc<Self>) -> Option<Rc<super::debug_model::Variable>> {
        None
    }

    /// `element instanceof Expression`, com a posse do `Rc`.
    fn into_expression(self: Rc<Self>) -> Option<Rc<super::debug_model::Expression>> {
        None
    }
}

/// `ITelemetryEndpoint`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ITelemetryEndpoint {
    pub id: String,
    pub ai_key: String,
    pub send_error_telemetry: bool,
}

/// `IDebugger`, só o que a sessão e a sessão bruta usam.
pub trait IDebugger {
    fn debug_type(&self) -> String;
    fn create_debug_adapter(&self, session: Rc<dyn IDebugSessionFull>) -> Promise<Rc<dyn IDebugAdapter>>;
    fn run_in_terminal(&self, args: Value, session_id: &str) -> Promise<Option<i64>>;
    fn start_debugging(&self, args: IConfig, parent_session_id: &str) -> Promise<bool>;
    fn get_custom_telemetry_endpoint(&self) -> Option<ITelemetryEndpoint>;
}

/// O corpo de `DebugProtocol.StackTraceResponse`/`ScopesResponse`... é `Value`; as respostas são `Option<Response>`.
pub type DapResponse = Option<Response>;

/// `IDebugSession` completo (o contrato mínimo está em [`IDebugSession`]).
pub trait IDebugSessionFull: IDebugSession + IDisposable {
    fn state(&self) -> State;
    fn unresolved_configuration(&self) -> Option<IConfig>;
    fn sub_id(&self) -> Option<String>;
    fn set_sub_id(&self, sub_id: Option<String>);
    fn compact(&self) -> bool;
    fn save_before_restart(&self) -> bool;
    fn auto_expand_lazy_variables(&self) -> bool;
    fn suppress_debug_toolbar(&self) -> bool;
    fn suppress_debug_statusbar(&self) -> bool;
    fn suppress_debug_view(&self) -> bool;
    fn lifecycle_managed_by_parent(&self) -> bool;
    fn get_label(&self) -> String;
    fn get_memory(&self, memory_reference: &str) -> Rc<MemoryRegion>;
    fn get_source_for_uri(&self, model_uri: &URI) -> Option<Rc<Source>>;
    fn get_source(&self, raw: Option<ProtocolSource>) -> Rc<Source>;
    fn set_configuration(&self, resolved: IConfig, unresolved: Option<IConfig>);
    fn raw_update(&self, data: IRawModelUpdate);
    fn get_thread(&self, thread_id: i64) -> Option<Rc<Thread>>;
    fn get_all_threads(&self) -> Vec<Rc<Thread>>;
    fn clear_threads(&self, remove_threads: bool, reference: Option<i64>);
    fn get_stopped_details(&self) -> Option<StoppedDetails>;
    /// O `ReplModel` da sessão (o `(this.parentSession as DebugSession).repl` da filha que o compartilha).
    fn repl_model(&self) -> Rc<super::repl_model::ReplModel>;
    fn get_repl_elements(&self) -> Vec<ReplElement>;
    fn has_separate_repl(&self) -> bool;
    fn remove_repl_expressions(&self);
    fn add_repl_expression(&self, stack_frame: Option<Rc<StackFrame>>, name: &str) -> Promise<()>;
    /// `appendToRepl(data, isImportant?)` com o `INewReplElementData` completo do `replModel.ts`.
    fn append_to_repl_data(&self, data: NewReplElementData, is_important: bool);
    fn cancel_correlated_test_run(&self);
    fn on_did_end_adapter(&self) -> Event<Option<AdapterEndEvent>>;
    fn on_did_change_state(&self) -> Event<()>;
    fn on_did_change_repl_elements(&self) -> Event<Option<ReplElement>>;
    /// `capabilities`: vazio (`{}`) sem sessão bruta.
    fn capabilities(&self) -> Value;
    fn remembered_capabilities(&self) -> Option<Value>;
    fn on_did_loaded_source(&self) -> Event<LoadedSourceEvent>;
    fn on_did_progress_start(&self) -> Event<ProtocolEvent>;
    fn on_did_progress_update(&self) -> Event<ProtocolEvent>;
    fn on_did_progress_end(&self) -> Event<ProtocolEvent>;
    fn on_did_invalidate_memory(&self) -> Event<ProtocolEvent>;

    // pedidos do DAP
    fn initialize(&self, dbgr: Rc<dyn IDebugger>) -> Promise<()>;
    fn launch_or_attach(&self, config: IConfig) -> Promise<()>;
    fn restart(&self) -> Promise<()>;
    fn terminate(&self, restart: bool) -> Promise<()>;
    fn disconnect(&self, restart: bool, suspend: bool) -> Promise<()>;
    fn send_breakpoints(&self, model_uri: URI, breakpoints: Vec<Rc<Breakpoint>>, source_modified: bool) -> Promise<()>;
    fn send_function_breakpoints(&self, fbps: Vec<Rc<FunctionBreakpoint>>) -> Promise<()>;
    fn data_breakpoint_info(&self, name: &str, variables_reference: Option<i64>, frame_id: Option<i64>) -> Promise<Option<IDataBreakpointInfoResponse>>;
    fn data_bytes_breakpoint_info(&self, address: &str, bytes: Option<i64>) -> Promise<Option<IDataBreakpointInfoResponse>>;
    fn send_data_breakpoints(&self, dbps: Vec<Rc<DataBreakpoint>>) -> Promise<()>;
    fn send_instruction_breakpoints(&self, ibps: Vec<Rc<InstructionBreakpoint>>) -> Promise<()>;
    fn send_exception_breakpoints(&self, exbpts: Vec<Rc<ExceptionBreakpoint>>) -> Promise<()>;
    fn breakpoints_locations(&self, uri: &URI, line_number: i64) -> Promise<Vec<(i64, i64)>>;
    fn resolve_location_reference(&self, location_reference: i64) -> Promise<IDebugLocationReferenced>;
    fn stack_trace(&self, thread_id: i64, start_frame: i64, levels: i64, token: CancellationToken) -> Promise<DapResponse>;
    fn exception_info(&self, thread_id: i64) -> Promise<Option<IExceptionInfo>>;
    fn scopes(&self, frame_id: i64, thread_id: i64) -> Promise<DapResponse>;
    fn variables(&self, variables_reference: i64, thread_id: Option<i64>, filter: Option<&str>, start: Option<i64>, count: Option<i64>) -> Promise<DapResponse>;
    fn evaluate(&self, expression: &str, frame_id: Option<i64>, context: Option<&str>, location: Option<IDebugEvaluatePosition>) -> Promise<DapResponse>;
    fn restart_frame(&self, frame_id: i64, thread_id: i64) -> Promise<()>;
    fn next(&self, thread_id: i64, granularity: Option<String>) -> Promise<()>;
    fn step_in(&self, thread_id: i64, target_id: Option<i64>, granularity: Option<String>) -> Promise<()>;
    fn step_out(&self, thread_id: i64, granularity: Option<String>) -> Promise<()>;
    fn step_back(&self, thread_id: i64, granularity: Option<String>) -> Promise<()>;
    fn continue_(&self, thread_id: i64) -> Promise<()>;
    fn reverse_continue(&self, thread_id: i64) -> Promise<()>;
    fn pause(&self, thread_id: i64) -> Promise<()>;
    fn terminate_threads(&self, thread_ids: Option<Vec<i64>>) -> Promise<()>;
    fn set_variable(&self, variables_reference: i64, name: &str, value: &str) -> Promise<DapResponse>;
    fn set_expression(&self, frame_id: i64, expression: &str, value: &str) -> Promise<DapResponse>;
    fn goto_targets(&self, source: ProtocolSource, line: i64, column: Option<i64>) -> Promise<DapResponse>;
    fn goto(&self, thread_id: i64, target_id: i64) -> Promise<DapResponse>;
    fn load_source(&self, resource: &URI) -> Promise<DapResponse>;
    fn get_loaded_sources(&self) -> Promise<Vec<Rc<Source>>>;
    fn completions(&self, frame_id: Option<i64>, thread_id: i64, text: &str, line_number: i64, column: i64, token: CancellationToken) -> Promise<DapResponse>;
    fn step_in_targets(&self, frame_id: i64) -> Promise<Option<Value>>;
    fn cancel(&self, progress_id: &str) -> Promise<DapResponse>;
    fn disassemble(&self, memory_reference: &str, offset: i64, instruction_offset: i64, instruction_count: i64) -> Promise<Option<Value>>;
    fn read_memory(&self, memory_reference: &str, offset: i64, count: i64) -> Promise<DapResponse>;
    fn write_memory(&self, memory_reference: &str, offset: i64, data: &str, allow_partial: Option<bool>) -> Promise<DapResponse>;
}
