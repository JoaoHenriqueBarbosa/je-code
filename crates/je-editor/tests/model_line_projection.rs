//! Porte de `src/vs/editor/test/browser/viewModel/modelLineProjection.test.ts` (suítes `Editor ViewModel -
//! SplitLinesCollection` e `SplitLinesCollection`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` vira o `LeakGuard` criado no começo de cada caso.
//! - `ISimpleModel` do `createModel` é o `SimpleTextModel` abaixo, que implementa `SimpleModel`. O original não
//!   tem `getLineCount` nem os métodos de coluna de indentação (que o `CursorSimpleModel` do porte exige), então o
//!   modelo de uma linha só responde a eles com o texto único, e `tokenization.getLineTokens` (que devolve `null!`
//!   e nenhum caso chama) é um pânico.
//! - `createLineBreakData` passa `wrappedTextIndentWidth` ao campo `wrappedTextIndentLength`, como o original.
//! - `TestConfiguration` é o de `tests/support/test_configuration.rs`; as opções de quebra (`wrappingInfo`,
//!   `wordWrapBreak*Characters`, `wrappingIndent`, `wordBreak`, `effectiveFullwidthCharacterWidth`) são lidas dele
//!   no `create_lines_collection`, que serve aos dois `withSplitLinesCollection` (o primeiro lê
//!   `wrapOnEscapedLineFeeds` da configuração e o segundo o recebe como parâmetro).
//! - `ITokenizationSupport` do `setup` é o `TokensSupport`: devolve `_tokens[_lineIndex++]` a cada chamada. O
//!   `TokenizationRegistry` é global por thread e o registro é descartado no `teardown`, como no original.
//! - `assertAllMinimapLinesRenderingData` tem um `break` no fim do laço de `desired`: só a primeira combinação
//!   (todas as linhas necessárias) é conferida, e é a que o porte confere.
//! - `deepStrictEqual` de `{ inlineDecorations: undefined }` é `None`.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod support;

use std::cell::Cell;
use std::rc::Rc;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::JsString;
use je_base::lifecycle::IDisposable;
use je_base::strings;
use je_editor::config::editor_configuration::IEditorConfiguration;
use je_editor::config::editor_options::editor_font_info::FONT_INFO;
use je_editor::config::editor_options::effective_options::{
    EFFECTIVE_FULLWIDTH_CHARACTER_WIDTH, FullwidthCharacterWidth,
};
use je_editor::config::editor_options::registry::{
    WORD_BREAK, WORD_WRAP_BREAK_AFTER_CHARACTERS, WORD_WRAP_BREAK_BEFORE_CHARACTERS, WRAP_ON_ESCAPED_LINE_FEEDS,
};
use je_editor::config::editor_options::wrapping_indent::WRAPPING_INDENT;
use je_editor::config::editor_options::wrapping_info::WRAPPING_INFO;
use je_editor::cursor_common::{CursorSimpleModel, EditorConfiguration};
use je_editor::encoded_token_attributes::metadata_consts;
use je_editor::languages::{EncodedTokenizationResult, ITokenizationSupport, State, TokenizationResult};
use je_editor::line_tokens::{IViewLineTokens, LineTokens};
use je_editor::model::{ModelDecorationOptionsInit, ModelDeltaDecoration, InjectedTextOptions, PositionAffinity};
use je_editor::model_line_projection::{IModelLineProjection, SimpleModel, create_model_line_projection};
use je_editor::model_line_projection_data::{LineBreaksComputerFactory, ModelLineProjectionData, WordBreak};
use je_editor::monospace_line_breaks_computer::MonospaceLineBreaksComputerFactory;
use je_editor::null_tokenize::null_state;
use je_editor::position::Position;
use je_editor::range::Range;
use je_editor::text_model::{ModelDecorationOptions, TextModel};
use je_editor::tokenization_registry::tokenization_registry;
use je_editor::view_model_lines::{IViewModelLines, ViewModelLinesFromProjectedModel, WrappingStrategy};
use je_editor::view_model_types::ViewLineData;
use support::test_configuration::TestConfiguration;
use support::test_text_model::create_text_model_with;

fn pos(line_number: i32, column: i32) -> Position {
    Position::new(line_number, column)
}

/// `createSplitLine(splitLengths, breakingOffsetsVisibleColumn, wrappedTextIndentWidth)`.
fn create_split_line(
    split_lengths: &[usize],
    breaking_offsets_visible_column: &[f64],
    wrapped_text_indent_width: usize,
) -> Box<dyn IModelLineProjection> {
    create_model_line_projection(
        Some(create_line_break_data(split_lengths, breaking_offsets_visible_column, wrapped_text_indent_width)),
        true,
    )
}

