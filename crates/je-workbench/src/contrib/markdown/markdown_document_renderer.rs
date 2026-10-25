//! Porte de `src/vs/workbench/contrib/markdown/browser/markdownDocumentRenderer.ts` (`renderMarkdownDocument` e
//! `DEFAULT_MARKDOWN_STYLES`) e de `markedGfmHeadingIdPlugin.ts`.
//!
//! Divergências:
//! - O `marked.Marked` com extensões não existe no porte: `renderMarkdownDocument` roda o `lexer_with`, percorre os tokens
//!   (o `walkTokens`), destaca os blocos de código e entrega o [`Parser`] com um [`InlineRenderer`] que junta as extensões na
//!   ordem do `marked` (a registrada por último tem a vez primeiro e passa adiante devolvendo `None`, o `undefined` ou o
//!   `false` do original). A extensão do `marked-highlight` e a do `markedGfmHeadingIdPlugin` são o próprio renderizador.
//! - `MarkedExtension` é o [`MarkedExtension`]: só o que `renderMarkdownDocument` recebe de quem o chama (`walkTokens` e os
//!   renderizadores `html`, `codespan` e `code`). A extensão `tokenizer.codespan` que marca os códigos fora de link e de
//!   bloco bruto é a leitura de [`Codespan::in_link`](je_base::marked::Codespan) e `in_raw_block`, que o tokenizador grava.
//! - `markedHighlight` com `async: true`: o `walkTokens` assíncrono vira um laço que espera cada bloco, na ordem do documento
//!   (o `Promise.all` do original não é observável por quem lê o resultado).
//! - `githubSlugReplaceRegex` (a lista literal de intervalos do `github-slugger`) é a classe Unicode equivalente
//!   `[^\p{L}\p{M}\p{Nd}\p{Nl}\p{Pc}\- ]`: tira tudo que não é letra, marca, dígito, número-letra, pontuação de conexão,
//!   hífen ou espaço. A tabela do original é a de uma versão do Unicode; a classe segue a do motor `zyarr`.
//! - `TrustedHTML` é o [`JsString`] do HTML sanitizado (o `sanitizeHtml` do porte devolve nós, e `inner_html` os serializa).

use std::rc::Rc;

use je_base::JsString;
use je_base::async_::{async_fn, race_cancellation_error};
use je_base::browser::html_parser::Element;
use je_base::cancellation::CancellationToken;
use je_base::errors::Error;
use je_base::js_regexp::replace_all;
use je_base::js_string::trim;
use je_base::marked::{
    Code, Codespan, Heading, Html, InlineRenderer, MarkedError, Options, Parser, Renderer, TextRenderer, Token, escape as marked_escape, lexer_with,
};
use je_base::markdown::dom::inner_html;
use je_base::markdown::sanitize::{AttributeRule, DomSanitizerConfig, Protocols, sanitize_html};
use je_base::markdown::{allowed_markdown_html_attributes, allowed_markdown_html_tags};
use je_base::network::schemas;
use je_base::promise::Promise;
use je_base::strings::escape;
use je_editor::language_service::ILanguageService;
use je_editor::text_to_html_tokenizer::tokenize_to_string;
use zyarr::RegExp;

use crate::contrib::update::update_info_parser::is_js_whitespace;
use crate::services::extensions::extensions::IExtensionService;

pub const DEFAULT_MARKDOWN_STYLES: &str = "
body {
	padding: 10px 20px;
	line-height: 22px;
	max-width: 882px;
	margin: 0 auto;
}

body *:last-child {
	margin-bottom: 0;
}

img {
	max-width: 100%;
	max-height: 100%;
}

a {
	text-decoration: var(--text-link-decoration);
}

a:hover {
	text-decoration: underline;
}

a:focus,
input:focus,
select:focus,
textarea:focus {
	outline: 1px solid -webkit-focus-ring-color;
	outline-offset: -1px;
}

hr {
	border: 0;
	height: 2px;
	border-bottom: 2px solid;
}

h1 {
	padding-bottom: 0.3em;
	line-height: 1.2;
	border-bottom-width: 1px;
	border-bottom-style: solid;
}

h1, h2, h3 {
	font-weight: normal;
}

table {
	border-collapse: collapse;
}

th {
	text-align: left;
	border-bottom: 1px solid;
}

th,
td {
	padding: 5px 10px;
}

table > tbody > tr + tr > td {
	border-top-width: 1px;
	border-top-style: solid;
}

