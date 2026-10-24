//! Porte de `src/vs/workbench/contrib/tasks/test/common/taskConfiguration.test.ts` (suítes `Tasks version 0.1.0`,
//! `Tasks version 2.0.0`, `Bugs / regression tests` e `Task configuration conversions`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` vira o guarda criado no começo de cada caso.
//! - Os literais de `tasks.json` de cada caso são `json!` com o mesmo conteúdo (a configuração externa é um
//!   `serde_json::Value`). Os `as ICustomTask` desaparecem.
//! - Os construtores (`ConfigurationBuilder`, `CustomTaskBuilder`, `CommandConfigurationBuilder`,
//!   `PresentationBuilder`, `ProblemMatcherBuilder`, `PatternBuilder`) compartilham a tarefa esperada por
//!   `Rc<RefCell<..>>`, porque o original os encadeia por referência (`.parent`); cada método devolve um novo
//!   manipulador do mesmo estado. `parent` é o método `parent()`. O padrão `pattern(regExp)` só liga ao casador o
//!   primeiro construtor, como no original.
//! - `TasksMockContextKeyService` é o `ContextKeyService` de `je-platform` (sobre o `TestConfigurationService`) com
//!   `getContext` sobrescrito para devolver sempre `true`.
//! - `ProblemReporter` do teste só registra a mensagem; o estado de validação nunca sobe, como no original.
//! - `Platform.platform` é `host_platform().kind`; `Platform.isWindows` e companhia são os campos de
//!   `host_platform()`.
//! - `(Platform.isLinux ? test.skip : test)('Bug 19548')` é `#[cfg_attr(target_os = "linux", ignore)]`; os
//!   `test.skip` são `#[ignore]`. `taskGroup.isDefault = true` (nos casos ignorados) altera no original a constante
//!   compartilhada, o que o porte não permite: nesses casos o grupo esperado é uma cópia com `isDefault` verdadeiro.
//! - `class TestParseContext` (um objeto vazio com campos atribuídos) é um `ParseContext` completo: o que o
//!   original deixa `undefined` (`engine`, `schemaVersion`, `platform`) assume o valor que as conferências
//!   `=== Terminal`, `=== V2_0_0` e `=== Platform.X` tratam do mesmo modo (`Process`, `V0_1_0`, `Web`).
//! - `INamedProblemMatcher` do teste (um objeto só com `name` e `label`) é um `ProblemMatcher` com campos
//!   padrão; como `applyTo` não é opcional no porte, `assert.deepEqual(result.value, [{ label }])` vira a conferência
//!   de `label` e `name`, e o caso com `applyTo` confere `apply_to`.
//! - `assert.deepEqual(result.value, [])` é `value == Some([])`; `result.errors?.length` é o tamanho de `errors`.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use std::cell::RefCell;
use std::rc::Rc;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use indexmap::IndexMap;
use je_base::arrays::OneOrMany;
use je_base::dynamic::Dynamic;
use je_base::event::Event;
use je_base::js_regexp::JsRegExp;
use je_base::parsers::{ProblemReporter, ValidationStatus};
use je_base::severity::Severity;
use je_base::uuid::{generate_uuid, is_uuid};
use je_platform::configuration::test_configuration_service::TestConfigurationService;
use je_platform::contextkey::service::ContextKeyService;
use je_platform::contextkey::{
    ContextKeyExpression, IContext, IContextKey, IContextKeyChangeEvent, IContextKeyService, IContextKeyServiceTarget, IScopedContextKeyService,
};
use je_platform::resources::{host_platform, uri_file};
use je_platform::workspace::{IWorkspace, WorkspaceFolder, new_test_workspace};
use je_workbench::contrib::tasks::problem_matcher::{ApplyToKind, FileLocationKind, FilePrefix, ProblemMatcher, ProblemPattern};
use je_workbench::contrib::tasks::task_configuration::{
    Globals, ParseContext, ParseResult, TaskConfigSource, TaskParseResult, UuidMap, parse, problem_matcher_converter, task_parser,
};
use je_workbench::contrib::tasks::task_definition_registry::TaskDefinitionGetter;
use je_workbench::contrib::tasks::tasks::{
    self as tasks, CUSTOMIZED_TASK_TYPE, CommandConfiguration, CommandOptions, CommandString, ConfigurationProperties, CustomTask, ExecutionEngine,
    FileBasedTaskSource, GroupValue, IsDefault, JsonSchemaVersion, PanelKind, PresentationOptions, ProblemMatcherValue, RevealKind, RevealProblemKind,
    RunOptions, RuntimeType, TaskDefinition, TaskGroup, TaskSourceConfigElement, TaskSourceKind,
};
use serde_json::{Value, json};

fn workspace_folder() -> WorkspaceFolder {
    WorkspaceFolder::new(uri_file("/workspace/folderOne"), "folderOne", 0, None)
}

fn workspace() -> Rc<dyn IWorkspace> {
    new_test_workspace("id", vec![workspace_folder()], None, None)
}

struct ProblemReporterDouble {
    validation_status: ValidationStatus,
    received_message: RefCell<bool>,
    last_message: RefCell<Option<String>>,
}

impl ProblemReporterDouble {
    fn new() -> Rc<Self> {
        Rc::new(Self { validation_status: ValidationStatus::new(), received_message: RefCell::new(false), last_message: RefCell::new(None) })
    }

    fn log(&self, message: &str) {
        *self.received_message.borrow_mut() = true;
        *self.last_message.borrow_mut() = Some(message.to_string());
    }

    fn received_message(&self) -> bool {
        *self.received_message.borrow()
    }

    fn last_message(&self) -> Option<String> {
        self.last_message.borrow().clone()
    }

    fn clear_message(&self) {
        *self.last_message.borrow_mut() = None;
    }
}

impl ProblemReporter for ProblemReporterDouble {
    fn info(&self, message: &str) {
        self.log(message);
    }

    fn warn(&self, message: &str) {
        self.log(message);
    }

    fn error(&self, message: &str) {
        self.log(message);
    }

    fn fatal(&self, message: &str) {
        self.log(message);
    }

    fn status(&self) -> &ValidationStatus {
        &self.validation_status
    }
}

// ---------------------------------------------------------------------------------------------
// Construtores
// ---------------------------------------------------------------------------------------------

thread_local! {
    static DEFAULT_UUID: String = generate_uuid();
}

fn default_uuid() -> String {
    DEFAULT_UUID.with(Clone::clone)
}

type SharedTask = Rc<RefCell<CustomTask>>;

struct ConfigurationBuilder {
    builders: Vec<SharedTask>,
}

impl ConfigurationBuilder {
    fn new() -> Self {
        Self { builders: Vec::new() }
    }

    fn task(&mut self, name: &str, command: &str) -> CustomTaskBuilder {
        let command_configuration = CommandConfiguration {
            name: Some(CommandString::Plain(command.to_string())),
            runtime: Some(RuntimeType::Process),
            args: Some(Vec::new()),
            options: Some(CommandOptions { cwd: Some("${workspaceFolder}".to_string()), ..Default::default() }),
            presentation: Some(PresentationOptions {
                echo: Some(false),
                reveal: Some(RevealKind::Always),
                reveal_problems: Some(RevealProblemKind::Never),
                focus: Some(false),
                panel: Some(PanelKind::Shared),
                show_reuse_message: Some(true),
                clear: Some(false),
                close: Some(false),
                ..Default::default()
            }),
            suppress_task_name: Some(false),
            ..Default::default()
        };
        let task = CustomTask::new(
            name,
            FileBasedTaskSource {
                kind: Some(TaskSourceKind::Workspace),
                label: "workspace".to_string(),
                config: TaskSourceConfigElement {
                    workspace_folder: Some(workspace_folder()),
                    workspace: None,
                    file: ".vscode/tasks.json".to_string(),
                    index: -1,
                    element: None,
                },
                customizes: None,
            },
            name,
            CUSTOMIZED_TASK_TYPE,
            Some(command_configuration),
            false,
            RunOptions { reevaluate_on_rerun: Some(true), ..Default::default() },
            ConfigurationProperties {
                identifier: Some(name.to_string()),
                name: Some(name.to_string()),
                is_background: Some(false),
                prompt_on_close: Some(true),
                problem_matchers: Some(Vec::new()),
                ..Default::default()
            },
        );
        let shared = Rc::new(RefCell::new(task));
        self.builders.push(shared.clone());
        CustomTaskBuilder { task: shared }
    }

