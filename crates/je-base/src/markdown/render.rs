//! Porte de `renderMarkdown` e de seus auxiliares em `src/vs/base/browser/markdownRenderer.ts`. Ver o plano em
//! [`crate::markdown`] para o que cabe à fatia 2.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use crate::JsString;
use crate::browser::html_parser::{Element, Node};
use crate::errors::{self, on_unexpected_error};
use crate::extpath::is_unc;
use crate::html_content::{IMarkdownString, IsTrusted, escape_double_quotes, is_portable_link_target, parse_href_and_dimensions, remove_markdown_escapes};
use crate::icon_labels::markdown_escape_escaped_icons;
use crate::id_generator::default_generator_next_id;
use crate::js_string::is_js_whitespace;
use crate::map::OrderedMap;
use crate::marked::{Blockquote, Code, Html, InlineRenderer, MarkedError, Options, Paragraph, Parser, Renderer, Token, lexer_with};
use crate::marshalling::{self, Marshalled};
use crate::network::{FileAccess, schemas};
use crate::path::{Environment, Platform};
use crate::resources::ExtUri;
use crate::strings::escape;
use crate::themables::ThemeIcon;
use crate::uri::{Field, URI, UriChange, decode_uri_component, encode_uri_component};

use super::dom::{Rewrite, rewrite_elements, set_attribute};
use super::incomplete::fill_in_incomplete_tokens;
use super::re::{exec, group, match_all, push_str, remove_first, test, units};
use super::sanitize::{AttributeRule, DomSanitizerConfig, KeepAttr, Protocols, SanitizeAttributeRule, convert_tag_to_plaintext, sanitize_html, BASIC_MARKUP_HTML_TAGS};

/// `MarkdownActionHandler`: recebe o destino do link e o `IMarkdownString` de origem.
pub type MarkdownActionHandler = Rc<dyn Fn(&JsString, &IMarkdownString)>;

/// O `kind` de `transformUri`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MarkdownLinkKind {
    Link,
    Image,
}

/// `MarkdownRendererMarkedOptions`.
#[derive(Clone, Copy, Debug, Default)]
pub struct MarkdownRendererMarkedOptions {
    pub gfm: Option<bool>,
    pub breaks: Option<bool>,
}

/// `MarkdownSanitizerConfig`.
#[derive(Clone, Default)]
pub struct MarkdownSanitizerConfig {
    pub replace_with_plaintext: bool,
    pub allowed_tags_override: Option<Vec<String>>,
    pub allowed_attributes_override: Option<Vec<AttributeRule>>,
    pub allowed_link_schemes_augment: Vec<String>,
    pub remote_image_is_allowed: Option<Rc<dyn Fn(&URI) -> bool>>,
}

/// `MarkdownRenderOptions`, sem o `codeBlockRenderer` assíncrono, o `asyncRenderCallback` e as extensões
/// (fatia 2).
#[derive(Clone, Default)]
pub struct MarkdownRenderOptions {
    /// `codeBlockRendererSync(languageId, value, raw)`: o elemento que substitui o bloco de código.
    pub code_block_renderer_sync: Option<Rc<dyn Fn(&JsString, &JsString, &JsString) -> Element>>,
    pub action_handler: Option<MarkdownActionHandler>,
    /// `transformUri(href, kind)`: `None` é o `undefined` que deixa o destino como está.
    pub transform_uri: Option<Rc<dyn Fn(&JsString, MarkdownLinkKind) -> Option<JsString>>>,
    pub fill_in_incomplete_tokens: bool,
    pub sanitizer_config: MarkdownSanitizerConfig,
    pub marked_options: MarkdownRendererMarkedOptions,
}

/// O que o original lê do ambiente: `resources.extUri`, `process.platform` e `FileAccess`.
#[derive(Clone, Copy)]
pub struct MarkdownEnvironment<'a> {
    pub ext_uri: &'a ExtUri,
    pub env: &'a dyn Environment,
    pub file_access: &'a FileAccess,
}

