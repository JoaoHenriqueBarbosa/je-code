//! Porte de `src/vs/workbench/contrib/notebook/test/browser/diff/notebookDiff.test.ts` (suíte `NotebookDiff`), caso a caso e na mesma ordem.
//!
//! Os 15 casos estão portados.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` é o `LeakGuard` criado no começo de cada caso. O `setup` (o
//!   `DisposableStore`, a `CancellationTokenSource`, o `NotebookDiffEditorEventDispatcher`, o `notebookEditorWorkerService` cujo
//!   `computeDiff` devolve o `diffResult` do caso e o `heightCalculator` que responde zero) é [`Suite::new`], e o `teardown`
//!   (`disposables.dispose()`) roda no fim de [`run_diff_test`], que também é o `withTestNotebookDiffModel` + o `await` do corpo
//!   (os casos `async` rodam em `run_with_faked_timers`).
//! - `accessor.get(INotebookService)`, o argumento `notebookService` do `NotebookDiffViewModel`, não existe no porte: o view
//!   model dispensa o serviço (ver as divergências dele). O `TestConfigurationService({ notebook: { diff: { ignoreMetadata:
//!   true } } })` do escopo da suíte é criado por caso (o `LeakGuard` rastreia o `Emitter` dele, que o guarda descarta).
//! - `CellSequence` é um `ISequence` dos hashes das células, e `LcsDiff.ComputeDiff(false)` é `LcsDiff::compute_diff(false)`.
//! - `assert.deepStrictEqual(eventArgs, { start, deleteCount, elements })` é a comparação do `INotebookDiffViewModelUpdateEvent`
//!   (os mesmos números e os mesmos elementos, por identidade); `{ ... }` sem `firstChangeIndex` é `first_change_index: None`.
//! - `diffViewModel.items[i].showHiddenCells()` e `(items[i] as SideBySideDiffElementViewModel).hideUnchangedCells()` são
//!   `DiffElementViewModel::show_hidden_cells()` e `as_cell().hide_unchanged_cells()`.
//! - `[{ outputId, outputs: [{ mime: Mimes.text, data: VSBuffer.wrap(new Uint8Array([byte])) }] }]` é [`out`], e os literais de
//!   metadados `{ metadata: { collapsed }, executionOrder }` são [`meta`]; `{}` é `Some(objeto vazio)`.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod support;

use std::cell::RefCell;
use std::future::Future;
use std::rc::Rc;

use common::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite, run_with_faked_timers};
use je_base::buffer::VSBuffer;
use je_base::cancellation::{CancellationToken, CancellationTokenSource};
use je_base::diff::{DiffChange, DiffResult, ISequence, LcsDiff, SequenceElements};
use je_base::dynamic::Dynamic;
use je_base::lifecycle::{DisposableStore, IDisposable};
use je_base::mime::mimes;
use je_base::promise::Promise;
use je_base::uri::URI;
use je_platform::configuration::service::IConfigurationService;
use je_platform::configuration::test_configuration_service::TestConfigurationService;
use je_workbench::contrib::notebook::browser::diff::diff_element_view_model::{DiffElementType, DiffElementViewModel};
use je_workbench::contrib::notebook::browser::diff::editor_height_calculator::IDiffEditorHeightCalculatorService;
use je_workbench::contrib::notebook::browser::diff::event_dispatcher::NotebookDiffEditorEventDispatcher;
use je_workbench::contrib::notebook::browser::diff::notebook_diff_editor_browser::{INotebookDiffViewModel, INotebookDiffViewModelUpdateEvent};
use je_workbench::contrib::notebook::browser::diff::notebook_diff_view_model::{NotebookDiffViewModel, pretty_changes};
use je_workbench::contrib::notebook::common::model::notebook_text_model::NotebookTextModel;
use je_workbench::contrib::notebook::common::notebook_common::{CellKind, ICell, INotebookDiffEditorModel, INotebookDiffResult, IOutputDto, IOutputItemDto, NotebookCellMetadata};
use je_workbench::contrib::notebook::common::services::notebook_worker_service::INotebookEditorWorkerService;
use support::test_notebook_editor::{MockNotebookCell, TestNotebookDiffModel, empty_metadata, with_test_notebook_diff_model};

/// `CellSequence`.
struct CellSequence {
    hash_value: Vec<i32>,
}

