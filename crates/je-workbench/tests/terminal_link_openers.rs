//! Porte de `src/vs/workbench/contrib/terminalContrib/links/test/browser/terminalLinkOpeners.test.ts` (suíte `Workbench - TerminalLinkOpeners`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - **`setup` e `ensureNoDisposablesAreLeakedInTestSuite()`.** O [`Fixture`] cria o guarda de vazamentos antes de tudo e traz os
//!   dublês de serviço de `tests/support/terminal_link_support.rs` (lista de divergências lá), o `FakeXterm` no lugar do `Terminal`
//!   do xterm, o store de capacidades e a `TestCommandDetectionCapability` (a capacidade real com o `setCommands`, que no original é
//!   uma subclasse que expõe o `_commands` protegido e aqui é [`CommandDetectionCapability::set_commands`]). Os `setup` das suítes
//!   internas (`macOS/Linux` e `Windows`) são as funções `*_opener` chamadas no começo dos casos delas.
//! - **`createInstance(...)`.** Os abridores são construídos à mão com os serviços do [`Fixture`] (o `TestInstantiationService` não
//!   entra). O `QueryBuilder` real que o `TerminalSearchLinkOpener` cria no original vira o `StubFileQueryBuilder`, porque o
//!   `TestSearchService` ignora a consulta; o `opener.setFileQueryBuilder({ file: () => null! })` dos casos continua no lugar.
//! - **`TestSearchService`.** `fileSearch` sem resultado guardado rejeita com `TypeError` (o `this._searchResult!` indefinido), o que o
//!   `catch` de `_tryOpenExactLink` engole.
//! - **`new TerminalCommand(xterm, { ... marker: { line: 0 } as Partial<IMarker> as any ... })`.** O marcador é um `FakeMarker` na
//!   linha 0.
//! - **`URI.from({ scheme: Schemas.file, path })` e `URI.file(path)`.** São [`uri_from_path`] e [`uri_file`].
//! - **`deepStrictEqual(activationResult, { link, source, selection })`.** Compara [`TerminalLinkActivationResult`]: o campo
//!   `selection` só existe nos casos do original que o põem (`selection: undefined` é `None`).
//! - **Casos assíncronos.** Rodam em `run_with_faked_timers`; `await opener.open(...)` é `.await.unwrap()`.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod support;

use std::rc::Rc;

use common::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite, run_with_faked_timers};
use je_base::platform::OperatingSystem;
use je_base::uri::{URI, UriComponents};
use je_base::uuid::generate_uuid;
use je_platform::log::NullLogService;
use je_platform::opener::editor_types::TextEditorSelection;
use je_platform::resources::uri_file;
use je_platform::terminal::capabilities::capabilities::{CommandDetection, IMarkProperties, IMarker, TerminalCapability, TerminalCommandConfidence};
use je_platform::terminal::capabilities::command_detection::terminal_command::{ITerminalCommandProperties, TerminalCommand};
use je_platform::terminal::capabilities::command_detection_capability::CommandDetectionCapability;
use je_platform::terminal::capabilities::terminal_capability_store::TerminalCapabilityStore;
use je_platform::terminal::xterm_headless::IHeadlessTerminal;
use je_workbench::contrib::terminal::xterm_buffer::{IBufferCellPosition, IBufferRange};
use je_workbench::contrib::terminal_contrib::links::links::{ITerminalLinkOpener, ITerminalSimpleLink, TerminalBuiltinLinkType};
use je_workbench::contrib::terminal_contrib::links::terminal_link_openers::{
    IFileQueryBuilder, TerminalLocalFileLinkOpener, TerminalLocalFolderInWorkspaceLinkOpener, TerminalSearchLinkOpener,
};
use je_workbench::services::search::search::{IFileMatch, ISearchComplete};
use je_workbench::test::browser::workbench_test_services::TestEnvironmentService;
use je_workbench::test::common::workbench_test_services::TestContextService;
use support::fake_xterm::{FakeMarker, FakeXterm};
use support::mock_command_service::MockCommandService;
use support::terminal_link_support::{
    ActivationResult, LinkTestFileService, RecordingEditorService, RecordingQuickAccess, StubFileQueryBuilder, TerminalLinkActivationResult, TestSearchService,
};

