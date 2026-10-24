//! Porte de `src/vs/workbench/contrib/notebook/test/browser/notebookExecutionStateService.test.ts` (suíte
//! `NotebookExecutionStateService`), caso a caso e na mesma ordem. Os 10 casos estão portados.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` é o `LeakGuard` criado no começo de cada caso (`_store`); o `setup` (o
//!   `DisposableStore`, o `setupInstantiationService`, o stub de `INotebookService` com `testNotebookModel`, os de `IMenuService` e
//!   `INotebookLoggingService`, o `NotebookKernelService`, o `NotebookExecutionService` e o `NotebookExecutionStateService` reais)
//!   é [`Suite::new`], e o `teardown` é o `Drop` de [`Suite`]. O `INotebookExecutionService` recebe o
//!   `TestNotebookExecutionStateService` do `setupInstantiationService`, como no original, onde ele é criado antes de o serviço de
//!   estado real ser registrado. O `ICommandService` e o `INotebookKernelHistoryService`, que o original deixa sem registro (o
//!   serviço de execução só cancela neste arquivo), são o `MockCommandService` vazio e o mock do histórico.
//! - O `withTestNotebook` local (que repassa `viewModel`, `viewModel.notebookDocument` e `disposables`) é [`with_notebook`].
//! - `assert.strictEqual(a.toString(), b.toString())` de URIs compara `to_string(false)`.
//! - Os `DeferredPromise` dos casos que esperam um evento viram uma anotação feita pelo ouvinte e conferida depois da chamada que
//!   dispara o evento (o evento é síncrono, e o `return deferred.p` só espera essa entrega); as asserções do ouvinte rodam depois,
//!   com os valores colhidos nele.
//! - `executionStateService.onDidChangeExecution(listener, this, disposables)` é o `listen` com o `DisposableStore` do caso.
//! - `testCancelOnDelete(expectedCancels, implementsInterrupt)` é [`test_cancel_on_delete`].

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

mod support;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use common::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite};
use je_base::event::Event;
use je_base::lifecycle::{DisposableStore, IDisposable};
use je_base::promise::Promise;
use je_base::uri::URI;
use je_platform::commands::commands::ICommandService;
use je_platform::log::NullLogService;
use je_platform::workspace_trust::IWorkspaceTrustRequestService;
use je_editor::code_action::platform::IAccessibilitySignalService;
use je_workbench::contrib::notebook::browser::controller::cell_operations::insert_cell_at_index;
use je_workbench::contrib::notebook::browser::services::notebook_execution_service_impl::NotebookExecutionService;
use je_workbench::contrib::notebook::browser::services::notebook_execution_state_service_impl::NotebookExecutionStateService;
use je_workbench::contrib::notebook::browser::services::notebook_kernel_service_impl::NotebookKernelService;
use je_workbench::contrib::notebook::browser::view_model::notebook_view_model_impl::NotebookViewModel;
use je_workbench::contrib::notebook::common::model::notebook_text_model::NotebookTextModel;
use je_workbench::contrib::notebook::common::notebook_common::{CellKind, CellUri, ICellEditOperation, ICellReplaceEdit, NotebookExecutionState};
use je_workbench::contrib::notebook::common::notebook_execution_service::{ICellExecuteOutputItemEdit, INotebookExecutionService};
use je_workbench::contrib::notebook::common::notebook_execution_state_service::{
    ExecutionStateChange, ICellExecuteUpdate, ICellExecutionComplete, ICellExecutionStateUpdate, INotebookExecutionStateService, NotebookExecutionType,
};
use je_workbench::contrib::notebook::common::notebook_kernel_service::{INotebookKernel, INotebookKernelHistoryService, INotebookKernelService};
use je_workbench::contrib::notebook::common::notebook_service::INotebookService;
use je_workbench::test::common::workbench_test_services::TestWorkspaceTrustRequestService;
use support::mock_command_service::MockCommandService;
use support::notebook_services_support::{
    AccessibilitySignalServiceMock, KernelHistoryServiceMock, NotebookServiceMock, TestNotebookKernel, TestNotebookKernelOptions, create_kernel_service,
};
use support::test_notebook_editor::{MockNotebookCell, empty_metadata, setup_notebook_services, with_test_notebook};

