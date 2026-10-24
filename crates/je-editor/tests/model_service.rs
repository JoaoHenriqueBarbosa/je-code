//! Porte de `src/vs/editor/test/common/services/modelService.test.ts` (suíte `ModelService`), caso a caso e na mesma
//! ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` é o `LeakGuard` criado no começo de cada caso, e o `disposables` da
//!   suíte é o próprio guarda. O `setup` (`TestConfigurationService` com `files.eol` e a raiz `/myroot`, e o
//!   `IModelService` do `createModelServices`) é [`setup`]: o `ModelService` recebe a configuração, o
//!   `TestTextResourcePropertiesService` e o `UndoRedoService` de teste.
//! - `disposables.add(createTextModel(...))` é o modelo descartado no fim do caso (`TextModel` tem `dispose`, não
//!   é `IDisposable`); `createAndRegisterTextBuffer` guarda o `disposable` do buffer no guarda.
//! - `ModelService._computeEdits` é `ModelService::compute_edits`; `assert.deepStrictEqual` é `assert_eq!` sobre
//!   as `SingleEditOperation`.
//! - `model.pushEditOperations(null, [{ range, text }], () => [new Selection(...)])` é `push_edit_operations` com
//!   o computador de seleção; `pushEditOperations([], edits, null)` é a versão sem ele.
//! - `platform.isWindows` é `Platform::detect().is_windows`; `URI.file(path)` leva a plataforma de caminho do processo.
//! - O bloco `GENERATE_TESTS` do original (constante `false`, um gerador aleatório) não é um caso e não é portado.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod support;

use std::rc::Rc;

use common::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite};
use je_base::JsString;
use je_base::dynamic::Dynamic;
use je_base::lifecycle::IDisposable;
use je_base::path::{Environment, ProcessEnvironment};
use je_base::platform::Platform;
use je_base::uri::URI;
use je_editor::edit_operation::{EditOperation, SingleEditOperation};
use je_editor::model::{DefaultEndOfLine, ValidEditOperation};
use je_editor::model_service::ModelService;
use je_editor::piece_tree_text_buffer::PieceTreeTextBuffer;
use je_editor::range::Range;
use je_editor::selection::Selection;
use je_editor::text_model::{TextModel, TextSource, create_text_buffer};
use je_editor::undo_redo_service::UndoRedoService;
use je_platform::configuration::service::IConfigurationService;
use je_platform::configuration::test_configuration_service::TestConfigurationService;
use support::test_text_model::create_text_model;
use support::test_text_resource_properties_service::TestTextResourcePropertiesService;

fn is_windows() -> bool {
    Platform::detect().is_windows
}

fn file_uri(unix_path: &str, windows_path: &str) -> URI {
    URI::file(if is_windows() { windows_path } else { unix_path }, ProcessEnvironment.platform())
}

/// O `setup` da suíte.
fn setup(ds: &LeakGuard) -> Rc<ModelService> {
    let config_service = TestConfigurationService::new(None);
    let _ = config_service.set_user_configuration("files", Dynamic::object([("eol", Dynamic::string("\n"))]), None);
    let _ = config_service.set_user_configuration(
        "files",
        Dynamic::object([("eol", Dynamic::string("\r\n"))]),
        Some(&file_uri("/myroot", "c:\\myroot")),
    );

    let configuration_service: Rc<dyn IConfigurationService> = config_service;
    let model_service = ModelService::new(
        configuration_service.clone(),
        TestTextResourcePropertiesService::new(configuration_service),
        UndoRedoService::for_tests(),
    );
    ds.add_rc(model_service)
}

fn create_and_register_text_buffer(ds: &LeakGuard, value: impl Into<TextSource>, default_eol: DefaultEndOfLine) -> PieceTreeTextBuffer {
    let text_buffer = create_text_buffer(value.into(), default_eol);
    ds.add(text_buffer.disposable());
    text_buffer
}

fn insert_text(range: Range, text: &str) -> SingleEditOperation {
    SingleEditOperation::new(range, Some(JsString::from(text)), false)
}

fn replace_move(range: Range, text: &str) -> SingleEditOperation {
    EditOperation::replace_move(range, Some(JsString::from(text)))
}

fn four_lines(first: &str, third: &str) -> Vec<String> {
    vec![first.to_string(), "and this is line number two".to_string(), third.to_string(), "and finished with the fourth.".to_string()]
}

#[test]
fn eol_setting_respected_depending_on_root() {
    let ds = ensure_no_disposables_are_leaked_in_test_suite();
    let model_service = setup(&ds);

    let model1 = model_service.create_model("farboo", None, None, false);
    let model2 = model_service.create_model("farboo", None, Some(&file_uri("/myroot/myfile.txt", "c:\\myroot\\myfile.txt")), false);
    let model3 = model_service.create_model("farboo", None, Some(&file_uri("/other/myfile.txt", "c:\\other\\myfile.txt")), false);

    assert_eq!(model1.get_options().default_eol, DefaultEndOfLine::Lf);
    assert_eq!(model2.get_options().default_eol, DefaultEndOfLine::CrLf);
    assert_eq!(model3.get_options().default_eol, DefaultEndOfLine::Lf);

    model1.dispose();
    model2.dispose();
    model3.dispose();
}

