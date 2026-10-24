//! Porte de `src/vs/workbench/contrib/terminal/browser/terminalEvents.ts`: o multiplexador dos eventos de uma capacidade em
//! todas as instâncias de terminal (`createInstanceCapabilityEventMultiplexer`), mais o trait de extensão que dá ao
//! `ITerminalService` o `createOnInstanceEvent` e o `createOnInstanceCapabilityEvent` genéricos.
//!
//! Divergências:
//! - **Genéricos num trait de objeto.** `createOnInstanceEvent<T>` e `createOnInstanceCapabilityEvent<T, K>` não cabem num
//!   `dyn ITerminalService`; o [`ITerminalServiceEvents`] os implementa para todo serviço com `instances`,
//!   `onDidCreateInstance` e `onDidDisposeInstance`, que é o que o `TerminalService` do original passa ao multiplexador.
//! - **Identidade da instância.** O `DynamicListEventMultiplexer<ITerminalInstance, T>` do original usa a instância como chave de
//!   um `Map`; aqui a chave é o [`ById`] (igualdade pelo endereço do `Rc`).
//! - **Capacidade tipada.** `capabilityId: T extends TerminalCapability` é o marcador `C: ITerminalCapabilityImplMap`, e o
//!   `ITerminalCapabilityImplMap[T]` é `Rc<C::Impl>`; o `DisposableMap` interno, com a capacidade como chave, usa o [`ById`].
//! - **`IDynamicListEventMultiplexer`.** `{ dispose, event }` é o [`InstanceEventMultiplexer`].

use std::rc::Rc;

use je_base::event::{DynamicListEventMultiplexer, Event, EventMultiplexer};
use je_base::lifecycle::{ById, DisposableMap, DisposableStore, IDisposable};
use je_platform::terminal::capabilities::capabilities::ITerminalCapabilityImplMap;
use je_platform::terminal::capabilities::terminal_capability_store::TerminalCapabilityStoreExt;

use super::terminal::{ITerminalInstance, ITerminalService};

/// O `{ instance: ITerminalInstance; data: K }` que o multiplexador de capacidades dispara.
pub struct InstanceCapabilityEvent<K> {
    pub instance: Rc<dyn ITerminalInstance>,
    pub data: K,
}

impl<K: Clone> Clone for InstanceCapabilityEvent<K> {
    fn clone(&self) -> Self {
        Self { instance: self.instance.clone(), data: self.data.clone() }
    }
}

/// `IDynamicListEventMultiplexer<T>`: `{ dispose, event }`.
pub struct InstanceEventMultiplexer<T: Clone + 'static> {
    store: Rc<DisposableStore>,
    pub event: Event<T>,
}

impl<T: Clone + 'static> IDisposable for InstanceEventMultiplexer<T> {
    fn dispose(&self) {
        self.store.dispose();
    }
}

/// Um evento de instâncias como evento de [`ById`] (as chaves do multiplexador dinâmico).
fn by_id(event: &Event<Rc<dyn ITerminalInstance>>) -> Event<ById<dyn ITerminalInstance>> {
    event.map(|instance| ById(instance.clone()), None)
}