impl CellSequence {
    fn new(text_model: &Rc<NotebookTextModel>) -> Self {
        Self { hash_value: text_model.cells().iter().map(|cell| cell.get_hash_value()).collect() }
    }
}

impl ISequence for CellSequence {
    fn get_elements(&self) -> SequenceElements {
        SequenceElements::Ints(self.hash_value.clone())
    }
}

/// `new LcsDiff(new CellSequence(model.original.notebook), new CellSequence(model.modified.notebook)).ComputeDiff(false)`.
fn lcs_diff(model: &TestNotebookDiffModel) -> DiffResult {
    LcsDiff::new(Rc::new(CellSequence::new(&model.original().notebook)), Rc::new(CellSequence::new(&model.modified().notebook)), None).compute_diff(false)
}

/// `diffResult.changes.map(change => ({ originalStart, originalLength, modifiedStart, modifiedLength }))`.
fn change_tuples(diff_result: &DiffResult) -> Vec<(i32, i32, i32, i32)> {
    diff_result.changes.iter().map(|change| (change.original_start, change.original_length, change.modified_start, change.modified_length)).collect()
}

/// O `notebookEditorWorkerService` do `setup`.
struct TestNotebookEditorWorkerService {
    diff_result: Rc<RefCell<Option<DiffResult>>>,
}

impl INotebookEditorWorkerService for TestNotebookEditorWorkerService {
    fn can_compute_diff(&self, _original: &URI, _modified: &URI) -> bool {
        unreachable!("TypeError: canComputeDiff is not a function")
    }

    fn compute_diff(&self, _original: &URI, _modified: &URI) -> Promise<INotebookDiffResult> {
        Promise::resolved(INotebookDiffResult { cells_diff: self.diff_result.borrow().clone().expect("diffResult"), metadata_changed: false })
    }

    fn can_prompt_recommendation(&self, _model: &URI) -> Promise<bool> {
        unreachable!("TypeError: canPromptRecommendation is not a function")
    }
}

/// O `heightCalculator` do `setup`.
struct TestHeightCalculator;

impl IDiffEditorHeightCalculatorService for TestHeightCalculator {
    fn diff_and_compute_height(&self, _original: &URI, _modified: &URI) -> Promise<f64> {
        Promise::resolved(0.0)
    }

    fn compute_height_from_lines(&self, _line_count: i64) -> f64 {
        0.0
    }
}

/// O `setup` da suíte.
struct Suite {
    disposables: Rc<DisposableStore>,
    token: CancellationToken,
    event_dispatcher: Rc<NotebookDiffEditorEventDispatcher>,
    diff_result: Rc<RefCell<Option<DiffResult>>>,
    notebook_editor_worker_service: Rc<dyn INotebookEditorWorkerService>,
    height_calculator: Rc<dyn IDiffEditorHeightCalculatorService>,
    configuration_service: Rc<dyn IConfigurationService>,
}

impl Suite {
    fn new(guard: &LeakGuard) -> Rc<Self> {
        let disposables = DisposableStore::new();
        let cancellation = CancellationTokenSource::new(None);
        disposables.add(Rc::new(cancellation.clone()));
        let event_dispatcher = disposables.add_rc(NotebookDiffEditorEventDispatcher::new());
        let diff_result: Rc<RefCell<Option<DiffResult>>> = Rc::default();

        let test_configuration_service = TestConfigurationService::new(Some(Dynamic::object([(
            "notebook",
            Dynamic::object([("diff", Dynamic::object([("ignoreMetadata", Dynamic::Bool(true))]))]),
        )])));
        guard.add_rc(test_configuration_service.on_did_change_configuration_emitter.clone());
        let configuration_service: Rc<dyn IConfigurationService> = test_configuration_service;

        Rc::new(Self {
            disposables,
            token: cancellation.token(),
            event_dispatcher,
            notebook_editor_worker_service: Rc::new(TestNotebookEditorWorkerService { diff_result: diff_result.clone() }),
            diff_result,
            height_calculator: Rc::new(TestHeightCalculator),
            configuration_service,
        })
    }

    /// `diffResult = ...`.
    fn set_diff_result(&self, diff_result: DiffResult) {
        *self.diff_result.borrow_mut() = Some(diff_result);
    }

