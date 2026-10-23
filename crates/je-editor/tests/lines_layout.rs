//! Porte de `src/vs/editor/test/common/viewLayout/linesLayout.test.ts` (suíte `Editor ViewLayout - LinesLayout`), caso a caso e na mesma ordem.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_editor::lines_layout::{EditorWhitespace, LinesLayout};
use je_editor::view_model_types::IViewWhitespaceViewportData;

fn insert_whitespace(
    lines_layout: &mut LinesLayout,
    after_line_number: i32,
    ordinal: i32,
    height_in_px: i32,
    min_width: i32,
) -> String {
    let mut id: Option<String> = None;
    lines_layout.change_whitespace(|accessor| {
        id = Some(accessor.insert_whitespace(after_line_number, ordinal, height_in_px, min_width));
    });
    id.unwrap()
}

fn change_one_whitespace(lines_layout: &mut LinesLayout, id: &str, new_after_line_number: i32, new_height: i32) {
    lines_layout.change_whitespace(|accessor| {
        accessor.change_one_whitespace(id, new_after_line_number, new_height);
    });
}

fn remove_whitespace(lines_layout: &mut LinesLayout, id: &str) {
    lines_layout.change_whitespace(|accessor| {
        accessor.remove_whitespace(id);
    });
}

#[test]
fn lines_layout_1() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();

    // Start off with 10 lines
    let mut ll = LinesLayout::new(10, 10.0, 0.0, 0.0, &[]);

    // lines: [0, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1]
    // whitespace: -
    assert_eq!(ll.get_lines_total_height(), 100.0);
    assert_eq!(ll.get_vertical_offset_for_line_number(1, false), 0.0);
    assert_eq!(ll.get_vertical_offset_for_line_number(2, false), 10.0);
    assert_eq!(ll.get_vertical_offset_for_line_number(3, false), 20.0);
    assert_eq!(ll.get_vertical_offset_for_line_number(4, false), 30.0);
    assert_eq!(ll.get_vertical_offset_for_line_number(5, false), 40.0);
    assert_eq!(ll.get_vertical_offset_for_line_number(6, false), 50.0);
    assert_eq!(ll.get_vertical_offset_for_line_number(7, false), 60.0);
    assert_eq!(ll.get_vertical_offset_for_line_number(8, false), 70.0);
    assert_eq!(ll.get_vertical_offset_for_line_number(9, false), 80.0);
    assert_eq!(ll.get_vertical_offset_for_line_number(10, false), 90.0);

    assert_eq!(ll.get_line_number_at_or_after_vertical_offset(0), 1);
    assert_eq!(ll.get_line_number_at_or_after_vertical_offset(1), 1);
    assert_eq!(ll.get_line_number_at_or_after_vertical_offset(5), 1);
    assert_eq!(ll.get_line_number_at_or_after_vertical_offset(9), 1);
    assert_eq!(ll.get_line_number_at_or_after_vertical_offset(10), 2);
    assert_eq!(ll.get_line_number_at_or_after_vertical_offset(11), 2);
    assert_eq!(ll.get_line_number_at_or_after_vertical_offset(15), 2);
    assert_eq!(ll.get_line_number_at_or_after_vertical_offset(19), 2);
    assert_eq!(ll.get_line_number_at_or_after_vertical_offset(20), 3);
    assert_eq!(ll.get_line_number_at_or_after_vertical_offset(21), 3);
    assert_eq!(ll.get_line_number_at_or_after_vertical_offset(29), 3);

    // Add whitespace of height 5px after 2nd line
    insert_whitespace(&mut ll, 2, 0, 5, 0);
    // lines: [0, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1]
    // whitespace: a(2,5)
    assert_eq!(ll.get_lines_total_height(), 105.0);
    assert_eq!(ll.get_vertical_offset_for_line_number(1, false), 0.0);
    assert_eq!(ll.get_vertical_offset_for_line_number(2, false), 10.0);
    assert_eq!(ll.get_vertical_offset_for_line_number(3, false), 25.0);
    assert_eq!(ll.get_vertical_offset_for_line_number(4, false), 35.0);
    assert_eq!(ll.get_vertical_offset_for_line_number(5, false), 45.0);

    assert_eq!(ll.get_line_number_at_or_after_vertical_offset(0), 1);
    assert_eq!(ll.get_line_number_at_or_after_vertical_offset(1), 1);
    assert_eq!(ll.get_line_number_at_or_after_vertical_offset(9), 1);
    assert_eq!(ll.get_line_number_at_or_after_vertical_offset(10), 2);
    assert_eq!(ll.get_line_number_at_or_after_vertical_offset(20), 3);
    assert_eq!(ll.get_line_number_at_or_after_vertical_offset(21), 3);
    assert_eq!(ll.get_line_number_at_or_after_vertical_offset(24), 3);
    assert_eq!(ll.get_line_number_at_or_after_vertical_offset(25), 3);
    assert_eq!(ll.get_line_number_at_or_after_vertical_offset(35), 4);
    assert_eq!(ll.get_line_number_at_or_after_vertical_offset(45), 5);
    assert_eq!(ll.get_line_number_at_or_after_vertical_offset(104), 10);
    assert_eq!(ll.get_line_number_at_or_after_vertical_offset(105), 10);

    // Add two more whitespaces of height 5px
    insert_whitespace(&mut ll, 3, 0, 5, 0);
    insert_whitespace(&mut ll, 4, 0, 5, 0);
    // lines: [0, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1]
    // whitespace: a(2,5), b(3, 5), c(4, 5)
    assert_eq!(ll.get_lines_total_height(), 115.0);
    assert_eq!(ll.get_vertical_offset_for_line_number(1, false), 0.0);
    assert_eq!(ll.get_vertical_offset_for_line_number(2, false), 10.0);
    assert_eq!(ll.get_vertical_offset_for_line_number(3, false), 25.0);
    assert_eq!(ll.get_vertical_offset_for_line_number(4, false), 40.0);
    assert_eq!(ll.get_vertical_offset_for_line_number(5, false), 55.0);
    assert_eq!(ll.get_vertical_offset_for_line_number(6, false), 65.0);

    assert_eq!(ll.get_line_number_at_or_after_vertical_offset(0), 1);
    assert_eq!(ll.get_line_number_at_or_after_vertical_offset(1), 1);
    assert_eq!(ll.get_line_number_at_or_after_vertical_offset(9), 1);
    assert_eq!(ll.get_line_number_at_or_after_vertical_offset(10), 2);
    assert_eq!(ll.get_line_number_at_or_after_vertical_offset(19), 2);
    assert_eq!(ll.get_line_number_at_or_after_vertical_offset(20), 3);
    assert_eq!(ll.get_line_number_at_or_after_vertical_offset(34), 3);
    assert_eq!(ll.get_line_number_at_or_after_vertical_offset(35), 4);
    assert_eq!(ll.get_line_number_at_or_after_vertical_offset(49), 4);
    assert_eq!(ll.get_line_number_at_or_after_vertical_offset(50), 5);
    assert_eq!(ll.get_line_number_at_or_after_vertical_offset(64), 5);
    assert_eq!(ll.get_line_number_at_or_after_vertical_offset(65), 6);

    assert_eq!(ll.get_vertical_offset_for_whitespace_index(0), 20.0); // 20 -> 25
    assert_eq!(ll.get_vertical_offset_for_whitespace_index(1), 35.0); // 35 -> 40
    assert_eq!(ll.get_vertical_offset_for_whitespace_index(2), 50.0);

    assert_eq!(ll.get_whitespace_index_at_or_after_verticall_offset(0), 0);
    assert_eq!(ll.get_whitespace_index_at_or_after_verticall_offset(19), 0);
    assert_eq!(ll.get_whitespace_index_at_or_after_verticall_offset(20), 0);
    assert_eq!(ll.get_whitespace_index_at_or_after_verticall_offset(21), 0);
    assert_eq!(ll.get_whitespace_index_at_or_after_verticall_offset(22), 0);
    assert_eq!(ll.get_whitespace_index_at_or_after_verticall_offset(23), 0);
    assert_eq!(ll.get_whitespace_index_at_or_after_verticall_offset(24), 0);
    assert_eq!(ll.get_whitespace_index_at_or_after_verticall_offset(25), 1);
    assert_eq!(ll.get_whitespace_index_at_or_after_verticall_offset(26), 1);
    assert_eq!(ll.get_whitespace_index_at_or_after_verticall_offset(34), 1);
    assert_eq!(ll.get_whitespace_index_at_or_after_verticall_offset(35), 1);
    assert_eq!(ll.get_whitespace_index_at_or_after_verticall_offset(36), 1);
    assert_eq!(ll.get_whitespace_index_at_or_after_verticall_offset(39), 1);
    assert_eq!(ll.get_whitespace_index_at_or_after_verticall_offset(40), 2);
    assert_eq!(ll.get_whitespace_index_at_or_after_verticall_offset(41), 2);
    assert_eq!(ll.get_whitespace_index_at_or_after_verticall_offset(49), 2);
    assert_eq!(ll.get_whitespace_index_at_or_after_verticall_offset(50), 2);
    assert_eq!(ll.get_whitespace_index_at_or_after_verticall_offset(51), 2);
    assert_eq!(ll.get_whitespace_index_at_or_after_verticall_offset(54), 2);
    assert_eq!(ll.get_whitespace_index_at_or_after_verticall_offset(55), -1);
    assert_eq!(ll.get_whitespace_index_at_or_after_verticall_offset(1000), -1);
}

