//! Porte de `src/vs/workbench/contrib/mergeEditor/test/browser/mapping.test.ts` (suíte `merge editor mapping`,
//! sub-suíte `DocumentRangeMap`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` vira o `LeakGuard` criado no começo de cada caso.
//! - O original lê a posição a projetar do título do caso (`this.test.title`); aqui cada caso passa o
//!   mesmo texto a [`project`].
//! - Os itens `string | [string, string]` de `createDocumentRangeMap` viram [`Item`].

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_editor::position::Position;
use je_editor::position_to_offset::TextLength;
use je_editor::range::Range;
use je_workbench::contrib::merge_editor::model::mapping::{DocumentRangeMap, RangeMapping};

enum Item {
    Same(&'static str),
    Changed(&'static str, &'static str),
}

fn parse_pos(s: &str) -> Position {
    let (line_count, column_count) = s.split_once(':').unwrap();
    Position::new(line_count.parse().unwrap(), column_count.parse().unwrap())
}

fn parse_length_obj(s: &str) -> TextLength {
    let (line_count, column_count) = s.split_once(':').unwrap();
    TextLength::new(line_count.parse().unwrap(), column_count.parse().unwrap())
}

fn to_position(length: TextLength) -> Position {
    Position::new(length.line_count + 1, length.column_count + 1)
}

fn create_document_range_map(items: &[Item]) -> DocumentRangeMap {
    let mut mappings: Vec<RangeMapping> = Vec::new();
    let mut last_len1 = TextLength::new(0, 0);
    let mut last_len2 = TextLength::new(0, 0);
    for item in items {
        match item {
            Item::Same(s) => {
                let len = parse_length_obj(s);
                last_len1 = last_len1.add(len);
                last_len2 = last_len2.add(len);
            }
            Item::Changed(a, b) => {
                let len1 = parse_length_obj(a);
                let len2 = parse_length_obj(b);
                mappings.push(RangeMapping::new(
                    Range::from_positions(to_position(last_len1), to_position(last_len1.add(len1))),
                    Range::from_positions(to_position(last_len2), to_position(last_len2.add(len2))),
                ));
                last_len1 = last_len1.add(len1);
                last_len2 = last_len2.add(len2);
            }
        }
    }

    DocumentRangeMap::new(mappings, last_len1.line_count)
}

fn document_map() -> DocumentRangeMap {
    create_document_range_map(&[
        Item::Same("1:3"),
        Item::Changed("0:2", "0:3"),
        Item::Same("1:1"),
        Item::Changed("1:2", "3:3"),
        Item::Same("0:2"),
        Item::Changed("0:2", "0:3"),
    ])
}

fn project(title: &str) -> String {
    document_map().project(parse_pos(title)).to_string()
}

#[test]
fn map() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let strings: Vec<String> = document_map().range_mappings.iter().map(ToString::to_string).collect();
    assert_eq!(strings, vec!["[2:4, 2:6) -> [2:4, 2:7)", "[3:2, 4:3) -> [3:2, 6:4)", "[4:5, 4:7) -> [6:6, 6:9)"]);
}

#[test]
fn project_1_1() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(project("1:1"), "[1:1, 1:1) -> [1:1, 1:1)");
}

#[test]
fn project_2_3() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(project("2:3"), "[2:3, 2:3) -> [2:3, 2:3)");
}

#[test]
fn project_2_4() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(project("2:4"), "[2:4, 2:6) -> [2:4, 2:7)");
}

#[test]
fn project_2_5() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(project("2:5"), "[2:4, 2:6) -> [2:4, 2:7)");
}

#[test]
fn project_2_6() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(project("2:6"), "[2:6, 2:6) -> [2:7, 2:7)");
}

#[test]
fn project_2_7() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(project("2:7"), "[2:7, 2:7) -> [2:8, 2:8)");
}

#[test]
fn project_3_1() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(project("3:1"), "[3:1, 3:1) -> [3:1, 3:1)");
}

#[test]
fn project_3_2() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(project("3:2"), "[3:2, 4:3) -> [3:2, 6:4)");
}

#[test]
fn project_4_2() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(project("4:2"), "[3:2, 4:3) -> [3:2, 6:4)");
}

#[test]
fn project_4_3() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(project("4:3"), "[4:3, 4:3) -> [6:4, 6:4)");
}

#[test]
fn project_4_4() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(project("4:4"), "[4:4, 4:4) -> [6:5, 6:5)");
}

#[test]
fn project_4_5() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(project("4:5"), "[4:5, 4:7) -> [6:6, 6:9)");
}
