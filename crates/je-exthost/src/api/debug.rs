//! `vscode.debug` e as classes `vscode.SourceBreakpoint`, `vscode.FunctionBreakpoint`, `vscode.Breakpoint`,
//! `vscode.DebugAdapterExecutable`, `vscode.DebugAdapterServer` e `vscode.DebugAdapterNamedPipeServer`.
//!
//! É a ponte para o `ExtHostDebugService` portado (`extHostDebugService.ts`), que fala com o cliente `MainThreadDebugService` do
//! workbench (`$registerDebugConfigurationProvider`, `$startDebugging`, `$registerBreakpoints`...). O namespace tem
//! `registerDebugConfigurationProvider`, `registerDebugAdapterDescriptorFactory`, `startDebugging`, `stopDebugging`,
//! `activeDebugSession`, `activeDebugConsole`, `breakpoints`, `addBreakpoints`, `removeBreakpoints`, `onDidStartDebugSession`,
//! `onDidTerminateDebugSession`, `onDidChangeActiveDebugSession`, `onDidReceiveDebugSessionCustomEvent` e `onDidChangeBreakpoints`.
//! O objeto `DebugSession` tem `id`, `type`, `name` (com atribuição), `workspaceFolder`, `configuration`, `parentSession`,
//! `customRequest` e `getDebugProtocolBreakpoint`.
//!
//! O provedor de configuração e a fábrica de descritor são objetos da extensão chamados POR REFERÊNCIA (`JsObjects::invoke`), com o
//! `this` do objeto e os métodos de protótipo de uma classe; o que eles recebem e devolvem (a configuração de depuração) atravessa
//! como JSON. Os objetos `DebugSession` e `Breakpoint` carregam só o `_id` (o id da sessão ou do breakpoint) e as propriedades são
//! acessores do protótipo sobre o objeto vivo do `ExtHostDebugService`.
//!
//! Falta: `registerDebugAdapterTrackerFactory`, `asDebugSourceUri`, `activeStackItem` e `onDidChangeActiveStackItem`
//! (`DebugThread` e `DebugStackFrame`), as propostas `debugVisualization`, `DebugSessionOptions#testRun` (o host não tem o namespace
//! `tests`), `DataBreakpoint` como classe construível e `DebugAdapterInlineImplementation` (o adaptador chegaria ao construtor por
//! cópia e perderia as funções dele; a classe existe e rejeita com uma mensagem explícita). O `${variable}` das configurações não é
//! substituído: o host não tem o `IConfigurationResolverService`, e `$substituteVariables` rejeita com essa explicação.
//!
//! Divergências:
//! - O `vscode` é um só para todas as extensões, então o host não sabe qual delas chama `registerDebugAdapterDescriptorFactory`. A
//!   fábrica é registrada em nome da extensão do host que contribui o depurador daquele tipo (a verificação de
//!   `definesDebugType` do original); sem nenhuma, vale o `je.exthost` e o registro falha com a mensagem do original.
//! - A tabela de sessões e de breakpoints não tem coletor: o objeto vive até o fim do host.
//! - A atribuição a uma propriedade de um breakpoint não existe (no `vscode.d.ts` elas são `readonly`).

use std::any::Any;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use je_base::cancellation::CancellationToken;
use je_base::errors::Error;
use je_base::event::Event;
use je_base::promise::{MaybePromise, Promise};
use je_workbench::api::common::ext_host_debug_protocol::{MainThreadDebugServiceShape, NameOrConfig};
use je_workbench::api::common::ext_host_debug_service::{
    DebugSession, ExtHostDebugServiceBase, IConfigurationResolver, IExtHostDebugExtensionService, IExtHostDebugTesting, IExtHostVariableResolverProvider,
};
use je_workbench::api::common::ext_host_debug_types::{
    Breakpoint, BreakpointKind, BreakpointsChangeEvent, DebugAdapterDescriptor, DebugAdapterDescriptorFactory, DebugAdapterExecutable, DebugAdapterNamedPipeServer,
    DebugAdapterServer, DebugConfigurationProvider, DebugConsoleMode, DebugSessionCustomEvent, DebugSessionOptions, ProviderResult,
};
use je_workbench::api::common::ext_host_protocol::{IMainContext, MainContext, MainThreadTelemetryShape};
use je_workbench::api::common::ext_host_testing::TestRun;
use je_workbench::api::common::ext_host_workspace::WorkspaceFolder;
use je_workbench::contrib::debug::debug::{DebugConfigurationProviderTriggerKind, IConfig, IDebugAdapterExecutableOptions};
use je_workbench::services::extensions::extension_description_registry::ExtensionDescriptionRegistry;
use serde_json::{Map, Number, Value};
use zjsc::api::embed_node::{ClassSpec, JsData, MethodCallback, NativeCallback, NodeRunner, CLASS_KEY};

use crate::api::accessor_namespace;
use crate::api::commands::promise_to_js;
use crate::api::disposable::DisposableTable;
use crate::api::events::{event_function, EventTables};
use crate::api::extensions::{json_to_js, ExtensionServiceSlot};
use crate::api::js_object::{JsObjects, JsResult};
use crate::api::language_types::location_from_js;
use crate::api::languages::host_extension;
use crate::api::workspace::folder_to_js;
use crate::convert::{api_to_js, class_name, field, is_nullish, opt_bool, opt_number, opt_string, string_arg, uri_from_js};

