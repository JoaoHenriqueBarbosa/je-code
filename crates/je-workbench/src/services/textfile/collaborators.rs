//! Contratos reduzidos dos serviços que o `TextFileEditorModel`, o `TextFileSaveParticipant` e o
//! `TextFileEditorModelManager` leem e que ainda não foram portados: `IProgressService`
//! (`platform/progress/common/progress.ts`) e `IWorkingCopyFileService` (`workingCopyFileService.ts`). O
//! `IWorkingCopyBackupService` completo é o de `services/working_copy/working_copy_backup.rs`. O `ILanguageDetectionService` e o `IAccessibilityService`
//! são os dos módulos deles (`language_detection` e `je_platform::accessibility`).
//!
//! Seguem a regra do porte: cada contrato traz só o que o consumidor lê, com a assinatura do original, e cresce
//! quando o serviço completo for portado (o dono do serviço move o trait para o módulo dele).
//!
//! Divergências:
//! - **Genéricos.** `withProgress<R>(options, task, onDidCancel): Promise<R>` não é objeto-seguro com `R` livre: os dois
//!   chamadores deste porte devolvem `Promise<void>`, então `R` é `()`.
//! - **Eventos com `waitUntil`.** `WorkingCopyFileEvent extends IWaitUntil`: [`WorkingCopyFileEvent`] guarda as
//!   promessas em `joiners`, que o serviço espera depois de disparar (o `waitUntil(promise)` é
//!   [`WorkingCopyFileEvent::wait_until`]).

use std::cell::RefCell;
use std::rc::Rc;

use je_base::cancellation::CancellationToken;
use je_base::dynamic::Dynamic;
use je_base::errors::Error;
use je_base::event::Event;
use je_base::promise::Promise;
use je_base::uri::URI;
use je_editor::text_model::TextModel;
use je_platform::files::files::{FileOperation, FileStatWithMetadata};
use je_platform::notification::notification::NotificationPriority;
use je_platform::notification::notification_telemetry::NotificationTelemetryId;

use crate::common::editor::{EditorIdentifier, EditorInputOrUntyped, SaveOptions, SaveReason};
use crate::services::textfile::textfiles::EncodedContent;
use crate::services::working_copy::stored_file_working_copy::IStoredFileWorkingCopy;

//#region IProgressService

/// `IProgressStep`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ProgressStep {
    pub message: Option<String>,
    pub increment: Option<f64>,
    pub total: Option<f64>,
}

/// `IProgress<T>`.
pub trait IProgress<T> {
    fn report(&self, item: T);
}

/// `ProgressLocation`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProgressLocation {
    Explorer = 1,
    Scm = 3,
    Extensions = 5,
    Window = 10,
    Notification = 15,
    Dialog = 20,
}

/// `cancellable?: boolean | string` de `IProgressNotificationOptions`: `true` ou o rótulo do botão de cancelar.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProgressCancellable {
    Yes,
    Label(String),
}

/// `IProgressOptions` (o recorte que `withProgress` lê: janela, notificação e diálogo).
#[derive(Clone, Debug)]
pub struct ProgressOptions {
    pub location: ProgressLocation,
    pub title: Option<String>,
    pub cancellable: Option<ProgressCancellable>,
    pub delay: Option<u64>,
    pub priority: Option<NotificationPriority>,
    pub telemetry: Option<NotificationTelemetryId>,
}

impl ProgressOptions {
    /// As opções só com o local, o resto ausente.
    pub fn at(location: ProgressLocation) -> Self {
        Self { location, title: None, cancellable: None, delay: None, priority: None, telemetry: None }
    }
}

/// A tarefa de `withProgress`: recebe o `IProgress` e devolve a promessa do trabalho.
pub type ProgressTask = Box<dyn FnOnce(Rc<dyn IProgress<ProgressStep>>) -> Promise<()>>;

/// `IProgressService`.
pub trait IProgressService {
    /// `withProgress(options, task, onDidCancel)`.
    fn with_progress(&self, options: ProgressOptions, task: ProgressTask, on_did_cancel: Option<Rc<dyn Fn()>>) -> Promise<()>;
}