/// O `setup` da suíte.
struct Suite {
    disposables: Rc<DisposableStore>,
    test_notebook_model: Rc<RefCell<Option<Rc<NotebookTextModel>>>>,
    kernel_service: Rc<NotebookKernelService>,
    execution_state_service: Rc<NotebookExecutionStateService>,
    _store: LeakGuard,
}

impl Suite {
    fn new() -> Self {
        let store = ensure_no_disposables_are_leaked_in_test_suite();
        let disposables = DisposableStore::new();
        let services = setup_notebook_services(&disposables);

        let test_notebook_model: Rc<RefCell<Option<Rc<NotebookTextModel>>>> = Rc::new(RefCell::new(None));
        let model = test_notebook_model.clone();
        let notebook_service: Rc<dyn INotebookService> = Rc::new(NotebookServiceMock {
            on_did_add_notebook_document: Event::none(),
            on_will_remove_notebook_document: Event::none(),
            get_notebook_text_model: Some(Box::new(move |_uri| model.borrow().clone())),
        });

        let kernel_service = create_kernel_service(&disposables, notebook_service.clone());
        let kernel_service_dyn: Rc<dyn INotebookKernelService> = kernel_service.clone();
        let kernel_history_service: Rc<dyn INotebookKernelHistoryService> = Rc::new(KernelHistoryServiceMock { kernel_service: kernel_service_dyn.clone() });
        let command_service: Rc<dyn ICommandService> = Rc::new(MockCommandService::default());
        let workspace_trust_request_service: Rc<dyn IWorkspaceTrustRequestService> = disposables.add_rc(TestWorkspaceTrustRequestService::new(true));
        let execution_service: Rc<dyn INotebookExecutionService> = disposables.add_rc(NotebookExecutionService::new(
            command_service,
            kernel_service_dyn.clone(),
            kernel_history_service,
            workspace_trust_request_service,
            services.notebook_logging_service(),
            services.notebook_execution_state_service(),
        ));
        let accessibility_signal_service: Rc<dyn IAccessibilitySignalService> = Rc::new(AccessibilitySignalServiceMock);
        let execution_state_service = disposables.add_rc(NotebookExecutionStateService::new(
            notebook_service,
            kernel_service_dyn,
            execution_service,
            NullLogService::shared(),
            accessibility_signal_service,
        ));
        Self { disposables, test_notebook_model, kernel_service, execution_state_service, _store: store }
    }

    /// `instantiationService.get(INotebookExecutionStateService)`.
    fn state(&self) -> Rc<dyn INotebookExecutionStateService> {
        self.execution_state_service.clone()
    }

    /// O `withTestNotebook` local da suíte.
    fn with_notebook(&self, cells: Vec<MockNotebookCell>, callback: impl FnOnce(&Rc<NotebookViewModel>, &Rc<NotebookTextModel>)) {
        with_test_notebook(cells, |_editor, view_model, _ds, _services| callback(view_model, &view_model.notebook_document()));
    }

    /// `testNotebookModel = viewModel.notebookDocument;` e o registro e a seleção do kernel.
    fn select(&self, view_model: &Rc<NotebookViewModel>, kernel: &Rc<TestNotebookKernel>) {
        *self.test_notebook_model.borrow_mut() = Some(view_model.notebook_document());
        let kernel: Rc<dyn INotebookKernel> = kernel.clone();
        self.disposables.add(self.kernel_service.register_kernel(kernel.clone()));
        self.kernel_service.select_kernel_for_notebook(Some(&kernel), &*view_model.notebook_document());
    }
}

