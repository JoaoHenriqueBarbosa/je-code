//! Porte de `src/vs/workbench/contrib/mergeEditor/test/browser/model.test.ts` (suíte `merge editor model`), caso a caso
//! e na mesma ordem.
//!
//! Notas de tradução:
//! - O original deixa o `ensureNoDisposablesAreLeakedInTestSuite()` comentado (`todo: renable when failing case is
//!   found`); aqui cada caso cria o guarda de vazamentos antes de rodar, e ele confere ao fim.
//! - `createModelServices(disposables)` é o `create_model_services_in` (os serviços de linguagem ficam no
//!   `DisposableStore` do caso) e `createTextModel(text, languageId)` é o `instantiate_text_model_with_languages` de
//!   `test_text_model`, com o mesmo `ILanguageService` e `IUndoRedoService` do `createInstance(MergeEditorModel, ...)`.
//! - `instantiationService.createInstance(MergeEditorModel, ...)` é `MergeEditorModel::new` com esses dois serviços;
//!   `new MergeEditorTelemetry(NullTelemetryService)` é o `NullTelemetryServiceShape`.
//! - `async`/`await` dos casos são `run_with_real_timers` (o mocha do original roda no relógio real) e o `await` de
//!   `Promise`; `testMergeModel(options, fn)` recebe o corpo como um fecho assíncrono.
//! - `class MergeModelInterface extends Disposable` é a struct [`MergeModelInterface`] com um `Disposable`, que
//!   registra os quatro modelos de texto e o modelo de merge na mesma ordem do original.
//! - `getProjections(): unknown` devolve a struct [`Projections`], e o `assert.deepStrictEqual` do objeto é a
//!   comparação dela com a struct esperada; os `string[]` viram `Vec<String>`.
//! - `this.mergeModel.modifiedBaseRanges.get()[idx]` é o `Rc<ModifiedBaseRange>` do índice, e o `throw new Error()` do
//!   índice inexistente é o pânico de `errors::throw`.
//! - O `computeDiff` do `IMergeDiffComputer` do teste usa o `LegacyLinesDiffComputer` (o `linesDiffComputers.getLegacy()`)
//!   e devolve a `Promise` já resolvida; o `.filter(isDefined)` não filtra nada, pois o `toRangeMapping` sempre devolve valor.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
#[path = "../../je-editor/tests/support/test_text_model.rs"]
mod test_text_model;

use std::future::Future;
use std::rc::Rc;

use common::{ensure_no_disposables_are_leaked_in_test_suite, run_with_real_timers};
use je_base::JsString;
use je_base::errors::{self, Error};
use je_base::lifecycle::{Disposable, DisposableStore, IDisposable};
use je_base::observable::{IReader, transaction};
use je_base::promise::Promise;
use je_editor::diff::legacy_lines_diff_computer::LegacyLinesDiffComputer;
use je_editor::language_configuration_registry::LanguageConfigurationService;
use je_editor::language_service::LanguageService;
use je_editor::diff::lines_diff_computer::{ILinesDiffComputer, LinesDiffComputerOptions};
use je_editor::edit_operation::SingleEditOperation;
use je_editor::model::EndOfLinePreference;
use je_editor::range::Range;
use je_editor::text_model::TextModel;
use je_platform::telemetry::telemetry_utils::NullTelemetryServiceShape;
use je_workbench::contrib::merge_editor::model::diff_computer::{
    IMergeDiffComputer, MergeDiffComputerResult, to_line_range, to_range_mapping,
};
use je_workbench::contrib::merge_editor::model::mapping::DetailedLineRangeMapping;
use je_workbench::contrib::merge_editor::model::merge_editor_model::{InputData, MergeEditorModel, MergeEditorModelOptions};
use je_workbench::contrib::merge_editor::model::modified_base_range::InputNumber;
use je_workbench::contrib::merge_editor::telemetry::MergeEditorTelemetry;
use test_text_model::{LanguageModelServices, create_model_services, create_text_model, instantiate_text_model_with_languages};

