//! Porte de `src/vs/editor/contrib/suggest/browser/suggestCommitCharacters.ts`: aceita a sugestão em foco quando o
//! caractere digitado é um dos caracteres de confirmação dela (`CommitCharacterController`).
//!
//! Representação:
//! - `CommitCharacterController` é a struct [`CommitCharacterController`], criada por
//!   [`CommitCharacterController::new`], que devolve `Rc<Self>`; os ouvintes guardam só um [`Weak`] dela e, do widget,
//!   do modelo e do editor, só um [`Weak`] também (cada um é dono de um emissor que guarda o ouvinte, e um `Rc`
//!   formaria um ciclo).
//! - `SuggestWidget` é o [`ISuggestWidget`] de `suggest_widget_contract.rs`; `SuggestModel` e `State` são os de
//!   `suggest_model.rs`; `accept` é um `Rc<dyn Fn(&ISelectedSuggestion)>` (o retorno `unknown` é descartado).
//! - `CharacterSet` guarda as unidades UTF-16 (`ch.charCodeAt(0)` e `text.charCodeAt(n)`); um `text` vazio é o
//!   `charCodeAt(-1)` (`NaN`), que nenhum conjunto contém.
//! - O `_active` é o [`Active`] dentro de um `RefCell`. `accept` roda depois de soltar o empréstimo, porque o aceite
//!   pode reentrar no controlador (por `onDidCancel` e `reset`).

use std::cell::RefCell;
use std::rc::{Rc, Weak};

use je_base::lifecycle::{DisposableStore, IDisposable};

use crate::character_classifier::CharacterSet;
use crate::code_editor_widget::{CodeEditorWidget, ICodeEditor};
use crate::config::editor_options::registry::ACCEPT_SUGGESTION_ON_COMMIT_CHARACTER;
use crate::suggest::suggest_model::{State, SuggestModel};
use crate::suggest::suggest_widget_contract::{ISelectedSuggestion, ISuggestWidget};

/// `_active`.
struct Active {
    accept_characters: CharacterSet,
    item: ISelectedSuggestion,
}

/// `CommitCharacterController`.
pub struct CommitCharacterController {
    disposables: Rc<DisposableStore>,
    active: RefCell<Option<Active>>,
}

impl CommitCharacterController {
    pub fn new(
        editor: &Rc<CodeEditorWidget>,
        widget: &Rc<dyn ISuggestWidget>,
        model: &Rc<SuggestModel>,
        accept: Rc<dyn Fn(&ISelectedSuggestion)>,
    ) -> Rc<Self> {
        let controller = Rc::new(Self { disposables: DisposableStore::new(), active: RefCell::new(None) });
        let disposables = &controller.disposables;

        {
            let weak = Rc::downgrade(&controller);
            model.on_did_suggest().listen_in(
                move |e| {
                    if e.completion_model.items().is_empty()
                        && let Some(controller) = weak.upgrade()
                    {
                        controller.reset();
                    }
                },
                disposables,
            );
        }
        {
            let weak = Rc::downgrade(&controller);
            model.on_did_cancel().listen_in(
                move |_| {
                    if let Some(controller) = weak.upgrade() {
                        controller.reset();
                    }
                },
                disposables,
            );
        }

        {
            let weak = Rc::downgrade(&controller);
            let weak_widget = Rc::downgrade(widget);
            widget.on_did_show().listen_in(
                move |_| {
                    if let (Some(controller), Some(widget)) = (weak.upgrade(), weak_widget.upgrade()) {
                        controller.on_item(widget.get_focused_item());
                    }
                },
                disposables,
            );
        }
        {
            let weak = Rc::downgrade(&controller);
            widget.on_did_focus().listen_in(
                move |selected| {
                    if let Some(controller) = weak.upgrade() {
                        controller.on_item(Some(selected.clone()));
                    }
                },
                disposables,
            );
        }
        {
            let weak = Rc::downgrade(&controller);
            widget.on_did_hide().listen_in(
                move |_| {
                    if let Some(controller) = weak.upgrade() {
                        controller.reset();
                    }
                },
                disposables,
            );
        }

        {
            let weak = Rc::downgrade(&controller);
            let weak_widget: Weak<dyn ISuggestWidget> = Rc::downgrade(widget);
            let weak_model = Rc::downgrade(model);
            let weak_editor = Rc::downgrade(editor);
            editor.on_will_type().listen_in(
                move |text| {
                    let (Some(controller), Some(widget), Some(model), Some(editor)) =
                        (weak.upgrade(), weak_widget.upgrade(), weak_model.upgrade(), weak_editor.upgrade())
                    else {
                        return;
                    };
                    if widget.is_frozen() || model.state() == State::Idle {
                        return;
                    }
                    let Some(&ch) = text.units().last() else {
                        return;
                    };
                    let item = {
                        let active = controller.active.borrow();
                        let Some(active) = active.as_ref() else {
                            return;
                        };
                        if !active.accept_characters.has(i32::from(ch)) {
                            return;
                        }
                        active.item.clone()
                    };
                    if *editor.get_option(ACCEPT_SUGGESTION_ON_COMMIT_CHARACTER) {
                        accept(&item);
                    }
                },
                disposables,
            );
        }

        controller
    }

    fn on_item(&self, selected: Option<ISelectedSuggestion>) {
        let Some(selected) = selected else {
            // no item or no commit characters
            self.reset();
            return;
        };
        let commit_characters = match selected.item.completion.borrow().commit_characters.clone() {
            Some(characters) if !characters.is_empty() => characters,
            _ => {
                // no item or no commit characters
                self.reset();
                return;
            }
        };

        if self.active.borrow().as_ref().is_some_and(|active| active.item.item.is_same(&selected.item)) {
            // still the same item
            return;
        }

        // keep item and its commit characters
        let mut accept_characters = CharacterSet::new();
        for ch in &commit_characters {
            if let Some(unit) = ch.encode_utf16().next() {
                accept_characters.add(i32::from(unit));
            }
        }
        *self.active.borrow_mut() = Some(Active { accept_characters, item: selected });
    }

    pub fn reset(&self) {
        *self.active.borrow_mut() = None;
    }
}

impl IDisposable for CommitCharacterController {
    fn dispose(&self) {
        self.disposables.dispose();
    }
}
