//! Porte de `src/vs/workbench/contrib/terminalContrib/quickFix/test/browser/quickFixAddon.test.ts` (suíte `QuickFixAddon`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - **`setup` e `ensureNoDisposablesAreLeakedInTestSuite()`.** O [`Fixture`] cria o guarda de vazamentos antes de tudo, o [`FakeXterm`] no
//!   lugar do `Terminal` do xterm, o store de capacidades com a `CommandDetectionCapability` real e o addon, e o `loadAddon` é
//!   [`TerminalQuickFixAddon::activate`]. O addon é descartado pelo guarda (no original, o `terminal.dispose()` descarta os addons carregados).
//! - **`TestInstantiationService`.** O addon é construído à mão com os serviços: o `ITerminalQuickFixService` do `stub` (eventos `Event.None` e
//!   `extensionQuickFixes: Promise.resolve([])`), o `ILabelService` (`{}` no original, que nenhum caso usa, é o `MockLabelService`), o
//!   `IOpenerService` (`{}`, o [`StubOpenerService`] que lança se for usado), o `ICommandService` (o `TestCommandService`, aqui o
//!   `MockCommandService`), e o `IExtensionService` e o `ITelemetryService`, que a injeção não-estrita do original resolve sem stub, são os
//!   dublês de teste. O `IConfigurationService`, o `IStorageService` e o `IContextMenuService` do `setup` não entram no addon do porte (ver
//!   o topo de `quick_fix_addon.rs`: a decoração e o menu ficam de fora).
//! - **`createCommand`.** O literal `{ ... } as ITerminalCommand` é o [`FakeCommand`]; só `command`, `exitCode`, `getOutputMatch` e o que o
//!   contrato exige existem. O `getOutputMatch` ignora o argumento e casa `output.match(outputMatcher)`, como o original.
//! - **`strictEqual(await getQuickFixesForCommand(...), undefined)`.** O resultado `Result<Option<Vec<ITerminalAction>>, Error>` é desembrulhado e
//!   comparado com `None`.
//! - **`assertMatchOptions`.** `strictEqual(actual?.length, expected.length)` falha com `None`; compara `id`, `enabled`, `label`, `tooltip` e, quando o
//!   esperado traz, `command` e `uri.toString()`.
//! - **`if (!isWindows)`** em volta da suíte `freePort` é `#[cfg(not(windows))]` nos dois casos dela.
//! - **Estado compartilhado dos `suite`.** `output` com `let` (reatribuído em dois casos de `gitSimilarCommand`) só é lido depois da reatribuição
//!   dentro de cada caso; cada caso usa o seu texto.
//! - **Casos assíncronos.** Rodam em `run_with_faked_timers`.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod support;

use std::rc::Rc;

use common::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite, run_with_faked_timers};
use indexmap::IndexMap;
use je_base::event::Event;
use je_base::js_regexp::JsRegExp;
use je_base::js_string::JsString;
use je_base::lifecycle::DisposableRef;
use je_base::promise::Promise;
use je_base::uri::URI;
use je_platform::commands::commands::ICommandService;
use je_platform::label::label::ILabelService;
use je_platform::log::NullLogService;
use je_platform::opener::opener::{
    IExternalOpener, IExternalUriResolver, IOpener, IOpenerService, IValidator, OpenOptions, OpenResource, ResolveExternalUriOptions, ResolvedExternalUri,
};
use je_platform::telemetry::telemetry_utils::null_telemetry_service;
use je_platform::terminal::capabilities::capabilities::{
    CommandDetection, IBaseTerminalCommand, IMarkProperties, IMarker, ITerminalCommand, TerminalCommandConfidence,
};
use je_platform::terminal::capabilities::command_detection_capability::CommandDetectionCapability;
use je_platform::terminal::capabilities::terminal_capability_store::TerminalCapabilityStore;
use je_platform::terminal::terminal::{ITerminalOutputMatch, ITerminalOutputMatcher, RegExpMatchArray};
use je_platform::terminal::xterm_headless::IHeadlessTerminal;
use je_workbench::contrib::terminal_contrib::quick_fix::quick_fix::{
    ITerminalCommandSelector, ITerminalQuickFixProvider, ITerminalQuickFixProviderSelector, ITerminalQuickFixService, QuickFixOption,
};
use je_workbench::contrib::terminal_contrib::quick_fix::quick_fix_addon::{
    ITerminalAction, ITerminalQuickFixAddon, TerminalQuickFixAddon, TerminalQuickFixAddonServices, get_quick_fixes_for_command,
};
use je_workbench::contrib::terminal_contrib::quick_fix::terminal_quick_fix_builtin_actions::{
    free_port, free_port_output_regex, git_create_pr, git_create_pr_output_regex, git_fast_forward_pull, git_fast_forward_pull_output_regex, git_push_output_regex,
    git_push_set_upstream, git_similar, git_similar_output_regex, git_two_dashes, git_two_dashes_regex, pwsh_general_error, pwsh_general_error_output_regex,
    pwsh_unix_command_not_found_error, pwsh_unix_command_not_found_error_output_regex,
};
use je_workbench::contrib::terminal_contrib::quick_fix::quick_fix::matcher_to_string;
use je_workbench::test::browser::workbench_test_services::{MockLabelService, TestExtensionService};
use support::fake_xterm::FakeXterm;
use support::mock_command_service::MockCommandService;

