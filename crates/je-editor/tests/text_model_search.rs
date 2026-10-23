//! Porte de `src/vs/editor/test/common/model/textModelSearch.test.ts` (suíte `TextModelSearch`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `assert.deepStrictEqual` entre `RegExp` compara `source` e `flags`; aqui a comparação é feita
//!   campo a campo sobre o `JsRegExp`.
//! - `ensureNoDisposablesAreLeakedInTestSuite` vem do `common` do `je-base`, incluído por `#[path]`.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod support;

use je_editor::cursor_common::CursorSimpleModel;
use std::rc::Rc;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::JsString;
use je_editor::model::{EndOfLineSequence, Eol, FindMatch};
use je_editor::position::Position;
use je_editor::range::Range;
use je_editor::text_model::TextModel;
use je_editor::text_model_search::{SearchParams, TextModelSearch, is_multiline_reg_exp_source};
use je_editor::word_helper::USUAL_WORD_SEPARATORS;
use support::test_text_model::*;

fn find_match(range: Range, matches: Option<&[&str]>) -> FindMatch {
    FindMatch {
        range,
        matches: matches.map(|matches| matches.iter().map(|m| Some(JsString::from(*m))).collect()),
    }
}

fn assert_find_match(actual: Option<FindMatch>, expected_range: Range, expected_matches: Option<&[&str]>) {
    assert_eq!(actual, Some(find_match(expected_range, expected_matches)));
}

fn assert_find_matches_in(model: &TextModel, search_params: &SearchParams, expected_matches: &[FindMatch]) {
    let actual = TextModelSearch::find_matches(model, search_params, &model.get_full_model_range(), false, 1000);
    assert_eq!(actual, expected_matches, "findMatches OK");

    // Testa `findNextMatch`.
    let mut start_pos = Position::new(1, 1);
    let mut m = TextModelSearch::find_next_match(model, search_params, start_pos, false);
    assert_eq!(m.as_ref(), expected_matches.first(), "findNextMatch {start_pos}");
    for expected_match in expected_matches {
        start_pos = expected_match.range.get_start_position();
        m = TextModelSearch::find_next_match(model, search_params, start_pos, false);
        assert_eq!(m.as_ref(), Some(expected_match), "findNextMatch {start_pos}");
    }

    // Testa `findPrevMatch`.
    start_pos = Position::new(model.get_line_count(), model.get_line_max_column(model.get_line_count()));
    m = TextModelSearch::find_previous_match(model, search_params, start_pos, false);
    assert_eq!(m.as_ref(), expected_matches.last(), "findPrevMatch {start_pos}");
    for expected_match in expected_matches {
        start_pos = expected_match.range.get_end_position();
        m = TextModelSearch::find_previous_match(model, search_params, start_pos, false);
        assert_eq!(m.as_ref(), Some(expected_match), "findPrevMatch {start_pos}");
    }
}

fn assert_find_matches(
    text: &str,
    search_string: &str,
    is_regex: bool,
    match_case: bool,
    word_separators: Option<&str>,
    expected: &[(i32, i32, i32, i32)],
) {
    let expected_matches: Vec<FindMatch> = expected
        .iter()
        .map(|&(a, b, c, d)| find_match(Range::new(a, b, c, d), None))
        .collect();
    let search_params = SearchParams::new(JsString::from(search_string), is_regex, match_case, word_separators);

    let model = create_text_model(text);
    assert_find_matches_in(&model, &search_params, &expected_matches);
    model.dispose();

    let model2 = create_text_model(text);
    model2.set_eol(EndOfLineSequence::CrLf);
    assert_find_matches_in(&model2, &search_params, &expected_matches);
    model2.dispose();
}

const REGULAR_TEXT: [&str; 5] = [
    "This is some foo - bar text which contains foo and bar - as in Barcelona.",
    "Now it begins a word fooBar and now it is caps Foo-isn't this great?",
    "And here's a dull line with nothing interesting in it",
    "It is also interesting if it's part of a word like amazingFooBar",
    "Again nothing interesting here",
];

fn regular_text() -> String {
    REGULAR_TEXT.join("\n")
}

