//! Porte de `src/vs/workbench/services/workingCopy/common/untitledFileWorkingCopy.ts`: a `UntitledFileWorkingCopy`, a
//! working copy sem título (sem arquivo no disco) e os contratos que a cercam: o modelo (`IUntitledFileWorkingCopyModel`),
//! a fábrica dele, o evento de mudança de conteúdo, o conteúdo inicial e o delegado que salva. Resolve (de um backup, do
//! conteúdo inicial ou vazia), rastreia o estado de modificado e de sujo (o scratchpad nunca fica sujo), faz backup,
//! salva pelo delegado e, ao reverter, descarta a si mesma.
//!
//! Divergências:
//! - **Genéricos.** A classe é genérica no modelo (`M`), como no original. O contrato `IUntitledFileWorkingCopy<M>` é o
//!   trait [`IUntitledFileWorkingCopy`], não genérico (ver `file_working_copy.rs`), e `IResolvedUntitledFileWorkingCopy`
//!   e o `isResolved(): this is ...` são guardas de tipo do TypeScript que não existem aqui: o modelo tipado sai de
//!   [`UntitledFileWorkingCopy::model`] (um `Option`).
//! - **`onDidChangeContent` do modelo.** `IUntitledFileWorkingCopyModel` sobrescreve o evento com um tipo mais estreito;
//!   como o `IStoredFileWorkingCopyModel`, o contrato acrescenta o evento tipado como
//!   [`IUntitledFileWorkingCopyModel::on_did_change_untitled_content`].
//! - **Fábrica.** `IUntitledFileWorkingCopyModelFactory<M>` não acrescenta nada à fábrica de arquivo
//!   (`interface ... extends ... { }`): toda fábrica de arquivo é também uma fábrica de arquivo sem título.
//! - **Injeção.** Os três parâmetros decorados do construtor (`IWorkingCopyService`, `IWorkingCopyBackupService`,
//!   `ILogService`) são os três últimos argumentos de [`UntitledFileWorkingCopy::new`].
//! - **Herança.** `extends Disposable` é um [`Disposable`] próprio (campo `store`), que o `dispose()` descarta; a ordem de
//!   registro (os eventos, o registro no serviço, depois o modelo e as escutas dele) é a do original.
//! - **`onWillDispose`.** A working copy sem título não é de recurso (ver `file_working_copy.rs`), então
//!   [`IUntitledFileWorkingCopy::on_will_dispose`] o declara.
//! - **Estado.** Os campos mutáveis são `Cell`/`RefCell` (a working copy vive em `Rc`); o modelo guardado não é limpo no
//!   `dispose()`, como no original.
//! - **Escutas.** As escutas do modelo seguram a working copy por `Weak`, para o ciclo `Rc` não vazar (no JavaScript o
//!   coletor de lixo cuida disso).
//! - **`this.trace`** é [`UntitledFileWorkingCopy::trace`], com o recurso e o `typeId` no fim da mensagem.

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use je_base::stream::WriteableStream;
use je_base::async_::{async_fn, race_cancellation};
use je_base::buffer::{VSBufferReadableStream, new_writeable_buffer_stream};
use je_base::cancellation::CancellationToken;
use je_base::event::{Emitter, Event};
use je_base::lifecycle::{Disposable, IDisposable};
use je_base::promise::Promise;
use je_base::uri::URI;
use je_platform::log::LogService;

use crate::common::editor::{RevertOptions, SaveOptions};
use crate::services::working_copy::file_working_copy::{IFileWorkingCopy, IFileWorkingCopyModel, IFileWorkingCopyModelFactory, SnapshotContext};
use crate::services::working_copy::working_copy::{IWorkingCopy, WorkingCopyBackup, WorkingCopyBackupContent, WorkingCopyCapabilities, WorkingCopyIdentifier, WorkingCopySaveEvent};
use crate::services::working_copy::working_copy_backup::IWorkingCopyBackupService;
use crate::services::working_copy::working_copy_service::IWorkingCopyService;

/// `IUntitledFileWorkingCopyModelFactory<M>`: untitled file specific working copy model factory.
pub trait IUntitledFileWorkingCopyModelFactory<M: IUntitledFileWorkingCopyModel + 'static>: IFileWorkingCopyModelFactory<M> {}

/// A fábrica de arquivo sem título não acrescenta nada à fábrica de arquivo: toda fábrica concreta de arquivo é também uma
/// de arquivo sem título. (Sem `?Sized`, pelo mesmo motivo da fábrica de arquivo armazenado: o erro E0371.)
impl<M: IUntitledFileWorkingCopyModel + 'static, T: IFileWorkingCopyModelFactory<M>> IUntitledFileWorkingCopyModelFactory<M> for T {}

