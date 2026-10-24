//! Porte de `src/vs/workbench/api/test/browser/extHostCommands.test.ts` (suíte `ExtHostCommands`), caso a caso e
//! na mesma ordem, seguido dos "Casos próprios" (a partir de `// ---- Casos próprios`), que cobrem o que o
//! original não cobre: validação do registro, conversão dos argumentos, o conversor de comandos e o erro anotado.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` vira o guarda criado no começo de cada caso; os casos `async`
//!   rodam em `run_with_faked_timers`.
//! - `mock<MainThreadCommandsShape>()` com `override` vira [`MockMainThreadCommands`] com fechos opcionais:
//!   um método sem `override` falha (`panic!`), como o método ausente do objeto vazio do original lançaria
//!   `TypeError`; `$registerCommand` (que o construtor chama para o comando delegante) responde em todos os casos.
//! - `SingleProxyRPCProtocol(shape)` devolve a mesma coisa para qualquer identificador. Os proxies de Rust são
//!   tipados por forma (`dyn MainThreadCommandsShape`, `dyn MainThreadTelemetryShape`), então o contexto daqui
//!   ([`CommandsContext`]) escolhe pelo `sid`: o dublê de comandos para `MainThreadCommands` e um dublê de
//!   telemetria vazio para `MainThreadTelemetry` (que o original recebe como o mesmo objeto vazio).
//! - `new class extends mock<IExtHostTelemetry>() { onExtensionError() { return true } }` é [`MockExtHostTelemetry`].
//! - O `this` do mocha que os casos passam como argumento é um objeto qualquer ([`context_object`]); o
//!   `assert.strictEqual(result, this)` é a identidade do objeto ([`Dynamic::strict_equals`]).
//! - `commands.executeCommand('fooo', [this, true])` passa um argumento só, o array.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use std::any::Any;
use std::cell::{Cell, RefCell};
use std::rc::Rc;

use common::{ensure_no_disposables_are_leaked_in_test_suite, run_with_faked_timers};
use je_base::dynamic::Dynamic;
use je_base::errors::Error;
use je_base::lifecycle::{DisposableStore, IDisposable};
use je_base::promise::{MaybePromise, Promise};
use je_base::types::TypeConstraint;
use je_extensions::manifest::ExtensionIdentifier;
use je_platform::commands::commands::{commands_registry, CommandArgument, CommandMetadata};
use je_platform::log::NullLogService;
use je_workbench::api::common::ext_host_commands::{CommandCallback, ExtHostCommands};
use je_workbench::api::common::ext_host_protocol::{
    CommandArgs, ExtHostCommandsShape, IExtHostTelemetry, IMainContext, MainThreadCommandsShape, MainThreadTelemetryShape,
};
use je_workbench::api::common::ext_host_type_converters::ICommandsConverter;
use je_workbench::api::common::ext_host_types::{self as types, Command};
use je_workbench::api::common::ext_host_values::{bytes_of, class_of, ApiValue};

type UnregisterFn = Box<dyn Fn(&str)>;
type ExecuteFn = Box<dyn Fn(&str, CommandArgs, bool) -> Promise<Dynamic>>;
type ActivationFn = Box<dyn Fn(&str)>;

/// O `new class extends mock<MainThreadCommandsShape>()` do original: só o que cada caso sobrescreve responde.
#[derive(Default)]
struct MockMainThreadCommands {
    unregister_command: Option<UnregisterFn>,
    execute_command: Option<ExecuteFn>,
    fire_command_activation_event: Option<ActivationFn>,
}

impl IDisposable for MockMainThreadCommands {
    fn dispose(&self) {}
}

impl MainThreadCommandsShape for MockMainThreadCommands {
    fn rpc_register_command(&self, _id: &str) {
        //
    }

    fn rpc_unregister_command(&self, id: &str) {
        match &self.unregister_command {
            Some(unregister_command) => unregister_command(id),
            None => panic!("$unregisterCommand is not a function"),
        }
    }

    fn rpc_fire_command_activation_event(&self, id: &str) {
        match &self.fire_command_activation_event {
            Some(fire_command_activation_event) => fire_command_activation_event(id),
            None => panic!("$fireCommandActivationEvent is not a function"),
        }
    }

