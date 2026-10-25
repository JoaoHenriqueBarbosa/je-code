//! Porte de `src/vs/base/test/browser/markdownRenderer.test.ts` (suíte `MarkdownRenderer`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - O resultado de `renderMarkdown` é a árvore `Element`/`Node` do `html_parser` (sem DOM). `innerHTML` e `outerHTML`
//!   são a serialização de `markdown::dom` (`inner_html`, `outer_html`), `assertNodeEquals` é a comparação estrutural
//!   `isEqualNode` (tag, conjunto de atributos e filhos, em ordem) contra o `DOMParser` do original, e `querySelector`
//!   e `querySelectorAll` por tag são percorridos aqui mesmo.
//! - `result.element` é `Rc<RefCell<Element>>` (os blocos de código assíncronos entram depois). O que
//!   `store.add(renderMarkdown(...))` faz é registrar um clone do `RenderedMarkdown` (que é `IDisposable`) no guarda de
//!   vazamentos, que descarta ao fim do teste.
//! - O original roda no Node (`isWeb` falso): os casos `if (isWeb) return;` rodam todos. O ambiente é Posix, com o
//!   `FileAccess` nativo de `network.rs`, que resolve `file:` para `vscode-file://vscode-app/...`.
//! - `img.src` e `a.title` são lidos como atributo (o `src` do DOM resolveria contra o documento; os valores dos casos
//!   já são absolutos). `dataset.href` é o atributo `data-href`.
//! - Os `setTimeout` de 10 ms dos casos de `asyncRenderCallback` rodam com o relógio virtual (`run_with_faked_timers`).
//! - `Code block reuse`: o `replaceChildrenPreservingCodeBlocks` mantém nós montados; numa árvore de valores não existe
//!   identidade de nó nem `MutationObserver`. Os sete casos de "igual a uma renderização nova" rodam com `target`
//!   (`render_markdown_into`) e conferem a árvore; o caso das mutações confere o que é expressável (os blocos
//!   reaproveitados entram na árvore reutilizada, que é a mesma de uma renderização nova) e não as identidades.
//! - `ignoreRaw` troca só o `raw` dos tokens de topo (`Token::raw_mut`), como o original; `assert.equal(newTokens, tokens)`
//!   e `deepStrictEqual` viram igualdade de `TokensList` (tokens e mapa `links`).
//! - `simpleMarkdownTestSuite(name, delimiter)` vira a macro `simple_markdown_test_suite!`, instanciada uma vez por
//!   delimitador, e os `test.skip` viram `#[ignore]`.
//! - `renderAsPlaintext(str, options)` é `render_as_plaintext_with` (o `useLinkFormatter` não é lido por nenhum caso).

mod common;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use common::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite, run_with_faked_timers, timeout};
use je_base::JsString;
use je_base::browser::html_parser::{Element, Node, parse_body};
use je_base::html_content::{IMarkdownString, IsTrusted, MarkdownString, MarkdownStringOptions, MarkdownStringTextNewlineStyle, TrustedOrOptions};
use je_base::lifecycle::IDisposable;
use je_base::map::OrderedMap;
use je_base::markdown::dom::{inner_html, outer_html};
use je_base::markdown::{
    MarkdownEnvironment, MarkdownLinkKind, MarkdownRenderOptions, MarkdownSanitizerConfig, RenderedMarkdown, fill_in_incomplete_tokens, render_markdown,
    render_markdown_into,
};
use je_base::marked::{TokensList, lexer};
use je_base::markdown_plaintext::{PlaintextOptions, render_as_plaintext_with};
use je_base::marshalling::{self, Marshalled};
use je_base::network::{FileAccess, FileAccessContext, RemoteAuthorities};
use je_base::path::{FixedEnvironment, Platform};
use je_base::promise::Promise;
use je_base::resources::ExtUri;
use je_base::runtime;
use je_base::uri::{UriComponents, URI};

fn units(text: &str) -> Vec<u16> {
    text.encode_utf16().collect()
}

fn js(text: &str) -> JsString {
    JsString::from(text)
}

/// O que o original lê do ambiente: `resources.extUri`, `process.platform` e `FileAccess`.
struct TestEnv {
    env: Rc<FixedEnvironment>,
    ext_uri: ExtUri,
    file_access: FileAccess,
}

impl TestEnv {
    fn new() -> Self {
        let env = Rc::new(FixedEnvironment::new(Platform::Posix, "/workspace/project"));
        let ext_uri = ExtUri::unbiased(env.clone());
        let file_access = FileAccess::new(
            env.clone(),
            FileAccessContext { is_native: true, web_worker_origin: None, file_root: Some("file:///workspace/vscode/out/".to_string()) },
            RemoteAuthorities::new(false),
        );
        Self { env, ext_uri, file_access }
    }

    fn environment(&self) -> MarkdownEnvironment<'_> {
        MarkdownEnvironment { ext_uri: &self.ext_uri, env: &*self.env, file_access: &self.file_access }
    }

    /// `store.add(renderMarkdown(markdown, options))`.
    fn render(&self, guard: &LeakGuard, markdown: &IMarkdownString, options: &MarkdownRenderOptions) -> RenderedMarkdown {
        let rendered = render_markdown(markdown, options, &self.environment()).expect("renderMarkdown");
        guard.add(Rc::new(rendered.clone()));
        rendered
    }

    /// `store.add(renderMarkdown(markdown, options, target))`.
    fn render_into(&self, guard: &LeakGuard, markdown: &IMarkdownString, options: &MarkdownRenderOptions, target: &Rc<RefCell<Element>>) -> RenderedMarkdown {
        let rendered = render_markdown_into(markdown, options, &self.environment(), Some(target)).expect("renderMarkdown");
        guard.add(Rc::new(rendered.clone()));
        rendered
    }

    /// `store.add(renderMarkdown(markdown)).element`, com as opções padrão.
    fn render_default(&self, guard: &LeakGuard, markdown: &IMarkdownString) -> RenderedMarkdown {
        self.render(guard, markdown, &MarkdownRenderOptions::default())
    }
}

/// `{ value }`.
fn plain(value: &str) -> IMarkdownString {
    IMarkdownString { value: js(value), ..IMarkdownString::default() }
}

/// `new MarkdownString(value, options)`.
fn markdown_string(value: Option<&str>, options: MarkdownStringOptions) -> MarkdownString {
    MarkdownString::new(value.map(js), TrustedOrOptions::Options(options))
}

fn trusted() -> Option<IsTrusted> {
    Some(IsTrusted::Bool(true))
}

fn dto(markdown: &MarkdownString) -> IMarkdownString {
    IMarkdownString {
        value: markdown.value.clone(),
        is_trusted: markdown.is_trusted.clone(),
        support_theme_icons: markdown.support_theme_icons,
        support_html: markdown.support_html,
        support_alert_syntax: markdown.support_alert_syntax,
        base_uri: markdown.base_uri.clone(),
        uris: markdown.uris.clone(),
    }
}

fn append_text(markdown: &mut MarkdownString, text: &str) {
    markdown.append_text(&units(text), MarkdownStringTextNewlineStyle::Paragraph);
}

fn append_markdown(markdown: &mut MarkdownString, text: &str) {
    markdown.append_markdown(&units(text));
}

/// `result.innerHTML`.
fn inner(rendered: &RenderedMarkdown) -> String {
    inner_html(&rendered.element.borrow()).to_string_lossy()
}

/// `element.querySelectorAll(tag)`, em ordem de documento.
fn select_all<'a>(element: &'a Element, tag: &str) -> Vec<&'a Element> {
    fn walk<'a>(element: &'a Element, tag: &str, out: &mut Vec<&'a Element>) {
        for child in element.element_children() {
            if child.tag_name == tag {
                out.push(child);
            }
            walk(child, tag, out);
        }
    }
    let mut out = Vec::new();
    walk(element, tag, &mut out);
    out
}

fn attr(element: &Element, name: &str) -> Option<String> {
    element.get_attribute(name).map(JsString::to_string_lossy)
}

fn text_content(rendered: &RenderedMarkdown) -> String {
    rendered.element.borrow().text_content().to_string_lossy()
}

/// `a.isEqualNode(b)`: tag, atributos (sem ordem) e filhos em ordem.
fn is_equal_node(actual: &Node, expected: &Node) -> bool {
    match (actual, expected) {
        (Node::Text(a), Node::Text(b)) => a == b,
        (Node::Element(a), Node::Element(b)) => {
            let sorted = |element: &Element| {
                let mut attributes: Vec<(String, String)> = element.attributes.iter().map(|a| (a.name.clone(), a.value.to_string_lossy())).collect();
                attributes.sort();
                attributes
            };
            a.tag_name == b.tag_name
                && sorted(a) == sorted(b)
                && a.children.len() == b.children.len()
                && a.children.iter().zip(&b.children).all(|(x, y)| is_equal_node(x, y))
        }
        _ => false,
    }
}

