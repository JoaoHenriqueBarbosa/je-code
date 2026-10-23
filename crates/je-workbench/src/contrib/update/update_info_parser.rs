//! Porte de `src/vs/workbench/contrib/update/common/updateInfoParser.ts`: lê o texto de "novidades da
//! atualização" (envelope JSON ou markdown com metadados no começo) e devolve markdown, botões e os
//! campos opcionais do widget.
//!
//! Formatos aceitos:
//! - envelope JSON: um objeto com `markdown` e campos opcionais (`title`, `badge`, `bannerImageUrl`,
//!   `buttons`, `features`);
//! - frontmatter em bloco: `---`, uma linha, o JSON dos metadados, `---` e o corpo markdown;
//! - frontmatter em linha: `--- { ... } ---` e o corpo markdown.
//!
//! Decisões de tradução:
//! - `JSON.parse` é estrito (sem comentários, sem vírgula final, só espaço, tab, `\n` e `\r` entre os
//!   tokens). O parser do projeto (`je_base::json`) é tolerante, então [`json_parse`] liga as opções
//!   estritas, exige zero erros e confere o espaço em branco com o scanner.
//! - As duas expressões regulares do original (frontmatter em bloco e em linha) viram busca manual com a
//!   mesma semântica de guloso e preguiçoso: [`match_block_frontmatter`] e [`match_inline_frontmatter`].
//! - `String.prototype.trim` do JavaScript é [`js_trim`] (inclui `U+FEFF`, não inclui `U+0085`).
//! - Os campos de texto são `String`; um surrogate isolado vindo de um escape `\uD800` do JSON vira
//!   `U+FFFD`. Os argumentos de um botão (`unknown[]`) são valores JavaScript genéricos ([`Dynamic`]).
//! - O `deepStrictEqual` do original distingue propriedade ausente de propriedade `undefined`; num
//!   `Option` as duas coisas são `None`, e o resultado do parser é o mesmo nos dois casos.
//! - Os metadados do frontmatter podem ser qualquer valor JSON: `null` faz `meta.buttons` lançar
//!   `TypeError` (capturado, volta o texto inteiro como markdown) e os demais valores que não são objeto
//!   simplesmente não têm campos.

use je_base::dynamic::Dynamic;
use je_base::json::{self, Object, ParseOptions, Scanner, SyntaxKind, Value};

/// O número máximo de destaques (`MAX_FEATURES`) que o widget mostra.
const MAX_FEATURES: usize = 5;

/// `UpdateInfoButtonStyle`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UpdateInfoButtonStyle {
    Primary,
    Secondary,
}

/// `IUpdateInfoButton`.
#[derive(Clone, Debug, PartialEq)]
pub struct UpdateInfoButton {
    pub label: String,
    pub command_id: String,
    pub args: Option<Vec<Dynamic>>,
    pub style: Option<UpdateInfoButtonStyle>,
}

/// `IUpdateInfoFeature`.
#[derive(Clone, Debug, PartialEq)]
pub struct UpdateInfoFeature {
    /// Identificador de Codicon opcional (por exemplo `$(sparkle)`) mostrado ao lado do título.
    pub icon: Option<String>,
    /// Título curto do destaque.
    pub title: String,
    /// Descrição de uma linha.
    pub description: String,
}

/// `IParsedUpdateInfoInput`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ParsedUpdateInfoInput {
    /// Corpo markdown mostrado no widget.
    pub markdown: String,
    /// Botões de ação mostrados abaixo do markdown.
    pub buttons: Option<Vec<UpdateInfoButton>>,
    /// URL opcional da imagem de banner (o original só documenta `https://`, mas não filtra aqui).
    pub banner_image_url: Option<String>,
    /// Rótulo curto opcional (por exemplo `New`).
    pub badge: Option<String>,
    /// Título opcional acima do corpo markdown.
    pub title: Option<String>,
    /// Destaques, no máximo [`MAX_FEATURES`]; os excedentes são descartados em silêncio.
    pub features: Option<Vec<UpdateInfoFeature>>,
}

impl ParsedUpdateInfoInput {
    /// O resultado sem nenhum metadado: só o markdown.
    fn plain(markdown: &str) -> Self {
        Self { markdown: markdown.to_owned(), ..Self::default() }
    }
}

/// Parses optional metadata from update info input.
///
/// Tenta o envelope JSON e, se o texto não for um, cai no frontmatter. No máximo 5 destaques são
/// mantidos.
pub fn parse_update_info_input(text: &str) -> ParsedUpdateInfoInput {
    let normalized = text.strip_prefix('\u{FEFF}').unwrap_or(text);
    try_parse_update_info_envelope(normalized).unwrap_or_else(|| parse_update_info_frontmatter(normalized))
}