/// O `ITerminalQuickFixService` do `instantiationService.stub(ITerminalQuickFixService, { ... })`: só os quatro membros do `stub`.
struct StubQuickFixService;

impl ITerminalQuickFixService for StubQuickFixService {
    fn on_did_register_provider(&self) -> Event<ITerminalQuickFixProviderSelector> {
        Event::none()
    }

    fn on_did_register_command_selector(&self) -> Event<ITerminalCommandSelector> {
        Event::none()
    }

    fn on_did_unregister_provider(&self) -> Event<String> {
        Event::none()
    }

    fn extension_quick_fixes(&self) -> Promise<Vec<ITerminalCommandSelector>> {
        Promise::resolved(Vec::new())
    }

    fn providers(&self) -> Rc<std::cell::RefCell<std::collections::HashMap<String, Rc<dyn ITerminalQuickFixProvider>>>> {
        panic!("providers is not defined")
    }

    fn register_quick_fix_provider(&self, _id: &str, _provider: Rc<dyn ITerminalQuickFixProvider>) -> DisposableRef {
        panic!("registerQuickFixProvider is not a function")
    }

    fn register_command_selector(&self, _selector: ITerminalCommandSelector) {
        panic!("registerCommandSelector is not a function")
    }
}

/// O `IOpenerService` `{}`: todo membro lança, como chamar um membro ausente do objeto vazio.
struct StubOpenerService;

impl IOpenerService for StubOpenerService {
    fn register_opener(&self, _opener: Rc<dyn IOpener>) -> DisposableRef {
        panic!("registerOpener is not a function")
    }

    fn register_validator(&self, _validator: Rc<dyn IValidator>) -> DisposableRef {
        panic!("registerValidator is not a function")
    }

    fn register_external_uri_resolver(&self, _resolver: Rc<dyn IExternalUriResolver>) -> DisposableRef {
        panic!("registerExternalUriResolver is not a function")
    }

    fn set_default_external_opener(&self, _opener: Rc<dyn IExternalOpener>) {
        panic!("setDefaultExternalOpener is not a function")
    }

    fn register_external_opener(&self, _opener: Rc<dyn IExternalOpener>) -> DisposableRef {
        panic!("registerExternalOpener is not a function")
    }

    fn open(&self, _resource: &OpenResource, _options: Option<&OpenOptions>) -> Promise<bool> {
        panic!("open is not a function")
    }

    fn resolve_external_uri(&self, _resource: &URI, _options: Option<&ResolveExternalUriOptions>) -> Promise<ResolvedExternalUri> {
        panic!("resolveExternalUri is not a function")
    }
}

/// O literal de `createCommand(...) as ITerminalCommand`.
struct FakeCommand {
    command: String,
    output: String,
    output_matcher: Option<Rc<JsRegExp>>,
    exit_code: Option<f64>,
    output_lines: Option<Vec<String>>,
}

impl IBaseTerminalCommand for FakeCommand {
    fn command(&self) -> String {
        self.command.clone()
    }

    fn set_command(&self, _command: &str) {
        panic!("setCommand is not a function")
    }

    fn command_line_confidence(&self) -> TerminalCommandConfidence {
        TerminalCommandConfidence::Low
    }

    fn set_command_line_confidence(&self, _confidence: TerminalCommandConfidence) {
        panic!("setCommandLineConfidence is not a function")
    }

    fn is_trusted(&self) -> bool {
        true
    }

    fn timestamp(&self) -> f64 {
        0.0
    }

    fn duration(&self) -> f64 {
        0.0
    }

    fn id(&self) -> Option<String> {
        None
    }

    fn cwd(&self) -> Option<String> {
        Some(String::new())
    }

    fn exit_code(&self) -> Option<f64> {
        self.exit_code
    }

    fn command_start_line_content(&self) -> Option<String> {
        Some(String::new())
    }

    fn mark_properties(&self) -> Option<IMarkProperties> {
        Some(IMarkProperties::default())
    }

    fn executed_x(&self) -> Option<f64> {
        None
    }

    fn start_x(&self) -> Option<f64> {
        None
    }
}

impl ITerminalCommand for FakeCommand {
    fn prompt_start_marker(&self) -> Option<Rc<dyn IMarker>> {
        None
    }

    fn marker(&self) -> Option<Rc<dyn IMarker>> {
        None
    }

    fn end_marker(&self) -> Option<Rc<dyn IMarker>> {
        None
    }

    fn set_end_marker(&self, _marker: Option<Rc<dyn IMarker>>) {
        panic!("setEndMarker is not a function")
    }

    fn executed_marker(&self) -> Option<Rc<dyn IMarker>> {
        None
    }

    fn aliases(&self) -> Option<Vec<Vec<String>>> {
        None
    }

    fn was_replayed(&self) -> Option<bool> {
        None
    }

    fn extract_command_line(&self) -> String {
        panic!("extractCommandLine is not a function")
    }

    fn get_output(&self) -> Option<String> {
        Some(self.output.clone())
    }

