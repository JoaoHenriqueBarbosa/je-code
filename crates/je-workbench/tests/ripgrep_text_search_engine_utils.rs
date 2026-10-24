//! Porte de `src/vs/workbench/services/search/test/node/ripgrepTextSearchEngineUtils.test.ts` (suíte
//! `RipgrepTextSearchEngine`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` vira o `LeakGuard` criado no começo de cada caso.
//! - `new RegExp(fixed).test(str)` vira o `JsRegExp` sem flags, que parte do zero como o `test` do JS.
//! - `JSON.stringify(<IRgMessage>{...})` vira [`IRgMessage::to_json`] serializado pelo `serde_json`.
//! - `joinPath` e `URI.file` usam a plataforma do processo, a mesma do parser.
//! - `deepStrictEqual` vira `assert_eq!`; os literais de string dos padrões viram `&str` convertidos em UTF-16
//!   pelo auxiliar `units`.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use std::cell::RefCell;
use std::rc::Rc;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::js_regexp::JsRegExp;
use je_base::path::{Environment, ProcessEnvironment};
use je_base::uri::URI;
use je_workbench::services::search::ripgrep_text_search_engine::{
    IRgBytesOrText, IRgMatch, IRgMessage, IRgSubmatch, RipgrepParser, fix_newline, fix_regex_newline, get_rg_args,
    perform_brace_expansion_for_ripgrep, unicode_escapes_to_pcre2,
};
use je_workbench::services::search::search::DEFAULT_TEXT_SEARCH_PREVIEW_OPTIONS;
use je_workbench::services::search::search_ext_types::{
    Range, TextSearchMatch2, TextSearchMatchRanges, TextSearchProviderFolderOptions, TextSearchQuery2,
    TextSearchResult2, UseIgnoreFiles,
};
use je_workbench::services::search::search_ext_types_internal::RipgrepTextSearchOptions;

fn units(text: &str) -> Vec<u16> {
    text.encode_utf16().collect()
}

fn join_path(base: &URI, fragment: &str) -> URI {
    URI::join_path(base, &[fragment], ProcessEnvironment.platform())
}

fn reg_exp_test(pattern: &je_base::js_string::JsString, text: &str) -> bool {
    JsRegExp::new(pattern.units(), "").expect("padrão válido").exec(&units(text)).is_some()
}

#[test]
fn unicode_escapes_to_pcre2_test() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let check = |input: &str, expected: &str| assert_eq!(unicode_escapes_to_pcre2(&units(input)), expected);
    check("\\u1234", "\\x{1234}");
    check("\\u1234\\u0001", "\\x{1234}\\x{0001}");
    check("foo\\u1234bar", "foo\\x{1234}bar");
    check("\\\\\\u1234", "\\\\\\x{1234}");
    check("foo\\\\\\u1234", "foo\\\\\\x{1234}");

    check("\\u{1234}", "\\x{1234}");
    check("\\u{1234}\\u{0001}", "\\x{1234}\\x{0001}");
    check("foo\\u{1234}bar", "foo\\x{1234}bar");
    check("[\\u00A0-\\u00FF]", "[\\x{00A0}-\\x{00FF}]");

    check("foo\\u{123456}7bar", "foo\\u{123456}7bar");
    check("\\u123", "\\u123");
    check("foo", "foo");
    check("", "");
}

#[test]
fn fix_regex_newline_src() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let ttable = [
        ["foo", "foo"],
        ["invalid(", "invalid("],
        ["fo\\no", "fo\\r?\\no"],
        ["f\\no\\no", "f\\r?\\no\\r?\\no"],
        ["f[a-z\\n1]", "f(?:[a-z1]|\\r?\\n)"],
        ["f[\\n-a]", "f[\\n-a]"],
        ["(?<=\\n)\\w", "(?<=\\n)\\w"],
        ["fo\\n+o", "fo(?:\\r?\\n)+o"],
        ["fo[^\\n]o", "fo(?!\\r?\\n)o"],
        ["fo[^\\na-z]o", "fo(?!\\r?\\n|[a-z])o"],
        ["foo[^\\n]+o", "foo.+o"],
        ["foo[^\\nzq]+o", "foo[^zq]+o"],
        ["foo[^\\nzq]+o", "foo[^zq]+o"],
        // preserves quantifies, #137899
        ["fo[^\\S\\n]*o", "fo[^\\S]*o"],
        ["fo[^\\S\\n]{3,}o", "fo[^\\S]{3,}o"],
    ];

    for [input, expected] in ttable {
        assert_eq!(fix_regex_newline(&units(input)), expected, "{input} -> {expected}");
    }
}

