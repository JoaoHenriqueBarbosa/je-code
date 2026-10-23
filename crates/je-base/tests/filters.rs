//! Porte de `src/vs/base/test/common/filters.test.ts` (suíte `Filters`), caso a caso e na mesma ordem.
//!
//! `IFilter` é `&FilterFn`; `string | undefined` do `assertMatches` é `Option<&str>`; as opções do
//! `assertMatches` (`patternPos`, `wordPos`, `firstMatchCanBeWeak`) são o `Opts`. Os índices e o texto
//! decorado do `assertMatches` são em unidades UTF-16, como o `substring` e o `split('')` do original.

mod common;

use std::cell::RefCell;
use std::rc::Rc;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::filters::{self, FilterFn, FuzzyScoreOptions, Match};

fn u(text: &str) -> Vec<u16> {
    text.encode_utf16().collect()
}

fn matches_words_default(word: &[u16], target: &[u16]) -> Option<Vec<Match>> {
    filters::matches_words(word, target, false)
}

fn filter_ok(filter: &FilterFn, word: &str, word_to_match_against: &str, highlights: Option<&[(usize, usize)]>) {
    let r = filter(&u(word), &u(word_to_match_against));
    assert!(r.is_some(), "{word} didn't match {word_to_match_against}");
    if let Some(highlights) = highlights {
        let actual: Vec<(usize, usize)> = r.unwrap().iter().map(|m| (m.start, m.end)).collect();
        assert_eq!(actual, highlights);
    }
}

fn filter_not_ok(filter: &FilterFn, word: &str, word_to_match_against: &str) {
    assert!(filter(&u(word), &u(word_to_match_against)).is_none(), "{word} matched {word_to_match_against}");
}

#[derive(Default)]
struct Opts {
    pattern_pos: usize,
    word_pos: usize,
    first_match_can_be_weak: bool,
}

type Scorer = filters::FuzzyScorerFn;

/// `anyScore` como `FuzzyScorer`: devolve sempre um resultado.
fn any_scorer(pattern: &[u16], low_pattern: &[u16], pattern_pos: usize, word: &[u16], low_word: &[u16], word_pos: usize, _options: Option<&FuzzyScoreOptions>) -> Option<Vec<i32>> {
    Some(filters::any_score(pattern, low_pattern, pattern_pos, word, low_word, word_pos))
}

fn lower(text: &str) -> Vec<u16> {
    je_base::JsString::from(text).to_lower_case().units().to_vec()
}

fn assert_matches(pattern: &str, word: &str, decorated_word: Option<&str>, filter: &Scorer, opts: Opts) {
    let r = filter(
        &u(pattern),
        &lower(pattern),
        opts.pattern_pos,
        &u(word),
        &lower(word),
        opts.word_pos,
        Some(&FuzzyScoreOptions { first_match_can_be_weak: opts.first_match_can_be_weak, boost_full_match: true }),
    );
    assert_eq!(decorated_word.is_none(), r.is_none());
    if let Some(r) = r {
        let word = u(word);
        let mut actual_word: Vec<u16> = Vec::new();
        let mut pos = 0;
        for m in filters::create_matches(Some(&r)) {
            actual_word.extend_from_slice(&word[pos..m.start]);
            for &unit in &word[m.start..m.end] {
                actual_word.push(u16::from(b'^'));
                actual_word.push(unit);
            }
            pos = m.end;
        }
        actual_word.extend_from_slice(&word[pos..]);
        assert_eq!(String::from_utf16_lossy(&actual_word), decorated_word.unwrap());
    }
}

fn assert_top_score(filter: &Scorer, pattern: &str, expected: usize, words: &[&str]) {
    let mut top_score = -(100 * 10);
    let mut top_idx = 0;
    for (i, word) in words.iter().enumerate() {
        if let Some(m) = filter(&u(pattern), &lower(pattern), 0, &u(word), &lower(word), 0, None) {
            let score = m[0];
            if score > top_score {
                top_score = score;
                top_idx = i;
            }
        }
    }
    assert_eq!(top_idx, expected, "{pattern} -> actual={} <> expected={}", words[top_idx], words[expected]);
}

#[test]
fn or() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let counters = Rc::new(RefCell::new([0, 0]));
    let new_filter = |i: usize, r: bool| -> Box<FilterFn> {
        let counters = counters.clone();
        Box::new(move |_, _| {
            counters.borrow_mut()[i] += 1;
            if r { Some(Vec::new()) } else { None }
        })
    };

    *counters.borrow_mut() = [0, 0];
    let filter = filters::or(vec![new_filter(0, false), new_filter(1, false)]);
    filter_not_ok(&filter, "anything", "anything");
    assert_eq!(*counters.borrow(), [1, 1]);

    *counters.borrow_mut() = [0, 0];
    let filter = filters::or(vec![new_filter(0, true), new_filter(1, false)]);
    filter_ok(&filter, "anything", "anything", None);
    assert_eq!(*counters.borrow(), [1, 0]);

    *counters.borrow_mut() = [0, 0];
    let filter = filters::or(vec![new_filter(0, true), new_filter(1, true)]);
    filter_ok(&filter, "anything", "anything", None);
    assert_eq!(*counters.borrow(), [1, 0]);

    *counters.borrow_mut() = [0, 0];
    let filter = filters::or(vec![new_filter(0, false), new_filter(1, true)]);
    filter_ok(&filter, "anything", "anything", None);
    assert_eq!(*counters.borrow(), [1, 1]);
}

