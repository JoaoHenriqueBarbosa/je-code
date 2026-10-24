//! Porte de `src/vs/editor/contrib/comment/test/browser/lineCommentCommand.test.ts` (as suítes
//! `Editor Contrib - Line Comment Command`, `ignoreEmptyLines false`, `Editor Contrib - Line Comment As Block
//! Comment`, `... As Block Comment 2` e `Editor Contrib - Line Comment in mixed modes`), caso a caso e na mesma
//! ordem. Cada `suite` do original vira um `mod`.
//!
//! Notas de tradução:
//! - `testCommand` recebe a seleção já ajustada e a `commandFactory` sem o `accessor` (ver
//!   `tests/support/test_command.rs`). O `prepare` de `createTestCommandHelper` (registra a linguagem
//!   `commentMode` e a sua configuração nos serviços) vira o registro nos serviços de um modelo com linguagens de
//!   verdade (`create_language_model_services`), e o comando recebe o `ILanguageConfigurationService` desses
//!   mesmos serviços, como no original. O caminho é `test_command_on_model`.
//! - `createTestCommandHelper(commentsConfig, commandFactory)` é o [`CommentCommandTester`]: a configuração de
//!   comentários e os argumentos do `LineCommentCommand` (`Type`, `insertSpace`, `ignoreEmptyLines`) ficam no
//!   valor, e [`CommentCommandTester::run`] é a função que o helper devolve.
//! - `ensureNoDisposablesAreLeakedInTestSuite()` vira o `LeakGuard` criado no começo de cada caso.
//! - O `ISimpleModel` do teste é o [`SimpleModel`], que implementa o `CursorSimpleModel` (o que a produção pede
//!   no lugar do `ISimpleModel`, só com `getLineContent`) a partir das linhas; só `get_line_content` é consultado.
//! - Os `IPreflightData` do original (união discriminada por `supported`) são o enum [`PreflightData`]; o
//!   `if (!r.supported) throw new Error('unexpected')` é o [`supported`].
//! - `OuterMode` e `InnerMode` dos modos mistos são as funções `register_outer_mode` e `register_inner_mode`,
//!   que devolvem o `Disposable` com os registros (como em `tests/model.rs`).

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod support;

use std::rc::Rc;

use common::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite};
use je_base::JsString;
use je_base::lifecycle::Disposable;
use je_base::strings::{first_non_whitespace_index, last_non_whitespace_index};
use je_editor::comment::line_comment_command::{
    InsertionPoint, LineCommentCommand, LinePreflightData, PreflightData, PreflightDataSupported, Type,
};
use je_editor::cursor_common::CursorSimpleModel;
use je_editor::encoded_token_attributes::{color_id, metadata_consts};
use je_editor::language_configuration::{CommentRule, LanguageConfiguration, LineComment, LineCommentConfig};
use je_editor::language_configuration_registry::ILanguageConfigurationService;
use je_editor::language_service::ILanguageService;
use je_editor::languages::{EncodedTokenizationResult, ILanguageIdCodec, ITokenizationSupport, State, TokenizationResult};
use je_editor::model::PositionAffinity;
use je_editor::modes_registry::LanguageExtensionPoint;
use je_editor::null_tokenize::null_state;
use je_editor::position::Position;
use je_editor::selection::Selection;
use je_editor::tokenization_registry::tokenization_registry;
use support::test_command::test_command_on_model;
use support::test_language_configuration_service::TestLanguageConfigurationService;
use support::test_text_model::{LanguageModelServices, create_language_model_services, instantiate_text_model_with_languages};

/// `languageId` de `createTestCommandHelper`.
const COMMENT_LANGUAGE_ID: &str = "commentMode";

/// `{ lineComment, blockComment }` com `lineComment` como texto simples.
fn comment_rule(line_comment: Option<&str>, block_comment: Option<(&str, &str)>) -> CommentRule {
    CommentRule {
        line_comment: line_comment.map(|token| LineComment::Token(token.to_owned())),
        block_comment: block_comment.map(|(start, end)| (start.to_owned(), end.to_owned())),
    }
}

/// `class OuterMode`/`class InnerMode` sem o tokenizador: registra a linguagem e a sua configuração de
/// comentários nos serviços e devolve o descarte dos dois registros.
fn register_comment_mode(services: &LanguageModelServices, language_id: &str, comments: CommentRule) -> Rc<Disposable> {
    let mode = Disposable::new();
    mode.register(services.language_service.register_language(LanguageExtensionPoint::new(language_id)));
    mode.register(services.language_configuration_service.register(
        language_id,
        LanguageConfiguration { comments: Some(comments), ..LanguageConfiguration::default() },
        None,
    ));
    mode
}

