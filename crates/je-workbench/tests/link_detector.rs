//! Porte de `src/vs/workbench/contrib/debug/test/browser/linkDetector.test.ts` (suítes `Debug - Link Detector` e
//! `Debug - Link Detector - fileLinkToUri`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - **DOM.** O `linkify` do original devolve um `HTMLElement`; no porte a UI é nativa e ele devolve o [`Element`] de
//!   `link_detector.rs` (árvore de `span`, `a` e texto), que responde ao que o teste lê do DOM: `tagName`, `children`,
//!   `firstElementChild`, `textContent` e `outerHTML`, com a mesma serialização. `isHTMLAnchorElement` é a conferência
//!   do `tagName` `A`.
//! - **`setup`.** `workbenchInstantiationService(undefined, disposables)` com `ITunnelService.canTunnel => false` e
//!   `createInstance(LinkDetector)` viram os dublês de serviço do workbench (`TestFileService`, `TestPathService`,
//!   `TestEnvironmentService`, `TestEditorService`, `TestConfigurationService` e um `IOpenerService` que não é
//!   exercitado) passados a [`LinkDetector::new`]. Os serviços são montados antes do guarda de vazamentos de cada caso, como
//!   o `workbenchInstantiationService` do original, que vive fora do rastreio do caso.
//! - **Expressões regulares.** O `expectedOutput.test(output.outerHTML)` usa o mesmo YARR (`zyarr`) com o texto da
//!   expressão do original.
//! - **`isWindows`** é a plataforma do hospedeiro (`Platform::detect().is_windows`); o `nativeSep` também.
//! - **`async`.** `relativeLinkWithWorkspace` é `async` no original mas não espera nada; aqui é síncrono.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use std::rc::Rc;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::JsString;
use je_base::lifecycle::{DisposableRef, DisposableStore, IDisposable};
use je_base::network::schemas;
use je_base::platform::Platform;
use je_base::promise::Promise;
use je_base::uri::{Field, URI, UriChange};
use je_platform::configuration::test_configuration_service::TestConfigurationService;
use je_platform::opener::opener::{
    IExternalOpener, IExternalUriResolver, IOpener, IOpenerService, IValidator, OpenOptions, OpenResource, ResolveExternalUriOptions, ResolvedExternalUri,
};
use je_platform::resources::uri_file;
use je_platform::workspace::WorkspaceFolder;
use je_widgets::highlighted_label::IHighlight;
use je_workbench::contrib::debug::link_detector::{
    DebugLinkHoverBehavior, DebugLinkHoverBehaviorTypeData, Element, ILinkDetector, ITunnelService, LinkDetector, LinkifyOptions, file_link_to_uri,
};
use je_workbench::test::browser::workbench_test_services::{TestEditorService, TestEnvironmentService, TestPathService};
use je_workbench::test::common::workbench_test_services::TestFileService;
use zyarr::RegExp;

/// `{ canTunnel: () => false }`.
struct NoTunnelService;

impl ITunnelService for NoTunnelService {
    fn can_tunnel(&self, _uri: &je_base::uri::URI) -> bool {
        false
    }
}

/// O `IOpenerService` do `workbenchInstantiationService`: nenhum caso abre um link web.
struct TestOpenerService;

impl IOpenerService for TestOpenerService {
    fn register_opener(&self, _opener: Rc<dyn IOpener>) -> DisposableRef {
        unreachable!("o linkDetector não registra opener")
    }

    fn register_validator(&self, _validator: Rc<dyn IValidator>) -> DisposableRef {
        unreachable!("o linkDetector não registra validador")
    }

    fn register_external_uri_resolver(&self, _resolver: Rc<dyn IExternalUriResolver>) -> DisposableRef {
        unreachable!("o linkDetector não registra resolvedor")
    }

    fn set_default_external_opener(&self, _opener: Rc<dyn IExternalOpener>) {
        unreachable!("o linkDetector não troca o opener externo")
    }

    fn register_external_opener(&self, _opener: Rc<dyn IExternalOpener>) -> DisposableRef {
        unreachable!("o linkDetector não registra opener externo")
    }

