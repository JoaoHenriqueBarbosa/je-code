//! Porte de `src/vs/editor/test/common/core/cursorColumns.test.ts` (suíte `CursorColumns`), caso a caso e na mesma ordem.
//!
//! Os dois laços do original que geram um caso por função (`nextRenderTabStop` e `nextIndentTabStop`;
//! `prevRenderTabStop` e `prevIndentTabStop`) viram um caso por nome, chamando o mesmo corpo.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::JsString;
use je_editor::cursor_columns;

fn js(text: &str) -> JsString {
    JsString::from(text)
}

#[test]
fn visible_column_from_column() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();

    fn test_visible_column_from_column(text: &str, tab_size: i32, column: i32, expected: i32) {
        assert_eq!(cursor_columns::visible_column_from_column(js(text).units(), column, tab_size), expected);
    }

    test_visible_column_from_column("\t\tvar x = 3;", 4, 1, 0);
    test_visible_column_from_column("\t\tvar x = 3;", 4, 2, 4);
    test_visible_column_from_column("\t\tvar x = 3;", 4, 3, 8);
    test_visible_column_from_column("\t\tvar x = 3;", 4, 4, 9);
    test_visible_column_from_column("\t\tvar x = 3;", 4, 5, 10);
    test_visible_column_from_column("\t\tvar x = 3;", 4, 6, 11);
    test_visible_column_from_column("\t\tvar x = 3;", 4, 7, 12);
    test_visible_column_from_column("\t\tvar x = 3;", 4, 8, 13);
    test_visible_column_from_column("\t\tvar x = 3;", 4, 9, 14);
    test_visible_column_from_column("\t\tvar x = 3;", 4, 10, 15);
    test_visible_column_from_column("\t\tvar x = 3;", 4, 11, 16);
    test_visible_column_from_column("\t\tvar x = 3;", 4, 12, 17);
    test_visible_column_from_column("\t\tvar x = 3;", 4, 13, 18);

    test_visible_column_from_column("\t \tvar x = 3;", 4, 1, 0);
    test_visible_column_from_column("\t \tvar x = 3;", 4, 2, 4);
    test_visible_column_from_column("\t \tvar x = 3;", 4, 3, 5);
    test_visible_column_from_column("\t \tvar x = 3;", 4, 4, 8);
    test_visible_column_from_column("\t \tvar x = 3;", 4, 5, 9);
    test_visible_column_from_column("\t \tvar x = 3;", 4, 6, 10);
    test_visible_column_from_column("\t \tvar x = 3;", 4, 7, 11);
    test_visible_column_from_column("\t \tvar x = 3;", 4, 8, 12);
    test_visible_column_from_column("\t \tvar x = 3;", 4, 9, 13);
    test_visible_column_from_column("\t \tvar x = 3;", 4, 10, 14);
    test_visible_column_from_column("\t \tvar x = 3;", 4, 11, 15);
    test_visible_column_from_column("\t \tvar x = 3;", 4, 12, 16);
    test_visible_column_from_column("\t \tvar x = 3;", 4, 13, 17);
    test_visible_column_from_column("\t \tvar x = 3;", 4, 14, 18);

    test_visible_column_from_column("\t  \tx\t", 4, -1, 0);
    test_visible_column_from_column("\t  \tx\t", 4, 0, 0);
    test_visible_column_from_column("\t  \tx\t", 4, 1, 0);
    test_visible_column_from_column("\t  \tx\t", 4, 2, 4);
    test_visible_column_from_column("\t  \tx\t", 4, 3, 5);
    test_visible_column_from_column("\t  \tx\t", 4, 4, 6);
    test_visible_column_from_column("\t  \tx\t", 4, 5, 8);
    test_visible_column_from_column("\t  \tx\t", 4, 6, 9);
    test_visible_column_from_column("\t  \tx\t", 4, 7, 12);
    test_visible_column_from_column("\t  \tx\t", 4, 8, 12);
    test_visible_column_from_column("\t  \tx\t", 4, 9, 12);

    test_visible_column_from_column("baz", 4, 1, 0);
    test_visible_column_from_column("baz", 4, 2, 1);
    test_visible_column_from_column("baz", 4, 3, 2);
    test_visible_column_from_column("baz", 4, 4, 3);

    test_visible_column_from_column("📚az", 4, 1, 0);
    test_visible_column_from_column("📚az", 4, 2, 1);
    test_visible_column_from_column("📚az", 4, 3, 2);
    test_visible_column_from_column("📚az", 4, 4, 3);
    test_visible_column_from_column("📚az", 4, 5, 4);
}