/// `createLineBreakData(breakingLengths, breakingOffsetsVisibleColumn, wrappedTextIndentWidth)`.
fn create_line_break_data(
    breaking_lengths: &[usize],
    breaking_offsets_visible_column: &[f64],
    wrapped_text_indent_width: usize,
) -> ModelLineProjectionData {
    let mut sums: Vec<usize> = Vec::new();
    for (i, length) in breaking_lengths.iter().enumerate() {
        let previous = if i > 0 { sums[i - 1] } else { 0 };
        sums.push(previous + length);
    }
    ModelLineProjectionData::new(None, None, sums, breaking_offsets_visible_column.to_vec(), wrapped_text_indent_width)
}

/// O objeto devolvido por `createModel(text)`: a mesma linha para qualquer número de linha.
struct SimpleTextModel {
    text: JsString,
}

/// `createModel(text)`.
fn create_model(text: &str) -> SimpleTextModel {
    SimpleTextModel { text: JsString::from(text) }
}

impl CursorSimpleModel for SimpleTextModel {
    fn get_line_count(&self) -> i32 {
        1
    }

    fn get_line_content(&self, _line_number: i32) -> JsString {
        self.text.clone()
    }

    fn get_line_min_column(&self, _line_number: i32) -> i32 {
        1
    }

    fn get_line_max_column(&self, _line_number: i32) -> i32 {
        self.text.len() as i32 + 1
    }

    fn get_line_first_non_whitespace_column(&self, _line_number: i32) -> i32 {
        strings::first_non_whitespace_index(self.text.units()).map_or(0, |index| index as i32 + 1)
    }

    fn get_line_last_non_whitespace_column(&self, _line_number: i32) -> i32 {
        strings::last_non_whitespace_index(self.text.units(), self.text.len() as i64 - 1)
            .map_or(0, |index| index as i32 + 2)
    }

    fn normalize_position(&self, position: Position, _affinity: PositionAffinity) -> Position {
        position
    }

    fn get_line_indent_column(&self, _line_number: i32) -> i32 {
        strings::first_non_whitespace_index(self.text.units()).unwrap_or(self.text.len()) as i32 + 1
    }
}

impl SimpleModel for SimpleTextModel {
    fn get_line_tokens(&self, _line_number: i32) -> LineTokens {
        // `getLineTokens` devolve `null!` no original, e nenhum caso chega a usar os tokens.
        panic!("os tokens do modelo simples não existem");
    }

    fn get_line_length(&self, _line_number: i32) -> usize {
        self.text.len()
    }

    fn get_value_in_range(&self, range: &Range) -> JsString {
        let units = self.text.units();
        let start = usize::try_from(range.start_column - 1).unwrap_or(0).min(units.len());
        let end = usize::try_from(range.end_column - 1).unwrap_or(0).min(units.len()).max(start);
        JsString::from(&units[start..end])
    }
}

/// `withSplitLinesCollection` (das duas suítes): as linhas projetadas sobre `model`, com as opções de quebra lidas da
/// configuração.
fn create_lines_collection(
    configuration: &Rc<dyn IEditorConfiguration>,
    model: &Rc<TextModel>,
    wrap_on_escaped_line_feeds: bool,
) -> Rc<ViewModelLinesFromProjectedModel> {
    let options = configuration.options();
    let line_breaks_computer_factory: Rc<dyn LineBreaksComputerFactory> = Rc::new(MonospaceLineBreaksComputerFactory::new(
        options.get(WORD_WRAP_BREAK_BEFORE_CHARACTERS),
        options.get(WORD_WRAP_BREAK_AFTER_CHARACTERS),
    ));
    ViewModelLinesFromProjectedModel::new(
        1,
        model.clone(),
        line_breaks_computer_factory.clone(),
        line_breaks_computer_factory,
        options.get(FONT_INFO).clone(),
        f64::from(model.get_options().tab_size),
        WrappingStrategy::Simple,
        options.get(WRAPPING_INFO).wrapping_column,
        *options.get(WRAPPING_INDENT),
        if options.get(WORD_BREAK) == "keepAll" { WordBreak::KeepAll } else { WordBreak::Normal },
        wrap_on_escaped_line_feeds,
        *options.get(EFFECTIVE_FULLWIDTH_CHARACTER_WIDTH) == FullwidthCharacterWidth::TwoCells,
    )
}