    fn open(&self, _resource: &OpenResource, _options: Option<&OpenOptions>) -> Promise<bool> {
        Promise::resolved(true)
    }

    fn resolve_external_uri(&self, _resource: &URI, _options: Option<&ResolveExternalUriOptions>) -> Promise<ResolvedExternalUri> {
        unreachable!("o linkDetector não resolve URI externo")
    }
}

/// O `setup`: o `LinkDetector` sobre os serviços de teste.
fn setup() -> LinkDetector {
    LinkDetector::new(
        Rc::new(TestEditorService),
        TestFileService::new(),
        Rc::new(TestOpenerService),
        TestPathService::new(None, None),
        Rc::new(NoTunnelService),
        TestEnvironmentService::new(),
        TestConfigurationService::new(None),
    )
}

fn is_windows() -> bool {
    Platform::detect().is_windows
}

/// `{ type: DebugLinkHoverBehavior.None, store: new DisposableStore() }`.
fn none_hover() -> DebugLinkHoverBehaviorTypeData {
    DebugLinkHoverBehaviorTypeData { kind: DebugLinkHoverBehavior::None, store: DisposableStore::new() }
}

/// `expectedOutput.test(value)` com o texto da expressão do original.
fn regex_test(pattern: &str, value: &str) -> bool {
    RegExp::from_str(pattern, "").expect("a expressão do teste é válida").exec(&value.encode_utf16().collect::<Vec<u16>>(), 0).is_some()
}

/// `assertElementIsLink`: `isHTMLAnchorElement(element)`.
fn assert_element_is_link(element: &Element) {
    assert_eq!(element.tag_name(), "A");
}

/// `linkify` com as opções do original (`splitLines`, `workspaceFolder`, `includeFulltext`, `highlights`).
fn linkify(link_detector: &LinkDetector, input: &str, hover_behavior: &DebugLinkHoverBehaviorTypeData, options: LinkifyOptions) -> Rc<Element> {
    link_detector.linkify(input, hover_behavior, &options)
}

fn split_lines() -> LinkifyOptions {
    LinkifyOptions { split_lines: true, ..LinkifyOptions::default() }
}

fn highlights(highlights: Vec<IHighlight>) -> LinkifyOptions {
    LinkifyOptions { highlights, ..LinkifyOptions::default() }
}

#[test]
fn no_links() {
    let link_detector = setup();
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let hover_behavior = none_hover();
    let input = "I am a string";
    let expected_output = "<span>I am a string</span>";
    let output = linkify(&link_detector, input, &hover_behavior, LinkifyOptions::default());

    assert_eq!(0, output.children().len());
    assert_eq!("SPAN", output.tag_name());
    assert_eq!(expected_output, output.outer_html());
    hover_behavior.store.dispose();
}

#[test]
fn trailing_newline() {
    let link_detector = setup();
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let hover_behavior = none_hover();
    let input = "I am a string\n";
    let expected_output = "<span>I am a string\n</span>";
    let output = linkify(&link_detector, input, &hover_behavior, LinkifyOptions::default());

    assert_eq!(0, output.children().len());
    assert_eq!("SPAN", output.tag_name());
    assert_eq!(expected_output, output.outer_html());
    hover_behavior.store.dispose();
}

#[test]
fn trailing_newline_split() {
    let link_detector = setup();
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let hover_behavior = none_hover();
    let input = "I am a string\n";
    let expected_output = "<span>I am a string\n</span>";
    let output = linkify(&link_detector, input, &hover_behavior, split_lines());

    assert_eq!(0, output.children().len());
    assert_eq!("SPAN", output.tag_name());
    assert_eq!(expected_output, output.outer_html());
    hover_behavior.store.dispose();
}

