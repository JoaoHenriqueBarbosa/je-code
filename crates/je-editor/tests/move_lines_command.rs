//! Porte de `src/vs/editor/contrib/linesOperations/test/browser/moveLinesCommand.test.ts` (as suítes
//! `Editor Contrib - Move Lines Command`, `... honors Indentation Rules` e `... honors onEnter Rules`), caso a
//! caso e na mesma ordem. Cada `suite` do original vira um `mod`.
//!
//! Notas de tradução:
//! - `testCommand` recebe a seleção já ajustada e a `commandFactory` sem o `accessor` (ver
//!   `tests/support/test_command.rs`); `forceTokenization` fica `false`, como no original.
//! - `testMoveLinesUpOrDownCommand` e `testMoveLinesUpOrDownWithIndentCommand` criam um
//!   `TestLanguageConfigurationService` próprio quando o caso não passa um (`languageConfigurationService`
//!   opcional), e o descartam no fim: aqui ele é descartado pelo `LeakGuard` do caso.
//! - `IndentRulesMode` e `EnterRulesMode` registram a linguagem e a sua configuração. No original o modelo do
//!   `testCommand` e o comando usam serviços separados; aqui o modelo nasce dos mesmos serviços em que o modo foi
//!   registrado (`create_language_model_services`), como no teste do `LineCommentCommand`, e o comando recebe o
//!   `ILanguageConfigurationService` desses serviços. O caminho é `test_command_on_model`.
//! - `ensureNoDisposablesAreLeakedInTestSuite()` vira o `LeakGuard` criado no começo de cada caso.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod support;

use std::rc::Rc;

use common::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite};
use je_base::lifecycle::Disposable;
use je_editor::language_service::ILanguageService;
use je_editor::config::editor_options::EditorAutoIndentStrategy;
use je_editor::language_configuration::{IndentationRule, LanguageConfiguration};
use je_editor::language_configuration_registry::ILanguageConfigurationService;
use je_editor::modes_registry::LanguageExtensionPoint;
use je_editor::move_lines_command::MoveLinesCommand;
use je_editor::selection::Selection;
use support::re;
use support::test_command::{test_command, test_command_on_model};
use support::test_language_configuration_service::TestLanguageConfigurationService;
use support::test_text_model::{create_language_model_services, instantiate_text_model_with_languages};

/// `MoveLinesDirection`.
#[derive(Clone, Copy)]
enum MoveLinesDirection {
    Up,
    Down,
}

impl MoveLinesDirection {
    /// `direction === MoveLinesDirection.Up ? false : true`.
    fn is_moving_down(self) -> bool {
        matches!(self, MoveLinesDirection::Down)
    }
}

/// `testMoveLinesDownCommand(lines, selection, expectedLines, expectedSelection)`.
fn test_move_lines_down_command(ds: &LeakGuard, lines: &[&str], selection: Selection, expected_lines: &[&str], expected_selection: Selection) {
    test_move_lines_up_or_down_command(ds, MoveLinesDirection::Down, lines, selection, expected_lines, expected_selection);
}

/// `testMoveLinesUpCommand(lines, selection, expectedLines, expectedSelection)`.
fn test_move_lines_up_command(ds: &LeakGuard, lines: &[&str], selection: Selection, expected_lines: &[&str], expected_selection: Selection) {
    test_move_lines_up_or_down_command(ds, MoveLinesDirection::Up, lines, selection, expected_lines, expected_selection);
}

/// `testMoveLinesUpOrDownCommand` sem `languageConfigurationService`: nasce um `TestLanguageConfigurationService`.
fn test_move_lines_up_or_down_command(
    ds: &LeakGuard,
    direction: MoveLinesDirection,
    lines: &[&str],
    selection: Selection,
    expected_lines: &[&str],
    expected_selection: Selection,
) {
    let language_configuration_service: Rc<dyn ILanguageConfigurationService> = ds.add_rc(TestLanguageConfigurationService::new()) as Rc<TestLanguageConfigurationService>;
    test_command(
        lines,
        None,
        selection,
        |sel| {
            Box::new(MoveLinesCommand::new(
                sel,
                direction.is_moving_down(),
                EditorAutoIndentStrategy::Advanced,
                language_configuration_service,
            ))
        },
        expected_lines,
        expected_selection,
        false,
    );
}

/// `testMoveLinesUpWithIndentCommand(languageId, ...)` com `languageConfigurationService` e o modo registrado
/// nos serviços do modelo.
fn test_move_lines_with_indent_command_on_mode(
    ds: &LeakGuard,
    direction: MoveLinesDirection,
    language_id: &str,
    configuration: LanguageConfiguration,
    lines: &[&str],
    selection: Selection,
    expected_lines: &[&str],
    expected_selection: Selection,
) {
    let services = create_language_model_services(ds);
    // `new IndentRulesMode(...)` / `new EnterRulesMode(...)`
    let mode = Disposable::new();
    mode.register(services.language_service.register_language(LanguageExtensionPoint::new(language_id)));
    mode.register(services.language_configuration_service.register(language_id, configuration, None));
    ds.add_rc(mode);

    let model = instantiate_text_model_with_languages(&services, lines.join("\n").as_str(), language_id);
    let language_configuration_service: Rc<dyn ILanguageConfigurationService> = services.language_configuration_service.clone();
    test_command_on_model(
        &model,
        selection,
        move |sel| {
            Box::new(MoveLinesCommand::new(
                sel,
                direction.is_moving_down(),
                EditorAutoIndentStrategy::Full,
                language_configuration_service,
            ))
        },
        expected_lines,
        expected_selection,
        false,
    );
}

