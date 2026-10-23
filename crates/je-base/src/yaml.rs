//! Porte de `src/vs/base/common/yaml.ts`: o parser de YAML simplificado do VS Code.
//!
//! Lê objetos, listas e escalares (texto, número, booleano e nulo, todos como texto) e guarda a posição
//! de cada nó para relatório de erro. Limitações herdadas do original: sem âncoras nem referências, sem
//! tipos complexos (datas, binário) e sem entradas implícitas de par único.
//!
//! O texto segue a semântica de `string` do JavaScript: a entrada é [`JsString`] (UTF-16) e todo
//! deslocamento (`start_offset`, `end_offset`) conta unidades UTF-16, como o original.
//!
//! Divergências do original:
//! - `String.fromCodePoint` lança `RangeError` num escape `\u`, `\U` com código fora de `0..=0x10FFFF`
//!   (por exemplo `"\UFFFFFFFF"`), e a exceção atravessa o `parse`. Aqui as funções públicas devolvem
//!   `Result` com [`YamlRangeError`] nesse caso, e o resto do comportamento é idêntico.
//! - [`YamlParseError::message`] é `String`: o texto de `Duplicate key` com um surrogate isolado na
//!   chave sai com U+FFFD no lugar dele.

use crate::js_string::{JsString, is_js_whitespace};
use crate::nls::localize;

// -- AST Node Types ----------------------------------------------------------

/// Estilo de um escalar. `Plain` é o `'none'` do original (sem aspas).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScalarFormat {
    Single,
    Double,
    Plain,
    Literal,
    Folded,
}

/// `'block'` ou `'flow'`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CollectionStyle {
    Block,
    Flow,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct YamlScalarNode {
    pub value: JsString,
    pub raw_value: JsString,
    pub start_offset: usize,
    pub end_offset: usize,
    pub format: ScalarFormat,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct YamlProperty {
    pub key: YamlScalarNode,
    pub value: YamlNode,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct YamlMapNode {
    pub properties: Vec<YamlProperty>,
    pub style: CollectionStyle,
    pub start_offset: usize,
    pub end_offset: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct YamlSequenceNode {
    pub items: Vec<YamlNode>,
    pub style: CollectionStyle,
    pub start_offset: usize,
    pub end_offset: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum YamlNode {
    Scalar(YamlScalarNode),
    Map(YamlMapNode),
    Sequence(YamlSequenceNode),
}

impl YamlNode {
    /// O `type` do original: `"scalar"`, `"map"` ou `"sequence"`.
    pub fn type_name(&self) -> &'static str {
        match self {
            YamlNode::Scalar(_) => "scalar",
            YamlNode::Map(_) => "map",
            YamlNode::Sequence(_) => "sequence",
        }
    }

    pub fn start_offset(&self) -> usize {
        match self {
            YamlNode::Scalar(n) => n.start_offset,
            YamlNode::Map(n) => n.start_offset,
            YamlNode::Sequence(n) => n.start_offset,
        }
    }

    pub fn end_offset(&self) -> usize {
        match self {
            YamlNode::Scalar(n) => n.end_offset,
            YamlNode::Map(n) => n.end_offset,
            YamlNode::Sequence(n) => n.end_offset,
        }
    }

    pub fn as_scalar(&self) -> Option<&YamlScalarNode> {
        match self {
            YamlNode::Scalar(n) => Some(n),
            _ => None,
        }
    }

    pub fn as_map(&self) -> Option<&YamlMapNode> {
        match self {
            YamlNode::Map(n) => Some(n),
            _ => None,
        }
    }

    pub fn as_sequence(&self) -> Option<&YamlSequenceNode> {
        match self {
            YamlNode::Sequence(n) => Some(n),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct YamlParseError {
    pub message: String,
    pub start_offset: usize,
    pub end_offset: usize,
    pub code: &'static str,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ParseOptions {
    pub allow_duplicate_keys: bool,
}

/// O `RangeError` de `String.fromCodePoint` num escape de código inválido (ver as divergências do módulo).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct YamlRangeError {
    pub code_point: i64,
}

impl std::fmt::Display for YamlRangeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Invalid code point {}", self.code_point)
    }
}

impl std::error::Error for YamlRangeError {}

// -- Entradas públicas --------------------------------------------------------

/// `parse(input, errors, options)`: lê o YAML de uma única string. Devolve `None` para entrada vazia.
pub fn parse(
    input: &JsString,
    errors: &mut Vec<YamlParseError>,
    options: ParseOptions,
) -> Result<Option<YamlNode>, YamlRangeError> {
    let tokens = Scanner::new(input.units()).scan()?;
    Ok(Parser::new(tokens, input.units(), errors, options).parse())
}

/// O corpo de um Markdown com frontmatter YAML (`MarkdownNode`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MarkdownNode {
    pub header: Option<YamlNode>,
    pub body: JsString,
}

impl MarkdownNode {
    pub fn new(header: Option<YamlNode>, body: JsString) -> Self {
        Self { header, body }
    }

    fn property(&self, name: &str) -> Option<&YamlProperty> {
        match &self.header {
            Some(YamlNode::Map(map)) => map.properties.iter().find(|p| p.key.value == name),
            _ => None,
        }
    }

    pub fn get_string_value(&self, name: &str) -> Option<JsString> {
        match &self.property(name)?.value {
            YamlNode::Scalar(scalar) => Some(scalar.value.clone()),
            _ => None,
        }
    }

    pub fn get_string_array_value(&self, name: &str) -> Option<Vec<JsString>> {
        match &self.property(name)?.value {
            YamlNode::Sequence(seq) => Some(
                seq.items
                    .iter()
                    .filter_map(|item| item.as_scalar().map(|s| s.value.clone()))
                    .collect(),
            ),
            YamlNode::Scalar(scalar) if scalar.format == ScalarFormat::Plain => Some(
                parse_comma_separated_list(&scalar.value, 0)
                    .map(|items| items.into_iter().map(|item| item.value).collect())
                    .unwrap_or_default(),
            ),
            YamlNode::Scalar(scalar) => Some(vec![scalar.value.clone()]),
            YamlNode::Map(_) => None,
        }
    }

    pub fn get_boolean_value(&self, name: &str) -> Option<bool> {
        let value = self.get_string_value(name)?;
        if value == "true" {
            Some(true)
        } else if value == "false" {
            Some(false)
        } else {
            None
        }
    }
}

/// `parseFrontMatter(input, errors, options)`: Markdown com frontmatter YAML entre dois `---`.
pub fn parse_front_matter(
    input: &JsString,
    errors: &mut Vec<YamlParseError>,
    options: ParseOptions,
) -> Result<MarkdownNode, YamlRangeError> {
    let tokens = Scanner::new(input.units()).scan()?;
    if tokens.first().is_none_or(|t| t.ty != TokenType::DocumentStart) {
        // does not start with a frontmatter header (---)
        return Ok(MarkdownNode::new(None, input.clone()));
    }
    if !tokens[1..].iter().any(|t| t.ty == TokenType::DocumentStart) {
        return Ok(MarkdownNode::new(None, input.clone()));
    }
    let last = &tokens[tokens.len() - 1];
    let body = if last.ty == TokenType::Eof {
        substring(input.units(), last.start_offset, input.len())
    } else {
        JsString::new()
    };
    let header = Parser::new(tokens, input.units(), errors, options).parse();
    Ok(MarkdownNode::new(header, body))
}

/// `parseCommaSeparatedList(value, offset)`: lista separada por vírgula de um escalar.
///
/// Embrulha o valor em `[...]` e delega ao parser completo, então aspas, espaços e escapes seguem as
/// regras do resto do parser. Os deslocamentos de cada item são relativos ao documento de origem.
pub fn parse_comma_separated_list(
    value: &JsString,
    offset: usize,
) -> Result<Vec<YamlScalarNode>, YamlRangeError> {
    // Wrap the value as a YAML flow sequence and parse it.
    let mut wrapped = JsString::from("[");
    wrapped.push_units(value.units());
    wrapped.push_unit(u16::from(b']'));
    let parsed = parse(&wrapped, &mut Vec::new(), ParseOptions::default())?;
    // Items from the synthetic string start at offset 1 (after the '['), so shift them.
    let shift = |n: usize| (n as i64 + offset as i64 - 1) as usize;
    let mut items = Vec::new();
    match parsed {
        Some(YamlNode::Sequence(seq)) => {
            for item in seq.items {
                if let YamlNode::Scalar(mut scalar) = item {
                    scalar.start_offset = shift(scalar.start_offset);
                    scalar.end_offset = shift(scalar.end_offset);
                    items.push(scalar);
                }
            }
        }
        _ => items.push(YamlScalarNode {
            value: value.clone(),
            raw_value: value.clone(),
            start_offset: offset,
            end_offset: value.len() + offset,
            format: ScalarFormat::Plain,
        }),
    }
    Ok(items)
}

// -- Helpers de texto UTF-16 --------------------------------------------------

/// Sentinela de "fora do texto" (o `undefined` de `input[i]`), fora do alcance de qualquer unidade UTF-16.
const EOI: u32 = u32::MAX;
const NL: u32 = 0x0A;
const CR: u32 = 0x0D;
const SP: u32 = 0x20;
const TAB: u32 = 0x09;

const fn ch(c: u8) -> u32 {
    c as u32
}

/// `String.prototype.substring(start, end)` com `start <= end` (índices acima do tamanho são limitados).
fn substring(units: &[u16], start: usize, end: usize) -> JsString {
    let end = end.min(units.len());
    let start = start.min(end);
    JsString::from_units(units[start..end].to_vec())
}

fn is_break(c: u32) -> bool {
    c == NL || c == CR
}

/// `parseInt(text, 16)`: espaços iniciais, sinal, prefixo `0x`, e para no primeiro dígito inválido.
fn parse_int_hex(text: &[u16]) -> Option<i64> {
    let mut i = 0;
    while i < text.len() && is_js_whitespace(text[i]) {
        i += 1;
    }
    let mut negative = false;
    if i < text.len() && (text[i] == u16::from(b'+') || text[i] == u16::from(b'-')) {
        negative = text[i] == u16::from(b'-');
        i += 1;
    }
    if i + 1 < text.len()
        && text[i] == u16::from(b'0')
        && (text[i + 1] == u16::from(b'x') || text[i + 1] == u16::from(b'X'))
    {
        i += 2;
    }
    let mut value: Option<i64> = None;
    while let Some(digit) = text.get(i).and_then(|&u| char::from_u32(u32::from(u))).and_then(|c| c.to_digit(16)) {
        value = Some(value.unwrap_or(0) * 16 + i64::from(digit));
        i += 1;
    }
    value.map(|v| if negative { -v } else { v })
}

/// `String.fromCodePoint(code)`: lança `RangeError` fora de `0..=0x10FFFF`.
fn push_code_point(out: &mut Vec<u16>, code: i64) -> Result<(), YamlRangeError> {
    if !(0..=0x10FFFF).contains(&code) {
        return Err(YamlRangeError { code_point: code });
    }
    let code = code as u32;
    if code < 0x10000 {
        out.push(code as u16);
    } else {
        let c = code - 0x10000;
        out.push(0xD800 + (c >> 10) as u16);
        out.push(0xDC00 + (c & 0x3FF) as u16);
    }
    Ok(())
}

// -- Token Types -------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TokenType {
    /// Unquoted, single-quoted, double-quoted and block scalars.
    Scalar,
    Colon,
    Dash,
    Comma,
    FlowMapStart,
    FlowMapEnd,
    FlowSeqStart,
    FlowSeqEnd,
    Newline,
    /// Leading whitespace at start of line (carries the indent level).
    Indent,
    Comment,
    DocumentStart,
    DocumentEnd,
    Eof,
}

#[derive(Debug, Clone)]
struct Token {
    ty: TokenType,
    start_offset: usize,
    end_offset: usize,
    /// For Scalar tokens: the interpreted string value.
    value: JsString,
    /// For Scalar tokens: quote style.
    format: ScalarFormat,
    /// For Indent tokens: the column (number of spaces).
    indent: i64,
}

fn make_token(ty: TokenType, start_offset: usize, end_offset: usize) -> Token {
    Token {
        ty,
        start_offset,
        end_offset,
        value: JsString::new(),
        format: ScalarFormat::Plain,
        indent: 0,
    }
}

// -- Scanner -----------------------------------------------------------------

struct Scanner<'a> {
    input: &'a [u16],
    pos: usize,
    tokens: Vec<Token>,
    // Track flow nesting depth so commas and flow indicators are only special inside flow collections
    flow_depth: usize,
    // After the first key: value colon, subsequent ': ' on the same line is part of the scalar value.
    seen_block_colon: bool,
    seen_document_start: usize,
}

