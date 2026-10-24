//! Utilitários compartilhados pelos testes de integração do je-workbench.
#![allow(dead_code)]

pub mod js_literal;
pub mod keyboard_mapper_test_utils;

pub mod ext_host_workspace_support;
pub mod mock_command_service;
pub mod test_notebook_editor;
pub mod test_rpc_protocol;
pub mod test_stubs;
pub mod test_service_mocks;

// Porte de `storedFileWorkingCopy.test.ts`.
pub mod stored_file_working_copy_support;

// Porte de `keybindingEditing.test.ts`.
pub mod keybinding_editing_support;

// Porte de `terminalCompletionService.test.ts`.
pub mod terminal_completion_support;

// O emulador de terminal dos testes de `contrib/terminal` e `contrib/terminalContrib` (no lugar do `Terminal` do xterm).
pub mod fake_xterm;

// Porte de `terminalLinkOpeners.test.ts` e `terminalLocalLinkDetector.test.ts`: os dublês dos serviços.
pub mod terminal_link_support;

// Porte de `textFileEditorModelManager.test.ts`.
pub mod text_file_support;

// Porte de `untitledFileWorkingCopy.test.ts` e de `untitledScratchpadWorkingCopy.test.ts`.
pub mod untitled_file_working_copy_support;

// Porte de `workingCopyBackupTracker.test.ts`: o `createEditorPart` e o `EditorService` real.
pub mod editor_service_support;

// Porte de `extHostNotebook.test.ts`, `extHostNotebookSerializer.test.ts` e `extHostNotebookStatusCommands.test.ts`.
pub mod ext_host_notebook_support;

// Porte de `extHostSignatureHelp.test.ts`, `extHostWorkspaceSymbols.test.ts`, `extHostSemanticTokens.test.ts` e `extHostHierarchy.test.ts`.
pub mod ext_host_language_features_support;

// Porte de `extHostDebugAdapters.test.ts`, `extHostDebugTreeSessions.test.ts` e `extHostDebugVisualization.test.ts`.
pub mod ext_host_debug_support;

// Porte de `TestMainThreadNotebookKernels.ts`, usado por `mainThreadVariableProvider.test.ts`.
pub mod test_main_thread_notebook_kernels;

// Porte dos testes de `contrib/extensions` (`extension.test.ts` e seguintes): os serviços de uma `Extension`.
pub mod extension_services_support;
