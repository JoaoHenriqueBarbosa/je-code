//! Conferência byte a byte do IPC contra o `ipc.ts`, o `ipc.net.ts` e o `webSocketFraming.ts` reais do VS Code
//! rodando no bun. Não é porte de teste do VS Code.
//!
//! O arquivo `golden/ipc.txt` (linhas `nome<TAB>hex`) é gerado por `scripts/gen_ipc_golden.sh`, que roda os
//! módulos de `ref/vscode` e grava os bytes que eles produzem: a serialização de valores, as mensagens do
//! `ChannelClient` e do `ChannelServer`, o primeiro envio do `IPCClient`, os quadros do `Protocol` e do
//! `PersistentProtocol` e os quadros de WebSocket. Os valores em Rust são montados com os mesmos dados, e os
//! bytes gerados pelo Node são lidos de volta pelo porte (ida e volta nos dois sentidos).

mod common;

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use common::{run_with_faked_timers, timeout};
use je_base::buffer::VSBuffer;
use je_base::cancellation::CancellationToken;
use je_base::errors::Error;
use je_base::event::{Emitter, Event};
use je_base::lifecycle::{DisposableRef, IDisposable};
use je_base::marshalling::Marshalled;
use je_base::parts::ipc::net::{ISocket, PersistentProtocol, PersistentProtocolOptions, Protocol, SocketCloseEvent};
use je_base::parts::ipc::web_socket_framing::{IWebSocketFrameOptions, WebSocketOpcode, encode_web_socket_frame};
use je_base::parts::ipc::{BufferReader, BufferWriter, ChannelClient, ChannelServer, IMessagePassingProtocol, IPCClient, IServerChannel, deserialize, error_obj, serialize};
use je_base::promise::Promise;

const GOLDEN: &str = include_str!("golden/ipc.txt");

fn golden() -> HashMap<&'static str, Vec<u8>> {
    GOLDEN
        .lines()
        .map(|line| {
            let (name, hex) = line.split_once('\t').expect("linha nome<TAB>hex");
            let bytes = (0..hex.len() / 2).map(|i| u8::from_str_radix(&hex[2 * i..2 * i + 2], 16).unwrap()).collect();
            (name, bytes)
        })
        .collect()
}

fn text(s: &str) -> Marshalled {
    Marshalled::String(s.into())
}

fn num(n: f64) -> Marshalled {
    Marshalled::Number(n)
}

fn obj(entries: Vec<(&str, Marshalled)>) -> Marshalled {
    Marshalled::Object(entries.into_iter().map(|(k, v)| (k.into(), v)).collect())
}

fn serialized(values: &[Marshalled]) -> Vec<u8> {
    let mut writer = BufferWriter::new();
    for value in values {
        serialize(&mut writer, value);
    }
    writer.buffer().to_vec()
}

#[test]
fn serialize_matches_the_node_bytes_and_reads_them_back() {
    let golden = golden();
    let long = "x".repeat(300);
    let values: Vec<(&str, Marshalled)> = vec![
        ("undefined", Marshalled::Undefined),
        ("empty_string", text("")),
        ("string", text("h\u{e9}llo \u{1F600}")),
        ("zero", num(0.0)),
        ("one", num(1.0)),
        ("minus_one", num(-1.0)),
        ("n12345", num(12345.0)),
        ("minus_12345", num(-12345.0)),
        ("int_max", num(2_147_483_647.0)),
        ("int_min", num(-2_147_483_648.0)),
        ("beyond_int", num(2_147_483_648.0)),
        ("float", num(42.6)),
        ("big", num(123_412_341_234.0)),
        ("true", Marshalled::Bool(true)),
        ("false", Marshalled::Bool(false)),
        ("null", Marshalled::Null),
        ("empty_array", Marshalled::Array(vec![])),
        ("nested_array", Marshalled::Array(vec![num(1.0), text("a"), Marshalled::Array(vec![Marshalled::Undefined, Marshalled::Null])])),
        ("object", obj(vec![("a", num(1.0)), ("b", Marshalled::Array(vec![num(1.0), num(2.0)])), ("c", Marshalled::Undefined), ("d", text("x"))])),
        ("nan", num(f64::NAN)),
        ("vsbuffer", Marshalled::Buffer(VSBuffer::wrap(vec![1, 2, 3]))),
        ("bytes", Marshalled::Bytes(vec![9, 8])),
        ("buffers_in_array", Marshalled::Array(vec![Marshalled::Buffer(VSBuffer::alloc(2)), Marshalled::Buffer(VSBuffer::alloc(3))])),
        ("long_string", text(&long)),
        ("object_with_date", obj(vec![("d", Marshalled::Date(0.0))])),
    ];
    for (name, value) in &values {
        let expected = &golden[format!("serialize.{name}").as_str()];
        assert_eq!(&serialized(std::slice::from_ref(value)), expected, "serialize.{name}");
        // A leitura dos bytes do Node, escrita de novo, dá os mesmos bytes.
        let read = deserialize(&mut BufferReader::new(VSBuffer::wrap(expected.clone())));
        assert_eq!(&serialized(&[read]), expected, "ida e volta de serialize.{name}");
    }
    // O tipo 2 (um `Buffer` do Node) é lido como `Uint8Array`.
    let native = deserialize(&mut BufferReader::new(VSBuffer::wrap(vec![2, 2, 9, 8])));
    assert!(matches!(native, Marshalled::Bytes(ref bytes) if bytes == &[9, 8]));
    // Um RegExp vira `{}` no `JSON.stringify` do protocolo (o `source` não é enumerável).
    let regexp = obj(vec![("r", Marshalled::RegExp(Rc::new(je_base::marshalling::RegExpValue::new(&"a+".into(), &"gi".into()))))]);
    assert_eq!(serialized(&[regexp]), golden["serialize.object_with_regexp"]);
}