    fn get_output_match(&self, _matcher: &ITerminalOutputMatcher) -> Option<ITerminalOutputMatch> {
        let output_matcher = self.output_matcher.as_ref()?;
        let input = JsString::from(self.output.as_str());
        output_matcher.set_last_index(0);
        let matched = output_matcher.exec(input.units())?;
        Some(ITerminalOutputMatch { regex_match: RegExpMatchArray { input, matched }, output_lines: self.output_lines.clone().unwrap_or_default() })
    }

    fn has_output(&self) -> bool {
        !self.output.is_empty()
    }

    fn get_prompt_row_count(&self) -> f64 {
        panic!("getPromptRowCount is not a function")
    }

    fn get_command_row_count(&self) -> f64 {
        panic!("getCommandRowCount is not a function")
    }
}

/// `createCommand(command, output, outputMatcher?, exitCode?, outputLines?)`.
fn create_command(command: &str, output: &str, output_matcher: Option<Rc<JsRegExp>>, exit_code: Option<f64>, output_lines: Option<Vec<String>>) -> Rc<dyn ITerminalCommand> {
    Rc::new(FakeCommand { command: command.to_owned(), output: output.to_owned(), output_matcher, exit_code, output_lines })
}

/// `TestAction`: o que `assertMatchOptions` compara.
struct TestAction {
    id: &'static str,
    enabled: bool,
    label: String,
    tooltip: String,
    command: Option<String>,
    uri: Option<&'static str>,
}

fn run_action(id: &'static str, command: &str) -> TestAction {
    TestAction { id, enabled: true, label: format!("Run: {command}"), tooltip: format!("Run: {command}"), command: Some(command.to_owned()), uri: None }
}

/// `assertMatchOptions(actual, expected)`.
fn assert_match_options(actual: Option<Vec<ITerminalAction>>, expected: &[TestAction]) {
    assert_eq!(actual.as_ref().map(Vec::len), Some(expected.len()));
    let actual = actual.unwrap();
    for (expected_item, actual_item) in expected.iter().zip(&actual) {
        assert_eq!(actual_item.id, expected_item.id, "ID");
        assert_eq!(actual_item.enabled, expected_item.enabled, "enabled");
        assert_eq!(actual_item.label, expected_item.label, "label");
        assert_eq!(actual_item.tooltip, expected_item.tooltip, "tooltip");
        if let Some(command) = &expected_item.command {
            assert_eq!(actual_item.command.as_ref(), Some(command));
        }
        if let Some(uri) = expected_item.uri {
            assert_eq!(actual_item.uri.as_ref().unwrap().to_string(false).to_string_lossy(), URI::parse(uri, false).to_string(false).to_string_lossy());
        }
    }
}

type OptionsMap = IndexMap<String, Vec<QuickFixOption>>;

/// O `setup` da suíte `QuickFixAddon`.
struct Fixture {
    xterm: Rc<FakeXterm>,
    quick_fix_addon: Rc<TerminalQuickFixAddon>,
    command_service: Rc<dyn ICommandService>,
    opener_service: Rc<dyn IOpenerService>,
    label_service: Rc<dyn ILabelService>,
    // Declarado por último: o `Drop` dele descarta o que o `Fixture` registrou e confere os vazamentos.
    _guard: LeakGuard,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        self.xterm.dispose();
    }
}

impl Fixture {
    fn new() -> Self {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let xterm = FakeXterm::new(80, 30);
        let terminal: Rc<dyn IHeadlessTerminal> = xterm.clone();
        let capabilities = guard.add_rc(TerminalCapabilityStore::new());
        let command_detection = guard.add_rc(CommandDetectionCapability::new(terminal.clone(), NullLogService::shared()));
        capabilities.add::<CommandDetection>(command_detection);
        let command_service: Rc<dyn ICommandService> = Rc::new(MockCommandService::default());
        let opener_service: Rc<dyn IOpenerService> = Rc::new(StubOpenerService);
        let label_service: Rc<dyn ILabelService> = MockLabelService::new(None);

        let quick_fix_addon = guard.add_rc(TerminalQuickFixAddon::new(
            &je_base::uuid::generate_uuid(),
            Some(Vec::new()),
            capabilities,
            TerminalQuickFixAddonServices {
                command_service: command_service.clone(),
                extension_service: TestExtensionService::new(),
                label_service: label_service.clone(),
                opener_service: opener_service.clone(),
                telemetry_service: null_telemetry_service(),
                quick_fix_service: Rc::new(StubQuickFixService),
            },
        ));
        quick_fix_addon.activate(terminal);
        Self { xterm, quick_fix_addon, command_service, opener_service, label_service, _guard: guard }
    }

    /// `setup(() => { expectedMap.set(command.commandLineMatcher.toString(), [command]); quickFixAddon.registerCommandFinishedListener(command); })`.
    fn register(&self, expected_map: &mut OptionsMap, command: QuickFixOption) {
        expected_map.insert(matcher_to_string(&command.command_line_matcher), vec![command.clone()]);
        self.quick_fix_addon.register_command_finished_listener(command);
    }

