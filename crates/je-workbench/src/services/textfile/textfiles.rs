//! Porte de `src/vs/workbench/services/textfile/common/textfiles.ts`: os contratos do serviço de arquivos de texto
//! (`ITextFileService`), do modelo (`ITextFileEditorModel`) e do gerenciador de modelos
//! (`ITextFileEditorModelManager`), os estados, os eventos, as opções de salvar e resolver e os auxiliares de snapshot.
//!
//! Divergências:
//! - **Herança de interfaces.** `ITextFileEditorModel extends ITextEditorModel, IEncodingSupport, ILanguageSupport,
//!   IWorkingCopy` é o trait [`ITextFileEditorModel`] com `IWorkingCopy` como supertrait; os membros de
//!   `IEncodingSupport` (`getEncoding`, `setEncoding`) e `ILanguageSupport` (`setLanguageId`) e os de `ITextEditorModel`
//!   que o serviço lê entram nele. Nomes que colidem com `IWorkingCopy` ganham outro nome: `onDidSave` com `stat` é
//!   [`ITextFileEditorModel::on_did_save_stored`] (como na `TestWorkingCopy`), `save(ITextFileSaveAsOptions)` é
//!   [`ITextFileEditorModel::save_text_file`] (o `save(SaveOptions)` do `IWorkingCopy` converte e chama este) e
//!   `resolve(options)` é [`ITextFileEditorModel::resolve_text_file`] (o `resolve()` do `IEditorModel` não recebe opções).
//! - **Opções de salvar.** `ITextFileSaveOptions extends ISaveOptions` e `ITextFileSaveAsOptions extends
//!   ITextFileSaveOptions` são structs com o nível base em `base` e `options`.
//! - **`IReadTextFileOptions`** (`IReadTextFileEncodingOptions & IReadFileStreamOptions`) é a struct
//!   [`ReadTextFileOptions`], com o `limits` do original em `limits`.
//! - **`ITextBufferFactory`** é consumido uma vez ao criar o buffer ([`PieceTreeTextBufferFactory::create`] recebe `self`):
//!   [`TextBufferFactoryRef`] o guarda num `Rc<RefCell<Option<_>>>` para as opções e o conteúdo serem clonáveis e o
//!   consumidor o retirar com [`TextBufferFactoryRef::take`].
//! - **`ITextSnapshot`** é `Box<dyn TextSnapshot>`; `string | ITextSnapshot` é [`TextOrSnapshot`], e o
//!   `VSBuffer | VSBufferReadable` é [`EncodedContent`] (o leitor em `Rc<RefCell<..>>`, como o `WorkingCopyBackupContent`).
//! - **`TextFileOperationError`** é um [`Error`] com a propriedade `textFileOperationResult` (sobre o `FileOperationError`
//!   `FILE_OTHER_ERROR`); [`is_text_file_operation_error`] é o `isTextFileOperationError`.
//! - **`untitled: IUntitledTextEditorModelManager`** de `ITextFileService` é [`ITextFileService::untitled`] (a propriedade
//!   vira método, como as demais do porte).
//! - **`IEncodingSupport`** é o trait [`IEncodingSupport`] (o `UntitledTextEditorInput` o implementa); o
//!   `ITextFileEditorModel` mantém `getEncoding` e `setEncoding` como membros próprios.
//! - **`getEncodedReadable`** e suas sobrecargas são [`ITextFileService::get_encoded_readable`], que devolve
//!   `Option<EncodedContent>` (`undefined` quando o valor é `undefined`).

use std::cell::RefCell;
use std::rc::Rc;

use je_base::buffer::{VSBuffer, VSBufferReadableStream};
use je_base::cancellation::CancellationToken;
use je_base::errors::Error;
use je_base::event::Event;
use je_base::lifecycle::DisposableRef;
use je_base::promise::Promise;
use je_base::stream::Readable;
use je_base::uri::URI;
use je_editor::model::TextSnapshot;
use je_editor::piece_tree_text_buffer_builder::PieceTreeTextBufferFactory;
use je_editor::text_model::TextModel;
use je_platform::files::files::{FileOperationError, FileOperationResult, FileReadLimits, FileStatWithMetadata, ReadFileOptions, WriteFileOptions};

