//! Porte de `src/vs/workbench/contrib/tasks/common/taskConfiguration.ts`: a leitura da configuração das tarefas
//! (`tasks.json`, o arquivo de workspace e as tarefas de usuário) para o modelo de `tasks.rs`.
//!
//! Divergências:
//! - A configuração externa (`IExternalTaskRunnerConfiguration`, `ICustomTask`, `IConfiguringTask`, as opções de
//!   comando e de apresentação, `dependsOn`...) é o `serde_json::Value` lido do arquivo, e os testes de
//!   `typeof`/`Types.is*` do original são as conferências sobre o `Value` (a verdade do JavaScript é [`truthy`]). Os
//!   campos de tipo errado são descartados na conversão para o modelo tipado (o original os repassa como estão).
//!   As interfaces de configuração que só descrevem o formato (`ICommandOptionsConfig`, `IRunOptionsConfig`...) não
//!   têm tipo próprio.
//! - `ProblemMatcherConfig.ProblemMatcherType` chega como `Value` e vira `problem_matcher::config::ProblemMatcher`
//!   em [`problem_matcher_from_json`] (o porte do casador não lê JSON).
//! - Os helpers genéricos do original (`_isEmpty`, `_assignProperties`, `_fillProperties`, `_fillDefaults`,
//!   `_freeze`, guiados por `IMetaData`) são as funções de cada tipo, escritas com macros de campos. Uma conferência
//!   do original merece nota: num `IMetaData` com `type`, qualquer propriedade não nula torna o contêiner não vazio
//!   (o ramo `else if` só barra arrays vazios); aqui `is_empty` é "todos os campos ausentes, ou arrays vazios".
//! - `Object.freeze` (`_freeze`, `Object.isFrozen`) não existe: os valores globais são clonados ao serem mesclados
//!   nas tarefas, e o preenchimento de padrões é idempotente, então pular a conferência de `isFrozen` não muda
//!   resultado. `ShellConfiguration.fillDefaults/freeze`, `CommandConfiguration.fillProperties/freeze`,
//!   `CommandOptions.freeze` e `PresentationOptions.freeze` (sem uso fora do congelamento) não existem.
//! - `IParseResult.validationStatus` é uma cópia do estado do relator no fim da leitura.
//! - `IParseContext.contextKeyService` e `workspace` são `Rc`. `uuidMap` é `Rc<RefCell<UuidMap>>` (o original o
//!   compartilha por referência).
//! - O objeto de fonte da tarefa global sintética (`Object.assign({}, source, 'workspace', { config })`) não tem
//!   `kind` nem `label` no original (o `Object.assign` espalha os caracteres da string): `kind` é `None`.
//! - `ShellExecutionSupportedContext` e `ProcessExecutionSupportedContext` são de `taskService.ts` (ainda não
//!   portado) e estão aqui, [`shell_execution_supported_context`] e [`process_execution_supported_context`].
//! - Quando `external.command` existe mas `CommandConfiguration.from` devolve `undefined`, o original lança
//!   `TypeError`; aqui também (`errors::throw`).
//! - `ConfigurationProperties.from` com `properties` copia as propriedades da definição para `extra`.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use indexmap::IndexMap;
use je_base::arrays::OneOrMany;
use je_base::errors::{self, Error};
use je_base::nls::{NlsArg, localize, localize_args};
use je_base::parsers::{ProblemReporter, ValidationStatus};
use je_base::platform::PlatformKind;
use je_base::uuid::generate_uuid;
use je_platform::contextkey::{ContextKeyMeta, IContextKeyService, RawContextKey};
use je_platform::workspace::{IWorkspace, WorkspaceFolder};
use serde_json::{Map, Value};

use super::problem_matcher::{ProblemMatcher, ProblemMatcherParser, config as matcher_config, is_named_problem_matcher, problem_matcher_registry};
use super::problem_matcher::IProblemMatcherRegistry;
use super::task_definition_registry::{TaskDefinitionGetter, task_definition_registry};
use super::tasks::{
    self as tasks, CUSTOMIZED_TASK_TYPE, CommandConfiguration, CommandOptions, CommandString, ConfigurationProperties, ConfiguringTask, CustomTask,
    DependsOrder, ExecutionEngine, FileBasedTaskSource, GroupValue, InstancePolicy, IsDefault, JsonObject, JsonSchemaVersion, PanelKind, PresentationOptions, ProblemMatcherValue, QuotedString, RevealKind, RevealProblemKind, RunOnOptions, RunOptions, RuntimeType,
    ShellConfiguration, ShellEscape, ShellQuoting, ShellQuotingOptions, TaskDefinition, TaskDependency, TaskGroup, TaskIcon, TaskLike, TaskReference,
    TaskSourceConfigElement, TaskSourceKind, UriOrString, USER_TASKS_GROUP_KEY,
};

// ---------------------------------------------------------------------------------------------
// Contexto de execução suportada (de taskService.ts)
// ---------------------------------------------------------------------------------------------

thread_local! {
    static SHELL_EXECUTION_SUPPORTED: RawContextKey = RawContextKey::new("shellExecutionSupported", false, ContextKeyMeta::Description(localize("tasks.shellExecutionSupported", "Whether ShellExecution tasks are supported. Consider using in the when clause of a 'taskDefinition' contribution.", &[])));
    static PROCESS_EXECUTION_SUPPORTED: RawContextKey = RawContextKey::new("processExecutionSupported", false, ContextKeyMeta::Description(localize("tasks.processExecutionSupported", "Whether ProcessExecution tasks are supported. Consider using in the when clause of a 'taskDefinition' contribution.", &[])));
}

/// `ShellExecutionSupportedContext`.
pub fn shell_execution_supported_context() -> RawContextKey {
    SHELL_EXECUTION_SUPPORTED.with(Clone::clone)
}

/// `ProcessExecutionSupportedContext`.
pub fn process_execution_supported_context() -> RawContextKey {
    PROCESS_EXECUTION_SUPPORTED.with(Clone::clone)
}

// ---------------------------------------------------------------------------------------------
// Auxiliares do valor de JavaScript
// ---------------------------------------------------------------------------------------------

/// `!!value`.
pub fn truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(flag) => *flag,
        Value::Number(number) => number.as_f64().is_some_and(|number| number != 0.0 && !number.is_nan()),
        Value::String(text) => !text.is_empty(),
        Value::Array(_) | Value::Object(_) => true,
    }
}

fn truthy_field<'a>(value: &'a Value, name: &str) -> Option<&'a Value> {
    value.get(name).filter(|field| truthy(field))
}

fn is_string_array(value: &Value) -> bool {
    value.as_array().is_some_and(|items| items.iter().all(Value::is_string))
}

fn join_strings(value: &Value) -> String {
    value.as_array().map(|items| items.iter().filter_map(Value::as_str).collect::<Vec<_>>().join(" ")).unwrap_or_default()
}

/// A conversão de um valor para texto como o `String(value)` do JavaScript.
fn js_to_string(value: &Value) -> String {
    match value {
        Value::Null => "null".to_string(),
        Value::Bool(flag) => flag.to_string(),
        Value::Number(number) => je_base::dynamic::number_to_js_string(number.as_f64().unwrap_or(f64::NAN)),
        Value::String(text) => text.clone(),
        Value::Array(items) => items.iter().map(|item| if item.is_null() { String::new() } else { js_to_string(item) }).collect::<Vec<_>>().join(","),
        Value::Object(_) => "[object Object]".to_string(),
    }
}

fn str_field(value: &Value, name: &str) -> Option<String> {
    value.get(name).and_then(Value::as_str).map(str::to_string)
}

fn bool_field(value: &Value, name: &str) -> Option<bool> {
    value.get(name).and_then(Value::as_bool)
}

fn usize_field(value: &Value, name: &str) -> Option<usize> {
    let number = value.get(name)?;
    number.as_u64().map(|number| number as usize).or_else(|| number.as_f64().filter(|number| *number >= 0.0 && number.fract() == 0.0).map(|number| number as usize))
}

fn compact(value: &Value) -> String {
    serde_json::to_string(value).unwrap_or_default()
}

fn pretty(value: &Value) -> String {
    matcher_config::stringify(value)
}

/// `a || b` entre textos (o texto vazio é falso).
fn or_text(first: Option<String>, second: Option<String>) -> Option<String> {
    first.filter(|text| !text.is_empty()).or(second)
}

// ---------------------------------------------------------------------------------------------
// Tipos do contexto
// ---------------------------------------------------------------------------------------------

/// `TaskConfigurationValueWithErrors<T>`.
#[derive(Clone, Debug)]
pub struct ValueWithErrors<T> {
    pub value: Option<T>,
    pub errors: Option<Vec<String>>,
}

impl<T> Default for ValueWithErrors<T> {
    fn default() -> Self {
        Self { value: None, errors: None }
    }
}

/// `TaskConfigSource`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TaskConfigSource {
    TasksJson,
    WorkspaceFile,
    User,
}

/// `UUIDMap`.
#[derive(Clone, Debug, Default)]
pub struct UuidMap {
    last: Option<IndexMap<String, OneOrMany<String>>>,
    current: IndexMap<String, OneOrMany<String>>,
}

impl UuidMap {
    pub fn new(other: Option<&UuidMap>) -> Self {
        Self { last: None, current: other.map(|other| other.current.clone()).unwrap_or_default() }
    }

    pub fn start(&mut self) {
        self.last = Some(std::mem::take(&mut self.current));
    }

    pub fn get_uuid(&mut self, identifier: &str) -> String {
        let mut result: Option<String> = None;
        if let Some(last) = self.last.as_mut()
            && let Some(last_value) = last.get_mut(identifier)
        {
            let remove_last = match last_value {
                OneOrMany::Many(values) => {
                    if !values.is_empty() {
                        result = Some(values.remove(0));
                    }
                    values.is_empty()
                }
                OneOrMany::One(value) => {
                    result = Some(value.clone());
                    true
                }
            };
            if remove_last {
                last.shift_remove(identifier);
            }
        }
        let result = result.unwrap_or_else(generate_uuid);
        match self.current.get_mut(identifier) {
            None => {
                self.current.insert(identifier.to_string(), OneOrMany::One(result.clone()));
            }
            Some(OneOrMany::Many(values)) => values.push(result.clone()),
            Some(existing) => {
                if let OneOrMany::One(first) = existing.clone() {
                    *existing = OneOrMany::Many(vec![first, result.clone()]);
                }
            }
        }
        result
    }