/// `assertNodeEquals(actualNode, expectedHtml)`.
#[track_caller]
fn assert_node_equals(actual: &RenderedMarkdown, expected_html: &str) {
    let body = parse_body(&units(expected_html));
    let expected = body.children.first().expect("o HTML esperado tem um nó").clone();
    let actual_node = Node::Element(actual.element.borrow().clone());
    assert!(
        is_equal_node(&actual_node, &expected),
        "Expected: {}\nActual: {}",
        match &expected {
            Node::Element(e) => outer_html(e).to_string_lossy(),
            Node::Text(t) => t.to_string_lossy(),
        },
        outer_html(&actual.element.borrow()).to_string_lossy()
    );
}

fn code_element(text: &str) -> Element {
    Element { tag_name: "code".to_owned(), attributes: Vec::new(), children: vec![Node::Text(js(text))] }
}

fn options_with_sanitizer(config: MarkdownSanitizerConfig) -> MarkdownRenderOptions {
    MarkdownRenderOptions { sanitizer_config: config, ..MarkdownRenderOptions::default() }
}

/// `{ remoteImageIsAllowed: () => false, replaceWithPlaintext: true }`.
fn deny_remote_images() -> MarkdownRenderOptions {
    options_with_sanitizer(MarkdownSanitizerConfig {
        replace_with_plaintext: true,
        remote_image_is_allowed: Some(Rc::new(|_: &URI| false)),
        ..MarkdownSanitizerConfig::default()
    })
}

fn file_uri(path: &str) -> URI {
    URI::file(path, Platform::Posix)
}

// ----------------------------------------------------------------------------------------------------------------
// Sanitization
// ----------------------------------------------------------------------------------------------------------------

#[test]
fn sanitization_should_not_render_images_with_unknown_schemes() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let env = TestEnv::new();

    let result = env.render_default(&guard, &plain("![image](no-such://example.com/cat.gif)"));
    assert_eq!(inner(&result), "<p><img alt=\"image\"></p>");
}

#[test]
fn sanitization_strips_links_with_disallowed_schemes_default_config() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let env = TestEnv::new();

    let result = env.render_default(&guard, &plain("Read [](vscode-agent-host://my-host/path/to/foo.ts?_ah%3DeyJzY2hlbWUiOiJmaWxlIn0)"));
    // Nenhum <a> deve sobrar, porque o esquema não é permitido.
    assert!(select_all(&result.element.borrow(), "a").is_empty());
}

#[test]
fn sanitization_preserves_link_when_scheme_is_allowed_via_allowed_link_schemes_augment() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let env = TestEnv::new();

    let options = options_with_sanitizer(MarkdownSanitizerConfig { allowed_link_schemes_augment: vec!["vscode-agent-host".to_owned()], ..MarkdownSanitizerConfig::default() });
    let result = env.render(&guard, &plain("Read [](vscode-agent-host://my-host/path/to/foo.ts?_ah%3DeyJzY2hlbWUiOiJmaWxlIn0)"), &options);
    let root = result.element.borrow();
    let anchors = select_all(&root, "a");
    assert_eq!(anchors.len(), 1, "expected <a> to be preserved when scheme is allowed");
    assert_eq!(attr(anchors[0], "data-href").as_deref(), Some("vscode-agent-host://my-host/path/to/foo.ts?_ah%3DeyJzY2hlbWUiOiJmaWxlIn0"));
}

#[test]
fn sanitization_only_allows_inline_block_display_style_on_spans() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let env = TestEnv::new();

    let markdown = markdown_string(
        Some("<span style=\"display:inline-block;\">allowed</span><span style=\"display:block;\">blocked</span>"),
        MarkdownStringOptions { support_html: Some(true), ..MarkdownStringOptions::default() },
    );
    let result = env.render_default(&guard, &dto(&markdown));

    assert_eq!(inner(&result), "<p><span style=\"display:inline-block;\">allowed</span><span>blocked</span></p>");
}

#[test]
fn sanitization_transforms_parsed_link_targets_without_changing_labels_titles_or_code() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let env = TestEnv::new();

    let options = MarkdownRenderOptions {
        transform_uri: Some(Rc::new(|href: &JsString, _kind: MarkdownLinkKind| {
            if href.to_string_lossy() == "file:///same" { Some(js("https://example.com/a.ts")) } else { Some(href.clone()) }
        })),
        ..MarkdownRenderOptions::default()
    };
    let result = env.render(&guard, &plain("`[same](file:///same)` [a[b].ts](file:///same \"file:///same\") ![image](file:///same|width=10,height=20)"), &options);
    let root = result.element.borrow();
    let anchors = select_all(&root, "a");
    let images = select_all(&root, "img");
    let actual = (
        anchors.len(),
        anchors.first().map(|a| a.text_content().to_string_lossy()),
        anchors.first().and_then(|a| attr(a, "data-href")),
        anchors.first().and_then(|a| attr(a, "title")),
        images.first().and_then(|i| attr(i, "src")),
        images.first().and_then(|i| attr(i, "width")),
        images.first().and_then(|i| attr(i, "height")),
    );
    let expected = (
        1,
        Some("a[b].ts".to_owned()),
        Some("https://example.com/a.ts".to_owned()),
        Some("file:///same".to_owned()),
        Some("https://example.com/a.ts".to_owned()),
        Some("10".to_owned()),
        Some("20".to_owned()),
    );
    assert_eq!(actual, expected);
}

// ----------------------------------------------------------------------------------------------------------------
// Images
// ----------------------------------------------------------------------------------------------------------------

#[test]
fn images_image_rendering_conforms_to_default() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let env = TestEnv::new();

    let result = env.render_default(&guard, &plain("![image](http://example.com/cat.gif 'caption')"));
    assert_node_equals(&result, "<div><p><img title=\"caption\" alt=\"image\" src=\"http://example.com/cat.gif\"></p></div>");
}

#[test]
fn images_image_rendering_conforms_to_default_without_title() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let env = TestEnv::new();

    let result = env.render_default(&guard, &plain("![image](http://example.com/cat.gif)"));
    assert_node_equals(&result, "<div><p><img alt=\"image\" src=\"http://example.com/cat.gif\"></p></div>");
}

#[test]
fn images_disallowed_remote_images_are_rendered_as_plaintext() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let env = TestEnv::new();

    let checked_uris = Rc::new(RefCell::new(Vec::<String>::new()));
    let seen = checked_uris.clone();
    let options = options_with_sanitizer(MarkdownSanitizerConfig {
        replace_with_plaintext: true,
        remote_image_is_allowed: Some(Rc::new(move |uri: &URI| {
            seen.borrow_mut().push(uri.to_string(false).to_string_lossy());
            false
        })),
        ..MarkdownSanitizerConfig::default()
    });
    let result = env.render(&guard, &plain("![image](https://example.com/collect?secret=value)"), &options);

    assert_eq!(
        (checked_uris.borrow().clone(), inner(&result)),
        (vec!["https://example.com/collect?secret%3Dvalue".to_owned()], "<p>&lt;img src=\"https://example.com/collect?secret=value\" alt=\"image\"&gt;</p>".to_owned())
    );
}

#[test]
fn images_network_backed_file_images_are_rendered_as_plaintext() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let env = TestEnv::new();

    let html_source = markdown_string(Some("<img src=\"file://remote-host/share/image.png\">"), MarkdownStringOptions { support_html: Some(true), ..MarkdownStringOptions::default() });
    let markdown_sources = [plain("![image](file://remote-host/share/image.png)"), dto(&html_source)];
    let results: Vec<RenderedMarkdown> = markdown_sources.iter().map(|markdown| env.render(&guard, markdown, &deny_remote_images())).collect();

    let actual: Vec<(usize, String)> = results.iter().map(|result| (select_all(&result.element.borrow(), "img").len(), text_content(result))).collect();
    assert_eq!(
        actual,
        vec![
            (0, "<img src=\"file://remote-host/share/image.png\" alt=\"image\">".to_owned()),
            (0, "<img src=\"file://remote-host/share/image.png\">".to_owned()),
        ]
    );
}

