//! Porte do que o host de extensões de depuração usa de `src/vs/workbench/api/common/extHostTypes.ts` (as classes de
//! breakpoint, os descritores de adaptador, `DebugStackFrame`, `DebugThread`, `DebugConsoleMode`, `DebugVisualization`) e
//! dos contratos `vscode.Debug*` do `vscode.d.ts` e de `vscode.proposed.debugVisualization.d.ts` (provedores, fábricas,
//! `DebugAdapterTracker`, `DebugSessionOptions`).
//!
//! Divergências:
//! - **Breakpoints.** `Breakpoint`, `SourceBreakpoint`, `FunctionBreakpoint` e `DataBreakpoint` são o struct
//!   [`Breakpoint`] com o [`BreakpointKind`] (o `instanceof` do original é um `match`). Os campos que o host muda no lugar
//!   (`$acceptBreakpointsDelta` faz `fbp.enabled = ...` por cast para `any`) ficam em `RefCell`/`Cell`. O `id` é o do
//!   `WeakMap breakpointIds` quando foi fixado por [`Breakpoint::set_id`] (o `setBreakpointId`), ou um uuid gerado no
//!   primeiro uso. O `null` do `location` do construtor não existe.
//! - **Interfaces.** `DebugConfigurationProvider`, `DebugAdapterDescriptorFactory`, `DebugAdapterTrackerFactory`,
//!   `DebugAdapterTracker`, `DebugVisualizationTree` e `DebugVisualizationProvider` são traits. Um membro opcional do
//!   original (`provideDebugConfigurations?`) é um método com corpo padrão mais o `has_*` que diz se o provedor o tem (o
//!   `!!provider.provideDebugConfigurations`). `ProviderResult<T>` é [`ProviderResult`] (`MaybePromise<Option<T>>`): o
//!   `null` e o `undefined` são `None`.
//! - `vscode.DebugAdapterDescriptor` é o enum [`DebugAdapterDescriptor`]. `vscode.DebugAdapter` é o trait [`DebugAdapter`],
//!   cujo `dispose` pode lançar (pânico), como o do original. O `DebugProtocolMessage` é a `ProtocolMessage`.
//! - `DebugTreeItem` é o struct [`DebugTreeItem`], guardado em `Rc` (a identidade do objeto é a que o host usa para dar o
//!   id do item). O `T extends DebugTreeItem` genérico do original é sempre o próprio `DebugTreeItem`.
//! - `DebugVisualization.iconPath` é o [`IconPath`] da API (`Uri | { light, dark } | ThemeIcon`) e
//!   `visualization: Command | { treeId }` é o [`DebugVisualizationTarget`].
//! - `DebugVisualizationContext.variable` (`any`) é a [`Variable`] do protocolo.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use je_base::cancellation::CancellationToken;
use je_base::errors::Error;
use je_base::event::Event;
use je_base::lifecycle::IDisposable;
use je_base::promise::MaybePromise;
use je_base::uuid::generate_uuid;

use super::ext_host_types::{Command, IconPath, Location};
use super::ext_host_workspace::WorkspaceFolder;
use crate::contrib::debug::debug::{DebugTreeItemCollapsibleState, IConfig, IDebugAdapterExecutableOptions};
use crate::contrib::debug::debug_protocol::{ProtocolMessage, Variable};

/// `vscode.ProviderResult<T>`: `T | undefined | null | Thenable<T | undefined | null>`.
pub type ProviderResult<T> = MaybePromise<Option<T>>;

// ---- breakpoints

/// A forma de um breakpoint: o que distingue as subclasses de `Breakpoint`.
#[derive(Clone, Debug)]
pub enum BreakpointKind {
    /// `SourceBreakpoint`.
    Source { location: Location },
    /// `FunctionBreakpoint`.
    Function { function_name: String },
    /// `DataBreakpoint`.
    Data { label: String, data_id: String, can_persist: bool },
}

/// `Breakpoint` e as suas subclasses.
pub struct Breakpoint {
    id: RefCell<Option<String>>,
    enabled: Cell<bool>,
    condition: RefCell<Option<String>>,
    hit_condition: RefCell<Option<String>>,
    log_message: RefCell<Option<String>>,
    mode: Option<String>,
    kind: RefCell<BreakpointKind>,
}

