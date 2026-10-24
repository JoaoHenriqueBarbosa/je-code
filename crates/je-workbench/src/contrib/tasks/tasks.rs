//! Porte de `src/vs/workbench/contrib/tasks/common/tasks.ts`: o modelo das tarefas (as opções de comando e de
//! apresentação, os grupos, as fontes, as classes `CustomTask`, `ConfiguringTask`, `ContributedTask` e
//! `InMemoryTask`, os eventos e os identificadores).
//!
//! Divergências:
//! - Os objetos de JavaScript viram structs tipadas com campos `Option` (um campo ausente é `undefined`). As
//!   propriedades que o original grava em tempo de execução e a interface não declara (por exemplo o `presentation`
//!   que `PresentationOptions.from` preenche com `undefined!`) são os próprios `Option`. As bolsas de propriedades
//!   abertas (`ITaskIdentifier`, `IJSONSchemaMap`, `env`) são `serde_json::Map` (com `preserve_order`).
//! - `CommonTask` abstrata é o dado [`CommonTaskData`] mais o trait [`TaskLike`], com os métodos virtuais
//!   (`getFolderId`, `getDefinition`, `getKey`...) como métodos do trait. `Task` é o enum
//!   [`Task`], que faz `Deref` para `dyn TaskLike`. O `clone()` do original (que recria a tarefa pelo construtor,
//!   sem `instance` nem mensagens de carga) é `clone_task`; o `Clone` derivado copia tudo. `fromObject` não existe
//!   (só o `clone()` da base o usa, e as três subclasses de `Task` o sobrescrevem). O `clone()` herdado do
//!   `ConfiguringTask` devolve o próprio objeto no original, e aqui é o `Clone` derivado.
//! - `ConfiguringTask` não é um `Task` (como no original): `ConfiguringTask.is` e `getTaskExecution` (que o original
//!   herda de `CommonTask`) não existem para ele, porque o tipo `Task` não o abrange.
//! - `CustomTask`: o parâmetro `type` do construtor é ignorado (o original passa `undefined` à base), e `type` fica
//!   vazio.
//! - `TaskGroup.Build` e companhia são `Rc<TaskGroup>` compartilhados por `thread_local`: a comparação por
//!   identidade do `TaskParser` (`group === Tasks.TaskGroup.Build`) é `Rc::ptr_eq` ([`GroupValue::is_same`]). Mutar a
//!   constante (`taskGroup.isDefault = true`, nos casos ignorados do teste) não é possível.
//! - `TaskSourceKind` é um enum com `as_str`. `FileBasedTaskSource` é uma struct única para as três formas
//!   (`workspace`, `workspaceFile`, `user`), que têm o mesmo formato; `kind` é o enum.
//! - `TerminalExitReason` ainda não existe no porte da plataforma e é definido aqui com os valores do original.
//! - `registerIcon('rerun-task', Codicon.refresh, ...)`: o registro de ícones não foi portado, então
//!   [`rerun_task_icon`] devolve o `ThemeIcon` (o que `registerIcon` devolve) sem registrar o padrão e a descrição.
//! - `ITaskDefinition` está em [`TaskDefinition`] (o nome `TaskDefinition` do namespace, com `createTaskIdentifier`,
//!   é [`create_task_identifier`]). `localeCompare` é `locale_compare` do `je-base`.

use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use indexmap::IndexMap;
use je_base::dynamic::number_to_js_string;
use je_base::nls::{LocalizedString, localize, localize2};
use je_base::parsers::ProblemReporter;
use je_base::resources::ExtUri;
use je_base::themables::ThemeIcon;
use je_base::uri::URI;
use je_platform::configuration::ConfigurationTarget;
use je_platform::contextkey::{ContextKeyExpression, ContextKeyMeta, RawContextKey};
use je_platform::resources::host_environment;
use je_platform::workspace::{IWorkspace, WorkspaceFolder};
use serde_json::{Map, Value};

use super::problem_matcher::ProblemMatcher;
use super::task_definition_registry::task_definition_registry;
use super::task_definition_registry::TaskDefinitionGetter;
use crate::services::extensions::extensions::IExtensionDescription;

/// Uma bolsa de propriedades de JavaScript (`{ [name: string]: unknown }`).
pub type JsonObject = Map<String, Value>;

pub const USER_TASKS_GROUP_KEY: &str = "settings";
pub const CUSTOMIZED_TASK_TYPE: &str = "$customized";
pub const RERUN_FOR_ACTIVE_TERMINAL_COMMAND_ID: &str = "workbench.action.tasks.rerunForActiveTerminal";
pub const RERUN_ALL_RUNNING_TASKS_COMMAND_ID: &str = "workbench.action.tasks.rerunAllRunningTasks";

thread_local! {
    static TASK_RUNNING_STATE: RawContextKey = RawContextKey::new("taskRunning", false, ContextKeyMeta::Description(localize("tasks.taskRunningContext", "Whether a task is currently running.", &[])));
    static TASK_TERMINAL_ACTIVE: RawContextKey = RawContextKey::new("taskTerminalActive", false, ContextKeyMeta::Description(localize("taskTerminalActive", "Whether the active terminal is a task terminal.", &[])));
}

/// `TASK_RUNNING_STATE`.
pub fn task_running_state() -> RawContextKey {
    TASK_RUNNING_STATE.with(Clone::clone)
}

/// `TASK_TERMINAL_ACTIVE`: whether the active terminal is a task terminal.
pub fn task_terminal_active() -> RawContextKey {
    TASK_TERMINAL_ACTIVE.with(Clone::clone)
}

/// `TASKS_CATEGORY`.
pub fn tasks_category() -> LocalizedString {
    localize2("tasksCategory", "Tasks", &[])
}

/// `rerunTaskIcon`.
pub fn rerun_task_icon() -> ThemeIcon {
    ThemeIcon::from_id("rerun-task")
}

fn uri_text(uri: &URI) -> String {
    uri.to_string(false).to_string_lossy()
}

fn json_text(value: &str) -> String {
    serde_json::to_string(value).unwrap_or_default()
}

fn instance_text(instance: Option<i64>) -> String {
    instance.map_or_else(|| "undefined".to_string(), |instance| instance.to_string())
}

// ---------------------------------------------------------------------------------------------
// Opções de comando e de apresentação
// ---------------------------------------------------------------------------------------------

/// `ShellQuoting`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShellQuoting {
    /// Use character escaping.
    Escape = 1,
    /// Use strong quoting.
    Strong = 2,
    /// Use weak quoting.
    Weak = 3,
}

impl ShellQuoting {
    /// `ShellQuoting.from`: vazio ou desconhecido é `Strong`.
    pub fn from(value: Option<&str>) -> ShellQuoting {
        let Some(value) = value.filter(|value| !value.is_empty()) else {
            return ShellQuoting::Strong;
        };
        match value.to_lowercase().as_str() {
            "escape" => ShellQuoting::Escape,
            "strong" => ShellQuoting::Strong,
            "weak" => ShellQuoting::Weak,
            _ => ShellQuoting::Strong,
        }
    }
}

