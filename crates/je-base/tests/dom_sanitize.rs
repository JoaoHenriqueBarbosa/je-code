//! Porte de `src/vs/base/test/browser/domSanitize.test.ts` (suíte `DomSanitize`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `sanitizeHtml(html, config).toString()` é `sanitize_html` sobre a árvore do `html_parser`, serializada por
//!   `markdown::dom::inner_html` (os filhos de um `body` que embrulha os nós devolvidos). O `TrustedHTML` não existe.
//! - A configuração `{ allowedTags: { override: [...] } }` vira os campos planos de `DomSanitizerConfig`
//!   (`allowed_tags_override` e os outros). `Schemas.*` é `network::schemas`. O predicado de atributo recebe o elemento, o
//!   nome e o valor, e devolve `KeepAttr` (`boolean | string`).
//! - `new DOMParser().parseFromString(html, 'text/html')` e `querySelector('[href]')` são `parse_body` e uma busca
//!   recursiva por elemento com o atributo.
//! - O caso `plaintext replacements remain in the source document` confere o `textContent` da substituição; a parte
//!   `ownerDocument === sourceDocument` não tem equivalente, porque a árvore de valores não tem documento dono.
//! - Os dois casos da suíte `stale trusted types policy` (`sanitizeSurvivingStalePolicy`) conferem o comportamento
//!   observável que a recuperação garante: a saída do sanitizador não muda quando uma política de Trusted Types fica obsoleta e uma
//!   falha que não é disso não é repetida. Não há Trusted Types nem dompurify aqui, então não há política que expire: o primeiro
//!   caso confere que duas chamadas seguidas devolvem a mesma saída (a segunda é a "nova tentativa"), e o segundo que cada
//!   chamada avalia a regra de atributo uma vez, sem repetir.
//! - Um comentário HTML é `Node::Comment` na árvore, e o sanitizador o troca por `<!--...-->` com `replaceWithPlaintext`.

mod common;

use std::rc::Rc;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::JsString;
use je_base::browser::dom_sanitize::{AttributeRule, DomSanitizerConfig, KeepAttr, Protocols, SanitizeAttributeRule, convert_tag_to_plaintext, sanitize_html};
use je_base::browser::html_parser::{Element, Node, parse_body};
use je_base::markdown::dom::inner_html;
use je_base::network::schemas;

fn units(text: &str) -> Vec<u16> {
    text.encode_utf16().collect()
}

fn body_of(nodes: Vec<Node>) -> Element {
    Element { tag_name: "body".to_owned(), attributes: Vec::new(), children: nodes }
}

/// `sanitizeHtml(html, config).toString()`.
fn sanitize(html: &str, config: &DomSanitizerConfig<'_>) -> String {
    inner_html(&body_of(sanitize_html(&units(html), config))).to_string_lossy()
}

fn strings(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_owned()).collect()
}

fn names(values: &[&str]) -> Vec<AttributeRule> {
    values.iter().map(|value| AttributeRule::Name((*value).to_owned())).collect()
}

fn protocols(values: &[&str]) -> Option<Protocols> {
    Some(Protocols::List(strings(values)))
}

fn source_starts_with_data() -> Option<Rc<dyn Fn(&[u16]) -> bool>> {
    let allowed: Rc<dyn Fn(&[u16]) -> bool> = Rc::new(|source: &[u16]| source.starts_with(&units("data:")));
    Some(allowed)
}

fn source_never_allowed() -> Option<Rc<dyn Fn(&[u16]) -> bool>> {
    let allowed: Rc<dyn Fn(&[u16]) -> bool> = Rc::new(|_: &[u16]| false);
    Some(allowed)
}

/// `querySelector('[href]')` (ou `[href="..."]` quando `value` vem) sobre os descendentes.
fn has_href(nodes: &[Node], value: Option<&str>) -> bool {
    nodes.iter().any(|node| match node {
        Node::Element(element) => {
            let matches = element.get_attribute("href").is_some_and(|href| value.is_none_or(|wanted| href.to_string_lossy() == wanted));
            matches || has_href(&element.children, value)
        }
        _ => false,
    })
}

#[test]
fn removes_unsupported_tags_by_default() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let html = "<div>safe<script>alert(1)</script>content</div>";
    let result = sanitize(html, &DomSanitizerConfig::default());

    assert!(result.contains("<div>"));
    assert!(result.contains("safe"));
    assert!(result.contains("content"));
    assert!(!result.contains("<script>"));
    assert!(!result.contains("alert(1)"));
}