#[test]
fn images_relative_image_with_a_network_backed_file_base_uri_is_rendered_as_plaintext() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let env = TestEnv::new();

    let mut markdown = MarkdownString::new(Some(js("![image](image.png)")), TrustedOrOptions::default());
    markdown.base_uri = Some(URI::parse("file://remote-host/share/base.md", false));
    let result = env.render(&guard, &dto(&markdown), &deny_remote_images());

    assert_eq!((select_all(&result.element.borrow(), "img").len(), text_content(&result)), (0, "<img src=\"image.png\" alt=\"image\">".to_owned()));
}

#[test]
fn images_relative_image_with_a_local_file_base_uri_remains_allowed() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let env = TestEnv::new();

    let mut markdown = MarkdownString::new(Some(js("![image](image.png)")), TrustedOrOptions::default());
    markdown.base_uri = Some(file_uri("/images/base.md"));
    let result = env.render(&guard, &dto(&markdown), &deny_remote_images());

    assert_eq!(select_all(&result.element.borrow(), "img").len(), 1);
}

#[test]
fn images_local_file_image_remains_allowed_by_remote_image_validation() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let env = TestEnv::new();

    let local_image = file_uri("/images/cat.gif");
    let result = env.render(&guard, &plain(&format!("![image]({})", local_image.to_string(false).to_string_lossy())), &deny_remote_images());

    assert_eq!(select_all(&result.element.borrow(), "img").len(), 1);
}

#[test]
fn images_image_width_from_title_params() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let env = TestEnv::new();

    let result = env.render_default(&guard, &plain("![image](http://example.com/cat.gif|width=100px 'caption')"));
    assert_node_equals(&result, "<div><p><img width=\"100\" title=\"caption\" alt=\"image\" src=\"http://example.com/cat.gif\"></p></div>");
}

#[test]
fn images_image_height_from_title_params() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let env = TestEnv::new();

    let result = env.render_default(&guard, &plain("![image](http://example.com/cat.gif|height=100 'caption')"));
    assert_node_equals(&result, "<div><p><img height=\"100\" title=\"caption\" alt=\"image\" src=\"http://example.com/cat.gif\"></p></div>");
}

#[test]
fn images_image_width_and_height_from_title_params() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let env = TestEnv::new();

    let result = env.render_default(&guard, &plain("![image](http://example.com/cat.gif|height=200,width=100 'caption')"));
    assert_node_equals(&result, "<div><p><img height=\"200\" width=\"100\" title=\"caption\" alt=\"image\" src=\"http://example.com/cat.gif\"></p></div>");
}

#[test]
fn images_image_with_file_uri_should_render_as_same_origin_uri() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let env = TestEnv::new();

    let result = env.render_default(&guard, &plain("![image](file:///images/cat.gif)"));
    assert_node_equals(&result, "<div><p><img src=\"vscode-file://vscode-app/images/cat.gif\" alt=\"image\"></p></div>");
}

// ----------------------------------------------------------------------------------------------------------------
// Code block renderer
// ----------------------------------------------------------------------------------------------------------------

/// `simpleCodeBlockRenderer(lang, code)`.
fn simple_code_block_renderer(_lang: &JsString, code: &JsString) -> Promise<Element> {
    Promise::resolved(code_element(&code.to_string_lossy()))
}

#[test]
fn code_block_renderer_async_render_callback_should_be_invoked_for_code_blocks() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let env = TestEnv::new();

    let invoked = Rc::new(Cell::new(false));
    let flag = invoked.clone();
    let options = MarkdownRenderOptions {
        async_render_callback: Some(Rc::new(move || flag.set(true))),
        code_block_renderer: Some(Rc::new(simple_code_block_renderer)),
        ..MarkdownRenderOptions::default()
    };
    let result = env.render(&guard, &plain("```js\n1 + 1;\n```"), &options);
    runtime::run_until_idle();

    assert!(invoked.get(), "o asyncRenderCallback não foi chamado");
    // O bloco assíncrono entrou no espaço reservado do `div[data-code]`.
    let root = result.element.borrow();
    assert_eq!(select_all(&root, "code").len(), 1);
}

#[test]
fn code_block_renderer_async_render_callback_should_not_be_invoked_if_result_is_immediately_disposed() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let env = TestEnv::new();

    let invoked = Rc::new(Cell::new(false));
    let flag = invoked.clone();
    let options = MarkdownRenderOptions {
        async_render_callback: Some(Rc::new(move || flag.set(true))),
        code_block_renderer: Some(Rc::new(simple_code_block_renderer)),
        ..MarkdownRenderOptions::default()
    };
    let result = render_markdown(&plain("```js\n1 + 1;\n```"), &options, &env.environment()).expect("renderMarkdown");
    result.dispose();
    runtime::run_until_idle();

    assert!(!invoked.get(), "o asyncRenderCallback foi chamado depois do dispose");
}

#[test]
fn code_block_renderer_async_render_callback_should_not_be_invoked_if_dispose_is_called_before_code_block_is_rendered() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    let invoked = Rc::new(Cell::new(false));
    let flag = invoked.clone();
    run_with_faked_timers(async move {
        let env = TestEnv::new();
        let (pending, resolver) = Promise::<Element>::new();
        let options = MarkdownRenderOptions {
            async_render_callback: Some(Rc::new(move || flag.set(true))),
            code_block_renderer: Some(Rc::new(move |_: &JsString, _: &JsString| pending.clone())),
            ..MarkdownRenderOptions::default()
        };
        let result = render_markdown(&plain("```js\n1 + 1;\n```"), &options, &env.environment()).expect("renderMarkdown");
        timeout(10).await;
        result.dispose();
        resolver.resolve(Element { tag_name: "code".to_owned(), attributes: Vec::new(), children: Vec::new() });
        timeout(10).await;
    });

    assert!(!invoked.get(), "o asyncRenderCallback foi chamado depois do dispose");
}

#[test]
fn code_block_renderer_code_blocks_should_use_leading_language_id_157793() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let env = TestEnv::new();

    let language = Rc::new(RefCell::new(None::<String>));
    let seen = language.clone();
    let options = MarkdownRenderOptions {
        code_block_renderer: Some(Rc::new(move |lang: &JsString, value: &JsString| {
            *seen.borrow_mut() = Some(lang.to_string_lossy());
            simple_code_block_renderer(lang, value)
        })),
        ..MarkdownRenderOptions::default()
    };
    env.render(&guard, &plain("```js some other stuff\n1 + 1;\n```"), &options);
    runtime::run_until_idle();

    assert_eq!(language.borrow().as_deref(), Some("js"));
}

// ----------------------------------------------------------------------------------------------------------------
// ThemeIcons Support On
// ----------------------------------------------------------------------------------------------------------------

fn theme_icons(enabled: bool, html: bool) -> MarkdownString {
    markdown_string(None, MarkdownStringOptions { support_theme_icons: Some(enabled), support_html: html.then_some(true), ..MarkdownStringOptions::default() })
}

#[test]
fn theme_icons_on_render_append_text() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let env = TestEnv::new();

    let mut mds = theme_icons(true, false);
    append_text(&mut mds, "$(zap) $(not a theme icon) $(add)");

    let result = env.render_default(&guard, &dto(&mds));
    assert_eq!(inner(&result), "<p>$(zap)&nbsp;$(not&nbsp;a&nbsp;theme&nbsp;icon)&nbsp;$(add)</p>");
}

#[test]
fn theme_icons_on_render_append_markdown() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let env = TestEnv::new();

    let mut mds = theme_icons(true, false);
    append_markdown(&mut mds, "$(zap) $(not a theme icon) $(add)");

    let result = env.render_default(&guard, &dto(&mds));
    assert_eq!(inner(&result), "<p><span class=\"codicon codicon-zap\"></span> $(not a theme icon) <span class=\"codicon codicon-add\"></span></p>");
}

#[test]
fn theme_icons_on_render_append_markdown_with_escaped_icon() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let env = TestEnv::new();

    let mut mds = theme_icons(true, false);
    append_markdown(&mut mds, "\\$(zap) $(not a theme icon) $(add)");

    let result = env.render_default(&guard, &dto(&mds));
    assert_eq!(inner(&result), "<p>$(zap) $(not a theme icon) <span class=\"codicon codicon-add\"></span></p>");
}

#[test]
fn theme_icons_on_render_icon_in_link() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let env = TestEnv::new();

    let mut mds = theme_icons(true, false);
    append_markdown(&mut mds, "[$(zap)-link](#link)");

    let result = env.render_default(&guard, &dto(&mds));
    assert_eq!(inner(&result), "<p><a href=\"\" title=\"#link\" draggable=\"false\" data-href=\"#link\"><span class=\"codicon codicon-zap\"></span>-link</a></p>");
}