#[test]
fn prefix_filter_case_sensitive() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    filter_not_ok(&filters::matches_strict_prefix, "", "");
    filter_ok(&filters::matches_strict_prefix, "", "anything", Some(&[]));
    filter_ok(&filters::matches_strict_prefix, "alpha", "alpha", Some(&[(0, 5)]));
    filter_ok(&filters::matches_strict_prefix, "alpha", "alphasomething", Some(&[(0, 5)]));
    filter_not_ok(&filters::matches_strict_prefix, "alpha", "alp");
    filter_ok(&filters::matches_strict_prefix, "a", "alpha", Some(&[(0, 1)]));
    filter_not_ok(&filters::matches_strict_prefix, "x", "alpha");
    filter_not_ok(&filters::matches_strict_prefix, "A", "alpha");
    filter_not_ok(&filters::matches_strict_prefix, "AlPh", "alPHA");
}

#[test]
fn prefix_filter_ignore_case() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    filter_ok(&filters::matches_prefix, "alpha", "alpha", Some(&[(0, 5)]));
    filter_ok(&filters::matches_prefix, "alpha", "alphasomething", Some(&[(0, 5)]));
    filter_not_ok(&filters::matches_prefix, "alpha", "alp");
    filter_ok(&filters::matches_prefix, "a", "alpha", Some(&[(0, 1)]));
    filter_ok(&filters::matches_prefix, "ä", "Älpha", Some(&[(0, 1)]));
    filter_not_ok(&filters::matches_prefix, "x", "alpha");
    filter_ok(&filters::matches_prefix, "A", "alpha", Some(&[(0, 1)]));
    filter_ok(&filters::matches_prefix, "AlPh", "alPHA", Some(&[(0, 4)]));
    filter_not_ok(&filters::matches_prefix, "T", "4"); // see https://github.com/microsoft/vscode/issues/22401
}

#[test]
fn camel_case_filter() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    filter_not_ok(&filters::matches_camel_case, "", "");
    filter_ok(&filters::matches_camel_case, "", "anything", Some(&[]));
    filter_ok(&filters::matches_camel_case, "alpha", "alpha", Some(&[(0, 5)]));
    filter_ok(&filters::matches_camel_case, "AlPhA", "alpha", Some(&[(0, 5)]));
    filter_ok(&filters::matches_camel_case, "alpha", "alphasomething", Some(&[(0, 5)]));
    filter_not_ok(&filters::matches_camel_case, "alpha", "alp");

    filter_ok(&filters::matches_camel_case, "c", "CamelCaseRocks", Some(&[(0, 1)]));
    filter_ok(&filters::matches_camel_case, "cc", "CamelCaseRocks", Some(&[(0, 1), (5, 6)]));
    filter_ok(&filters::matches_camel_case, "ccr", "CamelCaseRocks", Some(&[(0, 1), (5, 6), (9, 10)]));
    filter_ok(&filters::matches_camel_case, "cacr", "CamelCaseRocks", Some(&[(0, 2), (5, 6), (9, 10)]));
    filter_ok(&filters::matches_camel_case, "cacar", "CamelCaseRocks", Some(&[(0, 2), (5, 7), (9, 10)]));
    filter_ok(&filters::matches_camel_case, "ccarocks", "CamelCaseRocks", Some(&[(0, 1), (5, 7), (9, 14)]));
    filter_ok(&filters::matches_camel_case, "cr", "CamelCaseRocks", Some(&[(0, 1), (9, 10)]));
    filter_ok(&filters::matches_camel_case, "fba", "FooBarAbe", Some(&[(0, 1), (3, 5)]));
    filter_ok(&filters::matches_camel_case, "fbar", "FooBarAbe", Some(&[(0, 1), (3, 6)]));
    filter_ok(&filters::matches_camel_case, "fbara", "FooBarAbe", Some(&[(0, 1), (3, 7)]));
    filter_ok(&filters::matches_camel_case, "fbaa", "FooBarAbe", Some(&[(0, 1), (3, 5), (6, 7)]));
    filter_ok(&filters::matches_camel_case, "fbaab", "FooBarAbe", Some(&[(0, 1), (3, 5), (6, 8)]));
    filter_ok(&filters::matches_camel_case, "c2d", "canvasCreation2D", Some(&[(0, 1), (14, 16)]));
    filter_ok(&filters::matches_camel_case, "cce", "_canvasCreationEvent", Some(&[(1, 2), (7, 8), (15, 16)]));
}

#[test]
fn camel_case_filter_19256() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    assert!(filters::matches_camel_case(&u("Debug Console"), &u("Open: Debug Console")).is_some());
    assert!(filters::matches_camel_case(&u("Debug console"), &u("Open: Debug Console")).is_some());
    assert!(filters::matches_camel_case(&u("debug console"), &u("Open: Debug Console")).is_some());
}

#[test]
fn matches_contiguous_sub_string() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    filter_ok(&filters::matches_contiguous_sub_string, "cela", "cancelAnimationFrame()", Some(&[(3, 7)]));
}

#[test]
fn matches_base_contiguous_sub_string() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    filter_ok(&filters::matches_base_contiguous_sub_string, "cela", "cancelAnimationFrame()", Some(&[(3, 7)]));
    filter_ok(&filters::matches_base_contiguous_sub_string, "cafe", "café", Some(&[(0, 4)]));
    filter_ok(&filters::matches_base_contiguous_sub_string, "cafe", "caféBar", Some(&[(0, 4)]));
    filter_ok(&filters::matches_base_contiguous_sub_string, "resume", "résumé", Some(&[(0, 6)]));
    filter_ok(&filters::matches_base_contiguous_sub_string, "naïve", "naïve", Some(&[(0, 5)]));
    filter_ok(&filters::matches_base_contiguous_sub_string, "naive", "naïve", Some(&[(0, 5)]));
    filter_ok(&filters::matches_base_contiguous_sub_string, "aeou", "àéöü", Some(&[(0, 4)]));
}

