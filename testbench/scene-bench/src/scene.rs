//! Cena sintética de workbench (H03 e H04): partes do layout do VS Code em retângulos e o texto de
//! um arquivo real do próprio VS Code, com cores por token, rolando um pixel por frame.
//!
//! O texto é diagramado uma vez pelo parley (cache de linha, como o view model do editor faz) e a
//! cena de cada frame só reposiciona e pinta os glifos visíveis. Os dois rasterizadores recebem os
//! mesmos comandos pelo trait `Painter`.

use std::sync::Arc;

use parley::fontique::Blob;
use parley::{FontContext, FontData, LayoutContext, PositionedLayoutItem, StyleProperty};

pub type Rgba = [u8; 4];

pub const WIDTH: u32 = 1920;
pub const HEIGHT: u32 = 1080;
const FONT_SIZE: f32 = 14.0;
const LINE_HEIGHT: f32 = 19.0;
const ACTIVITY_BAR: f32 = 48.0;
const SIDEBAR: f32 = 300.0;
const TAB_BAR: f32 = 35.0;
const STATUS_BAR: f32 = 22.0;
const GUTTER: f32 = 64.0;
const MINIMAP: f32 = 110.0;

// Cores do tema Dark Modern (extensions/theme-defaults/themes/dark_modern.json).
const BACKGROUND: Rgba = [0x1f, 0x1f, 0x1f, 0xff];
const SIDEBAR_BG: Rgba = [0x18, 0x18, 0x18, 0xff];
const BORDER: Rgba = [0x2b, 0x2b, 0x2b, 0xff];
const STATUS_BG: Rgba = [0x18, 0x18, 0x18, 0xff];
const TAB_ACTIVE: Rgba = [0x1f, 0x1f, 0x1f, 0xff];
const LINE_NUMBER: Rgba = [0x6e, 0x76, 0x81, 0xff];
const FOREGROUND: Rgba = [0xcc, 0xcc, 0xcc, 0xff];
const TOKEN_COLORS: [Rgba; 6] = [
    [0x56, 0x9c, 0xd6, 0xff],
    [0x9c, 0xdc, 0xfe, 0xff],
    [0xce, 0x91, 0x78, 0xff],
    [0xdc, 0xdc, 0xaa, 0xff],
    [0x4e, 0xc9, 0xb0, 0xff],
    [0x6a, 0x99, 0x55, 0xff],
];

/// Um trecho de glifos com fonte, tamanho e cor únicos, em coordenadas relativas à linha.
pub struct Run {
    pub font: FontData,
    pub size: f32,
    pub color: Rgba,
    pub glyphs: Vec<(u32, f32, f32)>,
}

/// Uma linha já diagramada.
pub struct Line {
    pub runs: Vec<Run>,
}

pub trait Painter {
    fn rect(&mut self, x: f32, y: f32, w: f32, h: f32, color: Rgba);
    fn glyphs(&mut self, run: &Run, dx: f32, dy: f32);
}

pub struct Workbench {
    code: Vec<Line>,
    numbers: Vec<Line>,
    tree: Vec<Line>,
    minimap: Vec<Vec<(f32, f32, Rgba)>>,
}

fn token_color(word: &str) -> Rgba {
    let hash = word.bytes().fold(0u32, |h, b| h.wrapping_mul(31).wrapping_add(u32::from(b)));
    TOKEN_COLORS[(hash % TOKEN_COLORS.len() as u32) as usize]
}

/// Faixas de token: palavras e o resto, cada uma com uma cor, para multiplicar as trocas de pincel
/// como um tokenizador real faz.
fn token_ranges(text: &str) -> Vec<(std::ops::Range<usize>, Rgba)> {
    let mut ranges = Vec::new();
    let mut start = 0;
    let mut in_word = false;
    for (i, ch) in text.char_indices() {
        let word = ch.is_alphanumeric() || ch == '_';
        if i > start && word != in_word {
            let piece = &text[start..i];
            ranges.push((start..i, if in_word { token_color(piece) } else { FOREGROUND }));
            start = i;
        }
        in_word = word;
    }
    if start < text.len() {
        let piece = &text[start..];
        ranges.push((start..text.len(), if in_word { token_color(piece) } else { FOREGROUND }));
    }
    ranges
}

struct Shaper {
    fcx: FontContext,
    lcx: LayoutContext<Rgba>,
}

impl Shaper {
    fn line(&mut self, text: &str, family: &str, ranges: &[(std::ops::Range<usize>, Rgba)]) -> Line {
        let mut builder = self.lcx.ranged_builder(&mut self.fcx, text, 1.0, true);
        builder.push_default(StyleProperty::FontFamily(family.into()));
        builder.push_default(StyleProperty::FontSize(FONT_SIZE));
        builder.push_default(StyleProperty::Brush(FOREGROUND));
        for (range, color) in ranges {
            builder.push(StyleProperty::Brush(*color), range.clone());
        }
        let mut layout = builder.build(text);
        layout.break_all_lines(None);
        let mut runs = Vec::new();
        for line in layout.lines() {
            for item in line.items() {
                if let PositionedLayoutItem::GlyphRun(glyph_run) = item {
                    let run = glyph_run.run();
                    runs.push(Run {
                        font: run.font().clone(),
                        size: run.font_size(),
                        color: glyph_run.style().brush,
                        glyphs: glyph_run.positioned_glyphs().map(|g| (g.id, g.x, g.y)).collect(),
                    });
                }
            }
        }
        Line { runs }
    }
}