// ---- as dependências do `ExtHostDebugService` que o host não tem

/// O `IExtHostExtensionService` do serviço de depuração: o registro das extensões deste host, que assenta quando o serviço de extensões
/// é ligado.
impl IExtHostDebugExtensionService for ExtensionServiceSlot {
    fn get_extension_registry(&self) -> Promise<Rc<ExtensionDescriptionRegistry>> {
        self.extension_registry()
    }
}

/// O `IExtHostVariableResolverProvider` de um host sem o `IConfigurationResolverService`: o `$substituteVariables` rejeita.
pub struct NoVariableResolver;

impl IExtHostVariableResolverProvider for NoVariableResolver {
    fn get_resolver(&self) -> Promise<Rc<dyn IConfigurationResolver>> {
        Promise::rejected(Error::new("o host não tem o resolvedor de variáveis de configuração: ${variable} não pode ser substituída"))
    }
}

/// O `IExtHostTesting` de um host sem o namespace `tests`: nenhuma `TestRun` existe, então nenhuma execução tem metadados.
pub struct NoTesting;

impl IExtHostDebugTesting for NoTesting {
    fn get_metadata_for_run(&self, _run: &Rc<TestRun>) -> Option<(String, String)> {
        None
    }
}

/// O `IMainContext` do `ExtHostDebugService`: o proxy do `MainThreadDebugService` e o de telemetria, os dois que o construtor pede.
pub struct DebugContext {
    pub debug: Rc<dyn MainThreadDebugServiceShape>,
    pub telemetry: Rc<dyn MainThreadTelemetryShape>,
}

impl IMainContext for DebugContext {
    fn get_proxy_erased(&self, sid: &'static str) -> Rc<dyn Any> {
        if sid == MainContext::MAIN_THREAD_DEBUG_SERVICE.sid {
            Rc::new(self.debug.clone())
        } else if sid == MainContext::MAIN_THREAD_TELEMETRY.sid {
            Rc::new(self.telemetry.clone())
        } else {
            panic!("o ExtHostDebugService não pede o proxy '{sid}'")
        }
    }

    fn set_erased(&self, _sid: &'static str, _value: Rc<dyn Any>) {}
}

// ---- JSON

/// O valor do motor como JSON (a configuração de depuração): funções e `undefined` saem, como no `JSON.stringify`.
fn js_to_json(value: &JsData) -> Value {
    match value {
        JsData::Undefined | JsData::Null | JsData::Handle(_) => Value::Null,
        JsData::Bool(flag) => Value::Bool(*flag),
        JsData::Num(number) if number.fract() == 0.0 && number.abs() < 9.0e15 => Value::Number(Number::from(*number as i64)),
        JsData::Num(number) => Number::from_f64(*number).map_or(Value::Null, Value::Number),
        JsData::Str(text) => Value::String(text.clone()),
        JsData::Array(items) => Value::Array(items.iter().map(js_to_json).collect()),
        JsData::Object(fields) => Value::Object(
            fields
                .iter()
                .filter(|(key, item)| key != CLASS_KEY && !matches!(item, JsData::Handle(_) | JsData::Undefined))
                .map(|(key, item)| (key.clone(), js_to_json(item)))
                .collect::<Map<String, Value>>(),
        ),
    }
}

/// O objeto da extensão (entregue por referência) como dado: a cópia que o motor faz dele.
fn exported(runner: &NodeRunner, value: &JsData) -> JsData {
    match value {
        JsData::Handle(handle) => runner.export(handle),
        other => other.clone(),
    }
}

fn config_of(runner: &NodeRunner, value: &JsData) -> IConfig {
    js_to_json(&exported(runner, value))
}

/// `MaybePromise<T>` com `convert` (que pode falhar) aplicado ao valor quando ele chegar.
fn convert_maybe<T: Clone + 'static, U: Clone + 'static>(value: MaybePromise<T>, convert: impl Fn(T) -> Result<U, Error> + 'static) -> MaybePromise<U> {
    match value {
        MaybePromise::Value(value) => match convert(value) {
            Ok(converted) => MaybePromise::Value(converted),
            Err(error) => MaybePromise::Promise(Promise::rejected(error)),
        },
        MaybePromise::Promise(promise) => MaybePromise::Promise(promise.then_result(move |outcome| convert(outcome?))),
    }
}

// ---- os objetos vivos

fn object_of(class: &str, id: &str) -> JsData {
    JsData::Object(vec![(CLASS_KEY.to_owned(), JsData::Str(class.to_owned())), ("_id".to_owned(), JsData::Str(id.to_owned()))])
}

fn id_of(this: &JsData) -> Result<&str, String> {
    match field(this, "_id") {
        Some(JsData::Str(id)) => Ok(id.as_str()),
        _ => Err("debug: this inválido".to_owned()),
    }
}

/// As sessões e os breakpoints que a extensão enxerga, por id (os objetos JavaScript carregam só o `_id`).
#[derive(Default)]
pub struct DebugObjects {
    sessions: RefCell<HashMap<String, Rc<DebugSession>>>,
    breakpoints: RefCell<HashMap<String, Rc<Breakpoint>>>,
}

impl DebugObjects {
    fn session_to_js(&self, session: &Rc<DebugSession>) -> JsData {
        let id = session.id();
        self.sessions.borrow_mut().insert(id.clone(), session.clone());
        object_of("DebugSession", &id)
    }