#[test]
fn matches_sub_string() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    filter_ok(&filters::matches_sub_string, "cmm", "cancelAnimationFrame()", Some(&[(0, 1), (9, 10), (18, 19)]));
    filter_ok(&filters::matches_sub_string, "abc", "abcabc", Some(&[(0, 3)]));
    filter_ok(&filters::matches_sub_string, "abc", "aaabbbccc", Some(&[(0, 1), (3, 4), (6, 7)]));
}

#[test]
fn matches_sub_string_performance_35346() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    filter_not_ok(&filters::matches_sub_string, "aaaaaaaaaaaaaaaaaaaax", "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa");
}

#[test]
fn word_filter() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    filter_ok(&matches_words_default, "alpha", "alpha", Some(&[(0, 5)]));
    filter_ok(&matches_words_default, "alpha", "alphasomething", Some(&[(0, 5)]));
    filter_not_ok(&matches_words_default, "alpha", "alp");
    filter_ok(&matches_words_default, "a", "alpha", Some(&[(0, 1)]));
    filter_not_ok(&matches_words_default, "x", "alpha");
    filter_ok(&matches_words_default, "A", "alpha", Some(&[(0, 1)]));
    filter_ok(&matches_words_default, "AlPh", "alPHA", Some(&[(0, 4)]));
    assert!(matches_words_default(&u("Debug Console"), &u("Open: Debug Console")).is_some());

    filter_ok(&matches_words_default, "gp", "Git: Pull", Some(&[(0, 1), (5, 6)]));
    filter_ok(&matches_words_default, "g p", "Git: Pull", Some(&[(0, 1), (5, 6)]));
    filter_ok(&matches_words_default, "gipu", "Git: Pull", Some(&[(0, 2), (5, 7)]));

    filter_ok(&matches_words_default, "gp", "Category: Git: Pull", Some(&[(10, 11), (15, 16)]));
    filter_ok(&matches_words_default, "g p", "Category: Git: Pull", Some(&[(10, 11), (15, 16)]));
    filter_ok(&matches_words_default, "gipu", "Category: Git: Pull", Some(&[(10, 12), (15, 17)]));

    filter_not_ok(&matches_words_default, "it", "Git: Pull");
    filter_not_ok(&matches_words_default, "ll", "Git: Pull");

    filter_ok(&matches_words_default, "git: プル", "git: プル", Some(&[(0, 7)]));
    filter_ok(&matches_words_default, "git プル", "git: プル", Some(&[(0, 3), (5, 7)]));

    filter_ok(&matches_words_default, "öäk", "Öhm: Älles Klar", Some(&[(0, 1), (5, 6), (11, 12)]));

    // Handles issue #123915
    filter_ok(&matches_words_default, "C++", "C/C++: command", Some(&[(2, 5)]));

    // Handles issue #154533
    filter_ok(&matches_words_default, ".", ":", Some(&[]));
    filter_ok(&matches_words_default, ".", ".", Some(&[(0, 1)]));

    // assert.ok(matchesWords('gipu', 'Category: Git: Pull', true) === null);
    // assert.deepStrictEqual(matchesWords('pu', 'Category: Git: Pull', true), [{ start: 15, end: 17 }]);

    filter_ok(&matches_words_default, "bar", "foo-bar", None);
    filter_ok(&matches_words_default, "bar test", "foo-bar test", None);
    filter_ok(&matches_words_default, "fbt", "foo-bar test", None);
    filter_ok(&matches_words_default, "bar test", "foo-bar (test)", None);
    filter_ok(&matches_words_default, "foo bar", "foo (bar)", None);

    filter_not_ok(&matches_words_default, "bar est", "foo-bar test");
    filter_not_ok(&matches_words_default, "fo ar", "foo-bar test");
    filter_not_ok(&matches_words_default, "for", "foo-bar test");

    filter_ok(&matches_words_default, "foo bar", "foo-bar", None);
    filter_ok(&matches_words_default, "foo bar", "123 foo-bar 456", None);
    filter_ok(&matches_words_default, "foo-bar", "foo bar", None);
    filter_ok(&matches_words_default, "foo:bar", "foo:bar", None);
}

#[test]
fn matches_words_performance_309582() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    // Buscar um termo com um separador de palavra (por exemplo `.`) contra alvos parecidos com ids de
    // comando causava retrocesso catastrófico e congelava o editor de atalhos de teclado. Sem a correção
    // este laço passa do tempo limite do teste.
    let targets = [
        "workbench.action.terminal.focusNextLine",
        "editor.action.clipboardCopyAction",
        "workbench.action.editor.changeLanguageMode",
        "editor.action.smartSelect.expand",
        "workbench.action.files.saveAll",
    ];
    for _ in 0..1000 {
        for target in targets {
            filters::matches_words(&u("editor.action"), &u(target), false);
        }
    }
}