/// `createTestCommandHelper(commentsConfig, commandFactory)`.
struct CommentCommandTester {
    comments: CommentRule,
    type_: Type,
    insert_space: bool,
    ignore_empty_lines: bool,
}

impl CommentCommandTester {
    fn new(comments: CommentRule, type_: Type, insert_space: bool, ignore_empty_lines: bool) -> Self {
        Self { comments, type_, insert_space, ignore_empty_lines }
    }

    /// O `(lines, selection, expectedLines, expectedSelection) => void` que o helper devolve.
    fn run(&self, ds: &LeakGuard, lines: &[&str], selection: Selection, expected_lines: &[&str], expected_selection: Selection) {
        // `prepare`
        let services = create_language_model_services(ds);
        ds.add_rc(register_comment_mode(&services, COMMENT_LANGUAGE_ID, self.comments.clone()));
        let model = instantiate_text_model_with_languages(&services, lines.join("\n").as_str(), COMMENT_LANGUAGE_ID);
        let language_configuration_service: Rc<dyn ILanguageConfigurationService> = services.language_configuration_service.clone();
        let (type_, insert_space, ignore_empty_lines) = (self.type_, self.insert_space, self.ignore_empty_lines);
        test_command_on_model(
            &model,
            selection,
            move |sel| {
                Box::new(LineCommentCommand::new(language_configuration_service, sel, 4, type_, insert_space, ignore_empty_lines, None))
            },
            expected_lines,
            expected_selection,
            false,
        );
    }
}

/// `createSimpleModel(lines)`: um modelo só com o conteúdo das linhas.
struct SimpleModel {
    lines: Vec<JsString>,
}

impl SimpleModel {
    fn new(lines: &[&str]) -> Self {
        Self { lines: lines.iter().map(|line| JsString::from(*line)).collect() }
    }
}

impl CursorSimpleModel for SimpleModel {
    fn get_line_count(&self) -> i32 {
        self.lines.len() as i32
    }

    fn get_line_content(&self, line_number: i32) -> JsString {
        self.lines[line_number as usize - 1].clone()
    }

    fn get_line_min_column(&self, _line_number: i32) -> i32 {
        1
    }

    fn get_line_max_column(&self, line_number: i32) -> i32 {
        self.get_line_content(line_number).len() as i32 + 1
    }

    fn get_line_first_non_whitespace_column(&self, line_number: i32) -> i32 {
        first_non_whitespace_index(self.get_line_content(line_number).units()).map_or(0, |index| index as i32 + 1)
    }

    fn get_line_last_non_whitespace_column(&self, line_number: i32) -> i32 {
        let line = self.get_line_content(line_number);
        last_non_whitespace_index(line.units(), line.len() as i64 - 1).map_or(0, |index| index as i32 + 2)
    }

    fn normalize_position(&self, position: Position, _affinity: PositionAffinity) -> Position {
        position
    }

    fn get_line_indent_column(&self, line_number: i32) -> i32 {
        let line = self.get_line_content(line_number);
        first_non_whitespace_index(line.units()).unwrap_or(line.len()) as i32 + 1
    }
}

/// `createBasicLinePreflightData(commentTokens)`.
fn create_basic_line_preflight_data(comment_tokens: &[&str]) -> Vec<LinePreflightData> {
    comment_tokens
        .iter()
        .map(|comment_string| {
            let comment_str = JsString::from(*comment_string);
            LinePreflightData {
                ignore: false,
                comment_str_offset: 0,
                comment_str_length: comment_str.len() as i32,
                comment_str,
            }
        })
        .collect()
}

/// `if (!r.supported) { throw new Error('unexpected'); }`.
fn supported(data: PreflightData) -> PreflightDataSupported {
    match data {
        PreflightData::Supported(data) => data,
        PreflightData::Unsupported => panic!("unexpected"),
    }
}

mod editor_contrib_line_comment_command {
    use super::*;

    /// `testLineCommentCommand`.
    fn test_line_comment_command() -> CommentCommandTester {
        CommentCommandTester::new(comment_rule(Some("!@#"), Some(("<!@#", "#@!>"))), Type::Toggle, true, true)
    }

    /// `testAddLineCommentCommand`.
    fn test_add_line_comment_command() -> CommentCommandTester {
        CommentCommandTester::new(comment_rule(Some("!@#"), Some(("<!@#", "#@!>"))), Type::ForceAdd, true, true)
    }

    /// `testLineCommentCommandTokenFirstColumn`.
    fn test_line_comment_command_token_first_column() -> CommentCommandTester {
        CommentCommandTester::new(
            CommentRule {
                line_comment: Some(LineComment::Config(LineCommentConfig { comment: "!@#".to_owned(), no_indent: Some(true) })),
                block_comment: Some(("<!@#".to_owned(), "#@!>".to_owned())),
            },
            Type::Toggle,
            true,
            true,
        )
    }

