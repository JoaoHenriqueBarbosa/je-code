//! Porte de `.../inlineEdits/inlineEditsViews/longDistanceHint/longDistnaceWidgetPlacement.ts`.
//!
//! Notas de tradução:
//! - Os números do JavaScript são `f64`, mas `OffsetRange` e `LineRange` do crate são `i32`. Na fronteira
//!   com `OffsetRange` o valor `f64` é arredondado (`to_offset`).
//! - O original, ao indexar `availableSpaceHeightPrefixSums` fora do intervalo, obtém `undefined` e o
//!   `NaN` resultante faz a torre ter altura 0, logo nenhum contorno. Aqui `get_widget_vertical_outline`
//!   devolve `None` nesse caso e `try_find_widget_outline` o trata como "sem contorno".
//! - `splitIntoContinuousLineRanges` recebia o `ObservableCodeEditor` e um `IReader`. A leitura de
//!   `editor.getTopForLineNumber` vira o parâmetro `top_for_line_number`; a observação do topo da última
//!   linha (`derived` + `observeTopForLineNumber`, só para registrar dependência no leitor) não tem
//!   equivalente sem o editor observável e fica de fora.

use crate::core_2d::size::Size2D;
use crate::inline_completions::towers_layout::get_max_tower_height_in_available_area;
use crate::line_range::LineRange;
use crate::offset_range::OffsetRange;

/// `WidgetLayoutConstants`: constantes de layout do widget de dica de longa distância.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WidgetLayoutConstants {
    pub preview_editor_margin: f64,
    pub widget_padding: f64,
    pub widget_border: f64,
    pub lower_bar_height: f64,
    pub min_widget_width: f64,
}

/// `WidgetOutline`: o contorno do widget, com as faixas horizontal e vertical.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WidgetOutline {
    pub horizontal_widget_range: OffsetRange,
    pub vertical_widget_range: OffsetRange,
}

/// `ContinuousLineSizes`: uma faixa contínua de linhas com seus tamanhos e posição.
#[derive(Clone, Debug)]
pub struct ContinuousLineSizes {
    pub line_range: LineRange,
    pub top: f64,
    pub sizes: Vec<Size2D>,
}

fn to_offset(value: f64) -> i32 {
    value.round() as i32
}

/// `WidgetPlacementContext`: contexto para calcular o posicionamento dentro de uma faixa contínua de linhas.
#[derive(Clone, Debug)]
pub struct WidgetPlacementContext {
    pub available_space_sizes: Vec<Size2D>,
    pub available_space_height_prefix_sums: Vec<f64>,
    pub available_space_sizes_transposed: Vec<Size2D>,
    line_range_info: ContinuousLineSizes,
}

impl WidgetPlacementContext {
    pub fn new(line_range_info: ContinuousLineSizes, editor_true_content_width: f64, end_of_line_padding: impl Fn(i32) -> f64) -> Self {
        let available_space_sizes: Vec<Size2D> = line_range_info
            .sizes
            .iter()
            .enumerate()
            .map(|(idx, s)| {
                let line_number = line_range_info.line_range.start_line_number + idx as i32;
                let line_padding_left = end_of_line_padding(line_number);
                Size2D::new((editor_true_content_width - s.width - line_padding_left).max(0.0), s.height)
            })
            .collect();

        let available_space_height_prefix_sums = get_sums(&available_space_sizes, |s| s.height);
        let available_space_sizes_transposed = available_space_sizes.iter().map(Size2D::transpose).collect();

        Self { available_space_sizes, available_space_height_prefix_sums, available_space_sizes_transposed, line_range_info }
    }

    /// Calcula o contorno vertical de um widget colocado na linha dada. `None` quando a linha está fora da
    /// faixa (no original o resultado seria `NaN`).
    pub fn get_widget_vertical_outline(&self, line_number: i32, preview_editor_height: f64, layout_constants: WidgetLayoutConstants) -> Option<OffsetRange> {
        let size_idx = line_number - self.line_range_info.line_range.start_line_number;
        let prefix_sum = *usize::try_from(size_idx).ok().and_then(|i| self.available_space_height_prefix_sums.get(i))?;
        let top = self.line_range_info.top + prefix_sum;
        let editor_range = OffsetRange::of_start_and_length(to_offset(top), to_offset(preview_editor_height));
        let WidgetLayoutConstants { preview_editor_margin, widget_padding, widget_border, lower_bar_height, .. } = layout_constants;
        let vertical_widget_range = editor_range
            .with_margin(to_offset(preview_editor_margin + widget_padding + widget_border))
            .with_margin_start_end(0, to_offset(lower_bar_height));
        Some(vertical_widget_range)
    }

