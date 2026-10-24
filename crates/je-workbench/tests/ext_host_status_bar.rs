//! Porte de `src/vs/workbench/api/test/browser/extHostStatusBar.test.ts` (suíte `ExtHostStatusBar command lifetime`),
//! caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` vira o guarda criado no começo de cada caso, e o `setup` é [`Env::new`].
//!   Os casos com `await timeout(0)` rodam em `run_with_faked_timers`.
//! - `new class extends mock<...Shape>() { override ... }` com `SingleProxyRPCProtocol` são os dublês [`MockMainThreadCommands`],
//!   [`DisplayStatusBar`] e [`MockExtHostStatusBar`] (um método sem `override` falha com `panic!`, como o método ausente do objeto
//!   vazio lançaria `TypeError`) e o [`SingleProxyContext`].
//! - `sinon.spy(commands.converter, 'toInternal')` com `returnValues` é o [`SpyConverter`], que o item usa desde o `setup`: ele
//!   delega ao conversor real e grava o que `toInternal` devolve. Como nenhum caso converte comandos entre o `setup` e o ponto
//!   onde o `spy` seria criado, a lista gravada é a mesma do original.
//! - `TestRPCProtocol` e `rpc.getProxy(ExtHostContext.ExtHostCommands).$executeContributedCommand(...)`: a chamada remota executa
//!   depois de uma macrotarefa (`setTimeout(c, 0)`), e é isso que a deixa "na fila" antes de `hide()` e da troca do comando;
//!   [`queued_execute_contributed_command`] reproduz essa espera. A transferência de fio (JSON) dos argumentos, que só texto
//!   atravessa aqui, é a identidade.
//! - `assert.strictEqual(converter.fromInternal(previous), oldCommand)` compara o objeto por identidade; `fromInternal` devolve
//!   uma cópia aqui, então a conferência é por título, comando e argumentos ([`command_matches`]).
//! - O `MainThreadStatusBar` do terceiro caso usa o serviço [`MockStatusBarItemService`] (`mock<IExtensionStatusBarItemService>`), que
//!   guarda o `command` de `setOrUpdateEntry` (o `args[6]`) e devolve `DidUpdate`.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod support;

use std::any::Any;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use common::{ensure_no_disposables_are_leaked_in_test_suite, run_with_faked_timers, timeout};
use je_base::dynamic::Dynamic;
use je_base::errors::Error;
use je_base::event::Event;
use je_base::lifecycle::{DisposableStore, IDisposable};
use je_base::cancellation::CancellationToken;
use je_base::promise::{MaybePromise, Promise};
use je_base::themables::ThemeColor;
use je_editor::languages::StringOrMarkdown;
use je_extensions::manifest::ExtensionIdentifier;
use je_platform::accessibility::IAccessibilityInformation;
use je_platform::log::NullLogService;
use je_workbench::api::browser::main_thread_status_bar::{
    ExtensionStatusBarItem, ExtensionStatusBarItemChangeEvent, IExtensionStatusBarItemService, MainThreadStatusBar, StatusBarTooltip, StatusBarUpdateKind,
};
use je_workbench::api::common::ext_host_commands::{CommandsConverter, ExtHostCommands};
use je_workbench::api::common::ext_host_protocol::{
    CommandArgs, ExtHostCommandsShape, ExtHostStatusBarShape, ICommandDto, IExtHostTelemetry, IMainContext, MainThreadCommandsShape, MainThreadStatusBarShape,
    MainThreadTelemetryShape, StatusBarColor, StatusBarItemDto,
};
use je_workbench::api::common::ext_host_status_bar::{ExtHostStatusBarEntry, StatusBarCommand};
use je_workbench::api::common::ext_host_type_converters::ICommandsConverter;
use je_workbench::api::common::ext_host_types::Command;
use je_workbench::services::extensions::extensions::null_extension_description;
use support::test_rpc_protocol::single_proxy_rpc_protocol;

// ---- os dublês --------------------------------------------------------------------------------------

/// `new class extends mock<MainThreadCommandsShape>() { $registerCommand, $unregisterCommand e $fireCommandActivationEvent vazios }`.
struct MockMainThreadCommands;

impl IDisposable for MockMainThreadCommands {
    fn dispose(&self) {}
}