#[test]
fn prepend_line() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    run_with_real_timers(async {
        test_merge_model(
            MergeModelOptions {
                language_id: "plaintext",
                base: "line1\nline2",
                input1: "0\nline1\nline2",
                input2: "0\nline1\nline2",
                result: "",
                reset_result: false,
            },
            |model| async move {
                assert_eq!(
                    model.get_projections(),
                    projections(&["⟦⟧₀line1", "line2"], &["⟦0", "⟧₀line1", "line2"], &["⟦0", "⟧₀line1", "line2"], &["⟦⟧{unrecognized}₀"])
                );

                model.toggle_conflict(0, InputNumber::One);
                assert_eq!(model.get_result(), "0\nline1\nline2");

                model.toggle_conflict(0, InputNumber::Two);
                assert_eq!(model.get_result(), "0\n0\nline1\nline2");
            },
        )
        .await;
    });
}

#[test]
fn empty_base() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    run_with_real_timers(async {
        test_merge_model(
            MergeModelOptions {
                language_id: "plaintext",
                base: "",
                input1: "input1",
                input2: "input2",
                result: "",
                reset_result: false,
            },
            |model| async move {
                assert_eq!(model.get_projections(), projections(&["⟦⟧₀"], &["⟦input1⟧₀"], &["⟦input2⟧₀"], &["⟦⟧{base}₀"]));

                model.toggle_conflict(0, InputNumber::One);
                assert_eq!(model.get_result(), "input1");

                model.toggle_conflict(0, InputNumber::Two);
                assert_eq!(model.get_result(), "input2");
            },
        )
        .await;
    });
}

#[test]
fn can_merge_word_changes() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    run_with_real_timers(async {
        test_merge_model(
            MergeModelOptions {
                language_id: "plaintext",
                base: "hello",
                input1: "hallo",
                input2: "helloworld",
                result: "",
                reset_result: false,
            },
            |model| async move {
                assert_eq!(model.get_projections(), projections(&["⟦hello⟧₀"], &["⟦hallo⟧₀"], &["⟦helloworld⟧₀"], &["⟦⟧{unrecognized}₀"]));

                model.toggle_conflict(0, InputNumber::One);
                model.toggle_conflict(0, InputNumber::Two);

                assert_eq!(model.get_result(), "halloworld");
            },
        )
        .await;
    });
}

#[test]
fn can_combine_insertions_at_end_of_document() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    run_with_real_timers(async {
        test_merge_model(
            MergeModelOptions {
                language_id: "plaintext",
                base: "Zürich\nBern\nBasel\nChur\nGenf\nThun",
                input1: "Zürich\nBern\nChur\nDavos\nGenf\nThun\nfunction f(b:boolean) {}",
                input2: "Zürich\nBern\nBasel (FCB)\nChur\nGenf\nThun\nfunction f(a:number) {}",
                result: "Zürich\nBern\nBasel\nChur\nDavos\nGenf\nThun",
                reset_result: false,
            },
            |model| async move {
                assert_eq!(
                    model.get_projections(),
                    projections(
                        &["Zürich", "Bern", "⟦Basel", "⟧₀Chur", "⟦⟧₁Genf", "Thun⟦⟧₂"],
                        &["Zürich", "Bern", "⟦⟧₀Chur", "⟦Davos", "⟧₁Genf", "Thun", "⟦function f(b:boolean) {}⟧₂"],
                        &["Zürich", "Bern", "⟦Basel (FCB)", "⟧₀Chur", "⟦⟧₁Genf", "Thun", "⟦function f(a:number) {}⟧₂"],
                        &["Zürich", "Bern", "⟦Basel", "⟧{base}₀Chur", "⟦Davos", "⟧{1✓}₁Genf", "Thun⟦⟧{base}₂"],
                    )
                );

                model.toggle_conflict(2, InputNumber::One);
                model.toggle_conflict(2, InputNumber::Two);

                assert_eq!(
                    model.get_result(),
                    "Zürich\nBern\nBasel\nChur\nDavos\nGenf\nThun\nfunction f(b:boolean) {}\nfunction f(a:number) {}"
                );
            },
        )
        .await;
    });
}