    #[test]
    fn comment_single_line() {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        test_line_comment_command().run(
            &ds,
            &["some text", "\tsome more text"],
            Selection::new(1, 1, 1, 1),
            &["!@# some text", "\tsome more text"],
            Selection::new(1, 5, 1, 5),
        );
    }

    #[test]
    fn case_insensitive() {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        let test_line_comment_command = CommentCommandTester::new(comment_rule(Some("rem"), None), Type::Toggle, true, true);

        test_line_comment_command.run(&ds, &["REM some text"], Selection::new(1, 1, 1, 1), &["some text"], Selection::new(1, 1, 1, 1));
    }

    #[test]
    fn comment_with_token_column_fixed() {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        test_line_comment_command_token_first_column().run(
            &ds,
            &["some text", "\tsome more text"],
            Selection::new(2, 1, 2, 1),
            &["some text", "!@# \tsome more text"],
            Selection::new(2, 5, 2, 5),
        );
    }

    #[test]
    fn analyze_lines() {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        let service = ds.add_rc(TestLanguageConfigurationService::new());

        let r = supported(LineCommentCommand::analyze_lines(
            Type::Toggle,
            true,
            &SimpleModel::new(&["\t\t", "    ", "    c", "\t\td"]),
            create_basic_line_preflight_data(&["//", "rem", "!@#", "!@#"]),
            1,
            true,
            false,
            &*service,
            "plaintext",
        ));

        assert!(!r.should_remove_comments);

        // Does not change `commentStr`
        assert_eq!(r.lines[0].comment_str, JsString::from("//"));
        assert_eq!(r.lines[1].comment_str, JsString::from("rem"));
        assert_eq!(r.lines[2].comment_str, JsString::from("!@#"));
        assert_eq!(r.lines[3].comment_str, JsString::from("!@#"));

        // Fills in `isWhitespace`
        assert!(r.lines[0].ignore);
        assert!(r.lines[1].ignore);
        assert!(!r.lines[2].ignore);
        assert!(!r.lines[3].ignore);

        // Fills in `commentStrOffset`
        assert_eq!(r.lines[0].comment_str_offset, 2);
        assert_eq!(r.lines[1].comment_str_offset, 4);
        assert_eq!(r.lines[2].comment_str_offset, 4);
        assert_eq!(r.lines[3].comment_str_offset, 2);

        let r = supported(LineCommentCommand::analyze_lines(
            Type::Toggle,
            true,
            &SimpleModel::new(&["\t\t", "    rem ", "    !@# c", "\t\t!@#d"]),
            create_basic_line_preflight_data(&["//", "rem", "!@#", "!@#"]),
            1,
            true,
            false,
            &*service,
            "plaintext",
        ));

        assert!(r.should_remove_comments);

        // Does not change `commentStr`
        assert_eq!(r.lines[0].comment_str, JsString::from("//"));
        assert_eq!(r.lines[1].comment_str, JsString::from("rem"));
        assert_eq!(r.lines[2].comment_str, JsString::from("!@#"));
        assert_eq!(r.lines[3].comment_str, JsString::from("!@#"));

        // Fills in `isWhitespace`
        assert!(r.lines[0].ignore);
        assert!(!r.lines[1].ignore);
        assert!(!r.lines[2].ignore);
        assert!(!r.lines[3].ignore);

        // Fills in `commentStrOffset`
        assert_eq!(r.lines[0].comment_str_offset, 2);
        assert_eq!(r.lines[1].comment_str_offset, 4);
        assert_eq!(r.lines[2].comment_str_offset, 4);
        assert_eq!(r.lines[3].comment_str_offset, 2);

        // Fills in `commentStrLength`
        assert_eq!(r.lines[0].comment_str_length, 2);
        assert_eq!(r.lines[1].comment_str_length, 4);
        assert_eq!(r.lines[2].comment_str_length, 4);
        assert_eq!(r.lines[3].comment_str_length, 3);
    }

