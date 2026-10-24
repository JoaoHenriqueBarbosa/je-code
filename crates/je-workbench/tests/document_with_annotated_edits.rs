//! Porte de `src/vs/workbench/contrib/editTelemetry/test/browser/documentWithAnnotatedEdits.test.ts` (suíte `Documents
//! with Annotated Edits`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` é o `LeakGuard`, criado dentro do relógio virtual de cada caso
//!   ([`run`]); `runWithFakedTimers({}, async () => ...)` é o `run_with_faked_timers`, e `await timeout(ms)` é
//!   [`sleep`]. O contexto (`setup`) vive dentro do corpo assíncrono, portanto até depois da cauda assíncrona do
//!   `CombineStreamedChanges`, e é descartado no fim do caso.
//! - `new TestInstantiationService(new ServiceCollection(), false, undefined, true)` é um `InstantiationService` sobre um
//!   `ServiceCollection` descartado junto do `DisposableStore` do caso. `stubInstance(DiffService, { computeDiff:
//!   computeStringDiff(original, modified, { maxComputationTimeMs: 500 }, 'advanced') })` é o `DiffService` de verdade
//!   sobre um `IEditorWorkerService` que usa o `compute_string_edit_from_diff` padrão (o mesmo `computeStringDiff`, com
//!   500 ms e o algoritmo `advanced`). `createInstance(CombineStreamedChanges<EditSourceData>, document)` é o
//!   `create_instance` do mesmo serviço.
//! - `TestSourceDocument` é a struct de mesmo nome sobre um `ObservableValue` com o delta `AnnotatedEdit`;
//!   `edit.mapData(() => data)` é `map_data(|_| data.clone())`, e `edit.applyOnText(...)` é `apply_on_text`.
//! - `edit.replacements[0]?.data.source.toString()` é o `Display` do `EditSource` do primeiro trecho, ou `None` quando a
//!   edição composta é vazia. `assert.deepStrictEqual` de objetos literais é `assert_eq!` sobre structs locais com os
//!   mesmos campos, na mesma ordem.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use std::cell::RefCell;
use std::future::Future;
use std::rc::Rc;

use common::{ensure_no_disposables_are_leaked_in_test_suite, run_with_faked_timers, timeout};
use je_base::lifecycle::{Disposable, DisposableStore, IDisposable};
use je_base::observable::observables::observable_value::ObservableValue;
use je_base::observable::{IObservable, Observable, Settable, observable_value, run_on_change};
use je_base::promise::Promise;
use je_base::uri::URI;
use je_editor::edits::edit::BaseEdit;
use je_editor::edits::string_edit::{BaseStringEdit, StringEdit};
use je_editor::editor_worker::{I_EDITOR_WORKER_SERVICE, IEditorWorkerService, WordRanges};
use je_editor::offset_range::OffsetRange;
use je_editor::range::Range;
use je_editor::text::abstract_text::StringText;
use je_editor::text_model_edit_source::{ChatApplyEditsData, EditSources, TextModelEditSource};
use je_instantiation::{InstantiationService, ServiceCollection};
use je_workbench::contrib::edit_telemetry::browser::helpers::document_with_annotated_edits::{
    AnnotatedEdit, CombineStreamedChanges, EditSourceData, IDocumentWithAnnotatedEdits, MinimizeEditsProcessor,
};

// region: infraestrutura do caso

/// `ensureNoDisposablesAreLeakedInTestSuite()` e `runWithFakedTimers({}, async () => ...)`.
fn run(body: impl Future<Output = ()> + 'static) {
    run_with_faked_timers(async move {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        body.await;
    });
}

/// `await timeout(ms)`.
async fn sleep(ms: u64) {
    timeout(ms).await.expect("timeout");
}

/// O `IEditorWorkerService` sobre o qual o `DiffService` calcula a diferença: o `computeStringEditFromDiff` padrão.
struct TestEditorWorkerService;

impl IEditorWorkerService for TestEditorWorkerService {
    fn can_compute_word_ranges(&self, _resource: &URI) -> bool {
        false
    }

    fn compute_word_ranges(&self, _resource: &URI, _range: Range) -> Promise<Option<WordRanges>> {
        Promise::resolved(None)
    }
}

/// `TestSourceDocument`.
struct TestSourceDocument {
    base: Rc<Disposable>,
    value: Rc<ObservableValue<Rc<StringText>, AnnotatedEdit<EditSourceData>>>,
}

impl TestSourceDocument {
    fn new(initial_value: &str) -> Rc<Self> {
        Rc::new(Self { base: Disposable::new(), value: observable_value("value", Rc::new(StringText::new(initial_value))) })
    }

    /// `apply(edit, source)`.
    fn apply(&self, edit: &StringEdit, source: &Rc<TextModelEditSource>) {
        let data = EditSourceData::new(source.clone());
        let next = Rc::new(edit.apply_on_text(&self.value.get()));
        self.value.set_with_change(next, None, AnnotatedEdit { edit: edit.map_data(|_| data.clone()) });
    }
}

impl IDocumentWithAnnotatedEdits<EditSourceData> for TestSourceDocument {
    fn value(&self) -> IObservable<Rc<StringText>> {
        self.value.to_observable()
    }

    fn wait_for_queue(&self) -> Promise<()> {
        Promise::resolved(())
    }
}

