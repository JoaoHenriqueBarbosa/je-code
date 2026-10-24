//! Porte de `vs/workbench/services/themes/common/plistParser.ts`.
//!
//! Parser de plist XML (`.tmTheme`) escrito à mão, com as mesmas mensagens de erro do original.
//! O resultado é um `serde_json::Value` (com `preserve_order`, como o objeto JS de chaves ordenadas).
//!
//! Fidelidade e diferenças:
//! - O texto é percorrido em unidades UTF-16, como o `charCodeAt`/`substr` do JS, para que os
//!   deslocamentos (`Near offset N`) e o trecho de 50 unidades do erro saiam idênticos.
//! - O parâmetro privado `locationKeyName` do original nunca é usado pela função exportada
//!   (`parse` passa `null`), então o rastreio de linha e coluna não é portado.
//! - Um `<date>` vira a string ISO que o `JSON.stringify` daria ao `Date` (`null` quando o `Date`
//!   é inválido). A leitura de data aceita o RFC 3339 e a data simples (`AAAA-MM-DD`), ambas em UTC.
//! - Uma unidade substituta solta produzida por uma entidade numérica não existe em `String`
//!   Rust e vira U+FFFD.

use std::fmt;
use std::str::FromStr;

use serde_json::{Map, Number, Value};

/// O `Error` lançado pelo parser (mensagem idêntica à do original).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlistError {
    pub message: String,
}

impl fmt::Display for PlistError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for PlistError {}

type ParseResult<T> = Result<T, PlistError>;

const BOM: u16 = 0xFEFF;
const SPACE: u16 = 32;
const TAB: u16 = 9;
const CARRIAGE_RETURN: u16 = 13;
const LINE_FEED: u16 = 10;
const SLASH: u16 = 47;
const LESS_THAN: u16 = 60;
const QUESTION_MARK: u16 = 63;
const EXCLAMATION_MARK: u16 = 33;

/// Um contêiner aberto (`State.DICT_STATE` ou `State.ARR_STATE`). `parent_key` é a chave sob a qual o
/// contêiner entra no dicionário pai quando fechar (o original o grava na entrada; a ordem é a mesma).
enum Frame {
    Dict { map: Map<String, Value>, parent_key: Option<String> },
    Array { items: Vec<Value>, parent_key: Option<String> },
}

impl Frame {
    fn into_parts(self) -> (Value, Option<String>) {
        match self {
            Frame::Dict { map, parent_key } => (Value::Object(map), parent_key),
            Frame::Array { items, parent_key } => (Value::Array(items), parent_key),
        }
    }
}

/// O estado corrente (`state`).
#[derive(Clone, Copy, PartialEq, Eq)]
enum State {
    Root,
    Dict,
    Array,
}

/// `parse`: "A very fast plist parser".
pub fn parse(content: &str) -> Result<Value, PlistError> {
    let units: Vec<u16> = content.encode_utf16().collect();
    let pos = usize::from(units.first() == Some(&BOM));
    let mut parser = Parser { content: units, pos, stack: Vec::new(), root: Value::Null, cur_key: None };
    parser.run()?;
    Ok(parser.into_result())
}

struct Parser {
    content: Vec<u16>,
    pos: usize,
    stack: Vec<Frame>,
    /// O `cur` quando nenhum contêiner está aberto.
    root: Value,
    cur_key: Option<String>,
}

fn units_of(text: &str) -> Vec<u16> {
    text.encode_utf16().collect()
}

fn is_js_whitespace(ch: char) -> bool {
    ch.is_whitespace() || ch == '\u{FEFF}'
}

/// `parseFloat`: o maior prefixo decimal válido; `None` equivale a `NaN`.
fn parse_float(text: &str) -> Option<f64> {
    let trimmed = text.trim_start_matches(is_js_whitespace);
    let bytes = trimmed.as_bytes();
    let mut i = 0;
    if i < bytes.len() && (bytes[i] == b'+' || bytes[i] == b'-') {
        i += 1;
    }
    if trimmed[i..].starts_with("Infinity") {
        return Some(if bytes.first() == Some(&b'-') { f64::NEG_INFINITY } else { f64::INFINITY });
    }
    let digits_start = i;
    while i < bytes.len() && bytes[i].is_ascii_digit() {
        i += 1;
    }
    let mut digit_count = i - digits_start;
    if i < bytes.len() && bytes[i] == b'.' {
        let mut j = i + 1;
        while j < bytes.len() && bytes[j].is_ascii_digit() {
            j += 1;
        }
        digit_count += j - (i + 1);
        if digit_count > 0 {
            i = j;
        }
    }
    if digit_count == 0 {
        return None;
    }
    if i < bytes.len() && (bytes[i] == b'e' || bytes[i] == b'E') {
        let mut j = i + 1;
        if j < bytes.len() && (bytes[j] == b'+' || bytes[j] == b'-') {
            j += 1;
        }
        let exponent_start = j;
        while j < bytes.len() && bytes[j].is_ascii_digit() {
            j += 1;
        }
        if j > exponent_start {
            i = j;
        }
    }
    let literal = trimmed[..i].trim_end_matches('.');
    let literal = literal.strip_prefix('+').unwrap_or(literal);
    literal.parse::<f64>().ok()
}