#[test]
fn lines_layout_2() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();

    // Start off with 10 lines and one whitespace after line 2, of height 5
    let mut ll = LinesLayout::new(10, 1.0, 0.0, 0.0, &[]);
    let a = insert_whitespace(&mut ll, 2, 0, 5, 0);

    // 10 lines
    // whitespace: - a(2,5)
    assert_eq!(ll.get_lines_total_height(), 15.0);
    assert_eq!(ll.get_vertical_offset_for_line_number(1, false), 0.0);
    assert_eq!(ll.get_vertical_offset_for_line_number(2, false), 1.0);
    assert_eq!(ll.get_vertical_offset_for_line_number(3, false), 7.0);
    assert_eq!(ll.get_vertical_offset_for_line_number(4, false), 8.0);
    assert_eq!(ll.get_vertical_offset_for_line_number(5, false), 9.0);
    assert_eq!(ll.get_vertical_offset_for_line_number(6, false), 10.0);
    assert_eq!(ll.get_vertical_offset_for_line_number(7, false), 11.0);
    assert_eq!(ll.get_vertical_offset_for_line_number(8, false), 12.0);
    assert_eq!(ll.get_vertical_offset_for_line_number(9, false), 13.0);
    assert_eq!(ll.get_vertical_offset_for_line_number(10, false), 14.0);

    // Change whitespace height
    // 10 lines
    // whitespace: - a(2,10)
    change_one_whitespace(&mut ll, &a, 2, 10);
    assert_eq!(ll.get_lines_total_height(), 20.0);
    assert_eq!(ll.get_vertical_offset_for_line_number(1, false), 0.0);
    assert_eq!(ll.get_vertical_offset_for_line_number(2, false), 1.0);
    assert_eq!(ll.get_vertical_offset_for_line_number(3, false), 12.0);
    assert_eq!(ll.get_vertical_offset_for_line_number(4, false), 13.0);
    assert_eq!(ll.get_vertical_offset_for_line_number(5, false), 14.0);
    assert_eq!(ll.get_vertical_offset_for_line_number(6, false), 15.0);
    assert_eq!(ll.get_vertical_offset_for_line_number(7, false), 16.0);
    assert_eq!(ll.get_vertical_offset_for_line_number(8, false), 17.0);
    assert_eq!(ll.get_vertical_offset_for_line_number(9, false), 18.0);
    assert_eq!(ll.get_vertical_offset_for_line_number(10, false), 19.0);

    // Change whitespace position
    // 10 lines
    // whitespace: - a(5,10)
    change_one_whitespace(&mut ll, &a, 5, 10);
    assert_eq!(ll.get_lines_total_height(), 20.0);
    assert_eq!(ll.get_vertical_offset_for_line_number(1, false), 0.0);
    assert_eq!(ll.get_vertical_offset_for_line_number(2, false), 1.0);
    assert_eq!(ll.get_vertical_offset_for_line_number(3, false), 2.0);
    assert_eq!(ll.get_vertical_offset_for_line_number(4, false), 3.0);
    assert_eq!(ll.get_vertical_offset_for_line_number(5, false), 4.0);
    assert_eq!(ll.get_vertical_offset_for_line_number(6, false), 15.0);
    assert_eq!(ll.get_vertical_offset_for_line_number(7, false), 16.0);
    assert_eq!(ll.get_vertical_offset_for_line_number(8, false), 17.0);
    assert_eq!(ll.get_vertical_offset_for_line_number(9, false), 18.0);
    assert_eq!(ll.get_vertical_offset_for_line_number(10, false), 19.0);

    // Pretend that lines 5 and 6 were deleted
    // 8 lines
    // whitespace: - a(4,10)
    ll.on_lines_deleted(5, 6);
    assert_eq!(ll.get_lines_total_height(), 18.0);
    assert_eq!(ll.get_vertical_offset_for_line_number(1, false), 0.0);
    assert_eq!(ll.get_vertical_offset_for_line_number(2, false), 1.0);
    assert_eq!(ll.get_vertical_offset_for_line_number(3, false), 2.0);
    assert_eq!(ll.get_vertical_offset_for_line_number(4, false), 3.0);
    assert_eq!(ll.get_vertical_offset_for_line_number(5, false), 14.0);
    assert_eq!(ll.get_vertical_offset_for_line_number(6, false), 15.0);
    assert_eq!(ll.get_vertical_offset_for_line_number(7, false), 16.0);
    assert_eq!(ll.get_vertical_offset_for_line_number(8, false), 17.0);

    // Insert two lines at the beginning
    // 10 lines
    // whitespace: - a(6,10)
    ll.on_lines_inserted(1, 2);
    assert_eq!(ll.get_lines_total_height(), 20.0);
    assert_eq!(ll.get_vertical_offset_for_line_number(1, false), 0.0);
    assert_eq!(ll.get_vertical_offset_for_line_number(2, false), 1.0);
    assert_eq!(ll.get_vertical_offset_for_line_number(3, false), 2.0);
    assert_eq!(ll.get_vertical_offset_for_line_number(4, false), 3.0);
    assert_eq!(ll.get_vertical_offset_for_line_number(5, false), 4.0);
    assert_eq!(ll.get_vertical_offset_for_line_number(6, false), 5.0);
    assert_eq!(ll.get_vertical_offset_for_line_number(7, false), 16.0);
    assert_eq!(ll.get_vertical_offset_for_line_number(8, false), 17.0);
    assert_eq!(ll.get_vertical_offset_for_line_number(9, false), 18.0);
    assert_eq!(ll.get_vertical_offset_for_line_number(10, false), 19.0);

    // Remove whitespace
    // 10 lines
    remove_whitespace(&mut ll, &a);
    assert_eq!(ll.get_lines_total_height(), 10.0);
    assert_eq!(ll.get_vertical_offset_for_line_number(1, false), 0.0);
    assert_eq!(ll.get_vertical_offset_for_line_number(2, false), 1.0);
    assert_eq!(ll.get_vertical_offset_for_line_number(3, false), 2.0);
    assert_eq!(ll.get_vertical_offset_for_line_number(4, false), 3.0);
    assert_eq!(ll.get_vertical_offset_for_line_number(5, false), 4.0);
    assert_eq!(ll.get_vertical_offset_for_line_number(6, false), 5.0);
    assert_eq!(ll.get_vertical_offset_for_line_number(7, false), 6.0);
    assert_eq!(ll.get_vertical_offset_for_line_number(8, false), 7.0);
    assert_eq!(ll.get_vertical_offset_for_line_number(9, false), 8.0);
    assert_eq!(ll.get_vertical_offset_for_line_number(10, false), 9.0);
}

#[test]
fn lines_layout_padding() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();

    // Start off with 10 lines
    let mut ll = LinesLayout::new(10, 10.0, 15.0, 20.0, &[]);

    // lines: [0, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1]
    // whitespace: -
    assert_eq!(ll.get_lines_total_height(), 135.0);
    assert_eq!(ll.get_vertical_offset_for_line_number(1, false), 15.0);
    assert_eq!(ll.get_vertical_offset_for_line_number(2, false), 25.0);
    assert_eq!(ll.get_vertical_offset_for_line_number(3, false), 35.0);
    assert_eq!(ll.get_vertical_offset_for_line_number(4, false), 45.0);
    assert_eq!(ll.get_vertical_offset_for_line_number(5, false), 55.0);
    assert_eq!(ll.get_vertical_offset_for_line_number(6, false), 65.0);
    assert_eq!(ll.get_vertical_offset_for_line_number(7, false), 75.0);
    assert_eq!(ll.get_vertical_offset_for_line_number(8, false), 85.0);
    assert_eq!(ll.get_vertical_offset_for_line_number(9, false), 95.0);
    assert_eq!(ll.get_vertical_offset_for_line_number(10, false), 105.0);

    assert_eq!(ll.get_line_number_at_or_after_vertical_offset(0), 1);
    assert_eq!(ll.get_line_number_at_or_after_vertical_offset(10), 1);
    assert_eq!(ll.get_line_number_at_or_after_vertical_offset(15), 1);
    assert_eq!(ll.get_line_number_at_or_after_vertical_offset(24), 1);
    assert_eq!(ll.get_line_number_at_or_after_vertical_offset(25), 2);
    assert_eq!(ll.get_line_number_at_or_after_vertical_offset(34), 2);
    assert_eq!(ll.get_line_number_at_or_after_vertical_offset(35), 3);

    // Add whitespace of height 5px after 2nd line
    insert_whitespace(&mut ll, 2, 0, 5, 0);
    // lines: [0, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1]
    // whitespace: a(2,5)
    assert_eq!(ll.get_lines_total_height(), 140.0);
    assert_eq!(ll.get_vertical_offset_for_line_number(1, false), 15.0);
    assert_eq!(ll.get_vertical_offset_for_line_number(2, false), 25.0);
    assert_eq!(ll.get_vertical_offset_for_line_number(3, false), 40.0);
    assert_eq!(ll.get_vertical_offset_for_line_number(4, false), 50.0);

    assert_eq!(ll.get_line_number_at_or_after_vertical_offset(0), 1);
    assert_eq!(ll.get_line_number_at_or_after_vertical_offset(10), 1);
    assert_eq!(ll.get_line_number_at_or_after_vertical_offset(25), 2);
    assert_eq!(ll.get_line_number_at_or_after_vertical_offset(34), 2);
    assert_eq!(ll.get_line_number_at_or_after_vertical_offset(35), 3);
    assert_eq!(ll.get_line_number_at_or_after_vertical_offset(39), 3);
    assert_eq!(ll.get_line_number_at_or_after_vertical_offset(40), 3);
    assert_eq!(ll.get_line_number_at_or_after_vertical_offset(41), 3);
    assert_eq!(ll.get_line_number_at_or_after_vertical_offset(49), 3);
    assert_eq!(ll.get_line_number_at_or_after_vertical_offset(50), 4);

    // Add two more whitespaces of height 5px
    insert_whitespace(&mut ll, 3, 0, 5, 0);
    insert_whitespace(&mut ll, 4, 0, 5, 0);
    // lines: [0, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1]
    // whitespace: a(2,5), b(3, 5), c(4, 5)
    assert_eq!(ll.get_lines_total_height(), 150.0);
    assert_eq!(ll.get_vertical_offset_for_line_number(1, false), 15.0);
    assert_eq!(ll.get_vertical_offset_for_line_number(2, false), 25.0);
    assert_eq!(ll.get_vertical_offset_for_line_number(3, false), 40.0);
    assert_eq!(ll.get_vertical_offset_for_line_number(4, false), 55.0);
    assert_eq!(ll.get_vertical_offset_for_line_number(5, false), 70.0);
    assert_eq!(ll.get_vertical_offset_for_line_number(6, false), 80.0);

    assert_eq!(ll.get_line_number_at_or_after_vertical_offset(0), 1);
    assert_eq!(ll.get_line_number_at_or_after_vertical_offset(15), 1);
    assert_eq!(ll.get_line_number_at_or_after_vertical_offset(24), 1);
    assert_eq!(ll.get_line_number_at_or_after_vertical_offset(30), 2);
    assert_eq!(ll.get_line_number_at_or_after_vertical_offset(35), 3);
    assert_eq!(ll.get_line_number_at_or_after_vertical_offset(39), 3);
    assert_eq!(ll.get_line_number_at_or_after_vertical_offset(40), 3);
    assert_eq!(ll.get_line_number_at_or_after_vertical_offset(49), 3);
    assert_eq!(ll.get_line_number_at_or_after_vertical_offset(50), 4);
    assert_eq!(ll.get_line_number_at_or_after_vertical_offset(54), 4);
    assert_eq!(ll.get_line_number_at_or_after_vertical_offset(55), 4);
    assert_eq!(ll.get_line_number_at_or_after_vertical_offset(64), 4);
    assert_eq!(ll.get_line_number_at_or_after_vertical_offset(65), 5);
    assert_eq!(ll.get_line_number_at_or_after_vertical_offset(69), 5);
    assert_eq!(ll.get_line_number_at_or_after_vertical_offset(70), 5);
    assert_eq!(ll.get_line_number_at_or_after_vertical_offset(80), 6);

    assert_eq!(ll.get_vertical_offset_for_whitespace_index(0), 35.0); // 35 -> 40
    assert_eq!(ll.get_vertical_offset_for_whitespace_index(1), 50.0); // 50 -> 55
    assert_eq!(ll.get_vertical_offset_for_whitespace_index(2), 65.0);

    assert_eq!(ll.get_whitespace_index_at_or_after_verticall_offset(0), 0);
    assert_eq!(ll.get_whitespace_index_at_or_after_verticall_offset(34), 0);
    assert_eq!(ll.get_whitespace_index_at_or_after_verticall_offset(35), 0);
    assert_eq!(ll.get_whitespace_index_at_or_after_verticall_offset(39), 0);
    assert_eq!(ll.get_whitespace_index_at_or_after_verticall_offset(40), 1);
    assert_eq!(ll.get_whitespace_index_at_or_after_verticall_offset(49), 1);
    assert_eq!(ll.get_whitespace_index_at_or_after_verticall_offset(50), 1);
    assert_eq!(ll.get_whitespace_index_at_or_after_verticall_offset(54), 1);
    assert_eq!(ll.get_whitespace_index_at_or_after_verticall_offset(55), 2);
    assert_eq!(ll.get_whitespace_index_at_or_after_verticall_offset(64), 2);
    assert_eq!(ll.get_whitespace_index_at_or_after_verticall_offset(65), 2);
    assert_eq!(ll.get_whitespace_index_at_or_after_verticall_offset(70), -1);
}

