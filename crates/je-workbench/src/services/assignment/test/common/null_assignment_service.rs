//! Porte de `src/vs/workbench/services/assignment/test/common/nullAssignmentService.ts`: o serviço de atribuição que não
//! atribui nada, para os testes que só precisam de um `IWorkbenchAssignmentService`.

use std::rc::Rc;

use je_base::async_::async_fn;
use je_base::event::Event;
use je_base::promise::Promise;
use je_platform::assignment::{IAssignmentService, TreatmentValue};

use crate::services::assignment::assignment_service::{IAssignmentFilter, ITreatmentWithAssignment, IWorkbenchAssignmentService};

/// `NullWorkbenchAssignmentService`.
#[derive(Default)]
pub struct NullWorkbenchAssignmentService;

impl IAssignmentService for NullWorkbenchAssignmentService {
    /// `onDidRefetchAssignments: Event<void> = Event.None`.
    fn on_did_refetch_assignments(&self) -> Event<()> {
        Event::none()
    }

    fn get_treatment(&self, _name: &str) -> Promise<Option<TreatmentValue>> {
        async_fn(async { Ok(None) })
    }
}

impl IWorkbenchAssignmentService for NullWorkbenchAssignmentService {
    fn get_current_experiments(&self) -> Promise<Option<Vec<String>>> {
        async_fn(async { Ok(Some(Vec::new())) })
    }

    fn add_telemetry_assignment_filter(&self, _filter: Rc<dyn IAssignmentFilter>) {}

    fn get_treatment_with_assignment(&self, name: &str) -> Promise<ITreatmentWithAssignment> {
        let value = self.get_treatment(name);
        async_fn(async move { Ok(ITreatmentWithAssignment { value: value.await?, has_assignment: Promise::resolved(false) }) })
    }
}
