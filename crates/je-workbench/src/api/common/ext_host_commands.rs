//! Porte de `src/vs/workbench/api/common/extHostCommands.ts`: o lado da extensão dos comandos
//! (`vscode.commands.registerCommand`, `executeCommand`, os comandos de API e a conversão de `vscode.Command`).
//!
//! Divergências:
//! - O `callback: <T>(...args: any[]) => T | Thenable<T>` com `thisArg` é [`CommandCallback`]: recebe os
//!   argumentos como `&[Dynamic]` e devolve [`MaybePromise<Dynamic>`] (o valor direto ou uma promessa). O `thisArg`
//!   não existe: o fecho captura o que precisa. Uma exceção lançada (pânico com [`Error`]) pelo callback é
//!   capturada e vira rejeição, como o `try/catch` do original.
//! - Os métodos `registerCommand` e `registerApiCommand`, que lançam no original (`invalid id`,
//!   `command 'x' already exists`), devolvem [`Result`]. O `extHostTypes.Disposable` que eles devolvem é o
//!   [`Disposable`](types::Disposable) de `ext_host_types`.
//! - `ExtHostCommands` e `CommandsConverter` se referem de volta (o conversor registra o comando delegante e
//!   consulta os comandos de API). Cada um guarda um [`Weak`] do outro; o [`Rc`] mora no chamador.
//! - Os `any` do protocolo são [`Dynamic`]; as classes da API cruzam marcadas com `$$class` (ver
//!   `ext_host_values`), então os `instanceof` de `cloneAndChange` são [`is_instance_of`]. O `ArrayBuffer`,
//!   o `Uint8Array` e o `VSBuffer` seguem a representação de `ext_host_values` (`ArrayBuffer` e `VSBuffer`
//!   são objetos marcados, o `Uint8Array` é [`Dynamic::TypedArray`]).
//! - `ApiCommandArgument<V, O>` e `ApiCommandResult<V, O>` trabalham sobre [`Dynamic`], sem os genéricos.
//!   As estáticas (`ApiCommandArgument.Uri`, `.Position`...) são funções (`uri()`, `position()`...), porque um
//!   fecho não cabe numa constante; `ApiCommandResult.Void` é [`ApiCommandResult::void`], e o
//!   `candidate?.result === ApiCommandResult.Void` é [`ApiCommandResult::is_void`] (a identidade da estática
//!   não existe). `ApiCommandArgument.TestItem` e `.TestProfile` NÃO estão portados: `TestItemImpl` e
//!   `TestRunProfileBase` (testing) ainda não existem no porte.
//! - A validação por duck typing dos argumentos de API (`Position.isPosition`, `Range.isRange`,
//!   `Selection.isSelection`) aceita a classe marcada ou um objeto com as propriedades certas, como o original.
//! - O `Promise.reject(string)` de `_executeConvertedCommand` rejeita com um [`Error`] com essa mensagem.
//! - O `CommandError` anônimo (com `id` e `source`) é um [`Error`] com as propriedades `id` e `source`.
//! - `IExtensionDescription` é o `ExtensionDescription` de `je_extensions`.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::{Rc, Weak};

use je_base::async_::{async_fn, async_fn_maybe, await_maybe};
use je_base::buffer::VSBuffer;
use je_base::dynamic::{number_to_js_string, Dynamic};
use je_base::error_message::to_error_message_of;
use je_base::errors::{self, is_cancellation_error, Error};
use je_base::lifecycle::{to_disposable, DisposableStore, IDisposable};
use je_base::objects::clone_and_change;
use je_base::promise::{MaybePromise, Promise};
use je_base::stopwatch::StopWatch;
use je_base::types::validate_constraint;
use je_base::uuid::generate_uuid;
use je_editor::position::Position as EditorPosition;
use je_editor::range::Range as EditorRange;
use je_editor::selection::Selection as EditorSelection;
use je_extensions::scanner::ExtensionDescription;
use je_platform::commands::commands::{CommandArgument, CommandMetadata};
use je_platform::log::{LogArg, LogMessage, LogServiceRef, Logger};
use je_platform::telemetry::telemetry_utils::TelemetryTrustedValue;

use super::ext_host_protocol::{
    CommandArgs, CommandMetadataArgDto, ExtHostCommandsShape, ICommandDto, ICommandMetadataDto, IExtHostTelemetry, IMainContext, MainContext,
    MainThreadCommandsShape, MainThreadTelemetryShape, SerializableObjectWithBuffers,
};
use super::ext_host_type_converters::{self as type_converters, ICommandsConverter};
use super::ext_host_types::{self as types, dynamic_to_json, json_stringify_tabs, Command};
use super::ext_host_values::{bytes_of, class_of, is_instance_of, revive, vs_buffer_from_dynamic, vs_buffer_to_dynamic, ApiValue};

