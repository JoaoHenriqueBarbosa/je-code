//! Porte de `src/vs/base/test/common/strings.test.ts` (suíte `Strings`), caso a caso e na mesma ordem.
//!
//! Ficam de fora, porque dependem de um motor de regex com semântica ECMAScript que o projeto ainda não
//! tem (as funções correspondentes estão listadas como pendentes no topo de `src/strings.rs`):
//! - `createRegExp`: testa `strings.createRegExp`, que monta e devolve um `RegExp`;
//! - `replaceAsync`: testa `strings.replaceAsync`, que recebe um `RegExp` e um callback assíncrono.
//!
//! O que o TypeScript passa como `undefined`/`null` vira `None` (ou o texto vazio, onde a função trata os dois
//! igual, como em `containsUppercaseCharacter`), e os argumentos de `format`/`format2` chegam já em texto.

use std::collections::HashMap;

use je_base::JsString;
use je_base::js_string::trim as js_trim;
use je_base::strings::{self, AmbiguousCharacters, GraphemeBreakType, InvisibleCharacters};

fn u(text: &str) -> Vec<u16> {
    text.encode_utf16().collect()
}

/// Texto de um resultado (fatia, `Vec` ou `JsString`) para comparar com `&str`.
fn t(units: impl AsRef<[u16]>) -> String {
    String::from_utf16_lossy(units.as_ref())
}

fn lower(text: &[u16]) -> JsString {
    JsString::from(text).to_lower_case()
}

fn sign(n: i32) -> i32 {
    n.signum()
}

fn fmt(value: &str, args: &[&str]) -> String {
    let args: Vec<Vec<u16>> = args.iter().map(|a| u(a)).collect();
    t(strings::format(&u(value), &args))
}

fn fmt2(template: &str, values: &[(&str, &str)]) -> String {
    let values: HashMap<JsString, JsString> = values.iter().map(|&(k, v)| (JsString::from(k), JsString::from(v))).collect();
    t(strings::format2(&u(template), &values))
}

fn last_non_ws(text: &str, start_index: Option<i64>) -> Option<usize> {
    let units = u(text);
    strings::last_non_whitespace_index(&units, start_index.unwrap_or(units.len() as i64 - 1))
}

fn leading_ws(text: &str, start: Option<usize>, end: Option<usize>) -> String {
    let units = u(text);
    t(strings::get_leading_whitespace(&units, start.unwrap_or(0), end.unwrap_or(units.len())))
}

fn lcut_of(text: &str, n: usize, prefix: &str) -> String {
    t(strings::lcut(&u(text), n, &u(prefix)))
}

fn rcut_of(text: &str, n: usize, suffix: &str) -> String {
    t(strings::rcut(&u(text), n, &u(suffix)))
}

fn ltrim_of(haystack: &str, needle: &str) -> String {
    t(strings::ltrim(&u(haystack), &u(needle)))
}

fn rtrim_of(haystack: &str, needle: &str) -> String {
    t(strings::rtrim(&u(haystack), &u(needle)))
}

fn trim_of(haystack: &str, needle: &str) -> String {
    t(strings::trim(&u(haystack), &u(needle)))
}

fn utf8_bom() -> Vec<u16> {
    strings::UTF8_BOM_CHARACTER.to_vec()
}

fn with_bom(text: &str) -> Vec<u16> {
    [utf8_bom(), u(text)].concat()
}

fn contains_uppercase(text: &str, ignore_escaped_chars: bool) -> bool {
    strings::contains_uppercase_character(&u(text), ignore_escaped_chars)
}

fn get_n_lines_of(text: &str, n: usize) -> String {
    t(strings::get_n_lines(&u(text), n))
}

fn remove_ansi(text: &str) -> String {
    t(strings::remove_ansi_escape_codes(&u(text)))
}

fn test_sequence(sequence: &str) {
    let text = format!("hello{sequence}world");
    assert_eq!(remove_ansi(&text), "helloworld", "expect to remove {sequence:?}");
    let units = u(&text);
    let parts: Vec<(bool, String)> = strings::for_ansi_string_parts(&units).iter().map(|p| (p.is_code, t(p.str))).collect();
    assert_eq!(
        parts,
        vec![(false, "hello".to_owned()), (true, sequence.to_owned()), (false, "world".to_owned())],
        "expect to forAnsiStringParts {sequence:?}"
    );
}

fn test_osc_sequence(prefix: &str, suffix: &str) {
    for content in ["633;SetMark;", "633;P;Cwd=/foo", "7;file://local/Users/me/foo/bar"] {
        test_sequence(&format!("{prefix}{content}{suffix}"));
    }
}

