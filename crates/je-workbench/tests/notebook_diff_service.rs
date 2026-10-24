//! Porte de `src/vs/workbench/contrib/notebook/test/browser/diff/notebookDiffService.test.ts` (suíte `NotebookDiff Diff Service`), caso a caso e na mesma ordem.
//!
//! Os 53 casos estão portados. Os 9 últimos (`Detect modification and insertion of cells`, `Detect modification in 2 cells`,
//! `Detect modification in multiple cells`, `Modification with Insertion, detected as deletion of a cell`,
//! `Modification of three cells`, `Modification of multiple cells`, `Modification of a large cell and insert a new
//! cell`, `Misalignment due to 2 deleted cells` e `Insert a few cells (one on top), modify a few and completely
//! misaligned`) usam notebooks Jupyter reais de milhares de linhas, que o original escreve inline no teste: as fixtures são
//! os arquivos `tests/fixtures/notebook_diff/<caso>.original.json` e `<caso>.modified.json`, cada um com um array de
//! `{ "cell_type", "source" }` (os dois campos que o `fromJupyterCell` lê; o resto de cada célula Jupyter, que o original
//! nunca usa, não foi copiado), lidos por `include_str!` ([`jupyter_cells`]).
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` é o `LeakGuard` criado no começo de cada caso. O `suiteSetup` que cria
//!   o `NotebookWorker` (e o `suiteTeardown` que o descarta) é um worker novo por caso ([`map_cells`]); os modelos do
//!   worker são indexados por URI única (o contador `uriCounter`), então um worker por caso e um por suíte dão o mesmo
//!   resultado.
//! - `MockNotebookCell` é [`MockCell`]; `[[lines], lang, kind, outputs, metadata]` são os construtores [`cell`], [`js`],
//!   [`py`] e [`md`]. `undefined` de metadados é `None` e `{}` é `Some(objeto vazio)`; os literais
//!   `{ metadata: { collapsed }, executionOrder }` são [`meta`].
//! - `assert.deepStrictEqual(mapping, [{ modified, original }])` compara pares `(modified, original)`, e as mudanças
//!   `{ originalStart, originalLength, modifiedStart, modifiedLength }` são tuplas nessa ordem.
//! - `cellToDto` é [`CellDto`] (o `ICell` do casamento por similaridade) e `cellToMainCellTdo` é [`cell_to_main_cell_dto`].
//! - `$computeDiff` (um `async` sem `await`) devolve o resultado direto.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod support;

use std::cell::Cell;
use std::collections::HashMap;
use std::rc::Rc;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::JsString;
use je_base::buffer::VSBuffer;
use je_base::dynamic::Dynamic;
use je_base::mime::mimes;
use je_base::uri::{URI, UriComponents};
use je_workbench::contrib::notebook::common::notebook_common::{CellKind, IMainCellDto, INotebookDiffResult, IOutputDto, IOutputItemDto, NotebookCellMetadata};
use je_workbench::contrib::notebook::common::services::notebook_cell_matching::{ICell, match_cell_based_on_similarties};
use je_workbench::contrib::notebook::common::services::notebook_web_worker::NotebookWorker;
use support::test_notebook_editor::empty_metadata;

/// `MockNotebookCell`.
struct MockCell {
    lines: Vec<String>,
    lang: &'static str,
    kind: CellKind,
    outputs: Vec<IOutputDto>,
    metadata: Option<NotebookCellMetadata>,
}

fn cell(lines: &[&str], lang: &'static str, kind: CellKind, outputs: Vec<IOutputDto>, metadata: Option<NotebookCellMetadata>) -> MockCell {
    MockCell { lines: lines.iter().map(|line| (*line).to_owned()).collect(), lang, kind, outputs, metadata }
}

/// `[lines, 'javascript', CellKind::Code, [], {}]`.
fn js(lines: &[&str]) -> MockCell {
    cell(lines, "javascript", CellKind::Code, Vec::new(), Some(empty_metadata()))
}

/// `[lines, 'python', CellKind::Code, [], undefined]`.
fn py(lines: &[&str]) -> MockCell {
    cell(lines, "python", CellKind::Code, Vec::new(), None)
}

/// `[lines, 'markdown', CellKind::Markup, [], undefined]`.
fn md(lines: &[&str]) -> MockCell {
    cell(lines, "markdown", CellKind::Markup, Vec::new(), None)
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

/// O `cellToDto` do original.
struct CellDto {
    kind: CellKind,
    lines: Vec<String>,
}

impl ICell for CellDto {
    fn internal_id(&self) -> Option<String> {
        None
    }

    fn get_value(&self) -> JsString {
        JsString::from(self.lines.join("\n"))
    }

    fn get_lines_content(&self) -> Vec<JsString> {
        self.lines.iter().map(|line| JsString::from(line.as_str())).collect()
    }

    fn cell_kind(&self) -> CellKind {
        self.kind
    }
}

fn cell_to_dto(cell: &MockCell) -> Rc<dyn ICell> {
    Rc::new(CellDto { kind: cell.kind, lines: cell.lines.clone() })
}

fn cell_to_main_cell_dto(notebook_uri: &URI, cell: &MockCell, index: usize) -> IMainCellDto {
    let url = URI::from(
        &UriComponents {
            scheme: Some(JsString::from("file")),
            path: Some(notebook_uri.path().clone()),
            fragment: Some(JsString::from(format!("cell/{index}"))),
            ..Default::default()
        },
        false,
    );
    IMainCellDto {
        source: cell.lines.clone(),
        language: cell.lang.to_owned(),
        cell_kind: cell.kind,
        outputs: cell.outputs.clone(),
        metadata: cell.metadata.clone(),
        eol: "\n".to_owned(),
        handle: index as i64,
        url: url.to_string(false).to_string_lossy(),
        version_id: 0,
        internal_metadata: None,
    }
}

thread_local! {
    static URI_COUNTER: Cell<u32> = const { Cell::new(0) };
}

fn next_uri_counter() -> u32 {
    URI_COUNTER.with(|counter| {
        let value = counter.get();
        counter.set(value + 1);
        value
    })
}

/// `mapCells(original, modified)`: o `mapping` como `(modified, original)` e o diff do worker.
fn map_cells(original: Vec<MockCell>, modified: Vec<MockCell>) -> (Vec<(i64, i64)>, INotebookDiffResult) {
    let worker = NotebookWorker::new();
    let modified_dtos: Vec<_> = modified.iter().map(cell_to_dto).collect();
    let original_dtos: Vec<_> = original.iter().map(cell_to_dto).collect();
    let mapping: Vec<(i64, i64)> = match_cell_based_on_similarties(&modified_dtos, &original_dtos).into_iter().map(|m| (m.modified, m.original)).collect();
    let original_uri = URI::parse(format!("file://original{}.ipynb", next_uri_counter()), false);
    let modified_uri = URI::parse(format!("file://modified{}.ipynb", next_uri_counter()), false);
    worker.accept_new_model(
        &original_uri.to_string(false).to_string_lossy(),
        empty_metadata(),
        HashMap::new(),
        original.iter().enumerate().map(|(index, cell)| cell_to_main_cell_dto(&original_uri, cell, index)).collect(),
    );
    worker.accept_new_model(
        &modified_uri.to_string(false).to_string_lossy(),
        empty_metadata(),
        HashMap::new(),
        modified.iter().enumerate().map(|(index, cell)| cell_to_main_cell_dto(&modified_uri, cell, index)).collect(),
    );
    let diff = worker.compute_diff(&original_uri.to_string(false).to_string_lossy(), &modified_uri.to_string(false).to_string_lossy());
    worker.dispose();
    (mapping, diff)
}

/// `{ originalStart, originalLength, modifiedStart, modifiedLength }` de cada mudança.
fn changes(diff: &INotebookDiffResult) -> Vec<(i32, i32, i32, i32)> {
    diff.cells_diff.changes.iter().map(|c| (c.original_start, c.original_length, c.modified_start, c.modified_length)).collect()
}

/// A célula `x` com saída e metadados que a maioria dos casos do início usa.
fn x_cell(lines: &[&str], output_id: &str, byte: u8, order: f64) -> MockCell {
    cell(lines, "javascript", CellKind::Code, out(output_id, byte), meta(Some(false), Some(order)))
}

#[test]
fn no_changes() {
    let _disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let (mapping, diff) = map_cells(vec![x_cell(&["x"], "someOtherId", 3, 3.0)], vec![x_cell(&["x"], "someOtherId", 3, 3.0)]);

    assert_eq!(mapping, [(0, 0)]);
    assert_eq!(diff.cells_diff.changes.len(), 0);
}

#[test]
fn diff_different_source() {
    let _disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let (mapping, diff) = map_cells(vec![x_cell(&["x"], "someOtherId", 3, 3.0)], vec![x_cell(&["y"], "someOtherId", 3, 3.0)]);

    assert_eq!(mapping, [(0, 0)]);
    assert_eq!(changes(&diff), [(0, 1, 0, 1)]);
}

#[test]
fn diff_different_source_2_cells() {
    let _disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let (mapping, diff) = map_cells(
        vec![x_cell(&["x"], "someOtherId", 3, 3.0), x_cell(&["y"], "someOtherId", 3, 3.0)],
        vec![x_cell(&["x"], "someOtherId", 3, 3.0), x_cell(&["z"], "someOtherId", 3, 3.0)],
    );

    assert_eq!(mapping, [(0, 0), (1, 1)]);
    assert_eq!(changes(&diff), [(1, 1, 1, 1)]);
}

#[test]
fn diff_different_source_first_cell_changed() {
    let _disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let (mapping, diff) = map_cells(
        vec![x_cell(&["import sys"], "someOtherId", 3, 3.0), x_cell(&["y"], "someOtherId", 3, 3.0)],
        vec![x_cell(&["import pandas"], "someOtherId", 3, 3.0), x_cell(&["y"], "someOtherId", 3, 3.0)],
    );

    assert_eq!(mapping, [(0, 0), (1, 1)]);
    assert_eq!(changes(&diff), [(0, 1, 0, 1)]);
}

#[test]
fn diff_test_small_source() {
    let _disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let (mapping, diff) = map_cells(vec![js(&["123456789"])], vec![js(&["987654321"])]);

    assert_eq!(mapping, [(0, 0)]);
    assert_eq!(changes(&diff), [(0, 1, 0, 1)]);
}

#[test]
fn diff_test_data_single_cell() {
    let _disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let (mapping, diff) = map_cells(
        vec![js(&["# This version has a bug\n", "def mult(a, b):\n", "    return a / b"])],
        vec![js(&["def mult(a, b):\n", "    'This version is debugged.'\n", "    return a * b"])],
    );

    assert_eq!(mapping, [(0, 0)]);
    assert_eq!(changes(&diff), [(0, 1, 0, 1)]);
}

#[test]
fn diff_foo_foe_swapped_cells() {
    let _disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let foe = ["def foe(x, y):\n", "    return x + y\n", "foe(3, 2)"];
    let foo = ["def foo(x, y):\n", "    return x * y\n", "foo(1, 2)"];
    let (mapping, diff) = map_cells(
        vec![x_cell(&foe, "someId", 6, 5.0), x_cell(&foo, "someId", 2, 6.0), js(&[""])],
        vec![x_cell(&foo, "someId", 2, 6.0), x_cell(&foe, "someId", 6, 5.0), js(&[""])],
    );

    assert_eq!(mapping, [(0, 1), (1, 0), (2, 2)]);
    assert_eq!(changes(&diff), [(0, 0, 0, 1), (1, 1, 2, 0)]);
}

#[test]
fn diff_foo_foe_swapped_cells_with_changes_in_output() {
    let _disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let foe = ["def foe(x, y):\n", "    return x + y\n", "foe(3, 2)"];
    let foo = ["def foo(x, y):\n", "    return x * y\n", "foo(1, 2)"];
    let (mapping, diff) = map_cells(
        vec![x_cell(&foe, "someId", 6, 5.0), x_cell(&foo, "someId", 2, 6.0), js(&[""])],
        vec![x_cell(&foo, "someId", 6, 5.0), x_cell(&foe, "someId", 2, 6.0), js(&[""])],
    );

    assert_eq!(mapping, [(0, 1), (1, 0), (2, 2)]);
    assert_eq!(changes(&diff), [(0, 2, 0, 2)]);
}

#[test]
fn diff_markdown() {
    let _disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let (mapping, diff) = map_cells(
        vec![
            cell(&["This is a test notebook with only markdown cells"], "markdown", CellKind::Markup, Vec::new(), Some(empty_metadata())),
            cell(&["Lorem ipsum dolor sit amet"], "markdown", CellKind::Markup, Vec::new(), Some(empty_metadata())),
            cell(&["In other news"], "markdown", CellKind::Markup, Vec::new(), Some(empty_metadata())),
        ],
        vec![
            cell(&["This is a test notebook with markdown cells only"], "markdown", CellKind::Markup, Vec::new(), Some(empty_metadata())),
            cell(&["Lorem ipsum dolor sit amet"], "markdown", CellKind::Markup, Vec::new(), Some(empty_metadata())),
            cell(&["In the news"], "markdown", CellKind::Markup, Vec::new(), Some(empty_metadata())),
        ],
    );

    assert_eq!(mapping, [(0, 0), (1, 1), (2, 2)]);
    assert_eq!(changes(&diff), [(0, 1, 0, 1), (2, 1, 2, 1)]);
}

#[test]
fn diff_insert_1_at_the_top() {
    let _disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let (mapping, diff) = map_cells(vec![js(&["var a = 1;"]), js(&["var b = 2;"])], vec![js(&["var h = 8;"]), js(&["var a = 1;"]), js(&["var b = 2;"])]);

    assert_eq!(mapping, [(0, -1), (1, 0), (2, 1)]);
    assert_eq!(changes(&diff), [(0, 0, 0, 1)]);
}

#[test]
fn diff_insert_1_at_the_top_with_lots_of_cells() {
    let _disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let (mapping, diff) = map_cells(
        vec![js(&["var a = 1;"]), js(&["var b = 2;"]), js(&["var c = 3;"]), js(&["var d = 4;"]), js(&["var e = 5;"]), js(&["var f = 6;"]), js(&["var g = 7;"])],
        vec![
            js(&["var h = 8;"]),
            js(&["var a = 1;"]),
            js(&["var b = 2;"]),
            js(&["var c = 3;"]),
            js(&["var d = 4;"]),
            js(&["var e = 5;"]),
            js(&["var f = 6;"]),
            js(&["var g = 7;"]),
        ],
    );

    assert_eq!(mapping, [(0, -1), (1, 0), (2, 1), (3, 2), (4, 3), (5, 4), (6, 5), (7, 6)]);
    assert_eq!(changes(&diff), [(0, 0, 0, 1)]);
}

#[test]
fn diff_insert_3_at_the_top_with_lots_of_cells() {
    let _disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let (mapping, diff) = map_cells(
        vec![js(&["var a = 1;"]), js(&["var b = 2;"]), js(&["var c = 3;"]), js(&["var d = 4;"]), js(&["var e = 5;"]), js(&["var f = 6;"]), js(&["var g = 7;"])],
        vec![
            js(&["var h = 8;"]),
            js(&["var i = 8;"]),
            js(&["var j = 8;"]),
            js(&["var a = 1;"]),
            js(&["var b = 2;"]),
            js(&["var c = 3;"]),
            js(&["var d = 4;"]),
            js(&["var e = 5;"]),
            js(&["var f = 6;"]),
            js(&["var g = 7;"]),
        ],
    );

    assert_eq!(mapping, [(0, -1), (1, -1), (2, -1), (3, 0), (4, 1), (5, 2), (6, 3), (7, 4), (8, 5), (9, 6)]);
    assert_eq!(changes(&diff), [(0, 0, 0, 3)]);
}

#[test]
fn diff_insert_1_in_the_middle() {
    let _disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let (mapping, diff) = map_cells(
        vec![js(&["var a = 1;"]), js(&["var b = 2;"]), js(&["var c = 3;"]), js(&["var d = 4;"]), js(&["var e = 5;"]), js(&["var f = 6;"]), js(&["var g = 7;"])],
        vec![
            js(&["var a = 1;"]),
            js(&["var b = 2;"]),
            js(&["var c = 3;"]),
            js(&["var d = 4;"]),
            js(&["var h = 8;"]),
            js(&["var e = 5;"]),
            js(&["var f = 6;"]),
            js(&["var g = 7;"]),
        ],
    );

    assert_eq!(mapping, [(0, 0), (1, 1), (2, 2), (3, 3), (4, -1), (5, 4), (6, 5), (7, 6)]);
    assert_eq!(changes(&diff), [(4, 0, 4, 1)]);
}

/// `{ metadata: { collapsed: true }, executionOrder: 1 }` e companhia, dos casos de LCS.
fn lcs_cells(first_output: bool, second_output: bool) -> Vec<MockCell> {
    vec![
        cell(&["# Description"], "markdown", CellKind::Markup, Vec::new(), meta(None, None)),
        cell(&["x = 3"], "javascript", CellKind::Code, Vec::new(), meta(Some(true), Some(1.0))),
        cell(
            &["x"],
            "javascript",
            CellKind::Code,
            if first_output { out("someId", 3) } else { Vec::new() },
            if first_output { meta(Some(false), Some(1.0)) } else { meta(Some(false), None) },
        ),
        cell(
            &["x"],
            "javascript",
            CellKind::Code,
            if second_output { out("someId", 3) } else { Vec::new() },
            if second_output { meta(Some(false), Some(1.0)) } else { meta(Some(false), None) },
        ),
    ]
}

#[test]
fn lcs_swap_cells_with_outputs() {
    let _disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let (mapping, diff) = map_cells(lcs_cells(true, false), lcs_cells(false, true));

    assert_eq!(mapping, [(0, 0), (1, 1), (2, 2), (3, 3)]);
    assert_eq!(changes(&diff), [(2, 1, 2, 1), (3, 1, 3, 1)]);
}

#[test]
fn lcs_outputs_changed() {
    let _disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let (mapping, diff) = map_cells(
        vec![
            cell(&["# Description"], "markdown", CellKind::Markup, Vec::new(), meta(None, None)),
            cell(&["x = 3"], "javascript", CellKind::Code, Vec::new(), meta(Some(true), Some(1.0))),
            cell(&["x"], "javascript", CellKind::Code, out("someId", 3), meta(Some(false), Some(1.0))),
            cell(&["y"], "javascript", CellKind::Code, Vec::new(), meta(Some(false), None)),
        ],
        vec![
            cell(&["# Description"], "markdown", CellKind::Markup, Vec::new(), meta(None, None)),
            cell(&["x = 3"], "javascript", CellKind::Code, Vec::new(), meta(Some(true), Some(1.0))),
            cell(&["x"], "javascript", CellKind::Code, Vec::new(), meta(Some(true), None)),
            cell(&["y"], "javascript", CellKind::Code, Vec::new(), meta(Some(false), None)),
        ],
    );

    assert_eq!(mapping, [(0, 0), (1, 1), (2, 2), (3, 3)]);
    assert_eq!(changes(&diff), [(2, 1, 2, 1)]);
}

#[test]
fn swap_cells_with_output_and_with_same_code_in_each_cell() {
    let _disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let (mapping, diff) = map_cells(
        vec![
            cell(&["# Description"], "markdown", CellKind::Markup, Vec::new(), meta(None, None)),
            cell(&["x = 3"], "javascript", CellKind::Code, Vec::new(), meta(Some(true), Some(1.0))),
            cell(&["x"], "javascript", CellKind::Code, out("someId", 3), meta(Some(false), Some(1.0))),
            cell(&["x"], "javascript", CellKind::Code, Vec::new(), meta(Some(false), None)),
            js(&["x = 5"]),
            js(&["x"]),
            cell(&["x"], "javascript", CellKind::Code, out("someId", 5), Some(empty_metadata())),
        ],
        vec![
            cell(&["# Description"], "markdown", CellKind::Markup, Vec::new(), meta(None, None)),
            cell(&["x = 3"], "javascript", CellKind::Code, Vec::new(), meta(Some(true), Some(1.0))),
            cell(&["x"], "javascript", CellKind::Code, Vec::new(), meta(Some(false), None)),
            cell(&["x"], "javascript", CellKind::Code, out("someId", 3), meta(Some(false), Some(1.0))),
            js(&["x = 5"]),
            cell(&["x"], "javascript", CellKind::Code, out("someId", 5), Some(empty_metadata())),
            js(&["x"]),
        ],
    );

    assert_eq!(mapping, [(0, 0), (1, 1), (2, 2), (3, 3), (4, 4), (5, 5), (6, 6)]);
    assert_eq!(changes(&diff), [(2, 1, 2, 1), (3, 1, 3, 1), (5, 1, 5, 1), (6, 1, 6, 1)]);
}

#[test]
fn change_outputs_of_cells() {
    let _disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let (mapping, diff) = map_cells(
        vec![
            cell(&["# Description"], "markdown", CellKind::Markup, Vec::new(), meta(None, None)),
            cell(&["x = 3"], "javascript", CellKind::Code, Vec::new(), meta(Some(true), Some(1.0))),
            cell(&["x"], "javascript", CellKind::Code, out("someId", 3), meta(Some(false), Some(1.0))),
            cell(&["y"], "javascript", CellKind::Code, Vec::new(), meta(Some(false), None)),
            js(&["x = 5"]),
            js(&["x"]),
            cell(&["y"], "javascript", CellKind::Code, out("someId", 5), Some(empty_metadata())),
        ],
        vec![
            cell(&["# Description"], "markdown", CellKind::Markup, Vec::new(), meta(None, None)),
            cell(&["x = 3"], "javascript", CellKind::Code, Vec::new(), meta(Some(true), Some(1.0))),
            cell(&["x"], "javascript", CellKind::Code, Vec::new(), meta(Some(false), None)),
            cell(&["y"], "javascript", CellKind::Code, out("someId", 3), meta(Some(false), Some(1.0))),
            js(&["x = 5"]),
            cell(&["x"], "javascript", CellKind::Code, out("someId", 5), Some(empty_metadata())),
            js(&["y"]),
        ],
    );

    assert_eq!(mapping, [(0, 0), (1, 1), (2, 2), (3, 3), (4, 4), (5, 5), (6, 6)]);
    assert_eq!(changes(&diff), [(2, 2, 2, 2), (5, 2, 5, 2)]);
}

#[test]
fn insert_a_new_cell() {
    let _disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let (mapping, diff) = map_cells(
        vec![js(&["var a = 1;"]), js(&["var b = 2;"]), js(&["var c = 3;"]), js(&["var d = 4;"]), js(&["var e = 5;"]), js(&["var f = 6;"]), js(&["var g = 7;"])],
        vec![
            js(&["var a = 1;"]),
            js(&["var b = 2;"]),
            js(&["var c = 3;"]),
            js(&["var d = 4;"]),
            js(&["var h = 8;"]),
            js(&["var e = 5;"]),
            js(&["var f = 6;"]),
            js(&["var g = 7;"]),
        ],
    );

    assert_eq!(mapping, [(0, 0), (1, 1), (2, 2), (3, 3), (4, -1), (5, 4), (6, 5), (7, 6)]);
    assert_eq!(changes(&diff), [(4, 0, 4, 1)]);
}

#[test]
fn diff_output() {
    let _disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let (mapping, diff) = map_cells(
        vec![x_cell(&["x"], "someOtherId", 3, 3.0), x_cell(&["y"], "someOtherId", 4, 3.0)],
        vec![x_cell(&["x"], "someOtherId", 3, 3.0), x_cell(&["y"], "someOtherId", 5, 3.0)],
    );

    assert_eq!(mapping, [(0, 0), (1, 1)]);
    assert_eq!(changes(&diff), [(1, 1, 1, 1)]);
}

#[test]
fn diff_output_fast_check() {
    let _disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let (mapping, diff) = map_cells(
        vec![x_cell(&["x"], "someOtherId", 3, 3.0), x_cell(&["y"], "someOtherId", 4, 3.0)],
        vec![x_cell(&["x"], "someOtherId", 3, 3.0), x_cell(&["y"], "someOtherId", 5, 3.0)],
    );

    assert_eq!(mapping, [(0, 0), (1, 1)]);
    assert_eq!(changes(&diff), [(1, 1, 1, 1)]);
}

#[test]
fn no_cells() {
    let _disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let (mapping, diff) = map_cells(Vec::new(), Vec::new());

    assert!(mapping.is_empty());
    assert!(changes(&diff).is_empty());
}

#[test]
fn no_cells_and_add_1_cell() {
    let _disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let (mapping, diff) = map_cells(Vec::new(), vec![py(&[r#"print("Hello world")"#])]);

    assert_eq!(mapping, [(0, -1)]);
    assert_eq!(changes(&diff), [(0, 0, 0, 1)]);
}

#[test]
fn one_cells_and_delete_1_cell() {
    let _disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let (mapping, diff) = map_cells(vec![py(&[r#"print("Hello world")"#])], Vec::new());

    assert!(mapping.is_empty());
    assert_eq!(changes(&diff), [(0, 1, 0, 0)]);
}

#[test]
fn no_changes_with_1_cell() {
    let _disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let (mapping, diff) = map_cells(vec![py(&[r#"print("Hello world")"#])], vec![py(&[r#"print("Hello world")"#])]);

    assert_eq!(mapping, [(0, 0)]);
    assert!(changes(&diff).is_empty());
}

#[test]
fn one_changes_with_1_cell() {
    let _disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let (mapping, diff) = map_cells(vec![py(&[r#"print("Hello world")"#])], vec![py(&[r#"print("Foo Bar")"#])]);

    assert_eq!(mapping, [(0, 0)]);
    assert_eq!(changes(&diff), [(0, 1, 0, 1)]);
}

#[test]
fn no_changes_with_2_cells() {
    let _disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let (mapping, diff) = map_cells(
        vec![md(&["# Hello World"]), py(&[r#"print("Hello world")"#])],
        vec![md(&["# Hello World"]), py(&[r#"print("Hello world")"#])],
    );

    assert_eq!(mapping, [(0, 0), (1, 1)]);
    assert!(changes(&diff).is_empty());
}

#[test]
fn one_changes_with_2_cells_1st_cell_changed() {
    let _disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let (mapping, diff) = map_cells(
        vec![md(&["# Hello World"]), py(&[r#"print("Hello world")"#])],
        vec![md(&["# Foo Bar"]), py(&[r#"print("Hello world")"#])],
    );

    assert_eq!(mapping, [(0, 0), (1, 1)]);
    assert_eq!(changes(&diff), [(0, 1, 0, 1)]);
}

#[test]
fn two_cells_1st_cell_deleted() {
    let _disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let (mapping, diff) = map_cells(vec![md(&["# Hello World"]), py(&[r#"print("Hello world")"#])], vec![py(&[r#"print("Hello world")"#])]);

    assert_eq!(mapping, [(0, 1)]);
    assert_eq!(changes(&diff), [(0, 1, 0, 0)]);
}

#[test]
fn two_cells_inserted_a_cell_in_the_middle() {
    let _disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let (mapping, diff) = map_cells(
        vec![py(&[r#"print("Hello world")"#]), py(&[r#"print("Foo Bar")"#])],
        vec![py(&[r#"print("Hello world")"#]), py(&[r#"print("Bar Baz")"#]), py(&[r#"print("Foo Bar")"#])],
    );

    assert_eq!(mapping, [(0, 0), (1, -1), (2, 1)]);
    assert_eq!(changes(&diff), [(1, 0, 1, 1)]);
}

#[test]
fn one_changes_with_2_cells_2nd_cell_changed() {
    let _disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let (mapping, diff) = map_cells(
        vec![md(&["# Hello World"]), py(&[r#"print("Hello world")"#])],
        vec![md(&["# Hello World"]), py(&[r#"print("Foo Bar")"#])],
    );

    assert_eq!(mapping, [(0, 0), (1, 1)]);
    assert_eq!(changes(&diff), [(1, 1, 1, 1)]);
}

#[test]
fn two_cells_all_cells_changed() {
    let _disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let (mapping, diff) = map_cells(
        vec![md(&["# Hello World"]), py(&[r#"print("Hello world")"#])],
        vec![md(&["# Foo Bar"]), py(&[r#"print("Foo Bar")"#])],
    );

    assert_eq!(mapping, [(0, 0), (1, 1)]);
    assert_eq!(changes(&diff), [(0, 2, 0, 2)]);
}

#[test]
fn insert_a_md_cell_and_modify_the_next_code_cell_few_cells() {
    let _disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let (mapping, diff) = map_cells(
        vec![py(&[r#"print("Hello world")"#]), py(&[r#"print("foo bar")"#])],
        vec![md(&["# Description"]), py(&[r#"print("Hello worldz")"#]), py(&[r#"print("foo bar")"#])],
    );

    assert_eq!(mapping, [(0, -1), (1, 0), (2, 1)]);
    assert_eq!(changes(&diff), [(0, 0, 0, 1), (0, 1, 1, 1)]);
}

#[test]
fn insert_a_md_cell_and_modify_the_next_code_cell() {
    let _disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let (mapping, diff) = map_cells(
        vec![
            md(&["# Hello World"]),
            py(&[r#"print("Hello world")"#]),
            md(&["# Foo Bar"]),
            py(&[r#"print("foo bar")"#]),
            py(&[r#"print("bar baz1234")"#]),
        ],
        vec![
            md(&["# Hello World"]),
            py(&[r#"print("Hello world")"#]),
            md(&["# Foo Bar"]),
            md(&["# New Markdown cell"]),
            py(&[r#"print("foo baz")"#]),
            py(&[r#"print("bar baz1234")"#]),
        ],
    );

    assert_eq!(mapping, [(0, 0), (1, 1), (2, 2), (3, -1), (4, 3), (5, 4)]);
    assert_eq!(changes(&diff), [(3, 0, 3, 1), (3, 1, 4, 1)]);
}

#[test]
fn delete_code_cell_and_insert_md_cell_both_will_not_match() {
    let _disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let (mapping, diff) = map_cells(
        vec![
            md(&["# Hello World"]),
            py(&["import sys"]),
            py(&["sys.executable"]),
            py(&[r#"print("Hello world")"#]),
            py(&[r#"print("foo bar")"#]),
        ],
        vec![
            md(&["# Hello World"]),
            py(&["import sys"]),
            cell(&["# Header"], "python", CellKind::Markup, Vec::new(), None),
            py(&[r#"print("Hello world")"#]),
            py(&[r#"print("foo bar")"#]),
        ],
    );

    assert_eq!(mapping, [(0, 0), (1, 1), (2, -1), (3, 3), (4, 4)]);
    assert_eq!(changes(&diff), [(2, 1, 2, 1)]);
}

#[test]
fn delete_md_cell_and_insert_code_cell_both_will_not_match() {
    let _disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let (mapping, diff) = map_cells(
        vec![
            md(&["# Hello World"]),
            py(&["import sys"]),
            py(&["sys.executable"]),
            py(&[r#"print("Hello world")"#]),
            py(&[r#"print("foo bar")"#]),
        ],
        vec![
            md(&["# Hello World"]),
            py(&["import sys"]),
            cell(&["# Header"], "python", CellKind::Markup, Vec::new(), None),
            py(&[r#"print("Hello world")"#]),
            py(&[r#"print("foo bar")"#]),
        ],
    );

    assert_eq!(mapping, [(0, 0), (1, 1), (2, -1), (3, 3), (4, 4)]);
    assert_eq!(changes(&diff), [(2, 1, 2, 1)]);
}

#[test]
fn insert_a_md_cell_and_modify_the_next_2_code_cells() {
    let _disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let (mapping, diff) = map_cells(
        vec![
            md(&["# Hello World"]),
            py(&[r#"print("Hello world")"#]),
            md(&["# Foo Bar"]),
            py(&[r#"print("Foo Bar")"#]),
            py(&[r#"print("bar baz")"#]),
        ],
        vec![
            md(&["# Hello World"]),
            py(&[r#"print("Hello world")"#]),
            md(&["# Foo Bar"]),
            md(&["# New Markdown cell"]),
            py(&[r#"print("Foo BaZ")"#]),
            py(&[r#"print("bar baz Modified")"#]),
        ],
    );

    assert_eq!(mapping, [(0, 0), (1, 1), (2, 2), (3, -1), (4, 3), (5, 4)]);
    assert_eq!(changes(&diff), [(3, 0, 3, 1), (3, 1, 4, 1), (4, 1, 5, 1)]);
}

#[test]
fn insert_a_md_cell_delete_a_cell_and_modify_the_next_2_code_cells() {
    let _disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let (mapping, diff) = map_cells(
        vec![
            md(&["# Hello World"]),
            py(&[r#"print("Hello world")"#]),
            md(&["# Foo Bar"]),
            py(&[r#"print("Foo Bar")"#]),
            py(&[r#"print("bar baz")"#]),
            py(&[r#"print("Fox Trot")"#]),
        ],
        vec![
            md(&["# Hello World"]),
            py(&[r#"print("Hello world")"#]),
            md(&["# Foo Bar"]),
            md(&["# New Markdown cell"]),
            // [['print("Foo BaZ")'], 'python', CellKind::Code, [], undefined],
            py(&[r#"print("bar baz Modified")"#]),
            py(&[r#"print("Fox Trots")"#]),
        ],
    );

    assert_eq!(mapping, [(0, 0), (1, 1), (2, 2), (3, -1), (4, 4), (5, 5)]);
    assert_eq!(changes(&diff), [(3, 1, 3, 1), (4, 1, 4, 1), (5, 1, 5, 1)]);
}

#[test]
fn insert_a_md_cell_delete_a_code_cell_and_modify_the_next_2_code_cells_deleted_and_inserted_lineup() {
    let _disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let (mapping, diff) = map_cells(
        vec![
            md(&["# Hello World"]),
            py(&[r#"print("Hello world")"#]),
            py(&["import sys"]),
            py(&[r#"print("Foo Bar")"#]),
            py(&[r#"print("bar baz")"#]),
            py(&[r#"print("Fox Trot")"#]),
        ],
        vec![
            md(&["# Hello World"]),
            py(&[r#"print("Hello world")"#]),
            py(&["import sys"]),
            py(&["sys.executable"]),
            // [['print("Foo BaZ")'], 'python', CellKind::Code, [], undefined],
            py(&[r#"print("bar baz Modified")"#]),
            py(&[r#"print("Fox Trot")"#]),
        ],
    );

    assert_eq!(mapping, [(0, 0), (1, 1), (2, 2), (3, 3), (4, 4), (5, 5)]);
    assert_eq!(changes(&diff), [(3, 2, 3, 2)]);
}

/// Os oito (e dez) pares `# ...`/`print("foo bar")` dos casos de vários markdown.
fn few_md_original() -> Vec<MockCell> {
    vec![
        md(&["# Hello World"]),
        py(&[r#"print("Hello world")"#]),
        md(&["# Foo Bar"]),
        py(&[r#"print("foo bar")"#]),
        md(&["# Another MD cell"]),
        py(&[r#"print("foo bar")"#]),
        md(&["# Yet another MD cell"]),
        py(&[r#"print("foo bar")"#]),
    ]
}

fn few_md_modified() -> Vec<MockCell> {
    vec![
        md(&["# Hello World"]),
        py(&[r#"print("Hello world")"#]),
        md(&["# Foo Bar"]),
        py(&[r#"print("foo bar")"#]),
        md(&["# New 1"]),
        md(&["# New 2"]),
        md(&["# Another MD cell (modified1)"]),
        py(&[r#"print("foo bar")"#]),
        md(&["# Another MD cell (modified)"]),
        py(&[r#"print("foo bar")"#]),
    ]
}

#[test]
fn insert_a_few_md_cells_and_modify_the_next_few_code_cells() {
    let _disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let (mapping, diff) = map_cells(few_md_original(), few_md_modified());

    assert_eq!(mapping, [(0, 0), (1, 1), (2, 2), (3, 3), (4, -1), (5, -1), (6, 4), (7, 5), (8, 6), (9, 7)]);
    assert_eq!(changes(&diff), [(4, 0, 4, 2), (4, 1, 6, 1), (6, 1, 8, 1)]);
}

#[test]
fn insert_and_delete_a_few_md_cells_and_modify_the_next_few_code_cells() {
    let _disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let (mapping, diff) = map_cells(few_md_original(), few_md_modified());

    assert_eq!(mapping, [(0, 0), (1, 1), (2, 2), (3, 3), (4, -1), (5, -1), (6, 4), (7, 5), (8, 6), (9, 7)]);
    assert_eq!(changes(&diff), [(4, 0, 4, 2), (4, 1, 6, 1), (6, 1, 8, 1)]);
}

#[test]
fn modify_then_insert() {
    let _disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let (mapping, diff) = map_cells(
        few_md_original(),
        vec![
            md(&["# Hello World"]),
            py(&[r#"print("Hello world")"#]),
            md(&["# Foo BaZ"]),
            md(&["# Header"]),
            py(&[r#"print("foo bar")"#]),
            md(&["# Another MD cell"]),
            py(&[r#"print("foo bar")"#]),
            md(&["# Yet another MD cell"]),
            py(&[r#"print("foo bar")"#]),
        ],
    );

    assert_eq!(mapping, [(0, 0), (1, 1), (2, 2), (3, -1), (4, 3), (5, 4), (6, 5), (7, 6), (8, 7)]);
    assert_eq!(changes(&diff), [(2, 1, 2, 1), (3, 0, 3, 1)]);
}

#[test]
fn modify_then_delete() {
    let _disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let (mapping, diff) = map_cells(
        vec![
            md(&["# Hello World"]),
            py(&[r#"print("Hello world")"#]),
            md(&["# Foo Bar"]),
            py(&[r#"print("foo bar")"#]),
            md(&["# Another MD cell"]),
            py(&["import sys"]),
            md(&["# Yet another MD cell"]),
            py(&["sys.executable"]),
        ],
        vec![
            md(&["# Hello World"]),
            py(&[r#"print("Hello world")"#]),
            md(&["# Foo BaZ"]),
            md(&["# Another MD cell"]),
            py(&["import sys"]),
            md(&["# Yet another MD cell"]),
            py(&["sys.executable"]),
        ],
    );

    assert_eq!(mapping, [(0, 0), (1, 1), (2, 2), (3, 4), (4, 5), (5, 6), (6, 7)]);
    assert_eq!(changes(&diff), [(2, 1, 2, 1), (3, 1, 3, 0)]);
}

#[test]
fn update_code_cell() {
    let _disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let markdown = [
        "# This is a simple notebook\n",
        "\n",
        "There's nothing special here, I am just writing some text and plotting a function",
    ];
    let imports = ["import numpy as np\n", "import matplotlib.pyplot as plt\n", "import pandas as pd\n", "\n", "%matplotlib inline"];
    let dataframe = ["df = pd.DataFrame({f\"column_{c}\": np.random.normal(size=100) for c in range(100)})\n", "df"];
    let green = ["You could actually do some neat stuff with it, pandas like changing the colors (values above 1 should be green)"];
    let style = [r#"df.style.applymap(lambda x: "background-color: green" if x>1 else "background-color: white")"#];
    let (mapping, diff) = map_cells(
        vec![
            cell(&markdown, "markdown", CellKind::Markup, Vec::new(), None),
            py(&imports),
            py(&["x = np.linspace(0, 4*np.pi,50)\n", "y = np.sin(x)\n", "\n", "plt.plot(x, y)"]),
            py(&dataframe),
            cell(&green, "python", CellKind::Markup, Vec::new(), None),
            py(&style),
            py(&[]),
        ],
        vec![
            cell(&markdown, "markdown", CellKind::Markup, Vec::new(), None),
            py(&imports),
            py(&["x = np.linspace(0, 4*np.pi,50)\n", "y = 2 * np.sin(x)\n", "\n", "plt.plot(x, y)"]),
            py(&dataframe),
            cell(&green, "python", CellKind::Markup, Vec::new(), None),
            py(&style),
            py(&[]),
        ],
    );

    assert_eq!(mapping, [(0, 0), (1, 1), (2, 2), (3, 3), (4, 4), (5, 5), (6, 6)]);
    assert_eq!(changes(&diff), [(2, 1, 2, 1)]);
}

#[test]
fn update_code_cell_1() {
    let _disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let data = [
        "data_dir = '../../../app/data/'\n",
        "output_dir = 'output/'\n",
        "video_dir = 'videos/'\n",
        "\n",
        "file_path = data_dir+output_dir+data_file\n",
        "video_path = data_dir+video_dir+video_file\n",
        "\n",
        "df = pd.read_csv(file_path)\n",
        "df.head()",
    ];
    let convert = [
        "# convert the string representation of results to a list\n",
        "df['list_categories'] = df['categories'].apply(ast.literal_eval)\n",
        "\n",
        "# get most likely label from each list\n",
        "df['label'] = df['list_categories'].apply(lambda x: x[0] if x else None)",
    ];
    let encoder = [
        "# initialize the LabelEncoder\n",
        "label_encoder = LabelEncoder()\n",
        "\n",
        "# fit and transform the label to a numerical value\n",
        "numerical_data = label_encoder.fit_transform(df['label'])\n",
        "\n",
        "df['label_value'] = numerical_data",
    ];
    let (mapping, diff) = map_cells(
        vec![
            md(&["# Hello World"]),
            py(&[
                "import os\n",
                "from pprint import pprint \n",
                "from sklearn.preprocessing import LabelEncoder\n",
                "import ast\n",
                "import pandas as pd\n",
                "from clipper import clipper\n",
                "import ffmpeg",
            ]),
            py(&data),
            py(&convert),
            cell(&encoder, "python", CellKind::Markup, Vec::new(), None),
        ],
        vec![
            md(&["# Updated markdown cell"]),
            py(&["from sklearn.preprocessing import LabelEncoder\n", "import ast\n", "import pandas as pd\n", "from clipper import clipper\n", "import ffmpeg"]),
            py(&data),
            py(&convert),
            cell(&encoder, "python", CellKind::Markup, Vec::new(), None),
        ],
    );

    assert_eq!(mapping, [(0, 0), (1, 1), (2, 2), (3, 3), (4, 4)]);
    assert_eq!(changes(&diff), [(0, 2, 0, 2)]);
}

/// `fromJupyterCell(cell)`: o `source` e o `cell_type` da célula Jupyter da fixture (o resto da célula o original nunca lê).
fn from_jupyter_cell(cell: &serde_json::Value) -> MockCell {
    let is_code = cell["cell_type"] == "code";
    MockCell {
        lines: cell["source"].as_array().expect("source").iter().map(|line| line.as_str().expect("linha do source").to_owned()).collect(),
        lang: if is_code { "python" } else { "markdown" },
        kind: if is_code { CellKind::Code } else { CellKind::Markup },
        outputs: Vec::new(),
        metadata: None,
    }
}

/// `[ ...células Jupyter... ].map(fromJupyterCell)`, com as células lidas do arquivo JSON da fixture.
fn jupyter_cells(fixture: &str) -> Vec<MockCell> {
    let cells: serde_json::Value = serde_json::from_str(fixture).expect("fixture JSON");
    cells.as_array().expect("array de células").iter().map(from_jupyter_cell).collect()
}

/// `{ modified: i, original: i }` de 0 até `last` (inclusive).
fn identity_mapping(last: i64) -> Vec<(i64, i64)> {
    (0..=last).map(|index| (index, index)).collect()
}

#[test]
fn detect_modification_and_insertion_of_cells() {
    let _disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let (mapping, diff) = map_cells(
        jupyter_cells(include_str!("fixtures/notebook_diff/detect_modification_and_insertion_of_cells.original.json")),
        jupyter_cells(include_str!("fixtures/notebook_diff/detect_modification_and_insertion_of_cells.modified.json")),
    );

    let mut expected_mapping = identity_mapping(12);
    expected_mapping.extend((13..=21).map(|modified| (modified, modified + 2)));
    assert_eq!(mapping, expected_mapping);
    assert_eq!(changes(&diff), [(2, 1, 2, 1), (5, 1, 5, 1), (6, 1, 6, 1), (7, 1, 7, 1), (8, 1, 8, 1), (12, 1, 12, 1), (13, 2, 13, 0), (16, 1, 14, 1)]);
}

#[test]
fn detect_modification_in_2_cells() {
    let _disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let (mapping, diff) = map_cells(
        jupyter_cells(include_str!("fixtures/notebook_diff/detect_modification_in_2_cells.original.json")),
        jupyter_cells(include_str!("fixtures/notebook_diff/detect_modification_in_2_cells.modified.json")),
    );

    assert_eq!(mapping, identity_mapping(23));
    assert_eq!(changes(&diff), [(1, 1, 1, 1), (4, 1, 4, 1), (20, 1, 20, 1)]);
}

#[test]
fn detect_modification_in_multiple_cells() {
    let _disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let (mapping, diff) = map_cells(
        jupyter_cells(include_str!("fixtures/notebook_diff/detect_modification_in_multiple_cells.original.json")),
        jupyter_cells(include_str!("fixtures/notebook_diff/detect_modification_in_multiple_cells.modified.json")),
    );

    assert_eq!(mapping, identity_mapping(23));
    assert_eq!(changes(&diff), [(4, 1, 4, 1), (10, 1, 10, 1), (16, 1, 16, 1), (18, 1, 18, 1), (20, 1, 20, 1)]);
}

#[test]
fn modification_with_insertion_detected_as_deletion_of_a_cell() {
    let _disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let (mapping, diff) = map_cells(
        jupyter_cells(include_str!("fixtures/notebook_diff/modification_with_insertion_detected_as_deletion_of_a_cell.original.json")),
        jupyter_cells(include_str!("fixtures/notebook_diff/modification_with_insertion_detected_as_deletion_of_a_cell.modified.json")),
    );

    let mut expected_mapping = identity_mapping(49);
    expected_mapping.extend([(50, -1), (51, -1)]);
    assert_eq!(mapping, expected_mapping);
    assert_eq!(changes(&diff), [(49, 1, 49, 1), (50, 0, 50, 2)]);
}

#[test]
fn modification_of_three_cells() {
    let _disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let (mapping, diff) = map_cells(
        jupyter_cells(include_str!("fixtures/notebook_diff/modification_of_three_cells.original.json")),
        jupyter_cells(include_str!("fixtures/notebook_diff/modification_of_three_cells.modified.json")),
    );

    assert_eq!(mapping, identity_mapping(49));
    // O original deixa comentada a mudança `{ originalStart: 50, originalLength: 0, modifiedStart: 50, modifiedLength: 2 }`.
    assert_eq!(changes(&diff), [(6, 1, 6, 1), (15, 1, 15, 1), (36, 1, 36, 1)]);
}

#[test]
fn modification_of_multiple_cells() {
    let _disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let (mapping, diff) = map_cells(
        jupyter_cells(include_str!("fixtures/notebook_diff/modification_of_multiple_cells.original.json")),
        jupyter_cells(include_str!("fixtures/notebook_diff/modification_of_multiple_cells.modified.json")),
    );

    assert_eq!(mapping, identity_mapping(46));
    assert_eq!(changes(&diff), [(5, 2, 5, 2), (25, 1, 25, 1), (42, 1, 42, 1), (44, 1, 44, 1), (46, 1, 46, 1)]);
}

#[test]
fn modification_of_a_large_cell_and_insert_a_new_cell() {
    let _disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let (mapping, diff) = map_cells(
        jupyter_cells(include_str!("fixtures/notebook_diff/modification_of_a_large_cell_and_insert_a_new_cell.original.json")),
        jupyter_cells(include_str!("fixtures/notebook_diff/modification_of_a_large_cell_and_insert_a_new_cell.modified.json")),
    );

    let mut expected_mapping = identity_mapping(52);
    expected_mapping.extend([(53, -1), (54, -1)]);
    assert_eq!(mapping, expected_mapping);
    assert_eq!(changes(&diff), [(52, 1, 52, 1), (53, 0, 53, 2)]);
}

#[test]
fn misalignment_due_to_2_deleted_cells() {
    let _disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let (mapping, diff) = map_cells(
        jupyter_cells(include_str!("fixtures/notebook_diff/misalignment_due_to_2_deleted_cells.original.json")),
        jupyter_cells(include_str!("fixtures/notebook_diff/misalignment_due_to_2_deleted_cells.modified.json")),
    );

    let mut expected_mapping = identity_mapping(17);
    expected_mapping.extend([(18, 19), (19, 20)]);
    expected_mapping.extend((20..=67).map(|modified| (modified, modified + 2)));
    assert_eq!(mapping, expected_mapping);
    // O original deixa comentada a mudança `{ originalStart: 53, originalLength: 0, modifiedStart: 53, modifiedLength: 2 }`.
    assert_eq!(
        changes(&diff),
        [
            (12, 1, 12, 1),
            (15, 1, 15, 1),
            (18, 1, 18, 0),
            (20, 1, 19, 1),
            (21, 1, 20, 0),
            (22, 1, 20, 1),
            (23, 1, 21, 1),
            (26, 1, 24, 1),
            (37, 1, 35, 1),
            (44, 1, 42, 1),
            (48, 1, 46, 1),
            (56, 1, 54, 1),
            (58, 1, 56, 1),
            (60, 1, 58, 1),
            (63, 1, 61, 1),
            (68, 1, 66, 1),
        ]
    );
}

#[test]
fn insert_a_few_cells_one_on_top_modify_a_few_and_completely_misaligned() {
    let _disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let (mapping, diff) = map_cells(
        jupyter_cells(include_str!("fixtures/notebook_diff/insert_a_few_cells_one_on_top_modify_a_few_and_completely_misaligned.original.json")),
        jupyter_cells(include_str!("fixtures/notebook_diff/insert_a_few_cells_one_on_top_modify_a_few_and_completely_misaligned.modified.json")),
    );

    assert_eq!(mapping, [(0, -1), (1, 0), (2, 1), (3, 2), (4, 3), (5, 4), (6, 5), (7, 6), (8, -1), (9, 7), (10, -1)]);
    assert_eq!(changes(&diff), [(0, 0, 0, 1), (0, 1, 1, 1), (1, 1, 2, 1), (7, 0, 8, 1), (8, 0, 10, 1)]);
}
