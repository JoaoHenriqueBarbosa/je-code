//! Porte de `src/vs/workbench/services/workingCopy/common/storedFileWorkingCopyManager.ts`: o
//! `StoredFileWorkingCopyManager`, o único que lida com as `IStoredFileWorkingCopy` e com tudo que é operação de working
//! copy (salvar, reverter, backup e resolver). Guarda as working copies por recurso, resolve sem nunca resolver a mesma
//! duas vezes em paralelo, recarrega quando o arquivo muda no disco, acompanha mover, copiar e criar arquivos, prolonga o
//! desligamento enquanto há salvamento pendente e diz quando uma working copy pode ser descartada.
//!
//! Divergências:
//! - **Injeção.** Os 16 parâmetros do construtor são o `workingCopyTypeId`, a fábrica do modelo, o
//!   [`StoredFileWorkingCopyServices`] (os 11 serviços que a working copy recebe, entre eles os que o gerenciador lê:
//!   `IFileService`, `ILogService`, `IWorkingCopyFileService`...) e os três que só o gerenciador lê: `ILifecycleService`,
//!   `ILabelService` e `IUriIdentityService`.
//! - **Herança.** `extends BaseFileWorkingCopyManager` é a composição com [`BaseFileWorkingCopyManager`] (campo `base`,
//!   exposto por [`IBaseFileWorkingCopyManager::base_manager`]); o `_register(...)` é um [`Disposable`] próprio (campo
//!   `store`). A sobrescrita de `remove` é instalada por [`BaseFileWorkingCopyManager::set_remove_override`], e o
//!   `dispose()` descarta o `base` por [`BaseFileWorkingCopyManager::dispose_base`].
//! - **Genéricos.** O gerenciador é genérico no modelo (`M`), como no original, e devolve a working copy concreta
//!   (`Rc<StoredFileWorkingCopy<M>>`): o modelo tipado sai de [`StoredFileWorkingCopy::model`]. `IStoredFileWorkingCopyManager`
//!   é o trait [`IStoredFileWorkingCopyManager`] sobre essa working copy.
//! - **`IStoredFileWorkingCopyManagerResolveOptions`** é [`StoredFileWorkingCopyManagerResolveOptions`]: o nível base
//!   (`IStoredFileWorkingCopyResolveOptions`) em `options` e o `reload` ao lado.
//! - **`IStoredFileWorkingCopySaveEvent<M>`** é [`StoredFileWorkingCopyManagerSaveEvent`]: o `workingCopy` e o evento base
//!   (o `{ workingCopy, ...e }` do original).
//! - **`canDispose(workingCopy): true | Promise<true>`** é `Option<Promise<()>>`: `None` é o `true` imediato (como no
//!   `TextFileEditorModelManager`).
//! - **`waitUntil`** do evento de operação de working copy é [`WorkingCopyFileEvent::wait_until`].
//! - **`isWeb`** é `Platform::detect().is_web`; `event.veto(bool, id)` é [`BeforeShutdownEvent::veto`] com o valor direto.
//! - **Resolvedor externo.** O fecho que a working copy recebe (`async options => { await this.resolve(...) }`) segura o
//!   gerenciador por `Weak`, e as escutas que o gerenciador instala na working copy seguram a working copy por `Weak`,
//!   para o ciclo `Rc` não vazar (no JavaScript o coletor de lixo cuida disso).
//! - **`onUnexpectedError`** é [`on_unexpected_error`]; `Promises.settled` é [`promises::settled`] e `Event.toPromise` é
//!   [`Event::to_promise`].

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::{Rc, Weak};

use je_base::async_::{ResourceQueue, async_fn, promises};
use je_base::buffer::VSBufferReadableStream;
use je_base::cancellation::CancellationToken;
use je_base::errors::on_unexpected_error;
use je_base::event::{Emitter, Event};
use je_base::lifecycle::{Disposable, DisposableRef, DisposableStore, IDisposable};
use je_base::map::ResourceMap;
use je_base::nls::localize;
use je_base::platform::Platform;
use je_base::promise::{MaybePromise, Promise};
use je_base::uri::URI;
use je_platform::files::files::{FileChangeType, FileChangesEvent, FileOperation, FileSystemProviderCapabilitiesChangeEvent, FileSystemProviderRegistrationEvent};
use je_platform::label::label::ILabelService;
use je_platform::resources::ext_uri;
use je_platform::uri_identity::uri_identity::IUriIdentityService;

use crate::services::lifecycle::lifecycle::{ILifecycleService, WillShutdownEventJoiner};
use crate::services::textfile::collaborators::WorkingCopyFileEvent;
use crate::services::working_copy::abstract_file_working_copy_manager::{BaseFileWorkingCopyManager, IBaseFileWorkingCopyManager};
use crate::services::working_copy::file_working_copy::{IFileWorkingCopy, IFileWorkingCopyModel, SnapshotContext};
use crate::services::working_copy::resource_working_copy::IResourceWorkingCopy;
use crate::services::working_copy::stored_file_working_copy::{
    IStoredFileWorkingCopy, IStoredFileWorkingCopyModel, IStoredFileWorkingCopyModelFactory, StoredFileWorkingCopy, StoredFileWorkingCopyResolveOptions,
    StoredFileWorkingCopyResolver, StoredFileWorkingCopySaveEvent, StoredFileWorkingCopyServices, StoredFileWorkingCopyState,
};
use crate::services::working_copy::working_copy::IWorkingCopy;

