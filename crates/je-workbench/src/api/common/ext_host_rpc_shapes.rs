//! Os contratos tipados do protocolo `MainThreadXxx`/`ExtHostXxx` sobre o [`RpcProtocol`]: para cada serviço, o proxy (cada
//! `rpc_*` chama o `$método` homônimo pelo [`RpcProxy`]) e o ator (decodifica os argumentos de cada `$método` e chama a forma
//! tipada), mais a [`register_known_shapes`], que ensina o protocolo a montar uns e outros.
//!
//! No original o `RPCProtocol` cria o proxy de qualquer identificador com um `Proxy` do JavaScript e despacha as chamadas
//! recebidas por `this._locals[rpcId][methodName]`. O porte tem formas tipadas (`ExtHostCommandsShape`...), e o protocolo precisa
//! saber, por identificador, como montar o `Rc<dyn Shape>` de proxy e como despachar para um `Rc<dyn Shape>` local
//! ([`RpcProtocol::register_shape`]). Aqui estão as formas que o host em processo usa: `MainThreadExtensionService`,
//! `MainThreadCommands`, `ExtHostCommands` e `MainThreadTelemetry`.
//!
//! Divergências:
//! - Os `void` do original (`$registerCommand`, `$onDidActivateExtension`...) também recebem resposta no protocolo; o proxy a descarta
//!   (um erro de resposta que ninguém lê vai ao tratador de erros inesperados, como a `LazyPromise` do original).
//! - Os valores que viajam como objeto (`ExtensionDescription`, `SerializedError`, metadados de comando) seguem a forma no fio de
//!   `services/extensions/extension_host_protocol`.
//! - Um argumento mal formado recebido pelo ator rejeita a chamada com o `TypeError` que descreve o argumento.

use std::rc::Rc;

use je_base::dynamic::Dynamic;
use je_base::errors::{Error, SerializedError};
use je_base::lifecycle::IDisposable;
use je_base::performance::PerformanceMark;
use je_base::promise::{MaybePromise, Promise};
use je_base::uri::URI;
use je_extensions::manifest::ExtensionIdentifier;
use je_extensions::scanner::ExtensionDescription;
use je_platform::action::action::LocalizedString;
use je_platform::commands::commands::CommandDescription;

use super::ext_host_protocol::{
    CommandArgs, CommandMetadataArgDto, ExtHostCommandsShape, ExtHostContext, ICommandMetadataDto, MainContext, MainThreadCommandsShape,
    MainThreadExtensionServiceShape, MainThreadTelemetryShape, SerializableObjectWithBuffers,
};
use super::ext_host_values::{ApiValue, revive};
use crate::services::extensions::extension_activation::{ExtensionActivationReason, MissingExtensionDependency};
use crate::services::extensions::extension_host_protocol::{
    extension_activation_reason_from_dynamic, extension_activation_reason_to_dynamic, extension_description_from_dynamic, extension_description_to_dynamic,
    extension_identifier_from_dynamic, extension_identifier_to_dynamic, serialized_error_from_dynamic, serialized_error_to_dynamic,
};
use crate::services::extensions::rpc_protocol::{
    IRpcActor, RpcProtocol, RpcProxy, as_serializable_object_with_buffers, serializable_object_with_buffers,
};

// #region Auxiliares

fn argument(args: &[Dynamic], index: usize) -> Dynamic {
    args.get(index).cloned().unwrap_or(Dynamic::Undefined)
}

fn text_argument(args: &[Dynamic], index: usize, what: &str) -> Result<String, Error> {
    match argument(args, index) {
        Dynamic::String(text) => Ok(text.to_string_lossy()),
        _ => Err(Error::type_error(format!("{what}: o argumento {index} precisa ser texto"))),
    }
}

fn number_argument(args: &[Dynamic], index: usize, what: &str) -> Result<f64, Error> {
    match argument(args, index) {
        Dynamic::Number(number) => Ok(number),
        _ => Err(Error::type_error(format!("{what}: o argumento {index} precisa ser número"))),
    }
}

