//! Porte de `src/vs/base/common/cancellation.ts`: `CancellationToken`, `CancellationTokenSource`,
//! `CancellationTokenPool` e as pontes com `AbortSignal`.
//!
//! O `AbortController`/`AbortSignal` do DOM não existe em Rust; [`AbortController`] e [`AbortSignal`]
//! são o mínimo que o `cancellation.ts` usa (`aborted`, `reason`, `abort`, o evento `abort`).
//!
//! `CancellationToken` é uma interface no original, comparada por identidade (`token === None`); aqui é
//! um `enum` com as variantes `None` e `Cancelled` (os dois singletons), o token mutável e um token
//! qualquer que implemente [`ICancellationToken`].

use std::any::Any;
use std::cell::{Cell, RefCell};
use std::rc::Rc;

use crate::errors::Error;
use crate::event::{DomEventEmitter, Emitter, Event, Listener, from_dom_event_emitter};
use crate::lifecycle::{DisposableRef, DisposableStore, IDisposable, plain_disposable};
use crate::runtime;

/// O que um token tem, para quem quiser um token próprio (o `CancellationToken` estrutural do original).
pub trait ICancellationToken {
    /// Sinaliza se o cancelamento foi pedido.
    fn is_cancellation_requested(&self) -> bool;

    /// Dispara quando o cancelamento é pedido. Só dispara uma vez, porque só se cancela uma vez; quem se
    /// inscreve depois é chamado (no próximo tick do loop) também uma vez.
    fn on_cancellation_requested(&self) -> Event<()>;
}

/// `shortcutEvent`: o evento de quem se inscreve depois do cancelamento, entregue no próximo tick.
fn shortcut_event() -> Event<()> {
    Event::new(|callback: Listener<()>| {
        let handle = runtime::set_timeout(move || callback(&()), 0);
        plain_disposable(move || runtime::clear_timeout(handle))
    })
}

/// O token da fonte que ainda pode ser cancelada (`MutableToken`, privado no original).
#[derive(Default)]
pub struct MutableToken {
    is_cancelled: Cell<bool>,
    emitter: RefCell<Option<Rc<Emitter<()>>>>,
}

impl MutableToken {
    pub fn cancel(&self) {
        if self.is_cancelled.replace(true) {
            return;
        }
        let emitter = self.emitter.borrow().clone();
        if let Some(emitter) = emitter {
            emitter.fire(());
            self.dispose();
        }
    }
}

impl ICancellationToken for MutableToken {
    fn is_cancellation_requested(&self) -> bool {
        self.is_cancelled.get()
    }

    fn on_cancellation_requested(&self) -> Event<()> {
        if self.is_cancelled.get() {
            return shortcut_event();
        }
        self.emitter.borrow_mut().get_or_insert_with(Emitter::new).event()
    }
}

impl IDisposable for MutableToken {
    fn dispose(&self) {
        let emitter = self.emitter.borrow_mut().take();
        if let Some(emitter) = emitter {
            emitter.dispose();
        }
    }
}

/// `CancellationToken`.
#[derive(Clone)]
pub enum CancellationToken {
    /// `CancellationToken.None`: nunca cancela.
    None,
    /// `CancellationToken.Cancelled`: já nasce cancelado.
    Cancelled,
    Mutable(Rc<MutableToken>),
    Custom(Rc<dyn ICancellationToken>),
}