je_instantiation::service_id!(pub I_PROGRESS_SERVICE: dyn IProgressService = "progressService");

//#endregion

//#region IWorkingCopyFileService

/// `{ source?: URI; target: URI }` de `WorkingCopyFileEvent.files`.
#[derive(Clone, Debug, PartialEq)]
pub struct WorkingCopyFileEventFile {
    pub source: Option<URI>,
    pub target: URI,
}

/// `WorkingCopyFileEvent`.
#[derive(Clone)]
pub struct WorkingCopyFileEvent {
    /// An identifier to correlate the operation through the
    /// different event types (before, after, error).
    pub correlation_id: usize,
    /// The file operation that is taking place.
    pub operation: FileOperation,
    /// The array of source/target pair of files involved in given operation.
    pub files: Vec<WorkingCopyFileEventFile>,
    joiners: Rc<RefCell<Vec<Promise<()>>>>,
}

impl WorkingCopyFileEvent {
    pub fn new(correlation_id: usize, operation: FileOperation, files: Vec<WorkingCopyFileEventFile>) -> Self {
        Self { correlation_id, operation, files, joiners: Rc::new(RefCell::new(Vec::new())) }
    }

    /// `waitUntil(promise)`: o serviço espera estas promessas antes de seguir.
    pub fn wait_until(&self, promise: Promise<()>) {
        self.joiners.borrow_mut().push(promise);
    }

    /// As promessas registradas por `waitUntil`, para o serviço que disparou o evento esperar.
    pub fn take_joiners(&self) -> Vec<Promise<()>> {
        std::mem::take(&mut *self.joiners.borrow_mut())
    }
}

/// `IFileOperationUndoRedoInfo`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FileOperationUndoRedoInfo {
    /// Id of the undo group that the file operation belongs to.
    pub undo_redo_group_id: Option<usize>,
    /// Flag indicates if the operation is triggered by undo/redo operations.
    pub is_undoing: Option<bool>,
}

/// `ICreateFileOperation` (`ICreateOperation` com `contents`): `contents` é o `VSBuffer | VSBufferReadable` que o
/// `TextFileService.create` produz (o `VSBufferReadableStream` do original não é produzido por ele).
#[derive(Clone)]
pub struct CreateFileOperation {
    pub resource: URI,
    pub contents: Option<EncodedContent>,
    pub overwrite: Option<bool>,
}

/// `Required<SourceTargetPair>` de `IMoveOperation.file`.
#[derive(Clone, Debug, PartialEq)]
pub struct RequiredSourceTargetPair {
    pub source: URI,
    pub target: URI,
}

/// `IMoveOperation`.
#[derive(Clone, Debug, PartialEq)]
pub struct MoveOperation {
    pub file: RequiredSourceTargetPair,
    pub overwrite: Option<bool>,
}

/// A mensagem de `create` e `move` num `IWorkingCopyFileService` que não os traz.
const WORKING_COPY_FILE_OPERATION_UNSUPPORTED: &str = "esta implementação de IWorkingCopyFileService não executa a operação de arquivo";

/// `IWorkingCopyFileService` (os três eventos que o gerenciador de modelos de arquivo de texto escuta, mais `create` e
/// `move`, que o `TextFileService` chama).
pub trait IWorkingCopyFileService {
    /// An event that is fired before a working copy file operation is about to run.
    fn on_will_run_working_copy_file_operation(&self) -> Event<WorkingCopyFileEvent>;

    /// An event that is fired after a working copy file operation has failed.
    fn on_did_fail_working_copy_file_operation(&self) -> Event<WorkingCopyFileEvent>;

    /// An event that is fired after a working copy file operation has been performed.
    fn on_did_run_working_copy_file_operation(&self) -> Event<WorkingCopyFileEvent>;

    /// Will create a file. Working copy owners can listen to the events to participate. Quem não implementa a operação
    /// (o contrato do serviço de arquivos de working copy ainda não foi portado por inteiro) rejeita.
    fn create(&self, _operations: Vec<CreateFileOperation>, _token: CancellationToken, _undo_info: Option<FileOperationUndoRedoInfo>) -> Promise<Vec<FileStatWithMetadata>> {
        Promise::rejected(Error::new(WORKING_COPY_FILE_OPERATION_UNSUPPORTED))
    }