/// `callback` de um comando: os argumentos e o retorno `T | Thenable<T>`.
pub type CommandCallback = Rc<dyn Fn(&[Dynamic]) -> MaybePromise<Dynamic>>;

/// `CommandHandler` (privado no original): o que o host guarda de cada comando registrado.
struct CommandRegistration {
    callback: CommandCallback,
    metadata: Option<CommandMetadata>,
    extension: Option<ExtensionDescription>,
}

/// `ArgumentProcessor`.
pub trait ArgumentProcessor {
    fn process_argument(&self, arg: &Dynamic, extension: Option<&ExtensionDescription>) -> Dynamic;
}

/// O processador de argumentos que só aplica o `revive` de `marshalling.ts` (URI, RegExp, Date).
struct ReviveArgumentProcessor;

impl ArgumentProcessor for ReviveArgumentProcessor {
    fn process_argument(&self, arg: &Dynamic, _extension: Option<&ExtensionDescription>) -> Dynamic {
        revive(arg)
    }
}

/// O processador que desfaz o que `executeCommand` faz com os argumentos (Range, Position, Location e
/// VSBuffer voltam a ser as classes da API).
struct ApiTypesArgumentProcessor;

impl ArgumentProcessor for ApiTypesArgumentProcessor {
    fn process_argument(&self, arg: &Dynamic, _extension: Option<&ExtensionDescription>) -> Dynamic {
        clone_and_change(arg, &|obj| {
            // Reverse of https://github.com/microsoft/vscode/blob/1f28c5fc681f4c01226460b6d1c7e91b8acb4a5b/src/vs/workbench/api/node/extHostCommands.ts#L112-L127
            if let Some(range) = type_converters::Range::from_dto(obj) {
                return throw_on_error(type_converters::Range::to(&range)).to_dynamic();
            }
            if let Some(position) = type_converters::Position::from_dto(obj) {
                return throw_on_error(type_converters::Position::to(&position)).to_dynamic();
            }
            if let Some(location) = type_converters::Location::from_dto(obj) {
                return throw_on_error(type_converters::Location::to(&location)).to_dynamic();
            }
            if class_of(obj) == Some("VSBuffer") {
                // Create a copy of the buffer since the original buffer is owned by the extension host and might be reused for other commands
                return Dynamic::typed_array(throw_on_error(vs_buffer_from_dynamic(obj)).to_vec());
            }
            if !obj.is_array() {
                return obj.clone();
            }
            Dynamic::Undefined
        })
    }
}

/// `throw` num ponto do original que lança: o pânico com o [`Error`] é capturado pela função assíncrona que o
/// chama.
fn throw_on_error<T>(result: Result<T, Error>) -> T {
    result.unwrap_or_else(|error| errors::throw(error))
}

/// `${value}` e o `JSON.stringify` de um objeto, como a mensagem de `registerApiCommand` os monta.
fn describe_argument(value: &Dynamic) -> String {
    if value.type_of() == "object" {
        return dynamic_to_json(value).map(|json| json_stringify_tabs(&json)).unwrap_or_else(|| "undefined".to_owned());
    }
    match value {
        Dynamic::Undefined => "undefined".to_owned(),
        Dynamic::Bool(flag) => flag.to_string(),
        Dynamic::Number(number) => number_to_js_string(*number),
        Dynamic::BigInt(number) => number.to_string(),
        Dynamic::String(text) => text.to_string_lossy(),
        Dynamic::Function(function) => format!("function {}() {{ [native code] }}", function.name()),
        _ => unreachable!("os demais valores são objetos"),
    }
}

/// `ExtHostCommands`.
pub struct ExtHostCommands {
    this: Weak<ExtHostCommands>,
    proxy: Rc<dyn MainThreadCommandsShape>,
    telemetry: Rc<dyn MainThreadTelemetryShape>,
    commands: RefCell<Vec<(String, Rc<CommandRegistration>)>>,
    api_commands: RefCell<HashMap<String, Rc<ApiCommand>>>,
    log_service: LogServiceRef,
    ext_host_telemetry: Rc<dyn IExtHostTelemetry>,
    argument_processors: RefCell<Vec<Rc<dyn ArgumentProcessor>>>,
    converter: Rc<CommandsConverter>,
}