#[test]
fn equals_ignore_case() {
    let eq = |a: &str, b: &str| strings::equals_ignore_case(&u(a), &u(b));
    assert!(eq("", ""));
    assert!(!eq("", "1"));
    assert!(!eq("1", ""));

    assert!(eq("a", "a"));
    assert!(eq("abc", "Abc"));
    assert!(eq("abc", "ABC"));
    assert!(eq("Höhenmeter", "HÖhenmeter"));
    assert!(eq("ÖL", "Öl"));
}

#[test]
fn equals() {
    let eq = |a: Option<&str>, b: Option<&str>, ignore_case: bool| {
        strings::equals(a.map(u).as_deref(), b.map(u).as_deref(), ignore_case)
    };
    assert!(!eq(None, Some("abc"), false));
    assert!(!eq(Some("abc"), None, false));
    assert!(eq(None, None, false));
    assert!(eq(Some(""), Some(""), false));
    assert!(eq(Some("a"), Some("a"), false));
    assert!(!eq(Some("abc"), Some("Abc"), false));
    assert!(eq(Some("abc"), Some("ABC"), true));
    assert!(!eq(Some("Höhenmeter"), Some("HÖhenmeter"), false));
    assert!(!eq(Some("ÖL"), Some("Öl"), false));
    assert!(eq(Some("ÖL"), Some("Öl"), true));
}

#[test]
fn starts_with_ignore_case() {
    let sw = |a: &str, b: &str| strings::starts_with_ignore_case(&u(a), &u(b));
    assert!(sw("", ""));
    assert!(!sw("", "1"));
    assert!(sw("1", ""));

    assert!(sw("a", "a"));
    assert!(sw("abc", "Abc"));
    assert!(sw("abc", "ABC"));
    assert!(sw("Höhenmeter", "HÖhenmeter"));
    assert!(sw("ÖL", "Öl"));

    assert!(sw("alles klar", "a"));
    assert!(sw("alles klar", "A"));
    assert!(sw("alles klar", "alles k"));
    assert!(sw("alles klar", "alles K"));
    assert!(sw("alles klar", "ALLES K"));
    assert!(sw("alles klar", "alles klar"));
    assert!(sw("alles klar", "ALLES KLAR"));

    assert!(!sw("alles klar", " ALLES K"));
    assert!(!sw("alles klar", "ALLES K "));
    assert!(!sw("alles klar", "öALLES K "));
    assert!(!sw("alles klar", " "));
    assert!(!sw("alles klar", "ö"));
}

#[test]
fn ends_with_ignore_case() {
    let ew = |a: &str, b: &str| strings::ends_with_ignore_case(&u(a), &u(b));
    assert!(ew("", ""));
    assert!(!ew("", "1"));
    assert!(ew("1", ""));

    assert!(!ew("abcd", "abcde"));

    assert!(ew("a", "a"));
    assert!(ew("abc", "Abc"));
    assert!(ew("abc", "ABC"));
    assert!(ew("Höhenmeter", "HÖhenmeter"));
    assert!(ew("ÖL", "Öl"));

    assert!(ew("alles klar", "r"));
    assert!(ew("alles klar", "R"));
    assert!(ew("alles klar", "s klar"));
    assert!(ew("alles klar", "S klar"));
    assert!(ew("alles klar", "S KLAR"));
    assert!(ew("alles klar", "alles klar"));
    assert!(ew("alles klar", "ALLES KLAR"));

    assert!(!ew("alles klar", "S KLAR "));
    assert!(!ew("alles klar", " S KLAR"));
    assert!(!ew("alles klar", "S KLARö"));
    assert!(!ew("alles klar", " "));
    assert!(!ew("alles klar", "ö"));
}

#[test]
fn compare_ignore_case() {
    fn assert_compare_ignore_case(a: &str, b: &str, recurse: bool) {
        let (a_units, b_units) = (u(a), u(b));
        let actual = sign(strings::compare_ignore_case(&a_units, &b_units));
        let expected = sign(strings::compare(lower(&a_units).units(), lower(&b_units).units()));
        assert_eq!(actual, expected, "{a} <> {b}");

        if recurse {
            assert_compare_ignore_case(b, a, false);
        }
    }
    let check = |a: &str, b: &str| assert_compare_ignore_case(a, b, true);

    check("", "");
    check("abc", "ABC");
    check("abc", "ABc");
    check("abc", "ABcd");
    check("abc", "abcd");
    check("foo", "föo");
    check("Code", "code");
    check("Code", "cöde");

    check("B", "a");
    check("a", "B");
    check("b", "a");
    check("a", "b");

    check("aa", "ab");
    check("aa", "aB");
    check("aa", "aA");
    check("a", "aa");
    check("ab", "aA");
    check("O", "/");
}

