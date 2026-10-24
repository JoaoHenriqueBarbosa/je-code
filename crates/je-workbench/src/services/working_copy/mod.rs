//! Porte de `src/vs/workbench/services/workingCopy/common`: as working copies.

pub mod abstract_file_working_copy_manager;
pub mod file_working_copy;
pub mod file_working_copy_manager;
pub mod native_working_copy_backup_service;
pub mod native_working_copy_backup_tracker;
pub mod resource_working_copy;
pub mod stored_file_working_copy;
pub mod stored_file_working_copy_manager;
pub mod stored_file_working_copy_save_participant;
pub mod untitled_file_working_copy;
pub mod untitled_file_working_copy_manager;
pub mod working_copy;
pub mod working_copy_backup;
pub mod working_copy_backup_service;
pub mod working_copy_backup_tracker;
pub mod working_copy_file_operation_participant;
pub mod working_copy_file_service;
pub mod working_copy_service;

// Histórico local das working copies (`workingCopyHistory.ts`, `workingCopyHistoryService.ts` e `workingCopyHistoryTracker.ts`).
pub mod working_copy_history;
pub mod working_copy_history_service;
pub mod working_copy_history_tracker;

// Contratos de `workingCopyEditorService.ts` (o handler que reabre o editor de uma working copy e o serviço que o registra).
pub mod working_copy_editor_service;