#[test]
fn simple_find() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    assert_find_matches(
        &regular_text(),
        "foo",
        false,
        false,
        None,
        &[(1, 14, 1, 17), (1, 44, 1, 47), (2, 22, 2, 25), (2, 48, 2, 51), (4, 59, 4, 62)],
    );
}

#[test]
fn case_sensitive_find() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    assert_find_matches(&regular_text(), "foo", false, true, None, &[(1, 14, 1, 17), (1, 44, 1, 47), (2, 22, 2, 25)]);
}

#[test]
fn whole_words_find() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    assert_find_matches(
        &regular_text(),
        "foo",
        false,
        false,
        Some(USUAL_WORD_SEPARATORS),
        &[(1, 14, 1, 17), (1, 44, 1, 47), (2, 48, 2, 51)],
    );
}

#[test]
fn caret_find() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    assert_find_matches(
        &regular_text(),
        "^",
        true,
        false,
        None,
        &[(1, 1, 1, 1), (2, 1, 2, 1), (3, 1, 3, 1), (4, 1, 4, 1), (5, 1, 5, 1)],
    );
}

#[test]
fn dollar_find() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    assert_find_matches(
        &regular_text(),
        "$",
        true,
        false,
        None,
        &[(1, 74, 1, 74), (2, 69, 2, 69), (3, 54, 3, 54), (4, 65, 4, 65), (5, 31, 5, 31)],
    );
}

#[test]
fn dot_star_find() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    assert_find_matches(
        &regular_text(),
        ".*",
        true,
        false,
        None,
        &[(1, 1, 1, 74), (2, 1, 2, 69), (3, 1, 3, 54), (4, 1, 4, 65), (5, 1, 5, 31)],
    );
}

#[test]
fn caret_dollar_find() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    assert_find_matches(
        &[
            "This is some foo - bar text which contains foo and bar - as in Barcelona.",
            "",
            "And here's a dull line with nothing interesting in it",
            "",
            "Again nothing interesting here",
        ]
        .join("\n"),
        "^$",
        true,
        false,
        None,
        &[(2, 1, 2, 1), (4, 1, 4, 1)],
    );
}

const MULTILINE_TEXT: [&str; 4] = ["Just some text text", "Just some text text", "some text again", "again some text"];

#[test]
fn multiline_find_1() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    assert_find_matches(&MULTILINE_TEXT.join("\n"), r"text\n", true, false, None, &[(1, 16, 2, 1), (2, 16, 3, 1)]);
}

#[test]
fn multiline_find_2() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    assert_find_matches(&MULTILINE_TEXT.join("\n"), r"text\nJust", true, false, None, &[(1, 16, 2, 5)]);
}

#[test]
fn multiline_find_3() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    assert_find_matches(&MULTILINE_TEXT.join("\n"), r"\nagain", true, false, None, &[(3, 16, 4, 6)]);
}

#[test]
fn multiline_find_4() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    assert_find_matches(&MULTILINE_TEXT.join("\n"), r".*\nJust.*\n", true, false, None, &[(1, 1, 3, 1)]);
}

#[test]
fn multiline_find_with_line_beginning_regex() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    assert_find_matches(
        &["if", "else", "", "if", "else"].join("\n"),
        r"^if\nelse",
        true,
        false,
        None,
        &[(1, 1, 2, 5), (4, 1, 5, 5)],
    );
}

#[test]
fn matching_empty_lines_using_boundary_expression() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    assert_find_matches(
        &["if", "", "else", "  ", "if", " ", "else"].join("\n"),
        r"^\s*$\n",
        true,
        false,
        None,
        &[(2, 1, 3, 1), (4, 1, 5, 1), (6, 1, 7, 1)],
    );
}

#[test]
fn matching_lines_starting_with_a_and_ending_with_b() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    assert_find_matches(&["a if b", "a", "ab", "eb"].join("\n"), "^a.*b$", true, false, None, &[(1, 1, 1, 7), (3, 1, 3, 3)]);
}

