//! Porte de `src/vs/workbench/contrib/notebook/test/browser/notebookTextModel.test.ts` (suíte `NotebookTextModel`), caso a
//! caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` é o `LeakGuard` criado no começo de cada caso, depois do `suiteSetup`
//!   ([`suite_setup`], que cria o `DisposableStore`, o `setupInstantiationService` e o `ILanguageService`; o
//!   `suiteTeardown` é o `Drop` de [`Suite`], que roda depois da conferência de vazamento, como no mocha).
//! - `withTestNotebook` vem de `support::test_notebook_editor`; os callbacks `async` do original são síncronos (o
//!   `UndoRedoService` do porte é síncrono, então `await viewModel.undo()` é `view_model.undo()`).
//! - `textModel.applyEdits(edits, true, undefined, () => undefined, undefined, true)`, que se repete em quase todos os
//!   casos, é [`apply_edits`]; os casos que passam `false` no último argumento ou outra seleção chamam
//!   `apply_edits` do modelo direto.
//! - Uma `TestCell` usada onde se espera um `ICellDto2` é [`new_test_cell`] (o `TestCell::cell_dto`).
//! - Os literais de célula `['var a = 1;', 'javascript', CellKind.Code, [], {}]` são [`js_cell`], e as saídas e itens de
//!   saída são [`output_dto`] e [`item`].
//! - `index: Number.MAX_VALUE` e `index: -1` são `usize::MAX`: não existe `-1` em `usize`, e os dois caem no mesmo ramo
//!   (`_assertIndex` ou a leitura de `this.cells[index]`) e lançam.
//! - `assert.throws(...)` é `errors::catch(...).is_err()`.
//! - `assert.deepStrictEqual` das listas de edições compara as edições convertidas para valores dinâmicos
//!   ([`edit_value`]) com `assert_deep_strict_equal`; as edições do original são objetos, e `undefined` (ausente) é
//!   `None`.
//! - `assert.strictEqual(x, undefined)` e `assert.strictEqual(x, null)` são `is_none()`.
//! - Os `Uint8Array` comparados com `deepStrictEqual` são os bytes (`to_vec`).

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod support;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::JsString;
use je_base::buffer::VSBuffer;
use je_base::dynamic::Dynamic;
use je_base::errors;
use je_base::lifecycle::{DisposableStore, IDisposable};
use je_base::mime::mimes;
use je_editor::language_service::ILanguageService;
use je_editor::position::Position;
use je_instantiation::InstantiationService;
use je_workbench::contrib::notebook::common::model::notebook_text_model::{NotebookCellPosition, NotebookTextModel};
use je_workbench::contrib::notebook::common::notebook_common::{
    CellKind, ICell, ICellDto2, ICellEditOperation, ICellMetadataEdit, ICellOutput, ICellOutputEdit, ICellOutputEditByHandle,
    ICellOutputItemEdit, ICellPartialMetadataEdit, ICellReplaceEdit, IOutputDto, IOutputItemDto, ISelectionIndexState, ISelectionState,
    MOVE_CURSOR_1_LINE_COMMAND, NotebookCellInternalMetadata, NotebookTextModelChangedEvent, NotebookTextModelWillAddRemoveEvent,
};
use je_workbench::contrib::notebook::common::notebook_range::ICellRange;
use support::ext_host_workspace_support::assert_deep_strict_equal;
use support::test_notebook_editor::{
    MockNotebookCell, TestCell, empty_metadata, get_language_service, setup_instantiation_service, value_bytes_from_string, with_test_notebook,
};

/// O que o `suiteSetup` cria: o `disposables`, o `instantiationService` e o `languageService`.
struct Suite {
    disposables: Rc<DisposableStore>,
    _instantiation_service: Rc<InstantiationService>,
    language_service: Rc<dyn ILanguageService>,
}

/// O `suiteSetup`.
fn suite_setup() -> Suite {
    let disposables = DisposableStore::new();
    let instantiation_service = setup_instantiation_service(&disposables);
    let language_service = get_language_service(&instantiation_service);
    Suite { disposables, _instantiation_service: instantiation_service, language_service }
}

impl Drop for Suite {
    /// O `suiteTeardown(() => disposables.dispose())`.
    fn drop(&mut self) {
        self.disposables.dispose();
    }
}

/// `['source', 'javascript', CellKind.Code, [], {}]`.
fn js_cell(source: &'static str) -> MockNotebookCell {
    (source, "javascript", CellKind::Code, Vec::new(), Some(empty_metadata()))
}

/// `['source', 'javascript', CellKind.Code, outputs, {}]`.
fn js_cell_with_outputs(source: &'static str, outputs: Vec<IOutputDto>) -> MockNotebookCell {
    (source, "javascript", CellKind::Code, outputs, Some(empty_metadata()))
}

/// `{ mime, data: valueBytesFromString(data) }`.
fn item(mime: &str, data: &str) -> IOutputItemDto {
    IOutputItemDto { mime: mime.to_string(), data: value_bytes_from_string(data) }
}

/// `{ outputId, outputs }`.
fn output_dto(output_id: &str, outputs: Vec<IOutputItemDto>) -> IOutputDto {
    IOutputDto { output_id: output_id.to_string(), outputs, metadata: None }
}

/// `new TestCell(textModel.viewType, handle, source, 'javascript', CellKind.Code, [], languageService)` registrado em `ds`.
fn new_test_cell(ds: &DisposableStore, view_type: &str, handle: i64, source: &str, language_service: &Rc<dyn ILanguageService>) -> ICellDto2 {
    ds.add_rc(Rc::new(TestCell::new(view_type, handle, source, "javascript", CellKind::Code, Vec::new(), language_service.clone()))).cell_dto()
}

fn replace_edit(index: usize, count: usize, cells: Vec<ICellDto2>) -> ICellEditOperation {
    ICellEditOperation::Replace(ICellReplaceEdit { index, count, cells })
}

/// `{ index, editType: CellEditType.Output, append, outputs }`.
fn output_edit(index: usize, append: Option<bool>, outputs: Vec<IOutputDto>) -> ICellEditOperation {
    ICellEditOperation::Output(ICellOutputEdit { index, outputs, append })
}

/// `{ handle, editType: CellEditType.Output, append, outputs }`.
fn output_edit_by_handle(handle: i64, append: Option<bool>, outputs: Vec<IOutputDto>) -> ICellEditOperation {
    ICellEditOperation::OutputByHandle(ICellOutputEditByHandle { handle, outputs, append })
}

/// `{ editType: CellEditType.OutputItems, append, outputId, items }`.
fn output_items_edit(output_id: &str, append: Option<bool>, items: Vec<IOutputItemDto>) -> ICellEditOperation {
    ICellEditOperation::OutputItems(ICellOutputItemEdit { output_id: output_id.to_string(), items, append })
}

/// `{ editType: CellEditType.Metadata, index, metadata }`.
fn metadata_edit(index: usize, metadata: Dynamic) -> ICellEditOperation {
    ICellEditOperation::Metadata(ICellMetadataEdit { index, metadata })
}

/// `textModel.applyEdits(edits, true, undefined, () => undefined, undefined, true)`.
fn apply_edits(text_model: &NotebookTextModel, edits: &[ICellEditOperation]) -> bool {
    text_model.apply_edits(edits, true, None, &|| None, None, true)
}

/// `{ name: 'foo' }` e companhia: um objeto de um campo.
fn object_of(key: &str, value: Dynamic) -> Dynamic {
    Dynamic::object([(key, value)])
}

/// `{ kind: SelectionStateType.Index, focus: { start: 0, end: 1 }, selections: [{ start: 0, end: 1 }] }`.
fn index_selection_state() -> Option<ISelectionState> {
    Some(ISelectionState::Index(ISelectionIndexState {
        focus: ICellRange { start: 0, end: 1 },
        selections: vec![ICellRange { start: 0, end: 1 }],
    }))
}

/// `assert.deepStrictEqual(changeEvent!.endSelectionState?.selections, [{ start: 0, end: 1 }])`.
fn assert_end_selections_are_first_cell(event: &NotebookTextModelChangedEvent) {
    match &event.end_selection_state {
        Some(ISelectionState::Index(state)) => assert_eq!(state.selections, vec![ICellRange { start: 0, end: 1 }]),
        other => panic!("seleção final inesperada: {other:?}"),
    }
}

/// `{ source, language, cellKind, mime: undefined, outputs, metadata }` do teste de `computeEdits`.
fn dto(source: &str, language: &str, cell_kind: CellKind, outputs: Vec<IOutputDto>, metadata: Option<Dynamic>) -> ICellDto2 {
    ICellDto2 {
        source: JsString::from(source),
        language: language.to_string(),
        mime: None,
        cell_kind,
        outputs,
        metadata,
        internal_metadata: None,
        collapse_state: None,
    }
}

fn item_value(item: &IOutputItemDto) -> Dynamic {
    Dynamic::object([("mime", Dynamic::string(&item.mime)), ("data", Dynamic::typed_array(item.data.to_vec()))])
}