#[test]
fn fuzzy_score_23215() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    assert_matches("tit", "win.tit", Some("win.^t^i^t"), &filters::fuzzy_score, Opts::default());
    assert_matches("title", "win.title", Some("win.^t^i^t^l^e"), &filters::fuzzy_score, Opts::default());
    assert_matches("WordCla", "WordCharacterClassifier", Some("^W^o^r^dCharacter^C^l^assifier"), &filters::fuzzy_score, Opts::default());
    assert_matches("WordCCla", "WordCharacterClassifier", Some("^W^o^r^d^Character^C^l^assifier"), &filters::fuzzy_score, Opts::default());
}

#[test]
fn fuzzy_score_23332() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    assert_matches("dete", "\"editor.quickSuggestionsDelay\"", None, &filters::fuzzy_score, Opts::default());
}

#[test]
fn fuzzy_score_23190() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    assert_matches("c:\\do", "& 'C:\\Documents and Settings'", Some("& '^C^:^\\^D^ocuments and Settings'"), &filters::fuzzy_score, Opts::default());
    assert_matches("c:\\do", "& 'c:\\Documents and Settings'", Some("& '^c^:^\\^D^ocuments and Settings'"), &filters::fuzzy_score, Opts::default());
}

#[test]
fn fuzzy_score_23581() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    assert_matches("close", "css.lint.importStatement", Some("^css.^lint.imp^ort^Stat^ement"), &filters::fuzzy_score, Opts::default());
    assert_matches("close", "css.colorDecorators.enable", Some("^css.co^l^orDecorator^s.^enable"), &filters::fuzzy_score, Opts::default());
    assert_matches("close", "workbench.quickOpen.closeOnFocusOut", Some("workbench.quickOpen.^c^l^o^s^eOnFocusOut"), &filters::fuzzy_score, Opts::default());
    assert_top_score(&filters::fuzzy_score, "close", 2, &["css.lint.importStatement", "css.colorDecorators.enable", "workbench.quickOpen.closeOnFocusOut"]);
}

#[test]
fn fuzzy_score_23458() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    assert_matches("highlight", "editorHoverHighlight", Some("editorHover^H^i^g^h^l^i^g^h^t"), &filters::fuzzy_score, Opts::default());
    assert_matches("hhighlight", "editorHoverHighlight", Some("editor^Hover^H^i^g^h^l^i^g^h^t"), &filters::fuzzy_score, Opts::default());
    assert_matches("dhhighlight", "editorHoverHighlight", None, &filters::fuzzy_score, Opts::default());
}

#[test]
fn fuzzy_score_23746() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    assert_matches("-moz", "-moz-foo", Some("^-^m^o^z-foo"), &filters::fuzzy_score, Opts::default());
    assert_matches("moz", "-moz-foo", Some("-^m^o^z-foo"), &filters::fuzzy_score, Opts::default());
    assert_matches("moz", "-moz-animation", Some("-^m^o^z-animation"), &filters::fuzzy_score, Opts::default());
    assert_matches("moza", "-moz-animation", Some("-^m^o^z-^animation"), &filters::fuzzy_score, Opts::default());
}

