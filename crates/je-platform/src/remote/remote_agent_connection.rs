//! Porte parcial de `src/vs/platform/remote/common/remoteAgentConnection.ts`: o handshake que abre uma conexão com o agente
//! remoto (`connectToRemoteExtensionHostAgent`) e o que o túnel de portas usa dele, `connectRemoteAgentTunnel`, com o
//! contrato `IAddressProvider` / `IAddress` e as `IConnectionOptions`.
//!
//! Fora do porte (entram junto de quem os usar): `connectRemoteAgentManagement`, `connectRemoteAgentExtensionHost`, o
//! `createInitialConnection` com as cinco tentativas e a hierarquia `PersistentConnection` (reconexão e seus eventos).
//!
//! Decisões de tradução:
//! - As funções `async` são [`async_fn`] sobre o laço de eventos do porte; `Promise<T>` é a promessa de `je-base`.
//! - O `RemoteAuthorities.getServerRootPath()` (um singleton global) é o campo `server_root_path` das opções, preenchido por
//!   quem monta o serviço (o porte do `network.ts` não tem estado global).
//! - As mensagens do handshake (`AuthRequest`, `SignRequest`, `ConnectionTypeRequest`, `ErrorMessage`) são o JSON como
//!   [`Marshalled`]: o original as lê com `JSON.parse` sem validar além de `type` e `data`, e as escreve com
//!   `JSON.stringify`. A ordem das chaves é a dos literais do original.
//! - `error.code` dos erros do handshake (`ETIMEDOUT`, `VSCODE_CONNECTION_ERROR`) é a propriedade `code` do
//!   [`Error`](je_base::errors::Error).
//! - Um `JSON.parse` que lança dentro do ouvinte da mensagem de controle vai para o `onUnexpectedError`, como o emissor do
//!   original faz com a exceção do ouvinte, e a promessa segue pendente até o prazo.

use std::cell::Cell;
use std::rc::Rc;

use je_base::async_::async_fn;
use je_base::buffer::VSBuffer;
use je_base::cancellation::{CancellationToken, CancellationTokenSource};
use je_base::errors::{self, Error};
use je_base::lifecycle::{DisposableRef, DisposableStore, IDisposable};
use je_base::marshalling::{Marshalled, from_json, json_stringify};
use je_base::parts::ipc::IIPCLogger;
use je_base::parts::ipc::net::{ISocket, PersistentProtocol, PersistentProtocolOptions};
use je_base::promise::{Promise, Resolver};
use je_base::stopwatch::StopWatch;
use je_base::uuid::generate_uuid;
use je_base::{json, performance, runtime};

use super::remote_authority_resolver::RemoteConnection;
use super::remote_socket_factory_service::IRemoteSocketFactoryService;
use crate::log::{LogMessage, LogServiceRef, Logger};
use crate::sign::{IMessage, ISignService};

/// `ConnectionType`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConnectionType {
    Management = 1,
    ExtensionHost = 2,
    Tunnel = 3,
}

/// `connectionTypeToString`.
fn connection_type_to_string(connection_type: ConnectionType) -> &'static str {
    match connection_type {
        ConnectionType::Management => "Management",
        ConnectionType::ExtensionHost => "ExtensionHost",
        ConnectionType::Tunnel => "Tunnel",
    }
}

/// `IAddress`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IAddress {
    pub connect_to: RemoteConnection,
    pub connection_token: Option<String>,
}

/// `IAddressProvider`.
pub trait IAddressProvider {
    fn get_address(&self) -> Promise<IAddress>;
}

/// `IConnectionOptions`.
#[derive(Clone)]
pub struct IConnectionOptions {
    pub commit: Option<String>,
    pub quality: Option<String>,
    pub address_provider: Rc<dyn IAddressProvider>,
    pub remote_socket_factory_service: Rc<dyn IRemoteSocketFactoryService>,
    pub sign_service: Rc<dyn ISignService>,
    pub log_service: LogServiceRef,
    pub ipc_logger: Option<Rc<dyn IIPCLogger>>,
    /// `RemoteAuthorities.getServerRootPath()` (ver o topo).
    pub server_root_path: String,
}