#[test]
fn compare_ignore_case_substring() {
    #[allow(clippy::too_many_arguments)]
    fn assert_compare_ignore_case(a: &str, b: &str, a_start: usize, a_end: usize, b_start: usize, b_end: usize, recurse: bool) {
        let (a_units, b_units) = (u(a), u(b));
        let actual = sign(strings::compare_substring_ignore_case(&a_units, &b_units, a_start, a_end, b_start, b_end));

        let sub = |units: &[u16], start: usize, end: usize| lower(&units[start..end]);
        let expected = sign(strings::compare(sub(&a_units, a_start, a_end).units(), sub(&b_units, b_start, b_end).units()));
        assert_eq!(actual, expected, "{a} <> {b}");

        if recurse {
            assert_compare_ignore_case(b, a, b_start, b_end, a_start, a_end, false);
        }
    }
    let check = |a: &str, b: &str, a_start, a_end, b_start, b_end| {
        assert_compare_ignore_case(a, b, a_start, a_end, b_start, b_end, true)
    };

    check("", "", 0, 0, 0, 0);
    check("abc", "ABC", 0, 1, 0, 1);
    check("abc", "Aabc", 0, 3, 1, 4);
    check("abcABc", "ABcd", 3, 6, 0, 4);
    check("fluentui", "fluentu\u{130}", 0, 8, 0, 8);
}

#[test]
fn format() {
    assert_eq!(fmt("Foo Bar", &[]), "Foo Bar");
    assert_eq!(fmt("Foo {0} Bar", &[]), "Foo {0} Bar");
    assert_eq!(fmt("Foo {0} Bar", &["yes"]), "Foo yes Bar");
    assert_eq!(fmt("Foo {0} Bar {0}", &["yes"]), "Foo yes Bar yes");
    assert_eq!(fmt("Foo {0} Bar {1}{2}", &["yes"]), "Foo yes Bar {1}{2}");
    assert_eq!(fmt("Foo {0} Bar {1}{2}", &["yes", "undefined"]), "Foo yes Bar undefined{2}");
    assert_eq!(fmt("Foo {0} Bar {1}{2}", &["yes", "5", "false"]), "Foo yes Bar 5false");
    assert_eq!(fmt("Foo {0} Bar. {1}", &["(foo)", ".test"]), "Foo (foo) Bar. .test");
}

#[test]
fn format2() {
    assert_eq!(fmt2("Foo Bar", &[]), "Foo Bar");
    assert_eq!(fmt2("Foo {oops} Bar", &[]), "Foo {oops} Bar");
    assert_eq!(fmt2("Foo {foo} Bar", &[("foo", "bar")]), "Foo bar Bar");
    assert_eq!(fmt2("Foo {foo} Bar {foo}", &[("foo", "bar")]), "Foo bar Bar bar");
    assert_eq!(fmt2("Foo {foo} Bar {bar}{boo}", &[("foo", "bar")]), "Foo bar Bar {bar}{boo}");
    assert_eq!(fmt2("Foo {foo} Bar {bar}{boo}", &[("foo", "bar"), ("bar", "undefined")]), "Foo bar Bar undefined{boo}");
    assert_eq!(fmt2("Foo {foo} Bar {bar}{boo}", &[("foo", "bar"), ("bar", "5"), ("boo", "false")]), "Foo bar Bar 5false");
    assert_eq!(fmt2("Foo {foo} Bar. {bar}", &[("foo", "(foo)"), ("bar", ".test")]), "Foo (foo) Bar. .test");
}