#[test]
fn to_statusbar_column() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();

    fn t(text: &str, tab_size: i32, column: i32, expected: i32) {
        assert_eq!(
            cursor_columns::to_statusbar_column(js(text).units(), column, tab_size),
            expected,
            "<<t('{text}', {tab_size}, {column}, {expected})>>"
        );
    }

    t("    spaces", 4, 1, 1);
    t("    spaces", 4, 2, 2);
    t("    spaces", 4, 3, 3);
    t("    spaces", 4, 4, 4);
    t("    spaces", 4, 5, 5);
    t("    spaces", 4, 6, 6);
    t("    spaces", 4, 7, 7);
    t("    spaces", 4, 8, 8);
    t("    spaces", 4, 9, 9);
    t("    spaces", 4, 10, 10);
    t("    spaces", 4, 11, 11);

    t("\ttab", 4, 1, 1);
    t("\ttab", 4, 2, 5);
    t("\ttab", 4, 3, 6);
    t("\ttab", 4, 4, 7);
    t("\ttab", 4, 5, 8);

    t("𐌀𐌁𐌂𐌃𐌄𐌅𐌆", 4, 1, 1);
    t("𐌀𐌁𐌂𐌃𐌄𐌅𐌆", 4, 2, 2);
    t("𐌀𐌁𐌂𐌃𐌄𐌅𐌆", 4, 3, 2);
    t("𐌀𐌁𐌂𐌃𐌄𐌅𐌆", 4, 4, 3);
    t("𐌀𐌁𐌂𐌃𐌄𐌅𐌆", 4, 5, 3);
    t("𐌀𐌁𐌂𐌃𐌄𐌅𐌆", 4, 6, 4);
    t("𐌀𐌁𐌂𐌃𐌄𐌅𐌆", 4, 7, 4);
    t("𐌀𐌁𐌂𐌃𐌄𐌅𐌆", 4, 8, 5);
    t("𐌀𐌁𐌂𐌃𐌄𐌅𐌆", 4, 9, 5);
    t("𐌀𐌁𐌂𐌃𐌄𐌅𐌆", 4, 10, 6);
    t("𐌀𐌁𐌂𐌃𐌄𐌅𐌆", 4, 11, 6);
    t("𐌀𐌁𐌂𐌃𐌄𐌅𐌆", 4, 12, 7);
    t("𐌀𐌁𐌂𐌃𐌄𐌅𐌆", 4, 13, 7);
    t("𐌀𐌁𐌂𐌃𐌄𐌅𐌆", 4, 14, 8);
    t("𐌀𐌁𐌂𐌃𐌄𐌅𐌆", 4, 15, 8);

    t("🎈🎈🎈🎈", 4, 1, 1);
    t("🎈🎈🎈🎈", 4, 2, 2);
    t("🎈🎈🎈🎈", 4, 3, 2);
    t("🎈🎈🎈🎈", 4, 4, 3);
    t("🎈🎈🎈🎈", 4, 5, 3);
    t("🎈🎈🎈🎈", 4, 6, 4);
    t("🎈🎈🎈🎈", 4, 7, 4);
    t("🎈🎈🎈🎈", 4, 8, 5);
    t("🎈🎈🎈🎈", 4, 9, 5);

    t("何何何何", 4, 1, 1);
    t("何何何何", 4, 2, 2);
    t("何何何何", 4, 3, 3);
    t("何何何何", 4, 4, 4);
}