#[test]
fn theme_icons_on_render_icon_in_table() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let env = TestEnv::new();

    let mut mds = theme_icons(true, false);
    append_markdown(&mut mds, "\n| text   | text                 |\n|--------|----------------------|\n| $(zap) | [$(zap)-link](#link) |");

    let result = env.render_default(&guard, &dto(&mds));
    assert_eq!(
        inner(&result),
        "<table>\n<thead>\n<tr>\n<th>text</th>\n<th>text</th>\n</tr>\n</thead>\n<tbody><tr>\n<td><span class=\"codicon codicon-zap\"></span></td>\n<td><a href=\"\" title=\"#link\" draggable=\"false\" data-href=\"#link\"><span class=\"codicon codicon-zap\"></span>-link</a></td>\n</tr>\n</tbody></table>\n"
    );
}

#[test]
fn theme_icons_on_render_icon_in_a_without_href_152170() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let env = TestEnv::new();

    let mut mds = theme_icons(true, true);
    append_markdown(&mut mds, "<a>$(sync)</a>");

    let result = env.render_default(&guard, &dto(&mds));
    assert_eq!(inner(&result), "<p><span class=\"codicon codicon-sync\"></span></p>");
}

// ----------------------------------------------------------------------------------------------------------------
// ThemeIcons Support Off
// ----------------------------------------------------------------------------------------------------------------

#[test]
fn theme_icons_off_render_append_text() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let env = TestEnv::new();

    let mut mds = theme_icons(false, false);
    append_text(&mut mds, "$(zap) $(not a theme icon) $(add)");

    let result = env.render_default(&guard, &dto(&mds));
    assert_eq!(inner(&result), "<p>$(zap)&nbsp;$(not&nbsp;a&nbsp;theme&nbsp;icon)&nbsp;$(add)</p>");
}

#[test]
fn theme_icons_off_render_append_markdown_with_escaped_icon() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let env = TestEnv::new();

    let mut mds = theme_icons(false, false);
    append_markdown(&mut mds, "\\$(zap) $(not a theme icon) $(add)");

    let result = env.render_default(&guard, &dto(&mds));
    assert_eq!(inner(&result), "<p>$(zap) $(not a theme icon) $(add)</p>");
}

// ----------------------------------------------------------------------------------------------------------------
// Alerts
// ----------------------------------------------------------------------------------------------------------------

#[test]
fn alerts_should_render_alert_with_data_severity_attribute_and_icon() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let env = TestEnv::new();

    let markdown = markdown_string(Some("> [!NOTE]\n> This is a note alert"), MarkdownStringOptions { support_alert_syntax: Some(true), ..MarkdownStringOptions::default() });
    let result = env.render_default(&guard, &dto(&markdown));

    let html = inner(&result);
    let root = result.element.borrow();
    assert!(
        select_all(&root, "blockquote").iter().any(|b| attr(b, "data-severity").as_deref() == Some("note")),
        "Should have blockquote with data-severity=\"note\""
    );
    assert!(html.contains("This is a note alert"), "Should contain alert text");
    assert!(html.contains("codicon-info"), "Should contain info icon");
}

#[test]
fn alerts_should_render_regular_blockquote_when_support_alert_syntax_is_disabled() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let env = TestEnv::new();

    let markdown = MarkdownString::new(Some(js("> [!NOTE]\n> This should be a regular blockquote")), TrustedOrOptions::default());
    let result = env.render_default(&guard, &dto(&markdown));

    let html = inner(&result);
    let root = result.element.borrow();
    let blockquotes = select_all(&root, "blockquote");
    assert!(!blockquotes.is_empty(), "Should have blockquote");
    assert_eq!(attr(blockquotes[0], "data-severity"), None, "Should not have data-severity attribute");
    assert!(html.contains("[!NOTE]"), "Should contain literal [!NOTE] text");
}

#[test]
fn alerts_should_not_transform_blockquotes_without_alert_syntax() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let env = TestEnv::new();

    let markdown = markdown_string(Some("> This is a regular blockquote"), MarkdownStringOptions { support_alert_syntax: Some(true), ..MarkdownStringOptions::default() });
    let result = env.render_default(&guard, &dto(&markdown));

    let root = result.element.borrow();
    let blockquotes = select_all(&root, "blockquote");
    assert_eq!(blockquotes.first().and_then(|b| attr(b, "data-severity")), None, "Should not have data-severity attribute");
}

// ----------------------------------------------------------------------------------------------------------------
// Code block reuse
// ----------------------------------------------------------------------------------------------------------------

/// `createCodeBlock(language, text)`: um `pre` com o texto.
fn create_code_block(text: &str) -> Element {
    Element { tag_name: "pre".to_owned(), attributes: Vec::new(), children: vec![Node::Text(js(text))] }
}

/// `normalizedHtml(element)`: o `outerHTML` do clone sem os `data-code`.
fn normalized_html(element: &Element) -> String {
    fn strip(element: &mut Element) {
        element.attributes.retain(|attribute| attribute.name != "data-code");
        for child in &mut element.children {
            if let Node::Element(child) = child {
                strip(child);
            }
        }
    }
    let mut clone = element.clone();
    strip(&mut clone);
    outer_html(&clone).to_string_lossy()
}

const FIRST: &str = "```test\nfirst\n```";
const SECOND: &str = "```test\nsecond\n```";
const QUOTED: &str = "> ```test\n> first\n> ```";
const LIST: &str = "- First\n\n  ```test\n  first\n  ```\n\n- Second\n\n  ```test\n  second\n  ```";

/// `options` do caso: o `codeBlockRendererSync` que reaproveita o elemento pelo texto.
fn reusing_options(code_blocks: Rc<RefCell<Vec<(String, Element)>>>) -> MarkdownRenderOptions {
    MarkdownRenderOptions {
        code_block_renderer_sync: Some(Rc::new(move |_language: &JsString, text: &JsString, _raw: &JsString| {
            let text = text.to_string_lossy();
            let mut blocks = code_blocks.borrow_mut();
            if let Some((_, element)) = blocks.iter().find(|(key, _)| *key == text) {
                return element.clone();
            }
            let element = create_code_block(&text);
            blocks.push((text, element.clone()));
            element
        })),
        ..MarkdownRenderOptions::default()
    }
}

fn matches_a_fresh_render(before: &str, after: &str) {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let env = TestEnv::new();

    let options = reusing_options(Rc::new(RefCell::new(Vec::new())));
    let initial = env.render(&guard, &plain(before), &options);
    initial.dispose();
    let updated = env.render_into(&guard, &plain(after), &options, &initial.element);
    let fresh_options = MarkdownRenderOptions {
        code_block_renderer_sync: Some(Rc::new(|_language: &JsString, text: &JsString, _raw: &JsString| create_code_block(&text.to_string_lossy()))),
        ..MarkdownRenderOptions::default()
    };
    let fresh = env.render(&guard, &plain(after), &fresh_options);
    assert_eq!(normalized_html(&updated.element.borrow()), normalized_html(&fresh.element.borrow()));
}

#[test]
fn code_block_reuse_matches_a_fresh_render_after_appended_content_in_a_shared_ancestor() {
    matches_a_fresh_render(LIST, &format!("Intro\n\n{LIST}\n\n  More text\n\n- Last"));
}

#[test]
fn code_block_reuse_matches_a_fresh_render_after_changed_ancestor_attributes() {
    matches_a_fresh_render("3. Diagram\n\n   ```test\n   first\n   ```", "1. Diagram\n\n   ```test\n   first\n   ```");
}

#[test]
fn code_block_reuse_matches_a_fresh_render_after_changed_nesting() {
    matches_a_fresh_render(QUOTED, FIRST);
}

#[test]
fn code_block_reuse_matches_a_fresh_render_after_split_ancestors() {
    matches_a_fresh_render(LIST, "- First\n\n  ```test\n  first\n  ```\n\nOutside\n\n- Second\n\n  ```test\n  second\n  ```");
}

#[test]
fn code_block_reuse_matches_a_fresh_render_after_reordered_code_blocks() {
    matches_a_fresh_render(&format!("{FIRST}\n\n{SECOND}"), &format!("{SECOND}\n\n{FIRST}"));
}

#[test]
fn code_block_reuse_matches_a_fresh_render_after_removed_code_blocks() {
    matches_a_fresh_render(&format!("{FIRST}\n\n{SECOND}"), SECOND);
}

#[test]
fn code_block_reuse_matches_a_fresh_render_after_changed_code_blocks() {
    matches_a_fresh_render(&format!("{FIRST}\n\n{SECOND}"), &format!("{FIRST}\n\n```test\nchanged\n```"));
}