    fn done(&self) {
        for builder in &self.builders {
            let mut task = builder.borrow_mut();
            let task_name = task.common.configuration_properties.name.clone().unwrap_or_default();
            let args = task.command.args.take().unwrap_or_default();
            task.command.args = Some(
                args.into_iter()
                    .map(|arg| if arg.value() == "$name" { CommandString::Plain(task_name.clone()) } else { arg })
                    .collect(),
            );
        }
    }

    fn result(&self) -> Vec<CustomTask> {
        self.builders.iter().map(|builder| builder.borrow().clone()).collect()
    }
}

#[derive(Clone)]
struct CustomTaskBuilder {
    task: SharedTask,
}

impl CustomTaskBuilder {
    fn identifier(&self, value: &str) -> CustomTaskBuilder {
        self.task.borrow_mut().common.configuration_properties.identifier = Some(value.to_string());
        self.clone()
    }

    fn group(&self, value: GroupValue) -> CustomTaskBuilder {
        self.task.borrow_mut().common.configuration_properties.group = Some(value);
        self.clone()
    }

    fn is_background(&self, value: bool) -> CustomTaskBuilder {
        self.task.borrow_mut().common.configuration_properties.is_background = Some(value);
        self.clone()
    }

    fn prompt_on_close(&self, value: bool) -> CustomTaskBuilder {
        self.task.borrow_mut().common.configuration_properties.prompt_on_close = Some(value);
        self.clone()
    }

    fn problem_matcher(&self) -> ProblemMatcherBuilder {
        let matcher = ProblemMatcher {
            severity: None,
            file_prefix: Some(FilePrefix::Path("${workspaceFolder}".to_string())),
            ..ProblemMatcher::new(default_uuid(), ApplyToKind::AllDocuments, FileLocationKind::Relative, OneOrMany::Many(Vec::new()))
        };
        let mut task = self.task.borrow_mut();
        let matchers = task.common.configuration_properties.problem_matchers.get_or_insert_with(Vec::new);
        matchers.push(ProblemMatcherValue::Matcher(matcher));
        ProblemMatcherBuilder { task: self.task.clone(), index: matchers.len() - 1 }
    }

    fn command(&self) -> CommandConfigurationBuilder {
        CommandConfigurationBuilder { task: self.task.clone() }
    }
}

#[derive(Clone)]
struct CommandConfigurationBuilder {
    task: SharedTask,
}

impl CommandConfigurationBuilder {
    fn name(&self, value: &str) -> CommandConfigurationBuilder {
        self.task.borrow_mut().command.name = Some(CommandString::Plain(value.to_string()));
        self.clone()
    }

    fn runtime(&self, value: RuntimeType) -> CommandConfigurationBuilder {
        self.task.borrow_mut().command.runtime = Some(value);
        self.clone()
    }

    fn args(&self, value: &[&str]) -> CommandConfigurationBuilder {
        self.task.borrow_mut().command.args = Some(value.iter().map(|arg| CommandString::Plain((*arg).to_string())).collect());
        self.clone()
    }

    fn options(&self, value: CommandOptions) -> CommandConfigurationBuilder {
        self.task.borrow_mut().command.options = Some(value);
        self.clone()
    }

    fn task_selector(&self, value: &str) -> CommandConfigurationBuilder {
        self.task.borrow_mut().command.task_selector = Some(value.to_string());
        self.clone()
    }

    fn suppress_task_name(&self, value: bool) -> CommandConfigurationBuilder {
        self.task.borrow_mut().command.suppress_task_name = Some(value);
        self.clone()
    }

    fn presentation(&self) -> PresentationBuilder {
        PresentationBuilder { task: self.task.clone() }
    }

    fn parent(&self) -> CustomTaskBuilder {
        CustomTaskBuilder { task: self.task.clone() }
    }
}

#[derive(Clone)]
struct PresentationBuilder {
    task: SharedTask,
}

impl PresentationBuilder {
    fn with(&self, apply: impl FnOnce(&mut PresentationOptions)) -> PresentationBuilder {
        let mut task = self.task.borrow_mut();
        apply(task.command.presentation.get_or_insert_with(PresentationOptions::default));
        self.clone()
    }

    fn echo(&self, value: bool) -> PresentationBuilder {
        self.with(|presentation| presentation.echo = Some(value))
    }

    fn reveal(&self, value: RevealKind) -> PresentationBuilder {
        self.with(|presentation| presentation.reveal = Some(value))
    }

    #[allow(dead_code)]
    fn focus(&self, value: bool) -> PresentationBuilder {
        self.with(|presentation| presentation.focus = Some(value))
    }

    #[allow(dead_code)]
    fn instance(&self, value: PanelKind) -> PresentationBuilder {
        self.with(|presentation| presentation.panel = Some(value))
    }

    #[allow(dead_code)]
    fn show_reuse_message(&self, value: bool) -> PresentationBuilder {
        self.with(|presentation| presentation.show_reuse_message = Some(value))
    }

    #[allow(dead_code)]
    fn close(&self, value: bool) -> PresentationBuilder {
        self.with(|presentation| presentation.close = Some(value))
    }
}

#[derive(Clone)]
struct ProblemMatcherBuilder {
    task: SharedTask,
    index: usize,
}

impl ProblemMatcherBuilder {
    fn with(&self, apply: impl FnOnce(&mut ProblemMatcher)) -> ProblemMatcherBuilder {
        let mut task = self.task.borrow_mut();
        if let Some(matchers) = task.common.configuration_properties.problem_matchers.as_mut()
            && let Some(ProblemMatcherValue::Matcher(matcher)) = matchers.get_mut(self.index)
        {
            apply(matcher);
        }
        self.clone()
    }

    fn owner(&self, value: &str) -> ProblemMatcherBuilder {
        self.with(|matcher| matcher.owner = value.to_string())
    }

    fn apply_to(&self, value: ApplyToKind) -> ProblemMatcherBuilder {
        self.with(|matcher| matcher.apply_to = value)
    }

    fn severity(&self, value: Severity) -> ProblemMatcherBuilder {
        self.with(|matcher| matcher.severity = Some(value))
    }

    fn file_location(&self, value: FileLocationKind) -> ProblemMatcherBuilder {
        self.with(|matcher| matcher.file_location = value)
    }

    fn file_prefix(&self, value: Option<&str>) -> ProblemMatcherBuilder {
        self.with(|matcher| matcher.file_prefix = value.map(|prefix| FilePrefix::Path(prefix.to_string())))
    }

    fn pattern(&self, reg_exp: &str) -> PatternBuilder {
        let pattern = ProblemPattern {
            file: Some(1),
            message: Some(0),
            line: Some(2),
            character: Some(3),
            ..ProblemPattern::new(Rc::new(JsRegExp::from_str(reg_exp, "").expect("expressão regular válida")))
        };
        let mut first = false;
        self.with(|matcher| {
            if matches!(&matcher.pattern, OneOrMany::Many(items) if items.is_empty()) {
                matcher.pattern = OneOrMany::One(pattern);
                first = true;
            }
        });
        PatternBuilder { parent: self.clone(), first }
    }
}

#[derive(Clone)]
struct PatternBuilder {
    parent: ProblemMatcherBuilder,
    first: bool,
}

impl PatternBuilder {
    fn with(&self, apply: impl FnOnce(&mut ProblemPattern)) -> PatternBuilder {
        if self.first {
            self.parent.with(|matcher| {
                if let OneOrMany::One(pattern) = &mut matcher.pattern {
                    apply(pattern);
                }
            });
        }
        self.clone()
    }

