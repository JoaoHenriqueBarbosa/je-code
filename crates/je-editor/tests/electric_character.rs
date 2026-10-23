//! Porte de `src/vs/editor/test/common/modes/supports/electricCharacter.test.ts` (suíte `Editor Modes - Auto Indentation`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite` vem do `common` do `je-base`, incluído por `#[path]`.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod support;

use std::rc::Rc;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::JsString;
use je_editor::electric_character::{BracketElectricCharacterSupport, ElectricAction};
use je_editor::encoded_token_attributes::standard_token_type;
use je_editor::rich_edit_brackets::RichEditBrackets;
use support::modes_test_utils::{TokenText, create_fake_scoped_line_tokens};

const FAKE_LANGUAGE_ID: &str = "test";

fn run_on_electric_character(
    electric_character_support: &BracketElectricCharacterSupport,
    line: &[TokenText],
    character: &str,
    offset: i32,
) -> Option<ElectricAction> {
    electric_character_support.on_electric_character(
        JsString::from(character).units(),
        &create_fake_scoped_line_tokens(line),
        offset,
    )
}

fn test_does_nothing(
    electric_character_support: &BracketElectricCharacterSupport,
    line: &[TokenText],
    character: &str,
    offset: i32,
) {
    let actual = run_on_electric_character(electric_character_support, line, character, offset);
    assert_eq!(actual, None);
}

fn test_match_bracket(
    electric_character_support: &BracketElectricCharacterSupport,
    line: &[TokenText],
    character: &str,
    offset: i32,
    match_open_bracket: &str,
) {
    let actual = run_on_electric_character(electric_character_support, line, character, offset);
    assert_eq!(actual, Some(ElectricAction { match_open_bracket: JsString::from(match_open_bracket) }));
}

fn curly_and_round() -> BracketElectricCharacterSupport {
    let pairs = [("{".to_owned(), "}".to_owned()), ("(".to_owned(), ")".to_owned())];
    BracketElectricCharacterSupport::new(Some(Rc::new(RichEditBrackets::new(FAKE_LANGUAGE_ID, &pairs))))
}

#[test]
fn get_electric_characters_uses_all_sources_and_dedups() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let sup = curly_and_round();

    assert_eq!(sup.get_electric_characters(), vec![JsString::from("}"), JsString::from(")")]);
}

#[test]
fn match_open_bracket() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let sup = curly_and_round();
    let other = |text: &'static str| [TokenText { text, type_: standard_token_type::OTHER }];

    test_does_nothing(&sup, &other("\t{"), "\t", 1);
    test_does_nothing(&sup, &other("\t{"), "\t", 2);
    test_does_nothing(&sup, &other("\t\t"), "{", 3);

    test_does_nothing(&sup, &other("\t}"), "\t", 1);
    test_does_nothing(&sup, &other("\t}"), "\t", 2);
    test_match_bracket(&sup, &other("\t\t"), "}", 3, "}");
}