#[test]
fn lcut() {
    assert_eq!(lcut_of("foo bar", 0, ""), "");
    assert_eq!(lcut_of("foo bar", 1, ""), "bar");
    assert_eq!(lcut_of("foo bar", 3, ""), "bar");
    assert_eq!(lcut_of("foo bar", 4, ""), "bar"); // Espaço à esquerda cortado
    assert_eq!(lcut_of("foo bar", 5, ""), "foo bar");
    assert_eq!(lcut_of("test string 0.1.2.3", 3, ""), "2.3");

    assert_eq!(lcut_of("foo bar", 0, "…"), "…");
    assert_eq!(lcut_of("foo bar", 1, "…"), "…bar");
    assert_eq!(lcut_of("foo bar", 3, "…"), "…bar");
    assert_eq!(lcut_of("foo bar", 4, "…"), "…bar"); // Espaço à esquerda cortado
    assert_eq!(lcut_of("foo bar", 5, "…"), "foo bar");
    assert_eq!(lcut_of("test string 0.1.2.3", 3, "…"), "…2.3");

    assert_eq!(lcut_of("", 10, ""), "");
    assert_eq!(lcut_of("a", 10, ""), "a");
    assert_eq!(lcut_of(" a", 10, ""), "a");
    assert_eq!(lcut_of("            a", 10, ""), "a");
    assert_eq!(lcut_of(" bbbb       a", 10, ""), "bbbb       a");
    assert_eq!(lcut_of("............a", 10, ""), "............a");

    assert_eq!(lcut_of("", 10, "…"), "");
    assert_eq!(lcut_of("a", 10, "…"), "a");
    assert_eq!(lcut_of(" a", 10, "…"), "a");
    assert_eq!(lcut_of("            a", 10, "…"), "a");
    assert_eq!(lcut_of(" bbbb       a", 10, "…"), "bbbb       a");
    assert_eq!(lcut_of("............a", 10, "…"), "............a");
}

#[test]
fn rcut() {
    assert_eq!(rcut_of("foo bar", 0, ""), "");
    assert_eq!(rcut_of("foo bar", 1, ""), "");
    assert_eq!(rcut_of("foo bar", 3, ""), "foo");
    assert_eq!(rcut_of("foo bar", 4, ""), "foo"); // Espaço à direita cortado
    assert_eq!(rcut_of("foo bar", 5, ""), "foo");
    assert_eq!(rcut_of("foo bar", 7, ""), "foo bar");
    assert_eq!(rcut_of("foo bar", 10, ""), "foo bar");
    assert_eq!(rcut_of("test string 0.1.2.3", 6, ""), "test");

    assert_eq!(rcut_of("foo bar", 0, "…"), "…");
    assert_eq!(rcut_of("foo bar", 1, "…"), "…");
    assert_eq!(rcut_of("foo bar", 3, "…"), "foo…");
    assert_eq!(rcut_of("foo bar", 4, "…"), "foo…"); // Espaço à direita cortado
    assert_eq!(rcut_of("foo bar", 5, "…"), "foo…");
    assert_eq!(rcut_of("foo bar", 7, "…"), "foo bar");
    assert_eq!(rcut_of("foo bar", 10, "…"), "foo bar");
    assert_eq!(rcut_of("test string 0.1.2.3", 6, "…"), "test…");

    assert_eq!(rcut_of("", 10, ""), "");
    assert_eq!(rcut_of("a", 10, ""), "a");
    assert_eq!(rcut_of("a ", 10, ""), "a");
    assert_eq!(rcut_of("a            ", 10, ""), "a");
    assert_eq!(rcut_of("a       bbbb ", 10, ""), "a       bbbb");
    assert_eq!(rcut_of("a............", 10, ""), "a............");

    assert_eq!(rcut_of("", 10, "…"), "");
    assert_eq!(rcut_of("a", 10, "…"), "a");
    assert_eq!(rcut_of("a ", 10, "…"), "a");
    assert_eq!(rcut_of("a            ", 10, "…"), "a");
    assert_eq!(rcut_of("a       bbbb ", 10, "…"), "a       bbbb");
    assert_eq!(rcut_of("a............", 10, "…"), "a............");
}

#[test]
fn escape() {
    let esc = |text: &str| t(strings::escape(&u(text)));
    assert_eq!(esc(""), "");
    assert_eq!(esc("foo"), "foo");
    assert_eq!(esc("foo bar"), "foo bar");
    assert_eq!(esc("<foo bar>"), "&lt;foo bar&gt;");
    assert_eq!(esc("<foo>Hello</foo>"), "&lt;foo&gt;Hello&lt;/foo&gt;");
}

#[test]
fn ltrim() {
    assert_eq!(ltrim_of("foo", "f"), "oo");
    assert_eq!(ltrim_of("foo", "o"), "foo");
    assert_eq!(ltrim_of("http://www.test.de", "http://"), "www.test.de");
    assert_eq!(ltrim_of("/foo/", "/"), "foo/");
    assert_eq!(ltrim_of("//foo/", "/"), "foo/");
    assert_eq!(ltrim_of("/", ""), "/");
    assert_eq!(ltrim_of("/", "/"), "");
    assert_eq!(ltrim_of("///", "/"), "");
    assert_eq!(ltrim_of("", ""), "");
    assert_eq!(ltrim_of("", "/"), "");
    // Agulha de vários caracteres com repetições consecutivas
    assert_eq!(ltrim_of("---hello", "---"), "hello");
    assert_eq!(ltrim_of("------hello", "---"), "hello");
    assert_eq!(ltrim_of("---------hello", "---"), "hello");
    assert_eq!(ltrim_of("hello---", "---"), "hello---");
}