    fn rpc_execute_command(&self, id: &str, args: CommandArgs, retry: bool) -> Promise<Dynamic> {
        match &self.execute_command {
            Some(execute_command) => execute_command(id, args, retry),
            None => panic!("$executeCommand is not a function"),
        }
    }

    fn rpc_get_commands(&self) -> Promise<Vec<String>> {
        panic!("$getCommands is not a function")
    }
}

/// O `MainThreadTelemetry` que o construtor pede e que nenhum caso usa (o comando dos casos não tem extensão).
struct MockMainThreadTelemetry;

impl IDisposable for MockMainThreadTelemetry {
    fn dispose(&self) {}
}

impl MainThreadTelemetryShape for MockMainThreadTelemetry {
    fn rpc_public_log(&self, _event_name: &str, _data: Option<Dynamic>) {}

    fn rpc_public_log2(&self, _event_name: &str, _data: Option<Dynamic>) {}
}

/// `new class extends mock<IExtHostTelemetry>() { override onExtensionError(): boolean { return true; } }`.
struct MockExtHostTelemetry;

impl IExtHostTelemetry for MockExtHostTelemetry {
    fn on_extension_error(&self, _extension: &ExtensionIdentifier, _error: &Error) -> bool {
        true
    }
}

/// `SingleProxyRPCProtocol(shape)`, escolhendo o proxy pelo `sid` (ver as notas do módulo).
struct CommandsContext {
    commands: Rc<dyn MainThreadCommandsShape>,
    telemetry: Rc<dyn MainThreadTelemetryShape>,
}

impl IMainContext for CommandsContext {
    fn get_proxy_erased(&self, sid: &'static str) -> Rc<dyn Any> {
        match sid {
            "MainThreadCommands" => Rc::new(self.commands.clone()),
            "MainThreadTelemetry" => Rc::new(self.telemetry.clone()),
            other => panic!("proxy inesperado: {other}"),
        }
    }

    fn set_erased(&self, _sid: &'static str, _value: Rc<dyn Any>) {}
}

fn create_ext_host_commands(shape: MockMainThreadCommands) -> Rc<ExtHostCommands> {
    let context = CommandsContext { commands: Rc::new(shape), telemetry: Rc::new(MockMainThreadTelemetry) };
    ExtHostCommands::new(&context, NullLogService::shared(), Rc::new(MockExtHostTelemetry))
}

/// O `this` do mocha: um objeto qualquer, com identidade.
fn context_object() -> Dynamic {
    Dynamic::object([("title", Dynamic::string("ExtHostCommands"))])
}

/// `(): any => { }`.
fn noop_callback() -> CommandCallback {
    Rc::new(|_: &[Dynamic]| MaybePromise::Value(Dynamic::Undefined))
}

#[test]
fn dispose_calls_unregister() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    let last_unregister: Rc<RefCell<Option<String>>> = Rc::default();
    let sink = last_unregister.clone();
    let shape = MockMainThreadCommands { unregister_command: Some(Box::new(move |id| *sink.borrow_mut() = Some(id.to_owned()))), ..MockMainThreadCommands::default() };

    let commands = create_ext_host_commands(shape);
    commands.register_command(true, "foo", noop_callback(), None, None).expect("registro").dispose();
    assert_eq!(last_unregister.borrow().as_deref(), Some("foo"));
    assert!(commands_registry().get_command("foo").is_none());
}

#[test]
fn dispose_bubbles_only_once() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    let unregister_counter = Rc::new(Cell::new(0));
    let counter = unregister_counter.clone();
    let shape = MockMainThreadCommands { unregister_command: Some(Box::new(move |_| counter.set(counter.get() + 1))), ..MockMainThreadCommands::default() };

    let commands = create_ext_host_commands(shape);
    let reg = commands.register_command(true, "foo", noop_callback(), None, None).expect("registro");
    reg.dispose();
    reg.dispose();
    reg.dispose();
    assert_eq!(unregister_counter.get(), 1);
}