#[test]
fn code_block_reuse_keeps_reused_code_blocks_and_their_ancestors_mounted() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let env = TestEnv::new();

    let code_blocks = Rc::new(RefCell::new(Vec::<(String, Element)>::new()));
    let collecting = reusing_options(code_blocks.clone());
    let initial = env.render(&guard, &plain(LIST), &collecting);
    let blocks_before: Vec<String> = code_blocks.borrow().iter().map(|(text, _)| text.clone()).collect();
    initial.dispose();

    // Na segunda renderização cada bloco precisa existir (o `assert.ok(block)` do original) e vem da primeira.
    let found = Rc::new(RefCell::new(Vec::<String>::new()));
    let known = blocks_before.clone();
    let seen = found.clone();
    let reusing = MarkdownRenderOptions {
        code_block_renderer_sync: Some(Rc::new(move |_language: &JsString, text: &JsString, _raw: &JsString| {
            let text = text.to_string_lossy();
            assert!(known.contains(&text), "o bloco {text:?} deveria existir");
            seen.borrow_mut().push(text.clone());
            create_code_block(&text)
        })),
        ..MarkdownRenderOptions::default()
    };
    let list_before = outer_html(select_all(&initial.element.borrow(), "ul")[0]).to_string_lossy();
    let updated = env.render_into(&guard, &plain(&format!("Intro\n\n{LIST}\n\n  More text")), &reusing, &initial.element);

    let root = updated.element.borrow();
    assert_eq!(
        (
            blocks_before.clone(),
            found.borrow().clone(),
            select_all(&root, "ul").len(),
            select_all(&root, "pre").len(),
            list_before.contains("<pre>first</pre>") && list_before.contains("<pre>second</pre>"),
        ),
        (vec!["first".to_owned(), "second".to_owned()], vec!["first".to_owned(), "second".to_owned()], 1, 2, true)
    );
}

// ----------------------------------------------------------------------------------------------------------------
// Links e comandos
// ----------------------------------------------------------------------------------------------------------------

#[test]
fn npm_hover_run_script_not_working_90855() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let env = TestEnv::new();

    let command = "command:npm.runScriptFromHover?%7B%22documentUri%22%3A%7B%22%24mid%22%3A1%2C%22fsPath%22%3A%22c%3A%5C%5CUsers%5C%5Cjrieken%5C%5CCode%5C%5C_sample%5C%5Cfoo%5C%5Cpackage.json%22%2C%22_sep%22%3A1%2C%22external%22%3A%22file%3A%2F%2F%2Fc%253A%2FUsers%2Fjrieken%2FCode%2F_sample%2Ffoo%2Fpackage.json%22%2C%22path%22%3A%22%2Fc%3A%2FUsers%2Fjrieken%2FCode%2F_sample%2Ffoo%2Fpackage.json%22%2C%22scheme%22%3A%22file%22%7D%2C%22script%22%3A%22echo%22%7D";
    let mut uris = OrderedMap::new();
    uris.insert(
        js("__uri_e49443"),
        URI::from(&UriComponents { scheme: Some(js("file")), path: Some(js("/c:/Users/jrieken/Code/_sample/foo/package.json")), ..UriComponents::default() }, false),
    );
    uris.insert(
        js(command),
        URI::from(
            &UriComponents {
                scheme: Some(js("command")),
                path: Some(js("npm.runScriptFromHover")),
                query: Some(js("{\"documentUri\":\"__uri_e49443\",\"script\":\"echo\"}")),
                ..UriComponents::default()
            },
            false,
        ),
    );
    let md = IMarkdownString {
        value: js(&format!("[Run Script]({command} \"Run the script as a task\")")),
        support_theme_icons: Some(false),
        is_trusted: trusted(),
        uris: Some(uris),
        ..IMarkdownString::default()
    };
    let result = env.render_default(&guard, &md);

    let root = result.element.borrow();
    let anchors = select_all(&root, "a");
    assert!(!anchors.is_empty());
    let data_href = attr(anchors[0], "data-href").expect("data-href");
    assert!(!data_href.is_empty());

    let uri = URI::parse(js(&data_href), false);
    let decoded = je_base::uri::decode_uri_component(uri.query().units()).expect("decodeURIComponent");
    let Marshalled::Object(entries) = marshalling::parse(&decoded, Platform::Posix) else { panic!("o argumento do comando não é um objeto") };
    let find = |name: &str| entries.iter().find(|(key, _)| key.to_string_lossy() == name).map(|(_, value)| value);
    let Some(Marshalled::String(script)) = find("script") else { panic!("sem script") };
    assert_eq!(script.to_string_lossy(), "echo");
    let Some(Marshalled::Uri(document_uri)) = find("documentUri") else { panic!("o documentUri não foi revivido") };
    assert!(document_uri.to_string(false).to_string_lossy().starts_with("file:///c%3A/"));
}

#[test]
fn should_not_render_command_links_by_default() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let env = TestEnv::new();

    let md = markdown_string(
        Some("[command1](command:doFoo) <a href=\"command:doFoo\">command2</a>"),
        MarkdownStringOptions { support_html: Some(true), ..MarkdownStringOptions::default() },
    );
    let result = env.render_default(&guard, &dto(&md));
    assert_eq!(inner(&result), "<p>command1 command2</p>");
}

#[test]
fn should_render_command_links_in_trusted_strings() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let env = TestEnv::new();

    let md = markdown_string(
        Some("[command1](command:doFoo) <a href=\"command:doFoo\">command2</a>"),
        MarkdownStringOptions { is_trusted: trusted(), support_html: Some(true), ..MarkdownStringOptions::default() },
    );
    let result = env.render_default(&guard, &dto(&md));
    assert_eq!(
        inner(&result),
        "<p><a href=\"\" title=\"\" draggable=\"false\" data-href=\"command:doFoo\">command1</a> <a href=\"\" data-href=\"command:doFoo\">command2</a></p>"
    );
}

#[test]
fn should_remove_relative_links_if_there_is_no_base_url() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let env = TestEnv::new();

    let md = markdown_string(
        Some("[text](./foo) <a href=\"./bar\">bar</a>"),
        MarkdownStringOptions { is_trusted: trusted(), support_html: Some(true), ..MarkdownStringOptions::default() },
    );
    let result = env.render_default(&guard, &dto(&md));
    assert_eq!(inner(&result), "<p>text bar</p>");
}

#[test]
fn should_support_relative_links_if_baseurl_is_set() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let env = TestEnv::new();

    let mut md = markdown_string(
        Some("[text](./foo) <a href=\"./bar\">bar</a> <img src=\"cat.gif\">"),
        MarkdownStringOptions { is_trusted: trusted(), support_html: Some(true), ..MarkdownStringOptions::default() },
    );
    md.base_uri = Some(URI::parse("https://example.com/path/", false));
    let result = env.render_default(&guard, &dto(&md));
    assert_eq!(
        inner(&result),
        "<p><a href=\"\" title=\"./foo\" draggable=\"false\" data-href=\"https://example.com/path/foo\">text</a> <a href=\"\" data-href=\"https://example.com/path/bar\">bar</a> <img src=\"https://example.com/path/cat.gif\"></p>"
    );
}

// ----------------------------------------------------------------------------------------------------------------
// Copy-safe hrefs
// ----------------------------------------------------------------------------------------------------------------

#[test]
fn copy_safe_hrefs_keeps_the_real_href_only_for_targets_that_resolve_elsewhere() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let env = TestEnv::new();

    let md = markdown_string(
        Some("[web](https://example.com/page) [mail](mailto:user@example.com) [run](command:doFoo) [file](file:///home/user/a.ts) [ref](http://_vscodecontentref_/0)"),
        MarkdownStringOptions { is_trusted: trusted(), ..MarkdownStringOptions::default() },
    );
    let options = MarkdownRenderOptions { action_handler: Some(Rc::new(|_: &JsString, _: &IMarkdownString| {})), ..MarkdownRenderOptions::default() };
    let result = env.render(&guard, &dto(&md), &options);

    let root = result.element.borrow();
    let actual: Vec<[Option<String>; 3]> = select_all(&root, "a").iter().map(|a| [attr(a, "href"), attr(a, "data-href"), attr(a, "draggable")]).collect();
    let row = |href: &str, data_href: &str| [Some(href.to_owned()), Some(data_href.to_owned()), Some("false".to_owned())];
    assert_eq!(
        actual,
        vec![
            row("https://example.com/page", "https://example.com/page"),
            row("mailto:user@example.com", "mailto:user@example.com"),
            row("", "command:doFoo"),
            row("", "file:///home/user/a.ts"),
            row("", "http://_vscodecontentref_/0"),
        ]
    );
}

