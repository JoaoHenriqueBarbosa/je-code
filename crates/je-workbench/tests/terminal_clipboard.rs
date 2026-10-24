//! Porte de `src/vs/workbench/contrib/terminalContrib/clipboard/test/browser/terminalClipboard.test.ts` (suíte `TerminalClipboard`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - **`setup` e `ensureNoDisposablesAreLeakedInTestSuite()`.** O [`Fixture`] cria o guarda de vazamentos antes de tudo, o
//!   `TestConfigurationService` com `terminal.integrated.enableMultiLinePasteWarning` igual a `'auto'` e o `TestDialogService`
//!   com o `promptResult` `{ result: { confirmed: false } }`.
//! - **`instantiationService.invokeFunction(shouldPasteTerminalText, text, bracketedPasteMode)`.** É [`Fixture::should_paste`],
//!   que passa a configuração e o diálogo atuais (o `accessor.get` do original).
//! - **`setConfigValue(value)`.** É [`Fixture::set_config_value`]: troca o serviço de configuração por um novo (o
//!   `instantiationService.stub` de novo).
//! - **`JSON.stringify(...)` com `strictEqual`.** Compara o [`ShouldPasteTerminalText`] direto (`Paste(bool)` ou `ModifiedText`).
//! - **Suíte `TerminalClipboardContribution.paste` (4 casos).** O [`ContributionFixture`] cria o guarda de vazamentos, o
//!   `TestConfigurationService` com `enableMultiLinePasteWarning` igual a `'never'`, o [`TestDialogService`] sem resultado, o
//!   [`TestClipboardService`] (o de `platform/clipboard/test/common/testClipboardService.ts`), o `ITerminalConfigurationService`
//!   vazio (`upcastPartial({})`, o [`EmptyTerminalConfigurationService`]) e a contribuição. O `ctx` do original só tem `instance`
//!   (com `focus`); o `TerminalContributionContext` do porte exige também `processManager` e `widgetManager`, que são o
//!   `PartialProcessInfo` e um `TerminalWidgetManager::new()` (a contribuição não os lê). O `xterm` do `xtermReady` é o
//!   [`StubXterm`] sobre um `FakeXterm` (`onSelectionChange`, `bracketedPasteMode` falso e `paste`).
//! - **`await contribution.paste()` e `pasted`.** O `paste` devolve uma promessa; `pasted` é o último texto de `onDidPaste`.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod support;

use std::cell::RefCell;
use std::rc::Rc;

use common::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite, run_with_faked_timers};
use je_base::dynamic::Dynamic;
use je_base::event::Event;
use je_base::JsString;
use je_base::promise::Promise;
use je_base::uri::URI;
use je_editor::services::clipboard_service::IClipboardService;
use je_platform::configuration::service::IConfigurationService;
use je_platform::configuration::test_configuration_service::TestConfigurationService;
use je_platform::dialogs::{Confirmation, ConfirmationResult, IDialogService, Input, InputResult, Prompt, PromptResult};
use je_platform::resources::uri_file;
use je_platform::terminal::capabilities::capabilities::ITerminalCommand;
use je_platform::terminal::terminal::TerminalLocation;
use je_workbench::contrib::terminal::browser::terminal::{IXtermCore, ITerminalConfigurationService, ITerminalInstance, ITerminalPanelContainer};
use je_workbench::contrib::terminal::browser::terminal_extensions::{
    ITerminalContribution, IXtermRawTerminal, IXtermTerminal, TerminalContributionContext, TerminalContributionInstance, TerminalContributionProcess,
};
use je_workbench::contrib::terminal::browser::widgets::widget_manager::TerminalWidgetManager;
use je_workbench::contrib::terminal::common::terminal::TerminalFont;
use je_workbench::contrib::terminal_contrib::clipboard::terminal_clipboard::{ShouldPasteTerminalText, should_paste_terminal_text};
use je_workbench::contrib::terminal_contrib::clipboard::terminal_clipboard_contribution::TerminalClipboardContribution;
use support::fake_xterm::FakeXterm;
use support::terminal_contrib_support::{PartialProcessInfo, PartialTerminalInstance, unmocked};

/// `TestDialogService(undefined, { result: { confirmed: false } })`: o `prompt` resolve o resultado dado.
struct TestDialogService {
    prompt_result: PromptResult,
}

impl IDialogService for TestDialogService {
    fn on_will_show_dialog(&self) -> Event<()> {
        Event::none()
    }

    fn on_did_show_dialog(&self) -> Event<()> {
        Event::none()
    }