    /// `await getQuickFixesForCommand([], terminal, command, expectedMap, commandService, openerService, labelService)`.
    async fn quick_fixes(&self, command: Rc<dyn ITerminalCommand>, expected_map: &OptionsMap) -> Option<Vec<ITerminalAction>> {
        get_quick_fixes_for_command(
            Some(&[]),
            &*self.xterm,
            &command,
            expected_map,
            &self.command_service,
            &self.opener_service,
            &self.label_service,
            None,
            None,
        )
        .await
        .unwrap()
    }
}

fn lines(output: &str) -> Option<Vec<String>> {
    Some(output.split('\n').map(str::to_owned).collect())
}

// ---------------------------------------------------------------------------------------------------------------------
// suite('registerCommandFinishedListener & getMatchActions') > suite('gitSimilarCommand')
// ---------------------------------------------------------------------------------------------------------------------

const GIT_SIMILAR_COMMAND: &str = "git sttatus";
const GIT_SIMILAR_OUTPUT: &str = "git: 'sttatus' is not a git command. See 'git --help'.\n\n\t\t\tThe most similar command is\n\t\t\tstatus";
const GIT_SIMILAR_EXIT_CODE: f64 = 1.0;

fn git_similar_actions() -> Vec<TestAction> {
    vec![run_action("Git Similar", "git status")]
}

/// O `setup` da suíte `gitSimilarCommand`.
fn git_similar_setup(f: &Fixture) -> OptionsMap {
    let mut expected_map = OptionsMap::new();
    f.register(&mut expected_map, git_similar());
    expected_map
}

#[test]
fn git_similar_command_returns_undefined_when_output_does_not_match() {
    run_with_faked_timers(async {
        let f = Fixture::new();
        let expected_map = git_similar_setup(&f);
        let command = create_command(GIT_SIMILAR_COMMAND, "invalid output", Some(git_similar_output_regex()), Some(GIT_SIMILAR_EXIT_CODE), Some(vec!["invalid output".to_owned()]));
        assert!(f.quick_fixes(command, &expected_map).await.is_none());
    });
}

#[test]
fn git_similar_command_returns_undefined_when_command_does_not_match() {
    run_with_faked_timers(async {
        let f = Fixture::new();
        let expected_map = git_similar_setup(&f);
        let command = create_command("gt sttatus", GIT_SIMILAR_OUTPUT, Some(git_similar_output_regex()), Some(GIT_SIMILAR_EXIT_CODE), lines(GIT_SIMILAR_OUTPUT));
        assert!(f.quick_fixes(command, &expected_map).await.is_none());
    });
}

#[test]
fn git_similar_command_returns_actions_when_expected_unix_exit_code() {
    run_with_faked_timers(async {
        let f = Fixture::new();
        let expected_map = git_similar_setup(&f);
        let command = create_command(GIT_SIMILAR_COMMAND, GIT_SIMILAR_OUTPUT, Some(git_similar_output_regex()), Some(GIT_SIMILAR_EXIT_CODE), lines(GIT_SIMILAR_OUTPUT));
        assert_match_options(f.quick_fixes(command, &expected_map).await, &git_similar_actions());
    });
}

#[test]
fn git_similar_command_returns_actions_when_matching_exit_status() {
    run_with_faked_timers(async {
        let f = Fixture::new();
        let expected_map = git_similar_setup(&f);
        let command = create_command(GIT_SIMILAR_COMMAND, GIT_SIMILAR_OUTPUT, Some(git_similar_output_regex()), Some(2.0), lines(GIT_SIMILAR_OUTPUT));
        assert_match_options(f.quick_fixes(command, &expected_map).await, &git_similar_actions());
    });
}

#[test]
fn git_similar_command_returns_match_returns_match() {
    run_with_faked_timers(async {
        let f = Fixture::new();
        let expected_map = git_similar_setup(&f);
        let command = create_command(GIT_SIMILAR_COMMAND, GIT_SIMILAR_OUTPUT, Some(git_similar_output_regex()), Some(GIT_SIMILAR_EXIT_CODE), lines(GIT_SIMILAR_OUTPUT));
        assert_match_options(f.quick_fixes(command, &expected_map).await, &git_similar_actions());
    });
}

#[test]
fn git_similar_command_returns_match_returns_multiple_match() {
    run_with_faked_timers(async {
        let f = Fixture::new();
        let expected_map = git_similar_setup(&f);
        let output = "git: 'pu' is not a git command. See 'git --help'.\n\t\t\t\tThe most similar commands are\n\t\t\t\t\t\tpull\n\t\t\t\t\t\tpush";
        let actions = vec![run_action("Git Similar", "git pull"), run_action("Git Similar", "git push")];
        let command = create_command("git pu", output, Some(git_similar_output_regex()), Some(GIT_SIMILAR_EXIT_CODE), lines(output));
        assert_match_options(f.quick_fixes(command, &expected_map).await, &actions);
    });
}

#[test]
fn git_similar_command_returns_match_passes_any_arguments_through() {
    run_with_faked_timers(async {
        let f = Fixture::new();
        let expected_map = git_similar_setup(&f);
        let output = "git: 'checkoutt' is not a git command. See 'git --help'.\n\t\t\t\tThe most similar commands are\n\t\t\t\t\t\tcheckout";
        let command = create_command("git checkoutt .", output, Some(git_similar_output_regex()), Some(GIT_SIMILAR_EXIT_CODE), lines(output));
        assert_match_options(f.quick_fixes(command, &expected_map).await, &[run_action("Git Similar", "git checkout .")]);
    });
}

