//! Porte de `src/vs/workbench/contrib/tasks/common/taskDefinitionRegistry.ts`: o registro das definições de tipo de
//! tarefa contribuídas pelas extensões (`taskDefinitions`), que o `tasks.ts` e o `taskConfiguration.ts` consultam.
//!
//! Divergências:
//! - O ponto de extensão do original (`registerExtensionPoint` com `activationEventsGenerator`, `jsonSchema` e
//!   `setHandler`) não cabe no registro reduzido de [`crate::services::extensions::extensions_registry`]. Como no
//!   `problem_matcher`, [`register_task_definitions_extension_point`] registra o nome do ponto (não é chamada na
//!   criação do registro, para não lançar `Duplicate extension point`), [`task_definitions_activation_events`] é o
//!   `activationEventsGenerator`, [`task_definitions_extension_point_schema`] é o `jsonSchema` e o corpo do tratador é
//!   [`TaskDefinitionRegistryImpl::handle_delta`].
//! - `IExtensionPointUser` do porte não carrega `description.identifier`, que o tratador lê: o contrato
//!   [`TaskDefinitionUser`] traz o `extension_id` (o `extension.description.identifier.value`).
//! - `Partial<ITaskDefinitionRegistry>` (o que os testes passam ao `TaskParser`) é o trait [`TaskDefinitionGetter`],
//!   do qual [`ITaskDefinitionRegistry`] é extensão.
//! - `IJSONSchema` e `IJSONSchemaMap` são `serde_json::Value` e `serde_json::Map`.
//! - `get` devolve `Option` (o original o tipa como definido e devolve `undefined` quando falta).

use std::cell::RefCell;
use std::rc::Rc;

use indexmap::IndexMap;
use je_base::dynamic::Dynamic;
use je_base::errors;
use je_base::event::{Emitter, Event};
use je_base::nls::localize;
use je_base::promise::{Promise, Resolver};
use je_platform::contextkey::ContextKeyExpr;
use serde_json::{Map, Value};

use super::tasks::TaskDefinition;
use crate::services::extensions::extensions_registry::{ExtensionMessageCollector, ExtensionPoint, ExtensionPointDescriptor, extensions_registry};

/// `Configuration.ITaskDefinition`: o que a extensão escreve em `contributes.taskDefinitions`.
pub mod config {
    use serde_json::{Map, Value};

    #[derive(Clone, Debug, Default, PartialEq)]
    pub struct TaskDefinition {
        pub r#type: Option<String>,
        /// Os elementos que não são texto são descartados na conversão.
        pub required: Option<Vec<Value>>,
        pub properties: Option<Map<String, Value>>,
        pub when: Option<String>,
    }
}

/// `Configuration.from`.
pub fn definition_from_configuration(
    value: &config::TaskDefinition,
    extension_id: &str,
    message_collector: &dyn ExtensionMessageCollector,
) -> Option<TaskDefinition> {
    let task_type = value.r#type.clone().filter(|task_type| !task_type.is_empty());
    let Some(task_type) = task_type else {
        message_collector.error(&localize("TaskTypeConfiguration.noType", "The task type configuration is missing the required 'taskType' property", &[]));
        return None;
    };
    let required: Vec<String> = value
        .required
        .iter()
        .flatten()
        .filter_map(|element| element.as_str().map(str::to_string))
        .collect();
    Some(TaskDefinition {
        extension_id: extension_id.to_string(),
        task_type,
        required,
        properties: value.properties.clone().unwrap_or_default(),
        when: value.when.as_deref().filter(|when| !when.is_empty()).and_then(|when| ContextKeyExpr::deserialize(Some(when))),
    })
}

fn text(value: &str) -> Dynamic {
    Dynamic::string(value)
}

fn desc(key: &str, message: &str) -> Dynamic {
    Dynamic::string(&localize(key, message, &[]))
}