    pub fn finish(&mut self) {
        self.last = None;
    }
}

/// `IParseContext`.
pub struct ParseContext {
    pub workspace_folder: WorkspaceFolder,
    pub workspace: Option<Rc<dyn IWorkspace>>,
    pub problem_reporter: Rc<dyn ProblemReporter>,
    pub named_problem_matchers: IndexMap<String, ProblemMatcher>,
    pub uuid_map: Rc<RefCell<UuidMap>>,
    pub engine: ExecutionEngine,
    pub schema_version: JsonSchemaVersion,
    pub platform: PlatformKind,
    pub task_load_issues: Vec<String>,
    pub context_key_service: Rc<dyn IContextKeyService>,
}

/// `IGlobals`.
#[derive(Clone, Debug, Default)]
pub struct Globals {
    pub command: Option<CommandConfiguration>,
    pub problem_matcher: Option<Vec<ProblemMatcher>>,
    pub prompt_on_close: Option<bool>,
    pub suppress_task_name: Option<bool>,
}

/// `ITaskParseResult`.
#[derive(Clone, Default)]
pub struct TaskParseResult {
    pub custom: Vec<CustomTask>,
    pub configured: Vec<ConfiguringTask>,
}

/// `IParseResult`.
pub struct ParseResult {
    pub validation_status: ValidationStatus,
    pub custom: Vec<CustomTask>,
    pub configured: Vec<ConfiguringTask>,
    pub engine: ExecutionEngine,
}

// ---------------------------------------------------------------------------------------------
// Mesclagem de campos
// ---------------------------------------------------------------------------------------------

macro_rules! all_none {
    ($value:expr, $($field:ident),+) => { true $(&& $value.$field.is_none())+ };
}

macro_rules! assign_fields {
    ($target:expr, $source:expr, $($field:ident),+) => { $( if $source.$field.is_some() { $target.$field = $source.$field.clone(); } )+ };
}

macro_rules! fill_fields {
    ($target:expr, $source:expr, $($field:ident),+) => { $( if $target.$field.is_none() { $target.$field = $source.$field.clone(); } )+ };
}

// ---------------------------------------------------------------------------------------------
// RunOptions
// ---------------------------------------------------------------------------------------------

/// `RunOnOptions.fromString`.
pub fn run_on_options_from_string(value: Option<&str>) -> RunOnOptions {
    let Some(value) = value.filter(|value| !value.is_empty()) else {
        return RunOnOptions::Default;
    };
    match value.to_lowercase().as_str() {
        "folderopen" => RunOnOptions::FolderOpen,
        "worktreecreated" => RunOnOptions::WorktreeCreated,
        _ => RunOnOptions::Default,
    }
}

/// `InstancePolicy.fromString`.
pub fn instance_policy_from_string(value: Option<&str>) -> InstancePolicy {
    let Some(value) = value.filter(|value| !value.is_empty()) else {
        return InstancePolicy::Prompt;
    };
    match value.to_lowercase().as_str() {
        "terminatenewest" => InstancePolicy::TerminateNewest,
        "terminateoldest" => InstancePolicy::TerminateOldest,
        "warn" => InstancePolicy::Warn,
        "silent" => InstancePolicy::Silent,
        _ => InstancePolicy::Prompt,
    }
}

fn run_options_is_empty(value: &RunOptions) -> bool {
    all_none!(value, reevaluate_on_rerun, run_on, instance_limit, instance_policy)
}

/// `RunOptions.fromConfiguration`.
pub fn run_options_from_configuration(value: Option<&Value>) -> RunOptions {
    let Some(value) = value.filter(|value| truthy(value)) else {
        return RunOptions {
            reevaluate_on_rerun: Some(true),
            run_on: Some(RunOnOptions::Default),
            instance_limit: Some(1.0),
            instance_policy: Some(InstancePolicy::Prompt),
        };
    };
    let instance_limit = value.get("instanceLimit").filter(|limit| truthy(limit)).and_then(Value::as_f64);
    RunOptions {
        reevaluate_on_rerun: bool_field(value, "reevaluateOnRerun"),
        run_on: Some(run_on_options_from_string(value.get("runOn").and_then(Value::as_str))),
        instance_limit: Some(instance_limit.map_or(1.0, |limit| limit.max(1.0))),
        instance_policy: Some(instance_policy_from_string(value.get("instancePolicy").and_then(Value::as_str))),
    }
}

/// `RunOptions.assignProperties`.
pub fn run_options_assign_properties(target: RunOptions, source: Option<&RunOptions>) -> RunOptions {
    let Some(source) = source.filter(|source| !run_options_is_empty(source)) else {
        return target;
    };
    if run_options_is_empty(&target) {
        return source.clone();
    }
    let mut target = target;
    assign_fields!(target, source, reevaluate_on_rerun, run_on, instance_limit, instance_policy);
    target
}

/// `RunOptions.fillProperties`.
pub fn run_options_fill_properties(target: RunOptions, source: Option<&RunOptions>) -> RunOptions {
    let Some(source) = source.filter(|source| !run_options_is_empty(source)) else {
        return target;
    };
    if run_options_is_empty(&target) {
        return source.clone();
    }
    let mut target = target;
    fill_fields!(target, source, reevaluate_on_rerun, run_on, instance_limit, instance_policy);
    target
}

// ---------------------------------------------------------------------------------------------
// ShellConfiguration
// ---------------------------------------------------------------------------------------------

/// `ShellConfiguration.is`.
fn shell_configuration_is(value: Option<&Value>) -> bool {
    let Some(candidate) = value.filter(|value| value.is_object()) else {
        return false;
    };
    candidate.get("executable").is_some_and(Value::is_string) || candidate.get("args").is_some_and(is_string_array)
}

fn shell_quoting_options_from(value: &Value) -> ShellQuotingOptions {
    let escape = match value.get("escape") {
        Some(Value::String(text)) => Some(ShellEscape::Char(text.clone())),
        Some(object @ Value::Object(_)) => {
            Some(ShellEscape::Chars { escape_char: str_field(object, "escapeChar").unwrap_or_default(), chars_to_escape: str_field(object, "charsToEscape").unwrap_or_default() })
        }
        _ => None,
    };
    ShellQuotingOptions { escape, strong: str_field(value, "strong"), weak: str_field(value, "weak") }
}

/// `ShellConfiguration.from`.
fn shell_configuration_from(config: Option<&Value>) -> Option<ShellConfiguration> {
    if !shell_configuration_is(config) {
        return None;
    }
    let config = config?;
    let mut result = ShellConfiguration::default();
    result.executable = str_field(config, "executable");
    if let Some(args) = config.get("args").filter(|args| is_string_array(args)) {
        result.args = Some(args.as_array().into_iter().flatten().filter_map(Value::as_str).map(str::to_string).collect());
    }
    if let Some(quoting) = config.get("quoting").filter(|quoting| quoting.is_object()) {
        result.quoting = Some(shell_quoting_options_from(quoting));
    }
    Some(result)
}

fn shell_is_empty(value: &ShellConfiguration, allow_empty_array: bool) -> bool {
    value.executable.is_none() && value.quoting.is_none() && value.args.as_ref().is_none_or(|args| args.is_empty() && !allow_empty_array)
}

fn shell_assign_properties(target: Option<ShellConfiguration>, source: Option<&ShellConfiguration>) -> Option<ShellConfiguration> {
    let Some(source) = source.filter(|source| !shell_is_empty(source, false)) else {
        return target;
    };
    let Some(mut target) = target.filter(|target| !shell_is_empty(target, false)) else {
        return Some(source.clone());
    };
    assign_fields!(target, source, executable, args, quoting);
    Some(target)
}

fn shell_fill_properties(target: Option<ShellConfiguration>, source: Option<&ShellConfiguration>) -> Option<ShellConfiguration> {
    let Some(source) = source.filter(|source| !shell_is_empty(source, false)) else {
        return target;
    };
    let Some(mut target) = target.filter(|target| !shell_is_empty(target, true)) else {
        return Some(source.clone());
    };
    fill_fields!(target, source, executable, args, quoting);
    Some(target)
}

// ---------------------------------------------------------------------------------------------
// CommandOptions
// ---------------------------------------------------------------------------------------------

fn command_options_is_empty(value: Option<&CommandOptions>) -> bool {
    value.is_none_or(|value| all_none!(value, cwd, env, shell))
}

/// `CommandOptions.from`.
fn command_options_from(options: &Value, context: &mut ParseContext) -> Option<CommandOptions> {
    let mut result = CommandOptions::default();
    if let Some(cwd) = options.get("cwd") {
        if let Some(cwd) = cwd.as_str() {
            result.cwd = Some(cwd.to_string());
        } else {
            context.task_load_issues.push(localize(
                "ConfigurationParser.invalidCWD",
                "Warning: options.cwd must be of type string. Ignoring value {0}\n",
                &[&js_to_string(cwd)],
            ));
        }
    }
    if let Some(Value::Object(env)) = options.get("env") {
        result.env = Some(env.clone());
    }
    result.shell = shell_configuration_from(options.get("shell"));
    if command_options_is_empty(Some(&result)) { None } else { Some(result) }
}

/// `CommandOptions.assignProperties`.
pub fn command_options_assign_properties(target: Option<CommandOptions>, source: Option<CommandOptions>) -> Option<CommandOptions> {
    if command_options_is_empty(source.as_ref()) {
        return target;
    }
    let source = source?;
    if command_options_is_empty(target.as_ref()) {
        return Some(source);
    }
    let mut target = target?;
    assign_fields!(target, source, cwd);
    let merged_env = match (&target.env, &source.env) {
        (None, _) => source.env.clone(),
        (Some(target_env), Some(source_env)) => {
            let mut env = JsonObject::new();
            for (key, value) in target_env {
                env.insert(key.clone(), value.clone());
            }
            for (key, value) in source_env {
                env.insert(key.clone(), value.clone());
            }
            Some(env)
        }
        (Some(_), None) => target.env.clone(),
    };
    target.env = merged_env;
    target.shell = shell_assign_properties(target.shell.take(), source.shell.as_ref());
    Some(target)
}