impl ExtHostCommands {
    pub fn new(ext_host_rpc: &dyn IMainContext, log_service: LogServiceRef, ext_host_telemetry: Rc<dyn IExtHostTelemetry>) -> Rc<Self> {
        let argument_processors: Vec<Rc<dyn ArgumentProcessor>> = vec![Rc::new(ReviveArgumentProcessor), Rc::new(ApiTypesArgumentProcessor)];
        let commands = Rc::new_cyclic(|this: &Weak<ExtHostCommands>| ExtHostCommands {
            this: this.clone(),
            proxy: ext_host_rpc.get_proxy(&MainContext::MAIN_THREAD_COMMANDS),
            telemetry: ext_host_rpc.get_proxy(&MainContext::MAIN_THREAD_TELEMETRY),
            commands: RefCell::new(Vec::new()),
            api_commands: RefCell::new(HashMap::new()),
            ext_host_telemetry,
            argument_processors: RefCell::new(argument_processors),
            converter: CommandsConverter::new(this.clone(), log_service.clone()),
            log_service,
        });
        // o conversor registra o comando delegante no construtor do original
        commands.converter.register_delegating_command(&commands);
        commands
    }

    /// `converter`.
    pub fn converter(&self) -> Rc<CommandsConverter> {
        self.converter.clone()
    }

    /// O `Rc` deste host, para as operações assíncronas que precisam dele depois do retorno.
    fn rc(&self) -> Rc<ExtHostCommands> {
        self.this.upgrade().expect("o ExtHostCommands está vivo enquanto um método dele roda")
    }

    pub fn register_argument_processor(&self, processor: Rc<dyn ArgumentProcessor>) {
        self.argument_processors.borrow_mut().push(processor);
    }

    pub fn register_api_command(&self, api_command: ApiCommand) -> Result<types::Disposable, Error> {
        let api_command = Rc::new(api_command);
        let this = self.this.clone();
        let command = api_command.clone();
        let callback: CommandCallback = Rc::new(move |api_args: &[Dynamic]| {
            let Some(this) = this.upgrade() else {
                return MaybePromise::Promise(Promise::rejected(Error::new("ExtHostCommands is disposed")));
            };
            let command = command.clone();
            let api_args = api_args.to_vec();
            MaybePromise::Promise(async_fn(async move {
                let mut internal_args = Vec::with_capacity(command.args.len());
                for (index, arg) in command.args.iter().enumerate() {
                    let value = api_args.get(index).cloned().unwrap_or(Dynamic::Undefined);
                    if !(arg.validate)(&value) {
                        return Err(Error::new(format!(
                            "Invalid argument '{}' when running '{}', received: {} ",
                            arg.name,
                            command.id,
                            describe_argument(&value)
                        )));
                    }
                    internal_args.push((arg.convert)(&value)?);
                }

                let internal_result = this.execute_command(&command.internal_id, internal_args).await?;
                (command.result.convert)(&internal_result, &api_args, &this.converter)
            }))
        });

        let mut metadata = CommandMetadata::new(api_command.description.as_str());
        metadata.args = Some(
            api_command
                .args
                .iter()
                .map(|arg| {
                    let mut argument = CommandArgument::new(arg.name.clone());
                    argument.description = Some(arg.description.clone());
                    argument
                })
                .collect(),
        );
        metadata.returns = Some(api_command.result.description.clone());
        let registration = self.register_command(false, &api_command.id, callback, Some(metadata), None)?;

        self.api_commands.borrow_mut().insert(api_command.id.clone(), api_command.clone());

        let this = self.this.clone();
        Ok(types::Disposable::new(move || {
            registration.dispose();
            if let Some(this) = this.upgrade() {
                this.api_commands.borrow_mut().remove(&api_command.id);
            }
        }))
    }

    pub fn register_command(
        &self,
        global: bool,
        id: &str,
        callback: CommandCallback,
        metadata: Option<CommandMetadata>,
        extension: Option<ExtensionDescription>,
    ) -> Result<types::Disposable, Error> {
        self.log_service.trace_args("ExtHostCommands#registerCommand", &[LogArg::from(id)]);

        if id.trim().is_empty() {
            return Err(Error::new("invalid id"));
        }

        if self.has_command(id) {
            return Err(Error::new(format!("command '{id}' already exists")));
        }

        self.commands.borrow_mut().push((id.to_owned(), Rc::new(CommandRegistration { callback, metadata, extension })));
        if global {
            self.proxy.rpc_register_command(id);
        }

        let this = self.this.clone();
        let id = id.to_owned();
        Ok(types::Disposable::new(move || {
            let Some(this) = this.upgrade() else {
                return;
            };
            if this.remove_command(&id) && global {
                this.proxy.rpc_unregister_command(&id);
            }
        }))
    }

    fn has_command(&self, id: &str) -> bool {
        self.commands.borrow().iter().any(|(candidate, _)| candidate == id)
    }

    fn find_command(&self, id: &str) -> Option<Rc<CommandRegistration>> {
        self.commands.borrow().iter().find(|(candidate, _)| candidate == id).map(|(_, registration)| registration.clone())
    }

    /// `this._commands.delete(id)`.
    fn remove_command(&self, id: &str) -> bool {
        let mut commands = self.commands.borrow_mut();
        let before = commands.len();
        commands.retain(|(candidate, _)| candidate != id);
        commands.len() != before
    }

