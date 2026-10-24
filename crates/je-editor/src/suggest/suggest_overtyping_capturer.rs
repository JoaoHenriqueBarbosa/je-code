//! Porte de `src/vs/editor/contrib/suggest/browser/suggestOvertypingCapturer.ts`: o que o último `type` sobrescreveu
//! (`OvertypingCapturer`), que o `SnippetController2` lê para as variáveis de seleção ao inserir uma sugestão.
//!
//! Representação:
//! - `OvertypingCapturer implements IDisposable` é a struct [`OvertypingCapturer`], criada por
//!   [`OvertypingCapturer::new`], que devolve `Rc<Self>`; ela também implementa [`IOvertypingCapturer`] (o
//!   `getLastOvertypedInfo` que `snippet_variables.rs` consulta) e o `{ value, multiline }` é o [`OvertypedInfo`] de lá.
//! - O editor é o `Rc<CodeEditorWidget>` (`ICodeEditor` não é objeto de trait, tem métodos genéricos); o ouvinte de
//!   `onWillType` guarda só um [`Weak`] dele, porque o editor é dono do ouvinte e um `Rc` formaria um ciclo.
//! - O estado compartilhado pelos três ouvintes (`_lastOvertyped` e `_locked`) é o [`CapturerState`].
//! - `this._lastOvertyped[i] = ...` sobre um array vazio é o `push` na ordem dos índices; o `return` antecipado do
//!   limite de tamanho deixa a lista com os itens já capturados, como o array do original.
//! - `getLastOvertypedInfo(idx: number)` recebe `usize`: o teste `idx >= 0` do original é o próprio tipo.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use je_base::lifecycle::{DisposableStore, IDisposable};

use crate::code_editor_widget::{CodeEditorWidget, ICodeEditor};
use crate::snippet::snippet_variables::{IOvertypingCapturer, OvertypedInfo};
use crate::suggest::suggest_model::SuggestModel;

/// `_maxSelectionLength`.
const MAX_SELECTION_LENGTH: usize = 51200;

/// `_lastOvertyped` e `_locked`.
#[derive(Default)]
struct CapturerState {
    last_overtyped: RefCell<Vec<OvertypedInfo>>,
    locked: Cell<bool>,
}

/// `OvertypingCapturer`.
pub struct OvertypingCapturer {
    disposables: Rc<DisposableStore>,
    state: Rc<CapturerState>,
}

impl OvertypingCapturer {
    pub fn new(editor: &Rc<CodeEditorWidget>, suggest_model: &SuggestModel) -> Rc<Self> {
        let disposables = DisposableStore::new();
        let state = Rc::new(CapturerState::default());

        {
            let state = Rc::clone(&state);
            let weak_editor = Rc::downgrade(editor);
            editor.on_will_type().listen_in(
                move |_| {
                    let Some(editor) = weak_editor.upgrade() else {
                        return;
                    };
                    if state.locked.get() || !editor.has_model() {
                        return;
                    }

                    let selections = editor.get_selections().expect("o editor com modelo tem seleções");

                    // Check if it will overtype any selections
                    let will_overtype = selections.iter().any(|selection| !selection.is_empty());
                    if !will_overtype {
                        state.last_overtyped.borrow_mut().clear();
                        return;
                    }

                    let mut last_overtyped = Vec::with_capacity(selections.len());
                    let model = editor.get_model().expect("o editor tem modelo");
                    for selection in &selections {
                        // Check for overtyping capturer restrictions
                        if model.get_value_length_in_range(selection) > MAX_SELECTION_LENGTH {
                            *state.last_overtyped.borrow_mut() = last_overtyped;
                            return;
                        }
                        last_overtyped.push(OvertypedInfo {
                            value: model.get_value_in_range(selection),
                            multiline: selection.start_line_number != selection.end_line_number,
                        });
                    }
                    *state.last_overtyped.borrow_mut() = last_overtyped;
                },
                &disposables,
            );
        }

        {
            let state = Rc::clone(&state);
            suggest_model.on_did_trigger().listen_in(move |_| state.locked.set(true), &disposables);
        }

        {
            let state = Rc::clone(&state);
            suggest_model.on_did_cancel().listen_in(move |_| state.locked.set(false), &disposables);
        }

        Rc::new(Self { disposables, state })
    }
}

impl IOvertypingCapturer for OvertypingCapturer {
    fn get_last_overtyped_info(&self, idx: usize) -> Option<OvertypedInfo> {
        self.state.last_overtyped.borrow().get(idx).cloned()
    }
}

impl IDisposable for OvertypingCapturer {
    fn dispose(&self) {
        self.disposables.dispose();
    }
}