impl MainThreadCommandsShape for MockMainThreadCommands {
    fn rpc_register_command(&self, _id: &str) {}

    fn rpc_unregister_command(&self, _id: &str) {}

    fn rpc_fire_command_activation_event(&self, _id: &str) {}

    fn rpc_execute_command(&self, _id: &str, _args: CommandArgs, _retry: bool) -> Promise<Dynamic> {
        panic!("$executeCommand is not a function")
    }

    fn rpc_get_commands(&self) -> Promise<Vec<String>> {
        panic!("$getCommands is not a function")
    }
}

/// O `MainThreadTelemetry` que o construtor de `ExtHostCommands` pede e que nenhum caso usa.
struct MockMainThreadTelemetry;

impl IDisposable for MockMainThreadTelemetry {
    fn dispose(&self) {}
}

impl MainThreadTelemetryShape for MockMainThreadTelemetry {
    fn rpc_public_log(&self, _event_name: &str, _data: Option<Dynamic>) {}

    fn rpc_public_log2(&self, _event_name: &str, _data: Option<Dynamic>) {}
}

/// `new class extends mock<IExtHostTelemetry>() { }`: nenhum caso chega a `onExtensionError`.
struct MockExtHostTelemetry;

impl IExtHostTelemetry for MockExtHostTelemetry {
    fn on_extension_error(&self, _extension: &ExtensionIdentifier, _error: &Error) -> bool {
        panic!("onExtensionError is not a function")
    }
}

/// `SingleProxyRPCProtocol(...)`: o proxy dos comandos, e o da telemetria que o construtor pede.
struct SingleProxyContext {
    commands: Rc<dyn MainThreadCommandsShape>,
    telemetry: Rc<dyn MainThreadTelemetryShape>,
}

impl IMainContext for SingleProxyContext {
    fn get_proxy_erased(&self, sid: &'static str) -> Rc<dyn Any> {
        match sid {
            "MainThreadCommands" => Rc::new(self.commands.clone()),
            "MainThreadTelemetry" => Rc::new(self.telemetry.clone()),
            other => panic!("proxy inesperado: {other}"),
        }
    }

    fn set_erased(&self, _sid: &'static str, _value: Rc<dyn Any>) {}
}

/// `new class extends mock<MainThreadStatusBarShape>() { override $setEntry(...args) { displayedCommand = args[7]; } override $disposeEntry() { } }`.
struct DisplayStatusBar {
    displayed_command: Rc<RefCell<Option<ICommandDto>>>,
}

impl IDisposable for DisplayStatusBar {
    fn dispose(&self) {}
}

impl MainThreadStatusBarShape for DisplayStatusBar {
    fn rpc_set_entry(
        &self,
        _id: &str,
        _status_id: &str,
        _extension_id: Option<&str>,
        _status_name: &str,
        _text: &str,
        _tooltip: Option<StringOrMarkdown>,
        _has_tooltip_provider: bool,
        command: Option<ICommandDto>,
        _color: Option<StatusBarColor>,
        _background_color: Option<ThemeColor>,
        _align_left: bool,
        _priority: Option<f64>,
        _accessibility_information: Option<IAccessibilityInformation>,
    ) {
        *self.displayed_command.borrow_mut() = command;
    }

    fn rpc_dispose_entry(&self, _id: &str) {}
}

/// `new class extends mock<ExtHostStatusBarShape>() { override $acceptStaticEntries(): void { } }`.
struct MockExtHostStatusBar;

impl ExtHostStatusBarShape for MockExtHostStatusBar {
    fn rpc_accept_static_entries(&self, _added: Vec<StatusBarItemDto>) {}

    fn rpc_provide_tooltip(&self, _entry_id: &str, _cancellation: CancellationToken) -> Promise<Option<StringOrMarkdown>> {
        panic!("$provideTooltip is not a function")
    }
}

/// `new class extends mock<IExtensionStatusBarItemService>() { onDidChange = Event.None; getEntries() { return [] } setOrUpdateEntry(...)
/// { rendererCommand = args[6]; return DidUpdate } unsetEntry() { wasUnset = true } }`.
struct MockStatusBarItemService {
    renderer_command: Rc<RefCell<Option<ICommandDto>>>,
    was_unset: Rc<Cell<bool>>,
}

impl IExtensionStatusBarItemService for MockStatusBarItemService {
    fn on_did_change(&self) -> Event<ExtensionStatusBarItemChangeEvent> {
        Event::none()
    }