    /// Tenta achar um contorno de widget válido dentro deste contexto de faixa de linhas.
    pub fn try_find_widget_outline(
        &self,
        target_line_number: i32,
        preview_editor_height: f64,
        editor_true_content_right: f64,
        layout_constants: WidgetLayoutConstants,
    ) -> Option<WidgetOutline> {
        if self.line_range_info.line_range.length() < 3 {
            return None;
        }
        find_first_minimze_distance(self.line_range_info.line_range.add_margin(-1, -1), target_line_number, |line_number| {
            let vertical_widget_range = self.get_widget_vertical_outline(line_number, preview_editor_height, layout_constants)?;
            let max_width = get_max_tower_height_in_available_area(
                vertical_widget_range.delta(-to_offset(self.line_range_info.top)),
                &self.available_space_sizes_transposed,
            );
            if max_width < layout_constants.min_widget_width {
                return None;
            }
            let horizontal_widget_range = OffsetRange::new(to_offset(editor_true_content_right - max_width), to_offset(editor_true_content_right));
            Some(WidgetOutline { horizontal_widget_range, vertical_widget_range })
        })
    }
}

/// Divide as informações de tamanho de linha em faixas contínuas, quebrando onde o topo esperado difere do
/// real (por exemplo, por regiões dobradas). `top_for_line_number` é o `editor.getTopForLineNumber`.
pub fn split_into_continuous_line_ranges(
    line_range: LineRange,
    sizes: &[Size2D],
    top: f64,
    top_for_line_number: impl Fn(i32) -> f64,
) -> Vec<ContinuousLineSizes> {
    let mut result: Vec<ContinuousLineSizes> = Vec::new();
    let mut current_range_start = line_range.start_line_number;
    let mut current_range_top = top;
    let mut current_sizes: Vec<Size2D> = Vec::new();

    for (i, size) in sizes.iter().enumerate() {
        let line_number = line_range.start_line_number + i as i32;
        let expected_top = current_range_top + current_sizes.iter().fold(0.0, |p, c| p + c.height);
        let actual_top = top_for_line_number(line_number);

        if i > 0 && actual_top != expected_top {
            // Descontinuidade: fecha a faixa atual e começa outra.
            result.push(ContinuousLineSizes {
                line_range: LineRange::of_length(current_range_start, line_number - current_range_start),
                top: current_range_top,
                sizes: std::mem::take(&mut current_sizes),
            });
            current_range_start = line_number;
            current_range_top = actual_top;
        }
        current_sizes.push(*size);
    }

    // Fecha a última faixa.
    result.push(ContinuousLineSizes {
        line_range: LineRange::of_length(current_range_start, line_range.end_line_number_exclusive - current_range_start),
        top: current_range_top,
        sizes: current_sizes,
    });

    result
}

fn find_first_minimze_distance<T>(range: LineRange, target_line: i32, mut predicate: impl FnMut(i32) -> Option<T>) -> Option<T> {
    let mut offset = 0;
    loop {
        let down = target_line + offset;
        if down <= range.end_line_number_exclusive
            && let Some(result) = predicate(down)
        {
            return Some(result);
        }
        let up = target_line - offset;
        if up >= range.start_line_number
            && let Some(result) = predicate(up)
        {
            return Some(result);
        }
        if up < range.start_line_number && down > range.end_line_number_exclusive {
            return None;
        }
        offset += 1;
    }
}

fn get_sums<T>(array: &[T], f: impl Fn(&T) -> f64) -> Vec<f64> {
    let mut result = vec![0.0];
    let mut sum = 0.0;
    for item in array {
        sum += f(item);
        result.push(sum);
    }
    result
}