    pub fn execute_command(&self, id: &str, args: Vec<Dynamic>) -> Promise<Dynamic> {
        self.log_service.trace_args("ExtHostCommands#executeCommand", &[LogArg::from(id)]);
        self.do_execute_command(id, args, true)
    }

    fn do_execute_command(&self, id: &str, args: Vec<Dynamic>, retry: bool) -> Promise<Dynamic> {
        let this = self.rc();
        let id = id.to_owned();
        async_fn_maybe(async move {
            if this.has_command(&id) {
                // - We stay inside the extension host and support
                //   to pass any kind of parameters around.
                // - We still emit the corresponding activation event
                //   BUT we don't await that event
                this.proxy.rpc_fire_command_activation_event(&id);
                return Ok(MaybePromise::Promise(this.execute_contributed_command(&id, args, false)));
            }

            // automagically convert some argument types
            let has_buffers = Cell::new(false);
            let to_args = clone_and_change(&Dynamic::array(args.iter().cloned()), &|value| {
                if is_instance_of(value, "Position") {
                    return type_converters::Position::to_dto(&type_converters::Position::from(&throw_on_error(types::Position::from_dynamic(value))));
                }
                if is_instance_of(value, "Range") {
                    return type_converters::Range::to_dto(&type_converters::Range::from(&throw_on_error(types::Range::from_dynamic(value))));
                }
                if is_instance_of(value, "Location") {
                    let location = type_converters::Location::from(&throw_on_error(types::Location::from_dynamic(value)));
                    return type_converters::Location::to_dto(&location);
                }
                if let Some(range) = notebook_range_like(value) {
                    return type_converters::NotebookRange::to_dto(&range);
                }
                if class_of(value) == Some("ArrayBuffer") || matches!(value, Dynamic::TypedArray(_)) || class_of(value) == Some("VSBuffer") {
                    has_buffers.set(true);
                    return match value {
                        Dynamic::TypedArray(bytes) => vs_buffer_to_dynamic(&VSBuffer::wrap(bytes.to_vec())),
                        _ if class_of(value) == Some("ArrayBuffer") => {
                            vs_buffer_to_dynamic(&VSBuffer::wrap(bytes_of(value).expect("um ArrayBuffer marcado tem bytes")))
                        }
                        _ => value.clone(),
                    };
                }
                if !value.is_array() {
                    return value.clone();
                }
                Dynamic::Undefined
            });
            let Dynamic::Array(to_args) = to_args else {
                unreachable!("o clone de um array é um array");
            };
            let to_args = to_args.to_vec();
            let command_args = if has_buffers.get() { CommandArgs::WithBuffers(SerializableObjectWithBuffers::new(to_args)) } else { CommandArgs::Plain(to_args) };

            match this.proxy.rpc_execute_command(&id, command_args, retry).await {
                Ok(result) => Ok(MaybePromise::Value(revive(&result))),
                Err(error) => {
                    // Rerun the command when it wasn't known, had arguments, and when retry
                    // is enabled. We do this because the command might be registered inside
                    // the extension host now and can therefore accept the arguments as-is.
                    if error.message == "$executeCommand:retry" {
                        Ok(MaybePromise::Promise(this.do_execute_command(&id, args, false)))
                    } else {
                        Err(error)
                    }
                }
            }
        })
    }

    fn execute_contributed_command(&self, id: &str, args: Vec<Dynamic>, annotate_error: bool) -> Promise<Dynamic> {
        let this = self.rc();
        let mut id = id.to_owned();
        async_fn(async move {
            let Some(command) = this.find_command(&id) else {
                return Err(Error::new("Unknown command"));
            };
            if let Some(metadata_args) = command.metadata.as_ref().and_then(|metadata| metadata.args.as_ref()) {
                for (index, metadata_arg) in metadata_args.iter().enumerate() {
                    let arg = args.get(index).cloned().unwrap_or(Dynamic::Undefined);
                    if errors::catch(|| validate_constraint(&arg, metadata_arg.constraint.as_ref())).is_err() {
                        return Err(Error::new(format!(
                            "Running the contributed command: '{id}' failed. Illegal argument '{}' - {}",
                            metadata_arg.name,
                            metadata_arg.description.as_deref().unwrap_or("undefined")
                        )));
                    }
                }
            }

            let stop_watch = StopWatch::create(None);
            let outcome = match errors::catch(|| (command.callback)(&args)) {
                Ok(returned) => await_maybe(returned).await,
                Err(error) => Err(error),
            };
            let result = match outcome {
                Ok(value) => Ok(value),
                Err(error) => {
                    // The indirection-command from the converter can fail when invoking the actual
                    // command and in that case it is better to blame the correct command
                    if id == this.converter.delegating_command_id {
                        if let Some(actual) = this.converter.get_actual_command(&args) {
                            id = actual.command;
                        }
                    }
                    if !is_cancellation_error(&error) {
                        let identifier = command.extension.as_ref().map_or(Dynamic::Undefined, |extension| Dynamic::string(&extension.identifier.value));
                        this.log_service.error_args(&LogMessage::Error(error.clone()), &[LogArg::from(id.as_str()), LogArg::Value(identifier)]);
                    }

                    if annotate_error {
                        Err(this.annotate_error(&command, &id, &error))
                    } else {
                        Err(error)
                    }
                }
            };
            this.report_telemetry(&command, &id, stop_watch.elapsed());
            result
        })
    }

