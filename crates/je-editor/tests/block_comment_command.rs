//! Porte de `src/vs/editor/contrib/comment/test/browser/blockCommentCommand.test.ts` (suíte
//! `Editor Contrib - Block Comment Command`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `testCommand` recebe a seleção já ajustada e a `commandFactory` sem o `accessor` (ver
//!   `tests/support/test_command.rs`).
//! - O `prepare` de `_testCommentCommand` (registra a linguagem `commentMode` e a sua configuração nos
//!   serviços do editor de teste) vira o registro nos serviços de um modelo com linguagens de verdade
//!   (`create_language_model_services`), e o comando recebe o `ILanguageConfigurationService` desses mesmos
//!   serviços, como no original. O caminho é `test_command_on_model`.
//! - `ensureNoDisposablesAreLeakedInTestSuite()` vira o `LeakGuard` criado no começo de cada caso.
//! - As funções `testLineCommentCommand` internas dos dois últimos casos (que montam o comando com
//!   `insertSpace` falso) viram o parâmetro `insert_space` de [`test_comment_command`].

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod support;

use std::rc::Rc;

use common::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite};
use je_editor::comment::block_comment_command::BlockCommentCommand;
use je_editor::language_configuration::{CommentRule, LanguageConfiguration, LineComment};
use je_editor::language_configuration_registry::ILanguageConfigurationService;
use je_editor::language_service::ILanguageService;
use je_editor::modes_registry::LanguageExtensionPoint;
use je_editor::selection::Selection;
use support::test_command::test_command_on_model;
use support::test_text_model::{create_language_model_services, instantiate_text_model_with_languages};

/// `languageId` de `_testCommentCommand`.
const COMMENT_LANGUAGE_ID: &str = "commentMode";

/// `_testCommentCommand` com o `BlockCommentCommand` de `testBlockCommentCommand` (`insert_space` verdadeiro) e
/// o dos dois últimos casos (`insert_space` falso).
fn test_comment_command(
    ds: &LeakGuard,
    lines: &[&str],
    selection: Selection,
    insert_space: bool,
    expected_lines: &[&str],
    expected_selection: Selection,
) {
    // `prepare`
    let services = create_language_model_services(ds);
    ds.add(services.language_service.register_language(LanguageExtensionPoint::new(COMMENT_LANGUAGE_ID)));
    ds.add(services.language_configuration_service.register(
        COMMENT_LANGUAGE_ID,
        LanguageConfiguration {
            comments: Some(CommentRule {
                line_comment: Some(LineComment::Token("!@#".to_owned())),
                block_comment: Some(("<0".to_owned(), "0>".to_owned())),
            }),
            ..LanguageConfiguration::default()
        },
        None,
    ));
    let model = instantiate_text_model_with_languages(&services, lines.join("\n").as_str(), COMMENT_LANGUAGE_ID);
    let language_configuration_service: Rc<dyn ILanguageConfigurationService> = services.language_configuration_service.clone();
    test_command_on_model(
        &model,
        selection,
        move |sel| Box::new(BlockCommentCommand::new(sel, insert_space, language_configuration_service)),
        expected_lines,
        expected_selection,
        false,
    );
}

fn test_block_comment_command(ds: &LeakGuard, lines: &[&str], selection: Selection, expected_lines: &[&str], expected_selection: Selection) {
    test_comment_command(ds, lines, selection, true, expected_lines, expected_selection);
}

#[test]
fn empty_selection_wraps_itself() {
    let ds = ensure_no_disposables_are_leaked_in_test_suite();
    test_block_comment_command(
        &ds,
        &["first", "\tsecond line", "third line", "fourth line", "fifth"],
        Selection::new(1, 3, 1, 3),
        &["fi<0  0>rst", "\tsecond line", "third line", "fourth line", "fifth"],
        Selection::new(1, 6, 1, 6),
    );
}

