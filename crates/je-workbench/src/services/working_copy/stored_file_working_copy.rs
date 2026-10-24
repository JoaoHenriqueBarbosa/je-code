//! Porte parcial de `src/vs/workbench/services/workingCopy/common/storedFileWorkingCopy.ts`: só o evento de salvar
//! (`IStoredFileWorkingCopySaveEvent`) que a `TestWorkingCopy` dispara. O resto do arquivo (a `StoredFileWorkingCopy` e
//! o modelo) entra com o porte do serviço de working copy.
//!
//! Divergência: `IStoredFileWorkingCopySaveEvent extends IWorkingCopySaveEvent` (herança de interface) é a struct
//! [`StoredFileWorkingCopySaveEvent`], que guarda o [`WorkingCopySaveEvent`] base e acrescenta o `stat`. O
//! `isStoredFileWorkingCopySaveEvent` (que olha se existe `stat`) não existe: o tipo já diz qual é.

use je_platform::files::files::FileStatWithMetadata;

use super::working_copy::WorkingCopySaveEvent;

/// `IStoredFileWorkingCopySaveEvent`.
#[derive(Clone, Debug)]
pub struct StoredFileWorkingCopySaveEvent {
    /// O `reason` e o `source` herdados de `IWorkingCopySaveEvent`.
    pub event: WorkingCopySaveEvent,
    /// The resolved stat from the save operation.
    pub stat: FileStatWithMetadata,
}