impl<'a> Scanner<'a> {
    fn new(input: &'a [u16]) -> Self {
        Self {
            input,
            pos: 0,
            tokens: Vec::new(),
            flow_depth: 0,
            seen_block_colon: false,
            seen_document_start: 0,
        }
    }

    fn at(&self, index: usize) -> u32 {
        self.input.get(index).map_or(EOI, |&u| u32::from(u))
    }

    fn peek_char(&self) -> u32 {
        self.at(self.pos)
    }

    fn at_end(&self) -> bool {
        self.pos >= self.input.len()
    }

    fn scan(mut self) -> Result<Vec<Token>, YamlRangeError> {
        const MAX_DOCUMENTS: usize = 1;
        while !self.at_end() {
            self.scan_line()?;
            if self.seen_document_start > MAX_DOCUMENTS {
                break;
            }
        }
        self.tokens.push(make_token(TokenType::Eof, self.pos, self.pos));
        Ok(self.tokens)
    }

    /// Scan a single logical line (up to and including the newline character).
    fn scan_line(&mut self) -> Result<(), YamlRangeError> {
        self.seen_block_colon = false;
        // Handle blank lines / lines that are only whitespace
        if self.peek_char() == NL {
            self.tokens.push(make_token(TokenType::Newline, self.pos, self.pos + 1));
            self.pos += 1;
            return Ok(());
        }
        if self.peek_char() == CR {
            let end = self.pos + if self.at(self.pos + 1) == NL { 2 } else { 1 };
            self.tokens.push(make_token(TokenType::Newline, self.pos, end));
            self.pos = end;
            return Ok(());
        }

        // Measure leading whitespace -> Indent token
        let indent_start = self.pos;
        let mut indent = 0;
        while !self.at_end() && (self.peek_char() == SP || self.peek_char() == TAB) {
            indent += 1;
            self.pos += 1;
        }
        if indent > 0 {
            let mut token = make_token(TokenType::Indent, indent_start, self.pos);
            token.indent = indent;
            self.tokens.push(token);
        }

        // If line is now empty (only whitespace before newline/EOF), emit newline
        if self.at_end() || is_break(self.peek_char()) {
            if !self.at_end() {
                let nl_start = self.pos;
                let end = if self.peek_char() == CR && self.at(self.pos + 1) == NL {
                    self.pos + 2
                } else {
                    self.pos + 1
                };
                self.tokens.push(make_token(TokenType::Newline, nl_start, end));
                self.pos = end;
            }
            return Ok(());
        }

        // Check for document markers (--- / ...) at column 0
        if indent == 0 && self.input.len() - self.pos >= 3 {
            let (c0, c1, c2, c3) = (self.at(self.pos), self.at(self.pos + 1), self.at(self.pos + 2), self.at(self.pos + 3));
            let is_terminator = c3 == EOI || c3 == SP || c3 == TAB || c3 == NL || c3 == CR;
            if c0 == ch(b'-') && c1 == ch(b'-') && c2 == ch(b'-') && is_terminator {
                self.tokens.push(make_token(TokenType::DocumentStart, self.pos, self.pos + 3));
                self.pos += 3;
                self.scan_line_content()?;
                self.scan_newline();
                self.seen_document_start += 1;
                return Ok(());
            }
            if c0 == ch(b'.') && c1 == ch(b'.') && c2 == ch(b'.') && is_terminator {
                self.tokens.push(make_token(TokenType::DocumentEnd, self.pos, self.pos + 3));
                self.pos += 3;
                self.scan_line_content()?;
                self.scan_newline();
                return Ok(());
            }
        }

        // Check for comment-only line
        if self.peek_char() == ch(b'#') {
            self.scan_comment();
            self.scan_newline();
            return Ok(());
        }

        // Skip directive lines (e.g., %YAML 1.2, %TAG) - consume rest of line
        if self.peek_char() == ch(b'%') {
            while !self.at_end() && !is_break(self.peek_char()) {
                self.pos += 1;
            }
            self.scan_newline();
            return Ok(());
        }

        // Scan the rest of the line for tokens
        self.scan_line_content()?;
        self.scan_newline();
        Ok(())
    }

