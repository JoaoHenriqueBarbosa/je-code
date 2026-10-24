//! A barra de menus do workbench: os menus `MenubarFileMenu`, `MenubarEditMenu`... do VS Code, na ordem e com os
//! rótulos e atalhos do original (`vs/workbench/browser/parts/titlebar/menubarControl.ts` e as contribuições de
//! cada menu). Um item cujo comando o je-code ainda não tem fica desabilitado, como o VS Code mostra um item
//! cuja precondição é falsa; os que existem enfileiram o [`Command`] para o shell rodar.

use std::cell::RefCell;
use std::rc::Rc;

use je_base::actions::{Action, ActionRef, Separator, SubmenuAction};
use je_base::dynamic::Dynamic;
use je_base::key_codes::{KeyCode, KeyMod, key_chord};
use je_base::keybindings::{ResolvedKeybinding, decode_keybinding};
use je_base::platform::Platform;
use je_platform::keybinding::us_layout_resolved_keybinding::USLayoutResolvedKeybinding;
use je_widgets::menu::GetKeyBinding;
use je_widgets::menu_bar::MenuBarMenu;

use super::Command;
use super::quick_pick::PickItem;

/// A fila de comandos que os itens de menu e os botões do shell alimentam; o shell esvazia depois de cada evento.
pub type CommandQueue = Rc<RefCell<Vec<Command>>>;

/// Um item: rótulo com mnemônico (`&`), comando (ou `None`, desabilitado) e atalho na codificação binária.
struct Item(&'static str, Option<Command>, u32);

fn item(label: &'static str, command: Option<Command>, keybinding: u32) -> Item {
    Item(label, command, keybinding)
}

const CTRL: u32 = KeyMod::CTRL_CMD;
const SHIFT: u32 = KeyMod::SHIFT;
const ALT: u32 = KeyMod::ALT;

fn code(key: KeyCode) -> u32 {
    key.0 as u32
}

/// O atalho de cada comando do menu, para o rótulo à direita do item (o `getKeybinding` do `MenubarControl`).
fn keybindings() -> Rc<RefCell<Vec<(String, u32)>>> {
    Rc::new(RefCell::new(Vec::new()))
}

/// Os atalhos dos itens (para o rótulo do menu) e as entradas da paleta de comandos, preenchidos por [`to_actions`].
struct Ctx {
    shortcuts: Rc<RefCell<Vec<(String, u32)>>>,
    palette: Rc<RefCell<Vec<PickItem>>>,
    os: je_base::platform::OperatingSystem,
}

/// O rótulo do atalho na codificação binária (`Ctrl+Shift+P`, `Ctrl+K Ctrl+O`).
fn keybinding_label(binary: u32, os: je_base::platform::OperatingSystem) -> Option<String> {
    let keybinding = decode_keybinding(binary, os).ok().flatten()?;
    USLayoutResolvedKeybinding::resolve_keybinding(&keybinding, os).into_iter().next()?.get_label()
}

/// O nome da categoria do menu na paleta (`File: Save`).
fn category(prefix: &str) -> &'static str {
    match prefix.split('.').next().unwrap_or(prefix) {
        "file" => "File",
        "edit" => "Edit",
        "selection" => "Selection",
        "view" => "View",
        "go" => "Go",
        "run" => "Run",
        "terminal" => "Terminal",
        _ => "Help",
    }
}

fn to_actions(items: Vec<Option<Item>>, queue: &CommandQueue, ctx: &Ctx, prefix: &str) -> Vec<ActionRef> {
    let shortcuts = &ctx.shortcuts;
    let mut actions = Vec::new();
    for (index, entry) in items.into_iter().enumerate() {
        let Some(Item(label, command, keybinding)) = entry else {
            actions.push(Separator::new() as ActionRef);
            continue;
        };
        let id = format!("{prefix}.{index}");
        if keybinding != 0 {
            shortcuts.borrow_mut().push((id.clone(), keybinding));
        }
        let enabled = command.is_some();
        if let Some(command) = &command {
            let title = label.replace('&', "");
            ctx.palette.borrow_mut().push(PickItem {
                label: format!("{}: {}", category(prefix), title.trim_end_matches("...")),
                description: None,
                keybinding: (keybinding != 0).then(|| keybinding_label(keybinding, ctx.os)).flatten(),
                command: command.clone(),
                recent: false,
            });
        }
        let callback = command.map(|command| {
            let queue = queue.clone();
            Rc::new(move |_: Dynamic| {
                queue.borrow_mut().push(command.clone());
                Dynamic::Undefined.into()
            }) as je_base::actions::ActionCallback
        });
        actions.push(Action::new(&id, label, "", enabled, callback) as ActionRef);
    }
    actions
}