/// `IShellQuotingOptions.escape`: `string | { escapeChar, charsToEscape }`.
#[derive(Clone, Debug, PartialEq)]
pub enum ShellEscape {
    Char(String),
    Chars { escape_char: String, chars_to_escape: String },
}

/// `IShellQuotingOptions`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ShellQuotingOptions {
    /// The character used to do character escaping.
    pub escape: Option<ShellEscape>,
    /// The character used for string quoting.
    pub strong: Option<String>,
    /// The character used for weak quoting.
    pub weak: Option<String>,
}

/// `IShellConfiguration`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ShellConfiguration {
    /// The shell executable.
    pub executable: Option<String>,
    /// The arguments to be passed to the shell executable.
    pub args: Option<Vec<String>>,
    /// Which kind of quotes the shell supports.
    pub quoting: Option<ShellQuotingOptions>,
}

/// `CommandOptions`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CommandOptions {
    /// The shell to use if the task is a shell command.
    pub shell: Option<ShellConfiguration>,
    /// The current working directory of the executed program or shell. If omitted VSCode's current workspace root is used.
    pub cwd: Option<String>,
    /// The environment of the executed program or shell. If omitted the parent process' environment is used.
    pub env: Option<JsonObject>,
}

impl CommandOptions {
    /// `CommandOptions.defaults`.
    pub fn defaults() -> CommandOptions {
        CommandOptions { cwd: Some("${workspaceFolder}".to_string()), ..Default::default() }
    }
}

/// `RevealKind`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RevealKind {
    /// Always brings the terminal to front if the task is executed.
    Always = 1,
    /// Only brings the terminal to front if a problem is detected executing the task.
    Silent = 2,
    /// The terminal never comes to front when the task is executed.
    Never = 3,
}

impl RevealKind {
    pub fn from_string(value: &str) -> RevealKind {
        match value.to_lowercase().as_str() {
            "always" => RevealKind::Always,
            "silent" => RevealKind::Silent,
            "never" => RevealKind::Never,
            _ => RevealKind::Always,
        }
    }
}

/// `RevealProblemKind`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RevealProblemKind {
    /// Never reveals the problems panel when this task is executed.
    Never = 1,
    /// Only reveals the problems panel if a problem is found.
    OnProblem = 2,
    /// Always reveals the problems panel (o comentário do original repete o de `Never`).
    Always = 3,
}

impl RevealProblemKind {
    pub fn from_string(value: &str) -> RevealProblemKind {
        match value.to_lowercase().as_str() {
            "always" => RevealProblemKind::Always,
            "never" => RevealProblemKind::Never,
            "onproblem" => RevealProblemKind::OnProblem,
            _ => RevealProblemKind::OnProblem,
        }
    }
}

/// `PanelKind`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PanelKind {
    /// Shares a panel with other tasks. This is the default.
    Shared = 1,
    /// Uses a dedicated panel for this tasks. The panel is not shared with other tasks.
    Dedicated = 2,
    /// Creates a new panel whenever this task is executed.
    New = 3,
}

impl PanelKind {
    pub fn from_string(value: &str) -> PanelKind {
        match value.to_lowercase().as_str() {
            "shared" => PanelKind::Shared,
            "dedicated" => PanelKind::Dedicated,
            "new" => PanelKind::New,
            _ => PanelKind::Shared,
        }
    }
}

/// `IPresentationOptions`: os campos obrigatórios do original são `Option` porque o `PresentationOptions.from`
/// os preenche com `undefined!` quando a configuração não os dá.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PresentationOptions {
    /// Controls whether the task output is reveal in the user interface. Defaults to `RevealKind.Always`.
    pub reveal: Option<RevealKind>,
    /// Controls whether the problems pane is revealed when running this task or not.
    pub reveal_problems: Option<RevealProblemKind>,
    /// Controls whether the command associated with the task is echoed in the user interface.
    pub echo: Option<bool>,
    /// Controls whether the panel showing the task output is taking focus.
    pub focus: Option<bool>,
    /// Controls if the task panel is used for this task only (dedicated), shared between tasks (shared) or if a new
    /// panel is created on every task execution (new).
    pub panel: Option<PanelKind>,
    /// Controls whether to show the "Terminal will be reused by tasks, press any key to close it" message.
    pub show_reuse_message: Option<bool>,
    /// Controls whether to clear the terminal before executing the task.
    pub clear: Option<bool>,
    /// Controls whether the task is executed in a specific terminal group using split panes.
    pub group: Option<String>,
    /// Controls whether the terminal that the task runs in is closed when the task completes.
    pub close: Option<bool>,
    /// Controls whether to preserve the task name in the terminal after task completion.
    pub preserve_terminal_name: Option<bool>,
}

impl PresentationOptions {
    /// `PresentationOptions.defaults`.
    pub fn defaults() -> PresentationOptions {
        PresentationOptions {
            echo: Some(true),
            reveal: Some(RevealKind::Always),
            reveal_problems: Some(RevealProblemKind::Never),
            focus: Some(false),
            panel: Some(PanelKind::Shared),
            show_reuse_message: Some(true),
            clear: Some(false),
            preserve_terminal_name: Some(false),
            group: None,
            close: None,
        }
    }
}

/// `RuntimeType`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuntimeType {
    Shell = 1,
    Process = 2,
    CustomExecution = 3,
}

impl RuntimeType {
    /// `RuntimeType.fromString`. O `case 'customExecution'` do original compara o valor já em minúsculas e nunca
    /// casa: `customExecution` cai em `Process`.
    pub fn from_string(value: &str) -> RuntimeType {
        match value.to_lowercase().as_str() {
            "shell" => RuntimeType::Shell,
            "process" => RuntimeType::Process,
            _ => RuntimeType::Process,
        }
    }

    /// `RuntimeType.toString`.
    pub fn to_string(value: RuntimeType) -> &'static str {
        match value {
            RuntimeType::Shell => "shell",
            RuntimeType::Process => "process",
            RuntimeType::CustomExecution => "customExecution",
        }
    }
}

/// `IQuotedString`.
#[derive(Clone, Debug, PartialEq)]
pub struct QuotedString {
    pub value: String,
    pub quoting: ShellQuoting,
}

/// `CommandString`.
#[derive(Clone, Debug, PartialEq)]
pub enum CommandString {
    Plain(String),
    Quoted(QuotedString),
}

impl CommandString {
    /// `CommandString.value`.
    pub fn value(&self) -> &str {
        match self {
            CommandString::Plain(value) => value,
            CommandString::Quoted(quoted) => &quoted.value,
        }
    }
}

impl From<&str> for CommandString {
    fn from(value: &str) -> Self {
        CommandString::Plain(value.to_string())
    }
}

/// `ICommandConfiguration`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CommandConfiguration {
    /// The task type.
    pub runtime: Option<RuntimeType>,
    /// The command to execute.
    pub name: Option<CommandString>,
    /// Additional command options.
    pub options: Option<CommandOptions>,
    /// Command arguments.
    pub args: Option<Vec<CommandString>>,
    /// The task selector if needed.
    pub task_selector: Option<String>,
    /// Whether to suppress the task name when merging global args.
    pub suppress_task_name: Option<bool>,
    /// Describes how the task is presented in the UI.
    pub presentation: Option<PresentationOptions>,
}

