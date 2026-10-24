//! Porte de `src/vs/workbench/services/extensions/test/common/rpcProtocol.test.ts` (suíte `RPCProtocol`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` e o `DisposableStore` do `setup`/`teardown` viram o guarda do começo de cada caso,
//!   e os casos rodam em `run_with_faked_timers` (o `done` do mocha é o fim do corpo assíncrono).
//! - `bProxy.$m(a1, a2)` é `RpcProxy::call("$m", [a1, a2])` e o `BClass` é um [`IRpcActor`] que repassa os dois argumentos ao
//!   `delegate`, como `$m(a1, a2)` repassa a `delegate.call(null, a1, a2)`. O `delegate` devolve `MaybePromise` (um valor ou uma promessa) e
//!   lança com `errors::throw`.
//! - `VSBuffer` é `vs_buffer_to_dynamic`, `SerializableObjectWithBuffers` é `serializable_object_with_buffers`, e o `CancellationToken` que vai
//!   como argumento é `cancellation_token_to_dynamic`.
//! - O que o JavaScript rejeita ou lança sem ser um `Error` (`undefined`, `{ what: 'what' }`) é `rejection_with_value`, e `err === undefined`
//!   e `err.what` são lidos com `rejected_value`.
//! - `Reflect.get(bProtocol, '_cancelInvokedHandlers')` com `Object.keys` é `RpcProtocol::cancel_invoked_handler_ids`.
//! - `assert.throws(() => bProxy.$m(badObject, '2'))` é `errors::catch` sobre a chamada, que lança ao serializar.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use std::cell::RefCell;
use std::future::Future;
use std::rc::{Rc, Weak};

use common::{ensure_no_disposables_are_leaked_in_test_suite, run_with_faked_timers};
use je_base::buffer::VSBuffer;
use je_base::cancellation::{CancellationToken, CancellationTokenSource};
use je_base::dynamic::{Dynamic, ObjectRef};
use je_base::errors::{self, Error};
use je_base::event::{Emitter, Event};
use je_base::js_string::JsString;
use je_base::lifecycle::{DisposableRef, DisposableStore, IDisposable};
use je_base::parts::ipc::IMessagePassingProtocol;
use je_base::promise::{MaybePromise, Promise};
use je_workbench::api::common::ext_host_protocol::ProxyIdentifier;
use je_workbench::api::common::ext_host_values::{class_of, vs_buffer_from_dynamic, vs_buffer_to_dynamic};
use je_workbench::services::extensions::rpc_protocol::{
    as_cancellation_token, as_serializable_object_with_buffers, cancellation_token_to_dynamic, rejected_value, rejection_with_value,
    serializable_object_with_buffers, IRpcActor, RpcProtocol, RpcProxy,
};

/// `MessagePassingProtocol`: entrega ao par, uma microtarefa depois do envio.
struct MessagePassingProtocol {
    pair: RefCell<Weak<MessagePassingProtocol>>,
    on_message: Rc<Emitter<VSBuffer>>,
}

impl MessagePassingProtocol {
    fn new() -> Rc<Self> {
        Rc::new(Self { pair: RefCell::new(Weak::new()), on_message: Emitter::new() })
    }

    fn set_pair(&self, other: &Rc<MessagePassingProtocol>) {
        *self.pair.borrow_mut() = Rc::downgrade(other);
    }
}

impl IMessagePassingProtocol for MessagePassingProtocol {
    fn send(&self, buffer: VSBuffer) {
        let pair = self.pair.borrow().clone();
        let _ = Promise::resolved(()).then(move |_| {
            if let Some(pair) = pair.upgrade() {
                pair.on_message.fire(buffer);
            }
        });
    }

    fn on_message(&self) -> Event<VSBuffer> {
        self.on_message.event()
    }
}

type Delegate = Rc<dyn Fn(&[Dynamic]) -> MaybePromise<Dynamic>>;

/// `BClass`: `$m(a1, a2)` devolve `Promise.resolve(delegate.call(null, a1, a2))`.
struct BClass {
    delegate: Rc<RefCell<Delegate>>,
}