    fn push_single(&mut self, ty: TokenType) {
        self.tokens.push(make_token(ty, self.pos, self.pos + 1));
        self.pos += 1;
    }

    fn scan_line_content(&mut self) -> Result<(), YamlRangeError> {
        while !self.at_end() && !is_break(self.peek_char()) {
            self.skip_inline_whitespace();
            if self.at_end() || is_break(self.peek_char()) {
                break;
            }

            let c = self.peek_char();

            if c == ch(b'#') {
                self.scan_comment();
                break; // comment consumes rest of line
            } else if c == ch(b'{') {
                self.flow_depth += 1;
                self.push_single(TokenType::FlowMapStart);
            } else if c == ch(b'}') && self.flow_depth > 0 {
                self.flow_depth -= 1;
                self.push_single(TokenType::FlowMapEnd);
            } else if c == ch(b'[') {
                self.flow_depth += 1;
                self.push_single(TokenType::FlowSeqStart);
            } else if c == ch(b']') && self.flow_depth > 0 {
                self.flow_depth -= 1;
                self.push_single(TokenType::FlowSeqEnd);
            } else if c == ch(b',') && self.flow_depth > 0 {
                self.push_single(TokenType::Comma);
            } else if c == ch(b'-') && self.is_block_dash() {
                // Block sequence indicator: '- ' or '-' at end of line
                self.push_single(TokenType::Dash);
            } else if c == ch(b':') && self.is_block_colon() {
                self.push_single(TokenType::Colon);
                if self.flow_depth == 0 {
                    self.seen_block_colon = true;
                }
            } else if c == ch(b':') && self.flow_depth > 0 && self.last_token_is_json_like() {
                // In flow context, ':' immediately following a JSON-like node (quoted scalar,
                // flow mapping, or flow sequence) is a value indicator even without trailing space
                self.push_single(TokenType::Colon);
            } else if c == ch(b'\'') || c == ch(b'"') {
                self.scan_quoted_scalar(c)?;
            } else if (c == ch(b'|') || c == ch(b'>')) && self.flow_depth == 0 && self.is_block_scalar_start() {
                self.scan_block_scalar(c == ch(b'|'));
                break; // Block scalar consumed multiple lines; return to main scan loop
            } else {
                self.scan_unquoted_scalar();
            }
        }
        Ok(())
    }

    /// Check if '-' is a block sequence dash (followed by space, newline, or EOF).
    fn is_block_dash(&self) -> bool {
        let next = self.at(self.pos + 1);
        next == EOI || next == SP || next == TAB || next == NL || next == CR
    }

    /// Check if ':' acts as a mapping value indicator (followed by space, newline, EOF, or flow indicator).
    fn is_block_colon(&self) -> bool {
        // In block context, after the first key-value colon on a line,
        // subsequent ': ' is part of the scalar value, not a mapping indicator.
        if self.seen_block_colon && self.flow_depth == 0 {
            return false;
        }
        let next = self.at(self.pos + 1);
        if next == EOI || next == SP || next == TAB || next == NL || next == CR {
            return true;
        }
        // Flow indicators after colon only count inside flow context
        self.flow_depth > 0 && (next == ch(b',') || next == ch(b'}') || next == ch(b']'))
    }

    /// Check if the last non-whitespace token is a JSON-like node (quoted scalar or flow end).
    fn last_token_is_json_like(&self) -> bool {
        for t in self.tokens.iter().rev() {
            if matches!(t.ty, TokenType::Newline | TokenType::Indent | TokenType::Comment) {
                continue;
            }
            return (t.ty == TokenType::Scalar && t.format != ScalarFormat::Plain)
                || matches!(t.ty, TokenType::FlowMapEnd | TokenType::FlowSeqEnd);
        }
        false
    }

    fn push_scalar(&mut self, start: usize, end: usize, value: Vec<u16>, format: ScalarFormat) {
        let mut token = make_token(TokenType::Scalar, start, end);
        token.value = JsString::from_units(value);
        token.format = format;
        self.tokens.push(token);
    }

    fn scan_quoted_scalar(&mut self, quote: u32) -> Result<(), YamlRangeError> {
        let format = if quote == ch(b'\'') { ScalarFormat::Single } else { ScalarFormat::Double };
        let start = self.pos;
        self.pos += 1; // skip opening quote
        let mut value: Vec<u16> = Vec::new();
        // Track trailing literal whitespace count so flow folding only trims
        // source-level whitespace, not whitespace produced by escape sequences
        let mut trailing_literal_ws = 0usize;

        while !self.at_end() {
            let c = self.peek_char();
            if c == quote {
                // In single-quoted strings, '' is an escaped single quote
                if quote == ch(b'\'') && self.at(self.pos + 1) == ch(b'\'') {
                    value.push(u16::from(b'\''));
                    self.pos += 2;
                    trailing_literal_ws = 0;
                    continue;
                }
                self.pos += 1; // skip closing quote
                self.push_scalar(start, self.pos, value, format);
                return Ok(());
            }

            // Handle escape sequences in double-quoted strings
            if quote == ch(b'"') && c == ch(b'\\') {
                let next = self.at(self.pos + 1);
                // Escaped line break: \ + newline -> join lines without inserting a space
                if is_break(next) {
                    self.pos += 1; // skip '\'
                    self.consume_newline();
                    // Strip leading whitespace on continuation line
                    self.skip_inline_whitespace();
                    trailing_literal_ws = 0;
                    continue;
                }
                if next == ch(b'x') || next == ch(b'u') || next == ch(b'U') {
                    let (digits, marker) = match next {
                        n if n == ch(b'x') => (2, b'x'),
                        n if n == ch(b'u') => (4, b'u'),
                        _ => (8, b'U'),
                    };
                    let hex_end = (self.pos + 2 + digits).min(self.input.len());
                    let hex = &self.input[(self.pos + 2).min(hex_end)..hex_end];
                    let code = parse_int_hex(hex);
                    match code {
                        Some(code) if hex.len() == digits => {
                            if marker == b'x' {
                                // String.fromCharCode: ToUint16
                                value.push(code.rem_euclid(0x10000) as u16);
                            } else {
                                push_code_point(&mut value, code)?;
                            }
                            self.pos += 2 + digits;
                        }
                        _ => {
                            value.push(u16::from(b'\\'));
                            value.push(u16::from(marker));
                            self.pos += 2;
                        }
                    }
                    trailing_literal_ws = 0;
                    continue;
                }
                match u8::try_from(next).ok() {
                    Some(b'n') => value.push(0x0A),
                    Some(b't') => value.push(0x09),
                    Some(b'\\') => value.push(u16::from(b'\\')),
                    Some(b'"') => value.push(u16::from(b'"')),
                    Some(b'/') => value.push(u16::from(b'/')),
                    Some(b'r') => value.push(0x0D),
                    Some(b'0') => value.push(0),
                    Some(b'a') => value.push(0x07),
                    Some(b'b') => value.push(0x08),
                    Some(b'e') => value.push(0x1B),
                    Some(b'v') => value.push(0x0B),
                    Some(b'f') => value.push(0x0C),
                    Some(b' ') => value.push(0x20),
                    Some(b'_') => value.push(0xA0),
                    _ => {
                        value.push(u16::from(b'\\'));
                        if next != EOI {
                            value.push(next as u16);
                        }
                    }
                }
                self.pos += 2;
                trailing_literal_ws = 0;
                continue;
            }

            // Flow folding: handle newlines inside quoted scalars (both single and double)
            if is_break(c) {
                // Trim trailing literal whitespace (not escape-produced whitespace)
                if trailing_literal_ws > 0 {
                    value.truncate(value.len() - trailing_literal_ws);
                }
                trailing_literal_ws = 0;

                // Skip the newline
                self.consume_newline();

                // Count empty lines (lines with only whitespace)
                let mut empty_line_count = 0;
                while !self.at_end() {
                    // Skip whitespace at start of line
                    self.skip_inline_whitespace();
                    // Check if this line is empty (another newline follows)
                    if is_break(self.peek_char()) {
                        empty_line_count += 1;
                        self.consume_newline();
                    } else {
                        break;
                    }
                }

                // Apply folding: empty lines -> \n each, otherwise single newline -> space
                if empty_line_count > 0 {
                    value.extend(std::iter::repeat_n(0x0A, empty_line_count));
                } else {
                    value.push(0x20);
                }
                continue;
            }

            // Track literal whitespace for folding purposes
            if c == SP || c == TAB {
                trailing_literal_ws += 1;
            } else {
                trailing_literal_ws = 0;
            }
            value.push(c as u16);
            self.pos += 1;
        }

        // Unterminated string - emit what we have
        self.push_scalar(start, self.pos, value, format);
        Ok(())
    }

