//! Porte de `src/vs/workbench/services/workingCopy/common/storedFileWorkingCopy.ts`: a `StoredFileWorkingCopy`, a working
//! copy apoiada por um `URI` de um provider de arquivos conhecido. Resolve (do disco, de um backup ou de um conteúdo
//! dado), salva (com participantes, uma escrita por vez, com proteção contra escrita suja e com o tratamento do erro de
//! salvar), reverte, rastreia sujeira (dirty), arquivo órfão, erro e conflito por `etag`, e faz backup.
//!
//! Divergências:
//! - **Injeção.** Os 11 parâmetros decorados do construtor são o [`StoredFileWorkingCopyServices`]; `typeId`, `resource`,
//!   `name`, a fábrica do modelo e o resolvedor externo são os argumentos de [`StoredFileWorkingCopy::new`].
//! - **Herança.** `StoredFileWorkingCopy extends ResourceWorkingCopy` é a composição com [`ResourceWorkingCopy`] (campo
//!   `base`, exposto por [`IResourceWorkingCopy`]); o `_register(...)` é um [`Disposable`] próprio (campo `store`), que o
//!   `dispose()` descarta junto com a base. `implements IStoredFileWorkingCopy<M>` são os traits [`IWorkingCopy`],
//!   [`IResourceWorkingCopy`], [`IFileWorkingCopy`] e [`IStoredFileWorkingCopy`].
//! - **Genéricos.** A classe é genérica no modelo (`M`), como no original. Os contratos (`IStoredFileWorkingCopy`,
//!   `IStoredFileWorkingCopyModel`) são objetos de trait e não genéricos (ver `file_working_copy.rs`): quem recebe a
//!   working copy sem saber o modelo (os participantes de salvar) a recebe como `Rc<dyn IStoredFileWorkingCopy>`, e o
//!   modelo tipado sai de [`StoredFileWorkingCopy::model`]. `IResolvedStoredFileWorkingCopy` e os `this is ...` são
//!   guardas de tipo do TypeScript e não existem.
//! - **Nomes que colidem.** `IStoredFileWorkingCopy` sobrescreve `resolve(options?)`, `save(options?)` e `onDidSave` do
//!   `IFileWorkingCopy` e do `IWorkingCopy` com tipos mais largos; Rust não tem sobrescrita, então os do contrato stored
//!   são [`IStoredFileWorkingCopy::resolve_stored`], [`IStoredFileWorkingCopy::save_stored`] e
//!   [`IStoredFileWorkingCopy::on_did_save_stored`] (a mesma convenção do `ITextFileEditorModel`), e o `model` tipado do
//!   stored é [`IStoredFileWorkingCopy::stored_model`].
//! - **Opções de salvar.** `IStoredFileWorkingCopySaveOptions extends ISaveOptions` e `IStoredFileWorkingCopySaveAsOptions
//!   extends IStoredFileWorkingCopySaveOptions` são structs com o nível base em `base` e `options` (como as do arquivo de
//!   texto). `options.reason = SaveReason.EXPLICIT` no `doSave` muda uma cópia (as opções são `Clone`).
//! - **`isStoredFileWorkingCopySaveEvent`** não existe: o tipo [`StoredFileWorkingCopySaveEvent`] já diz que há o `stat`.
//! - **Estado.** Os campos mutáveis são `Cell`/`RefCell` (a working copy vive em `Rc`). `versionId` do modelo (`unknown`)
//!   é um [`Dynamic`], comparado por igualdade estrita. `lastResolvedFileStat` é lido por
//!   [`StoredFileWorkingCopy::last_resolved_file_stat`] (no original é público porque os testes o leem).
//! - **Estado de erro.** `error.fileOperationResult` é [`FileOperationError::from_error`]; `NotModifiedSinceFileOperationError`
//!   é o mesmo erro com `stat`.
//! - **`fileOperationError.options?.unlock`.** O [`FileOperationError`] do porte não guarda as opções da escrita que
//!   falhou. O serviço de arquivos as copia das opções recebidas, e a working copy as monta de `writeUnlock`, então
//!   `triedToUnlock` é `writeUnlock` das opções de salvar (o mesmo valor).
//! - **`isErrorWithActions(error)`.** O [`Error`] do porte não carrega ações (ver `error_message.rs`), então o ramo que
//!   copia as ações do erro para o aviso não tem como disparar com um erro que passou por uma promessa.
//! - **`isWindows`** é `cfg!(windows)`.
//! - **`Date.now()`** é [`date_now`].
//! - **`this.trace`** é [`StoredFileWorkingCopy::trace`], com o recurso e o `typeId` como argumentos do log (formatados no
//!   fim da mensagem).
//! - **Escutas e ações.** O que o `dispose()` ou o aviso de erro mantêm vivo segura a working copy por `Weak`, para o ciclo
//!   `Rc` não vazar (no JavaScript o coletor de lixo cuida disso).
//! - **`IEditorService.save`.** Vem do recorte [`IEditorServiceSave`] (o `IEditorService` reduzido ainda não o tem).
//! - **`IWorkingCopyFileService`.** `hasSaveParticipants` e `runSaveParticipants` são os métodos acrescentados ao contrato
//!   reduzido de `textfile/collaborators.rs`.
//! - **`save?` do modelo** (método opcional) é [`IStoredFileWorkingCopyModel::save`], que devolve `None` quando o modelo
//!   não o implementa.

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use je_base::actions::{ActionRef, ToActionProps, to_action};
use je_base::async_::{TaskSequentializer, async_fn, race_cancellation, timeout};
use je_base::buffer::VSBufferReadableStream;
use je_base::cancellation::{CancellationToken, CancellationTokenSource};
use je_base::dynamic::Dynamic;
use je_base::error_message::to_error_message_of;
use je_base::errors::{self, Error};
use je_base::event::{self, Emitter, Event};
use je_base::hash::hash;
use je_base::lifecycle::{Disposable, IDisposable};
use je_base::nls::localize;
use je_base::promise::{MaybePromise, Promise};
use je_base::runtime::date_now;
use je_base::uri::URI;
use je_platform::files::file_service::FileContentSource;
use je_platform::files::files::{
    ETAG_DISABLED, FileOperationError, FileOperationResult, FileReadLimits, FileReadStreamOptions, FileStat, FileStatWithMetadata, ReadFileOptions, WriteFileOptions,
};
use je_platform::files::i_file_service::IFileService;
use je_platform::log::LogService;
use je_platform::notification::notification::{INotificationService, Notification, NotificationActions, Severity};

use crate::common::editor::{RevertOptions, SaveOptions, SaveReason, SaveSource};
use crate::services::files::elevated_file_service::IElevatedFileService;
use crate::services::files_configuration::files_configuration_service::{IFilesConfigurationService, Readonly};
use crate::services::textfile::collaborators::{
    IEditorServiceSave, IProgress, IProgressService, IWorkingCopyFileService, ProgressCancellable, ProgressLocation, ProgressOptions, ProgressStep, SaveEditorsOptions,
    StoredFileWorkingCopySaveParticipantContext,
};
use crate::services::working_copy::file_working_copy::{IFileWorkingCopy, IFileWorkingCopyModel, IFileWorkingCopyModelFactory, SnapshotContext};
use crate::services::working_copy::resource_working_copy::{IResourceWorkingCopy, ResourceWorkingCopy};
use crate::services::working_copy::working_copy::{IWorkingCopy, WorkingCopyBackup, WorkingCopyBackupContent, WorkingCopyCapabilities, WorkingCopyIdentifier, WorkingCopySaveEvent};
use crate::services::working_copy::working_copy_backup::{IWorkingCopyBackupService, ResolvedWorkingCopyBackup};
use crate::services::working_copy::working_copy_editor_service::IWorkingCopyEditorService;
use crate::services::working_copy::working_copy_service::IWorkingCopyService;

/// `StoredFileWorkingCopy.UNDO_REDO_SAVE_PARTICIPANTS_AUTO_SAVE_THROTTLE_THRESHOLD`.
const UNDO_REDO_SAVE_PARTICIPANTS_AUTO_SAVE_THROTTLE_THRESHOLD: u64 = 500;

/// `IStoredFileWorkingCopyModelFactory<M>`: stored file specific working copy model factory.
pub trait IStoredFileWorkingCopyModelFactory<M: IStoredFileWorkingCopyModel + 'static>: IFileWorkingCopyModelFactory<M> {}

/// A fábrica de arquivo armazenado não acrescenta nada à fábrica de arquivo (`interface ... extends ... { }`): toda fábrica
/// concreta de arquivo é também uma de arquivo armazenado. (Sem `?Sized`: com ele o objeto `dyn IStoredFile...` colidiria
/// com a implementação automática dele mesmo, o erro E0371.)
impl<M: IStoredFileWorkingCopyModel + 'static, T: IFileWorkingCopyModelFactory<M>> IStoredFileWorkingCopyModelFactory<M> for T {}

