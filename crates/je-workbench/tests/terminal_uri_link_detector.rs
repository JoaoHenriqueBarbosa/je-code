//! Porte de `src/vs/workbench/contrib/terminalContrib/links/test/browser/terminalUriLinkDetector.test.ts` (suíte `Workbench - TerminalUriLinkDetector`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - **`setup` e `ensureNoDisposablesAreLeakedInTestSuite()`.** O [`Fixture`] cria o guarda de vazamentos antes de tudo e traz o
//!   `IFileService` do teste (o [`LinkTestFileService::with_path_matching`], cujo `stat` só olha o `path`), o `FakeXterm` (80
//!   colunas por 30 linhas) no lugar do `Terminal` do xterm, o `UriIdentityService` real, o `TestContextService` e o
//!   `TerminalLinkResolver` real. O `teardown` (`instantiationService.dispose()`) é o `Drop` do guarda.
//! - **`assertLink`.** Delega ao `assert_link_helper` de `tests/support/terminal_link_support.rs`.
//! - **`for (const c of linkComputerCases) test(...)`.** O macro `link_computer_case!` gera um `#[test]` por caso.
//! - **Casos fora do porte.** Os dois casos CJK de `linkComputerCases` (`請參閱 http://...。` e `（請參閱 http://...）`) esperam
//!   as posições do xterm real, em que cada ideograma ocupa duas células; o `FakeXterm` não tem caracteres largos (toda célula
//!   tem largura 1), então o intervalo esperado não é reproduzível. Sem `#[test]`.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod support;

use std::rc::Rc;

use common::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite, run_with_faked_timers};
use je_base::platform::OperatingSystem;
use je_base::uri::URI;
use je_instantiation::Injectable;
use je_platform::files::i_file_service::IFileService;
use je_platform::log::NullLogService;
use je_platform::resources::uri_file;
use je_platform::uri_identity::uri_identity_service::UriIdentityService;
use je_workbench::contrib::terminal_contrib::links::links::{TerminalBuiltinLinkType, TerminalLinkProcessManager};
use je_workbench::contrib::terminal_contrib::links::terminal_link_resolver::TerminalLinkResolver;
use je_workbench::contrib::terminal_contrib::links::terminal_uri_link_detector::TerminalUriLinkDetector;
use je_workbench::test::common::workbench_test_services::TestContextService;
use support::fake_xterm::FakeXterm;
use support::terminal_link_support::{LinkTestFileService, assert_link_helper};

/// O `setup` da suíte.
struct Fixture {
    file_service: Rc<LinkTestFileService>,
    xterm: Rc<FakeXterm>,
    detector: Rc<TerminalUriLinkDetector>,
    // Declarado por último: o `Drop` dele descarta o que o `Fixture` registrou e confere os vazamentos.
    _guard: LeakGuard,
}

impl Fixture {
    fn new() -> Self {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let file_service = LinkTestFileService::with_path_matching();
        let uri_identity_service = guard.add_rc(UriIdentityService::create((), (file_service.clone() as Rc<dyn IFileService>,)));
        let workspace_context_service = TestContextService::new(None, None);
        let resolver = Rc::new(TerminalLinkResolver::new(file_service.clone()));
        let xterm = FakeXterm::new(80, 30);
        let detector = Rc::new(TerminalUriLinkDetector::new(
            xterm.clone(),
            TerminalLinkProcessManager {
                initial_cwd: "/parent/cwd".to_owned(),
                os: Some(OperatingSystem::Linux),
                remote_authority: None,
                user_home: Some("/home".to_owned()),
                backend: None,
            },
            resolver,
            NullLogService::shared(),
            uri_identity_service,
            workspace_context_service,
        ));
        Self { file_service, xterm, detector, _guard: guard }
    }

    /// `validResources = resources`.
    fn set_valid_resources(&self, resources: Vec<URI>) {
        self.file_service.set_files(resources);
    }