    fn scan_unquoted_scalar(&mut self) {
        let start = self.pos;
        let mut end = self.pos;

        while !self.at_end() {
            let c = self.peek_char();
            // Stop at newline
            if is_break(c) {
                break;
            }
            // Stop at flow indicators (only inside flow collections)
            if self.flow_depth > 0 && (c == ch(b',') || c == ch(b'}') || c == ch(b']')) {
                break;
            }
            if self.flow_depth > 0 && (c == ch(b'{') || c == ch(b'[')) {
                break;
            }
            // Stop at ': ' or ':' at end-of-line (mapping value indicator)
            if c == ch(b':') && self.is_block_colon() {
                break;
            }
            // Stop at ' #' (comment)
            if c == ch(b'#') && self.pos > start && matches!(self.at(self.pos - 1), SP | TAB) {
                break;
            }

            self.pos += 1;
            // Track the last non-whitespace position to trim trailing whitespace
            if c != SP && c != TAB {
                end = self.pos;
            }
        }

        let value = self.input[start..end].to_vec();
        self.push_scalar(start, end, value, ScalarFormat::Plain);
    }

    /// Check if '|' or '>' at the current position is a block scalar indicator.
    /// Must be followed by optional indentation/chomping indicators, optional comment, then newline.
    fn is_block_scalar_start(&self) -> bool {
        let mut p = self.pos + 1;
        // Skip optional indentation indicator (digit 1-9) and chomping indicator (+/-)
        while p < self.input.len() {
            let c = self.at(p);
            if (ch(b'1')..=ch(b'9')).contains(&c) || c == ch(b'+') || c == ch(b'-') {
                p += 1;
                continue;
            }
            break;
        }
        // Skip optional whitespace
        while p < self.input.len() && matches!(self.at(p), SP | TAB) {
            p += 1;
        }
        // Must be at newline, EOF, or comment
        if p >= self.input.len() {
            return true;
        }
        let c = self.at(p);
        is_break(c) || c == ch(b'#')
    }