mod editor_view_model_split_lines_collection {
    use super::*;

    #[test]
    fn split_line() {
        let _ds = ensure_no_disposables_are_leaked_in_test_suite();
        let mut model1 = create_model("My First LineMy Second LineAnd another one");
        let mut line1 = create_split_line(&[13, 14, 15], &[13.0, 13.0 + 14.0, 13.0 + 14.0 + 15.0], 0);

        assert_eq!(line1.get_view_line_count(), 3);
        assert_eq!(line1.get_view_line_content(&model1, 1, 0), "My First Line");
        assert_eq!(line1.get_view_line_content(&model1, 1, 1), "My Second Line");
        assert_eq!(line1.get_view_line_content(&model1, 1, 2), "And another one");
        assert_eq!(line1.get_view_line_max_column(&model1, 1, 0), 14);
        assert_eq!(line1.get_view_line_max_column(&model1, 1, 1), 15);
        assert_eq!(line1.get_view_line_max_column(&model1, 1, 2), 16);
        assert_eq!(
            [0, 1, 2].map(|line_index| line1.get_view_line_continues_with_wrapped_line(line_index)),
            [true, true, false]
        );
        for col in 1..=14 {
            assert_eq!(line1.get_model_column_of_view_position(0, col), col, "getInputColumnOfOutputPosition(0, {col})");
        }
        for col in 1..=15 {
            assert_eq!(
                line1.get_model_column_of_view_position(1, col),
                13 + col,
                "getInputColumnOfOutputPosition(1, {col})"
            );
        }
        for col in 1..=16 {
            assert_eq!(
                line1.get_model_column_of_view_position(2, col),
                13 + 14 + col,
                "getInputColumnOfOutputPosition(2, {col})"
            );
        }
        for col in 1..=13 {
            assert_eq!(line1.get_view_position_of_model_position(0, col, None), pos(0, col), "getOutputPositionOfInputPosition({col})");
        }
        for col in (1 + 13)..=(14 + 13) {
            assert_eq!(
                line1.get_view_position_of_model_position(0, col, None),
                pos(1, col - 13),
                "getOutputPositionOfInputPosition({col})"
            );
        }
        for col in (1 + 13 + 14)..=(15 + 14 + 13) {
            assert_eq!(
                line1.get_view_position_of_model_position(0, col, None),
                pos(2, col - 13 - 14),
                "getOutputPositionOfInputPosition({col})"
            );
        }

        model1 = create_model("My First LineMy Second LineAnd another one");
        line1 = create_split_line(&[13, 14, 15], &[13.0, 13.0 + 14.0, 13.0 + 14.0 + 15.0], 4);

        assert_eq!(line1.get_view_line_count(), 3);
        assert_eq!(line1.get_view_line_content(&model1, 1, 0), "My First Line");
        assert_eq!(line1.get_view_line_content(&model1, 1, 1), "    My Second Line");
        assert_eq!(line1.get_view_line_content(&model1, 1, 2), "    And another one");
        assert_eq!(line1.get_view_line_max_column(&model1, 1, 0), 14);
        assert_eq!(line1.get_view_line_max_column(&model1, 1, 1), 19);
        assert_eq!(line1.get_view_line_max_column(&model1, 1, 2), 20);

        let mut actual_view_column_mapping: Vec<Vec<i32>> = Vec::new();
        for line_index in 0..line1.get_view_line_count() {
            let mut actual_line_view_column_mapping = Vec::new();
            for col in 1..=line1.get_view_line_max_column(&model1, 1, line_index) {
                actual_line_view_column_mapping.push(line1.get_model_column_of_view_position(line_index, col));
            }
            actual_view_column_mapping.push(actual_line_view_column_mapping);
        }
        assert_eq!(
            actual_view_column_mapping,
            vec![
                vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14],
                vec![14, 14, 14, 14, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28],
                vec![28, 28, 28, 28, 28, 29, 30, 31, 32, 33, 34, 35, 36, 37, 38, 39, 40, 41, 42, 43],
            ]
        );

