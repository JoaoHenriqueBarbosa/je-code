//! Porte de `TextEditorPaneSelection` de `src/vs/workbench/browser/parts/editor/textEditor.ts`: a seleção de um editor de texto
//! (a posição do cursor), que a navegação pelo histórico compara e restaura.
//!
//! Fora deste módulo: `AbstractTextEditor` e o resto de `textEditor.ts` (o painel de editor de texto em si, que depende do
//! `CodeEditorWidget`).
//!
//! Divergências:
//! - `ISelection` é o [`Selection`] (que já é a seleção lógica, então `Selection.liftSelection` do chamador é a identidade).
//! - `restore(options)` devolve as [`TextEditorOptions`] com a seleção como as do original (`{ ...options, selection,
//!   selectionRevealType: CenterIfOutsideViewport }`): o `ITextEditorSelection` é a faixa da seleção (início e fim da
//!   [`Range`] que ela estende).
//!
//! [`Range`]: je_editor::range::Range

use je_editor::selection::Selection;
use je_platform::opener::editor_types::TextEditorSelection;

use crate::common::editor::{EditorPaneSelectionCompareResult, IEditorPaneSelection, TextEditorOptions, TextEditorSelectionRevealType};

/// `TextEditorPaneSelection`.
pub struct TextEditorPaneSelection {
    text_selection: Selection,
}

impl TextEditorPaneSelection {
    /// number of lines to move in editor to justify for significant change
    const TEXT_EDITOR_SELECTION_THRESHOLD: i32 = 10;

    pub fn new(text_selection: Selection) -> Self {
        Self { text_selection }
    }

    /// `textSelection`.
    pub fn text_selection(&self) -> Selection {
        self.text_selection
    }
}

impl IEditorPaneSelection for TextEditorPaneSelection {
    fn compare(&self, other: &dyn IEditorPaneSelection) -> EditorPaneSelectionCompareResult {
        let Some(other) = other.as_any().downcast_ref::<TextEditorPaneSelection>() else {
            return EditorPaneSelectionCompareResult::Different;
        };

        let this_line_number = self.text_selection.selection_start_line_number.min(self.text_selection.position_line_number);
        let other_line_number = other.text_selection.selection_start_line_number.min(other.text_selection.position_line_number);

        if this_line_number == other_line_number {
            return EditorPaneSelectionCompareResult::Identical;
        }

        if (this_line_number - other_line_number).abs() < Self::TEXT_EDITOR_SELECTION_THRESHOLD {
            return EditorPaneSelectionCompareResult::Similar; // when in close proximity, treat selection as being similar
        }

        EditorPaneSelectionCompareResult::Different
    }

    fn restore(&self, options: &TextEditorOptions) -> TextEditorOptions {
        TextEditorOptions {
            selection: Some(TextEditorSelection {
                start_line_number: f64::from(self.text_selection.start_line_number),
                start_column: f64::from(self.text_selection.start_column),
                end_line_number: Some(f64::from(self.text_selection.end_line_number)),
                end_column: Some(f64::from(self.text_selection.end_column)),
            }),
            selection_reveal_type: Some(TextEditorSelectionRevealType::CenterIfOutsideViewport),
            ..options.clone()
        }
    }

    fn log(&self) -> Option<String> {
        Some(format!(
            "line: {}-{}, col:  {}-{}",
            self.text_selection.start_line_number, self.text_selection.end_line_number, self.text_selection.start_column, self.text_selection.end_column
        ))
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}