/// `CommandOptions.fillProperties`.
pub fn command_options_fill_properties(target: Option<CommandOptions>, source: Option<&CommandOptions>) -> Option<CommandOptions> {
    if command_options_is_empty(source) {
        return target;
    }
    let source = source?;
    if command_options_is_empty(target.as_ref()) {
        return Some(source.clone());
    }
    let mut target = target?;
    fill_fields!(target, source, cwd, env);
    let shell = shell_fill_properties(target.shell.take(), source.shell.as_ref());
    target.shell = shell;
    Some(target)
}

/// `CommandOptions.fillDefaults`.
pub fn command_options_fill_defaults(value: Option<CommandOptions>) -> Option<CommandOptions> {
    let Some(mut value) = value else {
        return Some(CommandOptions::defaults());
    };
    if value.cwd.is_none() {
        value.cwd = CommandOptions::defaults().cwd;
    }
    Some(value)
}

// ---------------------------------------------------------------------------------------------
// PresentationOptions
// ---------------------------------------------------------------------------------------------

fn presentation_is_empty(value: &PresentationOptions) -> bool {
    all_none!(value, echo, reveal, reveal_problems, focus, panel, show_reuse_message, clear, group, close, preserve_terminal_name)
}

/// `PresentationOptions.from`: lê as propriedades de apresentação (`presentation`, `terminal`, `showOutput` e
/// `echoCommand`) de uma tarefa ou da configuração global.
pub fn presentation_options_from(config: &Value, _context: &ParseContext) -> Option<PresentationOptions> {
    let mut result = PresentationOptions::default();
    let mut has_props = false;
    if let Some(echo) = bool_field(config, "echoCommand") {
        result.echo = Some(echo);
        has_props = true;
    }
    if let Some(show_output) = config.get("showOutput").and_then(Value::as_str) {
        result.reveal = Some(RevealKind::from_string(show_output));
        has_props = true;
    }
    let presentation = truthy_field(config, "presentation").or_else(|| truthy_field(config, "terminal"));
    if let Some(presentation) = presentation {
        if let Some(echo) = bool_field(presentation, "echo") {
            result.echo = Some(echo);
        }
        if let Some(reveal) = presentation.get("reveal").and_then(Value::as_str) {
            result.reveal = Some(RevealKind::from_string(reveal));
        }
        if let Some(reveal_problems) = presentation.get("revealProblems").and_then(Value::as_str) {
            result.reveal_problems = Some(RevealProblemKind::from_string(reveal_problems));
        }
        if let Some(focus) = bool_field(presentation, "focus") {
            result.focus = Some(focus);
        }
        if let Some(panel) = presentation.get("panel").and_then(Value::as_str) {
            result.panel = Some(PanelKind::from_string(panel));
        }
        if let Some(show_reuse_message) = bool_field(presentation, "showReuseMessage") {
            result.show_reuse_message = Some(show_reuse_message);
        }
        if let Some(clear) = bool_field(presentation, "clear") {
            result.clear = Some(clear);
        }
        if let Some(group) = presentation.get("group").and_then(Value::as_str) {
            result.group = Some(group.to_string());
        }
        if let Some(close) = bool_field(presentation, "close") {
            result.close = Some(close);
        }
        if let Some(preserve_terminal_name) = bool_field(presentation, "preserveTerminalName") {
            result.preserve_terminal_name = Some(preserve_terminal_name);
        }
        has_props = true;
    }
    if !has_props {
        return None;
    }
    Some(result)
}

/// `PresentationOptions.assignProperties`.
pub fn presentation_options_assign_properties(target: Option<PresentationOptions>, source: Option<&PresentationOptions>) -> Option<PresentationOptions> {
    let Some(source) = source.filter(|source| !presentation_is_empty(source)) else {
        return target;
    };
    let Some(mut target) = target.filter(|target| !presentation_is_empty(target)) else {
        return Some(source.clone());
    };
    assign_fields!(target, source, echo, reveal, reveal_problems, focus, panel, show_reuse_message, clear, group, close, preserve_terminal_name);
    Some(target)
}

/// `PresentationOptions.fillProperties`.
pub fn presentation_options_fill_properties(target: Option<PresentationOptions>, source: Option<&PresentationOptions>) -> Option<PresentationOptions> {
    let Some(source) = source.filter(|source| !presentation_is_empty(source)) else {
        return target;
    };
    let Some(mut target) = target.filter(|target| !presentation_is_empty(target)) else {
        return Some(source.clone());
    };
    fill_fields!(target, source, echo, reveal, reveal_problems, focus, panel, show_reuse_message, clear, group, close, preserve_terminal_name);
    Some(target)
}

/// `PresentationOptions.fillDefaults`.
pub fn presentation_options_fill_defaults(value: Option<PresentationOptions>, context: &ParseContext) -> Option<PresentationOptions> {
    let default_echo = context.engine == ExecutionEngine::Terminal;
    let defaults = PresentationOptions { echo: Some(default_echo), ..PresentationOptions::defaults() };
    let Some(mut value) = value else {
        return Some(defaults);
    };
    fill_fields!(value, defaults, echo, reveal, reveal_problems, focus, panel, show_reuse_message, clear, group, close, preserve_terminal_name);
    Some(value)
}

// ---------------------------------------------------------------------------------------------
// CommandConfiguration
// ---------------------------------------------------------------------------------------------

/// `ShellString.from`.
fn shell_string_from(value: Option<&Value>) -> Option<CommandString> {
    match value? {
        Value::Null => None,
        Value::String(text) => Some(CommandString::Plain(text.clone())),
        array @ Value::Array(_) if is_string_array(array) => Some(CommandString::Plain(join_strings(array))),
        other => {
            let quoting = ShellQuoting::from(other.get("quoting").and_then(Value::as_str));
            let result = match other.get("value") {
                Some(Value::String(text)) => Some(text.clone()),
                Some(array @ Value::Array(_)) if is_string_array(array) => Some(join_strings(array)),
                _ => None,
            };
            result.filter(|result| !result.is_empty()).map(|value| CommandString::Quoted(QuotedString { value, quoting }))
        }
    }
}

fn command_is_empty(value: Option<&CommandConfiguration>) -> bool {
    value.is_none_or(|value| all_none!(value, runtime, name, options, task_selector, suppress_task_name, presentation) && value.args.as_ref().is_none_or(|args| args.is_empty()))
}

/// `CommandConfiguration.hasCommand`.
pub fn command_has_command(value: &CommandConfiguration) -> bool {
    match &value.name {
        None => false,
        Some(CommandString::Plain(name)) => !name.is_empty(),
        Some(CommandString::Quoted(_)) => true,
    }
}

fn command_from_base(config: &Value, context: &mut ParseContext) -> Option<CommandConfiguration> {
    let name = shell_string_from(config.get("command"));
    let mut runtime: Option<RuntimeType> = None;
    if let Some(config_type) = config.get("type").and_then(Value::as_str)
        && (config_type == "shell" || config_type == "process")
    {
        runtime = Some(RuntimeType::from_string(config_type));
    }
    let is_shell_command = config.get("isShellCommand");
    if is_shell_command.is_some_and(Value::is_boolean) || shell_configuration_is(is_shell_command) {
        runtime = Some(RuntimeType::Shell);
    } else if let Some(is_shell_command) = is_shell_command {
        runtime = Some(if truthy(is_shell_command) { RuntimeType::Shell } else { RuntimeType::Process });
    }

    let mut result = CommandConfiguration { name, runtime, presentation: presentation_options_from(config, context), ..Default::default() };

    if let Some(args) = config.get("args") {
        let mut converted_args = Vec::new();
        for arg in args.as_array().into_iter().flatten() {
            match shell_string_from(Some(arg)) {
                Some(converted) => converted_args.push(converted),
                None => context.task_load_issues.push(localize(
                    "ConfigurationParser.inValidArg",
                    "Error: command argument must either be a string or a quoted string. Provided value is:\n{0}",
                    &[&if truthy(arg) { pretty(arg) } else { "undefined".to_string() }],
                )),
            }
        }
        result.args = Some(converted_args);
    }
    if let Some(options) = config.get("options") {
        result.options = if options.is_object() { command_options_from(options, context) } else { None };
        if let Some(result_options) = result.options.as_mut()
            && result_options.shell.is_none()
            && shell_configuration_is(is_shell_command)
        {
            result_options.shell = shell_configuration_from(is_shell_command);
            if context.engine != ExecutionEngine::Terminal {
                context.task_load_issues.push(localize(
                    "ConfigurationParser.noShell",
                    "Warning: shell configuration is only supported when executing tasks in the terminal.",
                    &[],
                ));
            }
        }
    }

    if let Some(task_selector) = config.get("taskSelector").and_then(Value::as_str) {
        result.task_selector = Some(task_selector.to_string());
    }
    if let Some(suppress) = bool_field(config, "suppressTaskName") {
        result.suppress_task_name = Some(suppress);
    }

    if command_is_empty(Some(&result)) { None } else { Some(result) }
}

/// `CommandConfiguration.assignProperties`.
pub fn command_assign_properties(target: Option<CommandConfiguration>, source: Option<CommandConfiguration>, overwrite_args: bool) -> Option<CommandConfiguration> {
    if command_is_empty(source.as_ref()) {
        return target;
    }
    let source = source?;
    if command_is_empty(target.as_ref()) {
        return Some(source);
    }
    let mut target = target?;
    assign_fields!(target, source, name, runtime, task_selector, suppress_task_name);
    if let Some(source_args) = &source.args {
        match target.args.as_mut() {
            Some(target_args) if !overwrite_args => target_args.extend(source_args.iter().cloned()),
            _ => target.args = Some(source_args.clone()),
        }
    }
    target.presentation = presentation_options_assign_properties(target.presentation.take(), source.presentation.as_ref());
    target.options = command_options_assign_properties(target.options.take(), source.options.clone());
    Some(target)
}

/// `CommandConfiguration.from`.
pub fn command_from(config: &Value, context: &mut ParseContext) -> Option<CommandConfiguration> {
    let mut result = command_from_base(config, context);

    let mut os_config: Option<CommandConfiguration> = None;
    if let Some(windows) = truthy_field(config, "windows")
        && context.platform == PlatformKind::Windows
    {
        os_config = command_from_base(windows, context);
    } else if let Some(osx) = truthy_field(config, "osx")
        && context.platform == PlatformKind::Mac
    {
        os_config = command_from_base(osx, context);
    } else if let Some(linux) = truthy_field(config, "linux")
        && context.platform == PlatformKind::Linux
    {
        os_config = command_from_base(linux, context);
    }
    if os_config.is_some() {
        result = command_assign_properties(result, os_config, context.schema_version == JsonSchemaVersion::V2_0_0);
    }
    if command_is_empty(result.as_ref()) { None } else { result }
}

