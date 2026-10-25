//! Porte de `src/vs/editor/contrib/inPlaceReplace/browser/inPlaceReplace.ts`: o controlador
//! (`InPlaceReplaceController`), as ações `editor.action.inPlaceReplace.up` e `.down` e as chamadas de registro do fim do
//! arquivo.
//!
//! Representação:
//! - `@IEditorWorkerService` no construtor é o serviço recebido por [`register_in_place_replace`], como o
//!   `register_find_controller` faz com os serviços do `CommonFindController`.
//! - `InPlaceReplaceUp` e `InPlaceReplaceDown` (`extends EditorAction`) são uma struct só, [`InPlaceReplaceAction`], que
//!   guarda o sentido; as duas diferem em dados e saem de [`in_place_replace_up`] e [`in_place_replace_down`].
//! - O estado mutável do controlador (`currentRequest`, `decorationRemover`) fica em `RefCell`, e os retornos das
//!   promessas guardam um [`Weak`] do controlador.
//!
//! Divergências:
//! - O `import './inPlaceReplace.css'` não existe: a UI nativa não usa CSS. A decoração `valueSetReplacement` (contorno
//!   de `editorBracketMatch.border`) é a classe de decoração nativa de mesmo nome, lida pelo desenho do editor.

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use je_base::async_::{CancelablePromise, create_cancelable_promise, timeout};
use je_base::dynamic::Dynamic;
use je_base::errors;
use je_base::key_codes::{KeyCode, KeyMod};
use je_base::lifecycle::IDisposable;
use je_base::promise::Promise;
use je_platform::action::localize2;
use je_platform::keybinding::keybindings_registry::{KeybindingWeight, Keybindings};

use crate::code_editor_widget::{
    CodeEditorWidget, EditorContributionInstantiation, ICodeEditor, IEditorContribution,
};
use crate::core_commands::{editor_text_focus, key, writable};
use crate::editor_decorations_collection::EditorDecorationsCollection;
use crate::editor_extensions::{
    ActionLabel, ActionOptions, CommandKeybindingsOptions, CommandOptions, EditorAction, EditorActionBase,
    ServicesAccessor, register_editor_action, register_editor_contribution,
};
use crate::editor_state::{EditorState, code_editor_state_flag};
use crate::editor_worker::IEditorWorkerService;
use crate::in_place_replace_command::InPlaceReplaceCommand;
use crate::inplace_replace_support::InplaceReplaceSupportResult;
use crate::model::{ModelDecorationOptionsInit, ModelDeltaDecoration};
use crate::range::Range;
use crate::selection::Selection;
use crate::text_model::ModelDecorationOptions;

thread_local! {
    /// `InPlaceReplaceController.DECORATION`.
    static DECORATION: Rc<ModelDecorationOptions> = ModelDecorationOptions::register(&ModelDecorationOptionsInit {
        description: "in-place-replace".to_owned(),
        class_name: Some("valueSetReplacement".to_owned()),
        ..ModelDecorationOptionsInit::default()
    });

    /// O carregamento do módulo acontece uma vez; o registro de ações e contribuição é por thread.
    static REGISTERED: Cell<bool> = const { Cell::new(false) };
}

/// Quanto tempo a decoração do valor trocado fica visível, em milissegundos.
const DECORATION_DURATION_MS: u64 = 350;

/// `InPlaceReplaceController`.
pub struct InPlaceReplaceController {
    me: Weak<Self>,
    editor: Weak<CodeEditorWidget>,
    editor_worker_service: Rc<dyn IEditorWorkerService>,
    decorations: EditorDecorationsCollection,
    current_request: RefCell<Option<CancelablePromise<Option<InplaceReplaceSupportResult>>>>,
    decoration_remover: RefCell<Option<CancelablePromise<()>>>,
}

impl InPlaceReplaceController {
    pub const ID: &'static str = "editor.contrib.inPlaceReplaceController";

    /// `InPlaceReplaceController.get(editor)`.
    pub fn get(editor: &Rc<CodeEditorWidget>) -> Option<Rc<InPlaceReplaceController>> {
        editor.get_contribution_as::<InPlaceReplaceController>(Self::ID)
    }

    pub fn new(editor: &Rc<CodeEditorWidget>, editor_worker_service: Rc<dyn IEditorWorkerService>) -> Rc<Self> {
        Rc::new_cyclic(|me: &Weak<Self>| Self {
            me: me.clone(),
            editor: Rc::downgrade(editor),
            editor_worker_service,
            decorations: editor.create_decorations_collection(None),
            current_request: RefCell::new(None),
            decoration_remover: RefCell::new(None),
        })
    }

