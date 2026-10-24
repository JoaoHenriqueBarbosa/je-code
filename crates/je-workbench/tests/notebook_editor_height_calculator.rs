//! Porte de `src/vs/workbench/contrib/notebook/test/browser/diff/editorHeightCalculator.test.ts` (suíte `NotebookDiff
//! EditorHeightCalculator`, com as sub-suítes `Hide Unchanged Regions` e `Show Unchanged Regions`), caso a caso e na mesma ordem.
//!
//! Os 5 casos de cada sub-suíte estão portados (10 `#[test]`, nos módulos [`hide_unchanged_regions`] e
//! [`show_unchanged_regions`]).
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` é o `LeakGuard` criado no começo de cada caso, e o `teardown`
//!   (`disposables.dispose()`) roda no fim dele. O `setup` (os dois mocks de serviço e o `DiffEditorHeightCalculatorService`)
//!   é [`Suite::new`]; o `TestConfigurationService` do escopo da sub-suíte é criado por caso (o `LeakGuard` descarta o `Emitter`
//!   dele). Os casos `async` rodam em `run_with_faked_timers`.
//! - `new class extends mock<ITextModelService>() { createModelReference }` é o [`TestTextModelService`], e a referência
//!   `{ dispose: () => {}, object: { textEditorModel, getLanguageId } }` é um `Reference` imortal de um [`ResolvedModel`]
//!   (o `getLanguageId` não é lido). `resource === original` compara a URI textual.
//! - `new class extends mock<IEditorWorkerService>() { computeDiff }` é o [`TestEditorWorkerService`]: os membros que o
//!   `mock` deixa ausentes lançam `TypeError`. O `new DefaultLinesDiffComputer()` e o `identical` são os do original.
//! - `fontInfo.lineHeight` (18) é a constante [`LINE_HEIGHT`], e `FontInfo` (só `lineHeight` e `fontSize` são lidos) não é
//!   construído.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
#[path = "../../je-editor/tests/support/test_text_model.rs"]
mod test_text_model;

use std::any::Any;
use std::cell::RefCell;
use std::future::Future;
use std::rc::Rc;

use common::{ensure_no_disposables_are_leaked_in_test_suite, run_with_faked_timers};
use je_base::JsString;
use je_base::dynamic::Dynamic;
use je_base::event::Event;
use je_base::lifecycle::{DisposableStore, IDisposable};
use je_base::promise::Promise;
use je_base::uri::URI;
use je_editor::cursor_common::CursorSimpleModel;
use je_editor::diff::default_lines_diff_computer::default_lines_diff_computer::DefaultLinesDiffComputer;
use je_editor::diff::document_diff_provider::{DocumentDiff, DocumentDiffProviderOptions};
use je_editor::diff::lines_diff_computer::ILinesDiffComputer;
use je_editor::editor_worker::{DiffAlgorithmName, IEditorWorkerService, WordRanges};
use je_editor::model::TextSnapshot;
use je_editor::range::Range;
use je_editor::text_model::TextModel;
use je_platform::configuration::service::IConfigurationService;
use je_platform::configuration::test_configuration_service::TestConfigurationService;
use je_workbench::common::editor::editor_model::IEditorModel;
use je_workbench::contrib::notebook::browser::diff::diff_cell_editor_options::get_editor_padding;
use je_workbench::contrib::notebook::browser::diff::diff_element_view_model::HEIGHT_OF_HIDDEN_LINES_REGION_IN_DIFF_EDITOR;
use je_workbench::contrib::notebook::browser::diff::editor_height_calculator::{DiffEditorHeightCalculatorService, IDiffEditorHeightCalculatorService};
use je_workbench::services::files_configuration::files_configuration_service::Readonly;
use je_workbench::services::textmodel_resolver::resolver_service::{ITextEditorModel, ITextModelService, TextModelReference};
use test_text_model::create_text_model;

/// `fontInfo.lineHeight`.
const LINE_HEIGHT: f64 = 18.0;

/// O `object` da referência: `{ textEditorModel, getLanguageId: () => 'javascript' }`.
struct ResolvedModel {
    text_editor_model: Rc<TextModel>,
}

impl IDisposable for ResolvedModel {
    fn dispose(&self) {}
}

impl IEditorModel for ResolvedModel {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn on_will_dispose(&self) -> Event<()> {
        Event::none()
    }

    fn resolve(&self) -> Promise<()> {
        Promise::resolved(())
    }

    fn is_resolved(&self) -> bool {
        true
    }

    fn is_disposed(&self) -> bool {
        false
    }
}

impl ITextEditorModel for ResolvedModel {
    fn text_editor_model(&self) -> Option<Rc<TextModel>> {
        Some(self.text_editor_model.clone())
    }

    fn is_readonly(&self) -> Readonly {
        Readonly::Bool(false)
    }

    fn create_snapshot(&self) -> Option<Box<dyn TextSnapshot>> {
        None
    }
}

/// Os modelos do caso (`originalModel` e `modifiedModel`), que o `setup` lê quando o caso os cria.
#[derive(Default)]
struct Models {
    original: RefCell<Option<Rc<TextModel>>>,
    modified: RefCell<Option<Rc<TextModel>>>,
}

/// `textModelResolver`.
struct TestTextModelService {
    models: Rc<Models>,
    original: URI,
}

impl ITextModelService for TestTextModelService {
    fn create_model_reference(&self, resource: &URI) -> Promise<TextModelReference> {
        let model = if resource.to_string(false) == self.original.to_string(false) { self.models.original.borrow().clone() } else { self.models.modified.borrow().clone() };
        let object: Rc<dyn ITextEditorModel> = Rc::new(ResolvedModel { text_editor_model: model.expect("model") });
        Promise::resolved(TextModelReference::immortal(object))
    }
}

/// `editorWorkerService`.
struct TestEditorWorkerService {
    models: Rc<Models>,
    diff_computer: DefaultLinesDiffComputer,
}

/// `new Array(model.getLineCount()).fill(0).map((_, i) => model.getLineContent(i + 1))`.
fn lines_of(model: &TextModel) -> Vec<JsString> {
    (1..=model.get_line_count()).map(|line_number| model.get_line_content(line_number)).collect()
}

impl IEditorWorkerService for TestEditorWorkerService {
    fn can_compute_word_ranges(&self, _resource: &URI) -> bool {
        unreachable!("TypeError: canComputeWordRanges is not a function")
    }

    fn compute_word_ranges(&self, _resource: &URI, _range: Range) -> Promise<Option<WordRanges>> {
        unreachable!("TypeError: computeWordRanges is not a function")
    }

    fn compute_diff(&self, _original: &URI, _modified: &URI, options: &DocumentDiffProviderOptions, _algorithm: DiffAlgorithmName) -> Promise<Option<DocumentDiff>> {
        let original_lines = lines_of(self.models.original.borrow().as_ref().expect("originalModel"));
        let modified_lines = lines_of(self.models.modified.borrow().as_ref().expect("modifiedModel"));
        let result = self.diff_computer.compute_diff(&original_lines, &modified_lines, options);
        let join = |lines: &[JsString]| lines.iter().map(JsString::to_string_lossy).collect::<String>();
        let identical = join(&original_lines) == join(&modified_lines);

        Promise::resolved(Some(DocumentDiff { identical, quit_early: result.hit_timeout, changes: result.changes, moves: result.moves }))
    }
}

/// O `setup` de uma sub-suíte.
struct Suite {
    disposables: Rc<DisposableStore>,
    models: Rc<Models>,
    calculator: Rc<DiffEditorHeightCalculatorService>,
    original: URI,
    modified: URI,
    hide_unchanged_regions: bool,
}

impl Suite {
    fn new(guard: &common::LeakGuard, hide_unchanged_regions: bool) -> Self {
        let disposables = DisposableStore::new();
        let original = URI::parse("original", false);
        let modified = URI::parse("modified", false);
        let models = Rc::new(Models::default());

        let configuration = TestConfigurationService::new(Some(Dynamic::object([
            ("notebook", Dynamic::object([("diff", Dynamic::object([("ignoreMetadata", Dynamic::Bool(true))]))])),
            (
                "diffEditor",
                Dynamic::object([(
                    "hideUnchangedRegions",
                    Dynamic::object([
                        ("enabled", Dynamic::Bool(hide_unchanged_regions)),
                        ("minimumLineCount", Dynamic::Number(3.0)),
                        ("contextLineCount", Dynamic::Number(3.0)),
                    ]),
                )]),
            ),
        ])));
        guard.add_rc(configuration.on_did_change_configuration_emitter.clone());
        let configuration_service: Rc<dyn IConfigurationService> = configuration;

        let calculator = DiffEditorHeightCalculatorService::new(
            LINE_HEIGHT,
            Rc::new(TestTextModelService { models: models.clone(), original: original.clone() }),
            Rc::new(TestEditorWorkerService { models: models.clone(), diff_computer: DefaultLinesDiffComputer::new() }),
            configuration_service,
        );
        Self { disposables, models, calculator, original, modified, hide_unchanged_regions }
    }

    /// `createTextModel(lines)`: `createTextModelWithText(lines.join('\n'))`, descartado no `teardown`.
    fn create_text_model(&self, lines: &[String]) -> Rc<TextModel> {
        self.disposables.add_rc(create_text_model(&lines.join("\n")))
    }

    /// `originalModel = ...; modifiedModel = ...` e `await calculator.diffAndComputeHeight(original, modified)`.
    async fn diff_and_compute_height(&self, original_lines: &[String], modified_lines: &[String]) -> f64 {
        *self.models.original.borrow_mut() = Some(self.create_text_model(original_lines));
        *self.models.modified.borrow_mut() = Some(self.create_text_model(modified_lines));

        self.calculator.diff_and_compute_height(&self.original, &self.modified).await.expect("diffAndComputeHeight")
    }
}

/// `getExpectedHeight(visibleLineCount, unchangeRegionsHeight)`.
fn get_expected_height(visible_line_count: i64, unchange_regions_height: i64) -> f64 {
    let padding = get_editor_padding(visible_line_count);
    (visible_line_count as f64 * LINE_HEIGHT) + padding.top as f64 + padding.bottom as f64 + (unchange_regions_height as f64 * HEIGHT_OF_HIDDEN_LINES_REGION_IN_DIFF_EDITOR)
}

/// `createLines(count, linePrefix = 'Hello World')`.
fn create_lines(count: usize) -> Vec<String> {
    (0..count).map(|i| format!("Hello World {i}")).collect()
}

fn lines(list: &[&str]) -> Vec<String> {
    list.iter().map(|line| (*line).to_owned()).collect()
}

/// `lines.splice(start, deleteCount, ...items)`.
fn splice(lines: &mut Vec<String>, start: usize, delete_count: usize, items: &[&str]) {
    let new_items: Vec<String> = items.iter().map(|item| (*item).to_owned()).collect();
    lines.splice(start..start + delete_count, new_items);
}

/// `ensureNoDisposablesAreLeakedInTestSuite()`, o `setup`, o corpo `async` e o `teardown` de um caso.
fn run_case<Fut: Future<Output = ()> + 'static>(hide_unchanged_regions: bool, body: impl FnOnce(Rc<Suite>) -> Fut + 'static) {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let suite = Rc::new(Suite::new(&guard, hide_unchanged_regions));
    let run_suite = suite.clone();
    run_with_faked_timers(async move {
        body(run_suite).await;
    });
    suite.disposables.dispose();
    drop(guard);
}

async fn case_1_original_line_with_change_in_same_line(suite: Rc<Suite>) {
    let height = suite.diff_and_compute_height(&lines(&["Hello World"]), &lines(&["Foo Bar"])).await;
    let expected_height = get_expected_height(1, 0);

    assert_eq!(height, expected_height);
}

async fn case_1_original_line_with_insertion_of_a_new_line(suite: Rc<Suite>) {
    let height = suite.diff_and_compute_height(&lines(&["Hello World"]), &lines(&["Hello World", "Foo Bar"])).await;
    let expected_height = get_expected_height(2, 0);

    assert_eq!(height, expected_height);
}

async fn case_1_line_with_update_to_a_line_and_insert_of_a_new_line(suite: Rc<Suite>) {
    let height = suite.diff_and_compute_height(&lines(&["Hello World"]), &lines(&["Foo Bar", "Bar Baz"])).await;
    let expected_height = get_expected_height(2, 0);

    assert_eq!(height, expected_height);
}

async fn case_10_line_with_update_to_a_line_and_insert_of_a_new_line(suite: Rc<Suite>) {
    let mut modified_lines = create_lines(10);
    modified_lines.push("Foo Bar".to_owned());
    let height = suite.diff_and_compute_height(&create_lines(10), &modified_lines).await;
    let hide = suite.hide_unchanged_regions;
    let expected_height = get_expected_height(if hide { 4 } else { 11 }, if hide { 1 } else { 0 });

    assert_eq!(height, expected_height);
}

async fn case_50_lines_with_updates_deletions_and_inserts(suite: Rc<Suite>) {
    let original_lines = create_lines(60);
    let mut modified_lines = create_lines(60);
    modified_lines[3] = "Foo Bar".to_owned();
    splice(&mut modified_lines, 7, 3, &[]);
    splice(&mut modified_lines, 10, 0, &["Foo Bar1", "Foo Bar2", "Foo Bar3"]);
    splice(&mut modified_lines, 30, 0, &["", ""]);
    splice(&mut modified_lines, 40, 4, &[]);
    splice(&mut modified_lines, 50, 0, &["1", "2", "3", "4", "5"]);

    let height = suite.diff_and_compute_height(&original_lines, &modified_lines).await;
    let hide = suite.hide_unchanged_regions;
    let expected_height = get_expected_height(if hide { 50 } else { 70 }, if hide { 3 } else { 0 });

    assert_eq!(height, expected_height);
}

/// Uma sub-suíte (`suite(suiteTitle, ...)`) com `hideUnchangedRegions = suiteTitle.startsWith('Hide')`.
macro_rules! height_calculator_suite {
    ($module:ident, $hide_unchanged_regions:expr) => {
        mod $module {
            use super::*;

            #[test]
            fn one_original_line_with_change_in_same_line() {
                run_case($hide_unchanged_regions, case_1_original_line_with_change_in_same_line);
            }

            #[test]
            fn one_original_line_with_insertion_of_a_new_line() {
                run_case($hide_unchanged_regions, case_1_original_line_with_insertion_of_a_new_line);
            }

            #[test]
            fn one_line_with_update_to_a_line_and_insert_of_a_new_line() {
                run_case($hide_unchanged_regions, case_1_line_with_update_to_a_line_and_insert_of_a_new_line);
            }

            #[test]
            fn ten_line_with_update_to_a_line_and_insert_of_a_new_line() {
                run_case($hide_unchanged_regions, case_10_line_with_update_to_a_line_and_insert_of_a_new_line);
            }

            #[test]
            fn fifty_lines_with_updates_deletions_and_inserts() {
                run_case($hide_unchanged_regions, case_50_lines_with_updates_deletions_and_inserts);
            }
        }
    };
}

height_calculator_suite!(hide_unchanged_regions, true);
height_calculator_suite!(show_unchanged_regions, false);