/// `parseInt(text, 10)`; `None` equivale a `NaN`.
fn parse_int(text: &str) -> Option<f64> {
    let trimmed = text.trim_start_matches(is_js_whitespace);
    let (negative, rest) = match trimmed.as_bytes().first() {
        Some(b'-') => (true, &trimmed[1..]),
        Some(b'+') => (false, &trimmed[1..]),
        _ => (false, trimmed),
    };
    let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
    if digits.is_empty() {
        return None;
    }
    let magnitude = digits.parse::<f64>().ok()?;
    Some(if negative { -magnitude } else { magnitude })
}

fn number_value(number: f64) -> Value {
    if number.fract() == 0.0 && number.abs() < 9.0e15 {
        // Todo inteiro exato de um double cabe em i64 nesta faixa.
        return Value::Number(Number::from(number as i64));
    }
    Number::from_f64(number).map_or(Value::Null, Value::Number)
}

/// `new Date(text)` seguido de `JSON.stringify`: a string ISO, ou `null` para data inválida.
fn date_value(text: &str) -> Value {
    let text = text.trim_matches(is_js_whitespace);
    let timestamp = jiff::Timestamp::from_str(text).ok().or_else(|| {
        let date = jiff::civil::Date::from_str(text).ok()?;
        date.at(0, 0, 0, 0).to_zoned(jiff::tz::TimeZone::UTC).ok().map(|zoned| zoned.timestamp())
    });
    match timestamp {
        Some(timestamp) => Value::String(timestamp.strftime("%Y-%m-%dT%H:%M:%S.%3fZ").to_string()),
        None => Value::Null,
    }
}

/// Substitui `&#N;` (decimal) ou `&#xH;` (hexadecimal minúsculo) pelo ponto de código,
/// como o `String.fromCodePoint(parseInt(...))` do original (que lança `RangeError` acima de 0x10FFFF).
fn replace_numeric(units: &[u16], hex: bool) -> ParseResult<Vec<u16>> {
    let prefix: &[u16] = if hex { &[38, 35, 120] } else { &[38, 35] };
    let radix: f64 = if hex { 16.0 } else { 10.0 };
    let mut out = Vec::with_capacity(units.len());
    let mut i = 0;
    while i < units.len() {
        if units[i..].starts_with(prefix) {
            let start = i + prefix.len();
            let mut j = start;
            let mut value = 0.0_f64;
            while j < units.len() {
                let digit = match char::from_u32(u32::from(units[j])).and_then(|ch| ch.to_digit(radix as u32)) {
                    Some(digit) if !(hex && units[j] >= 65 && units[j] <= 70) => digit,
                    _ => break,
                };
                value = value * radix + f64::from(digit);
                j += 1;
            }
            if j > start && j < units.len() && units[j] == 59 {
                if value > 1_114_111.0 {
                    return Err(PlistError { message: format!("Invalid code point {value}") });
                }
                let code_point = value as u32;
                match char::from_u32(code_point) {
                    Some(ch) => {
                        let mut buffer = [0_u16; 2];
                        out.extend_from_slice(ch.encode_utf16(&mut buffer));
                    }
                    // Unidade substituta solta: o JS a mantém, a `String` Rust não comporta.
                    None => out.push(code_point as u16),
                }
                i = j + 1;
                continue;
            }
        }
        out.push(units[i]);
        i += 1;
    }
    Ok(out)
}

/// `escapeVal`: entidades numéricas decimais, hexadecimais e as cinco nomeadas, nesta ordem.
fn escape_val(units: &[u16]) -> ParseResult<String> {
    let decimal = replace_numeric(units, false)?;
    let hexadecimal = replace_numeric(&decimal, true)?;
    let entities: [(&str, u16); 5] =
        [("&amp;", 38), ("&lt;", 60), ("&gt;", 62), ("&quot;", 34), ("&apos;", 39)];
    let named: Vec<(Vec<u16>, u16)> = entities.iter().map(|(name, ch)| (units_of(name), *ch)).collect();
    let mut out = Vec::with_capacity(hexadecimal.len());
    let mut i = 0;
    'scan: while i < hexadecimal.len() {
        for (name, replacement) in &named {
            if hexadecimal[i..].starts_with(name) {
                out.push(*replacement);
                i += name.len();
                continue 'scan;
            }
        }
        out.push(hexadecimal[i]);
        i += 1;
    }
    Ok(String::from_utf16_lossy(&out))
}