/// `ISimpleConnectionOptions`.
struct SimpleConnectionOptions {
    commit: Option<String>,
    #[allow(dead_code)]
    quality: Option<String>,
    connect_to: RemoteConnection,
    connection_token: Option<String>,
    reconnection_token: String,
    reconnection_protocol: Option<Rc<PersistentProtocol>>,
    remote_socket_factory_service: Rc<dyn IRemoteSocketFactoryService>,
    sign_service: Rc<dyn ISignService>,
    log_service: LogServiceRef,
    server_root_path: String,
}

/// `ITunnelConnectionStartParams`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ITunnelConnectionStartParams {
    pub host: String,
    pub port: u32,
}

const CONNECTION_ERROR_CODE: &str = "VSCODE_CONNECTION_ERROR";
const TIMEOUT_ERROR_CODE: &str = "ETIMEDOUT";

fn has_code(error: &Error, code: &str) -> bool {
    error.prop("code") == Some(code)
}

fn log_error(log: &LogServiceRef, error: &Error) {
    log.error_args(&LogMessage::from(error), &[]);
}

// region: JSON

fn string_value(text: &str) -> Marshalled {
    Marshalled::String(text.into())
}

fn object(entries: Vec<(&str, Marshalled)>) -> Marshalled {
    Marshalled::Object(entries.into_iter().map(|(key, value)| (key.into(), value)).collect())
}

fn field<'a>(value: &'a Marshalled, name: &str) -> Option<&'a Marshalled> {
    match value {
        Marshalled::Object(entries) => entries.iter().find(|(key, _)| key.to_string_lossy() == name).map(|(_, value)| value),
        _ => None,
    }
}

fn text_of(value: &Marshalled) -> Option<String> {
    match value {
        Marshalled::String(text) => Some(text.to_string_lossy()),
        _ => None,
    }
}

/// `JSON.stringify` de uma mensagem do handshake.
fn stringify_message(message: &Marshalled) -> VSBuffer {
    VSBuffer::from_string(&json_stringify(message).map(|text| text.to_string_lossy()).unwrap_or_default())
}

/// `JSON.parse(raw.toString())`.
fn parse_message(raw: &VSBuffer) -> Result<Marshalled, Error> {
    let text = String::from_utf8_lossy(&raw.to_vec()).into_owned();
    let mut parse_errors = Vec::new();
    match json::parse_str(&text, &mut parse_errors) {
        Some(value) if parse_errors.is_empty() => Ok(from_json(&value)),
        _ => Err(Error::named("SyntaxError", format!("Unexpected token in JSON: {text}"))),
    }
}

/// `getErrorFromMessage`.
fn get_error_from_message(msg: &Marshalled) -> Option<Error> {
    if field(msg, "type").and_then(text_of).as_deref() == Some("error") {
        let reason = match field(msg, "reason") {
            Some(Marshalled::String(reason)) => reason.to_string_lossy(),
            Some(Marshalled::Number(number)) => number.to_string(),
            Some(Marshalled::Bool(value)) => value.to_string(),
            Some(Marshalled::Null) => "null".to_owned(),
            _ => "undefined".to_owned(),
        };
        return Some(Error::new(format!("Connection error: {reason}")).with_prop("code", CONNECTION_ERROR_CODE));
    }
    None
}

// endregion

// region: tempo limite

/// `createTimeoutCancellation`.
fn create_timeout_cancellation(millis: u64) -> CancellationToken {
    let source = CancellationTokenSource::new(None);
    let token = source.token();
    runtime::set_timeout(move || source.cancel(), millis);
    token
}

