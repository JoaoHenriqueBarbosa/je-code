//! Texto do je-code (H09, H10 e H11 em `docs/bench-report.md`).
//!
//! - Fontes: as do sistema pelo `fontique` (os mesmos caminhos do Chromium: fontconfig, DirectWrite,
//!   CoreText), mais o `codicon.ttf` registrado sob a família `codicon`, como o `@font-face` do VS Code.
//! - Shaping e quebra: `parley`.
//! - Métrica: a largura de um trecho é arredondada para cima em 1/64 px, que é o que o LayoutNG do
//!   Chromium faz (`LayoutUnit::FromFloatCeil`); com isso a largura bate com a do oráculo (H10).
//! - Cache: o layout de um trecho (texto, família, tamanho, peso) é feito uma vez e devolvido como
//!   glifos compartilhados (`Arc<[Glyph]>`) prontos para a lista de exibição.
//! - Parágrafo: [`TextSystem::paragraph`] dispõe vários trechos com estilos diferentes (negrito, itálico, código,
//!   link) como um texto só, quebrado numa largura máxima pelo `break_all_lines` do parley (as opções de quebra
//!   do UAX #14, como o `white-space: normal` do Chromium). Devolve as linhas com a caixa de cada trecho
//!   ([`ParagraphPiece`]), para pintar e para acertar clique em link. Um trecho com `inset` (o `code` com
//!   `padding` e borda) entra no texto como caixa em linha (`InlineBox`) com a largura do texto mais os recuos, e
//!   não quebra por dentro; o texto dele é desenhado pelo chamador dentro da caixa. A altura da linha (o
//!   `line-height` do CSS) é do chamador: o parágrafo devolve só a linha de base e a altura natural de cada
//!   linha, como [`TextLine`].

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

mod woff;
pub use woff::woff_to_sfnt;

/// O ponto de código de um codicon pelo id (`files`), para quem monta um trecho de parágrafo com ícone.
pub use je_codicons::code_point as codicon_code_point;
use je_render::{DisplayList, FontData, Glyph, Rgba};
use parley::fontique::Blob;
use parley::{FontContext, FontStyle, FontWeight, InlineBox, InlineBoxKind, LayoutContext, PositionedLayoutItem, StyleProperty, VerticalAlign};

/// A família sob a qual o `codicon.ttf` é registrado (o `font-family: "codicon"` do `codicon.css`).
pub const CODICON_FAMILY: &str = "codicon";

/// Arredonda para cima em 1/64 px (`LayoutUnit::FromFloatCeil`).
pub fn layout_unit_ceil(px: f32) -> f32 {
    (px * 64.0).ceil() / 64.0
}

/// O limite de linhas no cache de layout, para o texto de um arquivo enorme não crescer sem fim.
const MAX_CACHED_LINES: usize = 100_000;

/// O limite de parágrafos no cache (o texto de uma tela de descrições cabe com folga).
const MAX_CACHED_PARAGRAPHS: usize = 4_096;

/// Uma fonte de texto pedida: a lista de famílias no formato CSS do VS Code
/// (`system-ui, "Ubuntu", "Droid Sans", sans-serif`), o tamanho em px e o peso.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct TextStyle {
    pub families: String,
    /// Tamanho em 1/64 px, para o estilo poder ser chave de cache.
    pub size_64: u32,
    pub weight: u16,
    /// `font-style: italic` (estilo de token do tema, por exemplo comentários em itálico).
    pub italic: bool,
}

impl TextStyle {
    pub fn new(families: impl Into<String>, size_px: f32, weight: u16) -> Self {
        Self { families: families.into(), size_64: (size_px * 64.0).round() as u32, weight, italic: false }
    }

    /// O mesmo estilo em itálico.
    pub fn italic(mut self, italic: bool) -> Self {
        self.italic = italic;
        self
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

/// Um trecho de um parágrafo: um texto com um estilo de fonte. O trecho com `inset_64` é atômico: vira uma caixa
/// em linha que não quebra, com esse recuo (em 1/64 px) de cada lado do texto (o `padding` mais a borda do `code`).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ParagraphSpan {
    pub text: String,
    pub style: TextStyle,
    pub inset_64: Option<u32>,
}

impl ParagraphSpan {
    /// Um trecho que quebra livremente.
    pub fn text(text: impl Into<String>, style: TextStyle) -> Self {
        Self { text: text.into(), style, inset_64: None }
    }

