//! Porte de `src/vs/editor/test/common/model/bracketPairColorizer/beforeEditPositionMapper.test.ts` (suíte `Bracket Pair Colorizer - BeforeEditPositionMapper`), caso a caso e na mesma ordem.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::strings::split_lines;
use je_editor::bracket_pairs_text_model_part::bracket_pairs_tree::before_edit_position_mapper::{BeforeEditPositionMapper, TextEditInfo};
use je_editor::bracket_pairs_text_model_part::bracket_pairs_tree::length::{length_of_string, length_to_obj, to_length};

/// `TextEdit extends TextEditInfo`.
struct TextEdit {
    info: TextEditInfo,
    new_text: &'static str,
}

fn text_edit(start: (i64, i64), end: (i64, i64), new_text: &'static str) -> TextEdit {
    let units: Vec<u16> = new_text.encode_utf16().collect();
    TextEdit {
        info: TextEditInfo::new(to_length(start.0, start.1), to_length(end.0, end.1), length_of_string(&units)),
        new_text,
    }
}

fn right_pad(s: &str, len: usize) -> String {
    let mut s = s.to_owned();
    while s.encode_utf16().count() < len {
        s.push(' ');
    }
    s
}

/// `applyLineColumnEdits`: `(linha, coluna)` em índices a partir de 0 (como os `Length`).
fn apply_line_column_edits(text: &[u16], edits: &[TextEdit]) -> Vec<u16> {
    let mut line_start_offset_by_line_idx: Vec<usize> = vec![0];
    for (i, &unit) in text.iter().enumerate() {
        if unit == u16::from(b'\n') {
            line_start_offset_by_line_idx.push(i + 1);
        }
    }
    let get_offset = |length: je_editor::bracket_pairs_text_model_part::bracket_pairs_tree::length::Length| {
        let obj = length_to_obj(length);
        line_start_offset_by_line_idx[obj.line_count as usize] + obj.column_count as usize
    };

    let mut offset_edits: Vec<(usize, usize, Vec<u16>)> =
        edits.iter().map(|e| (get_offset(e.info.start_offset), get_offset(e.info.end_offset), e.new_text.encode_utf16().collect())).collect();
    offset_edits.sort_by_key(|edit| std::cmp::Reverse(edit.0));

    let mut text = text.to_vec();
    for (start, end, new_text) in offset_edits {
        text.splice(start..end, new_text);
    }
    text
}

/// `@pure`
fn compute(input_arr: &[&str], edits: Vec<TextEdit>) -> Vec<String> {
    let input: Vec<u16> = input_arr.join("\n").encode_utf16().collect();
    let new_text = apply_line_column_edits(&input, &edits);
    let new_lines = split_lines(&new_text);

    let infos: Vec<TextEditInfo> = edits.iter().map(|e| e.info).collect();
    let mut mapper = BeforeEditPositionMapper::new(&infos);

    let mut result: Vec<String> = Vec::new();

    for (line_idx, line) in new_lines.iter().enumerate() {
        let mut line_line = String::new();
        let mut col_line = String::new();
        let mut line_str = String::new();

        let mut col_dist = String::new();
        let mut line_dist = String::new();

        for col_idx in 0..=line.len() {
            let before = mapper.get_offset_before_change(to_length(line_idx as i64, col_idx as i64));
            let before_obj = length_to_obj(before);
            if col_idx < line.len() {
                line_str += &right_pad(&String::from_utf16_lossy(&line[col_idx..=col_idx]), 3);
            }
            line_line += &right_pad(&before_obj.line_count.to_string(), 3);
            col_line += &right_pad(&before_obj.column_count.to_string(), 3);

            match mapper.get_distance_to_next_change(to_length(line_idx as i64, col_idx as i64)) {
                None => {
                    line_dist += "∞  ";
                    col_dist += "∞  ";
                }
                Some(dist_len) => {
                    let dist = length_to_obj(dist_len);
                    line_dist += &right_pad(&dist.line_count.to_string(), 3);
                    col_dist += &right_pad(&dist.column_count.to_string(), 3);
                }
            }
        }
        result.push(line_str);

        result.push(line_line);
        result.push(col_line);

        result.push(line_dist);
        result.push(col_dist);
    }

    result
}

#[test]
fn single_line_1() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    assert_eq!(
        compute(
            &["0123456789"],
            vec![text_edit((0, 4), (0, 7), "xy")]
        ),
        vec![
            "0  1  2  3  x  y  7  8  9  ",
            "0  0  0  0  0  0  0  0  0  0  ",
            "0  1  2  3  4  5  7  8  9  10 ",
            "0  0  0  0  0  0  ∞  ∞  ∞  ∞  ",
            "4  3  2  1  0  0  ∞  ∞  ∞  ∞  ",
        ]
    );
}