/// `combineTimeoutCancellation`.
fn combine_timeout_cancellation(a: &CancellationToken, b: &CancellationToken) -> CancellationToken {
    if a.is_cancellation_requested() || b.is_cancellation_requested() {
        return CancellationToken::Cancelled;
    }
    let source = CancellationTokenSource::new(None);
    let token = source.token();
    for input in [a, b] {
        let source = source.clone();
        input.on_cancellation_requested().listen(move |_| source.cancel());
    }
    token
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum TimeoutState {
    Pending,
    Resolved,
    Rejected,
    TimedOut,
}

/// `PromiseWithTimeout`.
struct PromiseWithTimeout<T: Clone + 'static> {
    state: Cell<TimeoutState>,
    disposables: Rc<DisposableStore>,
    promise: Promise<T>,
    resolver: Resolver<T>,
}

impl<T: Clone + 'static> PromiseWithTimeout<T> {
    fn new(timeout_cancellation_token: &CancellationToken) -> Rc<Self> {
        let (promise, resolver) = Promise::new();
        let this = Rc::new(Self { state: Cell::new(TimeoutState::Pending), disposables: DisposableStore::new(), promise, resolver });
        if timeout_cancellation_token.is_cancellation_requested() {
            this.timeout();
        } else {
            let on_timeout = this.clone();
            this.disposables.add(timeout_cancellation_token.on_cancellation_requested().listen(move |_| on_timeout.timeout()));
        }
        this
    }

    fn did_timeout(&self) -> bool {
        self.state.get() == TimeoutState::TimedOut
    }

    fn register_disposable(&self, disposable: DisposableRef) {
        if self.state.get() == TimeoutState::Pending {
            self.disposables.add(disposable);
        } else {
            disposable.dispose();
        }
    }

    fn timeout(&self) {
        if self.state.get() != TimeoutState::Pending {
            return;
        }
        self.disposables.dispose();
        self.state.set(TimeoutState::TimedOut);
        self.resolver.reject(Error::new("Time limit reached").with_prop("code", TIMEOUT_ERROR_CODE).with_prop("syscall", "connect"));
    }

    fn resolve(&self, value: T) {
        if self.state.get() != TimeoutState::Pending {
            return;
        }
        self.disposables.dispose();
        self.state.set(TimeoutState::Resolved);
        self.resolver.resolve(value);
    }

    fn reject(&self, error: Error) {
        if self.state.get() != TimeoutState::Pending {
            return;
        }
        self.disposables.dispose();
        self.state.set(TimeoutState::Rejected);
        self.resolver.reject(error);
    }
}

/// `raceWithTimeoutCancellation`.
fn race_with_timeout_cancellation<T: Clone + 'static>(promise: Promise<T>, timeout_cancellation_token: &CancellationToken) -> Promise<T> {
    let result = PromiseWithTimeout::new(timeout_cancellation_token);
    let promise_out = result.promise.clone();
    promise.then_result(move |outcome| {
        if !result.did_timeout() {
            match outcome {
                Ok(value) => result.resolve(value),
                Err(error) => result.reject(error),
            }
        }
        Ok(())
    });
    promise_out
}

/// `readOneControlMessage`.
fn read_one_control_message(protocol: &Rc<PersistentProtocol>, timeout_cancellation_token: &CancellationToken) -> Promise<Marshalled> {
    let result = PromiseWithTimeout::new(timeout_cancellation_token);
    let promise = result.promise.clone();
    let pending = result.clone();
    result.register_disposable(protocol.on_control_message().listen(move |raw| match parse_message(raw) {
        Ok(msg) => match get_error_from_message(&msg) {
            Some(error) => pending.reject(error),
            None => pending.resolve(msg),
        },
        Err(error) => errors::on_unexpected_error(&error),
    }));
    promise
}

// endregion

// region: registro

fn common_log_prefix(connection_type: ConnectionType, reconnection_token: &str, is_reconnect: bool) -> String {
    let token: String = reconnection_token.chars().take(5).collect();
    format!("[remote-connection][{:<13}][{token}…][{}]", connection_type_to_string(connection_type), if is_reconnect { "reconnect" } else { "initial" })
}

fn connect_log_prefix(options: &SimpleConnectionOptions, connection_type: ConnectionType) -> String {
    format!("{}[{}]", common_log_prefix(connection_type, &options.reconnection_token, options.reconnection_protocol.is_some()), options.connect_to)
}