#[test]
fn copy_safe_hrefs_leaves_the_href_empty_when_nothing_intercepts_clicks() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let env = TestEnv::new();

    // Sem um tratador de ação a âncora navegaria sozinha, contornando o abridor.
    let md = markdown_string(Some("[web](https://example.com/page)"), MarkdownStringOptions::default());
    let result = env.render_default(&guard, &dto(&md));

    let root = result.element.borrow();
    let anchor = select_all(&root, "a")[0];
    assert_eq!([attr(anchor, "href"), attr(anchor, "data-href")], [Some(String::new()), Some("https://example.com/page".to_owned())]);
}

#[test]
fn copy_safe_hrefs_keeps_the_resolved_href_for_relative_links_against_an_https_base_uri() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let env = TestEnv::new();

    let mut md = markdown_string(Some("[text](./foo)"), MarkdownStringOptions { is_trusted: trusted(), ..MarkdownStringOptions::default() });
    md.base_uri = Some(URI::parse("https://example.com/path/", false));
    let options = MarkdownRenderOptions { action_handler: Some(Rc::new(|_: &JsString, _: &IMarkdownString| {})), ..MarkdownRenderOptions::default() };
    let result = env.render(&guard, &dto(&md), &options);

    let root = result.element.borrow();
    let anchor = select_all(&root, "a")[0];
    assert_eq!(
        [attr(anchor, "href"), attr(anchor, "data-href")],
        [Some("https://example.com/path/foo".to_owned()), Some("https://example.com/path/foo".to_owned())]
    );
}

// ----------------------------------------------------------------------------------------------------------------
// Títulos de links file://
// ----------------------------------------------------------------------------------------------------------------

#[test]
fn should_use_decoded_file_path_as_title_for_file_links() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let env = TestEnv::new();

    let uri = file_uri("/home/user/project/lib.d.ts");
    let md = markdown_string(Some(&format!("[log]({})", uri.to_string(false).to_string_lossy())), MarkdownStringOptions::default());
    let result = env.render_default(&guard, &dto(&md));

    let root = result.element.borrow();
    let anchors = select_all(&root, "a");
    assert!(!anchors.is_empty());
    assert_eq!(attr(anchors[0], "title"), Some(uri.fs_path(Platform::Posix).to_string_lossy()));
}

#[test]
fn should_include_fragment_in_title_for_file_links_with_line_numbers() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let env = TestEnv::new();

    let uri = file_uri("/home/user/project/lib.d.ts");
    let md = markdown_string(Some(&format!("[log]({}#L42)", uri.to_string(false).to_string_lossy())), MarkdownStringOptions::default());
    let result = env.render_default(&guard, &dto(&md));

    let root = result.element.borrow();
    let anchors = select_all(&root, "a");
    assert!(!anchors.is_empty());
    assert_eq!(attr(anchors[0], "title"), Some(format!("{}#L42", uri.fs_path(Platform::Posix).to_string_lossy())));
}

#[test]
fn should_not_override_explicit_title_for_file_links() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let env = TestEnv::new();

    let uri = file_uri("/home/user/project/lib.d.ts");
    let md = markdown_string(Some(&format!("[log]({} \"Go to definition\")", uri.to_string(false).to_string_lossy())), MarkdownStringOptions::default());
    let result = env.render_default(&guard, &dto(&md));

    let root = result.element.borrow();
    let anchors = select_all(&root, "a");
    assert!(!anchors.is_empty());
    assert_eq!(attr(anchors[0], "title").as_deref(), Some("Go to definition"));
}

// ----------------------------------------------------------------------------------------------------------------
// PlaintextMarkdownRender
// ----------------------------------------------------------------------------------------------------------------

fn plaintext(value: &str) -> String {
    render_as_plaintext_with(value, PlaintextOptions::default())
}

#[test]
fn plaintext_code_blockquote_heading_list_listitem_paragraph_table_tablerow_tablecell_strong_em_br_del_text_are_rendered_plaintext() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    let markdown = "`code`\n>quote\n# heading\n- list\n\ntable | table2\n--- | --- \none | two\n\n\nbo**ld**\n_italic_\n~~del~~\nsome text";
    let expected = "code\nquote\nheading\nlist\n\ntable table2\none two\nbold\nitalic\ndel\nsome text";
    assert_eq!(plaintext(markdown), expected);
}

#[test]
fn plaintext_html_hr_image_link_are_rendered_plaintext() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    let markdown = "<div>html</div>\n\n---\n![image](imageLink)\n[text](textLink)";
    assert_eq!(plaintext(markdown), "text");
}

#[test]
fn plaintext_should_not_remove_html_inside_of_code_blocks() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    let markdown = ["```html", "<form>html</form>", "```"].join("\n");
    let expected = ["```", "<form>html</form>", "```"].join("\n");
    assert_eq!(render_as_plaintext_with(&markdown, PlaintextOptions { include_code_blocks_fences: true, ..PlaintextOptions::default() }), expected);
}

#[test]
fn plaintext_does_not_double_escape_entities_inside_code_spans() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    assert_eq!(plaintext("Run `tests & build`"), "Run tests & build");
    assert_eq!(plaintext("Use `<form>` tag"), "Use <form> tag");
}

#[test]
fn plaintext_reduces_inline_syntax_inside_list_items_when_omit_markdown_syntax_is_set() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    // O conteúdo de um item chega como um token de texto com tokens de linha. Por padrão o item sai como fonte
    // crua, então o link mantém o destino; a opção reduz ao texto que o leitor vê.
    let markdown = "- Added [src/](/some/path/to/src)\n- Uses **bold** and `code`";

    assert_eq!(plaintext(markdown), "Added [src/](/some/path/to/src)\n\nUses **bold** and `code`", "default output is unchanged");
    assert_eq!(
        render_as_plaintext_with(markdown, PlaintextOptions { omit_markdown_syntax: true, ..PlaintextOptions::default() }),
        "Added src/\n\nUses bold and code"
    );
}

#[test]
fn plaintext_separates_a_nested_list_from_the_item_holding_it_when_omit_markdown_syntax_is_set() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    assert_eq!(
        render_as_plaintext_with("- outer\n    - inner [link](/target)", PlaintextOptions { omit_markdown_syntax: true, ..PlaintextOptions::default() }),
        "outer\ninner link"
    );
}

// ----------------------------------------------------------------------------------------------------------------
// supportHtml
// ----------------------------------------------------------------------------------------------------------------

fn html_markdown(support_html: bool) -> MarkdownString {
    markdown_string(None, MarkdownStringOptions { support_html: support_html.then_some(true), ..MarkdownStringOptions::default() })
}

#[test]
fn support_html_is_disabled_by_default() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let env = TestEnv::new();

    let mut mds = html_markdown(false);
    append_markdown(&mut mds, "a<b>b</b>c");

    let result = env.render_default(&guard, &dto(&mds));
    assert_eq!(inner(&result), "<p>abc</p>");
}

#[test]
fn support_html_renders_html_when_support_html_is_true() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let env = TestEnv::new();

    let mut mds = html_markdown(true);
    append_markdown(&mut mds, "a<b>b</b>c");

    let result = env.render_default(&guard, &dto(&mds));
    assert_eq!(inner(&result), "<p>a<b>b</b>c</p>");
}

#[test]
fn support_html_should_not_include_scripts_even_when_support_html_is_true() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let env = TestEnv::new();

    let mut mds = html_markdown(true);
    append_markdown(&mut mds, "a<b onclick=\"alert(1)\">b</b><script>alert(2)</script>c");

    let result = env.render_default(&guard, &dto(&mds));
    assert_eq!(inner(&result), "<p>a<b>b</b>c</p>");
}

#[test]
fn support_html_should_not_render_html_appended_as_text() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let env = TestEnv::new();

    let mut mds = html_markdown(true);
    append_text(&mut mds, "a<b>b</b>c");

    let result = env.render_default(&guard, &dto(&mds));
    assert_eq!(inner(&result), "<p>a&lt;b&gt;b&lt;/b&gt;c</p>");
}

#[test]
fn support_html_should_render_html_images() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let env = TestEnv::new();

    let mut mds = html_markdown(true);
    append_markdown(&mut mds, "<img src=\"http://example.com/cat.gif\">");

    let result = env.render_default(&guard, &dto(&mds));
    assert_eq!(inner(&result), "<img src=\"http://example.com/cat.gif\">");
}

#[test]
fn support_html_should_render_html_images_with_file_uri_as_same_origin_uri() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let env = TestEnv::new();

    let mut mds = html_markdown(true);
    append_markdown(&mut mds, "<img src=\"file:///images/cat.gif\">");

    let result = env.render_default(&guard, &dto(&mds));
    assert_eq!(inner(&result), "<img src=\"vscode-file://vscode-app/images/cat.gif\">");
}

