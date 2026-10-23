//! Porte de `src/vs/workbench/contrib/terminalContrib/links/test/browser/terminalLinkParsing.test.ts`
//! (suíte `TerminalLinkParsing`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` vira o `LeakGuard` criado no começo de cada caso.
//! - Os testes gerados em laço (um por item de `testLinks`) viram um `#[test]` por suíte que percorre a
//!   tabela, com o link na mensagem de falha.
//! - `hasRow`, `hasCol`, `hasRowEnd` e `hasColEnd` viram a string de flags de [`test_link`]:
//!   `r` (row), `c` (col), `R` (rowEnd) e `C` (colEnd).
//! - Os índices do original contam unidades UTF-16, então os comprimentos aqui também.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::js_string::JsString;
use je_base::platform::OperatingSystem;
use je_workbench::contrib::terminal_contrib::links::terminal_link_parsing::{
    LinkPartialRange, LinkSuffix, ParsedLink, detect_link_suffixes, detect_links, get_link_suffix,
    remove_link_query_string, remove_link_suffix,
};

struct TestLink {
    link: &'static str,
    prefix: Option<&'static str>,
    suffix: Option<&'static str>,
    has_row: bool,
    has_col: bool,
    has_row_end: bool,
    has_col_end: bool,
}

fn test_link(link: &'static str, prefix: Option<&'static str>, suffix: Option<&'static str>, flags: &str) -> TestLink {
    TestLink {
        link,
        prefix,
        suffix,
        has_row: flags.contains('r'),
        has_col: flags.contains('c'),
        has_row_end: flags.contains('R'),
        has_col_end: flags.contains('C'),
    }
}

const OPERATING_SYSTEMS: [OperatingSystem; 3] =
    [OperatingSystem::Linux, OperatingSystem::Macintosh, OperatingSystem::Windows];

fn os_test_path(os: OperatingSystem) -> &'static str {
    match os {
        OperatingSystem::Linux => "/test/path/linux",
        OperatingSystem::Macintosh => "/test/path/macintosh",
        OperatingSystem::Windows => "C:\\test\\path\\windows",
    }
}

fn os_label(os: OperatingSystem) -> &'static str {
    match os {
        OperatingSystem::Linux => "[Linux]",
        OperatingSystem::Macintosh => "[macOS]",
        OperatingSystem::Windows => "[Windows]",
    }
}

const TEST_ROW: f64 = 339.0;
const TEST_COL: f64 = 12.0;
const TEST_ROW_END: f64 = 341.0;
const TEST_COL_END: f64 = 789.0;