#[test]
fn execute_with_retry() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    run_with_faked_timers(async move {
        let count = Rc::new(Cell::new(0));
        let counter = count.clone();
        let shape = MockMainThreadCommands {
            execute_command: Some(Box::new(move |_id, _args, retry| {
                counter.set(counter.get() + 1);
                assert_eq!(retry, counter.get() == 1);
                if counter.get() == 1 {
                    assert!(retry);
                    Promise::rejected(Error::new("$executeCommand:retry"))
                } else {
                    assert!(!retry);
                    Promise::resolved(Dynamic::Number(17.0))
                }
            })),
            ..MockMainThreadCommands::default()
        };

        let commands = create_ext_host_commands(shape);

        let result = commands.execute_command("fooo", vec![Dynamic::array([context_object(), Dynamic::Bool(true)])]).await.expect("resultado");
        assert_eq!(result, Dynamic::Number(17.0));
        assert_eq!(count.get(), 2);
    });
}

#[test]
fn on_command_abc_activates_extensions_when_executed_from_command_palette_but_not_when_executed_programmatically_with_vscode_commands_execute_command_150293() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    run_with_faked_timers(async move {
        let activation_events: Rc<RefCell<Vec<String>>> = Rc::default();
        let sink = activation_events.clone();
        let shape = MockMainThreadCommands {
            fire_command_activation_event: Some(Box::new(move |id| sink.borrow_mut().push(id.to_owned()))),
            ..MockMainThreadCommands::default()
        };
        let commands = create_ext_host_commands(shape);

        let echo: CommandCallback = Rc::new(|args: &[Dynamic]| MaybePromise::Value(args[0].clone()));
        commands.register_command(true, "extCmd", echo, None, None).expect("registro");

        let this = context_object();
        let result = commands.execute_command("extCmd", vec![this.clone()]).await.expect("resultado");
        assert!(result.strict_equals(&this));
        assert_eq!(*activation_events.borrow(), vec!["extCmd".to_owned()]);
    });
}

// ---- Casos próprios --------------------------------------------------------------------------------------

type SentCommands = Rc<RefCell<Vec<(String, CommandArgs)>>>;

/// O dublê que registra o que `$executeCommand` recebeu e responde `undefined`.
fn recording_shape(sent: &SentCommands) -> MockMainThreadCommands {
    let sink = sent.clone();
    MockMainThreadCommands {
        execute_command: Some(Box::new(move |id, args, _| {
            sink.borrow_mut().push((id.to_owned(), args));
            Promise::resolved(Dynamic::Undefined)
        })),
        fire_command_activation_event: Some(Box::new(|_| {})),
        ..MockMainThreadCommands::default()
    }
}

fn plain_args(args: &CommandArgs) -> &[Dynamic] {
    match args {
        CommandArgs::Plain(args) => args,
        CommandArgs::WithBuffers(buffers) => &buffers.value,
    }
}

#[test]
fn register_command_rejeita_id_vazio_e_repetido() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let commands = create_ext_host_commands(MockMainThreadCommands::default());

    let blank = commands.register_command(false, "   ", noop_callback(), None, None).err().expect("id vazio");
    assert_eq!(blank.message, "invalid id");

    commands.register_command(false, "dup", noop_callback(), None, None).expect("primeiro registro");
    let duplicate = commands.register_command(false, "dup", noop_callback(), None, None).err().expect("id repetido");
    assert_eq!(duplicate.message, "command 'dup' already exists");
}

#[test]
fn comando_nao_global_nao_avisa_a_thread_principal_ao_descartar() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    // o dublê falha se `$unregisterCommand` for chamado
    let commands = create_ext_host_commands(MockMainThreadCommands::default());
    let registration = commands.register_command(false, "local", noop_callback(), None, None).expect("registro");
    registration.dispose();
    // depois de descartado o id volta a ser livre
    commands.register_command(false, "local", noop_callback(), None, None).expect("novo registro");
}

#[test]
fn execute_command_converte_as_classes_da_api_nos_dtos_do_protocolo() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    run_with_faked_timers(async move {
        let sent: SentCommands = Rc::default();
        let commands = create_ext_host_commands(recording_shape(&sent));

        let position = types::Position::new(1.0, 2.0).expect("posição").to_dynamic();
        let range = types::Range::from_coordinates(0.0, 1.0, 2.0, 3.0).expect("intervalo").to_dynamic();
        commands.execute_command("remote.cmd", vec![position, range, Dynamic::Number(5.0)]).await.expect("resultado");

        let sent = sent.borrow();
        assert_eq!(sent[0].0, "remote.cmd");
        let args = plain_args(&sent[0].1);
        assert_eq!((args[0].get_str("lineNumber"), args[0].get_str("column")), (Dynamic::Number(2.0), Dynamic::Number(3.0)));
        assert_eq!(args[1].get_str("startLineNumber"), Dynamic::Number(1.0));
        assert_eq!(args[1].get_str("startColumn"), Dynamic::Number(2.0));
        assert_eq!(args[1].get_str("endLineNumber"), Dynamic::Number(3.0));
        assert_eq!(args[1].get_str("endColumn"), Dynamic::Number(4.0));
        assert_eq!(args[2], Dynamic::Number(5.0));
    });
}