// ---------------------------------------------------------------------------------------------
// Grupos, escopos e fontes
// ---------------------------------------------------------------------------------------------

/// `TaskGroup.isDefault`: `boolean | string`.
#[derive(Clone, Debug, PartialEq)]
pub enum IsDefault {
    Bool(bool),
    Str(String),
}

/// `TaskGroup`.
#[derive(Clone, Debug, PartialEq)]
pub struct TaskGroup {
    pub id: String,
    pub is_default: Option<IsDefault>,
}

thread_local! {
    static TASK_GROUP_CLEAN: Rc<TaskGroup> = Rc::new(TaskGroup { id: "clean".to_string(), is_default: Some(IsDefault::Bool(false)) });
    static TASK_GROUP_BUILD: Rc<TaskGroup> = Rc::new(TaskGroup { id: "build".to_string(), is_default: Some(IsDefault::Bool(false)) });
    static TASK_GROUP_REBUILD: Rc<TaskGroup> = Rc::new(TaskGroup { id: "rebuild".to_string(), is_default: Some(IsDefault::Bool(false)) });
    static TASK_GROUP_TEST: Rc<TaskGroup> = Rc::new(TaskGroup { id: "test".to_string(), is_default: Some(IsDefault::Bool(false)) });
}

impl TaskGroup {
    /// `TaskGroup.Clean`.
    pub fn clean() -> Rc<TaskGroup> {
        TASK_GROUP_CLEAN.with(Rc::clone)
    }

    /// `TaskGroup.Build`.
    pub fn build() -> Rc<TaskGroup> {
        TASK_GROUP_BUILD.with(Rc::clone)
    }

    /// `TaskGroup.Rebuild`.
    pub fn rebuild() -> Rc<TaskGroup> {
        TASK_GROUP_REBUILD.with(Rc::clone)
    }

    /// `TaskGroup.Test`.
    pub fn test() -> Rc<TaskGroup> {
        TASK_GROUP_TEST.with(Rc::clone)
    }

    /// `TaskGroup.is`.
    pub fn is(value: &str) -> bool {
        value == "clean" || value == "build" || value == "rebuild" || value == "test"
    }

    /// `TaskGroup.from`.
    pub fn from(value: Option<&GroupValue>) -> Option<Rc<TaskGroup>> {
        match value? {
            GroupValue::Name(name) => {
                if TaskGroup::is(name) {
                    return Some(Rc::new(TaskGroup { id: name.clone(), is_default: Some(IsDefault::Bool(false)) }));
                }
                None
            }
            GroupValue::Group(group) => Some(group.clone()),
        }
    }
}

/// `string | TaskGroup`.
#[derive(Clone, Debug)]
pub enum GroupValue {
    Name(String),
    Group(Rc<TaskGroup>),
}

impl GroupValue {
    /// `group === Tasks.TaskGroup.Build` e companhia: a comparação é por identidade do objeto.
    pub fn is_same(&self, group: &Rc<TaskGroup>) -> bool {
        matches!(self, GroupValue::Group(own) if Rc::ptr_eq(own, group))
    }
}

/// `TaskScope`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TaskScope {
    Global = 1,
    Workspace = 2,
    Folder = 3,
}

/// `TaskSourceKind`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TaskSourceKind {
    Workspace,
    Extension,
    InMemory,
    WorkspaceFile,
    User,
}

impl TaskSourceKind {
    pub fn as_str(self) -> &'static str {
        match self {
            TaskSourceKind::Workspace => "workspace",
            TaskSourceKind::Extension => "extension",
            TaskSourceKind::InMemory => "inMemory",
            TaskSourceKind::WorkspaceFile => "workspaceFile",
            TaskSourceKind::User => "user",
        }
    }

    /// `TaskSourceKind.toConfigurationTarget`.
    pub fn to_configuration_target(kind: &str) -> ConfigurationTarget {
        match kind {
            "user" => ConfigurationTarget::User,
            "workspaceFile" => ConfigurationTarget::Workspace,
            _ => ConfigurationTarget::WorkspaceFolder,
        }
    }
}

/// `ITaskSourceConfigElement`.
#[derive(Clone)]
pub struct TaskSourceConfigElement {
    pub workspace_folder: Option<WorkspaceFolder>,
    pub workspace: Option<Rc<dyn IWorkspace>>,
    pub file: String,
    pub index: i64,
    pub element: Option<Value>,
}

/// `ITaskConfig`.
#[derive(Clone, Debug, Default)]
pub struct TaskConfig {
    pub label: String,
    pub task: Option<CommandString>,
    pub r#type: Option<String>,
    pub command: Option<CommandString>,
    pub args: Option<Vec<CommandString>>,
    pub presentation: Option<PresentationOptions>,
    pub is_background: Option<bool>,
    pub problem_matcher: Option<je_base::arrays::OneOrMany<String>>,
    pub group: Option<GroupValue>,
}

/// `IWorkspaceTaskSource`, `IUserTaskSource` e `WorkspaceFileTaskSource` (ver as divergências no topo do módulo).
#[derive(Clone)]
pub struct FileBasedTaskSource {
    /// `Workspace`, `WorkspaceFile` ou `User` (`None` na fonte da tarefa global sintética, que não tem `kind`).
    pub kind: Option<TaskSourceKind>,
    pub label: String,
    pub config: TaskSourceConfigElement,
    pub customizes: Option<KeyedTaskIdentifier>,
}

/// `IExtensionTaskSource`.
#[derive(Clone, Debug)]
pub struct ExtensionTaskSource {
    pub label: String,
    pub extension: Option<String>,
    pub scope: TaskScope,
    pub workspace_folder: Option<WorkspaceFolder>,
}

/// `IExtensionTaskSourceTransfer`.
pub struct ExtensionTaskSourceTransfer {
    pub workspace_folder: je_base::uri::UriComponents,
    pub definition: JsonObject,
}

/// `IInMemoryTaskSource`.
#[derive(Clone, Debug)]
pub struct InMemoryTaskSource {
    pub label: String,
}

/// `TaskSource`.
#[derive(Clone)]
pub enum TaskSource {
    FileBased(FileBasedTaskSource),
    Extension(ExtensionTaskSource),
    InMemory(InMemoryTaskSource),
}

// ---------------------------------------------------------------------------------------------
// Identificadores
// ---------------------------------------------------------------------------------------------

/// `ITaskIdentifier`: `{ type: string; [name: string]: unknown }`.
pub type TaskIdentifier = JsonObject;

/// `KeyedTaskIdentifier`: as propriedades do identificador (incluindo `type`) e a chave `_key`.
#[derive(Clone, Debug, PartialEq)]
pub struct KeyedTaskIdentifier {
    pub key: String,
    pub properties: TaskIdentifier,
}

impl KeyedTaskIdentifier {
    /// `identifier.type`.
    pub fn task_type(&self) -> Option<&str> {
        self.properties.get("type").and_then(Value::as_str)
    }

    /// `identifier[name]`, com `type` e `_key` incluídos.
    pub fn get(&self, name: &str) -> Option<Value> {
        if name == "_key" {
            return Some(Value::from(self.key.as_str()));
        }
        self.properties.get(name).cloned()
    }