use crate::common::editor::editor_model::IEditorModel;
use crate::common::editor::{RevertOptions, SaveOptions, SaveReason, SaveSource};
use crate::services::files_configuration::files_configuration_service::Readonly;
use crate::services::textfile::collaborators::{IProgress, ProgressStep};
use crate::services::untitled::untitled_text_editor_service::IUntitledTextEditorModelManager;
use crate::services::working_copy::stored_file_working_copy::StoredFileWorkingCopySaveEvent;
use crate::services::working_copy::working_copy::{IWorkingCopy, WorkingCopyBackupContent};

je_instantiation::service_id!(pub I_TEXT_FILE_SERVICE: dyn ITextFileService = "textFileService");

/// `VSBufferReadable`: o leitor de buffers, compartilhado.
pub type VSBufferReadable = Rc<RefCell<Box<dyn Readable<VSBuffer>>>>;

/// `VSBuffer | VSBufferReadable`.
#[derive(Clone)]
pub enum EncodedContent {
    Buffer(VSBuffer),
    Readable(VSBufferReadable),
}

/// O conteúdo de um backup de working copy a partir do que o `getEncodedReadable` devolveu: o buffer vira o leitor dele
/// (`bufferToReadable`), que é o que o `IWorkingCopyBackup.content` aceita.
impl From<EncodedContent> for WorkingCopyBackupContent {
    fn from(content: EncodedContent) -> Self {
        match content {
            EncodedContent::Readable(readable) => WorkingCopyBackupContent::Readable(readable),
            EncodedContent::Buffer(buffer) => {
                let readable: Box<dyn Readable<VSBuffer>> = Box::new(je_base::stream::to_readable(buffer));
                WorkingCopyBackupContent::Readable(Rc::new(RefCell::new(readable)))
            }
        }
    }
}

/// `string | ITextSnapshot`.
pub enum TextOrSnapshot {
    Text(String),
    Snapshot(Box<dyn TextSnapshot>),
}

impl From<&str> for TextOrSnapshot {
    fn from(value: &str) -> Self {
        TextOrSnapshot::Text(value.to_string())
    }
}

impl From<String> for TextOrSnapshot {
    fn from(value: String) -> Self {
        TextOrSnapshot::Text(value)
    }
}

impl From<Box<dyn TextSnapshot>> for TextOrSnapshot {
    fn from(value: Box<dyn TextSnapshot>) -> Self {
        TextOrSnapshot::Snapshot(value)
    }
}

/// `ITextBufferFactory`, compartilhado e consumido uma vez (ver as divergências do módulo).
#[derive(Clone)]
pub struct TextBufferFactoryRef(Rc<RefCell<Option<PieceTreeTextBufferFactory>>>);

impl TextBufferFactoryRef {
    pub fn new(factory: PieceTreeTextBufferFactory) -> Self {
        Self(Rc::new(RefCell::new(Some(factory))))
    }

    /// Retira a fábrica (uma vez só).
    pub fn take(&self) -> Option<PieceTreeTextBufferFactory> {
        self.0.borrow_mut().take()
    }

    /// `getFirstLineText(lengthLimit)` (vazio depois de a fábrica ser consumida).
    pub fn get_first_line_text(&self, length_limit: usize) -> String {
        self.0.borrow().as_ref().map(|factory| factory.get_first_line_text(length_limit).to_string_lossy()).unwrap_or_default()
    }
}

/// `IReadTextFileEncodingOptions`.
#[derive(Clone, Debug, Default)]
pub struct ReadTextFileEncodingOptions {
    /// The optional encoding parameter allows to specify the desired encoding when resolving the contents of the file.
    pub encoding: Option<String>,
    /// The optional guessEncoding parameter allows to guess encoding from content of the file.
    pub auto_guess_encoding: Option<bool>,
    /// The optional candidateGuessEncodings parameter limits the allowed encodings to guess from.
    pub candidate_guess_encodings: Option<Vec<String>>,
    /// The optional acceptTextOnly parameter allows to fail this request early if the file contents are not textual.
    pub accept_text_only: Option<bool>,
}

/// `IReadTextFileOptions` (`IReadTextFileEncodingOptions` e `IReadFileStreamOptions`).
#[derive(Clone, Debug, Default)]
pub struct ReadTextFileOptions {
    pub encoding: ReadTextFileEncodingOptions,
    /// O `IReadFileOptions` (`etag`, `atomic`, posição e tamanho).
    pub read: ReadFileOptions,
    /// `limits`.
    pub limits: Option<FileReadLimits>,
}

