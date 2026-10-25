//! O `Renderer`, o `TextRenderer` e o `Parser` do `marked` v14.0.0: a árvore de tokens vira HTML, como o
//! `marked.parse` e o `marked.parseInline` do original.
//!
//! O renderizador padrão é o único: as opções `renderer`, `extensions`, `hooks`, `walkTokens` e `async` não
//! existem (ver [`Options`]); quem estende o renderizador implementa [`InlineRenderer`] e o passa a
//! [`Parser::with_renderer`], e o `walkTokens` é um laço sobre a [`TokensList`](crate::marked::TokensList) antes do
//! `Parser`. Um `code` com `escaped` (que o `walkTokens` de quem destaca a sintaxe define) mantém o texto como está.
//! Os trechos de texto são UTF-16, como o resto do módulo.

use crate::JsString;
use crate::js_string::is_js_whitespace;
use crate::marked::{Code, MarkedError, Options, Token, lex_inline, lexer_with};

/// Uma unidade UTF-16 de um caractere ASCII.
const fn u(c: char) -> u16 {
    c as u16
}

/// O texto sendo montado.
#[derive(Default)]
struct Buf(Vec<u16>);

impl Buf {
    fn str(&mut self, text: &str) -> &mut Self {
        self.0.extend(text.encode_utf16());
        self
    }

    fn js(&mut self, text: &JsString) -> &mut Self {
        self.0.extend_from_slice(text.units());
        self
    }

    fn units(&mut self, units: &[u16]) -> &mut Self {
        self.0.extend_from_slice(units);
        self
    }

    fn done(self) -> JsString {
        JsString::from_units(self.0)
    }
}

/// `(\d{1,7}|[Xx]hex{1,6}|\w+);` depois de um `&`: o `&` já começa uma entidade e não se escapa.
fn is_entity_after_ampersand(rest: &[u16]) -> bool {
    let is_digit = |c: u16| (u('0')..=u('9')).contains(&c);
    let is_hex = |c: u16| is_digit(c) || (u('a')..=u('f')).contains(&c) || (u('A')..=u('F')).contains(&c);
    let is_word = |c: u16| is_digit(c) || (u('a')..=u('z')).contains(&c) || (u('A')..=u('Z')).contains(&c) || c == u('_');
    // `#\d{1,7};`
    if rest.first() == Some(&u('#')) {
        let after = &rest[1..];
        let digits = after.iter().take_while(|&&c| is_digit(c)).count();
        if (1..=7).contains(&digits) && after.get(digits) == Some(&u(';')) {
            return true;
        }
        // `#[Xx][a-fA-F0-9]{1,6};`
        if after.first().is_some_and(|&c| c == u('x') || c == u('X')) {
            let hex = after[1..].iter().take_while(|&&c| is_hex(c)).count();
            if (1..=6).contains(&hex) && after.get(1 + hex) == Some(&u(';')) {
                return true;
            }
        }
        return false;
    }
    // `\w+;`
    let word = rest.iter().take_while(|&&c| is_word(c)).count();
    word >= 1 && rest.get(word) == Some(&u(';'))
}

/// `escape(html, encode)`: com `encode`, escapa todo `&`, `<`, `>`, `"` e `'`; sem `encode`, deixa em paz o `&`
/// que já começa uma entidade (`&amp;`, `&#39;`).
pub fn escape(html: &[u16], encode: bool) -> JsString {
    let mut out = Vec::with_capacity(html.len());
    for (i, &c) in html.iter().enumerate() {
        let replacement = match char::from_u32(u32::from(c)) {
            Some('&') if encode || !is_entity_after_ampersand(&html[i + 1..]) => Some("&amp;"),
            Some('<') => Some("&lt;"),
            Some('>') => Some("&gt;"),
            Some('"') => Some("&quot;"),
            Some('\'') => Some("&#39;"),
            _ => None,
        };
        match replacement {
            Some(text) => out.extend(text.encode_utf16()),
            None => out.push(c),
        }
    }
    JsString::from_units(out)
}