fn output_value(output: &IOutputDto) -> Dynamic {
    Dynamic::object([
        ("outputId", Dynamic::string(&output.output_id)),
        ("outputs", Dynamic::array(output.outputs.iter().map(item_value))),
        ("metadata", output.metadata.clone().unwrap_or(Dynamic::Undefined)),
    ])
}

fn dto_value(cell: &ICellDto2) -> Dynamic {
    Dynamic::object([
        ("source", Dynamic::string(&cell.source.to_string_lossy())),
        ("language", Dynamic::string(&cell.language)),
        ("cellKind", Dynamic::Number(cell.cell_kind as i32 as f64)),
        ("mime", cell.mime.as_deref().map_or(Dynamic::Undefined, Dynamic::string)),
        ("outputs", Dynamic::array(cell.outputs.iter().map(output_value))),
        ("metadata", cell.metadata.clone().unwrap_or(Dynamic::Undefined)),
        ("internalMetadata", cell.internal_metadata.as_ref().map_or(Dynamic::Undefined, |metadata| Dynamic::string(&format!("{metadata:?}")))),
    ])
}

fn append_value(append: Option<bool>) -> Dynamic {
    append.map_or(Dynamic::Undefined, Dynamic::Bool)
}

/// A edição como o objeto do original (só os tipos que os casos de `computeEdits` produzem).
fn edit_value(operation: &ICellEditOperation) -> Dynamic {
    let edit_type = Dynamic::Number(f64::from(operation.edit_type() as i32));
    match operation {
        ICellEditOperation::Metadata(edit) => {
            Dynamic::object([("editType", edit_type), ("index", Dynamic::Number(edit.index as f64)), ("metadata", edit.metadata.clone())])
        }
        ICellEditOperation::Replace(edit) => Dynamic::object([
            ("editType", edit_type),
            ("index", Dynamic::Number(edit.index as f64)),
            ("count", Dynamic::Number(edit.count as f64)),
            ("cells", Dynamic::array(edit.cells.iter().map(dto_value))),
        ]),
        ICellEditOperation::Output(edit) => Dynamic::object([
            ("editType", edit_type),
            ("index", Dynamic::Number(edit.index as f64)),
            ("outputs", Dynamic::array(edit.outputs.iter().map(output_value))),
            ("append", append_value(edit.append)),
        ]),
        ICellEditOperation::OutputItems(edit) => Dynamic::object([
            ("editType", edit_type),
            ("outputId", Dynamic::string(&edit.output_id)),
            ("items", Dynamic::array(edit.items.iter().map(item_value))),
            ("append", append_value(edit.append)),
        ]),
        other => panic!("edição inesperada: {other:?}"),
    }
}

/// `assert.deepStrictEqual(edits, expected)`.
#[track_caller]
fn assert_edits_equal(actual: &[ICellEditOperation], expected: &[ICellEditOperation]) {
    assert_deep_strict_equal(&Dynamic::array(actual.iter().map(edit_value)), &Dynamic::array(expected.iter().map(edit_value)));
}

const STD_OUT_MIME: &str = "application/vnd.code.notebook.stdout";
const STD_ERR_MIME: &str = "application/vnd.code.notebook.stderr";

#[test]
fn insert() {
    let suite = suite_setup();
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let language_service = suite.language_service.clone();
    with_test_notebook(
        vec![js_cell("var a = 1;"), js_cell("var b = 2;"), js_cell("var c = 3;"), js_cell("var d = 4;")],
        |editor, _view_model, ds, _services| {
            let text_model = &editor.text_model;
            apply_edits(
                text_model,
                &[
                    replace_edit(1, 0, vec![new_test_cell(ds, &text_model.view_type(), 5, "var e = 5;", &language_service)]),
                    replace_edit(3, 0, vec![new_test_cell(ds, &text_model.view_type(), 6, "var f = 6;", &language_service)]),
                ],
            );

            assert_eq!(text_model.cells().len(), 6);

            assert_eq!(text_model.cells()[1].get_value(), "var e = 5;");
            assert_eq!(text_model.cells()[4].get_value(), "var f = 6;");
        },
    );
}

#[test]
fn multiple_inserts_at_same_position() {
    let suite = suite_setup();
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let language_service = suite.language_service.clone();
    with_test_notebook(
        vec![js_cell("var a = 1;"), js_cell("var b = 2;"), js_cell("var c = 3;"), js_cell("var d = 4;")],
        |editor, _view_model, ds, _services| {
            let text_model = &editor.text_model;
            apply_edits(
                text_model,
                &[
                    replace_edit(1, 0, vec![new_test_cell(ds, &text_model.view_type(), 5, "var e = 5;", &language_service)]),
                    replace_edit(1, 0, vec![new_test_cell(ds, &text_model.view_type(), 6, "var f = 6;", &language_service)]),
                ],
            );

            assert_eq!(text_model.cells().len(), 6);

            assert_eq!(text_model.cells()[1].get_value(), "var e = 5;");
            assert_eq!(text_model.cells()[2].get_value(), "var f = 6;");
        },
    );
}

#[test]
fn delete() {
    let _suite = suite_setup();
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    with_test_notebook(
        vec![js_cell("var a = 1;"), js_cell("var b = 2;"), js_cell("var c = 3;"), js_cell("var d = 4;")],
        |editor, _view_model, _ds, _services| {
            let text_model = &editor.text_model;
            apply_edits(text_model, &[replace_edit(1, 1, vec![]), replace_edit(3, 1, vec![])]);

            assert_eq!(text_model.cells()[0].get_value(), "var a = 1;");
            assert_eq!(text_model.cells()[1].get_value(), "var c = 3;");
        },
    );
}

#[test]
fn delete_plus_insert() {
    let suite = suite_setup();
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let language_service = suite.language_service.clone();
    with_test_notebook(
        vec![js_cell("var a = 1;"), js_cell("var b = 2;"), js_cell("var c = 3;"), js_cell("var d = 4;")],
        |editor, _view_model, ds, _services| {
            let text_model = &editor.text_model;
            apply_edits(
                text_model,
                &[
                    replace_edit(1, 1, vec![]),
                    replace_edit(3, 0, vec![new_test_cell(ds, &text_model.view_type(), 5, "var e = 5;", &language_service)]),
                ],
            );
            assert_eq!(text_model.cells().len(), 4);

            assert_eq!(text_model.cells()[0].get_value(), "var a = 1;");
            assert_eq!(text_model.cells()[2].get_value(), "var e = 5;");
        },
    );
}

#[test]
fn delete_plus_insert_at_same_position() {
    let suite = suite_setup();
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let language_service = suite.language_service.clone();
    with_test_notebook(
        vec![js_cell("var a = 1;"), js_cell("var b = 2;"), js_cell("var c = 3;"), js_cell("var d = 4;")],
        |editor, _view_model, ds, _services| {
            let text_model = &editor.text_model;
            apply_edits(
                text_model,
                &[
                    replace_edit(1, 1, vec![]),
                    replace_edit(1, 0, vec![new_test_cell(ds, &text_model.view_type(), 5, "var e = 5;", &language_service)]),
                ],
            );

            assert_eq!(text_model.cells().len(), 4);
            assert_eq!(text_model.cells()[0].get_value(), "var a = 1;");
            assert_eq!(text_model.cells()[1].get_value(), "var e = 5;");
            assert_eq!(text_model.cells()[2].get_value(), "var c = 3;");
        },
    );
}

#[test]
fn replace_delete_plus_insert_at_same_position() {
    let suite = suite_setup();
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let language_service = suite.language_service.clone();
    with_test_notebook(
        vec![js_cell("var a = 1;"), js_cell("var b = 2;"), js_cell("var c = 3;"), js_cell("var d = 4;")],
        |editor, _view_model, ds, _services| {
            let text_model = &editor.text_model;
            apply_edits(
                text_model,
                &[replace_edit(1, 1, vec![new_test_cell(ds, &text_model.view_type(), 5, "var e = 5;", &language_service)])],
            );

            assert_eq!(text_model.cells().len(), 4);
            assert_eq!(text_model.cells()[0].get_value(), "var a = 1;");
            assert_eq!(text_model.cells()[1].get_value(), "var e = 5;");
            assert_eq!(text_model.cells()[2].get_value(), "var c = 3;");
        },
    );
}