#[test]
fn lines_layout_get_line_number_at_or_after_vertical_offset() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();

    let mut ll = LinesLayout::new(10, 1.0, 0.0, 0.0, &[]);
    insert_whitespace(&mut ll, 6, 0, 10, 0);

    // 10 lines
    // whitespace: - a(6,10)
    assert_eq!(ll.get_lines_total_height(), 20.0);
    assert_eq!(ll.get_vertical_offset_for_line_number(1, false), 0.0);
    assert_eq!(ll.get_vertical_offset_for_line_number(2, false), 1.0);
    assert_eq!(ll.get_vertical_offset_for_line_number(3, false), 2.0);
    assert_eq!(ll.get_vertical_offset_for_line_number(4, false), 3.0);
    assert_eq!(ll.get_vertical_offset_for_line_number(5, false), 4.0);
    assert_eq!(ll.get_vertical_offset_for_line_number(6, false), 5.0);
    assert_eq!(ll.get_vertical_offset_for_line_number(7, false), 16.0);
    assert_eq!(ll.get_vertical_offset_for_line_number(8, false), 17.0);
    assert_eq!(ll.get_vertical_offset_for_line_number(9, false), 18.0);
    assert_eq!(ll.get_vertical_offset_for_line_number(10, false), 19.0);

    // Do some hit testing
    // line      [1, 2, 3, 4, 5, 6,  7,  8,  9, 10]
    // vertical: [0, 1, 2, 3, 4, 5, 16, 17, 18, 19]
    assert_eq!(ll.get_line_number_at_or_after_vertical_offset(-100), 1);
    assert_eq!(ll.get_line_number_at_or_after_vertical_offset(-1), 1);
    assert_eq!(ll.get_line_number_at_or_after_vertical_offset(0), 1);
    assert_eq!(ll.get_line_number_at_or_after_vertical_offset(1), 2);
    assert_eq!(ll.get_line_number_at_or_after_vertical_offset(2), 3);
    assert_eq!(ll.get_line_number_at_or_after_vertical_offset(3), 4);
    assert_eq!(ll.get_line_number_at_or_after_vertical_offset(4), 5);
    assert_eq!(ll.get_line_number_at_or_after_vertical_offset(5), 6);
    assert_eq!(ll.get_line_number_at_or_after_vertical_offset(6), 7);
    assert_eq!(ll.get_line_number_at_or_after_vertical_offset(7), 7);
    assert_eq!(ll.get_line_number_at_or_after_vertical_offset(8), 7);
    assert_eq!(ll.get_line_number_at_or_after_vertical_offset(9), 7);
    assert_eq!(ll.get_line_number_at_or_after_vertical_offset(10), 7);
    assert_eq!(ll.get_line_number_at_or_after_vertical_offset(11), 7);
    assert_eq!(ll.get_line_number_at_or_after_vertical_offset(12), 7);
    assert_eq!(ll.get_line_number_at_or_after_vertical_offset(13), 7);
    assert_eq!(ll.get_line_number_at_or_after_vertical_offset(14), 7);
    assert_eq!(ll.get_line_number_at_or_after_vertical_offset(15), 7);
    assert_eq!(ll.get_line_number_at_or_after_vertical_offset(16), 7);
    assert_eq!(ll.get_line_number_at_or_after_vertical_offset(17), 8);
    assert_eq!(ll.get_line_number_at_or_after_vertical_offset(18), 9);
    assert_eq!(ll.get_line_number_at_or_after_vertical_offset(19), 10);
    assert_eq!(ll.get_line_number_at_or_after_vertical_offset(20), 10);
    assert_eq!(ll.get_line_number_at_or_after_vertical_offset(21), 10);
    assert_eq!(ll.get_line_number_at_or_after_vertical_offset(22), 10);
    assert_eq!(ll.get_line_number_at_or_after_vertical_offset(23), 10);
}