/// `IUntitledFileWorkingCopyModelContentChangedEvent`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct UntitledFileWorkingCopyModelContentChangedEvent {
    /// Flag that indicates that the content change should clear the dirty/modified flags, e.g. because the contents are
    /// back to being empty or back to an initial state that should not be considered as modified.
    pub is_initial: bool,
}

/// `IUntitledFileWorkingCopyModel`: the underlying model of a untitled file working copy provides some methods for the
/// untitled file working copy to function. The model is typically only available after the working copy has been resolved
/// via it's `resolve()` method.
pub trait IUntitledFileWorkingCopyModel: IFileWorkingCopyModel {
    /// O `onDidChangeContent` com o tipo do evento do untitled (o do [`IFileWorkingCopyModel`] é o sinal sem dados).
    fn on_did_change_untitled_content(&self) -> Event<UntitledFileWorkingCopyModelContentChangedEvent>;
}

/// `IUntitledFileWorkingCopy<M>`.
pub trait IUntitledFileWorkingCopy: IFileWorkingCopy {
    /// Whether this untitled file working copy model has an associated file path.
    fn has_associated_file_path(&self) -> bool;

    /// An event for when the untitled file working copy is about to be disposed.
    fn on_will_dispose(&self) -> Event<()>;
}

/// `IUntitledFileWorkingCopySaveDelegate<M>`: a delegate to enable saving of untitled file working copies.
pub type UntitledFileWorkingCopySaveDelegate<M> = Rc<dyn Fn(Rc<UntitledFileWorkingCopy<M>>, Option<SaveOptions>) -> Promise<bool>>;

/// `IUntitledFileWorkingCopyInitialContents`.
#[derive(Clone)]
pub struct UntitledFileWorkingCopyInitialContents {
    /// The initial contents of the untitled file working copy.
    pub value: VSBufferReadableStream,
    /// If not provided, the untitled file working copy will be marked modified by default given initial contents are
    /// provided.
    ///
    /// Note: if the untitled file working copy has an associated path the modified state will always be set.
    pub mark_modified: Option<bool>,
}

impl UntitledFileWorkingCopyInitialContents {
    /// `Boolean(this.initialContents && this.initialContents.markModified !== false)`.
    fn marks_modified(initial_contents: Option<&Self>) -> bool {
        initial_contents.is_some_and(|initial_contents| initial_contents.mark_modified != Some(false))
    }
}

/// `UntitledFileWorkingCopy<M>`.
pub struct UntitledFileWorkingCopy<M: IUntitledFileWorkingCopyModel + 'static> {
    this: Weak<UntitledFileWorkingCopy<M>>,
    /// O `_register(...)` da classe (o `Disposable` de que ela herda).
    store: Rc<Disposable>,
    type_id: String,
    resource: URI,
    name: String,
    has_associated_file_path: bool,
    is_scratchpad: bool,
    initial_contents: Option<UntitledFileWorkingCopyInitialContents>,
    model_factory: Rc<dyn IUntitledFileWorkingCopyModelFactory<M>>,
    save_delegate: UntitledFileWorkingCopySaveDelegate<M>,
    working_copy_backup_service: Rc<dyn IWorkingCopyBackupService>,
    log_service: Rc<dyn LogService>,

    capabilities: WorkingCopyCapabilities,
    model: RefCell<Option<Rc<M>>>,

    //#region Events
    on_did_change_content: Rc<Emitter<()>>,
    on_did_change_dirty: Rc<Emitter<()>>,
    on_did_save: Rc<Emitter<WorkingCopySaveEvent>>,
    on_did_revert: Rc<Emitter<()>>,
    on_will_dispose: Rc<Emitter<()>>,
    //#endregion
    modified: Cell<bool>,
}

