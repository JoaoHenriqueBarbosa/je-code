//! Porte de `src/vs/base/common/jsonFormatter.ts`: formata JSON com comentários devolvendo edições
//! mínimas (é o que o `ConfigurationService` usa para gravar o `settings.json` preservando o que o
//! usuário escreveu).
//!
//! Deslocamentos e comprimentos são em unidades UTF-16, como no original.

use crate::JsString;
use crate::dynamic::number_to_js_string;
use crate::json::{ScanError, Scanner, SyntaxKind, Value};

/// `FormattingOptions`. Campo ausente é `None`, como o campo opcional do original.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FormattingOptions {
    /// Número de espaços de um nível de indentação (usado com `insert_spaces`). Padrão 4.
    pub tab_size: Option<usize>,
    /// Indentação com espaços.
    pub insert_spaces: Option<bool>,
    /// Quebra de linha padrão; sem ela, `\n`.
    pub eol: Option<JsString>,
}

/// `Edit`: uma modificação de texto. Comprimento vazio é uma inserção, conteúdo vazio é uma remoção.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Edit {
    pub offset: usize,
    pub length: usize,
    pub content: JsString,
}

/// `Range`: um trecho do documento.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Range {
    pub offset: usize,
    pub length: usize,
}

fn tab_size(options: &FormattingOptions) -> usize {
    options.tab_size.filter(|&n| n != 0).unwrap_or(4)
}

fn repeat(s: &[u16], count: usize) -> Vec<u16> {
    s.repeat(count)
}

fn utf16(s: &str) -> Vec<u16> {
    s.encode_utf16().collect()
}

struct Formatter<'a> {
    scanner: Scanner<'a>,
    line_break: bool,
    has_error: bool,
}

impl Formatter<'_> {
    fn scan_next(&mut self) -> SyntaxKind {
        let mut token = self.scanner.scan();
        self.line_break = false;
        while token == SyntaxKind::Trivia || token == SyntaxKind::LineBreakTrivia {
            self.line_break = self.line_break || token == SyntaxKind::LineBreakTrivia;
            token = self.scanner.scan();
        }
        self.has_error = token == SyntaxKind::Unknown || self.scanner.token_error() != ScanError::None;
        token
    }
}

