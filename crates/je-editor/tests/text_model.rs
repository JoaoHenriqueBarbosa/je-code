//! Porte de `src/vs/editor/test/common/model/textModel.test.ts` (suítes `TextModelData.fromString`, `Editor Model - TextModel`, `TextModel.mightContainRTL` e `TextModel.createSnapshot`), caso a caso e na mesma ordem.
//!
//! Cada `suite` do original vira um `mod`.
//!
//! Notas de tradução:
//! - `Position` e `Range` guardam `i32`, então `NaN`, `undefined` e números fracionários não chegam ao
//!   modelo. O original os trata na entrada de `validatePosition`/`_validateRangeRelaxedNoAllocations`
//!   (`Math.floor`, e `NaN` ou não número vira 1). No porte essa conversão acontece na fronteira, quando o
//!   número do JavaScript vira `i32`: os casos que passam `NaN`, `undefined`, `Number.MIN_VALUE`,
//!   `Number.MAX_VALUE` ou fração usam `js_int`, que aplica a mesma regra (com saturação em `i32`), e o modelo
//!   recebe o inteiro resultante. O que o caso confere do modelo (o recorte no texto e nos pares
//!   substitutos) continua o mesmo.
//! - `createTextBuffer` devolve só o buffer; o `disposable` do original é `buffer.disposable()`.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod support;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::JsString;
use je_base::lifecycle::IDisposable;
use je_base::strings::UTF8_BOM_CHARACTER;
use je_editor::model::{EndOfLinePreference, IndentSize, RelaxedTextModelCreationOptions, TextModelUpdateOptions};
use je_editor::position::Position;
use je_editor::range::Range;
use je_editor::text_model::{TextModel, create_text_buffer};
use support::test_text_model::*;
use support::u;

/// O número do JavaScript que o original passa a `Position`/`Range`, já na forma que o modelo do original
/// usa: `Math.floor`, com `NaN` (e o `undefined`, aqui `None`) virando 1.
fn js_int(value: Option<f64>) -> i32 {
    match value {
        Some(value) if !value.is_nan() => value.floor() as i32,
        _ => 1,
    }
}

/// `Number.MIN_VALUE`: o menor número positivo do JavaScript.
const NUMBER_MIN_VALUE: f64 = 5e-324;

fn js_position(line_number: f64, column: f64) -> Position {
    Position::new(js_int(Some(line_number)), js_int(Some(column)))
}

fn js_range(start_line_number: f64, start_column: f64, end_line_number: f64, end_column: f64) -> Range {
    Range::new(
        js_int(Some(start_line_number)),
        js_int(Some(start_column)),
        js_int(Some(end_line_number)),
        js_int(Some(end_column)),
    )
}

fn test_guess_indentation(
    default_insert_spaces: bool,
    default_tab_size: i32,
    expected_insert_spaces: bool,
    expected_tab_size: i32,
    text: &[&str],
    msg: Option<&str>,
) {
    let m = create_text_model_with(
        text.join("\n").as_str(),
        None,
        RelaxedTextModelCreationOptions {
            tab_size: Some(default_tab_size),
            insert_spaces: Some(default_insert_spaces),
            detect_indentation: Some(true),
            ..Default::default()
        },
        None,
    );
    let r = m.get_options();
    m.dispose();

    let msg = msg.unwrap_or("");
    assert_eq!(r.insert_spaces, expected_insert_spaces, "{msg}");
    assert_eq!(r.tab_size, expected_tab_size, "{msg}");
}

/// `number | undefined | [number]` do `expectedTabSize`.
#[derive(Clone, Copy)]
enum ExpectedTabSize {
    /// `undefined`: não dá para adivinhar o `tabSize`.
    Unknown,
    /// `number`: dá para adivinhar o `tabSize`.
    Size(i32),
    /// `[number]`: só dá para adivinhar o `tabSize` quando `insertSpaces` é verdadeiro.
    OnlyWithSpaces(i32),
}

fn assert_guess(expected_insert_spaces: Option<bool>, expected_tab_size: ExpectedTabSize, text: &[&str], msg: Option<&str>) {
    match expected_insert_spaces {
        None => {
            // não dá para adivinhar o insertSpaces
            match expected_tab_size {
                ExpectedTabSize::Unknown => {
                    // não dá para adivinhar o tabSize
                    test_guess_indentation(true, 13370, true, 13370, text, msg);
                    test_guess_indentation(false, 13371, false, 13371, text, msg);
                }
                ExpectedTabSize::Size(expected_tab_size) => {
                    // dá para adivinhar o tabSize
                    test_guess_indentation(true, 13370, true, expected_tab_size, text, msg);
                    test_guess_indentation(false, 13371, false, expected_tab_size, text, msg);
                }
                ExpectedTabSize::OnlyWithSpaces(expected_tab_size) => {
                    // só dá para adivinhar o tabSize quando insertSpaces é verdadeiro
                    test_guess_indentation(true, 13370, true, expected_tab_size, text, msg);
                    test_guess_indentation(false, 13371, false, 13371, text, msg);
                }
            }
        }
        Some(expected_insert_spaces) => {
            // dá para adivinhar o insertSpaces
            match expected_tab_size {
                ExpectedTabSize::Unknown => {
                    // não dá para adivinhar o tabSize
                    test_guess_indentation(true, 13370, expected_insert_spaces, 13370, text, msg);
                    test_guess_indentation(false, 13371, expected_insert_spaces, 13371, text, msg);
                }
                ExpectedTabSize::Size(expected_tab_size) => {
                    // dá para adivinhar o tabSize
                    test_guess_indentation(true, 13370, expected_insert_spaces, expected_tab_size, text, msg);
                    test_guess_indentation(false, 13371, expected_insert_spaces, expected_tab_size, text, msg);
                }
                ExpectedTabSize::OnlyWithSpaces(expected_tab_size) => {
                    // só dá para adivinhar o tabSize quando insertSpaces é verdadeiro
                    if expected_insert_spaces {
                        test_guess_indentation(true, 13370, expected_insert_spaces, expected_tab_size, text, msg);
                        test_guess_indentation(false, 13371, expected_insert_spaces, expected_tab_size, text, msg);
                    } else {
                        test_guess_indentation(true, 13370, expected_insert_spaces, 13370, text, msg);
                        test_guess_indentation(false, 13371, expected_insert_spaces, 13371, text, msg);
                    }
                }
            }
        }
    }
}

mod text_model_data_from_string {
    use super::*;

    #[derive(Debug, PartialEq)]
    struct TextBufferData {
        eol: JsString,
        lines: Vec<JsString>,
        contains_rtl: bool,
        is_basic_ascii: bool,
    }

