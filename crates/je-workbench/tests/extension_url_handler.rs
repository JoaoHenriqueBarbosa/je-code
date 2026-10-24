//! Porte de `src/vs/workbench/services/extensions/test/browser/extensionUrlHandler.test.ts` (suíte
//! `ExtensionUrlHandler`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - **`ensureNoDisposablesAreLeakedInTestSuite()` e `sinon.restore()`.** O guarda de vazamentos é criado no começo de cada
//!   caso. Os `sinon.stub()` são dublês com contador de chamadas, e não há o que restaurar.
//! - **`createHandler`.** O original cria o objeto com `Object.create(descriptor.ctor.prototype)` e atribui só as
//!   dependências que o `handleURL` lê, sem rodar o construtor. Aqui o construtor roda de verdade (`Injectable::create`) com as
//!   dependências nas mãos: o `extensionService` (`getExtension` resolve `undefined`), o `productService` (sem
//!   `trustedExtensionProtocolHandlers`), o `configurationService` (sem lista de extensões confiáveis) e o `dialogService`
//!   (`confirm`) são os dublês do caso; o armazenamento é o em memória, vazio, que faz o papel do
//!   `userTrustedExtensionsStorage` (`has` sempre falso). `extensionHandlers` é preenchido pelo `registerExtensionHandler`.
//!   O handler é descartado no fim do caso, o que desarma o intervalo de 30 s do construtor.
//! - **`handler.handleURL(uri, options)`** é o `IURLHandler::handle_url`, e `deepStrictEqual` é `assert_eq!` sobre o
//!   [`Outcome`].

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use std::cell::Cell;
use std::rc::Rc;

use common::{ensure_no_disposables_are_leaked_in_test_suite, run_with_faked_timers};
use je_base::dynamic::Dynamic;
use je_base::event::Event;
use je_base::lifecycle::IDisposable;
use je_base::promise::Promise;
use je_base::uri::URI;
use je_extensions::manifest::ExtensionIdentifier;
use je_extensions::scanner::ExtensionDescription;
use je_instantiation::Injectable;
use je_platform::commands::{CommandEvent, ICommandService};
use je_platform::configuration::test_configuration_service::TestConfigurationService;
use je_platform::dialogs::{
    Confirmation, ConfirmationResult, IDialogService, Input, InputResult, Prompt, PromptResult,
};
use je_platform::product::ProductService;
use je_platform::storage::{InMemoryStorageService, StorageService};
use je_platform::url::url_service::{IUrlHandler, NativeUrlService, OpenUrlOptions};
use je_workbench::services::extensions::browser::extension_url_handler::{
    ExtensionUrlHandler, ExtensionUrlHandlerOverrideRegistry, IExtensionContributedUrlHandler, IExtensionUrlHandler,
    IExtensionUrlHandlerOverride,
};
use je_workbench::services::extensions::extensions::{ActivationKind, IExtensionDescription, IExtensionService};
use je_workbench::test::browser::workbench_test_services::TestHostService;
use je_workbench::test::common::workbench_test_services::TestNotificationService;

/// `{ getExtension: sinon.stub().resolves(undefined) }`.
struct StubExtensionService;

impl IExtensionService for StubExtensionService {
    fn get_extension(&self, _id: &str) -> Promise<Option<IExtensionDescription>> {
        Promise::resolved(None)
    }

    fn on_did_register_extensions(&self) -> Event<()> {
        Event::none()
    }

    fn when_installed_extensions_registered(&self) -> Promise<bool> {
        Promise::resolved(true)
    }

    fn extensions(&self) -> Vec<ExtensionDescription> {
        Vec::new()
    }

    fn activate_by_event(&self, _activation_event: &str, _activation_kind: Option<ActivationKind>) -> Promise<()> {
        Promise::resolved(())
    }
}

/// `{ confirm }`: `confirm` é um `sinon.stub().resolves({ confirmed, checkboxChecked: false })`.
struct StubDialogService {
    confirmed: bool,
    confirm_call_count: Cell<usize>,
}

impl IDialogService for StubDialogService {
    fn on_will_show_dialog(&self) -> Event<()> {
        Event::none()
    }

    fn on_did_show_dialog(&self) -> Event<()> {
        Event::none()
    }

    fn confirm(&self, _confirmation: Confirmation) -> Promise<ConfirmationResult> {
        self.confirm_call_count.set(self.confirm_call_count.get() + 1);
        Promise::resolved(ConfirmationResult { checkbox_checked: Some(false), confirmed: self.confirmed })
    }

    fn prompt(&self, _prompt: Prompt) -> Promise<PromptResult> {
        panic!("prompt não foi stubado")
    }

    fn input(&self, _input: Input) -> Promise<InputResult> {
        panic!("input não foi stubado")
    }

    fn info(&self, _message: &str, _detail: Option<&str>) -> Promise<()> {
        panic!("info não foi stubado")
    }

    fn warn(&self, _message: &str, _detail: Option<&str>) -> Promise<()> {
        panic!("warn não foi stubado")
    }