fn log_elapsed(start_time: u64) -> String {
    format!("{} ms", runtime::date_now().saturating_sub(start_time))
}

// endregion

/// `createSocket`.
#[allow(clippy::too_many_arguments)]
fn create_socket(
    log_service: &LogServiceRef,
    remote_socket_factory_service: &Rc<dyn IRemoteSocketFactoryService>,
    connect_to: &RemoteConnection,
    path: &str,
    query: &str,
    debug_connection_type: &str,
    debug_label: &str,
    timeout_cancellation_token: &CancellationToken,
) -> Promise<Rc<dyn ISocket>> {
    let result = PromiseWithTimeout::<Rc<dyn ISocket>>::new(timeout_cancellation_token);
    let promise = result.promise.clone();
    let sw = StopWatch::create(Some(false));
    log_service.info(&format!("Creating a socket ({debug_label})..."));
    performance::mark(&format!("code/willCreateSocket/{debug_connection_type}"), None);

    // O `connect` do original pode lançar (nenhuma fábrica registrada): o lançamento escapa de `createSocket` e o `await`
    // do chamador o vê como rejeição.
    let connecting = match errors::catch(|| remote_socket_factory_service.connect(connect_to, path, query, debug_label)) {
        Ok(connecting) => connecting,
        Err(error) => return Promise::rejected(error),
    };
    let (log_service, debug_connection_type, debug_label) = (log_service.clone(), debug_connection_type.to_owned(), debug_label.to_owned());
    connecting.then_result(move |outcome| {
        match outcome {
            Ok(socket) => {
                if result.did_timeout() {
                    performance::mark(&format!("code/didCreateSocketError/{debug_connection_type}"), None);
                    log_service.info(&format!("Creating a socket ({debug_label}) finished after {} ms, but this is too late and has timed out already.", sw.elapsed()));
                    socket.dispose();
                } else {
                    performance::mark(&format!("code/didCreateSocketOK/{debug_connection_type}"), None);
                    log_service.info(&format!("Creating a socket ({debug_label}) was successful after {} ms.", sw.elapsed()));
                    result.resolve(socket);
                }
            }
            Err(error) => {
                performance::mark(&format!("code/didCreateSocketError/{debug_connection_type}"), None);
                log_service.info(&format!("Creating a socket ({debug_label}) returned an error after {} ms.", sw.elapsed()));
                log_error(&log_service, &error);
                result.reject(error);
            }
        }
        Ok(())
    });
    promise
}

/// `safeDisposeProtocolAndSocket`.
fn safe_dispose_protocol_and_socket(protocol: &PersistentProtocol) {
    let disposed = errors::catch(|| {
        protocol.accept_disconnect();
        let socket = protocol.get_socket();
        protocol.dispose();
        socket.dispose();
    });
    if let Err(error) = disposed {
        errors::on_unexpected_error(&error);
    }
}