fn list_of(value: &Dynamic, what: &str) -> Result<Vec<Dynamic>, Error> {
    match value {
        Dynamic::Array(items) => Ok(items.to_vec()),
        _ => Err(Error::type_error(format!("{what}: esperava uma lista"))),
    }
}

fn optional_text(value: &Dynamic) -> Option<String> {
    match value {
        Dynamic::String(text) => Some(text.to_string_lossy()),
        _ => None,
    }
}

fn text_or_undefined(value: &Option<String>) -> Dynamic {
    value.as_ref().map_or(Dynamic::Undefined, |text| Dynamic::string(text))
}

/// Uma chamada sem retorno (`void`): dispara e descarta a resposta.
fn fire(proxy: &RpcProxy, method: &str, args: Vec<Dynamic>) {
    let _ = proxy.call(method, args).promise();
}

/// O resultado do ator: o valor ou a rejeição pelo erro.
fn settled(result: Result<MaybePromise<Dynamic>, Error>) -> Option<MaybePromise<Dynamic>> {
    Some(match result {
        Ok(value) => value,
        Err(error) => MaybePromise::Promise(Promise::rejected(error)),
    })
}

fn void_promise(promise: Promise<()>) -> MaybePromise<Dynamic> {
    MaybePromise::Promise(promise.then(|()| Dynamic::Undefined))
}

// #endregion

// #region MainThreadCommands

/// O proxy de `MainThreadCommandsShape`.
pub struct MainThreadCommandsProxy {
    proxy: RpcProxy,
}

impl IDisposable for MainThreadCommandsProxy {
    fn dispose(&self) {}
}

impl MainThreadCommandsShape for MainThreadCommandsProxy {
    fn rpc_register_command(&self, id: &str) {
        fire(&self.proxy, "$registerCommand", vec![Dynamic::string(id)]);
    }

    fn rpc_unregister_command(&self, id: &str) {
        fire(&self.proxy, "$unregisterCommand", vec![Dynamic::string(id)]);
    }

    fn rpc_fire_command_activation_event(&self, id: &str) {
        fire(&self.proxy, "$fireCommandActivationEvent", vec![Dynamic::string(id)]);
    }

    fn rpc_execute_command(&self, id: &str, args: CommandArgs, retry: bool) -> Promise<Dynamic> {
        let args = match args {
            CommandArgs::Plain(args) => Dynamic::array(args),
            CommandArgs::WithBuffers(args) => serializable_object_with_buffers(Dynamic::array(args.value)),
        };
        self.proxy.call("$executeCommand", vec![Dynamic::string(id), args, Dynamic::Bool(retry)]).promise()
    }

    fn rpc_get_commands(&self) -> Promise<Vec<String>> {
        self.proxy.call("$getCommands", Vec::new()).promise().then_result(|result| {
            list_of(&result?, "$getCommands")?
                .iter()
                .map(|id| optional_text(id).ok_or_else(|| Error::type_error("$getCommands: id que não é texto")))
                .collect()
        })
    }
}

/// O ator de `MainThreadCommandsShape`.
pub struct MainThreadCommandsActor {
    target: Rc<dyn MainThreadCommandsShape>,
}

