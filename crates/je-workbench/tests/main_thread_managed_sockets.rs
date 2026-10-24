//! Porte de `src/vs/workbench/api/test/browser/mainThreadManagedSockets.test.ts` (suíte `MainThreadManagedSockets`, subsuíte
//! `ManagedSocket`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - **`ensureNoDisposablesAreLeakedInTestSuite()`** é o `LeakGuard` criado no início de cada caso; o `ds.add(...)` é o
//!   `add`/`add_rc` do store do guarda. Os casos `async` rodam em `run_with_faked_timers` (o `timeout(1)` é o relógio
//!   virtual).
//! - **`setup`.** O `ExtHostMock` e o `RemoteSocketHalf` novos de cada caso são [`ExtHostMock`] e `RemoteSocketHalf::new()`.
//! - **`class ExtHostMock extends mock<ExtHostManagedSocketsShape>()`.** Os três métodos sobrescritos fazem o que o original
//!   faz; `$openRemoteSocket` (que o original não sobrescreve) falha com `panic!`, como o método ausente lançaria `TypeError`.
//!   O `events: any[]` é a lista de [`Evt`] (`{ socketId, data }` ou `{ socketId, event }`).
//! - **`expectEvent`.** Devolve uma promessa já resolvida quando o evento existe (o `await undefined` do original cede uma
//!   microtarefa, e é isso que deixa a retomada de `onData` rodar antes do caso seguinte escrever); quando não existe, a
//!   promessa nunca resolve e o temporizador de 1000 ms lança o erro, como no original (o ouvinte de `onDidFire` só
//!   confere e não resolve, também como no original).
//! - **`half.onClose: Emitter<SocketCloseEvent>`** é um `Emitter<Option<SocketCloseEvent>>` (ver `managed_socket.rs`).
//! - **`socket.onData(listener)`** é `socket.on_data().listen(listener)`; `d.toString()` é o `Display` do `VSBuffer`.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use std::cell::RefCell;
use std::rc::Rc;

use common::{ensure_no_disposables_are_leaked_in_test_suite, run_with_faked_timers, timeout};
use je_base::async_::disposable_timeout;
use je_base::buffer::VSBuffer;
use je_base::errors::{self, Error};
use je_base::event::Emitter;
use je_base::lifecycle::{DisposableStore, IDisposable};
use je_base::parts::ipc::net::ISocket;
use je_base::promise::Promise;
use je_platform::remote::managed_socket::{ManagedSocket, RemoteSocketHalf};
use je_workbench::api::browser::main_thread_managed_sockets::MainThreadManagedSocket;
use je_workbench::api::common::ext_host_protocol::ExtHostManagedSocketsShape;

/// Um elemento de `events`: `{ socketId, data }` ou `{ socketId, event }`.
#[derive(Clone, Debug)]
struct Evt {
    socket_id: u64,
    data: Option<String>,
    event: Option<&'static str>,
}

/// `ExtHostMock`.
struct ExtHostMock {
    on_did_fire: Rc<Emitter<()>>,
    events: Rc<RefCell<Vec<Evt>>>,
}

impl ExtHostMock {
    fn new() -> Rc<Self> {
        Rc::new(Self { on_did_fire: Emitter::new(), events: Rc::new(RefCell::new(Vec::new())) })
    }

    fn expect_event(&self, test: impl Fn(&Evt) -> bool + 'static, message: &str) -> Promise<()> {
        if self.events.borrow().iter().any(&test) {
            return Promise::resolved(());
        }

        let test = Rc::new(test);
        let d = DisposableStore::new();
        let events = self.events.clone();
        let on_did_fire = self.on_did_fire.clone();
        let message = message.to_string();
        Promise::<()>::with_executor({
            let d = d.clone();
            move |_resolve| {
                d.add(on_did_fire.event().listen({
                    let (events, test) = (events.clone(), test.clone());
                    move |_| {
                        let _found = events.borrow().iter().any(|evt| test(evt));
                    }
                }));
                d.add(disposable_timeout(
                    move || errors::throw(Error::new(format!("Expected {message} but only had {:?}", events.borrow()))),
                    1000,
                    None,
                ));
            }
        })
        .finally(move || d.dispose())
    }

