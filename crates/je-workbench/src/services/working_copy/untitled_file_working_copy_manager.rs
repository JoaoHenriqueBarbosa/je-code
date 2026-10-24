//! Porte de `src/vs/workbench/services/workingCopy/common/untitledFileWorkingCopyManager.ts`: o
//! `UntitledFileWorkingCopyManager`, o único que lida com as `IUntitledFileWorkingCopy` e com tudo que é operação de
//! working copy (salvar, reverter, backup e resolver). Cria working copies sem título com um recurso `untitled:` novo
//! (`Untitled-N`, `Scratchpad-N`, com o `typeId` na consulta quando há um) ou devolve a que já existe para o recurso,
//! guarda por recurso e repassa os eventos de sujeira e de descarte.
//!
//! Divergências:
//! - **Injeção.** Os 8 parâmetros do construtor são o `workingCopyTypeId`, a fábrica do modelo, o delegado de salvar e os
//!   cinco serviços (`IFileService`, `ILabelService`, `ILogService`, `IWorkingCopyBackupService`, `IWorkingCopyService`).
//! - **Herança.** `extends BaseFileWorkingCopyManager` é a composição com [`BaseFileWorkingCopyManager`] (campo `base`,
//!   exposto por [`IBaseFileWorkingCopyManager::base_manager`]); o `_register(...)` é um [`Disposable`] próprio (campo
//!   `store`). A sobrescrita de `remove` é instalada por [`BaseFileWorkingCopyManager::set_remove_override`], e o
//!   `dispose()` descarta o `base` por [`BaseFileWorkingCopyManager::dispose_base`].
//! - **Genéricos.** O gerenciador é genérico no modelo (`M`), como no original, e devolve a working copy concreta
//!   (`Rc<UntitledFileWorkingCopy<M>>`); `IUntitledFileWorkingCopyManager<M>` é o trait [`IUntitledFileWorkingCopyManager`]
//!   sobre essa working copy.
//! - **Sobrecargas de `resolve`.** As três sobrecargas (`INewUntitledFileWorkingCopyOptions`,
//!   `INewUntitledFileWorkingCopyWithAssociatedResourceOptions` e `INewOrExistingUntitledFileWorkingCopyOptions`) são
//!   as três structs de mesmo nome, e a interseção delas (`IInternalUntitledFileWorkingCopyOptions`, o tipo da
//!   implementação) é [`UntitledFileWorkingCopyResolveOptions`], para a qual cada uma converte por `From`. O argumento
//!   ausente (`options?`) é o `Option`.
//! - **`associatedResource`.** O objeto de componentes opcionais é o [`AssociatedResource`] do serviço de editores sem
//!   título (componente ausente é a string vazia, que é o que `URI.from` faz com `undefined`).
//! - **Escutas.** As escutas que o gerenciador instala na working copy seguram a working copy por `Weak`, para o ciclo
//!   `Rc` não vazar (no JavaScript o coletor de lixo cuida disso).

use std::cell::RefCell;
use std::rc::{Rc, Weak};

use je_base::JsString;
use je_base::async_::async_fn;
use je_base::event::{Emitter, Event};
use je_base::lifecycle::{Disposable, DisposableRef, DisposableStore, IDisposable};
use je_base::map::ResourceMap;
use je_base::network::schemas;
use je_base::promise::Promise;
use je_base::uri::{URI, UriComponents};
use je_platform::files::i_file_service::IFileService;
use je_platform::label::label::ILabelService;
use je_platform::log::LogService;

use crate::services::untitled::untitled_text_editor_service::AssociatedResource;
use crate::services::working_copy::abstract_file_working_copy_manager::{BaseFileWorkingCopyManager, IBaseFileWorkingCopyManager};
use crate::services::working_copy::file_working_copy::IFileWorkingCopy;
use crate::services::working_copy::untitled_file_working_copy::{
    IUntitledFileWorkingCopy, IUntitledFileWorkingCopyModel, IUntitledFileWorkingCopyModelFactory, UntitledFileWorkingCopy, UntitledFileWorkingCopyInitialContents,
    UntitledFileWorkingCopySaveDelegate,
};
use crate::services::working_copy::working_copy::IWorkingCopy;
use crate::services::working_copy::working_copy_backup::IWorkingCopyBackupService;
use crate::services::working_copy::working_copy_service::IWorkingCopyService;

