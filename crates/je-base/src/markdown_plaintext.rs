//! Porte de `renderAsPlaintext` de `src/vs/base/browser/markdownRenderer.ts`: o texto que um leitor vê de um trecho de
//! Markdown (`# Header` vira `Header`), sem opções (as opções `includeCodeBlocksFences`, `useLinkFormatter` e
//! `omitMarkdownSyntax` não são lidas por nenhum chamador portado).
//!
//! Divergências:
//! - O original é `marked.parse` com o renderer de texto puro (`createPlainTextRenderer`), `sanitizeRenderedMarkdown` e a
//!   troca das entidades por `unescapeInfo`. Aqui os blocos passam por [`plain_text_blocks`] (a mesma regra de `Parser.parse`,
//!   com cada `renderer.*` do texto puro) e os trechos de linha pelo [`PlainTextRenderer`] (um [`InlineRenderer`]).
//! - O `sanitizeRenderedMarkdown` (DOMPurify) sobre um HTML que só tem texto escapado só decodifica as entidades: a
//!   decodificação é [`decode_entities`] (as seis de `unescapeInfo`, as numéricas e as nomeadas da tabela HTML).
//! - Valores com mais de 100 000 unidades UTF-16 são cortados e terminam em `…`, como no original.

use crate::JsString;
use crate::browser::html_entities::{named_entity_with_semicolon, named_entity_without_semicolon};
use crate::marked::{InlineRenderer, Options, Parser, Token, escape, lexer_with};

/// O `Renderer` de `createPlainTextRenderer` para os tokens de linha.
struct PlainTextRenderer;

impl InlineRenderer for PlainTextRenderer {
    fn text(&self, _parser: &Parser, token: &Token) -> JsString {
        token.text().cloned().unwrap_or_default()
    }

    fn html(&self, _token: &Token) -> JsString {
        JsString::new()
    }

    fn link(&self, _parser: &Parser, token: &Token) -> JsString {
        token.text().cloned().unwrap_or_default()
    }

    fn image(&self, _token: &Token) -> JsString {
        JsString::new()
    }

    fn strong(&self, _parser: &Parser, token: &Token) -> JsString {
        token.text().cloned().unwrap_or_default()
    }

    fn em(&self, _parser: &Parser, token: &Token) -> JsString {
        token.text().cloned().unwrap_or_default()
    }

    fn codespan(&self, token: &Token) -> JsString {
        token.text().cloned().unwrap_or_default()
    }

    fn br(&self) -> JsString {
        JsString::from("\n")
    }

    fn del(&self, _parser: &Parser, token: &Token) -> JsString {
        token.text().cloned().unwrap_or_default()
    }
}

fn inline(parser: &Parser, tokens: &[Token]) -> String {
    parser.parse_inline_with(tokens, &PlainTextRenderer).map(|text| text.to_string_lossy()).unwrap_or_default()
}

/// As opções de `renderAsPlaintext`. `useLinkFormatter` não é lida por nenhum chamador portado e fica de fora.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PlaintextOptions {
    /// Mantém as cercas ``` dos blocos de código na saída.
    pub include_code_blocks_fences: bool,
    /// Reduz a sintaxe do Markdown ao texto também nos itens de lista (`parsedListItem` e `parsedText`).
    pub omit_markdown_syntax: bool,
}