    /// `diffViewModel = disposables.add(new NotebookDiffViewModel(model, notebookEditorWorkerService, configurationService,
    /// eventDispatcher, notebookService, heightCalculator, undefined))`.
    fn create_view_model(
        &self,
        model: &Rc<TestNotebookDiffModel>,
        disposables: &Rc<DisposableStore>,
        event_dispatcher: &Rc<NotebookDiffEditorEventDispatcher>,
    ) -> Rc<NotebookDiffViewModel> {
        disposables.add_rc(NotebookDiffViewModel::new(
            model.clone(),
            self.notebook_editor_worker_service.clone(),
            self.configuration_service.clone(),
            event_dispatcher.clone(),
            self.height_calculator.clone(),
            None,
            None,
        ))
    }

    /// `computeDiff(token)`.
    async fn compute_diff(&self, diff_view_model: &Rc<NotebookDiffViewModel>) {
        diff_view_model.compute_diff(self.token.clone()).await.expect("computeDiff");
    }

    /// `verifyChangeEventIsNotFired(diffViewModel)`.
    async fn verify_change_event_is_not_fired(&self, diff_view_model: &Rc<NotebookDiffViewModel>) {
        let event_args = listen_to_items(&self.disposables, diff_view_model);
        self.compute_diff(diff_view_model).await;

        assert!(event_args.borrow().is_none());
    }
}

type EventArgs = Rc<RefCell<Option<INotebookDiffViewModelUpdateEvent>>>;

/// `disposables.add(diffViewModel.onDidChangeItems(e => eventArgs = e))`.
fn listen_to_items(disposables: &Rc<DisposableStore>, diff_view_model: &Rc<NotebookDiffViewModel>) -> EventArgs {
    let event_args: EventArgs = Rc::default();
    let listener_args = event_args.clone();
    disposables.add(diff_view_model.on_did_change_items().listen(move |e| *listener_args.borrow_mut() = Some(e.clone())));
    event_args
}

/// `assert.deepStrictEqual(eventArgs, { start, deleteCount, elements })`.
#[track_caller]
fn assert_event_args(event_args: &EventArgs, start: i64, delete_count: usize, elements: Vec<DiffElementViewModel>) {
    let actual = event_args.borrow().clone().expect("eventArgs");
    assert_eq!(actual, INotebookDiffViewModelUpdateEvent { start, delete_count, elements, first_change_index: None });
}

/// `ensureNoDisposablesAreLeakedInTestSuite()`, o `setup`, o `withTestNotebookDiffModel` e o `teardown` de um caso.
fn run_diff_test<Fut>(
    original_cells: Vec<MockNotebookCell>,
    modified_cells: Vec<MockNotebookCell>,
    body: impl FnOnce(Rc<Suite>, Rc<TestNotebookDiffModel>, Rc<DisposableStore>) -> Fut + 'static,
) where
    Fut: Future<Output = ()> + 'static,
{
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let suite = Suite::new(&guard);
    let run_suite = suite.clone();
    run_with_faked_timers(async move {
        with_test_notebook_diff_model(original_cells, modified_cells, move |model, disposables, _services| body(run_suite, model, disposables)).await;
    });
    suite.disposables.dispose();
    drop(guard);
}

/// `[{ outputId, outputs: [{ mime: Mimes.text, data: VSBuffer.wrap(new Uint8Array([byte])) }] }]`.
fn out(output_id: &str, byte: u8) -> Vec<IOutputDto> {
    vec![IOutputDto {
        output_id: output_id.to_owned(),
        outputs: vec![IOutputItemDto { mime: mimes::TEXT.to_owned(), data: VSBuffer::wrap(vec![byte]) }],
        metadata: None,
    }]
}

/// `{ metadata: { collapsed }, executionOrder }`, sem as chaves ausentes.
fn meta(collapsed: Option<bool>, execution_order: Option<f64>) -> Option<NotebookCellMetadata> {
    let mut inner = Vec::new();
    if let Some(collapsed) = collapsed {
        inner.push(("collapsed", Dynamic::from(collapsed)));
    }
    let mut entries = vec![("metadata", Dynamic::object(inner))];
    if let Some(order) = execution_order {
        entries.push(("executionOrder", Dynamic::from(order)));
    }
    Some(Dynamic::object(entries))
}

/// `[source, 'javascript', CellKind.Code, outputs, metadata]`.
fn js(source: &'static str, outputs: Vec<IOutputDto>, metadata: Option<NotebookCellMetadata>) -> MockNotebookCell {
    (source, "javascript", CellKind::Code, outputs, metadata)
}

