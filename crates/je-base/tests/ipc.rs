//! Porte de `src/vs/base/parts/ipc/test/common/ipc.test.ts` (suíte `Base IPC`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - As suítes aninhadas (`one to one`, `one to one (proxy)`, ...) são módulos, porque os nomes dos casos se
//!   repetem entre elas. O `setup` de cada uma é a função `setup` do módulo, chamada no começo de cada caso, e
//!   o `teardown` é o `Drop` do `Fixture`.
//! - Os valores que atravessam o canal são `Marshalled`; `assert.strictEqual(r, 'polo')` compara o texto.
//! - O `ProxyChannel` usa `IProxyService` no lugar do acesso dinâmico a propriedades (ver `parts::ipc`).
//!
//! Pendente: `request rejects (and cleans up) when serialization throws on the deferred path`. O caso monta um
//! argumento circular para o `JSON.stringify` lançar; um `Marshalled` é uma árvore e não representa um ciclo, e
//! a serialização do porte não tem como falhar, então o caminho de limpeza que o caso exercita não existe.

#[path = "ipc/common.rs"]
mod ipc_common;
mod common;

use std::cell::RefCell;
use std::rc::Rc;

use common::{ensure_no_disposables_are_leaked_in_test_suite, run_with_faked_timers, timeout};
use ipc_common::{ProxyTestService, TestChannel, TestChannelClient, TestIPCServer, TestService, as_string, create_protocol_pair, string};
use je_base::buffer::VSBuffer;
use je_base::cancellation::{CancellationToken, CancellationTokenSource};
use je_base::errors::Error;
use je_base::event::{Emitter, Event};
use je_base::lifecycle::{DisposableStore, IDisposable};
use je_base::marshalling::Marshalled;
use je_base::parts::ipc::proxy_channel::{self, ICreateProxyServiceOptions, ICreateServiceChannelOptions, IProxyService};
use je_base::parts::ipc::{BufferReader, BufferWriter, ClientSelector, IMessagePassingProtocol, deserialize, get_delayed_channel, serialize};
use je_base::promise::Promise;
use je_base::runtime;
use je_base::uri::URI;
use je_base::path::Platform;

const TEST_CHANNEL_ID: &str = "testchannel";

fn messages(received: &Rc<RefCell<Vec<String>>>) -> Vec<String> {
    received.borrow().clone()
}

fn collector(received: &Rc<RefCell<Vec<String>>>) -> impl Fn(&Marshalled) + 'static {
    let received = received.clone();
    move |msg| received.borrow_mut().push(as_string(msg))
}

fn strings(values: &[&str]) -> Vec<String> {
    values.iter().map(|s| (*s).to_owned()).collect()
}

#[test]
fn delayed_channel_handles_rejected_listeners() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    run_with_faked_timers(async move {
        let error = Error::new("Channel unavailable");
        let channel = get_delayed_channel(Promise::rejected(error.clone()));
        store.add(channel.listen("event", Marshalled::Undefined).listen(|_| {}));

        let result = channel.call("command", Marshalled::Undefined, CancellationToken::None).await;
        assert_eq!(result.unwrap_err(), error);
        timeout(0).await.unwrap();
    });
}

#[test]
fn create_protocol_pair_test() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    run_with_faked_timers(async move {
        let (client_protocol, server_protocol) = create_protocol_pair();

        let b1 = VSBuffer::alloc(0);
        client_protocol.send(b1.clone());

        let b3 = VSBuffer::alloc(0);
        server_protocol.send(b3.clone());

        let b2 = server_protocol.on_message().to_promise(None).await.unwrap();
        let b4 = client_protocol.on_message().to_promise(None).await.unwrap();

        assert!(b1.shares_storage_with(&b2));
        assert!(b3.shares_storage_with(&b4));
    });
}

mod one_to_one {
    use super::*;