#[test]
fn multiline_find_with_line_ending_regex() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    assert_find_matches(
        &["if", "else", "", "if", "elseif", "else"].join("\n"),
        r"if\nelse$",
        true,
        false,
        None,
        &[(1, 1, 2, 5), (5, 5, 6, 5)],
    );
}

#[test]
fn issue_4836_caret_dot_star_dollar() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    assert_find_matches(
        &["Just some text text", "", "some text again", "", "again some text"].join("\n"),
        "^.*$",
        true,
        false,
        None,
        &[(1, 1, 1, 20), (2, 1, 2, 1), (3, 1, 3, 16), (4, 1, 4, 1), (5, 1, 5, 16)],
    );
}

#[test]
fn multiline_find_for_non_regex_string() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    assert_find_matches(
        &["Just some text text", "some text text", "some text again", "again some text", "but not some"].join("\n"),
        "text\nsome",
        false,
        false,
        None,
        &[(1, 16, 2, 5), (2, 11, 3, 5)],
    );
}

#[test]
fn issue_3623_match_whole_word_does_not_work_for_not_latin_characters() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    assert_find_matches(
        &["я", "компилятор", "обфускация", ":я-я"].join("\n"),
        "я",
        false,
        false,
        Some(USUAL_WORD_SEPARATORS),
        &[(1, 1, 1, 2), (4, 2, 4, 3), (4, 4, 4, 5)],
    );
}

#[test]
fn issue_27459_match_whole_words_regression() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    assert_find_matches(
        &[
            "this._register(this._textAreaInput.onKeyDown((e: IKeyboardEvent) => {",
            "\tthis._viewController.emitKeyDown(e);",
            "}));",
        ]
        .join("\n"),
        "((e: ",
        false,
        false,
        Some(USUAL_WORD_SEPARATORS),
        &[(1, 45, 1, 50)],
    );
}

#[test]
fn issue_27594_search_results_disappear() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    assert_find_matches("this.server.listen(0);", "listen(", false, false, Some(USUAL_WORD_SEPARATORS), &[(1, 13, 1, 20)]);
}

/// Encadeia `findNextMatch`: cada passo parte de `start` ou do fim do resultado anterior.
fn end_of(actual: &Option<FindMatch>) -> Position {
    actual.as_ref().expect("houve resultado").range.get_end_position()
}

#[test]
fn find_next_match_without_regex() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let model = create_text_model("line line one\nline two\nthree");

    let search_params = SearchParams::new(JsString::from("line"), false, false, None);

    let mut actual = TextModelSearch::find_next_match(&model, &search_params, Position::new(1, 1), false);
    assert_find_match(actual.clone(), Range::new(1, 1, 1, 5), None);

    actual = TextModelSearch::find_next_match(&model, &search_params, end_of(&actual), false);
    assert_find_match(actual.clone(), Range::new(1, 6, 1, 10), None);

    actual = TextModelSearch::find_next_match(&model, &search_params, Position::new(1, 3), false);
    assert_find_match(actual.clone(), Range::new(1, 6, 1, 10), None);

    actual = TextModelSearch::find_next_match(&model, &search_params, end_of(&actual), false);
    assert_find_match(actual.clone(), Range::new(2, 1, 2, 5), None);

    actual = TextModelSearch::find_next_match(&model, &search_params, end_of(&actual), false);
    assert_find_match(actual, Range::new(1, 1, 1, 5), None);

    model.dispose();
}

/// O corpo comum dos dois casos de `^line` em linha única: só o texto do modelo muda.
fn assert_find_next_match_with_beginning_boundary_regex(text: &str) {
    let model = create_text_model(text);

    let search_params = SearchParams::new(JsString::from("^line"), true, false, None);

    let mut actual = TextModelSearch::find_next_match(&model, &search_params, Position::new(1, 1), false);
    assert_find_match(actual.clone(), Range::new(1, 1, 1, 5), None);

    actual = TextModelSearch::find_next_match(&model, &search_params, end_of(&actual), false);
    assert_find_match(actual.clone(), Range::new(2, 1, 2, 5), None);

    actual = TextModelSearch::find_next_match(&model, &search_params, Position::new(1, 3), false);
    assert_find_match(actual.clone(), Range::new(2, 1, 2, 5), None);

    actual = TextModelSearch::find_next_match(&model, &search_params, end_of(&actual), false);
    assert_find_match(actual, Range::new(1, 1, 1, 5), None);

    model.dispose();
}