impl IRpcActor for BClass {
    fn invoke(&self, method: &str, args: Vec<Dynamic>) -> Option<MaybePromise<Dynamic>> {
        if method != "$m" {
            return None;
        }
        let delegate = self.delegate.borrow().clone();
        let argument = |index: usize| args.get(index).cloned().unwrap_or(Dynamic::Undefined);
        Some(delegate(&[argument(0), argument(1)]))
    }
}

struct Fixture {
    store: Rc<DisposableStore>,
    delegate: Rc<RefCell<Delegate>>,
    b_proxy: RpcProxy,
    b_protocol: Rc<RpcProtocol>,
}

impl Fixture {
    fn set_delegate(&self, delegate: impl Fn(&[Dynamic]) -> MaybePromise<Dynamic> + 'static) {
        *self.delegate.borrow_mut() = Rc::new(delegate);
    }

    /// `bProxy.$m(a1, a2)`.
    fn call(&self, args: Vec<Dynamic>) -> Promise<Dynamic> {
        self.b_proxy.call("$m", args).promise()
    }
}

/// O `setup` da suíte.
fn setup(store: &Rc<DisposableStore>) -> Fixture {
    let a_protocol = MessagePassingProtocol::new();
    let b_protocol = MessagePassingProtocol::new();
    a_protocol.set_pair(&b_protocol);
    b_protocol.set_pair(&a_protocol);

    let a = store.add_rc(RpcProtocol::new(a_protocol.clone(), None));
    let b_rpc = store.add_rc(RpcProtocol::new(b_protocol.clone(), None));
    // Os dois protocolos de transporte vivem enquanto o caso viver.
    let keep: Rc<dyn std::any::Any> = Rc::new((a_protocol, b_protocol));
    store.add(je_base::lifecycle::to_disposable(move || drop(keep)));

    let b_identifier = ProxyIdentifier::<BClass>::new("bb");
    let delegate: Rc<RefCell<Delegate>> = Rc::new(RefCell::new(Rc::new(|_: &[Dynamic]| MaybePromise::Value(Dynamic::Undefined))));
    b_rpc.set_actor(b_identifier.sid, Rc::new(BClass { delegate: delegate.clone() }));
    let b_proxy = a.proxy(&b_identifier);
    Fixture { store: store.clone(), delegate, b_proxy, b_protocol: b_rpc }
}

/// `suite`: o guarda, o `setup` e o corpo assíncrono do caso.
fn with_fixture<Fut: Future<Output = ()> + 'static>(body: impl FnOnce(Fixture) -> Fut + 'static) {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let store = guard.store();
    run_with_faked_timers(async move {
        body(setup(&store)).await;
    });
}

fn number(value: f64) -> Dynamic {
    Dynamic::Number(value)
}

fn as_number(value: &Dynamic) -> f64 {
    match value {
        Dynamic::Number(n) => *n,
        other => panic!("esperava um número, veio {other:?}"),
    }
}

/// `VSBuffer.alloc(4)` com `[1, 2, 3, 4]`.
fn buffer_1234() -> VSBuffer {
    let b = VSBuffer::alloc(4);
    for (index, byte) in [1u8, 2, 3, 4].into_iter().enumerate() {
        b.write_u_int8(byte, index);
    }
    b
}

fn assert_buffer_1234(value: &Dynamic) {
    assert_eq!(class_of(value), Some("VSBuffer"));
    let buffer = vs_buffer_from_dynamic(value).expect("VSBuffer");
    assert_eq!(buffer.to_vec(), vec![1, 2, 3, 4]);
}

#[test]
fn simple_call() {
    with_fixture(|f| async move {
        f.set_delegate(|args| MaybePromise::Value(number(as_number(&args[0]) + as_number(&args[1]))));
        let res = f.call(vec![number(4.0), number(1.0)]).await.expect("resultado");
        assert_eq!(as_number(&res), 5.0);
    });
}

#[test]
fn simple_call_without_result() {
    with_fixture(|f| async move {
        f.set_delegate(|_| MaybePromise::Value(Dynamic::Undefined));
        let res = f.call(vec![number(4.0), number(1.0)]).await.expect("resultado");
        assert!(res.is_undefined());
    });
}