    struct Fixture {
        store: common::LeakGuard,
        service: Rc<TestService>,
        ipc_service: TestChannelClient,
    }

    fn setup() -> Fixture {
        let store = ensure_no_disposables_are_leaked_in_test_suite();
        let service = store.add_rc(TestService::new());
        let test_server = store.add_rc(TestIPCServer::new());
        test_server.server.register_channel(TEST_CHANNEL_ID, Rc::new(TestChannel { service: service.clone() }));
        let client = store.add_rc(test_server.create_connection("client1"));
        let ipc_service = TestChannelClient { channel: client.get_channel(TEST_CHANNEL_ID) };
        Fixture { store, service, ipc_service }
    }

    #[test]
    fn call_success() {
        let f = setup();
        run_with_faked_timers(async move {
            let r = f.ipc_service.marco().await.unwrap();
            assert_eq!(as_string(&r), "polo");
            drop(f);
        });
    }

    #[test]
    fn call_error() {
        let f = setup();
        run_with_faked_timers(async move {
            let err = f.ipc_service.error("nice error").await.expect_err("não devia chegar aqui");
            assert_eq!(err.message, "nice error");
            drop(f);
        });
    }

    #[test]
    fn cancel_call_with_cancelled_cancellation_token() {
        let f = setup();
        run_with_faked_timers(async move {
            let err = f.ipc_service.never_complete_ct(CancellationToken::Cancelled).await.expect_err("não devia chegar aqui");
            assert_eq!(err.message, "Canceled");
            drop(f);
        });
    }

    #[test]
    fn cancel_call_with_cancellation_token_sync() {
        let f = setup();
        run_with_faked_timers(async move {
            let cts = CancellationTokenSource::new(None);
            let promise = f.ipc_service.never_complete_ct(cts.token());

            cts.cancel();

            let err = promise.await.expect_err("não devia chegar aqui");
            assert_eq!(err.message, "Canceled");
            cts.dispose();
            drop(f);
        });
    }

    #[test]
    fn cancel_call_with_cancellation_token_async() {
        let f = setup();
        run_with_faked_timers(async move {
            let cts = Rc::new(CancellationTokenSource::new(None));
            let promise = f.ipc_service.never_complete_ct(cts.token());

            let to_cancel = cts.clone();
            runtime::set_timeout(move || to_cancel.cancel(), 0);

            let err = promise.await.expect_err("não devia chegar aqui");
            assert_eq!(err.message, "Canceled");
            cts.dispose();
            drop(f);
        });
    }

    #[test]
    fn listen_to_events() {
        let f = setup();
        run_with_faked_timers(async move {
            let received: Rc<RefCell<Vec<String>>> = Rc::default();

            f.store.add(f.ipc_service.on_pong().listen(collector(&received)));
            timeout(0).await.unwrap();

            assert_eq!(messages(&received), Vec::<String>::new());
            f.service.ping("hello");
            timeout(0).await.unwrap();

            assert_eq!(messages(&received), strings(&["hello"]));
            f.service.ping("world");
            timeout(0).await.unwrap();

            assert_eq!(messages(&received), strings(&["hello", "world"]));
            drop(f);
        });
    }

    #[test]
    fn unbuffered_events_subscribe_lazily() {
        let f = setup();
        let service = f.store.add_rc(TestService::new());
        let channel_disposables = f.store.add_rc(DisposableStore::new());
        let channel = proxy_channel::from_service(
            Rc::new(ProxyTestService(service.clone())),
            &channel_disposables,
            Some(ICreateServiceChannelOptions { unbuffered_events: vec!["onPong".to_owned()], ..Default::default() }),
        );
        let on_pong = channel.listen(&string("context"), "onPong", Marshalled::Undefined);
        let received: Rc<RefCell<Vec<String>>> = Rc::default();

        service.ping("before");
        assert!(!service.has_pong_listeners());

        let listener = channel_disposables.add(on_pong.listen(collector(&received)));
        assert!(service.has_pong_listeners());
        service.ping("after");
        channel_disposables.delete(&listener);

        assert_eq!((messages(&received), service.has_pong_listeners()), (strings(&["after"]), false));
    }