/// `IParsedTag`.
struct ParsedTag {
    name: String,
    is_closed: bool,
}

impl Parser {
    fn len(&self) -> usize {
        self.content.len()
    }

    fn state(&self) -> State {
        match self.stack.last() {
            None => State::Root,
            Some(Frame::Dict { .. }) => State::Dict,
            Some(Frame::Array { .. }) => State::Array,
        }
    }

    fn slice_text(&self, from: usize, to: usize) -> String {
        let from = from.min(self.len());
        let to = to.min(self.len()).max(from);
        String::from_utf16_lossy(&self.content[from..to])
    }

    /// `fail`: `Near offset N: msg ~~~trecho~~~`.
    fn fail<T>(&self, msg: &str) -> ParseResult<T> {
        Err(PlistError { message: format!("Near offset {}: {} ~~~{}~~~", self.pos, msg, self.slice_text(self.pos, self.pos + 50)) })
    }

    fn skip_whitespace(&mut self) {
        while self.pos < self.len() {
            let unit = self.content[self.pos];
            if unit != SPACE && unit != TAB && unit != CARRIAGE_RETURN && unit != LINE_FEED {
                break;
            }
            self.pos += 1;
        }
    }

    fn advance_if_starts_with(&mut self, text: &str) -> bool {
        let needle = units_of(text);
        let end = (self.pos + needle.len()).min(self.len());
        if self.pos <= self.len() && self.content[self.pos.min(self.len())..end] == needle[..] {
            self.pos += needle.len();
            return true;
        }
        false
    }

    fn find(&self, needle: &[u16]) -> Option<usize> {
        if self.pos > self.len() {
            return None;
        }
        self.content[self.pos..].windows(needle.len()).position(|window| window == needle).map(|offset| self.pos + offset)
    }

    fn advance_until(&mut self, text: &str) {
        let needle = units_of(text);
        self.pos = match self.find(&needle) {
            Some(next) => next + needle.len(),
            None => self.len(),
        };
    }

    /// `captureUntil`, devolvendo as unidades capturadas.
    fn capture_until(&mut self, text: &str) -> Vec<u16> {
        let needle = units_of(text);
        match self.find(&needle) {
            Some(next) => {
                let captured = self.content[self.pos..next].to_vec();
                self.pos = next + needle.len();
                captured
            }
            None => {
                let captured = self.content[self.pos.min(self.len())..].to_vec();
                self.pos = self.len();
                captured
            }
        }
    }

    fn pop_state(&mut self) -> ParseResult<()> {
        let Some(frame) = self.stack.pop() else {
            return self.fail("illegal state stack");
        };
        let (value, parent_key) = frame.into_parts();
        match self.stack.last_mut() {
            Some(Frame::Dict { map, .. }) => {
                if let Some(key) = parent_key {
                    map.insert(key, value);
                }
            }
            Some(Frame::Array { items, .. }) => items.push(value),
            None => self.root = value,
        }
        Ok(())
    }

    /// `enterDict` / `enterArray`: o contêiner novo entra no pai (chave obrigatória num dicionário).
    fn enter_container(&mut self, is_dict: bool) -> ParseResult<()> {
        let parent_key = match self.state() {
            State::Dict => match self.cur_key.take() {
                Some(key) => Some(key),
                None => return self.fail("missing <key>"),
            },
            State::Array | State::Root => None,
        };
        self.stack.push(if is_dict {
            Frame::Dict { map: Map::new(), parent_key }
        } else {
            Frame::Array { items: Vec::new(), parent_key }
        });
        Ok(())
    }

    fn leave_dict(&mut self) -> ParseResult<()> {
        if self.state() == State::Dict {
            self.pop_state()
        } else {
            self.fail("unexpected </dict>")
        }
    }

    fn leave_array(&mut self) -> ParseResult<()> {
        if self.state() == State::Array {
            self.pop_state()
        } else {
            self.fail("unexpected </array>")
        }
    }