impl CancellationToken {
    /// `token === other`.
    pub fn is_same(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::None, Self::None) | (Self::Cancelled, Self::Cancelled) => true,
            (Self::Mutable(a), Self::Mutable(b)) => Rc::ptr_eq(a, b),
            (Self::Custom(a), Self::Custom(b)) => Rc::ptr_eq(a, b),
            _ => false,
        }
    }

    pub fn is_cancellation_requested(&self) -> bool {
        match self {
            Self::None => false,
            Self::Cancelled => true,
            Self::Mutable(token) => token.is_cancellation_requested(),
            Self::Custom(token) => token.is_cancellation_requested(),
        }
    }

    /// `onCancellationRequested`: o evento (um getter no original, que troca conforme o estado).
    pub fn on_cancellation_requested(&self) -> Event<()> {
        match self {
            Self::None => Event::none(),
            Self::Cancelled => shortcut_event(),
            Self::Mutable(token) => token.on_cancellation_requested(),
            Self::Custom(token) => token.on_cancellation_requested(),
        }
    }

    /// `CancellationToken.fromAbortSignal(signal, lifetime)`: um token preso à vida do `AbortSignal`.
    pub fn from_abort_signal(signal: &AbortSignal, lifetime: &DisposableStore) -> Self {
        if signal.aborted() {
            return Self::Cancelled;
        }
        let token = lifetime.add_rc(Rc::new(MutableToken::default()));
        let target: Rc<dyn DomEventEmitter<()>> = Rc::new(signal.clone());
        let abort = from_dom_event_emitter(target, "abort", |_| ());
        let cancel = token.clone();
        lifetime.add(abort.once().listen(move |_| cancel.cancel()));
        Self::Mutable(token)
    }

    /// `CancellationToken.toAbortSignal(token, lifetime)`: um sinal que aborta com `CancellationError`
    /// quando o token é cancelado.
    pub fn to_abort_signal(token: &Self, lifetime: &DisposableStore) -> AbortSignal {
        if matches!(token, Self::None) {
            return AbortController::new().signal();
        }
        if token.is_cancellation_requested() {
            return AbortSignal::abort(Some(Error::cancellation()));
        }
        let controller = AbortController::new();
        let signal = controller.signal();
        lifetime.add(token.on_cancellation_requested().listen(move |_| controller.abort(Some(Error::cancellation()))));
        signal
    }
}

/// `CancellationToken.isCancellationToken(thing)`: só reconhece o tipo `CancellationToken`; o duck typing
/// do original (qualquer objeto com `isCancellationRequested` e `onCancellationRequested`) é o trait
/// [`ICancellationToken`] em tempo de compilação.
pub fn is_cancellation_token(thing: &dyn Any) -> bool {
    thing.is::<CancellationToken>()
}

#[derive(Default)]
struct SourceInner {
    token: RefCell<Option<CancellationToken>>,
    parent_listener: RefCell<Option<DisposableRef>>,
}

/// `CancellationTokenSource`.
#[derive(Clone, Default)]
pub struct CancellationTokenSource(Rc<SourceInner>);

impl CancellationTokenSource {
    /// `new CancellationTokenSource(parent?)`: cancelar o pai cancela esta fonte.
    pub fn new(parent: Option<&CancellationToken>) -> Self {
        let source = Self::default();
        if let Some(parent) = parent {
            let this = source.clone();
            *source.0.parent_listener.borrow_mut() = Some(parent.on_cancellation_requested().listen(move |_| this.cancel()));
        }
        source
    }

    /// `get token()`: criado só quando alguém pede.
    pub fn token(&self) -> CancellationToken {
        self.0.token.borrow_mut().get_or_insert_with(|| CancellationToken::Mutable(Rc::new(MutableToken::default()))).clone()
    }

    pub fn cancel(&self) {
        let token = self.0.token.borrow().clone();
        match token {
            // Cancelar antes de alguém pedir o token devolve o token cancelado padrão.
            None => *self.0.token.borrow_mut() = Some(CancellationToken::Cancelled),
            Some(CancellationToken::Mutable(token)) => token.cancel(),
            Some(_) => {}
        }
    }

    /// `dispose(cancel)`.
    pub fn dispose_with(&self, cancel: bool) {
        if cancel {
            self.cancel();
        }
        let parent_listener = self.0.parent_listener.borrow_mut().take();
        if let Some(parent_listener) = parent_listener {
            parent_listener.dispose();
        }
        let token = self.0.token.borrow().clone();
        match token {
            // Garante um token vazio se não havia nenhum.
            None => *self.0.token.borrow_mut() = Some(CancellationToken::None),
            Some(CancellationToken::Mutable(token)) => token.dispose(),
            Some(_) => {}
        }
    }
}

impl IDisposable for CancellationTokenSource {
    fn dispose(&self) {
        self.dispose_with(false);
    }
}

/// `cancelOnDispose(store)`: um token que cancela quando o store é descartado.
pub fn cancel_on_dispose(store: &DisposableStore) -> CancellationToken {
    let source = CancellationTokenSource::new(None);
    let to_cancel = source.clone();
    store.add(plain_disposable(move || to_cancel.cancel()));
    source.token()
}

/// `abortOnDispose(store)`: um sinal que aborta com `CancellationError` quando o store é descartado.
pub fn abort_on_dispose(store: &DisposableStore) -> AbortSignal {
    let controller = AbortController::new();
    let signal = controller.signal();
    store.add(plain_disposable(move || controller.abort(Some(Error::cancellation()))));
    signal
}