/// Caracteres que o `trim` do JavaScript remove: espaços (`WhiteSpace`, com `U+FEFF`) e terminadores de
/// linha. O `char::is_whitespace` do Rust inclui `U+0085`, que o JavaScript não trata como espaço.
fn is_js_whitespace(c: char) -> bool {
    c == '\u{FEFF}' || (c.is_whitespace() && c != '\u{85}')
}

/// `String.prototype.trim`.
fn js_trim(text: &str) -> &str {
    text.trim_matches(is_js_whitespace)
}

/// Os terminadores de linha que o `.` de uma expressão regular sem a flag `s` não casa.
fn is_line_terminator(c: char) -> bool {
    matches!(c, '\n' | '\r' | '\u{2028}' | '\u{2029}')
}

/// O espaço em branco entre tokens que o `JSON.parse` aceita (e só ele)?
fn has_non_json_whitespace(units: &[u16]) -> bool {
    let mut scanner = Scanner::new(units, false);
    loop {
        match scanner.scan() {
            SyntaxKind::EOF => return false,
            SyntaxKind::Trivia | SyntaxKind::LineBreakTrivia => {
                if scanner.token_value().units().iter().any(|&unit| !matches!(unit, 0x20 | 0x09 | 0x0A | 0x0D)) {
                    return true;
                }
            }
            _ => {}
        }
    }
}

/// `JSON.parse(text)`: `None` onde o original lança `SyntaxError`.
fn json_parse(text: &str) -> Option<Value> {
    let units: Vec<u16> = text.encode_utf16().collect();
    let mut errors = Vec::new();
    let options = ParseOptions { disallow_comments: true, allow_trailing_comma: false, allow_empty_content: false };
    let value = json::parse(&units, &mut errors, options)?;
    if !errors.is_empty() || has_non_json_whitespace(&units) {
        return None;
    }
    Some(value)
}

fn try_parse_update_info_envelope(text: &str) -> Option<ParsedUpdateInfoInput> {
    let trimmed = js_trim(text);
    if !trimmed.starts_with('{') || !trimmed.ends_with('}') {
        return None;
    }

    let Value::Object(object) = json_parse(trimmed)? else {
        return None;
    };
    let Some(Value::String(markdown)) = object.get("markdown") else {
        return None;
    };

    Some(build_parsed_input(markdown.to_string_lossy(), Some(&object)))
}

/// `meta` é `None` quando os metadados não são um objeto (então nenhum campo existe).
fn build_parsed_input(markdown: String, meta: Option<&Object>) -> ParsedUpdateInfoInput {
    let string_field = |key: &str| match meta?.get(key) {
        Some(Value::String(value)) => Some(value.to_string_lossy()),
        _ => None,
    };
    ParsedUpdateInfoInput {
        markdown,
        buttons: meta.and_then(|meta| meta.get("buttons")).and_then(parse_update_info_buttons),
        banner_image_url: string_field("bannerImageUrl"),
        badge: string_field("badge"),
        title: string_field("title"),
        features: meta.and_then(|meta| meta.get("features")).and_then(parse_update_info_features),
    }
}

/// Pula `[ \t]*` a partir de `from` e devolve o índice seguinte.
fn skip_blanks(text: &str, from: usize) -> usize {
    from + text.as_bytes()[from..].iter().take_while(|byte| matches!(byte, b' ' | b'\t')).count()
}

/// Casa `\r?\n` em `at` e devolve o índice logo depois.
fn line_break_end(text: &str, at: usize) -> Option<usize> {
    let bytes = text.as_bytes();
    let newline = if bytes.get(at) == Some(&b'\r') { at + 1 } else { at };
    (bytes.get(newline) == Some(&b'\n')).then_some(newline + 1)
}

/// Casa, em `at`, o fecho do frontmatter em bloco: `\r?\n---[ \t]*(?:\r?\n(?<body>[\s\S]*))?$`. Devolve o
/// corpo (vazio quando o texto acaba logo depois do fecho).
fn closing_fence(text: &str, at: usize) -> Option<&str> {
    let after_break = line_break_end(text, at)?;
    if !text[after_break..].starts_with("---") {
        return None;
    }
    let after_fence = skip_blanks(text, after_break + 3);
    if after_fence == text.len() {
        return Some("");
    }
    line_break_end(text, after_fence).map(|body_start| &text[body_start..])
}