    fn file(&self, value: usize) -> PatternBuilder {
        self.with(|pattern| pattern.file = Some(value))
    }

    fn message(&self, value: usize) -> PatternBuilder {
        self.with(|pattern| pattern.message = Some(value))
    }

    fn location(&self, value: usize) -> PatternBuilder {
        self.with(|pattern| pattern.location = Some(value))
    }

    fn line(&self, value: usize) -> PatternBuilder {
        self.with(|pattern| pattern.line = Some(value))
    }

    fn character(&self, value: usize) -> PatternBuilder {
        self.with(|pattern| pattern.character = Some(value))
    }

    fn end_line(&self, value: usize) -> PatternBuilder {
        self.with(|pattern| pattern.end_line = Some(value))
    }

    fn end_character(&self, value: usize) -> PatternBuilder {
        self.with(|pattern| pattern.end_character = Some(value))
    }

    fn code(&self, value: usize) -> PatternBuilder {
        self.with(|pattern| pattern.code = Some(value))
    }

    fn severity(&self, value: usize) -> PatternBuilder {
        self.with(|pattern| pattern.severity = Some(value))
    }

    #[allow(dead_code)]
    fn r#loop(&self, value: bool) -> PatternBuilder {
        self.with(|pattern| pattern.r#loop = Some(value))
    }
}

// ---------------------------------------------------------------------------------------------
// Serviço de chaves de contexto
// ---------------------------------------------------------------------------------------------

struct AlwaysTrueContext;

impl IContext for AlwaysTrueContext {
    fn get_value(&self, _key: &str) -> Dynamic {
        Dynamic::Bool(true)
    }
}

struct TasksMockContextKeyService {
    inner: Rc<dyn IContextKeyService>,
}

impl TasksMockContextKeyService {
    fn new() -> Rc<Self> {
        let inner: Rc<dyn IContextKeyService> = ContextKeyService::new(TestConfigurationService::new(None));
        Rc::new(Self { inner })
    }
}

impl IContextKeyService for TasksMockContextKeyService {
    fn on_did_change_context(&self) -> Event<Rc<dyn IContextKeyChangeEvent>> {
        self.inner.on_did_change_context()
    }

    fn buffer_change_events(&self, callback: &dyn Fn()) {
        self.inner.buffer_change_events(callback)
    }

    fn create_key(&self, key: &str, default_value: Dynamic) -> Rc<dyn IContextKey> {
        self.inner.create_key(key, default_value)
    }

    fn context_matches_rules(&self, rules: Option<&ContextKeyExpression>) -> bool {
        self.inner.context_matches_rules(rules)
    }

    fn get_context_key_value(&self, key: &str) -> Dynamic {
        self.inner.get_context_key_value(key)
    }

    fn create_scoped(&self, target: Rc<dyn IContextKeyServiceTarget>) -> Rc<dyn IScopedContextKeyService> {
        self.inner.create_scoped(target)
    }

    fn create_overlay(&self, overlay: Vec<(String, Dynamic)>) -> Rc<dyn IContextKeyService> {
        self.inner.create_overlay(overlay)
    }

    fn get_context(&self, _target: Option<Rc<dyn IContextKeyServiceTarget>>) -> Rc<dyn IContext> {
        Rc::new(AlwaysTrueContext)
    }

    fn update_parent(&self, parent_context_key_service: Rc<dyn IContextKeyService>) {
        self.inner.update_parent(parent_context_key_service)
    }

    fn context_id(&self) -> i64 {
        self.inner.context_id()
    }

    fn get_context_values_container(&self, context_id: i64) -> Rc<dyn IContext> {
        self.inner.get_context_values_container(context_id)
    }
}

// ---------------------------------------------------------------------------------------------
// Conferências
// ---------------------------------------------------------------------------------------------

fn parse_external(external: &Value, reporter: &Rc<ProblemReporterDouble>) -> ParseResult {
    let logger: Rc<dyn ProblemReporter> = reporter.clone();
    parse(
        &workspace_folder(),
        Some(workspace()),
        host_platform().kind,
        external,
        logger,
        TaskConfigSource::TasksJson,
        TasksMockContextKeyService::new(),
        false,
    )
}

fn test_default_problem_matcher(external: Value, resolved: usize) {
    let reporter = ProblemReporterDouble::new();
    let result = parse_external(&external, &reporter);
    assert!(!reporter.received_message());
    assert_eq!(result.custom.len(), 1);
    let task = &result.custom[0];
    assert_eq!(task.common.configuration_properties.problem_matchers.as_ref().map(Vec::len), Some(resolved));
}

fn test_configuration(external: Value, builder: &ConfigurationBuilder) {
    builder.done();
    let reporter = ProblemReporterDouble::new();
    let result = parse_external(&external, &reporter);
    if reporter.received_message() {
        panic!("{}", reporter.last_message().unwrap_or_default());
    }
    assert_configuration(&result, &builder.result());
}

#[derive(Default)]
struct TaskGroupMap {
    store: IndexMap<String, Vec<String>>,
}

impl TaskGroupMap {
    fn add(&mut self, group: &str, task: &CustomTask) {
        self.store.entry(group.to_string()).or_default().push(task.common.configuration_properties.name.clone().unwrap_or_default());
    }

    fn assert(actual: &TaskGroupMap, expected: &TaskGroupMap) {
        if actual.store.is_empty() && expected.store.is_empty() {
            return;
        }
        assert_eq!(actual.store.len(), expected.store.len());
        for key in actual.store.keys() {
            assert!(expected.store.contains_key(key));
        }
        for (key, actual_tasks) in &actual.store {
            let expected_tasks = &expected.store[key];
            assert_eq!(actual_tasks.len(), expected_tasks.len());
            if actual_tasks.len() == 1 {
                assert_eq!(actual_tasks[0], expected_tasks[0]);
                continue;
            }
            let mut expected_task_map: Vec<&String> = expected_tasks.iter().collect();
            for task in actual_tasks {
                expected_task_map.retain(|name| *name != task);
            }
            assert_eq!(expected_task_map.len(), 0);
        }
    }
}

fn group_id(task: &CustomTask) -> Option<String> {
    TaskGroup::from(task.common.configuration_properties.group.as_ref()).map(|group| group.id.clone())
}

fn assert_configuration(result: &ParseResult, expected: &[CustomTask]) {
    assert!(result.validation_status.is_ok());
    let actual = &result.custom;

    // We can't compare Ids since the parser uses UUID which are random
    // So create a new map using the name.
    let mut actual_tasks: IndexMap<String, &CustomTask> = IndexMap::new();
    let mut actual_task_groups = TaskGroupMap::default();
    for task in actual {
        let name = task.common.configuration_properties.name.clone().unwrap_or_default();
        assert!(!actual_tasks.contains_key(&name));
        actual_tasks.insert(name, task);
        if let Some(task_id) = group_id(task) {
            actual_task_groups.add(&task_id, task);
        }
    }
    let mut expected_tasks: IndexMap<String, &CustomTask> = IndexMap::new();
    let mut expected_task_group = TaskGroupMap::default();
    for task in expected {
        let name = task.common.configuration_properties.name.clone().unwrap_or_default();
        assert!(!expected_tasks.contains_key(&name));
        expected_tasks.insert(name, task);
        if let Some(task_id) = group_id(task) {
            expected_task_group.add(&task_id, task);
        }
    }
    assert_eq!(actual_tasks.len(), expected.len());
    for (key, actual_task) in &actual_tasks {
        let expected_task = expected_tasks.get(key).expect("tarefa esperada");
        assert_task(actual_task, expected_task);
    }
    TaskGroupMap::assert(&actual_task_groups, &expected_task_group);
}

