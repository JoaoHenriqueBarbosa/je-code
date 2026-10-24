//! Porte de `src/vs/workbench/contrib/notebook/common/notebookLoggingService.ts`: o contrato do serviço de log
//! dos notebooks (`INotebookLoggingService`). O serviço em si (o canal de saída) fica para a fatia dos serviços.

use je_instantiation::service_id;

/// `INotebookLoggingService`.
pub trait INotebookLoggingService {
    fn info(&self, category: &str, output: &str);
    fn warn(&self, category: &str, output: &str);
    fn error(&self, category: &str, output: &str);
    fn debug(&self, category: &str, output: &str);
    fn trace(&self, category: &str, output: &str);
}

service_id!(pub I_NOTEBOOK_LOGGING_SERVICE: dyn INotebookLoggingService = "INotebookLoggingService");
