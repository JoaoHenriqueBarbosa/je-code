//! Porte de `src/vs/editor/common/core/2d/rect.ts`: `Rect`.
//!
//! Os números do JavaScript são `f64`. O `toString` usa a formatação `{}` do `f64`, que coincide com a do
//! JavaScript para inteiros e para decimais curtos (`100` e `1.5`).
//!
//! Notas de tradução:
//! - o `OffsetRange` do crate é `i32`: `fromRanges` e `withHorizontalRange`/`withVerticalRange` convertem os extremos
//!   para `f64`, e `getHorizontalRange`/`getVerticalRange` arredondam as bordas para o pixel inteiro mais próximo (a
//!   mesma convenção de `ObservableCodeEditor::observe_line_offset_range`);
//! - `Number.MAX_SAFE_INTEGER` do `hull` é `2^53 - 1` como `f64`;
//! - as três sobrecargas de `withMargin` são [`Rect::with_margin`] (uma margem), [`Rect::with_vertical_horizontal_margin`]
//!   (vertical e horizontal) e [`Rect::with_margins`] (as quatro, na ordem topo, direita, base, esquerda);
//! - o construtor falha (`BugIndicatingError`) se as bordas estão invertidas, como o original;
//! - [`StrictEquals`] compara por valor, como o `Point`: o observável que carrega um `Rect` só avisa quando o retângulo
//!   mudou de fato.
//!
//! Divergências (fora do escopo, por nome de membro do original):
//! - `toStyles`: devolve o objeto de estilo CSS (`position: absolute`, `left`, `top`, `width`, `height` em `px`) de um
//!   elemento DOM. A UI é nativa: quem pinta lê `left`, `top`, `width()` e `height()` direto do retângulo.

use std::fmt;

use je_base::errors::{self, Error};
use je_base::observable::deps::StrictEquals;

use super::point::Point;
use super::size::Size2D;
use crate::offset_range::OffsetRange;

/// `Number.MAX_SAFE_INTEGER`.
const MAX_SAFE_INTEGER: f64 = 9_007_199_254_740_991.0;

/// `Rect`: um retângulo imutável, dado pelas quatro bordas.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub left: f64,
    pub top: f64,
    pub right: f64,
    pub bottom: f64,
}

impl Rect {
    pub fn from_point(point: Point) -> Rect {
        Rect::new(point.x, point.y, point.x, point.y)
    }

    pub fn from_points(top_left: Point, bottom_right: Point) -> Rect {
        Rect::new(top_left.x, top_left.y, bottom_right.x, bottom_right.y)
    }

    pub fn from_point_size(point: Point, size: Point) -> Rect {
        Rect::new(point.x, point.y, point.x + size.x, point.y + size.y)
    }

    pub fn from_left_top_right_bottom(left: f64, top: f64, right: f64, bottom: f64) -> Rect {
        Rect::new(left, top, right, bottom)
    }

    pub fn from_left_top_width_height(left: f64, top: f64, width: f64, height: f64) -> Rect {
        Rect::new(left, top, left + width, top + height)
    }

    pub fn from_ranges(left_right: OffsetRange, top_bottom: OffsetRange) -> Rect {
        Rect::new(
            f64::from(left_right.start),
            f64::from(top_bottom.start),
            f64::from(left_right.end_exclusive),
            f64::from(top_bottom.end_exclusive),
        )
    }

    /// `Rect.hull(rects)`: o menor retângulo que cobre todos.
    pub fn hull(rects: &[Rect]) -> Rect {
        let mut left = MAX_SAFE_INTEGER;
        let mut top = MAX_SAFE_INTEGER;
        let mut right = -MAX_SAFE_INTEGER;
        let mut bottom = -MAX_SAFE_INTEGER;

        for rect in rects {
            left = left.min(rect.left);
            top = top.min(rect.top);
            right = right.max(rect.right);
            bottom = bottom.max(rect.bottom);
        }

        Rect::new(left, top, right, bottom)
    }

    /// O construtor: bordas invertidas são erro de lógica.
    pub fn new(left: f64, top: f64, right: f64, bottom: f64) -> Rect {
        if left > right {
            errors::throw(Error::bug_indicating(Some(&format!("Invalid arguments: Horizontally offset by {}", left - right))));
        }
        if top > bottom {
            errors::throw(Error::bug_indicating(Some(&format!("Invalid arguments: Vertically offset by {}", top - bottom))));
        }
        Rect { left, top, right, bottom }
    }

    pub fn width(&self) -> f64 {
        self.right - self.left
    }

    pub fn height(&self) -> f64 {
        self.bottom - self.top
    }

    /// `withMargin(margin)`: a mesma margem nos quatro lados.
    pub fn with_margin(&self, margin: f64) -> Rect {
        self.with_margins(margin, margin, margin, margin)
    }

    /// `withMargin(marginVertical, marginHorizontal)`.
    pub fn with_vertical_horizontal_margin(&self, margin_vertical: f64, margin_horizontal: f64) -> Rect {
        self.with_margins(margin_vertical, margin_horizontal, margin_vertical, margin_horizontal)
    }

    /// `withMargin(marginTop, marginRight, marginBottom, marginLeft)`.
    pub fn with_margins(&self, margin_top: f64, margin_right: f64, margin_bottom: f64, margin_left: f64) -> Rect {
        Rect::new(self.left - margin_left, self.top - margin_top, self.right + margin_right, self.bottom + margin_bottom)
    }