#[test]
fn fuzzy_score() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    assert_matches("ab", "abA", Some("^a^bA"), &filters::fuzzy_score, Opts::default());
    assert_matches("ccm", "cacmelCase", Some("^ca^c^melCase"), &filters::fuzzy_score, Opts::default());
    assert_matches("bti", "the_black_knight", None, &filters::fuzzy_score, Opts::default());
    assert_matches("ccm", "camelCase", None, &filters::fuzzy_score, Opts::default());
    assert_matches("cmcm", "camelCase", None, &filters::fuzzy_score, Opts::default());
    assert_matches("BK", "the_black_knight", Some("the_^black_^knight"), &filters::fuzzy_score, Opts::default());
    assert_matches("KeyboardLayout=", "KeyboardLayout", None, &filters::fuzzy_score, Opts::default());
    assert_matches("LLL", "SVisualLoggerLogsList", Some("SVisual^Logger^Logs^List"), &filters::fuzzy_score, Opts::default());
    assert_matches("LLLL", "SVilLoLosLi", None, &filters::fuzzy_score, Opts::default());
    assert_matches("LLLL", "SVisualLoggerLogsList", None, &filters::fuzzy_score, Opts::default());
    assert_matches("TEdit", "TextEdit", Some("^Text^E^d^i^t"), &filters::fuzzy_score, Opts::default());
    assert_matches("TEdit", "TextEditor", Some("^Text^E^d^i^tor"), &filters::fuzzy_score, Opts::default());
    assert_matches("TEdit", "Textedit", Some("^Text^e^d^i^t"), &filters::fuzzy_score, Opts::default());
    assert_matches("TEdit", "text_edit", Some("^text_^e^d^i^t"), &filters::fuzzy_score, Opts::default());
    assert_matches("TEditDit", "TextEditorDecorationType", Some("^Text^E^d^i^tor^Decorat^ion^Type"), &filters::fuzzy_score, Opts::default());
    assert_matches("TEdit", "TextEditorDecorationType", Some("^Text^E^d^i^torDecorationType"), &filters::fuzzy_score, Opts::default());
    assert_matches("Tedit", "TextEdit", Some("^Text^E^d^i^t"), &filters::fuzzy_score, Opts::default());
    assert_matches("ba", "?AB?", None, &filters::fuzzy_score, Opts::default());
    assert_matches("bkn", "the_black_knight", Some("the_^black_^k^night"), &filters::fuzzy_score, Opts::default());
    assert_matches("bt", "the_black_knight", Some("the_^black_knigh^t"), &filters::fuzzy_score, Opts::default());
    assert_matches("ccm", "camelCasecm", Some("^camel^Casec^m"), &filters::fuzzy_score, Opts::default());
    assert_matches("fdm", "findModel", Some("^fin^d^Model"), &filters::fuzzy_score, Opts::default());
    assert_matches("fob", "foobar", Some("^f^oo^bar"), &filters::fuzzy_score, Opts::default());
    assert_matches("fobz", "foobar", None, &filters::fuzzy_score, Opts::default());
    assert_matches("foobar", "foobar", Some("^f^o^o^b^a^r"), &filters::fuzzy_score, Opts::default());
    assert_matches("form", "editor.formatOnSave", Some("editor.^f^o^r^matOnSave"), &filters::fuzzy_score, Opts::default());
    assert_matches("g p", "Git: Pull", Some("^Git:^ ^Pull"), &filters::fuzzy_score, Opts::default());
    assert_matches("g p", "Git: Pull", Some("^Git:^ ^Pull"), &filters::fuzzy_score, Opts::default());
    assert_matches("gip", "Git: Pull", Some("^G^it: ^Pull"), &filters::fuzzy_score, Opts::default());
    assert_matches("gip", "Git: Pull", Some("^G^it: ^Pull"), &filters::fuzzy_score, Opts::default());
    assert_matches("gp", "Git: Pull", Some("^Git: ^Pull"), &filters::fuzzy_score, Opts::default());
    assert_matches("gp", "Git_Git_Pull", Some("^Git_Git_^Pull"), &filters::fuzzy_score, Opts::default());
    assert_matches("is", "ImportStatement", Some("^Import^Statement"), &filters::fuzzy_score, Opts::default());
    assert_matches("is", "isValid", Some("^i^sValid"), &filters::fuzzy_score, Opts::default());
    assert_matches("lowrd", "lowWord", Some("^l^o^wWo^r^d"), &filters::fuzzy_score, Opts::default());
    assert_matches("myvable", "myvariable", Some("^m^y^v^aria^b^l^e"), &filters::fuzzy_score, Opts::default());
    assert_matches("no", "", None, &filters::fuzzy_score, Opts::default());
    assert_matches("no", "match", None, &filters::fuzzy_score, Opts::default());
    assert_matches("ob", "foobar", None, &filters::fuzzy_score, Opts::default());
    assert_matches("sl", "SVisualLoggerLogsList", Some("^SVisual^LoggerLogsList"), &filters::fuzzy_score, Opts::default());
    assert_matches("sllll", "SVisualLoggerLogsList", Some("^SVisua^l^Logger^Logs^List"), &filters::fuzzy_score, Opts::default());
    assert_matches("Three", "HTMLHRElement", None, &filters::fuzzy_score, Opts::default());
    assert_matches("Three", "Three", Some("^T^h^r^e^e"), &filters::fuzzy_score, Opts::default());
    assert_matches("fo", "barfoo", None, &filters::fuzzy_score, Opts::default());
    assert_matches("fo", "bar_foo", Some("bar_^f^oo"), &filters::fuzzy_score, Opts::default());
    assert_matches("fo", "bar_Foo", Some("bar_^F^oo"), &filters::fuzzy_score, Opts::default());
    assert_matches("fo", "bar foo", Some("bar ^f^oo"), &filters::fuzzy_score, Opts::default());
    assert_matches("fo", "bar.foo", Some("bar.^f^oo"), &filters::fuzzy_score, Opts::default());
    assert_matches("fo", "bar/foo", Some("bar/^f^oo"), &filters::fuzzy_score, Opts::default());
    assert_matches("fo", "bar\\foo", Some("bar\\^f^oo"), &filters::fuzzy_score, Opts::default());
}

#[test]
fn fuzzy_score_first_match_can_be_weak() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();

    assert_matches("Three", "HTMLHRElement", Some("H^TML^H^R^El^ement"), &filters::fuzzy_score, Opts { first_match_can_be_weak: true, ..Opts::default() });
    assert_matches("tor", "constructor", Some("construc^t^o^r"), &filters::fuzzy_score, Opts { first_match_can_be_weak: true, ..Opts::default() });
    assert_matches("ur", "constructor", Some("constr^ucto^r"), &filters::fuzzy_score, Opts { first_match_can_be_weak: true, ..Opts::default() });
    assert_top_score(&filters::fuzzy_score, "tor", 2, &["constructor", "Thor", "cTor"]);
}

#[test]
fn fuzzy_score_many_matches() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();

    assert_matches("aaaaaa", "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", Some("^a^a^a^a^a^aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"), &filters::fuzzy_score, Opts::default());
}

