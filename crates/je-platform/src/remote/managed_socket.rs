//! Porte de `src/vs/platform/remote/common/managedSocket.ts`: um socket cujo transporte é gerido por fora (o extension
//! host abre e escreve do outro lado), a abertura do cabeçalho de WebSocket cru e o `RemoteSocketHalf`.
//!
//! Divergências:
//! - **`abstract class ManagedSocket`.** Os métodos abstratos `write` e `closeRemote` e o `drain` sobrescrevível são o
//!   trait [`ManagedSocketBackend`] (`drain` tem o padrão `Promise.resolve()` da classe base); [`ManagedSocket`] guarda o
//!   backend e implementa [`ISocket`]. Quem estende a classe no original (`MainThreadManagedSocket`) fornece o backend.
//!   `connectManagedSocket<T extends ManagedSocket>` devolve o próprio `Rc<ManagedSocket>`.
//! - **`extends Disposable`** é um [`Disposable`] guardado em `base`.
//! - **`SocketCloseEvent`** vai como `Option<SocketCloseEvent>` (o `undefined` do `ISocket.onClose` do porte), então
//!   `RemoteSocketHalf.onClose` é um `Emitter<Option<SocketCloseEvent>>`. O `reject(err ?? new Error('socket closed'))`
//!   rejeita com o erro do evento de socket Node quando ele tem um, e com `Error('socket closed')` no resto (o original
//!   rejeita com o objeto do evento).
//! - **`onData`** é o evento que, ao receber um ouvinte com os dados pausados, agenda a retomada numa microtarefa.
//! - **`traceSocketEvent`** é o gancho sem efeito do `ISocket` (o `SocketDiagnostics` não foi portado).
//! - **`makeRawSocketHeaders`**: `Math.round(Math.random() * 256)` dentro de um `Uint8Array` dá 0 para 256, que é o
//!   truncamento do `as u8`.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use je_base::async_::async_fn;
use je_base::buffer::{VSBuffer, encode_base64};
use je_base::errors::Error;
use je_base::event::{Emitter, Event};
use je_base::lifecycle::{Disposable, DisposableStore, IDisposable};
use je_base::parts::ipc::net::{ISocket, SocketCloseEvent};
use je_base::promise::Promise;
use je_base::random::random_f64;
use je_base::runtime;

/// `makeRawSocketHeaders(path, query, deubgLabel)`.
pub fn make_raw_socket_headers(path: &str, query: &str, _debug_label: &str) -> String {
    // https://tools.ietf.org/html/rfc6455#section-4
    let mut buffer = [0u8; 16];
    for byte in &mut buffer {
        *byte = (random_f64() * 256.0).round() as u32 as u8;
    }
    let nonce = encode_base64(&VSBuffer::wrap(buffer.to_vec()), true, false);

    let headers = [
        format!("GET ws://localhost{path}?{query}&skipWebSocketFrames=true HTTP/1.1"),
        "Connection: Upgrade".to_string(),
        "Upgrade: websocket".to_string(),
        format!("Sec-WebSocket-Key: {nonce}"),
    ];

    headers.join("\r\n") + "\r\n\r\n"
}

/// `socketRawEndHeaderSequence`.
pub fn socket_raw_end_header_sequence() -> VSBuffer {
    VSBuffer::from_string("\r\n\r\n")
}

/// `RemoteSocketHalf`.
#[derive(Clone)]
pub struct RemoteSocketHalf {
    pub on_data: Rc<Emitter<VSBuffer>>,
    pub on_close: Rc<Emitter<Option<SocketCloseEvent>>>,
    pub on_end: Rc<Emitter<()>>,
}

impl RemoteSocketHalf {
    /// Três emissores novos, como o literal `{ onData: new Emitter(), onClose: new Emitter(), onEnd: new Emitter() }`.
    pub fn new() -> Self {
        Self { on_data: Emitter::new(), on_close: Emitter::new(), on_end: Emitter::new() }
    }
}

impl Default for RemoteSocketHalf {
    fn default() -> Self {
        Self::new()
    }
}

/// Os métodos que as subclasses de `ManagedSocket` fornecem: `write` e `closeRemote` (abstratos) e `drain`.
pub trait ManagedSocketBackend {
    /// Escreve no socket remoto.
    fn write(&self, buffer: VSBuffer);

    /// Ends the remote socket.
    fn close_remote(&self);

    /// Flushes data to the socket.
    fn drain(&self) -> Promise<()> {
        Promise::resolved(())
    }
}

/// `ManagedSocket`.
pub struct ManagedSocket {
    base: Rc<Disposable>,
    pausable_data_emitter: Rc<je_base::event::PauseableEmitter<VSBuffer>>,
    on_close: Event<Option<SocketCloseEvent>>,
    on_end: Event<()>,
    did_dispose_emitter: Rc<Emitter<()>>,
    ended: Cell<bool>,
    #[allow(dead_code)]
    debug_label: String,
    backend: Rc<dyn ManagedSocketBackend>,
}