#[test]
fn invisible_selection_ignored() {
    let ds = ensure_no_disposables_are_leaked_in_test_suite();
    test_block_comment_command(
        &ds,
        &["first", "\tsecond line", "third line", "fourth line", "fifth"],
        Selection::new(2, 1, 1, 1),
        &["<0 first", " 0>\tsecond line", "third line", "fourth line", "fifth"],
        Selection::new(1, 4, 2, 1),
    );
}

#[test]
fn bug9511() {
    let ds = ensure_no_disposables_are_leaked_in_test_suite();
    test_block_comment_command(
        &ds,
        &["first", "\tsecond line", "third line", "fourth line", "fifth"],
        Selection::new(1, 6, 1, 1),
        &["<0 first 0>", "\tsecond line", "third line", "fourth line", "fifth"],
        Selection::new(1, 4, 1, 9),
    );

    test_block_comment_command(
        &ds,
        &["<0first0>", "\tsecond line", "third line", "fourth line", "fifth"],
        Selection::new(1, 8, 1, 3),
        &["first", "\tsecond line", "third line", "fourth line", "fifth"],
        Selection::new(1, 1, 1, 6),
    );
}

#[test]
fn one_line_selection() {
    let ds = ensure_no_disposables_are_leaked_in_test_suite();
    test_block_comment_command(
        &ds,
        &["first", "\tsecond line", "third line", "fourth line", "fifth"],
        Selection::new(1, 6, 1, 3),
        &["fi<0 rst 0>", "\tsecond line", "third line", "fourth line", "fifth"],
        Selection::new(1, 6, 1, 9),
    );
}

#[test]
fn one_line_selection_toggle() {
    let ds = ensure_no_disposables_are_leaked_in_test_suite();
    test_block_comment_command(
        &ds,
        &["first", "\tsecond line", "third line", "fourth line", "fifth"],
        Selection::new(1, 6, 1, 3),
        &["fi<0 rst 0>", "\tsecond line", "third line", "fourth line", "fifth"],
        Selection::new(1, 6, 1, 9),
    );

    test_block_comment_command(
        &ds,
        &["fi<0rst0>", "\tsecond line", "third line", "fourth line", "fifth"],
        Selection::new(1, 8, 1, 5),
        &["first", "\tsecond line", "third line", "fourth line", "fifth"],
        Selection::new(1, 3, 1, 6),
    );

    test_block_comment_command(
        &ds,
        &["<0 first 0>", "\tsecond line", "third line", "fourth line", "fifth"],
        Selection::new(1, 10, 1, 1),
        &["first", "\tsecond line", "third line", "fourth line", "fifth"],
        Selection::new(1, 1, 1, 6),
    );

    test_block_comment_command(
        &ds,
        &["<0 first0>", "\tsecond line", "third line", "fourth line", "fifth"],
        Selection::new(1, 9, 1, 1),
        &["first", "\tsecond line", "third line", "fourth line", "fifth"],
        Selection::new(1, 1, 1, 6),
    );

    test_block_comment_command(
        &ds,
        &["<0first 0>", "\tsecond line", "third line", "fourth line", "fifth"],
        Selection::new(1, 9, 1, 1),
        &["first", "\tsecond line", "third line", "fourth line", "fifth"],
        Selection::new(1, 1, 1, 6),
    );

    test_block_comment_command(
        &ds,
        &["fi<0rst0>", "\tsecond line", "third line", "fourth line", "fifth"],
        Selection::new(1, 8, 1, 5),
        &["first", "\tsecond line", "third line", "fourth line", "fifth"],
        Selection::new(1, 3, 1, 6),
    );
}

#[test]
fn multi_line_selection() {
    let ds = ensure_no_disposables_are_leaked_in_test_suite();
    test_block_comment_command(
        &ds,
        &["first", "\tsecond line", "third line", "fourth line", "fifth"],
        Selection::new(2, 4, 1, 1),
        &["<0 first", "\tse 0>cond line", "third line", "fourth line", "fifth"],
        Selection::new(1, 4, 2, 4),
    );
}

