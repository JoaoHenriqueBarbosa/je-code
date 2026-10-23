//! Porte de `src/vs/platform/terminal/common/capabilities/terminalCapabilityStore.ts`: o
//! [`TerminalCapabilityStore`] e o [`TerminalCapabilityStoreMultiplexer`].
//!
//! Decisões de tradução:
//! - O que as duas classes repetem (os dois emitters, o `_store` do `Disposable` e os getters
//!   `@memoize`) vive em [`CapabilityEvents`], que cada uma tem por composição. O `Disposable` do
//!   original é esse `store`: descartar a classe descarta tudo que ela registrou.
//! - O `@memoize` guarda o `Event` criado no primeiro acesso e devolve o mesmo nos seguintes; aqui o
//!   getter devolve o mesmo `Rc<Event<_>>`.
//! - O `items` do original é um iterador vivo sobre o `Map`; aqui é um retrato da ordem de inserção no
//!   momento da chamada.
//! - `get<T>` e `createOn...OfTypeEvent<T>` ficam em [`TerminalCapabilityStoreExt`], com a implementação
//!   para todo store (inclusive `dyn ITerminalCapabilityStore`).

use std::cell::RefCell;
use std::rc::Rc;

use indexmap::IndexMap;
use je_base::event::{self, Emitter, Event};
use je_base::lifecycle::{DisposableRef, DisposableStore, IDisposable};

use super::capabilities::{
    AnyTerminalCapabilityChangeEvent, ICommandDetectionCapability, ICwdDetectionCapability, ITerminalCapabilityImplMap, ITerminalCapabilityStore,
    TerminalCapability, TerminalCapabilityImpl,
};

/// O evento de um getter memoizado.
type Memo<T> = RefCell<Option<Rc<Event<T>>>>;

/// `@memoize`: cria o evento na primeira chamada e devolve o mesmo `Rc` depois.
fn memoize<T: 'static>(slot: &Memo<T>, create: impl FnOnce() -> Event<T>) -> Rc<Event<T>> {
    let cached = slot.borrow().clone();
    if let Some(event) = cached {
        return event;
    }
    let event = Rc::new(create());
    *slot.borrow_mut() = Some(event.clone());
    event
}

/// `Event.map(Event.filter(event, e => e.id === type), e => e.capability as ITerminalCapabilityImplMap[T])`.
fn filtered_by_type<T: ITerminalCapabilityImplMap>(event: Event<AnyTerminalCapabilityChangeEvent>, store: Option<&DisposableStore>) -> Event<Rc<T::Impl>> {
    event
        .filter(|e| e.id == T::CAPABILITY, store)
        .map(|e| T::unwrap_impl(&e.capability).expect("o filtro garante que a capacidade é deste tipo"), store)
}

/// `Event.map(Event.filter(event, e => e.id === capability), () => void 0)`.
fn signal_of_type(event: Event<AnyTerminalCapabilityChangeEvent>, capability: TerminalCapability, store: &DisposableStore) -> Event<()> {
    event.filter(move |e| e.id == capability, Some(store)).map(|_| (), Some(store))
}

/// O que o [`TerminalCapabilityStore`] e o [`TerminalCapabilityStoreMultiplexer`] têm em comum: o `Disposable`,
/// os emitters de adição e remoção e os getters memoizados derivados deles.
struct CapabilityEvents {
    store: Rc<DisposableStore>,
    on_did_add: Rc<Emitter<AnyTerminalCapabilityChangeEvent>>,
    on_did_remove: Rc<Emitter<AnyTerminalCapabilityChangeEvent>>,
    on_did_change_capabilities: Memo<()>,
    on_did_add_command_detection: Memo<Rc<dyn ICommandDetectionCapability>>,
    on_did_remove_command_detection: Memo<()>,
    on_did_add_cwd_detection: Memo<Rc<dyn ICwdDetectionCapability>>,
    on_did_remove_cwd_detection: Memo<()>,
}

impl CapabilityEvents {
    fn new() -> Self {
        let store = DisposableStore::new();
        let on_did_add = store.add_rc(Emitter::new());
        let on_did_remove = store.add_rc(Emitter::new());
        Self {
            store,
            on_did_add,
            on_did_remove,
            on_did_change_capabilities: RefCell::default(),
            on_did_add_command_detection: RefCell::default(),
            on_did_remove_command_detection: RefCell::default(),
            on_did_add_cwd_detection: RefCell::default(),
            on_did_remove_cwd_detection: RefCell::default(),
        }
    }