    /// `assertLink(type, text, expected)`: `expected` é a lista de `(uri, intervalo)`.
    async fn assert_link(&self, type_: TerminalBuiltinLinkType, text: &str, expected: &[(URI, [(i32, i32); 2])]) {
        let expected: Vec<(String, [(i32, i32); 2])> = expected.iter().map(|(uri, range)| (uri.to_string(false).to_string_lossy(), *range)).collect();
        assert_link_helper(&self.xterm, self.detector.as_ref(), text, &expected, type_).await;
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// linkComputerCases
// ---------------------------------------------------------------------------------------------------------------------

/// `test('link computer case: `' + c[1] + '`')` de um caso com um único link: `(início x, início y, fim x, fim y)`.
macro_rules! link_computer_case {
    ($name:ident, $type_:expr, $text:expr, ($sx:expr, $sy:expr, $ex:expr, $ey:expr), $uri:expr, $stat:expr) => {
        #[test]
        fn $name() {
            run_with_faked_timers(async {
                let f = Fixture::new();
                let stat: Option<&str> = $stat;
                f.set_valid_resources(stat.map(|resource| vec![URI::parse(resource, false)]).unwrap_or_default());
                f.assert_link($type_, $text, &[(URI::parse($uri, false), [($sx, $sy), ($ex, $ey)])]).await;
            });
        }
    };
}

link_computer_case!(link_computer_case_double_quotes, TerminalBuiltinLinkType::Url, "x = \"http://foo.bar\";", (6, 1, 19, 1), "http://foo.bar", None);
link_computer_case!(link_computer_case_parentheses, TerminalBuiltinLinkType::Url, "x = (http://foo.bar);", (6, 1, 19, 1), "http://foo.bar", None);
link_computer_case!(link_computer_case_single_quotes, TerminalBuiltinLinkType::Url, "x = 'http://foo.bar';", (6, 1, 19, 1), "http://foo.bar", None);
link_computer_case!(link_computer_case_surrounding_spaces, TerminalBuiltinLinkType::Url, "x =  http://foo.bar ;", (6, 1, 19, 1), "http://foo.bar", None);
link_computer_case!(link_computer_case_angle_brackets, TerminalBuiltinLinkType::Url, "x = <http://foo.bar>;", (6, 1, 19, 1), "http://foo.bar", None);
link_computer_case!(link_computer_case_curly_brackets, TerminalBuiltinLinkType::Url, "x = {http://foo.bar};", (6, 1, 19, 1), "http://foo.bar", None);
link_computer_case!(link_computer_case_see_parentheses, TerminalBuiltinLinkType::Url, "(see http://foo.bar)", (6, 1, 19, 1), "http://foo.bar", None);
link_computer_case!(link_computer_case_see_square_brackets, TerminalBuiltinLinkType::Url, "[see http://foo.bar]", (6, 1, 19, 1), "http://foo.bar", None);
link_computer_case!(link_computer_case_see_curly_brackets, TerminalBuiltinLinkType::Url, "{see http://foo.bar}", (6, 1, 19, 1), "http://foo.bar", None);
link_computer_case!(link_computer_case_see_angle_brackets, TerminalBuiltinLinkType::Url, "<see http://foo.bar>", (6, 1, 19, 1), "http://foo.bar", None);
link_computer_case!(link_computer_case_url_tag, TerminalBuiltinLinkType::Url, "<url>http://foo.bar</url>", (6, 1, 19, 1), "http://foo.bar", None);
link_computer_case!(
    link_computer_case_fwlink_wrapped,
    TerminalBuiltinLinkType::Url,
    "// Click here to learn more. https://go.microsoft.com/fwlink/?LinkID=513275&clcid=0x409",
    (30, 1, 7, 2),
    "https://go.microsoft.com/fwlink/?LinkID=513275&clcid=0x409",
    None
);
link_computer_case!(
    link_computer_case_msdn_with_parentheses,
    TerminalBuiltinLinkType::Url,
    "// Click here to learn more. https://msdn.microsoft.com/en-us/library/windows/desktop/aa365247(v=vs.85).aspx",
    (30, 1, 28, 2),
    "https://msdn.microsoft.com/en-us/library/windows/desktop/aa365247(v=vs.85).aspx",
    None
);
link_computer_case!(
    link_computer_case_kudu,
    TerminalBuiltinLinkType::Url,
    "// https://github.com/projectkudu/kudu/blob/master/Kudu.Core/Scripts/selectNodeVersion.js",
    (4, 1, 9, 2),
    "https://github.com/projectkudu/kudu/blob/master/Kudu.Core/Scripts/selectNodeVersion.js",
    None
);
link_computer_case!(
    link_computer_case_web_content_ref,
    TerminalBuiltinLinkType::Url,
    "<!-- !!! Do not remove !!!   WebContentRef(link:https://go.microsoft.com/fwlink/?LinkId=166007, area:Admin, updated:2015, nextUpdate:2016, tags:SqlServer)   !!! Do not remove !!! -->",
    (49, 1, 14, 2),
    "https://go.microsoft.com/fwlink/?LinkId=166007",
    None
);
link_computer_case!(
    link_computer_case_for_instructions_trailing_dot,
    TerminalBuiltinLinkType::Url,
    "For instructions, see https://go.microsoft.com/fwlink/?LinkId=166007.</value>",
    (23, 1, 68, 1),
    "https://go.microsoft.com/fwlink/?LinkId=166007",
    None
);
link_computer_case!(
    link_computer_case_for_instructions_msdn_trailing_dot,
    TerminalBuiltinLinkType::Url,
    "For instructions, see https://msdn.microsoft.com/en-us/library/windows/desktop/aa365247(v=vs.85).aspx.</value>",
    (23, 1, 21, 2),
    "https://msdn.microsoft.com/en-us/library/windows/desktop/aa365247(v=vs.85).aspx",
    None
);
link_computer_case!(link_computer_case_non_ascii_path, TerminalBuiltinLinkType::Url, "x = \"https://en.wikipedia.org/wiki/Zürich\";", (6, 1, 41, 1), "https://en.wikipedia.org/wiki/Zürich", None);
link_computer_case!(link_computer_case_file_triple_slash, TerminalBuiltinLinkType::LocalFile, "x = \"file:///foo.bar\";", (6, 1, 20, 1), "file:///foo.bar", Some("file:///foo.bar"));
link_computer_case!(link_computer_case_file_drive_authority, TerminalBuiltinLinkType::LocalFile, "x = \"file://c:/foo.bar\";", (6, 1, 22, 1), "file://c:/foo.bar", Some("file://c:/foo.bar"));
link_computer_case!(link_computer_case_file_share_authority, TerminalBuiltinLinkType::LocalFile, "x = \"file://shares/foo.bar\";", (6, 1, 26, 1), "file://shares/foo.bar", Some("file://shares/foo.bar"));
link_computer_case!(link_computer_case_file_non_ascii_authority, TerminalBuiltinLinkType::LocalFile, "x = \"file://shäres/foo.bar\";", (6, 1, 26, 1), "file://shäres/foo.bar", Some("file://shäres/foo.bar"));
link_computer_case!(link_computer_case_trailing_dot_after_domain, TerminalBuiltinLinkType::Url, "Some text, then http://www.bing.com.", (17, 1, 35, 1), "http://www.bing.com", None);
link_computer_case!(
    link_computer_case_backtick_sharepoint,
    TerminalBuiltinLinkType::Url,
    "let url = `http://***/_api/web/lists/GetByTitle('Teambuildingaanvragen')/items`;",
    (12, 1, 78, 1),
    "http://***/_api/web/lists/GetByTitle('Teambuildingaanvragen')/items",
    None
);
link_computer_case!(
    link_computer_case_markdown_link_wrapped,
    TerminalBuiltinLinkType::Url,
    "7. At this point, ServiceMain has been called.  There is no functionality presently in ServiceMain, but you can consult the [MSDN documentation](https://msdn.microsoft.com/en-us/library/windows/desktop/ms687414(v=vs.85).aspx) to add functionality as desired!",
    (66, 2, 64, 3),
    "https://msdn.microsoft.com/en-us/library/windows/desktop/ms687414(v=vs.85).aspx",
    None
);
link_computer_case!(link_computer_case_ipv6_host, TerminalBuiltinLinkType::Url, "let x = \"http://[::1]:5000/connect/token\"", (10, 1, 40, 1), "http://[::1]:5000/connect/token", None);
link_computer_case!(link_computer_case_markdown_bold, TerminalBuiltinLinkType::Url, "2. Navigate to **https://portal.azure.com**", (18, 1, 41, 1), "https://portal.azure.com", None);
link_computer_case!(link_computer_case_pipe_separated, TerminalBuiltinLinkType::Url, "POST|https://portal.azure.com|2019-12-05|", (6, 1, 29, 1), "https://portal.azure.com", None);
link_computer_case!(link_computer_case_brackets_with_spaces_in_path, TerminalBuiltinLinkType::Url, "aa  https://foo.bar/[this is foo site]  aa", (5, 1, 38, 1), "https://foo.bar/[this is foo site]", None);

// ---------------------------------------------------------------------------------------------------------------------
// Casos soltos
// ---------------------------------------------------------------------------------------------------------------------

#[test]
fn should_support_multiple_link_results() {
    run_with_faked_timers(async {
        let f = Fixture::new();
        f.assert_link(
            TerminalBuiltinLinkType::Url,
            "http://foo.bar http://bar.foo",
            &[(URI::parse("http://foo.bar", false), [(1, 1), (14, 1)]), (URI::parse("http://bar.foo", false), [(16, 1), (29, 1)])],
        )
        .await;
    });
}

#[test]
fn should_detect_file_links_with_line_suffix() {
    run_with_faked_timers(async {
        let f = Fixture::new();
        f.set_valid_resources(vec![uri_file("c:/folder/file")]);
        f.assert_link(TerminalBuiltinLinkType::LocalFile, "file:///c:/folder/file:23", &[(URI::parse("file:///c:/folder/file", false), [(1, 1), (25, 1)])]).await;
    });
}

#[test]
fn should_detect_file_links_with_line_col_suffix() {
    run_with_faked_timers(async {
        let f = Fixture::new();
        f.set_valid_resources(vec![uri_file("c:/folder/file")]);
        f.assert_link(TerminalBuiltinLinkType::LocalFile, "file:///c:/folder/file:23:10", &[(URI::parse("file:///c:/folder/file", false), [(1, 1), (28, 1)])]).await;
    });
}

#[test]
fn should_filter_out_https_link_that_exceed_4096_characters() {
    run_with_faked_timers(async {
        let f = Fixture::new();
        // 8 + 200 * 10 = 2008 characters
        let short = format!("https://{}", "foobarbaz/".repeat(200));
        f.assert_link(TerminalBuiltinLinkType::Url, &short, &[(URI::parse(short.as_str(), false), [(1, 1), (8, 26)])]).await;
        // 8 + 450 * 10 = 4508 characters
        f.assert_link(TerminalBuiltinLinkType::Url, &format!("https://{}", "foobarbaz/".repeat(450)), &[]).await;
    });
}

#[test]
fn should_filter_out_file_links_that_exceed_4096_characters() {
    run_with_faked_timers(async {
        let f = Fixture::new();
        // 8 + 200 * 10 = 2008 characters
        f.set_valid_resources(vec![uri_file(&format!("/{}", "foobarbaz/".repeat(200)))]);
        let short = format!("file:///{}", "foobarbaz/".repeat(200));
        f.assert_link(TerminalBuiltinLinkType::LocalFile, &short, &[(URI::parse(short.as_str(), false), [(1, 1), (8, 26)])]).await;
        // 8 + 450 * 10 = 4508 characters
        f.set_valid_resources(vec![uri_file(&format!("/{}", "foobarbaz/".repeat(450)))]);
        f.assert_link(TerminalBuiltinLinkType::LocalFile, &format!("file:///{}", "foobarbaz/".repeat(450)), &[]).await;
    });
}