/// `IWriteTextFileOptions`.
#[derive(Clone, Debug, Default)]
pub struct WriteTextFileOptions {
    /// O `IWriteFileOptions` (`mtime`, `etag`, `unlock`, `atomic`, `append`).
    pub write: WriteFileOptions,
    /// The encoding to use when updating a file.
    pub encoding: Option<String>,
    /// Whether to write to the file as elevated (admin) user. When setting this option a prompt will ask the user to
    /// authenticate as super user.
    pub write_elevated: Option<bool>,
}

/// `TextFileOperationResult`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextFileOperationResult {
    FileIsBinary = 0,
}

/// `TextFileOperationError`: o [`Error`] do `FileOperationError` `FILE_OTHER_ERROR` com `textFileOperationResult`.
pub struct TextFileOperationError;

impl TextFileOperationError {
    /// `new TextFileOperationError(message, textFileOperationResult, options)`.
    pub fn new(message: impl Into<String>, text_file_operation_result: TextFileOperationResult) -> Error {
        FileOperationError::new(message, FileOperationResult::FileOtherError).into_error().with_prop("textFileOperationResult", text_file_operation_result as usize)
    }
}

/// `TextFileOperationError.isTextFileOperationError(obj)`.
pub fn is_text_file_operation_error(error: &Error) -> bool {
    error.prop("textFileOperationResult").is_some()
}

/// O `error.textFileOperationResult` de um [`TextFileOperationError`] (`None` quando o erro é de outra origem).
pub fn text_file_operation_result_of(error: &Error) -> Option<TextFileOperationResult> {
    match error.prop("textFileOperationResult")? {
        "0" => Some(TextFileOperationResult::FileIsBinary),
        _ => None,
    }
}

/// `IResourceEncoding`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResourceEncoding {
    pub encoding: String,
    pub has_bom: bool,
}

/// `IResourceEncodings`.
pub trait IResourceEncodings {
    fn get_preferred_read_encoding(&self, resource: &URI) -> Promise<ResourceEncoding>;
    fn get_preferred_write_encoding(&self, resource: &URI, preferred_encoding: Option<&str>) -> Promise<ResourceEncoding>;
}

/// `{ preferredEncoding, guessEncoding, candidateGuessEncodings }` de `resolveDecoding`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolvedDecoding {
    pub preferred_encoding: String,
    pub guess_encoding: bool,
    pub candidate_guess_encodings: Vec<String>,
}

/// `{ encoding, addBOM }` de `resolveEncoding`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolvedEncoding {
    pub encoding: String,
    pub add_bom: bool,
}

/// `ITextFileSaveOptions`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TextFileSaveOptions {
    /// O `ISaveOptions` de que herda (`reason`, `source`, `force`, `skipSaveParticipants`, `availableFileSystems`).
    pub base: SaveOptions,
    /// Save the file with an attempt to unlock it.
    pub write_unlock: Option<bool>,
    /// Save the file with elevated privileges. Note: This may not be supported in all environments.
    pub write_elevated: Option<bool>,
    /// Allows to write to a file even if it has been modified on disk.
    pub ignore_modified_since: Option<bool>,
    /// If set, will bubble up the error to the caller instead of handling it.
    pub ignore_error_handler: Option<bool>,
}

/// `ITextFileSaveAsOptions`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TextFileSaveAsOptions {
    /// O `ITextFileSaveOptions` de que herda.
    pub options: TextFileSaveOptions,
    /// Optional URI of the resource the text file is saved from if known.
    pub from: Option<URI>,
    /// Optional URI to use as suggested file path to save as.
    pub suggested_target: Option<URI>,
}

impl TextFileSaveAsOptions {
    /// Opções só com o `reason`.
    pub fn with_reason(reason: SaveReason) -> Self {
        Self { options: TextFileSaveOptions { base: SaveOptions { reason: Some(reason), ..SaveOptions::default() }, ..TextFileSaveOptions::default() }, ..Self::default() }
    }

