//! Porte de `src/vs/workbench/contrib/notebook/test/browser/notebookExecutionService.test.ts` (suíte `NotebookExecutionService`),
//! caso a caso e na mesma ordem. Os 4 casos estão portados (o `test('ctor')` do original está comentado lá e não é portado).
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` é o `LeakGuard` criado no começo de cada caso (`_store`); o `setup` (o
//!   `DisposableStore`, o `setupInstantiationService` e os stubs de `INotebookService`, `INotebookLoggingService`, `IMenuService`,
//!   `INotebookKernelHistoryService` e `ICommandService`) é [`Suite::new`], e o `teardown` é o `Drop` de [`Suite`]. O
//!   `INotebookExecutionStateService` do serviço é o `TestNotebookExecutionStateService` do `setupInstantiationService`, como
//!   no original (o `instantiationService.get(INotebookExecutionStateService)` do último caso devolve o mesmo objeto).
//! - `await executionService.executeNotebookCells(...)` é a [`Promise`] devolvida, resolvida com `run_microtasks` (o
//!   `with_test_notebook` roda sob o relógio simulado). O callback `async` do `withTestNotebook` é síncrono.
//! - Os dois primeiros casos esperam uma exceção (`assertThrowsAsync`). No original, `insertCellAtIndex(viewModel, 1, ...)` em um
//!   notebook vazio devolve `undefined` (a célula entra no índice 0), e o `cell.model` dentro do bloco lança `TypeError`: é isso que
//!   o `assertThrowsAsync` pega, e nenhum dos dois casos chega ao serviço de execução. No porte o `expect` de `insert_cell_at_index`
//!   dispara nesse mesmo ponto lógico, então a inserção e a chamada ficam dentro do bloco que espera o pânico.
//! - `sinon.spy()` em `kernel.executeNotebookCellsRequest` é o fecho que conta as chamadas; `calledOnce` é `calls == 1`.
//! - `IMenuService` e `INotebookKernelHistoryService` são os mocks de `support/notebook_services_support.rs`; o
//!   `ICommandService` é o `MockCommandService` com `executeCommand` devolvendo `undefined`.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

mod support;

use std::cell::Cell;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::rc::Rc;

use common::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite};
use je_base::dynamic::Dynamic;
use je_base::event::Event;
use je_base::lifecycle::{DisposableStore, IDisposable};
use je_base::promise::Promise;
use je_base::runtime;
use je_platform::commands::commands::ICommandService;
use je_platform::contextkey::contextkey::IContextKeyService;
use je_platform::keybinding::mock_keybinding_service::MockContextKeyService;
use je_platform::workspace_trust::IWorkspaceTrustRequestService;
use je_workbench::contrib::notebook::browser::controller::cell_operations::insert_cell_at_index;
use je_workbench::contrib::notebook::browser::services::notebook_execution_service_impl::NotebookExecutionService;
use je_workbench::contrib::notebook::browser::services::notebook_kernel_service_impl::NotebookKernelService;
use je_workbench::contrib::notebook::common::notebook_common::CellKind;
use je_workbench::contrib::notebook::common::notebook_execution_service::INotebookExecutionService;
use je_workbench::contrib::notebook::common::notebook_kernel_service::{INotebookKernel, INotebookKernelHistoryService, INotebookKernelService};
use je_workbench::contrib::notebook::common::notebook_service::INotebookService;
use je_workbench::test::common::workbench_test_services::TestWorkspaceTrustRequestService;
use support::mock_command_service::MockCommandService;
use support::notebook_services_support::{KernelHistoryServiceMock, NotebookServiceMock, TestNotebookKernel, TestNotebookKernelOptions, create_kernel_service};
use support::test_notebook_editor::{NotebookTestServices, empty_metadata, setup_notebook_services, with_test_notebook};

/// O `setup` da suíte.
struct Suite {
    disposables: Rc<DisposableStore>,
    services: NotebookTestServices,
    kernel_service: Rc<NotebookKernelService>,
    context_key_service: Rc<dyn IContextKeyService>,
    command_service: Rc<dyn ICommandService>,
    kernel_history_service: Rc<dyn INotebookKernelHistoryService>,
    workspace_trust_request_service: Rc<dyn IWorkspaceTrustRequestService>,
    _store: LeakGuard,
}

impl Suite {
    fn new() -> Self {
        let store = ensure_no_disposables_are_leaked_in_test_suite();
        let disposables = DisposableStore::new();
        let services = setup_notebook_services(&disposables);

        let notebook_service: Rc<dyn INotebookService> =
            Rc::new(NotebookServiceMock { on_did_add_notebook_document: Event::none(), on_will_remove_notebook_document: Event::none(), get_notebook_text_model: None });
        let kernel_service = create_kernel_service(&disposables, notebook_service);
        let kernel_service_dyn: Rc<dyn INotebookKernelService> = kernel_service.clone();
        let kernel_history_service: Rc<dyn INotebookKernelHistoryService> = Rc::new(KernelHistoryServiceMock { kernel_service: kernel_service_dyn });
        let command_service: Rc<dyn ICommandService> =
            Rc::new(MockCommandService { execute_command: Some(Box::new(|_| Promise::resolved(Dynamic::Undefined))) });
        let context_key_service: Rc<dyn IContextKeyService> = MockContextKeyService::new();
        let workspace_trust_request_service_concrete = disposables.add_rc(TestWorkspaceTrustRequestService::new(true));
        let workspace_trust_request_service: Rc<dyn IWorkspaceTrustRequestService> = workspace_trust_request_service_concrete;
        Self { disposables, services, kernel_service, context_key_service, command_service, kernel_history_service, workspace_trust_request_service, _store: store }
    }

    /// `instantiationService.createInstance(NotebookExecutionService)`.
    fn create_execution_service(&self) -> Rc<NotebookExecutionService> {
        let kernel_service: Rc<dyn INotebookKernelService> = self.kernel_service.clone();
        NotebookExecutionService::new(
            self.command_service.clone(),
            kernel_service,
            self.kernel_history_service.clone(),
            self.workspace_trust_request_service.clone(),
            self.services.notebook_logging_service(),
            self.services.notebook_execution_state_service(),
        )
    }
}

impl Drop for Suite {
    fn drop(&mut self) {
        self.disposables.dispose();
    }
}

fn javascript_kernel(id: Option<&str>) -> Rc<TestNotebookKernel> {
    TestNotebookKernel::new(TestNotebookKernelOptions { languages: Some(vec!["javascript"]), id: id.map(str::to_string), ..Default::default() })
}

#[test]
fn cell_is_not_runnable_when_no_kernel_is_selected() {
    let suite = Suite::new();
    with_test_notebook(Vec::new(), |_editor, view_model, _ds, _services| {
        let execution_service = suite.create_execution_service();

        let outcome = catch_unwind(AssertUnwindSafe(|| {
            let cell = insert_cell_at_index(view_model, 1, "var c = 3", "javascript", CellKind::Code, Some(empty_metadata()), Vec::new(), true, true);
            execution_service.execute_notebook_cells(&*view_model.notebook_document(), vec![cell.model()], suite.context_key_service.clone());
        }));
        assert!(outcome.is_err(), "Missing expected exception");
    });
}

#[test]
fn cell_is_not_runnable_when_kernel_does_not_support_the_language() {
    let suite = Suite::new();
    with_test_notebook(Vec::new(), |_editor, view_model, _ds, _services| {
        let kernel: Rc<dyn INotebookKernel> = TestNotebookKernel::new(TestNotebookKernelOptions { languages: Some(vec!["testlang"]), ..Default::default() });
        suite.disposables.add(suite.kernel_service.register_kernel(kernel));
        let execution_service = suite.disposables.add_rc(suite.create_execution_service());

        let outcome = catch_unwind(AssertUnwindSafe(|| {
            let cell = insert_cell_at_index(view_model, 1, "var c = 3", "javascript", CellKind::Code, Some(empty_metadata()), Vec::new(), true, true);
            execution_service.execute_notebook_cells(&*view_model.notebook_document(), vec![cell.model()], suite.context_key_service.clone());
        }));
        assert!(outcome.is_err(), "Missing expected exception");
    });
}

#[test]
fn cell_is_runnable_when_kernel_does_support_the_language() {
    let suite = Suite::new();
    with_test_notebook(Vec::new(), |_editor, view_model, _ds, _services| {
        let text_model = view_model.notebook_document();
        let kernel = javascript_kernel(None);
        let kernel_dyn: Rc<dyn INotebookKernel> = kernel.clone();
        suite.disposables.add(suite.kernel_service.register_kernel(kernel_dyn.clone()));
        suite.kernel_service.select_kernel_for_notebook(Some(&kernel_dyn), &*text_model);
        let execution_service = suite.disposables.add_rc(suite.create_execution_service());
        let execute_calls = Rc::new(Cell::new(0));
        let calls = execute_calls.clone();
        *kernel.execute_notebook_cells_request.borrow_mut() = Box::new(move |_, _| {
            calls.set(calls.get() + 1);
            Promise::resolved(())
        });

        let cell = insert_cell_at_index(view_model, 0, "var c = 3", "javascript", CellKind::Code, Some(empty_metadata()), Vec::new(), true, true);
        let execution = execution_service.execute_notebook_cells(&*text_model, vec![cell.model()], suite.context_key_service.clone());
        runtime::run_microtasks();
        assert!(matches!(execution.result(), Some(Ok(()))));
        assert_eq!(execute_calls.get(), 1);
    });
}

#[test]
fn completes_unconfirmed_executions() {
    let suite = Suite::new();
    with_test_notebook(Vec::new(), |_editor, view_model, _ds, _services| {
        let text_model = view_model.notebook_document();
        let did_execute = Rc::new(Cell::new(false));
        let kernel = javascript_kernel(Some("mySpecialId"));
        let flag = did_execute.clone();
        *kernel.execute_notebook_cells_request.borrow_mut() = Box::new(move |_, _| {
            flag.set(true);
            Promise::resolved(())
        });

        let kernel_dyn: Rc<dyn INotebookKernel> = kernel.clone();
        suite.disposables.add(suite.kernel_service.register_kernel(kernel_dyn.clone()));
        suite.kernel_service.select_kernel_for_notebook(Some(&kernel_dyn), &*text_model);
        let execution_service = suite.disposables.add_rc(suite.create_execution_service());
        let exe_state_service = suite.services.notebook_execution_state_service();

        let cell = insert_cell_at_index(view_model, 0, "var c = 3", "javascript", CellKind::Code, Some(empty_metadata()), Vec::new(), true, true);
        let execution = execution_service.execute_notebook_cells(&*text_model, vec![cell.model()], suite.context_key_service.clone());
        runtime::run_microtasks();
        assert!(matches!(execution.result(), Some(Ok(()))));

        assert!(did_execute.get());
        assert!(exe_state_service.get_cell_execution(&cell.uri()).is_none());
    });
}