blockquote {
	margin: 0 7px 0 5px;
	padding: 0 16px 0 10px;
	border-left-width: 5px;
	border-left-style: solid;
}

code {
	font-family: \"SF Mono\", Monaco, Menlo, Consolas, \"Ubuntu Mono\", \"Liberation Mono\", \"DejaVu Sans Mono\", \"Courier New\", monospace;
}

pre {
	padding: 16px;
	border-radius: 3px;
	overflow: auto;
}

pre code {
	font-family: var(--vscode-editor-font-family);
	font-weight: var(--vscode-editor-font-weight);
	font-size: var(--vscode-editor-font-size);
	line-height: 1.5;
	color: var(--vscode-editor-foreground);
	tab-size: 4;
}

.monaco-tokenized-source {
	white-space: pre;
}

/** Theming */

.pre {
	background-color: var(--vscode-textCodeBlock-background);
}

.vscode-high-contrast h1 {
	border-color: rgb(0, 0, 0);
}

.vscode-light th {
	border-color: rgba(0, 0, 0, 0.69);
}

.vscode-dark th {
	border-color: rgba(255, 255, 255, 0.69);
}

.vscode-light h1,
.vscode-light hr,
.vscode-light td {
	border-color: rgba(0, 0, 0, 0.18);
}

.vscode-dark h1,
.vscode-dark hr,
.vscode-dark td {
	border-color: rgba(255, 255, 255, 0.18);
}

@media (forced-colors: active) and (prefers-color-scheme: light){
	body {
		forced-color-adjust: none;
	}
}

@media (forced-colors: active) and (prefers-color-scheme: dark){
	body {
		forced-color-adjust: none;
	}
}
";

/// `MarkdownDocumentSanitizerConfig`: `allowedTags.augment` e `allowedAttributes.augment` são listas simples.
#[derive(Clone, Default)]
pub struct MarkdownDocumentSanitizerConfig {
    pub allowed_link_protocols_override: Option<Protocols>,
    pub allow_relative_link_paths: bool,
    pub allowed_media_protocols: Option<Protocols>,
    pub allow_relative_media_paths: bool,
    pub allowed_tags_augment: Vec<String>,
    pub allowed_attributes_augment: Vec<String>,
}

/// `marked.MarkedExtension`, no que `renderMarkdownDocument` recebe: o `walkTokens` e os renderizadores. Um método que devolve
/// `None` passa a vez à extensão registrada antes e, por fim, ao renderizador padrão.
pub trait MarkedExtension {
    /// `walkTokens(token)`, chamado para cada token (os de bloco e os de linha) em ordem de documento, antes de destacar o código.
    fn walk_token(&self, _token: &Token) {}

    /// `renderer.html`: o `raw` do token (de bloco ou de linha).
    fn html(&self, _raw: &str) -> Option<JsString> {
        None
    }

    /// `renderer.codespan`.
    fn codespan(&self, _token: &Codespan) -> Option<JsString> {
        None
    }

    /// `renderer.code`.
    fn code(&self, _token: &Code) -> Option<JsString> {
        None
    }
}

/// `IRenderMarkdownDocumentOptions`.
#[derive(Clone, Default)]
pub struct RenderMarkdownDocumentOptions {
    pub sanitizer_config: Option<MarkdownDocumentSanitizerConfig>,
    pub marked_extensions: Vec<Rc<dyn MarkedExtension>>,
}

/// A configuração do sanitizador que `sanitize(documentContent, sanitizerConfig)` monta.
pub fn markdown_document_dom_config(sanitizer_config: Option<&MarkdownDocumentSanitizerConfig>) -> DomSanitizerConfig<'static> {
    let default_allowed_link_protocols = Protocols::List(vec![schemas::HTTP.to_owned(), schemas::HTTPS.to_owned()]);
    let mut attributes = allowed_markdown_html_attributes();
    attributes.extend(["name", "id", "class", "role", "tabindex", "placeholder"].iter().map(|name| AttributeRule::Name((*name).to_owned())));
    DomSanitizerConfig {
        allowed_link_protocols: Some(
            sanitizer_config.and_then(|config| config.allowed_link_protocols_override.clone()).unwrap_or(default_allowed_link_protocols),
        ),
        allow_relative_link_paths: sanitizer_config.is_some_and(|config| config.allow_relative_link_paths),
        allowed_media_protocols: sanitizer_config.and_then(|config| config.allowed_media_protocols.clone()),
        allow_relative_media_paths: sanitizer_config.is_some_and(|config| config.allow_relative_media_paths),
        allowed_tags_override: Some(allowed_markdown_html_tags()),
        allowed_tags_augment: sanitizer_config.map(|config| config.allowed_tags_augment.clone()),
        allowed_attributes_override: Some(attributes),
        allowed_attributes_augment: Some(
            sanitizer_config
                .map(|config| config.allowed_attributes_augment.iter().map(|name| AttributeRule::Name(name.clone())).collect())
                .unwrap_or_default(),
        ),
        ..DomSanitizerConfig::default()
    }
}