/// `IStoredFileWorkingCopyModelContentChangedEvent`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct StoredFileWorkingCopyModelContentChangedEvent {
    /// Flag that indicates that this event was generated while undoing.
    pub is_undoing: bool,
    /// Flag that indicates that this event was generated while redoing.
    pub is_redoing: bool,
}

/// `IStoredFileWorkingCopyModel`: the underlying model of a stored file working copy provides some methods for the stored
/// file working copy to function. The model is typically only available after the working copy has been resolved via it's
/// `resolve()` method.
pub trait IStoredFileWorkingCopyModel: IFileWorkingCopyModel {
    /// O `onDidChangeContent` com o tipo do evento do stored (o do [`IFileWorkingCopyModel`] é o sinal sem dados).
    fn on_did_change_stored_content(&self) -> Event<StoredFileWorkingCopyModelContentChangedEvent>;

    /// A version ID of the model. If a `onDidChangeContent` is fired from the model and the last known saved `versionId`
    /// matches with the `model.versionId`, the stored file working copy will discard any dirty state.
    ///
    /// A use case is the following:
    /// - a stored file working copy gets edited and thus dirty
    /// - the user triggers undo to revert the changes
    /// - at this point the `versionId` should match the one we had saved
    ///
    /// This requires the model to be aware of undo/redo operations.
    fn version_id(&self) -> Dynamic;

    /// Close the current undo-redo element. This offers a way to create an undo/redo stop point.
    ///
    /// This method may for example be called right before the save is triggered so that the user can always undo back to
    /// the state before saving.
    fn push_stack_element(&self);

    /// Optionally allows a stored file working copy model to implement the `save` method. This allows to implement a more
    /// efficient save logic compared to the default which is to ask the model for a `snapshot` and then writing that to
    /// the model's resource.
    ///
    /// `None` quando o modelo não implementa o método (o `typeof model.save === 'function'` do original); nesse caso a
    /// working copy pede o `snapshot` e escreve pelo serviço de arquivos.
    fn save(&self, _options: WriteFileOptions, _token: CancellationToken) -> Option<Promise<FileStatWithMetadata>> {
        None
    }
}

/// `StoredFileWorkingCopyState`: states the stored file working copy can be in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StoredFileWorkingCopyState {
    /// A stored file working copy is saved.
    Saved,
    /// A stored file working copy is dirty.
    Dirty,
    /// A stored file working copy is currently being saved but this operation has not completed yet.
    PendingSave,
    /// A stored file working copy is in conflict mode when changes cannot be saved because the underlying file has
    /// changed. Stored file working copies in conflict mode are always dirty.
    Conflict,
    /// A stored file working copy is in orphan state when the underlying file has been deleted.
    Orphan,
    /// Any error that happens during a save that is not causing the `StoredFileWorkingCopyState.CONFLICT` state. Stored
    /// file working copies in error mode are always dirty.
    Error,
}

/// `IStoredFileWorkingCopySaveOptions`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct StoredFileWorkingCopySaveOptions {
    /// O `ISaveOptions` de que herda (`reason`, `source`, `force`, `skipSaveParticipants`, `availableFileSystems`).
    pub base: SaveOptions,
    /// Save the stored file working copy with an attempt to unlock it.
    pub write_unlock: Option<bool>,
    /// Save the stored file working copy with elevated privileges.
    ///
    /// Note: This may not be supported in all environments.
    pub write_elevated: Option<bool>,
    /// Allows to write to a stored file working copy even if it has been modified on disk. This should only be triggered
    /// from an explicit user action.
    pub ignore_modified_since: Option<bool>,
    /// If set, will bubble up the stored file working copy save error to the caller instead of handling it.
    pub ignore_error_handler: Option<bool>,
}

/// `IStoredFileWorkingCopySaveAsOptions`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct StoredFileWorkingCopySaveAsOptions {
    /// O `IStoredFileWorkingCopySaveOptions` de que herda.
    pub options: StoredFileWorkingCopySaveOptions,
    /// Optional URI of the resource the text file is saved from if known.
    pub from: Option<URI>,
}

impl StoredFileWorkingCopySaveAsOptions {
    /// As opções do `IWorkingCopy.save(ISaveOptions)` como `IStoredFileWorkingCopySaveAsOptions` (os demais campos ausentes).
    pub fn from_save_options(options: SaveOptions) -> Self {
        Self { options: StoredFileWorkingCopySaveOptions { base: options, ..StoredFileWorkingCopySaveOptions::default() }, from: None }
    }

    pub fn reason(&self) -> Option<SaveReason> {
        self.options.base.reason
    }

    pub fn source(&self) -> Option<SaveSource> {
        self.options.base.source.clone()
    }

    pub fn force(&self) -> bool {
        self.options.base.force.unwrap_or(false)
    }

    pub fn skip_save_participants(&self) -> bool {
        self.options.base.skip_save_participants.unwrap_or(false)
    }

    /// `{ ...options, ...change, reason: SaveReason.EXPLICIT }`: as opções das ações do aviso de erro de salvar.
    fn explicit_with(&self, change: impl FnOnce(&mut StoredFileWorkingCopySaveAsOptions)) -> Self {
        let mut options = self.clone();
        change(&mut options);
        options.options.base.reason = Some(SaveReason::Explicit);
        options
    }
}

/// `IStoredFileWorkingCopyResolver`: resolves the working copy in a safe way from an external working copy manager that
/// can make sure multiple parallel resolves execute properly.
pub type StoredFileWorkingCopyResolver = Rc<dyn Fn(Option<StoredFileWorkingCopyResolveOptions>) -> Promise<()>>;

/// `IStoredFileWorkingCopyResolveOptions`.
#[derive(Clone, Default)]
pub struct StoredFileWorkingCopyResolveOptions {
    /// The contents to use for the stored file working copy if known. If not provided, the contents will be retrieved from
    /// the underlying resource or backup if present.
    ///
    /// If contents are provided, the stored file working copy will be marked as dirty right from the beginning.
    pub contents: Option<VSBufferReadableStream>,
    /// Go to disk bypassing any cache of the stored file working copy if any.
    pub force_read_from_file: Option<bool>,
    /// If provided, the size of the file will be checked against the limits and an error will be thrown if any limit is
    /// exceeded.
    pub limits: Option<FileReadLimits>,
}

/// `IStoredFileWorkingCopyBackupMetaData`: metadata associated with a stored file working copy backup (o `IBackupMetaData`
/// do `TextFileEditorModel` tem a mesma forma).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StoredFileWorkingCopyBackupMetaData {
    pub mtime: i64,
    pub ctime: i64,
    pub size: u64,
    pub etag: String,
    pub orphaned: bool,
}

impl StoredFileWorkingCopyBackupMetaData {
    /// O `IWorkingCopyBackupMeta` (um objeto) com os cinco campos.
    pub fn to_dynamic(&self) -> Dynamic {
        Dynamic::object([
            ("mtime", Dynamic::Number(self.mtime as f64)),
            ("ctime", Dynamic::Number(self.ctime as f64)),
            ("size", Dynamic::Number(self.size as f64)),
            ("etag", Dynamic::string(&self.etag)),
            ("orphaned", Dynamic::Bool(self.orphaned)),
        ])
    }

    /// Lê o `meta` do backup (um campo ausente ou de outro tipo vira o valor neutro).
    pub fn from_dynamic(meta: &Dynamic) -> Self {
        let number = |key: &str| match meta.get_str(key) {
            Dynamic::Number(value) => value,
            _ => 0.0,
        };
        Self {
            mtime: number("mtime") as i64,
            ctime: number("ctime") as i64,
            size: number("size") as u64,
            etag: match meta.get_str("etag") {
                Dynamic::String(value) => value.to_string_lossy(),
                _ => ETAG_DISABLED.to_string(),
            },
            orphaned: matches!(meta.get_str("orphaned"), Dynamic::Bool(true)),
        }
    }
}

/// `IStoredFileWorkingCopySaveEvent`.
#[derive(Clone, Debug)]
pub struct StoredFileWorkingCopySaveEvent {
    /// O `reason` e o `source` herdados de `IWorkingCopySaveEvent`.
    pub event: WorkingCopySaveEvent,
    /// The resolved stat from the save operation.
    pub stat: FileStatWithMetadata,
}

/// `IStoredFileWorkingCopy<M>`: a stored file based `IWorkingCopy` is backed by a `URI` from a known file system
/// provider. Given this assumption, a lot of functionality can be built on top, such as saving in a secure way to prevent
/// data loss.
pub trait IStoredFileWorkingCopy: IResourceWorkingCopy + IFileWorkingCopy {
    /// An event for when a stored file working copy was resolved.
    fn on_did_resolve(&self) -> Event<()>;

