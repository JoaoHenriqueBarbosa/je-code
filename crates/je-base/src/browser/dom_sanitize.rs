//! Porte de `src/vs/base/browser/domSanitize.ts`: o sanitizador de HTML do VS Code (DOMPurify com ganchos) sobre a
//! árvore sem DOM do `html_parser`, no lugar de um DOM de navegador. É o único sanitizador: o `markdownRenderer`
//! (`markdown::render`) usa este módulo.
//!
//! Regras do DOMPurify que o original usa e que valem aqui: elemento fora de `ALLOWED_TAGS` sai mantendo os
//! filhos (`KEEP_CONTENT`), salvo os de `FORBID_CONTENTS`, que saem com o conteúdo; atributo fora da lista sai;
//! valor de atributo passa por `IS_ALLOWED_URI` (com `ALLOW_UNKNOWN_PROTOCOLS`, que só recusa `*script:` e
//! `data:`), por `SAFE_FOR_XML` e pelos ganchos de protocolo de `href`, `src` e `poster`. Os atributos que
//! sobrevivem mantêm a ordem de origem (o DOMPurify só reescreve o atributo quando o valor muda).
//!
//! Um comentário (`Node::Comment`) fora de `ALLOWED_TAGS` sai; com `replaceWithPlaintext` vira o texto
//! `<!--...-->`, como no gancho `uponSanitizeElement` do original.
//!
//! Divergências:
//! - `new URL(valor, 'vscode-relative-path://')` é a leitura do protocolo em [`url_protocol`] (esquema, ou
//!   caminho relativo; esquema especial sem host, como `http:`, é erro), não o parser WHATWG inteiro.
//! - Não há Trusted Types: `sanitizeSurvivingStalePolicy` (recuperação de uma política de Trusted Types cujo
//!   realm sumiu) não tem equivalente, e o resultado é a lista de nós, não um `TrustedHTML`.
//! - Ficam de fora os testes de DOM clobbering (`id`/`name` não estão em nenhuma lista usada) e o de `innerHTML`
//!   de elementos de texto bruto, que o conjunto de tags permitido não tem.

use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use crate::browser::html_parser::{Attribute, Element, Node, parse_body};
use crate::js_string::{self, JsString};
use crate::markdown::dom::remove_attribute;
use crate::markdown::re::{push_str, test};

/// `basicMarkupHtmlTags`.
pub const BASIC_MARKUP_HTML_TAGS: &[&str] = &[
    "a", "abbr", "b", "bdo", "blockquote", "br", "caption", "cite", "code", "col", "colgroup", "dd", "del", "details", "dfn", "div", "dl", "dt",
    "em", "figcaption", "figure", "h1", "h2", "h3", "h4", "h5", "h6", "hr", "i", "img", "ins", "kbd", "label", "li", "mark", "ol", "p", "pre",
    "q", "rp", "rt", "ruby", "s", "samp", "small", "small", "source", "span", "strike", "strong", "sub", "summary", "sup", "table", "tbody",
    "td", "tfoot", "th", "thead", "time", "tr", "tt", "u", "ul", "var", "video", "wbr",
];

/// `defaultAllowedAttrs`.
pub const DEFAULT_ALLOWED_ATTRS: &[&str] = &[
    "href", "target", "src", "alt", "title", "for", "name", "role", "tabindex", "x-dispatch", "required", "checked", "placeholder", "type", "start",
    "width", "height", "align",
];

const FAKE_RELATIVE_URL_PROTOCOL: &str = "vscode-relative-path";

/// `FORBID_CONTENTS` do DOMPurify: elementos fora da lista de tags que saem com o conteúdo.
const FORBID_CONTENTS: &[&str] = &[
    "annotation-xml", "audio", "colgroup", "desc", "foreignobject", "head", "iframe", "math", "mi", "mn", "mo", "ms", "mtext", "noembed", "noframes",
    "noscript", "plaintext", "script", "style", "svg", "template", "thead", "title", "video", "xmp",
];