    /// Scan a block scalar (literal '|' or folded '>').
    /// Parses the header line for indentation indicator and chomping mode,
    /// then collects all content lines that are indented beyond the detected indentation.
    fn scan_block_scalar(&mut self, literal: bool) {
        #[derive(PartialEq)]
        enum Chomping {
            Clip,
            Strip,
            Keep,
        }

        let start = self.pos;
        self.pos += 1; // skip '|' or '>'

        // Parse header: optional indentation indicator (1-9) and chomping indicator (+/-)
        let mut explicit_indent: i64 = 0;
        let mut chomping = Chomping::Clip;

        // The order of indent indicator and chomping indicator can vary (D83L test)
        for _ in 0..2 {
            if !self.at_end() {
                let c = self.peek_char();
                if (ch(b'1')..=ch(b'9')).contains(&c) && explicit_indent == 0 {
                    explicit_indent = i64::from(c - ch(b'0'));
                    self.pos += 1;
                } else if c == ch(b'-') && chomping == Chomping::Clip {
                    chomping = Chomping::Strip;
                    self.pos += 1;
                } else if c == ch(b'+') && chomping == Chomping::Clip {
                    chomping = Chomping::Keep;
                    self.pos += 1;
                }
            }
        }

        // Skip any trailing whitespace on the header line
        self.skip_inline_whitespace();

        // Skip optional comment on header line
        if !self.at_end() && self.peek_char() == ch(b'#') {
            while !self.at_end() && !is_break(self.peek_char()) {
                self.pos += 1;
            }
        }

        // Skip the header line's newline
        self.consume_newline();

        // Per YAML spec 8.1.1.1, content indentation = parent_block_indent + N
        // where N is the explicit indent indicator (or auto-detected).
        let parent_block_indent = self.parent_block_indent();

        // Compute the content indentation level
        let mut content_indent: i64 = if explicit_indent > 0 { parent_block_indent + explicit_indent } else { 0 };
        let mut lines: Vec<Vec<u16>> = Vec::new();
        let mut trailing_newlines = 0usize;

        while !self.at_end() {
            let line_start = self.pos;

            // Count leading spaces on this line (tabs are not valid YAML indentation)
            let mut line_indent: i64 = 0;
            while !self.at_end() && self.peek_char() == SP {
                line_indent += 1;
                self.pos += 1;
            }

            // Check if this is an empty or whitespace-only line
            if self.at_end() || is_break(self.peek_char()) {
                if content_indent > 0 && line_indent >= content_indent {
                    // Whitespace-only line with enough indent - preserve excess whitespace
                    let preserved = self.input[line_start + content_indent as usize..self.pos].to_vec();
                    if preserved.is_empty() {
                        // Effectively an empty line - counts as trailing
                        trailing_newlines += 1;
                    } else {
                        trailing_newlines = 0;
                    }
                    lines.push(preserved);
                } else {
                    // Truly empty line - part of scalar content
                    lines.push(Vec::new());
                    trailing_newlines += 1;
                }
                // Skip newline
                self.consume_newline();
                continue;
            }

            // Check for document markers at column 0 - they terminate the block scalar
            if line_indent == 0 && self.input.len() - self.pos >= 3 {
                let (c0, c1, c2, c3) = (self.at(self.pos), self.at(self.pos + 1), self.at(self.pos + 2), self.at(self.pos + 3));
                let is_term = c3 == EOI || c3 == SP || c3 == TAB || c3 == NL || c3 == CR;
                if is_term
                    && ((c0 == ch(b'-') && c1 == ch(b'-') && c2 == ch(b'-'))
                        || (c0 == ch(b'.') && c1 == ch(b'.') && c2 == ch(b'.')))
                {
                    self.pos = line_start;
                    break;
                }
            }

            // Auto-detect content indent from first non-empty line.
            // Content must be more indented than the parent block.
            if content_indent == 0 {
                if line_indent <= parent_block_indent {
                    // Not enough indentation - terminates the block scalar
                    self.pos = line_start;
                    break;
                }
                content_indent = line_indent;
            }

            // If this line's indentation is less than the content indent, the block scalar is done
            if line_indent < content_indent {
                self.pos = line_start;
                break;
            }

            // Read the rest of the line (the content)
            let content_start = line_start + content_indent as usize;
            while !self.at_end() && !is_break(self.peek_char()) {
                self.pos += 1;
            }
            // The line content includes any extra indentation beyond content_indent
            lines.push(self.input[content_start..self.pos].to_vec());
            trailing_newlines = 0;

            // Skip newline
            self.consume_newline();
        }

        // Process the collected lines according to the block scalar style
        let mut value: Vec<u16> = Vec::new();
        if literal {
            // Literal: join lines with newlines (preserving all line breaks as-is)
            for (i, line) in lines.iter().enumerate() {
                if i > 0 {
                    value.push(0x0A);
                }
                value.extend_from_slice(line);
            }
        } else {
            // Folded: per YAML spec, line breaks between adjacent non-more-indented
            // content lines are folded into spaces. More-indented lines preserve breaks.
            // Empty lines produce \n each. The break from content into an empty run
            // is "trimmed" (absorbed) for non-more-indented lines, but preserved
            // for more-indented lines.
            let mut last_non_empty_is_more_indented = false;
            let mut in_empty_run = false;
            let mut seen_non_empty = false;

            for (i, line) in lines.iter().enumerate() {
                let is_more_indented = line.first().is_some_and(|&u| u == 0x20 || u == 0x09);

                if line.is_empty() {
                    // Empty line -> contributes one \n
                    value.push(0x0A);
                    in_empty_run = true;
                } else if i == 0 {
                    value = line.clone();
                    last_non_empty_is_more_indented = is_more_indented;
                    seen_non_empty = true;
                } else if in_empty_run {
                    // Transitioning from empty lines back to content.
                    // If the previous content or current line is more-indented
                    // AND we've seen content before, the break is preserved.
                    // Otherwise the empties already provided all needed line breaks.
                    if (last_non_empty_is_more_indented || is_more_indented) && seen_non_empty {
                        value.push(0x0A);
                    }
                    value.extend_from_slice(line);
                    last_non_empty_is_more_indented = is_more_indented;
                    in_empty_run = false;
                    seen_non_empty = true;
                } else if is_more_indented || last_non_empty_is_more_indented {
                    // More-indented line -> preserve newline
                    value.push(0x0A);
                    value.extend_from_slice(line);
                    last_non_empty_is_more_indented = is_more_indented;
                    seen_non_empty = true;
                } else {
                    // Normal adjacent non-more-indented lines -> fold to space
                    value.push(0x20);
                    value.extend_from_slice(line);
                    last_non_empty_is_more_indented = false;
                    seen_non_empty = true;
                }
            }
        }

        // Apply chomping to trailing newlines
        if trailing_newlines > 0 {
            // Strip all trailing newlines from the value
            while value.last() == Some(&0x0A) {
                value.pop();
            }
        }

        // Determine if there was any actual (non-empty) content
        let has_content = lines.iter().any(|l| !l.is_empty());

        match chomping {
            Chomping::Clip => {
                if has_content {
                    // Add exactly one trailing newline
                    value.push(0x0A);
                }
            }
            Chomping::Keep => {
                if has_content {
                    // Content + trailing: final line break + trailing empty line breaks
                    value.extend(std::iter::repeat_n(0x0A, trailing_newlines + 1));
                } else {
                    // No content, only trailing empties
                    value = vec![0x0A; trailing_newlines];
                }
            }
            Chomping::Strip => {}
        }

        let format = if literal { ScalarFormat::Literal } else { ScalarFormat::Folded };
        self.push_scalar(start, self.pos, value, format);
    }

    /// Determine the parent block's indentation level for a block scalar.
    /// Looks at preceding tokens to find the context:
    /// - After Colon: the indentation of the line containing the mapping key
    /// - After Dash: the column of the dash
    /// - At document level: -1 (allows content at indent 0)
    fn parent_block_indent(&self) -> i64 {
        let skippable = |t: &Token| matches!(t.ty, TokenType::Newline | TokenType::Comment | TokenType::Indent);
        for (i, t) in self.tokens.iter().enumerate().rev() {
            if skippable(t) {
                continue;
            }
            if t.ty == TokenType::Colon {
                // Block scalar is a mapping value. The parent indentation
                // is the column of the mapping key (the scalar before the colon).
                return self.tokens[..i]
                    .iter()
                    .rev()
                    .find(|kt| !skippable(kt))
                    .map_or(0, |kt| self.column_at(kt.start_offset) as i64);
            }
            if t.ty == TokenType::Dash {
                // Block scalar is a sequence item. Parent indent = column of the dash.
                return self.column_at(t.start_offset) as i64;
            }
            // Document root - content at indent 0 is valid
            if t.ty == TokenType::DocumentStart {
                return -1;
            }
            // For any other token, use 0
            break;
        }
        0
    }

    /// Column (0-based offset from start of line) for a position in the input.
    fn column_at(&self, offset: usize) -> usize {
        line_column(self.input, offset)
    }

    fn scan_comment(&mut self) {
        let start = self.pos;
        while !self.at_end() && !is_break(self.peek_char()) {
            self.pos += 1;
        }
        let mut token = make_token(TokenType::Comment, start, self.pos);
        token.value = substring(self.input, start, self.pos);
        self.tokens.push(token);
    }

    fn scan_newline(&mut self) {
        let start = self.pos;
        if self.consume_newline() {
            self.tokens.push(make_token(TokenType::Newline, start, self.pos));
        }
    }

    fn skip_inline_whitespace(&mut self) {
        while !self.at_end() && matches!(self.peek_char(), SP | TAB) {
            self.pos += 1;
        }
    }

    /// Advance past a newline sequence (\r\n, \n, or \r). Returns true if a newline was consumed.
    fn consume_newline(&mut self) -> bool {
        if self.at_end() {
            return false;
        }
        if self.peek_char() == CR && self.at(self.pos + 1) == NL {
            self.pos += 2;
            return true;
        }
        if is_break(self.peek_char()) {
            self.pos += 1;
            return true;
        }
        false
    }
}

/// Número de unidades entre o início da linha e `offset`.
fn line_column(input: &[u16], offset: usize) -> usize {
    let end = offset.min(input.len());
    input[..end].iter().rev().take_while(|&&u| u != 0x0A && u != 0x0D).count()
}

/// Início da linha que contém `offset`.
fn line_start(input: &[u16], offset: usize) -> usize {
    offset - line_column(input, offset)
}

// -- Parser ------------------------------------------------------------------

struct Parser<'a> {
    pos: usize,
    tokens: Vec<Token>,
    input: &'a [u16],
    errors: &'a mut Vec<YamlParseError>,
    options: ParseOptions,
}

impl<'a> Parser<'a> {
    fn new(tokens: Vec<Token>, input: &'a [u16], errors: &'a mut Vec<YamlParseError>, options: ParseOptions) -> Self {
        Self { pos: 0, tokens, input, errors, options }
    }

    fn parse(&mut self) -> Option<YamlNode> {
        self.skip_newlines_and_comments();
        // Skip document start marker (---) if present
        if self.cur_type() == TokenType::DocumentStart {
            self.advance();
            self.skip_newlines_and_comments();
        }
        if matches!(self.cur_type(), TokenType::Eof | TokenType::DocumentEnd) {
            return None;
        }
        self.parse_value(-1)
    }

    // -- helpers ----------------------------------------------------------

    fn cur(&self) -> &Token {
        &self.tokens[self.pos]
    }