/// `IStoredFileWorkingCopyManagerResolveOptions.reload`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StoredFileWorkingCopyReloadOptions {
    /// Controls whether the reload happens in the background or whether `resolve` will await the reload to happen.
    pub async_: bool,
    /// Controls whether to force reading the contents from the underlying resource even if the resource did not change.
    pub force: Option<bool>,
}

/// `IStoredFileWorkingCopyManagerResolveOptions`.
#[derive(Clone, Default)]
pub struct StoredFileWorkingCopyManagerResolveOptions {
    /// O `IStoredFileWorkingCopyResolveOptions` de que herda (`contents`, `forceReadFromFile`, `limits`).
    pub options: StoredFileWorkingCopyResolveOptions,
    /// If the stored file working copy was already resolved before, allows to trigger a reload of it to fetch the latest
    /// contents.
    pub reload: Option<StoredFileWorkingCopyReloadOptions>,
}

/// `IStoredFileWorkingCopySaveEvent<M>`.
pub struct StoredFileWorkingCopyManagerSaveEvent<M: IStoredFileWorkingCopyModel + 'static> {
    /// The stored file working copy that was successfully saved.
    pub working_copy: Rc<StoredFileWorkingCopy<M>>,
    /// O `IStoredFileWorkingCopySaveEvent` base: o evento de salvar mais o `stat` resolvido.
    pub event: StoredFileWorkingCopySaveEvent,
}

impl<M: IStoredFileWorkingCopyModel + 'static> Clone for StoredFileWorkingCopyManagerSaveEvent<M> {
    fn clone(&self) -> Self {
        Self { working_copy: self.working_copy.clone(), event: self.event.clone() }
    }
}

/// `IStoredFileWorkingCopyManager<M>`: the only one that should be dealing with `IStoredFileWorkingCopy` and handle all
/// operations that are working copy related, such as save/revert, backup and resolving.
pub trait IStoredFileWorkingCopyManager<M: IStoredFileWorkingCopyModel + 'static>: IBaseFileWorkingCopyManager<StoredFileWorkingCopy<M>> {
    /// An event for when a stored file working copy was resolved.
    fn on_did_resolve(&self) -> Event<Rc<StoredFileWorkingCopy<M>>>;

    /// An event for when a stored file working copy changed it's dirty state.
    fn on_did_change_dirty(&self) -> Event<Rc<StoredFileWorkingCopy<M>>>;

    /// An event for when a stored file working copy changed it's readonly state.
    fn on_did_change_readonly(&self) -> Event<Rc<StoredFileWorkingCopy<M>>>;

    /// An event for when a stored file working copy changed it's orphaned state.
    fn on_did_change_orphaned(&self) -> Event<Rc<StoredFileWorkingCopy<M>>>;

    /// An event for when a stored file working copy failed to save.
    fn on_did_save_error(&self) -> Event<Rc<StoredFileWorkingCopy<M>>>;

    /// An event for when a stored file working copy successfully saved.
    fn on_did_save(&self) -> Event<StoredFileWorkingCopyManagerSaveEvent<M>>;

    /// An event for when a stored file working copy was reverted.
    fn on_did_revert(&self) -> Event<Rc<StoredFileWorkingCopy<M>>>;

    /// An event for when a stored file working copy is removed from the manager.
    fn on_did_remove(&self) -> Event<URI>;

    /// Allows to resolve a stored file working copy. If the manager already knows about a stored file working copy with
    /// the same `URI`, it will return that existing stored file working copy. There will never be more than one stored
    /// file working copy per `URI` until the stored file working copy is disposed.
    ///
    /// Use the `IStoredFileWorkingCopyResolveOptions.reload` option to control the behaviour for when a stored file
    /// working copy was previously already resolved with regards to resolving it again from the underlying file resource
    /// or not.
    ///
    /// Note: Callers must `dispose` the working copy when no longer needed.
    ///
    /// `resource` is used as unique identifier of the stored file working copy in case one is already known for this `URI`.
    fn resolve(&self, resource: &URI, options: Option<StoredFileWorkingCopyManagerResolveOptions>) -> Promise<Rc<StoredFileWorkingCopy<M>>>;

    /// Waits for the stored file working copy to be ready to be disposed. There may be conditions under which the stored
    /// file working copy cannot be disposed, e.g. when it is dirty. Once the promise is settled, it is safe to dispose
    /// (`None` is the immediate `true`).
    fn can_dispose(&self, working_copy: Rc<StoredFileWorkingCopy<M>>) -> Option<Promise<()>>;
}

/// `{ source: URI; target: URI; snapshot?: VSBufferReadableStream }`: uma working copy a restaurar depois de mover ou copiar.
struct WorkingCopyToRestore {
    source: URI,
    target: URI,
    snapshot: Option<VSBufferReadableStream>,
}