/// `URI_SAFE_ATTRIBUTES` do DOMPurify: atributos cujo valor não é conferido como URI.
const URI_SAFE_ATTRIBUTES: &[&str] = &[
    "alt", "class", "for", "id", "label", "name", "pattern", "placeholder", "role", "summary", "title", "value", "style", "xmlns",
];

/// `DATA_URI_TAGS` do DOMPurify: onde `src`/`href` pode começar por `data:`.
const DATA_URI_TAGS: &[&str] = &["audio", "video", "img", "source", "image", "track"];

const SELF_CLOSING_TAGS: &[&str] = &[
    "area", "base", "br", "col", "command", "embed", "hr", "img", "input", "keygen", "link", "meta", "param", "source", "track", "wbr",
];

/// `IS_ALLOWED_URI` do DOMPurify.
const IS_ALLOWED_URI: &str = r"^(?:(?:(?:f|ht)tps?|mailto|tel|callto|sms|cid|xmpp|matrix):|[^a-z]|[a-z+.\-]+(?:[^a-z+.\-:]|$))";
const IS_SCRIPT_OR_DATA: &str = r"^(?:\w+script|data):";
const DATA_ATTR: &str = r"^data-[\-\w.·-￿]+$";
const ARIA_ATTR: &str = r"^aria-[\-\w]+$";
const SAFE_FOR_XML_VALUE: &str = r"((--!?|])>)|<\/(style|title)";

/// O que um predicado de atributo decide: manter ou não, ou manter com outro valor (`boolean | string`).
pub enum KeepAttr {
    Keep(bool),
    Replace(JsString),
}

/// `SanitizeAttributePredicate`: recebe o elemento, o nome do atributo e o valor.
pub type SanitizeAttributePredicate = Rc<dyn Fn(&Element, &str, &[u16]) -> KeepAttr>;

/// `SanitizeAttributeRule`.
#[derive(Clone)]
pub struct SanitizeAttributeRule {
    pub attribute_name: String,
    pub should_keep: SanitizeAttributePredicate,
}

/// `string | SanitizeAttributeRule`.
#[derive(Clone)]
pub enum AttributeRule {
    Name(String),
    Rule(SanitizeAttributeRule),
}

/// `readonly string[] | '*'`.
#[derive(Clone)]
pub enum Protocols {
    List(Vec<String>),
    All,
}

/// `DomSanitizerConfig`. `override`/`augment` de cada lista são dois campos.
#[derive(Clone, Default)]
pub struct DomSanitizerConfig<'a> {
    pub allowed_tags_override: Option<Vec<String>>,
    pub allowed_tags_augment: Option<Vec<String>>,
    pub allowed_attributes_override: Option<Vec<AttributeRule>>,
    pub allowed_attributes_augment: Option<Vec<AttributeRule>>,
    pub allowed_link_protocols: Option<Protocols>,
    pub allow_relative_link_paths: bool,
    pub allowed_media_protocols: Option<Protocols>,
    pub allow_relative_media_paths: bool,
    pub media_source_is_allowed: Option<Rc<dyn Fn(&[u16]) -> bool + 'a>>,
    pub replace_with_plaintext: bool,
}