/// `format`: as edições (ordenadas, sem sobreposição) que formatam `document_text`, ou só o trecho `range`.
pub fn format(document_text: &[u16], range: Option<Range>, options: &FormattingOptions) -> Vec<Edit> {
    let (format_text, initial_indent_level, format_text_start, range_start, range_end);
    if let Some(range) = range {
        range_start = range.offset;
        range_end = range_start + range.length;
        let mut start = range_start;
        while start > 0 && !is_eol(document_text, start - 1) {
            start -= 1;
        }
        let mut end_offset = range_end;
        while end_offset < document_text.len() && !is_eol(document_text, end_offset) {
            end_offset += 1;
        }
        format_text_start = start;
        format_text = &document_text[start..end_offset.min(document_text.len())];
        initial_indent_level = compute_indent_level(format_text, options);
    } else {
        format_text = document_text;
        initial_indent_level = 0;
        format_text_start = 0;
        range_start = 0;
        range_end = document_text.len();
    }
    let eol = get_eol(options, document_text);

    let mut indent_level: i64 = 0;
    let indent_value: Vec<u16> =
        if options.insert_spaces == Some(true) { repeat(&[0x20], tab_size(options)) } else { vec![0x09] };

    let mut f = Formatter { scanner: Scanner::new(format_text, false), line_break: false, has_error: false };
    let mut edit_operations: Vec<Edit> = Vec::new();

    let new_line_and_indent = |indent_level: i64| -> Vec<u16> {
        let mut out = eol.units().to_vec();
        out.extend(repeat(&indent_value, (initial_indent_level as i64 + indent_level).max(0) as usize));
        out
    };
    let add_edit = |edits: &mut Vec<Edit>, has_error: bool, text: &[u16], start: usize, end: usize| {
        if !has_error && start < range_end && end > range_start && &document_text[start..end] != text {
            edits.push(Edit { offset: start, length: end - start, content: JsString::from_units(text.to_vec()) });
        }
    };

    let mut first_token = f.scan_next();

    if first_token != SyntaxKind::EOF {
        let first_token_start = f.scanner.token_offset() + format_text_start;
        let initial_indent = repeat(&indent_value, initial_indent_level);
        add_edit(&mut edit_operations, f.has_error, &initial_indent, format_text_start, first_token_start);
    }

    while first_token != SyntaxKind::EOF {
        let mut first_token_end = f.scanner.token_offset() + f.scanner.token_length() + format_text_start;
        let mut second_token = f.scan_next();

        let mut replace_content: Vec<u16> = Vec::new();
        while !f.line_break
            && (second_token == SyntaxKind::LineCommentTrivia || second_token == SyntaxKind::BlockCommentTrivia)
        {
            // Comentários na mesma linha: ficam na mesma linha, fora isso são ignorados.
            let comment_token_start = f.scanner.token_offset() + format_text_start;
            add_edit(&mut edit_operations, f.has_error, &[0x20], first_token_end, comment_token_start);
            first_token_end = f.scanner.token_offset() + f.scanner.token_length() + format_text_start;
            replace_content =
                if second_token == SyntaxKind::LineCommentTrivia { new_line_and_indent(indent_level) } else { Vec::new() };
            second_token = f.scan_next();
        }

        if second_token == SyntaxKind::CloseBraceToken {
            if first_token != SyntaxKind::OpenBraceToken {
                indent_level -= 1;
                replace_content = new_line_and_indent(indent_level);
            }
        } else if second_token == SyntaxKind::CloseBracketToken {
            if first_token != SyntaxKind::OpenBracketToken {
                indent_level -= 1;
                replace_content = new_line_and_indent(indent_level);
            }
        } else {
            let mut handled = false;
            match first_token {
                SyntaxKind::OpenBracketToken | SyntaxKind::OpenBraceToken => {
                    indent_level += 1;
                    replace_content = new_line_and_indent(indent_level);
                    handled = true;
                }
                SyntaxKind::CommaToken | SyntaxKind::LineCommentTrivia => {
                    replace_content = new_line_and_indent(indent_level);
                    handled = true;
                }
                SyntaxKind::BlockCommentTrivia => {
                    // Símbolo depois de comentário na mesma linha: continua nela, separado por espaço.
                    replace_content = if f.line_break { new_line_and_indent(indent_level) } else { vec![0x20] };
                    handled = true;
                }
                SyntaxKind::ColonToken => {
                    replace_content = vec![0x20];
                    handled = true;
                }
                SyntaxKind::StringLiteral if second_token == SyntaxKind::ColonToken => {
                    replace_content = Vec::new();
                    handled = true;
                }
                SyntaxKind::Unknown => {
                    f.has_error = true;
                    handled = true;
                }
                _ => {}
            }
            if !handled
                && matches!(
                    first_token,
                    SyntaxKind::StringLiteral
                        | SyntaxKind::NullKeyword
                        | SyntaxKind::TrueKeyword
                        | SyntaxKind::FalseKeyword
                        | SyntaxKind::NumericLiteral
                        | SyntaxKind::CloseBraceToken
                        | SyntaxKind::CloseBracketToken
                )
            {
                if second_token == SyntaxKind::LineCommentTrivia || second_token == SyntaxKind::BlockCommentTrivia {
                    replace_content = vec![0x20];
                } else if second_token != SyntaxKind::CommaToken && second_token != SyntaxKind::EOF {
                    f.has_error = true;
                }
            }
            if f.line_break
                && (second_token == SyntaxKind::LineCommentTrivia || second_token == SyntaxKind::BlockCommentTrivia)
            {
                replace_content = new_line_and_indent(indent_level);
            }
        }
        let second_token_start = f.scanner.token_offset() + format_text_start;
        add_edit(&mut edit_operations, f.has_error, &replace_content, first_token_end, second_token_start);
        first_token = second_token;
    }
    edit_operations
}

