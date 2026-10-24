//! Porte de `src/vs/editor/contrib/bracketMatching/test/browser/bracketMatching.test.ts` (suíte `bracket matching`), caso
//! a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()`, o `setup` e o `teardown` da suíte são [`run_test`] (o `LeakGuard`, os
//!   serviços do editor de teste e o descarte, nessa ordem); cada caso roda dentro dele.
//! - `createTextModelWithBrackets` registra a linguagem `bracketMode` no `ILanguageService` e a configuração com os
//!   colchetes `{}`, `[]` e `()` no `ILanguageConfigurationService`; o modelo é o `instantiateTextModel(...)` do original
//!   com esses serviços de linguagem (`TextModel::new_with_language_service`) e é descartado junto com a suíte.
//! - `instantiateTestCodeEditor(instantiationService, model)` é `instantiate_test_code_editor` de
//!   `tests/support/test_code_editor.rs`, com as opções `{}` (o `Default` das opções de instanciação).
//! - `editor.registerAndInstantiateContribution(BracketMatchingController.ID, BracketMatchingController)` é
//!   `register_and_instantiate_contribution` com o construtor `BracketMatchingController::new`.
//! - `assert.deepStrictEqual(editor.getPosition(), new Position(...))` e o equivalente para `Selection` e
//!   `getSelections()` são `assert_eq!` sobre o valor (o `Option` que o `getPosition()` do porte devolve é comparado
//!   com o `Some` esperado).
//! - `editor.getModel().setValue(text)` é `model.set_value(text.into())`, e `getValue()` o `get_value()` convertido em
//!   texto.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod support;

use std::rc::Rc;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::lifecycle::{DisposableStore, IDisposable, to_disposable};
use je_editor::bracket_matching::BracketMatchingController;
use je_editor::code_editor_widget::ICodeEditor;
use je_editor::language_configuration::LanguageConfiguration;
use je_editor::language_configuration_registry::ILanguageConfigurationService as _;
use je_editor::language_service::{ILanguageService, I_LANGUAGE_SERVICE};
use je_editor::model::RelaxedTextModelCreationOptions;
use je_editor::modes_registry::LanguageExtensionPoint;
use je_editor::position::Position;
use je_editor::selection::Selection;
use je_editor::text_model::{TextModel, TextModelServices};
use support::test_code_editor::{
    CodeEditorServices, TestCodeEditor, TestCodeEditorInstantiationOptions, create_code_editor_services,
    instantiate_test_code_editor,
};
use support::test_language_configuration_service::TestLanguageConfigurationService;
use support::test_text_model::resolve_options;

/// Os `let` da suíte `bracket matching`.
struct Suite {
    disposables: Rc<DisposableStore>,
    instantiation_service: Rc<CodeEditorServices>,
    language_configuration_service: Rc<TestLanguageConfigurationService>,
    language_service: Rc<dyn ILanguageService>,
}

/// O que o mocha faz em volta de cada `test(...)` da suíte: o `setup`, o `ensureNoDisposablesAreLeakedInTestSuite()`, o
/// corpo e o `teardown`, nessa ordem.
fn run_test(body: impl FnOnce(&Suite)) {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let disposables = DisposableStore::new();
    let instantiation_service = create_code_editor_services(&disposables, None);
    let language_configuration_service = instantiation_service.language_configuration_service.clone();
    let language_service = instantiation_service.get(I_LANGUAGE_SERVICE);
    let suite = Suite { disposables, instantiation_service, language_configuration_service, language_service };
    body(&suite);
    suite.disposables.dispose();
}

fn create_text_model_with_brackets(suite: &Suite, text: &str) -> Rc<TextModel> {
    let language_id = "bracketMode";
    suite.disposables.add(suite.language_service.register_language(LanguageExtensionPoint::new(language_id)));
    suite.disposables.add(suite.language_configuration_service.register(
        language_id,
        LanguageConfiguration {
            brackets: Some(vec![
                ("{".to_owned(), "}".to_owned()),
                ("[".to_owned(), "]".to_owned()),
                ("(".to_owned(), ")".to_owned()),
            ]),
            ..LanguageConfiguration::default()
        },
        None,
    ));

    let model = TextModel::new_with_language_service(
        text.into(),
        language_id,
        resolve_options(&RelaxedTextModelCreationOptions::default()),
        None,
        suite.instantiation_service.undo_redo_service.clone(),
        Some(TextModelServices {
            language_service: suite.language_service.clone(),
            language_configuration_service: suite.language_configuration_service.clone(),
        }),
    );
    let owned = model.clone();
    suite.disposables.add(to_disposable(move || owned.dispose()));
    model
}