    fn record(&self, evt: Evt) {
        self.events.borrow_mut().push(evt);
        self.on_did_fire.fire(());
    }
}

impl ExtHostManagedSocketsShape for ExtHostMock {
    fn rpc_open_remote_socket(&self, _socket_factory_id: u64) -> Promise<u64> {
        panic!("$openRemoteSocket is not a function")
    }

    fn rpc_remote_socket_write(&self, socket_id: u64, buffer: VSBuffer) {
        self.record(Evt { socket_id, data: Some(buffer.to_string()), event: None });
    }

    fn rpc_remote_socket_end(&self, socket_id: u64) {
        self.record(Evt { socket_id, data: None, event: Some("end") });
    }

    fn rpc_remote_socket_drain(&self, socket_id: u64) -> Promise<()> {
        self.record(Evt { socket_id, data: None, event: Some("drain") });
        Promise::resolved(())
    }
}

/// `doConnect()`: abre o socket, espera o cabeçalho de abertura e responde com o fim do cabeçalho HTTP.
async fn do_connect(ext_host: &Rc<ExtHostMock>, half: &RemoteSocketHalf, ds: &Rc<DisposableStore>) -> Rc<ManagedSocket> {
    let socket = MainThreadManagedSocket::connect(1, ext_host.clone(), "/hello", "world=true", "", half.clone());
    ext_host
        .expect_event(
            |evt| {
                evt.data.as_deref().is_some_and(|data| {
                    data.starts_with("GET ws://localhost/hello?world=true&skipWebSocketFrames=true HTTP/1.1\r\nConnection: Upgrade\r\nUpgrade: websocket\r\nSec-WebSocket-Key:")
                })
            },
            "websocket open event",
        )
        .await
        .unwrap();
    half.on_data.fire(VSBuffer::from_string("Opened successfully ;)\r\n\r\n"));
    ds.add_rc(socket.await.unwrap())
}

#[test]
fn connects() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let ds = guard.store();
    let (ext_host, half) = (ExtHostMock::new(), RemoteSocketHalf::new());
    run_with_faked_timers(async move {
        do_connect(&ext_host, &half, &ds).await;
    });
}

#[test]
fn includes_trailing_connection_data() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let ds = guard.store();
    let (ext_host, half) = (ExtHostMock::new(), RemoteSocketHalf::new());
    run_with_faked_timers(async move {
        let socket_prom = MainThreadManagedSocket::connect(1, ext_host.clone(), "/hello", "world=true", "", half.clone());
        ext_host.expect_event(|evt| evt.data.as_deref().is_some_and(|data| data.contains("GET ws://localhost")), "websocket open event").await.unwrap();
        half.on_data.fire(VSBuffer::from_string("Opened successfully ;)\r\n\r\nSome trailing data"));
        let socket = ds.add_rc(socket_prom.await.unwrap());

        let data: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(Vec::new()));
        let sink = data.clone();
        ds.add(socket.on_data().listen(move |d| sink.borrow_mut().push(d.to_string())));
        timeout(1).await.unwrap(); // allow microtasks to flush
        assert_eq!(*data.borrow(), vec!["Some trailing data".to_string()]);
    });
}

#[test]
fn round_trips_data() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let ds = guard.store();
    let (ext_host, half) = (ExtHostMock::new(), RemoteSocketHalf::new());
    run_with_faked_timers(async move {
        let socket = do_connect(&ext_host, &half, &ds).await;
        let data: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(Vec::new()));
        let sink = data.clone();
        ds.add(socket.on_data().listen(move |d| sink.borrow_mut().push(d.to_string())));

        socket.write(VSBuffer::from_string("ping"));
        ext_host.expect_event(|evt| evt.data.as_deref() == Some("ping"), "expected ping").await.unwrap();
        half.on_data.fire(VSBuffer::from_string("pong"));
        assert_eq!(*data.borrow(), vec!["pong".to_string()]);
    });
}
