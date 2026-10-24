//! Porte parcial de `src/vs/workbench/services/workingCopy/common/fileWorkingCopy.ts`: só o `SnapshotContext` de que
//! o `INotebookSnapshotOptions` de `notebookCommon.ts` depende. O resto do arquivo (`IFileWorkingCopyModel`,
//! `IFileWorkingCopyModelFactory`, `IFileWorkingCopy` e os eventos) fica para a fatia das working copies de arquivo.

/// `SnapshotContext`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SnapshotContext {
    Save = 1,
    Backup = 2,
}