#[test]
fn passing_buffer_as_argument() {
    with_fixture(|f| async move {
        f.set_delegate(|args| {
            assert_eq!(class_of(&args[0]), Some("VSBuffer"));
            let buffer = vs_buffer_from_dynamic(&args[0]).expect("VSBuffer");
            MaybePromise::Value(number(f64::from(buffer.read_u_int8(as_number(&args[1]) as usize))))
        });
        let res = f.call(vec![vs_buffer_to_dynamic(&buffer_1234()), number(2.0)]).await.expect("resultado");
        assert_eq!(as_number(&res), 3.0);
    });
}

#[test]
fn returning_a_buffer() {
    with_fixture(|f| async move {
        f.set_delegate(|_| MaybePromise::Value(vs_buffer_to_dynamic(&buffer_1234())));
        let res = f.call(vec![number(4.0), number(1.0)]).await.expect("resultado");
        assert_buffer_1234(&res);
    });
}

#[test]
fn cancelling_a_call_via_cancellation_token_before() {
    with_fixture(|f| async move {
        f.set_delegate(|args| MaybePromise::Value(number(as_number(&args[0]) + 1.0)));
        let result = f.call(vec![number(4.0), cancellation_token_to_dynamic(CancellationToken::Cancelled)]).await;
        assert!(result.is_err(), "should not receive result");
    });
}

#[test]
fn passing_cancellation_token_none() {
    with_fixture(|f| async move {
        f.set_delegate(|args| {
            assert!(as_cancellation_token(&args[1]).is_some());
            MaybePromise::Value(number(as_number(&args[0]) + 1.0))
        });
        let res = f.call(vec![number(4.0), cancellation_token_to_dynamic(CancellationToken::None)]).await.expect("resultado");
        assert_eq!(as_number(&res), 5.0);
    });
}

#[test]
fn cancelling_a_call_via_cancellation_token_quickly() {
    with_fixture(|f| async move {
        // this is an implementation which, when cancellation is triggered, will return 7
        f.set_delegate(|args| {
            let token = as_cancellation_token(&args[1]).expect("token");
            MaybePromise::Promise(Promise::with_executor(move |resolver| {
                let disposable: Rc<RefCell<Option<DisposableRef>>> = Rc::default();
                let slot = disposable.clone();
                let listener = token.on_cancellation_requested().listen(move |_| {
                    if let Some(listener) = slot.borrow_mut().take() {
                        listener.dispose();
                    }
                    resolver.resolve(number(7.0));
                });
                *disposable.borrow_mut() = Some(listener);
            }))
        });
        let token_source = CancellationTokenSource::new(None);
        let p = f.call(vec![number(4.0), cancellation_token_to_dynamic(token_source.token())]);
        token_source.cancel();
        let res = p.await.expect("should not receive error");
        assert_eq!(as_number(&res), 7.0);
    });
}

#[test]
fn releases_cancellation_handler_when_the_invoked_call_does_not_settle() {
    with_fixture(|f| async move {
        let (remote_token, resolve_remote_token) = Promise::<Dynamic>::new();
        f.set_delegate(move |args| {
            resolve_remote_token.resolve(args[1].clone());
            MaybePromise::Promise(Promise::<Dynamic>::new().0)
        });

        let token_source = CancellationTokenSource::new(None);
        let source = token_source.clone();
        f.store.add(je_base::lifecycle::to_disposable(move || source.dispose_with(false)));
        let _ = f.b_proxy.call("$m", vec![number(4.0), cancellation_token_to_dynamic(token_source.token())]);
        let token = as_cancellation_token(&remote_token.await.expect("token")).expect("token");
        let (cancellation_requested, resolve_requested) = Promise::<()>::new();
        f.store.add(token.on_cancellation_requested().listen(move |_| resolve_requested.resolve(())));
        token_source.cancel();
        cancellation_requested.await.expect("cancelamento");

        assert_eq!(f.b_protocol.cancel_invoked_handler_ids(), Vec::<String>::new());
    });
}

