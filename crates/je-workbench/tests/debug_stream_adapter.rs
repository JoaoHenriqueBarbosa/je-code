//! Porte de `src/vs/workbench/contrib/debug/test/node/streamDebugAdapter.test.ts` (suíte `Debug - StreamDebugAdapter`), caso a
//! caso e na mesma ordem.
//!
//! Notas de tradução (divergência de E/S):
//! - O original sobe um `net.Server` de verdade (um pipe nomeado / socket de domínio Unix e uma porta TCP livre) e conecta o
//!   `NamedPipeDebugAdapter` e o `SocketDebugAdapter`. O porte não tem o `net` do Node, e o `INodeStream` é a fronteira com
//!   o sistema; aqui o servidor é um [`INodeStream`] em memória que fala o mesmo protocolo (lê o quadro `Content-Length`,
//!   separa por `\r\n` e responde com `success: true` quando o `adapterID` é `test`), então os casos exercitam o
//!   `StreamDebugAdapter` de verdade (cabeçalho, JSON, `startSession`, `sendRequest`, `stopSession`) e a escolha do
//!   endereço (host padrão `127.0.0.1`, porta e caminho do pipe), mas não o transporte do SO. `findFreePort` e
//!   `crypto.randomBytes` do original só serviam para montar o endereço e ficam fora.
//! - `ensureNoDisposablesAreLeakedInTestSuite()` vira o `LeakGuard` de cada caso; o laço roda no relógio virtual.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use common::{ensure_no_disposables_are_leaked_in_test_suite, run_with_faked_timers};
use je_base::buffer::VSBuffer;
use je_base::errors::Error;
use je_base::event::{Emitter, Event};
use je_base::lifecycle::IDisposable;
use je_base::parts::ipc::node_net::INodeStream;
use je_base::promise::Promise;
use je_base::runtime;
use je_workbench::contrib::debug::abstract_debug_adapter::AbstractDebugAdapter;
use je_workbench::contrib::debug::debug::{IDebugAdapterNamedPipeServer, IDebugAdapterServer};
use je_workbench::contrib::debug::debug_adapter::NetworkDebugAdapter;
use je_workbench::contrib::debug::debug_protocol::Response;
use serde_json::{Value, json};

/// O servidor do teste (`serverConnection(socket)`) no lugar do socket: o que o adaptador escreve é lido como o servidor
/// leria, e a resposta volta pelo evento `data`.
struct ServerStream {
    on_data: Rc<Emitter<VSBuffer>>,
    on_close: Rc<Emitter<bool>>,
    on_end: Rc<Emitter<()>>,
    on_error: Rc<Emitter<Error>>,
    on_timeout: Rc<Emitter<()>>,
    on_drain: Rc<Emitter<()>>,
    destroyed: Cell<bool>,
    ended: Cell<bool>,
}

impl ServerStream {
    fn new() -> Rc<Self> {
        Rc::new(Self {
            on_data: Emitter::new(),
            on_close: Emitter::new(),
            on_end: Emitter::new(),
            on_error: Emitter::new(),
            on_timeout: Emitter::new(),
            on_drain: Emitter::new(),
            destroyed: Cell::new(false),
            ended: Cell::new(false),
        })
    }
}

impl INodeStream for ServerStream {
    fn on_data(&self) -> Event<VSBuffer> {
        self.on_data.event()
    }

    fn on_close(&self) -> Event<bool> {
        self.on_close.event()
    }

    fn on_end(&self) -> Event<()> {
        self.on_end.event()
    }

    fn on_error(&self) -> Event<Error> {
        self.on_error.event()
    }

    fn on_timeout(&self) -> Event<()> {
        self.on_timeout.event()
    }

    fn on_drain(&self) -> Event<()> {
        self.on_drain.event()
    }

    fn write(&self, buffer: VSBuffer) -> Result<(), Error> {
        let text = String::from_utf8(buffer.to_vec()).expect("utf-8");
        let str_ = text.split("\r\n").nth(2).expect("o corpo").to_owned();
        let request: Value = serde_json::from_str(&str_).expect("json");
        let mut response = json!({ "seq": request["seq"], "request_seq": request["seq"], "type": "response", "command": request["command"] });
        if request["arguments"]["adapterID"] == "test" {
            response["success"] = json!(true);
        } else {
            response["success"] = json!(false);
            response["message"] = json!("failed");
        }

        let response_payload = response.to_string();
        let framed = format!("Content-Length: {}\r\n\r\n{}", response_payload.len(), response_payload);
        self.on_data.fire(VSBuffer::wrap(framed.into_bytes()));
        Ok(())
    }

    fn end(&self) {
        self.ended.set(true);
    }

    fn destroy(&self) {
        self.destroyed.set(true);
    }

    fn destroyed(&self) -> bool {
        self.destroyed.get()
    }

    fn buffer_size(&self) -> usize {
        0
    }
}

/// `sendInitializeRequest(debugAdapter)`.
fn send_initialize_request(debug_adapter: &Rc<NetworkDebugAdapter>) -> Promise<Response> {
    let (promise, resolver) = Promise::<Response>::new();
    debug_adapter.send_request("initialize", Some(&json!({ "adapterID": "test" })), Rc::new(move |result: &Response| resolver.resolve(result.clone())), Some(3000));
    promise
}

async fn check_initialize(debug_adapter: Rc<NetworkDebugAdapter>, server: Rc<ServerStream>) {
    let outcome = async {
        debug_adapter.start_session().await.unwrap();
        let response = send_initialize_request(&debug_adapter).await.unwrap();
        assert_eq!(response.command, "initialize");
        assert_eq!(response.request_seq, 1);
        assert!(response.success, "{:?}", response.message);
    }
    .await;
    debug_adapter.stop_session().await.unwrap();
    assert!(server.ended.get());
    debug_adapter.dispose();
    outcome
}

#[test]
fn stream_debug_adapter_named_pipe_debug_adapter_can_initialize_a_connection() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    run_with_faked_timers(async move {
        let server = ServerStream::new();
        let opened_path: Rc<RefCell<Option<String>>> = Rc::default();
        let open_server = server.clone();
        let open_path = opened_path.clone();
        let debug_adapter = NetworkDebugAdapter::named_pipe(
            IDebugAdapterNamedPipeServer { path: "/tmp/test-pipe".to_owned() },
            Rc::new(move |path, on_connected| {
                *open_path.borrow_mut() = Some(path.to_owned());
                runtime::set_timeout(on_connected, 0);
                open_server.clone() as Rc<dyn INodeStream>
            }),
        );
        check_initialize(debug_adapter, server).await;
        assert_eq!(opened_path.borrow().as_deref(), Some("/tmp/test-pipe"));
        drop(guard);
    });
}

#[test]
fn stream_debug_adapter_socket_debug_adapter_can_initialize_a_connection() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    run_with_faked_timers(async move {
        let server = ServerStream::new();
        let opened: Rc<RefCell<Option<(String, u16)>>> = Rc::default();
        let open_server = server.clone();
        let open_address = opened.clone();
        let debug_adapter = NetworkDebugAdapter::socket(
            IDebugAdapterServer { port: 8087.0, host: None },
            Rc::new(move |host, port, on_connected| {
                *open_address.borrow_mut() = Some((host.to_owned(), port));
                runtime::set_timeout(on_connected, 0);
                open_server.clone() as Rc<dyn INodeStream>
            }),
        );
        check_initialize(debug_adapter, server).await;
        assert_eq!(*opened.borrow(), Some(("127.0.0.1".to_owned(), 8087)));
        drop(guard);
    });
}
