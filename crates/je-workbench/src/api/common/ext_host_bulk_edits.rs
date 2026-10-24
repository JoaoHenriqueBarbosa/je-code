//! Porte de `src/vs/workbench/api/common/extHostBulkEdits.ts`: `workspace.applyEdit` do lado das extensões. Converte o
//! `vscode.WorkspaceEdit` no DTO do protocolo, com a versão de cada documento aberto, e o entrega à thread principal.
//!
//! Divergências:
//! - `vscode.WorkspaceEditMetadata` é o [`WorkspaceEditMetadata`] deste módulo (só tem `isRefactoring`), porque o
//!   `WorkspaceEditMetadata` do editor é o dos metadados de cada entrada, outro tipo.
//! - `getTextDocumentVersion: uri => extHostDocumentsAndEditors.getDocument(uri)?.version` é o
//!   [`DocumentsVersionInformationProvider`]; o `version` do documento (`number`) vira o `i64` do protocolo.
//!   `IExtHostRpcService` só serve para pedir o proxy (`MainThreadBulkEdits`) no construtor, como `IMainContext`.
//! - O `extension` do `applyWorkspaceEdit` não é usado no original.

use std::rc::Rc;

use je_base::promise::Promise;
use je_base::uri::URI;
use je_extensions::scanner::ExtensionDescription;

use super::ext_host_documents_and_editors::ExtHostDocumentsAndEditors;
use super::ext_host_protocol::{IMainContext, MainContext, MainThreadBulkEditsShape, SerializableObjectWithBuffers};
use super::ext_host_type_converters::{IVersionInformationProvider, WorkspaceEdit};
use super::ext_host_types as types;

/// `vscode.WorkspaceEditMetadata`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct WorkspaceEditMetadata {
    pub is_refactoring: Option<bool>,
}

/// O `_versionInformationProvider` do original: a versão do documento aberto, e nenhuma versão de notebook.
pub struct DocumentsVersionInformationProvider {
    documents_and_editors: Rc<ExtHostDocumentsAndEditors>,
}

impl IVersionInformationProvider for DocumentsVersionInformationProvider {
    fn get_text_document_version(&self, uri: &URI) -> Option<i64> {
        self.documents_and_editors.get_document(uri).map(|data| data.document().version() as i64)
    }

    fn get_notebook_document_version(&self, _uri: &URI) -> Option<i64> {
        None
    }
}

/// `ExtHostBulkEdits`.
pub struct ExtHostBulkEdits {
    proxy: Rc<dyn MainThreadBulkEditsShape>,
    version_information_provider: DocumentsVersionInformationProvider,
}

impl ExtHostBulkEdits {
    pub fn new(main_context: &dyn IMainContext, documents_and_editors: Rc<ExtHostDocumentsAndEditors>) -> Self {
        Self {
            proxy: main_context.get_proxy(&MainContext::MAIN_THREAD_BULK_EDITS),
            version_information_provider: DocumentsVersionInformationProvider { documents_and_editors },
        }
    }

    pub fn apply_workspace_edit(&self, edit: &types::WorkspaceEdit, _extension: &ExtensionDescription, metadata: Option<&WorkspaceEditMetadata>) -> Promise<bool> {
        let dto = SerializableObjectWithBuffers::new(WorkspaceEdit::from(edit, Some(&self.version_information_provider)));
        self.proxy.rpc_try_apply_workspace_edit(dto, None, Some(metadata.and_then(|metadata| metadata.is_refactoring).unwrap_or(false)))
    }
}