        for col in 1..=13 {
            assert_eq!(line1.get_view_position_of_model_position(0, col, None), pos(0, col), "6.getOutputPositionOfInputPosition({col})");
        }
        for col in (1 + 13)..=(14 + 13) {
            assert_eq!(
                line1.get_view_position_of_model_position(0, col, None),
                pos(1, 4 + col - 13),
                "7.getOutputPositionOfInputPosition({col})"
            );
        }
        for col in (1 + 13 + 14)..=(15 + 14 + 13) {
            assert_eq!(
                line1.get_view_position_of_model_position(0, col, None),
                pos(2, 4 + col - 13 - 14),
                "8.getOutputPositionOfInputPosition({col})"
            );
        }
    }

    /// `withSplitLinesCollection(text, callback)` da primeira suíte.
    fn with_split_lines_collection(text: &str, callback: impl FnOnce(&Rc<TextModel>, &Rc<ViewModelLinesFromProjectedModel>)) {
        let configuration: Rc<dyn IEditorConfiguration> = TestConfiguration::new(&support::test_view_model::no_options());
        let model = create_text_model_with(text, None, Default::default(), None);
        let wrap_on_escaped_line_feeds = *configuration.options().get(WRAP_ON_ESCAPED_LINE_FEEDS);

        let lines_collection = create_lines_collection(&configuration, &model, wrap_on_escaped_line_feeds);

        callback(&model, &lines_collection);

        lines_collection.dispose();
        model.dispose();
        configuration.dispose();
    }

    const HELLO_TEXT: [&str; 6] = [
        "int main() {",
        "\tprintf(\"Hello world!\");",
        "}",
        "int main() {",
        "\tprintf(\"Hello world!\");",
        "}",
    ];

    #[test]
    fn invalid_line_numbers() {
        let _ds = ensure_no_disposables_are_leaked_in_test_suite();
        let text = HELLO_TEXT.join("\n");

        with_split_lines_collection(&text, |_model, lines_collection| {
            assert_eq!(lines_collection.get_view_line_count(), 6);

            // getOutputIndentGuide
            assert_eq!(lines_collection.get_view_lines_indent_guides(-1, -1), vec![0]);
            assert_eq!(lines_collection.get_view_lines_indent_guides(0, 0), vec![0]);
            assert_eq!(lines_collection.get_view_lines_indent_guides(1, 1), vec![0]);
            assert_eq!(lines_collection.get_view_lines_indent_guides(2, 2), vec![1]);
            assert_eq!(lines_collection.get_view_lines_indent_guides(3, 3), vec![0]);
            assert_eq!(lines_collection.get_view_lines_indent_guides(4, 4), vec![0]);
            assert_eq!(lines_collection.get_view_lines_indent_guides(5, 5), vec![1]);
            assert_eq!(lines_collection.get_view_lines_indent_guides(6, 6), vec![0]);
            assert_eq!(lines_collection.get_view_lines_indent_guides(7, 7), vec![0]);

            assert_eq!(lines_collection.get_view_lines_indent_guides(0, 7), vec![0, 1, 0, 0, 1, 0]);

            // getOutputLineContent
            assert_eq!(lines_collection.get_view_line_content(-1), "int main() {");
            assert_eq!(lines_collection.get_view_line_content(0), "int main() {");
            assert_eq!(lines_collection.get_view_line_content(1), "int main() {");
            assert_eq!(lines_collection.get_view_line_content(2), "\tprintf(\"Hello world!\");");
            assert_eq!(lines_collection.get_view_line_content(3), "}");
            assert_eq!(lines_collection.get_view_line_content(4), "int main() {");
            assert_eq!(lines_collection.get_view_line_content(5), "\tprintf(\"Hello world!\");");
            assert_eq!(lines_collection.get_view_line_content(6), "}");
            assert_eq!(lines_collection.get_view_line_content(7), "}");

            // getOutputLineMinColumn
            for view_line_number in -1..=7 {
                assert_eq!(lines_collection.get_view_line_min_column(view_line_number), 1, "{view_line_number}");
            }

            // getOutputLineMaxColumn
            assert_eq!(lines_collection.get_view_line_max_column(-1), 13);
            assert_eq!(lines_collection.get_view_line_max_column(0), 13);
            assert_eq!(lines_collection.get_view_line_max_column(1), 13);
            assert_eq!(lines_collection.get_view_line_max_column(2), 25);
            assert_eq!(lines_collection.get_view_line_max_column(3), 2);
            assert_eq!(lines_collection.get_view_line_max_column(4), 13);
            assert_eq!(lines_collection.get_view_line_max_column(5), 25);
            assert_eq!(lines_collection.get_view_line_max_column(6), 2);
            assert_eq!(lines_collection.get_view_line_max_column(7), 2);

            // convertOutputPositionToInputPosition
            assert_eq!(lines_collection.convert_view_position_to_model_position(-1, 1), Position::new(1, 1));
            assert_eq!(lines_collection.convert_view_position_to_model_position(0, 1), Position::new(1, 1));
            assert_eq!(lines_collection.convert_view_position_to_model_position(1, 1), Position::new(1, 1));
            assert_eq!(lines_collection.convert_view_position_to_model_position(2, 1), Position::new(2, 1));
            assert_eq!(lines_collection.convert_view_position_to_model_position(3, 1), Position::new(3, 1));
            assert_eq!(lines_collection.convert_view_position_to_model_position(4, 1), Position::new(4, 1));
            assert_eq!(lines_collection.convert_view_position_to_model_position(5, 1), Position::new(5, 1));
            assert_eq!(lines_collection.convert_view_position_to_model_position(6, 1), Position::new(6, 1));
            assert_eq!(lines_collection.convert_view_position_to_model_position(7, 1), Position::new(6, 1));
            assert_eq!(lines_collection.convert_view_position_to_model_position(8, 1), Position::new(6, 1));
        });
    }

    #[test]
    fn issue_3662() {
        let _ds = ensure_no_disposables_are_leaked_in_test_suite();
        let text = HELLO_TEXT.join("\n");

        with_split_lines_collection(&text, |model, lines_collection| {
            lines_collection.set_hidden_areas(&[Range::new(1, 1, 3, 1), Range::new(5, 1, 6, 1)]);

            let view_line_count = lines_collection.get_view_line_count();
            assert_eq!(view_line_count, 1, "getOutputLineCount()");

            let model_line_count = model.get_line_count();
            for line_number in 0..=(model_line_count + 1) {
                let in_model = line_number >= 1 && line_number <= model_line_count;
                let line_min_column = if in_model { model.get_line_min_column(line_number) } else { 1 };
                let line_max_column = if in_model { model.get_line_max_column(line_number) } else { 1 };
                for column in (line_min_column - 1)..=(line_max_column + 1) {
                    let view_position = lines_collection.convert_model_position_to_view_position(
                        line_number,
                        column,
                        PositionAffinity::None,
                        false,
                        false,
                    );

                    // validate view position
                    let mut view_line_number = view_position.line_number;
                    let mut view_column = view_position.column;
                    if view_line_number < 1 {
                        view_line_number = 1;
                    }
                    let line_count = lines_collection.get_view_line_count();
                    if view_line_number > line_count {
                        view_line_number = line_count;
                    }
                    let view_min_column = lines_collection.get_view_line_min_column(view_line_number);
                    let view_max_column = lines_collection.get_view_line_max_column(view_line_number);
                    if view_column < view_min_column {
                        view_column = view_min_column;
                    }
                    if view_column > view_max_column {
                        view_column = view_max_column;
                    }
                    let valid_view_position = Position::new(view_line_number, view_column);
                    assert_eq!(view_position, valid_view_position, "model->view for {line_number}, {column}");
                }
            }

            for line_number in 0..=(view_line_count + 1) {
                let line_min_column = lines_collection.get_view_line_min_column(line_number);
                let line_max_column = lines_collection.get_view_line_max_column(line_number);
                for column in (line_min_column - 1)..=(line_max_column + 1) {
                    let model_position = lines_collection.convert_view_position_to_model_position(line_number, column);
                    let valid_model_position = model.validate_position(model_position);
                    assert_eq!(model_position, valid_model_position, "view->model for {line_number}, {column}");
                }
            }
        });
    }
}

