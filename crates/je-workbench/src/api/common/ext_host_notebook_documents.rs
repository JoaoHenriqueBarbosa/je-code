//! Porte de `src/vs/workbench/api/common/extHostNotebookDocuments.ts`: os eventos de mudança, de estado sujo e de gravação
//! dos documentos de notebook (`vscode.workspace.onDidChangeNotebookDocument`, `onDidSaveNotebookDocument`).
//!
//! Divergências:
//! - `ExtHostNotebookDocuments` é um `Rc` ([`ExtHostNotebookDocuments::new`]) que guarda o controlador por `Rc`. O `URI.revive`
//!   do original já está feito no protocolo, e `newMetadata?` é `Option<Dynamic>`.
//! - O `getNotebookDocument(uri)` que lança é [`ExtHostNotebookController::get_notebook_document`].

use std::rc::Rc;

use je_base::dynamic::Dynamic;
use je_base::event::{Emitter, Event};
use je_base::uri::URI;

use super::ext_host_notebook::ExtHostNotebookController;
use super::ext_host_notebook_document::{NotebookDocument, NotebookDocumentChangeEvent};
use super::ext_host_protocol::{ExtHostNotebookDocumentsShape, NotebookCellsChangedEventDto, SerializableObjectWithBuffers};

/// `ExtHostNotebookDocuments`.
pub struct ExtHostNotebookDocuments {
    on_did_save_notebook_document: Rc<Emitter<NotebookDocument>>,
    on_did_change_notebook_document: Rc<Emitter<NotebookDocumentChangeEvent>>,
    notebooks_and_editors: Rc<ExtHostNotebookController>,
}

impl ExtHostNotebookDocuments {
    pub fn new(notebooks_and_editors: Rc<ExtHostNotebookController>) -> Rc<ExtHostNotebookDocuments> {
        Rc::new(ExtHostNotebookDocuments {
            on_did_save_notebook_document: Emitter::new(),
            on_did_change_notebook_document: Emitter::new(),
            notebooks_and_editors,
        })
    }

    /// `onDidSaveNotebookDocument`.
    pub fn on_did_save_notebook_document(&self) -> Event<NotebookDocument> {
        self.on_did_save_notebook_document.event()
    }

    /// `onDidChangeNotebookDocument`.
    pub fn on_did_change_notebook_document(&self) -> Event<NotebookDocumentChangeEvent> {
        self.on_did_change_notebook_document.event()
    }
}

impl ExtHostNotebookDocumentsShape for ExtHostNotebookDocuments {
    fn rpc_accept_model_changed(&self, uri: &URI, event: SerializableObjectWithBuffers<NotebookCellsChangedEventDto>, is_dirty: bool, new_metadata: Option<Dynamic>) {
        let document = self.notebooks_and_editors.get_notebook_document(uri);
        let e = document.accept_model_changed(event.value, is_dirty, new_metadata);
        self.on_did_change_notebook_document.fire(e);
    }

    fn rpc_accept_dirty_state_changed(&self, uri: &URI, is_dirty: bool) {
        let document = self.notebooks_and_editors.get_notebook_document(uri);
        document.accept_dirty(is_dirty);
    }

    fn rpc_accept_model_saved(&self, uri: &URI) {
        let document = self.notebooks_and_editors.get_notebook_document(uri);
        self.on_did_save_notebook_document.fire(document.api_notebook());
    }
}