fn create_code_editor_with_brackets(suite: &Suite, text: &str) -> Rc<TestCodeEditor> {
    let model = create_text_model_with_brackets(suite, text);
    let options = TestCodeEditorInstantiationOptions::default().options;
    suite.disposables.add_rc(instantiate_test_code_editor(&suite.instantiation_service, Some(model), &options))
}

/// `disposables.add(editor.registerAndInstantiateContribution(BracketMatchingController.ID, BracketMatchingController))`.
fn create_bracket_matching_controller(suite: &Suite, editor: &TestCodeEditor) -> Rc<BracketMatchingController> {
    suite.disposables.add_rc(
        editor.register_and_instantiate_contribution(BracketMatchingController::ID, |editor, _| {
            BracketMatchingController::new(editor)
        }),
    )
}

/// `assert.deepStrictEqual(editor.getPosition(), new Position(line, column))`.
fn assert_position(editor: &TestCodeEditor, line_number: i32, column: i32) {
    assert_eq!(editor.get_position(), Some(Position::new(line_number, column)));
}

/// `assert.deepStrictEqual(editor.getSelection(), new Selection(...))`.
fn assert_selection(editor: &TestCodeEditor, expected: Selection) {
    assert_eq!(editor.get_selection(), Some(expected));
}

/// `editor.getModel().getValue()`.
fn model_value(editor: &TestCodeEditor) -> String {
    editor.get_model().expect("o editor tem o modelo").get_value().to_string_lossy()
}

#[test]
fn issue_183_jump_to_matching_bracket_position() {
    run_test(|suite| {
        let editor = create_code_editor_with_brackets(suite, "var x = (3 + (5-7)) + ((5+3)+5);");
        let bracket_matching_controller = create_bracket_matching_controller(suite, &editor);

        // start on closing bracket
        editor.set_position(Position::new(1, 20), None);
        bracket_matching_controller.jump_to_bracket();
        assert_position(&editor, 1, 9);
        bracket_matching_controller.jump_to_bracket();
        assert_position(&editor, 1, 19);
        bracket_matching_controller.jump_to_bracket();
        assert_position(&editor, 1, 9);

        // start on opening bracket
        editor.set_position(Position::new(1, 23), None);
        bracket_matching_controller.jump_to_bracket();
        assert_position(&editor, 1, 31);
        bracket_matching_controller.jump_to_bracket();
        assert_position(&editor, 1, 23);
        bracket_matching_controller.jump_to_bracket();
        assert_position(&editor, 1, 31);
    });
}

#[test]
fn jump_to_next_bracket() {
    run_test(|suite| {
        let editor = create_code_editor_with_brackets(suite, "var x = (3 + (5-7)); y();");
        let bracket_matching_controller = create_bracket_matching_controller(suite, &editor);

        // start position between brackets
        editor.set_position(Position::new(1, 16), None);
        bracket_matching_controller.jump_to_bracket();
        assert_position(&editor, 1, 18);
        bracket_matching_controller.jump_to_bracket();
        assert_position(&editor, 1, 14);
        bracket_matching_controller.jump_to_bracket();
        assert_position(&editor, 1, 18);

        // skip brackets in comments
        editor.set_position(Position::new(1, 21), None);
        bracket_matching_controller.jump_to_bracket();
        assert_position(&editor, 1, 23);
        bracket_matching_controller.jump_to_bracket();
        assert_position(&editor, 1, 24);
        bracket_matching_controller.jump_to_bracket();
        assert_position(&editor, 1, 23);

        // do not break if no brackets are available
        editor.set_position(Position::new(1, 26), None);
        bracket_matching_controller.jump_to_bracket();
        assert_position(&editor, 1, 26);
    });
}

#[test]
fn select_to_next_bracket() {
    run_test(|suite| {
        let editor = create_code_editor_with_brackets(suite, "var x = (3 + (5-7)); y();");
        let bracket_matching_controller = create_bracket_matching_controller(suite, &editor);

        // start position in open brackets
        editor.set_position(Position::new(1, 9), None);
        bracket_matching_controller.select_to_bracket(true);
        assert_position(&editor, 1, 20);
        assert_selection(&editor, Selection::new(1, 9, 1, 20));

        // start position in close brackets (should select backwards)
        editor.set_position(Position::new(1, 20), None);
        bracket_matching_controller.select_to_bracket(true);
        assert_position(&editor, 1, 9);
        assert_selection(&editor, Selection::new(1, 20, 1, 9));

        // start position between brackets
        editor.set_position(Position::new(1, 16), None);
        bracket_matching_controller.select_to_bracket(true);
        assert_position(&editor, 1, 19);
        assert_selection(&editor, Selection::new(1, 14, 1, 19));

        // start position outside brackets
        editor.set_position(Position::new(1, 21), None);
        bracket_matching_controller.select_to_bracket(true);
        assert_position(&editor, 1, 25);
        assert_selection(&editor, Selection::new(1, 23, 1, 25));

        // do not break if no brackets are available
        editor.set_position(Position::new(1, 26), None);
        bracket_matching_controller.select_to_bracket(true);
        assert_position(&editor, 1, 26);
        assert_selection(&editor, Selection::new(1, 26, 1, 26));
    });
}

