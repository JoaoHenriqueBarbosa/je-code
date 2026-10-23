//! Porte de `src/vs/base/test/common/linkedText.test.ts` (suíte `LinkedText`), caso a caso e na mesma ordem.

use je_base::JsString;
use je_base::linked_text::{Link, LinkedTextNode, parse_linked_text};

fn parse(text: &str) -> Vec<LinkedTextNode> {
    let units: Vec<u16> = text.encode_utf16().collect();
    parse_linked_text(&units).nodes
}

fn text(s: &str) -> LinkedTextNode {
    LinkedTextNode::Text(JsString::from(s))
}

fn link(label: &str, href: &str, title: Option<&str>) -> LinkedTextNode {
    LinkedTextNode::Link(Link { label: JsString::from(label), href: JsString::from(href), title: title.map(JsString::from) })
}

#[test]
fn parses_correctly() {
    assert_eq!(parse(""), Vec::<LinkedTextNode>::new());
    assert_eq!(parse("hello"), vec![text("hello")]);
    assert_eq!(parse("hello there"), vec![text("hello there")]);
    assert_eq!(
        parse("Some message with [link text](http://link.href)."),
        vec![text("Some message with "), link("link text", "http://link.href", None), text(".")]
    );
    assert_eq!(
        parse("Some message with [link text](http://link.href \"and a title\")."),
        vec![text("Some message with "), link("link text", "http://link.href", Some("and a title")), text(".")]
    );
    assert_eq!(
        parse("Some message with [link text](http://link.href 'and a title')."),
        vec![text("Some message with "), link("link text", "http://link.href", Some("and a title")), text(".")]
    );
    assert_eq!(
        parse("Some message with [link text](http://link.href \"and a 'title'\")."),
        vec![text("Some message with "), link("link text", "http://link.href", Some("and a 'title'")), text(".")]
    );
    assert_eq!(
        parse("Some message with [link text](http://link.href 'and a \"title\"')."),
        vec![text("Some message with "), link("link text", "http://link.href", Some("and a \"title\"")), text(".")]
    );
    assert_eq!(parse("Some message with [link text](random stuff)."), vec![text("Some message with [link text](random stuff).")]);
    assert_eq!(
        parse("Some message with [https link](https://link.href)."),
        vec![text("Some message with "), link("https link", "https://link.href", None), text(".")]
    );
    assert_eq!(parse("Some message with [https link](https:)."), vec![text("Some message with [https link](https:).")]);
    assert_eq!(
        parse("Some message with [a command](command:foobar)."),
        vec![text("Some message with "), link("a command", "command:foobar", None), text(".")]
    );
    assert_eq!(parse("Some message with [a command](command:)."), vec![text("Some message with [a command](command:).")]);
    assert_eq!(
        parse("link [one](command:foo \"nice\") and link [two](http://foo)..."),
        vec![
            text("link "),
            link("one", "command:foo", Some("nice")),
            text(" and link "),
            link("two", "http://foo", None),
            text("..."),
        ]
    );
    assert_eq!(
        parse("link\n[one](command:foo \"nice\")\nand link [two](http://foo)..."),
        vec![
            text("link\n"),
            link("one", "command:foo", Some("nice")),
            text("\nand link "),
            link("two", "http://foo", None),
            text("..."),
        ]
    );
}

#[test]
fn should_match_non_greedily() {
    assert_eq!(
        parse("a [link text 1](http://link.href \"title1\") b [link text 2](http://link.href \"title2\") c"),
        vec![
            text("a "),
            link("link text 1", "http://link.href", Some("title1")),
            text(" b "),
            link("link text 2", "http://link.href", Some("title2")),
            text(" c"),
        ]
    );
}