    fn expected(eol: &str, lines: &[&str], contains_rtl: bool, is_basic_ascii: bool) -> TextBufferData {
        TextBufferData {
            eol: JsString::from(eol),
            lines: lines.iter().map(|line| JsString::from(*line)).collect(),
            contains_rtl,
            is_basic_ascii,
        }
    }

    fn test_text_model_data_from_string(text: &str, expected: TextBufferData) {
        let text_buffer = create_text_buffer(text.into(), TextModel::default_creation_options().default_eol);
        let disposable = text_buffer.disposable();
        let actual = TextBufferData {
            eol: JsString::from(text_buffer.get_eol().as_units()),
            lines: text_buffer.get_lines_content(),
            contains_rtl: text_buffer.might_contain_rtl(),
            is_basic_ascii: !text_buffer.might_contain_non_basic_ascii(),
        };
        assert_eq!(actual, expected);
        disposable.dispose();
    }

    #[test]
    fn one_line_text() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        test_text_model_data_from_string("Hello world!", expected("\n", &["Hello world!"], false, true));
    }

    #[test]
    fn multiline_text() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        test_text_model_data_from_string(
            "Hello,\r\ndear friend\nHow\rare\r\nyou?",
            expected("\r\n", &["Hello,", "dear friend", "How", "are", "you?"], false, true),
        );
    }

    #[test]
    fn non_basic_ascii_1() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        test_text_model_data_from_string("Hello,\nZürich", expected("\n", &["Hello,", "Zürich"], false, false));
    }

    #[test]
    fn contains_rtl_1() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        test_text_model_data_from_string(
            "Hello,\nזוהי עובדה מבוססת שדעתו",
            expected("\n", &["Hello,", "זוהי עובדה מבוססת שדעתו"], true, false),
        );
    }

    #[test]
    fn contains_rtl_2() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        test_text_model_data_from_string(
            "Hello,\nهناك حقيقة مثبتة منذ زمن طويل",
            expected("\n", &["Hello,", "هناك حقيقة مثبتة منذ زمن طويل"], true, false),
        );
    }
}

mod editor_model_text_model {
    use super::*;