/// `URI | IStoredFileWorkingCopy<M>`, o argumento de `doResolve`.
enum ResourceOrWorkingCopy<M: IStoredFileWorkingCopyModel + 'static> {
    Resource(URI),
    WorkingCopy(Rc<StoredFileWorkingCopy<M>>),
}

/// `StoredFileWorkingCopyManager<M>`.
pub struct StoredFileWorkingCopyManager<M: IStoredFileWorkingCopyModel + 'static> {
    this: Weak<StoredFileWorkingCopyManager<M>>,
    /// O `BaseFileWorkingCopyManager` de que a classe herda.
    base: Rc<BaseFileWorkingCopyManager<StoredFileWorkingCopy<M>>>,
    /// O `_register(...)` da classe.
    store: Rc<Disposable>,

    //#region Events
    on_did_resolve: Rc<Emitter<Rc<StoredFileWorkingCopy<M>>>>,
    on_did_change_dirty: Rc<Emitter<Rc<StoredFileWorkingCopy<M>>>>,
    on_did_change_readonly: Rc<Emitter<Rc<StoredFileWorkingCopy<M>>>>,
    on_did_change_orphaned: Rc<Emitter<Rc<StoredFileWorkingCopy<M>>>>,
    on_did_save_error: Rc<Emitter<Rc<StoredFileWorkingCopy<M>>>>,
    on_did_save: Rc<Emitter<StoredFileWorkingCopyManagerSaveEvent<M>>>,
    on_did_revert: Rc<Emitter<Rc<StoredFileWorkingCopy<M>>>>,
    on_did_remove: Rc<Emitter<URI>>,
    //#endregion
    map_resource_to_working_copy_listeners: RefCell<ResourceMap<DisposableRef>>,
    map_resource_to_pending_working_copy_resolve: RefCell<ResourceMap<Promise<()>>>,

    working_copy_resolve_queue: Rc<ResourceQueue>,

    map_correlation_id_to_working_copies_to_restore: RefCell<HashMap<usize, Vec<WorkingCopyToRestore>>>,

    working_copy_type_id: String,
    model_factory: Rc<dyn IStoredFileWorkingCopyModelFactory<M>>,
    services: StoredFileWorkingCopyServices,
    label_service: Rc<dyn ILabelService>,
    uri_identity_service: Rc<dyn IUriIdentityService>,
}