    #[test]
    fn listen_to_events_resubscribe() {
        let f = setup();
        run_with_faked_timers(async move {
            let on_pong = f.ipc_service.on_pong();
            let received: Rc<RefCell<Vec<String>>> = Rc::default();

            let disposable1 = on_pong.listen(collector(&received));
            timeout(0).await.unwrap();
            assert_eq!(messages(&received), Vec::<String>::new());
            f.service.ping("hello");
            timeout(0).await.unwrap();
            assert_eq!(messages(&received), strings(&["hello"]));
            disposable1.dispose();

            let disposable2 = on_pong.listen(collector(&received));
            timeout(0).await.unwrap();
            assert_eq!(messages(&received), strings(&["hello"]));
            f.service.ping("world");
            timeout(0).await.unwrap();
            assert_eq!(messages(&received), strings(&["hello", "world"]));
            disposable2.dispose();
            drop(f);
        });
    }

    #[test]
    fn buffers_in_arrays() {
        let f = setup();
        run_with_faked_timers(async move {
            let r = f.ipc_service.buffers_length(vec![VSBuffer::alloc(2), VSBuffer::alloc(3)]).await.unwrap();
            assert!(matches!(r, Marshalled::Number(n) if n == 5.0));
            drop(f);
        });
    }

    #[test]
    fn round_trips_numbers() {
        let _f = setup();
        let input: Vec<f64> = vec![0.0, 1.0, -1.0, 12345.0, -12345.0, 42.6, 123_412_341_234.0];

        let mut writer = BufferWriter::new();
        serialize(&mut writer, &Marshalled::Array(input.iter().copied().map(Marshalled::Number).collect()));
        let Marshalled::Array(output) = deserialize(&mut BufferReader::new(writer.buffer())) else { panic!("devia ser um array") };
        let output: Vec<f64> = output.iter().map(|v| if let Marshalled::Number(n) = v { *n } else { panic!("devia ser um número") }).collect();
        assert_eq!(output, input);
    }

    #[test]
    fn buffer_writer_releases_its_buffers_on_dispose() {
        let _f = setup();
        let mut writer = BufferWriter::new();
        serialize(&mut writer, &Marshalled::Array(vec![string("a"), string("b"), string("c")]));
        assert!(writer.buffer().byte_length() > 0);

        writer.dispose();

        // After dispose the writer no longer retains the serialized buffers, so `buffer` is empty. This guards
        // against a thrown error's captured stack pinning large intermediate buffers.
        assert_eq!(writer.buffer().byte_length(), 0);
    }
}

mod one_to_one_proxy {
    use super::*;
    use je_base::parts::ipc::proxy_channel::ServiceProxy;

