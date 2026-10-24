//! Porte de `src/vs/editor/contrib/comment/browser/comment.ts`: as ações do editor que comentam e descomentam
//! (`editor.action.commentLine` com Ctrl+/, `addCommentLine` e `removeCommentLine` com os acordes Ctrl+K Ctrl+C e
//! Ctrl+K Ctrl+U, `blockComment` com Shift+Alt+A) e o `registerEditorAction` do fim do arquivo
//! ([`register_comment`]).
//!
//! Representação: `abstract class CommentLineAction` e as três subclasses (que só diferem no `Type` e nos dados da
//! ação) são a struct [`CommentLineAction`] com o `Type` como campo, e as classes concretas são as funções
//! [`toggle_comment_line_action`], [`add_line_comment_action`] e [`remove_line_comment_action`], como em
//! `lines_operations`. O menu `Edit` (`5_insert`) das duas ações que o declaram também é registrado.
//!
//! Divergência: `accessor.get(ILanguageConfigurationService)` é o serviço que o editor recebeu
//! ([`CodeEditorWidget::language_configuration_service`]), a mesma decisão de `lines_operations`.

use std::cell::Cell;
use std::cmp::Ordering;
use std::rc::Rc;

use je_base::dynamic::Dynamic;
use je_base::key_codes::{KeyCode, KeyMod, key_chord};
use je_base::nls::localize;
use je_base::promise::Promise;
use je_instantiation::ServicesAccessor;
use je_platform::action::localize2;
use je_platform::actions::menu_ids;
use je_platform::keybinding::keybindings_registry::{KeybindingWeight, Keybindings, PlatformKeybindings};

use crate::code_editor_widget::{CodeEditorWidget, ICodeEditor};
use crate::comment::block_comment_command::BlockCommentCommand;
use crate::comment::line_comment_command::{LineCommentCommand, Type};
use crate::config::editor_options::comments::COMMENTS;
use crate::core_commands::{editor_text_focus, writable};
use crate::editor_common::Command as EditCommand;
use crate::editor_extensions::{
    ActionLabel, ActionOptions, CommandKeybindingsOptions, CommandMenuOptions, CommandOptions, EditorAction, EditorActionBase, register_editor_action,
};
use crate::range::Range;
use crate::selection::Selection;

/// `IActionOptions` de uma ação de comentário: precondição `writable`, atalho com o foco no texto e peso `EditorContrib`.
fn comment_action_options(id: &str, label_key: &str, label: &str, keybindings: Keybindings, menu_opts: Vec<CommandMenuOptions>) -> ActionOptions {
    ActionOptions {
        command: CommandOptions {
            precondition: Some(writable()),
            kb_opts: vec![CommandKeybindingsOptions::new(KeybindingWeight::EDITOR_CONTRIB, Some(editor_text_focus()), keybindings)],
            menu_opts,
            can_trigger_inline_edits: true,
            ..CommandOptions::new(id)
        },
        context_menu_opts: Vec::new(),
        label: ActionLabel::Localized { label: localize2(label_key, label), alias: None },
    }
}

fn edit_menu_item(order: f64, label_key: &str, label: &str) -> Vec<CommandMenuOptions> {
    vec![CommandMenuOptions::new(menu_ids::menubar_edit_menu(), "5_insert", order, localize(label_key, label, &[]))]
}

fn compare_starts(a: &Range, b: &Range) -> Ordering {
    Range::compare_ranges_using_starts(Some(a), Some(b)).cmp(&0)
}

/// `abstract class CommentLineAction` e as suas três subclasses.
pub struct CommentLineAction {
    action: EditorActionBase,
    type_: Type,
}

impl CommentLineAction {
    pub fn new(type_: Type, opts: ActionOptions) -> Self {
        Self { action: EditorActionBase::new(opts), type_ }
    }
}

/// Uma seleção com o índice original, para tirar a primeira linha de quem repetiria a linha de outra.
struct IndexedSelection {
    selection: Selection,
    index: usize,
    ignore_first_line: bool,
}

impl EditorAction for CommentLineAction {
    fn action_base(&self) -> &EditorActionBase {
        &self.action
    }