#[test]
fn find_next_match_with_beginning_boundary_regex() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    assert_find_next_match_with_beginning_boundary_regex("line one\nline two\nthree");
}

#[test]
fn find_next_match_with_beginning_boundary_regex_and_line_has_repetitive_beginnings() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    assert_find_next_match_with_beginning_boundary_regex("line line one\nline two\nthree");
}

#[test]
fn find_next_match_with_beginning_boundary_multiline_regex_and_line_has_repetitive_beginnings() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let model = create_text_model("line line one\nline two\nline three\nline four");

    let search_params = SearchParams::new(JsString::from(r"^line.*\nline"), true, false, None);

    let mut actual = TextModelSearch::find_next_match(&model, &search_params, Position::new(1, 1), false);
    assert_find_match(actual.clone(), Range::new(1, 1, 2, 5), None);

    actual = TextModelSearch::find_next_match(&model, &search_params, end_of(&actual), false);
    assert_find_match(actual, Range::new(3, 1, 4, 5), None);

    let actual = TextModelSearch::find_next_match(&model, &search_params, Position::new(2, 1), false);
    assert_find_match(actual, Range::new(2, 1, 3, 5), None);

    model.dispose();
}

#[test]
fn find_next_match_with_ending_boundary_regex() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let model = create_text_model("one line line\ntwo line\nthree");

    let search_params = SearchParams::new(JsString::from("line$"), true, false, None);

    let mut actual = TextModelSearch::find_next_match(&model, &search_params, Position::new(1, 1), false);
    assert_find_match(actual.clone(), Range::new(1, 10, 1, 14), None);

    actual = TextModelSearch::find_next_match(&model, &search_params, Position::new(1, 4), false);
    assert_find_match(actual.clone(), Range::new(1, 10, 1, 14), None);

    actual = TextModelSearch::find_next_match(&model, &search_params, end_of(&actual), false);
    assert_find_match(actual.clone(), Range::new(2, 5, 2, 9), None);

    actual = TextModelSearch::find_next_match(&model, &search_params, end_of(&actual), false);
    assert_find_match(actual, Range::new(1, 10, 1, 14), None);

    model.dispose();
}

const LINE_CAPTURES: &[&str] = &["line", "line", "in"];
const LINE_FEED_CAPTURES: &[&str] = &["line\n", "line", "in"];

#[test]
fn find_matches_with_capturing_matches() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let model = create_text_model("one line line\ntwo line\nthree");

    let search_params = SearchParams::new(JsString::from("(l(in)e)"), true, false, None);

    let actual = TextModelSearch::find_matches(&model, &search_params, &model.get_full_model_range(), true, 100);
    assert_eq!(
        actual,
        vec![
            find_match(Range::new(1, 5, 1, 9), Some(LINE_CAPTURES)),
            find_match(Range::new(1, 10, 1, 14), Some(LINE_CAPTURES)),
            find_match(Range::new(2, 5, 2, 9), Some(LINE_CAPTURES)),
        ]
    );

    model.dispose();
}

#[test]
fn find_matches_multiline_with_capturing_matches() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let model = create_text_model("one line line\ntwo line\nthree");

    let search_params = SearchParams::new(JsString::from(r"(l(in)e)\n"), true, false, None);

    let actual = TextModelSearch::find_matches(&model, &search_params, &model.get_full_model_range(), true, 100);
    assert_eq!(
        actual,
        vec![
            find_match(Range::new(1, 10, 2, 1), Some(LINE_FEED_CAPTURES)),
            find_match(Range::new(2, 5, 3, 1), Some(LINE_FEED_CAPTURES)),
        ]
    );

    model.dispose();
}

