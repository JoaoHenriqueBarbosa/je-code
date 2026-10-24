//! Porte de `src/vs/workbench/services/textfile/common/textFileEditorModelManager.ts`: o `TextFileEditorModelManager`,
//! que guarda os modelos de arquivo de texto por recurso, resolve (sem nunca resolver o mesmo modelo duas vezes em
//! paralelo), recarrega quando o arquivo muda no disco, acompanha operações de mover/copiar/criar arquivos, roda os
//! participantes do salvar e diz quando um modelo pode ser descartado.
//!
//! Divergências:
//! - **Injeção.** O construtor recebe os serviços (`IFileService`, `INotificationService`, `IWorkingCopyFileService`,
//!   `IUriIdentityService`), o [`TextFileSaveParticipant`] que o original cria pelo `IInstantiationService` e a fábrica
//!   `model_factory`, que é o `instantiationService.createInstance(TextFileEditorModel, resource, encoding, languageId)`.
//! - **Herança.** `extends Disposable` é o `Disposable` base em `base`; os eventos são `Rc<Emitter<..>>`.
//! - **Eventos com o modelo.** O original tipa os eventos como `TextFileEditorModel`; o contrato
//!   ([`ITextFileEditorModelManager`]) os tipa como `ITextFileEditorModel`, que é o que o trait devolve aqui.
//! - **`saveErrorHandler`** é o par `save_error_handler`/`set_save_error_handler` do contrato; o padrão é o
//!   [`DefaultSaveErrorHandler`] (a notificação `genericSaveError`).
//! - **`ITextFileEditorModelToRestore.snapshot`** guarda a [`TextBufferFactoryRef`] já criada do snapshot (o snapshot do
//!   porte não é clonável e `createTextBufferFactoryFromSnapshot` é o único consumidor dele).
//! - **`canDispose(model): true | Promise<true>`** é `Option<Promise<()>>`: `None` é o `true` imediato.
//! - **`Promises.settled`** é [`promises::settled`]; `Event.toPromise` é [`Event::to_promise`].
//! - **`waitUntil`** do evento de operação de working copy é [`WorkingCopyFileEvent::wait_until`].
//! - **Vazamento por ciclo.** Os ouvintes que o gerenciador instala nos modelos seguram o modelo por `Weak`, porque o
//!   `Rc` do modelo guarda os emissores onde os ouvintes vivem (no JavaScript o coletor de lixo cuida do ciclo).

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::{Rc, Weak};

use je_base::async_::{ResourceQueue, async_fn, promises};
use je_base::cancellation::CancellationToken;
use je_base::error_message::to_error_message_of;
use je_base::errors::{Error, on_unexpected_error};
use je_base::event::{Emitter, Event};
use je_base::lifecycle::{Disposable, DisposableRef, DisposableStore, IDisposable};
use je_base::map::ResourceMap;
use je_base::nls::localize;
use je_base::promise::Promise;
use je_base::uri::URI;
use je_editor::modes_registry::{PLAINTEXT_EXTENSION, PLAINTEXT_LANGUAGE_ID};
use je_platform::files::files::{FileChangeType, FileChangesEvent, FileOperation, FileSystemProviderCapabilitiesChangeEvent, FileSystemProviderRegistrationEvent};
use je_platform::files::i_file_service::IFileService;
use je_platform::notification::notification::{INotificationService, NotificationMessage};
use je_platform::resources::ext_uri;
use je_platform::uri_identity::uri_identity::IUriIdentityService;

use crate::common::editor::editor_model::IEditorModel;
use crate::services::textfile::collaborators::{IProgress, IWorkingCopyFileService, ProgressStep, WorkingCopyFileEvent};
use crate::services::textfile::text_buffer_factory::text_buffer_factory_from_snapshot;
use crate::services::textfile::text_file_editor_model::TextFileEditorModel;
use crate::services::textfile::text_file_save_participant::TextFileSaveParticipant;
use crate::services::textfile::textfiles::{
    ILanguageSupport, ISaveErrorHandler, ITextFileEditorModel, ITextFileEditorModelManager, ITextFileSaveParticipant, TextBufferFactoryRef, TextFileEditorModelResolveOrCreateOptions,
    TextFileReloadOptions, TextFileResolveEvent, TextFileResolveOptions, TextFileSaveAsOptions, TextFileSaveEvent, TextFileSaveParticipantContext,
};
use crate::services::working_copy::working_copy::IWorkingCopy;

/// `ITextFileEditorModelToRestore`.
struct TextFileEditorModelToRestore {
    source: URI,
    target: URI,
    snapshot: Option<TextBufferFactoryRef>,
    /// `language: { id, explicit }`.
    language: Option<(String, bool)>,
    encoding: Option<String>,
}

