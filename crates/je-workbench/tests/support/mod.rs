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
