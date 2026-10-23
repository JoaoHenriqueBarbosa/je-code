//! Porte de `src/vs/editor/common/languages/supports/characterPair.ts`: os pares de fechamento
//! automático e de envolvimento da seleção de uma linguagem.

use std::rc::Rc;

use crate::language_configuration::{
    AutoClosingPair, AutoClosingPairConditional, LanguageConfiguration,
    StandardAutoClosingPairConditional,
};

/// `CharacterPairSupport`.
///
/// Divergência: com `surroundingPairs` ausente, o original devolve os próprios pares de fechamento
/// automático (que também servem de `IAutoClosingPair`); aqui o resultado é uma lista de
/// [`AutoClosingPair`] só com `open` e `close`.
#[derive(Debug)]
pub struct CharacterPairSupport {
    auto_closing_pairs: Vec<Rc<StandardAutoClosingPairConditional>>,
    surrounding_pairs: Vec<AutoClosingPair>,
    auto_close_before_for_quotes: String,
    auto_close_before_for_brackets: String,
}

impl CharacterPairSupport {
    pub const DEFAULT_AUTOCLOSE_BEFORE_LANGUAGE_DEFINED_QUOTES: &'static str = ";:.,=}])> \n\t";
    pub const DEFAULT_AUTOCLOSE_BEFORE_LANGUAGE_DEFINED_BRACKETS: &'static str =
        "'\"`;:.,=}])> \n\t";
    pub const DEFAULT_AUTOCLOSE_BEFORE_WHITESPACE: &'static str = " \n\t";

    pub fn new(config: &LanguageConfiguration) -> Self {
        let standard = |pair: &AutoClosingPairConditional| {
            Rc::new(StandardAutoClosingPairConditional::new(pair))
        };
        let mut auto_closing_pairs: Vec<Rc<StandardAutoClosingPairConditional>> =
            if let Some(pairs) = &config.auto_closing_pairs {
                pairs.iter().map(standard).collect()
            } else if let Some(brackets) = &config.brackets {
                brackets
                    .iter()
                    .map(|(open, close)| standard(&AutoClosingPairConditional::new(open, close)))
                    .collect()
            } else {
                Vec::new()
            };

        if let Some(doc_comment) = config
            .electric_character_support
            .as_ref()
            .and_then(|e| e.doc_comment.as_ref())
        {
            // `IDocComment` é legado, só parcialmente suportado.
            auto_closing_pairs.push(standard(&AutoClosingPairConditional::new(
                &doc_comment.open,
                doc_comment.close.as_deref().unwrap_or(""),
            )));
        }

        let auto_close_before_for_quotes = config
            .auto_close_before
            .clone()
            .unwrap_or_else(|| Self::DEFAULT_AUTOCLOSE_BEFORE_LANGUAGE_DEFINED_QUOTES.to_owned());
        let auto_close_before_for_brackets = config
            .auto_close_before
            .clone()
            .unwrap_or_else(|| Self::DEFAULT_AUTOCLOSE_BEFORE_LANGUAGE_DEFINED_BRACKETS.to_owned());

        let surrounding_pairs = config.surrounding_pairs.clone().unwrap_or_else(|| {
            auto_closing_pairs
                .iter()
                .map(|pair| pair.to_pair())
                .collect()
        });

        Self {
            auto_closing_pairs,
            surrounding_pairs,
            auto_close_before_for_quotes,
            auto_close_before_for_brackets,
        }
    }

    pub fn get_auto_closing_pairs(&self) -> &[Rc<StandardAutoClosingPairConditional>] {
        &self.auto_closing_pairs
    }

    pub fn get_auto_close_before_set(&self, for_quotes: bool) -> &str {
        if for_quotes {
            &self.auto_close_before_for_quotes
        } else {
            &self.auto_close_before_for_brackets
        }
    }

    pub fn get_surrounding_pairs(&self) -> &[AutoClosingPair] {
        &self.surrounding_pairs
    }
}