#[test]
fn issue_1772_jump_to_enclosing_brackets() {
    run_test(|suite| {
        let text = [
            "const x = {",
            "    something: [0, 1, 2],",
            "    another: true,",
            "    somethingmore: [0, 2, 4]",
            "};",
        ]
        .join("\n");
        let editor = create_code_editor_with_brackets(suite, &text);
        let bracket_matching_controller = create_bracket_matching_controller(suite, &editor);

        editor.set_position(Position::new(3, 5), None);
        bracket_matching_controller.jump_to_bracket();
        assert_selection(&editor, Selection::new(5, 1, 5, 1));
    });
}

#[test]
fn issue_43371_argument_to_not_select_brackets() {
    run_test(|suite| {
        let text = [
            "const x = {",
            "    something: [0, 1, 2],",
            "    another: true,",
            "    somethingmore: [0, 2, 4]",
            "};",
        ]
        .join("\n");
        let editor = create_code_editor_with_brackets(suite, &text);
        let bracket_matching_controller = create_bracket_matching_controller(suite, &editor);

        editor.set_position(Position::new(3, 5), None);
        bracket_matching_controller.select_to_bracket(false);
        assert_selection(&editor, Selection::new(1, 12, 5, 1));
    });
}

#[test]
fn issue_45369_select_to_bracket_with_multicursor() {
    run_test(|suite| {
        let editor = create_code_editor_with_brackets(suite, "{  }   {   }   { }");
        let bracket_matching_controller = create_bracket_matching_controller(suite, &editor);

        // cursors inside brackets become selections of the entire bracket contents
        editor.set_selections(&[Selection::new(1, 3, 1, 3), Selection::new(1, 10, 1, 10), Selection::new(1, 17, 1, 17)], None, None);
        bracket_matching_controller.select_to_bracket(true);
        assert_eq!(
            editor.get_selections(),
            Some(vec![Selection::new(1, 1, 1, 5), Selection::new(1, 8, 1, 13), Selection::new(1, 16, 1, 19)])
        );

        // cursors to the left of bracket pairs become selections of the entire pair
        editor.set_selections(&[Selection::new(1, 1, 1, 1), Selection::new(1, 6, 1, 6), Selection::new(1, 14, 1, 14)], None, None);
        bracket_matching_controller.select_to_bracket(true);
        assert_eq!(
            editor.get_selections(),
            Some(vec![Selection::new(1, 1, 1, 5), Selection::new(1, 8, 1, 13), Selection::new(1, 16, 1, 19)])
        );

        // cursors just right of a bracket pair become selections of the entire pair
        editor.set_selections(&[Selection::new(1, 5, 1, 5), Selection::new(1, 13, 1, 13), Selection::new(1, 19, 1, 19)], None, None);
        bracket_matching_controller.select_to_bracket(true);
        assert_eq!(
            editor.get_selections(),
            Some(vec![Selection::new(1, 5, 1, 1), Selection::new(1, 13, 1, 8), Selection::new(1, 19, 1, 16)])
        );
    });
}

#[test]
fn removes_brackets() {
    run_test(|suite| {
        let editor = create_code_editor_with_brackets(suite, "var x = (3 + (5-7)); y();");
        let bracket_matching_controller = create_bracket_matching_controller(suite, &editor);
        let remove_brackets = || bracket_matching_controller.remove_brackets(None);

        // position before the bracket
        editor.set_position(Position::new(1, 9), None);
        remove_brackets();
        assert_eq!(model_value(&editor), "var x = 3 + (5-7); y();");
        editor.get_model().expect("o editor tem o modelo").set_value("var x = (3 + (5-7)); y();".into());

        // position between brackets
        editor.set_position(Position::new(1, 16), None);
        remove_brackets();
        assert_eq!(model_value(&editor), "var x = (3 + 5-7); y();");
        remove_brackets();
        assert_eq!(model_value(&editor), "var x = 3 + 5-7; y();");
        remove_brackets();
        assert_eq!(model_value(&editor), "var x = 3 + 5-7; y();");
    });
}
