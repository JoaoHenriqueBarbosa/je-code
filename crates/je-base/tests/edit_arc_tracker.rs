//! Porte de `src/vs/workbench/contrib/editTelemetry/test/node/arcTracker.test.ts` (suíte `ArcTracker`), caso a
//! caso e na mesma ordem, para o `EditArcTracker` de `vs/base/common` (o único que o `je-base` tem).
//!
//! Notas de tradução:
//! - O original roda dois rastreadores, o `ArcTracker` do editor (`vs/workbench`) e o `EditArcTracker` de
//!   `vs/base`, e exige que deem os mesmos valores. O `ArcTracker` não existe no `je-base`; aqui roda só o
//!   `EditArcTracker` e confere os valores esperados do original (que são os do `ArcTracker`), mais as
//!   verificações de que a contagem de linhas e o ARC nunca aumentam.
//! - Os arquivos `*.edits.w.json` são os do VS Code (`vs/workbench/contrib/editTelemetry/test/node/data`),
//!   copiados para `golden/arc_tracker`.

mod common;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::edit_arc_tracker::{EditArcTracker, IArcTextEdit, IArcTextReplacement, IEditArcValues};
use je_base::json::{self, Value};
use je_base::js_string::JsString;

fn get(value: &Value, key: &str) -> Value {
    match value {
        Value::Object(o) => o.get(key).cloned().unwrap_or_else(|| panic!("falta a chave {key}")),
        other => panic!("esperava um objeto, veio {other:?}"),
    }
}

fn number(value: &Value) -> usize {
    match value {
        Value::Number(n) => *n as usize,
        other => panic!("esperava um número, veio {other:?}"),
    }
}

fn string(value: &Value) -> JsString {
    match value {
        Value::String(s) => s.clone(),
        other => panic!("esperava uma string, veio {other:?}"),
    }
}

fn array(value: &Value) -> Vec<Value> {
    match value {
        Value::Array(items) => items.clone(),
        other => panic!("esperava um array, veio {other:?}"),
    }
}

/// `createArcTextEditFromJson`.
fn arc_text_edit(edit_data: &Value) -> IArcTextEdit {
    IArcTextEdit {
        replacements: array(&get(edit_data, "replacements"))
            .iter()
            .map(|r| IArcTextReplacement { start: number(&get(r, "start")), end_exclusive: number(&get(r, "endEx")), text: string(&get(r, "text")) })
            .collect(),
    }
}

/// `runTestWithData`.
fn run_test_with_data(source: &str) -> Vec<IEditArcValues> {
    let data = json::parse_str(source, &mut Vec::new()).expect("JSON válido");
    let initial_text = string(&get(&data, "initialText"));
    let core_edits: Vec<IArcTextEdit> = array(&get(&data, "edits")).iter().map(arc_text_edit).collect();

    let mut core_tracker = EditArcTracker::new(&initial_text, &core_edits[0]);

    let mut stats = vec![core_tracker.get_values()];
    let mut last_line_numbers = core_tracker.get_line_count_info().inserted_line_counts;
    let mut last_arc = core_tracker.get_accepted_restrained_characters_count();

    for edit in &core_edits[1..] {
        core_tracker.handle_edits(edit);
        stats.push(core_tracker.get_values());

        let new_line_numbers = core_tracker.get_line_count_info().inserted_line_counts;
        assert!(new_line_numbers <= last_line_numbers, "Line numbers must not increase. Last: {last_line_numbers}, new: {new_line_numbers}");
        last_line_numbers = new_line_numbers;

        let new_arc = core_tracker.get_accepted_restrained_characters_count();
        assert!(new_arc <= last_arc, "ARC must not increase. Last: {last_arc}, new: {new_arc}");
        last_arc = new_arc;
    }
    stats
}

fn values(arc: usize, deleted_line_counts: usize, inserted_line_counts: usize) -> IEditArcValues {
    IEditArcValues { arc, deleted_line_counts, inserted_line_counts }
}

#[test]
fn issue_264048() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    let stats = run_test_with_data(include_str!("golden/arc_tracker/issue-264048.edits.w.json"));
    assert_eq!(stats, vec![values(8, 1, 1), values(8, 0, 1), values(8, 0, 1)]);
}

#[test]
fn line_insert() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    let stats = run_test_with_data(include_str!("golden/arc_tracker/line-insert.edits.w.json"));
    assert_eq!(stats, vec![values(7, 0, 1), values(5, 0, 1)]);
}

#[test]
fn line_modification() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    let stats = run_test_with_data(include_str!("golden/arc_tracker/line-modification.edits.w.json"));
    assert_eq!(stats, vec![values(6, 1, 1), values(6, 1, 1), values(0, 0, 0)]);
}

#[test]
fn multiline_insert() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    let stats = run_test_with_data(include_str!("golden/arc_tracker/multiline-insert.edits.w.json"));
    assert_eq!(stats, vec![values(24, 0, 3), values(23, 0, 2)]);
}
