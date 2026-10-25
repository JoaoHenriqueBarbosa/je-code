//! Porte de `src/vs/base/common/json.ts`: scanner e parser tolerantes de JSON com comentários
//! (o formato de `settings.json`, `keybindings.json`, `tasks.json` e dos temas).
//!
//! Deslocamentos e comprimentos são em unidades UTF-16, como no original. Onde o TypeScript usa
//! exceção para encerrar a visita cedo (`getLocation`), aqui o visitante devolve
//! [`ControlFlow::Break`].

use std::ops::ControlFlow;

use crate::JsString;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScanError {
    None = 0,
    UnexpectedEndOfComment = 1,
    UnexpectedEndOfString = 2,
    UnexpectedEndOfNumber = 3,
    InvalidUnicode = 4,
    InvalidEscapeCharacter = 5,
    InvalidCharacter = 6,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum SyntaxKind {
    OpenBraceToken = 1,
    CloseBraceToken = 2,
    OpenBracketToken = 3,
    CloseBracketToken = 4,
    CommaToken = 5,
    ColonToken = 6,
    NullKeyword = 7,
    TrueKeyword = 8,
    FalseKeyword = 9,
    StringLiteral = 10,
    NumericLiteral = 11,
    LineCommentTrivia = 12,
    BlockCommentTrivia = 13,
    LineBreakTrivia = 14,
    Trivia = 15,
    Unknown = 16,
    EOF = 17,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ParseErrorCode {
    InvalidSymbol = 1,
    InvalidNumberFormat = 2,
    PropertyNameExpected = 3,
    ValueExpected = 4,
    ColonExpected = 5,
    CommaExpected = 6,
    CloseBraceExpected = 7,
    CloseBracketExpected = 8,
    EndOfFileExpected = 9,
    InvalidCommentToken = 10,
    UnexpectedEndOfComment = 11,
    UnexpectedEndOfString = 12,
    UnexpectedEndOfNumber = 13,
    InvalidUnicode = 14,
    InvalidEscapeCharacter = 15,
    InvalidCharacter = 16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ParseError {
    pub error: ParseErrorCode,
    pub offset: usize,
    pub length: usize,
}

/// `ParseOptions`. O padrão é o `ParseOptions.DEFAULT` do original: vírgula final aceita.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ParseOptions {
    pub disallow_comments: bool,
    pub allow_trailing_comma: bool,
    pub allow_empty_content: bool,
}

impl Default for ParseOptions {
    fn default() -> Self {
        Self { disallow_comments: false, allow_trailing_comma: true, allow_empty_content: false }
    }
}

/// Valor JSON com a semântica de valor do JavaScript que o `parse` devolve.
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Null,
    Bool(bool),
    Number(f64),
    String(JsString),
    Array(Vec<Value>),
    Object(Object),
}

impl Value {
    /// A veracidade do JavaScript (`!!value`): `null`, `false`, `0`, `NaN` e `""` são falsos.
    pub fn is_truthy(&self) -> bool {
        match self {
            Value::Null => false,
            Value::Bool(b) => *b,
            Value::Number(n) => *n != 0.0 && !n.is_nan(),
            Value::String(s) => !s.is_empty(),
            Value::Array(_) | Value::Object(_) => true,
        }
    }
}

/// A veracidade do JavaScript (`!!value`) de um `serde_json::Value`.
pub fn is_serde_value_truthy(value: &serde_json::Value) -> bool {
    match value {
        serde_json::Value::Null => false,
        serde_json::Value::Bool(b) => *b,
        serde_json::Value::Number(n) => n.as_f64().is_some_and(|f| f != 0.0 && !f.is_nan()),
        serde_json::Value::String(s) => !s.is_empty(),
        serde_json::Value::Array(_) | serde_json::Value::Object(_) => true,
    }
}

/// Objeto com a ordem de propriedades do JavaScript: chaves que são índices de array
/// (inteiros canônicos abaixo de 2^32 - 1) primeiro, em ordem numérica; o resto na ordem de
/// inserção. Atribuir de novo a uma chave existente mantém a posição dela.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Object {
    entries: Vec<(JsString, Value)>,
}

fn array_index(key: &JsString) -> Option<u32> {
    let units = key.units();
    if units.is_empty() || units.len() > 10 || (units.len() > 1 && units[0] == u16::from(b'0')) {
        return None;
    }
    let mut n: u64 = 0;
    for &u in units {
        if !(u16::from(b'0')..=u16::from(b'9')).contains(&u) {
            return None;
        }
        n = n * 10 + u64::from(u - u16::from(b'0'));
    }
    u32::try_from(n).ok().filter(|&n| n != u32::MAX)
}

impl Object {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn get(&self, key: &str) -> Option<&Value> {
        self.entries.iter().find(|(k, _)| k == key).map(|(_, v)| v)
    }

    pub fn get_js(&self, key: &JsString) -> Option<&Value> {
        self.entries.iter().find(|(k, _)| k == key).map(|(_, v)| v)
    }

