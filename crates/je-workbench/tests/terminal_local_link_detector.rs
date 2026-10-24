//! Porte de `src/vs/workbench/contrib/terminalContrib/links/test/browser/terminalLocalLinkDetector.test.ts` (suíte `Workbench - TerminalLocalLinkDetector`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - **`setup` e `ensureNoDisposablesAreLeakedInTestSuite()`.** O [`Fixture`] cria o guarda de vazamentos antes de tudo e traz os
//!   dublês de `tests/support/terminal_link_support.rs` (o `TestFileService`), o `FakeXterm` (80 colunas por 30 linhas) no lugar do
//!   `Terminal` do xterm, o `UriIdentityService` real, o `TestContextService` e o `TerminalLinkResolver` real. O `setup` de cada suíte
//!   interna é a função `*_detector` chamada no começo dos casos dela.
//! - **`assertLinkHelper`.** Escreve o texto no `FakeXterm` (`xterm.reset()` e `xterm.write(text)`, síncrono), monta as linhas até a
//!   do cursor, detecta na última linha com conteúdo e compara `{ link, type, bufferRange }` com o esperado.
//! - **`assertLinks` e a corrida contra `timeout(2)`.** O original corre a asserção contra um `timeout(2)` real. Os casos rodam em
//!   `run_with_faked_timers`, em que o relógio é virtual e a asserção termina antes de o `timeout` valer; a corrida não existe aqui.
//! - **`for (const l of unixLinks) { suite(...) { for (formats) test(...) } }`.** Cada link é um `#[test]` que percorre os 35 formatos
//!   em `supportedLinkFormats`, com o título do `test` original na mensagem da asserção. O mesmo vale para os de Windows.
//! - **`if (isWindows)`.** `isWindows` é a plataforma do hospedeiro (`Platform::detect().is_windows`): os casos do bloco existem como
//!   `#[test]` e saem no começo quando a plataforma de execução não é o Windows (o original nem os registra).
//! - **`format(urlFormat, baseLink, line, column)`.** É [`format_link`], a substituição de `{0}`, `{1}` e `{2}`.
//! - **`URI.file(...)`.** É [`uri_file`] (na plataforma do hospedeiro).
//! - **`backend: undefined` e `backend: { getWslPath }`.** É o `Option` do `IWslPathBackend`; o mapa `wslUnixToWindowsPathMap` é um
//!   `RefCell<HashMap>` dentro do dublê.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod support;

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use common::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite, run_with_faked_timers};
use je_base::platform::{OperatingSystem, Platform};
use je_base::promise::Promise;
use je_base::uri::URI;
use je_instantiation::Injectable;
use je_platform::log::NullLogService;
use je_platform::files::i_file_service::IFileService;
use je_platform::resources::uri_file;
use je_platform::terminal::capabilities::terminal_capability_store::TerminalCapabilityStore;
use je_platform::uri_identity::uri_identity_service::UriIdentityService;
use je_workbench::contrib::terminal_contrib::links::links::{
    IWslPathBackend, TerminalBuiltinLinkType, TerminalLinkProcessManager, WslPathDirection,
};
use je_workbench::contrib::terminal_contrib::links::terminal_link_resolver::TerminalLinkResolver;
use je_workbench::contrib::terminal_contrib::links::terminal_local_link_detector::TerminalLocalLinkDetector;
use je_workbench::test::common::workbench_test_services::TestContextService;
use support::fake_xterm::FakeXterm;
use support::terminal_link_support::{LinkTestFileService, assert_link_helper};

