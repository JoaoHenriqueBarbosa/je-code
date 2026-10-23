//! Porte de `src/vs/editor/test/common/viewModel/monospaceLineBreaksComputer.test.ts` (suíte
//! `Editor ViewModel - MonospaceLineBreaksComputer`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` vira o `LeakGuard` criado no começo de cada caso.
//! - `EditorOptions.wordWrapBreakBeforeCharacters.defaultValue` (e `After`) é o valor padrão da opção no
//!   registro (`editor_options_registry`).
//! - `parseAnnotatedText` devolve `{ text, indices }` no original, mas só o `text` é lido; aqui devolve só ele.
//! - Os valores padrão dos parâmetros de `assertLineBreaks` (`WrappingIndent.None`, `'normal'`) ficam num
//!   fechamento local no caso que os usa o tempo todo; nos demais são passados por extenso.
//! - `undefined` de `lineBreakData?.campo` (quando o resultado é `null`) é `None`.
//! - Textos que o original escreve com `\uXXXX` ou `\xNN` usam `\u{XXXX}` e `\u{NN}`.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::JsString;
use je_editor::config::editor_options::registry::editor_options_registry;
use je_editor::config::editor_options::wrapping_indent::WrappingIndent;
use je_editor::config::editor_options::EditorOption;
use je_editor::config::font_info::{BareFontInfoOptions, FontInfo, FontInfoOptions};
use je_editor::model::{InjectedTextOptions, PositionAffinity};
use je_editor::model_line_projection_data::{
    LineBreaksComputerContext, LineBreaksComputerFactory, ModelLineProjectionData, WordBreak,
};
use je_editor::monospace_line_breaks_computer::MonospaceLineBreaksComputerFactory;
use je_editor::text_model_events::LineInjectedText;

/// `EditorOptions.<option>.defaultValue` das opções de texto.
fn default_string_option(option: EditorOption) -> String {
    editor_options_registry()[option as usize]
        .default_value()
        .downcast_ref::<String>()
        .cloned()
        .expect("a opção guarda um texto")
}

/// `new MonospaceLineBreaksComputerFactory(EditorOptions.wordWrapBreakBeforeCharacters.defaultValue,
/// EditorOptions.wordWrapBreakAfterCharacters.defaultValue)`.
fn default_factory() -> MonospaceLineBreaksComputerFactory {
    MonospaceLineBreaksComputerFactory::new(
        &default_string_option(EditorOption::WordWrapBreakBeforeCharacters),
        &default_string_option(EditorOption::WordWrapBreakAfterCharacters),
    )
}

fn parse_annotated_text(annotated_text: &str) -> JsString {
    let mut text = JsString::new();
    for character in annotated_text.chars().filter(|c| *c != '|') {
        text.push_units(&character.to_string().encode_utf16().collect::<Vec<u16>>());
    }
    text
}

fn to_annotated_text(text: &JsString, line_break_data: &Option<ModelLineProjectionData>) -> String {
    // Insert line break markers again, according to algorithm
    let mut actual_annotated_text: Vec<u16> = Vec::new();
    match line_break_data {
        Some(line_break_data) => {
            let mut previous_line_index = 0;
            for (i, unit) in text.units().iter().enumerate() {
                let r = line_break_data.translate_to_output_position(i, PositionAffinity::None);
                if previous_line_index != r.output_line_index {
                    previous_line_index = r.output_line_index;
                    actual_annotated_text.push(u16::from(b'|'));
                }
                actual_annotated_text.push(*unit);
            }
        }
        None => {
            // No wrapping
            actual_annotated_text.extend_from_slice(text.units());
        }
    }
    JsString::from_units(actual_annotated_text).to_string_lossy()
}

struct TestContext {
    text: JsString,
    injected_text: Option<Vec<LineInjectedText>>,
}

impl LineBreaksComputerContext for TestContext {
    fn get_line_content(&self, _line_number: i32) -> JsString {
        self.text.clone()
    }

    fn get_line_injected_text(&self, _line_number: i32) -> Option<Vec<LineInjectedText>> {
        self.injected_text.clone()
    }
}

#[allow(clippy::too_many_arguments)]
fn get_line_break_data(
    factory: &dyn LineBreaksComputerFactory,
    tab_size: f64,
    break_after: f64,
    columns_for_full_width_char: f64,
    wrapping_indent: WrappingIndent,
    word_break: WordBreak,
    wrap_on_escaped_line_feeds: bool,
    text: &JsString,
    previous_line_break_data: Option<&ModelLineProjectionData>,
    injected_text: Option<Vec<LineInjectedText>>,
    use_two_cell_fullwidth_characters: bool,
) -> Option<ModelLineProjectionData> {
    let font_info = FontInfo::new(
        FontInfoOptions {
            bare: BareFontInfoOptions {
                pixel_ratio: 1.0,
                font_family: "testFontFamily".into(),
                font_weight: "normal".into(),
                font_size: 14.0,
                font_feature_settings: String::new(),
                font_variation_settings: String::new(),
                line_height: 19.0,
                letter_spacing: 0.0,
            },
            is_monospace: true,
            typical_halfwidth_character_width: 7.0,
            typical_fullwidth_character_width: 7.0 * columns_for_full_width_char,
            can_use_halfwidth_rightwards_arrow: true,
            space_width: 7.0,
            middot_width: 7.0,
            wsmiddot_width: 7.0,
            max_digit_width: 7.0,
        },
        false,
    );
    let context = TestContext { text: text.clone(), injected_text };
    let mut line_breaks_computer = factory.create_line_breaks_computer(
        &context,
        &font_info,
        tab_size,
        break_after,
        wrapping_indent,
        word_break,
        wrap_on_escaped_line_feeds,
        use_two_cell_fullwidth_characters,
    );
    let previous_line_break_data_clone = previous_line_break_data.map(|previous| {
        ModelLineProjectionData::new(
            None,
            None,
            previous.break_offsets.clone(),
            previous.break_offsets_visible_column.clone(),
            previous.wrapped_text_indent_length,
        )
    });
    line_breaks_computer.add_request(1, previous_line_break_data_clone);
    line_breaks_computer.finalize().swap_remove(0)
}