/// O `setup` da suíte e o da suíte `TerminalSearchLinkOpener`.
struct Fixture {
    file_service: Rc<LinkTestFileService>,
    search_service: Rc<TestSearchService>,
    activation_result: ActivationResult,
    xterm: Rc<FakeXterm>,
    capabilities: Rc<TerminalCapabilityStore>,
    command_detection: Rc<CommandDetectionCapability>,
    local_file_opener: Rc<TerminalLocalFileLinkOpener>,
    local_folder_opener: Rc<TerminalLocalFolderInWorkspaceLinkOpener>,
    // Declarado por último: o `Drop` dele descarta o que o `Fixture` registrou e confere os vazamentos.
    _guard: LeakGuard,
}

impl Fixture {
    fn new() -> Self {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let file_service = LinkTestFileService::new();
        let search_service = TestSearchService::new();
        // Allow intercepting link activations
        let activation_result: ActivationResult = ActivationResult::default();
        let editor_service = Rc::new(RecordingEditorService { activation_result: activation_result.clone() });
        let xterm = FakeXterm::new(80, 24);

        // suite('TerminalSearchLinkOpener') > setup
        let capabilities = guard.add_rc(TerminalCapabilityStore::new());
        let terminal: Rc<dyn IHeadlessTerminal> = xterm.clone();
        let command_detection = guard.add_rc(CommandDetectionCapability::new(terminal, NullLogService::shared()));
        capabilities.add::<CommandDetection>(command_detection.clone());
        Self {
            file_service,
            search_service,
            activation_result,
            xterm,
            capabilities,
            command_detection,
            local_file_opener: Rc::new(TerminalLocalFileLinkOpener::new(editor_service)),
            local_folder_opener: Rc::new(TerminalLocalFolderInWorkspaceLinkOpener::new(Rc::new(MockCommandService::default()))),
            _guard: guard,
        }
    }

    /// `instantiationService.createInstance(TestTerminalSearchLinkOpener, capabilities, initialCwd, localFileOpener,
    /// localFolderOpener, () => os)`.
    fn opener(&self, initial_cwd: &str, os: OperatingSystem) -> TerminalSearchLinkOpener {
        TerminalSearchLinkOpener::new(
            self.capabilities.clone(),
            initial_cwd,
            self.local_file_opener.clone(),
            self.local_folder_opener.clone(),
            Rc::new(move || os),
            self.file_service.clone(),
            Rc::new(StubFileQueryBuilder),
            Rc::new(RecordingQuickAccess { activation_result: self.activation_result.clone() }),
            self.search_service.clone(),
            NullLogService::shared(),
            TestEnvironmentService::new(),
            TestContextService::new(None, None),
        )
    }

    /// `commandDetection.setCommands([new TerminalCommand(xterm, { ... marker: { line: 0 } ... })])`: um comando falso que começa
    /// na linha 0 para estabelecer o cwd.
    fn set_command_at_line_0(&self, cwd: &str) {
        let terminal: Rc<dyn IHeadlessTerminal> = self.xterm.clone();
        let marker: Rc<dyn IMarker> = FakeMarker::new(1.0, 0.0);
        self.command_detection.set_commands(vec![TerminalCommand::new(
            terminal,
            ITerminalCommandProperties {
                command: String::new(),
                command_line_confidence: TerminalCommandConfidence::Low,
                is_trusted: true,
                timestamp: 0.0,
                duration: 0.0,
                id: Some(generate_uuid()),
                marker: Some(marker),
                cwd: Some(cwd.to_owned()),
                exit_code: Some(0.0),
                command_start_line_content: Some(String::new()),
                mark_properties: Some(IMarkProperties::default()),
                executed_x: None,
                start_x: None,
                prompt_start_marker: None,
                end_marker: None,
                executed_marker: None,
                aliases: None,
                was_replayed: None,
            },
        )]);
    }

    /// `activationResult`.
    fn activation(&self) -> Option<TerminalLinkActivationResult> {
        self.activation_result.borrow().clone()
    }
}

fn uri_from_path(path: &str) -> URI {
    URI::from(&UriComponents { scheme: Some("file".into()), path: Some(path.into()), ..UriComponents::default() }, false)
}

/// `{ text, bufferRange: { start: { x: 1, y: 1 }, end: { x: end_x, y: 1 } }, type }`.
fn link(text: &str, end_x: i32, type_: TerminalBuiltinLinkType) -> ITerminalSimpleLink {
    ITerminalSimpleLink::new(text, IBufferRange { start: IBufferCellPosition { x: 1, y: 1 }, end: IBufferCellPosition { x: end_x, y: 1 } }, type_)
}