fn assert_task(actual: &CustomTask, expected: &CustomTask) {
    assert!(!actual.common.id.is_empty());
    let actual_props = &actual.common.configuration_properties;
    let expected_props = &expected.common.configuration_properties;
    assert_eq!(actual_props.name, expected_props.name, "name");
    assert_command_configuration(&actual.command, &expected.command);
    assert_eq!(actual_props.is_background, expected_props.is_background, "isBackground");
    assert_eq!(actual_props.problem_matchers.is_some(), expected_props.problem_matchers.is_some());
    assert_eq!(actual_props.prompt_on_close, expected_props.prompt_on_close, "promptOnClose");
    assert_eq!(actual_props.group.is_some(), expected_props.group.is_some(), "group types unequal");

    if let (Some(actual_matchers), Some(expected_matchers)) = (&actual_props.problem_matchers, &expected_props.problem_matchers) {
        assert_eq!(actual_matchers.len(), expected_matchers.len());
        for (actual_matcher, expected_matcher) in actual_matchers.iter().zip(expected_matchers) {
            assert_problem_matcher(actual_matcher, expected_matcher);
        }
    }

    if let (Some(actual_group), Some(expected_group)) = (&actual_props.group, &expected_props.group) {
        match (actual_group, expected_group) {
            (GroupValue::Name(actual_name), GroupValue::Name(expected_name)) => assert_eq!(actual_name, expected_name),
            (GroupValue::Group(actual_group), GroupValue::Group(expected_group)) => assert_group(actual_group, expected_group),
            _ => panic!("os tipos de grupo diferem"),
        }
    }
}

fn assert_command_configuration(actual: &CommandConfiguration, expected: &CommandConfiguration) {
    assert_presentation(actual.presentation.as_ref(), expected.presentation.as_ref());
    assert_eq!(actual.name, expected.name, "name");
    assert_eq!(actual.runtime, expected.runtime, "runtime type");
    assert_eq!(actual.suppress_task_name, expected.suppress_task_name, "suppressTaskName");
    assert_eq!(actual.task_selector, expected.task_selector, "taskSelector");
    assert_eq!(actual.args, expected.args, "args");
    assert_eq!(actual.options.is_some(), expected.options.is_some());
    if let (Some(actual_options), Some(expected_options)) = (&actual.options, &expected.options) {
        assert_eq!(actual_options.cwd, expected_options.cwd, "cwd");
        assert_eq!(actual_options.env.is_some(), expected_options.env.is_some(), "env");
        if let (Some(actual_env), Some(expected_env)) = (&actual_options.env, &expected_options.env) {
            assert_eq!(actual_env, expected_env, "env");
        }
    }
}

fn assert_group(actual: &TaskGroup, expected: &TaskGroup) {
    assert_eq!(actual.id, expected.id, "group ids unequal. actual: {} expected {}", actual.id, expected.id);
    assert_eq!(actual.is_default, expected.is_default, "group defaults unequal. actual: {:?} expected {:?}", actual.is_default, expected.is_default);
}

fn assert_presentation(actual: Option<&PresentationOptions>, expected: Option<&PresentationOptions>) {
    assert_eq!(actual.is_some(), expected.is_some());
    if let (Some(actual), Some(expected)) = (actual, expected) {
        assert_eq!(actual.echo, expected.echo);
        assert_eq!(actual.reveal, expected.reveal);
    }
}

fn assert_problem_matcher(actual: &ProblemMatcherValue, expected: &ProblemMatcherValue) {
    match (actual, expected) {
        (ProblemMatcherValue::Name(actual), ProblemMatcherValue::Name(expected)) => {
            assert_eq!(actual, expected, "Problem matcher references are different");
        }
        (ProblemMatcherValue::Matcher(actual), ProblemMatcherValue::Matcher(expected)) => {
            if expected.owner == default_uuid() {
                assert!(is_uuid(&actual.owner), "Owner must be a UUID");
            } else {
                assert_eq!(actual.owner, expected.owner);
            }
            assert_eq!(actual.apply_to, expected.apply_to);
            assert_eq!(actual.severity, expected.severity);
            assert_eq!(actual.file_location, expected.file_location);
            assert_eq!(actual.file_prefix, expected.file_prefix);
            if !matches!(&actual.pattern, OneOrMany::Many(items) if items.is_empty()) && !matches!(&expected.pattern, OneOrMany::Many(items) if items.is_empty()) {
                assert_problem_patterns(&actual.pattern, &expected.pattern);
            }
        }
        _ => panic!("os tipos dos casadores de problemas diferem"),
    }
}

fn assert_problem_patterns(actual: &OneOrMany<ProblemPattern>, expected: &OneOrMany<ProblemPattern>) {
    match (actual, expected) {
        (OneOrMany::Many(actuals), OneOrMany::Many(expecteds)) => {
            assert_eq!(actuals.len(), expecteds.len());
            for (actual, expected) in actuals.iter().zip(expecteds) {
                assert_problem_pattern(actual, expected);
            }
        }
        (OneOrMany::One(actual), OneOrMany::One(expected)) => assert_problem_pattern(actual, expected),
        _ => panic!("os tipos dos padrões diferem"),
    }
}

fn assert_problem_pattern(actual: &ProblemPattern, expected: &ProblemPattern) {
    assert_eq!(actual.regexp.source(), expected.regexp.source());
    assert_eq!(actual.regexp.flags(), expected.regexp.flags());
    assert_eq!(actual.file, expected.file);
    assert_eq!(actual.message, expected.message);
    if expected.location.is_some() {
        assert_eq!(actual.location, expected.location);
    } else {
        assert_eq!(actual.line, expected.line);
        assert_eq!(actual.character, expected.character);
        assert_eq!(actual.end_line, expected.end_line);
        assert_eq!(actual.end_character, expected.end_character);
    }
    assert_eq!(actual.code, expected.code);
    assert_eq!(actual.severity, expected.severity);
    assert_eq!(actual.r#loop, expected.r#loop);
}

fn build() -> GroupValue {
    GroupValue::Group(TaskGroup::build())
}

fn test_group() -> GroupValue {
    GroupValue::Group(TaskGroup::test())
}

// suite('Tasks version 0.1.0')

#[test]
fn tasks_all_default() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let mut builder = ConfigurationBuilder::new();
    builder.task("tsc", "tsc").group(build()).command().suppress_task_name(true);
    test_configuration(json!({ "version": "0.1.0", "command": "tsc" }), &builder);
}

#[test]
fn tasks_global_is_shell_command() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let mut builder = ConfigurationBuilder::new();
    builder.task("tsc", "tsc").group(build()).command().suppress_task_name(true).runtime(RuntimeType::Shell);
    test_configuration(json!({ "version": "0.1.0", "command": "tsc", "isShellCommand": true }), &builder);
}

#[test]
fn tasks_global_show_output_silent() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let mut builder = ConfigurationBuilder::new();
    builder.task("tsc", "tsc").group(build()).command().suppress_task_name(true).presentation().reveal(RevealKind::Silent);
    test_configuration(json!({ "version": "0.1.0", "command": "tsc", "showOutput": "silent" }), &builder);
}

#[test]
fn tasks_global_prompt_on_close_default() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let mut builder = ConfigurationBuilder::new();
    builder.task("tsc", "tsc").group(build()).command().suppress_task_name(true);
    test_configuration(json!({ "version": "0.1.0", "command": "tsc", "promptOnClose": true }), &builder);
}

#[test]
fn tasks_global_prompt_on_close() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let mut builder = ConfigurationBuilder::new();
    builder.task("tsc", "tsc").group(build()).prompt_on_close(false).command().suppress_task_name(true);
    test_configuration(json!({ "version": "0.1.0", "command": "tsc", "promptOnClose": false }), &builder);
}

#[test]
fn tasks_global_prompt_on_close_default_watching() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let mut builder = ConfigurationBuilder::new();
    builder.task("tsc", "tsc").group(build()).is_background(true).prompt_on_close(false).command().suppress_task_name(true);
    test_configuration(json!({ "version": "0.1.0", "command": "tsc", "isWatching": true }), &builder);
}

#[test]
fn tasks_global_show_output_never() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let mut builder = ConfigurationBuilder::new();
    builder.task("tsc", "tsc").group(build()).command().suppress_task_name(true).presentation().reveal(RevealKind::Never);
    test_configuration(json!({ "version": "0.1.0", "command": "tsc", "showOutput": "never" }), &builder);
}