impl<M: IStoredFileWorkingCopyModel + 'static> StoredFileWorkingCopyManager<M> {
    /// `constructor(workingCopyTypeId, modelFactory, ...services)`.
    pub fn new(
        working_copy_type_id: String,
        model_factory: Rc<dyn IStoredFileWorkingCopyModelFactory<M>>,
        services: StoredFileWorkingCopyServices,
        lifecycle_service: Rc<dyn ILifecycleService>,
        label_service: Rc<dyn ILabelService>,
        uri_identity_service: Rc<dyn IUriIdentityService>,
    ) -> Rc<Self> {
        let base = BaseFileWorkingCopyManager::new(
            Rc::new(|working_copy: &StoredFileWorkingCopy<M>| working_copy.on_will_dispose()),
            services.file_service.clone(),
            services.log_service.clone(),
            services.working_copy_backup_service.clone(),
        );
        let store = Disposable::new();

        let on_did_resolve = Emitter::new();
        let on_did_change_dirty = Emitter::new();
        let on_did_change_readonly = Emitter::new();
        let on_did_change_orphaned = Emitter::new();
        let on_did_save_error = Emitter::new();
        let on_did_save = Emitter::new();
        let on_did_revert = Emitter::new();
        let on_did_remove = Emitter::new();
        store.register(on_did_resolve.clone());
        store.register(on_did_change_dirty.clone());
        store.register(on_did_change_readonly.clone());
        store.register(on_did_change_orphaned.clone());
        store.register(on_did_save_error.clone());
        store.register(on_did_save.clone());
        store.register(on_did_revert.clone());
        store.register(on_did_remove.clone());

        let working_copy_resolve_queue = Rc::new(ResourceQueue::new(ext_uri()));
        store.register(working_copy_resolve_queue.clone());

        let this = Rc::new_cyclic(|this| Self {
            this: this.clone(),
            base,
            store,
            on_did_resolve,
            on_did_change_dirty,
            on_did_change_readonly,
            on_did_change_orphaned,
            on_did_save_error,
            on_did_save,
            on_did_revert,
            on_did_remove,
            map_resource_to_working_copy_listeners: RefCell::new(ResourceMap::new()),
            map_resource_to_pending_working_copy_resolve: RefCell::new(ResourceMap::new()),
            working_copy_resolve_queue,
            map_correlation_id_to_working_copies_to_restore: RefCell::new(HashMap::new()),
            working_copy_type_id,
            model_factory,
            services,
            label_service,
            uri_identity_service,
        });

        let weak = this.this.clone();
        this.base.set_remove_override(move |resource| weak.upgrade().is_some_and(|this| this.remove(resource)));

        this.register_listeners(lifecycle_service.as_ref());

        this
    }

    fn rc(&self) -> Rc<Self> {
        self.this.upgrade().expect("o gerenciador está vivo enquanto é usado")
    }

    fn register_listeners(&self, lifecycle_service: &dyn ILifecycleService) {
        let store = &self.store;

        // Update working copies from file change events
        let this = self.this.clone();
        store.register(self.services.file_service.on_did_files_change().listen(move |e| {
            if let Some(this) = this.upgrade() {
                this.on_did_files_change(e);
            }
        }));

        // File system provider changes
        let this = self.this.clone();
        store.register(self.services.file_service.on_did_change_file_system_provider_capabilities().listen(move |e: &FileSystemProviderCapabilitiesChangeEvent| {
            if let Some(this) = this.upgrade() {
                // Resolve working copies again for file systems that changed
                // capabilities to fetch latest metadata (e.g. readonly)
                // into all working copies.
                this.queue_working_copy_reloads_of_scheme(&e.scheme);
            }
        }));
        let this = self.this.clone();
        store.register(self.services.file_service.on_did_change_file_system_provider_registrations().listen(move |e: &FileSystemProviderRegistrationEvent| {
            if !e.added {
                return; // only if added
            }

            if let Some(this) = this.upgrade() {
                // Resolve working copies again for file systems that registered
                // to account for capability changes: extensions may unregister
                // and register the same provider with different capabilities,
                // so we want to ensure to fetch latest metadata (e.g. readonly)
                // into all working copies.
                this.queue_working_copy_reloads_of_scheme(&e.scheme);
            }
        }));

        // Working copy operations
        let this = self.this.clone();
        store.register(self.services.working_copy_file_service.on_will_run_working_copy_file_operation().listen(move |e| {
            if let Some(this) = this.upgrade() {
                this.on_will_run_working_copy_file_operation(e);
            }
        }));
        let this = self.this.clone();
        store.register(self.services.working_copy_file_service.on_did_fail_working_copy_file_operation().listen(move |e| {
            if let Some(this) = this.upgrade() {
                this.on_did_fail_working_copy_file_operation(e);
            }
        }));
        let this = self.this.clone();
        store.register(self.services.working_copy_file_service.on_did_run_working_copy_file_operation().listen(move |e| {
            if let Some(this) = this.upgrade() {
                this.on_did_run_working_copy_file_operation(e);
            }
        }));

        // Lifecycle
        if Platform::detect().is_web {
            let this = self.this.clone();
            store.register(lifecycle_service.on_before_shutdown().listen(move |event| {
                if let Some(this) = this.upgrade() {
                    event.veto(MaybePromise::Value(this.on_before_shutdown_web()), "veto.fileWorkingCopyManager");
                }
            }));
        } else {
            let this = self.this.clone();
            store.register(lifecycle_service.on_will_shutdown().listen(move |event| {
                if let Some(this) = this.upgrade() {
                    event.join(
                        this.on_will_shutdown_desktop(),
                        WillShutdownEventJoiner {
                            id: "join.fileWorkingCopyManager".to_string(),
                            label: localize("join.fileWorkingCopyManager", "Saving working copies", &[]),
                            order: None,
                        },
                    );
                }
            }));
        }
    }

    fn on_before_shutdown_web(&self) -> bool {
        // stored file working copies are pending to be saved:
        // veto because web does not support long running shutdown
        self.base.working_copies().iter().any(|working_copy| working_copy.has_state(StoredFileWorkingCopyState::PendingSave))
    }

    fn on_will_shutdown_desktop(&self) -> Promise<()> {
        let this = self.rc();

        async_fn(async move {
            // As long as stored file working copies are pending to be saved, we prolong the shutdown
            // until that has happened to ensure we are not shutting down in the middle of
            // writing to the working copy (https://github.com/microsoft/vscode/issues/116600).
            loop {
                let pending_saved_working_copies: Vec<Rc<StoredFileWorkingCopy<M>>> =
                    this.base.working_copies().into_iter().filter(|working_copy| working_copy.has_state(StoredFileWorkingCopyState::PendingSave)).collect();
                if pending_saved_working_copies.is_empty() {
                    break;
                }

                promises::settled(pending_saved_working_copies.iter().map(|working_copy| working_copy.join_state(StoredFileWorkingCopyState::PendingSave)).collect()).await?;
            }
            Ok(())
        })
    }

    //#region Resolve from file or file provider changes

    fn on_did_files_change(&self, e: &Rc<FileChangesEvent>) {
        // Trigger a resolve for any update or add event that impacts
        // the working copy. We also consider the added event
        // because it could be that a file was added and updated
        // right after.
        self.queue_working_copy_reloads(|working_copy| e.contains(&working_copy.resource(), &[FileChangeType::Updated, FileChangeType::Added]));
    }

    fn queue_working_copy_reloads_of_scheme(&self, scheme: &str) {
        self.queue_working_copy_reloads(|working_copy| scheme == working_copy.resource().scheme().to_string_lossy());
    }

    /// `queueWorkingCopyReloads(schemeOrEvent)`: `resolve_working_copy` é a decisão que o original toma pelo tipo do argumento.
    fn queue_working_copy_reloads(&self, resolve_working_copy: impl Fn(&StoredFileWorkingCopy<M>) -> bool) {
        for working_copy in self.base.working_copies() {
            if working_copy.is_dirty() {
                continue; // never reload dirty working copies
            }

            if resolve_working_copy(&working_copy) {
                self.queue_working_copy_reload(&working_copy);
            }
        }
    }

    fn queue_working_copy_reload(&self, working_copy: &Rc<StoredFileWorkingCopy<M>>) {
        // Resolves a working copy to update (use a queue to prevent accumulation of
        // resolve when the resolving actually takes long. At most we only want the
        // queue to have a size of 2 (1 running resolve and 1 queued resolve).
        let resource = working_copy.resource();
        let queue_size = self.working_copy_resolve_queue.queue_size(&resource);
        if queue_size <= 1 {
            let (this, working_copy) = (self.rc(), working_copy.clone());
            drop(self.working_copy_resolve_queue.queue_for(&resource, move || {
                async_fn(async move {
                    if let Err(error) = this.reload(working_copy).await {
                        this.services.log_service.error(&error.to_string());
                    }
                    Ok(())
                })
            }));
        }
    }

    //#endregion

    //#region Working Copy File Events

    fn on_will_run_working_copy_file_operation(&self, e: &WorkingCopyFileEvent) {
        // Move / Copy: remember working copies to restore after the operation
        if e.operation == FileOperation::Move || e.operation == FileOperation::Copy {
            let (this, files, correlation_id) = (self.rc(), e.files.clone(), e.correlation_id);
            e.wait_until(async_fn(async move {
                let mut working_copies_to_restore: Vec<WorkingCopyToRestore> = Vec::new();
                let ext_uri = this.uri_identity_service.ext_uri();

                for file in files {
                    let Some(source) = file.source else {
                        continue;
                    };
                    let target = file.target;

                    if ext_uri.is_equal(Some(&source), Some(&target), false) {
                        continue; // ignore if resources are considered equal
                    }

                    // Find all working copies that related to source (can be many if resource is a folder)
                    let source_working_copies: Vec<Rc<StoredFileWorkingCopy<M>>> =
                        this.base.working_copies().into_iter().filter(|working_copy| ext_uri.is_equal_or_parent(&working_copy.resource(), &source, false, false)).collect();

                    // Remember each source working copy to load again after move is done
                    // with optional content to restore if it was dirty
                    for source_working_copy in source_working_copies {
                        let source_resource = source_working_copy.resource();

                        // If the source is the actual working copy, just use target as new resource
                        let target_resource = if ext_uri.is_equal(Some(&source_resource), Some(&source), false) {
                            target.clone()
                        }
                        // Otherwise a parent folder of the source is being moved, so we need
                        // to compute the target resource based on that
                        else {
                            let path = source_resource.path();
                            let relative = path.slice((source.path().len() + 1).min(path.len()), path.len()).to_string_lossy();
                            ext_uri.join_path(&target, &[relative.as_str()])
                        };

                        let snapshot = match source_working_copy.model().filter(|_| source_working_copy.is_dirty()) {
                            Some(model) => Some(model.snapshot(SnapshotContext::Save, CancellationToken::None).await?),
                            None => None,
                        };

                        working_copies_to_restore.push(WorkingCopyToRestore { source: source_resource, target: target_resource, snapshot });
                    }
                }

                this.map_correlation_id_to_working_copies_to_restore.borrow_mut().insert(correlation_id, working_copies_to_restore);
                Ok(())
            }));
        }
    }

    fn on_did_fail_working_copy_file_operation(&self, e: &WorkingCopyFileEvent) {
        // Move / Copy: restore dirty flag on working copies to restore that were dirty
        if e.operation == FileOperation::Move || e.operation == FileOperation::Copy {
            let working_copies_to_restore = self.map_correlation_id_to_working_copies_to_restore.borrow_mut().remove(&e.correlation_id);
            if let Some(working_copies_to_restore) = working_copies_to_restore {
                for working_copy in working_copies_to_restore {
                    // Snapshot presence means this working copy used to be modified and so we restore that
                    // flag. we do NOT have to restore the content because the working copy was only soft
                    // reverted and did not loose its original modified contents.
                    if working_copy.snapshot.is_some()
                        && let Some(restored) = self.base.get(&working_copy.source)
                    {
                        restored.mark_modified();
                    }
                }
            }
        }
    }

    fn on_did_run_working_copy_file_operation(&self, e: &WorkingCopyFileEvent) {
        match e.operation {
            // Create: Revert existing working copies
            FileOperation::Create => {
                let (this, files) = (self.rc(), e.files.clone());
                e.wait_until(async_fn(async move {
                    for file in files {
                        let working_copy = this.base.get(&file.target);
                        if let Some(working_copy) = working_copy.filter(|working_copy| !working_copy.is_disposed()) {
                            working_copy.revert(None).await?;
                        }
                    }
                    Ok(())
                }));
            }

            // Move/Copy: restore working copies that were loaded before the operation took place
            FileOperation::Move | FileOperation::Copy => {
                let (this, correlation_id) = (self.rc(), e.correlation_id);
                e.wait_until(async_fn(async move {
                    let working_copies_to_restore = this.map_correlation_id_to_working_copies_to_restore.borrow_mut().remove(&correlation_id);
                    if let Some(working_copies_to_restore) = working_copies_to_restore {
                        promises::settled(working_copies_to_restore.into_iter().map(|working_copy_to_restore| this.restore_working_copy(working_copy_to_restore)).collect()).await?;
                    }
                    Ok(())
                }));
            }

            _ => {}
        }
    }

    /// O corpo do `Promises.settled(workingCopiesToRestore.map(async workingCopyToRestore => ...))`.
    fn restore_working_copy(&self, working_copy_to_restore: WorkingCopyToRestore) -> Promise<()> {
        let this = self.rc();

        async_fn(async move {
            // From this moment on, only operate on the canonical resource
            // to fix a potential data loss issue:
            // https://github.com/microsoft/vscode/issues/211374
            let target = this.uri_identity_service.as_canonical_uri(&working_copy_to_restore.target);

            // Restore the working copy at the target. if we have previous dirty content, we pass it
            // over to be used, otherwise we force a reload from disk. this is important
            // because we know the file has changed on disk after the move and the working copy might
            // have still existed with the previous state. this ensures that the working copy is not
            // tracking a stale state.
            this.resolve(
                &target,
                Some(StoredFileWorkingCopyManagerResolveOptions {
                    reload: Some(StoredFileWorkingCopyReloadOptions { async_: false, force: None }), // enforce a reload
                    options: StoredFileWorkingCopyResolveOptions { contents: working_copy_to_restore.snapshot, ..StoredFileWorkingCopyResolveOptions::default() },
                }),
            )
            .await?;
            Ok(())
        })
    }

    //#endregion

    //#region Reload & Resolve

    fn reload(&self, working_copy: Rc<StoredFileWorkingCopy<M>>) -> Promise<()> {
        let this = self.rc();

        async_fn(async move {
            // Await a pending working copy resolve first before proceeding
            // to ensure that we never resolve a working copy more than once
            // in parallel.
            if let Some(pending_resolve) = this.join_pending_resolves(&working_copy.resource()) {
                pending_resolve.await?;
            }

            if working_copy.is_dirty() || working_copy.is_disposed() || !this.base.has(&working_copy.resource()) {
                return Ok(()); // the working copy possibly got dirty or disposed, so return early then
            }

            // Trigger reload
            this.do_resolve(
                ResourceOrWorkingCopy::WorkingCopy(working_copy),
                Some(StoredFileWorkingCopyManagerResolveOptions {
                    reload: Some(StoredFileWorkingCopyReloadOptions { async_: false, force: None }),
                    ..StoredFileWorkingCopyManagerResolveOptions::default()
                }),
            )
            .await?;
            Ok(())
        })
    }

    fn do_resolve(&self, resource_or_working_copy: ResourceOrWorkingCopy<M>, options: Option<StoredFileWorkingCopyManagerResolveOptions>) -> Promise<Rc<StoredFileWorkingCopy<M>>> {
        let (resource, working_copy) = match resource_or_working_copy {
            ResourceOrWorkingCopy::Resource(resource) => {
                let working_copy = self.base.get(&resource);
                (resource, working_copy)
            }
            ResourceOrWorkingCopy::WorkingCopy(working_copy) => (working_copy.resource(), Some(working_copy)),
        };

        let options = options.unwrap_or_default();
        let working_copy_resolve: Promise<()>;
        let mut did_create_working_copy = false;

        let resolve_options = StoredFileWorkingCopyResolveOptions {
            contents: options.options.contents.clone(),
            force_read_from_file: options.reload.and_then(|reload| reload.force),
            limits: options.options.limits.clone(),
        };

        let working_copy = match working_copy {
            // Working copy exists
            Some(working_copy) => {
                // Always reload if contents are provided
                if options.options.contents.is_some() {
                    working_copy_resolve = working_copy.resolve_stored(Some(resolve_options));
                }
                // Reload async or sync based on options
                else if let Some(reload) = options.reload {
                    // Async reload: trigger a reload but return immediately
                    if reload.async_ {
                        working_copy_resolve = Promise::resolved(());
                        let reloaded = working_copy.clone();
                        drop(async_fn(async move {
                            if let Err(error) = reloaded.resolve_stored(Some(resolve_options)).await
                                && !reloaded.is_disposed()
                            {
                                on_unexpected_error(&error); // only log if the working copy is still around
                            }
                            Ok(())
                        }));
                    }
                    // Sync reload: do not return until working copy reloaded
                    else {
                        working_copy_resolve = working_copy.resolve_stored(Some(resolve_options));
                    }
                }
                // Do not reload
                else {
                    working_copy_resolve = Promise::resolved(());
                }
                working_copy
            }

            // Stored file working copy does not exist
            None => {
                did_create_working_copy = true;

                let weak_manager = self.this.clone();
                let resolve_resource = resource.clone();
                let external_resolver: StoredFileWorkingCopyResolver = Rc::new(move |options: Option<StoredFileWorkingCopyResolveOptions>| match weak_manager.upgrade() {
                    Some(manager) => manager
                        .resolve(
                            &resolve_resource,
                            Some(StoredFileWorkingCopyManagerResolveOptions {
                                options: options.unwrap_or_default(),
                                reload: Some(StoredFileWorkingCopyReloadOptions { async_: false, force: None }),
                            }),
                        )
                        .then(|_| ()),
                    None => Promise::resolved(()),
                });

                let new_working_copy = StoredFileWorkingCopy::new(
                    self.working_copy_type_id.clone(),
                    resource.clone(),
                    self.label_service.get_uri_basename_label(&resource),
                    self.model_factory.clone(),
                    external_resolver,
                    self.services.clone(),
                );

                working_copy_resolve = new_working_copy.resolve_stored(Some(resolve_options));

                self.register_working_copy(&new_working_copy);
                new_working_copy
            }
        };

        // Store pending resolve to avoid race conditions
        self.map_resource_to_pending_working_copy_resolve.borrow_mut().set(resource.clone(), working_copy_resolve.clone());

        // Make known to manager (if not already known)
        self.base.add(&resource, &working_copy);

        // Emit some events if we created the working copy
        if did_create_working_copy {
            // If the working copy is dirty right from the beginning,
            // make sure to emit this as an event
            if working_copy.is_dirty() {
                self.on_did_change_dirty.fire(working_copy.clone());
            }
        }

        let this = self.rc();
        async_fn(async move {
            let result = working_copy_resolve.await;

            // Remove from pending resolves
            this.map_resource_to_pending_working_copy_resolve.borrow_mut().delete(&resource);

            if let Err(error) = result {
                // Automatically dispose the working copy if we created
                // it because we cannot dispose a working copy we do not
                // own (https://github.com/microsoft/vscode/issues/138850)
                if did_create_working_copy {
                    working_copy.dispose();
                }

                return Err(error);
            }

            // Stored file working copy can be dirty if a backup was restored, so we make sure to
            // have this event delivered if we created the working copy here
            if did_create_working_copy && working_copy.is_dirty() {
                this.on_did_change_dirty.fire(working_copy.clone());
            }

            Ok(working_copy)
        })
    }

    fn join_pending_resolves(&self, resource: &URI) -> Option<Promise<()>> {
        let pending_working_copy_resolve = self.map_resource_to_pending_working_copy_resolve.borrow().get(resource).cloned();
        pending_working_copy_resolve?;

        Some(self.do_join_pending_resolves(resource.clone()))
    }

    fn do_join_pending_resolves(&self, resource: URI) -> Promise<()> {
        let this = self.rc();

        async_fn(async move {
            // While we have pending working copy resolves, ensure
            // to await the last one finishing before returning.
            // This prevents a race when multiple clients await
            // the pending resolve and then all trigger the resolve
            // at the same time.
            let mut current_working_copy_resolve: Option<Promise<()>> = None;
            loop {
                let next_pending_working_copy_resolve = this.map_resource_to_pending_working_copy_resolve.borrow().get(&resource).cloned();
                let Some(next_pending_working_copy_resolve) = next_pending_working_copy_resolve else {
                    break;
                };

                if current_working_copy_resolve.as_ref().is_some_and(|current| current.ptr_eq(&next_pending_working_copy_resolve)) {
                    return Ok(()); // already awaited on - return
                }

                current_working_copy_resolve = Some(next_pending_working_copy_resolve.clone());
                // ignore any error here, it will bubble to the original requestor
                let _ = next_pending_working_copy_resolve.await;
            }
            Ok(())
        })
    }

    fn register_working_copy(&self, working_copy: &Rc<StoredFileWorkingCopy<M>>) {
        // Install working copy listeners
        let working_copy_listeners = DisposableStore::new();
        let weak_working_copy = Rc::downgrade(working_copy);

        working_copy_listeners.add(Self::forward(&weak_working_copy, working_copy.on_did_resolve(), self.on_did_resolve.clone()));
        working_copy_listeners.add(Self::forward(&weak_working_copy, working_copy.on_did_change_dirty(), self.on_did_change_dirty.clone()));
        working_copy_listeners.add(Self::forward(&weak_working_copy, working_copy.on_did_change_readonly(), self.on_did_change_readonly.clone()));
        working_copy_listeners.add(Self::forward(&weak_working_copy, working_copy.on_did_change_orphaned(), self.on_did_change_orphaned.clone()));
        working_copy_listeners.add(Self::forward(&weak_working_copy, working_copy.on_did_save_error(), self.on_did_save_error.clone()));
        let (on_did_save, working_copy_for_save) = (self.on_did_save.clone(), weak_working_copy.clone());
        working_copy_listeners.add(working_copy.on_did_save_stored().listen(move |e| {
            if let Some(working_copy) = working_copy_for_save.upgrade() {
                on_did_save.fire(StoredFileWorkingCopyManagerSaveEvent { working_copy, event: e.clone() });
            }
        }));
        working_copy_listeners.add(Self::forward(&weak_working_copy, working_copy.on_did_revert(), self.on_did_revert.clone()));

        // Keep for disposal
        self.map_resource_to_working_copy_listeners.borrow_mut().set(working_copy.resource(), working_copy_listeners);
    }

    /// `workingCopyListeners.add(workingCopy.onXxx(() => this._onXxx.fire(workingCopy)))`.
    fn forward(working_copy: &Weak<StoredFileWorkingCopy<M>>, event: Event<()>, emitter: Rc<Emitter<Rc<StoredFileWorkingCopy<M>>>>) -> DisposableRef {
        let working_copy = working_copy.clone();
        event.listen(move |_| {
            if let Some(working_copy) = working_copy.upgrade() {
                emitter.fire(working_copy);
            }
        })
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

        if removed {
            self.on_did_remove.fire(resource.clone());
        }

        removed
    }

    //#endregion

    //#region Lifecycle

    fn do_can_dispose(&self, working_copy: Rc<StoredFileWorkingCopy<M>>) -> Promise<()> {
        let this = self.rc();

        async_fn(async move {
            // Await any pending resolves first before proceeding
            if let Some(pending_resolve) = this.join_pending_resolves(&working_copy.resource()) {
                pending_resolve.await?;

                return match this.can_dispose(working_copy) {
                    Some(promise) => promise.await,
                    None => Ok(()),
                };
            }

            // Dirty working copy: we do not allow to dispose dirty working copys
            // to prevent data loss cases. dirty working copys can only be disposed when
            // they are either saved or reverted
            if working_copy.is_dirty() {
                working_copy.on_did_change_dirty().to_promise(None).await?;

                return match this.can_dispose(working_copy) {
                    Some(promise) => promise.await,
                    None => Ok(()),
                };
            }

            Ok(())
        })
    }

    //#endregion
}