    pub fn contains_key(&self, key: &str) -> bool {
        self.get(key).is_some()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = (&JsString, &Value)> {
        self.entries.iter().map(|(k, v)| (k, v))
    }

    /// `obj[key] = value` (e `setObjectProperty`, que só difere no `__proto__`, aqui uma chave comum).
    pub fn insert(&mut self, key: JsString, value: Value) {
        if let Some(slot) = self.entries.iter_mut().find(|(k, _)| *k == key) {
            slot.1 = value;
            return;
        }
        let position = match array_index(&key) {
            Some(index) => self
                .entries
                .iter()
                .position(|(k, _)| array_index(k).is_none_or(|other| other > index))
                .unwrap_or(self.entries.len()),
            None => self.entries.len(),
        };
        self.entries.insert(position, (key, value));
    }
}

impl<K: Into<JsString>> FromIterator<(K, Value)> for Object {
    fn from_iter<T: IntoIterator<Item = (K, Value)>>(iter: T) -> Self {
        let mut object = Object::new();
        for (k, v) in iter {
            object.insert(k.into(), v);
        }
        object
    }
}

impl From<bool> for Value {
    fn from(b: bool) -> Self {
        Value::Bool(b)
    }
}

impl From<f64> for Value {
    fn from(n: f64) -> Self {
        Value::Number(n)
    }
}

impl From<&str> for Value {
    fn from(s: &str) -> Self {
        Value::String(s.into())
    }
}

impl From<Vec<Value>> for Value {
    fn from(v: Vec<Value>) -> Self {
        Value::Array(v)
    }
}

impl From<Object> for Value {
    fn from(o: Object) -> Self {
        Value::Object(o)
    }
}

// Códigos de caractere usados pelo scanner (o `CharacterCodes` do original).
mod cc {
    pub const LINE_FEED: u32 = 0x0A;
    pub const CARRIAGE_RETURN: u32 = 0x0D;
    pub const LINE_SEPARATOR: u32 = 0x2028;
    pub const PARAGRAPH_SEPARATOR: u32 = 0x2029;
    pub const SPACE: u32 = 0x20;
    pub const NON_BREAKING_SPACE: u32 = 0xA0;
    pub const EN_QUAD: u32 = 0x2000;
    pub const ZERO_WIDTH_SPACE: u32 = 0x200B;
    pub const NARROW_NO_BREAK_SPACE: u32 = 0x202F;
    pub const IDEOGRAPHIC_SPACE: u32 = 0x3000;
    pub const MATHEMATICAL_SPACE: u32 = 0x205F;
    pub const OGHAM: u32 = 0x1680;
    pub const DIGIT_0: u32 = 0x30;
    pub const DIGIT_9: u32 = 0x39;
    pub const A: u32 = 0x41;
    pub const E: u32 = 0x45;
    pub const F: u32 = 0x46;
    pub const LOWER_A: u32 = 0x61;
    pub const LOWER_B: u32 = 0x62;
    pub const LOWER_E: u32 = 0x65;
    pub const LOWER_F: u32 = 0x66;
    pub const LOWER_N: u32 = 0x6E;
    pub const LOWER_R: u32 = 0x72;
    pub const LOWER_T: u32 = 0x74;
    pub const LOWER_U: u32 = 0x75;
    pub const ASTERISK: u32 = 0x2A;
    pub const BACKSLASH: u32 = 0x5C;
    pub const CLOSE_BRACE: u32 = 0x7D;
    pub const CLOSE_BRACKET: u32 = 0x5D;
    pub const COLON: u32 = 0x3A;
    pub const COMMA: u32 = 0x2C;
    pub const DOT: u32 = 0x2E;
    pub const DOUBLE_QUOTE: u32 = 0x22;
    pub const MINUS: u32 = 0x2D;
    pub const OPEN_BRACE: u32 = 0x7B;
    pub const OPEN_BRACKET: u32 = 0x5B;
    pub const PLUS: u32 = 0x2B;
    pub const SLASH: u32 = 0x2F;
    pub const FORM_FEED: u32 = 0x0C;
    pub const BYTE_ORDER_MARK: u32 = 0xFEFF;
    pub const TAB: u32 = 0x09;
    pub const VERTICAL_TAB: u32 = 0x0B;
    /// O `NaN` de `charCodeAt` fora do texto: não é igual a nenhum código.
    pub const NONE: u32 = u32::MAX;
}

fn is_whitespace(ch: u32) -> bool {
    ch == cc::SPACE
        || ch == cc::TAB
        || ch == cc::VERTICAL_TAB
        || ch == cc::FORM_FEED
        || ch == cc::NON_BREAKING_SPACE
        || ch == cc::OGHAM
        || (cc::EN_QUAD..=cc::ZERO_WIDTH_SPACE).contains(&ch)
        || ch == cc::NARROW_NO_BREAK_SPACE
        || ch == cc::MATHEMATICAL_SPACE
        || ch == cc::IDEOGRAPHIC_SPACE
        || ch == cc::BYTE_ORDER_MARK
}

fn is_line_break(ch: u32) -> bool {
    ch == cc::LINE_FEED || ch == cc::CARRIAGE_RETURN || ch == cc::LINE_SEPARATOR || ch == cc::PARAGRAPH_SEPARATOR
}

fn is_digit(ch: u32) -> bool {
    (cc::DIGIT_0..=cc::DIGIT_9).contains(&ch)
}

fn is_unknown_content_character(code: u32) -> bool {
    if is_whitespace(code) || is_line_break(code) {
        return false;
    }
    !matches!(
        code,
        cc::CLOSE_BRACE
            | cc::CLOSE_BRACKET
            | cc::OPEN_BRACE
            | cc::OPEN_BRACKET
            | cc::DOUBLE_QUOTE
            | cc::COLON
            | cc::COMMA
            | cc::SLASH
    )
}

/// `createScanner`: o scanner sobre um texto UTF-16.
pub struct Scanner<'a> {
    text: &'a [u16],
    ignore_trivia: bool,
    pos: usize,
    value: JsString,
    token_offset: usize,
    token: SyntaxKind,
    scan_error: ScanError,
}

