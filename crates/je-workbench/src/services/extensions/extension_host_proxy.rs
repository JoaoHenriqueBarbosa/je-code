//! Porte de `src/vs/workbench/services/extensions/common/extensionHostProxy.ts`: o resultado de `resolveAuthority`
//! (`IResolveAuthorityErrorResult`, `IResolveAuthorityOKResult`, `IResolveAuthorityResult`) e a interface `IExtensionHostProxy` (o
//! lado do workbench do protocolo com o host de extensões), mais o `ExtensionHostProxy` de `mainThreadExtensionService.ts`, a
//! implementação dela sobre o RPC (`ExtHostExtensionServiceShape`).
//!
//! Divergências:
//! - `message: string | undefined` é `Option<String>` e `detail: unknown` é `Option<Dynamic>`.
//! - `IRemoteConnectionData` (de `remoteAuthorityResolver.ts`, ainda não portado no `je-platform`) é a struct local
//!   [`IRemoteConnectionData`], com o `RemoteConnection` do `je-platform`.
//! - `ExtHostExtensionServiceShape` não está no protocolo tipado do porte: o [`ExtensionHostProxy`] fala com o [`RpcProxy`] do
//!   identificador `ExtHostExtensionService` ([`EXT_HOST_EXTENSION_SERVICE_SID`]) pelo nome do método, e cada resposta é lida do
//!   [`Dynamic`] que o RPC devolve (`reviveResolveAuthorityResult` incluso).
//! - `test_latency(n)`, `test_up(b)` e `test_down(size)` usam `f64`/`VSBuffer` como o `number` e o `VSBuffer` do original.

use std::rc::Rc;

use je_base::buffer::VSBuffer;
use je_base::dynamic::Dynamic;
use je_base::errors::Error;
use je_base::promise::Promise;
use je_base::uri::URI;
use je_extensions::manifest::ExtensionIdentifier;
use je_platform::remote::remote_authority_resolver::{
    ManagedRemoteConnection, RemoteAuthorityResolverErrorCode, RemoteConnection, ResolvedAuthority, ResolvedAuthenticationSession, ResolvedOptions, ResolverResult,
    WebSocketRemoteConnection,
};

use super::extension_activation::ExtensionActivationReason;
use super::extension_host_protocol::{
    IExtensionDescriptionDelta, environment_to_dynamic, extension_activation_reason_to_dynamic, extension_identifier_to_dynamic,
};
use super::extensions::ActivationKind;
use super::rpc_protocol::RpcProxy;
use crate::api::common::ext_host_values::{ApiValue, vs_buffer_from_dynamic, vs_buffer_to_dynamic};

/// O `sid` de `ExtHostContext.ExtHostExtensionService`.
pub const EXT_HOST_EXTENSION_SERVICE_SID: &str = "ExtHostExtensionService";

/// O `error` de `IResolveAuthorityErrorResult`.
#[derive(Clone, Debug)]
pub struct ResolveAuthorityError {
    pub message: Option<String>,
    pub code: RemoteAuthorityResolverErrorCode,
    pub detail: Option<Dynamic>,
}

/// `IResolveAuthorityResult = IResolveAuthorityErrorResult | IResolveAuthorityOKResult`.
#[derive(Clone, Debug)]
pub enum IResolveAuthorityResult {
    /// `IResolveAuthorityErrorResult` (`type: 'error'`).
    Error(ResolveAuthorityError),
    /// `IResolveAuthorityOKResult` (`type: 'ok'`).
    Ok(ResolverResult),
}

/// `IRemoteConnectionData`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IRemoteConnectionData {
    pub connect_to: RemoteConnection,
    pub connection_token: Option<String>,
}