    fn session_of(&self, value: &JsData) -> Result<Rc<DebugSession>, String> {
        self.sessions.borrow().get(id_of(value)?).cloned().ok_or_else(|| "DebugSession: sessão desconhecida".to_owned())
    }

    fn remember_breakpoint(&self, breakpoint: &Rc<Breakpoint>) -> String {
        let id = breakpoint.id();
        self.breakpoints.borrow_mut().insert(id.clone(), breakpoint.clone());
        id
    }

    /// A instância que `new SourceBreakpoint(...)` devolve: o motor dá o protótipo da classe.
    fn new_breakpoint(&self, breakpoint: &Rc<Breakpoint>) -> JsData {
        JsData::Object(vec![("_id".to_owned(), JsData::Str(self.remember_breakpoint(breakpoint)))])
    }

    fn breakpoint_to_js(&self, breakpoint: &Rc<Breakpoint>) -> JsData {
        let class = match breakpoint.kind() {
            BreakpointKind::Source { .. } => "SourceBreakpoint",
            BreakpointKind::Function { .. } => "FunctionBreakpoint",
            BreakpointKind::Data { .. } => "Breakpoint",
        };
        object_of(class, &self.remember_breakpoint(breakpoint))
    }

    fn breakpoint_of(&self, value: &JsData) -> Result<Rc<Breakpoint>, String> {
        self.breakpoints.borrow().get(id_of(value)?).cloned().ok_or_else(|| "Breakpoint: o breakpoint não foi criado por este host".to_owned())
    }