/// `IRenderedMarkdown`: a árvore (um `div` raiz) e o que é preciso para ativar um link dela.
pub struct RenderedMarkdown {
    pub element: Element,
    markdown: IMarkdownString,
    action_handler: Option<MarkdownActionHandler>,
}

impl RenderedMarkdown {
    /// `activateLink`: o widget chama com o `data-href` do `a` ativado. Devolve se o destino foi entregue ao
    /// `actionHandler` (o original sempre cancela o evento, quem chama também).
    pub fn activate_link(&self, data_href: &JsString, env: &MarkdownEnvironment<'_>) -> bool {
        let result = errors::catch(|| {
            if data_href.is_empty() {
                return false;
            }
            let mut href = data_href.clone();
            if let Some(base) = &self.markdown.base_uri {
                href = resolve_with_base_uri(env, base, &href);
            }
            if let Some(handler) = &self.action_handler {
                handler(&href, &self.markdown);
            }
            true
        });
        match result {
            Ok(delivered) => delivered,
            Err(error) => {
                on_unexpected_error(&error);
                false
            }
        }
    }
}

fn starts_with(text: &[u16], prefix: &str) -> bool {
    text.starts_with(&units(prefix))
}

fn is_trusted(markdown: &IMarkdownString) -> bool {
    matches!(markdown.is_trusted, Some(IsTrusted::Bool(true)) | Some(IsTrusted::Options(_)))
}

/// `getLinkTitle`: o caminho do sistema de arquivos de um `file:`, sem o URL escapado.
fn get_link_title(href: &JsString, platform: Platform) -> JsString {
    let title = errors::catch(|| {
        let parsed = URI::parse(href.clone(), false);
        if *parsed.scheme() == schemas::FILE {
            let mut path = parsed.fs_path(platform);
            if !parsed.fragment().is_empty() {
                push_str(&mut path, "#");
                path.push_units(parsed.fragment().units());
            }
            return Some(escape_double_quotes(path.units()));
        }
        None
    });
    title.ok().flatten().unwrap_or_default()
}

/// `renderImage`.
fn render_image(href: &JsString, title: Option<&JsString>, text: &JsString, transform_uri: &dyn Fn(&JsString) -> Option<JsString>) -> JsString {
    let mut dimensions: Vec<String> = Vec::new();
    let mut attributes: Vec<JsString> = Vec::new();
    if !href.is_empty() {
        let parsed = parse_href_and_dimensions(href.units());
        dimensions = parsed.dimensions;
        let href = transform_uri(&parsed.href).unwrap_or(parsed.href);
        let mut attribute = JsString::from("src=\"");
        attribute.push_units(escape_double_quotes(href.units()).units());
        push_str(&mut attribute, "\"");
        attributes.push(attribute);
    }
    if !text.is_empty() {
        let mut attribute = JsString::from("alt=\"");
        attribute.push_units(escape_double_quotes(text.units()).units());
        push_str(&mut attribute, "\"");
        attributes.push(attribute);
    }
    if let Some(title) = title.filter(|title| !title.is_empty()) {
        let mut attribute = JsString::from("title=\"");
        attribute.push_units(escape_double_quotes(title.units()).units());
        push_str(&mut attribute, "\"");
        attributes.push(attribute);
    }
    attributes.extend(dimensions.into_iter().map(JsString::from));
    let mut out = JsString::from("<img ");
    for (i, attribute) in attributes.iter().enumerate() {
        if i > 0 {
            push_str(&mut out, " ");
        }
        out.push_units(attribute.units());
    }
    push_str(&mut out, ">");
    out
}

/// `postProcessCodeBlockLanguageId`: o id da linguagem é o que vem antes do primeiro separador.
fn post_process_code_block_language_id(lang: Option<&JsString>) -> JsString {
    let Some(lang) = lang.filter(|lang| !lang.is_empty()) else {
        return JsString::new();
    };
    let end = lang.units().iter().position(|&unit| is_js_whitespace(unit) || b"+|:,{?".iter().any(|&c| u16::from(c) == unit)).unwrap_or(lang.len());
    lang.slice(0, end)
}