fn test_links() -> Vec<TestLink> {
    let none = None;
    let dq = Some("\"");
    let sq = Some("'");
    vec![
        // Simple
        test_link("foo", none, none, ""),
        test_link("foo:339", none, Some(":339"), "r"),
        test_link("foo:339:12", none, Some(":339:12"), "rc"),
        test_link("foo:339:12-789", none, Some(":339:12-789"), "rcC"),
        test_link("foo:339.12", none, Some(":339.12"), "rc"),
        test_link("foo:339.12-789", none, Some(":339.12-789"), "rcC"),
        test_link("foo:339.12-341.789", none, Some(":339.12-341.789"), "rcRC"),
        test_link("foo#339", none, Some("#339"), "r"),
        test_link("foo#339:12", none, Some("#339:12"), "rc"),
        test_link("foo#339:12-789", none, Some("#339:12-789"), "rcC"),
        test_link("foo#339.12", none, Some("#339.12"), "rc"),
        test_link("foo#339.12-789", none, Some("#339.12-789"), "rcC"),
        test_link("foo#339.12-341.789", none, Some("#339.12-341.789"), "rcRC"),
        test_link("foo 339", none, Some(" 339"), "r"),
        test_link("foo 339:12", none, Some(" 339:12"), "rc"),
        test_link("foo 339:12-789", none, Some(" 339:12-789"), "rcC"),
        test_link("foo 339.12", none, Some(" 339.12"), "rc"),
        test_link("foo 339.12-789", none, Some(" 339.12-789"), "rcC"),
        test_link("foo 339.12-341.789", none, Some(" 339.12-341.789"), "rcRC"),
        test_link("foo, 339", none, Some(", 339"), "r"),
        // Double quotes
        test_link("\"foo\",339", dq, Some("\",339"), "r"),
        test_link("\"foo\",339:12", dq, Some("\",339:12"), "rc"),
        test_link("\"foo\",339.12", dq, Some("\",339.12"), "rc"),
        test_link("\"foo\", line 339", dq, Some("\", line 339"), "r"),
        test_link("\"foo\", line 339, col 12", dq, Some("\", line 339, col 12"), "rc"),
        test_link("\"foo\", line 339, column 12", dq, Some("\", line 339, column 12"), "rc"),
        test_link("\"foo\":line 339", dq, Some("\":line 339"), "r"),
        test_link("\"foo\":line 339, col 12", dq, Some("\":line 339, col 12"), "rc"),
        test_link("\"foo\":line 339, column 12", dq, Some("\":line 339, column 12"), "rc"),
        test_link("\"foo\": line 339", dq, Some("\": line 339"), "r"),
        test_link("\"foo\": line 339, col 12", dq, Some("\": line 339, col 12"), "rc"),
        test_link("\"foo\": line 339, column 12", dq, Some("\": line 339, column 12"), "rc"),
        test_link("\"foo\" on line 339", dq, Some("\" on line 339"), "r"),
        test_link("\"foo\" on line 339, col 12", dq, Some("\" on line 339, col 12"), "rc"),
        test_link("\"foo\" on line 339, column 12", dq, Some("\" on line 339, column 12"), "rc"),
        test_link("\"foo\" line 339", dq, Some("\" line 339"), "r"),
        test_link("\"foo\" line 339 column 12", dq, Some("\" line 339 column 12"), "rc"),
        // Single quotes
        test_link("'foo',339", sq, Some("',339"), "r"),
        test_link("'foo',339:12", sq, Some("',339:12"), "rc"),
        test_link("'foo',339.12", sq, Some("',339.12"), "rc"),
        test_link("'foo', line 339", sq, Some("', line 339"), "r"),
        test_link("'foo', line 339, col 12", sq, Some("', line 339, col 12"), "rc"),
        test_link("'foo', line 339, column 12", sq, Some("', line 339, column 12"), "rc"),
        test_link("'foo':line 339", sq, Some("':line 339"), "r"),
        test_link("'foo':line 339, col 12", sq, Some("':line 339, col 12"), "rc"),
        test_link("'foo':line 339, column 12", sq, Some("':line 339, column 12"), "rc"),
        test_link("'foo': line 339", sq, Some("': line 339"), "r"),
        test_link("'foo': line 339, col 12", sq, Some("': line 339, col 12"), "rc"),
        test_link("'foo': line 339, column 12", sq, Some("': line 339, column 12"), "rc"),
        test_link("'foo' on line 339", sq, Some("' on line 339"), "r"),
        test_link("'foo' on line 339, col 12", sq, Some("' on line 339, col 12"), "rc"),
        test_link("'foo' on line 339, column 12", sq, Some("' on line 339, column 12"), "rc"),
        test_link("'foo' line 339", sq, Some("' line 339"), "r"),
        test_link("'foo' line 339 column 12", sq, Some("' line 339 column 12"), "rc"),
        // No quotes
        test_link("foo, line 339", none, Some(", line 339"), "r"),
        test_link("foo, line 339, col 12", none, Some(", line 339, col 12"), "rc"),
        test_link("foo, line 339, column 12", none, Some(", line 339, column 12"), "rc"),
        test_link("foo:line 339", none, Some(":line 339"), "r"),
        test_link("foo:line 339, col 12", none, Some(":line 339, col 12"), "rc"),
        test_link("foo:line 339, column 12", none, Some(":line 339, column 12"), "rc"),
        test_link("foo: line 339", none, Some(": line 339"), "r"),
        test_link("foo: line 339, col 12", none, Some(": line 339, col 12"), "rc"),
        test_link("foo: line 339, column 12", none, Some(": line 339, column 12"), "rc"),
        test_link("foo on line 339", none, Some(" on line 339"), "r"),
        test_link("foo on line 339, col 12", none, Some(" on line 339, col 12"), "rc"),
        test_link("foo on line 339, column 12", none, Some(" on line 339, column 12"), "rc"),
        test_link("foo line 339", none, Some(" line 339"), "r"),
        test_link("foo line 339 column 12", none, Some(" line 339 column 12"), "rc"),
        // Parentheses
        test_link("foo(339)", none, Some("(339)"), "r"),
        test_link("foo(339,12)", none, Some("(339,12)"), "rc"),
        test_link("foo(339, 12)", none, Some("(339, 12)"), "rc"),
        test_link("foo (339)", none, Some(" (339)"), "r"),
        test_link("foo (339,12)", none, Some(" (339,12)"), "rc"),
        test_link("foo (339, 12)", none, Some(" (339, 12)"), "rc"),
        test_link("foo: (339)", none, Some(": (339)"), "r"),
        test_link("foo: (339,12)", none, Some(": (339,12)"), "rc"),
        test_link("foo: (339, 12)", none, Some(": (339, 12)"), "rc"),
        test_link("foo(339:12)", none, Some("(339:12)"), "rc"),
        test_link("foo (339:12)", none, Some(" (339:12)"), "rc"),
        // Square brackets
        test_link("foo[339]", none, Some("[339]"), "r"),
        test_link("foo[339,12]", none, Some("[339,12]"), "rc"),
        test_link("foo[339, 12]", none, Some("[339, 12]"), "rc"),
        test_link("foo [339]", none, Some(" [339]"), "r"),
        test_link("foo [339,12]", none, Some(" [339,12]"), "rc"),
        test_link("foo [339, 12]", none, Some(" [339, 12]"), "rc"),
        test_link("foo: [339]", none, Some(": [339]"), "r"),
        test_link("foo: [339,12]", none, Some(": [339,12]"), "rc"),
        test_link("foo: [339, 12]", none, Some(": [339, 12]"), "rc"),
        test_link("foo[339:12]", none, Some("[339:12]"), "rc"),
        test_link("foo [339:12]", none, Some(" [339:12]"), "rc"),
        // OCaml-style
        test_link("\"foo\", line 339, character 12", dq, Some("\", line 339, character 12"), "rc"),
        test_link("\"foo\", line 339, characters 12-789", dq, Some("\", line 339, characters 12-789"), "rcC"),
        test_link("\"foo\", lines 339-341", dq, Some("\", lines 339-341"), "rR"),
        test_link("\"foo\", lines 339-341, characters 12-789", dq, Some("\", lines 339-341, characters 12-789"), "rcRC"),
        // Non-breaking space
        test_link("foo\u{00A0}339:12", none, Some("\u{00A0}339:12"), "rc"),
        test_link("\"foo\" on line 339,\u{00A0}column 12", dq, Some("\" on line 339,\u{00A0}column 12"), "rc"),
        test_link("'foo' on line\u{00A0}339, column 12", sq, Some("' on line\u{00A0}339, column 12"), "rc"),
        test_link("foo (339,\u{00A0}12)", none, Some(" (339,\u{00A0}12)"), "rc"),
        test_link("foo\u{00A0}[339, 12]", none, Some("\u{00A0}[339, 12]"), "rc"),
    ]
}