/// O esquema que `new URL(value, 'vscode-relative-path://').protocol` daria (sem os dois pontos, em minúsculas),
/// ou `None` quando o `URL` lança.
fn url_protocol(value: &[u16]) -> Option<String> {
    let is_c0_or_space = |unit: &u16| *unit <= 0x20;
    let start = value.iter().position(|unit| !is_c0_or_space(unit)).unwrap_or(value.len());
    let end = value.iter().rposition(|unit| !is_c0_or_space(unit)).map_or(start, |i| i + 1);
    let input: Vec<u16> = value[start..end].iter().copied().filter(|&unit| unit != 9 && unit != 10 && unit != 13).collect();

    let scheme_len = input.iter().position(|&unit| unit == u16::from(b':'));
    let valid_scheme = scheme_len.filter(|&len| {
        len > 0
            && input[0].is_ascii_alphabetic_unit()
            && input[..len].iter().all(|&unit| unit.is_ascii_alphabetic_unit() || (u16::from(b'0')..=u16::from(b'9')).contains(&unit) || [b'+', b'-', b'.'].iter().any(|&c| u16::from(c) == unit))
    });
    let Some(len) = valid_scheme else {
        return Some(FAKE_RELATIVE_URL_PROTOCOL.to_owned());
    };
    let scheme = String::from_utf16_lossy(&input[..len]).to_ascii_lowercase();
    let special_with_host = matches!(scheme.as_str(), "http" | "https" | "ftp" | "ws" | "wss");
    if special_with_host {
        let rest = &input[len + 1..];
        let after_slashes = rest.iter().position(|&unit| unit != u16::from(b'/') && unit != u16::from(b'\\')).unwrap_or(rest.len());
        let host_end = rest[after_slashes..]
            .iter()
            .position(|&unit| [b'/', b'\\', b'?', b'#'].iter().any(|&c| u16::from(c) == unit))
            .unwrap_or(rest.len() - after_slashes);
        if host_end == 0 {
            return None;
        }
    }
    Some(scheme)
}

trait AsciiAlphabeticUnit {
    fn is_ascii_alphabetic_unit(&self) -> bool;
}

impl AsciiAlphabeticUnit for u16 {
    fn is_ascii_alphabetic_unit(&self) -> bool {
        u8::try_from(*self).is_ok_and(|byte| byte.is_ascii_alphabetic())
    }
}

/// `validateLink`.
fn validate_link(value: &[u16], protocols: &Protocols, allow_relative_paths: bool) -> bool {
    let Protocols::List(allowed) = protocols else {
        return true;
    };
    let Some(protocol) = url_protocol(value) else {
        return false;
    };
    if allowed.iter().any(|candidate| *candidate == protocol) {
        return true;
    }
    allow_relative_paths
        && protocol == FAKE_RELATIVE_URL_PROTOCOL
        && !JsString::from(js_string::trim(value)).to_lower_case().units().starts_with(&FAKE_RELATIVE_URL_PROTOCOL.encode_utf16().collect::<Vec<_>>())
}

/// `convertTagToPlaintext` de um elemento: a tag de abertura e a de fechamento viram texto, e os filhos ficam
/// entre os dois.
pub fn convert_tag_to_plaintext(element: Element) -> Vec<Node> {
    let mut start = JsString::from(format!("<{}", element.tag_name));
    for attribute in &element.attributes {
        push_str(&mut start, " ");
        push_str(&mut start, &attribute.name);
        push_str(&mut start, "=\"");
        start.push_units(attribute.value.units());
        push_str(&mut start, "\"");
    }
    push_str(&mut start, ">");
    let mut nodes = vec![Node::Text(start)];
    let self_closing = SELF_CLOSING_TAGS.contains(&element.tag_name.as_str());
    let tag_name = element.tag_name;
    nodes.extend(element.children);
    if !self_closing {
        nodes.push(Node::Text(JsString::from(format!("</{tag_name}>"))));
    }
    nodes
}

/// `convertTagToPlaintext(node)` inteiro: comentário vira `<!--data-->`, elemento vira o que
/// [`convert_tag_to_plaintext`] devolve, e um nó de texto não tem representação (`undefined`).
pub fn convert_node_to_plaintext(node: Node) -> Option<Vec<Node>> {
    match node {
        Node::Comment(data) => {
            let mut text = JsString::from("<!--");
            text.push_units(data.units());
            push_str(&mut text, "-->");
            Some(vec![Node::Text(text)])
        }
        Node::Element(element) => Some(convert_tag_to_plaintext(element)),
        Node::Text(_) => None,
    }
}