/// `IUntitledFileWorkingCopySaveEvent`.
#[derive(Clone, Debug, PartialEq)]
pub struct UntitledFileWorkingCopySaveEvent {
    /// The source untitled file working copy that was saved. It is disposed at this point.
    pub source: URI,
    /// The target file working copy the untitled was saved to. Is never untitled.
    pub target: URI,
}

/// `INewUntitledFileWorkingCopyOptions`.
#[derive(Clone, Default)]
pub struct NewUntitledFileWorkingCopyOptions {
    /// Initial value of the untitled file working copy with support to indicate whether this should turn the working copy
    /// dirty or not.
    pub contents: Option<UntitledFileWorkingCopyInitialContents>,
}

/// `INewUntitledFileWorkingCopyWithAssociatedResourceOptions`.
#[derive(Clone)]
pub struct NewUntitledFileWorkingCopyWithAssociatedResourceOptions {
    /// O `INewUntitledFileWorkingCopyOptions` de que herda.
    pub options: NewUntitledFileWorkingCopyOptions,
    /// Resource components to associate with the untitled file working copy. When saving, the associated components will
    /// be used and the user is not being asked to provide a file path.
    ///
    /// Note: currently it is not possible to specify the `scheme` to use. The untitled file working copy will saved to the
    /// default local or remote resource.
    pub associated_resource: AssociatedResource,
}

/// `INewOrExistingUntitledFileWorkingCopyOptions`.
#[derive(Clone)]
pub struct NewOrExistingUntitledFileWorkingCopyOptions {
    /// O `INewUntitledFileWorkingCopyOptions` de que herda.
    pub options: NewUntitledFileWorkingCopyOptions,
    /// A resource to identify the untitled file working copy to create or return if already existing.
    ///
    /// Note: the resource will not be used unless the scheme is `untitled`.
    pub untitled_resource: URI,
    /// A flag that will prevent the working copy from appearing dirty in the UI and not show a confirmation dialog when
    /// closed with unsaved content.
    pub is_scratchpad: Option<bool>,
}

/// `IInternalUntitledFileWorkingCopyOptions`: a interseção das três opções de `resolve`.
#[derive(Clone, Default)]
pub struct UntitledFileWorkingCopyResolveOptions {
    pub contents: Option<UntitledFileWorkingCopyInitialContents>,
    pub associated_resource: Option<AssociatedResource>,
    pub untitled_resource: Option<URI>,
    pub is_scratchpad: Option<bool>,
}

impl From<NewUntitledFileWorkingCopyOptions> for UntitledFileWorkingCopyResolveOptions {
    fn from(options: NewUntitledFileWorkingCopyOptions) -> Self {
        Self { contents: options.contents, ..Self::default() }
    }
}

impl From<NewUntitledFileWorkingCopyWithAssociatedResourceOptions> for UntitledFileWorkingCopyResolveOptions {
    fn from(options: NewUntitledFileWorkingCopyWithAssociatedResourceOptions) -> Self {
        Self { contents: options.options.contents, associated_resource: Some(options.associated_resource), ..Self::default() }
    }
}

impl From<NewOrExistingUntitledFileWorkingCopyOptions> for UntitledFileWorkingCopyResolveOptions {
    fn from(options: NewOrExistingUntitledFileWorkingCopyOptions) -> Self {
        Self { contents: options.options.contents, untitled_resource: Some(options.untitled_resource), is_scratchpad: options.is_scratchpad, ..Self::default() }
    }
}

