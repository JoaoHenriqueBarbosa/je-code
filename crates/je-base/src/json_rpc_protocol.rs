//! Porte de `src/vs/base/common/jsonRpcProtocol.ts`: o auxiliar genérico do protocolo JSON-RPC 2.0.
//!
//! Decisões de tradução:
//! - **Mensagens**: o original discrimina as mensagens pela forma (`'method' in message`, `hasKey(...)`),
//!   e aqui cada forma é uma variante de [`JsonRpcMessage`]. O campo `jsonrpc: '2.0'`, um literal de um
//!   valor só, não é guardado: [`JsonRpcMessage::to_value`] o escreve e [`JsonRpcMessage::from_value`] o
//!   exige. `from_value` aplica a mesma classificação estrutural de `isJsonRpcResponse`,
//!   `isJsonRpcRequest` e `isJsonRpcNotification` ao JSON recebido. `Omit<IJsonRpcNotification, 'jsonrpc'>`
//!   é o próprio [`IJsonRpcNotification`], e `Omit<IJsonRpcRequest, 'jsonrpc' | 'id'>` tem a mesma forma
//!   ([`JsonRpcRequestInit`]).
//! - **Valores `unknown`** (`params`, `result`, `data`) são [`Value`], o JSON do porte, e a ausência
//!   (`undefined`) é `None`. Por isso `sendRequest<T>` devolve `Promise<Option<Value>>`: o cast para `T` do
//!   original não tem equivalente.
//! - **Erros**: `JsonRpcError` viaja como [`Error`] de nome `JsonRpcError`, com `code` e `data` (em texto
//!   JSON) nas propriedades, ver [`JsonRpcError::into_error`] e [`JsonRpcError::from_error`], que são o
//!   `new JsonRpcError(...)` e o `instanceof JsonRpcError`. Um `throw` dentro de `handleRequest` é
//!   [`errors::throw`] (ou um `Promise` rejeitado). Como todo erro do porte é um [`Error`], o
//!   `error instanceof Error ? error.message : 'Internal error'` do original sempre usa a mensagem.
//! - **`handleMessage`** recebe `&[JsonRpcMessage]`: uma mensagem avulsa é um lote de um elemento, e o
//!   resultado e a ordem de resolução são os mesmos do ramo avulso do original.
//! - O protocolo é um [`Disposable`] que vive num `Rc` e as promessas de `handleMessage` guardam esse
//!   `Rc`, como a closure `this` do original. O `_store.isDisposed` é um `Cell` posto no `dispose`.
//! - `id` de requisição é um `number` no original; aqui, [`JsonRpcId::Number`] em `f64`, e o `Map`
//!   por id compara `"1"` e `1` como valores diferentes.

use std::cell::{Cell, RefCell};
use std::hash::{Hash, Hasher};
use std::rc::{Rc, Weak};

use crate::async_::{async_fn, async_fn_maybe};
use crate::cancellation::{CancellationToken, CancellationTokenSource};
use crate::errors::{self, Error};
use crate::json::{self, Object, Value};
use crate::js_string::JsString;
use crate::lifecycle::{Disposable, DisposableRef, IDisposable, to_disposable};
use crate::map::OrderedMap;
use crate::promise::{DeferredPromise, MaybePromise, Promise};

/// `JsonRpcId`: `string | number`.
#[derive(Clone, Debug, PartialEq)]
pub enum JsonRpcId {
    String(String),
    Number(f64),
}

/// A chave do `Map<JsonRpcId, ...>` (a igualdade `SameValueZero`: `NaN` é igual a `NaN`, `-0` a `0`).
#[derive(Clone, Debug, PartialEq, Eq)]
enum IdKey {
    String(String),
    Number(u64),
}

impl Hash for IdKey {
    fn hash<H: Hasher>(&self, state: &mut H) {
        match self {
            IdKey::String(s) => s.hash(state),
            IdKey::Number(n) => n.hash(state),
        }
        std::mem::discriminant(self).hash(state);
    }
}