#[test]
fn output() {
    let _suite = suite_setup();
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    with_test_notebook(vec![js_cell("var a = 1;")], |editor, _view_model, _ds, _services| {
        let text_model = &editor.text_model;

        // invalid index 1
        assert!(errors::catch(|| apply_edits(text_model, &[output_edit(usize::MAX, None, vec![])])).is_err());

        // invalid index 2
        assert!(errors::catch(|| apply_edits(text_model, &[output_edit(usize::MAX, None, vec![])])).is_err());

        apply_edits(text_model, &[output_edit(0, None, vec![output_dto("someId", vec![item(mimes::MARKDOWN, "_Hello_")])])]);

        assert_eq!(text_model.cells().len(), 1);
        assert_eq!(text_model.cells()[0].outputs().len(), 1);

        // append
        apply_edits(text_model, &[output_edit(0, Some(true), vec![output_dto("someId2", vec![item(mimes::MARKDOWN, "_Hello2_")])])]);

        assert_eq!(text_model.cells().len(), 1);
        assert_eq!(text_model.cells()[0].outputs().len(), 2);
        let outputs = text_model.cells()[0].outputs();
        let (first, second) = (&outputs[0], &outputs[1]);
        assert_eq!(first.output_id(), "someId");
        assert_eq!(second.output_id(), "someId2");

        // replace all
        apply_edits(text_model, &[output_edit(0, None, vec![output_dto("someId3", vec![item(mimes::TEXT, "Last, replaced output")])])]);

        assert_eq!(text_model.cells().len(), 1);
        assert_eq!(text_model.cells()[0].outputs().len(), 1);
        let outputs = text_model.cells()[0].outputs();
        let first = &outputs[0];
        assert_eq!(first.output_id(), "someId3");
    });
}

#[test]
fn multiple_append_output_in_one_position() {
    let _suite = suite_setup();
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    with_test_notebook(vec![js_cell("var a = 1;")], |editor, _view_model, _ds, _services| {
        let text_model = &editor.text_model;

        // append
        apply_edits(
            text_model,
            &[
                output_edit(0, Some(true), vec![output_dto("append1", vec![item(mimes::MARKDOWN, "append 1")])]),
                output_edit(0, Some(true), vec![output_dto("append2", vec![item(mimes::MARKDOWN, "append 2")])]),
            ],
        );

        assert_eq!(text_model.cells().len(), 1);
        assert_eq!(text_model.cells()[0].outputs().len(), 2);
        let outputs = text_model.cells()[0].outputs();
        let (first, second) = (&outputs[0], &outputs[1]);
        assert_eq!(first.output_id(), "append1");
        assert_eq!(second.output_id(), "append2");
    });
}

#[test]
fn append_to_output_created_in_same_batch() {
    let _suite = suite_setup();
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    with_test_notebook(vec![js_cell("var a = 1;")], |editor, _view_model, _ds, _services| {
        let text_model = &editor.text_model;

        apply_edits(
            text_model,
            &[
                output_edit(0, Some(true), vec![output_dto("append1", vec![item(mimes::MARKDOWN, "append 1")])]),
                output_items_edit("append1", Some(true), vec![item(mimes::MARKDOWN, "append 2")]),
            ],
        );

        assert_eq!(text_model.cells().len(), 1);
        assert_eq!(text_model.cells()[0].outputs().len(), 1, "has 1 output");
        let outputs = text_model.cells()[0].outputs();
        let first = &outputs[0];
        assert_eq!(first.output_id(), "append1");
        assert_eq!(first.outputs().len(), 2, "has 2 items");
    });
}

#[test]
fn appending_streaming_outputs() {
    let _suite = suite_setup();
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    with_test_notebook(vec![js_cell("var a = 1;")], |editor, _view_model, _ds, _services| {
        let text_model = &editor.text_model;

        apply_edits(text_model, &[output_edit(0, Some(true), vec![output_dto("append1", vec![item(STD_OUT_MIME, "append 1")])])]);
        let output = text_model.cells()[0].outputs()[0].clone();
        assert_eq!(output.version_id(), 0, "initial output version should be 0");

        apply_edits(
            text_model,
            &[output_items_edit("append1", Some(true), vec![item(STD_OUT_MIME, "append 2"), item(STD_OUT_MIME, "append 3")])],
        );
        assert_eq!(output.version_id(), 1, "version should bump per append");

        apply_edits(
            text_model,
            &[output_items_edit("append1", Some(true), vec![item(STD_OUT_MIME, "append 4"), item(STD_OUT_MIME, "append 5")])],
        );
        assert_eq!(output.version_id(), 2, "version should bump per append");

        assert_eq!(text_model.cells().len(), 1);
        assert_eq!(text_model.cells()[0].outputs().len(), 1, "has 1 output");
        assert_eq!(output.output_id(), "append1");
        assert_eq!(output.outputs().len(), 1, "outputs are compressed");
        assert_eq!(output.outputs()[0].data.to_string(), "append 1append 2append 3append 4append 5");
        assert_eq!(output.appended_since_version(0, STD_OUT_MIME).map(|buffer| buffer.to_string()), Some("append 2append 3append 4append 5".to_string()));
        assert_eq!(output.appended_since_version(1, STD_OUT_MIME).map(|buffer| buffer.to_string()), Some("append 4append 5".to_string()));
        assert!(output.appended_since_version(2, STD_OUT_MIME).is_none());
        assert!(output.appended_since_version(2, STD_ERR_MIME).is_none());
    });
}

#[test]
fn replacing_streaming_outputs() {
    let _suite = suite_setup();
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    with_test_notebook(vec![js_cell("var a = 1;")], |editor, _view_model, _ds, _services| {
        let text_model = &editor.text_model;

        apply_edits(text_model, &[output_edit(0, Some(true), vec![output_dto("append1", vec![item(STD_OUT_MIME, "append 1")])])]);
        let output = text_model.cells()[0].outputs()[0].clone();
        assert_eq!(output.version_id(), 0, "initial output version should be 0");

        apply_edits(text_model, &[output_items_edit("append1", Some(true), vec![item(STD_OUT_MIME, "append 2")])]);
        assert_eq!(output.version_id(), 1, "version should bump per append");

        apply_edits(text_model, &[output_items_edit("append1", Some(false), vec![item(STD_OUT_MIME, "replace 3")])]);
        assert_eq!(output.version_id(), 2, "version should bump per replace");

        apply_edits(text_model, &[output_items_edit("append1", Some(true), vec![item(STD_OUT_MIME, "append 4")])]);
        assert_eq!(output.version_id(), 3, "version should bump per append");

        assert_eq!(output.outputs()[0].data.to_string(), "replace 3append 4");
        assert!(output.appended_since_version(0, STD_OUT_MIME).is_none(), "replacing output should clear out previous versioned output buffers");
        assert!(output.appended_since_version(1, STD_OUT_MIME).is_none(), "replacing output should clear out previous versioned output buffers");
        assert_eq!(output.appended_since_version(2, STD_OUT_MIME).map(|buffer| buffer.to_string()), Some("append 4".to_string()));
    });
}

#[test]
fn appending_streaming_outputs_with_move_cursor_compression() {
    let _suite = suite_setup();
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    with_test_notebook(vec![js_cell("var a = 1;")], |editor, _view_model, _ds, _services| {
        let text_model = &editor.text_model;

        apply_edits(
            text_model,
            &[output_edit(
                0,
                Some(true),
                vec![output_dto("append1", vec![item(STD_OUT_MIME, "append 1"), item(STD_OUT_MIME, "\nappend 1")])],
            )],
        );
        let output = text_model.cells()[0].outputs()[0].clone();
        assert_eq!(output.version_id(), 0, "initial output version should be 0");

        apply_edits(
            text_model,
            &[output_items_edit("append1", Some(true), vec![item(STD_OUT_MIME, &format!("{MOVE_CURSOR_1_LINE_COMMAND}\nappend 2"))])],
        );
        assert_eq!(output.version_id(), 1, "version should bump per append");

        assert_eq!(output.outputs()[0].data.to_string(), "append 1\nappend 2");
        assert!(output.appended_since_version(0, STD_OUT_MIME).is_none(), "compressing outputs should clear out previous versioned output buffers");
    });
}

#[test]
fn appending_streaming_outputs_with_carraige_return_compression() {
    let _suite = suite_setup();
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    with_test_notebook(vec![js_cell("var a = 1;")], |editor, _view_model, _ds, _services| {
        let text_model = &editor.text_model;

        apply_edits(
            text_model,
            &[output_edit(
                0,
                Some(true),
                vec![output_dto("append1", vec![item(STD_OUT_MIME, "append 1"), item(STD_OUT_MIME, "\nappend 1")])],
            )],
        );
        let output = text_model.cells()[0].outputs()[0].clone();
        assert_eq!(output.version_id(), 0, "initial output version should be 0");

        apply_edits(text_model, &[output_items_edit("append1", Some(true), vec![item(STD_OUT_MIME, "\rappend 2")])]);
        assert_eq!(output.version_id(), 1, "version should bump per append");

        assert_eq!(output.outputs()[0].data.to_string(), "append 1\nappend 2");
        assert!(output.appended_since_version(0, STD_OUT_MIME).is_none(), "compressing outputs should clear out previous versioned output buffers");
    });
}

