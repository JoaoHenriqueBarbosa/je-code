//! Porte de `src/vs/workbench/api/test/common/extHostTerminalService.test.ts` (suíte `ExtHostTerminalService`), caso a caso e na
//! mesma ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` vira o guarda criado no começo de cada caso (`store`), e o corpo roda em
//!   `run_with_faked_timers`.
//! - `new TestRPCProtocol()` é o [`TestRpcProtocol`] (`support/test_rpc_protocol_deferred.rs`): o [`MainThreadProxy`] encaminha cada
//!   método de `MainThreadTerminalServiceShape` por `remote_call` (adiado por uma macrotarefa, como o `Proxy` do original) para o
//!   objeto que `set` registrou, e `await rpcProtocol.sync()` é o `sync()` do protocolo de teste.
//! - `new class extends mock<MainThreadTerminalServiceShape>() { ... }` é o [`MockMainThreadTerminalService`]: só os métodos que o
//!   caso sobrescreve existem, e qualquer outro falha (o `TypeError` de método ausente do `mock`). `processExitCalls++` é o
//!   contador `process_exit_calls`.
//! - `new class extends mock<ExtHostCommands>() { registerArgumentProcessor() {} }` é o [`MockCommands`] e a
//!   `IExtHostInitDataService` com `remote.authority` é o [`MockInitData`].
//! - `{} as IShellLaunchConfigDto` é `IShellLaunchConfigDto::default()`; `Pseudoterminal` do segundo caso é o [`TestPty`], cujo
//!   `close() {}` não faz nada e cujos `handleInput` e `open` fazem o que o literal faz. `closeEmitter.fire()` (`Event<number |
//!   void>`) dispara `None`.
//! - `store.add(terminal)` do original registra o `ExtHostTerminal` no guarda; o `store.add(token.onCancellationRequested(...))` do
//!   provedor de links registra o ouvinte no store do guarda (o [`LinkProvider`] guarda uma referência a ele).

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

#[path = "support/test_rpc_protocol_deferred.rs"]
mod test_rpc_protocol_deferred;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use common::{ensure_no_disposables_are_leaked_in_test_suite, run_with_faked_timers};
use je_base::cancellation::CancellationToken;
use je_base::event::{Emitter, Event};
use je_base::lifecycle::{DisposableStore, IDisposable};
use je_base::promise::{MaybePromise, Promise};
use je_platform::terminal::environment_variable::{ISerializableEnvironmentDescriptionMap, ISerializableEnvironmentVariableCollection};
use je_platform::terminal::terminal::{IProcessProperty, IProcessReadyWindowsPty, TerminalExitReason as PlatformExitReason};
use je_workbench::api::common::ext_host_commands::ArgumentProcessor;
use je_workbench::api::common::ext_host_protocol::{
    ExtHostTerminalIdentifier, ExtHostTerminalServiceShape, IShellLaunchConfigDto, MainContext, MainThreadTerminalServiceShape, TerminalLaunchConfig,
};
use je_workbench::api::common::ext_host_terminal_service::{
    IExtHostCommands, IExtHostInitDataService, Pseudoterminal, TerminalDimensions, TerminalLinkContext, TerminalLinkProvider,
    WorkerExtHostTerminalService,
};
use je_workbench::api::common::ext_host_type_converters::ICommandsConverter;
use je_workbench::api::common::ext_host_types::TerminalLink;
use test_rpc_protocol_deferred::TestRpcProtocol;

/// O que um método de `mock<T>()` que ninguém sobrescreveu faz: lançar.
fn unmocked(name: &str) -> ! {
    panic!("{name} is not a function")
}

/// `new class extends mock<MainThreadTerminalServiceShape>() { ... }`.
#[derive(Default)]
struct MockMainThreadTerminalService {
    process_exit_calls: Cell<u32>,
}

impl IDisposable for MockMainThreadTerminalService {
    fn dispose(&self) {}
}