#[test]
fn tasks_global_echo_command() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let mut builder = ConfigurationBuilder::new();
    builder.task("tsc", "tsc").group(build()).command().suppress_task_name(true).presentation().echo(true);
    test_configuration(json!({ "version": "0.1.0", "command": "tsc", "echoCommand": true }), &builder);
}

#[test]
fn tasks_global_args() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let mut builder = ConfigurationBuilder::new();
    builder.task("tsc", "tsc").group(build()).command().suppress_task_name(true).args(&["--p"]);
    test_configuration(json!({ "version": "0.1.0", "command": "tsc", "args": ["--p"] }), &builder);
}

#[test]
fn tasks_options_cwd() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let mut builder = ConfigurationBuilder::new();
    builder.task("tsc", "tsc").group(build()).command().suppress_task_name(true).options(CommandOptions { cwd: Some("myPath".to_string()), ..Default::default() });
    test_configuration(json!({ "version": "0.1.0", "command": "tsc", "options": { "cwd": "myPath" } }), &builder);
}

#[test]
fn tasks_options_env() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let mut builder = ConfigurationBuilder::new();
    let env = json!({ "key": "value" }).as_object().cloned();
    builder
        .task("tsc", "tsc")
        .group(build())
        .command()
        .suppress_task_name(true)
        .options(CommandOptions { cwd: Some("${workspaceFolder}".to_string()), env, ..Default::default() });
    test_configuration(json!({ "version": "0.1.0", "command": "tsc", "options": { "env": { "key": "value" } } }), &builder);
}

#[test]
fn tasks_os_windows() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let name = if host_platform().is_windows { "tsc.win" } else { "tsc" };
    let mut builder = ConfigurationBuilder::new();
    builder.task(name, name).group(build()).command().suppress_task_name(true);
    let external = json!({ "version": "0.1.0", "command": "tsc", "windows": { "command": "tsc.win" } });
    test_configuration(external, &builder);
}

#[test]
fn tasks_os_windows_and_global_is_shell_command() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let name = if host_platform().is_windows { "tsc.win" } else { "tsc" };
    let mut builder = ConfigurationBuilder::new();
    builder.task(name, name).group(build()).command().suppress_task_name(true).runtime(RuntimeType::Shell);
    let external = json!({ "version": "0.1.0", "command": "tsc", "isShellCommand": true, "windows": { "command": "tsc.win" } });
    test_configuration(external, &builder);
}

#[test]
fn tasks_os_mac() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let name = if host_platform().is_macintosh { "tsc.osx" } else { "tsc" };
    let mut builder = ConfigurationBuilder::new();
    builder.task(name, name).group(build()).command().suppress_task_name(true);
    let external = json!({ "version": "0.1.0", "command": "tsc", "osx": { "command": "tsc.osx" } });
    test_configuration(external, &builder);
}

#[test]
fn tasks_os_linux() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let name = if host_platform().is_linux { "tsc.linux" } else { "tsc" };
    let mut builder = ConfigurationBuilder::new();
    builder.task(name, name).group(build()).command().suppress_task_name(true);
    let external = json!({ "version": "0.1.0", "command": "tsc", "linux": { "command": "tsc.linux" } });
    test_configuration(external, &builder);
}

#[test]
fn tasks_overwrite_show_output() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let mut builder = ConfigurationBuilder::new();
    builder
        .task("tsc", "tsc")
        .group(build())
        .command()
        .suppress_task_name(true)
        .presentation()
        .reveal(if host_platform().is_windows { RevealKind::Always } else { RevealKind::Never });
    let external = json!({ "version": "0.1.0", "command": "tsc", "showOutput": "never", "windows": { "showOutput": "always" } });
    test_configuration(external, &builder);
}

#[test]
fn tasks_overwrite_echo_command() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let mut builder = ConfigurationBuilder::new();
    builder.task("tsc", "tsc").group(build()).command().suppress_task_name(true).presentation().echo(!host_platform().is_windows);
    let external = json!({ "version": "0.1.0", "command": "tsc", "echoCommand": true, "windows": { "echoCommand": false } });
    test_configuration(external, &builder);
}

#[test]
fn tasks_global_problem_matcher_one() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let external = json!({ "version": "0.1.0", "command": "tsc", "problemMatcher": "$msCompile" });
    test_default_problem_matcher(external, 1);
}

#[test]
fn tasks_global_problem_matcher_two() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let external = json!({ "version": "0.1.0", "command": "tsc", "problemMatcher": ["$eslint-compact", "$msCompile"] });
    test_default_problem_matcher(external, 2);
}

#[test]
fn tasks_task_definition() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let external = json!({ "version": "0.1.0", "command": "tsc", "tasks": [{ "taskName": "taskName" }] });
    let mut builder = ConfigurationBuilder::new();
    builder.task("taskName", "tsc").command().args(&["$name"]);
    test_configuration(external, &builder);
}

#[test]
fn tasks_build_task() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let external = json!({ "version": "0.1.0", "command": "tsc", "tasks": [{ "taskName": "taskName", "isBuildCommand": true }] });
    let mut builder = ConfigurationBuilder::new();
    builder.task("taskName", "tsc").group(build()).command().args(&["$name"]);
    test_configuration(external, &builder);
}

#[test]
fn tasks_default_build_task() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let external = json!({ "version": "0.1.0", "command": "tsc", "tasks": [{ "taskName": "build" }] });
    let mut builder = ConfigurationBuilder::new();
    builder.task("build", "tsc").group(build()).command().args(&["$name"]);
    test_configuration(external, &builder);
}

#[test]
fn tasks_test_task() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let external = json!({ "version": "0.1.0", "command": "tsc", "tasks": [{ "taskName": "taskName", "isTestCommand": true }] });
    let mut builder = ConfigurationBuilder::new();
    builder.task("taskName", "tsc").group(test_group()).command().args(&["$name"]);
    test_configuration(external, &builder);
}

#[test]
fn tasks_default_test_task() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let external = json!({ "version": "0.1.0", "command": "tsc", "tasks": [{ "taskName": "test" }] });
    let mut builder = ConfigurationBuilder::new();
    builder.task("test", "tsc").group(test_group()).command().args(&["$name"]);
    test_configuration(external, &builder);
}

#[test]
fn tasks_task_with_values() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let external = json!({
        "version": "0.1.0",
        "command": "tsc",
        "tasks": [{ "taskName": "test", "showOutput": "never", "echoCommand": true, "args": ["--p"], "isWatching": true }]
    });
    let mut builder = ConfigurationBuilder::new();
    builder
        .task("test", "tsc")
        .group(test_group())
        .is_background(true)
        .prompt_on_close(false)
        .command()
        .args(&["$name", "--p"])
        .presentation()
        .echo(true)
        .reveal(RevealKind::Never);
    test_configuration(external, &builder);
}

#[test]
fn tasks_task_inherits_global_values() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let external = json!({ "version": "0.1.0", "command": "tsc", "showOutput": "never", "echoCommand": true, "tasks": [{ "taskName": "test" }] });
    let mut builder = ConfigurationBuilder::new();
    builder.task("test", "tsc").group(test_group()).command().args(&["$name"]).presentation().echo(true).reveal(RevealKind::Never);
    test_configuration(external, &builder);
}

#[test]
fn tasks_problem_matcher_default() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let external = json!({
        "version": "0.1.0",
        "command": "tsc",
        "tasks": [{ "taskName": "taskName", "problemMatcher": { "pattern": { "regexp": "abc" } } }]
    });
    let mut builder = ConfigurationBuilder::new();
    builder.task("taskName", "tsc").command().args(&["$name"]).parent().problem_matcher().pattern("abc");
    test_configuration(external, &builder);
}

