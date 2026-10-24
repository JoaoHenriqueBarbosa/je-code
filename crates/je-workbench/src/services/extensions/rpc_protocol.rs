//! Porte de `src/vs/workbench/services/extensions/common/rpcProtocol.ts`, de `lazyPromise.ts` (em `lazy_promise.rs`)
//! e do que `proxyIdentifier.ts` pede do protocolo (`getStringIdentifierForProxy`, `IRPCProtocol`): o RPC entre a thread
//! principal e o host de extensões, sobre um [`IMessagePassingProtocol`].
//!
//! Tudo que o original tem na ida e na volta está aqui: o `MessageBuffer` e o `MessageIO` (mensagens
//! `RequestJSONArgs`, `RequestMixedArgs`, com e sem cancelamento, `Acknowledged`, `Cancel`, `ReplyOKEmpty`,
//! `ReplyOKVSBuffer`, `ReplyOKJSON`, `ReplyOKJSONWithBuffers`, `ReplyErrError`, `ReplyErrEmpty`), a serialização
//! `stringifyJsonWithBufferRefs`/`parseJsonAndRestoreBufferRefs`, o `_remoteCall` e o `_receiveRequest` com
//! cancelamento e confirmação, o estado de responsividade e o `drain`.
//!
//! Divergências:
//! - O `Proxy` do JavaScript (qualquer propriedade `$nome` vira uma chamada remota) não existe em Rust. O proxy é um
//!   [`RpcProxy`] com [`RpcProxy::call`]`(método, argumentos)`, e o objeto local registrado com `set` é um
//!   [`IRpcActor`], que recebe o nome do método e devolve `None` quando ele não existe (o `typeof method !==
//!   'function'` do original). Para o contrato tipado (`getProxy<T>`/`set<T>` do [`IMainContext`]) cada forma `T` é
//!   registrada uma vez com [`RpcProtocol::register_shape`], que diz como montar o `Rc<T>` sobre um [`RpcProxy`] e como
//!   expor um `Rc<T>` local como [`IRpcActor`].
//! - Os valores são [`Dynamic`]. O `VSBuffer` é o valor de `vs_buffer_to_dynamic`, o `SerializableObjectWithBuffers` é
//!   o objeto de carga nativa de [`serializable_object_with_buffers`] e o `CancellationToken` que vai no fim dos
//!   argumentos é o de [`cancellation_token_to_dynamic`].
//! - O que o JavaScript rejeita sem ser um `Error` (um objeto qualquer, `undefined`, `null`) vai codificado num
//!   [`Error`] ([`rejection_with_value`]) e se lê de volta com [`rejected_value`]. O fio é o do original: um erro vai
//!   como `transformErrorForSerialization`, um valor vai como o próprio JSON e `undefined` vai como `ReplyErrEmpty`.
//! - O id numérico do proxy (`nid`) vem de um registro por thread, na ordem do primeiro uso do `sid`; o original
//!   numera na ordem de criação dos `createProxyIdentifier`. Os dois lados têm de viver no mesmo processo (é o que
//!   há aqui, a thread principal e a das extensões dividem o processo).
//! - O `IURITransformer` do construtor não é portado: ele reescreve objetos `$mid` de URI dentro do JSON, e o
//!   [`Dynamic`] não os distingue de um objeto comum. `transformIncomingURIs` não existe pelo mesmo motivo.
//! - `Reflect.get(protocol, '_cancelInvokedHandlers')` do teste vira [`RpcProtocol::cancel_invoked_handler_ids`].
//! - O `replacer` de `stringifyJsonWithBufferRefs` e `createURIReplacer` não existem (só serviam ao transformador).
//! - Os fios de `ArgType.String` e `ReplyOKJSON` com valor sem representação JSON (uma função) viram `null`.

use std::any::Any;
use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, HashMap};
use std::rc::{Rc, Weak};

use je_base::buffer::VSBuffer;
use je_base::cancellation::{CancellationToken, CancellationTokenSource};
use je_base::dynamic::{json_quote, json_stringify, Dynamic, ObjectRef, Truthy};
use je_base::errors::{self, Error};
use je_base::event::{Emitter, Event};
use je_base::js_string::JsString;
use je_base::json::{self, ParseOptions};
use je_base::lifecycle::{Disposable, DisposableRef, DisposableStore, IDisposable};
use je_base::parts::ipc::{IMessagePassingProtocol, RequestInitiator};
use je_base::promise::{MaybePromise, Promise};
use je_base::runtime;
use je_base::async_::RunOnceScheduler;

use super::lazy_promise::LazyPromise;
use crate::api::common::ext_host_protocol::{IMainContext, ProxyIdentifier, SerializableObjectWithBuffers};
use crate::api::common::ext_host_values::{class_of, vs_buffer_from_dynamic, vs_buffer_to_dynamic};

// #region Identificadores, valores e erros

thread_local! {
    static IDENTIFIERS: RefCell<Vec<&'static str>> = const { RefCell::new(Vec::new()) };
}

/// O `nid` do proxy `sid` (a partir de 1, como o `++ProxyIdentifier.count` do original).
pub fn proxy_nid(sid: &'static str) -> u32 {
    IDENTIFIERS.with(|identifiers| {
        let mut identifiers = identifiers.borrow_mut();
        let index = identifiers.iter().position(|known| *known == sid).unwrap_or_else(|| {
            identifiers.push(sid);
            identifiers.len() - 1
        });
        u32::try_from(index + 1).expect("proxy ids fit in u32")
    })
}

/// `getStringIdentifierForProxy(nid)`.
pub fn get_string_identifier_for_proxy(nid: u32) -> &'static str {
    IDENTIFIERS.with(|identifiers| {
        usize::try_from(nid)
            .ok()
            .and_then(|nid| identifiers.borrow().get(nid.wrapping_sub(1)).copied())
            .unwrap_or_else(|| errors::throw(Error::type_error(format!("Cannot read properties of undefined (reading 'sid') for proxy {nid}"))))
    })
}

/// `new SerializableObjectWithBuffers(value)` como valor.
pub fn serializable_object_with_buffers(value: Dynamic) -> Dynamic {
    Dynamic::Object(ObjectRef::with_host(Rc::new(SerializableObjectWithBuffers::new(value))))
}

/// O objeto de um valor criado por [`serializable_object_with_buffers`] (`value instanceof SerializableObjectWithBuffers`).
pub fn as_serializable_object_with_buffers(value: &Dynamic) -> Option<Rc<SerializableObjectWithBuffers<Dynamic>>> {
    match value {
        Dynamic::Object(object) => object.host::<SerializableObjectWithBuffers<Dynamic>>(),
        _ => None,
    }
}