#[test]
fn conflicts_are_reset() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    run_with_real_timers(async {
        test_merge_model(
            MergeModelOptions {
                language_id: "typescript",
                base: "import { h } from 'vs/base/browser/dom';\nimport { Disposable, IDisposable } from 'vs/base/common/lifecycle';\nimport { CodeEditorWidget } from 'vs/editor/browser/widget/codeEditorWidget';\nimport { EditorOption } from 'vs/editor/common/config/editorOptions';\nimport { autorun, IReader, observableFromEvent, ObservableValue } from 'vs/workbench/contrib/audioCues/browser/observable';\nimport { LineRange } from 'vs/workbench/contrib/mergeEditor/browser/model/lineRange';\n",
                input1: "import { h } from 'vs/base/browser/dom';\nimport { Disposable, IDisposable } from 'vs/base/common/lifecycle';\nimport { observableSignalFromEvent } from 'vs/base/common/observable';\nimport { CodeEditorWidget } from 'vs/editor/browser/widget/codeEditorWidget';\nimport { autorun, IReader, observableFromEvent } from 'vs/workbench/contrib/audioCues/browser/observable';\nimport { LineRange } from 'vs/workbench/contrib/mergeEditor/browser/model/lineRange';\n",
                input2: "import { h } from 'vs/base/browser/dom';\nimport { Disposable, IDisposable } from 'vs/base/common/lifecycle';\nimport { CodeEditorWidget } from 'vs/editor/browser/widget/codeEditorWidget';\nimport { autorun, IReader, observableFromEvent, ObservableValue } from 'vs/workbench/contrib/audioCues/browser/observable';\nimport { LineRange } from 'vs/workbench/contrib/mergeEditor/browser/model/lineRange';\n",
                result: "import { h } from 'vs/base/browser/dom';\r\nimport { Disposable, IDisposable } from 'vs/base/common/lifecycle';\r\nimport { observableSignalFromEvent } from 'vs/base/common/observable';\r\nimport { CodeEditorWidget } from 'vs/editor/browser/widget/codeEditorWidget';\r\n<<<<<<< Updated upstream\r\nimport { autorun, IReader, observableFromEvent, ObservableValue } from 'vs/workbench/contrib/audioCues/browser/observable';\r\n=======\r\nimport { autorun, IReader, observableFromEvent } from 'vs/workbench/contrib/audioCues/browser/observable';\r\n>>>>>>> Stashed changes\r\nimport { LineRange } from 'vs/workbench/contrib/mergeEditor/browser/model/lineRange';\r\n",
                reset_result: false,
            },
            |model| async move {
                assert_eq!(
                    model.get_projections(),
                    projections(
                        &[
                            "import { h } from 'vs/base/browser/dom';",
                            "import { Disposable, IDisposable } from 'vs/base/common/lifecycle';",
                            "⟦⟧₀import { CodeEditorWidget } from 'vs/editor/browser/widget/codeEditorWidget';",
                            "⟦import { EditorOption } from 'vs/editor/common/config/editorOptions';",
                            "import { autorun, IReader, observableFromEvent, ObservableValue } from 'vs/workbench/contrib/audioCues/browser/observable';",
                            "⟧₁import { LineRange } from 'vs/workbench/contrib/mergeEditor/browser/model/lineRange';",
                            "",
                        ],
                        &[
                            "import { h } from 'vs/base/browser/dom';",
                            "import { Disposable, IDisposable } from 'vs/base/common/lifecycle';",
                            "⟦import { observableSignalFromEvent } from 'vs/base/common/observable';",
                            "⟧₀import { CodeEditorWidget } from 'vs/editor/browser/widget/codeEditorWidget';",
                            "⟦import { autorun, IReader, observableFromEvent } from 'vs/workbench/contrib/audioCues/browser/observable';",
                            "⟧₁import { LineRange } from 'vs/workbench/contrib/mergeEditor/browser/model/lineRange';",
                            "",
                        ],
                        &[
                            "import { h } from 'vs/base/browser/dom';",
                            "import { Disposable, IDisposable } from 'vs/base/common/lifecycle';",
                            "⟦⟧₀import { CodeEditorWidget } from 'vs/editor/browser/widget/codeEditorWidget';",
                            "⟦import { autorun, IReader, observableFromEvent, ObservableValue } from 'vs/workbench/contrib/audioCues/browser/observable';",
                            "⟧₁import { LineRange } from 'vs/workbench/contrib/mergeEditor/browser/model/lineRange';",
                            "",
                        ],
                        &[
                            "import { h } from 'vs/base/browser/dom';",
                            "import { Disposable, IDisposable } from 'vs/base/common/lifecycle';",
                            "⟦import { observableSignalFromEvent } from 'vs/base/common/observable';",
                            "⟧{1✓}₀import { CodeEditorWidget } from 'vs/editor/browser/widget/codeEditorWidget';",
                            "⟦<<<<<<< Updated upstream",
                            "import { autorun, IReader, observableFromEvent, ObservableValue } from 'vs/workbench/contrib/audioCues/browser/observable';",
                            "=======",
                            "import { autorun, IReader, observableFromEvent } from 'vs/workbench/contrib/audioCues/browser/observable';",
                            ">>>>>>> Stashed changes",
                            "⟧{unrecognized}₁import { LineRange } from 'vs/workbench/contrib/mergeEditor/browser/model/lineRange';",
                            "",
                        ],
                    )
                );
            },
        )
        .await;
    });
}