fn assert_line_breaks(
    factory: &dyn LineBreaksComputerFactory,
    tab_size: f64,
    break_after: f64,
    annotated_text: &str,
    wrapping_indent: WrappingIndent,
    word_break: WordBreak,
) -> Option<ModelLineProjectionData> {
    // Create version of `annotatedText` with line break markers removed
    let text = parse_annotated_text(annotated_text);
    let line_break_data =
        get_line_break_data(factory, tab_size, break_after, 2.0, wrapping_indent, word_break, false, &text, None, None, false);
    let actual_annotated_text = to_annotated_text(&text, &line_break_data);

    assert_eq!(actual_annotated_text, annotated_text);

    line_break_data
}

/// A faixa de `new LineInjectedText(0, 1, column, { content, widthInEm }, order)`.
fn fixed_width_injection(column: i32, content: &str, width_in_em: f64, order: i32) -> LineInjectedText {
    LineInjectedText {
        owner_id: 0,
        line_number: 1,
        column,
        options: InjectedTextOptions {
            content: content.to_owned(),
            width_in_em: Some(width_in_em),
            ..InjectedTextOptions::default()
        },
        order,
    }
}

/// `{ breakOffsets: data?.breakOffsets, breakOffsetsVisibleColumn: data?.breakOffsetsVisibleColumn }`.
fn break_offsets_and_columns(data: &Option<ModelLineProjectionData>) -> (Option<Vec<usize>>, Option<Vec<f64>>) {
    (
        data.as_ref().map(|data| data.break_offsets.clone()),
        data.as_ref().map(|data| data.break_offsets_visible_column.clone()),
    )
}

#[test]
fn monospace_line_breaks_computer() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();

    let factory = MonospaceLineBreaksComputerFactory::new("(", "\t).");
    let check = |tab_size: f64, break_after: f64, annotated_text: &str| {
        assert_line_breaks(&factory, tab_size, break_after, annotated_text, WrappingIndent::None, WordBreak::Normal);
    };

    // Empty string
    check(4.0, 5.0, "");

    // No wrapping if not necessary
    check(4.0, 5.0, "aaa");
    check(4.0, 5.0, "aaaaa");
    check(4.0, -1.0, "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa");

    // Acts like hard wrapping if no char found
    check(4.0, 5.0, "aaaaa|a");

    // Honors wrapping character
    check(4.0, 5.0, "aaaaa|.");
    check(4.0, 5.0, "aaaaa|a.|aaa.|aa");
    check(4.0, 5.0, "aaaaa|a..|aaa.|aa");
    check(4.0, 5.0, "aaaaa|a...|aaa.|aa");
    check(4.0, 5.0, "aaaaa|a....|aaa.|aa");

    // Honors tabs when computing wrapping position
    check(4.0, 5.0, "\t");
    check(4.0, 5.0, "\t|aaa");
    check(4.0, 5.0, "\t|a\t|aa");
    check(4.0, 5.0, "aa\ta");
    check(4.0, 5.0, "aa\t|aa");

    // Honors wrapping before characters (& gives it priority)
    check(4.0, 5.0, "aaa.|aa");
    check(4.0, 5.0, "aaa(.|aa");

    // Honors wrapping after characters (& gives it priority)
    check(4.0, 5.0, "aaa))|).aaa");
    check(4.0, 5.0, "aaa))|).|aaaa");
    check(4.0, 5.0, "aaa)|().|aaa");
    check(4.0, 5.0, "aaa|(().|aaa");
    check(4.0, 5.0, "aa.|(().|aaa");
    check(4.0, 5.0, "aa.|(.).|aaa");
}

#[test]
fn accounts_for_fixed_injected_text_width_when_wrapping() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let factory = MonospaceLineBreaksComputerFactory::new("", "");
    let line_break_data = get_line_break_data(
        &factory,
        4.0,
        5.0,
        2.0,
        WrappingIndent::None,
        WordBreak::Normal,
        false,
        &JsString::from("abcdef"),
        None,
        Some(vec![fixed_width_injection(5, "\u{a0}", 1.0, 0)]),
        false,
    );

    assert_eq!(break_offsets_and_columns(&line_break_data), (Some(vec![4, 7]), Some(vec![4.0, 7.0])));
}

#[test]
fn treats_multi_character_fixed_width_injected_text_as_an_atomic_span() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let factory = MonospaceLineBreaksComputerFactory::new("", "");
    let line_break_data = get_line_break_data(
        &factory,
        4.0,
        5.0,
        2.0,
        WrappingIndent::None,
        WordBreak::Normal,
        false,
        &JsString::from("abcdef"),
        None,
        Some(vec![fixed_width_injection(4, "hello", 1.0, 0)]),
        false,
    );

    assert_eq!(break_offsets_and_columns(&line_break_data), (Some(vec![8, 11]), Some(vec![8.0, 11.0])));
}

#[test]
fn wraps_after_an_escaped_line_feed_followed_by_fixed_width_injected_text() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let factory = MonospaceLineBreaksComputerFactory::new("", "");
    let line_break_data = get_line_break_data(
        &factory,
        4.0,
        100.0,
        2.0,
        WrappingIndent::None,
        WordBreak::Normal,
        true,
        &JsString::from("\"a\\nb\""),
        None,
        Some(vec![fixed_width_injection(5, "hint", 1.0, 0)]),
        false,
    );

    assert_eq!(line_break_data.map(|data| data.break_offsets), Some(vec![4, 10]));
}