impl Workbench {
    /// `mono` e `ui` são arquivos de fonte; `source` é o texto que vai no editor.
    pub fn new(mono: Vec<u8>, ui: Vec<u8>, source: &str) -> Self {
        let mut fcx = FontContext::new();
        let mono_family = register(&mut fcx, mono);
        let ui_family = register(&mut fcx, ui);
        let mut shaper = Shaper { fcx, lcx: LayoutContext::new() };
        let lines: Vec<&str> = source.lines().collect();
        let code = lines
            .iter()
            .map(|l| {
                let text = l.replace('\t', "    ");
                let ranges = token_ranges(&text);
                shaper.line(&text, &mono_family, &ranges)
            })
            .collect();
        let numbers = (1..=lines.len())
            .map(|n| shaper.line(&format!("{n:>5}"), &mono_family, &[]))
            .collect();
        let tree = (0..60)
            .map(|i| {
                let name = format!("{}pieceTreeTextBuffer{i}.ts", "  ".repeat(i % 4));
                shaper.line(&name, &ui_family, &[])
            })
            .collect();
        let minimap = lines
            .iter()
            .map(|l| {
                token_ranges(&l.replace('\t', "    "))
                    .into_iter()
                    .filter(|(_, c)| *c != FOREGROUND)
                    .map(|(r, c)| (r.start as f32, r.len() as f32, c))
                    .collect()
            })
            .collect();
        Self { code, numbers, tree, minimap }
    }

    pub fn glyph_count_per_frame(&self) -> usize {
        let visible = (HEIGHT as f32 / LINE_HEIGHT) as usize + 1;
        let code: usize = self.code.iter().take(visible).flat_map(|l| &l.runs).map(|r| r.glyphs.len()).sum();
        let tree: usize = self.tree.iter().flat_map(|l| &l.runs).map(|r| r.glyphs.len()).sum();
        code + tree
    }

    /// Pinta o frame `frame` (rolagem de um pixel por frame).
    pub fn paint(&self, painter: &mut impl Painter, frame: u64) {
        let (w, h) = (WIDTH as f32, HEIGHT as f32);
        painter.rect(0.0, 0.0, w, h, BACKGROUND);
        painter.rect(0.0, 0.0, ACTIVITY_BAR, h - STATUS_BAR, SIDEBAR_BG);
        painter.rect(ACTIVITY_BAR, 0.0, SIDEBAR, h - STATUS_BAR, SIDEBAR_BG);
        painter.rect(ACTIVITY_BAR + SIDEBAR, 0.0, 1.0, h - STATUS_BAR, BORDER);
        for i in 0..8 {
            painter.rect(12.0, 12.0 + 48.0 * i as f32, 24.0, 24.0, LINE_NUMBER);
        }
        for (i, line) in self.tree.iter().enumerate() {
            let y = 40.0 + 22.0 * i as f32;
            if y > h - STATUS_BAR {
                break;
            }
            for run in &line.runs {
                painter.glyphs(run, ACTIVITY_BAR + 20.0, y);
            }
        }
        let editor_x = ACTIVITY_BAR + SIDEBAR + 1.0;
        painter.rect(editor_x, 0.0, w - editor_x, TAB_BAR, SIDEBAR_BG);
        for t in 0..6 {
            let x = editor_x + 160.0 * t as f32;
            painter.rect(x, 0.0, 159.0, TAB_BAR, if t == 0 { TAB_ACTIVE } else { SIDEBAR_BG });
            painter.rect(x + 159.0, 0.0, 1.0, TAB_BAR, BORDER);
        }
        let top = TAB_BAR;
        let viewport = h - STATUS_BAR - top;
        let total = self.code.len() as f32 * LINE_HEIGHT;
        let scroll = (frame as f32) % (total - viewport).max(1.0);
        let first = (scroll / LINE_HEIGHT) as usize;
        let offset = scroll - first as f32 * LINE_HEIGHT;
        let visible = (viewport / LINE_HEIGHT) as usize + 2;
        let text_x = editor_x + GUTTER;
        let baseline = FONT_SIZE;
        for row in 0..visible {
            let index = first + row;
            let (Some(code), Some(number)) = (self.code.get(index), self.numbers.get(index)) else {
                break;
            };
            let y = top + row as f32 * LINE_HEIGHT - offset + (LINE_HEIGHT - baseline) / 2.0;
            for run in &number.runs {
                let mut run_color = Run { font: run.font.clone(), size: run.size, color: LINE_NUMBER, glyphs: Vec::new() };
                run_color.glyphs.clone_from(&run.glyphs);
                painter.glyphs(&run_color, editor_x + 8.0, y);
            }
            for run in &code.runs {
                painter.glyphs(run, text_x, y);
            }
        }
        let minimap_x = w - MINIMAP - 14.0;
        painter.rect(minimap_x, top, MINIMAP, viewport, BACKGROUND);
        let minimap_first = (scroll / 2.0) as usize;
        for (row, tokens) in self.minimap.iter().skip(minimap_first / 2).take((viewport / 2.0) as usize).enumerate() {
            for (column, len, color) in tokens {
                painter.rect(minimap_x + column, top + 2.0 * row as f32, *len, 2.0, *color);
            }
        }
        painter.rect(minimap_x, top, MINIMAP, viewport * 0.08, [0x79, 0x79, 0x79, 0x33]);
        painter.rect(0.0, h - STATUS_BAR, w, STATUS_BAR, STATUS_BG);
        painter.rect(0.0, h - STATUS_BAR, w, 1.0, BORDER);
    }
}

fn register(fcx: &mut FontContext, data: Vec<u8>) -> String {
    let families = fcx.collection.register_fonts(Blob::new(Arc::new(data)), None);
    let id = families.first().expect("fonte sem família").0;
    fcx.collection.family_name(id).expect("família sem nome").to_owned()
}
