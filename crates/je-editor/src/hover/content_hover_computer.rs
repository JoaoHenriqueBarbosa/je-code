//! Porte de `src/vs/editor/contrib/hover/browser/contentHoverComputer.ts`: o [`ContentHoverComputer`], que pergunta a cada
//! participante do hover o que mostrar para um ancorador (`IHoverComputer<ContentHoverComputerOptions, IHoverPart>`).
//!
//! Notas de tradução:
//! - `computeAsync` devolve o `AsyncIterableProducer.merge(...)` dos participantes como `Rc<dyn AsyncIterable<...>>`; um
//!   participante sem `computeAsync` (`None`) contribui com o `AsyncIterableProducer.EMPTY`.
//! - `coalesce(result)` tira os `null`/`undefined` da lista de partes; um `Rc<dyn IHoverPart>` nunca é nulo, então a
//!   lista já sai limpa.
//! - `anchor.type !== HoverAnchorType.Range && !anchor.supportsMarkerHover`: o ancorador de intervalo sempre passa, o de
//!   elemento estrangeiro só se `supportsMarkerHover` for verdadeiro (`undefined` é falso).
//! - `editor.hasModel()` é o `has_model` do editor; `IActiveCodeEditor` (com modelo) é o `get_model` que devolve `Some`.

use std::rc::Rc;

use je_base::async_::{AsyncIterable, AsyncIterableProducer};
use je_base::cancellation::CancellationToken;

use crate::code_editor_widget::{CodeEditorWidget, ICodeEditor};
use crate::cursor_common::CursorSimpleModel;
use crate::hover::hover_operation::{HoverStartSource, IHoverComputer};
use crate::hover::hover_types::{HoverAnchor, IEditorHoverParticipant, IHoverPart};
use crate::model::ModelDecoration;

/// `ContentHoverComputerOptions`.
#[derive(Clone)]
pub struct ContentHoverComputerOptions {
    pub should_focus: bool,
    pub anchor: HoverAnchor,
    pub source: HoverStartSource,
    pub insist_on_keeping_hover_visible: bool,
}

/// `ContentHoverComputer`.
pub struct ContentHoverComputer {
    editor: Rc<CodeEditorWidget>,
    participants: Vec<Rc<dyn IEditorHoverParticipant>>,
}

impl ContentHoverComputer {
    pub fn new(editor: Rc<CodeEditorWidget>, participants: Vec<Rc<dyn IEditorHoverParticipant>>) -> Self {
        Self { editor, participants }
    }

    /// `_getLineDecorations(editor, anchor)`.
    fn get_line_decorations(editor: &CodeEditorWidget, anchor: &HoverAnchor) -> Vec<ModelDecoration> {
        let supports_marker_hover = match anchor {
            HoverAnchor::Range(_) => true,
            HoverAnchor::ForeignElement(anchor) => anchor.supports_marker_hover.unwrap_or(false),
        };
        if !supports_marker_hover {
            return Vec::new();
        }

        let Some(model) = editor.get_model() else {
            return Vec::new();
        };
        let anchor_range = anchor.range();
        let line_number = anchor_range.start_line_number;

        if line_number > CursorSimpleModel::get_line_count(&*model) {
            // invalid line
            return Vec::new();
        }

        let max_column = CursorSimpleModel::get_line_max_column(&*model, line_number);

        editor
            .get_line_decorations(line_number)
            .unwrap_or_default()
            .into_iter()
            .filter(|decoration| {
                if decoration.options.is_whole_line {
                    return true;
                }

                let start_column = if decoration.range.start_line_number == line_number { decoration.range.start_column } else { 1 };
                let end_column = if decoration.range.end_line_number == line_number { decoration.range.end_column } else { max_column };

                if decoration.options.show_if_collapsed {
                    // Relax check around `showIfCollapsed` decorations to also include +/- 1 character
                    if start_column > anchor_range.start_column + 1 || anchor_range.end_column - 1 > end_column {
                        return false;
                    }
                } else if start_column > anchor_range.start_column || anchor_range.end_column > end_column {
                    return false;
                }

                true
            })
            .collect()
    }
}

impl IHoverComputer<ContentHoverComputerOptions, Rc<dyn IHoverPart>> for ContentHoverComputer {
    fn compute_async(&self, options: &ContentHoverComputerOptions, token: &CancellationToken) -> Option<Rc<dyn AsyncIterable<Rc<dyn IHoverPart>>>> {
        let anchor = &options.anchor;

        if !self.editor.has_model() {
            let empty: Rc<dyn AsyncIterable<Rc<dyn IHoverPart>>> = AsyncIterableProducer::<Rc<dyn IHoverPart>>::empty();
            return Some(empty);
        }

        let line_decorations = Self::get_line_decorations(&self.editor, anchor);

        let iterables: Vec<Rc<dyn AsyncIterable<Rc<dyn IHoverPart>>>> = self
            .participants
            .iter()
            .map(|participant| {
                participant.compute_async(anchor, &line_decorations, options.source, token).unwrap_or_else(|| {
                    let empty: Rc<dyn AsyncIterable<Rc<dyn IHoverPart>>> = AsyncIterableProducer::<Rc<dyn IHoverPart>>::empty();
                    empty
                })
            })
            .collect();
        let merged: Rc<dyn AsyncIterable<Rc<dyn IHoverPart>>> = AsyncIterableProducer::merge(iterables);
        Some(merged)
    }

    fn compute_sync(&self, options: &ContentHoverComputerOptions) -> Option<Vec<Rc<dyn IHoverPart>>> {
        if !self.editor.has_model() {
            return Some(Vec::new());
        }

        let anchor = &options.anchor;
        let line_decorations = Self::get_line_decorations(&self.editor, anchor);

        let mut result: Vec<Rc<dyn IHoverPart>> = Vec::new();
        for participant in &self.participants {
            result.extend(participant.compute_sync(anchor, &line_decorations, options.source));
        }

        Some(result)
    }
}