impl<'a> Scanner<'a> {
    pub fn new(text: &'a [u16], ignore_trivia: bool) -> Self {
        Self {
            text,
            ignore_trivia,
            pos: 0,
            value: JsString::new(),
            token_offset: 0,
            token: SyntaxKind::Unknown,
            scan_error: ScanError::None,
        }
    }

    fn at(&self, pos: usize) -> u32 {
        self.text.get(pos).map_or(cc::NONE, |&u| u32::from(u))
    }

    pub fn set_position(&mut self, new_position: usize) {
        self.pos = new_position;
        self.value = JsString::new();
        self.token_offset = 0;
        self.token = SyntaxKind::Unknown;
        self.scan_error = ScanError::None;
    }

    pub fn position(&self) -> usize {
        self.pos
    }

    pub fn token(&self) -> SyntaxKind {
        self.token
    }

    pub fn token_value(&self) -> &JsString {
        &self.value
    }

    pub fn token_offset(&self) -> usize {
        self.token_offset
    }

    pub fn token_length(&self) -> usize {
        self.pos - self.token_offset
    }

    pub fn token_error(&self) -> ScanError {
        self.scan_error
    }

    pub fn scan(&mut self) -> SyntaxKind {
        if self.ignore_trivia {
            loop {
                let result = self.scan_next();
                if !(SyntaxKind::LineCommentTrivia..=SyntaxKind::Trivia).contains(&result) {
                    return result;
                }
            }
        }
        self.scan_next()
    }

    fn scan_hex_digits(&mut self, count: usize) -> Option<u16> {
        let mut digits = 0;
        let mut hex_value: u32 = 0;
        while digits < count {
            let ch = self.at(self.pos);
            let digit = if is_digit(ch) {
                ch - cc::DIGIT_0
            } else if (cc::A..=cc::F).contains(&ch) {
                ch - cc::A + 10
            } else if (cc::LOWER_A..=cc::LOWER_F).contains(&ch) {
                ch - cc::LOWER_A + 10
            } else {
                break;
            };
            hex_value = hex_value * 16 + digit;
            self.pos += 1;
            digits += 1;
        }
        (digits == count).then_some(hex_value as u16)
    }