#[test]
fn single_line_link() {
    let link_detector = setup();
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let hover_behavior = none_hover();
    let input = if is_windows() { "C:\\foo\\bar.js:12:34" } else { "/Users/foo/bar.js:12:34" };
    let expected_output = if is_windows() { "<span><a tabindex=\"0\">C:\\foo\\bar.js:12:34</a></span>" } else { "<span><a tabindex=\"0\">/Users/foo/bar.js:12:34</a></span>" };
    let output = linkify(&link_detector, input, &hover_behavior, LinkifyOptions::default());

    assert_eq!(1, output.children().len());
    assert_eq!("SPAN", output.tag_name());
    assert_eq!("A", output.first_element_child().unwrap().tag_name());
    assert_eq!(expected_output, output.outer_html());
    assert_element_is_link(&output.first_element_child().unwrap());
    assert_eq!(if is_windows() { "C:\\foo\\bar.js:12:34" } else { "/Users/foo/bar.js:12:34" }, output.first_element_child().unwrap().text_content());
    hover_behavior.store.dispose();
}

#[test]
fn allows_links_with_at_282635() {
    let link_detector = setup();
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    if !is_windows() {
        let hover_behavior = none_hover();
        let input = "(/home/alexey_korepov/projects/dt2/playwright/node_modules/.pnpm/playwright-core@1.57.0/node_modules/playwright-core/lib/client/errors.js:56:16)";
        let expected_output = "<span>(<a tabindex=\"0\">/home/alexey_korepov/projects/dt2/playwright/node_modules/.pnpm/playwright-core@1.57.0/node_modules/playwright-core/lib/client/errors.js:56:16</a>)</span>";
        let output = linkify(&link_detector, input, &hover_behavior, LinkifyOptions::default());

        assert_eq!(expected_output, output.outer_html());
        assert_eq!(1, output.children().len());
        hover_behavior.store.dispose();
    }
}

#[test]
fn relative_link() {
    let link_detector = setup();
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let hover_behavior = none_hover();
    let input = "./foo/bar.js";
    let expected_output = "<span>./foo/bar.js</span>";
    let output = linkify(&link_detector, input, &hover_behavior, LinkifyOptions::default());

    assert_eq!(0, output.children().len());
    assert_eq!("SPAN", output.tag_name());
    assert_eq!(expected_output, output.outer_html());
    hover_behavior.store.dispose();
}

#[test]
fn relative_link_with_workspace() {
    let link_detector = setup();
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let hover_behavior = none_hover();
    let input = "./foo/bar.js";
    let options = LinkifyOptions { workspace_folder: Some(WorkspaceFolder::new(uri_file("/path/to/workspace"), "ws", 0, None)), ..LinkifyOptions::default() };
    let output = linkify(&link_detector, input, &hover_behavior, options);
    assert_eq!("SPAN", output.tag_name());
    assert!(output.outer_html().contains("link"));
    hover_behavior.store.dispose();
}

#[test]
fn single_line_link_and_text() {
    let link_detector = setup();
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let hover_behavior = none_hover();
    let input = if is_windows() { "The link: C:/foo/bar.js:12:34" } else { "The link: /Users/foo/bar.js:12:34" };
    let expected_output = r#"^<span>The link: <a tabindex="0">.*\/foo\/bar.js:12:34<\/a><\/span>$"#;
    let output = linkify(&link_detector, input, &hover_behavior, LinkifyOptions::default());
    let children = output.children();

    assert_eq!(1, children.len());
    assert_eq!("SPAN", output.tag_name());
    assert_eq!("A", children[0].tag_name());
    assert!(regex_test(expected_output, &output.outer_html()));
    assert_element_is_link(&children[0]);
    assert_eq!(if is_windows() { "C:/foo/bar.js:12:34" } else { "/Users/foo/bar.js:12:34" }, children[0].text_content());
    hover_behavior.store.dispose();
}