    fn breakpoints_to_js(&self, breakpoints: &[Rc<Breakpoint>]) -> JsData {
        JsData::Array(breakpoints.iter().map(|breakpoint| self.breakpoint_to_js(breakpoint)).collect())
    }
}

// ---- as classes

fn text_or_undefined(value: Option<String>) -> JsData {
    value.map_or(JsData::Undefined, JsData::Str)
}

fn define_session_class(runner: &NodeRunner, objects: &Rc<DebugObjects>) {
    let get = |name: &str, read: fn(&DebugObjects, &DebugSession) -> JsData| -> (String, MethodCallback) {
        let objects = objects.clone();
        (name.to_owned(), Rc::new(move |this, _| Ok(read(&objects, &*objects.session_of(this)?))))
    };
    let getters: Vec<(String, MethodCallback)> = vec![
        get("id", |_, session| JsData::Str(session.id())),
        get("type", |_, session| JsData::Str(session.session_type())),
        get("name", |_, session| JsData::Str(session.name())),
        get("workspaceFolder", |_, session| session.workspace_folder().map_or(JsData::Undefined, |folder| folder_to_js(&folder))),
        get("configuration", |_, session| json_to_js(&session.configuration())),
        get("parentSession", |objects, session| session.parent_session().map_or(JsData::Undefined, |parent| objects.session_to_js(&parent))),
    ];

    let name_objects = objects.clone();
    let setters: Vec<(String, MethodCallback)> = vec![(
        "name".to_owned(),
        Rc::new(move |this, args| {
            name_objects.session_of(this)?.set_name(&opt_string(args.first()).unwrap_or_default());
            Ok(JsData::Undefined)
        }),
    )];

    let (request_runner, request_objects) = (runner.clone(), objects.clone());
    let (protocol_runner, protocol_objects) = (runner.clone(), objects.clone());
    let reply = |value: &Option<Value>| value.as_ref().map_or(JsData::Undefined, json_to_js);
    let methods: Vec<(String, u32, MethodCallback)> = vec![
        (
            "customRequest".to_owned(),
            2,
            Rc::new(move |this, args| {
                let session = request_objects.session_of(this)?;
                let command = string_arg(args, 0, "customRequest: o comando")?;
                let request_args = args.get(1).filter(|value| !is_nullish(Some(*value))).map(js_to_json);
                Ok(promise_to_js(&request_runner, session.custom_request(&command, request_args), reply))
            }),
        ),
        (
            "getDebugProtocolBreakpoint".to_owned(),
            1,
            Rc::new(move |this, args| {
                let session = protocol_objects.session_of(this)?;
                let breakpoint = protocol_objects.breakpoint_of(args.first().ok_or("getDebugProtocolBreakpoint: falta o breakpoint")?)?;
                Ok(promise_to_js(&protocol_runner, session.get_debug_protocol_breakpoint(&breakpoint), reply))
            }),
        ),
    ];
    runner.define_accessor_class(
        ClassSpec {
            name: "DebugSession".to_owned(),
            length: 0,
            construct: Rc::new(|_| Err("DebugSession não tem construtor público: use debug.startDebugging".to_owned())),
            methods,
            getters,
            statics: vec![],
            parent: None,
        },
        setters,
    );
}

fn define_breakpoint_classes(runner: &NodeRunner, objects: &Rc<DebugObjects>) -> Vec<(String, JsData)> {
    let get = |name: &str, read: fn(&Breakpoint) -> JsData| -> (String, MethodCallback) {
        let objects = objects.clone();
        (name.to_owned(), Rc::new(move |this, _| Ok(read(&*objects.breakpoint_of(this)?))))
    };
    let base = runner.define_class(ClassSpec {
        name: "Breakpoint".to_owned(),
        length: 0,
        construct: Rc::new(|_| Err("Breakpoint não tem construtor público: use SourceBreakpoint ou FunctionBreakpoint".to_owned())),
        methods: vec![],
        getters: vec![
            get("id", |breakpoint| JsData::Str(breakpoint.id())),
            get("enabled", |breakpoint| JsData::Bool(breakpoint.enabled())),
            get("condition", |breakpoint| text_or_undefined(breakpoint.condition())),
            get("hitCondition", |breakpoint| text_or_undefined(breakpoint.hit_condition())),
            get("logMessage", |breakpoint| text_or_undefined(breakpoint.log_message())),
        ],
        statics: vec![],
        parent: None,
    });

    let source_objects = objects.clone();
    let source = runner.define_class(ClassSpec {
        name: "SourceBreakpoint".to_owned(),
        length: 5,
        construct: Rc::new(move |args| {
            let location = location_from_js(args.first().filter(|value| !is_nullish(Some(*value))).ok_or("Illegal argument: location")?).map_err(|error| error.message)?;
            let breakpoint = Breakpoint::new_source(location, opt_bool(args.get(1)), opt_string(args.get(2)), opt_string(args.get(3)), opt_string(args.get(4)), None);
            Ok(source_objects.new_breakpoint(&breakpoint))
        }),
        methods: vec![],
        getters: vec![get("location", |breakpoint| match breakpoint.kind() {
            BreakpointKind::Source { location } => api_to_js(&location),
            _ => JsData::Undefined,
        })],
        statics: vec![],
        parent: Some("Breakpoint".to_owned()),
    });

    let function_objects = objects.clone();
    let function = runner.define_class(ClassSpec {
        name: "FunctionBreakpoint".to_owned(),
        length: 5,
        construct: Rc::new(move |args| {
            let function_name = string_arg(args, 0, "FunctionBreakpoint: o functionName")?;
            let breakpoint = Breakpoint::new_function(&function_name, opt_bool(args.get(1)), opt_string(args.get(2)), opt_string(args.get(3)), opt_string(args.get(4)), None);
            Ok(function_objects.new_breakpoint(&breakpoint))
        }),
        methods: vec![],
        getters: vec![get("functionName", |breakpoint| match breakpoint.kind() {
            BreakpointKind::Function { function_name } => JsData::Str(function_name),
            _ => JsData::Undefined,
        })],
        statics: vec![],
        parent: Some("Breakpoint".to_owned()),
    });
    vec![
        ("Breakpoint".to_owned(), JsData::Handle(Rc::new(base))),
        ("SourceBreakpoint".to_owned(), JsData::Handle(Rc::new(source))),
        ("FunctionBreakpoint".to_owned(), JsData::Handle(Rc::new(function))),
    ]
}

/// `new DebugAdapterExecutable(command, args?, options?)`.
fn executable_fields(command: &str, args: &[String], options: Option<&IDebugAdapterExecutableOptions>) -> Vec<(String, JsData)> {
    let mut fields = vec![
        ("command".to_owned(), JsData::Str(command.to_owned())),
        ("args".to_owned(), JsData::Array(args.iter().cloned().map(JsData::Str).collect())),
    ];
    if let Some(options) = options {
        let mut entries: Vec<(String, JsData)> = Vec::new();
        if let Some(cwd) = &options.cwd {
            entries.push(("cwd".to_owned(), JsData::Str(cwd.clone())));
        }
        if let Some(env) = &options.env {
            entries.push(("env".to_owned(), JsData::Object(env.iter().map(|(key, value)| (key.clone(), JsData::Str(value.clone()))).collect())));
        }
        fields.push(("options".to_owned(), JsData::Object(entries)));
    }
    fields
}

fn executable_to_js(executable: &DebugAdapterExecutable) -> JsData {
    let mut fields = vec![(CLASS_KEY.to_owned(), JsData::Str("DebugAdapterExecutable".to_owned()))];
    fields.extend(executable_fields(&executable.command, &executable.args, executable.options.as_ref()));
    JsData::Object(fields)
}

fn strings_of(value: Option<&JsData>) -> Vec<String> {
    match value {
        Some(JsData::Array(items)) => items.iter().filter_map(|item| opt_string(Some(item))).collect(),
        _ => Vec::new(),
    }
}

fn executable_options_of(value: Option<&JsData>) -> Option<IDebugAdapterExecutableOptions> {
    let options = value.filter(|options| matches!(options, JsData::Object(_)))?;
    let env = match field(options, "env") {
        Some(JsData::Object(entries)) => Some(
            entries
                .iter()
                .filter(|(key, _)| key != CLASS_KEY)
                .filter_map(|(key, value)| opt_string(Some(value)).map(|value| (key.clone(), value)))
                .collect(),
        ),
        _ => None,
    };
    Some(IDebugAdapterExecutableOptions { cwd: opt_string(field(options, "cwd")), env })
}

fn define_descriptor_classes(runner: &NodeRunner) -> Vec<(String, JsData)> {
    let class = |name: &str, length: u32, construct: NativeCallback| {
        let constructor = runner.define_class(ClassSpec { name: name.to_owned(), length, construct, methods: vec![], getters: vec![], statics: vec![], parent: None });
        (name.to_owned(), JsData::Handle(Rc::new(constructor)))
    };
    vec![
        class(
            "DebugAdapterExecutable",
            3,
            Rc::new(|args| {
                let command = string_arg(args, 0, "DebugAdapterExecutable: o command")?;
                Ok(JsData::Object(executable_fields(&command, &strings_of(args.get(1)), executable_options_of(args.get(2)).as_ref())))
            }),
        ),
        class(
            "DebugAdapterServer",
            2,
            Rc::new(|args| {
                let port = opt_number(args.first()).ok_or("DebugAdapterServer: a porta precisa ser um número")?;
                let mut fields = vec![("port".to_owned(), JsData::Num(port))];
                if let Some(host) = opt_string(args.get(1)) {
                    fields.push(("host".to_owned(), JsData::Str(host)));
                }
                Ok(JsData::Object(fields))
            }),
        ),
        class(
            "DebugAdapterNamedPipeServer",
            1,
            Rc::new(|args| Ok(JsData::Object(vec![("path".to_owned(), JsData::Str(string_arg(args, 0, "DebugAdapterNamedPipeServer: o path")?))]))),
        ),
        class(
            "DebugAdapterInlineImplementation",
            1,
            Rc::new(|_| {
                Err("DebugAdapterInlineImplementation ainda não é suportado pelo host: o adaptador chegaria ao construtor por cópia e perderia as funções dele".to_owned())
            }),
        ),
    ]
}

/// As classes de `vscode.debug` como entradas do objeto `vscode`; define também o protótipo de `DebugSession`.
pub fn classes(runner: &NodeRunner, objects: &Rc<DebugObjects>) -> Vec<(String, JsData)> {
    define_session_class(runner, objects);
    let mut members = define_breakpoint_classes(runner, objects);
    members.extend(define_descriptor_classes(runner));
    members
}

// ---- o provedor de configuração e a fábrica de descritor da extensão

/// O que os objetos da extensão precisam para ser chamados.
#[derive(Clone)]
struct ProviderContext {
    js: JsObjects,
    events: Rc<EventTables>,
    objects: Rc<DebugObjects>,
}

fn folder_arg(folder: Option<Rc<WorkspaceFolder>>) -> JsData {
    folder.map_or(JsData::Undefined, |folder| folder_to_js(&folder))
}

/// O `vscode.DebugConfigurationProvider` da extensão.
struct JsConfigurationProvider {
    context: ProviderContext,
    target: JsData,
}

impl JsConfigurationProvider {
    fn has(&self, method: &str) -> bool {
        self.context.js.is_function(&self.target, method)
    }