#[test]
fn appends_a_batch_of_carriage_return_progress_lines_without_changing_output_events_or_version_tracking() {
    let _suite = suite_setup();
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    with_test_notebook(vec![js_cell("var a = 1;")], |editor, _view_model, store, _services| {
        let text_model = &editor.text_model;
        apply_edits(text_model, &[output_edit(0, Some(true), vec![output_dto("progress", vec![item(STD_OUT_MIME, "Log start\n")])])]);
        let output = text_model.cells()[0].outputs()[0].clone();
        let changes = Rc::new(Cell::new(0));
        store.add({
            let changes = changes.clone();
            output.on_did_change_data().listen(move |_| changes.set(changes.get() + 1))
        });
        let prefixes: Vec<String> = (0..4000).map(|index| format!("Task {index:05}: ")).collect();
        let progress: String = prefixes.iter().map(|prefix| format!("{prefix}running\r{prefix}complete\n")).collect();
        apply_edits(text_model, &[output_items_edit("progress", Some(true), vec![item(STD_OUT_MIME, &progress)])]);

        let completed: String = prefixes.iter().map(|prefix| format!("{prefix}complete\n")).collect();
        assert_eq!(
            (
                output.outputs().len(),
                output.outputs()[0].data.to_string(),
                output.version_id(),
                changes.get(),
                output.appended_since_version(0, STD_OUT_MIME).map(|buffer| buffer.to_string()),
            ),
            (1, format!("Log start\n{completed}"), 1, 1, None)
        );
    });
}

#[test]
fn appending_multiple_different_mime_streaming_outputs() {
    let _suite = suite_setup();
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    with_test_notebook(vec![js_cell("var a = 1;")], |editor, _view_model, _ds, _services| {
        let text_model = &editor.text_model;

        apply_edits(
            text_model,
            &[output_edit(
                0,
                Some(true),
                vec![output_dto("append1", vec![item(STD_OUT_MIME, "stdout 1"), item(STD_ERR_MIME, "stderr 1")])],
            )],
        );
        let output = text_model.cells()[0].outputs()[0].clone();
        assert_eq!(output.version_id(), 0, "initial output version should be 0");

        apply_edits(
            text_model,
            &[output_items_edit("append1", Some(true), vec![item(STD_OUT_MIME, "stdout 2"), item(STD_ERR_MIME, "stderr 2")])],
        );
        assert_eq!(output.version_id(), 1, "version should bump per replace");

        assert_eq!(output.appended_since_version(0, STD_ERR_MIME).map(|buffer| buffer.to_string()), Some("stderr 2".to_string()));
        assert_eq!(output.appended_since_version(0, STD_OUT_MIME).map(|buffer| buffer.to_string()), Some("stdout 2".to_string()));
    });
}

#[test]
fn metadata() {
    let _suite = suite_setup();
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    with_test_notebook(vec![js_cell("var a = 1;")], |editor, _view_model, _ds, _services| {
        let text_model = &editor.text_model;

        // invalid index 1
        assert!(errors::catch(|| apply_edits(text_model, &[metadata_edit(usize::MAX, empty_metadata())])).is_err());

        // invalid index 2
        assert!(errors::catch(|| apply_edits(text_model, &[metadata_edit(usize::MAX, empty_metadata())])).is_err());

        apply_edits(text_model, &[metadata_edit(0, object_of("customProperty", Dynamic::Number(15.0)))]);

        apply_edits(text_model, &[metadata_edit(0, empty_metadata())]);

        assert_eq!(text_model.cells().len(), 1);
        assert_eq!(text_model.cells()[0].metadata().get_str("customProperty"), Dynamic::Undefined);
    });
}

#[test]
fn partial_metadata() {
    let _suite = suite_setup();
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    with_test_notebook(vec![js_cell("var a = 1;")], |editor, _view_model, _ds, _services| {
        let text_model = &editor.text_model;

        apply_edits(
            text_model,
            &[ICellEditOperation::PartialMetadata(ICellPartialMetadataEdit {
                index: 0,
                metadata: object_of("customProperty", Dynamic::Number(15.0)),
            })],
        );

        apply_edits(text_model, &[ICellEditOperation::PartialMetadata(ICellPartialMetadataEdit { index: 0, metadata: empty_metadata() })]);

        assert_eq!(text_model.cells().len(), 1);
        assert_eq!(text_model.cells()[0].metadata().get_str("customProperty"), Dynamic::Number(15.0));
    });
}

#[test]
fn multiple_inserts_in_one_edit() {
    let suite = suite_setup();
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let language_service = suite.language_service.clone();
    with_test_notebook(
        vec![js_cell("var a = 1;"), js_cell("var b = 2;"), js_cell("var c = 3;"), js_cell("var d = 4;")],
        |editor, _view_model, ds, _services| {
            let text_model = &editor.text_model;
            let change_event: Rc<RefCell<Option<NotebookTextModelChangedEvent>>> = Rc::new(RefCell::new(None));
            let event_listener = {
                let change_event = change_event.clone();
                text_model.on_did_change_content().listen(move |e| {
                    *change_event.borrow_mut() = Some(e.clone());
                })
            };
            let will_change_events: Rc<RefCell<Vec<NotebookTextModelWillAddRemoveEvent>>> = Rc::new(RefCell::new(Vec::new()));
            let will_change_listener = {
                let will_change_events = will_change_events.clone();
                text_model.on_will_add_remove_cells().listen(move |e| {
                    will_change_events.borrow_mut().push(e.clone());
                })
            };
            let version = text_model.version_id();

            text_model.apply_edits(
                &[
                    replace_edit(1, 1, vec![]),
                    replace_edit(1, 0, vec![new_test_cell(ds, &text_model.view_type(), 5, "var e = 5;", &language_service)]),
                ],
                true,
                None,
                &index_selection_state,
                None,
                true,
            );

            assert_eq!(text_model.cells().len(), 4);
            assert_eq!(text_model.cells()[0].get_value(), "var a = 1;");
            assert_eq!(text_model.cells()[1].get_value(), "var e = 5;");
            assert_eq!(text_model.cells()[2].get_value(), "var c = 3;");

            let change_event = change_event.borrow();
            assert!(change_event.is_some());
            let change_event = change_event.as_ref().expect("o evento de mudança");
            assert_eq!(change_event.raw_events.len(), 2);
            assert_end_selections_are_first_cell(change_event);
            assert_eq!(will_change_events.borrow().len(), 2);
            assert_eq!(text_model.version_id(), version + 1);
            event_listener.dispose();
            will_change_listener.dispose();
        },
    );
}

#[test]
fn insert_and_metadata_change_in_one_edit() {
    let _suite = suite_setup();
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    with_test_notebook(
        vec![js_cell("var a = 1;"), js_cell("var b = 2;"), js_cell("var c = 3;"), js_cell("var d = 4;")],
        |editor, _view_model, _ds, _services| {
            let text_model = &editor.text_model;
            let change_event: Rc<RefCell<Option<NotebookTextModelChangedEvent>>> = Rc::new(RefCell::new(None));
            let event_listener = {
                let change_event = change_event.clone();
                text_model.on_did_change_content().listen(move |e| {
                    *change_event.borrow_mut() = Some(e.clone());
                })
            };
            let will_change_events: Rc<RefCell<Vec<NotebookTextModelWillAddRemoveEvent>>> = Rc::new(RefCell::new(Vec::new()));
            let will_change_listener = {
                let will_change_events = will_change_events.clone();
                text_model.on_will_add_remove_cells().listen(move |e| {
                    will_change_events.borrow_mut().push(e.clone());
                })
            };

            let version = text_model.version_id();

            text_model.apply_edits(
                &[replace_edit(1, 1, vec![]), metadata_edit(0, empty_metadata())],
                true,
                None,
                &index_selection_state,
                None,
                true,
            );

            let change_event = change_event.borrow();
            assert!(change_event.is_some());
            let change_event = change_event.as_ref().expect("o evento de mudança");
            assert_eq!(change_event.raw_events.len(), 2);
            assert_end_selections_are_first_cell(change_event);
            assert_eq!(will_change_events.borrow().len(), 1);
            assert_eq!(text_model.version_id(), version + 1);
            event_listener.dispose();
            will_change_listener.dispose();
        },
    );
}

#[test]
fn updating_appending_updating_output_in_notebooks_does_not_work_as_expected_117273() {
    let _suite = suite_setup();
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    with_test_notebook(vec![js_cell("var a = 1;")], |editor, _view_model, _ds, _services| {
        let model = &editor.text_model;

        assert_eq!(model.cells().len(), 1);
        assert_eq!(model.cells()[0].outputs().len(), 0);

        let success1 = model.apply_edits(
            &[output_edit(
                0,
                Some(false),
                vec![IOutputDto {
                    output_id: "out1".to_string(),
                    outputs: vec![IOutputItemDto { mime: "application/x.notebook.stream".to_string(), data: VSBuffer::wrap(vec![1]) }],
                    metadata: None,
                }],
            )],
            true,
            None,
            &|| None,
            None,
            false,
        );

        assert!(success1);
        assert_eq!(model.cells()[0].outputs().len(), 1);

        let success2 = model.apply_edits(
            &[output_edit(
                0,
                Some(true),
                vec![IOutputDto {
                    output_id: "out2".to_string(),
                    outputs: vec![IOutputItemDto { mime: "application/x.notebook.stream".to_string(), data: VSBuffer::wrap(vec![1]) }],
                    metadata: None,
                }],
            )],
            true,
            None,
            &|| None,
            None,
            false,
        );

        assert!(success2);
        assert_eq!(model.cells()[0].outputs().len(), 2);
    });
}