impl IRpcActor for MainThreadCommandsActor {
    fn invoke(&self, method: &str, args: Vec<Dynamic>) -> Option<MaybePromise<Dynamic>> {
        let target = &self.target;
        let result = match method {
            "$registerCommand" => text_argument(&args, 0, method).map(|id| {
                target.rpc_register_command(&id);
                MaybePromise::Value(Dynamic::Undefined)
            }),
            "$unregisterCommand" => text_argument(&args, 0, method).map(|id| {
                target.rpc_unregister_command(&id);
                MaybePromise::Value(Dynamic::Undefined)
            }),
            "$fireCommandActivationEvent" => text_argument(&args, 0, method).map(|id| {
                target.rpc_fire_command_activation_event(&id);
                MaybePromise::Value(Dynamic::Undefined)
            }),
            "$executeCommand" => text_argument(&args, 0, method).and_then(|id| {
                let raw = argument(&args, 1);
                let command_args = match as_serializable_object_with_buffers(&raw) {
                    Some(wrapped) => CommandArgs::WithBuffers(SerializableObjectWithBuffers::new(list_of(&wrapped.value, method)?)),
                    None => CommandArgs::Plain(list_of(&raw, method)?),
                };
                let retry = matches!(argument(&args, 2), Dynamic::Bool(true));
                Ok(MaybePromise::Promise(target.rpc_execute_command(&id, command_args, retry)))
            }),
            "$getCommands" => Ok(MaybePromise::Promise(target.rpc_get_commands().then(|ids| Dynamic::array(ids.iter().map(|id| Dynamic::string(id)))))),
            _ => return None,
        };
        settled(result)
    }
}

// #endregion

// #region ExtHostCommands

fn command_description_to_dynamic(description: &CommandDescription) -> Dynamic {
    match description {
        CommandDescription::Plain(text) => Dynamic::string(text),
        CommandDescription::Localized(localized) => Dynamic::object([("value", Dynamic::string(&localized.value)), ("original", Dynamic::string(&localized.original))]),
    }
}

fn command_description_from_dynamic(value: &Dynamic) -> CommandDescription {
    match value {
        Dynamic::String(text) => CommandDescription::Plain(text.to_string_lossy()),
        other => CommandDescription::Localized(LocalizedString {
            value: optional_text(&other.get_str("value")).unwrap_or_default(),
            original: optional_text(&other.get_str("original")).unwrap_or_default(),
        }),
    }
}

fn command_metadata_to_dynamic(metadata: &ICommandMetadataDto) -> Dynamic {
    let args = metadata.args.as_ref().map_or(Dynamic::Undefined, |args| {
        Dynamic::array(args.iter().map(|arg| {
            Dynamic::object([
                ("name", Dynamic::string(&arg.name)),
                ("isOptional", arg.is_optional.map_or(Dynamic::Undefined, Dynamic::Bool)),
                ("description", text_or_undefined(&arg.description)),
            ])
        }))
    });
    Dynamic::object([("description", command_description_to_dynamic(&metadata.description)), ("args", args), ("returns", text_or_undefined(&metadata.returns))])
}

fn command_metadata_from_dynamic(value: &Dynamic) -> Result<ICommandMetadataDto, Error> {
    let args = match value.get_str("args") {
        Dynamic::Array(items) => Some(
            items
                .to_vec()
                .iter()
                .map(|arg| {
                    Ok(CommandMetadataArgDto {
                        name: optional_text(&arg.get_str("name")).ok_or_else(|| Error::type_error("metadado de comando: argumento sem nome"))?,
                        is_optional: match arg.get_str("isOptional") {
                            Dynamic::Bool(flag) => Some(flag),
                            _ => None,
                        },
                        description: optional_text(&arg.get_str("description")),
                    })
                })
                .collect::<Result<Vec<_>, Error>>()?,
        ),
        _ => None,
    };
    Ok(ICommandMetadataDto { description: command_description_from_dynamic(&value.get_str("description")), args, returns: optional_text(&value.get_str("returns")) })
}

/// O proxy de `ExtHostCommandsShape`.
pub struct ExtHostCommandsProxy {
    proxy: RpcProxy,
}

impl ExtHostCommandsShape for ExtHostCommandsProxy {
    fn rpc_execute_contributed_command(&self, id: &str, args: Vec<Dynamic>) -> Promise<Dynamic> {
        self.proxy.call("$executeContributedCommand", vec![Dynamic::string(id), Dynamic::array(args)]).promise()
    }