// ---------------------------------------------------------------------------------------------------------------------
// suite('gitTwoDashes')
// ---------------------------------------------------------------------------------------------------------------------

const GIT_TWO_DASHES_COMMAND: &str = "git add . -all";
const GIT_TWO_DASHES_OUTPUT: &str = "error: did you mean `--all` (with two dashes)?";

fn git_two_dashes_actions() -> Vec<TestAction> {
    vec![run_action("Git Two Dashes", "git add . --all")]
}

fn git_two_dashes_setup(f: &Fixture) -> OptionsMap {
    let mut expected_map = OptionsMap::new();
    f.register(&mut expected_map, git_two_dashes());
    expected_map
}

#[test]
fn git_two_dashes_returns_undefined_when_output_does_not_match() {
    run_with_faked_timers(async {
        let f = Fixture::new();
        let expected_map = git_two_dashes_setup(&f);
        let command = create_command(GIT_TWO_DASHES_COMMAND, "invalid output", Some(git_two_dashes_regex()), Some(1.0), None);
        assert!(f.quick_fixes(command, &expected_map).await.is_none());
    });
}

#[test]
fn git_two_dashes_returns_undefined_when_command_does_not_match() {
    run_with_faked_timers(async {
        let f = Fixture::new();
        let expected_map = git_two_dashes_setup(&f);
        let command = create_command("gt sttatus", GIT_TWO_DASHES_OUTPUT, Some(git_two_dashes_regex()), Some(1.0), None);
        assert!(f.quick_fixes(command, &expected_map).await.is_none());
    });
}

#[test]
fn git_two_dashes_returns_actions_when_expected_unix_exit_code() {
    run_with_faked_timers(async {
        let f = Fixture::new();
        let expected_map = git_two_dashes_setup(&f);
        let command = create_command(GIT_TWO_DASHES_COMMAND, GIT_TWO_DASHES_OUTPUT, Some(git_two_dashes_regex()), Some(1.0), None);
        assert_match_options(f.quick_fixes(command, &expected_map).await, &git_two_dashes_actions());
    });
}

#[test]
fn git_two_dashes_returns_actions_when_matching_exit_status() {
    run_with_faked_timers(async {
        let f = Fixture::new();
        let expected_map = git_two_dashes_setup(&f);
        let command = create_command(GIT_TWO_DASHES_COMMAND, GIT_TWO_DASHES_OUTPUT, Some(git_two_dashes_regex()), Some(2.0), None);
        assert_match_options(f.quick_fixes(command, &expected_map).await, &git_two_dashes_actions());
    });
}

// ---------------------------------------------------------------------------------------------------------------------
// suite('gitFastForwardPull')
// ---------------------------------------------------------------------------------------------------------------------

const GIT_FAST_FORWARD_COMMAND: &str = "git checkout vnext";
const GIT_FAST_FORWARD_OUTPUT: &str = "Already on 'vnext' \n Your branch is behind 'origin/vnext' by 1 commit, and can be fast-forwarded.";

fn git_fast_forward_actions() -> Vec<TestAction> {
    vec![run_action("Git Fast Forward Pull", "git pull")]
}

fn git_fast_forward_setup(f: &Fixture) -> OptionsMap {
    let mut expected_map = OptionsMap::new();
    f.register(&mut expected_map, git_fast_forward_pull());
    expected_map
}

#[test]
fn git_fast_forward_pull_returns_undefined_when_output_does_not_match() {
    run_with_faked_timers(async {
        let f = Fixture::new();
        let expected_map = git_fast_forward_setup(&f);
        let command = create_command(GIT_FAST_FORWARD_COMMAND, "invalid output", Some(git_fast_forward_pull_output_regex()), Some(0.0), None);
        assert!(f.quick_fixes(command, &expected_map).await.is_none());
    });
}

#[test]
fn git_fast_forward_pull_returns_undefined_when_command_does_not_match() {
    run_with_faked_timers(async {
        let f = Fixture::new();
        let expected_map = git_fast_forward_setup(&f);
        let command = create_command("gt add", GIT_FAST_FORWARD_OUTPUT, Some(git_fast_forward_pull_output_regex()), Some(0.0), None);
        assert!(f.quick_fixes(command, &expected_map).await.is_none());
    });
}

#[test]
fn git_fast_forward_pull_returns_undefined_when_exit_code_does_not_match() {
    run_with_faked_timers(async {
        let f = Fixture::new();
        let expected_map = git_fast_forward_setup(&f);
        let command = create_command(GIT_FAST_FORWARD_COMMAND, GIT_FAST_FORWARD_OUTPUT, Some(git_fast_forward_pull_output_regex()), Some(2.0), None);
        assert!(f.quick_fixes(command, &expected_map).await.is_none());
    });
}