impl IDisposable for TestSourceDocument {
    fn dispose(&self) {
        self.base.dispose();
    }
}

/// `{ start, endExclusive, newText }`.
type Replacement = (i32, i32, String);

/// O objeto `{ value, source, replacements }` do `changes` do `setup`.
#[derive(Debug, PartialEq)]
struct Change {
    value: String,
    source: Option<String>,
    replacements: Vec<Replacement>,
}

fn replacements_of(edit: &je_editor::edits::string_edit::AnnotatedStringEdit<EditSourceData>) -> Vec<Replacement> {
    edit.replacements().iter().map(|r| (r.replace_range.start, r.replace_range.end_exclusive, r.new_text.to_string_lossy())).collect()
}

struct Context {
    disposables: Rc<DisposableStore>,
    document: Rc<TestSourceDocument>,
    changes: Rc<RefCell<Vec<Change>>>,
}

fn setup(initial_value: &str) -> Context {
    let disposables = DisposableStore::new();
    let editor_worker_service: Rc<dyn IEditorWorkerService> = Rc::new(TestEditorWorkerService);
    let instantiation_service =
        disposables.add_rc(InstantiationService::new(ServiceCollection::new().with(I_EDITOR_WORKER_SERVICE, editor_worker_service)));
    let document = disposables.add_rc(TestSourceDocument::new(initial_value));
    let document_dyn: Rc<dyn IDocumentWithAnnotatedEdits<EditSourceData>> = document.clone();
    let combined = disposables.add_rc(instantiation_service.create_instance::<CombineStreamedChanges<EditSourceData>>(document_dyn));
    let changes: Rc<RefCell<Vec<Change>>> = Rc::new(RefCell::new(Vec::new()));
    let sink = changes.clone();
    disposables.add(run_on_change::<Rc<StringText>, AnnotatedEdit<EditSourceData>>(combined.value(), move |value, _previous, edits| {
        let edit = AnnotatedEdit::compose_all(edits);
        sink.borrow_mut().push(Change {
            value: value.value.to_string_lossy(),
            source: edit.replacements().first().map(|r| r.data.source.to_string()),
            replacements: replacements_of(&edit),
        });
    }));
    Context { disposables, document, changes }
}

fn chat_edit() -> Rc<TextModelEditSource> {
    Rc::new(EditSources::chat_apply_edits(&ChatApplyEditsData {
        session_id: Some("session-1".to_owned()),
        request_id: Some("request-1".to_owned()),
        language_id: "typescript".to_owned(),
        mode: Some("agent".to_owned()),
        ..Default::default()
    }))
}

// endregion

#[test]
fn collapses_streamed_chat_edits_into_one_diff() {
    run(async {
        let context = setup("");
        let source = chat_edit();
        sleep(0).await;

        context.document.apply(&StringEdit::insert(0, "a"), &source);
        sleep(500).await;
        context.document.apply(&StringEdit::insert(1, "b"), &source);
        sleep(1100).await;

        assert_eq!(
            *context.changes.borrow(),
            vec![Change { value: "ab".to_owned(), source: Some("ai/chat/sidebar".to_owned()), replacements: vec![(0, 0, "ab".to_owned())] }]
        );
        context.disposables.dispose();
    });
}

#[test]
fn preserves_ordering_when_a_user_edit_interrupts_streamed_chat_edits() {
    run(async {
        let context = setup("");
        sleep(0).await;

        context.document.apply(&StringEdit::insert(0, "a"), &chat_edit());
        sleep(500).await;
        context.document.apply(&StringEdit::insert(1, "U"), &Rc::new(EditSources::cursor("type", None)));
        sleep(1100).await;

        assert_eq!(
            *context.changes.borrow(),
            vec![
                Change { value: "a".to_owned(), source: Some("ai/chat/sidebar".to_owned()), replacements: vec![(0, 0, "a".to_owned())] },
                Change { value: "aU".to_owned(), source: Some("user".to_owned()), replacements: vec![(1, 1, "U".to_owned())] },
            ]
        );
        context.disposables.dispose();
    });
}

#[test]
fn minimizes_common_prefixes_and_suffixes() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let disposables = DisposableStore::new();
    let document = disposables.add_rc(TestSourceDocument::new("hello world"));
    let document_dyn: Rc<dyn IDocumentWithAnnotatedEdits<EditSourceData>> = document.clone();
    let minimized = disposables.add_rc(MinimizeEditsProcessor::new(document_dyn));
    let changes: Rc<RefCell<Vec<(String, Vec<Replacement>)>>> = Rc::new(RefCell::new(Vec::new()));
    let sink = changes.clone();
    disposables.add(run_on_change::<Rc<StringText>, AnnotatedEdit<EditSourceData>>(minimized.value(), move |value, _previous, edits| {
        let edit = AnnotatedEdit::compose_all(edits);
        sink.borrow_mut().push((value.value.to_string_lossy(), replacements_of(&edit)));
    }));

    document.apply(&StringEdit::replace(OffsetRange::of_length(11), "hello brave world"), &chat_edit());

    assert_eq!(*changes.borrow(), vec![("hello brave world".to_owned(), vec![(5, 5, " brave".to_owned())])]);
    disposables.dispose();
}