/// `^---[ \t]*\r?\n(?<json>[\s\S]*?)\r?\n---[ \t]*(?:\r?\n(?<body>[\s\S]*))?$`: devolve `(json, body)`. O
/// `json` é preguiçoso, então vale o menor trecho cujo resto fecha o frontmatter.
fn match_block_frontmatter(text: &str) -> Option<(&str, &str)> {
    if !text.starts_with("---") {
        return None;
    }
    let json_start = line_break_end(text, skip_blanks(text, 3))?;
    let mut end = json_start;
    loop {
        if let Some(body) = closing_fence(text, end) {
            return Some((&text[json_start..end], body));
        }
        end += text[end..].chars().next()?.len_utf8();
    }
}

/// `^---[ \t]*(?<json>\{.*\})[ \t]*---[ \t]*(?<body>[\s\S]*)$`: devolve `(json, body)`. O `.*` é guloso e
/// não atravessa terminador de linha, então vale a última `}` da primeira linha seguida de `[ \t]*---`.
fn match_inline_frontmatter(text: &str) -> Option<(&str, &str)> {
    if !text.starts_with("---") {
        return None;
    }
    let open = skip_blanks(text, 3);
    if !text[open..].starts_with('{') {
        return None;
    }
    let line_end = text[open..].find(is_line_terminator).map_or(text.len(), |offset| open + offset);
    text[open + 1..line_end].rmatch_indices('}').find_map(|(offset, _)| {
        let close = open + 1 + offset;
        let after_blanks = skip_blanks(text, close + 1);
        text[after_blanks..].starts_with("---").then(|| (&text[open..=close], &text[skip_blanks(text, after_blanks + 3)..]))
    })
}

fn parse_update_info_frontmatter(text: &str) -> ParsedUpdateInfoInput {
    let Some((json_text, markdown)) = match_block_frontmatter(text).or_else(|| match_inline_frontmatter(text)) else {
        return ParsedUpdateInfoInput::plain(text);
    };

    match json_parse(json_text) {
        Some(Value::Object(meta)) => build_parsed_input(markdown.to_owned(), Some(&meta)),
        // JSON inválido, ou `null` (o `meta.buttons` do original lança `TypeError`): o texto inteiro é markdown.
        None | Some(Value::Null) => ParsedUpdateInfoInput::plain(text),
        Some(_) => build_parsed_input(markdown.to_owned(), None),
    }
}

fn parse_update_info_buttons(buttons: &Value) -> Option<Vec<UpdateInfoButton>> {
    let Value::Array(items) = buttons else {
        return None;
    };

    let parsed: Vec<UpdateInfoButton> = items
        .iter()
        .filter_map(|button| {
            let Value::Object(button) = button else {
                return None;
            };
            let (Some(Value::String(label)), Some(Value::String(command_id))) = (button.get("label"), button.get("commandId")) else {
                return None;
            };

            let style = match button.get("style") {
                Some(Value::String(style)) if *style == "primary" => Some(UpdateInfoButtonStyle::Primary),
                Some(Value::String(style)) if *style == "secondary" => Some(UpdateInfoButtonStyle::Secondary),
                _ => None,
            };
            let args = match button.get("args") {
                Some(Value::Array(args)) => Some(args.iter().map(Dynamic::from_json).collect()),
                _ => None,
            };
            Some(UpdateInfoButton { label: label.to_string_lossy(), command_id: command_id.to_string_lossy(), args, style })
        })
        .collect();

    (!parsed.is_empty()).then_some(parsed)
}

/// Parses an array of feature-highlight objects from raw update-info metadata. Only the first
/// [`MAX_FEATURES`] valid entries are returned; the rest are discarded. Each entry must have at minimum a
/// `title` and `description` string. The optional `icon` field accepts a Codicon identifier.
fn parse_update_info_features(features: &Value) -> Option<Vec<UpdateInfoFeature>> {
    let Value::Array(items) = features else {
        return None;
    };

    let mut parsed = Vec::new();
    for feature in items {
        let Value::Object(candidate) = feature else {
            continue;
        };
        let (Some(Value::String(title)), Some(Value::String(description))) = (candidate.get("title"), candidate.get("description")) else {
            continue;
        };
        let icon = match candidate.get("icon") {
            Some(Value::String(icon)) => Some(icon.to_string_lossy()),
            _ => None,
        };
        parsed.push(UpdateInfoFeature { icon, title: title.to_string_lossy(), description: description.to_string_lossy() });
        if parsed.len() >= MAX_FEATURES {
            break;
        }
    }

    (!parsed.is_empty()).then_some(parsed)
}