#[test]
fn auto_solve_equal_edits() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    run_with_real_timers(async {
        test_merge_model(
            MergeModelOptions {
                language_id: "javascript",
                base: "const { readFileSync } = require('fs');\n\nlet paths = process.argv.slice(2);\nmain(paths);\n\nfunction main(paths) {\n    // print the welcome message\n    printMessage();\n\n    let data = getLineCountInfo(paths);\n    console.log(\"Lines: \" + data.totalLineCount);\n}\n\n/**\n * Prints the welcome message\n*/\nfunction printMessage() {\n    console.log(\"Welcome To Line Counter\");\n}\n\n/**\n * @param {string[]} paths\n*/\nfunction getLineCountInfo(paths) {\n    let lineCounts = paths.map(path => ({ path, count: getLinesLength(readFileSync(path, 'utf8')) }));\n    return {\n        totalLineCount: lineCounts.reduce((acc, { count }) => acc + count, 0),\n        lineCounts,\n    };\n}\n\n/**\n * @param {string} str\n */\nfunction getLinesLength(str) {\n    return str.split('\\n').length;\n}\n",
                input1: "const { readFileSync } = require('fs');\n\nlet paths = process.argv.slice(2);\nmain(paths);\n\nfunction main(paths) {\n    // print the welcome message\n    printMessage();\n\n    const data = getLineCountInfo(paths);\n    console.log(\"Lines: \" + data.totalLineCount);\n}\n\nfunction printMessage() {\n    console.log(\"Welcome To Line Counter\");\n}\n\n/**\n * @param {string[]} paths\n*/\nfunction getLineCountInfo(paths) {\n    let lineCounts = paths.map(path => ({ path, count: getLinesLength(readFileSync(path, 'utf8')) }));\n    return {\n        totalLineCount: lineCounts.reduce((acc, { count }) => acc + count, 0),\n        lineCounts,\n    };\n}\n\n/**\n * @param {string} str\n */\nfunction getLinesLength(str) {\n    return str.split('\\n').length;\n}\n",
                input2: "const { readFileSync } = require('fs');\n\nlet paths = process.argv.slice(2);\nrun(paths);\n\nfunction run(paths) {\n    // print the welcome message\n    printMessage();\n\n    const data = getLineCountInfo(paths);\n    console.log(\"Lines: \" + data.totalLineCount);\n}\n\nfunction printMessage() {\n    console.log(\"Welcome To Line Counter\");\n}\n\n/**\n * @param {string[]} paths\n*/\nfunction getLineCountInfo(paths) {\n    let lineCounts = paths.map(path => ({ path, count: getLinesLength(readFileSync(path, 'utf8')) }));\n    return {\n        totalLineCount: lineCounts.reduce((acc, { count }) => acc + count, 0),\n        lineCounts,\n    };\n}\n\n/**\n * @param {string} str\n */\nfunction getLinesLength(str) {\n    return str.split('\\n').length;\n}\n",
                result: "<<<<<<< uiae\n>>>>>>> Stashed changes",
                reset_result: true,
            },
            |model| async move {
                model.merge_model.reset().await.expect("reset");

                assert_eq!(
                    model.get_result(),
                    "const { readFileSync } = require('fs');\n\nlet paths = process.argv.slice(2);\nrun(paths);\n\nfunction run(paths) {\n    // print the welcome message\n    printMessage();\n\n    const data = getLineCountInfo(paths);\n    console.log(\"Lines: \" + data.totalLineCount);\n}\n\nfunction printMessage() {\n    console.log(\"Welcome To Line Counter\");\n}\n\n/**\n * @param {string[]} paths\n*/\nfunction getLineCountInfo(paths) {\n    let lineCounts = paths.map(path => ({ path, count: getLinesLength(readFileSync(path, 'utf8')) }));\n    return {\n        totalLineCount: lineCounts.reduce((acc, { count }) => acc + count, 0),\n        lineCounts,\n    };\n}\n\n/**\n * @param {string} str\n */\nfunction getLinesLength(str) {\n    return str.split('\\n').length;\n}\n"
                );
            },
        )
        .await;
    });
}