#[test]
fn find_next_match_with_capturing_matches() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let model = create_text_model("one line line\ntwo line\nthree");

    let search_params = SearchParams::new(JsString::from("(l(in)e)"), true, false, None);

    let actual = TextModelSearch::find_next_match(&model, &search_params, Position::new(1, 1), true);
    assert_find_match(actual, Range::new(1, 5, 1, 9), Some(LINE_CAPTURES));

    model.dispose();
}

#[test]
fn find_next_match_multiline_with_capturing_matches() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let model = create_text_model("one line line\ntwo line\nthree");

    let search_params = SearchParams::new(JsString::from(r"(l(in)e)\n"), true, false, None);

    let actual = TextModelSearch::find_next_match(&model, &search_params, Position::new(1, 1), true);
    assert_find_match(actual, Range::new(1, 10, 2, 1), Some(LINE_FEED_CAPTURES));

    model.dispose();
}

#[test]
fn find_previous_match_with_capturing_matches() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let model = create_text_model("one line line\ntwo line\nthree");

    let search_params = SearchParams::new(JsString::from("(l(in)e)"), true, false, None);

    let actual = TextModelSearch::find_previous_match(&model, &search_params, Position::new(1, 1), true);
    assert_find_match(actual, Range::new(2, 5, 2, 9), Some(LINE_CAPTURES));

    model.dispose();
}

#[test]
fn find_previous_match_multiline_with_capturing_matches() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let model = create_text_model("one line line\ntwo line\nthree");

    let search_params = SearchParams::new(JsString::from(r"(l(in)e)\n"), true, false, None);

    let actual = TextModelSearch::find_previous_match(&model, &search_params, Position::new(1, 1), true);
    assert_find_match(actual, Range::new(2, 5, 3, 1), Some(LINE_FEED_CAPTURES));

    model.dispose();
}

#[test]
fn backslash_n_matches_backslash_r_backslash_n() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let model: Rc<TextModel> = create_text_model("a\r\nb\r\nc\r\nd\r\ne\r\nf\r\ng\r\nh\r\ni");

    assert_eq!(model.get_eol(), Eol::CrLf);

    let first_match = |search_string: &str| {
        let search_params = SearchParams::new(JsString::from(search_string), true, false, None);
        // O original chama `findNextMatch` e descarta o resultado antes do `findMatches`.
        let _ = TextModelSearch::find_next_match(&model, &search_params, Position::new(1, 1), true);
        TextModelSearch::find_matches(&model, &search_params, &model.get_full_model_range(), true, 1000)
            .into_iter()
            .next()
    };

    assert_find_match(first_match(r"h\n"), Range::new(8, 1, 9, 1), Some(&["h\n"]));
    assert_find_match(first_match(r"g\nh\n"), Range::new(7, 1, 9, 1), Some(&["g\nh\n"]));
    assert_find_match(first_match(r"\ni"), Range::new(8, 2, 9, 2), Some(&["\ni"]));

    model.dispose();
}

#[test]
fn backslash_r_can_never_be_found() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let model = create_text_model("a\r\nb\r\nc\r\nd\r\ne\r\nf\r\ng\r\nh\r\ni");

    assert_eq!(model.get_eol(), Eol::CrLf);

    let search_params = SearchParams::new(JsString::from(r"\r\n"), true, false, None);
    let actual = TextModelSearch::find_next_match(&model, &search_params, Position::new(1, 1), true);
    assert_eq!(actual, None);
    assert_eq!(
        TextModelSearch::find_matches(&model, &search_params, &model.get_full_model_range(), true, 1000),
        vec![]
    );

    model.dispose();
}

/// O `SearchData` esperado: `source` e `flags` da regex e o `simpleSearch`.
struct ExpectedSearchData<'a> {
    source: &'a str,
    flags: &'a str,
    simple_search: Option<&'a str>,
}

fn expected<'a>(source: &'a str, flags: &'a str, simple_search: Option<&'a str>) -> Option<ExpectedSearchData<'a>> {
    Some(ExpectedSearchData {
        source,
        flags,
        simple_search,
    })
}