    pub fn intersect_vertical(&self, range: OffsetRange) -> Rect {
        let new_top = self.top.max(f64::from(range.start));
        let new_bottom = self.bottom.min(f64::from(range.end_exclusive));
        Rect::new(self.left, new_top, self.right, new_top.max(new_bottom))
    }

    pub fn intersect_horizontal(&self, range: OffsetRange) -> Rect {
        let new_left = self.left.max(f64::from(range.start));
        let new_right = self.right.min(f64::from(range.end_exclusive));
        Rect::new(new_left, self.top, new_left.max(new_right), self.bottom)
    }

    /// `intersect(parent)`: `None` quando os retângulos não se tocam.
    pub fn intersect(&self, parent: &Rect) -> Option<Rect> {
        let left = self.left.max(parent.left);
        let right = self.right.min(parent.right);
        let top = self.top.max(parent.top);
        let bottom = self.bottom.min(parent.bottom);

        if left > right || top > bottom {
            return None;
        }

        Some(Rect::new(left, top, right, bottom))
    }

    pub fn union(&self, other: &Rect) -> Rect {
        Rect::new(
            self.left.min(other.left),
            self.top.min(other.top),
            self.right.max(other.right),
            self.bottom.max(other.bottom),
        )
    }

    pub fn contains_rect(&self, other: &Rect) -> bool {
        self.left <= other.left && self.top <= other.top && self.right >= other.right && self.bottom >= other.bottom
    }

    pub fn contains_point(&self, point: Point) -> bool {
        self.left <= point.x && self.top <= point.y && self.right >= point.x && self.bottom >= point.y
    }

    pub fn move_to_be_contained_in(&self, parent: &Rect) -> Rect {
        let width = self.width();
        let height = self.height();

        let mut left = self.left;
        let mut top = self.top;

        if left < parent.left {
            left = parent.left;
        } else if left + width > parent.right {
            left = parent.right - width;
        }

        if top < parent.top {
            top = parent.top;
        } else if top + height > parent.bottom {
            top = parent.bottom - height;
        }

        Rect::new(left, top, left + width, top + height)
    }

    pub fn with_width(&self, width: f64) -> Rect {
        Rect::new(self.left, self.top, self.left + width, self.bottom)
    }

    pub fn with_height(&self, height: f64) -> Rect {
        Rect::new(self.left, self.top, self.right, self.top + height)
    }

    pub fn with_top(&self, top: f64) -> Rect {
        Rect::new(self.left, top, self.right, self.bottom)
    }

    pub fn with_left(&self, left: f64) -> Rect {
        Rect::new(left, self.top, self.right, self.bottom)
    }

    pub fn translate_x(&self, delta: f64) -> Rect {
        Rect::new(self.left + delta, self.top, self.right + delta, self.bottom)
    }

    pub fn translate_y(&self, delta: f64) -> Rect {
        Rect::new(self.left, self.top + delta, self.right, self.bottom + delta)
    }

    pub fn translate(&self, point: Point) -> Rect {
        Rect::new(self.left + point.x, self.top + point.y, self.right + point.x, self.bottom + point.y)
    }

    pub fn delta_right(&self, delta: f64) -> Rect {
        Rect::new(self.left, self.top, self.right + delta, self.bottom)
    }

    pub fn delta_top(&self, delta: f64) -> Rect {
        Rect::new(self.left, self.top + delta, self.right, self.bottom)
    }

    pub fn delta_left(&self, delta: f64) -> Rect {
        Rect::new(self.left + delta, self.top, self.right, self.bottom)
    }

    pub fn delta_bottom(&self, delta: f64) -> Rect {
        Rect::new(self.left, self.top, self.right, self.bottom + delta)
    }

    pub fn get_left_bottom(&self) -> Point {
        Point::new(self.left, self.bottom)
    }

    pub fn get_right_bottom(&self) -> Point {
        Point::new(self.right, self.bottom)
    }

    pub fn get_left_top(&self) -> Point {
        Point::new(self.left, self.top)
    }

    pub fn get_right_top(&self) -> Point {
        Point::new(self.right, self.top)
    }

    pub fn get_horizontal_range(&self) -> OffsetRange {
        OffsetRange::new(self.left.round() as i32, self.right.round() as i32)
    }

    pub fn get_vertical_range(&self) -> OffsetRange {
        OffsetRange::new(self.top.round() as i32, self.bottom.round() as i32)
    }

    pub fn with_horizontal_range(&self, range: OffsetRange) -> Rect {
        Rect::new(f64::from(range.start), self.top, f64::from(range.end_exclusive), self.bottom)
    }

    pub fn with_vertical_range(&self, range: OffsetRange) -> Rect {
        Rect::new(self.left, f64::from(range.start), self.right, f64::from(range.end_exclusive))
    }

    pub fn get_size(&self) -> Size2D {
        Size2D::new(self.width(), self.height())
    }
}

impl fmt::Display for Rect {
    /// `toString()`: `Rect{(left,top), (right,bottom)}`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Rect{{({},{}), ({},{})}}", self.left, self.top, self.right, self.bottom)
    }
}

impl StrictEquals for Rect {
    fn strict_equals(&self, other: &Self) -> bool {
        self == other
    }
}