    #[test]
    fn normalize_insertion_point() {
        let _ds = ensure_no_disposables_are_leaked_in_test_suite();

        // `mixedArr` do original alterna linha e deslocamento; aqui são os pares `(linha, deslocamento)`.
        let run_test = |mixed: &[(&str, i32)], tab_size: i32, expected: &[i32], test_name: &str| {
            let model = SimpleModel::new(&mixed.iter().map(|(line, _)| *line).collect::<Vec<_>>());
            let mut offsets: Vec<InsertionPoint> =
                mixed.iter().map(|(_, offset)| InsertionPoint { ignore: false, comment_str_offset: *offset }).collect();
            LineCommentCommand::normalize_insertion_point(&model, &mut offsets, 1, tab_size);
            let actual: Vec<i32> = offsets.iter().map(|item| item.comment_str_offset).collect();
            assert_eq!(actual, expected, "{test_name}");
        };

        // Bug 16696:[comment] comments not aligned in this case
        run_test(&[("  XX", 2), ("    YY", 4)], 4, &[0, 0], "Bug 16696");

        run_test(&[("\t\t\tXX", 3), ("    \tYY", 5), ("        ZZ", 8), ("\t\tTT", 2)], 4, &[2, 5, 8, 2], "Test1");

        run_test(&[("\t\t\t   XX", 6), ("    \t\t\t\tYY", 8), ("        ZZ", 8), ("\t\t    TT", 6)], 4, &[2, 5, 8, 2], "Test2");

        run_test(&[("\t\t", 2), ("\t\t\t", 3), ("\t\t\t\t", 4), ("\t\t\t", 3)], 4, &[2, 2, 2, 2], "Test3");

        run_test(&[("\t\t", 2), ("\t\t\t", 3), ("\t\t\t\t", 4), ("\t\t\t", 3), ("    ", 4)], 2, &[2, 2, 2, 2, 4], "Test4");

        run_test(&[("\t\t", 2), ("\t\t\t", 3), ("\t\t\t\t", 4), ("\t\t\t", 3), ("    ", 4)], 4, &[1, 1, 1, 1, 4], "Test5");

        run_test(&[(" \t", 2), ("  \t", 3), ("   \t", 4), ("    ", 4), ("\t", 1)], 4, &[2, 3, 4, 4, 1], "Test6");

        run_test(&[(" \t\t", 3), ("  \t\t", 4), ("   \t\t", 5), ("    \t", 5), ("\t", 1)], 4, &[2, 3, 4, 4, 1], "Test7");

        run_test(&[("\t", 1), ("    ", 4)], 4, &[1, 4], "Test8:4");
        run_test(&[("\t", 1), ("   ", 3)], 4, &[0, 0], "Test8:3");
        run_test(&[("\t", 1), ("  ", 2)], 4, &[0, 0], "Test8:2");
        run_test(&[("\t", 1), (" ", 1)], 4, &[0, 0], "Test8:1");
        run_test(&[("\t", 1), ("", 0)], 4, &[0, 0], "Test8:0");
    }

    #[test]
    fn detects_indentation() {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        test_line_comment_command().run(
            &ds,
            &["\tsome text", "\tsome more text"],
            Selection::new(2, 2, 1, 1),
            &["\t!@# some text", "\t!@# some more text"],
            Selection::new(2, 2, 1, 1),
        );
    }

    #[test]
    fn detects_mixed_indentation() {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        test_line_comment_command().run(
            &ds,
            &["\tsome text", "    some more text"],
            Selection::new(2, 2, 1, 1),
            &["\t!@# some text", "    !@# some more text"],
            Selection::new(2, 2, 1, 1),
        );
    }

    #[test]
    fn ignores_whitespace_lines() {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        test_line_comment_command().run(
            &ds,
            &["\tsome text", "\t   ", "", "\tsome more text"],
            Selection::new(4, 2, 1, 1),
            &["\t!@# some text", "\t   ", "", "\t!@# some more text"],
            Selection::new(4, 2, 1, 1),
        );
    }

    #[test]
    fn removes_its_own() {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        test_line_comment_command().run(
            &ds,
            &["\t!@# some text", "\t   ", "\t\t!@# some more text"],
            Selection::new(3, 2, 1, 1),
            &["\tsome text", "\t   ", "\t\tsome more text"],
            Selection::new(3, 2, 1, 1),
        );
    }

    #[test]
    fn works_in_only_whitespace() {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        test_line_comment_command().run(
            &ds,
            &["\t    ", "\t", "\t\tsome more text"],
            Selection::new(3, 1, 1, 1),
            &["\t!@#     ", "\t!@# ", "\t\tsome more text"],
            Selection::new(3, 1, 1, 1),
        );
    }

    #[test]
    fn bug_9697_whitespace_before_comment_token() {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        test_line_comment_command().run(
            &ds,
            &["\t !@#first", "\tsecond line"],
            Selection::new(1, 1, 1, 1),
            &["\t first", "\tsecond line"],
            Selection::new(1, 1, 1, 1),
        );
    }

    #[test]
    fn bug_10162_line_comment_before_caret() {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        test_line_comment_command().run(
            &ds,
            &["first!@#", "\tsecond line"],
            Selection::new(1, 1, 1, 1),
            &["!@# first!@#", "\tsecond line"],
            Selection::new(1, 5, 1, 5),
        );
    }

    #[test]
    fn comment_single_line_leading_whitespace() {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        test_line_comment_command().run(
            &ds,
            &["first!@#", "\tsecond line"],
            Selection::new(2, 3, 2, 1),
            &["first!@#", "\t!@# second line"],
            Selection::new(2, 7, 2, 1),
        );
    }