    fn cur_type(&self) -> TokenType {
        self.cur().ty
    }

    fn peek(&self, offset: usize) -> &Token {
        &self.tokens[(self.pos + offset).min(self.tokens.len() - 1)]
    }

    fn peek_type(&self, offset: usize) -> TokenType {
        self.peek(offset).ty
    }

    fn advance(&mut self) -> Token {
        let t = self.tokens[self.pos].clone();
        if t.ty != TokenType::Eof {
            self.pos += 1;
        }
        t
    }

    fn expect(&mut self, ty: TokenType) -> Token {
        if self.cur_type() == ty {
            return self.advance();
        }
        self.cur().clone()
    }

    fn emit_error(&mut self, message: String, start_offset: usize, end_offset: usize, code: &'static str) {
        self.errors.push(YamlParseError { message, start_offset, end_offset, code });
    }

    fn emit_error_at_current(&mut self, message: String, code: &'static str) {
        let (start, end) = (self.cur().start_offset, self.cur().end_offset);
        self.emit_error(message, start, end, code);
    }

    fn emit_unexpected_indentation(&mut self, expected: i64, got: i64) {
        let message = localize(
            "unexpectedIndentation",
            "Unexpected indentation (expected {0}, got {1})",
            &[&expected.to_string(), &got.to_string()],
        );
        self.emit_error_at_current(message, "unexpected-indentation");
    }

    fn skip_newlines_and_comments(&mut self) {
        while matches!(self.cur_type(), TokenType::Newline | TokenType::Comment)
            || (self.cur_type() == TokenType::Indent && self.is_followed_by_newline_or_comment())
        {
            self.advance();
        }
    }

    /// True if the current Indent token is followed immediately by Newline/Comment/EOF.
    fn is_followed_by_newline_or_comment(&self) -> bool {
        matches!(self.peek_type(1), TokenType::Newline | TokenType::Comment | TokenType::Eof)
    }

    /// If the current token is an Indent, returns its indent value. Otherwise 0 (column 0).
    fn current_indent(&self) -> i64 {
        if self.cur_type() == TokenType::Indent { self.cur().indent } else { 0 }
    }

    fn line_start(&self, offset: usize) -> usize {
        line_start(self.input, offset)
    }

    fn column(&self, offset: usize) -> i64 {
        (offset - self.line_start(offset)) as i64
    }

    // -- Main parse entry for a value at a given indentation --------------

    fn parse_value(&mut self, parent_indent: i64) -> Option<YamlNode> {
        self.skip_newlines_and_comments();
        let token_type = self.cur_type();

        // Flow collections (also check past indent)
        let flow_type = if token_type == TokenType::Indent { self.peek_type(1) } else { token_type };
        if matches!(flow_type, TokenType::FlowMapStart | TokenType::FlowSeqStart) {
            if token_type == TokenType::Indent {
                self.advance();
            }
            return Some(if flow_type == TokenType::FlowMapStart {
                YamlNode::Map(self.parse_flow_map())
            } else {
                YamlNode::Sequence(self.parse_flow_seq())
            });
        }

        // Block-level: detect if this is a sequence or mapping
        let indent = self.current_indent();

        // Determine what the first meaningful token is at this indent
        if self.peek_past_indent().ty == TokenType::Dash {
            return Some(YamlNode::Sequence(self.parse_block_sequence(indent)));
        }

        // Check if this looks like a mapping (scalar followed by colon)
        if self.looks_like_mapping() {
            return Some(YamlNode::Map(self.parse_block_mapping(indent, false)));
        }

        // Otherwise it's a scalar
        if matches!(token_type, TokenType::Scalar | TokenType::Indent) {
            return Some(YamlNode::Scalar(self.parse_scalar(parent_indent)));
        }

        None
    }

    /// Peek past an optional Indent token to see the first content token.
    fn peek_past_indent(&self) -> &Token {
        if self.cur_type() == TokenType::Indent { self.peek(1) } else { self.cur() }
    }

    /// Check if tokens at current position look like a mapping entry (key: value).
    fn looks_like_mapping(&self) -> bool {
        let mut offset = 0;
        if self.peek_type(offset) == TokenType::Indent {
            offset += 1;
        }
        self.peek_type(offset) == TokenType::Scalar && self.peek_type(offset + 1) == TokenType::Colon
    }

    // -- Scalar ----------------------------------------------------------

    fn parse_scalar(&mut self, parent_indent: i64) -> YamlScalarNode {
        // Skip indent if present
        if self.cur_type() == TokenType::Indent {
            self.advance();
        }
        let token = self.expect(TokenType::Scalar);
        // Quoted scalars are complete as-is (scanner handles their multiline)
        if token.format != ScalarFormat::Plain {
            return self.scalar_from_token(&token);
        }
        // For unquoted (plain) scalars, check for multiline continuation
        self.parse_plain_multiline(&token, parent_indent)
    }

    /// Parse a multiline plain scalar. The first line's token is already consumed.
    /// Continuation lines must be indented deeper than `parent_indent`.
    /// Line folding rules: single line break -> space, each empty line -> preserved as \n.
    fn parse_plain_multiline(&mut self, first: &Token, parent_indent: i64) -> YamlScalarNode {
        let mut value: Vec<u16> = first.value.units().to_vec();
        let mut end_offset = first.end_offset;

        loop {
            // Save position to backtrack if continuation is not valid
            let saved_pos = self.pos;

            // Count empty lines (newlines with only whitespace between)
            let mut empty_line_count = 0;
            let mut found_content = false;

            while self.pos < self.tokens.len() {
                let ty = self.cur_type();
                if ty == TokenType::Comment {
                    // Comment terminates a plain scalar
                    break;
                }
                if ty == TokenType::Newline {
                    self.advance();
                    // Check if the next thing after this newline is blank or content
                    let after_type = self.cur_type();
                    if after_type == TokenType::Newline {
                        // Another newline means an empty line
                        empty_line_count += 1;
                        continue;
                    }
                    if after_type == TokenType::Indent {
                        // Check what follows the indent
                        let after_indent = self.peek_type(1);
                        if matches!(after_indent, TokenType::Newline | TokenType::Eof) {
                            // Indent followed by newline = empty line
                            empty_line_count += 1;
                            self.advance(); // skip the indent
                            continue;
                        }
                        if after_indent == TokenType::Comment {
                            // Comment terminates scalar
                            break;
                        }
                        // Content on this line - check indentation
                        found_content = self.cur().indent > parent_indent;
                        break;
                    }
                    if after_type == TokenType::Eof {
                        break;
                    }
                    // Document markers terminate plain scalars
                    if matches!(after_type, TokenType::DocumentStart | TokenType::DocumentEnd) {
                        break;
                    }
                    // Content at column 0: top-level (parent_indent = -1) accepts it as continuation
                    found_content = parent_indent < 0;
                    break;
                }
                // An Indent here, or any other token (EOF, structural), ends the scalar
                break;
            }

            if !found_content {
                // No continuation found - restore position
                self.pos = saved_pos;
                break;
            }

            // We found a continuation line. Skip optional indent.
            if self.cur_type() == TokenType::Indent {
                self.advance();
            }

            // The next token must be a Scalar for continuation
            if self.cur_type() != TokenType::Scalar {
                // A dash at a deeper indent than the parent is text content, not a sequence indicator
                // (e.g., "- single multiline\n - sequence entry" -> one scalar "single multiline - sequence entry")
                if self.cur_type() == TokenType::Dash {
                    let dash = self.advance();
                    let mut line_text: Vec<u16> = vec![u16::from(b'-')];
                    if self.cur_type() == TokenType::Scalar {
                        let rest = self.advance();
                        line_text.push(u16::from(b' '));
                        line_text.extend_from_slice(rest.value.units());
                        end_offset = rest.end_offset;
                    } else {
                        end_offset = dash.end_offset;
                    }
                    fold_separator(&mut value, empty_line_count);
                    value.extend_from_slice(&line_text);
                    continue;
                }
                // Not a scalar continuation (could be Colon, etc.)
                self.pos = saved_pos;
                break;
            }

            // Check that this line doesn't look like a mapping key (scalar followed by colon)
            // which would mean the scalar ended and a new mapping entry starts
            if self.peek_type(1) == TokenType::Colon {
                self.pos = saved_pos;
                break;
            }

            let cont = self.advance();

            // Apply line folding: empty lines become \n, single line break becomes space
            fold_separator(&mut value, empty_line_count);
            value.extend_from_slice(cont.value.units());
            end_offset = cont.end_offset;
        }

        YamlScalarNode {
            value: JsString::from_units(value),
            raw_value: substring(self.input, first.start_offset, end_offset),
            start_offset: first.start_offset,
            end_offset,
            format: ScalarFormat::Plain,
        }
    }

