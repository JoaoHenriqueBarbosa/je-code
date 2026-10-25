//! Porte de `src/vs/editor/standalone/common/monarch/monarchTypes.ts`: a descrição declarativa de uma
//! linguagem Monarch (`IMonarchLanguage`), que o `compile` transforma no `ILexer`.
//!
//! Decisões de tradução:
//! - O original recebe um objeto JSON solto (`IMonarchLanguage` com `[key: string]: any`). Aqui cada forma que o
//!   `compile` distingue em tempo de execução é uma variante: regra em `include`, em lista `[regex, ação, próximo]`
//!   ou em objeto `{ regex, action }`; ação em texto, objeto com `token`, lista (grupo) ou `cases`.
//! - Uma `RegExp` e um texto de regex são o mesmo [`String`] (o `compile` só lê o `source` de ambas).
//! - Os atributos livres da linguagem (`keywords`, `encoding` e afins, lidos por `@atributo`) são
//!   [`MonarchAttr`].
//! - Os pares de `cases` e as chaves de `tokenizer` ficam em `Vec`, que guarda a ordem de inserção como o objeto.

/// O valor de um atributo livre da linguagem (`lexer[attr]`).
#[derive(Clone, Debug, PartialEq)]
pub enum MonarchAttr {
    /// Um texto, que `@atributo` expande.
    Str(String),
    /// Uma `RegExp`, que `@atributo` expande pelo `source`.
    Re(String),
    /// Um booleano.
    Bool(bool),
    /// Uma lista só de textos (o alvo de `@lista` em um `cases`).
    Strs(Vec<String>),
    /// Qualquer outro valor verdadeiro (objeto, número, lista de outra coisa).
    Other,
}

/// `IMonarchLanguageAction`: o objeto de ação com `token` e os modificadores de estado.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MonarchTokenAction {
    pub token: Option<String>,
    pub next: Option<String>,
    pub switch_to: Option<String>,
    pub go_back: Option<i32>,
    pub bracket: Option<String>,
    pub next_embedded: Option<String>,
    pub log: Option<String>,
}

impl MonarchTokenAction {
    pub fn token(token: &str) -> Self {
        Self { token: Some(token.to_owned()), ..Self::default() }
    }
}

/// `IMonarchLanguageAction | IShortMonarchLanguageAction | (...)[]`: a ação de uma regra.
#[derive(Clone, Debug, PartialEq)]
pub enum MonarchAction {
    /// Ausente (`undefined`, `null`): vira `{ token: '' }`.
    None,
    /// O texto curto, que é o próprio `token`.
    Str(String),
    /// O objeto com `token`.
    Token(MonarchTokenAction),
    /// A lista de ações, uma por grupo de captura.
    Group(Vec<MonarchAction>),
    /// `{ cases: { guarda: ação, ... } }`.
    Cases(Vec<(String, MonarchAction)>),
}

/// `IMonarchLanguageRule`.
#[derive(Clone, Debug, PartialEq)]
pub enum MonarchRule {
    /// `{ include: '@estado' }`.
    Include(String),
    /// `[regex]`, `[regex, ação]` ou `[regex, ação, próximo]`.
    Tuple { regex: String, action: MonarchAction, next: Option<String> },
    /// `{ regex, action, name? }`. `regex` ausente é o erro "a rule must either be an array...".
    Object { regex: Option<String>, action: MonarchAction, name: Option<String> },
}

/// Um elemento de `brackets`: a lista `[open, close, token]` ou o objeto `{ open, close, token }`.
#[derive(Clone, Debug, PartialEq)]
pub enum MonarchBracketDef {
    Triple(String, String, String),
    Object { open: Option<String>, close: Option<String>, token: Option<String> },
}

/// `IMonarchLanguageBracket`.
#[derive(Clone, Debug, PartialEq)]
pub struct MonarchLanguageBracket {
    pub open: String,
    pub close: String,
    pub token: String,
}

/// `IMonarchLanguage`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MonarchLanguage {
    pub include_lf: Option<bool>,
    pub start: Option<String>,
    pub ignore_case: Option<bool>,
    pub unicode: Option<bool>,
    pub token_postfix: Option<String>,
    pub default_token: Option<String>,
    pub brackets: Option<Vec<MonarchBracketDef>>,
    /// `tokenizer`: os estados na ordem em que foram declarados.
    pub tokenizer: Vec<(String, Vec<MonarchRule>)>,
    /// Os atributos livres (`[key: string]: any`).
    pub attributes: Vec<(String, MonarchAttr)>,
}

impl MonarchLanguage {
    /// Uma linguagem só com o `tokenizer`.
    pub fn new(tokenizer: Vec<(&str, Vec<MonarchRule>)>) -> Self {
        Self { tokenizer: tokenizer.into_iter().map(|(name, rules)| (name.to_owned(), rules)).collect(), ..Self::default() }
    }

    /// Acrescenta um atributo livre.
    pub fn with_attribute(mut self, name: &str, value: MonarchAttr) -> Self {
        self.attributes.push((name.to_owned(), value));
        self
    }
}