/// Um protocolo que só guarda o que foi enviado e deixa o teste empurrar mensagens recebidas.
struct Stub {
    sent: RefCell<Vec<Vec<u8>>>,
    incoming: Rc<Emitter<VSBuffer>>,
}

impl Stub {
    fn new() -> Rc<Self> {
        Rc::new(Self { sent: RefCell::default(), incoming: Emitter::new() })
    }

    fn receive(&self, bytes: &[u8]) {
        self.incoming.fire(VSBuffer::wrap(bytes.to_vec()));
    }
}

impl IMessagePassingProtocol for Stub {
    fn send(&self, buffer: VSBuffer) {
        self.sent.borrow_mut().push(buffer.to_vec());
    }

    fn on_message(&self) -> Event<VSBuffer> {
        self.incoming.event()
    }
}

#[test]
fn channel_client_sends_the_node_request_bytes() {
    let golden = golden();
    run_with_faked_timers(async move {
        let stub = Stub::new();
        let client = ChannelClient::new(stub.clone(), None);
        stub.receive(&golden["client.init"]);
        let channel = client.get_channel("testchannel");
        let _ = channel.call("cmd", obj(vec![("x", num(1.0))]), CancellationToken::None);
        let _ = channel.call("other", Marshalled::Array(vec![text("a"), num(2.0)]), CancellationToken::None);
        let listener = channel.listen("onSomething", text("arg")).listen(|_| {});

        let sent = stub.sent.borrow().clone();
        assert_eq!(sent[0], golden["client.call1"]);
        assert_eq!(sent[1], golden["client.call2"]);
        assert_eq!(sent[2], golden["client.listen"]);
        listener.dispose();
        client.dispose();
    });
}

struct ServerChannel {
    events: Rc<Emitter<Marshalled>>,
}

impl IServerChannel for ServerChannel {
    fn call(&self, _ctx: &Marshalled, command: &str, arg: Marshalled, _token: CancellationToken) -> Promise<Marshalled> {
        match command {
            "ok" => Promise::resolved(obj(vec![("echoed", arg)])),
            "fail" => Promise::rejected(Error::new("boom").with_stack("Error: boom\n    at here (file.js:1:1)\n    at there (file.js:2:2)")),
            "failobj" => Promise::rejected(error_obj(&obj(vec![("code", num(7.0))]))),
            _ => Promise::rejected(Error::new("nope")),
        }
    }

    fn listen(&self, _ctx: &Marshalled, _event: &str, _arg: Marshalled) -> Event<Marshalled> {
        self.events.event()
    }
}

#[test]
fn channel_server_answers_the_node_requests_with_the_node_bytes() {
    let golden = golden();
    run_with_faked_timers(async move {
        let stub = Stub::new();
        let server = ChannelServer::new(stub.clone(), text("ctx"), None, None);
        assert_eq!(stub.sent.borrow()[0], golden["server.init"]);
        let events = Emitter::<Marshalled>::new();
        server.register_channel("chan", Rc::new(ServerChannel { events: events.clone() }));
        timeout(5).await.unwrap();

        stub.receive(&serialized(&[Marshalled::Array(vec![num(100.0), num(1.0), text("chan"), text("ok")]), text("hi")]));
        stub.receive(&serialized(&[Marshalled::Array(vec![num(100.0), num(2.0), text("chan"), text("fail")]), Marshalled::Undefined]));
        stub.receive(&serialized(&[Marshalled::Array(vec![num(100.0), num(3.0), text("chan"), text("failobj")]), Marshalled::Undefined]));
        stub.receive(&serialized(&[Marshalled::Array(vec![num(102.0), num(4.0), text("chan"), text("onEv")]), Marshalled::Undefined]));
        events.fire(text("fired"));
        timeout(5).await.unwrap();

        let sent = stub.sent.borrow().clone();
        for (index, bytes) in sent.iter().skip(1).enumerate() {
            assert_eq!(bytes, &golden[format!("server.response.{index}").as_str()], "server.response.{index}");
        }
        assert_eq!(sent.len() - 1, 4);
        server.dispose();
        events.dispose();
    });
}

#[test]
fn ipc_client_sends_its_context_first() {
    let stub = Stub::new();
    let client = IPCClient::new(stub.clone(), text("client-context"), None);
    assert_eq!(stub.sent.borrow()[0], golden()["ipcclient.ctx"]);
    client.dispose();
}

