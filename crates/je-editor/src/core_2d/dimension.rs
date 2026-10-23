//! Porte de `src/vs/editor/common/core/2d/dimension.ts`: `IDimension`.
//!
//! Os números do JavaScript são `f64`.

/// `IDimension`: largura e altura.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Dimension {
    pub width: f64,
    pub height: f64,
}
