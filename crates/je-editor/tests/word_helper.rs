//! Casos próprios (sem equivalente upstream), conferidos no Node real contra o `wordHelper.ts`.
//!
//! Conferência: o `wordHelper.ts` copiado para um diretório temporário, com os três imports de
//! `base/common` (`Iterable.first`, `toDisposable`, `LinkedList`) trocados por equivalentes de uma linha,
//! e rodado com `node --experimental-strip-types` (Node 26). Cada valor esperado abaixo é a saída de lá.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use std::rc::Rc;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::JsString;
use je_base::lifecycle::IDisposable;
use je_base::js_regexp::JsRegExp;
use je_editor::word_helper::{
    GetWordAtTextConfig, WordAtPosition, create_word_reg_exp, default_word_regexp, ensure_valid_word_definition,
    get_word_at_text, set_default_get_word_at_text_config,
};

fn js(text: &str) -> JsString {
    JsString::from(text)
}

fn word(word: &str, start_column: i32, end_column: i32) -> Option<WordAtPosition> {
    Some(WordAtPosition { word: js(word), start_column, end_column })
}

fn at(column: i32, text: &str) -> Option<WordAtPosition> {
    get_word_at_text(column, &default_word_regexp(), js(text).units(), 0, None)
}

#[test]
fn default_word_regexp_source() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let expected = r#"(-?\d*\.\d\w*)|([^\`\~\!\@\#\$\%\^\&\*\(\)\-\=\+\[\{\]\}\\\|\;\:\'\"\,\.\<\>\/\?\s]+)"#;
    assert_eq!(default_word_regexp().source(), js(expected).units());
    assert!(default_word_regexp().flags().global);
    assert!(Rc::ptr_eq(&default_word_regexp(), &default_word_regexp()));
}

#[test]
fn words_in_plain_text() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(at(1, "hello world"), word("hello", 1, 6));
    assert_eq!(at(3, "hello world"), word("hello", 1, 6));
    assert_eq!(at(6, "hello world"), word("hello", 1, 6));
    assert_eq!(at(7, "hello world"), word("world", 7, 12));
    assert_eq!(at(12, "hello world"), word("world", 7, 12));
    assert_eq!(at(5, "foo.bar(baz)"), word("bar", 5, 8));
    assert_eq!(at(9, "foo.bar(baz)"), word("baz", 9, 12));
    assert_eq!(at(3, "a-b-c"), word("b", 3, 4));
}

#[test]
fn no_word_at_whitespace_or_empty_text() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(at(1, ""), None);
    assert_eq!(at(2, "  x"), None);
    assert_eq!(at(1, "  x"), None);
    assert_eq!(at(1, "\tfoo"), None);
    assert_eq!(at(2, "\tfoo"), word("foo", 2, 5));
    assert_eq!(at(3, "a = -3.14e5 + 1"), None);
}

#[test]
fn numbers_with_sign_and_fraction() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(at(7, "a = -3.14e5 + 1"), word("-3.14e5", 5, 12));
    assert_eq!(at(5, "a = -3.14e5 + 1"), word("-3.14e5", 5, 12));
}

#[test]
fn non_ascii_words_count_utf16_units() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(at(4, "été à côté"), word("été", 1, 4));
    assert_eq!(at(3, "日本語 テキスト"), word("日本語", 1, 4));
    assert_eq!(at(2, "📚az b"), word("📚az", 1, 5));
}

#[test]
fn text_offset_shifts_columns() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let result = get_word_at_text(12, &default_word_regexp(), js("hello world").units(), 5, None);
    assert_eq!(result, word("world", 12, 17));
}

#[test]
fn non_global_definition_is_recreated_with_its_flags() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let re = Rc::new(JsRegExp::from_str("[a-z]+", "i").unwrap());
    let result = get_word_at_text(3, &re, js("ABC def").units(), 0, None);
    assert_eq!(result, word("ABC", 1, 4));
    assert_eq!(re.last_index(), 0);
    assert!(!re.flags().global);

    let valid = ensure_valid_word_definition(Some(&re));
    assert_eq!(valid.source(), js("[a-z]+").units());
    assert_eq!(valid.flags().to_flags_string(), "gi");
    assert!(!Rc::ptr_eq(&valid, &re));
}

#[test]
fn global_definition_is_kept_and_reset() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let g = Rc::new(JsRegExp::from_str("[a-z]+", "g").unwrap());
    g.set_last_index(5);
    assert!(Rc::ptr_eq(&ensure_valid_word_definition(Some(&g)), &g));
    assert_eq!(g.last_index(), 0);
    assert!(Rc::ptr_eq(&ensure_valid_word_definition(None), &default_word_regexp()));
}

#[test]
fn allowed_separators_join_words() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let dash = create_word_reg_exp(js("-").units());
    let expected = r#"(-?\d*\.\d\w*)|([^\`\~\!\@\#\$\%\^\&\*\(\)\=\+\[\{\]\}\\\|\;\:\'\"\,\.\<\>\/\?\s]+)"#;
    assert_eq!(dash.source(), js(expected).units());
    let dash = Rc::new(dash);
    assert_eq!(get_word_at_text(3, &dash, js("a-b-c d").units(), 0, None), word("a-b-c", 1, 6));
    let several = Rc::new(create_word_reg_exp(js("-$@").units()));
    let result = get_word_at_text(3, &several, js("x $foo-bar@baz y").units(), 0, None);
    assert_eq!(result, word("$foo-bar@baz", 3, 15));
}

#[test]
fn long_text_is_cut_around_the_column() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let long = format!("{}longword{}", "a ".repeat(600), " b".repeat(600));
    assert_eq!(long.len(), 2408);
    assert_eq!(at(1203, &long), word("longword", 1201, 1209));
    assert_eq!(at(10, &"abc ".repeat(400)), word("abc", 9, 12));

    // Uma palavra só, maior que `max_len`: sai o trecho de 1000 unidades em volta da coluna.
    let result = at(1500, &"x".repeat(3000)).unwrap();
    assert_eq!(result.word, js(&"x".repeat(1000)));
    assert_eq!((result.start_column, result.end_column), (1001, 2001));
}

#[test]
fn word_longer_than_the_window() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let lw = "q".repeat(100);
    let text = format!("a {lw} b");
    assert_eq!(at(90, &text), word(&lw, 3, 103));

    let small = GetWordAtTextConfig { max_len: 50, window_size: 5, time_budget: 150 };
    let result = get_word_at_text(90, &default_word_regexp(), js(&text).units(), 0, Some(&small));
    assert_eq!(result, word(&"q".repeat(37), 66, 103));
}

#[test]
fn zero_time_budget_finds_nothing() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let config = GetWordAtTextConfig { max_len: 1000, window_size: 15, time_budget: 0 };
    let result = get_word_at_text(7, &default_word_regexp(), js("hello world").units(), 0, Some(&config));
    assert_eq!(result, None);
    assert_eq!(default_word_regexp().last_index(), 0);
}

#[test]
fn default_config_can_be_replaced_and_restored() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let lw = "q".repeat(100);
    let text = format!("a {lw} b");
    let disposable = set_default_get_word_at_text_config(GetWordAtTextConfig { max_len: 50, window_size: 5, time_budget: 150 });
    assert_eq!(at(90, &text), word(&"q".repeat(37), 66, 103));
    disposable.dispose();
    assert_eq!(at(90, &text), word(&lw, 3, 103));
}
