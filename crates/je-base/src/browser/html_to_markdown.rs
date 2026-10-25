//! Porte de `src/vs/base/browser/htmlToMarkdown.ts`: o conversor de HTML para markdown usado ao colar
//! conteúdo de páginas da web numa superfície que entende markdown (a entrada do chat, por exemplo).
//!
//! Isto é conversão de texto, não renderização. O original usa o `DOMParser` do navegador só para obter
//! uma árvore de nós e depois a percorre; aqui a árvore vem de [`parse_body`], o parser de HTML em Rust
//! puro de `html_parser.rs` (as divergências do parser em relação ao `DOMParser` estão documentadas lá).
//!
//! Divergências de forma:
//!
//! - o `createTrustedTypesPolicy('htmlToMarkdown', ...)` do original só existe para entregar o HTML ao
//!   `DOMParser` sem violar a política de Trusted Types do navegador; sem `DOMParser`, não há o que
//!   proteger e ele não é portado;
//! - o texto é `&[u16]`/[`JsString`], como no resto do porte, e `trim` é o do JavaScript
//!   (`js_string::trim`).

use std::cell::LazyCell;

use zyarr::RegExp;

use crate::browser::html_parser::{Element, Node, parse_body};
use crate::html_content::{append_escaped_markdown_inline_code, is_portable_markdown_target, test as regexp_matches};
use crate::js_regexp::replace_all;
use crate::js_string::{self, JsString};

const MAX_INPUT_LENGTH: usize = 200_000;

thread_local! {
    static TAGS: LazyCell<RegExp> = LazyCell::new(|| RegExp::from_str("<[^>]+>", "").expect("regex válida"));
    static NON_BREAKING_SPACES: LazyCell<RegExp> = LazyCell::new(|| RegExp::from_str("\\u00A0", "").expect("regex válida"));
    static NEWLINE_RUNS: LazyCell<RegExp> = LazyCell::new(|| RegExp::from_str("\\n{3,}", "").expect("regex válida"));
    static EXECUTABLE_SCHEME: LazyCell<RegExp> = LazyCell::new(|| RegExp::from_str("^(javascript|vbscript|data):", "i").expect("regex válida"));
}

pub fn convert_html_to_markdown(html: &[u16]) -> JsString {
    // Bail out on very large inputs to limit DOM parsing cost
    if html.len() > MAX_INPUT_LENGTH {
        return TAGS.with(|regex| replace_all(regex, html, |_| JsString::new()));
    }

    let body = parse_body(html);
    let result = convert_children(&body);

    // Convert non-breaking spaces to regular spaces
    let result = NON_BREAKING_SPACES.with(|regex| replace_all(regex, result.units(), |_| JsString::from(" ")));

    // Collapse runs of 3+ newlines into 2
    let result = NEWLINE_RUNS.with(|regex| replace_all(regex, result.units(), |_| JsString::from("\n\n")));

    JsString::from(js_string::trim(result.units()))
}

/// `${prefix}${inner}${suffix}`.
fn wrap(prefix: &str, inner: &[u16], suffix: &str) -> JsString {
    let mut out = JsString::from(prefix);
    out.push_units(inner);
    out.push_units(JsString::from(suffix).units());
    out
}

/// `h1` a `h6`: o nível.
fn heading_level(tag: &str) -> Option<usize> {
    let digit = tag.strip_prefix('h').filter(|digit| digit.len() == 1)?;
    digit.parse::<usize>().ok().filter(|level| (1..=6).contains(level))
}

/// `text.replace(/^\n+|\n+$/g, '')`.
fn trim_newlines(units: &[u16]) -> &[u16] {
    let leading = units.iter().take_while(|&&unit| unit == 0x0A).count();
    let rest = &units[leading..];
    let trailing = rest.iter().rev().take_while(|&&unit| unit == 0x0A).count();
    &rest[..rest.len() - trailing]
}

fn attribute_units<'a>(element: &'a Element, name: &str) -> &'a [u16] {
    element.get_attribute(name).map_or(&[][..], JsString::units)
}

