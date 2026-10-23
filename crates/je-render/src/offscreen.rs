//! Pintura sem janela pelo caminho de CPU: a base do `je-code --screenshot` e dos testes visuais
//! contra o oráculo (H26).

use crate::cpu;
use crate::display_list::{DisplayList, Rgba};

/// Pinta `list` sobre `background` num buffer RGBA8 não pré-multiplicado de `width` x `height`.
pub fn render_rgba(list: &DisplayList, background: Rgba, width: u16, height: u16) -> Vec<u8> {
    let mut ctx = vello_cpu::RenderContext::new(width, height);
    let mut resources = vello_cpu::Resources::new();
    let mut pixmap = vello_cpu::Pixmap::new(width, height);
    ctx.set_paint(background.to_peniko());
    ctx.fill_rect(&vello::kurbo::Rect::new(0.0, 0.0, f64::from(width), f64::from(height)));
    cpu::encode(list, &mut ctx, &mut resources);
    ctx.flush();
    ctx.render(&mut pixmap, &mut resources);
    // Fundo opaco: o pré-multiplicado já é a cor final.
    pixmap.data().iter().flat_map(|p| [p.r, p.g, p.b, 0xff]).collect()
}
