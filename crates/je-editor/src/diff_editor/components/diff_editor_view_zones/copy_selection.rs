//! Porte de `src/vs/editor/browser/widget/diffEditor/components/diffEditorViewZones/copySelection.ts`: o `copy` numa zona de visão com o
//! código apagado copia o texto do modelo original (e não o texto que o DOM mostra).
//!
//! Divergências:
//! - **Evento vira método.** O `addDisposableListener(domNode, 'copy', ...)` não tem `domNode`: quem recebe o evento de cópia do pintor
//!   chama [`ViewZoneCopySelection::on_copy`], com a seleção já resolvida em [`DomSelectionRange`]. O `domNode` sai das opções.
//! - **Seleção do DOM.** `selection.getRangeAt(0)` com os `startContainer`/`endContainer` (o pai, quando o nó é de texto) e os
//!   deslocamentos são os dois [`ViewLineDomPoint`] que o pintor resolve; `None` num deles é o elemento ausente do original. A
//!   seleção ausente ou sem intervalo (`rangeCount === 0`) é `None` em vez de [`DomSelectionRange`].
//! - **`DisposableStore`.** O store que a função devolve só guardava o ouvinte; o objeto devolvido é o [`ViewZoneCopySelection`], que ao
//!   ser descartado deixa de tratar a cópia (o ouvinte removido).
//! - **`e.preventDefault()`.** [`ViewZoneCopySelection::on_copy`] devolve se a cópia padrão deve ser impedida: sempre, enquanto o
//!   objeto não foi descartado.

use std::cell::Cell;
use std::rc::Rc;

use je_base::lifecycle::IDisposable;

use crate::diff::range_mapping::DetailedLineRangeMapping;
use crate::diff_editor::components::diff_editor_view_zones::render_lines::{RenderLinesResult, ViewLineDomPoint};
use crate::range::Range;
use crate::services::clipboard_service::IClipboardService;
use crate::text_model::TextModel;

/// `IEnableViewZoneCopySelectionOptions`.
pub struct EnableViewZoneCopySelectionOptions {
    /// The diff entry for the current view zone.
    pub diff_entry: DetailedLineRangeMapping,

    /// The original text model, to get the original text based on selection.
    pub original_model: Rc<TextModel>,

    /// The render lines result that can translate DOM positions to model positions.
    pub render_lines_result: Rc<RenderLinesResult>,

    /// The clipboard service to write the selected text to.
    pub clipboard_service: Rc<dyn IClipboardService>,
}

/// O intervalo da seleção do DOM (`selection.getRangeAt(0)`) numa zona de visão.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DomSelectionRange {
    /// `domRange.collapsed`.
    pub collapsed: bool,
    /// O início, resolvido a partir do `startContainer` e do `startOffset`; `None` sem elemento.
    pub start: Option<ViewLineDomPoint>,
    /// O fim, resolvido a partir do `endContainer` e do `endOffset`; `None` sem elemento.
    pub end: Option<ViewLineDomPoint>,
}

/// O que `enableCopySelection` devolve no lugar do `DisposableStore`: trata o `copy` até ser descartado.
pub struct ViewZoneCopySelection {
    options: EnableViewZoneCopySelectionOptions,
    disposed: Cell<bool>,
}

/// `enableCopySelection(options)`.
pub fn enable_copy_selection(options: EnableViewZoneCopySelectionOptions) -> Rc<ViewZoneCopySelection> {
    Rc::new(ViewZoneCopySelection { options, disposed: Cell::new(false) })
}

impl ViewZoneCopySelection {
    /// O ouvinte do `copy`. `selection` é `None` sem seleção ou sem intervalo. Devolve se o `preventDefault()` foi chamado.
    pub fn on_copy(&self, selection: Option<&DomSelectionRange>) -> bool {
        if self.disposed.get() {
            return false;
        }
        let Some(dom_range) = selection else { return true };
        if dom_range.collapsed {
            return true;
        }
        let (Some(start), Some(end)) = (&dom_range.start, &dom_range.end) else { return true };

        let EnableViewZoneCopySelectionOptions { diff_entry, original_model, render_lines_result, clipboard_service } = &self.options;
        let (Some(start_position), Some(end_position)) =
            (render_lines_result.get_model_position_at(start), render_lines_result.get_model_position_at(end))
        else {
            return true;
        };

        let line_delta = diff_entry.original.start_line_number - 1;
        let adjusted_start = start_position.delta(line_delta, 0);
        let adjusted_end = end_position.delta(line_delta, 0);

        let range = if adjusted_end.is_before(&adjusted_start) {
            Range::from_positions(adjusted_end, adjusted_start)
        } else {
            Range::from_positions(adjusted_start, adjusted_end)
        };

        let selected_text = original_model.get_value_in_range(&range);
        // O original não espera a promessa da área de transferência.
        drop(clipboard_service.write_text(&selected_text.to_string_lossy()));
        true
    }
}

impl IDisposable for ViewZoneCopySelection {
    fn dispose(&self) {
        self.disposed.set(true);
    }
}