#[test]
fn treats_adjacent_fixed_width_injected_texts_as_separate_atomic_spans() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let factory = MonospaceLineBreaksComputerFactory::new("", "");
    let line_break_data = get_line_break_data(
        &factory,
        4.0,
        5.0,
        2.0,
        WrappingIndent::None,
        WordBreak::Normal,
        false,
        &JsString::from("abcdef"),
        None,
        Some(vec![fixed_width_injection(4, "x", 0.5, 0), fixed_width_injection(4, "y", 1.0, 1)]),
        false,
    );

    assert_eq!(break_offsets_and_columns(&line_break_data), (Some(vec![4, 8]), Some(vec![4.0, 8.0])));
}

#[test]
fn keeps_fixed_width_injected_text_wider_than_the_wrap_column_atomic() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let factory = MonospaceLineBreaksComputerFactory::new("", "");
    let line_break_data = get_line_break_data(
        &factory,
        4.0,
        5.0,
        2.0,
        WrappingIndent::None,
        WordBreak::Normal,
        false,
        &JsString::from("abcdef"),
        None,
        Some(vec![fixed_width_injection(4, "x", 3.0, 0)]),
        false,
    );

    assert_eq!(break_offsets_and_columns(&line_break_data), (Some(vec![3, 4, 7]), Some(vec![3.0, 4.0, 7.0])));
}

#[test]
fn wraps_before_a_tab_that_no_longer_fits_after_fixed_width_injected_text() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let factory = MonospaceLineBreaksComputerFactory::new("", "");
    let line_break_data = get_line_break_data(
        &factory,
        4.0,
        4.0,
        2.0,
        WrappingIndent::None,
        WordBreak::Normal,
        false,
        &JsString::from("ab\tcd"),
        None,
        Some(vec![fixed_width_injection(3, "\u{a0}", 0.75, 0)]),
        false,
    );

    // `ab` plus a 0.75em wide injection fills 2.75 columns worth of pixels, so the tab (which
    // expands from character column 3 to character column 4) no longer fits and the line wraps
    // right before it.
    assert_eq!(break_offsets_and_columns(&line_break_data), (Some(vec![3, 6]), Some(vec![3.0, 6.0])));
}

#[test]
fn expands_a_tab_following_fixed_width_injected_text_from_the_character_column() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let factory = MonospaceLineBreaksComputerFactory::new("", "");
    let line_break_data = get_line_break_data(
        &factory,
        4.0,
        10.0,
        2.0,
        WrappingIndent::None,
        WordBreak::Normal,
        false,
        &JsString::from("ab\tcdefgh"),
        None,
        Some(vec![fixed_width_injection(3, "\u{a0}", 1.5, 0)]),
        false,
    );

    // The injection counts as a single character column, so the tab at character column 3 expands
    // to character column 4 and the whole line spans 10 columns. Its 1.5em is spent on the wrapping
    // decision alone, which runs out of room at `g`. Were that width to leak into the column
    // accumulator, the injection would count as 3 columns, the tab would expand to character
    // column 8, and the line would already run out of room at `e`.
    assert_eq!(break_offsets_and_columns(&line_break_data), (Some(vec![8, 10]), Some(vec![8.0, 10.0])));
}

#[test]
fn reserves_width_for_a_spacing_only_injection_at_the_beginning_of_a_line() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let factory = MonospaceLineBreaksComputerFactory::new("", "");
    let line_break_data = get_line_break_data(
        &factory,
        4.0,
        6.0,
        2.0,
        WrappingIndent::None,
        WordBreak::Normal,
        false,
        &JsString::from("abcdef"),
        None,
        Some(vec![fixed_width_injection(1, "", 1.0, 0)]),
        false,
    );

    // 1em is two columns wide here, so the spacer pushes the last two characters onto a second
    // line. It consumes no character, so the offsets still refer to the original text.
    assert_eq!(break_offsets_and_columns(&line_break_data), (Some(vec![4, 6]), Some(vec![4.0, 6.0])));
}

#[test]
fn does_not_create_empty_lines_for_oversized_spacing_only_injected_text() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let factory = MonospaceLineBreaksComputerFactory::new("", "");
    let line_break_data = get_line_break_data(
        &factory,
        4.0,
        2.0,
        2.0,
        WrappingIndent::None,
        WordBreak::Normal,
        false,
        &JsString::from("abc"),
        None,
        Some(vec![fixed_width_injection(1, "", 2.0, 0)]),
        false,
    );

    // The spacer alone already overflows the wrap width, but there is no character before it to
    // wrap, so it must share the first line with `a` rather than emit an empty leading line.
    assert_eq!(line_break_data.map(|data| data.break_offsets), Some(vec![1, 3]));
}

#[test]
fn lets_a_spacing_only_injection_in_the_middle_of_a_line_inherit_surrounding_break_opportunities() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let factory = MonospaceLineBreaksComputerFactory::new("", " ");
    let line_break_data = get_line_break_data(
        &factory,
        4.0,
        8.0,
        2.0,
        WrappingIndent::None,
        WordBreak::Normal,
        false,
        &JsString::from("ab cdef"),
        None,
        Some(vec![fixed_width_injection(4, "", 1.0, 0)]),
        false,
    );

    // The spacer is transparent for break opportunities: it neither creates nor suppresses one, so
    // the break after the space is still recorded even though the spacer sits on top of it, and the
    // line breaks there rather than at `f` where it runs out of room. Its 1em is what makes the line
    // run out of room at all: without it the seven characters fit in the eight available columns.
    assert_eq!(break_offsets_and_columns(&line_break_data), (Some(vec![3, 7]), Some(vec![3.0, 7.0])));
}

