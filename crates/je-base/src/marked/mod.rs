//! Porte do lexer do `marked` v14.0.0 (`src/vs/base/common/marked/marked.js`): o `Lexer`, o `Tokenizer`, as
//! gramáticas de bloco e de linha e os tipos de token, com as mesmas regexes (no motor ECMAScript do
//! `zyarr`), na mesma ordem e com a mesma semântica de texto UTF-16.
//!
//! Fica de fora tudo que só serve para gerar HTML: `Parser`, `Renderer`, `TextRenderer`, `Marked`, `Hooks`,
//! `walkTokens` e as extensões. O `markdownLinks.ts` só chama `marked.lexer(markdown)` e lê a árvore de
//! tokens, então o `Parser`/`Renderer` não têm quem os use no VS Code que estamos portando.
//!
//! O `Tokenizer` do original é uma classe com referência de volta ao `Lexer`; aqui os métodos dele são
//! `impl Lexer` no submódulo `tokenizer`, com os mesmos nomes (`space`, `code`, `fences`...).
//!
//! Diferenças de modelagem (a semântica não muda):
//! - `inline(src)` do original devolve um array que o lexer preenche depois; aqui o token guarda o número
//!   do item da fila e o lexer preenche `tokens` no fim de `lex`.
//! - Os erros que o JavaScript lança (`Infinite loop on byte`, `TypeError`, `RangeError` por pilha) são
//!   [`MarkedError`]. A pilha do V8 acaba em profundidade imprevisível; aqui o limite é
//!   [`MAX_NESTING_DEPTH`] (ver a constante).

mod helpers;
mod lexer;
mod re;
mod rules;
mod tokenizer;

use std::collections::HashMap;
use std::fmt;

use crate::JsString;

pub use lexer::{Lexer, lex_inline, lexer, lexer_with};

/// Profundidade máxima de aninhamento (citação em citação, lista em lista, ênfase em ênfase) antes de
/// devolver [`MarkedError::RangeError`], no lugar do estouro de pilha do V8 (`Maximum call stack size
/// exceeded`, perto de dois mil níveis de citação no Node). Os níveis além dos primeiros 32 rodam numa
/// thread com pilha grande (fora do wasm32, onde o limite é 32).
pub const MAX_NESTING_DEPTH: usize = 1000;

/// As opções do `marked` que o lexer lê. O padrão é o `_getDefaults()` do original.
///
/// `async`, `extensions`, `hooks`, `renderer`, `tokenizer` e `walkTokens` não existem aqui: só valem para
/// quem estende o `marked` ou gera HTML.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Options {
    pub breaks: bool,
    pub gfm: bool,
    pub pedantic: bool,
    /// Com `silent`, um erro de laço infinito é só registrado (aqui: ignorado) e o lexer devolve o que já leu.
    pub silent: bool,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            breaks: false,
            gfm: true,
            pedantic: false,
            silent: false,
        }
    }
}

/// O que o JavaScript lançaria no lexer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MarkedError {
    /// `Error('Infinite loop on byte: ' + src.charCodeAt(0))`.
    InfiniteLoop { byte: u16 },
    /// `TypeError`: propriedade lida de `undefined` (um item de lista ou citação que não saiu).
    TypeError(&'static str),
    /// `RangeError`: aninhamento além de [`MAX_NESTING_DEPTH`] (ver a constante).
    RangeError,
}

impl fmt::Display for MarkedError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MarkedError::InfiniteLoop { byte } => write!(f, "Infinite loop on byte: {byte}"),
            MarkedError::TypeError(message) => write!(f, "TypeError: {message}"),
            MarkedError::RangeError => f.write_str("RangeError: Maximum call stack size exceeded"),
        }
    }
}

impl std::error::Error for MarkedError {}

/// `Tokens.Link` de uma definição de referência, o valor do mapa `links`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LinkDefinition {
    pub href: JsString,
    pub title: Option<JsString>,
}

/// `TokensList`: os tokens de topo mais o mapa `links` (chave em minúsculas, espaços colapsados).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TokensList {
    pub tokens: Vec<Token>,
    pub links: HashMap<JsString, LinkDefinition>,
}

/// Alinhamento de uma coluna de tabela.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Align {
    Left,
    Center,
    Right,
}

impl Align {
    pub fn as_str(self) -> &'static str {
        match self {
            Align::Left => "left",
            Align::Center => "center",
            Align::Right => "right",
        }
    }
}

