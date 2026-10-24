//! Porte de `src/vs/workbench/services/workingCopy/common/workingCopyService.ts`: o registro das working copies e o
//! rastreio de sujeira (dirty), com os eventos de registro, de mudança e de salvar.
//!
//! Divergências:
//! - **Injeção.** `IWorkingCopyService` é o trait [`IWorkingCopyService`]; a classe herda de `Disposable`, aqui um
//!   `Disposable` guardado no campo `base`. `registerSingleton(..., InstantiationType.Delayed)` vira [`register`], que o
//!   chamador roda uma vez na montagem do workbench; o proxy de instanciação atrasada é o [`WorkingCopyServiceProxy`].
//! - **Propriedades** (`workingCopies`, `dirtyCount`, `hasDirty`...) são métodos, e os eventos devolvem o [`Event`].
//! - **Sobrecargas.** `has(identifier)` e `has(resource)` são [`IWorkingCopyService::has`] (com o
//!   [`WorkingCopyIdentifier`]) e [`IWorkingCopyService::has_resource`] (com o `URI`): o `URI.isUri` do original é o tipo.
//!   Uma working copy, que no original é um `IWorkingCopyIdentifier` por tipagem estrutural, vira o identificador com
//!   [`WorkingCopyIdentifier::of`].
//! - **`IWorkingCopySaveEvent`** (`IBaseWorkingCopySaveEvent & { workingCopy }`) é a struct [`WorkingCopySaveEvent`]
//!   deste módulo, que guarda a working copy e o evento base (o `{ workingCopy, ...e }` do original).
//! - **Identidade.** `Set<IWorkingCopy>` e `DisposableMap<IWorkingCopy>` comparam por identidade do objeto: aqui é a
//!   identidade do `Rc` ([`id_of`] e [`ById`]).
//! - **`this` no descarte.** O `toDisposable(() => ...)` do original captura `this`; aqui o serviço guarda um `Weak`
//!   de si mesmo (`Rc::new_cyclic`) e o fecho o promove. Com o serviço já destruído não há mais registro a atualizar.
//! - **Pilha do vazamento.** `new Error().stack?.split('\n').slice(2).join('\n')` é a pilha capturada por
//!   [`StackSource::capture`], sem as duas primeiras linhas.

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use je_base::errors::{self, Error};
use je_base::event::{Emitter, Event};
use je_base::lifecycle::{ById, Disposable, DisposableMap, DisposableRef, DisposableStore, IDisposable, StackSource, id_of, to_disposable};
use je_base::map::{OrderedMap, ResourceMap};
use je_base::uri::URI;
use je_instantiation::{Injectable, InstantiationType, delayed_proxy, register_singleton, service_id, service_impl};
use je_platform::files::files::FileStatWithMetadata;

use super::working_copy::{IWorkingCopy, WorkingCopyIdentifier, WorkingCopySaveEvent as BaseWorkingCopySaveEvent};

/// `IWorkingCopySaveEvent` do serviço: o evento de salvar da working copy mais a working copy que foi salva.
#[derive(Clone)]
pub struct WorkingCopySaveEvent {
    /// The working copy that was saved.
    pub working_copy: Rc<dyn IWorkingCopy>,
    /// O `reason` e o `source` herdados de `IBaseWorkingCopySaveEvent`.
    pub event: BaseWorkingCopySaveEvent,
    /// O `stat` do `IStoredFileWorkingCopySaveEvent` (presente só quando a working copy é de arquivo armazenado, que é o que
    /// o `isStoredFileWorkingCopySaveEvent` do original testa).
    pub stat: Option<FileStatWithMetadata>,
}

/// `IWorkingCopyService`.
pub trait IWorkingCopyService {
    //#region Events

    /// An event for when a working copy was registered.
    fn on_did_register(&self) -> Event<Rc<dyn IWorkingCopy>>;

    /// An event for when a working copy was unregistered.
    fn on_did_unregister(&self) -> Event<Rc<dyn IWorkingCopy>>;

    /// An event for when a working copy dirty state changed.
    fn on_did_change_dirty(&self) -> Event<Rc<dyn IWorkingCopy>>;

    /// An event for when a working copy's content changed.
    fn on_did_change_content(&self) -> Event<Rc<dyn IWorkingCopy>>;

    /// An event for when a working copy was saved.
    fn on_did_save(&self) -> Event<WorkingCopySaveEvent>;

