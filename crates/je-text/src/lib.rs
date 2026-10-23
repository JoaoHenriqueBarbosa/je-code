//! Texto do je-code (H09, H10 e H11 em `docs/bench-report.md`).
//!
//! - Fontes: as do sistema pelo `fontique` (os mesmos caminhos do Chromium: fontconfig, DirectWrite,
//!   CoreText), mais o `codicon.ttf` registrado sob a família `codicon`, como o `@font-face` do VS Code.
//! - Shaping e quebra: `parley`.
//! - Métrica: a largura de um trecho é arredondada para cima em 1/64 px, que é o que o LayoutNG do
//!   Chromium faz (`LayoutUnit::FromFloatCeil`); com isso a largura bate com a do oráculo (H10).
//! - Cache: o layout de um trecho (texto, família, tamanho, peso) é feito uma vez e devolvido como
//!   glifos compartilhados (`Arc<[Glyph]>`) prontos para a lista de exibição.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

use je_render::{DisplayList, FontData, Glyph, Rgba};
use parley::fontique::Blob;
use parley::{FontContext, FontWeight, LayoutContext, PositionedLayoutItem, StyleProperty};

/// A família sob a qual o `codicon.ttf` é registrado (o `font-family: "codicon"` do `codicon.css`).
pub const CODICON_FAMILY: &str = "codicon";

/// Arredonda para cima em 1/64 px (`LayoutUnit::FromFloatCeil`).
pub fn layout_unit_ceil(px: f32) -> f32 {
    (px * 64.0).ceil() / 64.0
}

/// Uma fonte de texto pedida: a lista de famílias no formato CSS do VS Code
/// (`system-ui, "Ubuntu", "Droid Sans", sans-serif`), o tamanho em px e o peso.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct TextStyle {
    pub families: String,
    /// Tamanho em 1/64 px, para o estilo poder ser chave de cache.
    pub size_64: u32,
    pub weight: u16,
}

impl TextStyle {
    pub fn new(families: impl Into<String>, size_px: f32, weight: u16) -> Self {
        Self { families: families.into(), size_64: (size_px * 64.0).round() as u32, weight }
    }

    pub fn size(&self) -> f32 {
        self.size_64 as f32 / 64.0
    }
}

/// Um trecho de glifos com fonte e tamanho únicos, em coordenadas relativas ao início da linha
/// (y é a linha de base).
#[derive(Clone, Debug)]
pub struct Run {
    pub font: FontData,
    pub size: f32,
    pub glyphs: Arc<[Glyph]>,
}

/// O layout de uma linha de texto.
#[derive(Clone, Debug)]
pub struct TextLine {
    pub runs: Vec<Run>,
    /// Largura com o arredondamento do Chromium.
    pub width: f32,
    /// A linha de base medida a partir do topo da caixa de linha da fonte.
    pub baseline: f32,
    /// A altura da caixa de linha natural da fonte (ascent + descent + gap).
    pub line_height: f32,
}

impl TextLine {
    /// Pinta a linha com o topo da caixa em (x, y).
    pub fn paint(&self, list: &mut DisplayList, x: f32, y: f32, color: Rgba) {
        for run in &self.runs {
            list.glyphs(&run.font, run.size, color, x, y + self.baseline, &run.glyphs);
        }
    }
}

/// O sistema de texto: contextos do parley e o cache de linhas.
pub struct TextSystem {
    fcx: FontContext,
    lcx: LayoutContext<()>,
    cache: HashMap<(String, TextStyle), Rc<TextLine>>,
}

impl Default for TextSystem {
    fn default() -> Self {
        Self::new()
    }
}

impl TextSystem {
    pub fn new() -> Self {
        let mut fcx = FontContext::new();
        fcx.collection.register_fonts(Blob::new(Arc::new(je_codicons::FONT)), None);
        Self { fcx, lcx: LayoutContext::new(), cache: HashMap::new() }
    }

    /// Registra uma fonte extra (de extensão, de tema de ícones de produto).
    pub fn register_font(&mut self, data: Vec<u8>) -> Vec<String> {
        let families = self.fcx.collection.register_fonts(Blob::new(Arc::new(data)), None);
        families.iter().filter_map(|(id, _)| self.fcx.collection.family_name(*id).map(str::to_owned)).collect()
    }

    /// O layout de `text` numa linha só (sem quebra), do cache quando possível.
    pub fn line(&mut self, text: &str, style: &TextStyle) -> Rc<TextLine> {
        let key = (text.to_owned(), style.clone());
        if let Some(line) = self.cache.get(&key) {
            return line.clone();
        }
        let line = Rc::new(self.shape(text, style));
        self.cache.insert(key, line.clone());
        line
    }

    /// O glifo de um codicon pelo id (`$(files)`), no tamanho pedido.
    pub fn codicon(&mut self, id: &str, size_px: f32) -> Option<Rc<TextLine>> {
        let ch = je_codicons::code_point(id)?;
        Some(self.line(&ch.to_string(), &TextStyle::new(CODICON_FAMILY, size_px, 400)))
    }

    fn shape(&mut self, text: &str, style: &TextStyle) -> TextLine {
        let mut builder = self.lcx.ranged_builder(&mut self.fcx, text, 1.0, true);
        builder.push_default(StyleProperty::FontFamily(style.families.as_str().into()));
        builder.push_default(StyleProperty::FontSize(style.size()));
        builder.push_default(StyleProperty::FontWeight(FontWeight::new(f32::from(style.weight))));
        let mut layout = builder.build(text);
        layout.break_all_lines(None);
        let mut runs = Vec::new();
        let (mut width, mut baseline, mut line_height) = (0.0f32, 0.0f32, 0.0f32);
        if let Some(line) = layout.lines().next() {
            let m = line.metrics();
            width = m.advance;
            baseline = m.baseline;
            line_height = m.line_height;
            for item in line.items() {
                if let PositionedLayoutItem::GlyphRun(glyph_run) = item {
                    let run = glyph_run.run();
                    let glyphs: Vec<Glyph> = glyph_run.positioned_glyphs().map(|g| Glyph { id: g.id, x: g.x, y: g.y - baseline }).collect();
                    runs.push(Run { font: run.font().clone(), size: run.font_size(), glyphs: glyphs.into() });
                }
            }
        }
        TextLine { runs, width: layout_unit_ceil(width), baseline, line_height }
    }
}

thread_local! {
    static SYSTEM: RefCell<Option<TextSystem>> = const { RefCell::new(None) };
}

/// O sistema de texto do thread da UI, criado no primeiro uso.
pub fn with_text_system<T>(f: impl FnOnce(&mut TextSystem) -> T) -> T {
    SYSTEM.with(|s| f(s.borrow_mut().get_or_insert_with(TextSystem::new)))
}