#[test]
fn tasks_problem_matcher_dot_star_regular_expression() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let external = json!({
        "version": "0.1.0",
        "command": "tsc",
        "tasks": [{ "taskName": "taskName", "problemMatcher": { "pattern": { "regexp": ".*" } } }]
    });
    let mut builder = ConfigurationBuilder::new();
    builder.task("taskName", "tsc").command().args(&["$name"]).parent().problem_matcher().pattern(".*");
    test_configuration(external, &builder);
}

#[test]
fn tasks_problem_matcher_owner_apply_to_severity_and_file_location() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let external = json!({
        "version": "0.1.0",
        "command": "tsc",
        "tasks": [{
            "taskName": "taskName",
            "problemMatcher": {
                "owner": "myOwner",
                "applyTo": "closedDocuments",
                "severity": "warning",
                "fileLocation": "absolute",
                "pattern": { "regexp": "abc" }
            }
        }]
    });
    let mut builder = ConfigurationBuilder::new();
    builder
        .task("taskName", "tsc")
        .command()
        .args(&["$name"])
        .parent()
        .problem_matcher()
        .owner("myOwner")
        .apply_to(ApplyToKind::ClosedDocuments)
        .severity(Severity::Warning)
        .file_location(FileLocationKind::Absolute)
        .file_prefix(None)
        .pattern("abc");
    test_configuration(external, &builder);
}

#[test]
fn tasks_problem_matcher_file_location_and_file_prefix() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let external = json!({
        "version": "0.1.0",
        "command": "tsc",
        "tasks": [{
            "taskName": "taskName",
            "problemMatcher": { "fileLocation": ["relative", "myPath"], "pattern": { "regexp": "abc" } }
        }]
    });
    let mut builder = ConfigurationBuilder::new();
    builder
        .task("taskName", "tsc")
        .command()
        .args(&["$name"])
        .parent()
        .problem_matcher()
        .file_location(FileLocationKind::Relative)
        .file_prefix(Some("myPath"))
        .pattern("abc");
    test_configuration(external, &builder);
}

#[test]
fn tasks_problem_pattern_location() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let external = json!({
        "version": "0.1.0",
        "command": "tsc",
        "tasks": [{
            "taskName": "taskName",
            "problemMatcher": { "pattern": { "regexp": "abc", "file": 10, "message": 11, "location": 12, "severity": 13, "code": 14 } }
        }]
    });
    let mut builder = ConfigurationBuilder::new();
    builder.task("taskName", "tsc").command().args(&["$name"]).parent().problem_matcher().pattern("abc").file(10).message(11).location(12).severity(13).code(14);
    test_configuration(external, &builder);
}

#[test]
fn tasks_problem_pattern_line_and_column() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let external = json!({
        "version": "0.1.0",
        "command": "tsc",
        "tasks": [{
            "taskName": "taskName",
            "problemMatcher": {
                "pattern": {
                    "regexp": "abc",
                    "file": 10,
                    "message": 11,
                    "line": 12,
                    "column": 13,
                    "endLine": 14,
                    "endColumn": 15,
                    "severity": 16,
                    "code": 17
                }
            }
        }]
    });
    let mut builder = ConfigurationBuilder::new();
    builder
        .task("taskName", "tsc")
        .command()
        .args(&["$name"])
        .parent()
        .problem_matcher()
        .pattern("abc")
        .file(10)
        .message(11)
        .line(12)
        .character(13)
        .end_line(14)
        .end_character(15)
        .severity(16)
        .code(17);
    test_configuration(external, &builder);
}

#[test]
fn tasks_prompt_on_close_default() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let external = json!({ "version": "0.1.0", "command": "tsc", "tasks": [{ "taskName": "taskName" }] });
    let mut builder = ConfigurationBuilder::new();
    builder.task("taskName", "tsc").prompt_on_close(true).command().args(&["$name"]);
    test_configuration(external, &builder);
}

#[test]
fn tasks_prompt_on_close_watching() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let external = json!({ "version": "0.1.0", "command": "tsc", "tasks": [{ "taskName": "taskName", "isWatching": true }] });
    let mut builder = ConfigurationBuilder::new();
    builder.task("taskName", "tsc").is_background(true).prompt_on_close(false).command().args(&["$name"]);
    test_configuration(external, &builder);
}

#[test]
fn tasks_prompt_on_close_set() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let external = json!({ "version": "0.1.0", "command": "tsc", "tasks": [{ "taskName": "taskName", "promptOnClose": false }] });
    let mut builder = ConfigurationBuilder::new();
    builder.task("taskName", "tsc").prompt_on_close(false).command().args(&["$name"]);
    test_configuration(external, &builder);
}

#[test]
fn tasks_task_selector_set() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let external = json!({ "version": "0.1.0", "command": "tsc", "taskSelector": "/t:", "tasks": [{ "taskName": "taskName" }] });
    let mut builder = ConfigurationBuilder::new();
    builder.task("taskName", "tsc").command().task_selector("/t:").args(&["/t:taskName"]);
    test_configuration(external, &builder);
}

#[test]
fn tasks_suppress_task_name_set() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let external = json!({
        "version": "0.1.0",
        "command": "tsc",
        "suppressTaskName": false,
        "tasks": [{ "taskName": "taskName", "suppressTaskName": true }]
    });
    let mut builder = ConfigurationBuilder::new();
    builder.task("taskName", "tsc").command().suppress_task_name(true);
    test_configuration(external, &builder);
}

#[test]
fn tasks_suppress_task_name_inherit() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let external = json!({ "version": "0.1.0", "command": "tsc", "suppressTaskName": true, "tasks": [{ "taskName": "taskName" }] });
    let mut builder = ConfigurationBuilder::new();
    builder.task("taskName", "tsc").command().suppress_task_name(true);
    test_configuration(external, &builder);
}

#[test]
fn tasks_two_tasks() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let external = json!({ "version": "0.1.0", "command": "tsc", "tasks": [{ "taskName": "taskNameOne" }, { "taskName": "taskNameTwo" }] });
    let mut builder = ConfigurationBuilder::new();
    builder.task("taskNameOne", "tsc").command().args(&["$name"]);
    builder.task("taskNameTwo", "tsc").command().args(&["$name"]);
    test_configuration(external, &builder);
}

#[test]
fn tasks_with_command() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let external = json!({ "version": "0.1.0", "tasks": [{ "taskName": "taskNameOne", "command": "tsc" }] });
    let mut builder = ConfigurationBuilder::new();
    builder.task("taskNameOne", "tsc").command().suppress_task_name(true);
    test_configuration(external, &builder);
}

#[test]
fn tasks_two_tasks_with_command() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let external = json!({
        "version": "0.1.0",
        "tasks": [{ "taskName": "taskNameOne", "command": "tsc" }, { "taskName": "taskNameTwo", "command": "dir" }]
    });
    let mut builder = ConfigurationBuilder::new();
    builder.task("taskNameOne", "tsc").command().suppress_task_name(true);
    builder.task("taskNameTwo", "dir").command().suppress_task_name(true);
    test_configuration(external, &builder);
}

#[test]
fn tasks_with_command_and_args() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let external = json!({
        "version": "0.1.0",
        "tasks": [{
            "taskName": "taskNameOne",
            "command": "tsc",
            "isShellCommand": true,
            "args": ["arg"],
            "options": { "cwd": "cwd", "env": { "env": "env" } }
        }]
    });
    let mut builder = ConfigurationBuilder::new();
    let env = json!({ "env": "env" }).as_object().cloned();
    builder
        .task("taskNameOne", "tsc")
        .command()
        .suppress_task_name(true)
        .runtime(RuntimeType::Shell)
        .args(&["arg"])
        .options(CommandOptions { cwd: Some("cwd".to_string()), env, ..Default::default() });
    test_configuration(external, &builder);
}

#[test]
fn tasks_with_command_os_specific() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let name = if host_platform().is_windows { "tsc.win" } else { "tsc" };
    let external = json!({
        "version": "0.1.0",
        "tasks": [{ "taskName": "taskNameOne", "command": "tsc", "windows": { "command": "tsc.win" } }]
    });
    let mut builder = ConfigurationBuilder::new();
    builder.task("taskNameOne", name).command().suppress_task_name(true);
    test_configuration(external, &builder);
}