#[test]
fn single_line_2() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    assert_eq!(
        compute(
            &["0123456789"],
            vec![text_edit((0, 2), (0, 4), "xxxx"), text_edit((0, 6), (0, 6), "yy")]
        ),
        vec![
            "0  1  x  x  x  x  4  5  y  y  6  7  8  9  ",
            "0  0  0  0  0  0  0  0  0  0  0  0  0  0  0  ",
            "0  1  2  3  4  5  4  5  6  7  6  7  8  9  10 ",
            "0  0  0  0  0  0  0  0  0  0  ∞  ∞  ∞  ∞  ∞  ",
            "2  1  0  0  0  0  2  1  0  0  ∞  ∞  ∞  ∞  ∞  ",
        ]
    );
}

#[test]
fn multi_line_replace_1() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    assert_eq!(
        compute(
            &["₀₁₂₃₄₅₆₇₈₉", "0123456789", "⁰¹²³⁴⁵⁶⁷⁸⁹"],
            vec![text_edit((0, 3), (1, 3), "xy")]
        ),
        vec![
            "₀  ₁  ₂  x  y  3  4  5  6  7  8  9  ",
            "0  0  0  0  0  1  1  1  1  1  1  1  1  ",
            "0  1  2  3  4  3  4  5  6  7  8  9  10 ",
            "0  0  0  0  0  ∞  ∞  ∞  ∞  ∞  ∞  ∞  ∞  ",
            "3  2  1  0  0  ∞  ∞  ∞  ∞  ∞  ∞  ∞  ∞  ",
            "⁰  ¹  ²  ³  ⁴  ⁵  ⁶  ⁷  ⁸  ⁹  ",
            "2  2  2  2  2  2  2  2  2  2  2  ",
            "0  1  2  3  4  5  6  7  8  9  10 ",
            "∞  ∞  ∞  ∞  ∞  ∞  ∞  ∞  ∞  ∞  ∞  ",
            "∞  ∞  ∞  ∞  ∞  ∞  ∞  ∞  ∞  ∞  ∞  ",
        ]
    );
}

#[test]
fn multi_line_replace_2() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    assert_eq!(
        compute(
            &["₀₁₂₃₄₅₆₇₈₉", "012345678", "⁰¹²³⁴⁵⁶⁷⁸⁹"],
            vec![text_edit((0, 3), (1, 0), "ab"), text_edit((1, 5), (1, 7), "c")]
        ),
        vec![
            "₀  ₁  ₂  a  b  0  1  2  3  4  c  7  8  ",
            "0  0  0  0  0  1  1  1  1  1  1  1  1  1  ",
            "0  1  2  3  4  0  1  2  3  4  5  7  8  9  ",
            "0  0  0  0  0  0  0  0  0  0  0  ∞  ∞  ∞  ",
            "3  2  1  0  0  5  4  3  2  1  0  ∞  ∞  ∞  ",
            "⁰  ¹  ²  ³  ⁴  ⁵  ⁶  ⁷  ⁸  ⁹  ",
            "2  2  2  2  2  2  2  2  2  2  2  ",
            "0  1  2  3  4  5  6  7  8  9  10 ",
            "∞  ∞  ∞  ∞  ∞  ∞  ∞  ∞  ∞  ∞  ∞  ",
            "∞  ∞  ∞  ∞  ∞  ∞  ∞  ∞  ∞  ∞  ∞  ",
        ]
    );
}

#[test]
fn multi_line_replace_3() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    assert_eq!(
        compute(
            &["₀₁₂₃₄₅₆₇₈₉", "012345678", "⁰¹²³⁴⁵⁶⁷⁸⁹"],
            vec![text_edit((0, 3), (1, 0), "ab"), text_edit((1, 5), (1, 7), "c"), text_edit((1, 8), (2, 4), "d")]
        ),
        vec![
            "₀  ₁  ₂  a  b  0  1  2  3  4  c  7  d  ⁴  ⁵  ⁶  ⁷  ⁸  ⁹  ",
            "0  0  0  0  0  1  1  1  1  1  1  1  1  2  2  2  2  2  2  2  ",
            "0  1  2  3  4  0  1  2  3  4  5  7  8  4  5  6  7  8  9  10 ",
            "0  0  0  0  0  0  0  0  0  0  0  0  0  ∞  ∞  ∞  ∞  ∞  ∞  ∞  ",
            "3  2  1  0  0  5  4  3  2  1  0  1  0  ∞  ∞  ∞  ∞  ∞  ∞  ∞  ",
        ]
    );
}

