//! Porte de `instantiationService.ts`: o `InstantiationService`.
//!
//! O algoritmo de `_createAndCacheServiceInstance` é o do original passo a passo (pilha de descritos,
//! grafo, raízes em ordem de inserção, heurística de 1000 passos, erro de ciclo com a cadeia), porque a
//! ordem de criação dos serviços é observável.

use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::rc::{Rc, Weak};

use je_base::async_::GlobalIdleValue;
use je_base::errors::{self, Error};
use je_base::lifecycle::{DisposableRef, DisposableStore, IDisposable, dispose_all};

use crate::delayed::{DelayedHandle, EarlyListeners};
use crate::descriptors::{BuildFn, ErasedDescriptor, SyncDescriptor};
use crate::graph::Graph;
use crate::instantiation::{
    Dependencies, ErasedInstance, I_INSTANTIATION_SERVICE, Injectable, ProxyFn, ServiceId,
    ServiceKey, downcast, short_type_name,
};
use crate::service_collection::{Entry, ServiceCollection};
use crate::trace::Trace;

/// Os parâmetros do construtor do original além da coleção.
#[derive(Default)]
pub struct InstantiationOptions {
    pub strict: bool,
    pub parent: Option<Rc<InstantiationService>>,
    pub enable_tracing: bool,
}

#[derive(Clone)]
struct Triple {
    id: ServiceKey,
    desc: Rc<ErasedDescriptor>,
    trace: Trace,
}

fn throw_cyclic<T>(graph: &Graph<T>) -> ! {
    let message = graph
        .find_cycle_slow()
        .unwrap_or_else(|| format!("UNABLE to detect cycle, dumping graph: \n{graph}"));
    errors::throw(Error::new(message))
}

pub struct InstantiationService {
    weak_self: Weak<InstantiationService>,
    services: ServiceCollection,
    strict: bool,
    parent: Option<Rc<InstantiationService>>,
    enable_tracing: bool,
    global_graph: Option<Rc<RefCell<Graph<String>>>>,
    global_graph_implicit_dependency: RefCell<Option<String>>,
    is_disposed: Cell<bool>,
    services_to_maybe_dispose: RefCell<Vec<DisposableRef>>,
    children: RefCell<Vec<Rc<InstantiationService>>>,
    active_instantiations: RefCell<HashSet<ServiceKey>>,
    /// As chaves que este serviço materializou na sua coleção, para soltá-las no `dispose`.
    created_keys: RefCell<Vec<ServiceKey>>,
}

struct ActiveGuard<'a> {
    set: &'a RefCell<HashSet<ServiceKey>>,
    key: ServiceKey,
}

impl Drop for ActiveGuard<'_> {
    fn drop(&mut self) {
        self.set.borrow_mut().remove(&self.key);
    }
}

struct AccessorState {
    insta: Rc<InstantiationService>,
    trace: Trace,
    done: Cell<bool>,
}

/// `ServicesAccessor`. É `Clone` para o uso fora da invocação ser detectado em tempo de execução,
/// como no original.
#[derive(Clone)]
pub struct ServicesAccessor {
    state: Rc<AccessorState>,
}

impl ServicesAccessor {
    fn check_valid(&self) {
        if self.state.done.get() {
            errors::throw(Error::new(
                "Illegal state: service accessor is only valid during the invocation of its target method",
            ));
        }
    }

    /// `get(id)` na forma do original: `None` (o `undefined`) se o serviço não existe e o serviço de
    /// instanciação não é estrito; no modo estrito lança `[invokeFunction] unknown service '...'`.
    pub fn try_get<T: ?Sized + 'static>(&self, id: ServiceId<T>) -> Option<Rc<T>> {
        self.check_valid();
        let result = self
            .state
            .insta
            .get_or_create_service_instance(id.key(), &self.state.trace);
        if result.is_none() {
            self.state
                .insta
                .throw_if_strict(&format!("[invokeFunction] unknown service '{id}'"), false);
        }
        result.map(|r| downcast(&r))
    }

    /// `get(id)` que sempre devolve o serviço: lança se ele não existe (o original devolveria `undefined`
    /// fora do modo estrito).
    pub fn get<T: ?Sized + 'static>(&self, id: ServiceId<T>) -> Rc<T> {
        self.try_get(id).unwrap_or_else(|| {
            errors::throw(Error::new(format!(
                "[invokeFunction] unknown service '{id}'"
            )))
        })
    }
}

struct InvocationGuard {
    state: Rc<AccessorState>,
}

