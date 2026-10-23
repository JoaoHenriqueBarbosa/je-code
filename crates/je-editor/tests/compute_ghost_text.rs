//! Porte de `src/vs/editor/contrib/inlineCompletions/test/browser/computeGhostText.test.ts` (suíte
//! `computeGhostText`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` vira o `LeakGuard` criado no começo de cada caso.
//! - `createTextModel(text)` é `create_text_model` de `tests/support/test_text_model.rs`.
//! - `getOutput` devolve `unknown`: ou o valor único (quando `prefix` e `subword` coincidem, incluindo os dois
//!   `undefined`) ou o objeto `{ prefix, subword }`. Aqui é o enum [`Output`], com `Single` e `Pair`.
//! - `undefined` é `None`; `render(cleanedText, true)` é `GhostText::render(&cleaned, true)`.
//! - `text.indexOf('[')` conta unidades UTF-16; os textos do teste são ASCII, então o índice de byte basta.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod support;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_editor::inline_completions::compute_ghost_text::{GhostTextMode, compute_ghost_text};
use je_editor::edits::text_edit::TextReplacement;
use je_editor::range::Range;
use support::test_text_model::create_text_model;

#[derive(Debug, PartialEq, Eq)]
enum Output {
    Single(Option<String>),
    Pair { prefix: Option<String>, subword: Option<String> },
}

fn single(value: &str) -> Output {
    Output::Single(Some(value.to_string()))
}

fn pair(prefix: Option<&str>, subword: Option<&str>) -> Output {
    Output::Pair { prefix: prefix.map(str::to_string), subword: subword.map(str::to_string) }
}

fn get_output(text: &str, suggestion: &str) -> Output {
    let range_start_offset = text.find('[').unwrap() as i64;
    let range_end_offset = text.find(']').unwrap() as i64 - 1;
    let cleaned_text = text.replacen('[', "", 1).replacen(']', "", 1);
    let temp_model = create_text_model(&cleaned_text);
    let range = Range::from_positions(
        temp_model.get_position_at(range_start_offset),
        temp_model.get_position_at(range_end_offset),
    );
    let render = |mode: GhostTextMode| {
        compute_ghost_text(&TextReplacement::new(range, suggestion), &temp_model, mode, None, 0)
            .map(|ghost_text| ghost_text.render(&cleaned_text, true))
    };
    let prefix = render(GhostTextMode::Prefix);
    let subword = render(GhostTextMode::Subword);

    temp_model.dispose();

    if prefix == subword {
        return Output::Single(prefix);
    }

    Output::Pair { prefix, subword }
}

#[test]
fn basic() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(get_output("[foo]baz", "foobar"), single("foo[bar]baz"));
    assert_eq!(get_output("[aaa]aaa", "aaaaaa"), single("aaa[aaa]aaa"));
    assert_eq!(get_output("[foo]baz", "boobar"), Output::Single(None));
    assert_eq!(get_output("[foo]foo", "foofoo"), single("foo[foo]foo"));
    assert_eq!(get_output("foo[]", "bar\nhello"), single("foo[bar\nhello]"));
}

#[test]
fn empty_ghost_text() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(get_output("[foo]", "foo"), single("foo"));
}

#[test]
fn whitespace_indentation() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(get_output("[ foo]", "foobar"), single(" foo[bar]"));
    assert_eq!(get_output("[\tfoo]", "foobar"), single("\tfoo[bar]"));
    assert_eq!(get_output("[\t foo]", "\tfoobar"), single("\t foo[bar]"));
    assert_eq!(get_output("[\tfoo]", "\t\tfoobar"), pair(None, Some("\t[\t]foo[bar]")));
    assert_eq!(get_output("[\t]", "\t\tfoobar"), single("\t[\tfoobar]"));
    assert_eq!(get_output("\t[]", "\t"), single("\t[\t]"));
    assert_eq!(get_output("\t[\t]", ""), single("\t\t"));

    assert_eq!(get_output("[ ]", "return 1"), single(" [return 1]"));
}

#[test]
fn whitespace_outside_of_indentation() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(get_output("bar[ foo]", "foobar"), Output::Single(None));
    assert_eq!(get_output("bar[\tfoo]", "foobar"), Output::Single(None));
}

#[test]
fn unsupported_case() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(get_output("fo[o\n]", "x\nbar"), Output::Single(None));
}

#[test]
fn new_line() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(get_output("fo[o\n]", "o\nbar"), single("foo\n[bar]"));
}

#[test]
fn multi_part_diffing() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(get_output("foo[()]", "(x);"), pair(None, Some("foo([x])[;]")));
    assert_eq!(get_output("[\tfoo]", "\t\tfoobar"), pair(None, Some("\t[\t]foo[bar]")));
    assert_eq!(
        get_output("[(y ===)]", "(y === 1) { f(); }"),
        pair(None, Some("(y ===[ 1])[ { f(); }]"))
    );
    assert_eq!(
        get_output("[(y ==)]", "(y === 1) { f(); }"),
        pair(None, Some("(y ==[= 1])[ { f(); }]"))
    );

    assert_eq!(
        get_output("[(y ==)]", "(y === 1) { f(); }"),
        pair(None, Some("(y ==[= 1])[ { f(); }]"))
    );
}

#[test]
fn multi_part_diffing_1() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(get_output("[if () ()]", "if (1 == f()) ()"), pair(None, Some("if ([1 == f()]) ()")));
}

#[test]
fn multi_part_diffing_2() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(get_output("[)]", "())"), pair(None, Some("[(])[)]")));
    assert_eq!(get_output("[))]", "(())"), pair(None, Some("[((]))")));
}

#[test]
fn parenthesis_matching() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(
        get_output("[console.log()]", "console.log({ label: \"(\" })"),
        pair(None, Some("console.log([{ label: \"(\" }])"))
    );
}
