//! Porte de `src/vs/workbench/services/workingCopy/common/workingCopy.ts`: o contrato de uma working copy
//! (`IWorkingCopy`), as capacidades, o backup e o evento de salvar.
//!
//! Divergências:
//! - **Propriedades** viram métodos: `typeId` é [`IWorkingCopy::working_copy_type_id`] (o nome `type_id` colidiria com o
//!   `Any::type_id` quando o `Any` está no escopo), `resource` é [`IWorkingCopy::resource`], `name`,
//!   `capabilities` e `backupDelay?` também; os eventos devolvem o [`Event`].
//! - `IWorkingCopyIdentifier` é a struct [`WorkingCopyIdentifier`] e a working copy a expõe por
//!   [`IWorkingCopy::working_copy_type_id`] e [`IWorkingCopy::resource`].
//! - `WorkingCopyCapabilities` (um `const enum` de bits) é [`WorkingCopyCapabilities`], um número com as constantes.
//! - `IWorkingCopyBackup.content` (`VSBufferReadable | VSBufferReadableStream`) é o enum [`WorkingCopyBackupContent`],
//!   com o leitor em `Rc<RefCell<...>>` para a promessa do backup poder ser copiada.
//! - `IWorkingCopyBackupMeta` (`{ [key: string]: unknown }`) é um [`Dynamic`] objeto.

use std::cell::RefCell;
use std::rc::Rc;

use je_base::buffer::{VSBuffer, VSBufferReadableStream};
use je_base::cancellation::CancellationToken;
use je_base::dynamic::Dynamic;
use je_base::event::Event;
use je_base::promise::Promise;
use je_base::stream::Readable;
use je_base::uri::URI;

use crate::common::editor::{RevertOptions, SaveOptions, SaveReason, SaveSource};

/// `WorkingCopyCapabilities`: máscara de bits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WorkingCopyCapabilities(pub u32);

impl WorkingCopyCapabilities {
    /// Signals no specific capability for the working copy.
    pub const NONE: WorkingCopyCapabilities = WorkingCopyCapabilities(0);
    /// Signals that the working copy requires additional input when saving, e.g. an associated path to save to.
    pub const UNTITLED: WorkingCopyCapabilities = WorkingCopyCapabilities(1 << 1);
    /// The working copy will not indicate that it is dirty and unsaved content will be discarded without prompting if closed.
    pub const SCRATCHPAD: WorkingCopyCapabilities = WorkingCopyCapabilities(1 << 2);

    /// `(capabilities & capability) !== 0`.
    pub fn contains(self, capability: WorkingCopyCapabilities) -> bool {
        self.0 & capability.0 != 0
    }
}

/// `VSBufferReadable | VSBufferReadableStream`.
#[derive(Clone)]
pub enum WorkingCopyBackupContent {
    Readable(Rc<RefCell<Box<dyn Readable<VSBuffer>>>>),
    Stream(VSBufferReadableStream),
}

/// `IWorkingCopyBackup`: dados associados ao backup de uma working copy. Use
/// `IWorkingCopyBackupService.resolve(workingCopy)` para recuperar o backup ao carregar a working copy.
#[derive(Clone, Default)]
pub struct WorkingCopyBackup {
    /// Any serializable metadata to be associated with the backup (`IWorkingCopyBackupMeta`).
    pub meta: Option<Dynamic>,
    /// The actual snapshot of the contents of the working copy at the time the backup was made.
    pub content: Option<WorkingCopyBackupContent>,
}

/// `NO_TYPE_ID` (deprecated: it is important to provide a type identifier for working copies to enable all capabilities).
pub const NO_TYPE_ID: &str = "";

/// `IWorkingCopyIdentifier`: toda working copy é identificada por um recurso e um `typeId`; só pode haver uma working
/// copy registrada com o mesmo `URI` e `typeId`.
#[derive(Clone, Debug, PartialEq)]
pub struct WorkingCopyIdentifier {
    /// The type identifier of the working copy for grouping working copies of the same domain together.
    pub type_id: String,
    /// The resource of the working copy must be unique for working copies of the same `typeId`.
    pub resource: URI,
}

/// `IWorkingCopySaveEvent`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct WorkingCopySaveEvent {
    /// The reason why the working copy was saved.
    pub reason: Option<SaveReason>,
    /// The source of the working copy save request.
    pub source: Option<SaveSource>,
}

/// `IWorkingCopy`: um conceito abstrato para unificar o tratamento de dados que podem ser trabalhados (editados) em
/// um editor. O recurso pode ser o armazenamento dos dados (um arquivo em disco), mas não precisa ser.
pub trait IWorkingCopy {
    /// `typeId` (de `IWorkingCopyIdentifier`).
    fn working_copy_type_id(&self) -> String;

    /// `resource` (de `IWorkingCopyIdentifier`).
    fn resource(&self) -> URI;

    /// Human readable name of the working copy.
    fn name(&self) -> String;

    /// The capabilities of the working copy.
    fn capabilities(&self) -> WorkingCopyCapabilities;

    /// Used by the workbench to signal if the working copy is dirty or not. Typically a working copy is dirty once
    /// changed until saved or reverted.
    fn on_did_change_dirty(&self) -> Event<()>;

    /// Used by the workbench e.g. to trigger auto-save (unless this working copy is untitled) and backups.
    fn on_did_change_content(&self) -> Event<()>;

    /// Used by the workbench e.g. to track local history (unless this working copy is untitled).
    fn on_did_save(&self) -> Event<WorkingCopySaveEvent>;

    /// Indicates that the file has unsaved changes and should confirm before closing.
    fn is_dirty(&self) -> bool;

    /// Indicates that the file has unsaved changes. Used for backup tracking and accounts for working copies that are
    /// never dirty e.g. scratchpads.
    fn is_modified(&self) -> bool;

    /// The delay in milliseconds to wait before triggering a backup after the content of the model has changed. If not
    /// configured, a sensible default will be taken based on user settings.
    fn backup_delay(&self) -> Option<u64> {
        None
    }

    /// The workbench may call this method often after it receives the `onDidChangeContent` event for the working copy.
    /// The motivation is to allow to quit VSCode with dirty working copies present.
    fn backup(&self, token: &CancellationToken) -> Promise<WorkingCopyBackup>;

    /// Asks the working copy to save. If the working copy was dirty, it is expected to be non-dirty after this
    /// operation has finished. Resolves to `true` if the operation was successful and `false` otherwise.
    fn save(&self, options: Option<SaveOptions>) -> Promise<bool>;

    /// Asks the working copy to revert. If the working copy was dirty, it is expected to be non-dirty after this
    /// operation has finished.
    fn revert(&self, options: Option<RevertOptions>) -> Promise<()>;
}
