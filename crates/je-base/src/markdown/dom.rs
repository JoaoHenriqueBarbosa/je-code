//! Operações sobre a árvore sem DOM (`Element` e `Node` do `html_parser`) que o `markdownRenderer.ts` faz com
//! `setAttribute`, `replaceWith`, `querySelectorAll` e `reset`.

use crate::JsString;
use crate::browser::html_parser::{Attribute, Element, Node};

/// `element.setAttribute(name, value)`: troca o valor de um atributo que já existe (a posição não muda) ou
/// acrescenta um novo no fim.
pub fn set_attribute(element: &mut Element, name: &str, value: JsString) {
    match element.attributes.iter_mut().find(|attribute| attribute.name == name) {
        Some(attribute) => attribute.value = value,
        None => element.attributes.push(Attribute { name: name.to_owned(), value }),
    }
}

/// `element.removeAttribute(name)`.
pub fn remove_attribute(element: &mut Element, name: &str) {
    element.attributes.retain(|attribute| attribute.name != name);
}

/// `element.hasAttribute(name)`.
pub fn has_attribute(element: &Element, name: &str) -> bool {
    element.get_attribute(name).is_some()
}

/// O que fazer com um elemento visitado por [`rewrite_elements`].
pub enum Rewrite {
    /// Mantém o elemento (já alterado, se for o caso) e visita os filhos dele.
    Keep,
    /// Troca o elemento por estes nós (`el.replaceWith(...)`), que também são visitados. Vazio remove.
    Splice(Vec<Node>),
}

/// Visita os elementos em ordem de documento (o `querySelectorAll` estático do original) e deixa `visit`
/// alterar, trocar ou remover cada um.
pub fn rewrite_elements(nodes: Vec<Node>, visit: &mut dyn FnMut(&mut Element) -> Rewrite) -> Vec<Node> {
    let mut out = Vec::with_capacity(nodes.len());
    for node in nodes {
        match node {
            Node::Text(_) => out.push(node),
            Node::Element(mut element) => match visit(&mut element) {
                Rewrite::Keep => {
                    element.children = rewrite_elements(std::mem::take(&mut element.children), visit);
                    out.push(Node::Element(element));
                }
                Rewrite::Splice(replacement) => out.extend(rewrite_elements(replacement, visit)),
            },
        }
    }
    out
}

/// Os elementos vazios do HTML: a serialização não escreve a tag de fim.
const VOID_ELEMENTS: [&str; 14] = ["area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "param", "source", "track", "wbr"];

/// Os elementos de texto bruto: o conteúdo sai sem escape.
const RAW_TEXT_ELEMENTS: [&str; 7] = ["style", "script", "xmp", "iframe", "noembed", "noframes", "plaintext"];

fn push_escaped(out: &mut JsString, units: &[u16], attribute: bool) {
    for &unit in units {
        match unit {
            0x26 => out.push_units(&"&amp;".encode_utf16().collect::<Vec<_>>()),
            0xA0 => out.push_units(&"&nbsp;".encode_utf16().collect::<Vec<_>>()),
            0x22 if attribute => out.push_units(&"&quot;".encode_utf16().collect::<Vec<_>>()),
            0x3C => out.push_units(&"&lt;".encode_utf16().collect::<Vec<_>>()),
            0x3E => out.push_units(&"&gt;".encode_utf16().collect::<Vec<_>>()),
            _ => out.push_unit(unit),
        }
    }
}

fn push_str(out: &mut JsString, text: &str) {
    out.push_units(&text.encode_utf16().collect::<Vec<_>>());
}

fn serialize_node(out: &mut JsString, node: &Node, raw_text_parent: bool) {
    match node {
        Node::Text(text) if raw_text_parent => out.push_units(text.units()),
        Node::Text(text) => push_escaped(out, text.units(), false),
        Node::Element(element) => serialize_element(out, element),
    }
}

fn serialize_element(out: &mut JsString, element: &Element) {
    push_str(out, "<");
    push_str(out, &element.tag_name);
    for attribute in &element.attributes {
        push_str(out, " ");
        push_str(out, &attribute.name);
        push_str(out, "=\"");
        push_escaped(out, attribute.value.units(), true);
        push_str(out, "\"");
    }
    push_str(out, ">");
    if VOID_ELEMENTS.contains(&element.tag_name.as_str()) {
        return;
    }
    let raw_text = RAW_TEXT_ELEMENTS.contains(&element.tag_name.as_str());
    for child in &element.children {
        serialize_node(out, child, raw_text);
    }
    push_str(out, "</");
    push_str(out, &element.tag_name);
    push_str(out, ">");
}

/// `element.innerHTML`: a serialização de HTML dos filhos.
pub fn inner_html(element: &Element) -> JsString {
    let mut out = JsString::new();
    let raw_text = RAW_TEXT_ELEMENTS.contains(&element.tag_name.as_str());
    for child in &element.children {
        serialize_node(&mut out, child, raw_text);
    }
    out
}

/// `element.outerHTML`.
pub fn outer_html(element: &Element) -> JsString {
    let mut out = JsString::new();
    serialize_element(&mut out, element);
    out
}