#[test]
fn clearing_output_of_an_empty_notebook_makes_it_dirty_119608() {
    let _suite = suite_setup();
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    with_test_notebook(vec![js_cell("var a = 1;"), js_cell("var b = 2;")], |editor, _view_model, ds, _services| {
        let model = &editor.text_model;

        let event: Rc<RefCell<Option<NotebookTextModelChangedEvent>>> = Rc::new(RefCell::new(None));

        ds.add({
            let event = event.clone();
            model.on_did_change_content().listen(move |e| {
                *event.borrow_mut() = Some(e.clone());
            })
        });

        {
            // 1: add ouput -> event
            let success = model.apply_edits(
                &[output_edit(
                    0,
                    Some(false),
                    vec![IOutputDto {
                        output_id: "out1".to_string(),
                        outputs: vec![IOutputItemDto { mime: "application/x.notebook.stream".to_string(), data: VSBuffer::wrap(vec![1]) }],
                        metadata: None,
                    }],
                )],
                true,
                None,
                &|| None,
                None,
                false,
            );

            assert!(success);
            assert_eq!(model.cells()[0].outputs().len(), 1);
            assert!(event.borrow().is_some());
        }

        {
            // 2: clear all output w/ output -> event
            *event.borrow_mut() = None;
            let success = model.apply_edits(
                &[output_edit(0, Some(false), vec![]), output_edit(1, Some(false), vec![])],
                true,
                None,
                &|| None,
                None,
                false,
            );
            assert!(success);
            assert!(event.borrow().is_some());
        }

        {
            // 2: clear all output wo/ output -> NO event
            *event.borrow_mut() = None;
            let success = model.apply_edits(
                &[output_edit(0, Some(false), vec![]), output_edit(1, Some(false), vec![])],
                true,
                None,
                &|| None,
                None,
                false,
            );

            assert!(success);
            assert!(event.borrow().is_none());
        }
    });
}

#[test]
fn cell_metadata_output_change_should_update_version_id_and_alternative_id_121807() {
    let _suite = suite_setup();
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    with_test_notebook(vec![js_cell("var a = 1;"), js_cell("var b = 2;")], |editor, view_model, _ds, _services| {
        assert_eq!(editor.text_model.version_id(), 0);
        let first_alt_version = "0_0,1;1,1";
        assert_eq!(editor.text_model.alternative_version_id(), first_alt_version);
        apply_edits(&editor.text_model, &[metadata_edit(0, object_of("inputCollapsed", Dynamic::Bool(true)))]);
        assert_eq!(editor.text_model.version_id(), 1);
        assert_ne!(editor.text_model.alternative_version_id(), first_alt_version);
        let second_alt_version = "1_0,1;1,1";
        assert_eq!(editor.text_model.alternative_version_id(), second_alt_version);

        view_model.undo();
        assert_eq!(editor.text_model.version_id(), 2);
        assert_eq!(editor.text_model.alternative_version_id(), first_alt_version);

        view_model.redo();
        assert_eq!(editor.text_model.version_id(), 3);
        assert_ne!(editor.text_model.alternative_version_id(), first_alt_version);
        assert_eq!(editor.text_model.alternative_version_id(), second_alt_version);

        apply_edits(&editor.text_model, &[metadata_edit(1, object_of("inputCollapsed", Dynamic::Bool(true)))]);
        assert_eq!(editor.text_model.version_id(), 4);
        assert_eq!(editor.text_model.alternative_version_id(), "4_0,1;1,1");

        view_model.undo();
        assert_eq!(editor.text_model.version_id(), 5);
        assert_eq!(editor.text_model.alternative_version_id(), second_alt_version);
    });
}

#[test]
fn metadata_changes_on_newly_added_cells_should_combine_their_undo_operations() {
    let suite = suite_setup();
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let language_service = suite.language_service.clone();
    with_test_notebook(vec![js_cell("var a = 1;")], |editor, view_model, ds, _services| {
        let text_model = &editor.text_model;
        apply_edits(
            &editor.text_model,
            &[replace_edit(
                1,
                0,
                vec![
                    new_test_cell(ds, &text_model.view_type(), 1, "var e = 5;", &language_service),
                    new_test_cell(ds, &text_model.view_type(), 2, "var f = 6;", &language_service),
                ],
            )],
        );

        assert_eq!(text_model.cells().len(), 3);

        apply_edits(&editor.text_model, &[metadata_edit(1, object_of("id", Dynamic::string("123")))]);

        assert_eq!(text_model.cells()[1].metadata().get_str("id"), Dynamic::string("123"));

        view_model.undo();

        assert_eq!(text_model.cells().len(), 1);

        view_model.redo();

        assert_eq!(text_model.cells().len(), 3);
    });
}

#[test]
fn changes_with_non_metadata_edit_should_not_combine_their_undo_operations() {
    let suite = suite_setup();
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let language_service = suite.language_service.clone();
    with_test_notebook(vec![js_cell("var a = 1;")], |editor, view_model, ds, _services| {
        let text_model = &editor.text_model;
        apply_edits(
            &editor.text_model,
            &[replace_edit(
                1,
                0,
                vec![
                    new_test_cell(ds, &text_model.view_type(), 1, "var e = 5;", &language_service),
                    new_test_cell(ds, &text_model.view_type(), 2, "var f = 6;", &language_service),
                ],
            )],
        );

        assert_eq!(text_model.cells().len(), 3);

        apply_edits(
            &editor.text_model,
            &[
                metadata_edit(1, object_of("id", Dynamic::string("123"))),
                output_edit_by_handle(
                    0,
                    Some(true),
                    vec![output_dto("newOutput", vec![item(mimes::TEXT, "cba"), item("application/foo", "cba")])],
                ),
            ],
        );

        assert_eq!(text_model.cells()[1].metadata().get_str("id"), Dynamic::string("123"));

        view_model.undo();

        assert_eq!(text_model.cells().len(), 3);

        view_model.undo();

        assert_eq!(text_model.cells().len(), 1);
    });
}

#[test]
fn destructive_sorting_in_do_apply_edits_121994() {
    let _suite = suite_setup();
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    with_test_notebook(
        vec![js_cell_with_outputs("var a = 1;", vec![output_dto("i42", vec![item("m/ime", "test")])])],
        |editor, _view_model, _ds, _services| {
            let notebook = &editor.text_model;

            assert_eq!(notebook.cells()[0].outputs().len(), 1);
            assert_eq!(notebook.cells()[0].outputs()[0].outputs().len(), 1);
            assert_eq!(notebook.cells()[0].outputs()[0].outputs()[0].data.to_vec(), value_bytes_from_string("test").to_vec());

            let edits = vec![
                output_edit_by_handle(0, None, vec![]),
                output_edit_by_handle(
                    0,
                    Some(true),
                    vec![output_dto("newOutput", vec![item(mimes::TEXT, "cba"), item("application/foo", "cba")])],
                ),
            ];

            apply_edits(&editor.text_model, &edits);

            assert_eq!(notebook.cells()[0].outputs().len(), 1);
            assert_eq!(notebook.cells()[0].outputs()[0].outputs().len(), 2);
        },
    );
}

#[test]
fn destructive_sorting_in_do_apply_edits_121994_cell_splice_between_output_changes() {
    let _suite = suite_setup();
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    with_test_notebook(
        vec![
            js_cell_with_outputs("var a = 1;", vec![output_dto("i42", vec![item("m/ime", "test")])]),
            js_cell_with_outputs("var b = 2;", vec![output_dto("i43", vec![item("m/ime", "test")])]),
            js_cell_with_outputs("var c = 3;", vec![output_dto("i44", vec![item("m/ime", "test")])]),
        ],
        |editor, _view_model, _ds, _services| {
            let notebook = &editor.text_model;

            let edits = vec![
                output_edit(0, None, vec![]),
                replace_edit(1, 1, vec![]),
                output_edit(
                    2,
                    Some(true),
                    vec![output_dto("newOutput", vec![item(mimes::TEXT, "cba"), item("application/foo", "cba")])],
                ),
            ];

            apply_edits(&editor.text_model, &edits);

            assert_eq!(notebook.cells().len(), 2);
            assert_eq!(notebook.cells()[0].outputs().len(), 0);
            assert_eq!(notebook.cells()[1].outputs().len(), 2);
            assert_eq!(notebook.cells()[1].outputs()[0].output_id(), "i44");
            assert_eq!(notebook.cells()[1].outputs()[1].output_id(), "newOutput");
        },
    );
}

