//! Porte de `src/vs/editor/browser/view/renderingContext.ts`: o `RenderingContext` que cada parte da visão recebe
//! em `prepareRender`, e os tipos de posição horizontal que a medição das linhas devolve.
//!
//! Divergências:
//! - `ViewportData` é genérica no modelo de visão (ver `view_lines_viewport_data.rs`), então os contextos também
//!   são genéricos em `M: IViewModel` e guardam o `ViewportData<M>` por valor.
//! - `RenderingContext extends RestrictedRenderingContext` vira composição com `Deref`: o contexto completo
//!   guarda o restrito e herda os campos e métodos dele.
//! - `IViewLines` é um trait, guardado em `Rc<dyn IViewLines>` (o contexto compartilha as linhas com a visão).
//! - `Math.round` do JavaScript arredonda a metade para cima (`floor(x + 0.5)`), ao contrário de `f64::round`:
//!   `HorizontalRange` e `HorizontalPosition` usam [`js_round`].
//! - Os campos privados `_brand` (`void = undefined`) não existem: servem só à tipagem nominal do TypeScript.

use std::ops::Deref;
use std::rc::Rc;

use crate::position::Position;
use crate::range::Range;
use crate::view_lines_viewport_data::ViewportData;
use crate::view_model_decoration::ViewModelDecoration;
use crate::view_model_types::{IViewLayout, IViewModel};

/// `Math.round` do JavaScript.
fn js_round(value: f64) -> f64 {
    (value + 0.5).floor()
}

/// `IViewLines`.
pub trait IViewLines {
    fn lines_visible_ranges_for_range(&self, range: Range, include_new_lines: bool) -> Option<Vec<LineVisibleRanges>>;
    fn visible_range_for_position(&self, position: Position) -> Option<HorizontalPosition>;
}

/// `RestrictedRenderingContext`.
pub struct RestrictedRenderingContext<M: IViewModel> {
    pub viewport_data: ViewportData<M>,

    pub scroll_width: f64,
    pub scroll_height: f64,

    pub visible_range: Range,
    pub big_numbers_delta: f64,

    pub scroll_top: f64,
    pub scroll_left: f64,

    pub viewport_width: f64,
    pub viewport_height: f64,

    view_layout: Rc<dyn IViewLayout>,
}

impl<M: IViewModel> RestrictedRenderingContext<M> {
    pub fn new(view_layout: Rc<dyn IViewLayout>, viewport_data: ViewportData<M>) -> Self {
        let scroll_width = view_layout.get_scroll_width();
        let scroll_height = view_layout.get_scroll_height();

        let visible_range = viewport_data.visible_range;
        let big_numbers_delta = viewport_data.big_numbers_delta;

        let v_info = view_layout.get_current_viewport();
        Self {
            viewport_data,
            scroll_width,
            scroll_height,
            visible_range,
            big_numbers_delta,
            scroll_top: v_info.top,
            scroll_left: v_info.left,
            viewport_width: v_info.width,
            viewport_height: v_info.height,
            view_layout,
        }
    }

    pub fn get_scrolled_top_from_absolute_top(&self, absolute_top: f64) -> f64 {
        absolute_top - self.scroll_top
    }

    pub fn get_vertical_offset_for_line_number(&self, line_number: i32, include_view_zones: Option<bool>) -> f64 {
        self.view_layout.get_vertical_offset_for_line_number(line_number, include_view_zones)
    }

    pub fn get_vertical_offset_after_line_number(&self, line_number: i32, include_view_zones: Option<bool>) -> f64 {
        self.view_layout.get_vertical_offset_after_line_number(line_number, include_view_zones)
    }

    pub fn get_line_height_for_line_number(&self, line_number: i32) -> f64 {
        self.view_layout.get_line_height_for_line_number(line_number)
    }

    pub fn get_decorations_in_viewport(&self) -> Vec<ViewModelDecoration> {
        self.viewport_data.get_decorations_in_viewport()
    }
}

/// `RenderingContext`.
pub struct RenderingContext<M: IViewModel> {
    restricted: RestrictedRenderingContext<M>,
    view_lines: Rc<dyn IViewLines>,
    view_lines_gpu: Option<Rc<dyn IViewLines>>,
}

impl<M: IViewModel> Deref for RenderingContext<M> {
    type Target = RestrictedRenderingContext<M>;

    fn deref(&self) -> &Self::Target {
        &self.restricted
    }
}

impl<M: IViewModel> RenderingContext<M> {
    pub fn new(
        view_layout: Rc<dyn IViewLayout>,
        viewport_data: ViewportData<M>,
        view_lines: Rc<dyn IViewLines>,
        view_lines_gpu: Option<Rc<dyn IViewLines>>,
    ) -> Self {
        Self { restricted: RestrictedRenderingContext::new(view_layout, viewport_data), view_lines, view_lines_gpu }
    }

