//! Porte de `src/vs/editor/contrib/snippet/browser/snippetParser.ts`: o analisador da sintaxe de snippets
//! do TextMate (`$1`, `${1:padrão}`, `${1|a,b|}`, `$VAR`, `${VAR/regex/formato/opções}`).
//!
//! Decisões de tradução:
//! - Os marcadores formam uma árvore com identidade e ligação ao pai. Em vez de uma classe por tipo, há um
//!   único nó [`Marker`] (compartilhado por [`MarkerRef`], com mutabilidade interior) cujo [`MarkerData`]
//!   carrega o que é próprio de cada tipo (`Text`, `Placeholder`, `Choice`, `Transform`, `FormatString`,
//!   `Variable`, `TextmateSnippet`). O `instanceof` vira [`Marker::kind`] ou os acessores `as_*`. O pai é um
//!   `Weak`: como no original, quem segura a raiz mantém a árvore viva, e o pai não mantém o filho vivo.
//! - Texto é UTF-16 ([`JsString`] e `&[u16]`): índices do [`Scanner`], comprimentos de [`Marker::len`] e
//!   deslocamentos contam unidades de código, como no JavaScript.
//! - `RegExp` é o [`JsRegExp`] (motor `zyarr`); `String#replace`, `String#match` com `g` e `split` são
//!   emulados com as regras da especificação (`lastIndex`, avanço depois de casamento vazio).
//! - O índice de `Placeholder` e de `FormatString` é `u64` (`Number` do JavaScript); um literal com mais de
//!   19 dígitos satura em `u64::MAX`.
//! - `Text.escape` e `SnippetParser.escape` são a mesma função no original; aqui é só
//!   [`SnippetParser::escape`].
//! - A cópia profunda de `clone()` chama-se [`Marker::deep_clone`], para não se confundir com o `Clone` do
//!   `Rc`. `TextmateSnippet#offset` devolve `Option` no lugar do `-1` do original.
//! - Chamar um método próprio de um tipo (por exemplo [`Marker::resolve_transform`]) num marcador de outro
//!   tipo é erro de programação e dá pânico, como o `TypeError` do original.

use std::cell::{Ref, RefCell};
use std::cmp::Ordering;
use std::collections::{HashMap, HashSet, VecDeque};
use std::rc::{Rc, Weak};

use je_base::JsString;
use je_base::char_code as cc;
use je_base::js_regexp::Captures;
use je_base::js_string::trim;
use zyarr::Match;

use je_base::js_regexp::JsRegExp;

// ---------------------------------------------------------------------------------------------
// Scanner
// ---------------------------------------------------------------------------------------------

/// `TokenType`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TokenType {
    Dollar,
    Colon,
    Comma,
    CurlyOpen,
    CurlyClose,
    Backslash,
    Forwardslash,
    Pipe,
    Int,
    VariableName,
    Format,
    Plus,
    Dash,
    QuestionMark,
    EOF,
}

/// `Token`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Token {
    pub kind: TokenType,
    pub pos: usize,
    pub len: usize,
}

/// `Scanner`.
#[derive(Debug, Default)]
pub struct Scanner {
    pub value: Vec<u16>,
    pub pos: usize,
}

impl Scanner {
    /// `Scanner._table`: os caracteres que são um token sozinhos.
    fn static_token_type(ch: u16) -> Option<TokenType> {
        Some(match ch {
            cc::DollarSign => TokenType::Dollar,
            cc::Colon => TokenType::Colon,
            cc::Comma => TokenType::Comma,
            cc::OpenCurlyBrace => TokenType::CurlyOpen,
            cc::CloseCurlyBrace => TokenType::CurlyClose,
            cc::Backslash => TokenType::Backslash,
            cc::Slash => TokenType::Forwardslash,
            cc::Pipe => TokenType::Pipe,
            cc::Plus => TokenType::Plus,
            cc::Dash => TokenType::Dash,
            cc::QuestionMark => TokenType::QuestionMark,
            _ => return None,
        })
    }

    pub fn is_digit_character(ch: u16) -> bool {
        (cc::Digit0..=cc::Digit9).contains(&ch)
    }

    pub fn is_variable_character(ch: u16) -> bool {
        ch == cc::Underline || (cc::a..=cc::z).contains(&ch) || (cc::A..=cc::Z).contains(&ch)
    }

    /// `value.charCodeAt(index)`, com o `NaN` como `None`.
    fn at(&self, index: usize) -> Option<u16> {
        self.value.get(index).copied()
    }

    pub fn text(&mut self, value: &[u16]) {
        self.value = value.to_vec();
        self.pos = 0;
    }

    /// `value.substr(token.pos, token.len)`.
    pub fn token_text(&self, token: &Token) -> JsString {
        let start = token.pos.min(self.value.len());
        let end = (token.pos + token.len).min(self.value.len());
        JsString::from(&self.value[start..end])
    }

    #[allow(clippy::should_implement_trait)]
    pub fn next(&mut self) -> Token {
        if self.pos >= self.value.len() {
            return Token { kind: TokenType::EOF, pos: self.pos, len: 0 };
        }

        let pos = self.pos;
        let mut len = 0;
        let ch = self.value[pos];

        // static types
        if let Some(kind) = Scanner::static_token_type(ch) {
            self.pos += 1;
            return Token { kind, pos, len: 1 };
        }

        // number
        if Scanner::is_digit_character(ch) {
            loop {
                len += 1;
                if !self.at(pos + len).is_some_and(Scanner::is_digit_character) {
                    break;
                }
            }
            self.pos += len;
            return Token { kind: TokenType::Int, pos, len };
        }

        // variable name
        if Scanner::is_variable_character(ch) {
            loop {
                len += 1;
                let continues =
                    self.at(pos + len).is_some_and(|c| Scanner::is_variable_character(c) || Scanner::is_digit_character(c));
                if !continues {
                    break;
                }
            }
            self.pos += len;
            return Token { kind: TokenType::VariableName, pos, len };
        }

        // format
        loop {
            len += 1;
            let continues = self.at(pos + len).is_some_and(|c| {
                Scanner::static_token_type(c).is_none()
                    && !Scanner::is_digit_character(c)
                    && !Scanner::is_variable_character(c)
            });
            if !continues {
                break;
            }
        }
        self.pos += len;
        Token { kind: TokenType::Format, pos, len }
    }
}

// ---------------------------------------------------------------------------------------------
// Texto e expressões regulares, no estilo do JavaScript
// ---------------------------------------------------------------------------------------------

fn append(out: &mut JsString, text: &str) {
    for unit in text.encode_utf16() {
        out.push_unit(unit);
    }
}

/// `AdvanceStringIndex`.
fn advance_string_index(input: &[u16], index: usize, unicode: bool) -> usize {
    let pair = unicode
        && index + 1 < input.len()
        && (0xD800..=0xDBFF).contains(&input[index])
        && (0xDC00..=0xDFFF).contains(&input[index + 1]);
    if pair { index + 2 } else { index + 1 }
}

/// Os casamentos que `String#replace` e `String#match` percorrem: só o primeiro sem a flag `g`; com ela,
/// todos, zerando `lastIndex` antes e avançando depois de um casamento vazio.
fn collect_matches(regexp: &JsRegExp, input: &[u16]) -> Vec<Match> {
    let flags = regexp.flags();
    if !flags.global {
        return regexp.exec(input).into_iter().collect();
    }
    regexp.set_last_index(0);
    let mut matches = Vec::new();
    while let Some(found) = regexp.exec(input) {
        if found.end() == found.start() {
            regexp.set_last_index(advance_string_index(input, found.end(), flags.either_unicode()));
        }
        matches.push(found);
    }
    matches
}