impl MainThreadTerminalServiceShape for MockMainThreadTerminalService {
    fn rpc_create_terminal(&self, _ext_host_terminal_id: &str, _config: TerminalLaunchConfig) -> Promise<()> {
        unmocked("$createTerminal")
    }
    fn rpc_dispose(&self, _id: &ExtHostTerminalIdentifier) {
        unmocked("$dispose")
    }
    fn rpc_hide(&self, _id: &ExtHostTerminalIdentifier) {
        unmocked("$hide")
    }
    fn rpc_send_text(&self, _id: &ExtHostTerminalIdentifier, _text: &str, _should_execute: bool) {
        unmocked("$sendText")
    }
    fn rpc_show(&self, _id: &ExtHostTerminalIdentifier, _preserve_focus: bool) {
        unmocked("$show")
    }
    fn rpc_register_process_support(&self, _is_supported: bool) {}
    fn rpc_register_profile_provider(&self, _id: &str, _extension_identifier: &str) {
        unmocked("$registerProfileProvider")
    }
    fn rpc_unregister_profile_provider(&self, _id: &str) {
        unmocked("$unregisterProfileProvider")
    }
    fn rpc_register_completion_provider(&self, _id: &str, _extension_identifier: &str, _trigger_characters: &[String]) {
        unmocked("$registerCompletionProvider")
    }
    fn rpc_unregister_completion_provider(&self, _id: &str) {
        unmocked("$unregisterCompletionProvider")
    }
    fn rpc_register_quick_fix_provider(&self, _id: &str, _extension_identifier: &str) {
        unmocked("$registerQuickFixProvider")
    }
    fn rpc_unregister_quick_fix_provider(&self, _id: &str) {
        unmocked("$unregisterQuickFixProvider")
    }
    fn rpc_set_environment_variable_collection(
        &self,
        _extension_identifier: &str,
        _persistent: bool,
        _collection: Option<ISerializableEnvironmentVariableCollection>,
        _description_map: ISerializableEnvironmentDescriptionMap,
    ) {
        unmocked("$setEnvironmentVariableCollection")
    }
    fn rpc_start_sending_data_events(&self) {
        unmocked("$startSendingDataEvents")
    }
    fn rpc_stop_sending_data_events(&self) {
        unmocked("$stopSendingDataEvents")
    }
    fn rpc_start_sending_command_events(&self) {
        unmocked("$startSendingCommandEvents")
    }
    fn rpc_stop_sending_command_events(&self) {
        unmocked("$stopSendingCommandEvents")
    }
    fn rpc_start_link_provider(&self) {}
    fn rpc_stop_link_provider(&self) {}
    fn rpc_send_process_data(&self, _terminal_id: u32, _data: &str) {
        unmocked("$sendProcessData")
    }
    fn rpc_send_process_ready(&self, _terminal_id: u32, _pid: i32, _cwd: &str, _windows_pty: Option<IProcessReadyWindowsPty>) {}
    fn rpc_send_process_property(&self, _terminal_id: u32, _property: IProcessProperty) {
        unmocked("$sendProcessProperty")
    }
    fn rpc_send_process_exit(&self, _terminal_id: u32, _exit_code: Option<i32>) {
        self.process_exit_calls.set(self.process_exit_calls.get() + 1);
    }
}

/// O proxy de `MainContext.MainThreadTerminalService`: cada método vira uma chamada remota no objeto que `set` registrou.
struct MainThreadProxy {
    rpc: Rc<TestRpcProtocol>,
}

impl MainThreadProxy {
    /// Uma chamada sem retorno: adiada por uma macrotarefa e rodada no objeto local.
    fn call(&self, call: impl FnOnce(Rc<dyn MainThreadTerminalServiceShape>) + 'static) {
        let rpc = Rc::clone(&self.rpc);
        drop(self.rpc.remote_call(move || {
            call(rpc.local(&MainContext::MAIN_THREAD_TERMINAL_SERVICE));
            Promise::resolved(())
        }));
    }
}

impl IDisposable for MainThreadProxy {
    fn dispose(&self) {}
}

