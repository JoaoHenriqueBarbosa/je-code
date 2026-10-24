//! Porte de `src/vs/workbench/contrib/terminalContrib/links/test/browser/terminalMultiLineLinkDetector.test.ts` (suíte `Workbench - TerminalMultiLineLinkDetector`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - **`setup` e `ensureNoDisposablesAreLeakedInTestSuite()`.** O [`Fixture`] cria o guarda de vazamentos antes de tudo e traz o
//!   `IFileService` do teste (o [`LinkTestFileService::with_path_matching`], cujo `stat` só olha o `path`), o `FakeXterm` (80
//!   colunas por 30 linhas) no lugar do `Terminal` do xterm, o `UriIdentityService` real, o `TestContextService` e o
//!   `TerminalLinkResolver` real. O `setup` de cada suíte interna é o `process_manager` passado a [`Fixture::new`].
//! - **`assertLinks` e a corrida contra `timeout(2)`.** O original corre a asserção contra um `timeout(2)` real. Os casos rodam em
//!   `run_with_faked_timers`, em que o relógio é virtual e a asserção termina antes de o `timeout` valer; a corrida não existe aqui.
//! - **`for (const l of unixLinks) { suite(...) { for (formats) test(...) } }`.** O macro `unix_link_suite!` gera um módulo por
//!   link, com um `#[test]` por formato (`format_0` a `format_7`, na ordem de `supportedLinkFormats`).
//! - **`if (isWindows)`.** `isWindows` é a plataforma do hospedeiro (`Platform::detect().is_windows`): os casos do bloco existem como
//!   `#[test]` (um por link, percorrendo os formatos) e saem no começo quando a plataforma de execução não é o Windows (o original
//!   nem os registra).
//! - **`format(urlFormat, baseLink, line, column)`.** É [`format_link`], a substituição de `{0}`, `{1}` e `{2}`.
//! - **`URI.file(...)`.** É `uri_file` (na plataforma do hospedeiro).

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod support;

use std::rc::Rc;

use common::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite, run_with_faked_timers};
use je_base::platform::{OperatingSystem, Platform};
use je_base::uri::URI;
use je_instantiation::Injectable;
use je_platform::files::i_file_service::IFileService;
use je_platform::log::NullLogService;
use je_platform::resources::uri_file;
use je_platform::uri_identity::uri_identity_service::UriIdentityService;
use je_workbench::contrib::terminal_contrib::links::links::{TerminalBuiltinLinkType, TerminalLinkProcessManager};
use je_workbench::contrib::terminal_contrib::links::terminal_link_resolver::TerminalLinkResolver;
use je_workbench::contrib::terminal_contrib::links::terminal_multi_line_link_detector::TerminalMultiLineLinkDetector;
use je_workbench::test::common::workbench_test_services::TestContextService;
use support::fake_xterm::FakeXterm;
use support::terminal_link_support::{LinkTestFileService, assert_link_helper};

/// `unixLinks`: `string | { link, resource }`; `None` é o `string` (o recurso é o `URI.file` do próprio texto).
const UNIX_LINKS: [(&str, Option<&str>); 12] = [
    // Absolute
    ("/foo", None),
    ("/foo/bar", None),
    ("/foo/[bar]", None),
    ("/foo/[bar].baz", None),
    ("/foo/[bar]/baz", None),
    ("/foo/bar+more", None),
    // User home
    ("~/foo", Some("/home/foo")),
    // Relative
    ("./foo", Some("/parent/cwd/foo")),
    ("./$foo", Some("/parent/cwd/$foo")),
    ("../foo", Some("/parent/foo")),
    ("foo/bar", Some("/parent/cwd/foo/bar")),
    ("foo/bar+more", Some("/parent/cwd/foo/bar+more")),
];