/// `CommandConfiguration.fillGlobals`.
pub fn command_fill_globals(target: CommandConfiguration, source: Option<&CommandConfiguration>, task_name: Option<&str>) -> CommandConfiguration {
    let Some(source) = source.filter(|source| !command_is_empty(Some(source))) else {
        return target;
    };
    let mut target = target;
    if target.name.is_none() {
        fill_fields!(target, source, name, task_selector, suppress_task_name);
        let mut args: Vec<CommandString> = source.args.clone().unwrap_or_default();
        if target.suppress_task_name != Some(true)
            && let Some(task_name) = task_name.filter(|task_name| !task_name.is_empty())
        {
            match &target.task_selector {
                Some(selector) => args.push(CommandString::Plain(format!("{selector}{task_name}"))),
                None => args.push(CommandString::Plain(task_name.to_string())),
            }
        }
        if let Some(target_args) = &target.args {
            args.extend(target_args.iter().cloned());
        }
        target.args = Some(args);
    }
    fill_fields!(target, source, runtime);

    target.presentation = presentation_options_fill_properties(target.presentation.take(), source.presentation.as_ref());
    target.options = command_options_fill_properties(target.options.take(), source.options.as_ref());
    target
}

/// `CommandConfiguration.fillDefaults`.
pub fn command_fill_defaults(value: &mut CommandConfiguration, context: &ParseContext) {
    if value.name.is_some() && value.runtime.is_none() {
        value.runtime = Some(RuntimeType::Process);
    }
    value.presentation = presentation_options_fill_defaults(value.presentation.take(), context);
    if !command_is_empty(Some(value)) {
        value.options = command_options_fill_defaults(value.options.take());
    }
    if value.args.is_none() {
        value.args = Some(Vec::new());
    }
    if value.suppress_task_name.is_none() {
        value.suppress_task_name = Some(context.schema_version == JsonSchemaVersion::V2_0_0);
    }
}

// ---------------------------------------------------------------------------------------------
// ProblemMatcherConverter
// ---------------------------------------------------------------------------------------------

fn pattern_config_from(value: &Value) -> matcher_config::ProblemPattern {
    matcher_config::ProblemPattern {
        regexp: str_field(value, "regexp"),
        kind: str_field(value, "kind"),
        file: usize_field(value, "file"),
        location: usize_field(value, "location"),
        line: usize_field(value, "line"),
        column: usize_field(value, "column"),
        end_line: usize_field(value, "endLine"),
        end_column: usize_field(value, "endColumn"),
        severity: usize_field(value, "severity"),
        code: usize_field(value, "code"),
        message: usize_field(value, "message"),
        r#loop: bool_field(value, "loop"),
    }
}

fn watching_pattern_from(value: &Value) -> Option<matcher_config::WatchingPatternValue> {
    match value {
        Value::String(regexp) => Some(matcher_config::WatchingPatternValue::Regexp(regexp.clone())),
        Value::Object(_) => Some(matcher_config::WatchingPatternValue::Pattern(matcher_config::WatchingPattern { regexp: str_field(value, "regexp"), file: usize_field(value, "file") })),
        _ => None,
    }
}

fn background_monitor_from(value: &Value) -> Option<matcher_config::BackgroundMonitor> {
    if !value.is_object() {
        return None;
    }
    Some(matcher_config::BackgroundMonitor {
        active_on_start: bool_field(value, "activeOnStart"),
        begins_pattern: value.get("beginsPattern").and_then(watching_pattern_from),
        ends_pattern: value.get("endsPattern").and_then(watching_pattern_from),
    })
}

fn one_or_many_strings(value: &Value) -> Option<OneOrMany<String>> {
    match value {
        Value::String(text) => Some(OneOrMany::One(text.clone())),
        Value::Array(items) => Some(OneOrMany::Many(items.iter().filter_map(Value::as_str).map(str::to_string).collect())),
        _ => None,
    }
}

fn file_location_from(value: &Value) -> Option<matcher_config::FileLocation> {
    match value {
        Value::String(text) => Some(matcher_config::FileLocation::Single(text.clone())),
        Value::Array(items) if is_string_array(value) => Some(matcher_config::FileLocation::Strings(items.iter().filter_map(Value::as_str).map(str::to_string).collect())),
        Value::Array(items) => {
            let first = items.first().and_then(Value::as_str).unwrap_or_default().to_string();
            let args = items.get(1).filter(|args| args.is_object()).map(|args| matcher_config::SearchFileLocationArgs {
                include: args.get("include").and_then(one_or_many_strings),
                exclude: args.get("exclude").and_then(one_or_many_strings),
            });
            Some(matcher_config::FileLocation::SearchArray(first, args))
        }
        _ => None,
    }
}

/// Converte a descrição de um casador de problemas lida do JSON em `Config.ProblemMatcher`.
pub fn problem_matcher_from_json(value: &Value) -> matcher_config::ProblemMatcher {
    let pattern = match value.get("pattern") {
        Some(Value::String(name)) => Some(matcher_config::PatternValue::Name(name.clone())),
        Some(object @ Value::Object(_)) => Some(matcher_config::PatternValue::Single(pattern_config_from(object))),
        Some(Value::Array(items)) => Some(matcher_config::PatternValue::Multi(items.iter().map(pattern_config_from).collect())),
        _ => None,
    };
    matcher_config::ProblemMatcher {
        base: str_field(value, "base"),
        owner: str_field(value, "owner"),
        source: str_field(value, "source"),
        apply_to: str_field(value, "applyTo"),
        severity: str_field(value, "severity"),
        file_location: value.get("fileLocation").and_then(file_location_from),
        pattern,
        watched_task_begins_reg_exp: str_field(value, "watchedTaskBeginsRegExp"),
        watched_task_ends_reg_exp: str_field(value, "watchedTaskEndsRegExp"),
        watching: value.get("watching").and_then(background_monitor_from),
        background: value.get("background").and_then(background_monitor_from),
        name: str_field(value, "name"),
        label: str_field(value, "label"),
    }
}

/// `namespace ProblemMatcherConverter`.
pub mod problem_matcher_converter {
    use super::*;

    #[derive(PartialEq, Eq)]
    enum ProblemMatcherKind {
        Unknown,
        String,
        ProblemMatcher,
        Array,
    }

    /// `ProblemMatcherConverter.namedFrom`.
    pub fn named_from(declares: Option<&Value>, context: &ParseContext) -> IndexMap<String, ProblemMatcher> {
        let mut result: IndexMap<String, ProblemMatcher> = IndexMap::new();
        let Some(Value::Array(declares)) = declares else {
            return result;
        };
        for value in declares {
            let named_problem_matcher = ProblemMatcherParser::new(context.problem_reporter.clone()).parse(&problem_matcher_from_json(value));
            if is_named_problem_matcher(named_problem_matcher.as_ref())
                && let Some(matcher) = named_problem_matcher
            {
                let name = matcher.name.clone().unwrap_or_default();
                result.insert(name, matcher);
            } else {
                context.problem_reporter.error(&localize("ConfigurationParser.noName", "Error: Problem Matcher in declare scope must have a name:\n{0}\n", &[&pretty(value)]));
            }
        }
        result
    }

    /// `ProblemMatcherConverter.fromWithOsConfig`.
    pub fn from_with_os_config(external: &Value, context: &ParseContext) -> ValueWithErrors<Vec<ProblemMatcher>> {
        let os_matcher = |name: &str| external.get(name).and_then(|section| truthy_field(section, "problemMatcher"));
        if let Some(matcher) = os_matcher("windows").filter(|_| context.platform == PlatformKind::Windows) {
            from(Some(matcher), context)
        } else if let Some(matcher) = os_matcher("osx").filter(|_| context.platform == PlatformKind::Mac) {
            from(Some(matcher), context)
        } else if let Some(matcher) = os_matcher("linux").filter(|_| context.platform == PlatformKind::Linux) {
            from(Some(matcher), context)
        } else if let Some(matcher) = truthy_field(external, "problemMatcher") {
            from(Some(matcher), context)
        } else {
            ValueWithErrors::default()
        }
    }

    fn get_problem_matcher_kind(value: Option<&Value>) -> ProblemMatcherKind {
        match value {
            Some(Value::String(_)) => ProblemMatcherKind::String,
            Some(Value::Array(_)) => ProblemMatcherKind::Array,
            Some(_) => ProblemMatcherKind::ProblemMatcher,
            None => ProblemMatcherKind::Unknown,
        }
    }

    /// `ProblemMatcherConverter.from`.
    pub fn from(config: Option<&Value>, context: &ParseContext) -> ValueWithErrors<Vec<ProblemMatcher>> {
        let mut result: Vec<ProblemMatcher> = Vec::new();
        let Some(config_value) = config else {
            return ValueWithErrors { value: Some(result), errors: None };
        };
        let mut errors: Vec<String> = Vec::new();
        let mut add_result = |matcher: ValueWithErrors<ProblemMatcher>| {
            if let Some(value) = matcher.value {
                result.push(value);
            }
            if let Some(matcher_errors) = matcher.errors {
                errors.extend(matcher_errors);
            }
        };
        let kind = get_problem_matcher_kind(config);
        if kind == ProblemMatcherKind::Unknown {
            let error = localize(
                "ConfigurationParser.unknownMatcherKind",
                "Warning: the defined problem matcher is unknown. Supported types are string | ProblemMatcher | Array<string | ProblemMatcher>.\n{0}\n",
                &["undefined"],
            );
            context.problem_reporter.warn(&error);
        } else if kind == ProblemMatcherKind::String || kind == ProblemMatcherKind::ProblemMatcher {
            add_result(resolve_problem_matcher(config_value, context));
        } else if let Value::Array(problem_matchers) = config_value {
            for problem_matcher in problem_matchers {
                add_result(resolve_problem_matcher(problem_matcher, context));
            }
        }
        ValueWithErrors { value: Some(result), errors: Some(errors) }
    }

