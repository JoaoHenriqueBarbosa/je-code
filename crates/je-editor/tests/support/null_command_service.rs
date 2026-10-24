//! `NullCommandService` de `src/vs/platform/commands/test/common/nullCommandService.ts`: o serviço de comandos
//! que não executa nada. Os eventos nunca disparam (`() => Disposable.None`) e `executeCommand` resolve com
//! `undefined`.

use je_base::dynamic::Dynamic;
use je_base::event::Event;
use je_base::promise::Promise;
use je_platform::commands::{CommandEvent, ICommandService};

pub struct NullCommandService;

impl ICommandService for NullCommandService {
    fn on_will_execute_command(&self) -> Event<CommandEvent> {
        Event::none()
    }

    fn on_did_execute_command(&self) -> Event<CommandEvent> {
        Event::none()
    }

    fn execute_command(&self, _command_id: &str, _args: Vec<Dynamic>) -> Promise<Dynamic> {
        Promise::resolved(Dynamic::Undefined)
    }
}