    fn substring(&self, start: usize, end: usize) -> &'a [u16] {
        &self.text[start..end]
    }

    fn scan_number(&mut self) -> &'a [u16] {
        let start = self.pos;
        if self.at(self.pos) == cc::DIGIT_0 {
            self.pos += 1;
        } else {
            self.pos += 1;
            while self.pos < self.text.len() && is_digit(self.at(self.pos)) {
                self.pos += 1;
            }
        }
        if self.pos < self.text.len() && self.at(self.pos) == cc::DOT {
            self.pos += 1;
            if self.pos < self.text.len() && is_digit(self.at(self.pos)) {
                self.pos += 1;
                while self.pos < self.text.len() && is_digit(self.at(self.pos)) {
                    self.pos += 1;
                }
            } else {
                self.scan_error = ScanError::UnexpectedEndOfNumber;
                return self.substring(start, self.pos);
            }
        }
        let mut end = self.pos;
        if self.pos < self.text.len() && (self.at(self.pos) == cc::E || self.at(self.pos) == cc::LOWER_E) {
            self.pos += 1;
            // Precedência do original: `(pos < len && ch === plus) || ch === minus`.
            if (self.pos < self.text.len() && self.at(self.pos) == cc::PLUS) || self.at(self.pos) == cc::MINUS {
                self.pos += 1;
            }
            if self.pos < self.text.len() && is_digit(self.at(self.pos)) {
                self.pos += 1;
                while self.pos < self.text.len() && is_digit(self.at(self.pos)) {
                    self.pos += 1;
                }
                end = self.pos;
            } else {
                self.scan_error = ScanError::UnexpectedEndOfNumber;
            }
        }
        self.substring(start, end)
    }

    fn scan_string(&mut self) -> JsString {
        let len = self.text.len();
        let mut result = JsString::new();
        let mut start = self.pos;
        loop {
            if self.pos >= len {
                result.push_units(self.substring(start, self.pos));
                self.scan_error = ScanError::UnexpectedEndOfString;
                break;
            }
            let ch = self.at(self.pos);
            if ch == cc::DOUBLE_QUOTE {
                result.push_units(self.substring(start, self.pos));
                self.pos += 1;
                break;
            }
            if ch == cc::BACKSLASH {
                result.push_units(self.substring(start, self.pos));
                self.pos += 1;
                if self.pos >= len {
                    self.scan_error = ScanError::UnexpectedEndOfString;
                    break;
                }
                let ch2 = self.at(self.pos);
                self.pos += 1;
                let escaped = match ch2 {
                    cc::DOUBLE_QUOTE => Some(0x22),
                    cc::BACKSLASH => Some(0x5C),
                    cc::SLASH => Some(0x2F),
                    cc::LOWER_B => Some(0x08),
                    cc::LOWER_F => Some(0x0C),
                    cc::LOWER_N => Some(0x0A),
                    cc::LOWER_R => Some(0x0D),
                    cc::LOWER_T => Some(0x09),
                    cc::LOWER_U => {
                        let unit = self.scan_hex_digits(4);
                        if unit.is_none() {
                            self.scan_error = ScanError::InvalidUnicode;
                        }
                        unit
                    }
                    _ => {
                        self.scan_error = ScanError::InvalidEscapeCharacter;
                        None
                    }
                };
                if let Some(unit) = escaped {
                    result.push_unit(unit);
                }
                start = self.pos;
                continue;
            }
            if ch <= 0x1F {
                if is_line_break(ch) {
                    result.push_units(self.substring(start, self.pos));
                    self.scan_error = ScanError::UnexpectedEndOfString;
                    break;
                }
                // Marca o erro e segue com a string.
                self.scan_error = ScanError::InvalidCharacter;
            }
            self.pos += 1;
        }
        result
    }

    fn finish(&mut self, kind: SyntaxKind) -> SyntaxKind {
        self.token = kind;
        kind
    }

    fn scan_next(&mut self) -> SyntaxKind {
        let len = self.text.len();
        self.value = JsString::new();
        self.scan_error = ScanError::None;
        self.token_offset = self.pos;

        if self.pos >= len {
            self.token_offset = len;
            return self.finish(SyntaxKind::EOF);
        }

        let mut code = self.at(self.pos);
        if is_whitespace(code) {
            loop {
                self.pos += 1;
                self.value.push_unit(code as u16);
                code = self.at(self.pos);
                if !is_whitespace(code) {
                    break;
                }
            }
            return self.finish(SyntaxKind::Trivia);
        }

        if is_line_break(code) {
            self.pos += 1;
            self.value.push_unit(code as u16);
            if code == cc::CARRIAGE_RETURN && self.at(self.pos) == cc::LINE_FEED {
                self.pos += 1;
                self.value.push_unit(0x0A);
            }
            return self.finish(SyntaxKind::LineBreakTrivia);
        }

        let single = |kind| (1, kind);
        let punctuation = match code {
            cc::OPEN_BRACE => Some(single(SyntaxKind::OpenBraceToken)),
            cc::CLOSE_BRACE => Some(single(SyntaxKind::CloseBraceToken)),
            cc::OPEN_BRACKET => Some(single(SyntaxKind::OpenBracketToken)),
            cc::CLOSE_BRACKET => Some(single(SyntaxKind::CloseBracketToken)),
            cc::COLON => Some(single(SyntaxKind::ColonToken)),
            cc::COMMA => Some(single(SyntaxKind::CommaToken)),
            _ => None,
        };
        if let Some((width, kind)) = punctuation {
            self.pos += width;
            return self.finish(kind);
        }

        match code {
            cc::DOUBLE_QUOTE => {
                self.pos += 1;
                self.value = self.scan_string();
                self.finish(SyntaxKind::StringLiteral)
            }
            cc::SLASH => {
                let start = self.pos;
                if self.at(self.pos + 1) == cc::SLASH {
                    self.pos += 2;
                    while self.pos < len && !is_line_break(self.at(self.pos)) {
                        self.pos += 1;
                    }
                    self.value = self.substring(start, self.pos).into();
                    return self.finish(SyntaxKind::LineCommentTrivia);
                }
                if self.at(self.pos + 1) == cc::ASTERISK {
                    self.pos += 2;
                    let safe_length = len - 1;
                    let mut comment_closed = false;
                    while self.pos < safe_length {
                        if self.at(self.pos) == cc::ASTERISK && self.at(self.pos + 1) == cc::SLASH {
                            self.pos += 2;
                            comment_closed = true;
                            break;
                        }
                        self.pos += 1;
                    }
                    if !comment_closed {
                        self.pos += 1;
                        self.scan_error = ScanError::UnexpectedEndOfComment;
                    }
                    // O `pos` pode passar do fim em um: o `substring` do JavaScript corta no fim.
                    self.value = self.substring(start, self.pos.min(len)).into();
                    return self.finish(SyntaxKind::BlockCommentTrivia);
                }
                self.value.push_unit(code as u16);
                self.pos += 1;
                self.finish(SyntaxKind::Unknown)
            }
            cc::MINUS => {
                self.value.push_unit(code as u16);
                self.pos += 1;
                if self.pos == len || !is_digit(self.at(self.pos)) {
                    return self.finish(SyntaxKind::Unknown);
                }
                let number = self.scan_number();
                self.value.push_units(number);
                self.finish(SyntaxKind::NumericLiteral)
            }
            c if is_digit(c) => {
                let number = self.scan_number();
                self.value.push_units(number);
                self.finish(SyntaxKind::NumericLiteral)
            }
            _ => {
                while self.pos < len && is_unknown_content_character(code) {
                    self.pos += 1;
                    code = self.at(self.pos);
                }
                if self.token_offset != self.pos {
                    self.value = self.substring(self.token_offset, self.pos).into();
                    let kind = if self.value == "true" {
                        SyntaxKind::TrueKeyword
                    } else if self.value == "false" {
                        SyntaxKind::FalseKeyword
                    } else if self.value == "null" {
                        SyntaxKind::NullKeyword
                    } else {
                        SyntaxKind::Unknown
                    };
                    return self.finish(kind);
                }
                self.value.push_unit(code as u16);
                self.pos += 1;
                self.finish(SyntaxKind::Unknown)
            }
        }
    }

    /// O `pos` só passa do fim no comentário de bloco não fechado; o comprimento do token também
    /// conta essa unidade, como no original (`pos - tokenOffset`).
    pub fn token_end(&self) -> usize {
        self.pos
    }
}