impl Breakpoint {
    /// O construtor protegido de `Breakpoint`: `enabled` ausente vale `true`.
    fn create(kind: BreakpointKind, enabled: Option<bool>, condition: Option<String>, hit_condition: Option<String>, log_message: Option<String>, mode: Option<String>) -> Rc<Breakpoint> {
        Rc::new(Breakpoint {
            id: RefCell::new(None),
            enabled: Cell::new(enabled.unwrap_or(true)),
            condition: RefCell::new(condition),
            hit_condition: RefCell::new(hit_condition),
            log_message: RefCell::new(log_message),
            mode,
            kind: RefCell::new(kind),
        })
    }

    /// `new SourceBreakpoint(location, enabled, condition, hitCondition, logMessage, mode)`.
    pub fn new_source(location: Location, enabled: Option<bool>, condition: Option<String>, hit_condition: Option<String>, log_message: Option<String>, mode: Option<String>) -> Rc<Breakpoint> {
        Self::create(BreakpointKind::Source { location }, enabled, condition, hit_condition, log_message, mode)
    }

    /// `new FunctionBreakpoint(functionName, enabled, condition, hitCondition, logMessage, mode)`.
    pub fn new_function(function_name: &str, enabled: Option<bool>, condition: Option<String>, hit_condition: Option<String>, log_message: Option<String>, mode: Option<String>) -> Rc<Breakpoint> {
        Self::create(BreakpointKind::Function { function_name: function_name.to_owned() }, enabled, condition, hit_condition, log_message, mode)
    }

    /// `new DataBreakpoint(label, dataId, canPersist, enabled, condition, hitCondition, logMessage, mode)`: um `dataId`
    /// vazio lança `Illegal argument: dataId`.
    #[allow(clippy::too_many_arguments)] // os oito parâmetros do construtor do original
    pub fn new_data(
        label: &str,
        data_id: &str,
        can_persist: bool,
        enabled: Option<bool>,
        condition: Option<String>,
        hit_condition: Option<String>,
        log_message: Option<String>,
        mode: Option<String>,
    ) -> Rc<Breakpoint> {
        if data_id.is_empty() {
            je_base::errors::throw(super::ext_host_types::illegal_argument(Some("dataId")));
        }
        Self::create(BreakpointKind::Data { label: label.to_owned(), data_id: data_id.to_owned(), can_persist }, enabled, condition, hit_condition, log_message, mode)
    }

    /// `setBreakpointId(bp, id)`: fixa o id interno (a chamada vem logo depois da construção).
    pub fn set_id(&self, id: &str) {
        *self.id.borrow_mut() = Some(id.to_owned());
    }

    /// `get id()`.
    pub fn id(&self) -> String {
        self.id.borrow_mut().get_or_insert_with(generate_uuid).clone()
    }

    pub fn enabled(&self) -> bool {
        self.enabled.get()
    }

    pub fn condition(&self) -> Option<String> {
        self.condition.borrow().clone()
    }

    pub fn hit_condition(&self) -> Option<String> {
        self.hit_condition.borrow().clone()
    }

    pub fn log_message(&self) -> Option<String> {
        self.log_message.borrow().clone()
    }

    pub fn mode(&self) -> Option<String> {
        self.mode.clone()
    }

    /// A subclasse e os campos próprios dela.
    pub fn kind(&self) -> BreakpointKind {
        self.kind.borrow().clone()
    }

    /// Os campos que `$acceptBreakpointsDelta` muda no lugar, comuns às subclasses.
    pub fn update_common(&self, enabled: bool, condition: Option<String>, hit_condition: Option<String>, log_message: Option<String>) {
        self.enabled.set(enabled);
        *self.condition.borrow_mut() = condition;
        *self.hit_condition.borrow_mut() = hit_condition;
        *self.log_message.borrow_mut() = log_message;
    }

    /// `fbp.functionName = ...` e `sbp.location = ...` (o `kind` troca o campo próprio).
    pub fn set_kind(&self, kind: BreakpointKind) {
        *self.kind.borrow_mut() = kind;
    }
}

// ---- descritores de adaptador

/// `DebugAdapterExecutable`.
#[derive(Clone, Debug, PartialEq)]
pub struct DebugAdapterExecutable {
    pub command: String,
    pub args: Vec<String>,
    pub options: Option<IDebugAdapterExecutableOptions>,
}

impl DebugAdapterExecutable {
    /// `new DebugAdapterExecutable(command, args, options)`: `args || []`.
    pub fn new(command: &str, args: Option<Vec<String>>, options: Option<IDebugAdapterExecutableOptions>) -> Self {
        Self { command: command.to_owned(), args: args.unwrap_or_default(), options }
    }
}

/// `DebugAdapterServer`.
#[derive(Clone, Debug, PartialEq)]
pub struct DebugAdapterServer {
    pub port: f64,
    pub host: Option<String>,
}

