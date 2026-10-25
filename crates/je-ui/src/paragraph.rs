//! O parágrafo: um texto com quebra de linha feita pelo motor de texto, em trechos de estilos diferentes.
//!
//! No DOM, um parágrafo é um elemento com filhos em linha (`strong`, `em`, `code`, `a`) e o navegador quebra as
//! linhas na largura do elemento. Aqui o [`Paragraph`] é o conteúdo ([`crate::Content::Paragraph`]) de um nó só: a
//! lista de [`Span`] (texto, fonte, cor, destino de link e moldura opcional), a altura de linha (o `line-height`)
//! e se quebra. A largura máxima é a da caixa do nó; as linhas saem de [`je_text::TextSystem::paragraph`]
//! (parley, com o cache de parágrafos), a altura é a quantidade de linhas vezes o `line-height`, e a pintura e o
//! hit test de link leem as caixas de cada trecho nas linhas.
//!
//! O `textContent` do nó é o texto dos trechos de texto concatenados, como o do elemento no DOM: o ícone
//! (`span.codicon`, sem texto) e a quebra (`br`) não entram.
//!
//! Cada link do parágrafo (o `a` do DOM) tem identidade própria na árvore sem voltar ao nó por palavra: o
//! [`crate::Ui::sync_paragraph_links`] cria um filho `Role::Link` por link, sem caixa de layout (retângulo vazio, sem
//! ponteiro), com o nome acessível do [`Paragraph::link_name`]. As caixas do link vêm da disposição do parágrafo
//! ([`Paragraph::link_boxes`], [`crate::Tree::link_boxes`]); o filho entra na ordem de Tab
//! ([`crate::Tree::tab_order`]), o `Enter` nele dispara um `Click` com ele por alvo, e o contorno de foco é
//! pintado sobre essas caixas com a [`Paragraph::focus_color`].

use std::rc::Rc;

use je_render::Rgba;
use je_render::kurbo::Rect;
use je_text::{ParagraphLayout, ParagraphSpan, TextStyle, with_text_system};

use crate::tree::{Border, half_leading};

/// O que um trecho é para o `textContent`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpanKind {
    /// Texto: entra no `textContent`.
    Text,
    /// O glifo de um codicon (o texto é o ponto de código): não entra no `textContent`.
    Icon,
    /// A quebra de linha obrigatória (`br`): o texto é `\n` e não entra no `textContent`.
    LineBreak,
}

/// A moldura de um trecho em linha (o `padding`, a borda e o fundo do `code`). O trecho com moldura não quebra por
/// dentro.
#[derive(Clone, Debug, PartialEq)]
pub struct SpanFrame {
    /// O recuo horizontal de cada lado do texto (`padding-inline` mais a borda).
    pub inset_x: f64,
    /// O quanto a moldura passa da área de conteúdo da fonte, acima e abaixo (`padding-block` mais a borda).
    pub inset_y: f64,
    pub background: Option<Rgba>,
    pub border: Option<Border>,
    pub radius: f64,
}

/// Um trecho do parágrafo.
#[derive(Clone, Debug, PartialEq)]
pub struct Span {
    pub text: String,
    pub style: TextStyle,
    pub color: Rgba,
    /// O destino de link: o número que o dono do nó deu ao link (o clique devolve o mesmo número).
    pub link: Option<u32>,
    pub frame: Option<SpanFrame>,
    pub kind: SpanKind,
}

impl Span {
    pub fn text(text: impl Into<String>, style: TextStyle, color: Rgba) -> Self {
        Self { text: text.into(), style, color, link: None, frame: None, kind: SpanKind::Text }
    }

    /// O glifo de um codicon (`text` é o ponto de código, na família do `codicon`).
    pub fn icon(glyph: char, style: TextStyle, color: Rgba) -> Self {
        Self { kind: SpanKind::Icon, ..Self::text(glyph, style, color) }
    }

    /// A quebra de linha obrigatória.
    pub fn line_break(style: TextStyle, color: Rgba) -> Self {
        Self { kind: SpanKind::LineBreak, ..Self::text("\n", style, color) }
    }

    pub fn with_link(mut self, link: Option<u32>) -> Self {
        self.link = link;
        self
    }

    pub fn with_frame(mut self, frame: SpanFrame) -> Self {
        self.frame = Some(frame);
        self
    }
}

/// O conteúdo de parágrafo de um nó.
#[derive(Clone, Debug, PartialEq)]
pub struct Paragraph {
    pub spans: Vec<Span>,
    /// O `line-height` em px: a altura de cada linha.
    pub line_height: f64,
    /// `white-space: normal` quando verdadeiro (quebra na largura do nó); `pre` quando falso (só nas quebras do texto).
    pub wrap: bool,
    /// O link sob o ponteiro, que pinta com [`Paragraph::link_hover_color`].
    pub hover_link: Option<u32>,
    pub link_hover_color: Option<Rgba>,
    /// A cor do contorno de foco (`outline: 1px solid`, `outline-offset: -1px`) do link que tem o foco do teclado;
    /// sem cor, o foco não desenha nada.
    pub focus_color: Option<Rgba>,
}