/// `cleanUrl(href)`: `encodeURI(href).replace(/%25/g, '%')`; `None` se o `encodeURI` lança (surrogate isolado).
fn clean_url(href: &[u16]) -> Option<Vec<u16>> {
    let mut out: Vec<u16> = Vec::with_capacity(href.len());
    for decoded in char::decode_utf16(href.iter().copied()) {
        let c = decoded.ok()?;
        let keep = c.is_ascii_alphanumeric() || matches!(c, ';' | ',' | '/' | '?' | ':' | '@' | '&' | '=' | '+' | '$' | '-' | '_' | '.' | '!' | '~' | '*' | '\'' | '(' | ')' | '#');
        if keep {
            out.push(c as u16);
        } else if c == '%' {
            // `%` vira `%25` e o `.replace(/%25/g, '%')` o desfaz.
            out.push(u('%'));
        } else {
            let mut buf = [0u8; 4];
            for byte in c.encode_utf8(&mut buf).bytes() {
                out.extend(format!("%{byte:02X}").encode_utf16());
            }
        }
    }
    // `encodeURI` escreveu `%25` para cada `%` do original; o `.replace(/%25/g, '%')` também desfaz um `%25` que
    // já estava no texto. Aqui os dois efeitos são o mesmo: o `%` do original fica como está.
    Some(out)
}

/// O que `Parser.parseInline` precisa de um renderizador: o `Renderer` e o `TextRenderer`.
pub trait InlineRenderer {
    fn text(&self, parser: &Parser, token: &Token) -> JsString;
    fn html(&self, token: &Token) -> JsString;
    fn link(&self, parser: &Parser, token: &Token) -> JsString;
    fn image(&self, token: &Token) -> JsString;
    fn strong(&self, parser: &Parser, token: &Token) -> JsString;
    fn em(&self, parser: &Parser, token: &Token) -> JsString;
    fn codespan(&self, token: &Token) -> JsString;
    fn br(&self) -> JsString;
    fn del(&self, parser: &Parser, token: &Token) -> JsString;

    /// `renderer.code`: o bloco de código. Quem renderiza só inline não precisa trocar.
    fn code(&self, token: &Code) -> JsString {
        Renderer.code(token)
    }

    /// `renderer.heading`: `this.parser.parseInline(tokens)` é o `parser.parse_inline`.
    fn heading(&self, parser: &Parser, token: &crate::marked::Heading) -> Result<JsString, MarkedError> {
        let mut out = Buf::default();
        out.str(&format!("<h{}>", token.depth)).js(&parser.parse_inline(&token.tokens)?).str(&format!("</h{}>\n", token.depth));
        Ok(out.done())
    }

    /// `renderer.blockquote`.
    fn blockquote(&self, parser: &Parser, token: &crate::marked::Blockquote) -> Result<JsString, MarkedError> {
        let mut out = Buf::default();
        out.str("<blockquote>\n").js(&parser.parse(&token.tokens, true)?).str("</blockquote>\n");
        Ok(out.done())
    }

    /// `renderer.html` de um token `html` de bloco.
    fn block_html(&self, token: &crate::marked::Html) -> JsString {
        token.text.clone()
    }

    /// `renderer.paragraph`.
    fn paragraph(&self, parser: &Parser, token: &crate::marked::Paragraph) -> Result<JsString, MarkedError> {
        let mut out = Buf::default();
        out.str("<p>").js(&parser.parse_inline(&token.tokens)?).str("</p>\n");
        Ok(out.done())
    }
}

/// `Renderer`: o renderizador HTML padrão.
pub struct Renderer;

fn only_text(token: &Token) -> JsString {
    token.text().cloned().unwrap_or_default()
}

impl Renderer {
    /// `Renderer.code`: o texto de um token com `escaped` já vem em HTML e não é escapado de novo.
    pub fn code(&self, token: &Code) -> JsString {
        let lang = token.lang.as_ref().map_or(&[][..], |l| l.units());
        // `(lang || '').match(/^\S*/)[0]`
        let lang_len = lang.iter().take_while(|&&c| !is_js_whitespace(c)).count();
        let lang_string = &lang[..lang_len];
        let mut text = token.text.units().to_vec();
        if text.last() == Some(&10) {
            text.pop();
        }
        text.push(10);
        let body = if token.escaped { JsString::from(&text[..]) } else { escape(&text, true) };
        let mut out = Buf::default();
        if lang_string.is_empty() {
            out.str("<pre><code>").js(&body).str("</code></pre>\n");
        } else {
            out.str("<pre><code class=\"language-").js(&escape(lang_string, false)).str("\">").js(&body).str("</code></pre>\n");
        }
        out.done()
    }