    /// An event for when a stored file working copy was saved successfully (o `onDidSave` com o `stat`).
    fn on_did_save_stored(&self) -> Event<StoredFileWorkingCopySaveEvent>;

    /// An event indicating that a stored file working copy save operation failed.
    fn on_did_save_error(&self) -> Event<()>;

    /// An event for when the readonly state of the stored file working copy changes.
    fn on_did_change_readonly(&self) -> Event<()>;

    /// O `model` do stored (`IStoredFileWorkingCopyModel`), `None` enquanto a working copy não foi resolvida.
    fn stored_model(&self) -> Option<Rc<dyn IStoredFileWorkingCopyModel>>;

    /// Resolves a stored file working copy.
    fn resolve_stored(&self, options: Option<StoredFileWorkingCopyResolveOptions>) -> Promise<()>;

    /// Explicitly sets the working copy to be modified.
    fn mark_modified(&self);

    /// Whether the stored file working copy is in the provided `state` or not.
    fn has_state(&self, state: StoredFileWorkingCopyState) -> bool;

    /// Allows to join a state change away from the provided `state` (só o `PendingSave` pode ser esperado: o parâmetro
    /// existe pelo contrato, e o original também o ignora).
    fn join_state(&self, state: StoredFileWorkingCopyState) -> Promise<()>;

    /// Whether the stored file working copy is readonly or not.
    fn is_readonly(&self) -> Readonly;

    /// Asks the stored file working copy to save. If the stored file working copy was dirty, it is expected to be
    /// non-dirty after this operation has finished. Resolves to `true` if the operation was successful and `false`
    /// otherwise.
    fn save_stored(&self, options: Option<StoredFileWorkingCopySaveAsOptions>) -> Promise<bool>;
}

/// Os serviços que a `StoredFileWorkingCopy` recebe por injeção, na ordem do construtor do original (o `typeId`, o
/// recurso, o nome, a fábrica e o resolvedor são argumentos).
#[derive(Clone)]
pub struct StoredFileWorkingCopyServices {
    pub file_service: Rc<dyn IFileService>,
    pub log_service: Rc<dyn LogService>,
    pub working_copy_file_service: Rc<dyn IWorkingCopyFileService>,
    pub files_configuration_service: Rc<dyn IFilesConfigurationService>,
    pub working_copy_backup_service: Rc<dyn IWorkingCopyBackupService>,
    pub working_copy_service: Rc<dyn IWorkingCopyService>,
    pub notification_service: Rc<dyn INotificationService>,
    pub working_copy_editor_service: Rc<dyn IWorkingCopyEditorService>,
    /// O `IEditorService`, só o `save` (ver [`IEditorServiceSave`]).
    pub editor_service: Rc<dyn IEditorServiceSave>,
    pub elevated_file_service: Rc<dyn IElevatedFileService>,
    pub progress_service: Rc<dyn IProgressService>,
}

/// Os campos de `IFileStreamContent` que a resolução lê (o `IBaseFileStatWithMetadata` do conteúdo).
struct ContentStat {
    name: String,
    mtime: i64,
    ctime: i64,
    size: u64,
    etag: String,
    readonly: bool,
    locked: bool,
}

impl ContentStat {
    /// Os campos do stat que o serviço de arquivos devolveu.
    fn of(stat: &FileStatWithMetadata) -> Self {
        Self {
            name: stat.name.clone(),
            mtime: stat.mtime(),
            ctime: stat.ctime(),
            size: stat.size(),
            etag: stat.etag().to_string(),
            readonly: stat.readonly(),
            locked: stat.locked(),
        }
    }

    /// O stat do arquivo resolvido (`executable: false`, arquivo, sem filhos), do recurso dado.
    fn into_file_stat(self, resource: &URI) -> FileStatWithMetadata {
        FileStatWithMetadata::new(FileStat {
            resource: resource.clone(),
            name: self.name,
            size: Some(self.size),
            mtime: Some(self.mtime),
            ctime: Some(self.ctime),
            etag: Some(self.etag),
            readonly: Some(self.readonly),
            locked: Some(self.locked),
            executable: Some(false),
            is_file: true,
            is_directory: false,
            is_symbolic_link: false,
            children: None,
        })
    }
}

/// `Date.now()` como o `i64` que o `mtime` e o `ctime` usam.
fn now_millis() -> i64 {
    date_now() as i64
}

/// `StoredFileWorkingCopy<M>`.
pub struct StoredFileWorkingCopy<M: IStoredFileWorkingCopyModel + 'static> {
    this: Weak<StoredFileWorkingCopy<M>>,
    /// O `ResourceWorkingCopy` de que a classe herda.
    base: Rc<ResourceWorkingCopy>,
    /// O `_register(...)` da classe.
    store: Rc<Disposable>,
    type_id: String,
    name: String,
    model_factory: Rc<dyn IStoredFileWorkingCopyModelFactory<M>>,
    external_resolver: StoredFileWorkingCopyResolver,
    services: StoredFileWorkingCopyServices,

    model: RefCell<Option<Rc<M>>>,

    //#region Events
    on_did_change_content: Rc<Emitter<()>>,
    on_did_resolve: Rc<Emitter<()>>,
    on_did_change_dirty: Rc<Emitter<()>>,
    on_did_save_error: Rc<Emitter<()>>,
    on_did_save: Rc<Emitter<StoredFileWorkingCopySaveEvent>>,
    on_did_revert: Rc<Emitter<()>>,
    on_did_change_readonly: Rc<Emitter<()>>,
    //#endregion
    dirty: Cell<bool>,
    saved_version_id: RefCell<Dynamic>,

    last_resolved_file_stat: RefCell<Option<FileStatWithMetadata>>,
    ignore_dirty_on_model_content_change: Cell<bool>,

    version_id: Cell<usize>,
    last_content_change_from_undo_redo: Cell<Option<i64>>,
    save_sequentializer: TaskSequentializer,
    ignore_save_from_save_participants: Cell<bool>,

    in_conflict_mode: Cell<bool>,
    in_error_mode: Cell<bool>,
}

