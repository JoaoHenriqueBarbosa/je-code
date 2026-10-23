//! Porte de `.../bracketPairsTree/brackets.ts`: os tokens de colchete de cada linguagem
//! (`BracketTokens`) e o conjunto deles independente de linguagem (`LanguageAgnosticBracketTokens`).
//!
//! Divergências:
//! - `getRegExpStr` devolve a regex como [`JsString`], e a regex global (`regExpGlobal`) é criada na
//!   primeira leitura, como no original (`OnceCell`).
//! - Os fechamentos guardam os textos das aberturas que eles fecham (`ClosingBracketKind.openingBrackets`);
//!   o id de uma abertura só depende do idioma e do texto (`languageId:::bracketText`), então o texto basta.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use je_base::JsString;
use je_base::strings::escape_reg_exp_characters;

use super::ast::BracketAstNode;
use super::length::to_length;
use super::small_immutable_set::{DenseKeyProvider, IDENTITY_KEY_PROVIDER, SmallImmutableSet};
use super::tokenizer::{Token, TokenKind};
use crate::js_reg_exp::JsRegExp;
use crate::language_brackets_configuration::BracketKind;
use crate::language_configuration_registry::ResolvedLanguageConfiguration;
use std::cell::OnceCell;

/// `BracketTokens`.
pub struct BracketTokens {
    /// Do texto do colchete para o token, mais a ordem de inserção do `Map` do original.
    map: HashMap<JsString, Rc<Token>>,
    order: Vec<JsString>,
    reg_exp_global: OnceCell<Option<JsRegExp>>,
}

fn bracket_id(
    dense_key_provider: &DenseKeyProvider<String>,
    language_id: &str,
    bracket_text: &JsString,
) -> usize {
    dense_key_provider.get_key(&format!(
        "{}:::{}",
        language_id,
        bracket_text.to_string_lossy()
    ))
}

impl BracketTokens {
    /// `BracketTokens.createFromLanguage(configuration, denseKeyProvider)`.
    pub fn create_from_language(
        configuration: &ResolvedLanguageConfiguration,
        dense_key_provider: &DenseKeyProvider<String>,
    ) -> Self {
        let mut map: HashMap<JsString, Rc<Token>> = HashMap::new();
        let mut order: Vec<JsString> = Vec::new();
        let language_id = &configuration.language_id;

        for opening_bracket in configuration.brackets_new.opening_brackets() {
            let length = to_length(0, opening_bracket.bracket_text.len() as i64);

            let opening_text_id = bracket_id(
                dense_key_provider,
                language_id,
                &opening_bracket.bracket_text,
            );
            let bracket_ids =
                SmallImmutableSet::get_empty().add(&opening_text_id, &IDENTITY_KEY_PROVIDER);

            let info = Rc::new(BracketKind::Opening(opening_bracket.clone()));
            if map
                .insert(
                    opening_bracket.bracket_text.clone(),
                    Rc::new(Token::new(
                        length,
                        TokenKind::OpeningBracket,
                        opening_text_id as i64,
                        bracket_ids.clone(),
                        Some(BracketAstNode::create(length, info, bracket_ids)),
                    )),
                )
                .is_none()
            {
                order.push(opening_bracket.bracket_text.clone());
            }
        }

        for closing_bracket in configuration.brackets_new.closing_brackets() {
            let length = to_length(0, closing_bracket.bracket_text.len() as i64);

            let mut bracket_ids = SmallImmutableSet::get_empty();
            for opening_text in &closing_bracket.opening_brackets {
                let id = bracket_id(dense_key_provider, language_id, opening_text);
                bracket_ids = bracket_ids.add(&id, &IDENTITY_KEY_PROVIDER);
            }

            let first_id = bracket_id(
                dense_key_provider,
                language_id,
                &closing_bracket.opening_brackets[0],
            );
            let info = Rc::new(BracketKind::Closing(closing_bracket.clone()));
            if map
                .insert(
                    closing_bracket.bracket_text.clone(),
                    Rc::new(Token::new(
                        length,
                        TokenKind::ClosingBracket,
                        first_id as i64,
                        bracket_ids.clone(),
                        Some(BracketAstNode::create(length, info, bracket_ids)),
                    )),
                )
                .is_none()
            {
                order.push(closing_bracket.bracket_text.clone());
            }
        }

        Self {
            map,
            order,
            reg_exp_global: OnceCell::new(),
        }
    }

