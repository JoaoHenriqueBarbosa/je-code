//! Porte de `src/vs/base/test/common/jsonRpcProtocol.test.ts` (suíte `JsonRpcProtocol`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - Os testes `async` rodam em `common::run_with_faked_timers`, e o `await` é o das `Promise` do porte.
//! - `assert.deepStrictEqual(sentMessages, [...])` compara [`JsonRpcMessage`], e o objeto literal com
//!   `jsonrpc: '2.0'` é a variante correspondente (o `jsonrpc` é implícito, ver `json_rpc_protocol`).
//! - `assert.rejects(promise, error => error instanceof JsonRpcError)` é `JsonRpcError::from_error`, e
//!   `instanceof CancellationError` é `is_cancellation_error`.
//! - `handleRequest: async (...) => ...` é um fecho que devolve `MaybePromise::Promise(async_fn(...))`, e o
//!   `throw` de um handler síncrono é `errors::throw`.
//! - `createProtocol` guarda as mensagens enviadas num `Rc<RefCell<Vec<_>>>` e registra o protocolo no
//!   guarda de vazamento (`store.add`).

mod common;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use common::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite, run_with_faked_timers};
use je_base::async_::async_fn;
use je_base::cancellation::{CancellationToken, CancellationTokenSource};
use je_base::errors::{self, Error, is_cancellation_error};
use je_base::json::{Object, Value};
use je_base::json_rpc_protocol::{
    HandleNotification, HandleRequest, IJsonRpcError, IJsonRpcErrorResponse, IJsonRpcNotification, IJsonRpcProtocolHandlers, IJsonRpcRequest, IJsonRpcSuccessResponse,
    JsonRpcError, JsonRpcId, JsonRpcMessage, JsonRpcProtocol, JsonRpcRequestInit, JsonRpcResponse, OnCancel,
};
use je_base::promise::{DeferredPromise, MaybePromise};

type Sent = Rc<RefCell<Vec<JsonRpcMessage>>>;

fn create_protocol(store: &LeakGuard, handlers: IJsonRpcProtocolHandlers) -> (Rc<JsonRpcProtocol>, Sent) {
    let sent_messages: Sent = Rc::default();
    let sink = sent_messages.clone();
    let protocol = JsonRpcProtocol::new(move |message| sink.borrow_mut().push(message.clone()), handlers);
    store.add_rc(protocol.clone());
    (protocol, sent_messages)
}

fn object(entries: &[(&str, Value)]) -> Value {
    Value::Object(entries.iter().map(|(k, v)| (*k, v.clone())).collect::<Object>())
}

fn text(value: &str) -> Value {
    Value::from(value)
}

fn init(method: &str, params: Option<Value>) -> JsonRpcRequestInit {
    IJsonRpcNotification { method: method.to_owned(), params }
}

fn outgoing_id(sent_messages: &Sent) -> JsonRpcId {
    match &sent_messages.borrow()[0] {
        JsonRpcMessage::Request(IJsonRpcRequest { id, .. }) => id.clone(),
        other => panic!("esperava uma requisição, veio {other:?}"),
    }
}

fn success(id: JsonRpcId, result: Value) -> JsonRpcResponse {
    JsonRpcResponse::Success(IJsonRpcSuccessResponse { id, result: Some(result) })
}

fn failure(id: JsonRpcId, code: f64, message: &str, data: Option<Value>) -> JsonRpcResponse {
    JsonRpcResponse::Error(IJsonRpcErrorResponse { id: Some(id), error: IJsonRpcError { code, message: message.to_owned(), data } })
}

fn request_handler(handler: impl Fn(&IJsonRpcRequest, CancellationToken) -> MaybePromise<Option<Value>> + 'static) -> IJsonRpcProtocolHandlers {
    IJsonRpcProtocolHandlers { handle_request: Some(Rc::new(handler) as HandleRequest), handle_notification: None }
}

#[test]
fn send_notification_adds_jsonrpc_envelope() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let (protocol, sent_messages) = create_protocol(&store, IJsonRpcProtocolHandlers::default());

    protocol.send_notification(init("notify", Some(object(&[("value", Value::Number(1.0))]))));

    let notification = JsonRpcMessage::Notification(init("notify", Some(object(&[("value", Value::Number(1.0))]))));
    assert_eq!(*sent_messages.borrow(), vec![notification.clone()]);
    assert_eq!(
        notification.to_value(),
        object(&[("jsonrpc", text("2.0")), ("method", text("notify")), ("params", object(&[("value", Value::Number(1.0))]))])
    );
}