    /// `resolveDebugConfiguration*(folder, config, token)`.
    fn resolve(&self, method: &str, folder: Option<Rc<WorkspaceFolder>>, config: IConfig, token: CancellationToken) -> ProviderResult<IConfig> {
        let args = vec![folder_arg(folder), json_to_js(&config), self.context.events.token_to_js(token)];
        let runner = self.context.js.runner().clone();
        let method = method.to_owned();
        convert_maybe(self.context.js.invoke(&self.target, &method, args), move |result| match result {
            JsResult::Nullish => Ok(None),
            JsResult::Single(value) => Ok(Some(config_of(&runner, &value))),
            JsResult::Many(_) => Err(Error::new(format!("{method} precisa devolver uma configuração, não uma lista"))),
        })
    }
}

impl DebugConfigurationProvider for JsConfigurationProvider {
    fn has_provide_debug_configurations(&self) -> bool {
        self.has("provideDebugConfigurations")
    }

    fn has_resolve_debug_configuration(&self) -> bool {
        self.has("resolveDebugConfiguration")
    }

    fn has_resolve_debug_configuration_with_substituted_variables(&self) -> bool {
        self.has("resolveDebugConfigurationWithSubstitutedVariables")
    }

    fn provide_debug_configurations(&self, folder: Option<Rc<WorkspaceFolder>>, token: CancellationToken) -> ProviderResult<Vec<IConfig>> {
        let args = vec![folder_arg(folder), self.context.events.token_to_js(token)];
        let runner = self.context.js.runner().clone();
        convert_maybe(self.context.js.invoke(&self.target, "provideDebugConfigurations", args), move |result| {
            Ok(match result {
                JsResult::Nullish => None,
                JsResult::Single(value) => Some(vec![config_of(&runner, &value)]),
                JsResult::Many(values) => Some(values.iter().map(|value| config_of(&runner, value)).collect()),
            })
        })
    }

    fn resolve_debug_configuration(&self, folder: Option<Rc<WorkspaceFolder>>, debug_configuration: IConfig, token: CancellationToken) -> ProviderResult<IConfig> {
        self.resolve("resolveDebugConfiguration", folder, debug_configuration, token)
    }

