//! Porte de `src/vs/platform/commands/test/common/commands.test.ts` (suíte `Command Tests`), caso a caso e na mesma ordem.
//!
//! Divergências: o `accessor` que o original passa como `undefined` é um `ServicesAccessor` de verdade, de um
//! `InstantiationService` vazio (o handler recebe `&ServicesAccessor`). `assert.throws` é `errors::catch`.
//! A identidade `handler === command` é a do `Rc`. O registro de comandos é um singleton por thread, e cada
//! teste roda na sua thread, então o registro começa limpo (só com o `noop`).

mod common;

use std::rc::Rc;

use common::*;
use je_base::dynamic::{Builtin, Dynamic, FunctionRef};
use je_base::errors;
use je_base::lifecycle::{IDisposable, combined_disposable};
use je_instantiation::{InstantiationService, ServiceCollection};
use je_platform::commands::{Command, CommandArgument, CommandHandler, CommandMetadata, commands_registry};

fn handler() -> CommandHandler {
    Rc::new(|_, _| Dynamic::Undefined)
}

fn same(a: &CommandHandler, b: &CommandHandler) -> bool {
    Rc::ptr_eq(a, b)
}

fn invoke(handler: &CommandHandler, args: Vec<Dynamic>) -> Dynamic {
    InstantiationService::new(ServiceCollection::new()).invoke_function(|accessor| handler(accessor, &args))
}

#[test]
fn register_command_no_handler() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    assert!(errors::catch(|| commands_registry().register_command_handler("foo", None)).is_err());
}

#[test]
fn register_dispose() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let command = handler();
    let reg = commands_registry().register_command_handler("foo", Some(command.clone()));
    assert!(same(&commands_registry().get_command("foo").unwrap().handler, &command));
    reg.dispose();
    assert!(commands_registry().get_command("foo").is_none());
}

#[test]
fn register_register_dispose() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let command1 = handler();
    let command2 = handler();
    let registry = commands_registry();

    // dispose overriding command
    let reg1 = registry.register_command_handler("foo", Some(command1.clone()));
    assert!(same(&registry.get_command("foo").unwrap().handler, &command1));

    let reg2 = registry.register_command_handler("foo", Some(command2.clone()));
    assert!(same(&registry.get_command("foo").unwrap().handler, &command2));
    reg2.dispose();

    assert!(same(&registry.get_command("foo").unwrap().handler, &command1));
    reg1.dispose();
    assert!(registry.get_command("foo").is_none());

    // dispose override command first
    let reg1 = registry.register_command_handler("foo", Some(command1.clone()));
    let reg2 = registry.register_command_handler("foo", Some(command2.clone()));
    assert!(same(&registry.get_command("foo").unwrap().handler, &command2));

    reg1.dispose();
    assert!(same(&registry.get_command("foo").unwrap().handler, &command2));

    reg2.dispose();
    assert!(registry.get_command("foo").is_none());
}

#[test]
fn command_with_description() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let registry = commands_registry();

    let expects_string: CommandHandler = Rc::new(|_, args| {
        assert!(matches!(args.first(), Some(Dynamic::String(_))));
        Dynamic::Undefined
    });
    let r1 = registry.register_command_handler("test", Some(expects_string.clone()));
    let r2 = registry.register_command_handler("test2", Some(expects_string));

    let mut argument = CommandArgument::new("value");
    argument.constraint = Some(FunctionRef::builtin(Builtin::Number).into());
    let mut metadata = CommandMetadata::new("a command");
    metadata.args = Some(vec![argument]);
    let returns_true: CommandHandler = Rc::new(|_, _| Dynamic::Bool(true));
    let r3 = registry.register_command(Command::new("test3", returns_true).with_metadata(metadata));

    let commands = registry.get_commands();
    invoke(&commands["test"].handler, vec![s("string")]);
    invoke(&commands["test2"].handler, vec![s("string")]);
    assert!(errors::catch(|| invoke(&commands["test3"].handler, vec![s("string")])).is_err());
    assert_eq!(invoke(&commands["test3"].handler, vec![n(1.0)]), Dynamic::Bool(true));

    combined_disposable(vec![r1, r2, r3]).dispose();
}