    fn resolve_problem_matcher(value: &Value, context: &ParseContext) -> ValueWithErrors<ProblemMatcher> {
        if let Value::String(text) = value {
            let mut variable_name = text.as_str();
            if variable_name.chars().count() > 1 && variable_name.starts_with('$') {
                variable_name = &variable_name[1..];
                if let Some(global) = problem_matcher_registry().get(variable_name) {
                    return ValueWithErrors { value: Some(global), errors: None };
                }
                if let Some(local) = context.named_problem_matchers.get(variable_name) {
                    let mut local_problem_matcher = local.clone();
                    // remove the name
                    local_problem_matcher.name = None;
                    return ValueWithErrors { value: Some(local_problem_matcher), errors: None };
                }
            }
            ValueWithErrors {
                value: None,
                errors: Some(vec![localize("ConfigurationParser.invalidVariableReference", "Error: Invalid problemMatcher reference: {0}\n", &[text.as_str()])]),
            }
        } else {
            ValueWithErrors { value: ProblemMatcherParser::new(context.problem_reporter.clone()).parse(&problem_matcher_from_json(value)), errors: None }
        }
    }
}

// ---------------------------------------------------------------------------------------------
// GroupKind, TaskDependency e DependsOrder
// ---------------------------------------------------------------------------------------------

/// `namespace GroupKind`.
pub mod group_kind {
    use super::*;

    /// `GroupKind.from`.
    pub fn from(external: Option<&Value>) -> Option<GroupValue> {
        let external = external?;
        if let Some(name) = external.as_str()
            && TaskGroup::is(name)
        {
            return Some(GroupValue::Group(Rc::new(TaskGroup { id: name.to_string(), is_default: Some(IsDefault::Bool(false)) })));
        }
        let kind = external.get("kind").and_then(Value::as_str).filter(|kind| TaskGroup::is(kind))?;
        let is_default = match external.get("isDefault") {
            None => Some(IsDefault::Bool(false)),
            Some(Value::Bool(flag)) => Some(IsDefault::Bool(*flag)),
            Some(Value::String(text)) => Some(IsDefault::Str(text.clone())),
            Some(_) => None,
        };
        Some(GroupValue::Group(Rc::new(TaskGroup { id: kind.to_string(), is_default })))
    }

    /// `GroupKind.to`: `string | IGroupKind`.
    pub fn to(group: &GroupValue) -> Value {
        match group {
            GroupValue::Name(name) => Value::from(name.as_str()),
            GroupValue::Group(group) => {
                let truthy_default = match &group.is_default {
                    None => false,
                    Some(IsDefault::Bool(flag)) => *flag,
                    Some(IsDefault::Str(text)) => !text.is_empty(),
                };
                if !truthy_default {
                    return Value::from(group.id.as_str());
                }
                let mut object = Map::new();
                object.insert("kind".to_string(), Value::from(group.id.as_str()));
                object.insert(
                    "isDefault".to_string(),
                    match &group.is_default {
                        Some(IsDefault::Str(text)) => Value::from(text.as_str()),
                        _ => Value::Bool(true),
                    },
                );
                Value::Object(object)
            }
        }
    }
}

fn task_dependency_uri(context: &ParseContext, source: TaskConfigSource) -> UriOrString {
    match source {
        TaskConfigSource::User => UriOrString::Str(USER_TASKS_GROUP_KEY.to_string()),
        TaskConfigSource::TasksJson => UriOrString::Uri(context.workspace_folder.uri.clone()),
        TaskConfigSource::WorkspaceFile => match context.workspace.as_ref().and_then(|workspace| workspace.configuration()) {
            Some(configuration) => UriOrString::Uri(configuration),
            None => UriOrString::Uri(context.workspace_folder.uri.clone()),
        },
    }
}

/// `TaskDependency.from`.
fn task_dependency_from(external: &Value, context: &ParseContext, source: TaskConfigSource) -> Option<TaskDependency> {
    if let Some(label) = external.as_str() {
        return Some(TaskDependency { uri: task_dependency_uri(context, source), task: Some(TaskReference::Label(label.to_string())) });
    }
    if external.get("type").is_some_and(Value::is_string)
        && let Value::Object(identifier) = external
    {
        return Some(TaskDependency {
            uri: task_dependency_uri(context, source),
            task: tasks::create_task_identifier(identifier, context.problem_reporter.as_ref()).map(TaskReference::Identifier),
        });
    }
    None
}

/// `DependsOrder.from`.
fn depends_order_from(order: Option<&str>) -> DependsOrder {
    match order {
        Some("sequence") => DependsOrder::Sequence,
        _ => DependsOrder::Parallel,
    }
}

// ---------------------------------------------------------------------------------------------
// ConfigurationProperties
// ---------------------------------------------------------------------------------------------

fn properties_is_empty(value: &ConfigurationProperties) -> bool {
    all_none!(value, name, identifier, group, is_background, prompt_on_close, presentation, options, icon, hide, in_agents)
        && value.depends_on.as_ref().is_none_or(|items| items.is_empty())
        && value.problem_matchers.as_ref().is_none_or(|items| items.is_empty())
}

fn icon_from(value: Option<&Value>) -> Option<TaskIcon> {
    let value = value.filter(|value| value.is_object())?;
    Some(TaskIcon { id: str_field(value, "id"), color: str_field(value, "color") })
}

/// `ConfigurationProperties.from`.
fn configuration_properties_from(
    external: &Value,
    context: &mut ParseContext,
    include_command_options: bool,
    source: TaskConfigSource,
    properties: Option<&JsonObject>,
) -> ValueWithErrors<ConfigurationProperties> {
    if !truthy(external) {
        return ValueWithErrors::default();
    }
    let mut result = ConfigurationProperties::default();

    if let Some(properties) = properties {
        for property_name in properties.keys() {
            if let Some(value) = external.get(property_name) {
                result.extra.insert(property_name.clone(), value.clone());
            }
        }
    }

    if let Some(task_name) = external.get("taskName").and_then(Value::as_str) {
        result.name = Some(task_name.to_string());
    }
    if let Some(label) = external.get("label").and_then(Value::as_str)
        && context.schema_version == JsonSchemaVersion::V2_0_0
    {
        result.name = Some(label.to_string());
    }
    if let Some(identifier) = external.get("identifier").and_then(Value::as_str) {
        result.identifier = Some(identifier.to_string());
    }
    result.icon = icon_from(external.get("icon"));
    result.hide = bool_field(external, "hide");
    result.in_agents = bool_field(external, "inAgents");
    if let Some(is_background) = external.get("isBackground") {
        result.is_background = Some(truthy(is_background));
    }
    if let Some(prompt_on_close) = external.get("promptOnClose") {
        result.prompt_on_close = Some(truthy(prompt_on_close));
    }
    result.group = group_kind::from(external.get("group"));
    if let Some(depends_on) = external.get("dependsOn") {
        if let Value::Array(items) = depends_on {
            let mut dependencies = Vec::new();
            for item in items {
                if let Some(dependency) = task_dependency_from(item, context, source) {
                    dependencies.push(dependency);
                }
            }
            result.depends_on = Some(dependencies);
        } else {
            result.depends_on = task_dependency_from(depends_on, context, source).map(|dependency| vec![dependency]);
        }
    }
    result.depends_order = Some(depends_order_from(external.get("dependsOrder").and_then(Value::as_str)));
    if include_command_options && (external.get("presentation").is_some() || external.get("terminal").is_some()) {
        result.presentation = presentation_options_from(external, context);
    }
    if include_command_options && let Some(options) = external.get("options") {
        result.options = if options.is_object() { command_options_from(options, context) } else { None };
    }
    let config_problem_matcher = problem_matcher_converter::from_with_os_config(external, context);
    if let Some(matchers) = config_problem_matcher.value {
        result.problem_matchers = Some(matchers.into_iter().map(ProblemMatcherValue::Matcher).collect());
    }
    if let Some(detail) = external.get("detail").filter(|detail| truthy(detail)).and_then(Value::as_str) {
        result.detail = Some(detail.to_string());
    }
    if properties_is_empty(&result) {
        ValueWithErrors::default()
    } else {
        ValueWithErrors { value: Some(result), errors: config_problem_matcher.errors }
    }
}

/// O `Object.assign(task.configurationProperties, value)` de `ConfiguringTask.from` e `CustomTask.from`: o `name` e o
/// `identifier` só entram quando o valor os traz, e os demais campos o objeto lido sempre define (ou deixa em
/// branco, como a tarefa recém-criada já os tem).
fn merge_configuration_properties(target: &mut ConfigurationProperties, value: ConfigurationProperties) {
    if value.name.is_some() {
        target.name = value.name;
    }
    if value.identifier.is_some() {
        target.identifier = value.identifier;
    }
    target.group = value.group;
    target.presentation = value.presentation;
    target.options = value.options;
    target.is_background = value.is_background;
    target.prompt_on_close = value.prompt_on_close;
    target.depends_on = value.depends_on;
    target.depends_order = value.depends_order;
    target.detail = value.detail;
    target.problem_matchers = value.problem_matchers;
    target.icon = value.icon;
    target.hide = value.hide;
    target.in_agents = value.in_agents;
    target.extra.extend(value.extra);
}

// ---------------------------------------------------------------------------------------------
// ConfiguringTask e CustomTask
// ---------------------------------------------------------------------------------------------

const TASK_SOURCE_LABEL: &str = "Workspace";

fn lookup_definition(registry: Option<&dyn TaskDefinitionGetter>, task_type: &str) -> Option<TaskDefinition> {
    registry.and_then(|registry| registry.get(task_type)).or_else(|| task_definition_registry().get(task_type))
}

fn file_based_source(source: TaskConfigSource, config: TaskSourceConfigElement) -> FileBasedTaskSource {
    let kind = match source {
        TaskConfigSource::User => TaskSourceKind::User,
        TaskConfigSource::WorkspaceFile => TaskSourceKind::WorkspaceFile,
        TaskConfigSource::TasksJson => TaskSourceKind::Workspace,
    };
    FileBasedTaskSource { kind: Some(kind), label: TASK_SOURCE_LABEL.to_string(), config, customizes: None }
}

fn substring_from(text: &str, start: usize) -> String {
    text.chars().skip(start).collect()
}

