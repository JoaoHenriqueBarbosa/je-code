//! Porte de `src/vs/editor/test/browser/commands/trimTrailingWhitespaceCommand.test.ts` (suíte `Editor Commands - Trim Trailing Whitespace Command`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `createInsertDeleteSingleEditOp` e `createSingleEditOp` viram a mesma função: o `forceMoveMarkers: false`
//!   explícito de uma é o valor neutro da `SingleEditOperation` na outra.
//! - O `ITokenizationSupport` do segundo caso vira um tipo de teste; `tokenize` (que no original é
//!   `undefined!`) nunca é chamado.
//! - `ensureNoDisposablesAreLeakedInTestSuite()` vira o `LeakGuard` criado no começo de cada caso, e o
//!   `DisposableStore` da suíte é o próprio guard.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod support;

use std::rc::Rc;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::JsString;
use je_editor::edit_operation::SingleEditOperation;
use je_editor::editor_common::Command;
use je_editor::encoded_token_attributes::{metadata_consts as mc, standard_token_type};
use je_editor::language_service::ILanguageService;
use je_editor::languages::{EncodedTokenizationResult, ITokenizationSupport, State, TokenizationResult};
use je_editor::modes_registry::LanguageExtensionPoint;
use je_editor::null_tokenize::null_state;
use je_editor::position::Position;
use je_editor::range::Range;
use je_editor::selection::Selection;
use je_editor::tokenization_registry::{SupportRef, tokenization_registry};
use je_editor::trim_trailing_whitespace_command::{TrimTrailingWhitespaceCommand, trim_trailing_whitespace};
use support::test_command::get_edit_operation;
use support::test_text_model::{create_language_model_services, instantiate_text_model_with_languages, with_editor_model};

/// `createSingleEditOp` e `createInsertDeleteSingleEditOp`: `Range(selectionLine, selectionColumn,
/// positionLine, positionColumn)`, que o construtor de `Range` normaliza.
fn create_single_edit_op(
    text: Option<&str>,
    position_line_number: i32,
    position_column: i32,
    selection_line_number: i32,
    selection_column: i32,
) -> SingleEditOperation {
    SingleEditOperation::new(
        Range::new(selection_line_number, selection_column, position_line_number, position_column),
        text.map(JsString::from),
        false,
    )
}

fn assert_trim_trailing_whitespace_command(text: &[&str], expected: &[SingleEditOperation]) {
    with_editor_model(text, |model| {
        let mut op = TrimTrailingWhitespaceCommand::new(Selection::new(1, 1, 1, 1), vec![], true);
        let actual = get_edit_operation(model, &mut op);
        assert_eq!(actual, expected);
    });
}

fn assert_trim_trailing_whitespace(text: &[&str], mut cursors: Vec<Position>, expected: &[SingleEditOperation]) {
    with_editor_model(text, |model| {
        let actual = trim_trailing_whitespace(model, &mut cursors, true);
        assert_eq!(actual, expected);
    });
}

