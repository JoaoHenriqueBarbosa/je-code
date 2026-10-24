//! Contratos reduzidos dos serviços que o `TextFileEditorModel`, o `TextFileSaveParticipant` e o
//! `TextFileEditorModelManager` leem e que ainda não foram portados: `IProgressService`
//! (`platform/progress/common/progress.ts`), `IWorkingCopyBackupService` (`workingCopyBackup.ts`),
//! e `IWorkingCopyFileService` (`workingCopyFileService.ts`). O `ILanguageDetectionService` e o `IAccessibilityService`
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

use je_base::dynamic::Dynamic;
use je_base::event::Event;
use je_base::promise::Promise;
use je_base::uri::URI;
use je_platform::files::files::FileOperation;
use je_platform::notification::notification::NotificationPriority;
use je_platform::notification::notification_telemetry::NotificationTelemetryId;

use crate::services::working_copy::working_copy::IWorkingCopy;
use je_base::buffer::VSBufferReadableStream;

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

//#region IWorkingCopyBackupService

/// `IResolvedWorkingCopyBackup<T>`: o `meta` (um objeto) e o conteúdo do backup como stream.
#[derive(Clone)]
pub struct ResolvedWorkingCopyBackup {
    pub meta: Option<Dynamic>,
    pub value: VSBufferReadableStream,
}

/// `IWorkingCopyBackupService`.
pub trait IWorkingCopyBackupService {
    /// Resolves the backup for the given working copy if any.
    fn resolve(&self, working_copy: &dyn IWorkingCopy) -> Promise<Option<ResolvedWorkingCopyBackup>>;
}

je_instantiation::service_id!(pub I_WORKING_COPY_BACKUP_SERVICE: dyn IWorkingCopyBackupService = "workingCopyBackupService");

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

/// `IWorkingCopyFileService` (os três eventos que o gerenciador de modelos de arquivo de texto escuta).
pub trait IWorkingCopyFileService {
    /// An event that is fired before a working copy file operation is about to run.
    fn on_will_run_working_copy_file_operation(&self) -> Event<WorkingCopyFileEvent>;

    /// An event that is fired after a working copy file operation has failed.
    fn on_did_fail_working_copy_file_operation(&self) -> Event<WorkingCopyFileEvent>;

    /// An event that is fired after a working copy file operation has been performed.
    fn on_did_run_working_copy_file_operation(&self) -> Event<WorkingCopyFileEvent>;
}

je_instantiation::service_id!(pub I_WORKING_COPY_FILE_SERVICE: dyn IWorkingCopyFileService = "workingCopyFileService");

//#endregion