impl Drop for Suite {
    fn drop(&mut self) {
        self.disposables.dispose();
    }
}

fn uri_text(uri: &URI) -> String {
    uri.to_string(false).to_string_lossy()
}

fn js_cell(view_model: &Rc<NotebookViewModel>, index: i64) -> je_workbench::contrib::notebook::browser::view_model::notebook_view_model_impl::CellViewModel {
    insert_cell_at_index(view_model, index, "var c = 3", "javascript", CellKind::Code, Some(empty_metadata()), Vec::new(), true, true)
}

/// `viewModel.notebookDocument.applyEdits([{ editType: CellEditType.Replace, index, count, cells: [] }], true, undefined, () => undefined, undefined, false)`.
fn delete_cells(text_model: &NotebookTextModel, index: usize, count: usize) {
    text_model.apply_edits(&[ICellEditOperation::Replace(ICellReplaceEdit { index, count, cells: Vec::new() })], true, None, &|| None, None, false);
}

fn state_update(execution_order: f64) -> ICellExecuteUpdate {
    ICellExecuteUpdate::ExecutionState(ICellExecutionStateUpdate { execution_order: Some(execution_order), ..Default::default() })
}

/// `testCancelOnDelete(expectedCancels, implementsInterrupt)`.
fn test_cancel_on_delete(expected_cancels: usize, implements_interrupt: bool) {
    let suite = Suite::new();
    suite.with_notebook(Vec::new(), |view_model, text_model| {
        let cancels = Rc::new(Cell::new(0usize));
        let kernel = TestNotebookKernel::new(TestNotebookKernelOptions { languages: Some(vec!["javascript"]), ..Default::default() });
        kernel.implements_interrupt.set(Some(implements_interrupt));
        let counter = cancels.clone();
        *kernel.cancel_notebook_cell_execution.borrow_mut() = Box::new(move |_uri, handles| {
            counter.set(counter.get() + handles.len());
            Promise::resolved(())
        });
        suite.select(view_model, &kernel);

        let execution_state_service = suite.state();

        // Should cancel executing and pending cells, when kernel does not implement interrupt
        let cell = js_cell(view_model, 0);
        let cell2 = js_cell(view_model, 1);
        let cell3 = js_cell(view_model, 2);
        js_cell(view_model, 3); // Not deleted
        let exe = execution_state_service.create_cell_execution(&view_model.uri(), cell.handle()); // Executing
        exe.confirm();
        exe.update(vec![state_update(1.0)]);
        let exe2 = execution_state_service.create_cell_execution(&view_model.uri(), cell2.handle()); // Pending
        exe2.confirm();
        execution_state_service.create_cell_execution(&view_model.uri(), cell3.handle()); // Unconfirmed
        assert_eq!(cancels.get(), 0);
        delete_cells(text_model, 0, 3);
        assert_eq!(cancels.get(), expected_cancels);

        cell.dispose();
        cell2.dispose();
        cell3.dispose();
    });
}

// TODO@roblou Could be a test just for NotebookExecutionListeners, which can be a standalone contribution
#[test]
fn cancel_execution_when_cell_is_deleted() {
    test_cancel_on_delete(3, false);
}

#[test]
fn cancel_execution_when_cell_is_deleted_in_interrupt_type_kernel() {
    test_cancel_on_delete(1, true);
}

#[test]
fn fires_on_did_change_cell_execution_when_cell_is_completed_while_deleted() {
    let suite = Suite::new();
    suite.with_notebook(Vec::new(), |view_model, text_model| {
        let kernel = TestNotebookKernel::new(TestNotebookKernelOptions::default());
        suite.select(view_model, &kernel);

        let execution_state_service = suite.state();
        let cell = js_cell(view_model, 0);
        let exe = execution_state_service.create_cell_execution(&view_model.uri(), cell.handle());

        let did_fire = Rc::new(Cell::new(false));
        let flag = did_fire.clone();
        suite.disposables.add(execution_state_service.on_did_change_execution().listen(move |e| {
            if let ExecutionStateChange::Cell(e) = e {
                flag.set(e.changed().is_none());
            }
        }));

        delete_cells(text_model, 0, 1);
        exe.complete(ICellExecutionComplete::default());
        assert!(did_fire.get());
    });
}