#[test]
fn freeze_when_fjfj_jfjf_https_github_com_microsoft_vscode_issues_91807() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    assert_matches("jfjfj", "fjfjfjfjfjfjfjfjfjfjfj", None, &filters::fuzzy_score, Opts::default());
    assert_matches("jfjfjfjfjfjfjfjfjfj", "fjfjfjfjfjfjfjfjfjfjfjfjfjfjfjfjfjfjfjfjfjfjfjfjfjfjfjfjfjfj", None, &filters::fuzzy_score, Opts::default());
    assert_matches("jfjfjfjfjfjfjfjfjfjjfjfjfjfjfjfjfjfjfjjfjfjfjfjfjfjfjfjfjjfjfjfjfjfjfjfjfjfjjfjfjfjfjfjfjfjfjfjjfjfjfjfjfjfjfjfjfj", "fjfjfjfjfjfjfjfjfjfjfjfjfjfjfjfjfjfjfjfjfjfjfjfjfjfjfjfjfjfjfjfjfjfjfjfjfjfjfjfjfjfjfjfjfjfjfjfjfjfjfjfjfjfjfjfjfjfjfjfj", None, &filters::fuzzy_score, Opts::default());
    assert_matches("jfjfjfjfjfjfjfjfjfj", "fJfjfjfjfjfjfjfjfjfjfjfjfjfjfjfjfjfjfjfjfjfjfjfjfjfjfjfjfjfj", Some("f^J^f^j^f^j^f^j^f^j^f^j^f^j^f^j^f^j^f^jfjfjfjfjfjfjfjfjfjfjfjfjfjfjfjfjfjfjfjfj"), &filters::fuzzy_score, Opts::default());
    assert_matches("jfjfjfjfjfjfjfjfjfj", "fjfjfjfjfjfjfjfjfjfjfjfjfjfjfjfjfjfjfjfjfjfjfjfjfjfjfjfjfjfj", Some("f^j^f^j^f^j^f^j^f^j^f^j^f^j^f^j^f^j^f^jfjfjfjfjfjfjfjfjfjfjfjfjfjfjfjfjfjfjfjfj"), &filters::fuzzy_score, Opts { first_match_can_be_weak: true, ..Opts::default() });
}

#[test]
fn fuzzy_score_issue_26423() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();

    assert_matches("baba", "abababab", None, &filters::fuzzy_score, Opts::default());

    assert_matches("fsfsfs", "dsafdsafdsafdsafdsafdsafdsafasdfdsa", None, &filters::fuzzy_score, Opts::default());
    assert_matches("fsfsfsfsfsfsfsf", "dsafdsafdsafdsafdsafdsafdsafasdfdsafdsafdsafdsafdsfdsafdsfdfdfasdnfdsajfndsjnafjndsajlknfdsa", None, &filters::fuzzy_score, Opts::default());
}

#[test]
fn fuzzy_intelli_sense_matching_vs_haxe_metadata_completion_26995() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    assert_matches("f", ":Foo", Some(":^Foo"), &filters::fuzzy_score, Opts::default());
    assert_matches("f", ":foo", Some(":^foo"), &filters::fuzzy_score, Opts::default());
}

#[test]
fn separator_only_match_should_not_be_weak_79558() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    assert_matches(".", "foo.bar", Some("foo^.bar"), &filters::fuzzy_score, Opts::default());
}

#[test]
fn cannot_set_property_1_of_undefined_26511() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let word = "a".repeat(122); // new Array<void>(123).join('a')
    let pattern = "a".repeat(119); // new Array<void>(120).join('a')
    filters::fuzzy_score(&u(&pattern), &u(&pattern.to_lowercase()), 0, &u(&word), &u(&word.to_lowercase()), 0, None);
    // não pode explodir
}

#[test]
fn vscode_1_12_no_longer_obeys_sort_text_in_completion_items_from_language_server_26096() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    assert_matches("  ", "  group", None, &filters::fuzzy_score, Opts { pattern_pos: 2, ..Opts::default() });
    assert_matches("  g", "  group", Some("  ^group"), &filters::fuzzy_score, Opts { pattern_pos: 2, ..Opts::default() });
    assert_matches("g", "  group", Some("  ^group"), &filters::fuzzy_score, Opts::default());
    assert_matches("g g", "  groupGroup", None, &filters::fuzzy_score, Opts::default());
    assert_matches("g g", "  group Group", Some("  ^group^ ^Group"), &filters::fuzzy_score, Opts::default());
    assert_matches(" g g", "  group Group", Some("  ^group^ ^Group"), &filters::fuzzy_score, Opts { pattern_pos: 1, ..Opts::default() });
    assert_matches("zz", "zzGroup", Some("^z^zGroup"), &filters::fuzzy_score, Opts::default());
    assert_matches("zzg", "zzGroup", Some("^z^z^Group"), &filters::fuzzy_score, Opts::default());
    assert_matches("g", "zzGroup", Some("zz^Group"), &filters::fuzzy_score, Opts::default());
}

#[test]
fn pattern_pos_isn_t_working_correctly_79815() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    assert_matches(&":p"[1..], "prop", Some("^prop"), &filters::fuzzy_score, Opts { pattern_pos: 0, ..Opts::default() });
    assert_matches(":p", "prop", Some("^prop"), &filters::fuzzy_score, Opts { pattern_pos: 1, ..Opts::default() });
    assert_matches(":p", "prop", None, &filters::fuzzy_score, Opts { pattern_pos: 2, ..Opts::default() });
    assert_matches(":p", "proP", Some("pro^P"), &filters::fuzzy_score, Opts { pattern_pos: 1, word_pos: 1, ..Opts::default() });
    assert_matches(":p", "aprop", Some("a^prop"), &filters::fuzzy_score, Opts { pattern_pos: 1, first_match_can_be_weak: true, ..Opts::default() });
    assert_matches(":p", "aprop", None, &filters::fuzzy_score, Opts { pattern_pos: 1, first_match_can_be_weak: false, ..Opts::default() });
}