#[test]
fn git_fast_forward_pull_returns_actions_when_matching_exit_status_command_ouput() {
    run_with_faked_timers(async {
        let f = Fixture::new();
        let expected_map = git_fast_forward_setup(&f);
        let command = create_command(GIT_FAST_FORWARD_COMMAND, GIT_FAST_FORWARD_OUTPUT, Some(git_fast_forward_pull_output_regex()), Some(0.0), None);
        assert_match_options(f.quick_fixes(command, &expected_map).await, &git_fast_forward_actions());
    });
}

// ---------------------------------------------------------------------------------------------------------------------
// if (!isWindows) suite('freePort')
// ---------------------------------------------------------------------------------------------------------------------

#[cfg(not(windows))]
const FREE_PORT_COMMAND: &str = "yarn start dev";
#[cfg(not(windows))]
const FREE_PORT_OUTPUT: &str = "yarn run v1.22.17\n\t\t\twarning ../../package.json: No license field\n\t\t\tError: listen EADDRINUSE: address already in use 0.0.0.0:3000\n\t\t\t\tat Server.setupListenHandle [as _listen2] (node:net:1315:16)\n\t\t\t\tat listenInCluster (node:net:1363:12)\n\t\t\t\tat doListen (node:net:1501:7)\n\t\t\t\tat processTicksAndRejections (node:internal/process/task_queues:84:21)\n\t\t\tEmitted 'error' event on WebSocketServer instance at:\n\t\t\t\tat Server.emit (node:events:394:28)\n\t\t\t\tat emitErrorNT (node:net:1342:8)\n\t\t\t\tat processTicksAndRejections (node:internal/process/task_queues:83:21) {\n\t\t\t}\n\t\t\terror Command failed with exit code 1.\n\t\t\tinfo Visit https://yarnpkg.com/en/docs/cli/run for documentation about this command.";

#[cfg(not(windows))]
fn free_port_setup(f: &Fixture) -> OptionsMap {
    let mut expected_map = OptionsMap::new();
    f.register(&mut expected_map, free_port(Rc::new(|_port, _command_line| {})));
    expected_map
}

#[cfg(not(windows))]
#[test]
fn free_port_returns_undefined_when_output_does_not_match() {
    run_with_faked_timers(async {
        let f = Fixture::new();
        let expected_map = free_port_setup(&f);
        let command = create_command(FREE_PORT_COMMAND, "invalid output", Some(free_port_output_regex()), None, None);
        assert!(f.quick_fixes(command, &expected_map).await.is_none());
    });
}

#[cfg(not(windows))]
#[test]
fn free_port_returns_actions() {
    run_with_faked_timers(async {
        let f = Fixture::new();
        let expected_map = free_port_setup(&f);
        let action_options = vec![TestAction {
            id: "Free Port",
            enabled: true,
            label: "Free port 3000".to_owned(),
            tooltip: "Free port 3000".to_owned(),
            command: None,
            uri: None,
        }];
        let command = create_command(FREE_PORT_COMMAND, FREE_PORT_OUTPUT, Some(free_port_output_regex()), None, None);
        assert_match_options(f.quick_fixes(command, &expected_map).await, &action_options);
    });
}

// ---------------------------------------------------------------------------------------------------------------------
// suite('gitPushSetUpstream')
// ---------------------------------------------------------------------------------------------------------------------

const GIT_PUSH_COMMAND: &str = "git push";
const GIT_PUSH_OUTPUT: &str = "fatal: The current branch test22 has no upstream branch.\n\t\t\tTo push the current branch and set the remote as upstream, use\n\n\t\t\t\tgit push --set-upstream origin test22";
const GIT_PUSH_EXIT_CODE: f64 = 128.0;

fn git_push_actions() -> Vec<TestAction> {
    vec![run_action("Git Push Set Upstream", "git push --set-upstream origin test22")]
}

fn git_push_setup(f: &Fixture) -> OptionsMap {
    let mut expected_map = OptionsMap::new();
    f.register(&mut expected_map, git_push_set_upstream());
    expected_map
}

#[test]
fn git_push_set_upstream_returns_undefined_when_output_does_not_match() {
    run_with_faked_timers(async {
        let f = Fixture::new();
        let expected_map = git_push_setup(&f);
        let command = create_command(GIT_PUSH_COMMAND, "invalid output", Some(git_push_output_regex()), Some(GIT_PUSH_EXIT_CODE), None);
        assert!(f.quick_fixes(command, &expected_map).await.is_none());
    });
}

#[test]
fn git_push_set_upstream_returns_undefined_when_command_does_not_match() {
    run_with_faked_timers(async {
        let f = Fixture::new();
        let expected_map = git_push_setup(&f);
        let command = create_command("git status", GIT_PUSH_OUTPUT, Some(git_push_output_regex()), Some(GIT_PUSH_EXIT_CODE), None);
        assert!(f.quick_fixes(command, &expected_map).await.is_none());
    });
}

#[test]
fn git_push_set_upstream_returns_actions_when_expected_unix_exit_code() {
    run_with_faked_timers(async {
        let f = Fixture::new();
        let expected_map = git_push_setup(&f);
        let command = create_command(GIT_PUSH_COMMAND, GIT_PUSH_OUTPUT, Some(git_push_output_regex()), Some(GIT_PUSH_EXIT_CODE), None);
        assert_match_options(f.quick_fixes(command, &expected_map).await, &git_push_actions());
    });
}