impl MainThreadTerminalServiceShape for MainThreadProxy {
    fn rpc_create_terminal(&self, ext_host_terminal_id: &str, config: TerminalLaunchConfig) -> Promise<()> {
        let rpc = Rc::clone(&self.rpc);
        let id = ext_host_terminal_id.to_owned();
        self.rpc.remote_call(move || rpc.local(&MainContext::MAIN_THREAD_TERMINAL_SERVICE).rpc_create_terminal(&id, config))
    }
    fn rpc_dispose(&self, id: &ExtHostTerminalIdentifier) {
        let id = id.clone();
        self.call(move |local| local.rpc_dispose(&id));
    }
    fn rpc_hide(&self, id: &ExtHostTerminalIdentifier) {
        let id = id.clone();
        self.call(move |local| local.rpc_hide(&id));
    }
    fn rpc_send_text(&self, id: &ExtHostTerminalIdentifier, text: &str, should_execute: bool) {
        let (id, text) = (id.clone(), text.to_owned());
        self.call(move |local| local.rpc_send_text(&id, &text, should_execute));
    }
    fn rpc_show(&self, id: &ExtHostTerminalIdentifier, preserve_focus: bool) {
        let id = id.clone();
        self.call(move |local| local.rpc_show(&id, preserve_focus));
    }
    fn rpc_register_process_support(&self, is_supported: bool) {
        self.call(move |local| local.rpc_register_process_support(is_supported));
    }
    fn rpc_register_profile_provider(&self, id: &str, extension_identifier: &str) {
        let (id, extension_identifier) = (id.to_owned(), extension_identifier.to_owned());
        self.call(move |local| local.rpc_register_profile_provider(&id, &extension_identifier));
    }
    fn rpc_unregister_profile_provider(&self, id: &str) {
        let id = id.to_owned();
        self.call(move |local| local.rpc_unregister_profile_provider(&id));
    }
    fn rpc_register_completion_provider(&self, id: &str, extension_identifier: &str, trigger_characters: &[String]) {
        let (id, extension_identifier, trigger_characters) = (id.to_owned(), extension_identifier.to_owned(), trigger_characters.to_vec());
        self.call(move |local| local.rpc_register_completion_provider(&id, &extension_identifier, &trigger_characters));
    }
    fn rpc_unregister_completion_provider(&self, id: &str) {
        let id = id.to_owned();
        self.call(move |local| local.rpc_unregister_completion_provider(&id));
    }
    fn rpc_register_quick_fix_provider(&self, id: &str, extension_identifier: &str) {
        let (id, extension_identifier) = (id.to_owned(), extension_identifier.to_owned());
        self.call(move |local| local.rpc_register_quick_fix_provider(&id, &extension_identifier));
    }
    fn rpc_unregister_quick_fix_provider(&self, id: &str) {
        let id = id.to_owned();
        self.call(move |local| local.rpc_unregister_quick_fix_provider(&id));
    }
    fn rpc_set_environment_variable_collection(
        &self,
        extension_identifier: &str,
        persistent: bool,
        collection: Option<ISerializableEnvironmentVariableCollection>,
        description_map: ISerializableEnvironmentDescriptionMap,
    ) {
        let extension_identifier = extension_identifier.to_owned();
        self.call(move |local| local.rpc_set_environment_variable_collection(&extension_identifier, persistent, collection, description_map));
    }
    fn rpc_start_sending_data_events(&self) {
        self.call(|local| local.rpc_start_sending_data_events());
    }
    fn rpc_stop_sending_data_events(&self) {
        self.call(|local| local.rpc_stop_sending_data_events());
    }
    fn rpc_start_sending_command_events(&self) {
        self.call(|local| local.rpc_start_sending_command_events());
    }
    fn rpc_stop_sending_command_events(&self) {
        self.call(|local| local.rpc_stop_sending_command_events());
    }
    fn rpc_start_link_provider(&self) {
        self.call(|local| local.rpc_start_link_provider());
    }
    fn rpc_stop_link_provider(&self) {
        self.call(|local| local.rpc_stop_link_provider());
    }
    fn rpc_send_process_data(&self, terminal_id: u32, data: &str) {
        let data = data.to_owned();
        self.call(move |local| local.rpc_send_process_data(terminal_id, &data));
    }
    fn rpc_send_process_ready(&self, terminal_id: u32, pid: i32, cwd: &str, windows_pty: Option<IProcessReadyWindowsPty>) {
        let cwd = cwd.to_owned();
        self.call(move |local| local.rpc_send_process_ready(terminal_id, pid, &cwd, windows_pty));
    }
    fn rpc_send_process_property(&self, terminal_id: u32, property: IProcessProperty) {
        self.call(move |local| local.rpc_send_process_property(terminal_id, property));
    }
    fn rpc_send_process_exit(&self, terminal_id: u32, exit_code: Option<i32>) {
        self.call(move |local| local.rpc_send_process_exit(terminal_id, exit_code));
    }
}

