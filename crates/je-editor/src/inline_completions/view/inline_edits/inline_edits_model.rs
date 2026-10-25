//! Porte de `src/vs/editor/contrib/inlineCompletions/browser/view/inlineEdits/inlineEditsModel.ts`:
//! `ModelPerInlineEdit`, a fachada do `InlineCompletionsModel` para uma edição em linha.
//!
//! Notas de tradução:
//! - o original marca a classe como obsoleta (ela é criada a cada cálculo e não vale por identificador da edição);
//!   continua aqui porque as views de inline edits a recebem;
//! - `isSuggestionInViewport(editor, item, reader)` recebe o `scrollTop` do `observableCodeEditor(editor)`, que é o
//!   que a função do porte pede;
//! - `onDidAccept` é um campo (o evento do modelo), `accept(alternativeAction)` aceita no editor do próprio modelo
//!   (`undefined` no original) e devolve nada: a promessa de `accept` é descartada, como no original;
//! - `setTimeout0` não existe no porte (ver `je_base::platform`): é um `set_timeout` de zero milissegundo, que roda na
//!   próxima macrotarefa. O `addRef`/`removeRef` do item cercam a espera, como no original;
//! - `Date.now()` é [`date_now`].

use std::rc::Rc;

use je_base::event::Event;
use je_base::observable::base::IObservable;
use je_base::observable::derived;
use je_base::runtime::{self, date_now};

use super::inline_edit_with_changes::InlineEditWithChanges;
use super::inline_edits_view_interface::{InlineCompletionViewData, InlineCompletionViewKind, InlineEditTabAction};
use crate::inline_completions::inline_completions_model::{InlineCompletionsModel, is_suggestion_in_viewport};
use crate::inline_completions::inline_suggestion_item::{InlineSuggestHint, InlineSuggestionItemBase};
use crate::inline_completions::provide_inline_completions::InlineCompletionEditorType;
use crate::observable_code_editor::observable_code_editor;

/// `ModelPerInlineEdit`. Warning: this is not per inline edit id and gets created often.
pub struct ModelPerInlineEdit {
    model: Rc<InlineCompletionsModel>,
    pub editor_type: InlineCompletionEditorType,
    pub display_location: Option<InlineSuggestHint>,
    /// Determines if the inline suggestion is fully in the view port.
    pub in_view_port: IObservable<bool>,
    pub on_did_accept: Event<()>,
    pub inline_edit: Rc<InlineEditWithChanges>,
    pub tab_action: IObservable<InlineEditTabAction>,
}

impl ModelPerInlineEdit {
    pub fn new(model: Rc<InlineCompletionsModel>, inline_edit: Rc<InlineEditWithChanges>, tab_action: IObservable<InlineEditTabAction>) -> Self {
        let editor_type = model.editor_type;
        let display_location = inline_edit.inline_completion.base().hint.clone();

        let in_view_port = {
            let (model, inline_edit) = (Rc::clone(&model), Rc::clone(&inline_edit));
            let scroll_top = Rc::clone(&observable_code_editor(model.editor()).scroll_top);
            derived(move |reader| is_suggestion_in_viewport(model.editor(), &scroll_top, &inline_edit.inline_completion, Some(reader)))
        };
        let on_did_accept = model.on_did_accept();

        Self { model, editor_type, display_location, in_view_port, on_did_accept, inline_edit, tab_action }
    }

    /// `accept(alternativeAction)`.
    pub fn accept(&self, alternative_action: bool) {
        let _ = self.model.accept(self.model.editor(), alternative_action);
    }

    /// `handleInlineEditShownNextFrame(viewKind, viewData)`.
    pub fn handle_inline_edit_shown_next_frame(&self, view_kind: InlineCompletionViewKind, view_data: InlineCompletionViewData) {
        let item = self.inline_edit.inline_completion.clone();
        let time_when_shown = date_now() as f64;
        item.add_ref();
        let model = Rc::clone(&self.model);
        let _ = runtime::set_timeout(
            move || {
                let _ = model.handle_inline_suggestion_shown(&item, view_kind, view_data, time_when_shown);
                item.remove_ref();
            },
            0,
        );
    }
}