mod split_lines_collection {
    use super::*;

    const TEXT: [&str; 8] = [
        "class Nice {",
        "\tfunction hi() {",
        "\t\tconsole.log(\"Hello world\");",
        "\t}",
        "\tfunction hello() {",
        "\t\tconsole.log(\"Hello world, this is a somewhat longer line\");",
        "\t}",
        "}",
    ];

    /// `_tokens`: `{ startIndex, value }` por token, uma lista por linha.
    const TOKENS: [&[(u32, u32)]; 8] = [
        &[(0, 1), (5, 2), (6, 3), (10, 4)],
        &[(0, 5), (1, 6), (9, 7), (10, 8), (12, 9)],
        &[(0, 10), (2, 11), (9, 12), (10, 13), (13, 14), (14, 15), (27, 16)],
        &[(0, 17)],
        &[(0, 18), (1, 19), (9, 20), (10, 21), (15, 22)],
        &[(0, 23), (2, 24), (9, 25), (10, 26), (13, 27), (14, 28), (59, 29)],
        &[(0, 30)],
        &[(0, 31)],
    ];

    const LANGUAGE_ID: &str = "modelModeTest1";

    /// O `tokenizationSupport` do `setup`: entrega os tokens de `_tokens` na ordem em que o modelo pede as linhas.
    struct TokensSupport {
        line_index: Cell<usize>,
    }