/// Os menus da barra, com os itens ligados à fila de comandos, e o resolvedor de atalhos dos itens.
pub fn menubar_menus(queue: &CommandQueue) -> (Vec<MenuBarMenu>, GetKeyBinding, Vec<PickItem>) {
    use Command as C;
    let os = Platform::detect().os;
    let ctx = Ctx { shortcuts: keybindings(), palette: Rc::new(RefCell::new(Vec::new())), os };
    let shortcuts = ctx.shortcuts.clone();
    let file = vec![
        Some(item("&New Text File", Some(C::NewUntitled), CTRL | code(KeyCode::KeyN))),
        Some(item("New File...", None, CTRL | ALT | KeyMod::WIN_CTRL | code(KeyCode::KeyN))),
        Some(item("New &Window", None, CTRL | SHIFT | code(KeyCode::KeyN))),
        None,
        Some(item("&Open File...", Some(C::PromptOpenFile), CTRL | code(KeyCode::KeyO))),
        Some(item("Open &Folder...", Some(C::PromptOpenFolder), key_chord(CTRL | code(KeyCode::KeyK), CTRL | code(KeyCode::KeyO)))),
        Some(item("Open Wor&kspace from File...", None, 0)),
        None,
        Some(item("&Save", Some(C::Save), CTRL | code(KeyCode::KeyS))),
        Some(item("Save &As...", Some(C::PromptSaveAs), CTRL | SHIFT | code(KeyCode::KeyS))),
        Some(item("Save A&ll", Some(C::SaveAll), 0)),
        None,
        Some(item("A&uto Save", None, 0)),
        None,
        Some(item("Re&vert File", Some(C::Revert), 0)),
        Some(item("&Close Editor", Some(C::CloseActiveEditor), CTRL | code(KeyCode::KeyW))),
        Some(item("Close &Folder", None, key_chord(CTRL | code(KeyCode::KeyK), code(KeyCode::KeyF)))),
        Some(item("Clos&e Window", Some(C::Exit), ALT | code(KeyCode::F4))),
        None,
        Some(item("E&xit", Some(C::Exit), CTRL | code(KeyCode::KeyQ))),
    ];
    let edit = vec![
        Some(item("&Undo", Some(C::Undo), CTRL | code(KeyCode::KeyZ))),
        Some(item("&Redo", Some(C::Redo), CTRL | SHIFT | code(KeyCode::KeyZ))),
        None,
        Some(item("Cu&t", Some(C::Cut), CTRL | code(KeyCode::KeyX))),
        Some(item("&Copy", Some(C::Copy), CTRL | code(KeyCode::KeyC))),
        Some(item("&Paste", Some(C::Paste), CTRL | code(KeyCode::KeyV))),
        None,
        Some(item("&Find", Some(C::Find), CTRL | code(KeyCode::KeyF))),
        Some(item("&Replace", None, CTRL | code(KeyCode::KeyH))),
        None,
        Some(item("Find &in Files", Some(C::ShowSearch), CTRL | SHIFT | code(KeyCode::KeyF))),
        Some(item("Replace in Files", None, CTRL | SHIFT | code(KeyCode::KeyH))),
        None,
        Some(item("&Toggle Line Comment", None, CTRL | code(KeyCode::Slash))),
        Some(item("Toggle &Block Comment", None, CTRL | SHIFT | code(KeyCode::KeyA))),
    ];
    let selection = vec![
        Some(item("&Select All", Some(C::SelectAll), CTRL | code(KeyCode::KeyA))),
        Some(item("&Expand Selection", None, SHIFT | ALT | code(KeyCode::RightArrow))),
        Some(item("&Shrink Selection", None, SHIFT | ALT | code(KeyCode::LeftArrow))),
        None,
        Some(item("&Copy Line Up", None, CTRL | SHIFT | ALT | code(KeyCode::UpArrow))),
        Some(item("Co&py Line Down", None, CTRL | SHIFT | ALT | code(KeyCode::DownArrow))),
        Some(item("Mo&ve Line Up", None, ALT | code(KeyCode::UpArrow))),
        Some(item("Move &Line Down", None, ALT | code(KeyCode::DownArrow))),
        Some(item("&Duplicate Selection", None, 0)),
        None,
        Some(item("&Add Cursor Above", None, SHIFT | ALT | code(KeyCode::UpArrow))),
        Some(item("A&dd Cursor Below", None, SHIFT | ALT | code(KeyCode::DownArrow))),
        Some(item("Add C&ursors to Line Ends", None, SHIFT | ALT | code(KeyCode::KeyI))),
        Some(item("Add &Next Occurrence", None, CTRL | code(KeyCode::KeyD))),
        Some(item("Add P&revious Occurrence", None, 0)),
        Some(item("Select All &Occurrences", None, CTRL | SHIFT | code(KeyCode::KeyL))),
    ];
    let appearance = vec![
        Some(item("&Full Screen", Some(C::ToggleFullScreen), code(KeyCode::F11))),
        Some(item("Zen Mode", None, key_chord(CTRL | code(KeyCode::KeyK), code(KeyCode::KeyZ)))),
        Some(item("Centered Layout", None, 0)),
        None,
        Some(item("Menu &Bar", None, 0)),
        Some(item("&Primary Side Bar", Some(C::ToggleSidebar), CTRL | code(KeyCode::KeyB))),
        Some(item("Secondary Si&de Bar", None, CTRL | ALT | code(KeyCode::KeyB))),
        Some(item("&Status Bar", Some(C::ToggleStatusBar), 0)),
        Some(item("&Panel", Some(C::TogglePanel), CTRL | code(KeyCode::KeyJ))),
    ];
    let view_actions = {
        let mut actions = to_actions(
            vec![
                Some(item("&Command Palette...", Some(C::ShowCommands), CTRL | SHIFT | code(KeyCode::KeyP))),
                Some(item("&Open View...", None, 0)),
                None,
            ],
            queue,
            &ctx,
            "view.a",
        );
        actions.push(SubmenuAction::new("view.appearance", "&Appearance", to_actions(appearance, queue, &ctx, "view.appearance"), None) as ActionRef);
        actions.push(SubmenuAction::new("view.layout", "Editor &Layout", Vec::new(), None) as ActionRef);
        actions.extend(to_actions(
            vec![
                None,
                Some(item("&Explorer", Some(C::ShowExplorer), CTRL | SHIFT | code(KeyCode::KeyE))),
                Some(item("&Search", Some(C::ShowSearch), CTRL | SHIFT | code(KeyCode::KeyF))),
                Some(item("Source &Control", None, CTRL | SHIFT | code(KeyCode::KeyG))),
                Some(item("&Run", None, CTRL | SHIFT | code(KeyCode::KeyD))),
                Some(item("E&xtensions", None, CTRL | SHIFT | code(KeyCode::KeyX))),
                None,
                Some(item("&Problems", None, CTRL | SHIFT | code(KeyCode::KeyM))),
                Some(item("&Output", None, CTRL | SHIFT | code(KeyCode::KeyU))),
                Some(item("De&bug Console", None, CTRL | SHIFT | code(KeyCode::KeyY))),
                Some(item("&Terminal", Some(C::ToggleTerminal), CTRL | code(KeyCode::Backquote))),
                None,
                Some(item("&Word Wrap", None, ALT | code(KeyCode::KeyZ))),
            ],
            queue,
            &ctx,
            "view.b",
        ));
        actions
    };
    let go = vec![
        Some(item("&Back", None, CTRL | ALT | code(KeyCode::Minus))),
        Some(item("&Forward", None, CTRL | SHIFT | code(KeyCode::Minus))),
        Some(item("&Last Edit Location", None, key_chord(CTRL | code(KeyCode::KeyK), CTRL | code(KeyCode::KeyQ)))),
        None,
        Some(item("Next Editor", Some(C::NextEditor), CTRL | code(KeyCode::PageDown))),
        Some(item("Previous Editor", Some(C::PreviousEditor), CTRL | code(KeyCode::PageUp))),
        None,
        Some(item("Go to &File...", Some(C::QuickOpen), CTRL | code(KeyCode::KeyP))),
        Some(item("Go to Symbol in &Workspace...", None, CTRL | code(KeyCode::KeyT))),
        None,
        Some(item("Go to &Symbol in Editor...", None, CTRL | SHIFT | code(KeyCode::KeyO))),
        Some(item("Go to &Definition", None, code(KeyCode::F12))),
        Some(item("Go to &References", None, SHIFT | code(KeyCode::F12))),
        None,
        Some(item("Go to &Line/Column...", Some(C::PromptGoToLine), CTRL | code(KeyCode::KeyG))),
        Some(item("Go to &Bracket", None, CTRL | SHIFT | code(KeyCode::Backslash))),
    ];
    let run = vec![
        Some(item("&Start Debugging", None, code(KeyCode::F5))),
        Some(item("Run &Without Debugging", None, CTRL | code(KeyCode::F5))),
        Some(item("&Stop Debugging", None, SHIFT | code(KeyCode::F5))),
        Some(item("&Restart Debugging", None, CTRL | SHIFT | code(KeyCode::F5))),
        None,
        Some(item("Open &Configurations", None, 0)),
        Some(item("A&dd Configuration...", None, 0)),
        None,
        Some(item("Toggle &Breakpoint", None, code(KeyCode::F9))),
    ];
    let terminal = vec![
        Some(item("&New Terminal", Some(C::NewTerminal), CTRL | SHIFT | code(KeyCode::Backquote))),
        Some(item("&Split Terminal", None, CTRL | SHIFT | code(KeyCode::Digit5))),
        None,
        Some(item("Run &Task...", None, 0)),
        Some(item("Run &Build Task...", None, CTRL | SHIFT | code(KeyCode::KeyB))),
    ];
    let help = vec![
        Some(item("&Welcome", None, 0)),
        Some(item("Show All &Commands", Some(C::ShowCommands), CTRL | SHIFT | code(KeyCode::KeyP))),
        Some(item("&Documentation", None, 0)),
        None,
        Some(item("&Keyboard Shortcuts Reference", None, key_chord(CTRL | code(KeyCode::KeyK), CTRL | code(KeyCode::KeyR)))),
        None,
        Some(item("&About", None, 0)),
    ];
    let menus = vec![
        MenuBarMenu { label: "&File".to_owned(), actions: to_actions(file, queue, &ctx, "file") },
        MenuBarMenu { label: "&Edit".to_owned(), actions: to_actions(edit, queue, &ctx, "edit") },
        MenuBarMenu { label: "&Selection".to_owned(), actions: to_actions(selection, queue, &ctx, "selection") },
        MenuBarMenu { label: "&View".to_owned(), actions: view_actions },
        MenuBarMenu { label: "&Go".to_owned(), actions: to_actions(go, queue, &ctx, "go") },
        MenuBarMenu { label: "&Run".to_owned(), actions: to_actions(run, queue, &ctx, "run") },
        MenuBarMenu { label: "&Terminal".to_owned(), actions: to_actions(terminal, queue, &ctx, "terminal") },
        MenuBarMenu { label: "&Help".to_owned(), actions: to_actions(help, queue, &ctx, "help") },
    ];
    let get_key_binding: GetKeyBinding = Rc::new(move |action: &ActionRef| {
        let id = action.id();
        let binary = shortcuts.borrow().iter().find(|(item, _)| *item == id.as_str()).map(|(_, kb)| *kb)?;
        let keybinding = decode_keybinding(binary, os).ok().flatten()?;
        USLayoutResolvedKeybinding::resolve_keybinding(&keybinding, os).into_iter().next().map(|k| k as Rc<dyn ResolvedKeybinding>)
    });
    let mut palette = ctx.palette.borrow().clone();
    palette.dedup_by(|a, b| a.command == b.command);
    (menus, get_key_binding, palette)
}