    pub fn lines_visible_ranges_for_range(&self, range: Range, include_new_lines: bool) -> Option<Vec<LineVisibleRanges>> {
        let dom_ranges = self.view_lines.lines_visible_ranges_for_range(range, include_new_lines);
        let Some(view_lines_gpu) = &self.view_lines_gpu else {
            return dom_ranges;
        };
        let gpu_ranges = view_lines_gpu.lines_visible_ranges_for_range(range, include_new_lines);
        let Some(mut dom_ranges) = dom_ranges else {
            return gpu_ranges;
        };
        let Some(gpu_ranges) = gpu_ranges else {
            return Some(dom_ranges);
        };
        dom_ranges.extend(gpu_ranges);
        dom_ranges.sort_by_key(|range| range.line_number);
        Some(dom_ranges)
    }

    pub fn visible_range_for_position(&self, position: Position) -> Option<HorizontalPosition> {
        self.view_lines
            .visible_range_for_position(position)
            .or_else(|| self.view_lines_gpu.as_ref().and_then(|gpu| gpu.visible_range_for_position(position)))
    }
}

/// `LineVisibleRanges`.
#[derive(Clone, Debug, PartialEq)]
pub struct LineVisibleRanges {
    pub outside_rendered_line: bool,
    pub line_number: i32,
    pub ranges: Vec<HorizontalRange>,
    /// Indica se o intervalo pedido não termina nesta linha, mas continua na próxima.
    pub continues_on_next_line: bool,
}

impl LineVisibleRanges {
    pub fn new(outside_rendered_line: bool, line_number: i32, ranges: Vec<HorizontalRange>, continues_on_next_line: bool) -> Self {
        Self { outside_rendered_line, line_number, ranges, continues_on_next_line }
    }

    /// O elemento de menor `lineNumber` (o primeiro em caso de empate).
    pub fn first_line(ranges: Option<&[LineVisibleRanges]>) -> Option<&LineVisibleRanges> {
        let mut result: Option<&LineVisibleRanges> = None;
        for range in ranges? {
            if result.is_none_or(|current| range.line_number < current.line_number) {
                result = Some(range);
            }
        }
        result
    }

    /// O elemento de maior `lineNumber` (o primeiro em caso de empate).
    pub fn last_line(ranges: Option<&[LineVisibleRanges]>) -> Option<&LineVisibleRanges> {
        let mut result: Option<&LineVisibleRanges> = None;
        for range in ranges? {
            if result.is_none_or(|current| range.line_number > current.line_number) {
                result = Some(range);
            }
        }
        result
    }
}

/// `HorizontalRange`: `left` e `width` arredondados.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HorizontalRange {
    pub left: f64,
    pub width: f64,
}

impl HorizontalRange {
    pub fn from(ranges: &[FloatHorizontalRange]) -> Vec<HorizontalRange> {
        ranges.iter().map(|range| HorizontalRange::new(range.left, range.width)).collect()
    }

    pub fn new(left: f64, width: f64) -> Self {
        Self { left: js_round(left), width: js_round(width) }
    }
}

impl std::fmt::Display for HorizontalRange {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[{},{}]", self.left, self.width)
    }
}

/// `FloatHorizontalRange`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FloatHorizontalRange {
    pub left: f64,
    pub width: f64,
}

impl FloatHorizontalRange {
    pub fn new(left: f64, width: f64) -> Self {
        Self { left, width }
    }

    /// `compare(a, b)`: `a.left - b.left`.
    pub fn compare(a: &FloatHorizontalRange, b: &FloatHorizontalRange) -> f64 {
        a.left - b.left
    }
}

impl std::fmt::Display for FloatHorizontalRange {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[{},{}]", self.left, self.width)
    }
}

/// `HorizontalPosition`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HorizontalPosition {
    pub outside_rendered_line: bool,
    /// `Math.round(this.originalLeft)`.
    pub left: f64,
    pub original_left: f64,
}

impl HorizontalPosition {
    pub fn new(outside_rendered_line: bool, left: f64) -> Self {
        Self { outside_rendered_line, original_left: left, left: js_round(left) }
    }
}

/// `VisibleRanges`.
#[derive(Clone, Debug, PartialEq)]
pub struct VisibleRanges {
    pub outside_rendered_line: bool,
    pub ranges: Vec<FloatHorizontalRange>,
}

impl VisibleRanges {
    pub fn new(outside_rendered_line: bool, ranges: Vec<FloatHorizontalRange>) -> Self {
        Self { outside_rendered_line, ranges }
    }
}