/// O `{ onSaveError(error, model) { notificationService.error(...) } }` do `saveErrorHandler` padrão.
pub struct DefaultSaveErrorHandler {
    notification_service: Rc<dyn INotificationService>,
}

impl ISaveErrorHandler for DefaultSaveErrorHandler {
    fn on_save_error(&self, error: &Error, model: &dyn ITextFileEditorModel, _options: &TextFileSaveAsOptions) {
        let message = localize("genericSaveError", "Failed to save '{0}': {1}", &[model.name().as_str(), to_error_message_of(error, false).as_str()]);
        self.notification_service.error(vec![NotificationMessage::Text(message)]);
    }
}

/// `URI | TextFileEditorModel`, o argumento de `doResolve`.
enum ResourceOrModel {
    Resource(URI),
    Model(Rc<TextFileEditorModel>),
}

/// `TextFileEditorModelManager`.
pub struct TextFileEditorModelManager {
    this: Weak<TextFileEditorModelManager>,
    base: Rc<Disposable>,

    on_did_create: Rc<Emitter<Rc<dyn ITextFileEditorModel>>>,
    on_did_resolve: Rc<Emitter<TextFileResolveEvent>>,
    on_did_remove: Rc<Emitter<URI>>,
    on_did_change_dirty: Rc<Emitter<Rc<dyn ITextFileEditorModel>>>,
    on_did_change_readonly: Rc<Emitter<Rc<dyn ITextFileEditorModel>>>,
    on_did_change_orphaned: Rc<Emitter<Rc<dyn ITextFileEditorModel>>>,
    on_did_save_error: Rc<Emitter<Rc<dyn ITextFileEditorModel>>>,
    on_did_save: Rc<Emitter<TextFileSaveEvent>>,
    on_did_revert: Rc<Emitter<Rc<dyn ITextFileEditorModel>>>,
    on_did_change_encoding: Rc<Emitter<Rc<dyn ITextFileEditorModel>>>,

    map_resource_to_model: RefCell<ResourceMap<Rc<TextFileEditorModel>>>,
    map_resource_to_model_listeners: RefCell<ResourceMap<DisposableRef>>,
    map_resource_to_dispose_listener: RefCell<ResourceMap<DisposableRef>>,
    map_resource_to_pending_model_resolvers: RefCell<ResourceMap<Promise<()>>>,

    model_resolve_queue: Rc<ResourceQueue>,

    save_error_handler: RefCell<Rc<dyn ISaveErrorHandler>>,

    map_correlation_id_to_models_to_restore: RefCell<HashMap<usize, Vec<TextFileEditorModelToRestore>>>,

    save_participants: Rc<TextFileSaveParticipant>,

    file_service: Rc<dyn IFileService>,
    uri_identity_service: Rc<dyn IUriIdentityService>,
    model_factory: Rc<dyn Fn(URI, Option<String>, Option<String>) -> Rc<TextFileEditorModel>>,
}

impl TextFileEditorModelManager {
    /// `constructor(instantiationService, fileService, notificationService, workingCopyFileService, uriIdentityService)`.
    pub fn new(
        file_service: Rc<dyn IFileService>,
        notification_service: Rc<dyn INotificationService>,
        working_copy_file_service: Rc<dyn IWorkingCopyFileService>,
        uri_identity_service: Rc<dyn IUriIdentityService>,
        save_participants: Rc<TextFileSaveParticipant>,
        model_factory: Rc<dyn Fn(URI, Option<String>, Option<String>) -> Rc<TextFileEditorModel>>,
    ) -> Rc<Self> {
        let base = Disposable::new();

        // increased for users with hundreds of inputs opened
        let on_did_create = Emitter::with_options(je_base::event::EmitterOptions {
            leak_warning_threshold: Some(500),
            leak_warning_name: Some("TextFileEditorModelManager._onDidCreate".to_string()),
            ..Default::default()
        });
        let on_did_resolve = Emitter::new();
        let on_did_remove = Emitter::new();
        let on_did_change_dirty = Emitter::new();
        let on_did_change_readonly = Emitter::new();
        let on_did_change_orphaned = Emitter::new();
        let on_did_save_error = Emitter::new();
        let on_did_save = Emitter::new();
        let on_did_revert = Emitter::new();
        let on_did_change_encoding = Emitter::new();
        base.register(on_did_create.clone());
        base.register(on_did_resolve.clone());
        base.register(on_did_remove.clone());
        base.register(on_did_change_dirty.clone());
        base.register(on_did_change_readonly.clone());
        base.register(on_did_change_orphaned.clone());
        base.register(on_did_save_error.clone());
        base.register(on_did_save.clone());
        base.register(on_did_revert.clone());
        base.register(on_did_change_encoding.clone());

        let model_resolve_queue = Rc::new(ResourceQueue::new(ext_uri()));
        base.register(model_resolve_queue.clone());

        base.register(save_participants.clone());

        let this = Rc::new_cyclic(|this| Self {
            this: this.clone(),
            base,
            on_did_create,
            on_did_resolve,
            on_did_remove,
            on_did_change_dirty,
            on_did_change_readonly,
            on_did_change_orphaned,
            on_did_save_error,
            on_did_save,
            on_did_revert,
            on_did_change_encoding,
            map_resource_to_model: RefCell::new(ResourceMap::new()),
            map_resource_to_model_listeners: RefCell::new(ResourceMap::new()),
            map_resource_to_dispose_listener: RefCell::new(ResourceMap::new()),
            map_resource_to_pending_model_resolvers: RefCell::new(ResourceMap::new()),
            model_resolve_queue,
            save_error_handler: RefCell::new(Rc::new(DefaultSaveErrorHandler { notification_service })),
            map_correlation_id_to_models_to_restore: RefCell::new(HashMap::new()),
            save_participants,
            file_service,
            uri_identity_service,
            model_factory,
        });

        this.register_listeners(working_copy_file_service);

        this
    }

