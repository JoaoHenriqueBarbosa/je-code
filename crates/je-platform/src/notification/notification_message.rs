//! Porte de `src/vs/platform/notification/common/notificationMessage.ts`.
//!
//! O conteúdo é texto literal e links escritos explicitamente, nunca marcação interpretada. Os nós são
//! imutáveis pelo tipo (o `Object.freeze` do original): só há leitura.

use je_base::JsString;
use je_base::linked_text::{Link, LinkedTextNode};

/// `NotificationText`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NotificationText {
    nodes: Vec<LinkedTextNode>,
}

/// `string | NotificationText`.
#[derive(Clone, Debug)]
pub enum NotificationPart {
    Text(String),
    Rich(NotificationText),
}

impl From<&str> for NotificationPart {
    fn from(text: &str) -> Self {
        NotificationPart::Text(text.to_string())
    }
}

impl From<String> for NotificationPart {
    fn from(text: String) -> Self {
        NotificationPart::Text(text)
    }
}

impl From<NotificationText> for NotificationPart {
    fn from(text: NotificationText) -> Self {
        NotificationPart::Rich(text)
    }
}

impl NotificationText {
    pub fn nodes(&self) -> &[LinkedTextNode] {
        &self.nodes
    }

    /// `link(label, href, title?)`.
    pub fn link(label: &str, href: &str, title: Option<&str>) -> NotificationText {
        NotificationText {
            nodes: vec![LinkedTextNode::Link(Link { label: JsString::from(label), href: JsString::from(href), title: title.map(JsString::from) })],
        }
    }

    /// `concat(...parts)`.
    pub fn concat(parts: impl IntoIterator<Item = NotificationPart>) -> NotificationText {
        let nodes = parts
            .into_iter()
            .flat_map(|part| match part {
                NotificationPart::Text(text) => vec![LinkedTextNode::Text(JsString::from(text))],
                NotificationPart::Rich(rich) => rich.nodes,
            })
            .collect();
        NotificationText { nodes }
    }

    /// Substitui texto literal e links explícitos num modelo localizado, sem interpretar marcação.
    pub fn format(template: &str, args: &[NotificationPart]) -> NotificationText {
        let units: Vec<u16> = template.encode_utf16().collect();
        let mut parts: Vec<NotificationPart> = Vec::new();
        let mut index = 0;
        let mut i = 0;
        // `/\{(?<index>\d+)\}/g`
        while i < units.len() {
            if units[i] == b'{' as u16 {
                let mut j = i + 1;
                while j < units.len() && (b'0' as u16..=b'9' as u16).contains(&units[j]) {
                    j += 1;
                }
                if j > i + 1 && j < units.len() && units[j] == b'}' as u16 {
                    if i > index {
                        parts.push(NotificationPart::Text(String::from_utf16_lossy(&units[index..i])));
                    }
                    let number: usize = String::from_utf16_lossy(&units[i + 1..j]).parse().unwrap_or(usize::MAX);
                    parts.push(args.get(number).cloned().unwrap_or_else(|| NotificationPart::Text(String::from_utf16_lossy(&units[i..=j]))));
                    index = j + 1;
                    i = j + 1;
                    continue;
                }
            }
            i += 1;
        }
        if index < units.len() {
            parts.push(NotificationPart::Text(String::from_utf16_lossy(&units[index..])));
        }
        NotificationText::concat(parts)
    }

    /// Transforma o texto exibido sem mudar o destino dos links nem interpretar o resultado como marcação.
    pub fn map_text(&self, map: impl Fn(&JsString) -> JsString) -> NotificationText {
        NotificationText {
            nodes: self
                .nodes
                .iter()
                .map(|node| match node {
                    LinkedTextNode::Text(text) => LinkedTextNode::Text(map(text)),
                    LinkedTextNode::Link(link) => LinkedTextNode::Link(Link { label: map(&link.label), ..link.clone() }),
                })
                .collect(),
        }
    }
}

impl std::fmt::Display for NotificationText {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for node in &self.nodes {
            match node {
                LinkedTextNode::Text(text) => write!(f, "{}", text.to_string_lossy())?,
                LinkedTextNode::Link(link) => write!(f, "{}", link.label.to_string_lossy())?,
            }
        }
        Ok(())
    }
}
