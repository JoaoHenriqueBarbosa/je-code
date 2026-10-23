//! Porte de `src/vs/platform/notification/test/common/notificationMessage.test.ts` (suíte `NotificationText`), caso a caso e na mesma ordem.
//!
//! Pendente: `does not expose mutable nodes` (o `Object.isFrozen` dos nós) e `requires nominal notification
//! content and capabilities at every API boundary` (asserções de tipo do TypeScript): em Rust a imutabilidade
//! e a nominalidade são garantidas pelo compilador (`NotificationText` só expõe `&[LinkedTextNode]` e só se
//! constrói por `link`, `concat` e `format`), sem nada para medir em tempo de execução.

use je_base::JsString;
use je_base::icon_labels::strip_icons;
use je_base::linked_text::{Link, LinkedTextNode};
use je_platform::notification::notification_legacy::{LEGACY_EXTENSION_LINK_PARSING, is_legacy_extension_link_parsing};
use je_platform::notification::notification_message::{NotificationPart, NotificationText};

fn text(value: &str) -> LinkedTextNode {
    LinkedTextNode::Text(JsString::from(value))
}

fn link(label: &str, href: &str, title: Option<&str>) -> LinkedTextNode {
    LinkedTextNode::Link(Link { label: JsString::from(label), href: JsString::from(href), title: title.map(JsString::from) })
}

fn rich(value: NotificationText) -> NotificationPart {
    NotificationPart::Rich(value)
}

#[test]
fn formats_literal_arguments_and_explicit_links_without_interpreting_markup() {
    let filename = "README.md [Open](command:unexpected \"Open README\")";
    let link_text = NotificationText::link("[Show logs](command:unexpected)", "command:showLogs?%5B%22file%22%5D", None);
    let formatted = NotificationText::format("Failed to open {0}. {1}", &[filename.into(), rich(link_text)]);
    assert_eq!(
        formatted.nodes(),
        &[
            text("Failed to open "),
            text(filename),
            text(". "),
            link("[Show logs](command:unexpected)", "command:showLogs?%5B%22file%22%5D", None),
        ]
    );
}

#[test]
fn formats_reordered_repeated_and_missing_placeholders_without_expanding_arguments_recursively() {
    let link_text = NotificationText::link("Details", "https://example.com", None);
    let formatted = NotificationText::format("{1}: {0}, {1}, {2}", &["{1}".into(), rich(link_text)]);
    assert_eq!(
        formatted.nodes(),
        &[
            link("Details", "https://example.com", None),
            text(": "),
            text("{1}"),
            text(", "),
            link("Details", "https://example.com", None),
            text(", "),
            text("{2}"),
        ]
    );
}

#[test]
fn formats_empty_and_literal_templates() {
    assert!(NotificationText::format("", &[]).nodes().is_empty());
    assert_eq!(NotificationText::format("[Open](command:unexpected)", &[]).nodes(), &[text("[Open](command:unexpected)")]);
}

#[test]
fn composes_rich_fragments_and_preserves_link_targets_when_transforming_display_text() {
    let message = NotificationText::concat([
        "$(sync) Working. ".into(),
        rich(NotificationText::link("$(info) [Logs](command:unexpected)", "command:showLogs?%5B%22file%22%5D", Some("Show Logs"))),
    ]);
    let mapped = message.map_text(|value| strip_icons(value.units()));
    assert_eq!(message.to_string(), "$(sync) Working. $(info) [Logs](command:unexpected)");
    assert_eq!(
        mapped.nodes(),
        &[text(" Working. "), link(" [Logs](command:unexpected)", "command:showLogs?%5B%22file%22%5D", Some("Show Logs"))]
    );
    assert_eq!(mapped.to_string(), " Working.  [Logs](command:unexpected)");
}

#[test]
fn checks_legacy_capability_identity_rather_than_its_shape_or_truthiness() {
    // Só a capacidade (e a ausência de valor) chega aqui: `true`, `false`, `{}` e outro `Symbol` do
    // original não são representáveis no tipo.
    assert!(is_legacy_extension_link_parsing(Some(&LEGACY_EXTENSION_LINK_PARSING)));
    assert!(!is_legacy_extension_link_parsing(None));
}
