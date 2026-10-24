//! Porte de `src/vs/editor/test/common/model/linesTextBuffer/linesTextBufferBuilder.test.ts` (suíte `ModelBuilder`), caso a
//! caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` é o `LeakGuard` de `je-base/tests/common`, criado no começo de cada
//!   caso.
//! - `createTextBufferFactory(text).create(DefaultEndOfLine.LF)` devolve no original `{ disposable, textBuffer }`; no
//!   porte `create` devolve o buffer e o `disposable()` dele é o descarte.
//! - Os textos entram como UTF-16 (`support::u`), como no `string` do JavaScript; o fim de linha é comparado como
//!   unidades UTF-16 (`Eol::as_units`).
//! - `strings.UTF8_BOM_CHARACTER` é `je_base::strings::UTF8_BOM_CHARACTER` (U+FEFF).

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod support;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::lifecycle::IDisposable;
use je_base::strings::UTF8_BOM_CHARACTER;
use je_editor::model::DefaultEndOfLine;
use je_editor::text_model::create_text_buffer_factory;
use support::u;

fn test_text_buffer_factory(text: &[u16], eol: &str, might_contain_non_basic_ascii: bool, might_contain_rtl: bool) {
    let text_buffer = create_text_buffer_factory(text).create(DefaultEndOfLine::Lf);
    let disposable = text_buffer.disposable();

    assert_eq!(text_buffer.might_contain_non_basic_ascii(), might_contain_non_basic_ascii);
    assert_eq!(text_buffer.might_contain_rtl(), might_contain_rtl);
    assert_eq!(text_buffer.get_eol().as_units(), &u(eol)[..]);
    disposable.dispose();
}

fn check(text: &str, eol: &str, might_contain_non_basic_ascii: bool, might_contain_rtl: bool) {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    test_text_buffer_factory(&u(text), eol, might_contain_non_basic_ascii, might_contain_rtl);
}

#[test]
fn t1() {
    check("", "\n", false, false);
}

#[test]
fn t2() {
    check("Hello world", "\n", false, false);
}

#[test]
fn t3() {
    check("Hello world\nHow are you?", "\n", false, false);
}

#[test]
fn t4() {
    check("Hello world\nHow are you?\nIs everything good today?\nDo you enjoy the weather?", "\n", false, false);
}

#[test]
fn carriage_return_detection_1_crlf_2_lf() {
    check("Hello world\r\nHow are you?\nIs everything good today?\nDo you enjoy the weather?", "\n", false, false);
}

#[test]
fn carriage_return_detection_2_crlf_1_lf() {
    check("Hello world\r\nHow are you?\r\nIs everything good today?\nDo you enjoy the weather?", "\r\n", false, false);
}

#[test]
fn carriage_return_detection_3_crlf_0_lf() {
    check("Hello world\r\nHow are you?\r\nIs everything good today?\r\nDo you enjoy the weather?", "\r\n", false, false);
}

#[test]
fn bom_handling() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let mut text = UTF8_BOM_CHARACTER.to_vec();
    text.extend(u("Hello world!"));
    test_text_buffer_factory(&text, "\n", false, false);
}

#[test]
fn rtl_handling_2() {
    check("Hello world!זוהי עובדה מבוססת שדעתו", "\n", true, true);
}

#[test]
fn rtl_handling_3() {
    check("Hello world!זוהי \nעובדה מבוססת שדעתו", "\n", true, true);
}

#[test]
fn ascii_handling_1() {
    check("Hello world!!\nHow do you do?", "\n", false, false);
}

#[test]
fn ascii_handling_2() {
    check("Hello world!!\nHow do you do?Züricha📚📚b", "\n", true, false);
}