#[test]
fn destructive_sorting_in_do_apply_edits_121994_cell_splice_between_output_changes_2() {
    let _suite = suite_setup();
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    with_test_notebook(
        vec![
            js_cell_with_outputs("var a = 1;", vec![output_dto("i42", vec![item("m/ime", "test")])]),
            js_cell_with_outputs("var b = 2;", vec![output_dto("i43", vec![item("m/ime", "test")])]),
            js_cell_with_outputs("var c = 3;", vec![output_dto("i44", vec![item("m/ime", "test")])]),
        ],
        |editor, _view_model, _ds, _services| {
            let notebook = &editor.text_model;

            let edits = vec![
                output_edit(
                    1,
                    Some(true),
                    vec![output_dto("newOutput", vec![item(mimes::TEXT, "cba"), item("application/foo", "cba")])],
                ),
                replace_edit(1, 1, vec![]),
                output_edit(
                    1,
                    Some(true),
                    vec![output_dto("newOutput2", vec![item(mimes::TEXT, "cba"), item("application/foo", "cba")])],
                ),
            ];

            apply_edits(&editor.text_model, &edits);

            assert_eq!(notebook.cells().len(), 2);
            assert_eq!(notebook.cells()[0].outputs().len(), 1);
            assert_eq!(notebook.cells()[1].outputs().len(), 1);
            assert_eq!(notebook.cells()[1].outputs()[0].output_id(), "i44");
        },
    );
}

#[test]
fn output_edits_splice() {
    let _suite = suite_setup();
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    with_test_notebook(vec![js_cell("var a = 1;")], |editor, _view_model, _ds, _services| {
        let model = &editor.text_model;

        assert_eq!(model.cells().len(), 1);
        assert_eq!(model.cells()[0].outputs().len(), 0);

        let stream = "application/x.notebook.stream";
        let success1 = model.apply_edits(
            &[output_edit(
                0,
                Some(false),
                vec![
                    output_dto("out1", vec![item(stream, "1")]),
                    output_dto("out2", vec![item(stream, "2")]),
                    output_dto("out3", vec![item(stream, "3")]),
                    output_dto("out4", vec![item(stream, "4")]),
                ],
            )],
            true,
            None,
            &|| None,
            None,
            false,
        );

        assert!(success1);
        assert_eq!(model.cells()[0].outputs().len(), 4);

        let success2 = model.apply_edits(
            &[output_edit(
                0,
                Some(false),
                vec![
                    output_dto("out1", vec![item(stream, "1")]),
                    output_dto("out5", vec![item(stream, "5")]),
                    output_dto("out3", vec![item(stream, "3")]),
                    output_dto("out6", vec![item(stream, "6")]),
                ],
            )],
            true,
            None,
            &|| None,
            None,
            false,
        );

        assert!(success2);
        assert_eq!(model.cells()[0].outputs().len(), 4);
        assert_eq!(model.cells()[0].outputs()[0].output_id(), "out1");
        assert_eq!(model.cells()[0].outputs()[1].output_id(), "out5");
        assert_eq!(model.cells()[0].outputs()[2].output_id(), "out3");
        assert_eq!(model.cells()[0].outputs()[3].output_id(), "out6");
    });
}

#[test]
fn compute_edits_no_insert() {
    let _suite = suite_setup();
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    with_test_notebook(vec![js_cell("var a = 1;")], |editor, _view_model, _ds, _services| {
        let model = &editor.text_model;
        let edits = NotebookTextModel::compute_edits(model, &[dto("var a = 1;", "javascript", CellKind::Code, vec![], None)], &[]);

        assert_edits_equal(&edits, &[metadata_edit(0, empty_metadata())]);
    });
}

#[test]
fn compute_edits_cell_content_changed() {
    let _suite = suite_setup();
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    with_test_notebook(vec![js_cell("var a = 1;")], |editor, _view_model, _ds, _services| {
        let model = &editor.text_model;
        let cells = vec![dto("var a = 2;", "javascript", CellKind::Code, vec![], None)];
        let edits = NotebookTextModel::compute_edits(model, &cells, &[]);

        assert_edits_equal(&edits, &[replace_edit(0, 1, cells.clone())]);
    });
}

#[test]
fn compute_edits_last_cell_content_changed() {
    let _suite = suite_setup();
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    with_test_notebook(vec![js_cell("var a = 1;"), js_cell("var b = 1;")], |editor, _view_model, _ds, _services| {
        let model = &editor.text_model;
        let cells = vec![
            dto("var a = 1;", "javascript", CellKind::Code, vec![], None),
            dto("var b = 2;", "javascript", CellKind::Code, vec![], None),
        ];
        let edits = NotebookTextModel::compute_edits(model, &cells, &[]);

        assert_edits_equal(&edits, &[metadata_edit(0, empty_metadata()), replace_edit(1, 1, cells[1..].to_vec())]);
    });
}

#[test]
fn compute_edits_first_cell_content_changed() {
    let _suite = suite_setup();
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    with_test_notebook(vec![js_cell("var a = 1;"), js_cell("var b = 1;")], |editor, _view_model, _ds, _services| {
        let model = &editor.text_model;
        let cells = vec![
            dto("var a = 2;", "javascript", CellKind::Code, vec![], None),
            dto("var b = 1;", "javascript", CellKind::Code, vec![], None),
        ];
        let edits = NotebookTextModel::compute_edits(model, &cells, &[]);

        assert_edits_equal(&edits, &[replace_edit(0, 1, cells[0..1].to_vec()), metadata_edit(1, empty_metadata())]);
    });
}

#[test]
fn compute_edits_middle_cell_content_changed() {
    let _suite = suite_setup();
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    with_test_notebook(vec![js_cell("var a = 1;"), js_cell("var b = 1;"), js_cell("var c = 1;")], |editor, _view_model, _ds, _services| {
        let model = &editor.text_model;
        let cells = vec![
            dto("var a = 1;", "javascript", CellKind::Code, vec![], None),
            dto("var b = 2;", "javascript", CellKind::Code, vec![], None),
            dto("var c = 1;", "javascript", CellKind::Code, vec![], None),
        ];
        let edits = NotebookTextModel::compute_edits(model, &cells, &[]);

        assert_edits_equal(
            &edits,
            &[metadata_edit(0, empty_metadata()), replace_edit(1, 1, cells[1..2].to_vec()), metadata_edit(2, empty_metadata())],
        );
    });
}

#[test]
fn compute_edits_cell_metadata_changed() {
    let _suite = suite_setup();
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    with_test_notebook(vec![js_cell("var a = 1;"), js_cell("var b = 1;")], |editor, _view_model, _ds, _services| {
        let model = &editor.text_model;
        let cells = vec![
            dto("var a = 1;", "javascript", CellKind::Code, vec![], Some(object_of("name", Dynamic::string("foo")))),
            dto("var b = 1;", "javascript", CellKind::Code, vec![], None),
        ];
        let edits = NotebookTextModel::compute_edits(model, &cells, &[]);

        assert_edits_equal(
            &edits,
            &[metadata_edit(0, object_of("name", Dynamic::string("foo"))), metadata_edit(1, empty_metadata())],
        );
    });
}

#[test]
fn compute_edits_cell_language_changed() {
    let _suite = suite_setup();
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    with_test_notebook(vec![js_cell("var a = 1;"), js_cell("var b = 1;")], |editor, _view_model, _ds, _services| {
        let model = &editor.text_model;
        let cells = vec![
            dto("var a = 1;", "typescript", CellKind::Code, vec![], None),
            dto("var b = 1;", "javascript", CellKind::Code, vec![], None),
        ];
        let edits = NotebookTextModel::compute_edits(model, &cells, &[]);

        assert_edits_equal(&edits, &[replace_edit(0, 1, cells[0..1].to_vec()), metadata_edit(1, empty_metadata())]);
    });
}

#[test]
fn compute_edits_cell_kind_changed() {
    let _suite = suite_setup();
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    with_test_notebook(vec![js_cell("var a = 1;"), js_cell("var b = 1;")], |editor, _view_model, _ds, _services| {
        let model = &editor.text_model;
        let cells = vec![
            dto("var a = 1;", "javascript", CellKind::Code, vec![], None),
            dto("var b = 1;", "javascript", CellKind::Markup, vec![], None),
        ];
        let edits = NotebookTextModel::compute_edits(model, &cells, &[]);

        assert_edits_equal(&edits, &[metadata_edit(0, empty_metadata()), replace_edit(1, 1, cells[1..].to_vec())]);
    });
}

#[test]
fn compute_edits_cell_metadata_and_content_changed() {
    let _suite = suite_setup();
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    with_test_notebook(vec![js_cell("var a = 1;"), js_cell("var b = 1;")], |editor, _view_model, _ds, _services| {
        let model = &editor.text_model;
        let cells = vec![
            dto("var a = 1;", "javascript", CellKind::Code, vec![], Some(object_of("name", Dynamic::string("foo")))),
            dto("var b = 2;", "javascript", CellKind::Code, vec![], Some(object_of("name", Dynamic::string("bar")))),
        ];
        let edits = NotebookTextModel::compute_edits(model, &cells, &[]);

        assert_edits_equal(&edits, &[metadata_edit(0, object_of("name", Dynamic::string("foo"))), replace_edit(1, 1, cells[1..].to_vec())]);
    });
}

