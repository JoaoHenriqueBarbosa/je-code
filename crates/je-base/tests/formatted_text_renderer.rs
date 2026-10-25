//! Porte de `src/vs/base/test/browser/formattedTextRenderer.test.ts` (suíte `FormattedTextRenderer`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - O resultado é o [`Element`] do `html_parser` (sem DOM): `innerHTML` é `markdown::dom::inner_html`, `tagName` é a tag em
//!   minúsculas (o `DIV` e o `B` do original), `children.length` é a contagem de filhos que são elementos e `nodeType ===
//!   ELEMENT_NODE` vale por construção do tipo.
//! - `renderText('testing', {}, target)` devolve o próprio `target` no original (`assert.strictEqual(result, target)`); aqui o
//!   `target` entra por valor, então o caso confere que o elemento devolvido é o mesmo (tag e `class`) com o texto no lugar
//!   dos filhos.
//! - `document.createEvent('MouseEvent')` + `dispatchEvent(event)` no nó é `RenderedFormattedText::dispatch_click` com o
//!   caminho do nó (`firstChild.firstChild.firstChild` é `[0, 0, 0]`).
//! - `store` (`new DisposableStore()`, limpo no `setup` e no `teardown`) é um `DisposableStore` novo por caso, entregue ao
//!   `LeakGuard`, que o descarta no fim; `ensureNoDisposablesAreLeakedInTestSuite()` é o `LeakGuard` de cada caso.

mod common;

use std::cell::Cell;
use std::rc::Rc;

use common::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite};
use je_base::browser::formatted_text_renderer::{
    ContentActionEvent, ContentActionHandler, FormattedTextRenderOptions, RenderedFormattedText, render_formatted_text, render_text,
};
use je_base::browser::html_parser::{Attribute, Element, Node};
use je_base::lifecycle::DisposableStore;
use je_base::markdown::dom::inner_html;
use je_base::JsString;

fn setup() -> (LeakGuard, Rc<DisposableStore>) {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let store = guard.add_rc(DisposableStore::new());
    (guard, store)
}

fn html(rendered: &RenderedFormattedText) -> String {
    inner_html(&rendered.element).to_string_lossy()
}

fn element_children(rendered: &RenderedFormattedText) -> usize {
    rendered.element.element_children().count()
}

fn first_child(rendered: &RenderedFormattedText) -> &Node {
    rendered.element.children.first().expect("o elemento tem um primeiro filho")
}

fn text_content(node: &Node) -> String {
    match node {
        // o `textContent` de um comentário é o `data` dele, como no DOM
        Node::Text(text) | Node::Comment(text) => text.to_string_lossy(),
        Node::Element(element) => element.text_content().to_string_lossy(),
    }
}

/// O `actionHandler` do original: o `callback` confere que o conteúdo é `'0'` e marca que foi chamado.
fn action_options(store: &Rc<DisposableStore>, render_code_segments: bool, callback_called: &Rc<Cell<bool>>) -> FormattedTextRenderOptions {
    let called = Rc::clone(callback_called);
    FormattedTextRenderOptions {
        action_handler: Some(ContentActionHandler {
            callback: Rc::new(move |content: &str, _event: &ContentActionEvent| {
                assert_eq!(content, "0");
                called.set(true);
            }),
            disposables: Rc::clone(store),
        }),
        render_code_segments,
    }
}

#[test]
fn render_simple_element() {
    let (_guard, _store) = setup();
    let result = render_text("testing", None, None);

    assert_eq!(result.element.text_content().to_string_lossy(), "testing");
    assert_eq!(result.element.tag_name, "div");
}

#[test]
fn render_element_with_target() {
    let (_guard, _store) = setup();
    let target = Element::new("div", vec![Attribute { name: "class".to_owned(), value: JsString::from("testClass") }]);
    let result = render_text("testing", Some(&FormattedTextRenderOptions::default()), Some(target));
    assert_eq!(result.element.tag_name, "div");
    assert_eq!(result.element.get_attribute("class").map(JsString::to_string_lossy).as_deref(), Some("testClass"));
    assert_eq!(result.element.text_content().to_string_lossy(), "testing");
}

#[test]
fn simple_formatting() {
    let (_guard, _store) = setup();
    let mut result = render_formatted_text("**bold**", None, None);
    assert_eq!(element_children(&result), 1);
    assert_eq!(text_content(first_child(&result)), "bold");
    match first_child(&result) {
        Node::Element(element) => assert_eq!(element.tag_name, "b"),
        other => panic!("esperava um elemento, veio {other:?}"),
    }
    assert_eq!(html(&result), "<b>bold</b>");

    result = render_formatted_text("__italics__", None, None);
    assert_eq!(html(&result), "<i>italics</i>");

    result = render_formatted_text("``code``", None, None);
    assert_eq!(html(&result), "``code``");

    let code_options = FormattedTextRenderOptions { render_code_segments: true, ..FormattedTextRenderOptions::default() };
    result = render_formatted_text("``code``", Some(&code_options), None);
    assert_eq!(html(&result), "<code>code</code>");

    result = render_formatted_text("this string has **bold**, __italics__, and ``code``!!", Some(&code_options), None);
    assert_eq!(html(&result), "this string has <b>bold</b>, <i>italics</i>, and <code>code</code>!!");
}

#[test]
fn no_formatting() {
    let (_guard, _store) = setup();
    let result = render_formatted_text("this is just a string", None, None);
    assert_eq!(html(&result), "this is just a string");
}

#[test]
fn preserve_newlines() {
    let (_guard, _store) = setup();
    let result = render_formatted_text("line one\nline two", None, None);
    assert_eq!(html(&result), "line one<br>line two");
}

#[test]
fn action() {
    let (_guard, store) = setup();
    let callback_called = Rc::new(Cell::new(false));
    let result = render_formatted_text("[[action]]", Some(&action_options(&store, false, &callback_called)), None);
    assert_eq!(html(&result), "<a>action</a>");

    assert!(result.dispatch_click(&[0]));
    assert!(callback_called.get());
}

#[test]
fn fancy_action() {
    let (_guard, store) = setup();
    let callback_called = Rc::new(Cell::new(false));
    let result = render_formatted_text("__**[[action]]**__", Some(&action_options(&store, false, &callback_called)), None);
    assert_eq!(html(&result), "<i><b><a>action</a></b></i>");

    assert!(result.dispatch_click(&[0, 0, 0]));
    assert!(callback_called.get());
}

#[test]
fn fancier_action() {
    let (_guard, store) = setup();
    let callback_called = Rc::new(Cell::new(false));
    let result = render_formatted_text("``__**[[action]]**__``", Some(&action_options(&store, true, &callback_called)), None);
    assert_eq!(html(&result), "<code><i><b><a>action</a></b></i></code>");

    assert!(result.dispatch_click(&[0, 0, 0, 0]));
    assert!(callback_called.get());
}

#[test]
fn escaped_formatting() {
    let (_guard, _store) = setup();
    let result = render_formatted_text("\\*\\*bold\\*\\*", None, None);
    assert_eq!(element_children(&result), 0);
    assert_eq!(html(&result), "**bold**");
}
