//! Porte de `src/vs/editor/common/model/bracketPairsTextModelPart/fixBrackets.ts`: `fixBracketsInLine`, que remove
//! os fechamentos sem abertura de uma linha e fecha as aberturas sem fechamento.
//!
//! Notas de tradução:
//! - `IViewLineTokens` entra como o [`LineTokens`] concreto, que é o que o `ITokenizerSource` do porte devolve;
//! - `ILanguageConfigurationService` é um `Rc<dyn ...>`, porque o `LanguageAgnosticBracketTokens` guarda o acesso à
//!   configuração numa função `'static`;
//! - o texto é UTF-16 ([`JsString`]) e `substring` é `je_base::js_string::substring`.

use std::rc::Rc;

use je_base::JsString;
use je_base::js_string::substring;

use super::bracket_pairs_tree::ast::{AstNode, AstNodeRef};
use super::bracket_pairs_tree::brackets::LanguageAgnosticBracketTokens;
use super::bracket_pairs_tree::length::{LENGTH_ZERO, Length, length_add, length_get_column_count_if_zero_line_count};
use super::bracket_pairs_tree::parser::parse_document;
use super::bracket_pairs_tree::small_immutable_set::DenseKeyProvider;
use super::bracket_pairs_tree::tokenizer::{ITokenizerSource, TextBufferTokenizer};
use crate::language_configuration_registry::ILanguageConfigurationService;
use crate::line_tokens::{IViewLineTokens, LineTokens};

/// `fixBracketsInLine(tokens, languageConfigurationService)`.
pub fn fix_brackets_in_line(tokens: &LineTokens, language_configuration_service: &Rc<dyn ILanguageConfigurationService>) -> JsString {
    let dense_key_provider = Rc::new(DenseKeyProvider::<String>::new());
    let service = Rc::clone(language_configuration_service);
    let bracket_tokens = LanguageAgnosticBracketTokens::new(
        dense_key_provider,
        Box::new(move |language_id| service.get_language_configuration(language_id)),
    );
    let source = StaticTokenizerSource { lines: vec![tokens.clone()] };
    let mut tokenizer = TextBufferTokenizer::new(&source, &bracket_tokens);
    let node = parse_document(&mut tokenizer, &[], None, true);

    let mut result = JsString::new();
    let line = tokens.get_line_content();

    process_node(&node, LENGTH_ZERO, &line, &bracket_tokens, &mut result);

    result
}

/// `processNode(node, offset)` da função original: acumula em `result`.
fn process_node(node: &AstNodeRef, mut offset: Length, line: &JsString, bracket_tokens: &LanguageAgnosticBracketTokens, result: &mut JsString) {
    match &**node {
        AstNode::Pair(pair) => {
            process_node(&pair.opening_bracket, offset, line, bracket_tokens, result);
            offset = length_add(offset, pair.opening_bracket.length());

            if let Some(child) = &pair.child {
                process_node(child, offset, line, bracket_tokens, result);
                offset = length_add(offset, child.length());
            }
            if let Some(closing_bracket) = &pair.closing_bracket {
                process_node(closing_bracket, offset, line, bracket_tokens, result);
            } else {
                let opening = pair.opening();
                let single_lang_bracket_tokens = bracket_tokens.get_single_language_bracket_tokens(opening.language_id());

                if let Some(closing_token_text) = single_lang_bracket_tokens.find_closing_token_text(&opening.bracket_ids) {
                    result.push_units(closing_token_text.units());
                } else {
                    // `str += undefined` do original acrescenta o texto "undefined".
                    result.push_units(&"undefined".encode_utf16().collect::<Vec<u16>>());
                }
            }
        }
        AstNode::UnexpectedClosingBracket(_) => {
            // remove the bracket
        }
        AstNode::Text(_) | AstNode::Bracket(_) => {
            let start = length_get_column_count_if_zero_line_count(offset);
            let end = length_get_column_count_if_zero_line_count(length_add(offset, node.length()));
            result.push_units(substring(line.units(), start, end));
        }
        AstNode::List(_) => {
            for child in node.children() {
                process_node(&child, offset, line, bracket_tokens, result);
                offset = length_add(offset, child.length());
            }
        }
    }
}

/// `StaticTokenizerSource`: uma fonte de tokens de linhas já tokenizadas.
struct StaticTokenizerSource {
    lines: Vec<LineTokens>,
}

impl ITokenizerSource for StaticTokenizerSource {
    fn get_value(&self) -> JsString {
        let mut value = JsString::new();
        for (index, line) in self.lines.iter().enumerate() {
            if index > 0 {
                value.push_units(&[10]);
            }
            value.push_units(line.get_line_content().units());
        }
        value
    }

    fn get_line_count(&self) -> i32 {
        self.lines.len() as i32
    }

    fn get_line_length(&self, line_number: i32) -> usize {
        self.lines[(line_number - 1) as usize].get_line_content().len()
    }

    fn get_line_tokens(&self, line_number: i32) -> LineTokens {
        self.lines[(line_number - 1) as usize].clone()
    }
}