/// `{ text, bufferRange: { start: { x: 1, y: 1 }, end: { x: 8, y: 1 } }, type: Search }`.
fn search_link(text: &str) -> ITerminalSimpleLink {
    link(text, 8, TerminalBuiltinLinkType::Search)
}

/// `{ link, source: 'editor' }`.
fn editor(link: &str) -> Option<TerminalLinkActivationResult> {
    Some(TerminalLinkActivationResult { source: "editor", link: link.to_owned(), selection: None })
}

/// `{ link, source: 'editor', selection: { startColumn, startLineNumber, endColumn: undefined, endLineNumber: undefined } }`.
fn editor_at(link: &str, start_line_number: f64, start_column: f64) -> Option<TerminalLinkActivationResult> {
    Some(TerminalLinkActivationResult {
        source: "editor",
        link: link.to_owned(),
        selection: Some(TextEditorSelection { start_line_number, start_column, end_line_number: None, end_column: None }),
    })
}

/// `{ link, source: 'search' }`.
fn search(link: &str) -> Option<TerminalLinkActivationResult> {
    Some(TerminalLinkActivationResult { source: "search", link: link.to_owned(), selection: None })
}

/// `searchService.setSearchResult({ messages: [], results })`.
fn search_complete(resources: Vec<URI>) -> ISearchComplete {
    ISearchComplete {
        limit_hit: None,
        messages: Vec::new(),
        stats: None,
        results: resources.into_iter().map(|resource| IFileMatch::new(resource, None)).collect(),
        exit: None,
        ai_keywords: None,
    }
}

/// `await opener.open(link)`.
async fn open(opener: &TerminalSearchLinkOpener, link: ITerminalSimpleLink) {
    opener.open(&link).await.unwrap();
}

// ---------------------------------------------------------------------------------------------------------------------
// suite('TerminalSearchLinkOpener')
// ---------------------------------------------------------------------------------------------------------------------

#[test]
fn should_open_single_exact_match_against_cwd_when_searching_if_it_exists_when_command_detection_cwd_is_available() {
    run_with_faked_timers(async {
        let f = Fixture::new();
        let opener = f.opener("/initial/cwd", OperatingSystem::Linux);
        // Set a fake detected command starting as line 0 to establish the cwd
        f.set_command_at_line_0("/initial/cwd");
        f.file_service.set_files(vec![uri_from_path("/initial/cwd/foo/bar.txt"), uri_from_path("/initial/cwd/foo2/bar.txt")]);
        open(&opener, search_link("foo/bar.txt")).await;
        assert_eq!(f.activation(), editor("file:///initial/cwd/foo/bar.txt"));
    });
}

#[test]
fn should_open_single_exact_match_against_cwd_for_paths_containing_a_separator_when_searching_if_it_exists_even_when_command_detection_isnt_available() {
    run_with_faked_timers(async {
        let f = Fixture::new();
        let opener = f.opener("/initial/cwd", OperatingSystem::Linux);
        f.file_service.set_files(vec![uri_from_path("/initial/cwd/foo/bar.txt"), uri_from_path("/initial/cwd/foo2/bar.txt")]);
        open(&opener, search_link("foo/bar.txt")).await;
        assert_eq!(f.activation(), editor("file:///initial/cwd/foo/bar.txt"));
    });
}

#[test]
fn should_open_single_exact_match_against_any_folder_for_paths_not_containing_a_separator_when_there_is_a_single_search_result_even_when_command_detection_isnt_available() {
    run_with_faked_timers(async {
        let f = Fixture::new();
        let opener = f.opener("/initial/cwd", OperatingSystem::Linux);
        f.capabilities.remove(TerminalCapability::CommandDetection);
        opener.set_file_query_builder(Rc::new(StubFileQueryBuilder) as Rc<dyn IFileQueryBuilder>);
        f.file_service.set_files(vec![uri_from_path("/initial/cwd/foo/bar.txt"), uri_from_path("/initial/cwd/foo2/baz.txt")]);
        f.search_service.set_search_result(search_complete(vec![uri_from_path("/initial/cwd/foo/bar.txt")]));
        open(&opener, search_link("bar.txt")).await;
        assert_eq!(f.activation(), editor("file:///initial/cwd/foo/bar.txt"));
    });
}