    /// O `CommandError` do original: a mensagem do erro com o `id` do comando e a `source` (o nome da extensão).
    fn annotate_error(&self, command: &CommandRegistration, id: &str, error: &Error) -> Error {
        if let Some(extension) = &command.extension {
            let reported = self.ext_host_telemetry.on_extension_error(&extension.identifier, error);
            self.log_service.trace_args(
                "forwarded error to extension?",
                &[LogArg::Value(Dynamic::Bool(reported)), LogArg::Value(Dynamic::string(&extension.identifier.value))],
            );
        }
        let mut annotated = Error::new(to_error_message_of(error, false)).with_prop("id", id);
        if let Some(extension) = &command.extension {
            annotated = annotated.with_prop("source", extension.manifest.display_name.clone().unwrap_or_else(|| extension.manifest.name.clone()));
        }
        annotated
    }

    fn report_telemetry(&self, command: &CommandRegistration, id: &str, duration: f64) {
        let Some(extension) = &command.extension else {
            return;
        };
        if id.starts_with("code.copilot.logStructured") {
            // This command is very active. See https://github.com/microsoft/vscode/issues/254153.
            return;
        }
        self.telemetry.rpc_public_log2(
            "Extension:ActionExecuted",
            Some(Dynamic::object([
                ("extensionId", Dynamic::string(&extension.identifier.value)),
                ("id", TelemetryTrustedValue { value: Dynamic::string(id) }.to_dynamic()),
                ("duration", Dynamic::Number(duration)),
            ])),
        );
    }

    /// `getCommands(filterUnderscoreCommands = false)`.
    pub fn get_commands(&self, filter_underscore_commands: bool) -> Promise<Vec<String>> {
        self.log_service.trace_args("ExtHostCommands#getCommands", &[LogArg::Value(Dynamic::Bool(filter_underscore_commands))]);

        self.proxy.rpc_get_commands().then(move |result| {
            if filter_underscore_commands { result.into_iter().filter(|command| !command.starts_with('_')).collect() } else { result }
        })
    }

    /// `$executeContributedCommand` e `$getContributedCommandMetadata` são o [`ExtHostCommandsShape`].
    fn contributed_metadata(&self) -> Vec<(String, ICommandMetadataDto)> {
        self.commands
            .borrow()
            .iter()
            .filter_map(|(id, command)| {
                let metadata = command.metadata.as_ref()?;
                let args = metadata.args.as_ref().map(|args| {
                    args.iter()
                        .map(|arg| CommandMetadataArgDto { name: arg.name.clone(), is_optional: Some(arg.is_optional), description: arg.description.clone() })
                        .collect()
                });
                Some((id.clone(), ICommandMetadataDto { description: metadata.description.clone(), args, returns: metadata.returns.clone() }))
            })
            .collect()
    }
}

impl ExtHostCommandsShape for ExtHostCommands {
    fn rpc_execute_contributed_command(&self, id: &str, args: Vec<Dynamic>) -> Promise<Dynamic> {
        self.log_service.trace_args("ExtHostCommands#$executeContributedCommand", &[LogArg::from(id)]);

        let Some(handler) = self.find_command(id) else {
            return Promise::rejected(Error::new(format!("Contributed command '{id}' does not exist.")));
        };
        let processors = self.argument_processors.borrow().clone();
        let args = args
            .iter()
            .map(|arg| processors.iter().fold(arg.clone(), |processed, processor| processor.process_argument(&processed, handler.extension.as_ref())))
            .collect();
        self.execute_contributed_command(id, args, true)
    }

    fn rpc_get_contributed_command_metadata(&self) -> Promise<Vec<(String, ICommandMetadataDto)>> {
        Promise::resolved(self.contributed_metadata())
    }
}

/// `typeof thing.start === 'number' && typeof thing.end === 'number'`, ou uma `NotebookRange`: o
/// `NotebookRange.isNotebookRange` do original, que reconhece qualquer objeto com os dois números.
fn notebook_range_like(value: &Dynamic) -> Option<type_converters::CellRange> {
    if !matches!(value, Dynamic::Object(_)) {
        return None;
    }
    match (value.get_str("start"), value.get_str("end")) {
        (Dynamic::Number(start), Dynamic::Number(end)) => Some(type_converters::CellRange { start, end }),
        _ => None,
    }
}