fn convert_node(node: &Node) -> JsString {
    let element = match node {
        Node::Text(text) => return text.clone(),
        Node::Comment(_) => return JsString::new(),
        Node::Element(element) => element,
    };
    let tag = element.tag_name.as_str();

    if let Some(level) = heading_level(tag) {
        return wrap(&format!("\n{} ", "#".repeat(level)), js_string::trim(convert_children(element).units()), "\n");
    }

    match tag {
        "pre" => {
            let code = element.query_selector("code").unwrap_or(element);
            let text = code.text_content();
            wrap("\n```\n", trim_newlines(text.units()), "\n```\n")
        }

        "code" => append_escaped_markdown_inline_code(element.text_content().units()),

        "blockquote" => {
            let children = convert_children(element);
            let inner = js_string::trim(children.units());
            let mut lines = JsString::new();
            for (index, line) in inner.split(|&unit| unit == 0x0A).enumerate() {
                if index > 0 {
                    lines.push_unit(0x0A);
                }
                lines.push_units(wrap("> ", js_string::trim(line), "").units());
            }
            wrap("\n", lines.units(), "\n")
        }

        "ol" => convert_list(element, true),
        "ul" => convert_list(element, false),

        "li" => wrap("- ", js_string::trim(convert_children(element).units()), "\n"),

        "p" => wrap("", convert_children(element).units(), "\n\n"),

        "div" => wrap("", convert_children(element).units(), "\n"),

        "br" => JsString::from("\n"),

        "hr" => JsString::from("\n---\n"),

        "a" => sanitize_link(
            link_target_of(element).units(),
            js_string::trim(convert_children(element).units()),
            js_string::trim(element.text_content().units()),
        ),

        "img" => {
            let src = attribute_units(element, "src");
            let alt = attribute_units(element, "alt");
            if !is_portable_markdown_target(src) {
                return if alt.is_empty() { JsString::new() } else { append_escaped_markdown_inline_code(alt) };
            }
            let mut out = wrap("![", alt, "](");
            out.push_units(src);
            out.push_unit(u16::from(b')'));
            out
        }

        "strong" | "b" => wrap("**", convert_children(element).units(), "**"),

        "em" | "i" => wrap("*", convert_children(element).units(), "*"),

        "del" | "s" | "strike" => wrap("~~", convert_children(element).units(), "~~"),

        _ => convert_children(element),
    }
}

/// `ol` (numerada) e `ul`: só os filhos `li`, cada um numa linha.
fn convert_list(element: &Element, ordered: bool) -> JsString {
    let mut out = JsString::from("\n");
    for (index, child) in element.element_children().filter(|child| child.tag_name == "li").enumerate() {
        let marker = if ordered { format!("{}. ", index + 1) } else { "- ".to_owned() };
        out.push_units(wrap(&marker, js_string::trim(convert_children(child).units()), "\n").units());
    }
    out
}

fn convert_children(node: &Element) -> JsString {
    let mut result = JsString::new();
    for child in &node.children {
        result.push_units(convert_node(child).units());
    }
    result
}

/// Reads the target of an anchor. Rendered VS Code links route clicks through `data-href`, so
/// that attribute is consulted when `href` cannot be shared, but never ahead of a usable
/// `href`, since arbitrary pages use the same attribute name and could redirect a link.
fn link_target_of(element: &Element) -> JsString {
    let href = js_string::trim(attribute_units(element, "href"));
    if !href.is_empty() && is_portable_markdown_target(href) {
        return JsString::from(href);
    }
    let data_href = js_string::trim(attribute_units(element, "data-href"));
    JsString::from(if data_href.is_empty() { href } else { data_href })
}

/// Renders an anchor as markdown, keeping only targets that still mean something wherever the
/// markdown is pasted. Everything else falls back to the text the reader actually saw.
fn sanitize_link(href: &[u16], text: &[u16], plain_text: &[u16]) -> JsString {
    let target = js_string::trim(href);

    // An executable target carries no label worth marking up as code.
    if regexp_matches(&EXECUTABLE_SCHEME, target) {
        return JsString::from(text);
    }

    if target.is_empty() || !is_portable_markdown_target(target) {
        // Emphasis markers around the label would be read literally inside a code span.
        return if plain_text.is_empty() { JsString::new() } else { append_escaped_markdown_inline_code(plain_text) };
    }

    let mut out = wrap("[", text, "](");
    out.push_units(target);
    out.push_unit(u16::from(b')'));
    out
}