impl<M: IUntitledFileWorkingCopyModel + 'static> UntitledFileWorkingCopy<M> {
    /// `constructor(typeId, resource, name, hasAssociatedFilePath, isScratchpad, initialContents, modelFactory,
    /// saveDelegate, ...services)`.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        type_id: String,
        resource: URI,
        name: String,
        has_associated_file_path: bool,
        is_scratchpad: bool,
        initial_contents: Option<UntitledFileWorkingCopyInitialContents>,
        model_factory: Rc<dyn IUntitledFileWorkingCopyModelFactory<M>>,
        save_delegate: UntitledFileWorkingCopySaveDelegate<M>,
        working_copy_service: Rc<dyn IWorkingCopyService>,
        working_copy_backup_service: Rc<dyn IWorkingCopyBackupService>,
        log_service: Rc<dyn LogService>,
    ) -> Rc<Self> {
        let store = Disposable::new();

        let on_did_change_content = Emitter::new();
        let on_did_change_dirty = Emitter::new();
        let on_did_save = Emitter::new();
        let on_did_revert = Emitter::new();
        let on_will_dispose = Emitter::new();
        store.register(on_did_change_content.clone());
        store.register(on_did_change_dirty.clone());
        store.register(on_did_save.clone());
        store.register(on_did_revert.clone());
        store.register(on_will_dispose.clone());

        let capabilities = if is_scratchpad {
            WorkingCopyCapabilities(WorkingCopyCapabilities::UNTITLED.0 | WorkingCopyCapabilities::SCRATCHPAD.0)
        } else {
            WorkingCopyCapabilities::UNTITLED
        };
        let modified = has_associated_file_path || UntitledFileWorkingCopyInitialContents::marks_modified(initial_contents.as_ref());

        let this = Rc::new_cyclic(|this: &Weak<UntitledFileWorkingCopy<M>>| Self {
            this: this.clone(),
            store,
            type_id,
            resource,
            name,
            has_associated_file_path,
            is_scratchpad,
            initial_contents,
            model_factory,
            save_delegate,
            working_copy_backup_service,
            log_service,
            capabilities,
            model: RefCell::new(None),
            on_did_change_content,
            on_did_change_dirty,
            on_did_save,
            on_did_revert,
            on_will_dispose,
            modified: Cell::new(modified),
        });

        // Make known to working copy service
        let working_copy: Rc<dyn IWorkingCopy> = this.clone();
        this.store.register(working_copy_service.register_working_copy(working_copy));

        this
    }

    fn rc(&self) -> Rc<Self> {
        self.this.upgrade().expect("a working copy está viva enquanto é usada")
    }

    /// `model`: o modelo tipado, `None` enquanto a working copy não foi resolvida.
    pub fn model(&self) -> Option<Rc<M>> {
        self.model.borrow().clone()
    }

    //#region Dirty/Modified

    fn set_modified(&self, modified: bool) {
        if self.modified.get() == modified {
            return;
        }

        self.modified.set(modified);
        if !self.is_scratchpad {
            self.on_did_change_dirty.fire(());
        }
    }

    //#endregion

    //#region Resolve

    fn do_create_model(&self, contents: VSBufferReadableStream) -> Promise<()> {
        self.trace("doCreateModel()");

        let creating = self.model_factory.create_model(&self.resource, contents, CancellationToken::None);

        // Create model and dispose it when we get disposed
        let this = self.rc();
        async_fn(async move {
            let model = creating.await?;
            *this.model.borrow_mut() = Some(model.clone());
            this.store.register(model.clone());

            // Model listeners
            this.install_model_listeners(&model);
            Ok(())
        })
    }

    fn install_model_listeners(&self, model: &Rc<M>) {
        // Content Change
        let this = self.this.clone();
        self.store.register(model.on_did_change_untitled_content().listen(move |e| {
            if let Some(this) = this.upgrade() {
                this.on_model_content_changed(e);
            }
        }));

        // Lifecycle
        let this = self.this.clone();
        self.store.register(model.on_will_dispose().listen(move |_| {
            if let Some(this) = this.upgrade() {
                this.dispose();
            }
        }));
    }

    fn on_model_content_changed(&self, e: &UntitledFileWorkingCopyModelContentChangedEvent) {
        // Mark the untitled file working copy as non-modified once its
        // in case provided by the change event and in case we do not
        // have an associated path set
        if !self.has_associated_file_path && e.is_initial {
            self.set_modified(false);
        }
        // Turn modified otherwise
        else {
            self.set_modified(true);
        }

        // Emit as general content change event
        self.on_did_change_content.fire(());
    }

    //#endregion

    fn trace(&self, msg: &str) {
        self.log_service.trace(&format!("[untitled file working copy] {msg} {} {}", self.resource.to_string(false), self.type_id));
    }
}

impl<M: IUntitledFileWorkingCopyModel + 'static> IDisposable for UntitledFileWorkingCopy<M> {
    fn dispose(&self) {
        self.trace("dispose()");

        self.on_will_dispose.fire(());

        // super.dispose()
        self.store.dispose();
    }
}

