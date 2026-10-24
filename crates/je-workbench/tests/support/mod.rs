//! Utilitários compartilhados pelos testes de integração do je-workbench.
#![allow(dead_code)]

pub mod js_literal;
pub mod keyboard_mapper_test_utils;

pub mod ext_host_workspace_support;
pub mod mock_command_service;
pub mod notebook_outline_support;
pub mod notebook_services_support;
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

// Os dublês parciais das contribuições do terminal (`ITerminalInstance`, `ITerminalService` e companhia).
pub mod terminal_contrib_support;

// Porte de `terminalInstance.test.ts`: os dublês do processo, do serviço de instâncias e do resolvedor de perfil.
pub mod terminal_instance_support;

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

// Porte de `extensionEnablementService.test.ts`: o `TestExtensionEnablementService` e os dublês que outros testes reaproveitam.
pub mod extension_enablement_support;

// Porte de `extensionsWorkbenchService.test.ts`: os serviços do `setup` da suíte `ExtensionsWorkbenchServiceTest`.
pub mod extensions_workbench_service_support;

// Porte de `extensionsActions.test.ts`: o `setupTest` e os servidores de gerenciamento do teste.
pub mod extensions_actions_support;
// Porte de `extensionRecommendationsService.test.ts`: o `setup` da suíte `ExtensionRecommendationsService Test` (usa os
// suportes `extension_enablement_support` e `extensions_workbench_service_support`).
pub mod extension_recommendations_support;

// Porte de `contrib/debug/test/common/mockDebug.ts` e `test/browser/mockDebugModel.ts`: `MockSession`, `MockRawSession`,
// `MockDebugAdapter`, `createMockDebugModel` e o `createTestSession` de `callStack.test.ts`.
pub mod mock_debug;

// Porte de `baseDebugView.test.ts`, `watchExpressionView.test.ts` e `variablesView.test.ts`: o `LinkDetector` e o
// `DebugExpressionRenderer` de teste, o `IDebugService` com o `getViewModel` trocado, o escopo e a variável do `setup`.
pub mod debug_view_support;

// Porte de `editorAutoSave.test.ts` e `textFileEditorTracker.test.ts`: o `createEditorAutoSave`/`createTracker` comum e o `TestHostService` com foco.
pub mod editor_auto_save_support;
// Porte de `workbench/services/configuration/test/common/configurationModels.test.ts`: literais, nós de registro e `toConfigurationModel`.
pub mod configuration_models_support;

// Porte de `workbench/services/textfile/test/browser/textEditorService.test.ts`: o `NullFileSystemProvider` e o `FileServiceProvider`.
pub mod text_editor_service_support;
// Porte de `terminalProcessManager.test.ts`: o processo, o serviço de instâncias e o `workbenchInstantiationService` do gerenciador.
pub mod terminal_process_manager_support;

// Porte de `lineDataEventAddon.test.ts`, `markNavigationAddon.test.ts` e `decorationAddon.test.ts`: a decoração e o elemento do
// `registerDecoration` do `TestTerminal`, e os dublês do `DecorationAddon`.
pub mod fake_xterm_decoration;
pub mod terminal_decoration_support;

// Porte de `TestEditorGroupView` (`workbenchTestServices.ts`), usado por `openEditorsView.test.ts`.
pub mod test_editor_group_view;
// Porte de `searchResult.test.ts` e `searchViewlet.test.ts`: o `setup` e os `aSearchResult`, `aFileMatch` e `aFolderMatch`.
pub mod search_support;

// Porte de `openWithEditorPicker.test.ts`: o `TestQuickPick` e o `IQuickInputService` parcial.
pub mod open_with_editor_picker_support;