    /// `KeyedTaskIdentifier.create`.
    pub fn create(value: &TaskIdentifier) -> KeyedTaskIdentifier {
        let result_key = sorted_stringify(value);
        // `{ _key, type: value.taskType }` e depois `Object.assign(result, value)`: o `type` do valor vence.
        let mut properties = JsonObject::new();
        if let Some(task_type) = value.get("taskType") {
            properties.insert("type".to_string(), task_type.clone());
        }
        for (name, item) in value {
            properties.insert(name.clone(), item.clone());
        }
        let key = match properties.remove("_key") {
            Some(Value::String(key)) => key,
            Some(other) => {
                properties.insert("_key".to_string(), other);
                result_key
            }
            None => result_key,
        };
        KeyedTaskIdentifier { key, properties }
    }
}

fn sort_keys(literal: &JsonObject) -> Vec<&String> {
    let mut keys: Vec<&String> = literal.keys().collect();
    keys.sort_by(|a, b| a.encode_utf16().cmp(b.encode_utf16()));
    keys
}

/// `sortedStringify`.
fn sorted_stringify(literal: &JsonObject) -> String {
    let mut result = String::new();
    for key in sort_keys(literal) {
        let stringified = match &literal[key] {
            Value::Object(object) => sorted_stringify(object),
            Value::Array(items) => {
                let indexed: JsonObject = items.iter().enumerate().map(|(index, item)| (index.to_string(), item.clone())).collect();
                sorted_stringify(&indexed)
            }
            Value::String(text) => text.replace(',', ",,"),
            Value::Number(number) => number_to_js_string(number.as_f64().unwrap_or(f64::NAN)),
            Value::Bool(flag) => flag.to_string(),
            Value::Null => "null".to_string(),
        };
        result.push_str(key);
        result.push(',');
        result.push_str(&stringified);
        result.push(',');
    }
    result
}

/// `ITaskDefinition`.
#[derive(Clone, Debug)]
pub struct TaskDefinition {
    pub extension_id: String,
    pub task_type: String,
    pub required: Vec<String>,
    pub properties: JsonObject,
    pub when: Option<ContextKeyExpression>,
}

/// `TaskDefinition.createTaskIdentifier`. O `reporter` é o relator de problemas inteiro (o original só pede `error`).
pub fn create_task_identifier(external: &TaskIdentifier, reporter: &dyn ProblemReporter) -> Option<KeyedTaskIdentifier> {
    let external_type = external.get("type").and_then(Value::as_str).unwrap_or("undefined");
    let Some(definition) = task_definition_registry().get(external_type) else {
        // We have no task definition so we can't sanitize the literal. Take it as is
        let mut copy = external.clone();
        copy.remove("_key");
        return Some(KeyedTaskIdentifier::create(&copy));
    };

    let mut literal = JsonObject::new();
    literal.insert("type".to_string(), Value::from(definition.task_type.as_str()));
    let required: HashSet<&str> = definition.required.iter().map(String::as_str).collect();

    for (property, schema) in &definition.properties {
        match external.get(property) {
            Some(value) if !value.is_null() => {
                literal.insert(property.clone(), value.clone());
            }
            _ if required.contains(property.as_str()) => {
                if let Some(default) = schema.get("default") {
                    literal.insert(property.clone(), default.clone());
                } else {
                    match schema.get("type").and_then(Value::as_str) {
                        Some("boolean") => {
                            literal.insert(property.clone(), Value::Bool(false));
                        }
                        Some("number") | Some("integer") => {
                            literal.insert(property.clone(), Value::from(0));
                        }
                        Some("string") => {
                            literal.insert(property.clone(), Value::from(""));
                        }
                        _ => {
                            let json = serde_json::to_string(&Value::Object(external.clone())).unwrap_or_default();
                            reporter.error(&localize(
                                "TaskDefinition.missingRequiredProperty",
                                "Error: the task identifier '{0}' is missing the required property '{1}'. The task identifier will be ignored.",
                                &[&json, property],
                            ));
                            return None;
                        }
                    }
                }
            }
            _ => {}
        }
    }
    Some(KeyedTaskIdentifier::create(&literal))
}

/// `ITaskDependency.uri`: `URI | string`.
#[derive(Clone, Debug)]
pub enum UriOrString {
    Uri(URI),
    Str(String),
}

/// `ITaskDependency.task`: `string | KeyedTaskIdentifier | undefined`.
#[derive(Clone, Debug)]
pub enum TaskReference {
    Label(String),
    Identifier(KeyedTaskIdentifier),
}

/// `ITaskDependency`.
#[derive(Clone, Debug)]
pub struct TaskDependency {
    pub uri: UriOrString,
    pub task: Option<TaskReference>,
}

/// `DependsOrder`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DependsOrder {
    Parallel,
    Sequence,
}

impl DependsOrder {
    pub fn as_str(self) -> &'static str {
        match self {
            DependsOrder::Parallel => "parallel",
            DependsOrder::Sequence => "sequence",
        }
    }
}

/// `problemMatchers?: Array<string | ProblemMatcher>`.
#[derive(Clone, Debug)]
pub enum ProblemMatcherValue {
    Name(String),
    Matcher(ProblemMatcher),
}

/// `icon?: { id?: string; color?: string }`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TaskIcon {
    pub id: Option<String>,
    pub color: Option<String>,
}

/// `IConfigurationProperties`. `extra` guarda as propriedades que `ConfigurationProperties.from` copia do
/// `properties` da definição do tipo (o original as grava no mesmo objeto, sem declará-las).
#[derive(Clone, Debug, Default)]
pub struct ConfigurationProperties {
    /// The task's name.
    pub name: Option<String>,
    /// The task's identifier.
    pub identifier: Option<String>,
    /// The task's group.
    pub group: Option<GroupValue>,
    /// The presentation options.
    pub presentation: Option<PresentationOptions>,
    /// The command options.
    pub options: Option<CommandOptions>,
    /// Whether the task is a background task or not.
    pub is_background: Option<bool>,
    /// Whether the task should prompt on close for confirmation if running.
    pub prompt_on_close: Option<bool>,
    /// The other tasks this task depends on.
    pub depends_on: Option<Vec<TaskDependency>>,
    /// The order the dependsOn tasks should be executed in.
    pub depends_order: Option<DependsOrder>,
    /// A description of the task.
    pub detail: Option<String>,
    /// The problem watchers to use for this task.
    pub problem_matchers: Option<Vec<ProblemMatcherValue>>,
    /// The icon for this task in the terminal tabs list.
    pub icon: Option<TaskIcon>,
    /// Do not show this task in the run task quickpick.
    pub hide: Option<bool>,
    /// Show this task in the Agents run action dropdown.
    pub in_agents: Option<bool>,
    pub extra: JsonObject,
}

/// `RunOnOptions`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RunOnOptions {
    Default = 1,
    FolderOpen = 2,
    WorktreeCreated = 3,
}

/// `InstancePolicy`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InstancePolicy {
    TerminateNewest,
    TerminateOldest,
    Prompt,
    Warn,
    Silent,
}