/// O `JSONVisitor`. Cada método recebe deslocamento e comprimento do token em unidades UTF-16 e
/// pode devolver `Break` para encerrar a visita.
pub trait Visitor {
    fn on_object_begin(&mut self, _offset: usize, _length: usize) -> ControlFlow<()> {
        ControlFlow::Continue(())
    }
    fn on_object_property(&mut self, _property: &JsString, _offset: usize, _length: usize) -> ControlFlow<()> {
        ControlFlow::Continue(())
    }
    fn on_object_end(&mut self, _offset: usize, _length: usize) -> ControlFlow<()> {
        ControlFlow::Continue(())
    }
    fn on_array_begin(&mut self, _offset: usize, _length: usize) -> ControlFlow<()> {
        ControlFlow::Continue(())
    }
    fn on_array_end(&mut self, _offset: usize, _length: usize) -> ControlFlow<()> {
        ControlFlow::Continue(())
    }
    fn on_literal_value(&mut self, _value: &Value, _offset: usize, _length: usize) -> ControlFlow<()> {
        ControlFlow::Continue(())
    }
    fn on_separator(&mut self, _character: char, _offset: usize, _length: usize) -> ControlFlow<()> {
        ControlFlow::Continue(())
    }
    fn on_comment(&mut self, _offset: usize, _length: usize) -> ControlFlow<()> {
        ControlFlow::Continue(())
    }
    fn on_error(&mut self, _error: ParseErrorCode, _offset: usize, _length: usize) -> ControlFlow<()> {
        ControlFlow::Continue(())
    }
}

/// `JSON.parse` de um literal numérico já recortado pelo scanner: só a gramática de número do JSON
/// é aceita (o recorte de `1.` ou `1e` falha, como lá).
fn parse_json_number(units: &[u16]) -> Option<f64> {
    let text = String::from_utf16(units).ok()?;
    let bytes = text.as_bytes();
    let mut i = 0;
    if bytes.get(i) == Some(&b'-') {
        i += 1;
    }
    match bytes.get(i) {
        Some(b'0') => i += 1,
        Some(b'1'..=b'9') => {
            while bytes.get(i).is_some_and(u8::is_ascii_digit) {
                i += 1;
            }
        }
        _ => return None,
    }
    if bytes.get(i) == Some(&b'.') {
        i += 1;
        let digits = bytes[i..].iter().take_while(|b| b.is_ascii_digit()).count();
        if digits == 0 {
            return None;
        }
        i += digits;
    }
    if matches!(bytes.get(i), Some(b'e' | b'E')) {
        i += 1;
        if matches!(bytes.get(i), Some(b'+' | b'-')) {
            i += 1;
        }
        let digits = bytes[i..].iter().take_while(|b| b.is_ascii_digit()).count();
        if digits == 0 {
            return None;
        }
        i += digits;
    }
    if i != bytes.len() {
        return None;
    }
    text.parse().ok()
}

struct Parser<'v, 't, V: Visitor> {
    scanner: Scanner<'t>,
    visitor: &'v mut V,
    options: ParseOptions,
}

/// Resultado interno: `Break` propaga o encerramento pedido pelo visitante.
type Step<T> = ControlFlow<(), T>;

impl<V: Visitor> Parser<'_, '_, V> {
    fn at_token(&self) -> (usize, usize) {
        (self.scanner.token_offset(), self.scanner.token_length())
    }

    fn on_error(&mut self, error: ParseErrorCode) -> Step<()> {
        let (o, l) = self.at_token();
        self.visitor.on_error(error, o, l)
    }

    fn scan_next(&mut self) -> Step<SyntaxKind> {
        loop {
            let token = self.scanner.scan();
            let scan_error = match self.scanner.token_error() {
                ScanError::InvalidUnicode => Some(ParseErrorCode::InvalidUnicode),
                ScanError::InvalidEscapeCharacter => Some(ParseErrorCode::InvalidEscapeCharacter),
                ScanError::UnexpectedEndOfNumber => Some(ParseErrorCode::UnexpectedEndOfNumber),
                ScanError::UnexpectedEndOfComment if !self.options.disallow_comments => {
                    Some(ParseErrorCode::UnexpectedEndOfComment)
                }
                ScanError::UnexpectedEndOfString => Some(ParseErrorCode::UnexpectedEndOfString),
                ScanError::InvalidCharacter => Some(ParseErrorCode::InvalidCharacter),
                _ => None,
            };
            if let Some(error) = scan_error {
                self.on_error(error)?;
            }
            match token {
                SyntaxKind::LineCommentTrivia | SyntaxKind::BlockCommentTrivia => {
                    if self.options.disallow_comments {
                        self.on_error(ParseErrorCode::InvalidCommentToken)?;
                    } else {
                        let (o, l) = self.at_token();
                        self.visitor.on_comment(o, l)?;
                    }
                }
                SyntaxKind::Unknown => self.on_error(ParseErrorCode::InvalidSymbol)?,
                SyntaxKind::Trivia | SyntaxKind::LineBreakTrivia => {}
                other => return ControlFlow::Continue(other),
            }
        }
    }

    fn handle_error(&mut self, error: ParseErrorCode, skip_until_after: &[SyntaxKind], skip_until: &[SyntaxKind]) -> Step<()> {
        self.on_error(error)?;
        if !skip_until_after.is_empty() || !skip_until.is_empty() {
            let mut token = self.scanner.token();
            while token != SyntaxKind::EOF {
                if skip_until_after.contains(&token) {
                    self.scan_next()?;
                    break;
                } else if skip_until.contains(&token) {
                    break;
                }
                token = self.scan_next()?;
            }
        }
        ControlFlow::Continue(())
    }

    fn parse_string(&mut self, is_value: bool) -> Step<bool> {
        let value = self.scanner.token_value().clone();
        let (o, l) = self.at_token();
        if is_value {
            self.visitor.on_literal_value(&Value::String(value), o, l)?;
        } else {
            self.visitor.on_object_property(&value, o, l)?;
        }
        self.scan_next()?;
        ControlFlow::Continue(true)
    }