#[test]
fn single_line_multiple_links() {
    let link_detector = setup();
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let hover_behavior = none_hover();
    let input = if is_windows() {
        "Here is a link C:/foo/bar.js:12:34 and here is another D:/boo/far.js:56:78"
    } else {
        "Here is a link /Users/foo/bar.js:12:34 and here is another /Users/boo/far.js:56:78"
    };
    let expected_output = r#"^<span>Here is a link <a tabindex="0">.*\/foo\/bar.js:12:34<\/a> and here is another <a tabindex="0">.*\/boo\/far.js:56:78<\/a><\/span>$"#;
    let output = linkify(&link_detector, input, &hover_behavior, LinkifyOptions::default());
    let children = output.children();

    assert_eq!(2, children.len());
    assert_eq!("SPAN", output.tag_name());
    assert_eq!("A", children[0].tag_name());
    assert_eq!("A", children[1].tag_name());
    assert!(regex_test(expected_output, &output.outer_html()));
    assert_element_is_link(&children[0]);
    assert_element_is_link(&children[1]);
    assert_eq!(if is_windows() { "C:/foo/bar.js:12:34" } else { "/Users/foo/bar.js:12:34" }, children[0].text_content());
    assert_eq!(if is_windows() { "D:/boo/far.js:56:78" } else { "/Users/boo/far.js:56:78" }, children[1].text_content());
    hover_behavior.store.dispose();
}

#[test]
fn multiline_no_links() {
    let link_detector = setup();
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let hover_behavior = none_hover();
    let input = "Line one\nLine two\nLine three";
    let expected_output = r"^<span><span>Line one\n<\/span><span>Line two\n<\/span><span>Line three<\/span><\/span>$";
    let output = linkify(&link_detector, input, &hover_behavior, split_lines());
    let children = output.children();

    assert_eq!(3, children.len());
    assert_eq!("SPAN", output.tag_name());
    assert_eq!("SPAN", children[0].tag_name());
    assert_eq!("SPAN", children[1].tag_name());
    assert_eq!("SPAN", children[2].tag_name());
    assert!(regex_test(expected_output, &output.outer_html()));
    hover_behavior.store.dispose();
}

#[test]
fn multiline_trailing_newline() {
    let link_detector = setup();
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let hover_behavior = none_hover();
    let input = "I am a string\nAnd I am another\n";
    let expected_output = "<span><span>I am a string\n</span><span>And I am another\n</span></span>";
    let output = linkify(&link_detector, input, &hover_behavior, split_lines());
    let children = output.children();

    assert_eq!(2, children.len());
    assert_eq!("SPAN", output.tag_name());
    assert_eq!("SPAN", children[0].tag_name());
    assert_eq!("SPAN", children[1].tag_name());
    assert_eq!(expected_output, output.outer_html());
    hover_behavior.store.dispose();
}

#[test]
fn multiline_with_links() {
    let link_detector = setup();
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let hover_behavior = none_hover();
    let input = if is_windows() {
        "I have a link for you\nHere it is: C:/foo/bar.js:12:34\nCool, huh?"
    } else {
        "I have a link for you\nHere it is: /Users/foo/bar.js:12:34\nCool, huh?"
    };
    let expected_output =
        r#"^<span><span>I have a link for you\n<\/span><span>Here it is: <a tabindex="0">.*\/foo\/bar.js:12:34<\/a>\n<\/span><span>Cool, huh\?<\/span><\/span>$"#;
    let output = linkify(&link_detector, input, &hover_behavior, split_lines());
    let children = output.children();

    assert_eq!(3, children.len());
    assert_eq!("SPAN", output.tag_name());
    assert_eq!("SPAN", children[0].tag_name());
    assert_eq!("SPAN", children[1].tag_name());
    assert_eq!("SPAN", children[2].tag_name());
    assert_eq!("A", children[1].children()[0].tag_name());
    assert!(regex_test(expected_output, &output.outer_html()));
    assert_element_is_link(&children[1].children()[0]);
    assert_eq!(if is_windows() { "C:/foo/bar.js:12:34" } else { "/Users/foo/bar.js:12:34" }, children[1].children()[0].text_content());
    hover_behavior.store.dispose();
}

#[test]
fn highlight_no_links() {
    let link_detector = setup();
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let hover_behavior = none_hover();
    let input = "I am a string";
    let expected_output = "<span>I <span class=\"highlight\">am </span>a string</span>";
    let output = linkify(&link_detector, input, &hover_behavior, highlights(vec![IHighlight::new(2, 5)]));

    assert_eq!(1, output.children().len());
    assert_eq!("SPAN", output.tag_name());
    assert_eq!(expected_output, output.outer_html());
    hover_behavior.store.dispose();
}

