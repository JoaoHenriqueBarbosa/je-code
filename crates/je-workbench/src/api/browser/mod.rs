//! Porte de `src/vs/workbench/api/browser`: a camada `MainThreadXxx`, o lado do workbench do RPC com o host de
//! extensões (cada classe implementa o `MainThreadXxxShape` do protocolo e fala com o `ExtHostXxxShape` pelo proxy).

pub mod main_thread_commands;
pub mod main_thread_configuration;
pub mod main_thread_custom_editors;
pub mod main_thread_custom_text_editor_navigation;
pub mod main_thread_documents;
pub mod main_thread_documents_and_editors;
pub mod main_thread_editor;
pub mod main_thread_editors;
pub mod main_thread_language_features;
pub mod main_thread_managed_sockets;
pub mod main_thread_progress;
pub mod main_thread_status_bar;
pub mod main_thread_timeline;
pub mod main_thread_quick_open;
pub mod main_thread_workspace;
pub mod main_thread_message_service;

// Diagnostics, DocumentContentProviders, EditorTabs, Telemetry e Authentication (parcial): `mainThreadDiagnostics.ts`,
// `mainThreadDocumentContentProviders.ts`, `mainThreadEditorTabs.ts`, `mainThreadTelemetry.ts`, `mainThreadAuthentication.ts`.
pub mod main_thread_authentication;
pub mod main_thread_diagnostics;
pub mod main_thread_document_content_providers;
pub mod main_thread_editor_tabs;
pub mod main_thread_telemetry;

// Webviews: `mainThreadWebviews.ts` e `mainThreadWebviewPanels.ts`.
pub mod main_thread_webview_panels;
pub mod main_thread_webviews;

// Edições em massa: `mainThreadBulkEdits.ts`.
pub mod main_thread_bulk_edits;

// Notebooks: `mainThreadNotebook.ts`, `mainThreadNotebookDocuments.ts`, `mainThreadNotebookEditors.ts`,
// `mainThreadNotebookDocumentsAndEditors.ts` e `mainThreadNotebookDto.ts`.
pub mod main_thread_notebook;
pub mod main_thread_notebook_documents;
pub mod main_thread_notebook_documents_and_editors;
pub mod main_thread_notebook_dto;
pub mod main_thread_notebook_editors;
