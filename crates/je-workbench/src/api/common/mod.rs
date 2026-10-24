//! Porte parcial de `src/vs/workbench/api/common`.

pub mod ext_host_authentication;
pub mod ext_host_xaa_auth_provider;
pub mod ext_host_browsers;
pub mod ext_host_protocol;
// As formas tipadas do protocolo sobre o `RpcProtocol` (proxy e ator de cada serviço do host de extensões).
pub mod ext_host_rpc_shapes;
pub mod ext_host_text_editor;
pub mod ext_host_type_converters;
pub mod ext_host_types;

pub mod ext_host_document_content_providers;
pub mod ext_host_document_data;
pub mod ext_host_document_save_participant;
pub mod ext_host_documents;
pub mod ext_host_documents_and_editors;
pub mod ext_host_commands;
pub mod ext_host_comments;
pub mod ext_host_scm;
pub mod ext_host_values;

pub mod ext_host_configuration;
pub mod ext_host_progress;
pub mod ext_host_tree_views;
pub mod ext_host_workspace;
pub mod shared;

pub mod cache;
pub mod ext_host_custom_editors;
pub mod ext_host_file_system_event_service;
pub mod ext_host_file_system_info;
pub mod ext_host_editor_tabs;
pub mod ext_host_telemetry;
pub mod ext_host_status_bar;
pub mod ext_host_diagnostics;
pub mod ext_host_timeline;

// Webviews: `extHostWebview.ts`, `extHostWebviewPanels.ts`, `extHostWebviewView.ts` e `extHostWebviewMessaging.ts`.
pub mod ext_host_webview;
pub mod ext_host_webview_messaging;
pub mod ext_host_webview_panels;
pub mod ext_host_webview_view;

// LanguageFeatures: `extHostLanguageFeatures.ts` (parte 1), a superfície de provedores da API e os conversores novos.
pub mod ext_host_language_features;
pub mod ext_host_language_features_api;
pub mod ext_host_language_features_converters;

// Testing: `extHostTesting.ts`, `extHostTestItem.ts` e os tipos e conversores de teste.
pub mod ext_host_test_item;
pub mod ext_host_testing;
pub mod ext_host_testing_converters;
pub mod ext_host_testing_types;

// Notebook: `extHostNotebook.ts` (o controlador), o documento, o editor e `extHostNotebookDocuments.ts`.
pub mod ext_host_notebook;
pub mod ext_host_notebook_document;
pub mod ext_host_notebook_documents;
pub mod ext_host_notebook_editor;
// ApiCommands: `extHostApiCommands.ts` e os comandos internos `_execute*` do editor que ele chama.
pub mod editor_execute_commands;
pub mod ext_host_api_commands;
pub mod hierarchy_execute_commands;
pub mod ext_host_notebook_kernels;

// Notebook (restante do host): `extHostNotebookEditors.ts`, `extHostNotebookRenderers.ts`,
// `extHostNotebookDocumentSaveParticipant.ts` e o `_registerApiCommands` de `extHostNotebook.ts`.
pub mod ext_host_notebook_api_commands;
pub mod ext_host_notebook_document_save_participant;
pub mod ext_host_notebook_editors;
pub mod ext_host_notebook_renderers;

// BulkEdits e Decorations: `extHostBulkEdits.ts` e `extHostDecorations.ts`.
pub mod ext_host_bulk_edits;
pub mod ext_host_decorations;
// Ativação de extensões: `extHostExtensionActivator.ts` e `extensionHostMain.ts` (o tratador de erros do host).
pub mod ext_host_extension_activator;
pub mod extension_host_main;

// Depuração: `extHostDebugService.ts` e a região de debug de `extHost.protocol.ts`.
pub mod ext_host_debug_protocol;
pub mod ext_host_debug_service;
pub mod ext_host_debug_types;
// `extHost.api.impl.ts`: só `getTerminalInternalOptions`.
pub mod ext_host_api_impl;
// `extHostTerminalShellIntegration.ts`: só o `InternalTerminalShellIntegration` e os tipos `vscode.*` que ele produz.
pub mod ext_host_terminal_service;
pub mod ext_host_terminal_shell_integration;
// Openers externos de URI: `extHostUriOpener.ts`.
pub mod ext_host_uri_opener;
