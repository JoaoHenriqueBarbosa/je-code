//! Porte de `src/vs/workbench/services/search/test/common/replace.test.ts` (suíte `Replace Pattern test`),
//! caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` vira o `LeakGuard` criado no começo de cada caso.
//! - Os literais de objeto `{ pattern, isRegExp }` viram [`info`]; o `RegExp` literal vira
//!   `JsRegExp::from_str` (que guarda o `source`, como o `RegExp` do JavaScript).
//! - O `deepStrictEqual` entre `RegExp` compara `source` e flags ([`assert_same_reg_exp`]).
//! - O `getReplaceString(text)` sem o segundo argumento passa `preserveCase = false`.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::js_regexp::JsRegExp;
use je_base::js_string::JsString;
use je_workbench::services::search::replace::ReplacePattern;
use je_workbench::services::search::search::PatternInfo;

fn info(pattern: &str, is_reg_exp: bool) -> PatternInfo {
    PatternInfo { is_reg_exp: Some(is_reg_exp), ..PatternInfo::new(pattern) }
}

fn pattern_of(replace: &str, pattern: &str, is_reg_exp: bool) -> ReplacePattern {
    ReplacePattern::from_pattern_info(replace, &info(pattern, is_reg_exp)).unwrap()
}

fn reg_exp(source: &str, flags: &str) -> JsRegExp {
    JsRegExp::from_str(source, flags).unwrap()
}

/// `assert.deepStrictEqual` entre dois `RegExp`: mesmo `source` e mesmas flags.
fn assert_same_reg_exp(actual: &JsRegExp, expected: &JsRegExp) {
    assert_eq!(actual.source(), expected.source());
    assert_eq!(actual.flags(), expected.flags());
}

fn get(test_object: &ReplacePattern, text: &str, preserve_case: bool) -> Option<JsString> {
    test_object.get_replace_string(JsString::from(text).units(), preserve_case)
}

fn some(text: &str) -> Option<JsString> {
    Some(JsString::from(text))
}

#[test]
fn parse_replace_string() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    let test_parse = |input: &str, expected: &str, expected_has_parameters: bool| {
        let actual = pattern_of(input, "somepattern", true);
        assert_eq!(*actual.pattern(), expected);
        assert_eq!(expected_has_parameters, actual.has_parameters());

        let actual = pattern_of(&format!("hello{input}hi"), "sonepattern", true);
        assert_eq!(*actual.pattern(), format!("hello{expected}hi").as_str());
        assert_eq!(expected_has_parameters, actual.has_parameters());
    };

    // no backslash => no treatment
    test_parse("hello", "hello", false);

    // \t => TAB
    test_parse("\\thello", "\thello", false);

    // \n => LF
    test_parse("\\nhello", "\nhello", false);

    // \\t => \t
    test_parse("\\\\thello", "\\thello", false);

    // \\\t => \TAB
    test_parse("\\\\\\thello", "\\\thello", false);

    // \\\\t => \\t
    test_parse("\\\\\\\\thello", "\\\\thello", false);

    // \ at the end => no treatment
    test_parse("hello\\", "hello\\", false);

    // \ with unknown char => no treatment
    test_parse("hello\\x", "hello\\x", false);

    // \ with back reference => no treatment
    test_parse("hello\\0", "hello\\0", false);

    // $1 => no treatment
    test_parse("hello$1", "hello$1", true);
    // $2 => no treatment
    test_parse("hello$2", "hello$2", true);
    // $12 => no treatment
    test_parse("hello$12", "hello$12", true);
    // $99 => no treatment
    test_parse("hello$99", "hello$99", true);
    // $99a => no treatment
    test_parse("hello$99a", "hello$99a", true);
    // $100 => no treatment
    test_parse("hello$100", "hello$100", false);
    // $100a => no treatment
    test_parse("hello$100a", "hello$100a", false);
    // $10a0 => no treatment
    test_parse("hello$10a0", "hello$10a0", true);
    // $$ => no treatment
    test_parse("hello$$", "hello$$", false);
    // $$0 => no treatment
    test_parse("hello$$0", "hello$$0", false);

    // $0 => $&
    test_parse("hello$0", "hello$&", true);
    test_parse("hello$02", "hello$&2", true);

    test_parse("hello$`", "hello$`", true);
    test_parse("hello$'", "hello$'", true);
}