/// `windowsLinks`.
const WINDOWS_LINKS: [(&str, Option<&str>); 22] = [
    // Absolute
    ("c:\\foo", None),
    ("\\\\?\\C:\\foo", Some("C:\\foo")),
    ("c:/foo", None),
    ("c:/foo/bar", None),
    ("c:\\foo\\bar", None),
    ("c:\\foo\\bar+more", None),
    ("c:\\foo/bar\\baz", None),
    // User home
    ("~\\foo", Some("C:\\Home\\foo")),
    ("~/foo", Some("C:\\Home\\foo")),
    // Relative
    (".\\foo", Some("C:\\Parent\\Cwd\\foo")),
    ("./foo", Some("C:\\Parent\\Cwd\\foo")),
    ("./$foo", Some("C:\\Parent\\Cwd\\$foo")),
    ("..\\foo", Some("C:\\Parent\\foo")),
    ("foo/bar", Some("C:\\Parent\\Cwd\\foo\\bar")),
    ("foo/bar", Some("C:\\Parent\\Cwd\\foo\\bar")),
    ("foo/[bar]", Some("C:\\Parent\\Cwd\\foo\\[bar]")),
    ("foo/[bar].baz", Some("C:\\Parent\\Cwd\\foo\\[bar].baz")),
    ("foo/[bar]/baz", Some("C:\\Parent\\Cwd\\foo\\[bar]/baz")),
    ("foo\\bar", Some("C:\\Parent\\Cwd\\foo\\bar")),
    ("foo\\[bar].baz", Some("C:\\Parent\\Cwd\\foo\\[bar].baz")),
    ("foo\\[bar]\\baz", Some("C:\\Parent\\Cwd\\foo\\[bar]\\baz")),
    ("foo\\bar+more", Some("C:\\Parent\\Cwd\\foo\\bar+more")),
];

/// `LinkFormatInfo` (sem os deslocamentos, que este teste não usa): o formato e o `line` e o `column` que ele leva.
struct LinkFormatInfo {
    url_format: String,
    line: Option<&'static str>,
    column: Option<&'static str>,
}

fn format_info(url_format: &str, line: Option<&'static str>, column: Option<&'static str>) -> LinkFormatInfo {
    LinkFormatInfo { url_format: url_format.to_owned(), line, column }
}

/// `supportedLinkFormats`.
fn supported_link_formats() -> Vec<LinkFormatInfo> {
    vec![
        // 5: file content...                         [#181837]
        //   5:3  error                               [#181837]
        format_info("{0}\r\n{1}:foo", Some("5"), None),
        format_info("{0}\r\n{1}: foo", Some("5"), None),
        format_info("{0}\r\n5:another link\r\n{1}:{2} foo", Some("5"), Some("3")),
        format_info("{0}\r\n  {1}:{2} foo", Some("5"), Some("3")),
        format_info("{0}\r\n  5:6  error  another one\r\n  {1}:{2}  error", Some("5"), Some("3")),
        format_info(&format!("{{0}}\r\n  5:6  error  {}\r\n  {{1}}:{{2}}  error", "a".repeat(80)), Some("5"), Some("3")),
        // @@ ... <to-file-range> @@ content...       [#182878]   (tests check the entire line, so they don't include the line content at the end of the last @@)
        format_info("+++ b/{0}\r\n@@ -7,6 +{1},7 @@", Some("5"), None),
        format_info("+++ b/{0}\r\n@@ -1,1 +1,1 @@\r\nfoo\r\nbar\r\n@@ -7,6 +{1},7 @@", Some("5"), None),
    ]
}

/// `format(urlFormat, baseLink, line, column)`: troca `{0}`, `{1}` e `{2}`.
fn format_link(url_format: &str, base_link: &str, line: Option<&str>, column: Option<&str>) -> String {
    let mut result = url_format.replace("{0}", base_link);
    if let Some(line) = line {
        result = result.replace("{1}", line);
    }
    if let Some(column) = column {
        result = result.replace("{2}", column);
    }
    result
}

/// O `setup` da suíte.
struct Fixture {
    file_service: Rc<LinkTestFileService>,
    xterm: Rc<FakeXterm>,
    detector: Rc<TerminalMultiLineLinkDetector>,
    // Declarado por último: o `Drop` dele descarta o que o `Fixture` registrou e confere os vazamentos.
    _guard: LeakGuard,
}

impl Fixture {
    fn new(process_manager: TerminalLinkProcessManager) -> Self {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let file_service = LinkTestFileService::with_path_matching();
        let uri_identity_service = guard.add_rc(UriIdentityService::create((), (file_service.clone() as Rc<dyn IFileService>,)));
        let workspace_context_service = TestContextService::new(None, None);
        let resolver = Rc::new(TerminalLinkResolver::new(file_service.clone()));
        let xterm = FakeXterm::new(80, 30);
        let detector = Rc::new(TerminalMultiLineLinkDetector::new(
            xterm.clone(),
            process_manager,
            resolver,
            NullLogService::shared(),
            uri_identity_service,
            workspace_context_service,
        ));
        Self { file_service, xterm, detector, _guard: guard }
    }

    /// `validResources = [resource]`.
    fn set_valid_resources(&self, resources: Vec<URI>) {
        self.file_service.set_files(resources);
    }

