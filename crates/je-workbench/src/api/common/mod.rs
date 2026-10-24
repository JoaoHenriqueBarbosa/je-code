//! Porte parcial de `src/vs/workbench/api/common`.

pub mod ext_host_browsers;
pub mod ext_host_protocol;
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
pub mod ext_host_values;

pub mod ext_host_configuration;
pub mod ext_host_progress;
pub mod ext_host_tree_views;
pub mod ext_host_workspace;

pub mod cache;
pub mod ext_host_custom_editors;
pub mod ext_host_file_system_event_service;
pub mod ext_host_file_system_info;
pub mod ext_host_editor_tabs;
pub mod ext_host_telemetry;
pub mod ext_host_status_bar;
pub mod ext_host_diagnostics;

// Testing: `extHostTesting.ts`, `extHostTestItem.ts` e os tipos e conversores de teste.
pub mod ext_host_test_item;
pub mod ext_host_testing;
pub mod ext_host_testing_converters;
pub mod ext_host_testing_types;
