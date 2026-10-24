//! Porte de `src/vs/editor/contrib/cursorUndo/browser/cursorUndo.ts`: o desfazer e refazer do cursor
//! (`CursorUndoRedoController`), as ações `cursorUndo` e `cursorRedo` e as chamadas de registro do fim do arquivo.
//!
//! Representação:
//! - `CursorUndoRedoController extends Disposable implements IEditorContribution` é a struct
//!   [`CursorUndoRedoController`], que guarda um [`Disposable`] (o `_register`) e implementa [`IEditorContribution`]. O
//!   estado mutável (`_isCursorUndoRedo`, `_undoStack`, `_redoStack`) fica em `Cell` e `RefCell`, e os ouvintes de evento
//!   guardam um [`Weak`] do controlador ([`listen_weak`]).
//! - `CursorState` e `StackElement` (privadas no original) são as structs [`CursorState`] e [`StackElement`].
//! - `CursorUndo` e `CursorRedo` (`extends EditorAction`) são uma struct só, [`CursorUndoRedoAction`], que guarda se a ação
//!   desfaz ou refaz; as duas diferem em dados e saem de [`cursor_undo`] e [`cursor_redo`].
//! - `editor.setScrollPosition({ scrollTop, scrollLeft })` é `set_scroll_position` com os dois campos preenchidos.

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use je_base::dynamic::Dynamic;
use je_base::key_codes::{KeyCode, KeyMod};
use je_base::lifecycle::{Disposable, IDisposable};
use je_base::promise::Promise;
use je_base::scrollable::INewScrollPosition;
use je_platform::action::localize2;
use je_platform::keybinding::keybindings_registry::{KeybindingWeight, Keybindings};

use crate::code_editor_widget::{
    CodeEditorWidget, EditorContributionInstantiation, ICodeEditor, IEditorContribution,
};
use crate::core_commands::{key, text_input_focus};
use crate::cursor_events::ICursorSelectionChangedEvent;
use crate::editor_extensions::{
    ActionLabel, ActionOptions, CommandKeybindingsOptions, CommandOptions, EditorAction, EditorActionBase,
    ServicesAccessor, register_editor_action, register_editor_contribution,
};
use crate::multicursor::listen_weak;
use crate::selection::Selection;

/// `CursorState`: as seleções de um instante. `equals(other)` é o `==`: mesma quantidade de seleções e cada uma igual
/// (`equalsSelection`) à de mesmo índice.
#[derive(Clone, Debug, PartialEq, Eq)]
struct CursorState {
    selections: Vec<Selection>,
}

/// `StackElement`: o estado do cursor e a rolagem do editor naquele instante.
struct StackElement {
    cursor_state: CursorState,
    scroll_top: f64,
    scroll_left: f64,
}

/// O número máximo de elementos da pilha de desfazer do cursor.
const MAX_UNDO_STACK_LENGTH: usize = 50;

/// `CursorUndoRedoController`.
pub struct CursorUndoRedoController {
    base: Rc<Disposable>,
    editor: Weak<CodeEditorWidget>,
    is_cursor_undo_redo: Cell<bool>,
    undo_stack: RefCell<Vec<StackElement>>,
    redo_stack: RefCell<Vec<StackElement>>,
}

impl CursorUndoRedoController {
    pub const ID: &'static str = "editor.contrib.cursorUndoRedoController";

    /// `CursorUndoRedoController.get(editor)`.
    pub fn get(editor: &Rc<CodeEditorWidget>) -> Option<Rc<CursorUndoRedoController>> {
        editor.get_contribution_as::<CursorUndoRedoController>(Self::ID)
    }

    pub fn new(editor: &Rc<CodeEditorWidget>) -> Rc<Self> {
        Rc::new_cyclic(|me: &Weak<Self>| {
            let base = Disposable::new();
            base.register(listen_weak(me, editor.on_did_change_model(), |this: &Self, _| this.clear_stacks()));
            base.register(listen_weak(me, editor.on_did_change_model_content(), |this: &Self, _| this.clear_stacks()));
            base.register(listen_weak(me, editor.on_did_change_cursor_selection(), |this: &Self, e| {
                this.record_selection_change(e);
            }));
            Self {
                base,
                editor: Rc::downgrade(editor),
                is_cursor_undo_redo: Cell::new(false),
                undo_stack: RefCell::new(Vec::new()),
                redo_stack: RefCell::new(Vec::new()),
            }
        })
    }

    /// `this._undoStack = []; this._redoStack = [];`.
    fn clear_stacks(&self) {
        self.undo_stack.borrow_mut().clear();
        self.redo_stack.borrow_mut().clear();
    }