#[test]
fn fix_regex_newline_re() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let test_fix_regex_newline = |(input_reg, test_str, should_match): (&str, &str, bool)| {
        let fixed = fix_regex_newline(&units(input_reg));
        assert_eq!(reg_exp_test(&fixed, test_str), should_match, "{input_reg} => {fixed}, {test_str:?}, {should_match}");
    };

    [
        ("foo", "foo", true),
        ("foo\\n", "foo\r\n", true),
        ("foo\\n\\n", "foo\n\n", true),
        ("foo\\n\\n", "foo\r\n\r\n", true),
        ("foo\\n", "foo\n", true),
        ("foo\\nabc", "foo\r\nabc", true),
        ("foo\\nabc", "foo\nabc", true),
        ("foo\\r\\n", "foo\r\n", true),
        ("foo\\n+abc", "foo\r\nabc", true),
        ("foo\\n+abc", "foo\n\n\nabc", true),
        ("foo\\n+abc", "foo\r\n\r\n\r\nabc", true),
        ("foo[\\n-9]+abc", "foo1abc", true),
    ]
    .into_iter()
    .for_each(test_fix_regex_newline);
}

#[test]
fn fix_newline_matching() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let test_fix_newline = |(input_reg, test_str, should_match): (&str, &str, bool)| {
        let fixed = fix_newline(&units(input_reg));
        assert_eq!(reg_exp_test(&fixed, test_str), should_match, "{input_reg} => {fixed}, {test_str:?}, {should_match}");
    };

    [
        ("foo", "foo", true),
        ("foo\n", "foo\r\n", true),
        ("foo\n", "foo\n", true),
        ("foo\nabc", "foo\r\nabc", true),
        ("foo\nabc", "foo\nabc", true),
        ("foo\r\n", "foo\r\n", true),
        ("foo\nbarc", "foobar", false),
        ("foobar", "foo\nbar", false),
    ]
    .into_iter()
    .for_each(test_fix_newline);
}

mod ripgrep_parser {
    use super::*;

    fn test_folder() -> URI {
        URI::file("/foo/bar", ProcessEnvironment.platform())
    }

    fn test_parser(input_data: &[&str], expected_results: Vec<TextSearchResult2>) {
        let mut test_parser = RipgrepParser::new(1000, test_folder(), DEFAULT_TEXT_SEARCH_PREVIEW_OPTIONS);

        let actual_results: Rc<RefCell<Vec<TextSearchResult2>>> = Rc::new(RefCell::new(Vec::new()));
        let sink = Rc::clone(&actual_results);
        test_parser.on_result(move |r| {
            sink.borrow_mut().push(r.clone());
        });

        for d in input_data {
            test_parser.handle_data(*d);
        }
        test_parser.flush();

        assert_eq!(*actual_results.borrow(), expected_results);
    }

    fn make_rg_match(relative_path: &str, text: &str, line_number: i64, match_ranges: &[(i64, i64)]) -> String {
        let message = IRgMessage {
            message_type: "match".to_owned(),
            data: IRgMatch {
                path: IRgBytesOrText::Text(relative_path.to_owned()),
                lines: IRgBytesOrText::Text(text.to_owned()),
                line_number,
                absolute_offset: 0, // unused
                submatches: match_ranges
                    .iter()
                    .map(|&(start, end)| IRgSubmatch {
                        start,
                        end,
                        match_: IRgBytesOrText::Text(text[start as usize..end as usize].to_owned()),
                    })
                    .collect(),
            },
        };
        serde_json::to_string(&message.to_json()).expect("JSON válido") + "\n"
    }