    fn confirm(&self, _confirmation: Confirmation) -> Promise<ConfirmationResult> {
        Promise::resolved(ConfirmationResult { checkbox_checked: None, confirmed: false })
    }

    fn prompt(&self, _prompt: Prompt) -> Promise<PromptResult> {
        Promise::resolved(self.prompt_result.clone())
    }

    fn input(&self, _input: Input) -> Promise<InputResult> {
        Promise::resolved(InputResult { confirmation: ConfirmationResult { checkbox_checked: None, confirmed: true }, values: Some(Vec::new()) })
    }

    fn info(&self, _message: &str, _detail: Option<&str>) -> Promise<()> {
        Promise::resolved(())
    }

    fn warn(&self, _message: &str, _detail: Option<&str>) -> Promise<()> {
        Promise::resolved(())
    }

    fn error(&self, _message: &str, _detail: Option<&str>) -> Promise<()> {
        Promise::resolved(())
    }

    fn about(&self) -> Promise<()> {
        Promise::resolved(())
    }
}

/// `new TestConfigurationService({ [TerminalSettingId.EnableMultiLinePasteWarning]: value })`.
fn configuration_with(value: Dynamic) -> Rc<dyn IConfigurationService> {
    TestConfigurationService::new(Some(Dynamic::object([("terminal.integrated.enableMultiLinePasteWarning", value)])))
}

/// O `setup` da suíte `shouldPasteTerminalText`.
struct Fixture {
    configuration_service: RefCell<Rc<dyn IConfigurationService>>,
    dialog_service: Rc<dyn IDialogService>,
    // Declarado por último: o `Drop` dele descarta o que o `Fixture` registrou e confere os vazamentos.
    _guard: LeakGuard,
}

impl Fixture {
    fn new() -> Self {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        Self {
            configuration_service: RefCell::new(configuration_with(Dynamic::string("auto"))),
            dialog_service: Rc::new(TestDialogService {
                prompt_result: PromptResult { checkbox_checked: None, result: Some(Dynamic::object([("confirmed", Dynamic::Bool(false))])) },
            }),
            _guard: guard,
        }
    }

    /// `setConfigValue(value)`.
    fn set_config_value(&self, value: Dynamic) {
        *self.configuration_service.borrow_mut() = configuration_with(value);
    }

    /// `instantiationService.invokeFunction(shouldPasteTerminalText, text, bracketedPasteMode)`.
    async fn should_paste(&self, text: &str, bracketed_paste_mode: Option<bool>) -> ShouldPasteTerminalText {
        let configuration_service = self.configuration_service.borrow().clone();
        should_paste_terminal_text(configuration_service, self.dialog_service.clone(), text, bracketed_paste_mode).await.unwrap()
    }
}

fn paste(value: bool) -> ShouldPasteTerminalText {
    ShouldPasteTerminalText::Paste(value)
}

fn modified(text: &str) -> ShouldPasteTerminalText {
    ShouldPasteTerminalText::ModifiedText(text.to_owned())
}

// ---------------------------------------------------------------------------------------------------------------------
// suite('shouldPasteTerminalText')
// ---------------------------------------------------------------------------------------------------------------------

#[test]
fn single_line_string() {
    run_with_faked_timers(async {
        let f = Fixture::new();
        assert_eq!(f.should_paste("foo", None).await, paste(true));

        f.set_config_value(Dynamic::string("always"));
        assert_eq!(f.should_paste("foo", None).await, paste(true));

        f.set_config_value(Dynamic::string("never"));
        assert_eq!(f.should_paste("foo", None).await, paste(true));
    });
}

#[test]
fn single_line_string_with_trailing_new_line() {
    run_with_faked_timers(async {
        let f = Fixture::new();
        // Auto: strip the trailing newline to avoid auto-executing the command on paste
        // (mitigates clipboard hijack auto-exec via right-click paste).
        assert_eq!(f.should_paste("foo\n", None).await, modified("foo"));
        assert_eq!(f.should_paste("foo\n ", None).await, modified("foo"));
        assert_eq!(f.should_paste("foo\n\t", None).await, modified("foo"));
        // Auto with bracketed paste mode: shell handles newline literally, safe to paste as-is.
        assert_eq!(f.should_paste("foo\n", Some(true)).await, paste(true));
        assert_eq!(f.should_paste("foo\n ", Some(true)).await, paste(true));
        assert_eq!(f.should_paste("foo\n\t", Some(true)).await, paste(true));

        f.set_config_value(Dynamic::string("always"));
        assert_eq!(f.should_paste("foo\n", None).await, paste(false));

        f.set_config_value(Dynamic::string("never"));
        assert_eq!(f.should_paste("foo\n", None).await, paste(true));
    });
}

