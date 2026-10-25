//! Porte de `src/vs/editor/contrib/format/browser/formattingEdit.ts`: `FormattingEdit`, que aplica as edições de um
//! formatador no editor, com pilha de desfazer e rolagem estável.
//!
//! Decisões de tradução:
//! - O editor é o [`CodeEditorWidget`] (`ICodeEditor` não é objeto de trait, tem métodos genéricos).
//! - O `TextEdit` do porte ([`TextEdit`]) sempre traz `range` e `text`; o teste `edit.range && typeof edit.text ===
//!   'string'` do original é, aqui, sempre verdadeiro. Uma edição só de fim de linha (`eol` sem intervalo no original)
//!   é uma edição com intervalo vazio e texto vazio, que não muda nada no modelo.
//! - `editor.hasModel()` mais `editor.getModel()` é o `Option` de [`ICodeEditor::get_model`].
//! - `model.pushEOL(newEol)` só roda quando o editor tem modelo, como no original.

use je_base::JsString;

use crate::code_action::languages::TextEdit;
use crate::code_editor_widget::{CodeEditorWidget, ExecuteEditsSource, ICodeEditor};
use crate::edit_operation::{EditOperation, SingleEditOperation};
use crate::model::EndOfLineSequence;
use crate::range::Range;
use crate::stable_editor_scroll::StableEditorScrollState;

/// A origem das edições que o `FormattingEdit` entrega a `executeEdits`.
const FORMAT_EDITS_SOURCE: &str = "formatEditsCommand";

/// `FormattingEdit`.
pub struct FormattingEdit;

impl FormattingEdit {
    /// `_handleEolEdits(editor, edits)`.
    fn handle_eol_edits(editor: &CodeEditorWidget, edits: &[TextEdit]) -> Vec<TextEdit> {
        let mut new_eol: Option<EndOfLineSequence> = None;
        let mut single_edits: Vec<TextEdit> = Vec::new();

        for edit in edits {
            if let Some(eol) = edit.eol {
                new_eol = Some(eol);
            }
            single_edits.push(edit.clone());
        }

        if let Some(new_eol) = new_eol
            && let Some(model) = editor.get_model()
        {
            model.push_eol(new_eol);
        }

        single_edits
    }

    /// `_isFullModelReplaceEdit(editor, edit)`.
    fn is_full_model_replace_edit(editor: &CodeEditorWidget, edit: &TextEdit) -> bool {
        let Some(model) = editor.get_model() else {
            return false;
        };
        let edit_range = model.validate_range(&edit.range);
        let full_model_range = model.get_full_model_range();
        full_model_range == edit_range
    }

    /// `execute(editor, edits, addUndoStops)`.
    pub fn execute(editor: &CodeEditorWidget, edits: &[TextEdit], add_undo_stops: bool) {
        if add_undo_stops {
            editor.push_undo_stop();
        }
        let scroll_state = StableEditorScrollState::capture(editor);
        let edits = Self::handle_eol_edits(editor, edits);
        // We use replace semantics and hope that markers stay put...
        let make_operation: fn(Range, Option<JsString>) -> SingleEditOperation =
            if edits.len() == 1 && Self::is_full_model_replace_edit(editor, &edits[0]) { EditOperation::replace } else { EditOperation::replace_move };
        let operations: Vec<SingleEditOperation> =
            edits.iter().map(|edit| make_operation(edit.range, Some(JsString::from(edit.text.as_str())))).collect();
        editor.execute_edits(ExecuteEditsSource::Name(Some(FORMAT_EDITS_SOURCE)), operations, None);
        if add_undo_stops {
            editor.push_undo_stop();
        }
        scroll_state.restore_relative_vertical_position_of_cursor(editor);
    }
}