#[test]
fn should_open_single_exact_match_against_any_folder_for_paths_not_containing_a_separator_when_there_are_multiple_search_results_even_when_command_detection_isnt_available() {
    run_with_faked_timers(async {
        let f = Fixture::new();
        let opener = f.opener("/initial/cwd", OperatingSystem::Linux);
        f.capabilities.remove(TerminalCapability::CommandDetection);
        opener.set_file_query_builder(Rc::new(StubFileQueryBuilder) as Rc<dyn IFileQueryBuilder>);
        f.file_service.set_files(vec![
            uri_from_path("/initial/cwd/foo/bar.txt"),
            uri_from_path("/initial/cwd/foo/bar.test.txt"),
            uri_from_path("/initial/cwd/foo2/bar.test.txt"),
        ]);
        f.search_service.set_search_result(search_complete(vec![
            uri_from_path("/initial/cwd/foo/bar.txt"),
            uri_from_path("/initial/cwd/foo/bar.test.txt"),
            uri_from_path("/initial/cwd/foo2/bar.test.txt"),
        ]));
        open(&opener, search_link("bar.txt")).await;
        assert_eq!(f.activation(), editor("file:///initial/cwd/foo/bar.txt"));
    });
}

#[test]
fn should_not_open_single_exact_match_for_paths_not_containing_a_when_command_detection_isnt_available() {
    run_with_faked_timers(async {
        let f = Fixture::new();
        let opener = f.opener("/initial/cwd", OperatingSystem::Linux);
        f.file_service.set_files(vec![uri_from_path("/initial/cwd/foo/bar.txt"), uri_from_path("/initial/cwd/foo2/bar.txt")]);
        open(&opener, search_link("bar.txt")).await;
        assert_eq!(f.activation(), search("bar.txt"));
    });
}

// suite('TerminalSearchLinkOpener') > suite('macOS/Linux')

/// `setup` de `macOS/Linux`.
fn linux_opener(f: &Fixture, initial_cwd: &str) -> TerminalSearchLinkOpener {
    f.opener(initial_cwd, OperatingSystem::Linux)
}

#[test]
fn macos_linux_should_apply_the_cwd_to_the_link_only_when_the_file_exists_and_cwd_detection_is_enabled() {
    run_with_faked_timers(async {
        let f = Fixture::new();
        let opener = linux_opener(&f, "");
        let cwd = "/Users/home/folder";
        let absolute_file = "/Users/home/folder/file.txt";
        f.file_service.set_files(vec![uri_from_path(absolute_file), uri_from_path("/Users/home/folder/other/file.txt")]);

        // Set a fake detected command starting as line 0 to establish the cwd
        f.set_command_at_line_0(cwd);
        open(&opener, search_link("file.txt")).await;
        assert_eq!(f.activation(), editor("file:///Users/home/folder/file.txt"));

        // Clear detected commands and ensure the same request results in a search since there are 2 matches
        f.command_detection.set_commands(Vec::new());
        opener.set_file_query_builder(Rc::new(StubFileQueryBuilder) as Rc<dyn IFileQueryBuilder>);
        f.search_service.set_search_result(search_complete(vec![
            uri_from_path("file:///Users/home/folder/file.txt"),
            uri_from_path("file:///Users/home/folder/other/file.txt"),
        ]));
        open(&opener, search_link("file.txt")).await;
        assert_eq!(f.activation(), search("file.txt"));
    });
}

#[test]
fn macos_linux_should_extract_column_and_or_line_numbers_from_links_in_a_workspace_containing_spaces() {
    run_with_faked_timers(async {
        let f = Fixture::new();
        let opener = linux_opener(&f, "/space folder");
        f.file_service.set_files(vec![uri_from_path("/space folder/foo/bar.txt")]);
        open(&opener, search_link("./foo/bar.txt:10:5")).await;
        assert_eq!(f.activation(), editor_at("file:///space%20folder/foo/bar.txt", 10.0, 5.0));
        open(&opener, search_link("./foo/bar.txt:10")).await;
        assert_eq!(f.activation(), editor_at("file:///space%20folder/foo/bar.txt", 10.0, 1.0));
    });
}