impl Paragraph {
    pub fn new(spans: Vec<Span>, line_height: f64) -> Self {
        Self { spans, line_height, wrap: true, hover_link: None, link_hover_color: None, focus_color: None }
    }

    /// Os números dos links do parágrafo, na ordem em que aparecem no texto (sem repetir).
    pub fn link_numbers(&self) -> Vec<u32> {
        let mut numbers = Vec::new();
        for link in self.spans.iter().filter_map(|span| span.link) {
            if !numbers.contains(&link) {
                numbers.push(link);
            }
        }
        numbers
    }

    /// O nome acessível do link: o texto dos trechos de texto dele, concatenado e sem espaço nas pontas (o ícone e
    /// a quebra não entram).
    pub fn link_name(&self, link: u32) -> String {
        let text: String = self.spans.iter().filter(|span| span.link == Some(link) && span.kind == SpanKind::Text).map(|span| span.text.as_str()).collect();
        text.trim().to_owned()
    }

    /// As caixas do link na largura dada, relativas ao canto superior esquerdo do parágrafo: uma por sequência de
    /// peças contíguas do link em cada linha, na altura da área de conteúdo da fonte (do `ascent` ao `descent`
    /// em torno da linha de base), que é onde o navegador desenha o contorno de um `a` em linha.
    pub fn link_boxes(&self, width: f64, link: u32) -> Vec<Rect> {
        let layout = self.layout(width);
        let mut boxes = Vec::new();
        for (index, line) in layout.lines.iter().enumerate() {
            let top = index as f64 * self.line_height + half_leading(self.line_height, f64::from(line.line_height));
            let baseline = top + f64::from(line.baseline);
            let mut run: Option<Rect> = None;
            for piece in &line.pieces {
                if self.spans.get(piece.span).is_none_or(|span| span.link != Some(link)) {
                    boxes.extend(run.take());
                    continue;
                }
                let piece_box = Rect::new(f64::from(piece.x), baseline - f64::from(piece.ascent), f64::from(piece.x + piece.width), baseline + f64::from(piece.descent));
                run = Some(run.map_or(piece_box, |current| current.union(piece_box)));
            }
            boxes.extend(run);
        }
        boxes
    }

    /// O `textContent`: o texto dos trechos de texto, concatenado.
    pub fn text_content(&self) -> String {
        self.spans.iter().filter(|span| span.kind == SpanKind::Text).map(|span| span.text.as_str()).collect()
    }

    /// As linhas do parágrafo na largura dada.
    pub fn layout(&self, width: f64) -> Rc<ParagraphLayout> {
        let spans: Vec<ParagraphSpan> = self
            .spans
            .iter()
            .map(|span| match &span.frame {
                Some(frame) => ParagraphSpan::boxed(span.text.as_str(), span.style.clone(), frame.inset_x as f32),
                None => ParagraphSpan::text(span.text.as_str(), span.style.clone()),
            })
            .collect();
        with_text_system(|t| t.paragraph(&spans, self.wrap.then_some(width as f32)))
    }

    /// A altura na largura dada: a quantidade de linhas vezes o `line-height`.
    pub fn height(&self, width: f64) -> f64 {
        self.layout(width).lines.len() as f64 * self.line_height
    }

    /// O link no ponto (`x`, `y`), relativo ao canto superior esquerdo do parágrafo de largura `width`.
    pub fn link_at(&self, width: f64, x: f64, y: f64) -> Option<u32> {
        if self.line_height <= 0.0 || y < 0.0 {
            return None;
        }
        let layout = self.layout(width);
        let line = layout.lines.get((y / self.line_height).floor() as usize)?;
        let piece = line.pieces.iter().find(|piece| x >= f64::from(piece.x) && x < f64::from(piece.x + piece.width))?;
        self.spans.get(piece.span)?.link
    }

    /// A cor com que o trecho pinta agora: a do link sob o ponteiro, ou a própria.
    pub(crate) fn color_of(&self, span: &Span) -> Rgba {
        match (span.link, self.hover_link, self.link_hover_color) {
            (Some(link), Some(hover), Some(color)) if link == hover => color,
            _ => span.color,
        }
    }

    /// O mesmo parágrafo com tudo multiplicado por `scale` (pintura em pixels de dispositivo).
    pub(crate) fn scaled(&self, scale: f64) -> Self {
        let mut scaled = self.clone();
        scaled.line_height *= scale;
        for span in &mut scaled.spans {
            span.style.size_64 = (f64::from(span.style.size_64) * scale).round() as u32;
            if let Some(frame) = &mut span.frame {
                frame.inset_x *= scale;
                frame.inset_y *= scale;
                frame.radius *= scale;
                if let Some(border) = &mut frame.border {
                    border.width *= scale;
                }
            }
        }
        scaled
    }
}