    fn rc(&self) -> Rc<Self> {
        self.this.upgrade().expect("o gerenciador está vivo enquanto é usado")
    }

    fn register_listeners(&self, working_copy_file_service: Rc<dyn IWorkingCopyFileService>) {
        let store = &self.base;

        // Update models from file change events
        let this = self.this.clone();
        store.register(self.file_service.on_did_files_change().listen(move |e| {
            if let Some(this) = this.upgrade() {
                this.on_did_files_change(e);
            }
        }));

        // File system provider changes
        let this = self.this.clone();
        store.register(self.file_service.on_did_change_file_system_provider_capabilities().listen(move |e: &FileSystemProviderCapabilitiesChangeEvent| {
            if let Some(this) = this.upgrade() {
                // Resolve models again for file systems that changed
                // capabilities to fetch latest metadata (e.g. readonly)
                // into all models.
                this.queue_model_reloads(&e.scheme);
            }
        }));
        let this = self.this.clone();
        store.register(self.file_service.on_did_change_file_system_provider_registrations().listen(move |e: &FileSystemProviderRegistrationEvent| {
            if !e.added {
                return; // only if added
            }

            if let Some(this) = this.upgrade() {
                // Resolve models again for file systems that registered
                // to account for capability changes: extensions may
                // unregister and register the same provider with different
                // capabilities, so we want to ensure to fetch latest
                // metadata (e.g. readonly) into all models.
                this.queue_model_reloads(&e.scheme);
            }
        }));

        // Working copy operations
        let this = self.this.clone();
        store.register(working_copy_file_service.on_will_run_working_copy_file_operation().listen(move |e| {
            if let Some(this) = this.upgrade() {
                this.on_will_run_working_copy_file_operation(e);
            }
        }));
        let this = self.this.clone();
        store.register(working_copy_file_service.on_did_fail_working_copy_file_operation().listen(move |e| {
            if let Some(this) = this.upgrade() {
                this.on_did_fail_working_copy_file_operation(e);
            }
        }));
        let this = self.this.clone();
        store.register(working_copy_file_service.on_did_run_working_copy_file_operation().listen(move |e| {
            if let Some(this) = this.upgrade() {
                this.on_did_run_working_copy_file_operation(e);
            }
        }));
    }

    fn on_did_files_change(&self, e: &Rc<FileChangesEvent>) {
        for model in self.models_concrete() {
            if model.is_dirty() {
                continue; // never reload dirty models
            }

            // Trigger a model resolve for any update or add event that impacts
            // the model. We also consider the added event because it could
            // be that a file was added and updated right after.
            if e.contains(&model.resource(), &[FileChangeType::Updated, FileChangeType::Added]) {
                self.queue_model_reload(&model);
            }
        }
    }

    fn queue_model_reloads(&self, scheme: &str) {
        for model in self.models_concrete() {
            if model.is_dirty() {
                continue; // never reload dirty models
            }

            if scheme == model.resource().scheme().to_string_lossy() {
                self.queue_model_reload(&model);
            }
        }
    }