impl InstancePolicy {
    pub fn as_str(self) -> &'static str {
        match self {
            InstancePolicy::TerminateNewest => "terminateNewest",
            InstancePolicy::TerminateOldest => "terminateOldest",
            InstancePolicy::Prompt => "prompt",
            InstancePolicy::Warn => "warn",
            InstancePolicy::Silent => "silent",
        }
    }
}

/// `IRunOptions`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RunOptions {
    pub reevaluate_on_rerun: Option<bool>,
    pub run_on: Option<RunOnOptions>,
    pub instance_limit: Option<f64>,
    pub instance_policy: Option<InstancePolicy>,
}

impl RunOptions {
    /// `RunOptions.defaults`.
    pub fn defaults() -> RunOptions {
        RunOptions {
            reevaluate_on_rerun: Some(true),
            run_on: Some(RunOnOptions::Default),
            instance_limit: Some(1.0),
            instance_policy: Some(InstancePolicy::Prompt),
        }
    }
}

// ---------------------------------------------------------------------------------------------
// As tarefas
// ---------------------------------------------------------------------------------------------

/// O dado de `CommonTask`.
#[derive(Clone)]
pub struct CommonTaskData {
    /// The task's internal id.
    pub id: String,
    /// The cached label.
    pub label: String,
    pub r#type: Option<String>,
    pub run_options: RunOptions,
    pub configuration_properties: ConfigurationProperties,
    task_load_messages: Option<Vec<String>>,
}

impl CommonTaskData {
    pub fn new(id: &str, label: Option<&str>, task_type: Option<&str>, run_options: RunOptions, configuration_properties: ConfigurationProperties) -> Self {
        Self {
            id: id.to_string(),
            label: label.filter(|label| !label.is_empty()).unwrap_or_default().to_string(),
            r#type: task_type.filter(|task_type| !task_type.is_empty()).map(str::to_string),
            run_options,
            configuration_properties,
            task_load_messages: None,
        }
    }
}

/// O argumento `key` de `CommonTask.matches`: `string | KeyedTaskIdentifier`.
#[derive(Clone, Copy)]
pub enum TaskKey<'a> {
    Label(&'a str),
    Identifier(&'a KeyedTaskIdentifier),
}

/// `ITaskExecution`.
#[derive(Clone)]
pub struct TaskExecution {
    pub id: String,
    pub task: Task,
}

/// `CommonTask`: os métodos que as quatro classes compartilham, com os virtuais a implementar.
pub trait TaskLike {
    fn common(&self) -> &CommonTaskData;

    fn common_mut(&mut self) -> &mut CommonTaskData;

    fn get_folder_id(&self) -> Option<String>;

    fn get_definition(&self, _use_source: bool) -> Option<KeyedTaskIdentifier> {
        None
    }

    fn get_map_key(&self) -> String {
        self.common().id.clone()
    }

    fn get_key(&self) -> Option<String> {
        None
    }

    /// `CommonTask.getCommonTaskId` (a implementação da base, que `CustomTask` chama por `super`).
    fn base_common_task_id(&self) -> String {
        let mut key = String::from("{");
        if let Some(folder) = self.get_folder_id() {
            key.push_str(&format!("\"folder\":{},", json_text(&folder)));
        }
        key.push_str(&format!("\"id\":{}}}", json_text(&self.common().id)));
        key
    }

    fn get_common_task_id(&self) -> String {
        self.base_common_task_id()
    }

    fn get_workspace_folder(&self) -> Option<WorkspaceFolder> {
        None
    }

    fn get_workspace_file_name(&self) -> Option<String> {
        None
    }

    fn get_telemetry_kind(&self) -> String {
        "unknown".to_string()
    }

    fn matches(&self, key: Option<TaskKey<'_>>, compare_id: bool) -> bool {
        match key {
            None => false,
            Some(TaskKey::Label(label)) => {
                let common = self.common();
                label == common.label || common.configuration_properties.identifier.as_deref() == Some(label) || (compare_id && label == common.id)
            }
            Some(TaskKey::Identifier(key)) => self.get_definition(true).is_some_and(|identifier| identifier.key == key.key),
        }
    }

    fn get_qualified_label(&self) -> String {
        match self.get_workspace_folder() {
            Some(folder) => format!("{} ({})", self.common().label, folder.name),
            None => self.common().label.clone(),
        }
    }

    fn add_task_load_messages(&mut self, messages: Option<&[String]>) {
        let common = self.common_mut();
        let current = common.task_load_messages.get_or_insert_with(Vec::new);
        if let Some(messages) = messages {
            current.extend(messages.iter().cloned());
        }
    }

    fn task_load_messages(&self) -> Option<&[String]> {
        self.common().task_load_messages.as_deref()
    }
}

fn file_based_folder_id(source: &FileBasedTaskSource) -> Option<String> {
    if source.kind == Some(TaskSourceKind::User) {
        Some(USER_TASKS_GROUP_KEY.to_string())
    } else {
        source.config.workspace_folder.as_ref().map(|folder| uri_text(&folder.uri))
    }
}

fn file_based_workspace_file_name(source: &FileBasedTaskSource) -> Option<String> {
    let configuration = source.config.workspace.as_ref()?.configuration()?;
    Some(ExtUri::unbiased(host_environment()).basename(&configuration, None))
}

fn file_based_key(source: &FileBasedTaskSource, properties: &ConfigurationProperties, folder_id: Option<String>) -> Option<String> {
    let folder = folder_id?;
    let mut id = properties.identifier.clone();
    if source.kind != Some(TaskSourceKind::Workspace) {
        let kind = source.kind.map_or("undefined", TaskSourceKind::as_str);
        id = Some(format!("{}{}", id.as_deref().unwrap_or("undefined"), kind));
    }
    let mut key = format!("{{\"type\":{},\"folder\":{}", json_text(CUSTOMIZED_TASK_TYPE), json_text(&folder));
    if let Some(id) = id {
        key.push_str(&format!(",\"id\":{}", json_text(&id)));
    }
    key.push('}');
    Some(key)
}

/// `CustomTask`: criada na leitura do `tasks.json` ou do arquivo de workspace (para os tipos `shell` e `process`),
/// ou, para as outras, o resultado de resolver uma `ConfiguringTask`.
#[derive(Clone)]
pub struct CustomTask {
    pub common: CommonTaskData,
    pub instance: Option<i64>,
    /// Indicated the source of the task (e.g. tasks.json or extension).
    pub source: FileBasedTaskSource,
    pub has_defined_matchers: bool,
    /// The command configuration.
    pub command: CommandConfiguration,
}

impl CustomTask {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        id: &str,
        source: FileBasedTaskSource,
        label: &str,
        _type: &str,
        command: Option<CommandConfiguration>,
        has_defined_matchers: bool,
        run_options: RunOptions,
        configuration_properties: ConfigurationProperties,
    ) -> Self {
        Self {
            common: CommonTaskData::new(id, Some(label), None, run_options, configuration_properties),
            instance: None,
            source,
            has_defined_matchers,
            command: command.unwrap_or_default(),
        }
    }

    /// `clone()`.
    pub fn clone_task(&self) -> CustomTask {
        CustomTask::new(
            &self.common.id,
            self.source.clone(),
            &self.common.label,
            CUSTOMIZED_TASK_TYPE,
            Some(self.command.clone()),
            self.has_defined_matchers,
            self.common.run_options.clone(),
            self.common.configuration_properties.clone(),
        )
    }

    pub fn customizes(&self) -> Option<KeyedTaskIdentifier> {
        self.source.customizes.clone()
    }

    /// `CustomTask.is`.
    pub fn is(value: &Task) -> bool {
        matches!(value, Task::Custom(_))
    }
}