/// `JSON.stringify(value, undefined, indent)`: `indent` é o texto de um nível (vazio ou `None` é compacto).
pub fn json_stringify(value: &Value, indent: Option<&str>) -> String {
    let mut out = String::new();
    stringify_into(&mut out, value, indent.filter(|s| !s.is_empty()), 0);
    out
}

fn stringify_into(out: &mut String, value: &Value, indent: Option<&str>, depth: usize) {
    let newline = |out: &mut String, depth: usize| {
        if let Some(indent) = indent {
            out.push('\n');
            out.push_str(&indent.repeat(depth));
        }
    };
    match value {
        Value::Null => out.push_str("null"),
        Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Value::Number(n) => out.push_str(&if n.is_finite() { number_to_js_string(*n) } else { "null".to_string() }),
        Value::String(s) => out.push_str(&crate::dynamic::json_quote(s)),
        Value::Array(items) => {
            if items.is_empty() {
                out.push_str("[]");
                return;
            }
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                newline(out, depth + 1);
                stringify_into(out, item, indent, depth + 1);
            }
            newline(out, depth);
            out.push(']');
        }
        Value::Object(object) => {
            if object.is_empty() {
                out.push_str("{}");
                return;
            }
            out.push('{');
            for (i, (key, item)) in object.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                newline(out, depth + 1);
                out.push_str(&crate::dynamic::json_quote(key));
                out.push(':');
                if indent.is_some() {
                    out.push(' ');
                }
                stringify_into(out, item, indent, depth + 1);
            }
            newline(out, depth);
            out.push('}');
        }
    }
}

/// `toFormattedString`: o objeto serializado e formatado segundo `options`.
pub fn to_formatted_string(value: &Value, options: &FormattingOptions) -> JsString {
    let indent = if options.insert_spaces == Some(true) { " ".repeat(tab_size(options)) } else { "\t".to_string() };
    let content = json_stringify(value, Some(&indent));
    let Some(eol) = &options.eol else {
        return JsString::from(content);
    };
    // content.replace(/\r\n|\r|\n/g, options.eol)
    let units = utf16(&content);
    let mut out = Vec::with_capacity(units.len());
    let mut i = 0;
    while i < units.len() {
        match units[i] {
            0x0D => {
                if units.get(i + 1) == Some(&0x0A) {
                    i += 1;
                }
                out.extend_from_slice(eol.units());
            }
            0x0A => out.extend_from_slice(eol.units()),
            u => out.push(u),
        }
        i += 1;
    }
    JsString::from_units(out)
}

fn compute_indent_level(content: &[u16], options: &FormattingOptions) -> usize {
    let tab_size = tab_size(options);
    let mut n_chars = 0;
    for &ch in content {
        match ch {
            0x20 => n_chars += 1,
            0x09 => n_chars += tab_size,
            _ => break,
        }
    }
    n_chars / tab_size
}

/// `getEOL`: a quebra de linha que o texto já usa, ou a das opções (padrão `\n`).
pub fn get_eol(options: &FormattingOptions, text: &[u16]) -> JsString {
    for (i, &ch) in text.iter().enumerate() {
        if ch == 0x0D {
            return if text.get(i + 1) == Some(&0x0A) { JsString::from("\r\n") } else { JsString::from("\r") };
        } else if ch == 0x0A {
            return JsString::from("\n");
        }
    }
    match &options.eol {
        Some(eol) if !eol.is_empty() => eol.clone(),
        _ => JsString::from("\n"),
    }
}

/// `isEOL`. Fora do texto o `charAt` do JavaScript devolve `''`, e `'\r\n'.indexOf('')` é 0: verdadeiro.
pub fn is_eol(text: &[u16], offset: usize) -> bool {
    text.get(offset).is_none_or(|&ch| ch == 0x0D || ch == 0x0A)
}
