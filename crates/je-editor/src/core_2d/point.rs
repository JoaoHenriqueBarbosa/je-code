//! Porte de `src/vs/editor/common/core/2d/point.ts`: `Point`.
//!
//! Os números do JavaScript são `f64`. O `toString` usa a formatação `{}` do `f64`, que coincide com a do
//! JavaScript para inteiros e para decimais curtos (`100` e `1.5`).

use std::fmt;

/// `Point`: um ponto imutável.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

impl Point {
    /// `Point.equals(a, b)`.
    pub fn equals(a: &Point, b: &Point) -> bool {
        a.x == b.x && a.y == b.y
    }

    pub fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }

    pub fn add(&self, other: Point) -> Point {
        Point::new(self.x + other.x, self.y + other.y)
    }

    pub fn delta_x(&self, delta: f64) -> Point {
        Point::new(self.x + delta, self.y)
    }

    pub fn delta_y(&self, delta: f64) -> Point {
        Point::new(self.x, self.y + delta)
    }

    pub fn subtract(&self, other: Point) -> Point {
        Point::new(self.x - other.x, self.y - other.y)
    }

    pub fn scale(&self, factor: f64) -> Point {
        Point::new(self.x * factor, self.y * factor)
    }

    pub fn map_components(&self, map: impl Fn(f64) -> f64) -> Point {
        Point::new(map(self.x), map(self.y))
    }

    pub fn is_zero(&self) -> bool {
        self.x == 0.0 && self.y == 0.0
    }

    pub fn with_threshold(&self, threshold: f64) -> Point {
        self.map_components(|axis_val| {
            if axis_val > threshold {
                axis_val - threshold
            } else if axis_val < -threshold {
                axis_val + threshold
            } else {
                0.0
            }
        })
    }
}

impl fmt::Display for Point {
    /// `toString()`: `(x,y)`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "({},{})", self.x, self.y)
    }
}