#[test]
fn does_not_fire_on_did_change_cell_execution_for_output_updates() {
    let suite = Suite::new();
    suite.with_notebook(Vec::new(), |view_model, _text_model| {
        let kernel = TestNotebookKernel::new(TestNotebookKernelOptions::default());
        suite.select(view_model, &kernel);

        let execution_state_service = suite.state();
        let cell = js_cell(view_model, 0);
        let exe = execution_state_service.create_cell_execution(&view_model.uri(), cell.handle());

        let did_fire = Rc::new(Cell::new(false));
        let flag = did_fire.clone();
        suite.disposables.add(execution_state_service.on_did_change_execution().listen(move |e| {
            if e.type_() == NotebookExecutionType::Cell {
                flag.set(true);
            }
        }));

        exe.update(vec![ICellExecuteUpdate::OutputItems(ICellExecuteOutputItemEdit { append: None, output_id: "1".to_string(), items: Vec::new() })]);
        assert!(!did_fire.get());
        exe.update(vec![state_update(123.0)]);
        assert!(did_fire.get());
        exe.complete(ICellExecutionComplete::default());
        cell.dispose();
    });
}

// #142466
#[test]
fn get_cell_execution_and_on_did_change_cell_execution() {
    let suite = Suite::new();
    suite.with_notebook(Vec::new(), |view_model, _text_model| {
        let kernel = TestNotebookKernel::new(TestNotebookKernelOptions::default());
        suite.select(view_model, &kernel);

        let execution_state_service = suite.state();
        let cell = js_cell(view_model, 0);

        /// O que o ouvinte colhe do evento e da execução que o serviço devolve para a célula.
        struct Observed {
            event_notebook: String,
            exe_notebook: String,
            event_cell_handle: i64,
            exe_cell_handle: i64,
            changed_notebook: Option<String>,
            changed_cell_handle: Option<i64>,
        }
        let observed: Rc<RefCell<Option<Observed>>> = Rc::new(RefCell::new(None));
        let sink = observed.clone();
        let service = execution_state_service.clone();
        suite.disposables.add(execution_state_service.on_did_change_execution().listen(move |e| {
            if let ExecutionStateChange::Cell(e) = e {
                let cell_uri = CellUri::generate(&e.notebook(), e.cell_handle());
                let exe = service.get_cell_execution(&cell_uri).expect("a execução da célula existe");
                *sink.borrow_mut() = Some(Observed {
                    event_notebook: uri_text(&e.notebook()),
                    exe_notebook: uri_text(&exe.notebook()),
                    event_cell_handle: e.cell_handle(),
                    exe_cell_handle: exe.cell_handle(),
                    changed_notebook: e.changed().map(|changed| uri_text(&changed.notebook())),
                    changed_cell_handle: e.changed().map(|changed| changed.cell_handle()),
                });
            }
        }));

        execution_state_service.create_cell_execution(&view_model.uri(), cell.handle());

        let observed = observed.borrow();
        let observed = observed.as_ref().expect("o evento dispara");
        assert_eq!(observed.event_notebook, observed.exe_notebook);
        assert_eq!(observed.event_cell_handle, observed.exe_cell_handle);

        assert_eq!(Some(observed.exe_notebook.clone()), observed.changed_notebook);
        assert_eq!(Some(observed.exe_cell_handle), observed.changed_cell_handle);
        cell.dispose();
    });
}

