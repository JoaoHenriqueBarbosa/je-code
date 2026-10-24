//! O `new class extends mock<ICommandService>() { override executeCommand(...) }` dos testes da camada `MainThread`:
//! só o que o caso sobrescreve responde, e todo outro membro lança `X is not a function`, como chamar um membro
//! ausente do objeto vazio do `mock()` lançaria `TypeError`. `undefined!` no lugar do serviço é o serviço sem
//! `executeCommand`.

use je_base::dynamic::Dynamic;
use je_base::event::Event;
use je_base::promise::Promise;
use je_platform::commands::commands::{CommandEvent, ICommandService};

type ExecuteCommandFn = Box<dyn Fn(&str) -> Promise<Dynamic>>;

/// `mock<ICommandService>()` com `executeCommand` opcional.
#[derive(Default)]
pub struct MockCommandService {
    pub execute_command: Option<ExecuteCommandFn>,
}

impl ICommandService for MockCommandService {
    fn on_will_execute_command(&self) -> Event<CommandEvent> {
        panic!("onWillExecuteCommand is not defined")
    }

    fn on_did_execute_command(&self) -> Event<CommandEvent> {
        panic!("onDidExecuteCommand is not defined")
    }

    fn execute_command(&self, command_id: &str, _args: Vec<Dynamic>) -> Promise<Dynamic> {
        match &self.execute_command {
            Some(execute_command) => execute_command(command_id),
            None => panic!("executeCommand is not a function"),
        }
    }
}
