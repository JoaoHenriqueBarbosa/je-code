//! Porte de `InlineCompletionViewKind` e `InlineCompletionViewData` de
//! `src/vs/editor/contrib/inlineCompletions/browser/view/inlineEdits/inlineEditsViewInterface.ts`. O restante do
//! arquivo (`InlineEditTabAction`, `InlineEditClickEvent`, `IInlineEditsView`) é da camada de visão e entra com ela.

/// `InlineCompletionViewKind`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum InlineCompletionViewKind {
    GhostText,
    Custom,
    SideBySide,
    Deletion,
    InsertionInline,
    InsertionMultiLine,
    WordReplacements,
    LineReplacement,
    Collapsed,
    JumpTo,
}

impl InlineCompletionViewKind {
    /// O valor textual do membro (`'ghostText'`, `'custom'`, ...).
    pub fn as_str(self) -> &'static str {
        match self {
            Self::GhostText => "ghostText",
            Self::Custom => "custom",
            Self::SideBySide => "sideBySide",
            Self::Deletion => "deletion",
            Self::InsertionInline => "insertionInline",
            Self::InsertionMultiLine => "insertionMultiLine",
            Self::WordReplacements => "wordReplacements",
            Self::LineReplacement => "lineReplacement",
            Self::Collapsed => "collapsed",
            Self::JumpTo => "jumpTo",
        }
    }
}

/// `InlineCompletionViewData`.
#[derive(Clone, Debug, PartialEq)]
pub struct InlineCompletionViewData {
    pub long_distance_hint_visible: Option<bool>,
    pub long_distance_hint_distance: Option<i32>,
    /// Se a sugestão mira outro modelo de texto (URI) que o do editor ativo, isto é, uma Next Edit Suggestion entre
    /// documentos. `None` para completações em linha e ghost text.
    pub is_for_another_document: Option<bool>,
    pub cursor_column_distance: i32,
    pub cursor_line_distance: i32,
    pub line_count_original: i32,
    pub line_count_modified: i32,
    pub character_count_original: i32,
    pub character_count_modified: i32,
    pub disjoint_replacements: i32,
    pub same_shape_replacements: Option<bool>,
}

impl InlineCompletionViewData {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        cursor_column_distance: i32,
        cursor_line_distance: i32,
        line_count_original: i32,
        line_count_modified: i32,
        character_count_original: i32,
        character_count_modified: i32,
        disjoint_replacements: i32,
        same_shape_replacements: Option<bool>,
    ) -> Self {
        Self {
            long_distance_hint_visible: None,
            long_distance_hint_distance: None,
            is_for_another_document: None,
            cursor_column_distance,
            cursor_line_distance,
            line_count_original,
            line_count_modified,
            character_count_original,
            character_count_modified,
            disjoint_replacements,
            same_shape_replacements,
        }
    }

    /// `setLongDistanceViewData(lineNumber, inlineEditLineNumber)`.
    pub fn set_long_distance_view_data(&mut self, line_number: i32, inline_edit_line_number: i32) {
        self.long_distance_hint_visible = Some(true);
        self.long_distance_hint_distance = Some((inline_edit_line_number - line_number).abs());
    }

    /// `getData()`: o próprio valor, que já é o registro com todos os campos.
    pub fn get_data(&self) -> InlineCompletionViewData {
        self.clone()
    }
}