#[test]
fn lines_layout_get_centered_line_in_viewport() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();

    let mut ll = LinesLayout::new(10, 1.0, 0.0, 0.0, &[]);
    insert_whitespace(&mut ll, 6, 0, 10, 0);

    // 10 lines
    // whitespace: - a(6,10)
    assert_eq!(ll.get_lines_total_height(), 20.0);
    assert_eq!(ll.get_vertical_offset_for_line_number(1, false), 0.0);
    assert_eq!(ll.get_vertical_offset_for_line_number(2, false), 1.0);
    assert_eq!(ll.get_vertical_offset_for_line_number(3, false), 2.0);
    assert_eq!(ll.get_vertical_offset_for_line_number(4, false), 3.0);
    assert_eq!(ll.get_vertical_offset_for_line_number(5, false), 4.0);
    assert_eq!(ll.get_vertical_offset_for_line_number(6, false), 5.0);
    assert_eq!(ll.get_vertical_offset_for_line_number(7, false), 16.0);
    assert_eq!(ll.get_vertical_offset_for_line_number(8, false), 17.0);
    assert_eq!(ll.get_vertical_offset_for_line_number(9, false), 18.0);
    assert_eq!(ll.get_vertical_offset_for_line_number(10, false), 19.0);

    // Find centered line in viewport 1
    // line      [1, 2, 3, 4, 5, 6,  7,  8,  9, 10]
    // vertical: [0, 1, 2, 3, 4, 5, 16, 17, 18, 19]
    assert_eq!(ll.get_lines_viewport_data(0, 1).centered_line_number, 1);
    assert_eq!(ll.get_lines_viewport_data(0, 2).centered_line_number, 2);
    assert_eq!(ll.get_lines_viewport_data(0, 3).centered_line_number, 2);
    assert_eq!(ll.get_lines_viewport_data(0, 4).centered_line_number, 3);
    assert_eq!(ll.get_lines_viewport_data(0, 5).centered_line_number, 3);
    assert_eq!(ll.get_lines_viewport_data(0, 6).centered_line_number, 4);
    assert_eq!(ll.get_lines_viewport_data(0, 7).centered_line_number, 4);
    assert_eq!(ll.get_lines_viewport_data(0, 8).centered_line_number, 5);
    assert_eq!(ll.get_lines_viewport_data(0, 9).centered_line_number, 5);
    assert_eq!(ll.get_lines_viewport_data(0, 10).centered_line_number, 6);
    assert_eq!(ll.get_lines_viewport_data(0, 11).centered_line_number, 6);
    assert_eq!(ll.get_lines_viewport_data(0, 12).centered_line_number, 6);
    assert_eq!(ll.get_lines_viewport_data(0, 13).centered_line_number, 6);
    assert_eq!(ll.get_lines_viewport_data(0, 14).centered_line_number, 6);
    assert_eq!(ll.get_lines_viewport_data(0, 15).centered_line_number, 6);
    assert_eq!(ll.get_lines_viewport_data(0, 16).centered_line_number, 6);
    assert_eq!(ll.get_lines_viewport_data(0, 17).centered_line_number, 7);
    assert_eq!(ll.get_lines_viewport_data(0, 18).centered_line_number, 7);
    assert_eq!(ll.get_lines_viewport_data(0, 19).centered_line_number, 7);
    assert_eq!(ll.get_lines_viewport_data(0, 20).centered_line_number, 7);
    assert_eq!(ll.get_lines_viewport_data(0, 21).centered_line_number, 7);
    assert_eq!(ll.get_lines_viewport_data(0, 22).centered_line_number, 7);
    assert_eq!(ll.get_lines_viewport_data(0, 23).centered_line_number, 7);
    assert_eq!(ll.get_lines_viewport_data(0, 24).centered_line_number, 7);
    assert_eq!(ll.get_lines_viewport_data(0, 25).centered_line_number, 7);
    assert_eq!(ll.get_lines_viewport_data(0, 26).centered_line_number, 7);
    assert_eq!(ll.get_lines_viewport_data(0, 27).centered_line_number, 7);
    assert_eq!(ll.get_lines_viewport_data(0, 28).centered_line_number, 7);
    assert_eq!(ll.get_lines_viewport_data(0, 29).centered_line_number, 7);
    assert_eq!(ll.get_lines_viewport_data(0, 30).centered_line_number, 7);
    assert_eq!(ll.get_lines_viewport_data(0, 31).centered_line_number, 7);
    assert_eq!(ll.get_lines_viewport_data(0, 32).centered_line_number, 7);
    assert_eq!(ll.get_lines_viewport_data(0, 33).centered_line_number, 7);

    // Find centered line in viewport 2
    // line      [1, 2, 3, 4, 5, 6,  7,  8,  9, 10]
    // vertical: [0, 1, 2, 3, 4, 5, 16, 17, 18, 19]
    assert_eq!(ll.get_lines_viewport_data(0, 20).centered_line_number, 7);
    assert_eq!(ll.get_lines_viewport_data(1, 20).centered_line_number, 7);
    assert_eq!(ll.get_lines_viewport_data(2, 20).centered_line_number, 7);
    assert_eq!(ll.get_lines_viewport_data(3, 20).centered_line_number, 7);
    assert_eq!(ll.get_lines_viewport_data(4, 20).centered_line_number, 7);
    assert_eq!(ll.get_lines_viewport_data(5, 20).centered_line_number, 7);
    assert_eq!(ll.get_lines_viewport_data(6, 20).centered_line_number, 7);
    assert_eq!(ll.get_lines_viewport_data(7, 20).centered_line_number, 7);
    assert_eq!(ll.get_lines_viewport_data(8, 20).centered_line_number, 7);
    assert_eq!(ll.get_lines_viewport_data(9, 20).centered_line_number, 7);
    assert_eq!(ll.get_lines_viewport_data(10, 20).centered_line_number, 7);
    assert_eq!(ll.get_lines_viewport_data(11, 20).centered_line_number, 7);
    assert_eq!(ll.get_lines_viewport_data(12, 20).centered_line_number, 7);
    assert_eq!(ll.get_lines_viewport_data(13, 20).centered_line_number, 7);
    assert_eq!(ll.get_lines_viewport_data(14, 20).centered_line_number, 8);
    assert_eq!(ll.get_lines_viewport_data(15, 20).centered_line_number, 8);
    assert_eq!(ll.get_lines_viewport_data(16, 20).centered_line_number, 9);
    assert_eq!(ll.get_lines_viewport_data(17, 20).centered_line_number, 9);
    assert_eq!(ll.get_lines_viewport_data(18, 20).centered_line_number, 10);
    assert_eq!(ll.get_lines_viewport_data(19, 20).centered_line_number, 10);
    assert_eq!(ll.get_lines_viewport_data(20, 23).centered_line_number, 10);
    assert_eq!(ll.get_lines_viewport_data(21, 23).centered_line_number, 10);
    assert_eq!(ll.get_lines_viewport_data(22, 23).centered_line_number, 10);
}

#[test]
fn lines_layout_get_lines_viewport_data_1() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();

    let mut ll = LinesLayout::new(10, 10.0, 0.0, 0.0, &[]);
    insert_whitespace(&mut ll, 6, 0, 100, 0);

    // 10 lines
    // whitespace: - a(6,100)
    assert_eq!(ll.get_lines_total_height(), 200.0);
    assert_eq!(ll.get_vertical_offset_for_line_number(1, false), 0.0);
    assert_eq!(ll.get_vertical_offset_for_line_number(2, false), 10.0);
    assert_eq!(ll.get_vertical_offset_for_line_number(3, false), 20.0);
    assert_eq!(ll.get_vertical_offset_for_line_number(4, false), 30.0);
    assert_eq!(ll.get_vertical_offset_for_line_number(5, false), 40.0);
    assert_eq!(ll.get_vertical_offset_for_line_number(6, false), 50.0);
    assert_eq!(ll.get_vertical_offset_for_line_number(7, false), 160.0);
    assert_eq!(ll.get_vertical_offset_for_line_number(8, false), 170.0);
    assert_eq!(ll.get_vertical_offset_for_line_number(9, false), 180.0);
    assert_eq!(ll.get_vertical_offset_for_line_number(10, false), 190.0);

    // viewport 0->50
    let mut viewport_data = ll.get_lines_viewport_data(0, 50);
    assert_eq!(viewport_data.start_line_number, 1);
    assert_eq!(viewport_data.end_line_number, 5);
    assert_eq!(viewport_data.completely_visible_start_line_number, 1);
    assert_eq!(viewport_data.completely_visible_end_line_number, 5);
    assert_eq!(viewport_data.relative_vertical_offset, vec![0.0, 10.0, 20.0, 30.0, 40.0]);

    // viewport 1->51
    viewport_data = ll.get_lines_viewport_data(1, 51);
    assert_eq!(viewport_data.start_line_number, 1);
    assert_eq!(viewport_data.end_line_number, 6);
    assert_eq!(viewport_data.completely_visible_start_line_number, 2);
    assert_eq!(viewport_data.completely_visible_end_line_number, 5);
    assert_eq!(viewport_data.relative_vertical_offset, vec![0.0, 10.0, 20.0, 30.0, 40.0, 50.0]);

    // viewport 5->55
    viewport_data = ll.get_lines_viewport_data(5, 55);
    assert_eq!(viewport_data.start_line_number, 1);
    assert_eq!(viewport_data.end_line_number, 6);
    assert_eq!(viewport_data.completely_visible_start_line_number, 2);
    assert_eq!(viewport_data.completely_visible_end_line_number, 5);
    assert_eq!(viewport_data.relative_vertical_offset, vec![0.0, 10.0, 20.0, 30.0, 40.0, 50.0]);

    // viewport 10->60
    viewport_data = ll.get_lines_viewport_data(10, 60);
    assert_eq!(viewport_data.start_line_number, 2);
    assert_eq!(viewport_data.end_line_number, 6);
    assert_eq!(viewport_data.completely_visible_start_line_number, 2);
    assert_eq!(viewport_data.completely_visible_end_line_number, 6);
    assert_eq!(viewport_data.relative_vertical_offset, vec![10.0, 20.0, 30.0, 40.0, 50.0]);

    // viewport 50->100
    viewport_data = ll.get_lines_viewport_data(50, 100);
    assert_eq!(viewport_data.start_line_number, 6);
    assert_eq!(viewport_data.end_line_number, 6);
    assert_eq!(viewport_data.completely_visible_start_line_number, 6);
    assert_eq!(viewport_data.completely_visible_end_line_number, 6);
    assert_eq!(viewport_data.relative_vertical_offset, vec![50.0]);

    // viewport 60->110
    viewport_data = ll.get_lines_viewport_data(60, 110);
    assert_eq!(viewport_data.start_line_number, 7);
    assert_eq!(viewport_data.end_line_number, 7);
    assert_eq!(viewport_data.completely_visible_start_line_number, 7);
    assert_eq!(viewport_data.completely_visible_end_line_number, 7);
    assert_eq!(viewport_data.relative_vertical_offset, vec![160.0]);

    // viewport 65->115
    viewport_data = ll.get_lines_viewport_data(65, 115);
    assert_eq!(viewport_data.start_line_number, 7);
    assert_eq!(viewport_data.end_line_number, 7);
    assert_eq!(viewport_data.completely_visible_start_line_number, 7);
    assert_eq!(viewport_data.completely_visible_end_line_number, 7);
    assert_eq!(viewport_data.relative_vertical_offset, vec![160.0]);

    // viewport 50->159
    viewport_data = ll.get_lines_viewport_data(50, 159);
    assert_eq!(viewport_data.start_line_number, 6);
    assert_eq!(viewport_data.end_line_number, 6);
    assert_eq!(viewport_data.completely_visible_start_line_number, 6);
    assert_eq!(viewport_data.completely_visible_end_line_number, 6);
    assert_eq!(viewport_data.relative_vertical_offset, vec![50.0]);

    // viewport 50->160
    viewport_data = ll.get_lines_viewport_data(50, 160);
    assert_eq!(viewport_data.start_line_number, 6);
    assert_eq!(viewport_data.end_line_number, 6);
    assert_eq!(viewport_data.completely_visible_start_line_number, 6);
    assert_eq!(viewport_data.completely_visible_end_line_number, 6);
    assert_eq!(viewport_data.relative_vertical_offset, vec![50.0]);

    // viewport 51->161
    viewport_data = ll.get_lines_viewport_data(51, 161);
    assert_eq!(viewport_data.start_line_number, 6);
    assert_eq!(viewport_data.end_line_number, 7);
    assert_eq!(viewport_data.completely_visible_start_line_number, 7);
    assert_eq!(viewport_data.completely_visible_end_line_number, 7);
    assert_eq!(viewport_data.relative_vertical_offset, vec![50.0, 160.0]);

    // viewport 150->169
    viewport_data = ll.get_lines_viewport_data(150, 169);
    assert_eq!(viewport_data.start_line_number, 7);
    assert_eq!(viewport_data.end_line_number, 7);
    assert_eq!(viewport_data.completely_visible_start_line_number, 7);
    assert_eq!(viewport_data.completely_visible_end_line_number, 7);
    assert_eq!(viewport_data.relative_vertical_offset, vec![160.0]);

    // viewport 159->169
    viewport_data = ll.get_lines_viewport_data(159, 169);
    assert_eq!(viewport_data.start_line_number, 7);
    assert_eq!(viewport_data.end_line_number, 7);
    assert_eq!(viewport_data.completely_visible_start_line_number, 7);
    assert_eq!(viewport_data.completely_visible_end_line_number, 7);
    assert_eq!(viewport_data.relative_vertical_offset, vec![160.0]);

    // viewport 160->169
    viewport_data = ll.get_lines_viewport_data(160, 169);
    assert_eq!(viewport_data.start_line_number, 7);
    assert_eq!(viewport_data.end_line_number, 7);
    assert_eq!(viewport_data.completely_visible_start_line_number, 7);
    assert_eq!(viewport_data.completely_visible_end_line_number, 7);
    assert_eq!(viewport_data.relative_vertical_offset, vec![160.0]);

    // viewport 160->1000
    viewport_data = ll.get_lines_viewport_data(160, 1000);
    assert_eq!(viewport_data.start_line_number, 7);
    assert_eq!(viewport_data.end_line_number, 10);
    assert_eq!(viewport_data.completely_visible_start_line_number, 7);
    assert_eq!(viewport_data.completely_visible_end_line_number, 10);
    assert_eq!(viewport_data.relative_vertical_offset, vec![160.0, 170.0, 180.0, 190.0]);
}

