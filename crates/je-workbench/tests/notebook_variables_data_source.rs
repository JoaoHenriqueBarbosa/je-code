//! Porte de `src/vs/workbench/contrib/notebook/test/browser/notebookVariablesDataSource.test.ts` (suíte `NotebookVariableDataSource`), caso a caso e na mesma ordem. Os 5 casos estão portados.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` é o `LeakGuard` de cada caso, e o `setup` (`provideVariablesCalled = false`, a fonte de
//!   dados nova e o `results` com a variável `a`) é [`Suite::new`]. Os casos `async` rodam com `run_with_real_timers`, e
//!   `await dataSource.getChildren(...)` é o `.await` da promessa devolvida.
//! - `notebookModel = { uri: 'one.ipynb', languages: ['python'] } as unknown as NotebookTextModel` é o [`NotebookTextModelLike`] com o
//!   `URI` de `one.ipynb` (a fonte só lê o `uri`), e `kernel`/`kernelService` (`mock<...>()`) são o [`MockKernel`] e o
//!   [`MockKernelService`], em que só o que o original sobrescreve responde e todo outro membro lança `TypeError`.
//! - `results` é o `Rc<RefCell<Vec<..>>>` que o executor do `AsyncIterableProducer` lê quando roda (como a variável do módulo no
//!   original), e `action?: () => void` é o `Option<Rc<dyn Fn()>>` de [`ResultWithAction`].
//! - `VariablesResult` do porte tem o `extension_id` obrigatório (o original não o preenche nos literais): vai como texto vazio.
//! - `assert.equal(a, b, msg)` é `assert_eq!(a, b, msg)`.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use common::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite, run_with_real_timers};
use je_base::async_::AsyncIterableProducer;
use je_base::cancellation::CancellationToken;
use je_base::errors::{self, Error};
use je_base::event::Event;
use je_base::lifecycle::DisposableRef;
use je_base::path::Platform;
use je_base::promise::Promise;
use je_base::uri::URI;
use je_extensions::manifest::ExtensionIdentifier;
use je_platform::contextkey::contextkey::IContextKeyService;
use je_workbench::api::common::ext_host_protocol::{VariablesRequestKindDto, VariablesResult};
use je_workbench::contrib::notebook::browser::contrib::notebook_variables::notebook_variables_data_source::{
    INotebookScope, INotebookVariableElement, NotebookVariableDataSource, NotebookVariablesNode,
};
use je_workbench::contrib::notebook::common::notebook_common::INotebookKernelSourceAction;
use je_workbench::contrib::notebook::common::notebook_kernel_service::{
    IKernelSourceActionProvider, INotebookKernel, INotebookKernelChangeEvent, INotebookKernelDetectionTask, INotebookKernelMatchResult, INotebookKernelService,
    INotebookSourceActionChangeEvent, INotebookTextModelLike, ISelectedNotebooksChangeEvent, ISourceAction, NotebookTextModelLike,
};

fn absent(member: &str) -> ! {
    errors::throw(Error::type_error(format!("{member} is not a function")))
}

/// `VariablesResult & { action?: () => void }`.
#[derive(Clone)]
struct ResultWithAction {
    result: VariablesResult,
    action: Option<Rc<dyn Fn()>>,
}

type Results = Rc<RefCell<Vec<ResultWithAction>>>;

/// `new class extends mock<INotebookKernel>() { override hasVariableProvider = true; override provideVariables(...) }`.
struct MockKernel {
    results: Results,
    provide_variables_called: Rc<Cell<bool>>,
}

impl INotebookKernel for MockKernel {
    fn id(&self) -> String {
        absent("id")
    }

    fn view_type(&self) -> String {
        absent("viewType")
    }

    fn on_did_change(&self) -> Event<INotebookKernelChangeEvent> {
        absent("onDidChange")
    }

    fn extension(&self) -> ExtensionIdentifier {
        absent("extension")
    }