    fn list(&self, parser: &Parser, token: &crate::marked::List) -> Result<JsString, MarkedError> {
        let mut body = Buf::default();
        for item in &token.items {
            if let Token::ListItem(item) = item {
                body.js(&self.listitem(parser, item)?);
            }
        }
        let type_ = if token.ordered { "ol" } else { "ul" };
        let mut out = Buf::default();
        out.str("<").str(type_);
        if let Some(start) = token.start.filter(|&s| token.ordered && s != 1) {
            out.str(" start=\"").str(&start.to_string()).str("\"");
        }
        out.str(">\n").js(&body.done()).str("</").str(type_).str(">\n");
        Ok(out.done())
    }

    fn listitem(&self, parser: &Parser, item: &crate::marked::ListItem) -> Result<JsString, MarkedError> {
        let mut item_body = Buf::default();
        let mut tokens = item.tokens.clone();
        if item.task {
            let checkbox = self.checkbox(item.checked == Some(true));
            if item.loose {
                let mut handled = false;
                if let Some(Token::Paragraph(paragraph)) = tokens.first_mut() {
                    let mut text = Buf::default();
                    text.js(&checkbox).str(" ").js(&paragraph.text);
                    paragraph.text = text.done();
                    if let Some(Token::Text(first)) = paragraph.tokens.first_mut() {
                        let mut inner = Buf::default();
                        inner.js(&checkbox).str(" ").js(&first.text);
                        first.text = inner.done();
                    }
                    handled = true;
                }
                if !handled {
                    let mut text = Buf::default();
                    text.js(&checkbox).str(" ");
                    let text = text.done();
                    tokens.insert(0, Token::Text(crate::marked::Text { raw: text.clone(), text, tokens: None, slot: None }));
                }
            } else {
                item_body.js(&checkbox).str(" ");
            }
        }
        item_body.js(&parser.parse(&tokens, item.loose)?);
        let mut out = Buf::default();
        out.str("<li>").js(&item_body.done()).str("</li>\n");
        Ok(out.done())
    }

    fn checkbox(&self, checked: bool) -> JsString {
        JsString::from(format!("<input {}disabled=\"\" type=\"checkbox\">", if checked { "checked=\"\" " } else { "" }))
    }

    fn table(&self, parser: &Parser, token: &crate::marked::Table) -> Result<JsString, MarkedError> {
        let mut cell = Buf::default();
        for header in &token.header {
            cell.js(&self.tablecell(parser, header)?);
        }
        let mut header = Buf::default();
        header.str("<tr>\n").js(&cell.done()).str("</tr>\n");

        let mut body = Buf::default();
        for row in &token.rows {
            let mut cell = Buf::default();
            for c in row {
                cell.js(&self.tablecell(parser, c)?);
            }
            body.str("<tr>\n").js(&cell.done()).str("</tr>\n");
        }
        let body = body.done();
        let mut out = Buf::default();
        out.str("<table>\n<thead>\n").js(&header.done()).str("</thead>\n");
        if !body.is_empty() {
            out.str("<tbody>").js(&body).str("</tbody>");
        }
        out.str("</table>\n");
        Ok(out.done())
    }

    fn tablecell(&self, parser: &Parser, token: &crate::marked::TableCell) -> Result<JsString, MarkedError> {
        let content = parser.parse_inline(&token.tokens)?;
        let type_ = if token.header { "th" } else { "td" };
        let mut out = Buf::default();
        match token.align {
            Some(align) => out.str(&format!("<{type_} align=\"{}\">", align.as_str())),
            None => out.str(&format!("<{type_}>")),
        };
        out.js(&content).str(&format!("</{type_}>\n"));
        Ok(out.done())
    }
}

impl InlineRenderer for Renderer {
    fn text(&self, parser: &Parser, token: &Token) -> JsString {
        match token.tokens() {
            Some(tokens) if matches!(token, Token::Text(_)) => parser.parse_inline(tokens).unwrap_or_default(),
            _ => only_text(token),
        }
    }

    fn html(&self, token: &Token) -> JsString {
        only_text(token)
    }

