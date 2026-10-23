//! A lista de comandos de pintura, independente de rasterizador.

use std::sync::Arc;

use vello::kurbo::{BezPath, Rect, RoundedRect};
use vello::peniko::FontData;

/// Cor em sRGB com alfa não pré-multiplicado, como as cores de tema do VS Code (`#rrggbbaa`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Rgba {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Rgba {
    pub const TRANSPARENT: Rgba = Rgba::new(0, 0, 0, 0);

    pub const fn new(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }

    pub const fn opaque(r: u8, g: u8, b: u8) -> Self {
        Self::new(r, g, b, 0xff)
    }

    pub(crate) fn to_peniko(self) -> vello::peniko::Color {
        vello::peniko::Color::from_rgba8(self.r, self.g, self.b, self.a)
    }
}

/// Um glifo já posicionado, relativo à origem do trecho.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Glyph {
    pub id: u32,
    pub x: f32,
    pub y: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Shape {
    Rect(Rect),
    RoundedRect(RoundedRect),
    Path(BezPath),
}

impl Shape {
    pub(crate) fn to_path(&self) -> BezPath {
        use vello::kurbo::Shape as _;
        match self {
            Shape::Rect(r) => r.to_path(0.1),
            Shape::RoundedRect(r) => r.to_path(0.1),
            Shape::Path(p) => p.clone(),
        }
    }
}

#[derive(Clone, Debug)]
pub enum Command {
    Fill { shape: Shape, color: Rgba },
    Stroke { shape: Shape, width: f64, color: Rgba },
    /// Glifos compartilhados por `Arc`: o cache de layout de linha entrega o mesmo vetor a cada
    /// frame, e a lista só soma a origem.
    Glyphs { font: FontData, size: f32, color: Rgba, x: f32, y: f32, glyphs: Arc<[Glyph]> },
    PushClip(Shape),
    PopClip,
}

/// A lista de exibição de um frame.
#[derive(Clone, Debug, Default)]
pub struct DisplayList {
    commands: Vec<Command>,
    clip_depth: usize,
}

impl DisplayList {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn clear(&mut self) {
        self.commands.clear();
        self.clip_depth = 0;
    }

    pub fn commands(&self) -> &[Command] {
        &self.commands
    }

    pub fn fill_rect(&mut self, rect: Rect, color: Rgba) {
        if color.a != 0 && rect.area() > 0.0 {
            self.commands.push(Command::Fill { shape: Shape::Rect(rect), color });
        }
    }

    pub fn fill(&mut self, shape: Shape, color: Rgba) {
        if color.a != 0 {
            self.commands.push(Command::Fill { shape, color });
        }
    }

    pub fn stroke(&mut self, shape: Shape, width: f64, color: Rgba) {
        if color.a != 0 && width > 0.0 {
            self.commands.push(Command::Stroke { shape, width, color });
        }
    }

    pub fn glyphs(&mut self, font: &FontData, size: f32, color: Rgba, x: f32, y: f32, glyphs: &Arc<[Glyph]>) {
        if color.a != 0 && !glyphs.is_empty() {
            self.commands.push(Command::Glyphs { font: font.clone(), size, color, x, y, glyphs: glyphs.clone() });
        }
    }

    pub fn push_clip(&mut self, shape: Shape) {
        self.clip_depth += 1;
        self.commands.push(Command::PushClip(shape));
    }

    /// Fecha o recorte mais recente. Sem recorte aberto, não faz nada (o rasterizador não pode
    /// receber um `pop` sem `push`).
    pub fn pop_clip(&mut self) {
        if self.clip_depth > 0 {
            self.clip_depth -= 1;
            self.commands.push(Command::PopClip);
        }
    }

    /// Recortes ainda abertos no fim do frame são fechados aqui, antes da reprodução.
    pub(crate) fn balanced(&self) -> impl Iterator<Item = &Command> {
        self.commands.iter().chain(std::iter::repeat_n(&Command::PopClip, self.clip_depth))
    }
}