mod editor_contrib_move_lines_command {
    use super::*;

    #[test]
    fn move_first_up_last_down_disabled() {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();

        test_move_lines_up_command(
            &ds,
            &["first", "second line", "third line", "fourth line", "fifth"],
            Selection::new(1, 1, 1, 1),
            &["first", "second line", "third line", "fourth line", "fifth"],
            Selection::new(1, 1, 1, 1),
        );

        test_move_lines_down_command(
            &ds,
            &["first", "second line", "third line", "fourth line", "fifth"],
            Selection::new(5, 1, 5, 1),
            &["first", "second line", "third line", "fourth line", "fifth"],
            Selection::new(5, 1, 5, 1),
        );
    }

    #[test]
    fn move_first_line_down() {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        test_move_lines_down_command(
            &ds,
            &["first", "second line", "third line", "fourth line", "fifth"],
            Selection::new(1, 4, 1, 1),
            &["second line", "first", "third line", "fourth line", "fifth"],
            Selection::new(2, 4, 2, 1),
        );
    }

    #[test]
    fn move_2nd_line_up() {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        test_move_lines_up_command(
            &ds,
            &["first", "second line", "third line", "fourth line", "fifth"],
            Selection::new(2, 1, 2, 1),
            &["second line", "first", "third line", "fourth line", "fifth"],
            Selection::new(1, 1, 1, 1),
        );
    }

    #[test]
    fn issue_1322a_move_2nd_line_up() {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        test_move_lines_up_command(
            &ds,
            &["first", "second line", "third line", "fourth line", "fifth"],
            Selection::new(2, 12, 2, 12),
            &["second line", "first", "third line", "fourth line", "fifth"],
            Selection::new(1, 12, 1, 12),
        );
    }

    #[test]
    fn issue_1322b_move_last_line_up() {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        test_move_lines_up_command(
            &ds,
            &["first", "second line", "third line", "fourth line", "fifth"],
            Selection::new(5, 6, 5, 6),
            &["first", "second line", "third line", "fifth", "fourth line"],
            Selection::new(4, 6, 4, 6),
        );
    }

    #[test]
    fn issue_1322c_move_last_line_selected_up() {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        test_move_lines_up_command(
            &ds,
            &["first", "second line", "third line", "fourth line", "fifth"],
            Selection::new(5, 6, 5, 1),
            &["first", "second line", "third line", "fifth", "fourth line"],
            Selection::new(4, 6, 4, 1),
        );
    }

    #[test]
    fn move_last_line_up() {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        test_move_lines_up_command(
            &ds,
            &["first", "second line", "third line", "fourth line", "fifth"],
            Selection::new(5, 1, 5, 1),
            &["first", "second line", "third line", "fifth", "fourth line"],
            Selection::new(4, 1, 4, 1),
        );
    }

    #[test]
    fn move_4th_line_down() {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        test_move_lines_down_command(
            &ds,
            &["first", "second line", "third line", "fourth line", "fifth"],
            Selection::new(4, 1, 4, 1),
            &["first", "second line", "third line", "fifth", "fourth line"],
            Selection::new(5, 1, 5, 1),
        );
    }

    #[test]
    fn move_multiple_lines_down() {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        test_move_lines_down_command(
            &ds,
            &["first", "second line", "third line", "fourth line", "fifth"],
            Selection::new(4, 4, 2, 2),
            &["first", "fifth", "second line", "third line", "fourth line"],
            Selection::new(5, 4, 3, 2),
        );
    }

    #[test]
    fn invisible_selection_is_ignored() {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        test_move_lines_down_command(
            &ds,
            &["first", "second line", "third line", "fourth line", "fifth"],
            Selection::new(2, 1, 1, 1),
            &["second line", "first", "third line", "fourth line", "fifth"],
            Selection::new(3, 1, 2, 1),
        );
    }
}

mod editor_contrib_move_lines_command_honors_indentation_rules {
    use super::*;