#[test]
fn get_execution_and_on_did_change_execution() {
    let suite = Suite::new();
    suite.with_notebook(Vec::new(), |view_model, _text_model| {
        let kernel = TestNotebookKernel::new(TestNotebookKernelOptions::default());
        suite.select(view_model, &kernel);

        let event_raised_with_execution: Rc<RefCell<Vec<bool>>> = Rc::new(RefCell::new(Vec::new()));
        let execution_state_service = suite.state();
        let raised = event_raised_with_execution.clone();
        suite.disposables.add(execution_state_service.on_did_change_execution().listen(move |e| {
            raised.borrow_mut().push(match e {
                ExecutionStateChange::Notebook(e) => e.changed().is_some(),
                ExecutionStateChange::Cell(_) => false,
            });
        }));

        /// O que o segundo ouvinte colhe quando o evento é de notebook.
        struct Observed {
            exe_found: bool,
            notebook_matches: bool,
            affects_notebook: bool,
            raised_so_far: Vec<bool>,
        }
        let observed: Rc<RefCell<Option<Observed>>> = Rc::new(RefCell::new(None));
        let sink = observed.clone();
        let service = execution_state_service.clone();
        let raised = event_raised_with_execution.clone();
        let notebook_uri = view_model.uri();
        suite.disposables.add(execution_state_service.on_did_change_execution().listen(move |e| {
            if let ExecutionStateChange::Notebook(e) = e {
                let exe = service.get_execution(&notebook_uri);
                *sink.borrow_mut() = Some(Observed {
                    exe_found: exe.is_some(),
                    notebook_matches: exe.as_ref().is_some_and(|exe| uri_text(&e.notebook()) == uri_text(&exe.notebook())),
                    affects_notebook: e.affects_notebook(&notebook_uri),
                    raised_so_far: raised.borrow().clone(),
                });
            }
        }));

        execution_state_service.create_execution(&view_model.uri());

        let observed = observed.borrow();
        let observed = observed.as_ref().expect("o evento dispara");
        assert!(observed.exe_found);
        assert!(observed.notebook_matches);
        assert!(observed.affects_notebook);
        assert_eq!(observed.raised_so_far, vec![true]);
    });
}

#[test]
fn get_execution_and_on_did_change_execution_2() {
    let suite = Suite::new();
    suite.with_notebook(Vec::new(), |view_model, _text_model| {
        let kernel = TestNotebookKernel::new(TestNotebookKernelOptions::default());
        suite.select(view_model, &kernel);

        let execution_state_service = suite.state();

        let expected_notebook_event_states: Rc<RefCell<std::collections::VecDeque<Option<NotebookExecutionState>>>> = Rc::new(RefCell::new(
            [Some(NotebookExecutionState::Unconfirmed), Some(NotebookExecutionState::Pending), Some(NotebookExecutionState::Executing), None].into(),
        ));
        let remaining_at_end = Rc::new(Cell::new(usize::MAX));

        /// O que o ouvinte colhe de cada evento de notebook: o esperado, o estado de `changed`, e as conferências.
        struct Observed {
            expected_state: Option<NotebookExecutionState>,
            state: Option<NotebookExecutionState>,
            changed_is_none: bool,
            exe_found: bool,
            notebook_matches: bool,
            affects_notebook: bool,
        }
        let observed: Rc<RefCell<Vec<Observed>>> = Rc::new(RefCell::new(Vec::new()));
        let (sink, expected_states, remaining) = (observed.clone(), expected_notebook_event_states.clone(), remaining_at_end.clone());
        let service = execution_state_service.clone();
        let notebook_uri = view_model.uri();
        suite.disposables.add(execution_state_service.on_did_change_execution().listen(move |e| {
            if let ExecutionStateChange::Notebook(e) = e {
                let expected_state = expected_states.borrow_mut().pop_front().flatten();
                let exe = service.get_execution(&notebook_uri);
                sink.borrow_mut().push(Observed {
                    expected_state,
                    state: e.changed().map(|changed| changed.state()),
                    changed_is_none: e.changed().is_none(),
                    exe_found: exe.is_some(),
                    notebook_matches: exe.as_ref().is_some_and(|exe| uri_text(&e.notebook()) == uri_text(&exe.notebook())),
                    affects_notebook: e.affects_notebook(&notebook_uri),
                });
                remaining.set(expected_states.borrow().len());
            }
        }));

        let execution = execution_state_service.create_execution(&view_model.uri());
        execution.confirm();
        execution.begin();
        execution.complete();

        assert_eq!(remaining_at_end.get(), 0);
        let observed = observed.borrow();
        assert_eq!(observed.len(), 4);
        for entry in observed.iter() {
            match entry.expected_state {
                Some(expected_state) => {
                    assert!(entry.exe_found);
                    assert!(entry.notebook_matches);
                    assert_eq!(entry.state, Some(expected_state));
                }
                None => assert!(entry.changed_is_none),
            }
            assert!(entry.affects_notebook);
        }
    });
}