/// `[source, 'javascript', CellKind.Code, [], {}]`.
fn code(source: &'static str) -> MockNotebookCell {
    js(source, Vec::new(), Some(empty_metadata()))
}

/// `[source, 'markdown', CellKind.Markup, [], {}]`.
fn md(source: &'static str) -> MockNotebookCell {
    (source, "markdown", CellKind::Markup, Vec::new(), Some(empty_metadata()))
}

/// `diffResult.changes.length === 1` e `[{ originalStart: 0, originalLength: 1, modifiedStart: 0, modifiedLength: 1 }]`.
#[track_caller]
fn assert_single_replace_of_first_cell(diff_result: &DiffResult) {
    assert_eq!(diff_result.changes.len(), 1);
    assert_eq!(change_tuples(diff_result), vec![(0, 1, 0, 1)]);
}

/// `{ originalStart, originalLength, modifiedStart, modifiedLength }` de cada mudança, no formato do `DiffChange`.
fn changes(list: &[(i32, i32, i32, i32)]) -> Vec<DiffChange> {
    list.iter().map(|&(original_start, original_length, modified_start, modified_length)| DiffChange::new(original_start, original_length, modified_start, modified_length)).collect()
}

#[test]
fn diff_different_source() {
    run_diff_test(
        vec![js("x", out("someOtherId", 3), meta(Some(false), Some(3.0)))],
        vec![js("y", out("someOtherId", 3), meta(Some(false), Some(3.0)))],
        |suite, model, disposables| async move {
            let diff_result = lcs_diff(&model);
            assert_single_replace_of_first_cell(&diff_result);
            suite.set_diff_result(diff_result);

            let diff_view_model = suite.create_view_model(&model, &disposables, &suite.event_dispatcher);
            suite.compute_diff(&diff_view_model).await;

            assert_eq!(diff_view_model.items().len(), 1);
            assert_eq!(diff_view_model.items()[0].type_(), DiffElementType::Modified);
        },
    );
}

#[test]
fn no_changes_when_re_computing_diff_with_the_same_source() {
    run_diff_test(
        vec![js("x", out("someOtherId", 3), meta(Some(false), Some(3.0)))],
        vec![js("y", out("someOtherId", 3), meta(Some(false), Some(3.0)))],
        |suite, model, disposables| async move {
            let diff_result = lcs_diff(&model);
            assert_single_replace_of_first_cell(&diff_result);
            suite.set_diff_result(diff_result);

            let diff_view_model = suite.create_view_model(&model, &disposables, &suite.event_dispatcher);
            suite.compute_diff(&diff_view_model).await;

            suite.verify_change_event_is_not_fired(&diff_view_model).await;
        },
    );
}

#[test]
fn diff_different_output() {
    run_diff_test(
        vec![js("x", out("someId", 5), meta(Some(false), Some(5.0))), js("", Vec::new(), Some(empty_metadata()))],
        vec![js("x", out("someOtherId", 3), meta(Some(false), Some(3.0))), js("", Vec::new(), Some(empty_metadata()))],
        |suite, model, disposables| async move {
            let diff_result = lcs_diff(&model);
            assert_single_replace_of_first_cell(&diff_result);
            suite.set_diff_result(diff_result);

            let diff_view_model = suite.create_view_model(&model, &disposables, &suite.event_dispatcher);
            let event_args = listen_to_items(&disposables, &diff_view_model);
            suite.compute_diff(&diff_view_model).await;

            assert_eq!(diff_view_model.items().len(), 2);
            assert_eq!(diff_view_model.items()[0].type_(), DiffElementType::Modified);
            assert_eq!(diff_view_model.items()[1].type_(), DiffElementType::Placeholder);

            diff_view_model.items()[1].show_hidden_cells();

            assert_eq!(diff_view_model.items().len(), 2);
            assert_eq!(diff_view_model.items()[0].type_(), DiffElementType::Modified);
            assert_eq!(diff_view_model.items()[1].type_(), DiffElementType::Unchanged);
            assert_event_args(&event_args, 1, 1, vec![diff_view_model.items()[1].clone()]);

            diff_view_model.items()[1].as_cell().hide_unchanged_cells();

            assert_eq!(diff_view_model.items().len(), 2);
            assert_eq!(diff_view_model.items()[0].type_(), DiffElementType::Modified);
            assert_eq!(diff_view_model.items()[1].type_(), DiffElementType::Placeholder);
            assert_event_args(&event_args, 1, 1, vec![diff_view_model.items()[1].clone()]);

            suite.verify_change_event_is_not_fired(&diff_view_model).await;
        },
    );
}