/// Um `CancellationToken` como argumento de uma chamada.
pub fn cancellation_token_to_dynamic(token: CancellationToken) -> Dynamic {
    Dynamic::Object(ObjectRef::with_host(Rc::new(token)))
}

/// `CancellationToken.isCancellationToken(arg)` e o token, se for.
pub fn as_cancellation_token(value: &Dynamic) -> Option<CancellationToken> {
    match value {
        Dynamic::Object(object) => object.host::<CancellationToken>().map(|token| (*token).clone()),
        _ => None,
    }
}

const REJECTED_VALUE_NAME: &str = "RejectedValue";
const REJECTED_UNDEFINED_NAME: &str = "RejectedUndefined";

/// O `Error` que carrega o valor `value` rejeitado por uma chamada (o JavaScript rejeita qualquer valor).
pub fn rejection_with_value(value: &Dynamic) -> Error {
    match json_stringify(value) {
        Some(json) if !value.is_undefined() => Error::named(REJECTED_VALUE_NAME, json),
        _ => Error::named(REJECTED_UNDEFINED_NAME, ""),
    }
}

/// O valor rejeitado que `error` carrega: `Some` para o que [`rejection_with_value`] criou, `None` para um `Error` comum.
pub fn rejected_value(error: &Error) -> Option<Dynamic> {
    match error.name.as_str() {
        REJECTED_UNDEFINED_NAME => Some(Dynamic::Undefined),
        REJECTED_VALUE_NAME => Some(parse_json(&error.message)),
        _ => None,
    }
}

// #endregion

// #region JSON com referências a buffers

const REF_SYMBOL_NAME: &str = "$$ref$$";

/// `StringifiedJsonWithBufferRefs`.
pub struct StringifiedJsonWithBufferRefs {
    pub json_string: String,
    pub referenced_buffers: Vec<VSBuffer>,
}

fn is_vs_buffer(value: &Dynamic) -> bool {
    class_of(value) == Some("VSBuffer")
}

fn stringify_value(value: &Dynamic, buffers: &mut Option<Vec<VSBuffer>>, stack: &mut Vec<usize>) -> Option<String> {
    match value {
        Dynamic::Undefined => buffers.is_some().then(|| format!("{{\"{REF_SYMBOL_NAME}\":-1}}")),
        Dynamic::Object(_) if is_vs_buffer(value) && buffers.is_some() => {
            let buffer = vs_buffer_from_dynamic(value).unwrap_or_else(|error| errors::throw(error));
            let found = buffers.as_mut().expect("conferido acima");
            found.push(buffer);
            Some(format!("{{\"{REF_SYMBOL_NAME}\":{}}}", found.len() - 1))
        }
        Dynamic::Array(_) | Dynamic::Object(_) => {
            let identity = value.identity().expect("array e objeto têm identidade");
            if stack.contains(&identity) {
                errors::throw(Error::type_error("Converting circular structure to JSON"));
            }
            stack.push(identity);
            let text = if let Dynamic::Array(items) = value {
                let parts: Vec<String> = items.to_vec().iter().map(|item| stringify_value(item, buffers, stack).unwrap_or_else(|| "null".to_string())).collect();
                format!("[{}]", parts.join(","))
            } else {
                let parts: Vec<String> = value
                    .own_entries()
                    .iter()
                    .filter_map(|(key, item)| stringify_value(item, buffers, stack).map(|item| format!("{}:{item}", json_quote(key))))
                    .collect();
                format!("{{{}}}", parts.join(","))
            };
            stack.pop();
            Some(text)
        }
        leaf => json_stringify(leaf),
    }
}

/// `JSON.stringify(value)` (lança `TypeError` numa estrutura circular).
fn stringify(value: &Dynamic) -> Option<String> {
    stringify_value(value, &mut None, &mut Vec::new())
}

/// `safeStringify(value, null)`: `"null"` quando a serialização lança.
fn safe_stringify(value: &Dynamic) -> String {
    errors::catch(|| stringify(value)).ok().flatten().unwrap_or_else(|| "null".to_string())
}

/// `stringifyJsonWithBufferRefs(obj, null, useSafeStringify)`.
pub fn stringify_json_with_buffer_refs(obj: &Dynamic, use_safe_stringify: bool) -> StringifiedJsonWithBufferRefs {
    let run = |obj: &Dynamic| {
        let mut buffers = Some(Vec::new());
        let json = stringify_value(obj, &mut buffers, &mut Vec::new());
        (json, buffers.unwrap_or_default())
    };
    let (json, referenced_buffers) = if use_safe_stringify {
        errors::catch(|| run(obj)).unwrap_or((None, Vec::new()))
    } else {
        run(obj)
    };
    StringifiedJsonWithBufferRefs { json_string: json.unwrap_or_else(|| if use_safe_stringify { "null".to_string() } else { errors::throw(Error::type_error("JSON.stringify returned undefined")) }), referenced_buffers }
}

/// `JSON.parse(text)`: lança `SyntaxError` se o texto não for JSON.
fn parse_json(text: &str) -> Dynamic {
    let units: Vec<u16> = text.encode_utf16().collect();
    let mut parse_errors = Vec::new();
    let options = ParseOptions { disallow_comments: true, allow_trailing_comma: false, allow_empty_content: false };
    match json::parse(&units, &mut parse_errors, options).filter(|_| parse_errors.is_empty()) {
        Some(value) => Dynamic::from_json(&value),
        None => errors::throw(Error::named("SyntaxError", format!("Unexpected token in JSON: {text}"))),
    }
}

fn restore_buffer_refs(value: Dynamic, buffers: &[VSBuffer]) -> Dynamic {
    match value {
        Dynamic::Array(items) => Dynamic::array(items.to_vec().into_iter().map(|item| restore_buffer_refs(item, buffers))),
        Dynamic::Object(object) => {
            if let Some(Dynamic::Number(reference)) = object.get(&JsString::from(REF_SYMBOL_NAME)) {
                let found = (reference >= 0.0 && reference.fract() == 0.0).then(|| buffers.get(reference as usize)).flatten();
                return found.map_or(Dynamic::Undefined, vs_buffer_to_dynamic);
            }
            Dynamic::object(object.entries().into_iter().map(|(key, item)| (key, restore_buffer_refs(item, buffers))))
        }
        other => other,
    }
}

/// `parseJsonAndRestoreBufferRefs(jsonString, buffers, null)`.
pub fn parse_json_and_restore_buffer_refs(json_string: &str, buffers: &[VSBuffer]) -> Dynamic {
    restore_buffer_refs(parse_json(json_string), buffers)
}

// #endregion

// #region Contratos

/// `ResponsiveState`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResponsiveState {
    Responsive = 0,
    Unresponsive = 1,
}