struct Sanitizer<'c, 'a> {
    config: &'c DomSanitizerConfig<'a>,
    tags: HashSet<String>,
    attribute_names: HashSet<String>,
    predicates: HashMap<String, SanitizeAttributeRule>,
    link_protocols: Protocols,
    media_protocols: Protocols,
}

impl<'c, 'a> Sanitizer<'c, 'a> {
    fn new(config: &'c DomSanitizerConfig<'a>) -> Self {
        let mut tags: Vec<String> = BASIC_MARKUP_HTML_TAGS.iter().map(|tag| (*tag).to_owned()).collect();
        if let Some(over) = &config.allowed_tags_override {
            tags = over.clone();
        }
        if let Some(augment) = &config.allowed_tags_augment {
            tags.extend(augment.iter().cloned());
        }

        let mut rules: Vec<AttributeRule> = DEFAULT_ALLOWED_ATTRS.iter().map(|name| AttributeRule::Name((*name).to_owned())).collect();
        if let Some(over) = &config.allowed_attributes_override {
            rules = over.clone();
        }
        if let Some(augment) = &config.allowed_attributes_augment {
            rules.extend(augment.iter().cloned());
        }
        let mut attribute_names = HashSet::new();
        let mut predicates = HashMap::new();
        for rule in rules {
            match rule {
                // Um nome novo em texto limpa o predicado definido antes para ele.
                AttributeRule::Name(name) => {
                    let name = name.to_lowercase();
                    predicates.remove(&name);
                    attribute_names.insert(name);
                }
                AttributeRule::Rule(rule) => {
                    let name = rule.attribute_name.to_lowercase();
                    attribute_names.insert(name.clone());
                    predicates.insert(name, SanitizeAttributeRule { attribute_name: rule.attribute_name.to_lowercase(), should_keep: rule.should_keep });
                }
            }
        }

        let to_protocols = |configured: &Option<Protocols>| configured.clone().unwrap_or_else(|| Protocols::List(vec!["http".to_owned(), "https".to_owned()]));
        Sanitizer {
            config,
            tags: tags.into_iter().map(|tag| tag.to_lowercase()).collect(),
            attribute_names,
            predicates,
            link_protocols: to_protocols(&config.allowed_link_protocols),
            media_protocols: to_protocols(&config.allowed_media_protocols),
        }
    }

    /// `_isValidAttribute` do DOMPurify.
    fn is_valid_attribute(&self, tag: &str, name: &str, value: &JsString) -> bool {
        let name_units: Vec<u16> = name.encode_utf16().collect();
        if test(DATA_ATTR, "", &name_units) || test(ARIA_ATTR, "", &name_units) {
            return true;
        }
        if !self.attribute_names.contains(name) {
            return false;
        }
        if URI_SAFE_ATTRIBUTES.contains(&name) {
            return true;
        }
        let stripped: Vec<u16> = value
            .units()
            .iter()
            .copied()
            .filter(|&unit| !(unit <= 0x20 || unit == 0xA0 || unit == 0x1680 || unit == 0x180E || (0x2000..=0x2029).contains(&unit) || unit == 0x205F || unit == 0x3000))
            .collect();
        if test(IS_ALLOWED_URI, "i", &stripped) {
            return true;
        }
        let starts_with_data = value.units().starts_with(&"data:".encode_utf16().collect::<Vec<_>>());
        if matches!(name, "src" | "xlink:href" | "href") && tag != "script" && starts_with_data && DATA_URI_TAGS.contains(&tag) {
            return true;
        }
        // `ALLOW_UNKNOWN_PROTOCOLS` é verdadeiro no `defaultDomPurifyConfig`.
        if !test(IS_SCRIPT_OR_DATA, "i", &stripped) {
            return true;
        }
        value.is_empty()
    }