fn assert_parse_search_result(
    search_string: &str,
    is_regex: bool,
    match_case: bool,
    word_separators: Option<&str>,
    expected: Option<ExpectedSearchData<'_>>,
) {
    let search_params = SearchParams::new(JsString::from(search_string), is_regex, match_case, word_separators);
    let actual = search_params.parse_search_request();

    match expected {
        None => assert!(actual.is_none()),
        Some(expected) => {
            let actual = actual.expect("parseSearchRequest devolveu null");
            assert_eq!(JsString::from(actual.regex.source()), JsString::from(expected.source));
            assert_eq!(actual.regex.flags().to_flags_string(), expected.flags);
            assert_eq!(actual.simple_search, expected.simple_search.map(JsString::from));
            assert_eq!(actual.word_separators.is_some(), word_separators.is_some());
        }
    }
}

#[test]
fn parse_search_request_invalid() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    assert_parse_search_result("", true, true, Some(USUAL_WORD_SEPARATORS), None);
    assert_parse_search_result("(", true, false, None, None);
}

#[test]
fn parse_search_request_non_regex() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let usual = Some(USUAL_WORD_SEPARATORS);
    assert_parse_search_result("foo", false, false, None, expected("foo", "giu", None));
    assert_parse_search_result("foo", false, false, usual, expected("foo", "giu", None));
    assert_parse_search_result("foo", false, true, None, expected("foo", "gu", Some("foo")));
    assert_parse_search_result("foo", false, true, usual, expected("foo", "gu", Some("foo")));
    assert_parse_search_result(r"foo\n", false, false, None, expected(r"foo\\n", "giu", None));
    assert_parse_search_result(r"foo\\n", false, false, None, expected(r"foo\\\\n", "giu", None));
    assert_parse_search_result(r"foo\r", false, false, None, expected(r"foo\\r", "giu", None));
    assert_parse_search_result(r"foo\\r", false, false, None, expected(r"foo\\\\r", "giu", None));
}

#[test]
fn parse_search_request_regex() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let usual = Some(USUAL_WORD_SEPARATORS);
    assert_parse_search_result("foo", true, false, None, expected("foo", "giu", None));
    assert_parse_search_result("foo", true, false, usual, expected("foo", "giu", None));
    assert_parse_search_result("foo", true, true, None, expected("foo", "gu", None));
    assert_parse_search_result("foo", true, true, usual, expected("foo", "gu", None));
    assert_parse_search_result(r"foo\n", true, false, None, expected(r"foo\n", "gimu", None));
    assert_parse_search_result(r"foo\\n", true, false, None, expected(r"foo\\n", "giu", None));
    assert_parse_search_result(r"foo\r", true, false, None, expected(r"foo\r", "gimu", None));
    assert_parse_search_result(r"foo\\r", true, false, None, expected(r"foo\\r", "giu", None));
}

#[test]
fn issue_53415_w_should_match_line_break() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    assert_find_matches(&["text", "180702-", "180703-180704"].join("\n"), r"\d{6}-\W", true, false, None, &[(2, 1, 3, 1)]);

    assert_find_matches(
        &["Just some text", "", "Just"].join("\n"),
        r"\W",
        true,
        false,
        None,
        &[(1, 5, 1, 6), (1, 10, 1, 11), (1, 15, 2, 1), (2, 1, 3, 1)],
    );

    // A quebra de linha não muda o resultado, porque a busca sempre usa `\n` como quebra de linha.
    assert_find_matches(
        &["Just some text", "", "Just"].join("\r\n"),
        r"\W",
        true,
        false,
        None,
        &[(1, 5, 1, 6), (1, 10, 1, 11), (1, 15, 2, 1), (2, 1, 3, 1)],
    );

    assert_find_matches(
        &["Just some text", "\tJust", "Just"].join("\n"),
        r"\W",
        true,
        false,
        None,
        &[(1, 5, 1, 6), (1, 10, 1, 11), (1, 15, 2, 1), (2, 1, 2, 2), (2, 6, 3, 1)],
    );

    // A quebra de linha conta como um caractere que não é de palavra.
    assert_find_matches(
        &["Just  some text", "", "Just"].join("\n"),
        r"\W{2}",
        true,
        false,
        None,
        &[(1, 5, 1, 7), (1, 16, 3, 1)],
    );

    // Mesmo se for `\r\n`.
    assert_find_matches(
        &["Just  some text", "", "Just"].join("\r\n"),
        r"\W{2}",
        true,
        false,
        None,
        &[(1, 5, 1, 7), (1, 16, 3, 1)],
    );
}