struct PoolInner {
    source: CancellationTokenSource,
    listeners: Rc<DisposableStore>,
    total: Cell<usize>,
    cancelled: Cell<usize>,
    is_done: Cell<bool>,
}

impl PoolInner {
    fn check(&self) {
        if !self.is_done.get() && self.total.get() > 0 && self.total.get() == self.cancelled.get() {
            self.is_done.set(true);
            self.listeners.dispose();
            self.source.cancel();
        }
    }
}

/// `CancellationTokenPool`: agrega tokens. O token do pool só cancela depois que todos os tokens
/// adicionados cancelarem; adicionar depois de cancelado não faz nada.
pub struct CancellationTokenPool(Rc<PoolInner>);

impl CancellationTokenPool {
    pub fn new() -> Self {
        Self(Rc::new(PoolInner {
            source: CancellationTokenSource::new(None),
            listeners: DisposableStore::new(),
            total: Cell::new(0),
            cancelled: Cell::new(0),
            is_done: Cell::new(false),
        }))
    }

    pub fn token(&self) -> CancellationToken {
        self.0.source.token()
    }

    /// `add(token)`: um token já cancelado conta na hora.
    pub fn add(&self, token: &CancellationToken) {
        if self.0.is_done.get() {
            return;
        }
        self.0.total.set(self.0.total.get() + 1);

        if token.is_cancellation_requested() {
            self.0.cancelled.set(self.0.cancelled.get() + 1);
            self.0.check();
            return;
        }

        let subscription: Rc<RefCell<Option<DisposableRef>>> = Rc::default();
        let (pool, own) = (Rc::downgrade(&self.0), subscription.clone());
        let d = token.on_cancellation_requested().listen(move |_| {
            let subscription = own.borrow().clone();
            if let Some(subscription) = subscription {
                subscription.dispose();
            }
            if let Some(pool) = pool.upgrade() {
                pool.cancelled.set(pool.cancelled.get() + 1);
                pool.check();
            }
        });
        *subscription.borrow_mut() = Some(d.clone());
        self.0.listeners.add(d);
    }
}

impl Default for CancellationTokenPool {
    fn default() -> Self {
        Self::new()
    }
}

impl IDisposable for CancellationTokenPool {
    fn dispose(&self) {
        self.0.listeners.dispose();
        self.0.source.dispose();
    }
}

#[derive(Default)]
struct AbortState {
    aborted: Cell<bool>,
    reason: RefCell<Option<Error>>,
    listeners: RefCell<Vec<Listener<()>>>,
}

/// `AbortSignal`.
#[derive(Clone, Default)]
pub struct AbortSignal(Rc<AbortState>);

impl AbortSignal {
    /// `AbortSignal.abort(reason)`: um sinal que já nasce abortado.
    pub fn abort(reason: Option<Error>) -> Self {
        let signal = Self::default();
        signal.abort_with(reason);
        signal
    }

    pub fn aborted(&self) -> bool {
        self.0.aborted.get()
    }

    /// `signal.reason`.
    pub fn reason(&self) -> Option<Error> {
        self.0.reason.borrow().clone()
    }

    fn abort_with(&self, reason: Option<Error>) {
        if self.0.aborted.replace(true) {
            return;
        }
        *self.0.reason.borrow_mut() = Some(reason.unwrap_or_else(|| Error::named("AbortError", "This operation was aborted")));
        let listeners = self.0.listeners.borrow().clone();
        for listener in listeners {
            listener(&());
        }
    }
}

impl DomEventEmitter<()> for AbortSignal {
    fn add_event_listener(&self, event: &str, listener: Listener<()>) {
        if event == "abort" {
            self.0.listeners.borrow_mut().push(listener);
        }
    }

    fn remove_event_listener(&self, event: &str, listener: &Listener<()>) {
        if event == "abort" {
            self.0.listeners.borrow_mut().retain(|l| !Rc::ptr_eq(l, listener));
        }
    }
}

/// `AbortController`.
#[derive(Clone, Default)]
pub struct AbortController(AbortSignal);

impl AbortController {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn signal(&self) -> AbortSignal {
        self.0.clone()
    }

    /// `abort(reason)`: sem motivo, o sinal aborta com um `AbortError`.
    pub fn abort(&self, reason: Option<Error>) {
        self.0.abort_with(reason);
    }
}