/// `connectToRemoteExtensionHostAgent`: devolve o protocolo e se quem chamou é o dono dele (`ownsProtocol`).
fn connect_to_remote_extension_host_agent(
    options: Rc<SimpleConnectionOptions>,
    connection_type: ConnectionType,
    args: Option<Marshalled>,
    timeout_cancellation_token: CancellationToken,
) -> Promise<(Rc<PersistentProtocol>, bool)> {
    async_fn(async move {
        let log = options.log_service.clone();
        let log_prefix = connect_log_prefix(&options, connection_type);

        log.trace(&format!("{log_prefix} 1/6. invoking socketFactory.connect()."));

        let type_name = connection_type_to_string(connection_type);
        let query = format!("reconnectionToken={}&reconnection={}", options.reconnection_token, if options.reconnection_protocol.is_some() { "true" } else { "false" });
        let socket = match create_socket(
            &log,
            &options.remote_socket_factory_service,
            &options.connect_to,
            &options.server_root_path,
            &query,
            type_name,
            &format!("renderer-{type_name}-{}", options.reconnection_token),
            &timeout_cancellation_token,
        )
        .await
        {
            Ok(socket) => socket,
            Err(error) => {
                log.error(&format!("{log_prefix} socketFactory.connect() failed or timed out. Error:"));
                log_error(&log, &error);
                return Err(error);
            }
        };

        log.trace(&format!("{log_prefix} 2/6. socketFactory.connect() was successful."));

        let (protocol, owns_protocol) = match &options.reconnection_protocol {
            Some(reconnection_protocol) => {
                reconnection_protocol.begin_accept_reconnection(socket, None);
                (reconnection_protocol.clone(), false)
            }
            None => (PersistentProtocol::new(PersistentProtocolOptions { socket, initial_chunk: None, load_estimator: None, send_keep_alive: None }), true),
        };

        log.trace(&format!("{log_prefix} 3/6. sending AuthRequest control message."));
        let message = race_with_timeout_cancellation(options.sign_service.create_new_message(&generate_uuid()), &timeout_cancellation_token).await?;

        let connection_token = options.connection_token.as_deref().filter(|token| !token.is_empty()).unwrap_or("00000000000000000000");
        let auth_request = object(vec![("type", string_value("auth")), ("auth", string_value(connection_token)), ("data", string_value(&message.data))]);
        protocol.send_control(stringify_message(&auth_request));

        let handshake: Result<(), Error> = async {
            let msg = read_one_control_message(&protocol, &combine_timeout_cancellation(&timeout_cancellation_token, &create_timeout_cancellation(10000))).await?;

            let sign_data = match (field(&msg, "type").and_then(text_of).as_deref(), field(&msg, "data").and_then(text_of)) {
                (Some("sign"), Some(data)) => data,
                _ => return Err(Error::new("Unexpected handshake message").with_prop("code", CONNECTION_ERROR_CODE)),
            };

            log.trace(&format!("{log_prefix} 4/6. received SignRequest control message."));

            let signed_data = field(&msg, "signedData").and_then(text_of).unwrap_or_default();
            let is_valid = race_with_timeout_cancellation(options.sign_service.validate(&IMessage { id: message.id.clone(), data: message.data.clone() }, &signed_data), &timeout_cancellation_token).await?;
            if !is_valid {
                return Err(Error::new("Refused to connect to unsupported server").with_prop("code", CONNECTION_ERROR_CODE));
            }

            let signed = race_with_timeout_cancellation(options.sign_service.sign(&sign_data), &timeout_cancellation_token).await?;
            let mut entries = vec![
                ("type", string_value("connectionType")),
                ("commit", options.commit.as_deref().map_or(Marshalled::Undefined, string_value)),
                ("signedData", string_value(&signed)),
                ("desiredConnectionType", Marshalled::Number(f64::from(connection_type as u8))),
            ];
            if let Some(args) = &args {
                entries.push(("args", args.clone()));
            }

            log.trace(&format!("{log_prefix} 5/6. sending ConnectionTypeRequest control message."));
            protocol.send_control(stringify_message(&object(entries)));
            Ok::<(), Error>(())
        }
        .await;

        match handshake {
            Ok(()) => Ok((protocol, owns_protocol)),
            Err(error) => {
                if has_code(&error, TIMEOUT_ERROR_CODE) {
                    log.error(&format!("{log_prefix} the handshake timed out. Error:"));
                    log_error(&log, &error);
                }
                if has_code(&error, CONNECTION_ERROR_CODE) {
                    log.error(&format!("{log_prefix} received error control message when negotiating connection. Error:"));
                    log_error(&log, &error);
                }
                if owns_protocol {
                    safe_dispose_protocol_and_socket(&protocol);
                }
                Err(error)
            }
        }
    })
}