#[test]
fn ignores_a_spacing_only_injection_at_the_end_of_a_line() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let factory = MonospaceLineBreaksComputerFactory::new("", "");
    let line_break_data = get_line_break_data(
        &factory,
        4.0,
        6.0,
        2.0,
        WrappingIndent::None,
        WordBreak::Normal,
        false,
        &JsString::from("abcdef"),
        None,
        Some(vec![fixed_width_injection(7, "", 1.0, 0)]),
        false,
    );

    // The line is exactly full, and there is no character after the spacer that could be moved to a
    // following line, so the spacer cannot introduce a break.
    assert_eq!(break_offsets_and_columns(&line_break_data), (Some(vec![6]), Some(vec![6.0])));
}

#[test]
fn expands_a_tab_following_a_spacing_only_injection_from_the_character_column() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let factory = MonospaceLineBreaksComputerFactory::new("", "");
    let line_break_data = get_line_break_data(
        &factory,
        4.0,
        6.0,
        2.0,
        WrappingIndent::None,
        WordBreak::Normal,
        false,
        &JsString::from("ab\tcd"),
        None,
        Some(vec![fixed_width_injection(3, "", 1.0, 0)]),
        false,
    );

    // The spacer occupies no character column, so the tab still expands from character column 2 to
    // character column 4. Were its 1em width to leak into the column accumulator, the tab would
    // start at character column 4 and expand to character column 8 instead.
    assert_eq!(break_offsets_and_columns(&line_break_data), (Some(vec![3, 5]), Some(vec![4.0, 6.0])));
}

#[test]
fn does_not_use_fixed_width_injected_whitespace_as_continuation_indentation() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let factory = MonospaceLineBreaksComputerFactory::new("", "");
    let line_break_data = get_line_break_data(
        &factory,
        4.0,
        5.0,
        2.0,
        WrappingIndent::Same,
        WordBreak::Normal,
        false,
        &JsString::from("abcdef"),
        None,
        Some(vec![fixed_width_injection(1, " ", 0.75, 0)]),
        false,
    );

    assert_eq!(
        (
            line_break_data.as_ref().map(|data| data.break_offsets.clone()),
            line_break_data.as_ref().map(|data| data.wrapped_text_indent_length),
        ),
        (Some(vec![4, 7]), Some(0))
    );
}

fn assert_line_break_data_equal(a: &Option<ModelLineProjectionData>, b: &Option<ModelLineProjectionData>) {
    let (Some(a), Some(b)) = (a, b) else {
        assert_eq!(a, b);
        return;
    };
    assert_eq!(a.break_offsets, b.break_offsets);
    assert_eq!(a.wrapped_text_indent_length, b.wrapped_text_indent_length);
    assert_eq!(a.break_offsets_visible_column.len(), b.break_offsets_visible_column.len());
    for (a_column, b_column) in a.break_offsets_visible_column.iter().zip(&b.break_offsets_visible_column) {
        let diff = (a_column - b_column).abs();
        assert!(diff < 0.001);
    }
}

#[allow(clippy::too_many_arguments)]
fn assert_incremental_line_breaks(
    factory: &dyn LineBreaksComputerFactory,
    text: &str,
    tab_size: f64,
    break_after1: f64,
    annotated_text1: &str,
    break_after2: f64,
    annotated_text2: &str,
    wrapping_indent: WrappingIndent,
    columns_for_full_width_char: f64,
    use_two_cell_fullwidth_characters: bool,
) {
    // sanity check the test
    assert_eq!(text, parse_annotated_text(annotated_text1).to_string_lossy());
    assert_eq!(text, parse_annotated_text(annotated_text2).to_string_lossy());
    let text = JsString::from(text);
    let get = |break_after: f64, previous: Option<&ModelLineProjectionData>| {
        get_line_break_data(
            factory,
            tab_size,
            break_after,
            columns_for_full_width_char,
            wrapping_indent,
            WordBreak::Normal,
            false,
            &text,
            previous,
            None,
            use_two_cell_fullwidth_characters,
        )
    };

    // check that the direct mapping is ok for 1
    let direct_line_break_data1 = get(break_after1, None);
    assert_eq!(to_annotated_text(&text, &direct_line_break_data1), annotated_text1);

    // check that the direct mapping is ok for 2
    let direct_line_break_data2 = get(break_after2, None);
    assert_eq!(to_annotated_text(&text, &direct_line_break_data2), annotated_text2);

    // check that going from 1 to 2 is ok
    let line_break_data2_from1 = get(break_after2, direct_line_break_data1.as_ref());
    assert_eq!(to_annotated_text(&text, &line_break_data2_from1), annotated_text2);
    assert_line_break_data_equal(&line_break_data2_from1, &direct_line_break_data2);

    // check that going from 2 to 1 is ok
    let line_break_data1_from2 = get(break_after1, direct_line_break_data2.as_ref());
    assert_eq!(to_annotated_text(&text, &line_break_data1_from2), annotated_text1);
    assert_line_break_data_equal(&line_break_data1_from2, &direct_line_break_data1);
}