    /// O ouvinte de `onDidChangeCursorSelection`.
    fn record_selection_change(&self, e: &ICursorSelectionChangedEvent) {
        if self.is_cursor_undo_redo.get() {
            return;
        }
        let Some(old_selections) = &e.old_selections else {
            return;
        };
        if e.old_model_version_id != e.model_version_id {
            return;
        }
        let Some(editor) = self.editor.upgrade() else {
            return;
        };
        let prev_state = CursorState { selections: old_selections.clone() };
        let mut undo_stack = self.undo_stack.borrow_mut();
        let is_equal_to_last_undo_stack = undo_stack.last().is_some_and(|last| last.cursor_state == prev_state);
        if !is_equal_to_last_undo_stack {
            undo_stack.push(StackElement {
                cursor_state: prev_state,
                scroll_top: editor.get_scroll_top(),
                scroll_left: editor.get_scroll_left(),
            });
            self.redo_stack.borrow_mut().clear();
            if undo_stack.len() > MAX_UNDO_STACK_LENGTH {
                // keep the cursor undo stack bounded
                undo_stack.remove(0);
            }
        }
    }

    /// O estado atual do editor como elemento de pilha.
    fn current_element(editor: &Rc<CodeEditorWidget>) -> StackElement {
        StackElement {
            cursor_state: CursorState { selections: editor.get_selections().unwrap_or_default() },
            scroll_top: editor.get_scroll_top(),
            scroll_left: editor.get_scroll_left(),
        }
    }

    pub fn cursor_undo(&self) {
        let Some(editor) = self.editor.upgrade().filter(|editor| editor.has_model()) else {
            return;
        };
        let Some(state) = self.undo_stack.borrow_mut().pop() else {
            return;
        };

        self.redo_stack.borrow_mut().push(Self::current_element(&editor));
        self.apply_state(&editor, &state);
    }

    pub fn cursor_redo(&self) {
        let Some(editor) = self.editor.upgrade().filter(|editor| editor.has_model()) else {
            return;
        };
        let Some(state) = self.redo_stack.borrow_mut().pop() else {
            return;
        };

        self.undo_stack.borrow_mut().push(Self::current_element(&editor));
        self.apply_state(&editor, &state);
    }

    /// `_applyState(stackElement)`.
    fn apply_state(&self, editor: &Rc<CodeEditorWidget>, stack_element: &StackElement) {
        self.is_cursor_undo_redo.set(true);
        editor.set_selections(&stack_element.cursor_state.selections, None, None);
        editor.set_scroll_position(
            INewScrollPosition { scroll_top: Some(stack_element.scroll_top), scroll_left: Some(stack_element.scroll_left) },
            None,
        );
        self.is_cursor_undo_redo.set(false);
    }
}

impl IDisposable for CursorUndoRedoController {
    fn dispose(&self) {
        self.base.dispose();
    }
}

impl IEditorContribution for CursorUndoRedoController {}

/// `CursorUndo` e `CursorRedo`.
pub struct CursorUndoRedoAction {
    action: EditorActionBase,
    redo: bool,
}

/// `CursorUndo`.
pub fn cursor_undo() -> CursorUndoRedoAction {
    CursorUndoRedoAction {
        action: EditorActionBase::new(ActionOptions {
            command: CommandOptions {
                kb_opts: vec![CommandKeybindingsOptions::new(
                    KeybindingWeight::EDITOR_CONTRIB,
                    Some(text_input_focus()),
                    Keybindings { primary: Some(key(KeyMod::CTRL_CMD, KeyCode::KeyU)), ..Keybindings::default() },
                )],
                ..CommandOptions::new("cursorUndo")
            },
            context_menu_opts: Vec::new(),
            label: ActionLabel::Localized { label: localize2("cursor.undo", "Cursor Undo"), alias: None },
        }),
        redo: false,
    }
}

/// `CursorRedo`.
pub fn cursor_redo() -> CursorUndoRedoAction {
    CursorUndoRedoAction {
        action: EditorActionBase::new(ActionOptions {
            command: CommandOptions::new("cursorRedo"),
            context_menu_opts: Vec::new(),
            label: ActionLabel::Localized { label: localize2("cursor.redo", "Cursor Redo"), alias: None },
        }),
        redo: true,
    }
}

impl EditorAction for CursorUndoRedoAction {
    fn action_base(&self) -> &EditorActionBase {
        &self.action
    }

    fn run(&self, _accessor: Option<&ServicesAccessor>, editor: &Rc<CodeEditorWidget>, _args: &Dynamic) -> Promise<()> {
        if let Some(controller) = CursorUndoRedoController::get(editor) {
            if self.redo {
                controller.cursor_redo();
            } else {
                controller.cursor_undo();
            }
        }
        Promise::resolved(())
    }
}

thread_local! {
    /// O carregamento do módulo acontece uma vez; o registro de ações e contribuição é por thread.
    static REGISTERED: Cell<bool> = const { Cell::new(false) };
}

/// As chamadas `registerEditorContribution(...)` e `registerEditorAction(...)` do fim do arquivo (o que o carregamento do
/// módulo faz no original). É idempotente: o registro acontece uma vez por thread.
pub fn register_cursor_undo() {
    if REGISTERED.replace(true) {
        return;
    }
    // eager because it needs to listen to record cursor state ASAP
    register_editor_contribution(
        CursorUndoRedoController::ID,
        Rc::new(|editor: &Rc<CodeEditorWidget>| -> Rc<dyn IEditorContribution> { CursorUndoRedoController::new(editor) }),
        EditorContributionInstantiation::Eager,
    );
    register_editor_action(cursor_undo);
    register_editor_action(cursor_redo);
}
