//! Leitura dos arquivos `*.js` de mapa de teclas de `vs/workbench/services/keybinding/test/node`.
//!
//! `readRawMapping` do original executa o arquivo (um `define({...})` com um literal de objeto). Aqui o literal é lido
//! por um analisador mínimo ([`parse_js_literal`]) que cobre o formato desses arquivos: objetos, chaves sem aspas ou
//! com aspas, textos entre aspas simples ou duplas com escapes, `true` e `false` (as chaves `valueIsDeadKey` e
//! parentes dos layouts do Mac), comentários e vírgula final.

use super::keyboard_mapper_test_utils::keybinding_test_file_path;

/// Um valor do literal JavaScript: só objetos, textos e booleanos aparecem nos arquivos de mapa de teclas.
pub enum JsValue {
    Object(Vec<(String, JsValue)>),
    Text(String),
    Bool(bool),
}

struct JsLiteralParser {
    chars: Vec<char>,
    pos: usize,
}

impl JsLiteralParser {
    fn peek(&self) -> Option<char> {
        self.chars.get(self.pos).copied()
    }

    fn next(&mut self) -> char {
        let char = self.peek().expect("fim inesperado do literal");
        self.pos += 1;
        char
    }

    fn starts_with(&self, prefix: &str) -> bool {
        prefix.chars().enumerate().all(|(i, c)| self.chars.get(self.pos + i) == Some(&c))
    }

    fn skip_whitespace_and_comments(&mut self) {
        loop {
            match self.peek() {
                Some(c) if c.is_whitespace() => self.pos += 1,
                Some('/') if self.starts_with("//") => {
                    while self.peek().is_some_and(|c| c != '\n') {
                        self.pos += 1;
                    }
                }
                Some('/') if self.starts_with("/*") => {
                    self.pos += 2;
                    while !self.starts_with("*/") {
                        self.next();
                    }
                    self.pos += 2;
                }
                _ => return,
            }
        }
    }

    fn expect(&mut self, expected: char) {
        let found = self.next();
        assert_eq!(found, expected, "esperava `{expected}` na posição {}", self.pos - 1);
    }

    fn parse_hex(&mut self, digits: usize) -> u32 {
        let text: String = (0..digits).map(|_| self.next()).collect();
        u32::from_str_radix(&text, 16).expect("escape hexadecimal inválido")
    }

    fn parse_text(&mut self) -> String {
        let quote = self.next();
        let mut units: Vec<u16> = Vec::new();
        let push_char = |units: &mut Vec<u16>, c: char| {
            let mut buffer = [0u16; 2];
            units.extend_from_slice(c.encode_utf16(&mut buffer));
        };
        loop {
            let char = self.next();
            if char == quote {
                break;
            }
            if char != '\\' {
                push_char(&mut units, char);
                continue;
            }
            match self.next() {
                'n' => units.push(u16::from(b'\n')),
                't' => units.push(u16::from(b'\t')),
                'r' => units.push(u16::from(b'\r')),
                'b' => units.push(8),
                'f' => units.push(12),
                'v' => units.push(11),
                '0' => units.push(0),
                'x' => {
                    let code = self.parse_hex(2);
                    units.push(code as u16);
                }
                'u' => {
                    let code = self.parse_hex(4);
                    units.push(code as u16);
                }
                other => push_char(&mut units, other),
            }
        }
        String::from_utf16(&units).expect("texto UTF-16 inválido no literal")
    }

    fn parse_key(&mut self) -> String {
        if matches!(self.peek(), Some('\'' | '"')) {
            return self.parse_text();
        }
        let mut key = String::new();
        while let Some(c) = self.peek().filter(|c| c.is_alphanumeric() || *c == '_' || *c == '$') {
            key.push(c);
            self.pos += 1;
        }
        assert!(!key.is_empty(), "chave vazia na posição {}", self.pos);
        key
    }

    fn parse_object(&mut self) -> Vec<(String, JsValue)> {
        self.expect('{');
        let mut entries: Vec<(String, JsValue)> = Vec::new();
        loop {
            self.skip_whitespace_and_comments();
            if self.peek() == Some('}') {
                self.pos += 1;
                return entries;
            }
            let key = self.parse_key();
            self.skip_whitespace_and_comments();
            self.expect(':');
            let value = self.parse_value();
            // Uma chave repetida substitui o valor e mantém a posição, como a atribuição de propriedade do JS.
            match entries.iter_mut().find(|(existing, _)| *existing == key) {
                Some(entry) => entry.1 = value,
                None => entries.push((key, value)),
            }
            self.skip_whitespace_and_comments();
            if self.peek() == Some(',') {
                self.pos += 1;
            }
        }
    }

    fn parse_value(&mut self) -> JsValue {
        self.skip_whitespace_and_comments();
        match self.peek() {
            Some('{') => JsValue::Object(self.parse_object()),
            Some('\'' | '"') => JsValue::Text(self.parse_text()),
            Some(c) if c.is_alphabetic() => match self.parse_key().as_str() {
                "true" => JsValue::Bool(true),
                "false" => JsValue::Bool(false),
                other => panic!("valor inesperado `{other}` na posição {}", self.pos),
            },
            other => panic!("valor inesperado {other:?} na posição {}", self.pos),
        }
    }
}

/// O objeto passado a `define(...)` no texto de um arquivo de mapa de teclas.
pub fn parse_js_literal(contents: &str) -> Vec<(String, JsValue)> {
    let start = contents.find("define(").expect("o arquivo não chama define(...)") + "define(".len();
    let mut parser = JsLiteralParser { chars: contents[start..].chars().collect(), pos: 0 };
    parser.skip_whitespace_and_comments();
    parser.parse_object()
}

/// Uma entrada do mapa de teclas (`KeyA: { value: 'a', ... }`): o escancode e os campos do objeto.
pub struct RawEntry {
    pub code: String,
    fields: Vec<(String, JsValue)>,
}

impl RawEntry {
    /// O campo de texto `name`; vazio quando ele não existe (o original só lê `value`, `withShift` e parentes).
    pub fn text(&self, name: &str) -> String {
        self.fields
            .iter()
            .find_map(|(key, value)| match value {
                JsValue::Text(text) if key == name => Some(text.clone()),
                _ => None,
            })
            .unwrap_or_default()
    }
}

/// `readRawMapping(file)`: as entradas do arquivo `{file}.js` na ordem em que ele as declara.
pub fn read_raw_entries(file: &str) -> Vec<RawEntry> {
    let path = keybinding_test_file_path(&format!("{file}.js"));
    let contents = std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("não foi possível ler {}: {error}", path.display()));
    parse_js_literal(&contents)
        .into_iter()
        .map(|(code, mapping)| {
            let JsValue::Object(fields) = mapping else { panic!("o mapeamento de {code} não é um objeto") };
            RawEntry { code, fields }
        })
        .collect()
}