#[test]
fn macos_linux_should_extract_column_and_or_line_numbers_from_links_and_remove_trailing_periods() {
    run_with_faked_timers(async {
        let f = Fixture::new();
        let opener = linux_opener(&f, "/folder");
        f.file_service.set_files(vec![uri_from_path("/folder/foo/bar.txt")]);
        open(&opener, search_link("./foo/bar.txt.")).await;
        assert_eq!(f.activation(), editor("file:///folder/foo/bar.txt"));
        open(&opener, search_link("./foo/bar.txt:10:5.")).await;
        assert_eq!(f.activation(), editor_at("file:///folder/foo/bar.txt", 10.0, 5.0));
        open(&opener, search_link("./foo/bar.txt:10.")).await;
        assert_eq!(f.activation(), editor_at("file:///folder/foo/bar.txt", 10.0, 1.0));
    });
}

#[test]
fn macos_linux_should_extract_column_and_or_line_numbers_from_links_and_remove_grepped_lines() {
    run_with_faked_timers(async {
        let f = Fixture::new();
        let opener = linux_opener(&f, "/folder");
        f.file_service.set_files(vec![uri_from_path("/folder/foo/bar.txt")]);
        open(&opener, search_link("./foo/bar.txt:10:5:import { ILoveVSCode } from './foo/bar.ts';")).await;
        assert_eq!(f.activation(), editor_at("file:///folder/foo/bar.txt", 10.0, 5.0));
        open(&opener, search_link("./foo/bar.txt:10:import { ILoveVSCode } from './foo/bar.ts';")).await;
        assert_eq!(f.activation(), editor_at("file:///folder/foo/bar.txt", 10.0, 1.0));
    });
}

// Test for https://github.com/microsoft/vscode/pull/200919#discussion_r1428124196
#[test]
fn macos_linux_should_extract_column_and_or_line_numbers_from_links_and_remove_grepped_lines_incl_singular_spaces() {
    run_with_faked_timers(async {
        let f = Fixture::new();
        let opener = linux_opener(&f, "/folder");
        f.file_service.set_files(vec![uri_from_path("/folder/foo/bar.txt")]);
        open(&opener, search_link("./foo/bar.txt:10:5: ")).await;
        assert_eq!(f.activation(), editor_at("file:///folder/foo/bar.txt", 10.0, 5.0));
        open(&opener, search_link("./foo/bar.txt:10: ")).await;
        assert_eq!(f.activation(), editor_at("file:///folder/foo/bar.txt", 10.0, 1.0));
    });
}

#[test]
fn macos_linux_should_extract_line_numbers_from_links_and_remove_ruby_stack_traces() {
    run_with_faked_timers(async {
        let f = Fixture::new();
        let opener = linux_opener(&f, "/folder");
        f.file_service.set_files(vec![uri_from_path("/folder/foo/bar.rb")]);
        open(&opener, search_link("./foo/bar.rb:30:in `<main>`")).await;
        assert_eq!(f.activation(), editor_at("file:///folder/foo/bar.rb", 30.0, 1.0));
    });
}

#[test]
fn macos_linux_should_not_misinterpret_iso_8601_timestamps_as_line_column_numbers() {
    run_with_faked_timers(async {
        let f = Fixture::new();
        let opener = linux_opener(&f, "/folder");
        // Intentionally not set the file so it does not get picked up as localFile.
        f.file_service.set_files(Vec::new());
        open(&opener, link("test-2025-04-28T11:03:09+02:00.log", 34, TerminalBuiltinLinkType::Search)).await;
        assert_eq!(f.activation(), search("test-2025-04-28T11:03:09+02:00.log"));
        open(&opener, link("./test-2025-04-28T11:03:09+02:00.log", 36, TerminalBuiltinLinkType::Search)).await;
        assert_eq!(f.activation(), search("test-2025-04-28T11:03:09+02:00.log"));

        // Test when file exists, and there are preceding arguments
        f.file_service.set_files(vec![uri_from_path("/folder/test-2025-04-28T14:30:00+02:00.log")]);
        let mut link_with_offset = link("./test-2025-04-28T14:30:00+02:00.log", 45, TerminalBuiltinLinkType::LocalFile);
        link_with_offset.buffer_range.start.x = 10;
        open(&opener, link_with_offset).await;
        assert_eq!(f.activation(), editor("file:///folder/test-2025-04-28T14%3A30%3A00%2B02%3A00.log"));
    });
}

// suite('TerminalSearchLinkOpener') > suite('Windows')

/// `setup` de `Windows`.
fn windows_opener(f: &Fixture, initial_cwd: &str) -> TerminalSearchLinkOpener {
    f.opener(initial_cwd, OperatingSystem::Windows)
}