/// `sanitize(documentContent, sanitizerConfig)`.
fn sanitize(document: &[u16], sanitizer_config: Option<&MarkdownDocumentSanitizerConfig>) -> JsString {
    serialize_nodes(sanitize_html(document, &markdown_document_dom_config(sanitizer_config)))
}

/// O `innerHTML` dos nós sanitizados (o `TrustedHTML` do `sanitizeHtml`).
pub fn serialize_nodes(nodes: Vec<je_base::browser::html_parser::Node>) -> JsString {
    let mut body = Element::new("body", Vec::new());
    body.children = nodes;
    inner_html(&body)
}

fn compile(pattern: &str, flags: &str) -> RegExp {
    RegExp::from_str(pattern, flags).unwrap_or_else(|error| panic!("expressão regular inválida /{pattern}/: {}", error.message()))
}

thread_local! {
    static UNESCAPE: RegExp = compile(r"&(#(?:\d+)|(?:#x[0-9A-Fa-f]+)|(?:\w+));?", "gi");
    static HTML_TAG: RegExp = compile(r"<[!\/a-z].*?>", "gi");
    static SLUG_REMOVE: RegExp = compile(r"[^\p{L}\p{M}\p{Nd}\p{Nl}\p{Pc}\- ]", "gu");
}

/// `unescape` de `markedGfmHeadingIdPlugin.ts` (os `unescape` dos helpers do `marked`): só `colon` e as referências numéricas
/// viram caractere; qualquer entidade nomeada some.
fn unescape(html: &[u16]) -> JsString {
    UNESCAPE.with(|regex| {
        replace_all(regex, html, |captures| {
            let name = String::from_utf16_lossy(captures.group(1).unwrap_or_default()).to_lowercase();
            if name == "colon" {
                return JsString::from(":");
            }
            let Some(number) = name.strip_prefix('#') else { return JsString::new() };
            let value = match number.strip_prefix('x') {
                Some(hex) => hex.chars().fold(0.0_f64, |acc, digit| acc * 16.0 + f64::from(digit.to_digit(16).unwrap_or(0))),
                None => number.chars().fold(0.0_f64, |acc, digit| acc * 10.0 + f64::from(digit.to_digit(10).unwrap_or(0))),
            };
            // `String.fromCharCode`: o número módulo 2^16.
            let unit = if value.is_finite() { value.rem_euclid(65536.0) as u16 } else { 0 };
            JsString::from_units(vec![unit])
        })
    })
}

/// `slugify(heading)`.
fn slugify(heading: &[u16]) -> JsString {
    let trimmed = String::from_utf16_lossy(trim(heading)).to_lowercase();
    let units: Vec<u16> = trimmed.encode_utf16().collect();
    let kept = SLUG_REMOVE.with(|regex| replace_all(regex, &units, |_| JsString::new()));
    // `.replace(/\s/g, '-')`: sobram só espaços comuns, os outros foram removidos.
    JsString::from_units(kept.units().iter().map(|&unit| if unit == u16::from(b' ') { u16::from(b'-') } else { unit }).collect())
}

/// `heading({ tokens, depth })` do `markedGfmHeadingIdPlugin`.
fn render_heading(parser: &Parser, token: &Heading) -> Result<JsString, MarkedError> {
    let text = parser.parse_inline(&token.tokens)?;
    let unescaped = unescape(parser.parse_inline_with(&token.tokens, &TextRenderer)?.units());
    let trimmed = trim(unescaped.units());
    let raw = HTML_TAG.with(|regex| replace_all(regex, trimmed, |_| JsString::new()));
    let id = slugify(raw.units());
    let mut out = JsString::from(format!("<h{} id=\"", token.depth).as_str());
    out.push_units(id.units());
    out.push_units(format!("\">").encode_utf16().collect::<Vec<u16>>().as_slice());
    out.push_units(text.units());
    out.push_units(format!("</h{}>\n", token.depth).encode_utf16().collect::<Vec<u16>>().as_slice());
    Ok(out)
}

