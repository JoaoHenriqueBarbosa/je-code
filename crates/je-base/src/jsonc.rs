//! Porte de `src/vs/base/common/jsonc.ts`: `JSON.parse` que aceita comentários e vírgulas finais.
//!
//! As duas regexes do original rodam no mesmo motor ECMAScript (`zyarr`), então o que casa é igual.
//! Como não existe `JSON.parse` no `je-base`, o módulo traz [`json_parse`], um parser estrito da
//! gramática JSON do ECMAScript (o que o `JSON.parse` aceita, sem extensões).

use std::fmt;

use zyarr::RegExp;

use crate::JsString;
use crate::js_regexp::replace_all;
use crate::json::{Object, Value};

/// Grupo 1: string entre aspas duplas. 2: entre aspas simples. 3: comentário de bloco. 4: comentário de
/// linha. 5: vírgula final.
const COMMENTS_REGEXP: &str = r#"("[^"\\]*(?:\\.[^"\\]*)*")|('[^'\\]*(?:\\.[^'\\]*)*')|(\/\*[^\/\*]*(?:(?:\*|\/)[^\/\*]*)*?\*\/)|(\/{2,}.*?(?:(?:\r?\n)|$))|(,\s*[}\]])"#;

thread_local! {
    static COMMENTS: RegExp = compile(COMMENTS_REGEXP);
    static TRAILING_COMMAS: RegExp = compile(r",\s*([}\]])");
}

fn compile(source: &str) -> RegExp {
    let pattern: Vec<u16> = source.encode_utf16().collect();
    RegExp::new(&pattern, &[u16::from(b'g')]).expect("regex fixa e válida")
}

/// `stripComments`: tira comentários de linha e de bloco e vírgulas finais, sem mexer em strings.
pub fn strip_comments(content: &[u16]) -> JsString {
    COMMENTS.with(|regexp| {
        replace_all(regexp, content, |m| {
            // Só um dos grupos casa.
            if m.group(3).is_some_and(|g| !g.is_empty()) {
                // Comentário de bloco: some.
                JsString::default()
            } else if let Some(line_comment) = m.group(4).filter(|g| !g.is_empty()) {
                // Comentário de linha (pelo menos `//`): se termina em `\r?\n`, a quebra fica.
                match line_comment.last() {
                    Some(0x0A) => {
                        if line_comment.len() >= 2 && line_comment[line_comment.len() - 2] == 0x0D {
                            JsString::from("\r\n")
                        } else {
                            JsString::from("\n")
                        }
                    }
                    _ => JsString::default(),
                }
            } else if m.group(5).is_some_and(|g| !g.is_empty()) {
                // Tira a vírgula final.
                JsString::from(&m.whole()[1..])
            } else {
                // Uma string.
                JsString::from(m.whole())
            }
        })
    })
}

/// `parse`: substituto do `JSON.parse` que aceita comentários e vírgulas finais.
pub fn parse(content: &[u16]) -> Result<Value, JsonSyntaxError> {
    let comments_stripped = strip_comments(content);
    match json_parse(comments_stripped.units()) {
        Ok(value) => Ok(value),
        Err(_) => {
            let without_trailing_commas = TRAILING_COMMAS.with(|regexp| {
                replace_all(regexp, comments_stripped.units(), |m| JsString::from(m.group(1).unwrap_or_default()))
            });
            json_parse(without_trailing_commas.units())
        }
    }
}

/// O `SyntaxError` do `JSON.parse`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JsonSyntaxError {
    pub message: String,
    /// Posição (unidade UTF-16) onde a leitura parou.
    pub position: usize,
}

impl fmt::Display for JsonSyntaxError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} (position {})", self.message, self.position)
    }
}

impl std::error::Error for JsonSyntaxError {}

const MAX_DEPTH: usize = 2000;

/// `JSON.parse(text)` sem reviver: a gramática JSON do ECMAScript, estrita.
pub fn json_parse(text: &[u16]) -> Result<Value, JsonSyntaxError> {
    let mut parser = StrictParser { text, pos: 0 };
    parser.skip_whitespace();
    let value = parser.value(0)?;
    parser.skip_whitespace();
    if parser.pos < text.len() {
        return Err(parser.error("Unexpected non-whitespace character after JSON"));
    }
    Ok(value)
}

struct StrictParser<'a> {
    text: &'a [u16],
    pos: usize,
}