    fn queue_model_reload(&self, model: &Rc<TextFileEditorModel>) {
        // Resolve model to update (use a queue to prevent accumulation of resolves
        // when the resolve actually takes long. At most we only want the queue
        // to have a size of 2 (1 running resolve and 1 queued resolve).
        let queue_size = self.model_resolve_queue.queue_size(&model.resource());
        if queue_size <= 1 {
            let (this, model_to_reload) = (self.rc(), model.clone());
            drop(self.model_resolve_queue.queue_for(&model.resource(), move || {
                async_fn(async move {
                    if let Err(error) = this.reload(model_to_reload).await {
                        on_unexpected_error(&error);
                    }
                    Ok(())
                })
            }));
        }
    }

    fn on_will_run_working_copy_file_operation(&self, e: &WorkingCopyFileEvent) {
        // Move / Copy: remember models to restore after the operation
        if e.operation == FileOperation::Move || e.operation == FileOperation::Copy {
            let mut models_to_restore: Vec<TextFileEditorModelToRestore> = Vec::new();
            let ext_uri = self.uri_identity_service.ext_uri();

            for file in &e.files {
                let (Some(source), target) = (&file.source, &file.target) else {
                    continue;
                };

                if ext_uri.is_equal(Some(source), Some(target), false) {
                    continue; // ignore if resources are considered equal
                }

                // find all models that related to source (can be many if resource is a folder)
                let source_models: Vec<Rc<TextFileEditorModel>> =
                    self.models_concrete().into_iter().filter(|model| ext_uri.is_equal_or_parent(&model.resource(), source, false, false)).collect();

                // remember each source model to resolve again after move is done
                // with optional content to restore if it was dirty
                for source_model in source_models {
                    let source_model_resource = source_model.resource();

                    // If the source is the actual model, just use target as new resource
                    let target_model_resource = if ext_uri.is_equal(Some(&source_model_resource), Some(source), false) {
                        target.clone()
                    }
                    // Otherwise a parent folder of the source is being moved, so we need
                    // to compute the target resource based on that
                    else {
                        let path = source_model_resource.path();
                        let relative = path.slice((source.path().len() + 1).min(path.len()), path.len()).to_string_lossy();
                        ext_uri.join_path(target, &[relative.as_str()])
                    };

                    let language_id = source_model.get_language_id();
                    models_to_restore.push(TextFileEditorModelToRestore {
                        source: source_model_resource,
                        target: target_model_resource,
                        language: language_id.map(|id| (id, source_model.language_change_source() == Some(crate::common::editor::text_editor_model::LanguageChangeSource::User))),
                        encoding: source_model.get_encoding(),
                        snapshot: if source_model.is_dirty() { source_model.create_snapshot().map(|mut snapshot| text_buffer_factory_from_snapshot(snapshot.as_mut())) } else { None },
                    });
                }
            }

            self.map_correlation_id_to_models_to_restore.borrow_mut().insert(e.correlation_id, models_to_restore);
        }
    }

    fn on_did_fail_working_copy_file_operation(&self, e: &WorkingCopyFileEvent) {
        // Move / Copy: restore dirty flag on models to restore that were dirty
        if e.operation == FileOperation::Move || e.operation == FileOperation::Copy {
            let models_to_restore = self.map_correlation_id_to_models_to_restore.borrow_mut().remove(&e.correlation_id);
            if let Some(models_to_restore) = models_to_restore {
                for model in models_to_restore {
                    // snapshot presence means this model used to be dirty and so we restore that
                    // flag. we do NOT have to restore the content because the model was only soft
                    // reverted and did not loose its original dirty contents.
                    if model.snapshot.is_some() {
                        if let Some(restored) = self.get_concrete(&model.source) {
                            restored.set_dirty(true);
                        }
                    }
                }
            }
        }
    }

    fn on_did_run_working_copy_file_operation(&self, e: &WorkingCopyFileEvent) {
        match e.operation {
            // Create: Revert existing models
            FileOperation::Create => {
                let (this, files) = (self.rc(), e.files.clone());
                e.wait_until(async_fn(async move {
                    for file in files {
                        let model = this.get_concrete(&file.target);
                        if let Some(model) = model.filter(|model| !model.is_disposed()) {
                            model.revert(None).await?;
                        }
                    }
                    Ok(())
                }));
            }

            // Move/Copy: restore models that were resolved before the operation took place
            FileOperation::Move | FileOperation::Copy => {
                let (this, correlation_id) = (self.rc(), e.correlation_id);
                e.wait_until(async_fn(async move {
                    let models_to_restore = this.map_correlation_id_to_models_to_restore.borrow_mut().remove(&correlation_id);
                    if let Some(models_to_restore) = models_to_restore {
                        promises::settled(models_to_restore.into_iter().map(|model_to_restore| this.restore_model(model_to_restore)).collect()).await?;
                    }
                    Ok(())
                }));
            }

            _ => {}
        }
    }

