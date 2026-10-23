//! Testes próprios de `worker` (`webWorker.ts`), que o VS Code não testa diretamente. Não é porte de teste do
//! VS Code.
//!
//! Dois transportes: um `LoopWorker` que liga o cliente a um servidor na mesma thread (mensagens entregues no
//! próximo tick), e o `ThreadWebWorker`, com o servidor numa thread do SO e as mensagens como bytes.

mod common;

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use common::{ensure_no_disposables_are_leaked_in_test_suite, run_with_faked_timers, timeout};
use je_base::errors::Error;
use je_base::event::{Emitter, Event};
use je_base::lifecycle::IDisposable;
use je_base::marshalling::Marshalled;
use je_base::promise::Promise;
use je_base::runtime;
use je_base::worker::thread_worker::spawn;
use je_base::worker::{IWebWorker, IWebWorkerServer, IWorkerChannelHandler, Message, WebWorkerClient, WebWorkerServer, property_is_dynamic_event, property_is_event};

fn num(n: f64) -> Marshalled {
    Marshalled::Number(n)
}

fn text(s: &str) -> Marshalled {
    Marshalled::String(s.into())
}

fn as_number(value: &Marshalled) -> f64 {
    match value {
        Marshalled::Number(n) => *n,
        other => panic!("esperava um número, veio {other:?}"),
    }
}

/// O `requestHandler` dos testes: `$add`, `$fail`, o evento `onTick` e o evento dinâmico `onDynamicRange`.
struct Calculator {
    ticks: Rc<Emitter<Marshalled>>,
}

impl IWorkerChannelHandler for Calculator {
    fn call(&self, method: &str, args: Vec<Marshalled>) -> Option<Promise<Marshalled>> {
        match method {
            "$add" => Some(Promise::resolved(num(args.iter().map(as_number).sum()))),
            "$fail" => Some(Promise::rejected(Error::named("RangeError", "deu ruim").with_stack("RangeError: deu ruim\n    at worker"))),
            "$tick" => {
                self.ticks.fire(args.first().cloned().unwrap_or(Marshalled::Undefined));
                Some(Promise::resolved(Marshalled::Undefined))
            }
            _ => None,
        }
    }

    fn event(&self, event_name: &str) -> Option<Event<Marshalled>> {
        (event_name == "onTick").then(|| self.ticks.event())
    }

    fn dynamic_event(&self, event_name: &str, arg: Marshalled) -> Option<Event<Marshalled>> {
        (event_name == "onDynamicRange").then(|| {
            let offset = as_number(&arg);
            self.ticks.event().map(move |v| num(as_number(v) + offset), None)
        })
    }
}

fn calculator() -> Rc<dyn IWorkerChannelHandler> {
    Rc::new(Calculator { ticks: Emitter::new() })
}

/// O servidor na mesma thread que o cliente, com um tick de atraso em cada sentido.
struct LoopWorker {
    id: i64,
    server: RefCell<Option<Rc<WebWorkerServer>>>,
    on_message: Rc<Emitter<Message>>,
    on_error: Rc<Emitter<Error>>,
}

impl LoopWorker {
    fn new(id: i64, factory: impl FnOnce(&Rc<WebWorkerServer>) -> Rc<dyn IWorkerChannelHandler>) -> Rc<Self> {
        let worker = Rc::new(Self { id, server: RefCell::default(), on_message: Emitter::new(), on_error: Emitter::new() });
        let to_client = worker.on_message.clone();
        let server = WebWorkerServer::new(
            move |msg: &Message| {
                let (msg, to_client) = (msg.clone(), to_client.clone());
                runtime::set_timeout(move || to_client.fire(msg), 0);
            },
            factory,
        );
        *worker.server.borrow_mut() = Some(server);
        worker
    }
}

impl IDisposable for LoopWorker {
    fn dispose(&self) {
        self.on_message.dispose();
        self.on_error.dispose();
    }
}

impl IWebWorker for LoopWorker {
    fn get_id(&self) -> i64 {
        self.id
    }

    fn on_message(&self) -> Event<Message> {
        self.on_message.event()
    }

    fn on_error(&self) -> Event<Error> {
        self.on_error.event()
    }

    fn post_message(&self, message: &Message) {
        let (message, server) = (message.clone(), self.server.borrow().clone().expect("servidor criado"));
        runtime::set_timeout(move || server.onmessage(&message), 0);
    }
}