    fn parse_literal(&mut self) -> Step<bool> {
        let value = match self.scanner.token() {
            SyntaxKind::NumericLiteral => match parse_json_number(self.scanner.token_value().units()) {
                Some(n) => Value::Number(n),
                None => {
                    self.handle_error(ParseErrorCode::InvalidNumberFormat, &[], &[])?;
                    Value::Number(0.0)
                }
            },
            SyntaxKind::NullKeyword => Value::Null,
            SyntaxKind::TrueKeyword => Value::Bool(true),
            SyntaxKind::FalseKeyword => Value::Bool(false),
            _ => return ControlFlow::Continue(false),
        };
        let (o, l) = self.at_token();
        self.visitor.on_literal_value(&value, o, l)?;
        self.scan_next()?;
        ControlFlow::Continue(true)
    }

    fn parse_property(&mut self) -> Step<bool> {
        const RECOVER: [SyntaxKind; 2] = [SyntaxKind::CloseBraceToken, SyntaxKind::CommaToken];
        if self.scanner.token() != SyntaxKind::StringLiteral {
            self.handle_error(ParseErrorCode::PropertyNameExpected, &[], &RECOVER)?;
            return ControlFlow::Continue(false);
        }
        self.parse_string(false)?;
        if self.scanner.token() == SyntaxKind::ColonToken {
            let (o, l) = self.at_token();
            self.visitor.on_separator(':', o, l)?;
            self.scan_next()?;
            if !self.parse_value()? {
                self.handle_error(ParseErrorCode::ValueExpected, &[], &RECOVER)?;
            }
        } else {
            self.handle_error(ParseErrorCode::ColonExpected, &[], &RECOVER)?;
        }
        ControlFlow::Continue(true)
    }

    /// Objeto e array compartilham a forma: abre, itens separados por vírgula, fecha.
    fn parse_container(&mut self, object: bool) -> Step<bool> {
        let (close, close_error) = if object {
            (SyntaxKind::CloseBraceToken, ParseErrorCode::CloseBraceExpected)
        } else {
            (SyntaxKind::CloseBracketToken, ParseErrorCode::CloseBracketExpected)
        };
        let (o, l) = self.at_token();
        if object {
            self.visitor.on_object_begin(o, l)?;
        } else {
            self.visitor.on_array_begin(o, l)?;
        }
        self.scan_next()?;

        let mut needs_comma = false;
        while self.scanner.token() != close && self.scanner.token() != SyntaxKind::EOF {
            if self.scanner.token() == SyntaxKind::CommaToken {
                if !needs_comma {
                    self.handle_error(ParseErrorCode::ValueExpected, &[], &[])?;
                }
                let (o, l) = self.at_token();
                self.visitor.on_separator(',', o, l)?;
                self.scan_next()?;
                if self.scanner.token() == close && self.options.allow_trailing_comma {
                    break;
                }
            } else if needs_comma {
                self.handle_error(ParseErrorCode::CommaExpected, &[], &[])?;
            }
            let parsed = if object { self.parse_property()? } else { self.parse_value()? };
            if !parsed {
                self.handle_error(ParseErrorCode::ValueExpected, &[], &[close, SyntaxKind::CommaToken])?;
            }
            needs_comma = true;
        }
        let (o, l) = self.at_token();
        if object {
            self.visitor.on_object_end(o, l)?;
        } else {
            self.visitor.on_array_end(o, l)?;
        }
        if self.scanner.token() != close {
            self.handle_error(close_error, &[close], &[])?;
        } else {
            self.scan_next()?;
        }
        ControlFlow::Continue(true)
    }

    fn parse_value(&mut self) -> Step<bool> {
        match self.scanner.token() {
            SyntaxKind::OpenBracketToken => self.parse_container(false),
            SyntaxKind::OpenBraceToken => self.parse_container(true),
            SyntaxKind::StringLiteral => self.parse_string(true),
            _ => self.parse_literal(),
        }
    }

    fn run(&mut self) -> Step<bool> {
        self.scan_next()?;
        if self.scanner.token() == SyntaxKind::EOF {
            if self.options.allow_empty_content {
                return ControlFlow::Continue(true);
            }
            self.handle_error(ParseErrorCode::ValueExpected, &[], &[])?;
            return ControlFlow::Continue(false);
        }
        if !self.parse_value()? {
            self.handle_error(ParseErrorCode::ValueExpected, &[], &[])?;
            return ControlFlow::Continue(false);
        }
        if self.scanner.token() != SyntaxKind::EOF {
            self.handle_error(ParseErrorCode::EndOfFileExpected, &[], &[])?;
        }
        ControlFlow::Continue(true)
    }
}

/// `visit(text, visitor, options)`. Devolve `false` quando o texto não tem valor, e `true` também
/// quando o visitante encerrou a visita antes do fim.
pub fn visit(text: &[u16], visitor: &mut impl Visitor, options: ParseOptions) -> bool {
    let mut parser = Parser { scanner: Scanner::new(text, false), visitor, options };
    match parser.run() {
        ControlFlow::Continue(ok) => ok,
        ControlFlow::Break(()) => true,
    }
}

/// Construtor de valor do `parse`.
#[derive(Default)]
struct ValueBuilder {
    stack: Vec<(Value, Option<JsString>)>,
    root: Vec<Value>,
    property: Option<JsString>,
    errors: Vec<ParseError>,
}

impl ValueBuilder {
    fn on_value(&mut self, value: Value) {
        match self.stack.last_mut() {
            None => self.root.push(value),
            Some((Value::Array(items), _)) => items.push(value),
            Some((Value::Object(object), _)) => {
                if let Some(key) = &self.property {
                    object.insert(key.clone(), value);
                }
            }
            Some(_) => {}
        }
    }

    fn begin(&mut self, container: Value) {
        self.stack.push((container, self.property.take()));
    }