    /// O corpo do `Promises.settled(modelsToRestore.map(async modelToRestore => ...))`.
    fn restore_model(&self, model_to_restore: TextFileEditorModelToRestore) -> Promise<()> {
        let this = self.rc();
        async_fn(async move {
            // From this moment on, only operate on the canonical resource
            // to fix a potential data loss issue:
            // https://github.com/microsoft/vscode/issues/211374
            let target = this.uri_identity_service.as_canonical_uri(&model_to_restore.target);

            // restore the model at the target. if we have previous dirty content, we pass it
            // over to be used, otherwise we force a reload from disk. this is important
            // because we know the file has changed on disk after the move and the model might
            // have still existed with the previous state. this ensures that the model is not
            // tracking a stale state.
            let restored_model = this
                .resolve_model(
                    &target,
                    Some(TextFileEditorModelResolveOrCreateOptions {
                        reload: Some(TextFileReloadOptions { async_: false }), // enforce a reload
                        resolve: TextFileResolveOptions { contents: model_to_restore.snapshot.clone(), ..Default::default() },
                        encoding: model_to_restore.encoding.clone(),
                        ..Default::default()
                    }),
                )
                .await?;

            // restore model language only if it is specific
            if let Some((language_id, explicit)) = model_to_restore.language.as_ref().filter(|(id, _)| !id.is_empty() && id != PLAINTEXT_LANGUAGE_ID) {
                // an explicitly set language is restored via `setLanguageId`
                // to preserve it as explicitly set by the user.
                // (https://github.com/microsoft/vscode/issues/203648)
                if *explicit {
                    restored_model.set_language_id(language_id, None);
                }
                // otherwise, a model language is applied via lower level
                // APIs to not confuse it with an explicitly set language.
                // (https://github.com/microsoft/vscode/issues/125795)
                else if restored_model.get_language_id().as_deref() == Some(PLAINTEXT_LANGUAGE_ID) && ext_uri().extname(&target) != PLAINTEXT_EXTENSION {
                    restored_model.update_text_editor_model(None, Some(language_id), None);
                }
            }
            Ok(())
        })
    }

    /// `models`, como os modelos concretos.
    fn models_concrete(&self) -> Vec<Rc<TextFileEditorModel>> {
        self.map_resource_to_model.borrow().values().cloned().collect()
    }

    fn get_concrete(&self, resource: &URI) -> Option<Rc<TextFileEditorModel>> {
        self.map_resource_to_model.borrow().get(resource).cloned()
    }

    fn has(&self, resource: &URI) -> bool {
        self.map_resource_to_model.borrow().has(resource)
    }

    fn reload(&self, model: Rc<TextFileEditorModel>) -> Promise<()> {
        let this = self.rc();
        async_fn(async move {
            // Await a pending model resolve first before proceeding
            // to ensure that we never resolve a model more than once
            // in parallel.
            this.do_join_pending_resolves(model.resource()).await?;

            if model.is_dirty() || model.is_disposed() || !this.has(&model.resource()) {
                return Ok(()); // the model possibly got dirty or disposed, so return early then
            }

            // Trigger reload
            this.do_resolve(ResourceOrModel::Model(model), Some(TextFileEditorModelResolveOrCreateOptions { reload: Some(TextFileReloadOptions { async_: false }), ..Default::default() })).await?;
            Ok(())
        })
    }

    /// `resolve(resource, options?)` com o modelo concreto.
    pub fn resolve_model(&self, resource: &URI, options: Option<TextFileEditorModelResolveOrCreateOptions>) -> Promise<Rc<TextFileEditorModel>> {
        let (this, resource) = (self.rc(), resource.clone());
        async_fn(async move {
            // Await a pending model resolve first before proceeding
            // to ensure that we never resolve a model more than once
            // in parallel.
            let pending_resolve = this.join_pending_resolves(&resource);
            if let Some(pending_resolve) = pending_resolve {
                pending_resolve.await?;
            }

            // Trigger resolve
            this.do_resolve(ResourceOrModel::Resource(resource), options).await
        })
    }