    /// `_register(o)`.
    fn register(&self, disposable: DisposableRef) {
        self.store.add(disposable);
    }

    fn on_did_add_capability(&self) -> Event<AnyTerminalCapabilityChangeEvent> {
        self.on_did_add.event()
    }

    fn on_did_remove_capability(&self) -> Event<AnyTerminalCapabilityChangeEvent> {
        self.on_did_remove.event()
    }

    fn on_did_change_capabilities(&self) -> Rc<Event<()>> {
        memoize(&self.on_did_change_capabilities, || {
            event::any(vec![self.on_did_add_capability(), self.on_did_remove_capability()]).map(|_| (), Some(&self.store))
        })
    }

    fn on_did_add_command_detection_capability(&self) -> Rc<Event<Rc<dyn ICommandDetectionCapability>>> {
        memoize(&self.on_did_add_command_detection, || {
            filtered_by_type::<super::capabilities::CommandDetection>(self.on_did_add_capability(), Some(&self.store))
        })
    }

    fn on_did_remove_command_detection_capability(&self) -> Rc<Event<()>> {
        memoize(&self.on_did_remove_command_detection, || {
            signal_of_type(self.on_did_remove_capability(), TerminalCapability::CommandDetection, &self.store)
        })
    }

    fn on_did_add_cwd_detection_capability(&self) -> Rc<Event<Rc<dyn ICwdDetectionCapability>>> {
        memoize(&self.on_did_add_cwd_detection, || {
            filtered_by_type::<super::capabilities::CwdDetection>(self.on_did_add_capability(), Some(&self.store))
        })
    }

    fn on_did_remove_cwd_detection_capability(&self) -> Rc<Event<()>> {
        memoize(&self.on_did_remove_cwd_detection, || {
            signal_of_type(self.on_did_remove_capability(), TerminalCapability::CwdDetection, &self.store)
        })
    }
}

impl IDisposable for CapabilityEvents {
    fn dispose(&self) {
        self.store.dispose();
    }
}

/// `createCapabilityEvent(capability, impl)`.
fn create_capability_event(imp: TerminalCapabilityImpl) -> AnyTerminalCapabilityChangeEvent {
    AnyTerminalCapabilityChangeEvent { id: imp.capability(), capability: imp }
}

/// Implementa os membros de [`ITerminalCapabilityStore`] que só expõem o que está em `CapabilityEvents`
/// (os dois emitters e os getters memoizados, iguais nas duas classes do original). Vai dentro do `impl`.
macro_rules! impl_capability_events {
    () => {
        fn on_did_add_capability(&self) -> Event<AnyTerminalCapabilityChangeEvent> {
            self.events.on_did_add_capability()
        }

        fn on_did_remove_capability(&self) -> Event<AnyTerminalCapabilityChangeEvent> {
            self.events.on_did_remove_capability()
        }

        fn on_did_change_capabilities(&self) -> Rc<Event<()>> {
            self.events.on_did_change_capabilities()
        }

        fn on_did_add_command_detection_capability(&self) -> Rc<Event<Rc<dyn ICommandDetectionCapability>>> {
            self.events.on_did_add_command_detection_capability()
        }

        fn on_did_remove_command_detection_capability(&self) -> Rc<Event<()>> {
            self.events.on_did_remove_command_detection_capability()
        }

        fn on_did_add_cwd_detection_capability(&self) -> Rc<Event<Rc<dyn ICwdDetectionCapability>>> {
            self.events.on_did_add_cwd_detection_capability()
        }

        fn on_did_remove_cwd_detection_capability(&self) -> Rc<Event<()>> {
            self.events.on_did_remove_cwd_detection_capability()
        }
    };
}

/// A forma genérica de `get<T>`, `createOnDidAddCapabilityOfTypeEvent<T>` e
/// `createOnDidRemoveCapabilityOfTypeEvent<T>` de [`ITerminalCapabilityStore`].
pub trait TerminalCapabilityStoreExt: ITerminalCapabilityStore {
    /// Cria um evento disparado quando uma capacidade do tipo `T` é removida. Use no lugar de
    /// `on_did_remove_capability` quando o tipo da implementação precisa ser mantido.
    fn create_on_did_remove_capability_of_type_event<T: ITerminalCapabilityImplMap>(&self) -> Event<Rc<T::Impl>> {
        filtered_by_type::<T>(self.on_did_remove_capability(), None)
    }