impl ManagedSocket {
    /// O construtor protegido: `super(debugLabel, half)` da subclasse, que passa o seu backend.
    pub fn new(debug_label: &str, half: RemoteSocketHalf, backend: Rc<dyn ManagedSocketBackend>) -> Rc<Self> {
        let base = Disposable::new();
        let pausable_data_emitter = je_base::event::PauseableEmitter::new();
        base.register(pausable_data_emitter.clone());
        let did_dispose_emitter = Emitter::new();
        base.register(did_dispose_emitter.clone());

        base.register(half.on_data.clone());
        base.register(half.on_data.event().listen({
            let pausable = pausable_data_emitter.clone();
            move |data| pausable.fire(data.clone())
        }));

        base.register(half.on_close.clone());
        let on_close = half.on_close.event();
        base.register(half.on_end.clone());
        let on_end = half.on_end.event();

        Rc::new(Self { base, pausable_data_emitter, on_close, on_end, did_dispose_emitter, ended: Cell::new(false), debug_label: debug_label.to_string(), backend })
    }

    /// `onDidDispose`.
    pub fn on_did_dispose(&self) -> Event<()> {
        self.did_dispose_emitter.event()
    }

    /// Pauses data events until a new listener comes in onData().
    pub fn pause_data(&self) {
        self.pausable_data_emitter.pause();
    }
}

impl ISocket for ManagedSocket {
    fn on_data(&self) -> Event<VSBuffer> {
        let pausable = self.pausable_data_emitter.clone();
        Event::new(move |listener| {
            if pausable.is_paused() {
                let resumable = pausable.clone();
                runtime::queue_microtask(move || resumable.resume());
            }
            pausable.event().listen_rc(listener)
        })
    }

    fn on_close(&self) -> Event<Option<SocketCloseEvent>> {
        self.on_close.clone()
    }

    fn on_end(&self) -> Event<()> {
        self.on_end.clone()
    }

    fn write(&self, buffer: VSBuffer) {
        self.backend.write(buffer);
    }

    /// Ends the remote socket.
    fn end(&self) {
        self.ended.set(true);
        self.backend.close_remote();
    }

    fn drain(&self) -> Promise<()> {
        self.backend.drain()
    }
}

impl IDisposable for ManagedSocket {
    fn dispose(&self) {
        if !self.ended.get() {
            self.backend.close_remote();
        }

        self.did_dispose_emitter.fire(());
        self.base.dispose();
    }
}

/// `connectManagedSocket(socket, path, query, debugLabel, half)`: deve ser chamada logo depois de criar o socket, para
/// deixá-lo pronto para o fluxo de dados.
pub fn connect_managed_socket(socket: Rc<ManagedSocket>, path: &str, query: &str, debug_label: &str, half: RemoteSocketHalf) -> Promise<Rc<ManagedSocket>> {
    let headers = make_raw_socket_headers(path, query, debug_label);
    async_fn(async move {
        socket.write(VSBuffer::from_string(&headers));

        let d = DisposableStore::new();
        let (promise, resolver) = Promise::<Rc<ManagedSocket>>::new();
        let data_so_far: Rc<RefCell<Option<VSBuffer>>> = Rc::new(RefCell::new(None));
        d.add(socket.on_data().listen({
            let socket = socket.clone();
            let resolver = resolver.clone();
            move |data| {
                let combined = match data_so_far.borrow_mut().take() {
                    None => data.clone(),
                    Some(so_far) => VSBuffer::concat(&[so_far.clone(), data.clone()], Some(so_far.byte_length() + data.byte_length())),
                };
                *data_so_far.borrow_mut() = Some(combined.clone());

                let index = combined.index_of(&socket_raw_end_header_sequence(), 0);
                if index == -1 {
                    return;
                }

                resolver.resolve(socket.clone());
                // pause data events until the socket consumer is hooked up. We may
                // immediately emit remaining data, but if not there may still be
                // microtasks queued which would fire data into the abyss.
                socket.pause_data();

                let rest = combined.slice(Some(index + socket_raw_end_header_sequence().byte_length() as i64), None);
                if rest.byte_length() > 0 {
                    half.on_data.fire(rest);
                }
            }
        }));

        d.add(socket.on_close().listen({
            let resolver = resolver.clone();
            move |err| {
                resolver.reject(match err {
                    Some(SocketCloseEvent::Node { error: Some(error), .. }) => error.clone(),
                    _ => Error::new("socket closed"),
                })
            }
        }));
        d.add(socket.on_end().listen(move |_| resolver.reject(Error::new("socket ended"))));

        match promise.await {
            Ok(socket) => {
                d.dispose();
                Ok(socket)
            }
            Err(error) => {
                socket.dispose();
                d.dispose();
                Err(error)
            }
        }
    })
}