    //#endregion

    //#region Dirty Tracking

    /// The number of dirty working copies that are registered.
    fn dirty_count(&self) -> usize;

    /// All dirty working copies that are registered.
    fn dirty_working_copies(&self) -> Vec<Rc<dyn IWorkingCopy>>;

    /// The number of modified working copies that are registered, including scratchpads, which are never dirty.
    fn modified_count(&self) -> usize;

    /// All working copies with unsaved changes, including scratchpads, which are never dirty.
    fn modified_working_copies(&self) -> Vec<Rc<dyn IWorkingCopy>>;

    /// Whether there is any registered working copy that is dirty.
    fn has_dirty(&self) -> bool;

    /// Figure out if working copies with the given resource are dirty or not.
    ///
    /// - `resource`: the URI of the working copy.
    /// - `type_id`: optional type identifier to only consider working copies of that type.
    fn is_dirty(&self, resource: &URI, type_id: Option<&str>) -> bool;

    //#endregion

    //#region Registry

    /// All working copies that are registered.
    fn working_copies(&self) -> Vec<Rc<dyn IWorkingCopy>>;

    /// Register a new working copy with the service. This method will throw if you try to register a working copy on a
    /// resource that has already been registered.
    ///
    /// Overall there can only ever be 1 working copy with the same resource.
    fn register_working_copy(&self, working_copy: Rc<dyn IWorkingCopy>) -> DisposableRef;

    /// Whether a working copy with the given identifier exists.
    fn has(&self, identifier: &WorkingCopyIdentifier) -> bool;

    /// Whether a working copy with the given resource exists.
    fn has_resource(&self, resource: &URI) -> bool;

    /// Returns a working copy with the given identifier or `None` if no such working copy exists.
    fn get(&self, identifier: &WorkingCopyIdentifier) -> Option<Rc<dyn IWorkingCopy>>;

    /// Returns all working copies with the given resource or `None` if no such working copy exists.
    fn get_all(&self, resource: &URI) -> Option<Vec<Rc<dyn IWorkingCopy>>>;

    //#endregion
}

delayed_proxy!(pub WorkingCopyServiceProxy for dyn IWorkingCopyService {
    methods {
        fn dirty_count(&self) -> usize;
        fn dirty_working_copies(&self) -> Vec<Rc<dyn IWorkingCopy>>;
        fn modified_count(&self) -> usize;
        fn modified_working_copies(&self) -> Vec<Rc<dyn IWorkingCopy>>;
        fn has_dirty(&self) -> bool;
        fn is_dirty(&self, resource: &URI, type_id: Option<&str>) -> bool;
        fn working_copies(&self) -> Vec<Rc<dyn IWorkingCopy>>;
        fn register_working_copy(&self, working_copy: Rc<dyn IWorkingCopy>) -> DisposableRef;
        fn has(&self, identifier: &WorkingCopyIdentifier) -> bool;
        fn has_resource(&self, resource: &URI) -> bool;
        fn get(&self, identifier: &WorkingCopyIdentifier) -> Option<Rc<dyn IWorkingCopy>>;
        fn get_all(&self, resource: &URI) -> Option<Vec<Rc<dyn IWorkingCopy>>>;
    }
    events {
        fn on_did_register(&self) -> Rc<dyn IWorkingCopy>;
        fn on_did_unregister(&self) -> Rc<dyn IWorkingCopy>;
        fn on_did_change_dirty(&self) -> Rc<dyn IWorkingCopy>;
        fn on_did_change_content(&self) -> Rc<dyn IWorkingCopy>;
        fn on_did_save(&self) -> WorkingCopySaveEvent;
    }
});

service_id!(pub I_WORKING_COPY_SERVICE: dyn IWorkingCopyService = "workingCopyService", proxy WorkingCopyServiceProxy);

/// O `Map<string, IWorkingCopy>` das working copies de um recurso, por `typeId`, compartilhado entre o registro e o
/// fecho que descadastra.
type WorkingCopiesByType = Rc<RefCell<OrderedMap<String, Rc<dyn IWorkingCopy>>>>;

/// `WorkingCopyLeakError` não é uma classe aqui: é o [`Error`] com este nome e a pilha do dono mais frequente.
const LEAK_ERROR_NAME: &str = "WorkingCopyLeakError";