    fn set_or_update_entry(
        &self,
        _id: &str,
        _status_id: &str,
        _extension_id: Option<&str>,
        _name: &str,
        _text: &str,
        _tooltip: Option<StatusBarTooltip>,
        command: Option<ICommandDto>,
        _color: Option<StatusBarColor>,
        _background_color: Option<ThemeColor>,
        _align_left: bool,
        _priority: Option<f64>,
        _accessibility_information: Option<IAccessibilityInformation>,
    ) -> StatusBarUpdateKind {
        *self.renderer_command.borrow_mut() = command;
        StatusBarUpdateKind::DidUpdate
    }

    fn unset_entry(&self, _id: &str) {
        self.was_unset.set(true);
    }

    fn get_entries(&self) -> Vec<(String, ExtensionStatusBarItem)> {
        Vec::new()
    }
}

/// `sinon.spy(commands.converter, 'toInternal')`: delega ao conversor real e grava os valores devolvidos.
struct SpyConverter {
    inner: Rc<CommandsConverter>,
    return_values: RefCell<Vec<ICommandDto>>,
}

impl ICommandsConverter for SpyConverter {
    fn from_internal(&self, command: &ICommandDto) -> Option<Command> {
        self.inner.from_internal(command)
    }

    fn to_internal(&self, command: Option<&Command>, disposables: &DisposableStore) -> Result<Option<ICommandDto>, Error> {
        let result = self.inner.to_internal(command, disposables)?;
        if let Some(dto) = &result {
            self.return_values.borrow_mut().push(dto.clone());
        }
        Ok(result)
    }
}

// ---- o setup ------------------------------------------------------------------------------------------

struct Env {
    commands: Rc<ExtHostCommands>,
    converter: Rc<SpyConverter>,
    item: Rc<ExtHostStatusBarEntry>,
    displayed_command: Rc<RefCell<Option<ICommandDto>>>,
}

impl Env {
    /// O `setup`.
    fn new(guard: &common::LeakGuard) -> Env {
        let context = SingleProxyContext { commands: Rc::new(MockMainThreadCommands), telemetry: Rc::new(MockMainThreadTelemetry) };
        let commands = ExtHostCommands::new(&context, NullLogService::shared(), Rc::new(MockExtHostTelemetry));
        let displayed_command = Rc::new(RefCell::new(None));
        let converter = Rc::new(SpyConverter { inner: commands.converter(), return_values: RefCell::new(Vec::new()) });
        let item = guard.add_rc(ExtHostStatusBarEntry::new(
            Rc::new(DisplayStatusBar { displayed_command: displayed_command.clone() }),
            converter.clone(),
            &HashMap::new(),
            Some(null_extension_description()),
            Some("test.status"),
            None,
            None,
            None,
        ));
        Env { commands, converter, item, displayed_command }
    }

    fn displayed(&self) -> ICommandDto {
        self.displayed_command.borrow().clone().expect("o item deveria ter mostrado um comando")
    }

    /// `commands.converter.fromInternal(command)?.arguments`.
    fn arguments_of(&self, command: &ICommandDto) -> Option<Vec<Dynamic>> {
        self.converter.from_internal(command).and_then(|command| command.arguments)
    }

    /// `converted.returnValues.map(command => commands.converter.fromInternal(command!)?.arguments)`.
    fn returned_arguments(&self) -> Vec<Option<Vec<Dynamic>>> {
        self.converter.return_values.borrow().iter().map(|command| self.arguments_of(command)).collect()
    }