#[test]
fn lines_layout_get_lines_viewport_data_2_and_get_whitespace_viewport_data() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();

    let mut ll = LinesLayout::new(10, 10.0, 0.0, 0.0, &[]);
    let a = insert_whitespace(&mut ll, 6, 0, 100, 0);
    let b = insert_whitespace(&mut ll, 7, 0, 50, 0);

    // 10 lines
    // whitespace: - a(6,100), b(7, 50)
    assert_eq!(ll.get_lines_total_height(), 250.0);
    assert_eq!(ll.get_vertical_offset_for_line_number(1, false), 0.0);
    assert_eq!(ll.get_vertical_offset_for_line_number(2, false), 10.0);
    assert_eq!(ll.get_vertical_offset_for_line_number(3, false), 20.0);
    assert_eq!(ll.get_vertical_offset_for_line_number(4, false), 30.0);
    assert_eq!(ll.get_vertical_offset_for_line_number(5, false), 40.0);
    assert_eq!(ll.get_vertical_offset_for_line_number(6, false), 50.0);
    assert_eq!(ll.get_vertical_offset_for_line_number(7, false), 160.0);
    assert_eq!(ll.get_vertical_offset_for_line_number(8, false), 220.0);
    assert_eq!(ll.get_vertical_offset_for_line_number(9, false), 230.0);
    assert_eq!(ll.get_vertical_offset_for_line_number(10, false), 240.0);

    // viewport 50->160
    let mut viewport_data = ll.get_lines_viewport_data(50, 160);
    assert_eq!(viewport_data.start_line_number, 6);
    assert_eq!(viewport_data.end_line_number, 6);
    assert_eq!(viewport_data.completely_visible_start_line_number, 6);
    assert_eq!(viewport_data.completely_visible_end_line_number, 6);
    assert_eq!(viewport_data.relative_vertical_offset, vec![50.0]);
    let mut whitespace_data = ll.get_whitespace_viewport_data(50, 160);
    assert_eq!(
        whitespace_data,
        vec![IViewWhitespaceViewportData { id: a.clone(), after_line_number: 6, vertical_offset: 60.0, height: 100 }]
    );

    // viewport 50->219
    viewport_data = ll.get_lines_viewport_data(50, 219);
    assert_eq!(viewport_data.start_line_number, 6);
    assert_eq!(viewport_data.end_line_number, 7);
    assert_eq!(viewport_data.completely_visible_start_line_number, 6);
    assert_eq!(viewport_data.completely_visible_end_line_number, 7);
    assert_eq!(viewport_data.relative_vertical_offset, vec![50.0, 160.0]);
    whitespace_data = ll.get_whitespace_viewport_data(50, 219);
    assert_eq!(
        whitespace_data,
        vec![
            IViewWhitespaceViewportData { id: a.clone(), after_line_number: 6, vertical_offset: 60.0, height: 100 },
            IViewWhitespaceViewportData { id: b.clone(), after_line_number: 7, vertical_offset: 170.0, height: 50 },
        ]
    );

    // viewport 50->220
    viewport_data = ll.get_lines_viewport_data(50, 220);
    assert_eq!(viewport_data.start_line_number, 6);
    assert_eq!(viewport_data.end_line_number, 7);
    assert_eq!(viewport_data.completely_visible_start_line_number, 6);
    assert_eq!(viewport_data.completely_visible_end_line_number, 7);
    assert_eq!(viewport_data.relative_vertical_offset, vec![50.0, 160.0]);

    // viewport 50->250
    viewport_data = ll.get_lines_viewport_data(50, 250);
    assert_eq!(viewport_data.start_line_number, 6);
    assert_eq!(viewport_data.end_line_number, 10);
    assert_eq!(viewport_data.completely_visible_start_line_number, 6);
    assert_eq!(viewport_data.completely_visible_end_line_number, 10);
    assert_eq!(viewport_data.relative_vertical_offset, vec![50.0, 160.0, 220.0, 230.0, 240.0]);
}

#[test]
fn lines_layout_get_whitespace_at_vertical_offset() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();

    let mut ll = LinesLayout::new(10, 10.0, 0.0, 0.0, &[]);
    let a = insert_whitespace(&mut ll, 6, 0, 100, 0);
    let b = insert_whitespace(&mut ll, 7, 0, 50, 0);

    let mut whitespace = ll.get_whitespace_at_vertical_offset(0);
    assert_eq!(whitespace, None);

    whitespace = ll.get_whitespace_at_vertical_offset(59);
    assert_eq!(whitespace, None);

    whitespace = ll.get_whitespace_at_vertical_offset(60);
    assert_eq!(whitespace.unwrap().id, a);

    whitespace = ll.get_whitespace_at_vertical_offset(61);
    assert_eq!(whitespace.unwrap().id, a);

    whitespace = ll.get_whitespace_at_vertical_offset(159);
    assert_eq!(whitespace.unwrap().id, a);

    whitespace = ll.get_whitespace_at_vertical_offset(160);
    assert_eq!(whitespace, None);

    whitespace = ll.get_whitespace_at_vertical_offset(161);
    assert_eq!(whitespace, None);

    whitespace = ll.get_whitespace_at_vertical_offset(169);
    assert_eq!(whitespace, None);

    whitespace = ll.get_whitespace_at_vertical_offset(170);
    assert_eq!(whitespace.unwrap().id, b);

    whitespace = ll.get_whitespace_at_vertical_offset(171);
    assert_eq!(whitespace.unwrap().id, b);

    whitespace = ll.get_whitespace_at_vertical_offset(219);
    assert_eq!(whitespace.unwrap().id, b);

    whitespace = ll.get_whitespace_at_vertical_offset(220);
    assert_eq!(whitespace, None);
}