/// O codicon de cada tipo de alerta.
fn alert_icon(severity: &str) -> &'static str {
    match severity {
        "note" => "info",
        "tip" => "light-bulb",
        "important" => "comment",
        "warning" => "alert",
        _ => "stop",
    }
}

/// `renderIcon(icon).outerHTML`.
fn icon_html(id: &str) -> String {
    format!("<span class=\"{}\"></span>", ThemeIcon::from_id(id).as_class_name_array().join(" "))
}

/// `renderLabelWithIcons(html)` com os elementos juntados: `$(nome)` vira o span do codicon e `\$(nome)` fica como
/// texto `$(nome)`.
fn rewrite_theme_icons(html: &JsString) -> JsString {
    let text = html.units();
    let mut out = JsString::new();
    let mut text_start = 0;
    // `labelWithIconsRegex`, com `ThemeIcon.iconNameExpression` e `iconModifierExpression` expandidos.
    const LABEL_WITH_ICONS: &str = r"(\\)?\$\(([A-Za-z0-9-]+(?:~[A-Za-z]+)?)\)";
    for found in match_all(LABEL_WITH_ICONS, "g", text) {
        out.push_units(&text[text_start..found.start()]);
        text_start = found.end();
        let codicon = group(&found, text, 2).unwrap_or_default();
        if group(&found, text, 1).is_some() {
            push_str(&mut out, "$(");
            out.push_units(codicon);
            push_str(&mut out, ")");
        } else {
            push_str(&mut out, &icon_html(&String::from_utf16_lossy(codicon)));
        }
    }
    out.push_units(&text[text_start..]);
    out
}

macro_rules! forward_to_default_renderer {
    ($($name:ident($($arg:ident: $ty:ty),*);)*) => {
        $(fn $name(&self, $($arg: $ty),*) -> JsString {
            InlineRenderer::$name(&Renderer, $($arg),*)
        })*
    };
}

/// Os `renderer.*` que `createMarkdownRenderer` e `defaultMarkedRenderers` trocam no `Renderer` do `marked`.
#[derive(Clone)]
struct MarkdownHtmlRenderer {
    markdown: Rc<IMarkdownString>,
    options: MarkdownRenderOptions,
    platform: Platform,
    sync_code_blocks: Rc<RefCell<Vec<(String, Element)>>>,
}

impl MarkdownHtmlRenderer {
    fn transform(&self, href: &JsString, kind: MarkdownLinkKind) -> Option<JsString> {
        self.options.transform_uri.as_ref().and_then(|transform| transform(href, kind))
    }

    /// `renderer.html` quando `supportHtml` está desligado: só os `span` de um markdown confiável passam.
    fn unsupported_html(&self, text: &JsString) -> JsString {
        if self.options.sanitizer_config.replace_with_plaintext {
            return escape(text.units());
        }
        let keep = is_trusted(&self.markdown) && (test(r"^(<span[^>]+>)", "", text.units()) || test(r"(<\/\s*span>)$", "", text.units()));
        if keep { text.clone() } else { JsString::new() }
    }

    fn html_text(&self, text: &JsString) -> JsString {
        if self.markdown.support_html == Some(true) { text.clone() } else { self.unsupported_html(text) }
    }