/// `WorkingCopyService.LEAK_TRACKING_THRESHOLD`.
const LEAK_TRACKING_THRESHOLD: usize = 256;

/// `WorkingCopyService.LEAK_REPORTING_THRESHOLD`.
const LEAK_REPORTING_THRESHOLD: usize = 2 * LEAK_TRACKING_THRESHOLD;

thread_local! {
    /// `WorkingCopyService.LEAK_REPORTED` (estático no original: vale para todas as instâncias).
    static LEAK_REPORTED: Cell<bool> = const { Cell::new(false) };
}

/// `WorkingCopyService`.
pub struct WorkingCopyService {
    this: Weak<WorkingCopyService>,
    base: Rc<Disposable>,

    //#region Events
    on_did_register: Rc<Emitter<Rc<dyn IWorkingCopy>>>,
    on_did_unregister: Rc<Emitter<Rc<dyn IWorkingCopy>>>,
    on_did_change_dirty: Rc<Emitter<Rc<dyn IWorkingCopy>>>,
    on_did_change_content: Rc<Emitter<Rc<dyn IWorkingCopy>>>,
    on_did_save: Rc<Emitter<WorkingCopySaveEvent>>,
    //#endregion

    //#region Registry
    working_copies: RefCell<Vec<Rc<dyn IWorkingCopy>>>,
    map_resource_to_working_copies: RefCell<ResourceMap<WorkingCopiesByType>>,
    map_working_copy_to_listeners: Rc<DisposableMap<ById<dyn IWorkingCopy>, DisposableStore>>,
    //#endregion

    /// `mapLeakToCounter`.
    map_leak_to_counter: RefCell<OrderedMap<String, usize>>,
}

impl WorkingCopyService {
    /// `new WorkingCopyService()`.
    pub fn new() -> Rc<Self> {
        Rc::new_cyclic(|this| {
            let base = Disposable::new();
            let on_did_register = Emitter::new();
            let on_did_unregister = Emitter::new();
            let on_did_change_dirty = Emitter::new();
            let on_did_change_content = Emitter::new();
            let on_did_save = Emitter::new();
            let map_working_copy_to_listeners: Rc<DisposableMap<ById<dyn IWorkingCopy>, DisposableStore>> = DisposableMap::new();
            base.register(on_did_register.clone());
            base.register(on_did_unregister.clone());
            base.register(on_did_change_dirty.clone());
            base.register(on_did_change_content.clone());
            base.register(on_did_save.clone());
            base.register(map_working_copy_to_listeners.clone());
            Self {
                this: this.clone(),
                base,
                on_did_register,
                on_did_unregister,
                on_did_change_dirty,
                on_did_change_content,
                on_did_save,
                working_copies: RefCell::new(Vec::new()),
                map_resource_to_working_copies: RefCell::new(ResourceMap::new()),
                map_working_copy_to_listeners,
                map_leak_to_counter: RefCell::new(OrderedMap::new()),
            }
        })
    }

    /// `unregisterWorkingCopy` (protegido no original).
    fn unregister_working_copy(&self, working_copy: &Rc<dyn IWorkingCopy>) {
        // Registry (all)
        self.working_copies.borrow_mut().retain(|candidate| id_of(candidate) != id_of(working_copy));

        // Registry (type based)
        let resource = working_copy.resource();
        let working_copies_for_resource = self.map_resource_to_working_copies.borrow().get(&resource).cloned();
        if let Some(working_copies_for_resource) = working_copies_for_resource {
            let deleted = working_copies_for_resource.borrow_mut().remove(&working_copy.working_copy_type_id()).is_some();
            if deleted && working_copies_for_resource.borrow().is_empty() {
                self.map_resource_to_working_copies.borrow_mut().delete(&resource);
            }
        }

        // If copy is dirty, ensure to fire an event to signal the dirty change
        // (a disposed working copy cannot account for being dirty in our model)
        if working_copy.is_dirty() {
            self.on_did_change_dirty.fire(working_copy.clone());
        }

        // Remove all listeners associated to working copy
        self.map_working_copy_to_listeners.delete_and_dispose(&ById(working_copy.clone()));
    }

    //#region Leak Monitoring