#[test]
fn event_name_conventions() {
    assert!(property_is_event("onTick"));
    assert!(!property_is_event("on"));
    assert!(!property_is_event("onlower"));
    assert!(!property_is_event("$onTick"));
    assert!(property_is_dynamic_event("onDynamicRange"));
    assert!(!property_is_dynamic_event("onDynamic"));
    assert!(!property_is_dynamic_event("onDynamicrange"));
}

#[test]
fn message_bytes_round_trip() {
    let messages = vec![
        Message::Request { vs_worker: 3, req: "7".into(), channel: "default".into(), method: "$add".into(), args: vec![num(1.0), text("x"), Marshalled::Array(vec![Marshalled::Null])] },
        Message::Reply { vs_worker: 3, seq: "7".into(), res: num(5.0), err: Marshalled::Undefined },
        Message::SubscribeEvent { vs_worker: 3, req: "8".into(), channel: "c".into(), event_name: "onTick".into(), arg: Marshalled::Undefined },
        Message::Event { vs_worker: 3, req: "8".into(), event: text("tick") },
        Message::UnsubscribeEvent { vs_worker: 3, req: "8".into() },
    ];
    for message in messages {
        let read = Message::from_bytes(&message.to_bytes()).expect("a mensagem volta");
        assert_eq!(format!("{:?}", read.to_marshalled()), format!("{:?}", message.to_marshalled()));
    }
    assert!(Message::from_bytes(&[0]).is_none());
}

#[test]
fn client_calls_server_methods_through_the_default_channel() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    run_with_faked_timers(async move {
        let worker = LoopWorker::new(1, |_| calculator());
        let client = store.add_rc(WebWorkerClient::new(worker));

        let sum = client.proxy.call("$add", vec![num(2.0), num(3.0), num(4.0)]).await.unwrap();
        assert_eq!(as_number(&sum), 9.0);
        client.on_module_loaded().await.unwrap();

        // Erros viajam com nome, mensagem e pilha.
        let error = client.proxy.call("$fail", vec![]).await.expect_err("devia rejeitar");
        assert_eq!((error.name.as_str(), error.message.as_str(), error.stack.as_deref()), ("RangeError", "deu ruim", Some("RangeError: deu ruim\n    at worker")));

        // Método e canal que não existem.
        let missing = client.proxy.call("$nope", vec![]).await.expect_err("devia rejeitar");
        assert_eq!(missing.message, "Missing method $nope on worker thread channel default");
        let missing_channel = client.get_channel("other").call("$add", vec![]).await.expect_err("devia rejeitar");
        assert_eq!(missing_channel.message, "Missing channel other on worker thread");

        // Só os `$métodos` existem no proxy.
        assert!(client.proxy.call("add", vec![]).await.is_err());
    });
}

#[test]
fn events_subscribe_lazily_and_unsubscribe() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    run_with_faked_timers(async move {
        let worker = LoopWorker::new(2, |_| calculator());
        let client = store.add_rc(WebWorkerClient::new(worker));
        let ticks = client.proxy.event("onTick");

        let received: Rc<RefCell<Vec<f64>>> = Rc::default();
        let sink = received.clone();
        let listener = ticks.listen(move |v| sink.borrow_mut().push(as_number(v)));
        timeout(5).await.unwrap();

        client.proxy.call("$tick", vec![num(1.0)]).await.unwrap();
        client.proxy.call("$tick", vec![num(2.0)]).await.unwrap();
        timeout(5).await.unwrap();
        assert_eq!(*received.borrow(), vec![1.0, 2.0]);

        listener.dispose();
        timeout(5).await.unwrap();
        client.proxy.call("$tick", vec![num(3.0)]).await.unwrap();
        timeout(5).await.unwrap();
        assert_eq!(*received.borrow(), vec![1.0, 2.0]);

        // Evento dinâmico.
        let dynamic: Rc<RefCell<Vec<f64>>> = Rc::default();
        let sink = dynamic.clone();
        let dynamic_listener = client.proxy.dynamic_event("onDynamicRange", num(100.0)).listen(move |v| sink.borrow_mut().push(as_number(v)));
        timeout(5).await.unwrap();
        client.proxy.call("$tick", vec![num(4.0)]).await.unwrap();
        timeout(5).await.unwrap();
        assert_eq!(*dynamic.borrow(), vec![104.0]);
        dynamic_listener.dispose();
        timeout(5).await.unwrap();
    });
}