    /// `_sanitizeAttributes` com o gancho `uponSanitizeAttribute` de `domSanitize.ts`.
    fn sanitize_attributes(&self, element: &Element) -> Vec<Attribute> {
        let mut kept = Vec::new();
        // O DOMPurify só reescreve o atributo quando o valor muda, então a ordem de inserção sobrevive.
        for attribute in element.attributes.iter() {
            let name = attribute.name.to_lowercase();
            let mut value = if name == "value" { attribute.value.clone() } else { JsString::from(js_string::trim(attribute.value.units())) };
            let mut keep = true;
            if !self.predicates.is_empty() {
                match self.predicates.get(&name) {
                    Some(rule) => match (rule.should_keep)(element, &name, value.units()) {
                        KeepAttr::Keep(decision) => keep = decision,
                        KeepAttr::Replace(replacement) => {
                            keep = true;
                            value = replacement;
                        }
                    },
                    None => keep = self.attribute_names.contains(&name),
                }
            }
            if !keep || test(SAFE_FOR_XML_VALUE, "i", value.units()) || !self.is_valid_attribute(&element.tag_name, &name, &value) {
                continue;
            }
            kept.push(Attribute { name: attribute.name.clone(), value });
        }
        kept
    }

    /// O gancho `afterSanitizeAttributes` (`hookDomPurifyHrefAndSrcSanitizer`). Devolve os nós que trocam o
    /// elemento quando a fonte de mídia não é permitida e `replaceWithPlaintext` está ligado.
    fn after_sanitize_attributes(&self, element: &mut Element) -> Option<Vec<Node>> {
        for attribute in ["href", "src", "poster"] {
            let Some(value) = element.get_attribute(attribute).cloned() else {
                continue;
            };
            let starts_with_hash = value.units().first() == Some(&u16::from(b'#'));
            if attribute == "href" && element.tag_name == "a" {
                if !starts_with_hash && !validate_link(value.units(), &self.link_protocols, self.config.allow_relative_link_paths) {
                    remove_attribute(element, attribute);
                }
            } else if attribute == "href" && starts_with_hash {
                continue;
            } else if !validate_link(value.units(), &self.media_protocols, self.config.allow_relative_media_paths) {
                remove_attribute(element, attribute);
            } else if self.config.media_source_is_allowed.as_ref().is_some_and(|allowed| !allowed(value.units())) {
                if self.config.replace_with_plaintext {
                    return Some(convert_tag_to_plaintext(element.clone()));
                }
                remove_attribute(element, attribute);
            }
        }
        None
    }

    fn sanitize_nodes(&self, nodes: Vec<Node>) -> Vec<Node> {
        let mut out = Vec::with_capacity(nodes.len());
        for node in nodes {
            let mut element = match node {
                Node::Element(element) => element,
                // Um comentário nunca está em `ALLOWED_TAGS`: sai, ou vira texto com `replaceWithPlaintext`.
                comment @ Node::Comment(_) => {
                    if self.config.replace_with_plaintext {
                        out.extend(convert_node_to_plaintext(comment).unwrap_or_default());
                    }
                    continue;
                }
                text => {
                    out.push(text);
                    continue;
                }
            };
            if !self.tags.contains(&element.tag_name) {
                if self.config.replace_with_plaintext {
                    out.extend(self.sanitize_nodes(convert_tag_to_plaintext(element)));
                } else if !FORBID_CONTENTS.contains(&element.tag_name.as_str()) {
                    out.extend(self.sanitize_nodes(element.children));
                }
                continue;
            }
            element.attributes = self.sanitize_attributes(&element);
            if let Some(replacement) = self.after_sanitize_attributes(&mut element) {
                out.extend(self.sanitize_nodes(replacement));
                continue;
            }
            element.children = self.sanitize_nodes(std::mem::take(&mut element.children));
            out.push(Node::Element(element));
        }
        out
    }
}

/// `sanitizeHtml`/`safeSetInnerHtml`: os filhos de `body` já sanitizados, no lugar do `DocumentFragment`.
pub fn sanitize_html(untrusted: &[u16], config: &DomSanitizerConfig<'_>) -> Vec<Node> {
    let body = parse_body(untrusted);
    Sanitizer::new(config).sanitize_nodes(body.children)
}