/// `IUntitledFileWorkingCopyManager<M>`: the only one that should be dealing with `IUntitledFileWorkingCopy` and handle all
/// operations that are working copy related, such as save/revert, backup and resolving.
pub trait IUntitledFileWorkingCopyManager<M: IUntitledFileWorkingCopyModel + 'static>: IBaseFileWorkingCopyManager<UntitledFileWorkingCopy<M>> {
    /// An event for when an untitled file working copy was saved. At the point the event fires, the untitled file working
    /// copy is disposed.
    fn on_did_save(&self) -> Event<UntitledFileWorkingCopySaveEvent>;

    /// An event for when a untitled file working copy changed it's dirty state.
    fn on_did_change_dirty(&self) -> Event<Rc<UntitledFileWorkingCopy<M>>>;

    /// An event for when a untitled file working copy is about to be disposed.
    fn on_will_dispose(&self) -> Event<Rc<UntitledFileWorkingCopy<M>>>;

    /// Create a new untitled file working copy with optional initial contents (as
    /// [`NewUntitledFileWorkingCopyOptions`]), with optional initial contents and associated resource (as
    /// [`NewUntitledFileWorkingCopyWithAssociatedResourceOptions`]: the associated resource will be used when saving and
    /// will not require to ask the user for a file path), or with the provided resource, returning an existing untitled
    /// file working copy otherwise (as [`NewOrExistingUntitledFileWorkingCopyOptions`]).
    ///
    /// Note: Callers must `dispose` the working copy when no longer needed.
    fn resolve(&self, options: Option<UntitledFileWorkingCopyResolveOptions>) -> Promise<Rc<UntitledFileWorkingCopy<M>>>;

    /// Internal method: triggers the onDidSave event.
    fn notify_did_save(&self, source: &URI, target: &URI);
}

/// `UntitledFileWorkingCopyManager<M>`.
pub struct UntitledFileWorkingCopyManager<M: IUntitledFileWorkingCopyModel + 'static> {
    this: Weak<UntitledFileWorkingCopyManager<M>>,
    /// O `BaseFileWorkingCopyManager` de que a classe herda.
    base: Rc<BaseFileWorkingCopyManager<UntitledFileWorkingCopy<M>>>,
    /// O `_register(...)` da classe.
    store: Rc<Disposable>,

    //#region Events
    on_did_save: Rc<Emitter<UntitledFileWorkingCopySaveEvent>>,
    on_did_change_dirty: Rc<Emitter<Rc<UntitledFileWorkingCopy<M>>>>,
    on_will_dispose: Rc<Emitter<Rc<UntitledFileWorkingCopy<M>>>>,
    //#endregion
    map_resource_to_working_copy_listeners: RefCell<ResourceMap<DisposableRef>>,

    working_copy_type_id: String,
    model_factory: Rc<dyn IUntitledFileWorkingCopyModelFactory<M>>,
    save_delegate: UntitledFileWorkingCopySaveDelegate<M>,
    label_service: Rc<dyn ILabelService>,
    log_service: Rc<dyn LogService>,
    working_copy_backup_service: Rc<dyn IWorkingCopyBackupService>,
    working_copy_service: Rc<dyn IWorkingCopyService>,
}