#[test]
fn force_cancel_works_for_cell_execution() {
    let suite = Suite::new();
    suite.with_notebook(Vec::new(), |view_model, _text_model| {
        let kernel = TestNotebookKernel::new(TestNotebookKernelOptions::default());
        suite.select(view_model, &kernel);

        let execution_state_service = suite.state();
        let cell = js_cell(view_model, 0);
        execution_state_service.create_cell_execution(&view_model.uri(), cell.handle());
        let exe = execution_state_service.get_cell_execution(&cell.uri());
        assert!(exe.is_some());

        execution_state_service.force_cancel_notebook_executions(&view_model.uri());
        let exe2 = execution_state_service.get_cell_execution(&cell.uri());
        assert!(exe2.is_none());
        cell.dispose();
    });
}

#[test]
fn force_cancel_works_for_notebook_execution() {
    let suite = Suite::new();
    suite.with_notebook(Vec::new(), |view_model, _text_model| {
        let kernel = TestNotebookKernel::new(TestNotebookKernelOptions::default());
        suite.select(view_model, &kernel);
        let event_raised_with_execution: Rc<RefCell<Vec<bool>>> = Rc::new(RefCell::new(Vec::new()));

        let execution_state_service = suite.state();
        let raised = event_raised_with_execution.clone();
        suite.disposables.add(execution_state_service.on_did_change_execution().listen(move |e| {
            raised.borrow_mut().push(match e {
                ExecutionStateChange::Notebook(e) => e.changed().is_some(),
                ExecutionStateChange::Cell(_) => false,
            });
        }));
        execution_state_service.create_execution(&view_model.uri());
        let exe = execution_state_service.get_execution(&view_model.uri());
        assert!(exe.is_some());
        assert_eq!(*event_raised_with_execution.borrow(), vec![true]);

        execution_state_service.force_cancel_notebook_executions(&view_model.uri());
        let exe2 = execution_state_service.get_execution(&view_model.uri());
        assert_eq!(*event_raised_with_execution.borrow(), vec![true, false]);
        assert!(exe2.is_none());
    });
}

#[test]
fn force_cancel_works_for_cell_and_notebook_execution() {
    let suite = Suite::new();
    suite.with_notebook(Vec::new(), |view_model, _text_model| {
        let kernel = TestNotebookKernel::new(TestNotebookKernelOptions::default());
        suite.select(view_model, &kernel);

        let execution_state_service = suite.state();
        execution_state_service.create_execution(&view_model.uri());
        execution_state_service.create_execution(&view_model.uri());
        let cell_exe = execution_state_service.get_execution(&view_model.uri());
        let exe = execution_state_service.get_execution(&view_model.uri());
        assert!(cell_exe.is_some());
        assert!(exe.is_some());

        execution_state_service.force_cancel_notebook_executions(&view_model.uri());
        let cell_exe2 = execution_state_service.get_execution(&view_model.uri());
        let exe2 = execution_state_service.get_execution(&view_model.uri());
        assert!(cell_exe2.is_none());
        assert!(exe2.is_none());
    });
}
