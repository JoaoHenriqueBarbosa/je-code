//! A API pública: flags, nomes de grupo, os limites de `exec` e o laço do `matchAll` que o chamador
//! escreve por cima (`lastIndex` com `g`, avanço por ponto de código em `u`).
//!
//! Os valores esperados dos laços saem do Node (`"a1b22c333".matchAll(/\d+/g)`, `"😀x".matchAll(/(?:)/gu)`
//! e `.matchAll(/(?:)/g)`, `new RegExp("a", "dgimsuy").flags`, índices da flag `d`).

use zyarr::{ErrorKind, Flags, RegExp};

fn units(text: &str) -> Vec<u16> {
    text.encode_utf16().collect()
}

/// O `lastIndex` com `g`: parte de `last_index`, grava `end` de volta e, em casamento vazio, avança
/// `AdvanceStringIndex` (por ponto de código com `u`/`v`).
fn match_all(regexp: &RegExp, input: &[u16]) -> Vec<(usize, usize)> {
    let mut found = Vec::new();
    let mut last_index = 0;
    while let Some(result) = regexp.exec(input, last_index) {
        found.push((result.start(), result.end()));
        last_index = result.end();
        if result.start() == result.end() {
            last_index += 1;
            let split = regexp.flags().either_unicode()
                && input.get(result.end()).is_some_and(|unit| (0xD800..=0xDBFF).contains(unit))
                && input.get(result.end() + 1).is_some_and(|unit| (0xDC00..=0xDFFF).contains(unit));
            if split {
                last_index += 1;
            }
        }
        if last_index > input.len() {
            break;
        }
    }
    found
}

#[test]
fn regexp_is_send_and_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<RegExp>();
}

#[test]
fn flags_roundtrip_in_alphabetical_order() {
    let regexp = RegExp::from_str("a", "yimsgdu").unwrap();
    let flags = regexp.flags();
    assert_eq!(flags.to_flags_string(), "dgimsuy");
    assert!(flags.has_indices && flags.global && flags.ignore_case && flags.multiline);
    assert!(flags.dot_all && flags.unicode && flags.sticky && !flags.unicode_sets);
    assert_eq!(RegExp::from_str("a", "gv").unwrap().flags().to_flags_string(), "gv");
}

#[test]
fn parse_flags_matches_the_constructor_rules() {
    assert_eq!(Flags::parse(&units("")), Some(Flags::default()));
    assert!(Flags::parse(&units("gimsuyd")).is_some());
    assert_eq!(Flags::parse(&units("gg")), None);
    assert_eq!(Flags::parse(&units("uv")), None);
    assert_eq!(Flags::parse(&units("x")), None);
}

#[test]
fn invalid_flags_error_has_the_jsc_message() {
    let error = RegExp::from_str("a", "gg").unwrap_err();
    assert_eq!(error.kind(), ErrorKind::SyntaxError);
    assert_eq!(error.to_string(), "Invalid flags supplied to RegExp constructor.");
}

#[test]
fn syntax_error_has_the_jsc_message() {
    let error = RegExp::from_str("(", "").unwrap_err();
    assert_eq!(error.kind(), ErrorKind::SyntaxError);
    assert_eq!(error.message(), "Invalid regular expression: missing )");
    assert_eq!(error.to_string(), "Invalid regular expression: missing )");
}

#[test]
fn capture_metadata() {
    let regexp = RegExp::from_str("(a)(?<n>b)(c)(?<m>d)", "").unwrap();
    assert_eq!(regexp.capture_count(), 4);
    assert_eq!(
        regexp.capture_names(),
        &[None, Some("n".to_string()), None, Some("m".to_string())]
    );
    assert!(regexp.has_named_captures());
    let plain = RegExp::from_str("(a)", "").unwrap();
    assert!(!plain.has_named_captures());
    assert_eq!(plain.capture_names(), &[None]);
}

#[test]
fn match_accessors_and_named_group_lookup() {
    let regexp = RegExp::from_str("(?<y>\\d{4})-(?<m>\\d\\d)(?<x>z)?", "dg").unwrap();
    let input = units("x 2026-10");
    let result = regexp.exec(&input, 0).unwrap();
    // Node: `m.indices.groups.y` é [2, 6] e `lastIndex` termina em 9.
    assert_eq!(result.group("y"), Some(2..6));
    assert_eq!(result.group("m"), Some(7..9));
    assert_eq!(result.group("x"), None);
    assert_eq!(result.group("nope"), None);
    assert_eq!((result.start(), result.end()), (2, 9));
    assert_eq!(result.captures.len(), 4);
}

#[test]
fn start_past_the_end_is_no_match() {
    let regexp = RegExp::from_str("", "g").unwrap();
    let input = units("abc");
    assert!(regexp.exec(&input, 3).is_some());
    assert!(regexp.exec(&input, 4).is_none());
}

#[test]
fn exec_is_repeatable_and_stateless() {
    let regexp = RegExp::from_str("b+", "").unwrap();
    let input = units("abbbc");
    let first = regexp.exec(&input, 0);
    assert_eq!(first, regexp.exec(&input, 0));
    assert_eq!(first.unwrap().captures[0], Some(1..4));
}

#[test]
fn match_all_loop_with_g() {
    let regexp = RegExp::from_str("\\d+", "g").unwrap();
    assert_eq!(match_all(&regexp, &units("a1b22c333")), vec![(1, 2), (3, 5), (6, 9)]);
}

#[test]
fn match_all_empty_matches_advance_by_code_point_only_with_u() {
    let input = units("\u{1F600}x");
    // Node: índices 0, 2, 3 com `u` e 0, 1, 2, 3 sem `u`.
    let with_u = RegExp::from_str("(?:)", "gu").unwrap();
    assert_eq!(match_all(&with_u, &input).iter().map(|m| m.0).collect::<Vec<_>>(), vec![0, 2, 3]);
    let without_u = RegExp::from_str("(?:)", "g").unwrap();
    assert_eq!(match_all(&without_u, &input).iter().map(|m| m.0).collect::<Vec<_>>(), vec![0, 1, 2, 3]);
}

#[test]
fn from_utf16_accepts_lone_surrogates_in_pattern_and_input() {
    let pattern = [0xD83D_u16];
    let regexp = RegExp::new(&pattern, &[]).unwrap();
    let input = [0x61_u16, 0xD83D, 0x62];
    assert_eq!(regexp.exec(&input, 0).unwrap().captures[0], Some(1..2));
}

#[test]
fn many_captures_and_long_input() {
    let regexp = RegExp::from_str("(?:(a)|b)*c", "").unwrap();
    let mut text = "ab".repeat(20_000);
    text.push('c');
    let input = units(&text);
    let result = regexp.exec(&input, 0).unwrap();
    assert_eq!(result.captures[0], Some(0..input.len()));
}

#[test]
fn pathological_pattern_does_not_overflow_the_stack() {
    // 3000 parênteses aninhados: o JSC aceita ou recusa com erro, nunca estoura a pilha.
    let pattern = format!("{}a{}", "(".repeat(3000), ")".repeat(3000));
    match RegExp::from_str(&pattern, "") {
        Ok(regexp) => assert!(regexp.exec(&units("a"), 0).is_some()),
        Err(error) => assert!(error.message().starts_with("Invalid regular expression:")),
    }
}