    fn end(&mut self) {
        if let Some((container, property)) = self.stack.pop() {
            self.property = property;
            self.on_value(container);
        }
    }
}

impl Visitor for ValueBuilder {
    fn on_object_begin(&mut self, _: usize, _: usize) -> ControlFlow<()> {
        self.begin(Value::Object(Object::new()));
        ControlFlow::Continue(())
    }
    fn on_object_property(&mut self, property: &JsString, _: usize, _: usize) -> ControlFlow<()> {
        self.property = Some(property.clone());
        ControlFlow::Continue(())
    }
    fn on_object_end(&mut self, _: usize, _: usize) -> ControlFlow<()> {
        self.end();
        ControlFlow::Continue(())
    }
    fn on_array_begin(&mut self, _: usize, _: usize) -> ControlFlow<()> {
        self.begin(Value::Array(Vec::new()));
        ControlFlow::Continue(())
    }
    fn on_array_end(&mut self, _: usize, _: usize) -> ControlFlow<()> {
        self.end();
        ControlFlow::Continue(())
    }
    fn on_literal_value(&mut self, value: &Value, _: usize, _: usize) -> ControlFlow<()> {
        self.on_value(value.clone());
        ControlFlow::Continue(())
    }
    fn on_error(&mut self, error: ParseErrorCode, offset: usize, length: usize) -> ControlFlow<()> {
        self.errors.push(ParseError { error, offset, length });
        ControlFlow::Continue(())
    }
}

/// `parse(text, errors, options)`: o valor, ou `None` onde o original devolve `undefined`.
///
/// No original, um objeto ainda aberto no fim do texto já está pendurado no pai desde o
/// `onObjectBegin`; aqui o pai só o recebe no fechamento, então os contêineres que ficaram abertos
/// são fechados ao fim para dar o mesmo resultado.
pub fn parse(text: &[u16], errors: &mut Vec<ParseError>, options: ParseOptions) -> Option<Value> {
    let mut builder = ValueBuilder::default();
    visit(text, &mut builder, options);
    while !builder.stack.is_empty() {
        builder.end();
    }
    errors.append(&mut builder.errors);
    builder.root.into_iter().next()
}

/// `parse` sobre um `&str`, com as opções padrão.
pub fn parse_str(text: &str, errors: &mut Vec<ParseError>) -> Option<Value> {
    let units: Vec<u16> = text.encode_utf16().collect();
    parse(&units, errors, ParseOptions::default())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NodeType {
    Object,
    Array,
    Property,
    String,
    Number,
    Boolean,
    Null,
}

/// `getNodeType`.
pub fn node_type(value: &Value) -> NodeType {
    match value {
        Value::Bool(_) => NodeType::Boolean,
        Value::Number(_) => NodeType::Number,
        Value::String(_) => NodeType::String,
        Value::Array(_) => NodeType::Array,
        Value::Object(_) => NodeType::Object,
        Value::Null => NodeType::Null,
    }
}

pub type NodeId = usize;

/// Um nó da árvore do `parseTree`. `children` é `None` nos literais (no original o campo não existe).
#[derive(Clone, Debug, PartialEq)]
pub struct Node {
    pub kind: NodeType,
    pub value: Option<Value>,
    pub offset: i64,
    pub length: i64,
    pub colon_offset: Option<i64>,
    pub parent: Option<NodeId>,
    pub children: Option<Vec<NodeId>>,
}

/// A árvore do `parseTree`, com os nós numa arena; o pai de cada nó é um índice.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Tree {
    nodes: Vec<Node>,
    root: Option<NodeId>,
}

impl Tree {
    pub fn root(&self) -> Option<NodeId> {
        self.root
    }

    pub fn node(&self, id: NodeId) -> &Node {
        &self.nodes[id]
    }

    fn children(&self, id: NodeId) -> &[NodeId] {
        self.nodes[id].children.as_deref().unwrap_or_default()
    }

    /// `findNodeAtLocation`.
    pub fn find_node_at_location(&self, path: &[Segment]) -> Option<NodeId> {
        let mut node = self.root?;
        for segment in path {
            match segment {
                Segment::Property(name) => {
                    if self.nodes[node].kind != NodeType::Object {
                        return None;
                    }
                    let property = self.children(node).iter().copied().find(|&p| {
                        self.children(p).first().is_some_and(|&k| self.nodes[k].value.as_ref() == Some(&Value::String(name.clone())))
                    })?;
                    node = *self.children(property).get(1)?;
                }
                Segment::Index(index) => {
                    if self.nodes[node].kind != NodeType::Array {
                        return None;
                    }
                    node = *self.children(node).get(*index)?;
                }
            }
        }
        Some(node)
    }

    /// `getNodePath`.
    pub fn node_path(&self, node: NodeId) -> Vec<Segment> {
        let Some(parent) = self.nodes[node].parent else {
            return Vec::new();
        };
        if self.nodes[parent].children.is_none() {
            return Vec::new();
        }
        let mut path = self.node_path(parent);
        match self.nodes[parent].kind {
            NodeType::Property => {
                if let Some(Value::String(key)) = self.children(parent).first().and_then(|&k| self.nodes[k].value.as_ref()) {
                    path.push(Segment::Property(key.clone()));
                }
            }
            NodeType::Array => {
                if let Some(index) = self.children(parent).iter().position(|&c| c == node) {
                    path.push(Segment::Index(index));
                }
            }
            _ => {}
        }
        path
    }