#[test]
fn tasks_with_windows_specific_args() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let args: Vec<&str> = if host_platform().is_windows { vec!["arg1", "arg2"] } else { vec!["arg1"] };
    let external = json!({
        "version": "0.1.0",
        "tasks": [{ "taskName": "tsc", "command": "tsc", "args": ["arg1"], "windows": { "args": ["arg2"] } }]
    });
    let mut builder = ConfigurationBuilder::new();
    builder.task("tsc", "tsc").command().suppress_task_name(true).args(&args);
    test_configuration(external, &builder);
}

#[test]
fn tasks_with_linux_specific_args() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let args: Vec<&str> = if host_platform().is_linux { vec!["arg1", "arg2"] } else { vec!["arg1"] };
    let external = json!({
        "version": "0.1.0",
        "tasks": [{ "taskName": "tsc", "command": "tsc", "args": ["arg1"], "linux": { "args": ["arg2"] } }]
    });
    let mut builder = ConfigurationBuilder::new();
    builder.task("tsc", "tsc").command().suppress_task_name(true).args(&args);
    test_configuration(external, &builder);
}

#[test]
fn tasks_global_command_and_task_command_properties() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let external = json!({ "version": "0.1.0", "command": "tsc", "tasks": [{ "taskName": "taskNameOne", "isShellCommand": true }] });
    let mut builder = ConfigurationBuilder::new();
    builder.task("taskNameOne", "tsc").command().runtime(RuntimeType::Shell).args(&["$name"]);
    test_configuration(external, &builder);
}

#[test]
fn tasks_global_and_tasks_args() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let external = json!({
        "version": "0.1.0",
        "command": "tsc",
        "args": ["global"],
        "tasks": [{ "taskName": "taskNameOne", "args": ["local"] }]
    });
    let mut builder = ConfigurationBuilder::new();
    builder.task("taskNameOne", "tsc").command().args(&["global", "$name", "local"]);
    test_configuration(external, &builder);
}

#[test]
fn tasks_global_and_tasks_args_with_task_selector() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let external = json!({
        "version": "0.1.0",
        "command": "tsc",
        "args": ["global"],
        "taskSelector": "/t:",
        "tasks": [{ "taskName": "taskNameOne", "args": ["local"] }]
    });
    let mut builder = ConfigurationBuilder::new();
    builder.task("taskNameOne", "tsc").command().task_selector("/t:").args(&["global", "/t:taskNameOne", "local"]);
    test_configuration(external, &builder);
}

// suite('Tasks version 2.0.0')

#[test]
#[ignore = "test.skip no original"]
fn build_workspace_task() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let external = json!({ "version": "2.0.0", "tasks": [{ "taskName": "dir", "command": "dir", "type": "shell", "group": "build" }] });
    let mut builder = ConfigurationBuilder::new();
    builder.task("dir", "dir").group(build()).command().suppress_task_name(true).runtime(RuntimeType::Shell).presentation().echo(true);
    test_configuration(external, &builder);
}

#[test]
fn global_group_none() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let external = json!({ "version": "2.0.0", "command": "dir", "type": "shell", "group": "none" });
    let mut builder = ConfigurationBuilder::new();
    builder.task("dir", "dir").command().suppress_task_name(true).runtime(RuntimeType::Shell).presentation().echo(true);
    test_configuration(external, &builder);
}

#[test]
#[ignore = "test.skip no original"]
fn global_group_build() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let external = json!({ "version": "2.0.0", "command": "dir", "type": "shell", "group": "build" });
    let mut builder = ConfigurationBuilder::new();
    builder.task("dir", "dir").group(build()).command().suppress_task_name(true).runtime(RuntimeType::Shell).presentation().echo(true);
    test_configuration(external, &builder);
}

fn default_build_group() -> GroupValue {
    GroupValue::Group(Rc::new(TaskGroup { id: "build".to_string(), is_default: Some(IsDefault::Bool(true)) }))
}

#[test]
#[ignore = "test.skip no original"]
fn global_group_default_build() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let external = json!({ "version": "2.0.0", "command": "dir", "type": "shell", "group": { "kind": "build", "isDefault": true } });
    let mut builder = ConfigurationBuilder::new();
    builder.task("dir", "dir").group(default_build_group()).command().suppress_task_name(true).runtime(RuntimeType::Shell).presentation().echo(true);
    test_configuration(external, &builder);
}

#[test]
fn local_group_none() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let external = json!({ "version": "2.0.0", "tasks": [{ "taskName": "dir", "command": "dir", "type": "shell", "group": "none" }] });
    let mut builder = ConfigurationBuilder::new();
    builder.task("dir", "dir").command().suppress_task_name(true).runtime(RuntimeType::Shell).presentation().echo(true);
    test_configuration(external, &builder);
}

#[test]
#[ignore = "test.skip no original"]
fn local_group_build() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let external = json!({ "version": "2.0.0", "tasks": [{ "taskName": "dir", "command": "dir", "type": "shell", "group": "build" }] });
    let mut builder = ConfigurationBuilder::new();
    builder.task("dir", "dir").group(build()).command().suppress_task_name(true).runtime(RuntimeType::Shell).presentation().echo(true);
    test_configuration(external, &builder);
}

#[test]
#[ignore = "test.skip no original"]
fn local_group_default_build() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let external = json!({
        "version": "2.0.0",
        "tasks": [{ "taskName": "dir", "command": "dir", "type": "shell", "group": { "kind": "build", "isDefault": true } }]
    });
    let mut builder = ConfigurationBuilder::new();
    builder.task("dir", "dir").group(default_build_group()).command().suppress_task_name(true).runtime(RuntimeType::Shell).presentation().echo(true);
    test_configuration(external, &builder);
}

#[test]
fn arg_overwrite() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let external = json!({
        "version": "2.0.0",
        "tasks": [{
            "label": "echo",
            "type": "shell",
            "command": "echo",
            "args": ["global"],
            "windows": { "args": ["windows"] },
            "linux": { "args": ["linux"] },
            "osx": { "args": ["osx"] }
        }]
    });
    let mut builder = ConfigurationBuilder::new();
    let platform = host_platform();
    let args: Option<&str> = if platform.is_windows {
        Some("windows")
    } else if platform.is_linux {
        Some("linux")
    } else if platform.is_macintosh {
        Some("osx")
    } else {
        None
    };
    if let Some(args) = args {
        builder.task("echo", "echo").command().suppress_task_name(true).args(&[args]).runtime(RuntimeType::Shell).presentation().echo(true);
        test_configuration(external, &builder);
    }
}

// suite('Bugs / regression tests')

#[test]
#[cfg_attr(target_os = "linux", ignore = "test.skip no Linux, como no original")]
fn bug_19548() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let external = json!({
        "version": "0.1.0",
        "windows": {
            "command": "powershell",
            "options": { "cwd": "${workspaceFolder}" },
            "tasks": [{
                "taskName": "composeForDebug",
                "suppressTaskName": true,
                "args": ["-ExecutionPolicy", "RemoteSigned", ".\\dockerTask.ps1", "-ComposeForDebug", "-Environment", "debug"],
                "isBuildCommand": false,
                "showOutput": "always",
                "echoCommand": true
            }]
        },
        "osx": {
            "command": "/bin/bash",
            "options": { "cwd": "${workspaceFolder}" },
            "tasks": [{
                "taskName": "composeForDebug",
                "suppressTaskName": true,
                "args": ["-c", "./dockerTask.sh composeForDebug debug"],
                "isBuildCommand": false,
                "showOutput": "always"
            }]
        }
    });
    let mut builder = ConfigurationBuilder::new();
    let platform = host_platform();
    if platform.is_windows {
        builder
            .task("composeForDebug", "powershell")
            .command()
            .suppress_task_name(true)
            .args(&["-ExecutionPolicy", "RemoteSigned", ".\\dockerTask.ps1", "-ComposeForDebug", "-Environment", "debug"])
            .options(CommandOptions { cwd: Some("${workspaceFolder}".to_string()), ..Default::default() })
            .presentation()
            .echo(true)
            .reveal(RevealKind::Always);
        test_configuration(external, &builder);
    } else if platform.is_macintosh {
        builder
            .task("composeForDebug", "/bin/bash")
            .command()
            .suppress_task_name(true)
            .args(&["-c", "./dockerTask.sh composeForDebug debug"])
            .options(CommandOptions { cwd: Some("${workspaceFolder}".to_string()), ..Default::default() })
            .presentation()
            .reveal(RevealKind::Always);
        test_configuration(external, &builder);
    }
}