#[test]
fn top_score_fuzzy_score() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();

    assert_top_score(&filters::fuzzy_score, "cons", 2, &["ArrayBufferConstructor", "Console", "console"]);
    assert_top_score(&filters::fuzzy_score, "Foo", 1, &["foo", "Foo", "foo"]);

    // #24904
    assert_top_score(&filters::fuzzy_score, "onMess", 1, &["onmessage", "onMessage", "onThisMegaEscape"]);

    assert_top_score(&filters::fuzzy_score, "CC", 1, &["camelCase", "CamelCase"]);
    assert_top_score(&filters::fuzzy_score, "cC", 0, &["camelCase", "CamelCase"]);
    // assertTopScore(fuzzyScore, 'cC', 1, 'ccfoo', 'camelCase');
    // assertTopScore(fuzzyScore, 'cC', 1, 'ccfoo', 'camelCase', 'foo-cC-bar');

    // issue #17836
    // assertTopScore(fuzzyScore, 'TEdit', 1, 'TextEditorDecorationType', 'TextEdit', 'TextEditor');
    assert_top_score(&filters::fuzzy_score, "p", 4, &["parse", "posix", "pafdsa", "path", "p"]);
    assert_top_score(&filters::fuzzy_score, "pa", 0, &["parse", "pafdsa", "path"]);

    // issue #14583
    assert_top_score(&filters::fuzzy_score, "log", 3, &["HTMLOptGroupElement", "ScrollLogicalPosition", "SVGFEMorphologyElement", "log", "logger"]);
    assert_top_score(&filters::fuzzy_score, "e", 2, &["AbstractWorker", "ActiveXObject", "else"]);

    // issue #14446
    assert_top_score(&filters::fuzzy_score, "workbench.sideb", 1, &["workbench.editor.defaultSideBySideLayout", "workbench.sideBar.location"]);

    // issue #11423
    assert_top_score(&filters::fuzzy_score, "editor.r", 2, &["diffEditor.renderSideBySide", "editor.overviewRulerlanes", "editor.renderControlCharacter", "editor.renderWhitespace"]);
    // assertTopScore(fuzzyScore, 'editor.R', 1, 'diffEditor.renderSideBySide', 'editor.overviewRulerlanes', 'editor.renderControlCharacter', 'editor.renderWhitespace');
    // assertTopScore(fuzzyScore, 'Editor.r', 0, 'diffEditor.renderSideBySide', 'editor.overviewRulerlanes', 'editor.renderControlCharacter', 'editor.renderWhitespace');

    assert_top_score(&filters::fuzzy_score, "-mo", 1, &["-ms-ime-mode", "-moz-columns"]);
    // dupe, issue #14861
    assert_top_score(&filters::fuzzy_score, "convertModelPosition", 0, &["convertModelPositionToViewPosition", "convertViewToModelPosition"]);
    // dupe, issue #14942
    assert_top_score(&filters::fuzzy_score, "is", 0, &["isValidViewletId", "import statement"]);

    assert_top_score(&filters::fuzzy_score, "title", 1, &["files.trimTrailingWhitespace", "window.title"]);

    assert_top_score(&filters::fuzzy_score, "const", 1, &["constructor", "const", "cuOnstrul"]);
}

#[test]
fn unexpected_suggestion_scoring_28791() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    assert_top_score(&filters::fuzzy_score, "_lines", 1, &["_lineStarts", "_lines"]);
    assert_top_score(&filters::fuzzy_score, "_lines", 1, &["_lineS", "_lines"]);
    assert_top_score(&filters::fuzzy_score, "_lineS", 0, &["_lineS", "_lines"]);
}

#[test]
#[ignore = "test.skip no original"]
fn bad_completion_ranking_changes_valid_variable_name_to_class_name_when_pressing_187055() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    assert_top_score(&filters::fuzzy_score, "a", 1, &["A", "a"]);
    assert_top_score(&filters::fuzzy_score, "theme", 1, &["Theme", "theme"]);
}

#[test]
fn html_closing_tag_proposal_filtered_out_38880() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    assert_matches("\t\t<", "\t\t</body>", Some("^\t^\t^</body>"), &filters::fuzzy_score, Opts { pattern_pos: 0, ..Opts::default() });
    assert_matches("\t\t<", "\t\t</body>", Some("\t\t^</body>"), &filters::fuzzy_score, Opts { pattern_pos: 2, ..Opts::default() });
    assert_matches("\t<", "\t</body>", Some("\t^</body>"), &filters::fuzzy_score, Opts { pattern_pos: 1, ..Opts::default() });
}

#[test]
fn fuzzy_score_graceful() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();

    assert_matches("rlut", "result", None, &filters::fuzzy_score, Opts::default());
    assert_matches("rlut", "result", Some("^res^u^l^t"), &filters::fuzzy_score_graceful, Opts::default());

    assert_matches("cno", "console", Some("^co^ns^ole"), &filters::fuzzy_score, Opts::default());
    assert_matches("cno", "console", Some("^co^ns^ole"), &filters::fuzzy_score_graceful, Opts::default());
    assert_matches("cno", "console", Some("^c^o^nsole"), &filters::fuzzy_score_graceful_aggressive, Opts::default());
    assert_matches("cno", "co_new", Some("^c^o_^new"), &filters::fuzzy_score_graceful, Opts::default());
    assert_matches("cno", "co_new", Some("^c^o_^new"), &filters::fuzzy_score_graceful_aggressive, Opts::default());
}

#[test]
fn list_highlight_filter_not_all_characters_from_match_are_highlighterd_66923() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    assert_matches("foo", "barbarbarbarbarbarbarbarbarbarbarbarbarbarbarbar_foo", Some("barbarbarbarbarbarbarbarbarbarbarbarbarbarbarbar_^f^o^o"), &filters::fuzzy_score, Opts::default());
}

