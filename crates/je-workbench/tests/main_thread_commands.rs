//! Porte de `src/vs/workbench/api/test/browser/mainThreadCommands.test.ts` (suíte `MainThreadCommands`), caso a caso e
//! na mesma ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` vira o guarda criado no começo de cada caso; o caso `async` roda em
//!   `run_with_faked_timers`.
//! - `SingleProxyRPCProtocol(null)` devolve `null` como proxy de `ExtHostCommands`: em Rust o proxy é tipado, então é um
//!   dublê ([`NullExtHostCommands`]) que falha em qualquer chamada, como o `null` lançaria `TypeError`.
//! - `undefined!` no lugar do `ICommandService` (casos 1 e 2, que nunca o usam) é o `MockCommandService` sem
//!   `executeCommand` (`support/mock_command_service.rs`), que falha se for chamado. `new class extends mock<X>() { override ... }`
//!   vira `MockCommandService` e [`MockExtensionService`] com fechos opcionais: um método sem `override` falha (`panic!`), como o método ausente do
//!   objeto vazio do original lançaria `TypeError`.
//! - `commands.$executeCommand(...)` é `rpc_execute_command(...)` (o `$` dos métodos de RPC vira `rpc_`).

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod support;

use std::cell::RefCell;
use std::rc::Rc;

use common::{ensure_no_disposables_are_leaked_in_test_suite, run_with_faked_timers};
use je_base::dynamic::Dynamic;
use je_base::event::Event;
use je_base::lifecycle::IDisposable;
use je_base::promise::Promise;
use je_extensions::scanner::ExtensionDescription;
use je_platform::commands::commands::commands_registry;
use je_workbench::api::browser::main_thread_commands::MainThreadCommands;
use je_workbench::api::common::ext_host_protocol::{CommandArgs, ExtHostCommandsShape, ICommandMetadataDto, MainThreadCommandsShape};
use je_workbench::services::extensions::extensions::{ActivationKind, IExtensionDescription, IExtensionService};
use support::mock_command_service::MockCommandService;
use support::test_rpc_protocol::single_proxy_rpc_protocol;

/// O `null` de `SingleProxyRPCProtocol(null)`.
struct NullExtHostCommands;

impl ExtHostCommandsShape for NullExtHostCommands {
    fn rpc_execute_contributed_command(&self, _id: &str, _args: Vec<Dynamic>) -> Promise<Dynamic> {
        panic!("Cannot read properties of null (reading '$executeContributedCommand')")
    }

    fn rpc_get_contributed_command_metadata(&self) -> Promise<Vec<(String, ICommandMetadataDto)>> {
        panic!("Cannot read properties of null (reading '$getContributedCommandMetadata')")
    }
}

type ActivateByEventFn = Box<dyn Fn(&str) -> Promise<()>>;

/// `new class extends mock<IExtensionService>() { override activateByEvent(...) }`.
#[derive(Default)]
struct MockExtensionService {
    activate_by_event: Option<ActivateByEventFn>,
}

impl IExtensionService for MockExtensionService {
    fn activate_by_event(&self, activation_event: &str, _activation_kind: Option<ActivationKind>) -> Promise<()> {
        match &self.activate_by_event {
            Some(activate_by_event) => activate_by_event(activation_event),
            None => panic!("activateByEvent is not a function"),
        }
    }

    fn activation_event_is_done(&self, _activation_event: &str) -> bool {
        panic!("activationEventIsDone is not a function")
    }

    fn get_extension(&self, _id: &str) -> Promise<Option<IExtensionDescription>> {
        panic!("getExtension is not a function")
    }

    fn on_did_register_extensions(&self) -> Event<()> {
        panic!("onDidRegisterExtensions is not defined")
    }

    fn when_installed_extensions_registered(&self) -> Promise<bool> {
        panic!("whenInstalledExtensionsRegistered is not a function")
    }

    fn extensions(&self) -> Vec<ExtensionDescription> {
        panic!("extensions is not defined")
    }
}

/// `new MainThreadCommands(SingleProxyRPCProtocol(null), commandService, extensionService)`.
fn create_main_thread_commands(command_service: MockCommandService, extension_service: MockExtensionService) -> Rc<MainThreadCommands> {
    let proxy: Rc<dyn ExtHostCommandsShape> = Rc::new(NullExtHostCommands);
    MainThreadCommands::new(&*single_proxy_rpc_protocol(proxy), Rc::new(command_service), Rc::new(extension_service))
}

#[test]
fn dispose_on_unregister() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    let commands = create_main_thread_commands(MockCommandService::default(), MockExtensionService::default());
    assert!(commands_registry().get_command("foo").is_none());

    // register
    commands.rpc_register_command("foo");
    assert!(commands_registry().get_command("foo").is_some());

    // unregister
    commands.rpc_unregister_command("foo");
    assert!(commands_registry().get_command("foo").is_none());

    commands.dispose();
}

#[test]
fn unregister_all_on_dispose() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    let commands = create_main_thread_commands(MockCommandService::default(), MockExtensionService::default());
    assert!(commands_registry().get_command("foo").is_none());

    commands.rpc_register_command("foo");
    commands.rpc_register_command("bar");

    assert!(commands_registry().get_command("foo").is_some());
    assert!(commands_registry().get_command("bar").is_some());

    commands.dispose();

    assert!(commands_registry().get_command("foo").is_none());
    assert!(commands_registry().get_command("bar").is_none());
}

#[test]
fn activate_and_throw_when_needed() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    run_with_faked_timers(async move {
        let activations: Rc<RefCell<Vec<String>>> = Rc::default();
        let runs: Rc<RefCell<Vec<String>>> = Rc::default();

        let run_sink = runs.clone();
        let activation_sink = activations.clone();
        let commands = create_main_thread_commands(
            MockCommandService {
                execute_command: Some(Box::new(move |id| {
                    run_sink.borrow_mut().push(id.to_owned());
                    Promise::resolved(Dynamic::Undefined)
                })),
            },
            MockExtensionService {
                activate_by_event: Some(Box::new(move |id| {
                    activation_sink.borrow_mut().push(id.to_owned());
                    Promise::resolved(())
                })),
            },
        );

        // case 1: arguments and retry
        activations.borrow_mut().clear();
        let object = Dynamic::object([("n", Dynamic::Number(3.0))]);
        let args = CommandArgs::Plain(vec![Dynamic::Number(1.0), Dynamic::Number(2.0), object]);
        match commands.rpc_execute_command("bazz", args, true).await {
            Ok(_) => panic!("o $executeCommand deveria rejeitar com $executeCommand:retry"),
            Err(error) => {
                assert_eq!(*activations.borrow(), vec!["onCommand:bazz".to_owned()]);
                assert_eq!(error.message, "$executeCommand:retry");
            }
        }

        // case 2: no arguments and retry
        runs.borrow_mut().clear();
        commands.rpc_execute_command("bazz", CommandArgs::Plain(vec![]), true).await.expect("case 2");
        assert_eq!(*runs.borrow(), vec!["bazz".to_owned()]);

        // case 3: arguments and no retry
        runs.borrow_mut().clear();
        let args = CommandArgs::Plain(vec![Dynamic::Number(1.0), Dynamic::Number(2.0), Dynamic::Bool(true)]);
        commands.rpc_execute_command("bazz", args, false).await.expect("case 3");
        assert_eq!(*runs.borrow(), vec!["bazz".to_owned()]);

        commands.dispose();
    });
}