/// `input.replace(regexp, (match, ...groups) => replacement)`.
fn replace_with(regexp: &JsRegExp, input: &[u16], mut replacement: impl FnMut(&Captures<'_>) -> JsString) -> JsString {
    let mut out = JsString::new();
    let mut copied = 0;
    for found in collect_matches(regexp, input) {
        if found.start() >= copied {
            out.push_units(&input[copied..found.start()]);
            out.push_units(replacement(&Captures { input, matched: &found }).units());
            copied = found.end();
        }
    }
    out.push_units(&input[copied..]);
    out
}

/// `input.match(regexp)` com o texto de cada casamento (vazio quando `match` daria `null`).
fn match_texts<'a>(regexp: &JsRegExp, input: &'a [u16]) -> Vec<&'a [u16]> {
    collect_matches(regexp, input)
        .into_iter()
        .filter_map(|found| found.captures.first().cloned().flatten())
        .map(|range| &input[range])
        .collect()
}

/// `regexp.source`: o padrão com `/` fora de classe e as quebras de linha escapados, como o V8 devolve.
fn regexp_source(pattern: &[u16]) -> JsString {
    if pattern.is_empty() {
        return JsString::from("(?:)");
    }
    let line_terminator = |ch: u16| match ch {
        0x0A => Some("\\n"),
        0x0D => Some("\\r"),
        0x2028 => Some("\\u2028"),
        0x2029 => Some("\\u2029"),
        _ => None,
    };
    let mut out = JsString::new();
    let mut in_class = false;
    let mut i = 0;
    while i < pattern.len() {
        let ch = pattern[i];
        if ch == cc::Backslash {
            out.push_unit(ch);
            i += 1;
            if let Some(&escaped) = pattern.get(i) {
                match line_terminator(escaped) {
                    // `\` seguido de quebra de linha vira `\n`: a barra já saiu, falta a letra.
                    Some(text) => append(&mut out, &text[1..]),
                    None => out.push_unit(escaped),
                }
            }
        } else if let Some(text) = line_terminator(ch) {
            append(&mut out, text);
        } else if ch == cc::Slash && !in_class {
            out.push_unit(cc::Backslash);
            out.push_unit(ch);
        } else {
            if ch == cc::OpenSquareBracket {
                in_class = true;
            } else if ch == cc::CloseSquareBracket {
                in_class = false;
            }
            out.push_unit(ch);
        }
        i += 1;
    }
    out
}

fn fixed_regexp(pattern: &str, flags: &str) -> JsRegExp {
    JsRegExp::from_str(pattern, flags).unwrap_or_else(|e| panic!("regexp fixa inválida `{pattern}`: {e}"))
}

/// As expressões regulares dos `FormatString` de caixa (`_toKebabCase` e companhia), compiladas uma vez.
struct CaseRegExps {
    /// `/[\p{L}0-9]+/gu`
    words: JsRegExp,
    /// `/[\p{L}0-9]/u`
    has_word: JsRegExp,
    /// `/^_+|_+$/g`
    edge_underscores: JsRegExp,
    /// `/[\s_]+/g`
    whitespace_underscores: JsRegExp,
    /// A palavra do kebab case, com `gu`.
    kebab_words: JsRegExp,
    /// `/[\s_-]+/`, em `g` aqui só para percorrer todos os casamentos do `split`.
    separators: JsRegExp,
    /// `/(\p{Ll})(\p{Lu})/gu`
    lower_upper: JsRegExp,
    /// `/[\s\-]+/g`
    snake_separators: JsRegExp,
}

impl CaseRegExps {
    fn new() -> CaseRegExps {
        CaseRegExps {
            words: fixed_regexp(r"[\p{L}0-9]+", "gu"),
            has_word: fixed_regexp(r"[\p{L}0-9]", "u"),
            edge_underscores: fixed_regexp(r"^_+|_+$", "g"),
            whitespace_underscores: fixed_regexp(r"[\s_]+", "g"),
            kebab_words: fixed_regexp(
                r"\p{Lu}{2,}(?=\p{Lu}\p{Ll}+[0-9]*|[\s_-]|$)|\p{Lu}?\p{Ll}+[0-9]*|\p{Lu}(?=\p{Lu}\p{Ll})|\p{Lu}(?=[\s_-]|$)|[0-9]+",
                "gu",
            ),
            separators: fixed_regexp(r"[\s_-]+", "g"),
            lower_upper: fixed_regexp(r"(\p{Ll})(\p{Lu})", "gu"),
            snake_separators: fixed_regexp(r"[\s\-]+", "g"),
        }
    }
}

thread_local! {
    static CASE: CaseRegExps = CaseRegExps::new();
    /// `/\${?CLIPBOARD/`
    static CLIPBOARD: JsRegExp = fixed_regexp(r"\$\{?CLIPBOARD", "");
}

/// `word.charAt(0).toUpperCase() + word.substr(1)`.
fn upper_first(word: &[u16]) -> JsString {
    let head = word.len().min(1);
    let mut out = JsString::from(&word[..head]).to_upper_case();
    out.push_units(&word[head..]);
    out
}

/// `word.charAt(0).toLowerCase() + word.substr(1)`.
fn lower_first(word: &[u16]) -> JsString {
    let head = word.len().min(1);
    let mut out = JsString::from(&word[..head]).to_lower_case();
    out.push_units(&word[head..]);
    out
}

/// `parts.join(separator)`.
fn join_units(parts: impl IntoIterator<Item = JsString>, separator: Option<u16>) -> JsString {
    let mut out = JsString::new();
    for (i, part) in parts.into_iter().enumerate() {
        if i > 0
            && let Some(separator) = separator
        {
            out.push_unit(separator);
        }
        out.push_units(part.units());
    }
    out
}

fn to_kebab_case(value: &[u16]) -> JsString {
    CASE.with(|c| {
        if match_texts(&c.words, value).is_empty() {
            return JsString::from(value);
        }

        if collect_matches(&c.has_word, value).is_empty() {
            let lowered = JsString::from(trim(value)).to_lower_case();
            let stripped = replace_with(&c.edge_underscores, lowered.units(), |_| JsString::new());
            return replace_with(&c.whitespace_underscores, stripped.units(), |_| JsString::from("-"));
        }

        let cleaned = replace_with(&c.edge_underscores, trim(value), |_| JsString::new());
        let words = match_texts(&c.kebab_words, cleaned.units());
        if words.is_empty() {
            let mut parts: Vec<&[u16]> = Vec::new();
            let mut last = 0;
            for found in collect_matches(&c.separators, cleaned.units()) {
                parts.push(&cleaned.units()[last..found.start()]);
                last = found.end();
            }
            parts.push(&cleaned.units()[last..]);
            return join_units(
                parts.into_iter().filter(|word| !word.is_empty()).map(|word| JsString::from(word).to_lower_case()),
                Some(cc::Dash),
            );
        }

        join_units(words.into_iter().map(|word| JsString::from(word).to_lower_case()), Some(cc::Dash))
    })
}

fn to_pascal_case(value: &[u16]) -> JsString {
    let words = CASE.with(|c| match_texts(&c.words, value).into_iter().map(upper_first).collect::<Vec<_>>());
    if words.is_empty() {
        return JsString::from(value);
    }
    join_units(words, None)
}