/// `ConfiguringTask.from`.
fn configuring_task_from(
    external: &Value,
    context: &mut ParseContext,
    index: usize,
    source: TaskConfigSource,
    registry: Option<&dyn TaskDefinitionGetter>,
) -> Option<ConfiguringTask> {
    if !truthy(external) {
        return None;
    }
    let task_type = external.get("type").filter(|value| truthy(value));
    let customize = external.get("customize").filter(|value| truthy(value));
    if task_type.is_none() && customize.is_none() {
        context.problem_reporter.error(&localize(
            "ConfigurationParser.noTaskType",
            "Error: tasks configuration must have a type property. The configuration will be ignored.\n{0}\n",
            &[&pretty(external)],
        ));
        return None;
    }
    let type_declaration = task_type.and_then(|task_type| lookup_definition(registry, &js_to_string(task_type)));
    let Some(type_declaration) = type_declaration else {
        let message = localize_args(
            "ConfigurationParser.noTypeDefinition",
            "Error: there is no registered task type '{0}'. Did you miss installing an extension that provides a corresponding task provider?",
            &[match task_type {
                Some(task_type) => NlsArg::Str(js_to_string(task_type)),
                None => NlsArg::Undefined,
            }],
        );
        context.problem_reporter.error(&message);
        return None;
    };
    let mut identifier: Option<JsonObject> = None;
    if let Some(Value::String(customize)) = customize {
        let make = |kind: &str, key: &str, value: String| {
            let mut object = JsonObject::new();
            object.insert("type".to_string(), Value::from(kind));
            object.insert(key.to_string(), Value::from(value));
            object
        };
        if let Some(rest) = customize.strip_prefix("grunt.") {
            identifier = Some(make("grunt", "task", rest.to_string()));
        } else if let Some(rest) = customize.strip_prefix("jake.") {
            identifier = Some(make("jake", "task", rest.to_string()));
        } else if let Some(rest) = customize.strip_prefix("gulp.") {
            identifier = Some(make("gulp", "task", rest.to_string()));
        } else if customize.starts_with("vscode.npm.") {
            identifier = Some(make("npm", "script", substring_from(customize, "vscode.npm.".chars().count() + 4)));
        } else if customize.starts_with("vscode.typescript.") {
            identifier = Some(make("typescript", "tsconfig", substring_from(customize, "vscode.typescript.".chars().count() + 6)));
        }
    } else if external.get("type").is_some_and(Value::is_string)
        && let Value::Object(object) = external
    {
        identifier = Some(object.clone());
    }
    let Some(identifier) = identifier else {
        context.problem_reporter.error(&localize(
            "ConfigurationParser.missingType",
            "Error: the task configuration '{0}' is missing the required property 'type'. The task configuration will be ignored.",
            &[&compact(external)],
        ));
        return None;
    };
    let Some(task_identifier) = tasks::create_task_identifier(&identifier, context.problem_reporter.as_ref()) else {
        context.problem_reporter.error(&localize(
            "ConfigurationParser.incorrectType",
            "Error: the task configuration '{0}' is using an unknown type. The task configuration will be ignored.",
            &[&compact(external)],
        ));
        return None;
    };
    let config_element = TaskSourceConfigElement {
        workspace_folder: Some(context.workspace_folder.clone()),
        workspace: None,
        file: ".vscode/tasks.json".to_string(),
        index: index as i64,
        element: Some(external.clone()),
    };
    let task_source = file_based_source(source, config_element);
    let type_text = task_type.map(js_to_string);
    let mut result = ConfiguringTask::new(
        &format!("{}.{}", type_declaration.extension_id, task_identifier.key),
        task_source,
        None,
        type_text.as_deref(),
        task_identifier.clone(),
        run_options_from_configuration(external.get("runOptions")),
        ConfigurationProperties { hide: bool_field(external, "hide"), in_agents: bool_field(external, "inAgents"), ..Default::default() },
    );
    let configuration = configuration_properties_from(external, context, true, source, Some(&type_declaration.properties));
    result.add_task_load_messages(configuration.errors.as_deref());
    if let Some(value) = configuration.value {
        merge_configuration_properties(&mut result.common.configuration_properties, value);
        if let Some(name) = result.common.configuration_properties.name.clone().filter(|name| !name.is_empty()) {
            result.common.label = name;
        } else {
            let mut label = result.configures.task_type().unwrap_or("undefined").to_string();
            for required in &type_declaration.required {
                if let Some(value) = result.configures.get(required).filter(truthy) {
                    label = format!("{label}: {}", js_to_string(&value));
                    break;
                }
            }
            result.common.label = label;
        }
        if result.common.configuration_properties.identifier.as_deref().is_none_or(str::is_empty) {
            result.common.configuration_properties.identifier = Some(task_identifier.key.clone());
        }
    }
    Some(result)
}

/// `CustomTask.from`.
fn custom_task_from(external: &Value, context: &mut ParseContext, index: usize, source: TaskConfigSource) -> Option<CustomTask> {
    if !truthy(external) {
        return None;
    }
    let task_type = external.get("type").filter(|value| !value.is_null());
    let type_is_custom = match task_type {
        None => true,
        Some(Value::String(text)) => text == CUSTOMIZED_TASK_TYPE || text == "shell" || text == "process",
        Some(_) => false,
    };
    if !type_is_custom {
        context.problem_reporter.error(&localize(
            "ConfigurationParser.notCustom",
            "Error: tasks is not declared as a custom task. The configuration will be ignored.\n{0}\n",
            &[&pretty(external)],
        ));
        return None;
    }
    let mut task_name = external.get("taskName").filter(|value| truthy(value)).map(js_to_string);
    if let Some(label) = external.get("label").and_then(Value::as_str)
        && context.schema_version == JsonSchemaVersion::V2_0_0
    {
        task_name = Some(label.to_string()).filter(|label| !label.is_empty());
    }
    let Some(task_name) = task_name else {
        context.problem_reporter.error(&localize(
            "ConfigurationParser.noTaskName",
            "Error: a task must provide a label property. The task will be ignored.\n{0}\n",
            &[&pretty(external)],
        ));
        return None;
    };

    let workspace = if source == TaskConfigSource::WorkspaceFile { context.workspace.clone() } else { None };
    let task_source = file_based_source(
        source,
        TaskSourceConfigElement {
            workspace_folder: Some(context.workspace_folder.clone()),
            workspace,
            file: ".vscode/tasks.json".to_string(),
            index: index as i64,
            element: Some(external.clone()),
        },
    );

    let id = context.uuid_map.borrow_mut().get_uuid(&task_name);
    let mut result = CustomTask::new(
        &id,
        task_source,
        &task_name,
        CUSTOMIZED_TASK_TYPE,
        None,
        false,
        run_options_from_configuration(external.get("runOptions")),
        ConfigurationProperties { name: Some(task_name.clone()), identifier: Some(task_name.clone()), ..Default::default() },
    );
    let configuration = configuration_properties_from(external, context, false, source, None);
    result.add_task_load_messages(configuration.errors.as_deref());
    if let Some(value) = configuration.value {
        merge_configuration_properties(&mut result.common.configuration_properties, value);
    }
    // supportLegacy
    if result.common.configuration_properties.is_background.is_none()
        && let Some(is_watching) = external.get("isWatching")
    {
        result.common.configuration_properties.is_background = Some(truthy(is_watching));
    }
    if result.common.configuration_properties.group.is_none() {
        if external.get("isBuildCommand") == Some(&Value::Bool(true)) {
            result.common.configuration_properties.group = Some(GroupValue::Group(TaskGroup::build()));
        } else if external.get("isTestCommand") == Some(&Value::Bool(true)) {
            result.common.configuration_properties.group = Some(GroupValue::Group(TaskGroup::test()));
        }
    }
    let command = command_from(external, context);
    match command {
        Some(command) => result.command = command,
        None => {
            if external.get("command").is_some() {
                errors::throw(Error::type_error("Cannot set properties of undefined (setting 'suppressTaskName')"));
            }
        }
    }
    if external.get("command").is_some() {
        // if the task has its own command then we suppress the task name by default.
        result.command.suppress_task_name = Some(true);
    }
    Some(result)
}

/// `CustomTask.fillGlobals`.
fn custom_task_fill_globals(task: &mut CustomTask, globals: &Globals) {
    // We only merge a command from a global definition if there is no dependsOn or there is a dependsOn and a defined command.
    if command_has_command(&task.command) || task.common.configuration_properties.depends_on.is_none() {
        let name = task.common.configuration_properties.name.clone();
        task.command = command_fill_globals(std::mem::take(&mut task.command), globals.command.as_ref(), name.as_deref());
    }
    if task.common.configuration_properties.problem_matchers.is_none()
        && let Some(problem_matcher) = &globals.problem_matcher
    {
        task.common.configuration_properties.problem_matchers = Some(problem_matcher.iter().cloned().map(ProblemMatcherValue::Matcher).collect());
        task.has_defined_matchers = true;
    }
    // promptOnClose is inferred from isBackground if available
    if task.common.configuration_properties.prompt_on_close.is_none()
        && task.common.configuration_properties.is_background.is_none()
        && globals.prompt_on_close.is_some()
    {
        task.common.configuration_properties.prompt_on_close = globals.prompt_on_close;
    }
}

/// `CustomTask.fillDefaults`.
fn custom_task_fill_defaults(task: &mut CustomTask, context: &ParseContext) {
    command_fill_defaults(&mut task.command, context);
    let properties = &mut task.common.configuration_properties;
    if properties.prompt_on_close.is_none() {
        properties.prompt_on_close = Some(properties.is_background.map_or(true, |is_background| !is_background));
    }
    if properties.is_background.is_none() {
        properties.is_background = Some(false);
    }
    if properties.problem_matchers.is_none() {
        properties.problem_matchers = Some(Vec::new());
    }
}