/// `new class extends mock<ExtHostCommands>() { override registerArgumentProcessor(_processor) { } }`.
struct MockCommands;

impl IExtHostCommands for MockCommands {
    fn register_argument_processor(&self, _processor: Rc<dyn ArgumentProcessor>) {}

    fn converter(&self) -> Rc<dyn ICommandsConverter> {
        unmocked("converter")
    }
}

/// `new class extends mock<IExtHostInitDataService>() { override readonly remote = { authority: 'test+remote', ... } }`.
struct MockInitData;

impl IExtHostInitDataService for MockInitData {
    fn remote_authority(&self) -> Option<String> {
        Some("test+remote".to_owned())
    }
}

/// O `vscode.TerminalLinkProvider` do primeiro caso: só resolve quando o token de cancelamento dispara.
struct LinkProvider {
    store: Rc<DisposableStore>,
    provider_token_cancelled: Rc<Cell<bool>>,
    handled_after_close: Rc<Cell<bool>>,
}

impl TerminalLinkProvider for LinkProvider {
    fn provide_terminal_links(&self, _context: &TerminalLinkContext, token: CancellationToken) -> MaybePromise<Option<Vec<TerminalLink>>> {
        let (promise, resolve) = Promise::new();
        let cancelled = self.provider_token_cancelled.clone();
        self.store.add(token.on_cancellation_requested().listen(move |()| {
            cancelled.set(true);
            resolve.resolve(Some(vec![TerminalLink::new(0.0, 5.0, Some("x")).expect("o link é válido")]));
        }));
        MaybePromise::Promise(promise)
    }

    fn handle_terminal_link(&self, _link: &TerminalLink) {
        self.handled_after_close.set(true);
    }
}

/// O `vscode.Pseudoterminal` do segundo caso.
struct TestPty {
    write_emitter: Rc<Emitter<String>>,
    close_emitter: Rc<Emitter<Option<i32>>>,
    inputs: Rc<RefCell<Vec<String>>>,
    close_on_open: Rc<Cell<bool>>,
}

impl Pseudoterminal for TestPty {
    fn on_did_write(&self) -> Event<String> {
        self.write_emitter.event()
    }

    fn on_did_close(&self) -> Option<Event<Option<i32>>> {
        Some(self.close_emitter.event())
    }

    fn open(&self, _initial_dimensions: Option<TerminalDimensions>) {
        if self.close_on_open.get() {
            self.close_emitter.fire(None);
        }
    }

    fn close(&self) {}

    fn handle_input(&self, data: &str) {
        self.inputs.borrow_mut().push(data.to_owned());
    }
}

/// O começo comum dos dois casos: o protocolo, o mock da thread principal e o serviço.
fn create_service(rpc: &Rc<TestRpcProtocol>, main_thread: &Rc<MockMainThreadTerminalService>) -> Rc<WorkerExtHostTerminalService> {
    rpc.register_proxy(&MainContext::MAIN_THREAD_TERMINAL_SERVICE, Rc::new(MainThreadProxy { rpc: Rc::clone(rpc) }) as Rc<dyn MainThreadTerminalServiceShape>);
    rpc.set(&MainContext::MAIN_THREAD_TERMINAL_SERVICE, main_thread.clone() as Rc<dyn MainThreadTerminalServiceShape>);
    WorkerExtHostTerminalService::new(Rc::new(MockCommands), &**rpc, &MockInitData)
}