impl TaskLike for CustomTask {
    fn common(&self) -> &CommonTaskData {
        &self.common
    }

    fn common_mut(&mut self) -> &mut CommonTaskData {
        &mut self.common
    }

    fn get_definition(&self, use_source: bool) -> Option<KeyedTaskIdentifier> {
        if use_source && let Some(customizes) = &self.source.customizes {
            return Some(customizes.clone());
        }
        let task_type = match self.command.runtime {
            Some(RuntimeType::Shell) => "shell",
            Some(RuntimeType::Process) => "process",
            Some(RuntimeType::CustomExecution) => "customExecution",
            None => "$composite",
        };
        let mut properties = JsonObject::new();
        properties.insert("type".to_string(), Value::from(task_type));
        properties.insert("id".to_string(), Value::from(self.common.id.as_str()));
        Some(KeyedTaskIdentifier { key: self.common.id.clone(), properties })
    }

    fn get_map_key(&self) -> String {
        let instance = instance_text(self.instance);
        match &self.source.config.workspace_folder {
            Some(folder) => format!("{}|{}|{}", uri_text(&folder.uri), self.common.id, instance),
            None => format!("{}|{}", self.common.id, instance),
        }
    }

    fn get_folder_id(&self) -> Option<String> {
        file_based_folder_id(&self.source)
    }

    fn get_common_task_id(&self) -> String {
        if self.source.customizes.is_some() {
            self.base_common_task_id()
        } else {
            self.get_key().unwrap_or_else(|| self.base_common_task_id())
        }
    }

    /// A chave que representa a tarefa.
    fn get_key(&self) -> Option<String> {
        file_based_key(&self.source, &self.common.configuration_properties, self.get_folder_id())
    }

    fn get_workspace_folder(&self) -> Option<WorkspaceFolder> {
        self.source.config.workspace_folder.clone()
    }

    fn get_workspace_file_name(&self) -> Option<String> {
        file_based_workspace_file_name(&self.source)
    }

    fn get_telemetry_kind(&self) -> String {
        if self.source.customizes.is_some() { "workspace>extension".to_string() } else { "workspace".to_string() }
    }
}

/// `ConfiguringTask`: as propriedades de uma tarefa contribuída depois de lidas e antes de resolvidas pela extensão.
#[derive(Clone)]
pub struct ConfiguringTask {
    pub common: CommonTaskData,
    /// Indicated the source of the task (e.g. tasks.json or extension).
    pub source: FileBasedTaskSource,
    pub configures: KeyedTaskIdentifier,
}

impl ConfiguringTask {
    pub fn new(
        id: &str,
        source: FileBasedTaskSource,
        label: Option<&str>,
        task_type: Option<&str>,
        configures: KeyedTaskIdentifier,
        run_options: RunOptions,
        configuration_properties: ConfigurationProperties,
    ) -> Self {
        Self { common: CommonTaskData::new(id, label, task_type, run_options, configuration_properties), source, configures }
    }
}

impl TaskLike for ConfiguringTask {
    fn common(&self) -> &CommonTaskData {
        &self.common
    }

    fn common_mut(&mut self) -> &mut CommonTaskData {
        &mut self.common
    }

    fn get_definition(&self, _use_source: bool) -> Option<KeyedTaskIdentifier> {
        Some(self.configures.clone())
    }

    fn get_workspace_file_name(&self) -> Option<String> {
        file_based_workspace_file_name(&self.source)
    }

    fn get_workspace_folder(&self) -> Option<WorkspaceFolder> {
        self.source.config.workspace_folder.clone()
    }

    fn get_folder_id(&self) -> Option<String> {
        file_based_folder_id(&self.source)
    }

    fn get_key(&self) -> Option<String> {
        file_based_key(&self.source, &self.common.configuration_properties, self.get_folder_id())
    }
}

/// `ContributedTask`: uma tarefa de extensão criada por `resolveTask` ou `provideTask`.
#[derive(Clone)]
pub struct ContributedTask {
    pub common: CommonTaskData,
    /// Indicated the source of the task (e.g. tasks.json or extension).
    pub source: ExtensionTaskSource,
    pub instance: Option<i64>,
    pub defines: KeyedTaskIdentifier,
    pub has_defined_matchers: bool,
    /// The command configuration.
    pub command: CommandConfiguration,
    /// The icon for the task.
    pub icon: Option<TaskIcon>,
    /// Don't show the task in the run task quickpick.
    pub hide: Option<bool>,
}

impl ContributedTask {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        id: &str,
        source: ExtensionTaskSource,
        label: &str,
        task_type: Option<&str>,
        defines: KeyedTaskIdentifier,
        command: CommandConfiguration,
        has_defined_matchers: bool,
        run_options: RunOptions,
        configuration_properties: ConfigurationProperties,
    ) -> Self {
        Self {
            icon: configuration_properties.icon.clone(),
            hide: configuration_properties.hide,
            common: CommonTaskData::new(id, Some(label), task_type, run_options, configuration_properties),
            source,
            instance: None,
            defines,
            has_defined_matchers,
            command,
        }
    }

    /// `clone()`.
    pub fn clone_task(&self) -> ContributedTask {
        ContributedTask::new(
            &self.common.id,
            self.source.clone(),
            &self.common.label,
            self.common.r#type.as_deref(),
            self.defines.clone(),
            self.command.clone(),
            self.has_defined_matchers,
            self.common.run_options.clone(),
            self.common.configuration_properties.clone(),
        )
    }

    /// `ContributedTask.is`.
    pub fn is(value: &Task) -> bool {
        matches!(value, Task::Contributed(_))
    }
}

impl TaskLike for ContributedTask {
    fn common(&self) -> &CommonTaskData {
        &self.common
    }

    fn common_mut(&mut self) -> &mut CommonTaskData {
        &mut self.common
    }

    fn get_definition(&self, _use_source: bool) -> Option<KeyedTaskIdentifier> {
        Some(self.defines.clone())
    }

    fn get_map_key(&self) -> String {
        let scope = (self.source.scope as i32).to_string();
        let instance = instance_text(self.instance);
        match &self.source.workspace_folder {
            Some(folder) => format!("{}|{}|{}|{}", scope, uri_text(&folder.uri), self.common.id, instance),
            None => format!("{}|{}|{}", scope, self.common.id, instance),
        }
    }

    fn get_folder_id(&self) -> Option<String> {
        if self.source.scope == TaskScope::Folder
            && let Some(folder) = &self.source.workspace_folder
        {
            return Some(uri_text(&folder.uri));
        }
        None
    }