    /// `commands.registerCommand(false, 'test.action', argument => argument)`, descartado ao fim do teste.
    fn register_echo_command(&self, guard: &common::LeakGuard) {
        let registration = self
            .commands
            .register_command(
                false,
                "test.action",
                Rc::new(|args: &[Dynamic]| MaybePromise::Value(args.first().cloned().unwrap_or(Dynamic::Undefined))),
                None,
                None,
            )
            .expect("registerCommand");
        guard.add_rc(Rc::new(registration));
    }
}

/// `{ title, command: 'test.action', arguments }` com argumentos em texto.
fn action(title: &str, arguments: &[&str]) -> Rc<Command> {
    let mut command = Command::new(title, "test.action");
    command.arguments = Some(arguments.iter().map(|argument| Dynamic::string(argument)).collect());
    Rc::new(command)
}

fn api_command(command: &Rc<Command>) -> Option<StatusBarCommand> {
    Some(StatusBarCommand::Command(command.clone()))
}

/// O `assert.strictEqual(fromInternal(...), command)` do original, por título, comando e argumentos.
fn command_matches(actual: &Option<Command>, expected: &Command) -> bool {
    match actual {
        Some(actual) => actual.title == expected.title && actual.command == expected.command && actual.arguments == expected.arguments,
        None => false,
    }
}

/// `rpc.getProxy(ExtHostContext.ExtHostCommands).$executeContributedCommand(id, ...args)`: a chamada só executa depois de uma
/// macrotarefa, como no `TestRPCProtocol`.
fn queued_execute_contributed_command(commands: &Rc<ExtHostCommands>, command: &ICommandDto) -> Promise<Dynamic> {
    let commands = commands.clone();
    let id = command.id.clone();
    let arguments = command.arguments.clone().unwrap_or_default();
    timeout(0).then_promise(move |_| commands.rpc_execute_contributed_command(&id, arguments))
}

// ---- os casos -----------------------------------------------------------------------------------------

#[test]
fn replacing_commands_that_were_never_published_releases_their_arguments() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let env = Env::new(&guard);

    for i in 0..37 {
        let mut command = Command::new("Hidden", "test.action");
        command.arguments = Some(vec![Dynamic::Number(f64::from(i))]);
        env.item.set_command(api_command(&Rc::new(command)));
    }
    let mut expected: Vec<Option<Vec<Dynamic>>> = vec![None; 36];
    expected.push(Some(vec![Dynamic::Number(36.0)]));
    assert_eq!(env.returned_arguments(), expected);
}

fn queued_clicks_survive(replace_before_hide: bool) {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let env = Rc::new(Env::new(&guard));
    let guard = Rc::new(guard);
    let body_guard = guard.clone();
    run_with_faked_timers(async move {
        env.register_echo_command(&body_guard);
        env.item.set_command(api_command(&action("Old", &["old argument"])));
        env.item.show();
        timeout(0).await.unwrap();
        let previous = env.displayed();
        let pending_click = queued_execute_contributed_command(&env.commands, &previous);

        if !replace_before_hide {
            env.item.hide();
        }
        env.item.set_command(api_command(&action("New", &["new argument"])));
        if replace_before_hide {
            env.item.hide();
        }

        assert_eq!(pending_click.await.unwrap(), Dynamic::string("old argument"));
        env.item.show();
        timeout(0).await.unwrap();
        assert_eq!(
            (env.converter.from_internal(&previous).is_some(), env.arguments_of(&env.displayed())),
            (false, Some(vec![Dynamic::string("new argument")]))
        );
    });
}

#[test]
fn queued_clicks_survive_hide_before_replacement() {
    queued_clicks_survive(false);
}

#[test]
fn queued_clicks_survive_replacement_before_hide() {
    queued_clicks_survive(true);
}