#[test]
fn monospace_line_breaks_computer_incremental_1() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();

    let factory = default_factory();

    assert_incremental_line_breaks(
        &factory,
        "just some text and more",
        4.0,
        10.0,
        "just some |text and |more",
        15.0,
        "just some text |and more",
        WrappingIndent::None,
        2.0,
        false,
    );

    assert_incremental_line_breaks(
        &factory,
        "Cu scripserit suscipiantur eos, in affert pericula contentiones sed, cetero sanctus et pro. Ius vidit magna regione te, sit ei elaboraret liberavisse. Mundi verear eu mea, eam vero scriptorem in, vix in menandri assueverit. Natum definiebas cu vim. Vim doming vocibus efficiantur id. In indoctum deseruisse voluptatum vim, ad debitis verterem sed.",
        4.0,
        47.0,
        "Cu scripserit suscipiantur eos, in affert |pericula contentiones sed, cetero sanctus et |pro. Ius vidit magna regione te, sit ei |elaboraret liberavisse. Mundi verear eu mea, |eam vero scriptorem in, vix in menandri |assueverit. Natum definiebas cu vim. Vim |doming vocibus efficiantur id. In indoctum |deseruisse voluptatum vim, ad debitis verterem |sed.",
        142.0,
        "Cu scripserit suscipiantur eos, in affert pericula contentiones sed, cetero sanctus et pro. Ius vidit magna regione te, sit ei elaboraret |liberavisse. Mundi verear eu mea, eam vero scriptorem in, vix in menandri assueverit. Natum definiebas cu vim. Vim doming vocibus efficiantur |id. In indoctum deseruisse voluptatum vim, ad debitis verterem sed.",
        WrappingIndent::None,
        2.0,
        false,
    );

    assert_incremental_line_breaks(
        &factory,
        "An his legere persecuti, oblique delicata efficiantur ex vix, vel at graecis officiis maluisset. Et per impedit voluptua, usu discere maiorum at. Ut assum ornatus temporibus vis, an sea melius pericula. Ea dicunt oblique phaedrum nam, eu duo movet nobis. His melius facilis eu, vim malorum temporibus ne. Nec no sale regione, meliore civibus placerat id eam. Mea alii fabulas definitionem te, agam volutpat ad vis, et per bonorum nonumes repudiandae.",
        4.0,
        57.0,
        "An his legere persecuti, oblique delicata efficiantur ex |vix, vel at graecis officiis maluisset. Et per impedit |voluptua, usu discere maiorum at. Ut assum ornatus |temporibus vis, an sea melius pericula. Ea dicunt |oblique phaedrum nam, eu duo movet nobis. His melius |facilis eu, vim malorum temporibus ne. Nec no sale |regione, meliore civibus placerat id eam. Mea alii |fabulas definitionem te, agam volutpat ad vis, et per |bonorum nonumes repudiandae.",
        58.0,
        "An his legere persecuti, oblique delicata efficiantur ex |vix, vel at graecis officiis maluisset. Et per impedit |voluptua, usu discere maiorum at. Ut assum ornatus |temporibus vis, an sea melius pericula. Ea dicunt oblique |phaedrum nam, eu duo movet nobis. His melius facilis eu, |vim malorum temporibus ne. Nec no sale regione, meliore |civibus placerat id eam. Mea alii fabulas definitionem |te, agam volutpat ad vis, et per bonorum nonumes |repudiandae.",
        WrappingIndent::None,
        2.0,
        false,
    );

    assert_incremental_line_breaks(
        &factory,
        "\t\t\"owner\": \"vscode\",",
        4.0,
        14.0,
        "\t\t\"owner|\": |\"vscod|e\",",
        16.0,
        "\t\t\"owner\":| |\"vscode\"|,",
        WrappingIndent::Same,
        2.0,
        false,
    );

    assert_incremental_line_breaks(
        &factory,
        "🐇👬🌖🌞🏇🍼🐇👬🌖🌞🏇🍼🐇👬🌖🌞🏇🍼🐇👬🌖🌞🏇🍼🐇&👬🌖🌞🏇🍼🐇👬🌖🌞🏇🍼🐇👬🌖🌞🏇🍼🐇👬🌖🌞🏇🍼🐇👬",
        4.0,
        51.0,
        "🐇👬🌖🌞🏇🍼🐇👬🌖🌞🏇🍼🐇👬🌖🌞🏇🍼🐇👬🌖🌞🏇🍼🐇&|👬🌖🌞🏇🍼🐇👬🌖🌞🏇🍼🐇👬🌖🌞🏇🍼🐇👬🌖🌞🏇🍼🐇👬",
        50.0,
        "🐇👬🌖🌞🏇🍼🐇👬🌖🌞🏇🍼🐇👬🌖🌞🏇🍼🐇👬🌖🌞🏇🍼🐇|&|👬🌖🌞🏇🍼🐇👬🌖🌞🏇🍼🐇👬🌖🌞🏇🍼🐇👬🌖🌞🏇🍼🐇👬",
        WrappingIndent::Same,
        2.0,
        false,
    );

    assert_incremental_line_breaks(
        &factory,
        "🐇👬&🌞🌖",
        4.0,
        5.0,
        "🐇👬&|🌞🌖",
        4.0,
        "🐇👬|&|🌞🌖",
        WrappingIndent::Same,
        2.0,
        false,
    );

    assert_incremental_line_breaks(
        &factory,
        "\t\tfunc('🌞🏇🍼🌞🏇🍼🐇&👬🌖🌞👬🌖🌞🏇🍼🐇👬', WrappingIndent.Same);",
        4.0,
        26.0,
        "\t\tfunc|('🌞🏇🍼🌞🏇🍼🐇&|👬🌖🌞👬🌖🌞🏇🍼🐇|👬', |WrappingIndent.|Same);",
        27.0,
        "\t\tfunc|('🌞🏇🍼🌞🏇🍼🐇&|👬🌖🌞👬🌖🌞🏇🍼🐇|👬', |WrappingIndent.|Same);",
        WrappingIndent::Same,
        2.0,
        false,
    );

    assert_incremental_line_breaks(
        &factory,
        "factory, \"xtxtfunc(x\"🌞🏇🍼🌞🏇🍼🐇&👬🌖🌞👬🌖🌞🏇🍼🐇👬x\"",
        4.0,
        16.0,
        "factory, |\"xtxtfunc|(x\"🌞🏇🍼🌞🏇🍼|🐇&|👬🌖🌞👬🌖🌞🏇🍼|🐇👬x\"",
        17.0,
        "factory, |\"xtxtfunc|(x\"🌞🏇🍼🌞🏇🍼🐇|&👬🌖🌞👬🌖🌞🏇🍼|🐇👬x\"",
        WrappingIndent::Same,
        2.0,
        false,
    );
}