#[test]
fn compute_edits_no_change() {
    let ds = ensure_no_disposables_are_leaked_in_test_suite();

    let model = create_text_model(&four_lines("This is line one", "it is followed by #3").join("\n"));

    let text_buffer =
        create_and_register_text_buffer(&ds, four_lines("This is line one", "it is followed by #3").join("\n"), DefaultEndOfLine::Lf);

    let actual = ModelService::compute_edits(&model, &text_buffer);

    assert_eq!(actual, Vec::<SingleEditOperation>::new());
    model.dispose();
}

#[test]
fn compute_edits_first_line_changed() {
    let ds = ensure_no_disposables_are_leaked_in_test_suite();

    let model = create_text_model(&four_lines("This is line one", "it is followed by #3").join("\n"));

    let text_buffer =
        create_and_register_text_buffer(&ds, four_lines("This is line One", "it is followed by #3").join("\n"), DefaultEndOfLine::Lf);

    let actual = ModelService::compute_edits(&model, &text_buffer);

    assert_eq!(actual, vec![replace_move(Range::new(1, 1, 2, 1), "This is line One\n")]);
    model.dispose();
}

#[test]
fn compute_edits_eol_changed() {
    let ds = ensure_no_disposables_are_leaked_in_test_suite();

    let model = create_text_model(&four_lines("This is line one", "it is followed by #3").join("\n"));

    let text_buffer =
        create_and_register_text_buffer(&ds, four_lines("This is line one", "it is followed by #3").join("\r\n"), DefaultEndOfLine::Lf);

    let actual = ModelService::compute_edits(&model, &text_buffer);

    assert_eq!(actual, Vec::<SingleEditOperation>::new());
    model.dispose();
}

#[test]
fn compute_edits_eol_and_other_change_1() {
    let ds = ensure_no_disposables_are_leaked_in_test_suite();

    let model = create_text_model(&four_lines("This is line one", "it is followed by #3").join("\n"));

    let text_buffer =
        create_and_register_text_buffer(&ds, four_lines("This is line One", "It is followed by #3").join("\r\n"), DefaultEndOfLine::Lf);

    let actual = ModelService::compute_edits(&model, &text_buffer);

    assert_eq!(
        actual,
        vec![replace_move(
            Range::new(1, 1, 4, 1),
            &["This is line One", "and this is line number two", "It is followed by #3", ""].join("\r\n")
        )]
    );
    model.dispose();
}

#[test]
fn compute_edits_eol_and_other_change_2() {
    let ds = ensure_no_disposables_are_leaked_in_test_suite();

    let model = create_text_model(
        &[
            "package main", // 1
            "func foo() {", // 2
            "}",            // 3
        ]
        .join("\n"),
    );

    let text_buffer = create_and_register_text_buffer(
        &ds,
        [
            "package main", // 1
            "func foo() {", // 2
            "}",            // 3
            "",
        ]
        .join("\r\n"),
        DefaultEndOfLine::Lf,
    );

    let actual = ModelService::compute_edits(&model, &text_buffer);

    assert_eq!(actual, vec![replace_move(Range::new(3, 2, 3, 2), "\r\n")]);
    model.dispose();
}

/// `assertComputeEdits(lines1, lines2)`.
fn assert_compute_edits(lines1: &[&str], lines2: &[&str]) {
    let model = create_text_model(&lines1.join("\n"));
    let text_buffer = create_text_buffer(lines2.join("\n").into(), DefaultEndOfLine::Lf);

    // compute required edits
    let edits = ModelService::compute_edits(&model, &text_buffer);

    // apply edits
    model.push_edit_operations(Some(Vec::new()), &edits, None, None, None);

    assert_eq!(model.get_value().to_string_lossy(), lines2.join("\n"));
    text_buffer.disposable().dispose();
    model.dispose();
}

#[test]
fn generated1() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let file1 = ["pram", "okctibad", "pjuwtemued", "knnnm", "u", ""];
    let file2 = ["tcnr", "rxwlicro", "vnzy", "", "", "pjzcogzur", "ptmxyp", "dfyshia", "pee", "ygg"];
    assert_compute_edits(&file1, &file2);
}

#[test]
fn generated2() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let file1 = ["", "itls", "hrilyhesv", ""];
    let file2 = ["vdl", "", "tchgz", "bhx", "nyl"];
    assert_compute_edits(&file1, &file2);
}

#[test]
fn generated3() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let file1 = ["ubrbrcv", "wv", "xodspybszt", "s", "wednjxm", "fklajt", "fyfc", "lvejgge", "rtpjlodmmk", "arivtgmjdm"];
    let file2 = ["s", "qj", "tu", "ur", "qerhjjhyvx", "t"];
    assert_compute_edits(&file1, &file2);
}

#[test]
fn generated4() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let file1 = ["ig", "kh", "hxegci", "smvker", "pkdmjjdqnv", "vgkkqqx", "", "jrzeb"];
    let file2 = ["yk", ""];
    assert_compute_edits(&file1, &file2);
}

