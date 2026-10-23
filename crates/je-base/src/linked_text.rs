//! Porte de `src/vs/base/common/linkedText.ts`: texto com links no formato `[rótulo](href "título")`.

use zyarr::RegExp;

use crate::JsString;
use crate::decorators::Memoize;
use crate::js_regexp::match_all;

/// `ILink`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Link {
    pub label: JsString,
    pub href: JsString,
    pub title: Option<JsString>,
}

/// `LinkedTextNode`: texto ou link.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LinkedTextNode {
    Text(JsString),
    Link(Link),
}

/// `LinkedText`.
pub struct LinkedText {
    pub nodes: Vec<LinkedTextNode>,
    text: Memoize<JsString>,
}

impl LinkedText {
    pub fn new(nodes: Vec<LinkedTextNode>) -> Self {
        Self { nodes, text: Memoize::new() }
    }

    /// `toString` (`@memoize`): o texto sem os links, só com os rótulos.
    pub fn to_js_string(&self) -> &JsString {
        self.text.get(|| {
            let mut out = JsString::new();
            for node in &self.nodes {
                match node {
                    LinkedTextNode::Text(text) => out.push_units(text.units()),
                    LinkedTextNode::Link(link) => out.push_units(link.label.units()),
                }
            }
            out
        })
    }
}

const LINK_REGEX: &str = r#"\[([^\]]+)\]\(((?:https?:\/\/|command:|file:)[^\)\s]+)(?: (["'])(.+?)(\3))?\)"#;

thread_local! {
    static REGEX: RegExp = {
        let pattern: Vec<u16> = LINK_REGEX.encode_utf16().collect();
        RegExp::new(&pattern, &[u16::from(b'g'), u16::from(b'i')]).expect("regex fixa e válida")
    };
}

/// `parseLinkedText`.
pub fn parse_linked_text(text: &[u16]) -> LinkedText {
    let mut result: Vec<LinkedTextNode> = Vec::new();
    let mut index = 0;

    REGEX.with(|regex| {
        for found in match_all(regex, text) {
            if found.start() > index {
                result.push(LinkedTextNode::Text(JsString::from(&text[index..found.start()])));
            }

            let group = |n: usize| found.captures.get(n).and_then(Option::as_ref).map(|r| JsString::from(&text[r.clone()]));
            let label = group(1).unwrap_or_default();
            let href = group(2).unwrap_or_default();
            // Título vazio é falso no original.
            let title = group(4).filter(|t| !t.is_empty());
            result.push(LinkedTextNode::Link(Link { label, href, title }));

            index = found.end();
        }
    });

    if index < text.len() {
        result.push(LinkedTextNode::Text(JsString::from(&text[index..])));
    }

    LinkedText::new(result)
}