#[test]
fn multi_line_selection_toggle() {
    let ds = ensure_no_disposables_are_leaked_in_test_suite();
    test_block_comment_command(
        &ds,
        &["first", "\tsecond line", "third line", "fourth line", "fifth"],
        Selection::new(2, 4, 1, 1),
        &["<0 first", "\tse 0>cond line", "third line", "fourth line", "fifth"],
        Selection::new(1, 4, 2, 4),
    );

    test_block_comment_command(
        &ds,
        &["<0first", "\tse0>cond line", "third line", "fourth line", "fifth"],
        Selection::new(2, 4, 1, 3),
        &["first", "\tsecond line", "third line", "fourth line", "fifth"],
        Selection::new(1, 1, 2, 4),
    );

    test_block_comment_command(
        &ds,
        &["<0 first", "\tse0>cond line", "third line", "fourth line", "fifth"],
        Selection::new(2, 4, 1, 3),
        &["first", "\tsecond line", "third line", "fourth line", "fifth"],
        Selection::new(1, 1, 2, 4),
    );

    test_block_comment_command(
        &ds,
        &["<0first", "\tse 0>cond line", "third line", "fourth line", "fifth"],
        Selection::new(2, 4, 1, 3),
        &["first", "\tsecond line", "third line", "fourth line", "fifth"],
        Selection::new(1, 1, 2, 4),
    );

    test_block_comment_command(
        &ds,
        &["<0 first", "\tse 0>cond line", "third line", "fourth line", "fifth"],
        Selection::new(2, 4, 1, 3),
        &["first", "\tsecond line", "third line", "fourth line", "fifth"],
        Selection::new(1, 1, 2, 4),
    );
}

#[test]
fn fuzzy_removes() {
    let ds = ensure_no_disposables_are_leaked_in_test_suite();
    for selection in [
        Selection::new(2, 5, 1, 7),
        Selection::new(2, 5, 1, 6),
        Selection::new(2, 5, 1, 5),
        Selection::new(2, 5, 1, 11),
        Selection::new(2, 1, 1, 11),
        Selection::new(2, 7, 1, 11),
    ] {
        test_block_comment_command(&ds, &["asd <0 qwe", "asd 0> qwe"], selection, &["asd qwe", "asd qwe"], Selection::new(1, 5, 2, 4));
    }
}

#[test]
fn bug_30358() {
    let ds = ensure_no_disposables_are_leaked_in_test_suite();
    test_block_comment_command(
        &ds,
        &["<0 start 0> middle end"],
        Selection::new(1, 20, 1, 23),
        &["<0 start 0> middle <0 end 0>"],
        Selection::new(1, 23, 1, 26),
    );

    test_block_comment_command(
        &ds,
        &["<0 start 0> middle <0 end 0>"],
        Selection::new(1, 13, 1, 19),
        &["<0 start 0> <0 middle 0> <0 end 0>"],
        Selection::new(1, 16, 1, 22),
    );
}

#[test]
fn issue_34618() {
    let ds = ensure_no_disposables_are_leaked_in_test_suite();
    test_block_comment_command(&ds, &["<0  0> middle end"], Selection::new(1, 4, 1, 4), &[" middle end"], Selection::new(1, 1, 1, 1));
}

#[test]
fn insert_space_false() {
    let ds = ensure_no_disposables_are_leaked_in_test_suite();
    test_comment_command(&ds, &["some text"], Selection::new(1, 1, 1, 5), false, &["<0some0> text"], Selection::new(1, 3, 1, 7));
}

#[test]
fn insert_space_false_does_not_remove_space() {
    let ds = ensure_no_disposables_are_leaked_in_test_suite();
    test_comment_command(&ds, &["<0 some 0> text"], Selection::new(1, 4, 1, 8), false, &[" some  text"], Selection::new(1, 1, 1, 7));
}
