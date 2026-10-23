//! Porte de `src/vs/editor/test/browser/view/viewLayer.test.ts` (80 casos), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` vira o guarda criado no começo de cada caso.
//! - `TestLine` é a struct de teste que implementa `Line` (o `ILine` do original).
//! - `col._set(6, [...])` é `col.set(6, vec![...])`.
//! - Os `describe` viram prefixos: `deleted_*`, `changed_*`, `inserted_*` e `tokens_*`.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_editor::view_layer::{Line, LineNumberRange, RenderedLinesCollection};

struct TestLine {
    pinged: bool,
    id: String,
}

impl TestLine {
    fn new(id: &str) -> Self {
        Self { pinged: false, id: id.to_string() }
    }
}

impl Line for TestLine {
    fn on_content_changed(&mut self) {
        self.pinged = true;
    }
    fn on_tokens_changed(&mut self) {
        self.pinged = true;
    }
}

#[derive(Debug, PartialEq)]
struct LinesCollectionState {
    start_line_number: i32,
    lines: Vec<String>,
    pinged: Vec<bool>,
}

fn state(start_line_number: i32, lines: &[&str], pinged: &[bool]) -> LinesCollectionState {
    LinesCollectionState {
        start_line_number,
        lines: lines.iter().map(|l| l.to_string()).collect(),
        pinged: pinged.to_vec(),
    }
}

fn assert_state(col: &RenderedLinesCollection<TestLine>, expected: LinesCollectionState) {
    let mut actual = LinesCollectionState { start_line_number: col.get_start_line_number(), lines: vec![], pinged: vec![] };
    for line_number in col.get_start_line_number()..=col.get_end_line_number() {
        actual.lines.push(col.get_line(line_number).id.clone());
        actual.pinged.push(col.get_line(line_number).pinged);
    }
    assert_eq!(actual, expected);
}

fn new_collection() -> RenderedLinesCollection<TestLine> {
    let mut col = RenderedLinesCollection::<TestLine>::new(|| TestLine::new("new"));
    col.set(6, vec![TestLine::new("old6"), TestLine::new("old7"), TestLine::new("old8"), TestLine::new("old9")]);
    col
}

const OLD: [&str; 4] = ["old6", "old7", "old8", "old9"];
const NONE: [bool; 4] = [false; 4];

fn ids(lines: Option<Vec<TestLine>>) -> Vec<String> {
    lines.map(|l| l.into_iter().map(|line| line.id).collect()).unwrap_or_default()
}

fn test_on_model_lines_deleted(from: i32, to: i32, expected_deleted: &[&str], expected_state: LinesCollectionState) {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let mut col = new_collection();
    let actual_deleted = ids(col.on_lines_deleted(from, to));
    assert_eq!(actual_deleted, expected_deleted);
    assert_state(&col, expected_state);
}

fn test_on_model_line_changed(changed_line_number: i32, expected_pinged: bool, expected_state: LinesCollectionState) {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let mut col = new_collection();
    let actual_pinged = col.on_lines_changed(changed_line_number, 1);
    assert_eq!(actual_pinged, expected_pinged);
    assert_state(&col, expected_state);
}

fn test_on_model_lines_inserted(from: i32, to: i32, expected_deleted: &[&str], expected_state: LinesCollectionState) {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let mut col = new_collection();
    let actual_deleted = ids(col.on_lines_inserted(from, to));
    assert_eq!(actual_deleted, expected_deleted);
    assert_state(&col, expected_state);
}

fn test_on_model_tokens_changed(from: i32, to: i32, expected_pinged: bool, expected_state: LinesCollectionState) {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let mut col = new_collection();
    let actual_pinged = col.on_tokens_changed(&[LineNumberRange { from_line_number: from, to_line_number: to }]);
    assert_eq!(actual_pinged, expected_pinged);
    assert_state(&col, expected_state);
}

// RenderedLinesCollection onLinesDeleted

#[test]
fn deleted_a1() {
    test_on_model_lines_deleted(3, 3, &[], state(5, &OLD, &NONE));
}