/// `CommandsConverter`.
pub struct CommandsConverter {
    /// `delegatingCommandId`.
    pub delegating_command_id: String,
    cache: Rc<RefCell<HashMap<String, Command>>>,
    cache_id_pool: Cell<u64>,
    commands: Weak<ExtHostCommands>,
    log_service: LogServiceRef,
}

impl CommandsConverter {
    fn new(commands: Weak<ExtHostCommands>, log_service: LogServiceRef) -> Rc<CommandsConverter> {
        Rc::new(CommandsConverter {
            delegating_command_id: format!("__vsc{}", generate_uuid()),
            cache: Rc::new(RefCell::new(HashMap::new())),
            cache_id_pool: Cell::new(0),
            commands,
            log_service,
        })
    }

    /// O `this._commands.registerCommand(true, this.delegatingCommandId, this._executeConvertedCommand, this)` do
    /// construtor do original.
    fn register_delegating_command(self: &Rc<Self>, commands: &ExtHostCommands) {
        let converter = Rc::downgrade(self);
        let callback: CommandCallback = Rc::new(move |args: &[Dynamic]| match converter.upgrade() {
            Some(converter) => MaybePromise::Promise(converter.execute_converted_command(args)),
            None => MaybePromise::Promise(Promise::rejected(Error::new("CommandsConverter is disposed"))),
        });
        // o registro do comando delegante não pode falhar: o id é novo e não é vazio
        let _ = commands.register_command(true, &self.delegating_command_id, callback, None, None).expect("o comando delegante é registrado uma vez");
    }

    /// `API commands that have no return type (void) can be converted to their internal command and don't need any
    /// indirection commands`.
    fn lookup_api_command(&self, id: &str) -> Option<Rc<ApiCommand>> {
        let commands = self.commands.upgrade()?;
        let candidate = commands.api_commands.borrow().get(id).cloned();
        candidate.filter(|candidate| candidate.result.is_void())
    }

    /// `getActualCommand(...args)`.
    pub fn get_actual_command(&self, args: &[Dynamic]) -> Option<Command> {
        match args.first() {
            Some(Dynamic::String(id)) => self.cache.borrow().get(&id.to_string_lossy()).cloned(),
            _ => None,
        }
    }

    fn execute_converted_command(&self, args: &[Dynamic]) -> Promise<Dynamic> {
        let actual_command = self.get_actual_command(args);
        let first = args.first().map_or_else(|| "undefined".to_owned(), describe_argument);
        self.log_service.trace_args(
            "CommandsConverter#EXECUTE",
            &[LogArg::from(first.as_str()), LogArg::from(actual_command.as_ref().map_or("MISSING", |command| command.command.as_str()))],
        );

        let Some(actual_command) = actual_command else {
            return Promise::rejected(Error::new(format!("Actual command not found, wanted to execute {first}")));
        };
        match self.commands.upgrade() {
            Some(commands) => commands.execute_command(&actual_command.command, actual_command.arguments.unwrap_or_default()),
            None => Promise::rejected(Error::new("ExtHostCommands is disposed")),
        }
    }
}

impl ICommandsConverter for CommandsConverter {
    fn to_internal(&self, command: Option<&Command>, disposables: &DisposableStore) -> Result<Option<ICommandDto>, Error> {
        let Some(command) = command else {
            return Ok(None);
        };

        let mut result =
            ICommandDto { ident: None, id: command.command.clone(), title: command.title.clone(), tooltip: command.tooltip.clone(), arguments: None };

        if command.command.is_empty() {
            // falsy command id -> return converted command but don't attempt any
            // argument or API-command dance since this command won't run anyways
            return Ok(Some(result));
        }

        if let Some(api_command) = self.lookup_api_command(&command.command) {
            // API command with return-value can be converted inplace
            result.id = api_command.internal_id.clone();
            result.arguments = Some(
                api_command
                    .args
                    .iter()
                    .enumerate()
                    .map(|(index, arg)| {
                        let value = command.arguments.as_ref().and_then(|arguments| arguments.get(index)).cloned().unwrap_or(Dynamic::Undefined);
                        (arg.convert)(&value)
                    })
                    .collect::<Result<Vec<_>, _>>()?,
            );
        } else if command.arguments.as_ref().is_some_and(|arguments| !arguments.is_empty()) {
            // we have a contributed command with arguments. that
            // means we don't want to send the arguments around

            self.cache_id_pool.set(self.cache_id_pool.get() + 1);
            let id = format!("{} /{}", command.command, self.cache_id_pool.get());
            self.cache.borrow_mut().insert(id.clone(), command.clone());
            let cache = self.cache.clone();
            let log_service = self.log_service.clone();
            let cached_id = id.clone();
            disposables.add(to_disposable(move || {
                cache.borrow_mut().remove(&cached_id);
                log_service.trace_args("CommandsConverter#DISPOSE", &[LogArg::from(cached_id.as_str())]);
            }));
            result.ident = Some(id.clone());

            result.id = self.delegating_command_id.clone();
            result.arguments = Some(vec![Dynamic::string(&id)]);

            self.log_service.trace_args("CommandsConverter#CREATE", &[LogArg::from(command.command.as_str()), LogArg::from(id.as_str())]);
        }

        Ok(Some(result))
    }