#[test]
fn windows_should_apply_the_cwd_to_the_link_only_when_the_file_exists_and_cwd_detection_is_enabled() {
    run_with_faked_timers(async {
        let f = Fixture::new();
        let opener = windows_opener(&f, "c:\\Users");

        let cwd = "c:\\Users\\home\\folder";
        let absolute_file = "c:\\Users\\home\\folder\\file.txt";

        f.file_service.set_files(vec![uri_file("/c:/Users/home/folder/file.txt")]);

        // Set a fake detected command starting as line 0 to establish the cwd
        f.set_command_at_line_0(cwd);
        open(&opener, search_link("file.txt")).await;
        assert_eq!(f.activation(), editor("file:///c%3A/Users/home/folder/file.txt"));

        // Clear detected commands and ensure the same request results in a search
        f.command_detection.set_commands(Vec::new());
        opener.set_file_query_builder(Rc::new(StubFileQueryBuilder) as Rc<dyn IFileQueryBuilder>);
        f.search_service.set_search_result(search_complete(vec![uri_file(absolute_file), uri_file("/c:/Users/home/folder/other/file.txt")]));
        open(&opener, search_link("file.txt")).await;
        assert_eq!(f.activation(), search("file.txt"));
    });
}

#[test]
fn windows_should_extract_column_and_or_line_numbers_from_links_in_a_workspace_containing_spaces() {
    run_with_faked_timers(async {
        let f = Fixture::new();
        let opener = windows_opener(&f, "c:/space folder");
        f.file_service.set_files(vec![uri_from_path("c:/space folder/foo/bar.txt")]);
        open(&opener, search_link("./foo/bar.txt:10:5")).await;
        assert_eq!(f.activation(), editor_at("file:///c%3A/space%20folder/foo/bar.txt", 10.0, 5.0));
        open(&opener, search_link("./foo/bar.txt:10")).await;
        assert_eq!(f.activation(), editor_at("file:///c%3A/space%20folder/foo/bar.txt", 10.0, 1.0));
        open(&opener, search_link(".\\foo\\bar.txt:10:5")).await;
        assert_eq!(f.activation(), editor_at("file:///c%3A/space%20folder/foo/bar.txt", 10.0, 5.0));
        open(&opener, search_link(".\\foo\\bar.txt:10")).await;
        assert_eq!(f.activation(), editor_at("file:///c%3A/space%20folder/foo/bar.txt", 10.0, 1.0));
    });
}

#[test]
fn windows_should_extract_column_and_or_line_numbers_from_links_and_remove_trailing_periods() {
    run_with_faked_timers(async {
        let f = Fixture::new();
        let opener = windows_opener(&f, "c:/folder");
        f.file_service.set_files(vec![uri_from_path("c:/folder/foo/bar.txt")]);
        open(&opener, search_link("./foo/bar.txt.")).await;
        assert_eq!(f.activation(), editor("file:///c%3A/folder/foo/bar.txt"));
        open(&opener, search_link("./foo/bar.txt:10:5.")).await;
        assert_eq!(f.activation(), editor_at("file:///c%3A/folder/foo/bar.txt", 10.0, 5.0));
        open(&opener, search_link("./foo/bar.txt:10.")).await;
        assert_eq!(f.activation(), editor_at("file:///c%3A/folder/foo/bar.txt", 10.0, 1.0));
        open(&opener, search_link(".\\foo\\bar.txt.")).await;
        assert_eq!(f.activation(), editor("file:///c%3A/folder/foo/bar.txt"));
        open(&opener, search_link(".\\foo\\bar.txt:2:5.")).await;
        assert_eq!(f.activation(), editor_at("file:///c%3A/folder/foo/bar.txt", 2.0, 5.0));
        open(&opener, search_link(".\\foo\\bar.txt:2.")).await;
        assert_eq!(f.activation(), editor_at("file:///c%3A/folder/foo/bar.txt", 2.0, 1.0));
    });
}

