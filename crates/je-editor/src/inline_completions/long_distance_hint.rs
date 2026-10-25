//! Porte parcial de `src/vs/editor/contrib/inlineCompletions/browser/view/inlineEdits/inlineEditsViews/longDistanceHint/inlineEditsLongDistanceHint.ts`.
//!
//! Entram aqui a função `scrollToReveal` e as duas interfaces que a `InlineEditsView` troca com a view
//! ([`LongDistanceHint`], a `ILongDistanceHint`, e [`LongDistanceViewState`], a `ILongDistanceViewState`). O resto do
//! arquivo (a view `InlineEditsLongDistanceHint` e os helpers de DOM) depende do editor observável, do DOM de widgets e
//! do contexto de renderização, que ainda não existem no crate. Os números são `i32`, como o `OffsetRange` do crate.
//!
//! No `ILongDistanceViewState`, o `model` e o `inlineSuggestInfo` (que a `InlineEditsView` lê com `!`) chegam em `Rc`: o
//! estado só existe quando os dois existem.

use std::rc::Rc;

use crate::diff::range_mapping::DetailedLineRangeMapping;
use crate::inline_completions::provide_inline_completions::InlineCompletionEditorType;
use crate::inline_completions::text_model_value_reference::TextModelValueReference;
use crate::inline_completions::view::inline_edits::components::gutter_indicator_view::{InlineSuggestionGutterMenuData, SimpleInlineSuggestModel};
use crate::inline_completions::view::inline_edits::inline_edit_with_changes::InlineEditWithChanges;
use crate::offset_range::OffsetRange;
use crate::position::Position;

/// `ILongDistanceHint`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LongDistanceHint {
    pub line_number: i32,
    pub is_visible: bool,
}

/// `ILongDistanceViewState`.
#[derive(Clone)]
pub struct LongDistanceViewState {
    pub hint: LongDistanceHint,
    pub new_text_line_count: i32,
    pub edit: Rc<InlineEditWithChanges>,
    pub diff: Vec<DetailedLineRangeMapping>,
    pub next_cursor_position: Option<Position>,
    pub editor_type: InlineCompletionEditorType,
    pub target: Rc<TextModelValueReference>,

    pub model: Rc<SimpleInlineSuggestModel>,
    pub inline_suggest_info: Rc<InlineSuggestionGutterMenuData>,
}

/// O resultado de `scrollToReveal`: o original devolve `{ newScrollPosition: number }`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ScrollToReveal {
    pub new_scroll_position: i32,
}

/// Muda a posição de rolagem o mínimo possível, só para revelar o intervalo dado na janela.
pub fn scroll_to_reveal(current_scroll_position: i32, window_width: i32, content_range_to_reveal: OffsetRange) -> ScrollToReveal {
    let visible_range = OffsetRange::new(current_scroll_position, current_scroll_position + window_width);
    if visible_range.contains_range(content_range_to_reveal) {
        return ScrollToReveal { new_scroll_position: current_scroll_position };
    }
    if content_range_to_reveal.length() > window_width {
        return ScrollToReveal { new_scroll_position: content_range_to_reveal.start };
    }
    if content_range_to_reveal.end_exclusive > visible_range.end_exclusive {
        return ScrollToReveal { new_scroll_position: content_range_to_reveal.end_exclusive - window_width };
    }
    if content_range_to_reveal.start < visible_range.start {
        return ScrollToReveal { new_scroll_position: content_range_to_reveal.start };
    }
    ScrollToReveal { new_scroll_position: current_scroll_position }
}