#[test]
fn autocompletion_is_matched_against_truncated_filter_text_to_54_characters_74133() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    assert_matches("foo", "ffffffffffffffffffffffffffffbarbarbarbarbarbarbarbarbarbarbarbarbarbarbarbarbarbarbarbarbarbarbarbarbarbarbarbarbarbarbarbar_foo", Some("ffffffffffffffffffffffffffffbarbarbarbarbarbarbarbarbarbarbarbarbarbarbarbarbarbarbarbarbarbarbarbarbarbarbarbarbarbarbarbar_^f^o^o"), &filters::fuzzy_score, Opts::default());
    assert_matches("Aoo", "Affffffffffffffffffffffffffffbarbarbarbarbarbarbarbarbarbarbarbarbarbarbarbarbarbarbarbarbarbarbarbarbarbarbarbarbarbar_foo", Some("^Affffffffffffffffffffffffffffbarbarbarbarbarbarbarbarbarbarbarbarbarbarbarbarbarbarbarbarbarbarbarbarbarbarbarbarbarbar_f^o^o"), &filters::fuzzy_score, Opts::default());
    assert_matches("foo", "Gffffffffffffffffffffffffffffbarbarbarbarbarbarbarbarbarbarbarbarbarbarbarbarbarbarbarbarbarbarbarbarbarbarbarbarbarbarbarbar_foo", None, &filters::fuzzy_score, Opts::default());
}

#[test]
fn go_to_symbol_with_the_exact_method_name_doesn_t_work_as_expected_84787() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let options = FuzzyScoreOptions { first_match_can_be_weak: true, boost_full_match: true };
    let match_ = filters::fuzzy_score(&u(":get"), &u(":get"), 1, &u("get"), &u("get"), 0, Some(&options));
    assert!(match_.is_some());
}

#[test]
fn wrong_highlight_after_emoji_113404() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    assert_matches("di", "✨div classname=\"\"></div>", Some("✨^d^iv classname=\"\"></div>"), &filters::fuzzy_score, Opts::default());
    assert_matches("di", "adiv classname=\"\"></div>", Some("adiv classname=\"\"></^d^iv>"), &filters::fuzzy_score, Opts::default());
}

#[test]
fn suggestion_is_not_highlighted_85826() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    assert_matches("SemanticTokens", "SemanticTokensEdits", Some("^S^e^m^a^n^t^i^c^T^o^k^e^n^sEdits"), &filters::fuzzy_score, Opts::default());
    assert_matches("SemanticTokens", "SemanticTokensEdits", Some("^S^e^m^a^n^t^i^c^T^o^k^e^n^sEdits"), &filters::fuzzy_score_graceful_aggressive, Opts::default());
}

#[test]
fn intelli_sense_completion_not_correctly_highlighting_text_in_front_of_cursor_115250() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    assert_matches("lo", "log", Some("^l^og"), &filters::fuzzy_score, Opts::default());
    assert_matches(".lo", "log", Some("^l^og"), &any_scorer, Opts::default());
    assert_matches(".", "log", Some("log"), &any_scorer, Opts::default());
}

#[test]
fn any_score_should_not_require_a_strong_first_match() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    assert_matches("bar", "foobAr", Some("foo^b^A^r"), &any_scorer, Opts::default());
    assert_matches("bar", "foobar", Some("foo^b^a^r"), &any_scorer, Opts::default());
}

#[test]
fn configurable_full_match_boost() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let prefix = u("create");
    let a = "createModelServices";
    let b = "create";
    let score = |word: &[u16], word_low: &[u16], word_start: usize, boost_full_match: bool| {
        filters::fuzzy_score(&prefix, &prefix, 0, word, word_low, word_start, Some(&FuzzyScoreOptions { boost_full_match, first_match_can_be_weak: true }))
    };

    let mut a_boost = score(&u(a), &u(&a.to_lowercase()), 0, true);
    let mut b_boost = score(&u(b), &u(&b.to_lowercase()), 0, true);
    assert!(a_boost.is_some());
    assert!(b_boost.is_some());
    assert!(a_boost.unwrap()[0] < b_boost.unwrap()[0]);

    // também funciona com wordStart > 0 (https://github.com/microsoft/vscode/issues/187921)
    let word_prefix = "$(symbol-function) ";
    let with_prefix = |word: &str| u(&format!("{word_prefix}{word}"));
    let with_prefix_low = |word: &str| u(&format!("{word_prefix}{word}").to_lowercase());
    let word_start = word_prefix.len();
    a_boost = score(&with_prefix(a), &with_prefix_low(a), word_start, true);
    b_boost = score(&with_prefix(b), &with_prefix_low(b), word_start, true);
    assert!(a_boost.is_some());
    assert!(b_boost.is_some());
    assert!(a_boost.unwrap()[0] < b_boost.unwrap()[0]);

    let a_score = score(&u(a), &u(&a.to_lowercase()), 0, false);
    let b_score = score(&u(b), &u(&b.to_lowercase()), 0, false);
    assert!(a_score.is_some());
    assert!(b_score.is_some());
    assert!(a_score.unwrap()[0] == b_score.unwrap()[0]);
}

#[test]
fn unexpected_suggest_highlighting_ignores_whole_word_match_in_favor_of_matching_first_letter_147423() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();

    assert_matches("i", "machine/{id}", Some("machine/{^id}"), &filters::fuzzy_score, Opts::default());
    assert_matches("ok", "obobobf{ok}/user", Some("^obobobf{o^k}/user"), &filters::fuzzy_score, Opts::default());
}
