//! Testes do `jsonRpcProtocol.ts` que o `.test.ts` do VS Code não cobre: a classificação estrutural das
//! mensagens (`isJsonRpc*`), o `createParseError`, o `dispose` e as notificações recebidas.

mod common;

use std::cell::RefCell;
use std::rc::Rc;

use common::{ensure_no_disposables_are_leaked_in_test_suite, run_with_faked_timers};
use je_base::cancellation::CancellationToken;
use je_base::errors::is_cancellation_error;
use je_base::json::{self, Value};
use je_base::json_rpc_protocol::{
    HandleNotification, IJsonRpcNotification, IJsonRpcProtocolHandlers, JsonRpcMessage, JsonRpcProtocol, JsonRpcRequestInit, is_json_rpc_notification, is_json_rpc_request,
    is_json_rpc_response,
};

fn parse(text: &str) -> Value {
    json::parse_str(text, &mut Vec::new()).expect("JSON válido")
}

fn message(text: &str) -> Option<JsonRpcMessage> {
    JsonRpcMessage::from_value(&parse(text))
}

#[test]
fn from_value_classifies_like_the_is_json_rpc_functions() {
    let request = message(r#"{"jsonrpc":"2.0","id":"a","method":"m","params":[1]}"#).unwrap();
    let notification = message(r#"{"jsonrpc":"2.0","method":"m"}"#).unwrap();
    let result = message(r#"{"jsonrpc":"2.0","id":1,"result":null}"#).unwrap();
    let error = message(r#"{"jsonrpc":"2.0","id":1,"error":{"code":-1,"message":"x"}}"#).unwrap();
    let error_without_id = message(r#"{"jsonrpc":"2.0","error":{"code":-32700,"message":"parse"}}"#).unwrap();

    assert!(is_json_rpc_request(&request) && !is_json_rpc_response(&request) && !is_json_rpc_notification(&request));
    assert!(is_json_rpc_notification(&notification) && !is_json_rpc_request(&notification));
    assert!(is_json_rpc_response(&result) && is_json_rpc_response(&error));
    // `isJsonRpcResponse` exige `id` junto de `result` ou `error`
    assert!(!is_json_rpc_response(&error_without_id));
    assert_eq!(message(r#"{"jsonrpc":"2.0","method":"m","id":true}"#), None);
    assert_eq!(message(r#"{"jsonrpc":"1.0","method":"m"}"#), None);
    assert_eq!(message("[1]"), None);
}

#[test]
fn to_value_round_trips_through_from_value() {
    for text in [
        r#"{"jsonrpc":"2.0","id":3,"method":"m","params":{"a":1}}"#,
        r#"{"jsonrpc":"2.0","method":"n"}"#,
        r#"{"jsonrpc":"2.0","id":"s","result":[true]}"#,
        r#"{"jsonrpc":"2.0","id":2,"error":{"code":5,"message":"m","data":"d"}}"#,
        r#"{"jsonrpc":"2.0","error":{"code":-32700,"message":"parse"}}"#,
    ] {
        let message = message(text).unwrap();
        assert_eq!(message.to_value(), parse(text));
    }
}

#[test]
fn create_parse_error_has_the_parse_error_code_and_no_id() {
    let error = JsonRpcProtocol::create_parse_error("bad json", Some(Value::from("x")));

    assert_eq!((error.id, error.error.code, error.error.message.as_str(), error.error.data), (None, -32700.0, "bad json", Some(Value::from("x"))));
}

#[test]
fn notifications_reach_the_handler_and_produce_no_reply() {
    let store = Rc::new(ensure_no_disposables_are_leaked_in_test_suite());
    run_with_faked_timers(async move {
        let received: Rc<RefCell<Vec<String>>> = Rc::default();
        let sink = received.clone();
        let handlers = IJsonRpcProtocolHandlers {
            handle_request: None,
            handle_notification: Some(Rc::new(move |n: &IJsonRpcNotification| sink.borrow_mut().push(n.method.clone())) as HandleNotification),
        };
        let protocol = JsonRpcProtocol::new(|_| {}, handlers);
        store.add_rc(protocol.clone());

        let replies = protocol.handle_message(&[JsonRpcMessage::Notification(IJsonRpcNotification { method: "ping".into(), params: None })]).await.unwrap();

        assert_eq!((replies.len(), received.borrow().clone()), (0, vec!["ping".to_owned()]));
    });
}

#[test]
fn dispose_rejects_pending_requests_and_new_requests() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    run_with_faked_timers(async move {
        let protocol = JsonRpcProtocol::new(|_| {}, IJsonRpcProtocolHandlers::default());
        let request = JsonRpcRequestInit { method: "m".into(), params: None };
        let pending = protocol.send_request(request.clone(), &CancellationToken::None, None);

        je_base::lifecycle::IDisposable::dispose(&*protocol);
        let after = protocol.send_request(request, &CancellationToken::None, None);

        assert!(is_cancellation_error(&pending.await.unwrap_err()));
        assert!(is_cancellation_error(&after.await.unwrap_err()));
    });
}