#[test]
fn simple_find_using_unicode_escape_sequences() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    assert_find_matches(
        &regular_text(),
        r"\u{0066}oo",
        true,
        false,
        None,
        &[(1, 14, 1, 17), (1, 44, 1, 47), (2, 22, 2, 25), (2, 48, 2, 51), (4, 59, 4, 62)],
    );
}

fn is_multiline(search_string: &str) -> bool {
    is_multiline_reg_exp_source(JsString::from(search_string).units())
}

#[test]
fn is_multiline_regex_source() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    assert!(!is_multiline("foo"));
    assert!(!is_multiline(""));
    assert!(!is_multiline(r"foo\sbar"));
    assert!(!is_multiline(r"\\notnewline"));

    assert!(is_multiline(r"foo\nbar"));
    assert!(is_multiline(r"foo\nbar\s"));
    assert!(is_multiline(r"foo\r\n"));
    assert!(is_multiline(r"\n"));
    assert!(is_multiline(r"foo\W"));
    assert!(is_multiline("foo\n"));
    assert!(is_multiline("foo\r\n"));
}

#[test]
fn is_multiline_regex_source_correctly_identifies_multiline_patterns() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let single_line_patterns = [r"MARK:\s*(?<label>.*)$", "^// Header$", r"\s*[-=]+\s*"];

    let multi_line_patterns = [
        r"^// =+\n^// (?<label>[^\n]+?)\n^// =+$",
        r"header\r\nfooter",
        r"start\r|\nend",
        "top\nmiddle\r\nbottom",
    ];

    for pattern in single_line_patterns {
        assert!(!is_multiline(pattern), "Pattern should not be multiline: {pattern}");
    }

    for pattern in multi_line_patterns {
        assert!(is_multiline(pattern), "Pattern should be multiline: {pattern}");
    }
}

#[test]
fn issue_74715_d_star_finds_empty_string_and_stops_searching() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let model = create_text_model("10.243.30.10");

    let search_params = SearchParams::new(JsString::from(r"\d*"), true, false, None);

    let actual = TextModelSearch::find_matches(&model, &search_params, &model.get_full_model_range(), true, 100);
    assert_eq!(
        actual,
        vec![
            find_match(Range::new(1, 1, 1, 3), Some(&["10"])),
            find_match(Range::new(1, 3, 1, 3), Some(&[""])),
            find_match(Range::new(1, 4, 1, 7), Some(&["243"])),
            find_match(Range::new(1, 7, 1, 7), Some(&[""])),
            find_match(Range::new(1, 8, 1, 10), Some(&["30"])),
            find_match(Range::new(1, 10, 1, 10), Some(&[""])),
            find_match(Range::new(1, 11, 1, 13), Some(&["10"])),
        ]
    );

    model.dispose();
}

#[test]
fn issue_100134_zero_length_matches_should_properly_step_over_surrogate_pairs() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    // 1[Laptop]1: não deve haver casamento dentro do emoji [Laptop].
    assert_find_matches(
        "1\u{1F4BB}1",
        "()",
        true,
        false,
        None,
        &[(1, 1, 1, 1), (1, 2, 1, 2), (1, 4, 1, 4), (1, 5, 1, 5)],
    );
    // 1[Hacker Cat]1 = 1[Cat Face][ZWJ][Laptop]1: deve haver casamento entre os emojis e o ZWJ, e
    // nenhum dentro de [Cat Face] nem de [Laptop].
    assert_find_matches(
        "1\u{1F431}\u{200D}\u{1F4BB}1",
        "()",
        true,
        false,
        None,
        &[(1, 1, 1, 1), (1, 2, 1, 2), (1, 4, 1, 4), (1, 5, 1, 5), (1, 7, 1, 7), (1, 8, 1, 8)],
    );
}