/// `IRPCProtocolLogger`. O `data` é o `any` do original (os argumentos viram um array).
pub trait IRpcProtocolLogger {
    fn log_incoming(&self, msg_length: usize, req: u32, initiator: RequestInitiator, str: &str, data: Option<&Dynamic>);
    fn log_outgoing(&self, msg_length: usize, req: u32, initiator: RequestInitiator, str: &str, data: Option<&Dynamic>);
}

/// O objeto local registrado com `set`: recebe a chamada pelo nome do método.
pub trait IRpcActor {
    /// Executa `method`. `None` quando o ator não tem o método. Um erro lançado (pânico) rejeita a chamada.
    fn invoke(&self, method: &str, args: Vec<Dynamic>) -> Option<MaybePromise<Dynamic>>;
}

/// `IRPCProtocol`.
pub trait IRPCProtocol: IMainContext + IDisposable {
    /// `assertRegistered(identifiers)`: falha com `Missing proxy instance <sid>` no primeiro sem instância.
    fn assert_registered(&self, sids: &[&'static str]) -> Result<(), Error>;

    /// `drain()`: espera o buffer de escrita (se houver) esvaziar.
    fn drain(&self) -> Promise<()>;
}

/// O proxy de um objeto do outro lado: `proxy.call("$método", args)` é o `proxy.$método(...args)` do original.
#[derive(Clone)]
pub struct RpcProxy {
    protocol: Weak<RpcProtocol>,
    rpc_id: u32,
}

impl RpcProxy {
    /// O `sid` do proxy (`proxy[Symbol.for('rpcProxy')]`).
    pub fn debug_name(&self) -> &'static str {
        get_string_identifier_for_proxy(self.rpc_id)
    }

    /// `proxy.$method(...args)`. Só métodos que começam com `$` são chamadas remotas (os demais são `undefined` no
    /// `Proxy` do original, e chamá-los lança `TypeError`).
    pub fn call(&self, method: &str, args: Vec<Dynamic>) -> LazyPromise {
        if !method.starts_with('$') {
            errors::throw(Error::type_error(format!("proxy.{method} is not a function")));
        }
        match self.protocol.upgrade() {
            Some(protocol) => protocol.remote_call(self.rpc_id, method, args),
            None => LazyPromise::canceled(),
        }
    }
}

type ProxyFactory = Rc<dyn Fn(RpcProxy) -> Rc<dyn Any>>;
type ActorAdapter = Rc<dyn Fn(Rc<dyn Any>) -> Option<Rc<dyn IRpcActor>>>;

#[derive(Clone)]
struct Shape {
    proxy: ProxyFactory,
    actor: ActorAdapter,
}

// #endregion

const UNRESPONSIVE_TIME: u64 = 3 * 1000;

/// `PendingRPCReply`.
struct PendingRpcReply {
    promise: LazyPromise,
    disposable: Rc<DisposableStore>,
}

impl PendingRpcReply {
    fn resolve_ok(&self, value: Dynamic) {
        self.promise.resolve_ok(value);
        self.disposable.dispose();
    }

    fn resolve_err(&self, error: Error) {
        self.promise.resolve_err(error);
        self.disposable.dispose();
    }
}

/// `RPCProtocol`.
pub struct RpcProtocol {
    me: Weak<RpcProtocol>,
    base: Rc<Disposable>,
    on_did_change_responsive_state: Rc<Emitter<ResponsiveState>>,
    protocol: Rc<dyn IMessagePassingProtocol>,
    logger: Option<Rc<dyn IRpcProtocolLogger>>,
    is_disposed: Cell<bool>,
    locals: RefCell<HashMap<u32, Rc<dyn IRpcActor>>>,
    shapes: RefCell<HashMap<&'static str, Shape>>,
    proxies: RefCell<HashMap<&'static str, Rc<dyn Any>>>,
    last_message_id: Cell<u32>,
    cancel_invoked_handlers: RefCell<Vec<(String, Rc<dyn Fn()>)>>,
    pending_rpc_replies: RefCell<BTreeMap<u32, PendingRpcReply>>,
    responsive_state: Cell<ResponsiveState>,
    unacknowledged_count: Cell<u32>,
    unresponsive_time: Cell<u64>,
    async_check_unresponsive: Rc<RunOnceScheduler>,
}

impl RpcProtocol {
    pub fn new(protocol: Rc<dyn IMessagePassingProtocol>, logger: Option<Rc<dyn IRpcProtocolLogger>>) -> Rc<Self> {
        let this = Rc::new_cyclic(|me: &Weak<RpcProtocol>| {
            let weak = me.clone();
            let async_check_unresponsive = RunOnceScheduler::new(
                move || {
                    if let Some(this) = weak.upgrade() {
                        this.check_unresponsive();
                    }
                },
                1000,
            );
            RpcProtocol {
                me: me.clone(),
                base: Disposable::new(),
                on_did_change_responsive_state: Emitter::new(),
                protocol: protocol.clone(),
                logger,
                is_disposed: Cell::new(false),
                locals: RefCell::default(),
                shapes: RefCell::default(),
                proxies: RefCell::default(),
                last_message_id: Cell::new(0),
                cancel_invoked_handlers: RefCell::default(),
                pending_rpc_replies: RefCell::default(),
                responsive_state: Cell::new(ResponsiveState::Responsive),
                unacknowledged_count: Cell::new(0),
                unresponsive_time: Cell::new(0),
                async_check_unresponsive,
            }
        });
        this.base.register(this.on_did_change_responsive_state.clone());
        this.base.register(this.async_check_unresponsive.clone());
        let weak = Rc::downgrade(&this);
        this.base.register(protocol.on_message().listen(move |message: &VSBuffer| {
            if let Some(this) = weak.upgrade() {
                this.receive_one_message(message);
            }
        }));
        this
    }

    fn this(&self) -> Rc<Self> {
        self.me.upgrade().expect("o protocolo está vivo enquanto um método dele roda")
    }

    pub fn on_did_change_responsive_state(&self) -> Event<ResponsiveState> {
        self.on_did_change_responsive_state.event()
    }

    pub fn responsive_state(&self) -> ResponsiveState {
        self.responsive_state.get()
    }

    /// `drain()`.
    pub fn drain(&self) -> Promise<()> {
        self.protocol.drain().unwrap_or_else(|| Promise::resolved(()))
    }

    /// As chaves de `_cancelInvokedHandlers` (os testes conferem que esvaziam).
    pub fn cancel_invoked_handler_ids(&self) -> Vec<String> {
        self.cancel_invoked_handlers.borrow().iter().map(|(id, _)| id.clone()).collect()
    }

    fn log_incoming(&self, msg_length: usize, req: u32, initiator: RequestInitiator, str: &str, data: Option<&Dynamic>) {
        if let Some(logger) = &self.logger {
            logger.log_incoming(msg_length, req, initiator, str, data);
        }
    }

