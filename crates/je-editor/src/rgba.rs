//! Porte de `src/vs/editor/common/core/misc/rgba.ts`: `RGBA8`, a cor de 8 bits por canal que o minimap usa.
//!
//! Divergências:
//! - O construtor recebe `f64` (o original recebe `number`, e o alfa chega de `Math.round(a * 255)`), e
//!   `_clamp` vira um limite em [0, 255] seguido de truncamento (`c | 0`); `NaN` dá 0, como no JavaScript.
//! - `RGBA8.Empty` é a constante [`Rgba8::EMPTY`], e o campo de marca `_rgba8Brand` não existe.

/// `RGBA8`: uma estrutura de cor de 8 bits por canal, sem alocação (`Copy`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rgba8 {
    /// Vermelho: inteiro em [0, 255].
    pub r: u8,
    /// Verde: inteiro em [0, 255].
    pub g: u8,
    /// Azul: inteiro em [0, 255].
    pub b: u8,
    /// Alfa: inteiro em [0, 255].
    pub a: u8,
}

impl Rgba8 {
    /// `RGBA8.Empty`.
    pub const EMPTY: Rgba8 = Rgba8 { r: 0, g: 0, b: 0, a: 0 };

    pub fn new(r: f64, g: f64, b: f64, a: f64) -> Self {
        Self { r: Self::clamp(r), g: Self::clamp(g), b: Self::clamp(b), a: Self::clamp(a) }
    }

    /// `_clamp(c)`: limita a [0, 255] e trunca (`c | 0`). O `as u8` do Rust já satura e dá 0 para `NaN`.
    pub fn clamp(c: f64) -> u8 {
        c as u8
    }
}