/// `connectToRemoteExtensionHostAgentAndReadOneMessage`: devolve o protocolo e a primeira mensagem de controle depois do
/// handshake.
fn connect_to_remote_extension_host_agent_and_read_one_message(
    options: Rc<SimpleConnectionOptions>,
    connection_type: ConnectionType,
    args: Option<Marshalled>,
    timeout_cancellation_token: CancellationToken,
) -> Promise<(Rc<PersistentProtocol>, Marshalled)> {
    async_fn(async move {
        let start_time = runtime::date_now();
        let log_prefix = connect_log_prefix(&options, connection_type);
        let (protocol, owns_protocol) = connect_to_remote_extension_host_agent(options.clone(), connection_type, args, timeout_cancellation_token.clone()).await?;
        let result = PromiseWithTimeout::<(Rc<PersistentProtocol>, Marshalled)>::new(&timeout_cancellation_token);
        let promise = result.promise.clone();
        let pending = result.clone();
        let listening = protocol.clone();
        result.register_disposable(protocol.on_control_message().listen(move |raw| match parse_message(raw) {
            Ok(msg) => match get_error_from_message(&msg) {
                Some(error) => {
                    options.log_service.error(&format!("{log_prefix} received error control message when negotiating connection. Error:"));
                    log_error(&options.log_service, &error);
                    if owns_protocol {
                        safe_dispose_protocol_and_socket(&listening);
                    }
                    pending.reject(error);
                }
                None => {
                    if let Some(reconnection_protocol) = &options.reconnection_protocol {
                        reconnection_protocol.end_accept_reconnection();
                    }
                    options.log_service.trace(&format!("{log_prefix} 6/6. handshake finished, connection is up and running after {}!", log_elapsed(start_time)));
                    pending.resolve((listening.clone(), msg));
                }
            },
            Err(error) => errors::on_unexpected_error(&error),
        }));
        promise.await
    })
}

/// `doConnectRemoteAgentTunnel`.
fn do_connect_remote_agent_tunnel(options: Rc<SimpleConnectionOptions>, start_params: ITunnelConnectionStartParams, timeout_cancellation_token: CancellationToken) -> Promise<Rc<PersistentProtocol>> {
    async_fn(async move {
        let start_time = runtime::date_now();
        let log_prefix = connect_log_prefix(&options, ConnectionType::Tunnel);
        let args = object(vec![("host", string_value(&start_params.host)), ("port", Marshalled::Number(f64::from(start_params.port)))]);
        let (protocol, _first_message) = connect_to_remote_extension_host_agent_and_read_one_message(options.clone(), ConnectionType::Tunnel, Some(args), timeout_cancellation_token).await?;
        options.log_service.trace(&format!("{log_prefix} 6/6. handshake finished, connection is up and running after {}!", log_elapsed(start_time)));
        Ok(protocol)
    })
}

/// `resolveConnectionOptions`.
fn resolve_connection_options(options: &IConnectionOptions, reconnection_token: String, reconnection_protocol: Option<Rc<PersistentProtocol>>) -> Promise<Rc<SimpleConnectionOptions>> {
    let options = options.clone();
    options.address_provider.get_address().then(move |address| {
        Rc::new(SimpleConnectionOptions {
            commit: options.commit,
            quality: options.quality,
            connect_to: address.connect_to,
            connection_token: address.connection_token,
            reconnection_token,
            reconnection_protocol,
            remote_socket_factory_service: options.remote_socket_factory_service,
            sign_service: options.sign_service,
            log_service: options.log_service,
            server_root_path: options.server_root_path,
        })
    })
}

/// `connectRemoteAgentTunnel`: abre, pelo agente remoto, a conexão de túnel até `tunnel_remote_host:tunnel_remote_port`.
pub fn connect_remote_agent_tunnel(options: &IConnectionOptions, tunnel_remote_host: &str, tunnel_remote_port: u32) -> Promise<Rc<PersistentProtocol>> {
    let options = options.clone();
    let start_params = ITunnelConnectionStartParams { host: tunnel_remote_host.to_owned(), port: tunnel_remote_port };
    async_fn(async move {
        let simple_options = resolve_connection_options(&options, generate_uuid(), None).await?;
        do_connect_remote_agent_tunnel(simple_options, start_params, CancellationToken::None).await
    })
}