    fn local_resource_root(&self) -> URI {
        absent("localResourceRoot")
    }

    fn preload_uris(&self) -> Vec<URI> {
        absent("preloadUris")
    }

    fn preload_provides(&self) -> Vec<String> {
        absent("preloadProvides")
    }

    fn label(&self) -> String {
        absent("label")
    }

    fn description(&self) -> Option<String> {
        absent("description")
    }

    fn detail(&self) -> Option<String> {
        absent("detail")
    }

    fn supported_languages(&self) -> Vec<String> {
        absent("supportedLanguages")
    }

    fn implements_interrupt(&self) -> Option<bool> {
        absent("implementsInterrupt")
    }

    fn implements_execution_order(&self) -> Option<bool> {
        absent("implementsExecutionOrder")
    }

    fn has_variable_provider(&self) -> Option<bool> {
        Some(true)
    }

    fn execute_notebook_cells_request(&self, _uri: &URI, _cell_handles: Vec<i64>) -> Promise<()> {
        absent("executeNotebookCellsRequest")
    }

    fn cancel_notebook_cell_execution(&self, _uri: &URI, _cell_handles: Vec<i64>) -> Promise<()> {
        absent("cancelNotebookCellExecution")
    }

    fn provide_variables(
        &self,
        _notebook_uri: &URI,
        _parent_id: Option<u32>,
        _kind: VariablesRequestKindDto,
        _start: usize,
        token: CancellationToken,
    ) -> Rc<AsyncIterableProducer<VariablesResult>> {
        self.provide_variables_called.set(true);
        let results = self.results.clone();
        AsyncIterableProducer::new(
            move |emitter| {
                // `results.length` e `results[i]` são lidos a cada volta, como no original.
                let mut i = 0;
                while i < results.borrow().len() {
                    if token.is_cancellation_requested() {
                        break;
                    }
                    let current = results.borrow()[i].clone();
                    if let Some(action) = &current.action {
                        action();
                    }
                    emitter.emit_one(current.result);
                    i += 1;
                }
                ().into()
            },
            None,
        )
    }
}

/// `new class extends mock<INotebookKernelService>() { override getMatchingKernel(notebook) { ... } }`.
struct MockKernelService {
    kernel: Rc<dyn INotebookKernel>,
}

impl INotebookKernelService for MockKernelService {
    fn on_did_add_kernel(&self) -> Event<Rc<dyn INotebookKernel>> {
        absent("onDidAddKernel")
    }

    fn on_did_remove_kernel(&self) -> Event<Rc<dyn INotebookKernel>> {
        absent("onDidRemoveKernel")
    }

    fn on_did_change_selected_notebooks(&self) -> Event<ISelectedNotebooksChangeEvent> {
        absent("onDidChangeSelectedNotebooks")
    }

    fn on_did_change_notebook_affinity(&self) -> Event<()> {
        absent("onDidChangeNotebookAffinity")
    }

    fn on_did_notebook_variables_update(&self) -> Event<URI> {
        absent("onDidNotebookVariablesUpdate")
    }

    fn register_kernel(&self, _kernel: Rc<dyn INotebookKernel>) -> DisposableRef {
        absent("registerKernel")
    }

    fn get_matching_kernel(&self, _notebook: &dyn INotebookTextModelLike) -> INotebookKernelMatchResult {
        INotebookKernelMatchResult { selected: Some(self.kernel.clone()), all: Vec::new(), suggestions: Vec::new(), hidden: Vec::new() }
    }

    fn get_selected_or_suggested_kernel(&self, _notebook: &dyn INotebookTextModelLike) -> Option<Rc<dyn INotebookKernel>> {
        absent("getSelectedOrSuggestedKernel")
    }

    fn select_kernel_for_notebook(&self, _kernel: Option<&Rc<dyn INotebookKernel>>, _notebook: &dyn INotebookTextModelLike) {
        absent("selectKernelForNotebook")
    }