    /// `assertLinksMain(link, resource)`.
    async fn assert_links_main(&self, link: &str, resource: &URI) {
        let lines: Vec<&str> = link.split("\r\n").collect();
        let last_line = lines.last().expect("há ao menos uma linha");
        // Count lines, accounting for wrapping
        let line_count: i32 = lines.iter().map(|line| ((line.encode_utf16().count() as f64 / 80.0).ceil() as i32).max(1)).sum();
        let last_length = last_line.encode_utf16().count() as i32;
        assert_link_helper(
            &self.xterm,
            self.detector.as_ref(),
            link,
            &[(resource.to_string(false).to_string_lossy(), [(1, line_count), (last_length, line_count)])],
            TerminalBuiltinLinkType::LocalFile,
        )
        .await;
    }
}

/// `TerminalLinkProcessManager` de cada suíte.
fn process_manager(initial_cwd: &str, os: OperatingSystem, user_home: &str) -> TerminalLinkProcessManager {
    TerminalLinkProcessManager { initial_cwd: initial_cwd.to_owned(), os: Some(os), remote_authority: None, user_home: Some(user_home.to_owned()), backend: None }
}

/// O corpo de `test('should detect in "<formattedLink>"')`.
fn detect_in_format(links: &[(&str, Option<&str>)], link_index: usize, format_index: usize, process_manager: TerminalLinkProcessManager) {
    let (base_link, resource) = links[link_index];
    let resource = uri_file(resource.unwrap_or(base_link));
    let formats = supported_link_formats();
    let link_format = &formats[format_index];
    let formatted_link = format_link(&link_format.url_format, base_link, link_format.line, link_format.column);
    run_with_faked_timers(async move {
        let f = Fixture::new(process_manager);
        f.set_valid_resources(vec![resource.clone()]);
        f.assert_links_main(&formatted_link, &resource).await;
    });
}

fn linux_process_manager() -> TerminalLinkProcessManager {
    process_manager("/parent/cwd", OperatingSystem::Linux, "/home")
}

// ---------------------------------------------------------------------------------------------------------------------
// suite('macOS/Linux')
// ---------------------------------------------------------------------------------------------------------------------

/// `suite('Link: <baseLink>')`: um módulo por link, com um `#[test]` por formato.
macro_rules! unix_link_suite {
    ($module:ident, $index:expr) => {
        mod $module {
            use super::*;

            #[test]
            fn format_0() {
                detect_in_format(&UNIX_LINKS, $index, 0, linux_process_manager());
            }

            #[test]
            fn format_1() {
                detect_in_format(&UNIX_LINKS, $index, 1, linux_process_manager());
            }

            #[test]
            fn format_2() {
                detect_in_format(&UNIX_LINKS, $index, 2, linux_process_manager());
            }

            #[test]
            fn format_3() {
                detect_in_format(&UNIX_LINKS, $index, 3, linux_process_manager());
            }

            #[test]
            fn format_4() {
                detect_in_format(&UNIX_LINKS, $index, 4, linux_process_manager());
            }

            #[test]
            fn format_5() {
                detect_in_format(&UNIX_LINKS, $index, 5, linux_process_manager());
            }

            #[test]
            fn format_6() {
                detect_in_format(&UNIX_LINKS, $index, 6, linux_process_manager());
            }

            #[test]
            fn format_7() {
                detect_in_format(&UNIX_LINKS, $index, 7, linux_process_manager());
            }
        }
    };
}

unix_link_suite!(macos_linux_link_foo, 0);
unix_link_suite!(macos_linux_link_foo_bar, 1);
unix_link_suite!(macos_linux_link_foo_bracket_bar, 2);
unix_link_suite!(macos_linux_link_foo_bracket_bar_baz, 3);
unix_link_suite!(macos_linux_link_foo_bracket_bar_slash_baz, 4);
unix_link_suite!(macos_linux_link_foo_bar_plus_more, 5);
unix_link_suite!(macos_linux_link_home_foo, 6);
unix_link_suite!(macos_linux_link_relative_foo, 7);
unix_link_suite!(macos_linux_link_relative_dollar_foo, 8);
unix_link_suite!(macos_linux_link_parent_foo, 9);
unix_link_suite!(macos_linux_link_foo_slash_bar, 10);
unix_link_suite!(macos_linux_link_foo_slash_bar_plus_more, 11);

// ---------------------------------------------------------------------------------------------------------------------
// if (isWindows) { suite('Windows') }
// ---------------------------------------------------------------------------------------------------------------------

#[test]
fn windows_links() {
    if !Platform::detect().is_windows {
        return;
    }
    for link_index in 0..WINDOWS_LINKS.len() {
        for format_index in 0..supported_link_formats().len() {
            detect_in_format(&WINDOWS_LINKS, link_index, format_index, process_manager("C:\\Parent\\Cwd", OperatingSystem::Windows, "C:\\Home"));
        }
    }
}
