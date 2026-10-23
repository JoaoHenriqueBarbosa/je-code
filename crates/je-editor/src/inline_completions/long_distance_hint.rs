//! Porte parcial de `src/vs/editor/contrib/inlineCompletions/browser/view/inlineEdits/inlineEditsViews/longDistanceHint/inlineEditsLongDistanceHint.ts`.
//!
//! Só a função `scrollToReveal` entra aqui. O resto do arquivo (a view `InlineEditsLongDistanceHint` e os
//! helpers de DOM) depende do editor observável, do DOM de widgets e do contexto de renderização, que ainda
//! não existem no crate. Os números são `i32`, como o `OffsetRange` do crate.

use crate::offset_range::OffsetRange;

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