    fn preselect_kernel_for_notebook(&self, _kernel: &Rc<dyn INotebookKernel>, _notebook: &dyn INotebookTextModelLike) {
        absent("preselectKernelForNotebook")
    }

    fn update_kernel_notebook_affinity(&self, _kernel: &Rc<dyn INotebookKernel>, _notebook: &URI, _preference: Option<i32>) {
        absent("updateKernelNotebookAffinity")
    }

    fn on_did_change_kernel_detection_tasks(&self) -> Event<String> {
        absent("onDidChangeKernelDetectionTasks")
    }

    fn register_notebook_kernel_detection_task(&self, _task: Rc<dyn INotebookKernelDetectionTask>) -> DisposableRef {
        absent("registerNotebookKernelDetectionTask")
    }

    fn get_kernel_detection_tasks(&self, _notebook: &dyn INotebookTextModelLike) -> Vec<Rc<dyn INotebookKernelDetectionTask>> {
        absent("getKernelDetectionTasks")
    }

    fn on_did_change_source_actions(&self) -> Event<INotebookSourceActionChangeEvent> {
        absent("onDidChangeSourceActions")
    }

    fn get_source_actions(&self, _notebook: &dyn INotebookTextModelLike, _context_key_service: Option<Rc<dyn IContextKeyService>>) -> Vec<Rc<dyn ISourceAction>> {
        absent("getSourceActions")
    }

    fn get_running_source_actions(&self, _notebook: &dyn INotebookTextModelLike) -> Vec<Rc<dyn ISourceAction>> {
        absent("getRunningSourceActions")
    }

    fn register_kernel_source_action_provider(&self, _view_type: &str, _provider: Rc<IKernelSourceActionProvider>) -> DisposableRef {
        absent("registerKernelSourceActionProvider")
    }

    fn get_kernel_source_actions2(&self, _notebook: &dyn INotebookTextModelLike) -> Promise<Vec<INotebookKernelSourceAction>> {
        absent("getKernelSourceActions2")
    }

    fn notify_variables_change(&self, _notebook_uri: &URI) {
        absent("notifyVariablesChange")
    }
}

/// `{ id, name, value, hasNamedChildren: false, indexedChildrenCount: 0 }` (o `extension_id` é obrigatório no porte).
fn variable(id: u32, name: &str, value: &str) -> ResultWithAction {
    ResultWithAction {
        result: VariablesResult {
            id,
            name: name.to_string(),
            value: value.to_string(),
            r#type: None,
            interfaces: None,
            language: None,
            expression: None,
            has_named_children: false,
            indexed_children_count: 0,
            extension_id: String::new(),
        },
        action: None,
    }
}

/// O `setup` da suíte, com o `notebookModel`, o `dataSource`, o `provideVariablesCalled` e o `results`.
struct Suite {
    notebook_model: Rc<dyn INotebookTextModelLike>,
    data_source: Rc<NotebookVariableDataSource>,
    provide_variables_called: Rc<Cell<bool>>,
    results: Results,
    _store: LeakGuard,
}

impl Suite {
    fn new() -> Self {
        let store = ensure_no_disposables_are_leaked_in_test_suite();
        let notebook_model: Rc<dyn INotebookTextModelLike> =
            Rc::new(NotebookTextModelLike { uri: URI::file("one.ipynb", Platform::Posix), notebook_type: "python".to_string() });
        let provide_variables_called = Rc::new(Cell::new(false));
        let results: Results = Rc::new(RefCell::new(Vec::new()));
        let kernel: Rc<dyn INotebookKernel> = Rc::new(MockKernel { results: results.clone(), provide_variables_called: provide_variables_called.clone() });
        let kernel_service: Rc<dyn INotebookKernelService> = Rc::new(MockKernelService { kernel });
        let data_source = Rc::new(NotebookVariableDataSource::new(kernel_service));
        *results.borrow_mut() = vec![variable(1, "a", "1")];
        Self { notebook_model, data_source, provide_variables_called, results, _store: store }
    }