    fn log_outgoing(&self, msg_length: usize, req: u32, initiator: RequestInitiator, str: &str, data: Option<&Dynamic>) {
        if let Some(logger) = &self.logger {
            logger.log_outgoing(msg_length, req, initiator, str, data);
        }
    }

    fn on_will_send_request(&self) {
        if self.unacknowledged_count.get() == 0 {
            // Since this is the first request we are sending in a while, mark this moment as the start for the countdown to unresponsive time
            self.unresponsive_time.set(runtime::date_now() + UNRESPONSIVE_TIME);
        }
        self.unacknowledged_count.set(self.unacknowledged_count.get() + 1);
        if !self.async_check_unresponsive.is_scheduled() {
            self.async_check_unresponsive.schedule(None);
        }
    }

    fn on_did_receive_acknowledge(&self) {
        // The next possible unresponsive time is now + delta.
        self.unresponsive_time.set(runtime::date_now() + UNRESPONSIVE_TIME);
        self.unacknowledged_count.set(self.unacknowledged_count.get().wrapping_sub(1));
        if self.unacknowledged_count.get() == 0 {
            // No more need to check for unresponsive
            self.async_check_unresponsive.cancel();
        }
        // The ext host is responsive!
        self.set_responsive_state(ResponsiveState::Responsive);
    }

    fn check_unresponsive(&self) {
        if self.unacknowledged_count.get() == 0 {
            // Not waiting for anything => cannot say if it is responsive or not
            return;
        }
        if runtime::date_now() > self.unresponsive_time.get() {
            // Unresponsive!!
            self.set_responsive_state(ResponsiveState::Unresponsive);
        } else {
            // Not (yet) unresponsive, be sure to check again soon
            self.async_check_unresponsive.schedule(None);
        }
    }

    fn set_responsive_state(&self, new_responsive_state: ResponsiveState) {
        if self.responsive_state.get() == new_responsive_state {
            // no change
            return;
        }
        self.responsive_state.set(new_responsive_state);
        self.on_did_change_responsive_state.fire(new_responsive_state);
    }

    /// `getProxy(identifier)`, sem tipo.
    pub fn get_proxy_untyped(&self, sid: &'static str) -> RpcProxy {
        RpcProxy { protocol: self.me.clone(), rpc_id: proxy_nid(sid) }
    }

    /// `getProxy<T>(identifier)`, sem tipo: o proxy de chamadas por nome.
    pub fn proxy<T: ?Sized + 'static>(&self, identifier: &ProxyIdentifier<T>) -> RpcProxy {
        self.get_proxy_untyped(identifier.sid)
    }

    /// `set(identifier, value)` com um ator que despacha por nome.
    pub fn set_actor(&self, sid: &'static str, actor: Rc<dyn IRpcActor>) -> Rc<dyn IRpcActor> {
        self.locals.borrow_mut().insert(proxy_nid(sid), actor.clone());
        actor
    }