#[test]
fn multi_line_string() {
    run_with_faked_timers(async {
        let f = Fixture::new();
        assert_eq!(f.should_paste("foo\nbar", None).await, paste(false));

        f.set_config_value(Dynamic::string("always"));
        assert_eq!(f.should_paste("foo\nbar", None).await, paste(false));

        f.set_config_value(Dynamic::string("never"));
        assert_eq!(f.should_paste("foo\nbar", None).await, paste(true));
    });
}

#[test]
fn bracketed_paste_mode() {
    run_with_faked_timers(async {
        let f = Fixture::new();
        assert_eq!(f.should_paste("foo\nbar", Some(true)).await, paste(true));

        f.set_config_value(Dynamic::string("always"));
        assert_eq!(f.should_paste("foo\nbar", Some(true)).await, paste(false));

        f.set_config_value(Dynamic::string("never"));
        assert_eq!(f.should_paste("foo\nbar", Some(true)).await, paste(true));
    });
}

#[test]
fn legacy_config() {
    run_with_faked_timers(async {
        let f = Fixture::new();
        f.set_config_value(Dynamic::Bool(true)); // 'auto'
        assert_eq!(f.should_paste("foo\nbar", None).await, paste(false));
        assert_eq!(f.should_paste("foo\nbar", Some(true)).await, paste(true));

        f.set_config_value(Dynamic::Bool(false)); // 'never'
        assert_eq!(f.should_paste("foo\nbar", Some(true)).await, paste(true));
    });
}

#[test]
fn invalid_config() {
    run_with_faked_timers(async {
        let f = Fixture::new();
        f.set_config_value(Dynamic::Number(123.0));
        assert_eq!(f.should_paste("foo\nbar", None).await, paste(false));
        assert_eq!(f.should_paste("foo\nbar", Some(true)).await, paste(true));
    });
}

// ---------------------------------------------------------------------------------------------------------------------
// suite('TerminalClipboardContribution.paste')
// ---------------------------------------------------------------------------------------------------------------------

/// `TestClipboardService` de `platform/clipboard/test/common/testClipboardService.ts`: guarda o texto e os recursos escritos.
#[derive(Default)]
struct TestClipboardService {
    text: RefCell<Option<String>>,
    find_text: RefCell<Option<JsString>>,
    resources: RefCell<Option<Vec<URI>>>,
}

impl IClipboardService for TestClipboardService {
    fn write_text(&self, text: &str) -> Promise<()> {
        *self.text.borrow_mut() = Some(text.to_owned());
        Promise::resolved(())
    }

    fn read_text(&self) -> Promise<String> {
        Promise::resolved(self.text.borrow().clone().unwrap_or_default())
    }

    fn read_find_text(&self) -> Promise<JsString> {
        Promise::resolved(self.find_text.borrow().clone().unwrap_or_default())
    }

    fn write_find_text(&self, text: &JsString) -> Promise<()> {
        *self.find_text.borrow_mut() = Some(text.clone());
        Promise::resolved(())
    }

    fn write_resources(&self, resources: Vec<URI>) -> Promise<()> {
        *self.resources.borrow_mut() = Some(resources);
        Promise::resolved(())
    }

    fn read_resources(&self) -> Promise<Vec<URI>> {
        Promise::resolved(self.resources.borrow().clone().unwrap_or_default())
    }

    fn has_resources(&self) -> Promise<bool> {
        Promise::resolved(self.resources.borrow().as_ref().is_some_and(|resources| !resources.is_empty()))
    }
}

/// `upcastPartial<ITerminalConfigurationService>({})`.
struct EmptyTerminalConfigurationService;

impl ITerminalConfigurationService for EmptyTerminalConfigurationService {
    fn config(&self) -> Dynamic {
        unmocked("config")
    }

    fn default_location(&self) -> TerminalLocation {
        unmocked("defaultLocation")
    }

    fn on_config_changed(&self) -> Event<()> {
        unmocked("onConfigChanged")
    }

    fn set_panel_container(&self, _panel_container: Rc<dyn ITerminalPanelContainer>) {
        unmocked("setPanelContainer")
    }

    fn config_font_is_monospace(&self) -> bool {
        unmocked("configFontIsMonospace")
    }

    fn get_font(&self, _device_pixel_ratio: f64, _xterm_core: Option<&dyn IXtermCore>, _exclude_dimensions: bool) -> TerminalFont {
        unmocked("getFont")
    }