    /// `{ kind: 'variable', notebook, id: '1', extHostId: 1, name: 'list', value: '[...]', hasNamedChildren: false, indexedChildrenCount }`.
    fn list_parent(&self, indexed_children_count: usize) -> INotebookVariableElement {
        INotebookVariableElement {
            id: "1".to_string(),
            ext_host_id: 1,
            name: "list".to_string(),
            value: "[...]".to_string(),
            r#type: None,
            interfaces: None,
            expression: None,
            language: None,
            indexed_children_count,
            index_start: None,
            has_named_children: false,
            notebook: self.notebook_model.clone(),
            extension_id: None,
        }
    }
}

#[test]
fn root_element_should_return_children() {
    run_with_real_timers(async move {
        let suite = Suite::new();

        let variables = suite.data_source.get_children(&NotebookVariablesNode::Root(INotebookScope { notebook: suite.notebook_model.clone() })).await.unwrap();

        assert_eq!(variables.len(), 1);
    });
}

#[test]
fn get_children_of_list_element() {
    run_with_real_timers(async move {
        let suite = Suite::new();
        let parent = suite.list_parent(5);
        *suite.results.borrow_mut() = vec![variable(2, "first", "1"), variable(3, "second", "2"), variable(4, "third", "3"), variable(5, "fourth", "4"), variable(6, "fifth", "5")];

        let variables = suite.data_source.get_children(&NotebookVariablesNode::Variable(parent)).await.unwrap();

        assert_eq!(variables.len(), 5);
    });
}

#[test]
fn get_children_for_large_list() {
    run_with_real_timers(async move {
        let suite = Suite::new();
        let parent = suite.list_parent(2000);
        suite.results.borrow_mut().clear();

        let variables = suite.data_source.get_children(&NotebookVariablesNode::Variable(parent.clone())).await.unwrap();

        assert!(variables.len() > 1, "We should have results for groups of children");
        assert!(!suite.provide_variables_called.get(), "provideVariables should not be called");
        assert_eq!(variables[0].ext_host_id, parent.ext_host_id, "ExtHostId should match the parent since we will use it to get the real children");
    });
}

#[test]
fn get_children_for_very_large_list() {
    run_with_real_timers(async move {
        let suite = Suite::new();
        let parent = suite.list_parent(1_000_000);
        suite.results.borrow_mut().clear();

        let groups = suite.data_source.get_children(&NotebookVariablesNode::Variable(parent.clone())).await.unwrap();
        let children = suite.data_source.get_children(&NotebookVariablesNode::Variable(groups[99].clone())).await.unwrap();

        assert!(children.len() == 100, "We should have a full page of child groups");
        assert!(!suite.provide_variables_called.get(), "provideVariables should not be called");
        assert_eq!(children[0].ext_host_id, parent.ext_host_id, "ExtHostId should match the parent since we will use it to get the real children");
    });
}

#[test]
fn cancel_while_enumerating_through_children() {
    run_with_real_timers(async move {
        let suite = Suite::new();
        let parent = suite.list_parent(10);
        let data_source = suite.data_source.clone();
        let mut fifth = variable(5, "fifth", "4");
        fifth.action = Some(Rc::new(move || data_source.cancel()));
        *suite.results.borrow_mut() = vec![
            variable(2, "first", "1"),
            variable(3, "second", "2"),
            variable(4, "third", "3"),
            variable(5, "fourth", "4"),
            fifth,
            variable(7, "sixth", "6"),
            variable(8, "seventh", "7"),
            variable(9, "eighth", "8"),
            variable(10, "ninth", "9"),
            variable(11, "tenth", "10"),
        ];

        let variables = suite.data_source.get_children(&NotebookVariablesNode::Variable(parent)).await.unwrap();

        assert_eq!(variables.len(), 5, "Iterating should have been cancelled");
    });
}