    impl ITokenizationSupport for TokensSupport {
        fn get_initial_state(&self) -> State {
            null_state()
        }

        fn tokenize(&self, _line: &JsString, _has_eol: bool, _state: &State) -> TokenizationResult {
            unreachable!("`tokenize: undefined!` no original")
        }

        fn tokenize_encoded(&self, _line: &JsString, _has_eol: bool, state: &State) -> EncodedTokenizationResult {
            let line_index = self.line_index.get();
            self.line_index.set(line_index + 1);
            let tokens = TOKENS[line_index];

            let mut result = Vec::with_capacity(2 * tokens.len());
            for (start_index, value) in tokens {
                result.push(*start_index);
                result.push(value << metadata_consts::FOREGROUND_OFFSET);
            }
            EncodedTokenizationResult { tokens: result, font_info: Vec::new(), end_state: state.clone() }
        }
    }

    /// O `setup` e o `teardown` da suíte: registra a linguagem, cria o modelo e força a tokenização, roda o caso e
    /// descarta tudo.
    fn with_model(callback: impl FnOnce(&Rc<TextModel>)) {
        let registration =
            tokenization_registry().register(LANGUAGE_ID, Rc::new(TokensSupport { line_index: Cell::new(0) }));
        let model = create_text_model_with(TEXT.join("\n").as_str(), Some(LANGUAGE_ID), Default::default(), None);
        // force tokenization
        model.tokenization().force_tokenization(model.get_line_count());

        callback(&model);

        model.dispose();
        registration.dispose();
    }

    /// `ITestViewLineToken`: `(endIndex, value)`.
    type ExpectedToken = (i32, u32);

    /// `ITestMinimapLineRenderingData`.
    struct ExpectedLine {
        content: &'static str,
        min_column: i32,
        max_column: i32,
        tokens: Vec<ExpectedToken>,
    }

    fn expected_line(content: &'static str, min_column: i32, max_column: i32, tokens: &[ExpectedToken]) -> ExpectedLine {
        ExpectedLine { content, min_column, max_column, tokens: tokens.to_vec() }
    }

    /// `assertViewLineTokens(_actual, expected)`.
    fn assert_view_line_tokens(actual: &dyn IViewLineTokens, expected: &[ExpectedToken]) {
        let actual: Vec<ExpectedToken> =
            (0..actual.get_count()).map(|i| (actual.get_end_offset(i), actual.get_foreground(i))).collect();
        assert_eq!(actual, expected);
    }

    /// `assertMinimapLineRenderingData(actual, expected)`.
    fn assert_minimap_line_rendering_data(actual: &Option<ViewLineData>, expected: Option<&ExpectedLine>) {
        let (actual, expected) = match (actual, expected) {
            (None, None) => return,
            (Some(actual), Some(expected)) => (actual, expected),
            (_, None) => panic!("esperava uma linha vazia"),
            (None, Some(_)) => panic!("esperava os dados de uma linha"),
        };
        assert_eq!(actual.content, expected.content);
        assert_eq!(actual.min_column, expected.min_column);
        assert_eq!(actual.max_column, expected.max_column);
        assert_view_line_tokens(&*actual.tokens, &expected.tokens);
    }

    /// `assertMinimapLinesRenderingData(actual, expected)`.
    fn assert_minimap_lines_rendering_data(actual: &[Option<ViewLineData>], expected: &[Option<&ExpectedLine>]) {
        assert_eq!(actual.len(), expected.len());
        for i in 0..expected.len() {
            assert_minimap_line_rendering_data(&actual[i], expected[i]);
        }
    }

    /// `assertAllMinimapLinesRenderingData(splitLinesCollection, all)`.
    fn assert_all_minimap_lines_rendering_data(split_lines_collection: &ViewModelLinesFromProjectedModel, all: &[&ExpectedLine]) {
        let line_count = all.len() as i32;
        for line in 1..=line_count {
            assert_eq!(
                split_lines_collection.get_view_line_data(line).content,
                split_lines_collection.get_view_line_content(line)
            );
        }

        for start in 1..=line_count {
            for end in start..=line_count {
                let count = (end - start + 1) as u32;
                // O laço de `desired` do original para na primeira combinação: todas as linhas necessárias.
                let desired = (1u32 << count) - 1;
                let mut needed = Vec::new();
                let mut expected = Vec::new();
                for i in 0..count {
                    let is_needed = desired & (1 << i) != 0;
                    needed.push(is_needed);
                    expected.push(is_needed.then(|| all[(start - 1) as usize + i as usize]));
                }
                let actual = split_lines_collection.get_view_lines_data(start, end, &needed);

                assert_minimap_lines_rendering_data(&actual, &expected);
            }
        }
    }