#[test]
fn create_pattern_by_passing_reg_exp() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    let expected = reg_exp("abc", "");
    let actual = ReplacePattern::from_reg_exp("hello", false, reg_exp("abc", "")).unwrap();
    assert_same_reg_exp(actual.reg_exp(), &expected);

    let expected = reg_exp("abc", "");
    let actual = ReplacePattern::from_reg_exp("hello", false, reg_exp("abc", "g")).unwrap();
    assert_same_reg_exp(actual.reg_exp(), &expected);

    let test_object = ReplacePattern::from_reg_exp("hello$0", false, reg_exp("abc", "g")).unwrap();
    assert!(!test_object.has_parameters());

    let test_object = ReplacePattern::from_reg_exp("hello$0", true, reg_exp("abc", "g")).unwrap();
    assert!(test_object.has_parameters());
}

#[test]
fn get_replace_string_if_given_text_is_a_complete_match() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    let mut test_object = pattern_of("hello", "bla", true);
    let mut actual = get(&test_object, "bla", false);
    assert_eq!(actual, some("hello"));

    test_object = pattern_of("hello", "bla", false);
    actual = get(&test_object, "bla", false);
    assert_eq!(actual, some("hello"));

    test_object = pattern_of("hello", "(bla)", true);
    actual = get(&test_object, "bla", false);
    assert_eq!(actual, some("hello"));

    test_object = pattern_of("hello$0", "(bla)", true);
    actual = get(&test_object, "bla", false);
    assert_eq!(actual, some("hellobla"));

    let require_pattern = "let\\s+(\\w+)\\s*=\\s*require\\s*\\(\\s*['\"]([\\w.\\-/]+)\\s*['\"]\\s*\\)\\s*";

    test_object = pattern_of("import * as $1 from '$2';", require_pattern, true);
    actual = get(&test_object, "let fs = require('fs')", false);
    assert_eq!(actual, some("import * as fs from 'fs';"));

    actual = get(&test_object, "let something = require('fs')", false);
    assert_eq!(actual, some("import * as something from 'fs';"));

    actual = get(&test_object, "let require('fs')", false);
    assert_eq!(actual, None);

    test_object = pattern_of("import * as $1 from '$1';", require_pattern, true);
    actual = get(&test_object, "let something = require('fs')", false);
    assert_eq!(actual, some("import * as something from 'something';"));

    test_object = pattern_of("import * as $2 from '$1';", require_pattern, true);
    actual = get(&test_object, "let something = require('fs')", false);
    assert_eq!(actual, some("import * as fs from 'something';"));

    test_object = pattern_of("import * as $0 from '$0';", require_pattern, true);
    actual = get(&test_object, "let something = require('fs');", false);
    assert_eq!(actual, some("import * as let something = require('fs') from 'let something = require('fs')';"));

    test_object = pattern_of("import * as $1 from '$2';", require_pattern, false);
    actual = get(&test_object, "let fs = require('fs');", false);
    assert_eq!(actual, None);

    test_object = pattern_of("cat$1", "for(.*)", true);
    actual = get(&test_object, "for ()", false);
    assert_eq!(actual, some("cat ()"));
}

#[test]
fn case_operations() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    let test_object = pattern_of("a\\u$1l\\u\\l\\U$2M$3n", "a(l)l(good)m(e)n", true);
    let actual = get(&test_object, "allgoodmen", false);
    assert_eq!(actual, some("aLlGoODMen"));
}

#[test]
fn case_operations_no_false_positive() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    let mut test_object = pattern_of("\\left $1", "(pattern)", true);
    let mut actual = get(&test_object, "pattern", false);
    assert_eq!(actual, some("\\left pattern"));

    test_object = pattern_of("\\hi \\left $1", "(pattern)", true);
    actual = get(&test_object, "pattern", false);
    assert_eq!(actual, some("\\hi \\left pattern"));

    test_object = pattern_of("\\left \\L$1", "PATT(ERN)", true);
    actual = get(&test_object, "PATTERN", false);
    assert_eq!(actual, some("\\left ern"));
}