impl<M: IUntitledFileWorkingCopyModel + 'static> UntitledFileWorkingCopyManager<M> {
    /// `constructor(workingCopyTypeId, modelFactory, saveDelegate, ...services)`.
    pub fn new(
        working_copy_type_id: String,
        model_factory: Rc<dyn IUntitledFileWorkingCopyModelFactory<M>>,
        save_delegate: UntitledFileWorkingCopySaveDelegate<M>,
        file_service: Rc<dyn IFileService>,
        label_service: Rc<dyn ILabelService>,
        log_service: Rc<dyn LogService>,
        working_copy_backup_service: Rc<dyn IWorkingCopyBackupService>,
        working_copy_service: Rc<dyn IWorkingCopyService>,
    ) -> Rc<Self> {
        let base = BaseFileWorkingCopyManager::new(
            Rc::new(|working_copy: &UntitledFileWorkingCopy<M>| IUntitledFileWorkingCopy::on_will_dispose(working_copy)),
            file_service,
            log_service.clone(),
            working_copy_backup_service.clone(),
        );
        let store = Disposable::new();

        let on_did_save = Emitter::new();
        let on_did_change_dirty = Emitter::new();
        let on_will_dispose = Emitter::new();
        store.register(on_did_save.clone());
        store.register(on_did_change_dirty.clone());
        store.register(on_will_dispose.clone());

        let this = Rc::new_cyclic(|this| Self {
            this: this.clone(),
            base,
            store,
            on_did_save,
            on_did_change_dirty,
            on_will_dispose,
            map_resource_to_working_copy_listeners: RefCell::new(ResourceMap::new()),
            working_copy_type_id,
            model_factory,
            save_delegate,
            label_service,
            log_service,
            working_copy_backup_service,
            working_copy_service,
        });

        let weak = this.this.clone();
        this.base.set_remove_override(move |resource| weak.upgrade().is_some_and(|this| this.remove(resource)));

        this
    }

    //#region Resolve

    fn do_create_or_get(&self, options: UntitledFileWorkingCopyResolveOptions) -> Rc<UntitledFileWorkingCopy<M>> {
        let massaged_options = self.massage_options(&options);

        // Return existing instance if asked for it
        if let Some(untitled_resource) = &massaged_options.untitled_resource
            && let Some(existing_working_copy) = self.base.get(untitled_resource)
        {
            return existing_working_copy;
        }

        // Create new instance otherwise
        self.do_create(massaged_options)
    }

    fn massage_options(&self, options: &UntitledFileWorkingCopyResolveOptions) -> UntitledFileWorkingCopyResolveOptions {
        let mut massaged_options = UntitledFileWorkingCopyResolveOptions::default();

        // Handle associated resource
        if let Some(associated_resource) = &options.associated_resource {
            massaged_options.untitled_resource = Some(URI::from(
                &UriComponents {
                    scheme: Some(JsString::from(schemas::UNTITLED)),
                    authority: Some(JsString::from(associated_resource.authority.as_str())),
                    fragment: Some(JsString::from(associated_resource.fragment.as_str())),
                    path: Some(JsString::from(associated_resource.path.as_str())),
                    query: Some(JsString::from(associated_resource.query.as_str())),
                },
                false,
            ));
            massaged_options.associated_resource = Some(associated_resource.clone());
        }
        // Handle untitled resource
        else {
            if let Some(untitled_resource) = &options.untitled_resource
                && *untitled_resource.scheme() == schemas::UNTITLED
            {
                massaged_options.untitled_resource = Some(untitled_resource.clone());
            }
            massaged_options.is_scratchpad = options.is_scratchpad;
        }

        // Take over initial value
        massaged_options.contents = options.contents.clone();

        massaged_options
    }

    fn do_create(&self, options: UntitledFileWorkingCopyResolveOptions) -> Rc<UntitledFileWorkingCopy<M>> {
        let is_scratchpad = options.is_scratchpad.unwrap_or(false);

        // Create a new untitled resource if none is provided
        let untitled_resource = match options.untitled_resource.clone() {
            Some(untitled_resource) => untitled_resource,
            None => {
                let mut counter = 1;
                loop {
                    let candidate = URI::from(
                        &UriComponents {
                            scheme: Some(JsString::from(schemas::UNTITLED)),
                            path: Some(JsString::from(if is_scratchpad { format!("Scratchpad-{counter}") } else { format!("Untitled-{counter}") })),
                            // distinguish untitled resources among others by encoding the `typeId` as query param,
                            // and keep untitled resources for text files as they are (when `typeId === ''`)
                            query: (!self.working_copy_type_id.is_empty()).then(|| JsString::from(format!("typeId={}", self.working_copy_type_id))),
                            ..UriComponents::default()
                        },
                        false,
                    );
                    counter += 1;

                    if !self.base.has(&candidate) {
                        break candidate;
                    }
                }
            }
        };

        // Create new working copy with provided options
        let working_copy = UntitledFileWorkingCopy::new(
            self.working_copy_type_id.clone(),
            untitled_resource.clone(),
            self.label_service.get_uri_basename_label(&untitled_resource),
            options.associated_resource.is_some(),
            is_scratchpad,
            options.contents,
            self.model_factory.clone(),
            self.save_delegate.clone(),
            self.working_copy_service.clone(),
            self.working_copy_backup_service.clone(),
            self.log_service.clone(),
        );

        // Register
        self.register_working_copy(&working_copy);

        working_copy
    }

    fn register_working_copy(&self, working_copy: &Rc<UntitledFileWorkingCopy<M>>) {
        // Install working copy listeners
        let working_copy_listeners = DisposableStore::new();
        let weak_working_copy = Rc::downgrade(working_copy);

        let (on_did_change_dirty, weak) = (self.on_did_change_dirty.clone(), weak_working_copy.clone());
        working_copy_listeners.add(working_copy.on_did_change_dirty().listen(move |_| {
            if let Some(working_copy) = weak.upgrade() {
                on_did_change_dirty.fire(working_copy);
            }
        }));
        let (on_will_dispose, weak) = (self.on_will_dispose.clone(), weak_working_copy);
        working_copy_listeners.add(IUntitledFileWorkingCopy::on_will_dispose(&**working_copy).listen(move |_| {
            if let Some(working_copy) = weak.upgrade() {
                on_will_dispose.fire(working_copy);
            }
        }));

        // Keep for disposal
        self.map_resource_to_working_copy_listeners.borrow_mut().set(working_copy.resource(), working_copy_listeners);

        // Add to cache
        self.base.add(&working_copy.resource(), working_copy);

        // If the working copy is dirty right from the beginning,
        // make sure to emit this as an event
        if working_copy.is_dirty() {
            self.on_did_change_dirty.fire(working_copy.clone());
        }
    }

    /// `remove(resource)` (sobrescrita de `protected`).
    fn remove(&self, resource: &URI) -> bool {
        let removed = self.base.remove_base(resource);

        // Dispose any existing working copy listeners
        let working_copy_listener = self.map_resource_to_working_copy_listeners.borrow().get(resource).cloned();
        if let Some(working_copy_listener) = working_copy_listener {
            working_copy_listener.dispose();
            self.map_resource_to_working_copy_listeners.borrow_mut().delete(resource);
        }

        removed
    }

    //#endregion
}