    /// `getNodeValue`.
    pub fn node_value(&self, node: NodeId) -> Option<Value> {
        let n = &self.nodes[node];
        match n.kind {
            NodeType::Array => Some(Value::Array(self.children(node).iter().filter_map(|&c| self.node_value(c)).collect())),
            NodeType::Object => {
                let mut object = Object::new();
                for &property in self.children(node) {
                    let children = self.children(property);
                    if let (Some(&key), Some(&value)) = (children.first(), children.get(1))
                        && let (Some(Value::String(name)), Some(value)) = (&self.nodes[key].value, self.node_value(value))
                    {
                        object.insert(name.clone(), value);
                    }
                }
                Some(Value::Object(object))
            }
            NodeType::Null | NodeType::String | NodeType::Number | NodeType::Boolean => n.value.clone(),
            NodeType::Property => None,
        }
    }

    /// `contains`.
    pub fn contains(&self, node: NodeId, offset: i64, include_right_bound: bool) -> bool {
        let n = &self.nodes[node];
        (offset >= n.offset && offset < n.offset + n.length) || (include_right_bound && offset == n.offset + n.length)
    }

    /// `findNodeAtOffset`.
    pub fn find_node_at_offset(&self, node: NodeId, offset: i64, include_right_bound: bool) -> Option<NodeId> {
        if !self.contains(node, offset, include_right_bound) {
            return None;
        }
        for &child in self.children(node) {
            if self.nodes[child].offset > offset {
                break;
            }
            if let Some(found) = self.find_node_at_offset(child, offset, include_right_bound) {
                return Some(found);
            }
        }
        Some(node)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Segment {
    Property(JsString),
    Index(usize),
}

struct TreeBuilder {
    tree: Tree,
    current: NodeId,
    errors: Vec<ParseError>,
}

impl TreeBuilder {
    fn new() -> Self {
        // Raiz artificial, como no original.
        let root = Node { kind: NodeType::Array, value: None, offset: -1, length: -1, colon_offset: None, parent: None, children: Some(Vec::new()) };
        Self { tree: Tree { nodes: vec![root], root: None }, current: 0, errors: Vec::new() }
    }

    fn push(&mut self, kind: NodeType, value: Option<Value>, offset: usize, length: i64, container: bool) -> NodeId {
        let id = self.tree.nodes.len();
        self.tree.nodes.push(Node {
            kind,
            value,
            offset: offset as i64,
            length,
            colon_offset: None,
            parent: Some(self.current),
            children: container.then(Vec::new),
        });
        if let Some(children) = self.tree.nodes[self.current].children.as_mut() {
            children.push(id);
        }
        id
    }

    fn ensure_property_complete(&mut self, end_offset: usize) {
        if self.tree.nodes[self.current].kind == NodeType::Property {
            let node = &mut self.tree.nodes[self.current];
            node.length = end_offset as i64 - node.offset;
            self.current = node.parent.unwrap_or(0);
        }
    }

    fn close(&mut self, offset: usize, length: usize) {
        let node = &mut self.tree.nodes[self.current];
        node.length = (offset + length) as i64 - node.offset;
        self.current = node.parent.unwrap_or(0);
        self.ensure_property_complete(offset + length);
    }
}

impl Visitor for TreeBuilder {
    fn on_object_begin(&mut self, offset: usize, _: usize) -> ControlFlow<()> {
        self.current = self.push(NodeType::Object, None, offset, -1, true);
        ControlFlow::Continue(())
    }
    fn on_object_property(&mut self, name: &JsString, offset: usize, length: usize) -> ControlFlow<()> {
        self.current = self.push(NodeType::Property, None, offset, -1, true);
        self.push(NodeType::String, Some(Value::String(name.clone())), offset, length as i64, false);
        ControlFlow::Continue(())
    }
    fn on_object_end(&mut self, offset: usize, length: usize) -> ControlFlow<()> {
        self.close(offset, length);
        ControlFlow::Continue(())
    }
    fn on_array_begin(&mut self, offset: usize, _: usize) -> ControlFlow<()> {
        self.current = self.push(NodeType::Array, None, offset, -1, true);
        ControlFlow::Continue(())
    }
    fn on_array_end(&mut self, offset: usize, length: usize) -> ControlFlow<()> {
        self.close(offset, length);
        ControlFlow::Continue(())
    }
    fn on_literal_value(&mut self, value: &Value, offset: usize, length: usize) -> ControlFlow<()> {
        self.push(node_type(value), Some(value.clone()), offset, length as i64, false);
        self.ensure_property_complete(offset + length);
        ControlFlow::Continue(())
    }
    fn on_separator(&mut self, separator: char, offset: usize, _: usize) -> ControlFlow<()> {
        if self.tree.nodes[self.current].kind == NodeType::Property {
            if separator == ':' {
                self.tree.nodes[self.current].colon_offset = Some(offset as i64);
            } else if separator == ',' {
                self.ensure_property_complete(offset);
            }
        }
        ControlFlow::Continue(())
    }
    fn on_error(&mut self, error: ParseErrorCode, offset: usize, length: usize) -> ControlFlow<()> {
        self.errors.push(ParseError { error, offset, length });
        ControlFlow::Continue(())
    }
}

/// `parseTree(text, errors, options)`. Deslocamentos dos nós são `i64` porque o original usa -1
/// (raiz artificial e contêiner que nunca fechou) e o `contains` depende disso.
pub fn parse_tree(text: &[u16], errors: &mut Vec<ParseError>, options: ParseOptions) -> Tree {
    let mut builder = TreeBuilder::new();
    visit(text, &mut builder, options);
    errors.append(&mut builder.errors);
    let mut tree = builder.tree;
    tree.root = tree.nodes[0].children.as_ref().and_then(|c| c.first().copied());
    if let Some(root) = tree.root {
        tree.nodes[root].parent = None;
    }
    tree
}