    /// O renderizador de blockquote de alertas (`> [!NOTE]`).
    fn alert_blockquote(&self, parser: &Parser, token: &Blockquote) -> Result<JsString, MarkedError> {
        const PATTERN: &str = r"^\s*\[!(NOTE|TIP|IMPORTANT|WARNING|CAUTION)\]\s*?\n*";
        let fallback = || InlineRenderer::blockquote(&Renderer, parser, token);
        let Some(Token::Paragraph(paragraph)) = token.tokens.first() else {
            return fallback();
        };
        let Some(Token::Text(first_text)) = paragraph.tokens.first() else {
            return fallback();
        };
        let Some(found) = exec(PATTERN, "i", first_text.raw.units()) else {
            return fallback();
        };
        let kind = String::from_utf16_lossy(group(&found, first_text.raw.units(), 1).unwrap_or_default());

        let mut tokens = token.tokens.clone();
        if let Some(Token::Paragraph(paragraph)) = tokens.first_mut()
            && let Some(Token::Text(text)) = paragraph.tokens.first_mut()
        {
            text.raw = remove_first(PATTERN, "i", text.raw.units());
            text.text = remove_first(PATTERN, "i", text.text.units());
        }

        let severity = kind.to_lowercase();
        let capitalized = format!("{}{}", &kind[..1].to_uppercase(), &severity[1..]);
        let content = parser.parse(&tokens, true)?;
        // Pula os três primeiros caracteres do conteúdo (`<p>`), que o `<p>` de abertura abaixo já escreveu.
        let mut out = JsString::from(format!("<blockquote data-severity=\"{severity}\"><p><span>{}{capitalized}</span>", icon_html(alert_icon(&severity))));
        out.push_units(content.slice(3, content.len()).units());
        push_str(&mut out, "</blockquote>\n");
        Ok(out)
    }
}

impl InlineRenderer for MarkdownHtmlRenderer {
    forward_to_default_renderer! {
        text(parser: &Parser, token: &Token);
        strong(parser: &Parser, token: &Token);
        em(parser: &Parser, token: &Token);
        codespan(token: &Token);
        br();
        del(parser: &Parser, token: &Token);
    }

    fn html(&self, token: &Token) -> JsString {
        self.html_text(token.text().unwrap_or(&JsString::new()))
    }

    fn block_html(&self, token: &Html) -> JsString {
        self.html_text(&token.text)
    }

    fn image(&self, token: &Token) -> JsString {
        let Token::Image(image) = token else { return JsString::new() };
        render_image(&image.href, image.title.as_ref(), &image.text, &|href| self.transform(href, MarkdownLinkKind::Image))
    }

    fn link(&self, parser: &Parser, token: &Token) -> JsString {
        let Token::Link(link) = token else { return JsString::new() };
        let mut text = parser.parse_inline(&link.tokens).unwrap_or_default();
        let href = self.transform(&link.href, MarkdownLinkKind::Link).unwrap_or_else(|| link.href.clone());

        // Tira os escapes do markdown (https://github.com/chjj/marked/issues/829).
        if href == text {
            text = remove_markdown_escapes(text.units());
        }
        let mut title = link.title.as_ref().map_or_else(JsString::new, |title| escape_double_quotes(remove_markdown_escapes(title.units()).units()));
        let href = remove_markdown_escapes(href.units());

        // Um `file:` sem título mostra o caminho do sistema, não o URL escapado.
        if title.is_empty() && starts_with(href.units(), &format!("{}:", schemas::FILE)) {
            title = get_link_title(&href, self.platform);
        }
        // Um `command:` sem título não expõe o comando como dica (leitores de tela o anunciam).
        let is_command_uri = starts_with(href.units(), &format!("{}:", schemas::COMMAND));

        let mut encoded = JsString::new();
        for &unit in href.units() {
            match u8::try_from(unit) {
                Ok(b'&') => push_str(&mut encoded, "&amp;"),
                Ok(b'<') => push_str(&mut encoded, "&lt;"),
                Ok(b'>') => push_str(&mut encoded, "&gt;"),
                Ok(b'"') => push_str(&mut encoded, "&quot;"),
                Ok(b'\'') => push_str(&mut encoded, "&#39;"),
                _ => encoded.push_unit(unit),
            }
        }
        let effective_title = if !title.is_empty() {
            title
        } else if is_command_uri {
            JsString::new()
        } else {
            encoded.clone()
        };
        let mut out = JsString::from("<a href=\"");
        out.push_units(encoded.units());
        push_str(&mut out, "\" title=\"");
        out.push_units(effective_title.units());
        push_str(&mut out, "\" draggable=\"false\">");
        out.push_units(text.units());
        push_str(&mut out, "</a>");
        out
    }