#[test]
fn issue_95686_critical_loop_forever_on_the_monospace_line_breaks_computer() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let factory = default_factory();
    assert_incremental_line_breaks(
        &factory,
        "\t\t\t\t\t\t<tr dmx-class:table-danger=\"(alt <= 50)\" dmx-class:table-warning=\"(alt <= 200)\" dmx-class:table-primary=\"(alt <= 400)\" dmx-class:table-info=\"(alt <= 800)\" dmx-class:table-success=\"(alt >= 400)\">",
        4.0,
        179.0,
        "\t\t\t\t\t\t<tr dmx-class:table-danger=\"(alt <= 50)\" dmx-class:table-warning=\"(alt <= 200)\" dmx-class:table-primary=\"(alt <= 400)\" dmx-class:table-info=\"(alt <= 800)\" |dmx-class:table-success=\"(alt >= 400)\">",
        1.0,
        "\t|\t|\t|\t|\t|\t|<|t|r| |d|m|x|-|c|l|a|s|s|:|t|a|b|l|e|-|d|a|n|g|e|r|=|\"|(|a|l|t| |<|=| |5|0|)|\"| |d|m|x|-|c|l|a|s|s|:|t|a|b|l|e|-|w|a|r|n|i|n|g|=|\"|(|a|l|t| |<|=| |2|0|0|)|\"| |d|m|x|-|c|l|a|s|s|:|t|a|b|l|e|-|p|r|i|m|a|r|y|=|\"|(|a|l|t| |<|=| |4|0|0|)|\"| |d|m|x|-|c|l|a|s|s|:|t|a|b|l|e|-|i|n|f|o|=|\"|(|a|l|t| |<|=| |8|0|0|)|\"| |d|m|x|-|c|l|a|s|s|:|t|a|b|l|e|-|s|u|c|c|e|s|s|=|\"|(|a|l|t| |>|=| |4|0|0|)|\"|>",
        WrappingIndent::Same,
        2.0,
        false,
    );
}

#[test]
fn issue_110392_occasional_crash_when_resize_with_panel_on_the_right() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let factory = default_factory();
    assert_incremental_line_breaks(
        &factory,
        "你好 **hello** **hello** **hello-world** hey there!",
        4.0,
        15.0,
        "你好 **hello** |**hello** |**hello-world**| hey there!",
        1.0,
        "你|好| |*|*|h|e|l|l|o|*|*| |*|*|h|e|l|l|o|*|*| |*|*|h|e|l|l|o|-|w|o|r|l|d|*|*| |h|e|y| |t|h|e|r|e|!",
        WrappingIndent::Same,
        1.6605405405405405,
        false,
    );
}

#[test]
fn monospace_line_breaks_computer_cjk_and_kinsoku_shori() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let factory = MonospaceLineBreaksComputerFactory::new("(", "\t)");
    let check = |annotated_text: &str| {
        assert_line_breaks(&factory, 4.0, 5.0, annotated_text, WrappingIndent::None, WordBreak::Normal);
    };
    check("aa \u{5b89}|\u{5b89}");
    check("\u{3042} \u{5b89}|\u{5b89}");
    check("\u{3042}\u{3042}|\u{5b89}\u{5b89}");
    check("aa |\u{5b89})\u{5b89}|\u{5b89}");
    check("aa \u{3042}|\u{5b89}\u{3042})|\u{5b89}");
    check("aa |(\u{5b89}aa|\u{5b89}");
}

#[test]
fn monospace_line_breaks_computer_two_cell_full_width_character_width() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let factory = MonospaceLineBreaksComputerFactory::new("(", "\t)");
    let text = JsString::from("\u{3042}\u{3042}\u{3042}\u{3042}");
    let wrap_with = |columns_for_full_width_char: f64, use_two_cell_fullwidth_characters: bool| {
        to_annotated_text(
            &text,
            &get_line_break_data(
                &factory,
                4.0,
                5.0,
                columns_for_full_width_char,
                WrappingIndent::None,
                WordBreak::Normal,
                false,
                &text,
                None,
                None,
                use_two_cell_fullwidth_characters,
            ),
        )
    };

    assert_eq!(
        (wrap_with(3.0, false), wrap_with(3.0, true), wrap_with(2.0, false)),
        (
            "\u{3042}|\u{3042}|\u{3042}|\u{3042}".to_owned(),
            "\u{3042}\u{3042}|\u{3042}\u{3042}".to_owned(),
            "\u{3042}\u{3042}|\u{3042}\u{3042}".to_owned(),
        )
    );
}

#[test]
fn monospace_line_breaks_computer_two_cell_width_determines_whether_continuation_indentation_fits() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let factory = MonospaceLineBreaksComputerFactory::new("(", "\t)");
    let text = JsString::from("    \u{3042}\u{3042}\u{3042}");
    let wrapped_indent_with = |line_text: &JsString,
                               break_after: f64,
                               columns_for_full_width_char: f64,
                               use_two_cell_fullwidth_characters: bool| {
        get_line_break_data(
            &factory,
            4.0,
            break_after,
            columns_for_full_width_char,
            WrappingIndent::Same,
            WordBreak::Normal,
            false,
            line_text,
            None,
            None,
            use_two_cell_fullwidth_characters,
        )
        .map(|data| data.wrapped_text_indent_length)
    };

    assert_eq!(
        (
            wrapped_indent_with(&text, 6.0, 3.0, false),
            wrapped_indent_with(&text, 6.0, 3.0, true),
            wrapped_indent_with(&text, 5.0, 1.0, false),
            wrapped_indent_with(&text, 5.0, 1.0, true),
        ),
        (Some(0), Some(4), Some(4), Some(0))
    );
}