/// O `renderer.code` do `marked-highlight`.
fn render_highlighted_code(token: &Code) -> JsString {
    let lang = token.lang.as_ref().map_or(&[][..], |lang| lang.units());
    let mut out = JsString::from("<pre><code");
    if !lang.is_empty() {
        out.push_units(" class=\"language-".encode_utf16().collect::<Vec<u16>>().as_slice());
        out.push_units(marked_escape(lang, false).units());
        out.push_units(&[u16::from(b'"')]);
    }
    out.push_units(&[u16::from(b'>')]);
    let mut text = token.text.units().to_vec();
    if text.last() == Some(&10) {
        text.pop();
    }
    if token.escaped {
        out.push_units(&text);
    } else {
        out.push_units(marked_escape(&text, true).units());
    }
    out.push_units("\n</code></pre>".encode_utf16().collect::<Vec<u16>>().as_slice());
    out
}

/// O renderizador do `Marked` de `renderMarkdownDocument`: as extensões do chamador (a última primeiro), o destaque de código e
/// o `id` dos títulos; o resto é o [`Renderer`] padrão.
struct DocumentRenderer {
    extensions: Vec<Rc<dyn MarkedExtension>>,
}

impl DocumentRenderer {
    fn html_of(&self, raw: &JsString) -> Option<JsString> {
        let raw = raw.to_string_lossy();
        self.extensions.iter().rev().find_map(|extension| extension.html(&raw))
    }
}

impl InlineRenderer for DocumentRenderer {
    fn text(&self, parser: &Parser, token: &Token) -> JsString {
        Renderer.text(parser, token)
    }

    fn html(&self, token: &Token) -> JsString {
        self.html_of(token.raw()).unwrap_or_else(|| Renderer.html(token))
    }

    fn link(&self, parser: &Parser, token: &Token) -> JsString {
        Renderer.link(parser, token)
    }

    fn image(&self, token: &Token) -> JsString {
        Renderer.image(token)
    }

    fn strong(&self, parser: &Parser, token: &Token) -> JsString {
        Renderer.strong(parser, token)
    }

    fn em(&self, parser: &Parser, token: &Token) -> JsString {
        Renderer.em(parser, token)
    }

    fn codespan(&self, token: &Token) -> JsString {
        if let Token::Codespan(codespan) = token
            && let Some(rendered) = self.extensions.iter().rev().find_map(|extension| extension.codespan(codespan))
        {
            return rendered;
        }
        Renderer.codespan(token)
    }

    fn br(&self) -> JsString {
        Renderer.br()
    }

    fn del(&self, parser: &Parser, token: &Token) -> JsString {
        Renderer.del(parser, token)
    }

    fn code(&self, token: &Code) -> JsString {
        self.extensions.iter().rev().find_map(|extension| extension.code(token)).unwrap_or_else(|| render_highlighted_code(token))
    }

    fn heading(&self, parser: &Parser, token: &Heading) -> Result<JsString, MarkedError> {
        render_heading(parser, token)
    }

    fn block_html(&self, token: &Html) -> JsString {
        self.html_of(&token.raw).unwrap_or_else(|| token.text.clone())
    }
}

/// `walkTokens` de `marked`: cada token e depois os filhos (as células da tabela, os itens da lista, `tokens`).
fn walk_tokens(tokens: &[Token], visit: &mut dyn FnMut(&Token)) {
    for token in tokens {
        visit(token);
        match token {
            Token::Table(table) => {
                for cell in &table.header {
                    walk_tokens(&cell.tokens, visit);
                }
                for row in &table.rows {
                    for cell in row {
                        walk_tokens(&cell.tokens, visit);
                    }
                }
            }
            Token::List(list) => walk_tokens(&list.items, visit),
            other => {
                if let Some(children) = other.tokens() {
                    walk_tokens(children, visit);
                }
            }
        }
    }
}

