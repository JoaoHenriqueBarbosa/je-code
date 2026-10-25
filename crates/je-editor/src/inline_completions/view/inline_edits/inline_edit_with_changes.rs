//! Porte de `src/vs/editor/contrib/inlineCompletions/browser/view/inlineEdits/inlineEditWithChanges.ts`:
//! `InlineEditWithChanges`, a edição em linha com os intervalos de linhas que ela cobre.
//!
//! Notas de tradução:
//! - `ITextModel`-referência é o `Rc<TextModelValueReference>`; os `readonly Position[]` e
//!   `readonly InlineCompletionCommand[]` são `Vec`;
//! - os getters `lineEdit`, `originalLineRange`, `modifiedLineRange` e `displayRange` são métodos. O `this.edit!`
//!   (asserção de não nulo do original) é um `expect`: a ação `edit` sempre vem com a edição de texto.

use std::rc::Rc;

use crate::edits::line_edit::LineReplacement;
use crate::edits::text_edit::TextEdit;
use crate::inline_completions::inline_suggestion_item::{InlineSuggestionAction, InlineSuggestionItem};
use crate::inline_completions::text_model_value_reference::TextModelValueReference;
use crate::languages::InlineCompletionCommand;
use crate::line_range::LineRange;
use crate::position::Position;
use crate::text::abstract_text::AbstractText;

/// `InlineEditWithChanges`.
pub struct InlineEditWithChanges {
    pub original_text: Rc<TextModelValueReference>,
    pub action: Option<InlineSuggestionAction>,
    pub edit: Option<TextEdit>,
    pub cursor_position: Position,
    pub multi_cursor_positions: Vec<Position>,
    pub commands: Vec<InlineCompletionCommand>,
    pub inline_completion: InlineSuggestionItem,
}

impl InlineEditWithChanges {
    pub fn new(
        original_text: Rc<TextModelValueReference>,
        action: Option<InlineSuggestionAction>,
        edit: Option<TextEdit>,
        cursor_position: Position,
        multi_cursor_positions: Vec<Position>,
        commands: Vec<InlineCompletionCommand>,
        inline_completion: InlineSuggestionItem,
    ) -> Self {
        Self { original_text, action, edit, cursor_position, multi_cursor_positions, commands, inline_completion }
    }

    /// `lineEdit`. O original anota que estes campos deveriam migrar para a ação.
    pub fn line_edit(&self) -> LineReplacement {
        match &self.action {
            Some(InlineSuggestionAction::JumpTo(jump_to)) => LineReplacement::new(LineRange::of_length(jump_to.position.line_number, 0), Vec::new()),
            Some(InlineSuggestionAction::Edit(_)) => {
                let edit = self.edit.as_ref().expect("a ação edit vem com a edição de texto");
                LineReplacement::from_single_text_edit(&edit.to_replacement(&*self.original_text), &*self.original_text)
            }
            None => LineReplacement::new(LineRange::new(1, 1), Vec::new()),
        }
    }

    /// `originalLineRange`.
    pub fn original_line_range(&self) -> LineRange {
        self.line_edit().line_range
    }

    /// `modifiedLineRange`.
    pub fn modified_line_range(&self) -> LineRange {
        self.line_edit().to_line_edit().get_new_line_ranges()[0]
    }

    /// `displayRange`.
    pub fn display_range(&self) -> LineRange {
        let line_edit = self.line_edit();
        let original_line_range = line_edit.line_range;
        let modified_extent = LineRange::of_length(original_line_range.start_line_number, line_edit.new_lines.len() as i32);
        self.original_text
            .line_range()
            .intersect(&original_line_range.join(&modified_extent))
            .expect("o intervalo exibido cruza o texto original")
    }
}