#[test]
fn deleted_a2() {
    test_on_model_lines_deleted(3, 4, &[], state(4, &OLD, &NONE));
}

#[test]
fn deleted_a3() {
    test_on_model_lines_deleted(3, 5, &[], state(3, &OLD, &NONE));
}

#[test]
fn deleted_a4() {
    test_on_model_lines_deleted(3, 6, &["old6"], state(3, &["old7", "old8", "old9"], &[false; 3]));
}

#[test]
fn deleted_a5() {
    test_on_model_lines_deleted(3, 7, &["old6", "old7"], state(3, &["old8", "old9"], &[false; 2]));
}

#[test]
fn deleted_a6() {
    test_on_model_lines_deleted(3, 8, &["old6", "old7", "old8"], state(3, &["old9"], &[false]));
}

#[test]
fn deleted_a7() {
    test_on_model_lines_deleted(3, 9, &OLD, state(3, &[], &[]));
}

#[test]
fn deleted_a8() {
    test_on_model_lines_deleted(3, 10, &OLD, state(3, &[], &[]));
}

#[test]
fn deleted_b1() {
    test_on_model_lines_deleted(5, 5, &[], state(5, &OLD, &NONE));
}

#[test]
fn deleted_b2() {
    test_on_model_lines_deleted(5, 6, &["old6"], state(5, &["old7", "old8", "old9"], &[false; 3]));
}

#[test]
fn deleted_b3() {
    test_on_model_lines_deleted(5, 7, &["old6", "old7"], state(5, &["old8", "old9"], &[false; 2]));
}

#[test]
fn deleted_b4() {
    test_on_model_lines_deleted(5, 8, &["old6", "old7", "old8"], state(5, &["old9"], &[false]));
}

#[test]
fn deleted_b5() {
    test_on_model_lines_deleted(5, 9, &OLD, state(5, &[], &[]));
}

#[test]
fn deleted_b6() {
    test_on_model_lines_deleted(5, 10, &OLD, state(5, &[], &[]));
}

#[test]
fn deleted_c1() {
    test_on_model_lines_deleted(6, 6, &["old6"], state(6, &["old7", "old8", "old9"], &[false; 3]));
}

#[test]
fn deleted_c2() {
    test_on_model_lines_deleted(6, 7, &["old6", "old7"], state(6, &["old8", "old9"], &[false; 2]));
}

#[test]
fn deleted_c3() {
    test_on_model_lines_deleted(6, 8, &["old6", "old7", "old8"], state(6, &["old9"], &[false]));
}

#[test]
fn deleted_c4() {
    test_on_model_lines_deleted(6, 9, &OLD, state(6, &[], &[]));
}

#[test]
fn deleted_c5() {
    test_on_model_lines_deleted(6, 10, &OLD, state(6, &[], &[]));
}

#[test]
fn deleted_d1() {
    test_on_model_lines_deleted(7, 7, &["old7"], state(6, &["old6", "old8", "old9"], &[false; 3]));
}

#[test]
fn deleted_d2() {
    test_on_model_lines_deleted(7, 8, &["old7", "old8"], state(6, &["old6", "old9"], &[false; 2]));
}

#[test]
fn deleted_d3() {
    test_on_model_lines_deleted(7, 9, &["old7", "old8", "old9"], state(6, &["old6"], &[false]));
}

#[test]
fn deleted_d4() {
    test_on_model_lines_deleted(7, 10, &["old7", "old8", "old9"], state(6, &["old6"], &[false]));
}

#[test]
fn deleted_e1() {
    test_on_model_lines_deleted(8, 8, &["old8"], state(6, &["old6", "old7", "old9"], &[false; 3]));
}

#[test]
fn deleted_e2() {
    test_on_model_lines_deleted(8, 9, &["old8", "old9"], state(6, &["old6", "old7"], &[false; 2]));
}

#[test]
fn deleted_e3() {
    test_on_model_lines_deleted(8, 10, &["old8", "old9"], state(6, &["old6", "old7"], &[false; 2]));
}