#[test]
fn accept_terminal_closed_cancels_in_flight_link_providers_and_clears_the_link_cache() {
    run_with_faked_timers(async move {
        let store = ensure_no_disposables_are_leaked_in_test_suite();
        let rpc = TestRpcProtocol::new();
        let main_thread = Rc::new(MockMainThreadTerminalService::default());
        let service = store.add_rc(create_service(&rpc, &main_thread));

        let terminal_id = 42;
        service.rpc_accept_terminal_opened(terminal_id, None, "test", IShellLaunchConfigDto::default());
        // $acceptTerminalClosed splices the terminal out of `_terminals` but doesn't dispose it,
        // so register it with the test store to keep the leak detector quiet.
        let terminal = service.get_terminal_by_id(terminal_id).expect("o terminal foi aberto");
        store.add_rc(terminal);

        // Link provider that only resolves when the cancellation token fires, so we can observe
        // cancellation as the externally-visible effect of $acceptTerminalClosed.
        let provider_token_cancelled = Rc::new(Cell::new(false));
        let handled_after_close = Rc::new(Cell::new(false));
        let provider = Rc::new(LinkProvider {
            store: store.store(),
            provider_token_cancelled: provider_token_cancelled.clone(),
            handled_after_close: handled_after_close.clone(),
        });
        store.add_rc(service.register_link_provider(provider));

        let in_flight = service.rpc_provide_links(terminal_id, "hello");
        service.rpc_accept_terminal_closed(terminal_id, None, PlatformExitReason::Unknown).await.unwrap();
        let first_links = in_flight.await.unwrap();

        // Any cached links that might have been written before close should have been cleared, so
        // $activateLink for a closed terminal is a no-op.
        service.rpc_activate_link(terminal_id, 0);

        // A subsequent $provideLinks for the same id sees a clean slate (terminal is gone -> []).
        let links_after_close = service.rpc_provide_links(terminal_id, "hello").await.unwrap();

        assert_eq!(
            (provider_token_cancelled.get(), first_links, links_after_close, handled_after_close.get()),
            (true, Vec::new(), Vec::new(), false)
        );
    });
}

#[test]
fn extension_terminal_processes_are_released_when_terminals_close() {
    run_with_faked_timers(async move {
        let store = ensure_no_disposables_are_leaked_in_test_suite();
        let rpc = TestRpcProtocol::new();
        let main_thread = Rc::new(MockMainThreadTerminalService::default());
        let service = store.add_rc(create_service(&rpc, &main_thread));

        let terminal_id = 42;
        service.rpc_accept_terminal_opened(terminal_id, None, "test", IShellLaunchConfigDto::default());
        let terminal = service.get_terminal_by_id(terminal_id).expect("o terminal foi aberto");
        store.add_rc(terminal);

        let write_emitter = store.add_rc(Emitter::<String>::new());
        let close_emitter = store.add_rc(Emitter::<Option<i32>>::new());
        let inputs: Rc<RefCell<Vec<String>>> = Rc::default();
        let close_on_open = Rc::new(Cell::new(false));
        let pty: Rc<dyn Pseudoterminal> = Rc::new(TestPty {
            write_emitter,
            close_emitter: close_emitter.clone(),
            inputs: inputs.clone(),
            close_on_open: close_on_open.clone(),
        });
        service.attach_pty_to_terminal(terminal_id, pty.clone()).unwrap();
        service.rpc_start_extension_terminal(terminal_id, None).await.unwrap();
        rpc.sync().await.unwrap();

        service.rpc_accept_process_input(terminal_id, "before");
        service.rpc_accept_terminal_closed(terminal_id, None, PlatformExitReason::Unknown).await.unwrap();
        service.rpc_accept_process_input(terminal_id, "after");
        close_emitter.fire(None);
        rpc.sync().await.unwrap();

        assert_eq!(*inputs.borrow(), vec!["before".to_owned()]);
        assert_eq!(main_thread.process_exit_calls.get(), 0);

        let synchronous_terminal_id = 43;
        service.rpc_accept_terminal_opened(synchronous_terminal_id, None, "test", IShellLaunchConfigDto::default());
        store.add_rc(service.get_terminal_by_id(synchronous_terminal_id).expect("o terminal foi aberto"));
        close_on_open.set(true);
        service.attach_pty_to_terminal(synchronous_terminal_id, pty).unwrap();
        service.rpc_start_extension_terminal(synchronous_terminal_id, None).await.unwrap();
        rpc.sync().await.unwrap();

        assert!(!close_emitter.has_listeners());
        assert_eq!(main_thread.process_exit_calls.get(), 1);
    });
}