fn to_camel_case(value: &[u16]) -> JsString {
    let words = CASE.with(|c| {
        match_texts(&c.words, value)
            .into_iter()
            .enumerate()
            .map(|(index, word)| if index == 0 { lower_first(word) } else { upper_first(word) })
            .collect::<Vec<_>>()
    });
    if words.is_empty() {
        return JsString::from(value);
    }
    join_units(words, None)
}

fn to_snake_case(value: &[u16]) -> JsString {
    CASE.with(|c| {
        let joined = replace_with(&c.lower_upper, value, |found| {
            let mut out = JsString::from(found.group(1).unwrap_or_default());
            out.push_unit(cc::Underline);
            out.push_units(found.group(2).unwrap_or_default());
            out
        });
        let separated = replace_with(&c.snake_separators, joined.units(), |_| JsString::from("_"));
        separated.to_lower_case()
    })
}

// ---------------------------------------------------------------------------------------------
// Marcadores
// ---------------------------------------------------------------------------------------------

/// Um marcador compartilhado: a identidade do original (`===`) é [`Rc::ptr_eq`].
pub type MarkerRef = Rc<Marker>;

/// O tipo de um marcador (o que o `instanceof` distingue).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MarkerKind {
    Text,
    Placeholder,
    Choice,
    Transform,
    FormatString,
    Variable,
    TextmateSnippet,
}

/// `Text`.
#[derive(Debug)]
pub struct TextData {
    pub value: RefCell<JsString>,
}

/// `Placeholder`.
#[derive(Debug)]
pub struct PlaceholderData {
    pub index: u64,
    pub transform: RefCell<Option<MarkerRef>>,
}

impl PlaceholderData {
    pub fn is_final_tabstop(&self) -> bool {
        self.index == 0
    }
}

/// `Choice`: as opções ficam em `options`, e não em `children`.
#[derive(Debug)]
pub struct ChoiceData {
    pub options: RefCell<Vec<MarkerRef>>,
}

/// `Transform`.
#[derive(Debug)]
pub struct TransformData {
    pub regexp: RefCell<JsRegExp>,
}

impl TransformData {
    /// `regexp.source`.
    pub fn source(&self) -> JsString {
        regexp_source(self.regexp.borrow().source())
    }
}

/// `FormatString`.
#[derive(Debug)]
pub struct FormatStringData {
    pub index: u64,
    pub shorthand_name: Option<String>,
    pub if_value: Option<JsString>,
    pub else_value: Option<JsString>,
}

/// `Boolean(value)` de uma `string` opcional.
fn is_truthy(value: Option<&JsString>) -> bool {
    value.is_some_and(|text| !text.is_empty())
}

impl FormatStringData {
    pub fn new(index: u64, shorthand_name: Option<&str>, if_value: Option<JsString>, else_value: Option<JsString>) -> Self {
        FormatStringData { index, shorthand_name: shorthand_name.map(str::to_owned), if_value, else_value }
    }

    pub fn resolve(&self, value: Option<&[u16]>) -> JsString {
        let text = value.unwrap_or_default();
        let has_value = !text.is_empty();
        match self.shorthand_name.as_deref() {
            Some("upcase") => {
                return if has_value { JsString::from(text).to_upper_case() } else { JsString::new() };
            }
            Some("downcase") => {
                return if has_value { JsString::from(text).to_lower_case() } else { JsString::new() };
            }
            Some("capitalize") => return if has_value { upper_first(text) } else { JsString::new() },
            Some("pascalcase") => return if has_value { to_pascal_case(text) } else { JsString::new() },
            Some("camelcase") => return if has_value { to_camel_case(text) } else { JsString::new() },
            Some("kebabcase") => return if has_value { to_kebab_case(text) } else { JsString::new() },
            Some("snakecase") => return if has_value { to_snake_case(text) } else { JsString::new() },
            _ => {}
        }
        if has_value && let Some(if_value) = &self.if_value {
            if_value.clone()
        } else if !has_value && let Some(else_value) = &self.else_value {
            else_value.clone()
        } else {
            JsString::from(text)
        }
    }

    fn to_textmate_string(&self) -> JsString {
        let mut out = JsString::new();
        append(&mut out, "${");
        append(&mut out, &self.index.to_string());
        if let Some(shorthand) = self.shorthand_name.as_deref().filter(|name| !name.is_empty()) {
            append(&mut out, ":/");
            append(&mut out, shorthand);
        } else if is_truthy(self.if_value.as_ref()) && is_truthy(self.else_value.as_ref()) {
            append(&mut out, ":?");
            out.push_units(self.if_value.as_ref().map_or(&[][..], JsString::units));
            out.push_unit(cc::Colon);
            out.push_units(self.else_value.as_ref().map_or(&[][..], JsString::units));
        } else if is_truthy(self.if_value.as_ref()) {
            append(&mut out, ":+");
            out.push_units(self.if_value.as_ref().map_or(&[][..], JsString::units));
        } else if is_truthy(self.else_value.as_ref()) {
            append(&mut out, ":-");
            out.push_units(self.else_value.as_ref().map_or(&[][..], JsString::units));
        }
        append(&mut out, "}");
        out
    }
}

/// `Variable`.
#[derive(Debug)]
pub struct VariableData {
    pub name: String,
    pub transform: RefCell<Option<MarkerRef>>,
}

/// O resultado de `TextmateSnippet#placeholderInfo`.
#[derive(Clone, Debug)]
pub struct PlaceholderInfo {
    pub all: Vec<MarkerRef>,
    pub last: Option<MarkerRef>,
}

/// `TextmateSnippet`.
#[derive(Debug)]
pub struct SnippetData {
    placeholders: RefCell<Option<PlaceholderInfo>>,
}

/// O que é próprio de cada tipo de marcador.
#[derive(Debug)]
pub enum MarkerData {
    Text(TextData),
    Placeholder(PlaceholderData),
    Choice(ChoiceData),
    Transform(TransformData),
    FormatString(FormatStringData),
    Variable(VariableData),
    TextmateSnippet(SnippetData),
}

/// `VariableResolver`.
pub trait VariableResolver {
    fn resolve(&self, variable: &MarkerRef) -> Option<JsString>;
}

/// `Marker`.
#[derive(Debug)]
pub struct Marker {
    data: MarkerData,
    parent: RefCell<Weak<Marker>>,
    children: RefCell<Vec<MarkerRef>>,
}

/// `Marker#replace`: aponta o pai de cada marcador (e dos descendentes) para o certo.
fn fix_parent(children: &[MarkerRef], parent: &MarkerRef) {
    for child in children {
        *child.parent.borrow_mut() = Rc::downgrade(parent);
        fix_parent(&child.children(), child);
    }
}

/// `walk`: percorre em profundidade, na ordem do documento, até o visitante devolver `false`. Os filhos
/// são lidos depois da visita, então um visitante que troca os filhos (`Variable#resolve`) é respeitado.
pub fn walk(markers: &[MarkerRef], mut visitor: impl FnMut(&MarkerRef) -> bool) {
    let mut stack: VecDeque<MarkerRef> = markers.iter().cloned().collect();
    while let Some(marker) = stack.pop_front() {
        if !visitor(&marker) {
            break;
        }
        for child in marker.children().into_iter().rev() {
            stack.push_front(child);
        }
    }
}

impl Marker {
    fn from_data(data: MarkerData) -> MarkerRef {
        Rc::new(Marker { data, parent: RefCell::new(Weak::new()), children: RefCell::new(Vec::new()) })
    }

    /// `new Text(value)`.
    pub fn new_text(value: impl Into<JsString>) -> MarkerRef {
        Marker::from_data(MarkerData::Text(TextData { value: RefCell::new(value.into()) }))
    }