#[test]
fn deleted_f1() {
    test_on_model_lines_deleted(9, 9, &["old9"], state(6, &["old6", "old7", "old8"], &[false; 3]));
}

#[test]
fn deleted_f2() {
    test_on_model_lines_deleted(9, 10, &["old9"], state(6, &["old6", "old7", "old8"], &[false; 3]));
}

#[test]
fn deleted_g1() {
    test_on_model_lines_deleted(10, 10, &[], state(6, &OLD, &NONE));
}

#[test]
fn deleted_g2() {
    test_on_model_lines_deleted(10, 11, &[], state(6, &OLD, &NONE));
}

#[test]
fn deleted_h1() {
    test_on_model_lines_deleted(11, 13, &[], state(6, &OLD, &NONE));
}

// RenderedLinesCollection onLineChanged

#[test]
fn changed_3() {
    test_on_model_line_changed(3, false, state(6, &OLD, &NONE));
}

#[test]
fn changed_4() {
    test_on_model_line_changed(4, false, state(6, &OLD, &NONE));
}

#[test]
fn changed_5() {
    test_on_model_line_changed(5, false, state(6, &OLD, &NONE));
}

#[test]
fn changed_6() {
    test_on_model_line_changed(6, true, state(6, &OLD, &[true, false, false, false]));
}

#[test]
fn changed_7() {
    test_on_model_line_changed(7, true, state(6, &OLD, &[false, true, false, false]));
}

#[test]
fn changed_8() {
    test_on_model_line_changed(8, true, state(6, &OLD, &[false, false, true, false]));
}

#[test]
fn changed_9() {
    test_on_model_line_changed(9, true, state(6, &OLD, &[false, false, false, true]));
}

#[test]
fn changed_10() {
    test_on_model_line_changed(10, false, state(6, &OLD, &NONE));
}

#[test]
fn changed_11() {
    test_on_model_line_changed(11, false, state(6, &OLD, &NONE));
}

// RenderedLinesCollection onLinesInserted

#[test]
fn inserted_a1() {
    test_on_model_lines_inserted(3, 3, &[], state(7, &OLD, &NONE));
}

#[test]
fn inserted_a2() {
    test_on_model_lines_inserted(3, 4, &[], state(8, &OLD, &NONE));
}

#[test]
fn inserted_a3() {
    test_on_model_lines_inserted(3, 5, &[], state(9, &OLD, &NONE));
}

#[test]
fn inserted_a4() {
    test_on_model_lines_inserted(3, 6, &[], state(10, &OLD, &NONE));
}

#[test]
fn inserted_a5() {
    test_on_model_lines_inserted(3, 7, &[], state(11, &OLD, &NONE));
}

#[test]
fn inserted_a6() {
    test_on_model_lines_inserted(3, 8, &[], state(12, &OLD, &NONE));
}

#[test]
fn inserted_a7() {
    test_on_model_lines_inserted(3, 9, &[], state(13, &OLD, &NONE));
}

#[test]
fn inserted_a8() {
    test_on_model_lines_inserted(3, 10, &[], state(14, &OLD, &NONE));
}

#[test]
fn inserted_b1() {
    test_on_model_lines_inserted(5, 5, &[], state(7, &OLD, &NONE));
}

#[test]
fn inserted_b2() {
    test_on_model_lines_inserted(5, 6, &[], state(8, &OLD, &NONE));
}

#[test]
fn inserted_b3() {
    test_on_model_lines_inserted(5, 7, &[], state(9, &OLD, &NONE));
}

#[test]
fn inserted_b4() {
    test_on_model_lines_inserted(5, 8, &[], state(10, &OLD, &NONE));
}

#[test]
fn inserted_b5() {
    test_on_model_lines_inserted(5, 9, &[], state(11, &OLD, &NONE));
}

#[test]
fn inserted_b6() {
    test_on_model_lines_inserted(5, 10, &[], state(12, &OLD, &NONE));
}