#[test]
fn lines_layout() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();

    let mut ll = LinesLayout::new(100, 20.0, 0.0, 0.0, &[]);

    // Insert a whitespace after line number 2, of height 10
    let a = insert_whitespace(&mut ll, 2, 0, 10, 0);
    // whitespaces: a(2, 10)
    assert_eq!(ll.get_whitespaces_count(), 1);
    assert_eq!(ll.get_after_line_number_for_whitespace_index(0), 2);
    assert_eq!(ll.get_height_for_whitespace_index(0), 10);
    assert_eq!(ll.get_whitespaces_accumulated_height(0), 10.0);
    assert_eq!(ll.get_whitespaces_total_height(), 10.0);
    assert_eq!(ll.get_whitespace_accumulated_height_before_line_number(1), 0.0);
    assert_eq!(ll.get_whitespace_accumulated_height_before_line_number(2), 0.0);
    assert_eq!(ll.get_whitespace_accumulated_height_before_line_number(3), 10.0);
    assert_eq!(ll.get_whitespace_accumulated_height_before_line_number(4), 10.0);

    // Insert a whitespace again after line number 2, of height 20
    let mut b = insert_whitespace(&mut ll, 2, 0, 20, 0);
    // whitespaces: a(2, 10), b(2, 20)
    assert_eq!(ll.get_whitespaces_count(), 2);
    assert_eq!(ll.get_after_line_number_for_whitespace_index(0), 2);
    assert_eq!(ll.get_height_for_whitespace_index(0), 10);
    assert_eq!(ll.get_after_line_number_for_whitespace_index(1), 2);
    assert_eq!(ll.get_height_for_whitespace_index(1), 20);
    assert_eq!(ll.get_whitespaces_accumulated_height(0), 10.0);
    assert_eq!(ll.get_whitespaces_accumulated_height(1), 30.0);
    assert_eq!(ll.get_whitespaces_total_height(), 30.0);
    assert_eq!(ll.get_whitespace_accumulated_height_before_line_number(1), 0.0);
    assert_eq!(ll.get_whitespace_accumulated_height_before_line_number(2), 0.0);
    assert_eq!(ll.get_whitespace_accumulated_height_before_line_number(3), 30.0);
    assert_eq!(ll.get_whitespace_accumulated_height_before_line_number(4), 30.0);

    // Change last inserted whitespace height to 30
    change_one_whitespace(&mut ll, &b, 2, 30);
    // whitespaces: a(2, 10), b(2, 30)
    assert_eq!(ll.get_whitespaces_count(), 2);
    assert_eq!(ll.get_after_line_number_for_whitespace_index(0), 2);
    assert_eq!(ll.get_height_for_whitespace_index(0), 10);
    assert_eq!(ll.get_after_line_number_for_whitespace_index(1), 2);
    assert_eq!(ll.get_height_for_whitespace_index(1), 30);
    assert_eq!(ll.get_whitespaces_accumulated_height(0), 10.0);
    assert_eq!(ll.get_whitespaces_accumulated_height(1), 40.0);
    assert_eq!(ll.get_whitespaces_total_height(), 40.0);
    assert_eq!(ll.get_whitespace_accumulated_height_before_line_number(1), 0.0);
    assert_eq!(ll.get_whitespace_accumulated_height_before_line_number(2), 0.0);
    assert_eq!(ll.get_whitespace_accumulated_height_before_line_number(3), 40.0);
    assert_eq!(ll.get_whitespace_accumulated_height_before_line_number(4), 40.0);

    // Remove last inserted whitespace
    remove_whitespace(&mut ll, &b);
    // whitespaces: a(2, 10)
    assert_eq!(ll.get_whitespaces_count(), 1);
    assert_eq!(ll.get_after_line_number_for_whitespace_index(0), 2);
    assert_eq!(ll.get_height_for_whitespace_index(0), 10);
    assert_eq!(ll.get_whitespaces_accumulated_height(0), 10.0);
    assert_eq!(ll.get_whitespaces_total_height(), 10.0);
    assert_eq!(ll.get_whitespace_accumulated_height_before_line_number(1), 0.0);
    assert_eq!(ll.get_whitespace_accumulated_height_before_line_number(2), 0.0);
    assert_eq!(ll.get_whitespace_accumulated_height_before_line_number(3), 10.0);
    assert_eq!(ll.get_whitespace_accumulated_height_before_line_number(4), 10.0);

    // Add a whitespace before the first line of height 50
    b = insert_whitespace(&mut ll, 0, 0, 50, 0);
    // whitespaces: b(0, 50), a(2, 10)
    assert_eq!(ll.get_whitespaces_count(), 2);
    assert_eq!(ll.get_after_line_number_for_whitespace_index(0), 0);
    assert_eq!(ll.get_height_for_whitespace_index(0), 50);
    assert_eq!(ll.get_after_line_number_for_whitespace_index(1), 2);
    assert_eq!(ll.get_height_for_whitespace_index(1), 10);
    assert_eq!(ll.get_whitespaces_accumulated_height(0), 50.0);
    assert_eq!(ll.get_whitespaces_accumulated_height(1), 60.0);
    assert_eq!(ll.get_whitespaces_total_height(), 60.0);
    assert_eq!(ll.get_whitespace_accumulated_height_before_line_number(1), 50.0);
    assert_eq!(ll.get_whitespace_accumulated_height_before_line_number(2), 50.0);
    assert_eq!(ll.get_whitespace_accumulated_height_before_line_number(3), 60.0);
    assert_eq!(ll.get_whitespace_accumulated_height_before_line_number(4), 60.0);

    // Add a whitespace after line 4 of height 20
    insert_whitespace(&mut ll, 4, 0, 20, 0);
    // whitespaces: b(0, 50), a(2, 10), c(4, 20)
    assert_eq!(ll.get_whitespaces_count(), 3);
    assert_eq!(ll.get_after_line_number_for_whitespace_index(0), 0);
    assert_eq!(ll.get_height_for_whitespace_index(0), 50);
    assert_eq!(ll.get_after_line_number_for_whitespace_index(1), 2);
    assert_eq!(ll.get_height_for_whitespace_index(1), 10);
    assert_eq!(ll.get_after_line_number_for_whitespace_index(2), 4);
    assert_eq!(ll.get_height_for_whitespace_index(2), 20);
    assert_eq!(ll.get_whitespaces_accumulated_height(0), 50.0);
    assert_eq!(ll.get_whitespaces_accumulated_height(1), 60.0);
    assert_eq!(ll.get_whitespaces_accumulated_height(2), 80.0);
    assert_eq!(ll.get_whitespaces_total_height(), 80.0);
    assert_eq!(ll.get_whitespace_accumulated_height_before_line_number(1), 50.0);
    assert_eq!(ll.get_whitespace_accumulated_height_before_line_number(2), 50.0);
    assert_eq!(ll.get_whitespace_accumulated_height_before_line_number(3), 60.0);
    assert_eq!(ll.get_whitespace_accumulated_height_before_line_number(4), 60.0);
    assert_eq!(ll.get_whitespace_accumulated_height_before_line_number(5), 80.0);

    // Add a whitespace after line 3 of height 30
    insert_whitespace(&mut ll, 3, 0, 30, 0);
    // whitespaces: b(0, 50), a(2, 10), d(3, 30), c(4, 20)
    assert_eq!(ll.get_whitespaces_count(), 4);
    assert_eq!(ll.get_after_line_number_for_whitespace_index(0), 0);
    assert_eq!(ll.get_height_for_whitespace_index(0), 50);
    assert_eq!(ll.get_after_line_number_for_whitespace_index(1), 2);
    assert_eq!(ll.get_height_for_whitespace_index(1), 10);
    assert_eq!(ll.get_after_line_number_for_whitespace_index(2), 3);
    assert_eq!(ll.get_height_for_whitespace_index(2), 30);
    assert_eq!(ll.get_after_line_number_for_whitespace_index(3), 4);
    assert_eq!(ll.get_height_for_whitespace_index(3), 20);
    assert_eq!(ll.get_whitespaces_accumulated_height(0), 50.0);
    assert_eq!(ll.get_whitespaces_accumulated_height(1), 60.0);
    assert_eq!(ll.get_whitespaces_accumulated_height(2), 90.0);
    assert_eq!(ll.get_whitespaces_accumulated_height(3), 110.0);
    assert_eq!(ll.get_whitespaces_total_height(), 110.0);
    assert_eq!(ll.get_whitespace_accumulated_height_before_line_number(1), 50.0);
    assert_eq!(ll.get_whitespace_accumulated_height_before_line_number(2), 50.0);
    assert_eq!(ll.get_whitespace_accumulated_height_before_line_number(3), 60.0);
    assert_eq!(ll.get_whitespace_accumulated_height_before_line_number(4), 90.0);
    assert_eq!(ll.get_whitespace_accumulated_height_before_line_number(5), 110.0);

    // Change whitespace after line 2 to height of 100
    change_one_whitespace(&mut ll, &a, 2, 100);
    // whitespaces: b(0, 50), a(2, 100), d(3, 30), c(4, 20)
    assert_eq!(ll.get_whitespaces_count(), 4);
    assert_eq!(ll.get_after_line_number_for_whitespace_index(0), 0);
    assert_eq!(ll.get_height_for_whitespace_index(0), 50);
    assert_eq!(ll.get_after_line_number_for_whitespace_index(1), 2);
    assert_eq!(ll.get_height_for_whitespace_index(1), 100);
    assert_eq!(ll.get_after_line_number_for_whitespace_index(2), 3);
    assert_eq!(ll.get_height_for_whitespace_index(2), 30);
    assert_eq!(ll.get_after_line_number_for_whitespace_index(3), 4);
    assert_eq!(ll.get_height_for_whitespace_index(3), 20);
    assert_eq!(ll.get_whitespaces_accumulated_height(0), 50.0);
    assert_eq!(ll.get_whitespaces_accumulated_height(1), 150.0);
    assert_eq!(ll.get_whitespaces_accumulated_height(2), 180.0);
    assert_eq!(ll.get_whitespaces_accumulated_height(3), 200.0);
    assert_eq!(ll.get_whitespaces_total_height(), 200.0);
    assert_eq!(ll.get_whitespace_accumulated_height_before_line_number(1), 50.0);
    assert_eq!(ll.get_whitespace_accumulated_height_before_line_number(2), 50.0);
    assert_eq!(ll.get_whitespace_accumulated_height_before_line_number(3), 150.0);
    assert_eq!(ll.get_whitespace_accumulated_height_before_line_number(4), 180.0);
    assert_eq!(ll.get_whitespace_accumulated_height_before_line_number(5), 200.0);

    // Remove whitespace after line 2
    remove_whitespace(&mut ll, &a);
    // whitespaces: b(0, 50), d(3, 30), c(4, 20)
    assert_eq!(ll.get_whitespaces_count(), 3);
    assert_eq!(ll.get_after_line_number_for_whitespace_index(0), 0);
    assert_eq!(ll.get_height_for_whitespace_index(0), 50);
    assert_eq!(ll.get_after_line_number_for_whitespace_index(1), 3);
    assert_eq!(ll.get_height_for_whitespace_index(1), 30);
    assert_eq!(ll.get_after_line_number_for_whitespace_index(2), 4);
    assert_eq!(ll.get_height_for_whitespace_index(2), 20);
    assert_eq!(ll.get_whitespaces_accumulated_height(0), 50.0);
    assert_eq!(ll.get_whitespaces_accumulated_height(1), 80.0);
    assert_eq!(ll.get_whitespaces_accumulated_height(2), 100.0);
    assert_eq!(ll.get_whitespaces_total_height(), 100.0);
    assert_eq!(ll.get_whitespace_accumulated_height_before_line_number(1), 50.0);
    assert_eq!(ll.get_whitespace_accumulated_height_before_line_number(2), 50.0);
    assert_eq!(ll.get_whitespace_accumulated_height_before_line_number(3), 50.0);
    assert_eq!(ll.get_whitespace_accumulated_height_before_line_number(4), 80.0);
    assert_eq!(ll.get_whitespace_accumulated_height_before_line_number(5), 100.0);

    // Remove whitespace before line 1
    remove_whitespace(&mut ll, &b);
    // whitespaces: d(3, 30), c(4, 20)
    assert_eq!(ll.get_whitespaces_count(), 2);
    assert_eq!(ll.get_after_line_number_for_whitespace_index(0), 3);
    assert_eq!(ll.get_height_for_whitespace_index(0), 30);
    assert_eq!(ll.get_after_line_number_for_whitespace_index(1), 4);
    assert_eq!(ll.get_height_for_whitespace_index(1), 20);
    assert_eq!(ll.get_whitespaces_accumulated_height(0), 30.0);
    assert_eq!(ll.get_whitespaces_accumulated_height(1), 50.0);
    assert_eq!(ll.get_whitespaces_total_height(), 50.0);
    assert_eq!(ll.get_whitespace_accumulated_height_before_line_number(1), 0.0);
    assert_eq!(ll.get_whitespace_accumulated_height_before_line_number(2), 0.0);
    assert_eq!(ll.get_whitespace_accumulated_height_before_line_number(3), 0.0);
    assert_eq!(ll.get_whitespace_accumulated_height_before_line_number(4), 30.0);
    assert_eq!(ll.get_whitespace_accumulated_height_before_line_number(5), 50.0);

    // Delete line 1
    ll.on_lines_deleted(1, 1);
    // whitespaces: d(2, 30), c(3, 20)
    assert_eq!(ll.get_whitespaces_count(), 2);
    assert_eq!(ll.get_after_line_number_for_whitespace_index(0), 2);
    assert_eq!(ll.get_height_for_whitespace_index(0), 30);
    assert_eq!(ll.get_after_line_number_for_whitespace_index(1), 3);
    assert_eq!(ll.get_height_for_whitespace_index(1), 20);
    assert_eq!(ll.get_whitespaces_accumulated_height(0), 30.0);
    assert_eq!(ll.get_whitespaces_accumulated_height(1), 50.0);
    assert_eq!(ll.get_whitespaces_total_height(), 50.0);
    assert_eq!(ll.get_whitespace_accumulated_height_before_line_number(1), 0.0);
    assert_eq!(ll.get_whitespace_accumulated_height_before_line_number(2), 0.0);
    assert_eq!(ll.get_whitespace_accumulated_height_before_line_number(3), 30.0);
    assert_eq!(ll.get_whitespace_accumulated_height_before_line_number(4), 50.0);
    assert_eq!(ll.get_whitespace_accumulated_height_before_line_number(5), 50.0);

    // Insert a line before line 1
    ll.on_lines_inserted(1, 1);
    // whitespaces: d(3, 30), c(4, 20)
    assert_eq!(ll.get_whitespaces_count(), 2);
    assert_eq!(ll.get_after_line_number_for_whitespace_index(0), 3);
    assert_eq!(ll.get_height_for_whitespace_index(0), 30);
    assert_eq!(ll.get_after_line_number_for_whitespace_index(1), 4);
    assert_eq!(ll.get_height_for_whitespace_index(1), 20);
    assert_eq!(ll.get_whitespaces_accumulated_height(0), 30.0);
    assert_eq!(ll.get_whitespaces_accumulated_height(1), 50.0);
    assert_eq!(ll.get_whitespaces_total_height(), 50.0);
    assert_eq!(ll.get_whitespace_accumulated_height_before_line_number(1), 0.0);
    assert_eq!(ll.get_whitespace_accumulated_height_before_line_number(2), 0.0);
    assert_eq!(ll.get_whitespace_accumulated_height_before_line_number(3), 0.0);
    assert_eq!(ll.get_whitespace_accumulated_height_before_line_number(4), 30.0);
    assert_eq!(ll.get_whitespace_accumulated_height_before_line_number(5), 50.0);

    // Delete line 4
    ll.on_lines_deleted(4, 4);
    // whitespaces: d(3, 30), c(3, 20)
    assert_eq!(ll.get_whitespaces_count(), 2);
    assert_eq!(ll.get_after_line_number_for_whitespace_index(0), 3);
    assert_eq!(ll.get_height_for_whitespace_index(0), 30);
    assert_eq!(ll.get_after_line_number_for_whitespace_index(1), 3);
    assert_eq!(ll.get_height_for_whitespace_index(1), 20);
    assert_eq!(ll.get_whitespaces_accumulated_height(0), 30.0);
    assert_eq!(ll.get_whitespaces_accumulated_height(1), 50.0);
    assert_eq!(ll.get_whitespaces_total_height(), 50.0);
    assert_eq!(ll.get_whitespace_accumulated_height_before_line_number(1), 0.0);
    assert_eq!(ll.get_whitespace_accumulated_height_before_line_number(2), 0.0);
    assert_eq!(ll.get_whitespace_accumulated_height_before_line_number(3), 0.0);
    assert_eq!(ll.get_whitespace_accumulated_height_before_line_number(4), 50.0);
    assert_eq!(ll.get_whitespace_accumulated_height_before_line_number(5), 50.0);
}