    fn link(&self, parser: &Parser, token: &Token) -> JsString {
        let Token::Link(link) = token else { return JsString::new() };
        let text = parser.parse_inline(&link.tokens).unwrap_or_default();
        let Some(href) = clean_url(link.href.units()) else { return text };
        let mut out = Buf::default();
        out.str("<a href=\"").units(&href).str("\"");
        if let Some(title) = link.title.as_ref().filter(|t| !t.is_empty()) {
            out.str(" title=\"").js(title).str("\"");
        }
        out.str(">").js(&text).str("</a>");
        out.done()
    }

    fn image(&self, token: &Token) -> JsString {
        let Token::Image(image) = token else { return JsString::new() };
        let Some(href) = clean_url(image.href.units()) else { return image.text.clone() };
        let mut out = Buf::default();
        out.str("<img src=\"").units(&href).str("\" alt=\"").js(&image.text).str("\"");
        if let Some(title) = image.title.as_ref().filter(|t| !t.is_empty()) {
            out.str(" title=\"").js(title).str("\"");
        }
        out.str(">");
        out.done()
    }

    fn strong(&self, parser: &Parser, token: &Token) -> JsString {
        let mut out = Buf::default();
        out.str("<strong>").js(&parser.parse_inline(token.tokens().unwrap_or_default()).unwrap_or_default()).str("</strong>");
        out.done()
    }

    fn em(&self, parser: &Parser, token: &Token) -> JsString {
        let mut out = Buf::default();
        out.str("<em>").js(&parser.parse_inline(token.tokens().unwrap_or_default()).unwrap_or_default()).str("</em>");
        out.done()
    }

    fn codespan(&self, token: &Token) -> JsString {
        let mut out = Buf::default();
        out.str("<code>").js(&only_text(token)).str("</code>");
        out.done()
    }

    fn br(&self) -> JsString {
        JsString::from("<br>")
    }

    fn del(&self, parser: &Parser, token: &Token) -> JsString {
        let mut out = Buf::default();
        out.str("<del>").js(&parser.parse_inline(token.tokens().unwrap_or_default()).unwrap_or_default()).str("</del>");
        out.done()
    }
}

/// `TextRenderer`: só a parte textual de cada token.
pub struct TextRenderer;

impl InlineRenderer for TextRenderer {
    fn text(&self, _parser: &Parser, token: &Token) -> JsString {
        only_text(token)
    }

    fn html(&self, token: &Token) -> JsString {
        only_text(token)
    }

    fn link(&self, _parser: &Parser, token: &Token) -> JsString {
        only_text(token)
    }

    fn image(&self, token: &Token) -> JsString {
        only_text(token)
    }

    fn strong(&self, _parser: &Parser, token: &Token) -> JsString {
        only_text(token)
    }

    fn em(&self, _parser: &Parser, token: &Token) -> JsString {
        only_text(token)
    }

    fn codespan(&self, token: &Token) -> JsString {
        only_text(token)
    }

    fn br(&self) -> JsString {
        JsString::new()
    }

    fn del(&self, _parser: &Parser, token: &Token) -> JsString {
        only_text(token)
    }
}

/// `Parser`: percorre os tokens e chama o renderizador.
pub struct Parser {
    options: Options,
    renderer: Box<dyn InlineRenderer>,
}

impl Parser {
    pub fn new(options: Options) -> Self {
        Self::with_renderer(options, Box::new(Renderer))
    }

    /// `new Parser({ renderer })`: o `Parser` com o renderizador trocado.
    pub fn with_renderer(options: Options, renderer: Box<dyn InlineRenderer>) -> Self {
        Self { options, renderer }
    }

    /// Um token que o laço não sabe tratar: com `silent` devolve `''` (o original o registra no `console`).
    fn unknown(&self, token: &Token) -> Result<JsString, MarkedError> {
        let error = MarkedError::UnknownToken(token.type_name());
        if self.options.silent {
            eprintln!("{error}");
            return Ok(JsString::new());
        }
        Err(error)
    }