    /// Ensina o protocolo a montar o `Rc<T>` de proxy e a expor um `Rc<T>` local, para o contrato tipado do [`IMainContext`].
    pub fn register_shape<T: ?Sized + 'static>(
        &self,
        identifier: &ProxyIdentifier<T>,
        proxy: impl Fn(RpcProxy) -> Rc<T> + 'static,
        actor: impl Fn(Rc<T>) -> Rc<dyn IRpcActor> + 'static,
    ) {
        let shape = Shape {
            proxy: Rc::new(move |rpc_proxy| Rc::new(proxy(rpc_proxy)) as Rc<dyn Any>),
            actor: Rc::new(move |value| value.downcast_ref::<Rc<T>>().map(|value| actor(value.clone()))),
        };
        self.shapes.borrow_mut().insert(identifier.sid, shape);
    }

    fn receive_one_message(&self, rawmsg: &VSBuffer) {
        if self.is_disposed.get() {
            return;
        }
        let msg_length = rawmsg.byte_length();
        let mut buff = MessageBuffer::read(rawmsg.clone(), 0);
        let message_type = buff.read_u8();
        let req = buff.read_u32();

        match message_type {
            MESSAGE_REQUEST_JSON_ARGS | MESSAGE_REQUEST_JSON_ARGS_WITH_CANCELLATION => {
                let (rpc_id, method, args) = MessageIO::deserialize_request_json_args(&mut buff);
                self.receive_request(msg_length, req, rpc_id, &method, args, message_type == MESSAGE_REQUEST_JSON_ARGS_WITH_CANCELLATION);
            }
            MESSAGE_REQUEST_MIXED_ARGS | MESSAGE_REQUEST_MIXED_ARGS_WITH_CANCELLATION => {
                let (rpc_id, method, args) = MessageIO::deserialize_request_mixed_args(&mut buff);
                self.receive_request(msg_length, req, rpc_id, &method, args, message_type == MESSAGE_REQUEST_MIXED_ARGS_WITH_CANCELLATION);
            }
            MESSAGE_ACKNOWLEDGED => {
                self.log_incoming(msg_length, req, RequestInitiator::LocalSide, "ack", None);
                self.on_did_receive_acknowledge();
            }
            MESSAGE_CANCEL => self.receive_cancel(msg_length, req),
            MESSAGE_REPLY_OK_EMPTY => self.receive_reply(msg_length, req, Dynamic::Undefined),
            MESSAGE_REPLY_OK_JSON => {
                let value = MessageIO::deserialize_reply_ok_json(&mut buff);
                self.receive_reply(msg_length, req, value);
            }
            MESSAGE_REPLY_OK_JSON_WITH_BUFFERS => {
                let value = MessageIO::deserialize_reply_ok_json_with_buffers(&mut buff);
                self.receive_reply(msg_length, req, value);
            }
            MESSAGE_REPLY_OK_VSBUFFER => {
                let value = MessageIO::deserialize_reply_ok_vs_buffer(&mut buff);
                self.receive_reply(msg_length, req, value);
            }
            MESSAGE_REPLY_ERR_ERROR => {
                let err = MessageIO::deserialize_reply_err_error(&mut buff);
                self.receive_reply_err(msg_length, req, err);
            }
            MESSAGE_REPLY_ERR_EMPTY => self.receive_reply_err(msg_length, req, Dynamic::Undefined),
            _ => eprintln!("received unexpected message"),
        }
    }

    fn receive_request(&self, msg_length: usize, req: u32, rpc_id: u32, method: &str, mut args: Vec<Dynamic>, uses_cancellation_token: bool) {
        self.log_incoming(
            msg_length,
            req,
            RequestInitiator::OtherSide,
            &format!("receiveRequest {}.{method}(", get_string_identifier_for_proxy(rpc_id)),
            Some(&Dynamic::array(args.iter().cloned())),
        );
        let call_id = req.to_string();

        let promise;
        let mut cancel: Option<Rc<dyn Fn()>> = None;
        if uses_cancellation_token {
            let cancellation_token_source = CancellationTokenSource::new(None);
            args.push(cancellation_token_to_dynamic(cancellation_token_source.token()));
            promise = self.invoke_handler(rpc_id, method, args);
            cancel = Some(Rc::new(move || cancellation_token_source.cancel()));
        } else {
            promise = self.invoke_handler(rpc_id, method, args);
        }

        if let Some(cancel) = cancel {
            self.cancel_invoked_handlers.borrow_mut().push((call_id.clone(), cancel));
        }

        // Acknowledge the request
        let msg = MessageIO::serialize_acknowledged(req);
        self.log_outgoing(msg.byte_length(), req, RequestInitiator::OtherSide, "ack", None);
        self.protocol.send(msg);

        let this = self.this();
        let _ = promise.then_result(move |result| {
            this.delete_cancel_handler(&call_id);
            match result {
                Ok(value) => {
                    let msg = MessageIO::serialize_reply_ok(req, &value);
                    this.log_outgoing(msg.byte_length(), req, RequestInitiator::OtherSide, "reply:", Some(&value));
                    this.protocol.send(msg);
                }
                Err(error) => {
                    let msg = MessageIO::serialize_reply_err(req, &error);
                    this.log_outgoing(msg.byte_length(), req, RequestInitiator::OtherSide, "replyErr:", rejected_value(&error).as_ref());
                    this.protocol.send(msg);
                }
            }
            Ok(())
        });
    }

    fn delete_cancel_handler(&self, call_id: &str) -> Option<Rc<dyn Fn()>> {
        let mut handlers = self.cancel_invoked_handlers.borrow_mut();
        let index = handlers.iter().position(|(id, _)| id == call_id)?;
        Some(handlers.remove(index).1)
    }

    fn receive_cancel(&self, msg_length: usize, req: u32) {
        self.log_incoming(msg_length, req, RequestInitiator::OtherSide, "receiveCancel", None);
        if let Some(cancel) = self.delete_cancel_handler(&req.to_string()) {
            cancel();
        }
    }

    fn receive_reply(&self, msg_length: usize, req: u32, value: Dynamic) {
        self.log_incoming(msg_length, req, RequestInitiator::LocalSide, "receiveReply:", Some(&value));
        let pending_reply = self.pending_rpc_replies.borrow_mut().remove(&req);
        if let Some(pending_reply) = pending_reply {
            pending_reply.resolve_ok(value);
        }
    }

    fn receive_reply_err(&self, msg_length: usize, req: u32, value: Dynamic) {
        self.log_incoming(msg_length, req, RequestInitiator::LocalSide, "receiveReplyErr:", Some(&value));
        let pending_reply = self.pending_rpc_replies.borrow_mut().remove(&req);
        let Some(pending_reply) = pending_reply else {
            return;
        };
        let error = if value.is_truthy() {
            if value.get_str("$isError").is_truthy() {
                let text = |key: &str| match value.get_str(key) {
                    Dynamic::String(text) => text.to_string_lossy(),
                    _ => String::new(),
                };
                let mut error = Error::named(text("name"), text("message"));
                if let Dynamic::String(stack) = value.get_str("stack") {
                    error.stack = Some(stack.to_string_lossy());
                }
                error
            } else {
                rejection_with_value(&value)
            }
        } else {
            rejection_with_value(&Dynamic::Undefined)
        };
        pending_reply.resolve_err(error);
    }

    fn invoke_handler(&self, rpc_id: u32, method_name: &str, args: Vec<Dynamic>) -> Promise<Dynamic> {
        match errors::catch(|| self.do_invoke_handler(rpc_id, method_name, args)) {
            Ok(MaybePromise::Promise(promise)) => promise,
            Ok(MaybePromise::Value(value)) => Promise::resolved(value),
            Err(error) => Promise::rejected(error),
        }
    }

    fn do_invoke_handler(&self, rpc_id: u32, method_name: &str, args: Vec<Dynamic>) -> MaybePromise<Dynamic> {
        let actor = self.locals.borrow().get(&rpc_id).cloned();
        let Some(actor) = actor else {
            errors::throw(Error::new(format!("Unknown actor {}", get_string_identifier_for_proxy(rpc_id))));
        };
        actor.invoke(method_name, args).unwrap_or_else(|| {
            errors::throw(Error::new(format!("Unknown method {method_name} on actor {}", get_string_identifier_for_proxy(rpc_id))))
        })
    }

    /// `_remoteCall(rpcId, methodName, args)`. Lança se um argumento não pode ser serializado.
    pub fn remote_call(&self, rpc_id: u32, method_name: &str, mut args: Vec<Dynamic>) -> LazyPromise {
        if self.is_disposed.get() {
            return LazyPromise::canceled();
        }
        let cancellation_token = args.last().and_then(as_cancellation_token);
        if cancellation_token.is_some() {
            args.pop();
        }

        if cancellation_token.as_ref().is_some_and(CancellationToken::is_cancellation_requested) {
            // No need to do anything...
            return LazyPromise::from_promise(Promise::rejected(Error::cancellation()));
        }

        let serialized_request_arguments = MessageIO::serialize_request_arguments(&args);

        let req = self.last_message_id.get() + 1;
        self.last_message_id.set(req);
        let result = LazyPromise::new();

        let disposable = DisposableStore::new();
        if let Some(token) = &cancellation_token {
            let weak = self.me.clone();
            disposable.add(token.on_cancellation_requested().listen(move |_| {
                if let Some(this) = weak.upgrade() {
                    let msg = MessageIO::serialize_cancel(req);
                    this.log_outgoing(msg.byte_length(), req, RequestInitiator::LocalSide, "cancel", None);
                    this.protocol.send(msg);
                }
            }));
        }

        self.pending_rpc_replies.borrow_mut().insert(req, PendingRpcReply { promise: result.clone(), disposable });
        self.on_will_send_request();
        let msg = MessageIO::serialize_request(req, rpc_id, method_name, &serialized_request_arguments, cancellation_token.is_some());
        self.log_outgoing(
            msg.byte_length(),
            req,
            RequestInitiator::LocalSide,
            &format!("request: {}.{method_name}(", get_string_identifier_for_proxy(rpc_id)),
            Some(&Dynamic::array(args.iter().cloned())),
        );
        self.protocol.send(msg);
        result
    }
}

impl IDisposable for RpcProtocol {
    fn dispose(&self) {
        self.is_disposed.set(true);

        // Release all outstanding promises with a canceled error
        let pending = std::mem::take(&mut *self.pending_rpc_replies.borrow_mut());
        for (_, pending) in pending {
            pending.resolve_err(Error::cancellation());
        }

        self.base.dispose();
    }
}