/// `DebugAdapterNamedPipeServer`.
#[derive(Clone, Debug, PartialEq)]
pub struct DebugAdapterNamedPipeServer {
    pub path: String,
}

/// `vscode.DebugAdapter`: a implementação de um adaptador dentro do host de extensões.
pub trait DebugAdapter: IDisposable {
    /// `onDidSendMessage`.
    fn on_did_send_message(&self) -> Event<ProtocolMessage>;
    /// `handleMessage(message)`.
    fn handle_message(&self, message: &ProtocolMessage);
}

/// `DebugAdapterInlineImplementation`.
#[derive(Clone)]
pub struct DebugAdapterInlineImplementation {
    pub implementation: Rc<dyn DebugAdapter>,
}

impl DebugAdapterInlineImplementation {
    pub fn new(implementation: Rc<dyn DebugAdapter>) -> Self {
        Self { implementation }
    }
}

/// `vscode.DebugAdapterDescriptor`: `DebugAdapterExecutable | DebugAdapterServer | DebugAdapterNamedPipeServer |
/// DebugAdapterInlineImplementation`.
#[derive(Clone)]
pub enum DebugAdapterDescriptor {
    Executable(DebugAdapterExecutable),
    Server(DebugAdapterServer),
    NamedPipeServer(DebugAdapterNamedPipeServer),
    Inline(DebugAdapterInlineImplementation),
}

// ---- sessão, pilha e opções

/// `DebugConsoleMode`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DebugConsoleMode {
    /// Debug session should have a separate debug console.
    Separate = 0,
    /// Debug session should share debug console with its parent session. This value has no effect for sessions which do
    /// not have a parent session.
    MergeWithParent = 1,
}

/// `vscode.DebugSessionOptions`. O `parentSession` e o `testRun` são os objetos da API; o `debugUI` de compatibilidade
/// (#147264) é `debug_ui_simple`.
#[derive(Clone, Default)]
pub struct DebugSessionOptions {
    pub parent_session: Option<Rc<super::ext_host_debug_service::DebugSession>>,
    pub lifecycle_managed_by_parent: Option<bool>,
    pub console_mode: Option<DebugConsoleMode>,
    pub no_debug: Option<bool>,
    pub compact: Option<bool>,
    pub suppress_save_before_start: Option<bool>,
    pub suppress_debug_toolbar: Option<bool>,
    pub suppress_debug_statusbar: Option<bool>,
    pub suppress_debug_view: Option<bool>,
    /// `debugUI?.simple`.
    pub debug_ui_simple: Option<bool>,
    pub test_run: Option<Rc<super::ext_host_testing::TestRun>>,
}

/// `DebugStackFrame`.
#[derive(Clone)]
pub struct DebugStackFrame {
    pub session: Rc<super::ext_host_debug_service::DebugSession>,
    pub thread_id: i64,
    pub frame_id: i64,
}

/// `DebugThread`.
#[derive(Clone)]
pub struct DebugThread {
    pub session: Rc<super::ext_host_debug_service::DebugSession>,
    pub thread_id: i64,
}

/// `DebugThread | DebugStackFrame`, o item ativo da pilha.
#[derive(Clone)]
pub enum DebugStackItem {
    Thread(DebugThread),
    StackFrame(DebugStackFrame),
}

/// `vscode.BreakpointsChangeEvent`.
#[derive(Clone)]
pub struct BreakpointsChangeEvent {
    pub added: Vec<Rc<Breakpoint>>,
    pub removed: Vec<Rc<Breakpoint>>,
    pub changed: Vec<Rc<Breakpoint>>,
}

/// `vscode.DebugSessionCustomEvent`.
#[derive(Clone)]
pub struct DebugSessionCustomEvent {
    pub session: Rc<super::ext_host_debug_service::DebugSession>,
    pub event: String,
    pub body: Option<serde_json::Value>,
}

// ---- provedores e fábricas

/// `vscode.DebugConfigurationProvider`.
pub trait DebugConfigurationProvider {
    fn has_provide_debug_configurations(&self) -> bool {
        false
    }

    fn has_resolve_debug_configuration(&self) -> bool {
        false
    }

    fn has_resolve_debug_configuration_with_substituted_variables(&self) -> bool {
        false
    }

    /// `provideDebugConfigurations(folder, token)`.
    fn provide_debug_configurations(&self, _folder: Option<Rc<WorkspaceFolder>>, _token: CancellationToken) -> ProviderResult<Vec<IConfig>> {
        MaybePromise::Value(None)
    }