    /// As opções do `IWorkingCopy.save(ISaveOptions)` como `ITextFileSaveAsOptions` (os demais campos ausentes).
    pub fn from_save_options(options: SaveOptions) -> Self {
        Self { options: TextFileSaveOptions { base: options, ..TextFileSaveOptions::default() }, ..Self::default() }
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
}

/// `ISaveErrorHandler`: installed on the text file editor model manager to run code when save errors occur.
pub trait ISaveErrorHandler {
    /// Called whenever a save fails.
    fn on_save_error(&self, error: &Error, model: &dyn ITextFileEditorModel, options: &TextFileSaveAsOptions);
}

/// `TextFileEditorModelState`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextFileEditorModelState {
    /// A model is saved.
    Saved,
    /// A model is dirty.
    Dirty,
    /// A model is currently being saved but this operation has not completed yet.
    PendingSave,
    /// A model is in conflict mode when changes cannot be saved because the underlying file has changed. Models in
    /// conflict mode are always dirty.
    Conflict,
    /// A model is in orphan state when the underlying file has been deleted.
    Orphan,
    /// Any error that happens during a save that is not causing the CONFLICT state. Models in error mode are always dirty.
    Error,
}

/// `TextFileResolveReason`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextFileResolveReason {
    Editor = 1,
    Reference = 2,
    Other = 3,
}

/// `IBaseTextFileContent` (`IBaseFileStatWithMetadata` e `encoding`).
#[derive(Clone, Debug)]
pub struct BaseTextFileContent {
    pub resource: URI,
    pub name: String,
    pub mtime: i64,
    pub ctime: i64,
    pub size: u64,
    pub etag: String,
    pub readonly: bool,
    pub locked: bool,
    pub executable: bool,
    /// The encoding of the content if known.
    pub encoding: String,
}

impl BaseTextFileContent {
    /// O stat com os metadados e o encoding conhecido.
    pub fn from_stat(stat: &FileStatWithMetadata, encoding: impl Into<String>) -> Self {
        Self {
            resource: stat.resource.clone(),
            name: stat.name.clone(),
            mtime: stat.mtime(),
            ctime: stat.ctime(),
            size: stat.size(),
            etag: stat.etag().to_string(),
            readonly: stat.readonly(),
            locked: stat.locked(),
            executable: stat.executable(),
            encoding: encoding.into(),
        }
    }
}

/// `ITextFileContent`.
#[derive(Clone, Debug)]
pub struct TextFileContent {
    pub base: BaseTextFileContent,
    /// The content of a text file.
    pub value: String,
}

/// `ITextFileStreamContent`.
#[derive(Clone)]
pub struct TextFileStreamContent {
    pub base: BaseTextFileContent,
    /// The line grouped content of a text file.
    pub value: TextBufferFactoryRef,
}

/// `ITextFileEditorModelResolveOrCreateOptions` (que herda `ITextFileResolveOptions`).
#[derive(Clone, Default)]
pub struct TextFileEditorModelResolveOrCreateOptions {
    pub resolve: TextFileResolveOptions,
    /// The language id to use for the model text content.
    pub language_id: Option<String>,
    /// The encoding to use when resolving the model text content.
    pub encoding: Option<String>,
    /// If the model was already resolved before, allows to trigger a reload of it to fetch the latest contents.
    pub reload: Option<TextFileReloadOptions>,
}

/// `reload` de `ITextFileEditorModelResolveOrCreateOptions`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TextFileReloadOptions {
    /// Controls whether the reload happens in the background or whether `resolve` will await the reload to happen.
    pub async_: bool,
}

/// `ITextFileSaveEvent`: o `ITextFileEditorModelSaveEvent` com o modelo salvo.
#[derive(Clone)]
pub struct TextFileSaveEvent {
    /// The model that was saved.
    pub model: Rc<dyn ITextFileEditorModel>,
    pub event: StoredFileWorkingCopySaveEvent,
}

/// `ITextFileResolveEvent`.
#[derive(Clone)]
pub struct TextFileResolveEvent {
    /// The model that was resolved.
    pub model: Rc<dyn ITextFileEditorModel>,
    /// The reason why the model was resolved.
    pub reason: TextFileResolveReason,
}

