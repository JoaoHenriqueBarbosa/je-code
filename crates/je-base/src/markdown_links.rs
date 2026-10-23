//! Porte de `src/vs/base/common/markdownLinks.ts`: reescreve o destino de links e imagens de um markdown
//! deixando o resto do fonte, em especial trechos e blocos de código, byte a byte intacto.
//!
//! As posições são em unidades UTF-16, como no `String` do JavaScript.

use crate::JsString;
use crate::js_string::{index_of, substring, trim};
use crate::marked::{self, Token, TokensList};

/// `IMarkdownEdit`: troca `[start, end)` por `replacement`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MarkdownEdit {
    pub start: usize,
    pub end: usize,
    pub replacement: JsString,
}

/// `{ start, end }` de um trecho do fonte.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SourceRange {
    pub start: usize,
    pub end: usize,
}

/// `IMarkdownRewriteContext`.
pub struct MarkdownRewriteContext<'a> {
    /// Trechos de código em linha e de blocos de código, cujo conteúdo ficou intacto.
    pub code_ranges: &'a [SourceRange],
    /// Trechos com definições de referência de link (o lexer as resolve e não emite token para elas).
    pub definition_ranges: &'a [SourceRange],
    /// Links e imagens cujo fonte não foi localizado, então nenhuma edição saiu para eles.
    pub unlocatable: &'a [&'a Token],
}

/// `IMarkdownRewriter`.
pub trait MarkdownRewriter {
    /// O fonte que substitui um link ou imagem (`token` é [`Token::Link`] ou [`Token::Image`]), ou `None`
    /// para manter como está.
    fn rewrite_link(&mut self, token: &Token) -> Option<JsString>;

    /// Edições extras, depois que a travessia mapeou o documento.
    fn additional_edits(&mut self, _markdown: &[u16], _context: &MarkdownRewriteContext<'_>) -> Vec<MarkdownEdit> {
        Vec::new()
    }
}

impl<F: FnMut(&Token) -> Option<JsString>> MarkdownRewriter for F {
    fn rewrite_link(&mut self, token: &Token) -> Option<JsString> {
        self(token)
    }
}

/// Tokens cujo fonte é consumido inteiro, sem procurar links dentro.
fn is_opaque(token: &Token) -> bool {
    matches!(
        token,
        Token::Code(_) | Token::Codespan(_) | Token::Html(_) | Token::Link(_) | Token::Image(_) | Token::Def(_)
    )
}

fn children_of(token: &Token) -> Vec<&Token> {
    match token {
        Token::List(list) if !list.items.is_empty() => list.items.iter().collect(),
        Token::Table(table) => table
            .header
            .iter()
            .chain(table.rows.iter().flatten())
            .flat_map(|cell| &cell.tokens)
            .collect(),
        other => other.tokens().map(|tokens| tokens.iter().collect()).unwrap_or_default(),
    }
}

/// O texto que o leitor viu, sem os marcadores de ênfase e de código.
pub fn markdown_tokens_to_plain_text(tokens: &[Token]) -> JsString {
    let mut text = JsString::new();
    for token in tokens {
        match token.tokens().filter(|children| !children.is_empty()) {
            Some(children) => text.push_units(markdown_tokens_to_plain_text(children).units()),
            None => text.push_units(token.text().unwrap_or_else(|| token.raw()).units()),
        }
    }
    text
}

/// Percorre a árvore em ordem de documento, medindo quanto fonte cada token consumiu, para que a edição
/// caia no token que a produziu. Procurar o texto do token acharia o primeiro parecido, em geral um link
/// escrito dentro de um trecho de código.
///
/// Limitação conhecida: o rótulo de um link que atravessa linhas dentro de citação ou lista perde os
/// prefixos de bloco no `raw`, a busca falha e o link fica como está.
fn collect_edits<'t>(
    tokens: &[&'t Token],
    markdown: &[u16],
    mut cursor: usize,
    rewriter: &mut dyn MarkdownRewriter,
    edits: &mut Vec<MarkdownEdit>,
    code_ranges: &mut Vec<SourceRange>,
    unlocatable: &mut Vec<&'t Token>,
) -> usize {
    for &token in tokens {
        if !is_opaque(token) {
            let children = children_of(token);
            if !children.is_empty() {
                cursor = collect_edits(&children, markdown, cursor, rewriter, edits, code_ranges, unlocatable);
                continue;
            }
        }

        let raw = token.raw();
        if raw.is_empty() {
            continue;
        }

        let is_link_or_image = matches!(token, Token::Link(_) | Token::Image(_));
        let Some(start) = index_of(markdown, raw.units(), cursor) else {
            if is_link_or_image {
                unlocatable.push(token);
            }
            continue;
        };
        let end = start + raw.len();
        cursor = end;

        if matches!(token, Token::Code(_) | Token::Codespan(_)) {
            code_ranges.push(SourceRange { start, end });
        } else if is_link_or_image {
            if let Some(replacement) = rewriter.rewrite_link(token) {
                edits.push(MarkdownEdit {
                    start,
                    end,
                    replacement,
                });
            } else {
                // O token continua como foi escrito, então o que há dentro dele (uma imagem com
                // destino próprio dentro de um link) ainda precisa ser visitado.
                collect_edits(
                    &children_of(token),
                    markdown,
                    start,
                    rewriter,
                    edits,
                    code_ranges,
                    unlocatable,
                );
            }
        }
    }
    cursor
}