    /// Um trecho atômico com o recuo horizontal `inset` px de cada lado.
    pub fn boxed(text: impl Into<String>, style: TextStyle, inset: f32) -> Self {
        Self { text: text.into(), style, inset_64: Some((inset * 64.0).round() as u32) }
    }
}

/// A parte de um trecho que cabe numa linha: os glifos e o intervalo horizontal que ocupam.
#[derive(Clone, Debug)]
pub struct ParagraphPiece {
    /// O índice do trecho em `spans`.
    pub span: usize,
    /// O início e a largura da peça na linha (para o clique em link e para a moldura de um trecho atômico).
    pub x: f32,
    pub width: f32,
    /// O deslocamento dos glifos em relação ao início da linha: 0 num trecho de texto (os glifos já vêm em
    /// coordenadas da linha); no atômico, o início da caixa mais o recuo.
    pub dx: f32,
    pub runs: Vec<Run>,
    /// Num trecho atômico, a altura da fonte acima e abaixo da linha de base (a área de conteúdo, onde o
    /// navegador desenha o fundo e a borda do `code`).
    pub ascent: f32,
    pub descent: f32,
    pub boxed: bool,
}

impl ParagraphPiece {
    /// Pinta os glifos, com a linha começando em `origin_x` e a linha de base em `baseline_y`.
    pub fn paint(&self, list: &mut DisplayList, origin_x: f32, baseline_y: f32, color: Rgba) {
        for run in &self.runs {
            list.glyphs(&run.font, run.size, color, origin_x + self.dx, baseline_y, &run.glyphs);
        }
    }
}

/// Uma linha de um parágrafo.
#[derive(Clone, Debug)]
pub struct ParagraphLine {
    pub pieces: Vec<ParagraphPiece>,
    /// A largura do conteúdo da linha, sem o espaço pendurado no fim.
    pub width: f32,
    /// A linha de base medida a partir do topo da caixa de linha natural.
    pub baseline: f32,
    /// A altura natural da caixa de linha (a maior entre as fontes da linha).
    pub line_height: f32,
}

/// O parágrafo disposto: as linhas, de cima para baixo.
#[derive(Clone, Debug, Default)]
pub struct ParagraphLayout {
    pub lines: Vec<ParagraphLine>,
    /// A largura da maior linha.
    pub width: f32,
}

/// O sistema de texto: contextos do parley e os caches de linhas e de parágrafos.
pub struct TextSystem {
    fcx: FontContext,
    lcx: LayoutContext<()>,
    /// O contexto dos parágrafos: o `brush` de cada trecho é o índice dele em `spans`, para os glifos
    /// dizerem de que trecho vêm.
    pcx: LayoutContext<u32>,
    cache: HashMap<(String, TextStyle), Rc<TextLine>>,
    paragraphs: HashMap<(Vec<ParagraphSpan>, Option<u32>), Rc<ParagraphLayout>>,
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
        Self { fcx, lcx: LayoutContext::new(), pcx: LayoutContext::new(), cache: HashMap::new(), paragraphs: HashMap::new() }
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
        // O cache não tem despejo fino: passou do limite, recomeça (o editor repõe só o que está visível).
        if self.cache.len() >= MAX_CACHED_LINES {
            self.cache.clear();
        }
        self.cache.insert(key, line.clone());
        line
    }

    /// O glifo de um codicon pelo id (`$(files)`), no tamanho pedido.
    pub fn codicon(&mut self, id: &str, size_px: f32) -> Option<Rc<TextLine>> {
        let ch = je_codicons::code_point(id)?;
        Some(self.line(&ch.to_string(), &TextStyle::new(CODICON_FAMILY, size_px, 400)))
    }

    /// O parágrafo formado por `spans`, quebrado em `max_width` px (`None`: sem quebra, só nas quebras de linha
    /// do texto), do cache quando possível. Um `\n` num trecho de texto é quebra de linha obrigatória; o
    /// espaço em branco não é colapsado aqui (quem monta os trechos aplica o `white-space: normal`).
    pub fn paragraph(&mut self, spans: &[ParagraphSpan], max_width: Option<f32>) -> Rc<ParagraphLayout> {
        // A largura entra na chave em 1/64 px, que é a precisão do layout.
        let width_64 = max_width.map(|w| (w.max(0.0) * 64.0).round() as u32);
        let key = (spans.to_vec(), width_64);
        if let Some(layout) = self.paragraphs.get(&key) {
            return layout.clone();
        }
        let layout = Rc::new(self.shape_paragraph(spans, width_64.map(|w| w as f32 / 64.0)));
        if self.paragraphs.len() >= MAX_CACHED_PARAGRAPHS {
            self.paragraphs.clear();
        }
        self.paragraphs.insert(key, layout.clone());
        layout
    }