    pub fn run(&self, source: &str, up: bool) -> Option<Promise<()>> {
        // cancel any pending request
        if let Some(request) = self.current_request.borrow().as_ref() {
            request.cancel();
        }

        let editor = self.editor.upgrade()?;
        let (model, editor_selection) = (editor.get_model()?, editor.get_selection()?);
        let mut selection = editor_selection;
        if selection.start_line_number != selection.end_line_number {
            // Can't accept multiline selection
            return None;
        }

        let state = EditorState::new(editor.as_ref(), code_editor_state_flag::VALUE | code_editor_state_flag::POSITION);
        let model_uri = model.uri();
        if !self.editor_worker_service.can_navigate_value_set(&model_uri) {
            return Some(Promise::resolved(()));
        }

        let service = Rc::clone(&self.editor_worker_service);
        let request_selection = *selection;
        let request = create_cancelable_promise(move |_token| service.navigate_value_set(&model_uri, request_selection, up));
        *self.current_request.borrow_mut() = Some(request.clone());

        let me = self.me.clone();
        let source = source.to_owned();
        Some(
            request
                .then(move |result| {
                    let Some(result) = result.filter(|result| !result.value.is_empty()) else {
                        // No proper result
                        return;
                    };
                    let Some(this) = me.upgrade() else {
                        return;
                    };
                    let Some(editor) = this.editor.upgrade() else {
                        return;
                    };

                    if !state.validate(editor.as_ref()) {
                        // state has changed
                        return;
                    }

                    // Selection
                    let edit_range = result.range;
                    let highlight_range = result.range;
                    let diff = result.value.len() as i32 - (selection.end_column - selection.start_column);

                    // highlight
                    let highlight_range = Range::new(
                        highlight_range.start_line_number,
                        highlight_range.start_column,
                        highlight_range.end_line_number,
                        highlight_range.start_column + result.value.len() as i32,
                    );
                    if diff > 1 {
                        selection = Selection::new(
                            selection.start_line_number,
                            selection.start_column,
                            selection.end_line_number,
                            selection.end_column + diff - 1,
                        );
                    }

                    // Insert new text
                    let command = InPlaceReplaceCommand::new(edit_range, selection, result.value);

                    editor.push_undo_stop();
                    editor.execute_command(Some(source.as_str()), Box::new(command));
                    editor.push_undo_stop();

                    // add decoration
                    let options = DECORATION.with(Rc::clone);
                    this.decorations.set(&[ModelDeltaDecoration { range: highlight_range, options }]);

                    // remove decoration after delay
                    if let Some(remover) = this.decoration_remover.borrow().as_ref() {
                        remover.cancel();
                    }
                    let remover = timeout(DECORATION_DURATION_MS);
                    let weak = Rc::downgrade(&this);
                    let _ = remover
                        .then(move |()| {
                            if let Some(this) = weak.upgrade() {
                                this.decorations.clear();
                            }
                        })
                        .catch(|error| errors::on_unexpected_error(&error));
                    *this.decoration_remover.borrow_mut() = Some(remover);
                })
                .catch(|error| errors::on_unexpected_error(&error)),
        )
    }
}

impl IDisposable for InPlaceReplaceController {
    fn dispose(&self) {}
}

impl IEditorContribution for InPlaceReplaceController {}

/// `InPlaceReplaceUp` e `InPlaceReplaceDown`.
pub struct InPlaceReplaceAction {
    action: EditorActionBase,
    up: bool,
}

impl InPlaceReplaceAction {
    fn new(id: &str, label: ActionLabel, primary: KeyCode, up: bool) -> Self {
        Self {
            action: EditorActionBase::new(ActionOptions {
                command: CommandOptions {
                    precondition: Some(writable()),
                    kb_opts: vec![CommandKeybindingsOptions::new(
                        KeybindingWeight::EDITOR_CONTRIB,
                        Some(editor_text_focus()),
                        Keybindings {
                            primary: Some(key(KeyMod::CTRL_CMD | KeyMod::SHIFT, primary)),
                            ..Keybindings::default()
                        },
                    )],
                    ..CommandOptions::new(id)
                },
                context_menu_opts: Vec::new(),
                label,
            }),
            up,
        }
    }
}

/// `InPlaceReplaceUp`.
pub fn in_place_replace_up() -> InPlaceReplaceAction {
    InPlaceReplaceAction::new(
        "editor.action.inPlaceReplace.up",
        ActionLabel::Localized { label: localize2("InPlaceReplaceAction.previous.label", "Replace with Previous Value"), alias: None },
        KeyCode::Comma,
        false,
    )
}

/// `InPlaceReplaceDown`.
pub fn in_place_replace_down() -> InPlaceReplaceAction {
    InPlaceReplaceAction::new(
        "editor.action.inPlaceReplace.down",
        ActionLabel::Localized { label: localize2("InPlaceReplaceAction.next.label", "Replace with Next Value"), alias: None },
        KeyCode::Period,
        true,
    )
}

impl EditorAction for InPlaceReplaceAction {
    fn action_base(&self) -> &EditorActionBase {
        &self.action
    }

    fn run(&self, _accessor: Option<&ServicesAccessor>, editor: &Rc<CodeEditorWidget>, _args: &Dynamic) -> Promise<()> {
        match InPlaceReplaceController::get(editor) {
            Some(controller) => controller.run(&self.action.command.id, self.up).unwrap_or_else(|| Promise::resolved(())),
            None => Promise::resolved(()),
        }
    }
}

/// As chamadas `registerEditorContribution(...)` e `registerEditorAction(...)` do fim do arquivo (o que o carregamento do
/// módulo faz no original). É idempotente: o registro acontece uma vez por thread.
pub fn register_in_place_replace(editor_worker_service: Rc<dyn IEditorWorkerService>) {
    if REGISTERED.replace(true) {
        return;
    }
    register_editor_contribution(
        InPlaceReplaceController::ID,
        Rc::new(move |editor: &Rc<CodeEditorWidget>| -> Rc<dyn IEditorContribution> {
            InPlaceReplaceController::new(editor, Rc::clone(&editor_worker_service))
        }),
        EditorContributionInstantiation::Lazy,
    );
    register_editor_action(in_place_replace_up);
    register_editor_action(in_place_replace_down);
}