    fn resolve_debug_configuration_with_substituted_variables(&self, folder: Option<Rc<WorkspaceFolder>>, debug_configuration: IConfig, token: CancellationToken) -> ProviderResult<IConfig> {
        self.resolve("resolveDebugConfigurationWithSubstitutedVariables", folder, debug_configuration, token)
    }
}

/// O descritor que a fábrica devolveu: `DebugAdapterExecutable`, `DebugAdapterServer` ou `DebugAdapterNamedPipeServer`.
fn descriptor_of(runner: &NodeRunner, value: &JsData) -> Result<DebugAdapterDescriptor, Error> {
    let data = exported(runner, value);
    let kind = match class_name(&data) {
        Some(name) => name.to_owned(),
        None if field(&data, "command").is_some() => "DebugAdapterExecutable".to_owned(),
        None if field(&data, "port").is_some() => "DebugAdapterServer".to_owned(),
        None if field(&data, "path").is_some() => "DebugAdapterNamedPipeServer".to_owned(),
        None => String::new(),
    };
    match kind.as_str() {
        "DebugAdapterExecutable" => {
            let command = opt_string(field(&data, "command")).ok_or_else(|| Error::new("DebugAdapterExecutable: falta o command"))?;
            Ok(DebugAdapterDescriptor::Executable(DebugAdapterExecutable::new(&command, Some(strings_of(field(&data, "args"))), executable_options_of(field(&data, "options")))))
        }
        "DebugAdapterServer" => {
            let port = opt_number(field(&data, "port")).ok_or_else(|| Error::new("DebugAdapterServer: falta a porta"))?;
            Ok(DebugAdapterDescriptor::Server(DebugAdapterServer { port, host: opt_string(field(&data, "host")) }))
        }
        "DebugAdapterNamedPipeServer" => {
            let path = opt_string(field(&data, "path")).ok_or_else(|| Error::new("DebugAdapterNamedPipeServer: falta o path"))?;
            Ok(DebugAdapterDescriptor::NamedPipeServer(DebugAdapterNamedPipeServer { path }))
        }
        _ => Err(Error::new("a fábrica devolveu um descritor de adaptador desconhecido (use DebugAdapterExecutable, DebugAdapterServer ou DebugAdapterNamedPipeServer)")),
    }
}

/// O `vscode.DebugAdapterDescriptorFactory` da extensão.
struct JsDescriptorFactory {
    context: ProviderContext,
    target: JsData,
}

impl DebugAdapterDescriptorFactory for JsDescriptorFactory {
    fn create_debug_adapter_descriptor(&self, session: Rc<DebugSession>, executable: Option<DebugAdapterExecutable>) -> ProviderResult<DebugAdapterDescriptor> {
        let args = vec![self.context.objects.session_to_js(&session), executable.as_ref().map_or(JsData::Undefined, executable_to_js)];
        let runner = self.context.js.runner().clone();
        convert_maybe(self.context.js.invoke(&self.target, "createDebugAdapterDescriptor", args), move |result| match result {
            JsResult::Nullish => Ok(None),
            JsResult::Single(value) => descriptor_of(&runner, &value).map(Some),
            JsResult::Many(_) => Err(Error::new("createDebugAdapterDescriptor precisa devolver um descritor, não uma lista")),
        })
    }
}

// ---- os argumentos de startDebugging

fn folder_of(value: Option<&JsData>) -> Result<Option<Rc<WorkspaceFolder>>, String> {
    match value {
        Some(folder @ JsData::Object(_)) => {
            let uri = uri_from_js(field(folder, "uri").ok_or("startDebugging: a pasta precisa ter a uri")?)?;
            let index = opt_number(field(folder, "index")).unwrap_or(0.0) as usize;
            Ok(Some(WorkspaceFolder::new(uri, opt_string(field(folder, "name")).unwrap_or_default(), index)))
        }
        _ => Ok(None),
    }
}

/// O terceiro argumento de `startDebugging`: a sessão pai ou as `DebugSessionOptions` (a mesma distinção do original).
fn session_options_of(objects: &DebugObjects, value: Option<&JsData>) -> Result<DebugSessionOptions, String> {
    let Some(value @ JsData::Object(_)) = value else { return Ok(DebugSessionOptions::default()) };
    if class_name(value) == Some("DebugSession") {
        return Ok(DebugSessionOptions { parent_session: Some(objects.session_of(value)?), ..DebugSessionOptions::default() });
    }
    let parent_session = match field(value, "parentSession") {
        Some(parent @ JsData::Object(_)) => Some(objects.session_of(parent)?),
        _ => None,
    };
    let console_mode = match opt_number(field(value, "consoleMode")) {
        Some(mode) if mode == 1.0 => Some(DebugConsoleMode::MergeWithParent),
        Some(_) => Some(DebugConsoleMode::Separate),
        None => None,
    };
    let flag = |name: &str| opt_bool(field(value, name));
    Ok(DebugSessionOptions {
        parent_session,
        lifecycle_managed_by_parent: flag("lifecycleManagedByParent"),
        console_mode,
        no_debug: flag("noDebug"),
        compact: flag("compact"),
        suppress_save_before_start: flag("suppressSaveBeforeStart"),
        suppress_debug_toolbar: flag("suppressDebugToolbar"),
        suppress_debug_statusbar: flag("suppressDebugStatusbar"),
        suppress_debug_view: flag("suppressDebugView"),
        debug_ui_simple: field(value, "debugUI").and_then(|ui| opt_bool(field(ui, "simple"))),
        test_run: None,
    })
}

fn breakpoints_arg(objects: &DebugObjects, args: &[JsData], what: &str) -> Result<Vec<Rc<Breakpoint>>, String> {
    match args.first() {
        Some(JsData::Array(items)) => items.iter().map(|item| objects.breakpoint_of(item)).collect(),
        _ => Err(format!("{what}: a lista de breakpoints é obrigatória")),
    }
}

// ---- o namespace

/// Os serviços do namespace `debug`.
pub struct DebugServices {
    pub service: Rc<ExtHostDebugServiceBase>,
    pub extensions: Rc<ExtensionServiceSlot>,
}

fn event_getter<T: 'static>(
    runner: &NodeRunner,
    table: &Rc<DisposableTable>,
    service: &Rc<ExtHostDebugServiceBase>,
    name: &'static str,
    source: fn(&ExtHostDebugServiceBase) -> Event<T>,
    to_js: impl Fn(&T) -> JsData + Clone + 'static,
) -> (String, MethodCallback) {
    let (runner, table, service) = (runner.clone(), table.clone(), service.clone());
    (name.to_owned(), Rc::new(move |_, _| Ok(event_function(&runner, &table, name, source(&service), to_js.clone()))))
}