impl Drop for InvocationGuard {
    fn drop(&mut self) {
        self.state.done.set(true);
        self.state.trace.stop();
    }
}

impl InstantiationService {
    pub fn new(services: ServiceCollection) -> Rc<Self> {
        Self::with_options(services, InstantiationOptions::default())
    }

    pub fn with_options(services: ServiceCollection, options: InstantiationOptions) -> Rc<Self> {
        let InstantiationOptions {
            strict,
            parent,
            enable_tracing,
        } = options;
        let global_graph = if enable_tracing {
            parent
                .as_ref()
                .and_then(|p| p.global_graph.clone())
                .or_else(|| Some(Rc::new(RefCell::new(Graph::new(|e: &String| e.clone())))))
        } else {
            None
        };
        Rc::new_cyclic(|weak| Self {
            weak_self: weak.clone(),
            services,
            strict,
            parent,
            enable_tracing,
            global_graph,
            global_graph_implicit_dependency: RefCell::new(None),
            is_disposed: Cell::new(false),
            services_to_maybe_dispose: RefCell::new(Vec::new()),
            children: RefCell::new(Vec::new()),
            active_instantiations: RefCell::new(HashSet::new()),
            created_keys: RefCell::new(Vec::new()),
        })
    }

    /// `_globalGraph`: o grafo de dependências de todos os serviços (só com o rastreamento ligado).
    pub fn global_graph(&self) -> Option<Rc<RefCell<Graph<String>>>> {
        self.global_graph.clone()
    }

    fn this(&self) -> Rc<Self> {
        self.weak_self
            .upgrade()
            .expect("o serviço de instanciação está vivo enquanto um método dele roda")
    }

    fn throw_if_disposed(&self) {
        if self.is_disposed.get() {
            errors::throw(Error::new("InstantiationService has been disposed"));
        }
    }

    pub(crate) fn throw_if_strict(&self, msg: &str, print_warning: bool) {
        if print_warning {
            eprintln!("{msg}");
        }
        if self.strict {
            errors::throw(Error::new(msg));
        }
    }

    /// `createChild(services, store)`.
    pub fn create_child(
        &self,
        services: ServiceCollection,
        store: Option<&DisposableStore>,
    ) -> Rc<Self> {
        self.throw_if_disposed();
        let result = Self::with_options(
            services,
            InstantiationOptions {
                strict: self.strict,
                parent: Some(self.this()),
                enable_tracing: self.enable_tracing,
            },
        );
        self.children.borrow_mut().push(result.clone());
        if let Some(store) = store {
            let handle: DisposableRef = result.clone();
            store.add(handle);
        }
        result
    }

    /// `invokeFunction(fn)`.
    pub fn invoke_function<R, F: FnOnce(&ServicesAccessor) -> R>(&self, f: F) -> R {
        self.throw_if_disposed();
        let trace = Trace::trace_invocation(self.enable_tracing, std::any::type_name::<F>());
        let state = Rc::new(AccessorState {
            insta: self.this(),
            trace,
            done: Cell::new(false),
        });
        let accessor = ServicesAccessor {
            state: state.clone(),
        };
        let _guard = InvocationGuard { state };
        f(&accessor)
    }

    /// `createInstance(ctor, ...args)`.
    pub fn create_instance<C: Injectable>(&self, args: C::Args) -> Rc<C> {
        self.throw_if_disposed();
        let trace = Trace::trace_creation(self.enable_tracing, &short_type_name::<C>());
        let result = self.construct::<C>(args, &trace);
        trace.stop();
        result
    }

