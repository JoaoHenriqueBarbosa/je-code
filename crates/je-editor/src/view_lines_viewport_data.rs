//! Porte de `src/vs/editor/common/viewLayout/viewLinesViewportData.ts`: `ViewportData`, os dados necessários
//! para desenhar um viewport.
//!
//! Divergências de tradução:
//! - `IViewModel` tem tipos associados (`EventHandler`, `EditorTheme`, `GlyphLanes`), então não é objeto de
//!   trait: `ViewportData` é genérica no modelo `M` e o guarda num `Rc<M>`, como o `_model` compartilhado do
//!   original.
//! - Os `| 0` do construtor sobre `startLineNumber`, `endLineNumber` e `bigNumbersDelta`/`lineHeight`: os
//!   números de linha já são `i32` (a identidade); `bigNumbersDelta` e `lineHeight` são `f64` e passam por
//!   `to_int32`, de `view_model_types`.
//! - `partialData` é consumido (o `relativeVerticalOffset` muda de dono em vez de ser compartilhado).

use std::rc::Rc;

use crate::range::Range;
use crate::selection::Selection;
use crate::view_model_decoration::ViewModelDecoration;
use crate::view_model_types::{
    IPartialViewLinesViewportData, IViewModel, IViewWhitespaceViewportData, ViewLineRenderingData, to_int32,
};

/// Contém todos os dados necessários para desenhar um viewport específico.
pub struct ViewportData<M: IViewModel> {
    pub selections: Vec<Selection>,

    /// O número da linha em que o desenho começa (inclusive).
    pub start_line_number: i32,

    /// O número da linha em que o desenho termina (inclusive).
    pub end_line_number: i32,

    /// `relative_vertical_offset[i]` é o `top` da linha `i + start_line_number`.
    pub relative_vertical_offset: Vec<f64>,

    /// O viewport como intervalo: (`start_line_number`, 1) até (`end_line_number`, maxColumn(`end_line_number`)).
    pub visible_range: Range,

    /// Valor a subtrair de `scrollTop` (para os offsets verticais ficarem abaixo de 1MM).
    pub big_numbers_delta: f64,

    /// Informação de posição dos espaços (whitespaces) entre linhas.
    pub whitespace_viewport_data: Vec<IViewWhitespaceViewportData>,

    model: Rc<M>,

    pub line_height: f64,
}

impl<M: IViewModel> ViewportData<M> {
    pub fn new(
        selections: Vec<Selection>,
        partial_data: IPartialViewLinesViewportData,
        whitespace_viewport_data: Vec<IViewWhitespaceViewportData>,
        model: Rc<M>,
    ) -> Self {
        let visible_range = Range::new(
            partial_data.start_line_number,
            model.get_line_min_column(partial_data.start_line_number),
            partial_data.end_line_number,
            model.get_line_max_column(partial_data.end_line_number),
        );
        Self {
            selections,
            start_line_number: partial_data.start_line_number,
            end_line_number: partial_data.end_line_number,
            relative_vertical_offset: partial_data.relative_vertical_offset,
            visible_range,
            big_numbers_delta: to_int32(partial_data.big_numbers_delta),
            whitespace_viewport_data,
            model,
            line_height: to_int32(partial_data.line_height),
        }
    }

    pub fn get_view_line_rendering_data(&self, line_number: i32) -> ViewLineRenderingData {
        self.model.get_viewport_view_line_rendering_data(self.visible_range, line_number)
    }

    pub fn get_view_line_continues_with_wrapped_line(&self, line_number: i32) -> bool {
        self.model.get_view_line_continues_with_wrapped_line(line_number)
    }

    pub fn get_decorations_in_viewport(&self) -> Vec<ViewModelDecoration> {
        self.model.get_decorations_in_viewport(self.visible_range)
    }
}