impl IMainContext for RpcProtocol {
    fn get_proxy_erased(&self, sid: &'static str) -> Rc<dyn Any> {
        if let Some(proxy) = self.proxies.borrow().get(sid) {
            return proxy.clone();
        }
        let shape = self.shapes.borrow().get(sid).cloned().unwrap_or_else(|| panic!("o proxy '{sid}' não tem a forma registrada (register_shape)"));
        let proxy = (shape.proxy)(self.get_proxy_untyped(sid));
        self.proxies.borrow_mut().insert(sid, proxy.clone());
        proxy
    }

    fn set_erased(&self, sid: &'static str, value: Rc<dyn Any>) {
        let shape = self.shapes.borrow().get(sid).cloned().unwrap_or_else(|| panic!("o proxy '{sid}' não tem a forma registrada (register_shape)"));
        let actor = (shape.actor)(value).unwrap_or_else(|| panic!("o objeto de '{sid}' não tem a forma registrada"));
        self.set_actor(sid, actor);
    }
}

impl IRPCProtocol for RpcProtocol {
    fn assert_registered(&self, sids: &[&'static str]) -> Result<(), Error> {
        let locals = self.locals.borrow();
        match sids.iter().copied().find(|sid| !locals.contains_key(&proxy_nid(*sid))) {
            Some(sid) => Err(Error::new(format!("Missing proxy instance {sid}"))),
            None => Ok(()),
        }
    }

    fn drain(&self) -> Promise<()> {
        RpcProtocol::drain(self)
    }
}

// #region MessageBuffer e MessageIO

const MESSAGE_REQUEST_JSON_ARGS: u8 = 1;
const MESSAGE_REQUEST_JSON_ARGS_WITH_CANCELLATION: u8 = 2;
const MESSAGE_REQUEST_MIXED_ARGS: u8 = 3;
const MESSAGE_REQUEST_MIXED_ARGS_WITH_CANCELLATION: u8 = 4;
const MESSAGE_ACKNOWLEDGED: u8 = 5;
const MESSAGE_CANCEL: u8 = 6;
const MESSAGE_REPLY_OK_EMPTY: u8 = 7;
const MESSAGE_REPLY_OK_VSBUFFER: u8 = 8;
const MESSAGE_REPLY_OK_JSON: u8 = 9;
const MESSAGE_REPLY_OK_JSON_WITH_BUFFERS: u8 = 10;
const MESSAGE_REPLY_ERR_ERROR: u8 = 11;
const MESSAGE_REPLY_ERR_EMPTY: u8 = 12;

const ARG_TYPE_STRING: u8 = 1;
const ARG_TYPE_VSBUFFER: u8 = 2;
const ARG_TYPE_SERIALIZED_OBJECT_WITH_BUFFERS: u8 = 3;
const ARG_TYPE_UNDEFINED: u8 = 4;

/// `MixedArg`.
enum MixedArg {
    String(VSBuffer),
    VsBuffer(VSBuffer),
    SerializedObjectWithBuffers { value: VSBuffer, buffers: Vec<VSBuffer> },
    Undefined,
}

/// `SerializedRequestArguments`.
enum SerializedRequestArguments {
    Simple(String),
    Mixed(Vec<MixedArg>),
}

fn buffer_text(buffer: &VSBuffer) -> String {
    buffer.with_bytes(|bytes| String::from_utf8_lossy(bytes).into_owned())
}

/// `MessageBuffer`.
pub struct MessageBuffer {
    buff: VSBuffer,
    offset: usize,
}

impl MessageBuffer {
    pub const SIZE_U_INT32: usize = 4;

    pub fn alloc(message_type: u8, req: u32, message_size: usize) -> Self {
        let mut result = Self::read(VSBuffer::alloc(message_size + 1 /* type */ + 4 /* req */), 0);
        result.write_u8(message_type);
        result.write_u32(req);
        result
    }

    pub fn read(buff: VSBuffer, offset: usize) -> Self {
        Self { buff, offset }
    }

    pub fn buffer(&self) -> VSBuffer {
        self.buff.clone()
    }

    pub fn size_u8() -> usize {
        1
    }

    pub fn write_u8(&mut self, n: u8) {
        self.buff.write_u_int8(n, self.offset);
        self.offset += 1;
    }

    pub fn read_u8(&mut self) -> u8 {
        let n = self.buff.read_u_int8(self.offset);
        self.offset += 1;
        n
    }

    pub fn write_u32(&mut self, n: u32) {
        self.buff.write_u_int32_be(n, self.offset);
        self.offset += 4;
    }

    pub fn read_u32(&mut self) -> u32 {
        let n = self.buff.read_u_int32_be(self.offset);
        self.offset += 4;
        n
    }

    pub fn size_short_string(str: &VSBuffer) -> usize {
        1 /* string length */ + str.byte_length() /* actual string */
    }

    pub fn write_short_string(&mut self, str: &VSBuffer) {
        self.buff.write_u_int8(str.byte_length() as u8, self.offset);
        self.offset += 1;
        self.buff.set(str, self.offset);
        self.offset += str.byte_length();
    }

    pub fn read_short_string(&mut self) -> String {
        let str_byte_length = usize::from(self.buff.read_u_int8(self.offset));
        self.offset += 1;
        let str_buff = self.buff.slice(Some(self.offset as i64), Some((self.offset + str_byte_length) as i64));
        self.offset += str_byte_length;
        buffer_text(&str_buff)
    }

    pub fn size_long_string(str: &VSBuffer) -> usize {
        4 /* string length */ + str.byte_length() /* actual string */
    }

    pub fn write_long_string(&mut self, str: &VSBuffer) {
        self.buff.write_u_int32_be(str.byte_length() as u32, self.offset);
        self.offset += 4;
        self.buff.set(str, self.offset);
        self.offset += str.byte_length();
    }

    pub fn read_long_string(&mut self) -> String {
        let str_byte_length = self.buff.read_u_int32_be(self.offset) as usize;
        self.offset += 4;
        let str_buff = self.buff.slice(Some(self.offset as i64), Some((self.offset + str_byte_length) as i64));
        self.offset += str_byte_length;
        buffer_text(&str_buff)
    }

    pub fn write_buffer(&mut self, buff: &VSBuffer) {
        self.write_vs_buffer(buff);
    }

    pub fn size_vs_buffer(buff: &VSBuffer) -> usize {
        4 /* buffer length */ + buff.byte_length() /* actual buffer */
    }

    pub fn write_vs_buffer(&mut self, buff: &VSBuffer) {
        self.buff.write_u_int32_be(buff.byte_length() as u32, self.offset);
        self.offset += 4;
        self.buff.set(buff, self.offset);
        self.offset += buff.byte_length();
    }

