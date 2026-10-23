//! Renderização nativa do je-code (H03, H04 e H07 em `docs/bench-report.md`).
//!
//! Os widgets não falam com o rasterizador: produzem uma [`DisplayList`], que é reproduzida pelo
//! `vello` na GPU ou pelo `vello_cpu` na CPU. A lista é o ponto de teste (dá para comparar a saída
//! de um widget sem janela) e o ponto onde entra o recorte de região suja.

mod cpu;
mod display_list;
mod gpu;
mod offscreen;
mod presenter;
pub mod wsl;

pub use display_list::{Command, DisplayList, Glyph, Rgba, Shape};
pub use offscreen::render_rgba;
pub use presenter::{Backend, Presenter, Preference};
pub use vello::kurbo;
pub use vello::peniko::FontData;