#[test]
fn multi_line_insert_1() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    assert_eq!(
        compute(
            &["012345678"],
            vec![text_edit((0, 3), (0, 5), "a\nb")]
        ),
        vec![
            "0  1  2  a  ",
            "0  0  0  0  0  ",
            "0  1  2  3  4  ",
            "0  0  0  0  0  ",
            "3  2  1  0  0  ",
            "b  5  6  7  8  ",
            "1  0  0  0  0  0  ",
            "0  5  6  7  8  9  ",
            "0  ∞  ∞  ∞  ∞  ∞  ",
            "0  ∞  ∞  ∞  ∞  ∞  ",
        ]
    );
}

#[test]
fn multi_line_insert_2() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    assert_eq!(
        compute(
            &["012345678"],
            vec![text_edit((0, 3), (0, 5), "a\nb"), text_edit((0, 7), (0, 8), "x\ny")]
        ),
        vec![
            "0  1  2  a  ",
            "0  0  0  0  0  ",
            "0  1  2  3  4  ",
            "0  0  0  0  0  ",
            "3  2  1  0  0  ",
            "b  5  6  x  ",
            "1  0  0  0  0  ",
            "0  5  6  7  8  ",
            "0  0  0  0  0  ",
            "0  2  1  0  0  ",
            "y  8  ",
            "1  0  0  ",
            "0  8  9  ",
            "0  ∞  ∞  ",
            "0  ∞  ∞  ",
        ]
    );
}

#[test]
fn multi_line_replace_insert_1() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    assert_eq!(
        compute(
            &["₀₁₂₃₄₅₆₇₈₉", "012345678", "⁰¹²³⁴⁵⁶⁷⁸⁹"],
            vec![text_edit((0, 3), (1, 1), "aaa\nbbb")]
        ),
        vec![
            "₀  ₁  ₂  a  a  a  ",
            "0  0  0  0  0  0  0  ",
            "0  1  2  3  4  5  6  ",
            "0  0  0  0  0  0  0  ",
            "3  2  1  0  0  0  0  ",
            "b  b  b  1  2  3  4  5  6  7  8  ",
            "1  1  1  1  1  1  1  1  1  1  1  1  ",
            "0  1  2  1  2  3  4  5  6  7  8  9  ",
            "0  0  0  ∞  ∞  ∞  ∞  ∞  ∞  ∞  ∞  ∞  ",
            "0  0  0  ∞  ∞  ∞  ∞  ∞  ∞  ∞  ∞  ∞  ",
            "⁰  ¹  ²  ³  ⁴  ⁵  ⁶  ⁷  ⁸  ⁹  ",
            "2  2  2  2  2  2  2  2  2  2  2  ",
            "0  1  2  3  4  5  6  7  8  9  10 ",
            "∞  ∞  ∞  ∞  ∞  ∞  ∞  ∞  ∞  ∞  ∞  ",
            "∞  ∞  ∞  ∞  ∞  ∞  ∞  ∞  ∞  ∞  ∞  ",
        ]
    );
}

#[test]
fn multi_line_replace_insert_2() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    assert_eq!(
        compute(
            &["₀₁₂₃₄₅₆₇₈₉", "012345678", "⁰¹²³⁴⁵⁶⁷⁸⁹"],
            vec![text_edit((0, 3), (1, 1), "aaa\nbbb"), text_edit((1, 5), (1, 5), "x\ny"), text_edit((1, 7), (2, 4), "k\nl")]
        ),
        vec![
            "₀  ₁  ₂  a  a  a  ",
            "0  0  0  0  0  0  0  ",
            "0  1  2  3  4  5  6  ",
            "0  0  0  0  0  0  0  ",
            "3  2  1  0  0  0  0  ",
            "b  b  b  1  2  3  4  x  ",
            "1  1  1  1  1  1  1  1  1  ",
            "0  1  2  1  2  3  4  5  6  ",
            "0  0  0  0  0  0  0  0  0  ",
            "0  0  0  4  3  2  1  0  0  ",
            "y  5  6  k  ",
            "2  1  1  1  1  ",
            "0  5  6  7  8  ",
            "0  0  0  0  0  ",
            "0  2  1  0  0  ",
            "l  ⁴  ⁵  ⁶  ⁷  ⁸  ⁹  ",
            "2  2  2  2  2  2  2  2  ",
            "0  4  5  6  7  8  9  10 ",
            "0  ∞  ∞  ∞  ∞  ∞  ∞  ∞  ",
            "0  ∞  ∞  ∞  ∞  ∞  ∞  ∞  ",
        ]
    );
}