#[test]
fn send_request_resolves_on_success_response() {
    let store = Rc::new(ensure_no_disposables_are_leaked_in_test_suite());
    run_with_faked_timers(async move {
        let (protocol, sent_messages) = create_protocol(&store, IJsonRpcProtocolHandlers::default());

        let request_promise = protocol.send_request(init("echo", Some(object(&[("value", text("ok"))]))), &CancellationToken::None, None);
        let id = outgoing_id(&sent_messages);

        let replies = protocol.handle_message(&[JsonRpcMessage::Response(success(id, text("done")))]).await.unwrap();

        let result = request_promise.await.unwrap();
        assert_eq!(result, Some(text("done")));
        assert_eq!(replies, vec![]);
    });
}

#[test]
fn send_request_rejects_on_error_response() {
    let store = Rc::new(ensure_no_disposables_are_leaked_in_test_suite());
    run_with_faked_timers(async move {
        let (protocol, sent_messages) = create_protocol(&store, IJsonRpcProtocolHandlers::default());

        let request_promise = protocol.send_request(init("fail", None), &CancellationToken::None, None);
        let id = outgoing_id(&sent_messages);

        protocol
            .handle_message(&[JsonRpcMessage::Response(failure(id, 123.0, "Failure", Some(object(&[("source", text("test"))]))))])
            .await
            .unwrap();

        let error = request_promise.await.unwrap_err();
        let error = JsonRpcError::from_error(&error).expect("o erro é um JsonRpcError");
        assert_eq!(error.code, 123.0);
        assert_eq!(error.message, "Failure");
        assert_eq!(error.data, Some(object(&[("source", text("test"))])));
    });
}

#[test]
fn send_request_honors_cancellation_token_and_invokes_on_cancel() {
    let store = Rc::new(ensure_no_disposables_are_leaked_in_test_suite());
    run_with_faked_timers(async move {
        let (protocol, sent_messages) = create_protocol(&store, IJsonRpcProtocolHandlers::default());
        let cts = CancellationTokenSource::new(None);
        let canceled_id: Rc<RefCell<Option<JsonRpcId>>> = Rc::default();

        let sink = canceled_id.clone();
        let on_cancel: OnCancel = Rc::new(move |id| *sink.borrow_mut() = Some(id.clone()));
        let request_promise = protocol.send_request(init("cancel-me", None), &cts.token(), Some(on_cancel));
        let id = outgoing_id(&sent_messages);

        cts.cancel();

        assert!(is_cancellation_error(&request_promise.await.unwrap_err()));
        assert_eq!(*canceled_id.borrow(), Some(id));

        cts.dispose_with(true);
    });
}

#[test]
fn cancel_pending_request_rejects_active_request() {
    let store = Rc::new(ensure_no_disposables_are_leaked_in_test_suite());
    run_with_faked_timers(async move {
        let (protocol, sent_messages) = create_protocol(&store, IJsonRpcProtocolHandlers::default());

        let request_promise = protocol.send_request(init("pending", None), &CancellationToken::None, None);
        let id = outgoing_id(&sent_messages);
        protocol.cancel_pending_request(&id);

        assert!(is_cancellation_error(&request_promise.await.unwrap_err()));
    });
}

#[test]
fn handle_request_responds_with_method_not_found_without_handler() {
    let store = Rc::new(ensure_no_disposables_are_leaked_in_test_suite());
    run_with_faked_timers(async move {
        let (protocol, sent_messages) = create_protocol(&store, IJsonRpcProtocolHandlers::default());

        let replies = protocol
            .handle_message(&[JsonRpcMessage::Request(IJsonRpcRequest { id: JsonRpcId::Number(7.0), method: "unknown".to_owned(), params: None })])
            .await
            .unwrap();

        let expected = vec![failure(JsonRpcId::Number(7.0), -32601.0, "Method not found: unknown", None)];
        assert_eq!(*sent_messages.borrow(), expected.iter().cloned().map(JsonRpcMessage::from).collect::<Vec<_>>());
        assert_eq!(replies, expected);
    });
}

