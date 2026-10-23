//! Porte de `src/vs/platform/terminal/common/requestStore.ts`: o rastreador de requisições que têm resposta,
//! com que é simples implementar um evento que aceita uma resposta.
//!
//! Representação: `RequestStore<T, RequestArgs> extends Disposable` é o [`RequestStore`] com um `Disposable`
//! por composição. O evento `onCreateRequest` carrega `RequestArgs & { requestId: number }`, que é o
//! [`RequestEvent`] (o `requestId` e os argumentos). A função `resolve` de cada requisição pendente é o
//! [`Resolver`] da promessa devolvida. O motivo de rejeição por tempo esgotado, uma string no original, é o
//! `message` de um [`Error`]. O `@ILogService` do construtor é o [`LogServiceRef`] recebido.

use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::rc::Rc;

use je_base::async_::timeout_with_token;
use je_base::cancellation::CancellationTokenSource;
use je_base::errors::Error;
use je_base::event::{Emitter, Event};
use je_base::lifecycle::{Disposable, DisposableRef, IDisposable, dispose_all, to_disposable};
use je_base::promise::{Promise, Resolver};

use crate::log::{LogServiceRef, Logger};

/// O `RequestArgs & { requestId: number }` do evento `onCreateRequest`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RequestEvent<RequestArgs> {
    pub request_id: u64,
    pub args: RequestArgs,
}

/// A helper class to track requests that have replies. Using this it's easy to implement an event
/// that accepts a reply.
pub struct RequestStore<T: Clone + 'static, RequestArgs: 'static> {
    base: Rc<Disposable>,
    last_request_id: Cell<u64>,
    timeout: u64,
    pending_requests: RefCell<BTreeMap<u64, Resolver<T>>>,
    pending_request_disposables: Rc<RefCell<BTreeMap<u64, Vec<DisposableRef>>>>,
    on_create_request: Rc<Emitter<RequestEvent<RequestArgs>>>,
    log_service: LogServiceRef,
}

impl<T: Clone + 'static, RequestArgs: 'static> RequestStore<T, RequestArgs> {
    /// `timeout`: how long in ms to allow requests to go unanswered for, `None` will use the default
    /// (15 seconds).
    pub fn new(timeout: Option<u64>, log_service: LogServiceRef) -> Rc<Self> {
        let base = Disposable::new();
        let pending_request_disposables: Rc<RefCell<BTreeMap<u64, Vec<DisposableRef>>>> = Rc::default();
        let on_create_request: Rc<Emitter<RequestEvent<RequestArgs>>> = Emitter::new();
        base.register(on_create_request.clone());
        base.register(to_disposable({
            let pending_request_disposables = pending_request_disposables.clone();
            move || {
                let all: Vec<DisposableRef> = pending_request_disposables.borrow().values().flatten().cloned().collect();
                dispose_all(&all);
            }
        }));
        Rc::new(Self {
            base,
            last_request_id: Cell::new(0),
            timeout: timeout.unwrap_or(15000),
            pending_requests: RefCell::new(BTreeMap::new()),
            pending_request_disposables,
            on_create_request,
            log_service,
        })
    }

    /// `onCreateRequest`.
    pub fn on_create_request(&self) -> Event<RequestEvent<RequestArgs>> {
        self.on_create_request.event()
    }

    /// Creates a request.
    /// `args`: the arguments to pass to the `onCreateRequest` event.
    pub fn create_request(&self, args: RequestArgs) -> Promise<T> {
        Promise::with_executor(|resolver| {
            let request_id = self.last_request_id.get() + 1;
            self.last_request_id.set(request_id);
            self.pending_requests.borrow_mut().insert(request_id, resolver.clone());
            self.on_create_request.fire(RequestEvent { request_id, args });
            let token_source = CancellationTokenSource::new(None);
            let timeout = self.timeout;
            timeout_with_token(timeout, &token_source.token()).then(move |()| resolver.reject(Error::new(format!("Request {request_id} timed out ({timeout}ms)"))));
            self.pending_request_disposables.borrow_mut().insert(request_id, vec![to_disposable(move || token_source.cancel())]);
        })
    }

    /// Accept a reply to a request.
    /// `request_id`: the request ID originating from the `onCreateRequest` event.
    /// `data`: the reply data.
    pub fn accept_reply(&self, request_id: u64, data: T) {
        let resolve_request = self.pending_requests.borrow_mut().remove(&request_id);
        if let Some(resolve_request) = resolve_request {
            let disposables = self.pending_request_disposables.borrow().get(&request_id).cloned().unwrap_or_default();
            dispose_all(&disposables);
            self.pending_request_disposables.borrow_mut().remove(&request_id);
            resolve_request.resolve(data);
        } else {
            self.log_service.warn(&format!("RequestStore#acceptReply was called without receiving a matching request {request_id}"));
        }
    }
}

impl<T: Clone + 'static, RequestArgs: 'static> IDisposable for RequestStore<T, RequestArgs> {
    fn dispose(&self) {
        self.base.dispose();
    }
}