fn utf16(text: &str) -> Vec<u16> {
    text.encode_utf16().collect()
}

fn js(text: &str) -> JsString {
    JsString::from(text)
}

fn unit_len(text: &str) -> usize {
    text.encode_utf16().count()
}

/// `String#replace(string, '')`: tira só a primeira ocorrência.
fn replace_first(text: &str, needle: &str) -> String {
    text.replacen(needle, "", 1)
}

fn range(index: usize, text: &str) -> LinkPartialRange {
    LinkPartialRange { index, text: js(text) }
}

fn suffix_of(
    row: Option<f64>,
    col: Option<f64>,
    row_end: Option<f64>,
    col_end: Option<f64>,
    index: usize,
    text: &str,
) -> LinkSuffix {
    LinkSuffix { row, col, row_end, col_end, suffix: range(index, text) }
}

fn link_of(path: LinkPartialRange, prefix: Option<LinkPartialRange>, suffix: Option<LinkSuffix>) -> ParsedLink {
    ParsedLink { path, prefix, suffix }
}

/// Um link sem prefixo nem sufixo, só o caminho.
fn path_only(index: usize, text: &str) -> ParsedLink {
    link_of(range(index, text), None, None)
}

/// O sufixo esperado de um item de `testLinks`, com o sufixo no fim de `index_base + link`.
fn expected_suffix(test_link: &TestLink, index_base: usize) -> LinkSuffix {
    let suffix = test_link.suffix.expect("o item tem sufixo");
    suffix_of(
        test_link.has_row.then_some(TEST_ROW),
        test_link.has_col.then_some(TEST_COL),
        test_link.has_row_end.then_some(TEST_ROW_END),
        test_link.has_col_end.then_some(TEST_COL_END),
        index_base + unit_len(test_link.link) - unit_len(suffix),
        suffix,
    )
}