    pub fn read_vs_buffer(&mut self) -> VSBuffer {
        let buff_length = self.buff.read_u_int32_be(self.offset) as usize;
        self.offset += 4;
        let buff = self.buff.slice(Some(self.offset as i64), Some((self.offset + buff_length) as i64));
        self.offset += buff_length;
        buff
    }

    fn size_mixed_array(arr: &[MixedArg]) -> usize {
        let mut size = 0;
        size += 1; // arr length
        for el in arr {
            size += 1; // arg type
            match el {
                MixedArg::String(value) => size += Self::size_long_string(value),
                MixedArg::VsBuffer(value) => size += Self::size_vs_buffer(value),
                MixedArg::SerializedObjectWithBuffers { value, buffers } => {
                    size += Self::SIZE_U_INT32; // buffer count
                    size += Self::size_long_string(value);
                    for buffer in buffers {
                        size += Self::size_vs_buffer(buffer);
                    }
                }
                MixedArg::Undefined => {
                    // empty...
                }
            }
        }
        size
    }

    fn write_mixed_array(&mut self, arr: &[MixedArg]) {
        self.buff.write_u_int8(arr.len() as u8, self.offset);
        self.offset += 1;
        for el in arr {
            match el {
                MixedArg::String(value) => {
                    self.write_u8(ARG_TYPE_STRING);
                    self.write_long_string(value);
                }
                MixedArg::VsBuffer(value) => {
                    self.write_u8(ARG_TYPE_VSBUFFER);
                    self.write_vs_buffer(value);
                }
                MixedArg::SerializedObjectWithBuffers { value, buffers } => {
                    self.write_u8(ARG_TYPE_SERIALIZED_OBJECT_WITH_BUFFERS);
                    self.write_u32(buffers.len() as u32);
                    self.write_long_string(value);
                    for buffer in buffers {
                        self.write_buffer(buffer);
                    }
                }
                MixedArg::Undefined => self.write_u8(ARG_TYPE_UNDEFINED),
            }
        }
    }

    /// `readMixedArray()` junto do `JSON.parse` das strings que `deserializeRequestMixedArgs` faz em seguida.
    fn read_mixed_array(&mut self) -> Vec<Dynamic> {
        let arr_len = usize::from(self.buff.read_u_int8(self.offset));
        self.offset += 1;
        let mut arr = Vec::with_capacity(arr_len);
        for _ in 0..arr_len {
            let arg_type = self.read_u8();
            arr.push(match arg_type {
                ARG_TYPE_STRING => parse_json(&self.read_long_string()),
                ARG_TYPE_VSBUFFER => vs_buffer_to_dynamic(&self.read_vs_buffer()),
                ARG_TYPE_SERIALIZED_OBJECT_WITH_BUFFERS => {
                    let buffer_count = self.read_u32();
                    let json_string = self.read_long_string();
                    let buffers: Vec<VSBuffer> = (0..buffer_count).map(|_| self.read_vs_buffer()).collect();
                    serializable_object_with_buffers(parse_json_and_restore_buffer_refs(&json_string, &buffers))
                }
                // ARG_TYPE_UNDEFINED, e o que o switch do original deixa sem preencher (`undefined`).
                _ => Dynamic::Undefined,
            });
        }
        arr
    }
}

/// `MessageIO`.
struct MessageIO;

impl MessageIO {
    fn use_mixed_arg_serialization(arr: &[Dynamic]) -> bool {
        arr.iter().any(|arg| is_vs_buffer(arg) || as_serializable_object_with_buffers(arg).is_some() || arg.is_undefined())
    }

    fn serialize_request_arguments(args: &[Dynamic]) -> SerializedRequestArguments {
        if Self::use_mixed_arg_serialization(args) {
            let massaged_args = args
                .iter()
                .map(|arg| {
                    if is_vs_buffer(arg) {
                        MixedArg::VsBuffer(vs_buffer_from_dynamic(arg).unwrap_or_else(|error| errors::throw(error)))
                    } else if arg.is_undefined() {
                        MixedArg::Undefined
                    } else if let Some(object) = as_serializable_object_with_buffers(arg) {
                        let StringifiedJsonWithBufferRefs { json_string, referenced_buffers } = stringify_json_with_buffer_refs(&object.value, false);
                        MixedArg::SerializedObjectWithBuffers { value: VSBuffer::from_string(&json_string), buffers: referenced_buffers }
                    } else {
                        MixedArg::String(VSBuffer::from_string(&stringify(arg).unwrap_or_else(|| "null".to_string())))
                    }
                })
                .collect();
            return SerializedRequestArguments::Mixed(massaged_args);
        }
        SerializedRequestArguments::Simple(stringify(&Dynamic::array(args.iter().cloned())).unwrap_or_else(|| "null".to_string()))
    }

    fn serialize_request(req: u32, rpc_id: u32, method: &str, serialized_args: &SerializedRequestArguments, uses_cancellation_token: bool) -> VSBuffer {
        match serialized_args {
            SerializedRequestArguments::Simple(args) => Self::request_json_args(req, rpc_id, method, args, uses_cancellation_token),
            SerializedRequestArguments::Mixed(args) => Self::request_mixed_args(req, rpc_id, method, args, uses_cancellation_token),
        }
    }

    fn request_json_args(req: u32, rpc_id: u32, method: &str, args: &str, uses_cancellation_token: bool) -> VSBuffer {
        let method_buff = VSBuffer::from_string(method);
        let args_buff = VSBuffer::from_string(args);

        let mut len = 0;
        len += MessageBuffer::size_u8();
        len += MessageBuffer::size_short_string(&method_buff);
        len += MessageBuffer::size_long_string(&args_buff);

        let message_type = if uses_cancellation_token { MESSAGE_REQUEST_JSON_ARGS_WITH_CANCELLATION } else { MESSAGE_REQUEST_JSON_ARGS };
        let mut result = MessageBuffer::alloc(message_type, req, len);
        result.write_u8(rpc_id as u8);
        result.write_short_string(&method_buff);
        result.write_long_string(&args_buff);
        result.buffer()
    }

    fn deserialize_request_json_args(buff: &mut MessageBuffer) -> (u32, String, Vec<Dynamic>) {
        let rpc_id = u32::from(buff.read_u8());
        let method = buff.read_short_string();
        let args = buff.read_long_string();
        let args = match parse_json(&args) {
            Dynamic::Array(items) => items.to_vec(),
            _ => Vec::new(),
        };
        (rpc_id, method, args)
    }