/// O `configuredProps: ConfiguringTask | CustomTask` de `createCustomTask`.
#[derive(Clone, Copy)]
pub enum ConfiguredProps<'a> {
    Configuring(&'a ConfiguringTask),
    Custom(&'a CustomTask),
}

/// `createCustomTask` (e `CustomTask.createCustomTask`).
pub fn create_custom_task(contributed_task: &tasks::ContributedTask, configured_props: ConfiguredProps<'_>) -> CustomTask {
    let (configured_common, configured_source, configured_load_messages): (&tasks::CommonTaskData, &FileBasedTaskSource, Option<&[String]>) = match configured_props {
        ConfiguredProps::Configuring(task) => (&task.common, &task.source, task.task_load_messages()),
        ConfiguredProps::Custom(task) => (&task.common, &task.source, task.task_load_messages()),
    };
    let configured = &configured_common.configuration_properties;
    let contributed = &contributed_task.common.configuration_properties;
    let mut source = configured_source.clone();
    source.customizes = Some(contributed_task.defines.clone());
    let mut result = CustomTask::new(
        &configured_common.id,
        source,
        &or_text(configured.name.clone(), Some(contributed_task.common.label.clone())).unwrap_or_default(),
        CUSTOMIZED_TASK_TYPE,
        Some(contributed_task.command.clone()),
        false,
        contributed_task.common.run_options.clone(),
        ConfigurationProperties {
            name: or_text(configured.name.clone(), contributed.name.clone()),
            identifier: or_text(configured.identifier.clone(), contributed.identifier.clone()),
            icon: configured.icon.clone(),
            hide: configured.hide,
            in_agents: configured.in_agents,
            ..Default::default()
        },
    );
    result.add_task_load_messages(configured_load_messages);

    {
        let result_props = &mut result.common.configuration_properties;
        assign_fields!(result_props, configured, group, is_background, depends_on, problem_matchers, prompt_on_close, detail);
    }
    result.command.presentation = presentation_options_assign_properties(result.command.presentation.take(), configured.presentation.as_ref());
    result.command.options = command_options_assign_properties(result.command.options.take(), configured.options.clone());
    result.common.run_options = run_options_assign_properties(std::mem::take(&mut result.common.run_options), Some(&configured_common.run_options));

    {
        let result_props = &mut result.common.configuration_properties;
        fill_fields!(result_props, contributed, group, is_background, depends_on, problem_matchers, prompt_on_close, detail);
    }
    result.command.presentation = presentation_options_fill_properties(result.command.presentation.take(), contributed.presentation.as_ref());
    result.command.options = command_options_fill_properties(result.command.options.take(), contributed.options.as_ref());
    result.common.run_options = run_options_fill_properties(std::mem::take(&mut result.common.run_options), Some(&contributed_task.common.run_options));

    if contributed_task.has_defined_matchers {
        result.has_defined_matchers = true;
    }
    result
}

// ---------------------------------------------------------------------------------------------
// TaskParser
// ---------------------------------------------------------------------------------------------

/// `namespace TaskParser`.
pub mod task_parser {
    use super::*;

    fn is_custom_task(value: &Value) -> bool {
        let task_type = value.get("type");
        let customize = value.get("customize");
        customize.is_none()
            && match task_type {
                None | Some(Value::Null) => true,
                Some(Value::String(text)) => text == CUSTOMIZED_TASK_TYPE || text == "shell" || text == "process",
                Some(_) => false,
            }
    }

    struct DefaultTask {
        index: Option<usize>,
        rank: i32,
    }

    fn group_name(group: Option<&GroupValue>) -> Option<String> {
        match group? {
            GroupValue::Name(name) => Some(name.clone()),
            GroupValue::Group(group) => Some(group.id.clone()),
        }
    }

    /// `TaskParser.from`.
    pub fn from(
        externals: Option<&[Value]>,
        globals: &Globals,
        context: &mut ParseContext,
        source: TaskConfigSource,
        registry: Option<&dyn TaskDefinitionGetter>,
    ) -> TaskParseResult {
        let mut result = TaskParseResult::default();
        let Some(externals) = externals else {
            return result;
        };
        let mut default_build_task = DefaultTask { index: None, rank: -1 };
        let mut default_test_task = DefaultTask { index: None, rank: -1 };
        let schema2_0_0 = context.schema_version == JsonSchemaVersion::V2_0_0;
        let base_load_issues = context.task_load_issues.clone();
        for (index, external) in externals.iter().enumerate() {
            let external_type = external.get("type").filter(|value| truthy(value)).map(js_to_string);
            let definition = external_type.as_deref().and_then(|external_type| lookup_definition(registry, external_type));
            let mut type_not_supported = false;
            if let Some(definition) = &definition
                && let Some(when) = &definition.when
                && !context.context_key_service.context_matches_rules(Some(when))
            {
                type_not_supported = true;
            } else if definition.is_none()
                && let Some(external_type) = &external_type
                && (external_type == "shell" || external_type == "process")
            {
                let key_context = context.context_key_service.get_context(None);
                type_not_supported = !shell_execution_supported_context().expr().evaluate(key_context.as_ref());
            }

            if type_not_supported {
                context.problem_reporter.info(&localize(
                    "taskConfiguration.providerUnavailable",
                    "Warning: {0} tasks are unavailable in the current environment.\n",
                    &[external_type.as_deref().unwrap_or_default()],
                ));
                continue;
            }

            if is_custom_task(external) {
                if let Some(mut custom_task) = custom_task_from(external, context, index, source) {
                    custom_task_fill_globals(&mut custom_task, globals);
                    custom_task_fill_defaults(&mut custom_task, context);
                    let name = custom_task.common.configuration_properties.name.clone().unwrap_or_default();
                    if schema2_0_0 {
                        let no_command = custom_task.command.name.is_none();
                        let no_depends_on = custom_task.common.configuration_properties.depends_on.as_ref().is_none_or(|items| items.is_empty());
                        if no_command && no_depends_on {
                            context.problem_reporter.error(&localize(
                                "taskConfiguration.noCommandOrDependsOn",
                                "Error: the task '{0}' neither specifies a command nor a dependsOn property. The task will be ignored. Its definition is:\n{1}",
                                &[&name, &pretty(external)],
                            ));
                            continue;
                        }
                    } else if custom_task.command.name.is_none() {
                        context.problem_reporter.warn(&localize(
                            "taskConfiguration.noCommand",
                            "Error: the task '{0}' doesn't define a command. The task will be ignored. Its definition is:\n{1}",
                            &[&name, &pretty(external)],
                        ));
                        continue;
                    }
                    let group = custom_task.common.configuration_properties.group.as_ref();
                    let is_build = group.is_some_and(|group| group.is_same(&TaskGroup::build()));
                    let is_test = group.is_some_and(|group| group.is_same(&TaskGroup::test()));
                    let next_index = Some(result.custom.len());
                    if is_build && default_build_task.rank < 2 {
                        default_build_task = DefaultTask { index: next_index, rank: 2 };
                    } else if is_test && default_test_task.rank < 2 {
                        default_test_task = DefaultTask { index: next_index, rank: 2 };
                    } else if name == "build" && default_build_task.rank < 1 {
                        default_build_task = DefaultTask { index: next_index, rank: 1 };
                    } else if name == "test" && default_test_task.rank < 1 {
                        default_test_task = DefaultTask { index: next_index, rank: 1 };
                    }
                    let issues = context.task_load_issues.clone();
                    custom_task.add_task_load_messages(Some(&issues));
                    result.custom.push(custom_task);
                }
            } else if let Some(mut configured_task) = configuring_task_from(external, context, index, source, registry) {
                let issues = context.task_load_issues.clone();
                configured_task.add_task_load_messages(Some(&issues));
                result.configured.push(configured_task);
            }
            context.task_load_issues = base_load_issues.clone();
        }
        // There is some special logic for tasks with the labels "build" and "test".
        // Even if they are not marked as a task group Build or Test, we automagically group them as such.
        // However, if they are already grouped as Build or Test, we don't need to add this grouping.
        let default_build_group_name = default_build_task.index.and_then(|index| group_name(result.custom[index].common.configuration_properties.group.as_ref()));
        let default_test_group_name = default_test_task.index.and_then(|index| group_name(result.custom[index].common.configuration_properties.group.as_ref()));
        let build_id = TaskGroup::build().id.clone();
        let test_id = TaskGroup::test().id.clone();
        if default_build_group_name.as_deref() != Some(build_id.as_str())
            && default_build_task.rank > -1
            && default_build_task.rank < 2
            && let Some(index) = default_build_task.index
        {
            result.custom[index].common.configuration_properties.group = Some(GroupValue::Group(TaskGroup::build()));
        } else if default_test_group_name.as_deref() != Some(test_id.as_str())
            && default_test_task.rank > -1
            && default_test_task.rank < 2
            && let Some(index) = default_test_task.index
        {
            result.custom[index].common.configuration_properties.group = Some(GroupValue::Group(TaskGroup::test()));
        }

        result
    }

    /// `TaskParser.assignTasks`.
    pub fn assign_tasks(target: Vec<CustomTask>, source: Vec<CustomTask>) -> Vec<CustomTask> {
        if source.is_empty() {
            return target;
        }
        if target.is_empty() {
            return source;
        }
        // Tasks are keyed by ID but we need to merge by name
        let name_of = |task: &CustomTask| task.common.configuration_properties.name.clone().unwrap_or_default();
        let mut map: IndexMap<String, CustomTask> = IndexMap::new();
        for task in &target {
            map.insert(name_of(task), task.clone());
        }
        for task in source {
            map.insert(name_of(&task), task);
        }
        let mut new_target: Vec<CustomTask> = Vec::new();
        for task in &target {
            if let Some(merged) = map.shift_remove(&name_of(task)) {
                new_target.push(merged);
            }
        }
        new_target.extend(map.into_values());
        new_target
    }
}

// ---------------------------------------------------------------------------------------------
// Globals
// ---------------------------------------------------------------------------------------------

fn globals_from_base(config: &Value, context: &ParseContext) -> Globals {
    let mut result = Globals::default();
    if let Some(suppress) = config.get("suppressTaskName") {
        result.suppress_task_name = Some(truthy(suppress));
    }
    if let Some(prompt_on_close) = config.get("promptOnClose") {
        result.prompt_on_close = Some(truthy(prompt_on_close));
    }
    if let Some(problem_matcher) = truthy_field(config, "problemMatcher") {
        result.problem_matcher = problem_matcher_converter::from(Some(problem_matcher), context).value;
    }
    result
}

fn globals_is_empty(value: &Globals) -> bool {
    value.command.is_none() && value.prompt_on_close.is_none() && value.suppress_task_name.is_none()
}

fn globals_assign_properties(target: Globals, source: Globals) -> Globals {
    if globals_is_empty(&source) {
        return target;
    }
    if globals_is_empty(&target) {
        return source;
    }
    let mut target = target;
    assign_fields!(target, source, prompt_on_close, suppress_task_name);
    target
}

fn globals_fill_defaults(value: &mut Globals, context: &ParseContext) {
    if let Some(command) = value.command.as_mut() {
        command_fill_defaults(command, context);
    }
    if value.suppress_task_name.is_none() {
        value.suppress_task_name = Some(context.schema_version == JsonSchemaVersion::V2_0_0);
    }
    if value.prompt_on_close.is_none() {
        value.prompt_on_close = Some(true);
    }
}

/// `Globals.from`.
fn globals_from(config: &Value, context: &mut ParseContext) -> Globals {
    let mut result = globals_from_base(config, context);
    let mut os_globals: Option<Globals> = None;
    if let Some(windows) = truthy_field(config, "windows")
        && context.platform == PlatformKind::Windows
    {
        os_globals = Some(globals_from_base(windows, context));
    } else if let Some(osx) = truthy_field(config, "osx")
        && context.platform == PlatformKind::Mac
    {
        os_globals = Some(globals_from_base(osx, context));
    } else if let Some(linux) = truthy_field(config, "linux")
        && context.platform == PlatformKind::Linux
    {
        os_globals = Some(globals_from_base(linux, context));
    }
    if let Some(os_globals) = os_globals {
        result = globals_assign_properties(result, os_globals);
    }
    if let Some(command) = command_from(config, context) {
        result.command = Some(command);
    }
    globals_fill_defaults(&mut result, context);
    result
}

// ---------------------------------------------------------------------------------------------
// Motor de execução e versão do esquema
// ---------------------------------------------------------------------------------------------

/// `JsonSchemaVersion.from`.
pub fn json_schema_version_from(config: &Value) -> JsonSchemaVersion {
    match config.get("version").and_then(Value::as_str) {
        Some("0.1.0") => JsonSchemaVersion::V0_1_0,
        _ => JsonSchemaVersion::V2_0_0,
    }
}

/// `ExecutionEngine.from`.
pub fn execution_engine_from(config: &Value) -> ExecutionEngine {
    let runner = config.get("runner").filter(|value| truthy(value)).or_else(|| config.get("_runner")).and_then(Value::as_str);
    let result = match runner {
        Some("terminal") => Some(ExecutionEngine::Terminal),
        Some("process") => Some(ExecutionEngine::Process),
        _ => None,
    };
    match json_schema_version_from(config) {
        JsonSchemaVersion::V0_1_0 => result.unwrap_or(ExecutionEngine::Process),
        JsonSchemaVersion::V2_0_0 => ExecutionEngine::Terminal,
    }
}

// ---------------------------------------------------------------------------------------------
// ConfigurationParser
// ---------------------------------------------------------------------------------------------

fn tasks_array(section: &Value) -> Option<&[Value]> {
    section.get("tasks").and_then(Value::as_array).map(Vec::as_slice)
}

fn create_task_runner_configuration(file_config: &Value, context: &mut ParseContext, source: TaskConfigSource) -> TaskParseResult {
    let globals = globals_from(file_config, context);
    if context.problem_reporter.status().is_fatal() {
        return TaskParseResult::default();
    }
    context.named_problem_matchers = problem_matcher_converter::named_from(file_config.get("declares"), context);
    let mut global_tasks: Option<Vec<CustomTask>> = None;
    let mut external_global_tasks: Option<&[Value]> = None;
    let platform = context.platform;
    let section = |name: &str| truthy_field(file_config, name);
    if let Some(windows) = section("windows").filter(|_| platform == PlatformKind::Windows) {
        global_tasks = Some(task_parser::from(tasks_array(windows), &globals, context, source, None).custom);
        external_global_tasks = tasks_array(windows);
    } else if let Some(osx) = section("osx").filter(|_| platform == PlatformKind::Mac) {
        global_tasks = Some(task_parser::from(tasks_array(osx), &globals, context, source, None).custom);
        external_global_tasks = tasks_array(osx);
    } else if let Some(linux) = section("linux").filter(|_| platform == PlatformKind::Linux) {
        global_tasks = Some(task_parser::from(tasks_array(linux), &globals, context, source, None).custom);
        external_global_tasks = tasks_array(linux);
    }
    if context.schema_version == JsonSchemaVersion::V2_0_0
        && global_tasks.as_ref().is_some_and(|tasks| !tasks.is_empty())
        && external_global_tasks.is_some_and(|tasks| !tasks.is_empty())
    {
        let task_content: Vec<String> = external_global_tasks.unwrap_or_default().iter().map(pretty).collect();
        context.problem_reporter.error(&localize(
            "TaskParse.noOsSpecificGlobalTasks",
            "Task version 2.0.0 doesn't support global OS specific tasks. Convert them to a task with a OS specific command. Affected tasks are:\n{0}",
            &[&task_content.join("\n")],
        ));
    }

    let mut result = TaskParseResult::default();
    if let Some(tasks) = file_config.get("tasks").filter(|tasks| truthy(tasks)) {
        result = task_parser::from(tasks.as_array().map(Vec::as_slice), &globals, context, source, None);
    }
    if let Some(global_tasks) = global_tasks {
        result.custom = task_parser::assign_tasks(std::mem::take(&mut result.custom), global_tasks);
    }

    if result.custom.is_empty()
        && let Some(global_command) = &globals.command
        && global_command.name.is_some_and_truthy()
    {
        let matchers: Vec<ProblemMatcher> = problem_matcher_converter::from(file_config.get("problemMatcher"), context).value.unwrap_or_default();
        let is_background = if file_config.get("isBackground").is_some_and(truthy) {
            Some(true)
        } else if file_config.get("isWatching").is_some_and(truthy) {
            Some(true)
        } else {
            None
        };
        let name = global_command.name.as_ref().map(|name| name.value().to_string()).unwrap_or_default();
        let id = context.uuid_map.borrow_mut().get_uuid(&name);
        let mut task = CustomTask::new(
            &id,
            FileBasedTaskSource {
                kind: None,
                label: String::new(),
                config: TaskSourceConfigElement {
                    workspace_folder: Some(context.workspace_folder.clone()),
                    workspace: None,
                    file: String::new(),
                    index: -1,
                    element: Some(file_config.clone()),
                },
                customizes: None,
            },
            &name,
            CUSTOMIZED_TASK_TYPE,
            Some(CommandConfiguration { suppress_task_name: Some(true), ..Default::default() }),
            false,
            RunOptions { reevaluate_on_rerun: Some(true), ..Default::default() },
            ConfigurationProperties {
                name: Some(name.clone()),
                identifier: Some(name.clone()),
                group: Some(GroupValue::Group(TaskGroup::build())),
                is_background,
                problem_matchers: Some(matchers.into_iter().map(ProblemMatcherValue::Matcher).collect()),
                ..Default::default()
            },
        );
        let task_group_kind = group_kind::from(file_config.get("group"));
        if task_group_kind.is_some() {
            task.common.configuration_properties.group = task_group_kind;
        } else if file_config.get("group").and_then(Value::as_str) == Some("none") {
            task.common.configuration_properties.group = None;
        }
        custom_task_fill_globals(&mut task, &globals);
        custom_task_fill_defaults(&mut task, context);
        result.custom = vec![task];
    }
    result
}

/// `!!command.name` sobre o `Option<CommandString>`.
trait IsSomeAndTruthy {
    fn is_some_and_truthy(&self) -> bool;
}

impl IsSomeAndTruthy for Option<CommandString> {
    fn is_some_and_truthy(&self) -> bool {
        match self {
            None => false,
            Some(CommandString::Plain(name)) => !name.is_empty(),
            Some(CommandString::Quoted(_)) => true,
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn run_configuration_parser(
    workspace_folder: &WorkspaceFolder,
    workspace: Option<Rc<dyn IWorkspace>>,
    platform: PlatformKind,
    problem_reporter: Rc<dyn ProblemReporter>,
    uuid_map: Rc<RefCell<UuidMap>>,
    file_config: &Value,
    source: TaskConfigSource,
    context_key_service: Rc<dyn IContextKeyService>,
) -> ParseResult {
    let engine = execution_engine_from(file_config);
    let schema_version = json_schema_version_from(file_config);
    let mut context = ParseContext {
        workspace_folder: workspace_folder.clone(),
        workspace,
        problem_reporter: problem_reporter.clone(),
        uuid_map,
        named_problem_matchers: IndexMap::new(),
        engine,
        schema_version,
        platform,
        task_load_issues: Vec::new(),
        context_key_service,
    };
    let task_parse_result = create_task_runner_configuration(file_config, &mut context, source);
    let validation_status = ValidationStatus::new();
    validation_status.set_state(problem_reporter.status().state());
    ParseResult { validation_status, custom: task_parse_result.custom, configured: task_parse_result.configured, engine }
}

type UuidMaps = HashMap<TaskConfigSource, HashMap<String, Rc<RefCell<UuidMap>>>>;

thread_local! {
    static UUID_MAPS: RefCell<UuidMaps> = RefCell::new(HashMap::new());
    static RECENT_UUID_MAPS: RefCell<UuidMaps> = RefCell::new(HashMap::new());
}

/// O `try { ... } finally { uuidMap.finish(); }` de `parse`.
struct FinishOnDrop(Rc<RefCell<UuidMap>>);

impl Drop for FinishOnDrop {
    fn drop(&mut self) {
        self.0.borrow_mut().finish();
    }
}

/// `parse`.
#[allow(clippy::too_many_arguments)]
pub fn parse(
    workspace_folder: &WorkspaceFolder,
    workspace: Option<Rc<dyn IWorkspace>>,
    platform: PlatformKind,
    configuration: &Value,
    logger: Rc<dyn ProblemReporter>,
    source: TaskConfigSource,
    context_key_service: Rc<dyn IContextKeyService>,
    is_recents: bool,
) -> ParseResult {
    let key = workspace_folder.uri.to_string(false).to_string_lossy();
    let store = if is_recents { &RECENT_UUID_MAPS } else { &UUID_MAPS };
    let uuid_map = store.with(|maps| {
        let mut maps = maps.borrow_mut();
        let selected = maps.entry(source).or_default();
        selected.entry(key).or_insert_with(|| Rc::new(RefCell::new(UuidMap::new(None)))).clone()
    });
    uuid_map.borrow_mut().start();
    let _finish = FinishOnDrop(uuid_map.clone());
    run_configuration_parser(workspace_folder, workspace, platform, logger, uuid_map, configuration, source, context_key_service)
}
