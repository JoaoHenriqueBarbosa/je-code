//! Porte parcial de `src/vs/workbench/contrib/notebook/browser/services/notebookEditorService.ts`: o contrato
//! `INotebookEditorService` com os membros que a árvore de resultados de busca lê (`onDidAddNotebookEditor` e
//! `retrieveExistingWidgetFromURI`).
//!
//! Divergências: `IBorrowValue<INotebookEditor>` é [`IBorrowValue`] com `value: Option<Rc<dyn NotebookEditorWidget>>`.

use std::rc::Rc;

use je_base::event::Event;
use je_base::uri::URI;
use je_instantiation::service_id;

use crate::contrib::notebook::browser::notebook_editor_widget::NotebookEditorWidget;

/// `IBorrowValue<T>`.
pub struct IBorrowValue<T: ?Sized> {
    pub value: Option<Rc<T>>,
}

/// `INotebookEditorService`.
pub trait INotebookEditorService {
    fn on_did_add_notebook_editor(&self) -> Event<Rc<dyn NotebookEditorWidget>>;
    fn retrieve_existing_widget_from_uri(&self, resource: &URI) -> Option<IBorrowValue<dyn NotebookEditorWidget>>;
}

service_id!(pub I_NOTEBOOK_EDITOR_SERVICE: dyn INotebookEditorService = "INotebookEditorWidgetService");