#[test]
fn server_can_call_channels_set_on_the_client() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    run_with_faked_timers(async move {
        struct Greeter;
        impl IWorkerChannelHandler for Greeter {
            fn call(&self, method: &str, args: Vec<Marshalled>) -> Option<Promise<Marshalled>> {
                (method == "$greet").then(|| Promise::resolved(Marshalled::String(format!("olá, {}", if let Some(Marshalled::String(n)) = args.first() { n.to_string_lossy() } else { String::new() }).into())))
            }
        }
        struct CallsBack {
            server: Rc<WebWorkerServer>,
        }
        impl IWorkerChannelHandler for CallsBack {
            fn call(&self, method: &str, args: Vec<Marshalled>) -> Option<Promise<Marshalled>> {
                (method == "$relay").then(|| self.server.get_channel("greeter").call("$greet", args))
            }
        }

        let worker = LoopWorker::new(3, |server| Rc::new(CallsBack { server: server.clone() }));
        let client = store.add_rc(WebWorkerClient::new(worker));
        client.set_channel("greeter", Rc::new(Greeter));
        let reply = client.proxy.call("$relay", vec![text("mundo")]).await.unwrap();
        assert!(matches!(&reply, Marshalled::String(s) if s.to_string_lossy() == "olá, mundo"), "{reply:?}");
    });
}

#[test]
fn messages_for_another_worker_are_ignored() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    run_with_faked_timers(async move {
        let worker = LoopWorker::new(4, |_| calculator());
        let to_client = worker.on_message.clone();
        let client = store.add_rc(WebWorkerClient::new(worker));
        client.on_module_loaded().await.unwrap();
        // Uma resposta de outro worker (id diferente) e uma sem id (0) não resolvem nem quebram nada.
        to_client.fire(Message::Reply { vs_worker: 99, seq: "1".into(), res: Marshalled::Undefined, err: Marshalled::Undefined });
        to_client.fire(Message::Reply { vs_worker: 0, seq: "1".into(), res: Marshalled::Undefined, err: Marshalled::Undefined });
        let sum = client.proxy.call("$add", vec![num(1.0), num(1.0)]).await.unwrap();
        assert_eq!(as_number(&sum), 2.0);
    });
}

#[test]
fn os_thread_worker_runs_requests_and_events_on_another_thread() {
    let worker = spawn(5, Box::new(|_| calculator()));
    let client = WebWorkerClient::new(worker.clone());

    let main_thread = std::thread::current().id();
    let sum: Rc<RefCell<Option<Result<Marshalled, Error>>>> = Rc::default();
    let slot = sum.clone();
    client.proxy.call("$add", vec![num(40.0), num(2.0)]).then_result(move |r| {
        *slot.borrow_mut() = Some(r.clone());
        r
    });
    assert!(worker.run_until(Duration::from_secs(10), || sum.borrow().is_some()), "o worker respondeu");
    assert_eq!(as_number(&sum.borrow().clone().unwrap().unwrap()), 42.0);

    // Erro vindo da thread do worker.
    let failure: Rc<RefCell<Option<Error>>> = Rc::default();
    let slot = failure.clone();
    client.proxy.call("$fail", vec![]).catch(move |e| {
        *slot.borrow_mut() = Some(e);
        Marshalled::Undefined
    });
    assert!(worker.run_until(Duration::from_secs(10), || failure.borrow().is_some()));
    assert_eq!(failure.borrow().clone().unwrap().message, "deu ruim");

    // Evento: o worker dispara na própria thread e a thread principal recebe.
    let received: Rc<RefCell<Vec<f64>>> = Rc::default();
    let sink = received.clone();
    let listener = client.proxy.event("onTick").listen(move |v| sink.borrow_mut().push(as_number(v)));
    let ticked = Rc::new(RefCell::new(false));
    let flag = ticked.clone();
    // A inscrição precisa chegar ao worker antes do `$tick`, e as mensagens do canal chegam em ordem.
    client.proxy.call("$tick", vec![num(7.0)]).then(move |_| *flag.borrow_mut() = true);
    assert!(worker.run_until(Duration::from_secs(10), || *ticked.borrow() && !received.borrow().is_empty()));
    assert_eq!(*received.borrow(), vec![7.0]);

    assert_eq!(std::thread::current().id(), main_thread);
    listener.dispose();
    client.dispose();
}