#[test]
fn removes_unsupported_attributes_by_default() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let html = r#"<div onclick="alert(1)" title="safe">content</div>"#;
    let result = sanitize(html, &DomSanitizerConfig::default());

    assert!(result.contains(r#"<div title="safe">"#));
    assert!(!result.contains("onclick"));
    assert!(!result.contains("alert(1)"));
}

#[test]
fn allows_custom_tags_via_config() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    {
        let html = "<div>removed</div><custom-tag>hello</custom-tag>";
        let result = sanitize(html, &DomSanitizerConfig { allowed_tags_override: Some(strings(&["custom-tag"])), ..Default::default() });
        assert_eq!(result, "removed<custom-tag>hello</custom-tag>");
    }
    {
        let html = "<div>kept</div><augmented-tag>world</augmented-tag>";
        let result = sanitize(html, &DomSanitizerConfig { allowed_tags_augment: Some(strings(&["augmented-tag"])), ..Default::default() });
        assert_eq!(result, "<div>kept</div><augmented-tag>world</augmented-tag>");
    }
}

#[test]
fn allows_custom_attributes_via_config() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let html = r#"<div custom-attr="value">content</div>"#;
    let result = sanitize(html, &DomSanitizerConfig { allowed_attributes_override: Some(names(&["custom-attr"])), ..Default::default() });

    assert!(result.contains(r#"custom-attr="value""#));
}

#[test]
fn attributes_in_config_should_be_case_insensitive() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let html = r#"<div Custom-Attr="value">content</div>"#;

    {
        let result = sanitize(html, &DomSanitizerConfig { allowed_attributes_override: Some(names(&["custom-attr"])), ..Default::default() });
        assert!(result.contains(r#"custom-attr="value""#));
    }
    {
        let result = sanitize(html, &DomSanitizerConfig { allowed_attributes_override: Some(names(&["CUSTOM-ATTR"])), ..Default::default() });
        assert!(result.contains(r#"custom-attr="value""#));
    }
}

#[test]
fn removes_unsupported_protocols_for_href_by_default() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let html = r#"<a href="javascript:alert(1)">bad link</a>"#;
    let result = sanitize(html, &DomSanitizerConfig::default());

    assert!(result.contains("<a>bad link</a>"));
    assert!(!result.contains("javascript:"));
}

#[test]
fn removes_unsupported_protocols_for_src_by_default() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let html = r#"<img alt="text" src="javascript:alert(1)">"#;
    let result = sanitize(html, &DomSanitizerConfig::default());

    assert!(result.contains(r#"<img alt="text">"#));
    assert!(!result.contains("javascript:"));
}

#[test]
fn allows_safe_protocols_for_href() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let html = r#"<a href="https://example.com">safe link</a>"#;
    let result = sanitize(html, &DomSanitizerConfig::default());

    assert!(result.contains(r#"href="https://example.com""#));
}

#[test]
fn allows_fragment_links() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let html = r##"<a href="#section">fragment link</a>"##;
    let result = sanitize(html, &DomSanitizerConfig::default());

    assert!(result.contains(r##"href="#section""##));
}

const DATA_IMAGE: &str = r#"<img src="data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8/5+hHgAHggJ/PchI7wAAAABJRU5ErkJggg==">"#;

#[test]
fn removes_data_images_by_default() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let result = sanitize(DATA_IMAGE, &DomSanitizerConfig::default());

    assert!(result.contains("<img>"));
    assert!(!result.contains(r#"src="data:"#));
}

#[test]
fn allows_data_images_when_enabled() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let result = sanitize(DATA_IMAGE, &DomSanitizerConfig { allowed_media_protocols: protocols(&[schemas::DATA]), ..Default::default() });

    assert!(result.contains(r#"src="data:image/png;base64,"#));
}

#[test]
fn removes_relative_paths_for_img_src_by_default() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let html = r#"<img src="path/img.png">"#;
    assert_eq!(sanitize(html, &DomSanitizerConfig::default()), "<img>");
}

#[test]
fn can_allow_relative_paths_for_image() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let html = r#"<img src="path/img.png">"#;
    let result = sanitize(html, &DomSanitizerConfig { allow_relative_media_paths: true, ..Default::default() });
    assert_eq!(result, r#"<img src="path/img.png">"#);
}

#[test]
fn supports_additional_media_source_validation() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let html = r#"<img src="https://example.com/collect?secret=value" alt="remote"><img src="data:image/png;base64,AA==" alt="data">"#;
    let config = DomSanitizerConfig {
        allowed_media_protocols: protocols(&[schemas::HTTP, schemas::HTTPS, schemas::DATA]),
        media_source_is_allowed: source_starts_with_data(),
        ..Default::default()
    };

    let actual = (
        sanitize(html, &config),
        sanitize(html, &DomSanitizerConfig { replace_with_plaintext: true, ..config.clone() }),
    );

    assert_eq!(
        actual,
        (
            r#"<img alt="remote"><img src="data:image/png;base64,AA==" alt="data">"#.to_owned(),
            r#"&lt;img src="https://example.com/collect?secret=value" alt="remote"&gt;<img src="data:image/png;base64,AA==" alt="data">"#.to_owned(),
        )
    );
}

#[test]
fn applies_additional_media_source_validation_to_poster_attributes() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let html = r#"<video poster="https://example.invalid/poster.png"></video>"#;
    let result = sanitize(
        html,
        &DomSanitizerConfig {
            allowed_tags_override: Some(strings(&["video"])),
            allowed_attributes_override: Some(names(&["poster"])),
            allowed_media_protocols: protocols(&[schemas::HTTPS]),
            media_source_is_allowed: source_never_allowed(),
            replace_with_plaintext: true,
            ..Default::default()
        },
    );

    assert_eq!(result, r#"&lt;video poster="https://example.invalid/poster.png"&gt;&lt;/video&gt;"#);
}

#[test]
fn applies_media_source_validation_to_non_anchor_href_attributes() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let sanitize_href = |href: &str| {
        let html = format!("<svg><linearGradient href=\"{href}\"></linearGradient></svg>");
        let result = sanitize(
            &html,
            &DomSanitizerConfig {
                allowed_tags_override: Some(strings(&["svg", "lineargradient"])),
                allowed_attributes_override: Some(names(&["href"])),
                allowed_media_protocols: protocols(&[schemas::HTTPS]),
                media_source_is_allowed: source_never_allowed(),
                replace_with_plaintext: true,
                ..Default::default()
            },
        );
        parse_body(&units(&result)).children
    };

    let external = has_href(&sanitize_href("https://example.invalid/gradient.svg#paint"), None);
    let local_fragment = has_href(&sanitize_href("#paint"), Some("#paint"));

    assert_eq!((external, local_fragment), (false, true));
}

#[test]
fn plaintext_replacements_remain_in_the_source_document() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let source = parse_body(&units("<div><span>content</span></div>"));
    let Some(Node::Element(node)) = source.children.into_iter().next() else {
        panic!("o primeiro filho do body é o div");
    };
    let replacement = convert_tag_to_plaintext(node);

    assert_eq!(body_of(replacement).text_content().to_string_lossy(), "<div>content</div>");
}

#[test]
fn supports_dynamic_attribute_sanitization() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let html = r#"<div title="a" other="1">text1</div><div title="b" other="2">text2</div>"#;
    let result = sanitize(
        html,
        &DomSanitizerConfig {
            allowed_attributes_override: Some(vec![AttributeRule::Rule(SanitizeAttributeRule {
                attribute_name: "title".to_owned(),
                should_keep: Rc::new(|_el: &Element, _name: &str, value: &[u16]| KeepAttr::Keep(value.contains(&u16::from(b'b')))),
            })]),
            ..Default::default()
        },
    );
    assert_eq!(result, r#"<div>text1</div><div title="b">text2</div>"#);
}

#[test]
fn supports_changing_attributes_in_dynamic_sanitization() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let html = r#"<div title="abc" other="1">text1</div><div title="xyz" other="2">text2</div>"#;
    let result = sanitize(
        html,
        &DomSanitizerConfig {
            allowed_attributes_override: Some(vec![AttributeRule::Rule(SanitizeAttributeRule {
                attribute_name: "title".to_owned(),
                should_keep: Rc::new(|_el: &Element, _name: &str, value: &[u16]| {
                    if value == units("abc").as_slice() {
                        return KeepAttr::Keep(false);
                    }
                    let mut doubled = JsString::from_units(value.to_vec());
                    doubled.push_units(value);
                    KeepAttr::Replace(doubled)
                }),
            })]),
            ..Default::default()
        },
    );
    // xyz title should be preserved and doubled
    assert_eq!(result, r#"<div>text1</div><div title="xyzxyz">text2</div>"#);
}

#[test]
fn attr_name_should_clear_previously_set_dynamic_sanitizer() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let html = r#"<div title="abc" other="1">text1</div><div title="xyz" other="2">text2</div>"#;
    let result = sanitize(
        html,
        &DomSanitizerConfig {
            allowed_attributes_override: Some(vec![
                AttributeRule::Rule(SanitizeAttributeRule {
                    attribute_name: "title".to_owned(),
                    should_keep: Rc::new(|_el: &Element, _name: &str, _value: &[u16]| KeepAttr::Keep(false)),
                }),
                // Should allow everything since it comes after custom rule
                AttributeRule::Name("title".to_owned()),
            ]),
            ..Default::default()
        },
    );
    assert_eq!(result, r#"<div title="abc">text1</div><div title="xyz">text2</div>"#);
}

mod replace_with_plaintext {
    use super::*;

    fn plaintext() -> DomSanitizerConfig<'static> {
        DomSanitizerConfig { replace_with_plaintext: true, ..Default::default() }
    }

    #[test]
    fn replaces_unsupported_tags_with_plaintext_representation() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let html = "<div>safe<script>alert(1)</script>content</div>";
        assert_eq!(sanitize(html, &plaintext()), "<div>safe&lt;script&gt;alert(1)&lt;/script&gt;content</div>");
    }

    #[test]
    fn handles_self_closing_tags_correctly() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let html = r#"<div><input type="text"><custom-input /></div>"#;
        assert_eq!(sanitize(html, &plaintext()), r#"<div>&lt;input type="text"&gt;&lt;custom-input&gt;&lt;/custom-input&gt;</div>"#);
    }

    #[test]
    fn handles_tags_with_attributes() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let html = r#"<div><unknown-tag class="test" id="myid">content</unknown-tag></div>"#;
        assert_eq!(sanitize(html, &plaintext()), r#"<div>&lt;unknown-tag class="test" id="myid"&gt;content&lt;/unknown-tag&gt;</div>"#);
    }

    #[test]
    fn handles_nested_unsupported_tags() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let html = "<div><outer><inner>nested</inner></outer></div>";
        assert_eq!(sanitize(html, &plaintext()), "<div>&lt;outer&gt;&lt;inner&gt;nested&lt;/inner&gt;&lt;/outer&gt;</div>");
    }

    #[test]
    fn handles_comments_correctly() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let html = "<div><!-- this is a comment -->content</div>";
        assert_eq!(sanitize(html, &plaintext()), "<div>&lt;!-- this is a comment --&gt;content</div>");
    }

    #[test]
    fn handles_empty_tags() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let html = "<div><empty></empty></div>";
        assert_eq!(sanitize(html, &plaintext()), "<div>&lt;empty&gt;&lt;/empty&gt;</div>");
    }

    #[test]
    fn works_with_custom_allowed_tags_configuration() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let html = "<div><custom>allowed</custom><forbidden>not allowed</forbidden></div>";
        let result = sanitize(html, &DomSanitizerConfig { replace_with_plaintext: true, allowed_tags_augment: Some(strings(&["custom"])), ..Default::default() });
        assert_eq!(result, "<div><custom>allowed</custom>&lt;forbidden&gt;not allowed&lt;/forbidden&gt;</div>");
    }
}

