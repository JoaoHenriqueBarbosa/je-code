//! `domSanitize.ts` vive em [`crate::browser::dom_sanitize`] (um sanitizador só); este módulo reexporta a API para
//! o `markdownRenderer`.

pub use crate::browser::dom_sanitize::{
    AttributeRule, BASIC_MARKUP_HTML_TAGS, DEFAULT_ALLOWED_ATTRS, DomSanitizerConfig, KeepAttr, Protocols, SanitizeAttributePredicate, SanitizeAttributeRule,
    convert_node_to_plaintext, convert_tag_to_plaintext, sanitize_html,
};