    fn class_line() -> ExpectedLine {
        expected_line("class Nice {", 1, 13, &[(5, 1), (6, 2), (10, 3), (12, 4)])
    }

    fn function_hi_line() -> ExpectedLine {
        expected_line("\tfunction hi() {", 1, 17, &[(1, 5), (9, 6), (10, 7), (12, 8), (16, 9)])
    }

    fn close_tab_line(value: u32) -> ExpectedLine {
        expected_line("\t}", 1, 3, &[(2, value)])
    }

    fn function_hello_line() -> ExpectedLine {
        expected_line("\tfunction hello() {", 1, 20, &[(1, 18), (9, 19), (10, 20), (15, 21), (19, 22)])
    }

    fn close_brace_line() -> ExpectedLine {
        expected_line("}", 1, 2, &[(1, 31)])
    }

    /// As linhas da visão quebrada em 30 colunas (`wrappingIndent: 'indent'`), da quarta em diante a mesma lista
    /// que o caso com texto injetado repete.
    fn wrapped_lines() -> Vec<ExpectedLine> {
        vec![
            class_line(),
            function_hi_line(),
            expected_line("\t\tconsole.log(\"Hello ", 1, 22, &[(2, 10), (9, 11), (10, 12), (13, 13), (14, 14), (21, 15)]),
            expected_line("            world\");", 13, 21, &[(18, 15), (20, 16)]),
            close_tab_line(17),
            function_hello_line(),
            expected_line("\t\tconsole.log(\"Hello ", 1, 22, &[(2, 23), (9, 24), (10, 25), (13, 26), (14, 27), (21, 28)]),
            expected_line("            world, this is a ", 13, 30, &[(29, 28)]),
            expected_line("            somewhat longer ", 13, 29, &[(28, 28)]),
            expected_line("            line\");", 13, 20, &[(17, 28), (19, 29)]),
            close_tab_line(30),
            close_brace_line(),
        ]
    }

    /// `withSplitLinesCollection(model, wordWrap, wordWrapColumn, wrapOnEscapedLineFeeds, callback)`.
    fn with_split_lines_collection(
        model: &Rc<TextModel>,
        word_wrap: &str,
        word_wrap_column: i32,
        wrap_on_escaped_line_feeds: bool,
        callback: impl FnOnce(&ViewModelLinesFromProjectedModel),
    ) {
        use je_base::dynamic::Dynamic;

        let configuration: Rc<dyn IEditorConfiguration> = TestConfiguration::new(&Dynamic::object([
            ("wordWrap", Dynamic::from(word_wrap)),
            ("wordWrapColumn", Dynamic::from(word_wrap_column)),
            ("wrappingIndent", Dynamic::from("indent")),
        ]));
        let lines_collection = create_lines_collection(&configuration, model, wrap_on_escaped_line_feeds);

        callback(&lines_collection);

        configuration.dispose();
    }

    fn assert_model_positions_visible(split_lines_collection: &ViewModelLinesFromProjectedModel, visible: [bool; 8]) {
        for (index, expected) in visible.into_iter().enumerate() {
            assert_eq!(
                split_lines_collection.model_position_is_visible(index as i32 + 1, 1),
                expected,
                "modelPositionIsVisible({}, 1)",
                index + 1
            );
        }
    }