#[test]
fn handle_request_responds_with_result_and_passes_cancellation_token() {
    let store = Rc::new(ensure_no_disposables_are_leaked_in_test_suite());
    run_with_faked_timers(async move {
        let received_token: Rc<RefCell<Option<CancellationToken>>> = Rc::default();
        let was_canceled_during_handler: Rc<Cell<Option<bool>>> = Rc::default();
        let (token_sink, canceled_sink) = (received_token.clone(), was_canceled_during_handler.clone());
        let (protocol, sent_messages) = create_protocol(
            &store,
            request_handler(move |request, token| {
                *token_sink.borrow_mut() = Some(token.clone());
                canceled_sink.set(Some(token.is_cancellation_requested()));
                let result = text(&format!("{}:ok", request.method));
                MaybePromise::Promise(async_fn(async move { Ok(Some(result)) }))
            }),
        );

        let replies = protocol
            .handle_message(&[JsonRpcMessage::Request(IJsonRpcRequest { id: JsonRpcId::Number(9.0), method: "compute".to_owned(), params: None })])
            .await
            .unwrap();

        assert!(received_token.borrow().is_some());
        assert_eq!(was_canceled_during_handler.get(), Some(false));
        let expected = vec![success(JsonRpcId::Number(9.0), text("compute:ok"))];
        assert_eq!(*sent_messages.borrow(), expected.iter().cloned().map(JsonRpcMessage::from).collect::<Vec<_>>());
        assert_eq!(replies, expected);
    });
}

#[test]
fn handle_request_serializes_json_rpc_error_and_returns_it() {
    let store = Rc::new(ensure_no_disposables_are_leaked_in_test_suite());
    run_with_faked_timers(async move {
        let (protocol, sent_messages) = create_protocol(
            &store,
            request_handler(|_, _| errors::throw(JsonRpcError::new(88.0, "bad request", Some(object(&[("detail", Value::Bool(true))]))).into_error())),
        );

        let replies = protocol
            .handle_message(&[JsonRpcMessage::Request(IJsonRpcRequest { id: JsonRpcId::String("a".to_owned()), method: "boom".to_owned(), params: None })])
            .await
            .unwrap();

        let expected = vec![failure(JsonRpcId::String("a".to_owned()), 88.0, "bad request", Some(object(&[("detail", Value::Bool(true))])))];
        assert_eq!(*sent_messages.borrow(), expected.iter().cloned().map(JsonRpcMessage::from).collect::<Vec<_>>());
        assert_eq!(replies, expected);
    });
}

#[test]
fn handle_request_maps_unknown_errors_to_internal_error_and_returns_it() {
    let store = Rc::new(ensure_no_disposables_are_leaked_in_test_suite());
    run_with_faked_timers(async move {
        let (protocol, sent_messages) = create_protocol(&store, request_handler(|_, _| errors::throw(Error::new("unexpected"))));

        let replies = protocol
            .handle_message(&[JsonRpcMessage::Request(IJsonRpcRequest { id: JsonRpcId::String("b".to_owned()), method: "explode".to_owned(), params: None })])
            .await
            .unwrap();

        let expected = vec![failure(JsonRpcId::String("b".to_owned()), -32603.0, "unexpected", None)];
        assert_eq!(*sent_messages.borrow(), expected.iter().cloned().map(JsonRpcMessage::from).collect::<Vec<_>>());
        assert_eq!(replies, expected);
    });
}

#[test]
fn handle_message_processes_batch_sequentially() {
    let store = Rc::new(ensure_no_disposables_are_leaked_in_test_suite());
    run_with_faked_timers(async move {
        let sequence: Rc<RefCell<Vec<&'static str>>> = Rc::default();
        let gate = Rc::new(DeferredPromise::<()>::new());
        let (request_sequence, request_gate, notification_sequence) = (sequence.clone(), gate.clone(), sequence.clone());
        let handlers = IJsonRpcProtocolHandlers {
            handle_request: Some(Rc::new(move |_: &IJsonRpcRequest, _| {
                let (sequence, gate) = (request_sequence.clone(), request_gate.clone());
                MaybePromise::Promise(async_fn(async move {
                    sequence.borrow_mut().push("request:start");
                    gate.promise.clone().await?;
                    sequence.borrow_mut().push("request:end");
                    Ok(Some(Value::Bool(true)))
                }))
            }) as HandleRequest),
            handle_notification: Some(Rc::new(move |_: &IJsonRpcNotification| notification_sequence.borrow_mut().push("notification")) as HandleNotification),
        };
        let (protocol, _) = create_protocol(&store, handlers);

        let request = IJsonRpcRequest { id: JsonRpcId::Number(1.0), method: "first".to_owned(), params: None };
        let notification = IJsonRpcNotification { method: "second".to_owned(), params: None };

        let handling_promise = protocol.handle_message(&[JsonRpcMessage::Request(request), JsonRpcMessage::Notification(notification)]);
        assert_eq!(*sequence.borrow(), vec!["request:start"]);

        gate.complete(());
        let replies = handling_promise.await.unwrap();

        assert_eq!(*sequence.borrow(), vec!["request:start", "request:end", "notification"]);
        assert_eq!(replies, vec![success(JsonRpcId::Number(1.0), Value::Bool(true))]);
    });
}