/// `unixLinks`: `string | { link, resource }`; `None` é o `string` (o recurso é o `URI.file` do próprio texto).
const UNIX_LINKS: [(&str, Option<&str>); 15] = [
    // Absolute
    ("/foo", None),
    ("/foo/bar", None),
    ("/foo/[bar]", None),
    ("/foo/[bar].baz", None),
    ("/foo/[bar]/baz", None),
    ("/foo/bar+more", None),
    // URI file://
    ("file:///foo", Some("/foo")),
    ("file:///foo/bar", Some("/foo/bar")),
    ("file:///foo/bar%20baz", Some("/foo/bar baz")),
    // User home
    ("~/foo", Some("/home/foo")),
    // Relative
    ("./foo", Some("/parent/cwd/foo")),
    ("./$foo", Some("/parent/cwd/$foo")),
    ("../foo", Some("/parent/foo")),
    ("foo/bar", Some("/parent/cwd/foo/bar")),
    ("foo/bar+more", Some("/parent/cwd/foo/bar+more")),
];

/// `unixLinksWithIso`.
const UNIX_LINKS_WITH_ISO: [(&str, &str); 1] = [
    // ISO 8601 timestamps - tested separately to avoid line/column suffix conflicts
    ("./test-2025-04-28T11:03:09+02:00.log", "/parent/cwd/test-2025-04-28T11:03:09+02:00.log"),
];

