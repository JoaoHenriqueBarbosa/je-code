//! Casos próprios (sem equivalente upstream), conferidos no Node real contra o `indentationGuesser.ts`.
//!
//! Conferência: o `indentationGuesser.ts` copiado para um diretório em `/tmp`, com o import de `CharCode`
//! trocado por um objeto com `Space`, `Tab` e `Comma` e o de `ITextBuffer` (só tipo) por `any`, rodado com
//! `node --experimental-strip-types` (Node 26) sobre um buffer mínimo que separa as linhas em `\r\n`,
//! `\r` e `\n` e responde `getLineCount`, `getLineLength`, `getLineContent` e `getLineCharCode`. Cada
//! par esperado `(tab_size, insert_spaces)` abaixo é a saída de lá. Aqui o texto passa pelo
//! `PieceTreeTextBuffer` de verdade.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod support;

use common::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite};
use je_editor::indentation_guesser::guess_indentation;
use support::create_text_buffer;

fn guess(ds: &LeakGuard, text: &str, default_tab_size: i32, default_insert_spaces: bool) -> (i32, bool) {
    let buffer = create_text_buffer(ds, &[text], false);
    let result = guess_indentation(&buffer, default_tab_size, default_insert_spaces);
    (result.tab_size, result.insert_spaces)
}

#[test]
fn empty_file() {
    let ds = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(guess(&ds, "", 4, true), (4, true));
    assert_eq!(guess(&ds, "", 3, false), (3, false));
}

#[test]
fn two_spaces() {
    let ds = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(guess(&ds, "a\n  b\n    c\n  d\ne", 4, true), (2, true));
    assert_eq!(guess(&ds, "a\n  b\n    c\n  d\ne", 4, false), (2, true));
}

#[test]
fn four_spaces() {
    let ds = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(guess(&ds, "function f() {\n    if (x) {\n        y();\n    }\n}", 2, true), (4, true));
    assert_eq!(guess(&ds, "function f() {\n    if (x) {\n        y();\n    }\n}", 2, false), (4, true));
}

#[test]
fn eight_spaces() {
    let ds = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(guess(&ds, "a\n        b\n                c\n        d\ne", 4, true), (8, true));
}

#[test]
fn tabs() {
    let ds = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(guess(&ds, "a\n\tb\n\t\tc\n\tb\nd", 4, true), (4, false));
    assert_eq!(guess(&ds, "a\n\tb\n\t\tc\n\tb\nd", 2, false), (2, false));
}

#[test]
fn mixed_tabs_and_spaces() {
    let ds = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(guess(&ds, "a\n\tb\n  c\n    d\n\te", 4, true), (2, true));
    assert_eq!(guess(&ds, "a\n\tb\n\t  c\n\t    d\n\te\n\t\tf", 4, true), (4, false));
    assert_eq!(guess(&ds, "a\n  b\n\tc\n    d\n\t\te\n      f", 8, false), (2, true));
}

#[test]
fn looks_like_alignment() {
    let ds = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(guess(&ds, "const a = b + c,\n      d = b - c;\nfoo\n  bar\n", 4, true), (2, true));
    assert_eq!(guess(&ds, "const a = b + c,\n      d = b - c;\nfoo\n    bar\n", 6, true), (6, true));
    assert_eq!(guess(&ds, "- item1,\n  - item2\n", 2, true), (2, true));
    assert_eq!(guess(&ds, "- item1,\n  - item2\n", 4, true), (4, true));
    assert_eq!(guess(&ds, "x = [1,\n     2,\n     3];\nif (y) {\n    z();\n}", 4, true), (4, true));
    assert_eq!(guess(&ds, "const a = b + c,\n      d = b - c;\n", 4, true), (4, true));
    assert_eq!(guess(&ds, "const a = b + c,\n      d = b - c;\n", 6, true), (6, true));
}

#[test]
fn blank_lines_are_ignored() {
    let ds = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(guess(&ds, "a\n\n  \n  b\n\n    c\n\t\n  d", 4, true), (2, true));
    assert_eq!(guess(&ds, "\n\n\n", 4, false), (4, false));
    assert_eq!(guess(&ds, "   \n\t\n  \t ", 2, false), (2, false));
}

#[test]
fn crlf_line_endings() {
    let ds = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(guess(&ds, "a\r\n  b\r\n    c\r\n  d", 4, true), (2, true));
    assert_eq!(guess(&ds, "a\r\n\tb\r\n\t\tc", 4, true), (4, false));
}

#[test]
fn two_wins_over_four_with_two_thirds() {
    let ds = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(guess(&ds, "a:\n  b:\n    c: 1\n  d:\n      e: 2\n  f:\n    g:\n      h: 3\ni:\n    j: 4", 4, true), (2, true));
    assert_eq!(guess(&ds, "a:\n    b:\n        c: 1\n    d:\n  e: 2\n", 4, true), (4, true));
}

#[test]
fn odd_tab_sizes() {
    let ds = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(guess(&ds, "a\n   b\n      c\n   d\ne", 4, true), (3, true));
    assert_eq!(guess(&ds, "a\n     b\n          c\n     d\ne", 4, true), (5, true));
}

#[test]
fn single_space_lines_do_not_count() {
    let ds = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(guess(&ds, "a\n b\n  c\n b\na", 4, true), (4, true));
}

/// 20 mil linhas: as 10 mil primeiras indentadas com 0, 3, 6 ou 9 espaços, as outras com tabulações,
/// que o palpite não pode ver (ele lê no máximo 10 mil linhas).
fn large() -> String {
    (0..20000)
        .map(|i| if i < 10000 { format!("{}line{i}", " ".repeat((i % 4) * 3)) } else { format!("{}tail{i}", "\t".repeat(i % 3)) })
        .collect::<Vec<_>>()
        .join("\n")
}

/// 12 mil linhas com `\r\n`, indentadas com 0, 1 ou 2 tabulações, e uma em cada sete vazia de conteúdo.
fn large_tabs() -> String {
    (0..12000)
        .map(|i| format!("{}{}", "\t".repeat(i % 3), if i % 7 == 0 { "" } else { "x" }))
        .collect::<Vec<_>>()
        .join("\r\n")
}

#[test]
fn large_file() {
    let ds = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(guess(&ds, &large(), 4, true), (3, true));
    assert_eq!(guess(&ds, &large_tabs(), 4, true), (4, false));
}