#[test]
fn monospace_line_breaks_computer_two_cell_width_uses_shared_full_width_classification() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let factory = MonospaceLineBreaksComputerFactory::new("(", "\t)");
    let wrap_with = |text: &JsString, use_two_cell_fullwidth_characters: bool| {
        to_annotated_text(
            text,
            &get_line_break_data(
                &factory,
                4.0,
                5.0,
                3.0,
                WrappingIndent::None,
                WordBreak::Normal,
                false,
                text,
                None,
                None,
                use_two_cell_fullwidth_characters,
            ),
        )
    };
    let fullwidth_ascii = JsString::from("\u{FF21}".repeat(4));
    let vai_syllables = JsString::from("\u{A500}".repeat(4));

    assert_eq!(
        (
            wrap_with(&fullwidth_ascii, false),
            wrap_with(&fullwidth_ascii, true),
            wrap_with(&vai_syllables, false),
            wrap_with(&vai_syllables, true),
        ),
        (
            "\u{FF21}|\u{FF21}|\u{FF21}|\u{FF21}".to_owned(),
            "\u{FF21}\u{FF21}|\u{FF21}\u{FF21}".to_owned(),
            "\u{A500}|\u{A500}|\u{A500}|\u{A500}".to_owned(),
            "\u{A500}\u{A500}|\u{A500}\u{A500}".to_owned(),
        )
    );
}

#[test]
fn monospace_line_breaks_computer_two_cell_width_preserves_natural_control_character_metrics() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let factory = MonospaceLineBreaksComputerFactory::new("(", "\t)");
    let wrap_with = |text: &JsString, use_two_cell_fullwidth_characters: bool| {
        to_annotated_text(
            text,
            &get_line_break_data(
                &factory,
                4.0,
                4.0,
                3.0,
                WrappingIndent::None,
                WordBreak::Normal,
                false,
                text,
                None,
                None,
                use_two_cell_fullwidth_characters,
            ),
        )
    };
    let visible_columns_with_tab = |use_two_cell_fullwidth_characters: bool| {
        get_line_break_data(
            &factory,
            4.0,
            3.0,
            3.0,
            WrappingIndent::None,
            WordBreak::Normal,
            false,
            &JsString::from("\u{1}\t"),
            None,
            None,
            use_two_cell_fullwidth_characters,
        )
        .map(|data| data.break_offsets_visible_column)
    };
    let control_characters = JsString::from("\u{1}\u{1}");

    assert_eq!(
        (
            wrap_with(&control_characters, false),
            wrap_with(&control_characters, true),
            visible_columns_with_tab(false),
            visible_columns_with_tab(true),
        ),
        (
            "\u{1}|\u{1}".to_owned(),
            "\u{1}|\u{1}".to_owned(),
            Some(vec![3.0, 4.0]),
            Some(vec![3.0, 4.0]),
        )
    );
}

#[test]
fn monospace_line_breaks_computer_two_cell_width_treats_combining_marks_independently() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let factory = MonospaceLineBreaksComputerFactory::new("(", "\t)");
    let wrap_with = |text: &JsString, use_two_cell_fullwidth_characters: bool| {
        to_annotated_text(
            text,
            &get_line_break_data(
                &factory,
                4.0,
                4.0,
                3.0,
                WrappingIndent::None,
                WordBreak::Normal,
                false,
                text,
                None,
                None,
                use_two_cell_fullwidth_characters,
            ),
        )
    };
    let combined_character = JsString::from("\u{3042}\u{0301}aa");

    assert_eq!(
        (wrap_with(&combined_character, false), wrap_with(&combined_character, true)),
        ("\u{3042}|\u{0301}aa".to_owned(), "\u{3042}|\u{0301}aa".to_owned())
    );
}

#[test]
fn monospace_line_breaks_computer_two_cell_width_supports_incremental_line_breaks() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let factory = default_factory();

    // A font that lies about its full-width ratio: each ideograph still occupies exactly two cells.
    assert_incremental_line_breaks(
        &factory,
        "ああああああ",
        4.0,
        5.0,
        "ああ|ああ|ああ",
        7.0,
        "あああ|あああ",
        WrappingIndent::None,
        3.0,
        true,
    );

    assert_incremental_line_breaks(
        &factory,
        "    ああああ",
        4.0,
        8.0,
        "    ああ|ああ",
        10.0,
        "    あああ|あ",
        WrappingIndent::Same,
        3.0,
        true,
    );

    assert_incremental_line_breaks(
        &factory,
        "你好 **hello** **hello** **hello-world** hey there!",
        4.0,
        15.0,
        "你好 **hello** |**hello** |**hello-world**| hey there!",
        1.0,
        "你|好| |*|*|h|e|l|l|o|*|*| |*|*|h|e|l|l|o|*|*| |*|*|h|e|l|l|o|-|w|o|r|l|d|*|*| |h|e|y| |t|h|e|r|e|!",
        WrappingIndent::Same,
        1.6605405405405405,
        true,
    );
}

#[test]
fn monospace_line_breaks_computer_wrapping_indent_same() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let factory = MonospaceLineBreaksComputerFactory::new("", "\t ");
    assert_line_breaks(
        &factory,
        4.0,
        38.0,
        " *123456789012345678901234567890123456|7890",
        WrappingIndent::Same,
        WordBreak::Normal,
    );
}

#[test]
fn issue_16332_scroll_bar_overlaying_on_top_of_text() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let factory = MonospaceLineBreaksComputerFactory::new("", "\t ");
    assert_line_breaks(
        &factory,
        4.0,
        24.0,
        "a/ very/long/line/of/tex|t/that/expands/beyon|d/your/typical/line/|of/code/",
        WrappingIndent::Indent,
        WordBreak::Normal,
    );
}