/// `windowsLinks`.
const WINDOWS_LINKS: [(&str, Option<&str>); 25] = [
    // Absolute
    ("c:\\foo", None),
    ("\\\\?\\C:\\foo", Some("C:\\foo")),
    ("c:/foo", None),
    ("c:/foo/bar", None),
    ("c:\\foo\\bar", None),
    ("c:\\foo\\bar+more", None),
    ("c:\\foo/bar\\baz", None),
    // URI file://
    ("file:///c:/foo", Some("c:\\foo")),
    ("file:///c:/foo/bar", Some("c:\\foo\\bar")),
    ("file:///c:/foo/bar%20baz", Some("c:\\foo\\bar baz")),
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

/// `windowsLinksWithIso`.
const WINDOWS_LINKS_WITH_ISO: [(&str, &str); 1] = [
    // ISO 8601 timestamps - tested separately to avoid line/column suffix conflicts
    (".\\test-2025-04-28T11:03:09+02:00.log", "C:\\Parent\\Cwd\\test-2025-04-28T11:03:09+02:00.log"),
];

/// `LinkFormatInfo`.
struct LinkFormatInfo {
    url_format: &'static str,
    /// The start offset to the buffer range that is not in the actual link (but is in the matched
    /// area.
    link_cell_start_offset: Option<i32>,
    /// The end offset to the buffer range that is not in the actual link (but is in the matched
    /// area.
    link_cell_end_offset: Option<i32>,
    line: Option<&'static str>,
    column: Option<&'static str>,
}

const fn format_info(url_format: &'static str, line: Option<&'static str>, column: Option<&'static str>) -> LinkFormatInfo {
    LinkFormatInfo { url_format, link_cell_start_offset: None, link_cell_end_offset: None, line, column }
}

/// `supportedLinkFormats`.
const SUPPORTED_LINK_FORMATS: [LinkFormatInfo; 35] = [
    format_info("{0}", None, None),
    format_info("{0}\" on line {1}", Some("5"), None),
    format_info("{0}\" on line {1}, column {2}", Some("5"), Some("3")),
    format_info("{0}\":line {1}", Some("5"), None),
    format_info("{0}\":line {1}, column {2}", Some("5"), Some("3")),
    format_info("{0}\": line {1}", Some("5"), None),
    format_info("{0}\": line {1}, col {2}", Some("5"), Some("3")),
    format_info("{0}({1})", Some("5"), None),
    format_info("{0} ({1})", Some("5"), None),
    format_info("{0}, {1}", Some("5"), None),
    format_info("{0}({1},{2})", Some("5"), Some("3")),
    format_info("{0} ({1},{2})", Some("5"), Some("3")),
    format_info("{0}: ({1},{2})", Some("5"), Some("3")),
    format_info("{0}({1}, {2})", Some("5"), Some("3")),
    format_info("{0} ({1}, {2})", Some("5"), Some("3")),
    format_info("{0}: ({1}, {2})", Some("5"), Some("3")),
    format_info("{0}({1}:{2})", Some("5"), Some("3")),
    format_info("{0} ({1}:{2})", Some("5"), Some("3")),
    format_info("{0}:{1}", Some("5"), None),
    format_info("{0}:{1}:{2}", Some("5"), Some("3")),
    format_info("{0} {1}:{2}", Some("5"), Some("3")),
    format_info("{0}[{1}]", Some("5"), None),
    format_info("{0} [{1}]", Some("5"), None),
    format_info("{0}[{1},{2}]", Some("5"), Some("3")),
    format_info("{0} [{1},{2}]", Some("5"), Some("3")),
    format_info("{0}: [{1},{2}]", Some("5"), Some("3")),
    format_info("{0}[{1}, {2}]", Some("5"), Some("3")),
    format_info("{0} [{1}, {2}]", Some("5"), Some("3")),
    format_info("{0}: [{1}, {2}]", Some("5"), Some("3")),
    format_info("{0}[{1}:{2}]", Some("5"), Some("3")),
    format_info("{0} [{1}:{2}]", Some("5"), Some("3")),
    format_info("{0}\",{1}", Some("5"), None),
    format_info("{0}',{1}", Some("5"), None),
    format_info("{0}#{1}", Some("5"), None),
    format_info("{0}#{1}:{2}", Some("5"), Some("5")),
];

/// `windowsFallbackLinks`.
const WINDOWS_FALLBACK_LINKS: [&str; 4] = ["C:\\foo bar", "C:\\foo bar\\baz", "C:\\foo\\bar baz", "C:\\foo/bar baz"];

const fn fallback_format(url_format: &'static str, start: Option<i32>, end: Option<i32>, line: Option<&'static str>, column: Option<&'static str>) -> LinkFormatInfo {
    LinkFormatInfo { url_format, link_cell_start_offset: start, link_cell_end_offset: end, line, column }
}

/// `supportedFallbackLinkFormats`.
const SUPPORTED_FALLBACK_LINK_FORMATS: [LinkFormatInfo; 18] = [
    // Python style error: File "<path>", line <line>
    fallback_format("File \"{0}\"", Some(5), None, None, None),
    fallback_format("File \"{0}\", line {1}", Some(5), None, Some("5"), None),
    // Unknown tool #200166: FILE  <path>:<line>:<col>
    fallback_format(" FILE  {0}", Some(7), None, None, None),
    fallback_format(" FILE  {0}:{1}", Some(7), None, Some("5"), None),
    fallback_format(" FILE  {0}:{1}:{2}", Some(7), None, Some("5"), Some("3")),
    // Some C++ compile error formats
    fallback_format("{0}({1}) :", None, Some(-2), Some("5"), None),
    fallback_format("{0}({1},{2}) :", None, Some(-2), Some("5"), Some("3")),
    fallback_format("{0}({1}, {2}) :", None, Some(-2), Some("5"), Some("3")),
    fallback_format("{0}({1}):", None, Some(-1), Some("5"), None),
    fallback_format("{0}({1},{2}):", None, Some(-1), Some("5"), Some("3")),
    fallback_format("{0}({1}, {2}):", None, Some(-1), Some("5"), Some("3")),
    fallback_format("{0}:{1} :", None, Some(-2), Some("5"), None),
    fallback_format("{0}:{1}:{2} :", None, Some(-2), Some("5"), Some("3")),
    fallback_format("{0}:{1}:", None, Some(-1), Some("5"), None),
    fallback_format("{0}:{1}:{2}:", None, Some(-1), Some("5"), Some("3")),
    // PowerShell prompt
    fallback_format("PS {0}>", Some(3), Some(-1), None, None),
    // Cmd prompt
    fallback_format("{0}>", None, Some(-1), None, None),
    // The whole line is the path
    fallback_format("{0}", None, None, None, None),
];

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

fn is_windows() -> bool {
    Platform::detect().is_windows
}

/// O `backend: { async getWslPath(original, direction) { ... } }` do bloco `Windows`.
struct TestWslBackend {
    wsl_unix_to_windows_path_map: RefCell<HashMap<String, String>>,
}

impl IWslPathBackend for TestWslBackend {
    fn get_wsl_path(&self, original: &str, direction: WslPathDirection) -> Promise<String> {
        if direction == WslPathDirection::UnixToWin {
            return Promise::resolved(self.wsl_unix_to_windows_path_map.borrow().get(original).cloned().unwrap_or_else(|| original.to_owned()));
        }
        Promise::resolved(original.to_owned())
    }
}

/// O `setup` da suíte.
struct Fixture {
    file_service: Rc<LinkTestFileService>,
    xterm: Rc<FakeXterm>,
    detector: Rc<TerminalLocalLinkDetector>,
    // Declarado por último: o `Drop` dele descarta o que o `Fixture` registrou e confere os vazamentos.
    _guard: LeakGuard,
}

/// `TerminalLinkProcessManager` de cada suíte.
fn process_manager(initial_cwd: &str, os: OperatingSystem, user_home: &str, backend: Option<Rc<dyn IWslPathBackend>>) -> TerminalLinkProcessManager {
    TerminalLinkProcessManager {
        initial_cwd: initial_cwd.to_owned(),
        os: Some(os),
        remote_authority: None,
        user_home: Some(user_home.to_owned()),
        backend,
    }
}

impl Fixture {
    fn new(process_manager: TerminalLinkProcessManager) -> Self {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let file_service = LinkTestFileService::new();
        let uri_identity_service = guard.add_rc(UriIdentityService::create((), (file_service.clone() as Rc<dyn IFileService>,)));
        let workspace_context_service = TestContextService::new(None, None);
        let resolver = Rc::new(TerminalLinkResolver::new(file_service.clone()));
        let xterm = FakeXterm::new(80, 30);
        let capabilities = guard.add_rc(TerminalCapabilityStore::new());
        let detector = Rc::new(TerminalLocalLinkDetector::new(
            xterm.clone(),
            capabilities,
            process_manager,
            resolver,
            NullLogService::shared(),
            uri_identity_service,
            workspace_context_service,
        ));
        Self { file_service, xterm, detector, _guard: guard }
    }

    /// `validResources = resources; fileService.setFiles(validResources)`.
    fn set_valid_resources(&self, resources: Vec<URI>) {
        self.file_service.set_files(resources);
    }

    /// `assertLinks(type, text, expected)` com o `assertLinkHelper`.
    async fn assert_links(&self, type_: TerminalBuiltinLinkType, text: &str, expected: &[(String, [(i32, i32); 2])]) {
        assert_link_helper(&self.xterm, self.detector.as_ref(), text, expected, type_).await;
    }

    /// `assertLinks(LocalFile, text, [{ uri, range }])`.
    async fn assert_local_file(&self, text: &str, uri: &URI, range: [(i32, i32); 2]) {
        self.assert_links(TerminalBuiltinLinkType::LocalFile, text, &[(uri.to_string(false).to_string_lossy(), range)]).await;
    }

    /// `assertLinksWithWrapped(link, resource?)`.
    async fn assert_links_with_wrapped(&self, link: &str, resource: Option<&URI>) {
        let uri = resource.cloned().unwrap_or_else(|| uri_file(link));
        let length = link.encode_utf16().count() as i32;
        self.assert_local_file(link, &uri, [(1, 1), (length, 1)]).await;
        self.assert_local_file(&format!(" {link} "), &uri, [(2, 1), (length + 1, 1)]).await;
        self.assert_local_file(&format!("({link})"), &uri, [(2, 1), (length + 1, 1)]).await;
        self.assert_local_file(&format!("[{link}]"), &uri, [(2, 1), (length + 1, 1)]).await;
    }
}

/// O detector de `platform independent` e de `macOS/Linux`.
fn linux_fixture() -> Fixture {
    Fixture::new(process_manager("/parent/cwd", OperatingSystem::Linux, "/home", None))
}

/// O detector de `Windows`.
fn windows_fixture(backend: Rc<TestWslBackend>) -> Fixture {
    Fixture::new(process_manager("C:\\Parent\\Cwd", OperatingSystem::Windows, "C:\\Home", Some(backend)))
}

// ---------------------------------------------------------------------------------------------------------------------
// suite('platform independent')
// ---------------------------------------------------------------------------------------------------------------------

#[test]
fn should_support_multiple_link_results() {
    run_with_faked_timers(async {
        let f = linux_fixture();
        let valid_resources = vec![uri_file("/parent/cwd/foo"), uri_file("/parent/cwd/bar")];
        f.set_valid_resources(valid_resources);
        f.assert_links(
            TerminalBuiltinLinkType::LocalFile,
            "./foo ./bar",
            &[
                (uri_file("/parent/cwd/foo").to_string(false).to_string_lossy(), [(1, 1), (5, 1)]),
                (uri_file("/parent/cwd/bar").to_string(false).to_string_lossy(), [(7, 1), (11, 1)]),
            ],
        )
        .await;
    });
}

#[test]
fn should_support_trimming_extra_quotes() {
    run_with_faked_timers(async {
        let f = linux_fixture();
        f.set_valid_resources(vec![uri_file("/parent/cwd/foo")]);
        f.assert_local_file("\"foo\"\" on line 5", &uri_file("/parent/cwd/foo"), [(1, 1), (16, 1)]).await;
    });
}

#[test]
fn should_support_trimming_extra_square_brackets() {
    run_with_faked_timers(async {
        let f = linux_fixture();
        f.set_valid_resources(vec![uri_file("/parent/cwd/foo")]);
        f.assert_local_file("\"foo]\" on line 5", &uri_file("/parent/cwd/foo"), [(1, 1), (16, 1)]).await;
    });
}

#[test]
fn should_support_finding_links_after_brackets() {
    run_with_faked_timers(async {
        let f = linux_fixture();
        f.set_valid_resources(vec![uri_file("/parent/cwd/foo")]);
        f.assert_local_file("bar[foo:5", &uri_file("/parent/cwd/foo"), [(5, 1), (9, 1)]).await;
    });
}

// ---------------------------------------------------------------------------------------------------------------------
// suite('macOS/Linux')
// ---------------------------------------------------------------------------------------------------------------------

/// `suite('Link: <baseLink>') { for (formats) test('should detect in "<formattedLink>"') }`.
fn unix_link_suite(index: usize) {
    run_with_faked_timers(async move {
        let f = linux_fixture();
        let (base_link, resource) = UNIX_LINKS[index];
        let resource = uri_file(resource.unwrap_or(base_link));
        for link_format in &SUPPORTED_LINK_FORMATS {
            let formatted_link = format_link(link_format.url_format, base_link, link_format.line, link_format.column);
            f.set_valid_resources(vec![resource.clone()]);
            f.assert_links_with_wrapped(&formatted_link, Some(&resource)).await;
        }
    });
}

#[test]
fn macos_linux_link_foo() {
    unix_link_suite(0);
}

#[test]
fn macos_linux_link_foo_bar() {
    unix_link_suite(1);
}

#[test]
fn macos_linux_link_foo_bracket_bar() {
    unix_link_suite(2);
}

#[test]
fn macos_linux_link_foo_bracket_bar_baz() {
    unix_link_suite(3);
}

#[test]
fn macos_linux_link_foo_bracket_bar_slash_baz() {
    unix_link_suite(4);
}

#[test]
fn macos_linux_link_foo_bar_plus_more() {
    unix_link_suite(5);
}

#[test]
fn macos_linux_link_file_foo() {
    unix_link_suite(6);
}

#[test]
fn macos_linux_link_file_foo_bar() {
    unix_link_suite(7);
}

#[test]
fn macos_linux_link_file_foo_bar_baz() {
    unix_link_suite(8);
}

#[test]
fn macos_linux_link_home_foo() {
    unix_link_suite(9);
}

#[test]
fn macos_linux_link_relative_foo() {
    unix_link_suite(10);
}

#[test]
fn macos_linux_link_relative_dollar_foo() {
    unix_link_suite(11);
}

#[test]
fn macos_linux_link_parent_foo() {
    unix_link_suite(12);
}

#[test]
fn macos_linux_link_foo_slash_bar() {
    unix_link_suite(13);
}

#[test]
fn macos_linux_link_foo_slash_bar_plus_more() {
    unix_link_suite(14);
}

#[test]
fn macos_linux_git_diff_links() {
    run_with_faked_timers(async {
        let f = linux_fixture();
        let resource = uri_file("/parent/cwd/foo/bar");
        f.set_valid_resources(vec![resource.clone()]);
        f.assert_links(
            TerminalBuiltinLinkType::LocalFile,
            "diff --git a/foo/bar b/foo/bar",
            &[
                (resource.to_string(false).to_string_lossy(), [(14, 1), (20, 1)]),
                (resource.to_string(false).to_string_lossy(), [(24, 1), (30, 1)]),
            ],
        )
        .await;
        f.assert_local_file("--- a/foo/bar", &resource, [(7, 1), (13, 1)]).await;
        f.assert_local_file("+++ b/foo/bar", &resource, [(7, 1), (13, 1)]).await;
    });
}

// Test ISO 8601 links separately with only base format to avoid suffix conflicts
// Note: Only test plain format as colons are excluded path characters in the regex,
// so wrapped contexts (spaces, parentheses, brackets) won't work
#[test]
fn macos_linux_should_detect_iso_8601_link() {
    run_with_faked_timers(async {
        let f = linux_fixture();
        for (base_link, resource) in UNIX_LINKS_WITH_ISO {
            let resource = uri_file(resource);
            f.set_valid_resources(vec![resource.clone()]);
            let length = base_link.encode_utf16().count() as i32;
            f.assert_local_file(base_link, &resource, [(1, 1), (length, 1)]).await;
        }
    });
}

// ---------------------------------------------------------------------------------------------------------------------
// if (isWindows) { suite('Windows') }
// ---------------------------------------------------------------------------------------------------------------------

#[test]
fn windows_links() {
    if !is_windows() {
        return;
    }
    run_with_faked_timers(async {
        let f = windows_fixture(Rc::new(TestWslBackend { wsl_unix_to_windows_path_map: RefCell::default() }));
        for (base_link, resource) in WINDOWS_LINKS {
            let resource = uri_file(resource.unwrap_or(base_link));
            for link_format in &SUPPORTED_LINK_FORMATS {
                let formatted_link = format_link(link_format.url_format, base_link, link_format.line, link_format.column);
                f.set_valid_resources(vec![resource.clone()]);
                f.assert_links_with_wrapped(&formatted_link, Some(&resource)).await;
            }
        }
    });
}

#[test]
fn windows_fallback_links() {
    if !is_windows() {
        return;
    }
    run_with_faked_timers(async {
        let f = windows_fixture(Rc::new(TestWslBackend { wsl_unix_to_windows_path_map: RefCell::default() }));
        for base_link in WINDOWS_FALLBACK_LINKS {
            let resource = uri_file(base_link);
            for link_format in &SUPPORTED_FALLBACK_LINK_FORMATS {
                let formatted_link = format_link(link_format.url_format, base_link, link_format.line, link_format.column);
                let link_cell_start_offset = link_format.link_cell_start_offset.unwrap_or(0);
                let link_cell_end_offset = link_format.link_cell_end_offset.unwrap_or(0);
                f.set_valid_resources(vec![resource.clone()]);
                f.assert_local_file(
                    &formatted_link,
                    &resource,
                    [(1 + link_cell_start_offset, 1), (formatted_link.encode_utf16().count() as i32 + link_cell_end_offset, 1)],
                )
                .await;
            }
        }
    });
}

#[test]
fn windows_git_diff_links() {
    if !is_windows() {
        return;
    }
    run_with_faked_timers(async {
        let f = windows_fixture(Rc::new(TestWslBackend { wsl_unix_to_windows_path_map: RefCell::default() }));
        let resource = uri_file("C:\\Parent\\Cwd\\foo\\bar");
        f.set_valid_resources(vec![resource.clone()]);
        f.assert_links(
            TerminalBuiltinLinkType::LocalFile,
            "diff --git a/foo/bar b/foo/bar",
            &[
                (resource.to_string(false).to_string_lossy(), [(14, 1), (20, 1)]),
                (resource.to_string(false).to_string_lossy(), [(24, 1), (30, 1)]),
            ],
        )
        .await;
        f.assert_local_file("--- a/foo/bar", &resource, [(7, 1), (13, 1)]).await;
        f.assert_local_file("+++ b/foo/bar", &resource, [(7, 1), (13, 1)]).await;
    });
}

#[test]
fn windows_should_detect_iso_8601_link() {
    if !is_windows() {
        return;
    }
    run_with_faked_timers(async {
        let f = windows_fixture(Rc::new(TestWslBackend { wsl_unix_to_windows_path_map: RefCell::default() }));
        for (base_link, resource) in WINDOWS_LINKS_WITH_ISO {
            let resource = uri_file(resource);
            f.set_valid_resources(vec![resource.clone()]);
            let length = base_link.encode_utf16().count() as i32;
            f.assert_local_file(base_link, &resource, [(1, 1), (length, 1)]).await;
        }
    });
}

#[test]
fn windows_wsl_unix_to_windows_mnt_style_links() {
    if !is_windows() {
        return;
    }
    run_with_faked_timers(async {
        let backend = Rc::new(TestWslBackend { wsl_unix_to_windows_path_map: RefCell::default() });
        let f = windows_fixture(backend.clone());
        backend.wsl_unix_to_windows_path_map.borrow_mut().insert("/mnt/c/foo/bar".to_owned(), "C:\\foo\\bar".to_owned());
        let valid_resources = vec![uri_file("C:\\foo\\bar")];
        f.set_valid_resources(valid_resources.clone());
        f.assert_links_with_wrapped("/mnt/c/foo/bar", Some(&valid_resources[0])).await;
    });
}

#[test]
fn windows_wsl_windows_to_unix_wsl_dollar_style_links() {
    if !is_windows() {
        return;
    }
    run_with_faked_timers(async {
        let f = windows_fixture(Rc::new(TestWslBackend { wsl_unix_to_windows_path_map: RefCell::default() }));
        f.set_valid_resources(vec![uri_file("\\\\wsl$\\Debian\\home\\foo\\bar")]);
        f.assert_links_with_wrapped("\\\\wsl$\\Debian\\home\\foo\\bar", None).await;
    });
}

#[test]
fn windows_wsl_windows_to_unix_wsl_localhost_style_links() {
    if !is_windows() {
        return;
    }
    run_with_faked_timers(async {
        let f = windows_fixture(Rc::new(TestWslBackend { wsl_unix_to_windows_path_map: RefCell::default() }));
        f.set_valid_resources(vec![uri_file("\\\\wsl.localhost\\Debian\\home\\foo\\bar")]);
        f.assert_links_with_wrapped("\\\\wsl.localhost\\Debian\\home\\foo\\bar", None).await;
    });
}
