//! Porte de `src/vs/editor/contrib/inlineCompletions/browser/view/inlineEdits/inlineEditsViews/`: as views do inline
//! edits (uma por forma de apresentar a edição).
//!
//! Esta fatia (4c-2, parte 1) traz `inlineEditsWordInsertView` ([`inline_edits_word_insert_view`]),
//! `inlineEditsCollapsedView` ([`inline_edits_collapsed_view`]) e `inlineEditsDeletionView`
//! ([`inline_edits_deletion_view`]). As demais views entram nas fatias seguintes.
//!
//! Notas de tradução comuns:
//! - o `n.div({...}, [...]).keepUpdated(store)` do original monta um nó DOM que as views registram como widget de
//!   overlay. A UI é nativa: cada view expõe um observável com o dado já calculado (retângulos, cores, comandos de
//!   caminho) que o pintor nativo lê, e o `keepUpdated` vira `recompute_initially_and_on_change` sobre esse observável.
//!   As cores são identificadores do tema (`asCssVariable(id)` do original é só o `id`, que o pintor resolve);
//! - o widget de overlay que as três views registram (`domNode`, posição, `minContentWidthInPx` e
//!   `allowEditorOverflow: false`) é o [`NonOverflowOverlayWidget`]: o `domNode` não existe, e quem pinta lê o dado
//!   da view.

use std::rc::Rc;

use je_base::observable::IObservable;

use crate::editor_browser::OverlayWidgetPosition;
use crate::observable_code_editor::IObservableOverlayWidget;

pub mod inline_edits_collapsed_view;
pub mod inline_edits_deletion_view;
pub mod inline_edits_word_insert_view;

/// O `IObservableOverlayWidget` com `allowEditorOverflow: false` que as views desta pasta registram com
/// `createOverlayWidget`.
pub struct NonOverflowOverlayWidget {
    position: IObservable<Option<OverlayWidgetPosition>>,
    min_content_width_in_px: IObservable<f64>,
}

impl NonOverflowOverlayWidget {
    pub fn new(position: IObservable<Option<OverlayWidgetPosition>>, min_content_width_in_px: IObservable<f64>) -> Rc<Self> {
        Rc::new(Self { position, min_content_width_in_px })
    }
}

impl IObservableOverlayWidget for NonOverflowOverlayWidget {
    fn position(&self) -> IObservable<Option<OverlayWidgetPosition>> {
        Rc::clone(&self.position)
    }

    fn min_content_width_in_px(&self) -> IObservable<f64> {
        Rc::clone(&self.min_content_width_in_px)
    }

    fn allow_editor_overflow(&self) -> bool {
        false
    }
}