/// `Parser.parse(tokens, top)` com os `renderer.*` de bloco do texto puro.
fn plain_text_blocks(parser: &Parser, tokens: &[Token], options: PlaintextOptions) -> String {
    let mut out = String::new();
    let mut i = 0;
    while i < tokens.len() {
        match &tokens[i] {
            Token::Space(_) | Token::Hr(_) | Token::Html(_) => {}
            Token::Heading(heading) => {
                out.push_str(&inline(parser, &heading.tokens));
                out.push('\n');
            }
            Token::Code(code) => {
                let escaped = escape(code.text.units(), true).to_string_lossy();
                if options.include_code_blocks_fences {
                    out.push_str(&format!("\n```\n{escaped}\n```\n"));
                } else {
                    out.push_str(&escaped);
                }
            }
            Token::Table(table) => {
                let cell = |cell: &crate::marked::TableCell| inline(parser, &cell.tokens);
                out.push_str(&table.header.iter().map(cell).collect::<Vec<_>>().join(" "));
                out.push('\n');
                out.push_str(&table.rows.iter().map(|cells| cells.iter().map(cell).collect::<Vec<_>>().join(" ")).collect::<Vec<_>>().join("\n"));
                out.push('\n');
            }
            Token::Blockquote(quote) => {
                out.push_str(&quote.text.to_string_lossy());
                out.push('\n');
            }
            Token::List(list) => {
                let items: Vec<String> = list
                    .items
                    .iter()
                    .filter_map(|item| match item {
                        Token::ListItem(item) if options.omit_markdown_syntax => Some(plain_text_blocks(parser, &item.tokens, options)),
                        Token::ListItem(item) => Some(format!("{}\n", item.text.to_string_lossy())),
                        _ => None,
                    })
                    .collect();
                out.push_str(&items.join("\n"));
                out.push('\n');
            }
            Token::Paragraph(paragraph) => {
                out.push_str(&inline(parser, &paragraph.tokens));
                out.push('\n');
            }
            Token::Text(_) => {
                let block_text = |token: &Token| match token {
                    Token::Text(text) => match &text.tokens {
                        Some(tokens) if options.omit_markdown_syntax => inline(parser, tokens),
                        _ => text.text.to_string_lossy(),
                    },
                    _ => String::new(),
                };
                let mut body = block_text(&tokens[i]);
                while i + 1 < tokens.len() && matches!(tokens[i + 1], Token::Text(_)) {
                    i += 1;
                    body.push('\n');
                    body.push_str(&block_text(&tokens[i]));
                }
                // top: o parágrafo do texto puro (`parseInline` de um token de texto escapado) mais `\n`
                out.push_str(&body);
                out.push('\n');
            }
            _ => {}
        }
        i += 1;
    }
    out
}

/// O `.replace(/&(#\d+|[a-zA-Z]+);/g, m => unescapeInfo.get(m) ?? m)` junto com a decodificação que o sanitizador faz.
fn decode_entities(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '&'
            && let Some(end) = chars[i + 1..].iter().position(|c| *c == ';')
            && end > 0
            && end <= 32
        {
            let name: String = chars[i + 1..i + 1 + end].iter().collect();
            let decoded = if let Some(number) = name.strip_prefix('#') {
                let code = match number.strip_prefix(['x', 'X']) {
                    Some(hex) => u32::from_str_radix(hex, 16).ok(),
                    None => number.parse::<u32>().ok(),
                };
                code.and_then(char::from_u32)
            } else {
                named_entity_with_semicolon(&name).or_else(|| named_entity_without_semicolon(&name)).and_then(char::from_u32)
            };
            if let Some(decoded) = decoded {
                out.push(decoded);
                i += end + 2;
                continue;
            }
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

/// `renderAsPlaintext(str)` para um `IMarkdownString` (`{ value }`) ou um texto, sem opções.
pub fn render_as_plaintext(value: &str) -> String {
    render_as_plaintext_with(value, PlaintextOptions::default())
}

/// `renderAsPlaintext(str, options)`.
pub fn render_as_plaintext_with(value: &str, plaintext_options: PlaintextOptions) -> String {
    // values that are too long will freeze the UI
    let units: Vec<u16> = value.encode_utf16().collect();
    let units: Vec<u16> = if units.len() > 100_000 {
        let mut cut = units[..100_000].to_vec();
        cut.push(0x2026);
        cut
    } else {
        units
    };

    let options = Options::default();
    let Ok(list) = lexer_with(&units, options.clone()) else {
        return String::new();
    };
    let parser = Parser::new(options);
    decode_entities(&plain_text_blocks(&parser, &list.tokens, plaintext_options)).trim().to_string()
}
