//! Porte de `src/vs/workbench/services/textfile/common/textFileEditorModel.ts`: o `TextFileEditorModel`, que escuta as
//! mudanças do modelo de texto do editor por baixo e as salva de volta no disco pelo serviço de arquivos. Resolve (do
//! disco, de um backup ou de um conteúdo dado), salva (com participantes, uma escrita por vez), reverte, rastreia sujeira
//! (dirty), arquivo órfão, erro e conflito por `etag`, e o encoding.
//!
//! Divergências:
//! - **Injeção.** Os 16 parâmetros decorados do construtor são o [`TextFileEditorModelServices`] (o `createInstance` do
//!   gerenciador monta um por modelo); `resource`, `preferredEncoding` e `preferredLanguageId` são os argumentos.
//! - **Herança.** `TextFileEditorModel extends BaseTextEditorModel` é a composição com [`BaseTextEditorModel`] (em `base`);
//!   o `this.dispose()` virtual da base passa pelo `owner` dela. `implements ITextFileEditorModel` é o
//!   [`ITextFileEditorModel`] (com o `IWorkingCopy` de supertrait); onde o nome colide com o `IWorkingCopy`, ver
//!   `textfiles.rs`.
//! - **Estado.** Os campos mutáveis são `Cell`/`RefCell` (o modelo vive em `Rc`); `lastResolvedFileStat` é lido por
//!   [`TextFileEditorModel::last_resolved_file_stat`] (no original é público porque os testes o leem).
//! - **Estados de erro.** `error.fileOperationResult` é [`FileOperationError::from_error`]; `NotModifiedSinceFileOperationError`
//!   é o mesmo erro com `stat`.
//! - **`mark('code/...')`** é [`mark`].
//! - **`Date.now()`** é [`now_millis`].
//! - **`basename(this.labelService.getUriLabel(resource))`** recorta o último segmento do rótulo (separadores `/`, e `\`
//!   no Windows).
//! - **`TextFileEditorModel.TEXTFILE_SAVE_ENCODING_SOURCE`** é registrado no `SaveSourceRegistry` na hora do uso (o
//!   registro é idempotente).
//! - **`this.trace`** vira [`TextFileEditorModel::trace`], com o recurso como argumento do log.
//! - **`isCancellationError(err)`** é [`errors::is_cancellation_error`].

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};
use std::time::{SystemTime, UNIX_EPOCH};

use je_base::async_::{TaskSequentializer, async_fn, timeout};
use je_base::cancellation::{CancellationToken, CancellationTokenSource};
use je_base::dynamic::Dynamic;
use je_base::errors::{self, Error};
use je_base::event::{Emitter, Event};
use je_base::lifecycle::IDisposable;
use je_base::nls::localize;
use je_base::performance::mark;
use je_base::promise::Promise;
use je_base::uri::URI;
use je_editor::language_service::ILanguageService;
use je_editor::model::TextSnapshot;
use je_editor::model_service::ModelService;
use je_editor::modes_registry::PLAINTEXT_LANGUAGE_ID;
use je_editor::text_model::TextModel;
use je_editor::text_model_edit_source::{EditSources, TextModelEditSource};
use je_platform::files::files::{
    ETAG_DISABLED, FileChangeType, FileChangesEvent, FileOperationError, FileOperationResult, FileStat, FileStatWithMetadata, WriteFileOptions,
};
use je_platform::files::i_file_service::IFileService;
use je_platform::label::label::{ILabelService, UriLabelOptions};
use je_platform::log::LogService;
use je_platform::resources::ext_uri;

use crate::common::editor::editor_model::IEditorModel;
use crate::common::editor::text_editor_model::{BaseTextEditorModel, FirstLineSource, LanguageChangeSource};
use crate::common::editor::{RevertOptions, SaveOptions, SaveReason, with_save_source_registry};
use crate::services::extensions::extensions::IExtensionService;
use crate::services::files_configuration::files_configuration_service::{IFilesConfigurationService, Readonly};
use crate::services::path::path_service::IPathService;
use crate::services::language_detection::language_detection_worker_service::ILanguageDetectionService;
use je_platform::accessibility::IAccessibilityService;
use crate::services::textfile::collaborators::{
    IProgress, IProgressService, IWorkingCopyBackupService, ProgressCancellable, ProgressLocation,
    ProgressOptions, ProgressStep, ResolvedWorkingCopyBackup,
};
use crate::services::textfile::encoding::{UTF16BE, UTF16LE, UTF8, UTF8_WITH_BOM};
use crate::services::textfile::textfiles::{
    BaseTextFileContent, EncodingMode, ILanguageSupport, ITextFileEditorModel, ITextFileService, ReadTextFileEncodingOptions, ReadTextFileOptions, TextBufferFactoryRef,
    TextFileEditorModelState, TextFileResolveOptions, TextFileResolveReason, TextFileSaveAsOptions, TextFileSaveParticipantContext, TextFileStreamContent,
    TextOrSnapshot, WriteTextFileOptions,
};
use crate::services::working_copy::working_copy::{IWorkingCopy, NO_TYPE_ID, WorkingCopyBackup, WorkingCopyBackupContent, WorkingCopyCapabilities, WorkingCopySaveEvent};
use crate::services::working_copy::stored_file_working_copy::StoredFileWorkingCopySaveEvent;
use crate::services::working_copy::working_copy_service::IWorkingCopyService;

/// `TextFileEditorModel.UNDO_REDO_SAVE_PARTICIPANTS_AUTO_SAVE_THROTTLE_THRESHOLD`.
const UNDO_REDO_SAVE_PARTICIPANTS_AUTO_SAVE_THROTTLE_THRESHOLD: u64 = 500;

/// `Date.now()`.
fn now_millis() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |elapsed| elapsed.as_millis() as i64)
}

/// Os serviços que o `TextFileEditorModel` recebe por injeção, na ordem do construtor do original.
#[derive(Clone)]
pub struct TextFileEditorModelServices {
    pub language_service: Rc<dyn ILanguageService>,
    pub model_service: Rc<ModelService>,
    pub file_service: Rc<dyn IFileService>,
    pub text_file_service: Rc<dyn ITextFileService>,
    pub working_copy_backup_service: Rc<dyn IWorkingCopyBackupService>,
    pub log_service: Rc<dyn LogService>,
    pub working_copy_service: Rc<dyn IWorkingCopyService>,
    pub files_configuration_service: Rc<dyn IFilesConfigurationService>,
    pub label_service: Rc<dyn ILabelService>,
    pub language_detection_service: Rc<dyn ILanguageDetectionService>,
    pub accessibility_service: Rc<dyn IAccessibilityService>,
    pub path_service: Rc<dyn IPathService>,
    pub extension_service: Rc<dyn IExtensionService>,
    pub progress_service: Rc<dyn IProgressService>,
}

/// `IBackupMetaData`: o `meta` do backup (`mtime`, `ctime`, `size`, `etag`, `orphaned`).
struct BackupMetaData {
    mtime: i64,
    ctime: i64,
    size: u64,
    etag: String,
    orphaned: bool,
}

