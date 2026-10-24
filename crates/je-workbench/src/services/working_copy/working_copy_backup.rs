//! Porte de `src/vs/workbench/services/workingCopy/common/workingCopyBackup.ts`: o contrato do serviço de backup das
//! working copies (`IWorkingCopyBackupService`) e o backup resolvido (`IResolvedWorkingCopyBackup`).
//!
//! Este contrato substitui o `IWorkingCopyBackupService` reduzido que vivia em `textfile/collaborators.rs`.
//!
//! Divergências:
//! - **Genéricos.** `IResolvedWorkingCopyBackup<T extends IWorkingCopyBackupMeta>` e `resolve<T>` não são genéricos aqui:
//!   o `IWorkingCopyBackupMeta` (`{ [key: string]: unknown }`) é um [`Dynamic`] objeto, e quem lê o `meta` conhece as
//!   chaves (como o `TextFileEditorModel` com o `IBackupMetaData`).
//! - **Identificador.** Uma working copy, que no original é um `IWorkingCopyIdentifier` por tipagem estrutural, vira o
//!   identificador com [`WorkingCopyIdentifier::of`].
//! - **`hasBackupSync`.** O parâmetro `meta` existe só na classe `WorkingCopyBackupService` do original (a interface
//!   declara menos parâmetros, o que o TypeScript aceita). Rust não tem essa compatibilidade de assinatura, então o
//!   trait leva os três parâmetros.
//! - **Conteúdo.** `VSBufferReadable | VSBufferReadableStream` é o [`WorkingCopyBackupContent`] de `working_copy.rs`.
//! - **Filtro.** `{ except: IWorkingCopyIdentifier[] }` é a struct [`DiscardBackupsFilter`].

use je_base::buffer::VSBufferReadableStream;
use je_base::cancellation::CancellationToken;
use je_base::dynamic::Dynamic;
use je_base::promise::Promise;
use je_instantiation::service_id;

use super::working_copy::{WorkingCopyBackupContent, WorkingCopyIdentifier};

/// `IResolvedWorkingCopyBackup<T>`: a resolved working copy backup carries the backup value as well as associated
/// metadata with it.
#[derive(Clone)]
pub struct ResolvedWorkingCopyBackup {
    /// Additional metadata that is associated with the working copy backup (`meta?: T`).
    pub meta: Option<Dynamic>,

    /// The content of the working copy backup.
    pub value: VSBufferReadableStream,
}

/// O filtro de `discardBackups(filter?: { except: IWorkingCopyIdentifier[] })`.
#[derive(Clone, Debug, Default)]
pub struct DiscardBackupsFilter {
    pub except: Vec<WorkingCopyIdentifier>,
}

/// `IWorkingCopyBackupService`: the working copy backup service is the main service to handle backups for working
/// copies. Methods allow to persist and resolve working copy backups from the file system.
pub trait IWorkingCopyBackupService {
    /// Finds out if a working copy backup with the given identifier and optional version exists.
    ///
    /// Note: if the backup service has not been initialized yet, this may return the wrong result. Always use
    /// `resolve()` if you can do a long running operation.
    fn has_backup_sync(&self, identifier: &WorkingCopyIdentifier, version_id: Option<u64>, meta: Option<&Dynamic>) -> bool;

    /// Gets a list of working copy backups for the current workspace.
    fn get_backups(&self) -> Promise<Vec<WorkingCopyIdentifier>>;

    /// Resolves the working copy backup for the given identifier if that exists.
    fn resolve(&self, identifier: &WorkingCopyIdentifier) -> Promise<Option<ResolvedWorkingCopyBackup>>;

    /// Stores a new working copy backup for the given identifier.
    fn backup(
        &self,
        identifier: &WorkingCopyIdentifier,
        content: Option<WorkingCopyBackupContent>,
        version_id: Option<u64>,
        meta: Option<Dynamic>,
        token: Option<CancellationToken>,
    ) -> Promise<()>;

    /// Discards the working copy backup associated with the identifier if it exists.
    fn discard_backup(&self, identifier: &WorkingCopyIdentifier, token: Option<CancellationToken>) -> Promise<()>;

    /// Discards all working copy backups.
    ///
    /// The optional set of identifiers in the filter can be provided to discard all but the provided ones.
    fn discard_backups(&self, filter: Option<DiscardBackupsFilter>) -> Promise<()>;
}

service_id!(pub I_WORKING_COPY_BACKUP_SERVICE: dyn IWorkingCopyBackupService = "workingCopyBackupService");