    // -- Block mapping ---------------------------------------------------

    fn parse_block_mapping(&mut self, base_indent: i64, inline_first_entry: bool) -> YamlMapNode {
        let start_offset = self.cur().start_offset;
        let mut properties: Vec<YamlProperty> = Vec::new();
        let mut seen_keys: Vec<JsString> = Vec::new();

        // When called after a sequence dash, the first key is already at the current position
        if inline_first_entry && let Some(first) = self.parse_mapping_entry(base_indent) {
            seen_keys.push(first.key.value.clone());
            properties.push(first);
        }

        while self.cur_type() != TokenType::Eof {
            self.skip_newlines_and_comments();
            if self.cur_type() == TokenType::Eof {
                break;
            }

            let indent = self.current_indent();
            if indent < base_indent {
                break;
            }
            if indent != base_indent {
                if indent > base_indent {
                    self.emit_unexpected_indentation(base_indent, indent);
                } else {
                    break;
                }
            }
            if !self.looks_like_mapping() {
                break;
            }

            let Some(entry) = self.parse_mapping_entry(base_indent) else { break };

            if !self.options.allow_duplicate_keys && seen_keys.contains(&entry.key.value) {
                let message = localize(
                    "duplicateKey",
                    "Duplicate key: \"{0}\"",
                    &[&entry.key.value.to_string_lossy()],
                );
                self.emit_error(message, entry.key.start_offset, entry.key.end_offset, "duplicate-key");
            }
            seen_keys.push(entry.key.value.clone());
            properties.push(entry);
        }

        let end_offset = properties.last().map_or(start_offset, |p| p.value.end_offset());
        YamlMapNode { properties, style: CollectionStyle::Block, start_offset, end_offset }
    }

    fn parse_mapping_entry(&mut self, base_indent: i64) -> Option<YamlProperty> {
        // Skip indent
        if self.cur_type() == TokenType::Indent {
            self.advance();
        }

        // Parse key
        let key_token = self.expect(TokenType::Scalar);
        let key = self.scalar_from_token(&key_token);

        // Expect colon
        let colon = self.expect(TokenType::Colon);
        if colon.ty != TokenType::Colon {
            self.emit_error(localize("expectedColon", "Expected \":\"", &[]), colon.start_offset, colon.end_offset, "expected-colon");
            return None;
        }

        // Parse value: could be on same line or next line (indented)
        let value = self.parse_mapping_value(base_indent, &colon);

        Some(YamlProperty { key, value })
    }

    fn missing_value(&mut self, at: &Token) -> YamlNode {
        self.emit_error(localize("missingValue", "Missing value", &[]), at.start_offset, at.end_offset, "missing-value");
        YamlNode::Scalar(make_empty_scalar(at.end_offset))
    }

    fn parse_mapping_value(&mut self, base_indent: i64, colon: &Token) -> YamlNode {
        // Check if there's a value on the same line after the colon
        match self.cur_type() {
            // Same-line flow collections
            TokenType::FlowMapStart => return YamlNode::Map(self.parse_flow_map()),
            TokenType::FlowSeqStart => return YamlNode::Sequence(self.parse_flow_seq()),
            // Same-line scalar (may be multiline with continuation)
            TokenType::Scalar => {
                let token = self.advance();
                if token.format != ScalarFormat::Plain {
                    return YamlNode::Scalar(self.scalar_from_token(&token));
                }
                // Plain scalar - allow multiline continuation deeper than base_indent
                return YamlNode::Scalar(self.parse_plain_multiline(&token, base_indent));
            }
            _ => {}
        }

        // Value is on the next line (skip newlines/comments and check indentation)
        self.skip_newlines_and_comments();

        if self.cur_type() == TokenType::Eof {
            // Missing value at end of input
            return self.missing_value(colon);
        }

        let next_indent = self.current_indent();

        // Special case: a sequence at the same indent as the mapping key is allowed
        // as the mapping value (e.g., "foo:\n- 42")
        if next_indent == base_indent && self.peek_past_indent().ty == TokenType::Dash {
            return self
                .parse_value(base_indent)
                .unwrap_or_else(|| YamlNode::Scalar(make_empty_scalar(colon.end_offset)));
        }

        if next_indent <= base_indent {
            // No deeper indentation -> missing value
            return self.missing_value(colon);
        }

        // Parse the nested value
        self.parse_value(base_indent)
            .unwrap_or_else(|| YamlNode::Scalar(make_empty_scalar(colon.end_offset)))
    }

    // -- Block sequence --------------------------------------------------

    fn parse_block_sequence(&mut self, base_indent: i64) -> YamlSequenceNode {
        let mut items: Vec<YamlNode> = Vec::new();
        let start_offset = self.cur().start_offset;
        let mut end_offset = start_offset;
        let mut is_first_item = true;

        while self.cur_type() != TokenType::Eof {
            self.skip_newlines_and_comments();
            if self.cur_type() == TokenType::Eof {
                break;
            }

            // For the first item, the dash may be on the same line (no Indent token).
            // Compute the actual column to check against base_indent.
            let indent = if is_first_item && self.cur_type() == TokenType::Dash {
                self.column(self.cur().start_offset)
            } else {
                self.current_indent()
            };
            is_first_item = false;

            if indent < base_indent {
                break;
            }

            if indent != base_indent {
                if indent > base_indent {
                    self.emit_unexpected_indentation(base_indent, indent);
                } else {
                    break;
                }
            }

            if self.peek_past_indent().ty != TokenType::Dash {
                break;
            }

            // Skip indent
            if self.cur_type() == TokenType::Indent {
                self.advance();
            }

            // Consume the dash
            let dash = self.advance();

            // Parse the item value
            let item = self.parse_sequence_item_value(base_indent, &dash);
            end_offset = item.end_offset();
            items.push(item);
        }

        YamlSequenceNode { items, style: CollectionStyle::Block, start_offset, end_offset }
    }

    fn missing_seq_item(&mut self, dash: &Token) -> YamlNode {
        self.emit_error(
            localize("missingSeqItemValue", "Missing sequence item value", &[]),
            dash.start_offset,
            dash.end_offset,
            "missing-value",
        );
        YamlNode::Scalar(make_empty_scalar(dash.end_offset))
    }

