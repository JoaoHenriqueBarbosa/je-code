//! Porte de `src/vs/editor/common/languages/supports/languageBracketsConfiguration.ts`: os colchetes de
//! uma linguagem (abertura e fechamento por texto, com a relação entre eles e os coloridos).
//!
//! Decisões de tradução:
//! - No original os tipos se referenciam entre si por objeto (ciclo `Opening` <-> `Closing`). Aqui a
//!   relação guarda o texto do colchete do outro lado, que é a chave única da configuração
//!   ("não há dois colchetes com o mesmo texto"), e a identidade da configuração é um número
//!   (`config_id`), que o `closes` compara no lugar de `other['config'] !== this.config`.
//! - O `CachedFunction` mantém a ordem de inserção, que `openingBrackets`/`closingBrackets` expõem; as
//!   listas daqui preservam essa ordem.

use std::cell::Cell;
use std::collections::HashSet;

use je_base::JsString;

use crate::js_reg_exp::JsRegExp;
use crate::language_configuration::{CharacterPair, LanguageConfiguration};
use crate::rich_edit_brackets::{RegExpOptions, create_bracket_or_reg_exp};

thread_local! {
    static NEXT_CONFIG_ID: Cell<usize> = const { Cell::new(0) };
}

/// `OpeningBracketKind`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OpeningBracketKind {
    pub language_id: String,
    config_id: usize,
    pub bracket_text: JsString,
    /// Os textos dos fechamentos que este colchete abre (`openedBrackets`).
    pub opened_brackets: Vec<JsString>,
}

impl OpeningBracketKind {
    pub const IS_OPENING_BRACKET: bool = true;
}

/// `ClosingBracketKind`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClosingBracketKind {
    pub language_id: String,
    config_id: usize,
    pub bracket_text: JsString,
    /// Os textos das aberturas que este colchete fecha (`openingBrackets`), nunca vazio.
    pub opening_brackets: Vec<JsString>,
    opening_colorized_brackets: HashSet<JsString>,
}

impl ClosingBracketKind {
    pub const IS_OPENING_BRACKET: bool = false;

    /// Este colchete fecha `other`? Se vêm de configurações diferentes, é falso.
    pub fn closes(&self, other: &OpeningBracketKind) -> bool {
        other.config_id == self.config_id && self.opening_brackets.contains(&other.bracket_text)
    }

    pub fn closes_colorized(&self, other: &OpeningBracketKind) -> bool {
        other.config_id == self.config_id
            && self
                .opening_colorized_brackets
                .contains(&other.bracket_text)
    }
}

/// `BracketKind`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BracketKind {
    Opening(OpeningBracketKind),
    Closing(ClosingBracketKind),
}

impl BracketKind {
    /// `bracketText`.
    pub fn bracket_text(&self) -> &JsString {
        match self {
            BracketKind::Opening(b) => &b.bracket_text,
            BracketKind::Closing(b) => &b.bracket_text,
        }
    }

    /// `languageId`.
    pub fn language_id(&self) -> &str {
        match self {
            BracketKind::Opening(b) => &b.language_id,
            BracketKind::Closing(b) => &b.language_id,
        }
    }
}

fn filter_valid_brackets(bracket_pairs: &[CharacterPair]) -> Vec<CharacterPair> {
    bracket_pairs
        .iter()
        .filter(|(open, close)| !open.is_empty() && !close.is_empty())
        .cloned()
        .collect()
}

/// Um colchete e os textos do outro lado, na ordem de inserção (a entrada do `CachedFunction`).
struct Entry {
    text: JsString,
    others: Vec<JsString>,
    colorized: Vec<JsString>,
}

fn entry_of<'a>(entries: &'a mut Vec<Entry>, text: &str) -> &'a mut Entry {
    let text = JsString::from(text);
    let index = match entries.iter().position(|e| e.text == text) {
        Some(index) => index,
        None => {
            entries.push(Entry {
                text,
                others: Vec::new(),
                colorized: Vec::new(),
            });
            entries.len() - 1
        }
    };
    &mut entries[index]
}

fn push_unique(list: &mut Vec<JsString>, value: JsString) {
    if !list.contains(&value) {
        list.push(value);
    }
}