#[test]
fn column_from_visible_column() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();

    fn test_column_from_visible_column(text: &str, tab_size: i32, visible_column: i32, expected: i32) {
        assert_eq!(cursor_columns::column_from_visible_column(js(text).units(), visible_column, tab_size), expected);
    }

    // test_column_from_visible_column("\t\tvar x = 3;", 4, 0, 1);
    test_column_from_visible_column("\t\tvar x = 3;", 4, 1, 1);
    test_column_from_visible_column("\t\tvar x = 3;", 4, 2, 1);
    test_column_from_visible_column("\t\tvar x = 3;", 4, 3, 2);
    test_column_from_visible_column("\t\tvar x = 3;", 4, 4, 2);
    test_column_from_visible_column("\t\tvar x = 3;", 4, 5, 2);
    test_column_from_visible_column("\t\tvar x = 3;", 4, 6, 2);
    test_column_from_visible_column("\t\tvar x = 3;", 4, 7, 3);
    test_column_from_visible_column("\t\tvar x = 3;", 4, 8, 3);
    test_column_from_visible_column("\t\tvar x = 3;", 4, 9, 4);
    test_column_from_visible_column("\t\tvar x = 3;", 4, 10, 5);
    test_column_from_visible_column("\t\tvar x = 3;", 4, 11, 6);
    test_column_from_visible_column("\t\tvar x = 3;", 4, 12, 7);
    test_column_from_visible_column("\t\tvar x = 3;", 4, 13, 8);
    test_column_from_visible_column("\t\tvar x = 3;", 4, 14, 9);
    test_column_from_visible_column("\t\tvar x = 3;", 4, 15, 10);
    test_column_from_visible_column("\t\tvar x = 3;", 4, 16, 11);
    test_column_from_visible_column("\t\tvar x = 3;", 4, 17, 12);
    test_column_from_visible_column("\t\tvar x = 3;", 4, 18, 13);

    test_column_from_visible_column("\t \tvar x = 3;", 4, 0, 1);
    test_column_from_visible_column("\t \tvar x = 3;", 4, 1, 1);
    test_column_from_visible_column("\t \tvar x = 3;", 4, 2, 1);
    test_column_from_visible_column("\t \tvar x = 3;", 4, 3, 2);
    test_column_from_visible_column("\t \tvar x = 3;", 4, 4, 2);
    test_column_from_visible_column("\t \tvar x = 3;", 4, 5, 3);
    test_column_from_visible_column("\t \tvar x = 3;", 4, 6, 3);
    test_column_from_visible_column("\t \tvar x = 3;", 4, 7, 4);
    test_column_from_visible_column("\t \tvar x = 3;", 4, 8, 4);
    test_column_from_visible_column("\t \tvar x = 3;", 4, 9, 5);
    test_column_from_visible_column("\t \tvar x = 3;", 4, 10, 6);
    test_column_from_visible_column("\t \tvar x = 3;", 4, 11, 7);
    test_column_from_visible_column("\t \tvar x = 3;", 4, 12, 8);
    test_column_from_visible_column("\t \tvar x = 3;", 4, 13, 9);
    test_column_from_visible_column("\t \tvar x = 3;", 4, 14, 10);
    test_column_from_visible_column("\t \tvar x = 3;", 4, 15, 11);
    test_column_from_visible_column("\t \tvar x = 3;", 4, 16, 12);
    test_column_from_visible_column("\t \tvar x = 3;", 4, 17, 13);
    test_column_from_visible_column("\t \tvar x = 3;", 4, 18, 14);

    test_column_from_visible_column("\t  \tx\t", 4, -2, 1);
    test_column_from_visible_column("\t  \tx\t", 4, -1, 1);
    test_column_from_visible_column("\t  \tx\t", 4, 0, 1);
    test_column_from_visible_column("\t  \tx\t", 4, 1, 1);
    test_column_from_visible_column("\t  \tx\t", 4, 2, 1);
    test_column_from_visible_column("\t  \tx\t", 4, 3, 2);
    test_column_from_visible_column("\t  \tx\t", 4, 4, 2);
    test_column_from_visible_column("\t  \tx\t", 4, 5, 3);
    test_column_from_visible_column("\t  \tx\t", 4, 6, 4);
    test_column_from_visible_column("\t  \tx\t", 4, 7, 4);
    test_column_from_visible_column("\t  \tx\t", 4, 8, 5);
    test_column_from_visible_column("\t  \tx\t", 4, 9, 6);
    test_column_from_visible_column("\t  \tx\t", 4, 10, 6);
    test_column_from_visible_column("\t  \tx\t", 4, 11, 7);
    test_column_from_visible_column("\t  \tx\t", 4, 12, 7);
    test_column_from_visible_column("\t  \tx\t", 4, 13, 7);
    test_column_from_visible_column("\t  \tx\t", 4, 14, 7);

    test_column_from_visible_column("baz", 4, 0, 1);
    test_column_from_visible_column("baz", 4, 1, 2);
    test_column_from_visible_column("baz", 4, 2, 3);
    test_column_from_visible_column("baz", 4, 3, 4);

    test_column_from_visible_column("📚az", 4, 0, 1);
    test_column_from_visible_column("📚az", 4, 1, 1);
    test_column_from_visible_column("📚az", 4, 2, 3);
    test_column_from_visible_column("📚az", 4, 3, 4);
    test_column_from_visible_column("📚az", 4, 4, 5);
}