/// Acha o fonte que o lexer consumiu sem emitir token, que é onde ficam as definições de referência: o
/// lexer as resolve ao ler e só as informa pelo mapa `links`, nunca como token.
///
/// Derivar os trechos do que o lexer fez, em vez de procurar a sintaxe de definição no documento todo,
/// pega cada definição inteira, seja como o autor quebrou o destino ou o título em linhas, e não confunde
/// um texto parecido escrito em prosa ou dentro de um bloco de código com uma definição.
fn find_definition_ranges(tokens: &[Token], markdown: &[u16]) -> Vec<SourceRange> {
    let mut ranges = Vec::new();
    let mut cursor = 0;
    for token in tokens {
        let raw = token.raw();
        let start = if raw.is_empty() {
            None
        } else {
            index_of(markdown, raw.units(), cursor)
        };
        let Some(start) = start else {
            // Um bloco que não se localiza deixaria tudo depois dele sem dono, o que passaria por uma
            // definição enorme. Não reivindica nada, em vez de cortar texto vivo.
            return Vec::new();
        };
        if start > cursor && !trim(substring(markdown, cursor as i64, start as i64)).is_empty() {
            ranges.push(SourceRange {
                start: cursor,
                end: start,
            });
        }
        cursor = start + raw.len();
    }

    if !trim(substring(markdown, cursor as i64, markdown.len() as i64)).is_empty() {
        ranges.push(SourceRange {
            start: cursor,
            end: markdown.len(),
        });
    }
    ranges
}

/// Reescreve o destino de links e imagens de um markdown, deixando o fonte de todo o resto, em especial
/// trechos e blocos de código, byte a byte intacto.
///
/// Os tokens cujo fonte não se localiza (o `marked` tira os prefixos de bloco do rótulo que atravessa
/// linhas numa lista ou citação) vão para [`MarkdownRewriteContext::unlocatable`], para quem não pode
/// emitir um destino cair num recurso mais grosseiro. Se o markdown não pode ser lexado, devolve o fonte
/// como veio.
pub fn rewrite_markdown_links(markdown: &[u16], rewriter: &mut dyn MarkdownRewriter) -> JsString {
    let TokensList { tokens, links: _ } = match marked::lexer(markdown) {
        Ok(list) => list,
        Err(_) => return JsString::from(markdown),
    };

    let mut edits: Vec<MarkdownEdit> = Vec::new();
    let mut code_ranges: Vec<SourceRange> = Vec::new();
    let mut unlocatable: Vec<&Token> = Vec::new();
    let top: Vec<&Token> = tokens.iter().collect();
    collect_edits(
        &top,
        markdown,
        0,
        rewriter,
        &mut edits,
        &mut code_ranges,
        &mut unlocatable,
    );
    let definition_ranges = find_definition_ranges(&tokens, markdown);
    let context = MarkdownRewriteContext {
        code_ranges: &code_ranges,
        definition_ranges: &definition_ranges,
        unlocatable: &unlocatable,
    };
    edits.extend(rewriter.additional_edits(markdown, &context));
    if edits.is_empty() {
        return JsString::from(markdown);
    }

    edits.sort_by_key(|edit| edit.start);

    let mut result = JsString::new();
    let mut position = 0;
    for edit in &edits {
        if edit.start < position {
            continue;
        }
        result.push_units(substring(markdown, position as i64, edit.start as i64));
        result.push_units(edit.replacement.units());
        position = edit.end;
    }
    result.push_units(substring(markdown, position as i64, markdown.len() as i64));
    result
}
