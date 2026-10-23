//! Testes próprios de `search.rs` (`buildReplaceStringWithCasePreserved`), que o VS Code só cobre indiretamente
//! pelo `replacePattern.test.ts` do editor. Não é porte de teste do VS Code. Os resultados esperados foram
//! obtidos rodando o `search.ts` real no bun.

use je_base::js_string::JsString;
use je_base::search::build_replace_string_with_case_preserved;

fn run(matched: Option<&str>, pattern: &str) -> String {
    let matches = matched.map(|m| vec![JsString::from(m)]);
    build_replace_string_with_case_preserved(matches.as_deref(), &JsString::from(pattern)).to_string_lossy()
}

#[test]
fn preserves_the_case_of_the_matched_text() {
    let cases: [(Option<&str>, &str, &str); 23] = [
        (Some("abc"), "def", "def"),
        (Some("Abc"), "def", "Def"),
        (Some("ABC"), "def", "DEF"),
        (Some("aBc"), "Def", "def"),
        (Some("abc-def"), "ghi-jkl", "ghi-jkl"),
        (Some("Abc-Def"), "ghi-jkl", "Ghi-Jkl"),
        (Some("abc_def"), "ghi_jkl", "ghi_jkl"),
        (Some("ABC_DEF"), "ghi_jkl", "GHI_JKL"),
        (Some("abc-def"), "ghi_jkl", "ghi_jkl"),
        (Some("abc-def_x"), "a-b", "a-b"),
        (Some("Abc-def"), "x", "X"),
        (None, "zz", "zz"),
        (Some(""), "zz", "zz"),
        (Some("\u{df}x"), "\u{df}a", "\u{df}a"),
        (Some("\u{c9}a"), "\u{e9}b", "\u{c9}b"),
        (Some("1a"), "Zb", "zb"),
        (Some("1A"), "zb", "ZB"),
        (Some("a"), "", ""),
        (Some("Ab"), "", ""),
        (Some("aB-cD"), "xy-zw", "xy-zw"),
        (Some("A-b"), "x-y", "X-y"),
        (Some("a-B_c"), "x-y_z", "x-y_z"),
        (Some("a_b-c"), "x_y", "x_y"),
    ];
    for (matched, pattern, expected) in cases {
        assert_eq!(run(matched, pattern), expected, "{matched:?} -> {pattern:?}");
    }
}