    struct Fixture {
        _store: common::LeakGuard,
        disposables: Rc<DisposableStore>,
        service: Rc<TestService>,
        ipc_service: ServiceProxy,
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            self.disposables.clear();
        }
    }

    fn setup() -> Fixture {
        // O `disposables` é um `const` da suíte, criado antes do rastreador de vazamentos do teste.
        let disposables = DisposableStore::new();
        let store = ensure_no_disposables_are_leaked_in_test_suite();
        let service = store.add_rc(TestService::new());
        let test_server = disposables.add_rc(TestIPCServer::new());
        test_server.server.register_channel(TEST_CHANNEL_ID, proxy_channel::from_service(Rc::new(ProxyTestService(service.clone())), &disposables, None));
        let client = disposables.add_rc(test_server.create_connection("client1"));
        let ipc_service = proxy_channel::to_service(client.get_channel(TEST_CHANNEL_ID), None);
        Fixture { _store: store, disposables, service, ipc_service }
    }

    #[test]
    fn call_success() {
        let f = setup();
        run_with_faked_timers(async move {
            let r = f.ipc_service.call("marco", vec![]).await.unwrap();
            assert_eq!(as_string(&r), "polo");
            drop(f);
        });
    }

    #[test]
    fn call_error() {
        let f = setup();
        run_with_faked_timers(async move {
            let err = f.ipc_service.call("error", vec![string("nice error")]).await.expect_err("não devia chegar aqui");
            assert_eq!(err.message, "nice error");
            drop(f);
        });
    }

    #[test]
    fn listen_to_events() {
        let f = setup();
        run_with_faked_timers(async move {
            let received: Rc<RefCell<Vec<String>>> = Rc::default();

            f.disposables.add(f.ipc_service.event("onPong").listen(collector(&received)));
            timeout(0).await.unwrap();

            assert_eq!(messages(&received), Vec::<String>::new());
            f.service.ping("hello");
            timeout(0).await.unwrap();

            assert_eq!(messages(&received), strings(&["hello"]));
            f.service.ping("world");
            timeout(0).await.unwrap();

            assert_eq!(messages(&received), strings(&["hello", "world"]));
            drop(f);
        });
    }

    #[test]
    fn listen_to_events_resubscribe() {
        let f = setup();
        run_with_faked_timers(async move {
            let on_pong = f.ipc_service.event("onPong");
            let received: Rc<RefCell<Vec<String>>> = Rc::default();

            let disposable1 = on_pong.listen(collector(&received));
            timeout(0).await.unwrap();
            assert_eq!(messages(&received), Vec::<String>::new());
            f.service.ping("hello");
            timeout(0).await.unwrap();
            assert_eq!(messages(&received), strings(&["hello"]));
            disposable1.dispose();

            let disposable2 = on_pong.listen(collector(&received));
            timeout(0).await.unwrap();
            assert_eq!(messages(&received), strings(&["hello"]));
            f.service.ping("world");
            timeout(0).await.unwrap();
            assert_eq!(messages(&received), strings(&["hello", "world"]));
            disposable2.dispose();
            drop(f);
        });
    }

    #[test]
    fn marshalling_uri() {
        let f = setup();
        run_with_faked_timers(async move {
            let uri = URI::file("foobar", Platform::Posix);
            let r = f.ipc_service.call("marshall", vec![Marshalled::Uri(uri.clone())]).await.unwrap();
            let Marshalled::Uri(r) = r else { panic!("esperava um URI revivido, veio {r:?}") };
            assert_eq!(r, uri);
            drop(f);
        });
    }

    #[test]
    fn buffers_in_arrays() {
        let f = setup();
        run_with_faked_timers(async move {
            let r = f.ipc_service.call("buffersLength", vec![Marshalled::Array(vec![Marshalled::Buffer(VSBuffer::alloc(2)), Marshalled::Buffer(VSBuffer::alloc(3))])]).await.unwrap();
            assert!(matches!(r, Marshalled::Number(n) if n == 5.0));
            drop(f);
        });
    }

    #[test]
    fn proxy_is_not_a_thenable() {
        // O proxy do original responde `undefined` a `then`, para `await` não repassar a chamada pelo canal. O
        // `ServiceProxy` do porte não é uma `Promise`/`Future` nem expõe `then`: uma propriedade desconhecida
        // (`property`) é `None`, e `then` não vira chamada. Aguardar o valor devolve o próprio proxy.
        let f = setup();
        run_with_faked_timers(async move {
            assert!(f.ipc_service.property("then").is_none());

            let awaited = async { &f.ipc_service }.await;
            let r = awaited.call("marco", vec![]).await.unwrap();
            assert_eq!(as_string(&r), "polo");
        });
    }
}

mod one_to_one_proxy_extra_context {
    use super::*;

