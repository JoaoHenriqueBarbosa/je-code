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
//! - **Casos fora do porte.** A suíte `TerminalClipboardContribution.paste` (4 casos: `Clipboard text`, `Clipboard resource
//!   fallback when text is empty`, `Clipboard resource fallback ignores non-file URIs` e `Empty clipboard`) exercita a
//!   `TerminalClipboardContribution`, que depende do `ITerminalContribution`/`registerTerminalContribution` de
//!   `contrib/terminal/browser`, inexistente no porte. Sem `#[test]`.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use std::cell::RefCell;
use std::rc::Rc;

use common::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite, run_with_faked_timers};
use je_base::dynamic::Dynamic;
use je_base::event::Event;
use je_base::promise::Promise;
use je_platform::configuration::service::IConfigurationService;
use je_platform::configuration::test_configuration_service::TestConfigurationService;
use je_platform::dialogs::{Confirmation, ConfirmationResult, IDialogService, Input, InputResult, Prompt, PromptResult};
use je_workbench::contrib::terminal_contrib::clipboard::terminal_clipboard::{ShouldPasteTerminalText, should_paste_terminal_text};

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