#[test]
fn git_push_set_upstream_returns_actions_when_matching_exit_status() {
    run_with_faked_timers(async {
        let f = Fixture::new();
        let expected_map = git_push_setup(&f);
        let command = create_command(GIT_PUSH_COMMAND, GIT_PUSH_OUTPUT, Some(git_push_output_regex()), Some(2.0), None);
        assert_match_options(f.quick_fixes(command, &expected_map).await, &git_push_actions());
    });
}

// ---------------------------------------------------------------------------------------------------------------------
// suite('gitCreatePr')
// ---------------------------------------------------------------------------------------------------------------------

const GIT_CREATE_PR_OUTPUT: &str = "Total 0 (delta 0), reused 0 (delta 0), pack-reused 0\n\t\t\tremote:\n\t\t\tremote: Create a pull request for 'test22' on GitHub by visiting:\n\t\t\tremote:      https://github.com/meganrogge/xterm.js/pull/new/test22\n\t\t\tremote:\n\t\t\tTo https://github.com/meganrogge/xterm.js\n\t\t\t * [new branch]        test22 -> test22\n\t\t\tBranch 'test22' set up to track remote branch 'test22' from 'origin'. ";

fn git_create_pr_actions() -> Vec<TestAction> {
    vec![TestAction {
        id: "Git Create Pr",
        enabled: true,
        label: "Open: https://github.com/meganrogge/xterm.js/pull/new/test22".to_owned(),
        tooltip: "Open: https://github.com/meganrogge/xterm.js/pull/new/test22".to_owned(),
        command: None,
        uri: Some("https://github.com/meganrogge/xterm.js/pull/new/test22"),
    }]
}

fn git_create_pr_setup(f: &Fixture) -> OptionsMap {
    let mut expected_map = OptionsMap::new();
    f.register(&mut expected_map, git_create_pr());
    expected_map
}

#[test]
fn git_create_pr_returns_undefined_when_output_does_not_match() {
    run_with_faked_timers(async {
        let f = Fixture::new();
        let expected_map = git_create_pr_setup(&f);
        let command = create_command(GIT_PUSH_COMMAND, "invalid output", Some(git_create_pr_output_regex()), Some(0.0), None);
        assert!(f.quick_fixes(command, &expected_map).await.is_none());
    });
}

#[test]
fn git_create_pr_returns_undefined_when_command_does_not_match() {
    run_with_faked_timers(async {
        let f = Fixture::new();
        let expected_map = git_create_pr_setup(&f);
        let command = create_command("git status", GIT_CREATE_PR_OUTPUT, Some(git_create_pr_output_regex()), Some(0.0), None);
        assert!(f.quick_fixes(command, &expected_map).await.is_none());
    });
}

#[test]
fn git_create_pr_returns_undefined_when_failure_exit_status() {
    run_with_faked_timers(async {
        let f = Fixture::new();
        let expected_map = git_create_pr_setup(&f);
        let command = create_command(GIT_PUSH_COMMAND, GIT_CREATE_PR_OUTPUT, Some(git_create_pr_output_regex()), Some(2.0), None);
        assert!(f.quick_fixes(command, &expected_map).await.is_none());
    });
}

#[test]
fn git_create_pr_returns_actions_when_expected_unix_exit_code() {
    run_with_faked_timers(async {
        let f = Fixture::new();
        let expected_map = git_create_pr_setup(&f);
        let command = create_command(GIT_PUSH_COMMAND, GIT_CREATE_PR_OUTPUT, Some(git_create_pr_output_regex()), Some(0.0), None);
        assert_match_options(f.quick_fixes(command, &expected_map).await, &git_create_pr_actions());
    });
}

// ---------------------------------------------------------------------------------------------------------------------
// suite('gitPush - multiple providers')
// ---------------------------------------------------------------------------------------------------------------------

/// O `setup`: registra só o `prCommand` no addon, e o mapa leva `[pushCommand, prCommand]`.
fn git_push_multiple_setup(f: &Fixture) -> OptionsMap {
    let push_command = git_push_set_upstream();
    let pr_command = git_create_pr();
    f.quick_fix_addon.register_command_finished_listener(pr_command.clone());
    let mut expected_map = OptionsMap::new();
    expected_map.insert(matcher_to_string(&push_command.command_line_matcher), vec![push_command, pr_command]);
    expected_map
}

#[test]
fn git_push_multiple_providers_returns_undefined_when_output_does_not_match() {
    run_with_faked_timers(async {
        let f = Fixture::new();
        let expected_map = git_push_multiple_setup(&f);
        let command = create_command(GIT_PUSH_COMMAND, "invalid output", Some(git_push_output_regex()), Some(GIT_PUSH_EXIT_CODE), None);
        assert!(f.quick_fixes(command, &expected_map).await.is_none());
    });
}

#[test]
fn git_push_multiple_providers_returns_undefined_when_command_does_not_match() {
    run_with_faked_timers(async {
        let f = Fixture::new();
        let expected_map = git_push_multiple_setup(&f);
        let command = create_command("git status", GIT_PUSH_OUTPUT, Some(git_push_output_regex()), Some(GIT_PUSH_EXIT_CODE), None);
        assert!(f.quick_fixes(command, &expected_map).await.is_none());
    });
}