#[test]
fn highlight_with_link() {
    let link_detector = setup();
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let hover_behavior = none_hover();
    let input = if is_windows() { "C:\\foo\\bar.js:12:34" } else { "/Users/foo/bar.js:12:34" };
    let expected_output = if is_windows() {
        "<span><a tabindex=\"0\"><span class=\"highlight\">C:\\fo</span>o\\bar.js:12:34</a></span>"
    } else {
        "<span><a tabindex=\"0\"><span class=\"highlight\">/User</span>s/foo/bar.js:12:34</a></span>"
    };
    let output = linkify(&link_detector, input, &hover_behavior, highlights(vec![IHighlight::new(0, 5)]));

    assert_eq!(1, output.children().len());
    assert_eq!("SPAN", output.tag_name());
    assert_eq!("A", output.first_element_child().unwrap().tag_name());
    assert_eq!(expected_output, output.outer_html());
    assert_element_is_link(&output.first_element_child().unwrap());
    hover_behavior.store.dispose();
}

#[test]
fn highlight_overlapping_link_start() {
    let link_detector = setup();
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let hover_behavior = none_hover();
    let input = if is_windows() { "C:\\foo\\bar.js:12:34" } else { "/Users/foo/bar.js:12:34" };
    let expected_output = if is_windows() {
        "<span><a tabindex=\"0\"><span class=\"highlight\">C:\\foo\\bar</span>.js:12:34</a></span>"
    } else {
        "<span><a tabindex=\"0\"><span class=\"highlight\">/Users/foo</span>/bar.js:12:34</a></span>"
    };
    let output = linkify(&link_detector, input, &hover_behavior, highlights(vec![IHighlight::new(0, 10)]));

    assert_eq!(1, output.children().len());
    assert_eq!("SPAN", output.tag_name());
    assert_eq!("A", output.first_element_child().unwrap().tag_name());
    assert_eq!(expected_output, output.outer_html());
    assert_element_is_link(&output.first_element_child().unwrap());
    hover_behavior.store.dispose();
}

#[test]
fn highlight_overlapping_link_end() {
    let link_detector = setup();
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let hover_behavior = none_hover();
    let input = if is_windows() { "C:\\foo\\bar.js:12:34" } else { "/Users/foo/bar.js:12:34" };
    let expected_output = if is_windows() {
        "<span><a tabindex=\"0\">C:\\foo\\bar<span class=\"highlight\">.js:12:34</span></a></span>"
    } else {
        "<span><a tabindex=\"0\">/Users/foo<span class=\"highlight\">/bar.js:12</span>:34</a></span>"
    };
    let output = linkify(&link_detector, input, &hover_behavior, highlights(vec![IHighlight::new(10, 20)]));

    assert_eq!(1, output.children().len());
    assert_eq!("SPAN", output.tag_name());
    assert_eq!("A", output.first_element_child().unwrap().tag_name());
    assert_eq!(expected_output, output.outer_html());
    assert_element_is_link(&output.first_element_child().unwrap());
    hover_behavior.store.dispose();
}

#[test]
fn highlight_overlapping_link_start_and_end() {
    let link_detector = setup();
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let hover_behavior = none_hover();
    let input = if is_windows() { "C:\\foo\\bar.js:12:34" } else { "/Users/foo/bar.js:12:34" };
    let expected_output = if is_windows() {
        "<span><a tabindex=\"0\">C:\\fo<span class=\"highlight\">o\\bar.js:1</span>2:34</a></span>"
    } else {
        "<span><a tabindex=\"0\">/User<span class=\"highlight\">s/foo/bar.</span>js:12:34</a></span>"
    };
    let output = linkify(&link_detector, input, &hover_behavior, highlights(vec![IHighlight::new(5, 15)]));

    assert_eq!(1, output.children().len());
    assert_eq!("SPAN", output.tag_name());
    assert_eq!("A", output.first_element_child().unwrap().tag_name());
    assert_eq!(expected_output, output.outer_html());
    assert_element_is_link(&output.first_element_child().unwrap());
    hover_behavior.store.dispose();
}