/// `IExtensionHostProxy`.
pub trait IExtensionHostProxy {
    fn resolve_authority(&self, remote_authority: &str, resolve_attempt: u32) -> Promise<IResolveAuthorityResult>;
    /// Returns `None` if no resolver for `remoteAuthority` is found.
    fn get_canonical_uri(&self, remote_authority: &str, uri: &URI) -> Promise<Option<URI>>;
    fn start_extension_host(&self, extensions_delta: IExtensionDescriptionDelta) -> Promise<()>;
    fn extension_tests_execute(&self) -> Promise<i32>;
    fn activate_by_event(&self, activation_event: &str, activation_kind: ActivationKind) -> Promise<()>;
    fn activate(&self, extension_id: &ExtensionIdentifier, reason: ExtensionActivationReason) -> Promise<bool>;
    fn set_remote_environment(&self, env: Vec<(String, Option<String>)>) -> Promise<()>;
    fn update_remote_connection_data(&self, connection_data: IRemoteConnectionData) -> Promise<()>;
    fn delta_extensions(&self, extensions_delta: IExtensionDescriptionDelta) -> Promise<()>;
    fn test_latency(&self, n: f64) -> Promise<f64>;
    fn test_up(&self, b: VSBuffer) -> Promise<f64>;
    fn test_down(&self, size: usize) -> Promise<VSBuffer>;
}

// #region Codificação no fio (`ExtHostExtensionServiceShape`)

fn remote_connection_to_dynamic(connection: &RemoteConnection) -> Dynamic {
    match connection {
        RemoteConnection::WebSocket(connection) => {
            Dynamic::object([("type", Dynamic::Number(0.0)), ("host", Dynamic::string(&connection.host)), ("port", Dynamic::Number(f64::from(connection.port)))])
        }
        RemoteConnection::Managed(connection) => Dynamic::object([("type", Dynamic::Number(1.0)), ("id", Dynamic::Number(connection.id as f64))]),
    }
}

fn remote_connection_from_dynamic(value: &Dynamic) -> Result<RemoteConnection, Error> {
    match value.get_str("type") {
        Dynamic::Number(kind) if kind == 0.0 => match (value.get_str("host"), value.get_str("port")) {
            (Dynamic::String(host), Dynamic::Number(port)) => Ok(RemoteConnection::WebSocket(WebSocketRemoteConnection::new(host.to_string_lossy(), port as u16))),
            _ => Err(Error::new("conexão WebSocket sem host ou porta")),
        },
        Dynamic::Number(kind) if kind == 1.0 => match value.get_str("id") {
            Dynamic::Number(id) => Ok(RemoteConnection::Managed(ManagedRemoteConnection::new(id as u64))),
            _ => Err(Error::new("conexão gerenciada sem id")),
        },
        _ => Err(Error::new("tipo de conexão remota desconhecido")),
    }
}

/// `IRemoteConnectionData` no fio.
pub fn remote_connection_data_to_dynamic(data: &IRemoteConnectionData) -> Dynamic {
    Dynamic::object([
        ("connectTo", remote_connection_to_dynamic(&data.connect_to)),
        ("connectionToken", data.connection_token.as_ref().map_or(Dynamic::Undefined, |token| Dynamic::string(token))),
    ])
}

fn error_code_from_str(value: &str) -> RemoteAuthorityResolverErrorCode {
    match value {
        "NotAvailable" => RemoteAuthorityResolverErrorCode::NotAvailable,
        "TemporarilyNotAvailable" => RemoteAuthorityResolverErrorCode::TemporarilyNotAvailable,
        "NoResolverFound" => RemoteAuthorityResolverErrorCode::NoResolverFound,
        "InvalidAuthority" => RemoteAuthorityResolverErrorCode::InvalidAuthority,
        _ => RemoteAuthorityResolverErrorCode::Unknown,
    }
}

fn optional_text(value: &Dynamic) -> Option<String> {
    match value {
        Dynamic::String(text) => Some(text.to_string_lossy()),
        _ => None,
    }
}