#[test]
fn remove_trailing_whitespace() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    assert_trim_trailing_whitespace_command(&[""], &[]);
    assert_trim_trailing_whitespace_command(&["text"], &[]);
    assert_trim_trailing_whitespace_command(&["text   "], &[create_single_edit_op(None, 1, 5, 1, 8)]);
    assert_trim_trailing_whitespace_command(&["text\t   "], &[create_single_edit_op(None, 1, 5, 1, 9)]);
    assert_trim_trailing_whitespace_command(&["\t   "], &[create_single_edit_op(None, 1, 1, 1, 5)]);
    assert_trim_trailing_whitespace_command(&["text\t"], &[create_single_edit_op(None, 1, 5, 1, 6)]);
    assert_trim_trailing_whitespace_command(
        &["some text\t", "some more text", "\t  ", "even more text  ", "and some mixed\t   \t"],
        &[
            create_single_edit_op(None, 1, 10, 1, 11),
            create_single_edit_op(None, 3, 1, 3, 4),
            create_single_edit_op(None, 4, 15, 4, 17),
            create_single_edit_op(None, 5, 15, 5, 20),
        ],
    );

    assert_trim_trailing_whitespace(
        &["text   "],
        vec![Position::new(1, 1), Position::new(1, 2), Position::new(1, 3)],
        &[create_single_edit_op(None, 1, 5, 1, 8)],
    );
    assert_trim_trailing_whitespace(
        &["text   "],
        vec![Position::new(1, 1), Position::new(1, 5)],
        &[create_single_edit_op(None, 1, 5, 1, 8)],
    );
    assert_trim_trailing_whitespace(
        &["text   "],
        vec![Position::new(1, 1), Position::new(1, 5), Position::new(1, 6)],
        &[create_single_edit_op(None, 1, 6, 1, 8)],
    );
    assert_trim_trailing_whitespace(
        &["some text\t", "some more text", "\t  ", "even more text  ", "and some mixed\t   \t"],
        vec![],
        &[
            create_single_edit_op(None, 1, 10, 1, 11),
            create_single_edit_op(None, 3, 1, 3, 4),
            create_single_edit_op(None, 4, 15, 4, 17),
            create_single_edit_op(None, 5, 15, 5, 20),
        ],
    );
    assert_trim_trailing_whitespace(
        &["some text\t", "some more text", "\t  ", "even more text  ", "and some mixed\t   \t"],
        vec![Position::new(1, 11), Position::new(3, 2), Position::new(5, 1), Position::new(4, 1), Position::new(5, 10)],
        &[
            create_single_edit_op(None, 3, 2, 3, 4),
            create_single_edit_op(None, 4, 15, 4, 17),
            create_single_edit_op(None, 5, 15, 5, 20),
        ],
    );
}

/// O `ITokenizationSupport` do segundo caso: três linhas conhecidas, cada uma com os tokens fixos.
struct TestTokenizationSupport {
    other_metadata: u32,
    string_metadata: u32,
}

impl ITokenizationSupport for TestTokenizationSupport {
    fn get_initial_state(&self) -> State {
        null_state()
    }

    fn tokenize(&self, _line: &JsString, _has_eol: bool, _state: &State) -> TokenizationResult {
        unreachable!("o original deixa `tokenize` indefinido e nenhum caminho o chama")
    }

    fn tokenize_encoded(&self, line: &JsString, _has_eol: bool, state: &State) -> EncodedTokenizationResult {
        let tokens = if *line == JsString::from("const a = `  ") {
            vec![0, self.other_metadata, 10, self.string_metadata]
        } else if *line == JsString::from("  a string  ") {
            vec![0, self.string_metadata]
        } else if *line == JsString::from("`;  ") {
            vec![0, self.string_metadata, 1, self.other_metadata]
        } else {
            panic!("Unexpected");
        };
        EncodedTokenizationResult { tokens, font_info: Vec::new(), end_state: state.clone() }
    }
}

#[test]
fn skips_strings_and_regex_if_configured() {
    let ds = ensure_no_disposables_are_leaked_in_test_suite();
    let services = create_language_model_services(&ds);
    let language_id = "testLanguageId";
    let language_id_codec = services.language_service.language_id_codec();
    ds.add(services.language_service.register_language(LanguageExtensionPoint::new(language_id)));
    let encoded_language_id = language_id_codec.encode_language_id(language_id);

    let other_metadata = (encoded_language_id << mc::LANGUAGEID_OFFSET)
        | (standard_token_type::OTHER << mc::TOKEN_TYPE_OFFSET)
        | mc::BALANCED_BRACKETS_MASK;
    let string_metadata = (encoded_language_id << mc::LANGUAGEID_OFFSET)
        | (standard_token_type::STRING << mc::TOKEN_TYPE_OFFSET)
        | mc::BALANCED_BRACKETS_MASK;

    let tokenization_support: SupportRef = Rc::new(TestTokenizationSupport { other_metadata, string_metadata });

    ds.add(tokenization_registry().register(language_id, tokenization_support));

    let model = instantiate_text_model_with_languages(&services, ["const a = `  ", "  a string  ", "`;  "].join("\n").as_str(), language_id);

    model.tokenization().force_tokenization(1);
    model.tokenization().force_tokenization(2);
    model.tokenization().force_tokenization(3);

    let mut op = TrimTrailingWhitespaceCommand::new(Selection::new(1, 1, 1, 1), vec![], false);
    let actual = get_edit_operation(&model, &mut op as &mut dyn Command);
    assert_eq!(actual, [create_single_edit_op(None, 3, 3, 3, 5)]);

    model.dispose();
}
