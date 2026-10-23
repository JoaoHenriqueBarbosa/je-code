//! O worker como thread do SO: o transporte [`IWebWorker`] do alvo nativo, e o laço do lado do worker.
//!
//! O worker roda o seu próprio laço de eventos (o `runtime` é por thread) e recebe as mensagens por um canal
//! `mpsc` de bytes (a codificação do IPC, ver [`Message::to_bytes`]); as respostas voltam pelo canal inverso. A
//! thread principal não bloqueia: [`ThreadWebWorker::pump`] entrega ao `on_message` o que chegou, e quem hospeda
//! chama `pump` a cada volta do laço (ou [`ThreadWebWorker::run_until`], para um teste).
//!
//! Corresponde ao `initialize` e ao `bootstrapWebWorker` de `webWorkerBootstrap.ts`: [`spawn`] cria o servidor
//! dentro da thread (a fábrica do `requestHandler` roda lá, por isso é `Send`) e liga o `onmessage`.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::thread::JoinHandle;
use std::time::Duration;

use crate::errors::Error;
use crate::event::{Emitter, Event};
use crate::lifecycle::IDisposable;
use crate::runtime;
use crate::worker::web_worker::{IWebWorker, IWorkerChannelHandler, Message, WebWorkerServer};

/// O `requestHandler` do worker: criado na thread do worker, a partir do servidor.
pub type RequestHandlerFactory = Box<dyn FnOnce(&Rc<WebWorkerServer>) -> Rc<dyn IWorkerChannelHandler> + Send>;

/// O worker visto da thread principal.
pub struct ThreadWebWorker {
    id: i64,
    to_worker: RefCell<Option<Sender<Vec<u8>>>>,
    from_worker: Receiver<Vec<u8>>,
    on_message: Rc<Emitter<Message>>,
    on_error: Rc<Emitter<Error>>,
    thread: RefCell<Option<JoinHandle<()>>>,
    disposed: Cell<bool>,
}

/// `initialize`: cria o worker, uma thread do SO com o servidor e o laço de mensagens.
pub fn spawn(id: i64, factory: RequestHandlerFactory) -> Rc<ThreadWebWorker> {
    let (to_worker, worker_inbox) = mpsc::channel::<Vec<u8>>();
    let (worker_outbox, from_worker) = mpsc::channel::<Vec<u8>>();
    let thread = std::thread::Builder::new()
        .name(format!("je-web-worker-{id}"))
        .spawn(move || worker_main(worker_inbox, worker_outbox, factory))
        .expect("criar a thread do worker");
    Rc::new(ThreadWebWorker {
        id,
        to_worker: RefCell::new(Some(to_worker)),
        from_worker,
        on_message: Emitter::new(),
        on_error: Emitter::new(),
        thread: RefCell::new(Some(thread)),
        disposed: Cell::new(false),
    })
}

/// O laço da thread do worker: entrega cada mensagem ao servidor e roda o laço de eventos entre elas.
fn worker_main(inbox: Receiver<Vec<u8>>, outbox: Sender<Vec<u8>>, factory: RequestHandlerFactory) {
    let server = WebWorkerServer::new(
        move |msg: &Message| {
            let _ = outbox.send(msg.to_bytes());
        },
        factory,
    );
    loop {
        runtime::turn();
        // Dorme até a próxima mensagem ou o próximo timer do laço de eventos do worker.
        let wait = runtime::time_to_next_timer().unwrap_or(Duration::from_millis(50)).min(Duration::from_millis(50));
        match inbox.recv_timeout(wait) {
            Ok(bytes) => {
                if let Some(message) = Message::from_bytes(&bytes) {
                    server.onmessage(&message);
                }
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break,
        }
    }
    runtime::turn();
}

impl ThreadWebWorker {
    /// Entrega ao `on_message` as mensagens que o worker já mandou. Devolve quantas foram.
    pub fn pump(&self) -> usize {
        let mut delivered = 0;
        while let Ok(bytes) = self.from_worker.try_recv() {
            match Message::from_bytes(&bytes) {
                Some(message) => self.on_message.fire(message),
                None => self.on_error.fire(Error::new("mensagem do worker ilegível")),
            }
            delivered += 1;
        }
        delivered
    }

    /// Roda o laço da thread principal (mensagens do worker e timers) até `done()` ou até `timeout`; devolve se
    /// `done()` ficou verdadeiro.
    pub fn run_until(&self, timeout: Duration, mut done: impl FnMut() -> bool) -> bool {
        let deadline = std::time::Instant::now() + timeout;
        loop {
            self.pump();
            runtime::turn();
            if done() {
                return true;
            }
            if std::time::Instant::now() >= deadline {
                return false;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
    }
}

impl IWebWorker for ThreadWebWorker {
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
        if let Some(sender) = &*self.to_worker.borrow() {
            let _ = sender.send(message.to_bytes());
        }
    }
}

impl IDisposable for ThreadWebWorker {
    /// Fecha o canal (a thread do worker termina ao ver o canal fechado) e espera a thread.
    fn dispose(&self) {
        if self.disposed.replace(true) {
            return;
        }
        self.to_worker.borrow_mut().take();
        if let Some(thread) = self.thread.borrow_mut().take() {
            let _ = thread.join();
        }
        self.on_message.dispose();
        self.on_error.dispose();
    }
}