    #[test]
    fn get_view_lines_data_no_wrapping() {
        let _ds = ensure_no_disposables_are_leaked_in_test_suite();
        with_model(|model| {
            with_split_lines_collection(model, "off", 0, false, |split_lines_collection| {
                assert_eq!(split_lines_collection.get_view_line_count(), 8);
                assert_model_positions_visible(split_lines_collection, [true; 8]);

                let expected = [
                    class_line(),
                    function_hi_line(),
                    expected_line(
                        "\t\tconsole.log(\"Hello world\");",
                        1,
                        30,
                        &[(2, 10), (9, 11), (10, 12), (13, 13), (14, 14), (27, 15), (29, 16)],
                    ),
                    close_tab_line(17),
                    function_hello_line(),
                    expected_line(
                        "\t\tconsole.log(\"Hello world, this is a somewhat longer line\");",
                        1,
                        62,
                        &[(2, 23), (9, 24), (10, 25), (13, 26), (14, 27), (59, 28), (61, 29)],
                    ),
                    close_tab_line(30),
                    close_brace_line(),
                ];

                assert_all_minimap_lines_rendering_data(split_lines_collection, &expected.iter().collect::<Vec<_>>());

                split_lines_collection.set_hidden_areas(&[Range::new(2, 1, 4, 1)]);
                assert_eq!(split_lines_collection.get_view_line_count(), 5);
                assert_model_positions_visible(
                    split_lines_collection,
                    [true, false, false, false, true, true, true, true],
                );

                assert_all_minimap_lines_rendering_data(
                    split_lines_collection,
                    &[&expected[0], &expected[4], &expected[5], &expected[6], &expected[7]],
                );
            });
        });
    }

    #[test]
    fn get_view_lines_data_with_wrapping() {
        let _ds = ensure_no_disposables_are_leaked_in_test_suite();
        with_model(|model| {
            with_split_lines_collection(model, "wordWrapColumn", 30, false, |split_lines_collection| {
                assert_eq!(split_lines_collection.get_view_line_count(), 12);
                assert_model_positions_visible(split_lines_collection, [true; 8]);

                let expected = wrapped_lines();

                assert_all_minimap_lines_rendering_data(split_lines_collection, &expected.iter().collect::<Vec<_>>());

                split_lines_collection.set_hidden_areas(&[Range::new(2, 1, 4, 1)]);
                assert_eq!(split_lines_collection.get_view_line_count(), 8);
                assert_model_positions_visible(
                    split_lines_collection,
                    [true, false, false, false, true, true, true, true],
                );

                assert_all_minimap_lines_rendering_data(
                    split_lines_collection,
                    &[
                        &expected[0],
                        &expected[5],
                        &expected[6],
                        &expected[7],
                        &expected[8],
                        &expected[9],
                        &expected[10],
                        &expected[11],
                    ],
                );
            });
        });
    }

    #[test]
    fn get_view_lines_data_with_wrapping_and_injected_text() {
        let _ds = ensure_no_disposables_are_leaked_in_test_suite();
        with_model(|model| {
            model.delta_decorations(
                &[],
                &[ModelDeltaDecoration {
                    range: Range::new(1, 9, 1, 9),
                    options: ModelDecorationOptions::register(&ModelDecorationOptionsInit {
                        description: "example".to_owned(),
                        after: Some(InjectedTextOptions {
                            content: "very very long injected text that causes a line break".to_owned(),
                            inline_class_name: Some("myClassName".to_owned()),
                            ..Default::default()
                        }),
                        show_if_collapsed: true,
                        ..Default::default()
                    }),
                }],
                0,
            );

            with_split_lines_collection(model, "wordWrapColumn", 30, false, |split_lines_collection| {
                assert_eq!(split_lines_collection.get_view_line_count(), 14);

                assert_eq!(split_lines_collection.get_view_line_max_column(1), 24);

                let mut expected = vec![
                    expected_line(
                        "class Nivery very long ",
                        1,
                        24,
                        &[(5, 1), (6, 2), (8, 3), (23, 1)],
                    ),
                    expected_line("    injected text that causes ", 5, 31, &[(30, 1)]),
                    expected_line("    a line breakce {", 5, 21, &[(16, 1), (18, 3), (20, 4)]),
                ];
                expected.extend(wrapped_lines().into_iter().skip(1));

                assert_all_minimap_lines_rendering_data(
                    split_lines_collection,
                    &expected.iter().take(12).collect::<Vec<_>>(),
                );

                let data = split_lines_collection.get_view_lines_data(1, 14, &[true; 14]);
                let inline_decorations: Vec<Option<Vec<(i32, i32)>>> = data
                    .iter()
                    .map(|line| {
                        let line = line.as_ref().expect("a linha foi pedida");
                        line.inline_decorations.as_ref().map(|decorations| {
                            decorations
                                .iter()
                                .map(|decoration| (decoration.range.start_column - 1, decoration.range.end_column - 1))
                                .collect()
                        })
                    })
                    .collect();
                let mut expected_inline_decorations: Vec<Option<Vec<(i32, i32)>>> =
                    vec![Some(vec![(8, 23)]), Some(vec![(4, 30)]), Some(vec![(4, 16)])];
                expected_inline_decorations.resize(14, None);
                assert_eq!(inline_decorations, expected_inline_decorations);
            });
        });
    }
}