    fn expected_match(path: &str, ranges: &[(Range, Range)], preview_text: &str) -> TextSearchResult2 {
        TextSearchResult2::Match(TextSearchMatch2::new(
            join_path(&test_folder(), path),
            ranges
                .iter()
                .map(|&(preview_range, source_range)| TextSearchMatchRanges { source_range, preview_range })
                .collect(),
            preview_text,
        ))
    }

    #[test]
    fn single_result() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        test_parser(
            &[&make_rg_match("file1.js", "foobar", 4, &[(3, 6)])],
            vec![expected_match("file1.js", &[(Range::new(0, 3, 0, 6), Range::new(3, 3, 3, 6))], "foobar")],
        );
    }

    #[test]
    fn multiple_results() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        test_parser(
            &[
                &make_rg_match("file1.js", "foobar", 4, &[(3, 6)]),
                &make_rg_match("app/file2.js", "foobar", 4, &[(3, 6)]),
                &make_rg_match("app2/file3.js", "foobar", 4, &[(3, 6)]),
            ],
            vec![
                expected_match("file1.js", &[(Range::new(0, 3, 0, 6), Range::new(3, 3, 3, 6))], "foobar"),
                expected_match("app/file2.js", &[(Range::new(0, 3, 0, 6), Range::new(3, 3, 3, 6))], "foobar"),
                expected_match("app2/file3.js", &[(Range::new(0, 3, 0, 6), Range::new(3, 3, 3, 6))], "foobar"),
            ],
        );
    }

    #[test]
    fn chopped_up_input_chunks() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let data_strs = [
            make_rg_match("file1.js", "foo bar", 4, &[(3, 7)]),
            make_rg_match("app/file2.js", "foobar", 4, &[(3, 6)]),
            make_rg_match("app2/file3.js", "foobar", 4, &[(3, 6)]),
        ];

        let data_str0_space = data_strs[0].find(' ').expect("há um espaço no texto");
        let chunk_after_newline = format!("\n{}", &data_strs[2][..25]);
        test_parser(
            &[
                &data_strs[0][..data_str0_space + 1],
                &data_strs[0][data_str0_space + 1..],
                "\n",
                data_strs[1].trim(),
                &chunk_after_newline,
                &data_strs[2][25..],
            ],
            vec![
                expected_match("file1.js", &[(Range::new(0, 3, 0, 7), Range::new(3, 3, 3, 7))], "foo bar"),
                expected_match("app/file2.js", &[(Range::new(0, 3, 0, 6), Range::new(3, 3, 3, 6))], "foobar"),
                expected_match("app2/file3.js", &[(Range::new(0, 3, 0, 6), Range::new(3, 3, 3, 6))], "foobar"),
            ],
        );
    }

    #[test]
    fn empty_result_100569() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        test_parser(
            &[&make_rg_match("file1.js", "foobar", 4, &[]), &make_rg_match("file1.js", "", 5, &[])],
            vec![
                expected_match("file1.js", &[(Range::new(0, 0, 0, 1), Range::new(3, 0, 3, 1))], "foobar"),
                expected_match("file1.js", &[(Range::new(0, 0, 0, 0), Range::new(4, 0, 4, 0))], ""),
            ],
        );
    }

    #[test]
    fn multiple_submatches_without_newline_in_between_131507() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        test_parser(
            &[&make_rg_match("file1.js", "foobarbazquux", 4, &[(0, 4), (6, 10)])],
            vec![expected_match(
                "file1.js",
                &[
                    (Range::new(0, 0, 0, 4), Range::new(3, 0, 3, 4)),
                    (Range::new(0, 6, 0, 10), Range::new(3, 6, 3, 10)),
                ],
                "foobarbazquux",
            )],
        );
    }

    #[test]
    fn multiple_submatches_with_newline_in_between_131507() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        test_parser(
            &[&make_rg_match("file1.js", "foo\nbar\nbaz\nquux", 4, &[(0, 5), (8, 13)])],
            vec![expected_match(
                "file1.js",
                &[
                    (Range::new(0, 0, 1, 1), Range::new(3, 0, 4, 1)),
                    (Range::new(2, 0, 3, 1), Range::new(5, 0, 6, 1)),
                ],
                "foo\nbar\nbaz\nquux",
            )],
        );
    }
}

