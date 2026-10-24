//! Porte de `src/vs/editor/contrib/lineSelection/browser/lineSelection.ts`: a ação `expandLineSelection`
//! ([`ExpandLineSelectionAction`]) e a chamada de registro do fim do arquivo.
//!
//! Representação:
//! - `ExpandLineSelectionAction extends EditorAction` é a struct [`ExpandLineSelectionAction`], que implementa
//!   [`EditorAction`]. O `args: ExpandLinesSelectionArgs` (`{ source?: string }`) é o `Dynamic` do comando, lido por
//!   [`source_arg`]; `args = args || {}` não muda nada, porque um `Dynamic` ausente não tem `source`.
//! - `editor._getViewModel()` é [`CodeEditorWidget::get_view_model`], `viewModel.model` é `IViewModel::model` e
//!   `CursorMoveCommands.expandLineSelection` é [`expand_line_selection`].

use std::cell::Cell;
use std::rc::Rc;

use je_base::dynamic::Dynamic;
use je_base::key_codes::{KeyCode, KeyMod};
use je_base::promise::Promise;
use je_platform::action::localize2;
use je_platform::keybinding::keybindings_registry::{KeybindingWeight, Keybindings};

use crate::code_editor_widget::{CodeEditorWidget, ICodeEditor};
use crate::core_commands::{key, text_input_focus};
use crate::cursor_move_commands::expand_line_selection;
use crate::editor_extensions::{
    ActionLabel, ActionOptions, CommandKeybindingsOptions, CommandOptions, EditorAction, EditorActionBase,
    ServicesAccessor, register_editor_action,
};
use crate::multicursor::source_arg;
use crate::view_model_types::{CursorChangeReason, IViewModel};

/// `ExpandLineSelectionAction`.
pub struct ExpandLineSelectionAction {
    action: EditorActionBase,
}

impl ExpandLineSelectionAction {
    pub fn new() -> Self {
        Self {
            action: EditorActionBase::new(ActionOptions {
                command: CommandOptions {
                    kb_opts: vec![CommandKeybindingsOptions::new(
                        KeybindingWeight::EDITOR_CORE,
                        Some(text_input_focus()),
                        Keybindings { primary: Some(key(KeyMod::CTRL_CMD, KeyCode::KeyL)), ..Keybindings::default() },
                    )],
                    ..CommandOptions::new("expandLineSelection")
                },
                context_menu_opts: Vec::new(),
                label: ActionLabel::Localized { label: localize2("expandLineSelection", "Expand Line Selection"), alias: None },
            }),
        }
    }
}

impl Default for ExpandLineSelectionAction {
    fn default() -> Self {
        Self::new()
    }
}

impl EditorAction for ExpandLineSelectionAction {
    fn action_base(&self) -> &EditorActionBase {
        &self.action
    }

    fn run(&self, _accessor: Option<&ServicesAccessor>, editor: &Rc<CodeEditorWidget>, args: &Dynamic) -> Promise<()> {
        if !editor.has_model() {
            return Promise::resolved(());
        }
        let Some(view_model) = editor.get_view_model() else {
            return Promise::resolved(());
        };
        let source = source_arg(args);
        let source = source.as_deref();

        view_model.model().push_stack_element();
        view_model.set_cursor_states(
            source,
            CursorChangeReason::Explicit,
            Some(expand_line_selection(&*view_model, &view_model.get_cursor_states())),
        );
        view_model.reveal_all_cursors(source, true, None);
        Promise::resolved(())
    }
}

thread_local! {
    /// O carregamento do módulo acontece uma vez; o registro da ação é por thread.
    static REGISTERED: Cell<bool> = const { Cell::new(false) };
}

/// A chamada `registerEditorAction(ExpandLineSelectionAction)` do fim do arquivo (o que o carregamento do módulo faz no
/// original). É idempotente: o registro acontece uma vez por thread.
pub fn register_line_selection() {
    if REGISTERED.replace(true) {
        return;
    }
    register_editor_action(ExpandLineSelectionAction::new);
}
