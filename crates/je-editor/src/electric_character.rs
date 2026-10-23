//! Porte de `src/vs/editor/common/languages/supports/electricCharacter.ts`: os caracteres elétricos
//! dos colchetes (ao digitar o fechamento, a linha se realinha com a abertura).

use std::rc::Rc;

use je_base::JsString;
use je_base::arrays::distinct;
use je_base::js_string::{is_js_whitespace, substring};

use crate::rich_edit_brackets::{BracketsUtils, RichEditBrackets};
use crate::supports::{ScopedLineTokens, ignore_brackets_in_token};

/// `IElectricAction`: a linha fica na indentação da linha que tem a abertura do colchete dado.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ElectricAction {
    pub match_open_bracket: JsString,
}

/// `BracketElectricCharacterSupport`.
#[derive(Debug)]
pub struct BracketElectricCharacterSupport {
    rich_edit_brackets: Option<Rc<RichEditBrackets>>,
}

impl BracketElectricCharacterSupport {
    pub fn new(rich_edit_brackets: Option<Rc<RichEditBrackets>>) -> Self {
        Self { rich_edit_brackets }
    }

    pub fn get_electric_characters(&self) -> Vec<JsString> {
        let mut result = Vec::new();

        if let Some(rich_edit_brackets) = &self.rich_edit_brackets {
            for bracket in &rich_edit_brackets.brackets {
                for close in &bracket.close {
                    // O último caractere (`charAt(length - 1)`; vazio se o texto é vazio).
                    let last_char = close
                        .units()
                        .last()
                        .map_or_else(JsString::new, |u| JsString::from_units(vec![*u]));
                    result.push(last_char);
                }
            }
        }

        distinct(&result, JsString::clone)
    }

    pub fn on_electric_character(
        &self,
        character: &[u16],
        context: &ScopedLineTokens,
        column: i32,
    ) -> Option<ElectricAction> {
        let rich_edit_brackets = self
            .rich_edit_brackets
            .as_ref()
            .filter(|b| !b.brackets.is_empty())?;

        let token_index = context.find_token_index_at_offset(column - 1);
        if ignore_brackets_in_token(context.get_standard_token_type(token_index)) {
            return None;
        }

        let reversed_bracket_regex = &rich_edit_brackets.reversed_regex;
        let line_content = context.get_line_content();
        let mut text: Vec<u16> = substring(line_content.units(), 0, i64::from(column - 1)).to_vec();
        text.extend_from_slice(character);

        let r = BracketsUtils::find_prev_bracket_in_range(
            reversed_bracket_regex,
            1,
            &text,
            0,
            text.len() as i32,
        )?;

        let bracket_text = JsString::from(substring(
            &text,
            i64::from(r.start_column - 1),
            i64::from(r.end_column - 1),
        ))
        .to_lower_case();

        let is_open = rich_edit_brackets
            .text_is_open_bracket
            .get(&bracket_text)
            .copied()
            .unwrap_or(false);
        if is_open {
            return None;
        }

        let text_before_bracket = context.get_actual_line_content_before(r.start_column - 1);
        if !text_before_bracket
            .units()
            .iter()
            .all(|&u| is_js_whitespace(u))
        {
            // Há outro texto na linha antes do colchete.
            return None;
        }

        Some(ElectricAction {
            match_open_bracket: bracket_text,
        })
    }
}