#[test]
fn diff_test_small_source() {
    run_diff_test(vec![js("123456789", Vec::new(), Some(empty_metadata()))], vec![js("987654321", Vec::new(), Some(empty_metadata()))], |suite, model, disposables| async move {
        let diff_result = lcs_diff(&model);
        assert_single_replace_of_first_cell(&diff_result);
        suite.set_diff_result(diff_result);

        let diff_view_model = suite.create_view_model(&model, &disposables, &suite.event_dispatcher);
        suite.compute_diff(&diff_view_model).await;

        assert_eq!(diff_view_model.items().len(), 1);
        assert_eq!(diff_view_model.items()[0].type_(), DiffElementType::Modified);

        suite.verify_change_event_is_not_fired(&diff_view_model).await;
    });
}

#[test]
fn diff_test_data_single_cell() {
    run_diff_test(
        vec![js("# This version has a bug\ndef mult(a, b):\n    return a / b", Vec::new(), Some(empty_metadata()))],
        vec![js("def mult(a, b):\n    'This version is debugged.'\n    return a * b", Vec::new(), Some(empty_metadata()))],
        |suite, model, disposables| async move {
            let diff_result = lcs_diff(&model);
            assert_single_replace_of_first_cell(&diff_result);
            suite.set_diff_result(diff_result);

            let diff_view_model = suite.create_view_model(&model, &disposables, &suite.event_dispatcher);
            suite.compute_diff(&diff_view_model).await;

            assert_eq!(diff_view_model.items().len(), 1);
            assert_eq!(diff_view_model.items()[0].type_(), DiffElementType::Modified);

            suite.verify_change_event_is_not_fired(&diff_view_model).await;
        },
    );
}

#[test]
fn diff_foo_foe() {
    run_diff_test(
        vec![
            js("def foe(x, y):\n    return x + y\nfoe(3, 2)", out("someId", 6), meta(Some(false), Some(5.0))),
            js("def foo(x, y):\n    return x * y\nfoo(1, 2)", out("someId", 2), meta(Some(false), Some(6.0))),
            js("", Vec::new(), Some(empty_metadata())),
        ],
        vec![
            js("def foo(x, y):\n    return x * y\nfoo(1, 2)", out("someId", 6), meta(Some(false), Some(5.0))),
            js("def foe(x, y):\n    return x + y\nfoe(3, 2)", out("someId", 2), meta(Some(false), Some(6.0))),
            js("", Vec::new(), Some(empty_metadata())),
        ],
        |suite, model, disposables| async move {
            suite.set_diff_result(lcs_diff(&model));

            let diff_view_model = suite.create_view_model(&model, &disposables, &suite.event_dispatcher);
            let event_args = listen_to_items(&disposables, &diff_view_model);
            suite.compute_diff(&diff_view_model).await;

            assert_eq!(diff_view_model.items().len(), 3);
            assert_eq!(diff_view_model.items()[0].type_(), DiffElementType::Modified);
            assert_eq!(diff_view_model.items()[1].type_(), DiffElementType::Modified);
            assert_eq!(diff_view_model.items()[2].type_(), DiffElementType::Placeholder);
            diff_view_model.items()[2].show_hidden_cells();
            assert_eq!(diff_view_model.items()[2].type_(), DiffElementType::Unchanged);
            assert_event_args(&event_args, 2, 1, vec![diff_view_model.items()[2].clone()]);

            suite.verify_change_event_is_not_fired(&diff_view_model).await;
        },
    );
}

#[test]
fn diff_markdown() {
    run_diff_test(
        vec![
            md("This is a test notebook with only markdown cells"),
            md("Lorem ipsum dolor sit amet"),
            md("In other news"),
        ],
        vec![
            md("This is a test notebook with markdown cells only"),
            md("Lorem ipsum dolor sit amet"),
            md("In the news"),
        ],
        |suite, model, disposables| async move {
            suite.set_diff_result(lcs_diff(&model));

            let diff_view_model = suite.create_view_model(&model, &disposables, &suite.event_dispatcher);
            let event_args = listen_to_items(&disposables, &diff_view_model);
            suite.compute_diff(&diff_view_model).await;

            assert_eq!(diff_view_model.items().len(), 3);
            assert_eq!(diff_view_model.items()[0].type_(), DiffElementType::Modified);
            assert_eq!(diff_view_model.items()[1].type_(), DiffElementType::Placeholder);
            assert_eq!(diff_view_model.items()[2].type_(), DiffElementType::Modified);

            diff_view_model.items()[1].show_hidden_cells();
            assert_eq!(diff_view_model.items()[1].type_(), DiffElementType::Unchanged);
            assert_event_args(&event_args, 1, 1, vec![diff_view_model.items()[1].clone()]);

            suite.verify_change_event_is_not_fired(&diff_view_model).await;
        },
    );
}