    #[test]
    fn call_extra_context() {
        // O `disposables` é um `const` da suíte, criado antes do rastreador de vazamentos do teste.
        let disposables = DisposableStore::new();
        let store = ensure_no_disposables_are_leaked_in_test_suite();
        let service = store.add_rc(TestService::new());
        let test_server = disposables.add_rc(TestIPCServer::new());
        test_server.server.register_channel(TEST_CHANNEL_ID, proxy_channel::from_service(Rc::new(ProxyTestService(service.clone())), &disposables, None));
        let client = disposables.add_rc(test_server.create_connection("client1"));
        let ipc_service = proxy_channel::to_service(
            client.get_channel(TEST_CHANNEL_ID),
            Some(ICreateProxyServiceOptions { context: Some(string("Super Context")), ..Default::default() }),
        );
        run_with_faked_timers(async move {
            let r = ipc_service.call("context", vec![]).await.unwrap();
            assert_eq!(as_string(&r), "Super Context");
            disposables.clear();
        });
    }
}

mod one_to_one_proxy_event_buffering {
    use super::*;

    struct EmitterService {
        events: Vec<(String, Event<Marshalled>)>,
    }

    impl IProxyService for EmitterService {
        fn event_names(&self) -> Vec<String> {
            self.events.iter().map(|(n, _)| n.clone()).collect()
        }

        fn event(&self, name: &str) -> Option<Event<Marshalled>> {
            self.events.iter().find(|(n, _)| n == name).map(|(_, e)| e.clone())
        }

        fn dynamic_event(&self, _name: &str, _arg: Marshalled) -> Option<Event<Marshalled>> {
            None
        }

        fn call(&self, _name: &str, _args: Vec<Marshalled>) -> Option<Promise<Marshalled>> {
            None
        }
    }

    #[test]
    fn service_events_are_eagerly_subscribed_to_and_buffered_by_default() {
        let store = ensure_no_disposables_are_leaked_in_test_suite();
        run_with_faked_timers(async move {
            let disposables = store.add_rc(DisposableStore::new());
            let emitter = disposables.add_rc(Emitter::<Marshalled>::new());
            let service = Rc::new(EmitterService { events: vec![("onDidChange".to_owned(), emitter.event())] });

            let channel = proxy_channel::from_service(service, &disposables, None);
            assert!(emitter.has_listeners());

            emitter.fire(string("early"));
            let received: Rc<RefCell<Vec<String>>> = Rc::default();
            disposables.add(channel.listen(&Marshalled::Undefined, "onDidChange", Marshalled::Undefined).listen(collector(&received)));
            timeout(0).await.unwrap();
            assert_eq!(messages(&received), strings(&["early"]));

            emitter.fire(string("late"));
            assert_eq!(messages(&received), strings(&["early", "late"]));
        });
    }

    #[test]
    fn unbuffered_events_do_not_change_buffering_for_other_service_events() {
        let store = ensure_no_disposables_are_leaked_in_test_suite();
        run_with_faked_timers(async move {
            let disposables = store.add_rc(DisposableStore::new());
            let buffered = disposables.add_rc(Emitter::<Marshalled>::new());
            let unbuffered = disposables.add_rc(Emitter::<Marshalled>::new());
            let service = Rc::new(EmitterService {
                events: vec![("onDidChange".to_owned(), buffered.event()), ("onDidOutput".to_owned(), unbuffered.event())],
            });
            let channel = proxy_channel::from_service(service, &disposables, Some(ICreateServiceChannelOptions { unbuffered_events: vec!["onDidOutput".to_owned()], ..Default::default() }));
            let eager_subscriptions = (buffered.has_listeners(), unbuffered.has_listeners());

            buffered.fire(string("early"));
            unbuffered.fire(string("early"));
            let received_buffered: Rc<RefCell<Vec<String>>> = Rc::default();
            let received_unbuffered: Rc<RefCell<Vec<String>>> = Rc::default();
            disposables.add(channel.listen(&Marshalled::Undefined, "onDidChange", Marshalled::Undefined).listen(collector(&received_buffered)));
            disposables.add(channel.listen(&Marshalled::Undefined, "onDidOutput", Marshalled::Undefined).listen(collector(&received_unbuffered)));
            timeout(0).await.unwrap();
            buffered.fire(string("late"));
            unbuffered.fire(string("late"));
            disposables.dispose();

            assert_eq!(
                (eager_subscriptions, messages(&received_buffered), messages(&received_unbuffered), (buffered.has_listeners(), unbuffered.has_listeners())),
                ((true, false), strings(&["early", "late"]), strings(&["late"]), (false, false))
            );
        });
    }
}