/// `createInstanceCapabilityEventMultiplexer(currentInstances, onAddInstance, onRemoveInstance, capabilityId, getEvent)`.
pub fn create_instance_capability_event_multiplexer<C: ITerminalCapabilityImplMap, K: Clone + 'static>(
    current_instances: Vec<Rc<dyn ITerminalInstance>>,
    on_add_instance: Event<Rc<dyn ITerminalInstance>>,
    on_remove_instance: Event<Rc<dyn ITerminalInstance>>,
    get_event: impl Fn(Rc<C::Impl>) -> Event<K> + 'static,
) -> InstanceEventMultiplexer<InstanceCapabilityEvent<K>> {
    let store = DisposableStore::new();
    let multiplexer = store.add_rc(EventMultiplexer::<InstanceCapabilityEvent<K>>::new());
    let capability_listeners = store.add_rc(DisposableMap::<u32, DisposableMap<ById<C::Impl>, dyn IDisposable>>::new());

    let get_event = Rc::new(get_event);
    let add_capability: Rc<dyn Fn(&Rc<dyn ITerminalInstance>, Rc<C::Impl>)> = {
        let (multiplexer, capability_listeners) = (multiplexer.clone(), capability_listeners.clone());
        Rc::new(move |instance, capability| {
            let source = instance.clone();
            let listener = multiplexer.add(get_event(capability.clone()).map(move |data| InstanceCapabilityEvent { instance: source.clone(), data: data.clone() }, None));
            let existing = capability_listeners.get(&instance.instance_id());
            let instance_capability_listeners = match existing {
                Some(existing) => existing,
                None => {
                    let created = DisposableMap::<ById<C::Impl>, dyn IDisposable>::new();
                    capability_listeners.set(instance.instance_id(), created.clone(), false);
                    created
                }
            };
            instance_capability_listeners.set(ById(capability), listener, false);
        })
    };

    // Existing instances
    for instance in &current_instances {
        if let Some(capability) = instance.capabilities().get::<C>() {
            add_capability(instance, capability);
        }
    }

    // Removed instances
    {
        let capability_listeners = capability_listeners.clone();
        store.add(on_remove_instance.listen(move |instance| capability_listeners.delete_and_dispose(&instance.instance_id())));
    }

    let items: Vec<ById<dyn ITerminalInstance>> = current_instances.iter().cloned().map(ById).collect();
    let (on_add_by_id, on_remove_by_id) = (by_id(&on_add_instance), by_id(&on_remove_instance));

    // Added capabilities
    let add_capability_multiplexer = DynamicListEventMultiplexer::new(items.clone(), &on_add_by_id, &on_remove_by_id, |item: &ById<dyn ITerminalInstance>| {
        let instance = item.0.clone();
        instance.capabilities().create_on_did_add_capability_of_type_event::<C>().map(move |change| (instance.clone(), change.clone()), None)
    });
    {
        let add_capability = add_capability.clone();
        store.add(add_capability_multiplexer.event.listen(move |(instance, capability)| add_capability(instance, capability.clone())));
    }
    store.add(Rc::new(add_capability_multiplexer));

    // Removed capabilities
    let remove_capability_multiplexer = DynamicListEventMultiplexer::new(items, &on_add_by_id, &on_remove_by_id, |item: &ById<dyn ITerminalInstance>| {
        let instance = item.0.clone();
        instance.capabilities().create_on_did_remove_capability_of_type_event::<C>().map(move |change| (instance.clone(), change.clone()), None)
    });
    {
        let capability_listeners = capability_listeners.clone();
        store.add(remove_capability_multiplexer.event.listen(move |(instance, capability)| {
            if let Some(instance_capability_listeners) = capability_listeners.get(&instance.instance_id()) {
                instance_capability_listeners.delete_and_dispose(&ById(capability.clone()));
            }
        }));
    }
    store.add(Rc::new(remove_capability_multiplexer));

    InstanceEventMultiplexer { event: multiplexer.event(), store }
}

/// Os métodos genéricos de `ITerminalService` que não cabem num trait de objeto (ver as divergências no topo do módulo).
pub trait ITerminalServiceEvents: ITerminalService {
    /// `createOnInstanceEvent(getEvent)`.
    fn create_on_instance_event<T: Clone + 'static>(
        &self,
        get_event: impl Fn(&Rc<dyn ITerminalInstance>) -> Event<T> + 'static,
    ) -> DynamicListEventMultiplexer<ById<dyn ITerminalInstance>, T> {
        let items: Vec<ById<dyn ITerminalInstance>> = self.instances().into_iter().map(ById).collect();
        DynamicListEventMultiplexer::new(items, &by_id(&self.on_did_create_instance()), &by_id(&self.on_did_dispose_instance()), move |item| get_event(&item.0))
    }

    /// `createOnInstanceCapabilityEvent(capabilityId, getEvent)`.
    fn create_on_instance_capability_event<C: ITerminalCapabilityImplMap, K: Clone + 'static>(
        &self,
        get_event: impl Fn(Rc<C::Impl>) -> Event<K> + 'static,
    ) -> InstanceEventMultiplexer<InstanceCapabilityEvent<K>> {
        create_instance_capability_event_multiplexer::<C, K>(self.instances(), self.on_did_create_instance(), self.on_did_dispose_instance(), get_event)
    }
}

impl<S: ITerminalService + ?Sized> ITerminalServiceEvents for S {}