    fn should_command_skip_shell(&self, _command_id: &str) -> bool {
        unmocked("shouldCommandSkipShell")
    }
}

/// O `xterm` do `xtermReady`: `{ onDidRequestCopyAsHtml: Event.None, raw: { onSelectionChange: Event.None, modes: { bracketedPasteMode: false }, paste } }`.
struct StubXterm {
    raw: Rc<FakeXterm>,
}

impl IXtermTerminal for StubXterm {
    fn raw(&self) -> Rc<dyn IXtermRawTerminal> {
        self.raw.clone()
    }

    fn on_did_request_copy_as_html(&self) -> Event<Rc<dyn ITerminalCommand>> {
        Event::none()
    }

    fn copy_selection(&self, _as_html: bool, _command: Option<Rc<dyn ITerminalCommand>>) {
        unmocked("copySelection")
    }
}

/// O `setup` da suíte `TerminalClipboardContribution.paste`.
struct ContributionFixture {
    clipboard_service: Rc<TestClipboardService>,
    contribution: Rc<TerminalClipboardContribution>,
    /// `pasted`.
    pasted: Rc<RefCell<Option<String>>>,
    // Declarado por último: o `Drop` dele descarta o que o `ContributionFixture` registrou e confere os vazamentos.
    _guard: LeakGuard,
}

impl ContributionFixture {
    fn new() -> Self {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let configuration_service: Rc<dyn IConfigurationService> =
            TestConfigurationService::new(Some(Dynamic::object([("terminal.integrated.enableMultiLinePasteWarning", Dynamic::string("never"))])));
        let dialog_service: Rc<dyn IDialogService> = Rc::new(TestDialogService { prompt_result: PromptResult { checkbox_checked: None, result: None } });
        let clipboard_service = Rc::new(TestClipboardService::default());

        // `ctx = { instance: { focus } }`
        let context = TerminalContributionContext {
            instance: TerminalContributionInstance::Full(Rc::new(PartialTerminalInstance::default()) as Rc<dyn ITerminalInstance>),
            process_manager: TerminalContributionProcess::Info(Rc::new(PartialProcessInfo { initial_cwd: String::new() })),
            widget_manager: TerminalWidgetManager::new(),
        };
        let contribution = guard.add_rc(TerminalClipboardContribution::new(
            context,
            clipboard_service.clone(),
            configuration_service,
            dialog_service,
            Rc::new(EmptyTerminalConfigurationService),
        ));
        contribution.xterm_ready(Rc::new(StubXterm { raw: guard.add_rc(FakeXterm::new(80, 30)) }));

        let pasted: Rc<RefCell<Option<String>>> = Rc::default();
        let last_pasted = pasted.clone();
        guard.add(contribution.on_did_paste().listen(move |text| *last_pasted.borrow_mut() = Some(text.clone())));
        Self { clipboard_service, contribution, pasted, _guard: guard }
    }
}

#[test]
fn clipboard_text() {
    run_with_faked_timers(async {
        let f = ContributionFixture::new();
        f.clipboard_service.write_text("hello").await.unwrap();
        f.clipboard_service.write_resources(vec![uri_file("/should/not/be/used")]).await.unwrap();

        f.contribution.paste().await.unwrap();
        assert_eq!(*f.pasted.borrow(), Some("hello".to_owned()));
    });
}

#[test]
fn clipboard_resource_fallback_when_text_is_empty() {
    run_with_faked_timers(async {
        let f = ContributionFixture::new();
        f.clipboard_service.write_resources(vec![uri_file("/foo/bar.png"), uri_file("/foo/baz.png")]).await.unwrap();

        f.contribution.paste().await.unwrap();
        assert_eq!(*f.pasted.borrow(), Some(je_platform::resources::fs_path(&uri_file("/foo/bar.png"))));
    });
}

#[test]
fn clipboard_resource_fallback_ignores_non_file_uris() {
    run_with_faked_timers(async {
        let f = ContributionFixture::new();
        f.clipboard_service.write_resources(vec![URI::parse("https://example.com/foo/bar", false)]).await.unwrap();

        f.contribution.paste().await.unwrap();
        assert_eq!(*f.pasted.borrow(), Some(String::new()));
    });
}

#[test]
fn empty_clipboard() {
    run_with_faked_timers(async {
        let f = ContributionFixture::new();
        f.contribution.paste().await.unwrap();
        assert_eq!(*f.pasted.borrow(), Some(String::new()));
    });
}