#[test]
fn csharp_stack_trace_format_with_line() {
    let link_detector = setup();
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let hover_behavior = none_hover();
    let input = if is_windows() { "C:\\foo\\bar.cs:line 6" } else { "/Users/foo/bar.cs:line 6" };
    let expected_output = if is_windows() { "<span><a tabindex=\"0\">C:\\foo\\bar.cs:line 6</a></span>" } else { "<span><a tabindex=\"0\">/Users/foo/bar.cs:line 6</a></span>" };
    let output = linkify(&link_detector, input, &hover_behavior, LinkifyOptions::default());

    assert_eq!(1, output.children().len());
    assert_eq!("SPAN", output.tag_name());
    assert_eq!("A", output.first_element_child().unwrap().tag_name());
    assert_eq!(expected_output, output.outer_html());
    assert_element_is_link(&output.first_element_child().unwrap());
    assert_eq!(if is_windows() { "C:\\foo\\bar.cs:line 6" } else { "/Users/foo/bar.cs:line 6" }, output.first_element_child().unwrap().text_content());
    hover_behavior.store.dispose();
}

#[test]
fn csharp_stack_trace_format_with_line_and_column() {
    let link_detector = setup();
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let hover_behavior = none_hover();
    let input = if is_windows() { "C:\\foo\\bar.cs:line 6:10" } else { "/Users/foo/bar.cs:line 6:10" };
    let expected_output =
        if is_windows() { "<span><a tabindex=\"0\">C:\\foo\\bar.cs:line 6:10</a></span>" } else { "<span><a tabindex=\"0\">/Users/foo/bar.cs:line 6:10</a></span>" };
    let output = linkify(&link_detector, input, &hover_behavior, LinkifyOptions::default());

    assert_eq!(1, output.children().len());
    assert_eq!("SPAN", output.tag_name());
    assert_eq!("A", output.first_element_child().unwrap().tag_name());
    assert_eq!(expected_output, output.outer_html());
    assert_element_is_link(&output.first_element_child().unwrap());
    assert_eq!(if is_windows() { "C:\\foo\\bar.cs:line 6:10" } else { "/Users/foo/bar.cs:line 6:10" }, output.first_element_child().unwrap().text_content());
    hover_behavior.store.dispose();
}

#[test]
fn mixed_stack_trace_formats() {
    let link_detector = setup();
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let hover_behavior = none_hover();
    let input = if is_windows() { "C:\\foo\\bar.js:12:34 and C:\\baz\\qux.cs:line 6" } else { "/Users/foo/bar.js:12:34 and /Users/baz/qux.cs:line 6" };
    // Use flexible path separator matching for cross-platform compatibility
    let expected_output = if is_windows() {
        r#"^<span><a tabindex="0">.*\\foo\\bar\.js:12:34<\/a> and <a tabindex="0">.*\\baz\\qux\.cs:line 6<\/a><\/span>$"#
    } else {
        r#"^<span><a tabindex="0">.*\/foo\/bar\.js:12:34<\/a> and <a tabindex="0">.*\/baz\/qux\.cs:line 6<\/a><\/span>$"#
    };
    let output = linkify(&link_detector, input, &hover_behavior, LinkifyOptions::default());
    let children = output.children();

    assert_eq!(2, children.len());
    assert_eq!("SPAN", output.tag_name());
    assert_eq!("A", children[0].tag_name());
    assert_eq!("A", children[1].tag_name());
    assert!(regex_test(expected_output, &output.outer_html()));
    assert_element_is_link(&children[0]);
    assert_element_is_link(&children[1]);
    assert_eq!(if is_windows() { "C:\\foo\\bar.js:12:34" } else { "/Users/foo/bar.js:12:34" }, children[0].text_content());
    assert_eq!(if is_windows() { "C:\\baz\\qux.cs:line 6" } else { "/Users/baz/qux.cs:line 6" }, children[1].text_content());
    hover_behavior.store.dispose();
}