/// Um `ISocket` que guarda o que o protocolo escreve.
struct FakeSocket {
    written: RefCell<Vec<Vec<u8>>>,
    data: Rc<Emitter<VSBuffer>>,
    close: Rc<Emitter<Option<SocketCloseEvent>>>,
    end: Rc<Emitter<()>>,
}

impl FakeSocket {
    fn new() -> Rc<Self> {
        Rc::new(Self { written: RefCell::default(), data: Emitter::new(), close: Emitter::new(), end: Emitter::new() })
    }

    fn written(&self) -> Vec<u8> {
        self.written.borrow().concat()
    }
}

impl IDisposable for FakeSocket {
    fn dispose(&self) {}
}

impl ISocket for FakeSocket {
    fn on_data(&self) -> Event<VSBuffer> {
        self.data.event()
    }

    fn on_close(&self) -> Event<Option<SocketCloseEvent>> {
        self.close.event()
    }

    fn on_end(&self) -> Event<()> {
        self.end.event()
    }

    fn write(&self, buffer: VSBuffer) {
        self.written.borrow_mut().push(buffer.to_vec());
    }

    fn end(&self) {}

    fn drain(&self) -> Promise<()> {
        Promise::resolved(())
    }
}

#[test]
fn protocol_writes_the_node_frames() {
    let golden = golden();
    run_with_faked_timers(async move {
        let socket = FakeSocket::new();
        let protocol = Protocol::new(socket.clone());
        protocol.send(VSBuffer::from_string("abc"));
        protocol.send(VSBuffer::from_string("defg"));
        timeout(5).await.unwrap();
        assert_eq!(socket.written(), golden["protocol.written"]);
        // E lê de volta os quadros do Node.
        let received: Rc<RefCell<Vec<String>>> = Rc::default();
        let sink = received.clone();
        let listener: DisposableRef = protocol.on_message().listen(move |message| sink.borrow_mut().push(String::from_utf8(message.to_vec()).unwrap()));
        socket.data.fire(VSBuffer::wrap(golden["protocol.written"].clone()));
        assert_eq!(*received.borrow(), vec!["abc".to_owned(), "defg".to_owned()]);
        listener.dispose();
        je_base::parts::ipc::net::INetProtocol::dispose(&*protocol);
    });
}

/// O `LoadEstimator` padrão arma um intervalo de 1 s que nunca termina, e o relógio virtual esperaria por ele
/// para sempre; o teste passa um estimador sem carga (como os testes do original).
struct NoLoad;

impl je_base::parts::ipc::net::ILoadEstimator for NoLoad {
    fn has_high_load(&self) -> bool {
        false
    }
}

#[test]
fn persistent_protocol_writes_the_node_frames() {
    let golden = golden();
    run_with_faked_timers(async move {
        let socket = FakeSocket::new();
        let protocol = PersistentProtocol::new(PersistentProtocolOptions { socket: socket.clone(), initial_chunk: None, load_estimator: Some(Rc::new(NoLoad)), send_keep_alive: Some(false) });
        protocol.send(VSBuffer::from_string("one"));
        protocol.send(VSBuffer::from_string("two"));
        protocol.send_control(VSBuffer::from_string("ctl"));
        protocol.send_pause();
        protocol.send_resume();
        protocol.send_disconnect();
        timeout(5).await.unwrap();
        assert_eq!(socket.written(), golden["persistent.written"]);
        protocol.dispose();
    });
}

#[test]
fn web_socket_frames_match_the_node_bytes() {
    let golden = golden();
    for (name, length, fill) in [("empty", 0usize, 0u8), ("small", 3, 0), ("len125", 125, 7), ("len126", 126, 8), ("len65535", 65535, 9), ("len65536", 65536, 10)] {
        let payload = if name == "small" { VSBuffer::wrap(vec![1, 2, 3]) } else { VSBuffer::wrap(vec![fill; length]) };
        let unmasked = encode_web_socket_frame(&payload, IWebSocketFrameOptions::new(WebSocketOpcode::Binary)).unwrap();
        assert_eq!(unmasked.to_vec(), golden[format!("ws.unmasked.{name}").as_str()], "ws.unmasked.{name}");
        let masked = encode_web_socket_frame(&payload, IWebSocketFrameOptions { mask: Some(0x1234_5678), ..IWebSocketFrameOptions::new(WebSocketOpcode::Binary) }).unwrap();
        assert_eq!(masked.to_vec(), golden[format!("ws.masked.{name}").as_str()], "ws.masked.{name}");
    }
    let compressed = encode_web_socket_frame(&VSBuffer::from_string("hello"), IWebSocketFrameOptions { r#final: Some(false), compressed: Some(true), ..IWebSocketFrameOptions::new(WebSocketOpcode::Text) }).unwrap();
    assert_eq!(compressed.to_vec(), golden["ws.text.compressed.nonfinal"]);
    let ping = encode_web_socket_frame(&VSBuffer::from_string("p"), IWebSocketFrameOptions { mask: Some(0), ..IWebSocketFrameOptions::new(WebSocketOpcode::Ping) }).unwrap();
    assert_eq!(ping.to_vec(), golden["ws.ping"]);
}