#[test]
fn diff_insert() {
    run_diff_test(
        vec![code("var a = 1;"), code("var b = 2;")],
        vec![code("var h = 8;"), code("var a = 1;"), code("var b = 2;")],
        |suite, model, disposables| async move {
            suite.set_diff_result(DiffResult { changes: changes(&[(0, 0, 0, 1)]), quit_early: false });

            let diff_view_model = suite.create_view_model(&model, &disposables, &suite.event_dispatcher);
            let event_args = listen_to_items(&disposables, &diff_view_model);
            suite.compute_diff(&diff_view_model).await;

            assert_eq!(event_args.borrow().as_ref().and_then(|event| event.first_change_index), Some(0));
            assert_eq!(diff_view_model.items()[0].type_(), DiffElementType::Insert);
            assert_eq!(diff_view_model.items()[1].type_(), DiffElementType::Placeholder);

            diff_view_model.items()[1].show_hidden_cells();
            assert_eq!(diff_view_model.items()[1].type_(), DiffElementType::Unchanged);
            assert_eq!(diff_view_model.items()[2].type_(), DiffElementType::Unchanged);
            assert_event_args(&event_args, 1, 1, vec![diff_view_model.items()[1].clone(), diff_view_model.items()[2].clone()]);

            suite.verify_change_event_is_not_fired(&diff_view_model).await;
        },
    );
}

#[test]
fn diff_insert_2() {
    run_diff_test(
        vec![
            code("var a = 1;"),
            code("var b = 2;"),
            code("var c = 3;"),
            code("var d = 4;"),
            code("var e = 5;"),
            code("var f = 6;"),
            code("var g = 7;"),
        ],
        vec![
            code("var h = 8;"),
            code("var a = 1;"),
            code("var b = 2;"),
            code("var c = 3;"),
            code("var d = 4;"),
            code("var e = 5;"),
            code("var f = 6;"),
            code("var g = 7;"),
        ],
        |suite, model, disposables| async move {
            let event_dispatcher = disposables.add_rc(NotebookDiffEditorEventDispatcher::new());
            suite.set_diff_result(DiffResult { changes: changes(&[(0, 0, 0, 1), (0, 6, 1, 6)]), quit_early: false });

            let diff_view_model = suite.create_view_model(&model, &disposables, &event_dispatcher);
            let event_args = listen_to_items(&disposables, &diff_view_model);
            suite.compute_diff(&diff_view_model).await;

            assert_eq!(event_args.borrow().as_ref().and_then(|event| event.first_change_index), Some(0));
            assert_eq!(diff_view_model.items().len(), 2);
            assert_eq!(diff_view_model.items()[0].type_(), DiffElementType::Insert);
            assert_eq!(diff_view_model.items()[1].type_(), DiffElementType::Placeholder);

            diff_view_model.items()[1].show_hidden_cells();
            for index in 1..=7 {
                assert_eq!(diff_view_model.items()[index].type_(), DiffElementType::Unchanged);
            }
            assert_event_args(&event_args, 1, 1, diff_view_model.items()[1..].to_vec());

            diff_view_model.items()[1].as_cell().hide_unchanged_cells();

            assert_eq!(diff_view_model.items().len(), 2);
            assert_eq!(diff_view_model.items()[0].type_(), DiffElementType::Insert);
            assert_eq!(diff_view_model.items()[1].type_(), DiffElementType::Placeholder);
            assert_event_args(&event_args, 1, 7, vec![diff_view_model.items()[1].clone()]);

            suite.verify_change_event_is_not_fired(&diff_view_model).await;
        },
    );
}