    /// `getRegExpStr()`: `None` se não há colchetes.
    pub fn get_reg_exp_str(&self) -> Option<JsString> {
        if self.is_empty() {
            None
        } else {
            let mut keys: Vec<&JsString> = self.order.iter().collect();
            keys.sort();
            keys.reverse();
            let mut source = JsString::new();
            for (i, key) in keys.iter().enumerate() {
                if i > 0 {
                    source.push_unit(u16::from(b'|'));
                }
                source.push_units(prepare_bracket_for_reg_exp(key).units());
            }
            Some(source)
        }
    }

    /// `regExpGlobal`: `None` se não há regex (porque não há colchetes).
    pub fn reg_exp_global(&self) -> Option<&JsRegExp> {
        self.reg_exp_global
            .get_or_init(|| {
                self.get_reg_exp_str()
                    .and_then(|source| JsRegExp::new(source.units(), "gi").ok())
            })
            .as_ref()
    }

    /// `getToken(value)`: a busca é feita em minúsculas.
    pub fn get_token(&self, value: &[u16]) -> Option<Rc<Token>> {
        let lowered = JsString::from(value).to_lower_case();
        self.map.get(&lowered).cloned()
    }

    /// `findClosingTokenText(openingBracketIds)`.
    pub fn find_closing_token_text(
        &self,
        opening_bracket_ids: &SmallImmutableSet,
    ) -> Option<JsString> {
        for closing_text in &self.order {
            let info = &self.map[closing_text];
            if info.kind == TokenKind::ClosingBracket
                && info.bracket_ids.intersects(opening_bracket_ids)
            {
                return Some(closing_text.clone());
            }
        }
        None
    }

    /// `isEmpty`.
    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }
}

fn is_word_or_space(unit: u16) -> bool {
    matches!(unit, 0x30..=0x39 | 0x41..=0x5a | 0x61..=0x7a | 0x5f | 0x20)
}

fn prepare_bracket_for_reg_exp(str: &JsString) -> JsString {
    let mut escaped = escape_reg_exp_characters(str.units());
    // Estes delimitadores de par de colchetes começam ou terminam com letras
    // veja https://github.com/microsoft/vscode/issues/132162 https://github.com/microsoft/vscode/issues/150440
    if str.units().first().is_some_and(|&u| is_word_or_space(u)) {
        let mut with_boundary = JsString::from("\\b");
        with_boundary.push_units(escaped.units());
        escaped = with_boundary;
    }
    if str.units().last().is_some_and(|&u| is_word_or_space(u)) {
        escaped.push_units(&"\\b".encode_utf16().collect::<Vec<u16>>());
    }
    escaped
}

/// A função que devolve a configuração de uma linguagem.
pub type LanguageConfigurationGetter = Box<dyn Fn(&str) -> Rc<ResolvedLanguageConfiguration>>;

/// `LanguageAgnosticBracketTokens`.
pub struct LanguageAgnosticBracketTokens {
    language_id_to_bracket_tokens: RefCell<HashMap<String, Rc<BracketTokens>>>,
    dense_key_provider: Rc<DenseKeyProvider<String>>,
    get_language_configuration: LanguageConfigurationGetter,
}

impl LanguageAgnosticBracketTokens {
    pub fn new(
        dense_key_provider: Rc<DenseKeyProvider<String>>,
        get_language_configuration: LanguageConfigurationGetter,
    ) -> Self {
        Self {
            language_id_to_bracket_tokens: RefCell::new(HashMap::new()),
            dense_key_provider,
            get_language_configuration,
        }
    }

    /// `didLanguageChange(languageId)`: informa uma mudança sempre que a configuração da linguagem é atualizada.
    pub fn did_language_change(&self, language_id: &str) -> bool {
        self.language_id_to_bracket_tokens
            .borrow()
            .contains_key(language_id)
    }

    /// `getSingleLanguageBracketTokens(languageId)`.
    pub fn get_single_language_bracket_tokens(&self, language_id: &str) -> Rc<BracketTokens> {
        if let Some(existing) = self.language_id_to_bracket_tokens.borrow().get(language_id) {
            return existing.clone();
        }
        let created = Rc::new(BracketTokens::create_from_language(
            &(self.get_language_configuration)(language_id),
            &self.dense_key_provider,
        ));
        self.language_id_to_bracket_tokens
            .borrow_mut()
            .insert(language_id.to_owned(), created.clone());
        created
    }

    /// `getToken(value, languageId)`.
    pub fn get_token(&self, value: &[u16], language_id: &str) -> Option<Rc<Token>> {
        self.get_single_language_bracket_tokens(language_id)
            .get_token(value)
    }
}