#[test]
fn inserted_c1() {
    test_on_model_lines_inserted(6, 6, &[], state(7, &OLD, &NONE));
}

#[test]
fn inserted_c2() {
    test_on_model_lines_inserted(6, 7, &[], state(8, &OLD, &NONE));
}

#[test]
fn inserted_c3() {
    test_on_model_lines_inserted(6, 8, &[], state(9, &OLD, &NONE));
}

#[test]
fn inserted_c4() {
    test_on_model_lines_inserted(6, 9, &[], state(10, &OLD, &NONE));
}

#[test]
fn inserted_c5() {
    test_on_model_lines_inserted(6, 10, &[], state(11, &OLD, &NONE));
}

#[test]
fn inserted_d1() {
    test_on_model_lines_inserted(7, 7, &["old9"], state(6, &["old6", "new", "old7", "old8"], &NONE));
}

#[test]
fn inserted_d2() {
    test_on_model_lines_inserted(7, 8, &["old8", "old9"], state(6, &["old6", "new", "new", "old7"], &NONE));
}

#[test]
fn inserted_d3() {
    test_on_model_lines_inserted(7, 9, &["old7", "old8", "old9"], state(6, &["old6"], &[false]));
}

#[test]
fn inserted_d4() {
    test_on_model_lines_inserted(7, 10, &["old7", "old8", "old9"], state(6, &["old6"], &[false]));
}

#[test]
fn inserted_e1() {
    test_on_model_lines_inserted(8, 8, &["old9"], state(6, &["old6", "old7", "new", "old8"], &NONE));
}

#[test]
fn inserted_e2() {
    test_on_model_lines_inserted(8, 9, &["old8", "old9"], state(6, &["old6", "old7"], &[false; 2]));
}

#[test]
fn inserted_e3() {
    test_on_model_lines_inserted(8, 10, &["old8", "old9"], state(6, &["old6", "old7"], &[false; 2]));
}

#[test]
fn inserted_f1() {
    test_on_model_lines_inserted(9, 9, &["old9"], state(6, &["old6", "old7", "old8"], &[false; 3]));
}

#[test]
fn inserted_f2() {
    test_on_model_lines_inserted(9, 10, &["old9"], state(6, &["old6", "old7", "old8"], &[false; 3]));
}

#[test]
fn inserted_g1() {
    test_on_model_lines_inserted(10, 10, &[], state(6, &OLD, &NONE));
}

#[test]
fn inserted_g2() {
    test_on_model_lines_inserted(10, 11, &[], state(6, &OLD, &NONE));
}

#[test]
fn inserted_h1() {
    test_on_model_lines_inserted(11, 13, &[], state(6, &OLD, &NONE));
}

// RenderedLinesCollection onTokensChanged

#[test]
fn tokens_a() {
    test_on_model_tokens_changed(3, 3, false, state(6, &OLD, &NONE));
}

#[test]
fn tokens_b() {
    test_on_model_tokens_changed(3, 5, false, state(6, &OLD, &NONE));
}

#[test]
fn tokens_c() {
    test_on_model_tokens_changed(3, 6, true, state(6, &OLD, &[true, false, false, false]));
}

#[test]
fn tokens_d() {
    test_on_model_tokens_changed(6, 6, true, state(6, &OLD, &[true, false, false, false]));
}

#[test]
fn tokens_e() {
    test_on_model_tokens_changed(5, 10, true, state(6, &OLD, &[true; 4]));
}

#[test]
fn tokens_f() {
    test_on_model_tokens_changed(8, 9, true, state(6, &OLD, &[false, false, true, true]));
}

#[test]
fn tokens_g() {
    test_on_model_tokens_changed(8, 11, true, state(6, &OLD, &[false, false, true, true]));
}

#[test]
fn tokens_h() {
    test_on_model_tokens_changed(10, 10, false, state(6, &OLD, &NONE));
}

#[test]
fn tokens_i() {
    test_on_model_tokens_changed(10, 11, false, state(6, &OLD, &NONE));
}