    /// Cria um evento disparado quando uma capacidade do tipo `T` é adicionada. Use no lugar de
    /// `on_did_add_capability` quando o tipo da implementação precisa ser mantido.
    fn create_on_did_add_capability_of_type_event<T: ITerminalCapabilityImplMap>(&self) -> Event<Rc<T::Impl>> {
        filtered_by_type::<T>(self.on_did_add_capability(), None)
    }

    /// A implementação de uma capacidade, se ela foi adicionada ao store.
    fn get<T: ITerminalCapabilityImplMap>(&self) -> Option<Rc<T::Impl>> {
        self.get_capability(T::CAPABILITY).and_then(|imp| T::unwrap_impl(&imp))
    }
}

impl<S: ITerminalCapabilityStore + ?Sized> TerminalCapabilityStoreExt for S {}

/// `TerminalCapabilityStore`.
pub struct TerminalCapabilityStore {
    events: CapabilityEvents,
    map: RefCell<IndexMap<TerminalCapability, TerminalCapabilityImpl>>,
}

impl TerminalCapabilityStore {
    pub fn new() -> Rc<Self> {
        Rc::new(Self { events: CapabilityEvents::new(), map: RefCell::new(IndexMap::new()) })
    }

    /// `add(capability, impl)`: a capacidade é a do marcador `T`.
    pub fn add<T: ITerminalCapabilityImplMap>(&self, imp: Rc<T::Impl>) {
        let imp = T::wrap_impl(imp);
        self.map.borrow_mut().insert(T::CAPABILITY, imp.clone());
        self.events.on_did_add.fire(create_capability_event(imp));
    }

    /// `remove(capability)`.
    pub fn remove(&self, capability: TerminalCapability) {
        let removed = self.map.borrow_mut().shift_remove(&capability);
        let Some(imp) = removed else {
            return;
        };
        self.events.on_did_remove.fire(create_capability_event(imp));
    }
}

impl ITerminalCapabilityStore for TerminalCapabilityStore {
    impl_capability_events!();

    fn items(&self) -> Box<dyn Iterator<Item = TerminalCapability>> {
        Box::new(self.map.borrow().keys().copied().collect::<Vec<_>>().into_iter())
    }

    fn has(&self, capability: TerminalCapability) -> bool {
        self.map.borrow().contains_key(&capability)
    }

    fn get_capability(&self, capability: TerminalCapability) -> Option<TerminalCapabilityImpl> {
        self.map.borrow().get(&capability).cloned()
    }
}

impl IDisposable for TerminalCapabilityStore {
    fn dispose(&self) {
        self.events.dispose();
    }
}

/// `TerminalCapabilityStoreMultiplexer`: vários stores vistos como um só.
pub struct TerminalCapabilityStoreMultiplexer {
    events: CapabilityEvents,
    /// `_stores`.
    pub stores: RefCell<Vec<Rc<dyn ITerminalCapabilityStore>>>,
}

impl TerminalCapabilityStoreMultiplexer {
    pub fn new() -> Rc<Self> {
        Rc::new(Self { events: CapabilityEvents::new(), stores: RefCell::new(Vec::new()) })
    }

    /// `add(store)`: dispara a adição das capacidades que o store já tem e passa a repassar os eventos dele.
    pub fn add(&self, store: Rc<dyn ITerminalCapabilityStore>) {
        self.stores.borrow_mut().push(store.clone());
        for capability in store.items() {
            let imp = store.get_capability(capability).expect("a capacidade listada em items existe no store");
            self.events.on_did_add.fire(create_capability_event(imp));
        }
        self.events.register(store.on_did_add_capability().forward(&self.events.on_did_add));
        self.events.register(store.on_did_remove_capability().forward(&self.events.on_did_remove));
    }
}

impl ITerminalCapabilityStore for TerminalCapabilityStoreMultiplexer {
    impl_capability_events!();

    fn items(&self) -> Box<dyn Iterator<Item = TerminalCapability>> {
        let items: Vec<TerminalCapability> = self.stores.borrow().iter().flat_map(|store| store.items()).collect();
        Box::new(items.into_iter())
    }

    fn has(&self, capability: TerminalCapability) -> bool {
        self.stores.borrow().iter().any(|store| store.items().any(|c| c == capability))
    }

    fn get_capability(&self, capability: TerminalCapability) -> Option<TerminalCapabilityImpl> {
        self.stores.borrow().iter().find_map(|store| store.get_capability(capability))
    }
}

impl IDisposable for TerminalCapabilityStoreMultiplexer {
    fn dispose(&self) {
        self.events.dispose();
    }
}
