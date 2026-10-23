//! Porte de `src/vs/editor/test/common/model/textChange.test.ts` (suíte `TextChangeCompressor`), caso a caso e na mesma ordem.
//!
//! A segunda suíte do arquivo (`TextChange`, um caso) vem depois dos nove da primeira. O gerador de testes
//! aleatórios (`GENERATE_TESTS = false` no original, nunca roda) não é portado.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::JsString;
use je_editor::text_change::{TextChange, compress_consecutive_text_changes};

struct GeneratedEdit {
    offset: usize,
    length: usize,
    text: &'static str,
}

const fn edit(offset: usize, length: usize, text: &'static str) -> GeneratedEdit {
    GeneratedEdit { offset, length, text }
}

fn js(text: &str) -> JsString {
    JsString::from(text)
}

fn substring(content: &JsString, start: usize, end: usize) -> JsString {
    let (start, end) = (start.min(content.len()), end.min(content.len()));
    content.slice(start.min(end), end.max(start))
}

fn get_resulting_content(initial_content: &str, edits: &[GeneratedEdit]) -> JsString {
    let mut content = js(initial_content);
    for edit in edits.iter().rev() {
        let mut next = substring(&content, 0, edit.offset);
        next.push_units(js(edit.text).units());
        next.push_units(substring(&content, edit.offset + edit.length, content.len()).units());
        content = next;
    }
    content
}

fn get_text_changes(initial_content: &JsString, edits: &[GeneratedEdit]) -> Vec<TextChange> {
    let mut content = initial_content.clone();
    let mut changes = Vec::with_capacity(edits.len());
    let mut delta_offset: isize = 0;

    for edit in edits {
        let position = (edit.offset as isize + delta_offset) as usize;
        let length = edit.length;
        let text = js(edit.text);

        let old_text = substring(&content, position, position + length);

        let mut next = substring(&content, 0, position);
        next.push_units(text.units());
        next.push_units(substring(&content, position + length, content.len()).units());
        content = next;

        changes.push(TextChange::new(edit.offset, old_text, position, text.clone()));

        delta_offset += text.len() as isize - length as isize;
    }

    changes
}

fn assert_compression(initial_text: &str, edit1: &[GeneratedEdit], edit2: &[GeneratedEdit]) {
    let tmp_text = get_resulting_content(initial_text, edit1);
    let chg1 = get_text_changes(&js(initial_text), edit1);

    let tmp_text_str = tmp_text.to_string_lossy();
    let final_text = get_resulting_content(&tmp_text_str, edit2);
    let chg2 = get_text_changes(&tmp_text, edit2);

    let compressed_text_changes = compress_consecutive_text_changes(Some(chg1), chg2);

    // Confere se a compressão está correta
    let mut actual_do_result = js(initial_text);
    for change in compressed_text_changes.iter().rev() {
        let mut next = substring(&actual_do_result, 0, change.old_position);
        next.push_units(change.new_text.units());
        next.push_units(substring(&actual_do_result, change.old_position + change.old_length(), actual_do_result.len()).units());
        actual_do_result = next;
    }
    assert_eq!(actual_do_result, final_text);

    let mut actual_undo_result = final_text.clone();
    for change in compressed_text_changes.iter().rev() {
        let mut next = substring(&actual_undo_result, 0, change.new_position);
        next.push_units(change.old_text.units());
        next.push_units(
            substring(
                &actual_undo_result,
                change.new_position + change.new_length(),
                actual_undo_result.len(),
            )
            .units(),
        );
        actual_undo_result = next;
    }
    assert_eq!(actual_undo_result, js(initial_text));
}

#[test]
fn simple_1() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    assert_compression("", &[edit(0, 0, "h")], &[edit(1, 0, "e")]);
}

#[test]
fn simple_2() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    assert_compression("|", &[edit(0, 0, "h")], &[edit(2, 0, "e")]);
}

#[test]
fn complex1() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    assert_compression(
        "abcdefghij",
        &[edit(0, 3, "qh"), edit(5, 0, "1"), edit(8, 2, "X")],
        &[edit(1, 0, "Z"), edit(3, 3, "Y")],
    );
}

#[test]
fn gen1() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    assert_compression("kxm", &[edit(0, 1, "tod_neu")], &[edit(1, 2, "sag_e")]);
}

#[test]
fn gen2() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    assert_compression("kpb_r_v", &[edit(5, 2, "a_jvf_l")], &[edit(10, 2, "w")]);
}

#[test]
fn gen3() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    assert_compression("slu_w", &[edit(4, 1, "_wfw")], &[edit(3, 5, "")]);
}

#[test]
fn gen4() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    assert_compression("_e", &[edit(2, 0, "zo_b")], &[edit(1, 3, "tra")]);
}

#[test]
fn gen5() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    assert_compression("ssn_", &[edit(0, 2, "tat_nwe")], &[edit(2, 6, "jm")]);
}

#[test]
fn gen6() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    assert_compression("kl_nru", &[edit(4, 1, "")], &[edit(1, 4, "__ut")]);
}

#[test]
fn issue_118041_unicode_character_undo_bug() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let text_change = TextChange::new(428, JsString::from_units(vec![0xFEFF]), 428, JsString::new());
    let mut buff = vec![0u8; text_change.write_size()];
    text_change.write(&mut buff, 0);
    let mut actual: Vec<TextChange> = Vec::new();
    TextChange::read(&buff, 0, &mut actual);
    assert_eq!(actual[0], text_change);
}