    fn rpc_get_contributed_command_metadata(&self) -> Promise<Vec<(String, ICommandMetadataDto)>> {
        self.proxy.call("$getContributedCommandMetadata", Vec::new()).promise().then_result(|result| match result? {
            Dynamic::Object(object) => object
                .entries()
                .into_iter()
                .map(|(id, metadata)| command_metadata_from_dynamic(&metadata).map(|metadata| (id.to_string_lossy(), metadata)))
                .collect(),
            _ => Err(Error::type_error("$getContributedCommandMetadata: esperava um objeto")),
        })
    }
}

/// O ator de `ExtHostCommandsShape`.
pub struct ExtHostCommandsActor {
    target: Rc<dyn ExtHostCommandsShape>,
}

impl IRpcActor for ExtHostCommandsActor {
    fn invoke(&self, method: &str, args: Vec<Dynamic>) -> Option<MaybePromise<Dynamic>> {
        let target = &self.target;
        let result = match method {
            "$executeContributedCommand" => text_argument(&args, 0, method).and_then(|id| {
                let command_args = list_of(&argument(&args, 1), method)?;
                Ok(MaybePromise::Promise(target.rpc_execute_contributed_command(&id, command_args)))
            }),
            "$getContributedCommandMetadata" => Ok(MaybePromise::Promise(target.rpc_get_contributed_command_metadata().then(|metadata| {
                Dynamic::object(metadata.iter().map(|(id, metadata)| (id.as_str(), command_metadata_to_dynamic(metadata))))
            }))),
            _ => return None,
        };
        settled(result)
    }
}

// #endregion

// #region MainThreadTelemetry

/// O proxy de `MainThreadTelemetryShape`.
pub struct MainThreadTelemetryProxy {
    proxy: RpcProxy,
}

impl IDisposable for MainThreadTelemetryProxy {
    fn dispose(&self) {}
}

impl MainThreadTelemetryShape for MainThreadTelemetryProxy {
    fn rpc_public_log(&self, event_name: &str, data: Option<Dynamic>) {
        fire(&self.proxy, "$publicLog", vec![Dynamic::string(event_name), data.unwrap_or(Dynamic::Undefined)]);
    }

    fn rpc_public_log2(&self, event_name: &str, data: Option<Dynamic>) {
        fire(&self.proxy, "$publicLog2", vec![Dynamic::string(event_name), data.unwrap_or(Dynamic::Undefined)]);
    }
}

/// O ator de `MainThreadTelemetryShape`.
pub struct MainThreadTelemetryActor {
    target: Rc<dyn MainThreadTelemetryShape>,
}

impl IRpcActor for MainThreadTelemetryActor {
    fn invoke(&self, method: &str, args: Vec<Dynamic>) -> Option<MaybePromise<Dynamic>> {
        let target = &self.target;
        let result = match method {
            "$publicLog" | "$publicLog2" => text_argument(&args, 0, method).map(|event_name| {
                let data = match argument(&args, 1) {
                    Dynamic::Undefined => None,
                    data => Some(data),
                };
                if method == "$publicLog" {
                    target.rpc_public_log(&event_name, data);
                } else {
                    target.rpc_public_log2(&event_name, data);
                }
                MaybePromise::Value(Dynamic::Undefined)
            }),
            _ => return None,
        };
        settled(result)
    }
}

// #endregion

// #region MainThreadExtensionService

/// O proxy de `MainThreadExtensionServiceShape`.
pub struct MainThreadExtensionServiceProxy {
    proxy: RpcProxy,
}

impl IDisposable for MainThreadExtensionServiceProxy {
    fn dispose(&self) {}
}

impl MainThreadExtensionServiceShape for MainThreadExtensionServiceProxy {
    fn rpc_get_extension(&self, extension_id: &str) -> Promise<Option<ExtensionDescription>> {
        self.proxy.call("$getExtension", vec![Dynamic::string(extension_id)]).promise().then_result(|result| match result? {
            description @ Dynamic::Object(_) => extension_description_from_dynamic(&description).map(Some),
            _ => Ok(None),
        })
    }

