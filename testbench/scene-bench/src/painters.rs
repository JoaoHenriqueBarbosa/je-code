//! Os dois pintores da cena (GPU pelo `vello`, CPU pelo `vello_cpu`), compartilhados pelo
//! `scene-bench` (nativo) e pelo `scene-web` (navegador).

use crate::scene::{Painter, Rgba, Run};
use vello::kurbo::{Affine, Rect};
use vello::peniko::{Color, Fill};

pub fn color(c: Rgba) -> Color {
    Color::from_rgba8(c[0], c[1], c[2], c[3])
}

pub struct GpuPainter<'a>(pub &'a mut vello::Scene);

impl Painter for GpuPainter<'_> {
    fn rect(&mut self, x: f32, y: f32, w: f32, h: f32, c: Rgba) {
        let r = Rect::new(f64::from(x), f64::from(y), f64::from(x + w), f64::from(y + h));
        self.0.fill(Fill::NonZero, Affine::IDENTITY, color(c), None, &r);
    }

    fn glyphs(&mut self, run: &Run, dx: f32, dy: f32) {
        self.0
            .draw_glyphs(&run.font)
            .font_size(run.size)
            .brush(color(run.color))
            .draw(Fill::NonZero, run.glyphs.iter().map(|&(id, x, y)| vello::Glyph { id, x: x + dx, y: y + dy }));
    }
}

pub struct CpuPainter<'a> {
    pub ctx: &'a mut vello_cpu::RenderContext,
    pub resources: &'a mut vello_cpu::Resources,
}

impl Painter for CpuPainter<'_> {
    fn rect(&mut self, x: f32, y: f32, w: f32, h: f32, c: Rgba) {
        self.ctx.set_paint(color(c));
        self.ctx.fill_rect(&Rect::new(f64::from(x), f64::from(y), f64::from(x + w), f64::from(y + h)));
    }

    fn glyphs(&mut self, run: &Run, dx: f32, dy: f32) {
        self.ctx.set_paint(color(run.color));
        let glyphs = run.glyphs.iter().map(|&(id, x, y)| vello_cpu::Glyph { id, x: x + dx, y: y + dy });
        let _ = self.ctx.glyph_run(self.resources, &run.font).font_size(run.size).fill_glyphs(glyphs);
    }
}