    #[test]
    fn text_model_does_not_use_events_internally() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        // Garante que todas as partes do modelo recebem os eventos do modelo de texto explicitamente, para
        // que um listener de fora não os receba antes das partes e veja o modelo num estado inconsistente.
        //
        // Basta conferir que não há listener no modelo de texto depois da criação.
        let text_model = TextModel::new(
            "".into(),
            PLAINTEXT_LANGUAGE_ID,
            TextModel::default_creation_options(),
            None,
            create_model_services(),
        );
        assert!(!text_model.has_listeners());
        text_model.dispose();
    }

    #[test]
    fn get_value_length_in_range() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();

        let m = create_text_model("My First Line\r\nMy Second Line\r\nMy Third Line");
        assert_eq!(m.get_value_length_in_range(&Range::new(1, 1, 1, 1)), "".len());
        assert_eq!(m.get_value_length_in_range(&Range::new(1, 1, 1, 2)), "M".len());
        assert_eq!(m.get_value_length_in_range(&Range::new(1, 2, 1, 3)), "y".len());
        assert_eq!(m.get_value_length_in_range(&Range::new(1, 1, 1, 14)), "My First Line".len());
        assert_eq!(m.get_value_length_in_range(&Range::new(1, 1, 2, 1)), "My First Line\r\n".len());
        assert_eq!(m.get_value_length_in_range(&Range::new(1, 2, 2, 1)), "y First Line\r\n".len());
        assert_eq!(m.get_value_length_in_range(&Range::new(1, 2, 2, 2)), "y First Line\r\nM".len());
        assert_eq!(m.get_value_length_in_range(&Range::new(1, 2, 2, 1000)), "y First Line\r\nMy Second Line".len());
        assert_eq!(m.get_value_length_in_range(&Range::new(1, 2, 3, 1)), "y First Line\r\nMy Second Line\r\n".len());
        assert_eq!(
            m.get_value_length_in_range(&Range::new(1, 2, 3, 1000)),
            "y First Line\r\nMy Second Line\r\nMy Third Line".len()
        );
        assert_eq!(
            m.get_value_length_in_range(&Range::new(1, 1, 1000, 1000)),
            "My First Line\r\nMy Second Line\r\nMy Third Line".len()
        );
        m.dispose();

        let m = create_text_model("My First Line\nMy Second Line\nMy Third Line");
        assert_eq!(m.get_value_length_in_range(&Range::new(1, 1, 1, 1)), "".len());
        assert_eq!(m.get_value_length_in_range(&Range::new(1, 1, 1, 2)), "M".len());
        assert_eq!(m.get_value_length_in_range(&Range::new(1, 2, 1, 3)), "y".len());
        assert_eq!(m.get_value_length_in_range(&Range::new(1, 1, 1, 14)), "My First Line".len());
        assert_eq!(m.get_value_length_in_range(&Range::new(1, 1, 2, 1)), "My First Line\n".len());
        assert_eq!(m.get_value_length_in_range(&Range::new(1, 2, 2, 1)), "y First Line\n".len());
        assert_eq!(m.get_value_length_in_range(&Range::new(1, 2, 2, 2)), "y First Line\nM".len());
        assert_eq!(m.get_value_length_in_range(&Range::new(1, 2, 2, 1000)), "y First Line\nMy Second Line".len());
        assert_eq!(m.get_value_length_in_range(&Range::new(1, 2, 3, 1)), "y First Line\nMy Second Line\n".len());
        assert_eq!(
            m.get_value_length_in_range(&Range::new(1, 2, 3, 1000)),
            "y First Line\nMy Second Line\nMy Third Line".len()
        );
        assert_eq!(
            m.get_value_length_in_range(&Range::new(1, 1, 1000, 1000)),
            "My First Line\nMy Second Line\nMy Third Line".len()
        );
        m.dispose();
    }

    #[test]
    fn get_value_length_in_range_different_eol() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();

        let m = create_text_model("My First Line\r\nMy Second Line\r\nMy Third Line");
        assert_eq!(
            m.get_value_length_in_range_with(&Range::new(1, 1, 2, 1), EndOfLinePreference::TextDefined),
            "My First Line\r\n".len()
        );
        assert_eq!(m.get_value_length_in_range_with(&Range::new(1, 1, 2, 1), EndOfLinePreference::CrLf), "My First Line\r\n".len());
        assert_eq!(m.get_value_length_in_range_with(&Range::new(1, 1, 2, 1), EndOfLinePreference::Lf), "My First Line\n".len());
        assert_eq!(
            m.get_value_length_in_range_with(&Range::new(1, 1, 1000, 1000), EndOfLinePreference::TextDefined),
            "My First Line\r\nMy Second Line\r\nMy Third Line".len()
        );
        assert_eq!(
            m.get_value_length_in_range_with(&Range::new(1, 1, 1000, 1000), EndOfLinePreference::CrLf),
            "My First Line\r\nMy Second Line\r\nMy Third Line".len()
        );
        assert_eq!(
            m.get_value_length_in_range_with(&Range::new(1, 1, 1000, 1000), EndOfLinePreference::Lf),
            "My First Line\nMy Second Line\nMy Third Line".len()
        );
        m.dispose();

        let m = create_text_model("My First Line\nMy Second Line\nMy Third Line");
        assert_eq!(
            m.get_value_length_in_range_with(&Range::new(1, 1, 2, 1), EndOfLinePreference::TextDefined),
            "My First Line\n".len()
        );
        assert_eq!(m.get_value_length_in_range_with(&Range::new(1, 1, 2, 1), EndOfLinePreference::Lf), "My First Line\n".len());
        assert_eq!(m.get_value_length_in_range_with(&Range::new(1, 1, 2, 1), EndOfLinePreference::CrLf), "My First Line\r\n".len());
        assert_eq!(
            m.get_value_length_in_range_with(&Range::new(1, 1, 1000, 1000), EndOfLinePreference::TextDefined),
            "My First Line\nMy Second Line\nMy Third Line".len()
        );
        assert_eq!(
            m.get_value_length_in_range_with(&Range::new(1, 1, 1000, 1000), EndOfLinePreference::Lf),
            "My First Line\nMy Second Line\nMy Third Line".len()
        );
        assert_eq!(
            m.get_value_length_in_range_with(&Range::new(1, 1, 1000, 1000), EndOfLinePreference::CrLf),
            "My First Line\r\nMy Second Line\r\nMy Third Line".len()
        );
        m.dispose();
    }

    #[test]
    /// `guess indentation 1`
    fn guess_indentation_1() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();

        assert_guess(None, ExpectedTabSize::Unknown, &[
            "x",
            "x",
            "x",
            "x",
            "x",
            "x",
            "x"
        ], Some("no clues"));

        assert_guess(Some(false), ExpectedTabSize::Unknown, &[
            "\tx",
            "x",
            "x",
            "x",
            "x",
            "x",
            "x"
        ], Some("no spaces, 1xTAB"));

        assert_guess(Some(true), ExpectedTabSize::Size(2), &[
            "  x",
            "x",
            "x",
            "x",
            "x",
            "x",
            "x"
        ], Some("1x2"));

        assert_guess(Some(false), ExpectedTabSize::Unknown, &[
            "\tx",
            "\tx",
            "\tx",
            "\tx",
            "\tx",
            "\tx",
            "\tx"
        ], Some("7xTAB"));

        assert_guess(None, ExpectedTabSize::OnlyWithSpaces(2), &[
            "\tx",
            "  x",
            "\tx",
            "  x",
            "\tx",
            "  x",
            "\tx",
            "  x",
        ], Some("4x2, 4xTAB"));
        assert_guess(Some(false), ExpectedTabSize::Unknown, &[
            "\tx",
            " x",
            "\tx",
            " x",
            "\tx",
            " x",
            "\tx",
            " x"
        ], Some("4x1, 4xTAB"));
        assert_guess(Some(false), ExpectedTabSize::Unknown, &[
            "\tx",
            "\tx",
            "  x",
            "\tx",
            "  x",
            "\tx",
            "  x",
            "\tx",
            "  x",
        ], Some("4x2, 5xTAB"));
        assert_guess(Some(false), ExpectedTabSize::Unknown, &[
            "\tx",
            "\tx",
            "x",
            "\tx",
            "x",
            "\tx",
            "x",
            "\tx",
            "  x",
        ], Some("1x2, 5xTAB"));
        assert_guess(Some(false), ExpectedTabSize::Unknown, &[
            "\tx",
            "\tx",
            "x",
            "\tx",
            "x",
            "\tx",
            "x",
            "\tx",
            "    x",
        ], Some("1x4, 5xTAB"));
        assert_guess(Some(false), ExpectedTabSize::Unknown, &[
            "\tx",
            "\tx",
            "x",
            "\tx",
            "x",
            "\tx",
            "  x",
            "\tx",
            "    x",
        ], Some("1x2, 1x4, 5xTAB"));

        assert_guess(None, ExpectedTabSize::Unknown, &[
            "x",
            " x",
            " x",
            " x",
            " x",
            " x",
            " x",
            " x"
        ], Some("7x1 - 1 space is never guessed as an indentation"));
        assert_guess(Some(true), ExpectedTabSize::Unknown, &[
            "x",
            "          x",
            " x",
            " x",
            " x",
            " x",
            " x",
            " x"
        ], Some("1x10, 6x1"));
        assert_guess(None, ExpectedTabSize::Unknown, &[
            "",
            "  ",
            "    ",
            "      ",
            "        ",
            "          ",
            "            ",
            "              ",
        ], Some("whitespace lines don't count"));
        assert_guess(Some(true), ExpectedTabSize::Size(3), &[
            "x",
            "   x",
            "   x",
            "    x",
            "x",
            "   x",
            "   x",
            "    x",
            "x",
            "   x",
            "   x",
            "    x",
        ], Some("6x3, 3x4"));
        assert_guess(Some(true), ExpectedTabSize::Size(5), &[
            "x",
            "     x",
            "     x",
            "    x",
            "x",
            "     x",
            "     x",
            "    x",
            "x",
            "     x",
            "     x",
            "    x",
        ], Some("6x5, 3x4"));
        assert_guess(Some(true), ExpectedTabSize::Size(7), &[
            "x",
            "       x",
            "       x",
            "     x",
            "x",
            "       x",
            "       x",
            "    x",
            "x",
            "       x",
            "       x",
            "    x",
        ], Some("6x7, 1x5, 2x4"));
        assert_guess(Some(true), ExpectedTabSize::Size(2), &[
            "x",
            "  x",
            "  x",
            "  x",
            "  x",
            "x",
            "  x",
            "  x",
            "  x",
            "  x",
        ], Some("8x2"));

        assert_guess(Some(true), ExpectedTabSize::Size(2), &[
            "x",
            "  x",
            "  x",
            "x",
            "  x",
            "  x",
            "x",
            "  x",
            "  x",
            "x",
            "  x",
            "  x",
        ], Some("8x2"));
        assert_guess(Some(true), ExpectedTabSize::Size(2), &[
            "x",
            "  x",
            "    x",
            "x",
            "  x",
            "    x",
            "x",
            "  x",
            "    x",
            "x",
            "  x",
            "    x",
        ], Some("4x2, 4x4"));
        assert_guess(Some(true), ExpectedTabSize::Size(2), &[
            "x",
            "  x",
            "  x",
            "    x",
            "x",
            "  x",
            "  x",
            "    x",
            "x",
            "  x",
            "  x",
            "    x",
        ], Some("6x2, 3x4"));
        assert_guess(Some(true), ExpectedTabSize::Size(2), &[
            "x",
            "  x",
            "  x",
            "    x",
            "    x",
            "x",
            "  x",
            "  x",
            "    x",
            "    x",
        ], Some("4x2, 4x4"));
        assert_guess(Some(true), ExpectedTabSize::Size(2), &[
            "x",
            "  x",
            "    x",
            "    x",
            "x",
            "  x",
            "    x",
            "    x",
        ], Some("2x2, 4x4"));
        assert_guess(Some(true), ExpectedTabSize::Size(4), &[
            "x",
            "    x",
            "    x",
            "x",
            "    x",
            "    x",
            "x",
            "    x",
            "    x",
            "x",
            "    x",
            "    x",
        ], Some("8x4"));
        assert_guess(Some(true), ExpectedTabSize::Size(2), &[
            "x",
            "  x",
            "    x",
            "    x",
            "      x",
            "x",
            "  x",
            "    x",
            "    x",
            "      x",
        ], Some("2x2, 4x4, 2x6"));
        assert_guess(Some(true), ExpectedTabSize::Size(2), &[
            "x",
            "  x",
            "    x",
            "    x",
            "      x",
            "      x",
            "        x",
        ], Some("1x2, 2x4, 2x6, 1x8"));
        assert_guess(Some(true), ExpectedTabSize::Size(4), &[
            "x",
            "    x",
            "    x",
            "    x",
            "     x",
            "        x",
            "x",
            "    x",
            "    x",
            "    x",
            "     x",
            "        x",
        ], Some("6x4, 2x5, 2x8"));
        assert_guess(Some(true), ExpectedTabSize::Size(4), &[
            "x",
            "    x",
            "    x",
            "    x",
            "     x",
            "        x",
            "        x",
        ], Some("3x4, 1x5, 2x8"));
        assert_guess(Some(true), ExpectedTabSize::Size(4), &[
            "x",
            "x",
            "    x",
            "    x",
            "     x",
            "        x",
            "        x",
            "x",
            "x",
            "    x",
            "    x",
            "     x",
            "        x",
            "        x",
        ], Some("6x4, 2x5, 4x8"));
        assert_guess(Some(true), ExpectedTabSize::Size(3), &[
            "x",
            " x",
            " x",
            " x",
            " x",
            " x",
            "x",
            "   x",
            "    x",
            "    x",
        ], Some("5x1, 2x0, 1x3, 2x4"));
        assert_guess(Some(false), ExpectedTabSize::Unknown, &[
            "\t x",
            " \t x",
            "\tx"
        ], Some("mixed whitespace 1"));
        assert_guess(Some(false), ExpectedTabSize::Unknown, &[
            "\tx",
            "\t    x"
        ], Some("mixed whitespace 2"));
    }

    #[test]
    /// `issue #44991: Wrong indentation size auto-detection`
    fn issue_44991_wrong_indentation_size_auto_detection() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        assert_guess(Some(true), ExpectedTabSize::Size(4), &[
            "a = 10             # 0 space indent",
            "b = 5              # 0 space indent",
            "if a > 10:         # 0 space indent",
            "    a += 1         # 4 space indent      delta 4 spaces",
            "    if b > 5:      # 4 space indent",
            "        b += 1     # 8 space indent      delta 4 spaces",
            "        b += 1     # 8 space indent",
            "        b += 1     # 8 space indent",
            "# comment line 1   # 0 space indent      delta 8 spaces",
            "# comment line 2   # 0 space indent",
            "# comment line 3   # 0 space indent",
            "        b += 1     # 8 space indent      delta 8 spaces",
            "        b += 1     # 8 space indent",
            "        b += 1     # 8 space indent",
        ], None);
    }

    #[test]
    /// `issue #55818: Broken indentation detection`
    fn issue_55818_broken_indentation_detection() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        assert_guess(Some(true), ExpectedTabSize::Size(2), &[
            "",
            "/* REQUIRE */",
            "",
            "const foo = require ( 'foo' ),",
            "      bar = require ( 'bar' );",
            "",
            "/* MY FN */",
            "",
            "function myFn () {",
            "",
            "  const asd = 1,",
            "        dsa = 2;",
            "",
            "  return bar ( foo ( asd ) );",
            "",
            "}",
            "",
            "/* EXPORT */",
            "",
            "module.exports = myFn;",
            "",
        ], None);
    }

    #[test]
    /// `issue #70832: Broken indentation detection`
    fn issue_70832_broken_indentation_detection() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        assert_guess(Some(false), ExpectedTabSize::Unknown, &[
            "x",
            "x",
            "x",
            "x",
            "\tx",
            "\t\tx",
            "    x",
            "\t\tx",
            "\tx",
            "\t\tx",
            "\tx",
            "\tx",
            "\tx",
            "\tx",
            "x",
        ], None);
    }

    #[test]
    /// `issue #62143: Broken indentation detection`
    fn issue_62143_broken_indentation_detection() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        // funciona antes da correção
        assert_guess(Some(true), ExpectedTabSize::Size(2), &[
            "x",
            "x",
            "  x",
            "  x"
        ], None);

        // funciona antes da correção
        assert_guess(Some(true), ExpectedTabSize::Size(2), &[
            "x",
            "  - item2",
            "  - item3"
        ], None);

        // funciona antes da correção
        test_guess_indentation(true, 2, true, 2, &[
            "x x",
            "  x",
            "  x",
        ], None);

        // falha antes da correção
        // espaço em branco no meio da linha quebra a adivinhação da indentação
        test_guess_indentation(true, 2, true, 2, &[
            "x x",
            "  x",
            "  x",
            "    x"
        ], None);

        test_guess_indentation(true, 2, true, 2, &[
            "<!--test1.md -->",
            "- item1",
            "  - item2",
            "    - item3"
        ], None);
    }

    #[test]
    /// `issue #84217: Broken indentation detection`
    fn issue_84217_broken_indentation_detection() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        assert_guess(Some(true), ExpectedTabSize::Size(4), &[
            "def main():",
            "    print('hello')",
        ], None);
        assert_guess(Some(true), ExpectedTabSize::Size(4), &[
            "def main():",
            "    with open('foo') as fp:",
            "        print(fp.read())",
        ], None);
    }

    #[test]
    /// `issue #65668: YAML file indented with 2 spaces`
    fn issue_65668_yaml_file_indented_with_2_spaces() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        // O arquivo YAML inteiro da issue: deve detectar 2 espaços
        assert_guess(Some(true), ExpectedTabSize::Size(2), &[
            "version: 2",
            "",
            "jobs:",
            "  build:",
            "    docker:",
            "      - circleci/golang:1.11",
            "",
            "  environment:",
            "    TEST_RESULTS: /tmp/test-results",
            "",
            "  steps:",
            "    - checkout",
            "    - run: mkdir -p $TEST_RESULTS",
            "",
            "    - restore_cache:",
            "        keys:",
            "          - v1-pkg-cache",
            "",
            "    - run:",
            "        name: dep ensure",
            "        command: dep ensure -v",
            "",
            "    - run:",
            "        name: Run unit tests",
            "        command: |",
            "          trap \"go-junit-report <${TEST_RESULTS}/go-test.out > ${TEST_RESULTS}/go-test-report.xml\" EXIT",
            "          go test -v ./... | tee ${TEST_RESULTS}/go-test.out",
            "",
            "    - run:",
            "        name: Build",
            "        command: go build -v",
            "",
            "    - save_cache:",
            "        key: v1-pkg-cache",
            "        paths:",
            "          - \"/go/pkg\"",
            "",
            "    - store_artifacts:",
            "        path: /tmp/test-results",
            "        destination: raw-test-output",
            "",
            "    - store_test_results:",
            "        path: /tmp/test-results",
        ], None);
    }

    #[test]
    /// `issue #249040: 4-space indent should win over 2-space when predominant`
    fn issue_249040_4_space_indent_should_win_over_2_space_when_predominant() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        // Arquivo quase todo indentado com 4 espaços, mas com algumas indentações de 2: deve detectar 4 espaços
        assert_guess(Some(true), ExpectedTabSize::Size(4), &[
            "function foo() {",
            "    let a = 1;",
            "    let b = 2;",
            "    if (true) {",
            "        console.log(a);",
            "        console.log(b);",
            "    }",
            "    const obj = {",
            "      x: 1,",  // indentação de 2 espaços aqui
            "      y: 2",   // indentação de 2 espaços aqui
            "    };",
            "    return obj;",
            "}",
        ], None);
    }


    #[test]
    fn validate_position() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();

        let m = create_text_model("line one\nline two");

        assert_eq!(m.validate_position(Position::new(0, 0)), Position::new(1, 1));
        assert_eq!(m.validate_position(Position::new(0, 1)), Position::new(1, 1));

        assert_eq!(m.validate_position(Position::new(1, 1)), Position::new(1, 1));
        assert_eq!(m.validate_position(Position::new(1, 2)), Position::new(1, 2));
        assert_eq!(m.validate_position(Position::new(1, 30)), Position::new(1, 9));

        assert_eq!(m.validate_position(Position::new(2, 0)), Position::new(2, 1));
        assert_eq!(m.validate_position(Position::new(2, 1)), Position::new(2, 1));
        assert_eq!(m.validate_position(Position::new(2, 2)), Position::new(2, 2));
        assert_eq!(m.validate_position(Position::new(2, 30)), Position::new(2, 9));

        assert_eq!(m.validate_position(Position::new(3, 0)), Position::new(2, 9));
        assert_eq!(m.validate_position(Position::new(3, 1)), Position::new(2, 9));
        assert_eq!(m.validate_position(Position::new(3, 30)), Position::new(2, 9));

        assert_eq!(m.validate_position(Position::new(30, 30)), Position::new(2, 9));

        assert_eq!(m.validate_position(js_position(-123.123, -0.5)), Position::new(1, 1));
        assert_eq!(m.validate_position(js_position(NUMBER_MIN_VALUE, NUMBER_MIN_VALUE)), Position::new(1, 1));

        assert_eq!(m.validate_position(js_position(f64::MAX, f64::MAX)), Position::new(2, 9));
        assert_eq!(m.validate_position(js_position(123.23, 47.5)), Position::new(2, 9));

        m.dispose();
    }

    #[test]
    fn validate_position_around_high_low_surrogate_pairs_1() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();

        let m = create_text_model("a📚b");

        assert_eq!(m.validate_position(Position::new(0, 0)), Position::new(1, 1));
        assert_eq!(m.validate_position(Position::new(0, 1)), Position::new(1, 1));
        assert_eq!(m.validate_position(Position::new(0, 7)), Position::new(1, 1));

        assert_eq!(m.validate_position(Position::new(1, 1)), Position::new(1, 1));
        assert_eq!(m.validate_position(Position::new(1, 2)), Position::new(1, 2));
        assert_eq!(m.validate_position(Position::new(1, 3)), Position::new(1, 2));
        assert_eq!(m.validate_position(Position::new(1, 4)), Position::new(1, 4));
        assert_eq!(m.validate_position(Position::new(1, 5)), Position::new(1, 5));
        assert_eq!(m.validate_position(Position::new(1, 30)), Position::new(1, 5));

        assert_eq!(m.validate_position(Position::new(2, 0)), Position::new(1, 5));
        assert_eq!(m.validate_position(Position::new(2, 1)), Position::new(1, 5));
        assert_eq!(m.validate_position(Position::new(2, 2)), Position::new(1, 5));
        assert_eq!(m.validate_position(Position::new(2, 30)), Position::new(1, 5));

        assert_eq!(m.validate_position(js_position(-123.123, -0.5)), Position::new(1, 1));
        assert_eq!(m.validate_position(js_position(NUMBER_MIN_VALUE, NUMBER_MIN_VALUE)), Position::new(1, 1));

        assert_eq!(m.validate_position(js_position(f64::MAX, f64::MAX)), Position::new(1, 5));
        assert_eq!(m.validate_position(js_position(123.23, 47.5)), Position::new(1, 5));

        m.dispose();
    }

    #[test]
    fn validate_position_around_high_low_surrogate_pairs_2() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();

        let m = create_text_model("a📚📚b");

        assert_eq!(m.validate_position(Position::new(1, 1)), Position::new(1, 1));
        assert_eq!(m.validate_position(Position::new(1, 2)), Position::new(1, 2));
        assert_eq!(m.validate_position(Position::new(1, 3)), Position::new(1, 2));
        assert_eq!(m.validate_position(Position::new(1, 4)), Position::new(1, 4));
        assert_eq!(m.validate_position(Position::new(1, 5)), Position::new(1, 4));
        assert_eq!(m.validate_position(Position::new(1, 6)), Position::new(1, 6));
        assert_eq!(m.validate_position(Position::new(1, 7)), Position::new(1, 7));

        m.dispose();
    }

    #[test]
    /// `validatePosition handle NaN.`
    fn validate_position_handle_nan() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();

        let m = create_text_model("line one\nline two");

        assert_eq!(m.validate_position(js_position(f64::NAN, 1.0)), Position::new(1, 1));
        assert_eq!(m.validate_position(js_position(1.0, f64::NAN)), Position::new(1, 1));

        assert_eq!(m.validate_position(js_position(f64::NAN, f64::NAN)), Position::new(1, 1));
        assert_eq!(m.validate_position(js_position(2.0, f64::NAN)), Position::new(2, 1));
        assert_eq!(m.validate_position(js_position(f64::NAN, 3.0)), Position::new(1, 3));

        m.dispose();
    }

    #[test]
    fn issue_71480_validate_position_handle_floats() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let m = create_text_model("line one\nline two");

        assert_eq!(m.validate_position(js_position(0.2, 1.0)), Position::new(1, 1), "a");
        assert_eq!(m.validate_position(js_position(1.2, 1.0)), Position::new(1, 1), "b");
        assert_eq!(m.validate_position(js_position(1.5, 2.0)), Position::new(1, 2), "c");
        assert_eq!(m.validate_position(js_position(1.8, 3.0)), Position::new(1, 3), "d");
        assert_eq!(m.validate_position(js_position(1.0, 0.3)), Position::new(1, 1), "e");
        assert_eq!(m.validate_position(js_position(2.0, 0.8)), Position::new(2, 1), "f");
        assert_eq!(m.validate_position(js_position(1.0, 1.2)), Position::new(1, 1), "g");
        assert_eq!(m.validate_position(js_position(2.0, 1.5)), Position::new(2, 1), "h");

        m.dispose();
    }

    #[test]
    fn issue_71480_validate_range_handle_floats() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let m = create_text_model("line one\nline two");

        assert_eq!(m.validate_range(&js_range(0.2, 1.5, 0.8, 2.5)), Range::new(1, 1, 1, 1));
        assert_eq!(m.validate_range(&js_range(1.2, 1.7, 1.8, 2.2)), Range::new(1, 1, 1, 2));

        m.dispose();
    }

    #[test]
    fn validate_range_around_high_low_surrogate_pairs_1() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();

        let m = create_text_model("a📚b");

        assert_eq!(m.validate_range(&Range::new(0, 0, 0, 1)), Range::new(1, 1, 1, 1));
        assert_eq!(m.validate_range(&Range::new(0, 0, 0, 7)), Range::new(1, 1, 1, 1));

        assert_eq!(m.validate_range(&Range::new(1, 1, 1, 1)), Range::new(1, 1, 1, 1));
        assert_eq!(m.validate_range(&Range::new(1, 1, 1, 2)), Range::new(1, 1, 1, 2));
        assert_eq!(m.validate_range(&Range::new(1, 1, 1, 3)), Range::new(1, 1, 1, 4));
        assert_eq!(m.validate_range(&Range::new(1, 1, 1, 4)), Range::new(1, 1, 1, 4));
        assert_eq!(m.validate_range(&Range::new(1, 1, 1, 5)), Range::new(1, 1, 1, 5));

        assert_eq!(m.validate_range(&Range::new(1, 2, 1, 2)), Range::new(1, 2, 1, 2));
        assert_eq!(m.validate_range(&Range::new(1, 2, 1, 3)), Range::new(1, 2, 1, 4));
        assert_eq!(m.validate_range(&Range::new(1, 2, 1, 4)), Range::new(1, 2, 1, 4));
        assert_eq!(m.validate_range(&Range::new(1, 2, 1, 5)), Range::new(1, 2, 1, 5));

        assert_eq!(m.validate_range(&Range::new(1, 3, 1, 3)), Range::new(1, 2, 1, 2));
        assert_eq!(m.validate_range(&Range::new(1, 3, 1, 4)), Range::new(1, 2, 1, 4));
        assert_eq!(m.validate_range(&Range::new(1, 3, 1, 5)), Range::new(1, 2, 1, 5));

        assert_eq!(m.validate_range(&Range::new(1, 4, 1, 4)), Range::new(1, 4, 1, 4));
        assert_eq!(m.validate_range(&Range::new(1, 4, 1, 5)), Range::new(1, 4, 1, 5));

        assert_eq!(m.validate_range(&Range::new(1, 5, 1, 5)), Range::new(1, 5, 1, 5));

        m.dispose();
    }

    #[test]
    fn validate_range_around_high_low_surrogate_pairs_2() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();

        let m = create_text_model("a📚📚b");

        assert_eq!(m.validate_range(&Range::new(0, 0, 0, 1)), Range::new(1, 1, 1, 1));
        assert_eq!(m.validate_range(&Range::new(0, 0, 0, 7)), Range::new(1, 1, 1, 1));

        assert_eq!(m.validate_range(&Range::new(1, 1, 1, 1)), Range::new(1, 1, 1, 1));
        assert_eq!(m.validate_range(&Range::new(1, 1, 1, 2)), Range::new(1, 1, 1, 2));
        assert_eq!(m.validate_range(&Range::new(1, 1, 1, 3)), Range::new(1, 1, 1, 4));
        assert_eq!(m.validate_range(&Range::new(1, 1, 1, 4)), Range::new(1, 1, 1, 4));
        assert_eq!(m.validate_range(&Range::new(1, 1, 1, 5)), Range::new(1, 1, 1, 6));
        assert_eq!(m.validate_range(&Range::new(1, 1, 1, 6)), Range::new(1, 1, 1, 6));
        assert_eq!(m.validate_range(&Range::new(1, 1, 1, 7)), Range::new(1, 1, 1, 7));

        assert_eq!(m.validate_range(&Range::new(1, 2, 1, 2)), Range::new(1, 2, 1, 2));
        assert_eq!(m.validate_range(&Range::new(1, 2, 1, 3)), Range::new(1, 2, 1, 4));
        assert_eq!(m.validate_range(&Range::new(1, 2, 1, 4)), Range::new(1, 2, 1, 4));
        assert_eq!(m.validate_range(&Range::new(1, 2, 1, 5)), Range::new(1, 2, 1, 6));
        assert_eq!(m.validate_range(&Range::new(1, 2, 1, 6)), Range::new(1, 2, 1, 6));
        assert_eq!(m.validate_range(&Range::new(1, 2, 1, 7)), Range::new(1, 2, 1, 7));

        assert_eq!(m.validate_range(&Range::new(1, 3, 1, 3)), Range::new(1, 2, 1, 2));
        assert_eq!(m.validate_range(&Range::new(1, 3, 1, 4)), Range::new(1, 2, 1, 4));
        assert_eq!(m.validate_range(&Range::new(1, 3, 1, 5)), Range::new(1, 2, 1, 6));
        assert_eq!(m.validate_range(&Range::new(1, 3, 1, 6)), Range::new(1, 2, 1, 6));
        assert_eq!(m.validate_range(&Range::new(1, 3, 1, 7)), Range::new(1, 2, 1, 7));

        assert_eq!(m.validate_range(&Range::new(1, 4, 1, 4)), Range::new(1, 4, 1, 4));
        assert_eq!(m.validate_range(&Range::new(1, 4, 1, 5)), Range::new(1, 4, 1, 6));
        assert_eq!(m.validate_range(&Range::new(1, 4, 1, 6)), Range::new(1, 4, 1, 6));
        assert_eq!(m.validate_range(&Range::new(1, 4, 1, 7)), Range::new(1, 4, 1, 7));

        assert_eq!(m.validate_range(&Range::new(1, 5, 1, 5)), Range::new(1, 4, 1, 4));
        assert_eq!(m.validate_range(&Range::new(1, 5, 1, 6)), Range::new(1, 4, 1, 6));
        assert_eq!(m.validate_range(&Range::new(1, 5, 1, 7)), Range::new(1, 4, 1, 7));

        assert_eq!(m.validate_range(&Range::new(1, 6, 1, 6)), Range::new(1, 6, 1, 6));
        assert_eq!(m.validate_range(&Range::new(1, 6, 1, 7)), Range::new(1, 6, 1, 7));

        assert_eq!(m.validate_range(&Range::new(1, 7, 1, 7)), Range::new(1, 7, 1, 7));

        m.dispose();
    }

    #[test]
    fn modify_position() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();

        let m = create_text_model("line one\nline two");
        assert_eq!(m.modify_position(Position::new(1, 1), 0), Position::new(1, 1));
        assert_eq!(m.modify_position(Position::new(0, 0), 0), Position::new(1, 1));
        assert_eq!(m.modify_position(Position::new(30, 1), 0), Position::new(2, 9));

        assert_eq!(m.modify_position(Position::new(1, 1), 17), Position::new(2, 9));
        assert_eq!(m.modify_position(Position::new(1, 1), 1), Position::new(1, 2));
        assert_eq!(m.modify_position(Position::new(1, 1), 3), Position::new(1, 4));
        assert_eq!(m.modify_position(Position::new(1, 2), 10), Position::new(2, 3));
        assert_eq!(m.modify_position(Position::new(1, 5), 13), Position::new(2, 9));
        assert_eq!(m.modify_position(Position::new(1, 2), 16), Position::new(2, 9));

        assert_eq!(m.modify_position(Position::new(2, 9), -17), Position::new(1, 1));
        assert_eq!(m.modify_position(Position::new(1, 2), -1), Position::new(1, 1));
        assert_eq!(m.modify_position(Position::new(1, 4), -3), Position::new(1, 1));
        assert_eq!(m.modify_position(Position::new(2, 3), -10), Position::new(1, 2));
        assert_eq!(m.modify_position(Position::new(2, 9), -13), Position::new(1, 5));
        assert_eq!(m.modify_position(Position::new(2, 9), -16), Position::new(1, 2));

        assert_eq!(m.modify_position(Position::new(1, 2), 17), Position::new(2, 9));
        assert_eq!(m.modify_position(Position::new(1, 2), 100), Position::new(2, 9));

        assert_eq!(m.modify_position(Position::new(1, 2), -2), Position::new(1, 1));
        assert_eq!(m.modify_position(Position::new(1, 2), -100), Position::new(1, 1));
        assert_eq!(m.modify_position(Position::new(2, 2), -100), Position::new(1, 1));
        assert_eq!(m.modify_position(Position::new(2, 9), -18), Position::new(1, 1));

        m.dispose();
    }

    #[test]
    fn normalize_indentation_1() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let model = create_text_model_with(
            "",
            None,
            RelaxedTextModelCreationOptions {
                insert_spaces: Some(false),
                ..Default::default()
            },
            None,
        );

        assert_eq!(model.normalize_indentation(&u("\t")), "\t");
        assert_eq!(model.normalize_indentation(&u("    ")), "\t");
        assert_eq!(model.normalize_indentation(&u("   ")), "   ");
        assert_eq!(model.normalize_indentation(&u("  ")), "  ");
        assert_eq!(model.normalize_indentation(&u(" ")), " ");
        assert_eq!(model.normalize_indentation(&u("")), "");
        assert_eq!(model.normalize_indentation(&u(" \t    ")), "\t\t");
        assert_eq!(model.normalize_indentation(&u(" \t   ")), "\t   ");
        assert_eq!(model.normalize_indentation(&u(" \t  ")), "\t  ");
        assert_eq!(model.normalize_indentation(&u(" \t ")), "\t ");
        assert_eq!(model.normalize_indentation(&u(" \t")), "\t");

        assert_eq!(model.normalize_indentation(&u("\ta")), "\ta");
        assert_eq!(model.normalize_indentation(&u("    a")), "\ta");
        assert_eq!(model.normalize_indentation(&u("   a")), "   a");
        assert_eq!(model.normalize_indentation(&u("  a")), "  a");
        assert_eq!(model.normalize_indentation(&u(" a")), " a");
        assert_eq!(model.normalize_indentation(&u("a")), "a");
        assert_eq!(model.normalize_indentation(&u(" \t    a")), "\t\ta");
        assert_eq!(model.normalize_indentation(&u(" \t   a")), "\t   a");
        assert_eq!(model.normalize_indentation(&u(" \t  a")), "\t  a");
        assert_eq!(model.normalize_indentation(&u(" \t a")), "\t a");
        assert_eq!(model.normalize_indentation(&u(" \ta")), "\ta");

        model.dispose();
    }

    #[test]
    fn normalize_indentation_2() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let model = create_text_model("");

        assert_eq!(model.normalize_indentation(&u("\ta")), "    a");
        assert_eq!(model.normalize_indentation(&u("    a")), "    a");
        assert_eq!(model.normalize_indentation(&u("   a")), "   a");
        assert_eq!(model.normalize_indentation(&u("  a")), "  a");
        assert_eq!(model.normalize_indentation(&u(" a")), " a");
        assert_eq!(model.normalize_indentation(&u("a")), "a");
        assert_eq!(model.normalize_indentation(&u(" \t    a")), "        a");
        assert_eq!(model.normalize_indentation(&u(" \t   a")), "       a");
        assert_eq!(model.normalize_indentation(&u(" \t  a")), "      a");
        assert_eq!(model.normalize_indentation(&u(" \t a")), "     a");
        assert_eq!(model.normalize_indentation(&u(" \ta")), "    a");

        model.dispose();
    }

    #[test]
    fn get_line_first_non_whitespace_column() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let model = create_text_model(
            &["asd", " asd", "\tasd", "  asd", "\t\tasd", " ", "  ", "\t", "\t\t", "  \tasd", "", ""].join("\n"),
        );

        assert_eq!(model.get_line_first_non_whitespace_column(1), 1, "1");
        assert_eq!(model.get_line_first_non_whitespace_column(2), 2, "2");
        assert_eq!(model.get_line_first_non_whitespace_column(3), 2, "3");
        assert_eq!(model.get_line_first_non_whitespace_column(4), 3, "4");
        assert_eq!(model.get_line_first_non_whitespace_column(5), 3, "5");
        assert_eq!(model.get_line_first_non_whitespace_column(6), 0, "6");
        assert_eq!(model.get_line_first_non_whitespace_column(7), 0, "7");
        assert_eq!(model.get_line_first_non_whitespace_column(8), 0, "8");
        assert_eq!(model.get_line_first_non_whitespace_column(9), 0, "9");
        assert_eq!(model.get_line_first_non_whitespace_column(10), 4, "10");
        assert_eq!(model.get_line_first_non_whitespace_column(11), 0, "11");
        assert_eq!(model.get_line_first_non_whitespace_column(12), 0, "12");

        model.dispose();
    }

    #[test]
    fn get_line_last_non_whitespace_column() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let model = create_text_model(
            &["asd", "asd ", "asd\t", "asd  ", "asd\t\t", " ", "  ", "\t", "\t\t", "asd  \t", "", ""].join("\n"),
        );

        assert_eq!(model.get_line_last_non_whitespace_column(1), 4, "1");
        assert_eq!(model.get_line_last_non_whitespace_column(2), 4, "2");
        assert_eq!(model.get_line_last_non_whitespace_column(3), 4, "3");
        assert_eq!(model.get_line_last_non_whitespace_column(4), 4, "4");
        assert_eq!(model.get_line_last_non_whitespace_column(5), 4, "5");
        assert_eq!(model.get_line_last_non_whitespace_column(6), 0, "6");
        assert_eq!(model.get_line_last_non_whitespace_column(7), 0, "7");
        assert_eq!(model.get_line_last_non_whitespace_column(8), 0, "8");
        assert_eq!(model.get_line_last_non_whitespace_column(9), 0, "9");
        assert_eq!(model.get_line_last_non_whitespace_column(10), 4, "10");
        assert_eq!(model.get_line_last_non_whitespace_column(11), 0, "11");
        assert_eq!(model.get_line_last_non_whitespace_column(12), 0, "12");

        model.dispose();
    }

    #[test]
    /// `#50471. getValueInRange with invalid range`
    fn issue_50471_get_value_in_range_with_invalid_range() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let m = create_text_model("My First Line\r\nMy Second Line\r\nMy Third Line");
        assert_eq!(m.get_value_in_range(&js_range(1.0, f64::NAN, 1.0, 3.0)), "My");
        assert_eq!(m.get_value_in_range(&js_range(f64::NAN, f64::NAN, f64::NAN, f64::NAN)), "");
        m.dispose();
    }

    #[test]
    /// `issue #168836: updating tabSize should also update indentSize when indentSize is set to "tabSize"`
    fn issue_168836_updating_tab_size_should_also_update_indent_size_when_indent_size_is_set_to_tab_size() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let m = create_text_model_with(
            "some text",
            None,
            RelaxedTextModelCreationOptions {
                tab_size: Some(2),
                indent_size: Some(IndentSize::TabSize),
                ..Default::default()
            },
            None,
        );
        assert_eq!(m.get_options().tab_size, 2);
        assert_eq!(m.get_options().indent_size, 2);
        assert_eq!(m.get_options().original_indent_size(), IndentSize::TabSize);
        m.update_options(TextModelUpdateOptions {
            tab_size: Some(4),
            ..Default::default()
        });
        assert_eq!(m.get_options().tab_size, 4);
        assert_eq!(m.get_options().indent_size, 4);
        assert_eq!(m.get_options().original_indent_size(), IndentSize::TabSize);
        m.dispose();
    }
}