#[test]
fn diff_insert_3() {
    run_diff_test(
        vec![
            code("var a = 1;"),
            code("var b = 2;"),
            code("var c = 3;"),
            code("var d = 4;"),
            code("var e = 5;"),
            code("var f = 6;"),
            code("var g = 7;"),
        ],
        vec![
            code("var a = 1;"),
            code("var b = 2;"),
            code("var c = 3;"),
            code("var d = 4;"),
            code("var h = 8;"),
            code("var e = 5;"),
            code("var f = 6;"),
            code("var g = 7;"),
        ],
        |suite, model, disposables| async move {
            suite.set_diff_result(DiffResult { changes: changes(&[(4, 0, 4, 1)]), quit_early: false });

            let diff_view_model = suite.create_view_model(&model, &disposables, &suite.event_dispatcher);
            let event_args = listen_to_items(&disposables, &diff_view_model);
            suite.compute_diff(&diff_view_model).await;

            assert_eq!(diff_view_model.items()[0].type_(), DiffElementType::Placeholder);
            assert_eq!(diff_view_model.items()[1].type_(), DiffElementType::Insert);
            assert_eq!(diff_view_model.items()[2].type_(), DiffElementType::Placeholder);

            diff_view_model.items()[0].show_hidden_cells();
            assert_eq!(diff_view_model.items()[0].type_(), DiffElementType::Unchanged);
            assert_eq!(diff_view_model.items()[1].type_(), DiffElementType::Unchanged);
            assert_eq!(diff_view_model.items()[2].type_(), DiffElementType::Unchanged);
            assert_eq!(diff_view_model.items()[3].type_(), DiffElementType::Unchanged);
            assert_eq!(diff_view_model.items()[4].type_(), DiffElementType::Insert);
            assert_eq!(diff_view_model.items()[5].type_(), DiffElementType::Placeholder);
            assert_event_args(&event_args, 0, 1, diff_view_model.items()[0..4].to_vec());

            diff_view_model.items()[5].show_hidden_cells();
            assert_eq!(diff_view_model.items()[0].type_(), DiffElementType::Unchanged);
            assert_eq!(diff_view_model.items()[1].type_(), DiffElementType::Unchanged);
            assert_eq!(diff_view_model.items()[2].type_(), DiffElementType::Unchanged);
            assert_eq!(diff_view_model.items()[3].type_(), DiffElementType::Unchanged);
            assert_eq!(diff_view_model.items()[4].type_(), DiffElementType::Insert);
            assert_eq!(diff_view_model.items()[5].type_(), DiffElementType::Unchanged);
            assert_event_args(&event_args, 5, 1, diff_view_model.items()[5..].to_vec());

            diff_view_model.items()[0].as_cell().hide_unchanged_cells();
            assert_eq!(diff_view_model.items()[0].type_(), DiffElementType::Placeholder);
            assert_eq!(diff_view_model.items()[1].type_(), DiffElementType::Insert);
            assert_eq!(diff_view_model.items()[2].type_(), DiffElementType::Unchanged);
            assert_event_args(&event_args, 0, 4, diff_view_model.items()[0..1].to_vec());

            suite.verify_change_event_is_not_fired(&diff_view_model).await;
        },
    );
}

#[test]
fn lcs() {
    run_diff_test(
        vec![
            ("# Description", "markdown", CellKind::Markup, Vec::new(), Some(meta_empty())),
            js("x = 3", Vec::new(), meta(Some(true), Some(1.0))),
            js("x", out("someId", 3), meta(Some(false), Some(1.0))),
            js("x", Vec::new(), meta(Some(false), None)),
        ],
        vec![
            ("# Description", "markdown", CellKind::Markup, Vec::new(), Some(meta_empty())),
            js("x = 3", Vec::new(), meta(Some(true), Some(1.0))),
            js("x", Vec::new(), meta(Some(false), None)),
            js("x", out("someId", 3), meta(Some(false), Some(1.0))),
        ],
        |_suite, model, _disposables| async move {
            let diff_result = lcs_diff(&model);
            assert_eq!(change_tuples(&diff_result), vec![(2, 0, 2, 1), (3, 1, 4, 0)]);
        },
    );
}

/// `{ metadata: {} }`.
fn meta_empty() -> NotebookCellMetadata {
    meta(None, None).expect("metadata")
}