    #[test]
    fn ignores_invisible_selection() {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        test_line_comment_command().run(
            &ds,
            &["first", "\tsecond line", "third line", "fourth line", "fifth"],
            Selection::new(2, 1, 1, 1),
            &["!@# first", "\tsecond line", "third line", "fourth line", "fifth"],
            Selection::new(2, 1, 1, 5),
        );
    }

    #[test]
    fn multiple_lines() {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        test_line_comment_command().run(
            &ds,
            &["first", "\tsecond line", "third line", "fourth line", "fifth"],
            Selection::new(2, 4, 1, 1),
            &["!@# first", "!@# \tsecond line", "third line", "fourth line", "fifth"],
            Selection::new(2, 8, 1, 5),
        );
    }

    #[test]
    fn multiple_modes_on_multiple_lines() {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        test_line_comment_command().run(
            &ds,
            &["first", "\tsecond line", "third line", "fourth line", "fifth"],
            Selection::new(4, 4, 3, 1),
            &["first", "\tsecond line", "!@# third line", "!@# fourth line", "fifth"],
            Selection::new(4, 8, 3, 5),
        );
    }

    #[test]
    fn toggle_single_line() {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        test_line_comment_command().run(
            &ds,
            &["first", "\tsecond line", "third line", "fourth line", "fifth"],
            Selection::new(1, 1, 1, 1),
            &["!@# first", "\tsecond line", "third line", "fourth line", "fifth"],
            Selection::new(1, 5, 1, 5),
        );

        test_line_comment_command().run(
            &ds,
            &["!@# first", "\tsecond line", "third line", "fourth line", "fifth"],
            Selection::new(1, 4, 1, 4),
            &["first", "\tsecond line", "third line", "fourth line", "fifth"],
            Selection::new(1, 1, 1, 1),
        );
    }

    #[test]
    fn toggle_multiple_lines() {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        test_line_comment_command().run(
            &ds,
            &["first", "\tsecond line", "third line", "fourth line", "fifth"],
            Selection::new(2, 4, 1, 1),
            &["!@# first", "!@# \tsecond line", "third line", "fourth line", "fifth"],
            Selection::new(2, 8, 1, 5),
        );

        test_line_comment_command().run(
            &ds,
            &["!@# first", "!@# \tsecond line", "third line", "fourth line", "fifth"],
            Selection::new(2, 7, 1, 4),
            &["first", "\tsecond line", "third line", "fourth line", "fifth"],
            Selection::new(2, 3, 1, 1),
        );
    }

    #[test]
    fn issue_5964_ctrl_slash_to_create_comment_when_cursor_is_at_the_beginning_of_the_line_puts_the_cursor_in_a_strange_position() {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        test_line_comment_command().run(
            &ds,
            &["first", "\tsecond line", "third line", "fourth line", "fifth"],
            Selection::new(1, 1, 1, 1),
            &["!@# first", "\tsecond line", "third line", "fourth line", "fifth"],
            Selection::new(1, 5, 1, 5),
        );
    }

    #[test]
    fn issue_35673_comment_hotkeys_throws_the_cursor_before_the_comment() {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        test_line_comment_command().run(
            &ds,
            &["first", "", "\tsecond line", "third line", "fourth line", "fifth"],
            Selection::new(2, 1, 2, 1),
            &["first", "!@# ", "\tsecond line", "third line", "fourth line", "fifth"],
            Selection::new(2, 5, 2, 5),
        );

        test_line_comment_command().run(
            &ds,
            &["first", "\t", "\tsecond line", "third line", "fourth line", "fifth"],
            Selection::new(2, 2, 2, 2),
            &["first", "\t!@# ", "\tsecond line", "third line", "fourth line", "fifth"],
            Selection::new(2, 6, 2, 6),
        );
    }

    #[test]
    fn issue_2837_add_line_comment_fault_when_blank_lines_involved() {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        test_add_line_comment_command().run(
            &ds,
            &[
                "    if displayName == \"\":",
                "        displayName = groupName",
                "    description = getAttr(attributes, \"description\")",
                "    mailAddress = getAttr(attributes, \"mail\")",
                "",
                "    print \"||Group name|%s|\" % displayName",
                "    print \"||Description|%s|\" % description",
                "    print \"||Email address|[mailto:%s]|\" % mailAddress`",
            ],
            Selection::new(1, 1, 8, 56),
            &[
                "    !@# if displayName == \"\":",
                "    !@#     displayName = groupName",
                "    !@# description = getAttr(attributes, \"description\")",
                "    !@# mailAddress = getAttr(attributes, \"mail\")",
                "",
                "    !@# print \"||Group name|%s|\" % displayName",
                "    !@# print \"||Description|%s|\" % description",
                "    !@# print \"||Email address|[mailto:%s]|\" % mailAddress`",
            ],
            Selection::new(1, 1, 8, 60),
        );
    }