impl<M: IStoredFileWorkingCopyModel + 'static> StoredFileWorkingCopy<M> {
    /// `constructor(typeId, resource, name, modelFactory, externalResolver, ...services)`.
    pub fn new(
        type_id: String,
        resource: URI,
        name: String,
        model_factory: Rc<dyn IStoredFileWorkingCopyModelFactory<M>>,
        external_resolver: StoredFileWorkingCopyResolver,
        services: StoredFileWorkingCopyServices,
    ) -> Rc<Self> {
        let base = ResourceWorkingCopy::new(resource, services.file_service.clone());
        let store = Disposable::new();

        let on_did_change_content = Emitter::new();
        let on_did_resolve = Emitter::new();
        let on_did_change_dirty = Emitter::new();
        let on_did_save_error = Emitter::new();
        let on_did_save = Emitter::new();
        let on_did_revert = Emitter::new();
        let on_did_change_readonly = Emitter::new();
        store.register(on_did_change_content.clone());
        store.register(on_did_resolve.clone());
        store.register(on_did_change_dirty.clone());
        store.register(on_did_save_error.clone());
        store.register(on_did_save.clone());
        store.register(on_did_revert.clone());
        store.register(on_did_change_readonly.clone());

        let this = Rc::new_cyclic(|this: &Weak<StoredFileWorkingCopy<M>>| Self {
            this: this.clone(),
            base,
            store,
            type_id,
            name,
            model_factory,
            external_resolver,
            services,
            model: RefCell::new(None),
            on_did_change_content,
            on_did_resolve,
            on_did_change_dirty,
            on_did_save_error,
            on_did_save,
            on_did_revert,
            on_did_change_readonly,
            dirty: Cell::new(false),
            saved_version_id: RefCell::new(Dynamic::Undefined),
            last_resolved_file_stat: RefCell::new(None),
            ignore_dirty_on_model_content_change: Cell::new(false),
            version_id: Cell::new(0),
            last_content_change_from_undo_redo: Cell::new(None),
            save_sequentializer: TaskSequentializer::new(),
            ignore_save_from_save_participants: Cell::new(false),
            in_conflict_mode: Cell::new(false),
            in_error_mode: Cell::new(false),
        });

        // Make known to working copy service
        let working_copy: Rc<dyn IWorkingCopy> = this.clone();
        this.store.register(this.services.working_copy_service.register_working_copy(working_copy));

        this.register_listeners();

        this
    }

    fn rc(&self) -> Rc<Self> {
        self.this.upgrade().expect("a working copy está viva enquanto é usada")
    }

    fn register_listeners(&self) {
        let on_did_change_readonly = self.on_did_change_readonly.clone();
        self.store.register(self.services.files_configuration_service.on_did_change_readonly().listen(move |_| on_did_change_readonly.fire(())));
    }

    /// `model`: o modelo tipado, `None` enquanto a working copy não foi resolvida.
    pub fn model(&self) -> Option<Rc<M>> {
        self.model.borrow().clone()
    }

    /// `lastResolvedFileStat` (público no original porque os testes o leem).
    pub fn last_resolved_file_stat(&self) -> Option<FileStatWithMetadata> {
        self.last_resolved_file_stat.borrow().clone()
    }

    /// `error.fileOperationResult`.
    fn file_operation_result_of(error: &Error) -> Option<FileOperationResult> {
        FileOperationError::from_error(error).map(|error| error.file_operation_result)
    }

    fn is_readonly_flag(&self) -> bool {
        self.is_readonly().is_readonly()
    }

    //#region Dirty

    fn set_dirty(&self, dirty: bool) {
        if !self.is_resolved() {
            return; // only resolved working copies can be marked dirty
        }

        // Track dirty state and version id
        let was_dirty = self.dirty.get();
        drop(self.do_set_dirty(dirty)); // a função de desfazer não é usada aqui, como no original

        // Emit as Event if dirty changed
        if dirty != was_dirty {
            self.on_did_change_dirty.fire(());
        }
    }

    /// `doSetDirty(dirty)`: devolve o fecho que desfaz a chamada.
    fn do_set_dirty(&self, dirty: bool) -> Box<dyn FnOnce()> {
        let was_dirty = self.dirty.get();
        let was_in_conflict_mode = self.in_conflict_mode.get();
        let was_in_error_mode = self.in_error_mode.get();
        let old_saved_version_id = self.saved_version_id.borrow().clone();

        if !dirty {
            self.dirty.set(false);
            self.in_conflict_mode.set(false);
            self.in_error_mode.set(false);

            // we remember the models alternate version id to remember when the version
            // of the model matches with the saved version on disk. we need to keep this
            // in order to find out if the model changed back to a saved version (e.g.
            // when undoing long enough to reach to a version that is saved and then to
            // clear the dirty flag)
            if let Some(model) = self.model() {
                *self.saved_version_id.borrow_mut() = model.version_id();
            }
        } else {
            self.dirty.set(true);
        }

        // Return function to revert this call
        let this = self.rc();
        Box::new(move || {
            this.dirty.set(was_dirty);
            this.in_conflict_mode.set(was_in_conflict_mode);
            this.in_error_mode.set(was_in_error_mode);
            *this.saved_version_id.borrow_mut() = old_saved_version_id;
        })
    }

    //#endregion

    //#region Resolve

    fn do_resolve(&self, options: Option<StoredFileWorkingCopyResolveOptions>) -> Promise<()> {
        // First check if we have contents to use for the working copy
        if let Some(contents) = options.as_ref().and_then(|options| options.contents.clone()) {
            return self.resolve_from_buffer(contents);
        }

        // Second, check if we have a backup to resolve from (only for new working copies)
        let this = self.rc();
        async_fn(async move {
            let is_new = !this.is_resolved();
            if is_new {
                let resolved_from_backup = this.resolve_from_backup().await?;
                if resolved_from_backup {
                    return Ok(());
                }
            }

            // Finally, resolve from file resource
            this.resolve_from_file(options).await
        })
    }

    fn resolve_from_buffer(&self, buffer: VSBufferReadableStream) -> Promise<()> {
        self.trace("resolveFromBuffer()");

        let this = self.rc();
        async_fn(async move {
            // Try to resolve metdata from disk
            let (mtime, ctime, size, etag);
            match this.services.file_service.stat(this.base.resource()).await {
                Ok(metadata) => {
                    mtime = metadata.mtime();
                    ctime = metadata.ctime();
                    size = metadata.size();
                    etag = metadata.etag().to_string();

                    // Clear orphaned state when resolving was successful
                    this.base.set_orphaned(false);
                }
                Err(error) => {
                    // Put some fallback values in error case
                    mtime = now_millis();
                    ctime = now_millis();
                    size = 0;
                    etag = ETAG_DISABLED.to_string();

                    // Apply orphaned state based on error code
                    this.base.set_orphaned(Self::file_operation_result_of(&error) == Some(FileOperationResult::FileNotFound));
                }
            }

            // Resolve with buffer
            this.resolve_from_content(ContentStat { name: this.name.clone(), mtime, ctime, size, etag, readonly: false, locked: false }, buffer, true /* dirty (resolved from buffer) */)
                .await
        })
    }

    fn resolve_from_backup(&self) -> Promise<bool> {
        let this = self.rc();
        async_fn(async move {
            // Resolve backup if any
            let backup = this.services.working_copy_backup_service.resolve(&WorkingCopyIdentifier::of(&*this)).await?;

            // Abort if someone else managed to resolve the working copy by now
            let is_new = !this.is_resolved();
            if !is_new {
                this.trace("resolveFromBackup() - exit - withoutresolving because previously new file working copy got created meanwhile");

                return Ok(true); // imply that resolving has happened in another operation
            }

            // Try to resolve from backup if we have any
            if let Some(backup) = backup {
                this.do_resolve_from_backup(backup).await?;

                return Ok(true);
            }

            // Otherwise signal back that resolving did not happen
            Ok(false)
        })
    }

    fn do_resolve_from_backup(&self, backup: ResolvedWorkingCopyBackup) -> Promise<()> {
        self.trace("doResolveFromBackup()");

        let this = self.rc();
        async_fn(async move {
            let meta = backup.meta.as_ref().map(StoredFileWorkingCopyBackupMetaData::from_dynamic);

            // Resolve with backup
            this.resolve_from_content(
                ContentStat {
                    name: this.name.clone(),
                    mtime: meta.as_ref().map_or_else(now_millis, |meta| meta.mtime),
                    ctime: meta.as_ref().map_or_else(now_millis, |meta| meta.ctime),
                    size: meta.as_ref().map_or(0, |meta| meta.size),
                    etag: meta.as_ref().map_or_else(|| ETAG_DISABLED.to_string(), |meta| meta.etag.clone()), // etag disabled if unknown!
                    readonly: false,
                    locked: false,
                },
                backup.value.clone(),
                true, /* dirty (resolved from backup) */
            )
            .await?;

            // Restore orphaned flag based on state
            if meta.is_some_and(|meta| meta.orphaned) {
                this.base.set_orphaned(true);
            }
            Ok(())
        })
    }

    fn resolve_from_file(&self, options: Option<StoredFileWorkingCopyResolveOptions>) -> Promise<()> {
        self.trace("resolveFromFile()");

        let force_read_from_file = options.as_ref().and_then(|options| options.force_read_from_file).unwrap_or(false);

        // Decide on etag
        let etag: Option<String> = if force_read_from_file {
            Some(ETAG_DISABLED.to_string()) // disable ETag if we enforce to read from disk
        } else {
            self.last_resolved_file_stat.borrow().as_ref().map(|stat| stat.etag().to_string()) // otherwise respect etag to support caching
        };

        // Remember current version before doing any long running operation
        // to ensure we are not changing a working copy that was changed
        // meanwhile
        let current_version_id = self.version_id.get();

        // Resolve Content
        let read_options = ReadFileOptions {
            etag,
            stream: FileReadStreamOptions { limit_size: options.as_ref().and_then(|options| options.limits).and_then(|limits| limits.size), ..FileReadStreamOptions::default() },
            ..ReadFileOptions::default()
        };
        let reading = self.services.file_service.read_file_stream(self.base.resource(), Some(read_options), None);

        let this = self.rc();
        async_fn(async move {
            match reading.await {
                Ok(content) => {
                    // Clear orphaned state when resolving was successful
                    this.base.set_orphaned(false);

                    // Return early if the working copy content has changed
                    // meanwhile to prevent loosing any changes
                    if current_version_id != this.version_id.get() {
                        this.trace("resolveFromFile() - exit - without resolving because file working copy content changed");

                        return Ok(());
                    }

                    this.resolve_from_content(ContentStat::of(&content.stat), content.value.clone(), false /* not dirty (resolved from file) */).await
                }
                Err(error) => {
                    let file_error = FileOperationError::from_error(&error);
                    let result = file_error.as_ref().map(|error| error.file_operation_result);

                    // Apply orphaned state based on error code
                    this.base.set_orphaned(result == Some(FileOperationResult::FileNotFound));

                    // NotModified status is expected and can be handled gracefully
                    // if we are resolved. We still want to update our last resolved
                    // stat to e.g. detect changes to the file's readonly state
                    if this.is_resolved() && result == Some(FileOperationResult::FileNotModifiedSince) {
                        if let Some(stat) = file_error.and_then(|error| error.stat) {
                            this.update_last_resolved_file_stat(stat);
                        }

                        return Ok(());
                    }

                    // Unless we are forced to read from the file, ignore when a working copy has
                    // been resolved once and the file was deleted meanwhile. Since we already have
                    // the working copy resolved, we can return to this state and update the orphaned
                    // flag to indicate that this working copy has no version on disk anymore.
                    if this.is_resolved() && result == Some(FileOperationResult::FileNotFound) && !force_read_from_file {
                        return Ok(());
                    }

                    // Otherwise bubble up the error
                    Err(error)
                }
            }
        })
    }

    fn resolve_from_content(&self, content: ContentStat, value: VSBufferReadableStream, dirty: bool) -> Promise<()> {
        self.trace("resolveFromContent() - enter");

        // Return early if we are disposed
        if self.is_disposed() {
            self.trace("resolveFromContent() - exit - because working copy is disposed");

            return Promise::resolved(());
        }

        // Update our resolved disk stat
        self.update_last_resolved_file_stat(content.into_file_stat(self.base.resource()));

        let this = self.rc();
        async_fn(async move {
            // Update existing model if we had been resolved
            if this.is_resolved() {
                this.do_update_model(value).await?;
            }
            // Create new model otherwise
            else {
                this.do_create_model(value).await?;
            }

            // Update working copy dirty flag. This is very important to call
            // in both cases of dirty or not because it conditionally updates
            // the `savedVersionId` to determine the version when to consider
            // the working copy as saved again (e.g. when undoing back to the
            // saved state)
            this.set_dirty(dirty);

            // Emit as event
            this.on_did_resolve.fire(());
            Ok(())
        })
    }

    fn do_create_model(&self, contents: VSBufferReadableStream) -> Promise<()> {
        self.trace("doCreateModel()");

        let creating = self.model_factory.create_model(self.base.resource(), contents, CancellationToken::None);

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

    fn do_update_model(&self, contents: VSBufferReadableStream) -> Promise<()> {
        self.trace("doUpdateModel()");

        // Update model value in a block that ignores content change events for dirty tracking
        self.ignore_dirty_on_model_content_change.set(true);
        let updating = self.model().map(|model| model.update(contents, CancellationToken::None));

        let this = self.rc();
        async_fn(async move {
            let result = match updating {
                Some(updating) => updating.await,
                None => Ok(()),
            };
            this.ignore_dirty_on_model_content_change.set(false);
            result
        })
    }

    fn install_model_listeners(&self, model: &Rc<M>) {
        // See https://github.com/microsoft/vscode/issues/30189
        // This code has been extracted to a different method because it caused a memory leak
        // where `value` was captured in the content change listener closure scope.

        // Content Change
        let (this, weak_model) = (self.this.clone(), Rc::downgrade(model));
        self.store.register(model.on_did_change_stored_content().listen(move |e| {
            if let (Some(this), Some(model)) = (this.upgrade(), weak_model.upgrade()) {
                this.on_model_content_changed(&model, e.is_undoing || e.is_redoing);
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

    fn on_model_content_changed(&self, model: &M, is_undoing_or_redoing: bool) {
        self.trace("onModelContentChanged() - enter");

        // In any case increment the version id because it tracks the content state of the model at all times
        self.version_id.set(self.version_id.get() + 1);
        self.trace(&format!("onModelContentChanged() - new versionId {}", self.version_id.get()));

        // Remember when the user changed the model through a undo/redo operation.
        // We need this information to throttle save participants to fix
        // https://github.com/microsoft/vscode/issues/102542
        if is_undoing_or_redoing {
            self.last_content_change_from_undo_redo.set(Some(now_millis()));
        }

        // We mark check for a dirty-state change upon model content change, unless:
        // - explicitly instructed to ignore it (e.g. from model.resolve())
        // - the model is readonly (in that case we never assume the change was done by the user)
        if !self.ignore_dirty_on_model_content_change.get() && !self.is_readonly_flag() {
            // The contents changed as a matter of Undo and the version reached matches the saved one
            // In this case we clear the dirty flag and emit a SAVED event to indicate this state.
            if model.version_id() == *self.saved_version_id.borrow() {
                self.trace("onModelContentChanged() - model content changed back to last saved version");

                // Clear flags
                let was_dirty = self.dirty.get();
                self.set_dirty(false);

                // Emit revert event if we were dirty
                if was_dirty {
                    self.on_did_revert.fire(());
                }
            }
            // Otherwise the content has changed and we signal this as becoming dirty
            else {
                self.trace("onModelContentChanged() - model content changed and marked as dirty");

                // Mark as dirty
                self.set_dirty(true);
            }
        }

        // Emit as event
        self.on_did_change_content.fire(());
    }

    fn force_resolve_from_file(&self) -> Promise<()> {
        if self.is_disposed() {
            return Promise::resolved(()); // return early when the working copy is invalid
        }

        // We go through the resolver to make
        // sure this kind of `resolve` is properly
        // running in sequence with any other running
        // `resolve` if any, including subsequent runs
        // that are triggered right after.
        (self.external_resolver)(Some(StoredFileWorkingCopyResolveOptions { force_read_from_file: Some(true), ..StoredFileWorkingCopyResolveOptions::default() }))
    }

    //#endregion

    //#region Save

    fn do_save(&self, mut options: StoredFileWorkingCopySaveAsOptions) -> Promise<()> {
        if options.reason().is_none() {
            options.options.base.reason = Some(SaveReason::Explicit);
        }

        let version_id = self.version_id.get();
        self.trace(&format!("doSave({version_id}) - enter with versionId {version_id}"));

        // Return early if saved from within save participant to break recursion
        //
        // Scenario: a save participant triggers a save() on the working copy
        if self.ignore_save_from_save_participants.get() {
            self.trace(&format!("doSave({version_id}) - exit - refusing to save() recursively from save participant"));

            return Promise::resolved(());
        }

        // Lookup any running save for this versionId and return it if found
        //
        // Scenario: user invoked the save action multiple times quickly for the same contents
        //           while the save was not yet finished to disk
        //
        if self.save_sequentializer.is_running_task(version_id) {
            self.trace(&format!("doSave({version_id}) - exit - found a running save for versionId {version_id}"));

            return self.save_sequentializer.running().unwrap_or_else(|| Promise::resolved(()));
        }

        // Return early if not dirty (unless forced)
        //
        // Scenario: user invoked save action even though the working copy is not dirty
        if !options.force() && !self.dirty.get() {
            self.trace(&format!(
                "doSave({version_id}) - exit - because not dirty and/or versionId is different (this.isDirty: {}, this.versionId: {})",
                self.dirty.get(),
                self.version_id.get()
            ));

            return Promise::resolved(());
        }

        // Return if currently saving by storing this save request as the next save that should happen.
        // Never ever must 2 saves execute at the same time because this can lead to dirty writes and race conditions.
        //
        // Scenario A: auto save was triggered and is currently busy saving to disk. this takes long enough that another auto save
        //             kicks in.
        // Scenario B: save is very slow (e.g. network share) and the user manages to change the working copy and trigger another save
        //             while the first save has not returned yet.
        //
        if self.save_sequentializer.is_running() {
            self.trace(&format!("doSave({version_id}) - exit - because busy saving"));

            // Indicate to the save sequentializer that we want to
            // cancel the running operation so that ours can run
            // before the running one finishes.
            // Currently this will try to cancel running save
            // participants and running snapshots from the
            // save operation, but not the actual save which does
            // not support cancellation yet.
            self.save_sequentializer.cancel_running();

            // Queue this as the upcoming save and return
            let this = self.rc();
            return self.save_sequentializer.queue(move || this.do_save(options));
        }

        // Push all edit operations to the undo stack so that the user has a chance to
        // Ctrl+Z back to the saved version.
        if let Some(model) = self.model() {
            model.push_stack_element();
        }

        let save_cancellation = CancellationTokenSource::new(None);

        let this = self.rc();
        let (task_cancellation, cancel_cancellation, finally_cancellation) = (save_cancellation.clone(), save_cancellation.clone(), save_cancellation);
        self.services
            .progress_service
            .with_progress(
                ProgressOptions {
                    title: Some(localize("saveParticipants", "Saving '{0}'", &[self.name.as_str()])),
                    cancellable: Some(ProgressCancellable::Yes),
                    delay: Some(if self.dirty.get() { 3000 } else { 5000 }),
                    ..ProgressOptions::at(ProgressLocation::Window)
                },
                Box::new(move |progress| this.do_save_sequential(version_id, options, progress, task_cancellation)),
                Some(Rc::new(move || cancel_cancellation.cancel())),
            )
            .finally(move || finally_cancellation.dispose_with(false))
    }

    fn do_save_sequential(
        &self,
        version_id: usize,
        options: StoredFileWorkingCopySaveAsOptions,
        progress: Rc<dyn IProgress<ProgressStep>>,
        save_cancellation: CancellationTokenSource,
    ) -> Promise<()> {
        let this = self.rc();
        let on_cancel = save_cancellation.clone();
        let task = async_fn(async move {
            // A save participant can still change the working copy now
            // and since we are so close to saving we do not want to trigger
            // another auto save or similar, so we block this
            // In addition we update our version right after in case it changed
            // because of a working copy change
            // Save participants can also be skipped through API.
            if this.is_resolved() && !options.skip_save_participants() && this.services.working_copy_file_service.has_save_participants() {
                let participants: Result<(), Error> = async {
                    // Measure the time it took from the last undo/redo operation to this save. If this
                    // time is below `UNDO_REDO_SAVE_PARTICIPANTS_THROTTLE_THRESHOLD`, we make sure to
                    // delay the save participant for the remaining time if the reason is auto save.
                    //
                    // This fixes the following issue:
                    // - the user has configured auto save with delay of 100ms or shorter
                    // - the user has a save participant enabled that modifies the file on each save
                    // - the user types into the file and the file gets saved
                    // - the user triggers undo operation
                    // - this will undo the save participant change but trigger the save participant right after
                    // - the user has no chance to undo over the save participant
                    //
                    // Reported as: https://github.com/microsoft/vscode/issues/102542
                    if options.reason() == Some(SaveReason::Auto) {
                        if let Some(last_change) = this.last_content_change_from_undo_redo.get() {
                            let time_from_undo_redo_to_save = (now_millis() - last_change).max(0) as u64;
                            if time_from_undo_redo_to_save < UNDO_REDO_SAVE_PARTICIPANTS_AUTO_SAVE_THROTTLE_THRESHOLD {
                                timeout(UNDO_REDO_SAVE_PARTICIPANTS_AUTO_SAVE_THROTTLE_THRESHOLD - time_from_undo_redo_to_save).await?;
                            }
                        }
                    }

                    // Run save participants unless save was cancelled meanwhile
                    if !save_cancellation.token().is_cancellation_requested() {
                        this.ignore_save_from_save_participants.set(true);
                        let working_copy: Rc<dyn IStoredFileWorkingCopy> = this.clone();
                        let result = this
                            .services
                            .working_copy_file_service
                            .run_save_participants(
                                working_copy,
                                StoredFileWorkingCopySaveParticipantContext { reason: options.reason().unwrap_or(SaveReason::Explicit), saved_from: options.from.clone() },
                                progress.clone(),
                                save_cancellation.token(),
                            )
                            .await;
                        if let Err(err) = result {
                            if errors::is_cancellation_error(&err) && !save_cancellation.token().is_cancellation_requested() {
                                // participant wants to cancel this operation
                                save_cancellation.cancel();
                            }
                        }
                        this.ignore_save_from_save_participants.set(false);
                    }
                    Ok(())
                }
                .await;

                if let Err(error) = participants {
                    this.ignore_save_from_save_participants.set(false);
                    this.services.log_service.error(&format!(
                        "[stored file working copy] runSaveParticipants({version_id}) - resulted in an error: {error} {} {}",
                        this.base.resource().to_string(false),
                        this.type_id
                    ));
                }
            }

            // It is possible that a subsequent save is cancelling this
            // running save. As such we return early when we detect that.
            if save_cancellation.token().is_cancellation_requested() {
                return Ok(());
            }

            // We have to protect against being disposed at this point. It could be that the save() operation
            // was triggerd followed by a dispose() operation right after without waiting. Typically we cannot
            // be disposed if we are dirty, but if we are not dirty, save() and dispose() can still be triggered
            // one after the other without waiting for the save() to complete. If we are disposed(), we risk
            // saving contents to disk that are stale (see https://github.com/microsoft/vscode/issues/50942).
            // To fix this issue, we will not store the contents to disk when we got disposed.
            if this.is_disposed() {
                return Ok(());
            }

            // We require a resolved working copy from this point on, since we are about to write data to disk.
            let Some(model) = this.model() else {
                return Ok(());
            };

            // update versionId with its new value (if pre-save changes happened)
            let version_id = this.version_id.get();

            // Clear error flag since we are trying to save again
            this.in_error_mode.set(false);

            // Save to Disk. We mark the save operation as currently running with
            // the latest versionId because it might have changed from a save
            // participant triggering
            progress.report(ProgressStep { message: Some(localize("saveTextFile", "Writing into file...", &[])), ..ProgressStep::default() });
            this.trace(&format!("doSave({version_id}) - before write()"));
            let last_resolved_file_stat = this.last_resolved_file_stat.borrow().clone().ok_or_else(|| Error::new("Assertion Failed: argument is undefined or null"))?;

            let write_this = this.clone();
            let (write_cancellation, cancel_write) = (save_cancellation.clone(), save_cancellation.clone());
            let write = async_fn(async move {
                let this = write_this;
                let saved: Result<Option<FileStatWithMetadata>, Error> = async {
                    let write_file_options = WriteFileOptions {
                        mtime: Some(last_resolved_file_stat.mtime()),
                        etag: Some(
                            if options.options.ignore_modified_since.unwrap_or(false)
                                || !this.services.files_configuration_service.prevent_save_conflicts(&last_resolved_file_stat.resource, None)
                            {
                                ETAG_DISABLED.to_string()
                            } else {
                                last_resolved_file_stat.etag().to_string()
                            },
                        ),
                        unlock: options.options.write_unlock.unwrap_or(false),
                        ..WriteFileOptions::default()
                    };

                    // Delegate to working copy model save method if any
                    if let Some(saving) = model.save(write_file_options.clone(), write_cancellation.token()) {
                        return match saving.await {
                            Ok(stat) => Ok(Some(stat)),
                            Err(error) => {
                                if write_cancellation.token().is_cancellation_requested() {
                                    return Ok(None); // save was cancelled
                                }

                                Err(error)
                            }
                        };
                    }

                    // Otherwise ask for a snapshot and save via file services

                    // Snapshot working copy model contents
                    let snapshot = race_cancellation(&model.snapshot(SnapshotContext::Save, write_cancellation.token()), &write_cancellation.token()).await?;

                    // It is possible that a subsequent save is cancelling this
                    // running save. As such we return early when we detect that
                    // However, we do not pass the token into the file service
                    // because that is an atomic operation currently without
                    // cancellation support, so we dispose the cancellation if
                    // it was not cancelled yet.
                    if write_cancellation.token().is_cancellation_requested() {
                        return Ok(None);
                    } else {
                        write_cancellation.dispose_with(false);
                    }
                    let snapshot = snapshot.ok_or_else(|| Error::new("Assertion Failed: argument is undefined or null"))?;

                    // Write them to disk
                    let writing = if options.options.write_elevated.unwrap_or(false) && this.services.elevated_file_service.is_supported(&last_resolved_file_stat.resource) {
                        this.services.elevated_file_service.write_file_elevated(&last_resolved_file_stat.resource, FileContentSource::from(snapshot), Some(write_file_options))
                    } else {
                        this.services.file_service.write_file(&last_resolved_file_stat.resource, FileContentSource::from(snapshot), Some(write_file_options))
                    };
                    Ok(Some(writing.await?))
                }
                .await;

                match saved {
                    Ok(Some(stat)) => {
                        this.handle_save_success(stat, version_id, &options);
                        Ok(())
                    }
                    Ok(None) => Ok(()),
                    Err(error) => this.handle_save_error(error, version_id, &options),
                }
            });
            this.save_sequentializer.run(version_id, write, Some(Rc::new(move || cancel_write.cancel()))).await
        });

        self.save_sequentializer.run(version_id, task, Some(Rc::new(move || on_cancel.cancel())))
    }

    fn handle_save_success(&self, stat: FileStatWithMetadata, version_id: usize, options: &StoredFileWorkingCopySaveAsOptions) {
        // Updated resolved stat with updated stat
        self.update_last_resolved_file_stat(stat.clone());

        // Update dirty state unless working copy has changed meanwhile
        if version_id == self.version_id.get() {
            self.trace(&format!("handleSaveSuccess({version_id}) - setting dirty to false because versionId did not change"));
            self.set_dirty(false);
        } else {
            self.trace(&format!("handleSaveSuccess({version_id}) - not setting dirty to false because versionId did change meanwhile"));
        }

        // Update orphan state given save was successful
        self.base.set_orphaned(false);

        // Emit Save Event
        self.on_did_save.fire(StoredFileWorkingCopySaveEvent { event: WorkingCopySaveEvent { reason: options.reason(), source: options.source() }, stat });
    }

    fn handle_save_error(&self, error: Error, version_id: usize, options: &StoredFileWorkingCopySaveAsOptions) -> Result<(), Error> {
        let message = format!(
            "[stored file working copy] handleSaveError({version_id}) - exit - resulted in a save error: {error} {} {}",
            self.base.resource().to_string(false),
            self.type_id
        );
        if options.options.ignore_error_handler.unwrap_or(false) {
            self.services.log_service.trace(&message);
        } else {
            self.services.log_service.error(&message);
        }

        // Return early if the save() call was made asking to
        // handle the save error itself.
        if options.options.ignore_error_handler.unwrap_or(false) {
            return Err(error);
        }

        // In any case of an error, we mark the working copy as dirty to prevent data loss
        // It could be possible that the write corrupted the file on disk (e.g. when
        // an error happened after truncating the file) and as such we want to preserve
        // the working copy contents to prevent data loss.
        self.set_dirty(true);

        // Flag as error state
        self.in_error_mode.set(true);

        // Look out for a save conflict
        if Self::file_operation_result_of(&error) == Some(FileOperationResult::FileModifiedSince) {
            self.in_conflict_mode.set(true);
        }

        // Show save error to user for handling
        self.do_handle_save_error(&error, options);

        // Emit as event
        self.on_did_save_error.fire(());
        Ok(())
    }

    /// Uma ação do aviso de erro de salvar. O `run` recebe a working copy (a ação a segura por `Weak`).
    fn error_action(&self, id: &str, label: String, run: impl Fn(Rc<Self>) -> MaybePromise<Dynamic> + 'static) -> ActionRef {
        let this = self.this.clone();
        to_action(ToActionProps {
            id: id.to_string(),
            label,
            tooltip: None,
            enabled: None,
            checked: None,
            class: None,
            alt: None,
            run: Rc::new(move |_| match this.upgrade() {
                Some(this) => run(this),
                None => MaybePromise::Value(Dynamic::Undefined),
            }),
        })
    }

    /// Uma ação que salva de novo com as opções dadas (`run: () => this.save({ ... })`).
    fn save_action(&self, id: &str, label: String, options: StoredFileWorkingCopySaveAsOptions) -> ActionRef {
        self.error_action(id, label, move |this| MaybePromise::Promise(this.save_stored(Some(options.clone())).then(Dynamic::Bool)))
    }

    /// A ação `fileWorkingCopy.revert`.
    fn revert_action(&self) -> ActionRef {
        self.error_action("fileWorkingCopy.revert", localize("revert", "Revert", &[]), |this| MaybePromise::Promise(this.revert(None).then(|_| Dynamic::Undefined)))
    }

    fn do_handle_save_error(&self, error: &Error, options: &StoredFileWorkingCopySaveAsOptions) {
        let result = Self::file_operation_result_of(error);
        let mut primary_actions: Vec<ActionRef> = Vec::new();

        let message: String;

        // Dirty write prevention
        if result == Some(FileOperationResult::FileModifiedSince) {
            message = localize("staleSaveError", "Failed to save '{0}': The content of the file is newer. Do you want to overwrite the file with your changes?", &[self.name.as_str()]);

            primary_actions.push(self.save_action(
                "fileWorkingCopy.overwrite",
                localize("overwrite", "Overwrite", &[]),
                options.explicit_with(|options| options.options.ignore_modified_since = Some(true)),
            ));
            primary_actions.push(self.revert_action());
        }
        // Any other save error
        else {
            let is_write_locked = result == Some(FileOperationResult::FileWriteLocked);
            let tried_to_unlock = is_write_locked && options.options.write_unlock.unwrap_or(false);
            let is_permission_denied = result == Some(FileOperationResult::FilePermissionDenied);
            let can_save_elevated = self.services.elevated_file_service.is_supported(self.base.resource());

            // Save Elevated
            if can_save_elevated && (is_permission_denied || tried_to_unlock) {
                let label = if tried_to_unlock {
                    if cfg!(windows) {
                        localize("overwriteElevated", "Overwrite as Admin...", &[])
                    } else {
                        localize("overwriteElevatedSudo", "Overwrite as Sudo...", &[])
                    }
                } else if cfg!(windows) {
                    localize("saveElevated", "Retry as Admin...", &[])
                } else {
                    localize("saveElevatedSudo", "Retry as Sudo...", &[])
                };
                let options = options.explicit_with(|options| {
                    options.options.write_elevated = Some(true);
                    options.options.write_unlock = Some(tried_to_unlock);
                });
                // O `run` do original não devolve a promessa do salvar: o executor da ação não a espera.
                primary_actions.push(self.error_action("fileWorkingCopy.saveElevated", label, move |this| {
                    drop(this.save_stored(Some(options.clone())));
                    MaybePromise::Value(Dynamic::Undefined)
                }));
            }
            // Unlock
            else if is_write_locked {
                primary_actions.push(self.save_action(
                    "fileWorkingCopy.unlock",
                    localize("overwrite", "Overwrite", &[]),
                    options.explicit_with(|options| options.options.write_unlock = Some(true)),
                ));
            }
            // Retry
            else {
                primary_actions.push(self.save_action("fileWorkingCopy.retry", localize("retry", "Retry", &[]), options.explicit_with(|_| ())));
            }

            // Save As
            let (error_to_show, options_to_show) = (error.clone(), options.clone());
            primary_actions.push(self.error_action("fileWorkingCopy.saveAs", localize("saveAs", "Save As...", &[]), move |this| {
                let (error, options) = (error_to_show.clone(), options_to_show.clone());
                MaybePromise::Promise(async_fn(async move {
                    if let Some(editor) = this.services.working_copy_editor_service.find_editor(&*this) {
                        let result = this
                            .services
                            .editor_service
                            .save(editor, Some(SaveEditorsOptions { base: SaveOptions { reason: Some(SaveReason::Explicit), ..SaveOptions::default() }, save_as: Some(true) }))
                            .await?;
                        if !result.success {
                            this.do_handle_save_error(&error, &options); // show error again given the operation failed
                        }
                    }
                    Ok(Dynamic::Undefined)
                }))
            }));

            // Revert
            primary_actions.push(self.revert_action());

            // Message
            if is_write_locked {
                message = if tried_to_unlock && can_save_elevated {
                    if cfg!(windows) {
                        localize("readonlySaveErrorAdmin", "Failed to save '{0}': File is read-only. Select 'Overwrite as Admin' to retry as administrator.", &[self.name.as_str()])
                    } else {
                        localize("readonlySaveErrorSudo", "Failed to save '{0}': File is read-only. Select 'Overwrite as Sudo' to retry as superuser.", &[self.name.as_str()])
                    }
                } else {
                    localize("readonlySaveError", "Failed to save '{0}': File is read-only. Select 'Overwrite' to attempt to make it writeable.", &[self.name.as_str()])
                };
            } else if can_save_elevated && is_permission_denied {
                message = if cfg!(windows) {
                    localize("permissionDeniedSaveError", "Failed to save '{0}': Insufficient permissions. Select 'Retry as Admin' to retry as administrator.", &[self.name.as_str()])
                } else {
                    localize("permissionDeniedSaveErrorSudo", "Failed to save '{0}': Insufficient permissions. Select 'Retry as Sudo' to retry as superuser.", &[self.name.as_str()])
                };
            } else {
                message = localize("genericSaveError", "Failed to save '{0}': {1}", &[self.name.as_str(), to_error_message_of(error, false).as_str()]);
            }
        }

        // Show to the user as notification
        let mut notification = Notification::new(Severity::Error, message);
        notification.id = Some(hash(&Dynamic::string(&self.base.resource().to_string(false).to_string_lossy())).to_string());
        notification.actions = Some(NotificationActions { primary: primary_actions, secondary: Vec::new() });
        let handle = self.services.notification_service.notify(notification);

        // Remove automatically when we get saved/reverted
        let closing = handle.clone();
        let listener = self.store.register(
            event::any(vec![self.on_did_save.event().map(|_| (), None), self.on_did_revert.event()]).once().listen(move |_| closing.close()),
        );
        self.store.register(handle.on_did_close().once().listen(move |_| listener.dispose()));
    }

    fn update_last_resolved_file_stat(&self, new_file_stat: FileStatWithMetadata) {
        let old_readonly = self.is_readonly_flag();

        {
            let mut last = self.last_resolved_file_stat.borrow_mut();
            match last.as_ref() {
                // First resolve - just take
                None => *last = Some(new_file_stat),

                // Subsequent resolve - make sure that we only assign it if the mtime
                // is equal or has advanced.
                // This prevents race conditions from resolving and saving. If a save
                // comes in late after a revert was called, the mtime could be out of
                // sync.
                Some(current) if current.mtime() <= new_file_stat.mtime() => *last = Some(new_file_stat),

                // In all other cases update only the readonly and locked flags
                Some(current) => {
                    let mut stat = current.clone().into_stat();
                    stat.readonly = Some(new_file_stat.readonly());
                    stat.locked = Some(new_file_stat.locked());
                    *last = Some(FileStatWithMetadata::new(stat));
                }
            }
        }

        // Signal that the readonly state changed
        if self.is_readonly_flag() != old_readonly {
            self.on_did_change_readonly.fire(());
        }
    }

    //#endregion

    //#region Utilities

    fn trace(&self, msg: &str) {
        self.services.log_service.trace(&format!("[stored file working copy] {msg} {} {}", self.base.resource().to_string(false), self.type_id));
    }

    //#endregion
}

impl<M: IStoredFileWorkingCopyModel + 'static> IDisposable for StoredFileWorkingCopy<M> {
    //#region Dispose

    fn dispose(&self) {
        self.trace("dispose()");

        // State
        self.in_conflict_mode.set(false);
        self.in_error_mode.set(false);

        // Free up model for GC
        *self.model.borrow_mut() = None;

        // super.dispose(): o descarte da base dispara `onWillDispose`, e o que a classe registrou (os eventos, as escutas e
        // o modelo) é descartado junto.
        self.base.dispose();
        self.store.dispose();
    }

    //#endregion
}

impl<M: IStoredFileWorkingCopyModel + 'static> IResourceWorkingCopy for StoredFileWorkingCopy<M> {
    fn resource_working_copy(&self) -> &ResourceWorkingCopy {
        &self.base
    }
}

impl<M: IStoredFileWorkingCopyModel + 'static> IWorkingCopy for StoredFileWorkingCopy<M> {
    fn working_copy_type_id(&self) -> String {
        self.type_id.clone()
    }

    fn resource(&self) -> URI {
        self.base.resource().clone()
    }

    fn name(&self) -> String {
        self.name.clone()
    }

    fn capabilities(&self) -> WorkingCopyCapabilities {
        WorkingCopyCapabilities::NONE
    }

    fn on_did_change_dirty(&self) -> Event<()> {
        self.on_did_change_dirty.event()
    }

    fn on_did_change_content(&self) -> Event<()> {
        self.on_did_change_content.event()
    }

    fn on_did_save(&self) -> Event<WorkingCopySaveEvent> {
        self.on_did_save.event().map(|event| event.event.clone(), None)
    }

    fn stored_file_save_event(&self) -> Option<Event<StoredFileWorkingCopySaveEvent>> {
        Some(self.on_did_save.event())
    }

    fn is_dirty(&self) -> bool {
        self.dirty.get()
    }

    /// `isModified()` da `ResourceWorkingCopy`: `isDirty()`.
    fn is_modified(&self) -> bool {
        self.dirty.get()
    }

    //#region Backup

    fn backup_delay(&self) -> Option<u64> {
        self.model().and_then(|model| model.configuration()).and_then(|configuration| configuration.backup_delay)
    }

    fn backup(&self, token: &CancellationToken) -> Promise<WorkingCopyBackup> {
        // Fill in metadata if we are resolved
        let meta = self.last_resolved_file_stat.borrow().as_ref().map(|stat| {
            StoredFileWorkingCopyBackupMetaData { mtime: stat.mtime(), ctime: stat.ctime(), size: stat.size(), etag: stat.etag().to_string(), orphaned: self.is_orphaned() }.to_dynamic()
        });

        // Fill in content if we are resolved
        let snapshot = self.model().map(|model| race_cancellation(&model.snapshot(SnapshotContext::Backup, token.clone()), token));

        async_fn(async move {
            let content = match snapshot {
                Some(snapshot) => snapshot.await?.map(WorkingCopyBackupContent::Stream),
                None => None,
            };

            Ok(WorkingCopyBackup { meta, content })
        })
    }

    //#endregion

    fn save(&self, options: Option<SaveOptions>) -> Promise<bool> {
        self.save_stored(Some(StoredFileWorkingCopySaveAsOptions::from_save_options(options.unwrap_or_default())))
    }

    //#region Revert

    fn revert(&self, options: Option<RevertOptions>) -> Promise<()> {
        let force = options.and_then(|options| options.force).unwrap_or(false);
        if !self.is_resolved() || (!self.dirty.get() && !force) {
            return Promise::resolved(()); // ignore if not resolved or not dirty and not enforced
        }

        self.trace("revert()");

        // Unset flags
        let was_dirty = self.dirty.get();
        let undo_set_dirty = self.do_set_dirty(false);

        // Force read from disk unless reverting soft
        let soft_undo = options.and_then(|options| options.soft).unwrap_or(false);

        let this = self.rc();
        async_fn(async move {
            if !soft_undo {
                if let Err(error) = this.force_resolve_from_file().await {
                    // FileNotFound means the file got deleted meanwhile, so ignore it
                    if Self::file_operation_result_of(&error) != Some(FileOperationResult::FileNotFound) {
                        // Set flags back to previous values, we are still dirty if revert failed
                        undo_set_dirty();

                        return Err(error);
                    }
                }
            }

            // Emit file change event
            this.on_did_revert.fire(());

            // Emit dirty change event
            if was_dirty {
                this.on_did_change_dirty.fire(());
            }
            Ok(())
        })
    }

    //#endregion
}

impl<M: IStoredFileWorkingCopyModel + 'static> IFileWorkingCopy for StoredFileWorkingCopy<M> {
    fn on_did_revert(&self) -> Event<()> {
        self.on_did_revert.event()
    }

    fn model(&self) -> Option<Rc<dyn IFileWorkingCopyModel>> {
        self.model.borrow().clone().map(|model| model as Rc<dyn IFileWorkingCopyModel>)
    }

    fn resolve(&self) -> Promise<()> {
        self.resolve_stored(None)
    }

    fn is_resolved(&self) -> bool {
        self.model.borrow().is_some()
    }
}

impl<M: IStoredFileWorkingCopyModel + 'static> IStoredFileWorkingCopy for StoredFileWorkingCopy<M> {
    fn on_did_resolve(&self) -> Event<()> {
        self.on_did_resolve.event()
    }

    fn on_did_save_stored(&self) -> Event<StoredFileWorkingCopySaveEvent> {
        self.on_did_save.event()
    }

    fn on_did_save_error(&self) -> Event<()> {
        self.on_did_save_error.event()
    }

    fn on_did_change_readonly(&self) -> Event<()> {
        self.on_did_change_readonly.event()
    }

    fn stored_model(&self) -> Option<Rc<dyn IStoredFileWorkingCopyModel>> {
        self.model.borrow().clone().map(|model| model as Rc<dyn IStoredFileWorkingCopyModel>)
    }

    fn resolve_stored(&self, options: Option<StoredFileWorkingCopyResolveOptions>) -> Promise<()> {
        self.trace("resolve() - enter");

        // Return early if we are disposed
        if self.is_disposed() {
            self.trace("resolve() - exit - without resolving because file working copy is disposed");

            return Promise::resolved(());
        }

        // Unless there are explicit contents provided, it is important that we do not
        // resolve a working copy that is dirty or is in the process of saving to prevent
        // data loss.
        let has_contents = options.as_ref().is_some_and(|options| options.contents.is_some());
        if !has_contents && (self.dirty.get() || self.save_sequentializer.is_running()) {
            self.trace("resolve() - exit - without resolving because file working copy is dirty or being saved");

            return Promise::resolved(());
        }

        self.do_resolve(options)
    }

    fn mark_modified(&self) {
        self.set_dirty(true); // stored file working copy tracks modified via dirty
    }

    fn has_state(&self, state: StoredFileWorkingCopyState) -> bool {
        match state {
            StoredFileWorkingCopyState::Conflict => self.in_conflict_mode.get(),
            StoredFileWorkingCopyState::Dirty => self.dirty.get(),
            StoredFileWorkingCopyState::Error => self.in_error_mode.get(),
            StoredFileWorkingCopyState::Orphan => self.is_orphaned(),
            StoredFileWorkingCopyState::PendingSave => self.save_sequentializer.is_running(),
            StoredFileWorkingCopyState::Saved => !self.dirty.get(),
        }
    }

    fn join_state(&self, _state: StoredFileWorkingCopyState) -> Promise<()> {
        self.save_sequentializer.running().unwrap_or_else(|| Promise::resolved(()))
    }

    fn is_readonly(&self) -> Readonly {
        self.services.files_configuration_service.is_readonly(self.base.resource(), self.last_resolved_file_stat.borrow().as_ref().map(|stat| &**stat))
    }

    fn save_stored(&self, options: Option<StoredFileWorkingCopySaveAsOptions>) -> Promise<bool> {
        let options = options.unwrap_or_default();

        if !self.is_resolved() {
            return Promise::resolved(false);
        }

        if self.is_readonly_flag() {
            self.trace("save() - ignoring request for readonly resource");

            return Promise::resolved(false); // if working copy is readonly we do not attempt to save at all
        }

        if (self.has_state(StoredFileWorkingCopyState::Conflict) || self.has_state(StoredFileWorkingCopyState::Error))
            && matches!(options.reason(), Some(SaveReason::Auto) | Some(SaveReason::FocusChange) | Some(SaveReason::WindowChange))
        {
            self.trace("save() - ignoring auto save request for file working copy that is in conflict or error");

            return Promise::resolved(false); // if working copy is in save conflict or error, do not save unless save reason is explicit
        }

        // Actually do save
        self.trace("save() - enter");
        let this = self.rc();
        let saving = self.do_save(options);
        async_fn(async move {
            saving.await?;
            this.trace("save() - exit");

            Ok(this.has_state(StoredFileWorkingCopyState::Saved))
        })
    }
}