    fn get_key(&self) -> Option<String> {
        let mut key = format!("{{\"type\":\"contributed\",\"scope\":{},\"id\":{}", self.source.scope as i32, json_text(&self.common.id));
        if let Some(folder) = self.get_folder_id() {
            key.push_str(&format!(",\"folder\":{}", json_text(&folder)));
        }
        key.push('}');
        Some(key)
    }

    fn get_workspace_folder(&self) -> Option<WorkspaceFolder> {
        self.source.workspace_folder.clone()
    }

    fn get_telemetry_kind(&self) -> String {
        "extension".to_string()
    }
}

/// `InMemoryTask`.
#[derive(Clone)]
pub struct InMemoryTask {
    pub common: CommonTaskData,
    /// Indicated the source of the task (e.g. tasks.json or extension).
    pub source: InMemoryTaskSource,
    pub instance: Option<i64>,
}

impl InMemoryTask {
    pub fn new(id: &str, source: InMemoryTaskSource, label: &str, task_type: &str, run_options: RunOptions, configuration_properties: ConfigurationProperties) -> Self {
        Self { common: CommonTaskData::new(id, Some(label), Some(task_type), run_options, configuration_properties), source, instance: None }
    }

    /// `clone()`.
    pub fn clone_task(&self) -> InMemoryTask {
        InMemoryTask::new(
            &self.common.id,
            self.source.clone(),
            &self.common.label,
            self.common.r#type.as_deref().unwrap_or_default(),
            self.common.run_options.clone(),
            self.common.configuration_properties.clone(),
        )
    }

    /// `InMemoryTask.is`.
    pub fn is(value: &Task) -> bool {
        matches!(value, Task::InMemory(_))
    }
}

impl TaskLike for InMemoryTask {
    fn common(&self) -> &CommonTaskData {
        &self.common
    }

    fn common_mut(&mut self) -> &mut CommonTaskData {
        &mut self.common
    }

    fn get_telemetry_kind(&self) -> String {
        "composite".to_string()
    }

    fn get_map_key(&self) -> String {
        format!("{}|{}", self.common.id, instance_text(self.instance))
    }

    fn get_folder_id(&self) -> Option<String> {
        None
    }
}

/// `Task`: `CustomTask | ContributedTask | InMemoryTask`.
#[derive(Clone)]
pub enum Task {
    Custom(CustomTask),
    Contributed(ContributedTask),
    InMemory(InMemoryTask),
}

impl std::ops::Deref for Task {
    type Target = dyn TaskLike;

    fn deref(&self) -> &Self::Target {
        match self {
            Task::Custom(task) => task,
            Task::Contributed(task) => task,
            Task::InMemory(task) => task,
        }
    }
}

impl std::ops::DerefMut for Task {
    fn deref_mut(&mut self) -> &mut Self::Target {
        match self {
            Task::Custom(task) => task,
            Task::Contributed(task) => task,
            Task::InMemory(task) => task,
        }
    }
}

impl Task {
    /// `clone()`.
    pub fn clone_task(&self) -> Task {
        match self {
            Task::Custom(task) => Task::Custom(task.clone_task()),
            Task::Contributed(task) => Task::Contributed(task.clone_task()),
            Task::InMemory(task) => Task::InMemory(task.clone_task()),
        }
    }

    /// `getTaskExecution`.
    pub fn get_task_execution(&self) -> TaskExecution {
        TaskExecution { id: self.common().id.clone(), task: self.clone() }
    }
}

/// `ExecutionEngine`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExecutionEngine {
    Process = 1,
    Terminal = 2,
}

impl ExecutionEngine {
    /// `ExecutionEngine._default`.
    pub const DEFAULT: ExecutionEngine = ExecutionEngine::Terminal;
}

/// `JsonSchemaVersion`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JsonSchemaVersion {
    V0_1_0 = 1,
    V2_0_0 = 2,
}

/// `ITaskSet`.
#[derive(Clone)]
pub struct TaskSet {
    pub tasks: Vec<Task>,
    pub extension: Option<IExtensionDescription>,
}

/// `TaskSorter`.
pub struct TaskSorter {
    order: HashMap<String, usize>,
}

impl TaskSorter {
    pub fn new(workspace_folders: &[WorkspaceFolder]) -> Self {
        let mut order = HashMap::new();
        for (index, folder) in workspace_folders.iter().enumerate() {
            order.insert(uri_text(&folder.uri), index);
        }
        Self { order }
    }

    pub fn compare(&self, a: &dyn TaskLike, b: &dyn TaskLike) -> i64 {
        let aw = a.get_workspace_folder();
        let bw = b.get_workspace_folder();
        match (aw, bw) {
            (Some(aw), Some(bw)) => {
                let ai = self.order.get(&uri_text(&aw.uri)).map_or(0, |index| *index as i64 + 1);
                let bi = self.order.get(&uri_text(&bw.uri)).map_or(0, |index| *index as i64 + 1);
                if ai == bi {
                    match je_base::collator::locale_compare(&a.common().label, &b.common().label) {
                        std::cmp::Ordering::Less => -1,
                        std::cmp::Ordering::Equal => 0,
                        std::cmp::Ordering::Greater => 1,
                    }
                } else {
                    ai - bi
                }
            }
            (None, Some(_)) => -1,
            (Some(_), None) => 1,
            (None, None) => 0,
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Eventos
// ---------------------------------------------------------------------------------------------

/// `TaskRunType`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TaskRunType {
    SingleRun,
    Background,
}

/// `TaskEventKind`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TaskEventKind {
    /// Indicates that a task's properties or configuration have changed.
    Changed,
    /// Indicates that a task has begun executing.
    ProcessStarted,
    /// Indicates that a task process has completed.
    ProcessEnded,
    /// Indicates that a task was terminated, either by user action or by the system.
    Terminated,
    /// Indicates that a task has started running.
    Start,
    /// Indicates that a task has acquired all needed input/variables to execute.
    AcquiredInput,
    /// Indicates that a dependent task has started.
    DependsOnStarted,
    /// Indicates that a task is actively running/processing.
    Active,
    /// Indicates that a task is paused/waiting but not complete.
    Inactive,
    /// Indicates that a task has completed fully.
    End,
    /// Indicates that a task's problem matcher has started.
    ProblemMatcherStarted,
    /// Indicates that a task's problem matcher has ended.
    ProblemMatcherEnded,
    /// Indicates that a task's problem matcher has found errors.
    ProblemMatcherFoundErrors,
}

impl TaskEventKind {
    pub fn as_str(self) -> &'static str {
        match self {
            TaskEventKind::Changed => "changed",
            TaskEventKind::ProcessStarted => "processStarted",
            TaskEventKind::ProcessEnded => "processEnded",
            TaskEventKind::Terminated => "terminated",
            TaskEventKind::Start => "start",
            TaskEventKind::AcquiredInput => "acquiredInput",
            TaskEventKind::DependsOnStarted => "dependsOnStarted",
            TaskEventKind::Active => "active",
            TaskEventKind::Inactive => "inactive",
            TaskEventKind::End => "end",
            TaskEventKind::ProblemMatcherStarted => "problemMatcherStarted",
            TaskEventKind::ProblemMatcherEnded => "problemMatcherEnded",
            TaskEventKind::ProblemMatcherFoundErrors => "problemMatcherFoundErrors",
        }
    }
}