    #[test]
    fn issue_47004_toggle_comments_shouldnt_move_cursor() {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        test_add_line_comment_command().run(
            &ds,
            &["    A line", "    Another line"],
            Selection::new(2, 7, 1, 1),
            &["    !@# A line", "    !@# Another line"],
            Selection::new(2, 11, 1, 1),
        );
    }

    #[test]
    fn insert_space_false() {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        let test_line_comment_command = CommentCommandTester::new(comment_rule(Some("!@#"), None), Type::Toggle, false, true);

        test_line_comment_command.run(&ds, &["some text"], Selection::new(1, 1, 1, 1), &["!@#some text"], Selection::new(1, 4, 1, 4));
    }

    #[test]
    fn insert_space_false_does_not_remove_space() {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        let test_line_comment_command = CommentCommandTester::new(comment_rule(Some("!@#"), None), Type::Toggle, false, true);

        test_line_comment_command.run(&ds, &["!@#    some text"], Selection::new(1, 1, 1, 1), &["    some text"], Selection::new(1, 1, 1, 1));
    }
}

mod ignore_empty_lines_false {
    use super::*;

    /// `testLineCommentCommand`.
    fn test_line_comment_command() -> CommentCommandTester {
        CommentCommandTester::new(comment_rule(Some("!@#"), Some(("<!@#", "#@!>"))), Type::Toggle, true, false)
    }

    #[test]
    fn does_not_ignore_whitespace_lines() {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        test_line_comment_command().run(
            &ds,
            &["\tsome text", "\t   ", "", "\tsome more text"],
            Selection::new(4, 2, 1, 1),
            &["!@# \tsome text", "!@# \t   ", "!@# ", "!@# \tsome more text"],
            Selection::new(4, 6, 1, 5),
        );
    }

    #[test]
    fn removes_its_own() {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        test_line_comment_command().run(
            &ds,
            &["\t!@# some text", "\t   ", "\t\t!@# some more text"],
            Selection::new(3, 2, 1, 1),
            &["\tsome text", "\t   ", "\t\tsome more text"],
            Selection::new(3, 2, 1, 1),
        );
    }

    #[test]
    fn works_in_only_whitespace() {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        test_line_comment_command().run(
            &ds,
            &["\t    ", "\t", "\t\tsome more text"],
            Selection::new(3, 1, 1, 1),
            &["\t!@#     ", "\t!@# ", "\t\tsome more text"],
            Selection::new(3, 1, 1, 1),
        );
    }

    #[test]
    fn comments_single_line() {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        test_line_comment_command().run(
            &ds,
            &["some text", "\tsome more text"],
            Selection::new(1, 1, 1, 1),
            &["!@# some text", "\tsome more text"],
            Selection::new(1, 5, 1, 5),
        );
    }

    #[test]
    fn detects_indentation() {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        test_line_comment_command().run(
            &ds,
            &["\tsome text", "\tsome more text"],
            Selection::new(2, 2, 1, 1),
            &["\t!@# some text", "\t!@# some more text"],
            Selection::new(2, 2, 1, 1),
        );
    }
}

mod editor_contrib_line_comment_as_block_comment {
    use super::*;

    /// `testLineCommentCommand`.
    fn test_line_comment_command() -> CommentCommandTester {
        CommentCommandTester::new(comment_rule(Some(""), Some(("(", ")"))), Type::Toggle, true, true)
    }

    #[test]
    fn fall_back_to_block_comment_command() {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        test_line_comment_command().run(
            &ds,
            &["first", "\tsecond line", "third line", "fourth line", "fifth"],
            Selection::new(1, 1, 1, 1),
            &["( first )", "\tsecond line", "third line", "fourth line", "fifth"],
            Selection::new(1, 3, 1, 3),
        );
    }

    #[test]
    fn fall_back_to_block_comment_command_toggle() {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        test_line_comment_command().run(
            &ds,
            &["(first)", "\tsecond line", "third line", "fourth line", "fifth"],
            Selection::new(1, 7, 1, 2),
            &["first", "\tsecond line", "third line", "fourth line", "fifth"],
            Selection::new(1, 6, 1, 1),
        );
    }

    #[test]
    fn bug_9513_expand_single_line_to_uncomment_auto_block() {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        test_line_comment_command().run(
            &ds,
            &["first", "\tsecond line", "third line", "fourth line", "fifth"],
            Selection::new(1, 1, 1, 1),
            &["( first )", "\tsecond line", "third line", "fourth line", "fifth"],
            Selection::new(1, 3, 1, 3),
        );
    }