mod get_rg_args_suite {
    use super::*;

    #[test]
    fn simple_includes() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        // Só testa os argumentos que vêm de `includes`.
        fn test_get_rg_args(includes: &[&str], expected_from_includes: &[&str]) {
            let mut query = TextSearchQuery2::new("test");

            let mut options = RipgrepTextSearchOptions::new(TextSearchProviderFolderOptions {
                includes: includes.iter().map(|include| include.to_string()).collect(),
                excludes: Vec::new(),
                use_ignore_files: UseIgnoreFiles { local: false, global: false, parent: false },
                follow_symlinks: false,
                folder: URI::file("/some/folder", ProcessEnvironment.platform()),
                encoding: "utf8".to_owned(),
                ignore_glob_case: None,
            });
            options.max_results = Some(1000);
            let mut expected: Vec<&str> = vec!["--hidden", "--no-require-git", "--ignore-case"];
            expected.extend_from_slice(expected_from_includes);
            expected.extend_from_slice(&[
                "--no-ignore",
                "--crlf",
                "--fixed-strings",
                "--no-config",
                "--no-ignore-global",
                "--json",
                "--",
                "test",
                ".",
            ]);
            let result = get_rg_args(&mut query, &options);
            assert_eq!(result, expected);
        }

        test_get_rg_args(&["a/*", "b/*"], &["-g", "!*", "-g", "/a", "-g", "/a/*", "-g", "/b", "-g", "/b/*"]);
        test_get_rg_args(&["**/a/*", "b/*"], &["-g", "!*", "-g", "/b", "-g", "/b/*", "-g", "**/a/*"]);
        test_get_rg_args(&["**/a/*", "**/b/*"], &["-g", "**/a/*", "-g", "**/b/*"]);
        test_get_rg_args(
            &["foo/*bar/something/**"],
            &["-g", "!*", "-g", "/foo", "-g", "/foo/*bar", "-g", "/foo/*bar/something", "-g", "/foo/*bar/something/**"],
        );
    }
}

#[test]
fn brace_expansion_for_ripgrep() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let test_brace_expansion = |arg_glob: &str, expected_glob: &[&str]| {
        let result = perform_brace_expansion_for_ripgrep(arg_glob);
        assert_eq!(result, expected_glob);
    };

    test_brace_expansion("eep/{a,b}/test", &["eep/a/test", "eep/b/test"]);
    test_brace_expansion("eep/{a,b}/{c,d,e}", &["eep/a/c", "eep/a/d", "eep/a/e", "eep/b/c", "eep/b/d", "eep/b/e"]);
    test_brace_expansion("eep/{a,b}/\\{c,d,e}", &["eep/a/{c,d,e}", "eep/b/{c,d,e}"]);
    test_brace_expansion("eep/{a,b\\}/test", &["eep/{a,b}/test"]);
    test_brace_expansion("eep/{a,b\\\\}/test", &["eep/a/test", "eep/b\\\\/test"]);
    test_brace_expansion("eep/{a,b\\\\\\}/test", &["eep/{a,b\\\\}/test"]);
    test_brace_expansion("e\\{ep/{a,b}/test", &["e{ep/a/test", "e{ep/b/test"]);
    test_brace_expansion("eep/{a,\\b}/test", &["eep/a/test", "eep/\\b/test"]);
    test_brace_expansion("{a/*.*,b/*.*}", &["a/*.*", "b/*.*"]);
    test_brace_expansion("{{}", &["{{}"]);
    test_brace_expansion("aa{{}", &["aa{{}"]);
    test_brace_expansion("{b{}", &["{b{}"]);
    test_brace_expansion("{{}c", &["{{}c"]);
    test_brace_expansion("{{}}", &["{{}}"]);
    test_brace_expansion("\\{{}}", &["{}"]);
    test_brace_expansion("{}foo", &["foo"]);
    test_brace_expansion("bar{ }foo", &["bar foo"]);
    test_brace_expansion("{}", &[""]);
}