    fn request_mixed_args(req: u32, rpc_id: u32, method: &str, args: &[MixedArg], uses_cancellation_token: bool) -> VSBuffer {
        let method_buff = VSBuffer::from_string(method);

        let mut len = 0;
        len += MessageBuffer::size_u8();
        len += MessageBuffer::size_short_string(&method_buff);
        len += MessageBuffer::size_mixed_array(args);

        let message_type = if uses_cancellation_token { MESSAGE_REQUEST_MIXED_ARGS_WITH_CANCELLATION } else { MESSAGE_REQUEST_MIXED_ARGS };
        let mut result = MessageBuffer::alloc(message_type, req, len);
        result.write_u8(rpc_id as u8);
        result.write_short_string(&method_buff);
        result.write_mixed_array(args);
        result.buffer()
    }

    fn deserialize_request_mixed_args(buff: &mut MessageBuffer) -> (u32, String, Vec<Dynamic>) {
        let rpc_id = u32::from(buff.read_u8());
        let method = buff.read_short_string();
        let args = buff.read_mixed_array();
        (rpc_id, method, args)
    }

    fn serialize_acknowledged(req: u32) -> VSBuffer {
        MessageBuffer::alloc(MESSAGE_ACKNOWLEDGED, req, 0).buffer()
    }

    fn serialize_cancel(req: u32) -> VSBuffer {
        MessageBuffer::alloc(MESSAGE_CANCEL, req, 0).buffer()
    }

    fn serialize_reply_ok(req: u32, res: &Dynamic) -> VSBuffer {
        if res.is_undefined() {
            Self::serialize_reply_ok_empty(req)
        } else if is_vs_buffer(res) {
            Self::serialize_reply_ok_vs_buffer(req, &vs_buffer_from_dynamic(res).unwrap_or_else(|error| errors::throw(error)))
        } else if let Some(object) = as_serializable_object_with_buffers(res) {
            let StringifiedJsonWithBufferRefs { json_string, referenced_buffers } = stringify_json_with_buffer_refs(&object.value, true);
            Self::serialize_reply_ok_json_with_buffers(req, &json_string, &referenced_buffers)
        } else {
            Self::serialize_reply_ok_json(req, &safe_stringify(res))
        }
    }

    fn serialize_reply_ok_empty(req: u32) -> VSBuffer {
        MessageBuffer::alloc(MESSAGE_REPLY_OK_EMPTY, req, 0).buffer()
    }

    fn serialize_reply_ok_vs_buffer(req: u32, res: &VSBuffer) -> VSBuffer {
        let len = MessageBuffer::size_vs_buffer(res);
        let mut result = MessageBuffer::alloc(MESSAGE_REPLY_OK_VSBUFFER, req, len);
        result.write_vs_buffer(res);
        result.buffer()
    }

    fn deserialize_reply_ok_vs_buffer(buff: &mut MessageBuffer) -> Dynamic {
        vs_buffer_to_dynamic(&buff.read_vs_buffer())
    }

    fn serialize_reply_ok_json(req: u32, res: &str) -> VSBuffer {
        let res_buff = VSBuffer::from_string(res);
        let len = MessageBuffer::size_long_string(&res_buff);
        let mut result = MessageBuffer::alloc(MESSAGE_REPLY_OK_JSON, req, len);
        result.write_long_string(&res_buff);
        result.buffer()
    }

    fn serialize_reply_ok_json_with_buffers(req: u32, res: &str, buffers: &[VSBuffer]) -> VSBuffer {
        let res_buff = VSBuffer::from_string(res);

        let mut len = 0;
        len += MessageBuffer::SIZE_U_INT32; // buffer count
        len += MessageBuffer::size_long_string(&res_buff);
        for buffer in buffers {
            len += MessageBuffer::size_vs_buffer(buffer);
        }

        let mut result = MessageBuffer::alloc(MESSAGE_REPLY_OK_JSON_WITH_BUFFERS, req, len);
        result.write_u32(buffers.len() as u32);
        result.write_long_string(&res_buff);
        for buffer in buffers {
            result.write_buffer(buffer);
        }
        result.buffer()
    }

    fn deserialize_reply_ok_json(buff: &mut MessageBuffer) -> Dynamic {
        parse_json(&buff.read_long_string())
    }

    fn deserialize_reply_ok_json_with_buffers(buff: &mut MessageBuffer) -> Dynamic {
        let buffer_count = buff.read_u32();
        let res = buff.read_long_string();
        let buffers: Vec<VSBuffer> = (0..buffer_count).map(|_| buff.read_vs_buffer()).collect();
        serializable_object_with_buffers(parse_json_and_restore_buffer_refs(&res, &buffers))
    }

    /// `serializeReplyErr`: um erro vai como `transformErrorForSerialization`, o valor rejeitado vai como o próprio
    /// JSON e o que não é verdadeiro (`undefined`, `null`, `0`, `''`) vai como `ReplyErrEmpty`.
    fn serialize_reply_err(req: u32, err: &Error) -> VSBuffer {
        let err_str = match rejected_value(err) {
            Some(value) if !value.is_truthy() => None,
            Some(value) => Some(safe_stringify(&value)),
            None => Some(safe_stringify(&error_for_serialization(err))),
        };
        let Some(err_str) = err_str else {
            return Self::serialize_reply_err_empty(req);
        };
        let err_buff = VSBuffer::from_string(&err_str);
        let len = MessageBuffer::size_long_string(&err_buff);
        let mut result = MessageBuffer::alloc(MESSAGE_REPLY_ERR_ERROR, req, len);
        result.write_long_string(&err_buff);
        result.buffer()
    }

    fn deserialize_reply_err_error(buff: &mut MessageBuffer) -> Dynamic {
        parse_json(&buff.read_long_string())
    }

    fn serialize_reply_err_empty(req: u32) -> VSBuffer {
        MessageBuffer::alloc(MESSAGE_REPLY_ERR_EMPTY, req, 0).buffer()
    }
}

/// `transformErrorForSerialization(error)` como valor JSON.
fn error_for_serialization(error: &Error) -> Dynamic {
    let serialized = errors::transform_error_for_serialization(error);
    fn to_dynamic(serialized: &errors::SerializedError) -> Dynamic {
        let mut entries: Vec<(&str, Dynamic)> = vec![("$isError", Dynamic::Bool(true)), ("name", Dynamic::string(&serialized.name)), ("message", Dynamic::string(&serialized.message))];
        if let Some(stack) = &serialized.stack {
            entries.push(("stack", Dynamic::string(stack)));
        }
        entries.push(("noTelemetry", Dynamic::Bool(serialized.no_telemetry)));
        if let Some(cause) = &serialized.cause {
            entries.push(("cause", to_dynamic(cause)));
        }
        if let Some(code) = &serialized.code {
            entries.push(("code", Dynamic::string(code)));
        }
        Dynamic::object(entries)
    }
    to_dynamic(&serialized)
}

// #endregion