    #[test]
    fn bug_9691_always_expand_selection_to_line_boundaries() {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        test_line_comment_command().run(
            &ds,
            &["first", "\tsecond line", "third line", "fourth line", "fifth"],
            Selection::new(3, 2, 1, 3),
            &["( first", "\tsecond line", "third line )", "fourth line", "fifth"],
            Selection::new(3, 2, 1, 5),
        );

        test_line_comment_command().run(
            &ds,
            &["(first", "\tsecond line", "third line)", "fourth line", "fifth"],
            Selection::new(3, 11, 1, 2),
            &["first", "\tsecond line", "third line", "fourth line", "fifth"],
            Selection::new(3, 11, 1, 1),
        );
    }
}

mod editor_contrib_line_comment_as_block_comment_2 {
    use super::*;

    /// `testLineCommentCommand`.
    fn test_line_comment_command() -> CommentCommandTester {
        CommentCommandTester::new(comment_rule(None, Some(("<!@#", "#@!>"))), Type::Toggle, true, true)
    }

    /// As linhas que os casos de `can remove` compartilham.
    const CAN_REMOVE_LINES: [&str; 5] = ["\t\tfirst\t    ", "\t\tsecond line", "\tthird line", "fourth line", "\t\t<!@#fifth#@!>\t\t"];

    /// As linhas esperadas dos casos de `can remove` (a quinta sem os tokens).
    const CAN_REMOVE_EXPECTED_LINES: [&str; 5] = ["\t\tfirst\t    ", "\t\tsecond line", "\tthird line", "fourth line", "\t\tfifth\t\t"];

    #[test]
    fn no_selection_uses_indentation() {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        test_line_comment_command().run(
            &ds,
            &["\t\tfirst\t    ", "\t\tsecond line", "\tthird line", "fourth line", "\t\t<!@#fifth#@!>\t\t"],
            Selection::new(1, 1, 1, 1),
            &["\t\t<!@# first\t     #@!>", "\t\tsecond line", "\tthird line", "fourth line", "\t\t<!@#fifth#@!>\t\t"],
            Selection::new(1, 1, 1, 1),
        );

        test_line_comment_command().run(
            &ds,
            &["\t\t<!@#first\t    #@!>", "\t\tsecond line", "\tthird line", "fourth line", "\t\t<!@#fifth#@!>\t\t"],
            Selection::new(1, 1, 1, 1),
            &["\t\tfirst\t   ", "\t\tsecond line", "\tthird line", "fourth line", "\t\t<!@#fifth#@!>\t\t"],
            Selection::new(1, 1, 1, 1),
        );
    }

    #[test]
    fn can_remove() {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        test_line_comment_command().run(
            &ds,
            &CAN_REMOVE_LINES,
            Selection::new(5, 1, 5, 1),
            &CAN_REMOVE_EXPECTED_LINES,
            Selection::new(5, 1, 5, 1),
        );

        test_line_comment_command().run(
            &ds,
            &CAN_REMOVE_LINES,
            Selection::new(5, 3, 5, 3),
            &CAN_REMOVE_EXPECTED_LINES,
            Selection::new(5, 3, 5, 3),
        );

        test_line_comment_command().run(
            &ds,
            &CAN_REMOVE_LINES,
            Selection::new(5, 4, 5, 4),
            &CAN_REMOVE_EXPECTED_LINES,
            Selection::new(5, 3, 5, 3),
        );

        test_line_comment_command().run(
            &ds,
            &CAN_REMOVE_LINES,
            Selection::new(5, 16, 5, 3),
            &CAN_REMOVE_EXPECTED_LINES,
            Selection::new(5, 8, 5, 3),
        );

        test_line_comment_command().run(
            &ds,
            &CAN_REMOVE_LINES,
            Selection::new(5, 12, 5, 7),
            &CAN_REMOVE_EXPECTED_LINES,
            Selection::new(5, 8, 5, 3),
        );

        test_line_comment_command().run(
            &ds,
            &CAN_REMOVE_LINES,
            Selection::new(5, 18, 5, 18),
            &CAN_REMOVE_EXPECTED_LINES,
            Selection::new(5, 10, 5, 10),
        );
    }

    #[test]
    fn issue_993_remove_comment_does_not_work_consistently_in_html() {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        test_line_comment_command().run(
            &ds,
            &["     asd qwe", "     asd qwe", ""],
            Selection::new(1, 1, 3, 1),
            &["     <!@# asd qwe", "     asd qwe #@!>", ""],
            Selection::new(1, 1, 3, 1),
        );

        test_line_comment_command().run(
            &ds,
            &["     <!@#asd qwe", "     asd qwe#@!>", ""],
            Selection::new(1, 1, 3, 1),
            &["     asd qwe", "     asd qwe", ""],
            Selection::new(1, 1, 3, 1),
        );
    }
}