    fn code(&self, token: &Code) -> JsString {
        let Some(render) = &self.options.code_block_renderer_sync else {
            return InlineRenderer::code(&Renderer, token);
        };
        let id = default_generator_next_id();
        let element = render(&post_process_code_block_language_id(token.lang.as_ref()), &token.text, &token.raw);
        self.sync_code_blocks.borrow_mut().push((id.clone(), element));
        let mut out = JsString::from(format!("<div class=\"code\" data-code=\"{id}\">"));
        out.push_units(escape(token.text.units()).units());
        push_str(&mut out, "</div>");
        out
    }

    fn blockquote(&self, parser: &Parser, token: &Blockquote) -> Result<JsString, MarkedError> {
        if self.markdown.support_alert_syntax == Some(true) {
            self.alert_blockquote(parser, token)
        } else {
            InlineRenderer::blockquote(&Renderer, parser, token)
        }
    }

    fn paragraph(&self, parser: &Parser, token: &Paragraph) -> Result<JsString, MarkedError> {
        let mut out = JsString::from("<p>");
        out.push_units(parser.parse_inline(&token.tokens)?.units());
        push_str(&mut out, "</p>");
        Ok(out)
    }
}

/// `preprocessMarkdownString`.
fn preprocess_markdown_string(markdown: &IMarkdownString) -> JsString {
    let mut value = markdown.value.clone();
    // Valores longos demais travam a interface.
    if value.len() > 100_000 {
        value = value.slice(0, 100_000);
        push_str(&mut value, "…");
    }
    if markdown.support_theme_icons == Some(true) {
        value = markdown_escape_escaped_icons(value.units());
    }
    value
}

/// `resolveWithBaseUri`.
fn resolve_with_base_uri(env: &MarkdownEnvironment<'_>, base_uri: &URI, href: &JsString) -> JsString {
    if test(r"^\w[\w\d+.-]*:", "", href.units()) {
        return href.clone();
    }
    let href = href.to_string_lossy();
    let ends_with_slash = base_uri.path().units().last() == Some(&u16::from(b'/'));
    let resolved = if ends_with_slash { env.ext_uri.resolve_path(base_uri, &href) } else { env.ext_uri.resolve_path(&env.ext_uri.dirname(base_uri), &href) };
    resolved.to_string(false)
}

/// `uriMassage`: troca, dentro do JSON de um argumento de comando, os textos que são chaves de `uris`.
fn uri_massage(markdown: &IMarkdownString, part: &JsString, platform: Platform) -> JsString {
    fn is_falsy(value: &Marshalled) -> bool {
        match value {
            Marshalled::Undefined | Marshalled::Null => true,
            Marshalled::Bool(b) => !b,
            Marshalled::Number(n) => *n == 0.0 || n.is_nan(),
            Marshalled::String(s) => s.is_empty(),
            _ => false,
        }
    }
    fn replace_uris(value: Marshalled, uris: &OrderedMap<JsString, URI>) -> Marshalled {
        match value {
            Marshalled::String(text) => match uris.get(&text) {
                Some(uri) => Marshalled::Uri(uri.clone()),
                None => Marshalled::String(text),
            },
            Marshalled::Array(items) => Marshalled::Array(items.into_iter().map(|item| replace_uris(item, uris)).collect()),
            Marshalled::Object(entries) => Marshalled::Object(entries.into_iter().map(|(key, item)| (key, replace_uris(item, uris))).collect()),
            other => other,
        }
    }

    let parsed = errors::catch(|| {
        let decoded = decode_uri_component(part.units()).ok()?;
        Some(marshalling::parse(&decoded, platform))
    });
    let Some(data) = parsed.ok().flatten().filter(|data| !is_falsy(data)) else {
        return part.clone();
    };
    let changed = match &markdown.uris {
        Some(uris) => replace_uris(data, uris),
        None => data,
    };
    JsString::from_units(encode_uri_component(marshalling::stringify(&changed).units()))
}