    /// `new Placeholder(index)`.
    pub fn new_placeholder(index: u64) -> MarkerRef {
        Marker::from_data(MarkerData::Placeholder(PlaceholderData { index, transform: RefCell::new(None) }))
    }

    /// `new Choice()`.
    pub fn new_choice() -> MarkerRef {
        Marker::from_data(MarkerData::Choice(ChoiceData { options: RefCell::new(Vec::new()) }))
    }

    /// `new Transform()`, com `regexp = new RegExp('')`.
    pub fn new_transform() -> MarkerRef {
        Marker::from_data(MarkerData::Transform(TransformData { regexp: RefCell::new(fixed_regexp("", "")) }))
    }

    /// `new FormatString(index, shorthandName, ifValue, elseValue)`.
    pub fn new_format_string(
        index: u64,
        shorthand_name: Option<&str>,
        if_value: Option<JsString>,
        else_value: Option<JsString>,
    ) -> MarkerRef {
        Marker::from_data(MarkerData::FormatString(FormatStringData::new(index, shorthand_name, if_value, else_value)))
    }

    /// `new Variable(name)`.
    pub fn new_variable(name: impl Into<String>) -> MarkerRef {
        Marker::from_data(MarkerData::Variable(VariableData { name: name.into(), transform: RefCell::new(None) }))
    }

    /// `new TextmateSnippet()`.
    pub fn new_textmate_snippet() -> MarkerRef {
        Marker::from_data(MarkerData::TextmateSnippet(SnippetData { placeholders: RefCell::new(None) }))
    }

    pub fn data(&self) -> &MarkerData {
        &self.data
    }

    /// O tipo do marcador (`instanceof`).
    pub fn kind(&self) -> MarkerKind {
        match &self.data {
            MarkerData::Text(_) => MarkerKind::Text,
            MarkerData::Placeholder(_) => MarkerKind::Placeholder,
            MarkerData::Choice(_) => MarkerKind::Choice,
            MarkerData::Transform(_) => MarkerKind::Transform,
            MarkerData::FormatString(_) => MarkerKind::FormatString,
            MarkerData::Variable(_) => MarkerKind::Variable,
            MarkerData::TextmateSnippet(_) => MarkerKind::TextmateSnippet,
        }
    }

    pub fn as_text(&self) -> Option<&TextData> {
        match &self.data {
            MarkerData::Text(data) => Some(data),
            _ => None,
        }
    }

    pub fn as_placeholder(&self) -> Option<&PlaceholderData> {
        match &self.data {
            MarkerData::Placeholder(data) => Some(data),
            _ => None,
        }
    }

    pub fn as_choice(&self) -> Option<&ChoiceData> {
        match &self.data {
            MarkerData::Choice(data) => Some(data),
            _ => None,
        }
    }

    pub fn as_transform(&self) -> Option<&TransformData> {
        match &self.data {
            MarkerData::Transform(data) => Some(data),
            _ => None,
        }
    }

    pub fn as_format_string(&self) -> Option<&FormatStringData> {
        match &self.data {
            MarkerData::FormatString(data) => Some(data),
            _ => None,
        }
    }

    pub fn as_variable(&self) -> Option<&VariableData> {
        match &self.data {
            MarkerData::Variable(data) => Some(data),
            _ => None,
        }
    }

    fn as_snippet(&self) -> Option<&SnippetData> {
        match &self.data {
            MarkerData::TextmateSnippet(data) => Some(data),
            _ => None,
        }
    }

    /// `parent`: `None` quando o marcador não tem pai (a raiz, ou um filho que ninguém adotou).
    pub fn parent(&self) -> Option<MarkerRef> {
        self.parent.borrow().upgrade()
    }

    /// `children`: uma cópia da lista (os marcadores são os mesmos).
    pub fn children(&self) -> Vec<MarkerRef> {
        self.children.borrow().clone()
    }

    /// `transform` de um `TransformableMarker` (`Placeholder` e `Variable`); `None` nos demais.
    pub fn transform(&self) -> Option<MarkerRef> {
        match &self.data {
            MarkerData::Placeholder(data) => data.transform.borrow().clone(),
            MarkerData::Variable(data) => data.transform.borrow().clone(),
            _ => None,
        }
    }

    pub fn set_transform(&self, transform: Option<MarkerRef>) {
        match &self.data {
            MarkerData::Placeholder(data) => *data.transform.borrow_mut() = transform,
            MarkerData::Variable(data) => *data.transform.borrow_mut() = transform,
            _ => panic!("{:?} não aceita transform", self.kind()),
        }
    }

    /// `Placeholder.compareByIndex`.
    pub fn compare_by_index(a: &Marker, b: &Marker) -> Ordering {
        let (a, b) = (a.as_placeholder().expect("Placeholder"), b.as_placeholder().expect("Placeholder"));
        if a.index == b.index {
            Ordering::Equal
        } else if a.is_final_tabstop() {
            Ordering::Greater
        } else if b.is_final_tabstop() {
            Ordering::Less
        } else if a.index < b.index {
            Ordering::Less
        } else {
            Ordering::Greater
        }
    }

    /// `Placeholder#choice`.
    pub fn choice(&self) -> Option<MarkerRef> {
        self.as_placeholder()?;
        let children = self.children.borrow();
        match children.as_slice() {
            [only] if only.kind() == MarkerKind::Choice => Some(only.clone()),
            _ => None,
        }
    }

    /// `TextmateSnippet` guarda os placeholders calculados; qualquer mudança na árvore os invalida.
    fn invalidate_placeholders(&self) {
        if let Some(snippet) = self.as_snippet() {
            snippet.placeholders.borrow_mut().take();
        }
    }

    /// `appendChild`: dois `Text` seguidos viram um só; `Choice` só aceita `Text`, nas suas opções;
    /// `TextmateSnippet` invalida os placeholders.
    pub fn append_child(self: &Rc<Self>, child: MarkerRef) {
        if let MarkerData::Choice(choice) = &self.data {
            if child.as_text().is_some() {
                *child.parent.borrow_mut() = Rc::downgrade(self);
                choice.options.borrow_mut().push(child);
            }
            return;
        }
        self.invalidate_placeholders();

        let mut children = self.children.borrow_mut();
        if let (Some(incoming), Some(last)) = (child.as_text(), children.last().and_then(|last| last.as_text())) {
            // this and previous child are text -> merge them
            last.value.borrow_mut().push_units(incoming.value.borrow().units());
            return;
        }
        // normal adoption of child
        *child.parent.borrow_mut() = Rc::downgrade(self);
        children.push(child);
    }

    /// `replace`: troca `child` por `others` na lista de filhos do pai de `child`.
    pub fn replace(&self, child: &MarkerRef, others: Vec<MarkerRef>) {
        self.invalidate_placeholders();
        let parent = child.parent().expect("o marcador substituído precisa ter pai");
        let mut new_children = parent.children();
        let len = new_children.len();
        // `splice(-1, 1, ...)` do JavaScript troca o último quando `child` não está na lista.
        let start = new_children.iter().position(|candidate| Rc::ptr_eq(candidate, child)).unwrap_or(len.saturating_sub(1));
        let end = (start + 1).min(len);
        new_children.splice(start..end, others.iter().cloned());
        *parent.children.borrow_mut() = new_children;
        fix_parent(&others, &parent);
    }

    /// `rightMostDescendant`.
    pub fn right_most_descendant(self: &Rc<Self>) -> MarkerRef {
        let mut current = self.clone();
        loop {
            let last = current.children.borrow().last().cloned();
            match last {
                Some(last) => current = last,
                None => return current,
            }
        }
    }