    fn rpc_activate_extension(&self, extension_id: &ExtensionIdentifier, reason: &ExtensionActivationReason) -> Promise<()> {
        self.proxy
            .call("$activateExtension", vec![extension_identifier_to_dynamic(extension_id), extension_activation_reason_to_dynamic(reason)])
            .promise()
            .then(|_| ())
    }

    fn rpc_on_will_activate_extension(&self, extension_id: &ExtensionIdentifier) -> Promise<()> {
        self.proxy.call("$onWillActivateExtension", vec![extension_identifier_to_dynamic(extension_id)]).promise().then(|_| ())
    }

    fn rpc_on_did_activate_extension(
        &self,
        extension_id: &ExtensionIdentifier,
        code_loading_time: f64,
        activate_call_time: f64,
        activate_resolved_time: f64,
        activation_reason: &ExtensionActivationReason,
    ) {
        fire(
            &self.proxy,
            "$onDidActivateExtension",
            vec![
                extension_identifier_to_dynamic(extension_id),
                Dynamic::Number(code_loading_time),
                Dynamic::Number(activate_call_time),
                Dynamic::Number(activate_resolved_time),
                extension_activation_reason_to_dynamic(activation_reason),
            ],
        );
    }

    fn rpc_on_extension_activation_error(
        &self,
        extension_id: &ExtensionIdentifier,
        error: SerializedError,
        missing_extension_dependency: Option<MissingExtensionDependency>,
    ) -> Promise<()> {
        let missing = missing_extension_dependency.map_or(Dynamic::Null, |missing| Dynamic::object([("dependency", Dynamic::string(&missing.dependency))]));
        self.proxy
            .call("$onExtensionActivationError", vec![extension_identifier_to_dynamic(extension_id), serialized_error_to_dynamic(&error), missing])
            .promise()
            .then(|_| ())
    }

    fn rpc_on_extension_runtime_error(&self, extension_id: &ExtensionIdentifier, error: SerializedError) {
        fire(&self.proxy, "$onExtensionRuntimeError", vec![extension_identifier_to_dynamic(extension_id), serialized_error_to_dynamic(&error)]);
    }

    fn rpc_set_performance_marks(&self, marks: Vec<PerformanceMark>) -> Promise<()> {
        let marks = Dynamic::array(marks.iter().map(|mark| Dynamic::object([("name", Dynamic::string(&mark.name)), ("startTime", Dynamic::Number(mark.start_time))])));
        self.proxy.call("$setPerformanceMarks", vec![marks]).promise().then(|_| ())
    }

    fn rpc_as_browser_uri(&self, uri: &URI) -> Promise<URI> {
        self.proxy
            .call("$asBrowserUri", vec![uri.to_dynamic()])
            .promise()
            .then_result(|result| URI::from_dynamic(&revive(&result?)))
    }
}

/// O ator de `MainThreadExtensionServiceShape`.
pub struct MainThreadExtensionServiceActor {
    target: Rc<dyn MainThreadExtensionServiceShape>,
}

