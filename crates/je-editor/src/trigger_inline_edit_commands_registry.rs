//! Porte de `src/vs/editor/browser/triggerInlineEditCommandsRegistry.ts`: o registro dos comandos que podem
//! disparar edições inline (NES) quando invocados. `Command.register()` (`editor_extensions`) acrescenta o id de
//! todo comando com `canTriggerInlineEdits`.
//!
//! Divergências:
//! - A classe abstrata com membros estáticos é o tipo [`TriggerInlineEditCommandsRegistry`] com funções
//!   associadas, e o `Set<string>` estático é um estado por thread (como os demais singletons do porte), com a
//!   ordem de inserção preservada e sem repetição.

use std::cell::RefCell;

thread_local! {
    /// `REGISTERED_COMMANDS`.
    static REGISTERED_COMMANDS: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
}

/// `TriggerInlineEditCommandsRegistry`.
pub struct TriggerInlineEditCommandsRegistry;

impl TriggerInlineEditCommandsRegistry {
    /// `getRegisteredCommands()`.
    pub fn get_registered_commands() -> Vec<String> {
        REGISTERED_COMMANDS.with(|commands| commands.borrow().clone())
    }

    /// `registerCommand(commandId)`.
    pub fn register_command(command_id: &str) {
        REGISTERED_COMMANDS.with(|commands| {
            let mut commands = commands.borrow_mut();
            if !commands.iter().any(|id| id == command_id) {
                commands.push(command_id.to_string());
            }
        });
    }
}