    /// `parse(tokens, top)`: os tokens de bloco.
    pub fn parse(&self, tokens: &[Token], top: bool) -> Result<JsString, MarkedError> {
        let mut out = Buf::default();
        let mut i = 0;
        while i < tokens.len() {
            let token = &tokens[i];
            match token {
                Token::Space(_) => {}
                Token::Hr(_) => {
                    out.str("<hr>\n");
                }
                Token::Heading(heading) => {
                    out.js(&self.renderer.heading(self, heading)?);
                }
                Token::Code(code) => {
                    out.js(&self.renderer.code(code));
                }
                Token::Table(table) => {
                    out.js(&Renderer.table(self, table)?);
                }
                Token::Blockquote(quote) => {
                    out.js(&self.renderer.blockquote(self, quote)?);
                }
                Token::List(list) => {
                    out.js(&Renderer.list(self, list)?);
                }
                Token::Html(html) => {
                    out.js(&self.renderer.block_html(html));
                }
                Token::Paragraph(paragraph) => {
                    out.js(&self.renderer.paragraph(self, paragraph)?);
                }
                Token::Text(_) => {
                    let mut body = Buf::default();
                    body.js(&self.renderer.text(self, token));
                    while i + 1 < tokens.len() && matches!(tokens[i + 1], Token::Text(_)) {
                        i += 1;
                        body.str("\n").js(&self.renderer.text(self, &tokens[i]));
                    }
                    let body = body.done();
                    if top {
                        out.str("<p>").js(&body).str("</p>\n");
                    } else {
                        out.js(&body);
                    }
                }
                other => {
                    let unknown = self.unknown(other)?;
                    if self.options.silent {
                        return Ok(unknown);
                    }
                }
            }
            i += 1;
        }
        Ok(out.done())
    }

    /// `parseInline(tokens)` com o renderizador padrão.
    pub fn parse_inline(&self, tokens: &[Token]) -> Result<JsString, MarkedError> {
        self.parse_inline_with(tokens, self.renderer.as_ref())
    }

    /// `parseInline(tokens, renderer)`.
    pub fn parse_inline_with(&self, tokens: &[Token], renderer: &dyn InlineRenderer) -> Result<JsString, MarkedError> {
        let mut out = Buf::default();
        for token in tokens {
            match token {
                Token::Escape(_) | Token::Text(_) => {
                    out.js(&renderer.text(self, token));
                }
                Token::Html(_) => {
                    out.js(&renderer.html(token));
                }
                Token::Link(_) => {
                    out.js(&renderer.link(self, token));
                }
                Token::Image(_) => {
                    out.js(&renderer.image(token));
                }
                Token::Strong(_) => {
                    out.js(&renderer.strong(self, token));
                }
                Token::Em(_) => {
                    out.js(&renderer.em(self, token));
                }
                Token::Codespan(_) => {
                    out.js(&renderer.codespan(token));
                }
                Token::Br(_) => {
                    out.js(&renderer.br());
                }
                Token::Del(_) => {
                    out.js(&renderer.del(self, token));
                }
                other => {
                    let unknown = self.unknown(other)?;
                    if self.options.silent {
                        return Ok(unknown);
                    }
                }
            }
        }
        Ok(out.done())
    }
}

/// O erro que o `Marked.onError` mostra com `silent`: `<p>An error occurred:</p><pre>...</pre>`.
fn on_error(error: &MarkedError, options: &Options) -> Result<JsString, MarkedError> {
    if options.silent {
        let message = format!("{error}\nPlease report this to https://github.com/markedjs/marked.");
        let mut out = Buf::default();
        out.str("<p>An error occurred:</p><pre>").js(&escape(&message.encode_utf16().collect::<Vec<_>>(), true)).str("</pre>");
        return Ok(out.done());
    }
    Err(error.clone())
}

/// `marked.parse(src, options)`: o texto Markdown como HTML.
pub fn parse(src: &[u16], options: &Options) -> Result<JsString, MarkedError> {
    let result = lexer_with(src, options.clone()).and_then(|list| Parser::new(options.clone()).parse(&list.tokens, true));
    match result {
        Ok(html) => Ok(html),
        Err(error) => on_error(&error, options),
    }
}

/// `marked.parseInline(src, options)`: só o texto de linha como HTML (sem `<p>`).
pub fn parse_inline(src: &[u16], options: &Options) -> Result<JsString, MarkedError> {
    let result = lex_inline(src, options.clone()).and_then(|tokens| Parser::new(options.clone()).parse_inline(&tokens));
    match result {
        Ok(html) => Ok(html),
        Err(error) => on_error(&error, options),
    }
}