async fn test_merge_model<F, Fut>(options: MergeModelOptions, f: F)
where
    F: FnOnce(Rc<MergeModelInterface>) -> Fut,
    Fut: Future<Output = ()>,
{
    let disposables = DisposableStore::new();
    let services = create_model_services_in(&disposables);
    let model_interface = disposables.add_rc(Rc::new(MergeModelInterface::new(&options, &services)));
    model_interface.merge_model.on_initialized().await.expect("onInitialized");
    f(model_interface).await;
    disposables.dispose();
}

/// `createModelServices(disposables)`: os serviços de linguagem ficam no `DisposableStore` do caso.
fn create_model_services_in(disposables: &Rc<DisposableStore>) -> LanguageModelServices {
    let language_service = disposables.add_rc(LanguageService::new(false));
    let language_configuration_service = disposables.add_rc(LanguageConfigurationService::new(language_service.clone()));
    LanguageModelServices { undo_redo_service: create_model_services(), language_service, language_configuration_service }
}

struct MergeModelOptions {
    language_id: &'static str,
    input1: &'static str,
    input2: &'static str,
    base: &'static str,
    result: &'static str,
    reset_result: bool,
}

fn to_small_numbers_dec(value: usize) -> String {
    const SMALL_NUMBERS: [char; 10] = ['₀', '₁', '₂', '₃', '₄', '₅', '₆', '₇', '₈', '₉'];
    value.to_string().chars().map(|c| SMALL_NUMBERS[c.to_digit(10).expect("dígito") as usize]).collect()
}

/// O objeto que `getProjections()` devolve.
#[derive(Debug, PartialEq, Eq)]
struct Projections {
    base: Vec<String>,
    input1: Vec<String>,
    input2: Vec<String>,
    result: Vec<String>,
}

fn projections(base: &[&str], input1: &[&str], input2: &[&str], result: &[&str]) -> Projections {
    let to_vec = |lines: &[&str]| lines.iter().map(|l| (*l).to_string()).collect::<Vec<String>>();
    Projections { base: to_vec(base), input1: to_vec(input1), input2: to_vec(input2), result: to_vec(result) }
}

/// O `IMergeDiffComputer` do teste (ver as notas do módulo).
struct TestMergeDiffComputer;

impl IMergeDiffComputer for TestMergeDiffComputer {
    fn compute_diff(&self, text_model1: &Rc<TextModel>, text_model2: &Rc<TextModel>, _reader: &dyn IReader) -> Promise<MergeDiffComputerResult> {
        let result = LegacyLinesDiffComputer.compute_diff(
            &text_model1.get_lines_content(),
            &text_model2.get_lines_content(),
            &LinesDiffComputerOptions { ignore_trim_whitespace: false, max_computation_time_ms: 10000.0, compute_moves: false, extend_to_subwords: None },
        );
        let changes = result
            .changes
            .iter()
            .map(|c| {
                DetailedLineRangeMapping::new(
                    to_line_range(&c.original),
                    text_model1.clone(),
                    to_line_range(&c.modified),
                    text_model2.clone(),
                    c.inner_changes.as_ref().map(|inner| inner.iter().map(to_range_mapping).collect()),
                )
            })
            .collect();
        Promise::resolved(MergeDiffComputerResult { diffs: Some(changes) })
    }
}

struct MergeModelInterface {
    base: Rc<Disposable>,
    merge_model: Rc<MergeEditorModel>,
}

impl IDisposable for MergeModelInterface {
    fn dispose(&self) {
        self.base.dispose();
    }
}

