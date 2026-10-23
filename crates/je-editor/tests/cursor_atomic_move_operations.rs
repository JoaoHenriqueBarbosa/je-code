//! Porte de `src/vs/editor/test/common/controller/cursorAtomicMoveOperations.test.ts` (suíte `Cursor move command test`), caso a caso e na mesma ordem.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::JsString;
use je_editor::cursor_atomic_move_operations::{atomic_position, whitespace_visible_column, Direction};

fn js(text: &str) -> JsString {
    JsString::from(text)
}

#[test]
fn test_whitespace_visible_column() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();

    struct TestCase {
        line_content: &'static str,
        tab_size: i32,
        expected_prev_tab_stop_position: &'static [i32],
        expected_prev_tab_stop_visible_column: &'static [i32],
        expected_visible_column: &'static [i32],
    }

    let test_cases = [
        TestCase {
            line_content: "        ",
            tab_size: 4,
            expected_prev_tab_stop_position: &[-1, 0, 0, 0, 0, 4, 4, 4, 4, -1],
            expected_prev_tab_stop_visible_column: &[-1, 0, 0, 0, 0, 4, 4, 4, 4, -1],
            expected_visible_column: &[0, 1, 2, 3, 4, 5, 6, 7, 8, -1],
        },
        TestCase {
            line_content: "  ",
            tab_size: 4,
            expected_prev_tab_stop_position: &[-1, 0, 0, -1],
            expected_prev_tab_stop_visible_column: &[-1, 0, 0, -1],
            expected_visible_column: &[0, 1, 2, -1],
        },
        TestCase {
            line_content: "\t",
            tab_size: 4,
            expected_prev_tab_stop_position: &[-1, 0, -1],
            expected_prev_tab_stop_visible_column: &[-1, 0, -1],
            expected_visible_column: &[0, 4, -1],
        },
        TestCase {
            line_content: "\t ",
            tab_size: 4,
            expected_prev_tab_stop_position: &[-1, 0, 1, -1],
            expected_prev_tab_stop_visible_column: &[-1, 0, 4, -1],
            expected_visible_column: &[0, 4, 5, -1],
        },
        TestCase {
            line_content: " \t\t ",
            tab_size: 4,
            expected_prev_tab_stop_position: &[-1, 0, 0, 2, 3, -1],
            expected_prev_tab_stop_visible_column: &[-1, 0, 0, 4, 8, -1],
            expected_visible_column: &[0, 1, 4, 8, 9, -1],
        },
        TestCase {
            line_content: " \tA",
            tab_size: 4,
            expected_prev_tab_stop_position: &[-1, 0, 0, -1, -1],
            expected_prev_tab_stop_visible_column: &[-1, 0, 0, -1, -1],
            expected_visible_column: &[0, 1, 4, -1, -1],
        },
        TestCase {
            line_content: "A",
            tab_size: 4,
            expected_prev_tab_stop_position: &[-1, -1, -1],
            expected_prev_tab_stop_visible_column: &[-1, -1, -1],
            expected_visible_column: &[0, -1, -1],
        },
        TestCase {
            line_content: "",
            tab_size: 4,
            expected_prev_tab_stop_position: &[-1, -1],
            expected_prev_tab_stop_visible_column: &[-1, -1],
            expected_visible_column: &[0, -1],
        },
    ];

    for test_case in &test_cases {
        let line_content = js(test_case.line_content);
        let max_position = test_case.expected_visible_column.len();
        for position in 0..max_position {
            let actual = whitespace_visible_column(
                line_content.units(),
                position as i32,
                test_case.tab_size,
            );
            let expected = (
                test_case.expected_prev_tab_stop_position[position],
                test_case.expected_prev_tab_stop_visible_column[position],
                test_case.expected_visible_column[position],
            );
            assert_eq!(actual, expected);
        }
    }
}

#[test]
fn test_atomic_position() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();

    struct TestCase {
        line_content: &'static str,
        tab_size: i32,
        expected_left: &'static [i32],
        expected_right: &'static [i32],
        expected_nearest: &'static [i32],
    }

    let test_cases = [
        TestCase {
            line_content: "        ",
            tab_size: 4,
            expected_left: &[-1, 0, 0, 0, 0, 4, 4, 4, 4, -1],
            expected_right: &[4, 4, 4, 4, 8, 8, 8, 8, -1, -1],
            expected_nearest: &[0, 0, 0, 4, 4, 4, 4, 8, 8, -1],
        },
        TestCase {
            line_content: " \t",
            tab_size: 4,
            expected_left: &[-1, 0, 0, -1],
            expected_right: &[2, 2, -1, -1],
            expected_nearest: &[0, 0, 2, -1],
        },
        TestCase {
            line_content: "\t ",
            tab_size: 4,
            expected_left: &[-1, 0, -1, -1],
            expected_right: &[1, -1, -1, -1],
            expected_nearest: &[0, 1, -1, -1],
        },
        TestCase {
            line_content: " \t ",
            tab_size: 4,
            expected_left: &[-1, 0, 0, -1, -1],
            expected_right: &[2, 2, -1, -1, -1],
            expected_nearest: &[0, 0, 2, -1, -1],
        },
        TestCase {
            line_content: "        A",
            tab_size: 4,
            expected_left: &[-1, 0, 0, 0, 0, 4, 4, 4, 4, -1, -1],
            expected_right: &[4, 4, 4, 4, 8, 8, 8, 8, -1, -1, -1],
            expected_nearest: &[0, 0, 0, 4, 4, 4, 4, 8, 8, -1, -1],
        },
        TestCase {
            line_content: "      foo",
            tab_size: 4,
            expected_left: &[-1, 0, 0, 0, 0, -1, -1, -1, -1, -1, -1],
            expected_right: &[4, 4, 4, 4, -1, -1, -1, -1, -1, -1, -1],
            expected_nearest: &[0, 0, 0, 4, 4, -1, -1, -1, -1, -1, -1],
        },
    ];

    for test_case in &test_cases {
        let line_content = js(test_case.line_content);
        for (direction, expected) in [
            (Direction::Left, test_case.expected_left),
            (Direction::Right, test_case.expected_right),
            (Direction::Nearest, test_case.expected_nearest),
        ] {
            let actual: Vec<i32> = (0..expected.len())
                .map(|i| {
                    atomic_position(
                        line_content.units(),
                        i as i32,
                        test_case.tab_size,
                        direction,
                    )
                })
                .collect();
            assert_eq!(actual, expected);
        }
    }
}