#[test]
fn case_operations_and_newline() {
    // #140734
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    let test_object = pattern_of("$1\n\\U$2", "(multi)(line)", true);
    let actual = get(&test_object, "multiline", false);
    assert_eq!(actual, some("multi\nLINE"));
}

#[test]
fn get_replace_string_for_no_matches() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    let mut test_object = pattern_of("hello", "bla", true);
    let mut actual = get(&test_object, "foo", false);
    assert_eq!(actual, None);

    test_object = pattern_of("hello", "bla", false);
    actual = get(&test_object, "foo", false);
    assert_eq!(actual, None);
}

#[test]
fn get_replace_string_if_match_is_sub_string_of_the_text() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    let text = "this is a bla text";

    let mut test_object = pattern_of("hello", "bla", true);
    let mut actual = get(&test_object, text, false);
    assert_eq!(actual, some("hello"));

    test_object = pattern_of("hello", "bla", false);
    actual = get(&test_object, text, false);
    assert_eq!(actual, some("hello"));

    test_object = pattern_of("that", "this(?=.*bla)", true);
    actual = get(&test_object, text, false);
    assert_eq!(actual, some("that"));

    test_object = pattern_of("$1at", "(th)is(?=.*bla)", true);
    actual = get(&test_object, text, false);
    assert_eq!(actual, some("that"));

    test_object = pattern_of("$1e", "(th)is(?=.*bla)", true);
    actual = get(&test_object, text, false);
    assert_eq!(actual, some("the"));

    test_object = pattern_of("$1ere", "(th)is(?=.*bla)", true);
    actual = get(&test_object, text, false);
    assert_eq!(actual, some("there"));

    test_object = pattern_of("$1", "(th)is(?=.*bla)", true);
    actual = get(&test_object, text, false);
    assert_eq!(actual, some("th"));

    test_object = pattern_of("ma$1", "(th)is(?=.*bla)", true);
    actual = get(&test_object, text, false);
    assert_eq!(actual, some("math"));

    test_object = pattern_of("ma$1s", "(th)is(?=.*bla)", true);
    actual = get(&test_object, text, false);
    assert_eq!(actual, some("maths"));

    test_object = pattern_of("ma$1s", "(th)is(?=.*bla)", true);
    actual = get(&test_object, text, false);
    assert_eq!(actual, some("maths"));

    test_object = pattern_of("$0", "(th)is(?=.*bla)", true);
    actual = get(&test_object, text, false);
    assert_eq!(actual, some("this"));

    test_object = pattern_of("$0$1", "(th)is(?=.*bla)", true);
    actual = get(&test_object, text, false);
    assert_eq!(actual, some("thisth"));

    test_object = pattern_of("foo", "bla(?=\\stext$)", true);
    actual = get(&test_object, text, false);
    assert_eq!(actual, some("foo"));

    test_object = pattern_of("f$1", "b(la)(?=\\stext$)", true);
    actual = get(&test_object, text, false);
    assert_eq!(actual, some("fla"));

    test_object = pattern_of("f$0", "b(la)(?=\\stext$)", true);
    actual = get(&test_object, text, false);
    assert_eq!(actual, some("fbla"));

    test_object = pattern_of("$0ah", "b(la)(?=\\stext$)", true);
    actual = get(&test_object, text, false);
    assert_eq!(actual, some("blaah"));

    test_object = ReplacePattern::from_reg_exp("newrege$1", true, reg_exp("Testrege(\\w*)", "")).unwrap();
    actual = get(&test_object, "Testregex", true);
    assert_eq!(actual, some("Newregex"));

    test_object = ReplacePattern::from_reg_exp("newrege$1", true, reg_exp("TESTREGE(\\w*)", "")).unwrap();
    actual = get(&test_object, "TESTREGEX", true);
    assert_eq!(actual, some("NEWREGEX"));

    test_object = ReplacePattern::from_reg_exp("new_rege$1", true, reg_exp("Test_Rege(\\w*)", "")).unwrap();
    actual = get(&test_object, "Test_Regex", true);
    assert_eq!(actual, some("New_Regex"));

    test_object = ReplacePattern::from_reg_exp("new-rege$1", true, reg_exp("Test-Rege(\\w*)", "")).unwrap();
    actual = get(&test_object, "Test-Regex", true);
    assert_eq!(actual, some("New-Regex"));
}