mod text_model_might_contain_rtl {
    use super::*;

    #[test]
    fn nope() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let model = create_text_model("hello world!");
        assert!(!model.might_contain_rtl());
        model.dispose();
    }

    #[test]
    fn yes() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let model = create_text_model("Hello,\nזוהי עובדה מבוססת שדעתו");
        assert!(model.might_contain_rtl());
        model.dispose();
    }

    #[test]
    fn set_value_resets_1() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let model = create_text_model("hello world!");
        assert!(!model.might_contain_rtl());
        model.set_value("Hello,\nזוהי עובדה מבוססת שדעתו".into());
        assert!(model.might_contain_rtl());
        model.dispose();
    }

    #[test]
    fn set_value_resets_2() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let model = create_text_model("Hello,\nهناك حقيقة مثبتة منذ زمن طويل");
        assert!(model.might_contain_rtl());
        model.set_value("hello world!".into());
        assert!(!model.might_contain_rtl());
        model.dispose();
    }
}

mod text_model_create_snapshot {
    use super::*;

    #[test]
    fn empty_file() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let model = create_text_model("");
        let mut snapshot = model.create_snapshot(false);
        assert_eq!(snapshot.read(), None);
        model.dispose();
    }

    #[test]
    fn file_with_bom() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let mut text = JsString::from(&UTF8_BOM_CHARACTER[..]);
        text.push_units(&u("Hello"));
        let model = create_text_model_with(text.clone(), None, RelaxedTextModelCreationOptions::default(), None);
        assert_eq!(model.get_line_content(1), "Hello");
        let mut snapshot = model.create_snapshot(true);
        assert_eq!(snapshot.read(), Some(text));
        assert_eq!(snapshot.read(), None);
        model.dispose();
    }

    #[test]
    fn regular_file() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let model = create_text_model("My First Line\n\t\tMy Second Line\n    Third Line\n\n1");
        let mut snapshot = model.create_snapshot(false);
        assert_eq!(snapshot.read(), Some(JsString::from("My First Line\n\t\tMy Second Line\n    Third Line\n\n1")));
        assert_eq!(snapshot.read(), None);
        model.dispose();
    }

    #[test]
    fn large_file() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let lines = vec!["Just some text that is a bit long such that it can consume some memory"; 1000];
        let text = lines.join("\n");

        let model = create_text_model(&text);
        let mut snapshot = model.create_snapshot(false);
        let mut actual = JsString::new();

        // 70999 de comprimento => no máximo 2 chamadas de read
        let tmp1 = snapshot.read();
        assert!(tmp1.as_ref().is_some_and(|tmp1| !tmp1.is_empty()));
        actual.push_units(tmp1.unwrap().units());

        let tmp2 = snapshot.read();
        match tmp2 {
            None => {
                // tudo certo
            }
            Some(tmp2) => {
                actual.push_units(tmp2.units());
                assert_eq!(snapshot.read(), None);
            }
        }

        assert_eq!(actual, JsString::from(text));

        model.dispose();
    }

    #[test]
    fn issue_119632_invalid_range() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let model = create_text_model("hello world!");
        let actual = model.validate_range_relaxed(&Range::new(js_int(None), 0, js_int(None), 1));
        assert_eq!(actual, Range::new(1, 1, 1, 1));
        model.dispose();
    }
}