#[test]
fn compute_edits_cell_content_changed_while_executing() {
    let _suite = suite_setup();
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    with_test_notebook(vec![js_cell("var a = 1;"), js_cell("var b = 1;")], |editor, _view_model, _ds, _services| {
        let model = &editor.text_model;
        let cells = vec![
            dto("var a = 1;", "javascript", CellKind::Code, vec![], Some(empty_metadata())),
            dto("var b = 2;", "javascript", CellKind::Code, vec![], Some(empty_metadata())),
        ];
        let edits = NotebookTextModel::compute_edits(model, &cells, &[model.cells()[1].handle()]);

        assert_edits_equal(&edits, &[metadata_edit(0, empty_metadata()), replace_edit(1, 1, cells[1..].to_vec())]);
    });
}

#[test]
fn compute_edits_cell_internal_metadata_changed() {
    let _suite = suite_setup();
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    with_test_notebook(vec![js_cell("var a = 1;"), js_cell("var b = 1;")], |editor, _view_model, _ds, _services| {
        let model = &editor.text_model;
        let cells = vec![
            ICellDto2 {
                internal_metadata: Some(NotebookCellInternalMetadata { execution_order: Some(1.0), ..NotebookCellInternalMetadata::default() }),
                ..dto("var a = 1;", "javascript", CellKind::Code, vec![], None)
            },
            dto("var b = 1;", "javascript", CellKind::Code, vec![], None),
        ];
        let edits = NotebookTextModel::compute_edits(model, &cells, &[]);

        assert_edits_equal(&edits, &[replace_edit(0, 1, cells[0..1].to_vec()), metadata_edit(1, empty_metadata())]);
    });
}

#[test]
fn compute_edits_cell_internal_metadata_changed_while_executing() {
    let _suite = suite_setup();
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    with_test_notebook(vec![js_cell("var a = 1;"), js_cell("var b = 1;")], |editor, _view_model, _ds, _services| {
        let model = &editor.text_model;
        let cells = vec![
            dto("var a = 1;", "javascript", CellKind::Code, vec![], Some(empty_metadata())),
            ICellDto2 {
                internal_metadata: Some(NotebookCellInternalMetadata { execution_order: Some(1.0), ..NotebookCellInternalMetadata::default() }),
                ..dto("var b = 1;", "javascript", CellKind::Code, vec![], Some(empty_metadata()))
            },
        ];
        let edits = NotebookTextModel::compute_edits(model, &cells, &[model.cells()[1].handle()]);

        assert_edits_equal(&edits, &[metadata_edit(0, empty_metadata()), metadata_edit(1, empty_metadata())]);
    });
}

#[test]
fn compute_edits_cell_insertion() {
    let _suite = suite_setup();
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    with_test_notebook(vec![js_cell("var a = 1;"), js_cell("var b = 1;")], |editor, _view_model, _ds, _services| {
        let model = &editor.text_model;
        let cells = vec![
            dto("var a = 1;", "javascript", CellKind::Code, vec![], None),
            dto("var c = 1;", "javascript", CellKind::Code, vec![], None),
            dto("var b = 1;", "javascript", CellKind::Code, vec![], Some(object_of("foo", Dynamic::string("bar")))),
        ];
        let edits = NotebookTextModel::compute_edits(model, &cells, &[]);

        assert_edits_equal(
            &edits,
            &[
                metadata_edit(0, empty_metadata()),
                replace_edit(1, 0, cells[1..2].to_vec()),
                metadata_edit(1, object_of("foo", Dynamic::string("bar"))),
            ],
        );

        apply_edits(model, &edits);
        assert_eq!(model.cells().len(), 3);
        assert_eq!(model.cells()[1].get_value(), "var c = 1;");
        assert_eq!(model.cells()[2].get_value(), "var b = 1;");
        assert_deep_strict_equal(&model.cells()[2].metadata(), &object_of("foo", Dynamic::string("bar")));
    });
}

#[test]
fn compute_edits_output_changed() {
    let _suite = suite_setup();
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    with_test_notebook(vec![js_cell("var a = 1;"), js_cell("var b = 1;")], |editor, _view_model, _ds, _services| {
        let model = &editor.text_model;
        let cells = vec![
            dto(
                "var a = 1;",
                "javascript",
                CellKind::Code,
                vec![output_dto("someId", vec![item(mimes::MARKDOWN, "_World_")])],
                None,
            ),
            dto("var b = 1;", "javascript", CellKind::Code, vec![], Some(object_of("foo", Dynamic::string("bar")))),
        ];
        let edits = NotebookTextModel::compute_edits(model, &cells, &[]);

        assert_edits_equal(
            &edits,
            &[
                metadata_edit(0, empty_metadata()),
                output_edit(0, Some(false), vec![output_dto("someId", vec![item(mimes::MARKDOWN, "_World_")])]),
                metadata_edit(1, object_of("foo", Dynamic::string("bar"))),
            ],
        );

        apply_edits(model, &edits);
        assert_eq!(model.cells().len(), 2);
        assert_eq!(model.cells()[0].outputs().len(), 1);
        assert_eq!(model.cells()[0].outputs()[0].output_id(), "someId");
        assert_eq!(model.cells()[0].outputs()[0].outputs()[0].data.to_string(), "_World_");
    });
}

#[test]
fn compute_edits_output_items_changed() {
    let _suite = suite_setup();
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    with_test_notebook(
        vec![
            js_cell_with_outputs("var a = 1;", vec![output_dto("someId", vec![item(mimes::MARKDOWN, "_Hello_")])]),
            js_cell("var b = 1;"),
        ],
        |editor, _view_model, _ds, _services| {
            let model = &editor.text_model;
            let cells = vec![
                dto(
                    "var a = 1;",
                    "javascript",
                    CellKind::Code,
                    vec![output_dto("someId", vec![item(mimes::MARKDOWN, "_World_")])],
                    None,
                ),
                dto("var b = 1;", "javascript", CellKind::Code, vec![], Some(object_of("foo", Dynamic::string("bar")))),
            ];
            let edits = NotebookTextModel::compute_edits(model, &cells, &[]);

            assert_edits_equal(
                &edits,
                &[
                    metadata_edit(0, empty_metadata()),
                    output_items_edit("someId", Some(false), vec![item(mimes::MARKDOWN, "_World_")]),
                    metadata_edit(1, object_of("foo", Dynamic::string("bar"))),
                ],
            );

            apply_edits(model, &edits);
            assert_eq!(model.cells().len(), 2);
            assert_eq!(model.cells()[0].outputs().len(), 1);
            assert_eq!(model.cells()[0].outputs()[0].output_id(), "someId");
            assert_eq!(model.cells()[0].outputs()[0].outputs()[0].data.to_string(), "_World_");
        },
    );
}

#[test]
fn append_multiple_text_plain_output_items() {
    let _suite = suite_setup();
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    with_test_notebook(
        vec![js_cell_with_outputs("var a = 1;", vec![output_dto("1", vec![item("text/plain", "foo")])])],
        |editor, _view_model, _ds, _services| {
            let model = &editor.text_model;
            let edits = vec![output_items_edit(
                "1",
                Some(true),
                vec![
                    IOutputItemDto { mime: "text/plain".to_string(), data: VSBuffer::from_string("bar") },
                    IOutputItemDto { mime: "text/plain".to_string(), data: VSBuffer::from_string("baz") },
                ],
            )];
            apply_edits(model, &edits);
            assert_eq!(model.cells().len(), 1);
            assert_eq!(model.cells()[0].outputs().len(), 1);
            assert_eq!(model.cells()[0].outputs()[0].outputs().len(), 3);
            assert_eq!(model.cells()[0].outputs()[0].outputs()[0].mime, "text/plain");
            assert_eq!(model.cells()[0].outputs()[0].outputs()[0].data.to_string(), "foo");
            assert_eq!(model.cells()[0].outputs()[0].outputs()[1].mime, "text/plain");
            assert_eq!(model.cells()[0].outputs()[0].outputs()[1].data.to_string(), "bar");
            assert_eq!(model.cells()[0].outputs()[0].outputs()[2].mime, "text/plain");
            assert_eq!(model.cells()[0].outputs()[0].outputs()[2].data.to_string(), "baz");
        },
    );
}