    fn parse_sequence_item_value(&mut self, base_indent: i64, dash: &Token) -> YamlNode {
        let next_type = self.cur_type();
        let next_start = self.cur().start_offset;

        // Skip comment after dash
        if next_type == TokenType::Comment {
            self.advance();
        }

        match next_type {
            // Flow collections on same line
            TokenType::FlowMapStart => return YamlNode::Map(self.parse_flow_map()),
            TokenType::FlowSeqStart => return YamlNode::Sequence(self.parse_flow_seq()),
            // Nested sequence on same line (e.g., '- - value')
            TokenType::Dash => {
                // The nested sequence's base indent is the column of the dash
                let nested_indent = self.column(next_start);
                return YamlNode::Sequence(self.parse_block_sequence(nested_indent));
            }
            // Inline scalar on same line
            TokenType::Scalar => {
                // Check if this is actually a mapping (key: value on same line after dash)
                if self.peek_type(1) == TokenType::Colon {
                    // It's an inline mapping after '- ' like '- name: John'
                    // The implicit indent for continuation lines is the column of the key
                    let item_indent = self.column(next_start);
                    return YamlNode::Map(self.parse_block_mapping(item_indent, true));
                }
                return YamlNode::Scalar(self.parse_scalar(base_indent));
            }
            _ => {}
        }

        // Value on next line
        self.skip_newlines_and_comments();
        if self.cur_type() == TokenType::Eof {
            return self.missing_seq_item(dash);
        }

        if self.current_indent() <= base_indent {
            // Empty item (just a dash)
            return self.missing_seq_item(dash);
        }

        self.parse_value(base_indent)
            .unwrap_or_else(|| YamlNode::Scalar(make_empty_scalar(dash.end_offset)))
    }

    // -- Flow map --------------------------------------------------------

    fn parse_flow_map(&mut self) -> YamlMapNode {
        let start_token = self.advance(); // consume '{'
        let mut properties: Vec<YamlProperty> = Vec::new();

        self.skip_flow_whitespace();

        while !matches!(self.cur_type(), TokenType::FlowMapEnd | TokenType::Eof) {
            // Parse key (must be a scalar)
            if self.cur_type() != TokenType::Scalar {
                self.emit_error_at_current(localize("expectedMappingKey", "Expected mapping key", &[]), "expected-key");
                break;
            }
            let key = self.parse_flow_scalar();

            self.skip_flow_whitespace();

            // Check for colon - if missing, the key has an empty value (terminated by comma or })
            let value = if self.cur_type() == TokenType::Colon {
                self.advance();
                self.skip_flow_whitespace();

                // Parse value
                self.parse_flow_value()
            } else {
                // Key without value (e.g., { key, other: val })
                YamlNode::Scalar(make_empty_scalar(key.end_offset))
            };

            properties.push(YamlProperty { key, value });

            self.skip_flow_whitespace();

            // Consume comma if present
            if self.cur_type() == TokenType::Comma {
                self.advance();
                self.skip_flow_whitespace();
            }
        }

        let end_token = self.cur().clone();
        let closed = end_token.ty == TokenType::FlowMapEnd;
        if closed {
            self.advance();
        } else {
            self.emit_error(localize("expectedFlowMapEnd", "Expected \"}\"", &[]), end_token.start_offset, end_token.end_offset, "expected-flow-map-end");
        }

        YamlMapNode {
            properties,
            style: CollectionStyle::Flow,
            start_offset: start_token.start_offset,
            end_offset: if closed { end_token.end_offset } else { end_token.start_offset },
        }
    }

    // -- Flow sequence ---------------------------------------------------

    fn parse_flow_seq(&mut self) -> YamlSequenceNode {
        let start_token = self.advance(); // consume '['
        let mut items: Vec<YamlNode> = Vec::new();

        self.skip_flow_whitespace();

        while !matches!(self.cur_type(), TokenType::FlowSeqEnd | TokenType::Eof) {
            let item = match self.cur_type() {
                TokenType::FlowMapStart => YamlNode::Map(self.parse_flow_map()),
                TokenType::FlowSeqStart => YamlNode::Sequence(self.parse_flow_seq()),
                TokenType::Scalar => YamlNode::Scalar(self.parse_flow_scalar()),
                _ => {
                    self.emit_error_at_current(
                        localize("unexpectedTokenInFlowSeq", "Unexpected token in flow sequence", &[]),
                        "unexpected-token",
                    );
                    self.advance();
                    continue;
                }
            };

            items.push(item);
            self.skip_flow_whitespace();

            if self.cur_type() == TokenType::Comma {
                self.advance();
                self.skip_flow_whitespace();
            }
        }

        let end_token = self.cur().clone();
        let closed = end_token.ty == TokenType::FlowSeqEnd;
        if closed {
            self.advance();
        } else {
            self.emit_error(localize("expectedFlowSeqEnd", "Expected \"]\"", &[]), end_token.start_offset, end_token.end_offset, "expected-flow-seq-end");
        }

        YamlSequenceNode {
            items,
            style: CollectionStyle::Flow,
            start_offset: start_token.start_offset,
            end_offset: if closed { end_token.end_offset } else { end_token.start_offset },
        }
    }

    /// Parse a scalar inside a flow collection, handling multiline plain scalars.
    /// In flow context, plain (unquoted) scalars can span multiple lines;
    /// line breaks are folded into spaces.
    fn parse_flow_scalar(&mut self) -> YamlScalarNode {
        let token = self.advance();
        // Quoted scalars are complete as-is (scanner handles their multiline folding)
        if token.format != ScalarFormat::Plain {
            return self.scalar_from_token(&token);
        }
        // For unquoted (plain) scalars, fold continuation lines across newlines
        let mut value: Vec<u16> = token.value.units().to_vec();
        let mut end_offset = token.end_offset;

        loop {
            // Look ahead for a newline followed by a plain scalar continuation
            let mut has_newline = false;
            let mut p = self.pos;
            while p < self.tokens.len() {
                match self.tokens[p].ty {
                    TokenType::Newline => {
                        has_newline = true;
                        p += 1;
                    }
                    TokenType::Indent | TokenType::Comment => p += 1,
                    _ => break,
                }
            }

            if !has_newline || p >= self.tokens.len() {
                break;
            }

            let next = &self.tokens[p];
            if next.ty == TokenType::Scalar && next.format == ScalarFormat::Plain {
                // Fold continuation line into the scalar
                value.push(u16::from(b' '));
                value.extend_from_slice(next.value.units());
                end_offset = next.end_offset;
                self.pos = p + 1;
            } else {
                break;
            }
        }

        YamlScalarNode {
            value: JsString::from_units(value),
            raw_value: substring(self.input, token.start_offset, end_offset),
            start_offset: token.start_offset,
            end_offset,
            format: ScalarFormat::Plain,
        }
    }

    /// Parse a value in flow context (used after colon in flow mappings/implicit mappings).
    fn parse_flow_value(&mut self) -> YamlNode {
        match self.cur_type() {
            TokenType::FlowMapStart => YamlNode::Map(self.parse_flow_map()),
            TokenType::FlowSeqStart => YamlNode::Sequence(self.parse_flow_seq()),
            TokenType::Scalar => YamlNode::Scalar(self.parse_flow_scalar()),
            _ => YamlNode::Scalar(make_empty_scalar(self.cur().start_offset)),
        }
    }

    /// Skip whitespace, newlines, and comments inside flow collections.
    fn skip_flow_whitespace(&mut self) {
        while matches!(self.cur_type(), TokenType::Newline | TokenType::Indent | TokenType::Comment) {
            self.advance();
        }
    }

    fn scalar_from_token(&self, token: &Token) -> YamlScalarNode {
        YamlScalarNode {
            value: token.value.clone(),
            raw_value: substring(self.input, token.start_offset, token.end_offset),
            start_offset: token.start_offset,
            end_offset: token.end_offset,
            format: token.format,
        }
    }
}

/// Folding de linhas: linhas vazias viram `\n` cada uma, e uma quebra simples vira espaço.
fn fold_separator(value: &mut Vec<u16>, empty_line_count: usize) {
    if empty_line_count > 0 {
        value.extend(std::iter::repeat_n(0x0A, empty_line_count));
    } else {
        value.push(0x20);
    }
}

fn make_empty_scalar(offset: usize) -> YamlScalarNode {
    YamlScalarNode {
        value: JsString::new(),
        raw_value: JsString::new(),
        start_offset: offset,
        end_offset: offset,
        format: ScalarFormat::Plain,
    }
}