    fn do_resolve(&self, resource_or_model: ResourceOrModel, options: Option<TextFileEditorModelResolveOrCreateOptions>) -> Promise<Rc<TextFileEditorModel>> {
        let (resource, model) = match resource_or_model {
            ResourceOrModel::Resource(resource) => {
                let model = self.get_concrete(&resource);
                (resource, model)
            }
            ResourceOrModel::Model(model) => (model.resource(), Some(model)),
        };

        let options = options.unwrap_or_default();
        let model_resolve: Promise<()>;
        let mut did_create_model = false;

        let model = match model {
            // Model exists
            Some(model) => {
                // Always reload if contents are provided
                if options.resolve.contents.is_some() {
                    model_resolve = model.resolve_text_file(Some(options.resolve.clone()));
                }
                // Reload async or sync based on options
                else if let Some(reload) = options.reload {
                    // async reload: trigger a reload but return immediately
                    if reload.async_ {
                        model_resolve = Promise::resolved(());
                        let (reload_model, resolve_options) = (model.clone(), options.resolve.clone());
                        drop(async_fn(async move {
                            if let Err(error) = reload_model.resolve_text_file(Some(resolve_options)).await {
                                if !reload_model.is_disposed() {
                                    on_unexpected_error(&error); // only log if the model is still around
                                }
                            }
                            Ok(())
                        }));
                    }
                    // sync reload: do not return until model reloaded
                    else {
                        model_resolve = model.resolve_text_file(Some(options.resolve.clone()));
                    }
                }
                // Do not reload
                else {
                    model_resolve = Promise::resolved(());
                }
                model
            }

            // Model does not exist
            None => {
                did_create_model = true;

                let new_model = (self.model_factory)(resource.clone(), options.encoding.clone(), options.language_id.clone());
                model_resolve = new_model.resolve_text_file(Some(options.resolve.clone()));

                self.register_model(&new_model);
                new_model
            }
        };

        // Store pending resolves to avoid race conditions
        self.map_resource_to_pending_model_resolvers.borrow_mut().set(resource.clone(), model_resolve.clone());

        // Make known to manager (if not already known)
        self.add(&resource, &model);

        // Emit some events if we created the model
        if did_create_model {
            let created: Rc<dyn ITextFileEditorModel> = model.clone();
            self.on_did_create.fire(created.clone());

            // If the model is dirty right from the beginning,
            // make sure to emit this as an event
            if model.is_dirty() {
                self.on_did_change_dirty.fire(created);
            }
        }

        let this = self.rc();
        async_fn(async move {
            match model_resolve.await {
                Ok(()) => {
                    // Remove from pending resolves
                    this.map_resource_to_pending_model_resolvers.borrow_mut().delete(&resource);
                }
                Err(error) => {
                    // Automatically dispose the model if we created it
                    // because we cannot dispose a model we do not own
                    // https://github.com/microsoft/vscode/issues/138850
                    if did_create_model {
                        model.dispose();
                    }

                    // Remove from pending resolves
                    this.map_resource_to_pending_model_resolvers.borrow_mut().delete(&resource);

                    return Err(error);
                }
            }

            // Apply language if provided
            if let Some(language_id) = options.language_id.as_deref().filter(|id| !id.is_empty()) {
                model.set_language_id(language_id, None);
            }

            // Model can be dirty if a backup was restored, so we make sure to
            // have this event delivered if we created the model here
            if did_create_model && model.is_dirty() {
                let created: Rc<dyn ITextFileEditorModel> = model.clone();
                this.on_did_change_dirty.fire(created);
            }

            Ok(model)
        })
    }

    fn join_pending_resolves(&self, resource: &URI) -> Option<Promise<()>> {
        let pending_model_resolve = self.map_resource_to_pending_model_resolvers.borrow().get(resource).cloned();
        pending_model_resolve?;

        Some(self.do_join_pending_resolves(resource.clone()))
    }

    fn do_join_pending_resolves(&self, resource: URI) -> Promise<()> {
        let this = self.rc();
        async_fn(async move {
            // While we have pending model resolves, ensure
            // to await the last one finishing before returning.
            // This prevents a race when multiple clients await
            // the pending resolve and then all trigger the resolve
            // at the same time.
            let mut current_model_copy_resolve: Option<Promise<()>> = None;
            loop {
                let next_pending_model_resolve = this.map_resource_to_pending_model_resolvers.borrow().get(&resource).cloned();
                let Some(next_pending_model_resolve) = next_pending_model_resolve else {
                    break;
                };

                if current_model_copy_resolve.as_ref().is_some_and(|current| current.ptr_eq(&next_pending_model_resolve)) {
                    return Ok(()); // already awaited on - return
                }

                current_model_copy_resolve = Some(next_pending_model_resolve.clone());
                // ignore any error here, it will bubble to the original requestor
                let _ = next_pending_model_resolve.await;
            }
            Ok(())
        })
    }

