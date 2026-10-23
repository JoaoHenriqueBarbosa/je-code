//! Porte de `src/vs/editor/common/core/2d/size.ts`: `Size2D`.
//!
//! Os números do JavaScript são `f64`. O `toString` usa a formatação `{}` do `f64`, que coincide com a do
//! JavaScript para inteiros e para decimais curtos (`100` e `1.5`).

use super::dimension::Dimension;

/// `Size2D`: largura e altura imutáveis.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Size2D {
    pub width: f64,
    pub height: f64,
}

impl Size2D {
    /// `Size2D.equals(a, b)`.
    pub fn equals(a: Size2D, b: Size2D) -> bool {
        a.width == b.width && a.height == b.height
    }

    pub fn new(width: f64, height: f64) -> Self {
        Self { width, height }
    }

    pub fn add(&self, other: Size2D) -> Size2D {
        Size2D::new(self.width + other.width, self.height + other.height)
    }

    pub fn delta_x(&self, delta: f64) -> Size2D {
        Size2D::new(self.width + delta, self.height)
    }

    pub fn delta_y(&self, delta: f64) -> Size2D {
        Size2D::new(self.width, self.height + delta)
    }

    pub fn subtract(&self, other: Size2D) -> Size2D {
        Size2D::new(self.width - other.width, self.height - other.height)
    }

    pub fn scale(&self, factor: f64) -> Size2D {
        Size2D::new(self.width * factor, self.height * factor)
    }

    pub fn scale_width(&self, factor: f64) -> Size2D {
        Size2D::new(self.width * factor, self.height)
    }

    pub fn map_components(&self, map: impl Fn(f64) -> f64) -> Size2D {
        Size2D::new(map(self.width), map(self.height))
    }

    pub fn is_zero(&self) -> bool {
        self.width == 0.0 && self.height == 0.0
    }

    pub fn transpose(&self) -> Size2D {
        Size2D::new(self.height, self.width)
    }

    pub fn to_dimension(&self) -> Dimension {
        Dimension { width: self.width, height: self.height }
    }
}

impl std::fmt::Display for Size2D {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "({},{})", self.width, self.height)
    }
}