#[test]
fn lines_layout_find_insertion_index() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();

    let make_internal_whitespace = |after_line_numbers: &[i32], ordinal: i32| -> Vec<EditorWhitespace> {
        after_line_numbers.iter().map(|&after_line_number| EditorWhitespace::new("", after_line_number, ordinal, 0, 0)).collect()
    };

    // Cada linha confere `findInsertionIndex(arr, n, 0)` para os `n` dados e os índices esperados, na ordem do original.
    let check = |arr: &[EditorWhitespace], expected: &[(i32, usize)]| {
        for &(after_line_number, index) in expected {
            assert_eq!(LinesLayout::find_insertion_index(arr, after_line_number, 0), index);
        }
    };

    let mut arr = make_internal_whitespace(&[], 0);
    check(&arr, &[(0, 0), (1, 0), (2, 0)]);

    arr = make_internal_whitespace(&[1], 0);
    check(&arr, &[(0, 0), (1, 1), (2, 1)]);

    arr = make_internal_whitespace(&[1, 3], 0);
    check(&arr, &[(0, 0), (1, 1), (2, 1), (3, 2), (4, 2)]);

    arr = make_internal_whitespace(&[1, 3, 5], 0);
    check(&arr, &[(0, 0), (1, 1), (2, 1), (3, 2), (4, 2), (5, 3), (6, 3)]);

    arr = make_internal_whitespace(&[1, 3, 5], 3);
    check(&arr, &[(0, 0), (1, 0), (2, 1), (3, 1), (4, 2), (5, 2), (6, 3)]);

    arr = make_internal_whitespace(&[1, 3, 5, 7], 0);
    check(&arr, &[(0, 0), (1, 1), (2, 1), (3, 2), (4, 2), (5, 3), (6, 3), (7, 4), (8, 4)]);

    arr = make_internal_whitespace(&[1, 3, 5, 7, 9], 0);
    check(&arr, &[(0, 0), (1, 1), (2, 1), (3, 2), (4, 2), (5, 3), (6, 3), (7, 4), (8, 4), (9, 5), (10, 5)]);

    arr = make_internal_whitespace(&[1, 3, 5, 7, 9, 11], 0);
    check(
        &arr,
        &[(0, 0), (1, 1), (2, 1), (3, 2), (4, 2), (5, 3), (6, 3), (7, 4), (8, 4), (9, 5), (10, 5), (11, 6), (12, 6)],
    );

    arr = make_internal_whitespace(&[1, 3, 5, 7, 9, 11, 13], 0);
    check(
        &arr,
        &[
            (0, 0), (1, 1), (2, 1), (3, 2), (4, 2), (5, 3), (6, 3), (7, 4), (8, 4), (9, 5), (10, 5), (11, 6), (12, 6),
            (13, 7), (14, 7),
        ],
    );

    arr = make_internal_whitespace(&[1, 3, 5, 7, 9, 11, 13, 15], 0);
    check(
        &arr,
        &[
            (0, 0), (1, 1), (2, 1), (3, 2), (4, 2), (5, 3), (6, 3), (7, 4), (8, 4), (9, 5), (10, 5), (11, 6), (12, 6),
            (13, 7), (14, 7), (15, 8), (16, 8),
        ],
    );
}