    fn run(&self, _accessor: Option<&ServicesAccessor>, editor: &Rc<CodeEditorWidget>, _args: &Dynamic) -> Promise<()> {
        let (Some(model), Some(selections)) = (editor.get_model(), editor.get_selections()) else {
            return Promise::resolved(());
        };
        let model_options = model.get_options();
        let comments = *editor.get_option(COMMENTS);

        let mut selections: Vec<IndexedSelection> =
            selections.into_iter().enumerate().map(|(index, selection)| IndexedSelection { selection, index, ignore_first_line: false }).collect();
        selections.sort_by(|a, b| compare_starts(&a.selection, &b.selection));

        // Remove selections that would result in copying the same line
        let mut prev = 0;
        for i in 1..selections.len() {
            if selections[prev].selection.end_line_number == selections[i].selection.start_line_number {
                // these two selections would copy the same line
                if selections[prev].index < selections[i].index {
                    // prev wins
                    selections[i].ignore_first_line = true;
                } else {
                    // curr wins
                    selections[prev].ignore_first_line = true;
                    prev = i;
                }
            }
        }

        let commands: Vec<Option<Box<dyn EditCommand>>> = selections
            .iter()
            .map(|s| -> Option<Box<dyn EditCommand>> {
                Some(Box::new(LineCommentCommand::new(
                    editor.language_configuration_service().clone(),
                    s.selection,
                    model_options.indent_size,
                    self.type_,
                    comments.insert_space,
                    comments.ignore_empty_lines,
                    Some(s.ignore_first_line),
                )))
            })
            .collect();

        editor.push_undo_stop();
        editor.execute_commands(Some(&self.action.command.id), commands);
        editor.push_undo_stop();
        Promise::resolved(())
    }
}

/// `ToggleCommentLineAction`.
pub fn toggle_comment_line_action() -> CommentLineAction {
    CommentLineAction::new(
        Type::Toggle,
        comment_action_options(
            "editor.action.commentLine",
            "comment.line",
            "Toggle Line Comment",
            Keybindings { primary: Some(KeyMod::CTRL_CMD | KeyCode::Slash.bits()), ..Keybindings::default() },
            edit_menu_item(1.0, "miToggleLineComment", "&&Toggle Line Comment"),
        ),
    )
}

/// `AddLineCommentAction`.
pub fn add_line_comment_action() -> CommentLineAction {
    CommentLineAction::new(
        Type::ForceAdd,
        comment_action_options(
            "editor.action.addCommentLine",
            "comment.line.add",
            "Add Line Comment",
            Keybindings { primary: Some(key_chord(KeyMod::CTRL_CMD | KeyCode::KeyK.bits(), KeyMod::CTRL_CMD | KeyCode::KeyC.bits())), ..Keybindings::default() },
            Vec::new(),
        ),
    )
}

/// `RemoveLineCommentAction`.
pub fn remove_line_comment_action() -> CommentLineAction {
    CommentLineAction::new(
        Type::ForceRemove,
        comment_action_options(
            "editor.action.removeCommentLine",
            "comment.line.remove",
            "Remove Line Comment",
            Keybindings { primary: Some(key_chord(KeyMod::CTRL_CMD | KeyCode::KeyK.bits(), KeyMod::CTRL_CMD | KeyCode::KeyU.bits())), ..Keybindings::default() },
            Vec::new(),
        ),
    )
}

/// `BlockCommentAction`.
pub struct BlockCommentAction {
    action: EditorActionBase,
}

impl BlockCommentAction {
    pub fn new() -> Self {
        Self {
            action: EditorActionBase::new(comment_action_options(
                "editor.action.blockComment",
                "comment.block",
                "Toggle Block Comment",
                Keybindings {
                    primary: Some(KeyMod::SHIFT | KeyMod::ALT | KeyCode::KeyA.bits()),
                    linux: Some(PlatformKeybindings { primary: Some(KeyMod::CTRL_CMD | KeyMod::SHIFT | KeyCode::KeyA.bits()), secondary: None }),
                    ..Keybindings::default()
                },
                edit_menu_item(2.0, "miToggleBlockComment", "Toggle &&Block Comment"),
            )),
        }
    }
}

impl Default for BlockCommentAction {
    fn default() -> Self {
        Self::new()
    }
}

impl EditorAction for BlockCommentAction {
    fn action_base(&self) -> &EditorActionBase {
        &self.action
    }

    fn run(&self, _accessor: Option<&ServicesAccessor>, editor: &Rc<CodeEditorWidget>, _args: &Dynamic) -> Promise<()> {
        let (Some(_), Some(selections)) = (editor.get_model(), editor.get_selections()) else {
            return Promise::resolved(());
        };
        let comments = *editor.get_option(COMMENTS);
        let commands: Vec<Option<Box<dyn EditCommand>>> = selections
            .into_iter()
            .map(|selection| -> Option<Box<dyn EditCommand>> {
                Some(Box::new(BlockCommentCommand::new(selection, comments.insert_space, editor.language_configuration_service().clone())))
            })
            .collect();

        editor.push_undo_stop();
        editor.execute_commands(Some(&self.action.command.id), commands);
        editor.push_undo_stop();
        Promise::resolved(())
    }
}

thread_local! {
    /// O carregamento do módulo acontece uma vez; o registro das ações é por thread.
    static REGISTERED: Cell<bool> = const { Cell::new(false) };
}

/// Os `registerEditorAction(...)` do fim do arquivo (o que o carregamento do módulo faz no original). É idempotente: o
/// registro acontece uma vez por thread.
pub fn register_comment() {
    if REGISTERED.replace(true) {
        return;
    }
    register_editor_action(toggle_comment_line_action);
    register_editor_action(add_line_comment_action);
    register_editor_action(remove_line_comment_action);
    register_editor_action(BlockCommentAction::new);
}