impl<M: IUntitledFileWorkingCopyModel + 'static> IDisposable for UntitledFileWorkingCopyManager<M> {
    //#region Lifecycle

    fn dispose(&self) {
        self.base.dispose_base();
        self.store.dispose();

        // Dispose the working copy change listeners
        let working_copy_listeners: Vec<DisposableRef> = self.map_resource_to_working_copy_listeners.borrow().values().cloned().collect();
        for working_copy_listener in working_copy_listeners {
            working_copy_listener.dispose();
        }
        self.map_resource_to_working_copy_listeners.borrow_mut().clear();
    }

    //#endregion
}

impl<M: IUntitledFileWorkingCopyModel + 'static> IBaseFileWorkingCopyManager<UntitledFileWorkingCopy<M>> for UntitledFileWorkingCopyManager<M> {
    fn base_manager(&self) -> &BaseFileWorkingCopyManager<UntitledFileWorkingCopy<M>> {
        &self.base
    }

    fn destroy(&self) -> Promise<()> {
        let this = self.this.upgrade().expect("o gerenciador está vivo enquanto é usado");
        self.base.destroy(move || this.dispose())
    }
}

impl<M: IUntitledFileWorkingCopyModel + 'static> IUntitledFileWorkingCopyManager<M> for UntitledFileWorkingCopyManager<M> {
    fn on_did_save(&self) -> Event<UntitledFileWorkingCopySaveEvent> {
        self.on_did_save.event()
    }

    fn on_did_change_dirty(&self) -> Event<Rc<UntitledFileWorkingCopy<M>>> {
        self.on_did_change_dirty.event()
    }

    fn on_will_dispose(&self) -> Event<Rc<UntitledFileWorkingCopy<M>>> {
        self.on_will_dispose.event()
    }

    fn resolve(&self, options: Option<UntitledFileWorkingCopyResolveOptions>) -> Promise<Rc<UntitledFileWorkingCopy<M>>> {
        let working_copy = self.do_create_or_get(options.unwrap_or_default());

        async_fn(async move {
            working_copy.resolve().await?;

            Ok(working_copy)
        })
    }

    fn notify_did_save(&self, source: &URI, target: &URI) {
        self.on_did_save.fire(UntitledFileWorkingCopySaveEvent { source: source.clone(), target: target.clone() });
    }
}