#[test]
fn rtrim() {
    assert_eq!(rtrim_of("foo", "o"), "f");
    assert_eq!(rtrim_of("foo", "f"), "foo");
    assert_eq!(rtrim_of("http://www.test.de", ".de"), "http://www.test");
    assert_eq!(rtrim_of("/foo/", "/"), "/foo");
    assert_eq!(rtrim_of("/foo//", "/"), "/foo");
    assert_eq!(rtrim_of("/", ""), "/");
    assert_eq!(rtrim_of("/", "/"), "");
    assert_eq!(rtrim_of("///", "/"), "");
    assert_eq!(rtrim_of("", ""), "");
    assert_eq!(rtrim_of("", "/"), "");
    // Agulha de vários caracteres com repetições consecutivas (correção de bug)
    assert_eq!(rtrim_of("hello---", "---"), "hello");
    assert_eq!(rtrim_of("hello------", "---"), "hello");
    assert_eq!(rtrim_of("hello---------", "---"), "hello");
    assert_eq!(rtrim_of("---hello", "---"), "---hello");
    assert_eq!(rtrim_of(&format!("hello world{}", "---".repeat(10)), "---"), "hello world");
    assert_eq!(rtrim_of("path/to/file///", "//"), "path/to/file/");
}

#[test]
fn trim() {
    assert_eq!(trim_of(" foo ", " "), "foo");
    assert_eq!(trim_of("  foo", " "), "foo");
    assert_eq!(trim_of("bar  ", " "), "bar");
    assert_eq!(trim_of("   ", " "), "");
    assert_eq!(trim_of("foo bar", "bar"), "foo ");
}

#[test]
fn trim_whitespace() {
    let js = |text: &str| t(js_trim(&u(text)));
    assert_eq!(js(" foo "), "foo");
    assert_eq!(js("\t foo\t"), "foo");
    assert_eq!(js("  foo"), "foo");
    assert_eq!(js("bar  "), "bar");
    assert_eq!(js("   "), "");
    assert_eq!(js(" \t  "), "");
}

#[test]
fn last_non_whitespace_index() {
    assert_eq!(last_non_ws("abc  \t \t ", None), Some(2));
    assert_eq!(last_non_ws("abc", None), Some(2));
    assert_eq!(last_non_ws("abc\t", None), Some(2));
    assert_eq!(last_non_ws("abc ", None), Some(2));
    assert_eq!(last_non_ws("abc  \t \t ", None), Some(2));
    assert_eq!(last_non_ws("abc  \t \t abc \t \t ", None), Some(11));
    assert_eq!(last_non_ws("abc  \t \t abc \t \t ", Some(8)), Some(2));
    assert_eq!(last_non_ws("  \t \t ", None), None);
}

#[test]
fn contains_rtl() {
    let rtl = |text: &[u16]| strings::contains_rtl(text);
    assert!(!rtl(&u("a")));
    assert!(!rtl(&u("")));
    assert!(!rtl(&with_bom("a")));
    assert!(!rtl(&u("hello world!")));
    assert!(!rtl(&u("a📚📚b")));
    assert!(rtl(&u("هناك حقيقة مثبتة منذ زمن طويل")));
    assert!(rtl(&u("זוהי עובדה מבוססת שדעתו")));
}

#[test]
fn issue_115221_is_emoji_imprecise_misses_star() {
    let star = u("⭐");
    let code_point = strings::get_next_code_point(&star, star.len() as i64, 0);
    assert!(strings::is_emoji_imprecise(code_point));
}

#[test]
fn is_full_width_character() {
    let full = |c: char| strings::is_full_width_character(u(&c.to_string())[0]);
    // ASCII de largura total (FF01-FF5E)
    assert!(full('Ａ'), "Ａ U+FF21 fullwidth A");
    assert!(full('？'), "？ U+FF1F fullwidth question mark");
    assert!(full('＃'), "＃ U+FF03 fullwidth number sign");
    assert!(full('＝'), "＝ U+FF1D fullwidth equals sign");

    // Hiragana (3040-309F)
    assert!(full('あ'), "あ U+3042 hiragana");

    // Símbolos de largura total (FFE0-FFE6)
    assert!(full('￥'), "￥ U+FFE5 fullwidth yen sign");

    // ASCII comum não é largo
    assert!(!full('A'), "A regular ASCII");
    assert!(!full('?'), "? regular ASCII");
}