#[test]
fn issue_35162_wrapping_indent_not_consistently_working() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let factory = MonospaceLineBreaksComputerFactory::new("", "\t ");
    let mapper = assert_line_breaks(
        &factory,
        4.0,
        24.0,
        "                t h i s |i s |a l |o n |g l |i n |e",
        WrappingIndent::Indent,
        WordBreak::Normal,
    );
    assert_eq!(mapper.expect("há quebras").wrapped_text_indent_length, "                    ".len());
}

#[test]
fn issue_75494_surrogate_pairs() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let factory = MonospaceLineBreaksComputerFactory::new("\t", " ");
    assert_line_breaks(
        &factory,
        4.0,
        49.0,
        "🐇👬🌖🌞🏇🍼🐇👬🌖🌞🏇🍼🐇👬🌖🌞🏇🍼🐇👬🌖🌞🏇🍼|🐇👬🌖🌞🏇🍼🐇👬🌖🌞🏇🍼🐇👬🌖🌞🏇🍼🐇👬🌖🌞🏇🍼|🐇👬",
        WrappingIndent::Same,
        WordBreak::Normal,
    );
}

#[test]
fn issue_75494_surrogate_pairs_overrun_1() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let factory = default_factory();
    assert_line_breaks(&factory, 4.0, 4.0, "🐇👬|&|🌞🌖", WrappingIndent::Same, WordBreak::Normal);
}

#[test]
fn issue_75494_surrogate_pairs_overrun_2() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let factory = default_factory();
    assert_line_breaks(
        &factory,
        4.0,
        17.0,
        "factory, |\"xtxtfunc|(x\"🌞🏇🍼🌞🏇🍼🐇|&👬🌖🌞👬🌖🌞🏇🍼|🐇👬x\"",
        WrappingIndent::Same,
        WordBreak::Normal,
    );
}

#[test]
fn monospace_line_breaks_computer_wrapping_indent_deep_indent() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let factory = MonospaceLineBreaksComputerFactory::new("", "\t ");
    let mapper = assert_line_breaks(
        &factory,
        4.0,
        26.0,
        "        W e A r e T e s t |i n g D e |e p I n d |e n t a t |i o n",
        WrappingIndent::DeepIndent,
        WordBreak::Normal,
    );
    assert_eq!(mapper.expect("há quebras").wrapped_text_indent_length, "                ".len());
}

#[test]
fn issue_33366_word_wrap_algorithm_behaves_differently_around_punctuation() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let factory = default_factory();
    assert_line_breaks(
        &factory,
        4.0,
        23.0,
        "this is a line of |text, text that sits |on a line",
        WrappingIndent::Same,
        WordBreak::Normal,
    );
}

#[test]
fn issue_152773_word_wrap_algorithm_behaves_differently_with_bracket_followed_by_comma() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let factory = default_factory();
    assert_line_breaks(
        &factory,
        4.0,
        24.0,
        "this is a line of |(text), text that sits |on a line",
        WrappingIndent::Same,
        WordBreak::Normal,
    );
}

#[test]
fn issue_112382_word_wrap_does_not_work_well_with_control_characters() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let factory = default_factory();
    assert_line_breaks(&factory, 4.0, 6.0, "\u{6}\u{6}\u{6}|\u{6}\u{6}\u{6}", WrappingIndent::Same, WordBreak::Normal);
}

#[test]
fn word_break_works_well_with_cjk_text_when_setting_normal() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let factory = default_factory();
    assert_line_breaks(&factory, 4.0, 5.0, "你好|1111", WrappingIndent::Same, WordBreak::Normal);
}

#[test]
fn word_break_works_well_with_cjk_text_when_setting_keep_all() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let factory = default_factory();
    assert_line_breaks(&factory, 4.0, 8.0, "你好1111", WrappingIndent::Same, WordBreak::KeepAll);
}

#[test]
fn issue_258022_wrap_on_escaped_line_feeds_should_work_correctly_after_editor_resize() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let factory = default_factory();

    // Test text with escaped line feeds - simulates a JSON string with \n
    // The \n should trigger a soft wrap when wrapOnEscapedLineFeeds is enabled
    let text = JsString::from("\"Short text with\\nescaped newline and an escaped\\\\nbackslash\"");
    let get = |break_after: f64, previous: Option<&ModelLineProjectionData>| {
        get_line_break_data(
            &factory,
            4.0,
            break_after,
            2.0,
            WrappingIndent::None,
            WordBreak::Normal,
            true,
            &text,
            previous,
            None,
            false,
        )
    };

    // First, compute line breaks with wrapOnEscapedLineFeeds enabled at initial width
    let initial_break_data = get(30.0, None);
    let initial_annotated_text = to_annotated_text(&text, &initial_break_data);

    // Verify the escaped \n triggers a wrap in the initial case
    assert!(initial_annotated_text.contains("with\\n"), "Initial case should wrap at escaped line feeds");

    // Now simulate editor resize by computing line breaks with different width using previous data
    // This triggers createLineBreaksFromPreviousLineBreaks which has the bug
    let resized_break_data = get(35.0, initial_break_data.as_ref());
    let resized_annotated_text = to_annotated_text(&text, &resized_break_data);

    // Compute fresh line breaks at the new width (without using previous data)
    // This uses createLineBreaks which correctly handles wrapOnEscapedLineFeeds
    let fresh_break_data = get(35.0, None);
    let fresh_annotated_text = to_annotated_text(&text, &fresh_break_data);

    // Fresh computation should still wrap at escaped line feeds
    assert!(fresh_annotated_text.contains("with\\n"), "Fresh computation should wrap at escaped line feeds");

    // The two results should be identical
    assert_eq!(
        resized_annotated_text, fresh_annotated_text,
        "Bug: Incremental and fresh computations differ for escaped line feeds.\nIncremental (resize): {resized_annotated_text}\nFresh computation:   {fresh_annotated_text}\nThe incremental path (createLineBreaksFromPreviousLineBreaks) doesn't handle wrapOnEscapedLineFeeds"
    );
}