impl StrictParser<'_> {
    fn error(&self, message: &str) -> JsonSyntaxError {
        JsonSyntaxError { message: message.to_string(), position: self.pos }
    }

    fn peek(&self) -> Option<u16> {
        self.text.get(self.pos).copied()
    }

    fn skip_whitespace(&mut self) {
        while matches!(self.peek(), Some(0x20 | 0x09 | 0x0A | 0x0D)) {
            self.pos += 1;
        }
    }

    fn expect_literal(&mut self, literal: &str) -> Result<(), JsonSyntaxError> {
        for unit in literal.encode_utf16() {
            if self.peek() != Some(unit) {
                return Err(self.error("Unexpected token in JSON"));
            }
            self.pos += 1;
        }
        Ok(())
    }

    fn value(&mut self, depth: usize) -> Result<Value, JsonSyntaxError> {
        if depth > MAX_DEPTH {
            return Err(self.error("JSON nested too deeply"));
        }
        match self.peek() {
            None => Err(self.error("Unexpected end of JSON input")),
            Some(0x7B) => self.object(depth),
            Some(0x5B) => self.array(depth),
            Some(0x22) => Ok(Value::String(self.string()?)),
            Some(0x74) => self.expect_literal("true").map(|()| Value::Bool(true)),
            Some(0x66) => self.expect_literal("false").map(|()| Value::Bool(false)),
            Some(0x6E) => self.expect_literal("null").map(|()| Value::Null),
            Some(0x2D | 0x30..=0x39) => self.number(),
            Some(_) => Err(self.error("Unexpected token in JSON")),
        }
    }

    fn object(&mut self, depth: usize) -> Result<Value, JsonSyntaxError> {
        self.pos += 1;
        let mut object = Object::new();
        self.skip_whitespace();
        if self.peek() == Some(0x7D) {
            self.pos += 1;
            return Ok(Value::Object(object));
        }
        loop {
            self.skip_whitespace();
            if self.peek() != Some(0x22) {
                return Err(self.error("Expected double-quoted property name in JSON"));
            }
            let key = self.string()?;
            self.skip_whitespace();
            if self.peek() != Some(0x3A) {
                return Err(self.error("Expected ':' after property name in JSON"));
            }
            self.pos += 1;
            self.skip_whitespace();
            let value = self.value(depth + 1)?;
            object.insert(key, value);
            self.skip_whitespace();
            match self.peek() {
                Some(0x2C) => self.pos += 1,
                Some(0x7D) => {
                    self.pos += 1;
                    return Ok(Value::Object(object));
                }
                _ => return Err(self.error("Expected ',' or '}' after property value in JSON")),
            }
        }
    }

    fn array(&mut self, depth: usize) -> Result<Value, JsonSyntaxError> {
        self.pos += 1;
        let mut items = Vec::new();
        self.skip_whitespace();
        if self.peek() == Some(0x5D) {
            self.pos += 1;
            return Ok(Value::Array(items));
        }
        loop {
            self.skip_whitespace();
            items.push(self.value(depth + 1)?);
            self.skip_whitespace();
            match self.peek() {
                Some(0x2C) => self.pos += 1,
                Some(0x5D) => {
                    self.pos += 1;
                    return Ok(Value::Array(items));
                }
                _ => return Err(self.error("Expected ',' or ']' after array element in JSON")),
            }
        }
    }

    fn string(&mut self) -> Result<JsString, JsonSyntaxError> {
        self.pos += 1;
        let mut out: Vec<u16> = Vec::new();
        loop {
            let Some(unit) = self.peek() else {
                return Err(self.error("Unterminated string in JSON"));
            };
            self.pos += 1;
            match unit {
                0x22 => return Ok(JsString::from_units(out)),
                0x5C => {
                    let Some(escape) = self.peek() else {
                        return Err(self.error("Unterminated string in JSON"));
                    };
                    self.pos += 1;
                    out.push(match escape {
                        0x22 => 0x22,
                        0x5C => 0x5C,
                        0x2F => 0x2F,
                        0x62 => 0x08,
                        0x66 => 0x0C,
                        0x6E => 0x0A,
                        0x72 => 0x0D,
                        0x74 => 0x09,
                        0x75 => {
                            let mut code: u16 = 0;
                            for _ in 0..4 {
                                let digit = self.peek().and_then(|u| char::from_u32(u32::from(u))).and_then(|c| c.to_digit(16));
                                let Some(digit) = digit else {
                                    return Err(self.error("Bad Unicode escape in JSON"));
                                };
                                code = code * 16 + digit as u16;
                                self.pos += 1;
                            }
                            code
                        }
                        _ => {
                            self.pos -= 1;
                            return Err(self.error("Bad escaped character in JSON"));
                        }
                    });
                }
                0..=0x1F => {
                    self.pos -= 1;
                    return Err(self.error("Bad control character in string literal in JSON"));
                }
                _ => out.push(unit),
            }
        }
    }

    fn number(&mut self) -> Result<Value, JsonSyntaxError> {
        let start = self.pos;
        let digits = |p: &mut Self| {
            let from = p.pos;
            while matches!(p.peek(), Some(0x30..=0x39)) {
                p.pos += 1;
            }
            p.pos - from
        };
        if self.peek() == Some(0x2D) {
            self.pos += 1;
        }
        match self.peek() {
            Some(0x30) => self.pos += 1,
            Some(0x31..=0x39) => {
                digits(self);
            }
            _ => return Err(self.error("No number after minus sign in JSON")),
        }
        if self.peek() == Some(0x2E) {
            self.pos += 1;
            if digits(self) == 0 {
                return Err(self.error("Unterminated fractional number in JSON"));
            }
        }
        if matches!(self.peek(), Some(0x65 | 0x45)) {
            self.pos += 1;
            if matches!(self.peek(), Some(0x2B | 0x2D)) {
                self.pos += 1;
            }
            if digits(self) == 0 {
                return Err(self.error("Exponent part is missing a number in JSON"));
            }
        }
        let literal: String = self.text[start..self.pos].iter().map(|&u| u as u8 as char).collect();
        Ok(Value::Number(literal.parse::<f64>().expect("literal numérico validado")))
    }
}