/// Os caminhos (índices de token, descendo por citações e itens de lista) dos blocos de código, em ordem de documento.
fn collect_code_paths(tokens: &[Token], path: &mut Vec<usize>, out: &mut Vec<Vec<usize>>) {
    for (index, token) in tokens.iter().enumerate() {
        path.push(index);
        match token {
            Token::Code(_) => out.push(path.clone()),
            Token::Blockquote(quote) => collect_code_paths(&quote.tokens, path, out),
            Token::List(list) => collect_code_paths(&list.items, path, out),
            Token::ListItem(item) => collect_code_paths(&item.tokens, path, out),
            _ => {}
        }
        path.pop();
    }
}

fn code_at_mut<'a>(tokens: &'a mut [Token], path: &[usize]) -> Option<&'a mut Code> {
    let (first, rest) = path.split_first()?;
    let token = tokens.get_mut(*first)?;
    if rest.is_empty() {
        return match token {
            Token::Code(code) => Some(code),
            _ => None,
        };
    }
    let children: &mut [Token] = match token {
        Token::Blockquote(quote) => &mut quote.tokens,
        Token::List(list) => &mut list.items,
        Token::ListItem(item) => &mut item.tokens,
        _ => return None,
    };
    code_at_mut(children, rest)
}

/// `lang.split(/\s+|:|,|(?!^)\{|\?]/, 1)[0]`.
fn language_name_head(lang: &str) -> String {
    let mut end = lang.len();
    for (position, (index, c)) in lang.char_indices().enumerate() {
        let is_separator = is_js_whitespace(c) || c == ':' || c == ',' || (c == '{' && position > 0) || (c == '?' && lang[index + c.len_utf8()..].starts_with(']'));
        if is_separator {
            end = index;
            break;
        }
    }
    lang[..end].to_owned()
}

/// `highlight(code, lang)` do `markedHighlight`.
async fn highlight(
    extension_service: &Rc<dyn IExtensionService>,
    language_service: &Rc<dyn ILanguageService>,
    code: &JsString,
    lang: Option<&JsString>,
    token: &CancellationToken,
) -> Result<JsString, Error> {
    let Some(lang) = lang else { return Ok(escape(code.units())) };

    extension_service.when_installed_extensions_registered().await?;
    if token.is_cancellation_requested() {
        return Ok(JsString::new());
    }

    let lang = lang.to_string_lossy();
    let language_id = language_service
        .get_language_id_by_language_name(&lang)
        .or_else(|| language_service.get_language_id_by_language_name(&language_name_head(&lang)));
    tokenize_to_string(language_service.as_ref(), code, language_id.as_deref()).await
}

fn marked_error(error: MarkedError) -> Error {
    Error::new(error.to_string())
}

/// Renders a string of markdown for use in an external document context.
///
/// Uses VS Code's syntax highlighting code blocks. Also does not attach all the hooks and customization that normal
/// markdown renderer.
pub fn render_markdown_document(
    text: &str,
    extension_service: Rc<dyn IExtensionService>,
    language_service: Rc<dyn ILanguageService>,
    options: Option<RenderMarkdownDocumentOptions>,
    token: CancellationToken,
) -> Promise<JsString> {
    let options = options.unwrap_or_default();
    let sanitizer_config = options.sanitizer_config.clone();
    let extensions = options.marked_extensions;
    let source: Vec<u16> = text.encode_utf16().collect();
    let race_token = token.clone();
    let parsed = async_fn(async move {
        let marked_options = Options::default();
        let mut list = lexer_with(&source, marked_options.clone()).map_err(marked_error)?;

        walk_tokens(&list.tokens, &mut |token| {
            for extension in extensions.iter().rev() {
                extension.walk_token(token);
            }
        });

        let mut paths = Vec::new();
        collect_code_paths(&list.tokens, &mut Vec::new(), &mut paths);
        for path in paths {
            let (code_text, lang) = match code_at_mut(&mut list.tokens, &path) {
                Some(code) => (code.text.clone(), code.lang.clone()),
                None => continue,
            };
            let highlighted = highlight(&extension_service, &language_service, &code_text, lang.as_ref(), &token).await?;
            // `updateToken`: só troca o texto, e marca como já escapado, quando o destaque o mudou.
            if highlighted != code_text
                && let Some(code) = code_at_mut(&mut list.tokens, &path)
            {
                code.escaped = true;
                code.text = highlighted;
            }
        }

        let parser = Parser::with_renderer(marked_options, Box::new(DocumentRenderer { extensions }));
        parser.parse(&list.tokens, true).map_err(marked_error)
    });
    race_cancellation_error(&parsed, &race_token).then(move |raw| sanitize(raw.units(), sanitizer_config.as_ref()))
}