/// `massageHref`.
fn massage_href(markdown: &IMarkdownString, href: &JsString, is_dom_uri: bool, env: &MarkdownEnvironment<'_>) -> JsString {
    let revived = markdown.uris.as_ref().and_then(|uris| uris.get(href)).cloned();
    if is_dom_uri {
        if starts_with(href.units(), &format!("{}:", schemas::DATA)) {
            return href.clone();
        }
        let uri = revived.unwrap_or_else(|| URI::parse(href.clone(), false));
        // Esse URI vira o `src` de um nó: precisa de um protocolo que o navegador entenda.
        return env.file_access.uri_to_browser_uri(&uri).to_string(true);
    }
    let Some(mut uri) = revived else {
        return href.clone();
    };
    if URI::parse(href.clone(), false).to_string(false) == uri.to_string(false) {
        return href.clone(); // Sem transformação.
    }
    if !uri.query().is_empty() {
        let query = uri_massage(markdown, uri.query(), env.env.platform());
        uri = uri.with(&UriChange { query: Field::Set(query), ..UriChange::default() });
    }
    uri.to_string(false)
}

/// `rewriteRenderedLinks`: reescreve `src` de mídia e `href` de link, e tira os links que não valem.
fn rewrite_rendered_links(markdown: &IMarkdownString, options: &MarkdownRenderOptions, nodes: Vec<Node>, env: &MarkdownEnvironment<'_>) -> Vec<Node> {
    let trusted = is_trusted(markdown);
    rewrite_elements(nodes, &mut |element| {
        if matches!(element.tag_name.as_str(), "img" | "audio" | "video" | "source") {
            if let Some(src) = element.get_attribute("src").filter(|src| !src.is_empty()).cloned() {
                let mut href = src;
                if let Some(base) = &markdown.base_uri {
                    // Absoluto, caminho local relativo ou `file:`; um erro deixa o valor como está.
                    if let Ok(resolved) = errors::catch(|| resolve_with_base_uri(env, base, &href)) {
                        href = resolved;
                    }
                }
                set_attribute(element, "src", massage_href(markdown, &href, true, env));
            }
        } else if element.tag_name == "a" {
            let href = element.get_attribute("href").cloned();
            // O `href` fica vazio: os cliques usam o `data-href`.
            set_attribute(element, "href", JsString::new());
            let drop_link = match &href {
                None => true,
                Some(href) => {
                    let units = href.units();
                    units.is_empty()
                        || test(r"^data:|javascript:", "i", units)
                        || (test(r"^command:", "i", units) && !trusted)
                        || test(r"^command:(\/\/\/)?_workbench\.downloadResource", "i", units)
                }
            };
            if drop_link {
                return Rewrite::Splice(std::mem::take(&mut element.children));
            }
            let href = href.unwrap_or_default();
            let mut resolved_href = massage_href(markdown, &href, false, env);
            if let Some(base) = &markdown.base_uri {
                resolved_href = resolve_with_base_uri(env, base, &href);
            }
            set_attribute(element, "data-href", resolved_href.clone());
            // Só onde um `actionHandler` intercepta o clique: sem ele a âncora navegaria sozinha.
            if options.action_handler.is_some() && is_portable_link_target(resolved_href.units()) {
                set_attribute(element, "href", resolved_href);
            }
        }
        Rewrite::Keep
    })
}

fn is_local_file_uri(uri: &URI, env: &MarkdownEnvironment<'_>) -> bool {
    *uri.scheme() == schemas::FILE && uri.authority().is_empty() && !is_unc(&uri.fs_path(env.env.platform()).to_string_lossy(), env.env)
}

/// `isMediaSourceAllowed`.
fn is_media_source_allowed(base_uri: Option<&URI>, source: &[u16], remote_image_is_allowed: &dyn Fn(&URI) -> bool, env: &MarkdownEnvironment<'_>) -> bool {
    let parsed = errors::catch(|| {
        let has_scheme = test(r"^\w[\w\d+.-]*:", "", source);
        if !has_scheme && base_uri.is_some_and(|base| *base.scheme() == schemas::FILE && !is_local_file_uri(base, env)) {
            return None;
        }
        let href = match base_uri {
            Some(base) => resolve_with_base_uri(env, base, &JsString::from(source)),
            None => JsString::from(source),
        };
        Some(URI::parse(href, false))
    });
    let Some(uri) = parsed.ok().flatten() else {
        return false;
    };
    is_local_file_uri(&uri, env) || *uri.scheme() == schemas::DATA || remote_image_is_allowed(&uri)
}