mod stale_trusted_types_policy {
    use std::cell::Cell;

    use super::*;

    #[test]
    fn retries_with_a_replacement_policy_when_the_sanitizer_policy_stops_working() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let html = "<div>safe</div>";
        let config = DomSanitizerConfig::default();

        // A primeira chamada é a que falharia com a política obsoleta; a segunda é a nova tentativa.
        let first = sanitize(html, &config);
        let second = sanitize(html, &config);

        assert_eq!((first, second), ("<div>safe</div>".to_owned(), "<div>safe</div>".to_owned()));
    }

    #[test]
    fn a_failure_that_is_not_a_stale_policy_is_not_retried() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let attempts = Rc::new(Cell::new(0));
        let counter = attempts.clone();
        let config = DomSanitizerConfig {
            allowed_attributes_override: Some(vec![AttributeRule::Rule(SanitizeAttributeRule {
                attribute_name: "title".to_owned(),
                should_keep: Rc::new(move |_el: &Element, _name: &str, _value: &[u16]| {
                    counter.set(counter.get() + 1);
                    KeepAttr::Keep(true)
                }),
            })]),
            ..Default::default()
        };

        let result = sanitize(r#"<div title="x">safe</div>"#, &config);

        assert_eq!((result.as_str(), attempts.get()), (r#"<div title="x">safe</div>"#, 1));
    }
}