#[test]
fn support_html_should_only_allow_checkbox_inputs() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let env = TestEnv::new();

    let mds = markdown_string(Some("text: <input type=\"text\">\ncheckbox:<input type=\"checkbox\">"), MarkdownStringOptions { support_html: Some(true), ..MarkdownStringOptions::default() });

    let result = env.render_default(&guard, &dto(&mds));

    // Os inputs sempre ficam desabilitados também.
    assert_eq!(inner(&result), "<p>text: \ncheckbox:<input type=\"checkbox\" disabled=\"\"></p>");
}

// ----------------------------------------------------------------------------------------------------------------
// fillInIncompleteTokens
// ----------------------------------------------------------------------------------------------------------------

/// `marked.marked.lexer(text)`.
fn lex(text: &str) -> TokensList {
    lexer(&units(text)).expect("lexer")
}

/// `ignoreRaw(...tokenLists)`: zera o `raw` dos tokens de topo.
fn ignore_raw(lists: &mut [&mut TokensList]) {
    for list in lists.iter_mut() {
        for token in &mut list.tokens {
            *token.raw_mut() = JsString::new();
        }
    }
}

/// `deepStrictEqual(fillInIncompleteTokens(lexer(incomplete)), lexer(complete))`.
#[track_caller]
fn assert_completes(incomplete: &str, complete: &str) {
    assert_eq!(fill_in_incomplete_tokens(lex(incomplete)), lex(complete));
}

/// O mesmo, com `ignoreRaw` nos dois lados.
#[track_caller]
fn assert_completes_ignoring_raw(incomplete: &str, complete: &str) {
    let mut new_tokens = fill_in_incomplete_tokens(lex(incomplete));
    let mut complete_tokens = lex(complete);
    ignore_raw(&mut [&mut new_tokens, &mut complete_tokens]);
    assert_eq!(new_tokens, complete_tokens);
}

/// `deepStrictEqual(fillInIncompleteTokens(tokens), tokens)`.
#[track_caller]
fn assert_unchanged(text: &str) {
    let tokens = lex(text);
    assert_eq!(fill_in_incomplete_tokens(tokens.clone()), tokens);
}

const COMPLETE_TABLE: &str = "| a | b |\n| --- | --- |";

mod table {
    use super::*;

    #[test]
    fn complete_table() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        assert_unchanged(COMPLETE_TABLE);
    }

    #[test]
    fn full_header_only() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        assert_completes("| a | b |", COMPLETE_TABLE);
    }

    #[test]
    fn full_header_only_with_trailing_space() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        assert_completes_ignoring_raw("| a | b | ", COMPLETE_TABLE);
    }

    #[test]
    fn incomplete_header() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        assert_completes_ignoring_raw("| a | b", COMPLETE_TABLE);
    }

    #[test]
    fn incomplete_header_one_column() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let incomplete = "| a ";
        assert_completes_ignoring_raw(incomplete, &format!("{incomplete}|\n| --- |"));
    }

    #[test]
    fn full_header_with_extras() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let incomplete = "| a **bold** | b _italics_ |";
        assert_completes(incomplete, &format!("{incomplete}\n| --- | --- |"));
    }

    #[test]
    fn full_header_with_leading_text() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        // Ler isto dá um token e um subtoken `text`.
        let incomplete = "here is a table\n| a | b |";
        assert_completes(incomplete, &format!("{incomplete}\n| --- | --- |"));
    }

    #[test]
    fn full_header_with_leading_other_stuff() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let incomplete = "```js\nconst xyz = 123;\n```\n| a | b |";
        assert_completes(incomplete, &format!("{incomplete}\n| --- | --- |"));
    }

    #[test]
    fn full_header_with_incomplete_separator() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        assert_completes("| a | b |\n| ---", COMPLETE_TABLE);
    }

    #[test]
    fn full_header_with_incomplete_separator_2() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        assert_completes("| a | b |\n| --- |", COMPLETE_TABLE);
    }

    #[test]
    fn full_header_with_incomplete_separator_3() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        assert_completes("| a | b |\n|", COMPLETE_TABLE);
    }

    #[test]
    fn not_a_table() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        assert_unchanged("| a | b |\nsome text");
    }

    #[test]
    fn not_a_table_2() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        assert_unchanged("| a | b |\n| --- |\nsome text");
    }
}

/// `simpleMarkdownTestSuite(name, delimiter)`.
macro_rules! simple_markdown_test_suite {
    ($delimiter:expr) => {
        #[test]
        fn incomplete() {
            let _guard = ensure_no_disposables_are_leaked_in_test_suite();
            let delimiter = $delimiter;
            let incomplete = format!("{delimiter}code");
            assert_completes(&incomplete, &format!("{incomplete}{delimiter}"));
        }

        #[test]
        fn complete() {
            let _guard = ensure_no_disposables_are_leaked_in_test_suite();
            let delimiter = $delimiter;
            assert_unchanged(&format!("leading text {delimiter}code{delimiter} trailing text"));
        }

        #[test]
        fn with_leading_text() {
            let _guard = ensure_no_disposables_are_leaked_in_test_suite();
            let delimiter = $delimiter;
            let incomplete = format!("some text and {delimiter}some code");
            assert_completes(&incomplete, &format!("{incomplete}{delimiter}"));
        }

        #[test]
        fn with_trailing_space() {
            let _guard = ensure_no_disposables_are_leaked_in_test_suite();
            let delimiter = $delimiter;
            let incomplete = format!("some text and {delimiter}some code ");
            assert_completes(&incomplete, &format!("{}{delimiter}", incomplete.trim_end()));
        }

        #[test]
        fn single_loose_delimiter() {
            let _guard = ensure_no_disposables_are_leaked_in_test_suite();
            let delimiter = $delimiter;
            assert_unchanged(&format!("some text and {delimiter}by itself\nmore text here"));
        }

        #[test]
        fn incomplete_after_newline() {
            let _guard = ensure_no_disposables_are_leaked_in_test_suite();
            let delimiter = $delimiter;
            let text = format!("some text\nmore text here and {delimiter}text");
            assert_completes(&text, &format!("{text}{delimiter}"));
        }

        #[test]
        fn incomplete_after_complete() {
            let _guard = ensure_no_disposables_are_leaked_in_test_suite();
            let delimiter = $delimiter;
            let text = format!("leading text {delimiter}code{delimiter} trailing text and {delimiter}another");
            assert_completes(&text, &format!("{text}{delimiter}"));
        }

        #[test]
        fn incomplete_in_list() {
            let _guard = ensure_no_disposables_are_leaked_in_test_suite();
            let delimiter = $delimiter;
            let text = format!("- list item one\n- list item two and {delimiter}text");
            assert_completes(&text, &format!("{text}{delimiter}"));
        }

        #[test]
        fn incomplete_in_asterisk_list() {
            let _guard = ensure_no_disposables_are_leaked_in_test_suite();
            let delimiter = $delimiter;
            let text = format!("* list item one\n* list item two and {delimiter}text");
            assert_completes(&text, &format!("{text}{delimiter}"));
        }

        #[test]
        fn incomplete_in_numbered_list() {
            let _guard = ensure_no_disposables_are_leaked_in_test_suite();
            let delimiter = $delimiter;
            let text = format!("1. list item one\n2. list item two and {delimiter}text");
            assert_completes(&text, &format!("{text}{delimiter}"));
        }
    };
}

mod list {
    use super::*;