impl IRpcActor for MainThreadExtensionServiceActor {
    fn invoke(&self, method: &str, args: Vec<Dynamic>) -> Option<MaybePromise<Dynamic>> {
        let target = &self.target;
        let result = match method {
            "$getExtension" => text_argument(&args, 0, method).map(|id| {
                MaybePromise::Promise(target.rpc_get_extension(&id).then_result(|found| match found? {
                    Some(description) => extension_description_to_dynamic(&description),
                    None => Ok(Dynamic::Undefined),
                }))
            }),
            "$activateExtension" => extension_identifier_from_dynamic(&argument(&args, 0)).and_then(|id| {
                let reason = extension_activation_reason_from_dynamic(&argument(&args, 1))?;
                Ok(void_promise(target.rpc_activate_extension(&id, &reason)))
            }),
            "$onWillActivateExtension" => extension_identifier_from_dynamic(&argument(&args, 0)).map(|id| void_promise(target.rpc_on_will_activate_extension(&id))),
            "$onDidActivateExtension" => extension_identifier_from_dynamic(&argument(&args, 0)).and_then(|id| {
                let reason = extension_activation_reason_from_dynamic(&argument(&args, 4))?;
                target.rpc_on_did_activate_extension(
                    &id,
                    number_argument(&args, 1, method)?,
                    number_argument(&args, 2, method)?,
                    number_argument(&args, 3, method)?,
                    &reason,
                );
                Ok(MaybePromise::Value(Dynamic::Undefined))
            }),
            "$onExtensionRuntimeError" => extension_identifier_from_dynamic(&argument(&args, 0)).map(|id| {
                target.rpc_on_extension_runtime_error(&id, serialized_error_from_dynamic(&argument(&args, 1)));
                MaybePromise::Value(Dynamic::Undefined)
            }),
            "$onExtensionActivationError" => extension_identifier_from_dynamic(&argument(&args, 0)).map(|id| {
                let missing = match argument(&args, 2).get_str_if_object("dependency") {
                    Some(Dynamic::String(name)) => Some(MissingExtensionDependency::new(name.to_string_lossy())),
                    _ => None,
                };
                void_promise(target.rpc_on_extension_activation_error(&id, serialized_error_from_dynamic(&argument(&args, 1)), missing))
            }),
            "$setPerformanceMarks" => list_of(&argument(&args, 0), method).map(|marks| {
                let marks = marks
                    .iter()
                    .map(|mark| PerformanceMark {
                        name: optional_text(&mark.get_str("name")).unwrap_or_default(),
                        start_time: match mark.get_str("startTime") {
                            Dynamic::Number(time) => time,
                            _ => 0.0,
                        },
                    })
                    .collect();
                void_promise(target.rpc_set_performance_marks(marks))
            }),
            "$asBrowserUri" => URI::from_dynamic(&revive(&argument(&args, 0)))
                .map(|uri| MaybePromise::Promise(target.rpc_as_browser_uri(&uri).then(|uri| uri.to_dynamic()))),
            _ => return None,
        };
        settled(result)
    }
}

/// `value[key]` quando `value` é um objeto (senão `None`), sem o `TypeError` de ler propriedade de `null`.
trait GetIfObject {
    fn get_str_if_object(&self, key: &str) -> Option<Dynamic>;
}

impl GetIfObject for Dynamic {
    fn get_str_if_object(&self, key: &str) -> Option<Dynamic> {
        matches!(self, Dynamic::Object(_)).then(|| self.get_str(key))
    }
}

// #endregion

/// Ensina o protocolo a montar o proxy e a despachar para o objeto local de cada serviço do host de extensões que tem forma
/// tipada: `MainThreadExtensionService`, `MainThreadCommands`, `ExtHostCommands` e `MainThreadTelemetry`.
pub fn register_known_shapes(rpc: &RpcProtocol) {
    rpc.register_shape(
        &MainContext::MAIN_THREAD_EXTENSION_SERVICE,
        |proxy| Rc::new(MainThreadExtensionServiceProxy { proxy }) as Rc<dyn MainThreadExtensionServiceShape>,
        |target| Rc::new(MainThreadExtensionServiceActor { target }),
    );
    rpc.register_shape(
        &MainContext::MAIN_THREAD_COMMANDS,
        |proxy| Rc::new(MainThreadCommandsProxy { proxy }) as Rc<dyn MainThreadCommandsShape>,
        |target| Rc::new(MainThreadCommandsActor { target }),
    );
    rpc.register_shape(
        &ExtHostContext::EXT_HOST_COMMANDS,
        |proxy| Rc::new(ExtHostCommandsProxy { proxy }) as Rc<dyn ExtHostCommandsShape>,
        |target| Rc::new(ExtHostCommandsActor { target }),
    );
    rpc.register_shape(
        &MainContext::MAIN_THREAD_TELEMETRY,
        |proxy| Rc::new(MainThreadTelemetryProxy { proxy }) as Rc<dyn MainThreadTelemetryShape>,
        |target| Rc::new(MainThreadTelemetryActor { target }),
    );
}