fn contributed_item_keeps_published_command(replace_before_hide: bool) {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let env = Rc::new(Env::new(&guard));
    let guard = Rc::new(guard);
    let body_guard = guard.clone();
    run_with_faked_timers(async move {
        let renderer_command: Rc<RefCell<Option<ICommandDto>>> = Rc::default();
        let was_unset = Rc::new(Cell::new(false));
        let ext_host_context = single_proxy_rpc_protocol(Rc::new(MockExtHostStatusBar) as Rc<dyn ExtHostStatusBarShape>);
        let main_thread = body_guard.add_rc(MainThreadStatusBar::new(
            &*ext_host_context,
            Rc::new(MockStatusBarItemService { renderer_command: renderer_command.clone(), was_unset: was_unset.clone() }),
        ));
        let static_items = HashMap::from([(
            env.item.entry_id().to_owned(),
            StatusBarItemDto {
                entry_id: env.item.entry_id().to_owned(),
                name: "Static".to_owned(),
                text: "Static".to_owned(),
                priority: Some(0.0),
                align_left: true,
                tooltip: None,
                command: None,
                accessibility_information: None,
            },
        )]);
        let main_thread_shape: Rc<dyn MainThreadStatusBarShape> = main_thread;
        let contributed_item = body_guard.add_rc(ExtHostStatusBarEntry::new(
            main_thread_shape,
            env.converter.clone(),
            &static_items,
            Some(null_extension_description()),
            Some("test.status"),
            None,
            None,
            None,
        ));
        env.register_echo_command(&body_guard);
        contributed_item.set_command(api_command(&action("Old", &["old argument"])));
        timeout(0).await.unwrap();
        let published = renderer_command.borrow().clone();
        let published = published.expect("o renderizador deveria ter recebido o comando");

        if !replace_before_hide {
            contributed_item.hide();
        }
        contributed_item.set_command(api_command(&action("New", &["new argument"])));
        if replace_before_hide {
            contributed_item.hide();
        }
        // DidUpdate entries remain in the renderer even after the extension host hides them.
        timeout(0).await.unwrap();
        let result = env.commands.rpc_execute_contributed_command(&published.id, published.arguments.clone().unwrap_or_default()).await.unwrap();
        assert_eq!((was_unset.get(), result), (false, Dynamic::string("old argument")));
    });
}

#[test]
fn a_contributed_item_keeps_its_published_command_after_hide_before_replacement() {
    contributed_item_keeps_published_command(false);
}

#[test]
fn a_contributed_item_keeps_its_published_command_after_replacement_before_hide() {
    contributed_item_keeps_published_command(true);
}

#[test]
fn visible_replacement_keeps_the_previous_command_until_the_ui_update() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let env = Env::new(&guard);
    run_with_faked_timers(async move {
        let old_command = action("Old", &["old argument"]);
        let new_command = action("New", &["new argument"]);
        env.item.set_command(api_command(&old_command));
        env.item.show();
        timeout(0).await.unwrap();
        let previous = env.displayed();
        env.item.set_command(api_command(&new_command));
        assert!(command_matches(&env.converter.from_internal(&previous), &old_command));
        timeout(0).await.unwrap();
        assert!(env.converter.from_internal(&previous).is_none());
        assert!(command_matches(&env.converter.from_internal(&env.displayed()), &new_command));
    });
}

#[test]
fn hidden_replacements_retain_only_the_published_and_current_arguments() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let env = Env::new(&guard);
    run_with_faked_timers(async move {
        env.item.set_command(api_command(&action("Published", &["published"])));
        env.item.show();
        timeout(0).await.unwrap();
        env.item.hide();
        for i in 0..37 {
            let mut command = Command::new("Hidden", "test.action");
            command.arguments = Some(vec![Dynamic::Number(f64::from(i))]);
            env.item.set_command(api_command(&Rc::new(command)));
        }
        let mut expected: Vec<Option<Vec<Dynamic>>> = vec![Some(vec![Dynamic::string("published")])];
        expected.extend(vec![None; 36]);
        expected.push(Some(vec![Dynamic::Number(36.0)]));
        assert_eq!(env.returned_arguments(), expected);
        env.item.dispose();
        assert!(env.converter.return_values.borrow().iter().all(|command| env.converter.from_internal(command).is_none()));
    });
}

#[test]
fn showing_after_hidden_replacements_preserves_the_latest_arguments() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let env = Env::new(&guard);
    run_with_faked_timers(async move {
        env.item.set_command(api_command(&action("Old", &["old argument"])));
        let latest = action("Latest", &["latest argument"]);
        env.item.set_command(api_command(&latest));
        env.item.show();
        env.item.hide();
        env.item.show();
        timeout(0).await.unwrap();
        assert!(command_matches(&env.converter.from_internal(&env.displayed()), &latest));
    });
}

#[test]
fn disposing_a_hidden_item_releases_its_current_arguments() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let env = Env::new(&guard);
    run_with_faked_timers(async move {
        env.item.set_command(api_command(&action("Current", &["current argument"])));
        env.item.show();
        timeout(0).await.unwrap();
        let current = env.displayed();
        env.item.hide();
        env.item.dispose();
        assert!(env.converter.from_internal(&current).is_none());
    });
}
