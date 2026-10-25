//! Porte de `src/vs/editor/contrib/inlineCompletions/browser/view/inlineEdits/inlineEditsViewProducer.ts`:
//! `InlineEditsViewAndDiffProducer`, que monta o `InlineEditWithChanges` e o `ModelPerInlineEdit` do estado de edição do
//! `InlineCompletionsModel` e cria a `InlineEditsView` que os lê. O original anota que a classe já não produz diffs e que
//! deveria mudar de nome ou sumir.
//!
//! Notas de tradução:
//! - `@IInstantiationService` (que criava a `InlineEditsView`) é o [`InlineEditsViewServices`], como na própria view;
//! - `state.inlineSuggestion.originalTextRef.getTransformer()` e `t.getPosition(offset)` são o `get_transformer` do
//!   [`TextModelValueReference`] e o `get_position` do transformador;
//! - `model.primaryPosition.read(undefined)` e `model.allPositions.read(undefined)` são o `get` dos observáveis (leitura
//!   sem rastreio); `state.inlineSuggestion.source.inlineSuggestions.commands ?? []` é `commands` clonado ou vazio;
//! - `_inlineEditModel` (público no original, lido pela experiência de novos usuários) é
//!   [`InlineEditsViewAndDiffProducer::inline_edit_model`].

use std::rc::Rc;

use je_base::lifecycle::{Disposable, IDisposable};
use je_base::observable::{IObservable, Observable, ObservableExt, derived};

use crate::edits::edit::BaseEdit;
use crate::code_editor_widget::{CodeEditorWidget, ICodeEditor};
use crate::edits::text_edit::{TextEdit, TextReplacement};
use crate::inline_completions::inline_completions_model::InlineCompletionsModel;
use crate::inline_completions::inline_suggestion_item::{InlineSuggestionAction, InlineSuggestionItemBase};
use crate::observable_code_editor::observable_code_editor;
use crate::position_to_offset::PositionOffsetTransformerBase;
use crate::range::Range;
use crate::text::abstract_text::AbstractText;

use super::components::gutter_indicator_view::{InlineSuggestionGutterMenuData, SimpleInlineSuggestModel};
use super::inline_edit_with_changes::InlineEditWithChanges;
use super::inline_edits_model::ModelPerInlineEdit;
use super::inline_edits_view::{InlineEditsView, InlineEditsViewServices};
use super::inline_edits_view_interface::InlineEditTabAction;

/// `InlineEditsViewAndDiffProducer`.
pub struct InlineEditsViewAndDiffProducer {
    base: Rc<Disposable>,
    inline_edit_model: IObservable<Option<Rc<ModelPerInlineEdit>>>,
    view: Rc<InlineEditsView>,
}

impl InlineEditsViewAndDiffProducer {
    pub fn new(
        editor: Rc<CodeEditorWidget>,
        model: IObservable<Option<Rc<InlineCompletionsModel>>>,
        show_collapsed: IObservable<bool>,
        services: InlineEditsViewServices,
    ) -> Rc<Self> {
        let base = Disposable::new();
        let editor_obs = observable_code_editor(&editor);

        let inline_edit: IObservable<Option<Rc<InlineEditWithChanges>>> = {
            let (model, editor) = (Rc::clone(&model), Rc::clone(&editor));
            derived(move |reader| {
                let model = model.read(reader)?;
                editor.get_model()?;

                let state = model.inline_edit_state.read(reader)?;
                let inline_suggestion = state.inline_suggestion()?;
                let action = inline_suggestion.action();

                let diff_edits = if let Some(InlineSuggestionAction::Edit(edit)) = &action {
                    let t = inline_suggestion.base().original_text_ref.get_transformer();
                    let edits: Vec<TextReplacement> = edit
                        .string_edit
                        .replacements()
                        .iter()
                        .map(|e| {
                            let inner_edit_range =
                                Range::from_positions(t.get_position(e.replace_range.start), t.get_position(e.replace_range.end_exclusive));
                            TextReplacement::new(inner_edit_range, e.new_text.clone())
                        })
                        .collect();
                    Some(TextEdit::new(edits))
                } else {
                    None
                };

                Some(Rc::new(InlineEditWithChanges::new(
                    Rc::clone(&inline_suggestion.base().original_text_ref),
                    action,
                    diff_edits,
                    model.primary_position.get(),
                    (*model.all_positions.get()).clone(),
                    inline_suggestion.source().inline_suggestions.commands.clone().unwrap_or_default(),
                    inline_suggestion,
                )))
            })
        };

        let inline_edit_model: IObservable<Option<Rc<ModelPerInlineEdit>>> = {
            let (model, inline_edit, editor_obs) = (Rc::clone(&model), Rc::clone(&inline_edit), Rc::clone(&editor_obs));
            derived(move |reader| {
                let model = model.read(reader)?;
                let edit = inline_edit.read(reader)?;

                let tab_action: IObservable<InlineEditTabAction> = {
                    let (model, editor_obs) = (Rc::clone(&model), Rc::clone(&editor_obs));
                    derived(move |reader| {
                        // @description tabAction
                        if editor_obs.is_focused.read(reader) {
                            if model.tab_should_jump_to_inline_edit.read(reader) {
                                return InlineEditTabAction::Jump;
                            }
                            if model.tab_should_accept_inline_edit.read(reader) {
                                return InlineEditTabAction::Accept;
                            }
                        }
                        InlineEditTabAction::Inactive
                    })
                };

                Some(Rc::new(ModelPerInlineEdit::new(model, edit, tab_action)))
            })
        };

        let view = InlineEditsView::new(
            Rc::clone(&editor),
            Rc::clone(&inline_edit_model),
            model.map(|model, _| model.map(|model| Rc::new(SimpleInlineSuggestModel::from_inline_completion_model(&model)))),
            inline_edit.map(|edit, _| edit.map(|edit| Rc::new(InlineSuggestionGutterMenuData::from_inline_suggestion(&edit.inline_completion)))),
            show_collapsed,
            services,
        );
        base.register(view.clone());

        Rc::new(Self { base, inline_edit_model, view })
    }

    /// `_inlineEditModel`.
    pub fn inline_edit_model(&self) -> IObservable<Option<Rc<ModelPerInlineEdit>>> {
        Rc::clone(&self.inline_edit_model)
    }

    /// `view`.
    pub fn view(&self) -> &Rc<InlineEditsView> {
        &self.view
    }
}

impl IDisposable for InlineEditsViewAndDiffProducer {
    fn dispose(&self) {
        self.base.dispose();
    }
}