#[test]
fn execute_command_manda_os_bytes_como_vs_buffer_com_buffers() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    run_with_faked_timers(async move {
        let sent: SentCommands = Rc::default();
        let commands = create_ext_host_commands(recording_shape(&sent));

        commands.execute_command("remote.cmd", vec![Dynamic::typed_array(vec![1, 2, 3]), Dynamic::string("texto")]).await.expect("resultado");

        let sent = sent.borrow();
        assert!(matches!(sent[0].1, CommandArgs::WithBuffers(_)), "um buffer pede o envelope com buffers");
        let args = plain_args(&sent[0].1);
        assert_eq!(class_of(&args[0]), Some("VSBuffer"));
        assert_eq!(bytes_of(&args[0]), Some(vec![1, 2, 3]));
        assert_eq!(args[1], Dynamic::string("texto"));
    });
}

#[test]
fn execute_command_sem_buffers_manda_os_argumentos_simples() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    run_with_faked_timers(async move {
        let sent: SentCommands = Rc::default();
        let commands = create_ext_host_commands(recording_shape(&sent));
        commands.execute_command("remote.cmd", vec![Dynamic::Bool(true)]).await.expect("resultado");
        assert!(matches!(sent.borrow()[0].1, CommandArgs::Plain(_)));
    });
}

#[test]
fn comando_contribuido_recebe_os_argumentos_como_classes_da_api() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    run_with_faked_timers(async move {
        let commands = create_ext_host_commands(MockMainThreadCommands::default());
        let received: Rc<RefCell<Vec<Dynamic>>> = Rc::default();
        let sink = received.clone();
        let callback: CommandCallback = Rc::new(move |args: &[Dynamic]| {
            sink.borrow_mut().extend(args.iter().cloned());
            MaybePromise::Value(Dynamic::Undefined)
        });
        commands.register_command(false, "contributed", callback, None, None).expect("registro");

        let range_dto = Dynamic::object([
            ("startLineNumber", Dynamic::Number(1.0)),
            ("startColumn", Dynamic::Number(2.0)),
            ("endLineNumber", Dynamic::Number(3.0)),
            ("endColumn", Dynamic::Number(4.0)),
        ]);
        let uri_dto = Dynamic::object([("$mid", Dynamic::Number(1.0)), ("scheme", Dynamic::string("file")), ("path", Dynamic::string("/a"))]);
        let position_dto = Dynamic::object([("lineNumber", Dynamic::Number(5.0)), ("column", Dynamic::Number(6.0))]);
        commands
            .rpc_execute_contributed_command("contributed", vec![range_dto, uri_dto, Dynamic::array([position_dto]), Dynamic::Number(9.0)])
            .await
            .expect("resultado");

        let received = received.borrow();
        assert_eq!(class_of(&received[0]), Some("Range"));
        let range = types::Range::from_dynamic(&received[0]).expect("intervalo");
        assert_eq!((range.start().line(), range.start().character(), range.end().line(), range.end().character()), (0.0, 1.0, 2.0, 3.0));
        assert_eq!(class_of(&received[1]), Some("Uri"));
        assert_eq!(class_of(&received[2].get_str("0")), Some("Position"));
        assert_eq!(received[3], Dynamic::Number(9.0));
    });
}

#[test]
fn comando_contribuido_inexistente_rejeita() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    run_with_faked_timers(async move {
        let commands = create_ext_host_commands(MockMainThreadCommands::default());
        let error = commands.rpc_execute_contributed_command("nada", vec![]).await.expect_err("deveria rejeitar");
        assert_eq!(error.message, "Contributed command 'nada' does not exist.");
    });
}