    fn from_internal(&self, command: &ICommandDto) -> Option<Command> {
        match &command.ident {
            Some(ident) => self.cache.borrow().get(ident).cloned(),
            None => Some(Command { title: command.title.clone(), command: command.id.clone(), tooltip: None, arguments: command.arguments.clone() }),
        }
    }
}

type ArgumentValidate = Rc<dyn Fn(&Dynamic) -> bool>;
type ArgumentConvert = Rc<dyn Fn(&Dynamic) -> Result<Dynamic, Error>>;

/// `typeof thing.line === 'number' && typeof thing.character === 'number'`: o `Position.isPosition`, que aceita a
/// classe ou um objeto com os dois números.
fn position_like(value: &Dynamic) -> Option<EditorPosition> {
    if !matches!(value, Dynamic::Object(_)) {
        return None;
    }
    match (value.get_str("line"), value.get_str("character")) {
        (Dynamic::Number(line), Dynamic::Number(character)) => Some(EditorPosition::new(line as i32 + 1, character as i32 + 1)),
        _ => None,
    }
}

/// O `Range.isRange`: a classe ou um objeto com `start` e `end` que são posições.
fn range_like(value: &Dynamic) -> Option<EditorRange> {
    if !matches!(value, Dynamic::Object(_)) {
        return None;
    }
    let (start, end) = (position_like(&value.get_str("start"))?, position_like(&value.get_str("end"))?);
    Some(EditorRange { start_line_number: start.line_number, start_column: start.column, end_line_number: end.line_number, end_column: end.column })
}

/// O `Selection.isSelection`: a classe ou um intervalo com `anchor`, `active` e `isReversed` booleano.
fn selection_like(value: &Dynamic) -> Option<EditorSelection> {
    if !matches!(value, Dynamic::Object(_)) {
        return None;
    }
    let (anchor, active) = (position_like(&value.get_str("anchor"))?, position_like(&value.get_str("active"))?);
    let tagged = is_instance_of(value, "Selection");
    if !tagged && (range_like(value).is_none() || !matches!(value.get_str("isReversed"), Dynamic::Bool(_))) {
        return None;
    }
    Some(EditorSelection::new(anchor.line_number, anchor.column, active.line_number, active.column))
}

/// `ApiCommandArgument<V, O>`.
#[derive(Clone)]
pub struct ApiCommandArgument {
    pub name: String,
    pub description: String,
    pub validate: ArgumentValidate,
    pub convert: ArgumentConvert,
}

impl ApiCommandArgument {
    pub fn new(
        name: &str,
        description: &str,
        validate: impl Fn(&Dynamic) -> bool + 'static,
        convert: impl Fn(&Dynamic) -> Result<Dynamic, Error> + 'static,
    ) -> ApiCommandArgument {
        ApiCommandArgument { name: name.to_owned(), description: description.to_owned(), validate: Rc::new(validate), convert: Rc::new(convert) }
    }

    /// `ApiCommandArgument.Uri`.
    pub fn uri() -> ApiCommandArgument {
        ApiCommandArgument::new("uri", "Uri of a text document", |value| is_instance_of(value, "Uri"), |value| Ok(value.clone()))
    }

    /// `ApiCommandArgument.Position`: converte para o `IPosition`.
    pub fn position() -> ApiCommandArgument {
        ApiCommandArgument::new(
            "position",
            "A position in a text document",
            |value| position_like(value).is_some(),
            |value| Ok(type_converters::Position::to_dto(&position_like(value).expect("validado antes de converter"))),
        )
    }

    /// `ApiCommandArgument.Range`: converte para o `IRange`.
    pub fn range() -> ApiCommandArgument {
        ApiCommandArgument::new(
            "range",
            "A range in a text document",
            |value| range_like(value).is_some(),
            |value| Ok(type_converters::Range::to_dto(&range_like(value).expect("validado antes de converter"))),
        )
    }

    /// `ApiCommandArgument.Selection`: converte para o `ISelection`.
    pub fn selection() -> ApiCommandArgument {
        ApiCommandArgument::new(
            "selection",
            "A selection in a text document",
            |value| selection_like(value).is_some(),
            |value| Ok(type_converters::Selection::to_dto(&selection_like(value).expect("validado antes de converter"))),
        )
    }