/// Um token. Cada variante é um `Tokens.*` do `marked.d.ts`.
#[derive(Clone, Debug, PartialEq)]
pub enum Token {
    Space(Space),
    Code(Code),
    Heading(Heading),
    Table(Table),
    Hr(Hr),
    Blockquote(Blockquote),
    List(List),
    /// Só aparece em [`List::items`].
    ListItem(ListItem),
    Paragraph(Paragraph),
    Html(Html),
    Text(Text),
    Def(Def),
    Escape(Escape),
    Image(Image),
    Link(Link),
    Strong(Strong),
    Em(Em),
    Codespan(Codespan),
    Br(Br),
    Del(Del),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Space {
    pub raw: JsString,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Code {
    pub raw: JsString,
    /// `codeBlockStyle: 'indented'` (bloco por recuo); `false` para bloco cercado.
    pub indented: bool,
    /// `lang` do bloco cercado (vazio quando não há); `None` para bloco por recuo.
    pub lang: Option<JsString>,
    pub text: JsString,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Heading {
    pub raw: JsString,
    pub depth: u8,
    pub text: JsString,
    pub tokens: Vec<Token>,
    pub(crate) slot: Option<usize>,
}

/// Célula de tabela (`Tokens.TableCell`).
#[derive(Clone, Debug, PartialEq)]
pub struct TableCell {
    pub text: JsString,
    pub tokens: Vec<Token>,
    pub header: bool,
    pub align: Option<Align>,
    pub(crate) slot: Option<usize>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Table {
    pub raw: JsString,
    pub align: Vec<Option<Align>>,
    pub header: Vec<TableCell>,
    pub rows: Vec<Vec<TableCell>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hr {
    pub raw: JsString,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Blockquote {
    pub raw: JsString,
    pub text: JsString,
    pub tokens: Vec<Token>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct List {
    pub raw: JsString,
    pub ordered: bool,
    /// O número inicial de uma lista ordenada; `None` é o `''` de uma lista com marcador.
    pub start: Option<u32>,
    pub loose: bool,
    /// Sempre [`Token::ListItem`].
    pub items: Vec<Token>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ListItem {
    pub raw: JsString,
    pub task: bool,
    /// `checked` só existe quando `task`.
    pub checked: Option<bool>,
    pub loose: bool,
    pub text: JsString,
    pub tokens: Vec<Token>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Paragraph {
    pub raw: JsString,
    pub text: JsString,
    pub tokens: Vec<Token>,
    pub(crate) slot: Option<usize>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Html {
    pub raw: JsString,
    pub text: JsString,
    pub block: bool,
    /// Só em HTML de bloco: `<pre>`, `<script>` ou `<style>`.
    pub pre: Option<bool>,
    /// Só em HTML de linha.
    pub in_link: Option<bool>,
    /// Só em HTML de linha.
    pub in_raw_block: Option<bool>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Text {
    pub raw: JsString,
    pub text: JsString,
    /// Só no texto de bloco; o texto de linha não tem filhos.
    pub tokens: Option<Vec<Token>>,
    pub(crate) slot: Option<usize>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Def {
    pub raw: JsString,
    pub tag: JsString,
    pub href: JsString,
    pub title: Option<JsString>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Escape {
    pub raw: JsString,
    pub text: JsString,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Image {
    pub raw: JsString,
    pub href: JsString,
    pub title: Option<JsString>,
    pub text: JsString,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Link {
    pub raw: JsString,
    pub href: JsString,
    pub title: Option<JsString>,
    pub text: JsString,
    pub tokens: Vec<Token>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Strong {
    pub raw: JsString,
    pub text: JsString,
    pub tokens: Vec<Token>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Em {
    pub raw: JsString,
    pub text: JsString,
    pub tokens: Vec<Token>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Codespan {
    pub raw: JsString,
    pub text: JsString,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Br {
    pub raw: JsString,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Del {
    pub raw: JsString,
    pub text: JsString,
    pub tokens: Vec<Token>,
}

impl Token {
    /// O `type` do token.
    pub fn type_name(&self) -> &'static str {
        match self {
            Token::Space(_) => "space",
            Token::Code(_) => "code",
            Token::Heading(_) => "heading",
            Token::Table(_) => "table",
            Token::Hr(_) => "hr",
            Token::Blockquote(_) => "blockquote",
            Token::List(_) => "list",
            Token::ListItem(_) => "list_item",
            Token::Paragraph(_) => "paragraph",
            Token::Html(_) => "html",
            Token::Text(_) => "text",
            Token::Def(_) => "def",
            Token::Escape(_) => "escape",
            Token::Image(_) => "image",
            Token::Link(_) => "link",
            Token::Strong(_) => "strong",
            Token::Em(_) => "em",
            Token::Codespan(_) => "codespan",
            Token::Br(_) => "br",
            Token::Del(_) => "del",
        }
    }

    /// `token.raw`: o trecho do fonte que o token consumiu (todo token tem).
    pub fn raw(&self) -> &JsString {
        match self {
            Token::Space(t) => &t.raw,
            Token::Code(t) => &t.raw,
            Token::Heading(t) => &t.raw,
            Token::Table(t) => &t.raw,
            Token::Hr(t) => &t.raw,
            Token::Blockquote(t) => &t.raw,
            Token::List(t) => &t.raw,
            Token::ListItem(t) => &t.raw,
            Token::Paragraph(t) => &t.raw,
            Token::Html(t) => &t.raw,
            Token::Text(t) => &t.raw,
            Token::Def(t) => &t.raw,
            Token::Escape(t) => &t.raw,
            Token::Image(t) => &t.raw,
            Token::Link(t) => &t.raw,
            Token::Strong(t) => &t.raw,
            Token::Em(t) => &t.raw,
            Token::Codespan(t) => &t.raw,
            Token::Br(t) => &t.raw,
            Token::Del(t) => &t.raw,
        }
    }

    pub(crate) fn raw_mut(&mut self) -> &mut JsString {
        match self {
            Token::Space(t) => &mut t.raw,
            Token::Code(t) => &mut t.raw,
            Token::Heading(t) => &mut t.raw,
            Token::Table(t) => &mut t.raw,
            Token::Hr(t) => &mut t.raw,
            Token::Blockquote(t) => &mut t.raw,
            Token::List(t) => &mut t.raw,
            Token::ListItem(t) => &mut t.raw,
            Token::Paragraph(t) => &mut t.raw,
            Token::Html(t) => &mut t.raw,
            Token::Text(t) => &mut t.raw,
            Token::Def(t) => &mut t.raw,
            Token::Escape(t) => &mut t.raw,
            Token::Image(t) => &mut t.raw,
            Token::Link(t) => &mut t.raw,
            Token::Strong(t) => &mut t.raw,
            Token::Em(t) => &mut t.raw,
            Token::Codespan(t) => &mut t.raw,
            Token::Br(t) => &mut t.raw,
            Token::Del(t) => &mut t.raw,
        }
    }

    /// `token.text`, quando o tipo tem (`space`, `hr`, `br`, `list`, `table` e `def` não têm).
    pub fn text(&self) -> Option<&JsString> {
        match self {
            Token::Code(t) => Some(&t.text),
            Token::Heading(t) => Some(&t.text),
            Token::Blockquote(t) => Some(&t.text),
            Token::ListItem(t) => Some(&t.text),
            Token::Paragraph(t) => Some(&t.text),
            Token::Html(t) => Some(&t.text),
            Token::Text(t) => Some(&t.text),
            Token::Escape(t) => Some(&t.text),
            Token::Image(t) => Some(&t.text),
            Token::Link(t) => Some(&t.text),
            Token::Strong(t) => Some(&t.text),
            Token::Em(t) => Some(&t.text),
            Token::Codespan(t) => Some(&t.text),
            Token::Del(t) => Some(&t.text),
            Token::Space(_) | Token::Table(_) | Token::Hr(_) | Token::List(_) | Token::Def(_) | Token::Br(_) => None,
        }
    }

    pub(crate) fn text_mut(&mut self) -> Option<&mut JsString> {
        match self {
            Token::Code(t) => Some(&mut t.text),
            Token::Paragraph(t) => Some(&mut t.text),
            Token::Text(t) => Some(&mut t.text),
            _ => None,
        }
    }

    /// `token.tokens`, quando o tipo tem (e, no `text`, quando foi lexado em bloco).
    pub fn tokens(&self) -> Option<&[Token]> {
        match self {
            Token::Heading(t) => Some(&t.tokens),
            Token::Blockquote(t) => Some(&t.tokens),
            Token::ListItem(t) => Some(&t.tokens),
            Token::Paragraph(t) => Some(&t.tokens),
            Token::Text(t) => t.tokens.as_deref(),
            Token::Link(t) => Some(&t.tokens),
            Token::Strong(t) => Some(&t.tokens),
            Token::Em(t) => Some(&t.tokens),
            Token::Del(t) => Some(&t.tokens),
            _ => None,
        }
    }
}
