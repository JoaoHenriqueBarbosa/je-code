//! Porte de `src/vs/editor/test/common/model/bracketPairColorizer/combineTextEditInfos.test.ts` (suíte `combineTextEditInfos`), caso a caso e na mesma ordem.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod support;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::JsString;
use je_editor::bracket_pairs_text_model_part::bracket_pairs_tree::before_edit_position_mapper::TextEditInfo;
use je_editor::bracket_pairs_text_model_part::bracket_pairs_tree::combine_text_edit_infos::combine_text_edit_infos;
use je_editor::bracket_pairs_text_model_part::bracket_pairs_tree::length::{
    length_add, length_to_obj, length_to_position, position_to_length, to_length,
};
use je_editor::edit_operation::SingleEditOperation;
use je_editor::range::Range;
use je_editor::text_model::TextModel;
use support::random::Random;
use support::test_text_model::create_text_model;

fn run_test(seed: u32) {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    let mut rng = Random::create(seed);

    let str = "abcde\nfghij\nklmno\npqrst\n";
    let text_model_s0 = create_text_model(str);

    let count = rng.next_int_range(1, 4);
    let edits1 = get_random_edit_infos(&text_model_s0, count, &mut rng, false);
    let text_model_s1 = create_text_model(&text_model_s0.get_value().to_string_lossy());
    text_model_s1.apply_edits(&edits1.iter().map(to_edit).collect::<Vec<_>>());

    let count = rng.next_int_range(1, 4);
    let edits2 = get_random_edit_infos(&text_model_s1, count, &mut rng, false);
    let text_model_s2 = create_text_model(&text_model_s1.get_value().to_string_lossy());
    text_model_s2.apply_edits(&edits2.iter().map(to_edit).collect::<Vec<_>>());

    let combined_edits = combine_text_edit_infos(&edits1, &edits2);
    for edit in &combined_edits {
        let range = Range::from_positions(length_to_position(edit.start_offset), length_to_position(length_add(edit.start_offset, edit.new_length)));
        let value = text_model_s2.get_value_in_range(&range);
        // `/^(L|C|\n)*$/`
        if !value.units().iter().all(|&u| u == u16::from(b'L') || u == u16::from(b'C') || u == 10) {
            panic!("Invalid edit: {value}");
        }
        text_model_s2.apply_edits(&[SingleEditOperation::new(
            range,
            Some(text_model_s0.get_value_in_range(&Range::from_positions(
                length_to_position(edit.start_offset),
                length_to_position(edit.end_offset),
            ))),
            false,
        )]);
    }

    assert_eq!(text_model_s2.get_value(), text_model_s0.get_value());

    text_model_s0.dispose();
    text_model_s1.dispose();
    text_model_s2.dispose();
}

fn get_random_edit_infos(text_model: &TextModel, count: i64, rng: &mut Random, disjoint: bool) -> Vec<TextEditInfo> {
    let mut edits: Vec<TextEditInfo> = Vec::new();
    let mut i = 0;
    for j in 0..count as usize {
        edits.push(get_random_edit(text_model, i, rng));
        i = text_model.get_offset_at(length_to_position(edits[j].end_offset)) as i64 + i64::from(disjoint);
    }
    edits
}

fn get_random_edit(text_model: &TextModel, range_offset_start: i64, rng: &mut Random) -> TextEditInfo {
    let text_model_length = text_model.get_value_length() as i64;
    let offset_start = rng.next_int_range(range_offset_start, text_model_length);
    let offset_end = rng.next_int_range(offset_start, text_model_length);

    let line_count = rng.next_int_range(0, 3);
    let column_count = rng.next_int_range(0, 5);

    TextEditInfo::new(
        position_to_length(text_model.get_position_at(offset_start)),
        position_to_length(text_model.get_position_at(offset_end)),
        to_length(line_count, column_count),
    )
}

fn to_edit(edit_info: &TextEditInfo) -> SingleEditOperation {
    let l = length_to_obj(edit_info.new_length);
    let mut text = String::new();

    for _ in 0..l.line_count {
        text += "LLL\n";
    }
    for _ in 0..l.column_count {
        text += "C";
    }

    SingleEditOperation::new(
        Range::from_positions(length_to_position(edit_info.start_offset), length_to_position(edit_info.end_offset)),
        Some(JsString::from(text)),
        false,
    )
}

#[test]
fn test0() {
    run_test(0);
}

#[test]
fn test1() {
    run_test(1);
}

#[test]
fn test2() {
    run_test(2);
}

#[test]
fn test3() {
    run_test(3);
}

#[test]
fn test4() {
    run_test(4);
}

#[test]
fn test5() {
    run_test(5);
}

#[test]
fn test6() {
    run_test(6);
}

#[test]
fn test7() {
    run_test(7);
}

#[test]
fn test8() {
    run_test(8);
}

#[test]
fn test9() {
    run_test(9);
}

#[test]
fn test10() {
    run_test(10);
}

#[test]
fn test11() {
    run_test(11);
}

#[test]
fn test12() {
    run_test(12);
}

#[test]
fn test13() {
    run_test(13);
}

#[test]
fn test14() {
    run_test(14);
}

#[test]
fn test15() {
    run_test(15);
}

#[test]
fn test16() {
    run_test(16);
}

#[test]
fn test17() {
    run_test(17);
}

#[test]
fn test18() {
    run_test(18);
}

#[test]
fn test19() {
    run_test(19);
}

#[test]
fn test20() {
    run_test(20);
}

#[test]
fn test21() {
    run_test(21);
}

#[test]
fn test22() {
    run_test(22);
}

#[test]
fn test23() {
    run_test(23);
}

#[test]
fn test24() {
    run_test(24);
}

#[test]
fn test25() {
    run_test(25);
}

#[test]
fn test26() {
    run_test(26);
}

#[test]
fn test27() {
    run_test(27);
}

#[test]
fn test28() {
    run_test(28);
}

#[test]
fn test29() {
    run_test(29);
}

#[test]
fn test30() {
    run_test(30);
}

#[test]
fn test31() {
    run_test(31);
}

#[test]
fn test32() {
    run_test(32);
}

#[test]
fn test33() {
    run_test(33);
}

#[test]
fn test34() {
    run_test(34);
}

#[test]
fn test35() {
    run_test(35);
}

#[test]
fn test36() {
    run_test(36);
}

#[test]
fn test37() {
    run_test(37);
}

#[test]
fn test38() {
    run_test(38);
}

#[test]
fn test39() {
    run_test(39);
}

#[test]
fn test40() {
    run_test(40);
}

#[test]
fn test41() {
    run_test(41);
}

#[test]
fn test42() {
    run_test(42);
}

#[test]
fn test43() {
    run_test(43);
}

#[test]
fn test44() {
    run_test(44);
}

#[test]
fn test45() {
    run_test(45);
}

#[test]
fn test46() {
    run_test(46);
}

#[test]
fn test47() {
    run_test(47);
}

#[test]
fn test48() {
    run_test(48);
}

#[test]
fn test49() {
    run_test(49);
}
