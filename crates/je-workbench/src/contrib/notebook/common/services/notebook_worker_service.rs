//! Porte de `src/vs/workbench/contrib/notebook/common/services/notebookWorkerService.ts`: o contrato do serviço que calcula o
//! diff de dois notebooks no worker (`INotebookEditorWorkerService`). A implementação (`NotebookEditorWorkerServiceImpl`,
//! que fala com o worker por IPC) não é portada; o cálculo do worker é o [`NotebookWorker`](super::notebook_web_worker::NotebookWorker).

use je_base::promise::Promise;
use je_base::uri::URI;
use je_instantiation::service_id;

use crate::contrib::notebook::common::notebook_common::INotebookDiffResult;

/// `INotebookEditorWorkerService`.
pub trait INotebookEditorWorkerService {
    fn can_compute_diff(&self, original: &URI, modified: &URI) -> bool;
    fn compute_diff(&self, original: &URI, modified: &URI) -> Promise<INotebookDiffResult>;
    fn can_prompt_recommendation(&self, model: &URI) -> Promise<bool>;
}

service_id!(pub I_NOTEBOOK_EDITOR_WORKER_SERVICE: dyn INotebookEditorWorkerService = "notebookEditorWorkerService");