#[test]
fn does_not_track_uncancellable_calls_that_do_not_settle() {
    with_fixture(|f| async move {
        let (invoked, resolve_invoked) = Promise::<()>::new();
        f.set_delegate(move |_| {
            resolve_invoked.resolve(());
            MaybePromise::Promise(Promise::<Dynamic>::new().0)
        });

        let _ = f.b_proxy.call("$m", vec![number(4.0), number(1.0)]);
        invoked.await.expect("invocada");

        assert_eq!(f.b_protocol.cancel_invoked_handler_ids(), Vec::<String>::new());
    });
}

#[test]
fn throwing_an_error() {
    with_fixture(|f| async move {
        f.set_delegate(|_| errors::throw(Error::new("nope")));
        let err = f.call(vec![number(4.0), number(1.0)]).await.expect_err("unexpected");
        assert_eq!(err.message, "nope");
    });
}

#[test]
fn error_promise() {
    with_fixture(|f| async move {
        f.set_delegate(|_| MaybePromise::Promise(Promise::rejected(rejection_with_value(&Dynamic::Undefined))));
        let err = f.call(vec![number(4.0), number(1.0)]).await.expect_err("unexpected");
        assert!(rejected_value(&err).expect("valor rejeitado").is_undefined());
    });
}

#[test]
fn issue_60450_converting_circular_structure_to_json() {
    with_fixture(|f| async move {
        f.set_delegate(|_| {
            let circular = ObjectRef::new();
            circular.set(&JsString::from("self"), Dynamic::Object(circular.clone()));
            MaybePromise::Value(Dynamic::Object(circular))
        });
        let res = f.call(vec![number(4.0), number(1.0)]).await.expect("unexpected");
        assert_eq!(res, Dynamic::Null);
    });
}

#[test]
fn issue_72798_null_errors_are_hard_to_digest() {
    with_fixture(|f| async move {
        f.set_delegate(|_| errors::throw(rejection_with_value(&Dynamic::object([("what", Dynamic::string("what"))]))));
        let err = f.call(vec![number(4.0), number(1.0)]).await.expect_err("unexpected");
        assert_eq!(rejected_value(&err).expect("valor rejeitado").get_str("what"), Dynamic::string("what"));
    });
}

#[test]
fn undefined_arguments_arrive_as_null() {
    with_fixture(|f| async move {
        f.set_delegate(|args| {
            assert!(args[0].is_undefined());
            assert_eq!(args[1], Dynamic::Null);
            MaybePromise::Value(number(7.0))
        });
        let res = f.call(vec![Dynamic::Undefined, Dynamic::Null]).await.expect("resultado");
        assert_eq!(as_number(&res), 7.0);
    });
}

#[test]
fn issue_81424_serialize_request_should_throw_if_an_argument_can_not_be_serialized() {
    with_fixture(|f| async move {
        let bad_object = ObjectRef::new();
        bad_object.set(&JsString::from("loop"), Dynamic::Object(bad_object.clone()));

        let result = errors::catch(|| f.b_proxy.call("$m", vec![Dynamic::Object(bad_object), Dynamic::string("2")]));
        assert!(result.is_err());
    });
}

#[test]
fn serializable_object_with_buffers_is_correctly_transfered() {
    with_fixture(|f| async move {
        f.set_delegate(|args| {
            let value = as_serializable_object_with_buffers(&args[0]).expect("SerializableObjectWithBuffers").value.clone();
            let Dynamic::String(string) = value.get_str("string") else {
                panic!("esperava uma string");
            };
            MaybePromise::Value(serializable_object_with_buffers(Dynamic::object([
                ("string", Dynamic::string(&format!("{} world", string.to_string_lossy()))),
                ("buff", value.get_str("buff")),
            ])))
        });

        let b = buffer_1234();
        let arg = serializable_object_with_buffers(Dynamic::object([("string", Dynamic::string("hello")), ("buff", vs_buffer_to_dynamic(&b))]));
        let res = f.call(vec![arg, Dynamic::Undefined]).await.expect("resultado");
        let res = as_serializable_object_with_buffers(&res).expect("SerializableObjectWithBuffers");
        assert_eq!(res.value.get_str("string"), Dynamic::string("hello world"));
        assert_buffer_1234(&res.value.get_str("buff"));
    });
}