#[test]
fn is_basic_ascii() {
    fn assert_is_basic_ascii(text: &[u16], expected: bool) {
        assert_eq!(strings::is_basic_ascii(text), expected, "{} ({:?})", t(text), text.first());
    }
    let check = |text: &str, expected: bool| assert_is_basic_ascii(&u(text), expected);
    check("abcdefghijklmnopqrstuvwxyz", true);
    check("ABCDEFGHIJKLMNOPQRSTUVWXYZ", true);
    check("1234567890", true);
    check("`~!@#$%^&*()-_=+[{]}\\|;:'\",<.>/?", true);
    check(" ", true);
    check("\t", true);
    check("\n", true);
    check("\r", true);

    let mut all = u("\r\t\n");
    all.extend(32u16..127);
    assert_is_basic_ascii(&all, true);

    assert_is_basic_ascii(&[31], false);
    assert_is_basic_ascii(&[127], false);
    check("ü", false);
    check("a📚📚b", false);
}

#[test]
fn get_leading_whitespace() {
    assert_eq!(leading_ws("  foo", None, None), "  ");
    assert_eq!(leading_ws("  foo", Some(2), None), "");
    assert_eq!(leading_ws("  foo", Some(1), Some(1)), "");
    assert_eq!(leading_ws("  foo", Some(0), Some(1)), " ");
    assert_eq!(leading_ws("  ", None, None), "  ");
    assert_eq!(leading_ws("  ", Some(1), None), " ");
    assert_eq!(leading_ws("  ", Some(0), Some(1)), " ");
    assert_eq!(leading_ws("\t\tfunction foo(){", Some(0), Some(1)), "\t");
    assert_eq!(leading_ws("\t\tfunction foo(){", Some(0), Some(2)), "\t\t");
}

#[test]
fn fuzzy_contains() {
    let fuzzy = |target: &str, query: &str| strings::fuzzy_contains(&u(target), &u(query));
    assert!(!fuzzy("", "")); // (undefined, null) no original
    assert!(fuzzy("hello world", "h"));
    assert!(!fuzzy("hello world", "q"));
    assert!(fuzzy("hello world", "hw"));
    assert!(fuzzy("hello world", "horl"));
    assert!(fuzzy("hello world", "d"));
    assert!(!fuzzy("hello world", "wh"));
    assert!(!fuzzy("d", "dd"));
    assert!(fuzzy("hello world", "H"));
    assert!(fuzzy("Explorer", "E"));
    assert!(fuzzy("hello world", "HW"));
    // `toLowerCase()` pode alongar a consulta (İ -> i̇); toda unidade em minúsculas ainda precisa casar
    assert!(fuzzy("\u{130}ab", "\u{130}b"));
    assert!(!fuzzy("\u{130}ab", "\u{130}x"));
}

#[test]
fn starts_with_utf8_bom() {
    let bom = |text: &[u16]| strings::starts_with_utf8_bom(text);
    assert!(bom(&utf8_bom()));
    assert!(bom(&with_bom("a")));
    assert!(bom(&with_bom("aaaaaaaaaa")));
    assert!(!bom(&[u(" "), utf8_bom()].concat()));
    assert!(!bom(&u("foo")));
    assert!(!bom(&u("")));
}

#[test]
fn strip_utf8_bom() {
    let strip = |text: &[u16]| t(strings::strip_utf8_bom(text));
    assert_eq!(strip(&utf8_bom()), "");
    assert_eq!(strip(&with_bom("foobar")), "foobar");
    assert_eq!(strip(&[u("foobar"), utf8_bom()].concat()), t([u("foobar"), utf8_bom()].concat()));
    assert_eq!(strip(&u("abc")), "abc");
    assert_eq!(strip(&u("")), "");
}

#[test]
fn contains_uppercase_character() {
    for (text, result) in [
        ("", false), // `null` no original
        ("", false),
        ("foo", false),
        ("föö", false),
        ("ناك", false),
        ("מבוססת", false),
        ("😀", false),
        ("(#@()*&%()@*#&09827340982374}{:\">?></'\\~`", false),
        ("Foo", true),
        ("FOO", true),
        ("FöÖ", true),
        ("FöÖ", true),
        ("\\Foo", true),
    ] {
        assert_eq!(contains_uppercase(text, false), result, "Wrong result for {text}");
    }
}