    /// O `indentRules` da suíte.
    fn indent_rules() -> IndentationRule {
        IndentationRule {
            decrease_indent_pattern: re(
                r##"^\s*((?!\S.*\/[*]).*[*]\/\s*)?[})\]]|^\s*(case\b.*|default):\s*(\/\/.*|\/[*].*[*]\/\s*)?$"##,
                "",
            ),
            increase_indent_pattern: re(
                r##"(\{[^}"'`]*|\([^)"']*|\[[^\]"']*|^\s*(\{\}|\(\)|\[\]|(case\b.*|default):))\s*(\/\/.*|\/[*].*[*]\/\s*)?$"##,
                "",
            ),
            indent_next_line_pattern: Some(re(r##"^\s*(for|while|if|else)\b(?!.*[;{}]\s*(\/\/.*|\/[*].*[*]\/\s*)?$)"##, "")),
            un_indented_line_pattern: Some(re(
                r##"^(?!.*([;{}]|\S:)\s*(\/\/.*|\/[*].*[*]\/\s*)?$)(?!.*(\{[^}"']*|\([^)"']*|\[[^\]"']*|^\s*(\{\}|\(\)|\[\]|(case\b.*|default):))\s*(\/\/.*|\/[*].*[*]\/\s*)?$)(?!^\s*((?!\S.*\/[*]).*[*]\/\s*)?[})\]]|^\s*(case\b.*|default):\s*(\/\/.*|\/[*].*[*]\/\s*)?$)(?!^\s*(for|while|if|else)\b(?!.*[;{}]\s*(\/\/.*|\/[*].*[*]\/\s*)?$))"##,
                "",
            )),
        }
    }

    /// `IndentRulesMode`: a configuração só com as regras de indentação.
    const INDENT_RULES_LANGUAGE_ID: &str = "moveLinesIndentMode";

    fn indent_rules_mode_configuration() -> LanguageConfiguration {
        LanguageConfiguration { indentation_rules: Some(indent_rules()), ..LanguageConfiguration::default() }
    }

    // https://github.com/microsoft/vscode/issues/28552#issuecomment-307862797
    #[test]
    fn first_line_indentation_adjust_to_0() {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();

        test_move_lines_with_indent_command_on_mode(
            &ds,
            MoveLinesDirection::Up,
            INDENT_RULES_LANGUAGE_ID,
            indent_rules_mode_configuration(),
            &["class X {", "\tz = 2", "}"],
            Selection::new(2, 1, 2, 1),
            &["z = 2", "class X {", "}"],
            Selection::new(1, 1, 1, 1),
        );
    }

    // https://github.com/microsoft/vscode/issues/28552#issuecomment-307867717
    #[test]
    fn move_lines_across_block() {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();

        test_move_lines_with_indent_command_on_mode(
            &ds,
            MoveLinesDirection::Down,
            INDENT_RULES_LANGUAGE_ID,
            indent_rules_mode_configuration(),
            &["const value = 2;", "const standardLanguageDescriptions = [", "    {", "        diagnosticSource: 'js',", "    }", "];"],
            Selection::new(1, 1, 1, 1),
            &["const standardLanguageDescriptions = [", "    const value = 2;", "    {", "        diagnosticSource: 'js',", "    }", "];"],
            Selection::new(2, 5, 2, 5),
        );
    }

    #[test]
    fn move_line_should_still_work_as_before_if_there_is_no_indentation_rules() {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();

        // `testMoveLinesUpWithIndentCommand(null!, ...)`: sem linguagem e com `EditorAutoIndentStrategy.Full`.
        let language_configuration_service: Rc<dyn ILanguageConfigurationService> = ds.add_rc(TestLanguageConfigurationService::new()) as Rc<TestLanguageConfigurationService>;
        test_command(
            &["if (true) {", "    var task = new Task(() => {", "        var work = 1234;", "    });", "}"],
            None,
            Selection::new(3, 1, 3, 1),
            |sel| Box::new(MoveLinesCommand::new(sel, false, EditorAutoIndentStrategy::Full, language_configuration_service)),
            &["if (true) {", "        var work = 1234;", "    var task = new Task(() => {", "    });", "}"],
            Selection::new(2, 1, 2, 1),
            false,
        );
    }
}

mod editor_contrib_move_lines_command_honors_on_enter_rules {
    use super::*;

    /// `EnterRulesMode`.
    const ENTER_RULES_LANGUAGE_ID: &str = "moveLinesEnterMode";

    fn enter_rules_mode_configuration() -> LanguageConfiguration {
        LanguageConfiguration {
            indentation_rules: Some(IndentationRule {
                decrease_indent_pattern: re(r##"^\s*\[$"##, ""),
                increase_indent_pattern: re(r##"^\s*\]$"##, ""),
                indent_next_line_pattern: None,
                un_indented_line_pattern: None,
            }),
            brackets: Some(vec![("{".to_owned(), "}".to_owned())]),
            ..LanguageConfiguration::default()
        }
    }

    #[test]
    fn issue_54829_move_block_across_block() {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();

        test_move_lines_with_indent_command_on_mode(
            &ds,
            MoveLinesDirection::Down,
            ENTER_RULES_LANGUAGE_ID,
            enter_rules_mode_configuration(),
            &[
                "if (true) {",
                "    if (false) {",
                "        if (1) {",
                "            console.log('b');",
                "        }",
                "        console.log('a');",
                "    }",
                "}",
            ],
            Selection::new(3, 9, 5, 10),
            &[
                "if (true) {",
                "    if (false) {",
                "        console.log('a');",
                "        if (1) {",
                "            console.log('b');",
                "        }",
                "    }",
                "}",
            ],
            Selection::new(4, 9, 6, 10),
        );
    }
}