/// `allowedMarkdownHtmlTags`.
pub fn allowed_markdown_html_tags() -> Vec<String> {
    // `input` desenha as caixas de seleção; os outros tipos de `input` saem, e todos ficam desabilitados.
    BASIC_MARKUP_HTML_TAGS.iter().copied().chain(["input"]).map(str::to_owned).collect()
}

/// `allowedMarkdownHtmlAttributes`.
pub fn allowed_markdown_html_attributes() -> Vec<AttributeRule> {
    let names = [
        "align", "autoplay", "alt", "colspan", "controls", "draggable", "height", "href", "loop", "muted", "playsinline", "poster", "rowspan", "src",
        "target", "title", "type", "width", "start", // `input` desabilitado
        "checked", "disabled", "value", // atributos próprios do markdown
        "data-code", "data-href", "data-severity",
    ];
    let mut rules: Vec<AttributeRule> = names.iter().map(|name| AttributeRule::Name((*name).to_owned())).collect();
    // Só estilos bem específicos.
    rules.push(AttributeRule::Rule(SanitizeAttributeRule {
        attribute_name: "style".to_owned(),
        should_keep: Rc::new(|element, name, value| {
            KeepAttr::Keep(
                element.tag_name == "span"
                    && name == "style"
                    && test(
                        r"^(color:(#[0-9a-fA-F]+|var\(--vscode(-[a-zA-Z0-9]+)+\));)?(background-color:(#[0-9a-fA-F]+|var\(--vscode(-[a-zA-Z0-9]+)+\));)?(display:inline-block;)?(border-radius:[0-9]+px;)?$",
                        "",
                        value,
                    ),
            )
        }),
    }));
    // Só codicons como classes.
    rules.push(AttributeRule::Rule(SanitizeAttributeRule {
        attribute_name: "class".to_owned(),
        should_keep: Rc::new(|element, name, value| {
            KeepAttr::Keep(element.tag_name == "span" && name == "class" && test(r"^codicon codicon-[a-z\-]+( codicon-modifier-[a-z\-]+)?$", "", value))
        }),
    }));
    rules
}

/// `getDomSanitizerConfig`.
fn dom_sanitizer_config<'a>(markdown: &IMarkdownString, options: &MarkdownSanitizerConfig, env: &MarkdownEnvironment<'a>) -> DomSanitizerConfig<'a> {
    let mut link_schemes: Vec<String> = [
        schemas::HTTP,
        schemas::HTTPS,
        schemas::MAILTO,
        schemas::FILE,
        schemas::VSCODE_FILE_RESOURCE,
        schemas::VSCODE_REMOTE,
        schemas::VSCODE_REMOTE_RESOURCE,
        schemas::VSCODE_NOTEBOOK_CELL,
        // Links tratados só pelo `actionHandler`.
        schemas::INTERNAL,
    ]
    .iter()
    .map(|scheme| (*scheme).to_owned())
    .collect();
    if is_trusted(markdown) {
        link_schemes.push(schemas::COMMAND.to_owned());
    }
    link_schemes.extend(options.allowed_link_schemes_augment.iter().cloned());

    let env = *env;
    let base_uri = markdown.base_uri.clone();
    let media_source_is_allowed = options.remote_image_is_allowed.clone().map(|remote| {
        Rc::new(move |source: &[u16]| is_media_source_allowed(base_uri.as_ref(), source, remote.as_ref(), &env)) as Rc<dyn Fn(&[u16]) -> bool + 'a>
    });

    DomSanitizerConfig {
        // As tags são tudo que o Markdown gera (CommonMark 0.29 e as tabelas do GFM); o DOMPurify confere o que o
        // `marked` deixar passar.
        allowed_tags_override: Some(options.allowed_tags_override.clone().unwrap_or_else(allowed_markdown_html_tags)),
        allowed_attributes_override: Some(options.allowed_attributes_override.clone().unwrap_or_else(allowed_markdown_html_attributes)),
        allowed_link_protocols: Some(Protocols::List(link_schemes)),
        allow_relative_link_paths: markdown.base_uri.is_some(),
        allowed_media_protocols: Some(Protocols::List(
            [schemas::HTTP, schemas::HTTPS, schemas::DATA, schemas::FILE, schemas::VSCODE_FILE_RESOURCE, schemas::VSCODE_REMOTE, schemas::VSCODE_REMOTE_RESOURCE]
                .iter()
                .map(|scheme| (*scheme).to_owned())
                .collect(),
        )),
        allow_relative_media_paths: markdown.base_uri.is_some(),
        media_source_is_allowed,
        replace_with_plaintext: options.replace_with_plaintext,
        ..DomSanitizerConfig::default()
    }
}