    #[test]
    fn list_with_complete_codeblock() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        assert_unchanged("-\n\t```js\n\tlet x = 1;\n\t```\n- list item two\n");
    }

    #[test]
    #[ignore = "test.skip no original"]
    fn list_with_incomplete_codeblock() {
        let incomplete = "- list item one\n\n\t```js\n\tlet x = 1;";
        assert_completes(incomplete, &format!("{incomplete}\n\t```"));
    }

    #[test]
    fn list_with_subitems() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        assert_unchanged("- hello\n\t- sub item\n- text\n\tnewline for some reason\n");
    }

    #[test]
    fn ordered_list_with_subitems() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        assert_unchanged("1. hello\n\t- sub item\n2. text\n\tnewline for some reason\n");
    }

    #[test]
    fn list_with_stuff() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        assert_unchanged("- list item one `codespan` **bold** [link](http://microsoft.com) more text");
    }

    #[test]
    fn list_with_incomplete_link_text() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let incomplete = "- list item one\n- item two [link";
        assert_completes(incomplete, &format!("{incomplete}](https://microsoft.com)"));
    }

    #[test]
    fn list_with_incomplete_link_target() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let incomplete = "- list item one\n- item two [link](";
        assert_completes(incomplete, &format!("{incomplete})"));
    }

    #[test]
    fn ordered_list_with_incomplete_link_target() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let incomplete = "1. list item one\n2. item two [link](";
        assert_completes(incomplete, &format!("{incomplete})"));
    }

    #[test]
    fn ordered_list_with_extra_whitespace() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let incomplete = "1. list item one\n2. item two [link](";
        assert_completes(incomplete, &format!("{incomplete})"));
    }

    #[test]
    fn list_with_extra_whitespace() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let incomplete = "- list item one\n- item two [link](";
        assert_completes(incomplete, &format!("{incomplete})"));
    }

    #[test]
    fn list_with_incomplete_link_with_other_stuff() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let incomplete = "- list item one\n- item two [`link";
        assert_completes(incomplete, &format!("{incomplete}`](https://microsoft.com)"));
    }

    #[test]
    fn ordered_list_with_incomplete_link_with_other_stuff() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let incomplete = "1. list item one\n1. item two [`link";
        assert_completes(incomplete, &format!("{incomplete}`](https://microsoft.com)"));
    }

    #[test]
    fn list_with_bold_incomplete_link_target() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let incomplete = "- list item one\n- **[link](http://microsoft";
        assert_completes(incomplete, &format!("{incomplete})**"));
    }

    #[test]
    fn ordered_list_with_bold_incomplete_link_target() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let incomplete = "1. list item one\n2. **[link](http://microsoft";
        assert_completes(incomplete, &format!("{incomplete})**"));
    }

    #[test]
    fn list_with_incomplete_subitem() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let incomplete = "1. list item one\n\t- ";
        assert_completes(incomplete, &format!("{incomplete}&nbsp;"));
    }

    #[test]
    fn list_with_incomplete_nested_subitem() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let incomplete = "1. list item one\n\t- item 2\n\t\t- ";
        assert_completes(incomplete, &format!("{incomplete}&nbsp;"));
    }

    #[test]
    fn text_with_start_of_list_is_not_a_heading() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let incomplete = "hello\n- ";
        assert_completes(incomplete, &format!("{incomplete} &nbsp;"));
    }

    #[test]
    fn even_more_text_with_start_of_list_is_not_a_heading() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let incomplete = "# hello\n\ntext\n-";
        assert_completes(incomplete, &format!("{incomplete} &nbsp;"));
    }
}

mod blockquote {
    use super::*;

    #[test]
    fn incomplete_double_star() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let incomplete = "> **text";
        assert_completes(incomplete, &format!("{incomplete}**"));
    }

    #[test]
    fn incomplete_double_star_before_trailing_quote_only_lines() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        assert_completes("> **text\n>\n>", "> **text**\n>\n>");
    }

    #[test]
    fn preserves_reference_links_when_completing_inline_tokens() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let incomplete = "[id]: https://example.com\n\n> [label][id] **text";
        assert_completes(incomplete, &format!("{incomplete}**"));
    }
}

mod codespan {
    use super::*;

    simple_markdown_test_suite!("`");

    #[test]
    fn backtick_between_letters() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let text = "a`b";
        assert_completes(text, &format!("{text}`"));
    }

    #[test]
    fn nested_pattern() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let text = "sldkfjsd `abc __def__ ghi";
        assert_completes(text, &format!("{text}`"));
    }

    #[test]
    fn codespan_inside_body_wrapped_markdown() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        // O renderizador de conteúdo do chat envolve o Markdown com `supportHtml` em `<body>...</body>` para o
        // DOMPurify manter comentários iniciais. Isso faz de `</body>` o último token literal: o parágrafo com o
        // backtick solto deixa de ser o último. O ajuste ainda precisa fechar o codespan e preservar o HTML final.
        let text = "<body>\n\nCreated isolated worktree for branch `xyz\n\n</body>";
        assert_completes(text, "<body>\n\nCreated isolated worktree for branch `xyz`\n\n</body>");
    }
}

mod star {
    use super::*;

    simple_markdown_test_suite!("*");

    #[test]
    fn star_between_letters() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let text = "sldkfjsd a*b";
        assert_completes(text, &format!("{text}*"));
    }

    #[test]
    fn nested_pattern() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let text = "sldkfjsd *abc __def__ ghi";
        assert_completes(text, &format!("{text}*"));
    }
}

mod double_star {
    use super::*;

    simple_markdown_test_suite!("**");

    #[test]
    fn double_star_between_letters() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let text = "a**b";
        assert_completes(text, &format!("{text}**"));
    }

    // No original há um TODO: aparar esses padrões do fim.
    #[test]
    #[ignore = "test.skip no original"]
    fn ending_in_doublestar() {
        let incomplete = "some text and **";
        assert_completes(incomplete, &format!("{}**", incomplete.trim_end()));
    }
}

mod underscore {
    use super::*;

    simple_markdown_test_suite!("_");

    #[test]
    fn underscore_between_letters() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        assert_unchanged("this_not_italics");
    }
}

mod double_underscore {
    use super::*;

    simple_markdown_test_suite!("__");

    #[test]
    fn double_underscore_between_letters() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        assert_unchanged("this__not__bold");
    }
}

mod link {
    use super::*;

    #[test]
    fn incomplete_link_text() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let incomplete = "abc [text";
        assert_completes(incomplete, &format!("{incomplete}](https://microsoft.com)"));
    }

    #[test]
    fn incomplete_link_target() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let incomplete = "foo [text](http://microsoft";
        assert_completes(incomplete, &format!("{incomplete})"));
    }

    #[test]
    fn incomplete_link_target_2() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let incomplete = "foo [text](http://microsoft.com";
        assert_completes(incomplete, &format!("{incomplete})"));
    }

    #[test]
    fn incomplete_link_target_inside_parentheses() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let incomplete = "([text](http://microsoft.com";
        assert_completes(incomplete, &format!("{incomplete})"));
    }

    #[test]
    fn incomplete_link_target_with_extra_stuff() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let incomplete = "[before `text` after](http://microsoft.com";
        assert_completes(incomplete, &format!("{incomplete})"));
    }

    #[test]
    fn incomplete_link_target_with_extra_stuff_and_incomplete_arg() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let incomplete = "[before `text` after](http://microsoft.com \"more text ";
        assert_completes(incomplete, &format!("{incomplete}\")"));
    }

    #[test]
    fn incomplete_link_target_with_incomplete_arg() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let incomplete = "foo [text](http://microsoft.com \"more text here ";
        assert_completes(incomplete, &format!("{incomplete}\")"));
    }

    #[test]
    fn incomplete_link_target_with_incomplete_arg_2() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let incomplete = "[text](command:vscode.openRelativePath \"arg";
        assert_completes(incomplete, &format!("{incomplete}\")"));
    }

    #[test]
    fn incomplete_link_target_with_complete_arg() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let incomplete = "foo [text](http://microsoft.com \"more text here\"";
        assert_completes(incomplete, &format!("{incomplete})"));
    }

    #[test]
    fn link_text_with_incomplete_codespan() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let incomplete = "text [`codespan";
        assert_completes(incomplete, &format!("{incomplete}`](https://microsoft.com)"));
    }

    #[test]
    fn link_text_with_incomplete_stuff() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let incomplete = "text [more text `codespan` text **bold";
        assert_completes(incomplete, &format!("{incomplete}**](https://microsoft.com)"));
    }

    #[test]
    fn looks_like_incomplete_link_target_but_isnt() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let complete = "**bold** `codespan` text](";
        assert_completes(complete, complete);
    }

    #[test]
    fn incomplete_link_in_list() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let incomplete = "- [text";
        assert_completes(incomplete, &format!("{incomplete}](https://microsoft.com)"));
    }

    #[test]
    fn incomplete_link_target_inside_bold() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let incomplete = "**[text](http://microsoft";
        assert_completes(incomplete, &format!("{incomplete})**"));
    }

    #[test]
    fn incomplete_link_target_with_arg_inside_bold() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let incomplete = "**[text](http://microsoft.com \"more text ";
        assert_completes(incomplete, &format!("{incomplete}\")**"));
    }

    #[test]
    fn square_brace_between_letters() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        assert_unchanged("a[b");
    }

    #[test]
    fn square_brace_on_previous_line() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        assert_unchanged("text[\nmore text");
    }

    #[test]
    fn square_braces_in_text() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        assert_unchanged("hello [what] is going on");
    }

    #[test]
    fn complete_link() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        assert_unchanged("text [link](http://microsoft.com)");
    }
}