    fn error(&self, _message: &str, _detail: Option<&str>) -> Promise<()> {
        panic!("error não foi stubado")
    }

    fn about(&self) -> Promise<()> {
        panic!("about não foi stubado")
    }
}

/// O `commandService` que o `handleURL` destes casos não chega a usar.
struct UnusedCommandService;

impl ICommandService for UnusedCommandService {
    fn on_will_execute_command(&self) -> Event<CommandEvent> {
        Event::none()
    }

    fn on_did_execute_command(&self) -> Event<CommandEvent> {
        Event::none()
    }

    fn execute_command(&self, command_id: &str, _args: Vec<Dynamic>) -> Promise<Dynamic> {
        panic!("{command_id} não deveria ser executado")
    }
}

/// `{ extensionDisplayName: 'Copilot Chat', handleURL: sinon.stub().resolves(false) }`.
struct StubExtensionHandler;

impl IUrlHandler for StubExtensionHandler {
    fn handle_url(&self, _uri: &URI, _options: Option<&OpenUrlOptions>) -> Promise<bool> {
        Promise::resolved(false)
    }
}

impl IExtensionContributedUrlHandler for StubExtensionHandler {
    fn extension_display_name(&self) -> String {
        "Copilot Chat".to_string()
    }
}

/// `{ canHandleURL: uri => uri.authority === 'github.copilot-chat', handleURL: overrideHandleURL }`.
struct CopilotChatOverride {
    handle_url_call_count: Cell<usize>,
}

impl IExtensionUrlHandlerOverride for CopilotChatOverride {
    fn can_handle_url(&self, uri: &URI) -> bool {
        *uri.authority() == "github.copilot-chat"
    }

    fn handle_url(&self, _uri: &URI) -> Promise<bool> {
        self.handle_url_call_count.set(self.handle_url_call_count.get() + 1);
        Promise::resolved(true)
    }
}

/// `{ handled, confirmCallCount, overrideCallCount }`.
#[derive(Debug, PartialEq, Eq)]
struct Outcome {
    handled: bool,
    confirm_call_count: usize,
    override_call_count: usize,
}

/// `createHandler(confirm, extensionInstalled)` mais o `invokeOverride`.
async fn invoke_override(confirm_result: bool, options: Option<OpenUrlOptions>, extension_installed: bool) -> Outcome {
    let dialog_service = Rc::new(StubDialogService { confirmed: confirm_result, confirm_call_count: Cell::new(0) });
    let product_service = Rc::new(ProductService::default());
    let storage = InMemoryStorageService::new();
    let storage_service: Rc<dyn StorageService> = Rc::new(storage.clone());
    let handler = ExtensionUrlHandler::create(
        (),
        (
            Rc::new(NativeUrlService::new(product_service.clone())),
            Rc::new(StubExtensionService),
            dialog_service.clone(),
            Rc::new(UnusedCommandService),
            Rc::new(TestHostService),
            storage_service,
            TestConfigurationService::new(None),
            Rc::new(TestNotificationService),
            product_service,
        ),
    );
    if extension_installed {
        handler.register_extension_handler(&ExtensionIdentifier::new("github.copilot-chat"), Rc::new(StubExtensionHandler));
    }

    let override_handler = Rc::new(CopilotChatOverride { handle_url_call_count: Cell::new(0) });
    let registration = ExtensionUrlHandlerOverrideRegistry::register_handler(override_handler.clone());
    let handled = handler
        .handle_url(&URI::parse("vscode://github.copilot-chat/?agent=agent&prompt=Approve%20deployment", false), options.as_ref())
        .await;
    registration.dispose();
    handler.dispose();

    Outcome {
        handled: handled.unwrap(),
        confirm_call_count: dialog_service.confirm_call_count.get(),
        override_call_count: override_handler.handle_url_call_count.get(),
    }
}

#[test]
fn extension_url_trust_policy_rejects_override_before_invocation() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    run_with_faked_timers(async move {
        assert_eq!(invoke_override(false, None, true).await, Outcome { handled: true, confirm_call_count: 1, override_call_count: 0 });
    });
}

#[test]
fn extension_url_trust_policy_invokes_override_after_confirmation() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    run_with_faked_timers(async move {
        assert_eq!(invoke_override(true, None, true).await, Outcome { handled: true, confirm_call_count: 1, override_call_count: 1 });
    });
}

#[test]
fn extension_url_trust_policy_invokes_explicitly_trusted_override_without_confirmation() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    run_with_faked_timers(async move {
        assert_eq!(
            invoke_override(false, Some(OpenUrlOptions { trusted: Some(true), original_url: None }), true).await,
            Outcome { handled: true, confirm_call_count: 0, override_call_count: 1 }
        );
    });
}

#[test]
fn extension_url_trust_policy_preserves_approved_overrides_when_the_extension_is_absent() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    run_with_faked_timers(async move {
        assert_eq!(invoke_override(true, None, false).await, Outcome { handled: true, confirm_call_count: 1, override_call_count: 1 });
    });
}