#[test]
fn lines_layout_change_whitespace_after_line_number_and_get_first_whitespace_index_after_line_number() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();

    let mut ll = LinesLayout::new(100, 20.0, 0.0, 0.0, &[]);

    let a = insert_whitespace(&mut ll, 0, 0, 1, 0);
    let b = insert_whitespace(&mut ll, 7, 0, 1, 0);
    let c = insert_whitespace(&mut ll, 3, 0, 1, 0);

    assert_eq!(ll.get_id_for_whitespace_index(0), a); // 0
    assert_eq!(ll.get_after_line_number_for_whitespace_index(0), 0);
    assert_eq!(ll.get_id_for_whitespace_index(1), c); // 3
    assert_eq!(ll.get_after_line_number_for_whitespace_index(1), 3);
    assert_eq!(ll.get_id_for_whitespace_index(2), b); // 7
    assert_eq!(ll.get_after_line_number_for_whitespace_index(2), 7);

    assert_eq!(ll.get_first_whitespace_index_after_line_number(1), 1); // c
    assert_eq!(ll.get_first_whitespace_index_after_line_number(2), 1); // c
    assert_eq!(ll.get_first_whitespace_index_after_line_number(3), 1); // c
    assert_eq!(ll.get_first_whitespace_index_after_line_number(4), 2); // b
    assert_eq!(ll.get_first_whitespace_index_after_line_number(5), 2); // b
    assert_eq!(ll.get_first_whitespace_index_after_line_number(6), 2); // b
    assert_eq!(ll.get_first_whitespace_index_after_line_number(7), 2); // b
    assert_eq!(ll.get_first_whitespace_index_after_line_number(8), -1); // --

    // Do not really move a
    change_one_whitespace(&mut ll, &a, 1, 1);

    assert_eq!(ll.get_id_for_whitespace_index(0), a); // 1
    assert_eq!(ll.get_after_line_number_for_whitespace_index(0), 1);
    assert_eq!(ll.get_id_for_whitespace_index(1), c); // 3
    assert_eq!(ll.get_after_line_number_for_whitespace_index(1), 3);
    assert_eq!(ll.get_id_for_whitespace_index(2), b); // 7
    assert_eq!(ll.get_after_line_number_for_whitespace_index(2), 7);

    assert_eq!(ll.get_first_whitespace_index_after_line_number(1), 0); // a
    assert_eq!(ll.get_first_whitespace_index_after_line_number(2), 1); // c
    assert_eq!(ll.get_first_whitespace_index_after_line_number(3), 1); // c
    assert_eq!(ll.get_first_whitespace_index_after_line_number(4), 2); // b
    assert_eq!(ll.get_first_whitespace_index_after_line_number(5), 2); // b
    assert_eq!(ll.get_first_whitespace_index_after_line_number(6), 2); // b
    assert_eq!(ll.get_first_whitespace_index_after_line_number(7), 2); // b
    assert_eq!(ll.get_first_whitespace_index_after_line_number(8), -1); // --

    // Do not really move a
    change_one_whitespace(&mut ll, &a, 2, 1);

    assert_eq!(ll.get_id_for_whitespace_index(0), a); // 2
    assert_eq!(ll.get_after_line_number_for_whitespace_index(0), 2);
    assert_eq!(ll.get_id_for_whitespace_index(1), c); // 3
    assert_eq!(ll.get_after_line_number_for_whitespace_index(1), 3);
    assert_eq!(ll.get_id_for_whitespace_index(2), b); // 7
    assert_eq!(ll.get_after_line_number_for_whitespace_index(2), 7);

    assert_eq!(ll.get_first_whitespace_index_after_line_number(1), 0); // a
    assert_eq!(ll.get_first_whitespace_index_after_line_number(2), 0); // a
    assert_eq!(ll.get_first_whitespace_index_after_line_number(3), 1); // c
    assert_eq!(ll.get_first_whitespace_index_after_line_number(4), 2); // b
    assert_eq!(ll.get_first_whitespace_index_after_line_number(5), 2); // b
    assert_eq!(ll.get_first_whitespace_index_after_line_number(6), 2); // b
    assert_eq!(ll.get_first_whitespace_index_after_line_number(7), 2); // b
    assert_eq!(ll.get_first_whitespace_index_after_line_number(8), -1); // --

    // Change a to conflict with c => a gets placed after c
    change_one_whitespace(&mut ll, &a, 3, 1);

    assert_eq!(ll.get_id_for_whitespace_index(0), c); // 3
    assert_eq!(ll.get_after_line_number_for_whitespace_index(0), 3);
    assert_eq!(ll.get_id_for_whitespace_index(1), a); // 3
    assert_eq!(ll.get_after_line_number_for_whitespace_index(1), 3);
    assert_eq!(ll.get_id_for_whitespace_index(2), b); // 7
    assert_eq!(ll.get_after_line_number_for_whitespace_index(2), 7);

    assert_eq!(ll.get_first_whitespace_index_after_line_number(1), 0); // c
    assert_eq!(ll.get_first_whitespace_index_after_line_number(2), 0); // c
    assert_eq!(ll.get_first_whitespace_index_after_line_number(3), 0); // c
    assert_eq!(ll.get_first_whitespace_index_after_line_number(4), 2); // b
    assert_eq!(ll.get_first_whitespace_index_after_line_number(5), 2); // b
    assert_eq!(ll.get_first_whitespace_index_after_line_number(6), 2); // b
    assert_eq!(ll.get_first_whitespace_index_after_line_number(7), 2); // b
    assert_eq!(ll.get_first_whitespace_index_after_line_number(8), -1); // --

    // Make a no-op
    change_one_whitespace(&mut ll, &c, 3, 1);

    assert_eq!(ll.get_id_for_whitespace_index(0), c); // 3
    assert_eq!(ll.get_after_line_number_for_whitespace_index(0), 3);
    assert_eq!(ll.get_id_for_whitespace_index(1), a); // 3
    assert_eq!(ll.get_after_line_number_for_whitespace_index(1), 3);
    assert_eq!(ll.get_id_for_whitespace_index(2), b); // 7
    assert_eq!(ll.get_after_line_number_for_whitespace_index(2), 7);

    assert_eq!(ll.get_first_whitespace_index_after_line_number(1), 0); // c
    assert_eq!(ll.get_first_whitespace_index_after_line_number(2), 0); // c
    assert_eq!(ll.get_first_whitespace_index_after_line_number(3), 0); // c
    assert_eq!(ll.get_first_whitespace_index_after_line_number(4), 2); // b
    assert_eq!(ll.get_first_whitespace_index_after_line_number(5), 2); // b
    assert_eq!(ll.get_first_whitespace_index_after_line_number(6), 2); // b
    assert_eq!(ll.get_first_whitespace_index_after_line_number(7), 2); // b
    assert_eq!(ll.get_first_whitespace_index_after_line_number(8), -1); // --

    // Conflict c with b => c gets placed after b
    change_one_whitespace(&mut ll, &c, 7, 1);

    assert_eq!(ll.get_id_for_whitespace_index(0), a); // 3
    assert_eq!(ll.get_after_line_number_for_whitespace_index(0), 3);
    assert_eq!(ll.get_id_for_whitespace_index(1), b); // 7
    assert_eq!(ll.get_after_line_number_for_whitespace_index(1), 7);
    assert_eq!(ll.get_id_for_whitespace_index(2), c); // 7
    assert_eq!(ll.get_after_line_number_for_whitespace_index(2), 7);

    assert_eq!(ll.get_first_whitespace_index_after_line_number(1), 0); // a
    assert_eq!(ll.get_first_whitespace_index_after_line_number(2), 0); // a
    assert_eq!(ll.get_first_whitespace_index_after_line_number(3), 0); // a
    assert_eq!(ll.get_first_whitespace_index_after_line_number(4), 1); // b
    assert_eq!(ll.get_first_whitespace_index_after_line_number(5), 1); // b
    assert_eq!(ll.get_first_whitespace_index_after_line_number(6), 1); // b
    assert_eq!(ll.get_first_whitespace_index_after_line_number(7), 1); // b
    assert_eq!(ll.get_first_whitespace_index_after_line_number(8), -1); // --
}

#[test]
fn lines_layout_bug() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();

    let mut ll = LinesLayout::new(100, 20.0, 0.0, 0.0, &[]);

    let a = insert_whitespace(&mut ll, 0, 0, 1, 0);
    let b = insert_whitespace(&mut ll, 7, 0, 1, 0);

    assert_eq!(ll.get_id_for_whitespace_index(0), a); // 0
    assert_eq!(ll.get_id_for_whitespace_index(1), b); // 7

    let c = insert_whitespace(&mut ll, 3, 0, 1, 0);

    assert_eq!(ll.get_id_for_whitespace_index(0), a); // 0
    assert_eq!(ll.get_id_for_whitespace_index(1), c); // 3
    assert_eq!(ll.get_id_for_whitespace_index(2), b); // 7

    let d = insert_whitespace(&mut ll, 2, 0, 1, 0);
    assert_eq!(ll.get_id_for_whitespace_index(0), a); // 0
    assert_eq!(ll.get_id_for_whitespace_index(1), d); // 2
    assert_eq!(ll.get_id_for_whitespace_index(2), c); // 3
    assert_eq!(ll.get_id_for_whitespace_index(3), b); // 7

    let e = insert_whitespace(&mut ll, 8, 0, 1, 0);
    assert_eq!(ll.get_id_for_whitespace_index(0), a); // 0
    assert_eq!(ll.get_id_for_whitespace_index(1), d); // 2
    assert_eq!(ll.get_id_for_whitespace_index(2), c); // 3
    assert_eq!(ll.get_id_for_whitespace_index(3), b); // 7
    assert_eq!(ll.get_id_for_whitespace_index(4), e); // 8

    let f = insert_whitespace(&mut ll, 11, 0, 1, 0);
    assert_eq!(ll.get_id_for_whitespace_index(0), a); // 0
    assert_eq!(ll.get_id_for_whitespace_index(1), d); // 2
    assert_eq!(ll.get_id_for_whitespace_index(2), c); // 3
    assert_eq!(ll.get_id_for_whitespace_index(3), b); // 7
    assert_eq!(ll.get_id_for_whitespace_index(4), e); // 8
    assert_eq!(ll.get_id_for_whitespace_index(5), f); // 11

    let g = insert_whitespace(&mut ll, 10, 0, 1, 0);
    assert_eq!(ll.get_id_for_whitespace_index(0), a); // 0
    assert_eq!(ll.get_id_for_whitespace_index(1), d); // 2
    assert_eq!(ll.get_id_for_whitespace_index(2), c); // 3
    assert_eq!(ll.get_id_for_whitespace_index(3), b); // 7
    assert_eq!(ll.get_id_for_whitespace_index(4), e); // 8
    assert_eq!(ll.get_id_for_whitespace_index(5), g); // 10
    assert_eq!(ll.get_id_for_whitespace_index(6), f); // 11

    let h = insert_whitespace(&mut ll, 0, 0, 1, 0);
    assert_eq!(ll.get_id_for_whitespace_index(0), a); // 0
    assert_eq!(ll.get_id_for_whitespace_index(1), h); // 0
    assert_eq!(ll.get_id_for_whitespace_index(2), d); // 2
    assert_eq!(ll.get_id_for_whitespace_index(3), c); // 3
    assert_eq!(ll.get_id_for_whitespace_index(4), b); // 7
    assert_eq!(ll.get_id_for_whitespace_index(5), e); // 8
    assert_eq!(ll.get_id_for_whitespace_index(6), g); // 10
    assert_eq!(ll.get_id_for_whitespace_index(7), f); // 11
}