fn check_next_tab_stop(f: fn(i32, i32) -> i32) {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(f(0, 4), 4);
    assert_eq!(f(1, 4), 4);
    assert_eq!(f(2, 4), 4);
    assert_eq!(f(3, 4), 4);
    assert_eq!(f(4, 4), 8);
    assert_eq!(f(5, 4), 8);
    assert_eq!(f(6, 4), 8);
    assert_eq!(f(7, 4), 8);
    assert_eq!(f(8, 4), 12);

    assert_eq!(f(0, 2), 2);
    assert_eq!(f(1, 2), 2);
    assert_eq!(f(2, 2), 4);
    assert_eq!(f(3, 2), 4);
    assert_eq!(f(4, 2), 6);
    assert_eq!(f(5, 2), 6);
    assert_eq!(f(6, 2), 8);
    assert_eq!(f(7, 2), 8);
    assert_eq!(f(8, 2), 10);

    assert_eq!(f(0, 1), 1);
    assert_eq!(f(1, 1), 2);
    assert_eq!(f(2, 1), 3);
    assert_eq!(f(3, 1), 4);
    assert_eq!(f(4, 1), 5);
    assert_eq!(f(5, 1), 6);
    assert_eq!(f(6, 1), 7);
    assert_eq!(f(7, 1), 8);
    assert_eq!(f(8, 1), 9);
}

#[test]
fn next_render_tab_stop() {
    check_next_tab_stop(cursor_columns::next_render_tab_stop);
}

#[test]
fn next_indent_tab_stop() {
    check_next_tab_stop(cursor_columns::next_indent_tab_stop);
}

fn check_prev_tab_stop(f: fn(i32, i32) -> i32) {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(f(0, 4), 0);
    assert_eq!(f(1, 4), 0);
    assert_eq!(f(2, 4), 0);
    assert_eq!(f(3, 4), 0);
    assert_eq!(f(4, 4), 0);
    assert_eq!(f(5, 4), 4);
    assert_eq!(f(6, 4), 4);
    assert_eq!(f(7, 4), 4);
    assert_eq!(f(8, 4), 4);
    assert_eq!(f(9, 4), 8);

    assert_eq!(f(0, 2), 0);
    assert_eq!(f(1, 2), 0);
    assert_eq!(f(2, 2), 0);
    assert_eq!(f(3, 2), 2);
    assert_eq!(f(4, 2), 2);
    assert_eq!(f(5, 2), 4);
    assert_eq!(f(6, 2), 4);
    assert_eq!(f(7, 2), 6);
    assert_eq!(f(8, 2), 6);
    assert_eq!(f(9, 2), 8);

    assert_eq!(f(0, 1), 0);
    assert_eq!(f(1, 1), 0);
    assert_eq!(f(2, 1), 1);
    assert_eq!(f(3, 1), 2);
    assert_eq!(f(4, 1), 3);
    assert_eq!(f(5, 1), 4);
    assert_eq!(f(6, 1), 5);
    assert_eq!(f(7, 1), 6);
    assert_eq!(f(8, 1), 7);
    assert_eq!(f(9, 1), 8);
}

#[test]
fn prev_render_tab_stop() {
    check_prev_tab_stop(cursor_columns::prev_render_tab_stop);
}

#[test]
fn prev_indent_tab_stop() {
    check_prev_tab_stop(cursor_columns::prev_indent_tab_stop);
}