    fn register_model(&self, model: &Rc<TextFileEditorModel>) {
        // Install model listeners
        let model_listeners = DisposableStore::new();

        let weak_model = Rc::downgrade(model);
        let (this, model_for_resolve) = (self.this.clone(), weak_model.clone());
        model_listeners.add(model.on_did_resolve().listen(move |reason| {
            if let (Some(this), Some(model)) = (this.upgrade(), model_for_resolve.upgrade()) {
                this.on_did_resolve.fire(TextFileResolveEvent { model, reason: *reason });
            }
        }));
        model_listeners.add(Self::forward(&weak_model, model.on_did_change_dirty(), self.on_did_change_dirty.clone()));
        model_listeners.add(Self::forward(&weak_model, model.on_did_change_readonly(), self.on_did_change_readonly.clone()));
        model_listeners.add(Self::forward(&weak_model, model.on_did_change_orphaned(), self.on_did_change_orphaned.clone()));
        model_listeners.add(Self::forward(&weak_model, model.on_did_save_error(), self.on_did_save_error.clone()));
        let (on_did_save, model_for_save) = (self.on_did_save.clone(), weak_model.clone());
        model_listeners.add(model.on_did_save_stored().listen(move |e| {
            if let Some(model) = model_for_save.upgrade() {
                on_did_save.fire(TextFileSaveEvent { model, event: e.clone() });
            }
        }));
        model_listeners.add(Self::forward(&weak_model, model.on_did_revert(), self.on_did_revert.clone()));
        model_listeners.add(Self::forward(&weak_model, model.on_did_change_encoding(), self.on_did_change_encoding.clone()));

        // Keep for disposal
        self.map_resource_to_model_listeners.borrow_mut().set(model.resource(), model_listeners);
    }

    /// `modelListeners.add(model.onXxx(() => this._onXxx.fire(model)))`.
    fn forward(model: &Weak<TextFileEditorModel>, event: Event<()>, emitter: Rc<Emitter<Rc<dyn ITextFileEditorModel>>>) -> DisposableRef {
        let model = model.clone();
        event.listen(move |_| {
            if let Some(model) = model.upgrade() {
                let model: Rc<dyn ITextFileEditorModel> = model;
                emitter.fire(model);
            }
        })
    }

    /// `add(resource, model)`.
    pub fn add(&self, resource: &URI, model: &Rc<TextFileEditorModel>) {
        let known_model = self.map_resource_to_model.borrow().get(resource).cloned();
        if known_model.is_some_and(|known| Rc::ptr_eq(&known, model)) {
            return; // already cached
        }

        // dispose any previously stored dispose listener for this resource
        let dispose_listener = self.map_resource_to_dispose_listener.borrow().get(resource).cloned();
        if let Some(dispose_listener) = dispose_listener {
            dispose_listener.dispose();
        }

        // store in cache but remove when model gets disposed
        self.map_resource_to_model.borrow_mut().set(resource.clone(), model.clone());
        let (this, resource_to_remove) = (self.this.clone(), resource.clone());
        let listener = model.on_will_dispose().listen(move |_| {
            if let Some(this) = this.upgrade() {
                this.remove(&resource_to_remove);
            }
        });
        self.map_resource_to_dispose_listener.borrow_mut().set(resource.clone(), listener);
    }

    /// `remove(resource)`.
    pub fn remove(&self, resource: &URI) {
        let removed = self.map_resource_to_model.borrow_mut().delete(resource);

        let dispose_listener = self.map_resource_to_dispose_listener.borrow().get(resource).cloned();
        if let Some(dispose_listener) = dispose_listener {
            dispose_listener.dispose();
            self.map_resource_to_dispose_listener.borrow_mut().delete(resource);
        }

        let model_listener = self.map_resource_to_model_listeners.borrow().get(resource).cloned();
        if let Some(model_listener) = model_listener {
            model_listener.dispose();
            self.map_resource_to_model_listeners.borrow_mut().delete(resource);
        }

        if removed {
            self.on_did_remove.fire(resource.clone());
        }
    }

    fn do_can_dispose(&self, model: Rc<dyn ITextFileEditorModel>) -> Promise<()> {
        let this = self.rc();
        async_fn(async move {
            // Await any pending resolves first before proceeding
            let pending_resolve = this.join_pending_resolves(&model.resource());
            if let Some(pending_resolve) = pending_resolve {
                pending_resolve.await?;

                return match this.can_dispose(model) {
                    Some(promise) => promise.await,
                    None => Ok(()),
                };
            }

            // dirty model: we do not allow to dispose dirty models to prevent
            // data loss cases. dirty models can only be disposed when they are
            // either saved or reverted
            if model.is_dirty() {
                model.on_did_change_dirty().to_promise(None).await?;

                return match this.can_dispose(model) {
                    Some(promise) => promise.await,
                    None => Ok(()),
                };
            }

            Ok(())
        })
    }
}

