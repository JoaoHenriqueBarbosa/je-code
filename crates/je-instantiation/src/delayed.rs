//! A instanciação atrasada (`supportsDelayedInstantiation`): o proxy do `_createServiceInstance`.
//!
//! No original o serviço injetado é um `Proxy` de JavaScript que cria o objeto de verdade na primeira
//! leitura de propriedade (ou no tempo ocioso, via `GlobalIdleValue`) e, antes disso, devolve eventos
//! `onDid*`/`onWill*` "vazios" cujas inscrições ficam guardadas e são religadas ao serviço real quando
//! ele nasce. Rust não tem proxy genérico, então cada trait atrasável declara o seu com
//! [`delayed_proxy!`](crate::delayed_proxy): o macro gera um tipo que implementa o trait repassando
//! cada método ao [`DelayedHandle`], e cada evento pelo caminho das inscrições antecipadas.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use je_base::async_::GlobalIdleValue;
use je_base::event::Listener;
use je_base::lifecycle::{DisposableRef, to_disposable};

type AttachFn<T> = Box<dyn FnOnce(&Rc<T>) -> DisposableRef>;

struct EarlyEntry<T: ?Sized + 'static> {
    key: &'static str,
    id: u64,
    /// Liga o listener ao evento do serviço real. Roda uma vez, na criação.
    attach: RefCell<Option<AttachFn<T>>>,
    /// A inscrição real, preenchida por `attach`.
    disposable: RefCell<Option<DisposableRef>>,
}

/// As inscrições feitas antes de o serviço existir (`earlyListeners`).
pub(crate) struct EarlyListeners<T: ?Sized + 'static> {
    entries: RefCell<Vec<Rc<EarlyEntry<T>>>>,
    next_id: Cell<u64>,
}

impl<T: ?Sized + 'static> Default for EarlyListeners<T> {
    fn default() -> Self {
        Self {
            entries: RefCell::new(Vec::new()),
            next_id: Cell::new(0),
        }
    }
}

impl<T: ?Sized + 'static> EarlyListeners<T> {
    /// Religa tudo ao serviço real, agrupado por evento na ordem em que cada evento apareceu e, dentro
    /// do evento, na ordem de inscrição (a iteração do `Map` de `LinkedList` do original).
    pub(crate) fn attach_all(&self, service: &Rc<T>) {
        let entries = std::mem::take(&mut *self.entries.borrow_mut());
        let mut keys: Vec<&'static str> = Vec::new();
        for entry in &entries {
            if !keys.contains(&entry.key) {
                keys.push(entry.key);
            }
        }
        for key in keys {
            for entry in entries.iter().filter(|e| e.key == key) {
                if let Some(attach) = entry.attach.borrow_mut().take() {
                    let subscription = attach(service);
                    *entry.disposable.borrow_mut() = Some(subscription);
                }
            }
        }
    }

    fn remove(&self, id: u64) {
        self.entries.borrow_mut().retain(|e| e.id != id);
    }
}

/// O que o proxy usa para alcançar o serviço atrasado.
pub struct DelayedHandle<T: ?Sized + 'static> {
    idle: Rc<GlobalIdleValue<Rc<T>>>,
    early: Rc<EarlyListeners<T>>,
}

impl<T: ?Sized + 'static> Clone for DelayedHandle<T> {
    fn clone(&self) -> Self {
        Self {
            idle: self.idle.clone(),
            early: self.early.clone(),
        }
    }
}

impl<T: ?Sized + 'static> DelayedHandle<T> {
    pub(crate) fn new(idle: Rc<GlobalIdleValue<Rc<T>>>, early: Rc<EarlyListeners<T>>) -> Self {
        Self { idle, early }
    }

    /// `idle.value`: cria o serviço se ainda não existe.
    pub fn get(&self) -> Rc<T> {
        self.idle.value()
    }

    pub fn is_initialized(&self) -> bool {
        self.idle.is_initialized()
    }

    /// A inscrição num evento `onDid*`/`onWill*`. Com o serviço criado vai direto a ele; antes disso
    /// fica guardada, sem criar o serviço, e o `dispose` do resultado a cancela nos dois casos.
    pub fn subscribe<E: 'static>(
        &self,
        key: &'static str,
        event_of: fn(&T) -> je_base::event::Event<E>,
        listener: Listener<E>,
    ) -> DisposableRef {
        if self.idle.is_initialized() {
            return event_of(&*self.get()).listen_rc(listener);
        }
        let id = self.early.next_id.get();
        self.early.next_id.set(id + 1);
        let entry = Rc::new(EarlyEntry {
            key,
            id,
            attach: RefCell::new(Some(Box::new(move |service: &Rc<T>| {
                event_of(service).listen_rc(listener)
            }))),
            disposable: RefCell::new(None),
        });
        self.early.entries.borrow_mut().push(entry.clone());
        let early = Rc::downgrade(&self.early);
        to_disposable(move || {
            if let Some(early) = early.upgrade() {
                early.remove(id);
            }
            let subscription = entry.disposable.borrow_mut().take();
            if let Some(subscription) = subscription {
                subscription.dispose();
            }
        })
    }
}

/// Gera o proxy de instanciação atrasada de um trait.
///
/// `methods` repassa ao serviço real (criando-o se preciso). `events` são os métodos que devolvem
/// `Event<_>` (os `onDid*`/`onWill*` do original): obter ou assinar o evento não cria o serviço.
/// Só entram métodos que devolvem valores por posse (não referências).
///
/// ```ignore
/// delayed_proxy!(pub FooProxy for dyn IFoo {
///     methods { fn do_it(&self, n: u32) -> bool; }
///     events { fn on_did_do_it(&self) -> u32; }
/// });
/// service_id!(pub I_FOO: dyn IFoo = "foo", proxy FooProxy);
/// ```
#[macro_export]
macro_rules! delayed_proxy {
    ($vis:vis $proxy:ident for dyn $tr:path {
        methods { $( fn $m:ident(&self $(, $a:ident : $at:ty)* ) $(-> $r:ty)? ; )* }
        events { $( fn $e:ident(&self) -> $et:ty ; )* }
    }) => {
        $vis struct $proxy {
            handle: $crate::DelayedHandle<dyn $tr>,
        }

        impl $proxy {
            $vis fn wrap(handle: $crate::DelayedHandle<dyn $tr>) -> ::std::rc::Rc<dyn $tr> {
                ::std::rc::Rc::new(Self { handle })
            }
        }

        impl $tr for $proxy {
            $( fn $m(&self $(, $a: $at)*) $(-> $r)? { self.handle.get().$m($($a),*) } )*
            $(
                fn $e(&self) -> $crate::je_base::event::Event<$et> {
                    let handle = self.handle.clone();
                    $crate::je_base::event::Event::new(move |listener| handle.subscribe(stringify!($e), |s| s.$e(), listener))
                }
            )*
        }
    };
}