impl<M: IUntitledFileWorkingCopyModel + 'static> IWorkingCopy for UntitledFileWorkingCopy<M> {
    fn working_copy_type_id(&self) -> String {
        self.type_id.clone()
    }

    fn resource(&self) -> URI {
        self.resource.clone()
    }

    fn name(&self) -> String {
        self.name.clone()
    }

    fn capabilities(&self) -> WorkingCopyCapabilities {
        self.capabilities
    }

    fn on_did_change_dirty(&self) -> Event<()> {
        self.on_did_change_dirty.event()
    }

    fn on_did_change_content(&self) -> Event<()> {
        self.on_did_change_content.event()
    }

    fn on_did_save(&self) -> Event<WorkingCopySaveEvent> {
        self.on_did_save.event()
    }

    fn is_dirty(&self) -> bool {
        self.modified.get() && !self.is_scratchpad // Scratchpad working copies are never dirty
    }

    fn is_modified(&self) -> bool {
        self.modified.get()
    }

    //#region Backup

    fn backup_delay(&self) -> Option<u64> {
        self.model().and_then(|model| model.configuration()).and_then(|configuration| configuration.backup_delay)
    }

    fn backup(&self, token: &CancellationToken) -> Promise<WorkingCopyBackup> {
        // Make sure to check whether this working copy has been
        // resolved or not and fallback to the initial value -
        // if any - to prevent backing up an unresolved working
        // copy and loosing the initial value.
        let model = self.model();
        let initial_value = self.initial_contents.as_ref().map(|initial_contents| initial_contents.value.clone());
        let token = token.clone();

        async_fn(async move {
            let content = match model {
                Some(model) => race_cancellation(&model.snapshot(SnapshotContext::Backup, token.clone()), &token).await?,
                None => initial_value,
            };

            Ok(WorkingCopyBackup { meta: None, content: content.map(WorkingCopyBackupContent::Stream) })
        })
    }

    //#endregion

    //#region Save

    fn save(&self, options: Option<SaveOptions>) -> Promise<bool> {
        self.trace("save()");

        let (this, save_delegate) = (self.rc(), self.save_delegate.clone());
        async_fn(async move {
            let result = save_delegate(this.clone(), options.clone()).await?;

            // Emit Save Event
            if result {
                this.on_did_save.fire(WorkingCopySaveEvent {
                    reason: options.as_ref().and_then(|options| options.reason),
                    source: options.and_then(|options| options.source),
                });
            }

            Ok(result)
        })
    }

    //#endregion

    //#region Revert

    fn revert(&self, _options: Option<RevertOptions>) -> Promise<()> {
        self.trace("revert()");

        // No longer modified
        self.set_modified(false);

        // Emit as event
        self.on_did_revert.fire(());

        // A reverted untitled file working copy is invalid
        // because it has no actual source on disk to revert to.
        // As such we dispose the model.
        self.dispose();

        Promise::resolved(())
    }

    //#endregion
}

impl<M: IUntitledFileWorkingCopyModel + 'static> IFileWorkingCopy for UntitledFileWorkingCopy<M> {
    fn on_did_revert(&self) -> Event<()> {
        self.on_did_revert.event()
    }

    fn model(&self) -> Option<Rc<dyn IFileWorkingCopyModel>> {
        self.model.borrow().clone().map(|model| model as Rc<dyn IFileWorkingCopyModel>)
    }

    fn resolve(&self) -> Promise<()> {
        self.trace("resolve()");

        if self.is_resolved() {
            self.trace("resolve() - exit (already resolved)");

            // return early if the untitled file working copy is already
            // resolved assuming that the contents have meanwhile changed
            // in the underlying model. we only resolve untitled once.
            return Promise::resolved(());
        }

        let this = self.rc();
        async_fn(async move {
            // Check for backups or use initial value or empty
            let backup = this.working_copy_backup_service.resolve(&WorkingCopyIdentifier::of(&*this)).await?;
            let untitled_contents: VSBufferReadableStream = if let Some(backup) = &backup {
                this.trace("resolve() - with backup");

                backup.value.clone()
            } else if let Some(initial_contents) = &this.initial_contents {
                this.trace("resolve() - with initial contents");

                initial_contents.value.clone()
            } else {
                this.trace("resolve() - empty");

                let empty = new_writeable_buffer_stream(None);
                empty.end(None);
                let empty: VSBufferReadableStream = empty;
                empty
            };

            // Create model
            this.do_create_model(untitled_contents).await?;

            // Untitled associated to file path are modified right away as well as untitled with content
            this.set_modified(this.has_associated_file_path || backup.is_some() || UntitledFileWorkingCopyInitialContents::marks_modified(this.initial_contents.as_ref()));

            // If we have initial contents, make sure to emit this
            // as the appropriate events to the outside.
            if backup.is_some() || this.initial_contents.is_some() {
                this.on_did_change_content.fire(());
            }

            Ok(())
        })
    }

    fn is_resolved(&self) -> bool {
        self.model.borrow().is_some()
    }
}

impl<M: IUntitledFileWorkingCopyModel + 'static> IUntitledFileWorkingCopy for UntitledFileWorkingCopy<M> {
    fn has_associated_file_path(&self) -> bool {
        self.has_associated_file_path
    }

    fn on_will_dispose(&self) -> Event<()> {
        self.on_will_dispose.event()
    }
}
