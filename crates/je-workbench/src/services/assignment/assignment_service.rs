//! Porte parcial de `src/vs/workbench/services/assignment/common/assignmentService.ts`: só o contrato
//! `IAssignmentFilter`, que o `AssignmentContextFilter` e os filtros registrados no serviço compartilham.
//!
//! Divergências:
//! - **Fora do porte**: o `WorkbenchAssignmentService` (cliente TAS, `tas-client-umd`, telemetria de atribuição,
//!   memento de armazenamento), o `IWorkbenchAssignmentService` e o registro das configurações
//!   `workbench.enableExperiments`; dependem do cliente TAS e do `IAssignmentService` da plataforma, ainda não portados.
//! - **`IAssignmentFilter`** é um trait: `id` e `exclude(assignment)` são métodos, e `onDidChange` é
//!   [`IAssignmentFilter::on_did_change`].

use je_base::event::Event;

/// `IAssignmentFilter`.
pub trait IAssignmentFilter {
    /// Stable identifier for this filter. Used to persist and reconcile the set of assignment-context ids this filter
    /// has excluded, independently of other filters.
    fn id(&self) -> &str;

    fn exclude(&self, assignment: &str) -> bool;

    fn on_did_change(&self) -> Event<()>;
}
