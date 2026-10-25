//! Porte parcial de `src/vs/platform/log/common/logIpc.ts`: o `RemoteLoggerChannelClient`, que o `PtyHostService` usa para
//! manter o serviço de log do processo do pty host igual ao do processo que o controla. O resto do arquivo
//! (`LoggerChannelClient`, `LoggerChannel` e o `Logger` do canal) não tem consumidor ainda.
//!
//! Representação: `extends Disposable` é um [`Disposable`] por composição. Os valores do canal são [`Marshalled`]: um
//! `LogLevel` é o número do original (`Off` 0 a `Error` 5), um `ILoggerResource` é o objeto `{ resource, id, name?,
//! logLevel?, hidden?, when?, extensionId?, group? }` e o `URI.revive` é o `revive` do `Marshalled`.

use std::rc::Rc;

use je_base::cancellation::CancellationToken;
use je_base::lifecycle::{Disposable, IDisposable};
use je_base::marshalling::{Marshalled, revive};
use je_base::parts::ipc::IChannel;
use je_base::path::Platform as PathPlatform;
use je_base::uri::URI;

use super::{LogLevel, LoggerGroup, LoggerResource, LoggerService, LoggerServiceLogLevelChange, ResourceOrId};

fn log_level_to_marshalled(level: LogLevel) -> Marshalled {
    Marshalled::Number(f64::from(level as u8))
}

fn log_level_from_marshalled(value: &Marshalled) -> Option<LogLevel> {
    match value {
        Marshalled::Number(number) => Some(match *number as i64 {
            0 => LogLevel::Off,
            1 => LogLevel::Trace,
            2 => LogLevel::Debug,
            3 => LogLevel::Info,
            4 => LogLevel::Warning,
            5 => LogLevel::Error,
            _ => return None,
        }),
        _ => None,
    }
}

/// `URI.revive(value)`.
fn revive_uri(value: &Marshalled) -> Option<URI> {
    match revive(value.clone(), 0, PathPlatform::Posix) {
        Marshalled::Uri(uri) => Some(uri),
        _ => None,
    }
}

fn property<'a>(entries: &'a [(je_base::JsString, Marshalled)], name: &str) -> Option<&'a Marshalled> {
    entries.iter().find(|(key, _)| *key == name).map(|(_, value)| value).filter(|value| !matches!(value, Marshalled::Undefined | Marshalled::Null))
}

fn text_property(entries: &[(je_base::JsString, Marshalled)], name: &str) -> Option<String> {
    match property(entries, name) {
        Some(Marshalled::String(text)) => Some(text.to_string_lossy()),
        _ => None,
    }
}

/// `{ ...loggerResource, resource: URI.revive(loggerResource.resource) }`; um valor que não é um recurso de logger não vira nada.
fn logger_resource_from_marshalled(value: &Marshalled) -> Option<LoggerResource> {
    let Marshalled::Object(entries) = value else { return None };
    let mut resource = LoggerResource::new(revive_uri(property(entries, "resource")?)?, text_property(entries, "id")?);
    resource.name = text_property(entries, "name");
    resource.log_level = property(entries, "logLevel").and_then(log_level_from_marshalled);
    resource.hidden = match property(entries, "hidden") {
        Some(Marshalled::Bool(hidden)) => Some(*hidden),
        _ => None,
    };
    resource.when = text_property(entries, "when");
    resource.extension_id = text_property(entries, "extensionId");
    resource.group = match property(entries, "group") {
        Some(Marshalled::Object(group)) => Some(LoggerGroup { id: text_property(group, "id")?, name: text_property(group, "name")? }),
        _ => None,
    };
    Some(resource)
}

fn logger_resources_from_marshalled(value: Option<&Marshalled>) -> Vec<LoggerResource> {
    match value {
        Some(Marshalled::Array(items)) => items.iter().filter_map(logger_resource_from_marshalled).collect(),
        _ => Vec::new(),
    }
}

pub struct RemoteLoggerChannelClient {
    base: Rc<Disposable>,
}

impl RemoteLoggerChannelClient {
    pub fn new(logger_service: Rc<dyn LoggerService>, channel: Rc<dyn IChannel>) -> Rc<Self> {
        let base = Disposable::new();

        let _ = channel.call("setLogLevel", Marshalled::Array(vec![log_level_to_marshalled(logger_service.get_log_level(None))]), CancellationToken::None);
        base.register(logger_service.on_did_change_log_level().listen({
            let channel = channel.clone();
            move |arg| {
                let arg = match arg {
                    LoggerServiceLogLevelChange::Default(level) => log_level_to_marshalled(*level),
                    LoggerServiceLogLevelChange::Logger(resource, level) => Marshalled::Array(vec![Marshalled::Uri(resource.clone()), log_level_to_marshalled(*level)]),
                };
                let _ = channel.call("setLogLevel", Marshalled::Array(vec![arg]), CancellationToken::None);
            }
        }));

        channel.call("getRegisteredLoggers", Marshalled::Undefined, CancellationToken::None).then({
            let logger_service = logger_service.clone();
            move |loggers| {
                for logger_resource in logger_resources_from_marshalled(Some(&loggers)) {
                    logger_service.register_logger(logger_resource);
                }
            }
        });

        base.register(channel.listen("onDidChangeVisibility", Marshalled::Undefined).listen({
            let logger_service = logger_service.clone();
            move |event| {
                if let Marshalled::Array(items) = event
                    && let [resource, Marshalled::Bool(visibility)] = items.as_slice()
                    && let Some(resource) = revive_uri(resource)
                {
                    logger_service.set_visibility(&ResourceOrId::Resource(resource), *visibility);
                }
            }
        }));

        base.register(channel.listen("onDidChangeLoggers", Marshalled::Undefined).listen(move |event| {
            let Marshalled::Object(entries) = event else { return };
            for logger_resource in logger_resources_from_marshalled(property(entries, "added")) {
                logger_service.register_logger(logger_resource);
            }
            for logger_resource in logger_resources_from_marshalled(property(entries, "removed")) {
                logger_service.deregister_logger(&ResourceOrId::Resource(logger_resource.resource));
            }
        }));

        Rc::new(Self { base })
    }
}

impl IDisposable for RemoteLoggerChannelClient {
    fn dispose(&self) {
        self.base.dispose();
    }
}