/// O namespace `debug`.
pub fn namespace(
    runner: &NodeRunner,
    js: &JsObjects,
    services: &DebugServices,
    objects: &Rc<DebugObjects>,
    table: &Rc<DisposableTable>,
    events: &Rc<EventTables>,
) -> JsData {
    let service = &services.service;

    let (session_objects, session_service) = (objects.clone(), service.clone());
    let (console_runner, console_service) = (runner.clone(), service.clone());
    let (breakpoint_objects, breakpoint_service) = (objects.clone(), service.clone());
    let (start_objects, terminate_objects, active_objects, custom_objects, change_objects) = (objects.clone(), objects.clone(), objects.clone(), objects.clone(), objects.clone());
    let getters: Vec<(String, MethodCallback)> = vec![
        (
            "activeDebugSession".to_owned(),
            Rc::new(move |_, _| Ok(session_service.active_debug_session().map_or(JsData::Undefined, |session| session_objects.session_to_js(&session)))),
        ),
        (
            "activeDebugConsole".to_owned(),
            Rc::new(move |_, _| {
                let console = console_service.active_debug_console();
                let (append_console, line_console) = (console.clone(), console);
                let append = console_runner.make_function(
                    "append",
                    1,
                    Rc::new(move |args| {
                        append_console.append(&string_arg(args, 0, "append: o texto")?);
                        Ok(JsData::Undefined)
                    }),
                );
                let append_line = console_runner.make_function(
                    "appendLine",
                    1,
                    Rc::new(move |args| {
                        line_console.append_line(&string_arg(args, 0, "appendLine: o texto")?);
                        Ok(JsData::Undefined)
                    }),
                );
                Ok(JsData::Object(vec![
                    ("append".to_owned(), JsData::Handle(Rc::new(append))),
                    ("appendLine".to_owned(), JsData::Handle(Rc::new(append_line))),
                ]))
            }),
        ),
        ("breakpoints".to_owned(), Rc::new(move |_, _| Ok(breakpoint_objects.breakpoints_to_js(&breakpoint_service.breakpoints())))),
        event_getter(runner, table, service, "onDidStartDebugSession", ExtHostDebugServiceBase::on_did_start_debug_session, move |session: &Rc<DebugSession>| {
            start_objects.session_to_js(session)
        }),
        event_getter(runner, table, service, "onDidTerminateDebugSession", ExtHostDebugServiceBase::on_did_terminate_debug_session, move |session: &Rc<DebugSession>| {
            terminate_objects.session_to_js(session)
        }),
        event_getter(
            runner,
            table,
            service,
            "onDidChangeActiveDebugSession",
            ExtHostDebugServiceBase::on_did_change_active_debug_session,
            move |session: &Option<Rc<DebugSession>>| session.as_ref().map_or(JsData::Undefined, |session| active_objects.session_to_js(session)),
        ),
        event_getter(
            runner,
            table,
            service,
            "onDidReceiveDebugSessionCustomEvent",
            ExtHostDebugServiceBase::on_did_receive_debug_session_custom_event,
            move |event: &DebugSessionCustomEvent| {
                JsData::Object(vec![
                    ("session".to_owned(), custom_objects.session_to_js(&event.session)),
                    ("event".to_owned(), JsData::Str(event.event.clone())),
                    ("body".to_owned(), event.body.as_ref().map_or(JsData::Undefined, json_to_js)),
                ])
            },
        ),
        event_getter(runner, table, service, "onDidChangeBreakpoints", ExtHostDebugServiceBase::on_did_change_breakpoints, move |event: &BreakpointsChangeEvent| {
            JsData::Object(vec![
                ("added".to_owned(), change_objects.breakpoints_to_js(&event.added)),
                ("removed".to_owned(), change_objects.breakpoints_to_js(&event.removed)),
                ("changed".to_owned(), change_objects.breakpoints_to_js(&event.changed)),
            ])
        }),
    ];

    let (start_runner, start_service, start_session_objects) = (runner.clone(), service.clone(), objects.clone());
    let (stop_runner, stop_service, stop_objects) = (runner.clone(), service.clone(), objects.clone());
    let (add_runner, add_service, add_objects) = (runner.clone(), service.clone(), objects.clone());
    let (remove_runner, remove_service, remove_objects) = (runner.clone(), service.clone(), objects.clone());
    let methods: Vec<(String, u32, MethodCallback)> = vec![
        (
            "startDebugging".to_owned(),
            3,
            Rc::new(move |_, args| {
                let folder = folder_of(args.first())?;
                let name_or_config = match args.get(1) {
                    Some(JsData::Str(name)) => NameOrConfig::Name(name.clone()),
                    Some(config @ JsData::Object(_)) => NameOrConfig::Config(js_to_json(config)),
                    _ => return Err("startDebugging: o segundo argumento precisa ser o nome ou a configuração".to_owned()),
                };
                let options = session_options_of(&start_session_objects, args.get(2))?;
                Ok(promise_to_js(&start_runner, start_service.start_debugging(folder, name_or_config, options), |started| JsData::Bool(*started)))
            }),
        ),
        (
            "stopDebugging".to_owned(),
            1,
            Rc::new(move |_, args| {
                let session = match args.first() {
                    Some(session @ JsData::Object(_)) => Some(stop_objects.session_of(session)?),
                    _ => None,
                };
                Ok(promise_to_js(&stop_runner, stop_service.stop_debugging(session.as_deref()), |()| JsData::Undefined))
            }),
        ),
        (
            "addBreakpoints".to_owned(),
            1,
            Rc::new(move |_, args| {
                let breakpoints = breakpoints_arg(&add_objects, args, "addBreakpoints")?;
                Ok(promise_to_js(&add_runner, add_service.add_breakpoints(breakpoints), |()| JsData::Undefined))
            }),
        ),
        (
            "removeBreakpoints".to_owned(),
            1,
            Rc::new(move |_, args| {
                let breakpoints = breakpoints_arg(&remove_objects, args, "removeBreakpoints")?;
                Ok(promise_to_js(&remove_runner, remove_service.remove_breakpoints(breakpoints), |()| JsData::Undefined))
            }),
        ),
    ];

    // Os provedores são objetos da extensão (o `this` e o protótipo contam): funções por referência, próprias do namespace.
    let context = ProviderContext { js: js.clone(), events: events.clone(), objects: objects.clone() };
    let reference = |name: &str, length: u32, body: NativeCallback| (name.to_owned(), JsData::Handle(Rc::new(runner.make_function_by_reference(name, length, body))));
    let (config_service, config_table, config_context) = (service.clone(), table.clone(), context.clone());
    let (factory_service, factory_table, factory_extensions) = (service.clone(), table.clone(), services.extensions.clone());
    let references = vec![
        reference(
            "registerDebugConfigurationProvider",
            3,
            Rc::new(move |args| {
                let debug_type = string_arg(args, 0, "registerDebugConfigurationProvider: o tipo")?;
                let provider: Option<Rc<dyn DebugConfigurationProvider>> = match args.get(1) {
                    Some(target @ JsData::Handle(_)) => Some(Rc::new(JsConfigurationProvider { context: config_context.clone(), target: target.clone() })),
                    _ => None,
                };
                let trigger =
                    if opt_number(args.get(2)) == Some(2.0) { DebugConfigurationProviderTriggerKind::Dynamic } else { DebugConfigurationProviderTriggerKind::Initial };
                Ok(config_table.to_js(config_service.register_debug_configuration_provider(&debug_type, provider, trigger)))
            }),
        ),
        reference(
            "registerDebugAdapterDescriptorFactory",
            2,
            Rc::new(move |args| {
                let debug_type = string_arg(args, 0, "registerDebugAdapterDescriptorFactory: o tipo")?;
                let factory: Option<Rc<dyn DebugAdapterDescriptorFactory>> = match args.get(1) {
                    Some(target @ JsData::Handle(_)) => Some(Rc::new(JsDescriptorFactory { context: context.clone(), target: target.clone() })),
                    _ => None,
                };
                let extension = factory_extensions
                    .extensions()
                    .into_iter()
                    .find(|extension| ExtHostDebugServiceBase::defines_debug_type(extension, &debug_type))
                    .unwrap_or_else(host_extension);
                let registration = factory_service.register_debug_adapter_descriptor_factory(&extension, &debug_type, factory).map_err(|error| error.message)?;
                Ok(factory_table.to_js(registration))
            }),
        ),
    ];

    match accessor_namespace(runner, "debug", methods, getters) {
        JsData::Object(mut fields) => {
            fields.extend(references);
            JsData::Object(fields)
        }
        other => other,
    }
}