    fn track_leaks(&self, working_copy: &Rc<dyn IWorkingCopy>) -> Option<String> {
        let tracked = self.working_copies.borrow().len();
        if LEAK_REPORTED.with(Cell::get) || tracked < LEAK_TRACKING_THRESHOLD {
            return None;
        }

        let type_id = working_copy.working_copy_type_id();
        let stack = StackSource::capture().stack().lines().skip(2).collect::<Vec<_>>().join("\n");
        let leak_id = format!(
            "{}#{}\n{}",
            working_copy.resource().scheme().to_string_lossy(),
            if type_id.is_empty() { "<no typeId>" } else { type_id.as_str() },
            stack
        );
        {
            let mut counters = self.map_leak_to_counter.borrow_mut();
            let leak_counter = counters.get(&leak_id).copied().unwrap_or(0) + 1;
            counters.insert(leak_id.clone(), leak_counter);
        }

        if tracked > LEAK_REPORTING_THRESHOLD {
            LEAK_REPORTED.with(|reported| reported.set(true));

            let (top_leak, top_count) = {
                let counters = self.map_leak_to_counter.borrow();
                let mut entries = counters.iter();
                let first = entries.next().map(|(key, value)| (key.clone(), *value)).expect("o contador do vazamento acabou de ser gravado");
                entries.fold(first, |(top_leak, top_count), (key, value)| if *value > top_count { (key.clone(), *value) } else { (top_leak, top_count) })
            };

            let message = format!("Potential working copy LEAK detected, having {tracked} working copies already. Most frequent owner ({top_count})");
            errors::on_unexpected_error(&Error::named(LEAK_ERROR_NAME, message).with_stack(top_leak));
        }

        Some(leak_id)
    }

    fn untrack_leaks(&self, leak_id: &str) {
        let mut counters = self.map_leak_to_counter.borrow_mut();
        let stack_counter = counters.get(&leak_id.to_string()).copied().unwrap_or(1) - 1;
        counters.insert(leak_id.to_string(), stack_counter);

        if stack_counter == 0 {
            counters.remove(&leak_id.to_string());
        }
    }

    //#endregion
}

impl Injectable for WorkingCopyService {
    type Args = ();
    type Deps = ();

    fn create(_: (), _: ()) -> Rc<Self> {
        Self::new()
    }
}

service_impl!(WorkingCopyService => dyn IWorkingCopyService, disposable);

/// `registerSingleton(IWorkingCopyService, WorkingCopyService, InstantiationType.Delayed)`.
pub fn register() {
    register_singleton::<WorkingCopyService>(I_WORKING_COPY_SERVICE, InstantiationType::Delayed);
}

impl IDisposable for WorkingCopyService {
    fn dispose(&self) {
        self.base.dispose();
    }
}

impl IWorkingCopyService for WorkingCopyService {
    fn on_did_register(&self) -> Event<Rc<dyn IWorkingCopy>> {
        self.on_did_register.event()
    }

    fn on_did_unregister(&self) -> Event<Rc<dyn IWorkingCopy>> {
        self.on_did_unregister.event()
    }

    fn on_did_change_dirty(&self) -> Event<Rc<dyn IWorkingCopy>> {
        self.on_did_change_dirty.event()
    }

    fn on_did_change_content(&self) -> Event<Rc<dyn IWorkingCopy>> {
        self.on_did_change_content.event()
    }

    fn on_did_save(&self) -> Event<WorkingCopySaveEvent> {
        self.on_did_save.event()
    }

    fn working_copies(&self) -> Vec<Rc<dyn IWorkingCopy>> {
        self.working_copies.borrow().clone()
    }