impl IDisposable for TextFileEditorModelManager {
    fn dispose(&self) {
        self.base.dispose();

        // model caches
        self.map_resource_to_model.borrow_mut().clear();
        self.map_resource_to_pending_model_resolvers.borrow_mut().clear();

        // dispose the dispose listeners
        let dispose_listeners: Vec<DisposableRef> = self.map_resource_to_dispose_listener.borrow().values().cloned().collect();
        for listener in dispose_listeners {
            listener.dispose();
        }
        self.map_resource_to_dispose_listener.borrow_mut().clear();

        // dispose the model change listeners
        let model_listeners: Vec<DisposableRef> = self.map_resource_to_model_listeners.borrow().values().cloned().collect();
        for listener in model_listeners {
            listener.dispose();
        }
        self.map_resource_to_model_listeners.borrow_mut().clear();
    }
}

impl ITextFileEditorModelManager for TextFileEditorModelManager {
    fn on_did_create(&self) -> Event<Rc<dyn ITextFileEditorModel>> {
        self.on_did_create.event()
    }

    fn on_did_resolve(&self) -> Event<TextFileResolveEvent> {
        self.on_did_resolve.event()
    }

    fn on_did_change_dirty(&self) -> Event<Rc<dyn ITextFileEditorModel>> {
        self.on_did_change_dirty.event()
    }

    fn on_did_change_readonly(&self) -> Event<Rc<dyn ITextFileEditorModel>> {
        self.on_did_change_readonly.event()
    }

    fn on_did_remove(&self) -> Event<URI> {
        self.on_did_remove.event()
    }

    fn on_did_change_orphaned(&self) -> Event<Rc<dyn ITextFileEditorModel>> {
        self.on_did_change_orphaned.event()
    }

    fn on_did_change_encoding(&self) -> Event<Rc<dyn ITextFileEditorModel>> {
        self.on_did_change_encoding.event()
    }

    fn on_did_save_error(&self) -> Event<Rc<dyn ITextFileEditorModel>> {
        self.on_did_save_error.event()
    }

    fn on_did_save(&self) -> Event<TextFileSaveEvent> {
        self.on_did_save.event()
    }

    fn on_did_revert(&self) -> Event<Rc<dyn ITextFileEditorModel>> {
        self.on_did_revert.event()
    }

    fn models(&self) -> Vec<Rc<dyn ITextFileEditorModel>> {
        self.models_concrete().into_iter().map(|model| model as Rc<dyn ITextFileEditorModel>).collect()
    }

    fn set_save_error_handler(&self, handler: Rc<dyn ISaveErrorHandler>) {
        *self.save_error_handler.borrow_mut() = handler;
    }

    fn save_error_handler(&self) -> Rc<dyn ISaveErrorHandler> {
        self.save_error_handler.borrow().clone()
    }

    fn get(&self, resource: &URI) -> Option<Rc<dyn ITextFileEditorModel>> {
        self.get_concrete(resource).map(|model| model as Rc<dyn ITextFileEditorModel>)
    }

    fn resolve(&self, resource: &URI, options: Option<TextFileEditorModelResolveOrCreateOptions>) -> Promise<Rc<dyn ITextFileEditorModel>> {
        self.resolve_model(resource, options).then(|model| model as Rc<dyn ITextFileEditorModel>)
    }

    fn add_save_participant(&self, participant: Rc<dyn ITextFileSaveParticipant>) -> DisposableRef {
        self.save_participants.add_save_participant(participant)
    }

    fn run_save_participants(
        &self,
        model: Rc<dyn ITextFileEditorModel>,
        context: TextFileSaveParticipantContext,
        progress: Rc<dyn IProgress<ProgressStep>>,
        token: CancellationToken,
    ) -> Promise<()> {
        self.save_participants.participate(model, context, progress, token)
    }

    fn can_dispose(&self, model: Rc<dyn ITextFileEditorModel>) -> Option<Promise<()>> {
        // quick return if model already disposed or not dirty and not resolving
        if model.is_disposed() || (!self.map_resource_to_pending_model_resolvers.borrow().has(&model.resource()) && !model.is_dirty()) {
            return None;
        }

        // promise based return in all other cases
        Some(self.do_can_dispose(model))
    }
}