/// `ITextFileSaveParticipantContext`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextFileSaveParticipantContext {
    /// The reason why the save was triggered.
    pub reason: SaveReason,
    /// Only applies to when a text file was saved as, for example when starting with untitled and saving. This provides
    /// access to the initial resource the text file had before.
    pub saved_from: Option<URI>,
}

/// `ITextFileSaveParticipant`.
pub trait ITextFileSaveParticipant {
    /// The ordinal number which determines the order of participation. Lower values mean to participant sooner.
    fn ordinal(&self) -> Option<i32> {
        None
    }

    /// Participate in a save of a model. Allows to change the model before it is being saved to disk.
    fn participate(
        &self,
        model: Rc<dyn ITextFileEditorModel>,
        context: TextFileSaveParticipantContext,
        progress: Rc<dyn IProgress<ProgressStep>>,
        token: CancellationToken,
    ) -> Promise<()>;
}

/// `ITextFileEditorModelManager`.
pub trait ITextFileEditorModelManager {
    fn on_did_create(&self) -> Event<Rc<dyn ITextFileEditorModel>>;
    fn on_did_resolve(&self) -> Event<TextFileResolveEvent>;
    fn on_did_change_dirty(&self) -> Event<Rc<dyn ITextFileEditorModel>>;
    fn on_did_change_readonly(&self) -> Event<Rc<dyn ITextFileEditorModel>>;
    fn on_did_remove(&self) -> Event<URI>;
    fn on_did_change_orphaned(&self) -> Event<Rc<dyn ITextFileEditorModel>>;
    fn on_did_change_encoding(&self) -> Event<Rc<dyn ITextFileEditorModel>>;
    fn on_did_save_error(&self) -> Event<Rc<dyn ITextFileEditorModel>>;
    fn on_did_save(&self) -> Event<TextFileSaveEvent>;
    fn on_did_revert(&self) -> Event<Rc<dyn ITextFileEditorModel>>;

    /// Access to all text file editor models in memory.
    fn models(&self) -> Vec<Rc<dyn ITextFileEditorModel>>;

    /// Allows to configure the error handler that is called on save errors (`saveErrorHandler = ...`).
    fn set_save_error_handler(&self, handler: Rc<dyn ISaveErrorHandler>);

    /// `saveErrorHandler`: the error handler that is called on save errors.
    fn save_error_handler(&self) -> Rc<dyn ISaveErrorHandler>;

    /// Returns the text file editor model for the provided resource or `None` if none.
    fn get(&self, resource: &URI) -> Option<Rc<dyn ITextFileEditorModel>>;

    /// Allows to resolve a text file model from disk.
    fn resolve(&self, resource: &URI, options: Option<TextFileEditorModelResolveOrCreateOptions>) -> Promise<Rc<dyn ITextFileEditorModel>>;

    /// Adds a participant for saving text file models.
    fn add_save_participant(&self, participant: Rc<dyn ITextFileSaveParticipant>) -> DisposableRef;

    /// Runs the registered save participants on the provided model.
    fn run_save_participants(
        &self,
        model: Rc<dyn ITextFileEditorModel>,
        context: TextFileSaveParticipantContext,
        progress: Rc<dyn IProgress<ProgressStep>>,
        token: CancellationToken,
    ) -> Promise<()>;

    /// Waits for the model to be ready to be disposed. There may be conditions under which the model cannot be disposed,
    /// e.g. when it is dirty. Once the promise is settled, it is safe to dispose the model. `None` é o `true` imediato.
    fn can_dispose(&self, model: Rc<dyn ITextFileEditorModel>) -> Option<Promise<()>>;
}

/// `ITextFileResolveOptions`.
#[derive(Clone, Default)]
pub struct TextFileResolveOptions {
    /// The contents to use for the model if known. If not provided, the contents will be retrieved from the underlying
    /// resource or backup if present.
    pub contents: Option<TextBufferFactoryRef>,
    /// Go to file bypassing any cache of the model if any.
    pub force_read_from_file: Option<bool>,
    /// Allow to resolve a model even if we think it is a binary file.
    pub allow_binary: Option<bool>,
    /// Context why the model is being resolved.
    pub reason: Option<TextFileResolveReason>,
    /// If provided, the size of the file will be checked against the limits and an error will be thrown if any limit
    /// is exceeded.
    pub limits: Option<FileReadLimits>,
}