/// `TerminalExitReason` (ver as divergências no topo do módulo).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TerminalExitReason {
    Unknown = 0,
    Shutdown = 1,
    Process = 2,
    User = 3,
    Extension = 4,
}

/// `TaskRunSource`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TaskRunSource {
    System,
    User,
    FolderOpen,
    ConfigurationChange,
    Reconnect,
    ChatAgent,
}

/// `ITaskCommon`.
#[derive(Clone)]
pub struct TaskCommon {
    pub task_id: String,
    pub run_type: TaskRunType,
    pub task_name: Option<String>,
    pub group: Option<GroupValue>,
    /// `__task`.
    pub task: Task,
}

/// `ITaskEvent`.
#[derive(Clone)]
pub enum TaskEvent {
    Changed,
    ProcessStarted { common: TaskCommon, terminal_id: i64, process_id: i64 },
    ProcessEnded { common: TaskCommon, terminal_id: Option<i64>, exit_code: Option<i64>, duration_ms: Option<f64> },
    Terminated { common: TaskCommon, terminal_id: i64, exit_reason: Option<TerminalExitReason> },
    Start { common: TaskCommon, terminal_id: i64, resolved_variables: IndexMap<String, String> },
    /// `ITaskGeneralEvent` (`AcquiredInput`, `DependsOnStarted`, `Active`, `Inactive`, `End`, `ProblemMatcherStarted`,
    /// `ProblemMatcherFoundErrors`) e `ITaskInactiveEvent` (`Inactive` com `duration_ms`).
    General { common: TaskCommon, kind: TaskEventKind, terminal_id: Option<i64>, duration_ms: Option<f64> },
    ProblemMatcherEnded { common: TaskCommon, has_errors: bool },
}

fn event_common(task: &Task) -> TaskCommon {
    let properties = &task.common().configuration_properties;
    TaskCommon {
        task_id: task.common().id.clone(),
        task_name: properties.name.clone(),
        run_type: if properties.is_background == Some(true) { TaskRunType::Background } else { TaskRunType::SingleRun },
        group: properties.group.clone(),
        task: task.clone(),
    }
}

impl TaskEvent {
    pub fn start(task: &Task, terminal_id: i64, resolved_variables: IndexMap<String, String>) -> TaskEvent {
        TaskEvent::Start { common: event_common(task), terminal_id, resolved_variables }
    }

    pub fn process_started(task: &Task, terminal_id: i64, process_id: i64) -> TaskEvent {
        TaskEvent::ProcessStarted { common: event_common(task), terminal_id, process_id }
    }

    pub fn process_ended(task: &Task, terminal_id: Option<i64>, exit_code: Option<i64>, duration_ms: Option<f64>) -> TaskEvent {
        TaskEvent::ProcessEnded { common: event_common(task), terminal_id, exit_code, duration_ms }
    }

    pub fn inactive(task: &Task, terminal_id: Option<i64>, duration_ms: Option<f64>) -> TaskEvent {
        TaskEvent::General { common: event_common(task), kind: TaskEventKind::Inactive, terminal_id, duration_ms }
    }

    pub fn terminated(task: &Task, terminal_id: i64, exit_reason: Option<TerminalExitReason>) -> TaskEvent {
        TaskEvent::Terminated { common: event_common(task), terminal_id, exit_reason }
    }

    /// `general`: o `kind` é um dos tipos de `ITaskGeneralEvent`.
    pub fn general(kind: TaskEventKind, task: &Task, terminal_id: Option<i64>) -> TaskEvent {
        TaskEvent::General { common: event_common(task), kind, terminal_id, duration_ms: None }
    }

    pub fn problem_matcher_ended(task: &Task, has_errors: bool) -> TaskEvent {
        TaskEvent::ProblemMatcherEnded { common: event_common(task), has_errors }
    }

    pub fn changed() -> TaskEvent {
        TaskEvent::Changed
    }

    /// `event.kind`.
    pub fn kind(&self) -> TaskEventKind {
        match self {
            TaskEvent::Changed => TaskEventKind::Changed,
            TaskEvent::ProcessStarted { .. } => TaskEventKind::ProcessStarted,
            TaskEvent::ProcessEnded { .. } => TaskEventKind::ProcessEnded,
            TaskEvent::Terminated { .. } => TaskEventKind::Terminated,
            TaskEvent::Start { .. } => TaskEventKind::Start,
            TaskEvent::General { kind, .. } => *kind,
            TaskEvent::ProblemMatcherEnded { .. } => TaskEventKind::ProblemMatcherEnded,
        }
    }
}

/// `TaskSettingId`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TaskSettingId {
    AutoDetect,
    SaveBeforeRun,
    ShowDecorations,
    ProblemMatchersNeverPrompt,
    SlowProviderWarning,
    QuickOpenHistory,
    QuickOpenDetail,
    QuickOpenSkip,
    QuickOpenShowAll,
    AllowAutomaticTasks,
    Reconnection,
    VerboseLogging,
    NotifyWindowOnTaskCompletion,
}

impl TaskSettingId {
    pub fn as_str(self) -> &'static str {
        match self {
            TaskSettingId::AutoDetect => "task.autoDetect",
            TaskSettingId::SaveBeforeRun => "task.saveBeforeRun",
            TaskSettingId::ShowDecorations => "task.showDecorations",
            TaskSettingId::ProblemMatchersNeverPrompt => "task.problemMatchers.neverPrompt",
            TaskSettingId::SlowProviderWarning => "task.slowProviderWarning",
            TaskSettingId::QuickOpenHistory => "task.quickOpen.history",
            TaskSettingId::QuickOpenDetail => "task.quickOpen.detail",
            TaskSettingId::QuickOpenSkip => "task.quickOpen.skip",
            TaskSettingId::QuickOpenShowAll => "task.quickOpen.showAll",
            TaskSettingId::AllowAutomaticTasks => "task.allowAutomaticTasks",
            TaskSettingId::Reconnection => "task.reconnection",
            TaskSettingId::VerboseLogging => "task.verboseLogging",
            TaskSettingId::NotifyWindowOnTaskCompletion => "task.notifyWindowOnTaskCompletion",
        }
    }
}

/// `TasksSchemaProperties`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TasksSchemaProperties {
    Tasks,
    SuppressTaskName,
    Windows,
    Osx,
    Linux,
    ShowOutput,
    IsShellCommand,
    ServiceTestSetting,
}

impl TasksSchemaProperties {
    pub fn as_str(self) -> &'static str {
        match self {
            TasksSchemaProperties::Tasks => "tasks",
            TasksSchemaProperties::SuppressTaskName => "tasks.suppressTaskName",
            TasksSchemaProperties::Windows => "tasks.windows",
            TasksSchemaProperties::Osx => "tasks.osx",
            TasksSchemaProperties::Linux => "tasks.linux",
            TasksSchemaProperties::ShowOutput => "tasks.showOutput",
            TasksSchemaProperties::IsShellCommand => "tasks.isShellCommand",
            TasksSchemaProperties::ServiceTestSetting => "tasks.service.testSetting",
        }
    }
}