#[test]
fn windows_should_extract_column_and_or_line_numbers_from_links_and_remove_grepped_lines() {
    run_with_faked_timers(async {
        let f = Fixture::new();
        let opener = windows_opener(&f, "c:/folder");
        f.file_service.set_files(vec![uri_from_path("c:/folder/foo/bar.txt")]);
        open(&opener, search_link("./foo/bar.txt:10:5:import { ILoveVSCode } from './foo/bar.ts';")).await;
        assert_eq!(f.activation(), editor_at("file:///c%3A/folder/foo/bar.txt", 10.0, 5.0));
        open(&opener, search_link("./foo/bar.txt:10:import { ILoveVSCode } from './foo/bar.ts';")).await;
        assert_eq!(f.activation(), editor_at("file:///c%3A/folder/foo/bar.txt", 10.0, 1.0));
        open(&opener, search_link(".\\foo\\bar.txt:10:5:import { ILoveVSCode } from './foo/bar.ts';")).await;
        assert_eq!(f.activation(), editor_at("file:///c%3A/folder/foo/bar.txt", 10.0, 5.0));
        open(&opener, search_link(".\\foo\\bar.txt:10:import { ILoveVSCode } from './foo/bar.ts';")).await;
        assert_eq!(f.activation(), editor_at("file:///c%3A/folder/foo/bar.txt", 10.0, 1.0));
    });
}

// Test for https://github.com/microsoft/vscode/pull/200919#discussion_r1428124196
#[test]
fn windows_should_extract_column_and_or_line_numbers_from_links_and_remove_grepped_lines_incl_singular_spaces() {
    run_with_faked_timers(async {
        let f = Fixture::new();
        let opener = windows_opener(&f, "c:/folder");
        f.file_service.set_files(vec![uri_from_path("c:/folder/foo/bar.txt")]);
        open(&opener, search_link("./foo/bar.txt:10:5: ")).await;
        assert_eq!(f.activation(), editor_at("file:///c%3A/folder/foo/bar.txt", 10.0, 5.0));
        open(&opener, search_link("./foo/bar.txt:10: ")).await;
        assert_eq!(f.activation(), editor_at("file:///c%3A/folder/foo/bar.txt", 10.0, 1.0));
        open(&opener, search_link(".\\foo\\bar.txt:10:5: ")).await;
        assert_eq!(f.activation(), editor_at("file:///c%3A/folder/foo/bar.txt", 10.0, 5.0));
        open(&opener, search_link(".\\foo\\bar.txt:10: ")).await;
        assert_eq!(f.activation(), editor_at("file:///c%3A/folder/foo/bar.txt", 10.0, 1.0));
    });
}

#[test]
fn windows_should_extract_line_numbers_from_links_and_remove_ruby_stack_traces() {
    run_with_faked_timers(async {
        let f = Fixture::new();
        let opener = windows_opener(&f, "c:/folder");
        f.file_service.set_files(vec![uri_from_path("c:/folder/foo/bar.rb")]);
        open(&opener, search_link("./foo/bar.rb:30:in `<main>`")).await;
        // Since Ruby doesn't appear to put columns in stack traces, this should be 1
        assert_eq!(f.activation(), editor_at("file:///c%3A/folder/foo/bar.rb", 30.0, 1.0));
        open(&opener, search_link(".\\foo\\bar.rb:30:in `<main>`")).await;
        // Since Ruby doesn't appear to put columns in stack traces, this should be 1
        assert_eq!(f.activation(), editor_at("file:///c%3A/folder/foo/bar.rb", 30.0, 1.0));
    });
}

#[test]
fn windows_should_not_misinterpret_iso_8601_timestamps_as_line_column_numbers() {
    run_with_faked_timers(async {
        let f = Fixture::new();
        let opener = windows_opener(&f, "c:/folder");
        // Intentionally not set the file so it does not get picked up as localFile.
        f.file_service.set_files(Vec::new());
        open(&opener, link("test-2025-04-28T11:03:09+02:00.log", 34, TerminalBuiltinLinkType::Search)).await;
        assert_eq!(f.activation(), search("test-2025-04-28T11:03:09+02:00.log"));
        open(&opener, link(".\\test-2025-04-28T11:03:09+02:00.log", 36, TerminalBuiltinLinkType::Search)).await;
        assert_eq!(f.activation(), search("test-2025-04-28T11:03:09+02:00.log"));

        // Test when file exists, and there are preceding arguments
        f.file_service.set_files(vec![uri_from_path("c:/folder/test-2025-04-28T14:30:00+02:00.log")]);
        let mut link_with_offset = link(".\\test-2025-04-28T14:30:00+02:00.log", 45, TerminalBuiltinLinkType::LocalFile);
        link_with_offset.buffer_range.start.x = 10;
        open(&opener, link_with_offset).await;
        assert_eq!(f.activation(), editor("file:///c%3A/folder/test-2025-04-28T14%3A30%3A00%2B02%3A00.log"));
    });
}
