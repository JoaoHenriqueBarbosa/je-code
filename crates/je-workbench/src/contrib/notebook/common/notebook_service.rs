//! Porte parcial de `src/vs/workbench/contrib/notebook/common/notebookService.ts`: só o `INotebookService.getNotebookTextModel`
//! que o `MainThreadNotebookKernels` chama. O resto do serviço (os provedores de serializador, os controladores, os renderizadores
//! e o ciclo de vida dos modelos) fica para a fatia do serviço de notebooks.

use std::rc::Rc;

use je_base::uri::URI;

use super::notebook_common::INotebookTextModel;

/// `INotebookService`, só o `getNotebookTextModel`.
pub trait INotebookService {
    /// `getNotebookTextModel(uri)`.
    fn get_notebook_text_model(&self, uri: &URI) -> Option<Rc<dyn INotebookTextModel>>;
}
