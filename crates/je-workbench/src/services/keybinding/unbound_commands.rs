//! Porte de `src/vs/workbench/services/keybinding/browser/unboundCommands.ts`: os comandos que ainda não têm atalho, para o
//! editor de atalhos listá-los junto dos que têm.
//!
//! Divergências:
//! - `CommandsRegistry.getCommands().keys()` itera o `Map` na ordem de registro; o [`CommandsMap`](je_platform::commands::CommandsMap)
//!   de Rust é um `HashMap` sem ordem, então os ids saem em ordem alfabética (ordem estável entre execuções). O editor de
//!   atalhos reordena a lista depois, e só a ordem "por precedência" enxerga a posição dos comandos sem atalho.
//! - `EditorExtensionsRegistry.getEditorActions()` é o `get_editor_actions` do [`EditorExtensionsRegistry`] por thread.

use std::collections::{HashMap, HashSet};

use je_editor::code_editor_widget::IEditorExtensionsRegistry;
use je_editor::editor_extensions::EditorExtensionsRegistry;
use je_platform::actions::{AnyMenuItem, menu_ids, menu_registry};
use je_platform::commands::commands_registry;

/// `getAllUnboundCommands`: os ids da paleta de comandos, das ações do editor e do registro de comandos que não estão em
/// `bound_commands`, sem os comandos privados (`_...` e `vscode.`) e, no registro, sem os que recebem argumentos.
pub fn get_all_unbound_commands(bound_commands: &HashMap<String, bool>) -> Vec<String> {
    let mut unbound_commands: Vec<String> = Vec::new();
    let mut seen_map: HashSet<String> = HashSet::new();
    let mut add_command = |id: &str, include_command_with_args: bool| {
        if !seen_map.insert(id.to_string()) {
            return;
        }
        if id.starts_with('_') || id.starts_with("vscode.") {
            // private command
            return;
        }
        if bound_commands.get(id) == Some(&true) {
            return;
        }
        if !include_command_with_args
            && let Some(command) = commands_registry().get_command(id)
            && command.metadata.as_ref().is_some_and(|metadata| metadata.args.as_ref().is_some_and(|args| !args.is_empty()))
        {
            // command with args
            return;
        }
        unbound_commands.push(id.to_string());
    };

    // Add all commands from Command Palette
    for menu_item in menu_registry().get_menu_items(&menu_ids::command_palette()) {
        if let AnyMenuItem::Menu(menu_item) = menu_item {
            add_command(&menu_item.command.id, true);
        }
    }

    // Add all editor actions
    for editor_action in EditorExtensionsRegistry::instance().get_editor_actions() {
        add_command(&editor_action.id, true);
    }

    let mut command_ids: Vec<String> = commands_registry().get_commands().into_keys().collect();
    command_ids.sort();
    for id in &command_ids {
        add_command(id, false);
    }

    unbound_commands
}
