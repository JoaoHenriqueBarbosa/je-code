//! Porte de `src/vs/base/test/common/charCode.test.ts` (suíte `CharCode`), caso a caso e na mesma ordem.
//!
//! O `CharCode` do original é um `enum` do TypeScript; aqui são as constantes `u16` de `char_code`.

#![allow(non_snake_case)]

mod common;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::char_code as CharCode;

#[test]
fn has_good_values() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    fn assert_value(actual: u16, expected: &str) {
        // `expected.charCodeAt(0)`
        let expected_code = expected.encode_utf16().next().expect("caractere esperado");
        assert_eq!(actual, expected_code, "char code ok for <<{expected}>>");
    }

    assert_value(CharCode::Tab, "\t");
    assert_value(CharCode::LineFeed, "\n");
    assert_value(CharCode::CarriageReturn, "\r");
    assert_value(CharCode::Space, " ");
    assert_value(CharCode::ExclamationMark, "!");
    assert_value(CharCode::DoubleQuote, "\"");
    assert_value(CharCode::Hash, "#");
    assert_value(CharCode::DollarSign, "$");
    assert_value(CharCode::PercentSign, "%");
    assert_value(CharCode::Ampersand, "&");
    assert_value(CharCode::SingleQuote, "'");
    assert_value(CharCode::OpenParen, "(");
    assert_value(CharCode::CloseParen, ")");
    assert_value(CharCode::Asterisk, "*");
    assert_value(CharCode::Plus, "+");
    assert_value(CharCode::Comma, ",");
    assert_value(CharCode::Dash, "-");
    assert_value(CharCode::Period, ".");
    assert_value(CharCode::Slash, "/");

    assert_value(CharCode::Digit0, "0");
    assert_value(CharCode::Digit1, "1");
    assert_value(CharCode::Digit2, "2");
    assert_value(CharCode::Digit3, "3");
    assert_value(CharCode::Digit4, "4");
    assert_value(CharCode::Digit5, "5");
    assert_value(CharCode::Digit6, "6");
    assert_value(CharCode::Digit7, "7");
    assert_value(CharCode::Digit8, "8");
    assert_value(CharCode::Digit9, "9");

    assert_value(CharCode::Colon, ":");
    assert_value(CharCode::Semicolon, ";");
    assert_value(CharCode::LessThan, "<");
    assert_value(CharCode::Equals, "=");
    assert_value(CharCode::GreaterThan, ">");
    assert_value(CharCode::QuestionMark, "?");
    assert_value(CharCode::AtSign, "@");

    assert_value(CharCode::A, "A");
    assert_value(CharCode::B, "B");
    assert_value(CharCode::C, "C");
    assert_value(CharCode::D, "D");
    assert_value(CharCode::E, "E");
    assert_value(CharCode::F, "F");
    assert_value(CharCode::G, "G");
    assert_value(CharCode::H, "H");
    assert_value(CharCode::I, "I");
    assert_value(CharCode::J, "J");
    assert_value(CharCode::K, "K");
    assert_value(CharCode::L, "L");
    assert_value(CharCode::M, "M");
    assert_value(CharCode::N, "N");
    assert_value(CharCode::O, "O");
    assert_value(CharCode::P, "P");
    assert_value(CharCode::Q, "Q");
    assert_value(CharCode::R, "R");
    assert_value(CharCode::S, "S");
    assert_value(CharCode::T, "T");
    assert_value(CharCode::U, "U");
    assert_value(CharCode::V, "V");
    assert_value(CharCode::W, "W");
    assert_value(CharCode::X, "X");
    assert_value(CharCode::Y, "Y");
    assert_value(CharCode::Z, "Z");

    assert_value(CharCode::OpenSquareBracket, "[");
    assert_value(CharCode::Backslash, "\\");
    assert_value(CharCode::CloseSquareBracket, "]");
    assert_value(CharCode::Caret, "^");
    assert_value(CharCode::Underline, "_");
    assert_value(CharCode::BackTick, "`");

    assert_value(CharCode::a, "a");
    assert_value(CharCode::b, "b");
    assert_value(CharCode::c, "c");
    assert_value(CharCode::d, "d");
    assert_value(CharCode::e, "e");
    assert_value(CharCode::f, "f");
    assert_value(CharCode::g, "g");
    assert_value(CharCode::h, "h");
    assert_value(CharCode::i, "i");
    assert_value(CharCode::j, "j");
    assert_value(CharCode::k, "k");
    assert_value(CharCode::l, "l");
    assert_value(CharCode::m, "m");
    assert_value(CharCode::n, "n");
    assert_value(CharCode::o, "o");
    assert_value(CharCode::p, "p");
    assert_value(CharCode::q, "q");
    assert_value(CharCode::r, "r");
    assert_value(CharCode::s, "s");
    assert_value(CharCode::t, "t");
    assert_value(CharCode::u, "u");
    assert_value(CharCode::v, "v");
    assert_value(CharCode::w, "w");
    assert_value(CharCode::x, "x");
    assert_value(CharCode::y, "y");
    assert_value(CharCode::z, "z");

    assert_value(CharCode::OpenCurlyBrace, "{");
    assert_value(CharCode::Pipe, "|");
    assert_value(CharCode::CloseCurlyBrace, "}");
    assert_value(CharCode::Tilde, "~");
}