    fn shape_paragraph(&mut self, spans: &[ParagraphSpan], max_width: Option<f32>) -> ParagraphLayout {
        let Some(first) = spans.first() else { return ParagraphLayout::default() };
        // O texto do layout é o dos trechos que quebram; cada trecho atômico entra como caixa no ponto onde está.
        let mut text = String::new();
        let mut ranges = Vec::with_capacity(spans.len());
        let mut boxes = Vec::new();
        for (index, span) in spans.iter().enumerate() {
            match span.inset_64 {
                None => {
                    let start = text.len();
                    text.push_str(&span.text);
                    ranges.push(start..text.len());
                }
                Some(inset_64) => {
                    let shaped = self.line(&span.text, &span.style);
                    boxes.push((index, text.len(), shaped, inset_64 as f32 / 64.0));
                    ranges.push(text.len()..text.len());
                }
            }
        }
        // Um parágrafo só de caixas não tem texto para o parley formar linha: um espaço de largura zero (sem
        // glifo) depois das caixas dá a linha onde elas ficam.
        if text.is_empty() && !boxes.is_empty() {
            text.push('\u{200B}');
        }
        let mut builder = self.pcx.ranged_builder(&mut self.fcx, &text, 1.0, true);
        builder.push_default(StyleProperty::FontFamily(first.style.families.as_str().into()));
        builder.push_default(StyleProperty::FontSize(first.style.size()));
        for (index, (span, range)) in spans.iter().zip(&ranges).enumerate() {
            if range.is_empty() {
                continue;
            }
            builder.push(StyleProperty::FontFamily(span.style.families.as_str().into()), range.clone());
            builder.push(StyleProperty::FontSize(span.style.size()), range.clone());
            builder.push(StyleProperty::FontWeight(FontWeight::new(f32::from(span.style.weight))), range.clone());
            builder.push(StyleProperty::FontStyle(if span.style.italic { FontStyle::Italic } else { FontStyle::Normal }), range.clone());
            builder.push(StyleProperty::Brush(index as u32), range.clone());
        }
        for (index, at, shaped, inset) in &boxes {
            builder.push_inline_box(InlineBox {
                id: *index as u64,
                kind: InlineBoxKind::InFlow,
                index: *at,
                width: shaped.width + 2.0 * inset,
                // Sem altura: a caixa fica sobre a linha de base e não mexe na altura da linha.
                height: 0.0,
                baseline: None,
                vertical_align: VerticalAlign::BASELINE,
            });
        }
        let mut layout = builder.build(&text);
        layout.break_all_lines(max_width);

        let mut lines = Vec::new();
        let mut width = 0.0f32;
        for line in layout.lines() {
            let m = line.metrics();
            let mut pieces = Vec::new();
            for item in line.items() {
                match item {
                    PositionedLayoutItem::GlyphRun(glyph_run) => {
                        let run = glyph_run.run();
                        let glyphs: Vec<Glyph> = glyph_run.positioned_glyphs().map(|g| Glyph { id: g.id, x: g.x, y: g.y - m.baseline }).collect();
                        let span = glyph_run.style().brush as usize;
                        pieces.push(ParagraphPiece {
                            span,
                            x: glyph_run.offset(),
                            width: glyph_run.advance(),
                            dx: 0.0,
                            runs: vec![Run { font: run.font().clone(), size: run.font_size(), glyphs: glyphs.into() }],
                            ascent: 0.0,
                            descent: 0.0,
                            boxed: false,
                        });
                    }
                    PositionedLayoutItem::InlineBox(placed) => {
                        let span = placed.id as usize;
                        if let Some((_, _, shaped, inset)) = boxes.iter().find(|(index, ..)| *index == span) {
                            pieces.push(ParagraphPiece {
                                span,
                                x: placed.x,
                                width: placed.width,
                                dx: placed.x + inset,
                                runs: shaped.runs.clone(),
                                ascent: shaped.baseline,
                                descent: shaped.line_height - shaped.baseline,
                                boxed: true,
                            });
                        }
                    }
                }
            }
            let line_width = layout_unit_ceil(m.advance - m.hanging_advance);
            width = width.max(line_width);
            lines.push(ParagraphLine { pieces, width: line_width, baseline: m.baseline, line_height: m.line_height });
        }
        ParagraphLayout { lines, width }
    }

    fn shape(&mut self, text: &str, style: &TextStyle) -> TextLine {
        let mut builder = self.lcx.ranged_builder(&mut self.fcx, text, 1.0, true);
        builder.push_default(StyleProperty::FontFamily(style.families.as_str().into()));
        builder.push_default(StyleProperty::FontSize(style.size()));
        builder.push_default(StyleProperty::FontWeight(FontWeight::new(f32::from(style.weight))));
        if style.italic {
            builder.push_default(StyleProperty::FontStyle(FontStyle::Italic));
        }
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