#[test]
fn lcs_2() {
    run_diff_test(
        vec![
            ("# Description", "markdown", CellKind::Markup, Vec::new(), Some(meta_empty())),
            js("x = 3", Vec::new(), meta(Some(true), Some(1.0))),
            js("x", out("someId", 3), meta(Some(false), Some(1.0))),
            js("x", Vec::new(), meta(Some(false), None)),
            js("x = 5", Vec::new(), Some(empty_metadata())),
            js("x", Vec::new(), Some(empty_metadata())),
            js("x", out("someId", 5), Some(empty_metadata())),
        ],
        vec![
            ("# Description", "markdown", CellKind::Markup, Vec::new(), Some(meta_empty())),
            js("x = 3", Vec::new(), meta(Some(true), Some(1.0))),
            js("x", Vec::new(), meta(Some(false), None)),
            js("x", out("someId", 3), meta(Some(false), Some(1.0))),
            js("x = 5", Vec::new(), Some(empty_metadata())),
            js("x", out("someId", 5), Some(empty_metadata())),
            js("x", Vec::new(), Some(empty_metadata())),
        ],
        |_suite, model, _disposables| async move {
            let mut diff_result = lcs_diff(&model);
            pretty_changes(&model.original().notebook, &model.modified().notebook, &mut diff_result);

            assert_eq!(change_tuples(&diff_result), vec![(2, 0, 2, 1), (3, 1, 4, 0), (5, 0, 5, 1), (6, 1, 7, 0)]);
        },
    );
}

#[test]
fn lcs_3() {
    run_diff_test(
        vec![
            code("var a = 1;"),
            code("var b = 2;"),
            code("var c = 3;"),
            code("var d = 4;"),
            code("var e = 5;"),
            code("var f = 6;"),
            code("var g = 7;"),
        ],
        vec![
            code("var a = 1;"),
            code("var b = 2;"),
            code("var c = 3;"),
            code("var d = 4;"),
            code("var h = 8;"),
            code("var e = 5;"),
            code("var f = 6;"),
            code("var g = 7;"),
        ],
        |_suite, model, _disposables| async move {
            let mut diff_result = lcs_diff(&model);
            pretty_changes(&model.original().notebook, &model.modified().notebook, &mut diff_result);

            assert_eq!(change_tuples(&diff_result), vec![(4, 0, 4, 1)]);
        },
    );
}

#[test]
fn diff_output() {
    run_diff_test(
        vec![js("x", out("someOtherId", 3), meta(Some(false), Some(3.0))), js("y", out("someOtherId", 4), meta(Some(false), Some(3.0)))],
        vec![js("x", out("someOtherId", 3), meta(Some(false), Some(3.0))), js("y", out("someOtherId", 5), meta(Some(false), Some(3.0)))],
        |suite, model, disposables| async move {
            suite.set_diff_result(lcs_diff(&model));

            let diff_view_model = suite.create_view_model(&model, &disposables, &suite.event_dispatcher);
            suite.compute_diff(&diff_view_model).await;

            assert_eq!(diff_view_model.items().len(), 2);
            assert_eq!(diff_view_model.items()[0].type_(), DiffElementType::Placeholder);
            diff_view_model.items()[0].show_hidden_cells();
            assert!(diff_view_model.items()[0].as_cell().check_if_outputs_modified().is_none());
            assert_eq!(diff_view_model.items()[1].type_(), DiffElementType::Modified);

            suite.verify_change_event_is_not_fired(&diff_view_model).await;
        },
    );
}

#[test]
fn diff_output_fast_check() {
    run_diff_test(
        vec![js("x", out("someOtherId", 3), meta(Some(false), Some(3.0))), js("y", out("someOtherId", 4), meta(Some(false), Some(3.0)))],
        vec![js("x", out("someOtherId", 3), meta(Some(false), Some(3.0))), js("y", out("someOtherId", 5), meta(Some(false), Some(3.0)))],
        |suite, model, disposables| async move {
            suite.set_diff_result(lcs_diff(&model));

            let diff_view_model = suite.create_view_model(&model, &disposables, &suite.event_dispatcher);
            suite.compute_diff(&diff_view_model).await;

            assert_eq!(diff_view_model.items().len(), 2);
            assert_eq!(diff_view_model.items()[0].type_(), DiffElementType::Placeholder);
            diff_view_model.items()[0].show_hidden_cells();
            let items = diff_view_model.items();
            assert!(items[0].original().expect("original").text_model.equal(&items[0].modified().expect("modified").text_model));
            assert!(!items[1].original().expect("original").text_model.equal(&items[1].modified().expect("modified").text_model));

            suite.verify_change_event_is_not_fired(&diff_view_model).await;
        },
    );
}