#[test]
fn git_push_multiple_providers_returns_actions_when_expected_unix_exit_code() {
    run_with_faked_timers(async {
        let f = Fixture::new();
        let expected_map = git_push_multiple_setup(&f);
        let command = create_command(GIT_PUSH_COMMAND, GIT_PUSH_OUTPUT, Some(git_push_output_regex()), Some(GIT_PUSH_EXIT_CODE), None);
        assert_match_options(f.quick_fixes(command, &expected_map).await, &git_push_actions());
    });
}

#[test]
fn git_push_multiple_providers_returns_actions_when_matching_exit_status() {
    run_with_faked_timers(async {
        let f = Fixture::new();
        let expected_map = git_push_multiple_setup(&f);
        let command = create_command(GIT_PUSH_COMMAND, GIT_PUSH_OUTPUT, Some(git_push_output_regex()), Some(2.0), None);
        assert_match_options(f.quick_fixes(command, &expected_map).await, &git_push_actions());
    });
}

// ---------------------------------------------------------------------------------------------------------------------
// suite('pwsh feedback providers')
// ---------------------------------------------------------------------------------------------------------------------

const PWSH_COMMAND: &str = "not important";

fn pwsh_general_output() -> String {
    [
        "...",
        "",
        "Suggestion [General]:",
        "  The most similar commands are: python3, python3m, pamon, python3.6, rtmon, echo, pushd, etsn, pwsh, pwconv.",
        "",
        "Suggestion [cmd-not-found]:",
        "  Command 'python' not found, but can be installed with:",
        "  sudo apt install python3",
        "  sudo apt install python",
        "  sudo apt install python-minimal",
        "  You also have python3 installed, you can run 'python3' instead.'",
        "",
    ]
    .join("\n")
}

fn pwsh_unix_output() -> String {
    [
        "...",
        "",
        "Suggestion [General]",
        "  The most similar commands are: python3, python3m, pamon, python3.6, rtmon, echo, pushd, etsn, pwsh, pwconv.",
        "",
        "Suggestion [cmd-not-found]:",
        "  Command 'python' not found, but can be installed with:",
        "  sudo apt install python3",
        "  sudo apt install python",
        "  sudo apt install python-minimal",
        "  You also have python3 installed, you can run 'python3' instead.'",
        "",
    ]
    .join("\n")
}

/// O `setup` das suítes `General` e `Unix cmd-not-found`: registra no addon e põe no mapa.
fn pwsh_setup(f: &Fixture, command: QuickFixOption) -> OptionsMap {
    let mut expected_map = OptionsMap::new();
    f.quick_fix_addon.register_command_finished_listener(command.clone());
    expected_map.insert(matcher_to_string(&command.command_line_matcher), vec![command]);
    expected_map
}

#[test]
fn pwsh_feedback_providers_general_returns_undefined_when_output_does_not_match() {
    run_with_faked_timers(async {
        let f = Fixture::new();
        let expected_map = pwsh_setup(&f, pwsh_general_error());
        let command = create_command(PWSH_COMMAND, "invalid output", Some(pwsh_general_error_output_regex()), Some(128.0), None);
        assert!(f.quick_fixes(command, &expected_map).await.is_none());
    });
}

#[test]
fn pwsh_feedback_providers_general_returns_actions_when_output_matches() {
    run_with_faked_timers(async {
        let f = Fixture::new();
        let expected_map = pwsh_setup(&f, pwsh_general_error());
        let actions: Vec<TestAction> =
            ["python3", "python3m", "pamon", "python3.6", "rtmon", "echo", "pushd", "etsn", "pwsh", "pwconv"].iter().map(|command| run_action("Pwsh General Error", command)).collect();
        let command = create_command(PWSH_COMMAND, &pwsh_general_output(), Some(pwsh_general_error_output_regex()), Some(128.0), None);
        assert_match_options(f.quick_fixes(command, &expected_map).await, &actions);
    });
}

#[test]
fn pwsh_feedback_providers_unix_cmd_not_found_returns_undefined_when_output_does_not_match() {
    run_with_faked_timers(async {
        let f = Fixture::new();
        let expected_map = pwsh_setup(&f, pwsh_unix_command_not_found_error());
        let command = create_command(PWSH_COMMAND, "invalid output", Some(pwsh_unix_command_not_found_error_output_regex()), Some(128.0), None);
        assert!(f.quick_fixes(command, &expected_map).await.is_none());
    });
}

#[test]
fn pwsh_feedback_providers_unix_cmd_not_found_returns_actions_when_output_matches() {
    run_with_faked_timers(async {
        let f = Fixture::new();
        let expected_map = pwsh_setup(&f, pwsh_unix_command_not_found_error());
        let actions: Vec<TestAction> = ["sudo apt install python3", "sudo apt install python", "sudo apt install python-minimal", "python3"]
            .iter()
            .map(|command| run_action("Pwsh Unix Command Not Found Error", command))
            .collect();
        let command = create_command(PWSH_COMMAND, &pwsh_unix_output(), Some(pwsh_unix_command_not_found_error_output_regex()), Some(128.0), None);
        assert_match_options(f.quick_fixes(command, &expected_map).await, &actions);
    });
}