#[test]
fn contains_uppercase_character_ignore_escaped_chars() {
    for (text, result) in [("\\Woo", false), ("f\\S\\S", false), ("foo", false), ("Foo", true)] {
        assert_eq!(contains_uppercase(text, true), result, "Wrong result for {text}");
    }
}

#[test]
fn uppercase_first_letter() {
    for (input, result) in [("", ""), ("foo", "Foo"), ("f", "F"), ("123", "123"), (".a", ".a")] {
        assert_eq!(t(strings::uppercase_first_letter(&u(input))), result, "Wrong result for {input}");
    }
}

#[test]
fn get_n_lines() {
    assert_eq!(get_n_lines_of("", 5), "");
    assert_eq!(get_n_lines_of("foo", 5), "foo");
    assert_eq!(get_n_lines_of("foo\nbar", 5), "foo\nbar");
    assert_eq!(get_n_lines_of("foo\nbar", 2), "foo\nbar");

    assert_eq!(get_n_lines_of("foo\nbar", 1), "foo");
    assert_eq!(get_n_lines_of("foo\nbar", 1), "foo"); // `getNLines('foo\nbar')`: o padrão de `n` é 1, aqui explícito
    assert_eq!(get_n_lines_of("foo\nbar\nsomething", 2), "foo\nbar");
    assert_eq!(get_n_lines_of("foo", 0), "");
}

#[test]
fn get_grapheme_break_type() {
    assert_eq!(strings::get_grapheme_break_type(0xBC1), GraphemeBreakType::SpacingMark);
}

#[test]
fn truncate() {
    let trunc = |text: &str, max: usize| t(strings::truncate(&u(text), max, &strings::ELLIPSIS));
    assert_eq!("hello world", trunc("hello world", 100));
    assert_eq!("hello…", trunc("hello world", 5));
}

#[test]
fn truncate_middle() {
    let trunc = |text: &str, max: usize| t(strings::truncate_middle(&u(text), max, &strings::ELLIPSIS));
    assert_eq!("hello world", trunc("hello world", 100));
    assert_eq!("he…ld", trunc("hello world", 5));
    assert_eq!("a…de", trunc("a😀bcde", 5));
    assert_eq!("ab…f", trunc("abcde😀f", 5));
}

#[test]
fn csi_sequences() {
    const CSI: &str = "\x1b[";
    let sequences = [
        // Casos base de https://invisible-island.net/xterm/ctlseqs/ctlseqs.html#h3-Functions-using-CSI-_-ordered-by-the-final-character_s_
        "42@",
        "42 @",
        "42A",
        "42 A",
        "42B",
        "42C",
        "42D",
        "42E",
        "42F",
        "42G",
        "42;42H",
        "42I",
        "42J",
        "?42J",
        "42K",
        "?42K",
        "42L",
        "42M",
        "42P",
        "#P",
        "3#P",
        "#Q",
        "3#Q",
        "#R",
        "42S",
        "?1;2;3S",
        "42T",
        "42;42;42;42;42T",
        ">3T",
        "42X",
        "42Z",
        "42^",
        "42`",
        "42a",
        "42b",
        "42c",
        "=42c",
        ">42c",
        "42d",
        "42e",
        "42;42f",
        "42g",
        "3h",
        "?3h",
        "42i",
        "?42i",
        "3l",
        "?3l",
        "3m",
        ">0;0m",
        ">0m",
        "?0m",
        "42n",
        ">42n",
        "?42n",
        ">42p",
        "!p",
        "0;0\"p",
        "42$p",
        "?42$p",
        "#p",
        "3#p",
        ">42q",
        "42q",
        "42 q",
        "42\"q",
        "#q",
        "42;42r",
        "?3r",
        "0;0;0;0;3$r",
        "s",
        "0;0s",
        ">42s",
        "?3s",
        "42;42;42t",
        ">3t",
        "42 t",
        "0;0;0;0;3$t",
        "u",
        "42 u",
        "0;0;0;0;0;0;0;0$v",
        "42$w",
        "0;0;0;0'w",
        "42x",
        "42*x",
        "0;0;0;0;0$x",
        "42#y",
        "0;0;0;0;0;0*y",
        "42;0'z",
        "0;1;2;4$z",
        "3'{",
        "#{",
        "3#{",
        "0;0;0;0${",
        "0;0;0;0#|",
        "42$|",
        "42'|",
        "42*|",
        "#}",
        "42'}",
        "42$}",
        "42'~",
        "42$~",
        "1;31m",
        "105m",
        "48:5:128m",
        "48;5;128m",
        "38:2:0:255:255:255m",
        "38;2;255;255;255m",
    ];

    for sequence in sequences {
        test_sequence(&format!("{CSI}{sequence}"));
    }
}