    /// `resolveDebugConfiguration(folder, debugConfiguration, token)`.
    fn resolve_debug_configuration(&self, _folder: Option<Rc<WorkspaceFolder>>, _debug_configuration: IConfig, _token: CancellationToken) -> ProviderResult<IConfig> {
        MaybePromise::Value(None)
    }

    /// `resolveDebugConfigurationWithSubstitutedVariables(folder, debugConfiguration, token)`.
    fn resolve_debug_configuration_with_substituted_variables(&self, _folder: Option<Rc<WorkspaceFolder>>, _debug_configuration: IConfig, _token: CancellationToken) -> ProviderResult<IConfig> {
        MaybePromise::Value(None)
    }
}

/// `vscode.DebugAdapterDescriptorFactory`. O `executable` é o `DebugAdapterExecutable` do `package.json`.
pub trait DebugAdapterDescriptorFactory {
    fn create_debug_adapter_descriptor(&self, session: Rc<super::ext_host_debug_service::DebugSession>, executable: Option<DebugAdapterExecutable>) -> ProviderResult<DebugAdapterDescriptor>;
}

/// `vscode.DebugAdapterTracker`: os membros opcionais não fazem nada por padrão.
pub trait DebugAdapterTracker {
    fn on_will_start_session(&self) {}
    fn on_will_receive_message(&self, _message: &ProtocolMessage) {}
    fn on_did_send_message(&self, _message: &ProtocolMessage) {}
    fn on_will_stop_session(&self) {}
    fn on_error(&self, _error: &Error) {}
    fn on_exit(&self, _code: Option<i32>, _signal: Option<&str>) {}
}

/// `vscode.DebugAdapterTrackerFactory`.
pub trait DebugAdapterTrackerFactory {
    fn create_debug_adapter_tracker(&self, session: Rc<super::ext_host_debug_service::DebugSession>) -> ProviderResult<Rc<dyn DebugAdapterTracker>>;
}

// ---- visualização

/// `vscode.DebugTreeItem`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DebugTreeItem {
    pub label: String,
    pub description: Option<String>,
    pub collapsible_state: Option<DebugTreeItemCollapsibleState>,
    pub context_value: Option<String>,
    pub can_edit: Option<bool>,
}

/// `vscode.DebugVisualizationContext`, com a sessão da API.
#[derive(Clone)]
pub struct DebugVisualizationContext {
    pub session: Rc<super::ext_host_debug_service::DebugSession>,
    pub variable: Variable,
    pub container_id: Option<i64>,
    pub frame_id: Option<i64>,
    pub thread_id: i64,
}

/// `vscode.DebugVisualizationTree`.
pub trait DebugVisualizationTree {
    /// `getTreeItem(context)`.
    fn get_tree_item(&self, context: DebugVisualizationContext) -> ProviderResult<Rc<DebugTreeItem>>;

    /// `getChildren(element)`.
    fn get_children(&self, element: Rc<DebugTreeItem>) -> ProviderResult<Vec<Rc<DebugTreeItem>>>;

    /// Se o provedor tem `editItem` (o `!!provider.editItem`).
    fn has_edit_item(&self) -> bool {
        false
    }

    /// `editItem(item, value)`.
    fn edit_item(&self, _item: Rc<DebugTreeItem>, _value: &str) -> ProviderResult<Rc<DebugTreeItem>> {
        MaybePromise::Value(None)
    }
}

/// `DebugVisualization.visualization`: `Command | { treeId: string }`.
#[derive(Clone, Debug)]
pub enum DebugVisualizationTarget {
    Command(Command),
    Tree { tree_id: String },
}

/// `DebugVisualization`. É mutável (o `resolveDebugVisualization` devolve o objeto preenchido).
#[derive(Clone)]
pub struct DebugVisualization {
    pub name: String,
    pub icon_path: Option<IconPath>,
    pub visualization: Option<DebugVisualizationTarget>,
}

impl DebugVisualization {
    /// `new DebugVisualization(name)`.
    pub fn new(name: &str) -> Self {
        Self { name: name.to_owned(), icon_path: None, visualization: None }
    }
}

/// `vscode.DebugVisualizationProvider`.
pub trait DebugVisualizationProvider {
    /// `provideDebugVisualization(context, token)`.
    fn provide_debug_visualization(&self, context: DebugVisualizationContext, token: CancellationToken) -> ProviderResult<Vec<DebugVisualization>>;

    /// `resolveDebugVisualization(visualization, token)`: o `?.` do original é a resposta padrão `undefined`.
    fn resolve_debug_visualization(&self, _visualization: DebugVisualization, _token: CancellationToken) -> ProviderResult<DebugVisualization> {
        MaybePromise::Value(None)
    }
}