mod remove_link_suffix_suite {
    use super::*;

    #[test]
    fn test_links_are_stripped_of_their_suffix() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        for test_link in test_links() {
            let expected = match test_link.suffix {
                None => test_link.link.to_string(),
                Some(suffix) => replace_first(test_link.link, suffix),
            };
            assert_eq!(remove_link_suffix(&utf16(test_link.link)), js(&expected), "`{}`", test_link.link);
        }
    }
}

mod get_link_suffix_suite {
    use super::*;

    #[test]
    fn test_links_have_the_expected_suffix() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        for test_link in test_links() {
            let expected = test_link.suffix.map(|_| expected_suffix(&test_link, 0));
            assert_eq!(get_link_suffix(&utf16(test_link.link)), expected, "`{}`", test_link.link);
        }
    }
}

mod detect_link_suffixes_suite {
    use super::*;

    #[test]
    fn test_links_have_the_expected_suffix_list() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        for test_link in test_links() {
            let expected: Vec<LinkSuffix> =
                test_link.suffix.map(|_| expected_suffix(&test_link, 0)).into_iter().collect();
            assert_eq!(detect_link_suffixes(&utf16(test_link.link)), expected, "`{}`", test_link.link);
        }
    }

    #[test]
    fn foo_1_2_bar_3_4_baz_on_line_5() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        assert_eq!(
            detect_link_suffixes(&utf16("foo(1, 2) bar[3, 4] baz on line 5")),
            vec![
                suffix_of(Some(1.0), Some(2.0), None, None, 3, "(1, 2)"),
                suffix_of(Some(3.0), Some(4.0), None, None, 13, "[3, 4]"),
                suffix_of(Some(5.0), None, None, None, 23, " on line 5"),
            ]
        );
    }
}

mod remove_link_query_string_suite {
    use super::*;

    fn remove(link: &str) -> JsString {
        remove_link_query_string(&utf16(link))
    }

    #[test]
    fn should_remove_any_query_string_from_the_link() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        assert_eq!(remove("?a=b"), js(""));
        assert_eq!(remove("foo?a=b"), js("foo"));
        assert_eq!(remove("./foo?a=b"), js("./foo"));
        assert_eq!(remove("/foo/bar?a=b"), js("/foo/bar"));
        assert_eq!(remove("foo?a=b?"), js("foo"));
        assert_eq!(remove("foo?a=b&c=d"), js("foo"));
    }

    #[test]
    fn should_respect_question_mark_in_unc_paths() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        assert_eq!(remove("\\\\?\\foo?a=b"), js("\\\\?\\foo"));
    }
}

mod detect_links_suite {
    use super::*;

    fn detect(line: &str, os: OperatingSystem) -> Vec<ParsedLink> {
        detect_links(&utf16(line), os)
    }