/// `taskDefinitionSchema`.
fn task_definition_schema() -> Dynamic {
    Dynamic::object([
        ("type", text("object")),
        ("additionalProperties", Dynamic::Bool(false)),
        (
            "properties",
            Dynamic::object([
                (
                    "type",
                    Dynamic::object([
                        ("type", text("string")),
                        (
                            "description",
                            desc(
                                "TaskDefinition.description",
                                "The actual task type. Please note that types starting with a '$' are reserved for internal usage.",
                            ),
                        ),
                    ]),
                ),
                (
                    "required",
                    Dynamic::object([
                        ("type", text("array")),
                        (
                            "markdownDescription",
                            desc(
                                "TaskDefinition.required",
                                "The names of the properties from the `properties` object that must be provided for a task of this type to be considered a match. Used by VS Code to associate a `tasks.json` entry with a registered task provider.",
                            ),
                        ),
                        ("items", Dynamic::object([("type", text("string"))])),
                    ]),
                ),
                (
                    "properties",
                    Dynamic::object([
                        ("type", text("object")),
                        ("description", desc("TaskDefinition.properties", "Additional properties of the task type")),
                        ("additionalProperties", Dynamic::object([("$ref", text("http://json-schema.org/draft-07/schema#"))])),
                    ]),
                ),
                (
                    "when",
                    Dynamic::object([
                        ("type", text("string")),
                        (
                            "markdownDescription",
                            desc(
                                "TaskDefinition.when",
                                "Condition which must be true to enable this type of task. Consider using `shellExecutionSupported`, `processExecutionSupported`, and `customExecutionSupported` as appropriate for this task definition. See the [API documentation](https://code.visualstudio.com/api/extension-guides/task-provider#when-clause) for more information.",
                            ),
                        ),
                        ("default", text("")),
                    ]),
                ),
            ]),
        ),
    ])
}

/// O `jsonSchema` do ponto de extensão `taskDefinitions`.
pub fn task_definitions_extension_point_schema() -> Dynamic {
    Dynamic::object([
        ("description", desc("TaskDefinitionExtPoint", "Contributes task kinds")),
        ("type", text("array")),
        ("items", task_definition_schema()),
    ])
}

/// O `activationEventsGenerator` do ponto de extensão `taskDefinitions`.
pub fn task_definitions_activation_events(contributions: &[config::TaskDefinition]) -> Vec<String> {
    contributions
        .iter()
        .filter_map(|task| task.r#type.as_deref().filter(|task_type| !task_type.is_empty()).map(|task_type| format!("onTaskType:{task_type}")))
        .collect()
}

/// `registerExtensionPoint({ extensionPoint: 'taskDefinitions', ... })`.
pub fn register_task_definitions_extension_point() -> Rc<ExtensionPoint> {
    extensions_registry().register_extension_point(ExtensionPointDescriptor {
        extension_point: "taskDefinitions".to_string(),
        default_extension_kind: None,
        can_handle_resolver: None,
    })
}

/// Uma extensão no lote do tratador (ver as divergências no topo do módulo).
#[derive(Clone)]
pub struct TaskDefinitionUser {
    pub extension_id: String,
    pub value: Vec<config::TaskDefinition>,
    pub collector: Rc<dyn ExtensionMessageCollector>,
}

/// `delta` do tratador do ponto de extensão.
#[derive(Clone, Default)]
pub struct TaskDefinitionDelta {
    pub added: Vec<TaskDefinitionUser>,
    pub removed: Vec<TaskDefinitionUser>,
}

/// O pedaço de `Partial<ITaskDefinitionRegistry>` que o `TaskParser` e o `ConfiguringTask` usam.
pub trait TaskDefinitionGetter {
    fn get(&self, key: &str) -> Option<TaskDefinition>;
}

/// `ITaskDefinitionRegistry`.
pub trait ITaskDefinitionRegistry: TaskDefinitionGetter {
    fn on_ready(&self) -> Promise<()>;

    fn all(&self) -> Vec<TaskDefinition>;

    fn get_json_schema(&self) -> Value;

    fn on_definitions_changed(&self) -> Event<()>;
}

/// `TaskDefinitionRegistryImpl`.
pub struct TaskDefinitionRegistryImpl {
    task_types: RefCell<IndexMap<String, TaskDefinition>>,
    ready_promise: Promise<()>,
    ready_resolver: Resolver<()>,
    schema: RefCell<Option<Value>>,
    on_definitions_changed: Rc<Emitter<()>>,
}

impl Default for TaskDefinitionRegistryImpl {
    fn default() -> Self {
        Self::new()
    }
}

impl TaskDefinitionRegistryImpl {
    pub fn new() -> Self {
        let (ready_promise, ready_resolver) = Promise::new();
        Self {
            task_types: RefCell::new(IndexMap::new()),
            ready_promise,
            ready_resolver,
            schema: RefCell::new(None),
            on_definitions_changed: Emitter::new(),
        }
    }

    /// O corpo do `taskDefinitionsExtPoint.setHandler`: `delta.removed`, `delta.added`, o evento de mudança e a
    /// resolução de `onReady`.
    pub fn handle_delta(&self, delta: &TaskDefinitionDelta) {
        *self.schema.borrow_mut() = None;
        let _ = errors::catch(|| {
            for extension in &delta.removed {
                for task_type in &extension.value {
                    if let Some(name) = task_type.r#type.as_deref().filter(|name| !name.is_empty()) {
                        self.task_types.borrow_mut().shift_remove(name);
                    }
                }
            }
            for extension in &delta.added {
                for task_type in &extension.value {
                    if let Some(definition) = definition_from_configuration(task_type, &extension.extension_id, extension.collector.as_ref()) {
                        self.task_types.borrow_mut().insert(definition.task_type.clone(), definition);
                    }
                }
            }
            if !delta.removed.is_empty() || !delta.added.is_empty() {
                self.on_definitions_changed.fire(());
            }
        });
        self.ready_resolver.resolve(());
    }
}