    /// `snippet`: o `TextmateSnippet` que contém este marcador (ou ele mesmo).
    pub fn snippet(self: &Rc<Self>) -> Option<MarkerRef> {
        let mut candidate = Some(self.clone());
        while let Some(current) = candidate {
            if current.as_snippet().is_some() {
                return Some(current);
            }
            candidate = current.parent();
        }
        None
    }

    /// `toString`.
    pub fn to_js_string(&self) -> JsString {
        match &self.data {
            MarkerData::Text(text) => text.value.borrow().clone(),
            MarkerData::Choice(choice) => choice.options.borrow()[0].to_js_string(),
            MarkerData::Transform(_) => JsString::new(),
            _ => {
                let mut out = JsString::new();
                for child in self.children() {
                    out.push_units(child.to_js_string().units());
                }
                out
            }
        }
    }

    /// `len`.
    pub fn len(&self) -> usize {
        match &self.data {
            MarkerData::Text(text) => text.value.borrow().len(),
            MarkerData::Choice(choice) => choice.options.borrow()[0].len(),
            _ => 0,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    fn children_to_textmate_string(&self) -> JsString {
        let mut out = JsString::new();
        for child in self.children() {
            out.push_units(child.to_textmate_string().units());
        }
        out
    }

    /// `toTextmateString`.
    pub fn to_textmate_string(&self) -> JsString {
        let mut out = JsString::new();
        match &self.data {
            MarkerData::Text(text) => return SnippetParser::escape(text.value.borrow().units()),
            MarkerData::Placeholder(placeholder) => {
                let transform_string =
                    self.transform().map(|transform| transform.to_textmate_string()).unwrap_or_default();
                let has_transform = placeholder.transform.borrow().is_some();
                let index = placeholder.index.to_string();
                let has_children = !self.children.borrow().is_empty();
                if !has_children && !has_transform {
                    append(&mut out, "$");
                    append(&mut out, &index);
                } else if !has_children {
                    append(&mut out, "${");
                    append(&mut out, &index);
                    out.push_units(transform_string.units());
                    append(&mut out, "}");
                } else if let Some(choice) = self.choice() {
                    append(&mut out, "${");
                    append(&mut out, &index);
                    append(&mut out, "|");
                    out.push_units(choice.to_textmate_string().units());
                    append(&mut out, "|");
                    out.push_units(transform_string.units());
                    append(&mut out, "}");
                } else {
                    append(&mut out, "${");
                    append(&mut out, &index);
                    append(&mut out, ":");
                    out.push_units(self.children_to_textmate_string().units());
                    out.push_units(transform_string.units());
                    append(&mut out, "}");
                }
            }
            MarkerData::Choice(choice) => {
                let options = choice.options.borrow();
                for (i, option) in options.iter().enumerate() {
                    if i > 0 {
                        out.push_unit(cc::Comma);
                    }
                    let value = option.as_text().expect("opção de Choice é Text").value.borrow();
                    for &unit in value.units() {
                        if unit == cc::Pipe || unit == cc::Comma || unit == cc::Backslash {
                            out.push_unit(cc::Backslash);
                        }
                        out.push_unit(unit);
                    }
                }
            }
            MarkerData::Transform(transform) => {
                let flags = transform.regexp.borrow().flags();
                append(&mut out, "/");
                out.push_units(transform.source().units());
                append(&mut out, "/");
                out.push_units(self.children_to_textmate_string().units());
                append(&mut out, "/");
                if flags.ignore_case {
                    append(&mut out, "i");
                }
                if flags.global {
                    append(&mut out, "g");
                }
            }
            MarkerData::FormatString(format_string) => return format_string.to_textmate_string(),
            MarkerData::Variable(variable) => {
                let transform_string =
                    self.transform().map(|transform| transform.to_textmate_string()).unwrap_or_default();
                append(&mut out, "${");
                append(&mut out, &variable.name);
                if self.children.borrow().is_empty() {
                    out.push_units(transform_string.units());
                } else {
                    append(&mut out, ":");
                    out.push_units(self.children_to_textmate_string().units());
                    out.push_units(transform_string.units());
                }
                append(&mut out, "}");
            }
            MarkerData::TextmateSnippet(_) => return self.children_to_textmate_string(),
        }
        out
    }

    /// `clone`: a cópia profunda. Os filhos da cópia não ganham pai (como no original); só `replace` e
    /// `appendChild` os ligam. O `clone` de `Choice` reaproveita os mesmos `Text` das opções, e portanto os
    /// reaponta para a cópia, como no original.
    pub fn deep_clone(&self) -> MarkerRef {
        let ret = match &self.data {
            MarkerData::Text(text) => return Marker::new_text(text.value.borrow().clone()),
            MarkerData::Placeholder(placeholder) => {
                let ret = Marker::new_placeholder(placeholder.index);
                if let Some(transform) = self.transform() {
                    ret.set_transform(Some(transform.deep_clone()));
                }
                ret
            }
            MarkerData::Choice(choice) => {
                let ret = Marker::new_choice();
                let options = choice.options.borrow().clone();
                for option in options {
                    ret.append_child(option);
                }
                return ret;
            }
            MarkerData::Transform(transform) => {
                let ret = Marker::new_transform();
                let (source, flags) = {
                    let regexp = transform.regexp.borrow();
                    let flags = regexp.flags();
                    let mut text = String::new();
                    if flags.ignore_case {
                        text.push('i');
                    }
                    if flags.global {
                        text.push('g');
                    }
                    (regexp.source().to_vec(), text)
                };
                let rebuilt = JsRegExp::new(&source, &flags).expect("o padrão já compilou uma vez");
                *ret.as_transform().expect("Transform").regexp.borrow_mut() = rebuilt;
                ret
            }
            MarkerData::FormatString(format_string) => {
                return Marker::new_format_string(
                    format_string.index,
                    format_string.shorthand_name.as_deref(),
                    format_string.if_value.clone(),
                    format_string.else_value.clone(),
                );
            }
            MarkerData::Variable(variable) => {
                let ret = Marker::new_variable(variable.name.clone());
                if let Some(transform) = self.transform() {
                    ret.set_transform(Some(transform.deep_clone()));
                }
                ret
            }
            MarkerData::TextmateSnippet(_) => Marker::new_textmate_snippet(),
        };
        *ret.children.borrow_mut() = self.children().iter().map(|child| child.deep_clone()).collect();
        ret
    }

    /// `Transform#_replace`: monta o resultado a partir dos grupos (`undefined` e vazio contam igual).
    fn format_groups(&self, groups: &[Option<JsString>]) -> JsString {
        let mut ret = JsString::new();
        for marker in self.children() {
            if let Some(format_string) = marker.as_format_string() {
                let group = usize::try_from(format_string.index).ok().and_then(|index| groups.get(index)).and_then(Option::as_ref);
                let value = format_string.resolve(Some(group.map_or(&[][..], JsString::units)));
                ret.push_units(value.units());
            } else {
                ret.push_units(marker.to_js_string().units());
            }
        }
        ret
    }

    /// `Transform#resolve`. Com grupos nomeados o `replace` do JavaScript entrega também o objeto `groups`
    /// depois das capturas; ele só seria lido por um `FormatString` com índice além do último grupo, o que
    /// o original trataria como objeto (nenhum snippet real faz isso).
    pub fn resolve_transform(&self, value: &[u16]) -> JsString {
        let transform = self.as_transform().expect("Transform");
        let mut did_match = false;
        let mut ret = {
            let regexp = transform.regexp.borrow();
            replace_with(&regexp, value, |found| {
                did_match = true;
                let groups: Vec<Option<JsString>> = found
                    .matched
                    .captures
                    .iter()
                    .map(|range| range.as_ref().map(|range| JsString::from(&value[range.clone()])))
                    .collect();
                self.format_groups(&groups)
            })
        };
        // when the regex didn't match and when the transform has
        // else branches, then run those
        if !did_match
            && self.children.borrow().iter().any(|child| {
                child.as_format_string().is_some_and(|format_string| is_truthy(format_string.else_value.as_ref()))
            })
        {
            ret = self.format_groups(&[]);
        }
        ret
    }

    /// `Variable#resolve`: devolve se a variável ganhou valor (e portanto filho).
    pub fn resolve_variable(self: &Rc<Self>, resolver: &dyn VariableResolver) -> bool {
        let variable = self.as_variable().expect("Variable");
        let mut value = resolver.resolve(self);
        let transform = variable.transform.borrow().clone();
        if let Some(transform) = transform {
            let input = value.unwrap_or_default();
            value = Some(transform.resolve_transform(input.units()));
        }
        if let Some(value) = value {
            // O filho não é adotado: `_children = [new Text(value)]` não mexe no pai.
            *self.children.borrow_mut() = vec![Marker::new_text(value)];
            return true;
        }
        false
    }

    // ---- TextmateSnippet ----

    /// `TextmateSnippet#placeholderInfo`.
    pub fn placeholder_info(&self) -> PlaceholderInfo {
        let snippet = self.as_snippet().expect("TextmateSnippet");
        if let Some(info) = snippet.placeholders.borrow().as_ref() {
            return info.clone();
        }
        // fill in placeholders
        let mut all: Vec<MarkerRef> = Vec::new();
        let mut last: Option<MarkerRef> = None;
        self.walk(|candidate| {
            if let Some(placeholder) = candidate.as_placeholder() {
                all.push(candidate.clone());
                let replaces = last
                    .as_ref()
                    .is_none_or(|current| current.as_placeholder().expect("Placeholder").index < placeholder.index);
                if replaces {
                    last = Some(candidate.clone());
                }
            }
            true
        });
        let info = PlaceholderInfo { all, last };
        *snippet.placeholders.borrow_mut() = Some(info.clone());
        info
    }

    /// `TextmateSnippet#placeholders`.
    pub fn placeholders(&self) -> Vec<MarkerRef> {
        self.placeholder_info().all
    }

    /// `TextmateSnippet#offset`: `None` para um marcador que não está no snippet (o `-1` do original).
    pub fn offset(&self, marker: &MarkerRef) -> Option<usize> {
        let mut pos = 0;
        let mut found = false;
        self.walk(|candidate| {
            if Rc::ptr_eq(candidate, marker) {
                found = true;
                return false;
            }
            pos += candidate.len();
            true
        });
        found.then_some(pos)
    }

    /// `TextmateSnippet#fullLen`.
    pub fn full_len(&self, marker: &MarkerRef) -> usize {
        let mut ret = 0;
        walk(std::slice::from_ref(marker), |marker| {
            ret += marker.len();
            true
        });
        ret
    }

    /// `TextmateSnippet#enclosingPlaceholders`: do mais interno para o mais externo.
    pub fn enclosing_placeholders(&self, placeholder: &MarkerRef) -> Vec<MarkerRef> {
        let mut ret = Vec::new();
        let mut parent = placeholder.parent();
        while let Some(current) = parent {
            if current.as_placeholder().is_some() {
                ret.push(current.clone());
            }
            parent = current.parent();
        }
        ret
    }

    /// `TextmateSnippet#resolveVariables`.
    pub fn resolve_variables(&self, resolver: &dyn VariableResolver) {
        self.walk(|candidate| {
            if candidate.as_variable().is_some() && candidate.resolve_variable(resolver) {
                self.invalidate_placeholders();
            }
            true
        });
    }

    /// `TextmateSnippet#walk`.
    pub fn walk(&self, visitor: impl FnMut(&MarkerRef) -> bool) {
        walk(&self.children(), visitor);
    }
}

// ---------------------------------------------------------------------------------------------
// SnippetParser
// ---------------------------------------------------------------------------------------------

/// `/^\d+$/.test(value)`.
fn is_all_digits(value: &JsString) -> bool {
    !value.is_empty() && value.units().iter().all(|&unit| Scanner::is_digit_character(unit))
}

/// `Number(value)` de um literal só de dígitos (satura em `u64::MAX`).
fn parse_number(value: &JsString) -> u64 {
    value.units().iter().fold(0u64, |acc, &unit| {
        acc.saturating_mul(10).saturating_add(u64::from(unit - cc::Digit0))
    })
}

/// Um valor opcional que o original testa com `if (value)`: a `string` vazia conta como falsa.
fn truthy(value: Option<JsString>) -> Option<JsString> {
    value.filter(|text| !text.is_empty())
}

/// `SnippetParser`.
#[derive(Debug)]
pub struct SnippetParser {
    scanner: Scanner,
    token: Token,
}

impl Default for SnippetParser {
    fn default() -> Self {
        SnippetParser::new()
    }
}

impl SnippetParser {
    pub fn new() -> SnippetParser {
        SnippetParser { scanner: Scanner::default(), token: Token { kind: TokenType::EOF, pos: 0, len: 0 } }
    }

    /// `SnippetParser.escape`: escapa `$`, `}` e `\`.
    pub fn escape(value: &[u16]) -> JsString {
        let mut out = JsString::new();
        for &unit in value {
            if unit == cc::DollarSign || unit == cc::CloseCurlyBrace || unit == cc::Backslash {
                out.push_unit(cc::Backslash);
            }
            out.push_unit(unit);
        }
        out
    }

    /// Takes a snippet and returns the insertable string, e.g return the snippet-string
    /// without any placeholder, tabstop, variables etc...
    pub fn as_insert_text(value: &[u16]) -> JsString {
        SnippetParser::new().parse(value, false, false).to_js_string()
    }

    pub fn guess_needs_clipboard(template: &[u16]) -> bool {
        CLIPBOARD.with(|regexp| regexp.exec(template).is_some())
    }

    pub fn parse(&mut self, value: &[u16], insert_final_tabstop: bool, enforce_final_tabstop: bool) -> MarkerRef {
        let snippet = Marker::new_textmate_snippet();
        self.parse_fragment(value, &snippet);
        self.ensure_final_tabstop(&snippet, enforce_final_tabstop, insert_final_tabstop);
        snippet
    }

    pub fn parse_fragment(&mut self, value: &[u16], snippet: &MarkerRef) -> Vec<MarkerRef> {
        let offset = snippet.children.borrow().len();
        self.scanner.text(value);
        self.token = self.scanner.next();
        while self.parse_marker(snippet) {
            // nothing
        }

        // fill in values for placeholders. the first placeholder of an index
        // that has a value defines the value for all placeholders with that index
        let mut placeholder_default_values: HashMap<u64, Option<Vec<MarkerRef>>> = HashMap::new();
        let mut incomplete_placeholders: Vec<MarkerRef> = Vec::new();
        snippet.walk(|marker| {
            if let Some(placeholder) = marker.as_placeholder() {
                if placeholder.is_final_tabstop() {
                    placeholder_default_values.insert(0, None);
                } else if !placeholder_default_values.contains_key(&placeholder.index) && !marker.children.borrow().is_empty() {
                    placeholder_default_values.insert(placeholder.index, Some(marker.children()));
                } else {
                    incomplete_placeholders.push(marker.clone());
                }
            }
            true
        });

        let mut stack = HashSet::new();
        for placeholder in &incomplete_placeholders {
            fill_in_incomplete_placeholder(snippet, &placeholder_default_values, placeholder, &mut stack);
        }

        snippet.children().into_iter().skip(offset).collect()
    }

    pub fn ensure_final_tabstop(&self, snippet: &MarkerRef, enforce_final_tabstop: bool, insert_final_tabstop: bool) {
        if enforce_final_tabstop || insert_final_tabstop && !snippet.placeholders().is_empty() {
            let final_tabstop = snippet
                .placeholders()
                .into_iter()
                .find(|placeholder| placeholder.as_placeholder().is_some_and(|data| data.index == 0));
            if final_tabstop.is_none() {
                // the snippet uses placeholders but has no
                // final tabstop defined -> insert at the end
                snippet.append_child(Marker::new_placeholder(0));
            }
        }
    }

    /// `_accept(type)`: consome o token se for do tipo dado.
    fn accept(&mut self, kind: TokenType) -> bool {
        if self.token.kind == kind {
            self.token = self.scanner.next();
            return true;
        }
        false
    }

    /// `_accept(type, true)`: o texto do token consumido (`None` quando não casa, ou quando o texto é
    /// vazio, o que só acontece no fim do texto: nos dois casos o original recebe um valor falso).
    /// Com `kind = None` aceita qualquer token (`type === undefined`).
    fn accept_value(&mut self, kind: Option<TokenType>) -> Option<JsString> {
        if kind.is_none_or(|kind| self.token.kind == kind) {
            let text = self.scanner.token_text(&self.token);
            self.token = self.scanner.next();
            return (!text.is_empty()).then_some(text);
        }
        None
    }

    fn back_to(&mut self, token: Token) -> bool {
        self.scanner.pos = token.pos + token.len;
        self.token = token;
        false
    }

    fn until(&mut self, kind: TokenType) -> Option<JsString> {
        let start = self.token;
        while self.token.kind != kind {
            if self.token.kind == TokenType::EOF {
                return None;
            } else if self.token.kind == TokenType::Backslash {
                let next_token = self.scanner.next();
                if next_token.kind != TokenType::Dollar
                    && next_token.kind != TokenType::CurlyClose
                    && next_token.kind != TokenType::Backslash
                {
                    return None;
                }
            }
            self.token = self.scanner.next();
        }
        // `.replace(/\\(\$|}|\\)/g, '$1')`
        let raw = &self.scanner.value[start.pos..self.token.pos];
        let mut value = JsString::new();
        let mut i = 0;
        while i < raw.len() {
            let escapes = raw[i] == cc::Backslash
                && raw.get(i + 1).is_some_and(|&next| next == cc::DollarSign || next == cc::CloseCurlyBrace || next == cc::Backslash);
            if escapes {
                i += 1;
            }
            value.push_unit(raw[i]);
            i += 1;
        }
        self.token = self.scanner.next();
        Some(value)
    }

    fn parse_marker(&mut self, marker: &MarkerRef) -> bool {
        self.parse_escaped(marker)
            || self.parse_tabstop_or_variable_name(marker)
            || self.parse_complex_placeholder(marker)
            || self.parse_complex_variable(marker)
            || self.parse_anything(marker)
    }

    // \$, \\, \} -> just text
    fn parse_escaped(&mut self, marker: &MarkerRef) -> bool {
        if let Some(backslash) = self.accept_value(Some(TokenType::Backslash)) {
            // saw a backslash, append escaped token or that backslash
            let value = self
                .accept_value(Some(TokenType::Dollar))
                .or_else(|| self.accept_value(Some(TokenType::CurlyClose)))
                .or_else(|| self.accept_value(Some(TokenType::Backslash)))
                .unwrap_or(backslash);

            marker.append_child(Marker::new_text(value));
            return true;
        }
        false
    }

    // $foo -> variable, $1 -> tabstop
    fn parse_tabstop_or_variable_name(&mut self, parent: &MarkerRef) -> bool {
        let token = self.token;
        let value = if self.accept(TokenType::Dollar) {
            self.accept_value(Some(TokenType::VariableName)).or_else(|| self.accept_value(Some(TokenType::Int)))
        } else {
            None
        };

        let Some(value) = value else {
            return self.back_to(token);
        };

        parent.append_child(if is_all_digits(&value) {
            Marker::new_placeholder(parse_number(&value))
        } else {
            Marker::new_variable(value.to_string_lossy())
        });
        true
    }

    // ${1:<children>}, ${1} -> placeholder
    fn parse_complex_placeholder(&mut self, parent: &MarkerRef) -> bool {
        let token = self.token;
        let index = if self.accept(TokenType::Dollar) && self.accept(TokenType::CurlyOpen) {
            self.accept_value(Some(TokenType::Int))
        } else {
            None
        };

        let Some(index) = index else {
            return self.back_to(token);
        };

        let placeholder = Marker::new_placeholder(parse_number(&index));
        let placeholder_index = placeholder.as_placeholder().expect("Placeholder").index;

        if self.accept(TokenType::Colon) {
            // ${1:<children>}
            loop {
                // ...} -> done
                if self.accept(TokenType::CurlyClose) {
                    parent.append_child(placeholder);
                    return true;
                }

                if self.parse_marker(&placeholder) {
                    continue;
                }

                // fallback
                let mut prefix = JsString::from("${");
                prefix.push_units(index.units());
                prefix.push_unit(cc::Colon);
                parent.append_child(Marker::new_text(prefix));
                for child in placeholder.children() {
                    parent.append_child(child);
                }
                return true;
            }
        } else if placeholder_index > 0 && self.accept(TokenType::Pipe) {
            // ${1|one,two,three|}
            let choice = Marker::new_choice();

            loop {
                if self.parse_choice_element(&choice) {
                    if self.accept(TokenType::Comma) {
                        // opt, -> more
                        continue;
                    }

                    if self.accept(TokenType::Pipe) {
                        placeholder.append_child(choice.clone());
                        if self.accept(TokenType::CurlyClose) {
                            // ..|} -> done
                            parent.append_child(placeholder);
                            return true;
                        }
                    }
                }

                return self.back_to(token);
            }
        } else if self.accept(TokenType::Forwardslash) {
            // ${1/<regex>/<format>/<options>}
            if self.parse_transform(&placeholder) {
                parent.append_child(placeholder);
                return true;
            }

            self.back_to(token)
        } else if self.accept(TokenType::CurlyClose) {
            // ${1}
            parent.append_child(placeholder);
            true
        } else {
            // ${1 <- missing curly or colon
            self.back_to(token)
        }
    }

    fn parse_choice_element(&mut self, parent: &MarkerRef) -> bool {
        let token = self.token;
        let mut values = JsString::new();
        let mut count = 0;

        loop {
            if self.token.kind == TokenType::Comma || self.token.kind == TokenType::Pipe {
                break;
            }
            let value = if let Some(backslash) = self.accept_value(Some(TokenType::Backslash)) {
                // \, \|, or \\
                self.accept_value(Some(TokenType::Comma))
                    .or_else(|| self.accept_value(Some(TokenType::Pipe)))
                    .or_else(|| self.accept_value(Some(TokenType::Backslash)))
                    .or(Some(backslash))
            } else {
                self.accept_value(None)
            };
            let Some(value) = value else {
                // EOF
                return self.back_to(token);
            };
            values.push_units(value.units());
            count += 1;
        }

        if count == 0 {
            return self.back_to(token);
        }

        parent.append_child(Marker::new_text(values));
        true
    }

    // ${foo:<children>}, ${foo} -> variable
    fn parse_complex_variable(&mut self, parent: &MarkerRef) -> bool {
        let token = self.token;
        let name = if self.accept(TokenType::Dollar) && self.accept(TokenType::CurlyOpen) {
            self.accept_value(Some(TokenType::VariableName))
        } else {
            None
        };

        let Some(name) = name else {
            return self.back_to(token);
        };

        let variable = Marker::new_variable(name.to_string_lossy());

        if self.accept(TokenType::Colon) {
            // ${foo:<children>}
            loop {
                // ...} -> done
                if self.accept(TokenType::CurlyClose) {
                    parent.append_child(variable);
                    return true;
                }

                if self.parse_marker(&variable) {
                    continue;
                }

                // fallback
                let mut prefix = JsString::from("${");
                prefix.push_units(name.units());
                prefix.push_unit(cc::Colon);
                parent.append_child(Marker::new_text(prefix));
                for child in variable.children() {
                    parent.append_child(child);
                }
                return true;
            }
        } else if self.accept(TokenType::Forwardslash) {
            // ${foo/<regex>/<format>/<options>}
            if self.parse_transform(&variable) {
                parent.append_child(variable);
                return true;
            }

            self.back_to(token)
        } else if self.accept(TokenType::CurlyClose) {
            // ${foo}
            parent.append_child(variable);
            true
        } else {
            // ${foo <- missing curly or colon
            self.back_to(token)
        }
    }

    fn parse_transform(&mut self, parent: &MarkerRef) -> bool {
        // ...<regex>/<format>/<options>}

        let transform = Marker::new_transform();
        let mut regex_value = JsString::new();
        let mut regex_options = JsString::new();

        // (1) /regex
        loop {
            if self.accept(TokenType::Forwardslash) {
                break;
            }

            if let Some(backslash) = self.accept_value(Some(TokenType::Backslash)) {
                let escaped = self.accept_value(Some(TokenType::Forwardslash)).unwrap_or(backslash);
                regex_value.push_units(escaped.units());
                continue;
            }

            if self.token.kind != TokenType::EOF {
                if let Some(text) = self.accept_value(None) {
                    regex_value.push_units(text.units());
                }
                continue;
            }
            return false;
        }

        // (2) /format
        loop {
            if self.accept(TokenType::Forwardslash) {
                break;
            }

            if let Some(backslash) = self.accept_value(Some(TokenType::Backslash)) {
                let escaped = self
                    .accept_value(Some(TokenType::Backslash))
                    .or_else(|| self.accept_value(Some(TokenType::Forwardslash)))
                    .unwrap_or(backslash);
                transform.append_child(Marker::new_text(escaped));
                continue;
            }

            if self.parse_format_string(&transform) || self.parse_anything(&transform) {
                continue;
            }
            return false;
        }

        // (3) /option
        loop {
            if self.accept(TokenType::CurlyClose) {
                break;
            }
            if self.token.kind != TokenType::EOF {
                if let Some(text) = self.accept_value(None) {
                    regex_options.push_units(text.units());
                }
                continue;
            }
            return false;
        }

        let Ok(regexp) = JsRegExp::new(regex_value.units(), &regex_options.to_string_lossy()) else {
            // invalid regexp
            return false;
        };
        *transform.as_transform().expect("Transform").regexp.borrow_mut() = regexp;

        parent.set_transform(Some(transform));
        true
    }

    fn parse_format_string(&mut self, parent: &MarkerRef) -> bool {
        let token = self.token;
        if !self.accept(TokenType::Dollar) {
            return false;
        }

        let complex = self.accept(TokenType::CurlyOpen);

        let Some(index_text) = self.accept_value(Some(TokenType::Int)) else {
            return self.back_to(token);
        };
        let index = parse_number(&index_text);

        if !complex {
            // $1
            parent.append_child(Marker::new_format_string(index, None, None, None));
            return true;
        } else if self.accept(TokenType::CurlyClose) {
            // ${1}
            parent.append_child(Marker::new_format_string(index, None, None, None));
            return true;
        } else if !self.accept(TokenType::Colon) {
            return self.back_to(token);
        }

        if self.accept(TokenType::Forwardslash) {
            // ${1:/upcase}
            if let Some(shorthand) = self.accept_value(Some(TokenType::VariableName))
                && self.accept(TokenType::CurlyClose)
            {
                parent.append_child(Marker::new_format_string(index, Some(&shorthand.to_string_lossy()), None, None));
                return true;
            }
            return self.back_to(token);
        } else if self.accept(TokenType::Plus) {
            // ${1:+<if>}
            if let Some(if_value) = truthy(self.until(TokenType::CurlyClose)) {
                parent.append_child(Marker::new_format_string(index, None, Some(if_value), None));
                return true;
            }
        } else if self.accept(TokenType::Dash) {
            // ${2:-<else>}
            if let Some(else_value) = truthy(self.until(TokenType::CurlyClose)) {
                parent.append_child(Marker::new_format_string(index, None, None, Some(else_value)));
                return true;
            }
        } else if self.accept(TokenType::QuestionMark) {
            // ${2:?<if>:<else>}
            if let Some(if_value) = truthy(self.until(TokenType::Colon))
                && let Some(else_value) = truthy(self.until(TokenType::CurlyClose))
            {
                parent.append_child(Marker::new_format_string(index, None, Some(if_value), Some(else_value)));
                return true;
            }
        } else {
            // ${1:<else>}
            if let Some(else_value) = truthy(self.until(TokenType::CurlyClose)) {
                parent.append_child(Marker::new_format_string(index, None, None, Some(else_value)));
                return true;
            }
        }

        self.back_to(token)
    }

    fn parse_anything(&mut self, marker: &MarkerRef) -> bool {
        if self.token.kind != TokenType::EOF {
            marker.append_child(Marker::new_text(self.scanner.token_text(&self.token)));
            self.token = self.scanner.next();
            return true;
        }
        false
    }
}

/// `fillInIncompletePlaceholder`: o primeiro placeholder de um índice que tem valor define o valor de todos
/// os placeholders com esse índice. `stack` evita a recursão infinita de referências cíclicas.
fn fill_in_incomplete_placeholder(
    snippet: &MarkerRef,
    placeholder_default_values: &HashMap<u64, Option<Vec<MarkerRef>>>,
    placeholder: &MarkerRef,
    stack: &mut HashSet<u64>,
) {
    let index = placeholder.as_placeholder().expect("Placeholder").index;
    let Some(Some(default_values)) = placeholder_default_values.get(&index) else {
        return;
    };
    let clone = Marker::new_placeholder(index);
    clone.set_transform(placeholder.transform());
    for child in default_values {
        let new_child = child.deep_clone();
        clone.append_child(new_child.clone());

        // "recurse" on children that are again placeholders
        if let Some(inner) = new_child.as_placeholder()
            && placeholder_default_values.contains_key(&inner.index)
            && !stack.contains(&inner.index)
        {
            stack.insert(inner.index);
            fill_in_incomplete_placeholder(snippet, placeholder_default_values, &new_child, stack);
            stack.remove(&inner.index);
        }
    }
    snippet.replace(placeholder, vec![clone]);
}

/// Para quem só quer ler `regexp` de um `Transform` sem tocar no `RefCell`.
impl TransformData {
    pub fn regexp(&self) -> Ref<'_, JsRegExp> {
        self.regexp.borrow()
    }
}