#[test]
fn append_multiple_stdout_stream_output_items_to_an_output_with_another_mime() {
    let _suite = suite_setup();
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    with_test_notebook(
        vec![js_cell_with_outputs("var a = 1;", vec![output_dto("1", vec![item("text/plain", "foo")])])],
        |editor, _view_model, _ds, _services| {
            let model = &editor.text_model;
            let edits = vec![output_items_edit(
                "1",
                Some(true),
                vec![
                    IOutputItemDto { mime: "application/vnd.code.notebook.stdout".to_string(), data: VSBuffer::from_string("bar") },
                    IOutputItemDto { mime: "application/vnd.code.notebook.stdout".to_string(), data: VSBuffer::from_string("baz") },
                ],
            )];
            apply_edits(model, &edits);
            assert_eq!(model.cells().len(), 1);
            assert_eq!(model.cells()[0].outputs().len(), 1);
            assert_eq!(model.cells()[0].outputs()[0].outputs().len(), 3);
            assert_eq!(model.cells()[0].outputs()[0].outputs()[0].mime, "text/plain");
            assert_eq!(model.cells()[0].outputs()[0].outputs()[0].data.to_string(), "foo");
            assert_eq!(model.cells()[0].outputs()[0].outputs()[1].mime, "application/vnd.code.notebook.stdout");
            assert_eq!(model.cells()[0].outputs()[0].outputs()[1].data.to_string(), "bar");
            assert_eq!(model.cells()[0].outputs()[0].outputs()[2].mime, "application/vnd.code.notebook.stdout");
            assert_eq!(model.cells()[0].outputs()[0].outputs()[2].data.to_string(), "baz");
        },
    );
}

#[test]
fn compress_multiple_stdout_stream_output_items() {
    let _suite = suite_setup();
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    with_test_notebook(
        vec![js_cell_with_outputs("var a = 1;", vec![output_dto("1", vec![item("application/vnd.code.notebook.stdout", "foo")])])],
        |editor, _view_model, _ds, _services| {
            let model = &editor.text_model;
            let edits = vec![output_items_edit(
                "1",
                Some(true),
                vec![
                    IOutputItemDto { mime: "application/vnd.code.notebook.stdout".to_string(), data: VSBuffer::from_string("bar") },
                    IOutputItemDto { mime: "application/vnd.code.notebook.stdout".to_string(), data: VSBuffer::from_string("baz") },
                ],
            )];
            apply_edits(model, &edits);
            assert_eq!(model.cells().len(), 1);
            assert_eq!(model.cells()[0].outputs().len(), 1);
            assert_eq!(model.cells()[0].outputs()[0].outputs().len(), 1);
            assert_eq!(model.cells()[0].outputs()[0].outputs()[0].mime, "application/vnd.code.notebook.stdout");
            assert_eq!(model.cells()[0].outputs()[0].outputs()[0].data.to_string(), "foobarbaz");
        },
    );
}

#[test]
fn compress_multiple_stderr_stream_output_items() {
    let _suite = suite_setup();
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    with_test_notebook(
        vec![js_cell_with_outputs("var a = 1;", vec![output_dto("1", vec![item("application/vnd.code.notebook.stderr", "foo")])])],
        |editor, _view_model, _ds, _services| {
            let model = &editor.text_model;
            let edits = vec![output_items_edit(
                "1",
                Some(true),
                vec![
                    IOutputItemDto { mime: "application/vnd.code.notebook.stderr".to_string(), data: VSBuffer::from_string("bar") },
                    IOutputItemDto { mime: "application/vnd.code.notebook.stderr".to_string(), data: VSBuffer::from_string("baz") },
                ],
            )];
            apply_edits(model, &edits);
            assert_eq!(model.cells().len(), 1);
            assert_eq!(model.cells()[0].outputs().len(), 1);
            assert_eq!(model.cells()[0].outputs()[0].outputs().len(), 1);
            assert_eq!(model.cells()[0].outputs()[0].outputs()[0].mime, "application/vnd.code.notebook.stderr");
            assert_eq!(model.cells()[0].outputs()[0].outputs()[0].data.to_string(), "foobarbaz");
        },
    );
}

/// `{ cellIndex, position: new Position(line, column) }`.
fn cell_position(cell_index: usize, line_number: i32, column: i32) -> NotebookCellPosition {
    NotebookCellPosition { cell_index, position: Position::new(line_number, column) }
}

#[test]
fn find_next_match() {
    let _suite = suite_setup();
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    with_test_notebook(
        vec![js_cell("var a = 1;"), js_cell("var b = 2;"), js_cell("var c = 3;"), js_cell("var d = 4;")],
        |_editor, view_model, _ds, _services| {
            let notebook_model = view_model.notebook_document();

            // Test case 1: Find 'var' starting from the first cell
            let mut find_match = notebook_model.find_next_match("var", &cell_position(0, 1, 1), false, false, None, None);
            let found = find_match.as_ref().expect("a busca acha 'var'");
            assert_eq!(found.match_.range.start_line_number, 1);
            assert_eq!(found.match_.range.start_column, 1);

            // Test case 2: Find 'b' starting from the second cell
            find_match = notebook_model.find_next_match("b", &cell_position(1, 1, 1), false, false, None, None);
            let found = find_match.as_ref().expect("a busca acha 'b'");
            assert_eq!(found.match_.range.start_line_number, 1);
            assert_eq!(found.match_.range.start_column, 5);

            // Test case 3: Find 'c' starting from the third cell
            find_match = notebook_model.find_next_match("c", &cell_position(2, 1, 1), false, false, None, None);
            let found = find_match.as_ref().expect("a busca acha 'c'");
            assert_eq!(found.match_.range.start_line_number, 1);
            assert_eq!(found.match_.range.start_column, 5);

            // Test case 4: Find 'd' starting from the fourth cell
            find_match = notebook_model.find_next_match("d", &cell_position(3, 1, 1), false, false, None, None);
            let found = find_match.as_ref().expect("a busca acha 'd'");
            assert_eq!(found.match_.range.start_line_number, 1);
            assert_eq!(found.match_.range.start_column, 5);

            // Test case 5: No match found
            find_match = notebook_model.find_next_match("e", &cell_position(0, 1, 1), false, false, None, None);
            assert!(find_match.is_none());
        },
    );
}

#[test]
fn find_next_match_2() {
    let _suite = suite_setup();
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    with_test_notebook(
        vec![js_cell("var a = 1; var a = 2;"), js_cell("var b = 2;"), js_cell("var c = 3;"), js_cell("var d = 4;")],
        |_editor, view_model, _ds, _services| {
            let notebook_model = view_model.notebook_document();

            // Test case 1: Find 'var' starting from the first cell
            let mut find_match = notebook_model.find_next_match("var", &cell_position(0, 1, 1), false, false, None, None);
            let found = find_match.as_ref().expect("a busca acha 'var'");
            assert_eq!(found.match_.range.start_line_number, 1);
            assert_eq!(found.match_.range.start_column, 1);

            // Test case 2: Find 'b' starting from the second cell
            find_match = notebook_model.find_next_match("b", &cell_position(1, 1, 1), false, false, None, None);
            let found = find_match.as_ref().expect("a busca acha 'b'");
            assert_eq!(found.match_.range.start_line_number, 1);
            assert_eq!(found.match_.range.start_column, 5);

            // Test case 3: Find 'c' starting from the third cell
            find_match = notebook_model.find_next_match("c", &cell_position(2, 1, 1), false, false, None, None);
            let found = find_match.as_ref().expect("a busca acha 'c'");
            assert_eq!(found.match_.range.start_line_number, 1);
            assert_eq!(found.match_.range.start_column, 5);

            // Test case 4: Find 'd' starting from the fourth cell
            find_match = notebook_model.find_next_match("d", &cell_position(3, 1, 1), false, false, None, None);
            let found = find_match.as_ref().expect("a busca acha 'd'");
            assert_eq!(found.match_.range.start_line_number, 1);
            assert_eq!(found.match_.range.start_column, 5);

            // Test case 5: No match found
            find_match = notebook_model.find_next_match("e", &cell_position(0, 1, 1), false, false, None, None);
            assert!(find_match.is_none());

            // Test case 6: Same keywords in the same cell
            find_match = notebook_model.find_next_match("var", &cell_position(0, 1, 1), false, false, None, None);
            let found = find_match.as_ref().expect("a busca acha 'var'");
            assert_eq!(found.match_.range.start_line_number, 1);
            assert_eq!(found.match_.range.start_column, 1);

            find_match = notebook_model.find_next_match("var", &cell_position(0, 1, 5), false, false, None, None);
            let found = find_match.as_ref().expect("a busca acha 'var'");
            assert_eq!(found.match_.range.start_line_number, 1);
            assert_eq!(found.match_.range.start_column, 12);

            // Test case 7: Search from the middle of a cell with keyword before and after
            find_match = notebook_model.find_next_match("a", &cell_position(0, 1, 10), false, false, None, None);
            let found = find_match.as_ref().expect("a busca acha 'a'");
            assert_eq!(found.match_.range.start_line_number, 1);
            assert_eq!(found.match_.range.start_column, 13);

            // Test case 8: Search from a cell and next match is in another cell below
            find_match = notebook_model.find_next_match("var", &cell_position(0, 1, 20), false, false, None, None);
            let found = find_match.as_ref().expect("a busca acha 'var'");
            assert_eq!(found.match_.range.start_line_number, 1);
            assert_eq!(found.match_.range.start_column, 1);
            // assert.strictEqual(match!.cellIndex, 1);
        },
    );
}