#[test]
fn does_insertions_in_the_middle_of_the_document() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let file1 = ["line 1", "line 2", "line 3"];
    let file2 = ["line 1", "line 2", "line 5", "line 3"];
    assert_compute_edits(&file1, &file2);
}

#[test]
fn does_insertions_at_the_end_of_the_document() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let file1 = ["line 1", "line 2", "line 3"];
    let file2 = ["line 1", "line 2", "line 3", "line 4"];
    assert_compute_edits(&file1, &file2);
}

#[test]
fn does_insertions_at_the_beginning_of_the_document() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let file1 = ["line 1", "line 2", "line 3"];
    let file2 = ["line 0", "line 1", "line 2", "line 3"];
    assert_compute_edits(&file1, &file2);
}

#[test]
fn does_replacements() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let file1 = ["line 1", "line 2", "line 3"];
    let file2 = ["line 1", "line 7", "line 3"];
    assert_compute_edits(&file1, &file2);
}

#[test]
fn does_deletions() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let file1 = ["line 1", "line 2", "line 3"];
    let file2 = ["line 1", "line 3"];
    assert_compute_edits(&file1, &file2);
}

#[test]
fn does_insert_replace_and_delete() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let file1 = ["line 1", "line 2", "line 3", "line 4", "line 5"];
    let file2 = [
        "line 0", // insert line 0
        "line 1",
        "replace line 2", // replace line 2
        "line 3",
        // delete line 4
        "line 5",
    ];
    assert_compute_edits(&file1, &file2);
}

/// `model.pushEditOperations(null, [{ range: new Range(1, 5, 1, 5), text: '1' }], () => [new Selection(1, 5, 1, 5)])`.
fn push_one_at_end_of_text(model: &TextModel) {
    model.push_edit_operations(
        None,
        &[insert_text(Range::new(1, 5, 1, 5), "1")],
        Some(&mut |_: &[ValidEditOperation]| Some(vec![Selection::new(1, 5, 1, 5)])),
        None,
        None,
    );
}

#[test]
fn maintains_undo_for_same_resource_and_same_content() {
    let ds = ensure_no_disposables_are_leaked_in_test_suite();
    let model_service = setup(&ds);
    let resource = URI::parse("file://test.txt", false);

    // create a model
    let model1 = model_service.create_model("text", None, Some(&resource), false);
    // make an edit
    push_one_at_end_of_text(&model1);
    assert_eq!(model1.get_value().to_string_lossy(), "text1");
    // dispose it
    model_service.destroy_model(&resource);

    // create a new model with the same content
    let model2 = model_service.create_model("text1", None, Some(&resource), false);
    // undo
    model2.undo();
    assert_eq!(model2.get_value().to_string_lossy(), "text");
    // dispose it
    model_service.destroy_model(&resource);
}

#[test]
fn maintains_version_id_and_alternative_version_id_for_same_resource_and_same_content() {
    let ds = ensure_no_disposables_are_leaked_in_test_suite();
    let model_service = setup(&ds);
    let resource = URI::parse("file://test.txt", false);

    // create a model
    let model1 = model_service.create_model("text", None, Some(&resource), false);
    // make an edit
    push_one_at_end_of_text(&model1);
    assert_eq!(model1.get_value().to_string_lossy(), "text1");
    let version_id = model1.get_version_id();
    let alternative_version_id = model1.get_alternative_version_id();
    // dispose it
    model_service.destroy_model(&resource);

    // create a new model with the same content
    let model2 = model_service.create_model("text1", None, Some(&resource), false);
    assert_eq!(model2.get_version_id(), version_id);
    assert_eq!(model2.get_alternative_version_id(), alternative_version_id);
    // dispose it
    model_service.destroy_model(&resource);
}

#[test]
fn does_not_maintain_undo_for_same_resource_and_different_content() {
    let ds = ensure_no_disposables_are_leaked_in_test_suite();
    let model_service = setup(&ds);
    let resource = URI::parse("file://test.txt", false);

    // create a model
    let model1 = model_service.create_model("text", None, Some(&resource), false);
    // make an edit
    push_one_at_end_of_text(&model1);
    assert_eq!(model1.get_value().to_string_lossy(), "text1");
    // dispose it
    model_service.destroy_model(&resource);

    // create a new model with the same content
    let model2 = model_service.create_model("text2", None, Some(&resource), false);
    // undo
    model2.undo();
    assert_eq!(model2.get_value().to_string_lossy(), "text2");
    // dispose it
    model_service.destroy_model(&resource);
}

#[test]
fn set_value_should_clear_undo_stack() {
    let ds = ensure_no_disposables_are_leaked_in_test_suite();
    let model_service = setup(&ds);
    let resource = URI::parse("file://test.txt", false);

    let model = model_service.create_model("text", None, Some(&resource), false);
    push_one_at_end_of_text(&model);
    assert_eq!(model.get_value().to_string_lossy(), "text1");

    model.set_value("text2".into());
    model.undo();
    assert_eq!(model.get_value().to_string_lossy(), "text2");
    // dispose it
    model_service.destroy_model(&resource);
}