/// `Dto<IResolveAuthorityResult>` (o lado do host de extensões).
pub fn resolve_authority_result_to_dynamic(result: &IResolveAuthorityResult) -> Dynamic {
    match result {
        IResolveAuthorityResult::Error(error) => Dynamic::object([
            ("type", Dynamic::string("error")),
            (
                "error",
                Dynamic::object([
                    ("message", error.message.as_ref().map_or(Dynamic::Undefined, |message| Dynamic::string(message))),
                    ("code", Dynamic::string(error.code.as_str())),
                    ("detail", error.detail.clone().unwrap_or(Dynamic::Undefined)),
                ]),
            ),
        ]),
        IResolveAuthorityResult::Ok(resolved) => {
            let authority = Dynamic::object([
                ("authority", Dynamic::string(&resolved.authority.authority)),
                ("connectTo", remote_connection_to_dynamic(&resolved.authority.connect_to)),
                ("connectionToken", resolved.authority.connection_token.as_ref().map_or(Dynamic::Undefined, |token| Dynamic::string(token))),
            ]);
            let mut value = vec![("authority", authority)];
            if let Some(options) = &resolved.options {
                let mut entries = Vec::new();
                if let Some(env) = &options.extension_host_env {
                    entries.push(("extensionHostEnv", environment_to_dynamic(env)));
                }
                if let Some(is_trusted) = options.is_trusted {
                    entries.push(("isTrusted", Dynamic::Bool(is_trusted)));
                }
                if let Some(session) = &options.authentication_session {
                    entries.push(("authenticationSession", Dynamic::object([("id", Dynamic::string(&session.id)), ("providerId", Dynamic::string(&session.provider_id))])));
                }
                value.push(("options", Dynamic::object(entries)));
            }
            Dynamic::object([("type", Dynamic::string("ok")), ("value", Dynamic::object(value))])
        }
    }
}

/// `reviveResolveAuthorityResult(result)`.
pub fn resolve_authority_result_from_dynamic(result: &Dynamic) -> Result<IResolveAuthorityResult, Error> {
    match result.get_str("type") {
        Dynamic::String(kind) if kind == "ok" => {
            let value = result.get_str("value");
            let authority = value.get_str("authority");
            let Some(name) = optional_text(&authority.get_str("authority")) else { return Err(Error::new("resolveAuthority: 'authority' ausente")) };
            let options = match value.get_str("options") {
                Dynamic::Object(_) => {
                    let options = value.get_str("options");
                    let extension_host_env = match options.get_str("extensionHostEnv") {
                        env @ Dynamic::Object(_) => Some(super::extension_host_protocol::environment_from_dynamic(&env)?),
                        _ => None,
                    };
                    let authentication_session = match options.get_str("authenticationSession") {
                        session @ Dynamic::Object(_) => {
                            match (optional_text(&session.get_str("id")), optional_text(&session.get_str("providerId"))) {
                                (Some(id), Some(provider_id)) => Some(ResolvedAuthenticationSession { id, provider_id }),
                                _ => None,
                            }
                        }
                        _ => None,
                    };
                    Some(ResolvedOptions {
                        extension_host_env,
                        is_trusted: match options.get_str("isTrusted") {
                            Dynamic::Bool(flag) => Some(flag),
                            _ => None,
                        },
                        authentication_session,
                    })
                }
                _ => None,
            };
            Ok(IResolveAuthorityResult::Ok(ResolverResult {
                authority: ResolvedAuthority {
                    authority: name,
                    connect_to: remote_connection_from_dynamic(&authority.get_str("connectTo"))?,
                    connection_token: optional_text(&authority.get_str("connectionToken")),
                },
                options,
            }))
        }
        Dynamic::String(kind) if kind == "error" => {
            let error = result.get_str("error");
            let detail = match error.get_str("detail") {
                Dynamic::Undefined => None,
                detail => Some(detail),
            };
            Ok(IResolveAuthorityResult::Error(ResolveAuthorityError {
                message: optional_text(&error.get_str("message")),
                code: optional_text(&error.get_str("code")).map_or(RemoteAuthorityResolverErrorCode::Unknown, |code| error_code_from_str(&code)),
                detail,
            }))
        }
        _ => Err(Error::new("resolveAuthority: resultado sem 'type'")),
    }
}

// #endregion

/// `ExtensionHostProxy` (de `mainThreadExtensionService.ts`): cada método chama o `$método` homônimo do `ExtHostExtensionService`.
pub struct ExtensionHostProxy {
    actual: RpcProxy,
}

