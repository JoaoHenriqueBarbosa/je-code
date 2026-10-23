//! Reprodução da lista no `vello_cpu` (CPU).

use vello::kurbo::Stroke;

use crate::display_list::{Command, DisplayList, Shape};

pub(crate) fn encode(list: &DisplayList, ctx: &mut vello_cpu::RenderContext, resources: &mut vello_cpu::Resources) {
    for command in list.balanced() {
        match command {
            Command::Fill { shape, color } => {
                ctx.set_paint(color.to_peniko());
                match shape {
                    Shape::Rect(r) => ctx.fill_rect(r),
                    other => ctx.fill_path(&other.to_path()),
                }
            }
            Command::Stroke { shape, width, color } => {
                ctx.set_paint(color.to_peniko());
                ctx.set_stroke(Stroke::new(*width));
                ctx.stroke_path(&shape.to_path());
            }
            Command::Glyphs { font, size, color, x, y, glyphs } => {
                ctx.set_paint(color.to_peniko());
                // Erro de glifo (fonte corrompida) não derruba o frame: o trecho fica em branco,
                // como o Chromium faz com glifo que não rasteriza.
                let _ = ctx
                    .glyph_run(resources, font)
                    .font_size(*size)
                    .hint(true)
                    .fill_glyphs(glyphs.iter().map(|g| vello_cpu::Glyph { id: g.id, x: g.x + x, y: g.y + y }));
            }
            Command::PushClip(shape) => ctx.push_clip_layer(&shape.to_path()),
            Command::PopClip => ctx.pop_layer(),
        }
    }
}