impl TaskDefinitionGetter for TaskDefinitionRegistryImpl {
    fn get(&self, key: &str) -> Option<TaskDefinition> {
        self.task_types.borrow().get(key).cloned()
    }
}

impl ITaskDefinitionRegistry for TaskDefinitionRegistryImpl {
    fn on_ready(&self) -> Promise<()> {
        self.ready_promise.clone()
    }

    fn all(&self) -> Vec<TaskDefinition> {
        self.task_types.borrow().values().cloned().collect()
    }

    fn get_json_schema(&self) -> Value {
        if self.schema.borrow().is_none() {
            let mut schemas: Vec<Value> = Vec::new();
            for definition in self.all() {
                let mut schema = Map::new();
                schema.insert("type".to_string(), Value::from("object"));
                schema.insert("additionalProperties".to_string(), Value::Bool(false));
                if !definition.required.is_empty() {
                    schema.insert("required".to_string(), Value::Array(definition.required.iter().map(|name| Value::from(name.as_str())).collect()));
                }
                let mut properties = definition.properties.clone();
                let mut type_schema = Map::new();
                type_schema.insert("type".to_string(), Value::from("string"));
                type_schema.insert("enum".to_string(), Value::Array(vec![Value::from(definition.task_type.as_str())]));
                properties.insert("type".to_string(), Value::Object(type_schema));
                schema.insert("properties".to_string(), Value::Object(properties));
                schemas.push(Value::Object(schema));
            }
            let mut result = Map::new();
            result.insert("oneOf".to_string(), Value::Array(schemas));
            *self.schema.borrow_mut() = Some(Value::Object(result));
        }
        self.schema.borrow().clone().unwrap_or(Value::Null)
    }

    fn on_definitions_changed(&self) -> Event<()> {
        self.on_definitions_changed.event()
    }
}

thread_local! {
    static TASK_DEFINITION_REGISTRY: Rc<TaskDefinitionRegistryImpl> = Rc::new(TaskDefinitionRegistryImpl::new());
}

/// `TaskDefinitionRegistry`.
pub fn task_definition_registry() -> Rc<TaskDefinitionRegistryImpl> {
    TASK_DEFINITION_REGISTRY.with(Rc::clone)
}