#[test]
fn file_uri_with_line_and_column() {
    let link_detector = setup();
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let hover_behavior = none_hover();
    let input = if is_windows() { "file:///c:/foo/bar.js:12:34" } else { "file:///Users/foo/bar.js:12:34" };
    let output = linkify(&link_detector, input, &hover_behavior, LinkifyOptions::default());

    assert_eq!(1, output.children().len());
    assert_eq!("SPAN", output.tag_name());
    assert_element_is_link(&output.first_element_child().unwrap());
    // the whole URI, including the `:line:column` suffix, is part of the link
    assert_eq!(input, output.first_element_child().unwrap().text_content());
    hover_behavior.store.dispose();
}

#[test]
fn file_uri_without_line_and_column() {
    let link_detector = setup();
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let hover_behavior = none_hover();
    let input = if is_windows() { "file:///c:/foo/bar.js" } else { "file:///Users/foo/bar.js" };
    let output = linkify(&link_detector, input, &hover_behavior, LinkifyOptions::default());

    assert_eq!(1, output.children().len());
    assert_element_is_link(&output.first_element_child().unwrap());
    assert_eq!(input, output.first_element_child().unwrap().text_content());
    hover_behavior.store.dispose();
}

// suite('Debug - Link Detector - fileLinkToUri')

fn native_sep() -> &'static str {
    if is_windows() { "\\" } else { "/" }
}

#[test]
fn keeps_the_file_scheme_for_a_plain_file_uri() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let input = URI::parse(if is_windows() { "file:///c:/foo/bar.js" } else { "file:///Users/foo/bar.js" }, false);
    let actual = file_link_to_uri(&input, native_sep());

    assert_eq!(actual.scheme().to_string_lossy(), schemas::FILE);
    assert_eq!(actual.path().to_string_lossy(), if is_windows() { "/c:/foo/bar.js" } else { "/Users/foo/bar.js" });
}

#[test]
fn does_not_read_the_drive_letter_as_a_scheme_334283() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    // `URI.parse` on the file system path `c:/foo/bar.js` yields scheme `c` and drops the drive letter, which left the
    // link unopenable. This holds on every host OS, so the case is not guarded by `isWindows`.
    let input = URI::parse("file:///c:/foo/bar.js", false);
    let actual = file_link_to_uri(&input, native_sep());

    assert_eq!(actual.scheme().to_string_lossy(), schemas::FILE);
    assert_ne!(actual.scheme().to_string_lossy(), "c");
    assert_eq!(actual.path().to_string_lossy(), "/c:/foo/bar.js");
}

#[test]
fn round_trips_a_uri_that_already_had_its_line_and_column_stripped() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let with_suffix = URI::parse(if is_windows() { "file:///c:/foo/bar.js:12:34" } else { "file:///Users/foo/bar.js:12:34" }, false);
    // `withSuffix.path.replace(/:\d+:\d+$/, '')`
    let path = with_suffix.path().to_string_lossy();
    let units: Vec<u16> = path.encode_utf16().collect();
    let found = RegExp::from_str(r":\d+:\d+$", "").unwrap().exec(&units, 0);
    let stripped_path = match found {
        Some(found) => JsString::from(&units[..found.start()]),
        None => with_suffix.path().clone(),
    };
    let stripped = with_suffix.with(&UriChange { path: Field::Set(stripped_path), ..UriChange::default() });
    let actual = file_link_to_uri(&stripped, native_sep());

    assert_eq!(actual.scheme().to_string_lossy(), schemas::FILE);
    assert_eq!(actual.path().to_string_lossy(), if is_windows() { "/c:/foo/bar.js" } else { "/Users/foo/bar.js" });
}

#[test]
fn normalizes_separators_when_the_remote_uses_posix_paths() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let input = URI::parse(if is_windows() { "file:///c:/foo/../foo/bar.js" } else { "file:///Users/foo/../foo/bar.js" }, false);
    let actual = file_link_to_uri(&input, "/");

    assert_eq!(actual.scheme().to_string_lossy(), schemas::FILE);
    assert_eq!(actual.path().to_string_lossy(), if is_windows() { "/c:/foo/bar.js" } else { "/Users/foo/bar.js" });
}