    /// `ApiCommandArgument.Number`.
    pub fn number() -> ApiCommandArgument {
        ApiCommandArgument::new("number", "", |value| matches!(value, Dynamic::Number(_)), |value| Ok(value.clone()))
    }

    /// `ApiCommandArgument.String`.
    pub fn string() -> ApiCommandArgument {
        ApiCommandArgument::new("string", "", |value| matches!(value, Dynamic::String(_)), |value| Ok(value.clone()))
    }

    /// `ApiCommandArgument.Arr(element)`.
    pub fn arr(element: &ApiCommandArgument) -> ApiCommandArgument {
        let (validate_element, convert_element) = (element.validate.clone(), element.convert.clone());
        ApiCommandArgument::new(
            &format!("{}_array", element.name),
            &format!("Array of {}, {}", element.name, element.description),
            move |value| matches!(value, Dynamic::Array(items) if items.to_vec().iter().all(|item| validate_element(item))),
            move |value| match value {
                Dynamic::Array(items) => Ok(Dynamic::array(items.to_vec().iter().map(|item| convert_element(item)).collect::<Result<Vec<_>, _>>()?)),
                _ => Err(Error::type_error("v.map is not a function")),
            },
        )
    }

    /// `ApiCommandArgument.CallHierarchyItem`.
    pub fn call_hierarchy_item() -> ApiCommandArgument {
        ApiCommandArgument::new(
            "item",
            "A call hierarchy item",
            |value| is_instance_of(value, "CallHierarchyItem"),
            |value| {
                let item = types::CallHierarchyItem::from_dynamic(value)?;
                Ok(type_converters::CallHierarchyItem::from(&item, None, None)?.to_dynamic())
            },
        )
    }

    /// `ApiCommandArgument.TypeHierarchyItem`.
    pub fn type_hierarchy_item() -> ApiCommandArgument {
        ApiCommandArgument::new(
            "item",
            "A type hierarchy item",
            |value| is_instance_of(value, "TypeHierarchyItem"),
            |value| {
                let item = types::TypeHierarchyItem::from_dynamic(value)?;
                Ok(type_converters::TypeHierarchyItem::from(&item, None, None)?.to_dynamic())
            },
        )
    }

    /// `optional()`: aceita também `undefined` e `null`, que passam sem conversão.
    pub fn optional(&self) -> ApiCommandArgument {
        let (validate, convert) = (self.validate.clone(), self.convert.clone());
        ApiCommandArgument::new(
            &self.name,
            &format!("(optional) {}", self.description),
            move |value| value.is_nullish() || validate(value),
            move |value| if value.is_nullish() { Ok(value.clone()) } else { convert(value) },
        )
    }

    /// `with(name, description)`.
    pub fn with(&self, name: Option<&str>, description: Option<&str>) -> ApiCommandArgument {
        ApiCommandArgument {
            name: name.unwrap_or(&self.name).to_owned(),
            description: description.unwrap_or(&self.description).to_owned(),
            validate: self.validate.clone(),
            convert: self.convert.clone(),
        }
    }
}

type ResultConvert = Rc<dyn Fn(&Dynamic, &[Dynamic], &CommandsConverter) -> Result<Dynamic, Error>>;

/// `ApiCommandResult<V, O>`.
#[derive(Clone)]
pub struct ApiCommandResult {
    pub description: String,
    pub convert: ResultConvert,
    is_void: bool,
}

impl ApiCommandResult {
    pub fn new(description: &str, convert: impl Fn(&Dynamic, &[Dynamic], &CommandsConverter) -> Result<Dynamic, Error> + 'static) -> ApiCommandResult {
        ApiCommandResult { description: description.to_owned(), convert: Rc::new(convert), is_void: false }
    }

    /// `ApiCommandResult.Void`.
    pub fn void() -> ApiCommandResult {
        let convert: ResultConvert = Rc::new(|value: &Dynamic, _: &[Dynamic], _: &CommandsConverter| Ok::<Dynamic, Error>(value.clone()));
        ApiCommandResult { description: "no result".to_owned(), convert, is_void: true }
    }

    /// `result === ApiCommandResult.Void`.
    pub fn is_void(&self) -> bool {
        self.is_void
    }
}

/// `ApiCommand`.
pub struct ApiCommand {
    pub id: String,
    pub internal_id: String,
    pub description: String,
    pub args: Vec<ApiCommandArgument>,
    pub result: ApiCommandResult,
}

impl ApiCommand {
    pub fn new(id: &str, internal_id: &str, description: &str, args: Vec<ApiCommandArgument>, result: ApiCommandResult) -> ApiCommand {
        ApiCommand { id: id.to_owned(), internal_id: internal_id.to_owned(), description: description.to_owned(), args, result }
    }
}