impl MergeModelInterface {
    fn new(options: &MergeModelOptions, services: &LanguageModelServices) -> Self {
        let base = Disposable::new();
        let input1_text_model = instantiate_text_model_with_languages(services, options.input1, options.language_id);
        base.register(input1_text_model.clone());
        let input2_text_model = instantiate_text_model_with_languages(services, options.input2, options.language_id);
        base.register(input2_text_model.clone());
        let base_text_model = instantiate_text_model_with_languages(services, options.base, options.language_id);
        base.register(base_text_model.clone());
        let result_text_model = instantiate_text_model_with_languages(services, options.result, options.language_id);
        base.register(result_text_model.clone());

        let merge_model = MergeEditorModel::new(
            base_text_model,
            InputData { text_model: input1_text_model, description: Some(String::new()), detail: Some(String::new()), title: Some(String::new()) },
            InputData { text_model: input2_text_model, description: Some(String::new()), detail: Some(String::new()), title: Some(String::new()) },
            result_text_model,
            Rc::new(TestMergeDiffComputer),
            MergeEditorModelOptions { reset_result: options.reset_result },
            Rc::new(MergeEditorTelemetry::new(Rc::new(NullTelemetryServiceShape))),
            services.language_service.clone(),
            services.undo_redo_service.clone(),
        );
        base.register(merge_model.clone());
        Self { base, merge_model }
    }

    fn get_projections(&self) -> Projections {
        /// `LabeledRange`.
        struct LabeledRange {
            range: Range,
            label: String,
        }
        fn apply_ranges(text_model: &TextModel, ranges: &[LabeledRange]) {
            let edits: Vec<SingleEditOperation> = ranges
                .iter()
                .map(|LabeledRange { range, label }| {
                    let text = format!("⟦{}⟧{}", text_model.get_value_in_range(range), label);
                    SingleEditOperation::new(*range, Some(JsString::from(text)), false)
                })
                .collect();
            text_model.apply_edits(&edits);
        }
        fn lines_of(text_model: &TextModel) -> Vec<String> {
            text_model.get_value_with(EndOfLinePreference::Lf, false).to_string_lossy().split('\n').map(str::to_string).collect()
        }

        let base_ranges = self.merge_model.modified_base_ranges.get();

        let base_text_model = create_text_model(&self.merge_model.base.get_value().to_string_lossy());
        apply_ranges(
            &base_text_model,
            &base_ranges
                .iter()
                .enumerate()
                .map(|(idx, r)| LabeledRange { range: r.base_range.to_exclusive_range(), label: to_small_numbers_dec(idx) })
                .collect::<Vec<_>>(),
        );

        let input1_text_model = create_text_model(&self.merge_model.input1.text_model.get_value().to_string_lossy());
        apply_ranges(
            &input1_text_model,
            &base_ranges
                .iter()
                .enumerate()
                .map(|(idx, r)| LabeledRange { range: r.input1_range.to_exclusive_range(), label: to_small_numbers_dec(idx) })
                .collect::<Vec<_>>(),
        );

        let input2_text_model = create_text_model(&self.merge_model.input2.text_model.get_value().to_string_lossy());
        apply_ranges(
            &input2_text_model,
            &base_ranges
                .iter()
                .enumerate()
                .map(|(idx, r)| LabeledRange { range: r.input2_range.to_exclusive_range(), label: to_small_numbers_dec(idx) })
                .collect::<Vec<_>>(),
        );

        let result_text_model = create_text_model(&self.merge_model.result_text_model.get_value().to_string_lossy());
        apply_ranges(
            &result_text_model,
            &base_ranges
                .iter()
                .enumerate()
                .map(|(idx, r)| LabeledRange {
                    range: self.merge_model.get_line_range_in_result(&r.base_range, None).to_exclusive_range(),
                    label: format!("{{{}}}{}", self.merge_model.get_state(r).get(), to_small_numbers_dec(idx)),
                })
                .collect::<Vec<_>>(),
        );

        let result = Projections {
            base: lines_of(&base_text_model),
            input1: lines_of(&input1_text_model),
            input2: lines_of(&input2_text_model),
            result: lines_of(&result_text_model),
        };
        base_text_model.dispose();
        input1_text_model.dispose();
        input2_text_model.dispose();
        result_text_model.dispose();
        result
    }

    fn toggle_conflict(&self, conflict_idx: usize, input_number: InputNumber) {
        let Some(base_range) = self.merge_model.modified_base_ranges.get().get(conflict_idx).cloned() else {
            errors::throw(Error::new(""));
        };
        let state = self.merge_model.get_state(&base_range).get();
        transaction(|tx| {
            self.merge_model.set_state(&base_range, state.toggle(input_number), true, tx, false);
        });
    }

    fn get_result(&self) -> String {
        self.merge_model.result_text_model.get_value().to_string_lossy()
    }
}
