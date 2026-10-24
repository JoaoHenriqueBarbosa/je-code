//! Porte de `src/vs/editor/contrib/wordPartOperations/browser/wordPartOperations.ts`: os comandos de mover o cursor e
//! de apagar por parte de palavra (`DeleteWordPartLeft`, `DeleteWordPartRight`, `CursorWordPartLeft`...), mais as
//! chamadas de registro do fim do arquivo.
//!
//! As operações em si são as de [`crate::cursor_word_operations`] (`WordPartOperations`); os comandos reaproveitam o
//! [`MoveWordCommand`] e o [`DeleteWordCommand`] de [`crate::word_operations`], com o campo `part` ligado (o
//! `_move`/`_delete` que o original sobrescreve nas subclasses `WordPartLeftCommand`, `WordPartRightCommand`,
//! `DeleteWordPartLeft` e `DeleteWordPartRight` é a escolha por `part` dentro deles).
//!
//! Representação: como em `word_operations`, as classes concretas só diferem em dados e viram funções que montam o
//! comando. O `kbOpts` dos seis comandos tem `primary: 0` (sem atalho fora do mac) e, no mac, `WinCtrl | Alt` (mais
//! `Shift` nos de seleção) com a tecla.
//!
//! Divergências:
//! - O `_delete` de `DeleteWordPartRight` tem uma saída de reserva (o fim do documento) para um `deleteWordPartRight`
//!   que devolvesse `undefined`; o `WordPartOperations::delete_word_part_right` do porte devolve sempre um intervalo
//!   (o tipo do original também não admite `undefined` ali), então a reserva não existe.

use std::cell::Cell;
use std::rc::Rc;

use je_base::key_codes::{KeyCode, KeyMod};
use je_platform::commands::commands_registry;
use je_platform::keybinding::keybindings_registry::{KeybindingWeight, Keybindings};

use crate::core_commands::{key, platform_keybindings, text_input_focus};
use crate::cursor_word_operations::WordNavigationType;
use crate::editor_extensions::{CommandKeybindingsOptions, register_editor_command};
use crate::word_operations::{DeleteWordCommand, DeleteWordOptions, MoveWordCommand, MoveWordOptions, WordDirection};

/// O `kbOpts` dos comandos de parte de palavra: sem atalho fora do mac e, no mac, `WinCtrl | Alt` mais `modifiers` e a
/// tecla, com o texto em foco.
fn word_part_keybindings(key_code: KeyCode, modifiers: u32) -> Vec<CommandKeybindingsOptions> {
    vec![CommandKeybindingsOptions::new(
        KeybindingWeight::EDITOR_CONTRIB,
        Some(text_input_focus()),
        Keybindings {
            primary: Some(0),
            mac: platform_keybindings(key(KeyMod::WIN_CTRL | KeyMod::ALT | modifiers, key_code), None),
            ..Keybindings::default()
        },
    )]
}

/// Um comando de apagar por parte de palavra: `whitespaceHeuristics: true` e `kbOpts` com a tecla.
fn delete_word_part(
    id: &str,
    direction: WordDirection,
    word_navigation_type: WordNavigationType,
    key_code: KeyCode,
) -> DeleteWordCommand {
    let mut opts = DeleteWordOptions::new(id, direction, true, word_navigation_type);
    opts.command.kb_opts = word_part_keybindings(key_code, 0);
    opts.part = true;
    DeleteWordCommand::new(opts)
}

/// Um comando de mover por parte de palavra (`WordPartLeftCommand` ou `WordPartRightCommand`).
fn cursor_word_part(
    id: &str,
    direction: WordDirection,
    in_selection_mode: bool,
    word_navigation_type: WordNavigationType,
    key_code: KeyCode,
) -> MoveWordCommand {
    let modifiers = if in_selection_mode { KeyMod::SHIFT } else { 0 };
    let mut opts = MoveWordOptions::new(id, direction, in_selection_mode, word_navigation_type);
    opts.command.kb_opts = word_part_keybindings(key_code, modifiers);
    opts.part = true;
    MoveWordCommand::new(opts)
}

/// `DeleteWordPartLeft`.
pub fn delete_word_part_left() -> DeleteWordCommand {
    delete_word_part("deleteWordPartLeft", WordDirection::Left, WordNavigationType::WordStart, KeyCode::Backspace)
}

/// `DeleteWordPartRight`.
pub fn delete_word_part_right() -> DeleteWordCommand {
    delete_word_part("deleteWordPartRight", WordDirection::Right, WordNavigationType::WordEnd, KeyCode::Delete)
}

/// `CursorWordPartLeft`.
pub fn cursor_word_part_left() -> MoveWordCommand {
    cursor_word_part("cursorWordPartLeft", WordDirection::Left, false, WordNavigationType::WordStart, KeyCode::LeftArrow)
}

/// `CursorWordPartLeftSelect`.
pub fn cursor_word_part_left_select() -> MoveWordCommand {
    cursor_word_part(
        "cursorWordPartLeftSelect",
        WordDirection::Left,
        true,
        WordNavigationType::WordStart,
        KeyCode::LeftArrow,
    )
}

/// `CursorWordPartRight`.
pub fn cursor_word_part_right() -> MoveWordCommand {
    cursor_word_part("cursorWordPartRight", WordDirection::Right, false, WordNavigationType::WordEnd, KeyCode::RightArrow)
}

/// `CursorWordPartRightSelect`.
pub fn cursor_word_part_right_select() -> MoveWordCommand {
    cursor_word_part(
        "cursorWordPartRightSelect",
        WordDirection::Right,
        true,
        WordNavigationType::WordEnd,
        KeyCode::RightArrow,
    )
}

thread_local! {
    /// O carregamento do módulo acontece uma vez; o registro de comandos é por thread.
    static REGISTERED: Cell<bool> = const { Cell::new(false) };
}

/// As chamadas `registerCommandAlias(...)` e `registerEditorCommand(...)` do módulo (o que o carregamento dele faz no
/// original). É idempotente: o registro acontece uma vez por thread.
pub fn register_word_part_operations() {
    if REGISTERED.replace(true) {
        return;
    }
    // Register previous id for compatibility purposes
    commands_registry().register_command_alias("cursorWordPartStartLeft", "cursorWordPartLeft");
    commands_registry().register_command_alias("cursorWordPartStartLeftSelect", "cursorWordPartLeftSelect");

    register_editor_command(Rc::new(delete_word_part_left()));
    register_editor_command(Rc::new(delete_word_part_right()));
    register_editor_command(Rc::new(cursor_word_part_left()));
    register_editor_command(Rc::new(cursor_word_part_left_select()));
    register_editor_command(Rc::new(cursor_word_part_right()));
    register_editor_command(Rc::new(cursor_word_part_right_select()));
}
