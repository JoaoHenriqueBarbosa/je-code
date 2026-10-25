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