impl ExtensionHostProxy {
    pub fn new(actual: RpcProxy) -> Rc<Self> {
        Rc::new(Self { actual })
    }

    fn call(&self, method: &str, args: Vec<Dynamic>) -> Promise<Dynamic> {
        self.actual.call(method, args).promise()
    }
}

fn expect_number(value: &Dynamic, what: &str) -> Result<f64, Error> {
    match value {
        Dynamic::Number(number) => Ok(*number),
        other => Err(Error::new(format!("{what}: esperava um número, veio {other:?}"))),
    }
}

impl IExtensionHostProxy for ExtensionHostProxy {
    fn resolve_authority(&self, remote_authority: &str, resolve_attempt: u32) -> Promise<IResolveAuthorityResult> {
        self.call("$resolveAuthority", vec![Dynamic::string(remote_authority), Dynamic::Number(f64::from(resolve_attempt))])
            .then_result(|result| resolve_authority_result_from_dynamic(&result?))
    }

    fn get_canonical_uri(&self, remote_authority: &str, uri: &URI) -> Promise<Option<URI>> {
        self.call("$getCanonicalURI", vec![Dynamic::string(remote_authority), uri.to_dynamic()]).then_result(|result| match result? {
            Dynamic::Null | Dynamic::Undefined => Ok(None),
            components => URI::from_dynamic(&crate::api::common::ext_host_values::revive(&components)).map(Some),
        })
    }

    fn start_extension_host(&self, extensions_delta: IExtensionDescriptionDelta) -> Promise<()> {
        match extensions_delta.to_dynamic() {
            Ok(delta) => self.call("$startExtensionHost", vec![delta]).then(|_| ()),
            Err(error) => Promise::rejected(error),
        }
    }

    fn extension_tests_execute(&self) -> Promise<i32> {
        self.call("$extensionTestsExecute", Vec::new()).then_result(|result| expect_number(&result?, "$extensionTestsExecute").map(|code| code as i32))
    }

    fn activate_by_event(&self, activation_event: &str, activation_kind: ActivationKind) -> Promise<()> {
        self.call("$activateByEvent", vec![Dynamic::string(activation_event), Dynamic::Number(f64::from(activation_kind as i32))]).then(|_| ())
    }

    fn activate(&self, extension_id: &ExtensionIdentifier, reason: ExtensionActivationReason) -> Promise<bool> {
        self.call("$activate", vec![extension_identifier_to_dynamic(extension_id), extension_activation_reason_to_dynamic(&reason)])
            .then(|result| matches!(result, Dynamic::Bool(true)))
    }

    fn set_remote_environment(&self, env: Vec<(String, Option<String>)>) -> Promise<()> {
        self.call("$setRemoteEnvironment", vec![environment_to_dynamic(&env)]).then(|_| ())
    }

    fn update_remote_connection_data(&self, connection_data: IRemoteConnectionData) -> Promise<()> {
        self.call("$updateRemoteConnectionData", vec![remote_connection_data_to_dynamic(&connection_data)]).then(|_| ())
    }

    fn delta_extensions(&self, extensions_delta: IExtensionDescriptionDelta) -> Promise<()> {
        match extensions_delta.to_dynamic() {
            Ok(delta) => self.call("$deltaExtensions", vec![delta]).then(|_| ()),
            Err(error) => Promise::rejected(error),
        }
    }

    fn test_latency(&self, n: f64) -> Promise<f64> {
        self.call("$test_latency", vec![Dynamic::Number(n)]).then_result(|result| expect_number(&result?, "$test_latency"))
    }

    fn test_up(&self, b: VSBuffer) -> Promise<f64> {
        self.call("$test_up", vec![vs_buffer_to_dynamic(&b)]).then_result(|result| expect_number(&result?, "$test_up"))
    }

    fn test_down(&self, size: usize) -> Promise<VSBuffer> {
        self.call("$test_down", vec![Dynamic::Number(size as f64)]).then_result(|result| vs_buffer_from_dynamic(&result?))
    }
}