/// `renderMarkdown`: o Markdown vira uma árvore de elementos sanitizada (um `div` raiz). O `target`, o
/// `codeBlockRenderer` assíncrono e os ouvintes de evento ficam para a fatia 2.
pub fn render_markdown(markdown: &IMarkdownString, options: &MarkdownRenderOptions, env: &MarkdownEnvironment<'_>) -> Result<RenderedMarkdown, MarkedError> {
    let sync_code_blocks = Rc::new(RefCell::new(Vec::new()));
    let renderer = MarkdownHtmlRenderer {
        markdown: Rc::new(markdown.clone()),
        options: options.clone(),
        platform: env.env.platform(),
        sync_code_blocks: sync_code_blocks.clone(),
    };
    let value = preprocess_markdown_string(markdown);

    let marked_options = Options { gfm: options.marked_options.gfm.unwrap_or(true), breaks: options.marked_options.breaks.unwrap_or(false), ..Options::default() };
    let tokens = lexer_with(value.units(), marked_options.clone())?;
    let tokens = if options.fill_in_incomplete_tokens { fill_in_incomplete_tokens(tokens) } else { tokens };
    let mut rendered = Parser::with_renderer(marked_options, Box::new(renderer)).parse(&tokens.tokens, true)?;

    // Ícones de tema.
    if markdown.support_theme_icons == Some(true) {
        rendered = rewrite_theme_icons(&rendered);
    }

    let sanitizer_config = dom_sanitizer_config(markdown, &options.sanitizer_config, env);
    let mut nodes = sanitize_html(rendered.units(), &sanitizer_config);

    // Reescreve links e imagens antes de a árvore ser usada.
    nodes = rewrite_rendered_links(markdown, options, nodes, env);

    let code_blocks: HashMap<String, Element> = sync_code_blocks.take().into_iter().collect();
    if !code_blocks.is_empty() {
        nodes = rewrite_elements(nodes, &mut |element| {
            if element.tag_name == "div"
                && let Some(id) = element.get_attribute("data-code")
                && let Some(block) = code_blocks.get(&id.to_string_lossy())
            {
                element.children = vec![Node::Element(block.clone())];
            }
            Rewrite::Keep
        });
    }

    // Tira ou desabilita os `input`.
    let replace_with_plaintext = options.sanitizer_config.replace_with_plaintext;
    nodes = rewrite_elements(nodes, &mut |element| {
        if element.tag_name != "input" {
            return Rewrite::Keep;
        }
        if element.get_attribute("type").is_some_and(|kind| *kind == "checkbox") {
            set_attribute(element, "disabled", JsString::new());
            Rewrite::Keep
        } else if replace_with_plaintext {
            Rewrite::Splice(convert_tag_to_plaintext(element.clone()))
        } else {
            Rewrite::Splice(Vec::new())
        }
    });

    Ok(RenderedMarkdown {
        element: Element { tag_name: "div".to_owned(), attributes: Vec::new(), children: nodes },
        markdown: markdown.clone(),
        action_handler: options.action_handler.clone(),
    })
}