    /// Will move working copies matching the provided resources and corresponding children to the target resources
    /// using the associated file service for those resources. Rejeita quando a implementação não a traz.
    fn move_(&self, _operations: Vec<MoveOperation>, _token: CancellationToken, _undo_info: Option<FileOperationUndoRedoInfo>) -> Promise<Vec<FileStatWithMetadata>> {
        Promise::rejected(Error::new(WORKING_COPY_FILE_OPERATION_UNSUPPORTED))
    }

    /// Whether save participants are present for stored file working copies. Sem participantes registrados (o caso do
    /// serviço que ainda não os tem) é `false`.
    fn has_save_participants(&self) -> bool {
        false
    }

    /// Runs all available save participants for stored file working copies. Sem participantes não há o que rodar.
    fn run_save_participants(
        &self,
        _working_copy: Rc<dyn IStoredFileWorkingCopy>,
        _context: StoredFileWorkingCopySaveParticipantContext,
        _progress: Rc<dyn IProgress<ProgressStep>>,
        _token: CancellationToken,
    ) -> Promise<()> {
        Promise::resolved(())
    }
}

je_instantiation::service_id!(pub I_WORKING_COPY_FILE_SERVICE: dyn IWorkingCopyFileService = "workingCopyFileService");

/// `IStoredFileWorkingCopySaveParticipantContext`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StoredFileWorkingCopySaveParticipantContext {
    /// The reason why the save was triggered.
    pub reason: SaveReason,
    /// Only applies to when a text file was saved as, for example when starting with untitled and saving. This provides
    /// access to the initial resource the text file had before.
    pub saved_from: Option<URI>,
}

//#endregion

//#region IEditorService.save

/// `ISaveEditorsOptions`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SaveEditorsOptions {
    /// O `ISaveOptions` de que herda.
    pub base: SaveOptions,
    /// If true, will ask for a location of the editor to save to.
    pub save_as: Option<bool>,
}

/// `ISaveEditorsResult`.
#[derive(Clone)]
pub struct SaveEditorsResult {
    /// Whether the save operation was successful.
    pub success: bool,
    /// Resulting editors after the save operation.
    pub editors: Vec<EditorInputOrUntyped>,
}

/// O recorte de `IEditorService` que a working copy de arquivo armazenado lê (`save`): o `IEditorService` reduzido de
/// `services/editor/editor_service.rs` ainda não tem o método, e acrescentá-lo lá quebraria os dublês que o implementam.
/// O `EditorService` completo implementa este trait, e o `save(editors: IEditorIdentifier | readonly IEditorIdentifier[])`
/// aqui recebe só um identificador (o único uso).
pub trait IEditorServiceSave {
    /// `save(editors, options?)`: saves the provided editor.
    fn save(&self, editor: EditorIdentifier, options: Option<SaveEditorsOptions>) -> Promise<SaveEditorsResult>;
}

//#endregion

//#region ICodeEditorService.getTransientModelProperties

/// O recorte de `ICodeEditorService` que o `TextFileService` lê no "Salvar como" (`getTransientModelProperties` e
/// `setTransientModelProperty`): o `ICodeEditorService` reduzido de `je_editor::opener_service` só traz o que o
/// `EditorOpener` usa, e acrescentar métodos lá quebraria quem o implementa. O serviço de editores de código completo
/// implementa este trait. O valor `any` da propriedade é um [`Dynamic`].
pub trait ICodeEditorTransientProperties {
    /// `getTransientModelProperties(model)`: as propriedades transitórias do modelo, `None` quando não há.
    fn get_transient_model_properties(&self, model: &TextModel) -> Option<Vec<(String, Dynamic)>>;

    /// `setTransientModelProperty(model, key, value)`.
    fn set_transient_model_property(&self, model: &TextModel, key: &str, value: Dynamic);
}

//#endregion