/// `EncodingMode`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EncodingMode {
    /// Instructs the encoding support to encode the object with the provided encoding.
    Encode,
    /// Instructs the encoding support to decode the object with the provided encoding.
    Decode,
}

/// `ILanguageSupport`.
pub trait ILanguageSupport {
    /// Sets the language id of the object.
    fn set_language_id(&self, language_id: &str, source: Option<&str>);
}

/// `IEncodingSupport`.
pub trait IEncodingSupport {
    /// Gets the encoding of the object if known.
    fn get_encoding(&self) -> Option<String>;

    /// Sets the encoding for the object for saving.
    fn set_encoding(&self, encoding: &str, mode: EncodingMode) -> Promise<()>;
}

/// `ITextFileEditorModel` (e, por herança, `ITextEditorModel`, `IEncodingSupport`, `ILanguageSupport` e `IWorkingCopy`).
pub trait ITextFileEditorModel: IWorkingCopy + IEditorModel + ILanguageSupport {
    fn on_did_save_stored(&self) -> Event<StoredFileWorkingCopySaveEvent>;
    fn on_did_save_error(&self) -> Event<()>;
    fn on_did_change_orphaned(&self) -> Event<()>;
    fn on_did_change_readonly(&self) -> Event<()>;
    fn on_did_change_encoding(&self) -> Event<()>;

    fn has_state(&self, state: TextFileEditorModelState) -> bool;
    fn join_state(&self, state: TextFileEditorModelState) -> Promise<()>;

    fn update_preferred_encoding(&self, encoding: Option<&str>);

    fn save_text_file(&self, options: TextFileSaveAsOptions) -> Promise<bool>;
    fn resolve_text_file(&self, options: Option<TextFileResolveOptions>) -> Promise<()>;

    /// `IEncodingSupport.getEncoding`.
    fn get_encoding(&self) -> Option<String>;
    /// `IEncodingSupport.setEncoding`.
    fn set_encoding(&self, encoding: &str, mode: EncodingMode) -> Promise<()>;
    fn get_language_id(&self) -> Option<String>;

    /// `isReadonly()` de `ITextEditorModel` (`boolean | IMarkdownString`). O padrão é não somente leitura, para quem não
    /// consulta a configuração de arquivos.
    fn is_readonly(&self) -> Readonly {
        Readonly::Bool(false)
    }

    /// `textEditorModel` (`ITextModel | null`).
    fn text_editor_model(&self) -> Option<Rc<TextModel>>;

    /// `createSnapshot()` (`ITextSnapshot | null`).
    fn create_snapshot(&self) -> Option<Box<dyn TextSnapshot>>;
}

/// `snapshotToString(snapshot)`.
pub fn snapshot_to_string(snapshot: &mut dyn TextSnapshot) -> String {
    let mut units: Vec<u16> = Vec::new();
    while let Some(chunk) = snapshot.read() {
        units.extend_from_slice(chunk.units());
    }
    String::from_utf16_lossy(&units)
}

/// O `TextSnapshot` de uma só leitura de `stringToSnapshot`.
struct StringSnapshot {
    value: Option<je_base::JsString>,
}

impl TextSnapshot for StringSnapshot {
    fn read(&mut self) -> Option<je_base::JsString> {
        self.value.take()
    }
}

/// `stringToSnapshot(value)`.
pub fn string_to_snapshot(value: &str) -> Box<dyn TextSnapshot> {
    Box::new(StringSnapshot { value: Some(je_base::JsString::from(value)) })
}

/// O `VSBufferReadable` de `toBufferOrReadable(snapshot)`: lê um trecho do snapshot e o devolve como buffer.
struct SnapshotReadable {
    snapshot: Box<dyn TextSnapshot>,
}

impl Readable<VSBuffer> for SnapshotReadable {
    fn read(&mut self) -> Option<VSBuffer> {
        self.snapshot.read().map(|chunk| VSBuffer::from_string(&chunk.to_string_lossy()))
    }
}

/// `toBufferOrReadable(value)`: `undefined` é `None`, o texto vira buffer e o snapshot um leitor.
pub fn to_buffer_or_readable(value: Option<TextOrSnapshot>) -> Option<EncodedContent> {
    match value? {
        TextOrSnapshot::Text(text) => Some(EncodedContent::Buffer(VSBuffer::from_string(&text))),
        TextOrSnapshot::Snapshot(snapshot) => {
            let readable: Box<dyn Readable<VSBuffer>> = Box::new(SnapshotReadable { snapshot });
            Some(EncodedContent::Readable(Rc::new(RefCell::new(readable))))
        }
    }
}