    /// `createInstance(descriptor)`: cria o objeto do descritor sem registrá-lo como serviço.
    pub fn create_instance_from_descriptor<T: ?Sized + 'static>(
        &self,
        descriptor: &SyncDescriptor<T>,
    ) -> Rc<T> {
        self.throw_if_disposed();
        let trace = Trace::trace_creation(self.enable_tracing, descriptor.ctor_name());
        let result = (descriptor.build())(self, &trace).service;
        trace.stop();
        result
    }

    /// `_createInstance`: resolve os serviços injetados na ordem dos parâmetros e constrói.
    fn construct<C: Injectable>(&self, args: C::Args, trace: &Trace) -> Rc<C> {
        let deps = <C::Deps as Dependencies>::resolve(self, &short_type_name::<C>(), trace);
        C::create(args, deps)
    }

    /// O trecho de `_createInstance` que busca um serviço injetado.
    pub(crate) fn resolve_service_arg(
        &self,
        key: ServiceKey,
        ctor_name: &str,
        trace: &Trace,
    ) -> Option<ErasedInstance> {
        let service = self.get_or_create_service_instance(key, trace);
        if service.is_none() {
            self.throw_if_strict(
                &format!("[createInstance] {ctor_name} depends on UNKNOWN service {key}."),
                false,
            );
        }
        service
    }

    fn set_created_service_instance(&self, key: ServiceKey, instance: ErasedInstance) {
        if matches!(
            self.services.get_entry(&key),
            Some(Entry::Descriptor { .. })
        ) {
            self.services.set_entry(key, Entry::Instance(instance));
            self.created_keys.borrow_mut().push(key);
        } else if let Some(parent) = &self.parent {
            parent.set_created_service_instance(key, instance);
        } else {
            errors::throw(Error::new(
                "illegalState - setting UNKNOWN service instance",
            ));
        }
    }

    fn get_service_instance_or_descriptor(&self, key: &ServiceKey) -> Option<Entry> {
        // O original registra o próprio serviço na coleção, o que aqui formaria um ciclo de `Rc`.
        if *key == I_INSTANTIATION_SERVICE.key() {
            return Some(Entry::Instance(Rc::new(self.this())));
        }
        match self.services.get_entry(key) {
            Some(entry) => Some(entry),
            None => self
                .parent
                .as_ref()
                .and_then(|p| p.get_service_instance_or_descriptor(key)),
        }
    }

    pub(crate) fn get_or_create_service_instance(
        &self,
        key: ServiceKey,
        trace: &Trace,
    ) -> Option<ErasedInstance> {
        if let (Some(graph), Some(implicit)) = (
            &self.global_graph,
            &*self.global_graph_implicit_dependency.borrow(),
        ) {
            graph
                .borrow_mut()
                .insert_edge(implicit.clone(), key.name.to_string());
        }
        match self.get_service_instance_or_descriptor(&key) {
            Some(Entry::Descriptor { erased, .. }) => {
                Some(self.safe_create_and_cache_service_instance(
                    key,
                    erased,
                    trace.branch(key.name, true),
                ))
            }
            Some(Entry::Instance(instance)) => {
                trace.branch(key.name, false);
                Some(instance)
            }
            None => None,
        }
    }

    fn safe_create_and_cache_service_instance(
        &self,
        key: ServiceKey,
        desc: Rc<ErasedDescriptor>,
        trace: Trace,
    ) -> ErasedInstance {
        if !self.active_instantiations.borrow_mut().insert(key) {
            errors::throw(Error::new(format!(
                "illegal state - RECURSIVELY instantiating service '{key}'"
            )));
        }
        let _guard = ActiveGuard {
            set: &self.active_instantiations,
            key,
        };
        self.create_and_cache_service_instance(key, desc, trace)
    }

    fn create_and_cache_service_instance(
        &self,
        id: ServiceKey,
        desc: Rc<ErasedDescriptor>,
        trace: Trace,
    ) -> ErasedInstance {
        let mut graph = Graph::<Triple>::new(|data| data.id.name.to_string());

        let mut cycle_count = 0;
        let mut stack = vec![Triple { id, desc, trace }];
        let mut seen: HashSet<&'static str> = HashSet::new();
        while let Some(item) = stack.pop() {
            if !seen.insert(item.id.name) {
                continue;
            }

            graph.lookup_or_insert_node(item.clone());

            // a weak but working heuristic for cycle checks
            let steps = cycle_count;
            cycle_count += 1;
            if steps > 1000 {
                throw_cyclic(&graph);
            }

            // check all dependencies for existence and if they need to be created first
            for dependency in &item.desc.dependencies {
                let instance_or_desc = self.get_service_instance_or_descriptor(dependency);
                if instance_or_desc.is_none() {
                    self.throw_if_strict(
                        &format!(
                            "[createInstance] {id} depends on {dependency} which is NOT registered."
                        ),
                        true,
                    );
                }

                // take note of all service dependencies
                if let Some(global) = &self.global_graph {
                    global
                        .borrow_mut()
                        .insert_edge(item.id.name.to_string(), dependency.name.to_string());
                }

                if let Some(Entry::Descriptor { erased, .. }) = instance_or_desc {
                    let d = Triple {
                        id: *dependency,
                        desc: erased,
                        trace: item.trace.branch(dependency.name, true),
                    };
                    graph.insert_edge(item.clone(), d.clone());
                    stack.push(d);
                }
            }
        }

        loop {
            let roots: Vec<Triple> = graph
                .roots()
                .into_iter()
                .map(|node| node.data.clone())
                .collect();

            // if there is no more roots but still nodes in the graph we have a cycle
            if roots.is_empty() {
                if !graph.is_empty() {
                    throw_cyclic(&graph);
                }
                break;
            }

            for data in roots {
                // Repeat the check for this still being a service sync descriptor. That's because
                // instantiating a dependency might have side-effect and recursively trigger instantiation
                // so that some dependencies are now fullfilled already.
                if matches!(
                    self.get_service_instance_or_descriptor(&data.id),
                    Some(Entry::Descriptor { .. })
                ) {
                    // create instance and overwrite the service collections
                    let instance =
                        self.create_service_instance_with_owner(&data.id, &data.desc, &data.trace);
                    self.set_created_service_instance(data.id, instance);
                }
                graph.remove_node(&data);
            }
        }

        match self.get_service_instance_or_descriptor(&id) {
            Some(Entry::Instance(instance)) => instance,
            _ => errors::throw(Error::new(format!(
                "illegalState - service '{id}' was not created"
            ))),
        }
    }

    fn create_service_instance_with_owner(
        &self,
        key: &ServiceKey,
        desc: &ErasedDescriptor,
        trace: &Trace,
    ) -> ErasedInstance {
        if matches!(self.services.get_entry(key), Some(Entry::Descriptor { .. })) {
            (desc.instantiate)(self, trace)
        } else if let Some(parent) = &self.parent {
            parent.create_service_instance_with_owner(key, desc, trace)
        } else {
            errors::throw(Error::new(format!(
                "illegalState - creating UNKNOWN service instance {}",
                desc.ctor_name
            )));
        }
    }

    pub(crate) fn add_to_dispose_bucket(&self, disposable: Option<DisposableRef>) {
        if let Some(disposable) = disposable {
            self.services_to_maybe_dispose.borrow_mut().push(disposable);
        }
    }

    /// O ramo atrasado de `_createServiceInstance`: devolve o proxy; o objeto real nasce no primeiro
    /// uso ou no tempo ocioso, num serviço de instanciação filho (para o rastro das dependências).
    pub(crate) fn create_delayed_service<T: ?Sized + 'static>(
        &self,
        name: &'static str,
        proxy: ProxyFn<T>,
        build: BuildFn<T>,
        trace: &Trace,
    ) -> Rc<T> {
        let child = Self::with_options(
            ServiceCollection::new(),
            InstantiationOptions {
                strict: self.strict,
                parent: Some(self.this()),
                enable_tracing: self.enable_tracing,
            },
        );
        *child.global_graph_implicit_dependency.borrow_mut() = Some(name.to_string());

        let early = Rc::new(EarlyListeners::<T>::default());
        let owner = self.weak_self.clone();
        let idle = {
            let early = early.clone();
            let trace = trace.clone();
            Rc::new(GlobalIdleValue::new(move || {
                let built = build(&child, &trace);
                // early listeners that we kept are now being subscribed to the real service
                early.attach_all(&built.service);
                if let Some(owner) = owner.upgrade() {
                    owner.add_to_dispose_bucket(built.disposable);
                }
                built.service
            }))
        };
        proxy(DelayedHandle::new(idle, early))
    }
}

impl IDisposable for InstantiationService {
    fn dispose(&self) {
        // o filho sai do conjunto de filhos do pai antes de descartar a si mesmo
        if let Some(parent) = &self.parent {
            let me = std::ptr::from_ref(self);
            parent
                .children
                .borrow_mut()
                .retain(|c| !std::ptr::eq(Rc::as_ptr(c), me));
        }
        if self.is_disposed.get() {
            return;
        }
        self.is_disposed.set(true);

        // dispose all child services
        let children = std::mem::take(&mut *self.children.borrow_mut());
        dispose_all(&children);

        // dispose all services created by this service
        let candidates = self.services_to_maybe_dispose.borrow().clone();
        for candidate in &candidates {
            candidate.dispose();
        }
        self.services_to_maybe_dispose.borrow_mut().clear();

        // Solta as instâncias que este serviço materializou: elas apontam de volta para ele (o
        // `IInstantiationService` injetado), e sem isso o ciclo de `Rc` nunca seria desfeito.
        let created = std::mem::take(&mut *self.created_keys.borrow_mut());
        self.services.remove_entries(&created);
    }
}
