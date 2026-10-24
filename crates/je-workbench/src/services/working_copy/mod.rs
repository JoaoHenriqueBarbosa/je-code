//! Porte de `src/vs/workbench/services/workingCopy/common`: as working copies.

pub mod file_working_copy;
pub mod native_working_copy_backup_service;
pub mod resource_working_copy;
pub mod stored_file_working_copy;
pub mod working_copy;
pub mod working_copy_backup;
pub mod working_copy_backup_service;
pub mod working_copy_service;

// Contratos de `workingCopyEditorService.ts` (o handler que reabre o editor de uma working copy e o serviço que o registra).
pub mod working_copy_editor_service;