#[test]
fn comando_contribuido_valida_a_restricao_dos_argumentos() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    run_with_faked_timers(async move {
        let commands = create_ext_host_commands(MockMainThreadCommands::default());
        let mut argument = CommandArgument::new("name");
        argument.description = Some("um nome".to_owned());
        argument.constraint = Some(TypeConstraint::from("string"));
        let mut metadata = CommandMetadata::new("typed");
        metadata.args = Some(vec![argument]);
        commands.register_command(false, "typed", noop_callback(), Some(metadata), None).expect("registro");

        let error = commands.rpc_execute_contributed_command("typed", vec![Dynamic::Number(1.0)]).await.expect_err("restrição violada");
        assert_eq!(error.message, "Running the contributed command: 'typed' failed. Illegal argument 'name' - um nome");
        commands.rpc_execute_contributed_command("typed", vec![Dynamic::string("ok")]).await.expect("argumento válido");
    });
}

#[test]
fn erro_do_callback_vem_anotado_so_pelo_caminho_do_rpc() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    run_with_faked_timers(async move {
        let shape = MockMainThreadCommands { fire_command_activation_event: Some(Box::new(|_| {})), ..MockMainThreadCommands::default() };
        let commands = create_ext_host_commands(shape);
        let failing: CommandCallback = Rc::new(|_: &[Dynamic]| MaybePromise::Promise(Promise::rejected(Error::new("boom"))));
        commands.register_command(false, "failing", failing, None, None).expect("registro");

        let annotated = commands.rpc_execute_contributed_command("failing", vec![]).await.expect_err("deveria rejeitar");
        assert_eq!(annotated.message, "boom");
        assert_eq!(annotated.prop("id"), Some("failing"));

        let raw = commands.execute_command("failing", vec![]).await.expect_err("deveria rejeitar");
        assert_eq!(raw.message, "boom");
        assert_eq!(raw.prop("id"), None);
    });
}

#[test]
fn execute_command_devolve_o_resultado_do_comando_contribuido() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    run_with_faked_timers(async move {
        let shape = MockMainThreadCommands { fire_command_activation_event: Some(Box::new(|_| {})), ..MockMainThreadCommands::default() };
        let commands = create_ext_host_commands(shape);
        let promised: CommandCallback = Rc::new(|_: &[Dynamic]| MaybePromise::Promise(Promise::resolved(Dynamic::string("depois"))));
        commands.register_command(false, "promised", promised, None, None).expect("registro");
        assert_eq!(commands.execute_command("promised", vec![]).await.expect("resultado"), Dynamic::string("depois"));
    });
}

#[test]
fn conversor_guarda_o_comando_com_argumentos_ate_descartar() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let commands = create_ext_host_commands(MockMainThreadCommands::default());
    let converter = commands.converter();

    let command = Command { title: "T".to_owned(), command: "ext.run".to_owned(), tooltip: None, arguments: Some(vec![Dynamic::Number(1.0)]) };
    let store = DisposableStore::new();
    let dto = converter.to_internal(Some(&command), &store).expect("conversão").expect("comando");
    assert_eq!(dto.id, converter.delegating_command_id);
    assert_eq!(dto.ident.as_deref(), Some("ext.run /1"));
    assert_eq!(dto.arguments.as_deref(), Some(&[Dynamic::string("ext.run /1")][..]));
    assert_eq!(converter.from_internal(&dto).map(|restored| restored.command), Some("ext.run".to_owned()));

    store.dispose();
    assert!(converter.from_internal(&dto).is_none(), "descartar remove o comando guardado");
}

#[test]
fn conversor_nao_guarda_comando_sem_argumentos() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let commands = create_ext_host_commands(MockMainThreadCommands::default());
    let converter = commands.converter();
    let store = DisposableStore::new();

    let dto = converter.to_internal(Some(&Command::new("T", "ext.plain")), &store).expect("conversão").expect("comando");
    assert_eq!((dto.id.as_str(), dto.ident.as_deref(), dto.arguments.is_none()), ("ext.plain", None, true));
    let restored = converter.from_internal(&dto).expect("comando");
    assert_eq!((restored.command.as_str(), restored.title.as_str()), ("ext.plain", "T"));

    assert!(converter.to_internal(None, &store).expect("conversão").is_none());
    let empty_id = converter.to_internal(Some(&Command::new("T", "")), &store).expect("conversão").expect("comando");
    assert_eq!(empty_id.id, "");
    store.dispose();
}