    fn accept_key(&mut self, val: String) -> ParseResult<()> {
        match self.state() {
            State::Dict => {
                if self.cur_key.is_some() {
                    return self.fail("too many <key>");
                }
                self.cur_key = Some(val);
                Ok(())
            }
            State::Array | State::Root => self.fail("unexpected <key>"),
        }
    }

    /// `acceptString`, `acceptReal`, `acceptInteger`, `acceptDate`, `acceptData` e `acceptBool`
    /// são o mesmo corpo no original: gravar o valor no dicionário, no array ou na raiz.
    fn accept_value(&mut self, val: Value) -> ParseResult<()> {
        match self.state() {
            State::Dict => {
                let Some(key) = self.cur_key.take() else {
                    return self.fail("missing <key>");
                };
                if let Some(Frame::Dict { map, .. }) = self.stack.last_mut() {
                    map.insert(key, val);
                }
            }
            State::Array => {
                if let Some(Frame::Array { items, .. }) = self.stack.last_mut() {
                    items.push(val);
                }
            }
            State::Root => self.root = val,
        }
        Ok(())
    }

    fn parse_open_tag(&mut self) -> ParsedTag {
        let mut captured = self.capture_until(">");
        let mut is_closed = false;
        if captured.last() == Some(&SLASH) {
            is_closed = true;
            captured.pop();
        }
        ParsedTag { name: String::from_utf16_lossy(&captured).trim_matches(is_js_whitespace).to_string(), is_closed }
    }

    fn parse_tag_value(&mut self, tag: &ParsedTag) -> ParseResult<String> {
        if tag.is_closed {
            return Ok(String::new());
        }
        let value = self.capture_until("</");
        self.advance_until(">");
        escape_val(&value)
    }

    fn run(&mut self) -> ParseResult<()> {
        while self.pos < self.len() {
            self.skip_whitespace();
            if self.pos >= self.len() {
                break;
            }

            let unit = self.content[self.pos];
            self.pos += 1;
            if unit != LESS_THAN {
                return self.fail("expected <");
            }
            if self.pos >= self.len() {
                return self.fail("unexpected end of input");
            }

            let peek = self.content[self.pos];

            if peek == QUESTION_MARK {
                self.pos += 1;
                self.advance_until("?>");
                continue;
            }

            if peek == EXCLAMATION_MARK {
                self.pos += 1;
                if self.advance_if_starts_with("--") {
                    self.advance_until("-->");
                    continue;
                }
                self.advance_until(">");
                continue;
            }

            if peek == SLASH {
                self.pos += 1;
                self.skip_whitespace();

                if self.advance_if_starts_with("plist") {
                    self.advance_until(">");
                    continue;
                }
                if self.advance_if_starts_with("dict") {
                    self.advance_until(">");
                    self.leave_dict()?;
                    continue;
                }
                if self.advance_if_starts_with("array") {
                    self.advance_until(">");
                    self.leave_array()?;
                    continue;
                }
                return self.fail("unexpected closed tag");
            }

            let tag = self.parse_open_tag();

            match tag.name.as_str() {
                "dict" => {
                    self.enter_container(true)?;
                    if tag.is_closed {
                        self.leave_dict()?;
                    }
                }
                "array" => {
                    self.enter_container(false)?;
                    if tag.is_closed {
                        self.leave_array()?;
                    }
                }
                "key" => {
                    let value = self.parse_tag_value(&tag)?;
                    self.accept_key(value)?;
                }
                "string" | "data" => {
                    let value = self.parse_tag_value(&tag)?;
                    self.accept_value(Value::String(value))?;
                }
                "real" => {
                    let text = self.parse_tag_value(&tag)?;
                    let Some(number) = parse_float(&text) else {
                        return self.fail("cannot parse float");
                    };
                    self.accept_value(number_value(number))?;
                }
                "integer" => {
                    let text = self.parse_tag_value(&tag)?;
                    let Some(number) = parse_int(&text) else {
                        return self.fail("cannot parse integer");
                    };
                    self.accept_value(number_value(number))?;
                }
                "date" => {
                    let text = self.parse_tag_value(&tag)?;
                    self.accept_value(date_value(&text))?;
                }
                "true" | "false" => {
                    self.parse_tag_value(&tag)?;
                    self.accept_value(Value::Bool(tag.name == "true"))?;
                }
                name if name.starts_with("plist") => {}
                name => return self.fail(&format!("unexpected opened tag {name}")),
            }
        }
        Ok(())
    }

    /// O `cur` ao fim: o contêiner mais interno ainda aberto (entrada truncada) ou a raiz.
    fn into_result(mut self) -> Value {
        match self.stack.pop() {
            Some(frame) => frame.into_parts().0,
            None => self.root,
        }
    }
}