/// `ITextFileService`.
pub trait ITextFileService {
    /// Access to the manager of text file editor models providing further methods to work with them.
    fn files(&self) -> Rc<dyn ITextFileEditorModelManager>;

    /// Access to the manager of untitled text editors.
    fn untitled(&self) -> Rc<dyn IUntitledTextEditorModelManager>;

    /// Helper to determine encoding for resources.
    fn encoding(&self) -> Rc<dyn IResourceEncodings>;

    /// A resource is dirty if it has unsaved changes or is an untitled file not yet saved.
    fn is_dirty(&self, resource: &URI) -> bool;

    /// Saves the resource. Path of the saved resource or `None` if canceled.
    fn save(&self, resource: &URI, options: Option<TextFileSaveOptions>) -> Promise<Option<URI>>;

    /// Saves the provided resource asking the user for a file name or using the provided one.
    fn save_as(&self, resource: &URI, target_resource: Option<&URI>, options: Option<TextFileSaveAsOptions>) -> Promise<Option<URI>>;

    /// Reverts the provided resource.
    fn revert(&self, resource: &URI, options: Option<RevertOptions>) -> Promise<()>;

    /// Read the contents of a file identified by the resource.
    fn read(&self, resource: &URI, options: Option<ReadTextFileOptions>) -> Promise<TextFileContent>;

    /// Read the contents of a file identified by the resource as stream.
    fn read_stream(&self, resource: &URI, options: Option<ReadTextFileOptions>) -> Promise<TextFileStreamContent>;

    /// Update a file with given contents.
    fn write(&self, resource: &URI, value: TextOrSnapshot, options: Option<WriteTextFileOptions>) -> Promise<FileStatWithMetadata>;

    /// Create files. If the file exists it will be overwritten with the contents if the options enable to overwrite.
    fn create(&self, operations: Vec<TextFileCreateOperation>, undo_info: Option<crate::services::textfile::collaborators::FileOperationUndoRedoInfo>) -> Promise<Vec<FileStatWithMetadata>>;

    /// Returns the readable that uses the appropriate encoding. This method should be used whenever a `string` or
    /// `ITextSnapshot` is being persisted to the file system.
    fn get_encoded_readable(&self, resource: Option<&URI>, value: Option<TextOrSnapshot>, options: Option<WriteTextFileOptions>) -> Promise<Option<EncodedContent>>;

    /// Returns a stream of strings that uses the appropriate encoding. This method should be used whenever a
    /// `VSBufferReadableStream` is being loaded from the file system. Will throw an error if `acceptTextOnly: true` for
    /// resources that seem to be binary.
    fn get_decoded_stream(
        &self,
        resource: Option<&URI>,
        value: VSBufferReadableStream,
        options: Option<ReadTextFileEncodingOptions>,
    ) -> Promise<Rc<dyn je_base::stream::ReadableStream<String>>>;

    /// Get the encoding for the provided `resource`. Will try to determine the encoding from any existing model for that
    /// `resource` and fallback to the configured defaults.
    fn get_encoding(&self, resource: &URI) -> String;

    /// Get the properties for decoding the provided `resource` based on configuration.
    fn resolve_decoding(&self, resource: Option<&URI>, options: Option<ReadTextFileEncodingOptions>) -> Promise<ResolvedDecoding>;

    /// Get the properties for encoding the provided `resource` based on configuration.
    fn resolve_encoding(&self, resource: Option<&URI>, options: Option<WriteTextFileOptions>) -> Promise<ResolvedEncoding>;

    /// Given a detected encoding, validate it against the configured encoding options.
    fn validate_detected_encoding(&self, resource: Option<&URI>, detected_encoding: &str, options: Option<ReadTextFileEncodingOptions>) -> Promise<String>;
}

/// `{ resource, value?, options?: { overwrite? } }` de `ITextFileService.create`.
pub struct TextFileCreateOperation {
    pub resource: URI,
    pub value: Option<TextOrSnapshot>,
    pub overwrite: Option<bool>,
}
