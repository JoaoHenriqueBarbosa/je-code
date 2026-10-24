//! Porte parcial de `src/vs/workbench/contrib/notebook/browser/services/notebookEditorService.ts`: o contrato
//! `INotebookEditorService` com os membros que a árvore de resultados de busca lê (`onDidAddNotebookEditor` e
//! `retrieveExistingWidgetFromURI`) e os que o lado do workbench do host de extensões lê (`onDidRemoveNotebookEditor`,
//! `listNotebookEditors` e `getNotebookEditor`, com implementação padrão de serviço sem editores).
//!
//! Divergências: `IBorrowValue<INotebookEditor>` é [`IBorrowValue`] com `value: Option<Rc<dyn NotebookEditorWidget>>`. Os
//! editores que o serviço entrega são widgets; o `INotebookEditor` de cada um vem de
//! [`NotebookEditorWidget::as_notebook_editor`].

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

    // Os membros abaixo completam o contrato para o lado do workbench do host de extensões. Têm implementação padrão para
    // não quebrar quem implementa só o recorte acima: o serviço sem editores.

    /// `onDidRemoveNotebookEditor`.
    fn on_did_remove_notebook_editor(&self) -> Event<Rc<dyn NotebookEditorWidget>> {
        Event::none()
    }

    /// `listNotebookEditors()`.
    fn list_notebook_editors(&self) -> Vec<Rc<dyn NotebookEditorWidget>> {
        Vec::new()
    }

    /// `getNotebookEditor(editorId)`.
    fn get_notebook_editor(&self, _editor_id: &str) -> Option<Rc<dyn NotebookEditorWidget>> {
        None
    }
}

service_id!(pub I_NOTEBOOK_EDITOR_SERVICE: dyn INotebookEditorService = "INotebookEditorWidgetService");