impl<M: IStoredFileWorkingCopyModel + 'static> IDisposable for StoredFileWorkingCopyManager<M> {
    fn dispose(&self) {
        self.base.dispose_base();
        self.store.dispose();

        // Clear pending working copy resolves
        self.map_resource_to_pending_working_copy_resolve.borrow_mut().clear();

        // Dispose the working copy change listeners
        let working_copy_listeners: Vec<DisposableRef> = self.map_resource_to_working_copy_listeners.borrow().values().cloned().collect();
        for working_copy_listener in working_copy_listeners {
            working_copy_listener.dispose();
        }
        self.map_resource_to_working_copy_listeners.borrow_mut().clear();
    }
}

impl<M: IStoredFileWorkingCopyModel + 'static> IBaseFileWorkingCopyManager<StoredFileWorkingCopy<M>> for StoredFileWorkingCopyManager<M> {
    fn base_manager(&self) -> &BaseFileWorkingCopyManager<StoredFileWorkingCopy<M>> {
        &self.base
    }

    fn destroy(&self) -> Promise<()> {
        let this = self.rc();
        self.base.destroy(move || this.dispose())
    }
}

impl<M: IStoredFileWorkingCopyModel + 'static> IStoredFileWorkingCopyManager<M> for StoredFileWorkingCopyManager<M> {
    fn on_did_resolve(&self) -> Event<Rc<StoredFileWorkingCopy<M>>> {
        self.on_did_resolve.event()
    }

    fn on_did_change_dirty(&self) -> Event<Rc<StoredFileWorkingCopy<M>>> {
        self.on_did_change_dirty.event()
    }

    fn on_did_change_readonly(&self) -> Event<Rc<StoredFileWorkingCopy<M>>> {
        self.on_did_change_readonly.event()
    }

    fn on_did_change_orphaned(&self) -> Event<Rc<StoredFileWorkingCopy<M>>> {
        self.on_did_change_orphaned.event()
    }

    fn on_did_save_error(&self) -> Event<Rc<StoredFileWorkingCopy<M>>> {
        self.on_did_save_error.event()
    }

    fn on_did_save(&self) -> Event<StoredFileWorkingCopyManagerSaveEvent<M>> {
        self.on_did_save.event()
    }

    fn on_did_revert(&self) -> Event<Rc<StoredFileWorkingCopy<M>>> {
        self.on_did_revert.event()
    }

    fn on_did_remove(&self) -> Event<URI> {
        self.on_did_remove.event()
    }

    fn resolve(&self, resource: &URI, options: Option<StoredFileWorkingCopyManagerResolveOptions>) -> Promise<Rc<StoredFileWorkingCopy<M>>> {
        let (this, resource) = (self.rc(), resource.clone());

        async_fn(async move {
            // Await a pending working copy resolve first before proceeding
            // to ensure that we never resolve a working copy more than once
            // in parallel.
            if let Some(pending_resolve) = this.join_pending_resolves(&resource) {
                pending_resolve.await?;
            }

            // Trigger resolve
            this.do_resolve(ResourceOrWorkingCopy::Resource(resource), options).await
        })
    }

    fn can_dispose(&self, working_copy: Rc<StoredFileWorkingCopy<M>>) -> Option<Promise<()>> {
        // Quick return if working copy already disposed or not dirty and not resolving
        if working_copy.is_disposed() || (!self.map_resource_to_pending_working_copy_resolve.borrow().has(&working_copy.resource()) && !working_copy.is_dirty()) {
            return None;
        }

        // Promise based return in all other cases
        Some(self.do_can_dispose(working_copy))
    }
}
