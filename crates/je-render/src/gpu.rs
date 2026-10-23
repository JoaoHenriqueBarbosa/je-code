//! Reprodução da lista no `vello` (GPU).

use vello::kurbo::{Affine, Stroke};
use vello::peniko::Fill;

use crate::display_list::{Command, DisplayList, Shape};

fn fill_shape(scene: &mut vello::Scene, shape: &Shape, brush: vello::peniko::Color) {
    match shape {
        Shape::Rect(r) => scene.fill(Fill::NonZero, Affine::IDENTITY, brush, None, r),
        Shape::RoundedRect(r) => scene.fill(Fill::NonZero, Affine::IDENTITY, brush, None, r),
        Shape::Path(p) => scene.fill(Fill::NonZero, Affine::IDENTITY, brush, None, p),
    }
}

pub(crate) fn encode(list: &DisplayList, scene: &mut vello::Scene) {
    for command in list.balanced() {
        match command {
            Command::Fill { shape, color } => fill_shape(scene, shape, color.to_peniko()),
            Command::Stroke { shape, width, color } => {
                let style = Stroke::new(*width);
                match shape {
                    Shape::Rect(r) => scene.stroke(&style, Affine::IDENTITY, color.to_peniko(), None, r),
                    Shape::RoundedRect(r) => scene.stroke(&style, Affine::IDENTITY, color.to_peniko(), None, r),
                    Shape::Path(p) => scene.stroke(&style, Affine::IDENTITY, color.to_peniko(), None, p),
                }
            }
            Command::Glyphs { font, size, color, x, y, glyphs } => {
                scene.draw_glyphs(font).font_size(*size).brush(color.to_peniko()).draw(
                    Fill::NonZero,
                    glyphs.iter().map(|g| vello::Glyph { id: g.id, x: g.x + x, y: g.y + y }),
                );
            }
            Command::PushClip(shape) => match shape {
                Shape::Rect(r) => scene.push_clip_layer(Fill::NonZero, Affine::IDENTITY, r),
                Shape::RoundedRect(r) => scene.push_clip_layer(Fill::NonZero, Affine::IDENTITY, r),
                Shape::Path(p) => scene.push_clip_layer(Fill::NonZero, Affine::IDENTITY, p),
            },
            Command::PopClip => scene.pop_layer(),
        }
    }
}