#[test]
fn bug_28489() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let external = json!({
        "version": "0.1.0",
        "command": "",
        "isShellCommand": true,
        "args": [""],
        "showOutput": "always",
        "tasks": [{ "taskName": "build", "command": "bash", "args": ["build.sh"] }]
    });
    let mut builder = ConfigurationBuilder::new();
    builder.task("build", "bash").group(build()).command().suppress_task_name(true).args(&["build.sh"]).runtime(RuntimeType::Shell);
    test_configuration(external, &builder);
}

// suite('Task configuration conversions')

struct TaskDefinitionRegistryDouble {
    task: RefCell<Option<TaskDefinition>>,
}

impl TaskDefinitionRegistryDouble {
    fn set(&self, task: TaskDefinition) {
        *self.task.borrow_mut() = Some(task);
    }
}

impl TaskDefinitionGetter for TaskDefinitionRegistryDouble {
    fn get(&self, _key: &str) -> Option<TaskDefinition> {
        self.task.borrow().clone()
    }
}

struct ConversionsSetup {
    parse_context: ParseContext,
    problem_reporter: Rc<ProblemReporterDouble>,
}

/// `setup`: o `namedProblemMatcher`, o relator e o contexto de cada caso.
fn conversions_setup(apply_to: Option<ApplyToKind>) -> ConversionsSetup {
    let problem_reporter = ProblemReporterDouble::new();
    let mut named_problem_matcher = ProblemMatcher::new("", ApplyToKind::AllDocuments, FileLocationKind::Absolute, OneOrMany::Many(Vec::new()));
    named_problem_matcher.name = Some("real".to_string());
    named_problem_matcher.label = Some("real label".to_string());
    if let Some(apply_to) = apply_to {
        named_problem_matcher.apply_to = apply_to;
    }
    let mut named_problem_matchers = IndexMap::new();
    named_problem_matchers.insert("real".to_string(), named_problem_matcher);
    let reporter: Rc<dyn ProblemReporter> = problem_reporter.clone();
    let key_service: Rc<dyn IContextKeyService> = TasksMockContextKeyService::new();
    let parse_context = ParseContext {
        workspace_folder: workspace_folder(),
        workspace: None,
        problem_reporter: reporter,
        named_problem_matchers,
        uuid_map: Rc::new(RefCell::new(UuidMap::new(None))),
        engine: ExecutionEngine::Process,
        schema_version: JsonSchemaVersion::V0_1_0,
        platform: je_base::platform::PlatformKind::Web,
        task_load_issues: Vec::new(),
        context_key_service: key_service,
    };
    ConversionsSetup { parse_context, problem_reporter }
}

// suite('ProblemMatcherConverter.from')

#[test]
fn returns_empty_and_an_error_for_an_unknown_problem_matcher() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let setup = conversions_setup(None);
    let result = problem_matcher_converter::from(Some(&json!("$fake")), &setup.parse_context);
    assert_eq!(result.value.as_ref().map(Vec::len), Some(0));
    assert_eq!(result.errors.as_ref().map(Vec::len), Some(1));
}

#[test]
fn returns_config_for_a_known_problem_matcher() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let setup = conversions_setup(None);
    let result = problem_matcher_converter::from(Some(&json!("$real")), &setup.parse_context);
    assert_eq!(result.errors.as_ref().map(Vec::len), Some(0));
    let value = result.value.expect("valor");
    assert_eq!(value.len(), 1);
    assert_eq!(value[0].label.as_deref(), Some("real label"));
    assert_eq!(value[0].name, None);
}

#[test]
fn returns_config_for_a_known_problem_matcher_including_apply_to() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let setup = conversions_setup(Some(ApplyToKind::ClosedDocuments));
    let result = problem_matcher_converter::from(Some(&json!("$real")), &setup.parse_context);
    assert_eq!(result.errors.as_ref().map(Vec::len), Some(0));
    let value = result.value.expect("valor");
    assert_eq!(value.len(), 1);
    assert_eq!(value[0].label.as_deref(), Some("real label"));
    assert_eq!(value[0].apply_to, ApplyToKind::ClosedDocuments);
}

// suite('TaskParser.from')

/// `assertTaskParseResult`.
fn assert_task_parse_result(
    actual: &TaskParseResult,
    expected_custom: Option<&[&str]>,
    expected_configured: Option<&[&str]>,
    problem_reporter: &ProblemReporterDouble,
    expected_message: Option<&str>,
) {
    match expected_message {
        None => assert_eq!(problem_reporter.last_message(), None),
        Some(expected_message) => assert!(problem_reporter.last_message().is_some_and(|message| message.contains(expected_message))),
    }

    assert_eq!(actual.custom.len(), expected_custom.map_or(0, <[&str]>::len));
    assert_eq!(actual.configured.len(), expected_configured.map_or(0, <[&str]>::len));

    for (index, label) in expected_configured.unwrap_or_default().iter().enumerate() {
        assert_eq!(actual.configured[index].common.label, *label);
    }
    for (index, task_name) in expected_custom.unwrap_or_default().iter().enumerate() {
        assert_eq!(actual.custom[index].common.label, *task_name);
    }
    problem_reporter.clear_message();
}

#[test]
fn task_parser_custom_task_incomplete_config_reports_an_error_for_missing_name() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let mut setup = conversions_setup(None);
    let result = task_parser::from(Some(&[json!({})]), &Globals::default(), &mut setup.parse_context, TaskConfigSource::TasksJson, None);
    assert_task_parse_result(&result, None, None, &setup.problem_reporter, Some("Error: a task must provide a label property"));
}

#[test]
fn task_parser_custom_task_incomplete_config_reports_an_error_for_missing_command() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let mut setup = conversions_setup(None);
    let result = task_parser::from(Some(&[json!({ "taskName": "task" })]), &Globals::default(), &mut setup.parse_context, TaskConfigSource::TasksJson, None);
    assert_task_parse_result(&result, None, None, &setup.problem_reporter, Some("Error: the task 'task' doesn't define a command"));
}

#[test]
fn task_parser_custom_task_returns_expected_result() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let mut setup = conversions_setup(None);
    let expected = [json!({ "taskName": "task", "command": "echo test" }), json!({ "taskName": "task 2", "command": "echo test" })];
    let result = task_parser::from(Some(&expected), &Globals::default(), &mut setup.parse_context, TaskConfigSource::TasksJson, None);
    assert_task_parse_result(&result, Some(&["task", "task 2"]), None, &setup.problem_reporter, None);
}

#[test]
fn task_parser_configured_task_returns_expected_result() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let mut setup = conversions_setup(None);
    let expected = [
        json!({ "taskName": "task", "command": "echo test", "type": "any", "label": "task" }),
        json!({ "taskName": "task 2", "command": "echo test", "type": "any", "label": "task 2" }),
    ];
    let registry = TaskDefinitionRegistryDouble { task: RefCell::new(None) };
    registry.set(TaskDefinition {
        extension_id: "registered".to_string(),
        task_type: "any".to_string(),
        required: Vec::new(),
        properties: tasks::JsonObject::new(),
        when: None,
    });
    let result = task_parser::from(Some(&expected), &Globals::default(), &mut setup.parse_context, TaskConfigSource::TasksJson, Some(&registry));
    assert_task_parse_result(&result, None, Some(&["task", "task 2"]), &setup.problem_reporter, None);
}