    fn register_working_copy(&self, working_copy: Rc<dyn IWorkingCopy>) -> DisposableRef {
        let resource = working_copy.resource();
        let type_id = working_copy.working_copy_type_id();

        let existing = self.map_resource_to_working_copies.borrow().get(&resource).cloned();
        if existing.as_ref().is_some_and(|working_copies_for_resource| working_copies_for_resource.borrow().contains_key(&type_id)) {
            errors::throw(Error::new(format!(
                "Cannot register more than one working copy with the same resource {} and type {}.",
                resource.to_string(false).to_string_lossy(),
                type_id
            )));
        }

        // Registry (all)
        self.working_copies.borrow_mut().push(working_copy.clone());

        // Registry (type based)
        let working_copies_for_resource = existing.unwrap_or_else(|| {
            let created: WorkingCopiesByType = Rc::new(RefCell::new(OrderedMap::new()));
            self.map_resource_to_working_copies.borrow_mut().set(resource.clone(), created.clone());
            created
        });
        working_copies_for_resource.borrow_mut().insert(type_id, working_copy.clone());

        // Wire in Events
        let disposables = DisposableStore::new();
        let (on_did_change_content, on_did_change_dirty, on_did_save) =
            (self.on_did_change_content.clone(), self.on_did_change_dirty.clone(), self.on_did_save.clone());
        let (content_copy, dirty_copy, save_copy) = (working_copy.clone(), working_copy.clone(), working_copy.clone());
        working_copy.on_did_change_content().listen_in(move |_| on_did_change_content.fire(content_copy.clone()), &disposables);
        working_copy.on_did_change_dirty().listen_in(move |_| on_did_change_dirty.fire(dirty_copy.clone()), &disposables);
        // O `onDidSave` com o `stat` (arquivo armazenado) ou o evento base (as demais working copies)
        match working_copy.stored_file_save_event() {
            Some(stored_event) => {
                stored_event.listen_in(
                    move |e| on_did_save.fire(WorkingCopySaveEvent { working_copy: save_copy.clone(), event: e.event.clone(), stat: Some(e.stat.clone()) }),
                    &disposables,
                );
            }
            None => {
                working_copy.on_did_save().listen_in(
                    move |e| on_did_save.fire(WorkingCopySaveEvent { working_copy: save_copy.clone(), event: e.clone(), stat: None }),
                    &disposables,
                );
            }
        }
        self.map_working_copy_to_listeners.set(ById(working_copy.clone()), disposables, false);

        // Send some initial events
        self.on_did_register.fire(working_copy.clone());
        if working_copy.is_dirty() {
            self.on_did_change_dirty.fire(working_copy.clone());
        }

        // Track Leaks
        let leak_id = self.track_leaks(&working_copy);

        let this = self.this.clone();
        to_disposable(move || {
            let Some(this) = this.upgrade() else {
                return;
            };

            // Untrack Leaks
            if let Some(leak_id) = &leak_id {
                this.untrack_leaks(leak_id);
            }

            // Unregister working copy
            this.unregister_working_copy(&working_copy);

            // Signal as event
            this.on_did_unregister.fire(working_copy.clone());
        })
    }

    fn has(&self, identifier: &WorkingCopyIdentifier) -> bool {
        self.get(identifier).is_some()
    }

    fn has_resource(&self, resource: &URI) -> bool {
        self.map_resource_to_working_copies.borrow().has(resource)
    }

    fn get(&self, identifier: &WorkingCopyIdentifier) -> Option<Rc<dyn IWorkingCopy>> {
        let working_copies_for_resource = self.map_resource_to_working_copies.borrow().get(&identifier.resource).cloned()?;
        let working_copy = working_copies_for_resource.borrow().get(&identifier.type_id).cloned();
        working_copy
    }

    fn get_all(&self, resource: &URI) -> Option<Vec<Rc<dyn IWorkingCopy>>> {
        let working_copies = self.map_resource_to_working_copies.borrow().get(resource).cloned()?;
        let all = working_copies.borrow().values().cloned().collect();
        Some(all)
    }

    //#region Dirty Tracking

    fn has_dirty(&self) -> bool {
        self.working_copies().iter().any(|working_copy| working_copy.is_dirty())
    }

    fn dirty_count(&self) -> usize {
        self.working_copies().iter().filter(|working_copy| working_copy.is_dirty()).count()
    }

    fn dirty_working_copies(&self) -> Vec<Rc<dyn IWorkingCopy>> {
        self.working_copies().into_iter().filter(|working_copy| working_copy.is_dirty()).collect()
    }

    fn modified_count(&self) -> usize {
        self.working_copies().iter().filter(|working_copy| working_copy.is_modified()).count()
    }

    fn modified_working_copies(&self) -> Vec<Rc<dyn IWorkingCopy>> {
        self.working_copies().into_iter().filter(|working_copy| working_copy.is_modified()).collect()
    }

    fn is_dirty(&self, resource: &URI, type_id: Option<&str>) -> bool {
        let Some(working_copies) = self.get_all(resource) else {
            return false;
        };

        match type_id {
            // For a specific type
            Some(type_id) => working_copies.iter().any(|working_copy| working_copy.working_copy_type_id() == type_id && working_copy.is_dirty()),

            // Across all working copies
            None => working_copies.iter().any(|working_copy| working_copy.is_dirty()),
        }
    }

    //#endregion
}