#[test]
fn esc_bracket_ps_pt_esc_backslash() {
    test_osc_sequence("\x1b]", "\x1b\\");
}

#[test]
fn esc_bracket_ps_pt_bel() {
    test_osc_sequence("\x1b]", "\x07");
}

#[test]
fn esc_bracket_ps_pt_st() {
    test_osc_sequence("\x1b]", "\u{9c}");
}

#[test]
fn osc_ps_pt_esc_backslash() {
    test_osc_sequence("\u{9d}", "\x1b\\");
}

#[test]
fn osc_ps_pt_bel() {
    test_osc_sequence("\u{9d}", "\x07");
}

#[test]
fn osc_ps_pt_st() {
    test_osc_sequence("\u{9d}", "\u{9c}");
}

#[test]
fn esc_sequences() {
    let sequence_content = [
        " F",
        " G",
        " L",
        " M",
        " N",
        "#3",
        "#4",
        "#5",
        "#6",
        "#8",
        "%@",
        "%G",
        "(C",
        ")C",
        "*C",
        "+C",
        "-C",
        ".C",
        "/C",
    ];
    for content in sequence_content {
        test_sequence(&format!("\x1b{content}"));
    }
}

#[test]
fn issue_209937() {
    assert_eq!(remove_ansi("localhost:\x1b[31m1234"), "localhost:1234");
}

#[test]
fn remove_ansi_escape_codes_from_prompt() {
    let remove = |text: &str| t(strings::remove_ansi_escape_codes_from_prompt(&u(text)));
    assert_eq!(remove("\u{1b}[31m$ \u{1b}[0m"), "$ ");
    assert_eq!(
        remove("\n\\[\u{1b}[01;34m\\]\\w\\[\u{1b}[00m\\]\n\\[\u{1b}[1;32m\\]> \\[\u{1b}[0m\\]"),
        "\n\\w\n> "
    );
}

#[test]
fn count() {
    let count = |value: &str, substr: &str| strings::count(&u(value), &u(substr));
    assert_eq!(count("hello world", "o"), 2);
    assert_eq!(count("hello world", "l"), 3);
    assert_eq!(count("hello world", "z"), 0);
    assert_eq!(count("hello world", "hello"), 1);
    assert_eq!(count("hello world", "world"), 1);
    assert_eq!(count("hello world", "hello world"), 1);
    assert_eq!(count("hello world", "foo"), 0);
}

#[test]
fn contains_ambiguous_character() {
    let ambiguous = |text: &str| AmbiguousCharacters::get_instance(Vec::<&str>::new()).contains_ambiguous_character(&u(text));
    assert!(!ambiguous("abcd"));
    assert!(!ambiguous("üå"));
    assert!(!ambiguous("(*&^)"));

    assert!(ambiguous("ο"));
    assert!(ambiguous("abɡc"));
}

#[test]
fn contains_invisible_character() {
    let invisible = |text: &str| InvisibleCharacters::contains_invisible_character(&u(text));
    assert!(!invisible("abcd"));
    assert!(invisible(" "));
    assert!(invisible("a\u{e004e}b"));
    assert!(invisible("a\u{e015a}\u{000b}b"));
}

#[test]
fn multibyte_aware_btoa() {
    assert!(!strings::multibyte_aware_btoa(&u("hello world")).is_empty());
    assert!(!strings::multibyte_aware_btoa(&u("平仮名")).is_empty());
    assert!(!strings::multibyte_aware_btoa(&u(&"vs".repeat(100000))).is_empty()); // https://github.com/microsoft/vscode/issues/112013
}

#[test]
fn html_attribute_encode_value() {
    let encode = |text: &str| t(strings::html_attribute_encode_value(&u(text)));
    assert_eq!(encode(""), "");
    assert_eq!(encode("abc"), "abc");
    assert_eq!(encode("<script>alert(\"Hello\")</script>"), "&lt;script&gt;alert(&quot;Hello&quot;)&lt;/script&gt;");
    assert_eq!(encode("Hello & World"), "Hello &amp; World");
    assert_eq!(encode("\"Hello\""), "&quot;Hello&quot;");
    assert_eq!(encode("'Hello'"), "&apos;Hello&apos;");
    assert_eq!(encode("<>&'\""), "&lt;&gt;&amp;&apos;&quot;");
}