impl JsonRpcId {
    fn key(&self) -> IdKey {
        match self {
            JsonRpcId::String(s) => IdKey::String(s.clone()),
            JsonRpcId::Number(n) if n.is_nan() => IdKey::Number(f64::NAN.to_bits()),
            JsonRpcId::Number(n) => IdKey::Number((n + 0.0).to_bits()),
        }
    }

    fn to_value(&self) -> Value {
        match self {
            JsonRpcId::String(s) => Value::String(s.as_str().into()),
            JsonRpcId::Number(n) => Value::Number(*n),
        }
    }

    fn from_value(value: &Value) -> Option<Self> {
        match value {
            Value::String(s) => Some(JsonRpcId::String(s.to_string_lossy())),
            Value::Number(n) => Some(JsonRpcId::Number(*n)),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct IJsonRpcError {
    pub code: f64,
    pub message: String,
    pub data: Option<Value>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct IJsonRpcRequest {
    pub id: JsonRpcId,
    pub method: String,
    pub params: Option<Value>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct IJsonRpcNotification {
    pub method: String,
    pub params: Option<Value>,
}

/// `Omit<IJsonRpcRequest, 'jsonrpc' | 'id'>`: o que `sendRequest` recebe.
pub type JsonRpcRequestInit = IJsonRpcNotification;

#[derive(Clone, Debug, PartialEq)]
pub struct IJsonRpcSuccessResponse {
    pub id: JsonRpcId,
    pub result: Option<Value>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct IJsonRpcErrorResponse {
    pub id: Option<JsonRpcId>,
    pub error: IJsonRpcError,
}

/// `JsonRpcResponse`: `IJsonRpcSuccessResponse | IJsonRpcErrorResponse`.
#[derive(Clone, Debug, PartialEq)]
pub enum JsonRpcResponse {
    Success(IJsonRpcSuccessResponse),
    Error(IJsonRpcErrorResponse),
}

/// `JsonRpcMessage`.
#[derive(Clone, Debug, PartialEq)]
pub enum JsonRpcMessage {
    Request(IJsonRpcRequest),
    Notification(IJsonRpcNotification),
    Response(JsonRpcResponse),
}

impl From<JsonRpcResponse> for JsonRpcMessage {
    fn from(response: JsonRpcResponse) -> Self {
        JsonRpcMessage::Response(response)
    }
}

fn object(entries: Vec<(&str, Value)>) -> Value {
    Value::Object(entries.into_iter().collect::<Object>())
}

fn with_optional(mut entries: Vec<(&'static str, Value)>, name: &'static str, value: &Option<Value>) -> Vec<(&'static str, Value)> {
    if let Some(value) = value {
        entries.push((name, value.clone()));
    }
    entries
}

impl JsonRpcMessage {
    /// O objeto JSON da mensagem, com `jsonrpc: '2.0'`. O que é `undefined` no original não aparece.
    pub fn to_value(&self) -> Value {
        let version = ("jsonrpc", Value::from("2.0"));
        match self {
            JsonRpcMessage::Request(r) => {
                object(with_optional(vec![version, ("id", r.id.to_value()), ("method", Value::from(r.method.as_str()))], "params", &r.params))
            }
            JsonRpcMessage::Notification(n) => object(with_optional(vec![version, ("method", Value::from(n.method.as_str()))], "params", &n.params)),
            JsonRpcMessage::Response(JsonRpcResponse::Success(s)) => object(with_optional(vec![version, ("id", s.id.to_value())], "result", &s.result)),
            JsonRpcMessage::Response(JsonRpcResponse::Error(e)) => {
                let error = object(with_optional(vec![("code", Value::Number(e.error.code)), ("message", Value::from(e.error.message.as_str()))], "data", &e.error.data));
                let mut entries = vec![version];
                if let Some(id) = &e.id {
                    entries.push(("id", id.to_value()));
                }
                entries.push(("error", error));
                object(entries)
            }
        }
    }

    /// Classifica um objeto JSON recebido como `isJsonRpcResponse`, `isJsonRpcRequest` e
    /// `isJsonRpcNotification`, nessa ordem. `None` se não tem a forma de nenhuma (ou se não é `2.0`, ou
    /// se o `id` não é texto nem número).
    pub fn from_value(value: &Value) -> Option<Self> {
        let Value::Object(object) = value else {
            return None;
        };
        if object.get("jsonrpc") != Some(&Value::from("2.0")) {
            return None;
        }
        let method = || match object.get("method") {
            Some(Value::String(method)) => Some(method.to_string_lossy()),
            _ => None,
        };
        let id = object.get("id");
        if let Some(id) = id {
            let id = JsonRpcId::from_value(id)?;
            if let Some(result) = object.get("result") {
                return Some(JsonRpcMessage::Response(JsonRpcResponse::Success(IJsonRpcSuccessResponse { id, result: Some(result.clone()) })));
            }
            if let Some(error) = object.get("error") {
                return Some(JsonRpcMessage::Response(JsonRpcResponse::Error(IJsonRpcErrorResponse { id: Some(id), error: error_from_value(error)? })));
            }
            return Some(JsonRpcMessage::Request(IJsonRpcRequest { id, method: method()?, params: object.get("params").cloned() }));
        }
        if let Some(error) = object.get("error") {
            return Some(JsonRpcMessage::Response(JsonRpcResponse::Error(IJsonRpcErrorResponse { id: None, error: error_from_value(error)? })));
        }
        Some(JsonRpcMessage::Notification(IJsonRpcNotification { method: method()?, params: object.get("params").cloned() }))
    }
}

fn error_from_value(value: &Value) -> Option<IJsonRpcError> {
    let Value::Object(error) = value else {
        return None;
    };
    let (Some(Value::Number(code)), Some(Value::String(message))) = (error.get("code"), error.get("message")) else {
        return None;
    };
    Some(IJsonRpcError { code: *code, message: message.to_string_lossy(), data: error.get("data").cloned() })
}

struct PendingRequest {
    promise: DeferredPromise<Option<Value>>,
    cts: CancellationTokenSource,
}

/// `handleRequest(request, token)`: devolve `Promise<unknown> | unknown`; um `throw` é [`errors::throw`].
pub type HandleRequest = Rc<dyn Fn(&IJsonRpcRequest, CancellationToken) -> MaybePromise<Option<Value>>>;
/// `handleNotification(notification)`.
pub type HandleNotification = Rc<dyn Fn(&IJsonRpcNotification)>;
/// O callback `_send` do protocolo.
pub type SendMessage = Rc<dyn Fn(&JsonRpcMessage)>;
/// O `onCancel` de `sendRequest`.
pub type OnCancel = Rc<dyn Fn(&JsonRpcId)>;

#[derive(Clone, Default)]
pub struct IJsonRpcProtocolHandlers {
    pub handle_request: Option<HandleRequest>,
    pub handle_notification: Option<HandleNotification>,
}

/// `JsonRpcError`: o erro que leva o `code` e o `data` de um erro JSON-RPC.
#[derive(Clone, Debug, PartialEq)]
pub struct JsonRpcError {
    pub code: f64,
    pub message: String,
    pub data: Option<Value>,
}

const JSON_RPC_ERROR_NAME: &str = "JsonRpcError";

impl JsonRpcError {
    pub fn new(code: f64, message: impl Into<String>, data: Option<Value>) -> Self {
        Self { code, message: message.into(), data }
    }

    /// O [`Error`] que carrega este erro, para [`errors::throw`] e para rejeitar uma promessa.
    pub fn into_error(self) -> Error {
        let error = Error::named(JSON_RPC_ERROR_NAME, self.message).with_prop("code", self.code);
        match self.data {
            Some(data) => error.with_prop("data", value_to_json_text(&data)),
            None => error,
        }
    }

    /// `error instanceof JsonRpcError`: reconhece o erro de [`JsonRpcError::into_error`].
    pub fn from_error(error: &Error) -> Option<Self> {
        if error.name != JSON_RPC_ERROR_NAME {
            return None;
        }
        let data = error.prop("data").and_then(|text| json::parse_str(text, &mut Vec::new()));
        Some(Self { code: error.prop("code")?.parse().ok()?, message: error.message.clone(), data })
    }
}

/// `JSON.stringify(value)` de um valor JSON (só para guardar o `data` de um [`JsonRpcError`] num [`Error`]).
fn value_to_json_text(value: &Value) -> String {
    fn string(out: &mut String, text: &JsString) {
        out.push('"');
        for unit in char::decode_utf16(text.units().iter().copied()) {
            match unit {
                Ok('"') => out.push_str("\\\""),
                Ok('\\') => out.push_str("\\\\"),
                Ok('\n') => out.push_str("\\n"),
                Ok('\r') => out.push_str("\\r"),
                Ok('\t') => out.push_str("\\t"),
                Ok(c) if c < ' ' => out.push_str(&format!("\\u{:04x}", u32::from(c))),
                Ok(c) => out.push(c),
                Err(lone) => out.push_str(&format!("\\u{:04x}", lone.unpaired_surrogate())),
            }
        }
        out.push('"');
    }
    fn write(out: &mut String, value: &Value) {
        match value {
            Value::Null => out.push_str("null"),
            Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
            // `JSON.stringify` escreve `null` para `NaN` e infinito
            Value::Number(n) if !n.is_finite() => out.push_str("null"),
            Value::Number(n) => out.push_str(&n.to_string()),
            Value::String(s) => string(out, s),
            Value::Array(items) => {
                out.push('[');
                for (i, item) in items.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    write(out, item);
                }
                out.push(']');
            }
            Value::Object(object) => {
                out.push('{');
                for (i, (key, item)) in object.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    string(out, key);
                    out.push(':');
                    write(out, item);
                }
                out.push('}');
            }
        }
    }
    let mut out = String::new();
    write(&mut out, value);
    out
}

const PARSE_ERROR: f64 = -32700.0;
const METHOD_NOT_FOUND: f64 = -32601.0;
const INTERNAL_ERROR: f64 = -32603.0;

/// Generic JSON-RPC 2.0 protocol helper.
pub struct JsonRpcProtocol {
    base: Rc<Disposable>,
    me: Weak<JsonRpcProtocol>,
    disposed: Cell<bool>,

    next_request_id: Cell<f64>,
    pending_requests: RefCell<OrderedMap<IdKey, Rc<PendingRequest>>>,
    send: SendMessage,
    handlers: IJsonRpcProtocolHandlers,
}

impl JsonRpcProtocol {
    pub fn new(send: impl Fn(&JsonRpcMessage) + 'static, handlers: IJsonRpcProtocolHandlers) -> Rc<Self> {
        let base = Disposable::new();
        Rc::new_cyclic(|me| Self {
            base,
            me: me.clone(),
            disposed: Cell::new(false),
            next_request_id: Cell::new(1.0),
            pending_requests: RefCell::new(OrderedMap::new()),
            send: Rc::new(send),
            handlers,
        })
    }

    fn this(&self) -> Rc<Self> {
        self.me.upgrade().expect("o protocolo está vivo enquanto um método dele roda")
    }

    pub fn send_notification(&self, notification: IJsonRpcNotification) {
        (self.send)(&JsonRpcMessage::Notification(notification));
    }

    /// `sendRequest(request, token = CancellationToken.None, onCancel?)`.
    pub fn send_request(&self, request: JsonRpcRequestInit, token: &CancellationToken, on_cancel: Option<OnCancel>) -> Promise<Option<Value>> {
        if self.disposed.get() {
            return Promise::rejected(Error::cancellation());
        }

        let id = JsonRpcId::Number(self.next_request_id.get());
        self.next_request_id.set(self.next_request_id.get() + 1.0);
        let cts = CancellationTokenSource::new(None);
        let pending = Rc::new(PendingRequest { promise: DeferredPromise::new(), cts: cts.clone() });
        self.pending_requests.borrow_mut().insert(id.key(), pending.clone());

        let cancel_listener: Rc<RefCell<Option<DisposableRef>>> = Rc::default();
        let listener = token.on_cancellation_requested().listen({
            let (me, pending, id, cancel_listener) = (self.me.clone(), pending.clone(), id.clone(), cancel_listener.clone());
            move |_| {
                if !pending.promise.is_settled() {
                    if let Some(this) = me.upgrade() {
                        this.pending_requests.borrow_mut().remove(&id.key());
                    }
                    pending.cts.cancel();
                    if let Some(on_cancel) = &on_cancel {
                        on_cancel(&id);
                    }
                    pending.promise.cancel();
                }
                dispose_slot(&cancel_listener);
            }
        });
        *cancel_listener.borrow_mut() = Some(listener);

        (self.send)(&JsonRpcMessage::Request(IJsonRpcRequest { id: id.clone(), method: request.method, params: request.params }));

        let me = self.me.clone();
        pending.promise.promise.finally(move || {
            dispose_slot(&cancel_listener);
            if let Some(this) = me.upgrade() {
                this.pending_requests.borrow_mut().remove(&id.key());
            }
            cts.dispose_with(true);
        })
    }

    /// Handles one or more incoming JSON-RPC messages.
    ///
    /// Returns an array of JSON-RPC response objects generated for any incoming
    /// requests in the message(s). Notifications and responses to our own
    /// outgoing requests do not produce return values. For batch inputs, the
    /// returned responses are in the same order as the corresponding requests.
    ///
    /// Note: responses are also emitted via the `_send` callback, so callers
    /// that rely on the return value should not re-send them.
    pub fn handle_message(&self, messages: &[JsonRpcMessage]) -> Promise<Vec<JsonRpcResponse>> {
        let this = self.this();
        let messages = messages.to_vec();
        async_fn(async move {
            let mut replies = Vec::new();
            for single in messages {
                if let Some(reply) = this.handle_single_message(single).await? {
                    replies.push(reply);
                }
            }
            Ok(replies)
        })
    }

    pub fn cancel_pending_request(&self, id: &JsonRpcId) {
        let request = self.pending_requests.borrow_mut().remove(&id.key());
        if let Some(request) = request {
            request.cts.cancel();
            request.promise.cancel();
            request.cts.dispose_with(true);
        }
    }

    pub fn cancel_all_requests(&self) {
        let ids: Vec<IdKey> = self.pending_requests.borrow().keys().cloned().collect();
        for id in ids {
            let pending = self.pending_requests.borrow_mut().remove(&id);
            if let Some(pending) = pending {
                pending.cts.cancel();
                pending.promise.cancel();
                pending.cts.dispose_with(true);
            }
        }
    }

    /// `_handleMessage(message)`.
    fn handle_single_message(&self, message: JsonRpcMessage) -> Promise<Option<JsonRpcResponse>> {
        let this = self.this();
        async_fn_maybe(async move {
            match message {
                JsonRpcMessage::Response(JsonRpcResponse::Success(response)) => this.handle_result(&response),
                JsonRpcMessage::Response(JsonRpcResponse::Error(response)) => this.handle_error(&response),
                JsonRpcMessage::Request(request) => return Ok(MaybePromise::Promise(this.handle_request(request))),
                JsonRpcMessage::Notification(notification) => {
                    if let Some(handle_notification) = &this.handlers.handle_notification {
                        handle_notification(&notification);
                    }
                }
            }
            Ok(MaybePromise::Value(None))
        })
    }

    fn handle_result(&self, response: &IJsonRpcSuccessResponse) {
        let request = self.pending_requests.borrow_mut().remove(&response.id.key());
        if let Some(request) = request {
            request.promise.complete(response.result.clone());
            request.cts.dispose_with(true);
        }
    }

    fn handle_error(&self, response: &IJsonRpcErrorResponse) {
        let Some(id) = &response.id else {
            return;
        };

        let request = self.pending_requests.borrow_mut().remove(&id.key());
        if let Some(request) = request {
            let error = JsonRpcError::new(response.error.code, response.error.message.clone(), response.error.data.clone());
            request.promise.error(error.into_error());
            request.cts.dispose_with(true);
        }
    }

    /// `_handleRequest(request)`: nunca rejeita por erro do handler, que vira uma resposta de erro.
    fn handle_request(&self, request: IJsonRpcRequest) -> Promise<Option<JsonRpcResponse>> {
        let this = self.this();
        async_fn(async move {
            let Some(handle_request) = this.handlers.handle_request.clone() else {
                let response = JsonRpcResponse::Error(IJsonRpcErrorResponse {
                    id: Some(request.id.clone()),
                    error: IJsonRpcError { code: METHOD_NOT_FOUND, message: format!("Method not found: {}", request.method), data: None },
                });
                (this.send)(&JsonRpcMessage::Response(response.clone()));
                return Ok(Some(response));
            };

            let cts = CancellationTokenSource::new(None);
            this.base.register(to_disposable({
                let cts = cts.clone();
                move || cts.dispose_with(true)
            }));

            let outcome: Result<JsonRpcResponse, Error> = async {
                let result_or_thenable = errors::catch(|| handle_request(&request, cts.token()))?;
                let result = match result_or_thenable {
                    MaybePromise::Promise(promise) => promise.await?,
                    MaybePromise::Value(value) => value,
                };
                let response = JsonRpcResponse::Success(IJsonRpcSuccessResponse { id: request.id.clone(), result });
                errors::catch(|| (this.send)(&JsonRpcMessage::Response(response.clone())))?;
                Ok(response)
            }
            .await;

            let sent = match outcome {
                Ok(response) => Ok(response),
                Err(error) => {
                    let error = match JsonRpcError::from_error(&error) {
                        Some(json_rpc_error) => IJsonRpcError { code: json_rpc_error.code, message: json_rpc_error.message, data: json_rpc_error.data },
                        None => IJsonRpcError { code: INTERNAL_ERROR, message: error.message, data: None },
                    };
                    let response = JsonRpcResponse::Error(IJsonRpcErrorResponse { id: Some(request.id.clone()), error });
                    errors::catch(|| (this.send)(&JsonRpcMessage::Response(response.clone()))).map(|()| response)
                }
            };
            cts.dispose_with(true);
            sent.map(Some)
        })
    }

    pub fn create_parse_error(message: impl Into<String>, data: Option<Value>) -> IJsonRpcErrorResponse {
        IJsonRpcErrorResponse { id: None, error: IJsonRpcError { code: PARSE_ERROR, message: message.into(), data } }
    }
}

fn dispose_slot(slot: &RefCell<Option<DisposableRef>>) {
    let taken = slot.borrow_mut().take();
    if let Some(taken) = taken {
        taken.dispose();
    }
}

impl IDisposable for JsonRpcProtocol {
    fn dispose(&self) {
        self.cancel_all_requests();
        self.disposed.set(true);
        self.base.dispose();
    }

    fn constructor_name(&self) -> &'static str {
        "JsonRpcProtocol"
    }
}

pub fn is_json_rpc_request(message: &JsonRpcMessage) -> bool {
    matches!(message, JsonRpcMessage::Request(_))
}

pub fn is_json_rpc_response(message: &JsonRpcMessage) -> bool {
    matches!(message, JsonRpcMessage::Response(JsonRpcResponse::Success(_) | JsonRpcResponse::Error(IJsonRpcErrorResponse { id: Some(_), .. })))
}

pub fn is_json_rpc_notification(message: &JsonRpcMessage) -> bool {
    matches!(message, JsonRpcMessage::Notification(_))
}