    #[test]
    fn foo_1_2_bar_3_4_baz_on_line_5() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        assert_eq!(
            detect("foo(1, 2) bar[3, 4] \"baz\" on line 5", OperatingSystem::Linux),
            vec![
                link_of(range(0, "foo"), None, Some(suffix_of(Some(1.0), Some(2.0), None, None, 3, "(1, 2)"))),
                link_of(range(10, "bar"), None, Some(suffix_of(Some(3.0), Some(4.0), None, None, 13, "[3, 4]"))),
                link_of(
                    range(21, "baz"),
                    Some(range(20, "\"")),
                    Some(suffix_of(Some(5.0), None, None, None, 24, "\" on line 5")),
                ),
            ]
        );
    }

    #[test]
    fn should_detect_multiple_links_when_opening_brackets_are_in_the_text() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        assert_eq!(
            detect("notlink[foo:45]", OperatingSystem::Linux),
            vec![
                link_of(range(0, "notlink[foo"), None, Some(suffix_of(Some(45.0), None, None, None, 11, ":45"))),
                link_of(range(8, "foo"), None, Some(suffix_of(Some(45.0), None, None, None, 11, ":45"))),
            ]
        );
    }

    #[test]
    fn should_extract_the_link_prefix() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        assert_eq!(
            detect("\"foo\", line 5, col 6", OperatingSystem::Linux),
            vec![link_of(
                range(1, "foo"),
                Some(range(0, "\"")),
                Some(suffix_of(Some(5.0), Some(6.0), None, None, 4, "\", line 5, col 6")),
            )]
        );
    }

    #[test]
    fn should_be_smart_about_determining_the_link_prefix_when_multiple_prefix_characters_exist() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        assert_eq!(
            detect("echo '\"foo\", line 5, col 6'", OperatingSystem::Linux),
            vec![link_of(
                range(7, "foo"),
                Some(range(6, "\"")),
                Some(suffix_of(Some(5.0), Some(6.0), None, None, 10, "\", line 5, col 6")),
            )],
            "The outer single quotes should be excluded from the link prefix and suffix"
        );
    }

    #[test]
    fn should_detect_both_suffix_and_non_suffix_links_on_a_single_line() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        assert_eq!(
            detect("PS C:\\Github\\microsoft\\vscode> echo '\"foo\", line 5, col 6'", OperatingSystem::Windows),
            vec![
                path_only(3, "C:\\Github\\microsoft\\vscode"),
                link_of(
                    range(38, "foo"),
                    Some(range(37, "\"")),
                    Some(suffix_of(Some(5.0), Some(6.0), None, None, 41, "\", line 5, col 6")),
                ),
            ]
        );
    }

    mod pipe {
        use super::*;

        #[test]
        fn should_exclude_pipe_characters_from_link_paths() {
            let _guard = ensure_no_disposables_are_leaked_in_test_suite();
            assert_eq!(
                detect("|C:\\Github\\microsoft\\vscode|", OperatingSystem::Windows),
                vec![path_only(1, "C:\\Github\\microsoft\\vscode")]
            );
        }

        #[test]
        fn should_exclude_pipe_characters_from_link_paths_with_suffixes() {
            let _guard = ensure_no_disposables_are_leaked_in_test_suite();
            assert_eq!(
                detect("|C:\\Github\\microsoft\\vscode:400|", OperatingSystem::Windows),
                vec![link_of(
                    range(1, "C:\\Github\\microsoft\\vscode"),
                    None,
                    Some(suffix_of(Some(400.0), None, None, None, 27, ":400")),
                )]
            );
        }
    }

    mod angle_brackets {
        use super::*;

        #[test]
        fn should_exclude_bracket_characters_from_link_paths() {
            let _guard = ensure_no_disposables_are_leaked_in_test_suite();
            for os in OPERATING_SYSTEMS {
                let path = os_test_path(os);
                let label = os_label(os);
                assert_eq!(detect(&format!("<{path}<"), os), vec![path_only(1, path)], "{label}");
                assert_eq!(detect(&format!(">{path}>"), os), vec![path_only(1, path)], "{label}");
            }
        }

        #[test]
        fn should_exclude_bracket_characters_from_link_paths_with_suffixes() {
            let _guard = ensure_no_disposables_are_leaked_in_test_suite();
            for os in OPERATING_SYSTEMS {
                let path = os_test_path(os);
                let label = os_label(os);
                let expected = vec![link_of(
                    range(1, path),
                    None,
                    Some(suffix_of(Some(400.0), None, None, None, 1 + unit_len(path), ":400")),
                )];
                assert_eq!(detect(&format!("<{path}:400<"), os), expected, "{label}");
                assert_eq!(detect(&format!(">{path}:400>"), os), expected, "{label}");
            }
        }
    }

    mod query_strings {
        use super::*;

        fn any_path_starts_with_question_mark(line: &str, os: OperatingSystem) -> bool {
            detect(line, os).iter().any(|link| link.path.text.units().first() == Some(&u16::from(b'?')))
        }

        #[test]
        fn should_exclude_query_strings_from_link_paths() {
            let _guard = ensure_no_disposables_are_leaked_in_test_suite();
            for os in OPERATING_SYSTEMS {
                let path = os_test_path(os);
                let label = os_label(os);
                assert_eq!(detect(&format!("{path}?a=b"), os), vec![path_only(0, path)], "{label}");
                assert_eq!(detect(&format!("{path}?a=b&c=d"), os), vec![path_only(0, path)], "{label}");
            }
        }

        #[test]
        fn should_not_detect_links_starting_with_question_mark_within_query_strings_that_contain_posix_style_paths() {
            let _guard = ensure_no_disposables_are_leaked_in_test_suite();
            // #204195: ? appended to the cwd will exist since it's just the cwd
            for os in OPERATING_SYSTEMS {
                assert!(!any_path_starts_with_question_mark("http://foo.com/?bar=/a/b&baz=c", os), "{}", os_label(os));
            }
        }

        #[test]
        fn should_not_detect_links_starting_with_question_mark_within_query_strings_that_contain_windows_style_paths() {
            let _guard = ensure_no_disposables_are_leaked_in_test_suite();
            // #204195: ? appended to the cwd will exist since it's just the cwd
            for os in OPERATING_SYSTEMS {
                assert!(!any_path_starts_with_question_mark("http://foo.com/?bar=a:\\b&baz=c", os), "{}", os_label(os));
            }
        }
    }

    mod git_diffs {
        use super::*;

        #[test]
        fn diff_a_foo_bar() {
            let _guard = ensure_no_disposables_are_leaked_in_test_suite();
            for prefix in ["a", "c", "w", "i", "o"] {
                assert_eq!(
                    detect(&format!("--- {prefix}/foo/bar"), OperatingSystem::Linux),
                    vec![path_only(6, "foo/bar")]
                );
            }
        }

        #[test]
        fn diff_b_foo_bar() {
            let _guard = ensure_no_disposables_are_leaked_in_test_suite();
            for prefix in ["b", "c", "w", "i", "o"] {
                assert_eq!(
                    detect(&format!("+++ {prefix}/foo/bar"), OperatingSystem::Linux),
                    vec![path_only(6, "foo/bar")]
                );
            }
        }

        #[test]
        fn diff_git_a_foo_bar_b_foo_baz() {
            let _guard = ensure_no_disposables_are_leaked_in_test_suite();
            for (source_prefix, destination_prefix) in [("a", "b"), ("c", "w"), ("i", "o")] {
                assert_eq!(
                    detect(
                        &format!("diff --git {source_prefix}/foo/bar {destination_prefix}/foo/baz"),
                        OperatingSystem::Linux
                    ),
                    vec![path_only(13, "foo/bar"), path_only(23, "foo/baz")]
                );
            }
        }

        #[test]
        fn numeric_prefixes_used_by_git_diff_no_index() {
            let _guard = ensure_no_disposables_are_leaked_in_test_suite();
            assert_eq!(
                vec![
                    detect("--- 1/foo/bar", OperatingSystem::Linux),
                    detect("+++ 2/foo/baz", OperatingSystem::Linux),
                    detect("diff --git 1/foo/bar 2/foo/baz", OperatingSystem::Linux),
                ],
                vec![
                    vec![path_only(6, "foo/bar")],
                    vec![path_only(6, "foo/baz")],
                    vec![path_only(13, "foo/bar"), path_only(23, "foo/baz")],
                ]
            );
        }

        #[test]
        fn reversed_numeric_prefixes_used_by_git_diff_no_index_r() {
            let _guard = ensure_no_disposables_are_leaked_in_test_suite();
            assert_eq!(
                vec![
                    detect("--- 2/foo/baz", OperatingSystem::Linux),
                    detect("+++ 1/foo/bar", OperatingSystem::Linux),
                    detect("diff --git 2/foo/baz 1/foo/bar", OperatingSystem::Linux),
                ],
                vec![
                    vec![path_only(6, "foo/baz")],
                    vec![path_only(6, "foo/bar")],
                    vec![path_only(13, "foo/baz"), path_only(23, "foo/bar")],
                ]
            );
        }

        #[test]
        fn ordinary_numeric_line_suffix() {
            let _guard = ensure_no_disposables_are_leaked_in_test_suite();
            assert_eq!(
                detect("foo 1", OperatingSystem::Linux),
                vec![link_of(range(0, "foo"), None, Some(suffix_of(Some(1.0), None, None, None, 3, " 1")))]
            );
        }

        #[test]
        fn numeric_suffix_followed_by_a_path_separator() {
            let _guard = ensure_no_disposables_are_leaked_in_test_suite();
            assert_eq!(detect("foo 1/bar", OperatingSystem::Linux), vec![path_only(4, "1/bar")]);
        }

        #[test]
        fn ordinary_numeric_line_suffix_after_diff_git_text() {
            let _guard = ensure_no_disposables_are_leaked_in_test_suite();
            assert_eq!(
                detect("diff --git foo.ts:123", OperatingSystem::Linux),
                vec![link_of(range(11, "foo.ts"), None, Some(suffix_of(Some(123.0), None, None, None, 17, ":123")))]
            );
        }
    }

    mod three_suffix_links_on_a_single_line {
        use super::*;

        #[test]
        fn should_detect_3_suffix_links_on_a_single_line() {
            let _guard = ensure_no_disposables_are_leaked_in_test_suite();
            let links = test_links();
            let with_suffix: Vec<&TestLink> = links.iter().filter(|e| e.suffix.is_some()).collect();
            for i in 0..with_suffix.len() - 2 {
                let (link1, link2, link3) = (with_suffix[i], with_suffix[i + 1], with_suffix[i + 2]);
                let line = format!(" {} {} {} ", link1.link, link2.link, link3.link);
                let shown = line.replace('\u{00A0}', "<nbsp>");
                assert_eq!(detect(&line, OperatingSystem::Linux).len(), 3, "`{shown}`");

                // O link `n` começa depois do `n - 1` mais um espaço; `start` é onde o link começa
                // (no prefixo, se houver, senão no caminho).
                let detected = |link: &TestLink, start: usize| {
                    let prefix_len = link.prefix.map_or(0, unit_len);
                    let suffix = link.suffix.expect("o item tem sufixo");
                    link_of(
                        range(
                            start + prefix_len,
                            &replace_first(&replace_first(link.link, suffix), link.prefix.unwrap_or("")),
                        ),
                        link.prefix.map(|prefix| range(start, prefix)),
                        Some(expected_suffix(link, start)),
                    )
                };
                let start1 = 1;
                let start2 = start1 + unit_len(link1.link) + 1;
                let start3 = start2 + unit_len(link2.link) + 1;
                assert_eq!(
                    detect(&line, OperatingSystem::Linux),
                    vec![detected(link1, start1), detected(link2, start2), detected(link3, start3)],
                    "`{shown}`"
                );
            }
        }
    }

    #[test]
    fn should_ignore_links_with_suffixes_when_the_path_itself_is_the_empty_string() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        assert_eq!(detect("\"\"\",1", OperatingSystem::Linux), Vec::<ParsedLink>::new());
    }
}