mod editor_contrib_line_comment_in_mixed_modes {
    use super::*;

    const OUTER_LANGUAGE_ID: &str = "outerMode";
    const INNER_LANGUAGE_ID: &str = "innerMode";

    /// O tokenizador do `OuterMode`: a linha que começa com dois espaços é da linguagem interna e as outras, da
    /// externa, num token só.
    struct OuterModeTokenizationSupport {
        language_id_codec: Rc<dyn ILanguageIdCodec>,
    }

    impl ITokenizationSupport for OuterModeTokenizationSupport {
        fn get_initial_state(&self) -> State {
            null_state()
        }

        fn tokenize(&self, _line: &JsString, _has_eol: bool, _state: &State) -> TokenizationResult {
            unreachable!("`tokenize` lança `not implemented` no original")
        }

        fn tokenize_encoded(&self, line: &JsString, _has_eol: bool, state: &State) -> EncodedTokenizationResult {
            let language_id = if line.units().starts_with(&[u16::from(b' '), u16::from(b' ')]) { INNER_LANGUAGE_ID } else { OUTER_LANGUAGE_ID };
            let encoded_language_id = self.language_id_codec.encode_language_id(language_id);

            let tokens = vec![
                0,
                (color_id::DEFAULT_FOREGROUND << metadata_consts::FOREGROUND_OFFSET) | (encoded_language_id << metadata_consts::LANGUAGEID_OFFSET),
            ];
            EncodedTokenizationResult { tokens, font_info: Vec::new(), end_state: state.clone() }
        }
    }

    /// `class OuterMode`: a linguagem, a configuração de comentários e o tokenizador.
    fn register_outer_mode(services: &LanguageModelServices, comments: CommentRule) -> Rc<Disposable> {
        let mode = register_comment_mode(services, OUTER_LANGUAGE_ID, comments);
        let support = OuterModeTokenizationSupport { language_id_codec: services.language_service.language_id_codec() };
        mode.register(tokenization_registry().register(OUTER_LANGUAGE_ID, Rc::new(support)));
        mode
    }

    /// `testLineCommentCommand`: o `testCommand` com `forceTokenization` e o `setup` que registra os dois modos.
    fn test_line_comment_command(ds: &LeakGuard, lines: &[&str], selection: Selection, expected_lines: &[&str], expected_selection: Selection) {
        // `setup`
        let services = create_language_model_services(ds);
        ds.add_rc(register_outer_mode(&services, comment_rule(Some("//"), Some(("/*", "*/")))));
        ds.add_rc(register_comment_mode(&services, INNER_LANGUAGE_ID, comment_rule(None, Some(("{/*", "*/}")))));

        let model = instantiate_text_model_with_languages(&services, lines.join("\n").as_str(), OUTER_LANGUAGE_ID);
        let language_configuration_service: Rc<dyn ILanguageConfigurationService> = services.language_configuration_service.clone();
        test_command_on_model(
            &model,
            selection,
            move |sel| Box::new(LineCommentCommand::new(language_configuration_service, sel, 4, Type::Toggle, true, true, None)),
            expected_lines,
            expected_selection,
            true,
        );
    }

    #[test]
    fn issue_24047_part_1_commenting_code_in_jsx_files() {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        test_line_comment_command(
            &ds,
            &[
                "import React from 'react';",
                "const Loader = () => (",
                "  <div>",
                "    Loading...",
                "  </div>",
                ");",
                "export default Loader;",
            ],
            Selection::new(1, 1, 7, 22),
            &[
                "// import React from 'react';",
                "// const Loader = () => (",
                "//   <div>",
                "//     Loading...",
                "//   </div>",
                "// );",
                "// export default Loader;",
            ],
            Selection::new(1, 4, 7, 25),
        );
    }

    #[test]
    fn issue_24047_part_2_commenting_code_in_jsx_files() {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        test_line_comment_command(
            &ds,
            &[
                "import React from 'react';",
                "const Loader = () => (",
                "  <div>",
                "    Loading...",
                "  </div>",
                ");",
                "export default Loader;",
            ],
            Selection::new(3, 4, 3, 4),
            &[
                "import React from 'react';",
                "const Loader = () => (",
                "  {/* <div> */}",
                "    Loading...",
                "  </div>",
                ");",
                "export default Loader;",
            ],
            Selection::new(3, 8, 3, 8),
        );
    }

    #[test]
    fn issue_36173_commenting_code_in_jsx_tag_body() {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        test_line_comment_command(
            &ds,
            &["<div>", "  {123}", "</div>"],
            Selection::new(2, 4, 2, 4),
            &["<div>", "  {/* {123} */}", "</div>"],
            Selection::new(2, 8, 2, 8),
        );
    }
}