mod one_to_many {
    use super::*;

    #[test]
    fn all_clients_get_pinged() {
        let store = ensure_no_disposables_are_leaked_in_test_suite();
        run_with_faked_timers(async move {
            let service = store.add_rc(TestService::new());
            let channel = Rc::new(TestChannel { service: service.clone() });
            let server = store.add_rc(TestIPCServer::new());
            server.server.register_channel("channel", channel);

            let client1_got_pinged = Rc::new(RefCell::new(false));
            let client1 = store.add_rc(server.create_connection("client1"));
            let ipc_service1 = TestChannelClient { channel: client1.get_channel("channel") };
            let flag = client1_got_pinged.clone();
            store.add(ipc_service1.on_pong().listen(move |_| *flag.borrow_mut() = true));

            let client2_got_pinged = Rc::new(RefCell::new(false));
            let client2 = store.add_rc(server.create_connection("client2"));
            let ipc_service2 = TestChannelClient { channel: client2.get_channel("channel") };
            let flag = client2_got_pinged.clone();
            store.add(ipc_service2.on_pong().listen(move |_| *flag.borrow_mut() = true));

            timeout(1).await.unwrap();
            service.ping("hello");

            timeout(1).await.unwrap();
            assert!(*client1_got_pinged.borrow(), "client 1 got pinged");
            assert!(*client2_got_pinged.borrow(), "client 2 got pinged");
        });
    }

    #[test]
    fn server_gets_pings_from_all_clients_broadcast_channel() {
        let store = ensure_no_disposables_are_leaked_in_test_suite();
        run_with_faked_timers(async move {
            let server = store.add_rc(TestIPCServer::new());

            let client1 = server.create_connection("client1");
            let client_service1 = store.add_rc(TestService::new());
            client1.register_channel("channel", Rc::new(TestChannel { service: client_service1.clone() }));

            let pings: Rc<RefCell<Vec<String>>> = Rc::default();
            let channel = server.server.get_channel("channel", ClientSelector::Filter(Rc::new(|_| true)));
            let service = TestChannelClient { channel };
            store.add(service.on_pong().listen(collector(&pings)));

            timeout(1).await.unwrap();
            client_service1.ping("hello 1");

            timeout(1).await.unwrap();
            assert_eq!(messages(&pings), strings(&["hello 1"]));

            let client2 = server.create_connection("client2");
            let client_service2 = store.add_rc(TestService::new());
            client2.register_channel("channel", Rc::new(TestChannel { service: client_service2.clone() }));

            timeout(1).await.unwrap();
            client_service2.ping("hello 2");

            timeout(1).await.unwrap();
            assert_eq!(messages(&pings), strings(&["hello 1", "hello 2"]));

            client1.dispose();
            client_service1.ping("hello 1");

            timeout(1).await.unwrap();
            assert_eq!(messages(&pings), strings(&["hello 1", "hello 2"]));

            timeout(1).await.unwrap();
            client_service2.ping("hello again 2");

            timeout(1).await.unwrap();
            assert_eq!(messages(&pings), strings(&["hello 1", "hello 2", "hello again 2"]));

            client2.dispose();
        });
    }
}