/// `LanguageBracketsConfiguration`: os colchetes de uma linguagem. Imutável.
#[derive(Debug)]
pub struct LanguageBracketsConfiguration {
    pub language_id: String,
    opening_brackets: Vec<OpeningBracketKind>,
    closing_brackets: Vec<ClosingBracketKind>,
}

impl LanguageBracketsConfiguration {
    pub fn new(language_id: &str, config: &LanguageConfiguration) -> Self {
        let config_id = NEXT_CONFIG_ID.with(|c| {
            c.set(c.get() + 1);
            c.get()
        });
        let bracket_pairs = config
            .brackets
            .as_deref()
            .map(filter_valid_brackets)
            .unwrap_or_default();

        // Para cada abertura, os fechamentos; para cada fechamento, as aberturas e as coloridas.
        let mut openings: Vec<Entry> = Vec::new();
        let mut closings: Vec<Entry> = Vec::new();

        for (open, close) in &bracket_pairs {
            push_unique(
                &mut entry_of(&mut openings, open).others,
                JsString::from(close.as_str()),
            );
            push_unique(
                &mut entry_of(&mut closings, close).others,
                JsString::from(open.as_str()),
            );
        }

        // Os coloridos também são colchetes, e ficam marcados.
        let colorized_bracket_pairs: Vec<CharacterPair> = match &config.colorized_bracket_pairs {
            Some(pairs) => filter_valid_brackets(pairs),
            // Sem configuração, todos menos `<` ... `>`: muitas linguagens o declaram, mas também o usam
            // como comparação, o que atrapalha a colorização (issue #132476 do VS Code).
            None => bracket_pairs
                .iter()
                .filter(|(open, close)| !(open == "<" && close == ">"))
                .cloned()
                .collect(),
        };
        for (open, close) in &colorized_bracket_pairs {
            push_unique(
                &mut entry_of(&mut openings, open).others,
                JsString::from(close.as_str()),
            );
            let closing = entry_of(&mut closings, close);
            push_unique(&mut closing.colorized, JsString::from(open.as_str()));
            push_unique(&mut closing.others, JsString::from(open.as_str()));
        }

        Self {
            language_id: language_id.to_owned(),
            opening_brackets: openings
                .into_iter()
                .map(|e| OpeningBracketKind {
                    language_id: language_id.to_owned(),
                    config_id,
                    bracket_text: e.text,
                    opened_brackets: e.others,
                })
                .collect(),
            closing_brackets: closings
                .into_iter()
                .map(|e| ClosingBracketKind {
                    language_id: language_id.to_owned(),
                    config_id,
                    bracket_text: e.text,
                    opening_brackets: e.others,
                    opening_colorized_brackets: e.colorized.into_iter().collect(),
                })
                .collect(),
        }
    }

    /// Não há dois colchetes com o mesmo texto.
    pub fn opening_brackets(&self) -> &[OpeningBracketKind] {
        &self.opening_brackets
    }

    /// Não há dois colchetes com o mesmo texto.
    pub fn closing_brackets(&self) -> &[ClosingBracketKind] {
        &self.closing_brackets
    }

    pub fn get_opening_bracket_info(&self, bracket_text: &[u16]) -> Option<&OpeningBracketKind> {
        self.opening_brackets
            .iter()
            .find(|b| b.bracket_text.units() == bracket_text)
    }

    pub fn get_closing_bracket_info(&self, bracket_text: &[u16]) -> Option<&ClosingBracketKind> {
        self.closing_brackets
            .iter()
            .find(|b| b.bracket_text.units() == bracket_text)
    }

    pub fn get_bracket_info(&self, bracket_text: &[u16]) -> Option<BracketKind> {
        self.get_opening_bracket_info(bracket_text)
            .map(|b| BracketKind::Opening(b.clone()))
            .or_else(|| {
                self.get_closing_bracket_info(bracket_text)
                    .map(|b| BracketKind::Closing(b.clone()))
            })
    }

    pub fn get_bracket_reg_exp(&self, options: Option<&RegExpOptions>) -> JsRegExp {
        let brackets: Vec<JsString> = self
            .opening_brackets
            .iter()
            .map(|b| b.bracket_text.clone())
            .chain(self.closing_brackets.iter().map(|b| b.bracket_text.clone()))
            .collect();
        create_bracket_or_reg_exp(&brackets, options)
    }
}