impl BackupMetaData {
    fn to_dynamic(&self) -> Dynamic {
        Dynamic::object([
            ("mtime", Dynamic::Number(self.mtime as f64)),
            ("ctime", Dynamic::Number(self.ctime as f64)),
            ("size", Dynamic::Number(self.size as f64)),
            ("etag", Dynamic::string(&self.etag)),
            ("orphaned", Dynamic::Bool(self.orphaned)),
        ])
    }

    fn from_dynamic(meta: &Dynamic) -> Self {
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

/// `TextFileEditorModel`.
pub struct TextFileEditorModel {
    this: Weak<TextFileEditorModel>,
    base: Rc<BaseTextEditorModel>,
    services: TextFileEditorModelServices,

    //#region Events
    on_did_change_content: Rc<Emitter<()>>,
    on_did_resolve: Rc<Emitter<TextFileResolveReason>>,
    on_did_change_dirty: Rc<Emitter<()>>,
    on_did_save_error: Rc<Emitter<()>>,
    on_did_save: Rc<Emitter<StoredFileWorkingCopySaveEvent>>,
    on_did_revert: Rc<Emitter<()>>,
    on_did_change_encoding: Rc<Emitter<()>>,
    on_did_change_orphaned: Rc<Emitter<()>>,
    on_did_change_readonly: Rc<Emitter<()>>,
    //#endregion

    resource: URI,
    name: String,
    resource_has_extension: bool,

    /// `preferredEncoding`: encoding as chosen by the user.
    preferred_encoding: RefCell<Option<String>>,
    /// `preferredLanguageId`: language id as chosen by the user.
    preferred_language_id: RefCell<Option<String>>,
    /// `contentEncoding`: encoding as reported from disk.
    content_encoding: RefCell<Option<String>>,

    version_id: Cell<usize>,
    buffer_saved_version_id: Cell<Option<i64>>,

    ignore_dirty_on_model_content_change: Cell<bool>,
    ignore_save_from_save_participants: Cell<bool>,

    last_model_content_change_from_undo_redo: Cell<Option<i64>>,

    last_resolved_file_stat: RefCell<Option<FileStatWithMetadata>>,

    save_sequentializer: TaskSequentializer,

    dirty: Cell<bool>,
    in_conflict_mode: Cell<bool>,
    in_orphan_mode: Cell<bool>,
    in_error_mode: Cell<bool>,

    has_encoding_set_explicitly: Cell<bool>,
}

impl TextFileEditorModel {
    /// `constructor(resource, preferredEncoding, preferredLanguageId, ...services)`.
    pub fn new(resource: URI, preferred_encoding: Option<String>, preferred_language_id: Option<String>, services: TextFileEditorModelServices) -> Rc<Self> {
        let label = services.label_service.get_uri_label(&resource, &UriLabelOptions::default());
        let name = Self::basename_of(&label);
        let resource_has_extension = !ext_uri().extname(&resource).is_empty();

        let this = Rc::new_cyclic(|this: &Weak<TextFileEditorModel>| {
            let owner: Weak<dyn IDisposable> = this.clone();
            let base = BaseTextEditorModel::new(
                owner,
                services.model_service.clone(),
                services.language_service.clone(),
                services.language_detection_service.clone(),
                services.accessibility_service.clone(),
                None,
            );

            let on_did_change_content = Emitter::new();
            let on_did_resolve = Emitter::new();
            let on_did_change_dirty = Emitter::new();
            let on_did_save_error = Emitter::new();
            let on_did_save = Emitter::new();
            let on_did_revert = Emitter::new();
            let on_did_change_encoding = Emitter::new();
            let on_did_change_orphaned = Emitter::new();
            let on_did_change_readonly = Emitter::new();
            let store = base.store();
            store.add(on_did_change_content.clone());
            store.add(on_did_resolve.clone());
            store.add(on_did_change_dirty.clone());
            store.add(on_did_save_error.clone());
            store.add(on_did_save.clone());
            store.add(on_did_revert.clone());
            store.add(on_did_change_encoding.clone());
            store.add(on_did_change_orphaned.clone());
            store.add(on_did_change_readonly.clone());

            Self {
                this: this.clone(),
                base,
                services: services.clone(),
                on_did_change_content,
                on_did_resolve,
                on_did_change_dirty,
                on_did_save_error,
                on_did_save,
                on_did_revert,
                on_did_change_encoding,
                on_did_change_orphaned,
                on_did_change_readonly,
                resource: resource.clone(),
                name,
                resource_has_extension,
                preferred_encoding: RefCell::new(preferred_encoding),
                preferred_language_id: RefCell::new(preferred_language_id),
                content_encoding: RefCell::new(None),
                version_id: Cell::new(0),
                buffer_saved_version_id: Cell::new(None),
                ignore_dirty_on_model_content_change: Cell::new(false),
                ignore_save_from_save_participants: Cell::new(false),
                last_model_content_change_from_undo_redo: Cell::new(None),
                last_resolved_file_stat: RefCell::new(None),
                save_sequentializer: TaskSequentializer::new(),
                dirty: Cell::new(false),
                in_conflict_mode: Cell::new(false),
                in_orphan_mode: Cell::new(false),
                in_error_mode: Cell::new(false),
                has_encoding_set_explicitly: Cell::new(false),
            }
        });

        // Make known to working copy service
        let working_copy: Rc<dyn IWorkingCopy> = this.clone();
        this.base.store().add(this.services.working_copy_service.register_working_copy(working_copy));

        this.register_listeners();

        this
    }

    fn rc(&self) -> Rc<Self> {
        self.this.upgrade().expect("o modelo está vivo enquanto é usado")
    }

    fn basename_of(label: &str) -> String {
        let is_separator = |c: char| c == '/' || (cfg!(windows) && c == '\\');
        label.trim_end_matches(is_separator).rsplit(is_separator).next().unwrap_or_default().to_string()
    }

    fn register_listeners(&self) {
        let store = self.base.store();

        let this = self.this.clone();
        store.add(self.services.file_service.on_did_files_change().listen(move |e| {
            if let Some(this) = this.upgrade() {
                drop(this.on_did_files_change(e.clone()));
            }
        }));

        let this = self.this.clone();
        store.add(self.services.files_configuration_service.on_did_change_files_association().listen(move |_| {
            if let Some(this) = this.upgrade() {
                this.on_did_change_files_association();
            }
        }));

        let on_did_change_readonly = self.on_did_change_readonly.clone();
        store.add(self.services.files_configuration_service.on_did_change_readonly().listen(move |_| on_did_change_readonly.fire(())));
    }

    fn on_did_files_change(&self, e: Rc<FileChangesEvent>) -> Promise<()> {
        let this = self.rc();
        async_fn(async move {
            let mut file_event_impacts_model = false;
            let mut new_in_orphan_mode_guess: Option<bool> = None;

            // If we are currently orphaned, we check if the model file was added back
            if this.in_orphan_mode.get() {
                let model_file_added = e.contains(&this.resource, &[FileChangeType::Added]);
                if model_file_added {
                    new_in_orphan_mode_guess = Some(false);
                    file_event_impacts_model = true;
                }
            }
            // Otherwise we check if the model file was deleted
            else {
                let model_file_deleted = e.contains(&this.resource, &[FileChangeType::Deleted]);
                if model_file_deleted {
                    new_in_orphan_mode_guess = Some(true);
                    file_event_impacts_model = true;
                }
            }

            if file_event_impacts_model && Some(this.in_orphan_mode.get()) != new_in_orphan_mode_guess {
                let mut new_in_orphan_mode_validated = false;
                if new_in_orphan_mode_guess == Some(true) {
                    // We have received reports of users seeing delete events even though the file still
                    // exists (network shares issue: https://github.com/microsoft/vscode/issues/13665).
                    // Since we do not want to mark the model as orphaned, we have to check if the
                    // file is really gone and not just a faulty file event.
                    timeout(100).await?;

                    if this.is_disposed() {
                        new_in_orphan_mode_validated = true;
                    } else {
                        let exists = this.services.file_service.exists(&this.resource).await?;
                        new_in_orphan_mode_validated = !exists;
                    }
                }

                if this.in_orphan_mode.get() != new_in_orphan_mode_validated && !this.is_disposed() {
                    this.set_orphaned(new_in_orphan_mode_validated);
                }
            }
            Ok(())
        })
    }

    fn set_orphaned(&self, orphaned: bool) {
        if self.in_orphan_mode.get() != orphaned {
            self.in_orphan_mode.set(orphaned);
            self.on_did_change_orphaned.fire(());
        }
    }

    fn on_did_change_files_association(&self) {
        let Some(text_editor_model) = self.base.text_editor_model().filter(|_| self.is_resolved()) else {
            return;
        };

        let first_line_text = BaseTextEditorModel::get_first_line_text(FirstLineSource::Model(&text_editor_model));
        let language_selection = BaseTextEditorModel::get_or_create_language(
            Some(&self.resource),
            self.services.language_service.as_ref(),
            self.preferred_language_id.borrow().as_deref(),
            Some(&first_line_text),
        );

        text_editor_model.set_language(&language_selection.language_id(), None);
    }

    //#region Events

    pub fn on_did_resolve(&self) -> Event<TextFileResolveReason> {
        self.on_did_resolve.event()
    }

    pub fn on_did_revert(&self) -> Event<()> {
        self.on_did_revert.event()
    }

    //#endregion

    /// `lastResolvedFileStat` (público no original porque os testes o leem).
    pub fn last_resolved_file_stat(&self) -> Option<FileStatWithMetadata> {
        self.last_resolved_file_stat.borrow().clone()
    }

    /// `languageChangeSource`.
    pub fn language_change_source(&self) -> Option<LanguageChangeSource> {
        self.base.language_change_source()
    }

    /// `updateTextEditorModel(newValue?, preferredLanguageId?, reason?)`.
    pub fn update_text_editor_model(&self, new_value: Option<&TextBufferFactoryRef>, preferred_language_id: Option<&str>, reason: Option<TextModelEditSource>) {
        self.base.update_text_editor_model(new_value.and_then(TextBufferFactoryRef::take), preferred_language_id, reason);
    }

    /// `isReadonly()`.
    pub fn is_readonly(&self) -> Readonly {
        self.services.files_configuration_service.is_readonly(&self.resource, self.last_resolved_file_stat.borrow().as_ref().map(|stat| &**stat))
    }

    fn is_readonly_flag(&self) -> bool {
        self.is_readonly().is_readonly()
    }

    //#region Resolve

    /// `resolve(options?)`.
    fn resolve_impl(&self, options: Option<TextFileResolveOptions>) -> Promise<()> {
        self.trace("resolve() - enter");
        mark("code/willResolveTextFileEditorModel", None);

        // Return early if we are disposed
        if self.is_disposed() {
            self.trace("resolve() - exit - without resolving because model is disposed");

            return Promise::resolved(());
        }

        // Unless there are explicit contents provided, it is important that we do not
        // resolve a model that is dirty or is in the process of saving to prevent data
        // loss.
        let has_contents = options.as_ref().is_some_and(|options| options.contents.is_some());
        if !has_contents && (self.dirty.get() || self.save_sequentializer.is_running()) {
            self.trace("resolve() - exit - without resolving because model is dirty or being saved");

            return Promise::resolved(());
        }

        // Resolve either from backup or from file
        let this = self.rc();
        async_fn(async move {
            this.do_resolve(options).await?;

            mark("code/didResolveTextFileEditorModel", None);
            Ok(())
        })
    }

    fn do_resolve(&self, options: Option<TextFileResolveOptions>) -> Promise<()> {
        // First check if we have contents to use for the model
        if let Some(contents) = options.as_ref().and_then(|options| options.contents.clone()) {
            return self.resolve_from_buffer(contents, options);
        }

        // Second, check if we have a backup to resolve from (only for new models)
        let this = self.rc();
        async_fn(async move {
            let is_new_model = !this.is_resolved();
            if is_new_model {
                let resolved_from_backup = this.resolve_from_backup(options.clone()).await?;
                if resolved_from_backup {
                    return Ok(());
                }
            }

            // Finally, resolve from file resource
            this.resolve_from_file(options).await
        })
    }

    fn resolve_from_buffer(&self, buffer: TextBufferFactoryRef, options: Option<TextFileResolveOptions>) -> Promise<()> {
        self.trace("resolveFromBuffer()");

        let this = self.rc();
        async_fn(async move {
            // Try to resolve metdata from disk
            let (mtime, ctime, size, etag);
            match this.services.file_service.stat(&this.resource).await {
                Ok(metadata) => {
                    mtime = metadata.mtime();
                    ctime = metadata.ctime();
                    size = metadata.size();
                    etag = metadata.etag().to_string();

                    // Clear orphaned state when resolving was successful
                    this.set_orphaned(false);
                }
                Err(error) => {
                    // Put some fallback values in error case
                    mtime = now_millis();
                    ctime = now_millis();
                    size = 0;
                    etag = ETAG_DISABLED.to_string();

                    // Apply orphaned state based on error code
                    this.set_orphaned(Self::file_operation_result_of(&error) == Some(FileOperationResult::FileNotFound));
                }
            }

            let preferred_encoding = this
                .services
                .text_file_service
                .encoding()
                .get_preferred_write_encoding(&this.resource, this.preferred_encoding.borrow().as_deref())
                .await?;

            // Resolve with buffer
            this.resolve_from_content(
                TextFileStreamContent {
                    base: BaseTextFileContent {
                        resource: this.resource.clone(),
                        name: this.name.clone(),
                        mtime,
                        ctime,
                        size,
                        etag,
                        readonly: false,
                        locked: false,
                        executable: false,
                        encoding: preferred_encoding.encoding,
                    },
                    value: buffer,
                },
                true, /* dirty (resolved from buffer) */
                options,
            );
            Ok(())
        })
    }

    /// `error.fileOperationResult`.
    fn file_operation_result_of(error: &Error) -> Option<FileOperationResult> {
        FileOperationError::from_error(error).map(|error| error.file_operation_result)
    }

    fn resolve_from_backup(&self, options: Option<TextFileResolveOptions>) -> Promise<bool> {
        let this = self.rc();
        async_fn(async move {
            // Resolve backup if any
            let working_copy: Rc<dyn IWorkingCopy> = this.clone();
            let backup = this.services.working_copy_backup_service.resolve(working_copy.as_ref()).await?;

            // Resolve preferred encoding if we need it
            let mut encoding = UTF8.to_string();
            if backup.is_some() {
                encoding = this
                    .services
                    .text_file_service
                    .encoding()
                    .get_preferred_write_encoding(&this.resource, this.preferred_encoding.borrow().as_deref())
                    .await?
                    .encoding;
            }

            // Abort if someone else managed to resolve the model by now
            let is_new_model = !this.is_resolved();
            if !is_new_model {
                this.trace("resolveFromBackup() - exit - without resolving because previously new model got created meanwhile");

                return Ok(true); // imply that resolving has happened in another operation
            }

            // Try to resolve from backup if we have any
            if let Some(backup) = backup {
                this.do_resolve_from_backup(backup, encoding, options).await?;

                return Ok(true);
            }

            // Otherwise signal back that resolving did not happen
            Ok(false)
        })
    }

    fn do_resolve_from_backup(&self, backup: ResolvedWorkingCopyBackup, encoding: String, options: Option<TextFileResolveOptions>) -> Promise<()> {
        self.trace("doResolveFromBackup()");

        let this = self.rc();
        async_fn(async move {
            let meta = backup.meta.as_ref().map(BackupMetaData::from_dynamic);

            // Resolve with backup
            let decoded_stream = this
                .services
                .text_file_service
                .get_decoded_stream(Some(&this.resource), backup.value.clone(), Some(ReadTextFileEncodingOptions { encoding: Some(UTF8.to_string()), ..Default::default() }))
                .await?;
            let value = crate::services::textfile::text_buffer_factory::create_text_buffer_factory_from_stream(decoded_stream.as_ref()).await?;
            this.resolve_from_content(
                TextFileStreamContent {
                    base: BaseTextFileContent {
                        resource: this.resource.clone(),
                        name: this.name.clone(),
                        mtime: meta.as_ref().map_or_else(now_millis, |meta| meta.mtime),
                        ctime: meta.as_ref().map_or_else(now_millis, |meta| meta.ctime),
                        size: meta.as_ref().map_or(0, |meta| meta.size),
                        etag: meta.as_ref().map_or_else(|| ETAG_DISABLED.to_string(), |meta| meta.etag.clone()), // etag disabled if unknown!
                        readonly: false,
                        locked: false,
                        executable: false,
                        encoding,
                    },
                    value,
                },
                true, /* dirty (resolved from backup) */
                options,
            );

            // Restore orphaned flag based on state
            if meta.is_some_and(|meta| meta.orphaned) {
                this.set_orphaned(true);
            }
            Ok(())
        })
    }

    fn resolve_from_file(&self, options: Option<TextFileResolveOptions>) -> Promise<()> {
        self.trace("resolveFromFile()");

        let force_read_from_file = options.as_ref().and_then(|options| options.force_read_from_file).unwrap_or(false);
        let allow_binary = self.is_resolved() /* always allow if we resolved previously */ || options.as_ref().and_then(|options| options.allow_binary).unwrap_or(false);

        // Decide on etag
        let etag: Option<String> = if force_read_from_file {
            Some(ETAG_DISABLED.to_string()) // disable ETag if we enforce to read from disk
        } else {
            self.last_resolved_file_stat.borrow().as_ref().map(|stat| stat.etag().to_string()) // otherwise respect etag to support caching
        };

        // Remember current version before doing any long running operation
        // to ensure we are not changing a model that was changed meanwhile
        let current_version_id = self.version_id.get();

        // Resolve Content
        let this = self.rc();
        async_fn(async move {
            let read_options = ReadTextFileOptions {
                encoding: ReadTextFileEncodingOptions {
                    accept_text_only: Some(!allow_binary),
                    encoding: this.preferred_encoding.borrow().clone(),
                    ..Default::default()
                },
                read: je_platform::files::files::ReadFileOptions { etag, ..Default::default() },
                limits: options.as_ref().and_then(|options| options.limits),
            };

            match this.services.text_file_service.read_stream(&this.resource, Some(read_options)).await {
                Ok(content) => {
                    // Clear orphaned state when resolving was successful
                    this.set_orphaned(false);

                    // Return early if the model content has changed
                    // meanwhile to prevent loosing any changes
                    if current_version_id != this.version_id.get() {
                        this.trace("resolveFromFile() - exit - without resolving because model content changed");

                        return Ok(());
                    }

                    this.resolve_from_content(content, false /* not dirty (resolved from file) */, options);
                    Ok(())
                }
                Err(error) => {
                    let file_error = FileOperationError::from_error(&error);
                    let result = file_error.as_ref().map(|error| error.file_operation_result);

                    // Apply orphaned state based on error code
                    this.set_orphaned(result == Some(FileOperationResult::FileNotFound));

                    // NotModified status is expected and can be handled gracefully
                    // if we are resolved. We still want to update our last resolved
                    // stat to e.g. detect changes to the file's readonly state
                    if this.is_resolved() && result == Some(FileOperationResult::FileNotModifiedSince) {
                        if let Some(stat) = file_error.and_then(|error| error.stat) {
                            this.update_last_resolved_file_stat(stat);
                        }

                        return Ok(());
                    }

                    // Unless we are forced to read from the file, Ignore when a model has been resolved once
                    // and the file was deleted meanwhile. Since we already have the model resolved, we can return
                    // to this state and update the orphaned flag to indicate that this model has no version on
                    // disk anymore.
                    if this.is_resolved() && result == Some(FileOperationResult::FileNotFound) && !force_read_from_file {
                        return Ok(());
                    }

                    // Otherwise bubble up the error
                    Err(error)
                }
            }
        })
    }

    fn resolve_from_content(&self, content: TextFileStreamContent, dirty: bool, options: Option<TextFileResolveOptions>) {
        self.trace("resolveFromContent() - enter");

        // Return early if we are disposed
        if self.is_disposed() {
            self.trace("resolveFromContent() - exit - because model is disposed");

            return;
        }

        // Update our resolved disk stat model
        self.update_last_resolved_file_stat(FileStatWithMetadata::new(FileStat {
            resource: self.resource.clone(),
            name: content.base.name.clone(),
            size: Some(content.base.size),
            mtime: Some(content.base.mtime),
            ctime: Some(content.base.ctime),
            etag: Some(content.base.etag.clone()),
            readonly: Some(content.base.readonly),
            locked: Some(content.base.locked),
            executable: Some(false),
            is_file: true,
            is_directory: false,
            is_symbolic_link: false,
            children: None,
        }));

        // Keep the original encoding to not loose it when saving
        let old_encoding = self.content_encoding.borrow().clone();
        *self.content_encoding.borrow_mut() = Some(content.base.encoding.clone());

        // Handle events if encoding changed
        if self.preferred_encoding.borrow().is_some() {
            self.update_preferred_encoding(Some(&content.base.encoding)); // make sure to reflect the real encoding of the file (never out of sync)
        } else if old_encoding.as_deref() != Some(content.base.encoding.as_str()) {
            self.on_did_change_encoding.fire(());
        }

        // Update Existing Model
        if self.base.text_editor_model().is_some() {
            self.do_update_text_model(&content.value, EditSources::reload_from_disk());
        }
        // Create New Model
        else {
            self.do_create_text_model(&content.base.resource, &content.value);
        }

        // Update model dirty flag. This is very important to call
        // in both cases of dirty or not because it conditionally
        // updates the `bufferSavedVersionId` to determine the
        // version when to consider the model as saved again (e.g.
        // when undoing back to the saved state)
        self.set_dirty(dirty);

        // Emit as event
        self.on_did_resolve.fire(options.and_then(|options| options.reason).unwrap_or(TextFileResolveReason::Other));
    }

    fn do_create_text_model(&self, resource: &URI, value: &TextBufferFactoryRef) {
        self.trace("doCreateTextModel()");

        // Create model
        let factory = value.take().expect("a fábrica de buffer é consumida uma única vez");
        let text_model = self.base.create_text_editor_model(factory, Some(resource), self.preferred_language_id.borrow().as_deref());

        // Model Listeners
        self.install_model_listeners(&text_model);

        // Detect language from content
        drop(self.auto_detect_language());
    }

    fn do_update_text_model(&self, value: &TextBufferFactoryRef, reason: TextModelEditSource) {
        self.trace("doUpdateTextModel()");

        // Update model value in a block that ignores content change events for dirty tracking
        self.ignore_dirty_on_model_content_change.set(true);
        self.base.update_text_editor_model(value.take(), self.preferred_language_id.borrow().as_deref(), Some(reason));
        self.ignore_dirty_on_model_content_change.set(false);
    }

    fn install_model_listeners(&self, model: &TextModel) {
        // See https://github.com/microsoft/vscode/issues/30189
        // This code has been extracted to a different method because it caused a memory leak
        // where `value` was captured in the content change listener closure scope.
        let store = self.base.store();

        let (this, model_ref) = (self.this.clone(), self.base.text_editor_model());
        store.add(model.on_did_change_content().listen(move |e| {
            if let (Some(this), Some(model)) = (this.upgrade(), model_ref.clone()) {
                this.on_model_content_changed(&model, e.is_undoing || e.is_redoing);
            }
        }));

        let this = self.this.clone();
        store.add(model.on_did_change_language().listen(move |_| {
            // detect possible encoding change via language specific settings
            if let Some(this) = this.upgrade() {
                drop(this.on_maybe_should_change_encoding());
            }
        }));

        self.base.install_model_listeners(model);
    }

    fn on_model_content_changed(&self, model: &TextModel, is_undoing_or_redoing: bool) {
        self.trace("onModelContentChanged() - enter");

        // In any case increment the version id because it tracks the textual content state of the model at all times
        self.version_id.set(self.version_id.get() + 1);
        self.trace(&format!("onModelContentChanged() - new versionId {}", self.version_id.get()));

        // Remember when the user changed the model through a undo/redo operation.
        // We need this information to throttle save participants to fix
        // https://github.com/microsoft/vscode/issues/102542
        if is_undoing_or_redoing {
            self.last_model_content_change_from_undo_redo.set(Some(now_millis()));
        }

        // We mark check for a dirty-state change upon model content change, unless:
        // - explicitly instructed to ignore it (e.g. from model.resolve())
        // - the model is readonly (in that case we never assume the change was done by the user)
        if !self.ignore_dirty_on_model_content_change.get() && !self.is_readonly_flag() {
            // The contents changed as a matter of Undo and the version reached matches the saved one
            // In this case we clear the dirty flag and emit a SAVED event to indicate this state.
            if Some(model.get_alternative_version_id()) == self.buffer_saved_version_id.get() {
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

        // Detect language from content
        drop(self.auto_detect_language());
    }

    /// `autoDetectLanguage()` (override).
    fn auto_detect_language(&self) -> Promise<()> {
        let this = self.rc();
        async_fn(async move {
            // Wait to be ready to detect language
            this.services.extension_service.when_installed_extensions_registered().await?;

            // Only perform language detection conditionally
            let language_id = this.get_language_id();
            if this.resource.scheme().to_string_lossy() == this.services.path_service.default_uri_scheme() // make sure to not detect language for non-user visible documents
                && language_id.as_deref().is_none_or(|id| id.is_empty() || id == PLAINTEXT_LANGUAGE_ID) // only run on files with plaintext language set or no language set at all
                && !this.resource_has_extension
            // only run if this particular file doesn't have an extension
            {
                return this.base.auto_detect_language().await;
            }
            Ok(())
        })
    }

    fn force_resolve_from_file(&self) -> Promise<()> {
        if self.is_disposed() {
            return Promise::resolved(()); // return early when the model is invalid
        }

        // We go through the text file service to make
        // sure this kind of `resolve` is properly
        // running in sequence with any other running
        // `resolve` if any, including subsequent runs
        // that are triggered right after.
        let files = self.services.text_file_service.files();
        let resource = self.resource.clone();
        async_fn(async move {
            files
                .resolve(
                    &resource,
                    Some(crate::services::textfile::textfiles::TextFileEditorModelResolveOrCreateOptions {
                        reload: Some(crate::services::textfile::textfiles::TextFileReloadOptions { async_: false }),
                        resolve: TextFileResolveOptions { force_read_from_file: Some(true), ..Default::default() },
                        ..Default::default()
                    }),
                )
                .await?;
            Ok(())
        })
    }

    //#endregion

    //#region Dirty

    /// `setDirty(dirty)`.
    pub fn set_dirty(&self, dirty: bool) {
        if !self.is_resolved() {
            return; // only resolved models can be marked dirty
        }

        // Track dirty state and version id
        let was_dirty = self.dirty.get();
        self.do_set_dirty(dirty);

        // Emit as Event if dirty changed
        if dirty != was_dirty {
            self.on_did_change_dirty.fire(());
        }
    }

    /// `doSetDirty(dirty)`: devolve o fecho que desfaz a chamada.
    fn do_set_dirty(&self, dirty: bool) -> impl FnOnce() + use<> {
        let was_dirty = self.dirty.get();
        let was_in_conflict_mode = self.in_conflict_mode.get();
        let was_in_error_mode = self.in_error_mode.get();
        let old_buffer_saved_version_id = self.buffer_saved_version_id.get();

        if !dirty {
            self.dirty.set(false);
            self.in_conflict_mode.set(false);
            self.in_error_mode.set(false);
            self.update_saved_version_id();
        } else {
            self.dirty.set(true);
        }

        // Return function to revert this call
        let this = self.rc();
        move || {
            this.dirty.set(was_dirty);
            this.in_conflict_mode.set(was_in_conflict_mode);
            this.in_error_mode.set(was_in_error_mode);
            this.buffer_saved_version_id.set(old_buffer_saved_version_id);
        }
    }

    //#endregion

    //#region Save

    fn do_save(&self, mut options: TextFileSaveAsOptions) -> Promise<()> {
        if options.reason().is_none() {
            options.options.base.reason = Some(SaveReason::Explicit);
        }

        let version_id = self.version_id.get();
        self.trace(&format!("doSave({version_id}) - enter with versionId {version_id}"));

        // Return early if saved from within save participant to break recursion
        //
        // Scenario: a save participant triggers a save() on the model
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
        // Scenario: user invoked save action even though the model is not dirty
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
        // Scenario B: save is very slow (e.g. network share) and the user manages to change the buffer and trigger another save
        //             while the first save has not returned yet.
        //
        if self.save_sequentializer.is_running() {
            self.trace(&format!("doSave({version_id}) - exit - because busy saving"));

            // Indicate to the save sequentializer that we want to
            // cancel the running operation so that ours can run
            // before the running one finishes.
            // Currently this will try to cancel running save
            // participants but never a running save.
            self.save_sequentializer.cancel_running();

            // Queue this as the upcoming save and return
            let this = self.rc();
            return self.save_sequentializer.queue(move || this.do_save(options));
        }

        // Push all edit operations to the undo stack so that the user has a chance to
        // Ctrl+Z back to the saved version.
        if let Some(text_editor_model) = self.base.text_editor_model().filter(|_| self.is_resolved()) {
            text_editor_model.push_stack_element();
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
        options: TextFileSaveAsOptions,
        progress: Rc<dyn IProgress<ProgressStep>>,
        save_cancellation: CancellationTokenSource,
    ) -> Promise<()> {
        let this = self.rc();
        let on_cancel = save_cancellation.clone();
        let task = async_fn(async move {
            // A save participant can still change the model now and since we are so close to saving
            // we do not want to trigger another auto save or similar, so we block this
            // In addition we update our version right after in case it changed because of a model change
            //
            // Save participants can also be skipped through API.
            if this.is_resolved() && !options.skip_save_participants() {
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
                        if let Some(last_change) = this.last_model_content_change_from_undo_redo.get() {
                            let time_from_undo_redo_to_save = (now_millis() - last_change).max(0) as u64;
                            if time_from_undo_redo_to_save < UNDO_REDO_SAVE_PARTICIPANTS_AUTO_SAVE_THROTTLE_THRESHOLD {
                                timeout(UNDO_REDO_SAVE_PARTICIPANTS_AUTO_SAVE_THROTTLE_THRESHOLD - time_from_undo_redo_to_save).await?;
                            }
                        }
                    }

                    // Run save participants unless save was cancelled meanwhile
                    if !save_cancellation.token().is_cancellation_requested() {
                        this.ignore_save_from_save_participants.set(true);
                        let model: Rc<dyn ITextFileEditorModel> = this.clone();
                        let result = this
                            .services
                            .text_file_service
                            .files()
                            .run_save_participants(
                                model,
                                TextFileSaveParticipantContext { reason: options.reason().unwrap_or(SaveReason::Explicit), saved_from: options.from.clone() },
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
                    this.services.log_service.error(&format!("[text file model] runSaveParticipants({version_id}) - resulted in an error: {error} {}", this.resource.to_string(false)));
                }
            }

            // It is possible that a subsequent save is cancelling this
            // running save. As such we return early when we detect that
            // However, we do not pass the token into the file service
            // because that is an atomic operation currently without
            // cancellation support, so we dispose the cancellation if
            // it was not cancelled yet.
            if save_cancellation.token().is_cancellation_requested() {
                return Ok(());
            } else {
                save_cancellation.dispose_with(false);
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

            // We require a resolved model from this point on, since we are about to write data to disk.
            if !this.is_resolved() {
                return Ok(());
            }

            // update versionId with its new value (if pre-save changes happened)
            let version_id = this.version_id.get();

            // Clear error flag since we are trying to save again
            this.in_error_mode.set(false);

            // Save to Disk. We mark the save operation as currently running with
            // the latest versionId because it might have changed from a save
            // participant triggering
            progress.report(ProgressStep { message: Some(localize("saveTextFile", "Writing into file...", &[])), ..ProgressStep::default() });
            this.trace(&format!("doSave({version_id}) - before write()"));
            let last_resolved_file_stat = this.last_resolved_file_stat.borrow().clone().expect("o modelo resolvido tem o stat da última resolução");

            let write_this = this.clone();
            let write = async_fn(async move {
                let this = write_this;
                let Some(snapshot) = this.base.create_snapshot() else {
                    return Ok(());
                };
                let etag = if options.options.ignore_modified_since.unwrap_or(false)
                    || !this.services.files_configuration_service.prevent_save_conflicts(&last_resolved_file_stat.resource, this.get_language_id().as_deref())
                {
                    ETAG_DISABLED.to_string()
                } else {
                    last_resolved_file_stat.etag().to_string()
                };

                let write_options = WriteTextFileOptions {
                    write: WriteFileOptions {
                        mtime: Some(last_resolved_file_stat.mtime()),
                        etag: Some(etag),
                        unlock: options.options.write_unlock.unwrap_or(false),
                        ..WriteFileOptions::default()
                    },
                    encoding: this.get_encoding(),
                    write_elevated: options.options.write_elevated,
                };

                match this.services.text_file_service.write(&last_resolved_file_stat.resource, TextOrSnapshot::Snapshot(snapshot), Some(write_options)).await {
                    Ok(stat) => {
                        this.handle_save_success(stat, version_id, &options);
                        Ok(())
                    }
                    Err(error) => this.handle_save_error(error, version_id, &options),
                }
            });
            this.save_sequentializer.run(version_id, write, None).await
        });

        self.save_sequentializer.run(version_id, task, Some(Rc::new(move || on_cancel.cancel())))
    }

    fn handle_save_success(&self, stat: FileStatWithMetadata, version_id: usize, options: &TextFileSaveAsOptions) {
        // Updated resolved stat with updated stat
        self.update_last_resolved_file_stat(stat.clone());

        // Update dirty state unless model has changed meanwhile
        if version_id == self.version_id.get() {
            self.trace(&format!("handleSaveSuccess({version_id}) - setting dirty to false because versionId did not change"));
            self.set_dirty(false);
        } else {
            self.trace(&format!("handleSaveSuccess({version_id}) - not setting dirty to false because versionId did change meanwhile"));
        }

        // Update orphan state given save was successful
        self.set_orphaned(false);

        // Emit Save Event
        self.on_did_save.fire(StoredFileWorkingCopySaveEvent { event: WorkingCopySaveEvent { reason: options.reason(), source: options.source() }, stat });
    }

    fn handle_save_error(&self, error: Error, version_id: usize, options: &TextFileSaveAsOptions) -> Result<(), Error> {
        let message = format!("[text file model] handleSaveError({version_id}) - exit - resulted in a save error: {error} {}", self.resource.to_string(false));
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

        // In any case of an error, we mark the model as dirty to prevent data loss
        // It could be possible that the write corrupted the file on disk (e.g. when
        // an error happened after truncating the file) and as such we want to preserve
        // the model contents to prevent data loss.
        self.set_dirty(true);

        // Flag as error state in the model
        self.in_error_mode.set(true);

        // Look out for a save conflict
        if Self::file_operation_result_of(&error) == Some(FileOperationResult::FileModifiedSince) {
            self.in_conflict_mode.set(true);
        }

        // Show to user
        let model: Rc<dyn ITextFileEditorModel> = self.rc();
        self.services.text_file_service.files().save_error_handler().on_save_error(&error, model.as_ref(), options);

        // Emit as event
        self.on_did_save_error.fire(());
        Ok(())
    }

    fn update_saved_version_id(&self) {
        // we remember the models alternate version id to remember when the version
        // of the model matches with the saved version on disk. we need to keep this
        // in order to find out if the model changed back to a saved version (e.g.
        // when undoing long enough to reach to a version that is saved and then to
        // clear the dirty flag)
        if let Some(text_editor_model) = self.base.text_editor_model().filter(|_| self.is_resolved()) {
            self.buffer_saved_version_id.set(Some(text_editor_model.get_alternative_version_id()));
        }
    }

    fn update_last_resolved_file_stat(&self, new_file_stat: FileStatWithMetadata) {
        let old_readonly = self.is_readonly_flag();

        {
            let mut last = self.last_resolved_file_stat.borrow_mut();
            match last.as_ref() {
                // First resolve - just take
                None => *last = Some(new_file_stat),

                // Subsequent resolve - make sure that we only assign it if the mtime is equal or has advanced.
                // This prevents race conditions from resolving and saving. If a save comes in late after a revert
                // was called, the mtime could be out of sync.
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

    //#region Encoding

    fn on_maybe_should_change_encoding(&self) -> Promise<()> {
        // This is a bit of a hack but there is a narrow case where
        // per-language configured encodings are not working:
        //
        // On startup we may not yet have all languages resolved so
        // we pick a wrong encoding. We never used to re-apply the
        // encoding when the language was then resolved, because that
        // is an operation that is will have to fetch the contents
        // again from disk.
        //
        // To mitigate this issue, when we detect the model language
        // changes, we see if there is a specific encoding configured
        // for the new language and apply it, only if the model is
        // not dirty and only if the encoding was not explicitly set.
        //
        // (see https://github.com/microsoft/vscode/issues/127936)
        let this = self.rc();
        async_fn(async move {
            if this.has_encoding_set_explicitly.get() {
                this.trace("onMaybeShouldChangeEncoding() - ignoring because encoding was set explicitly");

                return Ok(()); // never change the user's choice of encoding
            }

            let content_encoding = this.content_encoding.borrow().clone();
            if matches!(content_encoding.as_deref(), Some(encoding) if encoding == UTF8_WITH_BOM || encoding == UTF16BE || encoding == UTF16LE) {
                this.trace("onMaybeShouldChangeEncoding() - ignoring because content encoding has a BOM");

                return Ok(()); // never change an encoding that we can detect 100% via BOMs
            }

            let encoding = this.services.text_file_service.encoding().get_preferred_read_encoding(&this.resource).await?.encoding;
            if !this.is_new_encoding(Some(&encoding)) {
                this.trace(&format!("onMaybeShouldChangeEncoding() - ignoring because preferred encoding {encoding} is not new"));

                return Ok(()); // return early if encoding is invalid or did not change
            }

            if this.dirty.get() {
                this.trace("onMaybeShouldChangeEncoding() - ignoring because model is dirty");

                return Ok(()); // return early to prevent accident saves in this case
            }

            this.services
                .log_service
                .info(&format!("Adjusting encoding based on configured language override to '{encoding}' for {}.", this.resource.to_string(true)));

            // Force resolve to pick up the new encoding
            this.force_resolve_from_file().await
        })
    }

    fn set_encoding_internal(&self, encoding: &str, mode: EncodingMode) -> Promise<()> {
        let this = self.rc();
        let encoding = encoding.to_string();
        async_fn(async move {
            // Encode: Save with encoding
            if mode == EncodingMode::Encode {
                this.update_preferred_encoding(Some(&encoding));

                // Save
                if !this.dirty.get() {
                    this.version_id.set(this.version_id.get() + 1); // needs to increment because we change the model potentially
                    this.set_dirty(true);
                }

                if !this.in_conflict_mode.get() {
                    let source = with_save_source_registry(|registry| registry.register_source("textFileEncoding.source", &localize("textFileCreate.source", "File Encoding Changed", &[])));
                    this.save_text_file(TextFileSaveAsOptions {
                        options: crate::services::textfile::textfiles::TextFileSaveOptions {
                            base: SaveOptions { source: Some(source), ..SaveOptions::default() },
                            ..Default::default()
                        },
                        ..Default::default()
                    })
                    .await?;
                }
            }
            // Decode: Resolve with encoding
            else {
                if !this.is_new_encoding(Some(&encoding)) {
                    return Ok(()); // return early if the encoding is already the same
                }

                if this.dirty.get() {
                    return Err(Error::new("Cannot re-open a dirty text document with different encoding. Save it first."));
                }

                this.update_preferred_encoding(Some(&encoding));

                this.force_resolve_from_file().await?;
            }
            Ok(())
        })
    }

    fn is_new_encoding(&self, encoding: Option<&str>) -> bool {
        let preferred_encoding = self.preferred_encoding.borrow();
        if preferred_encoding.as_deref() == encoding {
            return false; // return early if the encoding is already the same
        }

        if preferred_encoding.is_none() && self.content_encoding.borrow().as_deref() == encoding {
            return false; // also return if we don't have a preferred encoding but the content encoding is already the same
        }

        true
    }

    //#endregion

    fn trace(&self, msg: &str) {
        self.services.log_service.trace(&format!("[text file model] {msg} {}", self.resource.to_string(false)));
    }
}

impl IDisposable for TextFileEditorModel {
    fn dispose(&self) {
        self.trace("dispose()");

        self.in_conflict_mode.set(false);
        self.in_orphan_mode.set(false);
        self.in_error_mode.set(false);

        self.base.dispose();
    }
}

impl IEditorModel for TextFileEditorModel {
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn on_will_dispose(&self) -> Event<()> {
        self.base.on_will_dispose()
    }

    fn resolve(&self) -> Promise<()> {
        self.resolve_impl(None)
    }

    fn is_resolved(&self) -> bool {
        self.base.text_editor_model().is_some()
    }

    fn is_disposed(&self) -> bool {
        self.base.is_disposed()
    }
}

impl IWorkingCopy for TextFileEditorModel {
    /// `typeId`: IMPORTANT: never change this to not break existing assumptions (e.g. backups).
    fn working_copy_type_id(&self) -> String {
        NO_TYPE_ID.to_string()
    }

    fn resource(&self) -> URI {
        self.resource.clone()
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

    fn is_dirty(&self) -> bool {
        self.dirty.get()
    }

    fn is_modified(&self) -> bool {
        self.dirty.get()
    }

    fn backup(&self, _token: &CancellationToken) -> Promise<WorkingCopyBackup> {
        // Fill in metadata if we are resolved
        let meta = self.last_resolved_file_stat.borrow().as_ref().map(|stat| {
            BackupMetaData { mtime: stat.mtime(), ctime: stat.ctime(), size: stat.size(), etag: stat.etag().to_string(), orphaned: self.in_orphan_mode.get() }.to_dynamic()
        });

        // Fill in content the same way we would do when
        // saving the file via the text file service
        // encoding support (hardcode UTF-8)
        let snapshot = self.base.create_snapshot().map(TextOrSnapshot::Snapshot);
        let content = self.services.text_file_service.get_encoded_readable(
            Some(&self.resource),
            snapshot,
            Some(WriteTextFileOptions { encoding: Some(UTF8.to_string()), ..Default::default() }),
        );

        async_fn(async move {
            let content = content.await?.map(|content| match content {
                crate::services::textfile::textfiles::EncodedContent::Readable(readable) => WorkingCopyBackupContent::Readable(readable),
                crate::services::textfile::textfiles::EncodedContent::Buffer(buffer) => {
                    let readable: Box<dyn je_base::stream::Readable<je_base::buffer::VSBuffer>> = Box::new(je_base::stream::to_readable(buffer));
                    WorkingCopyBackupContent::Readable(Rc::new(RefCell::new(readable)))
                }
            });

            Ok(WorkingCopyBackup { meta, content })
        })
    }

    fn save(&self, options: Option<SaveOptions>) -> Promise<bool> {
        self.save_text_file(TextFileSaveAsOptions::from_save_options(options.unwrap_or_default()))
    }

    fn revert(&self, options: Option<RevertOptions>) -> Promise<()> {
        if !self.is_resolved() {
            return Promise::resolved(());
        }

        // Unset flags
        let was_dirty = self.dirty.get();
        let undo = self.do_set_dirty(false);

        // Force read from disk unless reverting soft
        let soft_undo = options.and_then(|options| options.soft).unwrap_or(false);

        let this = self.rc();
        async_fn(async move {
            if !soft_undo {
                if let Err(error) = this.force_resolve_from_file().await {
                    // FileNotFound means the file got deleted meanwhile, so ignore it
                    if Self::file_operation_result_of(&error) != Some(FileOperationResult::FileNotFound) {
                        // Set flags back to previous values, we are still dirty if revert failed
                        undo();

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
}

impl ILanguageSupport for TextFileEditorModel {
    fn set_language_id(&self, language_id: &str, source: Option<&str>) {
        self.base.set_language_id(language_id, source);

        *self.preferred_language_id.borrow_mut() = Some(language_id.to_string());
    }
}

impl ITextFileEditorModel for TextFileEditorModel {
    fn on_did_save_stored(&self) -> Event<StoredFileWorkingCopySaveEvent> {
        self.on_did_save.event()
    }

    fn on_did_save_error(&self) -> Event<()> {
        self.on_did_save_error.event()
    }

    fn on_did_change_orphaned(&self) -> Event<()> {
        self.on_did_change_orphaned.event()
    }

    fn on_did_change_readonly(&self) -> Event<()> {
        self.on_did_change_readonly.event()
    }

    fn on_did_change_encoding(&self) -> Event<()> {
        self.on_did_change_encoding.event()
    }

    fn has_state(&self, state: TextFileEditorModelState) -> bool {
        match state {
            TextFileEditorModelState::Conflict => self.in_conflict_mode.get(),
            TextFileEditorModelState::Dirty => self.dirty.get(),
            TextFileEditorModelState::Error => self.in_error_mode.get(),
            TextFileEditorModelState::Orphan => self.in_orphan_mode.get(),
            TextFileEditorModelState::PendingSave => self.save_sequentializer.is_running(),
            TextFileEditorModelState::Saved => !self.dirty.get(),
        }
    }

    fn join_state(&self, _state: TextFileEditorModelState) -> Promise<()> {
        self.save_sequentializer.running().unwrap_or_else(|| Promise::resolved(()))
    }

    fn update_preferred_encoding(&self, encoding: Option<&str>) {
        if !self.is_new_encoding(encoding) {
            return;
        }

        *self.preferred_encoding.borrow_mut() = encoding.map(str::to_string);

        // Emit
        self.on_did_change_encoding.fire(());
    }

    fn save_text_file(&self, options: TextFileSaveAsOptions) -> Promise<bool> {
        if !self.is_resolved() {
            return Promise::resolved(false);
        }

        if self.is_readonly_flag() {
            self.trace("save() - ignoring request for readonly resource");

            return Promise::resolved(false); // if model is readonly we do not attempt to save at all
        }

        if (self.in_conflict_mode.get() || self.in_error_mode.get())
            && matches!(options.reason(), Some(SaveReason::Auto) | Some(SaveReason::FocusChange) | Some(SaveReason::WindowChange))
        {
            self.trace("save() - ignoring auto save request for model that is in conflict or error");

            return Promise::resolved(false); // if model is in save conflict or error, do not save unless save reason is explicit
        }

        // Actually do save and log
        self.trace("save() - enter");
        let this = self.rc();
        let saving = self.do_save(options);
        async_fn(async move {
            saving.await?;
            this.trace("save() - exit");

            Ok(this.has_state(TextFileEditorModelState::Saved))
        })
    }

    fn resolve_text_file(&self, options: Option<TextFileResolveOptions>) -> Promise<()> {
        self.resolve_impl(options)
    }

    fn get_encoding(&self) -> Option<String> {
        self.preferred_encoding.borrow().clone().filter(|encoding| !encoding.is_empty()).or_else(|| self.content_encoding.borrow().clone())
    }

    fn set_encoding(&self, encoding: &str, mode: EncodingMode) -> Promise<()> {
        // Remember that an explicit encoding was set
        self.has_encoding_set_explicitly.set(true);

        self.set_encoding_internal(encoding, mode)
    }

    fn get_language_id(&self) -> Option<String> {
        if let Some(text_editor_model) = self.base.text_editor_model() {
            return Some(text_editor_model.get_language_id());
        }

        self.preferred_language_id.borrow().clone()
    }

    fn text_editor_model(&self) -> Option<Rc<TextModel>> {
        self.base.text_editor_model()
    }

    fn create_snapshot(&self) -> Option<Box<dyn TextSnapshot>> {
        self.base.create_snapshot()
    }
}
